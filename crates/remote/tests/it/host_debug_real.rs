//! SPEC-0322 v7: real Delve, lldb-dap and java-debug on the (in-process) host, driven through the
//! window's link like debugpy in `host_debug.rs`: a launch stops at a breakpoint set in the stand-in
//! path, a variable is read, a step and a resume work, the exit code arrives, and a break of the
//! session while paused loses nothing. Delve is also attached to by `host:port` on the host's
//! loopback. Each test skips when its tools are missing (`FERRIS_IDEA_REQUIRE_TOOLS` makes that fail).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferris_idea_debugger::{DebugSession, Event, PathMap, SourceBreakpoint};
use ferris_idea_remote::host_debug::{HostLink, start_host_debug};
use ferris_idea_remote::inprocess::{HostSwitch, InProcessProject, Setup};
use ferris_idea_remote::{ConnState, LiveProject, Timing};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(60);

fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idc-hostreal-{tag}-{}", std::process::id()));
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
        ferris_idea_host::bind(
            b,
            ferris_idea_host::HostOptions {
                language_servers: false,
                poll: Duration::from_millis(200),
                shell: Some("/bin/sh".into()),
                ..ferris_idea_host::HostOptions::default()
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
    let live = LiveProject::start(Box::new(p), fast());
    assert!(wait_until(Duration::from_secs(10), || connected(&live)));
    (live, switch)
}

fn connected(live: &LiveProject) -> bool {
    matches!(live.state(), ConnState::Connected { .. })
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

fn line(line: u64) -> SourceBreakpoint {
    SourceBreakpoint {
        line,
        ..Default::default()
    }
}

/// The thread of the next event, which must be a stop.
fn stopped(session: &mut DebugSession) -> i64 {
    match session.wait(WAIT).unwrap() {
        Event::Stopped(s) => s.thread_id,
        other => panic!("expected a stop, got {other:?}"),
    }
}

/// Breaks the session while the program is paused and waits until the window is back.
fn break_and_return(live: &LiveProject, switch: &HostSwitch) {
    switch.set_reachable(false);
    live.break_connection();
    assert!(wait_until(Duration::from_secs(5), || !connected(live)));
    std::thread::sleep(Duration::from_millis(300));
    switch.set_reachable(true);
    assert!(wait_until(Duration::from_secs(20), || connected(live)));
}

/// `evaluate` until the worker has reattached (the first request after a break may go to the
/// stream that broke).
fn evaluate_after_break(session: &mut DebugSession, expression: &str, frame: i64) -> String {
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

/// The value of local `name` in `frame`.
fn local(session: &mut DebugSession, frame: i64, name: &str) -> String {
    let scopes = session.scopes(frame).unwrap();
    for (_, reference) in scopes {
        let vars = session.variables(reference).unwrap();
        if let Some(v) = vars.iter().find(|v| v.name == name) {
            return v.value.clone();
        }
    }
    panic!("no variable {name} in frame {frame}");
}

/// A process this test started; ended by its own pid when the test is over.
struct Program(Child);

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Ends the process whose pid the debugged program wrote to `file` (it was started by this test,
/// below a Delve server this test started).
struct PidFile(PathBuf);

impl Drop for PidFile {
    fn drop(&mut self) {
        if let Some(pid) = std::fs::read_to_string(&self.0)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
        {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
    }
}

fn go_ready() -> bool {
    let dlv = ferris_idea_debugger::known_adapters()
        .into_iter()
        .any(|a| a.name == "dlv" && a.command.is_some());
    if !dlv || Command::new("go").arg("version").output().is_err() {
        skip("dlv or go not installed");
        return false;
    }
    if !ferris_idea_debugger::can_debug_unattended() {
        skip("debugging needs Developer Mode on this Mac");
        return false;
    }
    true
}

const GO_PROGRAM: &str = "package main

import (
\t\"fmt\"
\t\"os\"
)

func add(a, b int) int {
\treturn a + b
}

func main() {
\tx := 41
\ty := add(x, 1)
\tfmt.Println(\"y =\", y)
\tos.Exit(y - 39)
}
";

/// Line of `y := add(x, 1)` in [`GO_PROGRAM`].
const GO_CALL: u64 = 14;

/// AK-17: a real `dlv dap` on the host: breakpoint in the stand-in path, variable, step over,
/// a break while paused, resume, exit code.
#[cfg(unix)]
#[test]
fn delve_launch_on_the_host_with_a_break() {
    if !go_ready() {
        return;
    }
    let dir = scratch("dlv");
    let project = dir.join("project");
    ferris_idea_core::trust::trust_for_process(&project);
    std::fs::write(project.join("go.mod"), "module app\n\ngo 1.21\n").unwrap();
    std::fs::write(project.join("main.go"), GO_PROGRAM).unwrap();
    let (live, switch) = open(&dir);
    let stand = PathBuf::from("/stand-in/go");
    let paths = PathMap::new(&stand, &project.to_string_lossy());
    let link = HostLink::Live(live.clone());
    let debug = start_host_debug(&link, "dlv", json!({}), Some(paths)).unwrap();
    let breakpoints = BTreeMap::from([(stand.join("main.go"), vec![line(GO_CALL)])]);
    // The launch request as a Debug of `go run .` builds it (program output as events).
    let launch =
        ferris_idea_debugger::go_launch(&["run".into(), ".".into()], &project, &BTreeMap::new())
            .unwrap();
    let (mut session, _) = DebugSession::launch_on(
        debug.client,
        "go",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let thread = stopped(&mut session);
    let frames = session.stack_trace(thread).unwrap();
    assert_eq!(frames[0].line, GO_CALL as i64, "{:?}", frames[0]);
    assert_eq!(
        frames[0].path.as_deref(),
        Some(stand.join("main.go").as_path())
    );
    assert_eq!(local(&mut session, frames[0].id, "x"), "41");

    break_and_return(&live, &switch);
    assert_eq!(
        evaluate_after_break(&mut session, "x + 1", frames[0].id),
        "42"
    );

    session.next(thread).unwrap();
    let thread = stopped(&mut session);
    let frames = session.stack_trace(thread).unwrap();
    assert_eq!(frames[0].line, GO_CALL as i64 + 1, "{:?}", frames[0]);
    assert_eq!(local(&mut session, frames[0].id, "y"), "42");

    session.resume_continue(thread).unwrap();
    let end = session.wait(WAIT).unwrap();
    assert_eq!(end, Event::Exited(Some(3)), "{}", session.output());
    assert!(session.output().contains("y = 42"), "{}", session.output());
    let _ = session.disconnect();
    live.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}

const GO_SERVER: &str = "package main

import (
\t\"fmt\"
\t\"os\"
\t\"time\"
)

func main() {
\t_ = os.WriteFile(\"pid\", []byte(fmt.Sprint(os.Getpid())), 0o600)
\tfor i := 0; i < 6000; i++ {
\t\tn := i * 2
\t\ttime.Sleep(20 * time.Millisecond)
\t\t_ = n
\t}
}
";

/// Line of `time.Sleep(…)` in [`GO_SERVER`].
const GO_SLEEP: u64 = 13;

/// AK-18: `host:port` attach to a headless Delve server on the host's loopback. The backend talks
/// to the server directly (it speaks DAP itself), as a local attach does: no `dlv dap` of its own,
/// no port on the window's machine.
#[cfg(unix)]
#[test]
fn delve_server_attached_by_host_and_port() {
    if !go_ready() {
        return;
    }
    let dir = scratch("dlvsrv");
    let project = dir.join("project");
    std::fs::write(project.join("go.mod"), "module srv\n\ngo 1.21\n").unwrap();
    std::fs::write(project.join("main.go"), GO_SERVER).unwrap();
    let binary = project.join("srv");
    let built = Command::new("go")
        .args(["build", "-gcflags=all=-N -l", "-o"])
        .arg(&binary)
        .arg(".")
        .current_dir(&project)
        .status()
        .is_ok_and(|s| s.success());
    assert!(built, "go build failed");
    let dlv = ferris_idea_debugger::delve_binary().unwrap();
    let _pid = PidFile(project.join("pid"));
    let mut server = Command::new(dlv)
        .arg("exec")
        .arg(&binary)
        .args([
            "--headless",
            "--accept-multiclient",
            "--continue",
            "--api-version=2",
            "--listen=127.0.0.1:0",
        ])
        .current_dir(&project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = server.stdout.take().unwrap();
    let server = Program(server);
    // "API server listening at: 127.0.0.1:<port>"
    let port: u16 = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .find_map(|l| {
            l.split("127.0.0.1:")
                .nth(1)
                .and_then(|p| p.trim().parse().ok())
        })
        .expect("the Delve server named no port");

    let (live, switch) = open(&dir);
    let link = HostLink::Live(live.clone());
    let attach = json!({ "kind": "address", "host": "127.0.0.1", "port": port });
    let debug = start_host_debug(
        &link,
        "dlv",
        json!({ "mode": "attach", "attach": attach }),
        None,
    )
    .unwrap();
    assert_eq!(debug.adapter.port, None, "no adapter of its own");
    assert_eq!(debug.adapter.address, Some(("127.0.0.1".into(), port)));
    let target = ferris_idea_debugger::Target::Address {
        host: "127.0.0.1".into(),
        port,
    };
    let arguments = ferris_idea_debugger::attach_arguments(
        ferris_idea_debugger::Runtime::Go,
        &target,
        &project,
    )
    .unwrap();
    let breakpoints = BTreeMap::from([(project.join("main.go"), vec![line(GO_SLEEP)])]);
    let (mut session, _) = DebugSession::attach_on(
        debug.client,
        "dlv",
        arguments,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let thread = stopped(&mut session);
    let frames = session.stack_trace(thread).unwrap();
    assert_eq!(frames[0].line, GO_SLEEP as i64, "{:?}", frames[0]);
    let first: i64 = local(&mut session, frames[0].id, "n").parse().unwrap();
    assert_eq!(first % 2, 0);

    break_and_return(&live, &switch);
    let i: i64 = evaluate_after_break(&mut session, "i", frames[0].id)
        .parse()
        .unwrap();
    assert_eq!(i * 2, first);

    // Resume: the next stop is the loop's next round.
    session.resume_continue(thread).unwrap();
    let thread = stopped(&mut session);
    let frame = session.stack_trace(thread).unwrap()[0].id;
    let next: i64 = local(&mut session, frame, "n").parse().unwrap();
    assert_eq!(next, first + 2);
    let _ = session.disconnect();
    drop(server);
    live.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}

const C_PROGRAM: &str = r#"#include <stdio.h>

int add(int a, int b) {
    int s = a + b;
    return s;
}

int main(void) {
    int x = 42;
    int y = add(x, 1);
    printf("y=%d\n", y);
    return y - 40;
}
"#;

/// Line of `int y = add(x, 1);` in [`C_PROGRAM`].
const C_CALL: u64 = 10;

/// AK-19: a real lldb-dap (stdio) on the host: breakpoint in the stand-in path, variable, step in
/// and out, a break while paused, step over, resume, exit code.
#[test]
fn lldb_dap_on_the_host_with_a_break() {
    if ferris_idea_debugger::lldb_dap().is_none() {
        skip("lldb-dap not installed");
        return;
    }
    if !ferris_idea_debugger::can_debug_unattended() {
        skip("debugging needs Developer Mode on this Mac");
        return;
    }
    let dir = scratch("lldb");
    let project = dir.join("project");
    ferris_idea_core::trust::trust_for_process(&project);
    let source = project.join("prog.c");
    std::fs::write(&source, C_PROGRAM).unwrap();
    let binary = project.join("prog");
    let built = Command::new("cc")
        .args(["-g", "-O0", "-o"])
        .arg(&binary)
        .arg(&source)
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        skip("no C compiler");
        return;
    }
    let (live, switch) = open(&dir);
    let stand = PathBuf::from("/stand-in/c");
    let paths = PathMap::new(&stand, &project.to_string_lossy());
    let link = HostLink::Live(live.clone());
    let debug = start_host_debug(&link, "lldb-dap", json!({}), Some(paths)).unwrap();
    assert_eq!(debug.adapter.port, None, "a stdio adapter has no port");
    let breakpoints = BTreeMap::from([(stand.join("prog.c"), vec![line(C_CALL)])]);
    let (mut session, placed) = DebugSession::launch_on(
        debug.client,
        "lldb-dap",
        json!({ "program": binary, "cwd": project, "stopOnEntry": false }),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        placed[&stand.join("prog.c")][0].line,
        Some(C_CALL),
        "{placed:?}"
    );
    let thread = stopped(&mut session);
    let frames = session.stack_trace(thread).unwrap();
    assert!(frames[0].name.contains("main"), "{frames:?}");
    assert_eq!(frames[0].line, C_CALL as i64);
    assert_eq!(
        frames[0].path.as_deref(),
        Some(stand.join("prog.c").as_path()),
        "the frame comes back in the stand-in path"
    );
    assert_eq!(local(&mut session, frames[0].id, "x"), "42");

    session.step_in(thread).unwrap();
    let thread = stopped(&mut session);
    let frames = session.stack_trace(thread).unwrap();
    assert!(frames[0].name.contains("add"), "{frames:?}");

    break_and_return(&live, &switch);
    assert_eq!(
        evaluate_after_break(&mut session, "a + b", frames[0].id),
        "43"
    );

    session.step_out(thread).unwrap();
    let thread = stopped(&mut session);
    assert!(
        session.stack_trace(thread).unwrap()[0]
            .name
            .contains("main")
    );
    session.next(thread).unwrap();
    let thread = stopped(&mut session);
    let frame = session.stack_trace(thread).unwrap()[0].id;
    assert_eq!(local(&mut session, frame, "y"), "43");

    session.resume_continue(thread).unwrap();
    assert_eq!(session.wait(WAIT).unwrap(), Event::Exited(Some(3)));
    assert!(session.output().contains("y=43"), "{}", session.output());
    let _ = session.disconnect();
    live.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}

const JAVA_PROGRAM: &str = "public class App {
    static int add(int a, int b) {
        return a + b;
    }

    public static void main(String[] args) {
        int x = 41;
        int y = add(x, 1);
        System.out.println(\"y=\" + y);
        System.exit(y - 39);
    }
}
";

/// Line of `int y = add(x, 1);` in [`JAVA_PROGRAM`].
const JAVA_CALL: u64 = 8;

/// AK-20: a real java-debug in jdtls on the host: breakpoint in the stand-in path, variable, a
/// break while paused, step over, resume to the end.
#[test]
fn java_debug_on_the_host_with_a_break() {
    if !ferris_idea_languages::java_debug::java_debug_usable()
        || ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err()
    {
        skip("jdtls or java-debug not installed");
        return;
    }
    let dir = scratch("java");
    let project = dir.join("project");
    ferris_idea_core::trust::trust_for_process(&project);
    std::fs::write(project.join("App.java"), JAVA_PROGRAM).unwrap();
    let (live, switch) = open(&dir);
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
    let classpath = debug.adapter.classpath.clone().expect("a class path");
    let breakpoints = BTreeMap::from([(stand.join("App.java"), vec![line(JAVA_CALL)])]);
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
    let thread = stopped(&mut session);
    let frames = session.stack_trace(thread).unwrap();
    assert_eq!(frames[0].line, JAVA_CALL as i64, "{:?}", frames[0]);
    assert_eq!(local(&mut session, frames[0].id, "x"), "41");

    break_and_return(&live, &switch);
    assert_eq!(
        evaluate_after_break(&mut session, "x + 1", frames[0].id),
        "42"
    );

    session.next(thread).unwrap();
    let thread = stopped(&mut session);
    let frames = session.stack_trace(thread).unwrap();
    assert_eq!(frames[0].line, JAVA_CALL as i64 + 1, "{:?}", frames[0]);
    assert_eq!(local(&mut session, frames[0].id, "y"), "42");

    session.resume_continue(thread).unwrap();
    // java-debug's `exited` says 0 for `System.exit(3)`: no code is shown rather than a wrong one
    // (SPEC-0322 v7 behaviour 8).
    assert_eq!(session.wait(WAIT).unwrap(), Event::Exited(None));
    assert!(session.output().contains("y=42"), "{}", session.output());
    let _ = session.disconnect();
    live.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}
