//! Re-checks without a screen of the F-083 hand tests that need IDECraft's backend on a host
//! (`SPEC-0158`): hand tests 84 and 184 in a window of the test harness with real keys and real
//! mouse clicks, and hand test 84's agent step through the `idecraft` program over MCP. The host
//! is the system `sshd` started as the current user (`idecraft_remote::testing::TestHost`) with a
//! home folder of the test's own, so the backend uploaded there, its settings and its caches
//! never touch the user's home folder; the backend is this build's `idecraft`. The Linux part of
//! hand test 84 uses the Docker Engine's Lima VM (`IDECRAFT_TEST_DOCKER_SSH`, set by the
//! internal test environment) as a Linux host without IDECraft.
//!
//! Choosing the test host is a call (it is in no `~/.ssh/config`), as in the other F-083 window
//! tests (`crates/workbench/tests/it/f083_handtests.rs`). Every step on the host waits for the
//! window's own answer — the result or the error it shows — after the host has let a first
//! session in, not for a fixed time, so a machine under load only waits longer. What needs eyes
//! stays in the hand tests: how the window draws the rows, the terminal and the status bar, a
//! real keyboard, a second Mac and a Linux machine with IDECraft installed, and the comparison
//! with the reference's remote development.
//!
//! `SPEC-0158` v4 AK-6 (hand tests 84 and 184).

#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_remote::testing::TestHost;
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use serde_json::{Value, json};

type Bench = Entity<WorkbenchView>;

