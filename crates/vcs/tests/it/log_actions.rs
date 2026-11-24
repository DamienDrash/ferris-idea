//! SPEC-0086 v7 AK-18, AK-19, AK-22: the facts behind the commit menu of the Git log and the
//! operations behind its entries, on repositories built with `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use ferris_idea_vcs::{CommitAction, Repo, ResetMode, commit_actions};

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

/// A repository with three commits on `main` (`first`, `second`, `third`), the first two pushed
/// to a bare `origin`, and a branch `side` with a commit of its own.
fn repo(tag: &str) -> PathBuf {
    let base =
        std::env::temp_dir().join(format!("ferris-idea-logact-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let base = base.canonicalize().unwrap();
    let dir = base.join("work");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test Author"]);
    git(&dir, &["config", "user.email", "test@example.invalid"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    write(&dir, "a.txt", "one\n");
    commit(&dir, "first");
    write(&dir, "a.txt", "one\ntwo\n");
    commit(&dir, "second");
    git(&base, &["init", "-q", "--bare", "origin.git"]);
    git(&dir, &["remote", "add", "origin", "../origin.git"]);
    git(&dir, &["push", "-q", "origin", "main"]);
    git(&dir, &["checkout", "-q", "-b", "side"]);
    write(&dir, "side.txt", "side\n");
    commit(&dir, "on side");
    git(&dir, &["checkout", "-q", "main"]);
    write(&dir, "b.txt", "three\n");
    commit(&dir, "third");
    dir
}

fn write(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn rev(dir: &Path, r: &str) -> String {
    git(dir, &["rev-parse", r]).trim().to_owned()
}

fn subjects(dir: &Path) -> Vec<String> {
    git(dir, &["log", "--format=%s", "main"])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn facts_follow_head_branch_and_remote() {
    let dir = repo("facts");
    let r = Repo::discover(&dir).unwrap();
    let head = r.commit_facts("HEAD").unwrap();
    assert!(head.is_head && head.on_current_branch && !head.pushed && !head.root);
    assert!(!head.has_child);
    let second = r.commit_facts("HEAD~1").unwrap();
    assert!(second.pushed && second.on_current_branch && second.has_child);
    let first = r.commit_facts("HEAD~2").unwrap();
    assert!(first.root);
    let side = r.commit_facts("side").unwrap();
    assert!(!side.on_current_branch && !side.pushed);
    let menu = commit_actions(&second);
    let drop = menu
        .iter()
        .find(|e| e.action == CommitAction::Drop)
        .unwrap();
    assert!(!drop.enabled && drop.reason.as_deref().is_some_and(|r| r.contains("pushed")));
    assert!(
        r.drop_commit("HEAD~1").is_err(),
        "pushed commits are refused"
    );
    assert!(r.commit_facts("--all").is_err(), "options are no revisions");
}

#[test]
fn hard_reset_keeps_local_changes_in_a_stash() {
    let dir = repo("reset");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "a.txt", "one\ntwo\nlocal\n");
    let done = r.reset_to("HEAD~1", ResetMode::Hard).unwrap();
    assert!(done.ok, "{}", done.output);
    assert!(done.output.contains("Ferris IDEA: before hard reset"));
    assert_eq!(rev(&dir, "HEAD"), rev(&dir, "origin/main"));
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    let list = git(&dir, &["stash", "list"]);
    assert!(list.contains("Ferris IDEA: before hard reset"), "{list}");
    // A soft reset keeps the undone change staged.
    git(&dir, &["reset", "-q", "--hard", "side"]);
    let soft = r.reset_to("HEAD~1", ResetMode::Soft).unwrap();
    assert!(soft.ok);
    assert!(git(&dir, &["diff", "--cached", "--name-only"]).contains("side.txt"));
    // Without local changes no stash is made.
    git(&dir, &["reset", "-q", "--hard", "HEAD"]);
    let clean = r.reset_to("HEAD~1", ResetMode::Hard).unwrap();
    assert!(!clean.output.contains("stash"));
}

#[test]
fn revert_undo_drop_and_reword() {
    let dir = repo("rewrite");
    let r = Repo::discover(&dir).unwrap();
    // Reword and drop the unpushed commit; the pushed ones stay.
    write(&dir, "c.txt", "four\n");
    commit(&dir, "fourth");
    let done = r.reword_commit("HEAD~1", "third, reworded").unwrap();
    assert!(done.ok, "{}", done.output);
    assert_eq!(
        subjects(&dir),
        ["fourth", "third, reworded", "second", "first"]
    );
    let done = r.drop_commit("HEAD~1").unwrap();
    assert!(done.ok, "{}", done.output);
    assert_eq!(subjects(&dir), ["fourth", "second", "first"]);
    // Undo keeps the change staged.
    let done = r.undo_commit("HEAD").unwrap();
    assert!(done.ok);
    assert_eq!(subjects(&dir), ["second", "first"]);
    assert!(git(&dir, &["diff", "--cached", "--name-only"]).contains("c.txt"));
    git(&dir, &["reset", "-q", "--hard", "HEAD"]);
    assert!(
        r.undo_commit("HEAD").is_err(),
        "the pushed head is not undone"
    );
    // Revert adds a commit taking the change back.
    let done = r.revert_commit("HEAD").unwrap();
    assert!(done.ok, "{}", done.output);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
    assert!(subjects(&dir)[0].starts_with("Revert \"second\""));
}

#[test]
fn a_conflicting_revert_changes_nothing() {
    let dir = repo("revert-conflict");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "a.txt", "changed\n");
    commit(&dir, "rewrite a");
    let before = rev(&dir, "HEAD");
    let done = r.revert_commit("HEAD~2").unwrap();
    assert!(!done.ok);
    assert_eq!(done.conflicts, ["a.txt"]);
    assert!(done.output.contains("nothing was changed"));
    assert_eq!(done.in_progress, None);
    assert_eq!(rev(&dir, "HEAD"), before);
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "changed\n"
    );
}

#[test]
fn tags_patches_and_detached_checkout() {
    let dir = repo("tag");
    let r = Repo::discover(&dir).unwrap();
    r.create_tag("v0.1", "HEAD~1").unwrap();
    assert_eq!(rev(&dir, "v0.1"), rev(&dir, "HEAD~1"));
    assert!(r.create_tag("v0.1", "HEAD").is_err(), "an existing tag");
    assert!(r.create_tag("-x", "HEAD").is_err(), "an option as name");
    let patch = r.commit_patch("HEAD~2").unwrap();
    assert!(patch.contains("Subject: [PATCH] first") && patch.contains("+one"));
    let done = r.checkout_revision("side").unwrap();
    assert!(done.ok, "{}", done.output);
    assert_eq!(r.current_branch().unwrap(), None);
    let facts = r.commit_facts("HEAD~1").unwrap();
    assert!(facts.detached);
    assert!(r.reset_to("HEAD~1", ResetMode::Mixed).is_err());
}

#[test]
fn a_different_committer_is_reported() {
    let dir = repo("committer");
    let r = Repo::discover(&dir).unwrap();
    let out = Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", "applied"])
        .env("GIT_COMMITTER_NAME", "Other Person")
        .env("GIT_COMMITTER_EMAIL", "other@example.invalid")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let log = r.log_all(10).unwrap();
    assert_eq!(
        log[0].committer.as_deref(),
        Some("Other Person <other@example.invalid>")
    );
    assert!(log[1..].iter().all(|c| c.committer.is_none()));
    let details = r.commit_details("HEAD").unwrap();
    assert!(details.info.committer.is_some());
}

#[test]
fn revision_files_compare_fixup_and_push_up_to() {
    // SPEC-0086 v8 AK-26, SPEC-0068 v6 AK-12.
    use ferris_idea_vcs::PushOptions;
    let dir = repo("v8");
    let r = Repo::discover(&dir).unwrap();
    let second = rev(&dir, "HEAD~1");
    let third = rev(&dir, "HEAD");
    assert_eq!(r.files_at(&second).unwrap(), ["a.txt"]);
    assert_eq!(r.files_at("main").unwrap(), ["a.txt", "b.txt"]);
    assert!(r.files_at("--all").is_err());
    write(&dir, "a.txt", "one\ntwo\nlocal\n");
    let changed = r.changes_against_worktree(&second).unwrap();
    assert!(changed.iter().any(|c| c.path == "a.txt" && c.status == "M"));
    assert!(changed.iter().any(|c| c.path == "b.txt" && c.status == "A"));
    // Fixup and squash commits of chosen paths; a pushed commit is refused.
    assert_eq!(r.fixup_message(&third, true).unwrap(), "squash! third");
    assert!(
        r.commit_fixup(&second, false, &["a.txt".into()], false)
            .is_err()
    );
    let done = r
        .commit_fixup(&third, false, &["a.txt".into()], false)
        .unwrap();
    assert!(done.commit.is_some());
    assert_eq!(subjects(&dir)[0], "fixup! third");
    // Push All up to Here: only `third` goes, the fixup stays unpushed.
    let out = r.outgoing_of(None, Some(&third)).unwrap();
    assert_eq!(out.up_to.as_deref(), Some(third.as_str()));
    assert_eq!(out.commits.len(), 1);
    assert_eq!(out.commits[0].id, third);
    assert!(out.files.keys().all(|k| k == &third));
    assert!(
        r.outgoing_of(None, Some(&second)).is_err(),
        "pushed already"
    );
    let pushed = r
        .push_with(
            &PushOptions {
                up_to: Some(third.clone()),
                ..PushOptions::default()
            },
            ferris_idea_vcs::REMOTE_TIMEOUT,
        )
        .unwrap();
    assert!(pushed.ok, "{}", pushed.messages);
    let origin = dir.parent().unwrap().join("origin.git");
    assert_eq!(rev(&origin, "main"), third);
    assert_eq!(r.outgoing().unwrap().commits.len(), 1);
    // Compare with another branch, and push that branch without checking it out.
    let (ahead, behind) = r.compare_with("side").unwrap();
    assert_eq!(ahead.len(), 1);
    assert_eq!(ahead[0].summary, "on side");
    assert_eq!(behind.len(), 2);
    let side = r.outgoing_of(Some("side"), None).unwrap();
    assert_eq!(side.branch, "side");
    assert_eq!(side.commits.len(), 1);
    let pushed = r
        .push_with(
            &PushOptions {
                branch: Some("side".into()),
                ..PushOptions::default()
            },
            ferris_idea_vcs::REMOTE_TIMEOUT,
        )
        .unwrap();
    assert!(pushed.ok, "{}", pushed.messages);
    assert_eq!(rev(&origin, "side"), rev(&dir, "side"));
}
