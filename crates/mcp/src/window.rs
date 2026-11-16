//! What the running window does for its MCP clients, toolkit-free: asking the user for a right or a
//! database permission a client lacks (`SPEC-0053` v4 behaviours 7–9, `SPEC-0012` v2 behaviour 9) and
//! quitting (`SPEC-0261` v2). The window implements [`Window`]; the stdio server has none.

use std::path::PathBuf;

use serde::Serialize;
use tokio::sync::oneshot;

/// How long a call waits for the user's answer (`SPEC-0053` v4 behaviour 7).
pub const ASK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// How long an agent's edit of an open file waits for the window (`SPEC-0220` v2 behaviour 10).
pub const EDIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Longest action text shown in the question (behaviour 8).
pub const ACTION_CHARS: usize = 300;

/// What a question is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// An agent right of the workspace (`SPEC-0053`).
    Workspace,
    /// A database permission of the named connection (`SPEC-0012`).
    Connection(String),
}

/// One question for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    /// The client as it named itself at `initialize` (name and version).
    pub client: String,
    /// The right or permission, as written in the grant file or profile (`write`, `modify-data`).
    pub right: String,
    /// What it allows, in words.
    pub meaning: String,
    /// The tool and its arguments, at most [`ACTION_CHARS`] characters.
    pub action: String,
    /// The workspace the client works in.
    pub workspace: PathBuf,
    /// Workspace right or connection permission.
    pub scope: Scope,
}

/// The user's answer (behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionAnswer {
    /// Refuse this call; the session is not asked again for this right.
    Deny,
    /// Allow this call only.
    AllowOnce,
    /// Allow it and remember it for the workspace or connection.
    Always,
}

impl PermissionRequest {
    /// Headline and detail of the native alert.
    pub fn question(&self) -> (String, String) {
        let place = match &self.scope {
            Scope::Workspace => format!(
                "workspace \"{}\"",
                self.workspace.file_name().map_or_else(
                    || self.workspace.display().to_string(),
                    |n| { n.to_string_lossy().into_owned() }
                )
            ),
            Scope::Connection(name) => format!("connection \"{name}\""),
        };
        let message = format!(
            "Allow \"{}\" the right \"{}\" in {place}?",
            self.client, self.right
        );
        let scope = match &self.scope {
            Scope::Workspace => format!("workspace {}", self.workspace.display()),
            Scope::Connection(name) => format!(
                "connection \"{name}\" of workspace {}",
                self.workspace.display()
            ),
        };
        let detail = format!(
            "Client: {}\nRight: {} ({})\nAction: {}\nScope: {scope}",
            self.client, self.right, self.meaning, self.action
        );
        (message, detail)
    }

    /// Button labels with their answers, in the order the alert adds them; the first is the
    /// default (Return).
    pub fn answers(&self) -> [(&'static str, PermissionAnswer); 3] {
        let always = match self.scope {
            Scope::Workspace => "Always for This Workspace",
            Scope::Connection(_) => "Always for This Connection",
        };
        [
            ("Deny", PermissionAnswer::Deny),
            ("Allow Once", PermissionAnswer::AllowOnce),
            (always, PermissionAnswer::Always),
        ]
    }
}

/// The action text of a call: the tool and its arguments, cut to [`ACTION_CHARS`] characters.
pub fn action_text(
    tool: &str,
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
) -> String {
    let mut text = tool.to_owned();
    if let Some(args) = arguments.filter(|a| !a.is_empty()) {
        text.push(' ');
        text.push_str(&serde_json::Value::Object(args.clone()).to_string());
    }
    if text.chars().count() > ACTION_CHARS {
        text = text.chars().take(ACTION_CHARS - 1).collect();
        text.push('…');
    }
    text
}

/// What quitting would lose (`SPEC-0261` v2 behaviour 6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PendingWork {
    /// Unsaved files, named relative to their project.
    pub unsaved: Vec<String>,
    /// Running programs, debug sessions and statements, each with its project.
    pub running: Vec<String>,
}

