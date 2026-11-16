//! Line changes and unified hunks between two texts (`SPEC-0032` behaviour 4).

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, DiffOp, TextDiff};

/// Kind of a gutter marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineChangeKind {
    /// Lines only in the new text.
    Added,
    /// Lines replaced by other lines.
    Modified,
    /// Lines only in the old text (marker sits between new lines).
    Deleted,
}

/// One gutter marker; ranges are 1-based and inclusive, empty ranges have `end = start - 1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineChange {
    /// What changed.
    pub kind: LineChangeKind,
    /// First line in the new text.
    pub new_start: usize,
    /// Last line in the new text.
    pub new_end: usize,
    /// First line in the old text.
    pub old_start: usize,
    /// Last line in the old text.
    pub old_end: usize,
}

/// Gutter markers for `new` against `old`. Lines are compared without their separators, as the
/// editor shows them: a final newline is an empty last line, so removing it marks that line as
/// deleted instead of changing the line before it (SPEC-0064 v2).
pub fn line_changes(old: &str, new: &str) -> Vec<LineChange> {
    let old: Vec<&str> = editor_lines(old);
    let new: Vec<&str> = editor_lines(new);
    let diff = TextDiff::from_slices(&old, &new);
    let mut out: Vec<LineChange> = Vec::new();
    for op in diff.ops() {
        let (kind, o, n) = match *op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (LineChangeKind::Added, (old_index, 0), (new_index, new_len)),
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (
                LineChangeKind::Deleted,
                (old_index, old_len),
                (new_index, 0),
            ),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                LineChangeKind::Modified,
                (old_index, old_len),
                (new_index, new_len),
            ),
        };
        out.push(LineChange {
            kind,
            new_start: n.0 + 1,
            new_end: n.0 + n.1,
            old_start: o.0 + 1,
            old_end: o.0 + o.1,
        });
    }
    out
}

/// The lines of `text` as the editor numbers them, without separators.
fn editor_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// Unified hunks with three lines of context (`@@ -a,b +c,d @@` headers).
pub fn unified_hunks(old: &str, new: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(old, new);
    diff.unified_diff()
        .context_radius(3)
        .iter_hunks()
        .map(|hunk| {
            let mut text = format!("{}\n", hunk.header());
            for change in hunk.iter_changes() {
                let sign = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                text.push(sign);
                text.push_str(change.value());
                if change.missing_newline() {
                    text.push_str("\n\\ No newline at end of file\n");
                }
            }
            text
        })
        .collect()
}

/// Carries one value per old line over to the new lines: unchanged lines keep
/// it, inserted and replaced lines get `None` (`SPEC-0033` behaviour 2).
pub fn map_lines<T: Clone>(old: &str, new: &str, per_old_line: &[Option<T>]) -> Vec<Option<T>> {
    let diff = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    for op in diff.ops() {
        match *op {
            DiffOp::Equal { old_index, len, .. } => {
                for i in old_index..old_index + len {
                    out.push(per_old_line.get(i).cloned().flatten());
                }
            }
            DiffOp::Insert { new_len, .. } | DiffOp::Replace { new_len, .. } => {
                out.extend(std::iter::repeat_n(None, new_len));
            }
            DiffOp::Delete { .. } => {}
        }
    }
    out
}

/// One row of a side-by-side diff (`SPEC-0084`): the old line on the left, the new line on the right;
/// a side is `None` where the other side has extra lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffRow {
    /// What the row shows: unchanged, added, deleted or modified lines.
    pub kind: Option<LineChangeKind>,
    /// 1-based line number and text (without line break) on the old side.
    pub left: Option<(usize, String)>,
    /// 1-based line number and text (without line break) on the new side.
    pub right: Option<(usize, String)>,
}

