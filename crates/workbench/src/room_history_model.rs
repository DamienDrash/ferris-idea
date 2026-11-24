//! The project room's history as the Room History dialog shows it (`SPEC-0323` v9 behaviours
//! 61–65): the journal's entries as rows (a group, or a person's typing joined into an editing
//! phase) newest first with author, time and summary, document names followed through later
//! renames, the diffs of the selected row, which files Restore this version brings back, who may
//! restore and what the dialog says before and after. Pure: the window feeds it the room's
//! answers and the time; nothing here touches a window or the network.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Value, json};

use crate::room_comments_model::{shown_text, time_text};
use crate::room_model::shown_name;

/// Room History… in the File menu and Find Action (behaviour 61).
pub const SHOW_HISTORY: &str = "Room.ShowHistory";
/// The status line when there is no room to show (behaviour 61).
pub const HISTORY_NEEDS_ROOM: &str = "Room history needs a project room (File › Share Project…)";
/// What a person below `editor` reads instead of Restore (behaviour 65).
pub const RESTORE_NEEDS_ROLE: &str = "Restoring needs the role editor or higher in this room.";
/// How many of the newest journal entries the dialog asks for (behaviour 62).
pub const LOAD_ENTRIES: u64 = 1000;
/// The longest pause within one editing phase, in milliseconds (behaviour 62).
pub const PHASE_GAP_MS: u64 = 60_000;
/// Files of one row whose diff is shown (behaviour 63).
pub const MAX_DIFF_FILES: usize = 20;
/// Lines of one file's diff (behaviour 63).
pub const MAX_DIFF_LINES: usize = 400;
/// Unchanged lines around each change (behaviour 63).
pub const CONTEXT: usize = 2;
/// Names a summary lists before "and N more" (behaviour 62).
pub const NAMES_SHOWN: usize = 3;
/// The author shown for changes from outside the room (behaviour 62).
pub const OUTSIDE: &str = "Outside the room";

/// What one journal entry did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A document's text changed.
    Text(String),
    /// A file was created.
    Created(String),
    /// A folder was created.
    CreatedDir(String),
    /// A file or folder was renamed.
    Renamed {
        /// The former path.
        from: String,
        /// The new path.
        to: String,
    },
    /// A file was deleted.
    Deleted(String),
    /// A file changed on disk outside the room (not open in it).
    Changed(String),
    /// A named version.
    Version(String),
}

impl Change {
    /// The paths the change touches at its time.
    fn paths(&self) -> Vec<&str> {
        match self {
            Self::Text(p) | Self::Created(p) | Self::CreatedDir(p) | Self::Deleted(p) => vec![p],
            Self::Changed(p) => vec![p],
            Self::Renamed { from, to } => vec![from, to],
            Self::Version(_) => Vec::new(),
        }
    }

    /// How the summary says it.
    fn phrase(&self) -> String {
        match self {
            Self::Text(p) => format!("Edited {}", shown_text(p)),
            Self::Created(p) => format!("Created {}", shown_text(p)),
            Self::CreatedDir(p) => format!("Created folder {}", shown_text(p)),
            Self::Renamed { from, to } => {
                format!("Renamed {} to {}", shown_text(from), shown_text(to))
            }
            Self::Deleted(p) => format!("Deleted {}", shown_text(p)),
            Self::Changed(p) => format!("Changed {} outside the room", shown_text(p)),
            Self::Version(n) => format!("Version “{}”", shown_text(n)),
        }
    }
}

/// One entry of the journal as `room.history` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Room-wide position.
    pub seq: u64,
    /// Unix milliseconds.
    pub time: u64,
    /// Who: the person's id, `disk`, or for a guest the neutral name.
    pub actor: String,
    /// The name to show.
    pub author: String,
    /// The undo group.
    pub group: u64,
    /// What it did.
    pub change: Change,
}

