//! SPEC-0147 AK-1 to AK-3: attaching to programs that already run (a C program by process id, a
//! Node.js script on its inspector port, a Python script under `debugpy --listen`), stopping at a
//! breakpoint and detaching, after which the program goes on. Skipped without the adapter.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use idecraft_debugger::{
    DebugSession, JsDebugServer, Runtime, SourceBreakpoint, StopState, Target, attach,
    attach_arguments, can_debug_unattended, classify, known_adapters, lldb_dap,
};

const WAIT: Duration = Duration::from_secs(30);

fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn adapter(name: &str) -> Option<Vec<String>> {
    known_adapters()
        .into_iter()
        .find(|a| a.name == name)
        .and_then(|a| a.command)
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-attach-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Kills the program when the test ends, also when it fails.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Running {
    fn alive(&mut self) -> bool {
        self.0.try_wait().unwrap().is_none()
    }
}

/// Waits until `port` accepts connections.
fn wait_for_port(port: u16) {
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("nothing listens on port {port}");
}

/// Stops at the breakpoint, evaluates `name` there (the program's counter, a number), then
/// detaches; the program still runs afterwards.
fn stop_and_detach(mut session: DebugSession, program: &mut Running, name: &str) {
    assert!(session.attached());
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped { frames, .. } => {
            let value = session.evaluate(name, frames[0].id).unwrap();
            assert!(value.parse::<i64>().is_ok(), "{name} = {value}");
        }
        other => panic!("expected a stop, got {other:?}"),
    }
    session.disconnect().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(program.alive(), "detaching ended the program");
}

const LOOP_C: &str = r#"#include <unistd.h>
#ifdef __linux__
#include <sys/prctl.h>
#endif

int main(void) {
#ifdef __linux__
    /* Yama (ptrace_scope 1) lets only a parent trace a process; the debugger is not ours. */
    prctl(PR_SET_PTRACER, PR_SET_PTRACER_ANY, 0, 0, 0);
#endif
    int ticks = 0;
    for (;;) {
        ticks++;
        usleep(50000);
    }
    return ticks;
}
"#;

#[test]
fn attach_to_a_native_process_by_id() {
    let Some(command) = lldb_dap() else {
        return skip("lldb-dap is not installed");
    };
    if !can_debug_unattended() {
        return skip("this Mac needs Developer Mode to attach without a password dialog");
    }
    if yama_forbids_attach() {
        return skip("Yama ptrace_scope is 2 or 3: only an administrator may attach");
    }
    let dir = temp("native");
    let source = dir.join("loop.c");
    std::fs::write(&source, LOOP_C).unwrap();
    let binary = dir.join("loop");
    let built = Command::new("cc")
        .args(["-g", "-O0", "-o"])
        .arg(&binary)
        .arg(&source)
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        return skip("no C compiler");
    }
    let mut program = Running(Command::new(&binary).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(300));
    let target = Target::Pid(program.0.id());
    let arguments = attach_arguments(Runtime::Native, &target, &dir).unwrap();
    let breakpoints = BTreeMap::from([(
        source.clone(),
        vec![SourceBreakpoint::from(line_of(LOOP_C, "ticks++"))],
    )]);
    let (session, placed) = DebugSession::attach(
        &command,
        "lldb-dap",
        arguments,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(placed[&source].len(), 1);
    stop_and_detach(session, &mut program, "ticks");
}

const LOOP_JS: &str = "let ticks = 0;\nsetInterval(() => {\n  ticks++;\n}, 50);\n";

#[test]
fn attach_to_node_on_its_inspector_port() {
    let Some(command) = adapter("js-debug") else {
        return skip("node or js-debug is not installed");
    };
    let dir = temp("node");
    let script = dir.join("loop.js");
    std::fs::write(&script, LOOP_JS).unwrap();
    let port = free_port();
    let mut program = Running(
        Command::new("node")
            .arg(format!("--inspect=127.0.0.1:{port}"))
            .arg(&script)
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for_port(port);
    let args = ["node".to_owned(), format!("--inspect=127.0.0.1:{port}")];
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_eq!(classify("node", &args), (Runtime::Node, Some(port)));
    let target = Target::parse_address(&format!("127.0.0.1:{port}")).unwrap();
    let arguments = attach_arguments(Runtime::Node, &target, &dir).unwrap();
    let breakpoints = BTreeMap::from([(script.clone(), vec![SourceBreakpoint::from(3)])]);
    let server = JsDebugServer::start(&command).unwrap();
    let address = server.address();
    let (session, _) = DebugSession::launch_js(
        &address,
        "attach",
        arguments,
        &breakpoints,
        &Default::default(),
        Box::new(server),
    )
    .unwrap();
    stop_and_detach(session, &mut program, "ticks");
}

const LOOP_PY: &str =
    "import time\n\nticks = 0\nwhile True:\n    ticks += 1\n    time.sleep(0.05)\n";

#[test]
fn attach_to_python_under_debugpy_listen() {
    if adapter("debugpy").is_none() {
        return skip("debugpy is not installed");
    }
    let dir = temp("python");
    let script = dir.join("loop.py");
    std::fs::write(&script, LOOP_PY).unwrap();
    let port = free_port();
    let mut program = Running(
        Command::new("python3")
            .args(["-m", "debugpy", "--listen"])
            .arg(format!("127.0.0.1:{port}"))
            .arg(&script)
            .spawn()
            .unwrap(),
    );
    let target = Target::parse_address(&format!("127.0.0.1:{port}")).unwrap();
    let breakpoints = BTreeMap::from([(script.clone(), vec![SourceBreakpoint::from(5)])]);
    // `debugpy --listen` runs the adapter inside the program and serves one client, which `attach`
    // talks to directly; a probe connection would use it up, so the attach itself is retried.
    let (session, _) = (0..100)
        .find_map(|_| {
            attach(
                Runtime::Python,
                &target,
                &[],
                &dir,
                &breakpoints,
                &Default::default(),
            )
            .inspect_err(|_| std::thread::sleep(Duration::from_millis(100)))
            .ok()
        })
        .expect("debugpy listens");
    stop_and_detach(session, &mut program, "ticks");
}

/// On Linux, Yama's `ptrace_scope` 2 (admin only) and 3 (no attach at all) refuse an attach that
/// `PR_SET_PTRACER` cannot open up; scope 0 and 1 allow it with the test program's opt-in.
fn yama_forbids_attach() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .is_some_and(|scope| scope >= 2)
}

/// The 1-based line of the first line of `source` that contains `text`.
fn line_of(source: &str, text: &str) -> u64 {
    source
        .lines()
        .position(|l| l.contains(text))
        .expect("the text is in the source") as u64
        + 1
}
