//! The security tests of `SPEC-0319` that need no window, on the engine (UISEC-3 to UISEC-6 and
//! UISEC-10 to UISEC-14); the MCP side of UISEC-1 to UISEC-3, UISEC-7 and UISEC-13 is in
//! `crates/mcp/tests/it/ui_driver.rs`, the scans of UISEC-9 and AK-3 in `scans.rs`.

use crate::common;

use std::time::{Duration, Instant};

use common::*;
use idecraft_uidriver::engine::{CancelHandle, Command, Engine, Op, Target};
use idecraft_uidriver::expect::{Compare, Matcher};
use idecraft_uidriver::trace::{AuditFacts, audit_entry};
use idecraft_uidriver::{Action, Button, ErrorCode, Locator, StopSource};

const SECRET: &str = "hunter2-KNOWN-SECRET";

fn click(locator: &str) -> Op {
    Op::Act {
        target: Target::Locator(Locator::parse(locator).unwrap()),
        action: Action::Click {
            button: Button::Left,
            count: 1,
            modifiers: Vec::new(),
            position: None,
        },
        level: None,
        force: false,
    }
}

fn run_external(
    op: Op,
    grants: &[&str],
) -> (
    Result<serde_json::Value, idecraft_uidriver::UiError>,
    Vec<Command>,
) {
    let mut engine = Engine::new();
    let start = Instant::now();
    let (actor, g) = external(grants);
    let id = engine.submit(WINDOW, request(op, actor, g), start).unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, id, start, Duration::from_secs(2), |f, _| {
            tree(f, commit_window())
        });
    (done.expect("answered").reply.result, commands)
}

/// UISEC-3: without `ui` nothing goes; with `ui` but not `vcs-write` the Commit button answers
/// `not_granted` naming `vcs-write`; nothing is delivered either time.
#[test]
fn uisec3_missing_rights() {
    let (r, c) = run_external(click("id=commit-button"), &[]);
    let e = r.unwrap_err();
    assert_eq!(
        (e.code, e.right.as_deref()),
        (ErrorCode::NotGranted, Some("ui"))
    );
    assert_eq!(delivered(&c), 0);
    let (r, c) = run_external(click("id=commit-button"), &["ui"]);
    let e = r.unwrap_err();
    assert_eq!(
        (e.code, e.right.as_deref()),
        (ErrorCode::NotGranted, Some("vcs-write"))
    );
    assert_eq!(delivered(&c), 0);
    // Commit and Push needs both rights of its effects.
    let (r, _) = run_external(click("id=commit-push-button"), &["ui", "vcs-write"]);
    assert_eq!(r.unwrap_err().right.as_deref(), Some("remote"));
    let (r, c) = run_external(click("id=commit-button"), &["ui", "vcs-write"]);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(delivered(&c), 1);
}

/// UISEC-4: reserved elements and actions answer `reserved`, nothing is delivered.
#[test]
fn uisec4_reserved_for_the_user() {
    let all = &["ui", "write", "vcs-write", "run", "trust", "remote"];
    let (r, c) = run_external(click("id=trust-project"), all);
    assert_eq!(r.unwrap_err().code, ErrorCode::Reserved);
    assert_eq!(delivered(&c), 0);
    let typing = Op::Act {
        target: Target::Locator(Locator::id("connection-password")),
        action: Action::Type {
            text: "guess".into(),
            clear: false,
        },
        level: None,
        force: false,
    };
    let (r, c) = run_external(typing, all);
    assert_eq!(r.unwrap_err().code, ErrorCode::Reserved);
    assert_eq!(delivered(&c), 0);
    for keys in ["cmd-v", "ctrl-v", "shift-insert", "cmd-shift-v"] {
        let press = Op::Act {
            target: Target::Window,
            action: Action::Press { keys: keys.into() },
            level: None,
            force: false,
        };
        let (r, c) = run_external(press, all);
        assert_eq!(r.unwrap_err().code, ErrorCode::Reserved, "{keys}");
        assert_eq!(delivered(&c), 0);
    }
}

