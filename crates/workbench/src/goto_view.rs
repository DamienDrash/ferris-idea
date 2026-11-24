//! Go to Line and Recent Files popups (`SPEC-0069`).

use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Subscription, Window, div,
    prelude::*, px, uniform_list,
};

use crate::EditorView;
use crate::goto_model::{RecentFiles, parse_line_column};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
pub enum GotoEvent {
    /// Put the caret at a 0-based line and column.
    Line(usize, usize),
    /// Open a file (absolute path).
    Open(PathBuf),
    /// Open a file at a 0-based line and column (Recent Locations, `SPEC-0069` v2 behaviour 8).
    OpenAt(PathBuf, usize, usize),
    /// Run an action: a tool window of the Recent Files popup (v2 behaviour 5).
    Action(String),
    /// Show Recent Locations instead (v2 behaviour 5).
    Locations,
    /// Rename the symbol to this name, also in comments, strings and text files as chosen
    /// (SPEC-0074, SPEC-0148).
    Rename(String, ferris_idea_languages::rename_text::TextOptions),
    /// Change the function's parameters to this list (SPEC-0113).
    Signature(String),
    /// Move the file into this workspace folder (SPEC-0114).
    Move(String),
    /// Close the popup.
    Close,
}

/// The two kinds of popup.
enum Mode {
    /// Go to Line with the caret's current 0-based position.
    Line((usize, usize)),
    /// Recent Files: all files and the one shown in the editor.
    Recent(RecentFiles, Option<PathBuf>),
    /// Rename: the current name and where else to look (SPEC-0148 behaviour 1).
    Rename(String, ferris_idea_languages::rename_text::TextOptions),
    /// Change Signature: the function's name and its current parameters.
    Signature(String, String),
    /// Move: the file's name and its folder.
    Move(String, String),
    /// Recent Locations: the places, newest first (v2 behaviour 8).
    Locations(Vec<LocationRow>),
}

/// A row of Recent Locations: the place, the declarations around it, when, and its lines.
#[derive(Debug, Clone)]
pub struct LocationRow {
    /// The place (absolute path).
    pub place: crate::nav_model::Place,
    /// The declarations around the place, outermost first (`PriceFormatter > format()`).
    pub symbols: String,
    /// How long ago, in words.
    pub when: String,
    /// Whether text was changed there.
    pub edited: bool,
    /// The lines around the place.
    pub lines: Vec<crate::code_preview::ExcerptLine>,
}

/// A tool window entry of Recent Files: title, shortcut and action id.
pub type ToolEntry = (String, String, String);

/// A small popup with one field and, for Recent Files, a list.
pub struct GotoView {
    root: PathBuf,
    /// In a remote project: the stand-in folder and the folder on the host.
    remote_folders: Option<(PathBuf, String)>,
    mode: Mode,
    field: Entity<EditorView>,
    shown: Vec<PathBuf>,
    selected: usize,
    /// Files changed in this session, for "edited only" (v2 behaviour 6).
    edited: Vec<PathBuf>,
    edited_only: bool,
    /// Tool windows listed beside the recent files (v2 behaviour 5).
    tools: Vec<ToolEntry>,
    /// The selection is in the tool window column.
    in_tools: bool,
    tool_selected: usize,
    /// Indexes of the Recent Locations rows shown.
    places_shown: Vec<usize>,
    /// In Rename mode: the new name is checked by the planner, which says why it refuses one
    /// (a CSS class or id, `SPEC-0042` v22 behaviour 38b), not by the popup.
    planner_checks_name: bool,
    _watch: Subscription,
}

impl EventEmitter<GotoEvent> for GotoView {}

impl GotoView {
    /// Go to Line, starting with the current `line:column` selected (behaviour 1).
    pub fn line(current: (usize, usize), cx: &mut Context<Self>) -> Self {
        let field = cx.new(EditorView::field);
        let text = format!("{}:{}", current.0 + 1, current.1 + 1);
        field.update(cx, |f, cx| {
            f.set_text_selected(&text, cx);
        });
        Self::with(PathBuf::new(), Mode::Line(current), field, cx)
    }

