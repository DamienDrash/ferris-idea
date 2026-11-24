//! The bookmark popups and the Bookmarks tool window (`SPEC-0331` behaviours 16, 20–24): the
//! popup that gives a line a mnemonic and a description, the list of all bookmarks with a source
//! preview, the field for a description, and the tree of project, files and bookmarks. What they
//! show is decided in [`crate::bookmarks_model`]; the workbench carries out what they ask for.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use ferris_idea_core::bookmarks::{Bookmark, Listed, MNEMONICS, mnemonic};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use crate::EditorView;
use crate::bookmarks_model::{GRID_COLUMNS, Node, Row, grid_step, parent, rows, tree};
use crate::code_preview::ExcerptLine;
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// Lines of the preview before and after the bookmark's line.
const PREVIEW_BEFORE: usize = 6;
const PREVIEW_AFTER: usize = 8;

/// What a bookmark popup or the tool window asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkEvent {
    /// Give the popup's line a bookmark with this mnemonic (none: without) and description.
    Set {
        /// The chosen character.
        mnemonic: Option<char>,
        /// The description; empty for none.
        description: String,
    },
    /// Jump to the bookmark on this 0-based line of this project file.
    Jump(String, usize),
    /// Remove that bookmark.
    Remove(String, usize),
    /// Give that bookmark this description; empty for none.
    Describe(String, usize, String),
    /// Ask for the description of that bookmark.
    AskDescription(String, usize),
    /// Open this project file.
    OpenFile(String),
    /// Close the popup.
    Close,
}

/// The mnemonic character a key press types, without regard to case.
fn typed_mnemonic(event: &KeyDownEvent) -> Option<char> {
    let k = &event.keystroke;
    if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.function {
        return None;
    }
    let text = k.key_char.as_deref().unwrap_or(k.key.as_str());
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then(|| mnemonic(first)).flatten()
}

