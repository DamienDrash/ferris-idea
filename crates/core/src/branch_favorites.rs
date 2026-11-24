//! Favourite branches per repository (`SPEC-0068` v8 behaviour 25, `SPEC-0086` v10 behaviour 19):
//! kept in the workspace's Ferris IDEA settings (`.ferris-idea/branch-favorites.toml`), never in `.git`,
//! with local and remote-tracking branches apart. `main` and `master` are favourites until they are
//! unmarked.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Where the favourites of a workspace are kept, relative to its root.
pub const BRANCH_FAVORITES: &str = ".ferris-idea/branch-favorites.toml";

/// Branch names that are favourites until unmarked, locally and on every remote.
pub const DEFAULT_FAVORITES: [&str; 2] = ["main", "master"];

/// Whether `name` is a favourite by default: `main`/`master`, or `<remote>/main`/`<remote>/master`
/// for a remote-tracking branch.
pub fn is_default_favorite(name: &str, remote: bool) -> bool {
    let short = if remote {
        match name.split_once('/') {
            Some((_, rest)) => rest,
            None => return false,
        }
    } else {
        name
    };
    DEFAULT_FAVORITES.contains(&short)
}

/// The favourites of one repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoFavorites {
    /// The repository's work tree relative to the workspace root (`.` for the root itself).
    #[serde(default)]
    pub path: String,
    /// Local branches marked as favourites.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub local: BTreeSet<String>,
    /// Remote-tracking branches marked as favourites (`origin/feature`).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub remote: BTreeSet<String>,
    /// Default favourites (local) the user unmarked.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub local_unmarked: BTreeSet<String>,
    /// Default favourites (remote) the user unmarked.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub remote_unmarked: BTreeSet<String>,
}

impl RepoFavorites {
    /// Whether branch `name` (local or remote-tracking) is a favourite.
    pub fn is_favorite(&self, name: &str, remote: bool) -> bool {
        let (marked, unmarked) = if remote {
            (&self.remote, &self.remote_unmarked)
        } else {
            (&self.local, &self.local_unmarked)
        };
        marked.contains(name) || (is_default_favorite(name, remote) && !unmarked.contains(name))
    }

    /// Marks or unmarks `name`.
    pub fn set(&mut self, name: &str, remote: bool, favorite: bool) {
        let default = is_default_favorite(name, remote);
        let (marked, unmarked) = if remote {
            (&mut self.remote, &mut self.remote_unmarked)
        } else {
            (&mut self.local, &mut self.local_unmarked)
        };
        if favorite {
            unmarked.remove(name);
            if !default {
                marked.insert(name.to_owned());
            }
        } else {
            marked.remove(name);
            if default {
                unmarked.insert(name.to_owned());
            }
        }
    }

    /// After local branch `old` was renamed to `new`: a favourite stays one under its new name
    /// (`SPEC-0068` v8 behaviour 31, IMP-0340). A default name keeps being a default.
    pub fn rename(&mut self, old: &str, new: &str) {
        if self.is_favorite(old, false) {
            self.local.remove(old);
            self.set(new, false, true);
        }
    }

    fn is_empty(&self) -> bool {
        self.local.is_empty()
            && self.remote.is_empty()
            && self.local_unmarked.is_empty()
            && self.remote_unmarked.is_empty()
    }
}

/// The favourites of every repository of a workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchFavorites {
    /// One entry per repository that has a change from the defaults.
    #[serde(default)]
    pub repository: Vec<RepoFavorites>,
}

impl BranchFavorites {
    /// Reads the favourites of the workspace at `root`; a missing or broken file means the
    /// defaults.
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(BRANCH_FAVORITES))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Writes the favourites back.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let mut text = String::from("# Favourite branches of this workspace (Ferris IDEA)\n");
        text.push_str(&toml::to_string(self).map_err(std::io::Error::other)?);
        crate::write_atomic(&root.join(BRANCH_FAVORITES), text)
    }

    /// The favourites of the repository at `repo` (a key from [`repo_key`]).
    pub fn of(&self, repo: &str) -> RepoFavorites {
        self.repository
            .iter()
            .find(|r| r.path == repo)
            .cloned()
            .unwrap_or_else(|| RepoFavorites {
                path: repo.to_owned(),
                ..RepoFavorites::default()
            })
    }

    /// Marks or unmarks `name` in the repository `repo`.
    pub fn set(&mut self, repo: &str, name: &str, remote: bool, favorite: bool) {
        let mut entry = self.of(repo);
        entry.set(name, remote, favorite);
        self.put(repo, entry);
    }

    fn put(&mut self, repo: &str, entry: RepoFavorites) {
        self.repository.retain(|r| r.path != repo);
        if !entry.is_empty() {
            self.repository.push(entry);
            self.repository.sort_by(|a, b| a.path.cmp(&b.path));
        }
    }
}

