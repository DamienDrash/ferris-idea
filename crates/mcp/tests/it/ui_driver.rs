//! The UI driver's MCP tools (`SPEC-0319` behaviours 29 and 32-39) against a window stand-in that
//! runs the real engine on a fixture tree: UISEC-1, UISEC-2, UISEC-3, UISEC-7 (the route's
//! workspace), UISEC-10 (`ui_driver_stop`), UISEC-13, and the MCP part of AK-8.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use idecraft_core::Workspace;
use idecraft_mcp::IdeServer;
use idecraft_mcp::window::{UiCall, UiCallKind, Window};
use idecraft_uidriver::engine::{Engine, Keys};
use idecraft_uidriver::trace::AuditFacts;
use idecraft_uidriver::{
    AxAction, Effect, KeyPress, Rect, Reply, Role, Semantics, Tree, TreeBuilder,
};
use rmcp::ServiceExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, WriteHalf};

const WINDOW: u64 = 42;
const SECRET: &str = "hunter2-KNOWN-SECRET";

struct Client {
    write: WriteHalf<DuplexStream>,
    lines: Lines<BufReader<tokio::io::ReadHalf<DuplexStream>>>,
    next_id: u64,
}

impl Client {
    async fn send(&mut self, message: Value) {
        let mut line = serde_json::to_string(&message).unwrap();
        line.push('\n');
        self.write.write_all(line.as_bytes()).await.unwrap();
        self.write.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let line = self
                .lines
                .next_line()
                .await
                .unwrap()
                .expect("server closed");
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await
    }
}

