//! The rename preview (`SPEC-0148` behaviour 3): the occurrences in comments, strings and text
//! files that a rename would also change, each one included or excluded before anything is written.
//! Code usages in files that are not the project's own sources (a virtual environment, installed
//! dependencies, ignored files) follow as rows of their own and start excluded (v3 behaviour 7).

use std::path::{Path, PathBuf};

use ferris_idea_languages::rename::RenamePlan;
use ferris_idea_languages::rename_text::{Occurrence, OccurrenceKind, OutsideFile};
use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::palette::Palette;
use crate::workbench_view::c;

/// What the preview asks the workbench to do.
pub enum RenamePreviewEvent {
    /// Rename the code usages, these occurrences and the usages in these files outside the
    /// project's own sources.
    Apply(RenamePlan, Vec<Occurrence>, Vec<OutsideFile>),
    /// Rename nothing.
    Close,
}

/// The preview.
pub struct RenamePreview {
    root: PathBuf,
    plan: RenamePlan,
    occurrences: Vec<Occurrence>,
    /// Files outside the project's own sources; their rows follow the occurrences.
    outside: Vec<OutsideFile>,
    /// Per row: the occurrences, then the files outside.
    included: Vec<bool>,
    selected: usize,
    focus: FocusHandle,
}

impl EventEmitter<RenamePreviewEvent> for RenamePreview {}

impl RenamePreview {
    /// A preview of `plan` (the code usages in the project's own sources), the extra
    /// `occurrences`, all included, and the code usages in files `outside` the own sources, none
    /// included.
    pub fn new(
        root: PathBuf,
        plan: RenamePlan,
        occurrences: Vec<Occurrence>,
        outside: Vec<OutsideFile>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut included = vec![true; occurrences.len()];
        included.resize(occurrences.len() + outside.len(), false);
        Self {
            root,
            plan,
            included,
            outside,
            occurrences,
            selected: 0,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the preview.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The extra occurrences and whether each one is included.
    pub fn rows(&self) -> Vec<(&Occurrence, bool)> {
        self.occurrences
            .iter()
            .zip(self.included.iter().copied())
            .collect()
    }

    /// The files outside the project's own sources and whether each one is included.
    pub fn outside_rows(&self) -> Vec<(&OutsideFile, bool)> {
        self.outside
            .iter()
            .zip(self.included[self.occurrences.len()..].iter().copied())
            .collect()
    }

    /// The selected row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Includes or excludes row `i` (Space, a click on its box).
    pub fn toggle(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(on) = self.included.get_mut(i) {
            *on = !*on;
            cx.notify();
        }
    }

    /// Renames the code usages and the included occurrences (Enter, "Do Refactor").
    pub fn apply(&mut self, cx: &mut Context<Self>) {
        let chosen = self
            .occurrences
            .iter()
            .zip(&self.included)
            .filter(|(_, on)| **on)
            .map(|(o, _)| o.clone())
            .collect();
        let outside = self
            .outside_rows()
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        cx.emit(RenamePreviewEvent::Apply(
            self.plan.clone(),
            chosen,
            outside,
        ));
    }

    fn relative<'a>(&self, path: &'a Path) -> std::borrow::Cow<'a, str> {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.included.len()) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        }
        let n = self.included.len();
        match k.key.as_str() {
            "down" if n > 0 => self.selected = (self.selected + 1) % n,
            "up" if n > 0 => self.selected = (self.selected + n - 1) % n,
            "space" => self.toggle(self.selected, cx),
            "enter" => self.apply(cx),
            "escape" => cx.emit(RenamePreviewEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for RenamePreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let usages: usize = self.plan.files.iter().map(|f| f.edits.len()).sum();
        let texts = self.occurrences.len();
        let chosen = self.included[..texts].iter().filter(|on| **on).count();
        let outside_chosen = self.included[texts..].iter().filter(|on| **on).count();
        let outside_rows: Vec<_> = self
            .outside
            .iter()
            .zip(&self.included[texts..])
            .enumerate()
            .map(|(n, (f, on))| {
                let i = texts + n;
                let on = *on;
                div()
                    .id(("rename-outside", n))
                    .h(px(24.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                    .when(!on, |d| d.text_color(c(p.ui_dim)))
                    .child(div().flex_none().child(if on { "☑" } else { "☐" }))
                    .child(
                        div()
                            .w(px(120.))
                            .flex_none()
                            .text_color(c(p.ui_muted))
                            .child("Code usage"),
                    )
                    .child(div().flex_none().child(self.relative(&f.path).into_owned()))
                    .child(div().text_color(c(p.ui_dim)).child(format!(
                        "{} usage(s), {}",
                        f.edits.len(),
                        f.reason
                    )))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.selected = i;
                        this.toggle(i, cx);
                    }))
            })
            .collect();
        let summary = {
            let mut text = format!(
                "{usages} code usage(s) in {} file(s) are renamed; {chosen} of {texts} occurrence(s) in comments, strings and text files are included",
                self.plan.files.len(),
            );
            if !self.outside.is_empty() {
                text.push_str(&format!(
                    "; {outside_chosen} of {} file(s) outside the project's own sources are included",
                    self.outside.len()
                ));
            }
            text
        };
        let rows: Vec<_> = self
            .occurrences
            .iter()
            .zip(&self.included)
            .enumerate()
            .map(|(i, (o, on))| {
                let kind = match o.kind {
                    OccurrenceKind::CommentOrString => "Comment or string",
                    OccurrenceKind::Text => "Text",
                };
                let place = format!("{}:{}", self.relative(&o.path), o.edit.range.start_line + 1);
                let on = *on;
                div()
                    .id(("rename-occurrence", i))
                    .h(px(24.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                    .when(!on, |d| d.text_color(c(p.ui_dim)))
                    .child(div().flex_none().child(if on { "☑" } else { "☐" }))
                    .child(
                        div()
                            .w(px(120.))
                            .flex_none()
                            .text_color(c(p.ui_muted))
                            .child(kind),
                    )
                    .child(div().flex_none().child(place))
                    .child(
                        div()
                            .text_color(c(p.ui_dim))
                            .child(o.line_text.trim().to_owned()),
                    )
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.selected = i;
                        this.toggle(i, cx);
                    }))
            })
            .collect();
        div()
            .id("rename-preview")
            .track_focus(&self.focus)
            .occlude()
            .on_key_down(cx.listener(Self::on_key))
            .w(px(760.))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!(
                        "Rename '{}' to '{}'",
                        self.plan.old_name, self.plan.new_name
                    )),
            )
            .child(div().px_3().pb_2().text_color(c(p.ui_muted)).child(summary))
            .child(
                div()
                    .id("rename-occurrences")
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .py_1()
                    .children(rows)
                    .children(outside_rows),
            )
            .child(
                div().px_3().py_2().text_xs().text_color(c(p.ui_dim)).child(
                    "Space includes or excludes · Enter renames (Do Refactor) · Escape cancels",
                ),
            )
    }
}
