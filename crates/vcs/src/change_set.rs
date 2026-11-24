//! Change sets: an agent's proposed edits as one unit of work (`SPEC-0340`, ADR-0023 rule 6).
//!
//! A set holds, per file, the text the file had when its first edit was proposed (the base, with
//! its hash) and the proposed text. Its hunks are the line changes between the two; applying a
//! choice of hunks writes the base with only those changes. This module is the model and its
//! storage in `.ferris-idea/change-sets/`; the checks and the writing live in the MCP tools.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::diff::{LineChangeKind, apply_change, line_changes};

/// Where a workspace keeps its change sets (behaviour 1).
pub const SETS_DIR: &str = ".ferris-idea/change-sets";
/// Change sets kept per workspace (behaviour 4).
pub const MAX_SETS: usize = 200;
/// Files per change set (behaviour 7).
pub const MAX_FILES: usize = 500;
/// Largest proposed text, the local history's limit (behaviour 7).
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
/// Longest text of the metadata, in characters (behaviour 2).
pub const MAX_META_CHARS: usize = 200;

/// What went wrong with a change set.
#[derive(Debug, thiserror::Error)]
pub enum ChangeSetError {
    /// The request does not fit the set; the text says why.
    #[error("{0}")]
    Invalid(String),
    /// Reading or writing the set's file failed.
    #[error("change sets: {0}")]
    Io(String),
}

fn invalid(why: impl Into<String>) -> ChangeSetError {
    ChangeSetError::Invalid(why.into())
}

fn io(e: impl std::fmt::Display) -> ChangeSetError {
    ChangeSetError::Io(e.to_string())
}

/// Milliseconds since the epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// SHA-256 of a text, in hex (behaviour 5).
pub fn text_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What the work cost, as the agent reports it (behaviour 2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Input tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Money spent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    /// Currency of `amount`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// Task, agent, model and cost of a set (behaviour 2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// The task the set belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The agent that proposed it.
    pub agent: String,
    /// The model the agent used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// What the work cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

/// `text` cut to [`MAX_META_CHARS`] characters.
pub fn cut(text: &str) -> String {
    text.chars().take(MAX_META_CHARS).collect()
}

impl Meta {
    /// The metadata with every text cut to [`MAX_META_CHARS`] characters (behaviour 2).
    pub fn cut(self) -> Self {
        Self {
            task: self.task.as_deref().map(cut),
            agent: cut(&self.agent),
            model: self.model.as_deref().map(cut),
            cost: self.cost.map(|c| Cost {
                currency: c.currency.as_deref().map(cut),
                ..c
            }),
        }
    }
}

/// The Git working tree a set was made in (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    /// Root folder of the working tree.
    pub root: String,
    /// Current branch, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Whether it is a linked worktree rather than the main working tree.
    pub linked: bool,
}

/// Where a file of a set stands (behaviour 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    /// Proposed, not decided.
    Pending,
    /// Written to the workspace.
    Applied,
    /// Rejected.
    Rejected,
    /// Applied, then taken back.
    Undone,
}

impl FileStatus {
    /// The status's name in answers.
    pub fn name(self) -> &'static str {
        match self {
            FileStatus::Pending => "pending",
            FileStatus::Applied => "applied",
            FileStatus::Rejected => "rejected",
            FileStatus::Undone => "undone",
        }
    }
}

/// What applying a file wrote (behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    /// The hunks applied.
    pub hunks: Vec<usize>,
    /// Hash of the text written; `None` when the file was deleted.
    pub written_hash: Option<String>,
    /// The local-history revision of the state before; `None` when the file did not exist.
    pub before_revision: Option<usize>,
    /// When it was applied, in ms.
    pub time: u64,
}