/// The rows of a side-by-side diff of `old` and `new`: unchanged lines side by side, replaced lines
/// paired in order with the longer side padded, inserted and deleted lines against an empty side.
pub fn side_by_side(old: &str, new: &str) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(old, new);
    let olds: Vec<&str> = old.lines().collect();
    let news: Vec<&str> = new.lines().collect();
    let line = |v: &[&str], i: usize| v.get(i).map(|t| (i + 1, (*t).to_owned()));
    let mut rows = Vec::new();
    for op in diff.ops() {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for k in 0..len {
                    rows.push(DiffRow {
                        kind: None,
                        left: line(&olds, old_index + k),
                        right: line(&news, new_index + k),
                    });
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for k in 0..new_len {
                    rows.push(DiffRow {
                        kind: Some(LineChangeKind::Added),
                        left: None,
                        right: line(&news, new_index + k),
                    });
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for k in 0..old_len {
                    rows.push(DiffRow {
                        kind: Some(LineChangeKind::Deleted),
                        left: line(&olds, old_index + k),
                        right: None,
                    });
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                for k in 0..old_len.max(new_len) {
                    rows.push(DiffRow {
                        kind: Some(LineChangeKind::Modified),
                        left: (k < old_len).then(|| line(&olds, old_index + k)).flatten(),
                        right: (k < new_len).then(|| line(&news, new_index + k)).flatten(),
                    });
                }
            }
        }
    }
    rows
}

/// Indices of the rows where a change starts (targets of next/previous difference).
pub fn change_starts(rows: &[DiffRow]) -> Vec<usize> {
    (0..rows.len())
        .filter(|&i| rows[i].kind.is_some() && (i == 0 || rows[i - 1].kind != rows[i].kind))
        .collect()
}

/// The line change formed by the rows of the change starting at row `start` (`SPEC-0084` v2
/// behaviour 9): the rows up to the next row of another kind, with their old and new line ranges
/// in the numbering of [`line_changes`].
pub fn change_of_rows(rows: &[DiffRow], start: usize) -> Option<LineChange> {
    let kind = rows.get(start)?.kind?;
    let end = (start..rows.len())
        .find(|&i| rows[i].kind != Some(kind))
        .unwrap_or(rows.len());
    let before = |pick: fn(&DiffRow) -> Option<usize>| {
        rows[..start].iter().rev().find_map(pick).unwrap_or(0)
    };
    let (old_before, new_before) = (
        before(|r| r.left.as_ref().map(|l| l.0)),
        before(|r| r.right.as_ref().map(|l| l.0)),
    );
    let block = &rows[start..end];
    let olds = block.iter().filter(|r| r.left.is_some()).count();
    let news = block.iter().filter(|r| r.right.is_some()).count();
    Some(LineChange {
        kind,
        new_start: new_before + 1,
        new_end: new_before + news,
        old_start: old_before + 1,
        old_end: old_before + olds,
    })
}

/// The changes of side-by-side `rows` in their order: one per row where a change starts
/// (`SPEC-0084` v5 behaviour 23).
pub fn row_changes(rows: &[DiffRow]) -> Vec<LineChange> {
    change_starts(rows)
        .into_iter()
        .filter_map(|start| change_of_rows(rows, start))
        .collect()
}

/// The line break `text` uses: the first one found.
fn line_break(text: &str) -> Option<&'static str> {
    let at = text.find('\n')?;
    Some(if text[..at].ends_with('\r') {
        "\r\n"
    } else {
        "\n"
    })
}

/// A line without its line break, and whether it had one.
fn without_break(line: &str) -> (&str, bool) {
    match line.strip_suffix('\n') {
        Some(body) => (body.strip_suffix('\r').unwrap_or(body), true),
        None => (line, false),
    }
}

