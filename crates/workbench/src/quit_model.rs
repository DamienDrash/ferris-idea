//! The question asked before quitting with unsaved files or running work (`SPEC-0261`),
//! toolkit-free.

use std::path::{Path, PathBuf};

/// Entries of a list in the question before "and n more" (behaviour 2).
const NAMED: usize = 10;

/// What happens before quitting, the same for ⌘Q, the menu, `quit_ide` and `ferris-idea quit`
/// (`SPEC-0261` v3 behaviour 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitChoice {
    /// Quit only if nothing is unsaved or running now.
    IfNothingPending,
    /// Save every editor, then quit; nothing quits if a file stays unsaved.
    SaveAll,
    /// Quit without saving; running programs end.
    Discard,
}

/// What the alert shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuitQuestion {
    /// Headline.
    pub message: String,
    /// The unsaved files, one per line, then the running work.
    pub detail: String,
    /// Buttons in order with what each does; the first is the default, `None` cancels.
    pub answers: Vec<(&'static str, Option<QuitChoice>)>,
}

/// The question for the `unsaved` files (named as [`names`] names them) and the `running` work;
/// `None` when neither is there, so quitting asks nothing (behaviour 1).
pub fn question(unsaved: &[String], running: &[String]) -> Option<QuitQuestion> {
    if unsaved.is_empty() && running.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !unsaved.is_empty() {
        let mut names = unsaved.to_vec();
        names.sort();
        parts.push(cut(&names));
    }
    if !running.is_empty() {
        parts.push(format!(
            "Still running, ended when Ferris IDEA quits:\n{}",
            cut(running)
        ));
    }
    let (message, answers) = if unsaved.is_empty() {
        (
            "Stop running processes and quit?",
            vec![
                ("Stop and Quit", Some(QuitChoice::Discard)),
                ("Cancel", None),
            ],
        )
    } else {
        (
            "Save changes before quitting?",
            vec![
                ("Save All and Quit", Some(QuitChoice::SaveAll)),
                ("Quit Without Saving", Some(QuitChoice::Discard)),
                ("Cancel", None),
            ],
        )
    };
    Some(QuitQuestion {
        message: message.into(),
        detail: parts.join("\n\n"),
        answers,
    })
}

/// The question before quitting with nothing pending, asked only while the user's setting
/// "Confirm before quitting" is on (`SPEC-0337` behaviour 2). Quit quits only if still nothing is
/// pending when it is chosen.
pub fn confirm_question() -> QuitQuestion {
    QuitQuestion {
        message: "Quit Ferris IDEA?".into(),
        detail: String::new(),
        answers: vec![
            ("Quit", Some(QuitChoice::IfNothingPending)),
            ("Cancel", None),
        ],
    }
}