/// One file of a set (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEdit {
    /// Workspace-relative, `/`-separated.
    pub path: String,
    /// The text when the first edit was proposed; `None` when the file did not exist.
    pub base: Option<String>,
    /// Hash of `base`.
    pub base_hash: Option<String>,
    /// The proposed text; `None` when the file is to be deleted.
    pub proposed: Option<String>,
    /// Where the file stands.
    pub status: FileStatus,
    /// Hunks rejected while the file is pending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_hunks: Vec<usize>,
    /// What applying wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<Applied>,
}

/// One hunk of a file (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    /// 1-based, in file order.
    pub number: usize,
    /// Added, modified or deleted lines.
    pub kind: LineChangeKind,
    /// First old line (1-based).
    pub old_start: usize,
    /// Last old line (inclusive; `old_start - 1` when empty).
    pub old_end: usize,
    /// First new line (1-based).
    pub new_start: usize,
    /// Last new line (inclusive; `new_start - 1` when empty).
    pub new_end: usize,
    /// The old lines.
    pub old_lines: Vec<String>,
    /// The new lines.
    pub new_lines: Vec<String>,
}

fn lines_of(text: &str, from: usize, to: usize) -> Vec<String> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
        .skip(from.saturating_sub(1))
        .take((to + 1).saturating_sub(from))
        .collect()
}

impl FileEdit {
    /// The hunks from the base to the proposed text (behaviour 8): a new or deleted file has one.
    pub fn hunks(&self) -> Vec<Hunk> {
        let whole = |kind, text: &str| {
            let lines: Vec<String> = text.lines().map(str::to_owned).collect();
            let n = lines.len();
            let (old, new) = match kind {
                LineChangeKind::Added => (Vec::new(), lines),
                _ => (lines, Vec::new()),
            };
            let (old_end, new_end) = match kind {
                LineChangeKind::Added => (0, n),
                _ => (n, 0),
            };
            Hunk {
                number: 1,
                kind,
                old_start: 1,
                old_end,
                new_start: 1,
                new_end,
                old_lines: old,
                new_lines: new,
            }
        };
        match (&self.base, &self.proposed) {
            (None, Some(p)) => vec![whole(LineChangeKind::Added, p)],
            (Some(b), None) => vec![whole(LineChangeKind::Deleted, b)],
            (None, None) => Vec::new(),
            (Some(b), Some(p)) => line_changes(b, p)
                .into_iter()
                .enumerate()
                .map(|(i, c)| Hunk {
                    number: i + 1,
                    kind: c.kind,
                    old_start: c.old_start,
                    old_end: c.old_end,
                    new_start: c.new_start,
                    new_end: c.new_end,
                    old_lines: lines_of(b, c.old_start, c.old_end),
                    new_lines: lines_of(p, c.new_start, c.new_end),
                })
                .collect(),
        }
    }

    /// The hunks applying would take: `chosen`, else every hunk; never a rejected one
    /// (behaviour 10). Unknown or rejected numbers in `chosen` are refused.
    pub fn chosen_hunks(&self, chosen: Option<&[usize]>) -> Result<Vec<usize>, ChangeSetError> {
        let count = self.hunks().len();
        let Some(chosen) = chosen else {
            return Ok((1..=count)
                .filter(|n| !self.rejected_hunks.contains(n))
                .collect());
        };
        let mut out: Vec<usize> = Vec::new();
        for &n in chosen {
            if n == 0 || n > count {
                return Err(invalid(format!(
                    "{}: no hunk {n} (the file has {count})",
                    self.path
                )));
            }
            if self.rejected_hunks.contains(&n) {
                return Err(invalid(format!("{}: hunk {n} was rejected", self.path)));
            }
            if !out.contains(&n) {
                out.push(n);
            }
        }
        out.sort_unstable();
        Ok(out)
    }

