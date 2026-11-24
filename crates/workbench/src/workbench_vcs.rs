//! Git operations of the workbench added with OBS-0022: the branch popup's v5 events
//! (`SPEC-0068` v5), the delete offer after a merge (v5 behaviour 22), the VCS operations popup's
//! file actions (`SPEC-0109` v2 behaviour 6), the stash dialog (`SPEC-0106` v3 behaviour 14), and
//! Update Project's reset and update information (`SPEC-0144` v4 behaviours 8, 9).

use std::path::{Path, PathBuf};

use ferris_idea_vcs::{OpOutcome, Repo, UpdateInfo};
use gpui::{AppContext, Context, Entity, Window};

use super::WorkbenchView;
use crate::BranchesEvent;

/// State of these operations in the workbench.
#[derive(Default)]
pub(super) struct GitUi {
    /// The stash dialog (`Git.Stash`).
    pub(super) stash_dialog: Option<Entity<crate::StashDialog>>,
    pub(super) _stash_dialog_events: Option<gpui::Subscription>,
    /// Stash messages typed in this session, oldest first.
    pub(super) stash_typed: Vec<String>,
    /// A merge from the popup stopped on conflicts: once it is committed, the branch may be
    /// deleted.
    pub(super) merged_waiting: Option<String>,
    /// Offer to delete this merged branch at the next frame.
    pub(super) offer_delete_next: Option<String>,
    /// Show this update information at the next frame.
    pub(super) update_info_next: Option<UpdateInfo>,
    /// The last update's information (`FerrisIDEA.ShowUpdateInfo`).
    pub(super) last_update_info: Option<UpdateInfo>,
}

impl WorkbenchView {
    /// The path of the active editor relative to the repository root.
    fn active_repo_path(&self) -> Option<String> {
        let path = self.active_path()?;
        let repo = Repo::discover(&self.root).ok()?;
        let workdir = repo.workdir()?.to_path_buf();
        let workdir = workdir.canonicalize().unwrap_or(workdir);
        let path = path.canonicalize().unwrap_or(path);
        path.strip_prefix(&workdir)
            .ok()
            .map(|r| r.to_string_lossy().replace('\\', "/"))
    }

    /// The branch popup's v5 events (`SPEC-0068` v5 behaviours 15, 16, 20); `true` when handled.
    pub(super) fn branch_popup_event(
        &mut self,
        event: &BranchesEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match event.clone() {
            BranchesEvent::Run(action) => {
                self.run_action(action, window, cx);
            }
            BranchesEvent::CheckoutRevision(rev) => {
                let label = format!("Checkout {rev}");
                self.git_operation(label, move |r| r.checkout_revision(&rev), cx);
            }
            BranchesEvent::CreateFrom(name, start) => {
                let label = format!("New branch {name} from {start}");
                self.git_operation(
                    label,
                    move |r| {
                        r.create_branch(&name, Some(&start))?;
                        let out = r.checkout(&name, false)?;
                        Ok(OpOutcome {
                            ok: true,
                            conflicts: Vec::new(),
                            in_progress: None,
                            output: format!("checked out {}", out.branch),
                        })
                    },
                    cx,
                );
            }
            BranchesEvent::CheckoutAndRebase(branch, onto) => {
                let label = format!("Rebase {branch} onto {onto}");
                self.git_operation(label, move |r| r.checkout_and_rebase(&branch, &onto), cx);
            }
            BranchesEvent::Rename(old, new) => {
                let label = format!("Rename {old} to {new}");
                // A favourite keeps its mark under the new name (SPEC-0068 v8 behaviour 31).
                let (root, key) = (self.root.clone(), self.favorites_key());
                self.git_operation(
                    label,
                    move |r| {
                        let out = r.rename_branch(&old, &new)?;
                        if let Some(key) = key {
                            let _ =
                                ferris_idea_core::rename_branch_favorite(&root, &key, &old, &new);
                        }
                        Ok(out)
                    },
                    cx,
                );
            }
            BranchesEvent::Delete(name) => {
                let label = format!("Delete branch {name}");
                self.git_operation(label, move |r| r.delete_branch(&name), cx);
            }
            BranchesEvent::CompareWith(name) => self.compare_branch_commits(&name, window, cx),
            BranchesEvent::DiffWorkingTree(name) => self.revision_changes(&name, &name, window, cx),
            BranchesEvent::PushBranch(name) => self.open_push_of(Some(name), None, window, cx),
            BranchesEvent::Favorite(name, remote, favorite) => {
                self.save_branch_favorite(&name, remote, favorite, cx)
            }
            _ => return false,
        }
        true
    }

