//! Java source roots of a project without a build file (`SPEC-0042` v8 behaviour 17): the Maven and
//! Gradle convention folders, otherwise the folder from which a file's folders equal its declared
//! package. jdtls gets them as `java.project.sourcePaths`; nothing is written into the project.

use std::path::{Path, PathBuf};

/// Files whose presence means jdtls imports a build instead.
const BUILD_FILES: &[&str] = &[
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];

/// Convention folders, in this order.
const CONVENTION: &[&str] = &["src/main/java", "src/test/java"];

/// Folders never searched (besides hidden ones).
const SKIPPED: &[&str] = &["target", "build", "out", "bin", "node_modules"];

const MAX_FILES: usize = 5_000;
const MAX_DEPTH: usize = 20;

/// The source roots of `root`, relative to it with `/` separators; empty when it has a build file
/// or when the project folder itself would be the only root.
pub fn source_roots(root: &Path) -> Vec<String> {
    if BUILD_FILES.iter().any(|f| root.join(f).is_file()) {
        return Vec::new();
    }
    let mut roots: Vec<PathBuf> = CONVENTION
        .iter()
        .map(PathBuf::from)
        .filter(|p| root.join(p).is_dir())
        .collect();
    let mut inferred: Vec<PathBuf> = java_files(root)
        .into_iter()
        .filter(|f| !roots.iter().any(|r| f.starts_with(r)))
        .filter_map(|f| {
            let text = std::fs::read_to_string(root.join(&f)).ok()?;
            root_of(&f, package(&text).as_deref())
        })
        .collect();
    // Shorter paths first, then by name, so the choice does not depend on the folder order.
    inferred.sort_by(|a, b| {
        a.components()
            .count()
            .cmp(&b.components().count())
            .then(a.cmp(b))
    });
    inferred.dedup();
    for candidate in inferred {
        if candidate.as_os_str().is_empty() {
            continue;
        }
        if roots
            .iter()
            .any(|r| candidate.starts_with(r) || r.starts_with(&candidate))
        {
            continue;
        }
        roots.push(candidate);
    }
    roots
        .iter()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect()
}

/// File in a workspace data folder recording the source roots it was created with.
const ROOTS_STAMP: &str = "ferris-idea-source-roots";

/// The jdtls workspace data folder of the project `root` (`SPEC-0042` v9 behaviour 18):
/// `jdtls/<hash of the canonical path>` in Ferris IDEA's user cache folder; in the system's temporary
/// folder for a project inside it or without a user cache folder.
pub fn workspace_data_dir(root: &Path) -> PathBuf {
    let temp = std::env::temp_dir();
    let temp_base = temp.join("ferris-idea");
    let in_temp = [temp.clone(), temp.canonicalize().unwrap_or(temp)]
        .iter()
        .any(|t| root.starts_with(t));
    let base = if in_temp {
        temp_base
    } else {
        ferris_idea_core::user_cache_dir().unwrap_or(temp_base)
    }
    .join("jdtls");
    workspace_data_dir_in(&base, root)
}

/// [`workspace_data_dir`] below `base`.
pub fn workspace_data_dir_in(base: &Path, root: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let hex: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    base.join(format!("{name}-{hex}"))
}

/// Makes `dir` the data folder for a jdtls started with `roots` (behaviour 18): emptied when it
/// records other roots or none, then the roots are recorded. Returns whether it was renewed.
pub fn prepare_workspace_data(dir: &Path, roots: &[String]) -> std::io::Result<bool> {
    let stamp = roots.join("\n");
    let recorded = std::fs::read_to_string(dir.join(ROOTS_STAMP)).ok();
    if recorded.as_deref() == Some(stamp.as_str()) {
        return Ok(false);
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(ROOTS_STAMP), stamp)?;
    Ok(true)
}

/// The `.java` files below `root`, relative to it, sorted; hidden and build folders skipped.
fn java_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(PathBuf::new(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&dir)) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let rel = dir.join(&name);
            if kind.is_dir() {
                if depth + 1 < MAX_DEPTH
                    && !name_str.starts_with('.')
                    && !SKIPPED.contains(&name_str.as_ref())
                {
                    stack.push((rel, depth + 1));
                }
            } else if kind.is_file() && name_str.ends_with(".java") {
                found.push(rel);
                if found.len() >= MAX_FILES {
                    found.sort();
                    return found;
                }
            }
        }
    }
    found.sort();
    found
}

/// The declared package of a Java source text: the name after `package` before `;`, outside
/// comments; `None` without one.
pub fn package(text: &str) -> Option<String> {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix("//") {
            rest = r.split_once('\n').map_or("", |(_, after)| after);
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, after)| after);
        } else if rest.starts_with('@') {
            // A package annotation (`package-info.java`): skip to the end of its line.
            rest = rest.split_once('\n').map_or("", |(_, after)| after);
        } else {
            break;
        }
    }
    let r = rest.strip_prefix("package")?;
    if !r.starts_with(|c: char| c.is_whitespace() || c == '/') {
        return None;
    }
    let (name, _) = r.split_once(';')?;
    let name: String = name.split_whitespace().collect();
    (!name.is_empty()
        && name.split('.').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        }))
    .then_some(name)
}

