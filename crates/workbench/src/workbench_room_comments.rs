//! The comments of the project room in a remote project's window (`SPEC-0323` v8 behaviours
//! 53–60): the marks in the editors' gutters, the thread popover, Add Comment, and what the
//! window asks the room. The decisions are in [`crate::room_comments_model`] and the popover in
//! [`crate::room_comment_popup`]; this module feeds them and carries out what they say.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};
use serde_json::{Value, json};

use super::WorkbenchView;
use crate::room_comment_popup::{CommentPopupEvent, RoomCommentPopup};
use crate::room_comments_model::{self as model, Comments};

/// How long a comment request may take.
const LIMIT: Duration = Duration::from_secs(5);

/// What the window last saw of one document.
#[derive(Default)]
struct DocLook {
    text: String,
    revision: u64,
    changed: Option<Instant>,
    fetched: bool,
    asking: bool,
    /// The text changed since the last answer: ask again once it stands still.
    again: bool,
    /// The room said the comments changed: ask now.
    told: bool,
    /// The text was read once.
    seen: bool,
}

/// What a remote window keeps of the room's comments.
#[derive(Default)]
pub(crate) struct CommentsState {
    comments: Comments,
    docs: HashMap<String, DocLook>,
    generation: Option<u64>,
    /// The window's own role in the room.
    role: Option<String>,
    asking_role: bool,
    popup: Option<Entity<RoomCommentPopup>>,
    _popup_events: Option<Subscription>,
}

/// The 0-based line of character `at` in `editor`, clamped to the text.
fn line_of(e: &crate::EditorView, at: usize) -> usize {
    let buffer = &e.state.buffer;
    buffer
        .offset_to_position(at.min(buffer.len_chars()))
        .map_or(0, |p| p.line)
}

impl WorkbenchView {
    /// The room's comments as this window knows them (window tests).
    pub fn room_comments(&self) -> Option<&Comments> {
        let r = self.remote_project.as_ref()?;
        r.room.view.as_ref()?;
        Some(&r.room.comments.comments)
    }

    /// The thread popover, if it is open.
    pub fn room_comment_popup(&self) -> Option<Entity<RoomCommentPopup>> {
        self.remote_project.as_ref()?.room.comments.popup.clone()
    }

    /// Whether this window may comment: `None` without a room, else whether its role allows it
    /// (behaviour 54).
    pub fn room_can_comment(&self) -> Option<bool> {
        let r = self.remote_project.as_ref()?;
        r.room.view.as_ref()?;
        Some(
            r.room
                .comments
                .role
                .as_deref()
                .is_some_and(model::can_comment),
        )
    }

    /// One look at the comments, with the window's look at its room.
    pub(super) fn room_comments_tick(&mut self, cx: &mut Context<Self>) {
        let in_room = self.room_view().is_some();
        let own = self.room_view().map(|v| v.own().to_owned());
        let editors: Vec<(String, Entity<crate::EditorView>)> = self
            .editors
            .iter()
            .filter_map(|(p, e)| Some((self.remote_rel(p)?, e.clone())))
            .collect();
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let state = &mut r.room.comments;
        if !in_room {
            if state.generation.is_some() || state.popup.is_some() {
                *state = CommentsState::default();
                for (_, e) in &editors {
                    e.update(cx, |e, cx| e.set_comment_marks(Vec::new(), cx));
                }
                cx.notify();
            }
            return;
        }
        let generation = r.live.generation();
        if state.generation != Some(generation) {
            // Connected again (or for the first time): everything is asked for afresh.
            state.generation = Some(generation);
            state.role = None;
            state.asking_role = false;
            for look in state.docs.values_mut() {
                look.fetched = false;
            }
        }
        if state.role.is_none() && !state.asking_role {
            state.asking_role = true;
            let live = r.live.clone();
            let task = cx
                .background_executor()
                .spawn(async move { live.host_call("room.status", json!({}), LIMIT) });
            cx.spawn(async move |this, cx| {
                let status = task.await;
                let _ = this.update(cx, |this, cx| {
                    let Some(r) = this.remote_project.as_mut() else {
                        return;
                    };
                    let state = &mut r.room.comments;
                    state.asking_role = false;
                    let role = status.ok().and_then(|s| {
                        s["participants"]
                            .as_array()?
                            .iter()
                            .find(|p| own.is_some() && p["client"].as_str() == own.as_deref())?
                            ["role"]
                            .as_str()
                            .map(str::to_owned)
                    });
                    // Without an answer the next look asks again.
                    state.role = role;
                    cx.notify();
                });
            })
            .detach();
        }
        // The documents shown: their texts move the marks at once, and the room is asked once
        // the text stands still (behaviour 59).
        let now = Instant::now();
        let open: Vec<&String> = editors.iter().map(|(d, _)| d).collect();
        state.docs.retain(|d, _| open.contains(&d));
        let mut ask = Vec::new();
        for (doc, editor) in &editors {
            let e = editor.read(cx);
            let revision = e.revision();
            let look = state.docs.entry(doc.clone()).or_default();
            if !look.seen {
                look.seen = true;
                look.text = e.state.buffer.text();
                look.revision = revision;
            } else if look.revision != revision {
                let text = e.state.buffer.text();
                if let Some((at, deleted, inserted)) = model::text_change(&look.text, &text) {
                    state.comments.edit(doc, at, deleted, inserted);
                    look.again = true;
                    look.changed = Some(now);
                }
                look.text = text;
                look.revision = revision;
            }
            let quiet = look.changed.is_none_or(|t| {
                now.duration_since(t) >= Duration::from_millis(model::REFRESH_QUIET_MS)
            });
            if !look.asking && (!look.fetched || look.told || (look.again && quiet)) {
                look.asking = true;
                look.told = false;
                look.again = false;
                ask.push((doc.clone(), revision));
            }
        }
        for (doc, revision) in ask {
            self.room_comments_fetch(doc, revision, cx);
        }
        self.room_comments_show(cx);
    }

