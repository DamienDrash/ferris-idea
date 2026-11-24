//! Re-checks without a screen of the open hand tests of PARITY row F-082 (AI assistance): 64, 69,
//! 83, 86, 88, 213 and 214, done on 2026-11-14. Real key and mouse events go to a window of the
//! test harness; the assistant talks to a fake OpenAI-compatible endpoint this test starts on
//! 127.0.0.1, and the subscriptions to stand-in programs `claude`, `codex` and `gemini` written by
//! the test. No real provider, no real key, no network beyond the loopback address, no keychain:
//! API keys go to a test secrets folder (debug builds only), and terminals that would start a real
//! program start a recorder instead.
//!
//! Security checks on the way: nothing is sent while the assistant is off (the default); the API
//! key is read from the secrets folder and appears only in the `Authorization` header — never in a
//! request body, the settings file, a file of the project or settings folder, a chip, the chat's
//! state that MCP reads, or an error the endpoint repeats it in; secrets are masked before they
//! leave (chips, inline completion); a terminal is read only after the person's action.
//!
//! What needs eyes on a real window stays in the hand tests: how the chat, chips and grey text are
//! drawn, the answer appearing word by word on screen (the harness runs the request in one step),
//! a real provider's answers and sign-in, and the comparison with the reference.
//!
//! `SPEC-0138` v3, `SPEC-0143`, `SPEC-0157` v2, `SPEC-0160`, `SPEC-0162` v2, `SPEC-0313`.

#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::settings_model::Page;
use ferris_idea_workbench::{AiChatView, SettingsView, WorkbenchView, semantics};
use gpui::{
    Entity, Keystroke, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};

type Bench = Entity<WorkbenchView>;

/// An obviously fake key of the OpenAI shape (no credential).
const KEY: &str = "sk-test-FAKEFAKEFAKEFAKEFAKE0123";
/// A wrong one, which the fake endpoint repeats in its error as some providers do.
const WRONG_KEY: &str = "sk-test-WRONGWRONGWRONGWRONG987";

// ---------------------------------------------------------------------------------------------
// The fake endpoint
// ---------------------------------------------------------------------------------------------

/// A request the fake endpoint received.
#[derive(Debug, Clone, Default)]
struct Seen {
    line: String,
    authorization: Option<String>,
    /// Every header line except `Authorization`.
    headers: String,
    body: String,
}

impl Seen {
    fn chat(&self) -> bool {
        self.line.contains("/chat/completions")
    }
    fn inline(&self) -> bool {
        self.body.contains("You complete code")
    }
}

/// An OpenAI-compatible endpoint on 127.0.0.1: `GET /models` lists two models, chat requests are
/// answered by `answer` (as server-sent events, one per word, when asked to stream). With `key`
/// set, a request without `Authorization: Bearer key` gets 401 with a message repeating the key
/// it was sent.
struct Fake {
    endpoint: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Fake {
    fn start(key: Option<&'static str>, answer: fn(&Seen) -> String) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = log.clone();
                std::thread::spawn(move || serve(stream, key, answer, &log));
            }
        });
        Fake {
            endpoint: format!("http://127.0.0.1:{port}/v1"),
            seen,
        }
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn chats(&self) -> Vec<Seen> {
        self.seen().into_iter().filter(|s| s.chat()).collect()
    }
}

fn serve(
    stream: std::net::TcpStream,
    key: Option<&str>,
    answer: fn(&Seen) -> String,
    log: &Mutex<Vec<Seen>>,
) {
    let mut reader = BufReader::new(stream);
    let mut s = Seen::default();
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    s.line = line.trim().to_owned();
    let mut length = 0;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((&h, ""));
        if k.eq_ignore_ascii_case("content-length") {
            length = v.trim().parse().unwrap_or(0);
        }
        if k.eq_ignore_ascii_case("authorization") {
            s.authorization = Some(v.trim().to_owned());
        } else {
            s.headers.push_str(&h);
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);
    s.body = String::from_utf8_lossy(&body).into_owned();
    log.lock().unwrap().push(s.clone());
    let mut out = reader.into_inner();
    let json = |out: &mut std::net::TcpStream, status: u16, payload: serde_json::Value| {
        let payload = payload.to_string();
        let _ = write!(
            out,
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
    };
    if let Some(k) = key
        && s.authorization.as_deref() != Some(format!("Bearer {k}").as_str())
    {
        let sent = s
            .authorization
            .as_deref()
            .and_then(|a| a.strip_prefix("Bearer "))
            .unwrap_or("none");
        let message = format!("Incorrect API key provided: {sent}");
        return json(
            &mut out,
            401,
            serde_json::json!({"error": {"message": message}}),
        );
    }
    if s.line.contains("/models ") {
        return json(
            &mut out,
            200,
            serde_json::json!({"data": [{"id": "fake-small"}, {"id": "fake-large"}]}),
        );
    }
    let reply = answer(&s);
    if s.body.contains("\"stream\":true") {
        let _ = write!(
            out,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
        );
        for word in reply.split_inclusive(' ') {
            let event = serde_json::json!({"choices": [{"index": 0, "delta": {"content": word}}]});
            if write!(out, "data: {event}\n\n").is_err() || out.flush().is_err() {
                return;
            }
        }
        let _ = write!(out, "data: [DONE]\n\n");
        return;
    }
    json(
        &mut out,
        200,
        serde_json::json!({"choices": [{"message": {"role": "assistant", "content": reply}}]}),
    );
}

/// An address on 127.0.0.1 where nothing listens.
fn unreachable_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}/v1")
}

// ---------------------------------------------------------------------------------------------
// The secrets folder and the stand-in programs
// ---------------------------------------------------------------------------------------------

fn mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// The folder of this process's test secrets and of the stand-in programs: the keychain is never
/// used, and `claude`, `codex` and `gemini` are found there first.
fn harness() -> (PathBuf, PathBuf) {
    static DIRS: std::sync::OnceLock<(PathBuf, PathBuf)> = std::sync::OnceLock::new();
    DIRS.get_or_init(|| {
        let base = std::env::temp_dir()
            .join(format!("ferris-idea-ai-handtests-{}", std::process::id()))
            .canonicalize()
            .unwrap_or_else(|_| {
                let p = std::env::temp_dir()
                    .join(format!("ferris-idea-ai-handtests-{}", std::process::id()));
                std::fs::create_dir_all(&p).unwrap();
                p.canonicalize().unwrap()
            });
        let secrets = base.join("secrets");
        std::fs::create_dir_all(&secrets).unwrap();
        mode(&secrets, 0o700);
        let secrets = ferris_idea_database::test_store::use_dir(secrets);
        let programs = base.join("bin");
        std::fs::create_dir_all(&programs).unwrap();
        write_programs(&programs);
        let programs = ferris_idea_ai::subscription::use_test_programs(programs);
        (secrets, programs)
    })
    .clone()
}

/// What the stand-in programs were started with: one block per start, `cwd`, then each argument
/// in brackets, then `stdin:` and the prompt.
fn program_log() -> String {
    std::fs::read_to_string(harness().1.join("log")).unwrap_or_default()
}

/// `claude`, `codex` and `gemini` answering as the real programs do in their JSON modes, and a
/// `recorder` shell that writes what is typed into it to `typed`.
fn write_programs(dir: &Path) {
    let log = dir.join("log");
    let log = log.display();
    let common = format!(
        "#!/bin/sh\n{{ echo \"== $(basename \"$0\")\"; echo \"cwd $(pwd)\"; for a in \"$@\"; do printf '[%s]\\n' \"$a\"; done; }} >> '{log}'\n"
    );
    let claude = format!(
        "{common}\
case \"$1\" in\n\
  auth) [ \"$2\" = status ] && echo '{{\"loggedIn\":true}}'; exit 0;;\n\
esac\n\
prompt=$(cat)\n\
printf 'stdin: %s\\n' \"$prompt\" >> '{log}'\n\
case \"$prompt\" in\n\
  *'<CURSOR>'*) echo '{{\"type\":\"result\",\"result\":\"a * b\"}}'; exit 0;;\n\
esac\n\
for w in 'Claude ' 'answers ' 'in ' 'pieces.'; do\n\
  printf '{{\"type\":\"stream_event\",\"event\":{{\"delta\":{{\"type\":\"text_delta\",\"text\":\"%s\"}}}}}}\\n' \"$w\"\n\
done\n\
echo '{{\"type\":\"result\",\"result\":\"Claude answers in pieces.\"}}'\n"
    );
    let codex = format!(
        "{common}\
[ \"$1\" = login ] && exit 0\n\
prompt=$(cat)\n\
printf 'stdin: %s\\n' \"$prompt\" >> '{log}'\n\
echo '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"Codex answers at once.\"}}}}'\n"
    );
    let gemini = format!(
        "{common}\
for w in 'Gemini ' 'answers.'; do\n\
  printf '{{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"%s\"}}\\n' \"$w\"\n\
done\n"
    );
    let typed = dir.join("typed");
    let recorder = format!("#!/bin/sh\nexec /bin/cat >> '{}'\n", typed.display());
    for (name, text) in [
        ("claude", claude),
        ("codex", codex),
        ("gemini", gemini),
        ("recorder", recorder),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        mode(&path, 0o755);
    }
}

