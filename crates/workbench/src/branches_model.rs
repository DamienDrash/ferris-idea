//! Rows and selection of the branch popup (`SPEC-0068` behaviours 2–4, v5 behaviours 15–20, v8
//! favourites, behaviours 25–29), without toolkit.

use std::collections::BTreeSet;

use idecraft_core::{fuzzy_match, rank};
use idecraft_vcs::{Branch, Operation, Tag, Tracking};

/// What a row of a waiting operation does (`SPEC-0068` v3 behaviour 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpAction {
    /// Git: Continue.
    Continue,
    /// Git: Abort.
    Abort,
    /// Git: Skip (rebase and cherry-pick only).
    Skip,
}

/// One row of the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchRow {
    /// Create a local branch with this name from HEAD and check it out; an empty name is the
    /// "New Branch…" entry that asks for the name (SPEC-0068 v2 behaviour 7). While a name or a
    /// revision is typed (v5), the row confirms what was typed.
    New(String),
    /// Group heading.
    Header(&'static str),
    /// An existing branch.
    Branch(Branch),
    /// Continue, abort or skip the waiting operation, with its label (v3 behaviour 11).
    Op(OpAction, String),
    /// An action at the top (v5 behaviour 15).
    Action(PopupAction),
    /// A folder of branches sharing a prefix, or of all tags, and whether it is open (v5
    /// behaviours 18, 19).
    Folder(Group, String, bool),
    /// A tag (v5 behaviour 19).
    Tag(Tag),
}

/// Heading and rows of a waiting operation (v3 behaviour 11).
pub fn operation_rows(op: Operation) -> (&'static str, Vec<BranchRow>) {
    let (title, noun) = match op {
        Operation::Rebase => ("Rebase in progress", "Rebase"),
        Operation::CherryPick => ("Cherry-pick in progress", "Cherry-pick"),
        Operation::Merge => ("Merge in progress", "Merge"),
    };
    let mut rows = vec![
        BranchRow::Op(OpAction::Continue, format!("Continue {noun}")),
        BranchRow::Op(OpAction::Abort, format!("Abort {noun}")),
    ];
    if op != Operation::Merge {
        rows.push(BranchRow::Op(OpAction::Skip, "Skip Commit".into()));
    }
    (title, rows)
}

/// The key line at the bottom of the popup (v3 behaviour 12, v5 behaviour 20).
pub const FOOTER: &str = "↩ Checkout · → More · ⌥↩ Rebase onto · ⌘↩ Merge into current";

/// What the branch widget says (v3 behaviour 10): the branch with `↑n ↓m`, or the waiting
/// operation; `None` without branch and operation (detached HEAD). The flag asks for the warning
/// icon.
pub fn widget_label(state: &idecraft_vcs::BranchState) -> Option<(String, bool)> {
    if let Some(op) = state.operation {
        return Some((
            match op {
                Operation::Rebase => {
                    format!("Rebasing {}", state.rebasing.as_deref().unwrap_or("HEAD"))
                }
                Operation::Merge => {
                    format!("Merging {}", state.branch.as_deref().unwrap_or("HEAD"))
                }
                Operation::CherryPick => "Cherry-picking".to_owned(),
            },
            true,
        ));
    }
    let mut label = state.branch.clone()?;
    if state.ahead > 0 {
        label.push_str(&format!(" ↑{}", state.ahead));
    }
    if state.behind > 0 {
        label.push_str(&format!(" ↓{}", state.behind));
    }
    Some((label, false))
}

/// What the branch popup shows besides the branches (`SPEC-0068` v5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PopupData {
    /// Current first, then local, then remote.
    pub branches: Vec<Branch>,
    /// Upstream and ahead/behind of the local branches (behaviour 18).
    pub tracking: Vec<Tracking>,
    /// Recently checked-out local branches, newest first (behaviour 17).
    pub recent: Vec<String>,
    /// Tags (behaviour 19).
    pub tags: Vec<Tag>,
    /// The operation that waits on conflicts (v3 behaviour 11).
    pub operation: Option<Operation>,
    /// The repository's favourite branches (v8 behaviour 25).
    pub favorites: idecraft_core::RepoFavorites,
}

impl PopupData {
    /// Only branches, as before v5.
    pub fn of(branches: &[Branch], operation: Option<Operation>) -> Self {
        Self {
            branches: branches.to_vec(),
            operation,
            ..Self::default()
        }
    }

