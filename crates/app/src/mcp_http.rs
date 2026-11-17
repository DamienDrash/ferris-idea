//! The window's MCP server over HTTP (`SPEC-0221`): started with the window when `mcp.http` is on,
//! serving every open project, stopped when the setting goes off or the app quits. The window asks
//! the user for rights its clients lack (`SPEC-0053` v4 behaviours 7–9), quits for `quit_ide`
//! (`SPEC-0261` v2), reports its tooltips for `ui_tooltips` (`SPEC-0294` behaviour 10), serves
//! the UI driver (`SPEC-0319`), the editor tabs of `editor_tabs` (`SPEC-0059` v9), the project
//! tree of `project_tree` (`SPEC-0275` v10) and agents' edits of open files (`SPEC-0220` v2).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{App, Global};
use idecraft_mcp::http::HttpServer;
use idecraft_mcp::window::{
    DeclaredTooltip, PendingWork, PermissionAnswer, PermissionRequest, QuitMode, ShownTooltip,
    TooltipReport, Window,
};
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::appearance::CurrentIdeSettings;
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
        oneshot::Sender<Result<idecraft_mcp::window::CloseReport, String>>,
    ),
    Close(Vec<String>, QuitMode, oneshot::Sender<Result<(), String>>),
    Tooltips(oneshot::Sender<TooltipReport>),
    ShowImport(PathBuf, oneshot::Sender<Result<String, String>>),
    PairRemote(String, String, oneshot::Sender<Result<String, String>>),
    OpenRemote(
        String,
        String,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    Ui(
        idecraft_mcp::window::UiCall,
        oneshot::Sender<idecraft_uidriver::Reply>,
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
        oneshot::Sender<Result<Option<idecraft_mcp::window::EditorSelection>, String>>,
    ),
    EditDocument(
        idecraft_editor::documents::DocumentEdit,
        oneshot::Sender<Result<idecraft_editor::documents::DocumentEdited, String>>,
    ),
    EditDocuments(idecraft_editor::documents::DocumentChange, EditsReply),
}

/// The answer to a change of several open files (`SPEC-0220` v3 behaviour 14).
type EditsReply =
    oneshot::Sender<Result<Vec<idecraft_editor::documents::DocumentEdited>, (PathBuf, String)>>;

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
        reply: oneshot::Sender<Result<idecraft_mcp::window::CloseReport, String>>,
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

    fn pair_remote_host(
        &self,
        address: String,
        code: String,
        reply: oneshot::Sender<Result<String, String>>,
    ) {
        self.push(Request::PairRemote(address, code, reply));
    }

    fn ui(
        &self,
        call: idecraft_mcp::window::UiCall,
        reply: oneshot::Sender<idecraft_uidriver::Reply>,
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
        reply: oneshot::Sender<Result<Option<idecraft_mcp::window::EditorSelection>, String>>,
    ) {
        self.push(Request::Selection(workspace, reply));
    }

    fn edit_document(
        &self,
        edit: idecraft_editor::documents::DocumentEdit,
        reply: oneshot::Sender<Result<idecraft_editor::documents::DocumentEdited, String>>,
    ) {
        self.push(Request::EditDocument(edit, reply));
    }

    fn edit_documents(
        &self,
        change: idecraft_editor::documents::DocumentChange,
        reply: EditsReply,
    ) {
        self.push(Request::EditDocuments(change, reply));
    }
}