impl Entry {
    /// An entry of `room.history`; `None` when it is malformed.
    pub fn from_json(v: &Value) -> Option<Self> {
        let text = |k: &str| v[k].as_str().map(str::to_owned);
        let path = || text("path").filter(|p| !p.is_empty());
        let change = match v["kind"].as_str()? {
            "text" => Change::Text(text("doc").filter(|d| !d.is_empty())?),
            "file" => match v["change"].as_str()? {
                "created" => Change::Created(path()?),
                "created-dir" => Change::CreatedDir(path()?),
                "renamed" => Change::Renamed {
                    from: text("from").filter(|p| !p.is_empty())?,
                    to: path()?,
                },
                "deleted" => Change::Deleted(path()?),
                "changed" => Change::Changed(path()?),
                _ => return None,
            },
            "version" => Change::Version(text("name")?),
            _ => return None,
        };
        let author = text("author").unwrap_or_default();
        Some(Self {
            seq: v["seq"].as_u64().filter(|s| *s > 0)?,
            time: v["time"].as_u64()?,
            actor: text("actor").unwrap_or_else(|| author.clone()),
            author,
            group: v["group"].as_u64()?,
            change,
        })
    }

    /// The name the row shows (behaviour 62).
    fn shown_author(&self) -> String {
        if self.actor == "disk" {
            OUTSIDE.to_owned()
        } else {
            shown_name(&self.author)
        }
    }
}

/// `name` after a rename of `from` to `to`: the same path, or one below a renamed folder.
fn moved(name: &str, from: &str, to: &str) -> Option<String> {
    if name == from {
        return Some(to.to_owned());
    }
    name.strip_prefix(from)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(|rest| format!("{to}/{rest}"))
}

/// Up to [`NAMES_SHOWN`] of `items`, then "and N more".
fn listed(items: &[String]) -> String {
    let shown = items[..items.len().min(NAMES_SHOWN)].join(", ");
    if items.len() > NAMES_SHOWN {
        format!("{shown} and {} more", items.len() - NAMES_SHOWN)
    } else {
        shown
    }
}

/// One row of the list: a group of the journal, or an editing phase of several typing groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The row's id: its first journal position.
    pub id: u64,
    /// The last journal position of the row; a restore goes to it.
    pub last: u64,
    /// When its newest entry happened.
    pub time: u64,
    /// Who made it (the key the room gives).
    actor: String,
    /// The name shown.
    pub author: String,
    /// The groups it holds.
    groups: Vec<u64>,
    /// What it did, each once, in order.
    pub changes: Vec<Change>,
}

impl Row {
    fn of(e: &Entry) -> Self {
        Self {
            id: e.seq,
            last: e.seq,
            time: e.time,
            actor: e.actor.clone(),
            author: e.shown_author(),
            groups: vec![e.group],
            changes: vec![e.change.clone()],
        }
    }

    fn take(&mut self, e: &Entry) {
        self.last = self.last.max(e.seq);
        self.time = self.time.max(e.time);
        if !self.groups.contains(&e.group) {
            self.groups.push(e.group);
        }
        if !self.changes.contains(&e.change) {
            self.changes.push(e.change.clone());
        }
    }

    /// Only typing in `doc`, the only thing an editing phase may grow by.
    fn is_typing_in(&self, doc: &str) -> bool {
        self.changes
            .iter()
            .all(|c| matches!(c, Change::Text(d) if d == doc))
    }

    /// The version's name for a version row.
    pub fn version(&self) -> Option<&str> {
        self.changes.iter().find_map(|c| match c {
            Change::Version(n) => Some(n.as_str()),
            _ => None,
        })
    }

