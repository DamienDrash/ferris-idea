//! The Git log tool window (`SPEC-0086`): the commits of all branches and tags with the commit graph
//! and labels, a branches pane, a filter bar (text or hash with regular expressions and case,
//! branch, user, date, paths), the selected commit's changed files and details; a changed file
//! opens its diff. A context menu on commits (v7 behaviour 13), labels placed and merged as in the
//! reference (v7 behaviour 14), `*` for a different committer (v7 behaviour 15), and the branches
//! pane with incoming/outgoing counters and an action bar (v7 behaviour 16). v8 (behaviour 17):
//! the reference's other commit entries, a Checkout page and a page per branch at the commit.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use ferris_idea_vcs::{
    ActionState, CommitAction, CommitDetails, CommitInfo, Repo, ResetMode, commit_actions,
};
use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, PathBuilder, Pixels, Point, Window, canvas,
    div, point, prelude::*, px, rgb, uniform_list,
};

use crate::BranchesEvent;
use crate::branches_model::{BranchAction as PopupBranchAction, BranchRow, branch_actions};
use crate::commit_model::status_color_key;
use crate::git_branch_tree::{BranchName, TreeItem, TreeRow, tree_rows, tree_rows_with};
use crate::git_graph::{GraphRow, dots_only, layout};
use crate::git_log_marks::{
    HistoryFacts, Label, LabelKind, author_text, menu_branches, remote_tips, row_labels,
    tracking_text,
};
use crate::git_log_model::{
    DateRange, DateStyle, LocalZone, LogFilter, apply, authors, date_text, local_date_style,
    reachable, reselect, tip_of,
};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;

/// Most commits listed (behaviour 2).
const LIMIT: usize = 1000;

/// How often the window looks whether a branch, tag or HEAD moved (`SPEC-0086` v2 behaviour 6).
const REFS_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// Width of one lane of the graph (`SPEC-0086` v3 behaviour 8).
const LANE: f32 = 12.;
/// Most lanes drawn; lines further right are cut off.
const MAX_LANES: usize = 16;
/// Height of a row of the commit list.
const ROW: f32 = 24.;
/// Approximate width of a character of the list's text, to size the author and date columns.
const CHAR: f32 = 7.;
/// Width of the branches pane (`SPEC-0086` v4 behaviour 9).
const BRANCHES_W: f32 = 210.;

/// Width of the branches pane's action bar (`SPEC-0086` v7 behaviour 16).
const ACTIONS_W: f32 = 28.;

/// Labels pointing at a commit and the commit's graph row.
type Row = (CommitInfo, Vec<Label>, GraphRow);

/// The current branch name and the labels per commit, compared to see whether to read again.
type Refs = (Option<String>, BTreeMap<String, Vec<String>>);

/// What the open commit menu shows (`SPEC-0086` v7 behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuPage {
    /// The actions.
    Main,
    /// The modes of Reset Current Branch to Here, one highlighted.
    Reset(usize),
    /// The name of a new branch or tag, or a branch's new name.
    Name {
        /// What the name is for.
        kind: NameKind,
        /// Typed name.
        text: String,
        /// Why the name is refused.
        error: Option<String>,
    },
    /// The branches at the commit and the revision to check out, one highlighted (v8).
    Checkout(usize),
    /// The submenu of a branch at the commit, one entry highlighted (v8).
    Branch {
        /// The branch.
        name: String,
        /// A remote-tracking branch.
        remote: bool,
        /// Highlighted entry.
        cursor: usize,
    },
}

/// What the menu's name page names (`SPEC-0086` v7 behaviour 13, v8 behaviour 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameKind {
    /// A new branch at the commit.
    Branch,
    /// A new tag at the commit.
    Tag,
    /// A new branch starting at this branch (a branch page's New Branch from…).
    BranchFrom(String),
    /// The new name of this branch (a branch page's Rename…).
    Rename(String),
}

impl NameKind {
    fn noun(&self) -> &'static str {
        if *self == Self::Tag { "tag" } else { "branch" }
    }
}

/// A row of the commit menu's main page: an entry, or the page of a branch at the commit (v8).
#[derive(Debug, Clone, PartialEq, Eq)]
enum MenuRow {
    /// Entry `.0` of `entries`.
    Entry(usize),
    /// The page of branch `.0`; `.1`: remote-tracking.
    Branch(String, bool),
}

/// The context menu of a commit (`SPEC-0086` v7 behaviour 13).
struct CommitMenu {
    commit: String,
    entries: Vec<ActionState>,
    /// The main page's rows in order: entries and branch pages (v8).
    rows: Vec<MenuRow>,
    /// Local branches at the commit; when there are any, Checkout opens a page (v8).
    checkout: Vec<String>,
    /// Highlighted entry; disabled ones can be highlighted to read their reason.
    cursor: Option<usize>,
    page: MenuPage,
    /// Window position of the menu; `None` when opened by key (then under the list's top).
    at: Option<Point<Pixels>>,
    /// The reason shown after choosing a disabled entry.
    note: Option<String>,
}

/// The filter whose list is open under its button (`SPEC-0086` v5 behaviour 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// Branch filter.
    Branch,
    /// User (author) filter.
    User,
    /// Date filter.
    Date,
    /// Paths filter.
    Paths,
}

/// An open filter list.
struct Menu {
    kind: FilterKind,
    /// Typed text: narrows the list; for paths, the path to add.
    query: String,
    row: usize,
}

/// What choosing an entry of a filter list does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MenuItem {
    /// Branch filter off.
    AllBranches,
    /// Only this branch.
    Branch(String),
    /// User filter off.
    AllUsers,
    /// Only this author.
    User(String),
    /// This date range.
    Date(DateRange),
    /// Add the typed path.
    AddPath(String),
    /// Remove this path.
    RemovePath(String),
    /// Remove every path.
    ClearPaths,
}

/// What the tool window asks the workbench to do.
pub enum GitLogEvent {
    /// Show `path` before and after `commit`.
    ShowDiff {
        /// Full commit id.
        commit: String,
        /// Repository-relative path.
        path: String,
    },
    /// Apply the change of this commit onto the current branch (SPEC-0106 behaviour 8).
    CherryPick(String),
    /// Interactively rebase the commits from this one on (SPEC-0145).
    RebaseFrom(String),
    /// An entry of the commit menu the workbench carries out (`SPEC-0086` v7 behaviour 13).
    Commit {
        /// Full commit id.
        commit: String,
        /// The entry.
        action: CommitAction,
        /// For Reset: the mode.
        mode: Option<ResetMode>,
        /// For New Branch and New Tag: the name.
        name: Option<String>,
    },
    /// Check out this local branch (the branches pane's action bar, v7 behaviour 16).
    CheckoutBranch(String),
    /// What a branch page of the commit menu asks for, as the branch popup does (v8).
    Branch(BranchesEvent),
    /// Fetch every remote (the branches pane's action bar).
    Fetch,
    /// Close the tool window.
    Close,
}

/// History, labels and the state of the list.
pub struct GitLogView {
    root: PathBuf,
    /// `None` while loading.
    commits: Option<Vec<CommitInfo>>,
    labels: BTreeMap<String, Vec<String>>,
    /// Branch HEAD points at; `None` while detached (`SPEC-0086` v3 behaviour 2).
    current: Option<String>,
    zone: LocalZone,
    style: DateStyle,
    error: Option<String>,
    /// Text, user, date and path filters (`SPEC-0086` v5 behaviour 10).
    filter: LogFilter,
    /// Why the text filter is not a valid regular expression.
    text_error: Option<String>,
    /// Authors of the history, most commits first.
    authors: Vec<String>,
    /// Commits that change one of the filter's paths, once read; `None` while being read.
    touching: Option<HashSet<String>>,
    /// The open filter list.
    menu: Option<Menu>,
    shown: Vec<usize>,
    /// Graph of the shown rows (behaviour 8).
    graph: Vec<GraphRow>,
    /// Row of the selected commit; none until the user chooses one (`SPEC-0086` v3 behaviour 2).
    selected: Option<usize>,
    details: Option<CommitDetails>,
    /// Which details are being read, so a late answer for an earlier selection is dropped.
    asked: Option<String>,
    /// A read of the history is under way.
    reading: bool,
    /// A commit to select once the history is read (`SPEC-0277` behaviour 5).
    wanted: Option<String>,
    /// Keys go to the file list instead of the commit list.
    in_files: bool,
    file: usize,
    /// Branches of the repository and whether HEAD is detached (`SPEC-0086` v4 behaviour 9).
    branches: Vec<BranchName>,
    detached: bool,
    /// The branch whose commits alone are listed.
    branch_filter: Option<String>,
    /// Text of the branches pane's filter field.
    tree_filter: String,
    /// Keys of the closed folders of the branches pane.
    tree_closed: BTreeSet<String>,
    /// Highlighted row of the branches pane.
    tree_row: usize,
    /// Keys go to the branches pane.
    in_branches: bool,
    /// Commits of the checked-out branch, tinted in the list (`SPEC-0086` v6 behaviour 12).
    on_head: HashSet<String>,
    /// The open commit menu (`SPEC-0086` v7 behaviour 13).
    commit_menu: Option<CommitMenu>,
    /// Commits ahead of and behind the upstream per local branch (v7 behaviour 16).
    tracking: HashMap<String, (usize, usize)>,
    /// The name typed for a new branch from the branches pane, while its field is open.
    tree_new: Option<String>,
    /// Favourite branches of the repository (`SPEC-0086` v10 behaviour 19).
    favorites: ferris_idea_core::RepoFavorites,
    /// The branches pane lists only the favourites (v10 behaviour 19).
    only_favorites: bool,
    focus: FocusHandle,
    /// The repository of a remote project's host (`SPEC-0318` v2 detail 7).
    remote: Option<crate::workbench_view::HostCall>,
    /// Counts the reads started; only the latest one's result is shown, so a read of this
    /// machine's folder that ends after the host's cannot replace the host's history.
    reads: u64,
}

impl EventEmitter<GitLogEvent> for GitLogView {}

/// What the log shows of the host repository: commits from HEAD, labels from the branches, the
/// current branch and the branches (`git.log`, `git.branches`, `git.status`).
type HostLog = (
    Vec<CommitInfo>,
    BTreeMap<String, Vec<String>>,
    Option<String>,
    ferris_idea_vcs::Branches,
);

