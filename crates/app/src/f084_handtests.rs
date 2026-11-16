//! Re-checks without a screen of the hand tests that keep F-084 (MCP server for agents) at stage 3
//! in PARITY.md: the steps of hand tests 202, 203, 204 and 206 in a window of GPUI's test platform
//! with the window's own MCP server over HTTP (`mcp_http`: the same bridge, questions and quit
//! step as the app), found through its discovery file in a settings folder of the test's own.
//! A scripted MCP client over HTTP (`idecraft_mcp::client`, client name `claude-code`) takes the
//! place of Claude Code; `idecraft quit` runs as the command does (`quit_with`) with a scripted
//! terminal. The questions the window asks are the platform's alerts, answered with the button a
//! user presses; the project tree gets real key events. The grant file, the recent projects and
//! the settings are the test's own: the user's files are never read or written.
//!
//! The test platform cannot end the process, so a decided quit is counted ([`QuitsInHarness`]);
//! what the app does when the platform then quits — ending the programs of run configurations —
//! is called by the test. What needs eyes stays in the hand tests: the system alert on the
//! screen and its keys (Return, Escape, Space) on a real keyboard, VoiceOver reading it, a real
//! terminal, Claude Code itself, the window going away. Step 8 of 202, step 6 of 204 and step 5 of
//! 206 run the `idecraft` program itself (`tests/it/f084_handtests.rs`).
//!
//! `SPEC-0053` v8 behaviours 2, 3, 6–9 (hand test 202), `SPEC-0012` v3 behaviours 1, 9, 11 (202
//! step 7), `SPEC-0261` v9 behaviours 6–8, 11–13 (203, 204), `SPEC-0281` v2 behaviour 6 (206).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_mcp::client::Client;
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::project_windows::QuitsInHarness;
use serde_json::{Value, json};

type Bench = Entity<WorkbenchView>;

/// The window tests share the process's registry of open documents (what quitting counts as
/// unsaved) and its running programs, so they run one at a time.
static WINDOWS: Mutex<()> = Mutex::new(());

/// A project folder with `src/` and the files given, and the settings folder beside it.
fn project(tag: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("idecraft-f084-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join(format!("handtest-{tag}"));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let settings = base.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    (
        root.canonicalize().unwrap(),
        settings.canonicalize().unwrap(),
    )
}

struct Window {
    bench: Bench,
    cx: &'static mut VisualTestContext,
    /// Decided quits.
    quits: QuitsInHarness,
}

/// The project window of `root` as the app opens it: registered as a project window, served by
/// the window's MCP server over HTTP (`mcp.http` switched on, as the user does in Settings › AI
/// Assistant; it is off by default, SPEC-0221 v3) with the discovery and grant files in `settings`.
fn window(root: &Path, settings: &Path, cx: &mut TestAppContext) -> Window {
    idecraft_workbench::hit_targets::enable();
    cx.executor().allow_parking();
    let quits = QuitsInHarness::default();
    let settings = settings.to_path_buf();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("ide-settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        let mut ide = idecraft_core::IdeSettings::default();
        ide.mcp.http = true;
        cx.set_global(CurrentIdeSettings(ide));
        cx.set_global(idecraft_workbench::project_windows::RecentProjectsFile(
            settings.join("recent-projects"),
        ));
        cx.set_global(quits.clone());
        crate::mcp_http::install(Some(settings.clone()), cx);
    });
    let at = root.to_path_buf();
    let handle = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = handle.root(cx).unwrap();
    let shown = root.to_path_buf();
    cx.update(|cx| idecraft_workbench::project_windows::register_window(&shown, handle.into(), cx));
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    Window { bench, cx, quits }
}

/// Lets the window's tasks and timers run for a moment of real time.
fn pump(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    std::thread::sleep(Duration::from_millis(5));
    cx.executor().advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
}