    /// The checked-out branch.
    pub fn current(&self) -> Option<&str> {
        self.branches
            .iter()
            .find(|b| b.current)
            .map(|b| b.name.as_str())
    }

    /// Whether `branch` is a favourite (v8 behaviour 25).
    pub fn favorite(&self, branch: &Branch) -> bool {
        self.favorites.is_favorite(&branch.name, branch.remote)
    }

    /// Whether the branch of `row` is a favourite; `false` for every other row.
    pub fn row_favorite(&self, row: &BranchRow) -> bool {
        matches!(row, BranchRow::Branch(b) if self.favorite(b))
    }

    /// Upstream with ahead/behind, or "gone", of local branch `name` (behaviour 18).
    pub fn tracking_hint(&self, name: &str) -> Option<String> {
        let t = self.tracking.iter().find(|t| t.branch == name)?;
        let upstream = t.upstream.as_deref()?;
        if t.gone {
            return Some(format!("{upstream} (gone)"));
        }
        let mut hint = String::new();
        if t.ahead > 0 {
            hint.push_str(&format!("↑{} ", t.ahead));
        }
        if t.behind > 0 {
            hint.push_str(&format!("↓{} ", t.behind));
        }
        hint.push_str(upstream);
        Some(hint)
    }
}

/// An action at the top of the popup (`SPEC-0068` v5 behaviour 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupAction {
    /// Update Project (`Vcs.UpdateProject`).
    Update,
    /// Commit (`CheckinProject`, ⌘K).
    Commit,
    /// Push (`Vcs.Push`).
    Push,
    /// Checkout of a tag or revision (behaviour 16).
    CheckoutRevision,
}

impl PopupAction {
    /// The row's text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Update => "Update Project…",
            Self::Commit => "Commit…",
            Self::Push => "Push…",
            Self::CheckoutRevision => "Checkout Tag or Revision…",
        }
    }

    /// The workbench action it runs; `None` for the checkout, which asks in the popup.
    pub fn action(self) -> Option<&'static str> {
        match self {
            Self::Update => Some("Vcs.UpdateProject"),
            Self::Commit => Some("CheckinProject"),
            Self::Push => Some("Vcs.Push"),
            Self::CheckoutRevision => None,
        }
    }

    /// Its icon (own drawings, ADR-0009).
    pub fn icon(self) -> &'static str {
        match self {
            Self::Update => "idecraft/vcsUpdate",
            Self::Commit => "idecraft/vcsCommit",
            Self::Push => "idecraft/vcsPush",
            Self::CheckoutRevision => "idecraft/tag",
        }
    }
}

/// The group a folder belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Local branches, folded by the part before the first `/`.
    Local,
    /// Remote-tracking branches, folded by remote.
    Remote,
    /// All tags in one folder.
    Tags,
}

/// A folder's key in the set of collapsed folders.
pub fn folder_key(group: Group, prefix: &str) -> String {
    format!("{group:?}/{prefix}")
}

/// The folders collapsed when the popup opens: only the tags (IMP-0323).
pub fn initially_collapsed() -> BTreeSet<String> {
    [folder_key(Group::Tags, "")].into_iter().collect()
}

/// One row of a branch's submenu (`SPEC-0068` v5 behaviour 20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchAction {
    /// Check the branch (or tag) out.
    Checkout,
    /// Ask for a name and create a branch starting there, then check it out.
    NewBranchFrom,
    /// Check the branch out and rebase it onto the current one.
    CheckoutAndRebase,
    /// Rebase the current branch onto it.
    RebaseOnto,
    /// Merge it into the current branch.
    MergeInto,
    /// The commits it has and the current branch lacks, and the other way round (v6).
    CompareWith,
    /// The files that differ between it and the working tree (v6).
    DiffWorkingTree,
    /// Push the branch (the current one, or another local branch, v6).
    Push,
    /// Ask for a new name.
    Rename,
    /// Delete the local branch (only when merged).
    Delete,
    /// Mark or unmark the branch as a favourite (v8 behaviour 29).
    Favorite,
}

/// The label of the favourite entry of a branch's submenu (v8 behaviour 29).
pub fn favorite_label(favorite: bool) -> &'static str {
    if favorite {
        "Unmark as Favorite"
    } else {
        "Mark as Favorite"
    }
}

