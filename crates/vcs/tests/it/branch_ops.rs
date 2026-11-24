//! SPEC-0068 v5 AK-8, SPEC-0106 v3 AK-8, SPEC-0144 v4 AK-8/AK-9, SPEC-0109 v2 AK-6: branch popup
//! data, branch operations, stash options, update information, reset to the upstream and the
//! files per outgoing commit, against a bare remote and two clones built with `git`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use ferris_idea_vcs::{RECENT_BRANCHES, Repo, StashOptions};

const WAIT: Duration = Duration::from_secs(60);

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

struct Setup {
    dir: PathBuf,
}

impl Setup {
    /// `remote.git` (bare), `a` and `b` cloned from it with one commit on `main`.
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-branch-ops-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        git(&dir, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(&dir, &["clone", "-q", "remote.git", "a"]);
        let s = Self { dir };
        s.configure("a");
        s.write("a", "readme.txt", "one\n");
        s.commit("a", "first");
        git(&s.path("a"), &["push", "-q", "-u", "origin", "main"]);
        git(&s.dir, &["clone", "-q", "remote.git", "b"]);
        s.configure("b");
        s
    }

    fn path(&self, clone: &str) -> PathBuf {
        self.dir.join(clone)
    }

    fn configure(&self, clone: &str) {
        let p = self.path(clone);
        git(&p, &["config", "user.name", "Test Author"]);
        git(&p, &["config", "user.email", "test@example.invalid"]);
        git(&p, &["config", "commit.gpgsign", "false"]);
        git(&p, &["checkout", "-q", "-B", "main"]);
    }

    fn write(&self, clone: &str, file: &str, text: &str) {
        std::fs::write(self.path(clone).join(file), text).unwrap();
    }

    fn commit(&self, clone: &str, message: &str) {
        git(&self.path(clone), &["add", "-A"]);
        git(&self.path(clone), &["commit", "-q", "-m", message]);
    }

    fn repo(&self, clone: &str) -> Repo {
        Repo::discover(&self.path(clone)).unwrap()
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn tags_recent_branches_and_detached_checkout() {
    let s = Setup::new("popup");
    let a = s.repo("a");
    let dir = s.path("a");
    git(&dir, &["tag", "v1.0"]);
    git(&dir, &["tag", "-a", "-m", "annotated", "v2.0"]);
    let head = git(&dir, &["rev-parse", "--short=7", "HEAD"]);
    let tags = a.tags().unwrap();
    let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["v1.0", "v2.0"]);
    assert!(
        tags.iter().all(|t| t.target == head.trim()),
        "annotated tags are peeled: {tags:?}"
    );
    for b in ["feature/one", "feature/two", "fix"] {
        git(&dir, &["branch", b]);
    }
    git(&dir, &["checkout", "-q", "feature/one"]);
    git(&dir, &["checkout", "-q", "fix"]);
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["branch", "gone"]);
    git(&dir, &["checkout", "-q", "gone"]);
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["branch", "-q", "-D", "gone"]);
    assert_eq!(
        a.recent_branches(RECENT_BRANCHES).unwrap(),
        ["main", "fix", "feature/one"],
        "newest first, the current branch included, deleted ones left out (v10)"
    );
    assert_eq!(a.recent_branches(1).unwrap(), ["main"]);
    // The current branch stays first after a rename the reflog does not name as a checkout.
    git(&dir, &["branch", "-q", "-m", "main", "trunk"]);
    assert_eq!(
        a.recent_branches(RECENT_BRANCHES).unwrap(),
        ["trunk", "fix", "feature/one"]
    );
    git(&dir, &["branch", "-q", "-m", "trunk", "main"]);
    // A tag checks out as a detached HEAD; an option-like revision is refused.
    let done = a.checkout_revision("v1.0").unwrap();
    assert!(done.ok, "{done:?}");
    assert!(a.branches().unwrap().detached);
    assert!(a.checkout_revision("--orphan").is_err());
    assert!(a.checkout_revision("no-such-rev").is_err());
    git(&dir, &["checkout", "-q", "main"]);
}

