//! The tools Claude Code calls (`SPEC-0358` behaviours 12–22). Each answers a `tools/call` with
//! MCP text content; the texts and JSON shapes are the ones Claude Code expects from an IDE.
//! None of them writes a file (behaviour 21): Claude Code writes a change after the person
//! accepted its diff.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};

use crate::editor::{DiffDecision, DiffProposal, OpenRequest};
use crate::paths::{MAX_TEXT_BYTES, from_wire, inside, read_text, to_uri};
use crate::server::{PendingDiff, Shared, lock, selection_json};

/// How long a question to the window may take (behaviour 11).
pub const WINDOW_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest diff name taken.
const MAX_TAB_NAME: usize = 200;

type Answer = Result<Value, (i64, String)>;

fn invalid(m: impl Into<String>) -> (i64, String) {
    (-32602, m.into())
}

fn failed(m: impl Into<String>) -> (i64, String) {
    (-32000, m.into())
}

fn text(t: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": t.into()}]})
}

fn json_text(v: &Value) -> Value {
    text(v.to_string())
}

/// The tools `tools/list` names. `close_tab` is answered but not listed, as Claude Code calls it
/// itself (behaviour 12).
pub fn list() -> Value {
    let schema = |props: Value, required: &[&str]| json!({"type": "object", "properties": props, "required": required, "additionalProperties": false});
    let s = |d: &str| json!({"type": "string", "description": d});
    let b = |d: &str| json!({"type": "boolean", "description": d});
    let n = |d: &str| json!({"type": "integer", "description": d});
    json!([
        {"name": "openFile", "description": "Open a file of the project in the Ferris IDEA editor and optionally select a range of text",
         "inputSchema": schema(json!({
            "filePath": s("Absolute path of the file to open"),
            "preview": b("Ignored: Ferris IDEA has no preview tabs"),
            "startText": s("Select from the first occurrence of this text"),
            "endText": s("Select up to the end of the first occurrence of this text after startText"),
            "selectToEndOfLine": b("Extend the selection to the end of the line endText ends on"),
            "startLine": n("1-based first line to select when no text is given"),
            "endLine": n("1-based last line to select"),
            "makeFrontmost": b("Bring the editor to the front (default true)")}), &["filePath"])},
        {"name": "openDiff", "description": "Open a diff of a proposed change to a project file in Ferris IDEA; blocks until the user accepts or rejects it",
         "inputSchema": schema(json!({
            "old_file_path": s("Path of the file as it is"),
            "new_file_path": s("Path of the file the change is for"),
            "new_file_contents": s("The proposed text of the file"),
            "tab_name": s("Name of the diff")}), &["old_file_path", "new_file_path", "new_file_contents", "tab_name"])},
        {"name": "getCurrentSelection", "description": "Get the selection of the active editor", "inputSchema": schema(json!({}), &[])},
        {"name": "getLatestSelection", "description": "Get the latest selection made in any editor", "inputSchema": schema(json!({}), &[])},
        {"name": "getOpenEditors", "description": "Get the files open in editor tabs", "inputSchema": schema(json!({}), &[])},
        {"name": "getWorkspaceFolders", "description": "Get the project folder of this window", "inputSchema": schema(json!({}), &[])},
        {"name": "getDiagnostics", "description": "Get the errors and warnings of a file, or of every open file",
         "inputSchema": schema(json!({"uri": s("file:// URI or absolute path; all open files when left out")}), &[])},
        {"name": "checkDocumentDirty", "description": "Whether an open file has unsaved changes",
         "inputSchema": schema(json!({"filePath": s("Absolute path of the file")}), &["filePath"])},
        {"name": "saveDocument", "description": "Not offered: saving is left to the user",
         "inputSchema": schema(json!({"filePath": s("Absolute path of the file")}), &["filePath"])},
        {"name": "closeAllDiffTabs", "description": "Close every diff this link opened", "inputSchema": schema(json!({}), &[])},
    ])
}

