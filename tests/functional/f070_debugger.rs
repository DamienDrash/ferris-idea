//! F-070 · Debugger: breakpoints, steps, variables; stop a program and look at its state
//! (SPEC-0039 AK-2/AK-4, SPEC-0054, SPEC-0194).

use crate::harness::*;
use serde_json::json;

pub const SCRIPT: &str = "function twice(n) {\n  const doubled = n * 2;\n  return doubled;\n}\nlet total = 0;\nfor (let i = 0; i < 5; i++) {\n  total += twice(i);\n}\nconsole.log(\"total\", total);\n";

/// A project with `app.js`, the `debug` right granted, when Node.js and its debug adapter exist.
pub fn node_project() -> Option<(Project, Ide)> {
    let p = Project::empty();
    p.write("app.js", SCRIPT).grant(&["debug"]);
    let mut ide = Ide::stdio(&p);
    let adapters = ide.ok("debug_adapters", json!({}));
    let js = adapters["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "js-debug")
        .cloned();
    if js.is_none_or(|a| a["command"].is_null()) || adapters["can_debug_unattended"] != true {
        skip_optional(
            "Node.js debug adapter (js-debug) not installed or debugging not allowed here",
        );
        return None;
    }
    Some((p, ide))
}

#[test]
fn adapters_are_listed_with_their_availability() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("debug_adapters", json!({}));
    let names: Vec<String> = r["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| text(&a["name"]))
        .collect();
    for n in ["lldb-dap", "js-debug", "debugpy"] {
        assert!(names.contains(&n.to_owned()), "{r}");
    }
}

#[test]
fn starting_needs_the_debug_right() {
    let p = Project::empty();
    p.write("app.js", SCRIPT);
    let mut ide = Ide::stdio(&p);
    let preview = ide.ok("debug_start", json!({"program": "app.js"}));
    assert_eq!(preview["dry_run"], true);
    let reason = ide.refused(
        "debug_start",
        json!({"program": "app.js", "dry_run": false}),
    );
    assert!(reason.contains("debug"), "{reason}");
}

#[test]
fn stop_at_a_breakpoint_read_locals_step_and_run_to_the_end() {
    let Some((_p, mut ide)) = node_project() else {
        return;
    };
    let r = ide.ok(
        "debug_start",
        json!({"program": "app.js", "breakpoints": [{"path": "app.js", "line": 3}],
               "wait_seconds": 60, "dry_run": false}),
    );
    let session = r["session"].clone();
    let state = &r["state"];
    assert_eq!(state["state"], "stopped", "{r}");
    assert_eq!(state["frames"][0]["line"], 3);
    assert!(text(&state["locals"]).contains("doubled"), "{r}");
    let step = ide.ok(
        "debug_resume",
        json!({"session": session, "action": "step_out", "wait_seconds": 30}),
    );
    let top = &step["state"]["frames"][0];
    assert!(
        top["name"] != "global.twice" && [6, 7].contains(&top["line"].as_u64().unwrap()),
        "back in the loop: {step}"
    );
    ide.ok("debug_stop", json!({"session": session}));
}