fn until(
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

/// A client of the window's MCP server over HTTP, found through the discovery file, with a new
/// MCP session, named as Claude Code names itself.
fn agent(settings: &Path, name: &str) -> Client {
    let found = idecraft_mcp::client::running_windows(settings);
    assert_eq!(found.len(), 1, "one window serves MCP over HTTP: {found:?}");
    assert_eq!(found[0].pid, std::process::id());
    Client::connect(&found[0], name).unwrap()
}

/// One tool call of a client on a thread of its own, so the window can ask meanwhile.
type Call = std::thread::JoinHandle<(Client, Result<Value, String>)>;

fn start(mut client: Client, tool: &'static str, arguments: Value) -> Call {
    std::thread::spawn(move || {
        let result = client.call(tool, arguments);
        (client, result)
    })
}

/// Waits until the call shows a question in the window: its message and detail.
fn question(cx: &mut VisualTestContext, call: &Call) -> (String, String) {
    until(cx, "the window asks", |cx| {
        assert!(
            !call.is_finished() || cx.has_pending_prompt(),
            "the call ended without a question"
        );
        cx.has_pending_prompt()
    });
    cx.pending_prompt().unwrap()
}

/// Answers the question shown with the button `label`.
fn answer(cx: &mut VisualTestContext, label: &str) {
    cx.simulate_prompt_answer(label);
    pump(cx);
}

/// Waits for the end of the call; no question may be shown on the way.
fn finish(cx: &mut VisualTestContext, call: Call) -> (Client, Result<Value, String>) {
    until(cx, "the call ends", |cx| {
        assert!(
            !cx.has_pending_prompt(),
            "unexpected question: {:?}",
            cx.pending_prompt()
        );
        call.is_finished()
    });
    call.join().unwrap()
}

/// Opens `file` in an editor and types `text` at its start, without saving.
fn edit(w: &mut Window, file: &Path, text: &str) {
    w.bench.update_in(w.cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    w.cx.run_until_parked();
    w.cx.simulate_input(text);
    w.cx.run_until_parked();
    let (unsaved, _) = w.cx.update(|_, cx| WorkbenchView::pending_work(cx));
    assert!(
        unsaved.iter().any(|f| file.ends_with(f)),
        "the edit is unsaved: {unsaved:?}"
    );
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Hand test 202 steps 1–7 (`SPEC-0053` v8 behaviours 2, 3, 6–9, AK-7, AK-8; `SPEC-0012` v3
/// behaviour 9, AK-8): an MCP client over HTTP without `write` is asked in the window; Deny (the
/// default) refuses with `denied:` and creates nothing, the session is not asked again; a new
/// session is asked again; Allow Once allows one call; Always for This Workspace adds only its own
/// `[[workspace]]` entry to the grant file and is not asked again; a database permission is asked
/// for the connection and Always for This Connection writes a `[[connection]]` entry to the grant
/// file, not to the project file.
#[gpui::test]
fn hand_test_202_the_window_asks_http_clients_for_a_right(cx: &mut TestAppContext) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, settings) = project("202", &[]);
    // 1. The grant file names other rights and another folder, but neither `write` nor
    // `vcs-write` for this workspace.
    let grants = settings.join(idecraft_core::GRANT_FILE);
    let before = "# Rechte für Agenten, von Hand\n[[workspace]]\npath = \"/somewhere/else\"\nallow = [\"write\", \"vcs-write\"]\n\n[[workspace]]\npath = \"ROOT\"\nallow = [\"run\"]\n"
        .replace("ROOT", &root.display().to_string());
    std::fs::write(&grants, &before).unwrap();
    // The database of step 7, and the project trusted so its connection is usable.
    let db = root.join("data.sqlite");
    let sqlite = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg("create table t (v int); insert into t values (1), (2);")
        .status()
        .is_ok_and(|s| s.success());
    idecraft_core::trust::trust_for_process(&root);

    // 2. The window serves MCP over HTTP; the client finds URL and token in the discovery file.
    let w = window(&root, &settings, cx);
    let discovery = settings.join(format!("mcp-http/{}.json", std::process::id()));
    let found: Value = serde_json::from_str(&read(&discovery)).unwrap();
    assert!(
        found["token"].as_str().is_some_and(|t| t.len() >= 32),
        "{found}"
    );
    assert_eq!(found["projects"][0]["root"], json!(root));
    let client = agent(&settings, "claude-code");

    // 3. create_file without `write`: the window asks, naming client, right with its meaning,
    // action and scope.
    let call = start(
        client,
        "create_file",
        json!({"dir": "src", "name": "recht1.txt"}),
    );
    let (message, detail) = question(w.cx, &call);
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        message,
        format!(
            "Allow \"claude-code {}\" the right \"write\" in workspace \"{name}\"?",
            version()
        )
    );
    assert!(
        detail.contains(&format!("Client: claude-code {}", version())),
        "{detail}"
    );
    assert!(
        detail.contains("Right: write (") && detail.contains("files of the workspace"),
        "{detail}"
    );
    assert!(
        detail.contains("Action: create_file {\"dir\":\"src\",\"name\":\"recht1.txt\"}"),
        "{detail}"
    );
    assert!(
        detail.contains(&format!("Scope: workspace {}", root.display())),
        "{detail}"
    );
    // Return in the alert is its first button: Deny.
    answer(w.cx, "Deny");
    let (client, denied) = finish(w.cx, call);
    let message = denied.expect_err("denied");
    assert!(message.starts_with("denied:"), "{message}");
    assert!(message.contains("`write` right"), "{message}");
    assert!(
        !root.join("src/recht1.txt").exists(),
        "a denied call creates no file"
    );

    // 4. The same again in that session: no question, denied again.
    let call = start(
        client,
        "create_file",
        json!({"dir": "src", "name": "recht1.txt"}),
    );
    let (_, again) = finish(w.cx, call);
    assert!(again.expect_err("denied").starts_with("denied:"));
    assert!(!root.join("src/recht1.txt").exists());

    // 5. A new session is asked again; Allow Once creates the file and the next file is asked.
    let client = agent(&settings, "claude-code");
    let call = start(
        client,
        "create_file",
        json!({"dir": "src", "name": "recht1.txt"}),
    );
    let (message, _) = question(w.cx, &call);
    assert!(message.contains("the right \"write\""), "{message}");
    answer(w.cx, "Allow Once");
    let (client, created) = finish(w.cx, call);
    assert!(created.is_ok(), "{created:?}");
    assert!(root.join("src/recht1.txt").exists());
    assert_eq!(read(&grants), before, "Allow Once writes no grant");
    let call = start(
        client,
        "create_file",
        json!({"dir": "src", "name": "recht2.txt"}),
    );
    let (message, _) = question(w.cx, &call);
    assert!(message.contains("the right \"write\""), "{message}");
    answer(w.cx, "Always for This Workspace");
    let (client, created) = finish(w.cx, call);
    assert!(created.is_ok(), "{created:?}");
    assert!(root.join("src/recht2.txt").exists());
    let call = start(
        client,
        "create_file",
        json!({"dir": "src", "name": "recht3.txt"}),
    );
    let (client, created) = finish(w.cx, call);
    assert!(created.is_ok(), "{created:?}");
    assert!(root.join("src/recht3.txt").exists());

    // 6. The grant file: the old text unchanged, one new entry for this workspace with `write`.
    let after = read(&grants);
    assert_eq!(
        after,
        format!(
            "{before}\n[[workspace]]\npath = \"{}\"\nallow = [\"write\"]\n",
            root.display()
        ),
        "only the workspace's own entry is added"
    );
    // A new session reads it: granted without a question.
    let call = start(
        agent(&settings, "claude-code"),
        "create_file",
        json!({"dir": "src", "name": "recht4.txt"}),
    );
    let (_, created) = finish(w.cx, call);
    assert!(created.is_ok(), "{created:?}");

    // 7. A SQLite connection whose project file lets agents change data: deleting a row asks for
    // `modify-data` on the connection; Always for This Connection deletes it and writes the grant
    // to the user's grant file, not to the project file (`SPEC-0012` v3).
    if !sqlite {
        eprintln!("note: hand test 202 step 7 skipped: no sqlite3 to make the database");
        return;
    }
    let connections = root.join(".idecraft/connections.toml");
    std::fs::write(
        &connections,
        format!(
            "[[connection]]\nid = \"lite\"\nname = \"lite\"\ndialect = \"sqlite\"\npath = \"{}\"\nai = [\"read-schema\", \"read-data\", \"modify-data\"]\n",
            db.display()
        ),
    )
    .unwrap();
    let project_file = read(&connections);
    let call = start(
        client,
        "run_sql",
        json!({"connection": "lite", "sql": "delete from t where v = 1", "apply": true}),
    );
    let (message, detail) = question(w.cx, &call);
    assert_eq!(
        message,
        format!(
            "Allow \"claude-code {}\" the right \"modify-data\" in connection \"lite\"?",
            version()
        )
    );
    assert!(
        detail.contains("; on sqlite:"),
        "the target is named: {detail}"
    );
    assert!(
        detail.contains(&format!(
            "Scope: connection \"lite\" of workspace {}",
            root.display()
        )),
        "{detail}"
    );
    answer(w.cx, "Always for This Connection");
    let (_, deleted) = finish(w.cx, call);
    assert!(deleted.is_ok(), "{deleted:?}");
    let left = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg("select group_concat(v) from t;")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&left.stdout).trim(),
        "2",
        "the row is deleted"
    );
    let granted = read(&grants);
    assert!(
        granted.starts_with(&after),
        "the earlier entries stay: {granted}"
    );
    let added = &granted[after.len()..];
    assert!(
        added.contains("[[connection]]")
            && added.contains("connection = \"lite\"")
            && added.contains("allow = [\"modify-data\"]"),
        "{added}"
    );
    assert_eq!(
        read(&connections),
        project_file,
        "the project file is not written"
    );
    drop(w);
}

