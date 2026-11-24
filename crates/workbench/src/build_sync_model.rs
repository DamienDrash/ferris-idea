//! Which build files changed since the last sync (`SPEC-0100` v2 behaviour 9), without toolkit:
//! the badge over a changed build file and ⇧⌘I depend on it. A change only to comments or blank
//! space does not count.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The watched build files and what they were at the last sync.
#[derive(Debug, Clone, Default)]
pub struct BuildSync {
    /// Normalised text at the last sync, per file.
    known: BTreeMap<PathBuf, String>,
    /// Modification time last looked at, per file.
    stamps: BTreeMap<PathBuf, Option<SystemTime>>,
    /// Files whose meaning changed since the last sync.
    pending: BTreeSet<PathBuf>,
    /// Pending files whose badge the user closed; it comes back with the next change.
    dismissed: BTreeSet<PathBuf>,
}

fn normalized(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    std::fs::read_to_string(path)
        .map(|t| ferris_idea_core::buildtools::normalized_build_text(&name, &t))
        .unwrap_or_default()
}

fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl BuildSync {
    /// Watches `files` (absolute) as they are now, as synced.
    pub fn new(files: Vec<PathBuf>) -> Self {
        let mut s = Self::default();
        s.watch(files);
        s
    }

    /// Takes `files` as the watched set, as synced now.
    pub fn watch(&mut self, files: Vec<PathBuf>) {
        self.known = files.iter().map(|f| (f.clone(), normalized(f))).collect();
        self.stamps = files.iter().map(|f| (f.clone(), stamp(f))).collect();
        self.pending.clear();
        self.dismissed.clear();
    }

    /// Looks at the watched files again; true when the set of pending files changed.
    pub fn check(&mut self) -> bool {
        let before = self.pending.clone();
        let files: Vec<PathBuf> = self.known.keys().cloned().collect();
        for f in files {
            let now = stamp(&f);
            if self.stamps.get(&f) == Some(&now) {
                continue;
            }
            self.stamps.insert(f.clone(), now);
            if self.known.get(&f).map(String::as_str) == Some(normalized(&f).as_str()) {
                self.pending.remove(&f);
            } else {
                self.pending.insert(f.clone());
                self.dismissed.remove(&f);
            }
        }
        self.pending != before
    }

    /// Whether `path` changed since the last sync and its badge is not closed.
    pub fn badge(&self, path: &Path) -> bool {
        self.pending.contains(path) && !self.dismissed.contains(path)
    }

    /// Files changed since the last sync.
    pub fn pending(&self) -> Vec<PathBuf> {
        self.pending.iter().cloned().collect()
    }

    /// Hides the badge of `path` until it changes again.
    pub fn dismiss(&mut self, path: &Path) {
        self.dismissed.insert(path.to_path_buf());
    }

    /// Whether `path` is watched.
    pub fn watches(&self, path: &Path) -> bool {
        self.known.contains_key(path)
    }

    /// Watches a build file that came after the last sync (a new module) from now on, as it is on
    /// disk now; false when it was watched already.
    pub fn adopt(&mut self, path: &Path) -> bool {
        if self.watches(path) {
            return false;
        }
        self.known.insert(path.to_path_buf(), normalized(path));
        self.stamps.insert(path.to_path_buf(), stamp(path));
        true
    }
}

/// Whether a file of this name is a build file whose changes ask for a sync.
pub fn is_build_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some(
            "pom.xml"
                | "build.gradle"
                | "build.gradle.kts"
                | "settings.gradle"
                | "settings.gradle.kts"
                | "libs.versions.toml"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0100 v2 AK-8: a real change is pending until the sync, a comment is not, a dismissed
    /// badge comes back with the next change.
    #[test]
    fn changes_wait_for_a_sync() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-build-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pom = dir.join("pom.xml");
        std::fs::write(&pom, "<project><a>1</a></project>").unwrap();
        let mut sync = BuildSync::new(vec![pom.clone()]);
        assert!(sync.watches(&pom) && !sync.badge(&pom));
        let later = |s: &str| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            std::fs::write(&pom, s).unwrap();
        };
        later("<project><!-- c --><a>1</a></project>");
        assert!(!sync.check() && !sync.badge(&pom), "a comment is no change");
        later("<project><a>2</a></project>");
        assert!(sync.check() && sync.badge(&pom));
        sync.dismiss(&pom);
        assert!(!sync.badge(&pom) && sync.pending() == [pom.clone()]);
        later("<project><a>3</a></project>");
        sync.check();
        assert!(sync.badge(&pom), "the next change shows it again");
        later("<project><a>1</a></project>");
        assert!(sync.check() && !sync.badge(&pom), "back as synced");
        later("<project><a>4</a></project>");
        sync.check();
        sync.watch(vec![pom.clone()]);
        assert!(!sync.badge(&pom) && sync.pending().is_empty());
        let other = dir.join("build.gradle.kts");
        std::fs::write(&other, "plugins { java }").unwrap();
        assert!(is_build_file(&other) && !is_build_file(&dir.join("Main.java")));
        assert!(sync.adopt(&other) && !sync.adopt(&other));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&other, "plugins { application }").unwrap();
        assert!(sync.check() && sync.badge(&other));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
