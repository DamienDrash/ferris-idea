//! Files and commits of another revision as popups (`SPEC-0086` v8 behaviour 17, `SPEC-0068` v6
//! behaviour 23): the files at a commit (Show Repository at Revision), the files that differ from
//! the working tree (Compare with Local, Show Diff with Working Tree) and the commits one branch
//! has and the current one lacks (Compare with). A chosen file opens the read-only diff viewer of
//! `SPEC-0084` v2; a chosen commit is selected in the Git log.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{Context, Window, prelude::*};
use idecraft_vcs::Repo;

use super::WorkbenchView;
use super::popups::Popup;

/// What choosing item `i` of a revision popup does.
pub(super) type OnChoose =
    Rc<dyn Fn(&mut WorkbenchView, usize, &mut Window, &mut Context<WorkbenchView>)>;

/// Rows of a revision popup shown at once; the rest scroll with the selection.
const ROWS: usize = 16;

/// The items of the Compare with list: commits the branch has (`+`) and those the current branch
/// has (`-`), newest first in each part, with their ids (`SPEC-0068` v6 behaviour 23).
pub fn compare_items(
    ahead: &[idecraft_vcs::CommitInfo],
    behind: &[idecraft_vcs::CommitInfo],
) -> (Vec<String>, Vec<String>) {
    let mut items = Vec::new();
    let mut ids = Vec::new();
    for (sign, commits) in [("+", ahead), ("-", behind)] {
        for c in commits {
            items.push(format!("{sign} {}  {}", c.short_id, c.summary));
            ids.push(c.id.clone());
        }
    }
    (items, ids)
}

