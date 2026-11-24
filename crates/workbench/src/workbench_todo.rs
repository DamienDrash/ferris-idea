//! The TODO tool window in the workbench (`SPEC-0334`): showing and hiding it, the scans of the
//! project and of the current file in the background, the jump to an item, the occurrence
//! actions and the grouping toggles.
//!
//! What a TODO item is and the scan are [`ferris_idea_languages::todo`]; what the tree shows is
//! [`crate::todo_model`]; the view is [`crate::todo_view`].

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ferris_idea_languages::todo::{Scan, scan_project, scan_text, shown_path};
use gpui::{AppContext, Context, Entity, Window};

use super::WorkbenchView;
use crate::keymap_actions;
use crate::todo_model::{Entry, NOT_OPEN, Scope};
use crate::todo_view::{Left, TodoEvent, TodoToolView};

/// How often the window looks at its files again (behaviour 12): the current file every tick,
/// the project every fourth.
const TICK: Duration = Duration::from_millis(500);
const PROJECT_EVERY: u32 = 4;

/// The state of the TODO tool window in the workbench.
#[derive(Default)]
pub(super) struct TodoState {
    /// The window's view, made when it is first shown.
    pub(super) tool: Option<Entity<TodoToolView>>,
    /// Whether the window is shown.
    pub(super) shown: bool,
    _events: Option<gpui::Subscription>,
    /// What the scans found per file (behaviour 11); the scan in the background holds it.
    cache: Arc<std::sync::Mutex<ferris_idea_languages::todo::Cache>>,
    /// The stop flag of the running scan of the project.
    scan: Option<Arc<AtomicBool>>,
    /// Something changed while a scan ran: another one follows it.
    stale: bool,
    /// The file the Current File scope was last asked for; `None` for no file.
    file: Option<PathBuf>,
    /// The current file was edited, saved or changed since it was last searched.
    file_stale: bool,
    /// A search of the current file runs.
    file_busy: bool,
    ticks: u32,
}

impl TodoState {
    /// Tells the running scan to stop (behaviour 10).
    pub(super) fn stop(&mut self) {
        if let Some(stop) = self.scan.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Whether a scan of the project runs (`SPEC-0061` v16 behaviour 62).
    pub(super) fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Counts `stop` as the flag of a running scan (checks of the list of background tasks).
    pub(super) fn hold(&mut self, stop: Arc<AtomicBool>) {
        self.scan = Some(stop);
    }
}

impl Drop for TodoState {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The files of a scan as the tree takes them: each with the folder of its module.
fn entries(root: &Path, scan: Scan) -> Vec<Entry> {
    // The module of a folder is looked up once.
    let mut modules: HashMap<PathBuf, String> = HashMap::new();
    scan.files
        .into_iter()
        .map(|file| {
            let path = root.join(&file.path);
            let folder = path.parent().unwrap_or(root).to_path_buf();
            let module_dir = modules
                .entry(folder)
                .or_insert_with(|| {
                    let dir = ferris_idea_languages::context::module_folder(root, &path);
                    dir.strip_prefix(root).map(shown_path).unwrap_or_default()
                })
                .clone();
            Entry {
                path: file.path,
                module_dir,
                items: file.items,
            }
        })
        .collect()
}

impl WorkbenchView {
    /// The TODO tool window's view, once it was shown (checks).
    pub fn todo_tool_view(&self) -> Option<Entity<TodoToolView>> {
        self.todo.tool.clone()
    }

    /// `path` as the TODO window names it: relative to the project with `/` between its parts,
    /// or as it is outside the project.
    fn todo_path(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(relative) => shown_path(relative),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }

