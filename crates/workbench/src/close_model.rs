//! Closing project windows (`SPEC-0094` v3 behaviours 6 to 8, `SPEC-0261` v6 behaviour 19),
//! toolkit-free. Which windows close and when each action is offered follow the open platform's
//! `CloseProjectsActionBase` and its three actions (logic template, no code; `PROVENANCE.md`).

use std::path::{Path, PathBuf};

use crate::quit_model::{QuitChoice, QuitQuestion, cut};

/// Which project windows an action closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseScope {
    /// The window the action is used in (`CloseProject`).
    This,
    /// Every project window (`CloseAllProjects`).
    All,
    /// Every project window but the current one (`CloseOtherProjects`).
    Others,
}

impl CloseScope {
    /// The scope of action `id`, `None` for any other action.
    pub fn of_action(id: &str) -> Option<Self> {
        match id {
            "CloseProject" => Some(Self::This),
            "CloseAllProjects" => Some(Self::All),
            "CloseOtherProjects" => Some(Self::Others),
            _ => None,
        }
    }

    /// What a local project window's close button closes: its project, as Close Project
    /// (`SPEC-0094` v4 behaviour 11).
    pub fn of_close_button() -> Self {
        Self::This
    }

    /// The scope named by the MCP tool (`this`, `all`, `others`).
    pub fn of_name(name: &str) -> Option<Self> {
        match name {
            "this" => Some(Self::This),
            "all" => Some(Self::All),
            "others" => Some(Self::Others),
            _ => None,
        }
    }

    /// Whether the action is offered while `open` project windows are open: closing this one
    /// whenever there is one, all or the others only when there are several (behaviour 6).
    pub fn offered(self, open: usize) -> bool {
        match self {
            Self::This => open >= 1,
            Self::All | Self::Others => open > 1,
        }
    }

    /// The projects that close, of the `open` ones, when the action is used in the window of
    /// `current`: the others first, `current` last (behaviour 8). Empty when it is not offered.
    pub fn targets(self, open: &[PathBuf], current: &Path) -> Vec<PathBuf> {
        if !self.offered(open.len()) {
            return Vec::new();
        }
        let mut out: Vec<PathBuf> = open
            .iter()
            .filter(|p| p.as_path() != current)
            .filter(|_| self != Self::This)
            .cloned()
            .collect();
        if self != Self::Others && open.iter().any(|p| p.as_path() == current) {
            out.push(current.to_path_buf());
        }
        out
    }
}

/// The files of `unsaved` that belong to one of the `closing` projects (behaviour 7: the other
/// windows' files are not named).
pub fn unsaved_in(unsaved: &[PathBuf], closing: &[PathBuf]) -> Vec<PathBuf> {
    unsaved
        .iter()
        .filter(|f| closing.iter().any(|r| f.starts_with(r)))
        .cloned()
        .collect()
}

/// The question before closing the `projects` (folder names) with the `unsaved` files (named) and
/// the `running` work; `None` when neither is there, so the windows close at once (`SPEC-0261` v6
/// behaviour 19).
pub fn question(
    projects: &[String],
    unsaved: &[String],
    running: &[String],
) -> Option<QuitQuestion> {
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
            "Still running, ended when the window closes:\n{}",
            cut(running)
        ));
    }
    let what = match projects {
        [one] => one.clone(),
        many => format!("the {} projects", many.len()),
    };
    let (message, answers) = if unsaved.is_empty() {
        (
            "Stop running processes and close?".to_owned(),
            vec![
                ("Stop and Close", Some(QuitChoice::Discard)),
                ("Cancel", None),
            ],
        )
    } else {
        (
            format!("Save changes before closing {what}?"),
            vec![
                ("Save All and Close", Some(QuitChoice::SaveAll)),
                ("Close Without Saving", Some(QuitChoice::Discard)),
                ("Cancel", None),
            ],
        )
    };
    Some(QuitQuestion {
        message,
        detail: parts.join("\n\n"),
        answers,
    })
}

