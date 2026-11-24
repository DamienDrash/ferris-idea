//! SPEC-0106 AK-1 – AK-3: stash, cherry-pick and rebase on repositories built with `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use ferris_idea_vcs::{Operation, Repo, TodoAction};

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
    let dir = std::env::temp_dir().join(format!("ferris-idea-ops-{tag}-{}", std::process::id()));
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

#[test]
fn stash_push_list_apply_pop_and_drop() {
    let dir = repo("stash");
    let r = Repo::discover(&dir).unwrap();
    assert!(r.stashes().unwrap().is_empty());
    write(&dir, "readme.txt", "two\n");
    write(&dir, "new.txt", "untracked\n");
    let pushed = r.stash_push(Some("my change"), true).unwrap();
    assert!(pushed.ok, "{}", pushed.output);
    assert_eq!(read(&dir, "readme.txt"), "one\n");
    assert!(
        !dir.join("new.txt").exists(),
        "untracked files are stashed too"
    );
    let list = r.stashes().unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].message.ends_with("my change"), "{list:?}");
    let applied = r.stash_apply(0, false).unwrap();
    assert!(applied.ok && applied.conflicts.is_empty(), "{applied:?}");
    assert_eq!(read(&dir, "readme.txt"), "two\n");
    assert_eq!(r.stashes().unwrap().len(), 1, "apply keeps the stash");
    git(&dir, &["checkout", "--", "readme.txt"]);
    std::fs::remove_file(dir.join("new.txt")).unwrap();
    let popped = r.stash_apply(0, true).unwrap();
    assert!(popped.ok, "{popped:?}");
    assert!(r.stashes().unwrap().is_empty(), "pop drops the stash");
    r.stash_push(None, true).unwrap();
    assert!(r.stash_drop(0).unwrap().ok);
    assert!(r.stashes().unwrap().is_empty());
    assert!(r.stash_drop(0).is_err(), "no such stash");
}

#[test]
fn cherry_pick_applies_and_stops_on_conflicts() {
    let dir = repo("pick");
    git(&dir, &["checkout", "-q", "-b", "feature"]);
    write(&dir, "feature.txt", "feature\n");
    commit(&dir, "add feature");
    let feature = git(&dir, &["rev-parse", "HEAD"]).trim().to_owned();
    write(&dir, "readme.txt", "feature side\n");
    commit(&dir, "change readme on feature");
    let readme_change = git(&dir, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&dir, &["checkout", "-q", "main"]);
    write(&dir, "readme.txt", "main side\n");
    commit(&dir, "change readme on main");
    let r = Repo::discover(&dir).unwrap();
    let picked = r.cherry_pick(&feature).unwrap();
    assert!(picked.ok, "{picked:?}");
    assert_eq!(read(&dir, "feature.txt"), "feature\n");
    let stopped = r.cherry_pick(&readme_change).unwrap();
    assert!(!stopped.ok);
    assert_eq!(stopped.conflicts, ["readme.txt"]);
    assert_eq!(stopped.in_progress, Some(Operation::CherryPick));
    assert!(
        r.rebase("feature").is_err(),
        "nothing else starts while one is in progress"
    );
    let aborted = r.abort_operation().unwrap();
    assert!(aborted.ok && aborted.in_progress.is_none(), "{aborted:?}");
    assert_eq!(read(&dir, "readme.txt"), "main side\n");
    assert!(r.abort_operation().is_err(), "nothing to abort");
}

#[test]
fn rebase_replays_and_continues_after_resolving() {
    let dir = repo("rebase");
    git(&dir, &["checkout", "-q", "-b", "feature"]);
    write(&dir, "readme.txt", "feature\n");
    commit(&dir, "feature readme");
    write(&dir, "other.txt", "other\n");
    commit(&dir, "other file");
    git(&dir, &["checkout", "-q", "main"]);
    write(&dir, "readme.txt", "main\n");
    commit(&dir, "main readme");
    git(&dir, &["checkout", "-q", "feature"]);
    let r = Repo::discover(&dir).unwrap();
    let preview = r.rebase_preview("main").unwrap();
    assert_eq!(preview.len(), 2);
    assert!(preview[0].ends_with("feature readme"), "{preview:?}");
    let stopped = r.rebase("main").unwrap();
    assert!(!stopped.ok);
    assert_eq!(stopped.conflicts, ["readme.txt"]);
    assert_eq!(r.operation_in_progress(), Some(Operation::Rebase));
    assert_eq!(r.rebasing_branch().as_deref(), Some("feature"));
    let readme = r.conflicts().unwrap();
    assert_eq!(
        readme[0].ours.as_deref(),
        Some("main\n"),
        "HEAD is the target"
    );
    assert_eq!(readme[0].theirs.as_deref(), Some("feature\n"));
    write(&dir, "readme.txt", "both\n");
    git(&dir, &["add", "readme.txt"]);
    let done = r.continue_operation().unwrap();
    assert!(done.ok && done.in_progress.is_none(), "{done:?}");
    let log = git(&dir, &["log", "--format=%s"]);
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        ["other file", "feature readme", "main readme", "first"]
    );
    assert_eq!(read(&dir, "readme.txt"), "both\n");
    assert_eq!(r.rebasing_branch(), None);
}

