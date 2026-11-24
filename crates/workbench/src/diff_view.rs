//! The diff viewer (`SPEC-0084`): HEAD and the working copy of a file side by side or unified; F7 and
//! ⇧F7 go to the next and previous difference. v2: syntax colours, changed words, `»` to take a
//! change back into the working copy and the unified view (behaviours 7–10).

use std::ops::Range;
use std::path::{Path, PathBuf};

use ferris_idea_vcs::{
    DiffRow, LineChange, LineChangeKind, UnifiedRow, WordRanges, change_of_rows, change_starts,
    changed_words, unified_rows,
};
use gpui::{
    Context, EventEmitter, FocusHandle, HighlightStyle, KeyDownEvent, ScrollStrategy,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::diff_actions::{self, Pane};
use crate::diff_model::{self, Piece};
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// What the viewer asks the workbench to do.
pub enum DiffEvent {
    /// Close the viewer.
    Close,
    /// Replace this change of the working copy by its HEAD lines (behaviour 9).
    Revert(LineChange),
    /// The view switched between side by side (`false`) and unified (`true`): keep it for the next
    /// viewer (behaviour 10).
    Unified(bool),
    /// F7 or ⇧F7 was pressed in the viewer: `moved` says whether the step reached another
    /// difference; if not, the workbench may go on to another file (v5 behaviour 21).
    Stepped {
        /// Towards the next difference.
        forward: bool,
        /// The current difference changed.
        moved: bool,
    },
}

/// Syntax kinds per line of one side (char ranges within the line).
type SideSyntax = Vec<Vec<(Range<usize>, String)>>;

/// Two versions of one file side by side.
pub struct DiffView {
    title: String,
    /// Titles of the left and right column.
    sides: (String, String),
    rows: Vec<DiffRow>,
    /// Changed words of each row: left and right char ranges (behaviour 8).
    words: Vec<WordRanges>,
    unified: Vec<UnifiedRow>,
    /// Show one column instead of two (behaviour 10).
    unified_view: bool,
    /// Rows (of the current view) where a change starts.
    starts: Vec<usize>,
    /// Index into `starts` of the difference shown last.
    current: Option<usize>,
    /// Syntax colours of the left and right side (behaviour 7).
    syntax: (SideSyntax, SideSyntax),
    /// The right side is the working copy: changes can be taken back (behaviour 9).
    revertible: bool,
    /// The current pane of the side-by-side view (v5 behaviour 19).
    pane: Pane,
    /// The file whose language colours the sides (behaviour 7).
    syntax_path: Option<PathBuf>,
    /// The empty row across from an added or deleted line is marked too, in a lighter shade
    /// (the local history, `SPEC-0085` v2 behaviour 13).
    filler_marks: bool,
    /// The header has Previous Difference and Next Difference buttons (`SPEC-0085` v2
    /// behaviour 12).
    stepping: bool,
    /// The header has Accept and Reject buttons: the right side is a proposed change the user
    /// decides on (`SPEC-0358` behaviour 18).
    decision: bool,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
}

impl EventEmitter<DiffEvent> for DiffView {}

/// The user's answer to a proposed change (`SPEC-0358` behaviour 18): `true` accepts the right
/// side, `false` rejects it. Only a viewer made [`DiffView::with_decision`] sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffDecided(pub bool);

impl EventEmitter<DiffDecided> for DiffView {}

/// Changed words of every row (behaviour 8).
fn words_of(rows: &[DiffRow]) -> Vec<WordRanges> {
    rows.iter()
        .map(|r| match (r.kind, &r.left, &r.right) {
            (Some(LineChangeKind::Modified), Some(l), Some(rt)) => {
                changed_words(&l.1, &rt.1).unwrap_or_default()
            }
            _ => (Vec::new(), Vec::new()),
        })
        .collect()
}

/// Rows of the unified view where a change starts.
fn unified_starts(rows: &[UnifiedRow]) -> Vec<usize> {
    (0..rows.len())
        .filter(|&i| rows[i].kind.is_some() && (i == 0 || rows[i - 1].kind.is_none()))
        .collect()
}

impl DiffView {
    /// A viewer titled `title` showing `rows`, scrolled to the first difference (behaviour 2).
    pub fn new(title: String, rows: Vec<DiffRow>, cx: &mut Context<Self>) -> Self {
        let starts = change_starts(&rows);
        let mut view = Self {
            title,
            sides: ("HEAD".into(), "Working copy".into()),
            words: words_of(&rows),
            unified: unified_rows(&rows),
            rows,
            unified_view: false,
            starts,
            current: None,
            syntax: (Vec::new(), Vec::new()),
            revertible: false,
            pane: Pane::Right,
            syntax_path: None,
            filler_marks: false,
            stepping: false,
            decision: false,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
        };
        view.step(true);
        view
    }

    /// Marks the empty row across from an added or deleted line in a lighter shade of its colour
    /// (`SPEC-0085` v2 behaviour 13).
    pub fn with_filler_marks(mut self) -> Self {
        self.filler_marks = true;
        self
    }

    /// Whether the empty row across from a change is marked.
    pub fn filler_marks(&self) -> bool {
        self.filler_marks
    }

    /// Puts Previous Difference and Next Difference buttons into the header (`SPEC-0085` v2
    /// behaviour 12).
    pub fn with_stepping(mut self) -> Self {
        self.stepping = true;
        self
    }

    /// Puts Accept and Reject buttons into the header (`SPEC-0358` behaviour 18).
    pub fn with_decision(mut self) -> Self {
        self.decision = true;
        self
    }

    /// Accepts (`true`) or rejects the proposed change, as a click on its button.
    pub fn decide(&mut self, accept: bool, cx: &mut Context<Self>) {
        if self.decision {
            cx.emit(DiffDecided(accept));
        }
    }

    /// Names the two columns, e.g. a local history revision and "Current" (SPEC-0085).
    pub fn with_sides(mut self, left: &str, right: &str) -> Self {
        self.sides = (left.to_owned(), right.to_owned());
        self
    }

    /// Colours both sides by the language of `path`; `old` and `new` are the texts the rows were
    /// made of (behaviour 7).
    pub fn with_texts(mut self, path: &Path, old: &str, new: &str) -> Self {
        self.syntax_path = Some(path.to_path_buf());
        self.syntax = (
            diff_model::syntax_by_line(path, old),
            diff_model::syntax_by_line(path, new),
        );
        self
    }

    /// The right side is the working copy: `»` takes changes back (behaviour 9).
    pub fn revertible(mut self) -> Self {
        self.revertible = true;
        self
    }

    /// Starts in the unified view (behaviour 10).
    pub fn with_unified(mut self, unified: bool) -> Self {
        if unified != self.unified_view {
            self.set_unified(unified);
        }
        self
    }

    /// Shows new rows after the working copy changed, keeping the scroll position near the
    /// difference shown (behaviour 9).
    pub fn set_rows(&mut self, rows: Vec<DiffRow>, path: &Path, old: &str, new: &str) {
        let keep = self.current;
        self.words = words_of(&rows);
        self.unified = unified_rows(&rows);
        self.rows = rows;
        self.syntax = (
            diff_model::syntax_by_line(path, old),
            diff_model::syntax_by_line(path, new),
        );
        self.starts = self.current_starts();
        self.current = keep
            .map(|i| i.min(self.starts.len().saturating_sub(1)))
            .filter(|_| !self.starts.is_empty());
    }

    /// Shows the rows of `old` and `new` after a side changed, coloured as before and at the same
    /// place (v5 behaviour 18).
    pub fn refresh(&mut self, old: &str, new: &str) {
        let rows = ferris_idea_vcs::side_by_side(old, new);
        match self.syntax_path.clone() {
            Some(path) => self.set_rows(rows, &path, old, new),
            None => {
                let keep = self.current;
                self.words = words_of(&rows);
                self.unified = unified_rows(&rows);
                self.rows = rows;
                self.starts = self.current_starts();
                self.current = keep
                    .map(|i| i.min(self.starts.len().saturating_sub(1)))
                    .filter(|_| !self.starts.is_empty());
            }
        }
    }

    /// The titles of the left and right column.
    pub fn side_titles(&self) -> (&str, &str) {
        (&self.sides.0, &self.sides.1)
    }

    /// The current pane of the side-by-side view (v5 behaviour 19).
    pub fn pane(&self) -> Pane {
        self.pane
    }

    /// `Diff.FocusOppositePane`: makes the other pane current and returns it with the line it
    /// shows at the current difference; `None` in the unified view, which has one pane.
    pub fn focus_opposite_pane(&mut self) -> Option<(Pane, Option<usize>)> {
        if self.unified_view {
            return None;
        }
        self.pane = self.pane.other();
        let line = self
            .current_row()
            .and_then(|row| diff_actions::pane_line(&self.rows, row, self.pane));
        Some((self.pane, line))
    }

    /// Index of the current difference.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// The change of the current difference (v5 behaviour 18).
    pub fn current_change(&self) -> Option<LineChange> {
        self.change_of(self.current?)
    }

    /// Whether `»` is shown: the right side takes changes (behaviour 9).
    pub fn is_revertible(&self) -> bool {
        self.revertible
    }

    /// Goes to the last difference (a file reached with ⇧F7, v5 behaviour 21).
    pub fn go_to_last(&mut self) {
        if let Some(last) = self.starts.len().checked_sub(1) {
            self.current = Some(last);
            if let Some(row) = self.current_row() {
                self.scroll
                    .scroll_to_item(row.saturating_sub(3), ScrollStrategy::Top);
            }
        }
    }

    fn current_starts(&self) -> Vec<usize> {
        if self.unified_view {
            unified_starts(&self.unified)
        } else {
            change_starts(&self.rows)
        }
    }

    /// Switches between side by side and unified, staying at the same difference (behaviour 10).
    pub fn set_unified(&mut self, unified: bool) {
        self.unified_view = unified;
        self.starts = self.current_starts();
        if let Some(row) = self.current_row() {
            self.scroll
                .scroll_to_item(row.saturating_sub(3), ScrollStrategy::Top);
        }
    }

    /// Whether the unified view is shown.
    pub fn unified(&self) -> bool {
        self.unified_view
    }

    /// The file the viewer compares, if it compares one (the navigation bar, SPEC-0061 v13
    /// behaviour 60).
    pub fn file(&self) -> Option<&Path> {
        self.syntax_path.as_deref()
    }

    /// The viewer's title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Focus handle of the viewer.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Number of differences.
    pub fn differences(&self) -> usize {
        self.starts.len()
    }

    /// Row index (of the current view) of the current difference.
    pub fn current_row(&self) -> Option<usize> {
        self.current.and_then(|i| self.starts.get(i).copied())
    }

    /// The side-by-side rows.
    pub fn rows(&self) -> &[DiffRow] {
        &self.rows
    }

    /// The change a `»` at the difference `index` takes back (behaviour 9).
    pub fn change_of(&self, index: usize) -> Option<LineChange> {
        let row = *self.starts.get(index)?;
        let start = if self.unified_view {
            self.unified.get(row)?.row
        } else {
            row
        };
        change_of_rows(&self.rows, start)
    }

    /// Takes back the difference `index` (as a click on its `»`); only when revertible.
    pub fn revert(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.revertible {
            return;
        }
        if let Some(change) = self.change_of(index) {
            self.current = Some(index);
            cx.emit(DiffEvent::Revert(change));
        }
    }

    /// Goes to the next (previous) difference, stopping at the ends (behaviour 3); returns
    /// whether there was one to go to.
    pub(crate) fn step(&mut self, forward: bool) -> bool {
        let Some(next) = diff_actions::within(self.starts.len(), self.current, forward) else {
            return false;
        };
        self.current = Some(next);
        if let Some(row) = self.current_row() {
            self.scroll
                .scroll_to_item(row.saturating_sub(3), ScrollStrategy::Top);
        }
        true
    }

    /// Shows the difference whose change starts at new line `line` (1-based) or the first after it
    /// (the change bar's Show Diff, SPEC-0064 v3 behaviour 7).
    pub fn go_to_new_line(&mut self, line: usize) {
        let found = (0..self.starts.len()).find(|&i| {
            self.change_of(i)
                .is_some_and(|c| c.new_start.max(c.new_end) >= line)
        });
        if let Some(i) = found {
            self.current = Some(i);
            if let Some(row) = self.current_row() {
                self.scroll
                    .scroll_to_item(row.saturating_sub(3), ScrollStrategy::Top);
            }
        }
    }

    /// F7 / ⇧F7 step through the differences, `U` switches the view, Escape closes (behaviours 3,
    /// 4, 10).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        }
        match (k.key.as_str(), k.modifiers.shift) {
            ("f7", shift) => {
                let moved = self.step(!shift);
                cx.emit(DiffEvent::Stepped {
                    forward: !shift,
                    moved,
                });
            }
            ("escape", false) => cx.emit(DiffEvent::Close),
            ("u", _) => {
                let unified = !self.unified_view;
                self.set_unified(unified);
                cx.emit(DiffEvent::Unified(unified));
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// Colours of the viewer.
#[derive(Clone, Copy)]
struct Look {
    inserted: u32,
    deleted: u32,
    modified: u32,
    number: ferris_idea_ui::theme::Rgba,
}

/// The text of one side of a row: syntax colours and changed words as highlights (behaviours 7–8).
/// The backgrounds of a row's left and right side (`None`: unmarked). With `filler` the empty
/// side of an added or deleted line gets a lighter shade of the line's colour (`SPEC-0085` v2
/// behaviour 13).
fn row_marks(
    kind: Option<LineChangeKind>,
    has: (bool, bool),
    look: &Look,
    filler: bool,
) -> (Option<u32>, Option<u32>) {
    let faint = |rgb: u32| filler.then(|| fainter(rgb));
    match kind {
        None => (None, None),
        Some(LineChangeKind::Added) => (faint(look.inserted), Some(look.inserted)),
        Some(LineChangeKind::Deleted) => (Some(look.deleted), faint(look.deleted)),
        Some(LineChangeKind::Modified) => (
            has.0.then_some(look.modified),
            has.1.then_some(look.modified),
        ),
    }
}

/// A fainter shade of the background `rgb`, nearer the editor's background: darker on a dark
/// theme, lighter on a light one (`SPEC-0085` v2 behaviour 13).
pub fn fainter(rgb: u32) -> u32 {
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    let luma = (r * 299 + g * 587 + b * 114) / 1000;
    let mix = |v: u32| {
        if luma < 128 {
            v * 60 / 100
        } else {
            v + (255 - v) * 50 / 100
        }
    };
    (mix(r) << 16) | (mix(g) << 8) | mix(b)
}

/// A stronger shade of the background `rgb`: lighter on a dark theme, darker on a light one, so
/// changed words stand out on a row of the same colour (`SPEC-0084` v3 behaviour 13).
pub fn stronger(rgb: u32) -> u32 {
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    let luma = (r * 299 + g * 587 + b * 114) / 1000;
    let mix = |v: u32| {
        if luma < 128 {
            v + (255 - v) * 22 / 100
        } else {
            v * 82 / 100
        }
    };
    (mix(r) << 16) | (mix(g) << 8) | mix(b)
}

pub(crate) fn side_text(
    line: &str,
    kinds: &[(Range<usize>, String)],
    words: &[Range<usize>],
    word_bg: u32,
) -> gpui::AnyElement {
    let (text, pieces) = diff_model::styled_line(line, kinds, words);
    let p = Palette::current();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = pieces
        .into_iter()
        .map(|Piece { bytes, kind, word }| {
            let style = kind.map(|k| p.capture(&k));
            (
                bytes,
                HighlightStyle {
                    color: style
                        .as_ref()
                        .and_then(|s| s.foreground)
                        .map(|c| gpui::rgb(c).into()),
                    font_weight: style
                        .as_ref()
                        .filter(|s| s.bold)
                        .map(|_| gpui::FontWeight::BOLD),
                    font_style: style
                        .as_ref()
                        .filter(|s| s.italic)
                        .map(|_| gpui::FontStyle::Italic),
                    background_color: word.then(|| gpui::rgb(word_bg).into()),
                    ..HighlightStyle::default()
                },
            )
        })
        .collect();
    gpui::StyledText::new(text)
        .with_highlights(highlights)
        .into_any_element()
}

impl Render for DiffView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let look = Look {
            inserted: p.diff_inserted,
            deleted: p.diff_deleted,
            modified: p.diff_modified,
            number: p.ui_dim,
        };
        let rows = self.rows.clone();
        let words = self.words.clone();
        let unified = self.unified.clone();
        let syntax = self.syntax.clone();
        let unified_view = self.unified_view;
        let pane = self.pane;
        let current = self.current_row();
        let filler_marks = self.filler_marks;
        // `»` at the first row of each change (behaviour 9).
        let arrows: Vec<(usize, usize)> = if self.revertible {
            self.starts
                .iter()
                .copied()
                .enumerate()
                .map(|(i, r)| (r, i))
                .collect()
        } else {
            Vec::new()
        };
        let digits = rows
            .iter()
            .filter_map(|r| r.right.as_ref().or(r.left.as_ref()).map(|l| l.0))
            .max()
            .unwrap_or(1)
            .to_string()
            .len();
        let number_width = 10. + 8. * digits as f32;
        let count = if unified_view {
            unified.len()
        } else {
            rows.len()
        };
        let list = uniform_list(
            "diff-rows",
            count,
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                let number = |n: Option<usize>| {
                    div()
                        .w(px(number_width))
                        .flex_none()
                        .pr_2()
                        .flex()
                        .justify_end()
                        .text_color(c(look.number))
                        // Line 0 marks a header row (a file of a stash, SPEC-0145).
                        .child(
                            n.filter(|&n| n > 0)
                                .map(|n| n.to_string())
                                .unwrap_or_default(),
                        )
                };
                let kinds = |left: bool, n: usize| -> Vec<(Range<usize>, String)> {
                    let side = if left { &syntax.0 } else { &syntax.1 };
                    n.checked_sub(1)
                        .and_then(|i| side.get(i))
                        .cloned()
                        .unwrap_or_default()
                };
                let arrow = |i: usize, cx: &mut Context<DiffView>| {
                    let index = arrows.iter().find(|(r, _)| *r == i).map(|(_, k)| *k);
                    div()
                        .w(px(16.))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .when_some(index, |d, index| {
                            d.child(
                                div()
                                    .id(("diff-revert", i))
                                    .cursor_pointer()
                                    .text_color(c(p.accent))
                                    .hover(|s| s.bg(c(p.toolbar_hover)))
                                    .rounded(px(3.))
                                    .px(px(2.))
                                    .child("»")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.revert(index, cx);
                                        cx.notify();
                                    })),
                            )
                        })
                };
                range
                    .map(|i| {
                        let row = div()
                            .id(("diff-row", i))
                            .w_full()
                            .h(px(20.))
                            .flex()
                            .font_family(crate::fonts::editor_family())
                            .when(current == Some(i), |d| {
                                d.border_t_1().border_color(c(p.tab_selected_border))
                            });
                        if unified_view {
                            let u = &unified[i];
                            let (bg, sign, left) = match u.kind {
                                Some(LineChangeKind::Deleted) => (Some(look.deleted), "-", true),
                                Some(_) => (Some(look.inserted), "+", false),
                                None => (None, " ", false),
                            };
                            let (n, row_words) = if left {
                                (u.old.unwrap_or(0), words[u.row].0.clone())
                            } else {
                                (u.new.unwrap_or(0), words[u.row].1.clone())
                            };
                            // The row already has this colour: the words get a stronger shade
                            // of it, or they would not stand out (SPEC-0084 v3 behaviour 13).
                            let word_bg = stronger(if left { look.deleted } else { look.inserted });
                            return row
                                .when_some(bg, |d, bg| d.bg(gpui::rgb(bg)))
                                .child(number(u.old))
                                .child(number(u.new))
                                .child(arrow(i, cx))
                                .child(div().w(px(14.)).flex_none().child(sign))
                                .child(
                                    div().flex_1().overflow_hidden().whitespace_nowrap().child(
                                        side_text(&u.text, &kinds(left, n), &row_words, word_bg),
                                    ),
                                );
                        }
                        let r = &rows[i];
                        let (left_bg, right_bg) = row_marks(
                            r.kind,
                            (r.left.is_some(), r.right.is_some()),
                            &look,
                            filler_marks,
                        );
                        let side = |line: &Option<(usize, String)>,
                                    bg: Option<u32>,
                                    left: bool,
                                    row_words: &[Range<usize>]| {
                            let word_bg = if left { look.deleted } else { look.inserted };
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .h_full()
                                .when_some(bg, |d, bg| d.bg(gpui::rgb(bg)))
                                .child(number(line.as_ref().map(|l| l.0)))
                                .child(
                                    div()
                                        .flex_1()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .children(line.as_ref().map(|(n, t)| {
                                            side_text(t, &kinds(left, *n), row_words, word_bg)
                                        })),
                                )
                        };
                        row.child(side(&r.left, left_bg, true, &words[i].0))
                            .child(div().w(px(1.)).h_full().bg(c(p.separator)))
                            .child(arrow(i, cx))
                            .child(side(&r.right, right_bg, false, &words[i].1))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .flex_1();
        let count = self.starts.len();
        let stepping = self.stepping;
        let step_button = |id: &'static str,
                           glyph: &'static str,
                           name: &'static str,
                           forward: bool,
                           cx: &mut Context<Self>| {
            div()
                .id(id)
                .px_1()
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(c(p.ui_text))
                .hover(|s| s.bg(c(p.toolbar_hover)))
                .child(glyph)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let moved = this.step(forward);
                    cx.emit(DiffEvent::Stepped { forward, moved });
                    cx.notify();
                }))
                .semantics(
                    Semantics::new(Role::Button)
                        .name(name)
                        .test_id(id)
                        .action(AxAction::Click)
                        .effects(&[Effect::None]),
                )
        };
        let decision = self.decision;
        let decide_button =
            |id: &'static str, label: &'static str, accept: bool, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .px_2()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .border_1()
                    .border_color(c(p.separator))
                    .text_color(c(p.ui_text))
                    .hover(|s| s.bg(c(p.toolbar_hover)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.decide(accept, cx)))
                    .semantics(
                        Semantics::new(Role::Button)
                            .name(label)
                            .test_id(id)
                            .action(AxAction::Click)
                            // Accepting writes the file (behaviour 18).
                            .effects(if accept {
                                &[Effect::Write]
                            } else {
                                &[Effect::None]
                            }),
                    )
            };
        let switch = |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .px_2()
                .rounded(px(4.))
                .cursor_pointer()
                .when(on, |d| d.bg(c(p.toolbar_hover)).text_color(c(p.ui_text)))
                .when(!on, |d| d.text_color(c(p.ui_muted)))
                .hover(|s| s.bg(c(p.toolbar_hover)))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let unified = id == "diff-unified";
                    if this.unified_view != unified {
                        this.set_unified(unified);
                        cx.emit(DiffEvent::Unified(unified));
                    }
                    cx.notify();
                }))
        };
        div()
            .id("diff-view")
            .track_focus(&self.focus)
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
                    .gap_3()
                    .px_3()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.title.clone()),
                    )
                    .when(stepping, |d| {
                        d.child(step_button(
                            "diff-previous",
                            "↑",
                            "Previous Difference",
                            false,
                            cx,
                        ))
                        .child(step_button(
                            "diff-next",
                            "↓",
                            "Next Difference",
                            true,
                            cx,
                        ))
                    })
                    .child(
                        div()
                            .text_color(c(p.ui_muted))
                            .child(match count {
                                0 => "No differences".to_owned(),
                                1 => "1 difference".to_owned(),
                                n => format!("{n} differences"),
                            })
                            .semantics(
                                Semantics::new(Role::Status)
                                    .name("Differences")
                                    .test_id("diff-count"),
                            ),
                    )
                    .child(div().flex_1())
                    .when(decision, |d| {
                        d.child(decide_button("diff-accept", "Accept", true, cx))
                            .child(decide_button("diff-reject", "Reject", false, cx))
                    })
                    .child(switch(
                        "diff-side-by-side",
                        "Side-by-side",
                        !unified_view,
                        cx,
                    ))
                    .child(switch("diff-unified", "Unified", unified_view, cx))
                    .child(
                        div()
                            .text_color(c(p.ui_dim))
                            .child("F7 / ⇧F7 next / previous · U view · Esc closes"),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .h(px(24.))
                    .items_center()
                    .border_t_1()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .text_color(c(p.ui_muted))
                    .when(!unified_view, |d| {
                        // The current pane's title in the text colour with a line under it
                        // (v5 behaviour 19).
                        let title = |side: Pane, text: String| {
                            div().flex_1().px_3().flex().child(
                                div()
                                    .when(pane == side, |t| {
                                        t.text_color(c(p.ui_text))
                                            .border_b_1()
                                            .border_color(c(p.tab_selected_border))
                                    })
                                    .child(text),
                            )
                        };
                        d.child(title(Pane::Left, self.sides.0.clone()))
                            .child(title(Pane::Right, self.sides.1.clone()))
                    })
                    .when(unified_view, |d| {
                        d.child(
                            div()
                                .px_3()
                                .child(format!("{} → {}", self.sides.0, self.sides.1)),
                        )
                    }),
            )
            .child(div().flex_1().min_h_0().flex().flex_col().child(list))
    }
}

