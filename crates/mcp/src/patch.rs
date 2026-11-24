//! Unified diffs for `apply_patch` (`SPEC-0006` v59 behaviour 24): reading a patch of one or more
//! files and applying a file's hunks to its text, exactly, all or nothing.

/// One line of a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// In the old and the new text (` `).
    Context(String),
    /// Only in the old text (`-`).
    Removed(String),
    /// Only in the new text (`+`).
    Added(String),
}

/// One `@@` section of a file's patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The 1-based line of the old text the hunk starts at (the line after which it inserts when
    /// it has no old lines).
    pub old_start: usize,
    /// The hunk's lines in order.
    pub lines: Vec<Line>,
    /// The new text ends without a line ending after the hunk's last new line.
    pub new_without_newline: bool,
}

/// The patch of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    /// The workspace-relative path, as the patch names it (`a/`, `b/` dropped).
    pub path: String,
    /// The hunks in order.
    pub hunks: Vec<Hunk>,
}

/// The most files one patch may change (`SPEC-0006` v59 behaviour 24 c).
pub const MAX_FILES: usize = 100;

/// The most bytes of patch text one call takes (`SPEC-0006` v59 behaviour 24 c).
pub const MAX_BYTES: usize = 1 << 20;

/// A file's name from a `---` or `+++` line: up to a tab, `None` for `/dev/null`.
fn name_of(rest: &str) -> Option<String> {
    let name = rest.split('\t').next().unwrap_or("").trim_end();
    let name = name
        .strip_prefix('"')
        .and_then(|n| n.strip_suffix('"'))
        .unwrap_or(name);
    (name != "/dev/null" && !name.is_empty()).then(|| name.to_owned())
}

/// `old,count` or `old` of a hunk header: the start and the count (default 1).
fn range(text: &str) -> Option<(usize, usize)> {
    let (start, count) = match text.split_once(',') {
        Some((s, c)) => (s, c.parse().ok()?),
        None => (text, 1),
    };
    Some((start.parse().ok()?, count))
}

/// `@@ -a,b +c,d @@ …`: the old start, the old count and the new count.
fn hunk_header(line: &str) -> Option<(usize, usize, usize)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let (old_start, old_count) = range(old)?;
    let (_, new_count) = range(new)?;
    Some((old_start, old_count, new_count))
}

/// Why a Git header line refuses the file, with the tool to use instead.
fn refused_header(line: &str) -> Option<&'static str> {
    [
        ("new file mode", "a new file: create it with `create_file`"),
        (
            "deleted file mode",
            "a deletion: delete the file with `delete_file`",
        ),
        (
            "rename from",
            "a rename: rename or move the file with `move_file`",
        ),
        ("copy from", "a copy: create the copy with `create_file`"),
        (
            "old mode",
            "a change of the file's mode, which apply_patch does not make",
        ),
        ("GIT binary patch", "a binary patch"),
        ("Binary files", "a binary patch"),
    ]
    .iter()
    .find(|(prefix, _)| line.starts_with(prefix))
    .map(|(_, why)| *why)
}

