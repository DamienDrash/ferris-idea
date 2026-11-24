//! `SPEC-0358` acceptance criteria against the stand-ins.

use std::path::PathBuf;
use std::time::Duration;

use ferris_idea_claude_link::{
    Diagnostic, DiffDecision, OpenEditor, Position, Selection, Severity, lockfile,
};
use serde_json::{Value, json};

use crate::standin::{Client, Setup};

fn sel(path: PathBuf, text: &str) -> Selection {
    Selection {
        path,
        start: Position {
            line: 1,
            character: 2,
        },
        end: Position {
            line: 1,
            character: 2 + text.chars().count(),
        },
        text: text.into(),
    }
}

/// SPEC-0358 AK-1: the lock file names the process, the project, the IDE and the transport, holds
/// a fresh 256-bit token, and only the user can read it.
#[test]
fn lock_file_is_private_and_complete() {
    let s = Setup::new();
    let lock = s.lock();
    assert_eq!(lock["pid"], json!(std::process::id()));
    assert_eq!(lock["workspaceFolders"], json!([s.workspace]));
    assert_eq!(lock["ideName"], "Ferris IDEA");
    assert_eq!(lock["transport"], "ws");
    let token = s.token();
    assert_eq!(token.len(), 64);
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    let path = s.link().lock_file().unwrap().to_path_buf();
    assert_eq!(path, s.lock_dir.join(format!("{}.lock", s.link().port())));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "lock file mode {mode:o}");
        let dir = std::fs::metadata(&s.lock_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir, 0o700, "lock folder mode {dir:o}");
    }
    // A second link has another token and port.
    let other = Setup::new();
    assert_ne!(other.token(), token);
    // Only the lock file is in the folder: no temporary file stays.
    assert_eq!(std::fs::read_dir(&s.lock_dir).unwrap().count(), 1);
}

/// SPEC-0358 AK-2: stopping the link removes its lock file and closes the port.
#[test]
fn stopping_removes_the_lock_file() {
    let mut s = Setup::new();
    let path = s.link().lock_file().unwrap().to_path_buf();
    let port = s.link().port();
    assert!(path.exists());
    s.link = None;
    assert!(!path.exists());
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

/// SPEC-0358 AK-4: stale lock files of Ferris IDEA processes that ended are removed; those of other
/// IDEs, of running processes and of this process stay.
#[test]
fn only_stale_lock_files_of_ferris_idea_are_removed() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, ide: &str, pid: u32| {
        std::fs::write(
            dir.path().join(name),
            json!({"pid": pid, "ideName": ide, "workspaceFolders": [], "transport": "ws", "authToken": "x"}).to_string(),
        )
        .unwrap();
    };
    write("1001.lock", "Ferris IDEA", 11);
    write("1002.lock", "Visual Studio Code", 11);
    write("1003.lock", "Ferris IDEA", 22);
    write("1004.lock", "Ferris IDEA", 33);
    write("notes.txt", "Ferris IDEA", 11);
    let removed = lockfile::remove_stale_with(dir.path(), "Ferris IDEA", 33, |pid| pid == 22);
    assert_eq!(removed, 1);
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["1002.lock", "1003.lock", "1004.lock", "notes.txt"]);
}

/// SPEC-0358 AK-5: a connection without the token, with a wrong token, from a browser page or
/// under another host name is refused before any message.
#[test]
fn connections_without_the_right_token_are_refused() {
    let s = Setup::new();
    let port = s.link().port();
    let status = |r: Result<Client, tungstenite::Error>| match r {
        Err(tungstenite::Error::Http(resp)) => resp.status().as_u16(),
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => 101,
    };
    assert_eq!(status(Client::connect(port, None, &[])), 401);
    let mut wrong = s.token();
    wrong.replace_range(0..1, if wrong.starts_with('a') { "b" } else { "a" });
    assert_eq!(status(Client::connect(port, Some(&wrong), &[])), 401);
    assert_eq!(
        status(Client::connect(port, Some(&s.token()[..63]), &[])),
        401
    );
    assert_eq!(
        status(Client::connect(
            port,
            Some(&s.token()),
            &[("origin", "https://evil.example")]
        )),
        403
    );
    assert_eq!(
        status(Client::connect(
            port,
            Some(&s.token()),
            &[("host", "evil.example")]
        )),
        403
    );
    assert!(s.link().state().clients.is_empty());
    // The right token connects.
    assert_eq!(status(Client::connect(port, Some(&s.token()), &[])), 101);
}