/// The logged start of `program` whose arguments or prompt contain `needle` (other tests may start
/// the stand-ins at the same time).
fn started(log: &str, program: &str, needle: &str) -> String {
    log.split(&format!("== {program}\n"))
        .skip(1)
        .map(|block| block.split("\n== ").next().unwrap_or(block))
        .find(|block| block.contains(needle))
        .unwrap_or_else(|| panic!("{program} was started with {needle}: {log}"))
        .to_owned()
}

/// What was typed into recorder terminals.
fn typed() -> String {
    std::fs::read_to_string(harness().1.join("typed")).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------------

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-ai-handtests-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    root.canonicalize().unwrap()
}

/// A window on `root` with a settings file of its own (the user's settings are never read or
/// written), starting with `settings`.
fn window(
    root: &Path,
    settings: ferris_idea_core::IdeSettings,
    cx: &mut TestAppContext,
) -> (Bench, &'static mut VisualTestContext) {
    harness();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let folder = root.join(".test-settings");
    std::fs::create_dir_all(&folder).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(folder.join("settings.toml")));
        cx.set_global(ThemeFile(folder.join("theme")));
        cx.set_global(CurrentIdeSettings(settings));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

/// Settings with the assistant on, talking to `endpoint`.
fn on(endpoint: &str) -> ferris_idea_core::IdeSettings {
    let mut s = ferris_idea_core::IdeSettings::default();
    s.ai.enabled = true;
    s.ai.endpoint = endpoint.to_owned();
    s.ai.model = "fake-small".to_owned();
    s
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

/// Waits up to `secs` seconds of real time until `done` holds, letting background work, timers and
/// drawing run.
fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        draw(cx);
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

/// Types `typed` into a Python file and waits until the language server's answer to the
/// completion request the typing opened has come back. The harness runs background work on the
/// test's own thread, so until that answer is back (the server may still be starting, slowly on a
/// loaded machine) the inline suggestion cannot be asked for: waiting for the answer keeps that
/// time out of the short wait for the suggestion.
fn type_and_await_python_completion(bench: &Bench, cx: &mut VisualTestContext, typed: &str) {
    let before = bench.read_with(cx, |b, _| b.completions_answered());
    type_text(cx, typed);
    until(
        cx,
        PYTHON_SERVER_SECS,
        "the Python language server answers the completion request",
        |cx| bench.read_with(cx, |b, _| b.completions_answered()) > before,
    );
}

/// How long a Python language server may take to start and answer its first request (pyright's
/// start on a loaded machine; the request's own limit is 30 s, SPEC-0072).
const PYTHON_SERVER_SECS: u64 = 120;

/// Lets `ms` milliseconds of timers pass (nothing is expected to happen).
fn pause(cx: &mut VisualTestContext, ms: u64) {
    for _ in 0..ms / 50 {
        draw(cx);
        cx.executor().advance_clock(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(5));
    }
    draw(cx);
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

fn type_text(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    draw(cx);
}

/// The middle of what is shown of the control with test id `name` in the semantic tree of the
/// frame just drawn (SPEC-0319).
fn spot(cx: &mut VisualTestContext, name: &str) -> gpui::Point<gpui::Pixels> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("a tree is built");
        let r = tree
            .iter()
            .filter(|n| n.test_id() == Some(name))
            .find_map(|n| n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.))
            .unwrap_or_else(|| panic!("{name} is drawn and in view"));
        point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
    })
}

fn click(cx: &mut VisualTestContext, name: &str) {
    let at = spot(cx, name);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

fn right_click(cx: &mut VisualTestContext, name: &str) {
    let at = spot(cx, name);
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw(cx);
}

fn hover(cx: &mut VisualTestContext, name: &str) {
    let at = spot(cx, name);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    draw(cx);
    pause(cx, 300);
}

/// One keystroke without letting background work run, so the next one arrives while it waits.
fn key_now(cx: &mut VisualTestContext, keystroke: &str) {
    let keystroke = Keystroke::parse(keystroke).unwrap();
    cx.update(|window, cx| {
        window.dispatch_keystroke(keystroke, cx);
    });
}

/// A menu item of the menu bar (the native menu sends the same action).
fn menu(bench: &Bench, cx: &mut VisualTestContext, id: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    draw(cx);
}

/// Shows the AI chat (Tools › AI Assistant › AI Chat when hidden), clicks into its question
/// field, types `question` and presses Enter.
fn ask(bench: &Bench, cx: &mut VisualTestContext, question: &str) {
    if bench.read_with(cx, |b, _| b.ai_chat_view().is_none()) {
        menu(bench, cx, "FerrisIDEA.AiChat");
    }
    click(cx, "ai-question");
    type_text(cx, question);
    keys(cx, "enter");
}

fn chat(bench: &Bench, cx: &mut VisualTestContext) -> Entity<AiChatView> {
    bench
        .read_with(cx, |b, _| b.ai_chat_view())
        .expect("the AI chat is shown")
}

fn texts(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.ai_chat_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

fn chips(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    chat(bench, cx).read_with(cx, |v, _| v.chips())
}

fn busy(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, cx| {
        b.ai_chat_view().is_some_and(|v| v.read(cx).busy())
    })
}

fn settings(bench: &Bench, cx: &mut VisualTestContext) -> Entity<SettingsView> {
    bench
        .read_with(cx, |b, _| b.settings_dialog())
        .expect("Settings is open")
}

fn note(bench: &Bench, cx: &mut VisualTestContext) -> String {
    let s = settings(bench, cx);
    s.read_with(cx, |v, _| v.note().unwrap_or_default().to_owned())
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn ghost(bench: &Bench, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .and_then(|e| e.read(cx).ghost().map(str::to_owned))
    })
}

/// Opens `file` with the caret at `line` and `column` (0-based) and the focus in it.
fn open_at(bench: &Bench, cx: &mut VisualTestContext, file: &Path, line: usize, column: usize) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        if let Some(e) = b.active_editor_entity() {
            e.update(cx, |e, cx| e.go_to(line, column, cx));
        }
        b.focus_editor(window, cx);
    });
    draw(cx);
}

/// Types into the settings field labelled `label` after a click into it, replacing its text, and
/// presses Enter.
fn fill(cx: &mut VisualTestContext, label: &str, value: &str) {
    click(cx, &format!("settings-ai-field-{label}"));
    keys(cx, "cmd-a backspace");
    type_text(cx, value);
    keys(cx, "enter");
}

/// Every file under `dir` whose bytes contain `needle`.
fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if std::fs::read(&p).is_ok_and(|b| String::from_utf8_lossy(&b).contains(needle))
            {
                out.push(p);
            }
        }
    }
    out
}

/// What MCP's `ai_assistant` reads of the chat (chips and shared tabs), as JSON.
fn chat_state(root: &Path) -> String {
    serde_json::to_string(&ferris_idea_ai::live::Live::global().chat(root)).unwrap()
}

fn answer_by_question(s: &Seen) -> String {
    if s.body.contains("And with floats?") {
        "Use f64 instead of i32.".into()
    } else if s.body.contains("Explain what this code does") {
        "It adds two numbers.".into()
    } else {
        "Fine.".into()
    }
}

// ---------------------------------------------------------------------------------------------
// Hand tests 64 and 69: settings, key, Explain Code, follow-up, errors
// ---------------------------------------------------------------------------------------------

const CALC: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";

