//! Sharing the project room from a remote project's window (`SPEC-0323` v6 behaviours 37–43):
//! the Share dialog (start the room, make an expiring link and copy it on a click, revoke links,
//! remove people), the Share button in the main toolbar and the room widget in the status bar.
//! The decisions are in [`crate::room_share_model`]; this module asks the backend, feeds the
//! model and draws what it says. The link's secret lives only in the open dialog.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ferris_idea_remote::ConnState;
use ferris_idea_session::{ErrorCode, RpcError};
use gpui::{
    AnyElement, ClipboardItem, Context, FocusHandle, KeyDownEvent, Window, div, prelude::*, px,
};
use serde_json::{Value, json};

use super::{WorkbenchView, c, status_widget};
use crate::palette::Palette;
use crate::room_share_model::{Connection, Lifetime, ShareModel, ShareRole, Widget, remaining};
use crate::semantics::{Annotated, AxAction, Role, Semantics};
use crate::tips::Tipped;

/// How long one call of the dialog may take.
const SHARE_LIMIT: Duration = Duration::from_secs(10);

/// What the status bar says for Share Project… on a local project (behaviour 37).
pub const SHARE_NEEDS_REMOTE: &str =
    "Sharing needs a project opened from a host (File › Remote Development…)";

/// The open Share dialog.
pub(crate) struct ShareDialog {
    /// What it shows; the link made last lives only here.
    pub model: ShareModel,
    focus: FocusHandle,
    /// A call is under way; buttons wait.
    busy: bool,
    /// What a link carries of the host, once worked out (`SPEC-0323` v7 behaviour 44).
    pub(super) origin: Option<super::room_join::LinkOrigin>,
}

/// Unix milliseconds now.
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The connection as the room widget names it.
pub(super) fn connection_of(state: &ConnState) -> Connection {
    match state {
        ConnState::Connected { .. } => Connection::Connected,
        ConnState::Reconnecting { .. } => Connection::Reconnecting,
        ConnState::Disconnected { .. } | ConnState::Closed => Connection::Offline,
    }
}

fn reason(e: &RpcError) -> String {
    e.message.clone()
}

/// What the driver and the window tests see of a dialog button (`SPEC-0319` behaviour 44).
fn button_semantics(id: &str, label: &str) -> Semantics {
    Semantics::new(Role::Button)
        .name(label)
        .test_id(id)
        .action(AxAction::Click)
}

/// A button of the dialog, annotated with its test id.
fn button(id: &'static str, label: impl Into<String>, primary: bool) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    let semantics = button_semantics(id, &label);
    crate::metrics::button(id, label, primary)
        .relative()
        .semantics(semantics)
}

/// A button whose test id carries a key (a row's id).
fn row_button(name: String, label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
    let semantics = button_semantics(&name, label);
    crate::metrics::button(gpui::SharedString::from(name), label, primary)
        .relative()
        .semantics(semantics)
}

impl WorkbenchView {
    /// The Share dialog's model while it is open (window tests).
    pub fn share_model(&self) -> Option<&ShareModel> {
        Some(&self.remote_project.as_ref()?.room.share.as_ref()?.model)
    }

