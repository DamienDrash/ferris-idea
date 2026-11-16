//! The engine without a window (`SPEC-0319` AK-1, AK-3, AK-7, AK-10): locators, waiting on
//! frames, actionability, input planning and the async Rust API, fed with fixture trees.

use crate::common;

use std::time::{Duration, Instant};

use common::*;
use idecraft_uidriver::engine::{Command, Engine, Op, Request, Target};
use idecraft_uidriver::expect::Matcher;
use idecraft_uidriver::{
    Action, Button, ErrorCode, Input, Level, Locator, Modifier, Role, Semantics,
};

fn click() -> Action {
    Action::Click {
        button: Button::Left,
        count: 1,
        modifiers: Vec::new(),
        position: None,
    }
}

fn act(locator: &str, action: Action) -> Op {
    Op::Act {
        target: Target::Locator(Locator::parse(locator).unwrap()),
        action,
        level: None,
        force: false,
    }
}

/// AK-1: the exact name matches one button, the substring both; an action on two fails with
/// `ambiguous` listing both, and nothing is delivered.
#[test]
fn ak1_exact_names_match_one_and_substrings_are_ambiguous() {
    let t = tree(1, commit_window());
    let view = idecraft_uidriver::View::in_process();
    let exact = Locator::parse(r#"role=button[name="Commit"]"#).unwrap();
    assert_eq!(exact.resolve(&t, &view).unwrap().len(), 1);
    let mut engine = Engine::new();
    let start = Instant::now();
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(act(r#"role=button[name~="Commit"]"#, click())),
            start,
        )
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
            tree(f, commit_window())
        });
    let err = done.unwrap().reply.result.unwrap_err();
    assert_eq!(err.code, ErrorCode::Ambiguous);
    let names: Vec<_> = err.elements.iter().filter_map(|e| e.name.clone()).collect();
    assert_eq!(names, ["Commit", "Commit and Push"]);
    assert_eq!(delivered(&commands), 0);
    // Exact: one click on the Commit button's centre, at the window level.
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(act(r#"role=button[name="Commit"]"#, click())),
            start,
        )
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
            tree(f, commit_window())
        });
    let ok = done.unwrap().reply.result.unwrap();
    assert_eq!(ok["element"]["test_id"], "commit-button");
    let Some(Command::Deliver {
        inputs,
        level,
        origin,
        ..
    }) = commands
        .iter()
        .find(|c| matches!(c, Command::Deliver { .. }))
    else {
        panic!("nothing delivered");
    };
    assert_eq!(*level, Level::Window);
    assert_eq!(origin, "selftest");
    assert!(
        matches!(inputs[1], Input::PointerDown { x, y, button: Button::Left, .. } if x == 140. && y == 712.)
    );
}

/// AK-3: an element that appears after 300 ms is found within the frame it appears in; one that
/// never appears fails at its deadline naming the check.
#[test]
fn ak3_waits_resolve_on_the_frame_the_element_appears() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(Op::Expect {
                locator: Locator::parse("id=late").unwrap(),
                matcher: Matcher::Visible,
                not: false,
            }),
            start,
        )
        .unwrap();
    let mut appeared_at = None;
    let (done, _, at) = run_until_done(&mut engine, op, start, Duration::from_secs(2), |f, t| {
        let mut els = commit_window();
        if t >= Duration::from_millis(300) {
            appeared_at.get_or_insert(t);
            els.push(El(Semantics::marked("late"), r(0., 0., 10., 10.)));
        }
        tree(f, els)
    });
    assert!(done.unwrap().reply.result.is_ok());
    assert_eq!(
        Some(at),
        appeared_at,
        "resolved in the frame the element appeared in"
    );

    let op = engine
        .submit(
            WINDOW,
            Request::selftest(act("id=never", click())).with_timeout(Duration::from_millis(500)),
            start,
        )
        .unwrap();
    let (done, _, at) = run_until_done(&mut engine, op, start, Duration::from_secs(2), |f, _| {
        tree(f, commit_window())
    });
    let err = done.unwrap().reply.result.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(
        err.message.contains("deadline of 500 ms passed"),
        "{}",
        err.message
    );
    assert!(err.message.contains("attached"), "{}", err.message);
    assert!(at >= Duration::from_millis(500) && at < Duration::from_millis(600));
}

