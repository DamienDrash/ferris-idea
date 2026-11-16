//! SPEC-0230 AK-1 – AK-4: commit hooks on scratch repositories built with `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use idecraft_vcs::{GitError, Repo};

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
    let dir = std::env::temp_dir().join(format!("idecraft-hooks-{tag}-{}", std::process::id()));
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

fn hook(dir: &Path, name: &str, script: &str) {
    let path = dir.join(".git/hooks").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

#[test]
fn failing_pre_commit_blocks_and_no_verify_skips_it() {
    let dir = repo("block");
    let r = Repo::discover(&dir).unwrap();
    hook(
        &dir,
        "pre-commit",
        "echo checking >&2\necho 'trailing space in readme.txt'\nexit 1\n",
    );
    write(&dir, "readme.txt", "two\n");
    let before = head(&dir);
    let dry = r.commit("change", &["readme.txt".into()], true).unwrap();
    assert_eq!(dry.hooks.len(), 1);
    assert_eq!(dry.hooks[0].hook, "pre-commit");
    assert_eq!(dry.hooks[0].exit, None, "a dry run runs no hook");
    match r.commit("change", &["readme.txt".into()], false) {
        Err(GitError::HookFailed {
            hook,
            status,
            output,
        }) => {
            assert_eq!(hook, "pre-commit");
            assert_eq!(status, "exit 1");
            assert_eq!(output, "checking\ntrailing space in readme.txt");
        }
        other => panic!("expected a blocked commit: {other:?}"),
    }
    assert_eq!(head(&dir), before, "nothing committed");
    assert_eq!(read(&dir, "readme.txt"), "two\n");
    assert_eq!(
        git(&dir, &["status", "--porcelain"]),
        " M readme.txt\n",
        "the user's index is unchanged"
    );
    let done = r
        .commit_with("change", &["readme.txt".into()], false, false)
        .unwrap();
    assert!(done.commit.is_some());
    assert!(done.hooks.is_empty(), "pre-commit skipped");
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
}

#[test]
fn pre_commit_staging_a_formatted_file_is_committed() {
    let dir = repo("format");
    let r = Repo::discover(&dir).unwrap();
    // A formatter: upper-cases the staged files and stages them again.
    hook(
        &dir,
        "pre-commit",
        "for f in $(git diff --cached --name-only); do tr a-z A-Z < \"$f\" > \"$f.tmp\" && mv \"$f.tmp\" \"$f\" && git add \"$f\"; done\n",
    );
    write(&dir, "readme.txt", "two\n");
    write(&dir, "other.txt", "staged by the user\n");
    git(&dir, &["add", "other.txt"]);
    let done = r.commit("format", &["readme.txt".into()], false).unwrap();
    assert!(done.commit.is_some());
    assert_eq!(done.hooks[0].exit, Some(0));
    assert_eq!(git(&dir, &["show", "HEAD:readme.txt"]), "TWO\n");
    assert_eq!(read(&dir, "readme.txt"), "TWO\n");
    assert_eq!(
        git(&dir, &["show", "--name-only", "--format=", "HEAD"]),
        "readme.txt\n",
        "the user's staged file is not part of the commit"
    );
    assert_eq!(
        git(&dir, &["status", "--porcelain"]),
        "A  other.txt\n",
        "and stays staged; readme.txt is clean"
    );
}

#[test]
fn commit_msg_rewrites_and_post_commit_reports() {
    let dir = repo("msg");
    let r = Repo::discover(&dir).unwrap();
    hook(&dir, "commit-msg", "echo 'Refs: T-1' >> \"$1\"\n");
    hook(&dir, "post-commit", "echo done; exit 3\n");
    write(&dir, "readme.txt", "two\n");
    let done = r.commit("change", &["readme.txt".into()], false).unwrap();
    assert!(
        done.commit.is_some(),
        "post-commit's failure keeps the commit"
    );
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%B"]).trim_end(),
        "change\nRefs: T-1"
    );
    let post = done.hooks.iter().find(|h| h.hook == "post-commit").unwrap();
    assert_eq!((post.exit, post.output.as_str()), (Some(3), "done"));

    hook(&dir, "commit-msg", ": > \"$1\"\n");
    write(&dir, "readme.txt", "three\n");
    let refused = r.commit("change", &["readme.txt".into()], false);
    assert!(
        matches!(&refused, Err(GitError::Rejected(m)) if m.contains("empty")),
        "{refused:?}"
    );
}

#[test]
fn hooks_path_and_non_executable_hooks() {
    let dir = repo("path");
    let r = Repo::discover(&dir).unwrap();
    // Not executable: ignored.
    std::fs::write(dir.join(".git/hooks/pre-commit"), "#!/bin/sh\nexit 1\n").unwrap();
    assert!(r.commit_hooks(true).is_empty());
    git(&dir, &["config", "core.hooksPath", "ci-hooks"]);
    std::fs::create_dir_all(dir.join("ci-hooks")).unwrap();
    let path = dir.join("ci-hooks/pre-commit");
    std::fs::write(&path, "#!/bin/sh\necho from hooksPath\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(r.commit_hooks(true), vec!["pre-commit".to_owned()]);
    assert!(r.commit_hooks(false).is_empty());
    write(&dir, "readme.txt", "two\n");
    let blocked = r.commit("change", &["readme.txt".into()], false);
    assert!(
        matches!(&blocked, Err(GitError::HookFailed { output, .. }) if output == "from hooksPath"),
        "{blocked:?}"
    );
}
