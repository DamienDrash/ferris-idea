//! Watching workspace files by comparing snapshots (`SPEC-0026`).
//!
//! Portable and dependency-free: a background thread takes a snapshot of
//! file metadata at a fixed interval and reports the differences.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use crate::{Workspace, WorkspaceError};

/// Metadata that tells whether a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileState {
    /// Length in bytes.
    pub len: u64,
    /// Last modification time, if the platform reports it.
    pub modified: Option<SystemTime>,
    /// Inode number (Unix only).
    pub inode: Option<u64>,
}

/// File states by workspace-relative path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// One entry per file.
    pub files: BTreeMap<PathBuf, FileState>,
}

/// One change between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileChange {
    /// A new file.
    Created(PathBuf),
    /// Content or metadata changed.
    Modified(PathBuf),
    /// The file is gone.
    Deleted(PathBuf),
    /// The file moved.
    Renamed {
        /// Old path.
        from: PathBuf,
        /// New path.
        to: PathBuf,
    },
}

impl FileChange {
    /// Path the change is sorted by (the new path of a rename).
    pub fn path(&self) -> &Path {
        match self {
            Self::Created(p) | Self::Modified(p) | Self::Deleted(p) => p,
            Self::Renamed { to, .. } => to,
        }
    }
}

#[cfg(unix)]
fn inode(meta: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ino())
}

#[cfg(not(unix))]
fn inode(_meta: &std::fs::Metadata) -> Option<u64> {
    None
}

impl Workspace {
    /// Records the state of every workspace file (`SPEC-0026` behaviour 1).
    pub fn snapshot(&self) -> Result<Snapshot, WorkspaceError> {
        self.snapshot_without(&|_| false)
    }

    /// [`Workspace::snapshot`] without the folders for which `skip` says so
    /// ([`Workspace::files_without`]; `SPEC-0042` v11 behaviour 20).
    pub fn snapshot_without(
        &self,
        skip: &dyn Fn(&Path) -> bool,
    ) -> Result<Snapshot, WorkspaceError> {
        let mut files = BTreeMap::new();
        for relative in self.files_without(skip)? {
            // A file deleted between listing and reading is simply not in the snapshot.
            if let Ok(meta) = std::fs::metadata(self.root().join(&relative)) {
                files.insert(
                    relative,
                    FileState {
                        len: meta.len(),
                        modified: meta.modified().ok(),
                        inode: inode(&meta),
                    },
                );
            }
        }
        Ok(Snapshot { files })
    }
}

/// Changes from `old` to `new`, sorted by path (`SPEC-0026` behaviour 2).
pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<FileChange> {
    let mut changes = Vec::new();
    let mut deleted: Vec<(&PathBuf, &FileState)> = Vec::new();
    let mut created: Vec<(&PathBuf, &FileState)> = Vec::new();
    for (path, before) in &old.files {
        match new.files.get(path) {
            None => deleted.push((path, before)),
            Some(after) if after != before => changes.push(FileChange::Modified(path.clone())),
            Some(_) => {}
        }
    }
    for (path, after) in &new.files {
        if !old.files.contains_key(path) {
            created.push((path, after));
        }
    }
    let same = |a: &FileState, b: &FileState| match (a.inode, b.inode) {
        (Some(x), Some(y)) => x == y,
        _ => a.len == b.len && a.modified.is_some() && a.modified == b.modified,
    };
    let mut paired_created = vec![false; created.len()];
    for (from, before) in &deleted {
        let matches: Vec<usize> = created
            .iter()
            .enumerate()
            .filter(|(i, (_, after))| !paired_created[*i] && same(before, after))
            .map(|(i, _)| i)
            .collect();
        let unique_back = matches.len() == 1
            && deleted
                .iter()
                .filter(|(_, other)| same(other, created[matches[0]].1))
                .count()
                == 1;
        if unique_back {
            paired_created[matches[0]] = true;
            changes.push(FileChange::Renamed {
                from: (*from).clone(),
                to: created[matches[0]].0.clone(),
            });
        } else {
            changes.push(FileChange::Deleted((*from).clone()));
        }
    }
    for (i, (path, _)) in created.iter().enumerate() {
        if !paired_created[i] {
            changes.push(FileChange::Created((*path).clone()));
        }
    }
    changes.sort_by(|a, b| a.path().cmp(b.path()).then(a.cmp(b)));
    changes
}

/// The state to report after a tick (`SPEC-0026` v3 behaviour 3): a file takes its state from
/// `next` only when that state already held one tick earlier (`previous`); a file still changing
/// keeps the state last reported (`reported`). So a file another program is still writing (truncated,
/// half written) is reported once it has stood still for a whole interval, not in between.
pub fn settle(reported: &Snapshot, previous: &Snapshot, next: &Snapshot) -> Snapshot {
    let mut files = BTreeMap::new();
    let paths = reported
        .files
        .keys()
        .chain(previous.files.keys())
        .chain(next.files.keys());
    for path in paths {
        if files.contains_key(path) {
            continue;
        }
        let now = next.files.get(path);
        let state = if previous.files.get(path) == now {
            now
        } else {
            reported.files.get(path)
        };
        if let Some(state) = state {
            files.insert(path.clone(), *state);
        }
    }
    Snapshot { files }
}