impl WorkbenchView {
    /// A popup headed `title` listing `items`; `on_choose` runs for the chosen one.
    pub(super) fn revision_popup(
        &mut self,
        title: String,
        items: Vec<String>,
        on_choose: OnChoose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if items.is_empty() {
            self.status = Some(format!("{title}: nothing to show"));
            cx.notify();
            return;
        }
        let view = cx.new(|cx| {
            let mut v = crate::ChoiceView::new(title, items, cx);
            v.set_limit(ROWS);
            v
        });
        self._compare_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.compare_targets = None;
                // An open diff viewer lies over the editor: the focus goes back to it
                // (`SPEC-0084` v5 behaviour 22).
                match this.diff.clone() {
                    Some(diff) => window.focus(&diff.read(cx).focus()),
                    None => this.focus_active(window, cx),
                }
                if let crate::ChoiceEvent::Chosen(i) = *event {
                    on_choose(this, i, window, cx);
                }
                cx.notify();
            },
        ));
        let focus = view.read(cx).focus();
        window.focus(&focus);
        self.close_on_focus_out(Popup::CompareTargets, &view, &focus, window, cx);
        self.compare_targets = Some(view);
        cx.notify();
    }

    /// The files that differ between `rev` and the working tree; a chosen one opens the diff with
    /// `rev` (named `label`) on the left (Compare with Local, Show Diff with Working Tree).
    pub(super) fn revision_changes(
        &mut self,
        rev: &str,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changes = match Repo::discover(&self.root).and_then(|r| r.changes_against_worktree(rev))
        {
            Ok(c) => c,
            Err(e) => {
                self.status = Some(format!("Compare with {label} failed: {e}"));
                cx.notify();
                return;
            }
        };
        let items = changes
            .iter()
            .map(|c| format!("{}  {}", c.status, c.path))
            .collect();
        let paths: Vec<String> = changes.into_iter().map(|c| c.path).collect();
        let (rev, label) = (rev.to_owned(), label.to_owned());
        let title = format!("Differences between {label} and the working tree");
        let on_choose: OnChoose = Rc::new(move |this, i, window, cx| {
            if let Some(path) = paths.get(i) {
                this.diff_revision_with_worktree(&rev, &label, path, window, cx);
            }
        });
        self.revision_popup(title, items, on_choose, window, cx);
    }

    /// The working copy of `rel` against its text at `rev` (named `label` above the left side),
    /// read-only (`SPEC-0084` v2 behaviour 11; `SPEC-0086` v8 behaviour 17).
    pub(super) fn diff_revision_with_worktree(
        &mut self,
        rev: &str,
        label: &str,
        rel: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(repo) = Repo::discover(&self.root) else {
            return;
        };
        let Some(workdir) = repo.workdir().map(Path::to_path_buf) else {
            return;
        };
        let old = match repo.text_at(rev, rel) {
            Ok(t) => t.unwrap_or_default(),
            Err(e) => {
                self.status = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let file = workdir.join(rel);
        let new = file
            .canonicalize()
            .ok()
            .and_then(|f| self.editors.get(&f))
            .map(|e| e.read(cx).state.buffer.text())
            .or_else(|| std::fs::read_to_string(&file).ok())
            .unwrap_or_default();
        self.show_worktree_diff(file, label, rel, &old, &new, true, window, cx);
    }

    /// `rel` at the revision named `label` against the working copy, from the host's texts.
    pub(super) fn present_worktree_diff(
        &mut self,
        label: &str,
        rel: &str,
        old: &str,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let file = self.root.join(rel);
        self.show_worktree_diff(file, label, rel, old, new, false, window, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn show_worktree_diff(
        &mut self,
        file: PathBuf,
        label: &str,
        rel: &str,
        old: &str,
        new: &str,
        // The working copy is a file of this machine: it takes changes (`SPEC-0084` v5
        // behaviours 17 and 18).
        local: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = idecraft_vcs::side_by_side(old, new);
        let unified = self.diff_unified;
        let title = format!("{rel} ({label})");
        let label = label.to_owned();
        let view = cx.new(|cx| {
            let view = crate::DiffView::new(title, rows, cx)
                .with_sides(&label, "Working copy")
                .with_texts(&file, old, new)
                .with_unified(unified);
            if local { view.revertible() } else { view }
        });
        self.diff_file = None;
        self.present_diff(view, window, cx);
        if local {
            self.diff_sides = Some(crate::diff_actions::Sides {
                left: crate::diff_actions::Side::Fixed(old.to_owned()),
                right: crate::diff_actions::Side::File(file),
            });
        }
    }

    /// The files at `rev`; a chosen one shows its text at `rev` read-only (Show Repository at
    /// Revision, `SPEC-0086` v8 behaviour 17).
    pub(super) fn show_repository_at(
        &mut self,
        rev: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let short = rev[..rev.len().min(7)].to_owned();
        let files = match Repo::discover(&self.root).and_then(|r| r.files_at(rev)) {
            Ok(f) => f,
            Err(e) => {
                self.status = Some(format!("Show Repository at Revision failed: {e}"));
                cx.notify();
                return;
            }
        };
        let rev = rev.to_owned();
        let shown = files.clone();
        let on_choose: OnChoose = Rc::new(move |this, i, window, cx| {
            if let Some(path) = files.get(i) {
                this.show_file_at(&rev, path, window, cx);
            }
        });
        self.revision_popup(format!("Files at {short}"), shown, on_choose, window, cx);
    }

    /// The text of `rel` at `rev` in the diff viewer with that revision on both sides, so it shows
    /// the file without differences and cannot change anything.
    fn show_file_at(&mut self, rev: &str, rel: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = match Repo::discover(&self.root).and_then(|r| r.text_at(rev, rel)) {
            Ok(t) => t.unwrap_or_default(),
            Err(e) => {
                self.status = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        self.present_file_at(rev, rel, &text, window, cx);
    }

    /// The text of `rel` at `rev` read-only (also from the host).
    pub(super) fn present_file_at(
        &mut self,
        rev: &str,
        rel: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let short = rev[..rev.len().min(7)].to_owned();
        let rows = idecraft_vcs::side_by_side(text, text);
        let file = self.root.join(rel);
        let view = cx.new(|cx| {
            crate::DiffView::new(format!("{rel} @ {short}"), rows, cx)
                .with_sides(&short, &short)
                .with_texts(&file, text, text)
                .with_unified(true)
        });
        self.diff_file = None;
        self.present_diff(view, window, cx);
    }

    /// The commits `branch` has and the current branch lacks and the other way round; a chosen
    /// one is selected in the Git log (Compare with, `SPEC-0068` v6 behaviour 23).
    pub(super) fn compare_branch_commits(
        &mut self,
        branch: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (ahead, behind) = match Repo::discover(&self.root).and_then(|r| r.compare_with(branch))
        {
            Ok(c) => c,
            Err(e) => {
                self.status = Some(format!("Compare with failed: {e}"));
                cx.notify();
                return;
            }
        };
        let current = self.branch.clone().unwrap_or_else(|| "HEAD".into());
        let (items, ids) = compare_items(&ahead, &behind);
        let title = format!("{branch} (+) and {current} (-)");
        let on_choose: OnChoose = Rc::new(move |this, i, window, cx| {
            if let Some(id) = ids.get(i) {
                this.show_commit_in_log(id, window, cx);
            }
        });
        self.revision_popup(title, items, on_choose, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::compare_items;
    use idecraft_vcs::CommitInfo;

    fn commit(id: &str, summary: &str) -> CommitInfo {
        CommitInfo {
            id: id.repeat(40 / id.len()),
            short_id: id.repeat(7 / id.len().max(1)).chars().take(7).collect(),
            summary: summary.into(),
            author: String::new(),
            email: String::new(),
            time: 0,
            parents: Vec::new(),
            committer: None,
        }
    }

    #[test]
    fn compare_lists_the_branch_first_then_the_current_branch() {
        let (items, ids) = compare_items(&[commit("a", "theirs")], &[commit("b", "ours")]);
        assert_eq!(items, ["+ aaaaaaa  theirs", "- bbbbbbb  ours"]);
        assert_eq!(ids[1], "b".repeat(40));
        assert!(compare_items(&[], &[]).0.is_empty());
    }
}