impl PendingWork {
    /// Whether nothing would be lost.
    pub fn is_empty(&self) -> bool {
        self.unsaved.is_empty() && self.running.is_empty()
    }
}

/// What closing project windows would close and lose (`SPEC-0094` v3 behaviour 10).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CloseReport {
    /// The project folders whose windows close, the asking project's last.
    pub projects: Vec<String>,
    /// Their unsaved files, named relative to their project.
    pub unsaved: Vec<String>,
    /// Their running programs, debug sessions and statements.
    pub running: Vec<String>,
}

/// How the window is asked to quit (`SPEC-0261` v2 behaviours 7 and 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitMode {
    /// Quit only if nothing is pending now; else refuse naming it.
    IfNothingPending,
    /// Save every editor, then quit; refuse if a file stays unsaved.
    SaveAll,
    /// Quit without saving.
    Discard,
}

/// The running window, seen from its MCP server. Each method answers through `reply`, possibly
/// later from the window's own thread; a dropped `reply` means the window could not answer.
pub trait Window: Send + Sync {
    /// Asks the user (one question at a time).
    fn ask(&self, request: PermissionRequest, reply: oneshot::Sender<PermissionAnswer>);
    /// Reports the pending work of every project window.
    fn pending_work(&self, reply: oneshot::Sender<PendingWork>);
    /// Quits as `mode` says; the reply is sent before the window goes.
    fn quit(&self, mode: QuitMode, reply: oneshot::Sender<Result<(), String>>);
    /// What closing `scope` (`this`, `all`, `others`) would close, seen from the project window
    /// of `workspace` (`SPEC-0094` v3 behaviour 10). A window that cannot tell drops `reply`.
    fn close_plan(
        &self,
        workspace: PathBuf,
        scope: String,
        reply: oneshot::Sender<Result<CloseReport, String>>,
    ) {
        let _ = (workspace, scope);
        drop(reply);
    }
    /// Closes the windows of `projects` (from [`Window::close_plan`]) as `mode` says about their
    /// pending work; the reply is sent before the windows go. A window that cannot drops `reply`.
    fn close_projects(
        &self,
        projects: Vec<String>,
        mode: QuitMode,
        reply: oneshot::Sender<Result<(), String>>,
    ) {
        let _ = (projects, mode);
        drop(reply);
    }
    /// Reports the tooltip shown and the tooltips the controls declare (`SPEC-0294` behaviour
    /// 10). A window that cannot tell drops `reply`.
    fn tooltips(&self, reply: oneshot::Sender<TooltipReport>) {
        drop(reply);
    }
    /// Shows the import preview of the exchange file at `path` in the front project window
    /// (`SPEC-0303` behaviour 17); the user confirms there. Replies with the window's project, or
    /// why it could not. A window that cannot drops `reply`.
    fn show_import(&self, path: PathBuf, reply: oneshot::Sender<Result<String, String>>) {
        let _ = path;
        drop(reply);
    }
    /// Opens `folder` of `host` as a remote project in a project window of its own (`SPEC-0317`
    /// v3 behaviour 20) and replies with the project's state, or why it could not. A window that
    /// cannot drops `reply`; the server then keeps the session itself.
    fn open_remote_project(
        &self,
        host: String,
        folder: String,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        let _ = (host, folder);
        drop(reply);
    }
    /// Pairs with `address` using `code` (`SPEC-0321`): the window shows the digits and only the
    /// user confirms them. A window that cannot drops `reply`.
    fn pair_remote_host(
        &self,
        address: String,
        code: String,
        reply: oneshot::Sender<Result<String, String>>,
    ) {
        let _ = (address, code);
        drop(reply);
    }
    /// Serves a UI driver call (`SPEC-0319`); the reply comes when the call is done, after the
    /// frames it waited for. A window that cannot drops `reply`.
    fn ui(&self, call: UiCall, reply: oneshot::Sender<idecraft_uidriver::Reply>) {
        let _ = call;
        drop(reply);
    }
    /// Lists or tidies the editor tabs and groups of the project window of `workspace`
    /// (`SPEC-0059` v9 behaviour 21, `SPEC-0112` v5 behaviour 11): `request` holds `action`,
    /// `group`, `path` and `to_group` as the tool `editor_tabs` takes them; the reply is the
    /// groups after the action, or why it could not. A window that cannot drops `reply`.
    fn editor_tabs(
        &self,
        workspace: PathBuf,
        request: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        let _ = (workspace, request);
        drop(reply);
    }
    /// Reads or operates the project tree of the project window of `workspace` (`SPEC-0275` v10
    /// behaviour 31): `request` holds `action`, `path` and `on` as the tool `project_tree` takes
    /// them; the reply is the tree after the action, or why it could not. A window that cannot
    /// drops `reply`.
    fn project_tree(
        &self,
        workspace: PathBuf,
        request: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        let _ = (workspace, request);
        drop(reply);
    }
    /// Applies an agent's change of an open file to its editor as one undo step and saves it when
    /// `edit.save` says so (`SPEC-0220` v2 behaviours 7 and 8); the reply is the file's state after
    /// it, or why the window refused it (the revision moved on, a read-only editor). A window that
    /// cannot drops `reply`; the server then writes a saved file to disk and refuses an unsaved one
    /// (behaviour 10).
    fn edit_document(
        &self,
        edit: idecraft_editor::documents::DocumentEdit,
        reply: oneshot::Sender<Result<idecraft_editor::documents::DocumentEdited, String>>,
    ) {
        let _ = edit;
        drop(reply);
    }
    /// Applies the edits of one change of several open files all or nothing (`SPEC-0220` v3
    /// behaviour 14): checks every file first (revision, read-only, an editor that shows it), then
    /// applies each as one undo step of its editor and saves those whose `save` says so; with a
    /// name it keeps the change as an undo group of the window (`SPEC-0220` v4 behaviour 15). The
    /// reply is each file's state after it in the order of `change.edits`, or the first refused
    /// file and why; a refusal changed nothing. A window that cannot drops `reply`.
    fn edit_documents(
        &self,
        change: idecraft_editor::documents::DocumentChange,
        reply: oneshot::Sender<
            Result<Vec<idecraft_editor::documents::DocumentEdited>, (PathBuf, String)>,
        >,
    ) {
        let _ = change;
        drop(reply);
    }
    /// Lists, takes back or makes again the file operations of the Project tree of the project
    /// window of `workspace` (`SPEC-0275` v16 behaviour 43): `request` holds `action` as the tool
    /// `file_operation_undo` takes it; the reply is the history after the action, or why nothing
    /// was done. A window that cannot drops `reply`.
    fn file_operation_undo(
        &self,
        workspace: PathBuf,
        request: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    ) {
        let _ = (workspace, request);
        drop(reply);
    }
}

