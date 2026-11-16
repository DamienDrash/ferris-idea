//! SPEC-0055 AK-1 – AK-5 with a bare repository as remote and two clones, built with `git`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use idecraft_vcs::{MergeKind, PushOptions, Repo};

const WAIT: Duration = Duration::from_secs(60);

struct Setup {
    dir: PathBuf,
}

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

impl Setup {
    /// `remote.git` (bare), `a` and `b` cloned from it with one commit on `main`.
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("idecraft-remote-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        git(&dir, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(&dir, &["clone", "-q", "remote.git", "a"]);
        let s = Self { dir };
        s.configure("a");
        s.write("a", "readme.txt", "one\n");
        s.commit("a", "first");
        git(&s.path("a"), &["push", "-q", "origin", "main"]);
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
fn remotes_and_tracking() {
    let s = Setup::new("tracking");
    let a = s.repo("a");
    let remotes = a.remotes().unwrap();
    assert_eq!(remotes.len(), 1);
    assert_eq!(remotes[0].name, "origin");
    assert!(remotes[0].fetch_url.ends_with("remote.git"));
    assert_eq!(remotes[0].push_url, remotes[0].fetch_url);

    // Two local commits ahead; b pushes one, a fetches: one behind as well.
    s.write("a", "a.txt", "1\n");
    s.commit("a", "a1");
    s.write("a", "a.txt", "2\n");
    s.commit("a", "a2");
    s.write("b", "b.txt", "x\n");
    s.commit("b", "b1");
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    let fetch = a.fetch(None, WAIT).unwrap();
    assert!(fetch.ok, "{fetch:?}");
    let main = a
        .tracking()
        .unwrap()
        .into_iter()
        .find(|t| t.branch == "main")
        .unwrap();
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert!(main.current);
    assert_eq!((main.ahead, main.behind), (2, 1));
    // Same numbers as git itself reports.
    let status = git(&s.path("a"), &["status", "-sb"]);
    assert!(status.contains("[ahead 2, behind 1]"), "{status}");

    // A branch without upstream, and one whose upstream is gone.
    git(&s.path("a"), &["branch", "local-only"]);
    git(&s.path("b"), &["push", "-q", "origin", "main:temp"]);
    a.fetch(Some("origin"), WAIT).unwrap();
    git(
        &s.path("a"),
        &["branch", "-q", "--track", "follow", "origin/temp"],
    );
    git(&s.path("b"), &["push", "-q", "origin", "--delete", "temp"]);
    let fetch = a.fetch(Some("origin"), WAIT).unwrap();
    assert!(
        fetch
            .updates
            .iter()
            .any(|u| u.kind == "pruned" && u.reference == "refs/remotes/origin/temp"),
        "{fetch:?}"
    );
    let t = a.tracking().unwrap();
    let local = t.iter().find(|t| t.branch == "local-only").unwrap();
    assert_eq!(local.upstream, None);
    let follow = t.iter().find(|t| t.branch == "follow").unwrap();
    assert!(follow.gone, "{follow:?}");
}

#[test]
fn pull_fast_forward_merge_and_conflict() {
    let s = Setup::new("pull");
    let a = s.repo("a");
    // Fast-forward.
    s.write("b", "b.txt", "from b\n");
    s.commit("b", "b1");
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    let preview = a.pull(true, WAIT).unwrap();
    assert_eq!(preview.merge.as_ref().unwrap().kind, MergeKind::FastForward);
    assert!(
        !s.path("a").join("b.txt").exists(),
        "a preview changes no file"
    );
    let pulled = a.pull(false, WAIT).unwrap();
    assert!(pulled.merge.unwrap().merged);
    assert_eq!(
        std::fs::read_to_string(s.path("a").join("b.txt")).unwrap(),
        "from b\n"
    );

    // Diverged without conflict: a merge commit.
    s.write("a", "a.txt", "from a\n");
    s.commit("a", "a1");
    s.write("b", "c.txt", "more from b\n");
    s.commit("b", "b2");
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    let merged = a.pull(false, WAIT).unwrap();
    let m = merged.merge.unwrap();
    assert_eq!(m.kind, MergeKind::Merge);
    assert!(m.merged);
    assert_eq!(
        git(&s.path("a"), &["rev-list", "--count", "--merges", "HEAD"]).trim(),
        "1"
    );

    // Conflict: reported, nothing changed.
    s.write("a", "readme.txt", "a's line\n");
    s.commit("a", "a2");
    s.write("b", "readme.txt", "b's line\n");
    s.commit("b", "b3");
    git(
        &s.path("b"),
        &["pull", "-q", "--no-rebase", "origin", "main"],
    );
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    let head = git(&s.path("a"), &["rev-parse", "HEAD"]);
    let conflict = a.pull(false, WAIT).unwrap();
    let c = conflict.merge.unwrap();
    assert_eq!(c.kind, MergeKind::Conflict);
    assert_eq!(c.merge_conflicts, ["readme.txt"]);
    assert_eq!(git(&s.path("a"), &["rev-parse", "HEAD"]), head);

    // No upstream: refused before any network access.
    git(&s.path("a"), &["checkout", "-q", "-b", "lonely"]);
    assert!(a.pull(false, WAIT).is_err());
}

#[test]
fn push_new_branch_rejection_and_lease() {
    let s = Setup::new("push");
    let a = s.repo("a");
    git(&s.path("a"), &["checkout", "-q", "-b", "feature"]);
    s.write("a", "f.txt", "feature\n");
    s.commit("a", "f1");
    let preview = a.push_preview(None, None, WAIT).unwrap();
    assert_eq!(preview.remote, "origin");
    assert_eq!(preview.commits.len(), 1);
    assert_eq!(preview.commits[0].summary, "f1");
    assert_eq!(preview.dry_run.updates[0].kind, "new");
    assert!(
        git(
            &s.dir,
            &["--git-dir", "remote.git", "branch", "--list", "feature"]
        )
        .is_empty()
    );

    let pushed = a.push(None, None, true, None, WAIT).unwrap();
    assert!(pushed.ok, "{pushed:?}");
    assert_eq!(pushed.updates[0].kind, "new");
    let t = a.tracking().unwrap();
    let feature = t.iter().find(|t| t.branch == "feature").unwrap();
    assert_eq!(feature.upstream.as_deref(), Some("origin/feature"));
    assert_eq!(feature.ahead, 0);

    // Someone else pushes to main; a's push of main is rejected and says why.
    s.write("b", "b.txt", "b\n");
    s.commit("b", "b1");
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    git(&s.path("a"), &["checkout", "-q", "main"]);
    s.write("a", "a.txt", "a\n");
    s.commit("a", "a1");
    let preview = a.push_preview(None, None, WAIT).unwrap();
    assert_eq!(preview.dry_run.updates[0].kind, "rejected", "{preview:?}");
    let rejected = a.push(None, None, false, None, WAIT).unwrap();
    assert!(!rejected.ok);
    assert_eq!(rejected.updates[0].kind, "rejected");
    assert!(
        rejected.updates[0].summary.contains("fetch first")
            || rejected.updates[0].summary.contains("non-fast-forward"),
        "{rejected:?}"
    );

    // Force only with a lease: a stale expectation is refused, the right one overwrites.
    let remote_main = git(&s.dir, &["--git-dir", "remote.git", "rev-parse", "main"]);
    let stale = git(&s.path("a"), &["rev-parse", "HEAD~1"]);
    let refused = a.push(None, None, false, Some(stale.trim()), WAIT).unwrap();
    assert!(!refused.ok, "{refused:?}");
    let forced = a
        .push(None, None, false, Some(remote_main.trim()), WAIT)
        .unwrap();
    assert!(forced.ok, "{forced:?}");
    assert_eq!(forced.updates[0].kind, "forced");
    assert!(a.push(Some("nope"), None, false, None, WAIT).is_err());
    assert!(a.push(None, Some("upstream"), false, None, WAIT).is_err());
}

/// A remote that wants credentials fails at once instead of waiting for a prompt (IMP-0029).
#[test]
fn credentials_are_never_prompted_for() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let s = Setup::new("auth");
    let url = format!("http://127.0.0.1:{port}/private.git");
    git(&s.path("a"), &["remote", "add", "private", &url]);
    // No helper may answer for this test host.
    git(&s.path("a"), &["config", "credential.helper", ""]);
    let started = std::time::Instant::now();
    let fetch = s.repo("a").fetch(Some("private"), WAIT).unwrap();
    assert!(!fetch.ok);
    assert!(fetch.auth_required, "{fetch:?}");
    assert!(!fetch.timed_out);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn outgoing_lists_unpushed_commits_without_network() {
    let s = Setup::new("outgoing");
    let a = s.repo("a");
    let none = a.outgoing().unwrap();
    assert_eq!(
        (none.branch.as_str(), none.remote.as_str()),
        ("main", "origin")
    );
    assert!(none.has_upstream && none.commits.is_empty());
    s.write("a", "a.txt", "1\n");
    s.commit("a", "first local");
    s.write("a", "a.txt", "2\n");
    s.commit("a", "second local");
    let two = a.outgoing().unwrap();
    let subjects: Vec<&str> = two.commits.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(subjects, ["second local", "first local"], "newest first");
    // A new branch has no upstream yet; Push will set it on origin.
    git(&s.path("a"), &["switch", "-q", "-c", "topic"]);
    let topic = a.outgoing().unwrap();
    assert_eq!(
        (topic.branch.as_str(), topic.remote.as_str()),
        ("topic", "origin")
    );
    assert!(!topic.has_upstream);
    assert_eq!(topic.commits.len(), 2);
}

#[test]
fn update_with_rebase_keeps_history_linear_and_local_changes() {
    let s = Setup::new("pull-rebase");
    let a = s.repo("a");
    s.write("a", "a.txt", "from a\n");
    s.commit("a", "a1");
    s.write("b", "b.txt", "from b\n");
    s.commit("b", "b1");
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    // An uncommitted change survives through --autostash.
    s.write("a", "readme.txt", "local edit\n");
    let preview = a.pull_rebase(true, WAIT).unwrap();
    assert_eq!(preview.replayed.len(), 1, "{preview:?}");
    assert!(preview.replayed[0].ends_with(" a1"));
    assert!(preview.rebase.is_none());
    let done = a.pull_rebase(false, WAIT).unwrap();
    assert!(done.rebase.as_ref().unwrap().ok, "{done:?}");
    assert_eq!(
        git(&s.path("a"), &["rev-list", "--count", "--merges", "HEAD"]).trim(),
        "0"
    );
    assert!(s.path("a").join("b.txt").exists());
    assert_eq!(
        std::fs::read_to_string(s.path("a").join("readme.txt")).unwrap(),
        "local edit\n"
    );

    // A conflict stops the rebase and leaves it waiting.
    s.write("a", "readme.txt", "a's line\n");
    s.commit("a", "a2");
    s.write("b", "readme.txt", "b's line\n");
    s.commit("b", "b2");
    git(
        &s.path("b"),
        &["pull", "-q", "--no-rebase", "origin", "main"],
    );
    git(&s.path("b"), &["push", "-q", "origin", "main"]);
    let stopped = a.pull_rebase(false, WAIT).unwrap();
    let r = stopped.rebase.unwrap();
    assert!(!r.ok);
    assert_eq!(r.conflicts, ["readme.txt"]);
    assert!(r.in_progress.is_some());
    assert!(a.abort_operation().unwrap().ok);
}

#[test]
fn push_options_target_tags_and_lease() {
    let s = Setup::new("push-options");
    let a = s.repo("a");
    s.write("a", "a.txt", "one\n");
    s.commit("a", "a1");
    git(&s.path("a"), &["tag", "-a", "v1", "-m", "v1"]);
    // Hand test 70 (D35): a lightweight tag goes with the push as well.
    git(&s.path("a"), &["tag", "light"]);
    let pushed = a
        .push_with(
            &PushOptions {
                target: Some("review/a".into()),
                tags: true,
                ..PushOptions::default()
            },
            WAIT,
        )
        .unwrap();
    assert!(pushed.ok, "{pushed:?}");
    let remote_refs = git(&s.path("b"), &["ls-remote", "origin"]);
    assert!(remote_refs.contains("refs/heads/review/a"), "{remote_refs}");
    assert!(remote_refs.contains("refs/tags/v1"), "{remote_refs}");
    assert!(remote_refs.contains("refs/tags/light"), "{remote_refs}");
    // Pushed again, the tags the remote already has are up to date, not an error.
    let again = a
        .push_with(
            &PushOptions {
                target: Some("review/a".into()),
                tags: true,
                ..PushOptions::default()
            },
            WAIT,
        )
        .unwrap();
    assert!(again.ok, "{again:?}");

    assert!(a.push_with(&PushOptions::default(), WAIT).unwrap().ok);
    // Rewritten history: a plain push is rejected, a lease push goes through while the remote
    // is where the last fetch saw it, and is refused once someone else pushed.
    git(
        &s.path("a"),
        &["commit", "-q", "--amend", "-m", "a1 amended"],
    );
    let rejected = a.push_with(&PushOptions::default(), WAIT).unwrap();
    assert!(!rejected.ok);
    a.fetch(Some("origin"), WAIT).unwrap();
    let forced = a
        .push_with(
            &PushOptions {
                force_with_lease: true,
                ..PushOptions::default()
            },
            WAIT,
        )
        .unwrap();
    assert!(forced.ok, "{forced:?}");
    git(
        &s.path("b"),
        &[
            "pull",
            "-q",
            "--rebase=false",
            "-X",
            "theirs",
            "origin",
            "main",
        ],
    );
    s.write("b", "b.txt", "two\n");
    s.commit("b", "b1");
    git(&s.path("b"), &["push", "-q", "origin", "HEAD:main"]);
    git(&s.path("a"), &["commit", "-q", "--amend", "-m", "a1 again"]);
    let stale = a
        .push_with(
            &PushOptions {
                force_with_lease: true,
                ..PushOptions::default()
            },
            WAIT,
        )
        .unwrap();
    assert!(!stale.ok, "{stale:?}");
    // Hand test 70: the rejection carries git's reason for the branch.
    assert!(
        stale
            .updates
            .iter()
            .any(|u| u.kind == "rejected" && u.summary.contains("stale info")),
        "{stale:?}"
    );

    assert!(
        a.push_with(
            &PushOptions {
                target: Some("--delete".into()),
                ..PushOptions::default()
            },
            WAIT,
        )
        .is_err()
    );
}
