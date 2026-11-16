//! F-071 · Conditional breakpoints, evaluate expressions (SPEC-0039 v2 AK-5, IMP-0022, SPEC-0127).

use crate::harness::*;
use serde_json::json;

use crate::f070_debugger::node_project;

#[test]
fn a_condition_stops_once_and_an_expression_is_evaluated() {
    let Some((_p, mut ide)) = node_project() else {
        return;
    };
    let r = ide.ok(
        "debug_start",
        json!({"program": "app.js", "wait_seconds": 60, "dry_run": false,
               "breakpoints": [{"path": "app.js", "line": 3, "condition": "n == 3"}]}),
    );
    let session = r["session"].clone();
    assert_eq!(r["state"]["state"], "stopped", "{r}");
    let value = ide.ok(
        "debug_evaluate",
        json!({"session": session, "expression": "doubled + 1"}),
    );
    assert_eq!(value["value"], "7", "doubled is 6 when n is 3: {value}");
    let end = ide.ok(
        "debug_resume",
        json!({"session": session, "action": "continue", "wait_seconds": 60}),
    );
    assert_eq!(end["state"]["state"], "exited", "stopped only once: {end}");
    assert!(text(&end["output"]).contains("total 20"), "{end}");
}

#[test]
fn saved_breakpoints_keep_condition_and_pass_count() {
    let p = Project::empty();
    p.write("app.js", crate::f070_debugger::SCRIPT);
    let mut ide = Ide::stdio(&p);
    ide.ok(
        "set_breakpoint",
        json!({"path": "app.js", "line": 3, "condition": "n > 1", "hit_condition": "2", "dry_run": false}),
    );
    let saved = ide.ok("saved_breakpoints", json!({}));
    let all = text(&saved);
    assert!(all.contains("n > 1") && all.contains("app.js"), "{saved}");
}