/// The small square that shows a mnemonic in a row; empty without one.
fn badge(m: Option<char>, p: &Palette) -> gpui::Div {
    div()
        .w(px(16.))
        .h(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .text_size(px(11.))
        .bg(c(p.accent))
        .text_color(c(p.accent_text))
        .children(m.map(String::from))
}

/// What the popup is for.
enum Mode {
    /// A mnemonic and a description for one line (behaviour 16).
    Mnemonic {
        /// `file:line` of the line.
        place: String,
        /// The character the line's bookmark has.
        current: Option<char>,
        /// The characters other bookmarks have.
        used: BTreeSet<char>,
        /// The selected cell of the grid.
        selected: Option<usize>,
    },
    /// All bookmarks with a preview (behaviours 20, 21).
    List {
        /// The bookmarks, newest first.
        rows: Vec<Row>,
        /// The selected row.
        selected: usize,
    },
    /// The description of one bookmark (behaviour 24).
    Describe {
        /// The bookmark's file.
        file: String,
        /// Its 0-based line.
        line: usize,
    },
}

/// A popup of the bookmarks: the mnemonic chooser, the list, or the description field.
pub struct BookmarksPopup {
    mode: Mode,
    /// The grid or the list.
    focus: FocusHandle,
    /// The description.
    field: Entity<EditorView>,
    /// The project folder, for the preview's grammar.
    root: PathBuf,
    /// The texts of the listed files, by project file.
    texts: BTreeMap<String, Arc<str>>,
    /// Previews made so far, by file and line.
    previews: HashMap<(String, usize), Vec<ExcerptLine>>,
    /// The shortcut of `ToggleBookmark`, for the empty list.
    toggle_shortcut: String,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<BookmarkEvent> for BookmarksPopup {}

impl BookmarksPopup {
    fn with(mode: Mode, description: Option<&str>, cx: &mut Context<Self>) -> Self {
        let field = cx.new(EditorView::field);
        if let Some(text) = description {
            field.update(cx, |f, cx| {
                f.set_text_selected(text, cx);
            });
        }
        Self {
            mode,
            focus: cx.focus_handle(),
            field,
            root: PathBuf::new(),
            texts: BTreeMap::new(),
            previews: HashMap::new(),
            toggle_shortcut: String::new(),
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// The popup of `ToggleBookmarkWithMnemonic` for the line named `place`, which has the
    /// bookmark `current`; `used` are the mnemonics of the project's bookmarks.
    pub fn mnemonic(
        place: String,
        current: Option<&Bookmark>,
        used: BTreeSet<char>,
        cx: &mut Context<Self>,
    ) -> Self {
        let own = current.and_then(|b| b.mnemonic);
        let mode = Mode::Mnemonic {
            place,
            current: own,
            used: used.into_iter().filter(|m| Some(*m) != own).collect(),
            selected: own.and_then(|m| MNEMONICS.iter().position(|c| *c == m)),
        };
        Self::with(mode, current.and_then(|b| b.description.as_deref()), cx)
    }

    /// The list of `ShowBookmarks` over `listed`, with the texts of their files for the preview
    /// and the shortcut of `ToggleBookmark` for the empty list.
    pub fn list(
        root: PathBuf,
        listed: &[Listed],
        texts: BTreeMap<String, Arc<str>>,
        toggle_shortcut: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let mode = Mode::List {
            rows: rows(listed),
            selected: 0,
        };
        let mut popup = Self::with(mode, None, cx);
        popup.root = root;
        popup.texts = texts;
        popup.toggle_shortcut = toggle_shortcut;
        popup
    }

    /// The field for the description of the bookmark on `line` of `file`.
    pub fn describe(
        file: String,
        line: usize,
        description: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with(
            Mode::Describe { file, line },
            Some(description.unwrap_or_default()),
            cx,
        )
    }

    /// The bookmarks changed while the list is open: it shows `listed` now and keeps its
    /// selection where it was.
    pub fn set_listed(
        &mut self,
        listed: &[Listed],
        texts: BTreeMap<String, Arc<str>>,
        cx: &mut Context<Self>,
    ) {
        if let Mode::List {
            rows: shown,
            selected,
        } = &mut self.mode
        {
            *shown = rows(listed);
            *selected = (*selected).min(shown.len().saturating_sub(1));
            self.texts = texts;
            self.previews.clear();
            cx.notify();
        }
    }

    /// What takes the keys when the popup opens.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        match self.mode {
            Mode::Describe { .. } => self.field.read(cx).focus(),
            _ => self.focus.clone(),
        }
    }

    /// Whether this is the list of all bookmarks.
    pub fn is_list(&self) -> bool {
        matches!(self.mode, Mode::List { .. })
    }

    /// The rows of the list.
    pub fn rows(&self) -> &[Row] {
        match &self.mode {
            Mode::List { rows, .. } => rows,
            _ => &[],
        }
    }

    /// The selected row of the list, or the selected character of the grid.
    pub fn selected(&self) -> Option<usize> {
        match &self.mode {
            Mode::List { selected, .. } => Some(*selected),
            Mode::Mnemonic { selected, .. } => *selected,
            Mode::Describe { .. } => None,
        }
    }

    /// The description typed so far.
    pub fn description(&self, cx: &gpui::App) -> String {
        self.field.read(cx).state.buffer.text()
    }

    /// The preview of the selected row: the lines around the bookmark, its line marked.
    fn preview(&mut self) -> Vec<ExcerptLine> {
        let Mode::List { rows, selected } = &self.mode else {
            return Vec::new();
        };
        let Some(row) = rows.get(*selected) else {
            return Vec::new();
        };
        let key = (row.file.clone(), row.line);
        if let Some(lines) = self.previews.get(&key) {
            return lines.clone();
        }
        let lines = self.texts.get(&row.file).map_or_else(Vec::new, |text| {
            crate::code_preview::excerpt(
                &self.root.join(&row.file),
                text,
                row.line,
                None,
                PREVIEW_BEFORE,
                PREVIEW_AFTER,
            )
        });
        self.previews.insert(key, lines.clone());
        lines
    }

    /// Enter: what the popup is for.
    fn choose(&mut self, cx: &mut Context<Self>) {
        let description = self.description(cx);
        match &self.mode {
            Mode::Mnemonic { selected, .. } => cx.emit(BookmarkEvent::Set {
                mnemonic: selected.map(|i| MNEMONICS[i]),
                description,
            }),
            Mode::List { rows, selected } => {
                if let Some(row) = rows.get(*selected) {
                    cx.emit(BookmarkEvent::Jump(row.file.clone(), row.line));
                }
            }
            Mode::Describe { file, line } => {
                cx.emit(BookmarkEvent::Describe(file.clone(), *line, description))
            }
        }
    }

    /// The keys of the popup (behaviours 16, 21, 24).
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let plain = !(k.modifiers.control || k.modifiers.alt || k.modifiers.platform);
        let in_field = self.field.read(cx).focus().is_focused(window);
        // The first and the last bookmark of the list (`SPEC-0275` v19 behaviour 47).
        if !in_field
            && let Mode::List { rows, selected } = &mut self.mode
            && let Some(to) = crate::list_nav::edge(k, rows.len())
        {
            *selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match k.key.as_str() {
            "escape" if plain => cx.emit(BookmarkEvent::Close),
            "enter" if plain => self.choose(cx),
            // Tab moves between the grid and the description.
            "tab" if plain && matches!(self.mode, Mode::Mnemonic { .. }) => {
                if in_field {
                    window.focus(&self.focus);
                } else {
                    window.focus(&self.field.read(cx).focus());
                }
            }
            // Everything else in the field is typing.
            _ if in_field => return,
            key => match &mut self.mode {
                Mode::Mnemonic { selected, .. } => {
                    if let Some(m) = typed_mnemonic(event) {
                        let description = self.description(cx);
                        cx.emit(BookmarkEvent::Set {
                            mnemonic: Some(m),
                            description,
                        });
                    } else if plain && matches!(key, "backspace" | "delete") {
                        *selected = None;
                    } else if plain && let Some(next) = grid_step(*selected, key) {
                        *selected = Some(next);
                    } else {
                        return;
                    }
                }
                Mode::List { rows, selected } => {
                    let n = rows.len();
                    if let Some(row) = typed_mnemonic(event)
                        .and_then(|m| rows.iter().find(|r| r.mnemonic == Some(m)))
                    {
                        cx.emit(BookmarkEvent::Jump(row.file.clone(), row.line));
                    } else if !plain {
                        return;
                    } else {
                        match key {
                            "down" if n > 0 => *selected = (*selected + 1) % n,
                            "up" if n > 0 => *selected = (*selected + n - 1) % n,
                            "backspace" | "delete" => {
                                if let Some(row) = rows.get(*selected) {
                                    cx.emit(BookmarkEvent::Remove(row.file.clone(), row.line));
                                }
                            }
                            _ => return,
                        }
                        self.scroll
                            .scroll_to_item(*selected, gpui::ScrollStrategy::Center);
                    }
                }
                Mode::Describe { .. } => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The description field in its frame.
    fn field_box(&self, p: &Palette) -> gpui::Div {
        div()
            .h(px(26.))
            .px_2()
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.separator))
            .child(
                div()
                    .flex_1()
                    .h(px(18.))
                    .overflow_hidden()
                    .child(self.field.clone()),
            )
    }

    /// The grid of characters, its legend and the description field.
    fn render_mnemonic(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let Mode::Mnemonic {
            place,
            current,
            used,
            selected,
        } = &self.mode
        else {
            return div().into_any_element();
        };
        let cell = |i: usize, cx: &mut Context<Self>| {
            let m = MNEMONICS[i];
            let own = *current == Some(m);
            let taken = used.contains(&m);
            let on = *selected == Some(i);
            div()
                .id(("bookmark-mnemonic", i))
                .relative()
                .w(px(26.))
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(c(if on { p.focus } else { p.island }))
                .when(own, |d| d.bg(c(p.accent)).text_color(c(p.accent_text)))
                .when(taken, |d| d.bg(c(p.tree_selection)))
                .when(!own && !taken, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(m.to_string())
                .semantics(
                    Semantics::new(Role::Button)
                        .name(m.to_string())
                        .test_id(format!("bookmark-mnemonic-{m}"))
                        .selected(on)
                        .action(AxAction::Click)
                        .effects(&[Effect::Write]),
                )
                .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                    if let Mode::Mnemonic { selected, .. } = &mut this.mode {
                        *selected = Some(i);
                    }
                    window.focus(&this.focus);
                    // A double click sets the bookmark with the character.
                    if e.click_count() > 1 {
                        let description = this.description(cx);
                        cx.emit(BookmarkEvent::Set {
                            mnemonic: Some(m),
                            description,
                        });
                    }
                    cx.notify();
                }))
        };
        let grid_rows: Vec<gpui::Div> = (0..MNEMONICS.len())
            .step_by(GRID_COLUMNS)
            .map(|start| {
                div().flex().gap_1().children(
                    (start..(start + GRID_COLUMNS).min(MNEMONICS.len()))
                        .map(|i| cell(i, cx))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let swatch = |own: bool, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().w(px(10.)).h(px(10.)).rounded(px(2.)).bg(c(if own {
                    p.accent
                } else {
                    p.tree_selection
                })))
                .child(label)
        };
        div()
            .w(px(330.))
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .text_color(c(p.ui_muted))
                    .child(format!("Bookmark at {place}")),
            )
            .child(
                div()
                    .id("bookmark-mnemonic-grid")
                    .track_focus(&self.focus)
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(grid_rows),
            )
            .child(
                div()
                    .px_3()
                    .flex()
                    .gap_4()
                    .text_size(px(11.))
                    .text_color(c(p.ui_muted))
                    .child(swatch(true, "this line"))
                    .child(swatch(false, "in use")),
            )
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .text_color(c(p.ui_muted))
                    .child("Description (optional)"),
            )
            .child(div().m_2().child(self.field_box(p)))
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .text_size(px(11.))
                    .text_color(c(p.ui_dim))
                    .child("Type a character, or select one and press ⏎ · ⇥ description · ⌫ none"),
            )
            .into_any_element()
    }

    /// The list and the preview beside it.
    fn render_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let preview = self.preview();
        let Mode::List { rows, selected } = &self.mode else {
            return div().into_any_element();
        };
        let (shown, selected) = (rows.clone(), *selected);
        let title = div()
            .px_3()
            .py_2()
            .text_color(c(p.ui_muted))
            .child("Bookmarks");
        if shown.is_empty() {
            let hint = if self.toggle_shortcut.is_empty() {
                "No bookmarks. Toggle Bookmark sets one on the line of the caret.".to_owned()
            } else {
                format!(
                    "No bookmarks. {} sets one on the line of the caret.",
                    self.toggle_shortcut
                )
            };
            return div()
                .id("bookmarks-list")
                .track_focus(&self.focus)
                .w(px(420.))
                .child(title)
                .child(div().px_3().pb_3().text_color(c(p.ui_dim)).child(hint))
                .into_any_element();
        }
        let place = shown
            .get(selected)
            .map(|r| format!("{}:{}", r.file, r.line + 1))
            .unwrap_or_default();
        let count = shown.len();
        let list = uniform_list(
            "bookmark-rows",
            count,
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let row = &shown[i];
                        let on = i == selected;
                        let (file, line) = (row.file.clone(), row.line);
                        div()
                            .id(("bookmark-row", i))
                            .relative()
                            .w_full()
                            .h(px(24.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(match row.mnemonic {
                                Some(m) => badge(Some(m), p),
                                None => div().w(px(16.)).flex_none(),
                            })
                            .child(icon(ferris_idea_ui::icons::file_icon(&row.name)))
                            .child(div().flex_none().child(format!(
                                "{}:{}",
                                row.name,
                                row.line + 1
                            )))
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_color(c(if on { p.ui_muted } else { p.ui_dim }))
                                    .child(row.label.clone()),
                            )
                            .semantics(
                                Semantics::new(Role::ListItem)
                                    .name(format!("{}:{} {}", row.file, row.line + 1, row.label))
                                    .test_id(format!("bookmark-row-{}:{}", row.file, row.line + 1))
                                    .selected(on)
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            )
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                if let Mode::List { selected, .. } = &mut this.mode {
                                    *selected = i;
                                }
                                if e.click_count() > 1 {
                                    cx.emit(BookmarkEvent::Jump(file.clone(), line));
                                }
                                cx.notify();
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .w_full()
        .h(px((count.clamp(1, 14) as f32) * 24.0));
        div()
            .w(px(900.))
            .child(title)
            .child(
                div()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .flex()
                    .child(
                        div()
                            .id("bookmarks-list")
                            .track_focus(&self.focus)
                            .w(px(400.))
                            .flex_none()
                            .p_1()
                            .child(list),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .border_l_1()
                            .border_color(c(p.separator))
                            .bg(gpui::rgb(p.background))
                            .overflow_hidden()
                            .child(
                                div()
                                    .px_2()
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .text_size(px(11.))
                                    .text_color(c(p.ui_muted))
                                    .child(place),
                            )
                            .child(crate::code_preview::render(&preview)),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .text_size(px(11.))
                    .text_color(c(p.ui_dim))
                    .child("⏎ jump · a mnemonic jumps to its bookmark · ⌫ remove"),
            )
            .into_any_element()
    }
}

impl Render for BookmarksPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let body = match &self.mode {
            Mode::Mnemonic { .. } => self.render_mnemonic(cx),
            Mode::List { .. } => self.render_list(cx),
            Mode::Describe { file, line } => div()
                .w(px(480.))
                .child(div().px_3().pt_2().text_color(c(p.ui_muted)).child(format!(
                    "Description of the bookmark at {file}:{}",
                    line + 1
                )))
                .child(div().m_2().child(self.field_box(p)))
                .into_any_element(),
        };
        div()
            .id("bookmarks-popup")
            .occlude()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .capture_key_down(cx.listener(Self::on_key))
            .child(body)
    }
}

