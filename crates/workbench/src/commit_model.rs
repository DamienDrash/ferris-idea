//! What the Commit tool window lists and which files are included (`SPEC-0063` behaviours 1–3),
//! with the v5 grouping, folding, rollback targets and amend message (behaviours 10, 11 and 13),
//! without toolkit.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use idecraft_vcs::{ChangeKind, FileStatus};

/// One listed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// Repository-relative path.
    pub path: String,
    /// How it changed (working tree against HEAD).
    pub kind: ChangeKind,
}

/// The two groups of the Commit tool window and the checked files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitList {
    /// Tracked files with changes (group "Changes").
    pub changes: Vec<ChangedFile>,
    /// New files Git does not track yet (group "Unversioned Files").
    pub unversioned: Vec<ChangedFile>,
    checked: BTreeSet<String>,
}

impl CommitList {
    /// Builds the groups from a status; tracked changes start checked, unversioned files unchecked
    /// (behaviour 2). Checks of paths still listed in `previous` are kept.
    pub fn from_status(status: &[FileStatus], previous: Option<&CommitList>) -> Self {
        let mut out = Self::default();
        for s in status {
            let untracked = s.staged.is_none() && s.unstaged == Some(ChangeKind::Added);
            let Some(kind) = s.staged.or(s.unstaged) else {
                continue;
            };
            let file = ChangedFile {
                path: s.path.clone(),
                kind,
            };
            let was = previous.map(|p| (p.is_listed(&s.path), p.is_checked(&s.path)));
            let check = match was {
                Some((true, checked)) => checked,
                _ => !untracked,
            };
            if check {
                out.checked.insert(s.path.clone());
            }
            if untracked {
                out.unversioned.push(file);
            } else {
                out.changes.push(file);
            }
        }
        out.changes.sort_by(|a, b| a.path.cmp(&b.path));
        out.unversioned.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// Adds files with unsaved changes in an open editor (paths relative to the repository root) to
    /// "Changes" when Git does not list them yet, checked like other changes (SPEC-0063 v3
    /// behaviour 7): they will be saved before committing.
    pub fn with_unsaved(mut self, unsaved: &[String], previous: Option<&CommitList>) -> Self {
        for path in unsaved {
            if self.is_listed(path) {
                continue;
            }
            let keep = previous.filter(|p| p.is_listed(path));
            if keep.is_none_or(|p| p.is_checked(path)) {
                self.checked.insert(path.clone());
            }
            self.changes.push(ChangedFile {
                path: path.clone(),
                kind: ChangeKind::Modified,
            });
        }
        self.changes.sort_by(|a, b| a.path.cmp(&b.path));
        self
    }

    fn is_listed(&self, path: &str) -> bool {
        self.changes
            .iter()
            .chain(&self.unversioned)
            .any(|f| f.path == path)
    }

    /// Whether `path` is included in the next commit.
    pub fn is_checked(&self, path: &str) -> bool {
        self.checked.contains(path)
    }

    /// Checks or unchecks one file.
    pub fn toggle(&mut self, path: &str) {
        if !self.checked.remove(path) && self.is_listed(path) {
            self.checked.insert(path.to_owned());
        }
    }

    /// Checks a whole group, or unchecks it when all of it is checked (behaviour 3).
    pub fn toggle_group(&mut self, unversioned: bool) {
        let group = if unversioned {
            &self.unversioned
        } else {
            &self.changes
        };
        let paths: Vec<String> = group.iter().map(|f| f.path.clone()).collect();
        let all = paths.iter().all(|p| self.checked.contains(p));
        for p in paths {
            if all {
                self.checked.remove(&p);
            } else {
                self.checked.insert(p);
            }
        }
    }

    /// Whether every file of a group is checked (`None` for an empty group).
    pub fn group_checked(&self, unversioned: bool) -> Option<bool> {
        let group = if unversioned {
            &self.unversioned
        } else {
            &self.changes
        };
        (!group.is_empty()).then(|| group.iter().all(|f| self.checked.contains(&f.path)))
    }

    /// Checks every file of `paths` that is listed, or unchecks them all when all are checked
    /// (the checkbox of a folder or module node, SPEC-0063 v5 behaviour 11).
    pub fn toggle_paths(&mut self, paths: &[String]) {
        let listed: Vec<&String> = paths.iter().filter(|p| self.is_listed(p)).collect();
        let all = listed.iter().all(|p| self.checked.contains(*p));
        for p in listed {
            if all {
                self.checked.remove(p);
            } else {
                self.checked.insert(p.clone());
            }
        }
    }

    /// Whether every listed file of `paths` is checked (`None` when none is listed).
    pub fn paths_checked(&self, paths: &[String]) -> Option<bool> {
        let listed: Vec<&String> = paths.iter().filter(|p| self.is_listed(p)).collect();
        (!listed.is_empty()).then(|| listed.iter().all(|p| self.checked.contains(*p)))
    }

    /// Files a rollback acts on (v5 behaviour 10): the checked files of "Changes"; when none is
    /// checked, the selected row if it belongs to "Changes". Unversioned files never.
    pub fn rollback_targets(&self, selected: Option<&str>) -> Vec<String> {
        let checked: Vec<String> = self
            .changes
            .iter()
            .filter(|f| self.checked.contains(&f.path))
            .map(|f| f.path.clone())
            .collect();
        if !checked.is_empty() {
            return checked;
        }
        selected
            .filter(|s| self.changes.iter().any(|f| f.path == *s))
            .map(|s| vec![s.to_owned()])
            .unwrap_or_default()
    }

    /// The rows of the list for a grouping, without the folded nodes' children (v5 behaviour 11).
    /// `module_of` gives a file's module folder (repository-relative, `""` for the root module);
    /// `root_name` names the root module.
    pub fn rows(
        &self,
        group_by: GroupBy,
        module_of: &dyn Fn(&str) -> String,
        root_name: &str,
        folded: &BTreeSet<String>,
    ) -> Vec<Row> {
        let mut out = Vec::new();
        for unversioned in [false, true] {
            let files = if unversioned {
                &self.unversioned
            } else {
                &self.changes
            };
            if files.is_empty() {
                continue;
            }
            let section = Section {
                key: group_key(unversioned).to_owned(),
                kind: RowKind::Group { unversioned },
                label: group_title(unversioned).to_owned(),
                files: files.iter().collect(),
                unversioned,
            };
            section.push_rows(group_by, module_of, root_name, folded, &mut out);
        }
        out
    }

    /// The rows with a group per changelist in place of the group "Changes" (`SPEC-0335`
    /// behaviour 13): `lists` in the order they are shown, each with the changed files it holds;
    /// a changed file no list names is shown in the active one. A list without files is shown
    /// too, except the lone list of a project that has no other.
    pub fn rows_in_lists(
        &self,
        lists: &[ListGroup],
        group_by: GroupBy,
        module_of: &dyn Fn(&str) -> String,
        root_name: &str,
        folded: &BTreeSet<String>,
    ) -> Vec<Row> {
        let mut out = Vec::new();
        let named: BTreeSet<&str> = lists
            .iter()
            .flat_map(|l| l.files.iter().map(String::as_str))
            .collect();
        for list in lists {
            let files: Vec<&ChangedFile> = self
                .changes
                .iter()
                .filter(|f| {
                    list.files.contains(&f.path)
                        || (list.active && !named.contains(f.path.as_str()))
                })
                .collect();
            if files.is_empty() && lists.len() == 1 && list.name == group_title(false) {
                continue;
            }
            let section = Section {
                key: changelist_key(&list.name),
                kind: RowKind::Changelist {
                    active: list.active,
                },
                label: list.name.clone(),
                files,
                unversioned: false,
            };
            section.push_rows(group_by, module_of, root_name, folded, &mut out);
        }
        if !self.unversioned.is_empty() {
            let section = Section {
                key: group_key(true).to_owned(),
                kind: RowKind::Group { unversioned: true },
                label: group_title(true).to_owned(),
                files: self.unversioned.iter().collect(),
                unversioned: true,
            };
            section.push_rows(group_by, module_of, root_name, folded, &mut out);
        }
        out
    }

    /// The paths of the group "Changes", in order: the files changelists hold.
    pub fn changed_paths(&self) -> Vec<String> {
        self.changes.iter().map(|f| f.path.clone()).collect()
    }

    /// Keys of every node a grouping shows, for Collapse All (v5 behaviour 11).
    pub fn node_keys(
        &self,
        group_by: GroupBy,
        module_of: &dyn Fn(&str) -> String,
        root_name: &str,
    ) -> BTreeSet<String> {
        self.rows(group_by, module_of, root_name, &BTreeSet::new())
            .into_iter()
            .filter(|r| !matches!(r.kind, RowKind::File { .. }))
            .map(|r| r.key)
            .collect()
    }

    /// Paths to commit, in list order.
    pub fn selected(&self) -> Vec<String> {
        self.changes
            .iter()
            .chain(&self.unversioned)
            .filter(|f| self.checked.contains(&f.path))
            .map(|f| f.path.clone())
            .collect()
    }
}

/// One changelist as the Commit tool window groups by it (`SPEC-0335` behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListGroup {
    /// The list's name.
    pub name: String,
    /// Whether it is the active list.
    pub active: bool,
    /// The paths the list holds.
    pub files: BTreeSet<String>,
}

