//! The Commit tool window (`SPEC-0063`): changed files with checkboxes, a message field and Commit;
//! with commit hooks the commit runs in the background and shows their output (`SPEC-0230`).
//! v5: toolbar (refresh, rollback with confirmation, diff, Group By, expand/collapse all), amend
//! with the amended commit shown, a placeholder, Commit and Push…, and an options panel.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use gpui::{Context, Entity, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, rgb, rgba};
use idecraft_ui::theme::Rgba;
use idecraft_vcs::{CommitOptions, Repo};

use crate::EditorView;
use crate::changelist_model::ListStrip;
use crate::commit_model::{
    AmendMessage, CommitList, GroupBy, HISTORY_VISIBLE_ROWS, ListGroup, MessageHistory, Row,
    RowKind, module_folder, status_color_key,
};
use idecraft_core::changelists::Changelists;

mod changelists;
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::{Tipped, declared};

fn c(color: Rgba) -> gpui::Hsla {
    rgba((color.rgb << 8) | u32::from(color.alpha)).into()
}

/// The placeholder of the empty message field (v5 behaviour 14).
pub const MESSAGE_PLACEHOLDER: &str = "Commit message";

/// The toolbar's buttons with their icons, in order (`SPEC-0063` v7 behaviour 25: icons, no text
/// glyphs; OBS-0022 L15).
pub const TOOLBAR_ICONS: [(&idecraft_ui::tooltip::Control, &str); 6] = [
    (
        declared!(COMMIT_TOOLBAR, "commit-tool-refresh"),
        "general/refresh",
    ),
    (
        declared!(COMMIT_TOOLBAR, "commit-tool-rollback"),
        "idecraft/rollback",
    ),
    (
        declared!(COMMIT_TOOLBAR, "commit-tool-diff"),
        "idecraft/diff",
    ),
    (
        declared!(COMMIT_TOOLBAR, "commit-tool-group"),
        "idecraft/groupBy",
    ),
    (
        declared!(COMMIT_TOOLBAR, "commit-tool-expand"),
        "general/expandAll",
    ),
    (
        declared!(COMMIT_TOOLBAR, "commit-tool-collapse"),
        "general/collapseAll",
    ),
];

/// What the Commit tool window asks the workbench to do.
pub enum CommitEvent {
    /// Show the changes of a file (path relative to the repository root) in the diff viewer
    /// (SPEC-0084).
    ShowDiff(String),
    /// Open the file (path relative to the repository root) in the editor: F4 or ⌘↓ on a row
    /// (`EditSource`, SPEC-0063 v8).
    OpenFile(String),
    /// Save the open files, then call `commit_now` (SPEC-0063 v3 behaviour 7).
    Commit,
    /// A commit with hooks ended; the text is for the status bar (SPEC-0230 behaviour 7).
    Committed(String),
    /// "Commit and Push…" committed: open the Push dialog (SPEC-0063 v5 behaviour 15).
    Push,
    /// Files were rolled back (absolute paths): open editors show the disk content (v5
    /// behaviour 10).
    RolledBack(Vec<PathBuf>),
    /// A text for the status bar (v10 behaviour 22: the message history).
    Status(String),
}

/// The last commit, for the amend switch (v5 behaviour 13).
#[derive(Clone)]
struct LastCommit {
    short_id: String,
    summary: String,
    message: String,
}

/// Changes of the repository at the workspace root and the message for the next commit.
pub struct CommitView {
    root: PathBuf,
    list: CommitList,
    /// The row last clicked (SPEC-0084 behaviour 1).
    current: Option<String>,
    /// Files with unsaved changes in open editors, relative to the repository root.
    unsaved: Vec<String>,
    message: Entity<EditorView>,
    focus: FocusHandle,
    note: Option<(String, bool)>,
    /// Commit hooks of the repository (SPEC-0230 behaviour 6).
    hooks: Vec<String>,
    /// "Run Git hooks" is ticked.
    run_hooks: bool,
    /// A commit with hooks is running.
    committing: bool,
    /// Output of the hooks of the last commit (SPEC-0230 behaviour 3).
    hook_output: Option<String>,
    /// Working tree root of the repository, for modules (v5 behaviour 11).
    workdir: Option<PathBuf>,
    /// Group By switches (v5 behaviour 11).
    group_by: GroupBy,
    /// The Group By menu is open.
    group_menu: bool,
    /// Keys of folded nodes (v5 behaviour 11).
    folded: BTreeSet<String>,
    /// Module folder of each listed file, when grouping by module.
    modules: HashMap<String, String>,
    /// Files waiting for the rollback confirmation (v5 behaviour 10).
    rollback: Option<Vec<String>>,
    /// "Delete local copies of added files" in the confirmation.
    delete_added: bool,
    /// The amend switch (v5 behaviour 13).
    amend: bool,
    amend_message: AmendMessage,
    last: Option<LastCommit>,
    /// The commit was started by "Commit and Push…" (v5 behaviour 15).
    push_after: bool,
    /// The options panel is open (v5 behaviour 16).
    options_open: bool,
    /// The author option, `Name <email>` or empty.
    author: Entity<EditorView>,
    /// The repository of a remote project's host (`SPEC-0318` v2 detail 7).
    remote: Option<crate::workbench_view::HostCall>,
    /// A refresh of the host's status is due.
    refresh_wanted: bool,
    /// The open list of Commit Message History (v10 behaviour 22).
    history: Option<MessageHistory>,
    history_scroll: gpui::ScrollHandle,
    /// The changelists of the project (`SPEC-0335`); the one list `Changes` in a remote project.
    lists: Changelists,
    /// Why the state file of the changelists was not used at the last look.
    lists_problem: Option<idecraft_core::changelists::Unreadable>,
    /// Modification time and size of the state file at the last look (behaviour 17).
    lists_stamp: Option<(std::time::SystemTime, u64)>,
    /// The selected changelist (`SPEC-0335` behaviour 14); a selected file is in `current`.
    current_list: Option<String>,
    /// The open strip of a changelist action (`SPEC-0335` behaviours 8–12).
    list_strip: Option<ListStrip>,
    /// The name field of the strip.
    list_name: Entity<EditorView>,
    /// Why the strip's name was refused.
    list_error: Option<String>,
    /// "Set active" of New Changelist… and Move, kept for the window's lifetime.
    new_list_active: bool,
    /// The changelists menu of the toolbar is open.
    lists_menu: bool,
}

