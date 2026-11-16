//! Files changed on disk outside the editor, told to the running language servers with
//! `workspace/didChangeWatchedFiles` (`SPEC-0042` v11 behaviour 20).
//!
//! This module holds what needs no server: the net effect per path, the wait before a flush, the
//! folders that are never told, the check that a path stays inside the project, which server cares
//! about which file, the notification and the look at the disk. Sending is the pool's
//! ([`crate::pool::disk_changed`]).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use idecraft_core::{EXCLUDED_DIRS, FileChange, Snapshot, Workspace, diff};
use serde_json::{Value, json};

use crate::LanguageId;
use crate::lsp::file_uri;

/// A flush waits until nothing new came for this long …
pub const QUIET: Duration = Duration::from_millis(400);
/// … but at most this long after the first collected change.
pub const LONGEST_WAIT: Duration = Duration::from_secs(2);
/// Changes in one notification.
pub const MAX_PER_NOTIFICATION: usize = 500;
/// Pause between the notifications of one flush.
pub const PAUSE: Duration = Duration::from_millis(50);
/// With more changes for one server in a flush, the server is stopped instead and reads the disk
/// afresh when it is started again.
pub const RESTART_ABOVE: usize = 10_000;

/// What happened to a file; the numbers are the protocol's change types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The file is new.
    Created = 1,
    /// Its content changed.
    Changed = 2,
    /// It is gone.
    Deleted = 3,
}

/// The net effect of `now` after `before` on one path; `None`: nothing is left to tell.
pub fn merge(before: Option<Kind>, now: Kind) -> Option<Kind> {
    use Kind::{Changed, Created, Deleted};
    match (before, now) {
        (None, k) => Some(k),
        (Some(Created), Deleted) => None,
        (Some(Created), _) => Some(Created),
        (Some(Changed), Deleted) => Some(Deleted),
        (Some(Changed), _) => Some(Changed),
        (Some(Deleted), Deleted) => Some(Deleted),
        // It was there before and is there again, possibly with other content.
        (Some(Deleted), _) => Some(Changed),
    }
}

/// The collected changes of one project, per path only the net effect.
#[derive(Debug, Default)]
pub struct Pending {
    changes: BTreeMap<PathBuf, Kind>,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Pending {
    /// Adds a change seen at `now`.
    pub fn add(&mut self, path: PathBuf, kind: Kind, now: Instant) {
        if self.changes.is_empty() {
            self.first = Some(now);
        }
        self.last = Some(now);
        match merge(self.changes.get(&path).copied(), kind) {
            Some(k) => {
                self.changes.insert(path, k);
            }
            None => {
                self.changes.remove(&path);
            }
        }
    }

    /// Whether nothing is left to tell.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// When the collected changes are to be sent: [`QUIET`] after the last one, at the latest
    /// [`LONGEST_WAIT`] after the first. `None` with nothing collected.
    pub fn due(&self) -> Option<Instant> {
        if self.changes.is_empty() {
            return None;
        }
        Some((self.last? + QUIET).min(self.first? + LONGEST_WAIT))
    }

    /// Takes the collected changes, in path order.
    pub fn take(&mut self) -> Vec<(PathBuf, Kind)> {
        self.first = None;
        self.last = None;
        std::mem::take(&mut self.changes).into_iter().collect()
    }
}

/// Folders never told wherever they lie, beyond the workspace's own ([`EXCLUDED_DIRS`]).
const NEVER: &[&str] = &[".idea", ".gradle", ".venv", "venv"];
/// Build output, unless a folder named `src` lies above (a package may be called `build`).
const OUTPUT: &[&str] = &["build", "out"];

/// Whether the folder at `folder` (relative to the project) is excluded by its name.
pub fn folder_excluded(folder: &Path) -> bool {
    let Some(name) = folder.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if EXCLUDED_DIRS.contains(&name) || NEVER.contains(&name) {
        return true;
    }
    OUTPUT.contains(&name)
        && !folder
            .parent()
            .is_some_and(|above| above.components().any(|c| c.as_os_str() == "src"))
}

/// [`folder_excluded`], or a virtual environment (it holds a `pyvenv.cfg`).
fn folder_skipped(root: &Path, folder: &Path) -> bool {
    folder_excluded(folder) || root.join(folder).join("pyvenv.cfg").is_file()
}

/// Whether the file at `file` (relative to `root`) is told at all: none of its folders is
/// excluded.
pub fn told(root: &Path, file: &Path) -> bool {
    file.ancestors()
        .skip(1)
        .filter(|a| !a.as_os_str().is_empty())
        .all(|folder| !folder_skipped(root, folder))
}

/// `path` relative to `root` when it lies inside the project and no part of it is a symbolic
/// link; `path` may be relative to `root` already. Links are not followed: a linked folder could
/// lead out of the project.
pub fn inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let mut at = root.to_path_buf();
    for part in relative.components() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => return None,
            Ok(_) => {}
            // Gone (a deleted file): nothing below exists either.
            Err(_) => break,
        }
    }
    Some(relative.to_path_buf())
}