/// The error text of a refused call, or panics.
fn error(response: &Value) -> String {
    if let Some(e) = response.get("error") {
        return e["message"].as_str().unwrap_or_default().to_owned();
    }
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn ok(response: &Value) -> Value {
    assert!(
        response.get("error").is_none() && response["result"]["isError"] != json!(true),
        "{response}"
    );
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap()
}

/// The fixture window: a Commit button (vcs-write), a message field (none), a password field,
/// a disabled "Push" button.
fn fixture(frame: u64) -> Tree {
    let mut b = TreeBuilder::new();
    let mut push = |s: Semantics, r: Rect| b.push(s, r, Some(r));
    push(
        Semantics::new(Role::Button)
            .name("Commit")
            .test_id("commit-button")
            .action(AxAction::Click)
            .effects(&[Effect::VcsWrite]),
        Rect::new(10., 700., 80., 24.),
    );
    push(
        Semantics::new(Role::Button)
            .name("Push")
            .test_id("push-button")
            .enabled(false)
            .action(AxAction::Click)
            .effects(&[Effect::VcsWrite, Effect::Remote]),
        Rect::new(100., 700., 80., 24.),
    );
    push(
        Semantics::new(Role::MultilineTextInput)
            .name("Commit message")
            .test_id("commit-message")
            .effects(&[Effect::None]),
        Rect::new(10., 500., 600., 150.),
    );
    push(
        Semantics::new(Role::PasswordInput)
            .name("Password")
            .test_id("password")
            .value(SECRET)
            .effects(&[Effect::None]),
        Rect::new(700., 100., 200., 24.),
    );
    b.finish(frame)
}

struct FakeKeys;

impl Keys for FakeKeys {
    fn actions_for(&self, key: &KeyPress) -> Vec<String> {
        match key.to_text().as_str() {
            "cmd-0" => vec!["ActivateCommitToolWindow".into()],
            "cmd-v" => vec!["$Paste".into()],
            _ => Vec::new(),
        }
    }
}

/// A window that serves UI calls with the real engine, frames every millisecond.
#[derive(Default)]
struct DriverWindow {
    engine: Arc<Mutex<Engine>>,
    calls: Mutex<Vec<(PathBuf, String)>>,
    delivered: Arc<Mutex<usize>>,
}

impl Window for DriverWindow {
    fn ask(
        &self,
        _request: idecraft_mcp::window::PermissionRequest,
        reply: tokio::sync::oneshot::Sender<idecraft_mcp::window::PermissionAnswer>,
    ) {
        // Nobody answers: the call is refused as not granted.
        drop(reply);
    }
    fn pending_work(&self, reply: tokio::sync::oneshot::Sender<idecraft_mcp::window::PendingWork>) {
        drop(reply);
    }
    fn quit(
        &self,
        _mode: idecraft_mcp::window::QuitMode,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    ) {
        drop(reply);
    }

    fn ui(&self, call: UiCall, reply: tokio::sync::oneshot::Sender<Reply>) {
        let kind = match &call.kind {
            UiCallKind::Driver(r) => format!("driver {}", r.op.tool()),
            other => format!("{other:?}"),
        };
        self.calls
            .lock()
            .unwrap()
            .push((call.workspace.clone(), kind));
        let engine = self.engine.clone();
        let delivered = self.delivered.clone();
        std::thread::spawn(move || {
            let now = Instant::now();
            let answer = |result| Reply {
                result,
                facts: AuditFacts::default(),
            };
            let request = match call.kind {
                UiCallKind::Driver(r) => r,
                UiCallKind::Status => {
                    let _ = reply.send(answer(Ok(engine.lock().unwrap().status(now))));
                    return;
                }
                UiCallKind::Stop(source) => {
                    let (_, ended) = engine.lock().unwrap().stop(source);
                    let _ = reply.send(answer(Ok(
                        json!({"stopped": true, "sessions_ended": ended.len()}),
                    )));
                    return;
                }
                UiCallKind::Screenshot => {
                    let _ = reply.send(answer(Err(idecraft_uidriver::UiError::new(
                        idecraft_uidriver::ErrorCode::Unsupported,
                        "unsupported: no frame read-back",
                        "",
                    ))));
                    return;
                }
                _ => {
                    let _ = reply.send(answer(Ok(json!({}))));
                    return;
                }
            };
            let id = match engine.lock().unwrap().submit(WINDOW, request, now) {
                Ok(id) => id,
                Err(e) => {
                    let _ = reply.send(answer(Err(e)));
                    return;
                }
            };
            for frame in 1..20_000u64 {
                let out = engine.lock().unwrap().frame(
                    WINDOW,
                    &fixture(frame),
                    Instant::now(),
                    &FakeKeys,
                );
                *delivered.lock().unwrap() += out
                    .commands
                    .iter()
                    .filter(|c| matches!(c, idecraft_uidriver::Command::Deliver { .. }))
                    .count();
                if let Some(d) = out.done.into_iter().find(|d| d.op == id) {
                    let _ = reply.send(d.reply);
                    return;
                }
                // The stand-in's frame clock.
                std::thread::sleep(Duration::from_millis(1));
            }
        });
    }
}

fn temp_workspace() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("idecraft-mcp-ui-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn settings_file(root: &Path) -> PathBuf {
    root.with_extension("settings.toml")
}

fn set_external(root: &Path, on: bool) {
    let mut s = idecraft_core::load_ide_settings(&settings_file(root));
    s.ui.driver.external = on;
    idecraft_core::save_ide_settings(&settings_file(root), &s).unwrap();
}

async fn connect(
    grants: &[idecraft_core::Grant],
    window: Option<Arc<DriverWindow>>,
) -> (Client, PathBuf) {
    let root = temp_workspace();
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let permissions = idecraft_core::AgentPermissions {
        allow: grants.iter().copied().collect(),
        file: Some(root.with_extension("agent-permissions.toml")),
        problem: None,
    };
    let mut server = IdeServer::with_permissions(Workspace::open(&root).unwrap(), permissions)
        .with_ide_settings_file(settings_file(&root))
        .with_plugins_dir(root.join(".test-plugins"));
    if let Some(w) = window {
        let w: Arc<dyn Window> = w;
        server = server.with_window(w);
    }
    tokio::spawn(async move {
        let running = server.serve(tokio::io::split(server_io)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, write) = tokio::io::split(client_io);
    let mut client = Client {
        write,
        lines: BufReader::new(read).lines(),
        next_id: 0,
    };
    client
        .request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "ui-test", "version": "1"}}),
        )
        .await;
    client
        .send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;
    (client, root)
}

fn all_tools() -> Vec<(&'static str, Value)> {
    vec![
        ("ui_snapshot", json!({})),
        ("ui_find", json!({"locator": "id=commit-button"})),
        (
            "ui_act",
            json!({"locator": "id=commit-button", "action": "click"}),
        ),
        (
            "ui_expect",
            json!({"locator": "id=commit-button", "matcher": "visible"}),
        ),
        ("ui_screenshot", json!({})),
        ("ui_trace", json!({"command": "start"})),
    ]
}