/// Status and last commit of the host repository.
fn host_status(
    host: &crate::workbench_view::HostCall,
) -> Result<(Vec<idecraft_vcs::FileStatus>, Option<LastCommit>), String> {
    let v = host("git.status", serde_json::json!({}))?;
    // A path outside the project comes as `{outside: name}` (behaviour 3): not listed.
    let status = v["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["path"].is_string())
        .filter_map(|f| serde_json::from_value(f.clone()).ok())
        .collect();
    let last = host("git.show", serde_json::json!({ "id": "HEAD" }))
        .ok()
        .and_then(|d| serde_json::from_value::<idecraft_vcs::CommitDetails>(d).ok())
        .map(|d| LastCommit {
            short_id: d.info.short_id,
            summary: d.info.summary,
            message: d.message.trim_end().to_owned(),
        });
    Ok((status, last))
}

impl CommitView {
    /// A commit view on the repository at (or above) `root`.
    pub fn new(root: &Path, cx: &mut Context<Self>) -> Self {
        let message = cx.new(EditorView::field);
        let author = cx.new(EditorView::field);
        let list_name = cx.new(EditorView::field);
        // The strip marks a name no list has as new while it is typed.
        cx.observe(&list_name, |_, _, cx| cx.notify()).detach();
        let mut view = Self {
            root: root.to_path_buf(),
            list: CommitList::default(),
            current: None,
            unsaved: Vec::new(),
            message,
            focus: cx.focus_handle(),
            note: None,
            hooks: Vec::new(),
            run_hooks: true,
            committing: false,
            hook_output: None,
            workdir: None,
            group_by: GroupBy::default(),
            group_menu: false,
            folded: BTreeSet::new(),
            modules: HashMap::new(),
            rollback: None,
            delete_added: false,
            amend: false,
            amend_message: AmendMessage::default(),
            last: None,
            push_after: false,
            options_open: false,
            author,
            remote: None,
            refresh_wanted: false,
            history: None,
            history_scroll: gpui::ScrollHandle::new(),
            lists: Changelists::default(),
            lists_problem: None,
            lists_stamp: None,
            current_list: None,
            list_strip: None,
            list_name,
            list_error: None,
            new_list_active: false,
            lists_menu: false,
        };
        view.refresh();
        view
    }

    /// Shows the repository of a remote project's host from now on (`SPEC-0318` v2 detail 7);
    /// its status is read in the background whenever a refresh is due.
    pub fn set_remote(&mut self, host: crate::workbench_view::HostCall, cx: &mut Context<Self>) {
        self.remote = Some(host);
        self.list = CommitList::default();
        // No changelists in a remote project yet (`SPEC-0335` behaviour 18).
        self.lists = Changelists::default();
        self.lists_problem = None;
        self.lists_stamp = None;
        self.current_list = None;
        self.list_strip = None;
        self.hooks.clear();
        self.workdir = None;
        self.note = None;
        self.refresh_wanted = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let Ok(due) = this.update(cx, |this, _| {
                    std::mem::take(&mut this.refresh_wanted)
                        .then(|| this.remote.clone())
                        .flatten()
                }) else {
                    break;
                };
                let Some(host) = due else { continue };
                let read = cx
                    .background_executor()
                    .spawn(async move { host_status(&host) })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    match read {
                        Ok((status, last)) => {
                            this.last = last;
                            this.list = CommitList::from_status(&status, Some(&this.list))
                                .with_unsaved(&this.unsaved, Some(&this.list));
                        }
                        Err(e) => this.note = Some((e, false)),
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Whether the view shows a remote project's host repository.
    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// Focus handle of the message field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.message.read(cx).focus()
    }

    /// The message field (for the input method and for tests).
    pub fn message_field(&self) -> Entity<EditorView> {
        self.message.clone()
    }

    /// The author field of the commit options.
    pub fn author_field(&self) -> Entity<EditorView> {
        self.author.clone()
    }

    /// Whether the input method composes in the message or the author field: it owns the keys
    /// then (`SPEC-0028` v6 behaviour 11a).
    pub fn composing(&self, cx: &gpui::App) -> bool {
        self.message.read(cx).is_composing() || self.author.read(cx).is_composing()
    }

    /// The row last clicked, if any.
    pub fn current(&self) -> Option<String> {
        self.current.clone()
    }

    /// Selects a row as a click does, and with `diff` asks for its diff as a double-click does.
    pub fn click(&mut self, path: &str, diff: bool, cx: &mut Context<Self>) {
        self.current = Some(path.to_owned());
        self.current_list = None;
        if diff {
            cx.emit(CommitEvent::ShowDiff(path.to_owned()));
        }
        cx.notify();
    }

    /// Paths currently checked, for tests.
    pub fn selected(&self) -> Vec<String> {
        self.list.selected()
    }

    /// The repository's commit hooks, shown as "Run Git hooks" when there are any.
    pub fn hooks(&self) -> &[String] {
        &self.hooks
    }

    /// Ticks or unticks "Run Git hooks" (SPEC-0230 behaviour 6).
    pub fn set_run_hooks(&mut self, on: bool, cx: &mut Context<Self>) {
        self.run_hooks = on;
        cx.notify();
    }

    /// Output of the hooks of the last commit, if any.
    pub fn hook_output(&self) -> Option<&str> {
        self.hook_output.as_deref()
    }

    /// The line under the message: the last result.
    pub fn note(&self) -> Option<&str> {
        self.note.as_ref().map(|(t, _)| t.as_str())
    }

    /// Replaces the commit message.
    pub fn set_message(&mut self, text: &str, cx: &mut Context<Self>) {
        self.message.update(cx, |m, cx| m.set_text(text, cx));
    }

    /// `Vcs.ShowMessageHistory` (v10 behaviour 22): opens the list of the commit messages used in
    /// this project, newest first, with the focus in the message field, or closes it when it is
    /// open. Without a message, in a remote project or with a history file that cannot be used
    /// the status bar says so and no list opens.
    pub fn show_message_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.take().is_some() {
            cx.notify();
            return;
        }
        if self.remote.is_some() {
            cx.emit(CommitEvent::Status(
                crate::commit_model::NO_REMOTE_MESSAGE_HISTORY.into(),
            ));
            return;
        }
        let (history, problem) = idecraft_core::commit_messages::CommitMessages::load(&self.root);
        self.history = MessageHistory::new(history.newest_first());
        match (&self.history, problem) {
            (_, Some(why)) => cx.emit(CommitEvent::Status(
                crate::commit_model::history_unreadable(why),
            )),
            (None, None) => cx.emit(CommitEvent::Status(
                crate::commit_model::NO_MESSAGE_HISTORY.into(),
            )),
            (Some(_), None) => {
                self.history_scroll = gpui::ScrollHandle::new();
                window.focus(&self.focus(cx));
            }
        }
        cx.notify();
    }

    /// The rows of the open message history, newest first, and the selected one.
    pub fn message_history(&self) -> Option<(Vec<String>, usize)> {
        self.history.as_ref().map(|h| (h.rows(), h.selected()))
    }

    /// Moves the selection of the open message history down (`forward`) or up.
    pub fn step_message_history(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(h) = self.history.as_mut() {
            h.step(forward);
            self.history_scroll.scroll_to_item(h.selected());
            cx.notify();
        }
    }

    /// Chooses row `index` of the open message history, or the selected row: the message
    /// replaces the field's text as one undo step and the list closes.
    pub fn choose_message(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let Some(mut history) = self.history.take() else {
            return;
        };
        if index.is_none_or(|i| history.select(i))
            && let Some((range, text)) =
                crate::commit_model::message_edit(&self.message(cx), history.chosen())
        {
            self.message
                .update(cx, |m, cx| m.replace_ranges(&[(range, text)], cx));
        }
        cx.notify();
    }

    /// Closes the message history without choosing; `false` when it was not open.
    pub fn close_message_history(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.history.take().is_some();
        if open {
            cx.notify();
        }
        open
    }

    /// Takes back the last change of the message field, or with `redo` makes it again.
    pub fn undo_message(&mut self, redo: bool, cx: &mut Context<Self>) {
        self.message.update(cx, |m, cx| m.undo_plain(redo, cx));
    }

    /// Checks exactly `paths` (for the self-test and agents driving the window).
    pub fn check_only(&mut self, paths: &[&str], cx: &mut Context<Self>) {
        for p in self.list.selected() {
            if !paths.contains(&p.as_str()) {
                self.list.toggle(&p);
            }
        }
        for p in paths {
            if !self.list.is_checked(p) {
                self.list.toggle(p);
            }
        }
        cx.notify();
    }

    /// The typed commit message.
    pub fn message(&self, cx: &gpui::App) -> String {
        self.message.read(cx).state.buffer.text()
    }

    /// Whether the empty message field shows its placeholder (v5 behaviour 14).
    pub fn placeholder_shown(&self, cx: &gpui::App) -> bool {
        self.message.read(cx).state.buffer.len_chars() == 0
    }

    /// The rows of the list as shown (v5 behaviour 11).
    pub fn rows(&self) -> Vec<Row> {
        self.rows_folded(&self.folded)
    }

    /// The rows with the nodes of `folded` closed: a group per changelist (`SPEC-0335`
    /// behaviour 13), or the group Changes in a remote project.
    fn rows_folded(&self, folded: &BTreeSet<String>) -> Vec<Row> {
        let modules = &self.modules;
        let module_of = |p: &str| modules.get(p).cloned().unwrap_or_default();
        if self.remote.is_some() {
            return self
                .list
                .rows(self.group_by, &module_of, &self.root_name(), folded);
        }
        let groups: Vec<ListGroup> = self
            .lists
            .lists()
            .iter()
            .map(|l| ListGroup {
                name: l.name.clone(),
                active: l.name == self.lists.active(),
                files: l.files.clone(),
            })
            .collect();
        self.list.rows_in_lists(
            &groups,
            self.group_by,
            &module_of,
            &self.root_name(),
            folded,
        )
    }

    /// The files of the list in the order of its rows, folded folders included (the file list of
    /// the diff viewer, `SPEC-0084` v5 behaviour 20).
    pub fn files(&self) -> Vec<String> {
        self.rows_folded(&Default::default())
            .into_iter()
            .filter(|r| matches!(r.kind, RowKind::File { .. }))
            .map(|r| r.key)
            .collect()
    }

    fn root_name(&self) -> String {
        self.workdir
            .as_deref()
            .unwrap_or(&self.root)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "root".into())
    }

    /// The Group By switches.
    pub fn group_by(&self) -> GroupBy {
        self.group_by
    }

    /// Sets the Group By switches (v5 behaviour 11).
    pub fn set_group_by(&mut self, group_by: GroupBy, cx: &mut Context<Self>) {
        self.group_by = group_by;
        self.load_modules();
        cx.notify();
    }

    /// Flips one Group By switch, as its menu entry does (`ChangesView.GroupBy.Directory`,
    /// `ChangesView.GroupBy.Module`; v9 behaviour 21).
    pub fn toggle_group_by(&mut self, directory: bool, cx: &mut Context<Self>) {
        let mut group_by = self.group_by;
        if directory {
            group_by.directory = !group_by.directory;
        } else {
            group_by.module = !group_by.module;
        }
        self.set_group_by(group_by, cx);
    }

    /// Folds or unfolds a node by its key.
    pub fn toggle_fold(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.folded.remove(key) {
            self.folded.insert(key.to_owned());
        }
        cx.notify();
    }

    /// Unfolds every node (Expand All).
    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.folded.clear();
        cx.notify();
    }

    /// Folds every node, the groups included (Collapse All).
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.folded = self
            .rows_folded(&Default::default())
            .into_iter()
            .filter(|r| !matches!(r.kind, RowKind::File { .. }))
            .map(|r| r.key)
            .collect();
        cx.notify();
    }

    fn load_modules(&mut self) {
        self.modules.clear();
        if !self.group_by.module {
            return;
        }
        let Some(workdir) = self.workdir.clone() else {
            return;
        };
        for f in self.list.changes.iter().chain(&self.list.unversioned) {
            self.modules
                .insert(f.path.clone(), module_folder(&workdir, &f.path));
        }
    }

    /// Re-reads the status, keeping the user's checks (behaviour 4).
    pub fn refresh(&mut self) {
        if self.remote.is_some() {
            self.refresh_wanted = true;
            return;
        }
        let repo = Repo::discover(&self.root);
        self.hooks = repo
            .as_ref()
            .map(|r| r.commit_hooks(true))
            .unwrap_or_default();
        self.workdir = repo
            .as_ref()
            .ok()
            .and_then(|r| r.workdir().map(Path::to_path_buf));
        self.last = repo
            .as_ref()
            .ok()
            .and_then(|r| r.commit_details("HEAD").ok())
            .map(|d| LastCommit {
                short_id: d.info.short_id,
                summary: d.info.summary,
                message: d.message.trim_end().to_owned(),
            });
        match repo.and_then(|r| r.status_with_renames()) {
            Ok((status, renames)) => {
                self.list = CommitList::from_status(&status, Some(&self.list))
                    .with_unsaved(&self.unsaved, Some(&self.list));
                self.read_lists(&renames);
            }
            Err(e) => self.note = Some((e.to_string(), false)),
        }
        self.load_modules();
    }

    /// Files with unsaved changes in open editors (relative to the repository root), listed as
    /// changes from the next refresh on.
    pub fn set_unsaved(&mut self, paths: Vec<String>) {
        self.unsaved = paths;
    }

    /// Asks the workbench to save the open files and commit (behaviour 5).
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        self.push_after = false;
        cx.emit(CommitEvent::Commit);
    }

    /// "Commit and Push…": commits as Commit does, then asks for the Push dialog (v5
    /// behaviour 15).
    pub fn commit_and_push(&mut self, cx: &mut Context<Self>) {
        self.push_after = true;
        cx.emit(CommitEvent::Commit);
    }

    /// Whether the amend switch is on.
    pub fn amend(&self) -> bool {
        self.amend
    }

    /// The commit an amend replaces, as short id and subject, if there is one.
    pub fn amend_target(&self) -> Option<String> {
        self.last
            .as_ref()
            .map(|l| format!("{} {}", l.short_id, l.summary))
    }

    /// Turns the amend switch on or off (v5 behaviour 13).
    pub fn set_amend(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(last) = self.last.clone() else {
            self.amend = false;
            cx.notify();
            return;
        };
        if on == self.amend {
            return;
        }
        self.amend = on;
        let typed = self.message(cx);
        let text = if on {
            Some(self.amend_message.turn_on(&typed, &last.message))
        } else {
            self.amend_message.turn_off(&typed, &last.message)
        };
        if let Some(text) = text {
            self.set_message(&text, cx);
        }
        cx.notify();
    }

    /// The author option (v5 behaviour 16).
    pub fn author(&self, cx: &gpui::App) -> String {
        self.author.read(cx).state.buffer.text()
    }

    /// Sets the author option.
    pub fn set_author(&mut self, text: &str, cx: &mut Context<Self>) {
        self.author.update(cx, |a, cx| a.set_text(text, cx));
    }

    /// Whether the options panel is open.
    pub fn options_open(&self) -> bool {
        self.options_open
    }

    /// Files waiting for the rollback confirmation, if it is shown.
    pub fn rollback_pending(&self) -> Option<&[String]> {
        self.rollback.as_deref()
    }

    /// Asks to roll back the checked changes, or the selected row (v5 behaviour 10).
    pub fn ask_rollback(&mut self, cx: &mut Context<Self>) {
        let targets = self.list.rollback_targets(self.current.as_deref());
        if targets.is_empty() {
            self.note = Some(("No changes to roll back".into(), false));
        } else {
            self.rollback = Some(targets);
            self.delete_added = false;
            // One strip at a time (`SPEC-0335`).
            self.list_strip = None;
        }
        cx.notify();
    }

    /// Ticks or unticks "Delete local copies of added files" in the confirmation.
    pub fn set_delete_added(&mut self, on: bool, cx: &mut Context<Self>) {
        self.delete_added = on;
        cx.notify();
    }

    /// Closes the confirmation without rolling back.
    pub fn cancel_rollback(&mut self, cx: &mut Context<Self>) {
        self.rollback = None;
        cx.notify();
    }

    /// Rolls back the confirmed files (v5 behaviour 10).
    pub fn confirm_rollback(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = self.rollback.take() else {
            return;
        };
        if let Some(host) = self.remote.clone() {
            // On the host; open documents follow through the session (`SPEC-0318` v5 behaviour 35).
            let params = serde_json::json!({
                "paths": paths, "delete_added": self.delete_added,
            });
            let task = cx
                .background_executor()
                .spawn(async move { host("git.rollback", params) });
            cx.spawn(async move |this, cx| {
                let done = task.await;
                let _ = this.update(cx, |this, cx| {
                    match done {
                        Ok(v) => {
                            let count: usize = ["restored", "unstaged", "deleted"]
                                .iter()
                                .map(|k| v[*k].as_array().map_or(0, Vec::len))
                                .sum();
                            this.note = Some((format!("Rolled back {count} file(s)"), true));
                            this.unsaved.retain(|u| !paths.contains(u));
                            this.refresh();
                        }
                        Err(e) => this.note = Some((e, false)),
                    }
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        let result = Repo::discover(&self.root).and_then(|r| {
            let workdir = r.workdir().map(Path::to_path_buf);
            r.rollback(&paths, self.delete_added, false)
                .map(|o| (o, workdir))
        });
        match result {
            Ok((outcome, workdir)) => {
                let count = outcome.restored.len() + outcome.unstaged.len() + outcome.deleted.len();
                self.note = Some((format!("Rolled back {count} file(s)"), true));
                self.unsaved.retain(|u| !paths.contains(u));
                if let Some(workdir) = workdir {
                    cx.emit(CommitEvent::RolledBack(
                        paths.iter().map(|p| workdir.join(p)).collect(),
                    ));
                }
                self.refresh();
            }
            Err(e) => self.note = Some((e.to_string(), false)),
        }
        cx.notify();
    }

    /// Presses a toolbar button by id (`refresh`, `rollback`, `diff`, `group`, `expand`,
    /// `collapse`), as a click does (v5 behaviour 9).
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "refresh" => self.refresh(),
            "rollback" => self.ask_rollback(cx),
            "diff" => {
                if let Some(path) = self.current.clone() {
                    cx.emit(CommitEvent::ShowDiff(path));
                }
            }
            "group" => self.group_menu = !self.group_menu,
            "changelists" => self.lists_menu = !self.lists_menu,
            "expand" => self.expand_all(cx),
            "collapse" => self.collapse_all(cx),
            "options" => self.options_open = !self.options_open,
            _ => {}
        }
        cx.notify();
    }

    /// Commits the checked files with the message, once the open files are saved (behaviour 5).
    pub fn commit_now(&mut self, cx: &mut Context<Self>) {
        if self.committing {
            return;
        }
        self.unsaved.clear();
        self.history = None;
        let message = self.message.read(cx).state.buffer.text();
        let paths = self.list.selected();
        // The message joins the history before the commit runs, so a commit a hook blocks or
        // Git refuses keeps it (v10 behaviour 22). The message itself is never logged or shown.
        if self.remote.is_none()
            && crate::commit_model::remembers_message(self.amend, paths.len())
            && crate::goto_model::refresh_local_state_ignore(&self.root)
            && let Err(e) =
                idecraft_core::commit_messages::remember_commit_message(&self.root, &message)
        {
            cx.emit(CommitEvent::Status(crate::commit_model::history_not_saved(
                &e,
            )));
        }
        let options = CommitOptions {
            verify: self.run_hooks,
            amend: self.amend,
            author: Some(self.author(cx)).filter(|a| !a.trim().is_empty()),
        };
        if let Some(host) = self.remote.clone() {
            // On the host, its hooks following the host's trust (`SPEC-0318` behaviour 5).
            self.committing = true;
            self.hook_output = None;
            let params = serde_json::json!({
                "message": message, "paths": paths, "amend": options.amend,
            });
            let task = cx
                .background_executor()
                .spawn(async move { host("git.commit", params) });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.committing = false;
                    let note = result
                        .as_ref()
                        .ok()
                        .and_then(|v| v["note"].as_str().map(str::to_owned));
                    let outcome = result
                        .and_then(|v| {
                            serde_json::from_value::<idecraft_vcs::CommitOutcome>(v)
                                .map_err(|e| e.to_string())
                        })
                        .map_err(idecraft_vcs::GitError::Git);
                    this.finish(outcome, cx);
                    if let (Some(note), Some((text, true))) = (note, &this.note) {
                        this.note = Some((format!("{text} ({note})"), true));
                    }
                    if let Some((text, _)) = &this.note {
                        cx.emit(CommitEvent::Committed(text.clone()));
                    }
                });
            })
            .detach();
            cx.notify();
            return;
        }
        if !self.hooks.is_empty() {
            // Hooks may take long: commit in the background (SPEC-0230 behaviour 7).
            self.committing = true;
            self.hook_output = None;
            let root = self.root.clone();
            let task = cx.background_executor().spawn(async move {
                Repo::discover(&root).and_then(|r| r.commit_opts(&message, &paths, false, &options))
            });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.committing = false;
                    this.finish(result, cx);
                    if let Some((text, _)) = &this.note {
                        cx.emit(CommitEvent::Committed(text.clone()));
                    }
                });
            })
            .detach();
            cx.notify();
            return;
        }
        let result = Repo::discover(&self.root)
            .and_then(|r| r.commit_opts(&message, &paths, false, &options));
        self.finish(result, cx);
    }

    /// Shows a commit's result; a blocked commit keeps message and checks (SPEC-0230 behaviour 3).
    fn finish(
        &mut self,
        result: Result<idecraft_vcs::CommitOutcome, idecraft_vcs::GitError>,
        cx: &mut Context<Self>,
    ) {
        let push = std::mem::take(&mut self.push_after);
        match result {
            Err(idecraft_vcs::GitError::HookFailed {
                hook,
                status,
                output,
            }) => {
                self.note = Some((format!("Commit blocked by {hook} hook ({status})"), false));
                self.hook_output = Some(output).filter(|o| !o.is_empty());
            }
            Ok(outcome) => {
                let summary = outcome
                    .commit
                    .map(|c| format!("{} — {}", &c.id[..7.min(c.id.len())], c.summary))
                    .unwrap_or_default();
                let what = if self.amend { "Amended" } else { "Committed" };
                self.note = Some((
                    format!("{what} {} file(s): {summary}", outcome.changes.len()),
                    true,
                ));
                let shown: Vec<String> = outcome
                    .hooks
                    .iter()
                    .filter(|h| !h.output.is_empty())
                    .map(|h| format!("{}:\n{}", h.hook, h.output))
                    .collect();
                self.hook_output = (!shown.is_empty()).then(|| shown.join("\n"));
                self.amend = false;
                self.amend_message.clear();
                self.message.update(cx, |m, cx| m.set_text("", cx));
                self.refresh();
                if push {
                    cx.emit(CommitEvent::Push);
                }
            }
            Err(e) => self.note = Some((e.to_string(), false)),
        }
        cx.notify();
    }

    /// A toolbar button; its tooltip is its declared control (`SPEC-0294` behaviour 9).
    fn tool(
        &self,
        control: &'static idecraft_ui::tooltip::Control,
        icon_key: &'static str,
        on: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let id = control
            .name
            .strip_prefix("commit-tool-")
            .unwrap_or(control.name);
        div()
            .id(control.name)
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .text_color(c(p.ui_muted))
            .when(on, |d| d.bg(c(p.tree_selection)))
            .hover(|s| s.bg(c(p.toolbar_hover)))
            .child(icon(icon_key))
            .tip(control)
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.press(id, cx);
            }))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let grouped = self.group_by != GroupBy::default();
        let mut bar = div().flex_none().flex().items_center().gap(px(2.)).px_2();
        for (control, key) in TOOLBAR_ICONS {
            let on = match control.name {
                "commit-tool-rollback" => self.rollback.is_some(),
                "commit-tool-group" => grouped || self.group_menu,
                _ => false,
            };
            bar = bar.child(self.tool(control, key, on, cx));
        }
        if self.remote.is_none() {
            bar = bar.child(self.tool(
                declared!(COMMIT_TOOLBAR, "commit-tool-changelists"),
                "general/moreVertical",
                self.lists_menu,
                cx,
            ));
        }
        bar
    }

    fn group_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let item = |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(24.))
                .rounded(px(4.))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(checkbox(Some(on)))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut g = this.group_by;
                    if id == "commit-group-directory" {
                        g.directory = !g.directory;
                    } else {
                        g.module = !g.module;
                    }
                    this.set_group_by(g, cx);
                }))
        };
        div()
            .mx_2()
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.popup_border))
            .bg(c(p.popup))
            .child(item(
                "commit-group-directory",
                "Directory",
                self.group_by.directory,
                cx,
            ))
            .child(item(
                "commit-group-module",
                "Module",
                self.group_by.module,
                cx,
            ))
    }

    /// The open list of Commit Message History, above the message field (v10 behaviour 22).
    fn history_list(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (rows, selected) = self.message_history()?;
        let p = Palette::current();
        Some(
            div()
                .id("commit-message-history")
                .flex_none()
                .max_h(px(24. * HISTORY_VISIBLE_ROWS as f32 + 8.))
                .overflow_y_scroll()
                .track_scroll(&self.history_scroll)
                .mx_2()
                .mb_1()
                .p_1()
                .flex()
                .flex_col()
                .rounded(px(6.))
                .border_1()
                .border_color(c(p.popup_border))
                .bg(c(p.popup))
                .children(rows.into_iter().enumerate().map(|(i, row)| {
                    div()
                        .id(("commit-message-history-row", i))
                        .flex_none()
                        .px_2()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .when(i == selected, |d| d.bg(c(p.tree_selection)))
                        .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .child(row)
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.choose_message(Some(i), cx);
                        }))
                })),
        )
    }

    fn chevron(&self, key: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let open = !self.folded.contains(key);
        let fold = key.to_owned();
        div()
            .id(gpui::SharedString::from(format!("fold-{key}")))
            .child(icon(if open {
                "general/chevronDown"
            } else {
                "general/chevronRight"
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_fold(&fold, cx);
            }))
    }

    fn row(&self, row: &Row, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let indent = px(4. + 16. * row.depth as f32);
        let count = row.paths.len();
        let count_text = format!("{count} file{}", if count == 1 { "" } else { "s" });
        match &row.kind {
            RowKind::Group { unversioned } => {
                let unversioned = *unversioned;
                div()
                    .id(gpui::SharedString::from(group_id(unversioned)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(p.tree_row_height))
                    .mx_2()
                    .pl(indent)
                    .rounded(px(4.))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(self.chevron(&row.key, cx))
                    .child(checkbox(self.list.group_checked(unversioned)))
                    .child(row.label.clone())
                    .child(div().text_color(c(p.ui_dim)).child(count_text))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.list.toggle_group(unversioned);
                        cx.notify();
                    }))
                    .into_any_element()
            }
            RowKind::Changelist { active } => self.changelist_row(row, *active, cx),
            RowKind::Module | RowKind::Directory => {
                let paths = row.paths.clone();
                let key = row.key.clone();
                div()
                    .id(gpui::SharedString::from(format!("node-{}", row.key)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(p.tree_row_height))
                    .mx_2()
                    .pl(indent)
                    .rounded(px(4.))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(self.chevron(&row.key, cx))
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("node-check-{}", row.key)))
                            .child(checkbox(self.list.paths_checked(&row.paths)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.list.toggle_paths(&paths);
                                cx.notify();
                            })),
                    )
                    .child(icon("nodes/folder"))
                    .child(row.label.clone())
                    .child(div().text_color(c(p.ui_dim)).child(count_text))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_fold(&key, cx)))
                    .into_any_element()
            }
            RowKind::File {
                kind,
                unversioned,
                dir,
            } => {
                let path = row.key.clone();
                let color = p
                    .scheme_color(status_color_key(*kind, *unversioned))
                    .unwrap_or(p.text);
                let checked = self.list.is_checked(&path);
                let toggle = path.clone();
                let chosen = path.clone();
                let current = self.current.as_deref() == Some(path.as_str());
                div()
                    .id(gpui::SharedString::from(format!("commit-{path}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(p.tree_row_height))
                    .mx_2()
                    .pl(indent + px(20.))
                    .rounded(px(4.))
                    .when(current, |d| d.bg(c(p.tree_selection)))
                    .when(!current, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("check-{path}")))
                            .child(checkbox(Some(checked)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.list.toggle(&toggle);
                                cx.notify();
                            })),
                    )
                    .child(icon(idecraft_ui::icons::file_icon(&row.label)))
                    .child(div().text_color(rgb(color)).child(row.label.clone()))
                    .when_some(dir.clone(), |d, dir| {
                        d.child(div().text_color(c(p.ui_dim)).child(dir))
                    })
                    .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                        // A click selects the row, a double-click shows its diff (SPEC-0084
                        // behaviour 1). A single click takes the focus into the list, so its keys
                        // (⌘D, F4, ⌘↓) reach it instead of the field or editor that had it.
                        if e.click_count() == 1 {
                            window.focus(&this.focus);
                        }
                        this.click(&chosen, e.click_count() > 1, cx);
                    }))
                    .into_any_element()
            }
        }
    }

    fn rollback_strip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let p = Palette::current();
        let count = self.rollback.as_ref()?.len();
        let delete_added = self.delete_added;
        Some(
            div()
                .id("commit-rollback-confirm")
                .flex_none()
                .mx_2()
                .mb_2()
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .rounded(px(6.))
                .border_1()
                .border_color(c(p.popup_border))
                .bg(c(p.popup))
                .child(format!(
                    "Roll back {count} file{} to the last commit? Local changes are lost.",
                    if count == 1 { "" } else { "s" }
                ))
                .child(
                    div()
                        .id("commit-rollback-delete-added")
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(checkbox(Some(delete_added)))
                        .child("Delete local copies of added files")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_delete_added(!delete_added, cx)
                        })),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            button("commit-rollback-ok", "Rollback", true)
                                .on_click(cx.listener(|this, _, _, cx| this.confirm_rollback(cx))),
                        )
                        .child(
                            button("commit-rollback-cancel", "Cancel", false)
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_rollback(cx))),
                        ),
                ),
        )
    }

    fn options_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let run_hooks = self.run_hooks;
        div()
            .id("commit-options")
            .flex_none()
            .mx_2()
            .mb_2()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.popup_border))
            .bg(c(p.popup))
            .when(!self.hooks.is_empty(), |d| {
                d.child(
                    div()
                        .id("commit-run-hooks")
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(checkbox(Some(run_hooks)))
                        .child("Run Git hooks")
                        .on_click(
                            cx.listener(|this, _, _, cx| this.set_run_hooks(!this.run_hooks, cx)),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_color(c(p.ui_muted)).child("Author"))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(px(24.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(p.separator))
                            .overflow_hidden()
                            .child(self.author.clone())
                            .when(self.author(cx).is_empty(), |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left(px(4.))
                                        .h(px(22.))
                                        .flex()
                                        .items_center()
                                        .text_color(c(p.ui_dim))
                                        .child("Name <email>"),
                                )
                            }),
                    ),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // F4 or ⌘↓ on a row opens the file in the editor (`EditSource`, v8); in the message
        // field ⌘↓ keeps moving the caret.
        let m0 = &event.keystroke.modifiers;
        let in_message = self.message.read(cx).focus().is_focused(window)
            || self.list_name.read(cx).focus().is_focused(window);
        let edit_source = (event.keystroke.key == "f4" && !m0.modified())
            || (event.keystroke.key == "down"
                && (if cfg!(target_os = "macos") {
                    m0.platform
                } else {
                    m0.control
                })
                && !m0.shift
                && !m0.alt);
        if edit_source
            && !in_message
            && let Some(path) = self.current.clone()
        {
            cx.stop_propagation();
            cx.emit(CommitEvent::OpenFile(path));
            return;
        }
        // ⌘↩ / Ctrl+↩ commits before the message field turns it into a line break (behaviour 5).
        let m = &event.keystroke.modifiers;
        let primary = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        // The open message history takes Up, Down, Enter and Escape (v10 behaviour 22).
        if self.history.is_some() && !m.modified() {
            match event.keystroke.key.as_str() {
                "up" | "down" => {
                    cx.stop_propagation();
                    self.step_message_history(event.keystroke.key == "down", cx);
                    return;
                }
                "enter" => {
                    cx.stop_propagation();
                    self.choose_message(None, cx);
                    return;
                }
                "escape" => {
                    cx.stop_propagation();
                    self.close_message_history(cx);
                    return;
                }
                _ => {}
            }
        }
        // The open strip of a changelist action takes Enter, Escape, Up and Down (`SPEC-0335`).
        // In the message field the keys stay the field's.
        let typing_message = self.message.read(cx).focus().is_focused(window);
        if self.list_strip.is_some() && !m.modified() && !typing_message {
            match event.keystroke.key.as_str() {
                "up" | "down" => {
                    cx.stop_propagation();
                    self.step_list_strip(event.keystroke.key == "down", cx);
                    return;
                }
                "enter" => {
                    cx.stop_propagation();
                    self.confirm_list_strip(window, cx);
                    return;
                }
                "escape" => {
                    cx.stop_propagation();
                    self.cancel_list_strip(window, cx);
                    return;
                }
                _ => {}
            }
        }
        if event.keystroke.key == "escape" && self.rollback.is_some() {
            cx.stop_propagation();
            self.cancel_rollback(cx);
            return;
        }
        if event.keystroke.key == "enter" && primary {
            cx.stop_propagation();
            self.commit(cx);
        }
        // ⌘D / Ctrl+D shows the diff of the selected row (`Diff.ShowDiff`, SPEC-0084).
        if event.keystroke.key == "d"
            && primary
            && !m.shift
            && !m.alt
            && let Some(path) = self.current.clone()
        {
            cx.stop_propagation();
            cx.emit(CommitEvent::ShowDiff(path));
        }
    }
}

