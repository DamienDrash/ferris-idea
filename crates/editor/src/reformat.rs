//! Applying a formatter's result to an open document so that carets and selections stay on their
//! text (`SPEC-0329` v3 behaviour 7, `SPEC-0123` v3 behaviours 3 and 4): the result becomes small edits,
//! one per changed region of lines, and every caret end is carried over by those edits.
//!
//! All offsets are char offsets, as in [`crate::TextBuffer`].

use std::ops::Range;

use crate::buffer::line_hunks;

/// The edits that turn `old` into `new`: one per changed region of whole lines (a longest common
/// subsequence of lines), each shrunk to the chars that differ. Char ranges in `old`, in order.
pub fn reformat_edits(old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    // Char offset → byte offset of `old`, built once.
    let bytes: Vec<usize> = old
        .char_indices()
        .map(|(b, _)| b)
        .chain([old.len()])
        .collect();
    for (range, text) in line_hunks(old, new) {
        let was = &old[bytes[range.start]..bytes[range.end]];
        let prefix = was
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let (was_n, text_n) = (was.chars().count(), text.chars().count());
        let suffix = was
            .chars()
            .rev()
            .zip(text.chars().rev())
            .take(was_n.min(text_n) - prefix)
            .take_while(|(a, b)| a == b)
            .count();
        let inserted: String = text
            .chars()
            .skip(prefix)
            .take(text_n - prefix - suffix)
            .collect();
        out.push((range.start + prefix..range.end - suffix, inserted));
    }
    out
}

/// Where the char `offset` of `old` lies after `edits` ([`reformat_edits`] of `old`):
/// - in text no edit touches, it moves by the edits before it;
/// - inside a reformatted region, it stays before the same visible character: the characters
///   other than white space of both sides are matched, and the offset goes before the match of
///   the first visible character at or after it (after the match of the last one before it,
///   when none follows);
/// - in deleted text, it goes where that text was.
pub fn map_offset(old: &str, edits: &[(Range<usize>, String)], offset: usize) -> usize {
    let mut shift: isize = 0;
    for (range, text) in edits {
        let added = text.chars().count();
        if offset <= range.start {
            break;
        }
        if offset >= range.end {
            shift += added as isize - range.len() as isize;
            continue;
        }
        // Inside the edit.
        let start = (range.start as isize + shift) as usize;
        let was: Vec<char> = old.chars().skip(range.start).take(range.len()).collect();
        let now: Vec<char> = text.chars().collect();
        return start + inside(&was, &now, offset - range.start);
    }
    (offset as isize + shift).max(0) as usize
}

/// The position in `now` for position `at` of `was` (see [`map_offset`]).
fn inside(was: &[char], now: &[char], at: usize) -> usize {
    let matched = align(was, now);
    if let Some(j) = (at..was.len()).find_map(|i| matched[i]) {
        return j;
    }
    if let Some(j) = (0..at.min(was.len())).rev().find_map(|i| matched[i]) {
        return j + 1;
    }
    0
}