    /// Asks the room for the comments of `doc` (at the editor's `revision`).
    fn room_comments_fetch(&mut self, doc: String, revision: u64, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let live = r.live.clone();
        let params = json!({ "doc": doc });
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call("room.comments", params, LIMIT) });
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(r) = this.remote_project.as_mut() else {
                    return;
                };
                let state = &mut r.room.comments;
                let Some(look) = state.docs.get_mut(&doc) else {
                    return;
                };
                look.asking = false;
                if let Ok(answer) = answer {
                    look.fetched = true;
                    // Typed while the answer was on its way: asked again when the text stands
                    // still.
                    if look.revision != revision {
                        look.again = true;
                    }
                    state.comments.set_doc(&doc, &answer);
                }
                this.room_comments_show(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Hands every editor its marks and the popover its threads.
    fn room_comments_show(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<(PathBuf, String, Entity<crate::EditorView>)> = self
            .editors
            .iter()
            .filter_map(|(p, e)| Some((p.clone(), self.remote_rel(p)?, e.clone())))
            .collect();
        let can = self.room_can_comment() == Some(true);
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let comments = r.room.comments.comments.clone();
        let popup = r.room.comments.popup.clone();
        for (path, doc, editor) in &editors {
            let marks = {
                let e = editor.read(cx);
                comments.marks(doc, |at| line_of(e, at))
            };
            for view in std::iter::once(editor.clone()).chain(self.twins(path)) {
                view.update(cx, |e, cx| e.set_comment_marks(marks.clone(), cx));
            }
        }
        let Some(popup) = popup else {
            return;
        };
        let (doc, line, ids) = {
            let p = popup.read(cx);
            let (doc, line) = p.place();
            let ids: Vec<u64> = p.threads().iter().map(|t| t.id()).collect();
            (doc.to_owned(), line, ids)
        };
        let Some(editor) = editors
            .iter()
            .find(|(_, d, _)| *d == doc)
            .map(|(_, _, e)| e.clone())
        else {
            return;
        };
        let e = editor.read(cx);
        // The line follows the popover's first thread when the text moved it.
        let line = ids
            .iter()
            .find_map(|id| comments.get(&doc, *id))
            .map_or(line, |c| line_of(e, c.at));
        let threads = comments.threads_on(&doc, line, |at| line_of(e, at));
        popup.update(cx, |p, cx| {
            p.set_line(line);
            p.set_threads(threads, can);
            cx.notify();
        });
    }

    /// `room.comments_changed`: someone changed the comments of a document (behaviour 58).
    pub(super) fn room_comments_event(&mut self, params: &Value, cx: &mut Context<Self>) {
        let Some(doc) = params["doc"]
            .as_str()
            .filter(|d| crate::room_model::safe_doc(d))
        else {
            return;
        };
        if let Some(look) = self
            .remote_project
            .as_mut()
            .and_then(|r| r.room.comments.docs.get_mut(doc))
        {
            look.told = true;
        }
        cx.notify();
    }

    /// A click on the comment mark of `line` in the editor of `path` (behaviour 54).
    pub fn room_comment_mark_clicked(
        &mut self,
        path: &std::path::Path,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.remote_rel(path) else {
            return;
        };
        self.open_comment_popup(doc, line, None, window, cx);
    }

    /// Add Comment (`Room.AddComment`): a new thread on the caret's line (behaviour 54).
    pub fn room_add_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let place = self.active_path().and_then(|path| {
            let doc = self.remote_rel(&path)?;
            let e = self.editors.get(&path)?.read(cx);
            let head = e.state.carets.primary().head;
            let line = line_of(e, head);
            let at = e
                .state
                .buffer
                .position_to_offset(ferris_idea_editor::Position::new(line, 0))
                .unwrap_or(0);
            Some((doc, line, at))
        });
        match (self.room_can_comment(), place) {
            (Some(true), Some((doc, line, at))) => {
                self.open_comment_popup(doc, line, Some(at), window, cx)
            }
            _ => {
                self.status = Some(model::CANNOT_COMMENT.to_owned());
                cx.notify();
            }
        }
    }

    fn open_comment_popup(
        &mut self,
        doc: String,
        line: usize,
        new_at: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let can = self.room_can_comment() == Some(true);
        let Some(path) = self.remote_local_path(&doc) else {
            return;
        };
        let threads = match (self.room_comments(), self.editors.get(&path)) {
            (Some(comments), Some(editor)) => {
                let e = editor.read(cx);
                comments.threads_on(&doc, line, |at| line_of(e, at))
            }
            _ => Vec::new(),
        };
        let view = cx.new(|cx| RoomCommentPopup::new(doc, line, new_at, threads, can, cx));
        let events = cx.subscribe_in(&view, window, |this, popup, event, window, cx| {
            this.room_comment_popup_event(popup, event, window, cx)
        });
        window.focus(&view.read(cx).focus(cx));
        if let Some(r) = self.remote_project.as_mut() {
            r.room.comments.popup = Some(view);
            r.room.comments._popup_events = Some(events);
        }
        cx.notify();
    }

    fn room_comment_popup_event(
        &mut self,
        popup: &Entity<RoomCommentPopup>,
        event: &CommentPopupEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (method, params) = match event {
            CommentPopupEvent::Close => {
                if let Some(r) = self.remote_project.as_mut() {
                    r.room.comments.popup = None;
                    r.room.comments._popup_events = None;
                }
                self.focus_active(window, cx);
                cx.notify();
                return;
            }
            CommentPopupEvent::Add { doc, at, text } => (
                "room.comment",
                json!({ "doc": doc, "at": at, "text": text }),
            ),
            CommentPopupEvent::Reply { id, text } => {
                ("room.comment", json!({ "reply_to": id, "text": text }))
            }
            CommentPopupEvent::Resolve { id, resolved } => (
                "room.resolve_comment",
                json!({ "id": id, "resolved": resolved }),
            ),
            CommentPopupEvent::Delete { id } => ("room.delete_comment", json!({ "id": id })),
        };
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        let doc = popup.read(cx).place().0.to_owned();
        let live = r.live.clone();
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call(method, params, LIMIT) });
        let popup = popup.clone();
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(look) = this
                    .remote_project
                    .as_mut()
                    .and_then(|r| r.room.comments.docs.get_mut(&doc))
                {
                    look.told = true;
                }
                let note = done.err().map(|e| e.message);
                popup.update(cx, |p, cx| p.set_note(note, cx));
                cx.notify();
            });
        })
        .detach();
    }

    /// The popover beside its line, in window coordinates (behaviour 54).
    pub(super) fn room_comment_popup_element(&self, cx: &gpui::App) -> Option<AnyElement> {
        let popup = self.room_comment_popup()?;
        let spot = {
            let p = popup.read(cx);
            let (doc, line) = p.place();
            self.remote_local_path(doc)
                .and_then(|path| self.editors.get(&path))
                .and_then(|e| {
                    let e = e.read(cx);
                    let offset = e
                        .state
                        .buffer
                        .position_to_offset(ferris_idea_editor::Position::new(line, 0))
                        .ok()?;
                    let below = e.point_below(offset)?;
                    Some(gpui::point(e.painted_bounds().origin.x, below.y))
                })
        };
        Some(match spot {
            Some(at) => div()
                .absolute()
                .top(at.y)
                .left(at.x + px(8.))
                .child(popup)
                .into_any_element(),
            None => div()
                .absolute()
                .top(px(super::toolbar_height() + 60.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(popup)
                .into_any_element(),
        })
    }
}
