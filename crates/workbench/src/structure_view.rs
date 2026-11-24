//! The File Structure popup (`SPEC-0079`): the active file's symbols as a tree; typing filters,
//! Enter jumps. v4: ordered by kind and name, nested types closed, filters for inherited members,
//! anonymous classes and lambdas, the typed text in a field with the matches marked.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

use ferris_idea_languages::insight::Symbol;
use ferris_idea_languages::structure::{filtered, ordered};
use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, uniform_list,
};

use crate::icon_images::icon;
use crate::palette::Palette;
use crate::structure_model::{
    at_caret, closed_at_start, filter, has_children, matched_ranges, matches, open_rows,
};
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
pub enum StructureEvent {
    /// Jump to a 0-based line and character of the file.
    Open(u64, u64),
    /// Jump to a place in another file (an inherited member, behaviour 9).
    OpenAt(PathBuf, u64, u64),
    /// Ask for the inherited members (behaviour 9).
    Inherited,
    /// Close the popup.
    Close,
}

/// A filter of behaviour 9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureFilter {
    /// Inherited members (⌘F12 again).
    Inherited,
    /// Anonymous classes (⌘I).
    Anonymous,
    /// Lambdas (⌘L).
    Lambdas,
}

/// Symbols of one file.
pub struct StructureView {
    title: String,
    /// The file's symbols in document order, without packages.
    all: Vec<Symbol>,
    /// The same with the inherited members, once they arrived.
    with_inherited: Option<Vec<Symbol>>,
    inherited: bool,
    anonymous: bool,
    lambdas: bool,
    /// The rows that can be shown, ordered and filtered (behaviours 7, 9).
    symbols: Vec<Symbol>,
    closed: HashSet<usize>,
    caret: (u64, u64),
    query: String,
    shown: Vec<usize>,
    selected: usize,
    focus: FocusHandle,
    /// Shown in the Structure tool window instead of a popup (`SPEC-0061` v7 behaviour 42).
    docked: bool,
}

impl EventEmitter<StructureEvent> for StructureView {}

impl StructureView {
    /// A popup for the file `name` listing `symbols`, opened on the symbol around `caret`
    /// (behaviours 1–2).
    pub fn new(
        name: &str,
        symbols: Vec<Symbol>,
        caret: (u64, u64),
        cx: &mut Context<Self>,
    ) -> Self {
        let mut v = Self {
            title: name.to_owned(),
            all: symbols,
            with_inherited: None,
            inherited: false,
            anonymous: false,
            lambdas: false,
            symbols: Vec::new(),
            closed: HashSet::new(),
            caret,
            query: String::new(),
            shown: Vec::new(),
            selected: 0,
            focus: cx.focus_handle(),
            docked: false,
        };
        v.rebuild();
        v
    }

    /// Orders and filters the symbols again and starts with nested types closed.
    fn rebuild(&mut self) {
        let base = match (&self.with_inherited, self.inherited) {
            (Some(w), true) => w.clone(),
            _ => self.all.clone(),
        };
        self.symbols = ordered(filtered(base, self.anonymous, self.lambdas));
        self.closed = closed_at_start(&self.symbols, self.caret.0, self.caret.1);
        self.refilter();
    }

    /// Shows the list in the Structure tool window: it fills the window and has no frame of its
    /// own (`SPEC-0061` v7 behaviour 42).
    pub fn set_docked(&mut self, docked: bool) {
        self.docked = docked;
    }

    /// Focus handle of the list.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Names of the rows shown, in order.
    pub fn names(&self) -> Vec<String> {
        self.shown
            .iter()
            .map(|&i| self.symbols[i].name.clone())
            .collect()
    }

    /// Name of the selected row.
    pub fn selected_name(&self) -> Option<String> {
        self.shown
            .get(self.selected)
            .map(|&i| self.symbols[i].name.clone())
    }

    /// The typed filter text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Whether a filter is on.
    pub fn filter_on(&self, f: StructureFilter) -> bool {
        match f {
            StructureFilter::Inherited => self.inherited,
            StructureFilter::Anonymous => self.anonymous,
            StructureFilter::Lambdas => self.lambdas,
        }
    }

    /// Switches a filter (behaviour 9); the inherited members are asked for on first use.
    pub fn toggle(&mut self, f: StructureFilter, cx: &mut Context<Self>) {
        match f {
            StructureFilter::Inherited => {
                self.inherited = !self.inherited;
                if self.inherited && self.with_inherited.is_none() {
                    cx.emit(StructureEvent::Inherited);
                }
            }
            StructureFilter::Anonymous => self.anonymous = !self.anonymous,
            StructureFilter::Lambdas => self.lambdas = !self.lambdas,
        }
        self.rebuild();
        cx.notify();
    }

    /// The symbols with the inherited members, as the server gave them (behaviour 9).
    pub fn set_inherited(&mut self, symbols: Vec<Symbol>, cx: &mut Context<Self>) {
        let mut symbols = symbols;
        // The popup shows no package rows (v3).
        symbols = crate::structure_model::without_packages(symbols);
        self.with_inherited = Some(symbols);
        self.rebuild();
        cx.notify();
    }

