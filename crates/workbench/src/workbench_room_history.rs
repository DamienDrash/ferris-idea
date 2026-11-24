//! The project room's history in a remote project's window (`SPEC-0323` v9 behaviours 61–65):
//! the Room History dialog with the rows of the journal, the diff of the selected row and Restore
//! this version with its question in the dialog. The decisions are in
//! [`crate::room_history_model`]; this module asks the backend, feeds the model and draws what it
//! says.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ferris_idea_session::RpcError;
use gpui::{AnyElement, Context, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use serde_json::{Value, json};

use super::{WorkbenchView, c};
use crate::palette::Palette;
use crate::room_history_model::{
    DiffLine, FileState, HISTORY_NEEDS_ROOM, HistoryModel, RESTORE_NEEDS_ROLE,
};
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};

/// How long one call of the dialog may take.
const HISTORY_LIMIT: Duration = Duration::from_secs(10);
/// The shortest time between two loads while `room.changed` arrives (behaviour 62).
const RELOAD_EVERY: Duration = Duration::from_secs(1);

/// The open Room History dialog.
pub(crate) struct HistoryDialog {
    /// What it shows.
    pub model: HistoryModel,
    focus: FocusHandle,
    /// A load is under way.
    loading: bool,
    /// When the last load started.
    loaded_at: Option<Instant>,
    /// `room.changed` arrived since the last load.
    stale: bool,
    /// A restore is under way.
    busy: bool,
}

/// Unix milliseconds now.
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A button of the dialog with what the driver and the window tests see of it.
fn button(
    id: impl Into<String>,
    label: &'static str,
    primary: bool,
    effect: Effect,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let semantics = Semantics::new(Role::Button)
        .name(label)
        .test_id(id.clone())
        .action(AxAction::Click)
        .effects(&[effect]);
    crate::metrics::button(gpui::SharedString::from(id), label, primary)
        .relative()
        .semantics(semantics)
}

impl WorkbenchView {
    /// The Room History dialog's model while it is open (window tests).
    pub fn room_history_model(&self) -> Option<&HistoryModel> {
        Some(&self.remote_project.as_ref()?.room.history.as_ref()?.model)
    }

    fn history_mut(&mut self) -> Option<&mut HistoryDialog> {
        self.remote_project.as_mut()?.room.history.as_mut()
    }

