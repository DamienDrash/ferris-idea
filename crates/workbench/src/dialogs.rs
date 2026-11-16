//! Simple confirmations as native alerts (`SPEC-0281` behaviour 6), and questions drawn in the
//! window (`SPEC-0281` v2 behaviour 6, `SPEC-0261` v4 behaviour 5).
//!
//! GPUI 0.2 has no accessibility tree (ADR-0019), so a question IDECraft draws itself is invisible
//! to screen readers. A question whose answers are only buttons is asked with the platform's alert
//! instead (on macOS a system sheet on the window, read and pressed by accessibility clients; on
//! Linux GPUI draws it). The window self-test cannot answer a native alert, so it sets
//! [`DrawnConfirmations`] and gets the same question drawn in the window (behaviour 7).
//!
//! A question that keeps IDECraft from quitting or a window from closing is always drawn
//! ([`DrawnQuestion`]): a system sheet that does not appear (seen with Stage Manager on macOS 27)
//! would leave no way out.

use gpui::{App, Global};

/// Set by the window self-test: confirmations are drawn in the window instead of native alerts.
pub struct DrawnConfirmations;

impl Global for DrawnConfirmations {}

/// Whether confirmations are asked in native alerts.
pub fn native_alerts(cx: &App) -> bool {
    !cx.has_global::<DrawnConfirmations>()
}

/// A question with buttons, drawn by IDECraft above everything else in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnQuestion {
    /// Headline.
    pub message: String,
    /// Below the headline; line breaks are kept.
    pub detail: String,
    /// Button labels in the order of the answers.
    pub buttons: Vec<String>,
    /// The answer Return gives (drawn as the primary button).
    pub default: usize,
    /// The answer Escape gives, if any.
    pub cancel: Option<usize>,
}

impl DrawnQuestion {
    /// The quit question of `SPEC-0261` behaviour 2: the first answer is the default, the one
    /// without a choice cancels.
    pub fn quit(q: &crate::quit_model::QuitQuestion) -> Self {
        Self {
            message: q.message.clone(),
            detail: q.detail.clone(),
            buttons: q.answers.iter().map(|(l, _)| (*l).to_owned()).collect(),
            default: 0,
            cancel: q.answers.iter().position(|(_, c)| c.is_none()),
        }
    }

    /// A notice with only OK, which Return and Escape both close (`SPEC-0261` behaviour 15).
    pub fn notice(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            detail: detail.into(),
            buttons: vec!["OK".into()],
            default: 0,
            cancel: Some(0),
        }
    }

    /// The question before closing a remote project with `unsaved` files (`SPEC-0317` behaviour
    /// 11, `SPEC-0281` v2 behaviour 6): Save All (default), Don't Save, Cancel.
    pub fn remote_close(unsaved: &[String]) -> Self {
        Self {
            message: format!(
                "{} remote file{} unsaved",
                unsaved.len(),
                if unsaved.len() == 1 { " is" } else { "s are" }
            ),
            detail: unsaved.join("\n"),
            buttons: vec!["Save All".into(), "Don't Save".into(), "Cancel".into()],
            default: 0,
            cancel: Some(2),
        }
    }

    /// The answer a key gives: Return the default, Escape the cancel answer.
    pub fn answer_for_key(&self, key: &str) -> Option<usize> {
        match key {
            "enter" => Some(self.default),
            "escape" => self.cancel,
            _ => None,
        }
    }

    /// The buttons as drawn, left to right: the default rightmost (macOS order), the others in
    /// reverse, so Cancel comes first.
    pub fn drawn_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.buttons.len())
            .rev()
            .filter(|i| *i != self.default)
            .collect();
        order.push(self.default);
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quit_model::question;

    #[test]
    fn the_quit_question_is_drawn_with_its_message_detail_and_buttons() {
        let q = question(
            &["a.txt".into()],
            &["web: run configuration \"dev\"".into()],
        )
        .unwrap();
        let d = DrawnQuestion::quit(&q);
        assert_eq!(d.message, "Save changes before quitting?");
        assert_eq!(d.detail, q.detail);
        assert!(d.detail.contains("a.txt") && d.detail.contains("Still running"));
        assert_eq!(
            d.buttons,
            ["Save All and Quit", "Quit Without Saving", "Cancel"]
        );
        assert_eq!(d.answer_for_key("enter"), Some(0));
        assert_eq!(d.answer_for_key("escape"), Some(2));
        assert_eq!(d.answer_for_key("x"), None);
        assert_eq!(d.drawn_order(), [2, 1, 0]);

        let running = question(&[], &["web: run configuration \"dev\"".into()]).unwrap();
        let d = DrawnQuestion::quit(&running);
        assert_eq!(d.message, "Stop running processes and quit?");
        assert_eq!(d.buttons, ["Stop and Quit", "Cancel"]);
        assert_eq!(d.answer_for_key("escape"), Some(1));
        assert_eq!(d.drawn_order(), [1, 0]);
    }

    #[test]
    fn the_notice_has_only_ok() {
        let d = DrawnQuestion::notice("IDECraft did not quit", "not saved: a.txt");
        assert_eq!(d.buttons, ["OK"]);
        assert_eq!(d.answer_for_key("enter"), Some(0));
        assert_eq!(d.answer_for_key("escape"), Some(0));
        assert_eq!(d.drawn_order(), [0]);
    }

    #[test]
    fn the_remote_close_question_names_the_files() {
        let d = DrawnQuestion::remote_close(&["a.rs".into(), "b.rs".into()]);
        assert_eq!(d.message, "2 remote files are unsaved");
        assert_eq!(d.detail, "a.rs\nb.rs");
        assert_eq!(d.buttons, ["Save All", "Don't Save", "Cancel"]);
        assert_eq!(d.answer_for_key("escape"), Some(2));
        assert_eq!(
            DrawnQuestion::remote_close(&["a.rs".into()]).message,
            "1 remote file is unsaved"
        );
    }
}