    /// The text applying `hunks` writes: the base with only those changes (behaviour 10); `None`
    /// for a deletion. A new or deleted file is taken whole.
    pub fn compose(&self, hunks: &[usize]) -> Result<Option<String>, ChangeSetError> {
        match (&self.base, &self.proposed) {
            (Some(b), Some(p)) => {
                let changes = line_changes(b, p);
                let mut picked: Vec<_> = hunks
                    .iter()
                    .map(|&n| {
                        changes
                            .get(n.wrapping_sub(1))
                            .cloned()
                            .ok_or_else(|| invalid(format!("{}: no hunk {n}", self.path)))
                    })
                    .collect::<Result<_, _>>()?;
                // From the bottom up, so the old line numbers of the rest stay valid.
                picked.sort_by_key(|a| std::cmp::Reverse((a.old_start, a.old_end)));
                let mut text = b.clone();
                for change in &picked {
                    text = apply_change(&text, p, change);
                }
                Ok(Some(text))
            }
            (None, Some(p)) => Ok(Some(p.clone())),
            _ => Ok(None),
        }
    }
}

/// One event of a set (behaviour 19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// When, in ms.
    pub time: u64,
    /// `begin`, `edit`, `apply`, `reject` or `undo`.
    pub action: String,
    /// Who did it.
    pub agent: String,
    /// The files concerned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

/// An edit of one file (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// The file's whole new text.
    Text(String),
    /// Delete the file.
    Delete,
}

/// What a proposal did to the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proposed {
    /// The file is in the set with this many hunks.
    Hunks(usize),
    /// The proposal equals the base: the file is not (or no longer) in the set.
    Dropped,
}

/// A change set (behaviour 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// `cs-…`.
    pub id: String,
    /// Its name.
    pub name: String,
    /// When it was begun, in ms.
    pub created: u64,
    /// The workspace folder it belongs to.
    pub root: String,
    /// Task, agent, model, cost.
    pub meta: Meta,
    /// The Git working tree it was made in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Worktree>,
    /// Its files, in the order first proposed.
    pub files: Vec<FileEdit>,
    /// What happened to it.
    pub events: Vec<Event>,
}

/// A workspace-relative path a set may hold (behaviour 7): no `..`, not absolute, not in
/// `.ferris-idea/` or a `.git` folder. Returns it `/`-separated.
pub fn check_path(path: &str) -> Result<String, ChangeSetError> {
    let p = Path::new(path);
    let refused = || {
        invalid(format!(
            "{path} is not a plain workspace-relative path outside .ferris-idea/ and .git/"
        ))
    };
    if path.is_empty() || p.is_absolute() {
        return Err(refused());
    }
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_str().ok_or_else(refused)?;
                if s == ".git" || (parts.is_empty() && s == ".ferris-idea") {
                    return Err(refused());
                }
                parts.push(s);
            }
            Component::CurDir => {}
            _ => return Err(refused()),
        }
    }
    if parts.is_empty() {
        return Err(refused());
    }
    Ok(parts.join("/"))
}

/// Whether `id` is a set id (behaviour 1), so it can name a file.
pub fn check_id(id: &str) -> Result<(), ChangeSetError> {
    let ok = id.len() <= 64
        && id.strip_prefix("cs-").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if ok {
        Ok(())
    } else {
        Err(invalid(format!("`{id}` is not a change set id")))
    }
}

impl ChangeSet {
    /// A new set without files; its id is given by [`Store::create`].
    pub fn new(name: &str, root: &str, meta: Meta, worktree: Option<Worktree>) -> Self {
        let time = now_ms();
        let meta = meta.cut();
        Self {
            id: String::new(),
            name: cut(name),
            created: time,
            root: root.to_owned(),
            events: vec![Event {
                time,
                action: "begin".into(),
                agent: meta.agent.clone(),
                files: Vec::new(),
            }],
            meta,
            worktree,
            files: Vec::new(),
        }
    }

    /// The file `path` of the set.
    pub fn file(&self, path: &str) -> Option<&FileEdit> {
        self.files.iter().find(|f| f.path == path)
    }