    /// Room History… from the File menu, Find Action and the Share dialog (behaviour 61).
    pub fn open_room_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self
            .remote_project
            .as_mut()
            .filter(|r| r.room.view.is_some())
        else {
            self.status = Some(HISTORY_NEEDS_ROOM.into());
            cx.notify();
            return;
        };
        r.room.share = None;
        if r.room.history.is_none() {
            r.room.history = Some(HistoryDialog {
                model: HistoryModel::default(),
                focus: cx.focus_handle(),
                loading: false,
                loaded_at: None,
                stale: false,
                busy: false,
            });
        }
        if let Some(d) = &r.room.history {
            window.focus(&d.focus);
        }
        self.history_load(cx);
        cx.notify();
    }

    /// Closes the dialog.
    pub fn close_room_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(r) = self.remote_project.as_mut() {
            r.room.history = None;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// `room.changed`: the dialog loads again on the next look, at most once a second
    /// (behaviour 62).
    pub(super) fn room_history_changed(&mut self) {
        if let Some(d) = self.history_mut() {
            d.stale = true;
        }
    }

    /// One look of the window: loads again when the room changed.
    pub(super) fn room_history_tick(&mut self, cx: &mut Context<Self>) {
        let due = self.history_mut().is_some_and(|d| {
            d.stale && !d.loading && d.loaded_at.is_none_or(|t| t.elapsed() >= RELOAD_EVERY)
        });
        if due {
            self.history_load(cx);
        }
    }

    /// Asks the room's state and the newest entries of its journal (behaviour 62).
    fn history_load(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let live = r.live.clone();
        let Some(d) = self.history_mut() else {
            return;
        };
        d.loading = true;
        d.stale = false;
        d.loaded_at = Some(Instant::now());
        let task = cx.background_executor().spawn(async move {
            let own = live.try_with(|l| l.session_ref().info.client.clone());
            let status = live.host_call("room.status", json!({}), HISTORY_LIMIT)?;
            let history = live.host_call(
                "room.history",
                HistoryModel::history_params(&status),
                HISTORY_LIMIT,
            )?;
            Ok::<_, RpcError>((own, status, history))
        });
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(d) = this.history_mut() else {
                    return;
                };
                d.loading = false;
                let again = match answer {
                    Ok((own, status, history)) => {
                        if let Some(own) = own {
                            d.model.set_role(&status, &own);
                        }
                        d.model.set_history(&history)
                    }
                    Err(e) => {
                        d.model.error = Some(e.message);
                        false
                    }
                };
                if again {
                    this.history_load_files(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks `room.diff` for every file of the selected row (behaviour 63).
    fn history_load_files(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let live = r.live.clone();
        let Some(d) = self.history_mut() else {
            return;
        };
        let asks: Vec<Value> = d
            .model
            .start_files()
            .iter()
            .filter_map(|f| f.ask.clone())
            .collect();
        if asks.is_empty() {
            return;
        }
        let task = cx.background_executor().spawn(async move {
            asks.into_iter()
                .map(|ask| {
                    let answer = live.host_call("room.diff", ask.clone(), HISTORY_LIMIT);
                    (ask, answer)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let answers = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(d) = this.history_mut() {
                    for (ask, answer) in answers {
                        d.model
                            .set_diff(&ask, answer.as_ref().map_err(|e| e.message.as_str()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A click on a row.
    pub fn room_history_select(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.history_mut().is_some_and(|d| d.model.select(id)) {
            self.history_load_files(cx);
        }
        cx.notify();
    }

    /// Restore this version, first click: the dialog asks (behaviour 64).
    pub fn room_history_ask_restore(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.history_mut() {
            d.model.ask_restore();
        }
        cx.notify();
    }

    /// Keep: nothing happens.
    pub fn room_history_keep(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.history_mut() {
            d.model.keep();
        }
        cx.notify();
    }

    /// Restore: every file of the row back as one change of the room (behaviour 64).
    pub fn room_history_restore(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let live = r.live.clone();
        let Some(d) = self.history_mut().filter(|d| !d.busy) else {
            return;
        };
        let Some(params) = d.model.confirm_restore() else {
            cx.notify();
            return;
        };
        d.busy = true;
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call("room.restore", params, HISTORY_LIMIT) });
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(d) = this.history_mut() {
                    d.busy = false;
                    d.model
                        .restored(answer.as_ref().map_err(|e| e.message.as_str()));
                }
                this.history_load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn history_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.close_room_history(window, cx);
            cx.stop_propagation();
        }
    }

    /// The dialog, drawn above the window (behaviours 61–65).
    pub(super) fn room_history_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.remote_project.as_ref()?;
        let d = r.room.history.as_ref()?;
        let m = &d.model;
        let p = Palette::current();
        let now = unix_ms();
        let selected = m.selected().map(|row| row.id);
        let mut list = div()
            .id("room-history-rows")
            .w(px(320.))
            .flex_none()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(c(p.separator));
        if m.known() && m.rows().is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(c(p.ui_dim))
                    .child("Nothing has changed in this room yet"),
            );
        } else if !m.known() {
            list = list.child(div().p_2().text_color(c(p.ui_dim)).child("Loading…"));
        }
        for row in m.rows() {
            let id = row.id;
            let on = selected == Some(id);
            let summary = row.summary();
            let byline = row.byline(now);
            list = list.child(
                div()
                    .id(gpui::SharedString::from(format!("room-history-row-{id}")))
                    .px_2()
                    .py_1()
                    .flex()
                    .flex_col()
                    .when(on, |d| d.bg(c(p.tree_selection)))
                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .relative()
                    .semantics(
                        Semantics::new(Role::ListItem)
                            .name(summary.clone())
                            .description(byline.clone())
                            .test_id(format!("room-history-row-{id}"))
                            .selected(on)
                            .action(AxAction::Click),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.room_history_select(id, cx)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .when(row.version().is_some(), |d| {
                                d.child(
                                    div()
                                        .px_1()
                                        .rounded(px(3.))
                                        .text_xs()
                                        .bg(c(p.accent))
                                        .text_color(gpui::white())
                                        .child("Version"),
                                )
                            })
                            .child(summary),
                    )
                    .child(div().text_xs().text_color(c(p.ui_muted)).child(byline)),
            );
        }
        let detail = self.room_history_detail(m, d.busy, cx);
        let body = div()
            .id("room-history-dialog")
            .track_focus(&d.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::history_key))
            .w(px(940.))
            .h(px(560.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("Room history of {}", r.live.folder())),
            )
            .when_some(m.error.clone(), |el, e| {
                el.child(
                    div()
                        .id("room-history-error")
                        .text_color(c(p.error_text))
                        .child(e),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .border_1()
                    .border_color(c(p.separator))
                    .rounded(px(4.))
                    .child(list)
                    .child(detail),
            )
            .child(div().flex().child(div().flex_1()).child(
                button("room-history-close", "Close", false, Effect::None).on_click(
                    cx.listener(|this, _, window, cx| this.close_room_history(window, cx)),
                ),
            ));
        Some(body.into_any_element())
    }

    /// The right side: the selected row, Restore and the diffs (behaviours 63–65).
    fn room_history_detail(
        &self,
        m: &HistoryModel,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current();
        let mut detail = div()
            .id("room-history-detail")
            .flex_1()
            .min_w_0()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll();
        let Some(row) = m.selected() else {
            return detail;
        };
        detail = detail.child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(row.summary()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(row.byline(unix_ms())),
                ),
        );
        let has_files = !m.restore_docs().is_empty();
        if !m.can_restore() {
            detail = detail.child(
                div()
                    .id("room-history-role")
                    .text_xs()
                    .text_color(c(p.ui_muted))
                    .child(RESTORE_NEEDS_ROLE),
            );
        } else if m.confirming() {
            detail = detail.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_color(c(p.error_text)).child(m.question()))
                    .child(
                        button("room-history-restore-yes", "Restore", true, Effect::Write)
                            .on_click(cx.listener(|this, _, _, cx| this.room_history_restore(cx))),
                    )
                    .child(
                        button("room-history-restore-keep", "Keep", false, Effect::None)
                            .on_click(cx.listener(|this, _, _, cx| this.room_history_keep(cx))),
                    ),
            );
        } else if has_files && !busy {
            detail = detail.child(
                div().flex().child(
                    button(
                        "room-history-restore",
                        "Restore this version",
                        false,
                        Effect::None,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.room_history_ask_restore(cx))),
                ),
            );
        }
        if let Some(note) = &m.note {
            detail = detail.child(
                div()
                    .id("room-history-note")
                    .text_xs()
                    .text_color(c(p.ui_muted))
                    .child(note.clone()),
            );
        }
        let lh = crate::fonts::editor_line_height();
        for (i, f) in m.files().iter().enumerate() {
            let mut block = div()
                .flex()
                .flex_col()
                .border_1()
                .border_color(c(p.separator))
                .rounded(px(4.));
            let (value, lines) = match &f.state {
                FileState::Loading => ("loading".to_owned(), None),
                FileState::Said(s) => (s.clone(), None),
                FileState::Shown {
                    lines,
                    added,
                    removed,
                } => (format!("+{added} −{removed}"), Some(lines)),
            };
            if !f.doc.is_empty() || lines.is_some() {
                block = block.child(
                    div()
                        .px_2()
                        .py_1()
                        .flex()
                        .gap_2()
                        .bg(c(p.toolbar_hover))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(crate::room_comments_model::shown_text(&f.doc)),
                        )
                        .when(lines.is_some(), |d| {
                            d.child(div().text_color(c(p.ui_muted)).child(value.clone()))
                        }),
                );
            }
            match (&f.state, lines) {
                (_, Some(lines)) => {
                    let mut code = div()
                        .flex()
                        .flex_col()
                        .font_family(crate::fonts::editor_family())
                        .text_size(px(crate::fonts::editor_font_size()));
                    for l in lines {
                        code = code.child(match l {
                            DiffLine::Gap => {
                                div().h(px(lh)).px_2().text_color(c(p.ui_dim)).child("⋯")
                            }
                            DiffLine::Line(sign, text) => {
                                let bg = match sign {
                                    '+' => Some(p.diff_inserted),
                                    '−' => Some(p.diff_deleted),
                                    _ => None,
                                };
                                div()
                                    .h(px(lh))
                                    .px_2()
                                    .flex()
                                    .whitespace_nowrap()
                                    .when_some(bg, |d, bg| d.bg(gpui::rgba((bg << 8) | 0x55)))
                                    .child(
                                        div()
                                            .w(px(14.))
                                            .flex_none()
                                            .text_color(c(p.ui_muted))
                                            .child(sign.to_string()),
                                    )
                                    .child(text.clone())
                            }
                        });
                    }
                    block = block.child(code);
                }
                (FileState::Loading, None) => {
                    block = block.child(
                        div()
                            .px_2()
                            .py_1()
                            .text_color(c(p.ui_dim))
                            .child("Loading…"),
                    );
                }
                (_, None) => {
                    block = block.child(
                        div()
                            .px_2()
                            .py_1()
                            .text_color(c(p.ui_muted))
                            .child(value.clone()),
                    );
                }
            }
            detail = detail.child(
                block.relative().semantics(
                    Semantics::new(Role::Group)
                        .name(if f.doc.is_empty() {
                            "Version".to_owned()
                        } else {
                            f.doc.clone()
                        })
                        .value(value)
                        .test_id(format!("room-history-file-{i}")),
                ),
            );
        }
        detail
    }
}