/// Answers one `tools/call`.
pub(crate) fn call(shared: &Shared, client: u64, params: &Value) -> Answer {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| invalid("`name` must be a string"))?;
    let empty = serde_json::Map::new();
    let args = match &params["arguments"] {
        Value::Null => &empty,
        Value::Object(o) => o,
        _ => return Err(invalid("`arguments` must be an object")),
    };
    let ws = shared.workspace.clone();
    match name {
        "getCurrentSelection" => {
            let selection = ask(|r| shared.editor.selection(ws.clone(), r))?;
            Ok(json_text(&match selection {
                Some(s) => {
                    *lock(&shared.latest) = Some(s.clone());
                    with_success(selection_json(&s))
                }
                None => json!({"success": false, "message": "No active editor found"}),
            }))
        }
        "getLatestSelection" => Ok(json_text(&match lock(&shared.latest).clone() {
            Some(s) => with_success(selection_json(&s)),
            None => json!({"success": false, "message": "No selection available"}),
        })),
        "getOpenEditors" => {
            let editors = ask(|r| shared.editor.open_editors(ws.clone(), r))?;
            let tabs: Vec<Value> = editors
                .iter()
                .map(|e| {
                    json!({
                        "uri": to_uri(&e.path),
                        "isActive": e.active,
                        "label": e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        "languageId": language_id(&e.path),
                        "isDirty": e.dirty,
                    })
                })
                .collect();
            Ok(json_text(&json!({"tabs": tabs})))
        }
        "getWorkspaceFolders" => {
            let name = ws
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(json_text(&json!({
                "success": true,
                "folders": [{"name": name, "uri": to_uri(&ws), "path": ws}],
                "rootPath": ws,
            })))
        }
        "getDiagnostics" => {
            let file = match args.get("uri").and_then(Value::as_str) {
                Some(raw) if !raw.is_empty() => {
                    Some(inside(&ws, &from_wire(raw).map_err(invalid)?).map_err(invalid)?)
                }
                _ => None,
            };
            let found = ask(|r| shared.editor.diagnostics(ws.clone(), file.clone(), r))?;
            let mut files: Vec<(PathBuf, Vec<Value>)> = Vec::new();
            for d in found {
                let entry = json!({
                    "message": d.message,
                    "severity": d.severity.word(),
                    "range": {
                        "start": {"line": d.start.line, "character": d.start.character},
                        "end": {"line": d.end.line, "character": d.end.character},
                    },
                    "source": d.source,
                });
                match files.iter_mut().find(|(p, _)| *p == d.path) {
                    Some((_, list)) => list.push(entry),
                    None => files.push((d.path, vec![entry])),
                }
            }
            if let Some(f) = &file
                && files.is_empty()
            {
                files.push((f.clone(), Vec::new()));
            }
            let out: Vec<Value> = files
                .into_iter()
                .map(|(p, d)| json!({"uri": to_uri(&p), "diagnostics": d}))
                .collect();
            Ok(json_text(&Value::Array(out)))
        }
        "checkDocumentDirty" => {
            let path = path_arg(&ws, args, "filePath")?;
            let editors = ask(|r| shared.editor.open_editors(ws.clone(), r))?;
            Ok(json_text(&match editors.iter().find(|e| e.path == path) {
                Some(e) => {
                    json!({"success": true, "filePath": path, "isDirty": e.dirty, "isUntitled": false})
                }
                None => {
                    json!({"success": false, "message": format!("Document not open: {}", path.display())})
                }
            }))
        }
        // Saving writes the user's own unsaved text: left to the user (behaviour 21).
        "saveDocument" => {
            let path = path_arg(&ws, args, "filePath")?;
            Ok(json_text(&json!({
                "success": false, "filePath": path, "saved": false,
                "message": "Ferris IDEA leaves saving to the user",
            })))
        }
        "openFile" => open_file(shared, args),
        "openDiff" => open_diff(shared, client, args),
        "close_tab" => {
            let tab = args
                .get("tab_name")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("Missing required parameter: tab_name"))?;
            close(shared, tab);
            Ok(text("TAB_CLOSED"))
        }
        "closeAllDiffTabs" => {
            let names: Vec<String> = lock(&shared.diffs).keys().cloned().collect();
            for n in &names {
                close(shared, n);
            }
            Ok(text(format!("CLOSED_{}_DIFF_TABS", names.len())))
        }
        _ => Err((-32601, format!("Tool not found: {name}"))),
    }
}

fn with_success(mut v: Value) -> Value {
    v["success"] = json!(true);
    v
}

/// Asks the window and waits at most [`WINDOW_TIMEOUT`] (behaviour 11).
fn ask<T>(send: impl FnOnce(std::sync::mpsc::Sender<T>)) -> Result<T, (i64, String)> {
    let (tx, rx) = channel();
    send(tx);
    wait(&rx)
}

fn wait<T>(rx: &Receiver<T>) -> Result<T, (i64, String)> {
    rx.recv_timeout(WINDOW_TIMEOUT)
        .map_err(|_| failed("the Ferris IDEA window did not answer"))
}

fn path_arg(
    ws: &Path,
    args: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<PathBuf, (i64, String)> {
    let raw = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("Missing required parameter: {key}")))?;
    inside(ws, &from_wire(raw).map_err(invalid)?).map_err(invalid)
}