/// The actions of the tool `file_operation_undo` (`SPEC-0275` v16 behaviour 43).
pub const FILE_UNDO_ACTIONS: [&str; 3] = ["list", "undo", "redo"];

/// The actions of the tool `project_tree` (`SPEC-0275` v10 behaviour 31).
pub const PROJECT_TREE_ACTIONS: [&str; 7] = [
    "list",
    "expand",
    "collapse",
    "select",
    "compact",
    "expand_all",
    "collapse_all",
];

/// The actions of the tool `editor_tabs` (`SPEC-0059` v9 behaviour 21, `SPEC-0112` v5 behaviour
/// 11), with `open` of `SPEC-0094` v7 behaviour 21.
pub const EDITOR_TAB_ACTIONS: [&str; 13] = [
    "list",
    "open",
    "activate",
    "close",
    "close_others",
    "close_all",
    "pin",
    "unpin",
    "reopen_closed",
    "split_right",
    "split_down",
    "move_to_group",
    "unsplit_all",
];

/// One call of the UI driver for the window (`SPEC-0319` behaviour 29), already checked for the
/// setting and the right `ui` by the MCP server.
#[derive(Debug, Clone)]
pub struct UiCall {
    /// The workspace of the client's route: only its windows are reachable (behaviour 34).
    pub workspace: PathBuf,
    /// A window of that project by id (default: the project's window).
    pub window: Option<u64>,
    /// What to do.
    pub kind: UiCallKind,
}

