//! SPEC-0039 v7 AK-12 to AK-14 and SPEC-0147 v3 AK-7: Go programs under a real Delve. Skipped
//! without `dlv` or `go`, and on a Mac without Developer Mode.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_debugger::{
    DapClient, DebugSession, DelveServer, Runtime, SourceBreakpoint, SpecialBreakpoints, StopState,
    Target, attach, attach_arguments, go_launch, known_adapters, launch_go, lifetime,
};

/// The first build of a program compiles the standard library without optimisations.
const WAIT: Duration = Duration::from_secs(120);

const MAIN: &str = r#"package main

import (
	"fmt"
	"os"

	"example.com/demo/calc"
)

type point struct {
	X, Y int
}

func main() {
	total := 0
	p := point{1, 2}
	for i := 0; i < 5; i++ {
		total = calc.Add(total, i)
		fmt.Println("i =", i)
	}
	fmt.Fprintln(os.Stderr, "done", total, p)
	if len(os.Args) > 1 {
		var m map[string]int
		m["x"] = 1
	}
}
"#;

const CALC: &str =
    "package calc\n\n// Add adds.\nfunc Add(a, b int) int {\n\tsum := a + b\n\treturn sum\n}\n";

const CALC_TEST: &str = "package calc\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {\n\tgot := Add(2, 3)\n\tif got != 5 {\n\t\tt.Fatal(got)\n\t}\n}\n\nfunc TestOther(t *testing.T) {\n\tt.Log(\"other ran\")\n}\n";

/// A program that counts for ever, for attaching.
const LOOP: &str = "package main\n\nimport \"time\"\n\nfunc main() {\n\tn := 0\n\tfor {\n\t\tn++\n\t\ttime.Sleep(20 * time.Millisecond)\n\t}\n}\n";

/// `dlv dap` when Delve and Go are installed and a debugger may take over a process unattended.
fn delve() -> Option<Vec<String>> {
    let command = known_adapters()
        .into_iter()
        .find(|a| a.name == "dlv")
        .and_then(|a| a.command);
    let go = std::process::Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    match command {
        Some(command) if go && ferris_idea_debugger::can_debug_unattended() => Some(command),
        _ => {
            eprintln!("skipped: dlv or go is not installed, or no unattended debugging");
            None
        }
    }
}

