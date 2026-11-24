//! The TODO tool window (`SPEC-0334` behaviours 13–22): a toolbar, the tree of project, modules,
//! folders, files and items, and a preview of the selected item. What the tree shows is decided
//! in [`crate::todo_model`]; the workbench scans, sets what was found and carries out what the
//! window asks for ([`crate::workbench_view`], `workbench_todo.rs`).

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use crate::code_preview::ExcerptLine;
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::tips::{Tipped, declared};
use crate::todo_model::{
    Entry, Filter, Grouping, NO_FILE, Node, REMOTE, SCANNING, Scope, ancestors, closable,
    count_label, file_tree, notes, nothing_found, parent, step, totals_label, tree,
};
use crate::workbench_view::c;

/// Lines of the preview before and after the item's line (behaviour 20).
const PREVIEW_BEFORE: usize = 6;
const PREVIEW_AFTER: usize = 8;

/// The largest file read for the preview (behaviour 20).
const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

/// What the window asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoEvent {
    /// Open the file and put the caret at the 0-based line and column; `keep_focus` leaves the
    /// focus in the window (behaviours 18, 19).
    Open {
        /// The file, as [`Entry::path`].
        path: String,
        /// 0-based line.
        line: usize,
        /// 0-based column, in characters.
        column: usize,
        /// The focus stays in the window.
        keep_focus: bool,
    },
    /// Open the file (behaviour 18).
    OpenFile(String),
}

/// What was left out of the project's scan (behaviour 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Left {
    /// The list is cut at the cap.
    pub truncated: bool,
    /// Files not read.
    pub not_read: usize,
    /// Files with a match that were not checked.
    pub not_checked: usize,
}

/// The TODO tool window.
pub struct TodoToolView {
    /// The project folder, for the preview's files.
    root: PathBuf,
    /// The project folder's name.
    project: String,
    /// The window is on a remote project: nothing is scanned (behaviour 23).
    remote: bool,
    scope: Scope,
    grouping: Grouping,
    filter: Filter,
    /// The project's files with items; `None` until the first scan is done.
    entries: Option<Vec<Entry>>,
    left: Left,
    /// The current file's items; `None` without a file.
    file: Option<Entry>,
    /// The texts of files with unsaved changes, by [`Entry::path`], for the preview.
    unsaved: HashMap<String, Arc<str>>,
    /// The closed nodes, by [`Node::key`].
    closed: BTreeSet<String>,
    nodes: Vec<Node>,
    selected: usize,
    preview_on: bool,
    previews: HashMap<String, Vec<ExcerptLine>>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<TodoEvent> for TodoToolView {}

impl TodoToolView {
    /// The window of the project at `root`, before its first scan.
    pub fn new(root: PathBuf, remote: bool, cx: &mut Context<Self>) -> Self {
        let project = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            root,
            project,
            remote,
            scope: Scope::default(),
            grouping: Grouping::default(),
            filter: Filter::default(),
            entries: None,
            left: Left::default(),
            file: None,
            unsaved: HashMap::new(),
            closed: BTreeSet::new(),
            nodes: Vec::new(),
            selected: 0,
            preview_on: true,
            previews: HashMap::new(),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// Focus handle of the window.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The scope shown.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The grouping toggles.
    pub fn grouping(&self) -> Grouping {
        self.grouping
    }

    /// The filter.
    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// The visible rows.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The selected row.
    pub fn selected(&self) -> Option<&Node> {
        self.nodes.get(self.selected)
    }

    /// Whether the project's first scan is done.
    pub fn scanned(&self) -> bool {
        self.entries.is_some()
    }

    /// Whether the preview is shown.
    pub fn preview_on(&self) -> bool {
        self.preview_on
    }

    /// The rows of the scope with the nodes in `closed` closed.
    fn rows(&self, closed: &BTreeSet<String>) -> Vec<Node> {
        match self.scope {
            Scope::Project => tree(
                &self.project,
                self.entries.as_deref().unwrap_or_default(),
                self.grouping,
                self.filter,
                closed,
            ),
            Scope::File => self
                .file
                .as_ref()
                .map(|f| file_tree(f, self.filter, closed))
                .unwrap_or_default(),
        }
    }

    /// Builds the visible rows again, keeping the selection on its node, else at its index.
    fn rebuild(&mut self) {
        let key = self.selected().map(Node::key);
        self.nodes = self.rows(&self.closed);
        self.selected = key
            .and_then(|k| self.nodes.iter().position(|n| n.key() == k))
            .unwrap_or(self.selected)
            .min(self.nodes.len().saturating_sub(1));
    }

    /// A scan of the project is done: its files with items, what was left out, and the texts of
    /// the files with unsaved changes.
    pub fn set_project(
        &mut self,
        entries: Vec<Entry>,
        left: Left,
        unsaved: HashMap<String, Arc<str>>,
        cx: &mut Context<Self>,
    ) {
        self.unsaved = unsaved;
        self.previews.clear();
        if self.entries.as_ref() != Some(&entries) || self.left != left {
            self.entries = Some(entries);
            self.left = left;
            self.rebuild();
        }
        cx.notify();
    }

    /// The current file's items, or `None` without a file; `text` is the editor's text when it
    /// has unsaved changes.
    pub fn set_file(
        &mut self,
        file: Option<Entry>,
        text: Option<Arc<str>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(file) = &file {
            // The preview shows the editor's text while it is unsaved, else the file.
            match text {
                Some(text) => {
                    self.unsaved.insert(file.path.clone(), text);
                }
                None => {
                    self.unsaved.remove(&file.path);
                }
            }
            self.previews.clear();
        }
        if self.file != file {
            self.previews.clear();
            self.file = file;
            if self.scope == Scope::File {
                self.rebuild();
            }
            cx.notify();
        }
    }

    /// Shows `scope` (behaviour 14).
    pub fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        if self.scope != scope {
            self.scope = scope;
            self.selected = 0;
            self.rebuild();
            cx.notify();
        }
    }