/// SPEC-0358 AK-6: the handshake: `initialize` names Ferris IDEA, tools are listed, and requests
/// before `initialize` are refused.
#[test]
fn initialize_and_tools_list() {
    let s = Setup::new();
    let mut raw = Client::connect(s.link().port(), Some(&s.token()), &[]).unwrap();
    let early = raw.call("tools/list", json!({}));
    assert_eq!(early["error"]["code"], -32002);
    let init = raw.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": "claude-code", "version": "2.1"}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "Ferris IDEA");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    let tools = raw.call("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for wanted in [
        "openFile",
        "openDiff",
        "getCurrentSelection",
        "getLatestSelection",
        "getOpenEditors",
        "getWorkspaceFolders",
        "getDiagnostics",
        "checkDocumentDirty",
        "closeAllDiffTabs",
    ] {
        assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
    }
    assert!(!names.contains(&"executeCode"));
    assert_eq!(raw.call("ping", json!({}))["result"], json!({}));
    assert_eq!(raw.call("nope", json!({}))["error"]["code"], -32601);
    assert_eq!(
        raw.tool("nope", json!({}))["error"]["code"],
        -32601,
        "an unknown tool"
    );
    let old = raw.call(
        "initialize",
        json!({"protocolVersion": "1999-01-01", "clientInfo": {"name": "claude-code"}}),
    );
    assert_eq!(old["result"]["protocolVersion"], "2024-11-05");
}

/// SPEC-0358 AK-7: the selection: asked, pushed as `selection_changed` once per change, and the
/// latest one kept.
#[test]
fn selection_is_answered_and_pushed() {
    let s = Setup::new();
    let mut c = s.client();
    let none = c.tool_json("getCurrentSelection", json!({}));
    assert_eq!(none["success"], false);
    assert_eq!(
        c.tool_json("getLatestSelection", json!({}))["success"],
        false
    );
    let file = s.workspace.join("src/main.rs");
    *s.window.selection.lock().unwrap() = Some(sel(file.clone(), "hello"));
    let now = c.tool_json("getCurrentSelection", json!({}));
    assert_eq!(now["success"], true);
    assert_eq!(now["text"], "hello");
    assert_eq!(now["filePath"], json!(file));
    assert_eq!(
        now["selection"]["start"],
        json!({"line": 1, "character": 2})
    );
    assert_eq!(now["selection"]["end"], json!({"line": 1, "character": 7}));
    assert_eq!(now["selection"]["isEmpty"], false);
    assert!(now["fileUrl"].as_str().unwrap().starts_with("file:///"));

    s.link().notify_selection(Some(sel(file.clone(), "world")));
    s.link().notify_selection(Some(sel(file.clone(), "world")));
    s.link().notify_selection(Some(sel(file.clone(), "again")));
    let first = c.read();
    assert_eq!(first["method"], "selection_changed");
    assert_eq!(first["params"]["text"], "world");
    let second = c.read();
    assert_eq!(second["params"]["text"], "again", "the repeat is not sent");
    let latest = c.tool_json("getLatestSelection", json!({}));
    assert_eq!(latest["text"], "again");
}

/// SPEC-0358 AK-8: open editors, the project folder, diagnostics and the dirty state.
#[test]
fn editors_folders_and_diagnostics() {
    let s = Setup::new();
    let mut c = s.client();
    let a = s.workspace.join("src/a.rs");
    let b = s.workspace.join("src/b.py");
    *s.window.editors.lock().unwrap() = vec![
        OpenEditor {
            path: a.clone(),
            active: true,
            dirty: false,
        },
        OpenEditor {
            path: b.clone(),
            active: false,
            dirty: true,
        },
    ];
    let tabs = c.tool_json("getOpenEditors", json!({}));
    assert_eq!(tabs["tabs"][0]["label"], "a.rs");
    assert_eq!(tabs["tabs"][0]["languageId"], "rust");
    assert_eq!(tabs["tabs"][0]["isActive"], true);
    assert_eq!(tabs["tabs"][1]["isDirty"], true);
    assert_eq!(tabs["tabs"][1]["languageId"], "python");

    let folders = c.tool_json("getWorkspaceFolders", json!({}));
    assert_eq!(folders["success"], true);
    assert_eq!(folders["rootPath"], json!(s.workspace));
    assert_eq!(folders["folders"][0]["name"], "project");

    let diag = |path: &PathBuf, sev, msg: &str| Diagnostic {
        path: path.clone(),
        start: Position {
            line: 3,
            character: 4,
        },
        end: Position {
            line: 3,
            character: 9,
        },
        severity: sev,
        message: msg.into(),
        source: Some("rustc".into()),
    };
    *s.window.diagnostics.lock().unwrap() = vec![
        diag(&a, Severity::Error, "mismatched types"),
        diag(&a, Severity::Warning, "unused"),
        diag(&b, Severity::Hint, "hint"),
    ];
    let all = c.tool_json("getDiagnostics", json!({}));
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert_eq!(all[0]["diagnostics"].as_array().unwrap().len(), 2);
    assert_eq!(all[0]["diagnostics"][0]["severity"], "Error");
    assert_eq!(
        all[0]["diagnostics"][0]["range"]["end"],
        json!({"line": 3, "character": 9})
    );
    let uri = all[1]["uri"].as_str().unwrap().to_owned();
    let one = c.tool_json("getDiagnostics", json!({"uri": uri}));
    assert_eq!(one.as_array().unwrap().len(), 1);
    assert_eq!(one[0]["diagnostics"][0]["severity"], "Hint");

    let dirty = c.tool_json("checkDocumentDirty", json!({"filePath": b}));
    assert_eq!(dirty["isDirty"], true);
    let closed = c.tool_json(
        "checkDocumentDirty",
        json!({"filePath": s.workspace.join("src/c.rs")}),
    );
    assert_eq!(closed["success"], false);
}