/// The commits of `ids` that change one of `paths` in the host repository.
fn host_touching(
    host: &crate::workbench_view::HostCall,
    ids: &[String],
    paths: &[String],
) -> Result<HashSet<String>, ferris_idea_vcs::GitError> {
    let v = host(
        "git.commits_touching",
        serde_json::json!({ "ids": ids, "paths": paths }),
    )
    .map_err(ferris_idea_vcs::GitError::Git)?;
    Ok(v["ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| i.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default())
}

fn host_log(host: &crate::workbench_view::HostCall) -> Result<HostLog, String> {
    let log = host("git.log", serde_json::json!({ "limit": LIMIT }))?;
    let commits: Vec<CommitInfo> =
        serde_json::from_value(log["commits"].clone()).map_err(|e| e.to_string())?;
    let branches: ferris_idea_vcs::Branches =
        serde_json::from_value(host("git.branches", serde_json::json!({}))?)
            .map_err(|e| e.to_string())?;
    let current = host("git.status", serde_json::json!({}))
        .ok()
        .and_then(|s| s["branch"].as_str().map(str::to_owned));
    let mut labels: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for b in &branches.branches {
        if !b.name.ends_with("/HEAD") {
            labels
                .entry(b.target.clone())
                .or_default()
                .push(b.name.clone());
        }
    }
    if branches.detached
        && let Some(head) = commits.first()
    {
        labels
            .entry(head.id.clone())
            .or_default()
            .push("HEAD".into());
    }
    Ok((commits, labels, current, branches))
}

impl GitLogView {
    /// A tool window for the repository containing `root`; the history is read in the background
    /// and read again whenever a branch, tag or HEAD moves (`SPEC-0086` v2 behaviour 6).
    pub fn new(root: &Path, cx: &mut Context<Self>) -> Self {
        let watch = root.to_path_buf();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFS_POLL).await;
                let root = watch.clone();
                let refs = cx
                    .background_spawn(async move {
                        let repo = Repo::discover(&root).ok()?;
                        Some::<Refs>((repo.current_branch().ok()?, repo.ref_labels().ok()?))
                    })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    // A host repository is read again on `git.changed`, never after this
                    // machine's refs (the stand-in folder is not the host's repository).
                    if this.remote.is_none()
                        && this.commits.is_some()
                        && !this.reading
                        && refs.is_some_and(|(current, labels)| {
                            labels != this.labels || current != this.current
                        })
                    {
                        this.reload(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut view = Self {
            root: root.to_path_buf(),
            commits: None,
            labels: BTreeMap::new(),
            current: None,
            zone: LocalZone::default(),
            style: DateStyle::ISO,
            error: None,
            filter: LogFilter::default(),
            text_error: None,
            authors: Vec::new(),
            touching: None,
            menu: None,
            shown: Vec::new(),
            graph: Vec::new(),
            selected: None,
            details: None,
            asked: None,
            wanted: None,
            reading: false,
            in_files: false,
            file: 0,
            branches: Vec::new(),
            detached: false,
            branch_filter: None,
            tree_filter: String::new(),
            tree_closed: BTreeSet::new(),
            tree_row: 0,
            in_branches: false,
            on_head: HashSet::new(),
            commit_menu: None,
            tracking: HashMap::new(),
            tree_new: None,
            favorites: ferris_idea_core::RepoFavorites::default(),
            only_favorites: false,
            focus: cx.focus_handle(),
            remote: None,
            reads: 0,
        };
        view.reload(cx);
        view
    }

    /// Shows the history of a remote project's host from now on (`SPEC-0318` v2 detail 7); it is
    /// read again on `git.changed` ([`GitLogView::reload`]).
    pub fn set_remote(&mut self, host: crate::workbench_view::HostCall, cx: &mut Context<Self>) {
        self.remote = Some(host);
        self.error = None;
        self.reload(cx);
    }

    /// Whether the view shows a remote project's host repository.
    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// The subjects of the commits read, newest first (the window self-test).
    pub fn summaries(&self) -> Vec<String> {
        self.commits
            .iter()
            .flatten()
            .map(|c| c.summary.clone())
            .collect()
    }

    /// Reads the history and labels again, keeping the filter and the selected commit when it is
    /// still there: after the IDE's own commits, rebases and resets, and after changes made
    /// outside (`SPEC-0086` v2 behaviour 6).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.reading = true;
        self.reads += 1;
        let this_read = self.reads;
        let load = self.root.clone();
        let remote = self.remote.clone();
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    if let Some(host) = remote {
                        let (commits, labels, current, branches) = host_log(&host)?;
                        return Ok::<_, String>((
                            commits,
                            labels,
                            current,
                            (
                                branches,
                                Vec::new(),
                                ferris_idea_core::RepoFavorites::default(),
                            ),
                            LocalZone::system(),
                            local_date_style(),
                        ));
                    }
                    let repo = Repo::discover(&load).map_err(|e| e.to_string())?;
                    let commits = repo.log_all(LIMIT).map_err(|e| e.to_string())?;
                    let labels = repo.ref_labels().map_err(|e| e.to_string())?;
                    let current = repo.current_branch().map_err(|e| e.to_string())?;
                    let branches = repo.branches().map_err(|e| e.to_string())?;
                    // Without an upstream or outside a work tree there is nothing to count.
                    let tracking = repo.tracking().unwrap_or_default();
                    // Favourites of this repository (`SPEC-0086` v10 behaviour 19).
                    let favorites = favorites_of(&load, &repo);
                    Ok::<_, String>((
                        commits,
                        labels,
                        current,
                        (branches, tracking, favorites),
                        LocalZone::system(),
                        local_date_style(),
                    ))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this_read != this.reads {
                    // A later read (the host's, after `set_remote`) decides.
                    return;
                }
                this.reading = false;
                let previous = this.selected_commit().map(|c| c.id.clone());
                match read {
                    Ok((
                        commits,
                        labels,
                        current,
                        (branches, tracking, favorites),
                        zone,
                        style,
                    )) => {
                        this.tracking = tracking
                            .into_iter()
                            .map(|t| (t.branch, (t.ahead, t.behind)))
                            .collect();
                        this.authors = authors(&commits);
                        this.commits = Some(commits);
                        this.labels = labels;
                        this.current = current;
                        this.detached = branches.detached;
                        this.branches = branches
                            .branches
                            .into_iter()
                            .map(|b| BranchName {
                                favorite: favorites.is_favorite(&b.name, b.remote),
                                name: b.name,
                                remote: b.remote,
                                current: b.current,
                            })
                            .collect();
                        this.favorites = favorites;
                        if this
                            .branch_filter
                            .as_deref()
                            .is_some_and(|b| tip_of(&this.labels, b).is_none())
                        {
                            this.branch_filter = None;
                        }
                        this.refresh_rows(previous.as_deref());
                        this.load_touching(cx);
                        this.zone = zone;
                        this.style = style;
                        this.error = None;
                        if let Some(id) = this.wanted.take() {
                            this.choose_commit(&id);
                        }
                    }
                    Err(e) => {
                        this.commits = Some(Vec::new());
                        this.shown.clear();
                        this.graph.clear();
                        this.selected = None;
                        this.error = Some(e);
                    }
                }
                // A rewritten commit has new details even when the row stays selected.
                this.asked = None;
                this.load_details(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether a read of the history is under way.
    pub fn reading(&self) -> bool {
        self.reading
    }

    /// Focus handle of the window.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Whether the history is still being read.
    pub fn loading(&self) -> bool {
        self.commits.is_none()
    }

    /// Subjects of the listed commits, in order.
    pub fn subjects(&self) -> Vec<String> {
        let Some(commits) = &self.commits else {
            return Vec::new();
        };
        self.shown
            .iter()
            .map(|&i| commits[i].summary.clone())
            .collect()
    }

    /// Graph rows of the listed commits, in order (`SPEC-0086` v3 behaviour 8).
    pub fn graph(&self) -> &[GraphRow] {
        &self.graph
    }

    /// Row of the selected commit; `None` until one is chosen (`SPEC-0086` v3 behaviour 2).
    pub fn selected_row(&self) -> Option<usize> {
        self.selected
    }

    /// Branch HEAD points at, `None` while detached.
    pub fn current_branch(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// The branch whose commits alone are listed (`SPEC-0086` v4 behaviour 9).
    pub fn branch_filter(&self) -> Option<&str> {
        self.branch_filter.as_deref()
    }

    /// Visible rows of the branches pane (`SPEC-0086` v4 behaviour 9, v10 behaviour 19).
    pub fn branch_rows(&self) -> Vec<TreeRow> {
        tree_rows_with(
            &self.branches,
            self.detached,
            &self.tree_filter,
            &self.tree_closed,
            self.only_favorites,
        )
    }

    /// Highlights the pane row of branch `name` (not the HEAD row), as Up/Down would; `false` when
    /// it is not listed (self-test).
    pub fn highlight_branch_row(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let item = TreeItem::Branch(name.to_owned());
        let at = self
            .branch_rows()
            .iter()
            .position(|r| r.item == item && r.text != crate::git_branch_tree::CURRENT_ROW);
        if let Some(at) = at {
            self.tree_row = at;
            self.in_branches = true;
            cx.notify();
        }
        at.is_some()
    }

    /// Whether the branches pane lists only the favourites (v10 behaviour 19).
    pub fn only_favorites(&self) -> bool {
        self.only_favorites
    }

    /// Reads the favourites again, after they changed in the branch popup or through MCP.
    pub fn reload_favorites(&mut self, cx: &mut Context<Self>) {
        if let Ok(repo) = Repo::discover(&self.root) {
            self.favorites = favorites_of(&self.root, &repo);
            self.apply_favorites();
            cx.notify();
        }
    }

    fn apply_favorites(&mut self) {
        for b in &mut self.branches {
            b.favorite = self.favorites.is_favorite(&b.name, b.remote);
        }
    }

    /// Marks or unmarks branch `name` (remote-tracking when `remote`) and tells the workbench to
    /// keep it (`SPEC-0086` v10 behaviour 19).
    pub fn set_favorite(
        &mut self,
        name: &str,
        remote: bool,
        favorite: bool,
        cx: &mut Context<Self>,
    ) {
        self.favorites.set(name, remote, favorite);
        self.apply_favorites();
        cx.emit(GitLogEvent::Branch(BranchesEvent::Favorite(
            name.to_owned(),
            remote,
            favorite,
        )));
        cx.notify();
    }

    /// Marks or unmarks the branch of pane row `row` (the star, Space, the action bar).
    fn toggle_row_favorite(&mut self, row: usize, cx: &mut Context<Self>) {
        let rows = self.branch_rows();
        let Some(TreeItem::Branch(name)) = rows.get(row).map(|r| r.item.clone()) else {
            return;
        };
        if name == "HEAD" {
            return;
        }
        let head_row = rows[row].text == crate::git_branch_tree::CURRENT_ROW;
        let remote = row_is_remote(&rows, row);
        let favorite = !self.favorites.is_favorite(&name, remote);
        self.set_favorite(&name, remote, favorite, cx);
        // Keep the same branch highlighted where it moved to (the HEAD row stays in place).
        if head_row {
            return;
        }
        let rows = self.branch_rows();
        let item = TreeItem::Branch(name);
        if let Some(at) = (0..rows.len()).find(|&i| {
            rows[i].item == item
                && rows[i].text != crate::git_branch_tree::CURRENT_ROW
                && row_is_remote(&rows, i) == remote
        }) {
            self.tree_row = at;
        }
    }

    /// Lists only the commits of `branch`, or all again when it is the chosen branch already
    /// (`SPEC-0086` v4 behaviour 9).
    pub fn choose_branch(&mut self, branch: &str, cx: &mut Context<Self>) {
        let previous = self.selected_commit().map(|c| c.id.clone());
        self.branch_filter = if self.branch_filter.as_deref() == Some(branch) {
            None
        } else {
            Some(branch.to_owned())
        };
        self.refresh_rows(previous.as_deref());
        self.load_details(cx);
        cx.notify();
    }

    fn clear_branch_filter(&mut self, cx: &mut Context<Self>) {
        if let Some(b) = self.branch_filter.clone() {
            self.choose_branch(&b, cx);
        }
    }

    /// Lists the commits that match the filters again with their graph, keeping `previous`
    /// selected when it is still listed (`SPEC-0086` v3 behaviours 2, 3 and 8, v4 behaviour 9).
    fn refresh_rows(&mut self, previous: Option<&str>) {
        let Some(commits) = &self.commits else {
            return;
        };
        let in_branch = self.branch_filter.as_deref().map(|b| {
            tip_of(&self.labels, b).map_or_else(Default::default, |tip| reachable(commits, tip))
        });
        // The rows of the checked-out branch get a tint, as in the reference (OBS-0022 L9).
        self.on_head = tip_of(&self.labels, "HEAD")
            .or_else(|| {
                self.current
                    .as_deref()
                    .and_then(|b| tip_of(&self.labels, b))
            })
            .map_or_else(HashSet::new, |tip| reachable(commits, tip));
        // While the commits of the paths are read nothing is listed ("Filtering…").
        let empty = HashSet::new();
        let touching = if self.filter.paths.is_empty() {
            None
        } else {
            Some(self.touching.as_ref().unwrap_or(&empty))
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        match apply(commits, &self.filter, in_branch.as_ref(), touching, now) {
            Ok(shown) => {
                self.shown = shown;
                self.text_error = None;
            }
            Err(e) => {
                self.shown.clear();
                self.text_error = Some(e);
            }
        }
        // The commits of a branch hold all their parents that are listed, so their lines stay
        // true; the other filters leave gaps (v5 behaviour 3).
        self.graph = if !self.filter.active() {
            layout(self.shown.iter().map(|&i| {
                let c = &commits[i];
                (c.id.as_str(), c.parents.iter().map(String::as_str))
            }))
        } else {
            dots_only(self.shown.len())
        };
        self.selected = reselect(commits, &self.shown, previous);
    }

    /// The text, user, date and path filters (`SPEC-0086` v5 behaviour 10).
    pub fn log_filter(&self) -> &LogFilter {
        &self.filter
    }

    /// Authors of the history, most commits first, as the user filter offers them.
    pub fn authors(&self) -> &[String] {
        &self.authors
    }

    /// Whether the commits that change the filter's paths are still being read.
    pub fn filtering_paths(&self) -> bool {
        !self.filter.paths.is_empty() && self.touching.is_none()
    }

    /// Changes the filters and lists the commits again (`SPEC-0086` v5 behaviour 10); the commits
    /// of new paths are read in the background.
    pub fn update_filter(&mut self, change: impl FnOnce(&mut LogFilter), cx: &mut Context<Self>) {
        let previous = self.selected_commit().map(|c| c.id.clone());
        let paths = self.filter.paths.clone();
        change(&mut self.filter);
        if self.filter.paths != paths {
            self.touching = None;
            self.load_touching(cx);
        }
        self.refresh_rows(previous.as_deref());
        self.load_details(cx);
        cx.notify();
    }

    /// Clears every filter, the branch filter included.
    pub fn clear_filters(&mut self, cx: &mut Context<Self>) {
        self.branch_filter = None;
        self.update_filter(|f| *f = LogFilter::default(), cx);
    }

    /// Reads which listed commits change the filter's paths (`SPEC-0086` v5 behaviour 10).
    fn load_touching(&mut self, cx: &mut Context<Self>) {
        let Some(commits) = &self.commits else {
            return;
        };
        if self.filter.paths.is_empty() {
            self.touching = None;
            return;
        }
        let ids: Vec<String> = commits.iter().map(|c| c.id.clone()).collect();
        let paths = self.filter.paths.clone();
        let root = self.root.clone();
        let remote = self.remote.clone();
        cx.spawn(async move |this, cx| {
            let wanted = paths.clone();
            let read = cx
                .background_spawn(async move {
                    match remote {
                        // On the host, with project-relative paths (`SPEC-0318` v6 behaviour 47).
                        Some(host) => host_touching(&host, &ids, &paths),
                        None => {
                            Repo::discover(&root).and_then(|r| r.commits_touching(&ids, &paths))
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.filter.paths != wanted {
                    return;
                }
                match read {
                    Ok(set) => this.touching = Some(set),
                    Err(e) => {
                        this.touching = Some(HashSet::new());
                        this.error = Some(e.to_string());
                    }
                }
                let previous = this.selected_commit().map(|c| c.id.clone());
                this.refresh_rows(previous.as_deref());
                this.load_details(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the list of a filter, or closes it when it is open (`SPEC-0086` v5 behaviour 10).
    pub fn toggle_menu(&mut self, kind: FilterKind, cx: &mut Context<Self>) {
        self.menu = match &self.menu {
            Some(m) if m.kind == kind => None,
            _ => Some(Menu {
                kind,
                query: String::new(),
                row: 0,
            }),
        };
        cx.notify();
    }

    /// Entries of the open filter list, narrowed by the typed text.
    fn menu_items(&self) -> Vec<(String, MenuItem)> {
        let Some(menu) = &self.menu else {
            return Vec::new();
        };
        let q = menu.query.trim().to_lowercase();
        let keep = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);
        match menu.kind {
            FilterKind::Branch => std::iter::once(("All".to_owned(), MenuItem::AllBranches))
                .chain(
                    self.branches
                        .iter()
                        .filter(|b| keep(&b.name))
                        .map(|b| (b.name.clone(), MenuItem::Branch(b.name.clone()))),
                )
                .collect(),
            FilterKind::User => std::iter::once(("All".to_owned(), MenuItem::AllUsers))
                .chain(
                    self.authors
                        .iter()
                        .filter(|a| keep(a))
                        .map(|a| (a.clone(), MenuItem::User(a.clone()))),
                )
                .collect(),
            FilterKind::Date => DateRange::ALL
                .iter()
                .filter(|d| keep(d.label()))
                .map(|&d| (d.label().to_owned(), MenuItem::Date(d)))
                .collect(),
            FilterKind::Paths => {
                let typed = menu.query.trim();
                let mut items = Vec::new();
                if !typed.is_empty() {
                    items.push((format!("Add {typed}"), MenuItem::AddPath(typed.to_owned())));
                }
                items.extend(
                    self.filter
                        .paths
                        .iter()
                        .map(|p| (format!("Remove {p}"), MenuItem::RemovePath(p.clone()))),
                );
                if !self.filter.paths.is_empty() {
                    items.push(("Clear".to_owned(), MenuItem::ClearPaths));
                }
                items
            }
        }
    }

    /// Applies an entry of a filter list; the paths list stays open for more paths.
    fn choose_menu_item(&mut self, item: MenuItem, cx: &mut Context<Self>) {
        let keep_open = matches!(
            item,
            MenuItem::AddPath(_) | MenuItem::RemovePath(_) | MenuItem::ClearPaths
        );
        match item {
            MenuItem::AllBranches => {
                self.branch_filter = None;
                self.update_filter(|_| {}, cx);
            }
            MenuItem::Branch(b) => {
                if self.branch_filter.as_deref() != Some(b.as_str()) {
                    self.choose_branch(&b, cx);
                }
            }
            MenuItem::AllUsers => self.update_filter(|f| f.author = None, cx),
            MenuItem::User(a) => self.update_filter(|f| f.author = Some(a), cx),
            MenuItem::Date(d) => self.update_filter(|f| f.date = d, cx),
            MenuItem::AddPath(path) => self.update_filter(
                |f| {
                    if !f.paths.contains(&path) {
                        f.paths.push(path);
                    }
                },
                cx,
            ),
            MenuItem::RemovePath(path) => {
                self.update_filter(|f| f.paths.retain(|p| *p != path), cx)
            }
            MenuItem::ClearPaths => self.update_filter(|f| f.paths.clear(), cx),
        }
        match (&mut self.menu, keep_open) {
            (Some(m), true) => {
                m.query.clear();
                m.row = 0;
            }
            _ => self.menu = None,
        }
        cx.notify();
    }

    /// Keys of an open filter list.
    fn on_menu_key(&mut self, key: &str, text: Option<&str>, cx: &mut Context<Self>) -> bool {
        let items = self.menu_items();
        let count = items.len();
        if key == "enter" {
            let row = self.menu.as_ref().map_or(0, |m| m.row);
            if let Some((_, item)) = items.into_iter().nth(row) {
                self.choose_menu_item(item, cx);
            }
            return true;
        }
        let Some(menu) = &mut self.menu else {
            return false;
        };
        match key {
            "down" => menu.row = (menu.row + 1).min(count.saturating_sub(1)),
            "up" => menu.row = menu.row.saturating_sub(1),
            "escape" => self.menu = None,
            "backspace" => {
                menu.query.pop();
                menu.row = 0;
            }
            _ => match text {
                Some(text) => {
                    menu.query.push_str(text);
                    menu.row = 0;
                }
                None => return false,
            },
        }
        true
    }

    /// Opens or closes a folder, or chooses a branch, of the branches pane.
    fn activate_tree_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(item) = self.branch_rows().get(row).map(|r| r.item.clone()) else {
            return;
        };
        self.tree_row = row;
        match item {
            TreeItem::Folder(key) => {
                if !self.tree_closed.remove(&key) {
                    self.tree_closed.insert(key);
                }
            }
            TreeItem::Branch(name) => self.choose_branch(&name, cx),
        }
    }

    /// Keys of the branches pane (`SPEC-0086` v4 behaviour 9).
    fn on_tree_key(&mut self, key: &str, text: Option<&str>, cx: &mut Context<Self>) -> bool {
        let rows = self.branch_rows();
        let folder = |r: Option<&TreeRow>| match r.map(|r| (&r.item, r.open)) {
            Some((TreeItem::Folder(k), open)) => Some((k.clone(), open)),
            _ => None,
        };
        match key {
            "down" => self.tree_row = (self.tree_row + 1).min(rows.len().saturating_sub(1)),
            "up" => self.tree_row = self.tree_row.saturating_sub(1),
            "right" => {
                if let Some((k, false)) = folder(rows.get(self.tree_row)) {
                    self.tree_closed.remove(&k);
                }
            }
            "left" => {
                if let Some((k, true)) = folder(rows.get(self.tree_row)) {
                    self.tree_closed.insert(k);
                }
            }
            "enter" => self.activate_tree_row(self.tree_row, cx),
            // Space marks or unmarks the highlighted branch (v10 behaviour 19).
            "space" => self.toggle_row_favorite(self.tree_row, cx),
            "tab" => self.in_branches = false,
            "escape" if !self.tree_filter.is_empty() => self.tree_filter.clear(),
            "escape" => self.in_branches = false,
            "backspace" => {
                self.tree_filter.pop();
                self.tree_row = 0;
            }
            _ => match text {
                Some(text) => {
                    self.tree_filter.push_str(text);
                    self.tree_row = 0;
                }
                None => return false,
            },
        }
        true
    }

    /// Details of the selected commit, once read.
    pub fn details(&self) -> Option<&CommitDetails> {
        self.details.as_ref()
    }

    /// The filter text.
    pub fn filter(&self) -> &str {
        &self.filter.text
    }

    /// Selects the commit `id` (full or abbreviated) and reads its details; while the history is
    /// still read, once it is. A commit not among the listed ones is found through the filter
    /// (`SPEC-0277` behaviour 5).
    pub fn select_commit(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.commits.is_none() {
            self.wanted = Some(id.to_owned());
            return;
        }
        self.choose_commit(id);
        self.load_details(cx);
        cx.notify();
    }

    fn choose_commit(&mut self, id: &str) {
        let Some(commits) = &self.commits else {
            return;
        };
        let at = |shown: &[usize]| shown.iter().position(|&i| commits[i].id.starts_with(id));
        if let Some(row) = at(&self.shown) {
            self.selected = Some(row);
        } else {
            self.filter.text = id.to_owned();
            self.filter.regex = false;
            self.refresh_rows(None);
            let commits = self.commits.as_deref().unwrap_or_default();
            self.selected = self
                .shown
                .iter()
                .position(|&i| commits[i].id.starts_with(id));
        }
        self.in_files = false;
    }

    /// Full id of the selected commit.
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_commit().map(|c| c.id.as_str())
    }

    fn selected_commit(&self) -> Option<&CommitInfo> {
        let commits = self.commits.as_ref()?;
        self.shown.get(self.selected?).map(|&i| &commits[i])
    }

    /// Reads the selected commit's details in the background (behaviour 4).
    fn load_details(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_commit().map(|c| c.id.clone()) else {
            self.details = None;
            self.asked = None;
            return;
        };
        if self.asked.as_deref() == Some(id.as_str()) {
            return;
        }
        self.asked = Some(id.clone());
        self.file = 0;
        let root = self.root.clone();
        let remote = self.remote.clone();
        cx.spawn(async move |this, cx| {
            let wanted = id.clone();
            let read = cx
                .background_spawn(async move {
                    match remote {
                        // `git.show` (`SPEC-0318` v2 protocol detail 7).
                        Some(host) => host("git.show", serde_json::json!({ "id": id }))
                            .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                            .map_err(ferris_idea_vcs::GitError::Git),
                        None => Repo::discover(&root).and_then(|r| r.commit_details(&id)),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.asked.as_deref() != Some(wanted.as_str()) {
                    return;
                }
                match read {
                    Ok(d) => this.details = Some(d),
                    Err(e) => {
                        this.details = None;
                        this.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_filter(&mut self, filter: String, cx: &mut Context<Self>) {
        let previous = self.selected_commit().map(|c| c.id.clone());
        self.filter.text = filter;
        self.refresh_rows(previous.as_deref());
        self.load_details(cx);
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = (!self.shown.is_empty()).then(|| i.min(self.shown.len() - 1));
        self.in_files = false;
        self.in_branches = false;
        self.load_details(cx);
    }

    /// Opens the diff of the chosen file (behaviour 5).
    fn open_file(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.details else {
            return;
        };
        if let Some(change) = d.changes.get(self.file) {
            cx.emit(GitLogEvent::ShowDiff {
                commit: d.info.id.clone(),
                path: change.path.clone(),
            });
        }
    }

    /// Keys of behaviours 3–5.
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let plain = !(k.modifiers.control || k.modifiers.alt || k.modifiers.platform);
        // Typed text arrives through the platform's text input, not as the key behind it
        // (`SPEC-0004` v8 behaviour 17b); Space keeps marking a branch in the branches pane.
        let marks_branch = k.key == "space"
            && self.in_branches
            && self.tree_new.is_none()
            && self.menu.is_none()
            && self.commit_menu.is_none();
        if plain
            && !marks_branch
            && crate::committed_text::leave_to_text_input(k.key_char.as_deref(), false)
        {
            return;
        }
        let typed = k
            .key_char
            .as_deref()
            .filter(|t| !t.is_empty() && t.chars().all(|ch| !ch.is_control()))
            .map(str::to_owned);
        // The open commit menu takes every key (`SPEC-0086` v7 behaviour 13).
        if self.commit_menu.is_some() {
            self.on_commit_menu_key(k.key.as_str(), typed.as_deref().filter(|_| plain), cx);
            cx.stop_propagation();
            return;
        }
        if self.tree_new.is_some() && self.in_branches && plain {
            self.on_new_branch_key(k.key.as_str(), typed.as_deref(), cx);
            cx.stop_propagation();
            return;
        }
        // The first and the last commit, or changed file of the selected commit (`SPEC-0275`
        // v19 behaviour 47); the branch tree and the open menus keep their keys.
        if self.menu.is_none() && !self.in_branches {
            let files = self.details.as_ref().map_or(0, |d| d.changes.len());
            let rows = if self.in_files {
                files
            } else {
                self.shown.len()
            };
            if let Some(to) = crate::list_nav::edge(k, rows) {
                if self.in_files {
                    self.file = to;
                } else {
                    self.select(to, cx);
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        // ⇧F10 opens the menu of the selected commit; ⌥⇧⌘C copies its id; ⌘D shows its diff;
        // ← and → go to the child and the parent (v7 behaviour 13).
        if !self.in_files && !self.in_branches {
            let m = &k.modifiers;
            let only = |shift: bool, alt: bool, cmd: bool| {
                m.shift == shift && m.alt == alt && m.platform == cmd && !m.control
            };
            let action = match k.key.as_str() {
                "f10" if only(true, false, false) => {
                    if let Some(row) = self.selected {
                        self.open_commit_menu(row, None, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                "c" if only(true, true, true) => Some(CommitAction::CopyRevision),
                "d" if only(false, false, true) => Some(CommitAction::ShowDiff),
                "left" if only(false, false, false) => Some(CommitAction::GoToChild),
                "right" if only(false, false, false) => Some(CommitAction::GoToParent),
                _ => None,
            };
            if let Some(action) = action {
                match (action, self.selected_commit().map(|c| c.id.clone())) {
                    (CommitAction::GoToChild | CommitAction::GoToParent, Some(id)) => {
                        self.go_to_relative(&id, action == CommitAction::GoToChild, cx)
                    }
                    _ => self.selected_action(action, cx),
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        if plain && self.menu.is_some() {
            let text = k
                .key_char
                .as_deref()
                .filter(|t| !t.is_empty() && t.chars().all(|ch| !ch.is_control()));
            if self.on_menu_key(k.key.as_str(), text, cx) {
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        // ⌥R regular expression, ⌥C case (`SPEC-0086` v5 behaviour 3).
        if k.modifiers.alt && !k.modifiers.platform && !k.modifiers.control && !self.in_branches {
            match k.key.as_str() {
                "r" => {
                    self.update_filter(|f| f.regex = !f.regex, cx);
                    cx.stop_propagation();
                    return;
                }
                "c" => {
                    self.update_filter(|f| f.case = !f.case, cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        if k.modifiers.alt && k.key == "enter" && !self.in_files {
            if let Some(commit) = self.selected_commit() {
                cx.emit(GitLogEvent::CherryPick(commit.id.clone()));
            }
            cx.stop_propagation();
            return;
        }
        if k.modifiers.alt && k.key == "i" && !self.in_files {
            if let Some(commit) = self.selected_commit() {
                cx.emit(GitLogEvent::RebaseFrom(commit.id.clone()));
            }
            cx.stop_propagation();
            return;
        }
        if k.modifiers.platform && k.key == "d" && self.in_files {
            self.open_file(cx);
        } else if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        } else if self.in_branches {
            let text = k
                .key_char
                .as_deref()
                .filter(|t| !t.is_empty() && t.chars().all(|ch| !ch.is_control()));
            if !self.on_tree_key(k.key.as_str(), text, cx) {
                return;
            }
        } else {
            let files = self.details.as_ref().map_or(0, |d| d.changes.len());
            match k.key.as_str() {
                "down" if self.in_files => self.file = (self.file + 1).min(files.saturating_sub(1)),
                "up" if self.in_files => self.file = self.file.saturating_sub(1),
                "down" => self.select(self.selected.map_or(0, |s| s + 1), cx),
                "up" => self.select(self.selected.map_or(0, |s| s.saturating_sub(1)), cx),
                // Commits → files (when there are any) → branches → commits (v4 behaviour 9).
                "tab" if !self.in_files && files > 0 => self.in_files = true,
                "tab" => {
                    self.in_files = false;
                    self.in_branches = true;
                }
                "enter" if self.in_files => self.open_file(cx),
                "escape" if self.in_files => self.in_files = false,
                "escape" if !self.filter.text.is_empty() => self.set_filter(String::new(), cx),
                "escape" if self.branch_filter.is_some() => self.clear_branch_filter(cx),
                "escape" => cx.emit(GitLogEvent::Close),
                "backspace" => {
                    let mut f = self.filter.text.clone();
                    f.pop();
                    self.set_filter(f, cx);
                }
                _ => match k
                    .key_char
                    .as_deref()
                    .filter(|t| !t.is_empty() && t.chars().all(|ch| !ch.is_control()))
                {
                    Some(text) => {
                        let f = format!("{}{text}", self.filter.text);
                        self.set_filter(f, cx);
                    }
                    None => return,
                },
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Takes text the platform's text input committed (`SPEC-0004` v8 behaviour 17b): it goes
    /// where a typed key's text went — the commit menu's name field, a new branch's name, the
    /// open menu's search, the branches pane's filter, else the commit filter.
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        let text = Some(committed);
        if self.commit_menu.is_some() {
            self.on_commit_menu_key("", text, cx);
        } else if self.tree_new.is_some() && self.in_branches {
            self.on_new_branch_key("", text, cx);
        } else if self.menu.is_some() {
            self.on_menu_key("", text, cx);
        } else if self.in_branches {
            self.on_tree_key("", text, cx);
        } else {
            let f = format!("{}{committed}", self.filter.text);
            self.set_filter(f, cx);
        }
        cx.notify();
    }

    /// The typed filter of the branches pane.
    pub fn branch_tree_filter(&self) -> &str {
        &self.tree_filter
    }

    fn commit_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let Some(commits) = &self.commits else {
            return div()
                .p_3()
                .text_color(c(p.ui_muted))
                .child("Loading…")
                .into_any_element();
        };
        if self.shown.is_empty() {
            let text = if self.filtering_paths() {
                "Filtering…".to_owned()
            } else if let Some(e) = &self.text_error {
                format!("Invalid regular expression: {e}")
            } else if self.filter.active() || self.branch_filter.is_some() {
                "No commits match the filters".to_owned()
            } else {
                self.error.clone().unwrap_or_else(|| "No commits".into())
            };
            return div()
                .p_3()
                .text_color(c(p.ui_muted))
                .child(text)
                .into_any_element();
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let (zone, style) = (self.zone.clone(), self.style);
        let on_head = self.on_head.clone();
        let head_tint = {
            let mut t = c(p.tree_selection_inactive);
            t.a *= 0.45;
            t
        };
        let rows: Vec<Row> = self
            .shown
            .iter()
            .zip(&self.graph)
            .map(|(&i, graph)| {
                let commit = commits[i].clone();
                // The current branch's label comes first, local and remote branch of the same
                // name on one commit are one label (`SPEC-0086` v3 behaviour 2, v7 behaviour 14).
                let labels = self.labels_for(&commit.id);
                (commit, labels, graph.clone())
            })
            .collect();
        // Author and date are as wide as their longest text, up to a limit; the subject takes the
        // rest (`SPEC-0086` v3 behaviour 2).
        let longest = |f: &dyn Fn(&Row) -> usize| rows.iter().map(f).max().unwrap_or(0) as f32;
        let author_w = (longest(&|r| author_text(&r.0).chars().count()) * CHAR + 12.).min(180.);
        let this = cx.entity().downgrade();
        let date_w =
            longest(&|r| date_text(r.0.time, now, &zone, &style).chars().count()) * CHAR + 12.;
        let lanes = self.graph.iter().map(GraphRow::width).max().unwrap_or(1);
        let graph_w = lanes.min(MAX_LANES) as f32 * LANE + 4.;
        let colors = lane_colors(p);
        let (selected, active) = (self.selected, !self.in_files);
        uniform_list(
            "git-log",
            rows.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let (commit, labels, graph) = &rows[i];
                        let weak = this.clone();
                        let row = div()
                            .id(("git-log-row", i))
                            .w_full()
                            .h(px(ROW))
                            .flex()
                            .items_center()
                            .gap_2()
                            .pr_2()
                            .rounded(px(4.))
                            .when(selected == Some(i), |d| {
                                d.bg(c(if active {
                                    p.tree_selection
                                } else {
                                    p.tree_selection_inactive
                                }))
                            })
                            .when(selected != Some(i) && on_head.contains(&commit.id), |d| {
                                d.bg(head_tint)
                            })
                            .when(selected != Some(i), |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(graph_cell(graph.clone(), graph_w, colors.clone()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(commit.summary.clone()),
                            )
                            // The labels stand right-aligned before the author, as in the
                            // reference (`SPEC-0086` v7 behaviour 14).
                            .when(!labels.is_empty(), |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .max_w(px(320.))
                                        .overflow_hidden()
                                        .flex()
                                        .items_center()
                                        .justify_end()
                                        .gap_1()
                                        .children(labels.iter().map(|l| label_chip(l, p))),
                                )
                            })
                            .child(
                                div()
                                    .w(px(author_w))
                                    .flex_none()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_color(c(p.ui_muted))
                                    .child(author_text(commit)),
                            )
                            .child(
                                div()
                                    .w(px(date_w))
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .text_color(c(p.ui_muted))
                                    .child(date_text(commit.time, now, &zone, &style)),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.select(i, cx);
                                cx.notify();
                            }));
                        // A secondary click opens the commit menu (`SPEC-0086` v7 behaviour 13).
                        crate::mouse::on_secondary_click(row, move |e, _, app| {
                            let _ = weak
                                .update(app, |v, cx| v.open_commit_menu(i, Some(e.position), cx));
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .w_full()
        .px_1()
        .flex_1()
        .into_any_element()
    }

    fn details_pane(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let Some(d) = &self.details else {
            // Nothing is selected until the user chooses a commit (`SPEC-0086` v3 behaviour 2).
            let hint = if self.selected.is_none() && !self.shown.is_empty() {
                "Select a commit to see its changes and details"
            } else {
                ""
            };
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_3()
                .text_color(c(p.ui_muted))
                .child(hint)
                .into_any_element();
        };
        let date = date_text(
            d.info.time,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |t| t.as_secs() as i64),
            &self.zone,
            &self.style,
        );
        let parents = d
            .parents
            .iter()
            .map(|id| id[..7.min(id.len())].to_owned())
            .collect::<Vec<_>>()
            .join(", ");
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("git-log-files")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_1()
                    .children(d.changes.iter().enumerate().map(|(i, change)| {
                        let color = p
                            .scheme_color(status_color_key(change.kind, false))
                            .unwrap_or(p.text);
                        let current = self.in_files && i == self.file;
                        div()
                            .id(("git-log-file", i))
                            .h(px(22.))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(rgb(color))
                            .when(current, |e| e.bg(c(p.tree_selection)))
                            .when(!current, |e| e.hover(|s| s.bg(c(p.tree_hover))))
                            .child(change.path.clone())
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                this.in_files = true;
                                this.in_branches = false;
                                this.file = i;
                                if e.click_count() > 1 {
                                    this.open_file(cx);
                                }
                                cx.notify();
                            }))
                    })),
            )
            .child(div().h(px(1.)).flex_none().bg(c(p.separator)))
            .child(
                div()
                    .id("git-log-details")
                    .h(px(120.))
                    .flex_none()
                    .overflow_y_scroll()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .whitespace_normal()
                            .child(d.message.trim_end().to_owned()),
                    )
                    .child(div().text_color(c(p.ui_muted)).child(format!(
                        "{} {} <{}> on {date}",
                        d.info.short_id, d.info.author, d.info.email
                    )))
                    // Who committed it, when not the author (`SPEC-0086` v7 behaviour 15).
                    .when_some(d.info.committer.clone(), |e, by| {
                        e.child(
                            div()
                                .text_color(c(p.ui_muted))
                                .child(format!("committed by {by}")),
                        )
                    })
                    .when(!parents.is_empty(), |e| {
                        e.child(
                            div()
                                .text_color(c(p.ui_dim))
                                .child(format!("Parents: {parents}")),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl GitLogView {
    /// The branches pane: filter field and tree (`SPEC-0086` v4 behaviour 9).
    fn branches_pane(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let rows = self.branch_rows();
        let field = match &self.tree_new {
            // The new-branch field of the action bar (`SPEC-0086` v7 behaviour 16).
            Some(name) if name.is_empty() => div().text_color(c(p.ui_dim)).child(format!(
                "New branch from {}",
                self.highlighted_branch().unwrap_or_else(|| "HEAD".into())
            )),
            Some(name) => div().child(name.clone()),
            None if self.tree_filter.is_empty() => {
                div().text_color(c(p.ui_dim)).child("Filter branches")
            }
            None => div().child(self.tree_filter.clone()),
        };
        let tracking = self.tracking.clone();
        let (active, highlighted) = (self.in_branches, self.tree_row);
        let chosen = self.branch_filter.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .m_1()
                    .px_2()
                    .py(px(2.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c(if active { p.focus } else { p.separator }))
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(icon(if self.tree_new.is_some() {
                        "general/add"
                    } else {
                        "general/search"
                    }))
                    .child(field),
            )
            .child(
                div()
                    .id("git-log-branches")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_1()
                    .children(rows.into_iter().enumerate().map(|(i, row)| {
                        let is_chosen = matches!(&row.item, TreeItem::Branch(b) if chosen.as_deref() == Some(b.as_str()));
                        let lit = active && i == highlighted;
                        let folder = matches!(row.item, TreeItem::Folder(_));
                        // ↙ incoming and ↗ outgoing commits of a local branch against its
                        // upstream, as of the last fetch (`SPEC-0086` v7 behaviour 16).
                        // The "HEAD (Current Branch)" row has none: its branch row shows them, and the
                        // reference leaves them out there (they cut the row's name).
                        let counts = match &row.item {
                            TreeItem::Branch(_) if row.text == crate::git_branch_tree::CURRENT_ROW => None,
                            TreeItem::Branch(b) => tracking.get(b).copied(),
                            TreeItem::Folder(_) => None,
                        }
                        .map(|(ahead, behind)| tracking_text(ahead, behind))
                        .filter(|t| !t.is_empty());
                        div()
                            .id(("git-log-branch", i))
                            .h(px(22.))
                            .pl(px(4. + 14. * row.depth as f32))
                            .pr_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(lit, |d| d.bg(c(p.tree_selection)))
                            .when(!lit && is_chosen, |d| d.bg(c(p.tree_selection_inactive)))
                            .when(!lit && !is_chosen, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .when(folder, |d| {
                                d.child(icon(if row.open {
                                    "general/chevronDown"
                                } else {
                                    "general/chevronRight"
                                }))
                            })
                            // The star of a branch: filled on a favourite, an outline on the
                            // highlighted or hovered row (v10 behaviour 19).
                            .when(!folder, |d| {
                                let star = row.favorite;
                                let group = format!("git-log-branch-row-{i}");
                                let head = row.item == TreeItem::Branch("HEAD".into());
                                d.group(group.clone()).child(
                                    div()
                                        .id(("git-log-branch-star", i))
                                        .w(px(16.))
                                        .flex_none()
                                        .when(!head && star, |d| d.child(icon("ferris-idea/star")))
                                        .when(!head && !star, |d| {
                                            d.when(!lit, |d| {
                                                d.opacity(0.)
                                                    .group_hover(group, |s| s.opacity(1.))
                                            })
                                            .child(icon("ferris-idea/starOutline"))
                                        })
                                        .on_click(cx.listener(
                                            move |this, _: &gpui::ClickEvent, _, cx| {
                                                cx.stop_propagation();
                                                this.in_branches = true;
                                                this.in_files = false;
                                                this.tree_row = i;
                                                this.toggle_row_favorite(i, cx);
                                                cx.notify();
                                            },
                                        )),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .when(row.current, |d| {
                                        d.font_weight(gpui::FontWeight::SEMIBOLD)
                                    })
                                    .child(row.text.clone()),
                            )
                            .children(counts.map(|t| {
                                div().flex_none().text_color(c(p.ui_muted)).child(t)
                            }))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.in_branches = true;
                                this.in_files = false;
                                this.activate_tree_row(i, cx);
                                cx.notify();
                            }))
                    })),
            )
            .into_any_element()
    }
}

/// The filter's name in lower case, from its control name (`git-log-filter-user` → `user`).
fn name_lower(name: &str) -> &str {
    name.rsplit('-').next().unwrap_or(name)
}

impl GitLogView {
    /// A switch at the end of the text field (`SPEC-0086` v5 behaviour 3).
    fn toggle(
        &self,
        control: &'static ferris_idea_ui::tooltip::Control,
        text: &'static str,
        on: bool,
        flip: fn(&mut LogFilter),
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let p = Palette::current();
        div()
            .id(control.name)
            .tip(control)
            .flex_none()
            .px_1()
            .rounded(px(3.))
            .text_color(c(if on { p.accent_text } else { p.ui_muted }))
            .when(on, |d| d.bg(c(p.accent)))
            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
            .child(text)
            .on_click(
                cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.update_filter(flip, cx)),
            )
            .into_any_element()
    }

    /// A filter button naming its choice, with its list under it when open (`SPEC-0086` v5
    /// behaviour 10).
    fn filter_button(&self, kind: FilterKind, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let (name, choice) = match kind {
            FilterKind::Branch => ("Branch", self.branch_filter.clone()),
            FilterKind::User => ("User", self.filter.author.clone()),
            FilterKind::Date => (
                "Date",
                (self.filter.date != DateRange::All).then(|| self.filter.date.label().to_owned()),
            ),
            FilterKind::Paths => (
                "Paths",
                match self.filter.paths.as_slice() {
                    [] => None,
                    [one] => Some(one.clone()),
                    many => Some(format!("{} paths", many.len())),
                },
            ),
        };
        let set = choice.is_some();
        let label = match &choice {
            Some(c) => format!("{name}: {c}"),
            None => name.to_owned(),
        };
        let open = self.menu.as_ref().is_some_and(|m| m.kind == kind);
        let menu = open.then(|| self.menu_list(cx));
        let name = match kind {
            FilterKind::Branch => "git-log-filter-branch",
            FilterKind::User => "git-log-filter-user",
            FilterKind::Date => "git-log-filter-date",
            FilterKind::Paths => "git-log-filter-paths",
        };
        // The whole choice, also when the button cuts it short (`SPEC-0294` behaviour 1).
        let tip = match &choice {
            Some(_) => ferris_idea_ui::tooltip::Tip::plain(label.clone())
                .with_detail("Click to change; × removes this filter"),
            None => ferris_idea_ui::tooltip::Tip::plain(format!("Filter by {}", name_lower(name))),
        };
        div()
            .id(name)
            .tip_text(name, tip)
            .flex_none()
            .px_2()
            .py(px(2.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .gap_1()
            .whitespace_nowrap()
            .max_w(px(220.))
            .overflow_hidden()
            .when(set || open, |d| d.bg(c(p.tree_selection_inactive)))
            .when(!set && !open, |d| d.hover(|s| s.bg(c(p.tree_hover))))
            .text_color(c(if set { p.ui_text } else { p.ui_muted }))
            .child(div().text_ellipsis().overflow_hidden().child(label))
            .when(!set, |d| d.child(icon("general/chevronDown")))
            .when(set, |d| {
                // Removes this filter alone (the branch: `SPEC-0086` v4 behaviour 9).
                d.child(
                    div()
                        .id(("git-log-filter-remove", kind as usize))
                        .tip(declared!(GIT_LOG_TOOLBAR, "git-log-filter-remove"))
                        .rounded(px(3.))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(icon("general/closeSmall"))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            cx.stop_propagation();
                            match kind {
                                FilterKind::Branch => this.clear_branch_filter(cx),
                                FilterKind::User => this.update_filter(|f| f.author = None, cx),
                                FilterKind::Date => {
                                    this.update_filter(|f| f.date = DateRange::All, cx)
                                }
                                FilterKind::Paths => this.update_filter(|f| f.paths.clear(), cx),
                            }
                        })),
                )
            })
            .on_click(
                cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.toggle_menu(kind, cx)),
            )
            .children(menu)
            .into_any_element()
    }

    /// The open filter list, under its button.
    fn menu_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let Some(menu) = &self.menu else {
            return div().into_any_element();
        };
        let hint = match menu.kind {
            FilterKind::Paths => "Type a path, Enter adds it",
            _ => "Type to narrow",
        };
        let query = if menu.query.is_empty() {
            div().text_color(c(p.ui_dim)).child(hint)
        } else {
            div().child(menu.query.clone())
        };
        let row = menu.row;
        let items = self.menu_items();
        gpui::deferred(
            gpui::anchored()
                .anchor(gpui::Corner::TopLeft)
                .offset(point(px(0.), px(24.)))
                .snap_to_window()
                .child(
                    crate::metrics::menu_frame()
                        .occlude()
                        .min_w(px(200.))
                        .max_h(px(320.))
                        .bg(c(p.island))
                        .flex()
                        .flex_col()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .border_b_1()
                                .border_color(c(p.separator))
                                .child(query),
                        )
                        .children(items.into_iter().enumerate().map(|(i, (text, item))| {
                            div()
                                .id(("git-log-filter-item", i))
                                .px_2()
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .whitespace_nowrap()
                                .when(i == row, |d| d.bg(c(p.tree_selection)))
                                .when(i != row, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                .child(text)
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.choose_menu_item(item.clone(), cx)
                                }))
                        })),
                ),
        )
        .into_any_element()
    }
}

/// The commit menu, the labels and the branches pane's counters and actions (`SPEC-0086` v7).
impl GitLogView {
    fn names_of(&self, remote: bool) -> HashSet<String> {
        self.branches
            .iter()
            .filter(|b| b.remote == remote)
            .map(|b| b.name.clone())
            .collect()
    }

    fn labels_for(&self, id: &str) -> Vec<Label> {
        let names = self.labels.get(id).cloned().unwrap_or_default();
        row_labels(
            &names,
            self.current.as_deref(),
            &self.names_of(false),
            &self.names_of(true),
        )
    }

    /// The labels of listed row `row` as the list shows them (`SPEC-0086` v7 behaviour 14).
    pub fn labels_of_row(&self, row: usize) -> Vec<Label> {
        match (&self.commits, self.shown.get(row)) {
            (Some(commits), Some(&i)) => self.labels_for(&commits[i].id),
            _ => Vec::new(),
        }
    }

    /// The author column of listed row `row`, `*` included (`SPEC-0086` v7 behaviour 15).
    pub fn author_of_row(&self, row: usize) -> Option<String> {
        let commits = self.commits.as_ref()?;
        self.shown.get(row).map(|&i| author_text(&commits[i]))
    }

    /// Commits ahead of and behind its upstream of local branch `branch` (v7 behaviour 16).
    pub fn tracking_of(&self, branch: &str) -> Option<(usize, usize)> {
        self.tracking.get(branch).copied()
    }

    fn head_tip(&self) -> Option<&str> {
        tip_of(&self.labels, "HEAD").or_else(|| {
            self.current
                .as_deref()
                .and_then(|b| tip_of(&self.labels, b))
        })
    }

    fn history_facts(&self) -> Option<HistoryFacts> {
        let commits = self.commits.as_ref()?;
        let remotes = self.names_of(true);
        Some(HistoryFacts::new(
            commits,
            &self.on_head,
            self.head_tip(),
            &remote_tips(&self.labels, &remotes),
            self.detached,
        ))
    }

    /// Selects row `row` and opens its commit menu at window position `at`, or under the list's
    /// top when opened by key (`SPEC-0086` v7 behaviour 13).
    pub fn open_commit_menu(
        &mut self,
        row: usize,
        at: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if row >= self.shown.len() {
            return;
        }
        self.menu = None;
        self.select(row, cx);
        let Some(facts) = self.history_facts() else {
            return;
        };
        let Some(commits) = &self.commits else {
            return;
        };
        let commit = &commits[self.shown[row]];
        let entries = commit_actions(&facts.facts(commits, commit));
        let names = self.labels.get(&commit.id).cloned().unwrap_or_default();
        let branches = menu_branches(
            &names,
            self.current.as_deref(),
            &self.names_of(false),
            &self.names_of(true),
        );
        let locals = self.names_of(false);
        let mut checkout: Vec<String> = names
            .iter()
            .filter(|n| locals.contains(*n) && Some(n.as_str()) != self.current.as_deref())
            .cloned()
            .collect();
        checkout.sort();
        let rows = menu_rows(&entries, &branches);
        self.commit_menu = Some(CommitMenu {
            commit: commit.id.clone(),
            entries,
            rows,
            checkout,
            cursor: None,
            page: MenuPage::Main,
            at,
            note: None,
        });
        cx.notify();
    }

    /// The open commit menu's entries: label, whether it can be chosen, and why not.
    pub fn commit_menu_entries(&self) -> Option<Vec<(String, bool, Option<String>)>> {
        let menu = self.commit_menu.as_ref()?;
        Some(
            menu.rows
                .iter()
                .map(|r| match r {
                    MenuRow::Entry(i) => {
                        let e = &menu.entries[*i];
                        (row_label(e, &menu.checkout), e.enabled, e.reason.clone())
                    }
                    MenuRow::Branch(name, _) => (format!("Branch '{name}'"), true, None),
                })
                .collect(),
        )
    }

    /// Opens the page of branch `name` of the open commit menu, as a click on its row does (v8).
    pub fn choose_menu_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.commit_menu else {
            return;
        };
        let Some(pos) = menu
            .rows
            .iter()
            .position(|r| matches!(r, MenuRow::Branch(n, _) if n == name))
        else {
            return;
        };
        let MenuRow::Branch(name, remote) = menu.rows[pos].clone() else {
            return;
        };
        menu.cursor = Some(pos);
        menu.note = None;
        menu.page = MenuPage::Branch {
            name,
            remote,
            cursor: 0,
        };
        cx.notify();
    }

    /// The entries of the open branch page or Checkout page (v8).
    pub fn commit_menu_page_items(&self) -> Vec<String> {
        let Some(menu) = &self.commit_menu else {
            return Vec::new();
        };
        match &menu.page {
            MenuPage::Checkout(_) => checkout_items(&menu.checkout, &menu.commit),
            MenuPage::Branch { name, remote, .. } => page_actions(
                name,
                *remote,
                self.current.as_deref(),
                self.favorites.is_favorite(name, *remote),
            )
            .into_iter()
            .map(|(_, l)| l)
            .collect(),
            _ => Vec::new(),
        }
    }

    /// Runs entry `i` of the open Checkout page or branch page (v8).
    pub fn choose_page_item(&mut self, i: usize, cx: &mut Context<Self>) {
        let current = self.current.clone().unwrap_or_else(|| "HEAD".into());
        let Some(menu) = &mut self.commit_menu else {
            return;
        };
        match menu.page.clone() {
            MenuPage::Checkout(_) => {
                let commit = menu.commit.clone();
                let branch = menu.checkout.get(i).cloned();
                if i > menu.checkout.len() {
                    return;
                }
                self.commit_menu = None;
                match branch {
                    Some(name) => cx.emit(GitLogEvent::CheckoutBranch(name)),
                    None => cx.emit(GitLogEvent::Commit {
                        commit,
                        action: CommitAction::Checkout,
                        mode: None,
                        name: None,
                    }),
                }
            }
            MenuPage::Branch { name, remote, .. } => {
                let favorite = self.favorites.is_favorite(&name, remote);
                let Some((action, _)) = page_actions(&name, remote, Some(&current), favorite)
                    .into_iter()
                    .nth(i)
                else {
                    return;
                };
                let event = match action {
                    PopupBranchAction::NewBranchFrom | PopupBranchAction::Rename => {
                        let kind = if action == PopupBranchAction::Rename {
                            NameKind::Rename(name.clone())
                        } else {
                            NameKind::BranchFrom(name.clone())
                        };
                        let text = if action == PopupBranchAction::Rename {
                            name
                        } else {
                            String::new()
                        };
                        menu.page = MenuPage::Name {
                            kind,
                            text,
                            error: None,
                        };
                        cx.notify();
                        return;
                    }
                    PopupBranchAction::Checkout => BranchesEvent::Checkout(name, false),
                    PopupBranchAction::CheckoutAndRebase => {
                        BranchesEvent::CheckoutAndRebase(name, current)
                    }
                    PopupBranchAction::RebaseOnto => BranchesEvent::Rebase(name),
                    PopupBranchAction::MergeInto => BranchesEvent::Merge(name),
                    PopupBranchAction::CompareWith => BranchesEvent::CompareWith(name),
                    PopupBranchAction::DiffWorkingTree => BranchesEvent::DiffWorkingTree(name),
                    PopupBranchAction::Push => BranchesEvent::PushBranch(name),
                    PopupBranchAction::Delete => BranchesEvent::Delete(name),
                    PopupBranchAction::Favorite => {
                        self.commit_menu = None;
                        self.set_favorite(&name, remote, !favorite, cx);
                        return;
                    }
                };
                self.commit_menu = None;
                cx.emit(GitLogEvent::Branch(event));
            }
            _ => {}
        }
        cx.notify();
    }

    /// What the open commit menu shows.
    pub fn commit_menu_page(&self) -> Option<&MenuPage> {
        self.commit_menu.as_ref().map(|m| &m.page)
    }

    /// The reason shown after a disabled entry was chosen.
    pub fn commit_menu_note(&self) -> Option<&str> {
        self.commit_menu.as_ref()?.note.as_deref()
    }

    /// Closes the commit menu.
    pub fn close_commit_menu(&mut self, cx: &mut Context<Self>) {
        self.commit_menu = None;
        cx.notify();
    }

    /// Chooses `action` in the open commit menu, as a click does: a disabled entry only shows its
    /// reason; Reset asks for the mode, New Branch and New Tag for the name.
    pub fn choose_commit_action(&mut self, action: CommitAction, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.commit_menu else {
            return;
        };
        let Some(pos) = menu.entries.iter().position(|e| e.action == action) else {
            return;
        };
        menu.cursor = menu.rows.iter().position(|r| *r == MenuRow::Entry(pos));
        if !menu.entries[pos].enabled {
            menu.note = menu.entries[pos].reason.clone();
            cx.notify();
            return;
        }
        menu.note = None;
        let commit = menu.commit.clone();
        match action {
            CommitAction::Reset => menu.page = MenuPage::Reset(0),
            // Local branches at the commit: a page to choose one or the revision (v8).
            CommitAction::Checkout if !menu.checkout.is_empty() => {
                menu.page = MenuPage::Checkout(0)
            }
            CommitAction::NewBranch | CommitAction::NewTag => {
                menu.page = MenuPage::Name {
                    kind: if action == CommitAction::NewTag {
                        NameKind::Tag
                    } else {
                        NameKind::Branch
                    },
                    text: String::new(),
                    error: None,
                }
            }
            CommitAction::GoToChild | CommitAction::GoToParent => {
                self.commit_menu = None;
                self.go_to_relative(&commit, action == CommitAction::GoToChild, cx);
            }
            CommitAction::CherryPick => {
                self.commit_menu = None;
                cx.emit(GitLogEvent::CherryPick(commit));
            }
            CommitAction::RebaseFromHere => {
                self.commit_menu = None;
                cx.emit(GitLogEvent::RebaseFrom(commit));
            }
            _ => {
                self.commit_menu = None;
                cx.emit(GitLogEvent::Commit {
                    commit,
                    action,
                    mode: None,
                    name: None,
                });
            }
        }
        cx.notify();
    }

    /// Resets the current branch to the menu's commit in `mode`.
    pub fn choose_reset_mode(&mut self, mode: ResetMode, cx: &mut Context<Self>) {
        if let Some(menu) = self.commit_menu.take() {
            cx.emit(GitLogEvent::Commit {
                commit: menu.commit,
                action: CommitAction::Reset,
                mode: Some(mode),
                name: None,
            });
        }
        cx.notify();
    }

    /// Sets the name typed on the menu's name page.
    pub fn set_menu_name(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(CommitMenu {
            page: MenuPage::Name { text, error, .. },
            ..
        }) = &mut self.commit_menu
        {
            *text = name.to_owned();
            *error = None;
        }
        cx.notify();
    }

    /// Creates the branch or tag named on the name page, unless the name is refused.
    pub fn confirm_menu_name(&mut self, cx: &mut Context<Self>) {
        let branches: HashSet<String> = self.names_of(false);
        let all: HashSet<&String> = self.labels.values().flatten().collect();
        let Some(menu) = &mut self.commit_menu else {
            return;
        };
        let MenuPage::Name {
            kind: purpose,
            text,
            error,
        } = &mut menu.page
        else {
            return;
        };
        let name = text.trim().to_owned();
        let kind = purpose.noun();
        let exists = if *purpose == NameKind::Tag {
            all.contains(&name) && !branches.contains(&name)
        } else {
            branches.contains(&name)
        };
        if !ferris_idea_vcs::valid_branch_name(&name) {
            *error = Some(format!("\"{name}\" is not a valid {kind} name"));
        } else if exists {
            *error = Some(format!("the {kind} {name} already exists"));
        } else {
            let commit = menu.commit.clone();
            let purpose = purpose.clone();
            self.commit_menu = None;
            match purpose {
                NameKind::BranchFrom(start) => {
                    cx.emit(GitLogEvent::Branch(BranchesEvent::CreateFrom(name, start)))
                }
                NameKind::Rename(old) => {
                    cx.emit(GitLogEvent::Branch(BranchesEvent::Rename(old, name)))
                }
                NameKind::Branch | NameKind::Tag => cx.emit(GitLogEvent::Commit {
                    commit,
                    action: if purpose == NameKind::Tag {
                        CommitAction::NewTag
                    } else {
                        CommitAction::NewBranch
                    },
                    mode: None,
                    name: Some(name),
                }),
            }
        }
        cx.notify();
    }

    /// Selects the child (the newest listed one) or the first parent of commit `id`.
    fn go_to_relative(&mut self, id: &str, child: bool, cx: &mut Context<Self>) {
        let target = if child {
            self.history_facts()
                .and_then(|h| h.children(id).first().cloned())
        } else {
            self.commits
                .as_ref()
                .and_then(|cs| cs.iter().find(|c| c.id == id))
                .and_then(|c| c.parents.first().cloned())
        };
        if let Some(t) = target {
            self.select_commit(&t, cx);
        }
    }

    /// Keys of the open commit menu.
    fn on_commit_menu_key(&mut self, key: &str, text: Option<&str>, cx: &mut Context<Self>) {
        /// What a key leads to once the menu is no longer borrowed.
        enum Then {
            Nothing,
            Close,
            Choose(CommitAction),
            Branch(String),
            Item(usize),
            Reset(ResetMode),
            Confirm,
        }
        let page_len = self.commit_menu_page_items().len().max(1);
        let Some(menu) = &mut self.commit_menu else {
            return;
        };
        let count = menu.rows.len();
        let mut then = Then::Nothing;
        match &mut menu.page {
            MenuPage::Checkout(i) | MenuPage::Branch { cursor: i, .. } => match key {
                "down" => *i = (*i + 1) % page_len,
                "up" => *i = (*i + page_len - 1) % page_len,
                "enter" | "right" => then = Then::Item(*i),
                "escape" | "left" => menu.page = MenuPage::Main,
                _ => {}
            },
            MenuPage::Main => match key {
                "down" => {
                    menu.cursor = Some(menu.cursor.map_or(0, |c| (c + 1) % count));
                    menu.note = None;
                }
                "up" => {
                    menu.cursor = Some(menu.cursor.map_or(count - 1, |c| (c + count - 1) % count));
                    menu.note = None;
                }
                "enter" | "right" => match menu.cursor.and_then(|c| menu.rows.get(c)) {
                    Some(MenuRow::Entry(i)) => then = Then::Choose(menu.entries[*i].action),
                    Some(MenuRow::Branch(name, _)) => then = Then::Branch(name.clone()),
                    None => {}
                },
                "escape" => then = Then::Close,
                _ => {}
            },
            MenuPage::Reset(i) => match key {
                "down" => *i = (*i + 1) % ResetMode::ALL.len(),
                "up" => *i = (*i + ResetMode::ALL.len() - 1) % ResetMode::ALL.len(),
                "enter" => then = Then::Reset(ResetMode::ALL[*i]),
                "escape" | "left" => menu.page = MenuPage::Main,
                _ => {}
            },
            MenuPage::Name {
                text: name, error, ..
            } => match key {
                "enter" => then = Then::Confirm,
                "escape" => menu.page = MenuPage::Main,
                "backspace" => {
                    name.pop();
                    *error = None;
                }
                _ => {
                    if let Some(t) = text {
                        name.push_str(t);
                        *error = None;
                    }
                }
            },
        }
        match then {
            Then::Nothing => {}
            Then::Close => self.commit_menu = None,
            Then::Choose(action) => self.choose_commit_action(action, cx),
            Then::Branch(name) => self.choose_menu_branch(&name, cx),
            Then::Item(i) => self.choose_page_item(i, cx),
            Then::Reset(mode) => self.choose_reset_mode(mode, cx),
            Then::Confirm => self.confirm_menu_name(cx),
        }
        cx.notify();
    }

    /// Whether the keys go to the commit list (not to the files or the branches pane).
    pub fn in_commit_list(&self) -> bool {
        !self.in_files && !self.in_branches
    }

    /// Chooses `action` of the selected commit through its menu, so a disabled entry stays
    /// disabled and shows its reason there (`Git.Reword.Commit`, `Git.New.Branch.In.Log`; v11
    /// behaviour 20). False without a selected commit.
    pub fn run_commit_entry(&mut self, action: CommitAction, cx: &mut Context<Self>) -> bool {
        let Some(row) = self.selected else {
            return false;
        };
        self.open_commit_menu(row, None, cx);
        if self.commit_menu.is_none() {
            return false;
        }
        self.choose_commit_action(action, cx);
        true
    }

    /// Emits the action of the selected commit for a shortcut of the list (⌘D, ⌥⇧⌘C).
    fn selected_action(&mut self, action: CommitAction, cx: &mut Context<Self>) {
        if let Some(commit) = self.selected_commit().map(|c| c.id.clone()) {
            cx.emit(GitLogEvent::Commit {
                commit,
                action,
                mode: None,
                name: None,
            });
        }
    }

    /// The open commit menu.
    fn commit_menu_view(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let menu = self.commit_menu.as_ref()?;
        let p = Palette::current();
        let short = menu.commit[..7.min(menu.commit.len())].to_owned();
        let header = |text: String| {
            div()
                .px_2()
                .py_1()
                .text_color(c(p.ui_muted))
                .whitespace_nowrap()
                .child(text)
        };
        let mut body: Vec<gpui::AnyElement> = Vec::new();
        match &menu.page {
            MenuPage::Main => {
                let mut group = None;
                for (i, row) in menu.rows.iter().enumerate() {
                    // Branch pages sit in the group of New Branch… (v8).
                    let g = match row {
                        MenuRow::Entry(e) => menu.entries[*e].action.group(),
                        MenuRow::Branch(..) => CommitAction::NewBranch.group(),
                    };
                    if group.is_some_and(|last| last != g) {
                        body.push(div().my_1().h(px(1.)).bg(c(p.separator)).into_any_element());
                    }
                    group = Some(g);
                    let item = crate::metrics::menu_item(
                        ("git-log-commit-item", i),
                        menu.cursor == Some(i),
                    )
                    .justify_between()
                    .gap_4()
                    .whitespace_nowrap();
                    let item = match row {
                        MenuRow::Entry(e) => {
                            let e = &menu.entries[*e];
                            let action = e.action;
                            let page =
                                action == CommitAction::Checkout && !menu.checkout.is_empty();
                            item.when(!e.enabled, |d| d.text_color(c(p.ui_dim)))
                                .child(row_label(e, &menu.checkout))
                                .children(
                                    e.shortcut.map(|s| div().text_color(c(p.ui_dim)).child(s)),
                                )
                                .when(page, |d| d.child(div().text_color(c(p.ui_dim)).child("›")))
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.choose_commit_action(action, cx)
                                }))
                        }
                        MenuRow::Branch(name, _) => {
                            let name = name.clone();
                            item.child(format!("Branch '{name}'"))
                                .child(div().text_color(c(p.ui_dim)).child("›"))
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.choose_menu_branch(&name, cx)
                                }))
                        }
                    };
                    body.push(item.into_any_element());
                }
                // A disabled entry says why (IMP-0278): the highlighted one, or the one chosen.
                let why = menu.note.clone().or_else(|| {
                    match menu.cursor.and_then(|i| menu.rows.get(i)) {
                        Some(MenuRow::Entry(e)) => menu.entries[*e].reason.clone(),
                        _ => None,
                    }
                });
                if let Some(why) = why {
                    body.push(
                        div()
                            .mt_1()
                            .px_2()
                            .py_1()
                            .border_t_1()
                            .border_color(c(p.separator))
                            .text_xs()
                            .text_color(c(p.ui_muted))
                            .whitespace_normal()
                            .child(why)
                            .into_any_element(),
                    );
                }
            }
            MenuPage::Reset(cursor) => {
                let branch = self.current.clone().unwrap_or_else(|| "HEAD".into());
                body.push(header(format!("Reset {branch} to {short}")).into_any_element());
                for (i, mode) in ResetMode::ALL.into_iter().enumerate() {
                    body.push(
                        crate::metrics::menu_item(("git-log-reset-mode", i), *cursor == i)
                            .whitespace_nowrap()
                            .child(mode.label())
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.choose_reset_mode(mode, cx)
                            }))
                            .into_any_element(),
                    );
                }
            }
            MenuPage::Checkout(cursor) | MenuPage::Branch { cursor, .. } => {
                let title = match &menu.page {
                    MenuPage::Branch { name, .. } => format!("Branch '{name}'"),
                    _ => format!("Checkout at {short}"),
                };
                body.push(header(title).into_any_element());
                for (i, label) in self.commit_menu_page_items().into_iter().enumerate() {
                    body.push(
                        crate::metrics::menu_item(("git-log-page-item", i), *cursor == i)
                            .whitespace_nowrap()
                            .child(label)
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.choose_page_item(i, cx)
                            }))
                            .into_any_element(),
                    );
                }
            }
            MenuPage::Name {
                kind: purpose,
                text,
                error,
            } => {
                let kind = purpose.noun();
                let title = match purpose {
                    NameKind::BranchFrom(start) => format!("New branch from {start}"),
                    NameKind::Rename(old) => format!("Rename {old}"),
                    _ => format!("New {kind} at {short}"),
                };
                body.push(header(title).into_any_element());
                let field = if text.is_empty() {
                    div()
                        .text_color(c(p.ui_dim))
                        .child(format!("Name of the {kind}"))
                } else {
                    div().child(text.clone())
                };
                body.push(
                    crate::metrics::field_frame()
                        .mx_2()
                        .border_color(c(p.focus))
                        .child(field)
                        .into_any_element(),
                );
                body.push(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(c(if error.is_some() {
                            p.error_text
                        } else {
                            p.ui_dim
                        }))
                        .child(
                            error
                                .clone()
                                .unwrap_or_else(|| "Enter creates · Escape goes back".into()),
                        )
                        .into_any_element(),
                );
            }
        }
        let frame = crate::metrics::menu_frame()
            .id("git-log-commit-menu")
            .occlude()
            .min_w(px(260.))
            .max_w(px(360.))
            .bg(c(p.island))
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.commit_menu = None;
                cx.notify();
            }))
            .children(body);
        let anchored = match menu.at {
            Some(at) => gpui::anchored().position(at),
            None => gpui::anchored().offset(point(px(48.), px(ROW))),
        };
        Some(
            gpui::deferred(anchored.snap_to_window_with_margin(px(4.)).child(frame))
                .with_priority(2)
                .into_any_element(),
        )
    }

    /// The keys of every folder of the branches pane, to close them all.
    fn all_folders(&self) -> BTreeSet<String> {
        tree_rows(&self.branches, self.detached, "", &BTreeSet::new())
            .into_iter()
            .filter_map(|r| match r.item {
                TreeItem::Folder(k) => Some(k),
                TreeItem::Branch(_) => None,
            })
            .collect()
    }

    /// The branch highlighted in the branches pane.
    fn highlighted_branch(&self) -> Option<String> {
        match self
            .branch_rows()
            .get(self.tree_row)
            .map(|r| r.item.clone())
        {
            Some(TreeItem::Branch(b)) if b != "HEAD" => Some(b),
            _ => None,
        }
    }

    /// Runs a button of the branches pane's action bar (`SPEC-0086` v7 behaviour 16).
    pub fn branch_action(&mut self, action: BranchAction, cx: &mut Context<Self>) {
        match action {
            BranchAction::New => self.tree_new = Some(String::new()),
            BranchAction::Checkout => {
                let local = self.names_of(false);
                if let Some(b) = self.highlighted_branch().filter(|b| local.contains(b)) {
                    cx.emit(GitLogEvent::CheckoutBranch(b));
                }
            }
            BranchAction::Fetch => cx.emit(GitLogEvent::Fetch),
            BranchAction::ExpandAll => self.tree_closed.clear(),
            BranchAction::CollapseAll => self.tree_closed = self.all_folders(),
            BranchAction::Favorite => self.toggle_row_favorite(self.tree_row, cx),
            BranchAction::OnlyFavorites => {
                self.only_favorites = !self.only_favorites;
                self.tree_row = 0;
            }
        }
        self.in_branches = true;
        self.in_files = false;
        cx.notify();
    }

    /// Whether the new-branch field of the branches pane is open, and its text.
    pub fn new_branch_field(&self) -> Option<&str> {
        self.tree_new.as_deref()
    }

    /// Sets the text of the new-branch field (tests).
    pub fn set_new_branch_name(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.tree_new.is_some() {
            self.tree_new = Some(name.to_owned());
        }
        cx.notify();
    }

    /// Creates the branch typed in the field at the highlighted branch (HEAD when none).
    pub fn confirm_new_branch(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.tree_new.clone().map(|n| n.trim().to_owned()) else {
            return;
        };
        if !ferris_idea_vcs::valid_branch_name(&name) || self.names_of(false).contains(&name) {
            return;
        }
        let from = self.highlighted_branch();
        let commit = from
            .as_deref()
            .and_then(|b| tip_of(&self.labels, b))
            .or_else(|| self.head_tip())
            .map(str::to_owned);
        if let Some(commit) = commit {
            self.tree_new = None;
            cx.emit(GitLogEvent::Commit {
                commit,
                action: CommitAction::NewBranch,
                mode: None,
                name: Some(name),
            });
        }
        cx.notify();
    }

    /// Keys of the new-branch field.
    fn on_new_branch_key(&mut self, key: &str, text: Option<&str>, cx: &mut Context<Self>) {
        match key {
            "enter" => self.confirm_new_branch(cx),
            "escape" => self.tree_new = None,
            "backspace" => {
                if let Some(n) = &mut self.tree_new {
                    n.pop();
                }
            }
            _ => {
                if let (Some(n), Some(t)) = (&mut self.tree_new, text) {
                    n.push_str(t);
                }
            }
        }
        cx.notify();
    }

    /// The vertical action bar left of the branches tree.
    fn branch_actions_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let buttons = [
            (
                BranchAction::New,
                declared!(GIT_LOG_TOOLBAR, "git-log-branch-new"),
                "general/add",
            ),
            (
                BranchAction::Checkout,
                declared!(GIT_LOG_TOOLBAR, "git-log-branch-checkout"),
                "general/vcs",
            ),
            (
                BranchAction::Fetch,
                declared!(GIT_LOG_TOOLBAR, "git-log-branch-fetch"),
                "general/refresh",
            ),
            (
                BranchAction::ExpandAll,
                declared!(GIT_LOG_TOOLBAR, "git-log-branch-expand"),
                "general/expandAll",
            ),
            (
                BranchAction::CollapseAll,
                declared!(GIT_LOG_TOOLBAR, "git-log-branch-collapse"),
                "general/collapseAll",
            ),
            // Favourites (v10 behaviour 19): the star toggles, the second shows only them.
            (
                BranchAction::Favorite,
                declared!(GIT_LOG_TOOLBAR, "git-log-branch-favorite"),
                "ferris-idea/starOutline",
            ),
            (
                BranchAction::OnlyFavorites,
                declared!(GIT_LOG_TOOLBAR, "git-log-branch-only-favorites"),
                "ferris-idea/star",
            ),
        ];
        let only = self.only_favorites;
        div()
            .w(px(ACTIONS_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .pt_1()
            .border_r_1()
            .border_color(c(p.separator))
            .children(buttons.map(|(action, control, key)| {
                let pressed = action == BranchAction::OnlyFavorites && only;
                div()
                    .id(control.name)
                    .tip(control)
                    .p(px(3.))
                    .rounded(px(4.))
                    .when(pressed, |d| d.bg(c(p.toolbar_hover)))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(icon(key))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.branch_action(action, cx)
                    }))
            }))
            .into_any_element()
    }
}

/// A button of the branches pane's action bar (`SPEC-0086` v7 behaviour 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchAction {
    /// A new branch at the highlighted branch (or HEAD), named in a field.
    New,
    /// Check out the highlighted local branch.
    Checkout,
    /// Fetch every remote.
    Fetch,
    /// Open every folder.
    ExpandAll,
    /// Close every folder.
    CollapseAll,
    /// Mark or unmark the highlighted branch as a favourite (v10 behaviour 19).
    Favorite,
    /// List only the favourites, or all branches again.
    OnlyFavorites,
}

/// One label of a row: the current branch filled in the accent colour, tags with their symbol
/// and name (IMP-0321), remote branches dimmer (`SPEC-0086` v7 behaviour 14).
fn label_chip(label: &Label, p: &Palette) -> gpui::AnyElement {
    let emphasised = matches!(label.kind, LabelKind::Current | LabelKind::Head);
    div()
        .flex_none()
        .max_w(px(180.))
        .px_1()
        .rounded(px(3.))
        .flex()
        .items_center()
        .gap(px(2.))
        .overflow_hidden()
        .whitespace_nowrap()
        .when(emphasised, |d| {
            d.bg(c(p.accent))
                .text_color(c(p.accent_text))
                .font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .when(!emphasised, |d| {
            d.border_1().border_color(c(p.separator)).text_color(c(
                if label.kind == LabelKind::Remote {
                    p.ui_dim
                } else {
                    p.ui_muted
                },
            ))
        })
        .when(label.kind == LabelKind::Tag, |d| {
            d.child(icon("ferris-idea/gitTag"))
        })
        .child(
            div()
                .overflow_hidden()
                .text_ellipsis()
                .child(label.text.clone()),
        )
        .into_any_element()
}

crate::committed_text::committed_text_input!(GitLogView, take_committed);

impl Render for GitLogView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let filter = if self.filter.text.is_empty() {
            div().text_color(c(p.ui_dim)).child("Text or hash")
        } else {
            div()
                .when(self.text_error.is_some(), |d| d.text_color(c(p.error_text)))
                .child(self.filter.text.clone())
        };
        div()
            .id("git-log-panel")
            .track_focus(&self.focus)
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .size_full()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .h(px(36.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .overflow_hidden()
                    // Title and the "Log" tab are in the shared header (SPEC-0061 v5
                    // behaviour 18).
                    .child(
                        div()
                            .w(px(260.))
                            .flex_none()
                            .px_2()
                            .py(px(2.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(p.separator))
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(icon("general/search"))
                            .child(div().flex_1().min_w_0().overflow_hidden().child(filter))
                            .child(self.toggle(
                                declared!(GIT_LOG_TOOLBAR, "git-log-regex"),
                                ".*",
                                self.filter.regex,
                                |f| f.regex = !f.regex,
                                cx,
                            ))
                            .child(self.toggle(
                                declared!(GIT_LOG_TOOLBAR, "git-log-case"),
                                "Cc",
                                self.filter.case,
                                |f| f.case = !f.case,
                                cx,
                            )),
                    )
                    .children(
                        [
                            FilterKind::Branch,
                            FilterKind::User,
                            FilterKind::Date,
                            FilterKind::Paths,
                        ]
                        .map(|k| self.filter_button(k, cx)),
                    )
                    .when(self.filter.active() || self.branch_filter.is_some(), |d| {
                        d.child(
                            div()
                                .id("git-log-clear-filters")
                                .tip(declared!(GIT_LOG_TOOLBAR, "git-log-clear-filters"))
                                .p(px(2.))
                                .rounded(px(4.))
                                .hover(|s| s.bg(c(p.tree_hover)))
                                .child(icon("general/closeSmall"))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.clear_filters(cx)
                                })),
                        )
                    })
                    .child(div().flex_1()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .w(px(BRANCHES_W + ACTIONS_W))
                            .flex_none()
                            .min_h_0()
                            .flex()
                            .child(self.branch_actions_bar(cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .child(self.branches_pane(cx)),
                            ),
                    )
                    .child(div().w(px(1.)).flex_none().bg(c(p.separator)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(self.commit_list(cx))
                            .children(self.commit_menu_view(cx)),
                    )
                    .child(div().w(px(1.)).flex_none().bg(c(p.separator)))
                    .child(
                        div()
                            .w(px(380.))
                            .flex_none()
                            .min_h_0()
                            .child(self.details_pane(cx)),
                    ),
            )
    }
}

/// Colours of the graph's lines, taken from the theme's console colours (`SPEC-0086` v3
/// behaviour 8).
/// The main page's rows: the entries, with a page per branch at the commit before New Branch…
/// (`SPEC-0086` v8 behaviour 17).
fn menu_rows(entries: &[ActionState], branches: &[(String, bool)]) -> Vec<MenuRow> {
    let mut rows = Vec::with_capacity(entries.len() + branches.len());
    for (i, e) in entries.iter().enumerate() {
        if e.action == CommitAction::NewBranch {
            rows.extend(
                branches
                    .iter()
                    .map(|(n, remote)| MenuRow::Branch(n.clone(), *remote)),
            );
        }
        rows.push(MenuRow::Entry(i));
    }
    rows
}

/// An entry's label; Checkout Revision is "Checkout" when it opens a page (v8).
fn row_label(e: &ActionState, checkout: &[String]) -> String {
    if e.action == CommitAction::Checkout && !checkout.is_empty() {
        "Checkout".into()
    } else {
        e.label.to_owned()
    }
}

/// The Checkout page: the local branches at the commit, then the revision (v8).
fn checkout_items(branches: &[String], commit: &str) -> Vec<String> {
    branches
        .iter()
        .cloned()
        .chain(std::iter::once(format!(
            "Revision '{}'",
            &commit[..commit.len().min(7)]
        )))
        .collect()
}

/// The entries of a branch page: the branch popup's submenu of that branch (v8).
fn page_actions(
    name: &str,
    remote: bool,
    current: Option<&str>,
    favorite: bool,
) -> Vec<(PopupBranchAction, String)> {
    let row = BranchRow::Branch(ferris_idea_vcs::Branch {
        name: name.to_owned(),
        remote,
        current: false,
        target: String::new(),
    });
    branch_actions(&row, current, favorite)
}

/// Whether pane row `i` lies in the "Remote" folder.
fn row_is_remote(rows: &[TreeRow], i: usize) -> bool {
    rows[..=i.min(rows.len().saturating_sub(1))]
        .iter()
        .rev()
        .find(|r| r.depth == 0)
        .is_some_and(|r| r.item == TreeItem::Folder("remote".into()))
}

/// The favourites of the repository `repo` found from the workspace at `root`.
fn favorites_of(root: &Path, repo: &Repo) -> ferris_idea_core::RepoFavorites {
    let key = repo
        .workdir()
        .map_or_else(|| ".".to_owned(), |w| ferris_idea_core::repo_key(root, w));
    ferris_idea_core::BranchFavorites::load(root).of(&key)
}

fn lane_colors(p: &Palette) -> std::rc::Rc<[u32]> {
    [4, 2, 5, 3, 6, 1, 12, 10, 13, 11, 14, 9]
        .iter()
        .map(|&k| p.console_ansi[k])
        .collect()
}

/// The graph of one row: lines in the upper and lower half and the commit's dot
/// (`SPEC-0086` v3 behaviour 8).
fn graph_cell(row: GraphRow, width: f32, colors: std::rc::Rc<[u32]>) -> gpui::AnyElement {
    let x = |lane: usize| LANE * lane as f32 + LANE / 2. + 2.;
    let color = move |k: usize| rgb(colors[k % colors.len()]);
    let dot = (row.lane < MAX_LANES).then(|| {
        div()
            .absolute()
            .left(px(x(row.lane) - 3.5))
            .top(px(ROW / 2. - 3.5))
            .size(px(7.))
            .rounded_full()
            .bg(color(row.color))
    });
    let lines = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let o = bounds.origin;
            let mid = ROW / 2.;
            let halves = [(&row.up, 0., mid), (&row.down, mid, ROW)];
            for (edges, top, bottom) in halves {
                for e in edges.iter() {
                    if e.from >= MAX_LANES || e.to >= MAX_LANES {
                        continue;
                    }
                    let mut path = PathBuilder::stroke(px(1.5));
                    path.move_to(point(o.x + px(x(e.from)), o.y + px(top)));
                    path.line_to(point(o.x + px(x(e.to)), o.y + px(bottom)));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color(e.color));
                    }
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    div()
        .relative()
        .w(px(width))
        .h(px(ROW))
        .flex_none()
        .child(lines)
        .children(dot)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_rows_put_branch_pages_before_new_branch() {
        // SPEC-0086 v8 AK-28.
        let facts = ferris_idea_vcs::CommitFacts {
            on_current_branch: true,
            ..Default::default()
        };
        let entries = commit_actions(&facts);
        let rows = menu_rows(
            &entries,
            &[("topic".into(), false), ("origin/x".into(), true)],
        );
        assert_eq!(rows.len(), entries.len() + 2);
        let new_branch = entries
            .iter()
            .position(|e| e.action == CommitAction::NewBranch)
            .unwrap();
        let at = rows
            .iter()
            .position(|r| *r == MenuRow::Entry(new_branch))
            .unwrap();
        assert_eq!(rows[at - 2], MenuRow::Branch("topic".into(), false));
        assert_eq!(rows[at - 1], MenuRow::Branch("origin/x".into(), true));
        let checkout = entries
            .iter()
            .find(|e| e.action == CommitAction::Checkout)
            .unwrap();
        assert_eq!(row_label(checkout, &[]), "Checkout Revision");
        assert_eq!(row_label(checkout, &["topic".into()]), "Checkout");
        assert_eq!(
            checkout_items(&["topic".into()], "0123456789abcdef"),
            ["topic", "Revision '0123456'"]
        );
        let page = page_actions("origin/x", true, Some("main"), false);
        assert!(
            page.iter()
                .any(|(a, _)| *a == PopupBranchAction::CompareWith)
        );
        assert!(!page.iter().any(|(a, _)| *a == PopupBranchAction::Delete));
    }

    /// SPEC-0086 v10 AK-31: a branch page ends with Mark/Unmark as Favorite, as the popup's
    /// submenu; a remote row is known by its folder.
    #[test]
    fn branch_page_lists_the_popups_entries() {
        let page = page_actions("topic", false, Some("main"), false);
        assert_eq!(
            page.last(),
            Some(&(PopupBranchAction::Favorite, "Mark as Favorite".to_owned()))
        );
        let page = page_actions("origin/main", true, Some("main"), true);
        assert_eq!(
            page.last().map(|(_, l)| l.as_str()),
            Some("Unmark as Favorite")
        );
        let branches = [
            BranchName {
                name: "main".into(),
                remote: false,
                current: true,
                favorite: true,
            },
            BranchName {
                name: "origin/main".into(),
                remote: true,
                current: false,
                favorite: true,
            },
        ];
        let rows = tree_rows(&branches, false, "", &BTreeSet::new());
        let remote_main = rows
            .iter()
            .position(|r| r.item == TreeItem::Branch("origin/main".into()))
            .unwrap();
        assert!(row_is_remote(&rows, remote_main));
        assert!(!row_is_remote(&rows, 2), "main under Local");
    }
}
