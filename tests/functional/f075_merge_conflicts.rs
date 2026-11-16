//! F-075 · Resolve merge conflicts (three-way) (SPEC-0037 AK-3…AK-6, SPEC-0101).

use crate::harness::*;
use serde_json::json;

#[test]
fn three_way_merge_of_texts() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let clean = ide.ok(
        "merge_three_way",
        json!({"base": "a\nb\nc\n", "ours": "A\nb\nc\n", "theirs": "a\nb\nC\n"}),
    );
    assert_eq!(clean["conflicts"], 0);
    assert_eq!(clean["text"], "A\nb\nC\n");
    let conflict = ide.ok(
        "merge_three_way",
        json!({"base": "a\n", "ours": "x\n", "theirs": "y\n"}),
    );
    assert_eq!(conflict["conflicts"], 1);
    assert_eq!(
        conflict["text"],
        "<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n"
    );
}

#[test]
fn a_conflicted_merge_is_resolved_in_the_repository() {
    let p = Project::empty();
    p.write("f.txt", "base\n").git_repo();
    p.git(&["checkout", "-q", "-b", "other"]);
    p.write("f.txt", "theirs\n");
    p.git(&["commit", "-qam", "theirs"]);
    p.git(&["checkout", "-q", "main"]);
    p.write("f.txt", "ours\n");
    p.git(&["commit", "-qam", "ours"]);
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "git_merge",
        json!({"source": "other", "dry_run": false, "allow_conflicts": true}),
    );
    assert!(text(&r).contains("f.txt"), "{r}");
    let conflicts = ide.ok("git_conflicts", json!({}));
    let c = text(&conflicts);
    assert!(
        c.contains("ours") && c.contains("theirs") && c.contains("base"),
        "{conflicts}"
    );
    ide.ok(
        "git_resolve_conflict",
        json!({"path": "f.txt", "take": "theirs", "dry_run": false}),
    );
    assert_eq!(p.read("f.txt"), "theirs\n");
    assert!(!text(&ide.ok("git_conflicts", json!({}))).contains("f.txt"));
}