#[test]
fn stash_files_show_tracked_and_untracked_changes() {
    let dir = repo("stash-files");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "readme.txt", "two\n");
    write(&dir, "new.txt", "untracked\n");
    assert!(r.stash_push(None, true).unwrap().ok);
    let files = r.stash_files(0).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["new.txt", "readme.txt"]);
    assert_eq!(
        (files[0].old.as_str(), files[0].new.as_str()),
        ("", "untracked\n")
    );
    assert_eq!(
        (files[1].old.as_str(), files[1].new.as_str()),
        ("one\n", "two\n")
    );
    assert!(r.stash_files(3).is_err());
}

#[test]
fn interactive_rebase_reorders_rewords_squashes_and_drops() {
    let dir = repo("interactive");
    let r = Repo::discover(&dir).unwrap();
    for (file, msg) in [
        ("a.txt", "add a"),
        ("b.txt", "add b"),
        ("c.txt", "add c"),
        ("d.txt", "add d"),
    ] {
        write(&dir, file, &format!("{file}\n"));
        commit(&dir, msg);
    }
    let base = git(&dir, &["rev-list", "--max-parents=0", "HEAD"])
        .trim()
        .to_owned();
    let mut todo = r.rebase_todo(&base).unwrap();
    let subjects: Vec<&str> = todo.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(subjects, ["add a", "add b", "add c", "add d"]);
    // d first, a reworded, b squashed into a, c dropped.
    todo.rotate_right(1);
    todo[1].action = TodoAction::Reword;
    todo[1].message = Some("add a, better".into());
    todo[2].action = TodoAction::Fixup;
    todo[3].action = TodoAction::Drop;
    let done = r.rebase_interactive(&base, &todo).unwrap();
    assert!(done.ok, "{}", done.output);
    let log = git(&dir, &["log", "--format=%s", &format!("{base}..HEAD")]);
    assert_eq!(log, "add a, better\nadd d\n");
    assert!(dir.join("b.txt").exists());
    assert!(!dir.join("c.txt").exists());

    // Refusals: a squash first, a missing commit, reword without a message, an option as base.
    let mut bad = r.rebase_todo(&base).unwrap();
    bad[0].action = TodoAction::Squash;
    assert!(r.rebase_interactive(&base, &bad).is_err());
    let short = r.rebase_todo(&base).unwrap()[1..].to_vec();
    assert!(r.rebase_interactive(&base, &short).is_err());
    let mut quiet = r.rebase_todo(&base).unwrap();
    quiet[0].action = TodoAction::Reword;
    assert!(r.rebase_interactive(&base, &quiet).is_err());
    assert!(r.rebase_todo("--exec=touch x").is_err());
    assert_eq!(git(&dir, &["log", "--format=%s", "-1"]), "add a, better\n");
}

#[test]
fn interactive_rebase_stops_on_conflicts_and_continues() {
    let dir = repo("interactive-conflict");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "readme.txt", "two\n");
    commit(&dir, "two");
    write(&dir, "readme.txt", "three\n");
    commit(&dir, "three");
    let base = git(&dir, &["rev-list", "--max-parents=0", "HEAD"])
        .trim()
        .to_owned();
    let mut todo = r.rebase_todo(&base).unwrap();
    todo.swap(0, 1);
    let stopped = r.rebase_interactive(&base, &todo).unwrap();
    assert!(!stopped.ok);
    assert_eq!(stopped.conflicts, ["readme.txt"]);
    assert_eq!(stopped.in_progress, Some(Operation::Rebase));
    assert!(r.abort_operation().unwrap().ok);
    assert_eq!(read(&dir, "readme.txt"), "three\n");
}

/// Hand test 71: a squash keeps both messages — the first as the subject, the squashed one in the
/// body (SPEC-0145 behaviour 4); the log's subject column alone shows only the first.
#[test]
fn interactive_squash_keeps_both_messages() {
    let dir = repo("interactive-squash");
    let r = Repo::discover(&dir).unwrap();
    for (file, msg) in [("e.txt", "Eins"), ("z.txt", "Zwei"), ("d.txt", "Drei")] {
        write(&dir, file, &format!("{file}\n"));
        commit(&dir, msg);
    }
    let base = git(&dir, &["rev-parse", "HEAD~3"]).trim().to_owned();
    let mut todo = r.rebase_todo(&base).unwrap();
    // Zwei first, then Eins with Drei squashed into it.
    todo.swap(0, 1);
    todo[2].action = TodoAction::Squash;
    let done = r.rebase_interactive(&base, &todo).unwrap();
    assert!(done.ok, "{}", done.output);
    assert_eq!(
        git(&dir, &["log", "--format=%s", &format!("{base}..HEAD")]),
        "Eins\nZwei\n"
    );
    let body = git(&dir, &["log", "-1", "--format=%B"]);
    assert_eq!(body.trim(), "Eins\n\nDrei", "{body:?}");
}