/// For each char of `was`, the index of its match in `now`, matching only characters that are not
/// white space, in order: a longest common subsequence, or beyond a size limit a forward search.
fn align(was: &[char], now: &[char]) -> Vec<Option<usize>> {
    let a: Vec<usize> = (0..was.len())
        .filter(|&i| !was[i].is_whitespace())
        .collect();
    let b: Vec<usize> = (0..now.len())
        .filter(|&j| !now[j].is_whitespace())
        .collect();
    let mut matched = vec![None; was.len()];
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) <= 4_000_000 {
        let mut lcs = vec![0u32; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * (m + 1) + j] = if was[a[i]] == now[b[j]] {
                    lcs[(i + 1) * (m + 1) + j + 1] + 1
                } else {
                    lcs[(i + 1) * (m + 1) + j].max(lcs[i * (m + 1) + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if was[a[i]] == now[b[j]] {
                matched[a[i]] = Some(b[j]);
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * (m + 1) + j] >= lcs[i * (m + 1) + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    } else {
        let mut j = 0;
        for &i in &a {
            if let Some(k) = (j..m).take(64).find(|&k| now[b[k]] == was[i]) {
                matched[i] = Some(b[k]);
                j = k + 1;
            }
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, edits: &[(Range<usize>, String)]) -> String {
        let mut chars: Vec<char> = old.chars().collect();
        for (r, t) in edits.iter().rev() {
            chars.splice(r.clone(), t.chars());
        }
        chars.into_iter().collect()
    }

    /// The char offset of the first `needle` in `text`, plus `plus`.
    fn at(text: &str, needle: &str, plus: usize) -> usize {
        text[..text.find(needle).unwrap()].chars().count() + plus
    }

    const OLD: &str = "import os\n\nclass Shape:\n    def area( self ):\n        return 1\n    def sides(self):\n        return [ 'a','b' ]\n";
    const NEW: &str = "class Shape:\n    def area(self):\n        return 1\n\n    def sides(self):\n        return [\"a\", \"b\"]\n";

    #[test]
    fn the_edits_give_the_new_text_and_leave_unchanged_lines_alone() {
        let edits = reformat_edits(OLD, NEW);
        assert_eq!(apply(OLD, &edits), NEW);
        assert!(edits.len() >= 3, "{edits:?}");
        // No edit touches the unchanged `return 1` line.
        let line = at(OLD, "        return 1", 0);
        assert!(
            edits
                .iter()
                .all(|(r, _)| r.end <= line || r.start >= line + 17)
        );
        assert!(reformat_edits(OLD, OLD).is_empty());
        assert_eq!(apply("é\n", &reformat_edits("é\n", "ü\n")), "ü\n");
    }

    /// OBS-0034 item 46: a caret in `area` stayed in `area`'s line no longer; it moved to `sides`.
    #[test]
    fn a_caret_in_unchanged_text_stays_on_its_text() {
        let edits = reformat_edits(OLD, NEW);
        let caret = at(OLD, "return 1", 3);
        assert_eq!(map_offset(OLD, &edits, caret), at(NEW, "return 1", 3));
        let caret = at(OLD, "sides", 2);
        assert_eq!(map_offset(OLD, &edits, caret), at(NEW, "sides", 2));
        assert_eq!(
            map_offset(OLD, &edits, OLD.chars().count()),
            NEW.chars().count()
        );
    }

    #[test]
    fn a_caret_in_reformatted_text_stays_before_the_same_character() {
        let edits = reformat_edits(OLD, NEW);
        // In `area`, on a line whose spaces change.
        let caret = at(OLD, "area", 2);
        assert_eq!(map_offset(OLD, &edits, caret), at(NEW, "area", 2));
        // Before `self` among removed spaces.
        let caret = at(OLD, "self )", 0);
        assert_eq!(map_offset(OLD, &edits, caret), at(NEW, "self)", 0));
        // In the list whose quotes and spaces change: before `b`.
        let caret = at(OLD, "b' ]", 0);
        assert_eq!(map_offset(OLD, &edits, caret), at(NEW, "b\"]", 0));
    }

    #[test]
    fn a_caret_in_deleted_text_goes_where_the_text_was() {
        let edits = reformat_edits(OLD, NEW);
        let caret = at(OLD, "os", 1);
        assert_eq!(map_offset(OLD, &edits, caret), 0);
        let old = "a = 1\nimport sys\nb = 2\n";
        let new = "a = 1\nb = 2\n";
        let edits = reformat_edits(old, new);
        assert_eq!(map_offset(old, &edits, at(old, "sys", 2)), at(new, "b", 0));
        assert_eq!(map_offset(old, &edits, at(old, "b", 0)), at(new, "b", 0));
    }

    #[test]
    fn a_selection_keeps_both_ends_on_their_text() {
        let edits = reformat_edits(OLD, NEW);
        let (anchor, head) = (at(OLD, "def area", 0), at(OLD, "return 1", 8));
        let (a, h) = (
            map_offset(OLD, &edits, anchor),
            map_offset(OLD, &edits, head),
        );
        let new: Vec<char> = NEW.chars().collect();
        assert_eq!(
            new[a..h].iter().collect::<String>(),
            "def area(self):\n        return 1"
        );
    }
}