/// The root for the file at `file` (relative) declaring `package`: its folder without the package's
/// folders; `None` when the folders do not end in the package.
fn root_of(file: &Path, package: Option<&str>) -> Option<PathBuf> {
    let mut dir = file.parent()?.to_path_buf();
    let Some(package) = package else {
        return Some(dir);
    };
    for part in package.split('.').rev() {
        if dir.file_name()? != part {
            return None;
        }
        dir.pop();
    }
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary project with `files`, removed on drop.
    struct Project(PathBuf);

    impl Project {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn project(files: &[(&str, &str)]) -> Project {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-java-roots-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (path, text) in files {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Project(dir)
    }

    #[test]
    fn maven_layout_without_build_file() {
        let d = project(&[
            (
                "src/main/java/lab/inventory/Inventory.java",
                "package lab.inventory;\nclass Inventory {}",
            ),
            (
                "src/test/java/lab/inventory/InventoryTest.java",
                "package lab.inventory;",
            ),
        ]);
        assert_eq!(source_roots(d.path()), ["src/main/java", "src/test/java"]);
    }

    #[test]
    fn package_decides_the_root() {
        let d = project(&[
            (
                "x/lab/inventory/Inventory.java",
                "/* licence */\n// note\npackage lab . inventory ;\n",
            ),
            ("y/Wrong.java", "package lab.other;"),
            ("tools/Tool.java", "class Tool {}"),
        ]);
        assert_eq!(source_roots(d.path()), ["tools", "x"]);
    }

    #[test]
    fn nested_roots_are_dropped() {
        let d = project(&[
            ("src/main/java/a/A.java", "package a;"),
            ("src/main/java/a/b/Stray.java", "package z;"),
            ("lib/p/P.java", "package p;"),
            ("lib/p/q/Q.java", "package q;"),
            ("lib/Free.java", ""),
        ]);
        // `lib` (from `lib/p/P.java` and `lib/Free.java`) wins over `lib/p` (from `Q.java`).
        assert_eq!(source_roots(d.path()), ["src/main/java", "lib"]);
    }

    #[test]
    fn none_with_a_build_file_or_only_top_level_files() {
        let d = project(&[
            ("pom.xml", "<project/>"),
            ("src/main/java/a/A.java", "package a;"),
        ]);
        assert!(source_roots(d.path()).is_empty());
        let d = project(&[("build.gradle.kts", ""), ("x/a/A.java", "package a;")]);
        assert!(source_roots(d.path()).is_empty());
        let d = project(&[("Main.java", "class Main {}")]);
        assert!(source_roots(d.path()).is_empty());
    }

    #[test]
    fn hidden_and_build_folders_are_skipped() {
        let d = project(&[
            (".idea/x/a/A.java", "package a;"),
            ("target/gen/b/B.java", "package b;"),
            ("node_modules/m/c/C.java", "package c;"),
        ]);
        assert!(source_roots(d.path()).is_empty());
    }

    /// SPEC-0042 AK-14: one data folder per project path, outside the project.
    #[test]
    fn workspace_data_dir_per_project() {
        let a = project(&[("java-basic/A.java", "")]);
        let b = project(&[("java-basic/A.java", "")]);
        let base = Path::new("/cache/jdtls");
        let da = workspace_data_dir_in(base, &a.path().join("java-basic"));
        let db = workspace_data_dir_in(base, &b.path().join("java-basic"));
        assert_ne!(da, db, "same folder name, different projects");
        assert!(da.starts_with(base));
        assert!(!da.starts_with(a.path()));
        assert!(
            da.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("java-basic-")
        );
        assert_eq!(
            da,
            workspace_data_dir_in(base, &a.path().join("java-basic"))
        );
        // The default lies in the user cache (or the temporary folder), never in the project.
        assert!(!workspace_data_dir(a.path()).starts_with(a.path()));
    }

    /// SPEC-0042 AK-14: kept with the same roots, emptied when they change or none are recorded.
    #[test]
    fn workspace_data_renewed_when_roots_change() {
        let d = project(&[("old/.metadata/x", "imported")]);
        let dir = d.path().join("old");
        let roots = vec!["src/main/java".to_owned(), "src/test/java".to_owned()];
        // Data from before v9 (nothing recorded) is renewed.
        assert!(prepare_workspace_data(&dir, &roots).unwrap());
        assert!(!dir.join(".metadata/x").exists());
        std::fs::create_dir_all(dir.join(".metadata")).unwrap();
        std::fs::write(dir.join(".metadata/x"), "imported").unwrap();
        // Same roots: kept.
        assert!(!prepare_workspace_data(&dir, &roots).unwrap());
        assert!(dir.join(".metadata/x").exists());
        // Other roots: renewed.
        assert!(prepare_workspace_data(&dir, &roots[..1]).unwrap());
        assert!(!dir.join(".metadata/x").exists());
        // No roots (a build file) is a state of its own.
        assert!(prepare_workspace_data(&dir, &[]).unwrap());
        assert!(!prepare_workspace_data(&dir, &[]).unwrap());
        // A missing folder is created.
        let fresh = d.path().join("new");
        assert!(prepare_workspace_data(&fresh, &roots).unwrap());
        assert!(fresh.is_dir());
    }

    #[test]
    fn packages_are_read_outside_comments() {
        assert_eq!(
            package("// package no;\n/* package no; */\npackage a.b;").as_deref(),
            Some("a.b")
        );
        assert_eq!(package("@Deprecated\npackage a;").as_deref(), Some("a"));
        assert_eq!(package("class packages {}"), None);
        assert_eq!(package("import a.b;"), None);
        assert_eq!(package(""), None);
    }
}