/// Hand tests 64 (SPEC-0138) and 69 (SPEC-0143): nothing goes out while the assistant is off;
/// switched on with clicks and typing in Settings › Tools › AI Assistant, the key lands in the
/// secrets folder and not in `settings.toml`, Enter lists the account's models; the Provider row
/// cycles with Enter and fills a template's endpoint; selected code is visible as a chip before it
/// is sent; Explain Code and a follow-up question get answers; a wrong key and an unreachable
/// endpoint are named with status and message, the wrong key masked.
#[gpui::test]
fn hand_tests_64_and_69_endpoint_key_explain_and_errors(cx: &mut TestAppContext) {
    let fake = Fake::start(Some(KEY), answer_by_question);
    let root = workspace("explain", &[("src/calc.rs", CALC)]);
    let (bench, cx) = window(&root, ferris_idea_core::IdeSettings::default(), cx);
    let settings_file = root.join(".test-settings/settings.toml");
    let calc = root.join("src/calc.rs");

    // Off (the default): the chat, Explain Code and the terminal entries send and read nothing.
    open_at(&bench, cx, &calc, 0, 0);
    ask(&bench, cx, "Hello?");
    pause(cx, 300);
    assert!(
        texts(&bench, cx)
            .iter()
            .any(|t| t.starts_with("Error: the AI Assistant is off")),
        "{:?}",
        texts(&bench, cx)
    );
    keys(cx, "escape");
    menu(&bench, cx, "FerrisIDEA.AiExplainCode");
    pause(cx, 300);
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.ai_chat_view().is_none()));
    menu(&bench, cx, "FerrisIDEA.AiAttachGitDiff");
    assert!(status(&bench, cx).contains("The AI Assistant is off"));
    assert!(fake.seen().is_empty(), "nothing was sent while off");

    // Switched on in the settings, by mouse and keyboard (SPEC-0138 behaviour 1).
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Ai, window, cx));
    draw(cx);
    click(cx, "settings-ai-0");
    assert!(settings(&bench, cx).read_with(cx, |v, _| v.ide().ai.enabled));
    click(cx, "settings-ai-field-Endpoint");
    type_text(cx, &fake.endpoint);
    click(cx, "settings-ai-field-Model");
    type_text(cx, "fake-small");
    click(cx, "settings-ai-field-API key");
    type_text(cx, KEY);
    keys(cx, "enter");
    until(
        cx,
        10,
        "the key is stored and the models are listed",
        |cx| note(&bench, cx).contains("models: fake-large, fake-small"),
    );
    assert!(note(&bench, cx).contains("API key stored"));
    let account = fake
        .endpoint
        .trim_start_matches("http://")
        .trim_end_matches("/v1");
    let stored = harness().0.join("ferris-idea-ai").join(account);
    assert_eq!(
        std::fs::read_to_string(&stored).unwrap().trim_end(),
        KEY,
        "the key is in the secrets folder (the keychain in a release build)"
    );
    let saved = std::fs::read_to_string(&settings_file).unwrap();
    assert!(saved.contains(&fake.endpoint) && saved.contains("fake-small"));
    assert!(!saved.contains(KEY), "settings.toml never holds the key");
    let models = fake.seen().into_iter().find(|s| s.line.contains("/models"));
    assert_eq!(
        models.and_then(|s| s.authorization),
        Some(format!("Bearer {KEY}")),
        "the model list was asked with the stored key"
    );

    // The Provider row cycles with Enter: the three subscriptions, then the API templates, each
    // filling its endpoint (SPEC-0143, SPEC-0160 behaviour 1).
    click(cx, "settings-ai-1");
    assert_eq!(
        settings(&bench, cx).read_with(cx, |v, _| v.ide().ai.subscription.clone()),
        "claude"
    );
    keys(cx, "enter enter enter");
    assert!(note(&bench, cx).starts_with("OpenAI: endpoint https://api.openai.com/v1"));
    keys(cx, "enter");
    assert!(note(&bench, cx).starts_with("Anthropic: endpoint https://api.anthropic.com/v1"));
    keys(cx, "enter enter");
    assert!(note(&bench, cx).starts_with("Local (Ollama): endpoint http://localhost:11434/v1"));
    assert!(
        settings(&bench, cx).read_with(cx, |v, _| v.ide().ai.subscription.is_empty()),
        "a template uses the endpoint again"
    );
    // Back to the fake endpoint; nothing went to a template's endpoint (none was applied).
    fill(cx, "Endpoint", &fake.endpoint);
    until(cx, 10, "the fake endpoint is applied again", |cx| {
        note(&bench, cx).contains("models: fake-large, fake-small")
    });
    // Escape leaves the field, the next one closes Settings.
    keys(cx, "escape escape");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog().is_none()));

    // A selection is attached visibly before anything is sent (SPEC-0138 behaviour 3).
    let before = fake.chats().len();
    open_at(&bench, cx, &calc, 0, 0);
    keys(cx, "cmd-a");
    menu(&bench, cx, "FerrisIDEA.AiChat");
    assert_eq!(chips(&bench, cx), ["src/calc.rs:1-4 · 3 lines"]);
    pause(cx, 200);
    assert_eq!(fake.chats().len(), before, "attaching sends nothing");
    keys(cx, "escape");
    assert!(
        chips(&bench, cx).is_empty(),
        "Escape in the empty field removes the chip"
    );
    keys(cx, "escape");

    // Explain Code: the selection goes with the question, the answer appears.
    open_at(&bench, cx, &calc, 0, 0);
    keys(cx, "cmd-a");
    menu(&bench, cx, "FerrisIDEA.AiExplainCode");
    until(cx, 10, "Explain Code is answered", |cx| {
        !busy(&bench, cx)
            && texts(&bench, cx)
                .iter()
                .any(|t| t == "AI: It adds two numbers.")
    });
    let t = texts(&bench, cx);
    assert!(
        t.iter()
            .any(|l| l.starts_with("You: src/calc.rs:1-4:\n```rust\nfn add(a: i32, b: i32)")),
        "{t:?}"
    );
    // A follow-up question typed into the chat carries the conversation.
    click(cx, "ai-question");
    type_text(cx, "And with floats?");
    keys(cx, "enter");
    until(cx, 10, "the follow-up is answered", |cx| {
        !busy(&bench, cx)
            && texts(&bench, cx)
                .iter()
                .any(|t| t == "AI: Use f64 instead of i32.")
    });
    let chats = fake.chats();
    let last = chats.last().unwrap();
    assert!(last.body.contains("It adds two numbers.") && last.body.contains("fn add("));
    assert!(last.body.contains("\"model\":\"fake-small\""));
    for s in fake.seen() {
        assert_eq!(s.authorization, Some(format!("Bearer {KEY}")), "{}", s.line);
        assert!(
            !s.body.contains(KEY) && !s.headers.contains(KEY),
            "{}",
            s.line
        );
        assert!(
            s.line.contains(" /v1/"),
            "only the endpoint was asked: {}",
            s.line
        );
    }

    // A wrong key: the settings and the chat name status and message, never the key.
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Ai, window, cx));
    draw(cx);
    fill(cx, "API key", WRONG_KEY);
    until(cx, 10, "the wrong key is refused by the endpoint", |cx| {
        note(&bench, cx).contains("401")
    });
    let n = note(&bench, cx);
    assert!(n.contains("Incorrect API key provided"), "{n}");
    assert!(!n.contains("WRONGWRONG"), "the note masks the key: {n}");
    keys(cx, "escape escape");
    ask(&bench, cx, "Still there?");
    until(cx, 10, "the chat shows the endpoint's error", |cx| {
        !busy(&bench, cx)
            && texts(&bench, cx)
                .iter()
                .any(|t| t.starts_with("Error: the endpoint answered 401"))
    });
    let t = texts(&bench, cx);
    assert!(
        t.iter().all(|l| !l.contains("WRONGWRONG")),
        "the chat masks the key: {t:?}"
    );
    assert!(
        t.iter().all(|l| l != "AI: "),
        "no empty answer is kept: {t:?}"
    );

    // An unreachable endpoint is named.
    let dead = unreachable_endpoint();
    keys(cx, "escape");
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Ai, window, cx));
    draw(cx);
    fill(cx, "Endpoint", &dead);
    keys(cx, "escape escape");
    ask(&bench, cx, "Anyone?");
    until(
        cx,
        15,
        "the chat says the endpoint cannot be reached",
        |cx| {
            !busy(&bench, cx)
                && texts(&bench, cx)
                    .iter()
                    .filter(|t| t.starts_with("Error: "))
                    .any(|t| !t.contains("401"))
        },
    );

    // The key went nowhere else: not into a file of the project or settings folder, not into
    // the chat or its state for MCP.
    for key in [KEY, WRONG_KEY] {
        assert!(
            files_containing(&root, key).is_empty(),
            "{:?}",
            files_containing(&root, key)
        );
        assert!(!chat_state(&root).contains(key));
        assert!(texts(&bench, cx).iter().all(|t| !t.contains(key)));
        assert!(!status(&bench, cx).contains(key));
    }
    let _ = std::fs::remove_file(&stored);
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------------------------
// Hand tests 83 and 88: streaming, Escape, inline completion, alternatives
// ---------------------------------------------------------------------------------------------

/// The long answer of chat questions, 200 words.
fn long_answer() -> String {
    (1..=200).map(|i| format!("word{i} ")).collect()
}

/// A long streamed answer for chat questions; for inline completion an answer per language and,
/// inside `vec![…]`, a different one per request until the list repeats.
fn answer_streaming_and_inline(s: &Seen) -> String {
    if !s.inline() {
        return long_answer();
    }
    if s.body.contains("vec![<CURSOR>") {
        if s.body.contains("0; 8") {
            "1, 2, 3".into()
        } else if s.body.contains("different completion") {
            "0; 8".into()
        } else {
            "1, 2, 3".into()
        }
    } else if s.body.contains("```rust") {
        "a + b".into()
    } else if s.body.contains("```python") {
        " - b".into()
    } else {
        "x".into()
    }
}

