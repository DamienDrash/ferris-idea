//! The change bar of a gutter marker (`SPEC-0064` v3 behaviours 6–9), free of the UI toolkit:
//! which change a click hits, the neighbouring changes, the HEAD text shown and the words the status
//! bar uses after an action.

use ferris_idea_vcs::{LineChange, LineChangeKind};

/// Most HEAD lines the bar shows (behaviour 6).
pub const OLD_LINES_SHOWN: usize = 12;

/// The change a click on the marker column hits: `line` is the 0-based line under the pointer and
/// `upper` whether the pointer is in the upper half of it. A change's bar covers its lines; a
/// deletion's mark lies on the boundary, so it is hit from the lower half of the line above and the
/// upper half of the line below (behaviour 6).
pub fn change_at(changes: &[LineChange], line: usize, upper: bool) -> Option<usize> {
    let one = line + 1;
    changes
        .iter()
        .position(|c| c.new_end >= c.new_start && (c.new_start..=c.new_end).contains(&one))
        .or_else(|| {
            changes.iter().position(|c| {
                c.new_end < c.new_start
                    && ((upper && c.new_start == one) || (!upper && c.new_start == one + 1))
            })
        })
}

/// The change before (`forward == false`) or after `index`, stopping at the ends (behaviour 7).
pub fn neighbour(changes: &[LineChange], index: usize, forward: bool) -> Option<usize> {
    if forward {
        (index + 1 < changes.len()).then_some(index + 1)
    } else {
        index.checked_sub(1).filter(|_| !changes.is_empty())
    }
}

/// The first change after (or before) the 0-based caret line, for `VcsShowNextChangeMarker` and
/// `VcsShowPrevChangeMarker` (behaviour 7).
pub fn from_caret(changes: &[LineChange], caret_line: usize, forward: bool) -> Option<usize> {
    let first = |c: &LineChange| c.new_start.saturating_sub(1);
    if forward {
        changes.iter().position(|c| first(c) > caret_line)
    } else {
        changes.iter().rposition(|c| {
            let last = c.new_end.max(c.new_start).saturating_sub(1);
            last < caret_line
        })
    }
}

/// The 0-based line the bar opens below: the change's last line, or for a deletion the line above
/// its boundary.
pub fn anchor_line(change: &LineChange) -> usize {
    if change.new_end >= change.new_start {
        change.new_end - 1
    } else {
        change.new_start.saturating_sub(2)
    }
}

/// The HEAD lines the bar shows: at most [`OLD_LINES_SHOWN`], then a line saying how many more
/// there are; none for an added change (behaviour 6).
pub fn shown_old_lines(old: &str, change: &LineChange) -> Vec<String> {
    if change.kind == LineChangeKind::Added {
        return Vec::new();
    }
    let lines: Vec<&str> = old.split('\n').collect();
    let mut out: Vec<String> = lines
        .iter()
        .take(OLD_LINES_SHOWN)
        .map(|l| l.replace('\t', "    "))
        .collect();
    if lines.len() > OLD_LINES_SHOWN {
        out.push(format!("… {} more lines", lines.len() - OLD_LINES_SHOWN));
    }
    out
}

/// "lines 4–6", "line 4" or "the deleted lines after line 3" (behaviour 9).
pub fn describe(change: &LineChange) -> String {
    match change.kind {
        _ if change.new_end < change.new_start => {
            format!("the deleted lines after line {}", change.new_start - 1)
        }
        _ if change.new_start == change.new_end => format!("line {}", change.new_start),
        _ => format!("lines {}–{}", change.new_start, change.new_end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_vcs::line_changes;

    // SPEC-0064 v3 AK-5.
    #[test]
    fn hits_neighbours_and_words() {
        // Modified line 2, deleted line 4, added line 6 (1-based, new text).
        let changes = line_changes("a\nb\nc\nd\ne\nf\n", "a\nB\nc\ne\nf\nx\n");
        assert_eq!(changes.len(), 3);
        assert_eq!(change_at(&changes, 1, false), Some(0));
        // The deletion lies between "c" (line 2) and "e" (line 3).
        assert_eq!(change_at(&changes, 2, false), Some(1));
        assert_eq!(change_at(&changes, 3, true), Some(1));
        assert_eq!(change_at(&changes, 3, false), None);
        assert_eq!(change_at(&changes, 5, true), Some(2));
        assert_eq!(neighbour(&changes, 0, false), None);
        assert_eq!(neighbour(&changes, 0, true), Some(1));
        assert_eq!(neighbour(&changes, 2, true), None);
        assert_eq!(from_caret(&changes, 0, true), Some(0));
        assert_eq!(from_caret(&changes, 1, true), Some(1));
        assert_eq!(from_caret(&changes, 4, false), Some(1));
        assert_eq!(from_caret(&changes, 1, false), None);
        assert_eq!(anchor_line(&changes[0]), 1);
        assert_eq!(anchor_line(&changes[1]), 2);
        assert_eq!(describe(&changes[0]), "line 2");
        assert_eq!(describe(&changes[1]), "the deleted lines after line 3");
        let two = line_changes("a\n", "x\ny\n");
        assert_eq!(describe(&two[0]), "lines 1–2");
        let long: String = (0..20).map(|i| format!("l{i}\n")).collect();
        let shown = shown_old_lines(long.trim_end(), &changes[0]);
        assert_eq!(shown.len(), OLD_LINES_SHOWN + 1);
        assert_eq!(shown[OLD_LINES_SHOWN], "… 8 more lines");
        assert!(shown_old_lines("", &changes[2]).is_empty());
    }
}
