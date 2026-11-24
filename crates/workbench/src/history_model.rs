//! Rows of the local history view (`SPEC-0085`), free of the UI toolkit.

use ferris_idea_core::Revision;
use ferris_idea_core::RevisionKind;

/// How long ago `time` was, seen at `now` (both in milliseconds since the epoch).
pub fn age(now: u64, time: u64) -> String {
    let secs = now.saturating_sub(time) / 1000;
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => match secs / 86_400 {
            1 => "yesterday".to_owned(),
            d => format!("{d} days ago"),
        },
    }
}

/// What a revision's change was (`SPEC-0085` v2 behaviour 8): its label (a refactoring's name),
/// "Revert" for a restore, "Deleted" for a deletion, else "Change".
pub fn what(revision: &Revision) -> String {
    match (revision.kind, revision.deleted, &revision.label) {
        (_, _, Some(l)) => l.clone(),
        (RevisionKind::Restore, _, None) => "Revert".to_owned(),
        (_, true, None) => "Deleted".to_owned(),
        _ => "Change".to_owned(),
    }
}

/// `time` (milliseconds since the epoch) as date and time in `zone`: `2026-11-15 13:56`.
pub fn stamp(time: u64, zone: &jiff::tz::TimeZone) -> String {
    jiff::Timestamp::from_millisecond(time as i64)
        .map(|t| {
            t.to_zoned(zone.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The text of a revision's row: what the change was and when (`SPEC-0085` v2 behaviour 8).
pub fn label(revision: &Revision, zone: &jiff::tz::TimeZone) -> String {
    format!("{} · {}", what(revision), stamp(revision.time, zone))
}

/// The state before entry `i` of `contents` (newest first; `SPEC-0085` v2 behaviour 9): the next
/// older one, for the oldest entry its own; a deletion is an empty text.
pub fn before_of(contents: &[Option<String>], i: usize) -> String {
    let at = if i + 1 < contents.len() { i + 1 } else { i };
    contents.get(at).cloned().flatten().unwrap_or_default()
}

/// The entries whose state before contains `text`, ignoring case (behaviour 10); all for an empty
/// text.
pub fn matching(contents: &[Option<String>], text: &str) -> Vec<usize> {
    let text = text.to_lowercase();
    (0..contents.len())
        .filter(|&i| text.is_empty() || before_of(contents, i).to_lowercase().contains(&text))
        .collect()
}

/// A unified patch of `path` from `before` to `current` (behaviour 11); empty without differences.
pub fn patch(path: &str, before: &str, current: &str) -> String {
    let hunks = ferris_idea_vcs::unified_hunks(before, current);
    if hunks.is_empty() {
        return String::new();
    }
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    for h in hunks {
        out.push_str(&h);
    }
    out
}

/// The revisions shown: those with content of the file (workspace labels are not revisions of it),
/// newest first. A label put on the file names the revision right before it, e.g. "Rename runde to
/// aufrunden" (`SPEC-0193` v2 behaviour 8, D46).
pub fn shown(revisions: Vec<Revision>) -> Vec<Revision> {
    let mut out = Vec::new();
    let mut pending: Option<String> = None;
    for mut r in revisions {
        if r.workspace_label {
            continue;
        }
        if r.kind == RevisionKind::Label {
            // Newest first: the label comes before the revision it names.
            pending = r.label.take().or(pending);
            continue;
        }
        if let Some(label) = pending.take()
            && r.label.is_none()
        {
            r.label = Some(label);
        }
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rev(time: u64, kind: RevisionKind, deleted: bool, label: Option<&str>) -> Revision {
        Revision {
            id: 0,
            time,
            size: 1,
            label: label.map(str::to_owned),
            kind,
            deleted,
            workspace_label: false,
        }
    }

    #[test]
    fn ages_read_naturally() {
        let now = 10 * 86_400_000;
        assert_eq!(age(now, now - 5_000), "just now");
        assert_eq!(age(now, now - 5 * 60_000), "5 min ago");
        assert_eq!(age(now, now - 3 * 3_600_000), "3 h ago");
        assert_eq!(age(now, now - 86_400_000), "yesterday");
        assert_eq!(age(now, now - 4 * 86_400_000), "4 days ago");
        assert_eq!(age(now, now + 1), "just now");
    }

    /// SPEC-0085 v2 AK-4: a row names the change and its local date and time.
    #[test]
    fn rows_name_the_change_and_its_time() {
        let utc = jiff::tz::TimeZone::UTC;
        // 2025-11-15 13:56:20 UTC.
        let t = 1_763_214_980_000;
        assert_eq!(stamp(t, &utc), "2025-11-15 13:56");
        assert_eq!(
            label(&rev(t, RevisionKind::Change, false, None), &utc),
            "Change · 2025-11-15 13:56"
        );
        let plus_one = jiff::tz::TimeZone::fixed(jiff::tz::offset(1));
        assert_eq!(stamp(t, &plus_one), "2025-11-15 14:56", "in local time");
    }

    /// SPEC-0085 v2 AK-4: an entry shows the state before it; the oldest its own; a deletion is
    /// empty.
    #[test]
    fn entries_show_the_state_before() {
        let contents = vec![
            Some("a\nb\nc\n".to_owned()),
            Some("a\nb\n".to_owned()),
            None,
            Some("a\n".to_owned()),
        ];
        assert_eq!(
            before_of(&contents, 0),
            "a\nb\n",
            "the newest shows the last change"
        );
        assert_eq!(
            before_of(&contents, 1),
            "",
            "before it the file was deleted"
        );
        assert_eq!(before_of(&contents, 2), "a\n");
        assert_eq!(before_of(&contents, 3), "a\n", "the oldest shows its own");
        assert_eq!(before_of(&contents, 9), "");
    }

    /// SPEC-0085 v2 AK-4: the search looks at the states before; the patch has its header.
    #[test]
    fn search_and_patch() {
        let contents = vec![
            Some("Total = 3\n".to_owned()),
            Some("total = 2\n".to_owned()),
            Some("sum = 1\n".to_owned()),
        ];
        assert_eq!(matching(&contents, ""), [0, 1, 2]);
        assert_eq!(
            matching(&contents, "TOTAL"),
            [0],
            "entry 0 shows `total = 2`"
        );
        assert_eq!(matching(&contents, "sum"), [1, 2]);
        assert!(matching(&contents, "nothing").is_empty());
        let p = patch("src/Inventory.java", "a\n", "a\nb\n");
        assert!(
            p.starts_with("--- a/src/Inventory.java\n+++ b/src/Inventory.java\n@@"),
            "{p}"
        );
        assert!(p.contains("+b\n"), "{p}");
        assert_eq!(patch("x", "a\n", "a\n"), "");
    }

    #[test]
    fn labels_and_filter() {
        let now = 3_600_000;
        assert_eq!(what(&rev(now, RevisionKind::Change, false, None)), "Change");
        assert_eq!(what(&rev(0, RevisionKind::Restore, false, None)), "Revert");
        assert_eq!(what(&rev(now, RevisionKind::Change, true, None)), "Deleted");
        let mut ws = rev(now, RevisionKind::Label, false, Some("Release"));
        ws.workspace_label = true;
        let kept = shown(vec![rev(1, RevisionKind::Change, false, None), ws]);
        assert_eq!(kept.len(), 1);
    }

    /// SPEC-0193 v2 behaviour 8 (hand test 123, D46): a label on the file names its revision.
    #[test]
    fn refactoring_labels_name_their_revision() {
        let now = 3_600_000;
        let newest_first = vec![
            rev(now, RevisionKind::Change, false, None),
            rev(
                now - 1,
                RevisionKind::Label,
                false,
                Some("Rename runde to aufrunden"),
            ),
            rev(now - 1, RevisionKind::Change, false, None),
            rev(now - 2, RevisionKind::Change, false, None),
        ];
        let rows: Vec<String> = shown(newest_first).iter().map(what).collect();
        assert_eq!(rows, ["Change", "Rename runde to aufrunden", "Change"]);
    }

    /// SPEC-0193 v2 AK-6 (D46): with the real local history, a file a refactoring wrote shows the
    /// refactoring's name, and a file changed only in its editor gets its saved state labelled.
    #[test]
    fn refactorings_show_in_a_files_history() {
        use ferris_idea_languages::undo_group::{record_history, record_history_before};
        let root =
            std::env::temp_dir().join(format!("ferris-idea-hist-label-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let root = root.canonicalize().unwrap();
        let (geld, main) = (root.join("src/geld.rs"), root.join("src/main.rs"));
        std::fs::write(&geld, "fn runde() {}\n").unwrap();
        std::fs::write(&main, "fn main() { runde(); }\n").unwrap();
        let name = "Rename runde to aufrunden";
        record_history(&root, &[geld.as_path()], None);
        record_history_before(&root, &[main.as_path()], name);
        std::fs::write(&geld, "fn aufrunden() {}\n").unwrap();
        record_history(&root, &[geld.as_path()], Some(name));
        let history = ferris_idea_core::LocalHistory::open(&root).unwrap();
        let what = |rel: &str| -> Vec<String> {
            shown(history.revisions(rel))
                .iter()
                .map(super::what)
                .collect()
        };
        assert_eq!(what("src/geld.rs"), [name, "Change"]);
        assert_eq!(what("src/main.rs"), [format!("Before {name}")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