/// Hand test 83 (SPEC-0157): the answer is asked for as a stream and assembled from its pieces;
/// Escape while it waits stops it and keeps what came; inline completion (on in the settings)
/// shows grey text at the line's end in Rust and Python after a pause, Tab inserts it, typing on
/// or Escape drops it. Hand test 88 (SPEC-0162): typed into `vec![` with the `]` the IDE closed,
/// the suggestion stands before the `]`; ⌥] asks for another, ⌥[ goes back, ⌃⌥. and ⌃⌥, too, a
/// repeated answer is "No other suggestion", Tab inserts the one shown. Secrets around the caret
/// are masked before the request (SPEC-0157 v2).
#[gpui::test]
fn hand_tests_83_and_88_streaming_escape_and_inline_completion(cx: &mut TestAppContext) {
    let fake = Fake::start(None, answer_streaming_and_inline);
    let root = workspace(
        "inline",
        &[
            (
                "src/lib.rs",
                "pub fn add(a: i32, b: i32) -> i32 {\n    \n}\n",
            ),
            ("src/main.rs", "fn main() {\n    \n}\n"),
            (
                "calc.py",
                "def sub(a, b):\n    return \n\n\ndef neg(a):\n    return \n",
            ),
            (
                "settings.py",
                "GITHUB_TOKEN = \"ghp_FAKEFAKEFAKEFAKEFAKEFAKE0000\"\nname = \n",
            ),
        ],
    );
    let mut s = on(&fake.endpoint);
    s.ai.inline_completion = true;
    let (bench, cx) = window(&root, s, cx);

    // A streamed answer, assembled from its 200 pieces.
    ask(&bench, cx, "Tell me a long story");
    until(cx, 10, "the streamed answer is complete", |cx| {
        !busy(&bench, cx)
    });
    let t = texts(&bench, cx);
    assert_eq!(
        t.last().map(String::as_str),
        Some(format!("AI: {}", long_answer()).as_str()),
        "{t:?}"
    );
    assert!(fake.chats()[0].body.contains("\"stream\":true"));

    // Escape while the answer waits stops it; what came stays.
    type_text(cx, "Another long one");
    key_now(cx, "enter");
    assert!(busy(&bench, cx), "Waiting for the answer…");
    key_now(cx, "escape");
    until(cx, 10, "the stopped answer ends", |cx| !busy(&bench, cx));
    let t = texts(&bench, cx);
    let last = t.last().unwrap();
    assert!(last.starts_with("AI: word1"), "{t:?}");
    assert!(
        !last.contains("word200"),
        "Escape stopped the answer: {last}"
    );
    assert!(
        bench.read_with(cx, |b, _| b.ai_chat_view().is_some()),
        "the chat stays open"
    );
    keys(cx, "escape");
    assert!(
        bench.read_with(cx, |b, _| b.ai_chat_view().is_none()),
        "Escape hides it then"
    );

    // Inline completion in Rust at the line's end: grey text, Tab inserts.
    let lib = root.join("src/lib.rs");
    open_at(&bench, cx, &lib, 1, 4);
    type_text(cx, "a");
    until(cx, 10, "the Rust suggestion is shown", |cx| {
        ghost(&bench, cx).as_deref() == Some(" + b")
    });
    keys(cx, "tab");
    assert!(
        text(&bench, cx).contains("    a + b\n"),
        "{}",
        text(&bench, cx)
    );
    assert_eq!(ghost(&bench, cx), None);
    // Typing on drops a suggestion.
    keys(cx, "enter");
    type_text(cx, "a");
    until(cx, 10, "a new suggestion is shown", |cx| {
        ghost(&bench, cx).is_some()
    });
    type_text(cx, ";");
    assert_eq!(ghost(&bench, cx), None, "typing on drops it");

    // Python, where the completion list opens as the person types: Tab takes the suggestion
    // first; Escape hides suggestion and list at once (SPEC-0157 v2 behaviour 4).
    let py = root.join("calc.py");
    let popup =
        |cx: &mut VisualTestContext| bench.read_with(cx, |b, _| b.completion_items().is_some());
    open_at(&bench, cx, &py, 1, 11);
    type_and_await_python_completion(&bench, cx, "a");
    until(cx, 10, "the Python suggestion is shown", |cx| {
        ghost(&bench, cx).as_deref() == Some(" - b")
    });
    assert!(popup(cx), "the completion list is open too");
    keys(cx, "tab");
    assert!(
        text(&bench, cx).contains("return a - b\n"),
        "{}",
        text(&bench, cx)
    );
    assert!(!popup(cx) && ghost(&bench, cx).is_none());
    open_at(&bench, cx, &py, 5, 11);
    type_and_await_python_completion(&bench, cx, "a");
    until(cx, 10, "the second Python suggestion is shown", |cx| {
        ghost(&bench, cx).as_deref() == Some(" - b")
    });
    assert!(popup(cx));
    keys(cx, "escape");
    assert_eq!(ghost(&bench, cx), None, "Escape hides it");
    assert!(!popup(cx), "and the list");
    assert!(
        text(&bench, cx).ends_with("    return a\n"),
        "{}",
        text(&bench, cx)
    );
    let inline: Vec<Seen> = fake.chats().into_iter().filter(Seen::inline).collect();
    assert!(
        inline
            .iter()
            .any(|s| s.body.contains("```python") && s.body.contains("<CURSOR>"))
    );

    // Hand test 88: before the `]` the IDE closed.
    let main = root.join("src/main.rs");
    open_at(&bench, cx, &main, 1, 4);
    type_text(cx, "let v = vec![");
    assert!(
        text(&bench, cx).contains("let v = vec![]"),
        "{}",
        text(&bench, cx)
    );
    until(
        cx,
        10,
        "the suggestion inside the brackets is shown",
        |cx| ghost(&bench, cx).as_deref() == Some("1, 2, 3"),
    );
    let editor = bench.read_with(cx, |b, _| b.active_editor_entity().unwrap());
    let caret = editor.read_with(cx, |e, _| e.state.carets.primary().head);
    let line_start = text(&bench, cx).find("let v").unwrap();
    assert_eq!(
        caret,
        line_start + "let v = vec![".len(),
        "the caret is before the `]`"
    );
    keys(cx, "alt-]");
    until(cx, 10, "⌥] shows another suggestion", |cx| {
        ghost(&bench, cx).as_deref() == Some("0; 8")
    });
    keys(cx, "alt-[");
    assert_eq!(
        ghost(&bench, cx).as_deref(),
        Some("1, 2, 3"),
        "⌥[ goes back"
    );
    keys(cx, "ctrl-alt-.");
    assert_eq!(ghost(&bench, cx).as_deref(), Some("0; 8"), "⌃⌥. goes on");
    keys(cx, "ctrl-alt-.");
    until(cx, 10, "a repeated answer is no other suggestion", |cx| {
        bench.read_with(cx, |b, _| {
            b.status_note()
                .is_some_and(|s| s.contains("No other suggestion"))
        })
    });
    assert_eq!(
        ghost(&bench, cx).as_deref(),
        Some("0; 8"),
        "the shown one stays"
    );
    keys(cx, "ctrl-alt-,");
    assert_eq!(
        ghost(&bench, cx).as_deref(),
        Some("1, 2, 3"),
        "⌃⌥, goes back"
    );
    keys(cx, "alt-]");
    keys(cx, "tab");
    assert!(
        text(&bench, cx).contains("    let v = vec![0; 8]\n"),
        "Tab inserts the one shown: {}",
        text(&bench, cx)
    );

    // Secrets around the caret are masked before the request (SPEC-0157 v2 behaviour 3).
    let secret = root.join("settings.py");
    open_at(&bench, cx, &secret, 1, 7);
    type_and_await_python_completion(&bench, cx, "a");
    until(
        cx,
        10,
        "the suggestion in the settings file is shown",
        |cx| ghost(&bench, cx).is_some(),
    );
    for s in fake.seen() {
        assert!(
            !s.body.contains("FAKEFAKE"),
            "a token left the machine: {}",
            s.body
        );
    }
    assert!(
        fake.chats()
            .iter()
            .any(|s| s.body.contains("GITHUB_TOKEN = \\\"‹masked›\\\"")),
        "the request has the masked line"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------------------------
// Hand test 86: the subscriptions
// ---------------------------------------------------------------------------------------------

/// The settings in effect.
fn current(cx: &mut VisualTestContext) -> ferris_idea_core::IdeSettings {
    cx.update(|_, cx| ferris_idea_workbench::appearance::ide_settings(cx))
}

/// Hand test 86 (SPEC-0160) with stand-ins of `claude`, `codex` and `gemini`: choosing Claude
/// Code checks the sign-in ("signed in"); Sign in types the program's sign-in command into a new
/// terminal tab; a question runs the program without tools in an empty folder outside the project
/// and streams its pieces; Escape stops it; Codex answers at once; Gemini CLI gets the prompt as
/// an argument; inline completion works through Claude Code. The endpoint is never used.
#[gpui::test]
fn hand_test_86_subscriptions_run_the_programs_without_tools(cx: &mut TestAppContext) {
    let fake = Fake::start(None, |_| "endpoint".into());
    let root = workspace(
        "subscription",
        &[(
            "src/lib.rs",
            "pub fn mul(a: i32, b: i32) -> i32 {\n    \n}\n",
        )],
    );
    let mut s = on(&fake.endpoint);
    // Terminals start the recorder, so no real program is ever run by a typed command.
    s.terminal.shell = Some(harness().1.join("recorder").display().to_string());
    let (bench, cx) = window(&root, s, cx);
    let log_start = program_log().len();

    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Ai, window, cx));
    draw(cx);
    click(cx, "settings-ai-1");
    until(cx, 10, "the Claude Code sign-in is checked", |cx| {
        settings(&bench, cx).read_with(cx, |v, _| {
            v.ai_login() == Some(ferris_idea_ai::subscription::Login::SignedIn)
        })
    });
    assert_eq!(current(cx).ai.subscription, "claude");
    assert!(program_log()[log_start..].contains("== claude\n"));
    assert!(program_log()[log_start..].contains("[auth]\n[status]\n"));

    // Sign in: a new terminal tab with the sign-in command, typed for the person.
    let typed_before = typed().len();
    keys(cx, "down down enter");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog().is_none()));
    until(
        cx,
        10,
        "the sign-in command reaches the new terminal",
        |_| typed()[typed_before..].contains("claude auth login"),
    );

    // A question through Claude Code: streamed pieces, no tools, an empty folder.
    ask(&bench, cx, "Hi Claude");
    until(cx, 10, "Claude Code answers", |cx| {
        !busy(&bench, cx)
            && texts(&bench, cx)
                .iter()
                .any(|t| t == "AI: Claude answers in pieces.")
    });
    let log = program_log()[log_start..].to_owned();
    let run = started(&log, "claude", "Hi Claude");
    for arg in [
        "[-p]",
        "[--tools]\n[]",
        "[--setting-sources]\n[]",
        "[--strict-mcp-config]",
        "[--no-session-persistence]",
        "[--disable-slash-commands]",
        "[--model]\n[fake-small]",
    ] {
        assert!(run.contains(arg), "{arg} missing in {run}");
    }
    let cwd = run
        .lines()
        .find_map(|l| l.strip_prefix("cwd "))
        .unwrap()
        .to_owned();
    assert!(
        !cwd.starts_with(root.to_str().unwrap()),
        "not in the project: {cwd}"
    );
    assert!(cwd.contains("ferris-idea-ai-"), "{cwd}");
    assert!(
        !Path::new(&cwd).exists(),
        "the folder is removed afterwards"
    );
    assert!(run.contains("stdin: ") && run.contains("Hi Claude"));

    // Escape stops the program's answer and keeps its start.
    type_text(cx, "Again");
    key_now(cx, "enter");
    key_now(cx, "escape");
    until(cx, 10, "the stopped answer ends", |cx| !busy(&bench, cx));
    let t = texts(&bench, cx);
    assert_eq!(t.last().map(String::as_str), Some("AI: Claude "), "{t:?}");
    keys(cx, "escape");

    // Inline completion through Claude Code.
    let mut s = current(cx);
    s.ai.inline_completion = true;
    cx.update(|_, cx| cx.set_global(CurrentIdeSettings(s)));
    open_at(&bench, cx, &root.join("src/lib.rs"), 1, 4);
    type_text(cx, "a");
    until(cx, 10, "the suggestion comes from Claude Code", |cx| {
        ghost(&bench, cx).as_deref() == Some(" * b")
    });
    assert!(program_log()[log_start..].contains("<CURSOR>"));
    keys(cx, "escape");

    // Codex: the answer whole.
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Ai, window, cx));
    draw(cx);
    click(cx, "settings-ai-1");
    assert_eq!(current(cx).ai.subscription, "codex");
    keys(cx, "escape");
    ask(&bench, cx, "Hi Codex");
    until(cx, 10, "Codex answers", |cx| {
        !busy(&bench, cx)
            && texts(&bench, cx)
                .iter()
                .any(|t| t == "AI: Codex answers at once.")
    });
    let log = program_log()[log_start..].to_owned();
    let run = started(&log, "codex", "Hi Codex");
    for arg in ["[exec]", "[--sandbox]\n[read-only]", "[--ephemeral]", "[-]"] {
        assert!(run.contains(arg), "{arg} missing in {run}");
    }
    keys(cx, "escape");

    // Gemini CLI: the prompt as an argument, read-only approval mode.
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Ai, window, cx));
    draw(cx);
    click(cx, "settings-ai-1");
    assert_eq!(current(cx).ai.subscription, "gemini");
    keys(cx, "escape");
    ask(&bench, cx, "Hi Gemini");
    until(cx, 10, "Gemini CLI answers", |cx| {
        !busy(&bench, cx) && texts(&bench, cx).iter().any(|t| t == "AI: Gemini answers.")
    });
    let log = program_log()[log_start..].to_owned();
    let run = started(&log, "gemini", "Hi Gemini");
    assert!(run.contains("[--approval-mode]\n[plan]"), "{run}");
    assert!(
        run.contains("Hi Gemini"),
        "the prompt is an argument: {run}"
    );
    assert!(
        fake.seen().is_empty(),
        "the endpoint is never used with a subscription"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 86, Terminal mode (SPEC-0160 v2 behaviours 6 and 7) with the stand-in `claude`: the
/// Claude plan row switches Chat (the default) to Terminal; then a question with Claude chosen and
/// no Anthropic API template sends nothing — Claude Code is not run, the endpoint beside it is not
/// asked — and the chat says why with two actions: "Open Claude Code in the Terminal" types
/// Claude Code into a new terminal tab of the project (never run by the assistant), "Use the
/// Anthropic API…" opens the settings on the Anthropic template. Sign in opens Claude Code too.
#[gpui::test]
fn hand_test_86_claude_plan_in_the_terminal_sends_nothing_from_the_chat(cx: &mut TestAppContext) {
    let fake = Fake::start(None, |_| "endpoint".into());
    let root = workspace("claude-terminal", &[("src/lib.rs", "pub fn a() {}\n")]);
    let mut s = on(&fake.endpoint);
    s.ai.subscription = "claude".into();
    // Terminals start the recorder, so no real program is ever run by a typed command.
    s.terminal.shell = Some(harness().1.join("recorder").display().to_string());
    let (bench, cx) = window(&root, s, cx);
    let log_start = program_log().len();
    assert_eq!(
        current(cx).ai.claude_plan_mode,
        ferris_idea_core::ClaudePlanMode::Chat,
        "Chat is the default"
    );

    // The Claude plan row switches to Terminal.
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Ai, window, cx));
    draw(cx);
    assert_eq!(node(cx, "settings-ai-7").1, "Chat");
    click(cx, "settings-ai-7");
    assert_eq!(node(cx, "settings-ai-7").1, "Terminal");
    assert_eq!(
        current(cx).ai.claude_plan_mode,
        ferris_idea_core::ClaudePlanMode::Terminal
    );
    // Sign in opens Claude Code in a new terminal tab.
    let typed_before = typed().len();
    click(cx, "settings-ai-3");
    until(cx, 10, "Claude Code is typed into a new terminal", |_| {
        typed()[typed_before..].contains("/claude'")
    });
    assert!(bench.read_with(cx, |b, _| b.settings_dialog().is_none()));

    // A question sends nothing and says why.
    ask(&bench, cx, "Hi Terminal Claude");
    until(
        cx,
        10,
        "the chat says Claude's plan runs in the Terminal",
        |cx| {
            texts(&bench, cx).iter().any(|t| {
                t.starts_with("Error: Claude with your Claude plan is set to run in the Terminal")
            })
        },
    );
    let t = texts(&bench, cx);
    let error = t.iter().find(|t| t.starts_with("Error: ")).unwrap();
    for part in ["Claude Code", "Anthropic API key", "Nothing was sent"] {
        assert!(error.contains(part), "{part}: {error}");
    }
    assert!(!t.iter().any(|t| t.starts_with("AI:")), "no answer: {t:?}");
    assert!(!busy(&bench, cx));
    assert!(fake.seen().is_empty(), "no silent fallback to the endpoint");
    assert!(
        !program_log()[log_start..].contains("Hi Terminal Claude"),
        "Claude Code is never run for an answer"
    );

    // Way on 1: Claude Code in a new terminal tab of the project.
    let typed_before = typed().len();
    click(cx, "ai-plan-terminal");
    until(cx, 10, "Claude Code is typed into a new terminal", |_| {
        typed()[typed_before..].contains("/claude'")
    });

    // Way on 2: Settings on the Anthropic API template; the plan's mode stays.
    ask(&bench, cx, " again");
    until(cx, 10, "the message is shown again", |cx| {
        drawn(cx, "ai-plan-api")
    });
    click(cx, "ai-plan-api");
    until(cx, 10, "Settings open on the Anthropic template", |cx| {
        bench.read_with(cx, |b, _| b.settings_dialog().is_some())
            && note(&bench, cx).starts_with("Anthropic: endpoint https://api.anthropic.com/v1")
    });
    assert_eq!(
        current(cx).ai.claude_plan_mode,
        ferris_idea_core::ClaudePlanMode::Terminal
    );
    keys(cx, "escape");
    if bench.read_with(cx, |b, _| b.settings_dialog().is_some()) {
        keys(cx, "escape");
    }
    assert!(fake.seen().is_empty());
    assert!(!program_log()[log_start..].contains("Hi Terminal Claude"));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------------------------