/// The Bookmarks tool window: the project, its files with bookmarks, the bookmarks (behaviours
/// 22–24).
pub struct BookmarksToolView {
    /// The project folder's name.
    project: String,
    listed: Vec<Listed>,
    /// The closed nodes, by [`Node::key`].
    closed: BTreeSet<String>,
    nodes: Vec<Node>,
    selected: usize,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// The shortcut of `ToggleBookmark`, for the empty window.
    toggle_shortcut: String,
}

impl EventEmitter<BookmarkEvent> for BookmarksToolView {}

impl BookmarksToolView {
    /// The tree of `listed` below the project named `project`.
    pub fn new(
        project: String,
        listed: Vec<Listed>,
        toggle_shortcut: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let closed = BTreeSet::new();
        Self {
            nodes: tree(&project, &listed, &closed),
            project,
            listed,
            closed,
            selected: 0,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            toggle_shortcut,
        }
    }

    /// Focus handle of the tree.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The visible nodes.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The selected node.
    pub fn selected(&self) -> Option<&Node> {
        self.nodes.get(self.selected)
    }

    /// Builds the visible nodes again, keeping the selection on its node, else at its index.
    fn rebuild(&mut self) {
        let key = self.selected().map(Node::key);
        self.nodes = tree(&self.project, &self.listed, &self.closed);
        self.selected = key
            .and_then(|k| self.nodes.iter().position(|n| n.key() == k))
            .unwrap_or(self.selected)
            .min(self.nodes.len().saturating_sub(1));
    }

