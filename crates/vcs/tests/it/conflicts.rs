//! SPEC-0101 AK-10 (v6): real conflicts, stopped by a merge, a rebase and a cherry-pick, from the
//! repositories `tests/fixtures/conflict-repos.sh` builds. What IDECraft lists is compared with the
//! index, what it writes with the `git` program's own view of the repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use idecraft_vcs::{Operation, Outcome, Repo, Resolution, Side, merge_model, merge_outcome};

const OPERATIONS: [&str; 3] = ["merge", "rebase", "cherry-pick"];

/// The fixture repositories of `cases` in a fresh folder; `None` without `bash` or `git`.
struct Fixtures(PathBuf);

impl Fixtures {
    fn new(tag: &str, cases: &[&str]) -> Option<Self> {
        let dir =
            std::env::temp_dir().join(format!("idecraft-conflicts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conflict-repos.sh");
        let out = Command::new("bash")
            .arg(script)
            .arg(&dir)
            .args(cases)
            .output()
            .ok()?;
        if !out.status.success() && Command::new("git").arg("--version").output().is_err() {
            return None;
        }
        assert!(
            out.status.success(),
            "conflict-repos.sh: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(Self(dir))
    }

    fn path(&self, case: &str, operation: &str) -> PathBuf {
        self.0.join(format!("{case}-{operation}"))
    }

    fn repo(&self, case: &str, operation: &str) -> Repo {
        Repo::discover(&self.path(case, operation)).unwrap()
    }
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Output of `git` in `dir`, `None` when it fails.
fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    out.status.success().then_some(out.stdout)
}

fn status(dir: &Path) -> String {
    String::from_utf8(git(dir, &["status", "--porcelain"]).unwrap()).unwrap()
}

fn unmerged(dir: &Path) -> String {
    String::from_utf8(git(dir, &["ls-files", "-u"]).unwrap()).unwrap()
}

#[test]
fn sides_equal_the_index_stages() {
    let cases = [
        "same-lines",
        "adjacent",
        "delete-modify",
        "modify-delete",
        "add-add",
        "rename-edit",
        "whitespace",
        "crlf",
        "binary",
        "latin1",
        "three-in-one",
        "two-files",
    ];
    let Some(f) = Fixtures::new("sides", &cases) else {
        return;
    };
    for case in cases {
        for operation in OPERATIONS {
            let dir = f.path(case, operation);
            let conflicts = f.repo(case, operation).conflicts().unwrap();
            let listed: Vec<&str> = conflicts.iter().map(|c| c.path.as_str()).collect();
            let by_git =
                String::from_utf8(git(&dir, &["diff", "--name-only", "--diff-filter=U"]).unwrap())
                    .unwrap();
            assert_eq!(
                listed,
                by_git.lines().collect::<Vec<_>>(),
                "{case}-{operation}"
            );
            for c in &conflicts {
                let mut not_text = false;
                for (stage, side) in [(1, Side::Base), (2, Side::Ours), (3, Side::Theirs)] {
                    let blob = git(&dir, &["show", &format!(":{stage}:{}", c.path)]);
                    assert_eq!(
                        c.has(side),
                        blob.is_some(),
                        "{case}-{operation} {} {side}",
                        c.path
                    );
                    match (c.side_text(side), &blob) {
                        // A side shown as text is the index's bytes, nothing replaced.
                        (Some(text), Some(blob)) => assert_eq!(text.as_bytes(), blob.as_slice()),
                        (None, Some(_)) => not_text = true,
                        (None, None) => {}
                        (Some(_), None) => panic!("{case}-{operation}: text without a file"),
                    }
                }
                assert_eq!(c.binary, not_text, "{case}-{operation} {}", c.path);
                assert_eq!(
                    c.whole_side(),
                    matches!(
                        case,
                        "binary" | "latin1" | "delete-modify" | "modify-delete"
                    ),
                    "{case}-{operation}"
                );
                // No conflicted file ever counts as resolved before something was chosen.
                let rebasing = operation == "rebase";
                let merge = merge_model(c, rebasing);
                assert!(idecraft_vcs::merge_open(c, rebasing, &merge) > 0);
                assert_eq!(merge_outcome(c, rebasing, &merge), None);
            }
        }
    }
}

#[test]
fn a_binary_file_takes_a_whole_side() {
    let Some(f) = Fixtures::new("binary", &["binary"]) else {
        return;
    };
    let wanted = [
        ("merge", Side::Theirs, &b"BIN\0\x01\x02feature\n"[..]),
        // In a rebase HEAD is the target: stage 2 is main's file.
        ("rebase", Side::Ours, b"BIN\0\x01\x02main\n"),
        ("cherry-pick", Side::Base, b"BIN\0\x01\x02base\n"),
    ];
    for (operation, side, bytes) in wanted {
        let dir = f.path("binary", operation);
        let repo = f.repo("binary", operation);
        let c = repo.conflicts().unwrap().remove(0);
        assert!(c.binary && c.ours.is_none() && c.theirs.is_none());
        assert!(repo.resolve_conflict_with_side("blob.bin", side).unwrap());
        assert_eq!(std::fs::read(dir.join("blob.bin")).unwrap(), bytes);
        assert_eq!(unmerged(&dir), "", "{operation}");
        assert_eq!(
            git(&dir, &["show", ":0:blob.bin"]).unwrap(),
            bytes,
            "{operation}: staged as written"
        );
    }
}

#[test]
fn a_file_that_is_not_utf8_keeps_its_bytes() {
    let Some(f) = Fixtures::new("latin1", &["latin1"]) else {
        return;
    };
    for operation in OPERATIONS {
        let dir = f.path("latin1", operation);
        let repo = f.repo("latin1", operation);
        let c = repo.conflicts().unwrap().remove(0);
        assert!(
            c.binary && c.whole_side(),
            "not merged as text: writing it back would replace the byte 0xFC"
        );
        // The view's left side: the user's own change, also in a rebase.
        let rebasing = repo.operation_in_progress() == Some(Operation::Rebase);
        let mut merge = merge_model(&c, rebasing);
        merge.resolve(0, Some(Resolution::Ours));
        let outcome = merge_outcome(&c, rebasing, &merge).unwrap();
        repo.apply_resolution("umlaut.txt", &outcome).unwrap();
        let own = if rebasing { "feature" } else { "main" };
        assert_eq!(
            std::fs::read(dir.join("umlaut.txt")).unwrap(),
            [b"gr\xfcn\nzwei (", own.as_bytes(), b")\ndrei\n"].concat(),
            "{operation}"
        );
        assert_eq!(unmerged(&dir), "");
    }
}

#[test]
fn the_deleting_side_removes_the_file() {
    let Some(f) = Fixtures::new("deleted", &["delete-modify", "modify-delete", "add-add"]) else {
        return;
    };
    // Ours deleted the file, theirs changed it.
    let dir = f.path("delete-modify", "merge");
    let repo = f.repo("delete-modify", "merge");
    let c = repo.conflicts().unwrap().remove(0);
    assert!(!c.binary && !c.has_ours && c.has_theirs && c.has_base);
    assert!(
        !repo
            .resolve_conflict_with_side("notes.txt", Side::Ours)
            .unwrap()
    );
    assert!(!dir.join("notes.txt").exists(), "no empty file is left");
    assert_eq!(status(&dir), "", "the removal is what HEAD already has");
    assert_eq!(repo.operation_in_progress(), Some(Operation::Merge));

    // The same conflict, keeping the changed file.
    let dir = f.path("delete-modify", "cherry-pick");
    let repo = f.repo("delete-modify", "cherry-pick");
    assert!(
        repo.resolve_conflict_with_side("notes.txt", Side::Theirs)
            .unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "keep\nme, changed on feature\nor not\n"
    );
    assert_eq!(status(&dir), "A  notes.txt\n");

    // Ours changed the file, theirs deleted it; in a rebase stage 3 is the replayed deletion.
    for operation in OPERATIONS {
        let dir = f.path("modify-delete", operation);
        let repo = f.repo("modify-delete", operation);
        let c = repo.conflicts().unwrap().remove(0);
        assert!(c.has_ours && !c.has_theirs, "{operation}");
        match operation {
            "merge" => {
                assert!(
                    !repo
                        .resolve_conflict_with_side("notes.txt", Side::Theirs)
                        .unwrap()
                );
                assert!(!dir.join("notes.txt").exists());
                assert_eq!(status(&dir), "D  notes.txt\n");
            }
            "rebase" => {
                assert!(
                    repo.resolve_conflict_with_side("notes.txt", Side::Ours)
                        .unwrap()
                );
                assert_eq!(
                    std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
                    "keep\nme, changed on main\nor not\n"
                );
                assert_eq!(status(&dir), "");
            }
            _ => {
                assert!(
                    repo.resolve_conflict_with_side("notes.txt", Side::Base)
                        .unwrap()
                );
                assert_eq!(
                    std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
                    "keep\nme\nor not\n"
                );
            }
        }
        assert_eq!(unmerged(&dir), "", "{operation}");
    }

    // Both sides added the file: there is no base to take, and nothing is touched.
    let dir = f.path("add-add", "merge");
    let repo = f.repo("add-add", "merge");
    let before = std::fs::read(dir.join("new.txt")).unwrap();
    let refused = repo.resolve_conflict_with_side("new.txt", Side::Base);
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("no base version")),
        "{refused:?}"
    );
    assert_eq!(std::fs::read(dir.join("new.txt")).unwrap(), before);
    assert_eq!(status(&dir), "AA new.txt\n");
    assert!(
        repo.resolve_conflict_with_side("README.txt", Side::Ours)
            .is_err(),
        "no conflict there"
    );
}

