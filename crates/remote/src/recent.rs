//! Recent remote projects (`SPEC-0317` behaviour 1): host and folder, newest first, kept in the
//! user's settings folder next to the other recent projects. The file holds nothing else — no
//! user secret, no token, no key — and is private to the user (0600, `ADR-0025` SEC-10).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Most entries kept.
pub const MAX_RECENT: usize = 25;

/// One recent remote project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentRemote {
    /// Host alias from `~/.ssh/config` or `[user@]host[:port]` as typed.
    pub host: String,
    /// Absolute project folder on the host.
    pub folder: String,
}

impl RecentRemote {
    /// "folder — host" as lists show it.
    pub fn label(&self) -> String {
        let name = self
            .folder
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|n| !n.is_empty())
            .unwrap_or(&self.folder);
        format!("{name} — {}", self.host)
    }
}

/// The file in the settings folder `config_dir`.
pub fn recent_remote_file(config_dir: &Path) -> PathBuf {
    config_dir.join("recent-remote-projects.json")
}

/// The list in `file`, newest first; a missing or damaged file is an empty list.
pub fn load_recent_remote(file: &Path) -> Vec<RecentRemote> {
    std::fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<RecentRemote>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !r.host.is_empty() && r.folder.starts_with('/'))
        .take(MAX_RECENT)
        .collect()
}

/// Puts `host` and `folder` at the front of the list in `file` (written privately).
pub fn remember_remote(file: &Path, host: &str, folder: &str) -> std::io::Result<()> {
    let entry = RecentRemote {
        host: host.to_owned(),
        folder: folder.to_owned(),
    };
    let mut list = load_recent_remote(file);
    list.retain(|r| r != &entry);
    list.insert(0, entry);
    list.truncate(MAX_RECENT);
    let bytes = serde_json::to_vec_pretty(&list).map_err(std::io::Error::other)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        idecraft_session::reattach::write_private(file, &bytes)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(file, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates_and_private() {
        let dir =
            std::env::temp_dir().join(format!("idecraft-recent-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = recent_remote_file(&dir);
        assert!(load_recent_remote(&file).is_empty());
        remember_remote(&file, "build", "/srv/app").unwrap();
        remember_remote(&file, "me@box:2222", "/home/me/site/").unwrap();
        remember_remote(&file, "build", "/srv/app").unwrap();
        let list = load_recent_remote(&file);
        assert_eq!(
            list.iter().map(RecentRemote::label).collect::<Vec<_>>(),
            ["app — build", "site — me@box:2222"]
        );
        for i in 0..30 {
            remember_remote(&file, "h", &format!("/p{i}")).unwrap();
        }
        assert_eq!(load_recent_remote(&file).len(), MAX_RECENT);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(std::fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
        }
        // Only host and folder are kept.
        let text = std::fs::read_to_string(&file).unwrap();
        let keys: std::collections::BTreeSet<String> =
            serde_json::from_str::<Vec<serde_json::Map<String, serde_json::Value>>>(&text)
                .unwrap()
                .into_iter()
                .flat_map(|m| m.keys().cloned().collect::<Vec<_>>())
                .collect();
        assert_eq!(keys.into_iter().collect::<Vec<_>>(), ["folder", "host"]);
        std::fs::write(&file, "not json").unwrap();
        assert!(load_recent_remote(&file).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