/// The folder name of project `root`, as the question names it.
pub fn project_name(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    /// SPEC-0094 v3 AK-5: which windows each action closes, in which order, and when it is offered.
    #[test]
    fn each_action_closes_its_windows_current_last() {
        let open = paths(&["/p/a", "/p/b", "/p/c"]);
        let current = Path::new("/p/b");
        assert_eq!(CloseScope::This.targets(&open, current), paths(&["/p/b"]));
        assert_eq!(
            CloseScope::All.targets(&open, current),
            paths(&["/p/a", "/p/c", "/p/b"])
        );
        assert_eq!(
            CloseScope::Others.targets(&open, current),
            paths(&["/p/a", "/p/c"])
        );
        // One window: only Close Project is offered.
        let one = paths(&["/p/a"]);
        assert_eq!(CloseScope::This.targets(&one, Path::new("/p/a")), one);
        assert!(CloseScope::All.targets(&one, Path::new("/p/a")).is_empty());
        assert!(
            CloseScope::Others
                .targets(&one, Path::new("/p/a"))
                .is_empty()
        );
        assert!(CloseScope::This.offered(1));
        assert!(!CloseScope::All.offered(1) && !CloseScope::Others.offered(1));
        assert!(CloseScope::All.offered(2) && CloseScope::Others.offered(2));
        assert!(!CloseScope::This.offered(0));
    }

    /// SPEC-0094 v4 AK-9: the close button closes its own window as Close Project, with the same
    /// question naming that project.
    #[test]
    fn the_close_button_is_close_project() {
        let scope = CloseScope::of_close_button();
        assert_eq!(scope, CloseScope::of_action("CloseProject").unwrap());
        let open = paths(&["/p/a", "/p/b"]);
        assert_eq!(scope.targets(&open, Path::new("/p/b")), paths(&["/p/b"]));
        let q = question(&["b".into()], &["src/x.rs".into()], &[]).unwrap();
        assert_eq!(q.message, "Save changes before closing b?");
    }

    /// SPEC-0094 v3 AK-5: the action ids and the tool's names.
    #[test]
    fn actions_and_names() {
        assert_eq!(
            CloseScope::of_action("CloseProject"),
            Some(CloseScope::This)
        );
        assert_eq!(
            CloseScope::of_action("CloseAllProjects"),
            Some(CloseScope::All)
        );
        assert_eq!(
            CloseScope::of_action("CloseOtherProjects"),
            Some(CloseScope::Others)
        );
        assert_eq!(CloseScope::of_action("CloseContent"), None);
        assert_eq!(CloseScope::of_name("others"), Some(CloseScope::Others));
        assert_eq!(CloseScope::of_name("every"), None);
    }

    /// SPEC-0261 v6 AK-13, SPEC-0094 v3 AK-6: the question uses the words of closing; only the
    /// files of the projects that close are named.
    #[test]
    fn the_question_uses_the_words_of_closing() {
        assert!(question(&["a".into()], &[], &[]).is_none());
        let q = question(&["shop".into()], &["src/main.rs".into()], &[]).unwrap();
        assert_eq!(q.message, "Save changes before closing shop?");
        assert_eq!(
            q.answers,
            vec![
                ("Save All and Close", Some(QuitChoice::SaveAll)),
                ("Close Without Saving", Some(QuitChoice::Discard)),
                ("Cancel", None),
            ]
        );
        assert_eq!(q.detail, "src/main.rs");
        let q = question(
            &["a".into(), "b".into()],
            &[],
            &["a: run configuration \"server\"".into()],
        )
        .unwrap();
        assert_eq!(q.message, "Stop running processes and close?");
        assert_eq!(
            q.detail,
            "Still running, ended when the window closes:\na: run configuration \"server\""
        );
        assert_eq!(q.answers[0], ("Stop and Close", Some(QuitChoice::Discard)));
        let q = question(&["a".into(), "b".into()], &["a/x".into()], &[]).unwrap();
        assert_eq!(q.message, "Save changes before closing the 2 projects?");
        let unsaved = paths(&["/p/a/x.rs", "/p/b/y.rs", "/p/ab/z.rs"]);
        assert_eq!(
            unsaved_in(&unsaved, &paths(&["/p/a"])),
            paths(&["/p/a/x.rs"])
        );
        assert_eq!(project_name(Path::new("/p/shop")), "shop");
    }
}
