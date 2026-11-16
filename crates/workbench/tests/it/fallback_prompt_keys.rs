//! `SPEC-0281` v4 behaviour 9, AK-9: the question gpui draws where the platform has no alert of its
//! own (Linux), on the toolkit's test platform with gpui's own fallback renderer as the prompt
//! builder — the same code that draws the question on X11 and Wayland. Return gives the default,
//! Escape the cancel answer, Space the focused answer, which starts on the safe answer; Tab and
//! Shift+Tab move the focus; held keys, keys with Control or the platform key and typing never give
//! more than the safe answer; no key and no click reaches the window behind; the focus stays in the
//! question; the question is in the semantic tree, modal and reserved for the user.
//!
//! The real Linux window (X11 on Xvfb) runs the same questions in the window self-test, group
//! `prompt-keys` (AK-10).

#![cfg(target_os = "macos")]

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use gpui::{
    Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, Keystroke,
    Modifiers, MouseButton, ParentElement, PromptButton, PromptLevel, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, point, px,
};
use idecraft_workbench::semantics::{self, Reserved, Role};
use idecraft_workbench::trust_model;

/// The window behind the question: it counts the keys and clicks that reach it and has two places
/// that take the focus.
struct Behind {
    main: FocusHandle,
    other: FocusHandle,
    keys: Rc<Cell<usize>>,
    clicks: Rc<Cell<usize>>,
}

impl Render for Behind {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        semantics::begin_frame(window);
        let keys = self.keys.clone();
        let clicks = self.clicks.clone();
        div()
            .size_full()
            .track_focus(&self.main)
            .on_key_down(move |_: &KeyDownEvent, _, _| keys.set(keys.get() + 1))
            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                clicks.set(clicks.get() + 1)
            })
            .child(div().size_full().track_focus(&self.other))
            .children(semantics::frame_end())
    }
}

fn window(cx: &mut TestAppContext) -> (Entity<Behind>, &'static mut VisualTestContext) {
    // The builder Linux uses: gpui's fallback renderer.
    cx.update(|cx| cx.set_prompt_builder(gpui::fallback_prompt_renderer));
    let handle = cx.add_window(|window, cx| {
        let behind = Behind {
            main: cx.focus_handle(),
            other: cx.focus_handle(),
            keys: Rc::default(),
            clicks: Rc::default(),
        };
        window.focus(&behind.main);
        behind
    });
    let view = handle.root(cx).unwrap();
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1000.), px(700.)));
    cx.run_until_parked();
    (view, cx)
}

/// Asks the question with `answers`; the slot gets the index of the answer once one is given.
fn ask(answers: &[PromptButton], cx: &mut VisualTestContext) -> Rc<RefCell<Option<usize>>> {
    let slot = Rc::new(RefCell::new(None));
    let reply = cx.update(|window, cx| {
        window.prompt(
            PromptLevel::Warning,
            "Question?",
            Some("What the question is about"),
            answers,
            cx,
        )
    });
    let given = slot.clone();
    cx.foreground_executor()
        .spawn(async move {
            if let Ok(ix) = reply.await {
                *given.borrow_mut() = Some(ix);
            }
        })
        .detach();
    cx.run_until_parked();
    assert!(shown(cx), "the toolkit draws the question");
    slot
}

fn shown(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, _| window.fallback_prompt().is_some())
}

/// The label of the answer that holds the keyboard focus.
fn focused(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|window, cx| {
        let prompt = window.fallback_prompt()?;
        let prompt = prompt.read(cx);
        prompt
            .focused_answer()
            .map(|ix| prompt.answers()[ix].label().to_string())
    })
}

/// An agent's request for a right: Deny first, named safe (`mcp_http::permission_buttons`).
fn permission() -> Vec<PromptButton> {
    vec![
        PromptButton::safe("Deny"),
        PromptButton::new("Allow Once"),
        PromptButton::new("Always for This Workspace"),
    ]
}

/// Delete in the project tree (`workbench_menus::ask_delete`).
fn delete() -> Vec<PromptButton> {
    vec![PromptButton::ok("Delete"), PromptButton::cancel("Cancel")]
}

/// The trust question as `WorkbenchView::ask_trust` asks it.
fn trust(opening: bool) -> Vec<PromptButton> {
    let root = Path::new("/work/projects/shop");
    trust_model::prompt_buttons(&trust_model::answers(root, opening))
}

fn key_held(key: &str, cx: &mut VisualTestContext) {
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse(key).unwrap(),
        is_held: true,
    });
}

#[gpui::test]
fn the_focus_starts_on_the_safe_answer_else_the_cancel_answer_else_the_default(
    cx: &mut TestAppContext,
) {
    let (_, cx) = window(cx);
    for (answers, expected) in [
        (permission(), "Deny"),
        (trust(true), "Preview in Safe Mode"),
        (trust(false), "Preview in Safe Mode"),
        (delete(), "Cancel"),
        (
            vec![PromptButton::ok("OK"), PromptButton::new("Copy")],
            "OK",
        ),
        (
            vec![PromptButton::cancel("Cancel"), PromptButton::new("Run")],
            "Cancel",
        ),
    ] {
        let answer = ask(&answers, cx);
        assert_eq!(focused(cx).as_deref(), Some(expected));
        cx.simulate_keystrokes("enter");
        assert_eq!(*answer.borrow(), Some(0), "Return gives the first answer");
        assert!(!shown(cx));
    }
}

