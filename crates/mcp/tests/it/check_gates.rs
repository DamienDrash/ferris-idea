//! SPEC-0338 AK-6: `list_check_gates` and `run_check_gates` over MCP — nothing runs without the
//! `run` right or in safe mode, and a run answers with a structured, logged verdict.

use serde_json::{Value, json};

use super::protocol::{connect_custom, error_message, is_error, json_text};

const GATES: &str = r#"defaults = false

[[gate]]
name = "lint"
kind = "inspections"
command = ["sh", "-c", "echo 'src/app.py:2:1: E1 bad name'; echo API_KEY=sk-live-0123456789abcdef; exit 1"]

[[gate]]
name = "build"
kind = "build"
command = ["sh", "-c", "echo built > built.txt; exit 2"]

[[gate]]
name = "tests"
kind = "affected_tests"
command = ["sh", "-c", "echo \"$@\" > args.txt", "sh"]
pass_changed = true
"#;

fn write_gates(root: &std::path::Path) {
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    std::fs::write(root.join(".ferris-idea/check-gates.toml"), GATES).unwrap();
}

fn names(v: &Value) -> Vec<String> {
    v["gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| format!("{}={}", g["name"].as_str().unwrap(), g["status"]))
        .collect()
}

#[tokio::test]
async fn check_gate_tools_respect_rights_and_trust() {
    // Untrusted: listing and the dry run work, running is refused even with the `run` right.
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-mcp-gates-trust-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let settings = ferris_idea_core::trust::TrustSettings::with_file(&dir.join("trusted.toml"));
    let (mut client, root) = connect_custom(&[ferris_idea_core::Grant::Run], move |s, _| {
        s.with_trust(settings)
    })
    .await;
    write_gates(&root);
    let list = json_text(
        &client
            .call("list_check_gates", json!({"changed": ["src/app.py"]}))
            .await,
    );
    assert_eq!(list["problems"], json!([]), "{list}");
    assert_eq!(list["changed"], json!(["src/app.py"]));
    assert_eq!(
        list["gates"][2]["steps"][0]["command"][4], "./src/app.py",
        "{list}"
    );
    let dry = json_text(&client.call("run_check_gates", json!({})).await);
    assert_eq!(dry["dry_run"], true);
    let refused = client
        .call("run_check_gates", json!({"dry_run": false}))
        .await;
    assert!(is_error(&refused));
    assert!(
        error_message(&refused).starts_with("Safe mode:"),
        "{refused}"
    );
    assert!(!root.join("built.txt").exists());
    // The project file is among what makes the project run code.
    let trust = json_text(&client.call("project_trust", json!({})).await);
    assert!(
        trust["findings"]
            .to_string()
            .contains(".ferris-idea/check-gates.toml"),
        "{trust}"
    );
    // Trusted but without the `run` right: refused, nothing ran.
    let (mut plain, root) = connect_custom(&[], |s, _| s).await;
    write_gates(&root);
    let refused = plain
        .call("run_check_gates", json!({"dry_run": false}))
        .await;
    assert!(is_error(&refused), "{refused}");
    assert!(error_message(&refused).contains("`run`"), "{refused}");
    assert!(!root.join("built.txt").exists());
    // A changed path outside the workspace is refused.
    let refused = plain
        .call("list_check_gates", json!({"changed": ["../outside.rs"]}))
        .await;
    assert!(is_error(&refused));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn run_check_gates_returns_a_structured_verdict() {
    let (mut client, root) = connect_custom(&[ferris_idea_core::Grant::Run], |s, _| s).await;
    write_gates(&root);
    let run = json_text(
        &client
            .call(
                "run_check_gates",
                json!({"dry_run": false, "changed": ["src/app.py"]}),
            )
            .await,
    );
    assert_eq!(run["passed"], false, "{run}");
    assert_eq!(
        names(&run),
        ["lint=\"failed\"", "build=\"failed\"", "tests=\"not_run\""]
    );
    let lint = &run["gates"][0];
    let finding = &lint["findings"][0];
    assert_eq!(finding["file"], "src/app.py");
    assert_eq!(finding["line"], 2);
    assert_eq!(finding["column"], 1);
    assert_eq!(finding["severity"], "error");
    assert_eq!(finding["message"], "E1 bad name");
    let output = lint["steps"][0]["output"].as_str().unwrap();
    assert!(
        output.contains("E1 bad name") && !output.contains("sk-live-0123456789abcdef"),
        "{output}"
    );
    assert_eq!(
        run["gates"][1]["reason"],
        "sh -c echo built > built.txt; exit 2: exit code 2"
    );
    assert!(root.join("built.txt").exists());
    // Only the affected tests, with the changed files as `./` arguments.
    let run = json_text(
        &client
            .call(
                "run_check_gates",
                json!({"dry_run": false, "gates": ["tests"], "changed": ["src/app.py", "-x"]}),
            )
            .await,
    );
    assert_eq!(run["passed"], true, "{run}");
    assert_eq!(
        std::fs::read_to_string(root.join("args.txt"))
            .unwrap()
            .trim(),
        "./-x ./src/app.py"
    );
    let refused = client
        .call(
            "run_check_gates",
            json!({"dry_run": false, "gates": ["nope"]}),
        )
        .await;
    assert!(is_error(&refused));
    // Both runs are in the agent log.
    let log = std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl")).unwrap();
    assert!(
        log.contains("\"passed\":false") && log.contains("\"passed\":true"),
        "{log}"
    );
    // A project file with problems runs nothing.
    std::fs::write(
        root.join(".ferris-idea/check-gates.toml"),
        "[[gate]]\nname = \"x\"\nkind = \"lint\"\ncommand = [\"true\"]\n",
    )
    .unwrap();
    let refused = client
        .call("run_check_gates", json!({"dry_run": false}))
        .await;
    assert!(
        error_message(&refused).contains("unknown kind `lint`"),
        "{refused}"
    );
}
