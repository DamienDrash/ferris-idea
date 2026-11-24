//! The decisions behind the diff viewer's own actions (`SPEC-0084` v5 behaviours 17–22), free of
//! the UI toolkit: what a side of a viewer is and whether it may be written, where a step
//! between differences leads and when it goes on to another file, the next and previous file of
//! a list, the current pane and the rows of the settings popup. The rules follow the open
//! IntelliJ platform (`docs/legal/PROVENANCE.md`); the code is Ferris IDEA's own.

use std::path::{Path, PathBuf};

use ferris_idea_vcs::DiffRow;

/// One of the two panes of the side-by-side view (behaviour 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// The left pane.
    Left,
    /// The right pane.
    Right,
}

impl Pane {
    /// The pane opposite to this one.
    pub fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// `left` or `right`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// What a side of a viewer shows (behaviour 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    /// A file of the project: the open editor's text, else the file on disk.
    File(PathBuf),
    /// Text that is never written: a revision, a stash, a local history revision, the clipboard.
    Fixed(String),
}

/// The two sides of a viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sides {
    /// The left side.
    pub left: Side,
    /// The right side.
    pub right: Side,
}

impl Sides {
    /// The side shown in `pane`.
    pub fn of(&self, pane: Pane) -> &Side {
        match pane {
            Pane::Left => &self.left,
            Pane::Right => &self.right,
        }
    }
}

/// What is known about a file side before it is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TargetFacts {
    /// The window shows a remote project.
    pub remote: bool,
    /// The file exists.
    pub exists: bool,
    /// The file system marks the file read-only.
    pub file_read_only: bool,
    /// The file is open in a read-only editor (a library source, `SPEC-0149` v4).
    pub editor_read_only: bool,
}

impl TargetFacts {
    /// The facts of `file` on this machine; `editor_read_only` says whether its open editor is
    /// read-only.
    pub fn of(file: &Path, remote: bool, editor_read_only: bool) -> Self {
        let meta = std::fs::metadata(file)
            .ok()
            .filter(std::fs::Metadata::is_file);
        Self {
            remote,
            exists: meta.is_some(),
            file_read_only: meta.is_some_and(|m| m.permissions().readonly()),
            editor_read_only,
        }
    }
}

/// Why a side is not written (behaviour 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The side is fixed text.
    Fixed,
    /// The window shows a remote project.
    Remote,
    /// The file no longer exists.
    Missing,
    /// The file system marks the file read-only.
    ReadOnlyFile,
    /// The file's editor is read-only.
    ReadOnlyEditor,
}

impl Refusal {
    /// The status line's text for a side titled `title`.
    pub fn message(self, title: &str) -> String {
        match self {
            Self::Fixed => format!("{title} is not a file of the project: it cannot be changed"),
            Self::Remote => {
                format!("{title} belongs to a remote project: the diff viewer does not change it")
            }
            Self::Missing => format!("{title} no longer exists: nothing was changed"),
            Self::ReadOnlyFile => format!("{title} is read-only: nothing was changed"),
            Self::ReadOnlyEditor => {
                format!("{title} is read-only in the editor: nothing was changed")
            }
        }
    }
}

/// Why `side` is not written, or `None` when it may be (behaviour 17). `facts` are those of the
/// side's file and are not looked at for fixed text.
pub fn refusal(side: &Side, facts: &TargetFacts) -> Option<Refusal> {
    match side {
        Side::Fixed(_) => Some(Refusal::Fixed),
        Side::File(_) if facts.remote => Some(Refusal::Remote),
        Side::File(_) if !facts.exists => Some(Refusal::Missing),
        Side::File(_) if facts.file_read_only => Some(Refusal::ReadOnlyFile),
        Side::File(_) if facts.editor_read_only => Some(Refusal::ReadOnlyEditor),
        Side::File(_) => None,
    }
}

/// What taking says without a current difference.
pub const NO_DIFFERENCE: &str = "No difference is selected: nothing to take";
/// What taking says when the target's lines are already those of the source.
pub const ALREADY_EQUAL: &str = "The lines are already equal: nothing was changed";
/// What taking says when the sides changed since the viewer read them (behaviour 18).
pub const SIDES_CHANGED: &str =
    "The files changed since the diff was shown: nothing was written, the diff shows them now";
/// What Select Opposite Diff Pane says in the unified view (behaviour 19).
pub const ONE_PANE: &str = "The unified view has one pane";
/// What the file actions say in a viewer without a list of files (behaviour 20).
pub const NO_FILE_LIST: &str = "This diff has no list of files to go through";

/// What the file actions say at the end of the list (behaviour 20).
pub fn no_further_file(forward: bool) -> &'static str {
    if forward {
        "No next file to compare"
    } else {
        "No previous file to compare"
    }
}

/// What the first press of F7 (⇧F7) at the last (first) difference says (behaviour 21).
pub fn press_again(forward: bool, file: &str) -> String {
    if forward {
        format!("No further difference: press again to go to the next file, {file}")
    } else {
        format!("No earlier difference: press again to go to the previous file, {file}")
    }
}

