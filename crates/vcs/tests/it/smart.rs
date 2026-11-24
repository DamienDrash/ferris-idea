//! SPEC-0231 AK-1 – AK-3, SPEC-0036 v2 AK-7, SPEC-0106 v2 AK-7: smart checkout and merge, merge that stops, skip.

use std::path::{Path, PathBuf};
use std::process::Command;

use ferris_idea_vcs::{Operation, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A repository with `readme.txt` committed on `main`.
fn repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-smart-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test Author"]);
    git(&dir, &["config", "user.email", "test@example.invalid"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    write(&dir, "readme.txt", "one\n");
    commit(&dir, "first");
    dir
}

fn write(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap()
}

/// `main` with `readme.txt` = `one`; `other` changes `notes.txt`'s first line.
fn branches(tag: &str) -> PathBuf {
    let dir = repo(tag);
    write(&dir, "notes.txt", "a\nb\nc\nd\ne\n");
    commit(&dir, "notes");
    git(&dir, &["checkout", "-q", "-b", "other"]);
    write(&dir, "notes.txt", "A\nb\nc\nd\ne\n");
    commit(&dir, "upper a");
    git(&dir, &["checkout", "-q", "main"]);
    dir
}

#[test]
fn smart_checkout_puts_changes_aside_and_back() {
    let dir = branches("checkout");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "notes.txt", "a\nb\nc\nd\nE\n");
    write(&dir, "new.txt", "untracked\n");
    assert!(
        r.checkout("other", false).is_err(),
        "plain checkout refuses"
    );
    let done = r.smart_checkout("other", false).unwrap();
    assert_eq!(done.result.branch, "other");
    let stash = done.stash.unwrap();
    assert!(stash.restored, "{stash:?}");
    assert!(
        stash
            .message
            .ends_with("Ferris IDEA smart checkout: main → other")
    );
    assert_eq!(read(&dir, "notes.txt"), "A\nb\nc\nd\nE\n");
    assert_eq!(read(&dir, "new.txt"), "untracked\n");
    assert!(
        r.stashes().unwrap().is_empty(),
        "the stash is dropped again"
    );
    assert_eq!(r.pending_smart_stash(), None);
    // Nothing to put aside: a plain checkout.
    git(&dir, &["checkout", "-q", "--", "notes.txt"]);
    std::fs::remove_file(dir.join("new.txt")).unwrap();
    let done = r.smart_checkout("main", false).unwrap();
    assert!(done.stash.is_none());
}

#[test]
fn smart_checkout_keeps_a_conflicting_stash() {
    let dir = branches("keep");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "notes.txt", "x\nb\nc\nd\ne\n");
    let done = r.smart_checkout("other", false).unwrap();
    let stash = done.stash.unwrap();
    assert!(!stash.restored);
    assert_eq!(stash.conflicts, vec!["notes.txt".to_owned()]);
    assert!(read(&dir, "notes.txt").contains("<<<<<<<"));
    assert_eq!(r.stashes().unwrap().len(), 1, "kept in the list");
    assert_eq!(r.conflicts().unwrap().len(), 1);
}

#[test]
fn merge_stops_on_conflicts_and_continues() {
    let dir = branches("stop");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "notes.txt", "a1\nb\nc\nd\ne\n");
    commit(&dir, "a1 on main");
    let done = r.merge_stopping("other").unwrap();
    assert!(!done.ok);
    assert_eq!(done.conflicts, vec!["notes.txt".to_owned()]);
    assert_eq!(done.in_progress, Some(Operation::Merge));
    assert!(dir.join(".git/MERGE_HEAD").is_file());
    assert!(r.skip_operation().is_err(), "a merge cannot be skipped");
    r.resolve_conflict("notes.txt", "A1\nb\nc\nd\ne\n").unwrap();
    let done = r.continue_operation().unwrap();
    assert!(done.ok && done.in_progress.is_none(), "{done:?}");
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%P"])
            .split_whitespace()
            .count(),
        2,
        "a merge commit"
    );
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%s"]).trim(),
        "Merge branch 'other'"
    );
}