/// The files of `patch` with their hunks, in order; an error names the patch's 1-based line and
/// what is wrong, or what is refused and the tool to use instead.
pub fn parse(patch: &str) -> Result<Vec<FilePatch>, String> {
    let mut lines: Vec<&str> = patch
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    // The line ending of the last line is no empty line after it.
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut files: Vec<FilePatch> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(why) = refused_header(line) {
            return Err(format!("line {}: {why}; nothing changed", i + 1));
        }
        let (Some(old), Some(new)) = (
            line.strip_prefix("--- "),
            lines.get(i + 1).and_then(|l| l.strip_prefix("+++ ")),
        ) else {
            if line.starts_with("@@ ") {
                return Err(format!(
                    "line {}: a hunk without the `---` and `+++` lines naming its file",
                    i + 1
                ));
            }
            i += 1;
            continue;
        };
        let at = i + 1;
        let (old, new) = (name_of(old), name_of(new));
        let path = match (old, new) {
            (None, _) => {
                return Err(format!(
                    "line {at}: a new file: create it with `create_file`; nothing changed"
                ));
            }
            (_, None) => {
                return Err(format!(
                    "line {at}: a deletion: delete the file with `delete_file`; nothing changed"
                ));
            }
            (Some(old), Some(new)) => {
                let (old, new) = match (old.strip_prefix("a/"), new.strip_prefix("b/")) {
                    (Some(o), Some(n)) => (o.to_owned(), n.to_owned()),
                    _ => (old, new),
                };
                if old != new {
                    return Err(format!(
                        "line {at}: a rename from {old} to {new}: rename or move the file with `move_file`; nothing changed"
                    ));
                }
                new
            }
        };
        if files.iter().any(|f| f.path == path) {
            return Err(format!(
                "line {at}: {path} is named in two sections; give each file once"
            ));
        }
        i += 2;
        let mut hunks = Vec::new();
        while let Some((old_start, old_count, new_count)) =
            lines.get(i).and_then(|l| hunk_header(l))
        {
            let header = i + 1;
            i += 1;
            let (mut old_left, mut new_left) = (old_count, new_count);
            let mut hunk = Hunk {
                old_start,
                lines: Vec::new(),
                new_without_newline: false,
            };
            while old_left > 0 || new_left > 0 {
                let Some(&l) = lines.get(i) else {
                    return Err(format!(
                        "line {header}: the hunk ends before its {old_count} old and {new_count} new lines"
                    ));
                };
                let (kind, text) = match l.chars().next() {
                    Some(' ') => (' ', &l[1..]),
                    // An editor may drop the space of an empty context line.
                    None => (' ', ""),
                    Some('-') => ('-', &l[1..]),
                    Some('+') => ('+', &l[1..]),
                    Some('\\') => {
                        i += 1;
                        continue;
                    }
                    _ => {
                        return Err(format!(
                            "line {}: `{l}` in the hunk of line {header}, which still needs {old_left} old and {new_left} new lines",
                            i + 1
                        ));
                    }
                };
                let (old_takes, new_takes) = match kind {
                    ' ' => (1, 1),
                    '-' => (1, 0),
                    _ => (0, 1),
                };
                if old_left < old_takes || new_left < new_takes {
                    return Err(format!(
                        "line {}: more lines than the hunk of line {header} counts",
                        i + 1
                    ));
                }
                old_left -= old_takes;
                new_left -= new_takes;
                hunk.lines.push(match kind {
                    ' ' => Line::Context(text.to_owned()),
                    '-' => Line::Removed(text.to_owned()),
                    _ => Line::Added(text.to_owned()),
                });
                i += 1;
            }
            // `\ No newline at end of file` after the hunk's last lines: it is about the line
            // before it, which ends the old text (after `-`), the new text (after `+`) or both.
            while lines.get(i).is_some_and(|l| l.starts_with('\\')) {
                if !matches!(hunk.lines.last(), Some(Line::Removed(_))) {
                    hunk.new_without_newline = true;
                }
                i += 1;
            }
            hunks.push(hunk);
        }
        if hunks.is_empty() {
            return Err(format!("line {at}: {path} has no hunk (`@@ … @@`)"));
        }
        files.push(FilePatch { path, hunks });
        if files.len() > MAX_FILES {
            return Err(format!("has more than {MAX_FILES} files; split it"));
        }
    }
    if files.is_empty() {
        return Err("names no file (`--- a/<path>` and `+++ b/<path>`)".to_owned());
    }
    Ok(files)
}

/// The paths a patch names, for the secret-file guard; lenient, so a patch that does not parse
/// still has its names checked.
pub fn paths(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|l| l.strip_prefix("+++ ").or_else(|| l.strip_prefix("--- ")))
        .filter_map(name_of)
        .map(|n| {
            n.strip_prefix("a/")
                .or_else(|| n.strip_prefix("b/"))
                .map_or(n.clone(), str::to_owned)
        })
        .collect()
}

/// What applying a file's patch gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The new text.
    pub text: String,
    /// Lines added and removed.
    pub added: usize,
    /// Lines removed.
    pub removed: usize,
    /// The hunks (1-based) applied away from their line, and by how many lines.
    pub offsets: Vec<(usize, i64)>,
}