/// The difference a step from `current` leads to among `count` differences, or `None` when the
/// step would leave the file: past the last or first difference, or with no difference at all
/// (behaviour 3).
pub fn within(count: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    match (current, forward) {
        _ if count == 0 => None,
        (None, _) => Some(0),
        (Some(i), true) if i + 1 < count => Some(i + 1),
        (Some(i), false) if i > 0 => Some((i - 1).min(count - 1)),
        _ => None,
    }
}

/// What a step that cannot move within the file does (behaviour 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// Nothing: there is no file in that direction.
    Stop,
    /// Say that another press goes to the other file, and remember the direction.
    Ask,
    /// Show the other file.
    Go,
}

/// What a step at the edge of a file does: `asked` is the direction of the press remembered
/// from the step before, `neighbour` whether the list has a file in the direction of this step.
pub fn edge(asked: Option<bool>, forward: bool, neighbour: bool) -> Edge {
    match (neighbour, asked == Some(forward)) {
        (false, _) => Edge::Stop,
        (true, true) => Edge::Go,
        (true, false) => Edge::Ask,
    }
}

/// The file after (before) `current` in `files`; `None` at the end of the list — the list does
/// not wrap — and when `current` is not in it (behaviour 20).
pub fn neighbour<'a>(files: &'a [String], current: &str, forward: bool) -> Option<&'a str> {
    let at = files.iter().position(|f| f == current)?;
    let next = if forward {
        at.checked_add(1)?
    } else {
        at.checked_sub(1)?
    };
    files.get(next).map(String::as_str)
}

/// The 1-based line `pane` shows at row `row`, or the nearest line above where the pane has no
/// line there; `None` above the pane's first line (behaviour 19).
pub fn pane_line(rows: &[DiffRow], row: usize, pane: Pane) -> Option<usize> {
    rows.iter()
        .take(row.saturating_add(1))
        .rev()
        .find_map(|r| match pane {
            Pane::Left => r.left.as_ref().map(|l| l.0),
            Pane::Right => r.right.as_ref().map(|l| l.0),
        })
}

/// What the status line says about the current pane (behaviour 19).
pub fn pane_status(pane: Pane, title: &str, line: Option<usize>) -> String {
    let side = match pane {
        Pane::Left => "Left",
        Pane::Right => "Right",
    };
    match line {
        Some(line) => format!("{side} pane: {title}, line {line}"),
        None => format!("{side} pane: {title}"),
    }
}

/// The title of the settings popup (behaviour 22).
pub const SETTINGS_TITLE: &str = "Diff Settings";

/// The rows of the settings popup: the two views, the current one marked (behaviour 22).
pub fn settings_rows(unified: bool) -> Vec<String> {
    let mark = |on: bool| if on { "✓" } else { " " };
    vec![
        format!("{} Side-by-side viewer", mark(!unified)),
        format!("{} Unified viewer", mark(unified)),
    ]
}

