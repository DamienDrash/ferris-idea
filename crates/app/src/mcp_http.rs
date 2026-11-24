//! The window's MCP server over HTTP (`SPEC-0221`): started with the window when `mcp.http` is on,
//! serving every open project, stopped when the setting goes off or the app quits. The window asks
//! the user for rights its clients lack (`SPEC-0053` v4 behaviours 7–9), quits for `quit_ide`
//! (`SPEC-0261` v2), reports its tooltips for `ui_tooltips` (`SPEC-0294` behaviour 10), serves
//! the UI driver (`SPEC-0319`), the editor tabs of `editor_tabs` (`SPEC-0059` v9), the project
//! tree of `project_tree` (`SPEC-0275` v10) and agents' edits of open files (`SPEC-0220` v2).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ferris_idea_mcp::http::HttpServer;
use ferris_idea_mcp::window::{
    DeclaredTooltip, PendingWork, PermissionAnswer, PermissionRequest, QuitMode, ShownTooltip,
    TooltipReport, Window,
};
use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::appearance::CurrentIdeSettings;
use gpui::{App, Global};
use tokio::sync::oneshot;

struct McpHttp {
    server: Option<HttpServer>,
    /// The settings folder: where the discovery file goes and the grant file is read and written
    /// (`SPEC-0053` behaviour 2) — the user's, or the self-test's and the window tests' own.
    discovery_dir: Option<PathBuf>,
    /// What the server's threads ask of the window.
    bridge: Arc<Bridge>,
}

impl Global for McpHttp {}