    /// The summary (behaviour 62).
    pub fn summary(&self) -> String {
        if let Some(name) = self.version() {
            return Change::Version(name.to_owned()).phrase();
        }
        let edited: Vec<String> = self
            .changes
            .iter()
            .filter_map(|c| match c {
                Change::Text(d) => Some(shown_text(d)),
                _ => None,
            })
            .collect();
        if edited.len() == self.changes.len() {
            let mut s = format!("Edited {}", listed(&edited));
            if self.groups.len() > 1 {
                s.push_str(&format!(" ({} changes)", self.groups.len()));
            }
            return s;
        }
        let phrases: Vec<String> = self.changes.iter().map(Change::phrase).collect();
        listed(&phrases)
    }

    /// The line under the summary: author and time.
    pub fn byline(&self, now: u64) -> String {
        format!("{} · {}", self.author, time_text(self.time, now))
    }
}

/// One line of a diff as the dialog draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// A line: `−`, `+` or a space, and its text made plain.
    Line(char, String),
    /// "⋯" between two pieces.
    Gap,
}

/// The unified diff of `before` and `after` (behaviour 63): [`CONTEXT`] lines around each
/// change, gaps between pieces, at most [`MAX_DIFF_LINES`] lines; and the added and removed counts.
pub fn diff_lines(before: &str, after: &str) -> (Vec<DiffLine>, usize, usize) {
    use ferris_idea_vcs::LineChangeKind as K;
    let rows = ferris_idea_vcs::unified_rows(&ferris_idea_vcs::side_by_side(before, after));
    let keep: BTreeSet<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind.is_some())
        .flat_map(|(i, _)| i.saturating_sub(CONTEXT)..(i + CONTEXT + 1).min(rows.len()))
        .collect();
    let count = |k| rows.iter().filter(|r| r.kind == Some(k)).count();
    let mut out = Vec::new();
    let mut last: Option<usize> = None;
    for i in keep {
        if out.len() >= MAX_DIFF_LINES {
            out.push(DiffLine::Gap);
            break;
        }
        if last.is_some_and(|l| i > l + 1) {
            out.push(DiffLine::Gap);
        }
        last = Some(i);
        let sign = match rows[i].kind {
            Some(K::Added) => '+',
            Some(K::Deleted) => '−',
            _ => ' ',
        };
        out.push(DiffLine::Line(sign, shown_text(&rows[i].text)));
    }
    (out, count(K::Added), count(K::Deleted))
}

/// What one file of the selected row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    /// The room is asked.
    Loading,
    /// The diff.
    Shown {
        /// Its lines.
        lines: Vec<DiffLine>,
        /// Added lines.
        added: usize,
        /// Removed lines.
        removed: usize,
    },
    /// One line instead of a diff: what happened, or why no diff can be shown.
    Said(String),
}

/// One file of the selected row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// The file's name now.
    pub doc: String,
    /// `room.diff` parameters, when the file has a diff to ask for.
    pub ask: Option<Value>,
    /// The row created the file: its text before is empty.
    created: bool,
    /// What it shows.
    pub state: FileState,
}

/// The Room History dialog.
#[derive(Debug, Clone, Default)]
pub struct HistoryModel {
    entries: Vec<Entry>,
    rows: Vec<Row>,
    selected: Option<u64>,
    role: Option<String>,
    known: bool,
    files: Vec<FileDiff>,
    /// The row the files belong to and its last position then.
    files_of: Option<(u64, u64)>,
    confirming: bool,
    /// The outcome of the last restore.
    pub note: Option<String>,
    /// The room's reason when a call failed.
    pub error: Option<String>,
}

/// Whether `role` may restore (behaviours 64, 65).
pub fn can_restore(role: Option<&str>) -> bool {
    matches!(role, Some("editor" | "runner" | "admin"))
}

impl HistoryModel {
    /// Whether the history arrived.
    pub fn known(&self) -> bool {
        self.known
    }

    /// The rows, newest first.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The selected row.
    pub fn selected(&self) -> Option<&Row> {
        let id = self.selected?;
        self.rows.iter().find(|r| r.id == id)
    }