#[cfg(test)]
mod tests {
    use super::{Look, fainter, row_marks, stronger};
    use ferris_idea_vcs::LineChangeKind;

    // SPEC-0085 v2 AK-5: with filler marks the empty side of an added or deleted line is marked
    // in a lighter shade; without them (the diff viewer) it stays unmarked.
    #[test]
    fn filler_rows_are_marked_on_both_sides() {
        let look = Look {
            inserted: 0x294436,
            deleted: 0x484a4a,
            modified: 0x385570,
            number: ferris_idea_ui::theme::Rgba { rgb: 0, alpha: 255 },
        };
        let added = Some(LineChangeKind::Added);
        assert_eq!(
            row_marks(added, (false, true), &look, true),
            (Some(fainter(0x294436)), Some(0x294436))
        );
        assert_eq!(
            row_marks(added, (false, true), &look, false),
            (None, Some(0x294436))
        );
        let deleted = Some(LineChangeKind::Deleted);
        assert_eq!(
            row_marks(deleted, (true, false), &look, true),
            (Some(0x484a4a), Some(fainter(0x484a4a)))
        );
        assert_ne!(fainter(0x294436), 0x294436);
        assert!(
            (fainter(0x294436) >> 8) & 0xff < 0x44,
            "darker on a dark theme"
        );
        assert!(
            (fainter(0xc8e6c9) >> 8) & 0xff > 0xe6,
            "lighter on a light theme"
        );
    }

    // SPEC-0084 v3 AK-9: a changed word stands out on a row of its own colour.
    #[test]
    fn changed_words_get_a_stronger_shade_of_the_row() {
        let dark = 0x294436;
        let s = stronger(dark);
        assert_ne!(s, dark);
        assert!(
            (s >> 8) & 0xff > (dark >> 8) & 0xff,
            "lighter on a dark theme"
        );
        let light = 0xc8e6c9;
        assert!(
            (stronger(light) >> 8) & 0xff < (light >> 8) & 0xff,
            "darker on a light theme"
        );
    }
}