/// The tooltip shown and the tooltips the controls declare, read on the window's thread
/// (`SPEC-0294` behaviour 10).
pub fn tooltip_report() -> TooltipReport {
    use idecraft_ui::tooltip;
    let keymap = idecraft_workbench::keymaps::current();
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
        controls: tooltip::declared(&keymap, idecraft_workbench::tips::macos())
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
    cx.set_global(idecraft_workbench::project_windows::OnProjectOpened(
        std::rc::Rc::new(|root, cx| {
            let state = cx.global::<McpHttp>();
            if let Some(server) = &state.server {
                add(server, &state.bridge, state.discovery_dir.as_deref(), root);
            }
        }),
    ));
    // A closed project window is no longer served (SPEC-0094 v3 behaviour 8).
    cx.set_global(idecraft_workbench::project_windows::OnProjectClosed(
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
    let wanted = idecraft_workbench::appearance::ide_settings(cx).mcp.http;
    let running = cx.global::<McpHttp>().server.is_some();
    if wanted && !running {
        let dir = cx.global::<McpHttp>().discovery_dir.clone();
        match HttpServer::start(dir.as_deref()) {
            Ok(server) => {
                let bridge = cx.global::<McpHttp>().bridge.clone();
                for root in idecraft_workbench::project_windows::open_project_roots(cx) {
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
    match idecraft_core::Workspace::open(root) {
        Ok(ws) => {
            let window: Arc<dyn Window> = bridge.clone();
            let permissions = settings
                .map(|dir| {
                    idecraft_core::load_agent_permissions(
                        &dir.join(idecraft_core::GRANT_FILE),
                        root,
                    )
                })
                .unwrap_or_default();
            let ide =
                idecraft_mcp::IdeServer::with_permissions(ws, permissions).with_window(window);
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
                    // A question waits while another one is shown (behaviour 8: one at a time).
                    let busy = asking.load(std::sync::atomic::Ordering::SeqCst);
                    let pos = queue
                        .iter()
                        .position(|r| !busy || !matches!(r, Request::Ask(..)));
                    pos.and_then(|i| queue.remove(i))
                };
                let Some(request) = next else { break };
                let asking = asking.clone();
                let handled = cx.update(|cx| handle(request, asking, cx));
                if handled.is_err() {
                    return;
                }
            }
        }
    })
    .detach();
}

fn handle(request: Request, asking: Arc<std::sync::atomic::AtomicBool>, cx: &mut App) {
    match request {
        Request::Pending(reply) => {
            let (unsaved, running) = WorkbenchView::pending_work(cx);
            let _ = reply.send(PendingWork { unsaved, running });
        }
        Request::Quit(mode, reply) => {
            // The same step as ⌘Q (SPEC-0261 v3 behaviour 15).
            use idecraft_workbench::quit_model::QuitChoice;
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
        // `close_project` (SPEC-0094 v3 behaviour 10).
        Request::ClosePlan(workspace, scope, reply) => {
            let plan = WorkbenchView::close_plan_in(&workspace, &scope, cx).map(|p| {
                idecraft_mcp::window::CloseReport {
                    projects: p.projects.iter().map(|r| r.display().to_string()).collect(),
                    unsaved: p.unsaved,
                    running: p.running,
                }
            });
            let _ = reply.send(plan);
        }
        Request::Close(projects, mode, reply) => {
            use idecraft_workbench::quit_model::QuitChoice;
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
        // `import_file` with `show` and `idecraft open <file>` (SPEC-0303 behaviour 17).
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
                s.map(|s| idecraft_mcp::window::EditorSelection {
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
    }
}

/// Shows the question of `SPEC-0053` v4 behaviour 8 in the project's window (else the window in
/// front); without a window the reply is dropped, so the call is refused as not granted.
pub(crate) fn ask(
    request: PermissionRequest,
    reply: oneshot::Sender<PermissionAnswer>,
    asking: Arc<std::sync::atomic::AtomicBool>,
    cx: &mut App,
) {
    let target = idecraft_workbench::project_windows::project_window(&request.workspace, cx)
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
    cx.spawn(async move |_| {
        // Closing the alert without a button counts as Deny.
        let answer = chosen
            .await
            .ok()
            .and_then(|i| answers.get(i).map(|(_, a)| *a))
            .unwrap_or(PermissionAnswer::Deny);
        let _ = reply.send(answer);
        asking.store(false, std::sync::atomic::Ordering::SeqCst);
    })
    .detach();
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

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_mcp::window::Scope;

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