    /// The key of the workspace's repository in the favourites store (`SPEC-0068` v8 behaviour
    /// 25); `None` outside a repository.
    pub(super) fn favorites_key(&self) -> Option<String> {
        let repo = Repo::discover(&self.root).ok()?;
        Some(ferris_idea_core::repo_key(&self.root, repo.workdir()?))
    }

    /// The favourite branches of the workspace's repository.
    pub(super) fn branch_favorites(&self) -> ferris_idea_core::RepoFavorites {
        let key = self.favorites_key().unwrap_or_else(|| ".".into());
        ferris_idea_core::BranchFavorites::load(&self.root).of(&key)
    }

    /// Saves a favourite marked or unmarked in the branch popup, the Git log's branches pane or a
    /// branch page of its commit menu, and lets the Git log show it (`SPEC-0068` v8, `SPEC-0086`
    /// v10 behaviour 19).
    pub(crate) fn save_branch_favorite(
        &mut self,
        name: &str,
        remote: bool,
        favorite: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.favorites_key() else {
            return;
        };
        self.status = Some(
            match ferris_idea_core::set_branch_favorite(&self.root, &key, name, remote, favorite) {
                Ok(true) => format!("{name} marked as favourite"),
                Ok(false) => format!("{name} unmarked as favourite"),
                Err(e) => format!("Could not save favourites: {e}"),
            },
        );
        if let Some(log) = self.git_log() {
            log.update(cx, |l, cx| l.reload_favorites(cx));
        }
        cx.notify();
    }

    /// After a merge from the popup: a clean merge offers to delete the branch at once, a merge
    /// that waits on conflicts once it is committed (`SPEC-0068` v5 behaviour 22, OBS-0022 L20).
    pub(super) fn after_branch_merge(&mut self, source: &str, done: &OpOutcome) {
        let local = Repo::discover(&self.root)
            .and_then(|r| r.branches())
            .is_ok_and(|b| b.branches.iter().any(|b| !b.remote && b.name == source));
        if !local {
            return;
        }
        if done.in_progress.is_some() || !done.conflicts.is_empty() {
            self.git_ui.merged_waiting = Some(source.to_owned());
        } else if done.ok {
            self.git_ui.offer_delete_next = Some(source.to_owned());
        }
    }

