//! Context menus of the workbench and the file operations they lead to (`SPEC-0275`): the menus
//! of the project tree, the editor text and the database tree, "New" (⌘N in the project tree,
//! File › New…), the name field of New and Rename, and the confirmation of Delete.
//!
//! The menu items and their enabled state come from [`crate::context_menu`]; files are created,
//! renamed, deleted, copied and moved (Cut, Copy, Paste: v2 behaviours 17–22, model in
//! [`crate::file_clipboard`]) by [`ferris_idea_core::file_ops`], the same code as the MCP tools.

use std::path::{Path, PathBuf};

use ferris_idea_core::file_ops::{self, DeletePlan, NewKind, Transfer};
use gpui::{
    ClickEvent, Context, Entity, KeyDownEvent, MouseDownEvent, Pixels, Point, Window, div,
    prelude::*, px,
};

use super::{WorkbenchView, c};
use crate::EditorView;
use crate::context_menu::{self as model, MenuEntry};
use crate::file_clipboard::FileClipboard;
use crate::icon_images::icon;
use crate::palette::Palette;

/// Where a context menu was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPlace {
    /// A row of the project tree.
    ProjectTree,
    /// The text of the editor.
    Editor,
    /// The database tree.
    Database,
    /// The "New" popup (⌘N, File › New…).
    New,
    /// The line-number gutter of the editor (`SPEC-0277` behaviour 1).
    Gutter,
    /// The annotation column of the editor (`SPEC-0277` behaviour 6).
    Annotations,
    /// A terminal tab (`SPEC-0313` behaviours 2, 4, 5).
    Terminal,
    /// The Run console and its test tree (`SPEC-0313` behaviours 3, 4).
    Run,
    /// A task of the build tools window (`SPEC-0100` v2 behaviour 10).
    BuildTools,
    /// The Debug window's ⋮, a frame or a row of its Variables list (`SPEC-0091` v11).
    Debug(crate::debug_view::DebugMenu),
    /// The calls Smart Step Into offers (`SPEC-0091` v11 behaviour 18).
    SmartStep,
    /// Tab `index` of editor group `group` (`SPEC-0059` v9 behaviour 18).
    Tab {
        /// The group (`crate::editor_layout::MAIN` for the main group).
        group: usize,
        /// The tab's index in its group.
        index: usize,
    },
}

/// An open context menu.
pub(super) struct OpenMenu {
    place: MenuPlace,
    /// The file or folder the menu acts on (project tree, New, editor).
    target: Option<PathBuf>,
    /// The 0-based editor line it was opened on (annotation column).
    line: Option<usize>,
    /// Window position of its top left corner.
    at: Point<Pixels>,
    entries: Vec<MenuEntry>,
    /// The open submenu (index into `entries`).
    sub: Option<usize>,
    /// The highlighted entry (keyboard), and in the open submenu.
    cursor: Option<usize>,
    sub_cursor: Option<usize>,
}

/// What the name field is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PromptKind {
    /// A new file or folder of this kind in this folder.
    New(NewKind, PathBuf),
    /// A new name for this file or folder.
    Rename(PathBuf),
    /// The name of the copy or the moved file in the folder (v2 behaviour 19).
    Paste {
        from: PathBuf,
        dir: PathBuf,
        mode: Transfer,
    },
    /// A Maven goal or Gradle task line to run in this module (`SPEC-0100` v2 behaviour 5).
    BuildCommand(Box<ferris_idea_core::Module>),
    /// A build file to link (`SPEC-0100` v3 behaviour 16).
    LinkBuildFile,
    /// The name of an extracted interface or superclass (`SPEC-0320`).
    ClassName {
        interface: bool,
        place: crate::nav_model::Place,
    },
    /// The name of a new tool window layout holding this arrangement (`SPEC-0333` behaviour 4).
    NewLayout(Box<ferris_idea_ui::window_layouts::Arrangement>),
    /// A new name for this tool window layout (`SPEC-0333` behaviour 10).
    RenameLayout(String),
}

/// The suggestions of the Execute field (`SPEC-0100` v3 behaviour 20).
#[derive(Default)]
pub(super) struct Suggest {
    /// The chosen suggestion (↓/↑), if any.
    cursor: Option<usize>,
    /// The project's Maven profiles with their defaults.
    profiles: Vec<(String, bool)>,
    /// The line the suggestions were made for and the suggestions.
    cache: std::cell::RefCell<(
        Option<String>,
        Vec<ferris_idea_core::buildtasks::Suggestion>,
    )>,
}

/// The name field of New and Rename (behaviours 3 and 5).
pub(super) struct NamePrompt {
    kind: PromptKind,
    field: Entity<EditorView>,
    error: Option<String>,
    suggest: Suggest,
}

/// Width of a context menu.
const MENU_WIDTH: f32 = 240.;

fn enabled_indexes(entries: &[MenuEntry]) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| match e {
            MenuEntry::Item(i) => i.enabled,
            MenuEntry::Sub(..) => true,
            MenuEntry::Separator => false,
        })
        .map(|(i, _)| i)
        .collect()
}

/// The next (or previous) enabled index after `from`, wrapping.
fn step(entries: &[MenuEntry], from: Option<usize>, down: bool) -> Option<usize> {
    let on = enabled_indexes(entries);
    if on.is_empty() {
        return None;
    }
    let at = from.and_then(|f| on.iter().position(|&i| i == f));
    Some(match (at, down) {
        (None, true) => on[0],
        (None, false) => on[on.len() - 1],
        (Some(k), true) => on[(k + 1) % on.len()],
        (Some(k), false) => on[(k + on.len() - 1) % on.len()],
    })
}

/// The entry highlighted when a menu opens: the New popup has its first entry chosen, so Return
/// alone creates it (`SPEC-0275` v17 behaviour 44); other menus have none until ↑ or ↓.
fn first_cursor(place: MenuPlace, entries: &[MenuEntry]) -> Option<usize> {
    match place {
        MenuPlace::New => step(entries, None, true),
        _ => None,
    }
}

impl WorkbenchView {
    /// Whether a debug session is paused (the editor menu's debugger items, D2).
    pub fn debug_paused(&self, cx: &gpui::App) -> bool {
        self.debug_view.as_ref().is_some_and(|v| {
            let v = v.read(cx);
            v.active() && matches!(v.state(), crate::debug_view::DebugState::Paused(_))
        })
    }

    /// Opens the context menu of `place` at window position `at`.
    pub fn open_context_menu(
        &mut self,
        place: MenuPlace,
        target: Option<PathBuf>,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu_at_line(place, target, None, at, cx);
    }

    /// Opens the context menu of `place` at window position `at`, for 0-based editor line `line`
    /// (the annotation column, `SPEC-0277` behaviour 6).
    pub fn open_context_menu_at_line(
        &mut self,
        place: MenuPlace,
        target: Option<PathBuf>,
        line: Option<usize>,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let repository = self.branch.is_some();
        let entries = match place {
            MenuPlace::ProjectTree => {
                let path = target.clone().unwrap_or_else(|| self.root.clone());
                let targets = self.tree_targets(&path);
                let is_root = targets.iter().any(|p| self.is_project_folder(p));
                let file_type = (!path.is_dir()).then(|| self.file_type_target_of(&path));
                model::with_file_type_entries_in_tree_menu(
                    model::project_tree_menu(model::TreeTarget {
                        several: targets.len() > 1,
                        dir: path.is_dir(),
                        root: is_root,
                        repository,
                        paste: self.pastable_files(cx).is_some(),
                    }),
                    file_type,
                )
            }
            MenuPlace::Editor => {
                let editor = self.active_editor();
                let selection = editor.as_ref().is_some_and(|e| e.read(cx).has_selection());
                let caret_line = self.caret_position(cx).map(|(l, _)| l.saturating_sub(1));
                let runnable = self
                    .active_path()
                    .filter(|p| runnable(p))
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .map(|name| {
                        let text = editor
                            .as_ref()
                            .map(|e| e.read(cx).state.buffer.text())
                            .unwrap_or_default();
                        model::run_names(&name, &text, caret_line)
                    });
                let annotated = editor.as_ref().is_some_and(|e| e.read(cx).annotated());
                let debugging = self.debug_paused(cx);
                let sql = self
                    .active_path()
                    .and_then(|p| p.extension().map(|e| e.eq_ignore_ascii_case("sql")))
                    .unwrap_or(false);
                let request_file = self
                    .active_path()
                    .is_some_and(|p| crate::http_model::is_request_file(&p));
                let room = self.room_can_comment();
                // SPEC-0096 v6 behaviour 8: Run All Requests in File in a request file's menu.
                // SPEC-0323 v8 behaviour 54: Add Comment in a room.
                crate::room_comments_model::with_comment_entry(
                    model::with_http_entries(
                        model::with_sql_entries(
                            model::editor_menu(&model::EditorTarget {
                                selection,
                                runnable,
                                repository,
                                annotated,
                                debugging,
                            }),
                            sql,
                        ),
                        request_file,
                    ),
                    room,
                )
            }
            MenuPlace::Gutter => {
                model::gutter_menu(self.active_editor().is_some_and(|e| e.read(cx).annotated()))
            }
            MenuPlace::Annotations => {
                let committed = self.active_editor().is_some_and(|e| {
                    let e = e.read(cx);
                    line.and_then(|l| e.annotations()?.commit_at(l)).is_some()
                });
                model::annotation_menu(committed)
            }
            MenuPlace::Database => {
                let t = self
                    .database
                    .as_ref()
                    .map_or(model::DbTarget::Nothing, |d| d.read(cx).target());
                model::database_menu(t)
            }
            MenuPlace::New => model::new_entries(),
            MenuPlace::Terminal => {
                let ai = crate::appearance::ide_settings(cx).ai;
                let view = self.terminal();
                let v = view.as_ref().map(|v| v.read(cx));
                model::terminal_menu(model::TerminalTarget {
                    selection: v.is_some_and(|v| v.selection_text().is_some()),
                    command: v.is_some_and(|v| v.last_command().is_some()),
                    shared: v.is_some_and(|v| v.shared()),
                    ai: ai.enabled,
                    lines: ferris_idea_ai::context::terminal_lines(ai.terminal_lines),
                })
            }
            MenuPlace::BuildTools => self.build_tools_menu(cx),
            MenuPlace::Debug(which) => {
                let target = self.debug_target(cx);
                match which {
                    crate::debug_view::DebugMenu::More => model::debug_more_menu(target),
                    crate::debug_view::DebugMenu::Frame => model::debug_frame_menu(target),
                    crate::debug_view::DebugMenu::Variable => model::debug_variable_menu(target),
                }
            }
            MenuPlace::SmartStep => {
                let targets = self
                    .debug_view
                    .as_ref()
                    .map(|v| v.read(cx).step_targets().to_vec())
                    .unwrap_or_default();
                model::step_target_menu(&targets)
            }
            MenuPlace::Tab { group, index } => self.tab_menu_entries(group, index, cx),
            MenuPlace::Run => {
                let ai = crate::appearance::ide_settings(cx).ai;
                let run = self.run_view.as_ref().map(|v| v.read(cx));
                model::run_menu(model::RunTarget {
                    output: run.is_some_and(|r| r.selected_output().is_some()),
                    failed_test: run.is_some_and(|r| r.selected_failed_test().is_some()),
                    ai: ai.enabled,
                })
            }
        };
        self.stripe_menu = None;
        self.header_menu = None;
        self.view_menu = false;
        self.tab_list = None;
        // No hover card or change bar stays over the menu (SPEC-0061 v9 behaviour 48).
        self.change_bar = None;
        self.hide_hover_cards(cx);
        let cursor = first_cursor(place, &entries);
        self.context_menu = Some(OpenMenu {
            place,
            target,
            line,
            at,
            entries,
            sub: None,
            cursor,
            sub_cursor: None,
        });
        cx.notify();
    }