    /// Selects the row at `index`.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = index.min(self.nodes.len().saturating_sub(1));
        self.scroll
            .scroll_to_item(self.selected, gpui::ScrollStrategy::Center);
        cx.notify();
    }

    /// Opens or closes the row at `index`.
    fn set_open(&mut self, index: usize, open: bool) {
        let Some(node) = self.nodes.get(index).filter(|n| n.open().is_some()) else {
            return;
        };
        let key = node.key();
        if open {
            self.closed.remove(&key);
        } else {
            self.closed.insert(key);
        }
        self.rebuild();
    }

    /// Enter, F4 or a double click on the selected row (behaviour 18).
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        match self.selected().cloned() {
            Some(Node::Item {
                path, line, column, ..
            }) => cx.emit(TodoEvent::Open {
                path,
                line,
                column,
                keep_focus: false,
            }),
            Some(Node::File { path, .. }) => cx.emit(TodoEvent::OpenFile(path)),
            Some(node) => {
                if let Some(open) = node.open() {
                    self.set_open(self.selected, !open);
                    cx.notify();
                }
            }
            None => {}
        }
    }

    /// `NextOccurence` and `PreviousOccurence` (behaviour 19): selects the next or previous item,
    /// opens the nodes above it and asks for its place; `false` at the ends.
    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let all = self.rows(&BTreeSet::new());
        let selected = self.selected().map(Node::key);
        let Some(target) = step(&all, selected.as_deref(), forward) else {
            return false;
        };
        for key in ancestors(&all, target) {
            self.closed.remove(&key);
        }
        self.nodes = self.rows(&self.closed);
        let key = all[target].key();
        if let Some(at) = self.nodes.iter().position(|n| n.key() == key) {
            self.select(at, cx);
        }
        if let Node::Item {
            path, line, column, ..
        } = &all[target]
        {
            cx.emit(TodoEvent::Open {
                path: path.clone(),
                line: *line,
                column: *column,
                keep_focus: true,
            });
        }
        cx.notify();
        true
    }

    /// Runs a toolbar button by the last part of its control's name (behaviours 16, 17, 19–21);
    /// also what the grouping actions and the window tests press.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "previous" => {
                self.step(false, cx);
            }
            "next" => {
                self.step(true, cx);
            }
            "group-modules" => self.grouping.modules = !self.grouping.modules,
            "group-packages" => self.grouping.packages = !self.grouping.packages,
            // Flatten needs the folders (behaviour 16).
            "flatten" if self.grouping.packages => self.grouping.flatten = !self.grouping.flatten,
            "expand" => self.closed.clear(),
            "collapse" => self.closed = closable(&self.rows(&BTreeSet::new())),
            "preview" => self.preview_on = !self.preview_on,
            _ => {
                let Some(filter) = id
                    .strip_prefix("filter-")
                    .and_then(|name| Filter::ALL.into_iter().find(|f| f.id() == name))
                else {
                    return;
                };
                self.filter = filter;
            }
        }
        self.rebuild();
        cx.notify();
    }

    /// The keys of the tree (behaviour 18).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.nodes.len()) {
            self.select(to, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift {
            return;
        }
        let last = self.nodes.len().saturating_sub(1);
        match k.key.as_str() {
            "down" => self.select((self.selected + 1).min(last), cx),
            "up" => self.select(self.selected.saturating_sub(1), cx),
            "left" => match self.selected().and_then(Node::open) {
                Some(true) => self.set_open(self.selected, false),
                _ => {
                    if let Some(up) = parent(&self.nodes, self.selected) {
                        self.select(up, cx);
                    }
                }
            },
            "right" => self.set_open(self.selected, true),
            "enter" | "f4" => self.activate(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The preview of the selected item: the lines around it, its line marked (behaviour 20).
    fn preview(&mut self) -> Vec<ExcerptLine> {
        let Some(Node::Item {
            path, line, column, ..
        }) = self.selected().cloned()
        else {
            return Vec::new();
        };
        let key = format!("{path}:{line}:{column}");
        if let Some(lines) = self.previews.get(&key) {
            return lines.clone();
        }
        let file = self.root.join(&path);
        let text = self.unsaved.get(&path).cloned().or_else(|| {
            let small = std::fs::symlink_metadata(&file)
                .is_ok_and(|m| m.is_file() && m.len() <= MAX_PREVIEW_BYTES);
            small
                .then(|| std::fs::read_to_string(&file).ok())
                .flatten()
                .map(Arc::from)
        });
        let lines = text.map_or_else(Vec::new, |text| {
            crate::code_preview::excerpt(&file, &text, line, None, PREVIEW_BEFORE, PREVIEW_AFTER)
        });
        self.previews.insert(key, lines.clone());
        lines
    }

    /// A toolbar button with its declared tooltip; `label` is drawn, or `icon_key`'s icon.
    fn tool(
        &self,
        control: &'static ferris_idea_ui::tooltip::Control,
        label: &'static str,
        on: bool,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = Palette::current();
        let id = control.name.strip_prefix("todo-").unwrap_or(control.name);
        div()
            .id(control.name)
            .tip(control)
            .h(px(22.))
            .min_w(px(22.))
            .px(px(5.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .text_color(c(if enabled { p.ui_muted } else { p.ui_dim }))
            .when(on, |d| d.bg(c(p.tree_selection)))
            .when(enabled, |d| d.hover(|s| s.bg(c(p.toolbar_hover))))
            .child(label)
            .semantics(
                Semantics::new(Role::Button)
                    .name(control.title)
                    .test_id(control.name)
                    .selected(on)
                    .action(AxAction::Click)
                    .effects(&[Effect::None]),
            )
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                window.focus(&this.focus);
                this.press(id, cx);
            }))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let gap = || div().w(px(1.)).h(px(14.)).mx_1().bg(c(p.separator));
        let project = self.scope == Scope::Project;
        div()
            .id("todo-toolbar")
            .flex_none()
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap(px(2.))
            .border_b_1()
            .border_color(c(p.separator))
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-previous"),
                "↑",
                false,
                true,
                cx,
            ))
            .child(self.tool(declared!(TODO_TOOLBAR, "todo-next"), "↓", false, true, cx))
            .child(gap())
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-filter-all"),
                Filter::All.label(),
                self.filter == Filter::ALL[0],
                true,
                cx,
            ))
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-filter-todo"),
                Filter::ALL[1].label(),
                self.filter == Filter::ALL[1],
                true,
                cx,
            ))
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-filter-fixme"),
                Filter::ALL[2].label(),
                self.filter == Filter::ALL[2],
                true,
                cx,
            ))
            .child(gap())
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-group-modules"),
                "M",
                self.grouping.modules,
                project,
                cx,
            ))
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-group-packages"),
                "P",
                self.grouping.packages,
                project,
                cx,
            ))
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-flatten"),
                "F",
                self.grouping.packages && self.grouping.flatten,
                project && self.grouping.packages,
                cx,
            ))
            .child(gap())
            .child(self.tool(declared!(TODO_TOOLBAR, "todo-expand"), "⊞", false, true, cx))
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-collapse"),
                "⊟",
                false,
                true,
                cx,
            ))
            .child(self.tool(
                declared!(TODO_TOOLBAR, "todo-preview"),
                "◧",
                self.preview_on,
                true,
                cx,
            ))
    }

    /// Text with the characters in `mark` set off from the rest.
    fn marked(text: &str, mark: &Range<usize>) -> gpui::Div {
        let p = Palette::current();
        let chars: Vec<char> = text.chars().collect();
        let part = |r: Range<usize>| {
            chars[r.start.min(chars.len())..r.end.min(chars.len())]
                .iter()
                .collect::<String>()
        };
        div()
            .flex()
            .whitespace_nowrap()
            .child(div().text_color(c(p.ui_dim)).child(part(0..mark.start)))
            .child(part(mark.clone()))
            .child(
                div()
                    .text_color(c(p.ui_dim))
                    .child(part(mark.end..chars.len())),
            )
    }

    /// What the window says in place of the tree, if anything (behaviour 22).
    pub fn message(&self) -> Option<String> {
        if self.remote {
            return Some(REMOTE.into());
        }
        match self.scope {
            Scope::Project if self.entries.is_none() => Some(SCANNING.into()),
            Scope::File if self.file.is_none() => Some(NO_FILE.into()),
            _ => {
                let items = self.nodes.iter().any(Node::is_item)
                    || self.nodes.iter().any(|n| match n {
                        Node::Project { items, .. } | Node::File { items, .. } => *items > 0,
                        _ => false,
                    });
                (!items).then(|| nothing_found(self.filter))
            }
        }
    }
}

