//! The window commands and screenshots of the UI driver (`SPEC-0319` v7 behaviours 46-54,
//! AK-14, UISEC-15 to UISEC-19) in a window of GPUI's test platform, driven by a scripted MCP
//! client over the window's own HTTP server, found through its discovery file in a settings folder
//! of the test's own — the path a comparison agent takes with `ferris-idea drive`. The user's switch
//! and the grant file are the test's own; the user's files are never read or written.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ferris_idea_mcp::client::Client;
use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use gpui::{Entity, TestAppContext, VisualTestContext, px};
use serde_json::{Value, json};

/// The window tests share the process's registries, so they run one at a time.
pub(crate) static WINDOWS: Mutex<()> = Mutex::new(());

const MAIN: &str = "fn main() {\n    println!(\"hello\");\n}\n";

/// A Cargo project with `src/main.rs`, a secret-like file, a file outside it, and the settings
/// folder beside it with the switch on (or off) and the given rights.
pub(crate) fn project(tag: &str, external: bool, rights: &[&str]) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("ferris-idea-uicmd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join(format!("uicmd-{tag}"));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"uicmd\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), MAIN).unwrap();
    std::fs::write(root.join(".env"), "TOKEN=placeholder\n").unwrap();
    std::fs::write(base.join("outside.txt"), "outside\n").unwrap();
    let settings = base.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    let root = root.canonicalize().unwrap();
    let settings = settings.canonicalize().unwrap();
    std::fs::write(
        settings.join("settings.toml"),
        format!("[mcp]\nhttp = true\n\n[ui.driver]\nexternal = {external}\n"),
    )
    .unwrap();
    let allow: Vec<String> = rights.iter().map(|r| format!("\"{r}\"")).collect();
    std::fs::write(
        settings.join("agent-permissions.toml"),
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [{}]\n",
            root.display(),
            allow.join(", ")
        ),
    )
    .unwrap();
    (root, settings)
}

pub(crate) struct Window {
    pub(crate) bench: Entity<WorkbenchView>,
    pub(crate) cx: &'static mut VisualTestContext,
}

/// The project window as the app opens it, with its MCP server over HTTP and the UI driver's host.
pub(crate) fn window(
    root: &Path,
    settings: &Path,
    external: bool,
    cx: &mut TestAppContext,
) -> Window {
    ferris_idea_workbench::hit_targets::enable();
    cx.executor().allow_parking();
    let settings = settings.to_path_buf();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("ide-settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        let mut ide = ferris_idea_core::IdeSettings::default();
        ide.mcp.http = true;
        ide.ui.driver.external = external;
        cx.set_global(CurrentIdeSettings(ide));
        cx.set_global(ferris_idea_workbench::project_windows::RecentProjectsFile(
            settings.join("recent-projects"),
        ));
        crate::mcp_http::install(Some(settings.clone()), cx);
        crate::ui_driver::install(cx);
    });
    let at = root.to_path_buf();
    let handle = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = handle.root(cx).unwrap();
    let shown = root.to_path_buf();
    cx.update(|cx| {
        ferris_idea_workbench::project_windows::register_window(&shown, handle.into(), cx)
    });
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    Window { bench, cx }
}

pub(crate) fn pump(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    std::thread::sleep(Duration::from_millis(5));
    cx.executor().advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
}

pub(crate) fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        pump(cx);
        if done(cx) {
            return;
        }
    }
    panic!("timed out: {what}");
}

pub(crate) fn agent(settings: &Path) -> Client {
    let found = ferris_idea_mcp::client::running_windows(settings);
    assert_eq!(found.len(), 1, "one window serves MCP over HTTP: {found:?}");
    Client::connect(&found[0], "claude-code").unwrap()
}

/// One call on a thread of its own while the window runs; no question may be asked meanwhile.
pub(crate) fn call(
    w: &mut Window,
    client: Client,
    tool: &'static str,
    args: Value,
) -> (Client, Result<Value, String>) {
    let mut client = client;
    let job = std::thread::spawn(move || {
        let r = client.call(tool, args);
        (client, r)
    });
    until(w.cx, &format!("{tool} answers"), |cx| {
        assert!(
            !cx.has_pending_prompt(),
            "unexpected question: {:?}",
            cx.pending_prompt()
        );
        job.is_finished()
    });
    job.join().unwrap()
}

pub(crate) fn command(
    w: &mut Window,
    client: Client,
    args: Value,
) -> (Client, Result<Value, String>) {
    call(w, client, "ui_command", args)
}

pub(crate) fn log_lines(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &Value| v["tool"].as_str().is_some_and(|t| t.starts_with("ui_")))
        .collect()
}