    /// Shows the TODO tool window and focuses its tree, or hides it (`ActivateTODOToolWindow`,
    /// behaviour 13).
    pub(super) fn toggle_todo_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.todo.shown {
            self.hide_todo_tool();
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        let tool = match self.todo.tool.clone() {
            Some(tool) => tool,
            None => {
                let root = self.root.clone();
                let remote = self.remote_project.is_some();
                let tool = cx.new(|cx| TodoToolView::new(root, remote, cx));
                self.todo._events = Some(cx.subscribe_in(
                    &tool,
                    window,
                    |this, _, event: &TodoEvent, window, cx| this.todo_event(event, window, cx),
                ));
                self.todo.tool = Some(tool.clone());
                // The window looks at its files again while it is shown (behaviour 12).
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(TICK).await;
                        if this.update(cx, |this, cx| this.todo_tick(cx)).is_err() {
                            break;
                        }
                    }
                })
                .detach();
                tool
            }
        };
        self.todo.shown = true;
        self.todo.file_stale = true;
        self.scan_todos(cx);
        self.scan_todo_file(cx);
        window.focus(&tool.read(cx).focus());
        cx.notify();
    }

    /// Hides the TODO window; its scan stops (behaviour 10).
    pub(super) fn hide_todo_tool(&mut self) {
        self.todo.shown = false;
        self.todo.stop();
        self.todo.stale = false;
    }

    /// Whether the window scans at all: shown, and not on a remote project (behaviour 23).
    fn todo_scans(&self) -> bool {
        self.todo.shown && self.todo.tool.is_some() && self.remote_project.is_none()
    }

    /// Every half second while the window lives.
    fn todo_tick(&mut self, cx: &mut Context<Self>) {
        if !self.todo_scans() {
            return;
        }
        self.todo.ticks = self.todo.ticks.wrapping_add(1);
        if self.todo.file != self.active_path() || self.todo.file_stale {
            self.scan_todo_file(cx);
        }
        if self.todo.ticks.is_multiple_of(PROJECT_EVERY) || self.todo.stale {
            self.scan_todos(cx);
        }
    }

    /// A file was edited in an editor: the current file's items follow (behaviour 12).
    pub(super) fn todo_edited(&mut self, path: &Path) {
        if self.todo.shown && self.todo.file.as_deref() == Some(path) {
            self.todo.file_stale = true;
        }
    }

    /// A file was saved in an editor: the project is scanned again (behaviour 12).
    pub(super) fn todo_saved(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.todo_scans() {
            return;
        }
        if self.todo.file.as_deref() == Some(path) {
            self.todo.file_stale = true;
        }
        self.scan_todos(cx);
    }

    /// The texts of the editors with unsaved changes, by file.
    fn todo_unsaved(&self, cx: &gpui::App) -> Vec<(PathBuf, String)> {
        self.editors
            .iter()
            .filter(|(_, editor)| editor.read(cx).is_modified())
            .map(|(path, editor)| (path.clone(), editor.read(cx).state.buffer.text()))
            .collect()
    }

    /// Scans the project in the background (behaviours 10–12). While a scan runs, another one
    /// is only noted and follows it.
    pub(super) fn scan_todos(&mut self, cx: &mut Context<Self>) {
        if !self.todo_scans() {
            return;
        }
        if self.todo.scan.is_some() {
            self.todo.stale = true;
            return;
        }
        self.todo.stale = false;
        let stop = Arc::new(AtomicBool::new(false));
        self.todo.scan = Some(stop.clone());
        let root = self.root.clone();
        let cache = self.todo.cache.clone();
        let unsaved = self.todo_unsaved(cx);
        let shown: HashMap<String, Arc<str>> = unsaved
            .iter()
            .map(|(path, text)| (self.todo_path(path), Arc::from(text.as_str())))
            .collect();
        let flag = stop.clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let workspace = ferris_idea_core::Workspace::open(&root).ok()?;
                    let unsaved: BTreeMap<PathBuf, String> = unsaved
                        .into_iter()
                        .filter_map(|(path, text)| {
                            Some((path.strip_prefix(&root).ok()?.to_path_buf(), text))
                        })
                        .collect();
                    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
                    let scan = scan_project(&workspace, &unsaved, &mut cache, &flag)?;
                    let left = Left {
                        truncated: scan.truncated,
                        not_read: scan.not_read,
                        not_checked: scan.not_checked,
                    };
                    Some((entries(&root, scan), left))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // A scan that was told to stop is not this window's scan any more.
                if !this
                    .todo
                    .scan
                    .as_ref()
                    .is_some_and(|mine| Arc::ptr_eq(mine, &stop))
                {
                    return;
                }
                this.todo.scan = None;
                if let (Some((entries, left)), Some(tool)) = (found, this.todo.tool.clone()) {
                    tool.update(cx, |t, cx| t.set_project(entries, left, shown, cx));
                }
                if this.todo.stale {
                    this.scan_todos(cx);
                }
            });
        })
        .detach();
    }

    /// Searches the file of the active editor in the background, with the editor's text
    /// (behaviours 9, 14).
    fn scan_todo_file(&mut self, cx: &mut Context<Self>) {
        if !self.todo_scans() || self.todo.file_busy {
            return;
        }
        let Some(tool) = self.todo.tool.clone() else {
            return;
        };
        let active = self.active_path();
        self.todo.file = active.clone();
        self.todo.file_stale = false;
        let Some((path, editor)) =
            active.and_then(|path| Some((path.clone(), self.editors.get(&path)?.clone())))
        else {
            tool.update(cx, |t, cx| t.set_file(None, None, cx));
            return;
        };
        let (text, modified) = {
            let editor = editor.read(cx);
            (editor.state.buffer.text(), editor.is_modified())
        };
        let shown = self.todo_path(&path);
        self.todo.file_busy = true;
        cx.spawn(async move |this, cx| {
            let (items, text) = cx
                .background_spawn(async move {
                    let items = scan_text(&path, &text).items;
                    (items, modified.then(|| Arc::<str>::from(text)))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.todo.file_busy = false;
                let entry = Entry {
                    path: shown,
                    module_dir: String::new(),
                    items,
                };
                tool.update(cx, |t, cx| t.set_file(Some(entry), text, cx));
            });
        })
        .detach();
    }

    /// Shows a scope of the TODO window (the tabs of its header, behaviour 14).
    pub fn set_todo_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        if let Some(tool) = self.todo.tool.clone() {
            tool.update(cx, |t, cx| t.set_scope(scope, cx));
        }
        if scope == Scope::File {
            self.todo.file_stale = true;
            self.scan_todo_file(cx);
        }
        cx.notify();
    }

    /// What the TODO window asks for (behaviours 18, 19).
    fn todo_event(&mut self, event: &TodoEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (file, place, keep_focus) = match event {
            TodoEvent::Open {
                path,
                line,
                column,
                keep_focus,
            } => (path, Some((*line, *column)), *keep_focus),
            TodoEvent::OpenFile(path) => (path, None, false),
        };
        let path = self.root.join(file);
        if !path.is_file() {
            self.status = Some(format!("{file} is not there"));
            cx.notify();
            return;
        }
        self.remember_place(cx);
        self.open(&path, window, cx);
        if let (Some((line, column)), Some(editor)) = (place, self.editors.get(&path)) {
            editor.update(cx, |e, cx| e.go_to(line, column, cx));
        }
        match self.todo.tool.as_ref().filter(|_| keep_focus) {
            Some(tool) => window.focus(&tool.read(cx).focus()),
            None => self.focus_active(window, cx),
        }
        cx.notify();
    }

    /// Whether `NextOccurence` and `PreviousOccurence` belong to the TODO window now (behaviour
    /// 19): it has the focus, or it is shown while the Find tool window shows no usages.
    pub(super) fn todo_takes_occurrences(&self, window: &Window, cx: &gpui::App) -> bool {
        let Some(tool) = self.todo.tool.as_ref().filter(|_| self.todo.shown) else {
            return false;
        };
        tool.read(cx).focus().contains_focused(window, cx)
            || self
                .find_tool_window()
                .is_none_or(|v| v.read(cx).tree().is_none())
    }

    /// The next or previous TODO item; `false` at the ends.
    pub(super) fn step_todo(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        self.todo
            .tool
            .clone()
            .is_some_and(|tool| tool.update(cx, |t, cx| t.step(forward, cx)))
    }

    /// `TodoViewGroupByShowModules`, `TodoViewGroupByShowPackages` and
    /// `TodoViewGroupByFlattenPackage`: flips the toggle of the TODO window (behaviour 25);
    /// before the window was ever shown the status line says so.
    pub(super) fn todo_grouping_action(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(toggle) = keymap_actions::todo_grouping(id) else {
            return false;
        };
        match self.todo.tool.clone() {
            Some(tool) => tool.update(cx, |t, cx| t.press(toggle, cx)),
            None => self.status = Some(NOT_OPEN.into()),
        }
        cx.notify();
        true
    }

    /// Whether the keyboard focus is in the TODO window (the scope of its grouping keys).
    pub(super) fn todo_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.todo.shown
            && self
                .todo
                .tool
                .as_ref()
                .is_some_and(|t| t.read(cx).focus().contains_focused(window, cx))
    }

    /// The scope tabs of the TODO window's header (behaviour 14).
    pub(super) fn todo_tabs(&self, focused: bool, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
        use gpui::prelude::*;
        let shown = self
            .todo
            .tool
            .as_ref()
            .map(|t| t.read(cx).scope())
            .unwrap_or_default();
        [
            ("todo-tab-project", Scope::Project),
            ("todo-tab-file", Scope::File),
        ]
        .into_iter()
        .map(|(id, scope)| {
            crate::metrics::header_tab(id, shown == scope, focused)
                .child(scope.title())
                .semantics(
                    Semantics::new(Role::Tab)
                        .name(scope.title())
                        .test_id(id)
                        .selected(shown == scope)
                        .action(AxAction::Click)
                        .effects(&[Effect::None]),
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.set_todo_scope(scope, cx)
                }))
                .into_any_element()
        })
        .collect()
    }
}
