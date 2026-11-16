//! The local history view (`SPEC-0085`): the changes of a file on the left, the state before the
//! chosen one against the current text on the right; Revert brings that state back.

use std::ops::Range;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px,
    uniform_list,
};
use idecraft_core::Revision;

use crate::DiffView;
use crate::history_model::{before_of, label, matching, patch, stamp};
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// What the view asks the workbench to do.
pub enum HistoryEvent {
    /// Replace the current text with this text (the state before the chosen change).
    Revert(String),
    /// Put this patch on the clipboard (`SPEC-0085` v2 behaviour 11); empty without differences.
    Patch(String),
    /// Close the view.
    Close,
}

/// The local history of one file.
pub struct HistoryView {
    /// The file's path in the workspace.
    title: String,
    revisions: Vec<Revision>,
    /// Content of each revision (`None` for a deletion).
    contents: Vec<Option<String>>,
    current: String,
    selected: usize,
    /// The search text (behaviour 10).
    query: String,
    /// The entries the search lets through, newest first.
    shown: Vec<usize>,
    zone: jiff::tz::TimeZone,
    diff: Entity<DiffView>,
    focus: FocusHandle,
}

impl EventEmitter<HistoryEvent> for HistoryView {}

impl HistoryView {
    /// A view of `revisions` (newest first) with their `contents` against the `current` text.
    pub fn new(
        title: String,
        revisions: Vec<Revision>,
        contents: Vec<Option<String>>,
        current: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let zone = jiff::tz::TimeZone::system();
        let diff = Self::diff_of(&title, &revisions, &contents, &current, 0, &zone, cx);
        let shown = (0..revisions.len()).collect();
        Self {
            title,
            revisions,
            contents,
            current,
            selected: 0,
            query: String::new(),
            shown,
            zone,
            diff,
            focus: cx.focus_handle(),
        }
    }

    fn diff_of(
        title: &str,
        revisions: &[Revision],
        contents: &[Option<String>],
        current: &str,
        i: usize,
        zone: &jiff::tz::TimeZone,
        cx: &mut Context<Self>,
    ) -> Entity<DiffView> {
        let before = before_of(contents, i);
        let rows = idecraft_vcs::side_by_side(&before, current);
        let left = revisions
            .get(i)
            .map(|r| format!("Before {}", stamp(r.time, zone)))
            .unwrap_or_default();
        let title = title.to_owned();
        cx.new(|cx| {
            DiffView::new(title, rows, cx)
                .with_sides(&left, "Current")
                .with_filler_marks()
                .with_stepping()
        })
    }

    /// The view's title: "Local History: <file name>" (behaviour 7).
    pub fn heading(&self) -> String {
        let name = self.title.rsplit('/').next().unwrap_or(&self.title);
        format!("Local History: {name}")
    }

    /// Focus handle of the view.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Number of revisions listed.
    pub fn revision_count(&self) -> usize {
        self.revisions.len()
    }

    /// The rows the list shows, as their text (behaviour 8).
    pub fn rows(&self) -> Vec<String> {
        self.shown
            .iter()
            .map(|&i| label(&self.revisions[i], &self.zone))
            .collect()
    }

    /// The search text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The chosen entry (an index into the revisions, newest first).
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The comparison of the chosen entry.
    pub fn diff(&self) -> Entity<DiffView> {
        self.diff.clone()
    }

    /// Differences between the state before the chosen change and the current text.
    pub fn differences(&self, cx: &gpui::App) -> usize {
        self.diff.read(cx).differences()
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.revisions.len() {
            return;
        }
        self.selected = i;
        self.diff = Self::diff_of(
            &self.title,
            &self.revisions,
            &self.contents,
            &self.current,
            i,
            &self.zone,
            cx,
        );
    }

    /// Chooses the entry `by` rows below (above) the chosen one in the shown list.
    fn move_by(&mut self, down: bool, cx: &mut Context<Self>) {
        let at = self.shown.iter().position(|&i| i == self.selected);
        let to = match at {
            Some(at) if down => self.shown.get(at + 1),
            Some(at) => at.checked_sub(1).and_then(|a| self.shown.get(a)),
            None => self.shown.first(),
        };
        if let Some(&to) = to {
            self.select(to, cx);
        }
    }

