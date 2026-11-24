//! The project room in a remote project's window (`SPEC-0323` v5 behaviours 30–36): the other
//! participants as avatars in the main toolbar, their carets in the editors, following one of
//! them, the window's own place sent to the room, and what agents see and ask through
//! `room_presence` and `room_follow`. The decisions are in [`crate::room_model`]; this module
//! feeds it and carries out what it says.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ferris_idea_session::ErrorCode;
use gpui::{AnyElement, AnyWindowHandle, Context, Window, div, prelude::*, px};
use serde_json::{Value, json};

use super::{WorkbenchView, c};
use crate::palette::Palette;
use crate::room_model::{Go, Note, Presence, RemoteCaret, RoomView};
use crate::tips::Tipped;

/// How often a window without a room asks again whether one was made (behaviour 30).
const ASK_AGAIN: Duration = Duration::from_secs(5);
/// How long a room request may take.
const ROOM_LIMIT: Duration = Duration::from_secs(5);

/// Milliseconds on a clock of this process.
fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// What a remote window keeps of its room.
#[derive(Default)]
pub(crate) struct RoomState {
    /// The room as the window sees it; `None` without a room (or without a place in it).
    pub view: Option<RoomView>,
    /// The connection generation the window last asked for the room's state in.
    asked_generation: Option<u64>,
    asking: bool,
    last_ask: Option<Instant>,
    /// Per editor: presses, caret and revision at the last look (behaviour 35).
    seen: HashMap<PathBuf, (u64, usize, u64)>,
    /// The window, for following when a notification arrives between frames.
    pub window: Option<AnyWindowHandle>,
    /// What was last published for agents.
    published: Option<Value>,
    /// The Share dialog while it is open (`SPEC-0323` v6 behaviour 37).
    pub share: Option<super::room_share::ShareDialog>,
    /// The Room History dialog while it is open (`SPEC-0323` v9 behaviour 61).
    pub history: Option<super::room_history::HistoryDialog>,
    /// The room's comments (`SPEC-0323` v8).
    pub comments: super::room_comments::CommentsState,
}

impl WorkbenchView {
    /// The window this remote project is shown in (set once it is open).
    pub(crate) fn set_room_window(&mut self, handle: AnyWindowHandle) {
        if let Some(r) = self.remote_project.as_mut() {
            r.room.window = Some(handle);
        }
    }

    /// Asks the room's state again on the next look (after the room was made).
    pub(super) fn room_ask_again(&mut self) {
        if let Some(r) = self.remote_project.as_mut() {
            r.room.asked_generation = None;
            r.room.last_ask = None;
        }
    }

    /// The room as this window sees it (self-test, window tests).
    pub fn room_view(&self) -> Option<&RoomView> {
        self.remote_project.as_ref()?.room.view.as_ref()
    }

    /// One look at the room, with the window's look at its project.
    pub(super) fn room_tick(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let generation = r.live.generation();
        let connected = matches!(
            r.live.state(),
            ferris_idea_remote::ConnState::Connected { .. }
        );
        let due = connected
            && !r.room.asking
            && (r.room.asked_generation != Some(generation)
                || (r.room.view.is_none()
                    && r.room.last_ask.is_none_or(|t| t.elapsed() >= ASK_AGAIN)));
        if due {
            r.room.asking = true;
            r.room.asked_generation = Some(generation);
            r.room.last_ask = Some(Instant::now());
            let live = r.live.clone();
            let task = cx.background_executor().spawn(async move {
                let own = live.try_with(|l| l.session_ref().info.client.clone());
                (own, live.host_call("room.status", json!({}), ROOM_LIMIT))
            });
            cx.spawn(async move |this, cx| {
                let (own, status) = task.await;
                let _ = this.update(cx, |this, cx| this.room_status_arrived(own, status, cx));
            })
            .detach();
        }
        self.room_asks(cx);
        self.room_own_activity(cx);
        self.room_send_presence(cx);
        self.room_show_carets(cx);
        self.room_comments_tick(cx);
        self.room_history_tick(cx);
        self.room_publish();
    }

