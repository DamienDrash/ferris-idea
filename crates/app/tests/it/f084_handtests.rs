//! The steps of the F-084 hand tests that run the `idecraft` program itself, without a window:
//! hand test 202 step 8 (`idecraft mcp` over stdio without a grant), 204 step 6 (`idecraft quit`
//! without a running window) and 206 step 5 (`idecraft call … dialog_decisions`). Each run has a
//! settings folder of its own, so the user's rights, trust and windows never count. The window
//! steps of the same hand tests are window tests of the binary (`src/f084_handtests.rs`).
//!
//! `SPEC-0053` v8 behaviours 3 and 7 (202), `SPEC-0261` v9 behaviour 11 (204), `SPEC-0281` v2
//! behaviours 1 and 5 (206).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A project folder and a settings folder beside it.
fn folders(tag: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("idecraft-f084-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("handtest");
    let config = base.join("settings");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    (root.canonicalize().unwrap(), config.canonicalize().unwrap())
}

fn idecraft(config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_idecraft"));
    command
        .env("IDECRAFT_CONFIG_DIR", config)
        .env_remove("IDECRAFT_TRUST_PROJECTS");
    command
}

fn git(root: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@t.invalid"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
}

/// Hand test 202 step 8 (`SPEC-0053` v8 behaviours 3 and 7, AK-7): `idecraft mcp` over stdio
/// cannot ask; `git_create_branch` without `vcs-write` is refused at once with a message that
/// starts with "not granted:", names the right, the grant file and the entry to add, and says
/// neither "rejected" nor "denied". No branch is made.
#[test]
fn hand_test_202_stdio_without_a_grant_is_not_granted() {
    let (root, config) = folders("202");
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    // A grant file that gives this workspace other rights, not `vcs-write`.
    let grants = config.join("agent-permissions.toml");
    std::fs::write(
        &grants,
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"write\"]\n",
            root.display()
        ),
    )
    .unwrap();
    let mut child = idecraft(&config)
        .arg("mcp")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    for message in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "2"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "git_create_branch", "arguments": {"name": "recht"}}}),
    ] {
        writeln!(stdin, "{message}").unwrap();
    }
    stdin.flush().unwrap();
    let reply = loop {
        let line = lines.next().expect("an answer").unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == json!(2) {
            break v;
        }
    };
    drop(stdin);
    let _ = child.wait();
    let message = reply["error"]["message"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            (reply["result"]["isError"] == json!(true))
                .then(|| {
                    reply["result"]["content"][0]["text"]
                        .as_str()
                        .map(str::to_owned)
                })
                .flatten()
        })
        .unwrap_or_else(|| panic!("refused: {reply}"));
    assert!(message.starts_with("not granted:"), "{message}");
    assert!(message.contains("`vcs-write`"), "{message}");
    assert!(message.contains(&grants.display().to_string()), "{message}");
    assert!(
        message.contains(&format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"vcs-write\"]",
            root.display()
        )),
        "{message}"
    );
    assert!(message.contains("nobody was asked"), "{message}");
    assert!(
        !message.contains("rejected") && !message.contains("denied"),
        "{message}"
    );
    let branches = Command::new("git")
        .args(["branch", "--list", "recht"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(branches.stdout.is_empty(), "no branch was made");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// Hand test 204 step 6 (`SPEC-0261` v9 behaviour 11, AK-4): without a running window,
/// `idecraft quit` says so and exits with status 1.
#[test]
fn hand_test_204_quit_without_a_running_window() {
    let (root, config) = folders("204");
    let out = idecraft(&config).arg("quit").output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.starts_with("error: no running IDECraft window serves MCP over HTTP ("),
        "{err}"
    );
    assert!(out.stdout.is_empty());
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// Hand test 206 step 5 (`SPEC-0281` v2 behaviours 1, 3 and 5): `idecraft call … dialog_decisions
/// {"id": "delete-file"}` prints the Delete question as a native alert with the tool that makes
/// each decision.
#[test]
fn hand_test_206_the_delete_decision_from_a_shell() {
    let (root, config) = folders("206");
    let out = idecraft(&config)
        .arg("call")
        .arg(&root)
        .arg("dialog_decisions")
        .arg(r#"{"id": "delete-file"}"#)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    let dialogs = answer["dialogs"].as_array().unwrap();
    assert_eq!(dialogs.len(), 1, "{answer}");
    let delete = &dialogs[0];
    assert_eq!(delete["id"], "delete-file", "{delete}");
    assert_eq!(delete["presentation"], "native_alert", "{delete}");
    let decisions = delete["decisions"].as_array().unwrap();
    assert!(
        decisions.len() == 2 && decisions.iter().all(|d| d["tool"] == "delete_file"),
        "{delete}"
    );
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