// Hand tests 213 and 214: terminal, Run, Problems and Git diff in the chat
// ---------------------------------------------------------------------------------------------

fn answer_for_context(s: &Seen) -> String {
    let last = serde_json::from_str::<serde_json::Value>(&s.body)
        .ok()
        .and_then(|v| {
            v["messages"].as_array()?.last()?["content"]
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_default();
    if last.contains("Git-Zweige") {
        "Run this:\n```bash\ngit branch --list\n```".into()
    } else if last.contains("Why does this test fail") {
        "It divides by zero.".into()
    } else if last.contains("Explain this output") {
        "It printed a token.".into()
    } else {
        "Noted.".into()
    }
}

/// The last user message of a chat request.
fn last_question(s: &Seen) -> String {
    serde_json::from_str::<serde_json::Value>(&s.body)
        .ok()
        .and_then(|v| {
            v["messages"]
                .as_array()?
                .iter()
                .rev()
                .find(|m| m["role"] == "user")?["content"]
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn terminal_view(
    bench: &Bench,
    cx: &mut VisualTestContext,
) -> Entity<ferris_idea_workbench::TerminalView> {
    bench
        .read_with(cx, |b, _| b.terminal())
        .expect("a terminal tab is open")
}

/// Whether the shell marked `command` as finished.
fn ran(bench: &Bench, cx: &mut VisualTestContext, command: &str) -> bool {
    let t = terminal_view(bench, cx);
    t.read_with(cx, |t, _| {
        t.prompts()
            .iter()
            .any(|p| p.command.as_deref() == Some(command) && p.exit.is_some())
    })
}

/// Types `command` into the terminal with keys and presses Return; waits until the shell marks it
/// finished.
fn run_in_terminal(bench: &Bench, cx: &mut VisualTestContext, command: &str) {
    click(cx, "terminal-view");
    type_text(cx, command);
    keys(cx, "enter");
    until(cx, 20, &format!("the shell ran {command}"), |cx| {
        ran(bench, cx, command)
    });
}

/// The open context menu's entries and whether each is enabled.
fn menu_items(bench: &Bench, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    bench
        .read_with(cx, |b, _| b.context_menu_items())
        .map(|(_, items)| items)
        .unwrap_or_default()
}

/// Clicks the entry `label` of the open context menu (`Sub › Item` opens the submenu first).
fn choose(cx: &mut VisualTestContext, label: &str) {
    match label.split_once(" › ") {
        Some((sub, item)) => {
            hover(cx, &format!("menu-{sub}"));
            click(cx, &format!("menu-{item}"));
        }
        None => click(cx, &format!("menu-{label}")),
    }
}

fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

const TAP_RUN: &str = r#"[[run]]
name = "unit"
kind = "test"
report = "tap"
command = ["sh", "-c", "printf 'TAP version 13\n1..2\nok 1 - adds\nnot ok 2 - divides\n  ---\n  message: division by zero\n  ...\n'; exit 1"]
"#;

/// Hand tests 213 and 214 (SPEC-0313) with a real shell in the terminal: nothing is read from a
/// terminal before the person acts; the terminal's context menu attaches the last command as a
/// chip with source, exit code and size, the fake token masked, × removes it; Explain with AI
/// prefills the question and sends only on Enter; a failed test, the Run console, the Problems and
/// the Git diff become chips; sharing a tab marks it, sends its new output once and stops; Insert
/// into Terminal types a suggested command without running it; switched off, every AI entry is
/// disabled.
#[gpui::test]
fn hand_tests_213_and_214_terminal_run_problems_and_diff_in_the_chat(cx: &mut TestAppContext) {
    let fake = Fake::start(None, answer_for_context);
    let root = workspace(
        "context",
        &[
            ("src/calc.rs", CALC),
            ("broken.json", "{\"a\": 1,, }\n"),
            (".ferris-idea/run.toml", TAP_RUN),
            (".gitignore", ".test-settings/\n"),
        ],
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "start"]);
    std::fs::write(root.join("src/calc.rs"), CALC.replace("a + b", "a - b")).unwrap();
    let mut s = on(&fake.endpoint);
    s.terminal.shell = Some("/bin/zsh".into());
    let (bench, cx) = window(&root, s, cx);

    // A terminal with a command whose output holds a fake token.
    menu(&bench, cx, "Terminal.NewTab");
    until(cx, 20, "the shell marks its prompt", |cx| {
        terminal_view(&bench, cx).read_with(cx, |t, _| !t.prompts().is_empty())
    });
    run_in_terminal(&bench, cx, "printf 'API_TOKEN=nicht-echt\\n'");

    // Asking without an action reads no terminal.
    ask(&bench, cx, "Hello");
    until(cx, 10, "the first question is answered", |cx| {
        !busy(&bench, cx)
    });
    assert!(
        !fake.chats()[0].body.contains("printf"),
        "no terminal text was sent"
    );
    assert!(chat_state(&root).contains("\"attachments\":[]"));
    keys(cx, "escape");

    // Attach to Chat › Last Command from the terminal's context menu.
    right_click(cx, "terminal-view");
    let items = menu_items(&bench, cx);
    for entry in [
        "Explain with AI",
        "Attach to Chat › Last Command",
        "Share Tab with This Chat",
    ] {
        assert!(
            items.contains(&(entry.to_owned(), true)),
            "{entry}: {items:?}"
        );
    }
    choose(cx, "Attach to Chat › Last Command");
    let c = chips(&bench, cx);
    assert_eq!(c.len(), 1, "{c:?}");
    assert!(
        c[0].starts_with("Terminal \"Local\" · last command · printf 'API_TOKEN=‹masked›"),
        "{c:?}"
    );
    assert!(
        c[0].contains("· exit 0 ·") && c[0].contains(" masked"),
        "{c:?}"
    );
    assert!(!c[0].contains("nicht-echt"));
    let state = chat_state(&root);
    assert!(
        state.contains("terminal_command") && !state.contains("nicht-echt"),
        "{state}"
    );
    click(cx, "ai-chip-remove-0");
    assert!(chips(&bench, cx).is_empty(), "× removes the chip");

    // Explain with AI: chip and question, sent only with Enter, the token masked.
    let sent = fake.chats().len();
    right_click(cx, "terminal-view");
    choose(cx, "Explain with AI");
    let v = chat(&bench, cx);
    let question = v.read_with(cx, |v, cx| v.question(cx));
    assert_eq!(question, ferris_idea_ai::context::EXPLAIN_OUTPUT);
    assert_eq!(chips(&bench, cx).len(), 1);
    pause(cx, 200);
    assert_eq!(fake.chats().len(), sent, "nothing is sent before Enter");
    keys(cx, "enter");
    until(cx, 10, "the explanation arrives", |cx| {
        texts(&bench, cx)
            .iter()
            .any(|t| t == "AI: It printed a token.")
    });
    let body = &fake.chats()[sent].body;
    assert!(
        body.contains("API_TOKEN=‹masked›") && !body.contains("nicht-echt"),
        "{body}"
    );
    assert!(body.contains("[exit code 0]"), "{body}");

    // A failed test and the Run console.
    menu(&bench, cx, "ChooseRunConfiguration");
    type_text(cx, "unit");
    keys(cx, "enter");
    until(cx, 20, "the test run ends with a failed test", |cx| {
        bench.read_with(cx, |b, cx| {
            b.run_view()
                .is_some_and(|r| r.read(cx).selected_failed_test().is_some())
        })
    });
    right_click(cx, "run-panel");
    let items = menu_items(&bench, cx);
    assert!(
        items.contains(&("Explain Failure with AI".to_owned(), true)),
        "{items:?}"
    );
    choose(cx, "Explain Failure with AI");
    let c = chips(&bench, cx);
    assert!(
        c.iter()
            .any(|c| c.starts_with("Test ") && c.contains("divides")),
        "{c:?}"
    );
    let v = chat(&bench, cx);
    assert_eq!(
        v.read_with(cx, |v, cx| v.question(cx)),
        ferris_idea_ai::context::EXPLAIN_TEST
    );
    keys(cx, "enter");
    until(cx, 10, "the failure is explained", |cx| {
        texts(&bench, cx)
            .iter()
            .any(|t| t == "AI: It divides by zero.")
    });
    assert!(last_question(fake.chats().last().unwrap()).contains("division by zero"));
    right_click(cx, "run-panel");
    choose(cx, "Attach Output to Chat");
    let c = chips(&bench, cx);
    assert!(c.iter().any(|c| c.starts_with("Run \"unit\" · ")), "{c:?}");

    // Problems and the Git diff.
    open_at(&bench, cx, &root.join("broken.json"), 0, 0);
    until(cx, 10, "the JSON file has a problem", |cx| {
        bench.read_with(cx, |b, cx| !b.problems_list(cx).is_empty())
    });
    menu(&bench, cx, "FerrisIDEA.AiAttachProblems");
    let c = chips(&bench, cx);
    assert!(
        c.iter().any(|c| c.starts_with("Problems broken.json")),
        "{c:?}"
    );
    menu(&bench, cx, "FerrisIDEA.AiAttachGitDiff");
    let c = chips(&bench, cx);
    // The changed file and the untracked `.ferris-idea/.gitignore` the window wrote.
    let changed = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&root)
        .output()
        .unwrap();
    let files = String::from_utf8_lossy(&changed.stdout).lines().count();
    let label = format!("Git diff · {files} files");
    assert!(c.iter().any(|c| c.starts_with(&label)), "{c:?}");
    // Escape in the empty field removes the last chips again.
    for _ in 0..c.len() {
        keys(cx, "escape");
    }
    assert!(chips(&bench, cx).is_empty());
    keys(cx, "escape");

    // Hand test 214: share the tab; its new output goes with the next question, once.
    menu(&bench, cx, "ActivateTerminalToolWindow");
    right_click(cx, "terminal-view");
    choose(cx, "Share Tab with This Chat");
    assert_eq!(
        bench.read_with(cx, |b, cx| b.shared_terminal_tabs(cx)),
        ["Local"]
    );
    let v = chat(&bench, cx);
    assert_eq!(v.read_with(cx, |v, cx| v.shared_names(cx)), ["Local"]);
    run_in_terminal(&bench, cx, "ls -la");
    ask(&bench, cx, "Was ist der größte Eintrag?");
    until(cx, 10, "the shared question is answered", |cx| {
        !busy(&bench, cx)
    });
    let q = last_question(fake.chats().last().unwrap());
    assert!(q.contains("(shared)") && q.contains("ls -la"), "{q}");
    ask(&bench, cx, "Und noch einmal?");
    until(cx, 10, "the second question is answered", |cx| {
        !busy(&bench, cx)
    });
    let q = last_question(fake.chats().last().unwrap());
    assert!(
        !q.contains("(shared)"),
        "an unchanged tab is not sent again: {q}"
    );
    right_click(cx, "terminal-view");
    choose(cx, "Stop Sharing with Chat");
    assert!(
        bench
            .read_with(cx, |b, cx| b.shared_terminal_tabs(cx))
            .is_empty()
    );

    // Insert into Terminal types the suggested command and runs nothing.
    ask(&bench, cx, "Wie zeige ich die Git-Zweige an?");
    until(cx, 10, "the answer suggests a command", |cx| {
        !busy(&bench, cx)
            && chat(&bench, cx).read_with(cx, |v, _| v.commands()) == ["git branch --list"]
    });
    click(cx, "ai-insert-0");
    until(cx, 10, "the command stands at the prompt", |cx| {
        terminal_view(&bench, cx).read_with(cx, |t, _| t.text().contains("git branch --list"))
    });
    pause(cx, 1000);
    assert!(
        !ran(&bench, cx, "git branch --list"),
        "nothing ran without Enter"
    );
    assert!(status(&bench, cx).contains("press Enter to run it"));

    // Switched off, every AI entry of the terminal's menu is disabled and nothing is read.
    let mut s = current(cx);
    s.ai.enabled = false;
    cx.update(|_, cx| cx.set_global(CurrentIdeSettings(s)));
    right_click(cx, "terminal-view");
    let items = menu_items(&bench, cx);
    for (label, enabled) in &items {
        if label.contains("AI") || label.contains("Chat") {
            assert!(!enabled, "{label} is disabled while the assistant is off");
        }
    }
    keys(cx, "escape");
    let sent = fake.chats().len();
    menu(&bench, cx, "FerrisIDEA.AiExplainTerminal");
    assert!(status(&bench, cx).contains("The AI Assistant is off"));
    pause(cx, 200);
    assert_eq!(fake.chats().len(), sent);
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------------------------
// The chat drawn anew (OBS-0063 A1–A6, SPEC-0138 v4)
// ---------------------------------------------------------------------------------------------

const STATS: &str = "def mean(xs):\n    return sum(xs) / len(xs)\n";

/// The fixed code of the fake answer.
const FIXED: &str =
    "def mean(xs):\n    if not xs:\n        return None\n    return sum(xs) / len(xs)";

/// An answer as a real model writes it: a heading, inline code, bold, a link, a numbered list
/// with a nested item, a table, a rule, a Python block and a shell block.
fn markdown_answer(_: &Seen) -> String {
    format!(
        "## Two bugs\n\nThe function `mean` divides by **zero** for an empty list, see \
[the docs](https://docs.python.org/3/library/statistics.html).\n\nCalling `mean([])` raises \
`ZeroDivisionError`; for `[3, 1, 4, 1, 5, 9]` the `median()` should be `3.5`, not `4`.\n\n\
1. Check the length\n2. Return `None`\n   - when the list is empty\n3. `statistics.median()` \
averages the `two middle elements`\n\n| Function | Bug |\n|---|---|\n| `mean` | empty \
list |\n| `median` | even count |\n\n---\n\n```python\n{FIXED}\n```\n\nRun the tests:\n\n\
```bash\npython -m pytest\n```\n\nA long line:\n\n```python\n{LONG_LINE}\n```\n"
    )
}

/// A line far wider than the chat (C3).
const LONG_LINE: &str = "print(\"the mean of the numbers is\", mean([3, 1, 4, 1, 5, 9, 2, 6, 5, \
3, 5, 8, 9, 7, 9, 3, 2, 3, 8, 4, 6, 2, 6, 4, 3, 3, 8, 3, 2, 7, 9, 5]), \"and the median is\", \
median([3, 1, 4, 1, 5, 9]))";

/// Painted bounds (x, y, w, h) and name of every node with `role` (and test id `id` when given).
fn boxes(
    cx: &mut VisualTestContext,
    role: semantics::Role,
    id: Option<&str>,
) -> Vec<(String, (f32, f32, f32, f32))> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("a tree is built");
        tree.iter()
            .filter(|n| n.role() == role && id.is_none_or(|id| n.test_id().unwrap_or("") == id))
            .filter_map(|n| {
                // Unclipped: the answer is taller than the chat.
                let r = n.bounds();
                (r.w > 0. && r.h > 0.).then(|| {
                    (
                        n.name().unwrap_or_default().to_owned(),
                        (r.x, r.y, r.w, r.h),
                    )
                })
            })
            .collect()
    })
}

/// Turns the wheel over the conversation by `dy` pixels (positive: towards its top).
fn scroll_chat(cx: &mut VisualTestContext, dy: f32) {
    let field = spot(cx, "ai-question");
    let at = point(field.x, field.y - px(200.));
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(dy))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    draw(cx);
}