/// The lines the UI driver writes itself, without the line every changing call gets
/// (`SPEC-0053` v7 behaviour 11, `"call":true`).
fn log_of(root: &Path) -> String {
    std::fs::read_to_string(root.join(".idecraft/agent-log.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.contains("\"call\":true"))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// UISEC-1: with fresh settings every tool but status and stop answers `unavailable` with the
/// hint, and the window is never asked; the stdio server answers `unavailable` too.
#[tokio::test]
async fn uisec1_off_by_default() {
    let window = Arc::new(DriverWindow::default());
    let all = [idecraft_core::Grant::Ui, idecraft_core::Grant::VcsWrite];
    let (mut client, root) = connect(&all, Some(window.clone())).await;
    for (tool, args) in all_tools() {
        let e = error(&client.call(tool, args).await);
        assert!(e.starts_with("unavailable:"), "{tool}: {e}");
        assert!(
            e.contains("Allow agents and scripts to operate the window"),
            "{tool}: {e}"
        );
    }
    assert!(
        window.calls.lock().unwrap().is_empty(),
        "the window was asked"
    );
    let status = ok(&client.call("ui_driver_status", json!({})).await);
    assert_eq!(status["external"], false);
    let stop = ok(&client.call("ui_driver_stop", json!({})).await);
    assert_eq!(stop["stopped"], true);
    assert_eq!(*window.delivered.lock().unwrap(), 0);
    assert!(log_of(&root).is_empty() || !log_of(&root).contains("ui_act"));
    // Without a window (stdio): `unavailable` naming the window's HTTP server.
    let (mut stdio, root) = connect(&all, None).await;
    set_external(&root, true);
    let e = error(&stdio.call("ui_snapshot", json!({})).await);
    assert!(
        e.starts_with("unavailable:") && e.contains("over HTTP"),
        "{e}"
    );
}

/// UISEC-2: `set_ide_settings` cannot switch it on; the setting stays off.
#[tokio::test]
async fn uisec2_the_setting_is_not_writable_by_agents() {
    let (mut client, root) = connect(
        &[idecraft_core::Grant::Ui, idecraft_core::Grant::Settings],
        None,
    )
    .await;
    for key in ["ui_driver_external", "ui.driver.external"] {
        let e = error(
            &client
                .call("set_ide_settings", json!({key: true, "dry_run": false}))
                .await,
        );
        assert!(
            e.starts_with("invalid:") && e.contains("ui.driver.external"),
            "{e}"
        );
    }
    assert!(
        !idecraft_core::load_ide_settings(&settings_file(&root))
            .ui
            .driver
            .external
    );
    // Other settings still go.
    let r = client
        .call(
            "set_ide_settings",
            json!({"font_size": 15.0, "dry_run": false}),
        )
        .await;
    assert!(r.get("error").is_none(), "{r}");
}

/// UISEC-3 over MCP: no `ui` → `not_granted` naming `ui`; with `ui` but not `vcs-write` the
/// Commit button → `not_granted` naming `vcs-write`; nothing delivered; both logged.
#[tokio::test]
async fn uisec3_missing_rights_over_mcp() {
    let window = Arc::new(DriverWindow::default());
    let (mut client, root) = connect(&[], Some(window.clone())).await;
    set_external(&root, true);
    let e = error(
        &client
            .call(
                "ui_act",
                json!({"locator": "id=commit-button", "action": "click"}),
            )
            .await,
    );
    assert!(e.starts_with("not_granted:") && e.contains("`ui`"), "{e}");
    assert!(window.calls.lock().unwrap().is_empty());
    let (mut client, root) = connect(&[idecraft_core::Grant::Ui], Some(window.clone())).await;
    set_external(&root, true);
    let e = error(
        &client
            .call(
                "ui_act",
                json!({"locator": "id=commit-button", "action": "click"}),
            )
            .await,
    );
    assert!(
        e.starts_with("not_granted:") && e.contains("`vcs-write`"),
        "{e}"
    );
    assert_eq!(*window.delivered.lock().unwrap(), 0);
    let log = log_of(&root);
    assert!(
        log.contains("\"outcome\":\"not_granted\"") && log.contains("commit-button"),
        "{log}"
    );
}

/// AK-8 (MCP part) and UISEC-7: a shortcut, a find, an assertion; every call carries the route's
/// own workspace; a disabled button is found and asserted disabled.
#[tokio::test]
async fn ak8_press_find_expect_over_mcp() {
    let window = Arc::new(DriverWindow::default());
    let (mut client, root) = connect(&[idecraft_core::Grant::Ui], Some(window.clone())).await;
    set_external(&root, true);
    let r = ok(&client
        .call("ui_act", json!({"action": "press", "keys": "cmd-0"}))
        .await);
    assert_eq!(r["inputs"], 1);
    let found = ok(&client
        .call("ui_find", json!({"locator": "role=button[name=\"Push\"]"}))
        .await);
    assert_eq!(found["count"], 1);
    assert_eq!(found["matches"][0]["test_id"], "push-button");
    assert_eq!(
        found["matches"][0]["effects"],
        json!(["vcs-write", "remote"])
    );
    let held = ok(&client
        .call(
            "ui_expect",
            json!({"locator": "id=push-button", "matcher": "disabled", "timeout_ms": 500}),
        )
        .await);
    assert_eq!(held["passed"], true);
    let e = error(
        &client
            .call(
                "ui_expect",
                json!({"locator": "id=push-button", "matcher": "enabled", "timeout_ms": 200}),
            )
            .await,
    );
    assert!(e.starts_with("deadline_exceeded:"), "{e}");
    let e = error(
        &client
            .call("ui_find", json!({"locator": "role=buton"}))
            .await,
    );
    assert!(e.starts_with("invalid:") && e.contains("position 0"), "{e}");
    // Paste is the user's.
    let e = error(
        &client
            .call("ui_act", json!({"action": "press", "keys": "cmd-v"}))
            .await,
    );
    assert!(e.starts_with("reserved:"), "{e}");
    // Screenshots are not there yet.
    let e = error(&client.call("ui_screenshot", json!({})).await);
    assert!(e.starts_with("unsupported:"), "{e}");
    let calls = window.calls.lock().unwrap().clone();
    assert!(!calls.is_empty());
    let root = std::fs::canonicalize(&root).unwrap();
    assert!(
        calls
            .iter()
            .all(|(w, _)| std::fs::canonicalize(w).unwrap() == root),
        "{calls:?}"
    );
}

/// UISEC-6 over MCP and UISEC-13: the password appears in no answer; typed text appears in no
/// log line, only its length.
#[tokio::test]
async fn uisec6_and_13_no_secret_and_no_typed_text_leaves() {
    const TYPED: &str = "the-KNOWN-typed-text";
    let window = Arc::new(DriverWindow::default());
    let (mut client, root) = connect(&[idecraft_core::Grant::Ui], Some(window.clone())).await;
    set_external(&root, true);
    let snap = client.call("ui_snapshot", json!({"format": "json"})).await;
    let find = client
        .call("ui_find", json!({"locator": "id=password"}))
        .await;
    let expect = client
        .call("ui_expect", json!({"locator": "id=password", "matcher": "value", "text": SECRET, "timeout_ms": 100}))
        .await;
    for r in [&snap, &find, &expect] {
        assert!(!r.to_string().contains("hunter2"), "{r}");
    }
    let typing = client
        .call(
            "ui_act",
            json!({"locator": "id=password", "action": "type", "text": "guess"}),
        )
        .await;
    assert!(error(&typing).starts_with("reserved:"));
    let r = client
        .call(
            "ui_act",
            json!({"locator": "id=commit-message", "action": "type", "text": TYPED}),
        )
        .await;
    ok(&r);
    let log = log_of(&root);
    assert!(log.contains("\"typed_length\":20"), "{log}");
    assert!(!log.contains("KNOWN"), "{log}");
    assert!(!log.contains("hunter2"), "{log}");
}

/// UISEC-10 over MCP: `ui_driver_stop` switches the setting off; the next call answers
/// `unavailable`.
#[tokio::test]
async fn uisec10_stop_switches_driving_off() {
    let window = Arc::new(DriverWindow::default());
    let (mut client, root) = connect(&[idecraft_core::Grant::Ui], Some(window.clone())).await;
    set_external(&root, true);
    ok(&client
        .call("ui_find", json!({"locator": "id=commit-button"}))
        .await);
    let before = idecraft_uidriver::security::stop_generation();
    let stop = ok(&client.call("ui_driver_stop", json!({})).await);
    assert_eq!(stop["external"], false);
    assert!(idecraft_uidriver::security::stop_generation() > before);
    assert!(
        !idecraft_core::load_ide_settings(&settings_file(&root))
            .ui
            .driver
            .external
    );
    let e = error(
        &client
            .call("ui_find", json!({"locator": "id=commit-button"}))
            .await,
    );
    assert!(e.starts_with("unavailable:"), "{e}");
    assert!(log_of(&root).contains("ui_driver_stop"));
}