/// A run configuration `sleeper` that runs `sleep 600`, started in the window as Run does, until
/// the window counts it as running.
fn start_sleeper(w: &mut Window, root: &Path) {
    std::fs::write(
        root.join(".idecraft/run.toml"),
        "[[run]]\nname = \"sleeper\"\ncommand = [\"sleep\", \"600\"]\n",
    )
    .unwrap();
    w.bench
        .update(w.cx, |b, cx| b.select_run_configuration("sleeper", cx));
    w.bench
        .update_in(w.cx, |b, window, cx| b.run_selected(window, cx));
    until(w.cx, "the run configuration runs", |cx| {
        let (_, running) = cx.update(|_, cx| WorkbenchView::pending_work(cx));
        !running.is_empty()
    });
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Hand test 203 (`SPEC-0261` v9 behaviours 6–8 and 13, `SPEC-0053` v8 behaviour 13): with an
/// unsaved file and a running configuration, `quit_ide` without arguments quits nothing and names
/// both with the options; `pending=discard` asks for `quit` naming client, action and scope, Deny
/// keeps the window; in a new session Allow Once quits without a further question, the change is
/// discarded, the run is listed as stopped and ends with the quit, the agent log has the call.
#[gpui::test]
fn hand_test_203_quit_ide_with_an_unsaved_file(cx: &mut TestAppContext) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    // A failed step leaves no `sleep` behind: the test's own runs end with it.
    struct EndRuns;
    impl Drop for EndRuns {
        fn drop(&mut self) {
            idecraft_core::end_all_runs(Duration::from_millis(200));
        }
    }
    let _runs = EndRuns;
    let (root, settings) = project("203", &[("src/notes.txt", "saved\n")]);
    idecraft_core::trust::trust_for_process(&root);
    let mut w = window(&root, &settings, cx);
    let file = root.join("src/notes.txt");

    // 1. A running configuration and an unsaved change. In this order: Run saves the modified
    // files before it starts (`SPEC-0087` v5 behaviour 3a), so a change made first is saved.
    start_sleeper(&mut w, &root);
    assert_eq!(idecraft_core::live_runs(), 1);
    edit(&mut w, &file, "typed ");

    // 2. quit_ide without arguments: nothing quits, no question; the result names the file, the
    // run and the options.
    let call = start(agent(&settings, "claude-code"), "quit_ide", json!({}));
    let (client, listed) = finish(w.cx, call);
    let listed = listed.unwrap();
    assert_eq!(listed["quit"], json!(false), "{listed}");
    assert_eq!(strings(&listed["unsaved"]), ["src/notes.txt"], "{listed}");
    let running = strings(&listed["running"]);
    assert_eq!(running.len(), 1, "{listed}");
    assert!(
        running[0].contains("run configuration \"sleeper\""),
        "{listed}"
    );
    assert_eq!(
        strings(&listed["options"]),
        ["save_all", "discard", "cancel"]
    );
    assert_eq!(w.quits.0.get(), 0, "IDECraft stays open");

    // 3. pending=discard asks for `quit`; Deny keeps everything.
    let call = start(client, "quit_ide", json!({"pending": "discard"}));
    let (message, detail) = question(w.cx, &call);
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        message,
        format!(
            "Allow \"claude-code {}\" the right \"quit\" in workspace \"{name}\"?",
            version()
        )
    );
    assert!(
        detail.contains("Action: quit_ide {\"pending\":\"discard\"}"),
        "{detail}"
    );
    assert!(
        detail.contains(&format!("Scope: workspace {}", root.display())),
        "{detail}"
    );
    answer(w.cx, "Deny");
    let (_, denied) = finish(w.cx, call);
    assert!(denied.expect_err("denied").starts_with("denied:"));
    assert_eq!(w.quits.0.get(), 0, "IDECraft stays open after Deny");
    assert_eq!(idecraft_core::live_runs(), 1, "the run goes on");

    // 4. A new session, the same call, Allow Once: IDECraft quits without a further question.
    let call = start(
        agent(&settings, "claude-code"),
        "quit_ide",
        json!({"pending": "discard"}),
    );
    let (message, _) = question(w.cx, &call);
    assert!(message.contains("the right \"quit\""), "{message}");
    answer(w.cx, "Allow Once");
    let (_, quit) = finish(w.cx, call);
    let quit = quit.unwrap();
    assert_eq!(quit["quit"], json!(true), "{quit}");
    assert_eq!(strings(&quit["discarded"]), ["src/notes.txt"], "{quit}");
    assert_eq!(strings(&quit["stopped"]), running, "{quit}");
    until(w.cx, "the quit is decided", |_| w.quits.0.get() == 1);
    assert_eq!(read(&file), "saved\n", "the change is discarded");
    // What the app does when the platform quits (crates/app/src/workbench.rs): the window's run
    // is one of the programs that end with IDECraft.
    assert_eq!(idecraft_core::end_all_runs(Duration::from_millis(500)), 1);
    assert_eq!(
        idecraft_core::live_runs(),
        0,
        "the run configuration is ended"
    );
    let log = read(&root.join(idecraft_core::AGENT_LOG));
    assert!(
        log.lines().any(|l| l.contains("\"tool\":\"quit_ide\"")),
        "the agent log has the call: {log}"
    );
    drop(w);
}