/// The text of the other side after `change` of a side-by-side diff of `left` and `right` was
/// taken over (`SPEC-0084` v5 behaviour 18): with `from_left` the new `right`, where the change's
/// right lines are replaced by its left lines; else the new `left`, the other way round. `change`
/// is numbered as [`change_of_rows`] numbers it. The lines taken get the line break of the text
/// they go into (the first one found there, else the source's); a last line without a line break
/// stays without one at the end of the text and gets one when lines follow it.
pub fn take_change(left: &str, right: &str, change: &LineChange, from_left: bool) -> String {
    let (source, target, taken, replaced) = if from_left {
        (
            left,
            right,
            (change.old_start, change.old_end),
            (change.new_start, change.new_end),
        )
    } else {
        (
            right,
            left,
            (change.new_start, change.new_end),
            (change.old_start, change.old_end),
        )
    };
    let source_lines: Vec<&str> = source.split_inclusive('\n').collect();
    let target_lines: Vec<&str> = target.split_inclusive('\n').collect();
    let part = |lines: &[&str], (start, end): (usize, usize)| {
        let to = end.min(lines.len());
        start.saturating_sub(1).min(to)..to
    };
    let (taken, replaced) = (part(&source_lines, taken), part(&target_lines, replaced));
    let br = line_break(target)
        .or_else(|| line_break(source))
        .unwrap_or("\n");
    let mut out = String::with_capacity(target.len() + source.len());
    for line in &target_lines[..replaced.start] {
        out.push_str(line);
    }
    let tail = &target_lines[replaced.end..];
    let taken = &source_lines[taken];
    // The line before the block ends the target without a line break: it gets one.
    if !taken.is_empty() && !out.is_empty() && !out.ends_with('\n') {
        out.push_str(br);
    }
    for (i, line) in taken.iter().enumerate() {
        let (body, ended) = without_break(line);
        out.push_str(body);
        if ended || i + 1 < taken.len() || !tail.is_empty() {
            out.push_str(br);
        }
    }
    for line in tail {
        out.push_str(line);
    }
    out
}

/// The lines of `text` with their separators' `\r` kept, as [`line_changes`] numbers them.
fn raw_lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// `lines[from..to]` (0-based, clamped).
fn slice<'a>(lines: &[&'a str], from: usize, to: usize) -> Vec<&'a str> {
    let to = to.min(lines.len());
    lines[from.min(to)..to].to_vec()
}

/// The text of `change` on the old side (`base`), without the last line break (`SPEC-0064` v3
/// behaviours 6–7); empty for an added change.
pub fn old_text(base: &str, change: &LineChange) -> String {
    let old = raw_lines(base);
    slice(&old, change.old_start - 1, change.old_end)
        .iter()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `text` with `change` taken back: its lines replaced by the old ones of `base` (`SPEC-0064` v3
/// behaviour 7, `SPEC-0084` v2 behaviour 9). Other lines keep their endings.
pub fn revert_change(base: &str, text: &str, change: &LineChange) -> String {
    let (old, new) = (raw_lines(base), raw_lines(text));
    let mut out = slice(&new, 0, change.new_start - 1);
    out.extend(slice(&old, change.old_start - 1, change.old_end));
    out.extend(slice(&new, change.new_end, new.len()));
    out.join("\n")
}

/// `base` with only `change` of `text` applied (`SPEC-0064` v3 behaviour 8: committing one change).
pub fn apply_change(base: &str, text: &str, change: &LineChange) -> String {
    let (old, new) = (raw_lines(base), raw_lines(text));
    let mut out = slice(&old, 0, change.old_start - 1);
    out.extend(slice(&new, change.new_start - 1, change.new_end));
    out.extend(slice(&old, change.old_end, old.len()));
    out.join("\n")
}

/// The change containing 0-based editor `line` of the new text: a change whose lines include it, or
/// a deletion whose boundary lies just above it (`SPEC-0064` v3 behaviour 7).
pub fn change_at_line(changes: &[LineChange], line: usize) -> Option<usize> {
    let line = line + 1;
    changes.iter().position(|c| {
        if c.new_end >= c.new_start {
            (c.new_start..=c.new_end).contains(&line)
        } else {
            c.new_start == line
        }
    })
}

/// Words of a line: runs of letters, digits and `_`, every other char on its own; char ranges.
fn words(line: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut i = 0;
    while i < chars.len() {
        let mut j = i + 1;
        if word(chars[i].1) {
            while j < chars.len() && word(chars[j].1) {
                j += 1;
            }
        }
        let from = chars[i].0;
        let to = chars.get(j).map_or(line.len(), |c| c.0);
        out.push((i..j, &line[from..to]));
        i = j;
    }
    out
}

/// Char ranges of changed words on the left and on the right of a line pair.
pub type WordRanges = (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>);

/// The changed words of a modified line pair as char ranges on the left and the right
/// (`SPEC-0084` v2 behaviour 8); `None` when more than half of the two lines differs or nothing does.
pub fn changed_words(left: &str, right: &str) -> Option<WordRanges> {
    let (a, b) = (words(left), words(right));
    let ta: Vec<&str> = a.iter().map(|w| w.1).collect();
    let tb: Vec<&str> = b.iter().map(|w| w.1).collect();
    let diff = TextDiff::from_slices(&ta, &tb);
    let (mut l, mut r): (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>) =
        (Vec::new(), Vec::new());
    let push = |out: &mut Vec<std::ops::Range<usize>>, range: std::ops::Range<usize>| {
        if range.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.end == range.start => last.end = range.end,
            _ => out.push(range),
        }
    };
    for op in diff.ops() {
        let (old, new) = (op.old_range(), op.new_range());
        if matches!(op, DiffOp::Equal { .. }) {
            continue;
        }
        if !old.is_empty() {
            push(&mut l, a[old.start].0.start..a[old.end - 1].0.end);
        }
        if !new.is_empty() {
            push(&mut r, b[new.start].0.start..b[new.end - 1].0.end);
        }
    }
    let changed: usize = l.iter().chain(&r).map(|x| x.len()).sum();
    let total = left.chars().count() + right.chars().count();
    if changed == 0 || changed * 2 > total {
        return None;
    }
    Some((l, r))
}

/// One row of the unified view (`SPEC-0084` v2 behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnifiedRow {
    /// `None` for an unchanged line, `Deleted` for a line of the old side of a change, `Added` for
    /// one of the new side.
    pub kind: Option<LineChangeKind>,
    /// 1-based old line number, where the line has one.
    pub old: Option<usize>,
    /// 1-based new line number, where the line has one.
    pub new: Option<usize>,
    /// The line's text.
    pub text: String,
    /// Index of the side-by-side row the line comes from.
    pub row: usize,
}