    /// Rename, starting with the current name selected (SPEC-0074 behaviour 1) and the options of
    /// the last rename (SPEC-0148 behaviour 1).
    pub fn rename(
        old: &str,
        options: ferris_idea_languages::rename_text::TextOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(EditorView::field);
        field.update(cx, |f, cx| {
            f.set_text_selected(old, cx);
        });
        Self::with(
            PathBuf::new(),
            Mode::Rename(old.to_owned(), options),
            field,
            cx,
        )
    }

    /// Change Signature of `function`, starting with its parameter list `current` (SPEC-0113
    /// behaviour 1).
    pub fn signature(function: &str, current: &str, cx: &mut Context<Self>) -> Self {
        let field = cx.new(EditorView::field);
        field.update(cx, |f, cx| {
            f.set_text(current, cx);
            let len = f.state.buffer.len_chars();
            f.select_range(len..len, cx);
        });
        let mode = Mode::Signature(function.to_owned(), current.to_owned());
        Self::with(PathBuf::new(), mode, field, cx)
    }

    /// Move `name` out of the workspace folder `folder` (SPEC-0114 behaviour 1).
    pub fn move_file(name: &str, folder: &str, cx: &mut Context<Self>) -> Self {
        let field = cx.new(EditorView::field);
        field.update(cx, |f, cx| {
            f.set_text_selected(folder, cx);
        });
        let mode = Mode::Move(name.to_owned(), folder.to_owned());
        Self::with(PathBuf::new(), mode, field, cx)
    }

    /// Recent Files over `recent` with the file shown in the editor first and the previous one
    /// selected; `edited` are the files changed in this session, `tools` the tool windows of the
    /// left column (v2 behaviours 4–6).
    pub fn recent(
        root: PathBuf,
        recent: RecentFiles,
        current: Option<PathBuf>,
        edited: Vec<PathBuf>,
        tools: Vec<ToolEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(EditorView::field);
        let mut view = Self::with(root, Mode::Recent(recent, current), field, cx);
        view.edited = edited;
        view.tools = tools;
        view.refilter(cx);
        view
    }

    /// Recent Locations over `rows`, newest first (v2 behaviour 8).
    pub fn locations(root: PathBuf, rows: Vec<LocationRow>, cx: &mut Context<Self>) -> Self {
        let field = cx.new(EditorView::field);
        let mut view = Self::with(root, Mode::Locations(rows), field, cx);
        view.refilter(cx);
        view
    }

    /// Whether this is Go to Line, Recent Files or Recent Locations, which a navigation elsewhere
    /// closes (v5 behaviour 10), and not one of the name popups.
    pub fn is_navigation(&self) -> bool {
        matches!(
            self.mode,
            Mode::Line(_) | Mode::Recent(..) | Mode::Locations(_)
        )
    }

    /// Whether only edited files or places are listed (v2 behaviours 6, 8).
    pub fn edited_only(&self) -> bool {
        self.edited_only
    }

    /// Lists only edited files or places, or all (⌘E in Recent Files, ⇧⌘E in Recent Locations).
    pub fn set_edited_only(&mut self, on: bool, cx: &mut Context<Self>) {
        self.edited_only = on;
        self.refilter_now(cx);
    }

    /// The rows of Recent Locations shown.
    pub fn places(&self) -> Vec<&LocationRow> {
        match &self.mode {
            Mode::Locations(rows) => self.places_shown.iter().map(|i| &rows[*i]).collect(),
            _ => Vec::new(),
        }
    }

    /// The selected row's index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The footer of Recent Files: the selected file's folder with `~` (v2 behaviour 4).
    pub fn footer(&self) -> String {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        self.shown
            .get(self.selected)
            .and_then(|p| p.parent())
            .map(|d| crate::chrome::shown_path(d, home.as_deref(), self.remote_folders.as_ref()))
            .unwrap_or_default()
    }

    /// In a remote project: the stand-in folder and the host folder; the footer shows host paths.
    pub fn set_remote_folders(&mut self, folders: Option<(PathBuf, String)>) {
        self.remote_folders = folders;
    }

    fn refilter_now(&mut self, cx: &mut Context<Self>) {
        self.shown.clear();
        self.places_shown.clear();
        self.refilter(cx);
        cx.notify();
    }

