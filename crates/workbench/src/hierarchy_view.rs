//! The call hierarchy tool window (`SPEC-0082` v2): callers or callees of a function as a tree;
//! Enter jumps, Left/Right collapse and expand. The same window shows a Java type hierarchy and a
//! method hierarchy (`SPEC-0082` v5 behaviours 13 and 14).

use std::collections::HashSet;
use std::ops::Range;

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, uniform_list,
};
use idecraft_languages::insight::{CallDirection, CallNode};
use idecraft_languages::java_structure::TypeDirection;

use crate::hierarchy_model::{
    NodePath, Row, initially_expanded, node, root_expanded, row_detail, row_label, rows, sorted,
};
use crate::icon_images::icon;
use crate::nav_model::Place;
use crate::palette::Palette;
use crate::tips::Tipped;
use crate::workbench_view::c;

/// What the tool window asks the workbench to do.
pub enum HierarchyEvent {
    /// Jump to a function.
    Open(Place),
    /// Show the other direction for the same function.
    Direction(CallDirection),
    /// Show another part of the type hierarchy for the same type (`SPEC-0082` v5 behaviour 13).
    Types(TypeDirection),
    /// Build the method hierarchy again.
    Method,
    /// Close the tool window.
    Close,
}

/// What the tool window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyMode {
    /// Callers or callees of a function.
    Calls,
    /// A part of a type hierarchy.
    Types(TypeDirection),
    /// Which types define a method.
    Method,
}

/// The hierarchy of one function.
pub struct HierarchyView {
    /// The tree as the server gave it.
    original: CallNode,
    /// The tree shown: the original, or sorted by name (`SPEC-0082` v4 behaviour 10).
    root: CallNode,
    sort: bool,
    direction: CallDirection,
    mode: HierarchyMode,
    /// The method of a method hierarchy, for the header.
    method: Option<String>,
    /// Where the hierarchy was asked for, to ask again in the other direction.
    pub(crate) origin: Place,
    expanded: HashSet<NodePath>,
    rows: Vec<Row>,
    selected: usize,
    focus: FocusHandle,
}

impl EventEmitter<HierarchyEvent> for HierarchyView {}

impl HierarchyView {
    /// A tool window for `root`, the hierarchy asked for at `origin`.
    pub fn new(
        root: CallNode,
        direction: CallDirection,
        origin: Place,
        cx: &mut Context<Self>,
    ) -> Self {
        // Every level open (SPEC-0082 v4 behaviour 8).
        let expanded = initially_expanded(&root);
        let rows = rows(&root, &expanded);
        Self {
            original: root.clone(),
            root,
            sort: false,
            direction,
            mode: HierarchyMode::Calls,
            method: None,
            origin,
            expanded,
            rows,
            selected: 0,
            focus: cx.focus_handle(),
        }
    }

    /// The view showing a type hierarchy part or, with `method`, a method hierarchy.
    pub fn with_mode(mut self, mode: HierarchyMode, method: Option<String>) -> Self {
        self.mode = mode;
        self.method = method;
        self
    }

    /// What the tool window shows.
    pub fn mode(&self) -> HierarchyMode {
        self.mode
    }

