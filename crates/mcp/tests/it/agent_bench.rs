//! The agent benchmark (SPEC-0345): the repository's tasks, the checks, the steps, the report and
//! the MCP tool.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ferris_idea_mcp::agent_bench::{self, Check, Task, load_task, load_tasks, run, suite};
use serde_json::json;

use super::protocol::{connect_exact, is_error, json_text};

/// The repository's tasks folder.
fn repository_tasks() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(agent_bench::TASKS_DIR)
}

/// Every file below `dir` with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.insert(p.clone(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// SPEC-0345 AK-2.
#[tokio::test]
async fn repository_tasks_pass() {
    let dir = repository_tasks();
    let before = snapshot(&dir);
    let tasks = load_tasks(&dir).unwrap();
    assert!(tasks.len() >= 2, "{tasks:?}");
    let report = agent_bench::run_suite(&dir).await.unwrap();
    for r in &report.runs {
        assert!(r.error.is_none(), "{}/{}: {:?}", r.task, r.script, r.error);
        if r.control {
            assert!(
                !r.success,
                "{}/{}: the control was not caught",
                r.task, r.script
            );
        } else {
            assert!(r.success, "{}/{}: {r:#?}", r.task, r.script);
        }
    }
    for t in &tasks {
        assert!(
            t.scripts.iter().any(|s| s == "solve"),
            "{}: no solve run",
            t.id
        );
        assert!(
            t.scripts.iter().any(|s| s.starts_with("fail")),
            "{}: no control",
            t.id
        );
    }
    assert!(report.passed);
    assert_eq!(report.controls, report.controls_caught);
    assert_eq!(
        snapshot(&dir),
        before,
        "a run changed the repository's tasks"
    );
}

/// A task folder of its own for one test.
fn task_dir(tag: &str, toml: &str, files: &[(&str, &str)]) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-bench-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join(tag);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("task.toml"), toml.replace("ID", tag)).unwrap();
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn script(task: &Task, name: &str, text: &str) {
    std::fs::write(task.dir.join("scripts").join(format!("{name}.toml")), text).unwrap();
}