/// At most [`NAMED`] lines, then "and n more".
pub(crate) fn cut(lines: &[String]) -> String {
    let mut out = lines
        .iter()
        .take(NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if lines.len() > NAMED {
        out.push_str(&format!("\nand {} more", lines.len() - NAMED));
    }
    out
}

/// The `unsaved` files named as the question names them, sorted, all of them (`SPEC-0261` v2
/// behaviour 6: what `quit_ide` reports).
pub fn names(unsaved: &[PathBuf], roots: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = unsaved.iter().map(|p| shown(p, roots)).collect();
    names.sort();
    names
}

/// What keeps Ferris IDEA open after the save step of quitting (`SPEC-0261` v3 behaviour 15): the
/// files whose editors are still modified and the files the registry of open documents still marks
/// modified, each once, named as [`names`] names them. Empty: everything is saved, quit.
pub fn left_after_save(
    editors: &[PathBuf],
    registry: &[PathBuf],
    roots: &[PathBuf],
) -> Vec<String> {
    let mut left: Vec<PathBuf> = editors.iter().chain(registry).cloned().collect();
    left.sort();
    left.dedup();
    names(&left, roots)
}

/// `path` relative to the deepest root holding it, with that project's folder name in front when
/// several projects are open.
fn shown(path: &Path, roots: &[PathBuf]) -> String {
    let root = roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.components().count());
    match root.and_then(|r| Some((r, path.strip_prefix(r).ok()?))) {
        Some((root, rel)) if roots.len() > 1 => {
            let project = root.file_name().unwrap_or_default().to_string_lossy();
            format!("{project}/{}", rel.display())
        }
        Some((_, rel)) => rel.display().to_string(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// The save step names every file left unsaved once, whether an editor or only the registry
    /// knows it, and nothing when all is saved (`SPEC-0261` v3 behaviour 15).
    #[test]
    fn the_save_step_names_each_file_left_unsaved_once() {
        let roots = [PathBuf::from("/work/app")];
        assert!(left_after_save(&[], &[], &roots).is_empty());
        let left = left_after_save(
            &[PathBuf::from("/work/app/b.txt")],
            &[
                PathBuf::from("/work/app/b.txt"),
                PathBuf::from("/work/app/a.txt"),
            ],
            &roots,
        );
        assert_eq!(left, strings(&["a.txt", "b.txt"]));
    }

    #[test]
    fn nothing_pending_asks_nothing() {
        assert_eq!(question(&[], &[]), None);
    }

    /// SPEC-0337 AK-3: the user's own question with nothing pending; Quit checks again.
    #[test]
    fn confirming_a_quit_with_nothing_pending() {
        let q = confirm_question();
        assert_eq!(q.message, "Quit Ferris IDEA?");
        assert!(q.detail.is_empty());
        assert_eq!(
            q.answers,
            [
                ("Quit", Some(QuitChoice::IfNothingPending)),
                ("Cancel", None)
            ]
        );
    }

    #[test]
    fn names_the_files_relative_to_their_project() {
        let roots = [PathBuf::from("/work/app")];
        let unsaved = names(
            &[
                PathBuf::from("/work/app/src/main.rs"),
                PathBuf::from("/tmp/notes.txt"),
            ],
            &roots,
        );
        let q = question(&unsaved, &[]).unwrap();
        assert_eq!(q.message, "Save changes before quitting?");
        assert_eq!(q.detail, "/tmp/notes.txt\nsrc/main.rs");
        let labels: Vec<_> = q.answers.iter().map(|a| a.0).collect();
        assert_eq!(
            labels,
            ["Save All and Quit", "Quit Without Saving", "Cancel"]
        );
        assert_eq!(
            q.answers.iter().map(|a| a.1).collect::<Vec<_>>(),
            [Some(QuitChoice::SaveAll), Some(QuitChoice::Discard), None]
        );

        let roots = [PathBuf::from("/work/app"), PathBuf::from("/work/lib")];
        let unsaved = names(&[PathBuf::from("/work/lib/a.rs")], &roots);
        assert_eq!(question(&unsaved, &[]).unwrap().detail, "lib/a.rs");
    }

    #[test]
    fn running_work_is_named_and_asked_about_alone() {
        // D51/D58 of hand-test round 5: a running program alone is asked about too.
        let running = strings(&["ht-prof: run configuration \"schlaf\""]);
        let q = question(&[], &running).unwrap();
        assert_eq!(q.message, "Stop running processes and quit?");
        assert_eq!(
            q.detail,
            "Still running, ended when Ferris IDEA quits:\nht-prof: run configuration \"schlaf\""
        );
        assert_eq!(
            q.answers,
            [
                ("Stop and Quit", Some(QuitChoice::Discard)),
                ("Cancel", None)
            ]
        );
        // With unsaved files too: the files first, the three answers.
        let q = question(&strings(&["woerter.txt"]), &running).unwrap();
        assert_eq!(q.message, "Save changes before quitting?");
        assert!(q.detail.starts_with("woerter.txt\n\nStill running"));
        assert_eq!(q.answers.len(), 3);
    }

    #[test]
    fn names_for_mcp_are_all_listed() {
        let roots = [PathBuf::from("/w/app"), PathBuf::from("/w/lib")];
        let unsaved: Vec<PathBuf> = (0..12)
            .map(|i| PathBuf::from(format!("/w/app/f{i:02}")))
            .chain([PathBuf::from("/w/lib/a.rs")])
            .collect();
        let n = names(&unsaved, &roots);
        assert_eq!(n.len(), 13);
        assert_eq!(n[0], "app/f00");
        assert_eq!(n[12], "lib/a.rs");
        assert!(names(&[], &roots).is_empty());
    }

    #[test]
    fn long_lists_are_cut_after_ten() {
        let roots = [PathBuf::from("/p")];
        let unsaved: Vec<PathBuf> = (0..13)
            .map(|i| PathBuf::from(format!("/p/f{i:02}")))
            .collect();
        let q = question(&names(&unsaved, &roots), &[]).unwrap();
        let lines: Vec<&str> = q.detail.lines().collect();
        assert_eq!(lines.len(), 11);
        assert_eq!(lines[0], "f00");
        assert_eq!(lines[10], "and 3 more");
    }
}
