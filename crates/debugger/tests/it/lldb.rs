//! SPEC-0039 AK-2, AK-3 with `lldb-dap` and a C program (skipped without either).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use idecraft_debugger::native::{MAX_INSTRUCTIONS, hex_address, parse_address};
use idecraft_debugger::{
    DapError, DebugSession, Event, SourceBreakpoint, can_debug_unattended, lldb_dap,
};
use serde_json::json;

/// Skips a test whose tool is missing; with `IDECRAFT_REQUIRE_TOOLS` set (CI) it fails instead.
fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

const WAIT: Duration = Duration::from_secs(30);

const PROGRAM: &str = r#"#include <stdio.h>

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

/// Line of `int y = add(x, 1);`.
const CALL_LINE: u64 = 10;

fn build(dir: &Path) -> Option<PathBuf> {
    build_program(dir, PROGRAM)
}

fn build_program(dir: &Path, program: &str) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    // Compile under the resolved path: lldb matches breakpoint paths exactly, and on macOS the
    // temporary folder is reached through the `/var` → `/private/var` link.
    let dir = &std::fs::canonicalize(dir).ok()?;
    let source = dir.join("prog.c");
    std::fs::write(&source, program).ok()?;
    let binary = dir.join("prog");
    let ok = Command::new("cc")
        .args(["-g", "-O0", "-o"])
        .arg(&binary)
        .arg(&source)
        .status()
        .is_ok_and(|s| s.success());
    ok.then_some(binary)
}

fn stopped(event: Event) -> i64 {
    match event {
        Event::Stopped(s) => s.thread_id,
        other => panic!("expected a stop, got {other:?}"),
    }
}