/// The languages whose servers read the build file named `name`.
fn build_file_languages(name: &str) -> &'static [LanguageId] {
    use LanguageId::{Go, Groovy, Java, JavaScript, Kotlin, Python, Rust, Scala, Svelte, Vue};
    match name {
        "pom.xml"
        | "build.gradle"
        | "build.gradle.kts"
        | "settings.gradle"
        | "settings.gradle.kts"
        | "gradle.properties" => &[Java, Kotlin, Groovy, Scala],
        "package.json" | "tsconfig.json" | "jsconfig.json" => {
            &[LanguageId::TypeScript, JavaScript, Vue, Svelte]
        }
        "Cargo.toml" | "Cargo.lock" => &[Rust],
        "pyproject.toml" | "requirements.txt" | "setup.cfg" => &[Python],
        "go.mod" | "go.sum" | "go.work" => &[Go],
        _ => &[],
    }
}

/// Whether the server started as `program` is told about `file`: a file of a language it serves,
/// or a build file of its ecosystem.
pub fn interested(program: &str, file: &Path) -> bool {
    let serves = |language: LanguageId| {
        crate::servers::candidates(language)
            .iter()
            .any(|c| c.program == program)
    };
    if LanguageId::detect(file).is_some_and(serves) {
        return true;
    }
    file.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| build_file_languages(name).iter().any(|l| serves(*l)))
}

/// The parameters of the `workspace/didChangeWatchedFiles` notifications for `changes` (paths
/// relative to `root`, the server's workspace folder): at most [`MAX_PER_NOTIFICATION`] changes each, in the given order.
pub fn notifications(root: &Path, changes: &[(PathBuf, Kind)]) -> Vec<Value> {
    changes
        .chunks(MAX_PER_NOTIFICATION)
        .map(|chunk| {
            let list: Vec<Value> = chunk
                .iter()
                .map(|(path, kind)| json!({"uri": file_uri(&root.join(path)), "type": *kind as u8}))
                .collect();
            json!({ "changes": list })
        })
        .collect()
}

/// Snapshot changes as kinds; a rename is deleted plus created.
pub fn kinds(changes: &[FileChange]) -> Vec<(PathBuf, Kind)> {
    let mut out = Vec::with_capacity(changes.len());
    for change in changes {
        match change {
            FileChange::Created(p) => out.push((p.clone(), Kind::Created)),
            FileChange::Modified(p) => out.push((p.clone(), Kind::Changed)),
            FileChange::Deleted(p) => out.push((p.clone(), Kind::Deleted)),
            FileChange::Renamed { from, to } => {
                out.push((from.clone(), Kind::Deleted));
                out.push((to.clone(), Kind::Created));
            }
        }
    }
    out
}

/// Looks at a project's files from time to time and says what changed between two looks (the
/// snapshot comparison of `SPEC-0026`, without the folders that are never told).
#[derive(Debug, Default)]
pub struct DiskWatch {
    before: Option<Snapshot>,
}

impl DiskWatch {
    /// Looks at the disk below `root`: the changes since the previous look. The first look is the
    /// baseline and reports nothing; a look that fails keeps the previous one.
    pub fn look(&mut self, root: &Path) -> Vec<(PathBuf, Kind)> {
        let now = Workspace::open(root)
            .and_then(|w| w.snapshot_without(&|folder| folder_skipped(w.root(), folder)));
        let Ok(now) = now else {
            return Vec::new();
        };
        let changes = self
            .before
            .as_ref()
            .map(|before| kinds(&diff(before, &now)))
            .unwrap_or_default();
        self.before = Some(now);
        changes
    }

