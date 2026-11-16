//! The Find tool window for Find Usages (`SPEC-0293`): the search's tab, a vertical toolbar, the
//! tree of usages grouped by file and a preview of the selected usage.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use crate::find_usages_model::{Level, Node, PREVIEW_CONTEXT, Row, UsageTree, preview};
use crate::icon_images::icon;
use crate::nav_model::Place;
use crate::palette::Palette;
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;

/// What the tool window asks the workbench to do.
pub enum FindUsagesEvent {
    /// Open a usage, the window staying open.
    Open(Place),
    /// Search again for the same symbol at the same place.
    Rerun,
    /// Stop the running search.
    Stop,
    /// Give the focus back to the editor.
    FocusEditor,
    /// Write the usages to a text file (`SPEC-0293` v9 behaviour 26).
    Export,
    /// Hide the tool window.
    Close,
}

/// Where the search stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindState {
    /// Running since then (behaviour 1).
    Searching(Instant),
    /// Done with a tree.
    Done,
    /// Done without a tree: no usages, stopped, failed (behaviours 9, 11).
    Message(String),
}

/// The Find tool window with one search.
pub struct FindUsagesView {
    symbol: String,
    /// The file name a search of `FindUsagesInFile` is restricted to (v7 behaviour 19).
    in_file: Option<String>,
    /// Where the search was started, for Rerun.
    pub(crate) origin: Place,
    state: FindState,
    tree: Option<UsageTree>,
    note: Option<String>,
    /// File texts for the preview, by absolute path.
    texts: HashMap<PathBuf, String>,
    rows: Vec<Row>,
    selected: usize,
    preview_on: bool,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
}

impl EventEmitter<FindUsagesEvent> for FindUsagesView {}

impl FindUsagesView {
    /// A window searching for `symbol` from `origin`, in the project or only `in_file`.
    pub fn searching(
        symbol: &str,
        in_file: Option<&str>,
        origin: Place,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = Self {
            symbol: symbol.to_owned(),
            in_file: in_file.map(str::to_owned),
            origin,
            state: FindState::Searching(Instant::now()),
            tree: None,
            note: None,
            texts: HashMap::new(),
            rows: Vec::new(),
            selected: 0,
            preview_on: true,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
        };
        Self::tick(cx);
        this
    }

