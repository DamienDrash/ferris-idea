//! Joining a project room by link from the window (`SPEC-0323` v7 behaviours 44–52): the Join
//! dialog (paste, see the host before connecting, check its key against the user's own
//! `known_hosts`, join), the path agents take through `room_join_link`, and the full link the
//! Share dialog makes for a project opened over SSH. The decisions are in
//! [`crate::room_join_model`]; this module reads `known_hosts`, connects and draws. The link's
//! secret lives only in the open dialog and in the one `room.join` call.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ferris_idea_remote::known_hosts::{self, KnownHostsLookup};
use ferris_idea_remote::{
    HostKeyCheck, HostSpec, LiveProject, ProjectLink, RemoteError, RoomLink, SshTarget, Timing,
};
use gpui::{
    AnyElement, App, Context, FocusHandle, Global, KeyDownEvent, Window, div, prelude::*, px,
};
use serde_json::{Value, json};

use super::remote_project::{open_link, open_live_window, recent_file, recovery_dir, state_dir};
use super::{WorkbenchView, c};
use crate::palette::Palette;
use crate::room_join_model::{ACCOUNT_NOTE, JoinFailure, JoinModel, TRUST_NOTE};
use crate::semantics::{Annotated, AxAction, Role, Semantics};

/// How long `room.join` may take.
const JOIN_LIMIT: Duration = Duration::from_secs(30);

/// Where the host keys the user trusts are read (behaviour 49). The product reads them the way
/// `ssh` does; tests name their own files and never read `~/.ssh`. Without it the window reads
/// no host keys: the Share dialog keeps the short link and joining by link is refused.
#[derive(Clone, Debug)]
pub enum HostKeySource {
    /// The `known_hosts` files `ssh -G` names for the host.
    System,
    /// These files only.
    Files(Vec<PathBuf>),
}

impl Global for HostKeySource {}

/// Connects a link's project instead of SSH (tests: an in-process backend). Gets the link and
/// the extra `ssh` arguments the connection would use.
pub type JoinOpen =
    Arc<dyn Fn(&RoomLink, &[String]) -> Result<Box<dyn ProjectLink>, RemoteError> + Send + Sync>;

/// The connector of [`JoinOpen`] when set (tests).
#[derive(Clone)]
pub struct JoinOpener(pub JoinOpen);

impl Global for JoinOpener {}

/// The open Join dialog.
pub(crate) struct JoinDialog {
    /// What it shows; the pasted link lives only here.
    pub model: JoinModel,
    focus: FocusHandle,
}

/// What the Share dialog knows of its host to make a full link (behaviour 44).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkOrigin {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
}

/// Unix seconds now.
fn unix_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The label of a link's host as a remote project: `host`, or `host:port` off port 22.
pub fn link_label(link: &RoomLink) -> String {
    if link.port == 22 {
        link.host.clone()
    } else {
        format!("{}:{}", link.host, link.port)
    }
}

/// Where the keys of `host` and `port` are looked up, or `None` without a source.
fn lookup(
    source: Option<&HostKeySource>,
    host: &str,
    port: Option<u16>,
    extra: &[String],
) -> Option<KnownHostsLookup> {
    match source? {
        HostKeySource::System => Some(known_hosts::system_lookup(host, port, extra)),
        HostKeySource::Files(files) => {
            Some(known_hosts::files_lookup(host, port.unwrap_or(22), files))
        }
    }
}

/// The host key check of a link (behaviour 49); `None` without a source of host keys.
pub fn check_link(
    source: Option<&HostKeySource>,
    link: &RoomLink,
    extra: &[String],
) -> Option<HostKeyCheck> {
    let l = lookup(source, &link.host, Some(link.port), extra)?;
    Some(known_hosts::check(
        &known_hosts::known_keys(&l),
        &link.fingerprint,
    ))
}

/// What a project of `label` (as the window opened it) gives a link: the host `ssh` reaches,
/// its port and the preferred trusted key (behaviour 44); `None` for a host that is not SSH, or
/// without a trusted key.
fn origin_of(source: Option<&HostKeySource>, label: &str, extra: &[String]) -> Option<LinkOrigin> {
    if !HostSpec::parse(label).ok()?.is_ssh() {
        return None;
    }
    let target = SshTarget::parse(label, extra);
    let host = target
        .host
        .rsplit_once('@')
        .map_or(target.host.as_str(), |(_, h)| h)
        .to_owned();
    let l = lookup(source, &host, target.port, extra)?;
    let keys = known_hosts::known_keys(&l);
    let key = known_hosts::preferred(&keys)?;
    ferris_idea_remote::room_link::valid_host(&l.hostname).then(|| LinkOrigin {
        host: l.hostname.clone(),
        port: l.port,
        fingerprint: key.fingerprint.clone(),
    })
}