/// UISEC-5: an actionable element without declared effect, and a key whose action has none,
/// answer `undeclared`.
#[test]
fn uisec5_undeclared_effects() {
    let all = &["ui", "write", "vcs-write", "run", "remote", "debug", "quit"];
    let (r, c) = run_external(click("id=mystery"), all);
    assert_eq!(r.unwrap_err().code, ErrorCode::Undeclared);
    assert_eq!(delivered(&c), 0);
    let press = Op::Act {
        target: Target::Window,
        action: Action::Press {
            keys: "cmd-alt-y".into(),
        },
        level: None,
        force: false,
    };
    let (r, _) = run_external(press, all);
    assert_eq!(r.unwrap_err().code, ErrorCode::Undeclared);
    // A declared key goes: ⌘1 shows the Project window (effect none).
    let press = Op::Act {
        target: Target::Window,
        action: Action::Press {
            keys: "cmd-1".into(),
        },
        level: None,
        force: false,
    };
    let (r, c) = run_external(press, &["ui"]);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(delivered(&c), 1);
    // ⌥F12 opens a terminal: `run`.
    let press = Op::Act {
        target: Target::Window,
        action: Action::Press {
            keys: "alt-f12".into(),
        },
        level: None,
        force: false,
    };
    let (r, _) = run_external(press, &["ui"]);
    assert_eq!(r.unwrap_err().right.as_deref(), Some("run"));
}

/// UISEC-6: the password never appears in snapshots, finds, errors or traces; terminal text only
/// with `terminal-read`.
#[test]
fn uisec6_secrets_never_leave() {
    let mut texts = Vec::new();
    for (op, grants) in [
        (
            Op::Snapshot {
                locator: None,
                depth: 20,
                max_nodes: 100,
                text: false,
            },
            vec!["ui"],
        ),
        (
            Op::Snapshot {
                locator: None,
                depth: 20,
                max_nodes: 100,
                text: true,
            },
            vec!["ui"],
        ),
        (
            Op::Find {
                locator: Locator::parse("role=passwordinput").unwrap(),
                wait: false,
            },
            vec!["ui"],
        ),
        (
            Op::Expect {
                locator: Locator::id("connection-password"),
                matcher: Matcher::Value {
                    text: SECRET.into(),
                    compare: Compare::Exact,
                },
                not: false,
            },
            vec!["ui"],
        ),
    ] {
        let mut engine = Engine::new();
        let start = Instant::now();
        engine.trace_start(start);
        let (actor, g) = external(&grants);
        let mut req = request(op, actor, g);
        req.timeout = Duration::from_millis(100);
        let id = engine.submit(WINDOW, req, start).unwrap();
        let (done, _, _) =
            run_until_done(&mut engine, id, start, Duration::from_secs(1), |f, _| {
                tree(f, commit_window())
            });
        let reply = done.unwrap().reply;
        texts.push(match &reply.result {
            Ok(v) => v.to_string(),
            Err(e) => serde_json::to_string(e).unwrap(),
        });
        let trace = engine.trace_stop().unwrap();
        texts.push(serde_json::to_string(trace.entries()).unwrap());
    }
    for t in &texts {
        assert!(!t.contains(SECRET), "{t}");
        assert!(!t.contains("hunter2"), "{t}");
    }
    // The snapshot says the field is secret and not empty.
    assert!(texts[0].contains("\"secret\":true") && texts[0].contains("\"empty\":false"));
    // Terminal text: absent without `terminal-read`, masked with it.
    let snapshot = |grants: &[&str]| {
        let (r, _) = run_external(
            Op::Find {
                locator: Locator::id("terminal-0"),
                wait: false,
            },
            grants,
        );
        r.unwrap().to_string()
    };
    assert!(!snapshot(&["ui"]).contains("export TOKEN"));
    let read = snapshot(&["ui", "terminal-read"]);
    assert!(read.contains("export TOKEN=********"), "{read}");
}

