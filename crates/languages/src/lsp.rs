//! Language Server Protocol client (`SPEC-0023`).
//!
//! JSON-RPC over stdio with `Content-Length` framing. A reader task delivers
//! responses to waiting requests, queues notifications and answers requests
//! from the server; when the server stops, every waiting request fails with
//! the last lines it wrote to stderr.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::LanguageId;

/// LSP errors that mean "ask again": `ContentModified` and `ServerCancelled` (`SPEC-0090` v2).
const RETRIED_CODES: [i64; 2] = [-32801, -32802];
/// Attempts in all for a request rejected with one of [`RETRIED_CODES`].
const RETRY_ATTEMPTS: u32 = 5;

/// What went wrong talking to a language server.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LspError {
    /// The server process could not be started.
    #[error("could not start language server `{command}`: {message}")]
    Start {
        /// Command that was run.
        command: String,
        /// Operating-system error.
        message: String,
    },
    /// No server is known for the language.
    #[error("no usable language server for {0}")]
    NoServer(String),
    /// Reading or writing failed.
    #[error("i/o error: {0}")]
    Io(String),
    /// The server sent something that is not LSP.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The server answered with an error.
    #[error("{method}: server error {code}: {message}")]
    Response {
        /// Request method.
        method: String,
        /// JSON-RPC error code.
        code: i64,
        /// Error message.
        message: String,
    },
    /// The server does not know the request: it lacks the feature (`SPEC-0023` v3 behaviour 11).
    #[error("{server} does not offer {} ({method})", feature_name(.method))]
    Unsupported {
        /// The server's program name.
        server: String,
        /// Request method.
        method: String,
    },
    /// No answer in time.
    #[error("{0}: no answer within {1} s")]
    Timeout(String, u64),
    /// The caller stopped waiting (`SPEC-0293` behaviour 2).
    #[error("{0}: stopped")]
    Stopped(String),
    /// What was asked for cannot be done here, in words the user can act on (`SPEC-0043` v4
    /// behaviour 8: no formatter for the file, a missing tool of the server).
    #[error("{0}")]
    Refused(String),
    /// The server stopped; carries its last stderr lines.
    #[error("the language server stopped{}", if .0.is_empty() { String::new() } else { format!(": {}", .0) })]
    Closed(String),
}

/// JSON-RPC's `MethodNotFound`.
const METHOD_NOT_FOUND: i64 = -32601;

/// What a request method is called in a message to the user (`SPEC-0023` v3 behaviour 11).
fn feature_name(method: &str) -> &'static str {
    match method {
        "textDocument/codeAction" | "codeAction/resolve" => "code actions",
        "textDocument/prepareCallHierarchy"
        | "callHierarchy/incomingCalls"
        | "callHierarchy/outgoingCalls" => "a call hierarchy",
        "textDocument/prepareTypeHierarchy"
        | "typeHierarchy/supertypes"
        | "typeHierarchy/subtypes" => "a type hierarchy",
        "textDocument/rename" | "textDocument/prepareRename" => "renaming",
        "textDocument/references" => "usages",
        "textDocument/documentHighlight" => "usage highlights",
        "textDocument/definition" => "declarations",
        "textDocument/typeDefinition" => "type declarations",
        "textDocument/implementation" => "implementations",
        "textDocument/hover" => "quick documentation",
        "textDocument/completion" | "completionItem/resolve" => "completion",
        "textDocument/signatureHelp" => "parameter information",
        "textDocument/documentSymbol" => "a file structure",
        "textDocument/inlayHint" => "inlay hints",
        "textDocument/semanticTokens/full" => "semantic tokens",
        "textDocument/formatting" | "textDocument/rangeFormatting" => "formatting",
        "textDocument/diagnostic" => "diagnostics on request",
        _ => "this request",
    }
}

/// The error for a server's error answer to `method`.
fn response_error(server: &str, method: &str, code: i64, message: String) -> LspError {
    if code == METHOD_NOT_FOUND {
        LspError::Unsupported {
            server: server.to_owned(),
            method: method.to_owned(),
        }
    } else {
        LspError::Response {
            method: method.to_owned(),
            code,
            message,
        }
    }
}

/// A notification sent by the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// Method, e.g. `textDocument/publishDiagnostics`.
    pub method: String,
    /// Parameters.
    pub params: Value,
}

/// One diagnostic, positions 0-based as in LSP.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Diagnostic {
    /// `error`, `warning`, `information`, `hint` or `unknown`.
    pub severity: String,
    /// Start line.
    pub start_line: u64,
    /// Start character (UTF-16 code units, as the server counts).
    pub start_character: u64,
    /// End line.
    pub end_line: u64,
    /// End character.
    pub end_character: u64,
    /// Message text.
    pub message: String,
    /// Tool that produced it, e.g. `clang`.
    pub source: Option<String>,
    /// Diagnostic code.
    pub code: Option<String>,
    /// Tagged `Unnecessary` (`DiagnosticTag` 1): unused or unnecessary code, drawn grey
    /// (`SPEC-0070` v5 behaviour 6).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unnecessary: bool,
}

/// Error code used internally when the server stopped before answering.
const CLOSED: i64 = i64::MIN;
/// Lines of server stderr kept for error messages.
const STDERR_LINES: usize = 20;

type Answer = Result<Value, (i64, String)>;
type Pending = Arc<StdMutex<HashMap<i64, oneshot::Sender<Answer>>>>;
type Writer = Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

/// Frames `message` for the wire.
pub fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Largest message body read (M8.2): a larger `Content-Length` is a protocol error instead of an
/// allocation of that size.
pub const MAX_MESSAGE: usize = 256 << 20;

/// Reads one framed message; `None` at a clean end of input.
pub async fn read_message<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Value>, LspError> {
    let mut length = None;
    let mut first = true;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| LspError::Io(e.to_string()))?;
        if n == 0 {
            return if first {
                Ok(None)
            } else {
                Err(LspError::Protocol("input ended inside a header".into()))
            };
        }
        first = false;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = Some(value.trim().parse::<usize>().map_err(|_| {
                LspError::Protocol(format!("bad Content-Length `{}`", value.trim()))
            })?);
        }
    }
    let length = length.ok_or_else(|| LspError::Protocol("missing Content-Length".into()))?;
    if length > MAX_MESSAGE {
        return Err(LspError::Protocol(format!(
            "Content-Length {length} is larger than {MAX_MESSAGE}"
        )));
    }
    // Grows with what arrives, so a large length on a short stream allocates little.
    let mut body = Vec::new();
    (&mut *reader)
        .take(length as u64)
        .read_to_end(&mut body)
        .await
        .map_err(|e| LspError::Io(e.to_string()))?;
    if body.len() < length {
        return Err(LspError::Io("input ended inside a message".into()));
    }
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| LspError::Protocol(format!("body is not JSON: {e}")))
}

async fn write(writer: &Writer, message: &Value) -> Result<(), LspError> {
    let mut w = writer.lock().await;
    w.write_all(&frame(message))
        .await
        .map_err(|e| LspError::Io(e.to_string()))?;
    w.flush().await.map_err(|e| LspError::Io(e.to_string()))
}

/// The value at the dotted `section` of `settings` (`python.analysis`); the whole object without a
/// section, `null` where the path does not exist (`SPEC-0170` behaviour 1).
pub fn configuration_section(settings: &Value, section: Option<&str>) -> Value {
    let Some(section) = section.filter(|s| !s.is_empty()) else {
        return settings.clone();
    };
    section
        .split('.')
        .try_fold(settings, |v, key| v.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The options of the formatting requests under way, by file (`SPEC-0043` v5 behaviour 13).
static FORMAT_REQUESTS: StdMutex<Vec<(std::path::PathBuf, Value)>> = StdMutex::new(Vec::new());

/// A formatting request for a file, from before the file is opened until the answer is there:
/// while it lives, [`formatting_options`] answers with the request's options. Without it
/// typescript-language-server, which asks for a file's `formattingOptions` when the file is
/// opened, may hear the indentation the unformatted file happens to use after it got the
/// request, and formats with that.
pub(crate) struct FormatRequest(std::path::PathBuf);

impl FormatRequest {
    pub(crate) fn new(file: &Path, options: Value) -> Self {
        FORMAT_REQUESTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((file.to_path_buf(), options));
        Self(file.to_path_buf())
    }
}

impl Drop for FormatRequest {
    fn drop(&mut self) {
        let mut requests = FORMAT_REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = requests.iter().position(|(file, _)| *file == self.0) {
            requests.remove(at);
        }
    }
}

/// The `formattingOptions` of `path` for a server (`SPEC-0095` v7 behaviour 3a): `tabSize` and
/// `insertSpaces` from the file's `.editorconfig`, else from the indentation the file uses; `null`
/// when neither says (the server's defaults). While the file is being formatted, the options of
/// that request (`SPEC-0043` v5 behaviour 13).
pub fn formatting_options(path: &Path) -> Value {
    if let Some((_, options)) = FORMAT_REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(file, _)| file == path)
    {
        return json!({"tabSize": options["tabSize"], "insertSpaces": options["insertSpaces"]});
    }
    let config = ferris_idea_core::editorconfig::editorconfig_for(path);
    let tabs = config
        .indent_style()
        .map(|s| s == ferris_idea_core::editorconfig::IndentStyle::Tab);
    let (size, tabs) = match (config.indent_size(), tabs) {
        (Some(n), tabs) => (Some(n), tabs.unwrap_or(false)),
        (None, Some(true)) => (config.tab_width().or(Some(4)), true),
        (None, explicit) => {
            let found = std::fs::read_to_string(path)
                .ok()
                .and_then(|t| indentation_of(&t));
            match found {
                Some((n, t)) => (Some(n), explicit.unwrap_or(t)),
                None => (None, false),
            }
        }
    };
    match size {
        Some(n) => json!({"tabSize": n, "insertSpaces": !tabs}),
        None => Value::Null,
    }
}

/// The indentation a text uses: the step between the indents of its lines (their greatest common
/// divisor; the smallest indent when a lone odd one makes that 1), and whether more lines start
/// with a tab than with spaces. `None` without indented lines.
pub fn indentation_of(text: &str) -> Option<(usize, bool)> {
    let (mut tab_lines, mut space_lines) = (0, 0);
    let mut widths = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tab_lines += 1;
        } else if line.starts_with(' ') {
            space_lines += 1;
            let n = line.len() - line.trim_start_matches(' ').len();
            // A continuation such as ` * ` of a block comment is no indent level.
            if !line.trim_start().starts_with('*') {
                widths.push(n);
            }
        }
    }
    if tab_lines == 0 && space_lines == 0 {
        return None;
    }
    if tab_lines > space_lines {
        return Some((4, true));
    }
    fn gcd(a: usize, b: usize) -> usize {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    let step = widths.iter().copied().fold(0, gcd);
    let step = if step >= 2 {
        step
    } else {
        widths.iter().copied().min()?
    };
    Some((step, false))
}