    /// The file `path` of the set, to change.
    pub fn file_mut(&mut self, path: &str) -> Option<&mut FileEdit> {
        self.files.iter_mut().find(|f| f.path == path)
    }

    /// The text a replacement in `path` works on when the set already proposes one (behaviour 6):
    /// `Some(None)` for a proposed deletion.
    pub fn proposed_text(&self, path: &str) -> Option<Option<&str>> {
        self.file(path).map(|f| f.proposed.as_deref())
    }

    /// Proposes `edit` of `path`, whose text is `current` now (behaviours 5–7).
    pub fn propose(
        &mut self,
        path: &str,
        current: Option<String>,
        edit: Edit,
    ) -> Result<Proposed, ChangeSetError> {
        let path = check_path(path)?;
        let proposed = match edit {
            Edit::Text(t) => {
                if t.len() > MAX_TEXT_BYTES {
                    return Err(invalid(format!(
                        "{path}: the proposed text is larger than {MAX_TEXT_BYTES} bytes"
                    )));
                }
                Some(t)
            }
            Edit::Delete => None,
        };
        if let Some(i) = self.files.iter().position(|f| f.path == path) {
            let file = &mut self.files[i];
            if file.status != FileStatus::Pending {
                return Err(invalid(format!(
                    "{path} is {} in this set; begin a new set for further edits",
                    file.status.name()
                )));
            }
            if proposed.is_none() && file.base.is_none() {
                return Err(invalid(format!("{path} does not exist; nothing to delete")));
            }
            file.proposed = proposed;
            file.rejected_hunks.clear();
            if file.proposed == file.base {
                self.files.remove(i);
                return Ok(Proposed::Dropped);
            }
            return Ok(Proposed::Hunks(self.files[i].hunks().len()));
        }
        if proposed.is_none() && current.is_none() {
            return Err(invalid(format!("{path} does not exist; nothing to delete")));
        }
        if proposed == current {
            return Ok(Proposed::Dropped);
        }
        if self.files.len() >= MAX_FILES {
            return Err(invalid(format!(
                "the set already holds {MAX_FILES} files; begin a new set"
            )));
        }
        let file = FileEdit {
            base_hash: current.as_deref().map(text_hash),
            base: current,
            path,
            proposed,
            status: FileStatus::Pending,
            rejected_hunks: Vec::new(),
            applied: None,
        };
        let hunks = file.hunks().len();
        self.files.push(file);
        Ok(Proposed::Hunks(hunks))
    }

    /// Notes an event (behaviour 19).
    pub fn note(&mut self, action: &str, agent: &str, files: Vec<String>) {
        self.events.push(Event {
            time: now_ms(),
            action: action.into(),
            agent: cut(agent),
            files,
        });
    }

    /// Whether a file is still pending.
    pub fn has_pending(&self) -> bool {
        self.files.iter().any(|f| f.status == FileStatus::Pending)
    }

    /// Files per status.
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut out = BTreeMap::new();
        for f in &self.files {
            *out.entry(f.status.name()).or_insert(0) += 1;
        }
        out
    }

    /// Rejects (behaviour 16): with `hunks` for a file only those hunks, else the named files,
    /// else every pending file. Returns the files concerned.
    pub fn reject(
        &mut self,
        files: Option<&[String]>,
        hunks: &BTreeMap<String, Vec<usize>>,
    ) -> Result<Vec<String>, ChangeSetError> {
        let mut named: Vec<String> = files.unwrap_or_default().to_vec();
        named.extend(hunks.keys().cloned());
        if named.is_empty() {
            named = self
                .files
                .iter()
                .filter(|f| f.status == FileStatus::Pending)
                .map(|f| f.path.clone())
                .collect();
        }
        let mut done = Vec::new();
        for path in named {
            let path = check_path(&path)?;
            let file = self
                .file_mut(&path)
                .ok_or_else(|| invalid(format!("{path} is not in the set")))?;
            if file.status != FileStatus::Pending {
                return Err(invalid(format!("{path} is {}", file.status.name())));
            }
            match hunks.get(&path) {
                Some(numbers) => {
                    let chosen = file.chosen_hunks(Some(numbers))?;
                    file.rejected_hunks.extend(chosen);
                    file.rejected_hunks.sort_unstable();
                    file.rejected_hunks.dedup();
                    if file.rejected_hunks.len() >= file.hunks().len() {
                        file.status = FileStatus::Rejected;
                    }
                }
                None => file.status = FileStatus::Rejected,
            }
            if !done.contains(&path) {
                done.push(path);
            }
        }
        Ok(done)
    }
}