    /// Narrows the list to the entries whose state before contains `query` (behaviour 10).
    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.query = query;
        self.shown = matching(&self.contents, &self.query);
        if !self.shown.contains(&self.selected)
            && let Some(&first) = self.shown.first()
        {
            self.select(first, cx);
        }
        cx.notify();
    }

    /// Takes text the platform's text input committed: it extends the search (behaviour 10).
    fn take_committed(&mut self, text: &str, cx: &mut Context<Self>) {
        let query = format!("{}{text}", self.query);
        self.set_query(query, cx);
    }

    /// Asks the workbench to put the state before the chosen change back (behaviour 9); nothing
    /// for a deletion.
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        let at = (self.selected + 1).min(self.contents.len().saturating_sub(1));
        if let Some(Some(text)) = self.contents.get(at) {
            cx.emit(HistoryEvent::Revert(text.clone()));
        }
    }

    /// Asks the workbench to put a patch from the state before the chosen change to the current
    /// text on the clipboard (behaviour 11).
    pub fn create_patch(&mut self, cx: &mut Context<Self>) {
        let before = before_of(&self.contents, self.selected);
        cx.emit(HistoryEvent::Patch(patch(
            &self.title,
            &before,
            &self.current,
        )));
    }

    /// Up/Down choose an entry, F7/⇧F7 step through its differences, typed text searches, ⌫
    /// shortens the search, Escape clears it and then closes (behaviours 3 and 10).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last entry (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.shown.len()) {
            if let Some(&to) = self.shown.get(to) {
                self.select(to, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        }
        // Typed text arrives through the platform's text input (`SPEC-0004` v6 behaviour 17).
        if crate::committed_text::leave_to_text_input(k.key_char.as_deref(), false) {
            return;
        }
        match (k.key.as_str(), k.modifiers.shift) {
            ("down", false) => self.move_by(true, cx),
            ("up", false) => self.move_by(false, cx),
            ("f7", shift) => {
                self.diff.update(cx, |d, _| d.step(!shift));
            }
            ("backspace", _) if !self.query.is_empty() => {
                let mut query = self.query.clone();
                query.pop();
                self.set_query(query, cx);
            }
            ("escape", false) if !self.query.is_empty() => self.set_query(String::new(), cx),
            ("escape", false) => cx.emit(HistoryEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

crate::committed_text::committed_text_input!(HistoryView, take_committed);

impl Render for HistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let rows: Vec<(usize, String, bool)> = self
            .shown
            .iter()
            .map(|&i| {
                let r = &self.revisions[i];
                (i, label(r, &self.zone), r.deleted)
            })
            .collect();
        let selected = self.selected;
        let empty = rows.is_empty();
        let list = uniform_list(
            "history-revisions",
            rows.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|k| {
                        let (i, text, deleted) = rows[k].clone();
                        div()
                            .id(("history-row", i))
                            .w_full()
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .px_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .when(deleted, |d| d.text_color(c(p.ui_dim)))
                            .child(text.clone())
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.select(i, cx);
                                cx.notify();
                            }))
                            .semantics(
                                Semantics::new(Role::ListItem)
                                    .name(text)
                                    .test_id(format!("history-row-{i}"))
                                    .selected(i == selected)
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .px_1()
        .flex_1();
        let button = |id: &'static str, label: &'static str, effect: Effect, primary: bool| {
            div()
                .id(id)
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(primary, |d| d.bg(c(p.accent)).text_color(c(p.accent_text)))
                .when(!primary, |d| d.hover(|s| s.bg(c(p.toolbar_hover))))
                .child(label)
                .semantics(
                    Semantics::new(Role::Button)
                        .name(label)
                        .test_id(id)
                        .action(AxAction::Click)
                        .effects(&[effect]),
                )
        };
        div()
            .id("history-view")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(280.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(c(p.separator))
                    .child(
                        div()
                            .h(px(36.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .px_3()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.heading())
                            .semantics(
                                Semantics::new(Role::Status)
                                    .name(self.heading())
                                    .test_id("history-title"),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .pb_1()
                            .child(
                                button("history-revert", "Revert", Effect::Write, true).on_click(
                                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                        this.revert(cx)
                                    }),
                                ),
                            )
                            .child(
                                button("history-patch", "Create Patch", Effect::None, false)
                                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                        this.create_patch(cx)
                                    })),
                            ),
                    )
                    .when(!self.query.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .mx_2()
                                .mb_1()
                                .px_2()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(c(p.accent))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(format!("Search: {}", self.query))
                                .semantics(
                                    Semantics::new(Role::TextInput)
                                        .name("Search")
                                        .value(self.query.clone())
                                        .test_id("history-search"),
                                ),
                        )
                    })
                    .when(empty, |d| {
                        d.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_color(c(p.ui_muted))
                                .child(format!("No entry contains {}", self.query))
                                .semantics(
                                    Semantics::new(Role::Status)
                                        .name(format!("No entry contains {}", self.query))
                                        .test_id("history-none"),
                                ),
                        )
                    })
                    .child(div().flex_1().min_h_0().flex().flex_col().child(list)),
            )
            .child(div().flex_1().min_w_0().child(self.diff.clone()))
    }
}