#[gpui::test]
fn return_gives_the_default_escape_the_cancel_answer_and_space_the_focused_one(
    cx: &mut TestAppContext,
) {
    let (view, cx) = window(cx);
    // Delete in the project tree: Return deletes, Escape and Space keep the file.
    for (keys, expected) in [
        ("enter", 0),
        ("escape", 1),
        ("space", 1),
        ("tab space", 0),
        ("tab tab space", 1),
    ] {
        let answer = ask(&delete(), cx);
        cx.simulate_keystrokes(keys);
        assert_eq!(*answer.borrow(), Some(expected), "{keys}");
        assert!(!shown(cx), "{keys}");
    }
    // The trust question on opening: Return and Space preview in safe mode, Escape is Don't Open.
    for (keys, expected) in [
        ("enter", "Preview in Safe Mode"),
        ("space", "Preview in Safe Mode"),
        ("escape", "Don't Open"),
    ] {
        let answers = trust(true);
        let answer = ask(&answers, cx);
        cx.simulate_keystrokes(keys);
        let given = answer.borrow().map(|ix| answers[ix].label().to_string());
        assert_eq!(given.as_deref(), Some(expected), "{keys}");
    }
    // An agent's request for a right: Return and Space deny.
    for keys in ["enter", "space"] {
        let answer = ask(&permission(), cx);
        cx.simulate_keystrokes(keys);
        assert_eq!(*answer.borrow(), Some(0), "{keys} denies");
    }
    assert_eq!(
        view.read_with(cx, |b, _| b.keys.get()),
        0,
        "no key reached the window"
    );
}

#[gpui::test]
fn escape_without_a_cancel_answer_leaves_the_question_open(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    for answers in [permission(), trust(false)] {
        let answer = ask(&answers, cx);
        cx.simulate_keystrokes("escape");
        assert!(shown(cx), "Escape gives no answer");
        assert_eq!(*answer.borrow(), None);
        cx.simulate_keystrokes("enter");
        assert_eq!(*answer.borrow(), Some(0));
    }
}

#[gpui::test]
fn tab_and_shift_tab_move_the_focus_and_wrap_around(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    let answer = ask(&permission(), cx);
    for (keys, expected) in [
        ("tab", "Allow Once"),
        ("tab", "Always for This Workspace"),
        ("tab", "Deny"),
        ("shift-tab", "Always for This Workspace"),
        ("shift-tab", "Allow Once"),
    ] {
        cx.simulate_keystrokes(keys);
        assert_eq!(focused(cx).as_deref(), Some(expected), "{keys}");
    }
    // Return gives the default wherever the focus is, as on macOS.
    cx.simulate_keystrokes("enter");
    assert_eq!(*answer.borrow(), Some(0), "Return denies");

    // Tab, then Space: the deliberate answer.
    let answer = ask(&permission(), cx);
    cx.simulate_keystrokes("tab space");
    assert_eq!(*answer.borrow(), Some(1), "Allow Once after Tab");
}

#[gpui::test]
fn no_stray_key_grants(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    let answer = ask(&permission(), cx);
    // Held keys answer nothing: a key that was down when the question appeared.
    key_held("enter", cx);
    key_held("space", cx);
    key_held("tab", cx);
    assert!(shown(cx));
    assert_eq!(*answer.borrow(), None);
    assert_eq!(
        focused(cx).as_deref(),
        Some("Deny"),
        "a held Tab moves nothing"
    );
    // Keys with Control, Alt or the platform key answer nothing.
    cx.simulate_keystrokes("tab");
    cx.simulate_keystrokes("cmd-enter ctrl-space alt-enter cmd-tab");
    assert!(shown(cx));
    assert_eq!(*answer.borrow(), None);
    assert_eq!(focused(cx).as_deref(), Some("Allow Once"));
    // Typing moves the focus back to the safe answer.
    cx.simulate_input("if x");
    assert_eq!(
        focused(cx).as_deref(),
        Some("Deny"),
        "typing resets the focus"
    );
    // Typing on into a question that appeared: Tab for an indent, a word, Space.
    cx.simulate_keystrokes("tab");
    cx.simulate_input("let");
    cx.simulate_keystrokes("space");
    assert_eq!(*answer.borrow(), Some(0), "at most the safe answer: Deny");
    assert_eq!(
        view.read_with(cx, |b, _| b.keys.get()),
        0,
        "no key reached the window"
    );
}