/// SPEC-0345 AK-3.
#[tokio::test]
async fn checks_and_steps() {
    let secret = "SECRET-CONTENT-4711";
    let dir = task_dir(
        "steps",
        r#"id = "ID"
title = "t"
prompt = "p"
grants = ["write"]
max_tool_calls = 2
[[check]]
file = "a.txt"
contains = "done"
[[check]]
file = "a.txt"
absent = "todo"
[[check]]
file = "gone.txt"
absent = "x"
[[check]]
file = "a.txt"
equals = "expected.txt"
[[check]]
tool = "read_file"
args = { path = "a.txt" }
pointer = ""
equals = "done SECRET-CONTENT-4711"
[[check]]
command = ["grep", "-q", "done", "a.txt"]
[[check]]
command = ["grep", "-q", "todo", "a.txt"]
exit = 1
"#,
        &[
            ("project/a.txt", &format!("todo {secret}")),
            ("expected.txt", &format!("done {secret}")),
        ],
    );
    let mut task = load_task(&dir).unwrap();
    assert!(matches!(&task.checks[4], Check::Tool { pointer, .. } if pointer.is_empty()));
    let edit = |dry: bool| {
        format!(
            "[[call]]\ntool = \"replace_text_in_file\"\nargs = {{ path = \"a.txt\", old_text = \"todo\", new_text = \"done\", dry_run = {dry} }}\n"
        )
    };
    let head = "agent = \"a\"\nmodel = \"m\"\ncost_cents = 5\n";

    // Every check kind passes.
    script(&task, "solve", &format!("{head}{}", edit(false)));
    let ok = run(&task, "solve").await;
    assert!(ok.success, "{ok:#?}");
    assert_eq!(ok.checks.len(), 7);
    assert_eq!(
        ok.checks.iter().map(|c| c.kind).collect::<Vec<_>>(),
        ["file", "file", "file", "file", "tool", "command", "command"]
    );
    assert_eq!(
        (ok.tool_calls, ok.tool_errors, ok.cost_cents),
        (1, 0, Some(5))
    );
    assert_eq!(ok.steps[0].tool, "replace_text_in_file");
    // The repository's project is untouched; the run worked on a copy.
    assert_eq!(
        std::fs::read_to_string(dir.join("project/a.txt")).unwrap(),
        format!("todo {secret}")
    );
    // No file content in the report.
    let shown = serde_json::to_string(&suite(vec![ok.clone()])).unwrap();
    assert!(!shown.contains(secret), "{shown}");

    // Every check fails on the untouched project (the dry run changes nothing).
    script(&task, "fail-dry", &format!("{head}{}", edit(true)));
    let dry = run(&task, "fail-dry").await;
    assert!(!dry.success && dry.control);
    let passed: Vec<bool> = dry.checks.iter().map(|c| c.passed).collect();
    assert_eq!(passed, [false, false, true, false, false, false, false]);
    assert!(!serde_json::to_string(&dry).unwrap().contains(secret));

    // An unexpected outcome stops the run at that step; later calls are not made.
    script(
        &task,
        "stops",
        &format!("{head}[[call]]\ntool = \"no_such_tool\"\n{}", edit(false)),
    );
    let stopped = run(&task, "stops").await;
    assert!(!stopped.success);
    assert_eq!(stopped.failed_step.as_ref().unwrap().index, 0);
    assert_eq!(stopped.tool_calls, 1);
    assert_eq!(stopped.tool_errors, 1);
    // An expected error passes as a step.
    script(
        &task,
        "expected-error",
        &format!(
            "{head}[[call]]\ntool = \"no_such_tool\"\nexpect = \"error\"\n{}",
            edit(false)
        ),
    );
    let expected = run(&task, "expected-error").await;
    assert!(expected.success, "{expected:#?}");
    assert_eq!(expected.tool_errors, 1);
    // An answer where an error was expected is a failed step.
    script(
        &task,
        "not-refused",
        &format!(
            "{head}[[call]]\ntool = \"read_file\"\nargs = {{ path = \"a.txt\" }}\nexpect = \"error\"\n"
        ),
    );
    let answered = run(&task, "not-refused").await;
    assert!(
        answered
            .failed_step
            .unwrap()
            .reason
            .contains("an error was expected")
    );

    // The tool-call limit (2 here).
    let read = "[[call]]\ntool = \"read_file\"\nargs = { path = \"a.txt\" }\n";
    script(&task, "long", &format!("{head}{read}{read}{}", edit(false)));
    let long = run(&task, "long").await;
    assert_eq!(long.limit, Some("tool_calls"));
    assert_eq!(long.tool_calls, 2);
    assert!(!long.success);

    // A right the task does not grant is refused.
    task.grants.clear();
    script(&task, "no-right", &format!("{head}{}", edit(false)));
    let refused = run(&task, "no-right").await;
    assert!(!refused.success);
    let why = &refused.failed_step.unwrap().reason;
    assert!(why.contains("write"), "{why}");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// SPEC-0345 AK-4.
#[tokio::test]
async fn suite_report_and_tool() {
    let report = |agent: &str, model: &str, success: bool, control: bool, cost: Option<u64>| {
        agent_bench::RunReport {
            task: "t".into(),
            script: "s".into(),
            agent: agent.into(),
            model: model.into(),
            control,
            success,
            failed_step: None,
            limit: None,
            checks: Vec::new(),
            tool_calls: 1,
            tool_errors: 0,
            duration_ms: 10,
            cost_cents: cost,
            steps: Vec::new(),
            error: None,
        }
    };
    let s = suite(vec![
        report("a", "m1", true, false, Some(2)),
        report("a", "m1", false, false, Some(3)),
        report("a", "m2", true, false, None),
        report("a", "m1", false, true, None),
    ]);
    assert_eq!(s.summary.len(), 2);
    let m1 = &s.summary[0];
    assert_eq!(
        (m1.runs, m1.successes, m1.duration_ms, m1.cost_cents),
        (2, 1, 20, Some(5))
    );
    assert!((m1.success_rate - 0.5).abs() < 1e-9);
    assert_eq!(s.summary[1].cost_cents, None);
    assert_eq!((s.controls, s.controls_caught), (1, 1));
    assert!(!s.passed);

    // The MCP tool, on a workspace holding the repository's tasks.
    let (mut client, root) = connect_exact(&[], |server, _| server).await;
    let target = root.join(agent_bench::TASKS_DIR);
    for t in load_tasks(&repository_tasks()).unwrap() {
        copy(&t.dir, &target.join(&t.id));
    }
    let listed = json_text(&client.call("agent_benchmark", json!({})).await);
    let ids: Vec<&str> = listed["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"fix-typo"), "{listed}");
    let dry = json_text(
        &client
            .call("agent_benchmark", json!({"task": "fix-typo"}))
            .await,
    );
    assert_eq!(dry["dry_run"], json!(true));
    assert!(
        dry["would_run"]
            .as_array()
            .unwrap()
            .contains(&json!("solve"))
    );
    let refused = client
        .call(
            "agent_benchmark",
            json!({"task": "fix-typo", "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    let unknown = client
        .call("agent_benchmark", json!({"task": "nope"}))
        .await;
    assert!(is_error(&unknown));
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