/// The submenu of `row`: what can be done with it while `current` is checked out; a branch ends
/// with Mark/Unmark as Favorite after `favorite` (v8 behaviour 29).
pub fn branch_actions(
    row: &BranchRow,
    current: Option<&str>,
    favorite: bool,
) -> Vec<(BranchAction, String)> {
    let mut items = branch_entries(row, current);
    if matches!(row, BranchRow::Branch(_)) {
        items.push((BranchAction::Favorite, favorite_label(favorite).into()));
    }
    items
}

fn branch_entries(row: &BranchRow, current: Option<&str>) -> Vec<(BranchAction, String)> {
    let cur = current.unwrap_or("HEAD");
    match row {
        BranchRow::Branch(b) if b.current => vec![
            (
                BranchAction::NewBranchFrom,
                format!("New Branch from '{}'…", b.name),
            ),
            (BranchAction::Push, "Push…".into()),
            (BranchAction::Rename, "Rename…".into()),
        ],
        BranchRow::Branch(b) if b.remote => vec![
            (BranchAction::Checkout, "Checkout".into()),
            (
                BranchAction::NewBranchFrom,
                format!("New Branch from '{}'…", b.name),
            ),
            (
                BranchAction::RebaseOnto,
                format!("Rebase '{cur}' onto '{}'", b.name),
            ),
            (
                BranchAction::MergeInto,
                format!("Merge '{}' into '{cur}'", b.name),
            ),
            (BranchAction::CompareWith, format!("Compare with '{cur}'")),
            (
                BranchAction::DiffWorkingTree,
                "Show Diff with Working Tree".into(),
            ),
        ],
        BranchRow::Branch(b) => vec![
            (BranchAction::Checkout, "Checkout".into()),
            (
                BranchAction::NewBranchFrom,
                format!("New Branch from '{}'…", b.name),
            ),
            (
                BranchAction::CheckoutAndRebase,
                format!("Checkout and Rebase onto '{cur}'"),
            ),
            (
                BranchAction::RebaseOnto,
                format!("Rebase '{cur}' onto '{}'", b.name),
            ),
            (
                BranchAction::MergeInto,
                format!("Merge '{}' into '{cur}'", b.name),
            ),
            (BranchAction::CompareWith, format!("Compare with '{cur}'")),
            (
                BranchAction::DiffWorkingTree,
                "Show Diff with Working Tree".into(),
            ),
            (BranchAction::Push, "Push…".into()),
            (BranchAction::Rename, "Rename…".into()),
            (BranchAction::Delete, "Delete".into()),
        ],
        BranchRow::Tag(t) => vec![
            (BranchAction::Checkout, "Checkout".into()),
            (
                BranchAction::NewBranchFrom,
                format!("New Branch from '{}'…", t.name),
            ),
            (
                BranchAction::MergeInto,
                format!("Merge '{}' into '{cur}'", t.name),
            ),
            (BranchAction::CompareWith, format!("Compare with '{cur}'")),
            (
                BranchAction::DiffWorkingTree,
                "Show Diff with Working Tree".into(),
            ),
        ],
        _ => Vec::new(),
    }
}

/// The name a row stands for (branch or tag).
pub fn row_name(row: &BranchRow) -> Option<&str> {
    match row {
        BranchRow::Branch(b) => Some(&b.name),
        BranchRow::Tag(t) => Some(&t.name),
        _ => None,
    }
}

/// Rows for a filter text with the selected one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchList {
    /// Operation rows, actions, Recent, Local, Remote and the tags, or the matches of a filter.
    pub rows: Vec<BranchRow>,
    /// Index of the selected row (never a heading).
    pub selected: Option<usize>,
    /// Indentation of each row: 1 inside a folder (v5 behaviour 18).
    pub depth: Vec<u8>,
}

/// Whether `name` can be a new branch name: no spaces or Git-special characters (behaviour 3).
pub fn valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '/', '.'])
        && !name.ends_with(['/', '.'])
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\@{".contains(c))
}

/// Whether `rev` can be checked out as a revision: not empty, no spaces, no leading `-`.
pub fn valid_revision(rev: &str) -> bool {
    !rev.is_empty() && !rev.starts_with('-') && !rev.contains(char::is_whitespace)
}

/// The text of a row inside a folder: without the folder's prefix.
pub fn short_label(name: &str, depth: u8) -> &str {
    if depth == 0 {
        return name;
    }
    name.split_once('/').map_or(name, |(_, rest)| rest)
}