    /// The bookmarks changed: the tree shows `listed` now.
    pub fn set_listed(&mut self, listed: Vec<Listed>, cx: &mut Context<Self>) {
        if self.listed != listed {
            self.listed = listed;
            self.rebuild();
            cx.notify();
        }
    }

    /// Selects the node at `index`.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = index.min(self.nodes.len().saturating_sub(1));
        self.scroll
            .scroll_to_item(self.selected, gpui::ScrollStrategy::Center);
        cx.notify();
    }

    /// Opens or closes the node at `index`.
    fn set_open(&mut self, index: usize, open: bool) {
        let Some(key) = self.nodes.get(index).map(Node::key) else {
            return;
        };
        if matches!(self.nodes[index], Node::Bookmark(_)) {
            return;
        }
        if open {
            self.closed.remove(&key);
        } else {
            self.closed.insert(key);
        }
        self.rebuild();
    }

    /// Enter or a double click on the selected node (behaviour 24).
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        match self.selected().cloned() {
            Some(Node::Bookmark(row)) => cx.emit(BookmarkEvent::Jump(row.file, row.line)),
            Some(Node::File { file, .. }) => cx.emit(BookmarkEvent::OpenFile(file)),
            Some(Node::Project { open, .. }) => {
                self.set_open(self.selected, !open);
                cx.notify();
            }
            None => {}
        }
    }

    /// Delete on the selected node: a bookmark is removed, other nodes stay.
    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(Node::Bookmark(row)) = self.selected() {
            cx.emit(BookmarkEvent::Remove(row.file.clone(), row.line));
        }
    }

    /// F2 or ⇧F6 on a bookmark: its description is asked for.
    pub fn rename_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(Node::Bookmark(row)) = self.selected() {
            cx.emit(BookmarkEvent::AskDescription(row.file.clone(), row.line));
        }
    }

    /// The keys of the tree (behaviour 24).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.nodes.len()) {
            self.select(to, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        }
        let last = self.nodes.len().saturating_sub(1);
        match (k.key.as_str(), k.modifiers.shift) {
            ("down", false) => self.select((self.selected + 1).min(last), cx),
            ("up", false) => self.select(self.selected.saturating_sub(1), cx),
            ("left", false) => match self.selected() {
                Some(Node::Project { open: true, .. } | Node::File { open: true, .. }) => {
                    self.set_open(self.selected, false)
                }
                _ => {
                    if let Some(up) = parent(&self.nodes, self.selected) {
                        self.select(up, cx);
                    }
                }
            },
            ("right", false) => self.set_open(self.selected, true),
            ("enter", false) => self.activate(cx),
            ("backspace" | "delete", false) => self.remove_selected(cx),
            ("f2", false) | ("f6", true) => self.rename_selected(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for BookmarksToolView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let nodes = self.nodes.clone();
        let selected = self.selected;
        let count = nodes.len();
        let empty = self.listed.iter().all(|l| l.missing);
        let list = uniform_list(
            "bookmark-tree",
            count,
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let node = &nodes[i];
                        let on = i == selected;
                        let chevron = match node {
                            Node::Project { open, .. } | Node::File { open, .. } => Some(*open),
                            Node::Bookmark(_) => None,
                        };
                        let name = match node {
                            Node::Project { name, .. } => format!("project {name}"),
                            Node::File { file, .. } => file.clone(),
                            Node::Bookmark(row) => {
                                format!("{}:{} {}", row.file, row.line + 1, row.label)
                            }
                        };
                        let test_id = match node {
                            Node::Project { .. } => "bookmark-tree-project".to_owned(),
                            other => format!("bookmark-tree-{}", other.key()),
                        };
                        let mut semantics = Semantics::new(Role::TreeItem)
                            .name(name)
                            .test_id(test_id)
                            .selected(on)
                            .action(AxAction::Click)
                            .effects(&[Effect::None]);
                        if let Some(open) = chevron {
                            semantics = semantics.expanded(open);
                        }
                        div()
                            .id(("bookmark-node", i))
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
                                    .id(("bookmark-arrow", i))
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
                                Node::Project { name, .. } => {
                                    d.child(icon("nodes/module")).child(name.clone())
                                }
                                Node::File { name, folder, .. } => d
                                    .child(icon(ferris_idea_ui::icons::file_icon(name)))
                                    .child(div().flex_none().child(name.clone()))
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_color(c(p.ui_dim))
                                            .child(folder.clone()),
                                    ),
                                Node::Bookmark(row) => {
                                    d.child(match row.mnemonic {
                                        Some(m) => badge(Some(m), p),
                                        None => badge(None, p).w(px(8.)).h(px(8.)).mx(px(4.)),
                                    })
                                    .child(div().flex_1().overflow_hidden().child(row.line_label()))
                                }
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
        let hint = if self.toggle_shortcut.is_empty() {
            "No bookmarks. Toggle Bookmark sets one on the line of the caret.".to_owned()
        } else {
            format!(
                "No bookmarks. {} sets one on the line of the caret.",
                self.toggle_shortcut
            )
        };
        div()
            .id("bookmarks-tool-window")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .overflow_hidden()
            .on_key_down(cx.listener(Self::on_key))
            .when(empty, |d| {
                d.child(div().px_3().py_2().text_color(c(p.ui_dim)).child(hint))
            })
            .when(!empty, |d| {
                d.child(div().flex_1().min_h_0().py_1().child(list))
            })
    }
}
