// Modified by IDECraft: the fallback prompt answers the keyboard, keeps the focus and the pointer,
// and tells what it painted (see ../../IDECRAFT.md).
use std::{cell::RefCell, ops::Deref, rc::Rc};

use futures::channel::oneshot;

use crate::{
    AnyView, App, AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, PromptButton,
    PromptLevel, Render, StatefulInteractiveElement, Styled, blue, canvas, div, opaque_grey,
    transparent_black, white,
};

use super::Window;

/// The event emitted when a prompt's option is selected.
/// The usize is the index of the selected option, from the actions
/// passed to the prompt.
pub struct PromptResponse(pub usize);

/// A prompt that can be rendered in the window.
pub trait Prompt: EventEmitter<PromptResponse> + Focusable {}

impl<V: EventEmitter<PromptResponse> + Focusable> Prompt for V {}

/// A handle to a prompt that can be used to interact with it.
pub struct PromptHandle {
    sender: oneshot::Sender<usize>,
}

impl PromptHandle {
    pub(crate) fn new(sender: oneshot::Sender<usize>) -> Self {
        Self { sender }
    }

    /// Construct a new prompt handle from a view of the appropriate types
    pub fn with_view<V: Prompt + Render>(
        self,
        view: Entity<V>,
        window: &mut Window,
        cx: &mut App,
    ) -> RenderablePromptHandle {
        let mut sender = Some(self.sender);
        let previous_focus = window.focused(cx);
        let window_handle = window.window_handle();
        let focus = view.focus_handle(cx);
        let own_focus = focus.clone();
        cx.subscribe(&view, move |_: Entity<V>, e: &PromptResponse, cx| {
            if let Some(sender) = sender.take() {
                sender.send(e.0).ok();
                window_handle
                    .update(cx, |_, window, _cx| {
                        // IDECraft change: the focus goes where a view asked for it while the
                        // prompt was shown, else back to where it was before.
                        let asked = match window.prompt.take() {
                            Some(prompt) if prompt.focus == own_focus => prompt.focus_after,
                            other => {
                                window.prompt = other;
                                None
                            }
                        };
                        match asked {
                            Some(Some(handle)) => window.focus(&handle),
                            Some(None) => window.blur(),
                            None => {
                                if let Some(previous_focus) = &previous_focus {
                                    window.focus(previous_focus);
                                }
                            }
                        }
                    })
                    .ok();
            }
        })
        .detach();

        // IDECraft change: the prompt takes the focus, even from a prompt it replaces.
        window.focus_ignoring_prompt(&focus);

        RenderablePromptHandle {
            view: Box::new(view),
            focus,
            focus_after: None,
        }
    }
}

/// A prompt handle capable of being rendered in a window.
pub struct RenderablePromptHandle {
    pub(crate) view: Box<dyn PromptViewHandle>,
    /// IDECraft change: the prompt's own focus handle. While the prompt is shown the keyboard
    /// focus stays inside it (`Window::focus`).
    pub(crate) focus: FocusHandle,
    /// IDECraft change: what a view asked for while the prompt was shown — `Some(Some(handle))`
    /// to focus `handle`, `Some(None)` to blur — applied once the prompt is answered.
    pub(crate) focus_after: Option<Option<FocusHandle>>,
}

/// Use this function in conjunction with [App::set_prompt_builder] to force
/// GPUI to always use the fallback prompt renderer.
pub fn fallback_prompt_renderer(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    actions: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let renderer = cx.new(|cx| FallbackPromptRenderer {
        _level: level,
        message: message.to_string(),
        detail: detail.map(ToString::to_string),
        focused: crate::prompt_focused_answer(actions),
        actions: actions.to_vec(),
        focus: cx.focus_handle(),
        painted: Rc::default(),
    });

    handle.with_view(renderer, window, cx)
}

/// IDECraft change: where the fallback prompt was painted last, for its accessibility nodes.
#[derive(Default)]
struct Painted {
    question: Option<Bounds<Pixels>>,
    answers: Vec<Option<Bounds<Pixels>>>,
}

/// The default GPUI fallback for rendering prompts, when the platform doesn't support it.
///
/// IDECraft change (see ../../IDECRAFT.md; IDECraft's SPEC-0281 v4 behaviour 9): it answers the
/// keyboard like a macOS alert with keyboard navigation on. Return gives the default (the first
/// answer), Escape the first cancel answer (nothing without one), Space the focused answer. The
/// focus starts on [`crate::prompt_focused_answer`] — the named safe answer, else the cancel
/// answer, else the default — and a focus ring shows it; Tab and Shift+Tab move it, wrapping
/// around. Keys reported as held and keys with Control, Alt, the platform key or Fn do nothing;
/// a key that types a character moves the focus back to the safe answer, so typing on into a
/// prompt that appeared unexpectedly never gives more than the safe answer. No key reaches the
/// window behind, the pointer neither, and the focus stays in the prompt while it is shown
/// (`Window::focus`).
pub struct FallbackPromptRenderer {
    _level: PromptLevel,
    message: String,
    detail: Option<String>,
    actions: Vec<PromptButton>,
    focus: FocusHandle,
    /// IDECraft change: the answer that holds the keyboard focus.
    focused: Option<usize>,
    /// IDECraft change: the bounds of the question and its answers as last painted.
    painted: Rc<RefCell<Painted>>,
}

impl FallbackPromptRenderer {
    /// IDECraft change: the prompt's message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// IDECraft change: the text below the message.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// IDECraft change: the answers, in the order given and drawn (top to bottom).
    pub fn answers(&self) -> &[PromptButton] {
        &self.actions
    }

