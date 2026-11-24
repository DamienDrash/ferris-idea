//! Changes of a file in the workbench: the change bar of a gutter marker (`SPEC-0064` v3
//! behaviours 6–11), the authors of declarations (`SPEC-0292` v4 behaviour 7), the views of a file
//! in both editor groups (`SPEC-0112` v4 behaviour 9), the file items of the Git submenu
//! (`SPEC-0275` v8 behaviour 14c) and taking a change back from the diff viewer (`SPEC-0084` v2
//! behaviour 9).
//!
//! What a click hits and what the bar shows is in [`crate::change_bar_model`]; the hints'
//! bookkeeping is in [`crate::authors_model`].

use std::path::{Path, PathBuf};

use ferris_idea_vcs::{LineChange, Repo};
use gpui::{Context, Entity, Pixels, Point, Window, div, prelude::*, px};

use super::WorkbenchView;
use crate::EditorView;
use crate::change_bar_model;
use crate::palette::Palette;
use crate::workbench_view::c;

/// Markers and author labels of a second view (self-tests).
pub type SplitDecorations = (Vec<LineChange>, Vec<(usize, String)>);

/// Declarations read per file at most (`SPEC-0292` v4 behaviour 7a).
const MAX_DECLARATIONS: usize = 200;

/// The open change bar.
pub(super) struct ChangeBar {
    path: PathBuf,
    editor: Entity<EditorView>,
    index: usize,
    at: Point<Pixels>,
    /// The commit message field (behaviour 8).
    message: Entity<EditorView>,
    /// Why the last action was refused.
    note: Option<String>,
}

/// The file `path` relative to its repository's working tree, and the repository.
fn repo_path(path: &Path) -> Option<(Repo, String)> {
    let repo = Repo::discover(path.parent().unwrap_or(path)).ok()?;
    let workdir = repo.workdir()?.canonicalize().ok()?;
    let canonical = path.canonicalize().unwrap_or(path.to_path_buf());
    let rel = canonical
        .strip_prefix(&workdir)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some((repo, rel))
}

/// The authors of the declarations of `text`, the content of `path` (behaviours 7a–7b); empty
/// outside Git, for an untracked file and without a grammar.
pub(super) fn authors_of(path: &Path, text: &str) -> Vec<ferris_idea_vcs::CodeAuthor> {
    let spans = ferris_idea_languages::syntax::declaration_spans_of(path, text, MAX_DECLARATIONS);
    if spans.is_empty() {
        return Vec::new();
    }
    let Ok(blame) = super::annotate::blame_of(path) else {
        return Vec::new();
    };
    let declarations: Vec<ferris_idea_vcs::DeclarationLines> = spans
        .into_iter()
        .map(|s| ferris_idea_vcs::DeclarationLines {
            name: s.name,
            kind: s.kind,
            line: s.line,
            start: s.start_line,
            end: s.end_line,
        })
        .collect();
    ferris_idea_vcs::declaration_authors(&blame, text, &declarations)
}

impl WorkbenchView {
    /// The editor views of `path`: the main group's and the other groups' (`SPEC-0112` v4, v5).
    pub(super) fn views_of(&self, path: &Path) -> Vec<Entity<EditorView>> {
        self.editors
            .get(path)
            .cloned()
            .into_iter()
            .chain(self.twins(path))
            .collect()
    }

    /// Gives the other groups' views of `path` what the main view shows besides the text
    /// (`SPEC-0112` v4 behaviour 9).
    pub(super) fn sync_twin(&self, path: &Path, cx: &mut Context<Self>) {
        let Some(main) = self.editors.get(path).cloned() else {
            return;
        };
        for twin in self.twins(path) {
            let d = main.read(cx).decorations();
            twin.update(cx, |t, cx| t.set_decorations(d, cx));
        }
    }

