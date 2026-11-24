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

use ferris_idea_mcp::window::{UiCall, UiCallKind};
use ferris_idea_uidriver::engine::{Command, Engine, Keys, Op, OpId};
use ferris_idea_uidriver::trace::AuditFacts;
use ferris_idea_uidriver::{
    Actor, Button, ErrorCode, Input, KeyPress, Level, Modifier, Reply, StopSource, UiError,
};
use ferris_idea_workbench::semantics::{self, Consumer};
use gpui::{App, Global, Keystroke, Modifiers, Window};
use serde_json::json;
use tokio::sync::oneshot;

/// How long the marking stays after a client's last call.
const MARKING_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

struct Host {
    engine: Engine,
    replies: HashMap<OpId, oneshot::Sender<Reply>>,
    /// When each window last advanced its calls on a frame of its own.
    advanced: HashMap<u64, Instant>,
}

impl Global for Host {}

/// The keymap in use, as the engine asks it (behaviour 35: effects come from action ids).
pub struct KeymapKeys;

impl Keys for KeymapKeys {
    fn actions_for(&self, key: &KeyPress) -> Vec<String> {
        let has = |m| key.modifiers.contains(&m);
        let Some(stroke) = ferris_idea_ui::keymap::Keystroke::from_platform(
            &key.key,
            has(Modifier::Ctrl),
            has(Modifier::Alt),
            has(Modifier::Shift),
            has(Modifier::Cmd),
        ) else {
            return Vec::new();
        };
        ferris_idea_workbench::keymaps::current()
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
        advanced: HashMap::new(),
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
    let external = ferris_idea_workbench::appearance::ide_settings(cx)
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
        UiCallKind::Screenshot {
            ref locator,
            region,
            terminal_read,
        } => {
            let result = screenshot(&call, locator.clone(), region, terminal_read, now, cx);
            let facts = AuditFacts {
                element: None,
                effects: vec![ferris_idea_uidriver::Effect::None],
            };
            let _ = reply.send(Reply { result, facts });
        }
        UiCallKind::Command {
            ref command,
            ref actor,
            ref grants,
        } => {
            let (result, effects) = run_command(&call, command, actor, grants, now, cx);
            let facts = AuditFacts {
                element: None,
                effects,
            };
            let _ = reply.send(Reply { result, facts });
        }
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
            let Some(base) = dir.or_else(ferris_idea_core::user_config_dir) else {
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
                ferris_idea_workbench::project_windows::project_window(&call.workspace, cx)
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
            let settings_open = ferris_idea_workbench::project_windows::workbenches(cx)
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

/// The project window of the call's route and its workbench (behaviour 34, UISEC-7).
fn route_window(
    call: &UiCall,
    cx: &App,
) -> Result<
    (
        gpui::AnyWindowHandle,
        gpui::Entity<ferris_idea_workbench::WorkbenchView>,
    ),
    UiError,
> {
    let not_found = || {
        UiError::new(
            ErrorCode::NotFound,
            "not_found: no window of this project",
            "a client reaches only the windows of its own project's route",
        )
    };
    let handle = ferris_idea_workbench::project_windows::project_window(&call.workspace, cx)
        .filter(|h| call.window.is_none_or(|w| w == h.window_id().as_u64()))
        .ok_or_else(not_found)?;
    let bench = ferris_idea_workbench::project_windows::workbenches(cx)
        .into_iter()
        .find(|b| same_folder(b.read(cx).project_root(), &call.workspace))
        .ok_or_else(not_found)?;
    Ok((handle, bench))
}

/// Shows the marking for an external client and notes it as driving (behaviours 36, 37).
fn begin_external(actor: &Actor, window: u64, now: Instant, cx: &mut App) -> Result<(), UiError> {
    cx.global_mut::<Host>().engine.command(actor, window, now)?;
    if actor.is_external() {
        let mut m = semantics::marking(window).unwrap_or_default();
        let client = actor.client().to_owned();
        if !m.clients.contains(&client) {
            m.clients.push(client);
        }
        m.until = Some(now + MARKING_AFTER);
        semantics::set_marking(window, Some(m));
    }
    Ok(())
}

/// A screenshot of the window's own frame (`SPEC-0319` v7 behaviour 53): drawn anew when
/// something changed — also while the window is covered and draws no frames of its own — then
/// read back from an offscreen texture, never from the screen; secret fields, password inputs,
/// editors of files outside the workspace and, without `terminal-read`, the terminal are filled
/// before anything is encoded; then cut to the element or region and encoded as PNG.
fn screenshot(
    call: &UiCall,
    locator: Option<ferris_idea_uidriver::Locator>,
    region: Option<ferris_idea_uidriver::Rect>,
    terminal_read: bool,
    now: Instant,
    cx: &mut App,
) -> Result<serde_json::Value, UiError> {
    use base64::Engine as _;
    let (handle, _) = route_window(call, cx)?;
    let window = handle.window_id().as_u64();
    let _ = now;
    // The tree of a frame drawn now names what to fill.
    semantics::set_consumer(Consumer::Driver, true);
    let captured = handle
        .update(cx, |_, w, cx| {
            w.refresh();
            w.capture_frame(cx)
        })
        .ok()
        .flatten();
    if cx.global::<Host>().engine.pending() == 0 {
        semantics::set_consumer(Consumer::Driver, false);
    }
    let Some((image, scale)) = captured else {
        return Err(UiError::new(
            ErrorCode::Unsupported,
            "unsupported: this platform's renderer gives no read-back of the window's frame (Linux, Windows, the headless test platform)",
            "read the tree with ui_snapshot; the screen-recording permission is never requested",
        ));
    };
    let (width, height) = image.dimensions();
    let mut pixels = image.into_raw();
    let tree = semantics::tree(window);
    let masks = tree
        .as_deref()
        .map(|t| ferris_idea_uidriver::shot::masked_bounds(t, terminal_read))
        .unwrap_or_default();
    let masked = ferris_idea_uidriver::shot::fill(&mut pixels, width, height, scale, &masks);
    let cut = match (&locator, region) {
        (Some(l), _) => {
            let Some(tree) = tree.as_deref() else {
                return Err(UiError::new(
                    ErrorCode::NotFound,
                    "not_found: the window has no tree yet",
                    "try again after the window drew a frame",
                ));
            };
            let matches = l.resolve(tree, &ferris_idea_uidriver::tree::View::in_process())?;
            match matches.as_slice() {
                [one] => tree
                    .get(*one)
                    .and_then(|n| n.visible_bounds)
                    .ok_or_else(|| {
                        UiError::new(
                            ErrorCode::NotVisible,
                            format!("not_visible: {} has no visible part", l.to_text()),
                            "",
                        )
                    })
                    .map(Some)?,
                [] => {
                    return Err(UiError::new(
                        ErrorCode::NotFound,
                        format!("not_found: {} matches nothing", l.to_text()),
                        "",
                    ));
                }
                _ => {
                    return Err(UiError::new(
                        ErrorCode::Ambiguous,
                        format!(
                            "ambiguous: {} matches {} elements",
                            l.to_text(),
                            matches.len()
                        ),
                        "make the locator match one element",
                    ));
                }
            }
        }
        (None, r) => r,
    };
    let (pixels, width, height) = match cut {
        None => (pixels, width, height),
        Some(r) => ferris_idea_uidriver::shot::crop(&pixels, width, height, scale, &r)
            .ok_or_else(|| UiError::invalid("invalid: the region lies outside the window"))?,
    };
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| {
        UiError::new(
            ErrorCode::Internal,
            format!("internal: the screenshot could not be encoded: {e}"),
            "",
        )
    })?;
    Ok(json!({
        "width": width,
        "height": height,
        "scale": scale,
        "masked": masked,
        "format": "png",
        "png_base64": base64::engine::general_purpose::STANDARD.encode(&png),
    }))
}

/// A step that runs in the project's workbench.
type InWorkbench<'a> = dyn FnMut(
        &mut ferris_idea_workbench::WorkbenchView,
        &mut gpui::Window,
        &mut gpui::Context<ferris_idea_workbench::WorkbenchView>,
    ) -> Result<serde_json::Value, UiError>
    + 'a;

/// Runs a window command (`SPEC-0319` v7 behaviours 46-52) in the project window of the call's
/// route, after the checks only the window can make: the Settings dialog and a drawn question
/// keep the user's pages and answers, the focused field decides what typing means, the open
/// popup what choosing means. Returns the result and the effects that were checked.
fn run_command(
    call: &UiCall,
    command: &ferris_idea_uidriver::UiCommand,
    actor: &Actor,
    grants: &ferris_idea_uidriver::Grants,
    now: Instant,
    cx: &mut App,
) -> (
    Result<serde_json::Value, UiError>,
    Vec<ferris_idea_uidriver::Effect>,
) {
    use ferris_idea_uidriver::Effect;
    use ferris_idea_uidriver::command::{self as c, UiCommand};
    let mut effects = Vec::new();
    let result = (|| -> Result<serde_json::Value, UiError> {
        // The window repeats the checks that need no window (rule 12: twice is cheap).
        effects = c::authorize_command(actor, command, grants)?;
        let (handle, bench) = route_window(call, cx)?;
        let window = handle.window_id().as_u64();
        let external = actor.is_external();
        if external && bench.read(cx).settings_open() && !command.allowed_in_settings() {
            return Err(UiError::new(
                ErrorCode::Reserved,
                "reserved: the Settings dialog is open; only `settings` (pages that are not reserved), `close_settings`, `popup` and `list` work while it is",
                "close it with `close_settings`; nothing was delivered",
            ));
        }
        let question_open = handle
            .update(cx, |_, w, _| w.has_drawn_prompt())
            .unwrap_or(false);
        if external && question_open && !command.reads_only() {
            return Err(question_open_refusal());
        }
        begin_external(actor, window, now, cx)?;
        let checked = |list: Vec<Effect>, effects: &mut Vec<Effect>| -> Result<(), UiError> {
            if external {
                c::check_effects(&list, grants)?;
            }
            *effects = list;
            Ok(())
        };
        let in_window = |cx: &mut App, f: &mut InWorkbench| {
            let bench = bench.clone();
            handle
                .update(cx, |_, w, cx| bench.update(cx, |b, cx| f(b, w, cx)))
                .map_err(|_| {
                    UiError::new(
                        ErrorCode::Unavailable,
                        "unavailable: the window is gone",
                        "",
                    )
                })?
        };
        let settings_pages = || {
            ferris_idea_workbench::settings_model::Page::ALL
                .iter()
                .map(|p| (c::page_id(p.title()), *p))
                .collect::<Vec<_>>()
        };
        match command {
            UiCommand::OpenFile { path, line, column } => {
                let file = c::project_file(&call.workspace, path)?;
                let (line, column) = (*line, *column);
                in_window(cx, &mut |b, w, cx| {
                    b.open_at(&file, line, column, w, cx);
                    Ok(json!({
                        "opened": file,
                        "active": b.active_file(),
                        "caret": b.caret_position(cx),
                    }))
                })
            }
            UiCommand::ToolWindow { id, op } => {
                let Some(tw) = ferris_idea_ui::tool_windows::ToolWindow::from_id(id) else {
                    return Err(UiError::new(
                        ErrorCode::NotFound,
                        format!("not_found: no tool window `{id}`"),
                        "`list` names the tool windows",
                    ));
                };
                let op = *op;
                in_window(cx, &mut |b, w, cx| {
                    let (shown, focused) = b.driver_tool_window(tw, op, w, cx);
                    Ok(
                        json!({"id": tw.id(), "title": tw.title(), "shown": shown, "focused": focused}),
                    )
                })
            }
            UiCommand::Settings { page } => {
                let Some((_, p)) = settings_pages().into_iter().find(|(id, _)| id == page) else {
                    return Err(UiError::new(
                        ErrorCode::NotFound,
                        format!("not_found: no Settings page `{page}`"),
                        "`list` names the Settings pages",
                    ));
                };
                in_window(cx, &mut |b, w, cx| {
                    b.driver_settings(Some(p), w, cx);
                    Ok(json!({"settings": c::page_id(p.title()), "title": p.title()}))
                })
            }
            UiCommand::CloseSettings => in_window(cx, &mut |b, w, cx| {
                let was = b.settings_open();
                b.driver_settings(None, w, cx);
                Ok(json!({"closed": was}))
            }),
            UiCommand::Invoke { action } => {
                let id = action.clone();
                in_window(cx, &mut |b, w, cx| {
                    b.menu_action(&id, w, cx);
                    Ok(json!({"invoked": id, "popup": b.driver_popup(cx).map(|p| p.kind)}))
                })
            }
            UiCommand::TypeText { text } => {
                let target = handle
                    .update(cx, |_, w, cx| bench.read(cx).driver_typing_target(w, cx))
                    .ok()
                    .flatten();
                let Some((kind, file)) = target else {
                    return Err(UiError::new(
                        ErrorCode::Reserved,
                        "reserved: type_text goes only into the focused editor or the field of an open popup; the focus is elsewhere (a dialog, a tool window, the terminal or a secret field)",
                        "open a file or a popup first; nothing was typed",
                    ));
                };
                if kind == "editor" {
                    // The editor's file must be one an external client may open.
                    let allowed = file.as_deref().is_some_and(|f| {
                        f.to_str()
                            .is_some_and(|f| c::project_file(&call.workspace, f).is_ok())
                    });
                    if !allowed {
                        return Err(UiError::new(
                            ErrorCode::Reserved,
                            "reserved: the focused editor shows a file external clients may not change (outside the project, the project's agent or Git state, or credentials)",
                            "nothing was typed",
                        ));
                    }
                    checked(vec![Effect::Write], &mut effects)?;
                } else {
                    checked(vec![Effect::None], &mut effects)?;
                }
                let inputs = [Input::Text { text: text.clone() }];
                handle
                    .update(cx, |_, w, cx| {
                        deliver_external(w, cx, &inputs, Level::Window)
                    })
                    .map_err(|_| {
                        UiError::new(
                            ErrorCode::Unavailable,
                            "unavailable: the window is gone",
                            "",
                        )
                    })??;
                Ok(json!({"typed_length": text.chars().count(), "into": kind}))
            }
            UiCommand::Popup => {
                let popup = bench.read(cx).driver_popup(cx);
                Ok(match popup {
                    Some(p) => {
                        json!({"open": true, "kind": p.kind, "items": p.items, "selected": p.selected, "count": p.items.len()})
                    }
                    None => json!({"open": false}),
                })
            }
            UiCommand::Choose { index, label } => {
                let Some(popup) = bench.read(cx).driver_popup(cx) else {
                    return Err(UiError::new(
                        ErrorCode::NotFound,
                        "not_found: no popup is open",
                        "open one first (`invoke ShowIntentionActions`, `invoke GotoFile`)",
                    ));
                };
                let index = match (index, label) {
                    (Some(i), _) => *i,
                    (None, Some(l)) => {
                        let found: Vec<usize> = popup
                            .items
                            .iter()
                            .enumerate()
                            .filter(|(_, it)| &it.label == l && it.kind != "header")
                            .map(|(i, _)| i)
                            .collect();
                        match found.as_slice() {
                            [one] => *one,
                            [] => {
                                return Err(UiError::new(
                                    ErrorCode::NotFound,
                                    format!("not_found: no item {l:?} in the popup"),
                                    "`popup` lists its items",
                                ));
                            }
                            _ => {
                                return Err(UiError::new(
                                    ErrorCode::Ambiguous,
                                    format!("ambiguous: {} items are labelled {l:?}", found.len()),
                                    "choose by `index`",
                                ));
                            }
                        }
                    }
                    (None, None) => {
                        return Err(UiError::invalid("invalid: choose takes `index` or `label`"));
                    }
                };
                let Some(item) = popup.items.get(index) else {
                    return Err(UiError::new(
                        ErrorCode::NotFound,
                        format!("not_found: the popup has {} items", popup.items.len()),
                        "`popup` lists its items",
                    ));
                };
                // What choosing does decides its effects (behaviour 51).
                let item_effects = match (popup.kind, item.kind) {
                    (_, "header") => {
                        return Err(UiError::invalid("invalid: a heading cannot be chosen"));
                    }
                    ("intentions" | "completion", _) => vec![Effect::Write],
                    (_, "action") => c::invokable(item.action.as_deref().unwrap_or_default())?,
                    (_, "settings") => {
                        let page = c::page_id(item.action.as_deref().unwrap_or_default());
                        if c::RESERVED_SETTINGS_PAGES.contains(&page.as_str()) {
                            return Err(UiError::new(
                                ErrorCode::Reserved,
                                format!(
                                    "reserved: the Settings page `{page}` is reserved for the user"
                                ),
                                "nothing was chosen",
                            ));
                        }
                        vec![Effect::None]
                    }
                    _ => vec![Effect::None],
                };
                checked(item_effects, &mut effects)?;
                let label = item.label.clone();
                let kind = popup.kind;
                in_window(cx, &mut |b, w, cx| {
                    b.driver_choose(index, w, cx)
                        .map_err(|why| UiError::invalid(format!("invalid: {why}")))?;
                    Ok(json!({"chosen": index, "label": label, "popup": kind}))
                })
            }
            UiCommand::Dismiss => in_window(cx, &mut |b, w, cx| {
                let closed = b.driver_dismiss(w, cx);
                Ok(json!({"closed": closed}))
            }),
            UiCommand::List => {
                let b = bench.read(cx);
                let tool_windows: Vec<serde_json::Value> = ferris_idea_ui::tool_windows::ToolWindow::ALL
                    .iter()
                    .map(|t| json!({"id": t.id(), "title": t.title(), "shown": b.tool_window_shown(*t), "effects": c::tool_window_effects(t.id()).iter().map(|e| e.name()).collect::<Vec<_>>()}))
                    .collect();
                let pages: Vec<serde_json::Value> = settings_pages()
                    .into_iter()
                    .map(|(id, p)| json!({"id": id, "title": p.title(), "reserved": c::RESERVED_SETTINGS_PAGES.contains(&id.as_str())}))
                    .collect();
                Ok(json!({
                    "tool_windows": tool_windows,
                    "settings_pages": pages,
                    "settings_open": b.settings_open(),
                    "active_file": b.active_file(),
                    "popup": b.driver_popup(cx).map(|p| p.kind),
                }))
            }
        }
    })();
    (result, effects)
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

/// How long a window with waiting calls may go without a frame before the host draws one itself.
const STALLED: std::time::Duration = std::time::Duration::from_millis(250);

/// Draws a frame for each window whose calls wait but which draws none of its own — minimized,
/// fully covered or in the background, where macOS stops its display link — and advances its
/// calls on it (`SPEC-0319` v7 behaviour 55a). Called on each turn of the window's MCP request
/// loop, so waiting stays tied to requests and frames, never to a timer of the driver.
pub fn pump_stalled(cx: &mut App) {
    let Some(host) = cx.try_global::<Host>() else {
        return;
    };
    let now = Instant::now();
    let stalled: Vec<u64> = host
        .engine
        .waiting_windows()
        .into_iter()
        .filter(|w| {
            host.advanced
                .get(w)
                .is_none_or(|t| now.saturating_duration_since(*t) >= STALLED)
        })
        .collect();
    for id in stalled {
        let Some(handle) = cx
            .windows()
            .into_iter()
            .find(|w| w.window_id().as_u64() == id)
        else {
            continue;
        };
        let _ = handle.update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
            advance(window, cx);
        });
    }
}