/// The key of a changelist's row, for folding and for the row's id.
pub fn changelist_key(name: &str) -> String {
    format!("l:{name}")
}

/// A group of the list with the files below it.
struct Section<'a> {
    key: String,
    kind: RowKind,
    label: String,
    files: Vec<&'a ChangedFile>,
    unversioned: bool,
}

impl Section<'_> {
    /// The group's row and, unless it is folded, the rows below it.
    fn push_rows(
        self,
        group_by: GroupBy,
        module_of: &dyn Fn(&str) -> String,
        root_name: &str,
        folded: &BTreeSet<String>,
        out: &mut Vec<Row>,
    ) {
        let Section {
            key,
            kind,
            label,
            files,
            unversioned,
        } = self;
        let open = !folded.contains(&key);
        out.push(Row {
            key: key.clone(),
            depth: 0,
            kind,
            label,
            paths: files.iter().map(|f| f.path.clone()).collect(),
        });
        if !open {
            return;
        }
        // Modules in order of their folder, root first.
        let mut modules: Vec<(String, Vec<&ChangedFile>)> = Vec::new();
        for f in files {
            let m = if group_by.module {
                module_of(&f.path)
            } else {
                String::new()
            };
            match modules.iter_mut().find(|(k, _)| *k == m) {
                Some((_, v)) => v.push(f),
                None => modules.push((m, vec![f])),
            }
        }
        modules.sort_by(|a, b| a.0.cmp(&b.0));
        for (module, files) in modules {
            let mut depth = 1;
            let prefix = format!("{key}/m:{module}");
            if group_by.module {
                let label = if module.is_empty() {
                    root_name.to_owned()
                } else {
                    module.rsplit('/').next().unwrap_or(&module).to_owned()
                };
                out.push(Row {
                    key: prefix.clone(),
                    depth,
                    kind: RowKind::Module,
                    label,
                    paths: files.iter().map(|f| f.path.clone()).collect(),
                });
                if folded.contains(&prefix) {
                    continue;
                }
                depth += 1;
            }
            let base = if module.is_empty() {
                String::new()
            } else {
                format!("{module}/")
            };
            if group_by.directory {
                let mut tree = DirNode::default();
                for f in &files {
                    let rel = f.path.strip_prefix(&base).unwrap_or(&f.path);
                    let parts: Vec<&str> = rel.split('/').collect();
                    let mut node = &mut tree;
                    for dir in &parts[..parts.len() - 1] {
                        node = node.dirs.entry((*dir).to_owned()).or_default();
                    }
                    node.files.push(f);
                }
                tree.push_rows(&prefix, "", depth, unversioned, folded, out);
            } else {
                for f in files {
                    out.push(file_row(f, depth, unversioned, true));
                }
            }
        }
    }
}