    /// Scrolls the project tree so that its selection is visible.
    pub(super) fn scroll_tree_to_selection(&self) {
        self.tree_scroll
            .scroll_to_item(self.tree.selected_row(), gpui::ScrollStrategy::Center);
    }

    /// Reads the project tree again, expands down to `path`, selects and shows it.
    pub fn reveal_in_tree(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        self.tree = self.tree.reloaded();
        let shown = self.tree.reveal(path);
        self.scroll_tree_to_selection();
        cx.notify();
        shown
    }

    /// Gives the project tree the keyboard focus.
    pub fn focus_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_tree = true;
        window.focus(&self.tree_focus);
        cx.notify();
    }

    /// Whether the project tree shows the folder at `path` expanded.
    pub fn tree_is_expanded(&self, path: &Path) -> bool {
        self.tree.is_expanded(path)
    }

    /// The path selected in the project tree.
    pub fn tree_selected_path(&self) -> PathBuf {
        self.tree.selected_path().to_path_buf()
    }

    /// The file or folder the open context menu acts on (tests).
    pub fn context_menu_target(&self) -> Option<PathBuf> {
        self.context_menu.as_ref()?.target.clone()
    }

    /// The open context menu: where, and its entries as `label` and `Sub › label` with their
    /// enabled state (tests and the self-test).
    pub fn context_menu_items(&self) -> Option<(MenuPlace, Vec<(String, bool)>)> {
        let menu = self.context_menu.as_ref()?;
        let mut out = Vec::new();
        for e in &menu.entries {
            match e {
                MenuEntry::Item(i) => out.push((i.label.clone(), i.enabled)),
                MenuEntry::Sub(label, sub) => {
                    for s in model::flatten(sub) {
                        out.push((format!("{label} › {}", s.label), s.enabled));
                    }
                }
                MenuEntry::Separator => {}
            }
        }
        Some((menu.place, out))
    }

    /// Chooses the item labelled `path` (`Sub › Item`) of the open menu, as a click does; false
    /// when there is no such enabled item.
    pub fn choose_context_item(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(item) = self
            .context_menu
            .as_ref()
            .and_then(|m| model::find(&m.entries, path))
            .filter(|i| i.enabled)
            .map(|i| i.action.clone())
        else {
            return false;
        };
        self.run_context_item(&item, window, cx);
        true
    }

    /// Closes the menu and carries out `action` for its place and target.
    fn run_context_item(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.context_menu.take() else {
            return;
        };
        cx.notify();
        let target = menu.target.clone();
        if let Some(kind) = model::new_kind(action) {
            let path = target.unwrap_or_else(|| self.root.clone());
            let dir = if path.is_dir() {
                path
            } else {
                path.parent().map_or(self.root.clone(), Path::to_path_buf)
            };
            self.start_new(kind, dir, window, cx);
            return;
        }
        match menu.place {
            MenuPlace::ProjectTree | MenuPlace::New => {
                let path = target.unwrap_or_else(|| self.root.clone());
                self.tree_item(action, path, window, cx);
            }
            MenuPlace::Database => self.database_item(action, window, cx),
            // Open In acts on the editor's file (SPEC-0275 v6 behaviour 14b).
            MenuPlace::Editor if matches!(action, model::REVEAL | model::OPEN_IN_TERMINAL) => {
                if let Some(path) = self.active_path() {
                    self.tree_item(action, path, window, cx);
                }
            }
            MenuPlace::Editor => {
                // The editor's items act on its caret and selection (Add to Watches, D2).
                if let Some(e) = self.active_editor() {
                    window.focus(&e.read(cx).focus());
                }
                self.menu_action(action, window, cx)
            }
            MenuPlace::Gutter | MenuPlace::Annotations => {
                self.annotation_item(action, menu.line, window, cx)
            }
            MenuPlace::Terminal | MenuPlace::Run => {
                self.ai_item(action, window, cx);
            }
            MenuPlace::BuildTools => self.build_tools_item(action, window, cx),
            MenuPlace::Debug(_) | MenuPlace::SmartStep => self.debug_item(action, window, cx),
            MenuPlace::Tab { group, index } => self.tab_menu_item(group, index, action, window, cx),
        }
    }

    /// The group and tab whose menu is open, if a tab's menu is open.
    pub(super) fn open_tab_menu_target(&self) -> Option<(usize, usize)> {
        match self.context_menu.as_ref()?.place {
            MenuPlace::Tab { group, index } => Some((group, index)),
            _ => None,
        }
    }

    /// Highlights the open menu's first enabled entry, so ↑ ↓ and Enter choose at once (Smart
    /// Step Into, `SPEC-0091` v11 behaviour 18).
    pub fn context_menu_cursor_first(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.context_menu {
            menu.cursor = step(&menu.entries, None, true);
            cx.notify();
        }
    }

    /// What the Debug window's menus depend on (`SPEC-0091` v11).
    fn debug_target(&self, cx: &gpui::App) -> model::DebugTarget {
        let paused = self.debug_paused(cx);
        let Some(v) = self.debug_view.as_ref().map(|v| v.read(cx)) else {
            return model::DebugTarget {
                paused,
                ..model::DebugTarget::default()
            };
        };
        let (object, marked) = v.selected_mark();
        // SPEC-0091 v12: what the further items depend on.
        model::DebugTarget {
            paused,
            object,
            marked,
            running: matches!(v.state(), crate::debug_view::DebugState::Running),
            code_changed: !v.code_changed().is_empty(),
            number: v.selected_is_number(),
            field: v.selected_is_field(),
            row: v.selected_variable().is_some(),
            view: v.view_of_selected(),
            lang: Some(v.lang()),
            filters: v.filters(),
            renderers_muted: v.renderers_muted(),
        }
    }

    /// Carries out an item of the Debug window's menus or a Smart Step Into choice (`SPEC-0091`
    /// v11 behaviours 18, 19, 21, 23, 24); keymap actions run as such.
    fn debug_item(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.debug_view.clone() else {
            return;
        };
        if let Some(id) = action
            .strip_prefix(model::DEBUG_STEP_TARGET)
            .and_then(|id| id.parse::<i64>().ok())
        {
            view.update(cx, |v, cx| v.step_into_target(id, cx));
            return;
        }
        let selected = view.read(cx).selected_variable();
        let copy = |text: String, cx: &mut Context<Self>| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        };
        match (action, selected) {
            (model::DEBUG_COPY_VALUE, Some((_, value, _))) => copy(value, cx),
            (model::DEBUG_COPY_NAME, Some((name, _, _))) => copy(name, cx),
            (model::DEBUG_COPY_STACK, _) => copy(view.read(cx).stack_text(), cx),
            (model::DEBUG_EVALUATE_ROW, Some((_, _, expression))) => {
                view.update(cx, |v, cx| v.set_expression(&expression, cx));
                window.focus(&view.read(cx).expression_focus(cx));
            }
            (model::DEBUG_WATCH_ROW, Some((_, _, expression))) => {
                view.update(cx, |v, cx| v.add_watch(&expression, cx));
            }
            (model::DEBUG_UNMARK, _) => {
                view.update(cx, |v, cx| v.unmark_selected(cx));
            }
            ("XDebugger.SetValue", _) => {
                if view.update(cx, |v, cx| v.start_set_value(cx)) {
                    window.focus(&view.read(cx).expression_focus(cx));
                }
            }
            ("Debugger.MarkObject", _) => {
                if view.update(cx, |v, cx| v.start_mark(cx)) {
                    window.focus(&view.read(cx).expression_focus(cx));
                }
            }
            // SPEC-0091 v12 behaviours 30–34, SPEC-0146 v3.
            (model::DEBUG_INSPECT, _) => {
                view.update(cx, |v, cx| v.inspect_selected(false, cx));
            }
            (model::DEBUG_REFERRING, _) => {
                view.update(cx, |v, cx| v.inspect_selected(true, cx));
            }
            (model::DEBUG_RENDERER, _) => {
                if view.update(cx, |v, cx| v.start_renderer(cx)) {
                    window.focus(&view.read(cx).expression_focus(cx));
                }
            }
            (model::DEBUG_MUTE_RENDERERS, _) => {
                view.update(cx, |v, cx| v.toggle_mute_renderers(cx));
            }
            (model::DEBUG_FIELD_WATCH, _) => {
                view.update(cx, |v, cx| v.add_field_watch_selected(cx));
            }
            (a, _) if a.starts_with(model::DEBUG_VIEW) => {
                if let Some(view_as) =
                    ferris_idea_debugger::ViewAs::from_id(&a[model::DEBUG_VIEW.len()..])
                {
                    view.update(cx, |v, cx| v.set_view_selected(view_as, cx));
                }
            }
            (a, _) if a.starts_with(model::DEBUG_FILTER) => {
                let which = a[model::DEBUG_FILTER.len()..].to_owned();
                view.update(cx, |v, cx| v.toggle_filter(&which, cx));
            }
            (other, _) => self.menu_action(other, window, cx),
        }
    }

    /// The project tree's own items; the rest run as menu actions.
    pub(super) fn tree_item(
        &mut self,
        action: &str,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.root.canonicalize().unwrap_or(self.root.clone());
        let copy = |text: String, cx: &mut Context<Self>| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        };
        // Copy Path/Reference, Cut, Copy and Delete act on every selected item (v3 behaviour 25).
        let targets = self.tree_targets(&path);
        let lines = |f: &dyn Fn(&Path) -> String| {
            targets.iter().map(|p| f(p)).collect::<Vec<_>>().join("\n")
        };
        match action {
            // SPEC-0004 v5 behaviour 13: on every selected file.
            model::OVERRIDE_FILE_TYPE | model::REVERT_FILE_TYPE | model::ASSOCIATE_FILE_TYPE => {
                self.file_type_action_on(action, targets, window, cx);
            }
            model::COPY_PATH => {
                let folders = self.remote_folders();
                copy(
                    lines(&|p| crate::chrome::shown_path(p, None, folders.as_ref())),
                    cx,
                );
            }
            model::COPY_RELATIVE_PATH => copy(
                lines(&|p| {
                    if let Some(rel) = self.remote_rel(p) {
                        return rel;
                    }
                    let canonical = p.canonicalize().unwrap_or(p.to_path_buf());
                    let rel = canonical.strip_prefix(&root).unwrap_or(&canonical);
                    rel.to_string_lossy().replace('\\', "/")
                }),
                cx,
            ),
            model::COPY_FILE_NAME => copy(
                lines(&|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                }),
                cx,
            ),
            // One line per item that lies in a source folder (SPEC-0332 v2 behaviour 13d).
            model::COPY_SOURCE_ROOT_PATH => self.copy_path_of(action, targets.clone(), cx),
            model::RENAME_FILE if targets.len() > 1 => {
                self.status = Some(ONE_TO_RENAME.into());
            }
            model::RENAME_FILE => self.start_rename(path, window, cx),
            model::DELETE_FILE => self.ask_delete(&targets, window, cx),
            model::CUT_FILE => self.cut_or_copy_files(&targets, Transfer::Move, cx),
            model::COPY_FILE => self.cut_or_copy_files(&targets, Transfer::Copy, cx),
            model::PASTE_FILE => self.start_paste(path, window, cx),
            model::REVEAL => self.reveal_in_file_manager(&path),
            model::OPEN_IN_TERMINAL => {
                let dir = if path.is_dir() {
                    path
                } else {
                    path.parent().map_or(self.root.clone(), Path::to_path_buf)
                };
                self.open_terminal_in(dir, window, cx);
            }
            model::SHOW_HISTORY => {
                self.open(&path, window, cx);
                self.show_history(window, cx);
            }
            // The file items of the Git submenu act on the opened file (SPEC-0275 v8 behaviour
            // 14c).
            model::VCS_COMMIT_FILE
            | model::VCS_ROLLBACK_FILE
            | model::VCS_FILE_HISTORY
            | model::VCS_SHOW_DIFF
            | model::VCS_COMPARE_BRANCH => {
                self.open(&path, window, cx);
                self.menu_action(action, window, cx);
            }
            // Opens the file, then annotates it (SPEC-0277 behaviour 1).
            model::ANNOTATE => {
                self.open(&path, window, cx);
                self.toggle_annotations(cx);
            }
            other => self.menu_action(other, window, cx),
        }
    }

    /// The database tree's items.
    fn database_item(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let db = self.database.clone();
        match (action, db) {
            (model::DB_NEW_SOURCE, _) => self.open_connection_dialog(None, window, cx),
            (model::DB_CONSOLE | model::DB_JUMP_CONSOLE, _) => self.open_console(window, cx),
            (model::DB_DIAGRAM, _) => self.show_er_diagram(window, cx),
            (model::DB_COMPARE_OBJECTS, _) => self.compare_objects(window, cx),
            (model::DB_COMPARE, Some(d)) => {
                let source = d.read(cx).selected_profile();
                self.compare_schemas_of(source, window, cx);
            }
            (model::DB_REFRESH, Some(d)) => d.update(cx, |d, cx| d.refresh_selected(cx)),
            (model::DB_PROPERTIES, Some(d)) => d.update(cx, |d, cx| d.properties(cx)),
            (model::DB_RENAME, Some(d)) => d.update(cx, |d, cx| d.rename_selected(cx)),
            (model::DB_CHANGE_TYPE, Some(d)) => d.update(cx, |d, cx| d.change_type_selected(cx)),
            (model::DB_DELETE, Some(d)) => d.update(cx, |d, cx| d.remove_selected(cx)),
            (model::DB_COPY, Some(d)) => {
                if let Some(name) = d.read(cx).selected_label() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(name));
                }
            }
            (other, _) => self.menu_action(other, window, cx),
        }
    }

    /// Shows `path` in Finder (selected), or opens its folder in the file manager elsewhere.
    pub(super) fn reveal_in_file_manager(&mut self, path: &Path) {
        // A remote project's files are on the host; the stand-in folder is the window's own.
        if let Some(rel) = self.remote_rel(path) {
            let shown = if rel == "." {
                "The project"
            } else {
                rel.as_str()
            };
            self.status = Some(format!("{shown} is on the host, not on this computer"));
            return;
        }
        let spawned = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn()
        } else {
            let dir = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(path)
            };
            std::process::Command::new("xdg-open").arg(dir).spawn()
        };
        if let Err(e) = spawned {
            self.status = Some(format!("Cannot open the file manager: {e}"));
        }
    }

    /// A new terminal tab whose shell starts in `dir` (behaviour 1).
    fn open_terminal_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_terminals(window, cx);
        // A folder of a remote project opens a host terminal there (SPEC-0318 v2 detail 2).
        let remote = self
            .remote_rel(&dir)
            .and_then(|rel| self.remote_terminal_view(&rel, cx));
        let view = remote.unwrap_or_else(|| cx.new(|cx| crate::TerminalView::new(dir, cx)));
        self.add_terminal_tab(Some(view), None, window, cx);
        self.show_terminal = true;
        self.focus_terminal(window, cx);
        self.save_terminals(cx);
        cx.notify();
    }

    /// "New" for the project tree's selection (⌘N there, File › New…): the popup of kinds at the
    /// selected row, or near the top of the window.
    pub fn open_new_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // An extra node or a library has no folder: new files go to the project folder
        // (SPEC-0061 v4 behaviour 16).
        let path = if self.tree.is_virtual(self.tree.selected) {
            self.root.clone()
        } else {
            self.tree.selected_path().to_path_buf()
        };
        let at = crate::hit_targets::get(&format!("tree-row-{}", self.tree.selected))
            .map(|r| gpui::point(px(r.x + 24.), px(r.y + r.h)))
            .unwrap_or_else(|| {
                let w = window.viewport_size().width;
                gpui::point(w / 2. - px(MENU_WIDTH / 2.), px(120.))
            });
        self.open_context_menu(MenuPlace::New, Some(path), at, cx);
    }

    /// Opens the New menu for `folder`, under the top of the window: nothing in the tree is
    /// selected for it (`NewElementSamePlace`, `SPEC-0275` v13 behaviour 35).
    pub fn open_new_popup_for(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let w = window.viewport_size().width;
        let at = gpui::point(w / 2. - px(MENU_WIDTH / 2.), px(120.));
        self.open_context_menu(MenuPlace::New, Some(folder), at, cx);
    }

    /// Opens the name field for a new `kind` in `dir`.
    pub fn start_new(
        &mut self,
        kind: NewKind,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = cx.new(EditorView::field);
        window.focus(&field.read(cx).focus());
        self.name_prompt = Some(NamePrompt {
            kind: PromptKind::New(kind, dir),
            field,
            error: None,
            suggest: Suggest::default(),
        });
        cx.notify();
    }

    /// Opens the name field of an extracted type (`SPEC-0320`).
    pub(super) fn start_class_name(
        &mut self,
        interface: bool,
        place: crate::nav_model::Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = cx.new(EditorView::field);
        window.focus(&field.read(cx).focus());
        self.name_prompt = Some(NamePrompt {
            kind: PromptKind::ClassName { interface, place },
            field,
            error: None,
            suggest: Suggest::default(),
        });
        cx.notify();
    }

    /// Opens the name field to rename `path`, with the name before the extension selected.
    pub fn start_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = match name.rfind('.') {
            Some(dot) if dot > 0 && !path.is_dir() => name[..dot].chars().count(),
            _ => name.chars().count(),
        };
        let field = cx.new(EditorView::field);
        field.update(cx, |f, cx| {
            f.set_text(&name, cx);
            f.select_range(0..stem, cx);
        });
        window.focus(&field.read(cx).focus());
        self.name_prompt = Some(NamePrompt {
            kind: PromptKind::Rename(path),
            field,
            error: None,
            suggest: Suggest::default(),
        });
        cx.notify();
    }

    /// Opens the field for a build command line to run in `module` (Execute in the build tools
    /// window, `SPEC-0100` v2 behaviour 5), prefilled with the tool's name.
    pub fn start_build_command(
        &mut self,
        module: ferris_idea_core::Module,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = cx.new(EditorView::field);
        let tool = match module.kind {
            ferris_idea_core::ModuleKind::Maven => "mvn ",
            ferris_idea_core::ModuleKind::Gradle => "gradle ",
            _ => "",
        };
        field.update(cx, |f, cx| {
            f.set_text(tool, cx);
            let end = tool.chars().count();
            f.select_range(end..end, cx);
        });
        window.focus(&field.read(cx).focus());
        let files = ferris_idea_core::Workspace::open(&self.root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        let profiles = ferris_idea_core::buildtools::project_profiles(
            &ferris_idea_core::read_project(&self.root, &files),
        );
        self.name_prompt = Some(NamePrompt {
            kind: PromptKind::BuildCommand(Box::new(module)),
            field,
            error: None,
            suggest: Suggest {
                profiles,
                ..Default::default()
            },
        });
        cx.notify();
    }

    /// Opens the field for a build file to link (`SPEC-0100` v3 behaviour 16), prefilled with
    /// `start`.
    pub fn start_link_prompt(&mut self, start: &str, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(EditorView::field);
        field.update(cx, |f, cx| {
            f.set_text(start, cx);
            let end = start.chars().count();
            f.select_range(end..end, cx);
        });
        window.focus(&field.read(cx).focus());
        self.name_prompt = Some(NamePrompt {
            kind: PromptKind::LinkBuildFile,
            field,
            error: None,
            suggest: Suggest::default(),
        });
        cx.notify();
    }

    /// Opens the name field of Save Current Layout as New… for `arrangement` (`SPEC-0333`
    /// behaviour 4).
    pub(super) fn start_new_layout_prompt(
        &mut self,
        arrangement: ferris_idea_ui::window_layouts::Arrangement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_layout_prompt(PromptKind::NewLayout(Box::new(arrangement)), "", window, cx);
    }

    /// Opens the name field of Rename… for the layout `name`, starting with that name selected
    /// (`SPEC-0333` behaviour 10).
    pub(super) fn start_rename_layout_prompt(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = name.clone();
        self.start_layout_prompt(PromptKind::RenameLayout(name), &start, window, cx);
    }

    fn start_layout_prompt(
        &mut self,
        kind: PromptKind,
        start: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = cx.new(EditorView::field);
        field.update(cx, |f, cx| {
            f.set_text_selected(start, cx);
        });
        window.focus(&field.read(cx).focus());
        self.name_prompt = Some(NamePrompt {
            kind,
            field,
            error: None,
            suggest: Suggest::default(),
        });
        cx.notify();
    }

    /// The suggestions of the open Execute field for its current line (`SPEC-0100` v3 behaviour
    /// 20); empty for other fields.
    pub fn build_command_suggestions(
        &self,
        cx: &gpui::App,
    ) -> Vec<ferris_idea_core::buildtasks::Suggestion> {
        let Some(prompt) = &self.name_prompt else {
            return Vec::new();
        };
        let PromptKind::BuildCommand(module) = &prompt.kind else {
            return Vec::new();
        };
        let line = prompt.field.read(cx).state.buffer.text();
        let mut cache = prompt.suggest.cache.borrow_mut();
        if cache.0.as_deref() != Some(line.as_str()) {
            let list = ferris_idea_core::buildtasks::command_suggestions(
                &self.root,
                module,
                &prompt.suggest.profiles,
                &line,
            );
            *cache = (Some(line), list);
        }
        cache.1.clone()
    }

    /// Puts suggestion `i` of the Execute field on its line (`SPEC-0100` v3 behaviour 20).
    pub fn take_suggestion(&mut self, i: usize, cx: &mut Context<Self>) -> bool {
        let list = self.build_command_suggestions(cx);
        let Some(s) = list.get(i) else {
            return false;
        };
        let Some(prompt) = self.name_prompt.as_mut() else {
            return false;
        };
        let line = prompt.field.read(cx).state.buffer.text();
        let new = ferris_idea_core::buildtasks::complete_line(&line, &s.text);
        prompt.field.update(cx, |f, cx| {
            f.set_text(&new, cx);
            let end = new.chars().count();
            f.select_range(end..end, cx);
        });
        prompt.suggest.cursor = None;
        cx.notify();
        true
    }

    /// The chosen suggestion of the Execute field, if one was chosen with ↓/↑.
    pub fn suggestion_cursor(&self) -> Option<usize> {
        self.name_prompt.as_ref().and_then(|p| p.suggest.cursor)
    }

    /// Types `text` into the open name field (self-test).
    pub fn set_name_field(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = &self.name_prompt {
            p.field.update(cx, |f, cx| f.set_text(text, cx));
        }
    }

    /// The text of the open name field (self-test).
    pub fn name_field_text(&self, cx: &gpui::App) -> Option<String> {
        self.name_prompt
            .as_ref()
            .map(|p| p.field.read(cx).state.buffer.text())
    }

    /// The title of the open name field and its error, if one is open.
    pub fn name_prompt_state(&self) -> Option<(String, Option<String>)> {
        self.name_prompt
            .as_ref()
            .map(|p| (prompt_title(&p.kind, &self.root), p.error.clone()))
    }

    /// Enter in the name field: create or rename; an error keeps the field open with the reason.
    /// Escape (`keep` false) closes it.
    pub fn finish_name_prompt(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.name_prompt.as_ref() else {
            return;
        };
        if !keep {
            let layout = matches!(
                prompt.kind,
                PromptKind::NewLayout(_) | PromptKind::RenameLayout(_)
            );
            self.name_prompt = None;
            if layout {
                // The layout actions come from the menu bar, not from the tree.
                self.focus_active(window, cx);
            } else {
                window.focus(&self.tree_focus);
            }
            cx.notify();
            return;
        }
        let name = prompt.field.read(cx).state.buffer.text();
        let name = name.trim_end_matches(['\n', '\r']).to_owned();
        let kind = prompt.kind.clone();
        // A layout's name (SPEC-0333 behaviours 4, 5 and 10): a refused name keeps the field.
        if matches!(kind, PromptKind::NewLayout(_) | PromptKind::RenameLayout(_)) {
            let done = match kind {
                PromptKind::NewLayout(arrangement) => self.save_new_layout(&name, *arrangement, cx),
                PromptKind::RenameLayout(old) => self.rename_layout(&old, &name, cx),
                _ => Ok(()),
            };
            match done {
                Ok(()) => {
                    self.name_prompt = None;
                    self.focus_active(window, cx);
                }
                Err(e) => {
                    if let Some(prompt) = self.name_prompt.as_mut() {
                        prompt.error = Some(e);
                    }
                }
            }
            cx.notify();
            return;
        }
        if let PromptKind::BuildCommand(module) = &kind {
            match self.run_build_command(module, &name, window, cx) {
                Ok(()) => self.name_prompt = None,
                Err(e) => {
                    if let Some(prompt) = self.name_prompt.as_mut() {
                        prompt.error = Some(e);
                    }
                }
            }
            cx.notify();
            return;
        }
        if let PromptKind::ClassName { interface, place } = kind {
            self.name_prompt = None;
            self.choose_members_to_extract(interface, place, name, window, cx);
            return;
        }
        if let PromptKind::LinkBuildFile = kind {
            let view = self.ensure_build_tools(window, cx);
            match view.update(cx, |v, cx| v.link(&name, cx)) {
                Ok(()) => {
                    self.name_prompt = None;
                    window.focus(&view.read(cx).focus());
                }
                Err(e) => {
                    if let Some(prompt) = self.name_prompt.as_mut() {
                        prompt.error = Some(e);
                    }
                }
            }
            cx.notify();
            return;
        }
        if let PromptKind::Paste { from, dir, mode } = kind {
            match self.paste_files(&from, &dir, &name, mode, window, cx) {
                Ok(()) => self.name_prompt = None,
                Err(e) => {
                    if let Some(prompt) = self.name_prompt.as_mut() {
                        prompt.error = Some(e);
                    }
                }
            }
            cx.notify();
            return;
        }
        // New and Rename in a remote project act on the host (SPEC-0317 v3 behaviour 7).
        if self.remote_project.is_some() {
            let changed = match &kind {
                PromptKind::New(kind, dir) => {
                    self.remote_project_create(dir, &name, *kind == NewKind::Directory, window, cx)
                }
                PromptKind::Rename(path) if self.open_and_modified(path, cx) => {
                    Err("Save the open file first; it has changes".into())
                }
                PromptKind::Rename(path) => {
                    let path = path.clone();
                    let renamed = self.remote_project_rename(&path, &name, cx);
                    if renamed.is_ok() {
                        self.close_tabs_under(&path, window, cx);
                    }
                    renamed
                }
                _ => Ok(()),
            };
            match changed {
                Ok(()) => self.name_prompt = None,
                Err(e) => {
                    if let Some(prompt) = self.name_prompt.as_mut() {
                        prompt.error = Some(e);
                    }
                }
            }
            cx.notify();
            return;
        }
        let result = match &kind {
            PromptKind::Paste { .. }
            | PromptKind::BuildCommand(_)
            | PromptKind::LinkBuildFile
            | PromptKind::NewLayout(_)
            | PromptKind::RenameLayout(_)
            | PromptKind::ClassName { .. } => {
                return;
            }
            // New and Rename are steps of the file operations' history (v16 behaviour 37).
            PromptKind::New(kind, dir) => self.file_undo.create(dir, &name, *kind),
            PromptKind::Rename(path) => {
                if self.open_and_modified(path, cx) {
                    Err(file_ops::FileOpError::Io(
                        "Save the open file first; it has changes".into(),
                    ))
                } else {
                    self.file_undo.rename(path, &name)
                }
            }
        };
        match result {
            Err(e) => {
                if let Some(prompt) = self.name_prompt.as_mut() {
                    prompt.error = Some(e.to_string());
                }
                cx.notify();
            }
            Ok(path) => {
                self.name_prompt = None;
                if let PromptKind::Rename(old) = &kind {
                    // A renamed file's tabs follow in every group (behaviour 5); tabs of
                    // files below a renamed folder close.
                    if path.is_file() {
                        self.tabs_follow(old, &path, window, cx);
                    }
                    self.close_tabs_under(old, window, cx);
                }
                self.after_file_change(&path, window, cx);
            }
        }
    }

    /// Whether an editor of `path` or of a file below it has unsaved changes, in whichever
    /// group it is shown (`SPEC-0112` v5 behaviour 10).
    pub(super) fn open_and_modified(&self, path: &Path, cx: &Context<Self>) -> bool {
        self.tabs_under(path)
            .iter()
            .any(|(_, p)| self.doc_modified(p, cx))
    }

    /// Closes the tabs of `path` and of files below it, in every group; the main group keeps
    /// no document of them either.
    pub(super) fn close_tabs_under(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let canonical = path.canonicalize().unwrap_or(path.to_path_buf());
        while let Some((gid, at)) = self.tabs_under(path).into_iter().next() {
            let Some(i) = self.group_tabs(gid).and_then(|t| t.position(&at)) else {
                break;
            };
            self.close_tab(gid, i, window, cx);
        }
        self.editors
            .retain(|p, _| !(p.starts_with(&canonical) || p.starts_with(path)));
    }

    /// After a file was created or renamed: the tree shows and selects it, a file opens in the
    /// editor (behaviour 4) unless a tab already shows it (a renamed file's tabs followed).
    fn after_file_change(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.tree = self.tree.reloaded();
        let canonical = path.canonicalize().unwrap_or(path.to_path_buf());
        let root = self.root.canonicalize().unwrap_or(self.root.clone());
        let in_tree = canonical
            .strip_prefix(&root)
            .map(|rel| self.root.join(rel))
            .unwrap_or(canonical.clone());
        self.tree.reveal(&in_tree);
        self.scroll_tree_to_selection();
        if canonical.is_file() {
            if self.tabs_under(&canonical).is_empty() {
                self.open(&canonical, window, cx);
            }
            self.focus_active(window, cx);
        } else {
            window.focus(&self.tree_focus);
        }
        cx.notify();
    }

    /// The file clipboard while Paste still means it: the system clipboard holds the text Cut or
    /// Copy put there (v2 behaviour 18).
    pub(super) fn pastable_files(&self, cx: &Context<Self>) -> Option<FileClipboard> {
        let clip = self.file_clipboard.as_ref()?;
        let text = cx.read_from_clipboard().and_then(|c| c.text());
        clip.current(text.as_deref()).then(|| clip.clone())
    }

    /// The items an action from the menu opened on `path` acts on: the whole selection when `path`
    /// is in it, else `path` alone (v3 behaviour 25).
    pub(super) fn tree_targets(&self, path: &Path) -> Vec<PathBuf> {
        let selected = self.tree.selected_paths();
        if selected.len() > 1 && selected.iter().any(|p| p == path) {
            selected
        } else {
            vec![path.to_path_buf()]
        }
    }

    /// Whether `path` is the project folder.
    fn is_project_folder(&self, path: &Path) -> bool {
        let root = self.root.canonicalize().unwrap_or(self.root.clone());
        path == self.root || path == root
    }

    /// `$Cut`, `$Copy` or `$Paste` on the project tree's selection (⌘X, ⌘C, ⌘V there, also from
    /// the menu bar).
    pub fn tree_clipboard_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Moving and copying files on the host come with a later step (SPEC-0317 v3).
        if self.remote_project.is_some() {
            self.status = Some(
                "Cut, copy and paste of files are not available in remote projects yet; use Rename"
                    .into(),
            );
            cx.notify();
            return;
        }
        let paths = self.tree.selected_paths();
        // The extra nodes, their libraries and fixed folders are no files to move (SPEC-0061 v4
        // behaviour 16); the project folder keeps its own message below.
        let fixed = self
            .tree
            .selection()
            .into_iter()
            .any(|n| n != 0 && self.tree.is_fixed(n));
        let into_virtual = self.tree.is_virtual(self.tree.selected);
        if (fixed && id != model::PASTE_FILE) || (into_virtual && id == model::PASTE_FILE) {
            self.status = Some("This node is not a file or folder of the project".into());
            cx.notify();
            return;
        }
        match id {
            model::CUT_FILE => self.cut_or_copy_files(&paths, Transfer::Move, cx),
            model::COPY_FILE => self.cut_or_copy_files(&paths, Transfer::Copy, cx),
            model::PASTE_FILE => {
                let target = self.tree.selected_path().to_path_buf();
                self.start_paste(target, window, cx);
            }
            _ => {}
        }
    }

    /// Cut (`Transfer::Move`) or Copy of project tree paths: remembered for Paste, their paths as
    /// text on the system clipboard (v2 behaviours 17 and 18, v3 behaviour 25).
    pub fn cut_or_copy_files(&mut self, paths: &[PathBuf], mode: Transfer, cx: &mut Context<Self>) {
        if paths.is_empty() || paths.iter().any(|p| self.is_project_folder(p)) {
            self.status = Some("The project folder itself cannot be cut or copied".into());
            cx.notify();
            return;
        }
        let clip = FileClipboard::new(paths, mode);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(clip.text()));
        self.status = Some(clip.status());
        self.file_clipboard = Some(clip);
        cx.notify();
    }

    /// Paste into the selected folder (or the selected file's folder): refuses with the reason in
    /// the status bar, opens the name field with a free name proposed for one item, or pastes
    /// several at once (v2 behaviours 19, 20; v3 behaviour 25).
    pub fn start_paste(&mut self, target: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(clip) = self.pastable_files(cx) else {
            self.status =
                Some("Nothing to paste: Cut or Copy a file in the project tree first".into());
            cx.notify();
            return;
        };
        let dir = crate::file_clipboard::paste_dir(&target, target.is_dir(), &self.root);
        if clip.mode == Transfer::Move && clip.paths.iter().any(|p| self.open_and_modified(p, cx)) {
            self.status = Some("Save the open file first; it has changes".into());
            cx.notify();
            return;
        }
        let [from] = clip.paths.as_slice() else {
            self.paste_several(&clip, &dir, window, cx);
            return;
        };
        self.open_transfer_prompt(from, dir, clip.mode, window, cx);
    }

    /// `CopyElement`, Copy… (v18 behaviour 46): the name field of Paste for a copy of the item
    /// selected in the Project tree, or of the active editor's file, in its own folder, with a
    /// free name proposed. False without such an item.
    pub(super) fn copy_element(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let from = if self.tree_focus.is_focused(window) {
            match self.tree.selected_paths().as_slice() {
                [one] => one.clone(),
                _ => {
                    self.status = Some(crate::keymap_actions::ONE_TO_COPY.into());
                    cx.notify();
                    return true;
                }
            }
        } else {
            match self.active_path() {
                Some(path) => path,
                None => return false,
            }
        };
        if self.remote_project.is_some() || self.is_project_folder(&from) {
            self.status = Some(crate::keymap_actions::ONE_TO_COPY.into());
            cx.notify();
            return true;
        }
        let Some(dir) = from.parent().map(Path::to_path_buf) else {
            return false;
        };
        self.open_transfer_prompt(&from, dir, Transfer::Copy, window, cx);
        true
    }

    /// The name field for one item copied or moved into `dir`, with a free name proposed and
    /// its stem selected; a transfer that cannot be is refused in the status bar.
    fn open_transfer_prompt(
        &mut self,
        from: &Path,
        dir: PathBuf,
        mode: Transfer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = from
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let is_dir = from.is_dir();
        let dir_now = dir.canonicalize().unwrap_or(dir.clone());
        let proposed = file_ops::free_name(&dir_now, &name, is_dir);
        if let Err(e) = file_ops::transfer_plan(&self.root, from, &dir, Some(&proposed), mode) {
            self.status = Some(e.to_string());
            cx.notify();
            return;
        }
        let stem = match proposed.rfind('.') {
            Some(dot) if dot > 0 && !is_dir => proposed[..dot].chars().count(),
            _ => proposed.chars().count(),
        };
        let field = cx.new(EditorView::field);
        field.update(cx, |f, cx| {
            f.set_text(&proposed, cx);
            f.select_range(0..stem, cx);
        });
        window.focus(&field.read(cx).focus());
        self.name_prompt = Some(NamePrompt {
            kind: PromptKind::Paste {
                from: from.to_path_buf(),
                dir,
                mode,
            },
            field,
            error: None,
            suggest: Suggest::default(),
        });
        cx.notify();
    }

    /// Whether moving `from` into `dir` under `name` runs as Move (F6) with the language server's
    /// edits: a Java or Kotlin file keeping its name, into another folder (v3 behaviour 26).
    fn moves_as_refactoring(from: &Path, dir: &Path, name: &str, mode: Transfer) -> bool {
        let canonical = |p: &Path| p.canonicalize().unwrap_or(p.to_path_buf());
        mode == Transfer::Move
            && from.is_file()
            && ferris_idea_languages::move_file::has_package_line(from)
            && from
                .file_name()
                .is_some_and(|n| n.to_string_lossy() == name)
            && from.parent().map(canonical) != Some(canonical(dir))
    }

    /// Copies or moves one item plainly; open tabs of moved files follow; a Java or Kotlin file
    /// gets the package line of its new folder (v2 behaviours 21, 22; v3 behaviour 26).
    pub(super) fn transfer_one(
        &mut self,
        from: &Path,
        dir: &Path,
        name: &str,
        mode: Transfer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<file_ops::TransferPlan, String> {
        // A step of the file operations' history, with the package line it changed (v16
        // behaviours 37 and 39).
        let root = self.root.clone();
        let plan = self
            .file_undo
            .transfer(from, dir, Some(name), mode, |plan| {
                if !plan.dir
                    && let Some(old_dir) = plan.from.parent()
                {
                    let _ = ferris_idea_languages::move_file::update_package_line(
                        &root, &plan.to, old_dir,
                    );
                }
            })
            .map_err(|e| e.to_string())?;
        if mode == Transfer::Move {
            // In every group (behaviour 21); a tab may hold the path as it was given.
            self.tabs_follow(&plan.from, &plan.to, window, cx);
            self.tabs_follow(from, &plan.to, window, cx);
        }
        Ok(plan)
    }

    /// Copies or moves `from` into `dir` as `name`; open tabs of moved files follow, the tree shows
    /// and selects the result (v2 behaviours 21, 22). A cut Java or Kotlin file keeping its name
    /// moves as Move (F6) does (v3 behaviour 26). An error is the reason for the field.
    fn paste_files(
        &mut self,
        from: &Path,
        dir: &Path,
        name: &str,
        mode: Transfer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if mode == Transfer::Move && self.open_and_modified(from, cx) {
            return Err("Save the open file first; it has changes".into());
        }
        if Self::moves_as_refactoring(from, dir, name, mode) {
            file_ops::transfer_plan(&self.root, from, dir, Some(name), mode)
                .map_err(|e| e.to_string())?;
            self.file_clipboard = None;
            self.move_classes(
                vec![from.to_path_buf()],
                dir.to_path_buf(),
                Vec::new(),
                window,
                cx,
            );
            return Ok(());
        }
        let plan = self.transfer_one(from, dir, name, mode, window, cx)?;
        if mode == Transfer::Move {
            self.file_clipboard = None;
        }
        self.status = Some(plan.summary(&self.root));
        self.show_pasted(&[plan.to], window, cx);
        Ok(())
    }

    /// Paste of several items: all are checked first, then copied or moved, each under its own
    /// name or a free one; cut Java and Kotlin files move as Move (F6) does, one after the other
    /// (v3 behaviours 25 and 26, IMP-0281).
    fn paste_several(
        &mut self,
        clip: &FileClipboard,
        dir: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let plans = match file_ops::transfer_plans(&self.root, &clip.paths, dir, clip.mode) {
            Ok(plans) => plans,
            Err(e) => {
                self.status = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let mut done = Vec::new();
        let mut classes = Vec::new();
        let steps_before = self.file_undo.steps().0.len();
        for plan in plans {
            let name = plan
                .to
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if Self::moves_as_refactoring(&plan.from, dir, &name, clip.mode) {
                classes.push(plan.from);
                continue;
            }
            match self.transfer_one(&plan.from, dir, &name, clip.mode, window, cx) {
                Ok(p) => done.push(p.to),
                Err(e) => {
                    self.file_undo.join_last(done.len());
                    self.status = Some(e);
                    self.show_pasted(&done, window, cx);
                    return;
                }
            }
        }
        // Several pasted items are one step (v16 behaviour 37).
        let pushed = self.file_undo.steps().0.len().saturating_sub(steps_before);
        self.file_undo.join_last(pushed.min(done.len()));
        if clip.mode == Transfer::Move {
            self.file_clipboard = None;
        }
        if classes.is_empty() {
            let root = self.root.canonicalize().unwrap_or(self.root.clone());
            let dir_now = dir.canonicalize().unwrap_or(dir.to_path_buf());
            self.status = Some(crate::file_clipboard::pasted_status(
                clip.mode,
                done.len(),
                &dir_now,
                &root,
            ));
            self.show_pasted(&done, window, cx);
        } else {
            self.move_classes(classes, dir.to_path_buf(), done, window, cx);
        }
    }

    /// The tree read again, showing and selecting `pasted` (absolute paths), with the focus.
    pub(super) fn show_pasted(
        &mut self,
        pasted: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tree = self.tree.reloaded();
        let root = self.root.canonicalize().unwrap_or(self.root.clone());
        let in_tree: Vec<PathBuf> = pasted
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .map(|rel| self.root.join(rel))
                    .unwrap_or(p.clone())
            })
            .collect();
        self.tree.reveal_all(&in_tree);
        self.scroll_tree_to_selection();
        window.focus(&self.tree_focus);
        cx.notify();
    }

    /// Moves Java and Kotlin files into `dir` as Move (F6) does, one after the other, each planned
    /// in the background after the previous one was written; then the tree selects them and the
    /// items in `done` (v3 behaviour 26).
    fn move_classes(
        &mut self,
        files: Vec<PathBuf>,
        dir: PathBuf,
        done: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.root.canonicalize().unwrap_or(self.root.clone());
        let dir_now = dir.canonicalize().unwrap_or(dir.clone());
        let target = dir_now
            .strip_prefix(&root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        self.status = Some(format!(
            "Moving {} to {}…",
            if files.len() == 1 {
                files[0]
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                format!("{} files", files.len())
            },
            if target.is_empty() {
                "the project folder"
            } else {
                &target
            }
        ));
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let mut pasted = done;
            let mut notes = Vec::new();
            for file in files {
                let (root, target) = (root.clone(), target.clone());
                let plan = cx
                    .background_spawn(async move {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| e.to_string())?;
                        rt.block_on(ferris_idea_languages::move_file::plan_move(
                            &root,
                            &file,
                            &target,
                            std::time::Duration::from_secs(30),
                        ))
                    })
                    .await;
                let applied = this.update_in(cx, |this, window, cx| {
                    let plan = plan?;
                    let to = plan.to.clone();
                    let note = this.apply_move(plan, true, window, cx)?;
                    Ok::<_, String>((to, note))
                });
                match applied {
                    Ok(Ok((to, note))) => {
                        pasted.push(to);
                        notes.push(note);
                    }
                    Ok(Err(e)) => {
                        notes.push(format!("Move: {e}"));
                        break;
                    }
                    Err(_) => return,
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.status = Some(notes.join("; "));
                this.show_pasted(&pasted, window, cx);
            });
        })
        .detach();
    }

    /// Delete… of one item or the whole selection: asks once, naming what goes (behaviour 6, v3
    /// behaviour 25), in a native alert (SPEC-0281 behaviour 6) or, in the window self-test, drawn
    /// in the window.
    pub fn ask_delete(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        if self.delete_in_alert {
            return;
        }
        // A remote project's files are deleted on the host (SPEC-0317 v3 behaviour 7).
        let plans = if self.remote_project.is_some() {
            Ok(self.remote_delete_plans(paths))
        } else {
            file_ops::delete_plans(&self.root, paths)
        };
        match plans {
            Ok(plans) if crate::dialogs::native_alerts(cx) => {
                let (message, detail, buttons) = delete_alert(&plans);
                self.delete_confirm = Some(plans);
                self.delete_in_alert = true;
                let reply = window.prompt(
                    gpui::PromptLevel::Warning,
                    &message,
                    Some(detail),
                    &[
                        gpui::PromptButton::ok(buttons[0]),
                        gpui::PromptButton::cancel(buttons[1]),
                    ],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let yes = reply.await == Ok(0);
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.delete_in_alert = false;
                        this.finish_delete(yes, window, cx);
                    });
                })
                .detach();
            }
            Ok(plans) => self.delete_confirm = Some(plans),
            Err(e) => self.status = Some(e.to_string()),
        }
        cx.notify();
    }

    /// The question of the open Delete confirmation.
    pub fn delete_question(&self) -> Option<String> {
        self.delete_confirm
            .as_deref()
            .map(file_ops::delete_question)
    }

    /// Answers the Delete confirmation.
    pub fn finish_delete(&mut self, yes: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plans) = self.delete_confirm.take() else {
            return;
        };
        if yes {
            let mut deleted = Vec::new();
            let remote = self.remote_project.is_some();
            // What the undo store keeps (v16 behaviour 41): how many of the items, and why one
            // is not kept.
            let (mut kept, mut not_kept) = (0, None);
            for plan in &plans {
                self.close_tabs_under(&plan.path, window, cx);
                let done = if remote {
                    self.remote_project_delete(plan)
                } else {
                    self.file_undo
                        .delete(&plan.path)
                        .map(|d| {
                            match d.not_kept {
                                None => kept += 1,
                                Some(why) => not_kept = Some(why),
                            }
                            d.plan.path
                        })
                        .map_err(|e| e.to_string())
                };
                match done {
                    Ok(path) => deleted.push(path),
                    Err(e) => {
                        self.status = Some(e);
                        break;
                    }
                }
            }
            if deleted.len() == plans.len() {
                let what = match deleted.as_slice() {
                    [one] => one
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    many => format!("{} items", many.len()),
                };
                self.status = Some(if remote {
                    format!("Deleted {what} on the host")
                } else {
                    match not_kept {
                        None => format!("Deleted {what} (kept in Local History)"),
                        Some(why) => format!("Deleted {what}: {why}"),
                    }
                });
            }
            // Several deleted items are one step (v16 behaviour 37).
            self.file_undo.join_last(kept);
            self.tree = self.tree.reloaded();
        }
        window.focus(&self.tree_focus);
        cx.notify();
    }

    /// Keys while a menu, the name field or the Delete confirmation is open; returns whether the
    /// key was used.
    pub(super) fn overlay_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = event.keystroke.key.as_str();
        if self.delete_confirm.is_some() && !self.delete_in_alert {
            match key {
                "enter" => self.finish_delete(true, window, cx),
                "escape" => self.finish_delete(false, window, cx),
                _ => {}
            }
            return true;
        }
        if self.name_prompt.is_some() {
            // The Execute field's suggestions (SPEC-0100 v3 behaviour 20).
            let count = self.build_command_suggestions(cx).len();
            if count > 0 {
                let cursor = self.suggestion_cursor();
                match key {
                    "down" | "up" => {
                        let next = match (cursor, key == "down") {
                            (None, true) => 0,
                            (None, false) => count - 1,
                            (Some(c), true) => (c + 1) % count,
                            (Some(c), false) => (c + count - 1) % count,
                        };
                        if let Some(p) = self.name_prompt.as_mut() {
                            p.suggest.cursor = Some(next);
                        }
                        cx.notify();
                        return true;
                    }
                    "tab" => {
                        self.take_suggestion(cursor.unwrap_or(0), cx);
                        return true;
                    }
                    "enter" if cursor.is_some() => {
                        self.take_suggestion(cursor.unwrap_or(0), cx);
                        return true;
                    }
                    _ => {}
                }
            }
            match key {
                "enter" => self.finish_name_prompt(true, window, cx),
                "escape" => self.finish_name_prompt(false, window, cx),
                _ => {
                    // Typing changes the line: the chosen suggestion no longer applies.
                    if let Some(p) = self.name_prompt.as_mut() {
                        p.suggest.cursor = None;
                    }
                    return false;
                }
            }
            return true;
        }
        let Some(menu) = self.context_menu.as_mut() else {
            return false;
        };
        let sub_entries = menu.sub.and_then(|s| match &menu.entries[s] {
            MenuEntry::Sub(_, entries) => Some(entries.clone()),
            _ => None,
        });
        match (key, sub_entries) {
            ("escape", Some(_)) | ("left", Some(_)) => {
                menu.sub = None;
                menu.sub_cursor = None;
            }
            ("escape", None) => self.context_menu = None,
            ("up" | "down", Some(sub)) => {
                menu.sub_cursor = step(&sub, menu.sub_cursor, key == "down");
            }
            ("up" | "down", None) => menu.cursor = step(&menu.entries, menu.cursor, key == "down"),
            ("right" | "enter", None) => {
                if let Some(i) = menu.cursor {
                    match &menu.entries[i] {
                        MenuEntry::Sub(_, sub) => {
                            menu.sub = Some(i);
                            menu.sub_cursor = step(sub, None, true);
                        }
                        MenuEntry::Item(item) if key == "enter" && item.enabled => {
                            let action = item.action.clone();
                            self.run_context_item(&action, window, cx);
                        }
                        _ => {}
                    }
                }
            }
            ("enter" | "right", Some(sub)) => {
                if let Some(MenuEntry::Item(item)) = menu.sub_cursor.and_then(|k| sub.get(k))
                    && item.enabled
                    && key == "enter"
                {
                    let action = item.action.clone();
                    self.run_context_item(&action, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
        true
    }

    /// The open menu, the name field and the Delete confirmation, drawn over the window.
    pub(super) fn overlays(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let mut out = Vec::new();
        if let Some(menu) = &self.context_menu {
            // A click anywhere else closes the menu (and goes no further).
            out.push(
                gpui::deferred(
                    div()
                        .id("context-menu-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.close_context_menu(cx)),
                        )
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(|this, _, _, cx| this.close_context_menu(cx)),
                        ),
                )
                .with_priority(1)
                .into_any_element(),
            );
            let flip =
                f32::from(menu.at.x) + 2. * MENU_WIDTH > f32::from(window.viewport_size().width);
            out.push(
                gpui::deferred(
                    gpui::anchored()
                        .position(menu.at)
                        .snap_to_window_with_margin(px(4.))
                        .child(self.menu_frame(menu, flip, cx)),
                )
                .with_priority(2)
                .into_any_element(),
            );
        }
        if let Some(prompt) = &self.name_prompt {
            out.push(
                gpui::deferred(self.prompt_frame(prompt, cx))
                    .with_priority(3)
                    .into_any_element(),
            );
        }
        if let Some(plans) = self
            .delete_confirm
            .as_ref()
            .filter(|_| !self.delete_in_alert)
        {
            out.push(
                gpui::deferred(self.delete_frame(plans, cx))
                    .with_priority(3)
                    .into_any_element(),
            );
        }
        out
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu = None;
        cx.notify();
    }

    fn menu_frame(&self, menu: &OpenMenu, flip: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let gutter = checks_gutter(&menu.entries);
        let entries = menu.entries.iter().enumerate().map(|(i, e)| {
            // The open submenu beside its entry (left of it near the right edge).
            let sub = match e {
                MenuEntry::Sub(_, entries) if menu.sub == Some(i) => {
                    let frame = div().absolute().top(px(-6.));
                    let (frame, corner) = if flip {
                        (frame.left(px(0.)), gpui::Corner::TopRight)
                    } else {
                        (frame.left(gpui::relative(1.)), gpui::Corner::TopLeft)
                    };
                    // Anchored, so a submenu near the window's bottom moves up into the window
                    // instead of being cut off (SPEC-0275 v9; the Git submenu showed two rows).
                    Some(
                        frame.child(
                            gpui::anchored()
                                .anchor(corner)
                                .snap_to_window_with_margin(px(4.))
                                .child(self.entries_frame(entries, menu.sub_cursor, cx)),
                        ),
                    )
                }
                _ => None,
            };
            self.entry(
                e,
                i,
                menu.cursor == Some(i) || menu.sub == Some(i),
                true,
                gutter,
                cx,
            )
            .children(sub)
            .into_any_element()
        });
        div().occlude().child(
            crate::metrics::menu_frame()
                .bg(c(p.island))
                .min_w(px(MENU_WIDTH))
                .flex()
                .flex_col()
                .children(entries),
        )
    }

    /// A frame with `entries` (a submenu).
    fn entries_frame(
        &self,
        entries: &[MenuEntry],
        cursor: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let gutter = checks_gutter(entries);
        crate::metrics::menu_frame()
            .occlude()
            .bg(c(p.island))
            .min_w(px(MENU_WIDTH - 20.))
            .flex()
            .flex_col()
            .children(entries.iter().enumerate().map(|(i, e)| {
                self.entry(e, i, cursor == Some(i), false, gutter, cx)
                    .into_any_element()
            }))
    }

    /// One entry; `top` for the entries of the menu itself (submenus open on hover); `gutter` keeps
    /// a column for check marks so the labels of a menu with a switch that is on stay aligned.
    fn entry(
        &self,
        e: &MenuEntry,
        i: usize,
        selected: bool,
        top: bool,
        gutter: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current();
        let id = gpui::SharedString::from(format!("ctx-{}-{i}", if top { "top" } else { "sub" }));
        match e {
            MenuEntry::Separator => div()
                .id(id)
                .h(px(1.))
                .my(px(4.))
                .mx(px(8.))
                .bg(c(p.separator)),
            MenuEntry::Item(item) => {
                let shortcut = crate::context_menu::shortcut_label(
                    &item.action,
                    &crate::keymaps::current(),
                    cfg!(target_os = "macos"),
                );
                let action = item.action.clone();
                crate::metrics::menu_item(id, selected && item.enabled)
                    .relative()
                    .children(crate::hit_targets::mark(format!("menu-{}", item.label)))
                    .gap_4()
                    .when(!item.enabled, |d| d.text_color(c(p.ui_dim)))
                    // A switch that is on (SPEC-0100 v3 behaviour 18), in a column of its own so
                    // every label of the menu starts at the same place (SPEC-0100 v4).
                    .when(gutter, |d| {
                        d.child(div().w(px(12.)).flex_none().child(if item.checked {
                            "✓"
                        } else {
                            ""
                        }))
                    })
                    .child(div().flex_1().child(item.label.clone()))
                    .child(div().text_color(c(p.ui_dim)).child(shortcut))
                    .when(top, |d| {
                        d.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && let Some(m) = this.context_menu.as_mut() {
                                m.sub = None;
                                m.cursor = Some(i);
                                cx.notify();
                            }
                        }))
                    })
                    .when(item.enabled, |d| {
                        d.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.run_context_item(&action, window, cx);
                        }))
                    })
            }
            MenuEntry::Sub(label, _) => crate::metrics::menu_item(id, selected)
                .relative()
                .children(crate::hit_targets::mark(format!("menu-{label}")))
                .gap_4()
                .when(gutter, |d| d.child(div().w(px(12.)).flex_none()))
                .child(div().flex_1().child(label.clone()))
                .child(icon("general/chevronRight"))
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered && let Some(m) = this.context_menu.as_mut() {
                        if m.sub != Some(i) {
                            m.sub = Some(i);
                            m.sub_cursor = None;
                        }
                        m.cursor = Some(i);
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(m) = this.context_menu.as_mut() {
                        m.sub = Some(i);
                        cx.notify();
                    }
                })),
        }
    }

    /// A small frame near the top of the window, as the reference's popups for names.
    fn popup(&self, id: &'static str) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current();
        div()
            .id(id)
            .absolute()
            .top(px(96.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .text_color(c(p.ui_text))
    }

    fn prompt_frame(&self, prompt: &NamePrompt, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let suggestions = self.build_command_suggestions(cx);
        let cursor = prompt.suggest.cursor;
        let hint = match &prompt.kind {
            PromptKind::New(NewKind::Directory, _) => "Enter creates it; a/b creates both folders",
            PromptKind::New(..) => "Enter creates it and opens it in the editor",
            PromptKind::Rename(_) => "Enter renames it",
            PromptKind::Paste {
                mode: Transfer::Copy,
                ..
            } => "Enter copies it; an existing name is never overwritten",
            PromptKind::Paste { .. } => "Enter moves it; an existing name is never overwritten",
            PromptKind::BuildCommand(_) => {
                "Enter runs it in the module with the window's options (offline, skip tests, profiles); Tab completes"
            }
            PromptKind::ClassName { .. } => "Enter chooses the members",
            PromptKind::NewLayout(_) => "Enter saves the arrangement of the tool windows",
            PromptKind::RenameLayout(_) => "Enter renames the layout",
            PromptKind::LinkBuildFile => {
                "Enter links it: pom.xml, build.gradle[.kts] or settings.gradle[.kts], also in an ignored folder"
            }
        };
        self.popup("name-prompt").child(
            div()
                .occlude()
                .w(px(380.))
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .bg(c(p.popup))
                .border_1()
                .border_color(c(p.popup_border))
                .rounded(px(8.))
                .shadow_lg()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(prompt_title(&prompt.kind, &self.root)),
                )
                .child(
                    crate::metrics::field_frame()
                        .when(prompt.error.is_some(), |d| d.border_color(c(p.error_text)))
                        .child(div().flex_1().h(px(18.)).child(prompt.field.clone())),
                )
                .when(!suggestions.is_empty(), |d| {
                    d.child(
                        div()
                            .id("build-command-suggestions")
                            .flex()
                            .flex_col()
                            .children(suggestions.iter().enumerate().map(|(i, s)| {
                                crate::metrics::menu_item(
                                    gpui::SharedString::from(format!("build-suggestion-{i}")),
                                    cursor == Some(i),
                                )
                                .gap_4()
                                // The name keeps its width; a long detail (a plugin's
                                // coordinates) is cut with an ellipsis inside the popup
                                // (SPEC-0100 v4 behaviour 20).
                                .child(div().flex_none().child(s.text.clone()))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .text_right()
                                        .text_color(c(p.ui_dim))
                                        .child(s.detail.clone()),
                                )
                                .on_click(cx.listener(
                                    move |this, _: &ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        this.take_suggestion(i, cx);
                                        if let Some(p) = &this.name_prompt {
                                            window.focus(&p.field.read(cx).focus());
                                        }
                                    },
                                ))
                            })),
                    )
                })
                .child(match &prompt.error {
                    Some(e) => div().text_color(c(p.error_text)).child(e.clone()),
                    None => div().text_color(c(p.ui_dim)).child(hint),
                }),
        )
    }

    fn delete_frame(&self, plans: &[DeletePlan], cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        self.popup("delete-confirm").child(
            div()
                .occlude()
                .w(px(400.))
                .p_3()
                .flex()
                .flex_col()
                .gap_3()
                .bg(c(p.popup))
                .border_1()
                .border_color(c(p.popup_border))
                .rounded(px(8.))
                .shadow_lg()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Delete"),
                )
                .child(
                    div()
                        .whitespace_normal()
                        .child(file_ops::delete_question(plans)),
                )
                .child(
                    div()
                        .text_color(c(p.ui_dim))
                        .child("Deleted files stay in Local History."),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            crate::metrics::button("delete-cancel", "Cancel", false).on_click(
                                cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.finish_delete(false, window, cx)
                                }),
                            ),
                        )
                        .child(
                            crate::metrics::button("delete-ok", "Delete", true).on_click(
                                cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.finish_delete(true, window, cx)
                                }),
                            ),
                        ),
                ),
        )
    }

    /// Secondary click on project tree row `id`: selects it, or keeps the selection it is part of
    /// (v3 behaviour 24), and opens the menu (behaviour 1).
    pub(super) fn tree_secondary(
        &mut self,
        id: usize,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tree.secondary_click(id);
        window.focus(&self.tree_focus);
        // An extra node or a library has no file menu (SPEC-0061 v4 behaviour 16).
        if self.tree.is_virtual(id) {
            cx.notify();
            return;
        }
        let path = self.tree.path(id).to_path_buf();
        self.open_context_menu(MenuPlace::ProjectTree, Some(path), e.position, cx);
    }

    /// Secondary click on the editor text: caret, then the menu (behaviour 14). On the annotation
    /// column its own menu, on the rest of the gutter the gutter's menu (`SPEC-0277` behaviours 1
    /// and 6).
    pub(super) fn editor_secondary(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |ed, cx| ed.hide_annotation_hover(cx));
            let (column_line, gutter) = {
                let ed = editor.read(cx);
                (ed.annotation_line_at(e.position), ed.in_gutter(e.position))
            };
            if column_line.is_some() || gutter {
                let place = if column_line.is_some() {
                    MenuPlace::Annotations
                } else {
                    MenuPlace::Gutter
                };
                window.focus(&editor.read(cx).focus());
                let path = self.active_path();
                self.open_context_menu_at_line(place, path, column_line, e.position, cx);
                return;
            }
        }
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |ed, cx| ed.caret_for_menu(e.position, cx));
            window.focus(&editor.read(cx).focus());
        }
        self.open_context_menu(MenuPlace::Editor, self.active_path(), e.position, cx);
    }
}