/// UISEC-10: the emergency stop cancels a waiting assertion and a pending action at once, ends the
/// sessions and counts the stop; the self-test's calls go on.
#[test]
fn uisec10_emergency_stop() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let (actor, g) = external(&["ui"]);
    let waiting = engine
        .submit(
            WINDOW,
            request(
                Op::Expect {
                    locator: Locator::id("never"),
                    matcher: Matcher::Visible,
                    not: false,
                },
                actor.clone(),
                g.clone(),
            ),
            start,
        )
        .unwrap();
    let acting = engine
        .submit(WINDOW, request(click("id=never-either"), actor, g), start)
        .unwrap();
    let mine = engine
        .submit(
            WINDOW,
            idecraft_uidriver::Request::selftest(Op::Expect {
                locator: Locator::id("commit-button"),
                matcher: Matcher::Visible,
                not: false,
            }),
            start,
        )
        .unwrap();
    let out = engine.frame(WINDOW, &tree(1, commit_window()), start, &FakeKeys);
    // The self-test's assertion holds at once; the external ones wait.
    assert_eq!(out.done.iter().map(|d| d.op).collect::<Vec<_>>(), [mine]);
    assert_eq!(engine.supervisor.drivers_of(WINDOW).len(), 1);
    let before = idecraft_uidriver::security::stop_generation();
    let (done, ended) = engine.stop(StopSource::Escape);
    let ids: Vec<_> = done.iter().map(|d| d.op).collect();
    assert_eq!(ids, [waiting, acting]);
    assert!(
        done.iter()
            .all(|d| d.reply.result.as_ref().unwrap_err().code == ErrorCode::Cancelled)
    );
    assert_eq!(ended.len(), 1);
    assert!(engine.supervisor.drivers().is_empty());
    assert!(idecraft_uidriver::security::stop_generation() > before);
    assert_eq!(engine.pending(), 0);
    // A cancellation handle ends one call at the next frame.
    let cancel = CancelHandle::new();
    let (actor, g) = external(&["ui"]);
    let mut req = request(click("id=never"), actor, g);
    req.cancel = Some(cancel.clone());
    let id = engine.submit(WINDOW, req, start).unwrap();
    cancel.cancel();
    let out = engine.frame(WINDOW, &tree(2, commit_window()), start, &FakeKeys);
    assert_eq!(out.done[0].op, id);
    assert_eq!(
        out.done[0].reply.result.as_ref().unwrap_err().code,
        ErrorCode::Cancelled
    );
}

/// UISEC-11: the user's input pauses external clients for 3 s; nothing of theirs is delivered
/// meanwhile.
#[test]
fn uisec11_user_input_wins() {
    let mut engine = Engine::new();
    let start = Instant::now();
    engine.user_input(start);
    let (actor, g) = external(&["ui", "vcs-write"]);
    let id = engine
        .submit(
            WINDOW,
            request(click("id=commit-button"), actor.clone(), g.clone()),
            start,
        )
        .unwrap();
    let out = engine.frame(
        WINDOW,
        &tree(1, commit_window()),
        start + Duration::from_secs(1),
        &FakeKeys,
    );
    assert_eq!(out.done[0].op, id);
    assert_eq!(
        out.done[0].reply.result.as_ref().unwrap_err().code,
        ErrorCode::Interrupted
    );
    assert!(out.commands.is_empty());
    // 3 s after the last input the client goes on.
    let later = start + Duration::from_millis(3100);
    let id = engine
        .submit(WINDOW, request(click("id=commit-button"), actor, g), later)
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, id, later, Duration::from_secs(2), |f, _| {
            tree(f, commit_window())
        });
    assert!(done.unwrap().reply.result.is_ok());
    assert_eq!(delivered(&commands), 1);
}