/// Group By switches of the toolbar (SPEC-0063 v5 behaviour 11); both off is the flat list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroupBy {
    /// Nest files under their folders.
    pub directory: bool,
    /// A node per module above the folders.
    pub module: bool,
}

/// What a row of the list is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    /// "Changes" or "Unversioned Files".
    Group {
        /// Which of the two.
        unversioned: bool,
    },
    /// A changelist (`SPEC-0335` behaviour 13).
    Changelist {
        /// Whether it is the active list.
        active: bool,
    },
    /// A module node.
    Module,
    /// A folder node (a chain of folders joined).
    Directory,
    /// A file.
    File {
        /// How it changed.
        kind: ChangeKind,
        /// In "Unversioned Files".
        unversioned: bool,
        /// The folder shown dimmed after the name in the flat list.
        dir: Option<String>,
    },
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Stable key: the path for a file, a node key for folding otherwise.
    pub key: String,
    /// Indentation level, 0 for the groups.
    pub depth: usize,
    /// Group, module, folder or file.
    pub kind: RowKind,
    /// Text shown: group title, module or folder name, file name.
    pub label: String,
    /// The files at or below the row.
    pub paths: Vec<String>,
}

fn group_key(unversioned: bool) -> &'static str {
    if unversioned {
        "g:unversioned"
    } else {
        "g:changes"
    }
}

/// Title of a group.
pub fn group_title(unversioned: bool) -> &'static str {
    if unversioned {
        "Unversioned Files"
    } else {
        "Changes"
    }
}

fn file_row(f: &ChangedFile, depth: usize, unversioned: bool, show_dir: bool) -> Row {
    let (dir, name) = match f.path.rsplit_once('/') {
        Some((d, n)) => (Some(d.to_owned()), n.to_owned()),
        None => (None, f.path.clone()),
    };
    Row {
        key: f.path.clone(),
        depth,
        kind: RowKind::File {
            kind: f.kind,
            unversioned,
            dir: dir.filter(|_| show_dir),
        },
        label: name,
        paths: vec![f.path.clone()],
    }
}

/// Folders of a module while building the Directory grouping.
#[derive(Default)]
struct DirNode<'a> {
    dirs: std::collections::BTreeMap<String, DirNode<'a>>,
    files: Vec<&'a ChangedFile>,
}

