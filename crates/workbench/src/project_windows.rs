//! Project windows (`SPEC-0094`): one window per project folder, the list of recent projects.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Global, WindowBounds, WindowOptions, px, size,
};

use crate::WorkbenchView;

/// The recent projects file in use; the self-test points it into its own folder.
pub struct RecentProjectsFile(pub PathBuf);

impl Global for RecentProjectsFile {}

/// Set by window tests in the test harness: a decided quit (`SPEC-0261` behaviour 15) is counted
/// here instead of ending the process, which is the test runner. The test platform's own quit does
/// nothing, so the count is what such a test sees of quitting.
#[derive(Debug, Clone, Default)]
pub struct QuitsInHarness(pub std::rc::Rc<std::cell::Cell<usize>>);

impl Global for QuitsInHarness {}

/// Windows by project folder, to bring an open project to the front instead of opening it twice.
#[derive(Default)]
struct OpenProjects(HashMap<PathBuf, AnyWindowHandle>);

impl Global for OpenProjects {}

/// The recent projects file: the one set as [`RecentProjectsFile`], else the user's.
pub fn recent_file(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<RecentProjectsFile>() {
        Some(f) => Some(f.0.clone()),
        None => idecraft_core::recent_projects_file(),
    }
}

/// Recent projects, most recent first, without `except`.
pub fn recent_projects(except: &Path, cx: &App) -> Vec<PathBuf> {
    recent_file(cx)
        .map(|f| idecraft_core::load_recent_projects(&f))
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p != except)
        .collect()
}

/// Notes that `window` shows `root` and puts `root` at the front of the recent projects
/// (behaviour 3).
pub fn register_window(root: &Path, window: AnyWindowHandle, cx: &mut App) {
    if let Some(file) = recent_file(cx) {
        let _ = idecraft_core::remember_project(&file, root);
    }
    cx.default_global::<OpenProjects>()
        .0
        .insert(root.to_path_buf(), window);
    note_opened(&mut cx.default_global::<OpenedOrder>().0, window);
    if let Some(hook) = cx.try_global::<OnProjectOpened>().map(|h| h.0.clone()) {
        hook(root, cx);
    }
}

/// The project windows in the order they were opened, for Next and Previous Project Window
/// (`SPEC-0094` v2 behaviour 5).
#[derive(Default)]
struct OpenedOrder(Vec<AnyWindowHandle>);

impl Global for OpenedOrder {}

/// Puts `window` at the end of `order` unless it is in it: a project that is opened again in its
/// window keeps the window's place.
fn note_opened<T: PartialEq>(order: &mut Vec<T>, window: T) {
    if !order.contains(&window) {
        order.push(window);
    }
}

/// The project windows still open, in the order they were opened.
pub fn opened_windows(cx: &mut App) -> Vec<AnyWindowHandle> {
    let open = cx.windows();
    let order = &mut cx.default_global::<OpenedOrder>().0;
    order.retain(|w| open.contains(w));
    order.clone()
}

/// Every workbench of this process, whatever view a window has at its root (the first window
/// wraps it in the self-test's view): what quitting looks at (`SPEC-0261` v3 behaviour 14).
#[derive(Default)]
struct Workbenches(Vec<gpui::WeakEntity<WorkbenchView>>);

impl Global for Workbenches {}

/// Notes a new workbench; it leaves the list when it is dropped with its window.
pub fn register_workbench(bench: gpui::WeakEntity<WorkbenchView>, cx: &mut App) {
    let list = &mut cx.default_global::<Workbenches>().0;
    list.retain(|w| w.upgrade().is_some());
    list.push(bench);
}

/// The workbenches still open, oldest first.
pub fn workbenches(cx: &App) -> Vec<gpui::Entity<WorkbenchView>> {
    cx.try_global::<Workbenches>()
        .map(|w| w.0.iter().filter_map(|w| w.upgrade()).collect())
        .unwrap_or_default()
}

/// Called with the root of every project window that opens, e.g. so the MCP server serves it
/// (`SPEC-0221` behaviour 1).
pub struct OnProjectOpened(pub std::rc::Rc<ProjectHook>);

/// What [`OnProjectOpened`] calls.
pub type ProjectHook = dyn Fn(&Path, &mut App);

impl Global for OnProjectOpened {}

/// Called with the root of every project window closed by Close Project and its kind, e.g. so the
/// MCP server stops serving it (`SPEC-0094` v3 behaviour 8).
pub struct OnProjectClosed(pub std::rc::Rc<ProjectHook>);

impl Global for OnProjectClosed {}

/// Called when Close Project closed the last project window: the app offers the folder chooser
/// (`SPEC-0094` v3 behaviour 9).
pub struct OnLastProjectClosed(pub std::rc::Rc<dyn Fn(&mut App)>);

impl Global for OnLastProjectClosed {}

/// Notes that the window of `root` closes: it is no longer an open project, the project moves to
/// the front of the recent projects, and [`OnProjectClosed`] is told (`SPEC-0094` v3
/// behaviour 8). Returns the window it had.
pub fn unregister_window(root: &Path, cx: &mut App) -> Option<AnyWindowHandle> {
    let window = project_window(root, cx);
    if let Some(window) = window {
        cx.default_global::<OpenProjects>()
            .0
            .retain(|_, w| *w != window);
        cx.default_global::<OpenedOrder>()
            .0
            .retain(|w| *w != window);
    }
    if let Some(file) = recent_file(cx) {
        let _ = idecraft_core::remember_project(&file, root);
    }
    if let Some(hook) = cx.try_global::<OnProjectClosed>().map(|h| h.0.clone()) {
        hook(root, cx);
    }
    window
}