/// Whether the settings popup's row `index` is the unified view; `None` for no row.
pub fn setting_of_row(index: usize) -> Option<bool> {
    match index {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0084 AK-12 (v5).
    #[test]
    fn steps_stop_at_the_ends_or_ask_for_a_second_press() {
        // Within a file of three differences: in from nowhere, on, and back.
        assert_eq!(within(3, None, true), Some(0));
        assert_eq!(within(3, None, false), Some(0));
        assert_eq!(within(3, Some(0), true), Some(1));
        assert_eq!(within(3, Some(1), true), Some(2));
        assert_eq!(within(3, Some(2), false), Some(1));
        // The first and the last difference: no step past them.
        assert_eq!(within(3, Some(2), true), None);
        assert_eq!(within(3, Some(0), false), None);
        assert_eq!(within(1, Some(0), true), None);
        assert_eq!(within(1, Some(0), false), None);
        // A diff without differences has nowhere to go.
        assert_eq!(within(0, None, true), None);
        assert_eq!(within(0, None, false), None);
        // A current difference past the end (the rows shrank) steps back into the list.
        assert_eq!(within(2, Some(5), false), Some(1));

        // At the edge without a file in that direction: stop, whatever was pressed before.
        assert_eq!(edge(None, true, false), Edge::Stop);
        assert_eq!(edge(Some(true), true, false), Edge::Stop);
        // With one: the first press asks, the second in the same direction goes.
        assert_eq!(edge(None, true, true), Edge::Ask);
        assert_eq!(edge(Some(true), true, true), Edge::Go);
        assert_eq!(edge(None, false, true), Edge::Ask);
        assert_eq!(edge(Some(false), false, true), Edge::Go);
        // A press in the other direction is a first press again.
        assert_eq!(edge(Some(true), false, true), Edge::Ask);
        assert_eq!(edge(Some(false), true, true), Edge::Ask);
        assert!(press_again(true, "b.rs").contains("next file, b.rs"));
        assert!(press_again(false, "a.rs").contains("previous file, a.rs"));
    }

    /// SPEC-0084 AK-12 (v5).
    #[test]
    fn the_neighbour_file_does_not_wrap() {
        let files: Vec<String> = ["a.rs", "src/b.rs", "c.txt"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(neighbour(&files, "a.rs", true), Some("src/b.rs"));
        assert_eq!(neighbour(&files, "src/b.rs", true), Some("c.txt"));
        assert_eq!(neighbour(&files, "c.txt", true), None);
        assert_eq!(neighbour(&files, "c.txt", false), Some("src/b.rs"));
        assert_eq!(neighbour(&files, "a.rs", false), None);
        // A file that left the list, one file, no file.
        assert_eq!(neighbour(&files, "gone.rs", true), None);
        assert_eq!(neighbour(&files[..1], "a.rs", true), None);
        assert_eq!(neighbour(&files[..1], "a.rs", false), None);
        assert_eq!(neighbour(&[], "a.rs", true), None);
        assert_ne!(no_further_file(true), no_further_file(false));
    }

    /// SPEC-0084 AK-14 (v5).
    #[test]
    fn only_a_writable_file_side_is_written() {
        let file = Side::File(PathBuf::from("/project/a.rs"));
        let fine = TargetFacts {
            exists: true,
            ..TargetFacts::default()
        };
        assert_eq!(refusal(&file, &fine), None);
        // Fixed text is never written, whatever the facts say.
        assert_eq!(
            refusal(&Side::Fixed("text".into()), &fine),
            Some(Refusal::Fixed)
        );
        for (facts, why) in [
            (
                TargetFacts {
                    remote: true,
                    ..fine
                },
                Refusal::Remote,
            ),
            (TargetFacts::default(), Refusal::Missing),
            (
                TargetFacts {
                    file_read_only: true,
                    ..fine
                },
                Refusal::ReadOnlyFile,
            ),
            (
                TargetFacts {
                    editor_read_only: true,
                    ..fine
                },
                Refusal::ReadOnlyEditor,
            ),
            // A remote project comes first: nothing of this machine is looked at.
            (
                TargetFacts {
                    remote: true,
                    exists: false,
                    file_read_only: true,
                    editor_read_only: true,
                },
                Refusal::Remote,
            ),
        ] {
            assert_eq!(refusal(&file, &facts), Some(why), "{facts:?}");
            assert!(why.message("a.rs").starts_with("a.rs "), "{why:?}");
        }
        // The facts of a real file: writable, read-only, gone.
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-diff-facts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, "x\n").unwrap();
        assert_eq!(TargetFacts::of(&path, false, false), fine);
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms).unwrap();
        assert_eq!(
            refusal(
                &Side::File(path.clone()),
                &TargetFacts::of(&path, false, false)
            ),
            Some(Refusal::ReadOnlyFile)
        );
        // A folder is not a file to write.
        assert_eq!(
            refusal(
                &Side::File(dir.clone()),
                &TargetFacts::of(&dir, false, false)
            ),
            Some(Refusal::Missing)
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            refusal(
                &Side::File(path.clone()),
                &TargetFacts::of(&path, false, false)
            ),
            Some(Refusal::Missing)
        );
    }

    /// SPEC-0084 v5 behaviours 19 and 22.
    #[test]
    fn the_pane_names_its_line_and_the_popup_its_views() {
        let rows = ferris_idea_vcs::side_by_side("a\nb\n", "a\nnew\nb\n");
        // Row 1 is the added line: the left pane has no line there and names the one above.
        assert_eq!(pane_line(&rows, 1, Pane::Right), Some(2));
        assert_eq!(pane_line(&rows, 1, Pane::Left), Some(1));
        assert_eq!(pane_line(&rows, 2, Pane::Left), Some(2));
        let added = ferris_idea_vcs::side_by_side("", "x\n");
        assert_eq!(pane_line(&added, 0, Pane::Left), None);
        assert_eq!(
            pane_status(Pane::Left, "HEAD", Some(1)),
            "Left pane: HEAD, line 1"
        );
        assert_eq!(pane_status(Pane::Right, "a.rs", None), "Right pane: a.rs");
        assert_eq!(Pane::Left.other(), Pane::Right);
        assert_eq!(Pane::Right.other().name(), "left");
        let sides = Sides {
            left: Side::Fixed("old".into()),
            right: Side::File(PathBuf::from("a.rs")),
        };
        assert_eq!(sides.of(Pane::Left), &Side::Fixed("old".into()));
        assert_eq!(sides.of(Pane::Right), &Side::File(PathBuf::from("a.rs")));

        assert_eq!(
            settings_rows(false),
            ["✓ Side-by-side viewer", "  Unified viewer"]
        );
        assert_eq!(
            settings_rows(true),
            ["  Side-by-side viewer", "✓ Unified viewer"]
        );
        assert_eq!(setting_of_row(0), Some(false));
        assert_eq!(setting_of_row(1), Some(true));
        assert_eq!(setting_of_row(2), None);
    }
}