/// The change sets of one workspace (behaviour 1).
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The store of the workspace at `root`.
    pub fn of(root: &Path) -> Self {
        Self {
            dir: root.join(SETS_DIR),
        }
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Reads set `id`.
    pub fn load(&self, id: &str) -> Result<ChangeSet, ChangeSetError> {
        check_id(id)?;
        let text = match std::fs::read_to_string(self.file(id)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(invalid(format!("no change set `{id}`")));
            }
            Err(e) => return Err(io(e)),
        };
        serde_json::from_str(&text).map_err(|e| io(format!("{id}: {e}")))
    }

    /// Writes `set` (replacing its file as a whole).
    pub fn save(&self, set: &ChangeSet) -> Result<(), ChangeSetError> {
        check_id(&set.id)?;
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let text = serde_json::to_string_pretty(set).map_err(io)?;
        let tmp = self.dir.join(format!(".{}.tmp", set.id));
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, self.file(&set.id)).map_err(io)
    }

    /// Every set, oldest first; files that cannot be read are left out.
    pub fn list(&self) -> Vec<ChangeSet> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<ChangeSet> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(".json")?;
                self.load(id).ok()
            })
            .collect();
        out.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
        out
    }

    /// Makes room for one more set (behaviour 4): removes the oldest sets with nothing pending
    /// while [`MAX_SETS`] are kept; refuses when they all have pending files. Returns the ids
    /// removed.
    pub fn make_room(&self) -> Result<Vec<String>, ChangeSetError> {
        let sets = self.list();
        let mut over = (sets.len() + 1).saturating_sub(MAX_SETS);
        let mut removed = Vec::new();
        for set in &sets {
            if over == 0 {
                break;
            }
            if !set.has_pending() {
                std::fs::remove_file(self.file(&set.id)).map_err(io)?;
                removed.push(set.id.clone());
                over -= 1;
            }
        }
        if over > 0 {
            return Err(invalid(format!(
                "{MAX_SETS} change sets with pending files are kept; apply or reject some first"
            )));
        }
        Ok(removed)
    }

    /// Gives `set` a new id and writes it (behaviour 1).
    pub fn create(&self, set: &mut ChangeSet) -> Result<(), ChangeSetError> {
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let stamp = radix36(set.created);
        for n in 0..1000 {
            let id = format!("cs-{stamp}-{n}");
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.file(&id))
            {
                Ok(_) => {
                    set.id = id;
                    return self.save(set);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io(e)),
            }
        }
        Err(io("no free change set id"))
    }
}

