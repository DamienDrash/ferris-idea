//! The app's side of the Claude Code link (`SPEC-0358`): lets the window start links, hands the
//! links' questions to the window's thread, follows the open project windows, pushes the selection
//! to connected clients and shows the connection in the status bar.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_claude_link::{
    Diagnostic, DiffDecision, DiffProposal, Editor, HubConfig, OpenEditor, OpenRequest, Selection,
};
use ferris_idea_workbench::WorkbenchView;
use gpui::App;

/// One thing a link's thread asks of the window.
enum Request {
    Selection(PathBuf, Sender<Option<Selection>>),
    Editors(PathBuf, Sender<Vec<OpenEditor>>),
    Diagnostics(PathBuf, Option<PathBuf>, Sender<Vec<Diagnostic>>),
    Open(PathBuf, OpenRequest, Sender<Result<(), String>>),
    ShowDiff(PathBuf, DiffProposal, Sender<DiffDecision>),
    CloseDiff(PathBuf, String),
}

/// The window as the links see it: requests wait in a queue the window polls.
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

    fn next(&self) -> Option<Request> {
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }
}

impl Editor for Bridge {
    fn selection(&self, workspace: PathBuf, reply: Sender<Option<Selection>>) {
        self.push(Request::Selection(workspace, reply));
    }
    fn open_editors(&self, workspace: PathBuf, reply: Sender<Vec<OpenEditor>>) {
        self.push(Request::Editors(workspace, reply));
    }
    fn diagnostics(
        &self,
        workspace: PathBuf,
        file: Option<PathBuf>,
        reply: Sender<Vec<Diagnostic>>,
    ) {
        self.push(Request::Diagnostics(workspace, file, reply));
    }
    fn open_file(
        &self,
        workspace: PathBuf,
        request: OpenRequest,
        reply: Sender<Result<(), String>>,
    ) {
        self.push(Request::Open(workspace, request, reply));
    }
    fn show_diff(&self, workspace: PathBuf, proposal: DiffProposal, reply: Sender<DiffDecision>) {
        self.push(Request::ShowDiff(workspace, proposal, reply));
    }
    fn close_diff(&self, workspace: PathBuf, tab_name: String) {
        self.push(Request::CloseDiff(workspace, tab_name));
    }
}

/// The lock folder (`SPEC-0358` behaviour 3): `FERRIS_IDEA_CLAUDE_IDE_DIR` when set; the self-test's
/// own temporary folder; with a settings folder of its own (`FERRIS_IDEA_CONFIG_DIR`, as every
/// window test sets it) a folder inside it, so tests never write the user's `~/.claude`; else the
/// folder Claude Code reads.
fn lock_dir(selftest: bool) -> Option<PathBuf> {
    if let Some(dir) = ferris_idea_core::takeover::product_var_os("FERRIS_IDEA_CLAUDE_IDE_DIR") {
        return Some(PathBuf::from(dir));
    }
    if selftest {
        return Some(std::env::temp_dir().join(format!(
            "ferris-idea-selftest-claude-ide-{}",
            std::process::id()
        )));
    }
    if let Some(config) = ferris_idea_core::takeover::product_var_os("FERRIS_IDEA_CONFIG_DIR") {
        return Some(PathBuf::from(config).join("claude-ide"));
    }
    ferris_idea_claude_link::lockfile::default_lock_dir()
}

/// Lets the window's terminals start links and serves them (`SPEC-0358` behaviours 1–23).
pub fn install(selftest: bool, cx: &mut App) {
    let Some(dir) = lock_dir(selftest) else {
        return;
    };
    let bridge = Arc::new(Bridge::default());
    let editor: Arc<dyn Editor> = bridge.clone();
    ferris_idea_claude_link::install(
        HubConfig {
            lock_dir: dir,
            ide_version: ferris_idea_core::release::VERSION.into(),
        },
        editor,
    );
    cx.on_app_quit(|_| async move { ferris_idea_claude_link::shutdown() })
        .detach();
    cx.spawn(async move |cx| {
        let mut ticks = 0u32;
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            while let Some(request) = bridge.next() {
                if cx.update(|cx| handle(request, cx)).is_err() {
                    return;
                }
            }
            ticks = ticks.wrapping_add(1);
            if ticks.is_multiple_of(4) && cx.update(follow).is_err() {
                return;
            }
        }
    })
    .detach();
}

/// Every 200 ms: the open projects, the status bar, diffs whose viewer went away and the
/// selection of linked projects.
fn follow(cx: &mut App) {
    let roots = ferris_idea_workbench::project_windows::open_project_roots(cx);
    ferris_idea_claude_link::set_projects(roots.clone());
    WorkbenchView::claude_diffs_tick(cx);
    let connected: HashSet<PathBuf> = roots
        .iter()
        .filter(|r| ferris_idea_claude_link::connected(r))
        .cloned()
        .collect();
    for root in &connected {
        let selection = WorkbenchView::claude_selection_in(root, cx);
        ferris_idea_claude_link::notify_selection(root, selection);
    }
    if WorkbenchView::set_claude_connected(connected, cx) {
        cx.refresh_windows();
    }
}

fn handle(request: Request, cx: &mut App) {
    match request {
        Request::Selection(root, reply) => {
            let _ = reply.send(WorkbenchView::claude_selection_in(&root, cx));
        }
        Request::Editors(root, reply) => {
            let _ = reply.send(WorkbenchView::claude_open_editors_in(&root, cx));
        }
        Request::Diagnostics(root, file, reply) => {
            let _ = reply.send(WorkbenchView::claude_diagnostics_in(
                &root,
                file.as_deref(),
                cx,
            ));
        }
        Request::Open(root, request, reply) => {
            let _ = reply.send(WorkbenchView::claude_open_file_in(&root, &request, cx));
        }
        Request::ShowDiff(root, proposal, reply) => {
            WorkbenchView::claude_show_diff_in(&root, proposal, reply, cx);
        }
        Request::CloseDiff(root, tab) => WorkbenchView::claude_close_diff_in(&root, &tab, cx),
    }
}