impl WorkbenchView {
    /// The Join dialog's model while it is open (window tests).
    pub fn join_model(&self) -> Option<&JoinModel> {
        self.room_join.as_ref().map(|d| &d.model)
    }

    /// File › Join Project by Link… (behaviour 48), in any project window.
    pub fn open_join_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.room_join.is_none() {
            self.room_join = Some(JoinDialog {
                model: JoinModel::default(),
                focus: cx.focus_handle(),
            });
        }
        if let Some(d) = &self.room_join {
            window.focus(&d.focus);
        }
        cx.notify();
    }

    /// Cancel and Escape: the link is forgotten (behaviour 47).
    pub fn close_join_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.room_join = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Paste: the link from the clipboard, read and checked (behaviours 46 and 49).
    pub fn join_paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return;
        };
        let Some(d) = self.room_join.as_mut() else {
            return;
        };
        if d.model.paste(&text, unix_s()) {
            self.join_check(cx);
        }
        cx.notify();
    }

    /// Clear: forgets the link.
    pub fn join_clear(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.room_join.as_mut() {
            d.model.clear();
        }
        cx.notify();
    }

    /// Check Again: looks up the host key in `known_hosts` once more (behaviour 49).
    pub fn join_check(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self
            .room_join
            .as_mut()
            .and_then(|d| d.model.link().cloned())
        else {
            return;
        };
        if let Some(d) = self.room_join.as_mut() {
            d.model.busy = true;
        }
        let source = cx.try_global::<HostKeySource>().cloned();
        let extra = self.remote_extra.clone();
        let task = cx
            .background_executor()
            .spawn(async move { check_link(source.as_ref(), &link, &extra) });
        cx.spawn(async move |this, cx| {
            let check = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(d) = this.room_join.as_mut() else {
                    return;
                };
                d.model.busy = false;
                match check {
                    Some(check) => d.model.set_check(check),
                    None => d.model.fail(
                        JoinFailure::Other(
                            "Ferris IDEA cannot read your known_hosts here, so it cannot check the host"
                                .into(),
                        ),
                        unix_s(),
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Join (behaviour 50): only after the host key matched.
    pub fn join_go(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.room_join.as_mut() else {
            return;
        };
        if !d.model.can_join() {
            return;
        }
        let Some(link) = d.model.link().cloned() else {
            return;
        };
        d.model.busy = true;
        d.model.error = None;
        cx.notify();
        self.join_link_then(
            link,
            |this, result, cx| {
                match result {
                    Ok(answer) => {
                        this.status = Some(answer["joined"].as_str().unwrap_or("Joined").into());
                        this.room_join = None;
                    }
                    Err(f) => {
                        if let Some(d) = this.room_join.as_mut() {
                            d.model.fail(f, unix_s());
                        }
                    }
                }
                cx.notify();
            },
            cx,
        );
    }

    /// Joins the room of `link` (behaviours 49–51): checks the host key again, connects with the
    /// host key algorithms narrowed to the matching key, calls `room.join` before a window opens,
    /// and opens the remote project window only when the room admitted the session. `done` gets
    /// the room's answer with `joined` (a status line without the secret), or the failure.
    pub fn join_link_then(
        &mut self,
        link: RoomLink,
        done: impl FnOnce(&mut Self, Result<Value, JoinFailure>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if let Some(end) = link.expires
            && end <= unix_s()
        {
            done(self, Err(JoinFailure::Expired(end)), cx);
            return;
        }
        let source = cx.try_global::<HostKeySource>().cloned();
        let opener = cx.try_global::<JoinOpener>().map(|o| o.0.clone());
        let base = state_dir(cx);
        let backend_dir = self.remote_backend_dir.clone();
        let label = link_label(&link);
        let mut extra = self.remote_extra.clone();
        let already = ferris_idea_remote::live::find(&label, &link.folder);
        let task = cx.background_executor().spawn(async move {
            let key_type = match check_link(source.as_ref(), &link, &extra) {
                Some(HostKeyCheck::Matches { key_type }) => key_type,
                Some(HostKeyCheck::Different { .. }) => return Err(JoinFailure::WrongHostKey),
                Some(HostKeyCheck::Unknown) => return Err(JoinFailure::UnknownHostKey),
                None => {
                    return Err(JoinFailure::Other(
                        "Ferris IDEA cannot read your known_hosts here, so it cannot check the host"
                            .into(),
                    ));
                }
            };
            let (live, fresh) = match already {
                Some(live) => (live, false),
                None => {
                    extra.push("-o".into());
                    extra.push(format!(
                        "HostKeyAlgorithms={}",
                        known_hosts::host_key_algorithms(&key_type)
                    ));
                    let project = match &opener {
                        Some(open) => open(&link, &extra),
                        None => {
                            let recovery = base.as_deref().map(recovery_dir).ok_or_else(|| {
                                JoinFailure::Other("no settings folder for remote projects".into())
                            })?;
                            let opts = ferris_idea_remote::ServeOptions {
                                backend_dir,
                                ..ferris_idea_remote::ServeOptions::default()
                            };
                            open_link(
                                HostSpec::Ssh(label.clone()),
                                &link.folder,
                                opts,
                                recovery,
                                &extra,
                            )
                        }
                    }
                    .map_err(|e| JoinFailure::from_remote(&e))?;
                    (LiveProject::start(project, Timing::default()), true)
                }
            };
            let end = |live: &LiveProject| {
                if fresh {
                    let live = live.clone();
                    std::thread::spawn(move || {
                        let _ = live.close(ferris_idea_remote::Unsaved::Discard);
                    });
                }
            };
            let answer =
                match live.host_call("room.join", json!({ "link": link.text() }), JOIN_LIMIT) {
                    Ok(a) => a,
                    Err(e) => {
                        end(&live);
                        return Err(JoinFailure::from_join(&e));
                    }
                };
            if answer["id"].as_str() != Some(link.room.as_str()) {
                let _ = live.host_call("room.leave", json!({}), JOIN_LIMIT);
                end(&live);
                return Err(JoinFailure::OtherRoom);
            }
            Ok((live, answer, link, label))
        });
        cx.spawn(async move |this, cx| {
            let joined = task.await;
            let _ = this.update(cx, |this, cx| match joined {
                Ok((live, mut answer, link, label)) => {
                    if let Some(base) = state_dir(cx) {
                        let _ = ferris_idea_remote::remember_remote(
                            &recent_file(&base),
                            &label,
                            &link.folder,
                        );
                    }
                    let role = answer["role"].as_str().unwrap_or(&link.role).to_owned();
                    let shown = open_live_window(live.clone(), cx);
                    answer["joined"] = json!(format!(
                        "Joined the room of {} on {label} as {role}",
                        link.folder
                    ));
                    answer["host"] = json!(label);
                    answer["folder"] = json!(link.folder);
                    match shown {
                        Ok(_) => done(this, Ok(answer), cx),
                        Err(e) => done(
                            this,
                            Err(JoinFailure::Other(format!("Cannot open the window: {e}"))),
                            cx,
                        ),
                    }
                }
                Err(f) => done(this, Err(f), cx),
            });
        })
        .detach();
    }

    /// Joins by `link` for an agent in the front window (`room_join_link`, behaviour 52); `done`
    /// gets the room's answer or the failure's text.
    pub fn join_room_by_link_in_front(
        link: String,
        done: impl FnOnce(Result<Value, String>, &mut App) + 'static,
        cx: &mut App,
    ) {
        let now = unix_s();
        let link = match RoomLink::parse(&link, now) {
            Ok(l) => l,
            Err(ferris_idea_remote::LinkError::Expired(end)) => {
                done(Err(JoinFailure::Expired(end).text("", 0, now)), cx);
                return;
            }
            Err(e) => {
                done(Err(e.to_string()), cx);
                return;
            }
        };
        let Some(bench) = crate::project_windows::workbenches(cx).into_iter().next() else {
            done(Err("no project window is open".into()), cx);
            return;
        };
        let (host, port) = (link.host.clone(), link.port);
        bench.update(cx, |b, cx| {
            b.join_link_then(
                link,
                move |_, r, cx| done(r.map_err(|f| f.text(&host, port, unix_s())), cx),
                cx,
            )
        });
    }

    /// Works out in the background what a link of this window's project carries of its host,
    /// for the Share dialog (behaviour 44).
    pub(super) fn share_find_origin(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let Some(source) = cx.try_global::<HostKeySource>().cloned() else {
            return;
        };
        let label = r.live.host();
        let extra = self.remote_extra.clone();
        let task = cx
            .background_executor()
            .spawn(async move { origin_of(Some(&source), &label, &extra) });
        cx.spawn(async move |this, cx| {
            let origin = task.await;
            let _ = this.update(cx, |this, cx| {
                let folder = this
                    .remote_project
                    .as_ref()
                    .map(|r| r.live.folder())
                    .unwrap_or_default();
                if let Some(d) = this
                    .remote_project
                    .as_mut()
                    .and_then(|r| r.room.share.as_mut())
                {
                    d.origin = origin.clone();
                    if let Some(o) = origin {
                        d.model.lead_to(&o.host, o.port, &folder, &o.fingerprint);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn join_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.key == "escape" {
            self.close_join_dialog(window, cx);
            cx.stop_propagation();
        } else if k.key == "v" && (k.modifiers.platform || k.modifiers.control) {
            self.join_paste(cx);
            cx.stop_propagation();
        } else if k.key == "enter" {
            self.join_go(cx);
            cx.stop_propagation();
        }
    }

    /// The dialog itself, drawn above the window (behaviours 48–51).
    pub(super) fn join_dialog_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let d = self.room_join.as_ref()?;
        let m = &d.model;
        let p = Palette::current();
        let now = unix_s();
        let button = |id: &'static str, label: &'static str, primary: bool| {
            crate::metrics::button(id, label, primary)
                .relative()
                .semantics(
                    Semantics::new(Role::Button)
                        .name(label)
                        .test_id(id)
                        .action(AxAction::Click),
                )
        };
        let mut body = div()
            .id("room-join-dialog")
            .track_focus(&d.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::join_key))
            .w(px(620.))
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
                    .child("Join Project by Link"),
            );
        if m.link().is_none() {
            body = body.child(
                div()
                    .text_color(c(p.ui_muted))
                    .child("Paste a room link you got from an admin of the room (⌘V)."),
            );
        }
        for (label, value) in m.rows(now) {
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().w(px(80.)).text_color(c(p.ui_muted)).child(label))
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("room-join-row-{label}")))
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(value),
                    ),
            );
        }
        if let Some((line, good)) = m.check_line() {
            body = body.child(
                div()
                    .id("room-join-check-line")
                    .text_color(c(if good { p.success_text } else { p.error_text }))
                    .child(line),
            );
        }
        if let Some(e) = m.error.clone()
            && m.check_line().is_none_or(|(l, _)| l != e)
        {
            body = body.child(
                div()
                    .id("room-join-error")
                    .text_color(c(p.error_text))
                    .child(e),
            );
        }
        for step in m.steps() {
            body = body.child(div().text_xs().font_family("JetBrains Mono").child(step));
        }
        if m.link().is_some() {
            body = body
                .child(
                    div()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(ACCOUNT_NOTE),
                )
                .child(div().text_xs().text_color(c(p.ui_muted)).child(TRUST_NOTE));
        }
        let mut buttons = div().flex().gap_2().child(
            button("room-join-paste", "Paste", m.link().is_none())
                .on_click(cx.listener(|this, _, _, cx| this.join_paste(cx))),
        );
        if m.link().is_some() {
            buttons = buttons.child(
                button("room-join-clear", "Clear", false)
                    .on_click(cx.listener(|this, _, _, cx| this.join_clear(cx))),
            );
            if !m.can_join() && !m.busy {
                buttons = buttons.child(
                    button("room-join-check", "Check Again", false)
                        .on_click(cx.listener(|this, _, _, cx| this.join_check(cx))),
                );
            }
        }
        buttons = buttons.child(div().flex_1());
        if m.busy {
            buttons = buttons.child(div().text_color(c(p.ui_muted)).child("Working…"));
        }
        if m.can_join() {
            buttons = buttons.child(
                button("room-join-go", "Join", true)
                    .on_click(cx.listener(|this, _, _, cx| this.join_go(cx))),
            );
        }
        buttons = buttons.child(
            button("room-join-cancel", "Cancel", false)
                .on_click(cx.listener(|this, _, window, cx| this.close_join_dialog(window, cx))),
        );
        Some(body.child(buttons).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_carry_the_port_off_22() {
        let fp = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
        let short = format!("ferris-idea://room/ab#{}", "0".repeat(64));
        let l = RoomLink::compose(&short, "h.test", 22, "/p", fp, "viewer", None).unwrap();
        assert_eq!(link_label(&l), "h.test");
        let l = RoomLink::compose(&short, "h.test", 2222, "/p", fp, "viewer", None).unwrap();
        assert_eq!(link_label(&l), "h.test:2222");
        assert_eq!(check_link(None, &l, &[]), None, "no source, no check");
    }
}
