//! F-077 · Run configurations: start programs and tests with parameters (SPEC-0047 AK-1…AK-5,
//! SPEC-0053 AK-3/AK-4, IMP-0023).

use crate::harness::*;
use serde_json::json;

const RUN_TOML: &str = r#"[[run]]
name = "prepare"
command = ["sh", "-c", "echo prepared > prepared.txt"]

[[run]]
name = "greet"
command = ["sh", "-c", "cat prepared.txt; echo hello $WHO"]
env = { WHO = "world" }
before = ["prepare"]
"#;

#[test]
fn configurations_are_listed_and_previewed_without_running() {
    let p = Project::empty();
    p.write(".idecraft/run.toml", RUN_TOML);
    let mut ide = Ide::stdio(&p);
    let list = ide.ok("run_configurations", json!({}));
    assert_eq!(list["problems"], json!([]), "{list}");
    assert_eq!(list["configs"].as_array().unwrap().len(), 2);
    let preview = ide.ok("run_configuration", json!({"name": "greet"}));
    assert!(text(&preview).contains("prepare"), "the chain: {preview}");
    ide.refused(
        "run_configuration",
        json!({"name": "greet", "dry_run": false}),
    );
    assert!(!p.path("prepared.txt").exists());
}

/// SPEC-0278 behaviours 5, 10–14: an untrusted project runs nothing, even with the `run` right;
/// `idecraft trust add` trusts it, `idecraft trust remove` takes trust away.
#[test]
fn an_untrusted_project_runs_nothing_until_trusted() {
    let p = Project::empty();
    p.write(".idecraft/run.toml", RUN_TOML).grant(&["run"]);
    let root = p.root.to_str().unwrap();
    {
        let mut ide = Ide::stdio_trusted(&p, false);
        let state = ide.ok("project_trust", json!({}));
        assert_eq!(state["trusted"], false, "{state}");
        assert_eq!(state["findings"][0]["file"], ".idecraft/run.toml");
        let why = ide.refused(
            "run_configuration",
            json!({"name": "greet", "dry_run": false}),
        );
        assert!(why.contains("Safe mode"), "{why}");
        // The agent has no `trust` right.
        let why = ide.refused("trust_project", json!({"dry_run": false}));
        assert!(why.contains("`trust` right"), "{why}");
        assert!(!p.path("prepared.txt").exists());
    }
    let status = p.cli(&["trust", "status", root]);
    assert!(String::from_utf8_lossy(&status.stdout).starts_with("safe mode:"));
    assert!(p.cli(&["trust", "add", root]).status.success());
    let list = String::from_utf8_lossy(&p.cli(&["trust", "list"]).stdout).into_owned();
    assert!(list.contains("Project"), "{list}");
    {
        let mut ide = Ide::stdio_trusted(&p, false);
        assert_eq!(ide.ok("project_trust", json!({}))["trusted"], true);
        ide.ok(
            "run_configuration",
            json!({"name": "greet", "dry_run": false}),
        );
        assert!(p.path("prepared.txt").exists());
    }
    assert!(p.cli(&["trust", "remove", root]).status.success());
    let mut ide = Ide::stdio_trusted(&p, false);
    assert_eq!(ide.ok("project_trust", json!({}))["trusted"], false);
}

#[test]
fn with_the_run_right_the_chain_runs_in_order() {
    let p = Project::empty();
    p.write(".idecraft/run.toml", RUN_TOML).grant(&["run"]);
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "run_configuration",
        json!({"name": "greet", "dry_run": false}),
    );
    let all = text(&r);
    assert!(
        all.contains("prepared") && all.contains("hello world"),
        "{r}"
    );
    // Runs are logged for the user.
    assert!(p.read(".idecraft/agent-log.jsonl").contains("greet"));
}

#[test]
fn invalid_files_are_problems() {
    let p = Project::empty();
    p.write(".idecraft/run.toml", "[[run]]\nname = \"a\"\ncommand = []\n\n[[run]]\nname = \"b\"\ncommand = [\"x\"]\nbefore = [\"missing\"]\n");
    let mut ide = Ide::stdio(&p);
    let list = ide.ok("run_configurations", json!({}));
    assert!(!list["problems"].as_array().unwrap().is_empty(), "{list}");
}