    fn refilter(&mut self) {
        let typed = !self.query.trim().is_empty();
        let shown = filter(&self.symbols, &self.query);
        // While text is typed every row with a match below it is open (behaviour 8).
        self.shown = if typed {
            shown
        } else {
            open_rows(&self.symbols, &shown, &self.closed)
        };
        // With a query the first real match is selected, otherwise the symbol at the caret.
        self.selected = if typed {
            self.shown
                .iter()
                .position(|&i| matches(&self.symbols[i], &self.query))
                .unwrap_or(0)
        } else {
            at_caret(&self.symbols, &self.shown, self.caret.0, self.caret.1)
        };
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        if let Some(&i) = self.shown.get(self.selected) {
            let s = &self.symbols[i];
            match &s.path {
                Some(p) => cx.emit(StructureEvent::OpenAt(p.clone(), s.line, s.character)),
                None => cx.emit(StructureEvent::Open(s.line, s.character)),
            }
        }
    }

    /// Opens or closes the row `row` of the shown rows (behaviour 8).
    fn set_open(&mut self, row: usize, open: bool) {
        let Some(&i) = self.shown.get(row) else {
            return;
        };
        if open {
            self.closed.remove(&i);
        } else if has_children(&self.symbols, i) {
            self.closed.insert(i);
        }
        let typed = !self.query.trim().is_empty();
        if !typed {
            let all = filter(&self.symbols, "");
            self.shown = open_rows(&self.symbols, &all, &self.closed);
            self.selected = self.shown.iter().position(|&s| s == i).unwrap_or(0);
        }
    }

    /// Right opens (or moves to the first child), Left closes (or moves to the parent).
    fn side(&mut self, right: bool) {
        let Some(&i) = self.shown.get(self.selected) else {
            return;
        };
        let children = has_children(&self.symbols, i);
        let open = children && !self.closed.contains(&i);
        if right {
            if children && !open {
                self.set_open(self.selected, true);
            } else if open {
                self.selected = (self.selected + 1).min(self.shown.len().saturating_sub(1));
            }
        } else if open && self.query.trim().is_empty() {
            self.set_open(self.selected, false);
        } else if let Some(p) = self.shown[..self.selected]
            .iter()
            .rposition(|&j| self.symbols[j].depth < self.symbols[i].depth)
        {
            self.selected = p;
        }
    }

    /// The filter a shortcut switches: ⌘F12 again, ⌘I, ⌘L (Ctrl elsewhere).
    fn shortcut_filter(event: &KeyDownEvent) -> Option<StructureFilter> {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let primary = if cfg!(target_os = "macos") {
            m.platform && !m.control
        } else {
            m.control && !m.platform
        };
        if !primary || m.alt || m.shift {
            return None;
        }
        match k.key.as_str() {
            "f12" => Some(StructureFilter::Inherited),
            "i" => Some(StructureFilter::Anonymous),
            "l" => Some(StructureFilter::Lambdas),
            _ => None,
        }
    }

    /// Up/Down select with wrap-around, Enter jumps, Escape closes, Left/Right close and open,
    /// the filter shortcuts switch, other keys type into the speed search (behaviours 3–4, 8–9).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = Self::shortcut_filter(event) {
            self.toggle(f, cx);
            cx.stop_propagation();
            return;
        }
        let k = &event.keystroke;
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.shown.len()) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.platform {
            return;
        }
        let n = self.shown.len().max(1);
        match k.key.as_str() {
            "down" => self.selected = (self.selected + 1) % n,
            "up" => self.selected = (self.selected + n - 1) % n,
            "right" => self.side(true),
            "left" => self.side(false),
            "enter" => self.choose(cx),
            "escape" => cx.emit(StructureEvent::Close),
            "backspace" => {
                self.query.pop();
                self.refilter();
            }
            // Typed text arrives through the platform's text input, not as the key behind it
            // (`SPEC-0004` v8 behaviour 17b).
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Takes text the platform's text input committed: it extends the speed search
    /// (`SPEC-0004` v8 behaviour 17b).
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        self.query.push_str(committed);
        self.refilter();
        cx.notify();
    }

    /// A filter toggle above the tree (behaviour 9).
    fn filter_button(
        &self,
        control: &'static ferris_idea_ui::tooltip::Control,
        f: StructureFilter,
        glyph: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let on = self.filter_on(f);
        div()
            .id(control.name)
            .tip(control)
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .when(on, |d| d.bg(c(p.tree_selection)))
            .hover(|s| s.bg(c(p.toolbar_hover)))
            .text_color(c(p.ui_muted))
            .child(glyph)
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.toggle(f, cx);
            }))
    }
}

/// A row as drawn: symbol, whether it matches, its chevron (open or not), the matched ranges.
type ShownRow = (Symbol, bool, Option<bool>, Vec<Range<usize>>);

crate::committed_text::committed_text_input!(StructureView, take_committed);