/// One thing the MCP server's threads ask of the window.
enum Request {
    Ask(PermissionRequest, oneshot::Sender<PermissionAnswer>),
    Pending(oneshot::Sender<PendingWork>),
    Quit(QuitMode, oneshot::Sender<Result<(), String>>),
    ClosePlan(
        PathBuf,
        String,
        oneshot::Sender<Result<ferris_idea_mcp::window::CloseReport, String>>,
    ),
    Close(Vec<String>, QuitMode, oneshot::Sender<Result<(), String>>),
    Tooltips(oneshot::Sender<TooltipReport>),
    ShowImport(PathBuf, oneshot::Sender<Result<String, String>>),
    PairRemote(String, String, oneshot::Sender<Result<String, String>>),
    Welcome(
        String,
        Option<PathBuf>,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    WelcomeSsh(
        String,
        String,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    OpenRemote(
        String,
        String,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    /// `room_join_link` (SPEC-0323 v7 behaviour 52); the link only in memory.
    JoinRoomLink(String, oneshot::Sender<Result<serde_json::Value, String>>),
    Ui(
        ferris_idea_mcp::window::UiCall,
        oneshot::Sender<ferris_idea_uidriver::Reply>,
    ),
    EditorTabs(
        PathBuf,
        serde_json::Value,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    ProjectTree(
        PathBuf,
        serde_json::Value,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    FileUndo(
        PathBuf,
        serde_json::Value,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    Selection(
        PathBuf,
        oneshot::Sender<Result<Option<ferris_idea_mcp::window::EditorSelection>, String>>,
    ),
    EditDocument(
        ferris_idea_editor::documents::DocumentEdit,
        oneshot::Sender<Result<ferris_idea_editor::documents::DocumentEdited, String>>,
    ),
    EditDocuments(ferris_idea_editor::documents::DocumentChange, EditsReply),
}

/// The answer to a change of several open files (`SPEC-0220` v3 behaviour 14).
type EditsReply =
    oneshot::Sender<Result<Vec<ferris_idea_editor::documents::DocumentEdited>, (PathBuf, String)>>;

/// The window as its MCP server sees it: requests wait in a queue the window polls.
#[derive(Default)]
struct Bridge {
    queue: Mutex<VecDeque<Request>>,
}

impl Bridge {
    fn push(&self, r: Request) {
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(r);
    }
}

impl Window for Bridge {
    fn ask(&self, request: PermissionRequest, reply: oneshot::Sender<PermissionAnswer>) {
        self.push(Request::Ask(request, reply));
    }

    fn pending_work(&self, reply: oneshot::Sender<PendingWork>) {
        self.push(Request::Pending(reply));
    }

    fn quit(&self, mode: QuitMode, reply: oneshot::Sender<Result<(), String>>) {
        self.push(Request::Quit(mode, reply));
    }

    fn tooltips(&self, reply: oneshot::Sender<TooltipReport>) {
        self.push(Request::Tooltips(reply));
    }

    fn close_plan(
        &self,
        workspace: PathBuf,
        scope: String,
        reply: oneshot::Sender<Result<ferris_idea_mcp::window::CloseReport, String>>,
    ) {
        self.push(Request::ClosePlan(workspace, scope, reply));
    }

    fn close_projects(
        &self,
        projects: Vec<String>,
        mode: QuitMode,
        reply: oneshot::Sender<Result<(), String>>,
    ) {
        self.push(Request::Close(projects, mode, reply));
    }

    fn show_import(&self, path: PathBuf, reply: oneshot::Sender<Result<String, String>>) {
        self.push(Request::ShowImport(path, reply));
    }

    fn open_remote_project(
        &self,
        host: String,
        folder: String,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        self.push(Request::OpenRemote(host, folder, reply));
    }

    fn join_room_by_link(
        &self,
        link: String,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        self.push(Request::JoinRoomLink(link, reply));
    }

    fn welcome_ssh(
        &self,
        host: String,
        folder: String,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        self.push(Request::WelcomeSsh(host, folder, reply));
    }

    fn pair_remote_host(
        &self,
        address: String,
        code: String,
        reply: oneshot::Sender<Result<String, String>>,
    ) {
        self.push(Request::PairRemote(address, code, reply));
    }

    fn welcome(
        &self,
        action: String,
        path: Option<PathBuf>,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        self.push(Request::Welcome(action, path, reply));
    }

    fn ui(
        &self,
        call: ferris_idea_mcp::window::UiCall,
        reply: oneshot::Sender<ferris_idea_uidriver::Reply>,
    ) {
        self.push(Request::Ui(call, reply));
    }

    fn editor_tabs(
        &self,
        workspace: PathBuf,
        request: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        self.push(Request::EditorTabs(workspace, request, reply));
    }

    fn project_tree(
        &self,
        workspace: PathBuf,
        request: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        self.push(Request::ProjectTree(workspace, request, reply));
    }

    fn file_operation_undo(
        &self,
        workspace: PathBuf,
        request: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        self.push(Request::FileUndo(workspace, request, reply));
    }

    fn editor_selection(
        &self,
        workspace: PathBuf,
        reply: oneshot::Sender<Result<Option<ferris_idea_mcp::window::EditorSelection>, String>>,
    ) {
        self.push(Request::Selection(workspace, reply));
    }

    fn edit_document(
        &self,
        edit: ferris_idea_editor::documents::DocumentEdit,
        reply: oneshot::Sender<Result<ferris_idea_editor::documents::DocumentEdited, String>>,
    ) {
        self.push(Request::EditDocument(edit, reply));
    }

    fn edit_documents(
        &self,
        change: ferris_idea_editor::documents::DocumentChange,
        reply: EditsReply,
    ) {
        self.push(Request::EditDocuments(change, reply));
    }
}

/// The tooltip shown and the tooltips the controls declare, read on the window's thread
/// (`SPEC-0294` behaviour 10).
pub fn tooltip_report() -> TooltipReport {
    use ferris_idea_ui::tooltip;
    let keymap = ferris_idea_workbench::keymaps::current();
    TooltipReport {
        shown: tooltip::shown().map(|s| ShownTooltip {
            text: s.text,
            x: s.bounds.x,
            y: s.bounds.y,
            width: s.bounds.w,
            height: s.bounds.h,
            pointer: [s.pointer.0, s.pointer.1],
            window: [s.window.0, s.window.1],
        }),
        controls: tooltip::declared(&keymap, ferris_idea_workbench::tips::macos())
            .into_iter()
            .map(|(name, text)| DeclaredTooltip { name, text })
            .collect(),
    }
}

/// Starts serving when the setting allows it and follows the setting and new project windows.
pub fn install(discovery_dir: Option<PathBuf>, cx: &mut App) {
    let bridge = Arc::new(Bridge::default());
    cx.set_global(McpHttp {
        server: None,
        discovery_dir,
        bridge: bridge.clone(),
    });
    cx.set_global(ferris_idea_workbench::project_windows::OnProjectOpened(
        std::rc::Rc::new(|root, cx| {
            let state = cx.global::<McpHttp>();
            if let Some(server) = &state.server {
                add(server, &state.bridge, state.discovery_dir.as_deref(), root);
            }
        }),
    ));
    // A closed project window is no longer served (SPEC-0094 v3 behaviour 8).
    cx.set_global(ferris_idea_workbench::project_windows::OnProjectClosed(
        std::rc::Rc::new(|root, cx| {
            if let Some(server) = &cx.global::<McpHttp>().server
                && let Err(e) = server.remove_workspace(root)
            {
                eprintln!("warning: MCP over HTTP: {}: {e}", root.display());
            }
        }),
    ));
    cx.observe_global::<CurrentIdeSettings>(sync).detach();
    cx.on_app_quit(|cx| {
        let server = cx.global_mut::<McpHttp>().server.take();
        async move { drop(server) }
    })
    .detach();
    serve_requests(bridge, cx);
    sync(cx);
}

/// Port and token of the running server (self-test).
pub fn running(cx: &App) -> Option<(u16, String)> {
    cx.try_global::<McpHttp>()?
        .server
        .as_ref()
        .map(|s| (s.port(), s.token().to_owned()))
}

/// Starts or stops the server to match the setting (behaviour 9).
fn sync(cx: &mut App) {
    let wanted = ferris_idea_workbench::appearance::ide_settings(cx).mcp.http;
    let running = cx.global::<McpHttp>().server.is_some();
    if wanted && !running {
        let dir = cx.global::<McpHttp>().discovery_dir.clone();
        match HttpServer::start(dir.as_deref()) {
            Ok(server) => {
                let bridge = cx.global::<McpHttp>().bridge.clone();
                for root in ferris_idea_workbench::project_windows::open_project_roots(cx) {
                    add(&server, &bridge, dir.as_deref(), &root);
                }
                cx.global_mut::<McpHttp>().server = Some(server);
            }
            Err(e) => eprintln!("warning: the MCP server over HTTP could not start: {e}"),
        }
    } else if !wanted && running {
        // Dropping stops the listener and removes the discovery file.
        cx.global_mut::<McpHttp>().server = None;
    }
}

/// Serves the project at `root` with the rights of the grant file in `settings`: in the app the
/// user's settings folder, so the file `IdeServer::new` reads.
fn add(server: &HttpServer, bridge: &Arc<Bridge>, settings: Option<&Path>, root: &Path) {
    match ferris_idea_core::Workspace::open(root) {
        Ok(ws) => {
            let window: Arc<dyn Window> = bridge.clone();
            let permissions = settings
                .map(|dir| {
                    ferris_idea_core::load_agent_permissions(
                        &dir.join(ferris_idea_core::GRANT_FILE),
                        root,
                    )
                })
                .unwrap_or_default();
            let mut ide = ferris_idea_mcp::IdeServer::with_permissions(ws, permissions)
                .with_window(window)
                .recording_sessions();
            // The user's switches (UI automation, SPEC-0319 behaviour 32) are read from the
            // same folder as the grants: the user's own, or the test's or self-test's.
            if let Some(dir) = settings {
                ide = ide.with_ide_settings_file(dir.join("settings.toml"));
            }
            if let Err(e) = server.add_workspace(ide) {
                eprintln!("warning: MCP over HTTP: {}: {e}", root.display());
            }
        }
        Err(e) => eprintln!("warning: MCP over HTTP: {}: {e}", root.display()),
    }
}

/// Serves the queue on the window's thread: questions one at a time, pending work, quitting.
fn serve_requests(bridge: Arc<Bridge>, cx: &mut App) {
    let asking = Arc::new(std::sync::atomic::AtomicBool::new(false));
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            loop {
                let next = {
                    let mut queue = bridge.queue.lock().unwrap_or_else(|e| e.into_inner());
                    let busy = asking.load(std::sync::atomic::Ordering::SeqCst);
                    next_request(&mut queue, busy)
                };
                let Some(request) = next else { break };
                let asking = asking.clone();
                let handled = cx.update(|cx| handle(request, asking, cx));
                if handled.is_err() {
                    return;
                }
            }
            // UI driver calls of a window that draws no frames of its own (minimized, covered)
            // advance here (SPEC-0319 v7 behaviour 55a).
            if cx.update(crate::ui_driver::pump_stalled).is_err() {
                return;
            }
        }
    })
    .detach();
}

/// The next request to serve: a question waits while another one is shown (`SPEC-0053` behaviour
/// 8: one at a time), and a question whose call ended while it waited is dropped, never shown
/// (v11).
fn next_request(queue: &mut VecDeque<Request>, busy: bool) -> Option<Request> {
    queue.retain(|r| !matches!(r, Request::Ask(_, reply) if reply.is_closed()));
    let pos = queue
        .iter()
        .position(|r| !busy || !matches!(r, Request::Ask(..)));
    pos.and_then(|i| queue.remove(i))
}

fn handle(request: Request, asking: Arc<std::sync::atomic::AtomicBool>, cx: &mut App) {
    match request {
        Request::Pending(reply) => {
            let (unsaved, running) = WorkbenchView::pending_work(cx);
            let _ = reply.send(PendingWork { unsaved, running });
        }
        Request::Quit(mode, reply) => {
            // The same step as ⌘Q (SPEC-0261 v3 behaviour 15).
            use ferris_idea_workbench::quit_model::QuitChoice;
            let choice = match mode {
                QuitMode::IfNothingPending => QuitChoice::IfNothingPending,
                QuitMode::SaveAll => QuitChoice::SaveAll,
                QuitMode::Discard => QuitChoice::Discard,
            };
            let result = WorkbenchView::prepare_quit(choice, cx);
            let quit = result.is_ok();
            let _ = reply.send(result);
            if quit {
                // The answer leaves first (SPEC-0261 v2 behaviour 7).
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(300))
                        .await;
                    let _ = cx.update(WorkbenchView::quit_decided);
                })
                .detach();
            }
        }
        // `welcome` (SPEC-0355 behaviour 22).
        Request::Welcome(action, path, reply) => {
            let _ = reply.send(welcome(&action, path.as_deref(), cx));
        }
        // `close_project` (SPEC-0094 v3 behaviour 10).
        Request::ClosePlan(workspace, scope, reply) => {
            let plan = WorkbenchView::close_plan_in(&workspace, &scope, cx).map(|p| {
                ferris_idea_mcp::window::CloseReport {
                    projects: p.projects.iter().map(|r| r.display().to_string()).collect(),
                    unsaved: p.unsaved,
                    running: p.running,
                }
            });
            let _ = reply.send(plan);
        }
        Request::Close(projects, mode, reply) => {
            use ferris_idea_workbench::quit_model::QuitChoice;
            let choice = match mode {
                QuitMode::IfNothingPending => QuitChoice::IfNothingPending,
                QuitMode::SaveAll => QuitChoice::SaveAll,
                QuitMode::Discard => QuitChoice::Discard,
            };
            let projects: Vec<PathBuf> = projects.into_iter().map(PathBuf::from).collect();
            let result = WorkbenchView::prepare_close(&projects, choice, cx);
            let close = result.is_ok();
            let _ = reply.send(result);
            if close {
                // The answer leaves first, then the windows (and maybe this server's route) go.
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(300))
                        .await;
                    let _ = cx.update(|cx| WorkbenchView::close_now(&projects, cx));
                })
                .detach();
            }
        }
        Request::Ask(request, reply) => ask(request, reply, asking, cx),
        Request::Tooltips(reply) => {
            let _ = reply.send(tooltip_report());
        }
        // `import_file` with `show` and `ferris-idea open <file>` (SPEC-0303 behaviour 17).
        Request::ShowImport(path, reply) => {
            let _ = reply.send(WorkbenchView::show_import_in_front(&path, cx));
        }
        // `remote_project_open` opens a project window on the remote project (SPEC-0317 v3
        // behaviour 20).
        // The UI driver (SPEC-0319): answered after the frames the call waits for.
        Request::Ui(call, reply) => crate::ui_driver::handle(call, reply, cx),
        // `editor_tabs` (SPEC-0059 v9 behaviour 21, SPEC-0112 v5 behaviour 11).
        Request::EditorTabs(workspace, request, reply) => {
            let _ = reply.send(WorkbenchView::editor_tabs_in(&workspace, &request, cx));
        }
        // `project_tree` (SPEC-0275 v10 behaviour 31).
        Request::ProjectTree(workspace, request, reply) => {
            let _ = reply.send(WorkbenchView::project_tree_in(&workspace, &request, cx));
        }
        // `file_operation_undo` (SPEC-0275 v16 behaviour 43).
        Request::FileUndo(workspace, request, reply) => {
            let _ = reply.send(WorkbenchView::file_undo_in(&workspace, &request, cx));
        }
        // `editor_selection` (SPEC-0006 v56 behaviour 11): read only.
        Request::Selection(workspace, reply) => {
            let _ = reply.send(WorkbenchView::editor_selection_in(&workspace, cx).map(|s| {
                s.map(|s| ferris_idea_mcp::window::EditorSelection {
                    path: s.path,
                    start_line: s.start_line,
                    start_character: s.start_character,
                    end_line: s.end_line,
                    end_character: s.end_character,
                    text: s.text,
                    carets: s.carets,
                })
            }));
        }
        // An agent's edit of an open file (SPEC-0220 v2 behaviour 7).
        Request::EditDocument(edit, reply) => {
            let _ = reply.send(WorkbenchView::edit_document_in(&edit, cx));
        }
        // A language server's or a refactoring's change of several open files, all or nothing
        // (SPEC-0220 v3 behaviour 14), kept as an undo group of the window (v4 behaviour 15).
        Request::EditDocuments(change, reply) => {
            let _ = reply.send(WorkbenchView::edit_documents_in(&change, cx));
        }
        Request::PairRemote(address, code, reply) => {
            WorkbenchView::pair_remote_host_in_front(
                address,
                code,
                move |paired, _| {
                    let _ = reply.send(paired);
                },
                cx,
            );
        }
        // `welcome` `open_ssh` (SPEC-0355 v3 behaviour 22): as a click in the SSH section.
        Request::WelcomeSsh(host, folder, reply) => {
            ferris_idea_workbench::welcome_ssh_view::open_ssh_project(
                &host,
                &folder,
                std::sync::Arc::default(),
                move |opened, _| {
                    let _ = reply.send(opened);
                },
                cx,
            );
        }
        Request::OpenRemote(host, folder, reply) => {
            WorkbenchView::open_remote_project_in_front(
                host,
                folder,
                move |opened, _| {
                    let _ = reply.send(opened);
                },
                cx,
            );
        }
        Request::JoinRoomLink(link, reply) => {
            WorkbenchView::join_room_by_link_in_front(
                link,
                move |joined, _| {
                    let _ = reply.send(joined);
                },
                cx,
            );
        }
    }
}