/// The unified rows of side-by-side `rows`: unchanged lines once, each change's old lines and then
/// its new lines.
pub fn unified_rows(rows: &[DiffRow]) -> Vec<UnifiedRow> {
    let mut out = Vec::with_capacity(rows.len());
    let mut i = 0;
    while i < rows.len() {
        let Some(kind) = rows[i].kind else {
            let r = &rows[i];
            out.push(UnifiedRow {
                kind: None,
                old: r.left.as_ref().map(|l| l.0),
                new: r.right.as_ref().map(|l| l.0),
                text: r
                    .right
                    .as_ref()
                    .or(r.left.as_ref())
                    .map(|l| l.1.clone())
                    .unwrap_or_default(),
                row: i,
            });
            i += 1;
            continue;
        };
        let end = (i..rows.len())
            .find(|&k| rows[k].kind != Some(kind))
            .unwrap_or(rows.len());
        for (k, r) in rows.iter().enumerate().take(end).skip(i) {
            if let Some((n, t)) = &r.left {
                out.push(UnifiedRow {
                    kind: Some(LineChangeKind::Deleted),
                    old: Some(*n),
                    new: None,
                    text: t.clone(),
                    row: k,
                });
            }
        }
        for (k, r) in rows.iter().enumerate().take(end).skip(i) {
            if let Some((n, t)) = &r.right {
                out.push(UnifiedRow {
                    kind: Some(LineChangeKind::Added),
                    old: None,
                    new: Some(*n),
                    text: t.clone(),
                    row: k,
                });
            }
        }
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_by_side_aligns_changes() {
        let old = "a\nb\nc\nd\n";
        let new = "a\nB\nB2\nc\ne\n";
        let rows = side_by_side(old, new);
        let show: Vec<(Option<LineChangeKind>, Option<usize>, Option<usize>)> = rows
            .iter()
            .map(|r| {
                (
                    r.kind,
                    r.left.as_ref().map(|l| l.0),
                    r.right.as_ref().map(|l| l.0),
                )
            })
            .collect();
        use LineChangeKind::*;
        assert_eq!(
            show,
            [
                (None, Some(1), Some(1)),
                (Some(Modified), Some(2), Some(2)),
                (Some(Modified), None, Some(3)),
                (None, Some(3), Some(4)),
                (Some(Modified), Some(4), Some(5)),
            ]
        );
        assert_eq!(rows[1].right.as_ref().unwrap().1, "B");
        assert_eq!(change_starts(&rows), [1, 4]);
        let added = side_by_side("a\n", "a\nb\n");
        assert_eq!(added[1].kind, Some(Added));
        assert!(added[1].left.is_none());
        let gone = side_by_side("a\nb\n", "a\n");
        assert_eq!(gone[1].kind, Some(Deleted));
        assert!(side_by_side("", "").is_empty());
    }

    fn kinds(old: &str, new: &str) -> Vec<(LineChangeKind, usize, usize, usize, usize)> {
        line_changes(old, new)
            .into_iter()
            .map(|c| (c.kind, c.new_start, c.new_end, c.old_start, c.old_end))
            .collect()
    }

    #[test]
    fn gutter_markers() {
        use LineChangeKind::*;
        let old = "a\nb\nc\nd\n";
        assert_eq!(kinds(old, "a\nx\nb\nc\nd\n"), [(Added, 2, 2, 2, 1)]);
        assert_eq!(kinds(old, "a\nc\nd\n"), [(Deleted, 2, 1, 2, 2)]);
        assert_eq!(kinds(old, "a\nB\nc\nd\n"), [(Modified, 2, 2, 2, 2)]);
        assert_eq!(kinds("", "x\ny\n"), [(Added, 1, 2, 1, 0)]);
        assert!(kinds(old, old).is_empty());
        // Removing the final newline deletes the empty last line; "d" itself is unchanged.
        assert_eq!(kinds(old, "a\nb\nc\nd"), [(Deleted, 5, 4, 5, 5)]);
        assert_eq!(kinds("a\r\nb\r\n", "a\r\nB\r\n"), [(Modified, 2, 2, 2, 2)]);
    }

    // SPEC-0064 v3 AK-5.
    #[test]
    fn one_change_reverted_and_applied() {
        let base = "a\r\nb\r\nc\r\nd\r\n";
        let text = "a\r\nB\r\nc\r\nx\r\nd\r\n";
        let changes = line_changes(base, text);
        assert_eq!(changes.len(), 2);
        let (modified, added) = (&changes[0], &changes[1]);
        assert_eq!(old_text(base, modified), "b");
        assert_eq!(old_text(base, added), "");
        assert_eq!(
            revert_change(base, text, modified),
            "a\r\nb\r\nc\r\nx\r\nd\r\n"
        );
        assert_eq!(revert_change(base, text, added), "a\r\nB\r\nc\r\nd\r\n");
        assert_eq!(apply_change(base, text, modified), "a\r\nB\r\nc\r\nd\r\n");
        assert_eq!(apply_change(base, text, added), "a\r\nb\r\nc\r\nx\r\nd\r\n");
        // A deletion comes back where it was, and is found on the line below its boundary.
        let gone = "a\nd\n";
        let changes = line_changes("a\nb\nc\nd\n", gone);
        assert_eq!(
            revert_change("a\nb\nc\nd\n", gone, &changes[0]),
            "a\nb\nc\nd\n"
        );
        assert_eq!(change_at_line(&changes, 1), Some(0));
        assert_eq!(change_at_line(&changes, 0), None);
        let both = line_changes(base, text);
        assert_eq!(change_at_line(&both, 1), Some(0));
        assert_eq!(change_at_line(&both, 3), Some(1));
        assert_eq!(change_at_line(&both, 2), None);
    }

    // SPEC-0084 v2 AK-5.
    #[test]
    fn changed_words_of_paired_lines() {
        let (l, r) = changed_words("int total = price * qty;", "int total = cost * qty;").unwrap();
        assert_eq!((l[0].clone(), r[0].clone()), (12..17, 12..16));
        assert_eq!((l.len(), r.len()), (1, 1));
        // Mostly different: the whole line is the change.
        assert!(changed_words("abc", "xyz").is_none());
        assert!(changed_words("same", "same").is_none());
        let (l, r) = changed_words("a(b, c)", "a(b, c, d)").unwrap();
        assert!(l.is_empty());
        assert_eq!((r.len(), r[0].clone()), (1, 6..9));
    }

    // SPEC-0084 v2 AK-5.
    #[test]
    fn unified_rows_follow_the_changes() {
        let rows = side_by_side("a\nb\nc\nd\n", "a\nB\nB2\nc\n");
        let u = unified_rows(&rows);
        type Shown<'a> = (
            Option<LineChangeKind>,
            Option<usize>,
            Option<usize>,
            &'a str,
        );
        let show: Vec<Shown> = u
            .iter()
            .map(|r| (r.kind, r.old, r.new, r.text.as_str()))
            .collect();
        use LineChangeKind::*;
        assert_eq!(
            show,
            [
                (None, Some(1), Some(1), "a"),
                (Some(Deleted), Some(2), None, "b"),
                (Some(Added), None, Some(2), "B"),
                (Some(Added), None, Some(3), "B2"),
                (None, Some(3), Some(4), "c"),
                (Some(Deleted), Some(4), None, "d"),
            ]
        );
        let first = change_of_rows(&rows, 1).unwrap();
        assert_eq!(
            (
                first.kind,
                first.new_start,
                first.new_end,
                first.old_start,
                first.old_end
            ),
            (Modified, 2, 3, 2, 2)
        );
        let last = change_of_rows(&rows, 4).unwrap();
        assert_eq!((last.kind, last.new_start, last.new_end), (Deleted, 5, 4));
        assert_eq!(
            revert_change("a\nb\nc\nd\n", "a\nB\nB2\nc\n", &last),
            "a\nB\nB2\nc\nd\n"
        );
        assert!(change_of_rows(&rows, 0).is_none());
    }

    /// Every change of the diff of `left` and `right`, taken alone from the left and from the
    /// right; taking all of them one after the other makes the sides equal.
    fn taken(left: &str, right: &str) -> Vec<(LineChangeKind, String, String)> {
        let changes = row_changes(&side_by_side(left, right));
        let each = changes
            .iter()
            .map(|c| {
                (
                    c.kind,
                    take_change(left, right, c, true),
                    take_change(left, right, c, false),
                )
            })
            .collect();
        // From the left, change by change, the right becomes the left; and the other way round.
        for from_left in [true, false] {
            let (mut l, mut r) = (left.to_owned(), right.to_owned());
            for _ in 0..=changes.len() {
                let Some(next) = row_changes(&side_by_side(&l, &r)).into_iter().next() else {
                    break;
                };
                let text = take_change(&l, &r, &next, from_left);
                if from_left {
                    r = text;
                } else {
                    l = text;
                }
            }
            assert_eq!(l, r, "from_left={from_left}: {left:?} / {right:?}");
            assert_eq!(l, if from_left { left } else { right });
        }
        each
    }

    // SPEC-0084 v5 AK-13.
    #[test]
    fn a_change_is_taken_in_both_directions() {
        use LineChangeKind::*;
        let left = "a\nb\nc\nd\ne\n";
        // Line 2 changed, a line added after c, e deleted.
        let right = "a\nB\nc\nnew\nd\n";
        assert_eq!(
            taken(left, right),
            [
                (
                    Modified,
                    "a\nb\nc\nnew\nd\n".to_owned(),
                    "a\nB\nc\nd\ne\n".to_owned()
                ),
                (
                    Added,
                    "a\nB\nc\nd\n".to_owned(),
                    "a\nb\nc\nnew\nd\ne\n".to_owned()
                ),
                (
                    Deleted,
                    "a\nB\nc\nnew\nd\ne\n".to_owned(),
                    "a\nb\nc\nd\n".to_owned()
                ),
            ]
        );
        // A changed block of uneven length, at the start and at the end of the text.
        assert_eq!(
            taken("x\ny\nz\n", "X1\nX2\ny\n"),
            [
                (Modified, "x\ny\n".to_owned(), "X1\nX2\ny\nz\n".to_owned()),
                (Deleted, "X1\nX2\ny\nz\n".to_owned(), "x\ny\n".to_owned()),
            ]
        );
        // An empty side, and no change at all.
        assert_eq!(
            taken("", "a\nb\n"),
            [(Added, String::new(), "a\nb\n".to_owned())]
        );
        assert!(taken("same\n", "same\n").is_empty());
        // A range past the end of a text that changed meanwhile is cut, not a panic.
        let stale = LineChange {
            kind: Modified,
            new_start: 7,
            new_end: 9,
            old_start: 8,
            old_end: 9,
        };
        assert_eq!(take_change("a\n", "b\n", &stale, true), "b\n");
    }

    // SPEC-0084 v5 AK-13.
    #[test]
    fn taken_lines_get_the_line_break_of_their_target() {
        use LineChangeKind::*;
        // Both sides with CRLF: only the block changes, every line keeps its break.
        assert_eq!(
            taken("a\r\nb\r\nc\r\n", "a\r\nB\r\nc\r\nd\r\n"),
            [
                (
                    Modified,
                    "a\r\nb\r\nc\r\nd\r\n".to_owned(),
                    "a\r\nB\r\nc\r\n".to_owned()
                ),
                (
                    Added,
                    "a\r\nB\r\nc\r\n".to_owned(),
                    "a\r\nb\r\nc\r\nd\r\n".to_owned()
                ),
            ]
        );
        // An LF line taken into a CRLF text gets CRLF, and the other way round. The sides differ
        // in every line break, so the diff is one block.
        let (left, right) = ("one\ntwo\n", "one\r\nTWO\r\n");
        let rows = side_by_side(left, right);
        let changes = row_changes(&rows);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            take_change(left, right, &changes[0], true),
            "one\r\ntwo\r\n"
        );
        assert_eq!(take_change(left, right, &changes[0], false), "one\nTWO\n");
        // Sides that differ only in their line breaks: taking writes nothing new.
        let (left, right) = ("one\ntwo\n", "one\r\ntwo\r\n");
        let changes = row_changes(&side_by_side(left, right));
        assert_eq!(take_change(left, right, &changes[0], true), right);
        assert_eq!(take_change(left, right, &changes[0], false), left);
        // A target without any line break takes the source's.
        let changes = row_changes(&side_by_side("a\r\nb\r\n", "a"));
        assert_eq!(
            take_change("a\r\nb\r\n", "a", &changes[0], true),
            "a\r\nb\r\n"
        );
    }

    // SPEC-0084 v5 AK-13.
    #[test]
    fn the_last_line_without_a_line_break() {
        use LineChangeKind::*;
        // Only the final line break differs: the last line is the change, in both directions.
        assert_eq!(
            taken("a\nb\n", "a\nb"),
            [(Modified, "a\nb\n".to_owned(), "a\nb".to_owned())]
        );
        // A line added after a last line without a line break, and taken away again.
        assert_eq!(
            taken("a\nb", "a\nb\nc"),
            [(Modified, "a\nb".to_owned(), "a\nb\nc".to_owned())]
        );
        // The last line changed and neither side ends with a line break.
        assert_eq!(
            taken("a\nb", "a\nB"),
            [(Modified, "a\nb".to_owned(), "a\nB".to_owned())]
        );
        // A line without a line break taken in front of lines that follow gets one.
        let (left, right) = ("x", "a\nb\n");
        let change = LineChange {
            kind: Added,
            new_start: 1,
            new_end: 0,
            old_start: 1,
            old_end: 1,
        };
        assert_eq!(take_change(left, right, &change, true), "x\na\nb\n");
        // A block taken behind a last line without a line break: that line gets one first.
        let change = LineChange {
            kind: Added,
            new_start: 2,
            new_end: 3,
            old_start: 2,
            old_end: 1,
        };
        assert_eq!(take_change("x", "x\na\nb\n", &change, false), "x\na\nb\n");
        // Deleted at the end: the text ends as the source does.
        assert_eq!(
            taken("a\n", "a\nb"),
            [(Added, "a\n".to_owned(), "a\nb".to_owned())]
        );
    }

    #[test]
    fn mapping_values_through_a_diff() {
        let old = "a\nb\nc\nd\n";
        let blame = [Some(1), Some(2), Some(3), Some(4)];
        assert_eq!(map_lines(old, old, &blame), blame);
        assert_eq!(
            map_lines(old, "a\nnew\nb\nC\nd\n", &blame),
            [Some(1), None, Some(2), None, Some(4)]
        );
        assert_eq!(map_lines(old, "a\nd\n", &blame), [Some(1), Some(4)]);
        assert_eq!(map_lines::<u8>("", "x\n", &[]), [None]);
    }

    #[test]
    fn hunks() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n";
        assert_eq!(
            unified_hunks(old, new),
            ["@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n"]
        );
        assert!(unified_hunks(old, old).is_empty());
        assert_eq!(
            unified_hunks("a", "b"),
            ["@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n"]
        );
    }
}