/// Resolves `path` in the merge view's model by taking the right column for every conflict.
fn take_right(repo: &Repo, path: &str) {
    let rebasing = repo.operation_in_progress() == Some(Operation::Rebase);
    let c = repo
        .conflicts()
        .unwrap()
        .into_iter()
        .find(|c| c.path == path)
        .unwrap();
    let mut merge = merge_model(&c, rebasing);
    for chunk in merge.conflicts() {
        merge.resolve(chunk, Some(Resolution::Theirs));
    }
    let outcome = merge_outcome(&c, rebasing, &merge).unwrap();
    assert!(matches!(outcome, Outcome::Text(_)));
    repo.apply_resolution(path, &outcome).unwrap();
}

#[test]
fn an_unresolved_file_keeps_the_operation_waiting() {
    let Some(f) = Fixtures::new("waiting", &["two-files"]) else {
        return;
    };
    for operation in OPERATIONS {
        let dir = f.path("two-files", operation);
        let repo = f.repo("two-files", operation);
        let waiting = repo.operation_in_progress();
        assert!(waiting.is_some());
        let head = git(&dir, &["rev-parse", "HEAD"]).unwrap();
        take_right(&repo, "a.txt");
        // One file is still in conflict: continuing is refused and nothing is committed.
        let outcome = repo.continue_operation().unwrap();
        assert!(!outcome.ok, "{operation}: {}", outcome.output);
        assert_eq!(outcome.conflicts, ["b.txt"], "{operation}");
        assert_eq!(outcome.in_progress, waiting, "{operation}");
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]).unwrap(), head);
        assert!(
            std::fs::read_to_string(dir.join("b.txt"))
                .unwrap()
                .contains("<<<<<<<"),
            "the unresolved file is as Git left it"
        );
    }
}