    fn with(root: PathBuf, mode: Mode, field: Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        let watch = cx.observe(&field, |this, _, cx| this.refilter(cx));
        Self {
            root,
            remote_folders: None,
            mode,
            field,
            shown: Vec::new(),
            selected: 0,
            edited: Vec::new(),
            edited_only: false,
            tools: Vec::new(),
            in_tools: false,
            tool_selected: 0,
            places_shown: Vec::new(),
            planner_checks_name: false,
            _watch: watch,
        }
    }

    /// The rename popup of a CSS class or id: any name but the empty one goes to the planner,
    /// which refuses one that is no CSS identifier with its reason (`SPEC-0042` v22 behaviour 38b);
    /// `-` is part of such names.
    pub fn with_name_checked_by_planner(mut self) -> Self {
        self.planner_checks_name = true;
        self
    }

    /// Focus handle of the field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.field.read(cx).focus()
    }

    /// The rename options shown, in Rename mode.
    pub fn rename_options(&self) -> Option<ferris_idea_languages::rename_text::TextOptions> {
        match &self.mode {
            Mode::Rename(_, o) => Some(*o),
            _ => None,
        }
    }

    /// The field's text.
    pub fn field_text(&self, cx: &gpui::App) -> String {
        self.field.read(cx).state.buffer.text()
    }

    /// Replaces the field's text, caret at the end.
    pub fn set_field_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| {
            f.set_text(text, cx);
            let len = f.state.buffer.len_chars();
            f.select_range(len..len, cx);
        });
    }

    /// Files shown in Recent Files.
    pub fn shown(&self) -> &[PathBuf] {
        &self.shown
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).state.buffer.text();
        match &self.mode {
            Mode::Recent(recent, current) => {
                let mut shown = recent.matching(&text, current.as_deref());
                if self.edited_only {
                    shown.retain(|p| self.edited.contains(p));
                }
                if shown != self.shown {
                    // The previous file is selected when the current one leads (v2 behaviour 4).
                    self.selected = usize::from(
                        text.trim().is_empty()
                            && shown.len() > 1
                            && current.as_ref() == shown.first(),
                    );
                    self.shown = shown;
                    cx.notify();
                }
            }
            Mode::Locations(rows) => {
                let needle = text.trim().to_lowercase();
                let shown: Vec<usize> = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| !self.edited_only || r.edited)
                    .filter(|(_, r)| {
                        needle.is_empty()
                            || r.place
                                .path
                                .to_string_lossy()
                                .to_lowercase()
                                .contains(&needle)
                            || r.symbols.to_lowercase().contains(&needle)
                            || r.lines
                                .iter()
                                .any(|l| l.text.to_lowercase().contains(&needle))
                    })
                    .map(|(i, _)| i)
                    .collect();
                if shown != self.places_shown {
                    self.places_shown = shown;
                    self.selected = 0;
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        match &self.mode {
            Mode::Line(current) => {
                let text = self.field.read(cx).state.buffer.text();
                if let Some((line, column)) = parse_line_column(&text, *current) {
                    cx.emit(GotoEvent::Line(line, column));
                }
            }
            Mode::Recent(..) if self.in_tools => match self.tools.get(self.tool_selected) {
                Some((_, _, action)) => cx.emit(GotoEvent::Action(action.clone())),
                None => cx.emit(GotoEvent::Locations),
            },
            Mode::Recent(..) => {
                if let Some(path) = self.shown.get(self.selected) {
                    cx.emit(GotoEvent::Open(path.clone()));
                }
            }
            Mode::Locations(rows) => {
                if let Some(r) = self.places_shown.get(self.selected).map(|i| &rows[*i]) {
                    cx.emit(GotoEvent::OpenAt(
                        r.place.path.clone(),
                        r.place.line,
                        r.place.column,
                    ));
                }
            }
            Mode::Rename(old, options) => {
                let options = *options;
                let name = self.field.read(cx).state.buffer.text().trim().to_owned();
                let valid = !name.is_empty()
                    && (self.planner_checks_name
                        || name.chars().all(|c| c.is_alphanumeric() || c == '_')
                            && !name.starts_with(|c: char| c.is_ascii_digit()));
                if name == *old {
                    cx.emit(GotoEvent::Close);
                } else if valid {
                    cx.emit(GotoEvent::Rename(name, options));
                }
            }
            Mode::Move(_, old) => {
                let dir = self.field.read(cx).state.buffer.text().trim().to_owned();
                if dir == *old {
                    cx.emit(GotoEvent::Close);
                } else {
                    cx.emit(GotoEvent::Move(dir));
                }
            }
            Mode::Signature(_, old) => {
                let list = self.field.read(cx).state.buffer.text().trim().to_owned();
                if list == *old {
                    cx.emit(GotoEvent::Close);
                } else {
                    cx.emit(GotoEvent::Signature(list));
                }
            }
        }
    }

    /// Enter chooses, Escape closes, Up/Down move in Recent Files (behaviours 2, 4, 5).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // ⌥C and ⌥T switch the rename options (SPEC-0148 behaviour 1).
        if let Mode::Rename(_, options) = &mut self.mode
            && k.modifiers.alt
            && !k.modifiers.control
            && !k.modifiers.platform
        {
            match k.key.as_str() {
                "c" => options.comments_and_strings = !options.comments_and_strings,
                "t" => options.text_files = !options.text_files,
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // The shortcut of `SwitcherRecentEditedChangedToggleCheckBox` (⌘E) in Recent Files and
        // ⇧⌘E in Recent Locations list only the edited ones (v2 behaviours 6, 8; v3 behaviour 9).
        let toggles = match self.mode {
            Mode::Recent(..) => {
                let m = &k.modifiers;
                ferris_idea_ui::keymap::Keystroke::from_platform(
                    &k.key, m.control, m.alt, m.shift, m.platform,
                )
                .is_some_and(|stroke| {
                    crate::keymap_actions::is_shortcut_of(
                        &crate::keymaps::current(),
                        crate::keymap_actions::EDITED_ONLY_ACTION,
                        &stroke,
                    )
                })
            }
            Mode::Locations(_) => locations_toggle(&k.key, &k.modifiers, cfg!(target_os = "macos")),
            _ => false,
        };
        if toggles {
            self.set_edited_only(!self.edited_only, cx);
            cx.stop_propagation();
            return;
        }
        let n = match self.mode {
            Mode::Recent(..) if self.in_tools => self.tools.len() + 1,
            Mode::Locations(_) => self.places_shown.len(),
            _ => self.shown.len(),
        };
        let tools = self.in_tools;
        let empty = self.field.read(cx).state.buffer.is_empty();
        // The first and the last row of the column in use (`SPEC-0275` v19 behaviour 47); Home
        // and End stay the field's while it holds text.
        if let Some(to) =
            crate::list_nav::edge_with(k, n, crate::list_nav::Keys::beside_field(empty))
        {
            if tools {
                self.tool_selected = to;
            } else {
                self.selected = to;
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift {
            return;
        }
        match k.key.as_str() {
            // ← and → move between the tool windows and the files (v2 behaviour 5).
            "left" if empty && matches!(self.mode, Mode::Recent(..)) && !self.tools.is_empty() => {
                self.in_tools = true
            }
            "right" if empty && matches!(self.mode, Mode::Recent(..)) => self.in_tools = false,
            "down" if n > 0 && tools => self.tool_selected = (self.tool_selected + 1) % n,
            "up" if n > 0 && tools => self.tool_selected = (self.tool_selected + n - 1) % n,
            "down" if n > 0 => self.selected = (self.selected + 1) % n,
            "up" if n > 0 => self.selected = (self.selected + n - 1) % n,
            "enter" => self.choose(cx),
            "escape" => cx.emit(GotoEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for GotoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let recent_mode = matches!(self.mode, Mode::Recent(..));
        let locations_mode = matches!(self.mode, Mode::Locations(_));
        let title = match &self.mode {
            Mode::Line(_) => "Go to Line:Column".to_owned(),
            Mode::Recent(..) => "Recent Files".to_owned(),
            Mode::Locations(_) => format!("Recent Locations ({})", self.places_shown.len()),
            Mode::Rename(old, _) => format!("Rename '{old}' and its usages to"),
            Mode::Move(name, _) => format!("Move '{name}' to the workspace folder"),
            Mode::Signature(f, _) => {
                format!("Parameters of '{f}' (reorder, rename, remove; new ones as `name = value`)")
            }
        };
        let rename_options = match &self.mode {
            Mode::Rename(_, o) => Some(*o),
            _ => None,
        };
        let shown = self.shown.clone();
        let selected = self.selected;
        let files_on = !self.in_tools;
        let root = self.root.clone();
        let list = uniform_list(
            "recent-files",
            shown.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let path = &shown[i];
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let dir = path
                            .parent()
                            .and_then(|d| d.strip_prefix(&root).ok())
                            .map(|d| d.display().to_string())
                            .unwrap_or_default();
                        let on = i == selected && files_on;
                        // The selection spans the whole row (OBS-0023 L22).
                        div()
                            .id(("recent", i))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(24.))
                            .px_2()
                            .rounded(px(4.))
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(icon(ferris_idea_ui::icons::file_icon(&name)))
                            .child(name)
                            .child(
                                div()
                                    .text_color(c(if on { p.ui_text } else { p.ui_dim }))
                                    .child(dir),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                this.in_tools = false;
                                this.choose(cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .w_full()
        .h(px((self.shown.len().clamp(1, 14) as f32) * 24.0));
        let check = |on: bool, label: &'static str, key: String| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_color(c(p.ui_muted))
                .child(if on { "☑" } else { "☐" })
                .child(label)
                .child(div().text_color(c(p.ui_dim)).child(key))
        };
        let tools_column = recent_mode.then(|| {
            let mut entries: Vec<gpui::AnyElement> = self
                .tools
                .iter()
                .enumerate()
                .map(|(i, (title, shortcut, _))| {
                    let on = self.in_tools && self.tool_selected == i;
                    div()
                        .id(("recent-tool", i))
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(22.))
                        .px_2()
                        .rounded(px(4.))
                        .when(on, |d| d.bg(c(p.tree_selection)))
                        .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .child(div().flex_1().child(title.clone()))
                        .child(div().text_color(c(p.ui_muted)).child(shortcut.clone()))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.in_tools = true;
                            this.tool_selected = i;
                            this.choose(cx);
                        }))
                        .into_any_element()
                })
                .collect();
            let n = self.tools.len();
            let on = self.in_tools && self.tool_selected == n;
            entries.push(
                div()
                    .id("recent-locations-entry")
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(22.))
                    .px_2()
                    .mt_1()
                    .rounded(px(4.))
                    .when(on, |d| d.bg(c(p.tree_selection)))
                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(div().flex_1().child("Recent Locations"))
                    .child(
                        div()
                            .text_color(c(p.ui_muted))
                            .child(shortcut_of("RecentLocations")),
                    )
                    .on_click(
                        cx.listener(|_, _: &gpui::ClickEvent, _, cx| cx.emit(GotoEvent::Locations)),
                    )
                    .into_any_element(),
            );
            div()
                .w(px(200.))
                .flex_none()
                .py_1()
                .px_1()
                .border_r_1()
                .border_color(c(p.separator))
                .flex()
                .flex_col()
                .children(entries)
        });
        let places: Vec<gpui::AnyElement> = match &self.mode {
            Mode::Locations(rows) => self
                .places_shown
                .iter()
                .enumerate()
                .map(|(k, i)| {
                    let r = &rows[*i];
                    let on = k == self.selected;
                    let name = r
                        .place
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    div()
                        .id(("recent-place", k))
                        .flex()
                        .flex_col()
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(c(p.separator))
                        .when(on, |d| d.bg(c(p.tree_selection)))
                        .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .h(px(22.))
                                .child(icon(ferris_idea_ui::icons::file_icon(&name)))
                                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(name))
                                .child(div().text_color(c(p.ui_muted)).child(r.symbols.clone()))
                                .child(div().flex_1())
                                .when(r.edited, |d| {
                                    d.child(div().text_color(c(p.ui_muted)).child("changed"))
                                })
                                .child(div().text_color(c(p.ui_muted)).child(r.when.clone())),
                        )
                        .child(crate::code_preview::render(&r.lines))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.selected = k;
                            this.choose(cx);
                        }))
                        .into_any_element()
                })
                .collect(),
            _ => Vec::new(),
        };
        let footer = self.footer();
        div()
            .id("goto-popup")
            .occlude()
            .w(px(match self.mode {
                Mode::Line(_) => 300.,
                Mode::Recent(..) => 700.,
                Mode::Locations(_) => 720.,
                _ => 480.,
            }))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .capture_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_3()
                    .pt_2()
                    .child(div().flex_1().text_color(c(p.ui_muted)).child(title))
                    .when(recent_mode, |d| {
                        d.child(
                            check(
                                self.edited_only,
                                "Show edited only",
                                crate::keymaps::current().shortcut_label(
                                    crate::keymap_actions::EDITED_ONLY_ACTION,
                                    cfg!(target_os = "macos"),
                                ),
                            )
                            .id("recent-edited-only")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_edited_only(!this.edited_only, cx)
                            })),
                        )
                    })
                    .when(locations_mode, |d| {
                        d.child(
                            check(self.edited_only, "Show edited only", keys("⇧⌘E"))
                                .id("locations-edited-only")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_edited_only(!this.edited_only, cx)
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .m_2()
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
                    ),
            )
            .when_some(rename_options, |d, o| {
                d.child(
                    div()
                        .px_3()
                        .pb_2()
                        .flex()
                        .gap_4()
                        .text_sm()
                        .child(check(
                            o.comments_and_strings,
                            "Search in comments and strings",
                            keys("⌥C"),
                        ))
                        .child(check(
                            o.text_files,
                            "Search for text occurrences",
                            keys("⌥T"),
                        )),
                )
            })
            .when(recent_mode, |d| {
                d.child(
                    div()
                        .border_t_1()
                        .border_color(c(p.separator))
                        .flex()
                        .children(tools_column)
                        .child(div().flex_1().min_w_0().py_1().px_1().child(list)),
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
                        .text_color(c(p.ui_muted))
                        .child(footer),
                )
            })
            .when(locations_mode, |d| {
                d.child(
                    div()
                        .id("recent-locations-list")
                        .border_t_1()
                        .border_color(c(p.separator))
                        .max_h(px(520.))
                        .overflow_y_scroll()
                        .children(places),
                )
            })
    }
}