/// UISEC-12, the part without a window: an external action is highlighted for 300 ms before its
/// input is delivered, and the marking knows the client.
#[test]
fn uisec12_highlight_before_input() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let (actor, g) = external(&["ui", "vcs-write"]);
    let id = engine
        .submit(WINDOW, request(click("id=commit-button"), actor, g), start)
        .unwrap();
    let mut highlighted_at = None;
    let mut delivered_at = None;
    for f in 1..100u64 {
        let t = Duration::from_millis(16 * f);
        let out = engine.frame(WINDOW, &tree(f, commit_window()), start + t, &FakeKeys);
        for c in &out.commands {
            match c {
                Command::Highlight { bounds, point, .. } => {
                    assert!(bounds.is_some() && point.is_some());
                    highlighted_at.get_or_insert(t);
                }
                Command::Deliver { origin, .. } => {
                    assert_eq!(origin, "driver:test-agent 1.0");
                    delivered_at.get_or_insert(t);
                }
            }
        }
        if f == 2 {
            let drivers = engine.supervisor.drivers_of(WINDOW);
            assert_eq!(drivers[0].client, "test-agent 1.0");
        }
        if out.done.iter().any(|d| d.op == id) {
            break;
        }
    }
    let (h, d) = (highlighted_at.unwrap(), delivered_at.unwrap());
    assert!(d >= h + Duration::from_millis(300), "{h:?} {d:?}");
}

/// UISEC-13: typing a known text leaves only its length in the trace and the audit entry.
#[test]
fn uisec13_audit_and_trace_without_content() {
    const TYPED: &str = "the-KNOWN-typed-text";
    let mut engine = Engine::new();
    let start = Instant::now();
    engine.trace_start(start);
    let (actor, g) = external(&["ui"]);
    let op = Op::Act {
        target: Target::Locator(Locator::id("commit-message")),
        action: Action::Type {
            text: TYPED.into(),
            clear: true,
        },
        level: None,
        force: false,
    };
    let typed_len = match &op {
        Op::Act { action, .. } => action.typed_len(),
        _ => None,
    };
    let id = engine.submit(WINDOW, request(op, actor, g), start).unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, id, start, Duration::from_secs(2), |f, _| {
            tree(f, commit_window())
        });
    let reply = done.unwrap().reply;
    assert!(reply.result.is_ok(), "{:?}", reply.result);
    // The text was delivered…
    assert!(commands.iter().any(|c| matches!(c, Command::Deliver { inputs, .. }
        if inputs.iter().any(|i| matches!(i, idecraft_uidriver::Input::Text { text } if text == TYPED)))));
    // …but appears in no record.
    let trace = serde_json::to_string(engine.trace_stop().unwrap().entries()).unwrap();
    assert!(!trace.contains("KNOWN"), "{trace}");
    assert!(trace.contains("\"length\":20"), "{trace}");
    let audit = audit_entry(
        "test-agent 1.0",
        "s-1",
        "ui_act",
        Some("id=commit-message"),
        &AuditFacts {
            element: reply.facts.element.clone(),
            effects: reply.facts.effects.clone(),
        },
        "ok",
        Duration::from_millis(5),
        typed_len,
    )
    .to_string();
    assert!(!audit.contains("KNOWN"), "{audit}");
    assert!(audit.contains("\"typed_length\":20"), "{audit}");
    assert!(audit.contains("commit-message"));
}

/// The question the toolkit draws where the platform has no alert (SPEC-0281 v4 behaviour 9), as
/// the workbench records it: a cover over the window, the modal, reserved `alertdialog` and its
/// reserved answers, Deny focused.
fn drawn_question() -> Vec<El> {
    use idecraft_uidriver::{Reserved, Role, Semantics};
    let answer = |ix: usize, name: &str, y: f32| {
        let mut s = Semantics::new(Role::Button)
            .name(name)
            .test_id(format!("native-prompt-answer-{ix}"))
            .action(idecraft_uidriver::AxAction::Click)
            .reserved(Reserved::Decision);
        if ix == 0 {
            s = s.focused(true).keyboard_shortcut("Enter");
        }
        El(s, r(500., y, 280., 24.))
    };
    vec![
        El(
            Semantics::new(Role::GenericContainer)
                .test_id("native-prompt-cover")
                .overlay(true),
            r(0., 0., 1280., 800.),
        ),
        El(
            Semantics::new(Role::AlertDialog)
                .name("Allow \"test-agent 1.0\" the right \"write\" in workspace \"shop\"?")
                .test_id("native-prompt")
                .modal()
                .reserved(Reserved::Decision),
            r(490., 300., 300., 160.),
        ),
        answer(0, "Deny", 360.),
        answer(1, "Allow Once", 390.),
        answer(2, "Always for This Workspace", 420.),
    ]
}