/// The folder a branch name falls into within its group; `None` for a local name without `/`.
fn folder_of(group: Group, name: &str) -> Option<&str> {
    match name.split_once('/') {
        Some((prefix, _)) => Some(prefix),
        None if group == Group::Remote => Some(""),
        None => None,
    }
}

impl BranchList {
    fn push(&mut self, row: BranchRow, depth: u8) {
        self.rows.push(row);
        self.depth.push(depth);
    }

    /// Branches matching `filter` (fuzzy, best first; all in their order when empty). The current
    /// branch is selected when the filter is empty, otherwise the best match (behaviour 2).
    pub fn build(branches: &[Branch], filter: &str) -> Self {
        Self::build_with(branches, filter, None)
    }

    /// As `build`; with an empty filter a waiting operation's rows come first (v3 behaviour 11).
    pub fn build_with(branches: &[Branch], filter: &str, operation: Option<Operation>) -> Self {
        Self::build_from(
            &PopupData::of(branches, operation),
            filter,
            &initially_collapsed(),
        )
    }

    /// The v5 popup (behaviours 15–19): with an empty filter the operation rows, the actions,
    /// Recent, Local and Remote folded by prefix, and the tags; with a filter the matching
    /// branches and tags flat by group. `collapsed` holds the closed folders (`folder_key`).
    pub fn build_from(data: &PopupData, filter: &str, collapsed: &BTreeSet<String>) -> Self {
        let filter = filter.trim();
        let pick = |remote: bool| -> Vec<Branch> {
            let mut found: Vec<(i64, Branch)> = data
                .branches
                .iter()
                .filter(|b| b.remote == remote)
                .filter_map(|b| {
                    if filter.is_empty() {
                        Some((0, b.clone()))
                    } else {
                        fuzzy_match(filter, &b.name).map(|m| (m.score, b.clone()))
                    }
                })
                .collect();
            if !filter.is_empty() {
                found.sort_by(|a, b| rank((a.0, &a.1.name), (b.0, &b.1.name)));
            }
            let mut found: Vec<Branch> = found.into_iter().map(|(_, b)| b).collect();
            // v8 behaviour 27: the current branch, then the favourites, then the others; a stable
            // sort keeps the name order within each part. A filter keeps the order of the match.
            if filter.is_empty() {
                found.sort_by_key(|b| (!b.current, !data.favorite(b)));
            }
            found
        };
        let (local, remote) = (pick(false), pick(true));
        let mut list = Self::default();
        if let Some(op) = data.operation.filter(|_| filter.is_empty()) {
            let (title, ops) = operation_rows(op);
            list.push(BranchRow::Header(title), 0);
            for r in ops {
                list.push(r, 0);
            }
        }
        if filter.is_empty() {
            list.push(BranchRow::Action(PopupAction::Update), 0);
            list.push(BranchRow::Action(PopupAction::Commit), 0);
            list.push(BranchRow::Action(PopupAction::Push), 0);
            list.push(BranchRow::New(String::new()), 0);
            list.push(BranchRow::Action(PopupAction::CheckoutRevision), 0);
            let recent: Vec<&Branch> = data
                .recent
                .iter()
                .filter_map(|n| local.iter().find(|b| &b.name == n && !b.current))
                .collect();
            if !recent.is_empty() {
                list.push(BranchRow::Header("Recent"), 0);
                for b in recent {
                    list.push(BranchRow::Branch(b.clone()), 0);
                }
            }
            for (title, group, items) in [
                ("Local", Group::Local, &local),
                ("Remote", Group::Remote, &remote),
            ] {
                if items.is_empty() {
                    continue;
                }
                list.push(BranchRow::Header(title), 0);
                // Plain branches and folders in the order of their first branch (the current
                // branch's first).
                let mut seen: Vec<&str> = Vec::new();
                for b in items.iter() {
                    match folder_of(group, &b.name) {
                        None => list.push(BranchRow::Branch(b.clone()), 0),
                        Some(prefix) if seen.contains(&prefix) => {}
                        Some(prefix) => {
                            seen.push(prefix);
                            let open = !collapsed.contains(&folder_key(group, prefix));
                            list.push(BranchRow::Folder(group, prefix.to_owned(), open), 0);
                            if open {
                                for b in items
                                    .iter()
                                    .filter(|b| folder_of(group, &b.name) == Some(prefix))
                                {
                                    list.push(BranchRow::Branch(b.clone()), 1);
                                }
                            }
                        }
                    }
                }
            }
            if !data.tags.is_empty() {
                let open = !collapsed.contains(&folder_key(Group::Tags, ""));
                list.push(BranchRow::Folder(Group::Tags, String::new(), open), 0);
                if open {
                    for t in &data.tags {
                        list.push(BranchRow::Tag(t.clone()), 1);
                    }
                }
            }
        } else {
            let exists = data.branches.iter().any(|b| !b.remote && b.name == filter);
            if !exists && valid_branch_name(filter) {
                list.push(BranchRow::New(filter.to_owned()), 0);
            }
            for (title, group) in [("Local", local), ("Remote", remote)] {
                if !group.is_empty() {
                    list.push(BranchRow::Header(title), 0);
                    for b in group {
                        list.push(BranchRow::Branch(b), 0);
                    }
                }
            }
            let mut tags: Vec<(i64, &Tag)> = data
                .tags
                .iter()
                .filter_map(|t| fuzzy_match(filter, &t.name).map(|m| (m.score, t)))
                .collect();
            tags.sort_by(|a, b| rank((a.0, &a.1.name), (b.0, &b.1.name)));
            if !tags.is_empty() {
                list.push(BranchRow::Header("Tags"), 0);
                for (_, t) in tags {
                    list.push(BranchRow::Tag(t.clone()), 0);
                }
            }
        }
        list.selected = if filter.is_empty() {
            list.rows
                .iter()
                .position(|r| matches!(r, BranchRow::Branch(b) if b.current))
                .or_else(|| {
                    list.rows
                        .iter()
                        .position(|r| !matches!(r, BranchRow::Header(_)))
                })
        } else {
            // The best existing match beats "new branch"; a new name is selected when nothing matches.
            list.rows
                .iter()
                .position(|r| matches!(r, BranchRow::Branch(_)))
                .or_else(|| {
                    list.rows
                        .iter()
                        .position(|r| matches!(r, BranchRow::Tag(_)))
                })
                .or_else(|| {
                    list.rows
                        .iter()
                        .position(|r| matches!(r, BranchRow::New(_)))
                })
        };
        list
    }