fn group_id(unversioned: bool) -> &'static str {
    crate::commit_model::group_title(unversioned)
}

/// A push button: the accent one or a plain one.
fn button(id: &'static str, label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
    let p = Palette::current();
    div()
        .id(id)
        .px_3()
        .h(px(28.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .when(primary, |d| d.bg(c(p.accent)).text_color(c(p.accent_text)))
        .when(!primary, |d| {
            d.border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.toolbar_hover)))
        })
        .child(label)
}

/// A checkbox: checked, unchecked, or `None` for a group without files.
fn checkbox(state: Option<bool>) -> gpui::Div {
    let p = Palette::current();
    let on = state == Some(true);
    div()
        .size(px(14.))
        .flex_none()
        .rounded(px(3.))
        .border_1()
        .border_color(if on { c(p.accent) } else { c(p.ui_dim) })
        .when(on, |d| d.bg(c(p.accent)))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .text_color(c(p.accent_text))
        .when(on, |d| d.child("✓"))
}

impl gpui::EventEmitter<CommitEvent> for CommitView {}

impl gpui::Focusable for CommitView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CommitView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let rows: Vec<gpui::AnyElement> = self.rows().iter().map(|r| self.row(r, cx)).collect();
        let empty = rows.is_empty();
        let can_commit = (self.amend || !self.list.selected().is_empty()) && !self.committing;
        let placeholder = self.placeholder_shown(cx);
        let amend = self.amend;
        let has_last = self.last.is_some();
        let amend_target = self.amend_target();
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_key_down))
            // The title is in the shared header (SPEC-0061 v5 behaviour 18).
            .child(self.toolbar(cx))
            .when(self.group_menu, |d| d.child(self.group_menu(cx)))
            .when(self.lists_menu, |d| d.child(self.lists_menu(cx)))
            .child(
                div()
                    .id("commit-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt_1()
                    .when(empty, |d| {
                        d.child(div().px_3().text_color(c(p.ui_dim)).child("No changes"))
                    })
                    .children(rows),
            )
            .when_some(self.rollback_strip(cx), |d, s| d.child(s))
            .when_some(self.list_strip_element(cx), |d, s| d.child(s))
            .child(
                div()
                    .id("commit-amend")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .pb_1()
                    .overflow_hidden()
                    .when(!has_last, |d| d.opacity(0.5))
                    .child(checkbox(Some(amend)))
                    .child(div().flex_none().child("Amend last commit"))
                    .when_some(amend_target, |d, t| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_color(c(p.ui_dim))
                                .child(t),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_amend(!amend, cx))),
            )
            .when_some(self.history_list(cx), |d, list| d.child(list))
            .child(
                div()
                    .relative()
                    .h(px(120.))
                    .flex_none()
                    .mx_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c(p.separator))
                    .overflow_hidden()
                    .child(self.message.clone())
                    .when(placeholder, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .left(px(6.))
                                .text_color(c(p.ui_dim))
                                .child(MESSAGE_PLACEHOLDER),
                        )
                    }),
            )
            .when_some(self.hook_output.clone(), |d, out| {
                d.child(
                    div()
                        .id("commit-hook-output")
                        .flex_none()
                        .max_h(px(140.))
                        .overflow_y_scroll()
                        .mx_2()
                        .mt_2()
                        .p_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(c(p.separator))
                        .font_family(crate::fonts::editor_family())
                        .text_size(px(12.))
                        .text_color(c(p.ui_muted))
                        .children(
                            out.lines()
                                .map(|l| div().child(l.to_owned()))
                                .collect::<Vec<_>>(),
                        ),
                )
            })
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .child(
                        button(
                            "commit-button",
                            if self.committing {
                                "Committing…"
                            } else if amend {
                                "Amend Commit"
                            } else {
                                "Commit"
                            },
                            true,
                        )
                        .when(!can_commit, |d| d.opacity(0.5))
                        .on_click(cx.listener(|this, _, _, cx| this.commit(cx))),
                    )
                    .child(
                        button("commit-and-push-button", "Commit and Push…", false)
                            .when(!can_commit, |d| d.opacity(0.5))
                            .on_click(cx.listener(|this, _, _, cx| this.commit_and_push(cx))),
                    )
                    .child(div().flex_1())
                    .child(self.tool(
                        declared!(COMMIT_TOOLBAR, "commit-tool-options"),
                        "general/settings",
                        self.options_open,
                        cx,
                    )),
            )
            .when(self.options_open, |d| d.child(self.options_panel(cx)))
            .when_some(self.note.clone(), |d, (text, ok)| {
                d.child(
                    div()
                        .flex_none()
                        .px_3()
                        .pb_2()
                        .overflow_hidden()
                        .text_color(if ok { c(p.ui_muted) } else { c(p.error_text) })
                        .child(text),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0063 v7 AK-9: every toolbar button shows a bundled icon in both themes, in order.
    #[test]
    fn toolbar_buttons_have_bundled_icons() {
        let names: Vec<&str> = TOOLBAR_ICONS.iter().map(|(c, _)| c.name).collect();
        assert_eq!(
            names,
            [
                "commit-tool-refresh",
                "commit-tool-rollback",
                "commit-tool-diff",
                "commit-tool-group",
                "commit-tool-expand",
                "commit-tool-collapse"
            ]
        );
        for (_, key) in TOOLBAR_ICONS.iter().chain(
            [(
                declared!(COMMIT_TOOLBAR, "commit-tool-options"),
                "general/settings",
            )]
            .iter(),
        ) {
            for dark in [false, true] {
                assert!(idecraft_ui::icons::svg(key, dark).is_some(), "{key}");
            }
        }
    }
}