/// AK-14: a file at a line, Problems and Containers, the ⌥↩ list read, a screenshot answered
/// `unsupported` on the test platform; every call logged.
#[gpui::test]
fn ak14_open_a_file_tool_windows_and_the_context_actions_without_the_screen(
    cx: &mut TestAppContext,
) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, settings) = project("ak14", true, &["ui"]);
    let mut w = window(&root, &settings, true, cx);
    let client = agent(&settings);

    // open_file at line 1 (the line of `fn main`, which has a ▶ marker and so context actions).
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "open_file", "path": "src/main.rs", "line": 1, "column": 4}),
    );
    let r = r.unwrap();
    assert!(
        r["active"]
            .as_str()
            .is_some_and(|a| a.ends_with("src/main.rs")),
        "{r}"
    );
    assert_eq!(r["caret"], json!([1, 4]), "{r}");
    let active = w.cx.update(|_, cx| w.bench.read(cx).active_file());
    assert_eq!(active, Some(root.join("src/main.rs")));

    // Problems and Containers.
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "problems"}),
    );
    assert_eq!(r.unwrap()["shown"], json!(true));
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "containers"}),
    );
    assert_eq!(r.unwrap()["shown"], json!(true));
    let shown = w.cx.update(|_, cx| {
        let b = w.bench.read(cx);
        (
            b.tool_window_shown(ferris_idea_ui::tool_windows::ToolWindow::Problems),
            b.tool_window_shown(ferris_idea_ui::tool_windows::ToolWindow::Containers),
        )
    });
    assert!(shown.1, "Containers is shown");
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "containers", "op": "close"}),
    );
    assert_eq!(r.unwrap()["shown"], json!(false));

    // Back to the editor, then ⌥↩ through the allow list, and the list read.
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "open_file", "path": "src/main.rs", "line": 1, "column": 4}),
    );
    r.unwrap();
    let (mut client, r) = command(
        &mut w,
        client,
        json!({"command": "invoke", "action": "ShowIntentionActions"}),
    );
    r.unwrap();
    let mut popup = Value::Null;
    for _ in 0..50 {
        let (c, r) = command(&mut w, client, json!({"command": "popup"}));
        client = c;
        popup = r.unwrap();
        if popup["open"] == json!(true) {
            break;
        }
    }
    assert_eq!(popup["kind"], "intentions", "{popup}");
    let items = popup["items"].as_array().cloned().unwrap_or_default();
    assert!(!items.is_empty(), "{popup}");
    assert!(
        items
            .iter()
            .all(|i| i["kind"] == "intention" && i["label"].is_string()),
        "{popup}"
    );

    // `dismiss` closes it; opened again, another file closes it too.
    let (client, r) = command(&mut w, client, json!({"command": "dismiss"}));
    assert_eq!(r.unwrap()["closed"], json!(true));
    let (client, r) = command(&mut w, client, json!({"command": "popup"}));
    assert_eq!(r.unwrap()["open"], json!(false));
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "invoke", "action": "ShowIntentionActions"}),
    );
    r.unwrap();
    let open =
        w.cx.update(|_, cx| w.bench.read(cx).intentions_popup().is_some());
    assert!(open, "the context actions are open again");
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "open_file", "path": "Cargo.toml"}),
    );
    r.unwrap();
    let (client, r) = command(&mut w, client, json!({"command": "popup"}));
    assert_eq!(
        r.unwrap()["open"],
        json!(false),
        "no popup over the next file"
    );

    // Typing into the editor without `write` fails at once, naming the right; no question.
    let (client, r) = command(&mut w, client, json!({"command": "type_text", "text": "x"}));
    let e = r.unwrap_err();
    assert!(e.starts_with("not_granted") && e.contains("`write`"), "{e}");

    // The test platform has no read-back of the frame.
    let (client, r) = call(&mut w, client, "ui_screenshot", json!({}));
    let e = r.unwrap_err();
    assert!(e.starts_with("unsupported"), "{e}");

    // list names tool windows and pages, and which pages are reserved.
    let (_client, r) = command(&mut w, client, json!({"command": "list"}));
    let r = r.unwrap();
    assert!(
        r["tool_windows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "containers")
    );
    assert!(
        r["settings_pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "ai-assistant" && p["reserved"] == json!(true))
    );

    let log = log_lines(&root);
    let commands: Vec<&str> = log
        .iter()
        .filter(|l| l["tool"] == "ui_command")
        .filter_map(|l| l["locator"].as_str())
        .collect();
    assert!(
        commands.contains(&"open_file src/main.rs:1:4"),
        "{commands:?}"
    );
    assert!(
        commands.contains(&"tool_window problems open"),
        "{commands:?}"
    );
    assert!(
        commands.contains(&"tool_window containers open"),
        "{commands:?}"
    );
    assert!(
        commands.contains(&"invoke ShowIntentionActions"),
        "{commands:?}"
    );
    assert!(commands.contains(&"popup"), "{commands:?}");
    assert!(
        log.iter()
            .any(|l| l["tool"] == "ui_screenshot" && l["outcome"] == "unsupported")
    );
}

/// UISEC-15 to UISEC-19 through the window: outside the project, the allow list, rights,
/// Settings, typing only where it belongs, and nothing of a typed text in the log.
#[gpui::test]
fn uisec15_to_19_commands_keep_the_users_rights_files_and_pages(cx: &mut TestAppContext) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, settings) = project("uisec", true, &["ui"]);
    let mut w = window(&root, &settings, true, cx);
    let client = agent(&settings);
    let code = |r: &Result<Value, String>| -> String {
        r.as_ref()
            .err()
            .map(|e| e.split(':').next().unwrap_or_default().to_owned())
            .unwrap_or_else(|| "ok".into())
    };
    let outside = root.parent().unwrap().join("outside.txt");
    let mut client = client;
    for (path, want) in [
        ("../outside.txt", "denied"),
        (outside.to_str().unwrap(), "denied"),
        (".env", "reserved"),
        (".ferris-idea/agent-log.jsonl", "reserved"),
    ] {
        let (c, r) = command(
            &mut w,
            client,
            json!({"command": "open_file", "path": path}),
        );
        client = c;
        assert_eq!(code(&r), want, "{path}: {r:?}");
    }
    let opened = w.cx.update(|_, cx| w.bench.read(cx).active_file());
    assert_eq!(opened, None, "nothing opened");

    for action in [
        "SafeDelete",
        "$Paste",
        "ShowSettings",
        "Vcs.Push",
        "CloseProject",
    ] {
        let (c, r) = command(
            &mut w,
            client,
            json!({"command": "invoke", "action": action}),
        );
        client = c;
        assert_eq!(code(&r), "reserved", "{action}: {r:?}");
    }
    // A reserved Settings page.
    let (c, r) = command(
        &mut w,
        client,
        json!({"command": "settings", "page": "ai-assistant"}),
    );
    client = c;
    assert_eq!(code(&r), "reserved", "{r:?}");

    // Typing with no editor or popup focused goes nowhere.
    let (c, r) = command(
        &mut w,
        client,
        json!({"command": "type_text", "text": "hunter2-TYPED"}),
    );
    client = c;
    assert_eq!(code(&r), "reserved", "{r:?}");

    // Settings open: only settings, close_settings, popup and list.
    let (c, r) = command(
        &mut w,
        client,
        json!({"command": "settings", "page": "keymap"}),
    );
    client = c;
    assert_eq!(r.unwrap()["settings"], "keymap");
    let (c, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "problems"}),
    );
    client = c;
    assert_eq!(code(&r), "reserved", "{r:?}");
    let (c, r) = command(&mut w, client, json!({"command": "close_settings"}));
    client = c;
    assert_eq!(r.unwrap()["closed"], json!(true));
    let open = w.cx.update(|_, cx| w.bench.read(cx).settings_open());
    assert!(!open);

    let (_c, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "nope"}),
    );
    assert_eq!(code(&r), "not_found", "{r:?}");

    let log =
        std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl")).unwrap_or_default();
    assert!(
        !log.contains("hunter2-TYPED"),
        "typed text never in the log"
    );
    assert!(log.contains("\"typed_length\":13"), "{log}");
    assert!(log.contains("open_file ../outside.txt"), "{log}");
}

/// UISEC-17: with the switch off nothing works, whatever the grant file gives (the missing right
/// `ui` is checked by the MCP tests of UISEC-3).
#[gpui::test]
fn uisec17_nothing_works_without_the_switch_and_the_right(cx: &mut TestAppContext) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, settings) = project("off", false, &["ui", "write"]);
    let mut w = window(&root, &settings, false, cx);
    let client = agent(&settings);
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "open_file", "path": "src/main.rs"}),
    );
    assert!(r.unwrap_err().starts_with("unavailable"));
    let (_client, r) = call(&mut w, client, "ui_screenshot", json!({}));
    assert!(r.unwrap_err().starts_with("unavailable"));
    let active = w.cx.update(|_, cx| w.bench.read(cx).active_file());
    assert_eq!(active, None);
}