#[gpui::test]
fn the_focus_stays_in_the_question_and_goes_where_it_was_asked_for_after_it(
    cx: &mut TestAppContext,
) {
    let (view, cx) = window(cx);
    let (main, other) = view.read_with(cx, |b, _| (b.main.clone(), b.other.clone()));
    let answer = ask(&delete(), cx);
    cx.update(|window, _| window.focus(&other));
    cx.run_until_parked();
    assert!(
        !cx.update(|window, _| other.is_focused(window) || main.is_focused(window)),
        "the question keeps the focus"
    );
    cx.simulate_keystrokes("space");
    assert_eq!(
        *answer.borrow(),
        Some(1),
        "Space reached the question: Cancel"
    );
    assert!(
        cx.update(|window, _| other.is_focused(window)),
        "the view that asked for the focus has it now"
    );
    // Without a request in between, the focus goes back to where it was.
    let answer = ask(&delete(), cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(*answer.borrow(), Some(1));
    assert!(cx.update(|window, _| other.is_focused(window)));
}

#[gpui::test]
fn the_pointer_reaches_only_the_question_and_the_answers_stand_in_order(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    let answers = trust(true);
    let answer = ask(&answers, cx);
    let bounds: Vec<_> = cx.update(|window, cx| {
        let prompt = window.fallback_prompt().unwrap();
        let prompt = prompt.read(cx);
        (0..answers.len())
            .map(|ix| prompt.answer_bounds(ix).expect("painted"))
            .collect()
    });
    for pair in bounds.windows(2) {
        assert!(
            pair[0].origin.y < pair[1].origin.y,
            "the answers stand top to bottom in the order given: {bounds:?}"
        );
    }
    // A click on the cover reaches nothing behind it and answers nothing.
    cx.simulate_click(point(px(10.), px(10.)), Modifiers::none());
    assert_eq!(view.read_with(cx, |b, _| b.clicks.get()), 0);
    assert!(shown(cx));
    // A click on an answer gives it.
    let at = bounds[0].center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    assert_eq!(*answer.borrow(), Some(0));
    assert_eq!(view.read_with(cx, |b, _| b.clicks.get()), 0);
}

#[gpui::test]
fn the_question_is_in_the_semantic_tree_modal_and_reserved(cx: &mut TestAppContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let (_, cx) = window(cx);
    let answer = ask(&permission(), cx);
    let tree = |cx: &mut VisualTestContext| {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        cx.update(|window, _| semantics::tree(semantics::window_id(window)))
            .expect("a tree is built")
    };
    let t = tree(cx);
    let dialog = t
        .iter()
        .find(|n| n.test_id() == Some(semantics::DRAWN_PROMPT))
        .expect("the question is in the tree");
    assert_eq!(dialog.role(), Role::AlertDialog);
    assert_eq!(dialog.name(), Some("Question?"));
    assert_eq!(
        dialog.node.description(),
        Some("What the question is about")
    );
    assert!(dialog.node.is_modal());
    assert_eq!(dialog.reserved, Some(Reserved::Decision));
    assert_eq!(
        t.reserved_modal().map(|n| n.id),
        Some(dialog.id),
        "the reserved modal the driver refuses to act into"
    );
    for (ix, name) in ["Deny", "Allow Once", "Always for This Workspace"]
        .iter()
        .enumerate()
    {
        let id = semantics::drawn_prompt_answer(ix);
        let button = t
            .iter()
            .find(|n| n.test_id() == Some(id.as_str()))
            .unwrap_or_else(|| panic!("answer {ix} in the tree"));
        assert_eq!(button.role(), Role::Button);
        assert_eq!(button.name(), Some(*name));
        assert_eq!(button.reserved, Some(Reserved::Decision));
        assert!(
            t.is_ancestor_or_self(dialog.id, button.id),
            "inside the question"
        );
        assert!(button.visible());
    }
    assert_eq!(t.focused().and_then(|n| n.name()), Some("Deny"));
    assert_eq!(
        t.focused().and_then(|n| n.node.keyboard_shortcut()),
        Some("Enter")
    );
    cx.simulate_keystrokes("tab");
    let t = tree(cx);
    assert_eq!(t.focused().and_then(|n| n.name()), Some("Allow Once"));
    cx.simulate_keystrokes("enter");
    assert_eq!(*answer.borrow(), Some(0));
    let t = tree(cx);
    assert!(
        t.iter()
            .all(|n| n.test_id() != Some(semantics::DRAWN_PROMPT)),
        "the answered question leaves the tree"
    );
    assert!(t.reserved_modal().is_none());

    // Delete: the default carries Enter, the cancel answer Escape.
    let _answer = ask(&delete(), cx);
    let t = tree(cx);
    let shortcut = |ix: usize| {
        let id = semantics::drawn_prompt_answer(ix);
        t.iter()
            .find(|n| n.test_id() == Some(id.as_str()))
            .and_then(|n| n.node.keyboard_shortcut().map(str::to_owned))
    };
    assert_eq!(shortcut(0).as_deref(), Some("Enter"));
    assert_eq!(shortcut(1).as_deref(), Some("Escape"));
    cx.simulate_keystrokes("escape");
}
