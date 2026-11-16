//! SPEC-0032 AK-2 – AK-4 on a scratch repository built with the `git` program.

use std::path::{Path, PathBuf};
use std::process::Command;

use idecraft_vcs::{ChangeKind, FileStatus, LineChangeKind, MergeKind, Repo};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Option<Self> {
        let dir = std::env::temp_dir().join(format!("idecraft-git-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Self(dir);
        s.git(&["init", "-q", "-b", "main"])?;
        s.git(&["config", "user.name", "Test Author"])?;
        s.git(&["config", "user.email", "test@example.invalid"])?;
        s.git(&["config", "commit.gpgsign", "false"])?;
        Some(s)
    }

    fn git(&self, args: &[&str]) -> Option<()> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .output()
            .ok()?;
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(())
    }

    fn write(&self, path: &str, text: &[u8]) {
        let p = self.0.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]).unwrap();
        self.git(&["commit", "-q", "-m", message]).unwrap();
    }

    fn out(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn status(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
    FileStatus {
        path: path.into(),
        staged,
        unstaged,
    }
}

#[test]
fn status_of_staged_unstaged_untracked_and_ignored() {
    let Some(s) = Scratch::new("status") else {
        return;
    };
    s.write("keep.txt", b"keep\n");
    s.write("edit.txt", b"one\n");
    s.write("gone.txt", b"bye\n");
    s.write("stage.txt", b"before\n");
    s.write(".gitignore", b"*.log\n");
    s.commit("base");
    s.write("edit.txt", b"one\ntwo\n");
    std::fs::remove_file(s.path().join("gone.txt")).unwrap();
    s.write("stage.txt", b"after\n");
    s.write("new/added.txt", b"new\n");
    s.git(&["add", "stage.txt", "new/added.txt"]).unwrap();
    s.write("untracked.txt", b"?\n");
    s.write("debug.log", b"ignored\n");

    let repo = Repo::discover(&s.path().join("new")).unwrap();
    use ChangeKind::*;
    assert_eq!(
        repo.status().unwrap(),
        [
            status("edit.txt", None, Some(Modified)),
            status("gone.txt", None, Some(Deleted)),
            status("new/added.txt", Some(Added), None),
            status("stage.txt", Some(Modified), None),
            status("untracked.txt", None, Some(Added)),
        ]
    );
}

/// SPEC-0032/AK-6 (v3): the summary counts what the status lists, a path staged and changed in
/// both; an unborn HEAD has a branch and no commit, a detached HEAD a commit and no branch; the
/// repositories below a folder are found by their `.git`, within the depth and the limit.
#[test]
fn summary_of_a_repository_and_the_repositories_below() {
    let Some(s) = Scratch::new("summary") else {
        return;
    };
    let repo = Repo::discover(s.path()).unwrap();
    let unborn = repo.summary().unwrap();
    assert_eq!(
        (
            unborn.branch.as_deref(),
            unborn.detached,
            unborn.head.as_deref(),
            unborn.clean
        ),
        (Some("main"), false, None, true)
    );
    s.write("edit.txt", b"one\n");
    s.write("both.txt", b"one\n");
    s.write(".gitignore", b"*.log\n");
    s.commit("base");
    let clean = repo.summary().unwrap();
    assert!(
        clean.clean && clean.head.as_ref().is_some_and(|h| h.len() == 7),
        "{clean:?}"
    );
    s.write("edit.txt", b"two\n");
    s.write("both.txt", b"two\n");
    s.git(&["add", "both.txt"]).unwrap();
    s.write("both.txt", b"three\n");
    s.write("new.txt", b"?\n");
    s.write("debug.log", b"ignored\n");
    let busy = repo.summary().unwrap();
    assert_eq!(
        (
            busy.staged,
            busy.changed,
            busy.untracked,
            busy.conflicts,
            busy.clean
        ),
        (1, 2, 1, 0, false),
        "{busy:?}"
    );
    s.commit("more");
    s.git(&["checkout", "-q", "--detach"]).unwrap();
    let detached = repo.summary().unwrap();
    assert_eq!((detached.branch, detached.detached), (None, true));
    assert!(detached.head.is_some());

    // Nested working trees: `lib/a` and `tools/deep/b/c` (four levels), one in `node_modules`.
    for nested in ["lib/a", "tools/deep/b/c", "node_modules/x", "lib/a/inner"] {
        std::fs::create_dir_all(s.path().join(nested).join(".git")).unwrap();
    }
    let rel = |found: Vec<PathBuf>| -> Vec<String> {
        found
            .iter()
            .map(|p| {
                p.strip_prefix(s.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    };
    let (all, cut) = idecraft_vcs::nested_repositories(s.path(), 4, 50);
    assert_eq!(rel(all), ["lib/a", "lib/a/inner", "tools/deep/b/c"]);
    assert!(!cut);
    let (shallow, _) = idecraft_vcs::nested_repositories(s.path(), 3, 50);
    assert_eq!(rel(shallow), ["lib/a", "lib/a/inner"]);
    let (one, cut) = idecraft_vcs::nested_repositories(s.path(), 4, 1);
    assert_eq!((rel(one), cut), (vec!["lib/a".to_owned()], true));
}

#[test]
fn history_newest_first_and_unborn_head() {
    let Some(s) = Scratch::new("log") else { return };
    let repo = Repo::discover(s.path()).unwrap();
    assert!(repo.log(10).unwrap().is_empty(), "unborn HEAD");
    for (i, msg) in ["first", "second\n\nbody", "third"].iter().enumerate() {
        s.write("f.txt", format!("{i}\n").as_bytes());
        s.commit(msg);
    }
    let repo = Repo::discover(s.path()).unwrap();
    let log = repo.log(10).unwrap();
    let summaries: Vec<&str> = log.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["third", "second", "first"]);
    assert_eq!(log[0].author, "Test Author");
    assert_eq!(log[0].email, "test@example.invalid");
    assert_eq!(log[0].short_id.len(), 7);
    assert!(log[0].id.starts_with(&log[0].short_id));
    assert!(log[0].time > 1_600_000_000);
    assert_eq!(repo.log(2).unwrap().len(), 2);
}

/// Hand test 31 (D14): commits of other branches are in the full log, so they can be found and
/// cherry-picked; stashes are not (SPEC-0086 v2 behaviour 2).
#[test]
fn full_history_lists_other_branches_and_tags() {
    let Some(s) = Scratch::new("log-all") else {
        return;
    };
    let repo = Repo::discover(s.path()).unwrap();
    assert!(repo.log_all(10).unwrap().is_empty(), "unborn HEAD");
    s.write("f.txt", b"base\n");
    s.commit("base");
    s.git(&["checkout", "-q", "-b", "other"]).unwrap();
    s.write("o.txt", b"other\n");
    s.commit("on other");
    s.git(&["checkout", "-q", "--detach"]).unwrap();
    s.write("t.txt", b"tagged\n");
    s.commit("only tagged");
    s.git(&["tag", "lonely"]).unwrap();
    s.git(&["checkout", "-q", "main"]).unwrap();
    s.write("f.txt", b"main\n");
    s.commit("on main");
    s.write("f.txt", b"stashed\n");
    s.git(&["stash", "-q"]).unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    let head: Vec<String> = repo
        .log(10)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(head, ["on main", "base"]);
    let all: Vec<String> = repo
        .log_all(10)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    for want in ["on main", "on other", "only tagged", "base"] {
        assert!(all.contains(&want.to_owned()), "{want} missing in {all:?}");
    }
    assert_eq!(all.len(), 4, "each commit once, no stash commits: {all:?}");
    assert_eq!(repo.log_all(2).unwrap().len(), 2);
}

/// SPEC-0086 v5 AK-14: the commits that change a file or anything below a folder; a merge only
/// when the path differs from all its parents.
#[test]
fn commits_touching_files_and_folders() {
    let Some(s) = Scratch::new("touching") else {
        return;
    };
    s.write("src/a.rs", b"a\n");
    s.write("README", b"r\n");
    s.commit("base");
    s.git(&["checkout", "-q", "-b", "side"]).unwrap();
    s.write("src/deep/b.rs", b"b\n");
    s.commit("side adds b");
    s.git(&["checkout", "-q", "main"]).unwrap();
    s.write("README", b"r2\n");
    s.commit("main readme");
    s.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"])
        .unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    let log = repo.log_all(10).unwrap();
    let ids: Vec<String> = log.iter().map(|c| c.id.clone()).collect();
    let subjects = |paths: &[&str]| {
        let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
        let set = repo.commits_touching(&ids, &paths).unwrap();
        let mut out: Vec<&str> = log
            .iter()
            .filter(|c| set.contains(&c.id))
            .map(|c| c.summary.as_str())
            .collect();
        out.sort_unstable();
        out
    };
    assert_eq!(subjects(&["README"]), ["base", "main readme"]);
    assert_eq!(subjects(&["src"]), ["base", "side adds b"]);
    assert_eq!(subjects(&["src/deep/b.rs"]), ["side adds b"]);
    assert_eq!(
        subjects(&["src/", "README"]),
        ["base", "main readme", "side adds b"]
    );
    assert!(subjects(&["missing"]).is_empty());
    assert!(subjects(&[]).is_empty());
}

/// SPEC-0086 v3 AK-9: commits carry their parents (a merge two); `HEAD` is a label of its own only
/// while detached; the current branch is named.
#[test]
fn log_parents_and_head_label_only_when_detached() {
    let Some(s) = Scratch::new("log-graph") else {
        return;
    };
    s.write("f.txt", b"base\n");
    s.commit("base");
    s.git(&["checkout", "-q", "-b", "side"]).unwrap();
    s.write("s.txt", b"side\n");
    s.commit("on side");
    s.git(&["checkout", "-q", "main"]).unwrap();
    s.write("m.txt", b"main\n");
    s.commit("on main");
    s.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"])
        .unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    let log = repo.log_all(10).unwrap();
    let by = |subject: &str| log.iter().find(|c| c.summary == subject).unwrap();
    let merge = by("merge side");
    assert_eq!(
        merge.parents,
        [by("on main").id.clone(), by("on side").id.clone()]
    );
    assert_eq!(by("on side").parents, [by("base").id.clone()]);
    assert!(by("base").parents.is_empty());
    assert_eq!(repo.current_branch().unwrap().as_deref(), Some("main"));
    assert_eq!(repo.ref_labels().unwrap()[&merge.id], ["main"]);

    s.git(&["checkout", "-q", "--detach", "side"]).unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    assert_eq!(repo.current_branch().unwrap(), None);
    assert_eq!(
        repo.ref_labels().unwrap()[&by("on side").id],
        ["HEAD", "side"]
    );
}

#[test]
fn changes_of_files_against_head() {
    let Some(s) = Scratch::new("diff") else {
        return;
    };
    s.write("src/a.rs", b"fn a() {}\nfn b() {}\nfn c() {}\n");
    s.write("img.bin", b"\0\x01\x02");
    s.commit("base");
    s.write("src/a.rs", b"fn a() {}\nfn B() {}\nfn c() {}\nfn d() {}\n");
    s.write("new.txt", b"x\ny\n");
    s.write("img.bin", b"\0\x01\x03");
    let repo = Repo::discover(s.path()).unwrap();

    let a = repo.diff_file("src/a.rs").unwrap();
    let kinds: Vec<(LineChangeKind, usize, usize)> = a
        .changes
        .iter()
        .map(|c| (c.kind, c.new_start, c.new_end))
        .collect();
    assert_eq!(
        kinds,
        [
            (LineChangeKind::Modified, 2, 2),
            (LineChangeKind::Added, 4, 4)
        ]
    );
    assert!(a.hunks[0].contains("-fn b() {}\n+fn B() {}\n"));

    let new = repo.diff_file("new.txt").unwrap();
    assert_eq!(new.changes[0].kind, LineChangeKind::Added);
    assert_eq!((new.changes[0].new_start, new.changes[0].new_end), (1, 2));

    let bin = repo.diff_file("img.bin").unwrap();
    assert!(bin.binary && bin.changes.is_empty() && bin.hunks.is_empty());

    let outside = std::env::temp_dir().join(format!("idecraft-nogit-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    // The temp directory itself is normally not inside a repository.
    if Repo::discover(&outside).is_err() {
        assert!(
            Repo::discover(&outside)
                .err()
                .unwrap()
                .to_string()
                .contains("not inside a Git repository")
        );
    }
    let _ = std::fs::remove_dir_all(&outside);
}

// SPEC-0033 AK-2: blame per line, uncommitted lines, new file.
#[test]
fn blame_attributes_lines_and_marks_uncommitted() {
    let Some(s) = Scratch::new("blame") else {
        return;
    };
    s.write("a.txt", b"one\ntwo\nthree\n");
    s.commit("first");
    s.git(&["config", "user.name", "Second Author"]).unwrap();
    s.write("a.txt", b"one\nTWO\nthree\nfour\n");
    s.commit("second");
    let repo = Repo::discover(s.path()).unwrap();
    let ranges = repo.blame("a.txt").unwrap();
    let view: Vec<_> = ranges
        .iter()
        .map(|r| {
            (
                r.start_line,
                r.end_line,
                r.commit
                    .as_ref()
                    .map(|c| (c.summary.clone(), c.author.clone())),
            )
        })
        .collect();
    let first = Some(("first".to_owned(), "Test Author".to_owned()));
    let second = Some(("second".to_owned(), "Second Author".to_owned()));
    assert_eq!(
        view,
        [
            (1, 1, first.clone()),
            (2, 2, second.clone()),
            (3, 3, first.clone()),
            (4, 4, second.clone()),
        ]
    );

    s.write("a.txt", b"one\nTWO\nedited\nfour\n");
    let view: Vec<_> = repo
        .blame("a.txt")
        .unwrap()
        .iter()
        .map(|r| (r.start_line, r.end_line, r.commit.is_some()))
        .collect();
    assert_eq!(
        view,
        [(1, 1, true), (2, 2, true), (3, 3, false), (4, 4, true)]
    );

    s.write("new.txt", b"x\ny\n");
    let ranges = repo.blame("new.txt").unwrap();
    assert_eq!(ranges.len(), 1);
    assert_eq!((ranges[0].start_line, ranges[0].end_line), (1, 2));
    assert!(ranges[0].commit.is_none());
}

// SPEC-0277 AK-1: blame at HEAD with whole messages; the reasons it cannot annotate.
#[test]
fn blame_head_with_messages_and_reasons() {
    let Some(s) = Scratch::new("blame-head") else {
        return;
    };
    s.write("a.txt", b"one\ntwo\nthree\n");
    s.git(&["add", "-A"]).unwrap();
    s.git(&["commit", "-q", "-m", "first\n\nWhy it starts."])
        .unwrap();
    s.git(&["config", "user.name", "Second Author"]).unwrap();
    s.write("a.txt", b"one\nTWO\nthree\n");
    s.commit("second");
    let repo = Repo::discover(s.path()).unwrap();
    let blame = repo.blame_head("a.txt").unwrap();
    assert_eq!(blame.head_text, "one\nTWO\nthree\n");
    assert_eq!(blame.lines, [Some(0), Some(1), Some(0)]);
    assert_eq!(blame.commits.len(), 2);
    assert_eq!(blame.commits[0].info.author, "Test Author");
    assert_eq!(blame.commits[0].message, "first\n\nWhy it starts.");
    assert_eq!(blame.commits[1].info.author, "Second Author");
    assert_eq!(blame.commits[1].info.summary, "second");
    assert!(blame.commits[1].info.time >= blame.commits[0].info.time);
    // Mapped onto an edited text: the inserted line has no commit, the rest keep theirs.
    let mapped = blame.map("one\nnew\nTWO\nthree\n");
    assert_eq!(mapped, [Some(0), None, Some(1), Some(0)]);
    let ranges = blame.ranges(&mapped);
    assert_eq!(
        ranges
            .iter()
            .map(|r| (
                r.start_line,
                r.end_line,
                r.commit.as_ref().map(|c| &*c.summary)
            ))
            .collect::<Vec<_>>(),
        [
            (1, 1, Some("first")),
            (2, 2, None),
            (3, 3, Some("second")),
            (4, 4, Some("first"))
        ]
    );

    s.write("untracked.txt", b"x\n");
    let e = repo.blame_head("untracked.txt").unwrap_err();
    assert!(matches!(e, idecraft_vcs::GitError::Untracked(_)), "{e}");
    assert_eq!(e.to_string(), "untracked.txt is not tracked by Git");
    s.write("added.txt", b"y\n");
    s.git(&["add", "added.txt"]).unwrap();
    let e = repo.blame_head("added.txt").unwrap_err();
    assert_eq!(e.to_string(), "added.txt has no committed version yet");

    let outside = std::env::temp_dir().join(format!("idecraft-no-repo-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    if Repo::discover(&outside).is_err() {
        let e = Repo::discover(&outside).err().unwrap();
        assert!(e.to_string().contains("not inside a Git repository"), "{e}");
    }
    let _ = std::fs::remove_dir_all(&outside);
}

// SPEC-0277 AK-6: blame of a 20 000-line file with 50 commits and the remap after an edit stay
// within the budget of behaviour 11 (×10 in a debug build).
#[test]
fn blame_of_a_large_file_within_budget() {
    let Some(s) = Scratch::new("blame-large") else {
        return;
    };
    const LINES: usize = 20_000;
    const COMMITS: usize = 50;
    let mut lines: Vec<String> = (0..LINES).map(|i| format!("line {i} v0")).collect();
    for c in 0..COMMITS {
        // Each commit rewrites every 50th line from its own offset.
        for i in (c % 50..LINES).step_by(50) {
            lines[i] = format!("line {i} v{}", c + 1);
        }
        s.write("big.txt", (lines.join("\n") + "\n").as_bytes());
        s.commit(&format!("change {c}"));
    }
    let factor = if cfg!(debug_assertions) { 10.0 } else { 1.0 };
    let repo = Repo::discover(s.path()).unwrap();
    // The budget is for the work, measured as this thread's CPU time: on a loaded machine the
    // wall clock also counts the time the thread waits to be scheduled, which is not blame's.
    let started = std::time::Instant::now();
    let cpu = thread_cpu();
    let blame = repo.blame_head("big.txt").unwrap();
    let blamed = thread_cpu() - cpu;
    let wall = started.elapsed().as_secs_f64();
    assert_eq!(blame.lines.len(), LINES);
    assert!(blame.lines.iter().all(Option::is_some));
    assert_eq!(blame.commits.len(), COMMITS);
    let mut edited = lines.clone();
    edited.insert(10_000, "typed".into());
    edited[15_000] = "changed".into();
    let text = edited.join("\n") + "\n";
    let cpu = thread_cpu();
    let mapped = blame.map(&text);
    let remapped = thread_cpu() - cpu;
    assert_eq!(mapped.len(), LINES + 1);
    assert_eq!(mapped[10_000], None);
    assert_eq!(mapped[15_000], None);
    assert_eq!(mapped[10_001], blame.lines[10_000]);
    eprintln!("blame {blamed:.3} s CPU ({wall:.3} s wall), remap {remapped:.4} s CPU");
    assert!(blamed <= 2.0 * factor, "blame took {blamed:.3} s CPU");
    assert!(remapped <= 0.05 * factor, "remap took {remapped:.4} s CPU");
}

/// The CPU time this thread has used, in seconds (the wall clock where that is not available).
fn thread_cpu() -> f64 {
    #[cfg(unix)]
    {
        use nix::time::{ClockId, clock_gettime};
        if let Ok(t) = clock_gettime(ClockId::CLOCK_THREAD_CPUTIME_ID) {
            return t.tv_sec() as f64 + t.tv_nsec() as f64 / 1e9;
        }
    }
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
}

// SPEC-0033 AK-3: current, local, remote-tracking and detached HEAD.
#[test]
fn branches_local_remote_and_detached() {
    let Some(s) = Scratch::new("branches") else {
        return;
    };
    s.write("a.txt", b"a\n");
    s.commit("base");
    s.git(&["branch", "feature"]).unwrap();
    s.git(&["update-ref", "refs/remotes/origin/main", "HEAD"])
        .unwrap();
    s.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ])
    .unwrap();
    s.git(&["switch", "-q", "feature"]).unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    let b = repo.branches().unwrap();
    assert!(!b.detached);
    let names: Vec<_> = b
        .branches
        .iter()
        .map(|x| (x.name.as_str(), x.remote, x.current))
        .collect();
    assert_eq!(
        names,
        [
            ("feature", false, true),
            ("main", false, false),
            ("origin/main", true, false),
        ]
    );
    assert_eq!(b.branches[0].target.len(), 7);

    s.git(&["switch", "-q", "--detach", "main"]).unwrap();
    let b = Repo::discover(s.path()).unwrap().branches().unwrap();
    assert!(b.detached);
    assert!(b.branches.iter().all(|x| !x.current));
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| p.to_string()).collect()
}

// SPEC-0034 AK-1, AK-4: commit selected files, others keep their state.
#[test]
fn commit_selected_files() {
    let Some(s) = Scratch::new("commit") else {
        return;
    };
    s.write("mod.txt", b"old\n");
    s.write("del.txt", b"bye\n");
    s.write("other.txt", b"other\n");
    s.write("staged.txt", b"s1\n");
    s.commit("base");
    let base = s.out(&["rev-parse", "HEAD"]);
    s.write("mod.txt", b"new\n");
    std::fs::remove_file(s.path().join("del.txt")).unwrap();
    s.write("dir/add.txt", b"added\n");
    s.write("other.txt", b"other changed\n");
    s.write("staged.txt", b"s2\n");
    s.git(&["add", "staged.txt"]).unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    let selected = paths(&["mod.txt", "del.txt", "dir/add.txt"]);

    let preview = repo.commit("Change things", &selected, true).unwrap();
    assert!(preview.commit.is_none());
    let kinds: Vec<_> = preview
        .changes
        .iter()
        .map(|c| (c.path.as_str(), c.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("mod.txt", ChangeKind::Modified),
            ("del.txt", ChangeKind::Deleted),
            ("dir/add.txt", ChangeKind::Added),
        ]
    );
    assert_eq!(s.out(&["rev-parse", "HEAD"]), base);

    let done = repo.commit("Change things", &selected, false).unwrap();
    let commit = done.commit.unwrap();
    assert_eq!(commit.summary, "Change things");
    assert_eq!(commit.author, "Test Author");
    assert_eq!(s.out(&["rev-parse", "HEAD"]).trim(), commit.id);
    assert_eq!(s.out(&["rev-parse", "HEAD^"]), base);
    assert_eq!(s.out(&["show", "HEAD:mod.txt"]), "new\n");
    assert_eq!(s.out(&["show", "HEAD:dir/add.txt"]), "added\n");
    assert_eq!(s.out(&["show", "HEAD:other.txt"]), "other\n");
    assert_eq!(s.out(&["show", "HEAD:staged.txt"]), "s1\n");
    assert_eq!(
        s.out(&["ls-tree", "-r", "--name-only", "HEAD"]),
        "dir/add.txt\nmod.txt\nother.txt\nstaged.txt\n"
    );
    assert_eq!(
        s.out(&["status", "--porcelain"]),
        " M other.txt\nM  staged.txt\n"
    );
    s.git(&["fsck", "--no-progress"]).unwrap();
    assert_eq!(
        s.out(&["log", "-g", "-1", "--format=%gs"]),
        "commit: Change things\n"
    );
    // The index stays consistent for the `git` program (no stale tree cache).
    s.git(&["commit", "-q", "-m", "next"]).unwrap();
    assert_eq!(
        s.out(&["ls-tree", "-r", "--name-only", "HEAD"]),
        "dir/add.txt\nmod.txt\nother.txt\nstaged.txt\n"
    );
    assert_eq!(s.out(&["show", "HEAD:staged.txt"]), "s2\n");
}

// SPEC-0034 AK-2: first commit on an unborn HEAD.
#[test]
fn first_commit_creates_the_branch() {
    let Some(s) = Scratch::new("unborn") else {
        return;
    };
    s.write("a.txt", b"a\n");
    let repo = Repo::discover(s.path()).unwrap();
    let done = repo.commit("Initial", &paths(&["a.txt"]), false).unwrap();
    assert_eq!(
        s.out(&["rev-parse", "main"]).trim(),
        done.commit.unwrap().id
    );
    assert_eq!(s.out(&["status", "--porcelain"]), "");
}

// SPEC-0034 AK-3: rejections leave HEAD unchanged.
#[test]
fn commit_rejections() {
    let Some(s) = Scratch::new("reject") else {
        return;
    };
    s.write("a.txt", b"a\n");
    s.commit("base");
    let head = s.out(&["rev-parse", "HEAD"]);
    let repo = Repo::discover(s.path()).unwrap();
    s.write("a.txt", b"changed\n");
    for (message, list, expected) in [
        ("  ", &["a.txt"][..], "message is empty"),
        ("m", &[][..], "no files selected"),
        ("m", &["../x"][..], "repository-relative"),
        ("m", &["missing.txt"][..], "neither exists"),
    ] {
        let err = repo.commit(message, &paths(list), false).unwrap_err();
        assert!(err.to_string().contains(expected), "{err}");
    }
    s.write("a.txt", b"a\n");
    let err = repo.commit("m", &paths(&["a.txt"]), false).unwrap_err();
    assert!(err.to_string().contains("nothing to commit"), "{err}");
    assert_eq!(s.out(&["rev-parse", "HEAD"]), head);
}

// SPEC-0034 AK-5: create branches.
#[test]
fn create_branches() {
    let Some(s) = Scratch::new("mkbranch") else {
        return;
    };
    s.write("a.txt", b"a\n");
    s.commit("one");
    let first = s.out(&["rev-parse", "HEAD"]);
    s.write("a.txt", b"b\n");
    s.commit("two");
    let repo = Repo::discover(s.path()).unwrap();
    let b = repo.create_branch("feature/x", None).unwrap();
    assert!(!b.current);
    assert_eq!(
        s.out(&["rev-parse", "feature/x"]),
        s.out(&["rev-parse", "HEAD"])
    );
    repo.create_branch("old", Some("HEAD~1")).unwrap();
    assert_eq!(s.out(&["rev-parse", "old"]), first);
    assert_eq!(s.out(&["branch", "--show-current"]), "main\n");
    assert!(
        repo.create_branch("old", None)
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    assert!(repo.create_branch("bad..name", None).is_err());
    assert!(repo.create_branch("x", Some("nope")).is_err());
}

fn two_branches(tag: &str) -> Option<Scratch> {
    let s = Scratch::new(tag)?;
    s.write("same.txt", b"same\n");
    s.write("change.txt", b"main\n");
    s.write("gone/old.txt", b"old\n");
    s.write("run.sh", b"echo hi\n");
    s.write("local.txt", b"local\n");
    s.commit("main");
    s.git(&["switch", "-q", "-c", "topic"]).unwrap();
    s.write("change.txt", b"topic\n");
    std::fs::remove_file(s.path().join("gone/old.txt")).unwrap();
    s.write("new/added.txt", b"added\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = s.path().join("run.sh");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    s.commit("topic");
    s.git(&["switch", "-q", "main"]).unwrap();
    Some(s)
}

// SPEC-0035 AK-1, AK-2 (carry-over): switch updates tree, index and HEAD.
#[test]
fn switch_branch_updates_working_tree_index_and_head() {
    let Some(s) = two_branches("switch") else {
        return;
    };
    s.write("local.txt", b"edited locally\n");
    let repo = Repo::discover(s.path()).unwrap();
    let out = repo.switch_branch("topic", false).unwrap();
    assert!(out.switched);
    let kinds: Vec<_> = out
        .changes
        .iter()
        .map(|c| (c.path.as_str(), c.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("change.txt", ChangeKind::Modified),
            ("gone/old.txt", ChangeKind::Deleted),
            ("new/added.txt", ChangeKind::Added),
            ("run.sh", ChangeKind::Modified),
        ]
    );
    assert_eq!(s.out(&["branch", "--show-current"]), "topic\n");
    assert_eq!(
        std::fs::read(s.path().join("change.txt")).unwrap(),
        b"topic\n"
    );
    assert!(!s.path().join("gone").exists());
    assert_eq!(
        std::fs::read(s.path().join("new/added.txt")).unwrap(),
        b"added\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(s.path().join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0);
    }
    assert_eq!(s.out(&["status", "--porcelain"]), " M local.txt\n");
    s.git(&["fsck", "--no-progress"]).unwrap();
    assert_eq!(
        s.out(&["log", "-g", "-1", "--format=%gs", "HEAD"]),
        "checkout: moving from main to topic\n"
    );
    // And back again.
    repo.switch_branch("main", false).unwrap();
    assert_eq!(s.out(&["status", "--porcelain"]), " M local.txt\n");
    assert_eq!(
        std::fs::read(s.path().join("gone/old.txt")).unwrap(),
        b"old\n"
    );
    assert!(!s.path().join("new").exists());
}

// SPEC-0068 AK-1: checkout of local, remote-tracking and new branches.
#[test]
fn checkout_local_remote_and_new() {
    let Some(s) = two_branches("checkout") else {
        return;
    };
    // A remote-tracking branch without a remote server: a ref under refs/remotes.
    let topic = s.out(&["rev-parse", "topic"]);
    s.git(&["update-ref", "refs/remotes/origin/feature", topic.trim()])
        .unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    assert!(repo.checkout("topic", false).unwrap().switched);
    assert_eq!(s.out(&["branch", "--show-current"]), "topic\n");
    let out = repo.checkout("origin/feature", false).unwrap();
    assert_eq!(out.branch, "feature");
    assert_eq!(s.out(&["branch", "--show-current"]), "feature\n");
    assert_eq!(s.out(&["rev-parse", "feature"]), topic);
    repo.checkout("main", false).unwrap();
    // A second checkout of the remote branch reuses the local one.
    assert_eq!(
        repo.checkout("origin/feature", false).unwrap().branch,
        "feature"
    );
    repo.checkout("brand-new", true).unwrap();
    assert_eq!(s.out(&["branch", "--show-current"]), "brand-new\n");
    assert!(
        repo.checkout("brand-new", true).is_err(),
        "an existing name is refused"
    );
}

// SPEC-0035 AK-2, AK-3: conflicts refuse, dry run, no-op, unknown branch.
#[test]
fn switch_branch_refusals_and_preview() {
    let Some(s) = two_branches("switchno") else {
        return;
    };
    s.write("change.txt", b"mine\n");
    s.write("new/added.txt", b"untracked in the way\n");
    let repo = Repo::discover(s.path()).unwrap();
    let preview = repo.switch_branch("topic", true).unwrap();
    assert!(!preview.switched);
    assert_eq!(preview.conflicts, ["change.txt", "new/added.txt"]);
    let err = repo.switch_branch("topic", false).unwrap_err().to_string();
    assert!(
        err.contains("change.txt") && err.contains("new/added.txt"),
        "{err}"
    );
    assert_eq!(s.out(&["branch", "--show-current"]), "main\n");
    assert_eq!(
        std::fs::read(s.path().join("change.txt")).unwrap(),
        b"mine\n"
    );
    assert!(s.path().join("gone/old.txt").exists());

    let same = repo.switch_branch("main", false).unwrap();
    assert!(!same.switched && same.changes.is_empty());
    assert!(repo.switch_branch("nope", false).is_err());
}

fn diverged(tag: &str) -> Option<Scratch> {
    let s = Scratch::new(tag)?;
    s.write("shared.txt", b"1\n2\n3\n4\n5\n6\n7\n8\n");
    s.write("a.txt", b"a\n");
    s.commit("base");
    s.git(&["switch", "-q", "-c", "topic"]).unwrap();
    s.write("shared.txt", b"1\n2\n3\n4\n5\n6\n7\nEIGHT\n");
    s.write("topic.txt", b"topic\n");
    s.commit("topic work");
    s.git(&["switch", "-q", "main"]).unwrap();
    s.write("shared.txt", b"ONE\n2\n3\n4\n5\n6\n7\n8\n");
    s.write("main.txt", b"main\n");
    s.commit("main work");
    Some(s)
}

// SPEC-0036 AK-1, AK-2: up to date and fast-forward.
#[test]
fn merge_up_to_date_and_fast_forward() {
    let Some(s) = Scratch::new("ff") else {
        return;
    };
    s.write("a.txt", b"a\n");
    s.commit("base");
    s.git(&["branch", "old"]).unwrap();
    s.git(&["switch", "-q", "-c", "ahead"]).unwrap();
    s.write("b/new.txt", b"new\n");
    s.commit("ahead");
    s.git(&["switch", "-q", "main"]).unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    let up = repo.merge("old", false).unwrap();
    assert_eq!((up.kind, up.merged), (MergeKind::UpToDate, false));

    let preview = repo.merge("ahead", true).unwrap();
    assert_eq!(
        (preview.kind, preview.merged),
        (MergeKind::FastForward, false)
    );
    assert!(!s.path().join("b/new.txt").exists());
    let ff = repo.merge("ahead", false).unwrap();
    assert_eq!((ff.kind, ff.merged), (MergeKind::FastForward, true));
    assert_eq!(
        s.out(&["rev-parse", "main"]),
        s.out(&["rev-parse", "ahead"])
    );
    assert_eq!(std::fs::read(s.path().join("b/new.txt")).unwrap(), b"new\n");
    assert_eq!(s.out(&["status", "--porcelain"]), "");
    assert_eq!(
        s.out(&["log", "-g", "-1", "--format=%gs", "main"]),
        "merge ahead: Fast-forward\n"
    );
}

// SPEC-0036 AK-3: clean three-way merge equals `git merge`.
#[test]
fn merge_three_way_like_git() {
    let Some(s) = diverged("3way") else {
        return;
    };
    let repo = Repo::discover(s.path()).unwrap();
    let objects = s.out(&["count-objects"]);
    let preview = repo.merge("topic", true).unwrap();
    assert_eq!(preview.kind, MergeKind::Merge);
    // The preview writes no objects.
    assert_eq!(s.out(&["count-objects"]), objects);
    let done = repo.merge("topic", false).unwrap();
    assert!(done.merged);
    assert_eq!(done.commit.unwrap().summary, "Merge branch 'topic'");
    let ours_tree = s.out(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        s.out(&["rev-parse", "HEAD^2"]),
        s.out(&["rev-parse", "topic"])
    );
    assert_eq!(
        std::fs::read(s.path().join("shared.txt")).unwrap(),
        b"ONE\n2\n3\n4\n5\n6\n7\nEIGHT\n"
    );
    assert_eq!(s.out(&["status", "--porcelain"]), "");
    s.git(&["fsck", "--no-progress"]).unwrap();

    // The reference result from the git program on a copy of the history.
    s.git(&["switch", "-q", "--detach", "HEAD^1"]).unwrap();
    s.git(&["merge", "-q", "--no-edit", "topic"]).unwrap();
    assert_eq!(s.out(&["rev-parse", "HEAD^{tree}"]), ours_tree);
}

// SPEC-0036 AK-4, AK-5: conflicts and local changes refuse the merge.
#[test]
fn merge_conflicts_and_local_changes_refuse() {
    let Some(s) = diverged("conflict") else {
        return;
    };
    s.git(&["switch", "-q", "topic"]).unwrap();
    s.write("shared.txt", b"uno\n2\n3\n4\n5\n6\n7\nEIGHT\n");
    s.commit("conflicting");
    s.git(&["switch", "-q", "main"]).unwrap();
    let head = s.out(&["rev-parse", "HEAD"]);
    let repo = Repo::discover(s.path()).unwrap();
    let preview = repo.merge("topic", true).unwrap();
    assert_eq!(preview.kind, MergeKind::Conflict);
    assert_eq!(preview.merge_conflicts, ["shared.txt"]);
    let err = repo.merge("topic", false).unwrap_err().to_string();
    assert!(err.contains("shared.txt"), "{err}");
    assert_eq!(s.out(&["rev-parse", "HEAD"]), head);
    assert_eq!(s.out(&["status", "--porcelain"]), "");
    assert!(!s.path().join(".git/MERGE_HEAD").exists());

    // A clean merge blocked by a local change on a path it touches.
    s.git(&["reset", "-q", "--hard", "HEAD"]).unwrap();
    s.git(&["branch", "-f", "clean", "topic~1"]).unwrap();
    s.write("topic.txt", b"untracked in the way\n");
    let err = repo.merge("clean", false).unwrap_err().to_string();
    assert!(err.contains("topic.txt"), "{err}");
    assert_eq!(s.out(&["rev-parse", "HEAD"]), head);
}

/// SPEC-0086 AK-2: details, labels and the diff inside a commit.
#[test]
fn commit_details_labels_and_diff_in_commit() {
    let Some(s) = Scratch::new("show") else {
        return;
    };
    s.write("a.txt", b"one\ntwo\n");
    s.write("b.txt", b"bee\n");
    s.commit("first\n\nbody of the first");
    s.git(&["tag", "v1"]).unwrap();
    s.write("a.txt", b"one\nTWO\nthree\n");
    std::fs::remove_file(s.path().join("b.txt")).unwrap();
    s.write("c.txt", b"sea\n");
    s.commit("second");
    s.git(&["branch", "feature"]).unwrap();
    let repo = Repo::discover(s.path()).unwrap();

    let second = repo.commit_details("HEAD").unwrap();
    assert_eq!(second.info.summary, "second");
    assert_eq!(second.parents.len(), 1);
    let changes: Vec<(&str, ChangeKind)> = second
        .changes
        .iter()
        .map(|c| (c.path.as_str(), c.kind))
        .collect();
    assert_eq!(
        changes,
        [
            ("a.txt", ChangeKind::Modified),
            ("b.txt", ChangeKind::Deleted),
            ("c.txt", ChangeKind::Added)
        ]
    );
    let first = repo.commit_details(&second.parents[0][..7]).unwrap();
    assert!(first.message.contains("body of the first"), "{first:?}");
    assert!(first.parents.is_empty());
    assert_eq!(first.changes.len(), 2);
    assert_eq!(repo.commit_details("v1").unwrap().info.id, first.info.id);
    assert!(repo.commit_details("nope").is_err());

    let labels = repo.ref_labels().unwrap();
    assert_eq!(labels[&second.info.id], ["feature", "main"]);
    assert_eq!(labels[&first.info.id], ["v1"]);

    let rows = repo.side_by_side_in_commit("HEAD", "a.txt").unwrap();
    let left: Vec<Option<&str>> = rows
        .iter()
        .map(|r| r.left.as_ref().map(|l| l.1.as_str()))
        .collect();
    let right: Vec<Option<&str>> = rows
        .iter()
        .map(|r| r.right.as_ref().map(|l| l.1.as_str()))
        .collect();
    assert!(
        left.contains(&Some("two")) && right.contains(&Some("TWO")),
        "{rows:?}"
    );
    assert!(right.contains(&Some("three")));
    let added = repo
        .side_by_side_in_commit(&first.info.id, "b.txt")
        .unwrap();
    assert!(added.iter().all(|r| r.left.is_none()), "{added:?}");
}

/// SPEC-0101 AK-1: the three sides of a conflicted file are read from the index; resolving writes the
/// file and clears the conflict.
#[test]
fn conflicts_and_resolving_them() {
    let Some(s) = Scratch::new("resolve") else {
        return;
    };
    s.write("a.txt", b"one\ntwo\nthree\n");
    s.write("same.txt", b"x\n");
    s.commit("base");
    s.git(&["checkout", "-q", "-b", "topic"]).unwrap();
    s.write("a.txt", b"one\nTWO (topic)\nthree\n");
    s.commit("topic");
    s.git(&["checkout", "-q", "main"]).unwrap();
    s.write("a.txt", b"one\ntwo (main)\nthree\n");
    s.commit("main");
    // The merge stops with a conflict.
    let merged = Command::new("git")
        .args(["merge", "-q", "topic"])
        .current_dir(s.path())
        .output()
        .unwrap();
    assert!(!merged.status.success());
    let repo = Repo::discover(s.path()).unwrap();
    let conflicts = repo.conflicts().unwrap();
    assert_eq!(conflicts.len(), 1);
    let c = &conflicts[0];
    assert_eq!(c.path, "a.txt");
    assert_eq!(c.base.as_deref(), Some("one\ntwo\nthree\n"));
    assert_eq!(c.ours.as_deref(), Some("one\ntwo (main)\nthree\n"));
    assert_eq!(c.theirs.as_deref(), Some("one\nTWO (topic)\nthree\n"));
    assert!(
        repo.resolve_conflict("same.txt", "y\n").is_err(),
        "no conflict there"
    );
    repo.resolve_conflict("a.txt", "one\nboth\nthree\n")
        .unwrap();
    assert!(repo.conflicts().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(s.path().join("a.txt")).unwrap(),
        "one\nboth\nthree\n"
    );
    let _ = std::fs::remove_dir_all(s.path());
}

// SPEC-0064 v3 AK-6: one change of a file is committed, the other stays in the working file.
#[test]
fn commit_one_change_keeps_the_others() {
    let Some(s) = Scratch::new("one-change") else {
        return;
    };
    s.write("A.txt", b"a\nb\nc\nd\ne\n");
    s.commit("first");
    s.write("A.txt", b"a\nB\nc\nd\nE\n");
    let repo = Repo::discover(s.path()).unwrap();
    let head = repo.head_text("A.txt").unwrap().unwrap();
    let text = std::fs::read_to_string(s.path().join("A.txt")).unwrap();
    let changes = idecraft_vcs::line_changes(&head, &text);
    assert_eq!(changes.len(), 2);
    let content = idecraft_vcs::apply_change(&head, &text, &changes[0]);
    let options = idecraft_vcs::CommitOptions::default();
    assert!(
        repo.commit_change("", "A.txt", &content, false, &options)
            .is_err(),
        "empty message"
    );
    let done = repo
        .commit_change("Only b", "A.txt", &content, false, &options)
        .unwrap();
    assert!(done.commit.is_some());
    assert_eq!(s.out(&["show", "HEAD:A.txt"]), "a\nB\nc\nd\ne\n");
    assert_eq!(
        std::fs::read_to_string(s.path().join("A.txt")).unwrap(),
        "a\nB\nc\nd\nE\n"
    );
    // The other change is still a change of the working file, nothing is staged.
    let repo = Repo::discover(s.path()).unwrap();
    let left = idecraft_vcs::line_changes(&repo.head_text("A.txt").unwrap().unwrap(), &text);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].new_start, 5);
    assert_eq!(s.out(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(s.out(&["diff", "--name-only"]), "A.txt\n");
    // A staged change of the file refuses.
    s.git(&["add", "A.txt"]).unwrap();
    let repo = Repo::discover(s.path()).unwrap();
    let err = repo
        .commit_change("x", "A.txt", "a\n", false, &options)
        .unwrap_err();
    assert!(err.to_string().contains("staged"), "{err}");
    // A file only in the index refuses too.
    s.write("N.txt", b"n\n");
    s.git(&["add", "N.txt"]).unwrap();
    assert!(
        repo.commit_change("x", "N.txt", "n\n", false, &options)
            .is_err()
    );
}

// SPEC-0084 v2 behaviour 11: a file's text on another branch.
#[test]
fn text_of_a_file_at_a_branch() {
    let Some(s) = Scratch::new("text-at") else {
        return;
    };
    s.write("A.txt", b"one\n");
    s.commit("first");
    s.git(&["branch", "other"]).unwrap();
    s.write("A.txt", b"two\n");
    s.commit("second");
    let repo = Repo::discover(s.path()).unwrap();
    assert_eq!(
        repo.text_at("other", "A.txt").unwrap().as_deref(),
        Some("one\n")
    );
    assert_eq!(
        repo.text_at("main", "A.txt").unwrap().as_deref(),
        Some("two\n")
    );
    assert_eq!(repo.text_at("other", "missing.txt").unwrap(), None);
    assert!(repo.text_at("no-such-branch", "A.txt").is_err());
}