    /// Each frame: a waiting merge that is no longer in progress was committed (offer to delete
    /// the branch) or aborted (forget it); queued popups open.
    pub(super) fn git_ui_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.branch_state.operation.is_none()
            && let Some(source) = self.git_ui.merged_waiting.take()
            && Repo::discover(&self.root)
                .and_then(|r| r.is_merged(&source))
                .unwrap_or(false)
        {
            self.git_ui.offer_delete_next = Some(source);
        }
        if let Some(source) = self.git_ui.offer_delete_next.take() {
            cx.defer_in(window, move |this, window, cx| {
                this.offer_branch_delete(source, window, cx)
            });
        }
        if let Some(info) = self.git_ui.update_info_next.take() {
            cx.defer_in(window, move |this, window, cx| {
                this.show_update_info(info, window, cx)
            });
        }
    }

    /// Asks whether to delete the merged branch (`SPEC-0068` v5 behaviour 22).
    fn offer_branch_delete(&mut self, source: String, window: &mut Window, cx: &mut Context<Self>) {
        let into = self.branch.clone().unwrap_or_else(|| "HEAD".into());
        let title = format!("Merged '{source}' into '{into}'");
        self.show_jvm_popup(
            title,
            vec![
                format!("Delete branch '{source}'"),
                format!("Keep '{source}'"),
            ],
            window,
            cx,
            move |this, i, _window, cx| {
                if i == 0 {
                    let source = source.clone();
                    let label = format!("Delete branch {source}");
                    this.git_operation(label, move |r| r.delete_branch(&source), cx);
                }
            },
        );
    }

    /// The VCS operations popup's file actions (`SPEC-0109` v2 behaviour 6); `true` when `id` is
    /// one of them.
    pub(super) fn vcs_file_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match id {
            "Git.CopyBranchName" => {
                match self.branch.clone() {
                    Some(b) => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(b.clone()));
                        self.status = Some(format!("Copied branch name {b}"));
                    }
                    None => self.status = Some("HEAD is not on a branch".into()),
                }
                cx.notify();
                return true;
            }
            "FerrisIDEA.ShowUpdateInfo" => {
                match self.git_ui.last_update_info.clone() {
                    Some(info) => self.show_update_info(info, window, cx),
                    None => self.status = Some("No update in this session yet".into()),
                }
                cx.notify();
                return true;
            }
            "CheckinFiles"
            | "ChangesView.Revert"
            | "Vcs.ShowTabbedFileHistory"
            | "Compare.SameVersion"
            | "ChangesView.AddUnversioned" => {}
            _ => return false,
        }
        let Some(path) = self.active_repo_path() else {
            self.status = Some("Open a file of the repository first".into());
            cx.notify();
            return true;
        };
        match id {
            "CheckinFiles" => {
                let w = ferris_idea_ui::tool_windows::ToolWindow::Commit;
                if !self.tool_window_shown(w) {
                    self.toggle_tool_window(w, window, cx);
                }
                if let Some(view) = self.commit_view() {
                    view.update(cx, |v, cx| {
                        v.refresh();
                        v.check_only(&[path.as_str()], cx);
                    });
                    window.focus(&view.read(cx).focus(cx));
                }
            }
            // Every changed file, the current one checked, then a confirmation naming the number
            // (SPEC-0109 v4 behaviour 12, OBS-0119 K4).
            "ChangesView.Revert" => self.ask_rollback_files(Some(path), window, cx),
            "Vcs.ShowTabbedFileHistory" => {
                if self.git_log.is_none() {
                    self.toggle_git_log(window, cx);
                }
                if let Some(log) = self.git_log.clone() {
                    log.update(cx, |l, cx| {
                        l.update_filter(|f| f.paths = vec![path.clone()], cx)
                    });
                }
            }
            "Compare.SameVersion" => self.show_diff(&path, window, cx),
            "ChangesView.AddUnversioned" => {
                let done = Repo::discover(&self.root)
                    .and_then(|r| r.add_paths(std::slice::from_ref(&path)));
                self.status = Some(match done {
                    Ok(_) => format!("Added {path} to Git"),
                    Err(e) => format!("Add failed: {e}"),
                });
            }
            _ => {}
        }
        cx.notify();
        true
    }

    /// Rollback (`ChangesView.Revert`, ⌥⌘Z; `SPEC-0109` v4 behaviour 12): a list of the changed
    /// files with check boxes, `current` (repository-relative) checked; Enter asks to confirm with
    /// the number of files, and the confirmation rolls the checked files back.
    pub fn ask_rollback_files(
        &mut self,
        current: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = Repo::discover(&self.root).and_then(|r| r.status());
        let (rows, checked) = match status {
            Ok(status) => crate::vcs_ops_model::rollback_rows(&status, current.as_deref()),
            Err(e) => {
                self.status = Some(format!("Rollback failed: {e}"));
                cx.notify();
                return;
            }
        };
        if rows.is_empty() {
            self.status = Some("No changes to roll back".into());
            cx.notify();
            return;
        }
        let headings = vec![false; rows.len()];
        let items = rows.clone();
        let view = cx.new(|cx| {
            crate::ChoiceView::new("Rollback: check the files".into(), items, cx)
                .with_checks(checked, headings)
                .with_none_allowed()
        });
        self.jvm_popup_place = super::JvmPopupPlace::Top;
        self._jvm_popup_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.jvm_popup = None;
                this.focus_active(window, cx);
                if let crate::ChoiceEvent::ChosenMany(chosen) = event {
                    let paths: Vec<String> = chosen
                        .iter()
                        .filter_map(|i| rows.get(*i).cloned())
                        .collect();
                    if !paths.is_empty() {
                        this.confirm_rollback_files(paths, window, cx);
                    }
                }
                cx.notify();
            },
        ));
        let focus = view.read(cx).focus();
        window.focus(&focus);
        self.close_on_focus_out(super::popups::Popup::RunMarker, &view, &focus, window, cx);
        self.jvm_popup = Some(view);
        cx.notify();
    }

    /// The confirmation of Rollback for `paths`, Rollback first (behaviour 12).
    fn confirm_rollback_files(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = crate::vcs_ops_model::rollback_question(&paths);
        let button = crate::vcs_ops_model::rollback_button(paths.len());
        self.show_jvm_popup(
            title,
            vec![button, "Cancel".into()],
            window,
            cx,
            move |this, i, _window, cx| {
                if i == 0 {
                    this.rollback_files(&paths, cx);
                }
            },
        );
    }

    /// Rolls `paths` (repository-relative) back to HEAD: the Local History keeps their state
    /// before under the label "Before Rollback" and names the step "Rollback"; open editors show
    /// what the rollback wrote (behaviour 12).
    pub fn rollback_files(&mut self, paths: &[String], cx: &mut Context<Self>) {
        let repo = Repo::discover(&self.root);
        let workdir = repo
            .as_ref()
            .ok()
            .and_then(|r| r.workdir().map(Path::to_path_buf))
            .map(|w| w.canonicalize().unwrap_or(w));
        let files: Vec<PathBuf> = workdir
            .iter()
            .flat_map(|w| paths.iter().map(move |p| w.join(p)))
            .collect();
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        ferris_idea_languages::undo_group::record_history_before(&self.root, &refs, "Rollback");
        let done = repo.and_then(|r| r.rollback(paths, false, false));
        self.status = Some(match done {
            Ok(o) => {
                let n = o.restored.len() + o.unstaged.len() + o.deleted.len();
                let existing: Vec<&Path> = refs.iter().copied().filter(|p| p.exists()).collect();
                ferris_idea_languages::undo_group::record_history(
                    &self.root,
                    &existing,
                    Some("Rollback"),
                );
                for file in &files {
                    let file = file.canonicalize().unwrap_or(file.clone());
                    if let Some(editor) = self.editors.get(&file) {
                        editor.update(cx, |e, cx| e.reload_from_disk(cx));
                    }
                }
                self.refresh_annotations(None, cx);
                match n {
                    1 => "Rolled back 1 file".to_owned(),
                    n => format!("Rolled back {n} files"),
                }
            }
            Err(e) => format!("Rollback failed: {e}"),
        });
        cx.notify();
    }

    /// Opens the stash dialog (`Git.Stash`, `SPEC-0106` v3 behaviour 14).
    pub fn open_stash_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stashes = Repo::discover(&self.root)
            .and_then(|r| r.stashes())
            .unwrap_or_default();
        let history =
            crate::vcs_ops_model::stash_message_history(&self.git_ui.stash_typed, &stashes);
        let branch = self.branch.clone().unwrap_or_else(|| "HEAD".into());
        let view = cx.new(|cx| crate::StashDialog::new(branch, history, cx));
        self.git_ui._stash_dialog_events = Some(cx.subscribe_in(
            &view,
            window,
            |this, _, event: &crate::StashDialogEvent, window, cx| {
                this.git_ui.stash_dialog = None;
                if let crate::StashDialogEvent::Stash(options) = event.clone() {
                    if let Some(m) = &options.message {
                        this.git_ui.stash_typed.retain(|t| t != m);
                        this.git_ui.stash_typed.push(m.clone());
                    }
                    if this.remote_project.is_some() {
                        this.remote_stash_push(options, cx);
                    } else {
                        this.git_operation(
                            "Stash".into(),
                            move |r| r.stash_push_with(&options),
                            cx,
                        );
                    }
                }
                this.focus_active(window, cx);
                cx.notify();
            },
        ));
        window.focus(&view.read(cx).focus(cx));
        self.git_ui.stash_dialog = Some(view);
        cx.notify();
    }

    /// The stash dialog, if it is open.
    pub fn stash_dialog(&self) -> Option<Entity<crate::StashDialog>> {
        self.git_ui.stash_dialog.clone()
    }

    /// Edit actions of the menu bar while a dialog with a text field is open go to that field,
    /// never to the editor behind it (`SPEC-0109` v2 behaviour 8, OBS-0022 L23); `true` when
    /// taken.
    pub(super) fn dialog_edit_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(
            id,
            "$SelectAll" | "$Copy" | "$Cut" | "$Paste" | "$Undo" | "$Redo"
        ) {
            return false;
        }
        // Settings: the focused field or the search (`SPEC-0105` v8 behaviour 10, OBS-0062 S6).
        if let Some(settings) = self.settings_dialog() {
            return settings.update(cx, |s, cx| s.edit_action(id, window, cx));
        }
        if let Some(push) = self.push.clone() {
            push.update(cx, |p, cx| p.edit_action(id, window, cx));
            return true;
        }
        if let Some(dialog) = self.git_ui.stash_dialog.clone() {
            let field = dialog.read(cx).field();
            window.focus(&field.read(cx).focus());
            field.update(cx, |f, cx| {
                f.run_action(id, cx);
                cx.notify();
            });
            return true;
        }
        false
    }

    /// Update Project with the reset to the upstream (`SPEC-0144` v4 behaviour 9); the choice is
    /// never remembered (IMP-0322).
    pub fn reset_to_upstream(&mut self, cx: &mut Context<Self>) {
        if self.remote_project.is_some() {
            self.remote_reset_upstream(cx);
            return;
        }
        self.update_operation(
            "Reset".into(),
            |r| {
                let done = r.reset_to_upstream(false, ferris_idea_vcs::REMOTE_TIMEOUT);
                let info = done.as_ref().ok().and_then(|p| p.info.clone());
                (crate::vcs_ops_model::reset_report(&done), info)
            },
            cx,
        );
    }

    /// Runs an update in the background; the report goes to the status bar and what came in to
    /// the Update Info popup (`SPEC-0144` v4 behaviour 8).
    pub(super) fn update_operation(
        &mut self,
        label: String,
        op: impl FnOnce(&Repo) -> (String, Option<UpdateInfo>) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let root: PathBuf = self.root.clone();
        self.status = Some(format!("{label}…"));
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            match Repo::discover(&root) {
                Ok(r) => op(&r),
                Err(e) => (format!("{label} failed: {e}"), None),
            }
        });
        cx.spawn(async move |this, cx| {
            let (report, info) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(report);
                this.close_merge_next = this.merge.is_some();
                if let Some(info) = info.filter(|i| !i.commits.is_empty() || !i.files.is_empty()) {
                    this.git_ui.last_update_info = Some(info.clone());
                    this.git_ui.update_info_next = Some(info);
                }
                this.refresh_branch();
                this.reload_git_log(cx);
                for editor in this.editors.values() {
                    editor.update(cx, |e, cx| e.check_disk(cx));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The Update Info popup: commits and files that came in; Enter on a commit selects it in the
    /// Git log, on a file opens it (`SPEC-0144` v4 behaviour 8).
    pub fn show_update_info(
        &mut self,
        info: UpdateInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, rows) = crate::vcs_ops_model::update_info_rows(&info);
        let workdir = if self.remote_project.is_some() {
            Some(self.root.clone())
        } else {
            Repo::discover(&self.root)
                .ok()
                .and_then(|r| r.workdir().map(Path::to_path_buf))
        };
        self.show_jvm_popup(title, rows, window, cx, move |this, i, window, cx| {
            if let Some(c) = info.commits.get(i) {
                this.show_commit_in_log(&c.id, window, cx);
            } else if let Some(f) = info.files.get(i - info.commits.len())
                && let Some(dir) = workdir.as_ref()
            {
                let path = dir.join(&f.path);
                // A remote project's file is the host's: its document opens through the session.
                if this.remote_project.is_some() || path.is_file() {
                    this.open(&path, window, cx);
                } else {
                    this.status = Some(format!("{} was deleted by the update", f.path));
                }
            }
        });
    }
}