impl DirNode<'_> {
    fn all_paths(&self, out: &mut Vec<String>) {
        for d in self.dirs.values() {
            d.all_paths(out);
        }
        out.extend(self.files.iter().map(|f| f.path.clone()));
    }

    /// Folders first (chains with nothing else joined into one node), then files.
    fn push_rows(
        &self,
        prefix: &str,
        path: &str,
        depth: usize,
        unversioned: bool,
        folded: &BTreeSet<String>,
        out: &mut Vec<Row>,
    ) {
        for (name, child) in &self.dirs {
            let mut label = name.clone();
            let mut node = child;
            while node.files.is_empty() && node.dirs.len() == 1 {
                let (n, c) = node.dirs.iter().next().expect("one folder");
                label = format!("{label}/{n}");
                node = c;
            }
            let full = if path.is_empty() {
                label.clone()
            } else {
                format!("{path}/{label}")
            };
            let key = format!("{prefix}/d:{full}");
            let mut paths = Vec::new();
            node.all_paths(&mut paths);
            out.push(Row {
                key: key.clone(),
                depth,
                kind: RowKind::Directory,
                label,
                paths,
            });
            if !folded.contains(&key) {
                node.push_rows(prefix, &full, depth + 1, unversioned, folded, out);
            }
        }
        for f in &self.files {
            out.push(file_row(f, depth, unversioned, false));
        }
    }
}

/// Build files that make a folder a module (SPEC-0063 v5 behaviour 11).
pub const MODULE_FILES: &[&str] = &[
    "Cargo.toml",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "package.json",
    "pyproject.toml",
    "go.mod",
];

/// The module folder of `path` (repository-relative): the nearest folder from the file up to the
/// root that holds a build file; `""` for the root module.
pub fn module_folder(workdir: &Path, path: &str) -> String {
    let mut dir = Path::new(path).parent();
    while let Some(d) = dir {
        if d.as_os_str().is_empty() {
            break;
        }
        if MODULE_FILES
            .iter()
            .any(|f| workdir.join(d).join(f).is_file())
        {
            return d.to_string_lossy().replace('\\', "/");
        }
        dir = d.parent();
    }
    String::new()
}

/// The message field while the amend switch is used (SPEC-0063 v5 behaviour 13).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmendMessage {
    kept: Option<String>,
}

impl AmendMessage {
    /// Switch on: keeps the typed text aside; the field shows the last commit's message.
    pub fn turn_on(&mut self, typed: &str, last: &str) -> String {
        self.kept = Some(typed.to_owned());
        last.to_owned()
    }

    /// Switch off: the kept text when the field still holds the last message unchanged, else
    /// `None` (the field keeps the edit).
    pub fn turn_off(&mut self, current: &str, last: &str) -> Option<String> {
        let kept = self.kept.take()?;
        (current == last).then_some(kept)
    }

    /// Forgets the kept text (after an amend commit).
    pub fn clear(&mut self) {
        self.kept = None;
    }
}

/// Characters of a message shown in a row of the message history.
pub const HISTORY_ROW_CHARS: usize = 100;

/// Rows of the message history visible at once; the list scrolls beyond them.
pub const HISTORY_VISIBLE_ROWS: usize = 7;

/// What Commit Message History says when no message is remembered (`SPEC-0063` v10 behaviour
/// 22).
pub const NO_MESSAGE_HISTORY: &str = "No commit messages in the history of this project yet";

/// What Commit Message History says in a remote project.
pub const NO_REMOTE_MESSAGE_HISTORY: &str =
    "Commit Message History is not available in a remote project yet";

/// What the status bar says when the history file was not used; `why` never quotes the file.
pub fn history_unreadable(why: idecraft_core::commit_messages::Unreadable) -> String {
    format!(
        "The commit message history ({}) was not read: {why}. It starts empty with the next commit",
        idecraft_core::commit_messages::COMMIT_MESSAGES_FILE
    )
}

/// What the status bar says when a message could not be added to the history; the reason is
/// the file system's, never the message.
pub fn history_not_saved(error: &std::io::Error) -> String {
    format!(
        "The commit message was not added to the history ({}): {error}",
        idecraft_core::commit_messages::COMMIT_MESSAGES_FILE
    )
}

/// A message as one row of the history: on one line, a line break shown as `↵`, any other
/// control character and any direction override as a space, cut after [`HISTORY_ROW_CHARS`]
/// characters with `…`. The row is for reading only; choosing it gives the message itself.
pub fn history_row(message: &str) -> String {
    let mut out = String::new();
    let mut chars = message.trim_end().chars().peekable();
    let mut n = 0;
    while let Some(ch) = chars.next() {
        if n == HISTORY_ROW_CHARS {
            out.push('…');
            break;
        }
        match ch {
            '\r' if chars.peek() == Some(&'\n') => continue,
            '\n' | '\r' => out.push('↵'),
            // A direction override would reorder what is read, like a control character.
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => out.push(' '),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
        n += 1;
    }
    out
}

/// The open list of Commit Message History: the messages, newest first, and the selected row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageHistory {
    messages: Vec<String>,
    selected: usize,
}

impl MessageHistory {
    /// The list for `messages` (newest first) with the newest selected; `None` without any.
    pub fn new(messages: &[String]) -> Option<Self> {
        (!messages.is_empty()).then(|| Self {
            messages: messages.to_vec(),
            selected: 0,
        })
    }