/// Applies `file`'s hunks to `text`: each where its context and removed lines equal the text's
/// lines exactly (line endings aside), at its line or the nearest place after the previous hunk.
/// An error names the hunk and its line; nothing is half applied.
pub fn apply(text: &str, file: &FilePatch) -> Result<Applied, String> {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<(&str, &str)> = text
        .split_inclusive('\n')
        .map(|l| {
            let content = l.strip_suffix('\n').unwrap_or(l);
            let content = content.strip_suffix('\r').unwrap_or(content);
            (content, &l[content.len()..])
        })
        .collect();
    let mut out = String::with_capacity(text.len());
    let (mut added, mut removed) = (0, 0);
    let mut offsets = Vec::new();
    let mut next = 0usize;
    for (n, hunk) in file.hunks.iter().enumerate() {
        let number = n + 1;
        let old: Vec<&str> = hunk
            .lines
            .iter()
            .filter_map(|l| match l {
                Line::Context(t) | Line::Removed(t) => Some(t.as_str()),
                Line::Added(_) => None,
            })
            .collect();
        let expected = if old.is_empty() {
            hunk.old_start
        } else {
            hunk.old_start.saturating_sub(1)
        };
        let fits = |p: usize| {
            p >= next
                && p + old.len() <= lines.len()
                && old.iter().enumerate().all(|(k, o)| lines[p + k].0 == *o)
        };
        let at = if fits(expected) {
            Some(expected)
        } else {
            // The nearest place, looking both ways from the expected line.
            let last = lines.len().saturating_sub(old.len());
            (1..=lines.len().max(expected))
                .flat_map(|d| [expected.checked_sub(d), expected.checked_add(d)])
                .flatten()
                .filter(|p| *p <= last)
                .find(|p| fits(*p))
        };
        let Some(at) = at else {
            return Err(format!(
                "hunk {number} (line {}): its {} context and removed lines are not in the file{}",
                hunk.old_start,
                old.len(),
                if expected < next {
                    " after the hunk before it"
                } else {
                    ""
                }
            ));
        };
        if at != expected {
            offsets.push((number, at as i64 - expected as i64));
        }
        for (content, ending) in &lines[next..at] {
            out.push_str(content);
            out.push_str(ending);
        }
        let news: Vec<&str> = hunk
            .lines
            .iter()
            .filter_map(|l| match l {
                Line::Context(t) | Line::Added(t) => Some(t.as_str()),
                Line::Removed(_) => None,
            })
            .collect();
        let at_end = at + old.len() == lines.len();
        for (k, line) in news.iter().enumerate() {
            out.push_str(line);
            let last = k + 1 == news.len();
            if !(last && at_end && hunk.new_without_newline) {
                out.push_str(eol);
            }
        }
        for l in &hunk.lines {
            match l {
                Line::Added(_) => added += 1,
                Line::Removed(_) => removed += 1,
                Line::Context(_) => {}
            }
        }
        next = at + old.len();
    }
    for (content, ending) in &lines[next..] {
        out.push_str(content);
        out.push_str(ending);
    }
    Ok(Applied {
        text: out,
        added,
        removed,
        offsets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "diff --git a/src/a.txt b/src/a.txt\nindex 1..2 100644\n--- a/src/a.txt\n+++ b/src/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n--- b.txt\t2026-01-01\n+++ b.txt\t2026-01-02\n@@ -2 +2,2 @@\n-b\n+B\n+C\n";

    #[test]
    fn reads_files_and_hunks_of_git_and_plain_diffs() {
        let files = parse(TWO).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.txt");
        assert_eq!(files[1].path, "b.txt");
        assert_eq!(
            files[0].hunks[0].lines,
            [
                Line::Context("one".into()),
                Line::Removed("two".into()),
                Line::Added("TWO".into()),
                Line::Context("three".into())
            ]
        );
        assert_eq!(files[1].hunks[0].old_start, 2);
        assert_eq!(paths(TWO), ["src/a.txt", "src/a.txt", "b.txt", "b.txt"]);
    }

    #[test]
    fn refuses_new_deleted_renamed_and_binary_files() {
        let cases = [
            (
                "--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1 @@\n+x\n",
                "create_file",
            ),
            (
                "--- a/n.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n",
                "delete_file",
            ),
            (
                "--- a/x.txt\n+++ b/y.txt\n@@ -1 +1 @@\n-x\n+y\n",
                "move_file",
            ),
            (
                "diff --git a/x b/y\nrename from x\nrename to y\n",
                "move_file",
            ),
            ("diff --git a/x b/x\nGIT binary patch\n", "binary"),
            ("", "names no file"),
            ("@@ -1 +1 @@\n-a\n+b\n", "without the `---`"),
            ("--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+b\n", "ends before"),
            (
                "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n",
                "two sections",
            ),
            ("--- a/x\n+++ b/x\n", "no hunk"),
        ];
        for (patch, why) in cases {
            let e = parse(patch).unwrap_err();
            assert!(e.contains(why), "{patch:?}: {e}");
        }
    }

    #[test]
    fn applies_exactly_with_offsets_and_line_endings() {
        let files = parse(TWO).unwrap();
        let a = apply("one\ntwo\nthree\n", &files[0]).unwrap();
        assert_eq!(a.text, "one\nTWO\nthree\n");
        assert_eq!((a.added, a.removed), (1, 1));
        assert!(a.offsets.is_empty());
        // Two lines above: the hunk applies two lines down.
        let a = apply("x\ny\none\ntwo\nthree\nz", &files[0]).unwrap();
        assert_eq!(a.text, "x\ny\none\nTWO\nthree\nz");
        assert_eq!(a.offsets, [(1, 2)]);
        // CRLF stays CRLF.
        let a = apply("one\r\ntwo\r\nthree\r\n", &files[0]).unwrap();
        assert_eq!(a.text, "one\r\nTWO\r\nthree\r\n");
        // Whitespace counts; no fuzzy match.
        let e = apply("one\ntwo \nthree\n", &files[0]).unwrap_err();
        assert!(e.contains("hunk 1 (line 1)"), "{e}");
        let b = apply("a\nb\n", &files[1]).unwrap();
        assert_eq!(b.text, "a\nB\nC\n");
    }

    #[test]
    fn keeps_or_drops_the_last_line_ending_as_the_patch_says() {
        let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n\\ No newline at end of file\n";
        let files = parse(patch).unwrap();
        assert!(files[0].hunks[0].new_without_newline);
        assert_eq!(apply("a\n", &files[0]).unwrap().text, "b");
        let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n";
        let files = parse(patch).unwrap();
        assert!(!files[0].hunks[0].new_without_newline);
        assert_eq!(apply("a", &files[0]).unwrap().text, "b\n");
    }

    #[test]
    fn inserts_into_an_empty_file_and_after_a_line() {
        let files = parse("--- a/x\n+++ b/x\n@@ -0,0 +1,2 @@\n+a\n+b\n").unwrap();
        assert_eq!(apply("", &files[0]).unwrap().text, "a\nb\n");
        let files = parse("--- a/x\n+++ b/x\n@@ -1,0 +2 @@\n+mid\n").unwrap();
        assert_eq!(apply("a\nb\n", &files[0]).unwrap().text, "a\nmid\nb\n");
    }

    #[test]
    fn hunks_apply_in_order_and_never_overlap() {
        let patch = "--- a/x\n+++ b/x\n@@ -3 +3 @@\n-c\n+C\n@@ -1 +1 @@\n-a\n+A\n";
        let files = parse(patch).unwrap();
        let e = apply("a\nb\nc\n", &files[0]).unwrap_err();
        assert!(
            e.contains("hunk 2") && e.contains("after the hunk before it"),
            "{e}"
        );
        let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+A\n@@ -3 +3 @@\n-c\n+C\n";
        let files = parse(patch).unwrap();
        assert_eq!(apply("a\nb\nc\n", &files[0]).unwrap().text, "A\nb\nC\n");
    }
}