    /// One row that confirms `text`, or none.
    fn single(text: &str, ok: bool) -> Self {
        if ok {
            Self {
                rows: vec![BranchRow::New(text.to_owned())],
                selected: Some(0),
                depth: vec![0],
            }
        } else {
            Self::default()
        }
    }

    /// Rows while a new branch is named: only the new name, if it can be a branch (behaviour 7).
    pub fn naming(branches: &[Branch], name: &str) -> Self {
        let name = name.trim();
        let exists = branches.iter().any(|b| !b.remote && b.name == name);
        Self::single(name, valid_branch_name(name) && !exists)
    }

    /// Rows while a revision is typed: the revision, if it can be one (v5 behaviour 16).
    pub fn revision(rev: &str) -> Self {
        let rev = rev.trim();
        Self::single(rev, valid_revision(rev))
    }

    /// Moves the selection, skipping headings, wrapping around (behaviour 3).
    pub fn step(&mut self, down: bool) {
        let rows: Vec<usize> = (0..self.rows.len())
            .filter(|i| !matches!(self.rows[*i], BranchRow::Header(_)))
            .collect();
        let Some(at) = self
            .selected
            .and_then(|s| rows.iter().position(|r| *r == s))
        else {
            return;
        };
        let n = rows.len();
        self.selected = Some(rows[if down { (at + 1) % n } else { (at + n - 1) % n }]);
    }

    /// The selected row.
    pub fn selected_row(&self) -> Option<&BranchRow> {
        self.rows.get(self.selected?)
    }

    /// The folder row that holds row `i`, if it is inside one.
    pub fn parent_folder(&self, i: usize) -> Option<usize> {
        if self.depth.get(i).copied().unwrap_or(0) == 0 {
            return None;
        }
        (0..i)
            .rev()
            .find(|j| matches!(self.rows[*j], BranchRow::Folder(..)))
    }

