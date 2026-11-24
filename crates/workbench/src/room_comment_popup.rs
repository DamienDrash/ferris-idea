//! The thread popover of the project room's comments beside a line (`SPEC-0323` v8 behaviours
//! 54–57): the threads of the line with author, time, text and resolved state, a field for a new
//! comment or a reply, Resolve and Reopen, and Delete on one's own comments after a second click.
//! Texts are drawn as plain text: no markup, no links. The window carries out what it asks for
//! ([`CommentPopupEvent`]) and hands it the room's threads again.

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::room_comments_model::{Thread, clean_input, shown_text, time_text};
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// What the popover asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentPopupEvent {
    /// A new thread on `doc` at `at` (characters).
    Add {
        /// The document.
        doc: String,
        /// The place.
        at: usize,
        /// The text, made plain.
        text: String,
    },
    /// A reply in thread `id`.
    Reply {
        /// The thread.
        id: u64,
        /// The text, made plain.
        text: String,
    },
    /// Resolve (`true`) or reopen thread `id`.
    Resolve {
        /// The thread.
        id: u64,
        /// The new state.
        resolved: bool,
    },
    /// Delete one's own comment `id`.
    Delete {
        /// The comment.
        id: u64,
    },
    /// Close the popover.
    Close,
}

/// The popover of one line.
pub struct RoomCommentPopup {
    doc: String,
    line: usize,
    /// Add Comment: a new thread at this place (characters), until it is sent.
    new_at: Option<usize>,
    threads: Vec<Thread>,
    can_comment: bool,
    /// The thread the field answers when no new thread is being written.
    reply_to: Option<u64>,
    /// A Delete clicked once: the comment waiting for the second click.
    confirm_delete: Option<u64>,
    field: Entity<EditorView>,
    focus: FocusHandle,
    note: Option<String>,
}

impl EventEmitter<CommentPopupEvent> for RoomCommentPopup {}