    /// Reads the authors of the declarations of `path` in the background and hands them to its
    /// views (`SPEC-0292` v4 behaviour 7d).
    pub(super) fn request_code_authors(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !crate::appearance::ide_settings(cx).editor.code_authors || self.remote_project.is_some()
        {
            return;
        }
        let Some(editor) = self.editors.get(path) else {
            return;
        };
        if ferris_idea_languages::LanguageId::detect(path).is_none() {
            return;
        }
        let (text, revision) = {
            let e = editor.read(cx);
            (e.state.buffer.text(), e.revision())
        };
        let file = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn({
                    let file = file.clone();
                    async move { authors_of(&file, &text) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !crate::appearance::ide_settings(cx).editor.code_authors {
                    return;
                }
                if let Some(editor) = this.editors.get(&file).cloned() {
                    let taken = editor.update(cx, |e, cx| e.set_code_authors(revision, &found, cx));
                    if taken {
                        this.sync_twin(&file, cx);
                    } else {
                        // The text changed meanwhile: ask again for the current one.
                        this.request_code_authors(&file, cx);
                    }
                }
            });
        })
        .detach();
    }

    /// Follows the *Show code author* setting: reads or forgets the hints of every open file
    /// (`SPEC-0292` v4 behaviour 7f).
    pub(super) fn code_authors_setting(&mut self, cx: &mut Context<Self>) {
        let on = crate::appearance::ide_settings(cx).editor.code_authors;
        if on == self.authors_on {
            return;
        }
        self.authors_on = on;
        let open: Vec<PathBuf> = self.editors.keys().cloned().collect();
        for path in open {
            if on {
                self.request_code_authors(&path, cx);
            } else {
                for view in self.views_of(&path) {
                    view.update(cx, |e, cx| e.clear_code_authors(cx));
                }
            }
        }
    }

    /// Shows or hides the annotations of the editor view `editor` of `path`, as a click on an
    /// author hint does (`SPEC-0292` v4 behaviour 7e).
    pub(super) fn toggle_annotations_of(
        &mut self,
        path: &Path,
        editor: &Entity<EditorView>,
        cx: &mut Context<Self>,
    ) {
        if editor.read(cx).annotated() {
            editor.update(cx, |e, cx| e.set_annotations(None, cx));
            cx.notify();
        } else {
            self.annotate_view(path.to_path_buf(), editor, cx);
        }
    }

    /// Opens the change bar of change `index` of `editor` at window position `at` (behaviour 6).
    pub(super) fn open_change_bar(
        &mut self,
        path: &Path,
        editor: &Entity<EditorView>,
        index: usize,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if editor.read(cx).changes().get(index).is_none() {
            return;
        }
        self.hide_hover_cards(cx);
        self.context_menu = None;
        let message = match self.change_bar.take() {
            Some(bar) => bar.message,
            None => cx.new(EditorView::field),
        };
        // The message field takes the keys, so Escape and Enter reach the bar.
        window.focus(&message.read(cx).focus());
        self.change_bar = Some(ChangeBar {
            path: path.to_path_buf(),
            editor: editor.clone(),
            index,
            at,
            message,
            note: None,
        });
        cx.notify();
    }

    /// A file tab of the main group comes to the front, however it got there (a click on its
    /// tab, Go to File, the Project tree, a navigation, the next or previous tab, the switcher):
    /// the diff viewer and its tab close (`SPEC-0084` v6 behaviour 6).
    pub(super) fn close_diff_for_file_tab(&mut self) {
        self.diff = None;
        self.diff_file = None;
        self.diff_sides = None;
        self.diff_asked = None;
    }

    /// The diff viewer's tab at the end of the tab row (`SPEC-0084` v2 behaviour 6).
    pub(super) fn diff_tab(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let title = self.diff.as_ref()?.read(cx).title().to_owned();
        Some(
            crate::metrics::tab("diff-tab", true, true)
                .pr(px(ferris_idea_ui::metrics::TAB_CLOSE_PADDING - 1.))
                .children(crate::hit_targets::mark("diff-tab"))
                .child(crate::metrics::tab_content(
                    div().text_color(c(Palette::current().ui_muted)).child("⇄"),
                    div().child(title),
                ))
                .child(
                    div()
                        .id("diff-tab-close")
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .hover(|s| s.bg(c(Palette::current().toolbar_hover)))
                        .child("×")
                        .on_click(cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.diff = None;
                            this.diff_file = None;
                            this.focus_active(window, cx);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    /// The branches to compare the active file with, as a chooser (`SPEC-0084` v2 behaviour 11).
    fn show_compare_branches(
        &mut self,
        branches: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view =
            cx.new(|cx| crate::ChoiceView::new("Compare with branch".into(), branches.clone(), cx));
        self._compare_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.compare_targets = None;
                this.focus_active(window, cx);
                if let crate::ChoiceEvent::Chosen(i) = *event
                    && let Some(branch) = branches.get(i)
                {
                    this.compare_with_branch(branch, window, cx);
                }
                cx.notify();
            },
        ));
        let focus = view.read(cx).focus();
        window.focus(&focus);
        self.close_on_focus_out(super::Popup::CompareTargets, &view, &focus, window, cx);
        self.compare_targets = Some(view);
    }

    /// Opens the change bar of change `index` of the active view, as a click on its marker does
    /// (self-tests and the window's own checks).
    pub fn show_change_bar(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            return;
        };
        let at = editor
            .read(cx)
            .change_bar_anchor(index)
            .unwrap_or_else(|| gpui::point(px(80.), px(120.)));
        self.open_change_bar(&path, &editor, index, at, window, cx);
    }

    /// The HEAD text the open change bar shows (self-tests).
    pub fn change_bar_old_text(&self, cx: &gpui::App) -> Option<String> {
        let bar = self.change_bar.as_ref()?;
        bar.editor.read(cx).change_old_text(bar.index)
    }

    /// Markers and author hints of the second group's view of `path` (self-tests, `SPEC-0112` v4).
    pub fn split_decorations_of(&self, path: &Path, cx: &gpui::App) -> Option<SplitDecorations> {
        let view = self.twins(path).into_iter().next()?;
        let view = view.read(cx);
        Some((view.changes().to_vec(), view.author_labels()))
    }

    /// Closes the change bar, if open.
    pub fn close_change_bar(&mut self, cx: &mut Context<Self>) {
        if self.change_bar.take().is_some() {
            cx.notify();
        }
    }

    /// Whether the change bar is open, with the change's index (self-tests).
    pub fn change_bar_index(&self) -> Option<usize> {
        self.change_bar.as_ref().map(|b| b.index)
    }

    /// Runs a button of the change bar: `previous`, `next`, `rollback`, `diff`, `copy`, `commit`
    /// (behaviours 7–9).
    pub fn change_bar_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self.change_bar.as_ref() else {
            return;
        };
        let (path, editor, index) = (bar.path.clone(), bar.editor.clone(), bar.index);
        let Some(change) = editor.read(cx).changes().get(index).cloned() else {
            self.close_change_bar(cx);
            return;
        };
        match action {
            "previous" | "next" => {
                let changes = editor.read(cx).changes().to_vec();
                if let Some(to) = change_bar_model::neighbour(&changes, index, action == "next") {
                    editor.update(cx, |e, cx| e.goto_change(to, cx));
                }
            }
            "rollback" => {
                if editor
                    .update(cx, |e, cx| e.revert_change(index, cx))
                    .is_some()
                {
                    self.status = Some(format!(
                        "Rolled back {}",
                        change_bar_model::describe(&change)
                    ));
                }
                self.change_bar = None;
                window.focus(&editor.read(cx).focus());
            }
            "diff" => {
                self.change_bar = None;
                if let Some((_, rel)) = repo_path(&path) {
                    self.show_diff(&rel, window, cx);
                    if let Some(diff) = self.diff.clone() {
                        diff.update(cx, |d, _| d.go_to_new_line(change.new_start));
                    }
                }
            }
            "copy" => {
                if let Some(old) = editor.read(cx).change_old_text(index)
                    && !old.is_empty()
                {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(old));
                    self.status = Some(format!(
                        "Copied the HEAD text of {}",
                        change_bar_model::describe(&change)
                    ));
                }
                self.change_bar = None;
            }
            "commit" => self.commit_change(&path, &editor, &change, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Commits only `change` of `path` with the bar's message (behaviour 8).
    fn commit_change(
        &mut self,
        path: &Path,
        editor: &Entity<EditorView>,
        change: &LineChange,
        cx: &mut Context<Self>,
    ) {
        let Some(bar) = self.change_bar.as_mut() else {
            return;
        };
        let message = bar.message.read(cx).state.buffer.text();
        let Some((repo, rel)) = repo_path(path) else {
            bar.note = Some("The file is not in a Git repository".into());
            return;
        };
        let (base, text) = {
            let e = editor.read(cx);
            (e.base_text().map(str::to_owned), e.state.buffer.text())
        };
        let Some(base) = base else {
            bar.note = Some("The file has no committed version".into());
            return;
        };
        let content = ferris_idea_vcs::apply_change(&base, &text, change);
        let options = ferris_idea_vcs::CommitOptions::default();
        match repo.commit_change(&message, &rel, &content, false, &options) {
            Ok(done) => {
                let short = done
                    .commit
                    .as_ref()
                    .map(|c| c.short_id.clone())
                    .unwrap_or_default();
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(rel);
                self.status = Some(format!("Committed 1 change of {name} as {short}"));
                self.change_bar = None;
                self.refresh_branch();
                self.reload_git_log(cx);
                self.refresh_annotations(None, cx);
                self.load_bases(vec![path.to_path_buf()], cx);
                if let Some(commit) = self.commit.clone() {
                    commit.update(cx, |c, cx| {
                        c.refresh();
                        cx.notify();
                    });
                }
            }
            Err(e) => bar.note = Some(e.to_string()),
        }
    }

    /// `VcsShowNextChangeMarker` / `VcsShowPrevChangeMarker`: the change after or before the caret
    /// with its bar (behaviour 7).
    pub(super) fn step_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let (changes, line) = {
            let e = editor.read(cx);
            let line = e.status().line.saturating_sub(1);
            (e.changes().to_vec(), line)
        };
        match change_bar_model::from_caret(&changes, line, forward) {
            Some(i) => editor.update(cx, |e, cx| e.goto_change(i, cx)),
            None => {
                self.status = Some(if changes.is_empty() {
                    "No changes in this file".into()
                } else if forward {
                    "No change after the caret".into()
                } else {
                    "No change before the caret".into()
                });
                cx.notify();
            }
        }
    }

    /// `Vcs.RollbackChangedLines`: takes back the change at the caret (behaviour 7). Without a
    /// change at the caret it does nothing and returns false, so ⌥⌘Z, which Rollback shares, opens
    /// Rollback of files instead (`SPEC-0109` v4 behaviour 12).
    pub(super) fn rollback_change_at_caret(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.active_editor() else {
            return false;
        };
        let (changes, line) = {
            let e = editor.read(cx);
            (e.changes().to_vec(), e.status().line.saturating_sub(1))
        };
        match ferris_idea_vcs::change_at_line(&changes, line) {
            Some(i) => {
                if let Some(change) = editor.update(cx, |e, cx| e.revert_change(i, cx)) {
                    self.status = Some(format!(
                        "Rolled back {}",
                        change_bar_model::describe(&change)
                    ));
                }
            }
            None => return false,
        }
        cx.notify();
        true
    }

    /// Hides the hover cards of every editor view (`SPEC-0061` v9 behaviour 48).
    pub(super) fn hide_hover_cards(&mut self, cx: &mut Context<Self>) {
        let views: Vec<Entity<EditorView>> = self
            .editors
            .values()
            .chain(self.groups.iter().flat_map(|s| s.editors.values()))
            .cloned()
            .collect();
        for v in views {
            v.update(cx, |e, cx| e.hide_annotation_hover(cx));
        }
    }

    /// The file items of the Git submenu (`SPEC-0275` v8 behaviour 14c): `commit-file`,
    /// `rollback`, `history`, `diff`, `compare-branch`, for the active file.
    pub(super) fn vcs_file_item(
        &mut self,
        item: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.active_path() else {
            self.status = Some("Open a file first".into());
            cx.notify();
            return;
        };
        let Some((repo, rel)) = repo_path(&path) else {
            self.status = Some(format!("{} is not inside a Git repository", path.display()));
            cx.notify();
            return;
        };
        let tracked = repo.head_text(&rel).ok().flatten().is_some()
            || repo
                .status()
                .ok()
                .is_some_and(|s| s.iter().any(|f| f.path == rel && f.staged.is_some()));
        if !tracked && item != "commit-file" {
            self.status = Some(format!("{rel} is not tracked by Git"));
            cx.notify();
            return;
        }
        match item {
            "commit-file" | "rollback" => {
                if !self.show_commit {
                    self.toggle_commit(window, cx);
                }
                if let Some(commit) = self.commit.clone() {
                    commit.update(cx, |c, cx| {
                        c.refresh();
                        if item == "commit-file" {
                            c.check_only(&[rel.as_str()], cx);
                        }
                        c.click(&rel, false, cx);
                        if item == "rollback" {
                            c.ask_rollback(cx);
                        }
                    });
                }
            }
            "history" => {
                if self.git_log.is_none() {
                    self.toggle_git_log(window, cx);
                }
                if let Some(log) = self.git_log.clone() {
                    log.update(cx, |l, cx| {
                        l.update_filter(|f| f.paths = vec![rel.clone()], cx)
                    });
                    window.focus(&log.read(cx).focus());
                }
            }
            "diff" => self.show_diff(&rel, window, cx),
            "compare-branch" => {
                let branches: Vec<String> = repo
                    .branches()
                    .map(|b| b.branches.into_iter().map(|b| b.name).collect())
                    .unwrap_or_default();
                if branches.is_empty() {
                    self.status = Some("No branches to compare with".into());
                } else {
                    self.compare_branch = Some((rel, branches.clone()));
                    self.show_compare_branches(branches, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// Shows the working copy of the file chosen for Compare with Branch… against `branch`
    /// (`SPEC-0084` v2 behaviour 11).
    pub(super) fn compare_with_branch(
        &mut self,
        branch: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((rel, _)) = self.compare_branch.take() else {
            return;
        };
        self.diff_revision_with_worktree(branch, branch, &rel, window, cx);
    }

    /// The change bar as an overlay below its change (behaviour 6).
    pub(super) fn change_bar_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let bar = self.change_bar.as_ref()?;
        let e = bar.editor.read(cx);
        let change = e.changes().get(bar.index)?.clone();
        let total = e.changes().len();
        let old = e.change_old_text(bar.index).unwrap_or_default();
        let lines = change_bar_model::shown_old_lines(&old, &change);
        // Syntax colours of the old lines and their changed words against the lines now in the
        // buffer, as in the diff viewer (SPEC-0064 v4 behaviour 12).
        let old_raw: Vec<&str> = old.split('\n').collect();
        let old_syntax = crate::diff_model::syntax_by_line(&bar.path, &old);
        let new_lines: Vec<Option<String>> =
            if change.kind == ferris_idea_vcs::LineChangeKind::Modified {
                (0..old_raw.len())
                    .map(|i| {
                        let line = change.new_start + i;
                        (line <= change.new_end)
                            .then(|| e.state.buffer.line(line - 1))
                            .flatten()
                    })
                    .collect()
            } else {
                Vec::new()
            };
        let p = Palette::current();
        let deleted = p.diff_deleted;
        let button = |id: &'static str, label: &str, enabled: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .px_2()
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .children(crate::hit_targets::mark(id))
                .when(enabled, |d| {
                    d.text_color(c(p.ui_text))
                        .cursor_pointer()
                        .hover(|s| s.bg(c(p.toolbar_hover)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            let action = id.strip_prefix("change-bar-").unwrap_or(id);
                            this.change_bar_action(action, window, cx);
                        }))
                })
                .when(!enabled, |d| d.text_color(c(p.ui_dim)))
                .child(label.to_owned())
        };
        let added = change.kind == ferris_idea_vcs::LineChangeKind::Added;
        let toolbar = div()
            .flex()
            .items_center()
            .gap_1()
            .child(button("change-bar-previous", "↑", bar.index > 0, cx))
            .child(button("change-bar-next", "↓", bar.index + 1 < total, cx))
            .child(div().w(px(1.)).h(px(16.)).bg(c(p.separator)))
            .child(button("change-bar-rollback", "↶ Rollback", true, cx))
            .child(button("change-bar-diff", "Show Diff", true, cx))
            .child(button("change-bar-copy", "Copy", !added, cx))
            .child(div().flex_1())
            .child(
                div()
                    .text_color(c(p.ui_muted))
                    .child(format!("{} of {total}", bar.index + 1)),
            );
        let old_box = (!lines.is_empty()).then(|| {
            div()
                .id("change-bar-old")
                .max_h(px(20. * 12.))
                .overflow_hidden()
                .bg(gpui::rgb(deleted))
                .rounded(px(4.))
                .px_2()
                .py_1()
                .font_family(crate::fonts::editor_family())
                .children(lines.into_iter().enumerate().map(|(i, l)| {
                    let row = div().h(px(20.)).whitespace_nowrap();
                    match old_raw.get(i) {
                        Some(raw) if i < change_bar_model::OLD_LINES_SHOWN => {
                            let words = new_lines
                                .get(i)
                                .and_then(|n| n.as_deref())
                                .and_then(|n| ferris_idea_vcs::changed_words(raw, n))
                                .map(|w| w.0)
                                .unwrap_or_default();
                            let kinds = old_syntax.get(i).cloned().unwrap_or_default();
                            row.child(crate::diff_view::side_text(
                                raw,
                                &kinds,
                                &words,
                                crate::diff_view::stronger(deleted),
                            ))
                        }
                        _ => row.child(l),
                    }
                }))
        });
        let commit_row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .id("change-bar-message")
                    .children(crate::hit_targets::mark("change-bar-message"))
                    .flex_1()
                    .h(px(24.))
                    .px_1()
                    .border_1()
                    .border_color(c(p.popup_border))
                    .rounded(px(4.))
                    .child(bar.message.clone()),
            )
            .child(button("change-bar-commit", "Commit", true, cx));
        Some(
            gpui::deferred(
                gpui::anchored().position(bar.at).snap_to_window().child(
                    div()
                        .id("change-bar")
                        .occlude()
                        .w(px(440.))
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .bg(c(p.popup))
                        .border_1()
                        .border_color(c(p.popup_border))
                        .rounded(px(6.))
                        .shadow_lg()
                        .text_color(c(p.ui_text))
                        .capture_key_down(cx.listener(
                            |this, ev: &gpui::KeyDownEvent, window, cx| {
                                match ev.keystroke.key.as_str() {
                                    "escape" => {
                                        this.close_change_bar(cx);
                                        this.focus_active(window, cx);
                                    }
                                    "enter" if !ev.keystroke.modifiers.shift => {
                                        this.change_bar_action("commit", window, cx)
                                    }
                                    _ => return,
                                }
                                cx.stop_propagation();
                            },
                        ))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.close_change_bar(cx);
                        }))
                        .child(toolbar)
                        .children(old_box)
                        .child(commit_row)
                        .children(
                            bar.note
                                .clone()
                                .map(|n| div().text_color(c(p.error_text)).child(n)),
                        ),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