/// Whether `$/progress` parameters end ruby-lsp's indexing (`SPEC-0042` v6 behaviour 13).
pub(crate) fn indexing_done(params: &Value) -> bool {
    params["token"] == "indexing-progress" && params["value"]["kind"] == "end"
}

/// The notification with which intelephense says its indexing ended, besides the progress (up to
/// version 1.12 it reports no progress at all; `SPEC-0042` v19 behaviour 25).
pub(crate) const INDEXING_ENDED: &str = "indexingEnded";

/// The notification with which intelephense up to 1.12 says it begins indexing.
pub(crate) const INDEXING_STARTED: &str = "indexingStarted";

/// How long a server that indexes only when its stored index is out of date may stay silent,
/// without beginning an indexing, before it counts as ready (`SPEC-0042` v19 behaviour 25).
pub(crate) const INDEXING_SETTLE: Duration = Duration::from_secs(3);

/// The work-done progresses with which a server reports that it indexes the project: ruby-lsp's
/// token `indexing-progress`, and a progress intelephense titles "Intelephense (version)
/// indexing" under a token it makes up (`SPEC-0042` v16 behaviour 25).
#[derive(Debug, Default)]
pub(crate) struct Indexing {
    tokens: std::collections::HashSet<String>,
}

impl Indexing {
    /// Takes the parameters of a `$/progress`; whether they end the server's indexing.
    pub(crate) fn ended(&mut self, params: &Value) -> bool {
        if indexing_done(params) {
            return true;
        }
        let token = params["token"].to_string();
        let value = &params["value"];
        match value["kind"].as_str() {
            Some("begin") => {
                let title = value["title"].as_str().unwrap_or_default().to_lowercase();
                if title.contains("intelephense") && title.contains("indexing") {
                    self.tokens.insert(token);
                }
                false
            }
            Some("end") => self.tokens.remove(&token),
            _ => false,
        }
    }

    /// Whether an intelephense indexing progress has begun and not ended.
    pub(crate) fn running(&self) -> bool {
        !self.tokens.is_empty()
    }
}

/// Whether a `$/progress` titled `title` is the server loading the project, as opposed to work on
/// a loaded one such as a check or a build (`SPEC-0095` v10 behaviour 12).
pub(crate) fn loads_project(title: &str) -> bool {
    let title = title.to_lowercase();
    [
        "load",
        "index",
        "import",
        "initializ",
        "initialis",
        "setting up",
        "starting",
        "scanning",
    ]
    .iter()
    .any(|word| title.contains(word))
}

/// How many of a server's messages to the user are kept ([`LspClient::take_messages`]).
const KEPT_MESSAGES: usize = 20;

/// What a server tells the user (`SPEC-0043` v4 behaviour 9): the text of an error it shows with
/// `window/showMessage` (type 1; warnings, notes and logs are not kept), or of a question it asks
/// with `window/showMessageRequest`, followed by the answers it offers. `None` for anything else.
pub(crate) fn user_message(method: &str, params: &Value) -> Option<String> {
    let text = params.get("message").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }
    match method {
        "window/showMessage" => (params["type"] == 1).then(|| text.to_owned()),
        "window/showMessageRequest" => {
            let choices: Vec<&str> = params
                .get("actions")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|c| c["title"].as_str()).collect())
                .unwrap_or_default();
            Some(if choices.is_empty() {
                text.to_owned()
            } else {
                format!("{text} [{}]", choices.join(" | "))
            })
        }
        _ => None,
    }
}

/// Keeps `message` as the newest of at most [`KEPT_MESSAGES`].
fn keep_message(kept: &StdMutex<Vec<String>>, message: String) {
    let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
    if kept.len() == KEPT_MESSAGES {
        kept.remove(0);
    }
    kept.push(message);
}

/// jdtls's `java.format.insertSpaces` or `java.format.tabSize` (`key`) for the file of a
/// `workspace/configuration` item (`SPEC-0095` v13 behaviour 3b): from [`formatting_options`];
/// when the file says nothing, four spaces, the usual Java indentation.
fn java_format_setting(item: &Value, key: &str) -> Value {
    let options = item
        .get("scopeUri")
        .and_then(Value::as_str)
        .and_then(crate::insight::uri_to_path)
        .map(|path| formatting_options(&path))
        .filter(|o| !o.is_null())
        .unwrap_or_else(|| json!({"tabSize": 4, "insertSpaces": true}));
    options[key].clone()
}

/// Answer to a request from the server (`SPEC-0023` behaviour 4).
fn answer_server_request(
    method: &str,
    params: &Value,
    settings: &Value,
) -> Result<Value, (i64, String)> {
    match method {
        "workspace/configuration" => {
            let items = params
                .get("items")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            Ok(Value::Array(
                items
                    .iter()
                    .map(|item| {
                        let section = item.get("section").and_then(Value::as_str);
                        // typescript-language-server asks each file's indentation before its code
                        // actions and refactorings (`SPEC-0095` v7 behaviour 3a).
                        if section == Some("formattingOptions")
                            && let Some(path) = item
                                .get("scopeUri")
                                .and_then(Value::as_str)
                                .and_then(crate::insight::uri_to_path)
                        {
                            return formatting_options(&path);
                        }
                        // jdtls asks the same before its code actions, as two settings of the
                        // file; unanswered it indents with a tab (`SPEC-0095` v13 behaviour 3b).
                        if let Some(key) = section
                            .and_then(|s| s.strip_prefix("java.format."))
                            .filter(|k| matches!(*k, "insertSpaces" | "tabSize"))
                        {
                            return java_format_setting(item, key);
                        }
                        if settings.is_null() {
                            return Value::Null;
                        }
                        configuration_section(settings, section)
                    })
                    .collect(),
            ))
        }
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create" => Ok(Value::Null),
        // A question to the user: none of the offered answers is chosen for them; the text is
        // kept for whoever asked the server (`SPEC-0043` v4 behaviour 9).
        "window/showMessageRequest" => Ok(Value::Null),
        // typescript-language-server asks the client to start a rename after an extraction; the
        // edit has arrived, and Ferris IDEA offers its own in-place rename (SPEC-0095 v3).
        "_typescript.rename" => Ok(Value::Null),
        other => Err((-32601, format!("method not found: {other}"))),
    }
}

/// A connection to one language server.
pub struct LspClient {
    writer: Writer,
    pending: Pending,
    next_id: AtomicI64,
    notifications: mpsc::UnboundedReceiver<Notification>,
    /// Notifications received while waiting for the server to be ready, handed out first.
    held: std::collections::VecDeque<Notification>,
    /// How many diagnostics publications have come for each URI (`SPEC-0042` v20 behaviour 36).
    publications: std::collections::HashMap<String, u32>,
    closed: Arc<StdMutex<Option<String>>>,
    applied_edits: Arc<StdMutex<Vec<Value>>>,
    /// Errors the server showed and questions it asked ([`user_message`]), newest last.
    messages: Arc<StdMutex<Vec<String>>>,
    /// Settings handed out for `workspace/configuration` (`SPEC-0170`; null: none).
    settings: Arc<StdMutex<Value>>,
    child: Option<Child>,
    /// The server's program name, for messages.
    pub(crate) name: String,
    /// Time allowed for each request.
    pub timeout: Duration,
    /// `initializationOptions` sent with `initialize` (null: none).
    pub init_options: Value,
}