/// Hand test 203 step 5 (`SPEC-0261` v9 behaviours 7 and 8; v8: quitting through MCP needs
/// `quit` also with nothing pending): after a restart, nothing changed, `quit_ide` asks for
/// `quit` once and quits after Allow Once.
#[gpui::test]
fn hand_test_203_quit_ide_with_nothing_pending(cx: &mut TestAppContext) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, settings) = project("203-nothing", &[("src/notes.txt", "saved\n")]);
    let w = window(&root, &settings, cx);
    assert_eq!(
        w.cx.update(|_, cx| WorkbenchView::pending_work(cx)),
        (Vec::new(), Vec::new()),
        "nothing pending"
    );
    let call = start(agent(&settings, "claude-code"), "quit_ide", json!({}));
    let (message, detail) = question(w.cx, &call);
    assert!(message.contains("the right \"quit\""), "{message}");
    assert!(detail.contains("Action: quit_ide\n"), "{detail}");
    answer(w.cx, "Allow Once");
    let (_, quit) = finish(w.cx, call);
    assert_eq!(quit.unwrap()["quit"], json!(true));
    until(w.cx, "the quit is decided", |_| w.quits.0.get() == 1);
    drop(w);
}

/// `idecraft quit` with `args`, run as the command runs it, with the settings folder of the test
/// and a terminal that answers `typed` (`None`: standard input is no terminal); on a thread of its
/// own. Its exit status, standard output and standard error.
type Quit = std::thread::JoinHandle<(u8, String, String)>;

