//! Local copies of remote files for editing (behaviour 4): every opened file has one place in the
//! user's cache folder, so the editor, highlighting and local history work on it, and saving
//! sends it back.

use std::path::{Component, Path, PathBuf};

use crate::ssh::SshTarget;

/// The user's cache folder for Ferris IDEA (`$XDG_CACHE_HOME`, else `~/Library/Caches` on macOS,
/// else `~/.cache`).
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("ferris-idea");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if cfg!(target_os = "macos") {
        home.join("Library/Caches/ferris-idea")
    } else {
        home.join(".cache/ferris-idea")
    }
}

/// Folder name of a host: the alias (with the port when one is set), other characters as `_`.
pub fn host_folder(target: &SshTarget) -> String {
    let mut name: String = target
        .host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._@-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if let Some(port) = target.port {
        name.push_str(&format!("_{port}"));
    }
    name
}

/// Where the copy of absolute remote `path` on `target` lives under `base`; `None` for a path
/// that is not absolute or goes up with `..`.
pub fn mirror_path(base: &Path, target: &SshTarget, path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return None;
    }
    let mut out = base.join("remote").join(host_folder(target));
    for c in p.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(n) => out.push(n),
            _ => return None,
        }
    }
    Some(out)
}

/// Joins remote folder `dir` and `name` with `/`.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The parent of remote path `path` (`/` stays `/`).
pub fn parent(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((p, _)) => p.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_have_one_place_per_host_and_path() {
        let mut t = SshTarget::new("me@dev box");
        let base = Path::new("/c");
        assert_eq!(
            mirror_path(base, &t, "/srv/app/main.rs").unwrap(),
            Path::new("/c/remote/me@dev_box/srv/app/main.rs")
        );
        t.port = Some(2222);
        assert_eq!(host_folder(&t), "me@dev_box_2222");
        assert!(mirror_path(base, &t, "srv/x").is_none());
        assert!(mirror_path(base, &t, "/srv/../etc/passwd").is_none());
        assert_eq!(join("/srv", "a"), "/srv/a");
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(parent("/srv/app/"), "/srv");
        assert_eq!(parent("/srv"), "/");
        assert_eq!(parent("/"), "/");
    }
}