#[test]
fn smart_merge_waits_with_the_stash_until_the_merge_is_done() {
    let dir = branches("merge");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "notes.txt", "a1\nb\nc\nd\ne\n");
    commit(&dir, "a1 on main");
    write(&dir, "notes.txt", "a1\nb\nc\nd\nlocal\n");
    // The preview names the local change in the way although the merge itself conflicts, so
    // the IDE asks for a smart merge (SPEC-0231 v2 behaviour 7, OBS-0022 L2).
    let preview = r.merge("other", true).unwrap();
    assert_eq!(preview.merge_conflicts, vec!["notes.txt".to_string()]);
    assert_eq!(preview.local_conflicts, vec!["notes.txt".to_string()]);
    assert!(
        r.merge_stopping("other").is_err(),
        "local change in the way"
    );
    let done = r.smart_merge("other").unwrap();
    assert_eq!(done.result.in_progress, Some(Operation::Merge));
    let stash = done.stash.unwrap();
    assert!(!stash.restored);
    assert!(r.pending_smart_stash().is_some());
    r.resolve_conflict("notes.txt", "A1\nb\nc\nd\ne\n").unwrap();
    let done = r.continue_operation().unwrap();
    assert!(done.ok && done.conflicts.is_empty(), "{done:?}");
    assert_eq!(read(&dir, "notes.txt"), "A1\nb\nc\nd\nlocal\n");
    assert_eq!(r.pending_smart_stash(), None);
    assert!(r.stashes().unwrap().is_empty());
}

#[test]
fn skip_drops_the_stopped_commit() {
    let dir = branches("skip");
    let r = Repo::discover(&dir).unwrap();
    assert!(r.skip_operation().is_err(), "nothing to skip");
    write(&dir, "notes.txt", "a1\nb\nc\nd\ne\n");
    commit(&dir, "a1 on main");
    git(&dir, &["checkout", "-q", "other"]);
    let done = r.rebase("main").unwrap();
    assert_eq!(done.in_progress, Some(Operation::Rebase));
    let state = r.branch_state();
    assert_eq!(state.operation, Some(Operation::Rebase));
    assert_eq!(state.rebasing.as_deref(), Some("other"));
    let done = r.skip_operation().unwrap();
    assert!(done.ok && done.in_progress.is_none(), "{done:?}");
    assert_eq!(
        git(&dir, &["rev-parse", "other"]),
        git(&dir, &["rev-parse", "main"]),
        "the only commit was skipped"
    );
}

#[test]
fn branch_state_counts_ahead_and_behind_the_upstream() {
    let dir = branches("ahead");
    let r = Repo::discover(&dir).unwrap();
    let state = r.branch_state();
    assert_eq!(
        (state.branch.as_deref(), state.ahead, state.behind),
        (Some("main"), 0, 0)
    );
    git(&dir, &["branch", "--set-upstream-to", "other"]);
    write(&dir, "readme.txt", "two\n");
    commit(&dir, "two");
    let state = r.branch_state();
    assert_eq!((state.ahead, state.behind), (1, 1));
    assert_eq!(state.operation, None);
}

#[test]
fn merging_a_remote_tracking_branch_uses_gits_message() {
    // SPEC-0144 v3 behaviour 6, OBS-0022 L19: an update by merge names the remote-tracking branch
    // as Git does.
    let dir = branches("remote-msg");
    let r = Repo::discover(&dir).unwrap();
    let other = git(&dir, &["rev-parse", "other"]);
    git(
        &dir,
        &["update-ref", "refs/remotes/origin/other", other.trim()],
    );
    write(&dir, "readme.txt", "two\n");
    commit(&dir, "two on main");
    r.merge("origin/other", false).unwrap();
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%s"]).trim(),
        "Merge remote-tracking branch 'origin/other'"
    );
}
