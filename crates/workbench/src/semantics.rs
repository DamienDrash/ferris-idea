//! The annotation helper of the semantic tree (`SPEC-0319` behaviour 1) and, on GPUI 0.2.2, its
//! **bridge producer** (ADR-0027 section 6): an element describes itself once with
//! `.semantics(Semantics::new(Role::Button).name("Commit").test_id("commit-button")…)`; while a
//! consumer is active (the self-test, an external driver call) an invisible canvas records the
//! element's semantics and painted bounds, and the window's frame ends with the finished tree.
//! Without a consumer nothing is recorded and no canvas is added (behaviour 7).
//!
//! The same calls map to GPUI's own accessibility API after the toolkit switch (phase 3); call
//! sites do not change.
//!
//! Also here: the marking shown while an external client drives the window (behaviour 36) and
//! the hooks the window's driver host sets (emergency stop, the user's own input).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use gpui::{
    App, InteractiveElement, IntoElement, MouseDownEvent, MouseMoveEvent, ParentElement,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, Window, div, px,
};
pub use idecraft_uidriver::{
    AxAction, Effect, Rect, Reserved, Role, Semantics, StopSource, TextClass, Tree, TreeBuilder,
};

/// The self-test wants the tree.
static SELFTEST: AtomicBool = AtomicBool::new(false);
/// The driver host has calls waiting for frames.
static DRIVER: AtomicBool = AtomicBool::new(false);

/// Who needs the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    /// The window self-test (its driver steps).
    SelfTest,
    /// The driver host while external calls wait.
    Driver,
}

/// Starts or stops building the tree for `consumer`.
pub fn set_consumer(consumer: Consumer, on: bool) {
    match consumer {
        Consumer::SelfTest => SELFTEST.store(on, Ordering::Relaxed),
        Consumer::Driver => DRIVER.store(on, Ordering::Relaxed),
    }
}

/// Whether anyone needs the tree now.
pub fn active() -> bool {
    SELFTEST.load(Ordering::Relaxed) || DRIVER.load(Ordering::Relaxed)
}

#[derive(Default)]
struct WindowFrame {
    frame: u64,
    building: TreeBuilder,
    complete: Option<Rc<Tree>>,
}

type FrameHook = Rc<dyn Fn(&mut Window, &mut App)>;
type StopHook = Rc<dyn Fn(StopSource, &mut Window, &mut App)>;
type InputHook = Rc<dyn Fn(&mut App)>;

#[derive(Default)]
struct Hooks {
    on_frame: Option<FrameHook>,
    on_stop: Option<StopHook>,
    on_user_input: Option<InputHook>,
}

/// What the marking shows in one window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marking {
    /// The clients driving it.
    pub clients: Vec<String>,
    /// Shown until then (30 s after a client's last call); `None`: until the stop.
    pub until: Option<Instant>,
    /// The element about to get input, until when.
    pub highlight: Option<(Rect, Instant)>,
    /// The action point of the last pointer action, until when.
    pub pointer: Option<((f32, f32), Instant)>,
}

thread_local! {
    static FRAMES: RefCell<HashMap<u64, WindowFrame>> = RefCell::new(HashMap::new());
    static HOOKS: RefCell<Hooks> = RefCell::new(Hooks::default());
    static MARKINGS: RefCell<HashMap<u64, Marking>> = RefCell::new(HashMap::new());
    static SYNTHESIZING: Cell<bool> = const { Cell::new(false) };
}

/// The id of `window` as the driver names windows.
pub fn window_id(window: &Window) -> u64 {
    window.window_handle().window_id().as_u64()
}

/// Starts the tree of a new frame of `window` (called by the workbench's render).
pub fn begin_frame(window: &Window) {
    if !active() {
        return;
    }
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let w = f.entry(window_id(window)).or_default();
        w.frame += 1;
        w.building = TreeBuilder::new();
    });
}

/// Records one element of the frame being drawn.
fn record(window: u64, semantics: Semantics, bounds: Rect, visible: Option<Rect>) {
    FRAMES.with(|f| {
        f.borrow_mut()
            .entry(window)
            .or_default()
            .building
            .push(semantics, bounds, visible);
    });
}

/// The tree of the last frame `window` presented, if one was built.
pub fn tree(window: u64) -> Option<Rc<Tree>> {
    FRAMES.with(|f| f.borrow().get(&window).and_then(|w| w.complete.clone()))
}

/// Forgets the tree of a closed window.
pub fn forget(window: u64) {
    FRAMES.with(|f| f.borrow_mut().remove(&window));
    MARKINGS.with(|m| m.borrow_mut().remove(&window));
}

fn rect(b: gpui::Bounds<gpui::Pixels>) -> Rect {
    Rect::new(
        f32::from(b.origin.x),
        f32::from(b.origin.y),
        f32::from(b.size.width),
        f32::from(b.size.height),
    )
}

