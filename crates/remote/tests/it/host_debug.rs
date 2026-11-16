//! SPEC-0322: a real debug adapter on the (in-process) host, driven through the window's link:
//! breakpoints and frames in stand-in paths, a break during a session that loses nothing, js-debug
//! on its Unix socket, attaching to `debugpy --listen`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use idecraft_debugger::{DebugSession, PathMap, SourceBreakpoint, StopState};
use idecraft_remote::host_debug::{HostLink, start_host_debug};
use idecraft_remote::inprocess::{HostSwitch, InProcessProject, Setup};
use idecraft_remote::{ConnState, LiveProject, Timing};
use serde_json::json;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idc-hostdbg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn fast() -> Timing {
    Timing {
        delays: [40, 80, 160].map(Duration::from_millis).to_vec(),
        then: Duration::from_millis(200),
        reattach: Duration::from_secs(600),
        heartbeat: Duration::from_millis(200),
        heartbeat_limit: Duration::from_secs(5),
        tick: Duration::from_millis(10),
    }
}

fn open(dir: &Path) -> (LiveProject, HostSwitch) {
    let setup: Setup = Arc::new(|b| {
        idecraft_host::bind(
            b,
            idecraft_host::HostOptions {
                language_servers: false,
                poll: Duration::from_millis(200),
                shell: Some("/bin/sh".into()),
                ..idecraft_host::HostOptions::default()
            },
        );
    });
    let p = InProcessProject::open_with(
        "testhost",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_secs(600),
        None,
        Duration::ZERO,
        Some(setup),
    )
    .unwrap();
    let switch = p.switch();
    (LiveProject::start(Box::new(p), fast()), switch)
}

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