impl Render for StructureView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let query = self.query.clone();
        let typed = !query.trim().is_empty();
        let rows: Vec<ShownRow> = self
            .shown
            .iter()
            .map(|&i| {
                let s = self.symbols[i].clone();
                let hit = !typed || matches(&s, &query);
                let chevron =
                    has_children(&self.symbols, i).then(|| typed || !self.closed.contains(&i));
                let marks = if typed {
                    matched_ranges(&s.name, &query)
                } else {
                    Vec::new()
                };
                (s, hit, chevron, marks)
            })
            .collect();
        let selected = self.selected;
        let count = rows.len();
        let hit_bg = gpui::rgb(
            p.scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
                .unwrap_or(p.selection),
        );
        let list = uniform_list(
            "structure",
            count,
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let (s, hit, chevron, marks) = &rows[i];
                        let inherited = s.inherited_from.is_some();
                        // Matched parts in the accent colour on the search-hit background (v4).
                        let name = gpui::StyledText::new(s.name.clone()).with_highlights(
                            marks.iter().map(|r| {
                                (
                                    r.clone(),
                                    gpui::HighlightStyle {
                                        color: Some(c(p.link)),
                                        background_color: Some(hit_bg.into()),
                                        ..Default::default()
                                    },
                                )
                            }),
                        );
                        let detail = match &s.inherited_from {
                            Some(from) => Some(format!("↑ {from}")),
                            None => s.detail.clone().filter(|d| !d.trim().is_empty()),
                        };
                        div()
                            .id(("structure-row", i))
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(24.))
                            .w_full()
                            .pr_2()
                            .pl(px(crate::metrics::tree_indent(s.depth)))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|st| st.bg(c(p.tree_hover))))
                            .when(!hit || inherited, |d| d.text_color(c(p.ui_dim)))
                            .child(match chevron {
                                Some(open) => div()
                                    .id(("structure-arrow", i))
                                    .flex_none()
                                    .child(icon(if *open {
                                        "general/chevronDown"
                                    } else {
                                        "general/chevronRight"
                                    }))
                                    .on_click(cx.listener(
                                        move |this: &mut Self, _: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            let open = this
                                                .shown
                                                .get(i)
                                                .is_some_and(|j| this.closed.contains(j));
                                            this.set_open(i, open);
                                            cx.notify();
                                        },
                                    ))
                                    .into_any_element(),
                                None => div().w(px(16.)).flex_none().into_any_element(),
                            })
                            .child(icon(ferris_idea_ui::icons::symbol_icon(&s.kind)))
                            .child(div().flex_none().child(name))
                            .when_some(detail, |d, detail| {
                                d.child(
                                    div()
                                        .flex_1()
                                        .overflow_hidden()
                                        .text_color(c(if i == selected {
                                            p.ui_muted
                                        } else {
                                            p.ui_dim
                                        }))
                                        .child(detail),
                                )
                            })
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                this.choose(cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .px_1()
        // In the tool window the list fills it (SPEC-0061 v7 behaviour 42).
        .when(self.docked, |l| l.size_full())
        .when(!self.docked, |l| {
            l.h(px((count.clamp(1, 16) as f32) * 24.0))
        });
        let docked = self.docked;
        div()
            .id(if docked {
                "structure-tool-window"
            } else {
                "structure-popup"
            })
            .track_focus(&self.focus)
            .occlude()
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .when(docked, |d| d.size_full())
            .when(!docked, |d| {
                d.w(px(520.))
                    .border_1()
                    .border_color(c(p.separator))
                    .rounded(px(8.))
                    .shadow_lg()
            })
            .overflow_hidden()
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .text_color(c(p.ui_muted))
                            .child(self.title.clone()),
                    )
                    .child(self.filter_button(
                        declared!(OTHER_TOOL_WINDOWS, "structure-inherited"),
                        StructureFilter::Inherited,
                        "↑",
                        cx,
                    ))
                    .child(self.filter_button(
                        declared!(OTHER_TOOL_WINDOWS, "structure-anonymous"),
                        StructureFilter::Anonymous,
                        "{}",
                        cx,
                    ))
                    .child(self.filter_button(
                        declared!(OTHER_TOOL_WINDOWS, "structure-lambdas"),
                        StructureFilter::Lambdas,
                        "λ",
                        cx,
                    )),
            )
            // The typed text in a field below the title (behaviour 10).
            .when(!self.query.is_empty(), |d| {
                d.child(
                    div()
                        .mx_2()
                        .mb_1()
                        .px_2()
                        .py_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c(p.focus))
                        .child(icon("general/search"))
                        .child(self.query.clone()),
                )
            })
            .child(
                div()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .py_1()
                    .when(docked, |d| d.flex_1().min_h_0())
                    .child(list),
            )
            .when(count == 0, |d| {
                d.child(
                    div()
                        .px_3()
                        .pb_2()
                        .text_color(c(p.ui_dim))
                        .child("Nothing matches"),
                )
            })
    }
}