/// SPEC-0358 AK-9: files are opened only inside the project; the selection asked for is handed to
/// the window.
#[test]
fn open_file_inside_the_project_only() {
    let s = Setup::new();
    let mut c = s.client();
    let file = s.workspace.join("src/main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let opened = c.tool_text(
        "openFile",
        json!({"filePath": file, "startText": "fn", "endText": "}"}),
    );
    assert_eq!(opened, format!("Opened file: {}", file.display()));
    let request = s.window.opened.lock().unwrap()[0].clone();
    assert_eq!(request.start_text.as_deref(), Some("fn"));
    assert_eq!(request.end_text.as_deref(), Some("}"));
    assert!(request.focus);
    let quiet = c.tool_json(
        "openFile",
        json!({"filePath": file, "makeFrontmost": false}),
    );
    assert_eq!(quiet["lineCount"], 1);
    assert_eq!(quiet["languageId"], "rust");

    let outside = s.dir.path().join("secret.txt");
    std::fs::write(&outside, "x").unwrap();
    let refused = c.tool("openFile", json!({"filePath": outside}));
    assert_eq!(refused["error"]["code"], -32602, "{refused}");
    let dots = c.tool(
        "openFile",
        json!({"filePath": format!("{}/../secret.txt", s.workspace.display())}),
    );
    assert_eq!(dots["error"]["code"], -32602);
    let missing = c.tool("openFile", json!({"filePath": s.workspace.join("nope.rs")}));
    assert_eq!(missing["error"]["code"], -32000);
    assert_eq!(s.window.opened.lock().unwrap().len(), 2);
}

/// SPEC-0358 AK-10: an accepted diff answers `FILE_SAVED` with the accepted text for Claude Code to
/// write; a rejected one answers `DIFF_REJECTED`; the link itself writes nothing.
#[test]
fn diff_accept_hands_back_the_text_and_reject_leaves_the_file() {
    let s = Setup::new();
    let mut c = s.client();
    let file = s.workspace.join("src/lib.rs");
    std::fs::write(&file, "old\n").unwrap();
    let args = |tab: &str, text: &str| json!({"old_file_path": file, "new_file_path": file, "new_file_contents": text, "tab_name": tab});

    // Rejected: the file stays.
    let id = c.start(
        "tools/call",
        json!({"name": "openDiff", "arguments": args("t1", "new\n")}),
    );
    let (proposal, reply) = s.window.next_diff();
    assert_eq!(proposal.old_text, "old\n");
    assert_eq!(proposal.new_text, "new\n");
    assert_eq!(proposal.tab_name, "t1");
    assert!(!proposal.new_file);
    assert_eq!(s.link().state().pending_diffs, 1);
    reply.send(DiffDecision::Rejected).unwrap();
    let answer = c.answer(id);
    assert_eq!(answer["result"]["content"][0]["text"], "DIFF_REJECTED");
    assert_eq!(answer["result"]["content"][1]["text"], "t1");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "old\n");
    assert_eq!(s.link().state().pending_diffs, 0);

    // Accepted: the answer hands Claude Code the accepted text, which it writes itself; the link
    // writes nothing (a write here would make Claude Code's own write fail as stale).
    let id = c.start(
        "tools/call",
        json!({"name": "openDiff", "arguments": args("t2", "new\n")}),
    );
    let (_, reply) = s.window.next_diff();
    reply.send(DiffDecision::Accepted("new\n".into())).unwrap();
    let answer = c.answer(id);
    assert_eq!(answer["result"]["content"][0]["text"], "FILE_SAVED");
    assert_eq!(answer["result"]["content"][1]["text"], "new\n");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "old\n");

    // A proposed new file is shown as new; rejecting it creates nothing.
    let new = s.workspace.join("src/deep/new.rs");
    let id = c.start(
        "tools/call",
        json!({"name": "openDiff", "arguments": {"old_file_path": new, "new_file_path": new, "new_file_contents": "x", "tab_name": "t4"}}),
    );
    let (proposal, reply) = s.window.next_diff();
    assert!(proposal.new_file);
    assert_eq!(proposal.old_text, "");
    reply.send(DiffDecision::Rejected).unwrap();
    c.answer(id);
    assert!(!new.exists());
    assert!(!s.workspace.join("src/deep").exists());

    // Outside the project: refused before anything is shown.
    let outside = s.dir.path().join("evil.rs");
    let refused = c.tool(
        "openDiff",
        json!({"old_file_path": outside, "new_file_path": outside, "new_file_contents": "x", "tab_name": "t5"}),
    );
    assert_eq!(refused["error"]["code"], -32602);
    assert!(!outside.exists());
    assert!(s.window.diffs.lock().unwrap().is_empty());
}