/// How long a step on a host may take before it counts as hanging. Every call of the window has
/// its own limit (10 s to connect, 30 s a command, 120 s a file; SPEC-0121 behaviour 2) and shows
/// an error when it is exceeded.
const HANG: Duration = Duration::from_secs(300);

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The test host and the folders of its sessions.
struct Host {
    host: TestHost,
    /// The test's folder (removed at the end).
    base: PathBuf,
    /// The home folder of the host's sessions.
    home: PathBuf,
    /// A folder on the host's `PATH`, empty at first.
    bin: PathBuf,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A ready test host whose sessions have `HOME` and a `PATH` folder of the test's own; `None`
/// without `sshd` or `ssh-keygen`.
fn host(tag: &str) -> Option<Host> {
    let base = std::env::temp_dir().join(format!("idecraft-f083-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("home/bin")).unwrap();
    let base = base.canonicalize().unwrap();
    let home = base.join("home");
    let bin = home.join("bin");
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display());
    let host = TestHost::start_with_env(
        &base.join("ssh"),
        &[("HOME", &home.display().to_string()), ("PATH", &path)],
    )?;
    // The ready signal: a whole session through IDECraft's own `ssh` succeeded.
    if let Err(e) = host.wait_ready(HANG) {
        panic!("the test host started but never let a session in: {e}");
    }
    Some(Host {
        host,
        base,
        home,
        bin,
    })
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A local workspace for the window (its own settings beside it).
fn workspace(base: &Path) -> PathBuf {
    let root = base.join("local");
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    root
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    idecraft_workbench::hit_targets::enable();
    idecraft_workbench::semantics::set_consumer(
        idecraft_workbench::semantics::Consumer::SelfTest,
        true,
    );
    cx.executor().allow_parking();
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    // The backend is this build's `idecraft`, not the test binary the window runs in.
    bench.update(cx, |b, _| {
        b.set_remote_backend_binary(PathBuf::from(env!("CARGO_BIN_EXE_idecraft")))
    });
    (bench, cx)
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn texts(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.remote_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

fn error(bench: &Bench, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| {
        b.remote_view()
            .and_then(|v| v.read(cx).error().map(str::to_owned))
    })
}

fn place(bench: &Bench, cx: &mut VisualTestContext) -> Option<PathBuf> {
    bench.read_with(cx, |b, cx| {
        b.remote_view()
            .and_then(|v| v.read(cx).place().map(|(_, p)| PathBuf::from(p)))
    })
}

/// The Remote Host window and the status bar, for a failure message.
fn state(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        let view = b.remote_view().map(|v| {
            let v = v.read(cx);
            format!(
                "place {:?} field {:?} busy {} error {:?} backend {:?} selected {:?} rows {:#?}",
                v.place(),
                v.field_text(cx),
                v.busy(),
                v.error(),
                v.backend(),
                v.selected_text(),
                v.texts()
            )
        });
        format!(
            "{}\nstatus {:?}\nactive {:?} at {:?}",
            view.unwrap_or_else(|| "no Remote Host window".into()),
            b.status_message(cx),
            b.active_file(),
            b.caret_position(cx)
        )
    })
}

/// Waits until `done` holds after a step on a host: on the window's own answer. An error the
/// window shows ends the wait at once with that error; a step that neither finishes nor fails
/// within [`HANG`] is a hang.
fn until(
    bench: &Bench,
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + HANG;
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        if let Some(e) = error(bench, cx) {
            panic!("{what}: the window reports {e:?}\n{}", state(bench, cx));
        }
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    let shown = state(bench, cx);
    panic!("timed out: {what}\n{shown}");
}

/// [`until`] for a step whose answer is the error the window shows.
fn until_error(bench: &Bench, cx: &mut VisualTestContext, what: &str) -> String {
    let end = Instant::now() + HANG;
    while Instant::now() < end {
        cx.run_until_parked();
        if let Some(e) = error(bench, cx) {
            return e;
        }
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    let shown = state(bench, cx);
    panic!("timed out: {what}\n{shown}");
}

/// Puts the focus into the Remote Host window's field (as a click there would), selects what it
/// holds and types `text`.
fn type_field(bench: &Bench, cx: &mut VisualTestContext, text: &str) {
    let field = bench.read_with(cx, |b, cx| {
        b.remote_view()
            .map(|v| v.read(cx).field_focus(cx))
            .expect("the Remote Host window is shown")
    });
    cx.update(|window, _| window.focus(&field));
    keys(cx, "cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Moves the selection with ↑/↓ (where the focus is) to the first row containing `part`.
fn select(bench: &Bench, cx: &mut VisualTestContext, part: &str) {
    for _ in 0..100 {
        let (rows, selected) = bench.read_with(cx, |b, cx| {
            b.remote_view()
                .map(|v| (v.read(cx).texts(), v.read(cx).selected_text()))
                .unwrap_or_default()
        });
        if selected.as_deref().is_some_and(|s| s.contains(part)) {
            return;
        }
        let want = rows
            .iter()
            .position(|t| t.contains(part))
            .unwrap_or_else(|| panic!("no row with {part:?}: {rows:#?}"));
        let at = selected
            .and_then(|s| rows.iter().position(|t| *t == s))
            .unwrap_or(0);
        keys(cx, if want > at { "down" } else { "up" });
    }
    panic!("the selection does not reach {part:?}");
}

/// Where the control `name` was last drawn: the visible bounds of the element with that test id in
/// the window's semantic tree (`SPEC-0319` behaviour 42).
fn drawn(cx: &mut VisualTestContext, name: &str) -> Option<idecraft_workbench::semantics::Rect> {
    use idecraft_workbench::semantics;
    cx.update(|window, _| {
        semantics::tree(semantics::window_id(window))?
            .iter()
            .filter(|n| n.test_id() == Some(name))
            .filter_map(|n| n.visible_bounds)
            .last()
    })
}

/// Clicks the control `name` of the window where it was last drawn, with real mouse events.
fn click(cx: &mut VisualTestContext, name: &str) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let r = drawn(cx, name).unwrap_or_else(|| {
        let shown: Vec<&str> = ["remote-terminal", "remote-hosts", "remote-backend"]
            .into_iter()
            .filter(|n| drawn(cx, n).is_some())
            .collect();
        panic!("the control {name} is drawn (drawn: {shown:?})")
    });
    cx.simulate_click(
        point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)),
        Modifiers::none(),
    );
    cx.run_until_parked();
}

/// Double-clicks the control `name` as the system reports it: a click, then a press and release
/// with the click count 2.
fn double_click(cx: &mut VisualTestContext, name: &str) {
    click(cx, name);
    let r = drawn(cx, name).expect("still drawn");
    let at = point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    cx.simulate_event(gpui::MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: gpui::MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: gpui::MouseButton::Left,
        click_count: 2,
    });
    cx.run_until_parked();
}

/// Connects the Remote Host window to `target` and waits for the home folder `home` (any home
/// folder when `None`).
fn connect(
    bench: &Bench,
    cx: &mut VisualTestContext,
    target: idecraft_remote::SshTarget,
    home: Option<&Path>,
) {
    bench.update_in(cx, |b, window, cx| {
        if b.remote_view().is_none() {
            b.toggle_remote(window, cx);
        }
        b.remote_connect(target, cx);
    });
    until(bench, cx, "the host's home folder is shown", |cx| {
        place(bench, cx).is_some_and(|p| home.is_none_or(|h| p == h))
    });
}

/// The folder typed into the field, Enter: it is shown.
fn go_to(bench: &Bench, cx: &mut VisualTestContext, folder: &Path) {
    type_field(bench, cx, &folder.display().to_string());
    keys(cx, "enter");
    until(bench, cx, "the project folder is shown", |cx| {
        place(bench, cx).is_some_and(|p| p == folder)
    });
}

/// Clicks Start Backend; returns the status bar's note on how it was installed.
fn start_backend(bench: &Bench, cx: &mut VisualTestContext, folder: &Path) -> String {
    click(cx, "remote-backend");
    let mut note = String::new();
    until(bench, cx, "the backend runs for the folder", |cx| {
        let running = bench.read_with(cx, |b, cx| {
            b.remote_view().is_some_and(|v| {
                v.read(cx)
                    .backend()
                    .is_some_and(|(name, f)| Path::new(f) == folder && name.starts_with("idecraft"))
            })
        });
        note = status(bench, cx);
        running && note.starts_with("Backend ")
    });
    note
}

/// Whether a process of `program` runs (its command line starts with it).
fn runs(program: &Path) -> bool {
    let out = Command::new("ps")
        .args(["-axww", "-o", "command="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.starts_with(&program.display().to_string()))
}

/// The backends this build uploaded below `home` (`~/.cache/idecraft/bin`).
fn uploaded(home: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(home.join(idecraft_remote::BACKEND_DIR))
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(&format!("idecraft-{VERSION}-")))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + HANG;
    while Instant::now() < end {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out: {what}");
}

// ------------------------------------------------------------------------------- hand test 84

/// Hand test 84 (`SPEC-0158` behaviours 1, 2, 4, 5) with keys: the Remote Host window on a host
/// of this Mac's platform (as a second Mac): the project folder typed into the field and Enter,
/// Start Backend clicked uploads this build and the status bar says so; `?needle` and Enter list
/// the hits with file and line; ↓ and Enter open the second hit at its line, a double click the
/// first; leaving the host stops the backend. Then an IDECraft installed on the host's `PATH` is
/// found instead of uploading, and on a Linux host without IDECraft (the Docker VM, when its
/// variables are set) the window says what to install.
#[gpui::test]
fn hand_test_84_backend_on_a_host_with_keys(cx: &mut TestAppContext) {
    let Some(h) = host("84") else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    let project = h.home.join("proj84");
    write(
        &project.join("notes.txt"),
        "first line\nneedle in the notes\n",
    );
    write(
        &project.join("src/tool.py"),
        "def helper():\n    return 1\n\n\ndef finder():\n    return 'needle here'\n",
    );
    let root = workspace(&h.base);
    let (bench, cx) = window(&root, cx);

    // A second Mac: connect, go to the project folder, Start Backend uploads this build.
    connect(&bench, cx, h.host.target.clone(), Some(&h.home));
    go_to(&bench, cx, &project);
    let note = start_backend(&bench, cx, &project);
    assert!(
        note.starts_with(&format!("Backend idecraft {VERSION} (uploaded) runs in "))
            && note.contains(&project.display().to_string())
            && note.ends_with(" tools"),
        "the status bar names the upload: {note:?}"
    );
    let copies = uploaded(&h.home);
    assert_eq!(
        copies.len(),
        1,
        "one upload in ~/.cache/idecraft/bin: {copies:?}"
    );
    assert!(runs(&copies[0]), "the uploaded backend runs on the host");

    // `?needle` and Enter: the hits with file and line.
    type_field(&bench, cx, "?needle");
    keys(cx, "enter");
    until(&bench, cx, "?needle lists the hits", |cx| {
        let t = texts(&bench, cx);
        t.iter().any(|r| r == "notes.txt:2  needle in the notes")
            && t.iter().any(|r| r == "tool.py:6  return 'needle here'")
    });
    // ↓ to the hit in tool.py, Enter: the file opens at that line.
    select(&bench, cx, "tool.py:6");
    keys(cx, "enter");
    until(&bench, cx, "Enter opens the hit at its line", |cx| {
        bench.read_with(cx, |b, cx| {
            b.active_file().is_some_and(|p| p.ends_with("tool.py"))
                && b.caret_position(cx).map(|(l, _)| l) == Some(6)
        })
    });
    // A double click on the other hit opens it at its line too.
    let row = texts(&bench, cx)
        .iter()
        .position(|r| r == "notes.txt:2  needle in the notes")
        .expect("the hits are still listed");
    double_click(cx, &format!("remote-row-{row}"));
    until(
        &bench,
        cx,
        "a double click opens the hit at its line",
        |cx| {
            bench.read_with(cx, |b, cx| {
                b.active_file().is_some_and(|p| p.ends_with("notes.txt"))
                    && b.caret_position(cx).map(|(l, _)| l) == Some(2)
            })
        },
    );

    // Leaving the host stops the backend.
    click(cx, "remote-hosts");
    until(&bench, cx, "Hosts leaves the host", |cx| {
        place(&bench, cx).is_none()
            && bench.read_with(cx, |b, _| b.remote_backend_folder().is_none())
    });
    wait_for("the backend on the host ends", || !runs(&copies[0]));

    // A host with IDECraft installed: this build on the host's PATH is found, nothing uploaded.
    for c in &copies {
        std::fs::remove_file(c).unwrap();
    }
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_idecraft"), h.bin.join("idecraft")).unwrap();
    connect(&bench, cx, h.host.target.clone(), Some(&h.home));
    go_to(&bench, cx, &project);
    let note = start_backend(&bench, cx, &project);
    assert!(
        note.starts_with(&format!(
            "Backend idecraft {VERSION} (found on the host) runs in "
        )),
        "the status bar says the backend was there: {note:?}"
    );
    assert!(uploaded(&h.home).is_empty(), "nothing was uploaded");
    type_field(&bench, cx, "?return 1");
    keys(cx, "enter");
    until(&bench, cx, "the installed backend searches", |cx| {
        texts(&bench, cx) == ["tool.py:2  return 1"]
    });
    keys(cx, "enter");
    until(&bench, cx, "Enter opens the only hit at its line", |cx| {
        bench.read_with(cx, |b, cx| {
            b.active_file().is_some_and(|p| p.ends_with("tool.py"))
                && b.caret_position(cx).map(|(l, _)| l) == Some(2)
        })
    });
    click(cx, "remote-hosts");
    until(&bench, cx, "Hosts leaves the host again", |cx| {
        place(&bench, cx).is_none()
    });

    // A Linux host without IDECraft: the window says what to install there.
    match std::env::var("IDECRAFT_TEST_DOCKER_SSH") {
        Ok(words) => {
            let mut words: Vec<String> = words.split_whitespace().map(str::to_owned).collect();
            let destination = words.pop().unwrap();
            let target = idecraft_remote::SshTarget {
                host: destination,
                port: None,
                extra: words,
                program: None,
            };
            connect(&bench, cx, target, None);
            click(cx, "remote-backend");
            let refused = until_error(&bench, cx, "Start Backend on the Linux host answers");
            assert!(
                refused.contains(": the host runs Linux ")
                    && refused.contains(&format!(
                        "this IDECraft is built for {}",
                        idecraft_remote::local_platform()
                    ))
                    && refused.contains(&format!("install IDECraft {VERSION} there")),
                "the window says what to install: {refused}"
            );
            assert!(
                bench.read_with(cx, |b, _| b.remote_backend_folder().is_none()),
                "no backend runs"
            );
        }
        Err(_) => eprintln!("84, Linux host skipped: IDECRAFT_TEST_DOCKER_SSH is not set"),
    }
}

/// The MCP server `idecraft mcp <root>` over standard input/output, as an agent runs it, with
/// the settings folder `config` and `ssh` reaching the test host.
struct Agent {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: std::io::Lines<BufReader<std::process::ChildStdout>>,
    next: u64,
}

impl Agent {
    fn start(root: &Path, config: &Path, ssh_dir: &Path) -> Self {
        let path = format!(
            "{}:{}",
            ssh_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_idecraft"))
            .arg("mcp")
            .arg(root)
            .env("IDECRAFT_CONFIG_DIR", config)
            .env("PATH", path)
            .env_remove("IDECRAFT_TRUST_PROJECTS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut agent = Self {
            child,
            input,
            output,
            next: 0,
        };
        agent.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "2"}}),
        );
        writeln!(
            agent.input,
            "{}",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .unwrap();
        agent
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let line = self.output.next().expect("an answer").unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// Calls `tool`: its JSON answer, or the error message.
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        let reply = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        if let Some(m) = reply["error"]["message"].as_str() {
            return Err(m.to_owned());
        }
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if reply["result"]["isError"] == json!(true) {
            return Err(text);
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Hand test 84, agent step (`SPEC-0158` behaviour 6): an agent of `idecraft mcp` starts the
/// backend on the host with `remote_backend_start` — the plan first, then with the user-granted
/// `ssh` and `run` rights — calls `inspect_code` there with `remote_backend_call`, which finds the
/// host file's bare `except`, and stops it with `remote_backend_stop`; the start and the call are
/// logged without the call's arguments.
#[test]
fn hand_test_84_an_agent_inspects_the_host_project() {
    let Some(h) = host("84-agent") else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    let project = h.home.join("proj84");
    write(
        &project.join("src/risky.py"),
        "def risky():\n    try:\n        return 1 / 0\n    except:\n        return None\n",
    );
    let root = workspace(&h.base);
    let config = h.base.join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("agent-permissions.toml"),
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"ssh\", \"run\"]\n",
            root.display()
        ),
    )
    .unwrap();
    // The agent's `ssh` reaches the test host with the test's client config, as an entry in
    // `~/.ssh/config` would; everything else is the system `ssh` IDECraft starts.
    let ssh_dir = h.base.join("ssh-client");
    std::fs::create_dir_all(&ssh_dir).unwrap();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("ssh"))
        .find(|p| p.is_file())
        .expect("ssh on PATH");
    let config_file = h.host.target.extra[1].clone();
    std::fs::write(
        ssh_dir.join("ssh"),
        format!(
            "#!/bin/sh\nexec '{}' -F '{config_file}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(ssh_dir.join("ssh"), std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut agent = Agent::start(&root, &config, &ssh_dir);
    let target = json!({"host": h.host.target.host, "folder": project.display().to_string()});
    let plan = agent.call("remote_backend_start", target.clone()).unwrap();
    assert_eq!(plan["dry_run"], json!(true), "{plan}");
    assert_eq!(
        plan["local_platform"],
        json!(idecraft_remote::local_platform())
    );
    assert!(
        plan["would"].as_str().is_some_and(|w| w.contains("upload")),
        "{plan}"
    );
    assert!(uploaded(&h.home).is_empty(), "the plan uploads nothing");

    let mut start = target.clone();
    start["dry_run"] = json!(false);
    let started = agent.call("remote_backend_start", start).unwrap();
    assert_eq!(started["installed"], json!("uploaded"), "{started}");
    assert_eq!(started["server"], json!(format!("idecraft {VERSION}")));
    assert!(
        started["tools"]
            .as_array()
            .is_some_and(|t| t.iter().any(|n| n == "inspect_code")),
        "{started}"
    );
    assert_eq!(uploaded(&h.home).len(), 1);

    let mut call = target.clone();
    call["tool"] = json!("inspect_code");
    call["arguments"] = json!({});
    let found = agent.call("remote_backend_call", call.clone()).unwrap();
    let findings = found["findings"].as_array().cloned().unwrap_or_default();
    assert!(
        findings.iter().any(|f| f["path"] == "src/risky.py"
            && f["line"] == 3
            && f["id"].as_str().is_some_and(|i| i.contains("bare-except"))),
        "inspect_code on the host finds the bare except: {found}"
    );

    let stopped = agent.call("remote_backend_stop", target.clone()).unwrap();
    assert_eq!(stopped, json!({"stopped": true}));
    let after = agent.call("remote_backend_call", call).unwrap_err();
    assert!(after.contains("no backend for this host"), "{after}");

    let log = std::fs::read_to_string(root.join(".idecraft/agent-log.jsonl")).unwrap_or_default();
    let lines: Vec<Value> = log
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l["tool"] == "remote_backend_start" && l["ok"] == json!(true)),
        "{log}"
    );
    assert!(
        lines.iter().any(|l| l["tool"] == "remote_backend_call"
            && l["call"] == "inspect_code"
            && l["ok"] == json!(true)
            && l.get("arguments").is_none()),
        "{log}"
    );
}

