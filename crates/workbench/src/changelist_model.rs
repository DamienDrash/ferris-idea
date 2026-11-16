//! The changelist actions of the Commit tool window without toolkit (`SPEC-0335` behaviours
//! 8–12): what an action acts on, what its strip holds and what the window says.

use idecraft_core::changelists::{CHANGELISTS_FILE, Changelists, Deleted, Moved, Unreadable};

/// A changelist action (`SPEC-0335` behaviours 8–12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOp {
    /// New Changelist…
    New,
    /// Edit Changelist…
    Edit,
    /// Set Active Changelist.
    SetActive,
    /// Delete Changelist.
    Delete,
    /// Move to Another Changelist…
    Move,
}

impl ListOp {
    /// The five actions with their titles, in the order of the window's menu.
    pub const ALL: [(ListOp, &'static str); 5] = [
        (ListOp::New, "New Changelist…"),
        (ListOp::Edit, "Edit Changelist…"),
        (ListOp::SetActive, "Set Active Changelist"),
        (ListOp::Delete, "Delete Changelist"),
        (ListOp::Move, "Move to Another Changelist…"),
    ];
}

/// The strip an action shows in the Commit tool window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListStrip {
    /// New Changelist…: a name and "Set active".
    New {
        /// "Set active" is ticked.
        set_active: bool,
    },
    /// Edit Changelist…: the name of `list` and "Set active".
    Edit {
        /// The list being edited.
        list: String,
        /// "Set active" is ticked.
        set_active: bool,
        /// The list is the active one: the option is ticked and fixed.
        fixed: bool,
    },
    /// Delete Changelist: a question, and the list that becomes active when one must be chosen.
    Delete {
        /// The list being deleted.
        list: String,
        /// How many files it holds.
        files: usize,
        /// The lists one of which becomes active; empty when there is nothing to choose.
        choose: Vec<String>,
        /// The chosen one of `choose`.
        selected: usize,
    },
    /// Move to Another Changelist…: a name, the lists offered and "Set active".
    Move {
        /// The files being moved.
        files: Vec<String>,
        /// The lists offered.
        offered: Vec<String>,
        /// "Set active" is ticked.
        set_active: bool,
    },
}

impl ListStrip {
    /// Whether the strip has a name field.
    pub fn has_name(&self) -> bool {
        !matches!(self, ListStrip::Delete { .. })
    }

    /// The rows to choose from: the lists a move offers, or the lists one of which becomes
    /// active.
    pub fn choices(&self) -> &[String] {
        match self {
            ListStrip::Delete { choose, .. } => choose,
            ListStrip::Move { offered, .. } => offered,
            _ => &[],
        }
    }

    /// The strip's "Set active" option: its state and whether it can be changed; `None` for a
    /// strip without it.
    pub fn set_active(&self) -> Option<(bool, bool)> {
        match self {
            ListStrip::New { set_active } | ListStrip::Move { set_active, .. } => {
                Some((*set_active, true))
            }
            ListStrip::Edit {
                set_active, fixed, ..
            } => Some((*set_active, !fixed)),
            ListStrip::Delete { .. } => None,
        }
    }

    /// Flips "Set active" where it can be changed.
    pub fn toggle_set_active(&mut self) {
        match self {
            ListStrip::New { set_active } | ListStrip::Move { set_active, .. } => {
                *set_active = !*set_active;
            }
            ListStrip::Edit {
                set_active,
                fixed: false,
                ..
            } => *set_active = !*set_active,
            _ => {}
        }
    }

    /// What the strip asks.
    pub fn title(&self) -> String {
        match self {
            ListStrip::New { .. } => "New changelist".into(),
            ListStrip::Edit { list, .. } => format!("Edit changelist '{list}'"),
            ListStrip::Delete {
                list,
                files,
                choose,
                ..
            } => {
                let none = *files == 0;
                let files = count(*files);
                if choose.is_empty() {
                    format!(
                        "Delete changelist '{list}'? Its {files} will be moved to the active \
                         changelist."
                    )
                } else if none {
                    format!(
                        "Delete the active changelist '{list}'? Select the changelist to make \
                         active:"
                    )
                } else {
                    format!(
                        "Delete the active changelist '{list}'? Select the changelist its \
                         {files} move to; it becomes active:"
                    )
                }
            }
            ListStrip::Move { files, .. } => {
                format!("Move {} to the changelist:", count(files.len()))
            }
        }
    }

    /// The label of the confirming button.
    pub fn confirm_label(&self) -> &'static str {
        match self {
            ListStrip::New { .. } => "Create",
            ListStrip::Edit { .. } => "OK",
            ListStrip::Delete { .. } => "Delete",
            ListStrip::Move { .. } => "Move",
        }
    }
}

fn count(files: usize) -> String {
    format!("{files} file{}", if files == 1 { "" } else { "s" })
}