/// Feeds the engine with the window's last tree, delivers what it plans, answers what is done.
fn advance(window: &mut Window, cx: &mut App) {
    let id = semantics::window_id(window);
    if cx.has_global::<Host>() {
        cx.global_mut::<Host>().advanced.insert(id, Instant::now());
    }
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
    let roots = ferris_idea_workbench::project_windows::open_project_roots(cx);
    for d in gone {
        let entry = json!({
            "time": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            "actor": {"client": d.client, "session": d.session},
            "tool": "ui_session_ended",
            "outcome": "no call for 60 s",
        });
        for root in &roots {
            if let Err(e) = ferris_idea_core::log_agent_action(root, &entry) {
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
    let mut settings = ferris_idea_workbench::appearance::ide_settings(cx);
    if settings.ui.driver.external {
        settings.ui.driver.external = false;
        if let Err(e) = ferris_idea_workbench::appearance::set_ide_settings(settings, cx) {
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
    for root in ferris_idea_workbench::project_windows::open_project_roots(cx) {
        if let Err(e) = ferris_idea_core::log_agent_action(&root, &entry) {
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
    ferris_idea_uidriver::security::question_open("a question (alertdialog #native-prompt)", None)
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

/// Keys, text and pointer actions into `window`: keys and text to the focused element, clicks and
/// the pointer coming to rest through the window's own input handler in process (SPEC-0319 v7
/// behaviour 16b, the hook of the vendored gpui in `native_click.rs`); scrolling and semantic
/// requests answer `unsupported` before anything is delivered.
fn deliver_window(window: &mut Window, cx: &mut App, inputs: &[Input]) -> Result<(), UiError> {
    if inputs
        .iter()
        .any(|i| matches!(i, Input::Scroll { .. } | Input::Semantic { .. }))
    {
        return Err(UiError::new(
            ErrorCode::Unsupported,
            "unsupported: scrolling and semantic requests at the window level come with the toolkit switch (ADR-0019, phase 3)",
            "click, hover, press keys or type text",
        ));
    }
    // Pointer actions go to the window's own input handler in process (SPEC-0319 v7 behaviour
    // 16b): a click is the last press with its count and modifiers, a hover the last move.
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
    let pointer = inputs.iter().any(|i| {
        matches!(
            i,
            Input::PointerMove { .. } | Input::PointerDown { .. } | Input::PointerUp { .. }
        )
    });
    if pointer {
        use crate::native_click::{Click, deliver_in_process};
        match last_down {
            Some((x, y, button, count, m)) => {
                let click = match button {
                    Button::Right => Click::Right,
                    Button::Left if m.contains(&Modifier::Ctrl) => Click::ControlLeft,
                    Button::Left => Click::Left(usize::from(count)),
                    Button::Middle => {
                        return Err(UiError::new(
                            ErrorCode::Unsupported,
                            "unsupported: the middle button",
                            "use the left or right button",
                        ));
                    }
                };
                deliver_in_process(window, cx, x, y, Some(click));
            }
            None => {
                if let Some(Input::PointerMove { x, y, .. }) = inputs
                    .iter()
                    .rev()
                    .find(|i| matches!(i, Input::PointerMove { .. }))
                {
                    deliver_in_process(window, cx, *x, *y, None);
                }
            }
        }
        // Keys and text after a click (the focus click of `press` and `type`) go after it: the
        // click is handed over once the current frame is done, so keys delivered now would reach
        // the element that had the focus before it (a form field took the text of the next one,
        // found by the F-083 window tests).
        let keys: Vec<Input> = inputs
            .iter()
            .filter(|i| matches!(i, Input::Key(_) | Input::Text { .. }))
            .cloned()
            .collect();
        if !keys.is_empty() {
            window.defer(cx, move |window, cx| {
                // A question the click opened takes none of them (SPEC-0319 v6 behaviour 35).
                for key in &keys {
                    if window.has_drawn_prompt() {
                        break;
                    }
                    semantics::synthesizing(|| deliver_keys(window, cx, std::slice::from_ref(key)));
                }
                window.refresh();
            });
        }
        return Ok(());
    }
    deliver_keys(window, cx, inputs);
    Ok(())
}

/// Delivers the keys and the text of `inputs` to the focused element.
fn deliver_keys(window: &mut Window, cx: &mut App, inputs: &[Input]) {
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
}

/// Native events to Ferris IDEA's own application queue (macOS), or the toolkit's pointer events into
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
