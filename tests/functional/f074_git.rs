//! F-074 · Git: status, diff, commit, branches, log, blame (SPEC-0032…SPEC-0036, SPEC-0055,
//! SPEC-0106).

use crate::harness::*;
use serde_json::json;

fn repo() -> Project {
    let p = Project::reference("python-basic");
    p.git_repo();
    p
}

#[test]
fn status_diff_commit_log_and_blame() {
    let p = repo();
    p.write("test_model.py", &(p.read("test_model.py") + "\n# more\n"));
    p.write("new.txt", "n\n");
    let mut ide = Ide::stdio(&p);
    let status = ide.ok("git_status", json!({}));
    let all = text(&status);
    assert!(
        all.contains("test_model.py") && all.contains("new.txt"),
        "{status}"
    );
    let diff = ide.ok("git_diff", json!({"path": "test_model.py"}));
    assert_eq!(diff["changes"][0]["kind"], "added", "{diff}");
    assert!(text(&diff["hunks"]).contains("+# more"), "{diff}");

    let commit = ide.ok(
        "git_commit",
        json!({"message": "More", "paths": ["test_model.py"], "dry_run": false}),
    );
    assert_eq!(commit["commit"]["summary"], "More");
    // Only the given file went in.
    let status = ide.ok("git_status", json!({}));
    assert!(
        !text(&status).contains("test_model.py") && text(&status).contains("new.txt"),
        "{status}"
    );
    let log = ide.ok("git_log", json!({}));
    assert_eq!(log[0]["summary"], "More");
    assert_eq!(log[1]["summary"], "initial");
    let blame = ide.ok("git_blame", json!({"path": "test_model.py"}));
    let last = blame.as_array().unwrap().last().unwrap();
    assert_eq!(last["commit"]["summary"], "More", "{blame}");
}

#[test]
fn branches_switch_and_merge() {
    let p = repo();
    let mut ide = Ide::stdio(&p);
    ide.ok("git_create_branch", json!({"name": "feature"}));
    ide.ok(
        "git_switch_branch",
        json!({"name": "feature", "dry_run": false}),
    );
    p.write("feature.txt", "f\n");
    ide.ok(
        "git_commit",
        json!({"message": "Feature", "paths": ["feature.txt"], "dry_run": false}),
    );
    ide.ok(
        "git_switch_branch",
        json!({"name": "main", "dry_run": false}),
    );
    assert!(!p.path("feature.txt").exists());
    let merged = ide.ok("git_merge", json!({"source": "feature", "dry_run": false}));
    assert!(text(&merged).contains("fast"), "{merged}");
    assert!(p.path("feature.txt").exists());
    let branches = ide.ok("git_branches", json!({}));
    assert_eq!(branches["branches"][0]["name"], "main");
    assert_eq!(branches["branches"][0]["current"], true);
}

#[test]
fn network_operations_need_the_remote_right() {
    let p = repo();
    let mut ide = Ide::stdio(&p);
    let reason = ide.refused("git_fetch", json!({}));
    assert!(reason.contains("remote"), "{reason}");
}