fn idecraft_quit(settings: &Path, args: &[&str], typed: Option<&str>) -> Quit {
    let settings = settings.to_path_buf();
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let typed = typed.map(str::to_owned);
    std::thread::spawn(move || {
        let mut input = std::io::Cursor::new(typed.clone().unwrap_or_default().into_bytes());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::quit_with(
            &args,
            Some(&settings),
            &mut crate::Console {
                terminal: typed.is_some(),
                input: &mut input,
                out: &mut out,
                err: &mut err,
            },
        );
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    })
}

/// Waits for `idecraft quit`, answering each question the window shows with Allow Once; the
/// questions asked, as message and detail.
fn allow_each(
    cx: &mut VisualTestContext,
    quit: Quit,
) -> ((u8, String, String), Vec<(String, String)>) {
    let mut asked = Vec::new();
    until(cx, "idecraft quit ends", |cx| {
        if let Some(q) = cx.pending_prompt() {
            asked.push(q);
            cx.simulate_prompt_answer("Allow Once");
        }
        quit.is_finished()
    });
    (quit.join().unwrap(), asked)
}

/// Hand test 204 steps 1–5 (`SPEC-0261` v9 behaviours 7, 8, 11 and 12): `idecraft quit` with
/// nothing pending asks for `quit` as client `idecraft-cli` and quits; with an unsaved file it
/// lists it and asks on the terminal, `c` cancels; `s` asks for `quit` and `write`, saves and
/// quits; without a terminal it names the flags and exits with 2; `--discard` quits without
/// saving.
#[gpui::test]
fn hand_test_204_idecraft_quit_in_the_terminal(cx: &mut TestAppContext) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, settings) = project("204", &[("src/notes.txt", "saved\n")]);
    let mut w = window(&root, &settings, cx);
    let file = root.join("src/notes.txt");
    let pid = std::process::id();
    let client = format!("\"idecraft-cli {}\"", version());

    // 1. Nothing changed: one question for `quit`, then "IDECraft (pid …) quit" and status 0.
    let ((code, out, err), asked) = allow_each(w.cx, idecraft_quit(&settings, &[], Some("")));
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(out, format!("IDECraft (pid {pid}) quit\n"));
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].0.contains(&client) && asked[0].0.contains("the right \"quit\""),
        "{asked:?}"
    );
    until(w.cx, "the quit is decided", |_| w.quits.0.get() == 1);

    // 2. An unsaved file: listed, the terminal asks; `c` cancels, nothing is asked in the window.
    edit(&mut w, &file, "typed ");
    let ((code, out, err), asked) = allow_each(w.cx, idecraft_quit(&settings, &[], Some("c\n")));
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(
        out,
        "Unsaved files:\n  src/notes.txt\n[s]ave all and quit, [d]iscard and quit, [c]ancel? Cancelled; IDECraft keeps running\n"
    );
    assert!(asked.is_empty(), "{asked:?}");
    assert_eq!(w.quits.0.get(), 1, "IDECraft keeps running");
    assert_eq!(read(&file), "saved\n");

    // 3. `s`: questions for `quit` and `write`, both naming idecraft-cli; saved, then quit.
    let ((code, out, err), asked) = allow_each(w.cx, idecraft_quit(&settings, &[], Some("s\n")));
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.ends_with(&format!(
            "[s]ave all and quit, [d]iscard and quit, [c]ancel? IDECraft (pid {pid}) quit\nSaved:\n  src/notes.txt\n"
        )),
        "{out}"
    );
    let rights: Vec<String> = asked
        .iter()
        .map(|(m, _)| {
            assert!(m.contains(&client), "{m}");
            m.split("the right \"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(rights, ["quit", "write"]);
    until(w.cx, "the quit is decided", |_| w.quits.0.get() == 2);
    assert_eq!(read(&file), "typed saved\n", "the file is saved");

    // 4. Changed again, standard input no terminal: listed, the flags named, status 2.
    edit(&mut w, &file, "more ");
    let ((code, out, err), asked) = allow_each(w.cx, idecraft_quit(&settings, &[], None));
    assert_eq!(code, 2, "{out}{err}");
    assert_eq!(out, "Unsaved files:\n  src/notes.txt\n");
    assert!(
        err.contains("--save-all") && err.contains("--discard"),
        "{err}"
    );
    assert!(asked.is_empty(), "{asked:?}");
    assert_eq!(w.quits.0.get(), 2, "IDECraft keeps running");

    // 5. --discard: one question for `quit`; IDECraft quits, the change is discarded.
    let ((code, out, err), asked) =
        allow_each(w.cx, idecraft_quit(&settings, &["--discard"], None));
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(out, format!("IDECraft (pid {pid}) quit\n"));
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].0.contains("the right \"quit\""), "{asked:?}");
    until(w.cx, "the quit is decided", |_| w.quits.0.get() == 3);
    assert_eq!(read(&file), "typed saved\n", "the change is discarded");
    drop(w);
}