#[test]
fn delete_rename_and_merged() {
    let s = Setup::new("delete");
    let a = s.repo("a");
    let dir = s.path("a");
    git(&dir, &["checkout", "-q", "-b", "topic"]);
    s.write("a", "t.txt", "t\n");
    s.commit("a", "topic work");
    git(&dir, &["checkout", "-q", "main"]);
    assert!(!a.is_merged("topic").unwrap());
    let refused = a.delete_branch("topic").unwrap_err().to_string();
    assert!(refused.contains("not fully merged"), "{refused}");
    git(&dir, &["merge", "-q", "--no-edit", "topic"]);
    assert!(a.is_merged("topic").unwrap());
    assert!(a.delete_branch("main").is_err(), "the current branch stays");
    // Checkout and rebase onto main: topic ends on top of main's new commit.
    git(&dir, &["checkout", "-q", "-b", "side", "HEAD~1"]);
    s.write("a", "side.txt", "s\n");
    s.commit("a", "side work");
    git(&dir, &["checkout", "-q", "main"]);
    let done = a.checkout_and_rebase("side", "main").unwrap();
    assert!(done.ok, "{done:?}");
    assert_eq!(a.current_branch().unwrap().as_deref(), Some("side"));
    assert!(a.is_merged("main").unwrap());
    git(&dir, &["checkout", "-q", "main"]);
    a.rename_branch("topic", "topic-done").unwrap();
    assert!(a.rename_branch("topic-done", "main").is_err());
    assert!(a.rename_branch("topic-done", "a b").is_err());
    assert!(a.delete_branch("topic-done").unwrap().ok);
    assert!(
        !a.branches()
            .unwrap()
            .branches
            .iter()
            .any(|b| b.name.starts_with("topic"))
    );
}

#[test]
fn stash_keeps_the_index_and_takes_a_message() {
    let s = Setup::new("stash");
    let a = s.repo("a");
    s.write("a", "readme.txt", "staged\n");
    git(&s.path("a"), &["add", "readme.txt"]);
    s.write("a", "new.txt", "untracked\n");
    let done = a
        .stash_push_with(&StashOptions {
            message: Some("half done".into()),
            include_untracked: true,
            keep_index: true,
        })
        .unwrap();
    assert!(done.ok, "{done:?}");
    assert_eq!(a.stashes().unwrap()[0].message, "On main: half done");
    let staged = git(&s.path("a"), &["diff", "--cached", "--name-only"]);
    assert_eq!(staged.trim(), "readme.txt", "the index is kept");
    assert!(!s.path("a").join("new.txt").exists(), "untracked stashed");
    // Add to VCS (SPEC-0109 v2 behaviour 6).
    s.write("a", "added.txt", "a\n");
    a.add_paths(&["added.txt".into()]).unwrap();
    let staged = git(&s.path("a"), &["diff", "--cached", "--name-status"]);
    assert!(staged.contains("A\tadded.txt"), "{staged}");
    assert!(a.add_paths(&["--all".into()]).is_err());
}

#[test]
fn update_info_and_reset_to_upstream() {
    let s = Setup::new("update");
    let a = s.repo("a");
    s.write("b", "b.txt", "from b\n");
    s.commit("b", "b1");
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    let pulled = a.pull(false, WAIT).unwrap();
    let info = pulled.info.expect("HEAD moved");
    assert_eq!(info.commits.len(), 1);
    assert_eq!(info.commits[0].summary, "b1");
    assert_eq!(
        (info.files[0].status.as_str(), info.files[0].path.as_str()),
        ("A", "b.txt")
    );
    assert!(a.pull(false, WAIT).unwrap().info.is_none(), "nothing new");

    // Reset: one local commit is given up, the local change comes back.
    s.write("a", "a.txt", "local commit\n");
    s.commit("a", "mine");
    s.write("b", "b.txt", "again\n");
    s.commit("b", "b2");
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    s.write("a", "readme.txt", "local change\n");
    let preview = a.reset_to_upstream(true, WAIT).unwrap();
    assert_eq!(preview.dropped.len(), 1);
    assert!(
        preview.dropped[0].ends_with(" mine"),
        "{:?}",
        preview.dropped
    );
    assert!(preview.reset.is_none());
    let done = a.reset_to_upstream(false, WAIT).unwrap();
    assert!(done.reset.as_ref().is_some_and(|r| r.ok), "{done:?}");
    let log = git(&s.path("a"), &["log", "--format=%s"]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["b2", "b1", "first"]);
    assert_eq!(
        std::fs::read_to_string(s.path("a").join("readme.txt")).unwrap(),
        "local change\n"
    );
    assert!(a.stashes().unwrap().is_empty());
    let info = done.info.expect("HEAD moved");
    assert!(
        info.files
            .iter()
            .any(|f| f.path == "a.txt" && f.status == "D")
    );
}

#[test]
fn outgoing_names_files_and_remote_branches() {
    let s = Setup::new("outgoing");
    let a = s.repo("a");
    git(&s.path("a"), &["push", "-q", "origin", "main:ziel"]);
    a.fetch(None, WAIT).unwrap();
    s.write("a", "x.txt", "x\n");
    s.write("a", "readme.txt", "two\n");
    s.commit("a", "two files");
    let out = a.outgoing().unwrap();
    assert_eq!(out.commits.len(), 1);
    let files = &out.files[&out.commits[0].id];
    let paths: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f.status.as_str(), f.path.as_str()))
        .collect();
    assert_eq!(paths, [("M", "readme.txt"), ("A", "x.txt")]);
    assert_eq!(out.remote_branches, ["main", "ziel"]);
}