/// The close button of `bench`'s local project `window` closes as Close Project (`SPEC-0094` v4
/// behaviour 11): asks about unsaved files and running work, stops the runs, offers the folder
/// chooser after the last window. Until v3 the window just went.
pub fn close_button_closes_the_project(
    bench: &gpui::Entity<WorkbenchView>,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let weak = bench.downgrade();
    window.on_window_should_close(cx, move |_, cx| {
        weak.update(cx, |b, cx| b.close_button(cx)).unwrap_or(true)
    });
}

/// The window of the project at `root`, if it is open.
pub fn project_window(root: &Path, cx: &App) -> Option<AnyWindowHandle> {
    let o = cx.try_global::<OpenProjects>()?;
    o.0.get(root).copied().or_else(|| {
        let wanted = std::fs::canonicalize(root).ok()?;
        o.0.iter()
            .find(|(r, _)| std::fs::canonicalize(r).ok().as_ref() == Some(&wanted))
            .map(|(_, w)| *w)
    })
}

/// Roots of the open project windows.
pub fn open_project_roots(cx: &App) -> Vec<PathBuf> {
    // A window closed by its close button stays in the map; it is not an open project.
    let windows = cx.windows();
    cx.try_global::<OpenProjects>()
        .map(|o| {
            o.0.iter()
                .filter(|(_, w)| windows.contains(w))
                .map(|(r, _)| r.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Window options of a workbench window: the main toolbar shares the title bar (SPEC-0061).
pub fn window_options(title: &str, cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(1300.0), px(900.0)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(title.to_owned().into()),
            appears_transparent: true,
            // SPEC-0061 v7 behaviour 44: the close button here, the others 20 pt apart
            // (`traffic_lights::space`).
            traffic_light_position: Some(gpui::point(
                px(crate::traffic_lights::TRAFFIC_LIGHT_POSITION[0]),
                px(crate::traffic_lights::TRAFFIC_LIGHT_POSITION[1]),
            )),
        }),
        // Linux desktops find the icon through `idecraft.desktop` (SPEC-0060 v6 behaviour 12).
        app_id: Some(idecraft_ui::brand::APP_ID.to_owned()),
        ..Default::default()
    }
}

/// Opens `path` in a window of its own, or brings its window to the front when it is open
/// (behaviour 2). Returns whether a new window was opened. A file is never a project root: it
/// opens its project (`SPEC-0094` v7 behaviour 18) and itself in a focused editor tab, in the
/// project's window when one is open (behaviour 19).
pub fn open_project(path: &Path, cx: &mut App) -> Result<bool, String> {
    let target = idecraft_core::project_root::open_target(path).map_err(|e| e.to_string())?;
    let (root, file) = (target.root, target.file);
    let open = cx
        .try_global::<OpenProjects>()
        .and_then(|o| o.0.get(&root).copied());
    if let Some(window) = open
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        if let Some(file) = file {
            // Deferred: the caller may be updating a workbench of that very window (behaviour 16).
            cx.defer(move |cx| {
                let _ = open_file_in(&root, &file, cx);
            });
        }
        return Ok(false);
    }
    let title = format!("IDECraft — {}", root.display());
    let options = window_options(&title, cx);
    let shown = root.clone();
    let handle = cx
        .open_window(options, move |window, cx| {
            crate::x11_wake::until_first_frame(window);
            window.set_window_title(&title);
            let bench = cx.new(|cx| WorkbenchView::new(&shown, cx));
            close_button_closes_the_project(&bench, window, cx);
            bench.update(cx, |b, cx| b.watch_window(window, cx));
            window.focus(&bench.read(cx).start_focus(cx));
            crate::appearance::watch_os_appearance(window);
            // The file the path named, at once (SPEC-0094 v7 behaviour 19).
            if let Some(file) = &file {
                bench.update(cx, |b, cx| b.open(file, window, cx));
            }
            // An untrusted project asks once its window is there (SPEC-0278 behaviour 1).
            let asking = bench.clone();
            window.defer(cx, move |window, cx| {
                asking.update(cx, |b, cx| b.ask_trust(true, window, cx));
            });
            bench
        })
        .map_err(|e| e.to_string())?;
    register_window(&root, handle.into(), cx);
    Ok(true)
}

/// Opens `file` in a focused editor tab of the open window of the project at `root` and brings
/// that window to the front (`SPEC-0094` v7 behaviour 19). Errors when no window shows `root`.
pub fn open_file_in(root: &Path, file: &Path, cx: &mut App) -> Result<(), String> {
    let wanted = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let bench = workbenches(cx)
        .into_iter()
        .find(|b| {
            let r = b.read(cx).project_root().to_path_buf();
            r.canonicalize().unwrap_or(r) == wanted
        })
        .ok_or("no project window is open for this project")?;
    let handle = project_window(root, cx).ok_or("the project window is not registered")?;
    handle
        .update(cx, |_, window, cx| {
            window.activate_window();
            bench.update(cx, |b, cx| b.open(file, window, cx));
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::note_opened;

    /// SPEC-0094 AK-4 (v2): windows are kept in opening order, each once.
    #[test]
    fn the_order_keeps_each_window_once() {
        let mut order = Vec::new();
        for window in [3, 1, 3, 2, 1] {
            note_opened(&mut order, window);
        }
        assert_eq!(order, [3, 1, 2]);
    }
}