/// Turns the wheel down over the conversation until the control `name` is in view.
fn scroll_to(cx: &mut VisualTestContext, name: &str) {
    for _ in 0..40 {
        let shown = cx.update(|window, _| {
            semantics::tree(semantics::window_id(window)).is_some_and(|t| {
                t.iter().any(|n| {
                    n.test_id() == Some(name)
                        && n.visible_bounds.is_some_and(|r| r.w > 0. && r.h > 0.)
                })
            })
        });
        if shown {
            return;
        }
        scroll_chat(cx, -120.);
    }
    panic!("{name} came into view");
}

fn overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> f32 {
    let w = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
    let h = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
    if w > 0. && h > 0. { w * h } else { 0. }
}

/// The nodes of the frame just drawn with `role`, as (name, value, test id).
fn nodes(cx: &mut VisualTestContext, role: semantics::Role) -> Vec<(String, String, String)> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("a tree is built");
        tree.iter()
            .filter(|n| n.role() == role)
            .map(|n| {
                (
                    n.name().unwrap_or_default().to_owned(),
                    n.raw_value().unwrap_or_default().to_owned(),
                    n.test_id().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    })
}

/// The node with test id `name`: (name, value, checked).
fn node(cx: &mut VisualTestContext, name: &str) -> (String, String, Option<bool>) {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("a tree is built");
        let n = tree
            .iter()
            .find(|n| n.test_id() == Some(name))
            .unwrap_or_else(|| panic!("{name} is drawn"));
        (
            n.name().unwrap_or_default().to_owned(),
            n.raw_value().unwrap_or_default().to_owned(),
            n.checked(),
        )
    })
}