/// Unix milliseconds now.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl RoomCommentPopup {
    /// A popover for the 0-based `line` of `doc` with its `threads`; with `new_at` it writes a
    /// new thread at that place.
    pub fn new(
        doc: String,
        line: usize,
        new_at: Option<usize>,
        threads: Vec<Thread>,
        can_comment: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut popup = Self {
            doc,
            line,
            new_at,
            threads: Vec::new(),
            can_comment,
            reply_to: None,
            confirm_delete: None,
            field: cx.new(EditorView::field),
            focus: cx.focus_handle(),
            note: None,
        };
        popup.set_threads(threads, can_comment);
        popup
    }

    /// The document and 0-based line it stands beside.
    pub fn place(&self) -> (&str, usize) {
        (&self.doc, self.line)
    }

    /// Moves it to another line (its threads moved with the text).
    pub fn set_line(&mut self, line: usize) {
        self.line = line;
    }

    /// The threads it shows.
    pub fn threads(&self) -> &[Thread] {
        &self.threads
    }

    /// Whether it writes a new thread.
    pub fn writes_new(&self) -> bool {
        self.new_at.is_some()
    }

    /// The thread a reply goes to.
    pub fn reply_target(&self) -> Option<u64> {
        self.reply_to
    }

    /// The line under the field: why something was not sent, or the room's answer.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The comment waiting for the second click on Delete.
    pub fn confirming(&self) -> Option<u64> {
        self.confirm_delete
    }

    /// The threads of its line as the room has them now.
    pub fn set_threads(&mut self, threads: Vec<Thread>, can_comment: bool) {
        self.can_comment = can_comment;
        if self
            .reply_to
            .is_none_or(|id| !threads.iter().any(|t| t.id() == id))
        {
            // The last open thread, else the last one.
            self.reply_to = threads
                .iter()
                .rev()
                .find(|t| !t.resolved())
                .or(threads.last())
                .map(Thread::id);
        }
        if let Some(id) = self.confirm_delete
            && !threads.iter().any(|t| t.comments().any(|c| c.id == id))
        {
            self.confirm_delete = None;
        }
        self.threads = threads;
    }

    /// Shows `note` under the field.
    pub fn set_note(&mut self, note: Option<String>, cx: &mut Context<Self>) {
        self.note = note;
        cx.notify();
    }

    /// The field's focus, where the popover opens.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        if self.can_comment {
            self.field.read(cx).focus()
        } else {
            self.focus.clone()
        }
    }

    /// The text in the field.
    pub fn input(&self, cx: &gpui::App) -> String {
        self.field.read(cx).state.buffer.text()
    }

    /// Puts `text` in the field (the window tests type through this as well as through keys).
    pub fn set_input(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Lets the field answer thread `id` (its Reply).
    pub fn reply(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.new_at = None;
        self.reply_to = Some(id);
        window.focus(&self.field.read(cx).focus());
        cx.notify();
    }

    /// Sends the field: a new thread or a reply. Text that is empty or too long stays, with the
    /// reason.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_comment {
            return;
        }
        let text = match clean_input(&self.input(cx)) {
            Ok(t) => t,
            Err(e) => {
                self.note = Some(e.text().to_owned());
                cx.notify();
                return;
            }
        };
        let event = match (self.new_at, self.reply_to) {
            (Some(at), _) => CommentPopupEvent::Add {
                doc: self.doc.clone(),
                at,
                text,
            },
            (None, Some(id)) => CommentPopupEvent::Reply { id, text },
            (None, None) => return,
        };
        self.new_at = None;
        self.note = None;
        self.field.update(cx, |f, cx| f.set_text("", cx));
        cx.emit(event);
        cx.notify();
    }

    /// Resolves thread `id`, or reopens it when it is resolved.
    pub fn toggle_resolved(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.threads.iter().find(|t| t.id() == id) else {
            return;
        };
        if self.can_comment {
            cx.emit(CommentPopupEvent::Resolve {
                id,
                resolved: !t.resolved(),
            });
        }
    }

    /// Delete on comment `id`: the first click asks, the second deletes (behaviour 56).
    pub fn delete(&mut self, id: u64, cx: &mut Context<Self>) {
        let own = self
            .threads
            .iter()
            .flat_map(Thread::comments)
            .any(|c| c.id == id && c.own);
        if !own || !self.can_comment {
            return;
        }
        if self.confirm_delete == Some(id) {
            self.confirm_delete = None;
            cx.emit(CommentPopupEvent::Delete { id });
        } else {
            self.confirm_delete = Some(id);
        }
        cx.notify();
    }

    fn keep(&mut self, cx: &mut Context<Self>) {
        self.confirm_delete = None;
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        match k.key.as_str() {
            "escape" => cx.emit(CommentPopupEvent::Close),
            "enter" if !k.modifiers.shift && !k.modifiers.alt => self.submit(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn link(
        id: String,
        label: &'static str,
        cx: &mut Context<Self>,
        on: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui::AnyElement {
        let p = Palette::current();
        div()
            .id(SharedString::from(id.clone()))
            .cursor_pointer()
            .text_color(c(p.link))
            .child(label)
            // In the semantic tree for the UI driver and the window tests (`SPEC-0319` behaviour
            // 44); Resolve, Reopen and the confirming Delete change the room's comments.
            .semantics(button_semantics(label, id.clone(), effect_of(&id)))
            .on_click(
                cx.listener(move |this, _: &gpui::ClickEvent, window, cx| on(this, window, cx)),
            )
            .into_any_element()
    }
}

/// A button of the popover in the semantic tree.
fn button_semantics(name: &str, test_id: String, effect: Effect) -> Semantics {
    Semantics::new(Role::Button)
        .name(name)
        .test_id(test_id)
        .action(AxAction::Click)
        .effects(&[effect])
}

/// What a link of the popover changes: Resolve, Reopen and the confirming Delete change the
/// room's comments (`write`); Reply, the first Delete, Keep and Close only change the popover.
fn effect_of(id: &str) -> Effect {
    if id.starts_with("room-comment-resolve-") || id == "room-comment-delete-confirm" {
        Effect::Write
    } else {
        Effect::None
    }
}

impl Render for RoomCommentPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let now = unix_now();
        let mut list = div()
            .id("room-comment-threads")
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(320.))
            .overflow_y_scroll();
        for t in self.threads.clone() {
            let mut thread = div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded(px(6.))
                .border_1()
                .border_color(c(p.separator))
                .when(t.resolved(), |d| d.opacity(0.7));
            for (i, comment) in t.comments().enumerate() {
                let id = comment.id;
                let mut head = div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(comment.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child(time_text(comment.time, now)),
                    );
                if i == 0 && t.resolved() {
                    head = head.child(
                        div()
                            .text_xs()
                            .px_1()
                            .rounded(px(3.))
                            .bg(c(p.tree_hover))
                            .text_color(c(p.ui_muted))
                            .child("Resolved"),
                    );
                }
                head = head.child(div().flex_1());
                if comment.own && self.can_comment && self.confirm_delete != Some(id) {
                    head = head.child(Self::link(
                        format!("room-comment-delete-{id}"),
                        "Delete",
                        cx,
                        move |this, _, cx| this.delete(id, cx),
                    ));
                }
                thread = thread.child(head).child(
                    div()
                        .id(SharedString::from(format!("room-comment-text-{id}")))
                        .text_color(c(p.ui_text))
                        // Plain text: never markup, never a link (behaviour 57).
                        .child(shown_text(&comment.text)),
                );
                if self.confirm_delete == Some(id) {
                    let what = if i == 0 && !t.replies.is_empty() {
                        "Delete this comment and its replies?"
                    } else {
                        "Delete this comment?"
                    };
                    thread = thread.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(c(p.error_text))
                            .child(what)
                            .child(div().flex_1())
                            .child(Self::link(
                                "room-comment-delete-confirm".into(),
                                "Delete",
                                cx,
                                move |this, _, cx| this.delete(id, cx),
                            ))
                            .child(Self::link(
                                "room-comment-delete-keep".into(),
                                "Keep",
                                cx,
                                |this, _, cx| this.keep(cx),
                            )),
                    );
                }
            }
            if self.can_comment {
                let id = t.id();
                let answering = self.new_at.is_none() && self.reply_to == Some(id);
                thread = thread.child(
                    div()
                        .flex()
                        .gap_3()
                        .text_xs()
                        .when(!answering, |d| {
                            d.child(Self::link(
                                format!("room-comment-reply-{id}"),
                                "Reply",
                                cx,
                                move |this, window, cx| this.reply(id, window, cx),
                            ))
                        })
                        .child(Self::link(
                            format!("room-comment-resolve-{id}"),
                            if t.resolved() { "Reopen" } else { "Resolve" },
                            cx,
                            move |this, _, cx| this.toggle_resolved(id, cx),
                        )),
                );
            }
            list = list.child(thread);
        }
        let footer =
            if self.can_comment {
                let (hint, action) = match (self.new_at, self.reply_to) {
                    (Some(_), _) => (format!("New comment on line {}", self.line + 1), "Comment"),
                    (None, Some(id)) => {
                        let name = self
                            .threads
                            .iter()
                            .find(|t| t.id() == id)
                            .map(|t| t.first.name.clone())
                            .unwrap_or_default();
                        (format!("Reply to {name}"), "Reply")
                    }
                    (None, None) => (String::new(), "Reply"),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().text_color(c(p.ui_muted)).child(hint))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                crate::metrics::field_frame()
                                    .flex_1()
                                    .child(self.field.clone()),
                            )
                            .child(
                                crate::metrics::button("room-comment-submit", action, true)
                                    .semantics(button_semantics(
                                        action,
                                        "room-comment-submit".to_owned(),
                                        Effect::Write,
                                    ))
                                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                        this.submit(cx)
                                    })),
                            ),
                    )
                    .into_any_element()
            } else {
                div()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("Commenting needs the role commenter or higher in this room.")
                    .into_any_element()
            };
        div()
            .id("room-comment-popup")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(380.))
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
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Comments"),
                    )
                    .child(
                        div()
                            .pl_2()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(format!("{}:{}", self.doc, self.line + 1)),
                    )
                    .child(div().flex_1())
                    .child(Self::link(
                        "room-comment-close".into(),
                        "Close",
                        cx,
                        |_, _, cx| cx.emit(CommentPopupEvent::Close),
                    )),
            )
            .when(!self.threads.is_empty(), |d| d.child(list))
            .child(footer)
            .when_some(self.note.clone(), |d, note| {
                d.child(div().text_xs().text_color(c(p.error_text)).child(note))
            })
    }
}
