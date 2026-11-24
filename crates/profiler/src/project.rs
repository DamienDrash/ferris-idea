//! Which frames are the project's own code (`SPEC-0120` v5 behaviour 11), the digests that tie
//! line costs to the text they were measured on (behaviour 10) and the stored "Only project code"
//! choice (behaviour 12).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::run::{PROFILES_DIR, ProfileRun};

/// The workspace's files, listed once, to decide which reported source files are the project's.
pub struct SourceIndex {
    root: PathBuf,
    real_root: PathBuf,
    files: HashSet<PathBuf>,
    listed: Vec<PathBuf>,
    cache: HashMap<String, Option<PathBuf>>,
}

/// Folders of installed packages inside a workspace (a virtual environment in `.venv`): their
/// files are libraries, not the project's code.
fn in_packages(relative: &Path) -> bool {
    relative
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n == "site-packages" || n == "dist-packages"))
}

impl SourceIndex {
    /// Lists the files of the workspace at `root` (without version control, Ferris IDEA, build
    /// output and dependency folders, as the project view).
    pub fn new(root: &Path) -> Self {
        let listed = ferris_idea_core::Workspace::open(root)
            .ok()
            .and_then(|w| w.files().ok())
            .unwrap_or_default();
        Self {
            root: root.to_path_buf(),
            real_root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
            files: listed.iter().cloned().collect(),
            listed,
            cache: HashMap::new(),
        }
    }

    /// The workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The workspace-relative path of the project file a profiler `reported`, if the frame is the
    /// project's (behaviour 11): an absolute path of a listed file, a relative path listed as it
    /// is, a relative path with folders that ends exactly one listed path, or a bare name exactly
    /// one listed file has; never a file of installed packages.
    pub fn project_file(&mut self, reported: &str) -> Option<PathBuf> {
        if let Some(found) = self.cache.get(reported) {
            return found.clone();
        }
        let found = self.find(reported).filter(|p| !in_packages(p));
        self.cache.insert(reported.to_owned(), found.clone());
        found
    }

    /// Whether the frame of `reported` is in the project.
    pub fn in_project(&mut self, reported: &str) -> bool {
        self.project_file(reported).is_some()
    }

    fn find(&self, reported: &str) -> Option<PathBuf> {
        let path = Path::new(reported);
        if path.is_absolute() {
            let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let relative = real
                .strip_prefix(&self.real_root)
                .or_else(|_| path.strip_prefix(&self.root))
                .ok()?;
            return self
                .files
                .contains(relative)
                .then(|| relative.to_path_buf());
        }
        let path: PathBuf = path
            .components()
            .filter(|c| !matches!(c, Component::CurDir))
            .collect();
        if self.files.contains(&path) {
            return Some(path);
        }
        let unique = |mut found: std::slice::Iter<'_, PathBuf>,
                      keep: &dyn Fn(&PathBuf) -> bool|
         -> Option<PathBuf> {
            let first = found.by_ref().find(|f| keep(f))?.clone();
            found.all(|f| !keep(f)).then_some(first)
        };
        if path.components().count() > 1 {
            // A path with folders never falls back to its bare name (`java/util/Objects.java`).
            unique(self.listed.iter(), &|f| f.ends_with(&path))
        } else {
            let name = path.file_name()?;
            unique(self.listed.iter(), &|f| f.file_name() == Some(name))
        }
    }
}

/// The workspace of a profile directory `<root>/.ferris-idea/profiles`, if it is one.
pub(crate) fn workspace_of(dir: &Path) -> Option<&Path> {
    dir.ends_with(PROFILES_DIR)
        .then(|| dir.parent()?.parent())
        .flatten()
}

/// Marks the project's frames of `run` (behaviour 11) and keeps the digest of every project file
/// it has line costs for (behaviour 10).
pub fn prepare(run: &mut ProfileRun, root: &Path) {
    let mut index = SourceIndex::new(root);
    run.profile.mark_project(&mut |f| index.in_project(f));
    let mut sources = BTreeMap::new();
    for cost in run.profile.line_costs() {
        if sources.contains_key(&cost.file) {
            continue;
        }
        if let Some(file) = index.project_file(&cost.file)
            && let Ok(bytes) = fs::read(root.join(file))
        {
            sources.insert(cost.file.clone(), ferris_idea_core::sha256_hex(&bytes));
        }
    }
    run.sources = sources;
    run.marked = true;
}

/// Marks the project's frames of a run stored before `SPEC-0120` v5; it gets no digests, so its
/// costs are not shown next to lines.
pub(crate) fn mark_older(run: &mut ProfileRun, root: &Path) {
    let mut index = SourceIndex::new(root);
    run.profile.mark_project(&mut |f| index.in_project(f));
    run.marked = true;
}

/// The Profiler window's choices kept per project in `.ferris-idea/profiles/view.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewSettings {
    /// "Only project code" (`SPEC-0120` v5 behaviour 12).
    #[serde(default)]
    pub only_project_code: bool,
}

/// The file of [`ViewSettings`] in a profile directory.
pub const VIEW_FILE: &str = "view.json";

impl ViewSettings {
    /// The choices stored in the profile directory `dir`; the defaults without a readable file.
    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join(VIEW_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Stores the choices in `dir`.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(dir.join(VIEW_FILE), json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn workspace(tag: &str, files: &[&str]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-profiler-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for f in files {
            let path = root.join(f);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("// {f}\n")).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn frames_are_the_projects_by_their_file() {
        let root = workspace(
            "rule",
            &[
                "src/main/java/demo/Busy.java",
                "busy.c",
                "app.py",
                "a/util.py",
                "b/util.py",
                ".venv/lib/python3.12/site-packages/requests/api.py",
                "node_modules/left/index.js",
            ],
        );
        let mut index = SourceIndex::new(&root);
        // Absolute paths below the root and listed.
        assert!(index.in_project(&root.join("app.py").display().to_string()));
        assert!(!index.in_project("/usr/lib/python3.12/json/decoder.py"));
        assert!(
            !index.in_project(
                &root
                    .join(".venv/lib/python3.12/site-packages/requests/api.py")
                    .display()
                    .to_string()
            )
        );
        assert!(
            !index.in_project(
                &root
                    .join("node_modules/left/index.js")
                    .display()
                    .to_string()
            )
        );
        // Relative as it is, by the end of a path, by a unique bare name.
        assert!(index.in_project("./app.py"));
        assert_eq!(
            index.project_file("demo/Busy.java"),
            Some(PathBuf::from("src/main/java/demo/Busy.java"))
        );
        assert!(index.in_project("busy.c"));
        // A shared bare name, a JDK path and a path with folders whose name alone would match.
        assert!(!index.in_project("util.py"));
        assert!(!index.in_project("java/util/Objects.java"));
        assert!(!index.in_project("other/busy.c"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_filter_choice_is_kept_per_project() {
        let root = workspace("view", &[]);
        let dir = root.join(PROFILES_DIR);
        assert_eq!(ViewSettings::load(&dir), ViewSettings::default());
        ViewSettings {
            only_project_code: true,
        }
        .save(&dir)
        .unwrap();
        assert!(ViewSettings::load(&dir).only_project_code);
        assert_eq!(workspace_of(&dir), Some(root.as_path()));
        assert_eq!(workspace_of(Path::new("/tmp/x")), None);
        let _ = fs::remove_dir_all(&root);
    }
}
