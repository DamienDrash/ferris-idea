//! The interactive rebase dialog (`SPEC-0145`): reorder the commits since a base, and pick,
//! reword, squash, fix up or drop each one; the selected commit's files, details and the diff of
//! a file on the right (v3 behaviour 7).

use std::path::PathBuf;

use std::ops::Range;

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, ScrollStrategy, UniformListScrollHandle,
    Window, div, prelude::*, px, rgb, uniform_list,
};
use idecraft_vcs::{
    CommitDetails, DiffRow, LineChangeKind, Repo, TodoAction, TodoEntry, WordRanges, change_starts,
    changed_words,
};

use crate::commit_model::status_color_key;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the dialog asks the workbench to do.
pub enum RebaseEvent {
    /// Rebase onto the base with these entries, in this order.
    Start(String, Vec<TodoEntry>),
    /// Close without rebasing.
    Close,
}

/// The commits since `onto`, oldest first, with an action each.
pub struct RebaseView {
    onto: String,
    /// How the title names the base (`git_ops_model::rebase_base_label`).
    base_label: String,
    entries: Vec<TodoEntry>,
    selected: usize,
    /// The message field while a reword is edited.
    message: gpui::Entity<crate::editor_view::EditorView>,
    editing: bool,
    note: Option<String>,
    /// The repository, to read the selected commit's details (`SPEC-0145` v3 behaviour 7).
    root: Option<PathBuf>,
    details: Option<CommitDetails>,
    /// Which commit's details are being read, so a late answer is dropped.
    asked: Option<String>,
    /// Keys go to the file list of the selected commit.
    in_files: bool,
    file: usize,
    /// Path and rows of the diff shown under the files.
    diff: Option<(String, Vec<DiffRow>)>,
    /// Syntax colours, changed words and change starts of the diff shown (v4: the look of the
    /// diff viewer of `SPEC-0084` v2, opened at the first change, F7/⇧F7 between changes).
    diff_look: DiffLook,
    diff_scroll: UniformListScrollHandle,
    focus: FocusHandle,
}

/// What the dialog's diff draws besides the rows.
#[derive(Default, Clone)]
struct DiffLook {
    syntax: (SideSyntax, SideSyntax),
    words: Vec<WordRanges>,
    starts: Vec<usize>,
    current: Option<usize>,
}

/// Syntax kinds per line of one side.
type SideSyntax = Vec<Vec<(Range<usize>, String)>>;

/// The text of one side of `rows`, each line at its number, for the syntax colours.
fn side_text_of(rows: &[DiffRow], left: bool) -> String {
    let mut lines: Vec<(usize, &str)> = rows
        .iter()
        .filter_map(|r| {
            if left {
                r.left.as_ref()
            } else {
                r.right.as_ref()
            }
        })
        .map(|(n, t)| (*n, t.as_str()))
        .collect();
    lines.sort_by_key(|l| l.0);
    let mut out = String::new();
    let mut next = 1;
    for (n, t) in lines {
        while next < n {
            out.push('\n');
            next += 1;
        }
        out.push_str(t);
        out.push('\n');
        next = n + 1;
    }
    out
}

impl EventEmitter<RebaseEvent> for RebaseView {}

