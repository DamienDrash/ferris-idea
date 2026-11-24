//! The lock file through which Claude Code finds the link (`SPEC-0358` behaviours 3–7): one JSON
//! file `<port>.lock` per link in the lock folder, readable by the user only, removed when the link
//! stops. Files left by Ferris IDEA processes that no longer run are removed at start; files of
//! other IDEs and of running processes are never touched.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The largest lock file read while cleaning up (behaviour 6).
const MAX_LOCK_BYTES: u64 = 64 * 1024;

/// The lock folder Claude Code reads: `$CLAUDE_CONFIG_DIR/ide`, else `~/.claude/ide`
/// (behaviour 3). `None` without a home folder.
pub fn default_lock_dir() -> Option<PathBuf> {
    lock_dir_from(
        std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// [`default_lock_dir`] from the two variables.
pub fn lock_dir_from(claude_config: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    match claude_config.filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => Some(dir.join("ide")),
        None => home
            .filter(|h| !h.as_os_str().is_empty())
            .map(|h| h.join(".claude").join("ide")),
    }
}

/// What the lock file says (behaviour 4). The token is the only secret in it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LockContent<'a> {
    pub pid: u32,
    pub workspace_folders: Vec<String>,
    pub ide_name: &'a str,
    pub transport: &'static str,
    pub auth_token: &'a str,
}

/// A written lock file; dropping it removes the file (behaviour 5).
#[derive(Debug)]
pub struct LockFile {
    path: PathBuf,
}

impl LockFile {
    /// Writes `<dir>/<port>.lock` with `content`, the folder created private (0700) and the file
    /// readable and writable by the user only (0600), through a new temporary file renamed into
    /// place, so no reader ever sees a partial file or wider permissions.
    pub(crate) fn write(dir: &Path, port: u16, content: &LockContent<'_>) -> std::io::Result<Self> {
        if cfg!(not(unix)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "a private lock file is not supported on this platform yet",
            ));
        }
        create_private_dir(dir)?;
        let json = serde_json::to_vec(content).map_err(std::io::Error::other)?;
        let path = dir.join(format!("{port}.lock"));
        let tmp = dir.join(format!(".{port}.lock.{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let written = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut file = options.open(&tmp)?;
            file.write_all(&json)?;
            file.sync_all()?;
            std::fs::rename(&tmp, &path)
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(Self { path })
    }

    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

/// Removes the lock files in `dir` that `ide_name` wrote from processes that no longer run, other
/// than `own_pid`; returns how many (behaviour 6). Files of other IDEs, unreadable files, links and
/// files of running processes stay.
pub fn remove_stale(dir: &Path, ide_name: &str, own_pid: u32) -> usize {
    remove_stale_with(dir, ide_name, own_pid, process_alive)
}

/// [`remove_stale`] with the liveness check given (tests).
pub fn remove_stale_with(
    dir: &Path,
    ide_name: &str,
    own_pid: u32,
    alive: impl Fn(u32) -> bool,
) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "lock") {
            continue;
        }
        // Only plain files: a link could point anywhere.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_LOCK_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if value["ideName"].as_str() != Some(ide_name) {
            continue;
        }
        let Some(pid) = value["pid"].as_u64().and_then(|p| u32::try_from(p).ok()) else {
            continue;
        };
        if pid != own_pid && !alive(pid) && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The lock files in `dir` that `ide_name` wrote for `workspace` from running processes: port and
/// pid, never the token (for the MCP tool in a process without the link).
pub fn running_links(dir: &Path, ide_name: &str, workspace: &Path) -> Vec<(u16, u32)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let wanted = workspace.to_string_lossy();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(port) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|_| path.extension().is_some_and(|x| x == "lock"))
        else {
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_LOCK_BYTES {
            continue;
        }
        let Some(value) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            continue;
        };
        let ours = value["ideName"].as_str() == Some(ide_name);
        let serves = value["workspaceFolders"]
            .as_array()
            .is_some_and(|a| a.iter().any(|f| f.as_str() == Some(&wanted)));
        let pid = value["pid"].as_u64().and_then(|p| u32::try_from(p).ok());
        if let Some(pid) = pid.filter(|p| ours && serves && process_alive(*p)) {
            out.push((port, pid));
        }
    }
    out.sort_unstable();
    out
}

/// Whether a process with `pid` runs (signal 0 through `kill`, no unsafe code).
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Off Unix every process counts as running, so nothing is removed.
#[cfg(not(unix))]
pub fn process_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0358 AK-3: the lock folder follows CLAUDE_CONFIG_DIR, else the home folder.
    #[test]
    fn lock_dir_follows_claude_config_dir_then_home() {
        assert_eq!(
            lock_dir_from(Some("/c".into()), Some("/h".into())),
            Some(PathBuf::from("/c/ide"))
        );
        assert_eq!(
            lock_dir_from(Some("".into()), Some("/h".into())),
            Some(PathBuf::from("/h/.claude/ide"))
        );
        assert_eq!(lock_dir_from(None, None), None);
    }
}