    /// The window's own role, from `room.status`.
    pub fn set_role(&mut self, status: &Value, own: &str) {
        self.role = status["participants"]
            .as_array()
            .and_then(|l| l.iter().find(|p| p["client"] == own))
            .and_then(|p| p["role"].as_str())
            .map(str::to_owned);
    }

    /// Whether Restore this version is offered.
    pub fn can_restore(&self) -> bool {
        can_restore(self.role.as_deref())
    }

    /// The parameters of `room.history` after `room.status` said where the journal stands.
    pub fn history_params(status: &Value) -> Value {
        let seq = status["seq"].as_u64().unwrap_or(0);
        json!({ "since": seq.saturating_sub(LOAD_ENTRIES), "limit": LOAD_ENTRIES })
    }

    /// The answer of `room.history` (behaviour 62). Keeps the selection while its row is there,
    /// else selects the newest row. Returns whether the selected row's files must be asked again.
    pub fn set_history(&mut self, answer: &Value) -> bool {
        let Some(list) = answer["entries"].as_array() else {
            return false;
        };
        let mut entries: Vec<Entry> = list.iter().filter_map(Entry::from_json).collect();
        entries.sort_by_key(|e| e.seq);
        entries.dedup_by_key(|e| e.seq);
        self.rows = rows_of(&entries);
        self.entries = entries;
        self.known = true;
        if self.selected().is_none() {
            self.selected = self.rows.first().map(|r| r.id);
            self.confirming = false;
        }
        let now = self.selected().map(|r| (r.id, r.last));
        now != self.files_of || now.is_none() && !self.files.is_empty()
    }

    /// Selects the row `id`. Returns whether its files must be asked.
    pub fn select(&mut self, id: u64) -> bool {
        if !self.rows.iter().any(|r| r.id == id) || self.selected == Some(id) {
            return false;
        }
        self.selected = Some(id);
        self.confirming = false;
        self.note = None;
        true
    }

    /// `name` (as it was at journal position `at`) as the file is called now.
    pub fn name_now(&self, name: &str, at: u64) -> String {
        let mut name = name.to_owned();
        for e in self.entries.iter().filter(|e| e.seq > at) {
            if let Change::Renamed { from, to } = &e.change
                && let Some(n) = moved(&name, from, to)
            {
                name = n;
            }
        }
        name
    }

