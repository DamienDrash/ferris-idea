//! SPEC-0063 v5 AK-5: amend, author, rollback and ignored files on scratch repositories built with
//! `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use idecraft_vcs::{CommitOptions, GitError, Repo, parse_author};

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
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A repository with `readme.txt` committed, then `second.txt` in a second commit.
fn repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-cw-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test Author"]);
    git(&dir, &["config", "user.email", "test@example.invalid"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    write(&dir, "readme.txt", "one\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    write(&dir, "second.txt", "two\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "second\n\nbody line"]);
    dir
}

fn write(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap()
}

fn amend() -> CommitOptions {
    CommitOptions {
        amend: true,
        ..CommitOptions::default()
    }
}

#[test]
fn amend_replaces_the_last_commit() {
    let dir = repo("amend");
    let r = Repo::discover(&dir).unwrap();
    let parent = git(&dir, &["rev-parse", "HEAD~1"]);
    let old = git(&dir, &["rev-parse", "HEAD"]);
    // The last commit's whole message, for the amend switch.
    assert_eq!(
        r.commit_details("HEAD").unwrap().message.trim_end(),
        "second\n\nbody line"
    );

    // Message only: no file needed, same parent, same tree, the author kept.
    let done = r
        .commit_opts("second, reworded", &[], false, &amend())
        .unwrap();
    assert_eq!(done.commit.unwrap().summary, "second, reworded");
    assert_ne!(git(&dir, &["rev-parse", "HEAD"]), old);
    assert_eq!(git(&dir, &["rev-parse", "HEAD~1"]), parent);
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%an <%ae>"]),
        "Test Author <test@example.invalid>"
    );
    assert_eq!(git(&dir, &["show", "HEAD:second.txt"]), "two");

    // With a file: the tree gains it, the parent stays.
    write(&dir, "readme.txt", "one, amended\n");
    r.commit_opts("second", &["readme.txt".into()], false, &amend())
        .unwrap();
    assert_eq!(git(&dir, &["rev-parse", "HEAD~1"]), parent);
    assert_eq!(git(&dir, &["show", "HEAD:readme.txt"]), "one, amended");
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
    // The branch moved, HEAD still points at it.
    assert_eq!(git(&dir, &["symbolic-ref", "HEAD"]), "refs/heads/main");

    // An author given replaces the amended commit's author.
    let with_author = CommitOptions {
        author: Some("Other Person <other@example.invalid>".into()),
        ..amend()
    };
    r.commit_opts("second", &[], false, &with_author).unwrap();
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%an <%ae>"]),
        "Other Person <other@example.invalid>"
    );
    assert_eq!(git(&dir, &["rev-parse", "HEAD~1"]), parent);
}

#[test]
fn amend_needs_a_commit_and_a_message() {
    let dir = std::env::temp_dir().join(format!("idecraft-cw-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    let r = Repo::discover(&dir).unwrap();
    assert!(matches!(
        r.commit_opts("m", &[], false, &amend()),
        Err(GitError::Rejected(_))
    ));
    let dir = repo("amend-empty");
    let r = Repo::discover(&dir).unwrap();
    assert!(matches!(
        r.commit_opts("  ", &[], false, &amend()),
        Err(GitError::Rejected(_))
    ));
    // Without amend, no files is still refused.
    assert!(matches!(
        r.commit_opts("m", &[], false, &CommitOptions::default()),
        Err(GitError::Rejected(_))
    ));
}

#[test]
fn author_override_and_invalid_author() {
    let dir = repo("author");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "readme.txt", "changed\n");
    let bad = CommitOptions {
        author: Some("no email".into()),
        ..CommitOptions::default()
    };
    let refused = r.commit_opts("m", &["readme.txt".into()], false, &bad);
    assert!(matches!(refused, Err(GitError::Rejected(m)) if m.contains("Name <email>")));
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "2");
    let good = CommitOptions {
        author: Some(" Ada Lovelace <ada@example.invalid> ".into()),
        ..CommitOptions::default()
    };
    r.commit_opts("by Ada", &["readme.txt".into()], false, &good)
        .unwrap();
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%an <%ae>|%cn"]),
        "Ada Lovelace <ada@example.invalid>|Test Author"
    );
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "3");
    assert_eq!(
        parse_author("A <a@b>").unwrap(),
        ("A".to_owned(), "a@b".to_owned())
    );
    assert!(parse_author("<a@b>").is_err() && parse_author("A <>").is_err());
}

#[test]
fn rollback_restores_unstages_and_deletes() {
    let dir = repo("rollback");
    let r = Repo::discover(&dir).unwrap();
    write(&dir, "readme.txt", "edited\n");
    std::fs::remove_file(dir.join("second.txt")).unwrap();
    write(&dir, "added.txt", "new\n");
    write(&dir, "other.txt", "new too\n");
    git(&dir, &["add", "added.txt", "other.txt"]);
    write(&dir, "loose.txt", "untracked\n");

    // Untracked files are not rolled back.
    assert!(r.rollback(&["loose.txt".into()], false, true).is_err());
    let paths: Vec<String> = ["readme.txt", "second.txt", "added.txt"]
        .map(String::from)
        .to_vec();
    let preview = r.rollback(&paths, false, true).unwrap();
    assert!(!preview.done);
    assert_eq!(preview.restored, ["readme.txt", "second.txt"]);
    assert_eq!(preview.unstaged, ["added.txt"]);
    assert_eq!(
        read(&dir, "readme.txt"),
        "edited\n",
        "a dry run changes nothing"
    );

    let done = r.rollback(&paths, false, false).unwrap();
    assert!(done.done);
    assert_eq!(read(&dir, "readme.txt"), "one\n");
    assert_eq!(read(&dir, "second.txt"), "two\n");
    assert_eq!(
        read(&dir, "added.txt"),
        "new\n",
        "an added file stays on disk"
    );
    let status = git(&dir, &["status", "--porcelain"]);
    assert_eq!(
        status.lines().collect::<Vec<_>>(),
        ["A  other.txt", "?? added.txt", "?? loose.txt"]
    );

    // With the option an added file is deleted.
    let deleted = r.rollback(&["other.txt".into()], true, false).unwrap();
    assert_eq!(deleted.deleted, ["other.txt"]);
    assert!(!dir.join("other.txt").exists());
    assert_eq!(
        git(&dir, &["status", "--porcelain"])
            .lines()
            .collect::<Vec<_>>(),
        ["?? added.txt", "?? loose.txt"]
    );
    assert!(matches!(
        r.rollback(&[], false, false),
        Err(GitError::Rejected(_))
    ));
}

#[test]
fn ignored_files_are_not_listed() {
    let dir = repo("ignored");
    write(&dir, ".gitignore", "*.log\nbuild/\n");
    git(&dir, &["add", ".gitignore"]);
    git(&dir, &["commit", "-q", "-m", "ignore"]);
    write(&dir, "debug.log", "x\n");
    std::fs::create_dir_all(dir.join("build")).unwrap();
    write(&dir, "build/out.bin", "x\n");
    std::fs::create_dir_all(dir.join(".git/info")).unwrap();
    write(&dir, ".git/info/exclude", "secret.txt\n");
    write(&dir, "secret.txt", "x\n");
    write(&dir, "kept.txt", "x\n");
    let status = Repo::discover(&dir).unwrap().status().unwrap();
    let paths: Vec<&str> = status.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths, ["kept.txt"]);
}