fn press(keys: &str) -> Op {
    Op::Act {
        target: Target::Window,
        action: Action::Press { keys: keys.into() },
        level: None,
        force: false,
    }
}

/// UISEC-14 (SPEC-0319 v6 behaviour 35): while the question the toolkit draws is open, no action
/// of an external client goes — not a key into the window (⌘1 alone would go), not a click on an
/// answer or on a control behind the question — whatever rights it has; the self-test still acts.
#[test]
fn uisec14_nothing_reaches_a_window_while_a_drawn_question_is_open() {
    let all = &["ui", "write", "vcs-write", "run", "remote", "trust"];
    let with_question = |f: u64| {
        let mut els = commit_window();
        els.extend(drawn_question());
        tree(f, els)
    };
    for op in [
        press("cmd-1"),
        press("enter"),
        press("tab space"),
        click("id=native-prompt-answer-2"),
        click("id=commit-button"),
    ] {
        let mut engine = Engine::new();
        let start = Instant::now();
        let (actor, g) = external(all);
        let id = engine.submit(WINDOW, request(op, actor, g), start).unwrap();
        let (done, commands, _) =
            run_until_done(&mut engine, id, start, Duration::from_secs(2), |f, _| {
                with_question(f)
            });
        let e = done.expect("answered").reply.result.unwrap_err();
        assert_eq!(e.code, ErrorCode::Reserved, "{e:?}");
        assert!(e.message.contains("native-prompt"), "{}", e.message);
        assert_eq!(delivered(&commands), 0, "nothing reaches the question");
    }
    // The tree names the question and its focused safe answer.
    let t = with_question(1);
    let modal = t
        .reserved_modal()
        .expect("the question is the reserved modal");
    assert_eq!(modal.test_id(), Some("native-prompt"));
    assert_eq!(t.focused().and_then(|n| n.name()), Some("Deny"));
    // The in-process self-test is not refused.
    let mut engine = Engine::new();
    let start = Instant::now();
    let id = engine
        .submit(
            WINDOW,
            request(
                press("enter"),
                idecraft_uidriver::Actor::SelfTest,
                idecraft_uidriver::Grants::default(),
            ),
            start,
        )
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, id, start, Duration::from_secs(2), |f, _| {
            with_question(f)
        });
    assert!(done.expect("answered").reply.result.is_ok());
    assert_eq!(delivered(&commands), 1);
}

/// UISEC-14: a question that opens while a press waits for its highlight — one the client's own
/// call may have caused — gets none of its keys.
#[test]
fn uisec14_a_question_that_opens_during_the_highlight_gets_no_key() {
    let mut engine = Engine::new();
    let start = Instant::now();
    let (actor, g) = external(&["ui"]);
    let id = engine
        .submit(WINDOW, request(press("cmd-1"), actor, g), start)
        .unwrap();
    let (done, commands, _) =
        run_until_done(&mut engine, id, start, Duration::from_secs(2), |f, t| {
            let mut els = commit_window();
            if t >= Duration::from_millis(100) {
                els.extend(drawn_question());
            }
            tree(f, els)
        });
    let e = done.expect("answered").reply.result.unwrap_err();
    assert_eq!(e.code, ErrorCode::Reserved, "{e:?}");
    assert_eq!(delivered(&commands), 0);
    let highlighted = commands
        .iter()
        .any(|c| matches!(c, Command::Highlight { .. }));
    assert!(
        highlighted,
        "the press was planned before the question opened"
    );
}
