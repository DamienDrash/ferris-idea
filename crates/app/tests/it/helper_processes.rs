//! SPEC-0263 AK-1: a language server that ignores `exit` and outlives its closed input is still gone
//! within 3 s after the MCP client of `ferris-idea mcp` disconnects, and it was told Ferris IDEA's process
//! id. The server is a small script standing in for `clangd` on the `PATH`.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Answers just enough LSP for diagnostics, writes `<pid> <processId>` to `$STUBBORN_PIDS`, ignores
/// `exit` and `SIGTERM` and keeps running after its input closes.
const STUBBORN_SERVER: &str = r#"#!/usr/bin/env python3
import json, os, signal, sys, time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
inp, out = sys.stdin.buffer, sys.stdout.buffer
def send(msg):
    body = json.dumps(msg).encode()
    out.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    out.flush()
while True:
    length = None
    while True:
        line = inp.readline()
        if not line:
            time.sleep(60)
            sys.exit(0)
        if line in (b"\r\n", b"\n"):
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":")[1])
    msg = json.loads(inp.read(length))
    method = msg.get("method")
    if method == "initialize":
        with open(os.environ["STUBBORN_PIDS"], "w") as f:
            f.write("%d %s" % (os.getpid(), msg["params"].get("processId")))
        send({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {"textDocumentSync": 1}}})
    elif method == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
              "params": {"uri": uri, "diagnostics": []}})
    elif "id" in msg:
        send({"jsonrpc": "2.0", "id": msg["id"], "result": None})
"#;

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn send(stdin: &mut impl Write, message: Value) {
    writeln!(stdin, "{message}").unwrap();
    stdin.flush().unwrap();
}

/// Reads messages until the response with `id`.
fn response(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: u64) -> Value {
    for line in lines {
        let message: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if message["id"] == id {
            return message;
        }
    }
    panic!("no response {id}");
}

#[test]
fn language_server_ends_with_the_mcp_session() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("skipped: no python3");
        return;
    }
    let base = std::env::temp_dir().join(format!("ferris-idea-helpers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let bin = base.join("bin");
    let project = base.join("project");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("main.c"), "int main(void) { return 0; }\n").unwrap();
    let server = bin.join("clangd");
    std::fs::write(&server, STUBBORN_SERVER).unwrap();
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
    let pids = base.join("pids");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mut mcp = Command::new(env!("CARGO_BIN_EXE_ferris-idea"))
        .arg("mcp")
        .arg(&project)
        .env("PATH", path)
        .env("STUBBORN_PIDS", &pids)
        // The test project is trusted: its servers start as in everyday use (SPEC-0278).
        .env("FERRIS_IDEA_TRUST_PROJECTS", "all")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = mcp.stdin.take().unwrap();
    let mut lines = BufReader::new(mcp.stdout.take().unwrap()).lines();
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}),
    );
    response(&mut lines, 1);
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "file_diagnostics", "arguments": {"path": "main.c", "timeout_seconds": 10}}}),
    );
    let answer = response(&mut lines, 2);
    assert!(answer["error"].is_null(), "{answer}");

    let text = std::fs::read_to_string(&pids).expect("the server was started");
    let (pid, told) = text.split_once(' ').unwrap();
    let pid: u32 = pid.parse().unwrap();
    assert_eq!(told, mcp.id().to_string(), "processId in initialize");
    assert!(alive(pid));

    // The client disconnects: the MCP process ends and takes the server with it.
    drop(stdin);
    let start = Instant::now();
    while mcp.try_wait().unwrap().is_none() {
        assert!(start.elapsed() < Duration::from_secs(10), "MCP did not end");
        std::thread::sleep(Duration::from_millis(50));
    }
    let ended = Instant::now();
    while alive(pid) {
        if ended.elapsed() > Duration::from_secs(3) {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            panic!("the language server outlived the MCP session by more than 3 s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&base);
}