    /// One round for a project: while a language server of `root` runs or starts, looks at the
    /// disk and hands the changes to the pool; without one it forgets what it saw, so the next
    /// server starts from a fresh baseline. Reads the disk — call it off the UI thread. Returns
    /// whether a Java, Kotlin or JVM build file was created, changed or deleted: then the source
    /// folders derived from the files may differ (`SPEC-0048` v6 behaviour 14).
    pub fn round(&mut self, root: &Path) -> bool {
        if !crate::pool::watches(root) {
            self.before = None;
            return false;
        }
        let changes = self.look(root);
        let sources = changes.iter().any(|(path, _)| {
            idecraft_core::source_roots::is_jvm_source(path)
                || idecraft_core::source_roots::is_jvm_build_file(path)
        });
        if !changes.is_empty() {
            crate::pool::disk_changed(root, changes);
        }
        sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-watched-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn write(root: &Path, file: &str, text: &str) {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    // SPEC-0042/AK-16 (v11).
    #[test]
    fn per_path_only_the_net_effect_remains() {
        use Kind::{Changed, Created, Deleted};
        let now = Instant::now();
        let mut p = Pending::default();
        p.add("gone.java".into(), Created, now);
        p.add("gone.java".into(), Changed, now);
        p.add("gone.java".into(), Deleted, now);
        assert!(p.is_empty() && p.due().is_none(), "created then deleted");
        p.add("new.java".into(), Created, now);
        p.add("new.java".into(), Changed, now);
        p.add("old.java".into(), Changed, now);
        p.add("old.java".into(), Changed, now);
        p.add("old.java".into(), Deleted, now);
        p.add("back.java".into(), Deleted, now);
        p.add("back.java".into(), Created, now);
        p.add("edit.java".into(), Changed, now);
        p.add("edit.java".into(), Changed, now);
        assert_eq!(
            p.take(),
            [
                (PathBuf::from("back.java"), Changed),
                (PathBuf::from("edit.java"), Changed),
                (PathBuf::from("new.java"), Created),
                (PathBuf::from("old.java"), Deleted),
            ]
        );
        assert!(p.is_empty() && p.take().is_empty());
        assert_eq!(merge(Some(Deleted), Deleted), Some(Deleted));
        assert_eq!(merge(Some(Changed), Created), Some(Changed));
    }

    #[test]
    fn the_flush_waits_for_quiet_but_not_for_ever() {
        let start = Instant::now();
        let mut p = Pending::default();
        assert_eq!(p.due(), None);
        p.add("a.java".into(), Kind::Changed, start);
        assert_eq!(p.due(), Some(start + QUIET));
        // Changes keep coming every 300 ms: the quiet time moves, the longest wait does not.
        let mut at = start;
        for _ in 0..10 {
            at += Duration::from_millis(300);
            p.add("a.java".into(), Kind::Changed, at);
        }
        assert_eq!(p.due(), Some(start + LONGEST_WAIT));
        p.take();
        // The next collection starts its own wait.
        p.add("b.java".into(), Kind::Created, at);
        assert_eq!(p.due(), Some(at + QUIET));
    }

    #[test]
    fn excluded_folders_are_never_told() {
        let root = temp("excluded");
        write(&root, "env/pyvenv.cfg", "home = /usr/bin\n");
        for file in [
            ".git/HEAD",
            ".idea/workspace.xml",
            ".idecraft/settings.toml",
            ".gradle/8.5/x.bin",
            "target/classes/A.class",
            "module/target/A.java",
            "build/generated/A.java",
            "app/build/tmp/B.java",
            "out/production/A.java",
            "web/node_modules/x/index.js",
            "pkg/__pycache__/a.pyc",
            ".venv/lib/site.py",
            "venv/lib/site.py",
            "env/lib/python3.12/site-packages/x.py",
        ] {
            assert!(!told(&root, Path::new(file)), "{file}");
        }
        for file in [
            "pom.xml",
            "src/main/java/shop/Dog.java",
            "src/main/java/com/acme/build/Tool.java",
            "app/src/out/Printer.kt",
            "environment/app.py",
        ] {
            assert!(told(&root, Path::new(file)), "{file}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn paths_outside_the_project_are_dropped() {
        let root = temp("inside");
        let other = temp("inside-other");
        write(&root, "src/A.java", "class A {}\n");
        write(&other, "B.java", "class B {}\n");
        assert_eq!(
            inside(&root, Path::new("src/A.java")),
            Some("src/A.java".into())
        );
        assert_eq!(
            inside(&root, &root.join("src/A.java")),
            Some("src/A.java".into())
        );
        // A deleted file is still inside.
        assert_eq!(
            inside(&root, Path::new("src/Gone.java")),
            Some("src/Gone.java".into())
        );
        assert_eq!(inside(&root, Path::new("../x/B.java")), None);
        assert_eq!(inside(&root, Path::new("src/../../B.java")), None);
        assert_eq!(inside(&root, &other.join("B.java")), None);
        assert_eq!(inside(&root, &root), None);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&other, root.join("linked")).unwrap();
            std::os::unix::fs::symlink(other.join("B.java"), root.join("src/B.java")).unwrap();
            assert_eq!(inside(&root, Path::new("linked/B.java")), None);
            assert_eq!(inside(&root, Path::new("linked/Gone.java")), None);
            assert_eq!(inside(&root, Path::new("src/B.java")), None);
        }
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&other).unwrap();
    }

    #[test]
    fn a_server_gets_its_languages_and_build_files() {
        let p = Path::new;
        assert!(interested("jdtls", p("src/main/java/shop/Dog.java")));
        assert!(interested("jdtls", p("pom.xml")));
        assert!(interested("jdtls", p("app/build.gradle.kts")));
        assert!(!interested("jdtls", p("tools/gen.py")));
        assert!(!interested("jdtls", p("package.json")));
        assert!(!interested("jdtls", p("notes.txt")));
        assert!(interested("rust-analyzer", p("crates/a/src/lib.rs")));
        assert!(interested("rust-analyzer", p("Cargo.toml")));
        assert!(!interested("rust-analyzer", p("pyproject.toml")));
        assert!(interested("typescript-language-server", p("web/app.tsx")));
        assert!(interested("typescript-language-server", p("tsconfig.json")));
        assert!(interested("gopls", p("go.mod")));
        assert!(interested("pyright-langserver", p("pyproject.toml")));
        assert!(!interested("no-such-server", p("src/A.java")));
    }

    #[test]
    fn notifications_carry_uris_and_types_in_chunks() {
        let root = Path::new("/p/my shop");
        let one = notifications(
            root,
            &[
                ("src/New.java".into(), Kind::Created),
                ("src/Edit.java".into(), Kind::Changed),
                ("src/Gone.java".into(), Kind::Deleted),
            ],
        );
        assert_eq!(
            one,
            [json!({"changes": [
                {"uri": "file:///p/my%20shop/src/New.java", "type": 1},
                {"uri": "file:///p/my%20shop/src/Edit.java", "type": 2},
                {"uri": "file:///p/my%20shop/src/Gone.java", "type": 3},
            ]})]
        );
        let many: Vec<(PathBuf, Kind)> = (0..1201)
            .map(|i| (PathBuf::from(format!("src/C{i:04}.java")), Kind::Created))
            .collect();
        let sent = notifications(root, &many);
        let sizes: Vec<usize> = sent
            .iter()
            .map(|n| n["changes"].as_array().unwrap().len())
            .collect();
        assert_eq!(sizes, [500, 500, 201]);
        assert_eq!(
            sent[2]["changes"][200]["uri"],
            "file:///p/my%20shop/src/C1200.java"
        );
        assert!(notifications(root, &[]).is_empty());
    }

    #[test]
    fn two_looks_report_what_changed_between_them() {
        let root = temp("looks");
        write(&root, "src/Animal.java", "class Animal {}\n");
        write(&root, "src/Old.java", "class Old {}\n");
        write(&root, "build/Gen.java", "class Gen {}\n");
        let mut watch = DiskWatch::default();
        assert!(
            watch.look(&root).is_empty(),
            "the first look is the baseline"
        );
        assert!(watch.look(&root).is_empty(), "nothing happened");
        write(&root, "src/Cat.java", "class Cat extends Animal {}\n");
        write(&root, "src/Animal.java", "abstract class Animal {}\n");
        std::fs::remove_file(root.join("src/Old.java")).unwrap();
        // Never told: build output, a virtual environment, version control.
        write(&root, "build/Gen2.java", "class Gen2 {}\n");
        write(&root, "tools/pyvenv.cfg", "home = /usr/bin\n");
        write(&root, "tools/lib/site.py", "");
        write(&root, ".git/index", "x");
        let mut seen = watch.look(&root);
        seen.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            seen,
            [
                (PathBuf::from("src/Animal.java"), Kind::Changed),
                (PathBuf::from("src/Cat.java"), Kind::Created),
                (PathBuf::from("src/Old.java"), Kind::Deleted),
            ]
        );
        assert!(watch.look(&root).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn renames_are_deleted_plus_created() {
        assert_eq!(
            kinds(&[
                FileChange::Renamed {
                    from: "a/Old.java".into(),
                    to: "b/New.java".into()
                },
                FileChange::Modified("c.java".into()),
            ]),
            [
                (PathBuf::from("a/Old.java"), Kind::Deleted),
                (PathBuf::from("b/New.java"), Kind::Created),
                (PathBuf::from("c.java"), Kind::Changed),
            ]
        );
    }
}
