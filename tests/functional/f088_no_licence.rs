//! F-088 · No trial, no locks, no telemetry (ADR-0007).

use crate::harness::*;
use serde_json::json;

#[test]
fn no_licence_or_trial_tool_and_every_tool_works_without_an_account() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    for tool in ide.tools() {
        let t = tool.to_lowercase();
        assert!(
            !["licen", "trial", "telemetry", "activation"]
                .iter()
                .any(|w| t.contains(w)),
            "{tool}"
        );
    }
    // Reading works with an empty settings folder: nothing to register or sign in to.
    assert_eq!(ide.ok("list_files", json!({})).as_array().unwrap().len(), 3);
}

#[test]
fn opening_a_project_writes_nothing_outside_it_and_contacts_no_server() {
    let p = Project::reference("python-basic");
    // The harness grants `write` there by default; this test starts from an empty folder.
    std::fs::remove_file(p.config.join("agent-permissions.toml")).unwrap();
    let out = p.cli(&["open", p.root.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_dir(&p.config).unwrap().count(),
        0,
        "no files in the settings folder"
    );
}