/// The list Edit and Set Active act on: the selected list, else the list of the selected file
/// (behaviours 9 and 10).
pub fn target_list(
    lists: &Changelists,
    selected_list: Option<&str>,
    selected_file: Option<&str>,
) -> Option<String> {
    selected_list
        .filter(|l| lists.get(l).is_some())
        .or_else(|| selected_file.and_then(|f| lists.list_of(f)))
        .map(str::to_owned)
}

/// The files Move acts on: the selected file if a list holds it, or all files of the selected
/// list (behaviour 12).
pub fn files_to_move(
    lists: &Changelists,
    selected_list: Option<&str>,
    selected_file: Option<&str>,
) -> Vec<String> {
    if let Some(list) = selected_list.and_then(|l| lists.get(l)) {
        return list.files.iter().cloned().collect();
    }
    selected_file
        .filter(|f| lists.list_of(f).is_some())
        .map(|f| vec![f.to_owned()])
        .unwrap_or_default()
}

/// The strip Delete shows for `list`, or `None` when the list goes without a question
/// (behaviour 11): a list that is not active asks when it has files; the active list asks for
/// the list that becomes active unless there is nothing to choose and nothing to move.
pub fn delete_strip(lists: &Changelists, list: &str) -> Option<ListStrip> {
    let files = lists.get(list)?.files.len();
    let choose: Vec<String> = if lists.delete_asks_for_active(list) {
        lists
            .lists()
            .iter()
            .filter(|l| l.name != list)
            .map(|l| l.name.clone())
            .collect()
    } else {
        Vec::new()
    };
    let asks = !choose.is_empty() || (lists.active() != list && files > 0);
    asks.then(|| ListStrip::Delete {
        list: list.to_owned(),
        files,
        choose,
        selected: 0,
    })
}

/// What the status bar says without a list to act on.
pub const NO_LIST_SELECTED: &str = "Select a changelist or one of its files first";

/// What Delete Changelist says without a selected list.
pub const NO_LIST_TO_DELETE: &str = "Select the changelist to delete first";

/// What Move to Another Changelist… says without a file to move.
pub const NOTHING_TO_MOVE: &str = "Nothing is selected that can be moved";

/// What the changelist actions say in a remote project.
pub const NO_REMOTE_CHANGELISTS: &str = "Changelists are not available in a remote project yet";

/// What Delete Changelist says for the only list.
pub const LAST_LIST_STAYS: &str = "The last changelist stays";

/// What Set Active Changelist says for the active list.
pub fn already_active(list: &str) -> String {
    format!("'{list}' is the active changelist")
}

/// What the window says after a list was created.
pub fn created(list: &str, active: bool) -> String {
    if active {
        format!("Changelist '{list}' created and set active")
    } else {
        format!("Changelist '{list}' created")
    }
}

/// What the window says after Edit Changelist….
pub fn edited(old: &str, renamed: Option<&str>, activated: bool) -> String {
    match (renamed, activated) {
        (Some(new), true) => format!("Changelist '{old}' renamed to '{new}' and set active"),
        (Some(new), false) => format!("Changelist '{old}' renamed to '{new}'"),
        (None, true) => format!("'{old}' is the active changelist now"),
        (None, false) => format!("Changelist '{old}' is unchanged"),
    }
}

/// What the window says after Set Active Changelist.
pub fn activated(list: &str) -> String {
    format!("'{list}' is the active changelist now")
}

/// What the window says after Delete Changelist.
pub fn deleted(list: &str, outcome: &Deleted) -> String {
    match outcome {
        Deleted::Removed { files: 0, .. } => format!("Changelist '{list}' deleted"),
        Deleted::Removed { to, files } => {
            format!(
                "Changelist '{list}' deleted; {} moved to '{to}'",
                count(*files)
            )
        }
        Deleted::Replaced { .. } => format!(
            "Changelist '{list}' replaced by '{}': a project keeps one changelist",
            idecraft_core::changelists::DEFAULT_NAME
        ),
        Deleted::Kept => LAST_LIST_STAYS.to_owned(),
    }
}

/// What the window says after Move to Another Changelist….
pub fn moved(to: &str, outcome: &Moved) -> String {
    let n = outcome.moved.len();
    if n == 0 {
        return format!("Nothing moved: the files are in '{to}' already");
    }
    let new = if outcome.created {
        "the new changelist "
    } else {
        ""
    };
    format!("{} moved to {new}'{to}'", count(n))
}

/// What the window says when the state file was not used; `why` never quotes the file.
pub fn lists_unreadable(why: Unreadable) -> String {
    let then = if why.keeps_the_file() {
        "The file is left as it is and changelists cannot be changed"
    } else {
        "The next change of a changelist replaces it"
    };
    format!("The changelists ({CHANGELISTS_FILE}) were not read: {why}. {then}")
}