/// Keeps a watcher running; dropping it stops the thread.
#[derive(Debug)]
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Default time between snapshots.
pub const WATCH_INTERVAL: Duration = Duration::from_millis(500);

/// Watches `workspace` and sends non-empty change batches (`SPEC-0026` behaviour 3).
pub fn watch(
    workspace: Workspace,
    interval: Duration,
) -> Result<(WatchHandle, mpsc::Receiver<Vec<FileChange>>), WorkspaceError> {
    // What the receiver has been told, and the snapshot of the previous tick.
    let mut reported = workspace.snapshot()?;
    let mut previous = reported.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let flag = Arc::clone(&stop);
    std::thread::Builder::new()
        .name("idecraft-watch".into())
        .spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(interval);
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                // A directory that vanishes mid-scan is retried on the next tick.
                let Ok(next) = workspace.snapshot() else {
                    continue;
                };
                let settled = settle(&reported, &previous, &next);
                let changes = diff(&reported, &settled);
                reported = settled;
                previous = next;
                if !changes.is_empty() && tx.send(changes).is_err() {
                    break;
                }
            }
        })
        .map_err(|source| WorkspaceError::Io {
            path: PathBuf::from("<watch thread>"),
            source,
        })?;
    Ok((WatchHandle { stop }, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(len: u64, secs: u64, inode: Option<u64>) -> FileState {
        FileState {
            len,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
            inode,
        }
    }

    fn snap(entries: &[(&str, FileState)]) -> Snapshot {
        Snapshot {
            files: entries
                .iter()
                .map(|(p, s)| (PathBuf::from(p), *s))
                .collect(),
        }
    }

    #[test]
    fn diff_kinds_and_rename_pairing() {
        let old = snap(&[
            ("a.txt", state(1, 1, Some(10))),
            ("b.txt", state(2, 1, Some(11))),
            ("c.txt", state(3, 1, Some(12))),
        ]);
        let new = snap(&[
            ("a.txt", state(5, 2, Some(10))),
            ("d/moved.txt", state(2, 1, Some(11))),
            ("e.txt", state(9, 3, Some(13))),
        ]);
        assert_eq!(
            diff(&old, &new),
            [
                FileChange::Modified("a.txt".into()),
                FileChange::Deleted("c.txt".into()),
                FileChange::Renamed {
                    from: "b.txt".into(),
                    to: "d/moved.txt".into()
                },
                FileChange::Created("e.txt".into()),
            ]
        );
        assert!(diff(&new, &new).is_empty());
    }

    #[test]
    fn a_file_still_being_written_is_reported_once_it_stands_still() {
        let before = snap(&[("a.txt", state(14, 1, Some(10)))]);
        // Truncated by a writer that has not written yet: not reported.
        let truncated = snap(&[("a.txt", state(0, 2, Some(10)))]);
        let held = settle(&before, &before, &truncated);
        assert!(diff(&before, &held).is_empty());
        // Written in full: still moving, still not reported.
        let full = snap(&[("a.txt", state(19, 2, Some(10)))]);
        let held = settle(&held, &truncated, &full);
        assert!(diff(&before, &held).is_empty());
        // The same state one tick later: reported once, as the final state.
        let now = settle(&held, &full, &full);
        assert_eq!(diff(&held, &now), [FileChange::Modified("a.txt".into())]);
        assert_eq!(now, full);
        assert_eq!(settle(&now, &full, &full), now);
        // A rename settles as a rename; a new file and a deletion settle after one quiet tick.
        let moved = snap(&[
            ("b.txt", state(19, 2, Some(10))),
            ("c.txt", state(1, 3, Some(11))),
        ]);
        let held = settle(&now, &full, &moved);
        assert_eq!(held, now);
        let done = settle(&held, &moved, &moved);
        assert_eq!(
            diff(&held, &done),
            [
                FileChange::Renamed {
                    from: "a.txt".into(),
                    to: "b.txt".into()
                },
                FileChange::Created("c.txt".into()),
            ]
        );
    }

    #[test]
    fn ambiguous_renames_without_inode_stay_delete_and_create() {
        let old = snap(&[("x", state(4, 7, None)), ("y", state(4, 7, None))]);
        let new = snap(&[("p", state(4, 7, None)), ("q", state(4, 7, None))]);
        let changes = diff(&old, &new);
        assert_eq!(changes.len(), 4);
        assert!(
            changes
                .iter()
                .all(|c| !matches!(c, FileChange::Renamed { .. }))
        );
        let one = diff(
            &snap(&[("x", state(4, 7, None))]),
            &snap(&[("z", state(4, 7, None))]),
        );
        assert_eq!(
            one,
            [FileChange::Renamed {
                from: "x".into(),
                to: "z".into()
            }]
        );
    }
}