/// Status text when Rename is asked for with several items selected (v3 behaviour 25).
pub(super) const ONE_TO_RENAME: &str = "Select one file or folder to rename it";
/// The status when F6 in the Project tree has no single item to move (`SPEC-0275` v22 behaviour 49).
pub(super) const ONE_TO_MOVE: &str = "Select one file or folder to move it";

/// The title of the name field.
fn prompt_title(kind: &PromptKind, root: &Path) -> String {
    match kind {
        PromptKind::Paste { from, dir, mode } => {
            let root = root.canonicalize().unwrap_or(root.to_path_buf());
            let dir = dir.canonicalize().unwrap_or(dir.clone());
            crate::file_clipboard::paste_title(*mode, from, &dir, &root)
        }
        PromptKind::New(k, _) => format!("New {}", k.label()),
        PromptKind::ClassName {
            interface: true, ..
        } => "Extract Interface".to_owned(),
        PromptKind::ClassName { .. } => "Extract Superclass".to_owned(),
        PromptKind::LinkBuildFile => "Link Build File".to_owned(),
        PromptKind::NewLayout(_) => "Save Current Layout as New".to_owned(),
        PromptKind::RenameLayout(name) => format!("Rename Layout {name}"),
        PromptKind::BuildCommand(m) => format!(
            "Execute {} in {}",
            match m.kind {
                ferris_idea_core::ModuleKind::Maven => "Maven Goal",
                ferris_idea_core::ModuleKind::Gradle => "Gradle Task",
                _ => "Command",
            },
            m.short_name()
        ),
        PromptKind::Rename(p) => format!(
            "Rename {}",
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
    }
}

/// Whether "Run '…'" makes sense for a file of this kind (a program on its own).
pub(super) fn runnable(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("py" | "rb" | "js" | "mjs" | "ts" | "go" | "java" | "kt" | "rs" | "c" | "cpp" | "sh")
    )
}