    /// Redraws every half second while searching, for the elapsed seconds.
    fn tick(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let Ok(searching) = this.update(cx, |this, cx| {
                    // A search that never reported back is ended here (v8 behaviour 21).
                    if let FindState::Searching(since) = this.state
                        && crate::find_usages_model::overdue(since.elapsed())
                    {
                        this.state =
                            FindState::Message(crate::find_usages_model::gave_up(&this.symbol));
                        cx.emit(FindUsagesEvent::Stop);
                    }
                    cx.notify();
                    matches!(this.state, FindState::Searching(_))
                }) else {
                    return;
                };
                if !searching {
                    return;
                }
            }
        })
        .detach();
    }

    /// Starts over for a new search (a new ⌥F7 or Rerun).
    pub fn restart(
        &mut self,
        symbol: &str,
        in_file: Option<&str>,
        origin: Place,
        cx: &mut Context<Self>,
    ) {
        self.symbol = symbol.to_owned();
        self.in_file = in_file.map(str::to_owned);
        self.origin = origin;
        self.state = FindState::Searching(Instant::now());
        self.tree = None;
        self.note = None;
        self.rows.clear();
        self.selected = 0;
        Self::tick(cx);
        cx.notify();
    }

    /// Shows the result: `tree`, an optional `note`, and the file texts for the preview.
    pub fn show(
        &mut self,
        tree: UsageTree,
        note: Option<String>,
        texts: HashMap<PathBuf, String>,
        cx: &mut Context<Self>,
    ) {
        // Without a usage there is nothing to show but the words for it (v8 behaviour 21).
        if let Some(nothing) = crate::find_usages_model::finished(&tree) {
            self.message(nothing, cx);
            return;
        }
        let mut tree = tree;
        // The grouping chosen stays for the next search (SPEC-0293 v3 behaviour 14).
        if let Some(old) = &self.tree {
            tree.by_file = old.by_file;
            tree.by_kind = old.by_kind;
            tree.by_module = old.by_module;
            tree.by_package = old.by_package;
            tree.by_structure = old.by_structure;
            // So do the filters and the two grouping variants (v9 behaviours 22–24); what was
            // excluded stays excluded when the same search runs again (behaviour 25).
            tree.show_read = old.show_read;
            tree.show_write = old.show_write;
            tree.show_imports = old.show_imports;
            tree.nested_dirs = old.nested_dirs;
            tree.flatten_modules = old.flatten_modules;
            if old.symbol == tree.symbol && old.in_file == tree.in_file {
                tree.keep_excluded(old);
            }
        }
        self.rows = tree.rows();
        self.selected = if tree.hits.is_empty() {
            0
        } else {
            tree.reveal(0).unwrap_or(0)
        };
        self.rows = tree.rows();
        self.tree = Some(tree);
        self.note = note;
        self.texts = texts;
        self.state = FindState::Done;
        cx.notify();
    }

    /// Ends the search with a message instead of a tree (behaviours 9–11).
    pub fn message(&mut self, text: String, cx: &mut Context<Self>) {
        self.state = FindState::Message(text);
        cx.notify();
    }

    /// The tab's title, `<symbol> in Project Files` or `<symbol> in <file name>` (behaviours 1
    /// and 19).
    pub fn title(&self) -> String {
        crate::find_usages_model::search_title(&self.symbol, self.in_file.as_deref())
    }

    /// The searched symbol.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Whether the search is restricted to the file it was started in (v7 behaviour 19).
    pub fn in_file(&self) -> bool {
        self.in_file.is_some()
    }

    /// Where the search stands.
    pub fn state(&self) -> &FindState {
        &self.state
    }

    /// The tree, once there is a result.
    pub fn tree(&self) -> Option<&UsageTree> {
        self.tree.as_ref()
    }

    /// The visible rows as depth and label.
    pub fn visible(&self) -> Vec<(usize, String)> {
        self.rows
            .iter()
            .map(|r| (r.depth, r.label.clone()))
            .collect()
    }

    /// Label of the selected row.
    pub fn selected_label(&self) -> Option<String> {
        self.rows.get(self.selected).map(|r| r.label.clone())
    }

    /// Focus handle of the tree.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn refresh(&mut self) {
        let node = self.rows.get(self.selected).map(|r| r.node.clone());
        if let Some(t) = &self.tree {
            self.rows = t.rows();
        }
        self.selected = node
            .and_then(|n| self.rows.iter().position(|r| r.node == n))
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1));
    }

    fn open_selected(&mut self, cx: &mut Context<Self>) {
        let Some(Row {
            node: Node::Usage(i),
            ..
        }) = self.rows.get(self.selected)
        else {
            return;
        };
        if let Some(h) = self.tree.as_ref().and_then(|t| t.hits.get(*i)) {
            cx.emit(FindUsagesEvent::Open(h.place.clone()));
        }
    }

    /// Selects the next (or previous) usage and opens it (behaviour 8); `true` if there was one.
    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let node = self.rows.get(self.selected).map(|r| r.node.clone());
        let Some(tree) = self.tree.as_mut() else {
            return false;
        };
        let Some(hit) = tree.step(node.as_ref(), forward) else {
            return false;
        };
        let row = tree.reveal(hit);
        self.rows = tree.rows();
        self.selected = row.unwrap_or(0);
        self.scroll
            .scroll_to_item(self.selected, gpui::ScrollStrategy::Center);
        self.open_selected(cx);
        cx.notify();
        true
    }

    fn set_expanded(&mut self, i: usize, open: bool) {
        let Some(row) = self.rows.get(i).cloned() else {
            return;
        };
        if let Some(t) = self.tree.as_mut() {
            t.set_expanded(&row.node, open);
        }
        self.selected = i;
        self.refresh();
    }

    /// Right expands or moves in, Left collapses or moves to the parent (behaviour 8).
    fn side(&mut self, right: bool) {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return;
        };
        if right {
            if row.expandable && !row.expanded {
                self.set_expanded(self.selected, true);
            } else if row.expanded {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
            }
        } else if row.expanded {
            self.set_expanded(self.selected, false);
        } else if row.depth > 0
            && let Some(p) = self.rows[..self.selected]
                .iter()
                .rposition(|r| r.depth < row.depth)
        {
            self.selected = p;
        }
    }

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
            "enter" => self.open_selected(cx),
            "escape" => cx.emit(FindUsagesEvent::FocusEditor),
            _ => return,
        }
        self.scroll
            .scroll_to_item(self.selected, gpui::ScrollStrategy::Top);
        cx.stop_propagation();
        cx.notify();
    }

    /// Runs a toolbar button by its id (behaviour 7); also for self-tests.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "rerun" => cx.emit(FindUsagesEvent::Rerun),
            "stop" => cx.emit(FindUsagesEvent::Stop),
            "previous" => {
                self.step(false, cx);
            }
            "next" => {
                self.step(true, cx);
            }
            "expand" => {
                if let Some(t) = self.tree.as_mut() {
                    t.expand_all();
                }
                self.refresh();
            }
            "collapse" => {
                if let Some(t) = self.tree.as_mut() {
                    t.collapse_all();
                }
                self.refresh();
            }
            "group" | "group-kind" | "group-module" | "group-package" | "group-structure" => {
                if let Some(t) = self.tree.as_mut() {
                    let flag = match id {
                        "group" => &mut t.by_file,
                        "group-kind" => &mut t.by_kind,
                        "group-module" => &mut t.by_module,
                        "group-package" => &mut t.by_package,
                        _ => &mut t.by_structure,
                    };
                    *flag = !*flag;
                }
                self.refresh();
            }
            "preview" => self.preview_on = !self.preview_on,
            "close" => cx.emit(FindUsagesEvent::Close),
            "export" => cx.emit(FindUsagesEvent::Export),
            _ => {
                self.change(id, cx);
            }
        }
        cx.notify();
    }

    /// The filters, the two grouping variants and Exclude/Include by the toolbar's ids
    /// (`SPEC-0293` v9 behaviours 22–25): `show-read`, `show-write`, `show-imports`,
    /// `group-dirs`, `flatten-modules`, `exclude`, `include`. Returns whether anything changed:
    /// Flatten Modules needs Group by Module, Exclude and Include a row they change.
    pub fn change(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let row = self.rows.get(self.selected).cloned();
        let Some(t) = self.tree.as_mut() else {
            return false;
        };
        let flip = |flag: &mut bool| {
            *flag = !*flag;
            true
        };
        let changed = match id {
            "show-read" => flip(&mut t.show_read),
            "show-write" => flip(&mut t.show_write),
            "show-imports" => flip(&mut t.show_imports),
            "group-dirs" => flip(&mut t.nested_dirs),
            "flatten-modules" if t.by_module => flip(&mut t.flatten_modules),
            "exclude" | "include" => row
                .as_ref()
                .is_some_and(|r| t.set_excluded(&r.node, id == "exclude")),
            _ => false,
        };
        if !changed {
            return false;
        }
        self.refresh();
        // After excluding one usage the next usage shown is selected, so a row of them goes
        // quickly and the key never takes a whole group by accident.
        if id == "exclude"
            && row.is_some_and(|r| matches!(r.node, Node::Usage(_)))
            && let Some(next) = (self.selected + 1..self.rows.len())
                .find(|&i| matches!(self.rows[i].node, Node::Usage(_)))
        {
            self.selected = next;
        }
        self.scroll
            .scroll_to_item(self.selected, gpui::ScrollStrategy::Center);
        cx.notify();
        true
    }

    /// Selects row `i` of the tree, as a click on it does (checks).
    pub fn select_row(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = i.min(self.rows.len().saturating_sub(1));
        cx.notify();
    }

    /// The place of the selected usage, or of the first usage of the selected group.
    pub fn selected_place(&self) -> Option<Place> {
        let t = self.tree.as_ref()?;
        let row = self.rows.get(self.selected)?;
        if matches!(row.node, Node::Target | Node::Group) {
            return None;
        }
        t.hits.get(t.hit_of(&row.node)?).map(|h| h.place.clone())
    }

    /// The usages as the text Export to Text File writes, and how many it holds; `None`
    /// without a result (`SPEC-0293` v9 behaviour 26).
    pub fn export(&self) -> Option<(String, usize)> {
        let t = self.tree.as_ref()?;
        Some((t.export_text(), t.included().len()))
    }

    /// A toolbar button with its declared tooltip (`SPEC-0294` behaviour 9).
    fn tool(
        &self,
        control: &'static idecraft_ui::tooltip::Control,
        glyph: Option<&'static str>,
        icon_key: Option<&'static str>,
        on: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let id = control
            .name
            .strip_prefix("find-usages-")
            .unwrap_or(control.name);
        div()
            .id(control.name)
            .tip(control)
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .text_color(c(p.ui_muted))
            .when(on, |d| d.bg(c(p.tree_selection)))
            .hover(|s| s.bg(c(p.toolbar_hover)))
            .when_some(icon_key, |d, k| d.child(icon(k)))
            .when_some(glyph, |d, g| d.child(g))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.press(id, cx);
            }))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let searching = matches!(self.state, FindState::Searching(_));
        let by_file = self.tree.as_ref().is_none_or(|t| t.by_file);
        let by_kind = self.tree.as_ref().is_none_or(|t| t.by_kind);
        let by_module = self.tree.as_ref().is_none_or(|t| t.by_module);
        let by_package = self.tree.as_ref().is_none_or(|t| t.by_package);
        let by_structure = self.tree.as_ref().is_none_or(|t| t.by_structure);
        div()
            .flex_none()
            .w(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .py_1()
            .border_r_1()
            .border_color(c(p.separator))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-rerun"),
                None,
                Some("general/refresh"),
                false,
                cx,
            ))
            .when(searching, |d| {
                d.child(self.tool(
                    declared!(FIND_USAGES_TOOLBAR, "find-usages-stop"),
                    None,
                    Some("run/stop"),
                    false,
                    cx,
                ))
            })
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-previous"),
                Some("↑"),
                None,
                false,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-next"),
                Some("↓"),
                None,
                false,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-expand"),
                Some("⊞"),
                None,
                false,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-collapse"),
                Some("⊟"),
                None,
                false,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-group-kind"),
                Some("K"),
                None,
                by_kind,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-group-module"),
                Some("M"),
                None,
                by_module,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-group-package"),
                Some("P"),
                None,
                by_package,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-group"),
                Some("▤"),
                None,
                by_file,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-group-structure"),
                Some("C"),
                None,
                by_structure,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-preview"),
                Some("◧"),
                None,
                self.preview_on,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-close"),
                None,
                Some("general/closeSmall"),
                false,
                cx,
            ))
    }

    /// A second column beside the toolbar (`SPEC-0293` v9 behaviours 22–24 and 26): the three
    /// filters, Directory Structure, Flatten Modules and the export. A column of its own, so
    /// the first one stays as short as it was in a low tool window.
    fn filters(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let tree = self.tree.as_ref();
        let show_read = tree.is_none_or(|t| t.show_read);
        let show_write = tree.is_none_or(|t| t.show_write);
        let show_imports = tree.is_none_or(|t| t.show_imports);
        let nested_dirs = tree.is_some_and(|t| t.nested_dirs);
        let flat = tree.is_none_or(|t| t.flatten_modules && t.by_module);
        div()
            .flex_none()
            .w(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .py_1()
            .border_r_1()
            .border_color(c(p.separator))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-show-read"),
                Some("R"),
                None,
                show_read,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-show-write"),
                Some("W"),
                None,
                show_write,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-show-imports"),
                Some("I"),
                None,
                show_imports,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-group-dirs"),
                Some("D"),
                None,
                nested_dirs,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-flatten-modules"),
                Some("F"),
                None,
                flat,
                cx,
            ))
            .child(self.tool(
                declared!(FIND_USAGES_TOOLBAR, "find-usages-export"),
                Some("⇪"),
                None,
                false,
                cx,
            ))
    }

    /// Text with the chars in `mark` on the search-result background.
    fn marked(text: &str, mark: Option<Range<usize>>) -> gpui::AnyElement {
        let p = Palette::current();
        let Some(mark) = mark.filter(|m| !m.is_empty()) else {
            return div().child(text.to_owned()).into_any_element();
        };
        let chars: Vec<char> = text.chars().collect();
        let part = |r: Range<usize>| {
            chars[r.start.min(chars.len())..r.end.min(chars.len())]
                .iter()
                .collect::<String>()
        };
        let bg = p
            .scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
            .unwrap_or(p.selection);
        div()
            .flex()
            .whitespace_nowrap()
            .child(part(0..mark.start))
            .child(div().bg(gpui::rgb(bg)).child(part(mark.clone())))
            .child(part(mark.end..chars.len()))
            .into_any_element()
    }

    fn preview_pane(&self) -> gpui::AnyElement {
        let p = Palette::current();
        let lines = self
            .rows
            .get(self.selected)
            .and_then(|r| {
                let t = self.tree.as_ref()?;
                let h = t.hits.get(t.hit_of(&r.node)?)?;
                let text = self.texts.get(&h.place.path)?;
                Some(preview(
                    text,
                    h.place.line,
                    h.place.column,
                    h.mark.len(),
                    PREVIEW_CONTEXT,
                ))
            })
            .unwrap_or_default();
        div()
            .id("find-usages-preview")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .border_l_1()
            .border_color(c(p.separator))
            .py_1()
            .font_family(crate::fonts::editor_family())
            .children(lines.into_iter().map(|l| {
                div()
                    .flex()
                    .h(px(20.))
                    .items_center()
                    .when(l.current, |d| d.bg(gpui::rgb(p.caret_row)))
                    .child(
                        div()
                            .w(px(44.))
                            .flex_none()
                            .pr_2()
                            .flex()
                            .justify_end()
                            .text_color(gpui::rgb(p.line_number))
                            .child(l.number.to_string()),
                    )
                    .child(Self::marked(&l.text, l.mark))
            }))
            .into_any_element()
    }

    fn body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        match &self.state {
            FindState::Searching(since) => {
                return div()
                    .p_3()
                    .text_color(c(p.ui_muted))
                    .child(format!(
                        "Searching for usages of {}… {} s",
                        self.symbol,
                        since.elapsed().as_secs()
                    ))
                    .into_any_element();
            }
            FindState::Message(m) => {
                return div()
                    .p_3()
                    .text_color(c(p.ui_muted))
                    .child(m.clone())
                    .into_any_element();
            }
            FindState::Done => {}
        }
        let rows = self.rows.clone();
        let hits = self
            .tree
            .as_ref()
            .map(|t| t.hits.clone())
            .unwrap_or_default();
        let target_kind = self.tree.as_ref().and_then(|t| t.target_kind.clone());
        let selected = self.selected;
        let list = uniform_list(
            "find-usages",
            rows.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let row = &rows[i];
                        let chevron = if !row.expandable {
                            None
                        } else if row.expanded {
                            Some("general/chevronDown")
                        } else {
                            Some("general/chevronRight")
                        };
                        let lead = match &row.node {
                            Node::Target => icon("general/search"),
                            Node::Group => div().w(px(16.)).flex_none().into_any_element(),
                            // Each grouping level has its icon (SPEC-0293 v3 behaviour 14).
                            Node::Bucket(_) => match row.level {
                                Some(Level::File) => {
                                    icon(idecraft_ui::icons::file_icon(&row.label))
                                }
                                Some(Level::Module) => {
                                    icon(idecraft_ui::icons::symbol_icon("module"))
                                }
                                Some(Level::Package | Level::Dir) => {
                                    icon(idecraft_ui::icons::symbol_icon("package"))
                                }
                                Some(Level::ModuleGroup) => {
                                    icon(idecraft_ui::icons::symbol_icon("module"))
                                }
                                Some(Level::Class) => {
                                    icon(idecraft_ui::icons::symbol_icon("class"))
                                }
                                Some(Level::Method) => {
                                    icon(idecraft_ui::icons::symbol_icon("method"))
                                }
                                _ => div().w(px(16.)).flex_none().into_any_element(),
                            },
                            Node::Usage(_) => div().w(px(4.)).flex_none().into_any_element(),
                        };
                        let label = match &row.node {
                            Node::Usage(u) => {
                                let h = &hits[*u];
                                let number = format!("{}  ", h.place.line + 1);
                                let shift = number.chars().count();
                                Self::marked(
                                    &row.label,
                                    Some(h.mark.start + shift..h.mark.end + shift),
                                )
                            }
                            // The target names its kind first (`method apply`).
                            Node::Target => div()
                                .flex()
                                .gap_1()
                                .when_some(target_kind.clone(), |d, k| {
                                    d.child(div().text_color(c(p.ui_dim)).child(k))
                                })
                                .child(row.label.clone())
                                .into_any_element(),
                            _ => div().child(row.label.clone()).into_any_element(),
                        };
                        div()
                            .id(("find-usages-row", i))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(24.))
                            .pr_2()
                            .pl(px(crate::metrics::tree_indent(row.depth)))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            // An excluded usage is struck through (v9 behaviour 25).
                            .when(row.excluded, |d| d.line_through().text_color(c(p.ui_dim)))
                            .child(match chevron {
                                Some(key) => div()
                                    .id(("find-usages-arrow", i))
                                    .flex_none()
                                    .child(icon(key))
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            let open =
                                                this.rows.get(i).is_some_and(|r| !r.expanded);
                                            this.set_expanded(i, open);
                                            cx.notify();
                                        },
                                    ))
                                    .into_any_element(),
                                None => div().w(px(16.)).flex_none().into_any_element(),
                            })
                            .child(lead)
                            .child(
                                div()
                                    .flex_shrink()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(label),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_color(c(if i == selected {
                                        p.ui_muted
                                    } else {
                                        p.ui_dim
                                    }))
                                    .child(row.detail.clone()),
                            )
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                if e.click_count() > 1 {
                                    this.open_selected(cx);
                                }
                                cx.notify();
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .px_1()
        .flex_1();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .when_some(self.note.clone(), |d, note| {
                d.child(div().px_3().py_1().text_color(c(p.ui_muted)).child(note))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(list),
                    )
                    .when(self.preview_on, |d| d.child(self.preview_pane())),
            )
            .into_any_element()
    }
}

impl Render for FindUsagesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        div()
            .id("find-usages-panel")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title("Find"))
                    .child(
                        div()
                            .px_2()
                            .py(px(2.))
                            .rounded(px(4.))
                            .bg(c(p.tab_selected))
                            .child(self.title()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("find-usages-close")
                            .tip(declared!(FIND_USAGES_TOOLBAR, "find-usages-hide"))
                            .p(px(2.))
                            .rounded(px(4.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(icon("general/closeSmall"))
                            .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(FindUsagesEvent::Close)
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .child(self.toolbar(cx))
                    .child(self.filters(cx))
                    .child(self.body(cx)),
            )
    }
}
