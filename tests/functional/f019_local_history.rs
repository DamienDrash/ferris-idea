//! F-019 · Local history: restore file versions independently of VCS (SPEC-0044 AK-1, AK-3, AK-5).

use crate::harness::*;
use serde_json::json;

#[test]
fn changes_are_recorded_and_a_restore_can_be_undone() {
    let p = Project::empty();
    p.write("notes.txt", "first\n");
    let mut ide = Ide::stdio(&p);
    let h = ide.ok("local_history", json!({"path": "notes.txt"}));
    let first = h["revisions"][0]["id"].clone();
    // Unchanged content adds nothing.
    let h = ide.ok("local_history", json!({"path": "notes.txt"}));
    assert_eq!(h["revisions"].as_array().unwrap().len(), 1, "{h}");

    p.write("notes.txt", "second\n");
    let h = ide.ok("local_history", json!({"path": "notes.txt"}));
    assert_eq!(h["revisions"].as_array().unwrap().len(), 2, "{h}");
    let second = h["revisions"][0]["id"].clone();

    let preview = ide.ok(
        "restore_local_revision",
        json!({"path": "notes.txt", "revision": first}),
    );
    assert!(text(&preview).contains("first"), "{preview}");
    assert_eq!(p.read("notes.txt"), "second\n", "dry run writes nothing");
    ide.ok(
        "restore_local_revision",
        json!({"path": "notes.txt", "revision": first, "dry_run": false}),
    );
    assert_eq!(p.read("notes.txt"), "first\n");
    // The state before the restore is a revision too, so the restore can be undone.
    ide.ok(
        "restore_local_revision",
        json!({"path": "notes.txt", "revision": second, "dry_run": false}),
    );
    assert_eq!(p.read("notes.txt"), "second\n");
}

#[test]
fn history_works_without_git() {
    let p = Project::empty();
    p.write("a.txt", "x\n");
    let mut ide = Ide::stdio(&p);
    assert!(!p.path(".git").exists());
    let h = ide.ok("local_history", json!({"path": "a.txt"}));
    assert_eq!(h["revisions"].as_array().unwrap().len(), 1);
    let unknown = ide.refused(
        "restore_local_revision",
        json!({"path": "a.txt", "revision": 999_999}),
    );
    assert!(unknown.contains("999999"), "{unknown}");
}
