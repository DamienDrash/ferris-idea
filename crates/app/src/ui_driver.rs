//! The window's host of the UI driver (`SPEC-0319`): it takes the calls the window's MCP server
//! passes on (already checked for the setting and the right `ui`), feeds the engine with the tree
//! of every frame a window presents, delivers the input the engine plans into that window only,
//! and shows the marking while an external client drives (behaviour 36). The user's own keys and
//! pointer pause external clients, Esc stops them (behaviour 37).
//!
//! Waiting is driven by frames: a waiting call asks its window for another frame, never for a
//! timer.

use std::collections::HashMap;
use std::time::Instant;

use gpui::{App, Global, Keystroke, Modifiers, Window};
use idecraft_mcp::window::{UiCall, UiCallKind};
use idecraft_uidriver::engine::{Command, Engine, Keys, Op, OpId};
use idecraft_uidriver::trace::AuditFacts;
use idecraft_uidriver::{
    Actor, Button, ErrorCode, Input, KeyPress, Level, Modifier, Reply, StopSource, UiError,
};
use idecraft_workbench::semantics::{self, Consumer};
use serde_json::json;
use tokio::sync::oneshot;

/// How long the marking stays after a client's last call.
const MARKING_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

struct Host {
    engine: Engine,
    replies: HashMap<OpId, oneshot::Sender<Reply>>,
}

impl Global for Host {}

/// The keymap in use, as the engine asks it (behaviour 35: effects come from action ids).
pub struct KeymapKeys;