    /// File › Share Project…, the Share button and the room widget (behaviour 37).
    pub fn open_share_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_mut() else {
            self.status = Some(SHARE_NEEDS_REMOTE.into());
            cx.notify();
            return;
        };
        let own = r
            .live
            .try_with(|l| l.session_ref().info.client.clone())
            .unwrap_or_default();
        let opened = r.room.share.is_none();
        if opened {
            r.room.share = Some(ShareDialog {
                model: ShareModel::new(&own),
                focus: cx.focus_handle(),
                busy: false,
                origin: None,
            });
        }
        if let Some(d) = &r.room.share {
            window.focus(&d.focus);
        }
        if opened {
            self.share_find_origin(cx);
        }
        self.share_refresh(cx);
        cx.notify();
    }

    /// Closes the dialog and forgets the link shown in it (behaviour 39).
    pub fn close_share_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(r) = self.remote_project.as_mut() {
            r.room.share = None;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn share_mut(&mut self) -> Option<&mut ShareDialog> {
        self.remote_project.as_mut()?.room.share.as_mut()
    }

    /// Calls `method` on the backend in the background, then `then` with the answer while the
    /// dialog is still open.
    fn share_call(
        &mut self,
        method: &'static str,
        params: Value,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<Value, RpcError>, &mut Context<Self>) + 'static,
    ) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let live = r.live.clone();
        if let Some(d) = self.share_mut() {
            d.busy = true;
        }
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call(method, params, SHARE_LIMIT) });
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(d) = this.share_mut() else {
                    return;
                };
                d.busy = false;
                then(this, answer, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks for the room's state, and for an admin its links (behaviour 37).
    pub(super) fn share_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        if r.room.share.is_none() {
            return;
        }
        let live = r.live.clone();
        let task = cx.background_executor().spawn(async move {
            let own = live.try_with(|l| l.session_ref().info.client.clone());
            let status = live.host_call("room.status", json!({}), SHARE_LIMIT);
            let admin = status.as_ref().is_ok_and(|s| {
                s["participants"].as_array().is_some_and(|list| {
                    list.iter().any(|p| {
                        own.as_deref().is_some_and(|o| p["client"] == o) && p["role"] == "admin"
                    })
                })
            });
            let links = admin.then(|| live.host_call("room.links", json!({}), SHARE_LIMIT));
            (own, status, links)
        });
        cx.spawn(async move |this, cx| {
            let (own, status, links) = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(d) = this.share_mut() else {
                    return;
                };
                let m = &mut d.model;
                if let Some(own) = own {
                    m.set_own(&own);
                }
                match status {
                    Ok(s) => {
                        m.set_status(&s);
                    }
                    Err(e) if e.code == ErrorCode::NotFound => m.set_no_room(),
                    Err(e) => m.error = Some(reason(&e)),
                }
                match links {
                    Some(Ok(l)) => m.set_links(&l, unix_ms()),
                    Some(Err(e)) => m.error = Some(reason(&e)),
                    None => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Start Room (behaviour 38).
    pub fn share_start_room(&mut self, cx: &mut Context<Self>) {
        let name = self
            .remote_project
            .as_ref()
            .map(|r| {
                r.live
                    .folder()
                    .rsplit(['/', '\\'])
                    .find(|s| !s.is_empty())
                    .unwrap_or("project")
                    .to_owned()
            })
            .unwrap_or_else(|| "project".into());
        self.share_call(
            "room.create",
            json!({ "name": name }),
            cx,
            |this, answer, cx| {
                match answer {
                    Ok(_) => {
                        if let Some(d) = this.share_mut() {
                            d.model.error = None;
                        }
                        this.room_ask_again();
                    }
                    Err(e) => {
                        if let Some(d) = this.share_mut() {
                            d.model.error = Some(reason(&e));
                        }
                    }
                }
                this.share_refresh(cx);
            },
        );
    }

    /// The role chosen for the next link.
    pub fn share_choose_role(&mut self, role: ShareRole, cx: &mut Context<Self>) {
        if let Some(d) = self.share_mut() {
            d.model.choice_role = role;
        }
        cx.notify();
    }

    /// The lifetime chosen for the next link.
    pub fn share_choose_lifetime(&mut self, life: Lifetime, cx: &mut Context<Self>) {
        if let Some(d) = self.share_mut() {
            d.model.choice_life = life;
        }
        cx.notify();
    }

    /// Create Link (behaviour 39): shown once, never copied by itself.
    pub fn share_create_link(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.share_mut() else {
            return;
        };
        if d.busy || !d.model.is_admin() {
            return;
        }
        let params = d.model.share_params();
        let folder = self
            .remote_project
            .as_ref()
            .map(|r| r.live.folder())
            .unwrap_or_default();
        self.share_call("room.share", params, cx, move |this, answer, cx| {
            if let Some(d) = this.share_mut() {
                match answer {
                    Ok(a) => {
                        d.model.set_fresh(&a);
                        // A link that leads to its host (v7 behaviour 44).
                        if let Some(o) = d.origin.clone() {
                            d.model.lead_to(&o.host, o.port, &folder, &o.fingerprint);
                        }
                        d.model.error = None;
                    }
                    Err(e) => d.model.error = Some(reason(&e)),
                }
            }
            this.share_refresh(cx);
        });
    }

    /// Copy: the only way the link reaches the clipboard (behaviour 39).
    pub fn share_copy(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self
            .share_mut()
            .and_then(|d| d.model.fresh().map(|f| f.link.clone()))
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(link));
        if let Some(d) = self.share_mut() {
            d.model.note = Some("Copied the link to the clipboard".into());
        }
        cx.notify();
    }

    /// Revoke at a link in force (behaviour 40).
    pub fn share_revoke(&mut self, id: String, cx: &mut Context<Self>) {
        self.share_call(
            "room.revoke_link",
            json!({ "id": id }),
            cx,
            |this, answer, cx| {
                if let (Err(e), Some(d)) = (answer, this.share_mut()) {
                    d.model.error = Some(reason(&e));
                }
                this.share_refresh(cx);
            },
        );
    }

    /// The first click on Remove: the row asks (behaviour 41).
    pub fn share_ask_remove(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(d) = self.share_mut() {
            d.model.ask_remove(id);
        }
        cx.notify();
    }

    /// Keep in the row.
    pub fn share_keep(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.share_mut() {
            d.model.keep();
        }
        cx.notify();
    }

    /// Remove in the row: the person leaves and cannot come back through a link.
    pub fn share_confirm_remove(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(person) = self.share_mut().and_then(|d| d.model.confirm_remove(id)) else {
            return;
        };
        self.share_call(
            "room.remove",
            json!({ "person": person }),
            cx,
            |this, answer, cx| {
                if let (Err(e), Some(d)) = (answer, this.share_mut()) {
                    d.model.error = Some(reason(&e));
                }
                this.share_refresh(cx);
            },
        );
    }

    fn share_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.close_share_dialog(window, cx);
            cx.stop_propagation();
        }
    }

    /// The Share button at the left of the avatars, on every remote project (behaviour 37).
    pub(super) fn share_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.remote_project.as_ref()?;
        let p = Palette::current();
        Some(
            div()
                .id("room-share-button")
                .h(px(22.))
                .px(px(7.))
                .mr(px(4.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .hover(|s| s.bg(c(p.toolbar_hover)))
                .children(crate::title_bar::control())
                .tip_text(
                    "room-share-button",
                    ferris_idea_ui::tooltip::Tip::plain(
                        "Share this project: start a room, make links, see who is here",
                    ),
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_share_dialog(window, cx)))
                .child("Share")
                .into_any_element(),
        )
    }

    /// The room widget of the status bar (behaviour 43).
    pub(super) fn room_widget(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.remote_project.as_ref()?;
        let view = r.room.view.as_ref()?;
        let w = Widget::of(view, connection_of(&r.live.state()));
        let p = Palette::current();
        let dot = match w.connection {
            Connection::Connected => p.success_text,
            Connection::Reconnecting => p.accent,
            Connection::Offline => p.error_text,
        };
        Some(
            div()
                .id("room-state-widget")
                .tip_text(
                    "room-state-widget",
                    ferris_idea_ui::tooltip::Tip {
                        title: w.title.clone(),
                        shortcut: String::new(),
                        details: w.lines.clone(),
                    },
                )
                .map(status_widget)
                .on_click(cx.listener(|this, _, window, cx| this.open_share_dialog(window, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().size(px(7.)).rounded_full().bg(c(dot)))
                        .child(w.text),
                )
                .into_any_element(),
        )
    }

    /// The dialog itself, drawn above the window (behaviours 37–42).
    pub(super) fn share_dialog_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.remote_project.as_ref()?;
        let d = r.room.share.as_ref()?;
        let m = &d.model;
        let p = Palette::current();
        let now = unix_ms();
        let host = r.live.host();
        let mut body = div()
            .id("room-share-dialog")
            .track_focus(&d.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::share_key))
            .w(px(600.))
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
                    .child(format!("Share {} on {host}", r.live.folder())),
            )
            .child(
                div()
                    .id("room-share-summary")
                    .text_color(c(p.ui_muted))
                    .child(m.summary()),
            )
            .when_some(m.error.clone(), |el, e| {
                el.child(
                    div()
                        .id("room-share-error")
                        .text_color(c(p.error_text))
                        .child(e),
                )
            });
        if m.known() && m.room_name().is_none() {
            body = body.child(
                div().flex().child(
                    button("room-share-start", "Start Room", true)
                        .on_click(cx.listener(|this, _, _, cx| this.share_start_room(cx))),
                ),
            );
        }
        if m.is_admin() {
            let mut roles = div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().w(px(70.)).text_color(c(p.ui_muted)).child("Role"));
            for role in ShareRole::ALL {
                let id = match role {
                    ShareRole::Viewer => "room-share-role-viewer",
                    ShareRole::Editor => "room-share-role-editor",
                };
                roles =
                    roles.child(button(id, role.label(), m.choice_role == role).on_click(
                        cx.listener(move |this, _, _, cx| this.share_choose_role(role, cx)),
                    ));
            }
            let mut lives = div().flex().items_center().gap_1().child(
                div()
                    .w(px(70.))
                    .text_color(c(p.ui_muted))
                    .child("Works for"),
            );
            for life in Lifetime::ALL {
                let id = match life {
                    Lifetime::Hour => "room-share-life-hour",
                    Lifetime::Day => "room-share-life-day",
                    Lifetime::Week => "room-share-life-week",
                    Lifetime::Month => "room-share-life-month",
                };
                lives = lives.child(button(id, life.label(), m.choice_life == life).on_click(
                    cx.listener(move |this, _, _, cx| this.share_choose_lifetime(life, cx)),
                ));
            }
            body = body
                .child(
                    div()
                        .mt_1()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("New link"),
                )
                .child(roles)
                .child(lives)
                .child(
                    div().flex().child(
                        button("room-share-create", "Create Link", m.fresh().is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.share_create_link(cx))),
                    ),
                );
            if let Some(f) = m.fresh() {
                body = body
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                crate::metrics::field_frame()
                                    .id("room-share-link")
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_xs()
                                    .child(f.link.clone()),
                            )
                            .child(
                                button("room-share-copy", "Copy", true)
                                    .on_click(cx.listener(|this, _, _, cx| this.share_copy(cx))),
                            ),
                    )
                    .child(div().text_xs().text_color(c(p.ui_muted)).child(format!(
                        "{} link, {}. Shown only now. Whoever gets it also needs an account on {host} or a paired device of it.",
                        f.role,
                        remaining(f.expires, now)
                    )));
            }
            if let Some(note) = &m.note {
                body = body.child(
                    div()
                        .id("room-share-note")
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(note.clone()),
                );
            }
            body = body.child(
                div()
                    .mt_1()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Links in force"),
            );
            if m.links().is_empty() {
                body = body.child(div().text_color(c(p.ui_dim)).child("No link works now"));
            }
            for l in m.links() {
                let id = l.id.clone();
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().child(format!(
                            "{} · {}",
                            l.role,
                            remaining(l.expires, now)
                        )))
                        .child(
                            row_button(format!("room-share-revoke-{}", l.id), "Revoke", false)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.share_revoke(id.clone(), cx)
                                })),
                        ),
                );
            }
        }
        if m.room_name().is_some() {
            body = body.child(
                div()
                    .mt_1()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("People"),
            );
            for person in m.people() {
                let mut row = div().flex().items_center().gap_2().h(px(26.)).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(person.name.clone())
                        .child(
                            div()
                                .ml_2()
                                .text_color(c(p.ui_muted))
                                .text_xs()
                                .child(person.detail()),
                        )
                        .flex()
                        .items_center(),
                );
                if let (true, Some(id)) = (person.removable, person.id.clone()) {
                    let key = crate::room_share_model::key_of(&id);
                    if m.confirming() == Some(id.as_str()) {
                        let confirm = id.clone();
                        row = row
                            .child(
                                div()
                                    .text_color(c(p.error_text))
                                    .child(format!("Remove {}?", person.name)),
                            )
                            .child(
                                row_button(format!("room-share-remove-yes-{key}"), "Remove", true)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.share_confirm_remove(&confirm, cx)
                                    })),
                            )
                            .child(
                                row_button(format!("room-share-remove-keep-{key}"), "Keep", false)
                                    .on_click(cx.listener(|this, _, _, cx| this.share_keep(cx))),
                            );
                    } else {
                        row = row.child(
                            row_button(format!("room-share-remove-{key}"), "Remove", false)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.share_ask_remove(&id, cx)
                                })),
                        );
                    }
                }
                body = body.child(row);
            }
        }
        // The room's history, reachable from the room widget's dialog (v9 behaviour 61).
        let history = m.room_name().is_some().then(|| {
            button("room-share-history", "History", false)
                .on_click(cx.listener(|this, _, window, cx| this.open_room_history(window, cx)))
        });
        body =
            body.child(div().flex().children(history).child(div().flex_1()).child(
                button("room-share-close", "Close", false).on_click(
                    cx.listener(|this, _, window, cx| this.close_share_dialog(window, cx)),
                ),
            ));
        Some(body.into_any_element())
    }
}