    /// The rows as shown, newest first.
    pub fn rows(&self) -> Vec<String> {
        self.messages.iter().map(|m| history_row(m)).collect()
    }

    /// The selected row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Moves the selection one row down (`forward`) or up, around the ends.
    pub fn step(&mut self, forward: bool) {
        let n = self.messages.len();
        self.selected = if forward {
            (self.selected + 1) % n
        } else {
            (self.selected + n - 1) % n
        };
    }

    /// Selects row `index`, if there is one.
    pub fn select(&mut self, index: usize) -> bool {
        let known = index < self.messages.len();
        if known {
            self.selected = index;
        }
        known
    }

    /// The message of the selected row, as it was typed.
    pub fn chosen(&self) -> &str {
        &self.messages[self.selected]
    }
}

/// The one edit that puts `chosen` in a field holding `current`: the whole text is replaced, so
/// one undo brings `current` back. `None` when the field holds it already.
pub fn message_edit(current: &str, chosen: &str) -> Option<(std::ops::Range<usize>, String)> {
    (current != chosen).then(|| (0..current.chars().count(), chosen.to_owned()))
}

/// Whether Commit remembers the message before it commits: something is committed (a checked
/// file, or an amend) — a press of the button with nothing to commit remembers nothing.
pub fn remembers_message(amend: bool, checked: usize) -> bool {
    amend || checked > 0
}

/// Scheme colour key of a change, as the reference colours file names (behaviour 1).
pub fn status_color_key(kind: ChangeKind, unversioned: bool) -> &'static str {
    if unversioned {
        return "FILESTATUS_UNKNOWN";
    }
    match kind {
        ChangeKind::Added => "FILESTATUS_ADDED",
        ChangeKind::Deleted => "FILESTATUS_DELETED",
        ChangeKind::Modified | ChangeKind::TypeChange => "FILESTATUS_MODIFIED",
        ChangeKind::Renamed => "FILESTATUS_RENAMED",
        ChangeKind::Conflict => "FILESTATUS_IDEA_FILESTATUS_MERGED_WITH_CONFLICTS",
    }
}