/// The shortcut of `action` in the keymap in use.
fn shortcut_of(action: &str) -> String {
    crate::keymaps::current()
        .shortcuts(action)
        .iter()
        .find(|s| !matches!(s, ferris_idea_ui::keymap::Shortcut::Mouse { .. }))
        .map(|s| s.display(cfg!(target_os = "macos")))
        .unwrap_or_default()
}

/// Whether a key toggles "Show edited only" in Recent Locations: ⇧⌘E, Ctrl+Shift+E outside
/// macOS (`SPEC-0028` v8 behaviour 13).
fn locations_toggle(key: &str, m: &gpui::Modifiers, macos: bool) -> bool {
    let (primary, other) = if macos {
        (m.platform, m.control)
    } else {
        (m.control, m.platform)
    };
    key == "e" && primary && m.shift && !m.alt && !other
}

/// A shortcut of this dialog written for the platform: ⌘ is Ctrl and ⌥ is Alt outside macOS
/// (`SPEC-0028` v8 behaviour 13).
fn keys(text: &str) -> String {
    ferris_idea_ui::keymap::local_keys(text, cfg!(target_os = "macos"))
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    /// SPEC-0028 v8 AK-15: Recent Locations takes Ctrl+Shift+E outside macOS.
    #[test]
    fn the_locations_toggle_follows_the_platform() {
        let m = |control: bool, platform: bool| gpui::Modifiers {
            control,
            platform,
            shift: true,
            ..Default::default()
        };
        assert!(locations_toggle("e", &m(false, true), true));
        assert!(!locations_toggle("e", &m(true, false), true));
        assert!(locations_toggle("e", &m(true, false), false));
        assert!(!locations_toggle("e", &m(false, true), false));
        assert_eq!(
            ferris_idea_ui::keymap::local_keys("⇧⌘E", false),
            "Ctrl+Shift+E"
        );
        assert_eq!(ferris_idea_ui::keymap::local_keys("⌥C", false), "Alt+C");
    }
}