fn radix36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n";

    fn set() -> ChangeSet {
        ChangeSet::new(
            "fix",
            "/w",
            Meta {
                agent: "agent".into(),
                ..Meta::default()
            },
            None,
        )
    }

    /// SPEC-0340 AK-1: hunks in file order; two of three applied give the base with exactly those.
    #[test]
    fn hunks_numbered_and_partial_apply() {
        let mut s = set();
        let proposed = "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\nnine\nten\n";
        assert_eq!(
            s.propose("src/a.txt", Some(BASE.into()), Edit::Text(proposed.into()))
                .unwrap(),
            Proposed::Hunks(3)
        );
        let f = s.file("src/a.txt").unwrap();
        let hunks = f.hunks();
        let kinds: Vec<_> = hunks.iter().map(|h| (h.number, h.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (1, LineChangeKind::Modified),
                (2, LineChangeKind::Deleted),
                (3, LineChangeKind::Added)
            ]
        );
        assert_eq!((hunks[0].old_start, hunks[0].old_end), (1, 1));
        assert_eq!(hunks[0].old_lines, vec!["one"]);
        assert_eq!(hunks[0].new_lines, vec!["ONE"]);
        assert_eq!((hunks[1].old_start, hunks[1].old_end), (8, 8));
        assert_eq!(hunks[2].new_lines, vec!["ten"]);
        assert_eq!(
            f.compose(&[1, 3]).unwrap().unwrap(),
            "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n"
        );
        assert_eq!(
            f.compose(&[2]).unwrap().unwrap(),
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\nnine\n"
        );
        assert_eq!(f.compose(&[1, 2, 3]).unwrap().unwrap(), proposed);
        assert_eq!(f.compose(&[]).unwrap().unwrap(), BASE);
        assert_eq!(f.base_hash.as_deref(), Some(text_hash(BASE).as_str()));
        // Rejected hunks are never chosen.
        let mut chosen_without = f.clone();
        chosen_without.rejected_hunks = vec![2];
        assert_eq!(chosen_without.chosen_hunks(None).unwrap(), vec![1, 3]);
        assert!(chosen_without.chosen_hunks(Some(&[2])).is_err());
        assert!(chosen_without.chosen_hunks(Some(&[4])).is_err());
    }

    /// SPEC-0340 AK-1: a new file and a deletion each have one hunk, taken whole.
    #[test]
    fn new_and_deleted_files_have_one_hunk() {
        let mut s = set();
        s.propose("new.txt", None, Edit::Text("a\nb\n".into()))
            .unwrap();
        s.propose("old.txt", Some("x\n".into()), Edit::Delete)
            .unwrap();
        let new = s.file("new.txt").unwrap();
        assert_eq!(new.hunks().len(), 1);
        assert_eq!(new.hunks()[0].kind, LineChangeKind::Added);
        assert_eq!(new.compose(&[1]).unwrap().as_deref(), Some("a\nb\n"));
        assert_eq!(new.base_hash, None);
        let old = s.file("old.txt").unwrap();
        assert_eq!(old.hunks()[0].kind, LineChangeKind::Deleted);
        assert_eq!(old.hunks()[0].old_lines, vec!["x"]);
        assert_eq!(old.compose(&[1]).unwrap(), None);
        assert!(s.propose("missing.txt", None, Edit::Delete).is_err());
    }

    /// SPEC-0340 AK-1: an edit replaces the proposal; back to the base drops the file; a settled
    /// file takes no more edits.
    #[test]
    fn edits_replace_and_drop_back_to_base() {
        let mut s = set();
        s.propose("a.txt", Some("a\n".into()), Edit::Text("b\n".into()))
            .unwrap();
        assert_eq!(s.proposed_text("a.txt"), Some(Some("b\n")));
        s.file_mut("a.txt").unwrap().rejected_hunks = vec![1];
        s.propose("a.txt", Some("ignored".into()), Edit::Text("c\n".into()))
            .unwrap();
        let f = s.file("a.txt").unwrap();
        assert_eq!(
            f.base.as_deref(),
            Some("a\n"),
            "the base stays the first one"
        );
        assert!(f.rejected_hunks.is_empty());
        assert_eq!(
            s.propose("a.txt", None, Edit::Text("a\n".into())).unwrap(),
            Proposed::Dropped
        );
        assert!(s.files.is_empty());
        assert_eq!(
            s.propose("b.txt", Some("same".into()), Edit::Text("same".into()))
                .unwrap(),
            Proposed::Dropped
        );
        s.propose("c.txt", Some("1\n".into()), Edit::Text("2\n".into()))
            .unwrap();
        s.reject(Some(&["c.txt".into()]), &BTreeMap::new()).unwrap();
        assert_eq!(s.file("c.txt").unwrap().status, FileStatus::Rejected);
        assert!(
            s.propose("c.txt", Some("1\n".into()), Edit::Text("3\n".into()))
                .is_err()
        );
    }

    /// SPEC-0340 AK-1: ids and paths that leave the folder are refused.
    #[test]
    fn ids_and_paths_stay_inside() {
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            ".ferris-idea/agent-log.jsonl",
            ".git/config",
            "sub/.git/hooks/pre-commit",
        ] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
        assert_eq!(check_path("./src/a.rs").unwrap(), "src/a.rs");
        assert_eq!(
            check_path("docs/.ferris-idea-notes").unwrap(),
            "docs/.ferris-idea-notes"
        );
        for bad in ["cs-", "cs-../x", "x-1", "cs-A", "cs-1/2"] {
            assert!(check_id(bad).is_err(), "{bad}");
        }
        assert!(check_id("cs-mabc12-0").is_ok());
        let dir = std::env::temp_dir().join(format!("ferris-idea-cs-ids-{}", std::process::id()));
        assert!(Store::of(&dir).load("../../etc/passwd").is_err());
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-cs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// SPEC-0340 AK-2: metadata cut to 200 characters, the set stored and read back with events.
    #[test]
    fn metadata_cut_and_stored_with_events() {
        let dir = temp("store");
        let store = Store::of(&dir);
        let mut s = ChangeSet::new(
            &"n".repeat(300),
            "/w",
            Meta {
                task: Some("t".repeat(250)),
                agent: "claude".into(),
                model: Some("m".into()),
                cost: Some(Cost {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    amount: Some(0.25),
                    currency: Some("USD".into()),
                }),
            },
            Some(Worktree {
                root: "/w".into(),
                branch: Some("main".into()),
                linked: false,
            }),
        );
        assert_eq!(s.name.chars().count(), MAX_META_CHARS);
        assert_eq!(s.meta.task.as_ref().unwrap().len(), MAX_META_CHARS);
        store.create(&mut s).unwrap();
        assert!(s.id.starts_with("cs-"));
        s.propose("a.txt", Some("a".into()), Edit::Text("b".into()))
            .unwrap();
        s.note("edit", "claude", vec!["a.txt".into()]);
        store.save(&s).unwrap();
        let back = store.load(&s.id).unwrap();
        assert_eq!(back, s);
        assert_eq!(
            back.events
                .iter()
                .map(|e| e.action.as_str())
                .collect::<Vec<_>>(),
            vec!["begin", "edit"]
        );
        let mut other = set();
        store.create(&mut other).unwrap();
        assert_ne!(other.id, s.id);
        assert_eq!(store.list().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0340 AK-2: with 200 sets kept the oldest without pending files make room; when all
    /// are pending a new one is refused.
    #[test]
    fn oldest_settled_sets_make_room() {
        let dir = temp("room");
        let store = Store::of(&dir);
        for i in 0..MAX_SETS {
            let mut s = set();
            s.created = 1000 + i as u64;
            if i != 3 && i != 7 {
                s.propose("a.txt", Some("a".into()), Edit::Text("b".into()))
                    .unwrap();
            }
            store.create(&mut s).unwrap();
        }
        let removed = store.make_room().unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(store.list().len(), MAX_SETS - 1);
        assert!(store.list().iter().all(|s| s.created != 1003));
        let mut s = set();
        s.created = 5000;
        s.propose("a.txt", Some("a".into()), Edit::Text("b".into()))
            .unwrap();
        store.create(&mut s).unwrap();
        assert_eq!(store.make_room().unwrap().len(), 1);
        let mut s = set();
        s.created = 6000;
        s.propose("a.txt", Some("a".into()), Edit::Text("b".into()))
            .unwrap();
        store.create(&mut s).unwrap();
        assert!(store.make_room().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