/// An invisible element filling its (relatively positioned) parent that records the parent's
/// `semantics` with its painted bounds, clipped by the scroll areas and windows around it;
/// nothing unless a consumer is active.
pub fn node(semantics: Semantics) -> Option<impl IntoElement> {
    if !active() {
        return None;
    }
    Some(
        gpui::canvas(
            move |b, window, _| {
                let bounds = rect(b);
                let visible = bounds.intersect(&rect(window.content_mask().bounds));
                record(window_id(window), semantics, bounds, visible);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

/// Records the bounds of a `hit_targets::mark` site as a node with that test id (migration
/// step 1, `SPEC-0319` behaviour 42): role, name and effects come with its annotation.
pub(crate) fn record_mark(window: &Window, name: &str, bounds: Rect, visible: Option<Rect>) {
    record(window_id(window), Semantics::marked(name), bounds, visible);
}

/// The annotation of an element (behaviour 1).
pub trait Annotated: ParentElement + Sized {
    /// Describes this element in the semantic tree.
    fn semantics(self, semantics: Semantics) -> Self {
        self.children(node(semantics))
    }
}

impl<E: ParentElement + Sized> Annotated for E {}

/// The last element of a window's frame: when it paints, every element of the frame has been
/// prepainted, so the tree is complete; the driver host's frame hook runs then.
pub fn frame_end() -> Option<impl IntoElement> {
    if !active() {
        return None;
    }
    Some(
        gpui::canvas(
            |_, _, _| {},
            |_, _, window, cx| {
                let id = window_id(window);
                record_drawn_prompt(window, cx);
                FRAMES.with(|f| {
                    let mut f = f.borrow_mut();
                    let w = f.entry(id).or_default();
                    let frame = w.frame;
                    let built = std::mem::take(&mut w.building);
                    w.complete = Some(Rc::new(built.finish(frame)));
                });
                let hook = HOOKS.with(|h| h.borrow().on_frame.clone());
                if let Some(hook) = hook {
                    hook(window, cx);
                }
            },
        )
        .absolute()
        .size_0(),
    )
}

/// The test id of the question the toolkit draws where the platform has no alert.
pub const DRAWN_PROMPT: &str = "native-prompt";

/// The test id of answer `ix` of that question (from 0, in the order of its answers).
pub fn drawn_prompt_answer(ix: usize) -> String {
    format!("{DRAWN_PROMPT}-answer-{ix}")
}

/// Records the question the toolkit draws over the window where the platform has no alert —
/// Linux — as the last nodes of the frame (`SPEC-0281` v4 behaviour 9.6): a cover over the window,
/// a modal `alertdialog` named by the message with the detail as its description, and its answers
/// as buttons, the focused one marked, the default with `Enter`, the cancel answer with `Escape`.
/// All of it is reserved for the user (`SPEC-0319` v6 behaviour 35). Painted after the workbench,
/// the question's prepaint has recorded its bounds when this runs.
fn record_drawn_prompt(window: &Window, cx: &App) {
    let Some(prompt) = window.fallback_prompt() else {
        return;
    };
    let prompt = prompt.read(cx);
    let Some(question) = prompt.question_bounds() else {
        return;
    };
    let id = window_id(window);
    let viewport = window.viewport_size();
    let cover = Rect::new(
        0.,
        0.,
        f32::from(viewport.width),
        f32::from(viewport.height),
    );
    record(
        id,
        Semantics::new(Role::GenericContainer)
            .test_id(format!("{DRAWN_PROMPT}-cover"))
            .overlay(true),
        cover,
        Some(cover),
    );
    let question = rect(question);
    let mut dialog = Semantics::new(Role::AlertDialog)
        .name(prompt.message())
        .test_id(DRAWN_PROMPT)
        .modal()
        .reserved(Reserved::Decision);
    if let Some(detail) = prompt.detail() {
        dialog = dialog.description(detail);
    }
    record(id, dialog, question, question.intersect(&cover));
    for (ix, answer) in prompt.answers().iter().enumerate() {
        let Some(bounds) = prompt.answer_bounds(ix) else {
            continue;
        };
        let keys: Vec<&str> = [
            (prompt.default_answer() == Some(ix)).then_some("Enter"),
            (prompt.cancel_answer() == Some(ix)).then_some("Escape"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut button = Semantics::new(Role::Button)
            .name(answer.label().as_ref())
            .test_id(drawn_prompt_answer(ix))
            .action(AxAction::Click)
            .focused(prompt.focused_answer() == Some(ix))
            .reserved(Reserved::Decision);
        if !keys.is_empty() {
            button = button.keyboard_shortcut(keys.join(" "));
        }
        let bounds = rect(bounds);
        record(id, button, bounds, bounds.intersect(&cover));
    }
}

/// Sets what runs when a window's tree is complete (the driver host).
pub fn on_frame(hook: impl Fn(&mut Window, &mut App) + 'static) {
    HOOKS.with(|h| h.borrow_mut().on_frame = Some(Rc::new(hook)));
}

/// Sets what the emergency stop does (the driver host).
pub fn on_stop(hook: impl Fn(StopSource, &mut Window, &mut App) + 'static) {
    HOOKS.with(|h| h.borrow_mut().on_stop = Some(Rc::new(hook)));
}

/// Sets what the user's own input does while a client drives (the driver host).
pub fn on_user_input(hook: impl Fn(&mut App) + 'static) {
    HOOKS.with(|h| h.borrow_mut().on_user_input = Some(Rc::new(hook)));
}

/// The emergency stop from the window: Esc, the Stop button, the menu item (behaviour 37).
pub fn emergency_stop(source: StopSource, window: &mut Window, cx: &mut App) {
    let hook = HOOKS.with(|h| h.borrow().on_stop.clone());
    if let Some(hook) = hook {
        hook(source, window, cx);
    }
}

/// Runs `f` while input the driver synthesises is dispatched, so the window does not take it for
/// the user's own (behaviour 37).
pub fn synthesizing<R>(f: impl FnOnce() -> R) -> R {
    let before = SYNTHESIZING.with(|s| s.replace(true));
    let r = f();
    SYNTHESIZING.with(|s| s.set(before));
    r
}

/// Whether the input being dispatched comes from the driver.
pub fn is_synthesized() -> bool {
    SYNTHESIZING.with(Cell::get)
}

/// Sets or clears the marking of `window`.
pub fn set_marking(window: u64, marking: Option<Marking>) {
    MARKINGS.with(|m| {
        let mut m = m.borrow_mut();
        match marking {
            Some(k) => {
                m.insert(window, k);
            }
            None => {
                m.remove(&window);
            }
        }
    });
}

/// The marking of `window`, if a client drives it.
pub fn marking(window: u64) -> Option<Marking> {
    MARKINGS.with(|m| m.borrow().get(&window).cloned())
}

fn user_input(cx: &mut App) {
    if is_synthesized() {
        return;
    }
    let hook = HOOKS.with(|h| h.borrow().on_user_input.clone());
    if let Some(hook) = hook {
        hook(cx);
    }
}

/// The marking while an external client drives this window (behaviour 36): a 2-pixel frame in the
/// accent colour, a chip "Automated by <client>" with Stop, the highlight of the next target and
/// a marker at the last pointer action point. Drawn over everything without taking space; it
/// notes the user's own pointer input, which pauses the client (behaviour 37).
pub fn automation_overlay(window: &Window) -> Option<gpui::AnyElement> {
    let marking = marking(window_id(window))?;
    if marking.clients.is_empty() || marking.until.is_some_and(|u| u <= Instant::now()) {
        return None;
    }
    let p = crate::palette::Palette::current();
    let c = |color: idecraft_ui::theme::Rgba| -> gpui::Hsla {
        gpui::rgba((color.rgb << 8) | u32::from(color.alpha)).into()
    };
    let accent = c(p.accent);
    let accent_text = c(p.accent_text);
    let now = Instant::now();
    let mut overlay = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .border_2()
        .border_color(accent)
        .child(
            gpui::canvas(
                |_, _, _| {},
                |_, _, window, _| {
                    window.on_mouse_event(|_: &MouseDownEvent, _, _, cx| user_input(cx));
                    window.on_mouse_event(|_: &MouseMoveEvent, _, _, cx| user_input(cx));
                    window.on_mouse_event(|_: &ScrollWheelEvent, _, _, cx| user_input(cx));
                },
            )
            .absolute()
            .size_0(),
        );
    if let Some((r, until)) = marking.highlight
        && until > now
    {
        overlay = overlay.child(
            div()
                .absolute()
                .left(px(r.x - 2.))
                .top(px(r.y - 2.))
                .w(px(r.w + 4.))
                .h(px(r.h + 4.))
                .border_2()
                .border_color(accent)
                .rounded(px(3.)),
        );
    }
    if let Some(((x, y), until)) = marking.pointer
        && until > now
    {
        overlay = overlay.child(
            div()
                .absolute()
                .left(px(x - 6.))
                .top(px(y - 6.))
                .size(px(12.))
                .rounded_full()
                .border_2()
                .border_color(accent),
        );
    }
    let chip = div()
        .absolute()
        .bottom(px(28.))
        .right(px(12.))
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded(px(4.))
        .bg(accent)
        .text_color(accent_text)
        .text_size(px(12.))
        .child(format!("Automated by {}", marking.clients.join(", ")))
        .child(
            div()
                .id("ui-automation-stop")
                .px_2()
                .rounded(px(3.))
                .border_1()
                .border_color(accent_text)
                .cursor_pointer()
                .child("Stop")
                .on_click(|_, window, cx| emergency_stop(StopSource::StopButton, window, cx)),
        );
    Some(overlay.child(chip).into_any_element())
}
