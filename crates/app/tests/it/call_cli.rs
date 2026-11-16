//! SPEC-0281 AK-3: `idecraft call <folder> <tool> [<json>]` makes one MCP tool call from a shell —
//! the decisions of IDECraft's dialogs are reachable without a window and without an MCP client.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-call-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.txt"), "a\n").unwrap();
    std::fs::write(dir.join("src/b.txt"), "b\n").unwrap();
    dir
}

/// Runs `idecraft` with a settings folder of its own, so the user's rights and trust never count.
fn idecraft(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_idecraft"))
        .args(args)
        .env("IDECRAFT_CONFIG_DIR", config)
        .env_remove("IDECRAFT_TRUST_PROJECTS")
        .output()
        .unwrap()
}

fn json(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn call_prints_a_tools_answer_and_makes_a_dialogs_decision() {
    let dir = folder("ok");
    let config = dir.with_extension("config");
    let d = dir.to_str().unwrap();

    // The list of dialogs, from a shell.
    let list = json(&idecraft(&config, &["call", d, "dialog_decisions"]));
    let ids: Vec<&str> = list["dialogs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert!(ids.contains(&"delete-file") && ids.contains(&"trust-project"));

    // The Delete question (dry run) and then the decision itself.
    let asked = json(&idecraft(
        &config,
        &["call", d, "delete_file", r#"{"path": "src/a.txt"}"#],
    ));
    assert_eq!(asked["question"], "Delete file \"a.txt\"?");
    assert!(dir.join("src/a.txt").exists());
    let delete = idecraft(
        &config,
        &[
            "call",
            d,
            "delete_file",
            r#"{"path": "src/a.txt", "dry_run": false}"#,
        ],
    );
    if delete.status.success() {
        assert_eq!(json(&delete)["deleted"], Value::Bool(true));
        assert!(!dir.join("src/a.txt").exists());
    } else {
        // Refused for lack of a right the user did not grant: nothing happened.
        assert_eq!(delete.status.code(), Some(1));
        assert!(dir.join("src/a.txt").exists());
    }

    // Trust is refused without the user's right: the decision stays the user's.
    let refused = idecraft(
        &config,
        &["call", d, "trust_project", r#"{"dry_run": false}"#],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).starts_with("error: "));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&config);
}

#[test]
fn call_reports_errors_and_wrong_usage() {
    let dir = folder("err");
    let config = dir.with_extension("config");
    let d = dir.to_str().unwrap();

    let unknown = idecraft(&config, &["call", d, "no_such_tool"]);
    assert_eq!(unknown.status.code(), Some(1));
    let refused = idecraft(
        &config,
        &["call", d, "dialog_decisions", r#"{"id": "nope"}"#],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("no dialog nope"));

    let not_object = idecraft(&config, &["call", d, "dialog_decisions", "[1]"]);
    assert_eq!(not_object.status.code(), Some(2));
    let missing = idecraft(
        &config,
        &["call", "/nonexistent/idecraft", "workspace_info"],
    );
    assert_eq!(missing.status.code(), Some(2));
    let usage = idecraft(&config, &["call", d]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&usage.stderr).contains("idecraft call <directory> <tool>"));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&config);
}