impl LspClient {
    /// Connects over the given streams; `stderr` (if any) feeds error messages.
    pub fn new<R, W, E>(reader: R, writer: W, stderr: Option<E>) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
        E: AsyncRead + Send + Unpin + 'static,
    {
        let writer: Writer = Arc::new(Mutex::new(Box::new(writer)));
        let pending: Pending = Arc::default();
        let closed: Arc<StdMutex<Option<String>>> = Arc::default();
        let tail: Arc<StdMutex<VecDeque<String>>> = Arc::default();
        let (tx, notifications) = mpsc::unbounded_channel();

        let (stderr_done, stderr_finished) = oneshot::channel::<()>();
        match stderr {
            Some(stderr) => {
                let tail = Arc::clone(&tail);
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let mut t = tail.lock().unwrap_or_else(|p| p.into_inner());
                        if t.len() == STDERR_LINES {
                            t.pop_front();
                        }
                        t.push_back(line);
                    }
                    let _ = stderr_done.send(());
                });
            }
            None => {
                let _ = stderr_done.send(());
            }
        }

        let applied_edits = Arc::new(StdMutex::new(Vec::new()));
        let settings = Arc::new(StdMutex::new(Value::Null));
        let messages: Arc<StdMutex<Vec<String>>> = Arc::default();
        let (w, p, c, a, st, said) = (
            Arc::clone(&writer),
            Arc::clone(&pending),
            Arc::clone(&closed),
            Arc::clone(&applied_edits),
            Arc::clone(&settings),
            Arc::clone(&messages),
        );
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let reason = loop {
                let message = match read_message(&mut reader).await {
                    Ok(Some(m)) => m,
                    Ok(None) => break String::new(),
                    Err(e) => break e.to_string(),
                };
                let id = message.get("id").cloned();
                match (message.get("method").and_then(Value::as_str), id) {
                    (Some(method), Some(id)) => {
                        let params = message.get("params").cloned().unwrap_or(Value::Null);
                        if let Some(text) = user_message(method, &params) {
                            keep_message(&said, text);
                        }
                        // Edits the server wants applied are kept for a preview; the
                        // server only learns that they were taken (SPEC-0041).
                        let answer = if method == "workspace/applyEdit" {
                            a.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push(params.get("edit").cloned().unwrap_or(Value::Null));
                            Ok(json!({"applied": true}))
                        } else {
                            let settings = st.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            answer_server_request(method, &params, &settings)
                        };
                        let reply = match answer {
                            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                            Err((code, text)) => json!({"jsonrpc": "2.0", "id": id,
                                "error": {"code": code, "message": text}}),
                        };
                        if write(&w, &reply).await.is_err() {
                            break String::new();
                        }
                    }
                    (Some(method), None) => {
                        let params = message.get("params").cloned().unwrap_or(Value::Null);
                        if let Some(text) = user_message(method, &params) {
                            keep_message(&said, text);
                        }
                        let _ = tx.send(Notification {
                            method: method.to_owned(),
                            params,
                        });
                    }
                    (None, Some(id)) => {
                        let Some(id) = id.as_i64() else { continue };
                        let waiting = p.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                        if let Some(waiting) = waiting {
                            let answer = match message.get("error") {
                                Some(err) => Err((
                                    err.get("code").and_then(Value::as_i64).unwrap_or(0),
                                    err.get("message")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned(),
                                )),
                                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                            };
                            let _ = waiting.send(answer);
                        }
                    }
                    (None, None) => {}
                }
            };
            // Give the stderr reader a moment to collect the server's last words.
            let _ = tokio::time::timeout(Duration::from_millis(500), stderr_finished).await;
            let stderr = tail
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            let text = [reason, stderr]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("; ");
            *c.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
            for (_, waiting) in p.lock().unwrap_or_else(|e| e.into_inner()).drain() {
                let _ = waiting.send(Err((CLOSED, text.clone())));
            }
        });

        Self {
            writer,
            pending,
            next_id: AtomicI64::new(1),
            notifications,
            held: std::collections::VecDeque::new(),
            publications: std::collections::HashMap::new(),
            closed,
            applied_edits,
            messages,
            settings,
            child: None,
            name: "the language server".to_owned(),
            timeout: Duration::from_secs(30),
            init_options: Value::Null,
        }
    }

    /// Starts `command` with `args` in `root` and connects to it.
    pub fn spawn(command: &str, args: &[&str], root: &Path) -> Result<Self, LspError> {
        Self::spawn_with_path(command, args, root, None)
    }

    /// [`Self::spawn`] with `path` as the server's `PATH` when given (`SPEC-0042` v6 behaviour 12).
    pub fn spawn_with_path(
        command: &str,
        args: &[&str],
        root: &Path,
        path: Option<&std::ffi::OsStr>,
    ) -> Result<Self, LspError> {
        let mut command_line = Command::new(command);
        if let Some(path) = path {
            command_line.env("PATH", path);
        }
        let mut child = command_line
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| LspError::Start {
                command: command.to_owned(),
                message: e.to_string(),
            })?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take();
        let mut client = Self::new(stdout, stdin, stderr);
        client.child = Some(child);
        if let Some(name) = Path::new(command).file_name() {
            client.name = name.to_string_lossy().into_owned();
        }
        Ok(client)
    }

    /// Hands the notifications (those held back while waiting first) to a long-lived reader, such as
    /// the server pool's pump (`SPEC-0090`); later calls of `next_notification` see none.
    pub(crate) fn take_notifications(
        &mut self,
    ) -> (
        VecDeque<Notification>,
        mpsc::UnboundedReceiver<Notification>,
    ) {
        let (_, empty) = mpsc::unbounded_channel();
        (
            std::mem::take(&mut self.held),
            std::mem::replace(&mut self.notifications, empty),
        )
    }

    /// Whether the server has stopped or its connection is gone.
    pub(crate) fn is_closed(&self) -> bool {
        self.closed_error().is_some()
    }

    fn closed_error(&self) -> Option<LspError> {
        self.closed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .map(LspError::Closed)
    }

    /// Sends a request and waits for its result. A rejection because the document changed or the
    /// server restarted its analysis meanwhile (`ContentModified`, `ServerCancelled`) is asked again
    /// a few times with growing pauses (`SPEC-0090` v2 behaviour 7).
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, LspError> {
        let mut pause = Duration::from_millis(100);
        let mut attempt = 1;
        loop {
            match self.request_once(method, params.clone()).await {
                Err(LspError::Response { code, .. })
                    if RETRIED_CODES.contains(&code) && attempt < RETRY_ATTEMPTS =>
                {
                    tokio::time::sleep(pause).await;
                    pause *= 2;
                    attempt += 1;
                }
                answer => return answer,
            }
        }
    }

    async fn request_once(&self, method: &str, params: Value) -> Result<Value, LspError> {
        if let Some(e) = self.closed_error() {
            return Err(e);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = write(&self.writer, &message).await {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            return Err(self.closed_error().unwrap_or(e));
        }
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err((CLOSED, text)))) => Err(LspError::Closed(text)),
            Ok(Ok(Err((code, message)))) => Err(response_error(&self.name, method, code, message)),
            Ok(Err(_)) => Err(self
                .closed_error()
                .unwrap_or(LspError::Closed(String::new()))),
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                Err(LspError::Timeout(method.to_owned(), self.timeout.as_secs()))
            }
        }
    }

    /// Sends a request and waits at most `wait` for its result, or until `stop` is set; a request
    /// given up is withdrawn with `$/cancelRequest` so the server stops working on it (`SPEC-0293`
    /// behaviour 2). The client's own [`Self::timeout`] does not apply.
    pub async fn request_bounded(
        &self,
        method: &str,
        params: Value,
        wait: Duration,
        stop: Option<Arc<AtomicBool>>,
    ) -> Result<Value, LspError> {
        if let Some(e) = self.closed_error() {
            return Err(e);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = write(&self.writer, &message).await {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            return Err(self.closed_error().unwrap_or(e));
        }
        let stopped = async {
            loop {
                if stop.as_ref().is_some_and(|s| s.load(Ordering::SeqCst)) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        let given_up = tokio::select! {
            answer = tokio::time::timeout(wait, rx) => match answer {
                Ok(Ok(Ok(result))) => return Ok(result),
                Ok(Ok(Err((CLOSED, text)))) => return Err(LspError::Closed(text)),
                Ok(Ok(Err((code, message)))) => {
                    return Err(response_error(&self.name, method, code, message));
                }
                Ok(Err(_)) => {
                    return Err(self
                        .closed_error()
                        .unwrap_or(LspError::Closed(String::new())));
                }
                Err(_) => LspError::Timeout(method.to_owned(), wait.as_secs()),
            },
            () = stopped => LspError::Stopped(method.to_owned()),
        };
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        let _ = self.notify("$/cancelRequest", json!({"id": id})).await;
        Err(given_up)
    }

    /// Sets the settings answered to `workspace/configuration` (before `initialize`, so the
    /// first answers already carry them; `SPEC-0170` behaviour 3).
    pub fn set_settings(&self, settings: Value) {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings;
    }

    /// The settings answered to `workspace/configuration`.
    pub fn settings(&self) -> Value {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Replaces the settings and tells the server with `workspace/didChangeConfiguration`
    /// (`SPEC-0170` behaviour 4).
    pub async fn change_settings(&self, settings: Value) -> Result<(), LspError> {
        self.set_settings(settings.clone());
        self.notify(
            "workspace/didChangeConfiguration",
            json!({"settings": settings}),
        )
        .await
    }

    /// Sends a notification.
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        write(
            &self.writer,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
        .await
    }

    /// The next notification, or `None` after `wait` or when the server stopped.
    pub async fn next_notification(&mut self, wait: Duration) -> Option<Notification> {
        if let Some(n) = self.held.pop_front() {
            return Some(n);
        }
        let n = tokio::time::timeout(wait, self.notifications.recv())
            .await
            .ok()
            .flatten()?;
        self.count(&n);
        Some(n)
    }

    /// Counts a received publication of diagnostics.
    fn count(&mut self, n: &Notification) {
        if n.method == "textDocument/publishDiagnostics"
            && let Some(uri) = n.params["uri"].as_str()
        {
            *self.publications.entry(uri.to_owned()).or_default() += 1;
        }
    }

    /// How many publications of diagnostics for `uri` have come so far, held ones included.
    pub fn publications(&self, uri: &str) -> u32 {
        self.publications.get(uri).copied().unwrap_or(0)
    }

    /// The diagnostics of the last publication for `uri` among the held notifications, which stay
    /// held (`SPEC-0076` v6 behaviour 10).
    pub fn latest_held_diagnostics(&self, uri: &str) -> Option<Vec<Value>> {
        self.held
            .iter()
            .rev()
            .find(|n| {
                n.method == "textDocument/publishDiagnostics"
                    && n.params["uri"].as_str() == Some(uri)
            })
            .map(|n| {
                n.params["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
    }

    /// Waits up to `wait` until a server that reports its state says it has loaded the workspace:
    /// `experimental/serverStatus` with `quiescent` (rust-analyzer), `language/status` with
    /// `ServiceReady` (jdtls) or the end of the `indexing-progress` progress (ruby-lsp, v6) or of
    /// intelephense's indexing (v16 behaviour 25; the diagnostics it published before are about a
    /// project it had not read and are dropped: it publishes them again). Other notifications are
    /// kept for `next_notification`. Returns whether the server became ready in time
    /// (`SPEC-0042` v2, v3).
    pub async fn wait_until_ready(&mut self, wait: Duration, settle: Option<Duration>) -> bool {
        let deadline = tokio::time::Instant::now() + wait;
        let mut indexing = Indexing::default();
        // With `settle`, a server that has begun no indexing and stays silent that long is
        // ready: it read its stored index (intelephense, v19 behaviour 25).
        let mut begun = false;
        loop {
            let now = tokio::time::Instant::now();
            let until = match settle {
                Some(quiet) if !begun => (now + quiet).min(deadline),
                _ => deadline,
            };
            let n = match tokio::time::timeout(until - now, self.notifications.recv()).await {
                Ok(Some(n)) => n,
                Ok(None) => return false,
                Err(_) => return settle.is_some() && !begun && until < deadline,
            };
            self.count(&n);
            if n.method == "experimental/serverStatus" {
                if n.params["quiescent"] == true {
                    return true;
                }
            } else if n.method == "language/status" {
                if n.params["type"] == "ServiceReady" {
                    return true;
                }
            } else if (n.method == "$/progress" && indexing.ended(&n.params))
                || n.method == INDEXING_ENDED
            {
                if n.method == INDEXING_ENDED || !indexing_done(&n.params) {
                    self.held
                        .retain(|h| h.method != "textDocument/publishDiagnostics");
                }
                return true;
            } else {
                begun |= n.method == INDEXING_STARTED || indexing.running();
                self.held.push_back(n);
            }
        }
    }

    /// Waits up to `wait` until the server publishes diagnostics for `uri`, the sign that it has
    /// taken in the opened text (jdtls answers about it only after that). Every notification, the
    /// diagnostics included, stays for `next_notification` (`SPEC-0042` v3).
    pub async fn wait_for_diagnostics(&mut self, uri: &str, wait: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Ok(Some(n)) = tokio::time::timeout(left, self.notifications.recv()).await else {
                return false;
            };
            self.count(&n);
            let done = n.method == "textDocument/publishDiagnostics"
                && n.params["uri"].as_str() == Some(uri);
            self.held.push_back(n);
            if done {
                return true;
            }
        }
    }

    /// `initialize` with `root` as workspace, then `initialized`; returns the server capabilities.
    pub async fn initialize(&self, root: &Path) -> Result<Value, LspError> {
        let uri = file_uri(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let result = self
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "clientInfo": {"name": "Ferris IDEA"},
                    "rootUri": uri,
                    "workspaceFolders": [{"uri": uri, "name": name}],
                    "initializationOptions": self.init_options,
                    "capabilities": client_capabilities()
                }),
            )
            .await?;
        self.notify("initialized", json!({})).await?;
        // typescript-language-server reads its settings only from this notification, and
        // kotlin-language-server gives parameter hints once it has had it (SPEC-0291).
        let settings = self.settings();
        if settings.get("typescript").is_some() || settings.get("kotlin").is_some() {
            self.change_settings(settings).await?;
        }
        Ok(result.get("capabilities").cloned().unwrap_or(Value::Null))
    }

    /// Opens `path` with `text` and returns the first diagnostics published for it.
    pub async fn diagnostics(
        &mut self,
        path: &Path,
        language: LanguageId,
        text: &str,
        wait: Duration,
    ) -> Result<Vec<Diagnostic>, LspError> {
        let uri = file_uri(path);
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": language.lsp_id_for(&uri), "version": 1, "text": text}}),
        )
        .await?;
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Some(n) = self.next_notification(left).await else {
                return Err(self.closed_error().unwrap_or(LspError::Timeout(
                    "textDocument/publishDiagnostics".into(),
                    wait.as_secs(),
                )));
            };
            if n.method == "textDocument/publishDiagnostics"
                && n.params.get("uri").and_then(Value::as_str) == Some(uri.as_str())
            {
                let list = n.params.get("diagnostics").and_then(Value::as_array);
                return Ok(list.into_iter().flatten().map(diagnostic).collect());
            }
        }
    }

    /// Opens every file of `files` (path, language, text) and then collects the first diagnostics
    /// published for each, all within one `wait`, in the order given (`SPEC-0023` v5 behaviour 13).
    /// A file without diagnostics by then is a timeout of its own; when the server stops, every
    /// file still waiting gets its error.
    pub async fn diagnostics_of_files(
        &mut self,
        files: &[(PathBuf, LanguageId, String)],
        wait: Duration,
    ) -> Vec<Result<Vec<Diagnostic>, LspError>> {
        let uris: Vec<String> = files.iter().map(|(p, ..)| file_uri(p)).collect();
        let mut found: Vec<Option<Result<Vec<Diagnostic>, LspError>>> =
            (0..files.len()).map(|_| None).collect();
        for (i, ((_, language, text), uri)) in files.iter().zip(&uris).enumerate() {
            if let Err(e) = self
                .notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {"uri": uri, "languageId": language.lsp_id_for(uri), "version": 1, "text": text}}),
                )
                .await
            {
                found[i] = Some(Err(e));
            }
        }
        let deadline = tokio::time::Instant::now() + wait;
        while found.iter().any(Option::is_none) {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Some(n) = self.next_notification(left).await else {
                let e = self.closed_error().unwrap_or(LspError::Timeout(
                    "textDocument/publishDiagnostics".into(),
                    wait.as_secs(),
                ));
                for slot in found.iter_mut().filter(|f| f.is_none()) {
                    *slot = Some(Err(e.clone()));
                }
                break;
            };
            if n.method != "textDocument/publishDiagnostics" {
                continue;
            }
            let uri = n.params.get("uri").and_then(Value::as_str);
            if let Some(i) = (0..files.len()).find(|&i| found[i].is_none() && uri == Some(&uris[i]))
            {
                let list = n.params.get("diagnostics").and_then(Value::as_array);
                found[i] = Some(Ok(list.into_iter().flatten().map(diagnostic).collect()));
            }
        }
        found
            .into_iter()
            .map(|f| f.unwrap_or_else(|| Err(LspError::NoServer(String::new()))))
            .collect()
    }

    /// Takes the workspace edits the server sent with `workspace/applyEdit` so far.
    pub fn take_applied_edits(&self) -> Vec<Value> {
        std::mem::take(&mut *self.applied_edits.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Takes the errors the server showed the user and the questions it asked so far
    /// ([`user_message`]), oldest first.
    pub fn take_messages(&self) -> Vec<String> {
        std::mem::take(&mut *self.messages.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// `shutdown` and `exit`; kills the server if it does not exit within 2 s.
    pub async fn shutdown(mut self) {
        self.timeout = Duration::from_secs(5);
        let _ = self.request("shutdown", Value::Null).await;
        let _ = self.notify("exit", Value::Null).await;
        if let Some(mut child) = self.child.take()
            && tokio::time::timeout(Duration::from_secs(2), child.wait())
                .await
                .is_err()
        {
            let _ = child.kill().await;
        }
    }
}

pub(crate) fn diagnostic(v: &Value) -> Diagnostic {
    let pos = |a: &str, b: &str| {
        v.pointer(&format!("/range/{a}/{b}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    Diagnostic {
        severity: match v.get("severity").and_then(Value::as_u64) {
            Some(1) => "error",
            Some(2) => "warning",
            Some(3) => "information",
            Some(4) => "hint",
            _ => "unknown",
        }
        .to_owned(),
        start_line: pos("start", "line"),
        start_character: pos("start", "character"),
        end_line: pos("end", "line"),
        end_character: pos("end", "character"),
        message: v
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        source: v.get("source").and_then(Value::as_str).map(str::to_owned),
        code: v.get("code").map(|c| match c {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }),
        unnecessary: v
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|tags| tags.iter().any(|t| t.as_u64() == Some(1))),
    }
}

/// `file://` URI of an absolute path (RFC 8089), percent-encoding everything
/// outside the unreserved set and `/`.
pub fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Diagnostics of `file` (inside `root`) from its language's default server.
pub async fn file_diagnostics(
    root: &Path,
    file: &Path,
    wait: Duration,
) -> Result<Vec<Diagnostic>, LspError> {
    let language =
        LanguageId::detect(file).ok_or_else(|| LspError::NoServer(file.display().to_string()))?;
    let server = crate::servers::resolve_for(language, root)?;
    // A server that publishes in steps is asked through `Opened`, which pulls its diagnostics
    // (`SPEC-0141` v4 behaviour 9).
    // So is a server that says when it has loaded the project: its diagnostics from before are
    // not the answer (`SPEC-0042` v16 behaviour 25).
    if crate::pool::enabled()
        || server.candidate.pulls_diagnostics
        || server.candidate.reports_status
    {
        let mut o = crate::insight::Opened::open_text(root, file, None, wait).await?;
        let found = o.published_diagnostics(wait).await;
        o.close().await;
        // A server that says nothing in time has not analysed the file: no empty list, which
        // would read as a clean file (`SPEC-0006` v59 behaviour 23).
        return found
            .map(|list| list.iter().map(diagnostic).collect())
            .ok_or_else(|| {
                LspError::Timeout("textDocument/publishDiagnostics".into(), wait.as_secs())
            });
    }
    let text = std::fs::read_to_string(file).map_err(|e| LspError::Io(e.to_string()))?;
    let mut client = server.spawn(root)?;
    client.timeout = wait;
    client.init_options = server.initialization_options(root);
    client.set_settings(server.settings(root));
    let result = async {
        client.initialize(root).await?;
        client.diagnostics(file, language, &text, wait).await
    }
    .await;
    client.shutdown().await;
    result
}

/// Diagnostics of several `files` (inside `root`), one result per file in the order given
/// (`SPEC-0023` v5 behaviour 13). Files served by the same server program share one start of it
/// and one `wait`; a server that is asked through an opened document (it pulls its diagnostics or
/// says when it has loaded the project, or the pool runs) is asked file by file as
/// [`file_diagnostics`] does. A file that cannot be read, has no server or gets no answer has its
/// error; the other files are answered all the same.
pub async fn files_diagnostics(
    root: &Path,
    files: &[PathBuf],
    wait: Duration,
) -> Vec<Result<Vec<Diagnostic>, LspError>> {
    type Member = (usize, PathBuf, LanguageId, String);
    let mut out: Vec<Option<Result<Vec<Diagnostic>, LspError>>> =
        (0..files.len()).map(|_| None).collect();
    let mut groups: Vec<(crate::servers::Resolved, Vec<Member>)> = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let Some(language) = LanguageId::detect(file) else {
            out[i] = Some(Err(LspError::NoServer(file.display().to_string())));
            continue;
        };
        let server = match crate::servers::resolve_for(language, root) {
            Ok(s) => s,
            Err(e) => {
                out[i] = Some(Err(e));
                continue;
            }
        };
        if crate::pool::enabled()
            || server.candidate.pulls_diagnostics
            || server.candidate.reports_status
        {
            out[i] = Some(file_diagnostics(root, file, wait).await);
            continue;
        }
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                out[i] = Some(Err(LspError::Io(e.to_string())));
                continue;
            }
        };
        let member = (i, file.clone(), language, text);
        let same = |s: &crate::servers::Resolved| {
            s.command == server.command
                && s.candidate.program == server.candidate.program
                && s.prefix_args == server.prefix_args
                && s.extra_args == server.extra_args
                && s.path == server.path
                && s.safe == server.safe
        };
        match groups.iter_mut().find(|(s, _)| same(s)) {
            Some((_, members)) => members.push(member),
            None => groups.push((server, vec![member])),
        }
    }
    for (server, members) in groups {
        let results = match server.spawn(root) {
            Err(e) => members.iter().map(|_| Err(e.clone())).collect(),
            Ok(mut client) => {
                client.timeout = wait;
                client.init_options = server.initialization_options(root);
                client.set_settings(server.settings(root));
                let results = match client.initialize(root).await {
                    Err(e) => members.iter().map(|_| Err(e.clone())).collect(),
                    Ok(_) => {
                        let docs: Vec<(PathBuf, LanguageId, String)> = members
                            .iter()
                            .map(|(_, p, l, t)| (p.clone(), *l, t.clone()))
                            .collect();
                        client.diagnostics_of_files(&docs, wait).await
                    }
                };
                client.shutdown().await;
                results
            }
        };
        for ((i, ..), result) in members.iter().zip(results) {
            out[*i] = Some(result);
        }
    }
    out.into_iter()
        .zip(files)
        .map(|(o, f)| o.unwrap_or_else(|| Err(LspError::NoServer(f.display().to_string()))))
        .collect()
}

/// What the client announces in `initialize` (`SPEC-0023`; completion details `SPEC-0072` v5;
/// snippets v7 behaviour 18).
pub(crate) fn client_capabilities() -> Value {
    json!({
        "general": {"positionEncodings": ["utf-8", "utf-16", "utf-32"]},
        "textDocument": {
            "synchronization": {"didSave": false},
            "publishDiagnostics": {"relatedInformation": false, "tagSupport": {"valueSet": [1, 2]}},
            "hover": {"contentFormat": ["markdown", "plaintext"]},
            "signatureHelp": {"signatureInformation": {
                "documentationFormat": ["plaintext", "markdown"],
                "parameterInformation": {"labelOffsetSupport": true},
                "activeParameterSupport": true
            }},
            "definition": {"linkSupport": true},
            "typeDefinition": {"linkSupport": true},
            "implementation": {"linkSupport": true},
            "callHierarchy": {"dynamicRegistration": false},
            "completion": {"completionItem": {
                "snippetSupport": true,
                "labelDetailsSupport": true,
                "preselectSupport": true,
                "documentationFormat": ["markdown", "plaintext"],
                "resolveSupport": {"properties": ["documentation", "detail"]}
            }},
            "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
            // SPEC-0295 behaviour 14.
            "documentHighlight": {"dynamicRegistration": false},
            // SPEC-0291 behaviour 1.
            "inlayHint": {"dynamicRegistration": false},
            // Usage lenses (SPEC-0292 v8 behaviour 11).
            "codeLens": {"dynamicRegistration": false},
            // SPEC-0290 behaviour 1.
            "semanticTokens": {
                "requests": {"full": true},
                "tokenTypes": [
                    "namespace", "type", "class", "enum", "interface", "struct",
                    "typeParameter", "parameter", "variable", "property", "enumMember",
                    "event", "function", "method", "macro", "keyword", "modifier", "comment",
                    "string", "number", "regexp", "operator", "decorator"
                ],
                "tokenModifiers": [
                    "declaration", "definition", "readonly", "static", "deprecated",
                    "abstract", "async", "modification", "documentation", "defaultLibrary"
                ],
                "formats": ["relative"],
                "multilineTokenSupport": false,
                "overlappingTokenSupport": false
            },
            "rename": {"prepareSupport": true},
            "codeAction": {
                "codeActionLiteralSupport": {"codeActionKind": {"valueSet": [
                    "", "quickfix", "refactor", "refactor.extract",
                    "refactor.inline", "refactor.rewrite", "source",
                    "source.organizeImports"
                ]}},
                "isPreferredSupport": true,
                "disabledSupport": true,
                "dataSupport": true,
                "resolveSupport": {"properties": ["edit", "command"]}
            }
        },
        "workspace": {
            "configuration": true, "workspaceFolders": true, "applyEdit": true,
            "workspaceEdit": {"documentChanges": true},
            "fileOperations": {"willRename": true, "didRename": true}
        },
        "window": {"workDoneProgress": true},
        // rust-analyzer offers its reference lenses only to a client that knows their command;
        // Ferris IDEA reads the places from it and never runs it (SPEC-0292 v8 behaviour 11).
        "experimental": {
            "serverStatusNotification": true,
            "commands": {"commands": ["rust-analyzer.showReferences"]}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, DuplexStream, duplex};

    #[test]
    fn completion_details_are_announced() {
        let item = &client_capabilities()["textDocument"]["completion"]["completionItem"];
        assert_eq!(item["labelDetailsSupport"], true);
        assert_eq!(item["preselectSupport"], true);
        assert_eq!(item["snippetSupport"], true);
        assert!(
            item["resolveSupport"]["properties"]
                .as_array()
                .is_some_and(|p| p.contains(&json!("documentation")))
        );
    }

    /// SPEC-0070/AK-5: a diagnostic tagged `Unnecessary` (1) is marked as unused code.
    #[test]
    fn unnecessary_tag_is_read() {
        let tagged = diagnostic(&json!({"message": "unused", "tags": [2, 1]}));
        assert!(tagged.unnecessary);
        assert!(!diagnostic(&json!({"message": "old", "tags": [2]})).unnecessary);
        assert!(!diagnostic(&json!({"message": "plain"})).unnecessary);
        assert_eq!(
            client_capabilities()["textDocument"]["publishDiagnostics"]["tagSupport"]["valueSet"],
            json!([1, 2])
        );
    }

    #[tokio::test]
    async fn framing() {
        let a = json!({"jsonrpc": "2.0", "id": 1, "result": "ü"});
        let mut wire = frame(&a);
        wire.extend_from_slice(b"content-length: 2\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}");
        let mut reader = BufReader::new(&wire[..]);
        assert_eq!(read_message(&mut reader).await.unwrap(), Some(a));
        assert_eq!(read_message(&mut reader).await.unwrap(), Some(json!({})));
        assert_eq!(read_message(&mut reader).await.unwrap(), None);
        let mut bad = BufReader::new(&b"Content-Length: 3\r\n\r\nabc"[..]);
        assert!(matches!(
            read_message(&mut bad).await,
            Err(LspError::Protocol(_))
        ));
    }

    /// M8.2 fuzz finding: a huge `Content-Length` allocated its size up front and aborted.
    #[tokio::test]
    async fn huge_or_cut_length_is_an_error() {
        let mut huge = BufReader::new(&b"Content-Length: 38888888888888888\r\n\r\n{}"[..]);
        assert!(matches!(
            read_message(&mut huge).await,
            Err(LspError::Protocol(_))
        ));
        let mut cut = BufReader::new(&b"Content-Length: 100000\r\n\r\n{}"[..]);
        assert!(matches!(read_message(&mut cut).await, Err(LspError::Io(_))));
    }

    #[test]
    fn uris() {
        assert_eq!(
            file_uri(Path::new("/tmp/a b/ü#1.c")),
            "file:///tmp/a%20b/%C3%BC%231.c"
        );
    }

    /// A fake server on the other ends of the streams.
    struct Fake {
        input: BufReader<DuplexStream>,
        output: DuplexStream,
        stderr: DuplexStream,
    }

    impl Fake {
        async fn recv(&mut self) -> Value {
            read_message(&mut self.input).await.unwrap().unwrap()
        }
        async fn send(&mut self, v: Value) {
            self.output.write_all(&frame(&v)).await.unwrap();
        }
    }

    fn pair() -> (LspClient, Fake) {
        let (client_out, server_in) = duplex(1 << 16);
        let (server_out, client_in) = duplex(1 << 16);
        let (stderr_w, stderr_r) = duplex(1 << 12);
        let client = LspClient::new(client_in, client_out, Some(stderr_r));
        let fake = Fake {
            input: BufReader::new(server_in),
            output: server_out,
            stderr: stderr_w,
        };
        (client, fake)
    }

    /// `SPEC-0043` AK-6: a server's question is answered with no choice and kept with its
    /// answers; an error it shows is kept, a warning or a log line is not.
    #[tokio::test]
    async fn questions_and_errors_of_the_server_are_kept() {
        assert_eq!(
            user_message(
                "window/showMessage",
                &json!({"type": 1, "message": " no formatter "})
            ),
            Some("no formatter".into())
        );
        assert_eq!(
            user_message("window/showMessage", &json!({"type": 2, "message": "x"})),
            None
        );
        assert_eq!(
            user_message("window/logMessage", &json!({"type": 1, "message": "x"})),
            None
        );
        assert_eq!(
            user_message(
                "window/showMessageRequest",
                &json!({"type": 3, "message": ""})
            ),
            None
        );

        let (client, mut fake) = pair();
        fake.send(
            json!({"jsonrpc": "2.0", "id": 7, "method": "window/showMessageRequest",
            "params": {"type": 1, "message": "No configuration. Proceed?",
                "actions": [{"title": "Create"}, {"title": "Not now"}]}}),
        )
        .await;
        let reply = fake.recv().await;
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["result"], Value::Null);
        assert!(reply.get("error").is_none(), "{reply}");
        fake.send(json!({"jsonrpc": "2.0", "method": "window/showMessage",
            "params": {"type": 2, "message": "only a warning"}}))
            .await;
        fake.send(json!({"jsonrpc": "2.0", "method": "window/showMessage",
            "params": {"type": 1, "message": "tool not found"}}))
            .await;
        // The second message has arrived once a later request is answered.
        let (answer, ()) = tokio::join!(client.request("textDocument/hover", json!(1)), async {
            let r = fake.recv().await;
            fake.send(json!({"jsonrpc": "2.0", "id": r["id"], "result": null}))
                .await;
        });
        answer.unwrap();
        assert_eq!(
            client.take_messages(),
            vec![
                "No configuration. Proceed? [Create | Not now]".to_owned(),
                "tool not found".to_owned()
            ]
        );
        assert!(client.take_messages().is_empty());
        // Only the newest are kept.
        for n in 0..KEPT_MESSAGES + 3 {
            keep_message(&client.messages, n.to_string());
        }
        let kept = client.take_messages();
        assert_eq!(kept.len(), KEPT_MESSAGES);
        assert_eq!(kept[0], "3");
    }

    #[tokio::test]
    async fn configuration_from_settings_and_changes() {
        let (client, mut fake) = pair();
        client.set_settings(json!({"python": {"pythonPath": "/v/bin/python"}}));
        fake.send(
            json!({"jsonrpc": "2.0", "id": 3, "method": "workspace/configuration",
            "params": {"items": [{"section": "python"}, {"section": "python.analysis"}, {}]}}),
        )
        .await;
        assert_eq!(
            fake.recv().await["result"],
            json!([{"pythonPath": "/v/bin/python"}, null,
                {"python": {"pythonPath": "/v/bin/python"}}])
        );
        client
            .change_settings(json!({"python": {"pythonPath": "/w/bin/python"}}))
            .await
            .unwrap();
        assert_eq!(
            fake.recv().await,
            json!({"jsonrpc": "2.0", "method": "workspace/didChangeConfiguration",
                "params": {"settings": {"python": {"pythonPath": "/w/bin/python"}}}})
        );
        assert_eq!(client.settings()["python"]["pythonPath"], "/w/bin/python");
    }

    /// SPEC-0095 v7 behaviour 3a, AK-15: the indentation a file uses, and the answer to
    /// `formattingOptions`, `.editorconfig` first.
    #[test]
    fn formatting_options_follow_the_file() {
        assert_eq!(
            indentation_of("function f() {\n  if (x) {\n    return 1;\n  }\n}\n"),
            Some((2, false))
        );
        assert_eq!(
            indentation_of("class A {\n    m() {\n        return 1;\n    }\n}\n"),
            Some((4, false))
        );
        assert_eq!(
            indentation_of("a {\n    b {\n      c\n    }\n}\n"),
            Some((2, false))
        );
        assert_eq!(
            indentation_of("f() {\n\tif (x) {\n\t\treturn;\n\t}\n}\n"),
            Some((4, true))
        );
        // A block comment's ` * ` and an odd alignment do not decide.
        assert_eq!(
            indentation_of("/**\n * doc\n */\nf(a,\n   b);\nif (x) {\n  y();\n}\n"),
            Some((2, false))
        );
        assert_eq!(indentation_of("const a = 1;\n"), None);

        let dir =
            std::env::temp_dir().join(format!("ferris-idea-fmt-options-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.ts"), "function f() {\n  return 1;\n}\n").unwrap();
        assert_eq!(
            formatting_options(&dir.join("a.ts")),
            json!({"tabSize": 2, "insertSpaces": true})
        );
        assert_eq!(formatting_options(&dir.join("missing.ts")), Value::Null);
        std::fs::write(
            dir.join("sub/.editorconfig"),
            "root = true\n[*.ts]\nindent_style = tab\ntab_width = 8\n",
        )
        .unwrap();
        std::fs::write(dir.join("sub/b.ts"), "function f() {\n  return 1;\n}\n").unwrap();
        assert_eq!(
            formatting_options(&dir.join("sub/b.ts")),
            json!({"tabSize": 8, "insertSpaces": false})
        );
        // Answered in `workspace/configuration`, also for a server without other settings.
        let uri = format!("file://{}", dir.join("a.ts").display());
        let answer = answer_server_request(
            "workspace/configuration",
            &json!({"items": [{"scopeUri": uri, "section": "formattingOptions"}, {"section": "x"}]}),
            &Value::Null,
        )
        .unwrap();
        assert_eq!(answer, json!([{"tabSize": 2, "insertSpaces": true}, null]));
        // SPEC-0043/AK-12: while the file is being formatted, the request's options are the
        // answer, not the indentation the unformatted file happens to have.
        let ask = || {
            answer_server_request(
                "workspace/configuration",
                &json!({"items": [{"scopeUri": uri, "section": "formattingOptions"}]}),
                &Value::Null,
            )
            .unwrap()
        };
        {
            let _request = FormatRequest::new(
                &dir.join("a.ts"),
                json!({"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}),
            );
            assert_eq!(ask(), json!([{"tabSize": 4, "insertSpaces": true}]));
            assert_eq!(
                formatting_options(&dir.join("sub/b.ts")),
                json!({"tabSize": 8, "insertSpaces": false})
            );
        }
        assert_eq!(ask(), json!([{"tabSize": 2, "insertSpaces": true}]));
        // SPEC-0095 v13 AK-23: jdtls asks the two settings of a Java file before a code action;
        // the file's indentation, four spaces when it has none, never no answer.
        std::fs::write(
            dir.join("A.java"),
            "class A {\n  int a() {\n    return 1;\n  }\n}\n",
        )
        .unwrap();
        std::fs::write(dir.join("sub/B.java"), "class B {\n\tint b;\n}\n").unwrap();
        std::fs::write(dir.join("Empty.java"), "class Empty {}\n").unwrap();
        let java = |file: &str| {
            let uri = format!("file://{}", dir.join(file).display());
            answer_server_request(
                "workspace/configuration",
                &json!({"items": [
                    {"scopeUri": uri, "section": "java.format.insertSpaces"},
                    {"scopeUri": uri, "section": "java.format.tabSize"},
                ]}),
                &Value::Null,
            )
            .unwrap()
        };
        assert_eq!(java("A.java"), json!([true, 2]));
        assert_eq!(java("Empty.java"), json!([true, 4]));
        assert_eq!(java("sub/B.java")[0], json!(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0023/AK-7: a request the server does not know is a feature it does not offer, said
    /// with the server's name and in words.
    #[tokio::test]
    async fn an_unknown_request_is_a_feature_the_server_lacks() {
        let (mut client, mut fake) = pair();
        client.name = "some-server".to_owned();
        let (answer, ()) = tokio::join!(
            client.request("textDocument/prepareCallHierarchy", json!(1)),
            async {
                let r = fake.recv().await;
                fake.send(json!({"jsonrpc": "2.0", "id": r["id"],
                    "error": {"code": -32601, "message": "Unhandled method"}}))
                    .await;
            }
        );
        let error = answer.unwrap_err();
        assert!(matches!(error, LspError::Unsupported { .. }), "{error:?}");
        assert_eq!(
            error.to_string(),
            "some-server does not offer a call hierarchy (textDocument/prepareCallHierarchy)"
        );
        let other = response_error("s", "custom/thing", -32601, String::new()).to_string();
        assert_eq!(other, "s does not offer this request (custom/thing)");
        // Any other error stays the server's own.
        assert!(matches!(
            response_error("s", "textDocument/hover", -32603, "boom".into()),
            LspError::Response { code: -32603, .. }
        ));
    }

    /// SPEC-0042/AK-26: intelephense's indexing is the progress it titles so, under any token;
    /// ruby-lsp's is its fixed token; other progress is not.
    #[test]
    fn indexing_progress_of_intelephense_and_ruby_lsp() {
        let begin = |token: &str, title: &str| json!({"token": token, "value": {"kind": "begin", "title": title}});
        let end = |token: &str| json!({"token": token, "value": {"kind": "end"}});
        let mut indexing = Indexing::default();
        assert!(!indexing.ended(&begin("7f3a", "Intelephense (1.18.5) indexing")));
        assert!(
            !indexing
                .ended(&json!({"token": "7f3a", "value": {"kind": "report", "percentage": 90}}))
        );
        assert!(!indexing.ended(&end("other")), "another progress ends");
        assert!(indexing.ended(&end("7f3a")));
        assert!(!indexing.ended(&end("7f3a")), "only once");
        // rust-analyzer's own indexing is not this: it says `quiescent`.
        assert!(!indexing.ended(&begin("rustAnalyzer/Indexing", "Indexing")));
        assert!(!indexing.ended(&end("rustAnalyzer/Indexing")));
        assert!(indexing.ended(&end("indexing-progress")), "ruby-lsp");
    }

    /// SPEC-0042/AK-26: the diagnostics intelephense published before its index was built are
    /// dropped when it is ready; those after it are the answer.
    #[tokio::test]
    async fn diagnostics_from_before_the_index_are_dropped() {
        let (mut client, mut fake) = pair();
        let publish = |list: Value| {
            json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
            "params": {"uri": "file:///p/a.php", "diagnostics": list}})
        };
        fake.send(publish(json!([]))).await;
        fake.send(json!({"jsonrpc": "2.0", "method": "$/progress", "params":
            {"token": "t1", "value": {"kind": "begin", "title": "Intelephense (1.18.5) indexing"}}}))
            .await;
        fake.send(json!({"jsonrpc": "2.0", "method": "$/progress", "params":
            {"token": "t1", "value": {"kind": "end"}}}))
            .await;
        fake.send(publish(json!([{"message": "Undefined method"}])))
            .await;
        assert!(client.wait_until_ready(Duration::from_secs(5), None).await);
        // The begin of the progress is kept; the diagnostics from before it are not.
        let begin = client.next_notification(Duration::from_secs(5)).await;
        assert_eq!(begin.map(|n| n.method).as_deref(), Some("$/progress"));
        let next = client
            .next_notification(Duration::from_secs(5))
            .await
            .expect("the diagnostics after the index");
        assert_eq!(next.method, "textDocument/publishDiagnostics");
        assert_eq!(next.params["diagnostics"][0]["message"], "Undefined method");
    }

    /// SPEC-0042/AK-35 (v19): an intelephense that reports no progress (1.12) ends its indexing
    /// with `indexingEnded`; the diagnostics from before it are dropped as after the progress.
    #[tokio::test]
    async fn intelephense_without_progress_ends_its_indexing_by_notification() {
        let (mut client, mut fake) = pair();
        let publish = |list: Value| {
            json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
            "params": {"uri": "file:///p/a.php", "diagnostics": list}})
        };
        fake.send(json!({"jsonrpc": "2.0", "method": "indexingStarted", "params": [null]}))
            .await;
        fake.send(publish(json!([]))).await;
        fake.send(json!({"jsonrpc": "2.0", "method": "indexingEnded", "params": [null]}))
            .await;
        fake.send(publish(
            json!([{"message": "Undefined method 'missingMethod'."}]),
        ))
        .await;
        assert!(client.wait_until_ready(Duration::from_secs(5), None).await);
        let started = client.next_notification(Duration::from_secs(5)).await;
        assert_eq!(
            started.map(|n| n.method).as_deref(),
            Some("indexingStarted")
        );
        let next = client
            .next_notification(Duration::from_secs(5))
            .await
            .expect("the diagnostics after the index");
        assert_eq!(next.method, "textDocument/publishDiagnostics");
        assert_eq!(
            next.params["diagnostics"][0]["message"],
            "Undefined method 'missingMethod'."
        );
    }

    /// SPEC-0042/AK-35 (v19): an intelephense that read its stored index begins no indexing and
    /// says nothing more: it is ready after the quiet time; one that began indexing is waited for.
    #[tokio::test]
    async fn a_server_with_its_stored_index_is_ready_when_quiet() {
        let (mut client, mut fake) = pair();
        let log = |text: &str| {
            json!({"jsonrpc": "2.0", "method": "window/logMessage",
            "params": {"type": 3, "message": text}})
        };
        fake.send(log("Reading state from the stored index.")).await;
        fake.send(
            json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
            "params": {"uri": "file:///p/a.php", "diagnostics": []}}),
        )
        .await;
        let quiet = Some(Duration::from_millis(300));
        let started = std::time::Instant::now();
        assert!(client.wait_until_ready(Duration::from_secs(5), quiet).await);
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
        // Without the quiet time the same server is not ready.
        let (mut client, mut fake) = pair();
        fake.send(log("Reading state from the stored index.")).await;
        assert!(
            !client
                .wait_until_ready(Duration::from_millis(600), None)
                .await
        );
        // An indexing that began is waited for, however quiet the server is meanwhile.
        let (mut client, mut fake) = pair();
        fake.send(json!({"jsonrpc": "2.0", "method": "indexingStarted", "params": [null]}))
            .await;
        assert!(
            !client
                .wait_until_ready(Duration::from_millis(900), quiet)
                .await
        );
        let (mut client, mut fake) = pair();
        fake.send(json!({"jsonrpc": "2.0", "method": "$/progress", "params":
            {"token": "t1", "value": {"kind": "begin", "title": "Intelephense (1.18.5) indexing"}}}))
            .await;
        assert!(
            !client
                .wait_until_ready(Duration::from_millis(900), quiet)
                .await
        );
    }

    /// `SPEC-0090` v2 AK-v2: content-modified rejections are asked again, other errors are not.
    #[tokio::test]
    async fn content_modified_is_asked_again() {
        let (client, mut fake) = pair();
        let (answer, ()) =
            tokio::join!(client.request("textDocument/references", json!(1)), async {
                for _ in 0..2 {
                    let r = fake.recv().await;
                    fake.send(json!({"jsonrpc": "2.0", "id": r["id"],
                    "error": {"code": -32801, "message": "content modified"}}))
                        .await;
                }
                let r = fake.recv().await;
                assert_eq!(r["method"], "textDocument/references");
                fake.send(json!({"jsonrpc": "2.0", "id": r["id"], "result": [1]}))
                    .await;
            });
        assert_eq!(answer.unwrap(), json!([1]));

        let (answer, ()) = tokio::join!(client.request("textDocument/hover", json!(2)), async {
            for _ in 0..RETRY_ATTEMPTS {
                let r = fake.recv().await;
                fake.send(json!({"jsonrpc": "2.0", "id": r["id"],
                    "error": {"code": -32802, "message": "cancelled"}}))
                    .await;
            }
        });
        assert!(matches!(
            answer,
            Err(LspError::Response { code: -32802, .. })
        ));

        let (answer, ()) = tokio::join!(client.request("textDocument/hover", json!(3)), async {
            let r = fake.recv().await;
            fake.send(json!({"jsonrpc": "2.0", "id": r["id"],
                "error": {"code": -32000, "message": "nope"}}))
                .await;
        });
        assert!(matches!(
            answer,
            Err(LspError::Response { code: -32000, .. })
        ));
    }

    #[tokio::test]
    async fn requests_notifications_and_server_requests() {
        let (mut client, mut fake) = pair();
        let client_ref = &client;
        let (a, b, ()) = tokio::join!(
            client_ref.request("a", json!(1)),
            client_ref.request("b", json!(2)),
            async {
                let first = fake.recv().await;
                let second = fake.recv().await;
                // Server-to-client request and a notification before answering out of order.
                fake.send(
                    json!({"jsonrpc": "2.0", "id": "s1", "method": "workspace/configuration",
                    "params": {"items": [{}, {}]}}),
                )
                .await;
                fake.send(json!({"jsonrpc": "2.0", "id": 7, "method": "custom/thing"}))
                    .await;
                fake.send(json!({"jsonrpc": "2.0", "method": "n/one", "params": 1}))
                    .await;
                fake.send(json!({"jsonrpc": "2.0", "method": "n/two", "params": 2}))
                    .await;
                fake.send(json!({"jsonrpc": "2.0", "id": second["id"], "error": {"code": -32000, "message": "nope"}})).await;
                fake.send(json!({"jsonrpc": "2.0", "id": first["id"], "result": "ok"}))
                    .await;
                assert_eq!(
                    fake.recv().await,
                    json!({"jsonrpc": "2.0", "id": "s1", "result": [null, null]})
                );
                let unknown = fake.recv().await;
                assert_eq!(unknown["id"], 7);
                assert_eq!(unknown["error"]["code"], -32601);
            }
        );
        assert_eq!(a.unwrap(), json!("ok"));
        assert_eq!(
            b.unwrap_err(),
            LspError::Response {
                method: "b".into(),
                code: -32000,
                message: "nope".into()
            }
        );
        let wait = Duration::from_secs(1);
        assert_eq!(
            client.next_notification(wait).await.unwrap().method,
            "n/one"
        );
        assert_eq!(
            client.next_notification(wait).await.unwrap().params,
            json!(2)
        );

        client.timeout = Duration::from_millis(50);
        let err = client
            .request("slow/method", Value::Null)
            .await
            .unwrap_err();
        assert_eq!(err, LspError::Timeout("slow/method".into(), 0));
        assert!(err.to_string().starts_with("slow/method: no answer"));
    }

    #[tokio::test]
    async fn server_exit_fails_pending_requests_with_stderr() {
        let (client, mut fake) = pair();
        let (result, ()) = tokio::join!(client.request("hang", Value::Null), async {
            fake.recv().await;
            fake.stderr
                .write_all(b"starting\nfatal: no compile database\n")
                .await
                .unwrap();
            drop(fake);
        });
        let err = result.unwrap_err();
        assert_eq!(
            err,
            LspError::Closed("starting\nfatal: no compile database".into())
        );
        assert!(client.request("later", Value::Null).await.is_err());
    }

    #[tokio::test]
    async fn diagnostics_of_one_file() {
        let (mut client, mut fake) = pair();
        let path = Path::new("/w/a.c");
        let server = tokio::spawn(async move {
            let open = fake.recv().await;
            assert_eq!(open["params"]["textDocument"]["languageId"], "c");
            fake.send(
                json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                "params": {"uri": "file:///w/other.c", "diagnostics": []}}),
            )
            .await;
            fake.send(json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                "params": {"uri": "file:///w/a.c", "diagnostics": [{"severity": 1, "message": "boom",
                "source": "clang", "code": "undeclared_var_use",
                "range": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 7}}}]}})).await;
            fake
        });
        let d = client
            .diagnostics(path, LanguageId::C, "int main(){}", Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(
            d,
            [Diagnostic {
                severity: "error".into(),
                start_line: 2,
                start_character: 4,
                end_line: 2,
                end_character: 7,
                message: "boom".into(),
                source: Some("clang".into()),
                code: Some("undeclared_var_use".into()),
                unnecessary: false,
            }]
        );
        drop(server.await.unwrap());
    }

    /// SPEC-0023/AK-9 (v5): several files over one connection: all are opened first, each gets
    /// its own first publication in the order asked whatever order they come in, and a file the
    /// server says nothing about is a timeout of its own.
    #[tokio::test]
    async fn diagnostics_of_several_files_over_one_connection() {
        let (mut client, mut fake) = pair();
        let server = tokio::spawn(async move {
            let mut opened = Vec::new();
            for _ in 0..3 {
                let open = fake.recv().await;
                assert_eq!(open["method"], "textDocument/didOpen");
                opened.push(
                    open["params"]["textDocument"]["uri"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                );
            }
            assert_eq!(opened, ["file:///w/a.c", "file:///w/b.c", "file:///w/c.c"]);
            fake.send(json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                "params": {"uri": "file:///w/b.c", "diagnostics": [{"severity": 2, "message": "b",
                "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}}}]}}))
            .await;
            fake.send(
                json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                "params": {"uri": "file:///w/a.c", "diagnostics": []}}),
            )
            .await;
            // A later list for a file already answered does not replace the first.
            fake.send(json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                "params": {"uri": "file:///w/a.c", "diagnostics": [{"severity": 1, "message": "late"}]}}))
            .await;
            fake
        });
        let files = [
            (PathBuf::from("/w/a.c"), LanguageId::C, "int a;".to_owned()),
            (PathBuf::from("/w/b.c"), LanguageId::C, "int b;".to_owned()),
            (PathBuf::from("/w/c.c"), LanguageId::C, "int c;".to_owned()),
        ];
        let found = client
            .diagnostics_of_files(&files, Duration::from_millis(400))
            .await;
        assert_eq!(found.len(), 3);
        assert_eq!(found[0], Ok(Vec::new()));
        let b = found[1].as_ref().unwrap();
        assert_eq!(
            (b.len(), b[0].severity.as_str(), b[0].start_line),
            (1, "warning", 1)
        );
        assert!(
            matches!(&found[2], Err(LspError::Timeout(method, _)) if method == "textDocument/publishDiagnostics"),
            "{:?}",
            found[2]
        );
        drop(server.await.unwrap());
    }

    /// SPEC-0293/AK-3: a request given up at its deadline or by a stop flag is withdrawn.
    #[tokio::test]
    async fn bounded_requests_are_withdrawn() {
        let (mut client, mut fake) = pair();
        client.timeout = Duration::from_secs(600);
        let (answer, ()) = tokio::join!(
            client.request_bounded(
                "textDocument/references",
                json!({}),
                Duration::from_millis(100),
                None
            ),
            async {
                let asked = fake.recv().await;
                let cancel = fake.recv().await;
                assert_eq!(cancel["method"], "$/cancelRequest");
                assert_eq!(cancel["params"]["id"], asked["id"]);
            }
        );
        assert_eq!(
            answer.unwrap_err(),
            LspError::Timeout("textDocument/references".into(), 0)
        );

        let stop = Arc::new(AtomicBool::new(false));
        let (answer, ()) = tokio::join!(
            client.request_bounded(
                "textDocument/references",
                json!({}),
                Duration::from_secs(600),
                Some(Arc::clone(&stop))
            ),
            async {
                let asked = fake.recv().await;
                stop.store(true, Ordering::SeqCst);
                let cancel = fake.recv().await;
                assert_eq!(cancel["method"], "$/cancelRequest");
                assert_eq!(cancel["params"]["id"], asked["id"]);
            }
        );
        assert_eq!(
            answer.unwrap_err(),
            LspError::Stopped("textDocument/references".into())
        );

        let (answer, ()) = tokio::join!(
            client.request_bounded("a", json!({}), Duration::from_secs(5), None),
            async {
                let asked = fake.recv().await;
                fake.send(json!({"jsonrpc": "2.0", "id": asked["id"], "result": [1]}))
                    .await;
            }
        );
        assert_eq!(answer.unwrap(), json!([1]));
    }
}