/// Shows the question of `SPEC-0053` v4 behaviour 8 in the project's window (else the window in
/// front); without a window the reply is dropped, so the call is refused as not granted. The
/// question lives as long as its call: when the call is cancelled, times out or loses its session,
/// the waiting side of `reply` goes away and the question is withdrawn without an answer (v11).
pub(crate) fn ask(
    request: PermissionRequest,
    reply: oneshot::Sender<PermissionAnswer>,
    asking: Arc<std::sync::atomic::AtomicBool>,
    cx: &mut App,
) {
    let target = ferris_idea_workbench::project_windows::project_window(&request.workspace, cx)
        .or_else(|| cx.active_window())
        .or_else(|| cx.windows().into_iter().next());
    let Some(target) = target else {
        return;
    };
    let (message, detail) = request.question();
    let answers = request.answers();
    let buttons = permission_buttons(&request);
    let shown = target.update(cx, |_, window, cx| {
        window.activate_window();
        window.prompt(
            gpui::PromptLevel::Warning,
            &message,
            Some(&detail),
            &buttons,
            cx,
        )
    });
    let Ok(chosen) = shown else {
        return;
    };
    asking.store(true, std::sync::atomic::Ordering::SeqCst);
    cx.spawn(async move |cx| {
        let mut reply = reply;
        match answer_or_call_ended(chosen, &mut reply).await {
            Some(chosen) => {
                // Closing the alert without a button counts as Deny.
                let answer = chosen
                    .ok()
                    .and_then(|i| answers.get(i).map(|(_, a)| *a))
                    .unwrap_or(PermissionAnswer::Deny);
                // An answer after the call ended reaches nobody and grants nothing.
                let _ = reply.send(answer);
            }
            None => {
                let _ = target.update(cx, |_, window, _| window.withdraw_prompt());
            }
        }
        asking.store(false, std::sync::atomic::Ordering::SeqCst);
    })
    .detach();
}