    /// IDECraft change: the answer that holds the keyboard focus, the one Space gives.
    pub fn focused_answer(&self) -> Option<usize> {
        self.focused
    }

    /// IDECraft change: the answer Return gives, the first.
    pub fn default_answer(&self) -> Option<usize> {
        (!self.actions.is_empty()).then_some(0)
    }

    /// IDECraft change: the answer Escape gives, the first cancel answer.
    pub fn cancel_answer(&self) -> Option<usize> {
        self.actions.iter().position(PromptButton::is_cancel)
    }

    /// IDECraft change: the bounds of the question's box as last painted, in window coordinates.
    pub fn question_bounds(&self) -> Option<Bounds<Pixels>> {
        self.painted.borrow().question
    }

    /// IDECraft change: the bounds of answer `ix` as last painted, in window coordinates.
    pub fn answer_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.painted.borrow().answers.get(ix).copied().flatten()
    }

    fn answer(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        if let Some(ix) = ix.filter(|ix| *ix < self.actions.len()) {
            cx.emit(PromptResponse(ix));
        }
    }

    fn move_focus(&mut self, back: bool, cx: &mut Context<Self>) {
        let count = self.actions.len();
        if count == 0 {
            return;
        }
        let at = self.focused.unwrap_or(0) % count;
        self.focused = Some(if back {
            (at + count - 1) % count
        } else {
            (at + 1) % count
        });
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Nothing behind the prompt gets the key.
        cx.stop_propagation();
        // A key the platform reports as held (auto-repeat) answers nothing; X11 reports none.
        if event.is_held {
            return;
        }
        let stroke = &event.keystroke;
        let m = &stroke.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return;
        }
        match (stroke.key.as_str(), m.shift) {
            ("enter", false) => self.answer(self.default_answer(), cx),
            ("escape", false) => self.answer(self.cancel_answer(), cx),
            ("space", false) => self.answer(self.focused, cx),
            ("tab", back) => self.move_focus(back, cx),
            _ => {
                // Typing: whoever types on into the prompt gives at most the safe answer.
                if stroke.key_char.as_deref().is_some_and(|c| !c.is_empty()) {
                    let safe = crate::prompt_focused_answer(&self.actions);
                    if self.focused != safe {
                        self.focused = safe;
                        cx.notify();
                    }
                }
            }
        }
    }
}

impl Render for FallbackPromptRenderer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            let mut painted = self.painted.borrow_mut();
            painted.question = None;
            painted.answers = vec![None; self.actions.len()];
        }
        let painted = self.painted.clone();
        let prompt = div()
            .relative()
            .cursor_default()
            .w_72()
            .bg(white())
            .rounded_lg()
            .overflow_hidden()
            .p_3()
            .child(
                canvas(
                    move |bounds, _, _| painted.borrow_mut().question = Some(bounds),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .justify_around()
                    .child(div().overflow_hidden().child(self.message.clone())),
            )
            .children(self.detail.clone().map(|detail| {
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .justify_around()
                    .text_sm()
                    .mb_2()
                    .child(div().child(detail))
            }))
            .children(self.actions.iter().enumerate().map(|(ix, action)| {
                let painted = self.painted.clone();
                let focused = self.focused == Some(ix);
                // The focus ring: a 2-pixel frame around the button, invisible around the others.
                div()
                    .mt_1()
                    .rounded_sm()
                    .border_2()
                    .border_color(if focused { blue() } else { transparent_black() })
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_row()
                            .justify_around()
                            .border_1()
                            .border_color(opaque_grey(0.2, 0.5))
                            .rounded_xs()
                            .cursor_pointer()
                            .text_sm()
                            .child(action.label().clone())
                            .child(
                                canvas(
                                    move |bounds, _, _| {
                                        if let Some(slot) =
                                            painted.borrow_mut().answers.get_mut(ix)
                                        {
                                            *slot = Some(bounds);
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            )
                            .id(ix)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(PromptResponse(ix));
                            })),
                    )
            }));

        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .child(
                // The cover: the pointer reaches nothing behind the prompt.
                div()
                    .id("prompt-cover")
                    .size_full()
                    .bg(opaque_grey(0.5, 0.6))
                    .absolute()
                    .top_0()
                    .left_0()
                    .occlude(),
            )
            .child(
                div()
                    .size_full()
                    .absolute()
                    .top_0()
                    .left_0()
                    .flex()
                    .flex_col()
                    .justify_around()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .justify_around()
                            .child(prompt),
                    ),
            )
    }
}

impl EventEmitter<PromptResponse> for FallbackPromptRenderer {}

impl Focusable for FallbackPromptRenderer {
    fn focus_handle(&self, _: &crate::App) -> FocusHandle {
        self.focus.clone()
    }
}

pub(crate) trait PromptViewHandle {
    fn any_view(&self) -> AnyView;
}

impl<V: Prompt + Render> PromptViewHandle for Entity<V> {
    fn any_view(&self) -> AnyView {
        self.clone().into()
    }
}

pub(crate) enum PromptBuilder {
    Default,
    Custom(
        Box<
            dyn Fn(
                PromptLevel,
                &str,
                Option<&str>,
                &[PromptButton],
                PromptHandle,
                &mut Window,
                &mut App,
            ) -> RenderablePromptHandle,
        >,
    ),
}

impl Deref for PromptBuilder {
    type Target = dyn Fn(
        PromptLevel,
        &str,
        Option<&str>,
        &[PromptButton],
        PromptHandle,
        &mut Window,
        &mut App,
    ) -> RenderablePromptHandle;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Default => &fallback_prompt_renderer,
            Self::Custom(f) => f.as_ref(),
        }
    }
}