/// AK-7: a disabled button waits and fails with `not_enabled`; one under a popup fails with
/// `obscured` naming the popup; a moving one is clicked only after it stops; `force` clicks the
/// disabled one.
#[test]
fn ak7_actionability() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let short = |op| Request::selftest(op).with_timeout(Duration::from_millis(200));
    let op = engine
        .submit(WINDOW, short(act("id=disabled-button", click())), start)
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
            tree(f, commit_window())
        });
    let err = done.unwrap().reply.result.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotEnabled);
    assert_eq!(delivered(&commands), 0);

    // A popup painted over the Commit button.
    let with_popup = |f| {
        let mut els = commit_window();
        els.push(El(
            Semantics::new(Role::Menu)
                .name("Recent Messages")
                .test_id("message-popup"),
            r(50., 650., 300., 120.),
        ));
        tree(f, els)
    };
    let op = engine
        .submit(WINDOW, short(act("id=commit-button", click())), start)
        .unwrap();
    let (done, _, _) = run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
        with_popup(f)
    });
    let err = done.unwrap().reply.result.unwrap_err();
    assert_eq!(err.code, ErrorCode::Obscured);
    assert_eq!(err.elements[0].test_id.as_deref(), Some("message-popup"));

    // A button that moves for the first five frames.
    let moving = |f: u64| {
        let mut els = commit_window();
        let x = if f <= 5 { 400. + f as f32 * 10. } else { 460. };
        els.push(El(
            Semantics::new(Role::Button)
                .name("Moving")
                .test_id("moving")
                .action(idecraft_uidriver::AxAction::Click)
                .effects(&[idecraft_uidriver::Effect::None]),
            r(x, 100., 50., 20.),
        ));
        tree(f, els)
    };
    let op = engine
        .submit(WINDOW, Request::selftest(act("id=moving", click())), start)
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
            moving(f)
        });
    assert!(done.unwrap().reply.result.is_ok());
    let Some(Command::Deliver { inputs, .. }) = commands
        .iter()
        .find(|c| matches!(c, Command::Deliver { .. }))
    else {
        panic!("nothing delivered");
    };
    assert!(
        matches!(inputs[0], Input::PointerMove { x, .. } if x == 485.),
        "{inputs:?}"
    );

    // force: the disabled button gets its click.
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(Op::Act {
                target: Target::Locator(Locator::id("disabled-button")),
                action: click(),
                level: None,
                force: true,
            }),
            start,
        )
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
            tree(f, commit_window())
        });
    assert!(done.unwrap().reply.result.is_ok());
    assert_eq!(delivered(&commands), 1);
}

/// Handles go stale when their node goes; locators never do.
#[test]
fn element_handles_go_stale() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let t = tree(1, commit_window());
    let id = t
        .iter()
        .find(|n| n.test_id() == Some("commit-button"))
        .unwrap()
        .id;
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(Op::Act {
                target: Target::Element(id),
                action: click(),
                level: None,
                force: false,
            }),
            start,
        )
        .unwrap();
    let (done, _, _) = run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
        tree(
            f,
            commit_window()
                .into_iter()
                .filter(|e| e.0.test_id.as_deref() != Some("commit-button"))
                .collect(),
        )
    });
    assert_eq!(
        done.unwrap().reply.result.unwrap_err().code,
        ErrorCode::Stale
    );
}