impl RebaseView {
    /// A dialog for rebasing onto `onto`, named `base_label` in the title, with `entries` (all
    /// `pick`).
    pub fn new(
        onto: String,
        base_label: String,
        entries: Vec<TodoEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            onto,
            base_label,
            entries,
            selected: 0,
            message: cx.new(crate::editor_view::EditorView::field),
            editing: false,
            note: None,
            root: None,
            details: None,
            asked: None,
            in_files: false,
            file: 0,
            diff: None,
            diff_look: DiffLook::default(),
            diff_scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
        }
    }

    /// Reads the selected commit's files and details from the repository at `root` from now on
    /// (`SPEC-0145` v3 behaviour 7).
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.root = Some(root);
        self.load_details(cx);
    }

    /// Details of the selected commit, once read.
    pub fn details(&self) -> Option<&CommitDetails> {
        self.details.as_ref()
    }

    /// Path and rows of the diff shown, if one is open.
    pub fn diff(&self) -> Option<(&str, &[DiffRow])> {
        self.diff.as_ref().map(|(p, r)| (p.as_str(), r.as_slice()))
    }

    /// Whether the keys go to the file list.
    pub fn in_files(&self) -> bool {
        self.in_files
    }

    /// Reads the details of the selected commit in the background.
    fn load_details(&mut self, cx: &mut Context<Self>) {
        let (Some(root), Some(id)) = (
            self.root.clone(),
            self.entries.get(self.selected).map(|e| e.id.clone()),
        ) else {
            return;
        };
        if self.asked.as_deref() == Some(id.as_str()) {
            return;
        }
        self.asked = Some(id.clone());
        self.diff = None;
        self.file = 0;
        cx.spawn(async move |this, cx| {
            let wanted = id.clone();
            let read = cx
                .background_spawn(async move {
                    Repo::discover(&root).and_then(|r| r.commit_details(&id))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.asked.as_deref() == Some(wanted.as_str()) {
                    this.details = read.ok();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Shows the diff of file `i` of the selected commit under the files.
    pub fn show_file(&mut self, i: usize, cx: &mut Context<Self>) {
        let (Some(root), Some(d)) = (self.root.clone(), &self.details) else {
            return;
        };
        let Some(change) = d.changes.get(i) else {
            return;
        };
        self.file = i;
        let (id, path) = (d.info.id.clone(), change.path.clone());
        let rows = Repo::discover(&root)
            .and_then(|r| r.side_by_side_in_commit(&id, &path))
            .unwrap_or_default();
        let file = std::path::Path::new(&path);
        self.diff_look = DiffLook {
            syntax: (
                crate::diff_model::syntax_by_line(file, &side_text_of(&rows, true)),
                crate::diff_model::syntax_by_line(file, &side_text_of(&rows, false)),
            ),
            words: rows
                .iter()
                .map(|r| match (r.kind, &r.left, &r.right) {
                    (Some(LineChangeKind::Modified), Some(l), Some(rt)) => {
                        changed_words(&l.1, &rt.1).unwrap_or_default()
                    }
                    _ => (Vec::new(), Vec::new()),
                })
                .collect(),
            starts: change_starts(&rows),
            current: None,
        };
        self.diff = Some((path, rows));
        // Opens at the first change (v4).
        self.step_change(true);
        cx.notify();
    }

    /// Goes to the next (or previous) change of the diff shown, stopping at the ends (F7 / ⇧F7).
    pub fn step_change(&mut self, forward: bool) {
        let n = self.diff_look.starts.len();
        if n == 0 {
            return;
        }
        let next = match (self.diff_look.current, forward) {
            (None, _) => 0,
            (Some(i), true) => (i + 1).min(n - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        self.diff_look.current = Some(next);
        let row = self.diff_look.starts[next];
        self.diff_scroll
            .scroll_to_item(row.saturating_sub(3), ScrollStrategy::Top);
    }

    /// Index of the change of the dialog's diff shown last, if a diff is open.
    pub fn diff_change(&self) -> Option<usize> {
        self.diff.as_ref().and(self.diff_look.current)
    }

    /// Selects the entry of commit `id` and opens its message field, for Edit Commit Message from
    /// the Git log (`SPEC-0086` v7 behaviour 13).
    pub fn preset_reword(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.entries.iter().position(|e| e.id == id) {
            self.select(i, cx);
            self.set_action(TodoAction::Reword, window, cx);
        }
    }

    /// Focus handle of the dialog.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The dialog's title: the base as the commit the rebase starts from (hand test 71, D34).
    pub fn title(&self) -> String {
        format!("Interactive Rebase onto {}", self.base_label)
    }

    /// Whether the reword message field is open.
    pub fn editing(&self) -> bool {
        self.editing
    }

    /// The text in the reword message field.
    pub fn message_text(&self, cx: &gpui::App) -> String {
        self.message.read(cx).state.buffer.text()
    }

    /// The entries in their current order.
    pub fn entries(&self) -> &[TodoEntry] {
        &self.entries
    }

    /// The selected row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The note under the list (why Start is refused).
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Selects row `i`.
    pub fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = i.min(self.entries.len().saturating_sub(1));
        self.load_details(cx);
        cx.notify();
    }

    /// A click on row `i`: an open message is kept, the row is selected and the keys go to the
    /// list again (hand test 71, D33: after a click no key arrived).
    pub fn click_row(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing {
            self.finish_message(window, cx);
        }
        self.in_files = false;
        self.select(i, cx);
        window.focus(&self.focus);
    }

    /// Sets the selected commit's action; reword opens the message field with its subject.
    pub fn set_action(&mut self, action: TodoAction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.entries.get_mut(self.selected) else {
            return;
        };
        e.action = action;
        if action == TodoAction::Reword {
            let text = e.message.clone().unwrap_or_else(|| e.subject.clone());
            self.message.update(cx, |f, cx| f.set_text(&text, cx));
            self.editing = true;
            let f = self.message.read(cx).focus();
            window.focus(&f);
        } else {
            e.message = None;
        }
        self.note = None;
        cx.notify();
    }

    /// Sets the message of the reworded commit (tests).
    pub fn set_message(&mut self, text: &str, cx: &mut Context<Self>) {
        self.message.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Moves the selected commit up or down by one (⌥↑/⌥↓).
    pub fn move_selected(&mut self, down: bool, cx: &mut Context<Self>) {
        let i = self.selected;
        let j = if down { i + 1 } else { i.wrapping_sub(1) };
        if j < self.entries.len() {
            self.entries.swap(i, j);
            self.selected = j;
            cx.notify();
        }
    }

    fn finish_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.message.read(cx).state.buffer.text().trim().to_owned();
        if let Some(e) = self.entries.get_mut(self.selected) {
            e.message = (!text.is_empty()).then_some(text);
        }
        self.editing = false;
        window.focus(&self.focus);
        cx.notify();
    }

    /// Why the list cannot run, if it cannot (checked again by the rebase itself).
    pub fn problem(&self) -> Option<String> {
        if let Some(first) = self.entries.iter().find(|e| e.action != TodoAction::Drop)
            && matches!(first.action, TodoAction::Squash | TodoAction::Fixup)
        {
            return Some(format!(
                "{} has no earlier commit to {} into",
                first.short_id,
                first.action.word()
            ));
        }
        self.entries
            .iter()
            .find(|e| e.action == TodoAction::Reword && e.message.is_none())
            .map(|e| format!("reword of {} needs a message", e.short_id))
    }

    /// Starts the rebase unless the list is refused.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        match self.problem() {
            Some(p) => self.note = Some(p),
            None => cx.emit(RebaseEvent::Start(self.onto.clone(), self.entries.clone())),
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        // Keys are seen before the message field (capture phase): Enter and Tab end the message
        // instead of going into it, Escape leaves it (hand test 71, D19 pattern).
        if self.editing {
            match k.key.as_str() {
                "enter" | "tab" => self.finish_message(window, cx),
                "escape" => {
                    self.editing = false;
                    window.focus(&self.focus);
                }
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Tab moves to the selected commit's files; there ↑/↓ choose, Enter shows the diff,
        // Escape closes the diff and then goes back to the list (`SPEC-0145` v3 behaviour 7).
        let files = self.details.as_ref().map_or(0, |d| d.changes.len());
        if self.in_files {
            match k.key.as_str() {
                "down" => self.file = (self.file + 1).min(files.saturating_sub(1)),
                "up" => self.file = self.file.saturating_sub(1),
                "enter" => self.show_file(self.file, cx),
                "f7" if self.diff.is_some() => self.step_change(!m.shift),
                "escape" if self.diff.is_some() => self.diff = None,
                "escape" | "tab" => self.in_files = false,
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.key == "tab" && files > 0 {
            self.in_files = true;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Home and End go to the first and the last commit (`SPEC-0275` v19 behaviour 47); ⌥↑
        // and ⌥↓ move the selected commit here, so the keymap's chords are not taken.
        if let Some(to) =
            crate::list_nav::edge_with(k, self.entries.len(), crate::list_nav::Keys::HOME_END)
        {
            self.select(to, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let action = |key: &str| match key {
            "p" => Some(TodoAction::Pick),
            "r" => Some(TodoAction::Reword),
            "s" => Some(TodoAction::Squash),
            "f" => Some(TodoAction::Fixup),
            "d" => Some(TodoAction::Drop),
            _ => None,
        };
        match k.key.as_str() {
            "up" | "down" if m.alt => self.move_selected(k.key == "down", cx),
            "up" => self.select(self.selected.saturating_sub(1), cx),
            "down" => self.select(self.selected + 1, cx),
            "enter" => self.start(cx),
            "escape" => cx.emit(RebaseEvent::Close),
            key if !m.platform && !m.control && !m.alt && action(key).is_some() => {
                if let Some(a) = action(key) {
                    self.set_action(a, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for RebaseView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let editing = self.editing;
        div()
            .id("rebase-dialog")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(980.))
            .max_w_full()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.title()),
            )
            .child(
                div().flex().gap_3().child(
                div()
                    .id("rebase-entries")
                    .flex_1()
                    .min_w_0()
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .children(self.entries.iter().enumerate().map(|(i, e)| {
                        let subject = match (&e.action, &e.message) {
                            (TodoAction::Reword, Some(m)) => format!("{}  →  {m}", e.subject),
                            _ => e.subject.clone(),
                        };
                        div()
                            .id(("rebase-entry", i))
                            .h(px(24.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_3()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(e.action == TodoAction::Drop, |d| {
                                d.text_color(c(p.ui_dim)).line_through()
                            })
                            .child(div().w(px(56.)).text_color(c(p.ui_muted)).child(e.action.word()))
                            // The id stays readable on the selection (dim text vanished there).
                            .child(div().w(px(64.)).text_color(c(p.ui_muted)).child(e.short_id.clone()))
                            .child(subject)
                            .on_click(cx.listener(
                                move |this, _: &gpui::ClickEvent, window, cx| {
                                    this.click_row(i, window, cx)
                                },
                            ))
                    })),
                )
                .child(self.commit_pane(cx)),
            )
            .children(self.diff_pane(cx))
            .when(editing, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_color(c(p.ui_muted)).child("New message"))
                        .child(crate::metrics::field_frame().flex_1().child(self.message.clone())),
                )
            })
            .when_some(self.note.clone(), |d, n| {
                d.child(div().text_color(c(p.ui_muted)).child(n))
            })
            .child(div().text_xs().text_color(c(p.ui_dim)).child(
                "P pick · R reword · S squash · F fixup · D drop · ⌥↑/⌥↓ move · Tab files · Enter starts · Escape cancels",
            ))
    }
}

impl RebaseView {
    /// The selected commit's changed files and details (`SPEC-0145` v3 behaviour 7).
    fn commit_pane(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let Some(d) = &self.details else {
            return div()
                .w(px(380.))
                .flex_none()
                .text_color(c(p.ui_dim))
                .child(if self.root.is_some() {
                    "Reading the commit…"
                } else {
                    ""
                })
                .into_any_element();
        };
        let (current, active) = (self.file, self.in_files);
        div()
            .w(px(380.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id("rebase-files")
                    .max_h(px(200.))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(c(if active { p.focus } else { p.separator }))
                    .rounded(px(4.))
                    .children(d.changes.iter().enumerate().map(|(i, change)| {
                        let color = p
                            .scheme_color(status_color_key(change.kind, false))
                            .unwrap_or(p.text);
                        div()
                            .id(("rebase-file", i))
                            .h(px(22.))
                            .px_2()
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(rgb(color))
                            .when(active && i == current, |e| e.bg(c(p.tree_selection)))
                            .when(!(active && i == current), |e| {
                                e.hover(|s| s.bg(c(p.tree_hover)))
                            })
                            .child(change.path.clone())
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                                this.in_files = true;
                                window.focus(&this.focus);
                                this.show_file(i, cx);
                            }))
                    })),
            )
            .child(
                div()
                    .id("rebase-details")
                    .max_h(px(140.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .whitespace_normal()
                            .child(d.message.trim_end().to_owned()),
                    )
                    .child(div().text_color(c(p.ui_muted)).child(format!(
                        "{} {} <{}>",
                        d.info.short_id, d.info.author, d.info.email
                    )))
                    .when_some(d.info.committer.clone(), |e, by| {
                        e.child(
                            div()
                                .text_color(c(p.ui_muted))
                                .child(format!("committed by {by}")),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The diff of the chosen file, old and new side by side, under the list: syntax colours and
    /// changed words as in the diff viewer, opened at the first change (v4).
    fn diff_pane(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let (path, rows) = self.diff.as_ref()?;
        let p = Palette::current();
        let inserted = p.diff_inserted;
        let deleted = p.diff_deleted;
        let modified = p.diff_modified;
        let rows = rows.clone();
        let look = self.diff_look.clone();
        let current = look.current.and_then(|i| look.starts.get(i).copied());
        let selection = p.tab_selected_border;
        let dim = p.ui_dim;
        let list = uniform_list(
            "rebase-diff-rows",
            rows.len(),
            cx.processor(move |_this, range: Range<usize>, _window, _cx| {
                range
                    .map(|i| {
                        let r = &rows[i];
                        let kinds = |left: bool, n: usize| {
                            let side = if left { &look.syntax.0 } else { &look.syntax.1 };
                            n.checked_sub(1)
                                .and_then(|k| side.get(k))
                                .cloned()
                                .unwrap_or_default()
                        };
                        let cell = |side: &Option<(usize, String)>, old: bool| {
                            let tint = match (r.kind, side.is_some()) {
                                (Some(LineChangeKind::Added), true) if !old => Some(inserted),
                                (Some(LineChangeKind::Deleted), true) if old => Some(deleted),
                                (Some(LineChangeKind::Modified), true) => Some(modified),
                                _ => None,
                            };
                            let words = look
                                .words
                                .get(i)
                                .map(|w| if old { w.0.clone() } else { w.1.clone() })
                                .unwrap_or_default();
                            let word_bg = if old { deleted } else { inserted };
                            let (n, text) = match side {
                                Some((n, t)) => (
                                    n.to_string(),
                                    crate::diff_view::side_text(
                                        t,
                                        &kinds(old, *n),
                                        &words,
                                        word_bg,
                                    ),
                                ),
                                None => (String::new(), div().into_any_element()),
                            };
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .gap_2()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .when_some(tint, |d, t| d.bg(rgb(t)))
                                .child(div().w(px(32.)).flex_none().text_color(c(dim)).child(n))
                                .child(text)
                        };
                        div()
                            .h(px(18.))
                            .w_full()
                            .flex()
                            .gap_2()
                            .when(current == Some(i), |d| {
                                d.border_t_1().border_color(c(selection))
                            })
                            .child(cell(&r.left, true))
                            .child(cell(&r.right, false))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.diff_scroll.clone())
        .w_full()
        .h(px(240.));
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(c(p.ui_muted)).child(format!(
                    "{path} — F7/⇧F7 next/previous change, Escape closes"
                )))
                .child(
                    div()
                        .id("rebase-diff")
                        .font_family(crate::fonts::editor_family())
                        .text_xs()
                        .border_1()
                        .border_color(c(p.separator))
                        .rounded(px(4.))
                        .child(list),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::side_text_of;
    use idecraft_vcs::{DiffRow, LineChangeKind};

    fn row(
        kind: Option<LineChangeKind>,
        left: Option<(usize, &str)>,
        right: Option<(usize, &str)>,
    ) -> DiffRow {
        DiffRow {
            kind,
            left: left.map(|(n, t)| (n, t.to_owned())),
            right: right.map(|(n, t)| (n, t.to_owned())),
        }
    }

    // SPEC-0145 v4 AK-6: each side's text keeps its line numbers for the syntax colours.
    #[test]
    fn each_side_keeps_its_line_numbers() {
        let rows = vec![
            row(None, Some((1, "a")), Some((1, "a"))),
            row(Some(LineChangeKind::Added), None, Some((2, "new"))),
            row(
                Some(LineChangeKind::Modified),
                Some((2, "b")),
                Some((3, "B")),
            ),
        ];
        assert_eq!(side_text_of(&rows, true), "a\nb\n");
        assert_eq!(side_text_of(&rows, false), "a\nnew\nB\n");
    }
}
