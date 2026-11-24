//! Workspace model (`SPEC-0001`).
//!
//! A workspace is a directory the user opens. It owns an optional settings
//! file at `.ferris-idea/settings.toml` and exposes the project files below its
//! root, skipping version-control metadata and common build output.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::{ConfigError, Layer, LayeredConfig};

/// Directory names that are never listed as project content.
pub const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".ferris-idea",
    "target",
    "node_modules",
    "__pycache__",
];

/// Relative location of the workspace settings file.
pub const SETTINGS_FILE: &str = ".ferris-idea/settings.toml";

/// Errors raised while opening or reading a workspace.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// The path does not exist or is not a directory.
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
    /// A file-system operation failed.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// Path the operation was performed on.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// The workspace settings file is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// An opened workspace.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Opens the directory at `root`. The path is canonicalized so that two
    /// spellings of the same directory yield equal roots.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref();
        let canonical = fs::canonicalize(root).map_err(|source| WorkspaceError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        if !canonical.is_dir() {
            return Err(WorkspaceError::NotADirectory(canonical));
        }
        // The project folder of the product's former name moves once, before anything reads or
        // writes the project's IDE state (`SPEC-0356` behaviour 2).
        crate::takeover::take_over_project_folder(&canonical);
        Ok(Self { root: canonical })
    }

    /// Canonical root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Display name: the last component of the root path.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    /// Loads the workspace settings file, if present, into `config` as the
    /// [`Layer::Workspace`] layer. Returns whether a file was found.
    pub fn load_settings(&self, config: &mut LayeredConfig) -> Result<bool, WorkspaceError> {
        let path = self.root.join(SETTINGS_FILE);
        match fs::read_to_string(&path) {
            Ok(source) => {
                config.set_layer_toml(Layer::Workspace, &source)?;
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(WorkspaceError::Io { path, source }),
        }
    }

    /// Lists project files relative to the root, sorted, excluding
    /// [`EXCLUDED_DIRS`]. Symbolic links are not followed.
    pub fn files(&self) -> Result<Vec<PathBuf>, WorkspaceError> {
        self.list(EXCLUDED_DIRS, &|_| false)
    }

    /// Like [`Workspace::files`], but also without the folders for which `skip` says so; it gets
    /// each folder's path relative to the root and the listing does not descend into a skipped
    /// one (`SPEC-0042` v11 behaviour 20).
    pub fn files_without(
        &self,
        skip: &dyn Fn(&Path) -> bool,
    ) -> Result<Vec<PathBuf>, WorkspaceError> {
        self.list(EXCLUDED_DIRS, skip)
    }

    /// Like [`Workspace::files`], but also the files in excluded folders such as `node_modules`,
    /// `target` and `__pycache__` (Search Everywhere's non-project items, `SPEC-0067` v5
    /// behaviour 13); only the version-control and Ferris IDEA folders stay out.
    pub fn all_files(&self) -> Result<Vec<PathBuf>, WorkspaceError> {
        self.list(&[".git", ".hg", ".svn", ".ferris-idea"], &|_| false)
    }

    fn list(
        &self,
        excluded_dirs: &[&str],
        skip: &dyn Fn(&Path) -> bool,
    ) -> Result<Vec<PathBuf>, WorkspaceError> {
        let mut out = Vec::new();
        let mut pending = vec![self.root.clone()];
        while let Some(dir) = pending.pop() {
            let entries = fs::read_dir(&dir).map_err(|source| WorkspaceError::Io {
                path: dir.clone(),
                source,
            })?;
            for entry in entries {
                let entry = entry.map_err(|source| WorkspaceError::Io {
                    path: dir.clone(),
                    source,
                })?;
                let file_type = entry.file_type().map_err(|source| WorkspaceError::Io {
                    path: entry.path(),
                    source,
                })?;
                let path = entry.path();
                if file_type.is_dir() {
                    let excluded = excluded_dirs.iter().any(|d| entry.file_name() == *d)
                        || path.strip_prefix(&self.root).is_ok_and(skip);
                    if !excluded {
                        pending.push(path);
                    }
                } else if file_type.is_file()
                    && let Ok(relative) = path.strip_prefix(&self.root)
                {
                    out.push(relative.to_path_buf());
                }
            }
        }
        out.sort();
        Ok(out)
    }
}

/// The SHA-256 digest of `data` as lowercase hex: what a profile keeps of each source file it has
/// line costs for, so that costs show only next to the text they were measured on (`SPEC-0120` v5
/// behaviour 10).
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("ferris-idea-ws-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, rel: &str, content: &str) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lists_files_sorted_and_skips_excluded_dirs() {
        let dir = TempDir::new("files");
        dir.write("src/main.py", "print(1)");
        dir.write("README.md", "#");
        dir.write(".git/HEAD", "ref");
        dir.write("node_modules/x/index.js", "");
        dir.write("target/debug/out", "");
        let ws = Workspace::open(&dir.0).unwrap();
        let files = ws.files().unwrap();
        assert_eq!(
            files,
            vec![PathBuf::from("README.md"), PathBuf::from("src/main.py")]
        );
    }

    // SPEC-0042/AK-18 (v11).
    #[test]
    fn lists_without_the_folders_the_caller_excludes() {
        let dir = TempDir::new("without");
        dir.write("src/main.py", "print(1)");
        dir.write("src/build/gen.py", "");
        dir.write("build/out.py", "");
        dir.write("target/debug/out", "");
        let ws = Workspace::open(&dir.0).unwrap();
        let files = ws
            .files_without(&|folder| folder == Path::new("build"))
            .unwrap();
        assert_eq!(
            files,
            vec![
                PathBuf::from("src/build/gen.py"),
                PathBuf::from("src/main.py")
            ]
        );
        let snapshot = ws
            .snapshot_without(&|folder| folder.ends_with("build"))
            .unwrap();
        assert_eq!(
            snapshot.files.keys().collect::<Vec<_>>(),
            [&PathBuf::from("src/main.py")]
        );
    }

    #[test]
    fn rejects_files_and_missing_paths() {
        let dir = TempDir::new("reject");
        dir.write("a.txt", "");
        assert!(matches!(
            Workspace::open(dir.0.join("a.txt")),
            Err(WorkspaceError::NotADirectory(_))
        ));
        assert!(matches!(
            Workspace::open(dir.0.join("missing")),
            Err(WorkspaceError::Io { .. })
        ));
    }

    #[test]
    fn workspace_settings_become_highest_layer() {
        let dir = TempDir::new("settings");
        dir.write(SETTINGS_FILE, "[editor]\ntab_size = 3");
        let ws = Workspace::open(&dir.0).unwrap();
        let mut config = LayeredConfig::new();
        config
            .set_layer_toml(Layer::User, "[editor]\ntab_size = 2")
            .unwrap();
        assert!(ws.load_settings(&mut config).unwrap());
        assert_eq!(config.get::<i64>("editor.tab_size").unwrap(), Some(3));
        assert!(
            !ws.files()
                .unwrap()
                .iter()
                .any(|p| p.starts_with(".ferris-idea"))
        );
    }

    #[test]
    fn missing_settings_file_is_not_an_error() {
        let dir = TempDir::new("nosettings");
        let ws = Workspace::open(&dir.0).unwrap();
        assert!(!ws.load_settings(&mut LayeredConfig::new()).unwrap());
    }

    #[test]
    fn sha256_of_the_standard_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