/// The middle of the project tree row named `name` as it was last drawn.
fn row(cx: &mut VisualTestContext, name: &str) -> gpui::Point<gpui::Pixels> {
    cx.run_until_parked();
    let r = idecraft_workbench::hit_targets::get(&format!("tree-{name}"))
        .unwrap_or_else(|| panic!("the row {name} is drawn"));
    point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
}

/// The nodes of the semantic tree of a frame drawn now (`SPEC-0319` behaviour 1): role, name and
/// value.
fn semantic_nodes(cx: &mut VisualTestContext) -> Vec<(String, String, String)> {
    cx.update(|window, cx| {
        window.draw(cx).clear();
        let tree =
            idecraft_workbench::semantics::tree(idecraft_workbench::semantics::window_id(window))
                .expect("the frame has a semantic tree");
        tree.iter()
            .map(|n| {
                (
                    format!("{:?}", n.role()),
                    n.name().unwrap_or_default().to_owned(),
                    n.raw_value().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    })
}

/// Hand test 206 steps 1–4 (`SPEC-0281` v2 behaviour 6, `SPEC-0275` behaviour 6): Delete in the
/// project tree — from the context menu and with ⌦ — asks in the platform's alert, not in a
/// question drawn by IDECraft: message "Delete file "…"?", detail "Deleted files stay in Local
/// History.", buttons Delete and Cancel. While it is shown, the window's semantic tree holds no
/// drawn copy of the question, so what a screen reader reads is the system alert alone. Cancel
/// keeps the file; Delete deletes it and closes its tab; a folder's question names what it holds.
#[gpui::test]
fn hand_test_206_delete_asks_in_the_native_alert(cx: &mut TestAppContext) {
    use idecraft_workbench::semantics::{Consumer, set_consumer};
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, settings) = project(
        "206",
        &[
            ("src/a.txt", "a\n"),
            ("src/keep.txt", "keep\n"),
            ("pack/in.txt", "in\n"),
            ("pack/sub/deep.txt", "deep\n"),
        ],
    );
    let w = window(&root, &settings, cx);
    set_consumer(Consumer::SelfTest, true);
    let file = root.join("src/a.txt");
    w.bench.update_in(w.cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.project_tree_call(&json!({"action": "expand", "path": root.join("src")}), cx)
            .unwrap();
        b.focus_tree(window, cx);
    });
    w.cx.run_until_parked();
    assert!(
        w.bench
            .read_with(w.cx, |b, _| b.main_tab_titles())
            .contains(&"a.txt".to_owned())
    );

    // 1. Right click › Delete…: the system alert with the question, its detail and the buttons.
    let at = row(w.cx, "a.txt");
    w.cx.simulate_click(at, Modifiers::none());
    w.cx.simulate_mouse_down(at, gpui::MouseButton::Right, Modifiers::none());
    w.cx.simulate_mouse_up(at, gpui::MouseButton::Right, Modifiers::none());
    w.cx.run_until_parked();
    let chosen = w.bench.update_in(w.cx, |b, window, cx| {
        b.choose_context_item("Delete…", window, cx)
    });
    assert!(chosen, "the menu has Delete…");
    w.cx.run_until_parked();
    assert_eq!(
        w.cx.pending_prompt(),
        Some((
            "Delete file \"a.txt\"?".to_owned(),
            "Deleted files stay in Local History.".to_owned()
        ))
    );
    assert!(
        w.bench.read_with(w.cx, |b, _| b.drawn_question().is_none()),
        "nothing is drawn in the window instead"
    );
    // 2. What assistive technology reads is the system alert: no node of the window's own tree
    // carries the question.
    let nodes = semantic_nodes(w.cx);
    assert!(!nodes.is_empty(), "the window has a semantic tree");
    assert!(
        nodes
            .iter()
            .all(|(_, name, value)| !name.contains("Delete file") && !value.contains("Delete file")),
        "{nodes:?}"
    );
    // A second Delete while the alert is shown asks no second question.
    w.bench
        .update_in(w.cx, |b, window, cx| b.focus_tree(window, cx));
    w.cx.simulate_keystrokes("delete");
    w.cx.run_until_parked();
    // Escape is the alert's Cancel: the file stays.
    answer(w.cx, "Cancel");
    assert!(!w.cx.has_pending_prompt(), "one question only");
    assert!(file.exists());

    // 3. ⌦ on the selected file, then Return: the alert's default button Delete.
    let at = row(w.cx, "a.txt");
    w.cx.simulate_click(at, Modifiers::none());
    w.cx.simulate_keystrokes("delete");
    w.cx.run_until_parked();
    assert_eq!(
        w.cx.pending_prompt().map(|(m, _)| m).as_deref(),
        Some("Delete file \"a.txt\"?")
    );
    answer(w.cx, "Delete");
    assert!(!file.exists(), "the file is deleted");
    assert!(root.join("src/keep.txt").exists());
    assert!(
        !w.bench
            .read_with(w.cx, |b, _| b.main_tab_titles())
            .contains(&"a.txt".to_owned()),
        "its tab is closed"
    );

    // 4. A folder: the question names how many files and folders go; Escape keeps everything.
    let at = row(w.cx, "pack");
    w.cx.simulate_click(at, Modifiers::none());
    w.cx.simulate_keystrokes("delete");
    w.cx.run_until_parked();
    assert_eq!(
        w.cx.pending_prompt(),
        Some((
            "Delete folder \"pack\" with 2 files and 1 folder?".to_owned(),
            "Deleted files stay in Local History.".to_owned()
        ))
    );
    answer(w.cx, "Cancel");
    assert!(root.join("pack/in.txt").exists() && root.join("pack/sub/deep.txt").exists());
    set_consumer(Consumer::SelfTest, false);
    drop(w);
}
