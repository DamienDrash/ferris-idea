//! What the window does for the link (`SPEC-0358` behaviours 12–22), toolkit-free: the window
//! implements [`Editor`]; the link's threads call it and wait for the answer on the `reply` channel.
//! A dropped `reply` means the window could not answer.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// A place in a file: 0-based line and 0-based character of that line (behaviour 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// 0-based line.
    pub line: usize,
    /// 0-based character (Unicode scalar value) of the line.
    pub character: usize,
}

/// The selection of the active editor; start and end are the same when nothing is selected
/// (behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The editor's file, absolute.
    pub path: PathBuf,
    /// Where the selection starts.
    pub start: Position,
    /// Where it ends (exclusive); the caret when nothing is selected.
    pub end: Position,
    /// The selected text.
    pub text: String,
}

/// One open editor tab (behaviour 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenEditor {
    /// The file, absolute.
    pub path: PathBuf,
    /// The tab in front.
    pub active: bool,
    /// The editor holds unsaved changes.
    pub dirty: bool,
}

/// How serious a diagnostic is (behaviour 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// Information.
    Information,
    /// A hint, such as unused code.
    Hint,
}

impl Severity {
    /// The word the protocol uses.
    pub fn word(self) -> &'static str {
        match self {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
            Severity::Information => "Information",
            Severity::Hint => "Hint",
        }
    }
}

/// One diagnostic of a file (behaviour 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file, absolute.
    pub path: PathBuf,
    /// Where it starts.
    pub start: Position,
    /// Where it ends.
    pub end: Position,
    /// How serious it is.
    pub severity: Severity,
    /// The message.
    pub message: String,
    /// Who found it (a language server, an inspection), when known.
    pub source: Option<String>,
}

/// What `openFile` asks the window to do (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenRequest {
    /// The file, absolute and inside the workspace (checked by the link).
    pub path: PathBuf,
    /// Select from the first occurrence of this text.
    pub start_text: Option<String>,
    /// ...up to the end of the first occurrence of this text after it.
    pub end_text: Option<String>,
    /// Extend the selection to the end of the line `end_text` ends on.
    pub select_to_end_of_line: bool,
    /// 1-based first line to select, when no text is given.
    pub start_line: Option<usize>,
    /// 1-based last line to select.
    pub end_line: Option<usize>,
    /// Bring the editor to the front (default); otherwise open it without taking the focus.
    pub focus: bool,
}

/// A change Claude Code proposes for one file (behaviour 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffProposal {
    /// The name Claude Code gave the diff; it closes it by this name.
    pub tab_name: String,
    /// The file, absolute and inside the workspace (checked by the link).
    pub path: PathBuf,
    /// The file's text now (empty for a new file).
    pub old_text: String,
    /// The proposed text.
    pub new_text: String,
    /// The file does not exist yet.
    pub new_file: bool,
}

/// The user's answer to a diff (behaviour 18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffDecision {
    /// Accepted, with the text as accepted.
    Accepted(String),
    /// Rejected, or the diff was closed without an answer.
    Rejected,
}

/// The project window as the link sees it. Every method answers through `reply`, possibly later
/// from the window's own thread.
pub trait Editor: Send + Sync {
    /// The selection of the active editor of `workspace`'s window, `None` without an editor.
    fn selection(&self, workspace: PathBuf, reply: Sender<Option<Selection>>);
    /// The open editor tabs of the window.
    fn open_editors(&self, workspace: PathBuf, reply: Sender<Vec<OpenEditor>>);
    /// The diagnostics of `file`, or of every open file when `None`.
    fn diagnostics(
        &self,
        workspace: PathBuf,
        file: Option<PathBuf>,
        reply: Sender<Vec<Diagnostic>>,
    );
    /// Opens a file of the workspace and selects what `request` names.
    fn open_file(
        &self,
        workspace: PathBuf,
        request: OpenRequest,
        reply: Sender<Result<(), String>>,
    );
    /// Shows `proposal` as a diff the user accepts or rejects; answers once with the decision.
    /// A diff that is replaced or closed without an answer answers [`DiffDecision::Rejected`].
    fn show_diff(&self, workspace: PathBuf, proposal: DiffProposal, reply: Sender<DiffDecision>);
    /// Closes the diff `tab_name` if it is still shown (its call was answered elsewhere).
    fn close_diff(&self, workspace: PathBuf, tab_name: String);
    /// A client connected or left: the window may show it (behaviour 23).
    fn link_changed(&self, workspace: PathBuf) {
        let _ = workspace;
    }
}