/// What a [`UiCall`] asks.
#[derive(Debug, Clone)]
pub enum UiCallKind {
    /// A driver call: snapshot, find, act, expect.
    Driver(idecraft_uidriver::Request),
    /// `ui_driver_status`.
    Status,
    /// `ui_driver_stop` or `idecraft drive stop`.
    Stop(idecraft_uidriver::StopSource),
    /// `ui_trace start`.
    TraceStart,
    /// `ui_trace stop`, written below `dir` (the user's folder, or a folder in the workspace).
    TraceStop {
        /// Folder to write `ui-traces/<time>/` into; `None`: the user's folder.
        dir: Option<PathBuf>,
    },
    /// `ui_trace status`.
    TraceStatus,
    /// `ui_screenshot`.
    Screenshot,
}

/// The tooltip shown at the moment (`SPEC-0294` behaviour 10).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShownTooltip {
    /// Title, shortcut and detail lines.
    pub text: String,
    /// Left edge of the box in window coordinates.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Where the pointer was, `[x, y]`.
    pub pointer: [f32; 2],
    /// Size of the window's content, `[width, height]`.
    pub window: [f32; 2],
}

/// One control and the tooltip it declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclaredTooltip {
    /// The control's name (element id).
    pub name: String,
    /// Its tooltip text in the keymap in use.
    pub text: String,
}

/// What `ui_tooltips` answers (`SPEC-0294` behaviour 10).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TooltipReport {
    /// The tooltip shown now, if any.
    pub shown: Option<ShownTooltip>,
    /// Every control drawn with a tooltip, sorted by name.
    pub controls: Vec<DeclaredTooltip>,
}

impl TooltipReport {
    /// Only the controls whose name contains `part` (all without).
    pub fn filtered(mut self, part: Option<&str>) -> Self {
        if let Some(part) = part.filter(|p| !p.is_empty()) {
            self.controls.retain(|c| c.name.contains(part));
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(scope: Scope) -> PermissionRequest {
        PermissionRequest {
            client: "claude-code 2.1".into(),
            right: "write".into(),
            meaning: "create, change, move or delete files of the workspace".into(),
            action: "create_file {\"path\":\"a.txt\"}".into(),
            workspace: PathBuf::from("/work/shop"),
            scope,
        }
    }

    #[test]
    fn the_question_names_client_right_action_and_scope() {
        let (message, detail) = request(Scope::Workspace).question();
        assert_eq!(
            message,
            "Allow \"claude-code 2.1\" the right \"write\" in workspace \"shop\"?"
        );
        assert!(detail.contains("Client: claude-code 2.1"));
        assert!(detail.contains("Right: write (create, change"));
        assert!(detail.contains("Action: create_file {\"path\":\"a.txt\"}"));
        assert!(detail.contains("Scope: workspace /work/shop"));
        let labels: Vec<_> = request(Scope::Workspace)
            .answers()
            .iter()
            .map(|(l, _)| *l)
            .collect();
        assert_eq!(labels, ["Deny", "Allow Once", "Always for This Workspace"]);
        let db = request(Scope::Connection("shop-db".into()));
        assert!(db.question().0.ends_with("in connection \"shop-db\"?"));
        assert_eq!(db.answers()[2].0, "Always for This Connection");
        assert_eq!(db.answers()[0].1, PermissionAnswer::Deny);
    }

    #[test]
    fn action_text_is_cut() {
        let mut args = serde_json::Map::new();
        args.insert("text".into(), serde_json::json!("x".repeat(1000)));
        let text = action_text("create_file", Some(&args));
        assert_eq!(text.chars().count(), ACTION_CHARS);
        assert!(text.starts_with("create_file {\"text\":\"xx") && text.ends_with('…'));
        assert_eq!(action_text("quit_ide", None), "quit_ide");
    }
}
