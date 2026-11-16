//! Local history of workspace files (`SPEC-0044` v2: bounded log, stable revision ids).

use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::watch::FileChange;

/// Directory of the history, relative to the workspace root.
pub const HISTORY_DIR: &str = ".idecraft/history";
/// Largest file that is recorded.
pub const MAX_RECORDED_BYTES: u64 = 2 * 1024 * 1024;
/// Default retention for [`LocalHistory::prune`].
pub const DEFAULT_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Entries kept per file, and workspace labels kept, after a compaction (behaviour 8).
pub const MAX_REVISIONS_PER_FILE: usize = 1000;
/// Entries of one file (or workspace labels) that trigger a compaction (behaviour 8).
pub const COMPACT_AT: usize = MAX_REVISIONS_PER_FILE + MAX_REVISIONS_PER_FILE / 4;

/// What went wrong with the local history.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// Reading or writing failed.
    #[error("local history: {0}")]
    Io(String),
    /// No such revision for the file.
    #[error("no revision {revision} for {path}")]
    NoRevision {
        /// The file.
        path: String,
        /// The revision id asked for.
        revision: usize,
    },
}

fn io<E: std::fmt::Display>(e: E) -> HistoryError {
    HistoryError::Io(e.to_string())
}

/// Why a revision was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionKind {
    /// Content changed (or the file was deleted).
    Change,
    /// A label without new content.
    Label,
    /// Content written by a restore.
    Restore,
}

/// One line of `revisions.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    /// Sequence number (behaviour 9); lines written before v2 have none and count on from the
    /// previous line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<usize>,
    /// Workspace-relative path; `None` for a workspace label.
    path: Option<String>,
    /// Milliseconds since the epoch.
    time: u64,
    /// Content hash; `None` for a deletion or a label.
    hash: Option<String>,
    /// Size in bytes of the content.
    #[serde(default)]
    size: u64,
    /// Label text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    kind: RevisionKind,
}

/// A revision as listed for a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Revision {
    /// Id within the history; never changes (behaviour 9).
    pub id: usize,
    /// Milliseconds since the epoch.
    pub time: u64,
    /// Size in bytes (0 for deletions and labels).
    pub size: u64,
    /// Label, if any.
    pub label: Option<String>,
    /// Kind of revision.
    pub kind: RevisionKind,
    /// Whether the file was deleted at this point.
    pub deleted: bool,
    /// Whether this is a workspace-wide label.
    pub workspace_label: bool,
}

/// The local history of one workspace.
pub struct LocalHistory {
    root: PathBuf,
    dir: PathBuf,
    entries: Vec<Entry>,
    /// Identity of the log file and how far it has been read, for [`LocalHistory::refresh`].
    read: (u64, u64),
}

/// Identity of a file that changes when it is replaced (by a compaction's rename).
#[cfg(unix)]
fn file_id(m: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(m)
}