#[test]
fn resolving_everything_finishes_the_operation() {
    let Some(f) = Fixtures::new("finish", &["two-files", "crlf", "three-in-one"]) else {
        return;
    };
    for operation in OPERATIONS {
        let dir = f.path("two-files", operation);
        let repo = f.repo("two-files", operation);
        take_right(&repo, "a.txt");
        take_right(&repo, "b.txt");
        assert_eq!(unmerged(&dir), "");
        let outcome = repo.continue_operation().unwrap();
        assert!(outcome.ok, "{operation}: {}", outcome.output);
        assert_eq!(outcome.in_progress, None);
        assert_eq!(status(&dir), "", "{operation}");
        // Right is the branch being merged or picked; in a rebase it is the target.
        let right = if operation == "rebase" {
            "main"
        } else {
            "feature"
        };
        for file in ["a", "b"] {
            assert_eq!(
                std::fs::read_to_string(dir.join(format!("{file}.txt"))).unwrap(),
                format!("{file}1\n{file}2 {right}\n{file}3\n"),
                "{operation}"
            );
        }
        let parents = String::from_utf8(git(&dir, &["log", "-1", "--format=%P"]).unwrap()).unwrap();
        assert_eq!(
            parents.split_whitespace().count(),
            if operation == "merge" { 2 } else { 1 },
            "{operation}"
        );
    }

    // A file with CRLF line ends keeps them, in what is shown and in what is written.
    let dir = f.path("crlf", "merge");
    let repo = f.repo("crlf", "merge");
    let c = repo.conflicts().unwrap().remove(0);
    let merge = merge_model(&c, false);
    assert_eq!(
        merge.text(
            &idecraft_vcs::Labels::default(),
            idecraft_vcs::MarkerStyle::Merge
        ),
        "first\r\n<<<<<<< ours\r\nsecond from main\r\n=======\r\nsecond from feature\r\n>>>>>>> \
         theirs\r\nthird\r\n"
    );
    take_right(&repo, "windows.txt");
    assert_eq!(
        std::fs::read(dir.join("windows.txt")).unwrap(),
        b"first\r\nsecond from feature\r\nthird\r\n"
    );
    assert!(repo.continue_operation().unwrap().ok);

    // Three conflicts, two of them simple: Resolve Simple leaves the real one open, and nothing
    // is written until it is resolved too.
    let dir = f.path("three-in-one", "cherry-pick");
    let repo = f.repo("three-in-one", "cherry-pick");
    let c = repo.conflicts().unwrap().remove(0);
    let mut merge = merge_model(&c, false);
    assert_eq!(merge.open_conflicts(), 3);
    assert_eq!(merge.resolve_simple(), 2);
    assert_eq!(merge.open_conflicts(), 1);
    assert_eq!(merge_outcome(&c, false, &merge), None);
    let open = merge
        .conflicts()
        .into_iter()
        .find(|&i| merge.chunks[i].resolution.is_none())
        .unwrap();
    merge.resolve(open, Some(Resolution::Ours));
    let outcome = merge_outcome(&c, false, &merge).unwrap();
    repo.apply_resolution("shapes.txt", &outcome).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("shapes.txt")).unwrap(),
        "let width = 11, height = 25;\nstable a\nstable b\nlet colour = green;\nstable c\nstable \
         d\nfn area(top, bottom) {}\nend\n",
        "both sides' words in the simple conflicts, ours in the real one"
    );
    let outcome = repo.continue_operation().unwrap();
    assert!(outcome.ok, "{}", outcome.output);
    assert_eq!(status(&dir), "");
}
