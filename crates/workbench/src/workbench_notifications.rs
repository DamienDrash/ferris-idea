//! The Notifications tool window in the workbench (`SPEC-0061` v17 behaviour 64): the notes the
//! status bar showed in this project window, newest first, with their repeats, and Clear All.
//!
//! The log itself is [`ferris_idea_core::notifications`]; the window records a note there when it
//! first shows it, the MCP tool `notifications` reads it.

use ferris_idea_core::notifications::{Kind, Notification, NotificationRegistry};
use gpui::{Context, Window, div, prelude::*, px};

use super::{WorkbenchView, c};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::{Tipped, declared};

/// Whether informational notes are kept (`SPEC-0337` behaviour 3); failures always are.
fn keeps_info(cx: &gpui::App) -> bool {
    crate::appearance::ide_settings(cx).notifications.log_info
}

/// What the window says without notifications.
pub const EMPTY: &str = "No notifications";

/// The state of the Notifications tool window in the workbench.
#[derive(Default)]
pub(super) struct NotificationsState {
    /// Whether the window is shown.
    pub(super) shown: bool,
    /// The status note last recorded, so a note is recorded once per showing.
    logged: Option<String>,
    /// The log of an earlier window of the same project was dropped (a new window starts empty).
    started: bool,
}

impl WorkbenchView {
    /// Records the status note when it is new (SPEC-0061 v17 behaviour 64): once per showing, a
    /// passing hint not at all. Called on every frame; notes are set in many places and each is
    /// drawn before it can be seen.
    pub(super) fn log_status_note(&mut self, cx: &gpui::App) {
        if !self.notifications.started {
            self.notifications.started = true;
            NotificationRegistry::global().clear(&self.root);
        }
        let note = self.status_note().map(str::to_owned);
        if note == self.notifications.logged {
            return;
        }
        if let Some(text) = &note
            && !self.status_is_passing_hint()
        {
            let kind = if crate::status_note_model::is_failure(text) {
                Kind::Error
            } else {
                Kind::Info
            };
            // Settings › Notifications (SPEC-0337 behaviour 3): informational notes may be left
            // out; a failure is always kept.
            if kind == Kind::Error || keeps_info(cx) {
                NotificationRegistry::global().record(&self.root, text, kind);
            }
        }
        self.notifications.logged = note;
    }

    /// The notifications of this window, newest first (MCP, tests).
    pub fn notifications(&self) -> Vec<Notification> {
        NotificationRegistry::global().list(&self.root)
    }

    /// Clear All (behaviour 64): empties the list.
    pub fn clear_notifications(&mut self, cx: &mut Context<Self>) {
        NotificationRegistry::global().clear(&self.root);
        cx.notify();
    }

    /// Shows or hides the Notifications window; it has no focusable content, so the focus stays.
    pub(super) fn toggle_notifications(&mut self, cx: &mut Context<Self>) {
        self.notifications.shown = !self.notifications.shown;
        cx.notify();
    }

    /// Clear All in the row below the header.
    pub(super) fn notifications_clear_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        crate::metrics::header_button("notifications-clear-all")
            .tip(declared!(OTHER_TOOL_WINDOWS, "notifications-clear-all"))
            .child(icon("general/delete"))
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                this.clear_notifications(cx);
            }))
            .into_any_element()
    }

    /// The list: per notification its icon, its text, the time and the repeats.
    pub(super) fn notifications_body(&self, _window: &Window) -> gpui::AnyElement {
        let p = Palette::current();
        let list = self.notifications();
        if list.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(c(p.ui_muted))
                .child(EMPTY)
                .into_any_element();
        }
        let zone = crate::git_log_model::LocalZone::system();
        let rows = list.into_iter().enumerate().map(|(i, n)| {
            let meta = row_meta(&n, &zone);
            div()
                .id(("notification", i))
                .flex()
                .gap_2()
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(c(p.separator))
                .child(div().flex_none().pt(px(2.)).child(icon(match n.kind {
                    Kind::Error => "status/error",
                    Kind::Info => "status/info",
                })))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_color(c(p.ui_text)).child(n.text))
                        .child(div().text_xs().text_color(c(p.ui_muted)).child(meta)),
                )
        });
        div()
            .id("notifications-list")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }
}

/// The line below a notification's text: the local time of its last showing and its repeats.
pub fn row_meta(n: &Notification, zone: &crate::git_log_model::LocalZone) -> String {
    let time = n.unix_time() as i64;
    let local = (time + zone.offset_at(time)).rem_euclid(86_400);
    let clock = format!("{:02}:{:02}", local / 3600, local % 3600 / 60);
    if n.count > 1 {
        format!("{clock} · ×{}", n.count)
    } else {
        clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// SPEC-0061 AK-v17b: the time in the zone and the repeats.
    #[test]
    fn a_row_shows_its_time_and_repeats() {
        let n = Notification {
            text: "Saved".into(),
            kind: Kind::Info,
            count: 1,
            time: SystemTime::UNIX_EPOCH + Duration::from_secs(13 * 3600 + 5 * 60 + 9),
        };
        let utc = crate::git_log_model::LocalZone::Fixed(0);
        assert_eq!(row_meta(&n, &utc), "13:05");
        let plus_one = crate::git_log_model::LocalZone::Fixed(3600);
        assert_eq!(
            row_meta(&Notification { count: 3, ..n }, &plus_one),
            "14:05 · ×3"
        );
    }
}