    /// The files of the selected row with what to ask the room (behaviour 63), set as loading;
    /// the window then asks `room.diff` for every file with `ask`.
    pub fn start_files(&mut self) -> &[FileDiff] {
        let Some(row) = self.selected().cloned() else {
            self.files.clear();
            self.files_of = None;
            return &self.files;
        };
        let mut files: Vec<FileDiff> = Vec::new();
        let said = |doc: String, text: String| FileDiff {
            doc,
            ask: None,
            created: false,
            state: FileState::Said(text),
        };
        if let Some(name) = row.version() {
            files.push(said(
                String::new(),
                format!(
                    "Version “{}” marks this point. Restore this version brings every file changed after it back to how it was here.",
                    shown_text(name)
                ),
            ));
        }
        let created: Vec<&str> = row
            .changes
            .iter()
            .filter_map(|c| match c {
                Change::Created(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();
        let mut asked: Vec<String> = Vec::new();
        for c in &row.changes {
            match c {
                Change::Text(d) | Change::Created(d) => {
                    if asked.contains(d) || asked.len() >= MAX_DIFF_FILES {
                        continue;
                    }
                    asked.push(d.clone());
                    let doc = self.name_now(d, row.last);
                    files.push(FileDiff {
                        ask: Some(json!({ "doc": doc, "from": row.id - 1, "to": row.last })),
                        doc,
                        created: created.contains(&d.as_str()),
                        state: FileState::Loading,
                    });
                }
                Change::CreatedDir(_)
                | Change::Renamed { .. }
                | Change::Deleted(_)
                | Change::Changed(_) => {
                    let doc = c
                        .paths()
                        .last()
                        .map(|p| (*p).to_owned())
                        .unwrap_or_default();
                    files.push(said(doc, c.phrase()));
                }
                Change::Version(_) => {}
            }
        }
        self.files = files;
        self.files_of = Some((row.id, row.last));
        &self.files
    }

    /// The files shown for the selected row.
    pub fn files(&self) -> &[FileDiff] {
        &self.files
    }

    /// The room's answer of `room.diff` for `doc` with parameters `ask`; ignored when the
    /// selection moved on meanwhile.
    pub fn set_diff(&mut self, ask: &Value, answer: Result<&Value, &str>) {
        let Some(f) = self.files.iter_mut().find(|f| f.ask.as_ref() == Some(ask)) else {
            return;
        };
        f.state = match answer {
            Ok(a) => {
                let before = if f.created {
                    ""
                } else {
                    a["before"].as_str().unwrap_or_default()
                };
                let (lines, added, removed) =
                    diff_lines(before, a["after"].as_str().unwrap_or_default());
                if lines.is_empty() {
                    FileState::Said(format!(
                        "{} reads the same before and after",
                        shown_text(&f.doc)
                    ))
                } else {
                    FileState::Shown {
                        lines,
                        added,
                        removed,
                    }
                }
            }
            Err(reason) => FileState::Said(shown_text(reason)),
        };
    }

    /// The files Restore this version brings back (behaviour 64), by their names now.
    pub fn restore_docs(&self) -> Vec<String> {
        let Some(row) = self.selected() else {
            return Vec::new();
        };
        let mut docs: Vec<String> = Vec::new();
        let mut add = |name: String| {
            if !docs.contains(&name) {
                docs.push(name);
            }
        };
        // A rename alone changes no text; the room's undo takes it back, and its path may be a
        // folder, so it brings nothing to restore.
        let restorable = |c: &Change| match c {
            Change::Text(p) | Change::Created(p) | Change::Deleted(p) | Change::Changed(p) => {
                Some(p.clone())
            }
            Change::Renamed { .. } | Change::CreatedDir(_) | Change::Version(_) => None,
        };
        if row.version().is_some() {
            let later: Vec<&Entry> = self.entries.iter().filter(|e| e.seq > row.last).collect();
            // Files created after the version would have to be deleted: a restore never does.
            let born: Vec<String> = later
                .iter()
                .filter_map(|e| match &e.change {
                    Change::Created(p) | Change::CreatedDir(p) => Some(self.name_now(p, e.seq)),
                    _ => None,
                })
                .collect();
            for e in &later {
                let Some(p) = restorable(&e.change) else {
                    continue;
                };
                let now = self.name_now(&p, e.seq);
                if !born
                    .iter()
                    .any(|b| now == *b || now.starts_with(&format!("{b}/")))
                {
                    add(now);
                }
            }
        } else {
            for c in &row.changes {
                if let Some(p) = restorable(c) {
                    add(self.name_now(&p, row.last));
                }
            }
        }
        docs
    }

    /// Restore this version, first click: the dialog asks (behaviour 64). `false` when nothing
    /// can be restored or the role does not allow it.
    pub fn ask_restore(&mut self) -> bool {
        if !self.can_restore() || self.restore_docs().is_empty() {
            return false;
        }
        self.confirming = true;
        self.note = None;
        true
    }

    /// Keep: nothing happens.
    pub fn keep(&mut self) {
        self.confirming = false;
    }

    /// Whether the dialog asks.
    pub fn confirming(&self) -> bool {
        self.confirming
    }

    /// The question of the first click.
    pub fn question(&self) -> String {
        let n = self.restore_docs().len();
        let files = if n == 1 {
            "1 file to how it was".to_owned()
        } else {
            format!("{n} files to how they were")
        };
        match self.selected().and_then(Row::version) {
            Some(name) => format!("Restore {files} at version “{}”?", shown_text(name)),
            None => format!("Restore {files} after this change?"),
        }
    }

    /// Restore in the row: the parameters of `room.restore`, once.
    pub fn confirm_restore(&mut self) -> Option<Value> {
        if !std::mem::take(&mut self.confirming) || !self.can_restore() {
            return None;
        }
        let row = self.selected()?;
        let docs = self.restore_docs();
        (!docs.is_empty()).then(|| json!({ "docs": docs, "seq": row.last }))
    }

    /// The answer of `room.restore` (behaviour 64).
    pub fn restored(&mut self, answer: Result<&Value, &str>) {
        match answer {
            Ok(a) => {
                let n = a["changes"].as_u64().unwrap_or(0);
                self.error = None;
                self.note = Some(match n {
                    0 => "Nothing to restore: the files already read like this.".to_owned(),
                    1 => "Restored 1 file. The room's undo takes it back.".to_owned(),
                    n => format!("Restored {n} files. The room's undo takes it back."),
                });
            }
            Err(reason) => {
                self.note = None;
                self.error = Some(shown_text(reason));
            }
        }
    }
}

/// The rows of `entries` (in journal order), newest first (behaviour 62).
fn rows_of(entries: &[Entry]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut by_group: HashMap<u64, usize> = HashMap::new();
    // The last row that touched each path.
    let mut last_at: HashMap<String, usize> = HashMap::new();
    for e in entries {
        let at = if let Some(&i) = by_group.get(&e.group) {
            Some(i)
        } else if let Change::Text(doc) = &e.change {
            last_at.get(doc).copied().filter(|&i| {
                let r = &rows[i];
                r.actor == e.actor
                    && r.is_typing_in(doc)
                    && e.time.saturating_sub(r.time) <= PHASE_GAP_MS
            })
        } else {
            None
        };
        let i = match at {
            Some(i) => {
                rows[i].take(e);
                i
            }
            None => {
                rows.push(Row::of(e));
                rows.len() - 1
            }
        };
        by_group.insert(e.group, i);
        for p in e.change.paths() {
            last_at.insert(p.to_owned(), i);
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.last));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(seq: u64, time: u64, actor: &str, group: u64, doc: &str) -> Value {
        json!({"seq": seq, "time": time, "actor": actor, "author": actor, "group": group,
               "kind": "text", "doc": doc, "revision": 1, "base_len": 0, "hash": "h"})
    }

    fn file(
        seq: u64,
        actor: &str,
        group: u64,
        change: &str,
        path: &str,
        from: Option<&str>,
    ) -> Value {
        let mut v = json!({"seq": seq, "time": 1000 * seq, "actor": actor, "author": actor,
                           "group": group, "kind": "file", "change": change, "path": path});
        if let Some(f) = from {
            v["from"] = json!(f);
        }
        v
    }

    fn model(entries: Vec<Value>) -> HistoryModel {
        let mut m = HistoryModel::default();
        m.set_history(&json!({ "entries": entries, "seq": 99 }));
        m
    }

    /// SPEC-0323 v9 AK-37: rows per group, typing joined into editing phases only for one person
    /// in one document within 60 seconds with nobody else in between; newest first; summaries.
    #[test]
    fn rows_group_changes_and_join_typing_into_phases() {
        let m = model(vec![
            text(1, 1_000, "ann", 1, "a.txt"),
            text(2, 2_000, "ann", 2, "a.txt"),
            text(3, 30_000, "ann", 3, "a.txt"),
            // Bob in between: Ann's next typing starts a new row.
            text(4, 31_000, "bob", 4, "a.txt"),
            text(5, 32_000, "ann", 5, "a.txt"),
            // More than a minute later: a new phase.
            text(6, 100_000, "ann", 6, "a.txt"),
            // A change set of three files is one row.
            text(7, 101_000, "bob", 7, "x.txt"),
            text(8, 101_000, "bob", 7, "y.txt"),
            file(9, "bob", 7, "created", "z.txt", None),
            file(10, "bob", 7, "deleted", "w.txt", None),
            json!({"seq": 11, "time": 102_000, "actor": "disk", "author": "disk", "group": 8,
                   "kind": "text", "doc": "a.txt"}),
            json!({"seq": 12, "time": 103_000, "actor": "p:ann", "author": "Ann\u{7}", "group": 9,
                   "kind": "version", "name": "v1"}),
            json!({"seq": 13, "kind": "text"}),
            json!("nonsense"),
        ]);
        let rows = m.rows();
        let ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, [12, 11, 7, 6, 5, 4, 1], "newest first");
        assert_eq!(rows[6].last, 3);
        assert_eq!(rows[6].summary(), "Edited a.txt (3 changes)");
        assert_eq!(rows[5].summary(), "Edited a.txt");
        assert_eq!(rows[5].author, "bob");
        assert_eq!(
            rows[2].summary(),
            "Edited x.txt, Edited y.txt, Created z.txt and 1 more"
        );
        assert_eq!(rows[1].author, OUTSIDE);
        assert_eq!(rows[0].summary(), "Version “v1”");
        assert_eq!(rows[0].author, "Ann", "control characters dropped");
        assert_eq!(rows[0].byline(103_000 + 120_000), "Ann · 2 min ago");
        assert_eq!(
            m.selected().map(|r| r.id),
            Some(12),
            "the newest is selected"
        );
        let only_edits = model(vec![
            text(1, 1, "a", 1, "1"),
            text(2, 1, "a", 1, "2"),
            text(3, 1, "a", 1, "3"),
            text(4, 1, "a", 1, "4"),
        ]);
        assert_eq!(only_edits.rows()[0].summary(), "Edited 1, 2, 3 and 1 more");
    }

    /// SPEC-0323 v9 AK-37: names follow later renames; a version restores what changed after
    /// it except files created after it; other rows restore their own files.
    #[test]
    fn names_follow_renames_and_restore_picks_the_files() {
        let mut m = model(vec![
            text(1, 1, "ann", 1, "a.txt"),
            json!({"seq": 2, "time": 2, "actor": "ann", "author": "Ann", "group": 2,
                   "kind": "version", "name": "v1"}),
            text(3, 3, "ann", 3, "b.txt"),
            file(4, "ann", 4, "renamed", "c.txt", Some("a.txt")),
            file(5, "ann", 5, "created", "new.txt", None),
            text(6, 6, "ann", 6, "new.txt"),
            file(7, "ann", 7, "deleted", "d.txt", None),
            file(8, "ann", 8, "renamed", "dir2", Some("dir")),
            text(9, 9, "ann", 9, "dir2/e.txt"),
        ]);
        assert_eq!(m.name_now("a.txt", 1), "c.txt");
        assert_eq!(m.name_now("a.txt", 4), "a.txt");
        assert_eq!(m.name_now("dir/e.txt", 1), "dir2/e.txt");
        assert!(m.select(2));
        assert_eq!(m.restore_docs(), ["b.txt", "d.txt", "dir2/e.txt"]);
        assert!(m.select(1));
        assert_eq!(m.restore_docs(), ["c.txt"]);
        let files = m.start_files().to_vec();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].doc, "c.txt");
        assert_eq!(
            files[0].ask,
            Some(json!({"doc": "c.txt", "from": 0, "to": 1}))
        );
        assert!(m.select(7));
        assert_eq!(m.restore_docs(), ["d.txt"], "a deleted file comes back");
        assert_eq!(
            m.start_files()[0].state,
            FileState::Said("Deleted d.txt".into())
        );
        assert!(!m.select(42), "no such row");
    }

