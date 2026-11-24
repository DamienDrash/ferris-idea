//! The question when a file opened from an SSH host changed there since it was read
//! (`SPEC-0121` v7 behaviour 4a): what each side changed against the base, what Overwrite would
//! discard, and the answers on offer. Toolkit-free, so the texts are unit-tested.

use crate::dialogs::DrawnQuestion;

/// One answer to the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictAnswer {
    /// Three-way merge with the base in the middle; Apply uploads the result.
    Merge,
    /// Diff of the copy against the host's version; nothing is sent.
    Compare,
    /// Upload the copy over the host's version just read (still checked against its stamp).
    Overwrite,
    /// Send nothing; both versions stay.
    Cancel,
}

/// Whether `bytes` can be merged as text: no NUL byte and valid UTF-8.
pub fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// Lines of `new` that differ from `old` (added, changed or removed rows of the side-by-side
/// diff).
pub fn changed_lines(old: &str, new: &str) -> usize {
    ferris_idea_vcs::side_by_side(old, new)
        .iter()
        .filter(|r| r.kind.is_some())
        .count()
}

/// Regions where the copy and the host changed the same lines differently.
pub fn conflicting_regions(base: &str, ours: &str, theirs: &str) -> usize {
    ferris_idea_vcs::Merge3::new(base, ours, theirs)
        .chunks
        .iter()
        .filter(|c| c.kind == ferris_idea_vcs::ChunkKind::Conflict)
        .count()
}

fn lines(n: usize) -> String {
    format!("{n} line{}", if n == 1 { "" } else { "s" })
}

/// The question for file `name` at `label` (`host:path`): `base` is the version read last,
/// `ours` the copy being saved, `host` the host's version now (`None` when the host deleted the
/// file). Returns the question and the answer of each button in order.
pub fn question(
    name: &str,
    label: &str,
    base: &[u8],
    ours: &[u8],
    host: Option<&[u8]>,
) -> (DrawnQuestion, Vec<ConflictAnswer>) {
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let (base_t, ours_t) = (text(base), text(ours));
    let mut detail = vec![label.to_owned()];
    let mergeable = match host {
        None => {
            detail.push(format!(
                "Your copy: {} changed since you opened it. The host deleted the file.",
                lines(changed_lines(&base_t, &ours_t))
            ));
            detail.push("Overwrite writes your copy there again.".into());
            false
        }
        Some(host) => {
            let host_t = text(host);
            let theirs = changed_lines(&base_t, &host_t);
            detail.push(format!(
                "Your copy: {} changed. On the host: {} changed since you opened it.",
                lines(changed_lines(&base_t, &ours_t)),
                lines(theirs)
            ));
            let text_files = is_text(base) && is_text(ours) && is_text(host);
            if text_files {
                detail.push(match conflicting_regions(&base_t, &ours_t, &host_t) {
                    0 => "Merge takes both sides' changes; none of them conflict.".into(),
                    1 => "Merge takes both sides' changes; 1 region conflicts and needs your choice."
                        .into(),
                    n => format!(
                        "Merge takes both sides' changes; {n} regions conflict and need your choice."
                    ),
                });
            } else {
                detail.push("The file is not text: it can be compared, not merged.".into());
            }
            detail.push(format!(
                "Overwrite discards the host's change: {}.",
                lines(theirs)
            ));
            text_files
        }
    };
    let mut answers = Vec::new();
    if mergeable {
        answers.push(ConflictAnswer::Merge);
    }
    answers.extend([
        ConflictAnswer::Compare,
        ConflictAnswer::Overwrite,
        ConflictAnswer::Cancel,
    ]);
    let buttons = answers
        .iter()
        .map(|a| {
            match a {
                ConflictAnswer::Merge => "Merge…",
                ConflictAnswer::Compare => "Compare",
                ConflictAnswer::Overwrite => "Overwrite Host Version",
                ConflictAnswer::Cancel => "Cancel",
            }
            .to_owned()
        })
        .collect();
    let q = DrawnQuestion {
        message: format!("{name} changed on the host since you opened it"),
        detail: detail.join("\n"),
        buttons,
        // Return merges (or compares); Escape cancels; Overwrite is never a key's answer.
        default: 0,
        cancel: Some(answers.len() - 1),
    };
    (q, answers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0121 v7 AK-10: the question names both sides' changes and what Overwrite discards;
    /// Return merges, Escape cancels, no key overwrites.
    #[test]
    fn the_question_names_what_each_answer_does() {
        let (q, answers) = question(
            "notes.txt",
            "dev:/srv/notes.txt",
            b"a\nb\nc\n",
            b"a\nmine\nc\n",
            Some(b"a\nb\nc\nhost\n"),
        );
        assert_eq!(
            q.message,
            "notes.txt changed on the host since you opened it"
        );
        assert!(q.detail.starts_with("dev:/srv/notes.txt\n"), "{}", q.detail);
        assert!(
            q.detail
                .contains("Your copy: 1 line changed. On the host: 1 line changed"),
            "{}",
            q.detail
        );
        assert!(q.detail.contains("none of them conflict"), "{}", q.detail);
        assert!(
            q.detail
                .contains("Overwrite discards the host's change: 1 line.")
        );
        assert_eq!(
            answers,
            [
                ConflictAnswer::Merge,
                ConflictAnswer::Compare,
                ConflictAnswer::Overwrite,
                ConflictAnswer::Cancel
            ]
        );
        assert_eq!(
            q.buttons,
            ["Merge…", "Compare", "Overwrite Host Version", "Cancel"]
        );
        assert_eq!(
            answers[q.answer_for_key("enter").unwrap()],
            ConflictAnswer::Merge
        );
        assert_eq!(
            answers[q.answer_for_key("escape").unwrap()],
            ConflictAnswer::Cancel
        );
    }

    #[test]
    fn conflicts_are_counted_and_binary_or_deleted_files_are_not_merged() {
        let (q, _) = question("a", "h:/a", b"x\n", b"mine\n", Some(b"theirs\n"));
        assert!(q.detail.contains("1 region conflicts"), "{}", q.detail);

        let (q, answers) = question("a", "h:/a", b"x\0", b"y\0", Some(b"z\0"));
        assert_eq!(answers[0], ConflictAnswer::Compare);
        assert!(q.detail.contains("not text"));
        assert_eq!(
            answers[q.answer_for_key("enter").unwrap()],
            ConflictAnswer::Compare
        );

        let (q, answers) = question("a", "h:/a", b"x\n", b"y\n", None);
        assert!(
            q.detail.contains("The host deleted the file"),
            "{}",
            q.detail
        );
        assert_eq!(
            answers,
            [
                ConflictAnswer::Compare,
                ConflictAnswer::Overwrite,
                ConflictAnswer::Cancel
            ]
        );
    }

    #[test]
    fn lines_count_changed_rows() {
        assert_eq!(changed_lines("a\nb\n", "a\nb\n"), 0);
        assert_eq!(changed_lines("a\nb\n", "a\nc\nd\n"), 2);
        assert!(is_text(b"hello\n") && !is_text(b"a\0b") && !is_text(&[0xff, 0xfe]));
    }
}
