//! SPEC-0091 v12 AK-17 (Python half): the further debugger actions with real debugpy — Step Out
//! of Code Block, the thread dump, code fragments and the console's `repl` input, referring
//! objects, Resume All, and the refusals of Force Return, Throw Exception, Reset Frame and hot
//! swap. Skipped without debugpy.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use idecraft_debugger::{DebugSession, Lang, SourceBreakpoint, StopState, known_adapters};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(60);

const APP: &str = r#"import threading, time

def worker():
    time.sleep(30)

def main():
    t = threading.Thread(target=worker, name="worker-1", daemon=True)
    t.start()
    items = [1, 2, 3]
    n = 255
    for i in range(3):
        n = n + i
        items.append(n)
    print("after loop", n)
    print("done", len(items))

main()
"#;

fn debugpy() -> Option<Vec<String>> {
    known_adapters()
        .into_iter()
        .find(|a| a.name == "debugpy")
        .and_then(|a| a.command)
}

fn stopped_at(state: &StopState) -> (i64, i64) {
    match state {
        StopState::Stopped {
            thread_id, frames, ..
        } => (*thread_id, frames[0].line),
        other => panic!("not stopped: {other:?}"),
    }
}

#[test]
fn further_actions_with_debugpy() {
    let Some(adapter) = debugpy() else {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: debugpy"
        );
        eprintln!("skipped: debugpy is not installed");
        return;
    };
    let dir = std::env::temp_dir().join(format!("idecraft-pymore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir: PathBuf = dir.canonicalize().unwrap();
    let script = dir.join("app.py");
    std::fs::write(&script, APP).unwrap();
    let launch = json!({
        "type": "python", "request": "launch", "program": script, "cwd": dir,
        "console": "internalConsole", "justMyCode": true, "stopOnEntry": false,
    });
    // Line 12 `n = n + i` inside the loop.
    let breakpoints = BTreeMap::from([(script.clone(), vec![SourceBreakpoint::from(12)])]);
    let (mut s, _) = DebugSession::launch(
        &adapter,
        "debugpy",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(s.lang(), Lang::Python);
    let (thread, line) = stopped_at(&s.wait_state(WAIT, 20).unwrap());
    assert_eq!(line, 12);
    let frame = s.stack_trace(thread).unwrap()[0].id;

    // Behaviour 36: a fragment's statements run, the last line gives the value.
    let value = s
        .evaluate_fragment("total = 0\nfor v in items:\n    total += v\ntotal", frame)
        .unwrap();
    assert_eq!(value, "6");
    // Behaviour 38: the console's input runs statements in the `repl` context; printed text
    // reaches the console as the program's output.
    s.evaluate_value("print('from repl', n)", frame, "repl")
        .unwrap();
    assert_eq!(
        s.evaluate_value("n * 2", frame, "repl").unwrap().value,
        "510"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        s.transcript().contains("from repl 255"),
        "{}",
        s.transcript()
    );

    // Behaviour 31: referring objects of the list (the frame's locals refer to it).
    let referring = s.referring_objects("items", frame).unwrap();
    assert!(referring.reference != 0, "{referring:?}");

    // Behaviour 26/28: the dump lists both threads with frames, without states (Python has none).
    let dump = s.thread_dump(Some(frame), 5).unwrap();
    let names: Vec<&str> = dump.iter().map(|(t, _)| t.name.as_str()).collect();
    assert!(
        names.contains(&"MainThread") && names.contains(&"worker-1"),
        "{names:?}"
    );
    let (main, frames) = dump.iter().find(|(t, _)| t.name == "MainThread").unwrap();
    assert!(main.suspended && main.state.is_empty());
    assert_eq!(frames[0].name, "main");

    // Behaviour 29 and 19: refused with a reason.
    for refused in [
        s.force_return(frame).unwrap_err().to_string(),
        s.throw_exception(frame, "ValueError()")
            .unwrap_err()
            .to_string(),
        s.restart_frame(frame).unwrap_err().to_string(),
        s.build_changed(std::slice::from_ref(&script), Duration::from_secs(1))
            .unwrap_err()
            .to_string(),
    ] {
        assert!(refused.contains("cannot"), "{refused}");
    }

    // Behaviour 25: Step Out of Code Block leaves the loop body and its header for the print. A
    // breakpoint in the block would stop it in the next pass, so it goes first.
    s.set_breakpoints(&script, &[]).unwrap();
    s.step_out_of_block(thread).unwrap();
    let (_, line) = stopped_at(&s.stopped_state("step", thread, 20).unwrap());
    assert_eq!(line, 14, "the first line after the loop");
    let frame = s.stack_trace(thread).unwrap()[0].id;
    assert_eq!(s.evaluate("n", frame).unwrap(), "258");

    // Behaviour 27: Resume All runs the program to its end.
    s.resume_all(thread).unwrap();
    match s.wait_state(WAIT, 20).unwrap() {
        StopState::Exited { .. } => {}
        other => panic!("not ended: {other:?}"),
    }
    assert!(s.output().contains("done 6"), "{}", s.output());
    let _ = s.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}