// ------------------------------------------------------------------------------ hand test 184

const CALC: &str = "/* Totals for hand test 184. */\n\nint compute_total(int n) {\n    int sum = 0;\n    for (int i = 0; i < n; i++) {\n        sum += i;\n    }\n    return sum;\n}\n\nint main(void) {\n    return compute_total(missing_count);\n}\n";

const RUN: &str = "[[run]]\nname = \"prepare\"\ncommand = [\"sh\", \"-c\", \"echo prepared-$MODE > prepared.txt\"]\nenv = { MODE = \"ht184\" }\n\n[[run]]\nname = \"app\"\ncommand = [\"sh\", \"-c\", \"cat prepared.txt; echo app-ran-in-$(basename $PWD)\"]\nbefore = [\"prepare\"]\n";

/// Hand test 184 (`SPEC-0158` v2 behaviours 7–9) with keys: Start Backend on a host with a
/// project and `.idecraft/run.toml`; `@compute` and Enter list the symbol with kind and line, and
/// Enter opens it there; the status bar reports the host-side check by its language server
/// (`clangd`, when the host has one) or why none ran; `!` and Enter list the problems, Enter
/// opens one; `>` and Enter list the run configurations, ↓ and Enter run one with its
/// before-launch step in an SSH terminal tab named after it, which ends with "Process finished
/// with exit code 0".
#[gpui::test]
fn hand_test_184_symbols_problems_and_runs_on_the_host_with_keys(cx: &mut TestAppContext) {
    let Some(h) = host("184") else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    let project = h.home.join("proj184");
    write(&project.join("src/calc.c"), CALC);
    write(&project.join(".idecraft/run.toml"), RUN);
    let root = workspace(&h.base);
    let (bench, cx) = window(&root, cx);
    connect(&bench, cx, h.host.target.clone(), Some(&h.home));
    go_to(&bench, cx, &project);
    start_backend(&bench, cx, &project);

    // `@compute` and Enter: the symbol with its kind and line; Enter opens it there.
    type_field(&bench, cx, "@compute");
    keys(cx, "enter");
    until(&bench, cx, "@compute finds the function", |cx| {
        texts(&bench, cx)
            .first()
            .is_some_and(|r| r == "calc.c:3  compute_total  function")
    });
    // The status bar, read when the host has checked the opened file.
    let mut note = String::new();
    keys(cx, "enter");
    until(
        &bench,
        cx,
        "Enter opens the symbol's line and the host checks the file",
        |cx| {
            note = status(&bench, cx);
            bench.read_with(cx, |b, cx| {
                b.active_file().is_some_and(|p| p.ends_with("calc.c"))
                    && b.caret_position(cx).map(|(l, _)| l) == Some(3)
            }) && (note.contains("problems on the host in calc.c")
                || note.starts_with("Host check of calc.c: "))
        },
    );
    if !Path::new("/usr/bin/clangd").is_file() {
        assert!(
            note.starts_with("Host check of calc.c: "),
            "without clangd the status bar says why: {note:?}"
        );
        eprintln!("184, problems skipped: the host has no clangd ({note})");
    } else {
        assert!(
            note.ends_with(" problems on the host in calc.c (! lists them)")
                && !note.starts_with("0 "),
            "clangd on the host finds the undeclared name: {note:?}"
        );
        // `!` and Enter: the problems with file, line, severity and message; Enter opens one.
        type_field(&bench, cx, "!");
        keys(cx, "enter");
        until(&bench, cx, "! lists the problems", |cx| {
            texts(&bench, cx)
                .iter()
                .any(|r| r.starts_with("calc.c:12  error ") && r.contains("missing_count"))
        });
        select(&bench, cx, "calc.c:12  error ");
        keys(cx, "enter");
        until(&bench, cx, "Enter opens the problem's line", |cx| {
            bench.read_with(cx, |b, cx| {
                b.active_file().is_some_and(|p| p.ends_with("calc.c"))
                    && b.caret_position(cx).map(|(l, _)| l) == Some(12)
            })
        });
    }

    // `>` and Enter: the run configurations; ↓ to `app`, Enter runs it with `prepare` first.
    type_field(&bench, cx, ">");
    keys(cx, "enter");
    until(&bench, cx, "> lists the host's run configurations", |cx| {
        let t = texts(&bench, cx);
        t.len() == 2 && t[0].starts_with("▶ prepare  sh -c ") && t[1].starts_with("▶ app  sh -c ")
    });
    select(&bench, cx, "▶ app");
    keys(cx, "enter");
    let history = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.terminal()
                .map(|t| t.read(cx).history_text())
                .unwrap_or_default()
        })
    };
    let end = Instant::now() + HANG;
    loop {
        cx.run_until_parked();
        let text = history(cx);
        if text.contains("Process finished with exit code") {
            assert!(
                text.lines().any(|l| l.trim() == "prepared-ht184")
                    && text.lines().any(|l| l.trim() == "app-ran-in-proj184")
                    && text.contains("Process finished with exit code 0"),
                "the chain ran on the host: {text}"
            );
            break;
        }
        assert!(
            Instant::now() < end,
            "the run never finished: {text}\n{}",
            state(&bench, cx)
        );
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    assert!(
        project.join("prepared.txt").is_file(),
        "the before-launch step ran in the host's project folder"
    );
    let tabs = bench.read_with(cx, |b, _| b.terminal_tabs());
    assert_eq!(
        tabs.last().map(String::as_str),
        Some("app"),
        "the tab is named after the configuration: {tabs:?}"
    );
    bench.update_in(cx, |b, window, cx| {
        let last = b.terminal_tabs().len().saturating_sub(1);
        b.close_terminal_tab(last, window, cx);
        if b.remote_view().is_none() {
            b.toggle_remote(window, cx);
        }
    });
    // Leaving the host ends the backend there.
    let backend = uploaded(&h.home);
    click(cx, "remote-hosts");
    until(&bench, cx, "Hosts leaves the host", |cx| {
        place(&bench, cx).is_none()
            && bench.read_with(cx, |b, _| b.remote_backend_folder().is_none())
    });
    wait_for("the backend on the host ends", || {
        !backend.iter().any(|b| runs(b))
    });
}