/// A Go module with a program, a package and its tests, in the temporary folder as it is named
/// (on macOS behind a symbolic link, which Delve's path substitution has to bridge).
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-delve-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("calc")).unwrap();
    std::fs::write(dir.join("go.mod"), "module example.com/demo\n\ngo 1.21\n").unwrap();
    std::fs::write(dir.join("main.go"), MAIN).unwrap();
    std::fs::write(dir.join("calc/calc.go"), CALC).unwrap();
    std::fs::write(dir.join("calc/calc_test.go"), CALC_TEST).unwrap();
    dir
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// AK-12: a program stops in its loop where the condition holds, shows its locals and its
/// goroutines as threads, evaluates an expression, logs at a logpoint without stopping, stops
/// once at a hit count and runs to its end; Delve, its socket folder and the built program are
/// gone afterwards and nothing was written into the project.
#[test]
fn go_program_stops_in_a_loop_with_condition_and_evaluate() {
    let Some(command) = delve() else { return };
    let dir = project("run");
    let main = dir.join("main.go");
    let mut launch = go_launch(&words("run main.go"), &dir, &BTreeMap::new()).unwrap();
    let breakpoints = BTreeMap::from([(
        main.clone(),
        vec![
            SourceBreakpoint {
                line: 18,
                condition: Some("i == 3".into()),
                ..Default::default()
            },
            SourceBreakpoint {
                line: 19,
                log_message: Some("i is {i}".into()),
                ..Default::default()
            },
            SourceBreakpoint {
                line: 21,
                hit_condition: Some("1".into()),
                ..Default::default()
            },
        ],
    )]);
    let server = DelveServer::start(&command).unwrap();
    let pid = server.pid();
    let address = server.address().to_owned();
    let folder = PathBuf::from(address.trim_start_matches("unix:"))
        .parent()
        .map(Path::to_path_buf);
    server.complete(&mut launch);
    let client = DapClient::connect(&address, Some(Box::new(server))).unwrap();
    let (mut session, placed) =
        DebugSession::launch_on(client, "dlv", launch, &breakpoints, &Default::default()).unwrap();
    assert_eq!(placed[&main].len(), 3);
    assert!(placed[&main].iter().all(|b| b.verified), "{placed:?}");
    assert!(
        placed[&main]
            .iter()
            .all(|b| b.evaluated_by == ferris_idea_debugger::EvaluatedBy::Adapter),
        "Delve evaluates conditions, hit counts and log messages itself: {placed:?}"
    );
    let thread = match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            reason,
            frames,
            locals,
            thread_id,
            pending_breakpoints,
        } => {
            assert_eq!(reason, "breakpoint");
            assert!(frames[0].name.ends_with("main.main"), "{}", frames[0].name);
            assert_eq!(frames[0].line, 18);
            assert_eq!(frames[0].path.as_deref(), Some(main.as_path()));
            let value = |name: &str| {
                locals
                    .iter()
                    .find(|v| v.name == name)
                    .map(|v| v.value.clone())
            };
            assert_eq!(value("i").as_deref(), Some("3"));
            assert_eq!(value("total").as_deref(), Some("3"));
            assert!(value("p").is_some_and(|p| p.contains("X: 1")), "{locals:?}");
            assert_eq!(
                session.evaluate("total * 2 + i", frames[0].id).unwrap(),
                "9"
            );
            assert!(pending_breakpoints.is_empty(), "{pending_breakpoints:?}");
            thread_id
        }
        other => panic!("not stopped: {other:?}"),
    };
    // Goroutines are the threads.
    let threads = session.threads().unwrap();
    assert!(
        threads
            .iter()
            .any(|(id, name)| *id == thread && name.contains("main.main")),
        "{threads:?}"
    );
    assert!(threads.len() > 1, "{threads:?}");
    session.resume_continue(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped { frames, locals, .. } => {
            assert_eq!(frames[0].line, 21, "the hit count stops after the loop");
            assert!(locals.iter().any(|v| v.name == "total" && v.value == "10"));
        }
        other => panic!("not stopped at the hit count: {other:?}"),
    }
    session.resume_continue(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Exited { exit_code, .. } => assert_eq!(exit_code, Some(0)),
        other => panic!("not ended: {other:?}"),
    }
    let output = session.output().to_owned();
    assert!(
        output.contains("i = 0\n") && output.contains("i = 4\n"),
        "{output}"
    );
    assert!(output.contains("done 10 {1 2}"), "{output}");
    assert!(
        !output.contains("i is"),
        "logpoint messages are console text: {output}"
    );
    let console = session.console().to_owned();
    for i in 0..5 {
        assert!(console.contains(&format!("i is {i}")), "{console}");
    }
    assert!(console.contains("Building"), "{console}");
    session.disconnect().unwrap();
    assert!(
        lifetime::command_of(pid).is_none(),
        "Delve ended with the session"
    );
    if let Some(folder) = folder {
        assert!(!folder.exists(), "the socket folder is removed: {folder:?}");
    }
    assert_eq!(
        files(&dir),
        ["calc", "go.mod", "main.go"],
        "nothing built into the project"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// AK-13: one test function of a package, as its ▶ marker names it: the breakpoint in the code
/// under test stops below the test function, the other test does not run.
#[test]
fn go_test_function_stops_at_a_breakpoint() {
    let Some(command) = delve() else { return };
    let dir = project("test");
    let calc = dir.join("calc/calc.go");
    let launch = go_launch(
        &words("test -json -run ^TestAdd$ ./calc"),
        &dir,
        &BTreeMap::new(),
    )
    .unwrap();
    let breakpoints = BTreeMap::from([(calc.clone(), vec![SourceBreakpoint::from(5)])]);
    let (mut session, placed) =
        launch_go(&command, launch, &breakpoints, &Default::default()).unwrap();
    assert!(placed[&calc][0].verified, "{placed:?}");
    let thread = match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert!(frames[0].name.ends_with("calc.Add"), "{}", frames[0].name);
            assert_eq!(frames[0].line, 5);
            assert_eq!(frames[0].path.as_deref(), Some(calc.as_path()));
            assert!(
                frames[1].name.ends_with("calc.TestAdd"),
                "{}",
                frames[1].name
            );
            assert!(locals.iter().any(|v| v.name == "a" && v.value == "2"));
            assert!(locals.iter().any(|v| v.name == "b" && v.value == "3"));
            assert_eq!(session.evaluate("a + b", frames[0].id).unwrap(), "5");
            thread_id
        }
        other => panic!("not stopped: {other:?}"),
    };
    session.next(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped { frames, locals, .. } => {
            assert_eq!(frames[0].line, 6);
            assert!(locals.iter().any(|v| v.name == "sum" && v.value == "5"));
        }
        other => panic!("not stepped: {other:?}"),
    }
    session.resume_continue(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Exited { exit_code, .. } => assert_eq!(exit_code, Some(0)),
        other => panic!("not ended: {other:?}"),
    }
    let output = session.output().to_owned();
    assert!(output.contains("--- PASS: TestAdd"), "{output}");
    assert!(!output.contains("TestOther"), "{output}");
    session.disconnect().unwrap();
    assert_eq!(files(&dir.join("calc")), ["calc.go", "calc_test.go"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// AK-14 and SPEC-0146 v5 AK-9: with uncaught exceptions on, a panic nothing recovers stops with its message, and a
/// function breakpoint stops on entry; a program that does not build is refused at once with
/// what the compiler said.
#[test]
fn go_panic_function_breakpoint_and_build_error() {
    let Some(command) = delve() else { return };
    let dir = project("panic");
    let launch = go_launch(&words("run . crash"), &dir, &BTreeMap::new()).unwrap();
    let special = SpecialBreakpoints {
        uncaught: true,
        functions: vec![ferris_idea_debugger::FunctionBreakpoint {
            name: "calc.Add".into(),
            condition: Some("b == 4".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let (mut session, _) = launch_go(&command, launch, &BTreeMap::new(), &special).unwrap();
    assert!(
        !session.output().contains("[Ferris IDEA]"),
        "{}",
        session.output()
    );
    let thread = match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert!(frames[0].name.ends_with("calc.Add"), "{}", frames[0].name);
            assert!(locals.iter().any(|v| v.name == "b" && v.value == "4"));
            thread_id
        }
        other => panic!("not stopped at the function: {other:?}"),
    };
    session.resume_continue(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            reason, thread_id, ..
        } => {
            assert_eq!(reason, "exception");
            let info = session.exception_info(thread_id).unwrap_or_default();
            assert!(info.contains("nil map"), "{info}");
        }
        other => panic!("not stopped at the panic: {other:?}"),
    }
    session.disconnect().unwrap();
    // Caught exceptions: Delve has no stop on every panic; it is said, the session goes on.
    let launch = go_launch(&words("run ."), &dir, &BTreeMap::new()).unwrap();
    let caught = SpecialBreakpoints {
        caught: true,
        ..Default::default()
    };
    let (mut session, _) = launch_go(&command, launch, &BTreeMap::new(), &caught).unwrap();
    assert!(
        session
            .output()
            .contains("no breakpoint on caught exceptions"),
        "{}",
        session.output()
    );
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Exited { exit_code, .. } => assert_eq!(exit_code, Some(0)),
        other => panic!("not ended: {other:?}"),
    }
    session.disconnect().unwrap();
    // A build error.
    std::fs::write(
        dir.join("main.go"),
        "package main\n\nfunc main() {\n\tmissing()\n}\n",
    )
    .unwrap();
    let launch = go_launch(&words("run ."), &dir, &BTreeMap::new()).unwrap();
    let started = std::time::Instant::now();
    let error = match launch_go(&command, launch, &BTreeMap::new(), &Default::default()) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a program that does not build started"),
    };
    assert!(error.contains("undefined: missing"), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(100),
        "not a timeout"
    );
    assert_eq!(files(&dir), ["calc", "go.mod", "main.go"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0147 v3 AK-7: a Delve server on this machine is attached to through its port, without
/// an adapter of Ferris IDEA's: the program stops at the breakpoint, and the console says what Stop
/// does to such a server.
#[test]
fn go_delve_server_is_attached_through_its_port() {
    use std::io::BufRead;
    let Some(command) = delve() else { return };
    let dir = std::fs::canonicalize(project("server")).unwrap();
    let main = dir.join("main.go");
    std::fs::write(&main, LOOP).unwrap();
    let built = std::process::Command::new("go")
        .args(["build", "-gcflags=all=-N -l", "-o", "loop", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let arguments = ["exec", "./loop", "--headless", "--listen=127.0.0.1:0"];
    let mut server = std::process::Command::new(&command[0])
        .args(arguments)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // "API server listening at: 127.0.0.1:<port>"
    let mut line = String::new();
    std::io::BufReader::new(server.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port: Option<u16> = line.trim().rsplit(':').next().and_then(|p| p.parse().ok());
    let outcome = port.ok_or(format!("no port in `{line}`")).and_then(|port| {
        let listed = [
            "dlv",
            "exec",
            "./loop",
            "--headless",
            &format!("--listen=127.0.0.1:{port}"),
        ];
        assert_eq!(
            ferris_idea_debugger::classify("dlv", &listed),
            (Runtime::Go, Some(port))
        );
        let target = Target::Address {
            host: "127.0.0.1".into(),
            port,
        };
        let breakpoints = BTreeMap::from([(main.clone(), vec![SourceBreakpoint::from(8)])]);
        // No adapter command: the server is the adapter.
        let (mut session, _) = attach(
            Runtime::Go,
            &target,
            &[],
            &dir,
            &breakpoints,
            &Default::default(),
        )?;
        let state = session.wait_state(WAIT, 10).map_err(|e| e.to_string());
        let console = session.console().to_owned();
        let _ = session.disconnect();
        Ok((state?, console))
    });
    // The server ends with its only client; whatever happened, nothing of this test stays.
    let ended = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        matches!(server.try_wait(), Ok(Some(_)))
    });
    let _ = server.kill();
    let _ = server.wait();
    let (state, console) = outcome.unwrap();
    match state {
        StopState::Stopped { frames, locals, .. } => {
            assert_eq!(frames[0].line, 8);
            assert!(locals.iter().any(|v| v.name == "n"), "{locals:?}");
        }
        other => panic!("not stopped: {other:?}"),
    }
    assert!(console.contains("--accept-multiclient"), "{console}");
    assert!(ended, "the server ended when its client left");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0147 v3 AK-7: a running Go program is attached to by its process id, stops at a
/// breakpoint in its loop with the counter, and goes on running after the detach.
#[test]
fn go_process_is_attached_and_detached() {
    let Some(command) = delve() else { return };
    let dir = project("attach");
    let dir = std::fs::canonicalize(&dir).unwrap();
    let main = dir.join("main.go");
    std::fs::write(&main, LOOP).unwrap();
    let built = std::process::Command::new("go")
        .args(["build", "-gcflags=all=-N -l", "-o", "loop", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let mut program = std::process::Command::new(dir.join("loop"))
        .current_dir(&dir)
        .spawn()
        .unwrap();
    let target = Target::Pid(program.id());
    let arguments = attach_arguments(Runtime::Go, &target, &dir).unwrap();
    assert_eq!(arguments["mode"], "local");
    let breakpoints = BTreeMap::from([(main.clone(), vec![SourceBreakpoint::from(8)])]);
    let attached = attach(
        Runtime::Go,
        &target,
        &command,
        &dir,
        &breakpoints,
        &Default::default(),
    );
    let (mut session, _) = match attached {
        Ok(s) => s,
        Err(e) => {
            let _ = program.kill();
            let _ = program.wait();
            panic!("{e}");
        }
    };
    let state = session.wait_state(WAIT, 10);
    let detached = session.disconnect();
    let still_runs = matches!(program.try_wait(), Ok(None));
    let _ = program.kill();
    let _ = program.wait();
    match state.unwrap() {
        StopState::Stopped { frames, locals, .. } => {
            assert_eq!(frames[0].line, 8);
            assert!(locals.iter().any(|v| v.name == "n"), "{locals:?}");
        }
        other => panic!("not stopped: {other:?}"),
    }
    detached.unwrap();
    assert!(still_runs, "the program goes on after the detach");
    // A Delve server elsewhere is refused: its port is not protected.
    let far = Target::Address {
        host: "build.example".into(),
        port: 2345,
    };
    assert!(
        attach_arguments(Runtime::Go, &far, &dir)
            .unwrap_err()
            .contains("this machine only")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