/// Colour key of every changed file below `workdir`, by absolute path, for the file names in the
/// project tree and tabs (`SPEC-0059` behaviour 7). Unchanged files are absent.
pub fn file_status_keys(workdir: &Path, status: &[FileStatus]) -> HashMap<PathBuf, &'static str> {
    status
        .iter()
        .filter_map(|s| {
            let untracked = s.staged.is_none() && s.unstaged == Some(ChangeKind::Added);
            let kind = s.staged.or(s.unstaged)?;
            Some((workdir.join(&s.path), status_color_key(kind, untracked)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0063 AK-12 (v10): rows are one line each, newest first; the selection goes around;
    /// the chosen message is the typed one, not its row.
    #[test]
    fn the_message_history_lists_rows_and_gives_the_message() {
        assert_eq!(MessageHistory::new(&[]), None);
        let messages: Vec<String> = [
            "Fix the parser\n\nIt lost the last token.\r\nSee #12.\n",
            "tab\there \u{1b}[31mred\u{202e}",
            "short",
        ]
        .map(str::to_owned)
        .to_vec();
        let mut h = MessageHistory::new(&messages).unwrap();
        assert_eq!(
            h.rows(),
            [
                "Fix the parser↵↵It lost the last token.↵See #12.",
                "tab here  [31mred ",
                "short"
            ]
        );
        assert_eq!((h.selected(), h.chosen()), (0, messages[0].as_str()));
        h.step(false);
        assert_eq!((h.selected(), h.chosen()), (2, "short"));
        h.step(true);
        h.step(true);
        assert_eq!(h.chosen(), messages[1]);
        assert!(h.select(2) && !h.select(3));
        assert_eq!(h.selected(), 2);
        // A long message is cut for the row only.
        let long = "é".repeat(HISTORY_ROW_CHARS + 20);
        let row = history_row(&long);
        assert_eq!(row.chars().count(), HISTORY_ROW_CHARS + 1);
        assert!(row.ends_with('…'));
        assert_eq!(
            history_row(&"x".repeat(HISTORY_ROW_CHARS)).chars().count(),
            HISTORY_ROW_CHARS
        );
    }

    /// SPEC-0063 AK-12 (v10): choosing replaces the whole text as one undo step.
    #[test]
    fn a_chosen_message_replaces_the_text_in_one_undo_step() {
        let typed = "half a thought\nwith ünïcode";
        let chosen = "Fix the parser\n\nIt lost the last token.";
        let mut buffer = idecraft_editor::TextBuffer::from_text(typed);
        buffer.track_changes();
        let (range, text) = message_edit(typed, chosen).unwrap();
        assert_eq!(range, 0..buffer.len_chars());
        buffer.begin_group();
        buffer.replace(range, &text).unwrap();
        buffer.end_group();
        assert_eq!(buffer.text(), chosen);
        assert!(buffer.undo());
        assert_eq!(buffer.text(), typed);
        assert!(!buffer.undo(), "one step");
        // The field holds it already: nothing to undo afterwards.
        assert_eq!(message_edit(chosen, chosen), None);
        // An empty field takes it too.
        assert_eq!(message_edit("", "a"), Some((0..0, "a".to_owned())));
    }

    /// SPEC-0063 AK-12 (v10): a commit with something to commit remembers its message; the
    /// status texts name the file and never a message.
    #[test]
    fn a_commit_with_something_to_commit_remembers_its_message() {
        assert!(remembers_message(false, 1));
        assert!(remembers_message(true, 0));
        assert!(!remembers_message(false, 0));
        let why = idecraft_core::commit_messages::Unreadable::Broken;
        assert!(history_unreadable(why).contains(".idecraft/commit-messages.toml"));
        let e = std::io::Error::other("disk full");
        assert!(history_not_saved(&e).ends_with("disk full"));
    }

    #[test]
    fn unsaved_files_join_the_changes() {
        let list = CommitList::default().with_unsaved(&["src/a.cpp".into()], None);
        assert_eq!(list.changes.len(), 1);
        assert!(list.is_checked("src/a.cpp"));
        let mut again = CommitList::default().with_unsaved(&["src/a.cpp".into()], None);
        again.toggle("src/a.cpp");
        let kept = CommitList::default().with_unsaved(&["src/a.cpp".into()], Some(&again));
        assert!(!kept.is_checked("src/a.cpp"));
    }

    fn st(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        FileStatus {
            path: path.into(),
            staged,
            unstaged,
        }
    }

    #[test]
    fn groups_defaults_and_toggles() {
        use ChangeKind::*;
        let status = [
            st("b.rs", None, Some(Modified)),
            st("new.txt", None, Some(Added)),
            st("a.rs", Some(Added), None),
            st("gone.rs", None, Some(Deleted)),
        ];
        let mut l = CommitList::from_status(&status, None);
        let paths = |v: &[ChangedFile]| v.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
        assert_eq!(paths(&l.changes), ["a.rs", "b.rs", "gone.rs"]);
        assert_eq!(paths(&l.unversioned), ["new.txt"]);
        assert_eq!(
            l.selected(),
            ["a.rs", "b.rs", "gone.rs"],
            "unversioned not checked"
        );
        assert_eq!(l.group_checked(true), Some(false));
        l.toggle("new.txt");
        l.toggle("b.rs");
        assert_eq!(l.selected(), ["a.rs", "gone.rs", "new.txt"]);
        l.toggle_group(false);
        assert_eq!(l.group_checked(false), Some(true));
        l.toggle_group(false);
        assert_eq!(l.selected(), ["new.txt"]);
        l.toggle("not-listed");
        assert_eq!(l.selected(), ["new.txt"]);
        // A refresh keeps the user's choices for files still listed.
        let again = CommitList::from_status(&status[..3], Some(&l));
        assert_eq!(again.selected(), ["new.txt"]);
        assert_eq!(
            CommitList::from_status(&[], None).group_checked(false),
            None
        );
        assert_eq!(status_color_key(Modified, false), "FILESTATUS_MODIFIED");
        assert_eq!(status_color_key(Added, true), "FILESTATUS_UNKNOWN");
    }

    fn v5_list() -> CommitList {
        use ChangeKind::*;
        CommitList::from_status(
            &[
                st("app/src/main/java/A.java", None, Some(Modified)),
                st("app/src/main/java/B.java", None, Some(Modified)),
                st("app/pom.xml", None, Some(Modified)),
                st("README.md", None, Some(Modified)),
                st("lib/x.rs", Some(Added), None),
                st("notes/todo.txt", None, Some(Added)),
            ],
            None,
        )
    }

    fn module_of(path: &str) -> String {
        if path.starts_with("app/") {
            "app".into()
        } else {
            String::new()
        }
    }

    fn shown(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.label))
            .collect()
    }

    #[test]
    fn grouping_by_directory_and_module() {
        let l = v5_list();
        let none = BTreeSet::new();
        let flat = l.rows(GroupBy::default(), &module_of, "repo", &none);
        assert_eq!(
            shown(&flat),
            [
                "Changes",
                "  README.md",
                "  pom.xml",
                "  A.java",
                "  B.java",
                "  x.rs",
                "Unversioned Files",
                "  todo.txt"
            ]
        );
        assert!(matches!(&flat[2].kind, RowKind::File { dir: Some(d), .. } if d == "app"));
        let by_dir = GroupBy {
            directory: true,
            module: false,
        };
        let rows = l.rows(by_dir, &module_of, "repo", &none);
        assert_eq!(
            shown(&rows),
            [
                "Changes",
                "  app",
                "    src/main/java",
                "      A.java",
                "      B.java",
                "    pom.xml",
                "  lib",
                "    x.rs",
                "  README.md",
                "Unversioned Files",
                "  notes",
                "    todo.txt"
            ]
        );
        assert_eq!(
            rows[1].paths.len(),
            3,
            "a folder node holds the files below it"
        );
        assert!(matches!(rows[3].kind, RowKind::File { dir: None, .. }));
        let both = GroupBy {
            directory: true,
            module: true,
        };
        let rows = l.rows(both, &module_of, "repo", &none);
        assert_eq!(
            shown(&rows)[..8],
            [
                "Changes",
                "  repo",
                "    lib",
                "      x.rs",
                "    README.md",
                "  app",
                "    src/main/java",
                "      A.java"
            ]
        );
        let modules = GroupBy {
            directory: false,
            module: true,
        };
        let rows = l.rows(modules, &module_of, "repo", &none);
        assert_eq!(shown(&rows)[1..4], ["  repo", "    README.md", "    x.rs"]);
        assert_eq!(rows[4].kind, RowKind::Module);
    }

    fn group(name: &str, active: bool, files: &[&str]) -> ListGroup {
        ListGroup {
            name: name.into(),
            active,
            files: files.iter().map(|f| (*f).to_owned()).collect(),
        }
    }

    /// SPEC-0335 AK-6 (behaviour 13): a group per changelist in the given order, the active one
    /// marked, empty lists shown, unversioned files in their own group.
    #[test]
    fn a_group_per_changelist_with_the_active_one_marked() {
        let l = v5_list();
        let none = BTreeSet::new();
        let lists = [
            group("Api", false, &["app/pom.xml", "gone.rs"]),
            group("Changes", true, &["README.md"]),
            group("Empty", false, &[]),
        ];
        let rows = l.rows_in_lists(&lists, GroupBy::default(), &module_of, "repo", &none);
        assert_eq!(
            shown(&rows),
            [
                "Api",
                "  pom.xml",
                "Changes",
                "  README.md",
                "  A.java",
                "  B.java",
                "  x.rs",
                "Empty",
                "Unversioned Files",
                "  todo.txt"
            ]
        );
        assert_eq!(rows[0].kind, RowKind::Changelist { active: false });
        assert_eq!(rows[2].kind, RowKind::Changelist { active: true });
        assert_eq!(rows[0].key, "l:Api");
        // The row of a list holds its changed files: what its checkbox checks.
        assert_eq!(rows[0].paths, ["app/pom.xml"]);
        assert_eq!(
            rows[2].paths.len(),
            4,
            "files no list names are the active list's"
        );
        assert!(rows[7].paths.is_empty());
        assert_eq!(rows[8].kind, RowKind::Group { unversioned: true });
        let mut checks = l.clone();
        checks.toggle_paths(&rows[0].paths);
        assert_eq!(checks.paths_checked(&rows[0].paths), Some(false));
        assert_eq!(checks.paths_checked(&rows[2].paths), Some(true));
        assert_eq!(checks.paths_checked(&rows[7].paths), None);

        // The lone list Changes: its group as before, and nothing when it has no file.
        let lone = [group("Changes", true, &[])];
        let rows = l.rows_in_lists(&lone, GroupBy::default(), &module_of, "repo", &none);
        assert_eq!(shown(&rows)[0], "Changes");
        assert_eq!(rows[0].paths.len(), 5);
        let empty = CommitList::default();
        assert!(
            empty
                .rows_in_lists(&lone, GroupBy::default(), &module_of, "repo", &none)
                .is_empty()
        );
        // A lone list of another name is shown without files, and so are several lists.
        let other = [group("Work", true, &[])];
        assert_eq!(
            shown(&empty.rows_in_lists(&other, GroupBy::default(), &module_of, "repo", &none)),
            ["Work"]
        );
        assert_eq!(
            shown(&empty.rows_in_lists(&lists, GroupBy::default(), &module_of, "repo", &none)),
            ["Api", "Changes", "Empty"]
        );
        assert_eq!(l.changed_paths().len(), 5);
    }

    /// SPEC-0335 AK-6 (behaviour 13): Group By nests inside each list; a list folds like a group.
    #[test]
    fn grouping_and_folding_inside_a_changelist() {
        let l = v5_list();
        let lists = [
            group("Api", false, &["app/pom.xml", "app/src/main/java/A.java"]),
            group("Changes", true, &[]),
        ];
        let by_dir = GroupBy {
            directory: true,
            module: false,
        };
        let mut folded = BTreeSet::new();
        let rows = l.rows_in_lists(&lists, by_dir, &module_of, "repo", &folded);
        assert_eq!(
            shown(&rows)[..9],
            [
                "Api",
                "  app",
                "    src/main/java",
                "      A.java",
                "    pom.xml",
                "Changes",
                "  app/src/main/java",
                "    B.java",
                "  lib"
            ]
        );
        assert_eq!(rows[1].key, "l:Api/m:/d:app");
        folded.insert("l:Api".to_owned());
        folded.insert("l:Changes/m:/d:lib".to_owned());
        let rows = l.rows_in_lists(&lists, by_dir, &module_of, "repo", &folded);
        assert_eq!(
            shown(&rows)[..5],
            [
                "Api",
                "Changes",
                "  app/src/main/java",
                "    B.java",
                "  lib"
            ]
        );
        assert_eq!(rows[0].paths.len(), 2, "a folded list keeps its files");
    }

    #[test]
    fn folding_expand_and_collapse_all() {
        let mut l = v5_list();
        let by_dir = GroupBy {
            directory: true,
            module: false,
        };
        let mut folded = BTreeSet::new();
        folded.insert("g:changes/m:/d:app".to_owned());
        let rows = l.rows(by_dir, &module_of, "repo", &folded);
        assert_eq!(shown(&rows)[..4], ["Changes", "  app", "  lib", "    x.rs"]);
        // Folding survives a refresh (the keys do not depend on the list object).
        l = CommitList::from_status(
            &[st("app/pom.xml", None, Some(ChangeKind::Modified))],
            Some(&l),
        );
        assert_eq!(
            shown(&l.rows(by_dir, &module_of, "repo", &folded)),
            ["Changes", "  app"]
        );
        // Collapse All folds every node, groups included; Expand All is the empty set.
        let l = v5_list();
        let all = l.node_keys(by_dir, &module_of, "repo");
        assert!(all.contains("g:changes") && all.contains("g:unversioned"));
        assert_eq!(
            shown(&l.rows(by_dir, &module_of, "repo", &all)),
            ["Changes", "Unversioned Files"]
        );
        assert_eq!(
            l.rows(by_dir, &module_of, "repo", &BTreeSet::new()).len(),
            12
        );
    }

    #[test]
    fn node_checkboxes_and_rollback_targets() {
        let mut l = v5_list();
        let app: Vec<String> = ["app/pom.xml", "app/src/main/java/A.java"]
            .map(String::from)
            .to_vec();
        assert_eq!(l.paths_checked(&app), Some(true));
        l.toggle_paths(&app);
        assert_eq!(l.paths_checked(&app), Some(false));
        assert!(!l.is_checked("app/pom.xml") && l.is_checked("README.md"));
        l.toggle("app/pom.xml");
        l.toggle_paths(&app);
        assert_eq!(l.paths_checked(&app), Some(true), "mixed checks all");
        assert_eq!(l.paths_checked(&["nope".into()]), None);

        // Rollback: checked changes; unversioned files never.
        l.toggle("notes/todo.txt");
        let targets = l.rollback_targets(Some("README.md"));
        assert!(targets.contains(&"README.md".to_owned()));
        assert!(!targets.contains(&"notes/todo.txt".to_owned()));
        // Nothing checked: the selected row, if it is a change.
        let mut none = v5_list();
        none.toggle_group(false);
        assert_eq!(none.rollback_targets(Some("lib/x.rs")), ["lib/x.rs"]);
        assert!(none.rollback_targets(Some("notes/todo.txt")).is_empty());
        assert!(none.rollback_targets(None).is_empty());
    }

    #[test]
    fn amend_message_on_and_off() {
        let mut a = AmendMessage::default();
        assert_eq!(a.turn_on("typed", "last\n\nbody"), "last\n\nbody");
        assert_eq!(
            a.turn_off("last\n\nbody", "last\n\nbody"),
            Some("typed".into())
        );
        assert_eq!(a.turn_off("x", "last"), None, "nothing kept any more");
        a.turn_on("", "last");
        assert_eq!(a.turn_off("last, edited", "last"), None, "an edit stays");
    }

    #[test]
    fn modules_are_folders_with_a_build_file() {
        let dir = std::env::temp_dir().join(format!("idecraft-modules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("svc/api/src")).unwrap();
        std::fs::write(dir.join("svc/api/Cargo.toml"), "").unwrap();
        std::fs::write(dir.join("svc/package.json"), "{}").unwrap();
        assert_eq!(module_folder(&dir, "svc/api/src/main.rs"), "svc/api");
        assert_eq!(module_folder(&dir, "svc/index.js"), "svc");
        assert_eq!(module_folder(&dir, "top.txt"), "");
        assert_eq!(module_folder(&dir, "other/x.txt"), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tree_colour_keys_by_absolute_path() {
        use ChangeKind::*;
        let keys = file_status_keys(
            Path::new("/r"),
            &[
                st("src/a.rs", None, Some(Modified)),
                st("new.txt", None, Some(Added)),
                st("staged.rs", Some(Added), None),
                st("clean.rs", None, None),
            ],
        );
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[Path::new("/r/src/a.rs")], "FILESTATUS_MODIFIED");
        assert_eq!(keys[Path::new("/r/new.txt")], "FILESTATUS_UNKNOWN");
        assert_eq!(keys[Path::new("/r/staged.rs")], "FILESTATUS_ADDED");
    }
}