#[test]
fn debug_a_c_program_with_lldb_dap() {
    let Some(adapter) = lldb_dap() else {
        skip("lldb-dap not installed");
        return;
    };
    if !can_debug_unattended() {
        // macOS would wait for a password dialog (Developer Mode off).
        skip("debugging needs Developer Mode on this Mac");
        return;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-dap-{}", std::process::id()));
    let Some(binary) = build(&dir) else {
        skip("no C compiler");
        return;
    };
    let source = std::fs::canonicalize(dir.join("prog.c")).unwrap();
    let breakpoints = BTreeMap::from([(source.clone(), vec![CALL_LINE.into()])]);
    let launch = json!({"program": binary, "cwd": dir, "stopOnEntry": false});
    let (mut session, placed) = DebugSession::launch(
        &adapter,
        "lldb-dap",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    // Linux lldb-dap verifies at once; Xcode's lldb-dap reports the breakpoint as pending until the
    // program is loaded. Either way it must be placed on the line; the stop below proves it works.
    assert_eq!(placed[&source][0].line, Some(CALL_LINE), "{placed:?}");

    let thread = match session.wait(WAIT).unwrap() {
        Event::Stopped(s) => {
            assert_eq!(s.reason, "breakpoint");
            s.thread_id
        }
        other => panic!("{other:?}"),
    };
    assert!(
        session
            .threads()
            .unwrap()
            .iter()
            .any(|(id, _)| *id == thread)
    );
    let frames = session.stack_trace(thread).unwrap();
    assert!(frames[0].name.contains("main"), "{frames:?}");
    assert_eq!(frames[0].line, CALL_LINE as i64);
    let frame = frames[0].id;
    let scopes = session.scopes(frame).unwrap();
    let locals = scopes.iter().find(|(n, _)| n.contains("Locals")).unwrap().1;
    let vars = session.variables(locals).unwrap();
    let x = vars.iter().find(|v| v.name == "x").unwrap();
    assert_eq!((x.value.as_str(), x.kind.as_deref()), ("42", Some("int")));
    assert_eq!(session.evaluate("x * 2", frame).unwrap(), "84");

    session.step_in(thread).unwrap();
    let thread = stopped(session.wait(WAIT).unwrap());
    let frames = session.stack_trace(thread).unwrap();
    assert!(frames[0].name.contains("add"), "{frames:?}");
    session.step_out(thread).unwrap();
    let thread = stopped(session.wait(WAIT).unwrap());
    assert!(
        session.stack_trace(thread).unwrap()[0]
            .name
            .contains("main")
    );
    session.next(thread).unwrap();
    let thread = stopped(session.wait(WAIT).unwrap());
    let frame = session.stack_trace(thread).unwrap()[0].id;
    assert_eq!(session.evaluate("y", frame).unwrap(), "43");

    // AK-3: a failing request carries the adapter's message. (lldb-dap leaves unknown
    // commands unanswered, which ends in a timeout; a bad expression is answered.)
    match session.evaluate("no_such_variable + 1", frame) {
        Err(DapError::Failed { command, message }) => {
            assert_eq!(command, "evaluate");
            assert!(!message.is_empty());
        }
        other => panic!("{other:?}"),
    }

    session.resume_continue(thread).unwrap();
    assert_eq!(session.wait(WAIT).unwrap(), Event::Exited(Some(3)));
    assert!(session.output().contains("y=43"), "{:?}", session.output());
    let _ = session.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}

const LOOP: &str = r#"#include <stdio.h>

int main(void) {
    int total = 0;
    for (int i = 0; i < 5; i++) {
        total += i;
    }
    printf("total=%d\n", total);
    return 0;
}
"#;

/// Line of `total += i;`.
const LOOP_LINE: u64 = 6;

/// Runs LOOP with one breakpoint and returns the values of `i` at each stop and the console.
fn stops_with(tag: &str, bp: SourceBreakpoint) -> Option<(Vec<String>, String)> {
    let adapter = lldb_dap()?;
    if !can_debug_unattended() {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-dap-{tag}-{}", std::process::id()));
    let binary = build_program(&dir, LOOP)?;
    let source = std::fs::canonicalize(dir.join("prog.c")).unwrap();
    let breakpoints = BTreeMap::from([(source, vec![bp])]);
    let launch = json!({"program": binary, "cwd": dir});
    let (mut session, placed) = DebugSession::launch(
        &adapter,
        "lldb-dap",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    // Pending (unverified) on macOS until the program is loaded; the stops below prove placement.
    assert!(
        placed.values().next().unwrap()[0].line.is_some(),
        "{placed:?}"
    );
    let mut seen = Vec::new();
    loop {
        match session.wait(WAIT).unwrap() {
            Event::Stopped(s) => {
                let frame = session.stack_trace(s.thread_id).unwrap()[0].id;
                seen.push(session.evaluate("i", frame).unwrap());
                session.resume_continue(s.thread_id).unwrap();
            }
            Event::Exited(code) => {
                assert_eq!(code, Some(0));
                break;
            }
        }
    }
    let console = session.console().to_owned();
    let _ = session.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
    Some((seen, console))
}

// SPEC-0039 v2 AK-5: conditions, hit counts and logpoints with lldb-dap.
#[test]
fn conditional_breakpoints_and_logpoints() {
    let condition = SourceBreakpoint {
        line: LOOP_LINE,
        condition: Some("i == 3".into()),
        ..SourceBreakpoint::default()
    };
    let Some((seen, _)) = stops_with("cond", condition) else {
        skip("lldb-dap, a C compiler or unattended debugging missing");
        return;
    };
    assert_eq!(seen, ["3"]);

    let hits = SourceBreakpoint {
        line: LOOP_LINE,
        hit_condition: Some("3".into()),
        ..SourceBreakpoint::default()
    };
    let (seen, _) = stops_with("hits", hits).unwrap();
    assert_eq!(seen.first().map(String::as_str), Some("2"), "{seen:?}");

    let log = SourceBreakpoint {
        line: LOOP_LINE,
        log_message: Some("i is {i}".into()),
        ..SourceBreakpoint::default()
    };
    let (seen, console) = stops_with("log", log).unwrap();
    assert!(seen.is_empty(), "a logpoint does not stop: {seen:?}");
    assert!(
        console.contains("i is 0") && console.contains("i is 4"),
        "{console:?}"
    );
}

/// IMP-0030: a program built through a symbolic link does not bind a breakpoint set on the resolved
/// path; the session reports it as pending with a hint, and a source map makes it bind.
#[test]
fn breakpoints_through_a_symbolic_link() {
    let Some(adapter) = lldb_dap() else {
        skip("lldb-dap not installed");
        return;
    };
    if !can_debug_unattended() {
        skip("debugging needs Developer Mode on this Mac");
        return;
    }
    let base = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("idecraft-dap-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let real = base.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = base.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    std::fs::write(link.join("prog.c"), PROGRAM).unwrap();
    // Compile with the absolute source path through the link, so the debug information names the
    // link path (a relative path in a linked folder would record the resolved one, as `getcwd` does).
    let built = Command::new("cc")
        .args(["-g", "-O0", "-o"])
        .arg(real.join("prog"))
        .arg(link.join("prog.c"))
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        skip("no C compiler");
        return;
    }
    let breakpoints = BTreeMap::from([(real.join("prog.c"), vec![CALL_LINE.into()])]);
    let program = real.join("prog");

    // Without a source map the breakpoint never binds; the end reports it.
    let launch = json!({"program": program, "cwd": link, "stopOnEntry": false});
    let (mut session, _) = DebugSession::launch(
        &adapter,
        "lldb-dap",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    match session.wait_state(WAIT, 20).unwrap() {
        idecraft_debugger::StopState::Exited {
            pending_breakpoints,
            ..
        } => {
            assert_eq!(pending_breakpoints.len(), 1, "{pending_breakpoints:?}");
            assert_eq!(pending_breakpoints[0].line, Some(CALL_LINE));
            assert!(pending_breakpoints[0].hint.contains("symbolic link"));
        }
        other => panic!("expected the program to run past the breakpoint: {other:?}"),
    }
    let _ = session.disconnect();

    // With the link mapped to the real folder it binds and stops.
    let launch = json!({
        "program": program, "cwd": link, "stopOnEntry": false,
        "sourceMap": [[link, real]],
    });
    let (mut session, _) = DebugSession::launch(
        &adapter,
        "lldb-dap",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    match session.wait_state(WAIT, 20).unwrap() {
        idecraft_debugger::StopState::Stopped {
            frames,
            pending_breakpoints,
            ..
        } => {
            assert_eq!(frames[0].line, CALL_LINE as i64);
            assert!(pending_breakpoints.is_empty(), "{pending_breakpoints:?}");
        }
        other => panic!("expected a stop: {other:?}"),
    }
    let _ = session.disconnect();
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0146 AK-2: a function breakpoint stops on entry of `add`; a C++ `throw` stops with
/// "caught" exception breakpoints and names the exception.
#[test]
fn function_and_exception_breakpoints_with_lldb_dap() {
    let Some(adapter) = lldb_dap() else {
        skip("lldb-dap not installed");
        return;
    };
    if !can_debug_unattended() {
        skip("debugging needs Developer Mode on this Mac");
        return;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-dap-special-{}", std::process::id()));
    let Some(binary) = build(&dir) else {
        skip("no C compiler");
        return;
    };
    let special = idecraft_debugger::SpecialBreakpoints {
        functions: vec![idecraft_debugger::FunctionBreakpoint {
            name: "add".into(),
            condition: None,
            ..Default::default()
        }],
        ..Default::default()
    };
    let launch = json!({"program": binary, "cwd": dir, "stopOnEntry": false});
    let (mut session, _) =
        DebugSession::launch(&adapter, "lldb-dap", launch, &BTreeMap::new(), &special).unwrap();
    let thread = stopped(session.wait(WAIT).unwrap());
    let frames = session.stack_trace(thread).unwrap();
    assert!(frames[0].name.contains("add"), "{frames:?}");
    session.disconnect().unwrap();

    // C++: an exception thrown and caught.
    let cpp = dir.join("throw.cpp");
    std::fs::write(
        &cpp,
        "#include <stdexcept>\nint main() {\n  try { throw std::runtime_error(\"boom\"); }\n  catch (const std::exception&) { return 0; }\n}\n",
    )
    .unwrap();
    let thrower = dir.join("throw");
    if !Command::new("c++")
        .args(["-g", "-O0", "-o"])
        .arg(&thrower)
        .arg(&cpp)
        .status()
        .is_ok_and(|s| s.success())
    {
        skip("no C++ compiler");
        return;
    }
    let special = idecraft_debugger::SpecialBreakpoints {
        caught: true,
        ..Default::default()
    };
    let launch = json!({"program": thrower, "cwd": dir, "stopOnEntry": false});
    let (mut session, _) =
        DebugSession::launch(&adapter, "lldb-dap", launch, &BTreeMap::new(), &special).unwrap();
    match session.wait(WAIT).unwrap() {
        Event::Stopped(s) => {
            assert_eq!(s.reason, "exception", "{s:?}");
            eprintln!("exception info: {:?}", session.exception_info(s.thread_id));
        }
        other => panic!("expected a stop on the throw, got {other:?}"),
    }
    session.disconnect().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0194 AK-1: pause a busy program, run to a later line with a temporary breakpoint, change a
/// local variable.
#[test]
fn pause_run_to_line_and_set_value_with_lldb_dap() {
    let Some(adapter) = lldb_dap() else {
        skip("lldb-dap not installed");
        return;
    };
    if !can_debug_unattended() {
        skip("debugging needs Developer Mode on this Mac");
        return;
    }
    const BUSY: &str = r#"#include <stdio.h>

int main(void) {
    volatile long n = 0;
    int x = 1;
    int y = 2;
    printf("x=%d\n", x);
    while (n >= 0) {
        n++;
    }
    return 0;
}
"#;
    let dir = std::env::temp_dir().join(format!("idecraft-dap-pause-{}", std::process::id()));
    let Some(binary) = build_program(&dir, BUSY) else {
        skip("no C compiler");
        return;
    };
    let source = std::fs::canonicalize(dir.join("prog.c")).unwrap();
    // Stop on `int y = 2;` (line 6), then run to the `printf` (line 7).
    let breakpoints = BTreeMap::from([(source.clone(), vec![6.into()])]);
    let launch = json!({"program": binary, "cwd": dir, "stopOnEntry": false});
    let (mut session, _) = DebugSession::launch(
        &adapter,
        "lldb-dap",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let thread = stopped(session.wait(WAIT).unwrap());
    let frame = session.stack_trace(thread).unwrap()[0].id;

    // Set Value on the local scope.
    let locals = session.locals_reference(frame).unwrap().unwrap();
    assert_eq!(session.set_variable(locals, "x", "42").unwrap(), "42");
    assert_eq!(session.evaluate("x", frame).unwrap(), "42");

    // Run to Cursor: a breakpoint next to the file's own until the stop; a line past the code is
    // refused without resuming.
    assert!(
        session.run_to_line(thread, &source, 100).is_err(),
        "past the end"
    );
    session.run_to_line(thread, &source, 7).unwrap();
    let thread = stopped(session.wait(WAIT).unwrap());
    assert_eq!(session.stack_trace(thread).unwrap()[0].line, 7);
    let left = session.set_breakpoints(&source, &[6.into()]).unwrap();
    assert_eq!(left.len(), 1, "only the file's own breakpoint");

    // Pause the endless loop.
    session.resume_continue(thread).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    session.pause(0).unwrap();
    match session.wait(WAIT).unwrap() {
        Event::Stopped(s) => {
            let info = session.exception_info(s.thread_id);
            assert_eq!(s.reason, "pause", "{s:?} {info:?}")
        }
        other => panic!("expected the pause, got {other:?}"),
    }
    session.disconnect().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

const BUFFER: &str = r#"#include <stdio.h>

static char marker[32] = "IDECRAFT-MEMORY-0123456789";

int main(void) {
    marker[0] = 'I';
    printf("%s\n", marker);
    return 0;
}
"#;

/// Line of `printf("%s\n", marker);`.
const BUFFER_LINE: u64 = 7;

/// SPEC-0054 v6 AK-8 (live, skipped without lldb-dap, a C compiler or unattended debugging): the
/// registers of the top frame hold the program counter the stack gives, `readMemory` of a known
/// buffer returns its bytes, and the disassembly around the program counter contains it.
#[test]
fn registers_memory_and_disassembly_with_lldb_dap() {
    let Some(adapter) = lldb_dap() else {
        skip("lldb-dap not installed");
        return;
    };
    if !can_debug_unattended() {
        skip("debugging needs Developer Mode on this Mac");
        return;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-dap-native-{}", std::process::id()));
    let Some(binary) = build_program(&dir, BUFFER) else {
        skip("no C compiler");
        return;
    };
    let source = std::fs::canonicalize(dir.join("prog.c")).unwrap();
    let breakpoints = BTreeMap::from([(source.clone(), vec![BUFFER_LINE.into()])]);
    let launch = json!({"program": binary, "cwd": dir, "stopOnEntry": false});
    let (mut session, _) = DebugSession::launch(
        &adapter,
        "lldb-dap",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let thread = stopped(session.wait(WAIT).unwrap());

    // Registers of the top frame: some group holds the program counter, equal to the frame's
    // instruction pointer.
    let ip = session.instruction_pointer(thread).unwrap();
    let groups = session.registers(thread).unwrap();
    assert!(!groups.is_empty());
    let pc = groups
        .iter()
        .flat_map(|g| &g.registers)
        .find(|r| matches!(r.name.as_str(), "pc" | "rip"))
        .unwrap_or_else(|| panic!("no program counter in {groups:?}"));
    let pc_value = parse_address(pc.value.split_whitespace().next().unwrap_or_default())
        .unwrap_or_else(|e| panic!("{pc:?}: {e}"));
    assert_eq!(pc_value, ip, "{pc:?}");

    // Memory of the known buffer, by the address the program itself evaluates.
    let frame = session.stack_trace(thread).unwrap()[0].id;
    let address: u64 = session
        .evaluate("(unsigned long long)&marker[0]", frame)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let block = session.read_memory(address, Some(32)).unwrap();
    assert_eq!(block.address, hex_address(address));
    assert_eq!((block.requested, block.read), (32, 32), "{block:?}");
    assert_eq!(block.rows.len(), 2);
    assert_eq!(block.rows[0].ascii, "IDECRAFT-MEMORY-");
    assert!(block.rows[0].hex.starts_with("49 44 45 43"), "{block:?}");
    assert!(block.rows[1].ascii.starts_with("0123456789"), "{block:?}");

    // Disassembly around the program counter: instructions before it, it, and after it.
    let code = session.disassemble(ip, 4, Some(12)).unwrap();
    assert_eq!(code.len(), 12, "{code:?}");
    let at = code
        .iter()
        .position(|i| i.address == hex_address(ip))
        .unwrap_or_else(|| panic!("{ip:#x} not in {code:?}"));
    assert!(at > 0 && at < code.len() - 1, "{at} in {code:?}");
    assert!(
        code.iter()
            .all(|i| !i.instruction.is_empty() && i.instruction.trim_end() == i.instruction),
        "{code:?}"
    );
    assert!(code.iter().any(|i| i.line.is_some()), "{code:?}");
    // lldb-dap names the symbol on the first instruction of a function only: from main's start.
    let entry = session
        .disassemble(ip, MAX_INSTRUCTIONS, Some(MAX_INSTRUCTIONS))
        .unwrap();
    assert!(
        entry
            .iter()
            .any(|i| i.symbol.as_deref().is_some_and(|s| s.contains("main"))),
        "{entry:?}"
    );

    session.disconnect().ok();
    std::fs::remove_dir_all(&dir).ok();
}