impl Keys for KeymapKeys {
    fn actions_for(&self, key: &KeyPress) -> Vec<String> {
        let has = |m| key.modifiers.contains(&m);
        let Some(stroke) = idecraft_ui::keymap::Keystroke::from_platform(
            &key.key,
            has(Modifier::Ctrl),
            has(Modifier::Alt),
            has(Modifier::Shift),
            has(Modifier::Cmd),
        ) else {
            return Vec::new();
        };
        idecraft_workbench::keymaps::current()
            .actions_for(&stroke, None)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
}

/// Sets up the host: the frame hook, the emergency stop and the user's own input.
pub fn install(cx: &mut App) {
    cx.set_global(Host {
        engine: Engine::new(),
        replies: HashMap::new(),
    });
    semantics::on_frame(on_frame);
    semantics::on_stop(|source, _, cx| {
        stop(source, cx);
    });
    semantics::on_user_input(|cx| {
        if cx.has_global::<Host>() {
            let h = cx.global_mut::<Host>();
            h.engine.user_input(Instant::now());
        }
    });
    // Esc pressed by the user while a client drives stops it; any other key pauses it.
    cx.intercept_keystrokes(|event, window, cx| {
        if semantics::is_synthesized() {
            return;
        }
        if semantics::marking(semantics::window_id(window)).is_none() {
            return;
        }
        let k = &event.keystroke;
        if k.key == "escape" && k.modifiers == Modifiers::default() {
            semantics::emergency_stop(StopSource::Escape, window, cx);
        } else if cx.has_global::<Host>() {
            let h = cx.global_mut::<Host>();
            h.engine.user_input(Instant::now());
        }
    })
    .detach();
}

fn send(reply: oneshot::Sender<Reply>, result: Result<serde_json::Value, UiError>) {
    let _ = reply.send(Reply {
        result,
        facts: AuditFacts::default(),
    });
}

/// Serves one call from the MCP server.
pub fn handle(call: UiCall, reply: oneshot::Sender<Reply>, cx: &mut App) {
    if cx.try_global::<Host>().is_none() {
        return send(
            reply,
            Err(UiError::new(
                ErrorCode::Unavailable,
                "unavailable: no UI driver in this window",
                "",
            )),
        );
    }
    retire_idle(cx);
    let external = idecraft_workbench::appearance::ide_settings(cx)
        .ui
        .driver
        .external;
    let now = Instant::now();
    match call.kind {
        UiCallKind::Status => {
            let mut v = cx.global::<Host>().engine.status(now);
            v["external"] = json!(external);
            send(reply, Ok(v))
        }
        UiCallKind::Stop(source) => {
            let ended = stop(source, cx);
            send(reply, Ok(json!({"stopped": true, "sessions_ended": ended})))
        }
        // The window checks the setting too (behaviour 32).
        _ if !external => send(reply, Err(UiError::driving_off())),
        UiCallKind::Screenshot => send(
            reply,
            Err(UiError::new(
                ErrorCode::Unsupported,
                "unsupported: screenshots need a read-back of the window's frame, which GPUI 0.2.2 does not give",
                "read the tree with ui_snapshot; the screen-recording permission is never requested",
            )),
        ),
        UiCallKind::TraceStart => {
            let started = cx.global_mut::<Host>().engine.trace_start(now);
            send(reply, Ok(json!({"started": started, "running": true})))
        }
        UiCallKind::TraceStatus => {
            let entries = cx.global::<Host>().engine.trace_entries();
            send(
                reply,
                Ok(json!({"running": entries.is_some(), "entries": entries})),
            )
        }
        UiCallKind::TraceStop { dir } => {
            let Some(trace) = cx.global_mut::<Host>().engine.trace_stop() else {
                return send(reply, Err(UiError::invalid("invalid: no trace runs")));
            };
            let Some(base) = dir.or_else(idecraft_core::user_config_dir) else {
                return send(
                    reply,
                    Err(UiError::invalid("invalid: no folder to write the trace to")),
                );
            };
            match trace.write(&base) {
                Ok(file) => send(
                    reply,
                    Ok(json!({"file": file, "entries": trace.entries().len()})),
                ),
                Err(e) => send(
                    reply,
                    Err(UiError::new(
                        ErrorCode::Internal,
                        format!("internal: the trace could not be written: {e}"),
                        "",
                    )),
                ),
            }
        }
        UiCallKind::Driver(request) => {
            // Only windows of the client's own project are reachable (behaviour 34, UISEC-7).
            let Some(handle) =
                idecraft_workbench::project_windows::project_window(&call.workspace, cx)
                    .filter(|h| call.window.is_none_or(|w| w == h.window_id().as_u64()))
            else {
                return send(
                    reply,
                    Err(UiError::new(
                        ErrorCode::NotFound,
                        "not_found: no window of this project",
                        "a client reaches only the windows of its own project's route",
                    )),
                );
            };
            // While Settings is open, its reserved pages could be operated with keys alone.
            let settings_open = idecraft_workbench::project_windows::workbenches(cx)
                .into_iter()
                .any(|b| {
                    same_folder(b.read(cx).project_root(), &call.workspace)
                        && b.read(cx).settings_open()
                });
            if settings_open && matches!(request.op, Op::Act { .. }) && request.actor.is_external()
            {
                return send(
                    reply,
                    Err(UiError::new(
                        ErrorCode::Reserved,
                        "reserved: the Settings dialog is open; its pages for agent rights, MCP and UI automation are reserved for the user",
                        "the user closes Settings first; nothing was delivered",
                    )),
                );
            }
            // While the question the toolkit draws is open (Linux), its answers are the user's
            // (SPEC-0319 v6 behaviour 35).
            let question_open = handle
                .update(cx, |_, w, _| w.has_drawn_prompt())
                .unwrap_or(false);
            if question_open && matches!(request.op, Op::Act { .. }) && request.actor.is_external()
            {
                return send(reply, Err(question_open_refusal()));
            }
            let window = handle.window_id().as_u64();
            let client = request.actor.client().to_owned();
            let external_actor = request.actor.is_external();
            let host = cx.global_mut::<Host>();
            match host.engine.submit(window, request, now) {
                Err(e) => send(reply, Err(e)),
                Ok(op) => {
                    host.replies.insert(op, reply);
                    semantics::set_consumer(Consumer::Driver, true);
                    if external_actor {
                        let mut m = semantics::marking(window).unwrap_or_default();
                        if !m.clients.contains(&client) {
                            m.clients.push(client);
                        }
                        m.until = Some(now + MARKING_AFTER);
                        semantics::set_marking(window, Some(m));
                    }
                    let _ = handle.update(cx, |_, w, _| w.refresh());
                }
            }
        }
    }
}

fn same_folder(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b
        || std::fs::canonicalize(a)
            .ok()
            .is_some_and(|x| std::fs::canonicalize(b).ok() == Some(x))
}

/// Called when a window's tree is complete (in its paint): advances its calls right after.
fn on_frame(window: &mut Window, cx: &mut App) {
    let id = semantics::window_id(window);
    let waiting = cx
        .try_global::<Host>()
        .is_some_and(|h| h.engine.pending_for(id));
    if waiting {
        window.on_next_frame(advance);
    }
}

/// Feeds the engine with the window's last tree, delivers what it plans, answers what is done.
fn advance(window: &mut Window, cx: &mut App) {
    let id = semantics::window_id(window);
    let Some(tree) = semantics::tree(id) else {
        window.refresh();
        return;
    };
    let now = Instant::now();
    let out = cx
        .global_mut::<Host>()
        .engine
        .frame(id, &tree, now, &KeymapKeys);
    let mut failed = Vec::new();
    for command in out.commands {
        match command {
            Command::Highlight {
                bounds,
                point,
                until,
                ..
            } => {
                let mut m = semantics::marking(id).unwrap_or_default();
                m.highlight = bounds.map(|b| (b, until));
                m.pointer = point.map(|p| (p, until + std::time::Duration::from_millis(700)));
                semantics::set_marking(id, Some(m));
            }
            Command::Deliver {
                op,
                inputs,
                level,
                origin,
                ..
            } => {
                let delivered = if origin == Actor::SelfTest.origin() {
                    deliver(window, cx, &inputs, level)
                } else {
                    deliver_external(window, cx, &inputs, level)
                };
                if let Err(e) = delivered {
                    failed.push((op, e));
                }
            }
        }
    }
    let host = cx.global_mut::<Host>();
    let mut done = out.done;
    for (op, e) in failed {
        done.extend(host.engine.fail(op, e));
    }
    for d in done {
        if let Some(reply) = host.replies.remove(&d.op) {
            let _ = reply.send(d.reply);
        }
    }
    if host.engine.pending() == 0 {
        semantics::set_consumer(Consumer::Driver, false);
    }
    if host.engine.pending_for(id) {
        // Another frame: the next tree decides (no timer).
        window.refresh();
    }
}

/// Ends the sessions of clients that went away, each with an audit entry (behaviour 37b).
fn retire_idle(cx: &mut App) {
    if !cx.has_global::<Host>() {
        return;
    }
    let gone = cx.global_mut::<Host>().engine.expire_idle(Instant::now());
    if gone.is_empty() {
        return;
    }
    let roots = idecraft_workbench::project_windows::open_project_roots(cx);
    for d in gone {
        let entry = json!({
            "time": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            "actor": {"client": d.client, "session": d.session},
            "tool": "ui_session_ended",
            "outcome": "no call for 60 s",
        });
        for root in &roots {
            if let Err(e) = idecraft_core::log_agent_action(root, &entry) {
                eprintln!("warning: the end of a UI automation session could not be logged: {e}");
            }
        }
    }
}

/// The emergency stop (behaviour 37): pending external calls answer `cancelled`, sessions end,
/// the marking goes, the setting goes off, and it is logged. Returns the sessions ended.
fn stop(source: StopSource, cx: &mut App) -> usize {
    if !cx.has_global::<Host>() {
        return 0;
    }
    retire_idle(cx);
    let host = cx.global_mut::<Host>();
    let (done, ended) = host.engine.stop(source);
    for d in done {
        if let Some(reply) = host.replies.remove(&d.op) {
            let _ = reply.send(d.reply);
        }
    }
    if host.engine.pending() == 0 {
        semantics::set_consumer(Consumer::Driver, false);
    }
    let mut settings = idecraft_workbench::appearance::ide_settings(cx);
    if settings.ui.driver.external {
        settings.ui.driver.external = false;
        if let Err(e) = idecraft_workbench::appearance::set_ide_settings(settings, cx) {
            eprintln!("warning: UI automation could not be switched off in the settings: {e}");
        }
    }
    let windows: Vec<u64> = cx
        .windows()
        .iter()
        .map(|w| w.window_id().as_u64())
        .collect();
    for w in windows {
        semantics::set_marking(w, None);
    }
    cx.refresh_windows();
    let entry = json!({
        "time": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "actor": {"client": "user", "session": ""},
        "tool": "ui_driver_stop",
        "source": source,
        "sessions_ended": ended.iter().map(|d| &d.client).collect::<Vec<_>>(),
    });
    for root in idecraft_workbench::project_windows::open_project_roots(cx) {
        if let Err(e) = idecraft_core::log_agent_action(&root, &entry) {
            eprintln!("warning: the stop of UI automation could not be logged: {e}");
        }
    }
    ended.len()
}

fn modifiers(list: &[Modifier]) -> Modifiers {
    Modifiers {
        control: list.contains(&Modifier::Ctrl),
        alt: list.contains(&Modifier::Alt),
        shift: list.contains(&Modifier::Shift),
        platform: list.contains(&Modifier::Cmd),
        function: list.contains(&Modifier::Fn),
    }
}

/// The keystroke of `key` on this keyboard layout: with a modifier, a key a layout cannot type is
/// pressed where the system puts it (`⌘[` is `⌘ö` on German layouts, SPEC-0071 v4 behaviour 6);
/// Option combinations carry no character, so the layout's character is never typed.
fn keystroke(key: &KeyPress, cx: &App) -> Keystroke {
    let mut name = key.key.clone();
    if !key.modifiers.is_empty()
        && let Some(c) = (name.chars().count() == 1)
            .then(|| name.chars().next())
            .flatten()
        && let Some(e) = cx
            .keyboard_mapper()
            .get_key_equivalents()
            .and_then(|e| e.get(&c).copied())
    {
        name = e.to_string();
    }
    Keystroke {
        modifiers: modifiers(&key.modifiers),
        key: name,
        key_char: None,
    }
}

/// The refusal while the question the toolkit draws is open in the window (SPEC-0281 v4
/// behaviour 9.6, SPEC-0319 v6 behaviour 35).
fn question_open_refusal() -> UiError {
    idecraft_uidriver::security::question_open("a question (alertdialog #native-prompt)", None)
}

/// Delivers the input of an external client like [`deliver`], one input at a time, and stops
/// before the first one that would reach a question the toolkit draws: one that opened after the
/// tree was read, also one an earlier key of the same call opened (SPEC-0319 v6 behaviour 35).
fn deliver_external(
    window: &mut Window,
    cx: &mut App,
    inputs: &[Input],
    level: Level,
) -> Result<(), UiError> {
    if window.has_drawn_prompt() {
        window.refresh();
        return Err(question_open_refusal());
    }
    let keys_only = inputs
        .iter()
        .all(|i| matches!(i, Input::Key(_) | Input::Text { .. }));
    if !matches!(level, Level::Window) || !keys_only {
        return deliver(window, cx, inputs, level);
    }
    for input in inputs {
        if window.has_drawn_prompt() {
            window.refresh();
            return Err(question_open_refusal());
        }
        deliver(window, cx, std::slice::from_ref(input), level)?;
    }
    Ok(())
}

/// Delivers `inputs` into `window` on `level`, marked as synthesised (behaviour 20).
pub fn deliver(
    window: &mut Window,
    cx: &mut App,
    inputs: &[Input],
    level: Level,
) -> Result<(), UiError> {
    match level {
        Level::Window => {
            semantics::synthesizing(|| deliver_window(window, cx, inputs))?;
            window.refresh();
            Ok(())
        }
        Level::Native => deliver_native(window, cx, inputs),
        Level::Semantic => {
            if inputs.is_empty() {
                return Ok(());
            }
            Err(UiError::new(
                ErrorCode::Unsupported,
                "unsupported: GPUI 0.2.2 has no accessibility actions; semantic requests come with the toolkit switch (ADR-0019, phase 3)",
                "use the window level (click, press, type)",
            ))
        }
    }
}

/// Keys and text into the focused element of `window`. GPUI 0.2.2 dispatches pointer events into
/// a window only from inside the toolkit (`Window::dispatch_event` answers a type private to it),
/// so pointer actions at the window level answer `unsupported` before anything is delivered
/// (SPEC-0319 v2 behaviour 20b); the self-test clicks at the native level.
fn deliver_window(window: &mut Window, cx: &mut App, inputs: &[Input]) -> Result<(), UiError> {
    if inputs.iter().any(|i| {
        matches!(
            i,
            Input::PointerMove { .. }
                | Input::PointerDown { .. }
                | Input::PointerUp { .. }
                | Input::Scroll { .. }
                | Input::Semantic { .. }
        )
    }) {
        return Err(UiError::new(
            ErrorCode::Unsupported,
            "unsupported: GPUI 0.2.2 has no public way to dispatch pointer events into a window; pointer actions come with the toolkit switch (ADR-0019, phase 3)",
            "press keys or type text; the window self-test clicks at the native level",
        ));
    }
    for input in inputs {
        match input {
            Input::Key(k) => {
                let stroke = keystroke(k, cx);
                window.dispatch_keystroke(stroke, cx);
            }
            // The path an input method commits text on: the focused element's input handler, so
            // the layout, dead keys and input methods play no part (behaviour 18).
            Input::Text { text } => {
                window.dispatch_keystroke(
                    Keystroke {
                        modifiers: Modifiers::default(),
                        key: String::new(),
                        key_char: Some(text.clone()),
                    },
                    cx,
                );
            }
            _ => {}
        }
    }
    Ok(())
}

/// Native events to IDECraft's own application queue (macOS), or the toolkit's pointer events into
/// the window's own input handler (Linux); the self-test only: a click or a pointer move, as
/// `native_click` delivers them (SPEC-0275 behaviour 12, SPEC-0319 behaviours 20a and 20c).
fn deliver_native(window: &mut Window, cx: &mut App, inputs: &[Input]) -> Result<(), UiError> {
    use crate::native_click::{Click, post, post_move};
    let unsupported = || {
        UiError::new(
            ErrorCode::Unsupported,
            "unsupported: the native level posts clicks and pointer moves only",
            "use the window level",
        )
    };
    let last_down = inputs.iter().rev().find_map(|i| match i {
        Input::PointerDown {
            x,
            y,
            button,
            count,
            modifiers,
        } => Some((*x, *y, *button, *count, modifiers.clone())),
        _ => None,
    });
    let posted = match last_down {
        Some((x, y, b, count, m)) => {
            let kind = match b {
                Button::Right => Click::Right,
                Button::Left if m.contains(&Modifier::Ctrl) => Click::ControlLeft,
                Button::Left => Click::Left(usize::from(count)),
                Button::Middle => return Err(unsupported()),
            };
            post(window, cx, x, y, kind)
        }
        None => match inputs.last() {
            Some(Input::PointerMove { x, y, .. }) => post_move(window, cx, *x, *y),
            _ => return Err(unsupported()),
        },
    };
    if posted {
        Ok(())
    } else {
        Err(UiError::new(
            ErrorCode::Unavailable,
            "unavailable: no window to post native events to",
            "",
        ))
    }
}