/// Message, detail and buttons (default first) of the Delete alert (SPEC-0281 behaviour 6).
pub fn delete_alert(plans: &[DeletePlan]) -> (String, &'static str, [&'static str; 2]) {
    (
        file_ops::delete_question(plans),
        "Deleted files stay in Local History.",
        ["Delete", "Cancel"],
    )
}

/// Whether a menu with `entries` needs the check mark column: some item of it is a switch that is
/// on.
fn checks_gutter(entries: &[MenuEntry]) -> bool {
    entries
        .iter()
        .any(|e| matches!(e, MenuEntry::Item(item) if item.checked))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_alert_question_and_buttons() {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-delete-alert-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/sub")).unwrap();
        std::fs::write(root.join("src/a.txt"), "a").unwrap();
        std::fs::write(root.join("report.sql"), "").unwrap();
        let file = file_ops::delete_plan(&root, &root.join("report.sql")).unwrap();
        let (message, detail, buttons) = delete_alert(std::slice::from_ref(&file));
        assert_eq!(message, "Delete file \"report.sql\"?");
        assert_eq!(detail, "Deleted files stay in Local History.");
        assert_eq!(buttons, ["Delete", "Cancel"]);
        let folder = file_ops::delete_plan(&root, &root.join("src")).unwrap();
        assert_eq!(
            delete_alert(std::slice::from_ref(&folder)).0,
            "Delete folder \"src\" with 1 file and 1 folder?"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// SPEC-0275 v17 AK-39: the New popup opens with its first entry chosen; the other menus
    /// open without one.
    #[test]
    fn the_new_popup_opens_with_its_first_entry_chosen() {
        let entries = model::new_entries();
        let first = first_cursor(MenuPlace::New, &entries).unwrap();
        assert_eq!(Some(first), step(&entries, None, true));
        assert!(
            matches!(entries[first], MenuEntry::Item(_)),
            "{:?}",
            entries[first]
        );
        assert_eq!(first_cursor(MenuPlace::ProjectTree, &entries), None);
        assert_eq!(first_cursor(MenuPlace::Editor, &entries), None);
    }

    #[test]
    fn keyboard_steps_skip_separators_and_disabled_items() {
        let entries = model::editor_menu(&model::EditorTarget {
            selection: false,
            runnable: None,
            repository: true,
            annotated: false,
            debugging: false,
        });
        // 0 Show Context Actions, 1 separator, 2 Paste (no Cut and Copy without a selection).
        assert_eq!(step(&entries, None, true), Some(0));
        assert_eq!(step(&entries, Some(0), true), Some(2));
        assert_eq!(step(&entries, Some(2), false), Some(0));
        let last = enabled_indexes(&entries).last().copied();
        assert_eq!(step(&entries, Some(0), false), last, "wraps");
        assert!(runnable(Path::new("a/main.py")) && !runnable(Path::new("notes.md")));
    }
}