    /// The visible rows with their marks (`+`, `-`, `!`, `*`), e.g. `"+ Cat"`, for self-tests.
    pub fn marked(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| node(&self.root, &r.path))
            .map(|n| match &n.mark {
                Some(m) => format!("{m} {}", n.name),
                None => n.name.clone(),
            })
            .collect()
    }

    /// Focus handle of the tree.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Which way the tree goes.
    pub fn direction(&self) -> CallDirection {
        self.direction
    }

    /// Runs a toolbar button by its id (behaviour 10); also for self-tests.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "sort" => {
                self.sort = !self.sort;
                let open = self.expanded.clone();
                self.root = if self.sort {
                    sorted(&self.original)
                } else {
                    self.original.clone()
                };
                // Paths change with the order; everything that was open stays open.
                self.expanded = if open.len() <= 1 {
                    open
                } else {
                    initially_expanded(&self.root)
                };
                self.refresh();
            }
            "expand" => {
                self.expanded = initially_expanded(&self.root);
                self.refresh();
            }
            "collapse" => {
                self.expanded = root_expanded();
                self.refresh();
            }
            "refresh" => cx.emit(match self.mode {
                HierarchyMode::Calls => HierarchyEvent::Direction(self.direction),
                HierarchyMode::Types(d) => HierarchyEvent::Types(d),
                HierarchyMode::Method => HierarchyEvent::Method,
            }),
            "close" => cx.emit(HierarchyEvent::Close),
            _ => {}
        }
        cx.notify();
    }

    /// Whether the children are sorted by name.
    pub fn sorted(&self) -> bool {
        self.sort
    }

    /// Names of the visible rows with their depth, in order.
    pub fn visible(&self) -> Vec<(usize, String)> {
        self.rows
            .iter()
            .filter_map(|r| node(&self.root, &r.path).map(|n| (r.depth, n.name.clone())))
            .collect()
    }

    /// Name of the selected row.
    pub fn selected_name(&self) -> Option<String> {
        let r = self.rows.get(self.selected)?;
        node(&self.root, &r.path).map(|n| n.name.clone())
    }

    fn refresh(&mut self) {
        let path = self.rows.get(self.selected).map(|r| r.path.clone());
        self.rows = rows(&self.root, &self.expanded);
        self.selected = path
            .and_then(|p| self.rows.iter().position(|r| r.path == p))
            .unwrap_or(0);
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        let Some(n) = self
            .rows
            .get(self.selected)
            .and_then(|r| node(&self.root, &r.path))
        else {
            return;
        };
        cx.emit(HierarchyEvent::Open(Place {
            path: n
                .location
                .path
                .canonicalize()
                .unwrap_or(n.location.path.clone()),
            line: n.location.range.start_line as usize,
            column: n.location.range.start_character as usize,
        }));
    }

    /// Selects row `i` and expands or collapses it.
    fn toggle_row(&mut self, i: usize) {
        let Some(row) = self.rows.get(i).cloned() else {
            return;
        };
        self.selected = i;
        if row.expanded {
            self.expanded.remove(&row.path);
        } else if row.has_children {
            self.expanded.insert(row.path);
        }
        self.refresh();
    }

    /// Right expands (or moves to the first child), Left collapses (or moves to the parent).
    fn side(&mut self, right: bool) {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return;
        };
        if right {
            if row.has_children && !row.expanded {
                self.expanded.insert(row.path);
                self.refresh();
            } else if row.expanded {
                self.selected += 1;
            }
        } else if row.expanded {
            self.expanded.remove(&row.path);
            self.refresh();
        } else if let Some(parent) = row.path.split_last().map(|(_, p)| p.to_vec())
            && let Some(i) = self.rows.iter().position(|r| r.path == parent)
        {
            self.selected = i;
        }
    }

    /// Up/Down select, Left/Right collapse and expand, Enter jumps, Escape closes (behaviour 7).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.rows.len()) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift {
            return;
        }
        let n = self.rows.len().max(1);
        match k.key.as_str() {
            "down" => self.selected = (self.selected + 1).min(n - 1),
            "up" => self.selected = self.selected.saturating_sub(1),
            "right" => self.side(true),
            "left" => self.side(false),
            "enter" => self.choose(cx),
            "escape" => cx.emit(HierarchyEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn tab(
        &self,
        label: &'static str,
        which: CallDirection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let on = self.direction == which;
        div()
            .id(label)
            .flex_none()
            .px_2()
            .py(px(2.))
            .rounded(px(4.))
            .when(on, |d| d.bg(c(p.tree_selection)))
            .when(!on, |d| {
                d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tree_hover)))
            })
            .child(label)
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                if this.direction != which {
                    cx.emit(HierarchyEvent::Direction(which));
                }
            }))
    }

    fn type_tab(
        &self,
        label: &'static str,
        which: TypeDirection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let on = self.mode == HierarchyMode::Types(which);
        div()
            .id(label)
            .flex_none()
            .px_2()
            .py(px(2.))
            .rounded(px(4.))
            .when(on, |d| d.bg(c(p.tree_selection)))
            .when(!on, |d| {
                d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tree_hover)))
            })
            .child(label)
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                if this.mode != HierarchyMode::Types(which) {
                    cx.emit(HierarchyEvent::Types(which));
                }
            }))
    }

    /// The header: what the hierarchy is of.
    fn heading(&self) -> String {
        fn starred(n: &CallNode) -> Option<&CallNode> {
            if n.mark.as_deref() == Some("*") {
                return Some(n);
            }
            n.children.iter().find_map(starred)
        }
        let ty = starred(&self.root).unwrap_or(&self.root).name.clone();
        match self.mode {
            HierarchyMode::Calls => {
                let what = match self.direction {
                    CallDirection::Callers => "Callers of",
                    CallDirection::Callees => "Callees of",
                };
                format!("{what} {}", short_name(&self.root.name))
            }
            HierarchyMode::Types(TypeDirection::Class) => format!("Class {ty}"),
            HierarchyMode::Types(TypeDirection::Supertypes) => format!("Supertypes of {ty}"),
            HierarchyMode::Types(TypeDirection::Subtypes) => format!("Subtypes of {ty}"),
            HierarchyMode::Method => format!(
                "Method {}",
                self.method.as_deref().unwrap_or(&self.root.name)
            ),
        }
    }

    /// A toolbar button with its declared tooltip (behaviour 10).
    fn tool(
        &self,
        control: &'static idecraft_ui::tooltip::Control,
        glyph: &'static str,
        on: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let id = control
            .name
            .strip_prefix("hierarchy-")
            .unwrap_or(control.name);
        div()
            .id(control.name)
            .tip(control)
            .flex_none()
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .text_color(c(p.ui_muted))
            .when(on, |d| d.bg(c(p.tree_selection)))
            .hover(|s| s.bg(c(p.tree_hover)))
            .child(glyph)
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.press(id, cx);
            }))
    }
}