    /// Index of the first row equal to `row`.
    pub fn position(&self, row: &BranchRow) -> Option<usize> {
        self.rows.iter().position(|r| r == row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, remote: bool, current: bool) -> Branch {
        Branch {
            name: name.into(),
            remote,
            current,
            target: "abc1234".into(),
        }
    }

    fn all() -> Vec<Branch> {
        vec![
            b("main", false, true),
            b("feat/login", false, false),
            b("origin/main", true, false),
            b("origin/feat/search", true, false),
        ]
    }

    #[test]
    fn operation_rows_footer_and_widget_labels() {
        use idecraft_vcs::BranchState;
        let list = BranchList::build_with(&[b("main", false, true)], "", Some(Operation::Rebase));
        assert_eq!(list.rows[0], BranchRow::Header("Rebase in progress"));
        assert_eq!(
            list.rows[1..4],
            [
                BranchRow::Op(OpAction::Continue, "Continue Rebase".into()),
                BranchRow::Op(OpAction::Abort, "Abort Rebase".into()),
                BranchRow::Op(OpAction::Skip, "Skip Commit".into()),
            ]
        );
        assert!(
            matches!(list.selected_row(), Some(BranchRow::Branch(b)) if b.current),
            "the current branch stays selected"
        );
        let (_, merge) = operation_rows(Operation::Merge);
        assert_eq!(merge.len(), 2, "a merge has no skip");
        let filtered =
            BranchList::build_with(&[b("main", false, true)], "ma", Some(Operation::Merge));
        assert!(!filtered.rows.iter().any(|r| matches!(r, BranchRow::Op(..))));
        assert!(FOOTER.contains("⌘↩ Merge into current") && FOOTER.contains("→ More"));
        let mut s = BranchState {
            branch: Some("main".into()),
            ..Default::default()
        };
        assert_eq!(widget_label(&s), Some(("main".into(), false)));
        (s.ahead, s.behind) = (2, 1);
        assert_eq!(widget_label(&s), Some(("main ↑2 ↓1".into(), false)));
        s.behind = 0;
        assert_eq!(widget_label(&s), Some(("main ↑2".into(), false)));
        s.operation = Some(Operation::Merge);
        assert_eq!(widget_label(&s), Some(("Merging main".into(), true)));
        s.operation = Some(Operation::Rebase);
        s.branch = None;
        s.rebasing = Some("topic".into());
        assert_eq!(widget_label(&s), Some(("Rebasing topic".into(), true)));
        s.operation = Some(Operation::CherryPick);
        assert_eq!(widget_label(&s), Some(("Cherry-picking".into(), true)));
        assert_eq!(widget_label(&BranchState::default()), None, "detached HEAD");
    }

    #[test]
    fn groups_current_selected_and_filter() {
        let list = BranchList::build(&all(), "");
        assert_eq!(
            list.rows[..5],
            [
                BranchRow::Action(PopupAction::Update),
                BranchRow::Action(PopupAction::Commit),
                BranchRow::Action(PopupAction::Push),
                BranchRow::New(String::new()),
                BranchRow::Action(PopupAction::CheckoutRevision),
            ],
            "v5 behaviour 15: the actions at the top"
        );
        assert!(matches!(list.selected_row(), Some(BranchRow::Branch(b)) if b.name == "main"));
        assert_eq!(list.rows.len(), list.depth.len());
        let f = BranchList::build(&all(), "search");
        assert!(matches!(&f.rows[0], BranchRow::New(n) if n == "search"));
        assert!(
            matches!(f.selected_row(), Some(BranchRow::Branch(b)) if b.name == "origin/feat/search"),
            "an existing match is preferred to a new branch"
        );
        assert!(!f.rows.iter().any(|r| matches!(r, BranchRow::Action(_))));
        let exact = BranchList::build(&all(), "main");
        assert!(!exact.rows.iter().any(|r| matches!(r, BranchRow::New(_))));
        let new = BranchList::build(&all(), "zzz-topic");
        assert!(matches!(new.selected_row(), Some(BranchRow::New(n)) if n == "zzz-topic"));
    }

    #[test]
    fn recent_folders_tracking_and_tags() {
        let data = PopupData {
            branches: vec![
                b("main", false, true),
                b("feature/conflict", false, false),
                b("feature/discount", false, false),
                b("fix", false, false),
                b("origin/feature/discount", true, false),
                b("origin/main", true, false),
            ],
            tracking: vec![
                Tracking {
                    branch: "main".into(),
                    current: true,
                    upstream: Some("origin/main".into()),
                    ahead: 1,
                    behind: 1,
                    gone: false,
                },
                Tracking {
                    branch: "fix".into(),
                    current: false,
                    upstream: Some("origin/fix".into()),
                    ahead: 0,
                    behind: 0,
                    gone: true,
                },
            ],
            recent: vec!["feature/discount".into(), "main".into(), "gone".into()],
            tags: vec![Tag {
                name: "v1.0".into(),
                target: "abc1234".into(),
            }],
            operation: None,
            favorites: Default::default(),
        };
        let mut collapsed = initially_collapsed();
        let list = BranchList::build_from(&data, "", &collapsed);
        let at = |row: &BranchRow| list.position(row).unwrap();
        let recent = at(&BranchRow::Header("Recent"));
        assert!(
            matches!(&list.rows[recent + 1], BranchRow::Branch(b) if b.name == "feature/discount"),
            "Recent lists the branch, not the current one or a deleted one"
        );
        assert!(matches!(&list.rows[recent + 2], BranchRow::Header("Local")));
        let folder = at(&BranchRow::Folder(Group::Local, "feature".into(), true));
        assert_eq!(list.depth[folder + 1], 1);
        assert_eq!(
            short_label(row_name(&list.rows[folder + 1]).unwrap(), 1),
            "conflict"
        );
        assert_eq!(list.parent_folder(folder + 1), Some(folder));
        assert_eq!(list.parent_folder(folder), None);
        let origin = at(&BranchRow::Folder(Group::Remote, "origin".into(), true));
        assert_eq!(
            short_label(row_name(&list.rows[origin + 1]).unwrap(), 1),
            "main",
            "v8: origin/main is a favourite by default and comes first"
        );
        assert_eq!(
            short_label(row_name(&list.rows[origin + 2]).unwrap(), 1),
            "feature/discount"
        );
        assert_eq!(
            list.rows.last(),
            Some(&BranchRow::Folder(Group::Tags, String::new(), false)),
            "IMP-0323: only the tags start collapsed"
        );
        assert_eq!(
            data.tracking_hint("main").as_deref(),
            Some("↑1 ↓1 origin/main")
        );
        assert_eq!(
            data.tracking_hint("fix").as_deref(),
            Some("origin/fix (gone)")
        );
        assert_eq!(data.tracking_hint("feature/conflict"), None);
        // Collapsing and opening folders.
        collapsed.insert(folder_key(Group::Local, "feature"));
        collapsed.remove(&folder_key(Group::Tags, ""));
        let list = BranchList::build_from(&data, "", &collapsed);
        let folder = list
            .position(&BranchRow::Folder(Group::Local, "feature".into(), false))
            .unwrap();
        assert!(matches!(&list.rows[folder + 1], BranchRow::Branch(b) if b.name == "fix"));
        assert!(matches!(list.rows.last(), Some(BranchRow::Tag(t)) if t.name == "v1.0"));
        // A filter finds tags too, flat.
        let f = BranchList::build_from(&data, "v1", &collapsed);
        assert!(matches!(f.selected_row(), Some(BranchRow::Tag(t)) if t.name == "v1.0"));
        assert!(f.depth.iter().all(|d| *d == 0));
    }

    #[test]
    fn submenus_follow_the_kind_of_row() {
        let names = |row: BranchRow| -> Vec<BranchAction> {
            branch_actions(&row, Some("main"), false)
                .into_iter()
                .map(|(a, _)| a)
                .collect()
        };
        assert_eq!(
            names(BranchRow::Branch(b("main", false, true))),
            [
                BranchAction::NewBranchFrom,
                BranchAction::Push,
                BranchAction::Rename,
                BranchAction::Favorite
            ]
        );
        let local = branch_actions(
            &BranchRow::Branch(b("topic", false, false)),
            Some("main"),
            false,
        );
        assert_eq!(local.len(), 11);
        assert_eq!(
            local[10],
            (BranchAction::Favorite, "Mark as Favorite".into()),
            "v8 behaviour 29: the last entry"
        );
        assert_eq!(local[4].1, "Merge 'topic' into 'main'");
        assert_eq!(local[2].1, "Checkout and Rebase onto 'main'");
        // v6: Compare with, Show Diff with Working Tree and Push… for another local branch.
        assert_eq!(local[5].1, "Compare with 'main'");
        assert_eq!(local[6].0, BranchAction::DiffWorkingTree);
        assert_eq!(local[7].0, BranchAction::Push);
        let remote = names(BranchRow::Branch(b("origin/x", true, false)));
        assert!(remote.contains(&BranchAction::CompareWith));
        assert!(!remote.contains(&BranchAction::Push));
        assert!(
            !names(BranchRow::Branch(b("origin/x", true, false))).contains(&BranchAction::Delete),
            "remote branches are not deleted from the popup"
        );
        let tag = BranchRow::Tag(Tag {
            name: "v1".into(),
            target: "abc".into(),
        });
        assert_eq!(names(tag.clone())[0], BranchAction::Checkout);
        assert!(
            !names(tag).contains(&BranchAction::Favorite),
            "tags have no favourites"
        );
        assert!(branch_actions(&BranchRow::Header("Local"), None, false).is_empty());
        assert_eq!(PopupAction::Update.action(), Some("Vcs.UpdateProject"));
        assert_eq!(PopupAction::CheckoutRevision.action(), None);
    }

    #[test]
    fn step_wraps_and_names_are_checked() {
        let mut list = BranchList::build(&all(), "");
        list.step(false);
        assert_eq!(
            list.selected_row(),
            Some(&BranchRow::Action(PopupAction::CheckoutRevision))
        );
        list.step(true);
        list.step(true);
        assert!(matches!(
            list.selected_row(),
            Some(BranchRow::Folder(Group::Local, p, true)) if p == "feat"
        ));
        assert!(valid_branch_name("feat/x-1"));
        assert_eq!(
            BranchList::naming(&all(), "topic").rows,
            [BranchRow::New("topic".into())]
        );
        assert!(BranchList::naming(&all(), "main").rows.is_empty());
        assert!(BranchList::naming(&all(), "a b").rows.is_empty());
        assert_eq!(
            BranchList::revision("v1.0^").rows,
            [BranchRow::New("v1.0^".into())]
        );
        assert!(BranchList::revision("-x").rows.is_empty());
        for bad in [
            "", "a b", "a..b", "-x", "x/", "x.lock", "a~b", "a:b", "x.", "a//b",
        ] {
            assert!(!valid_branch_name(bad), "{bad}");
        }
        assert!(
            BranchList::build(&all(), "a b")
                .rows
                .iter()
                .all(|r| !matches!(r, BranchRow::New(_)))
        );
    }

    /// SPEC-0068 v8 AK-15: favourites come first after the current branch, folders move up with
    /// their favourite, the filter keeps the match order, rows know their star.
    #[test]
    fn favourites_come_first_with_their_stars() {
        let mut data = PopupData {
            branches: vec![
                b("topic", false, true),
                b("alpha", false, false),
                b("bugfix/a", false, false),
                b("main", false, false),
                b("zeta", false, false),
                b("origin/alpha", true, false),
                b("origin/main", true, false),
            ],
            ..PopupData::default()
        };
        data.favorites.set("zeta", false, true);
        data.favorites.set("bugfix/a", false, true);
        let list = BranchList::build_from(&data, "", &initially_collapsed());
        let local = list.position(&BranchRow::Header("Local")).unwrap();
        let names: Vec<String> = list.rows[local + 1..]
            .iter()
            .take_while(|r| !matches!(r, BranchRow::Header(_)))
            .map(|r| match r {
                BranchRow::Branch(b) => b.name.clone(),
                BranchRow::Folder(_, p, _) => format!("{p}/"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            names,
            ["topic", "bugfix/", "bugfix/a", "main", "zeta", "alpha"],
            "current, then favourites (a folder with its favourite), then the rest"
        );
        let remote = list.position(&BranchRow::Header("Remote")).unwrap();
        assert!(
            matches!(&list.rows[remote + 2], BranchRow::Branch(b) if b.name == "origin/main"),
            "the remote default favourite first in its folder"
        );
        assert!(data.row_favorite(&BranchRow::Branch(b("main", false, false))));
        assert!(!data.row_favorite(&BranchRow::Branch(b("main", true, false))));
        assert!(!data.row_favorite(&BranchRow::Header("Local")));
        // Unmarking the default puts main back in name order.
        data.favorites.set("main", false, false);
        let list = BranchList::build_from(&data, "", &initially_collapsed());
        let pos = |n: &str| {
            list.rows
                .iter()
                .position(|r| matches!(r, BranchRow::Branch(b) if b.name == n))
                .unwrap()
        };
        assert!(pos("alpha") < pos("main"));
        // While filtering, the order of the match stays.
        let f = BranchList::build_from(&data, "alpha", &initially_collapsed());
        assert!(matches!(f.selected_row(), Some(BranchRow::Branch(b)) if b.name == "alpha"));
        assert_eq!(favorite_label(true), "Unmark as Favorite");
    }
}