    /// SPEC-0323 v9 AK-38: diff lines with context, gaps and counts; a created file is empty
    /// before; control characters are replaced.
    #[test]
    fn diffs_have_context_gaps_and_counts() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let after = "1\n2\n3\nfour\n5\n6\n7\n8\n9\nten\u{1b}\n";
        let (lines, added, removed) = diff_lines(before, after);
        assert_eq!((added, removed), (2, 1));
        let l = |s: char, t: &str| DiffLine::Line(s, t.to_owned());
        assert_eq!(
            lines,
            [
                l(' ', "2"),
                l(' ', "3"),
                l('−', "4"),
                l('+', "four"),
                l(' ', "5"),
                l(' ', "6"),
                DiffLine::Gap,
                l(' ', "8"),
                l(' ', "9"),
                l('+', "ten\u{fffd}"),
            ]
        );
        let long: String = (0..1000).map(|i| format!("{i}\n")).collect();
        let (lines, added, _) = diff_lines("", &long);
        assert_eq!(added, 1000);
        assert_eq!(lines.len(), MAX_DIFF_LINES + 1);
        assert_eq!(lines.last(), Some(&DiffLine::Gap));

        let mut m = model(vec![
            file(1, "ann", 1, "created", "n.txt", None),
            text(2, 1000, "ann", 1, "n.txt"),
        ]);
        let ask = m.start_files()[0].ask.clone().unwrap();
        m.set_diff(&ask, Ok(&json!({"before": "old\n", "after": "hello\n"})));
        assert_eq!(
            m.files()[0].state,
            FileState::Shown {
                lines: vec![l('+', "hello")],
                added: 1,
                removed: 0
            }
        );
        m.set_diff(&ask, Err("n.txt is too large"));
        assert_eq!(
            m.files()[0].state,
            FileState::Said("n.txt is too large".into())
        );
    }

    /// SPEC-0323 v9 AK-38: only editors and above restore; the question and outcomes.
    #[test]
    fn restoring_needs_the_role_and_asks_first() {
        assert!(!can_restore(None));
        for r in ["viewer", "commenter", "guest"] {
            assert!(!can_restore(Some(r)), "{r}");
        }
        for r in ["editor", "runner", "admin"] {
            assert!(can_restore(Some(r)), "{r}");
        }
        let mut m = model(vec![
            text(1, 1, "ann", 1, "a.txt"),
            text(2, 1, "ann", 1, "b.txt"),
        ]);
        let status = |role: &str| json!({"participants": [{"client": "c1", "role": role}]});
        m.set_role(&status("viewer"), "c1");
        assert!(!m.ask_restore());
        assert!(m.confirm_restore().is_none());
        m.set_role(&status("editor"), "c1");
        assert!(
            m.confirm_restore().is_none(),
            "never without the first click"
        );
        assert!(m.ask_restore());
        assert_eq!(
            m.question(),
            "Restore 2 files to how they were after this change?"
        );
        m.keep();
        assert!(!m.confirming());
        assert!(m.ask_restore());
        assert_eq!(
            m.confirm_restore(),
            Some(json!({"docs": ["a.txt", "b.txt"], "seq": 2}))
        );
        assert!(m.confirm_restore().is_none(), "once");
        m.restored(Ok(&json!({"group": 5, "changes": 2})));
        assert_eq!(
            m.note.as_deref(),
            Some("Restored 2 files. The room's undo takes it back.")
        );
        m.restored(Ok(&json!({"group": null, "changes": 0})));
        assert_eq!(
            m.note.as_deref(),
            Some("Nothing to restore: the files already read like this.")
        );
        m.restored(Err("a.txt changed while no backend was watching"));
        assert!(m.note.is_none());
        assert!(m.error.as_deref().unwrap().contains("no backend"));
        assert_eq!(
            HistoryModel::history_params(&json!({"seq": 1500})),
            json!({"since": 500, "limit": 1000})
        );
        let mut v = model(vec![
            json!({"seq": 1, "time": 1, "actor": "ann", "author": "Ann",
                                      "group": 1, "kind": "version", "name": "v1"}),
        ]);
        v.set_role(&status("admin"), "c1");
        assert!(!v.ask_restore(), "nothing changed after the version");
    }
}