fn has_debugpy() -> bool {
    Command::new("python3")
        .args(["-c", "import debugpy"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn line_breakpoint(line: u64) -> SourceBreakpoint {
    SourceBreakpoint {
        line,
        ..Default::default()
    }
}

/// AK-2, AK-6 (over the wire): a breakpoint set in the window's stand-in path stops the program on
/// the host, frames come back in stand-in paths, stepping and variables work, and a break of the
/// session while paused loses nothing.
#[test]
fn debugpy_on_the_host_through_the_window_link_and_a_break() {
    if !has_debugpy() {
        eprintln!("debugpy not installed; skipped");
        return;
    }
    let dir = scratch("py");
    let project = dir.join("project");
    idecraft_core::trust::trust_for_process(&project);
    std::fs::write(project.join("app.py"), "a = 1\nb = a + 1\nprint(b)\n").unwrap();
    let (live, switch) = open(&dir);
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));

    let stand = PathBuf::from("/stand-in/project");
    let paths = PathMap::new(&stand, &project.to_string_lossy());
    let link = HostLink::Live(live.clone());
    let debug = start_host_debug(&link, "debugpy", json!({}), Some(paths)).unwrap();
    let script = project.join("app.py");
    let mut breakpoints = BTreeMap::new();
    breakpoints.insert(stand.join("app.py"), vec![line_breakpoint(2)]);
    let (mut session, placed) = DebugSession::launch_on(
        debug.client,
        "debugpy",
        json!({
            "type": "python", "request": "launch", "program": script,
            "cwd": project, "console": "internalConsole", "justMyCode": true,
        }),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    assert!(placed[&stand.join("app.py")][0].verified, "{placed:?}");

    let state = session.wait_state(Duration::from_secs(60), 5).unwrap();
    let StopState::Stopped {
        frames, thread_id, ..
    } = state
    else {
        panic!("not stopped: {state:?}");
    };
    let top = &frames[0];
    assert_eq!(top.line, 2);
    assert_eq!(
        top.path.as_deref(),
        Some(stand.join("app.py").as_path()),
        "the frame comes back in the stand-in path: {top:?}"
    );

    // A break while paused: the program waits; afterwards the session goes on.
    switch.set_reachable(false);
    live.break_connection();
    assert!(wait_until(Duration::from_secs(5), || !matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    std::thread::sleep(Duration::from_millis(300));
    switch.set_reachable(true);
    assert!(wait_until(Duration::from_secs(20), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    // The worker reattaches on its next loop; the request below goes through the new stream.
    let value = wait_for_value(&mut session, "a + 41", frames[0].id);
    assert_eq!(value, "42");

    session.resume_continue(thread_id).unwrap();
    let state = session.wait_state(Duration::from_secs(30), 5).unwrap();
    assert!(
        matches!(state, StopState::Exited { .. }),
        "program ended: {state:?}"
    );
    assert!(session.output().contains('2'), "{}", session.output());
    live.disconnect();
}

fn wait_for_value(session: &mut DebugSession, expression: &str, frame: i64) -> String {
    let t0 = Instant::now();
    loop {
        match session.evaluate(expression, frame) {
            Ok(v) => return v,
            Err(e) if t0.elapsed() < Duration::from_secs(40) => {
                eprintln!("evaluate again after {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => panic!("evaluate failed: {e}"),
        }
    }
}

/// The adapter process this test's host started whose command line holds `marker`: its pid and
/// the Unix socket named on that command line.
#[cfg(unix)]
fn adapter_with_socket(marker: &str) -> (u32, PathBuf) {
    let out = Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "command="])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let own = std::process::id().to_string();
    for line in text.lines().filter(|l| l.contains(marker)) {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.get(1) != Some(&own.as_str()) {
            continue;
        }
        let socket = words
            .iter()
            .find(|w| w.ends_with("/dap.sock"))
            .unwrap_or_else(|| panic!("no socket on the command line: {line}"));
        let path = socket.trim_start_matches("--listen=unix:");
        return (words[0].parse().unwrap(), PathBuf::from(path));
    }
    panic!("no adapter process with {marker}:\n{text}");
}

/// AK-3: the adapter's socket is a socket, its folder has mode 0700, and it opened no TCP port.
#[cfg(unix)]
fn assert_private_socket(marker: &str) {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let (pid, socket) = adapter_with_socket(marker);
    let kind = std::fs::symlink_metadata(&socket).unwrap().file_type();
    assert!(kind.is_socket(), "{socket:?} is not a socket");
    let folder = socket.parent().unwrap();
    let mode = std::fs::metadata(folder).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700, "{folder:?}");
    let out = Command::new("lsof")
        .args(["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-Fn", "-p"])
        .arg(pid.to_string())
        .output()
        .expect("lsof");
    let listening = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        !listening.lines().any(|l| l.starts_with('n')),
        "{marker} opened a TCP port: {listening}"
    );
}

/// AK-3: a real js-debug on its Unix socket in a 0700 folder; the window connects through
/// `dap.connect` for the child session.
#[test]
fn js_debug_on_a_private_unix_socket() {
    let Some(_) = idecraft_debugger::js_debug_server() else {
        eprintln!("js-debug not installed; skipped");
        return;
    };
    if Command::new("node").arg("-v").output().is_err() {
        return;
    }
    let dir = scratch("js");
    let project = dir.join("project");
    idecraft_core::trust::trust_for_process(&project);
    std::fs::write(project.join("app.js"), "const a = 1;\nconsole.log(a);\n").unwrap();
    let (live, _switch) = open(&dir);
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    let stand = PathBuf::from("/stand-in/js");
    let paths = PathMap::new(&stand, &project.to_string_lossy());
    let link = HostLink::Live(live.clone());
    let debug = start_host_debug(&link, "js-debug", json!({}), Some(paths)).unwrap();
    #[cfg(unix)]
    assert_private_socket("dapDebugServer");

    let mut breakpoints = BTreeMap::new();
    breakpoints.insert(stand.join("app.js"), vec![line_breakpoint(2)]);
    let launch = idecraft_debugger::node_launch(
        &project.join("app.js"),
        None,
        &[],
        &project,
        &BTreeMap::new(),
    );
    let (mut session, _) = DebugSession::launch_js_on(
        debug.connect.clone(),
        "launch",
        launch,
        &breakpoints,
        &Default::default(),
        Box::new(debug.client),
    )
    .unwrap();
    let state = session.wait_state(Duration::from_secs(60), 5).unwrap();
    let StopState::Stopped { frames, .. } = state else {
        panic!("not stopped: {state:?}");
    };
    assert_eq!(
        frames[0].path.as_deref(),
        Some(stand.join("app.js").as_path())
    );
    drop(session);
    live.disconnect();
}

/// AK-3 for Delve: a real `dlv dap` on a Unix socket in a 0700 folder, without a TCP port, stops at
/// a breakpoint set in the stand-in path.
#[cfg(unix)]
#[test]
fn dlv_on_a_private_unix_socket() {
    let installed = idecraft_debugger::known_adapters()
        .into_iter()
        .any(|a| a.name == "dlv" && a.command.is_some());
    if !installed || Command::new("go").arg("version").output().is_err() {
        eprintln!("dlv or go not installed; skipped");
        return;
    }
    let dir = scratch("dlv");
    let project = dir.join("project");
    idecraft_core::trust::trust_for_process(&project);
    std::fs::write(project.join("go.mod"), "module app\n\ngo 1.21\n").unwrap();
    std::fs::write(
        project.join("main.go"),
        "package main\n\nimport \"fmt\"\n\nfunc main() {\n\ta := 41\n\tfmt.Println(a + 1)\n}\n",
    )
    .unwrap();
    let (live, _switch) = open(&dir);
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    let stand = PathBuf::from("/stand-in/go");
    let paths = PathMap::new(&stand, &project.to_string_lossy());
    let link = HostLink::Live(live.clone());
    let debug = start_host_debug(&link, "dlv", json!({}), Some(paths)).unwrap();
    assert_private_socket("--listen=unix:");
    assert_eq!(debug.adapter.port, None, "a socket adapter has no port");
    if !idecraft_debugger::can_debug_unattended() {
        eprintln!("this machine asks before debugging; the session itself is skipped");
        live.disconnect();
        return;
    }
    let mut breakpoints = BTreeMap::new();
    breakpoints.insert(stand.join("main.go"), vec![line_breakpoint(7)]);
    let (mut session, _) = DebugSession::launch_on(
        debug.client,
        "go",
        json!({ "request": "launch", "mode": "debug", "program": project, "cwd": project }),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let state = session.wait_state(Duration::from_secs(120), 5).unwrap();
    let StopState::Stopped { frames, .. } = state else {
        panic!("not stopped: {state:?}");
    };
    assert_eq!(frames[0].line, 7);
    assert_eq!(
        frames[0].path.as_deref(),
        Some(stand.join("main.go").as_path()),
        "{:?}",
        frames[0]
    );
    let _ = session.disconnect();
    live.disconnect();
}

/// Behaviour 4: a real java-debug in jdtls on the host stops at a breakpoint in the stand-in path,
/// over a private socket: the adapter has no TCP port and nothing of the backend's children
/// listens on another interface.
#[test]
fn java_debug_on_the_host() {
    if !idecraft_languages::java_debug::java_debug_usable()
        || idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err()
    {
        eprintln!("jdtls or java-debug not installed; skipped");
        return;
    }
    let dir = scratch("java");
    let project = dir.join("project");
    idecraft_core::trust::trust_for_process(&project);
    std::fs::write(
        project.join("App.java"),
        "public class App {\n    public static void main(String[] a) {\n        int n = 41;\n        System.out.println(n + 1);\n    }\n}\n",
    )
    .unwrap();
    let (live, _switch) = open(&dir);
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    let stand = PathBuf::from("/stand-in/java");
    let paths = PathMap::new(&stand, &project.to_string_lossy());
    let link = HostLink::Live(live.clone());
    let debug = start_host_debug(
        &link,
        "java-debug",
        json!({ "main_class": "App" }),
        Some(paths),
    )
    .unwrap();
    assert_eq!(debug.adapter.port, None, "the adapter has a TCP port");

    let classpath = debug.adapter.classpath.clone().expect("a class path");
    let mut breakpoints = BTreeMap::new();
    breakpoints.insert(stand.join("App.java"), vec![line_breakpoint(4)]);
    let (mut session, _) = DebugSession::launch_on(
        debug.client,
        "java",
        json!({
            "type": "java", "request": "launch", "mainClass": "App", "cwd": project,
            "console": "internalConsole", "stopOnEntry": false,
            "modulePaths": classpath["modules"], "classPaths": classpath["classes"],
            "projectName": classpath["project"],
        }),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let state = session.wait_state(Duration::from_secs(120), 5).unwrap();
    let StopState::Stopped { frames, .. } = state else {
        panic!("not stopped: {state:?}");
    };
    assert_eq!(frames[0].line, 4, "{:?}", frames[0]);
    let open = open_listeners();
    assert!(open.is_empty(), "open on other interfaces: {open:?}");
    let _ = session.disconnect();
    live.disconnect();
}

/// TCP listeners of this process's children on anything but the loopback, as `lsof` prints them.
fn open_listeners() -> Vec<String> {
    let ps = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .expect("ps");
    let pairs: Vec<(u32, u32)> = String::from_utf8_lossy(&ps.stdout)
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
        })
        .collect();
    let mut found = vec![std::process::id()];
    let mut i = 0;
    while i < found.len() {
        let parent = found[i];
        found.extend(pairs.iter().filter(|p| p.1 == parent).map(|p| p.0));
        i += 1;
    }
    let pids = found[1..]
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let out = Command::new("lsof")
        .args(["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-Fn", "-p", &pids])
        .output()
        .expect("lsof");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter(|a| !a.starts_with("127.") && !a.starts_with("[::1]"))
        .map(str::to_owned)
        .collect()
}

struct Program(Child);

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// AK-8: attaching to `debugpy --listen` on the host, without a port on the window's machine.
#[test]
fn attaching_to_a_listening_debugpy() {
    if !has_debugpy() {
        eprintln!("debugpy not installed; skipped");
        return;
    }
    let dir = scratch("attach");
    let project = dir.join("project");
    std::fs::write(
        project.join("srv.py"),
        "import time\nfor i in range(600):\n    n = i\n    time.sleep(0.1)\n",
    )
    .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let _program = Program(
        Command::new("python3")
            .args([
                "-m",
                "debugpy",
                "--listen",
                &format!("127.0.0.1:{port}"),
                "--wait-for-client",
                "srv.py",
            ])
            .current_dir(&project)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let (live, _switch) = open(&dir);
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    let link = HostLink::Live(live.clone());
    let attach = json!({ "kind": "address", "host": "127.0.0.1", "port": port });
    // The program needs a moment before it listens.
    let mut last = None;
    let debug = (0..50).find_map(|_| {
        match start_host_debug(
            &link,
            "debugpy",
            json!({ "mode": "attach", "attach": attach }),
            None,
        ) {
            Ok(d) => Some(d),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(200));
                None
            }
        }
    });
    let debug = debug.unwrap_or_else(|| panic!("attach failed: {last:?}"));
    let target = idecraft_debugger::Target::Address {
        host: "127.0.0.1".into(),
        port,
    };
    let arguments =
        idecraft_debugger::attach_arguments(idecraft_debugger::Runtime::Python, &target, &project)
            .unwrap();
    let mut breakpoints = BTreeMap::new();
    breakpoints.insert(project.join("srv.py"), vec![line_breakpoint(3)]);
    let (mut session, _) = DebugSession::attach_on(
        debug.client,
        "debugpy",
        arguments,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let state = session.wait_state(Duration::from_secs(60), 5).unwrap();
    assert!(
        matches!(state, StopState::Stopped { .. }),
        "stopped: {state:?}"
    );
    let _ = session.disconnect();
    live.disconnect();
}