    fn room_status_arrived(
        &mut self,
        own: Option<String>,
        status: Result<Value, ferris_idea_session::RpcError>,
        cx: &mut Context<Self>,
    ) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        r.room.asking = false;
        let now = now_ms();
        match (status, own) {
            (Ok(status), Some(own)) => match r.room.view.as_mut() {
                Some(view) => {
                    if let Some(note) = view.reset(&own, &status, now) {
                        self.status = Some(note.text());
                    }
                }
                None => r.room.view = Some(RoomView::from_status(&own, &status, now)),
            },
            // Asked again on the next look.
            (Ok(_), None) => r.room.asked_generation = None,
            (Err(e), _) if e.code == ErrorCode::Unavailable => r.room.asked_generation = None,
            // No room, or no place in it: nothing is shown.
            (Err(_), _) => {
                if let Some(note) = r.room.view.take().and_then(|mut v| v.stop()) {
                    self.status = Some(note.text());
                }
            }
        }
        cx.notify();
    }

    /// A notification of the room (`room.joined`, `room.presence`, `room.left`).
    pub(super) fn room_event(&mut self, method: &str, params: &Value, cx: &mut Context<Self>) {
        let Some(view) = self
            .remote_project
            .as_mut()
            .and_then(|r| r.room.view.as_mut())
        else {
            return;
        };
        let (note, go) = view.event(method, params, now_ms());
        if let Some(note) = note {
            self.status = Some(note.text());
        }
        if let Some(go) = go {
            self.room_go_later(go, cx);
        }
        // The Share dialog's list follows people coming and going (v6 behaviour 37).
        if matches!(method, "room.joined" | "room.left") {
            self.share_refresh(cx);
        }
        cx.notify();
    }

    /// Follows `go` in the window as soon as it can.
    fn room_go_later(&mut self, go: Go, cx: &mut Context<Self>) {
        let Some(handle) = self.remote_project.as_ref().and_then(|r| r.room.window) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| this.room_go(go, window, cx));
            });
        })
        .detach();
    }

    /// Opens the followed person's document at their caret (behaviour 34).
    fn room_go(&mut self, go: Go, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        // Checked twice: the model refuses a path leaving the project, and the window too.
        if !crate::room_model::safe_doc(&go.doc)
            || ferris_idea_session::paths::check_relative(&go.doc).is_err()
        {
            return;
        }
        let path = r.stand_in.join(&go.doc);
        match self.editors.get(&path).cloned() {
            Some(editor) => {
                self.open(&path, window, cx);
                editor.update(cx, |e, cx| e.select_range(go.caret..go.caret, cx));
                let seen = editor.read_with(cx, |e, _| {
                    (e.input_count(), e.state.carets.primary().head, e.revision())
                });
                if let Some(r) = self.remote_project.as_mut() {
                    r.room.seen.insert(path, seen);
                }
            }
            None => {
                self.remote_pend(
                    &path,
                    super::remote_host::PendingPlace::Select(go.caret..go.caret),
                );
                self.open(&path, window, cx);
            }
        }
        // Opening a file replaces the status note; the hint stays while following.
        if let Some(name) = self
            .room_view()
            .and_then(|v| Some(v.participant(v.following()?)?.shown_name()))
        {
            self.status = Some(Note::Following(name).text());
        }
        cx.notify();
    }

    /// A click on the avatar of `client` (behaviour 34): follow, or stop following.
    pub fn room_avatar_clicked(
        &mut self,
        client: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self
            .remote_project
            .as_mut()
            .and_then(|r| r.room.view.as_mut())
        else {
            return;
        };
        match view.toggle_follow(client, now_ms()) {
            Ok((note, go)) => {
                self.status = Some(note.text());
                self.room_reset_seen(cx);
                if let Some(go) = go {
                    self.room_go(go, window, cx);
                }
            }
            Err(e) => self.status = Some(e.text()),
        }
        cx.notify();
    }

    /// Remembers every editor's presses, caret and revision as they are now.
    fn room_reset_seen(&mut self, cx: &mut Context<Self>) {
        let seen: HashMap<PathBuf, (u64, usize, u64)> = self
            .editors
            .iter()
            .map(|(p, e)| {
                let e = e.read(cx);
                (
                    p.clone(),
                    (e.input_count(), e.state.carets.primary().head, e.revision()),
                )
            })
            .collect();
        if let Some(r) = self.remote_project.as_mut() {
            r.room.seen = seen;
        }
    }

    /// Requests of agents (`room_follow`, behaviour 36), answered with the window's state.
    fn room_asks(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let live = r.live.clone();
        for (id, kind, value) in live.take_window_asks() {
            let Some(view) = self
                .remote_project
                .as_mut()
                .and_then(|r| r.room.view.as_mut())
            else {
                live.answer_window_ask(
                    id,
                    json!({"error": "the window is in no room of this project"}),
                );
                continue;
            };
            if kind != "room_follow" {
                live.answer_window_ask(id, json!({"error": format!("unknown request {kind}")}));
                continue;
            }
            let answer = match value["client"].as_str() {
                None => Ok((view.stop(), None)),
                Some(client) => view
                    .start_follow(client, now_ms())
                    .map(|(note, go)| (Some(note), go))
                    .map_err(|e| e.text()),
            };
            match answer {
                Ok((note, go)) => {
                    live.answer_window_ask(id, view.to_json());
                    if let Some(note) = note {
                        self.status = Some(note.text());
                    }
                    self.room_reset_seen(cx);
                    if let Some(go) = go {
                        self.room_go_later(go, cx);
                    }
                }
                Err(e) => live.answer_window_ask(id, json!({ "error": e })),
            }
            cx.notify();
        }
    }

    /// Whether the user moved on their own (behaviour 35): a press in an editor that changed its
    /// caret or text, or another document in front.
    fn room_own_activity(&mut self, cx: &mut Context<Self>) {
        let following = self.room_view().and_then(RoomView::following).is_some();
        let now: HashMap<PathBuf, (u64, usize, u64)> = self
            .editors
            .iter()
            .map(|(p, e)| {
                let e = e.read(cx);
                (
                    p.clone(),
                    (e.input_count(), e.state.carets.primary().head, e.revision()),
                )
            })
            .collect();
        let active = self.active_path().and_then(|p| self.remote_rel(&p));
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let input = now.iter().any(|(p, (presses, caret, rev))| {
            r.room
                .seen
                .get(p)
                .is_some_and(|(p0, c0, r0)| presses > p0 && (caret != c0 || rev != r0))
        });
        r.room.seen = now;
        if !following {
            return;
        }
        let Some(view) = r.room.view.as_mut() else {
            return;
        };
        if let Some(note) = view.own_activity(input, active.as_deref(), now_ms()) {
            self.status = Some(note.text());
            cx.notify();
        }
    }

    /// Sends the window's own place when it changed (behaviour 33).
    fn room_send_presence(&mut self, cx: &mut Context<Self>) {
        if self.room_view().is_none() {
            return;
        }
        let place = match self.active_path() {
            Some(path) => match (self.remote_rel(&path), self.editors.get(&path)) {
                (Some(rel), Some(e)) => {
                    let c = e.read(cx).state.carets.primary();
                    own_place(Some(rel), c.head, c.anchor)
                }
                _ => Presence::default(),
            },
            None => Presence::default(),
        };
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let Some(params) = r
            .room
            .view
            .as_mut()
            .and_then(|v| v.presence_due(place, now_ms()))
        else {
            return;
        };
        let live = r.live.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = live.host_call("room.presence", params, ROOM_LIMIT);
            })
            .detach();
    }

    /// Hands every editor the carets of the others in its document (behaviour 32).
    fn room_show_carets(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        let editors: Vec<(PathBuf, gpui::Entity<crate::EditorView>)> = self
            .editors
            .iter()
            .map(|(p, e)| (p.clone(), e.clone()))
            .collect();
        for (path, editor) in editors {
            let carets: Vec<RemoteCaret> = match (self.room_view(), self.remote_rel(&path)) {
                (Some(view), Some(rel)) => {
                    let len = editor.read(cx).state.buffer.len_chars();
                    view.carets_in(&rel, len, now)
                }
                _ => Vec::new(),
            };
            editor.update(cx, |e, cx| e.set_remote_carets(carets, cx));
        }
    }

    /// What agents see of the room in this window (`room_presence`, behaviour 36).
    fn room_publish(&mut self) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let part = match &r.room.view {
            Some(v) => {
                let mut part = v.to_json();
                // What the room widget shows (v6 behaviour 43); never a link of the dialog.
                let connection = super::room_share::connection_of(&r.live.state());
                part["widget"] = crate::room_share_model::Widget::of(v, connection).to_json();
                part
            }
            None => Value::Null,
        };
        if r.room.published.as_ref() != Some(&part) {
            r.live.set_window_part("room", part.clone());
            r.room.published = Some(part);
        }
    }

    /// The avatars of the room's other participants for the main toolbar (behaviour 31).
    pub(super) fn room_avatars(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.room_view()?;
        let avatars = view.avatars();
        if avatars.shown.is_empty() {
            return None;
        }
        let p = Palette::current();
        let mut row = div().id("room-avatars").flex().items_center().gap(px(2.));
        for a in avatars.shown {
            let client = a.client.clone();
            let tip = ferris_idea_ui::tooltip::Tip {
                title: a.title.clone(),
                shortcut: String::new(),
                details: a.details.clone(),
            };
            let ring = if a.followed {
                c(p.accent)
            } else {
                gpui::transparent_black()
            };
            let face = div()
                .size(px(20.))
                .rounded_full()
                .bg(gpui::rgb(a.rgb))
                .text_color(gpui::white())
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::BOLD)
                .when(a.away, |d| d.opacity(0.5))
                .child(a.initials.clone());
            row = row.child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "room-avatar-{}",
                        a.client
                    )))
                    .relative()
                    .size(px(24.))
                    .rounded_full()
                    .border_2()
                    .border_color(ring)
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(crate::title_bar::control())
                    .tip_text(format!("room-avatar-{}", a.client), tip)
                    .child(face)
                    .when(a.away, |d| {
                        d.child(
                            div()
                                .absolute()
                                .right(px(1.))
                                .bottom(px(1.))
                                .size(px(7.))
                                .rounded_full()
                                .border_1()
                                .border_color(c(p.window))
                                .bg(gpui::rgb(0x9a9a9a)),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.room_avatar_clicked(&client, window, cx)
                    })),
            );
        }
        if !avatars.more.is_empty() {
            let tip = ferris_idea_ui::tooltip::Tip {
                title: format!("{} more", avatars.more.len()),
                shortcut: String::new(),
                details: avatars.more.clone(),
            };
            row = row.child(
                div()
                    .id("room-avatars-more")
                    .h(px(20.))
                    .px(px(5.))
                    .rounded_full()
                    .bg(c(p.toolbar_hover))
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .tip_text("room-avatars-more", tip)
                    .child(format!("+{}", avatars.more.len())),
            );
        }
        Some(row.into_any_element())
    }
}

/// The window's own place in `editor` for the room: its document, caret and selection.
pub(super) fn own_place(doc: Option<String>, caret: usize, anchor: usize) -> Presence {
    Presence {
        doc,
        caret: Some(caret),
        selection: (anchor != caret).then(|| (anchor.min(caret), anchor.max(caret))),
        follows: None,
    }
}