/// The function's name for the header: without parameters and return type (`apply`).
fn short_name(name: &str) -> &str {
    name.split('(').next().unwrap_or(name).trim()
}

impl Render for HierarchyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let shown: Vec<(Row, CallNode)> = self
            .rows
            .iter()
            .filter_map(|r| {
                node(&self.root, &r.path).map(|n| {
                    let mut n = n.clone();
                    n.children.clear();
                    (r.clone(), n)
                })
            })
            .collect();
        let selected = self.selected;
        let list = uniform_list(
            "hierarchy",
            shown.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let (row, n) = &shown[i];
                        let detail = row_detail(n);
                        let chevron = if !row.has_children {
                            None
                        } else if row.expanded {
                            Some("general/chevronDown")
                        } else {
                            Some("general/chevronRight")
                        };
                        div()
                            .id(("hierarchy-row", i))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(24.))
                            .pr_2()
                            .pl(px(crate::metrics::tree_indent(row.depth)))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(match chevron {
                                // One click on the arrow toggles (SPEC-0275 behaviour 8).
                                Some(key) => div()
                                    .id(("hierarchy-arrow", i))
                                    .flex_none()
                                    .child(icon(key))
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.toggle_row(i);
                                            cx.notify();
                                        },
                                    ))
                                    .into_any_element(),
                                None => div().w(px(16.)).flex_none().into_any_element(),
                            })
                            .when_some(n.mark.clone(), |d, mark| {
                                let color = match mark.as_str() {
                                    "+" => p.success_text,
                                    "!" => p.error_text,
                                    "-" => p.ui_dim,
                                    _ => p.ui_muted,
                                };
                                d.child(
                                    div()
                                        .w(px(10.))
                                        .flex_none()
                                        .text_color(c(color))
                                        .child(mark),
                                )
                            })
                            .child(icon(idecraft_ui::icons::symbol_icon(&n.kind)))
                            .child(div().flex_none().child(row_label(&n.name).to_owned()))
                            .when(n.recursive, |d| {
                                d.child(div().text_color(c(p.ui_dim)).child("(recursive)"))
                            })
                            .child(
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
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                if e.click_count() > 1 {
                                    this.choose(cx);
                                }
                                cx.notify();
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .px_1()
        .flex_1();
        let heading = self.heading();
        let mode = self.mode;
        div()
            .id("hierarchy-panel")
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
                    .gap_2()
                    .px_3()
                    .child(
                        // The heading gives way, so the tabs and buttons always fit (OBS-0027).
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(heading),
                    )
                    .child(div().flex_1().min_w(px(4.)))
                    .when(mode == HierarchyMode::Calls, |d| {
                        d.child(self.tab("Callers", CallDirection::Callers, cx))
                            .child(self.tab("Callees", CallDirection::Callees, cx))
                    })
                    .when(matches!(mode, HierarchyMode::Types(_)), |d| {
                        d.child(self.type_tab("Class", TypeDirection::Class, cx))
                            .child(self.type_tab("Supertypes", TypeDirection::Supertypes, cx))
                            .child(self.type_tab("Subtypes", TypeDirection::Subtypes, cx))
                    })
                    .child(self.tool(
                        crate::tips::declared!(OTHER_TOOL_WINDOWS, "hierarchy-sort"),
                        "↓a",
                        self.sort,
                        cx,
                    ))
                    .child(self.tool(
                        crate::tips::declared!(OTHER_TOOL_WINDOWS, "hierarchy-expand"),
                        "⊞",
                        false,
                        cx,
                    ))
                    .child(self.tool(
                        crate::tips::declared!(OTHER_TOOL_WINDOWS, "hierarchy-collapse"),
                        "⊟",
                        false,
                        cx,
                    ))
                    .child(self.tool(
                        crate::tips::declared!(OTHER_TOOL_WINDOWS, "hierarchy-refresh"),
                        "↻",
                        false,
                        cx,
                    ))
                    .child(
                        div()
                            .id("hierarchy-close")
                            .flex_none()
                            .tip(crate::tips::declared!(
                                OTHER_TOOL_WINDOWS,
                                "hierarchy-close"
                            ))
                            .p(px(2.))
                            .rounded(px(4.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(icon("general/closeSmall"))
                            .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(HierarchyEvent::Close)
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().flex().flex_col().child(list))
            // The marks explained under a method hierarchy (behaviour 14).
            .when(mode == HierarchyMode::Method, |d| {
                d.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child("+ defines the method · - inherits it · ! must define it"),
                )
            })
    }
}
