//! SPEC-0037 AK-3: merged text equals `git merge-file -p`.

use std::process::Command;

use ferris_idea_vcs::{Labels, MarkerStyle, Merge3};

fn git_merge_file(base: &str, ours: &str, theirs: &str, diff3: bool) -> Option<String> {
    // One folder per call: the tests of this file run at the same time.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-merge3-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    for (name, text) in [("base", base), ("ours", ours), ("theirs", theirs)] {
        std::fs::write(dir.join(name), text).ok()?;
    }
    let mut cmd = Command::new("git");
    cmd.args(["merge-file", "-p"]);
    if diff3 {
        cmd.arg("--diff3");
    }
    cmd.args([
        "-L", "ours", "-L", "base", "-L", "theirs", "ours", "base", "theirs",
    ])
    .current_dir(&dir);
    let out = cmd.output().ok()?;
    let _ = std::fs::remove_dir_all(&dir);
    // The exit code is the number of conflicts; only signals are failures.
    out.status.code()?;
    Some(String::from_utf8(out.stdout).unwrap())
}

const CASES: &[(&str, &str, &str)] = &[
    // Different lines changed.
    ("a\nb\nc\nd\ne\n", "a\nB\nc\nd\ne\n", "a\nb\nc\nD\ne\n"),
    // Same change on both sides.
    ("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n"),
    // Simple conflict.
    ("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n"),
    // Conflict with common lines inside (zealous trimming).
    ("a\nb\nc\n", "a\nsame\nO\nend\nc\n", "a\nsame\nT\nend\nc\n"),
    // Insertions at both ends and a deletion.
    ("a\nb\nc\nd\n", "start\na\nb\nc\nd\n", "a\nc\nd\nend\n"),
    // Two separate conflicts.
    (
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n",
        "1\nX\n3\n4\n5\n6\n7\nY\n9\n",
        "1\nx\n3\n4\n5\n6\n7\ny\n9\n",
    ),
    // One side deletes what the other changes.
    ("a\nb\nc\n", "a\nc\n", "a\nB\nc\n"),
    // No line break at the end.
    ("a\nb", "A\nb", "a\nb"),
    ("a", "o", "t"),
];

#[test]
fn equals_git_merge_file() {
    for &(base, ours, theirs) in CASES {
        for (diff3, style) in [(false, MarkerStyle::Merge), (true, MarkerStyle::Diff3)] {
            let Some(expected) = git_merge_file(base, ours, theirs, diff3) else {
                return;
            };
            let merge = if diff3 {
                Merge3::untrimmed(base, ours, theirs)
            } else {
                Merge3::new(base, ours, theirs)
            };
            let merged = merge.text(&Labels::default(), style);
            assert_eq!(
                merged, expected,
                "base {base:?} ours {ours:?} theirs {theirs:?} diff3 {diff3}"
            );
        }
    }
}

/// SPEC-0037 AK-7 (v3): the marker lines end as the file's lines do, byte for byte as
/// `git merge-file -p` writes them.
#[test]
fn markers_end_like_the_lines_of_the_file() {
    let cases: &[(&str, &str, &str)] = &[
        // CRLF everywhere.
        (
            "a\r\nb\r\nc\r\n",
            "a\r\nours\r\nc\r\n",
            "a\r\ntheirs\r\nc\r\n",
        ),
        // The conflict starts the file.
        ("a\r\nb\r\n", "ours\r\nb\r\n", "theirs\r\nb\r\n"),
        // The conflict ends the file, the sides have no final line break.
        ("a\r\nb", "a\r\nours", "a\r\ntheirs"),
        ("a\r\nb\r\n", "a\r\nours", "a\r\ntheirs\r\n"),
        // Both sides added a CRLF file: without a base line the markers end with LF.
        ("", "one\r\nours\r\n", "one\r\ntheirs\r\n"),
        ("", "ours\r\n", "theirs\r\n"),
        // One side converted the file.
        ("a\nb\nc\n", "a\r\nours\r\nc\r\n", "a\ntheirs\nc\n"),
        ("a\r\nb\r\nc\r\n", "a\r\nours\r\nc\r\n", "a\ntheirs\nc\n"),
        ("a\nb\nc\n", "a\r\nours\r\nc\r\n", "a\r\ntheirs\r\nc\r\n"),
        // Mixed line ends: the line before the conflict decides.
        ("a\nb\r\nc\nd\n", "a\nb\r\nours\nd\n", "a\nb\r\ntheirs\nd\n"),
        (
            "a\r\nb\nc\r\nd\r\n",
            "a\r\nb\nours\r\nd\r\n",
            "a\r\nb\ntheirs\r\nd\r\n",
        ),
        // A single line without a line break on each side.
        ("a\r\n", "ours", "theirs"),
        // Plain LF stays LF.
        ("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n"),
    ];
    for &(base, ours, theirs) in cases {
        for (diff3, style) in [(false, MarkerStyle::Merge), (true, MarkerStyle::Diff3)] {
            let Some(expected) = git_merge_file(base, ours, theirs, diff3) else {
                return;
            };
            let merge = if diff3 {
                Merge3::untrimmed(base, ours, theirs)
            } else {
                Merge3::new(base, ours, theirs)
            };
            assert_eq!(
                merge.text(&Labels::default(), style),
                expected,
                "base {base:?} ours {ours:?} theirs {theirs:?} diff3 {diff3}"
            );
        }
    }
}