/// SPEC-0358 AK-11: `close_tab`, `closeAllDiffTabs`, a replaced diff and a client that leaves all
/// answer the waiting diff as rejected and close it; nothing is written.
#[test]
fn closed_diffs_are_rejected() {
    let s = Setup::new();
    let mut c = s.client();
    let file = s.workspace.join("a.txt");
    std::fs::write(&file, "a").unwrap();
    let args = |tab: &str| json!({"name": "openDiff", "arguments": {"old_file_path": file, "new_file_path": file, "new_file_contents": "b", "tab_name": tab}});
    let id = c.start("tools/call", args("x"));
    s.window.next_diff();
    let mut other = s.client();
    assert_eq!(
        other.tool_text("close_tab", json!({"tab_name": "x"})),
        "TAB_CLOSED"
    );
    assert_eq!(
        c.answer(id)["result"]["content"][0]["text"],
        "DIFF_REJECTED"
    );
    assert_eq!(s.window.closed.lock().unwrap().as_slice(), ["x"]);

    let first = c.start("tools/call", args("y"));
    s.window.next_diff();
    let second = c.start("tools/call", args("y"));
    s.window.next_diff();
    assert_eq!(
        c.answer(first)["result"]["content"][0]["text"],
        "DIFF_REJECTED"
    );
    assert_eq!(
        other.tool_text("closeAllDiffTabs", json!({})),
        "CLOSED_1_DIFF_TABS"
    );
    assert_eq!(
        c.answer(second)["result"]["content"][0]["text"],
        "DIFF_REJECTED"
    );

    // The client leaves while its diff waits.
    c.start("tools/call", args("z"));
    let (_, reply) = s.window.next_diff();
    drop(c);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !s.window.closed.lock().unwrap().contains(&"z".to_owned()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the diff was not closed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // A late acceptance writes nothing.
    let _ = reply.send(DiffDecision::Accepted("late".into()));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "a");
}

/// SPEC-0358 AK-12: no tool writes without an accepted diff: saving is declined.
#[test]
fn save_document_writes_nothing() {
    let s = Setup::new();
    let mut c = s.client();
    let file = s.workspace.join("a.txt");
    std::fs::write(&file, "a").unwrap();
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    let saved = c.tool_json("saveDocument", json!({"filePath": file}));
    assert_eq!(saved["success"], false);
    assert_eq!(saved["saved"], false);
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        before
    );
}

/// SPEC-0358 AK-13: the state names the port, the clients and the lock file, never the token.
#[test]
fn state_reports_clients_without_the_token() {
    let s = Setup::new();
    assert!(!s.link().connected());
    let _c = s.client();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let state = loop {
        let state = s.link().state();
        if state.clients.first().is_some_and(|c| c.pid.is_some()) {
            break state;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(s.link().connected());
    assert!(state.listening);
    assert_eq!(state.port, s.link().port());
    assert_eq!(state.clients[0].name, "claude-code");
    assert_eq!(state.clients[0].version, "9.9.9");
    assert_eq!(state.clients[0].pid, Some(4242));
    let text: Value = serde_json::to_value(&state).unwrap();
    assert!(!text.to_string().contains(&s.token()));
    assert!(s.window.changes.load(std::sync::atomic::Ordering::SeqCst) >= 1);
}

/// SPEC-0358 AK-5a: the subprotocol the client offers (`mcp`) is named in the answer; Claude Code
/// refuses a server that names none.
#[test]
fn the_offered_subprotocol_is_answered() {
    let s = Setup::new();
    let c = Client::connect(
        s.link().port(),
        Some(&s.token()),
        &[("sec-websocket-protocol", "mcp")],
    )
    .unwrap();
    assert_eq!(
        c.protocol.as_deref(),
        Some("mcp"),
        "the answer names the subprotocol"
    );
    let plain = Client::connect(s.link().port(), Some(&s.token()), &[]).unwrap();
    assert_eq!(plain.protocol, None);
}