/// What an action says when the state file must stay as it is.
pub fn lists_kept(why: Unreadable) -> String {
    format!("Changelists cannot be changed: {CHANGELISTS_FILE} is left as it is, {why}")
}

/// What the window says when the lists could not be written; the reason is the file system's.
pub fn lists_not_saved(error: &std::io::Error) -> String {
    format!("The changelists were not saved ({CHANGELISTS_FILE}): {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists() -> Changelists {
        let mut l = Changelists::default();
        l.sync(&["a.rs".to_owned(), "b.rs".to_owned()], &[]);
        l.create("Cart", false).unwrap();
        l.move_files(&["b.rs".to_owned()], "Cart", false).unwrap();
        l.create("Empty", false).unwrap();
        l
    }

    /// SPEC-0335 AK-8 (behaviours 9, 10 and 12): what the actions act on.
    #[test]
    fn the_selected_list_or_the_list_of_the_selected_file() {
        let l = lists();
        assert_eq!(target_list(&l, Some("Cart"), None), Some("Cart".into()));
        assert_eq!(target_list(&l, None, Some("b.rs")), Some("Cart".into()));
        assert_eq!(target_list(&l, None, Some("a.rs")), Some("Changes".into()));
        assert_eq!(target_list(&l, None, Some("untracked.rs")), None);
        assert_eq!(target_list(&l, Some("Gone"), None), None);
        assert_eq!(target_list(&l, None, None), None);

        assert_eq!(files_to_move(&l, None, Some("a.rs")), ["a.rs"]);
        assert_eq!(files_to_move(&l, Some("Cart"), None), ["b.rs"]);
        assert!(files_to_move(&l, Some("Empty"), None).is_empty());
        assert!(files_to_move(&l, None, Some("untracked.rs")).is_empty());
        assert!(files_to_move(&l, None, None).is_empty());
    }

    /// SPEC-0335 AK-8 (behaviour 11): when Delete asks and what it offers.
    #[test]
    fn delete_asks_only_when_there_is_something_to_decide() {
        let l = lists();
        // Not active and empty: no question.
        assert_eq!(delete_strip(&l, "Empty"), None);
        // Not active with files: a question without a choice.
        let ask = delete_strip(&l, "Cart").unwrap();
        assert_eq!(ask.choices(), [] as [String; 0]);
        assert!(ask.title().contains("'Cart'") && ask.title().contains("1 file "));
        // The active list with others: the others to choose from.
        let ask = delete_strip(&l, "Changes").unwrap();
        assert_eq!(ask.choices(), ["Cart", "Empty"]);
        assert!(!ask.has_name());
        assert_eq!(ask.set_active(), None);
        // The active list, one other, no files: no question.
        let mut two = Changelists::default();
        two.create("Cart", true).unwrap();
        assert_eq!(delete_strip(&two, "Cart"), None);
        // The only list: no question.
        assert_eq!(delete_strip(&Changelists::default(), "Changes"), None);
        assert_eq!(delete_strip(&l, "Gone"), None);
    }

    /// SPEC-0335 AK-8: the option "Set active" of the strips.
    #[test]
    fn set_active_is_fixed_for_the_active_list() {
        let mut new = ListStrip::New { set_active: false };
        new.toggle_set_active();
        assert_eq!(new.set_active(), Some((true, true)));
        let mut fixed = ListStrip::Edit {
            list: "Changes".into(),
            set_active: true,
            fixed: true,
        };
        fixed.toggle_set_active();
        assert_eq!(fixed.set_active(), Some((true, false)));
        let mut free = ListStrip::Edit {
            list: "Cart".into(),
            set_active: false,
            fixed: false,
        };
        free.toggle_set_active();
        assert_eq!(free.set_active(), Some((true, true)));
        assert!(free.has_name() && free.choices().is_empty());
    }

    /// SPEC-0335 behaviour 21: the texts name the file and the reason, and say what follows.
    #[test]
    fn the_texts_name_the_file_and_the_reason() {
        let broken = lists_unreadable(Unreadable::Broken);
        assert!(broken.contains(CHANGELISTS_FILE) && broken.contains("replaces it"));
        let newer = lists_unreadable(Unreadable::Version(2));
        assert!(newer.contains("version 2") && newer.contains("left as it is"));
        assert!(lists_kept(Unreadable::Version(2)).contains("cannot be changed"));
        assert_eq!(
            deleted(
                "Cart",
                &Deleted::Removed {
                    to: "Changes".into(),
                    files: 2
                }
            ),
            "Changelist 'Cart' deleted; 2 files moved to 'Changes'"
        );
        assert_eq!(deleted("Changes", &Deleted::Kept), LAST_LIST_STAYS);
        let m = Moved {
            created: true,
            moved: vec!["a.rs".into()],
            ..Moved::default()
        };
        assert_eq!(moved("Api", &m), "1 file moved to the new changelist 'Api'");
        assert_eq!(ListOp::ALL.len(), 5);
    }
}
