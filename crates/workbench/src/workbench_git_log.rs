//! The workbench side of the Git log's commit menu and branches action bar (`SPEC-0086` v7
//! behaviours 13 and 16): copying, the Git operations behind the entries, Edit Commit Message
//! through the interactive rebase dialog, checking out and fetching from the branches pane.

use ferris_idea_vcs::{CommitAction, Repo, ResetMode};
use gpui::{Context, Window};

use super::WorkbenchView;

impl WorkbenchView {
    /// Carries out an entry of the Git log's commit menu (`SPEC-0086` v7 behaviour 13).
    pub fn run_commit_action(
        &mut self,
        commit: String,
        action: CommitAction,
        mode: Option<ResetMode>,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let short = commit[..commit.len().min(7)].to_owned();
        // The host repository's actions run on the host (SPEC-0318 v5 behaviour 37).
        if self.remote_project.is_some() && !matches!(action, CommitAction::CopyRevision) {
            self.remote_commit_action(commit, action, mode, name, window, cx);
            return;
        }
        let root = self.root.clone();
        let repo = || Repo::discover(&root);
        match action {
            CommitAction::CopyRevision => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(commit.clone()));
                self.status = Some(format!("Copied revision {short}"));
            }
            CommitAction::CopyPatch => {
                self.status = Some(match repo().and_then(|r| r.commit_patch(&commit)) {
                    Ok(patch) => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(patch));
                        format!("Copied {short} as a patch")
                    }
                    Err(e) => format!("Copy as Patch failed: {e}"),
                });
            }
            CommitAction::ShowDiff => match repo().and_then(|r| r.commit_details(&commit)) {
                Ok(d) => match d.changes.first() {
                    Some(change) => {
                        let path = change.path.clone();
                        self.show_commit_diff(&commit, &path, window, cx);
                    }
                    None => self.status = Some(format!("{short} changes no file")),
                },
                Err(e) => self.status = Some(format!("Show Diff failed: {e}")),
            },
            CommitAction::CherryPick => {
                let label = format!("Cherry-pick {short}");
                self.git_operation(label, move |r| r.cherry_pick(&commit), cx);
            }
            CommitAction::Checkout => {
                let label = format!("Checkout {short}");
                self.git_operation(label, move |r| r.checkout_revision(&commit), cx);
            }
            CommitAction::Reset => {
                let mode = mode.unwrap_or(ResetMode::Mixed);
                let label = format!("Reset ({}) to {short}", mode.id());
                self.git_operation(label, move |r| r.reset_to(&commit, mode), cx);
            }
            CommitAction::Revert => {
                let label = format!("Revert {short}");
                self.git_operation(label, move |r| r.revert_commit(&commit), cx);
            }
            CommitAction::Undo => {
                let label = format!("Undo commit {short}");
                self.git_operation(label, move |r| r.undo_commit(&commit), cx);
            }
            CommitAction::Drop => {
                let label = format!("Drop {short}");
                self.git_operation(label, move |r| r.drop_commit(&commit), cx);
            }
            CommitAction::EditMessage => {
                // The interactive rebase dialog from the commit's parent with this commit set to
                // reword and its message field open.
                self.open_rebase(&format!("{commit}^"), window, cx);
                if let Some(dialog) = self.rebase.clone() {
                    dialog.update(cx, |d, cx| d.preset_reword(&commit, window, cx));
                }
            }
            CommitAction::RebaseFromHere => self.open_rebase(&format!("{commit}^"), window, cx),
            CommitAction::NewBranch | CommitAction::NewTag => {
                let name = name.unwrap_or_default();
                let tag = action == CommitAction::NewTag;
                let done = repo().and_then(|r| {
                    if tag {
                        r.create_tag(&name, &commit)
                    } else {
                        r.create_branch(&name, Some(&commit)).map(|_| ())
                    }
                });
                let kind = if tag { "tag" } else { "branch" };
                self.status = Some(match done {
                    Ok(()) => format!("Created {kind} {name} at {short}"),
                    Err(e) => format!("New {kind} failed: {e}"),
                });
                self.refresh_branch();
                self.reload_git_log(cx);
            }
            // v8 (SPEC-0086 behaviour 17).
            CommitAction::ShowRepository => self.show_repository_at(&commit, window, cx),
            CommitAction::CompareLocal => self.revision_changes(&commit, &short, window, cx),
            CommitAction::Fixup | CommitAction::SquashInto => {
                let squash = action == CommitAction::SquashInto;
                match repo().and_then(|r| r.fixup_message(&commit, squash)) {
                    Ok(message) => {
                        // ⌘K's way: shows the Commit window and never hides it.
                        self.run_action("CheckinProject", window, cx);
                        if let Some(view) = self.commit.clone() {
                            view.update(cx, |v, cx| v.set_message(&message, cx));
                        }
                        self.status =
                            Some(format!("Choose the files for \"{message}\" and commit"));
                    }
                    Err(e) => self.status = Some(format!("{} failed: {e}", action.label())),
                }
            }
            CommitAction::PushUpTo => self.open_push_of(None, Some(commit.clone()), window, cx),
            // The log window selects child and parent itself.
            CommitAction::GoToChild | CommitAction::GoToParent => {}
        }
        cx.notify();
    }

    /// Checks out a local branch from the Git log's branches pane; local changes in the way lead
    /// to the smart checkout question (`SPEC-0086` v7 behaviour 16, SPEC-0231).
    pub fn checkout_from_log(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_project.is_some() {
            self.branches_event(&crate::BranchesEvent::Checkout(name, false), window, cx);
            return;
        }
        let done = Repo::discover(&self.root).and_then(|r| r.checkout(&name, false));
        self.status = Some(match done {
            Ok(out) => format!("Checked out {}", out.branch),
            Err(ferris_idea_vcs::GitError::Rejected(m))
                if m.starts_with("local changes would be overwritten") =>
            {
                let files = m.split_once(": ").map(|(_, f)| f).unwrap_or_default();
                self.ask_smart(name.clone(), false, false, files, window, cx);
                m
            }
            Err(e) => format!("Checkout failed: {e}"),
        });
        self.refresh_branch();
        self.reload_git_log(cx);
        for editor in self.editors.values() {
            editor.update(cx, |e, cx| e.check_disk(cx));
        }
        cx.notify();
    }

    /// Fetches every remote from the Git log's branches pane; the log reads the moved remote
    /// branches by itself.
    pub fn fetch_from_log(&mut self, cx: &mut Context<Self>) {
        if self.remote_project.is_some() {
            self.remote_fetch(cx);
            return;
        }
        self.remote_operation(
            "Fetch".into(),
            |r| crate::vcs_ops_model::fetch_report(&r.fetch(None, ferris_idea_vcs::REMOTE_TIMEOUT)),
            cx,
        );
    }
}