fn open_file(shared: &Shared, args: &serde_json::Map<String, Value>) -> Answer {
    let ws = shared.workspace.clone();
    let path = path_arg(&ws, args, "filePath")?;
    if !path.is_file() {
        return Err(failed(format!("File not found: {}", path.display())));
    }
    let string = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let line = |k: &str| {
        args.get(k)
            .and_then(Value::as_u64)
            .and_then(|l| usize::try_from(l).ok())
            .filter(|l| *l > 0)
    };
    let focus = args
        .get("makeFrontmost")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let request = OpenRequest {
        path: path.clone(),
        start_text: string("startText"),
        end_text: string("endText"),
        select_to_end_of_line: args
            .get("selectToEndOfLine")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        start_line: line("startLine"),
        end_line: line("endLine"),
        focus,
    };
    ask(|r| shared.editor.open_file(ws.clone(), request, r))?.map_err(failed)?;
    if focus {
        return Ok(text(format!("Opened file: {}", path.display())));
    }
    let lines = read_text(&path)
        .ok()
        .flatten()
        .map_or(0, |t| t.lines().count());
    Ok(json_text(&json!({
        "success": true,
        "filePath": path,
        "languageId": language_id(&path),
        "lineCount": lines,
    })))
}

/// Answers a waiting diff as rejected and closes it in the window (behaviour 19).
fn close(shared: &Shared, tab: &str) {
    if let Some(d) = lock(&shared.diffs).remove(tab) {
        let _ = d.decide.send(DiffDecision::Rejected);
    }
    shared
        .editor
        .close_diff(shared.workspace.clone(), tab.to_owned());
}

fn open_diff(shared: &Shared, client: u64, args: &serde_json::Map<String, Value>) -> Answer {
    let ws = shared.workspace.clone();
    let get = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("Missing required parameter: {k}")))
    };
    let tab_name = get("tab_name")?;
    if tab_name.is_empty()
        || tab_name.chars().count() > MAX_TAB_NAME
        || tab_name.chars().any(char::is_control)
    {
        return Err(invalid("tab_name must be 1 to 200 printable characters"));
    }
    let new_text = get("new_file_contents")?;
    if new_text.len() > MAX_TEXT_BYTES {
        return Err(invalid("new_file_contents is too large"));
    }
    let path = path_arg(&ws, args, "new_file_path")?;
    let old = path_arg(&ws, args, "old_file_path")?;
    let old_text = read_text(&old).map_err(failed)?;
    let new_file = old_text.is_none() && !path.exists();
    let proposal = DiffProposal {
        tab_name: tab_name.to_owned(),
        path: path.clone(),
        old_text: old_text.unwrap_or_default(),
        new_text: new_text.to_owned(),
        new_file,
    };
    let (decide, decided) = channel();
    let id = shared.next_id.fetch_add(1, Ordering::SeqCst);
    // A diff of the same name is replaced: the earlier call is answered as rejected.
    if let Some(earlier) = lock(&shared.diffs).insert(
        tab_name.to_owned(),
        PendingDiff {
            id,
            client,
            decide: decide.clone(),
        },
    ) {
        let _ = earlier.decide.send(DiffDecision::Rejected);
    }
    shared.editor.show_diff(ws.clone(), proposal, decide);
    let decision = loop {
        match decided.recv_timeout(Duration::from_millis(200)) {
            Ok(d) => break d,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if shared.stopping.load(Ordering::SeqCst) {
                    break DiffDecision::Rejected;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break DiffDecision::Rejected,
        }
    };
    {
        let mut diffs = lock(&shared.diffs);
        if diffs.get(tab_name).is_some_and(|d| d.id == id) {
            diffs.remove(tab_name);
        }
    }
    match decision {
        DiffDecision::Rejected => Ok(json!({"content": [
            {"type": "text", "text": "DIFF_REJECTED"},
            {"type": "text", "text": tab_name},
        ]})),
        DiffDecision::Accepted(accepted) => {
            if shared.stopping.load(Ordering::SeqCst) {
                return Err(failed("the link stopped"));
            }
            if accepted.len() > MAX_TEXT_BYTES {
                return Err(failed("the accepted text is too large"));
            }
            // Claude Code writes the accepted text itself; a write here first would make its own
            // write fail as "changed since it was last read" (behaviour 18, hand test).
            Ok(json!({"content": [
                {"type": "text", "text": "FILE_SAVED"},
                {"type": "text", "text": accepted},
            ]}))
        }
    }
}

/// A language id from the file's extension, as editors name them.
pub fn language_id(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "jsx" => "javascriptreact",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "sql" => "sql",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" | "zsh" => "shellscript",
        "xml" => "xml",
        _ => "plaintext",
    }
}
