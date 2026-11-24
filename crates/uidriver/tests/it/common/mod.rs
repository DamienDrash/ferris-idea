//! Fixture trees and a frame loop for the engine tests: frames are fed by hand with a clock the
//! test moves, so nothing waits for real time.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use ferris_idea_uidriver::engine::{Command, Done, Engine, Keys, Op, Request};
use ferris_idea_uidriver::{
    Actor, Effect, Grants, KeyPress, Rect, Reserved, Role, Semantics, TextClass, Tree, TreeBuilder,
};

pub const WINDOW: u64 = 7;

pub fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

/// One element of a fixture: its semantics and where it is painted.
pub struct El(pub Semantics, pub Rect);

pub fn tree(frame: u64, els: Vec<El>) -> Tree {
    let mut b = TreeBuilder::new();
    let window = r(0., 0., 1280., 800.);
    for El(s, rect) in els {
        let visible = rect.intersect(&window);
        b.push(s, rect, visible);
    }
    b.finish(frame)
}

/// The commit window of the fixture: "Commit" (vcs-write), "Commit and Push" (vcs-write, remote),
/// a message field (write), a password field, a terminal with masked text, a reserved button and
/// one without declared effect.
pub fn commit_window() -> Vec<El> {
    vec![
        El(
            Semantics::new(Role::Window)
                .name("Ferris IDEA")
                .test_id("window")
                .overlay(false),
            r(0., 0., 1280., 800.),
        ),
        El(
            Semantics::new(Role::Button)
                .name("Commit")
                .test_id("commit-button")
                .action(ferris_idea_uidriver::AxAction::Click)
                .effects(&[Effect::VcsWrite]),
            r(100., 700., 80., 24.),
        ),
        El(
            Semantics::new(Role::Button)
                .name("Commit and Push")
                .test_id("commit-push-button")
                .action(ferris_idea_uidriver::AxAction::Click)
                .effects(&[Effect::VcsWrite, Effect::Remote]),
            r(200., 700., 140., 24.),
        ),
        El(
            Semantics::new(Role::MultilineTextInput)
                .name("Commit message")
                .test_id("commit-message")
                .value("Fix the parser")
                .effects(&[Effect::None]),
            r(100., 500., 600., 150.),
        ),
        El(
            Semantics::new(Role::PasswordInput)
                .name("Password")
                .test_id("connection-password")
                .value("hunter2-KNOWN-SECRET")
                .effects(&[Effect::None]),
            r(800., 100., 200., 24.),
        ),
        El(
            Semantics::new(Role::Terminal)
                .name("Terminal")
                .test_id("terminal-0")
                .value("export TOKEN=********")
                .text_class(TextClass::Terminal)
                .effects(&[Effect::Run]),
            r(0., 300., 600., 150.),
        ),
        El(
            Semantics::new(Role::Button)
                .name("Trust Project…")
                .test_id("trust-project")
                .action(ferris_idea_uidriver::AxAction::Click)
                .effects(&[Effect::Trust])
                .reserved(Reserved::Trust),
            r(800., 200., 120., 24.),
        ),
        El(
            Semantics::new(Role::Button)
                .name("Mystery")
                .test_id("mystery")
                .action(ferris_idea_uidriver::AxAction::Click),
            r(800., 300., 120., 24.),
        ),
        El(
            Semantics::new(Role::Button)
                .name("Disabled")
                .test_id("disabled-button")
                .enabled(false)
                .action(ferris_idea_uidriver::AxAction::Click)
                .effects(&[Effect::None]),
            r(800., 400., 120., 24.),
        ),
    ]
}

/// A keymap with a few bindings.
pub struct FakeKeys;

impl Keys for FakeKeys {
    fn actions_for(&self, key: &KeyPress) -> Vec<String> {
        match key.to_text().as_str() {
            "cmd-v" => vec!["$Paste".into()],
            "cmd-shift-v" => vec!["PasteMultiple".into()],
            "cmd-k" => vec!["CheckinProject".into()],
            "alt-f12" => vec!["ActivateTerminalToolWindow".into()],
            "cmd-1" => vec!["ActivateProjectToolWindow".into()],
            "cmd-[" => vec!["Back".into()],
            "cmd-alt-y" => vec!["Synchronize".into()],
            _ => Vec::new(),
        }
    }
}

pub fn external(grants: &[&str]) -> (Actor, Grants) {
    (
        Actor::External {
            client: "test-agent 1.0".into(),
            session: "s-1".into(),
        },
        Grants::of(grants),
    )
}

pub fn request(op: Op, actor: Actor, grants: Grants) -> Request {
    Request {
        op,
        actor,
        grants,
        timeout: Duration::from_secs(10),
        cancel: None,
    }
}

/// Feeds frames of `make(frame)` every 16 ms of the test clock until `op` is done or `limit`
/// passes; returns its answer and every command.
pub fn run_until_done(
    engine: &mut Engine,
    op: u64,
    start: Instant,
    limit: Duration,
    mut make: impl FnMut(u64, Duration) -> Tree,
) -> (Option<Done>, Vec<Command>, Duration) {
    let mut commands = Vec::new();
    let mut frame = 1u64;
    loop {
        let t = Duration::from_millis(16 * frame);
        let tree = make(frame, t);
        let out = engine.frame(WINDOW, &tree, start + t, &FakeKeys);
        commands.extend(out.commands);
        if let Some(d) = out.done.into_iter().find(|d| d.op == op) {
            return (Some(d), commands, t);
        }
        if t > limit {
            return (None, commands, t);
        }
        frame += 1;
    }
}

pub fn delivered(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|c| matches!(c, Command::Deliver { .. }))
        .count()
}