impl Render for TodoToolView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let nodes = self.nodes.clone();
        let selected = self.selected;
        let count = nodes.len();
        let message = self.message();
        let preview = (self.preview_on && message.is_none()).then(|| self.preview());
        let list = uniform_list(
            "todo-tree",
            count,
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let node = &nodes[i];
                        let on = i == selected;
                        let chevron = node.open();
                        let name = match node {
                            Node::Project {
                                name, items, files, ..
                            } => {
                                format!("project {name}, {}", totals_label(*items, *files))
                            }
                            Node::Module { name, items, .. } => {
                                format!("module {name}, {}", count_label(*items))
                            }
                            Node::Folder { name, items, .. } => {
                                format!("folder {name}, {}", count_label(*items))
                            }
                            Node::File { path, items, .. } => {
                                format!("{path}, {}", count_label(*items))
                            }
                            Node::Item {
                                path, line, text, ..
                            } => format!("{path}:{} {text}", line + 1),
                        };
                        let mut semantics = Semantics::new(Role::TreeItem)
                            .name(name)
                            .test_id(format!("todo-tree-{}", node.key()))
                            .selected(on)
                            .action(AxAction::Click)
                            .effects(&[Effect::None]);
                        if let Some(open) = chevron {
                            semantics = semantics.expanded(open);
                        }
                        let dim =
                            |text: String| div().flex_none().text_color(c(p.ui_dim)).child(text);
                        div()
                            .id(("todo-node", i))
                            .relative()
                            .w_full()
                            .h(px(24.))
                            .pr_2()
                            .pl(px(crate::metrics::tree_indent(node.depth())))
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(match chevron {
                                Some(open) => div()
                                    .id(("todo-arrow", i))
                                    .flex_none()
                                    .child(icon(if open {
                                        "general/chevronDown"
                                    } else {
                                        "general/chevronRight"
                                    }))
                                    .on_click(cx.listener(
                                        move |this: &mut Self, _: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.set_open(i, !open);
                                            cx.notify();
                                        },
                                    ))
                                    .into_any_element(),
                                None => div().w(px(16.)).flex_none().into_any_element(),
                            })
                            .map(|d| match node {
                                Node::Project {
                                    name, items, files, ..
                                } => d
                                    .child(icon("nodes/module"))
                                    .child(div().flex_none().child(name.clone()))
                                    .child(dim(totals_label(*items, *files))),
                                Node::Module { name, items, .. } => d
                                    .child(icon("nodes/module"))
                                    .child(div().flex_none().child(name.clone()))
                                    .child(dim(count_label(*items))),
                                Node::Folder { name, items, .. } => d
                                    .child(icon("nodes/folder"))
                                    .child(div().flex_none().child(name.clone()))
                                    .child(dim(count_label(*items))),
                                Node::File {
                                    name,
                                    folder,
                                    items,
                                    ..
                                } => d
                                    .child(icon(ferris_idea_ui::icons::file_icon(name)))
                                    .child(div().flex_none().child(name.clone()))
                                    .when(!folder.is_empty(), |d| d.child(dim(folder.clone())))
                                    .child(dim(count_label(*items))),
                                Node::Item {
                                    line, text, mark, ..
                                } => d
                                    .child(dim((line + 1).to_string()))
                                    .child(Self::marked(text, mark).flex_1().overflow_hidden()),
                            })
                            .semantics(semantics)
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                                window.focus(&this.focus);
                                this.select(i, cx);
                                if e.click_count() > 1 {
                                    this.activate(cx);
                                }
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .px_1()
        .size_full();
        let notes = if self.scope == Scope::Project && !self.remote {
            notes(
                self.left.truncated,
                self.left.not_read,
                self.left.not_checked,
            )
        } else {
            Vec::new()
        };
        div()
            .id("todo-tool-window")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .overflow_hidden()
            .on_key_down(cx.listener(Self::on_key))
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when_some(message.clone(), |d, text| {
                        d.child(
                            div()
                                .id("todo-message")
                                .px_3()
                                .py_2()
                                .text_color(c(p.ui_dim))
                                .child(text),
                        )
                    })
                    .when(message.is_none(), |d| {
                        d.child(div().flex_1().min_w_0().h_full().py_1().child(list))
                    })
                    .when_some(preview, |d, lines| {
                        d.child(
                            div()
                                .id("todo-preview-pane")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_hidden()
                                .border_l_1()
                                .border_color(c(p.separator))
                                .py_1()
                                .child(crate::code_preview::render(&lines)),
                        )
                    }),
            )
            .children(notes.into_iter().map(|line| {
                div()
                    .flex_none()
                    .px_3()
                    .py(px(2.))
                    .text_color(c(p.ui_dim))
                    .child(line)
            }))
    }
}