#[cfg(not(unix))]
fn file_id(_: &std::fs::Metadata) -> u64 {
    0
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl LocalHistory {
    /// Opens (or creates) the history of the workspace at `root` (behaviour 1).
    pub fn open(root: &Path) -> Result<Self, HistoryError> {
        let dir = root.join(HISTORY_DIR);
        std::fs::create_dir_all(dir.join("objects")).map_err(io)?;
        let mut history = Self {
            root: root.to_path_buf(),
            dir,
            entries: Vec::new(),
            read: (0, 0),
        };
        history.refresh()?;
        Ok(history)
    }

    /// Reads what other processes appended to the log since it was last read, or the whole log
    /// again when it was replaced by a compaction, so a long-lived holder stays current without
    /// parsing the whole log for every request (behaviour 8).
    pub fn refresh(&mut self) -> Result<(), HistoryError> {
        let log = self.dir.join("revisions.jsonl");
        let mut file = match std::fs::File::open(&log) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.entries.clear();
                self.read = (0, 0);
                return Ok(());
            }
            Err(e) => return Err(io(e)),
        };
        let meta = file.metadata().map_err(io)?;
        let id = file_id(&meta);
        if id != self.read.0 || meta.len() < self.read.1 {
            self.entries.clear();
            self.read = (id, 0);
        }
        if meta.len() == self.read.1 {
            return Ok(());
        }
        file.seek(SeekFrom::Start(self.read.1)).map_err(io)?;
        let mut next = self.next_id();
        let mut offset = self.read.1;
        // Line by line, without a copy of the whole log (behaviour 8).
        let mut reader = std::io::BufReader::new(file.take(meta.len() - offset));
        let mut line = Vec::new();
        loop {
            line.clear();
            let n = reader.read_until(b'\n', &mut line).map_err(io)?;
            // A line still being written is read next time.
            if n == 0 || line.last() != Some(&b'\n') {
                break;
            }
            offset += n as u64;
            // A damaged line is skipped, not the whole history (behaviour 6).
            let Ok(mut entry) = serde_json::from_slice::<Entry>(&line) else {
                continue;
            };
            let id = entry.id.unwrap_or(next);
            // Our own appends are already known.
            if id < next {
                continue;
            }
            entry.id = Some(id);
            next = id + 1;
            self.entries.push(entry);
        }
        self.read.1 = offset;
        Ok(())
    }

    fn next_id(&self) -> usize {
        self.entries
            .last()
            .and_then(|e| e.id)
            .map_or(0, |id| id + 1)
    }

    fn append(&mut self, mut entry: Entry) -> Result<(), HistoryError> {
        // Numbers on from what other processes appended meanwhile.
        self.refresh()?;
        entry.id = Some(self.next_id());
        let mut line = serde_json::to_string(&entry).map_err(io)?;
        line.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("revisions.jsonl"))
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .map_err(io)?;
        let path = entry.path.clone();
        let time = entry.time;
        self.entries.push(entry);
        let same = self.entries.iter().filter(|e| e.path == path).count();
        if same > COMPACT_AT {
            self.compact(DEFAULT_RETENTION, time)?;
        }
        Ok(())
    }

    fn store(&self, bytes: &[u8]) -> Result<String, HistoryError> {
        let h = hash(bytes);
        let object = self.dir.join("objects").join(&h);
        if !object.exists() {
            std::fs::write(&object, bytes).map_err(io)?;
        }
        Ok(h)
    }

    /// Latest content state of `path`: `Some(Some(hash))`, `Some(None)` if deleted, `None` if unknown.
    fn latest(&self, path: &str) -> Option<Option<&str>> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.path.as_deref() == Some(path) && e.kind != RevisionKind::Label)
            .map(|e| e.hash.as_deref())
    }

    /// Records the current state of `path` at `time`; returns whether a revision was added (behaviour 2).
    pub fn record_at(&mut self, path: &str, time: u64) -> Result<bool, HistoryError> {
        self.record_kind(path, time, RevisionKind::Change)
    }

    fn record_kind(
        &mut self,
        path: &str,
        time: u64,
        kind: RevisionKind,
    ) -> Result<bool, HistoryError> {
        self.refresh()?;
        let file = self.root.join(path);
        let (hash, size) = match std::fs::metadata(&file) {
            Ok(m) if m.is_file() => {
                if m.len() > MAX_RECORDED_BYTES {
                    return Ok(false);
                }
                let bytes = std::fs::read(&file).map_err(io)?;
                (Some(self.store(&bytes)?), bytes.len() as u64)
            }
            Ok(_) => return Ok(false),
            Err(_) => (None, 0),
        };
        match self.latest(path) {
            Some(last) if last == hash.as_deref() => return Ok(false),
            // Never seen and not there: nothing to remember.
            None if hash.is_none() => return Ok(false),
            _ => {}
        }
        self.append(Entry {
            id: None,
            path: Some(path.to_owned()),
            time,
            hash,
            size,
            label: None,
            kind,
        })?;
        Ok(true)
    }

    /// Records the current state of `path` now.
    pub fn record(&mut self, path: &str) -> Result<bool, HistoryError> {
        self.record_at(path, now_ms())
    }

    /// Records the files of watcher changes; returns how many revisions were added.
    pub fn record_changes(&mut self, changes: &[FileChange]) -> Result<usize, HistoryError> {
        let time = now_ms();
        let mut added = 0;
        for change in changes {
            let mut paths = vec![change.path()];
            if let FileChange::Renamed { from, .. } = change {
                paths.push(from);
            }
            for path in paths {
                let path = path.to_string_lossy().replace('\\', "/");
                added += usize::from(self.record_at(&path, time)?);
            }
        }
        Ok(added)
    }

    /// Puts a label on `path`, or on the whole workspace when `None` (behaviour 3).
    pub fn put_label_at(
        &mut self,
        path: Option<&str>,
        label: &str,
        time: u64,
    ) -> Result<(), HistoryError> {
        if let Some(p) = path {
            self.record_at(p, time)?;
        }
        self.append(Entry {
            id: None,
            path: path.map(str::to_owned),
            time,
            hash: None,
            size: 0,
            label: Some(label.to_owned()),
            kind: RevisionKind::Label,
        })
    }

    /// Revisions of `path`, newest first, with workspace labels (behaviour 4).
    pub fn revisions(&self, path: &str) -> Vec<Revision> {
        let first = self
            .entries
            .iter()
            .position(|e| e.path.as_deref() == Some(path));
        let mut out: Vec<Revision> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                e.path.as_deref() == Some(path)
                    || (e.path.is_none() && first.is_some_and(|f| *i > f))
            })
            .map(|(i, e)| Revision {
                id: e.id.unwrap_or(i),
                time: e.time,
                size: e.size,
                label: e.label.clone(),
                kind: e.kind,
                deleted: e.kind != RevisionKind::Label && e.hash.is_none(),
                workspace_label: e.path.is_none(),
            })
            .collect();
        out.reverse();
        out
    }

    fn entry(&self, path: &str, revision: usize) -> Result<&Entry, HistoryError> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.id == Some(revision))
            .filter(|e| e.path.as_deref() == Some(path) && e.kind != RevisionKind::Label)
            .ok_or_else(|| HistoryError::NoRevision {
                path: path.to_owned(),
                revision,
            })
    }

    /// Content of a revision; `None` for a deletion.
    pub fn content(&self, path: &str, revision: usize) -> Result<Option<Vec<u8>>, HistoryError> {
        match &self.entry(path, revision)?.hash {
            Some(h) => std::fs::read(self.dir.join("objects").join(h))
                .map(Some)
                .map_err(io),
            None => Ok(None),
        }
    }

    /// Restores `path` to `revision` (behaviour 5).
    pub fn restore_at(
        &mut self,
        path: &str,
        revision: usize,
        time: u64,
    ) -> Result<(), HistoryError> {
        let content = self.content(path, revision)?;
        // Keep what is there now, so the restore can be undone.
        self.record_at(path, time)?;
        let file = self.root.join(path);
        match content {
            Some(bytes) => {
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::write(&file, bytes).map_err(io)?;
            }
            None => {
                if file.exists() {
                    std::fs::remove_file(&file).map_err(io)?;
                }
            }
        }
        self.record_kind(path, time, RevisionKind::Restore)?;
        Ok(())
    }

    /// Restores `path` to `revision` now.
    pub fn restore(&mut self, path: &str, revision: usize) -> Result<(), HistoryError> {
        self.restore_at(path, revision, now_ms())
    }

    /// Drops revisions older than `retention` before `now` (keeping each file's latest), all but
    /// the newest [`MAX_REVISIONS_PER_FILE`] entries per file and workspace labels, and
    /// unreferenced contents; returns how many revisions were removed (behaviours 6 and 8).
    pub fn prune_at(&mut self, retention: Duration, now: u64) -> Result<usize, HistoryError> {
        self.compact(retention, now)
    }

    fn compact(&mut self, retention: Duration, now: u64) -> Result<usize, HistoryError> {
        let cutoff = now.saturating_sub(retention.as_millis() as u64);
        let mut latest: BTreeMap<&str, usize> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            if let (Some(p), true) = (e.path.as_deref(), e.kind != RevisionKind::Label) {
                latest.insert(p, i);
            }
        }
        let keep: HashSet<usize> = latest.into_values().collect();
        // Newest first: how many entries of the same file (or workspace labels) came later.
        let mut seen: BTreeMap<Option<&str>, usize> = BTreeMap::new();
        let mut within = vec![false; self.entries.len()];
        for (i, e) in self.entries.iter().enumerate().rev() {
            let n = seen.entry(e.path.as_deref()).or_default();
            within[i] = *n < MAX_REVISIONS_PER_FILE;
            *n += 1;
        }
        let before = self.entries.len();
        let kept: Vec<Entry> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| keep.contains(i) || (within[*i] && e.time >= cutoff))
            .map(|(_, e)| e.clone())
            .collect();
        let removed = before - kept.len();
        let mut text = String::new();
        for e in &kept {
            text.push_str(&serde_json::to_string(e).map_err(io)?);
            text.push('\n');
        }
        let log = self.dir.join("revisions.jsonl");
        let tmp = self.dir.join("revisions.jsonl.tmp");
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, &log).map_err(io)?;
        let meta = std::fs::metadata(&log).map_err(io)?;
        self.read = (file_id(&meta), meta.len());
        self.entries = kept;
        let used: HashSet<&str> = self
            .entries
            .iter()
            .filter_map(|e| e.hash.as_deref())
            .collect();
        for object in std::fs::read_dir(self.dir.join("objects")).map_err(io)? {
            let object = object.map_err(io)?;
            if !used.contains(object.file_name().to_string_lossy().as_ref()) {
                std::fs::remove_file(object.path()).map_err(io)?;
            }
        }
        Ok(removed)
    }

    /// Number of stored contents.
    pub fn object_count(&self) -> usize {
        std::fs::read_dir(self.dir.join("objects")).map_or(0, |d| d.count())
    }
}