/// The button pressed (`Some`), or `None` once the call that asked has ended: the waiting side of
/// `reply` went away (cancelled, timed out, session ended).
async fn answer_or_call_ended<T>(
    mut chosen: T,
    reply: &mut oneshot::Sender<PermissionAnswer>,
) -> Option<T::Output>
where
    T: std::future::Future + Unpin,
{
    use std::task::Poll;
    let mut ended = std::pin::pin!(reply.closed());
    std::future::poll_fn(|task| {
        if let Poll::Ready(chosen) = std::pin::Pin::new(&mut chosen).poll(task) {
            return Poll::Ready(Some(chosen));
        }
        if ended.as_mut().poll(task).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

/// The question's buttons in the order of [`PermissionRequest::answers`]: the first, "Deny", is
/// the default (Return) and the question's named safe answer, which holds the keyboard focus, so
/// Space denies too (`SPEC-0053` v9 behaviour 8, `SPEC-0281` v3 behaviour 8). No answer is a
/// cancel button, so Escape gives none.
fn permission_buttons(request: &PermissionRequest) -> Vec<gpui::PromptButton> {
    request
        .answers()
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
            if i == 0 {
                gpui::PromptButton::safe(*label)
            } else {
                gpui::PromptButton::new(*label)
            }
        })
        .collect()
}

/// The window's part of the MCP tool `welcome` (`SPEC-0355` behaviour 22): show the Welcome
/// window, or open a project of the list as a click there does.
fn welcome(action: &str, path: Option<&Path>, cx: &mut App) -> Result<serde_json::Value, String> {
    match (action, path) {
        ("show", _) => {
            ferris_idea_workbench::welcome_view::show(cx);
            Ok(serde_json::json!({ "shown": true }))
        }
        ("open_recent", Some(path)) => {
            let new_window = ferris_idea_workbench::project_windows::open_project(path, cx)?;
            Ok(serde_json::json!({ "opened": path, "new_window": new_window }))
        }
        _ => Err(format!("the window does not do `{action}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_mcp::window::Scope;

    fn request(scope: Scope) -> PermissionRequest {
        PermissionRequest {
            client: "claude-code 2.1".into(),
            right: "write".into(),
            meaning: "create, change, move or delete files of the workspace".into(),
            action: "create_file {\"dir\":\".\",\"name\":\"a.txt\"}".into(),
            workspace: PathBuf::from("/work/shop"),
            scope,
        }
    }

    /// `SPEC-0053` v11 AK-15: a question whose call ended while it waited behind another one is
    /// dropped from the queue and never shown; the next question still waits while one is shown,
    /// and comes next once none is.
    #[test]
    fn a_question_whose_call_ended_is_never_shown() {
        let (ended, gone) = oneshot::channel();
        drop(gone);
        let (waiting, mut answer) = oneshot::channel();
        let (pending, _work) = oneshot::channel();
        let mut queue = VecDeque::from([
            Request::Ask(request(Scope::Workspace), ended),
            Request::Ask(request(Scope::Workspace), waiting),
            Request::Pending(pending),
        ]);
        // While a question is shown, other requests go on; the questions wait.
        assert!(matches!(
            next_request(&mut queue, true),
            Some(Request::Pending(_))
        ));
        assert_eq!(queue.len(), 1, "the ended question is dropped");
        assert!(next_request(&mut queue, true).is_none());
        let Some(Request::Ask(_, reply)) = next_request(&mut queue, false) else {
            panic!("the waiting question comes next");
        };
        reply.send(PermissionAnswer::AllowOnce).unwrap();
        assert_eq!(answer.try_recv().unwrap(), PermissionAnswer::AllowOnce);
        assert!(queue.is_empty());
    }

    /// `SPEC-0053` v11 AK-15: a question is answered by its button while its call waits; once the
    /// call has ended (the waiting side went away) it is withdrawn — no answer is taken, and one
    /// given afterwards reaches nobody.
    #[test]
    fn a_question_is_withdrawn_when_its_call_ends() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            // Answered while the call waits.
            let (mut reply, answer) = oneshot::channel::<PermissionAnswer>();
            let (button, chosen) = futures_oneshot();
            button.send(1).unwrap();
            assert_eq!(answer_or_call_ended(chosen, &mut reply).await, Some(Ok(1)));
            drop(answer);
            // The call ends while the question is shown: withdrawn, whatever is pressed later.
            let (mut reply, answer) = oneshot::channel::<PermissionAnswer>();
            let (button, chosen) = futures_oneshot();
            let ended = tokio::spawn(async move {
                tokio::task::yield_now().await;
                drop(answer);
            });
            assert_eq!(answer_or_call_ended(chosen, &mut reply).await, None);
            ended.await.unwrap();
            assert!(
                button.send(2).is_err(),
                "the withdrawn question takes no answer"
            );
            assert!(
                reply.send(PermissionAnswer::Always).is_err(),
                "an answer after the call ended reaches nobody"
            );
            // The question never shown because the call ended first.
            let (mut reply, answer) = oneshot::channel::<PermissionAnswer>();
            drop(answer);
            let (_button, chosen) = futures_oneshot();
            assert_eq!(answer_or_call_ended(chosen, &mut reply).await, None);
        });
    }

    /// The channel a gpui prompt answers on: the index of the button pressed.
    fn futures_oneshot() -> (
        tokio::sync::oneshot::Sender<usize>,
        tokio::sync::oneshot::Receiver<usize>,
    ) {
        tokio::sync::oneshot::channel()
    }

    /// `SPEC-0281` v3 AK-7, `SPEC-0053` v9 behaviour 8: Deny is the default, the named safe answer
    /// and the focused answer of the question, for a workspace right and a connection permission;
    /// the granting answers are none of these.
    #[test]
    fn deny_is_the_default_and_holds_the_keyboard_focus() {
        for scope in [Scope::Workspace, Scope::Connection("shop".into())] {
            let req = request(scope);
            let buttons = permission_buttons(&req);
            let labels: Vec<&str> = buttons.iter().map(|b| b.label().as_ref()).collect();
            assert_eq!(labels[0], "Deny");
            assert_eq!(labels.len(), 3);
            assert!(labels[2].starts_with("Always for This"));
            assert!(buttons[0].is_safe());
            assert!(
                buttons[1..]
                    .iter()
                    .all(|b| matches!(b, gpui::PromptButton::Other(_)))
            );
            let focused = gpui::prompt_focused_answer(&buttons).unwrap();
            assert_eq!(req.answers()[focused].1, PermissionAnswer::Deny);
        }
    }

    /// `SPEC-0281` v3 behaviour 8, AK-7: the focused answer is the answer the prompt names as safe,
    /// else its cancel answer wherever it stands, else its first answer (the default); a prompt
    /// without answers focuses none.
    #[test]
    fn the_safe_answer_else_the_cancel_answer_else_the_default_holds_the_keyboard_focus() {
        use gpui::PromptButton as B;
        let save = [B::ok("Save All"), B::new("Don't Save"), B::cancel("Cancel")];
        assert_eq!(gpui::prompt_focused_answer(&save), Some(2));
        let delete = [B::ok("Delete"), B::cancel("Cancel")];
        assert_eq!(gpui::prompt_focused_answer(&delete), Some(1));
        let guard = [B::cancel("Cancel"), B::new("Run")];
        assert_eq!(gpui::prompt_focused_answer(&guard), Some(0));
        // The trust question when the window opens names Preview in Safe Mode, also with Don't Open.
        let trust = [
            B::safe("Preview in Safe Mode"),
            B::new("Trust Projects in \"work\""),
            B::new("Trust Project"),
            B::cancel("Don't Open"),
        ];
        assert_eq!(gpui::prompt_focused_answer(&trust), Some(0));
        assert_eq!(gpui::prompt_focused_answer(&trust[..3]), Some(0));
        // Without a named safe answer the cancel answer wins over the default.
        let unnamed = [B::ok("Preview"), B::new("Trust"), B::cancel("Don't Open")];
        assert_eq!(gpui::prompt_focused_answer(&unnamed), Some(2));
        // A named safe answer that is not first still holds the focus.
        let later = [B::ok("Run"), B::safe("Keep"), B::cancel("Cancel")];
        assert_eq!(gpui::prompt_focused_answer(&later), Some(1));
        let about = [B::ok("OK"), B::new("Copy")];
        assert_eq!(gpui::prompt_focused_answer(&about), Some(0));
        assert_eq!(gpui::prompt_focused_answer(&[]), None);
    }
}