fn drawn(cx: &mut VisualTestContext, name: &str) -> bool {
    draw(cx);
    cx.update(|window, _| {
        semantics::tree(semantics::window_id(window))
            .is_some_and(|t| t.iter().any(|n| n.test_id() == Some(name)))
    })
}

fn answered(bench: &Bench, cx: &mut VisualTestContext, fake: &Fake, chats: usize) {
    until(cx, 20, "the answer arrived", |cx| {
        fake.chats().len() > chats && !busy(bench, cx)
    });
}

fn undo(bench: &Bench, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, _, cx| {
        if let Some(e) = b.active_editor_entity() {
            e.update(cx, |e, cx| {
                e.run_action("$Undo", cx);
            });
        }
    });
    draw(cx);
}

/// OBS-0063 A1–A6 (SPEC-0138 v4 AK-6 to AK-9): an answer with Markdown is drawn as headings,
/// list items, a table, inline code and a link, its code block in the editors' colours with Copy,
/// Insert at Caret, Replace Selection and Apply — Apply shows the diff first and changes the file
/// in one undo step; the open file is offered as a chip that goes only when clicked; "+" attaches;
/// the field takes several lines; a link opens only after confirmation; the header names the
/// model with the endpoint in the tooltip; Explain Code is available with the focus in the chat.
#[gpui::test]
fn chat_draws_markdown_code_actions_and_offers_the_file(cx: &mut TestAppContext) {
    let fake = Fake::start(None, markdown_answer);
    let root = workspace("redesign", &[("stats.py", STATS)]);
    let (bench, cx) = window(&root, on(&fake.endpoint), cx);
    let stats = root.join("stats.py");
    open_at(&bench, cx, &stats, 0, 0);

    // A3: the open file is offered, not sent; the chip starts as suggested.
    menu(&bench, cx, "FerrisIDEA.AiChat");
    assert!(
        chips(&bench, cx).is_empty(),
        "nothing attached without a selection"
    );
    draw(cx);
    assert_eq!(
        chat(&bench, cx).read_with(cx, |v, _| v.context_chip()),
        Some(("stats.py:1-3".to_owned(), false))
    );
    assert_eq!(node(cx, "ai-context").2, Some(false));

    // A4: the header names the model; the endpoint is in the tooltip only.
    assert_eq!(node(cx, "ai-model").0, "fake-small");
    assert!(drawn(cx, "ai-new"));

    // A5: ⇧↩ makes a second line, nothing is sent; ↩ sends.
    click(cx, "ai-question");
    type_text(cx, "What is wrong");
    keys(cx, "shift-enter");
    type_text(cx, "with mean?");
    assert_eq!(
        chat(&bench, cx).read_with(cx, |v, cx| v.question(cx)),
        "What is wrong\nwith mean?"
    );
    assert!(fake.chats().is_empty());
    assert!(drawn(cx, "ai-send"));
    keys(cx, "enter");
    answered(&bench, cx, &fake, 0);
    let first = fake.chats()[0].body.clone();
    assert!(first.contains("What is wrong\\nwith mean?"), "{first}");
    assert!(
        !first.contains("sum(xs)"),
        "the offered file is not sent unasked: {first}"
    );

    // A1: the Markdown is drawn, not shown as its marks.
    let headings = nodes(cx, semantics::Role::Heading);
    assert!(headings.iter().any(|h| h.0 == "Two bugs"), "{headings:?}");
    let items: Vec<String> = nodes(cx, semantics::Role::ListItem)
        .into_iter()
        .map(|i| i.0)
        .collect();
    for item in [
        "1. Check the length",
        "2. Return None",
        "• when the list is empty",
    ] {
        assert!(items.iter().any(|i| i == item), "{item} in {items:?}");
    }
    let (table, size, _) = node(cx, "ai-table-0");
    assert_eq!(
        (table.as_str(), size.as_str()),
        ("Function | Bug", "2 columns, 2 rows")
    );
    let cells: Vec<String> = nodes(cx, semantics::Role::Cell)
        .into_iter()
        .map(|c| c.0)
        .collect();
    assert!(
        cells.contains(&"median".to_owned()) && cells.contains(&"even count".to_owned()),
        "{cells:?}"
    );
    let paragraphs: Vec<String> = nodes(cx, semantics::Role::Paragraph)
        .into_iter()
        .map(|p| p.0)
        .collect();
    assert!(
        paragraphs
            .iter()
            .any(|p| p.starts_with("The function mean divides by zero")),
        "no backticks or stars: {paragraphs:?}"
    );
    assert!(
        !paragraphs
            .iter()
            .any(|p| p.contains("**") || p.contains('`') || p.contains("##")),
        "{paragraphs:?}"
    );
    assert!(
        !nodes(cx, semantics::Role::Splitter).is_empty(),
        "the rule is drawn"
    );

    // C3: a code line wider than the chat shows that it goes on (a fade at the right edge and a
    // bar under the lines); short lines show nothing of the kind.
    assert_eq!(node(cx, "ai-code-2").1, LONG_LINE);
    assert!(
        drawn(cx, "ai-code-more-2"),
        "the long line shows it goes on"
    );
    assert!(
        !drawn(cx, "ai-code-more-1"),
        "the short shell block shows nothing"
    );

    // C1 (window check of 2026-11-15): an inline code chip is one unit in the line, as high as
    // a line of text and as wide as its text, and covers no other word.
    scroll_chat(cx, 20_000.);
    let inline: Vec<_> = boxes(cx, semantics::Role::Code, Some(""));
    let words: Vec<_> = boxes(cx, semantics::Role::TextRun, None);
    let mut heights: Vec<f32> = words
        .iter()
        .filter(|(n, _)| n.chars().all(char::is_alphabetic))
        .map(|(_, b)| b.3)
        .collect();
    heights.sort_by(f32::total_cmp);
    let line = heights[heights.len() / 2];
    for name in [
        "mean([])",
        "ZeroDivisionError",
        "[3, 1, 4, 1, 5, 9]",
        "median()",
        "statistics.median()",
        "two middle elements",
    ] {
        let (_, chip) = inline
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("the chip {name} is drawn: {inline:?}"));
        assert!(
            chip.3 <= line * 1.3,
            "{name} is one line high ({chip:?}, a line is {line})"
        );
        assert!(
            chip.2 >= name.chars().count() as f32 * 5.,
            "{name} is as wide as its text: {chip:?}"
        );
        for (word, b) in &words {
            // The word the chip belongs to holds it.
            if word.contains(name) {
                continue;
            }
            assert!(
                overlap(*chip, *b) < 1.,
                "{name} {chip:?} covers the word {word:?} {b:?}"
            );
        }
    }
    for (i, (a, x)) in inline.iter().enumerate() {
        for (b, y) in &inline[i + 1..] {
            assert!(overlap(*x, *y) < 1., "{a} {x:?} and {b} {y:?} overlap");
        }
    }

    // A2: the code blocks with their language and actions.
    assert_eq!(node(cx, "ai-code-0").0, "python");
    assert_eq!(node(cx, "ai-code-0").1, FIXED);
    assert_eq!(node(cx, "ai-code-1").0, "bash");
    assert!(
        drawn(cx, "ai-insert-0"),
        "the shell block keeps Insert into Terminal"
    );
    click(cx, "ai-code-copy-0");
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(copied.as_deref(), Some(FIXED));

    // Insert at Caret: at the caret, one undo step.
    open_at(&bench, cx, &stats, 2, 0);
    click(cx, "ai-code-insert-0");
    assert_eq!(text(&bench, cx), format!("{STATS}{FIXED}"));
    undo(&bench, cx);
    assert_eq!(text(&bench, cx), STATS, "one Undo takes the insertion back");

    // Replace Selection needs a selection.
    open_at(&bench, cx, &stats, 0, 0);
    click(cx, "ai-code-replace-0");
    assert!(
        texts(&bench, cx)
            .iter()
            .any(|t| t == "Error: Select the code to replace in the editor first"),
        "{:?}",
        texts(&bench, cx)
    );
    assert_eq!(text(&bench, cx), STATS);

    // Apply: the diff first, nothing changed; then the change in one undo step.
    open_at(&bench, cx, &stats, 0, 0);
    click(cx, "ai-code-apply-0");
    let preview = chat(&bench, cx).read_with(cx, |v, _| v.apply_preview().cloned());
    let preview = preview.expect("Apply shows a preview");
    assert_eq!(preview.label, "stats.py");
    assert_eq!(text(&bench, cx), STATS, "the preview changes nothing");
    let (what, counts, _) = node(cx, "ai-apply-preview");
    assert_eq!(
        (what.as_str(), counts.as_str()),
        ("Apply to stats.py", "+2 −0")
    );
    scroll_to(cx, "ai-apply-confirm");
    click(cx, "ai-apply-confirm");
    assert_eq!(text(&bench, cx), format!("{FIXED}\n"));
    undo(&bench, cx);
    assert_eq!(
        text(&bench, cx),
        STATS,
        "one Undo takes the whole change back"
    );

    // A link opens only after confirmation.
    scroll_chat(cx, 20_000.);
    click(cx, "ai-link-0");
    assert_eq!(
        chat(&bench, cx).read_with(cx, |v, _| v.pending_link().map(str::to_owned)),
        Some("https://docs.python.org/3/library/statistics.html".to_owned())
    );
    assert!(bench.read_with(cx, |b, _| b.opened_urls().is_empty()));
    click(cx, "ai-link-open");
    assert_eq!(
        bench.read_with(cx, |b, _| b.opened_urls().to_vec()),
        ["https://docs.python.org/3/library/statistics.html"]
    );

    // A3: one click turns the offered file on; it goes with the next question, then is
    // offered again.
    click(cx, "ai-context");
    assert_eq!(node(cx, "ai-context").2, Some(true));
    click(cx, "ai-question");
    type_text(cx, "And now?");
    keys(cx, "enter");
    answered(&bench, cx, &fake, 1);
    let second = fake.chats()[1].body.clone();
    assert!(
        second.contains("stats.py:1-3") && second.contains("return sum(xs) / len(xs)"),
        "{second}"
    );
    assert_eq!(
        chat(&bench, cx).read_with(cx, |v, _| v.context_chip()),
        Some(("stats.py:1-3".to_owned(), false))
    );

    // "+" attaches the file as a chip.
    click(cx, "ai-add");
    click(cx, "ai-add-file");
    assert_eq!(chips(&bench, cx).len(), 1, "{:?}", chips(&bench, cx));
    assert!(chips(&bench, cx)[0].starts_with("stats.py:1-3"));
    click(cx, "ai-chip-remove-0");

    // A6: with the focus in the chat, Explain Code is available (the menu bar asks this) and
    // explains the open file.
    click(cx, "ai-question");
    let explain = ferris_idea_workbench::menu_bar::IdeAction {
        id: "FerrisIDEA.AiExplainCode".into(),
    };
    let available = cx.update(|window, cx| window.is_action_available(&explain, cx));
    assert!(
        available,
        "Explain Code is enabled with the focus in the chat"
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(explain.clone()), cx));
    answered(&bench, cx, &fake, 2);
    let third = fake.chats()[2].body.clone();
    assert!(
        third.contains("Explain what this code does") && third.contains("sum(xs)"),
        "{third}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