/// The key of the repository whose work tree is `workdir` in the workspace at `root`: its path
/// relative to the root, `.` for the root itself, the full path when it lies outside.
pub fn repo_key(root: &Path, workdir: &Path) -> String {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let (root, workdir) = (canon(root), canon(workdir));
    match workdir.strip_prefix(&root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => workdir.to_string_lossy().replace('\\', "/"),
    }
}

/// Loads, changes and saves one favourite; returns the new state.
pub fn set_branch_favorite(
    root: &Path,
    repo: &str,
    name: &str,
    remote: bool,
    favorite: bool,
) -> std::io::Result<bool> {
    let mut all = BranchFavorites::load(root);
    all.set(repo, name, remote, favorite);
    all.save(root)?;
    Ok(all.of(repo).is_favorite(name, remote))
}

/// After local branch `old` of repository `repo` was renamed to `new`: moves its mark (IMP-0340).
/// Nothing is written when `old` was no favourite.
pub fn rename_branch_favorite(
    root: &Path,
    repo: &str,
    old: &str,
    new: &str,
) -> std::io::Result<()> {
    let mut all = BranchFavorites::load(root);
    let mut entry = all.of(repo);
    if !entry.is_favorite(old, false) {
        return Ok(());
    }
    entry.rename(old, new);
    all.put(repo, entry);
    all.save(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0068 v8 AK-14: defaults, marking, unmarking a default, local and remote apart.
    #[test]
    fn defaults_marks_and_unmarks_local_and_remote_apart() {
        let mut f = RepoFavorites::default();
        assert!(f.is_favorite("main", false) && f.is_favorite("master", false));
        assert!(f.is_favorite("origin/main", true) && f.is_favorite("upstream/master", true));
        assert!(
            !f.is_favorite("origin/main", false),
            "a local branch named origin/main"
        );
        assert!(
            !f.is_favorite("main", true),
            "a remote name needs its remote"
        );
        assert!(!f.is_favorite("feature/main", false));
        f.set("feature/x", false, true);
        assert!(f.is_favorite("feature/x", false));
        assert!(
            !f.is_favorite("feature/x", true),
            "local and remote are apart"
        );
        f.set("main", false, false);
        assert!(!f.is_favorite("main", false));
        assert!(
            f.is_favorite("origin/main", true),
            "the remote default stays"
        );
        assert!(f.local_unmarked.contains("main") && !f.local.contains("main"));
        f.set("main", false, true);
        assert!(f.is_favorite("main", false) && f.local_unmarked.is_empty());
        f.set("feature/x", false, false);
        assert!(f.is_empty());
        // IMP-0340: a renamed favourite stays one.
        f.set("topic", false, true);
        f.rename("topic", "topic2");
        assert!(f.is_favorite("topic2", false) && !f.is_favorite("topic", false));
        f.rename("main", "trunk");
        assert!(f.is_favorite("trunk", false));
        f.rename("other", "other2");
        assert!(!f.is_favorite("other2", false));
    }

    #[test]
    fn saved_per_repository_in_the_workspace_settings() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-fav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert_eq!(repo_key(&dir, &dir), ".");
        assert_eq!(repo_key(&dir, &dir.join("sub")), "sub");
        assert!(set_branch_favorite(&dir, ".", "topic", false, true).unwrap());
        assert!(!set_branch_favorite(&dir, "sub", "origin/main", true, false).unwrap());
        let all = BranchFavorites::load(&dir);
        assert!(all.of(".").is_favorite("topic", false));
        assert!(!all.of("sub").is_favorite("topic", false), "per repository");
        assert!(!all.of("sub").is_favorite("origin/main", true));
        assert!(all.of(".").is_favorite("origin/main", true));
        let text = std::fs::read_to_string(dir.join(BRANCH_FAVORITES)).unwrap();
        assert!(text.contains("[[repository]]") && text.contains("remote_unmarked"));
        assert!(!dir.join(".git").exists(), "nothing goes into .git");
        // Back to the defaults: the entry goes away.
        set_branch_favorite(&dir, ".", "topic", false, false).unwrap();
        assert_eq!(BranchFavorites::load(&dir).repository.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