/// AK-5 and AK-6 without a window: ⌥ combinations are keys with the modifier, never characters;
/// text is committed whole through the input handler.
#[test]
fn keys_and_text_are_planned_as_the_window_takes_them() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(Op::Act {
                target: Target::Window,
                action: Action::Press {
                    keys: "alt-f12 cmd-[".into(),
                },
                level: None,
                force: false,
            }),
            start,
        )
        .unwrap();
    let (_, commands, _) =
        run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
            tree(f, commit_window())
        });
    let Some(Command::Deliver { inputs, .. }) = commands
        .iter()
        .find(|c| matches!(c, Command::Deliver { .. }))
    else {
        panic!("nothing delivered");
    };
    assert_eq!(inputs.len(), 2);
    assert!(
        matches!(&inputs[0], Input::Key(k) if k.modifiers == [Modifier::Alt] && k.key == "f12")
    );
    let text = "@ö é ß 日本語 🦀";
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(act(
                "id=commit-message",
                Action::Type {
                    text: text.into(),
                    clear: false,
                },
            )),
            start,
        )
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
            tree(f, commit_window())
        });
    assert!(done.unwrap().reply.result.is_ok());
    let Some(Command::Deliver { inputs, .. }) = commands
        .iter()
        .find(|c| matches!(c, Command::Deliver { .. }))
    else {
        panic!("nothing delivered");
    };
    // A click puts the focus into the field, then the text arrives as written.
    assert!(matches!(inputs[0], Input::PointerMove { .. }));
    assert!(matches!(inputs.last(), Some(Input::Text { text: t }) if t == text));
}

/// AK-10: the async Rust API on the fixture, driven by frames, without a screen.
#[test]
fn ak10_async_api_without_a_screen() {
    use futures::task::LocalSpawnExt;
    use std::cell::Cell;
    use std::rc::Rc;

    let now = Rc::new(Cell::new(Instant::now()));
    let clock = now.clone();
    let driver = idecraft_uidriver::Driver::new(WINDOW, move || clock.get());
    let mut pool = futures::executor::LocalPool::new();
    let result = Rc::new(Cell::new(None));
    let d = driver.clone();
    let out = result.clone();
    pool.spawner()
        .spawn_local(async move {
            let commit = d.role(Role::Button).name("Commit");
            let clicked = commit.click().await.is_ok();
            let disabled = idecraft_uidriver::expect(d.id("disabled-button"))
                .to_be_disabled()
                .await
                .is_ok();
            let ambiguous = d
                .role(Role::Button)
                .name_contains("Commit")
                .click()
                .await
                .unwrap_err()
                .code;
            out.set(Some((clicked, disabled, ambiguous)));
        })
        .unwrap();
    let mut delivered = 0;
    for frame in 1..200u64 {
        pool.run_until_stalled();
        if result.get().is_some() {
            break;
        }
        now.set(now.get() + Duration::from_millis(16));
        delivered += driver
            .frame(&tree(frame, commit_window()), &FakeKeys)
            .iter()
            .filter(|c| matches!(c, Command::Deliver { .. }))
            .count();
    }
    assert_eq!(result.get(), Some((true, true, ErrorCode::Ambiguous)));
    assert_eq!(delivered, 1);
    assert!(!driver.waiting());
}

/// Snapshots: JSON and outline, limited, with `truncated`.
#[test]
fn snapshots_are_limited() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let op = engine
        .submit(
            WINDOW,
            Request::selftest(Op::Snapshot {
                locator: None,
                depth: 10,
                max_nodes: 3,
                text: true,
            }),
            start,
        )
        .unwrap();
    let (done, _, _) = run_until_done(&mut engine, op, start, Duration::from_secs(1), |f, _| {
        tree(f, commit_window())
    });
    let v = done.unwrap().reply.result.unwrap();
    let outline = v["outline"].as_str().unwrap();
    assert_eq!(outline.lines().count(), 3, "{outline}");
    assert!(
        outline.starts_with("window \"IDECraft\" #window"),
        "{outline}"
    );
    assert!(
        outline.contains("\n  button \"Commit\" #commit-button"),
        "{outline}"
    );
    assert_eq!(v["truncated"], 6);
}
