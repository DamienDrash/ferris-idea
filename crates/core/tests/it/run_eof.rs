//! SPEC-0087 v10 behaviour 18: a run started for the Run tool window keeps its program's standard
//! input open; `RunHandle::send_eof` closes it, and a program that reads its input to the end
//! goes on. The blocking run and a run on a remote project's host read nothing, as before.
#![cfg(unix)]

use std::time::Duration;

use ferris_idea_core::{
    RunEvent, RunOutcome, load_run_configs, run_config, start_host_run, start_run,
};

fn workspace(tag: &str, script: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-run-eof-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    std::fs::write(
        root.join(".ferris-idea/run.toml"),
        format!("[[run]]\nname = \"p\"\ncommand = [\"/bin/sh\", \"-c\", \"{script}\"]\n"),
    )
    .unwrap();
    root
}

fn finished(events: &std::sync::mpsc::Receiver<RunEvent>) -> RunOutcome {
    loop {
        if let RunEvent::Finished(o) = events.recv_timeout(Duration::from_secs(20)).unwrap() {
            return *o;
        }
    }
}

/// AK-12: the program waits for its input; the end of input lets it finish by itself.
#[test]
fn the_end_of_input_lets_a_reading_program_go_on() {
    // `cat` returns when its input ends; "after" shows that the program went on and was not
    // stopped.
    let root = workspace("cat", "echo before; cat; echo after");
    let configs = load_run_configs(&root);
    let handle = start_run(&root, &configs, "p").unwrap();
    assert!(matches!(
        handle.events.recv().unwrap(),
        RunEvent::Started { .. }
    ));
    assert_eq!(
        handle.events.recv_timeout(Duration::from_secs(20)).unwrap(),
        RunEvent::Output {
            stderr: false,
            text: "before".into()
        }
    );
    // It waits: nothing more arrives while the input is open.
    assert!(
        handle
            .events
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
    assert!(handle.stdin_open());
    assert!(handle.send_eof(), "an open input was closed");
    let outcome = finished(&handle.events);
    assert_eq!(outcome.exit_code, Some(0));
    assert!(!outcome.stopped);
    assert_eq!(outcome.stdout, "before\nafter\n");
    // Sent once: there is no input left to close.
    assert!(!handle.stdin_open());
    assert!(!handle.send_eof());
    let _ = std::fs::remove_dir_all(root);
}

/// AK-12: Stop still ends a program that waits for input, and a program that never reads is
/// not held up by the open input.
#[test]
fn stop_and_programs_that_do_not_read_are_as_before() {
    let root = workspace("stop", "echo before; cat; echo after");
    let configs = load_run_configs(&root);
    let handle = start_run(&root, &configs, "p").unwrap();
    assert!(matches!(
        handle.events.recv().unwrap(),
        RunEvent::Started { .. }
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(20)).unwrap(),
        RunEvent::Output { .. }
    ));
    handle.stop();
    let outcome = finished(&handle.events);
    assert!(outcome.stopped);
    assert!(!handle.send_eof(), "the step ended, its input is closed");
    let _ = std::fs::remove_dir_all(root);

    let root = workspace("plain", "echo done");
    let configs = load_run_configs(&root);
    let handle = start_run(&root, &configs, "p").unwrap();
    let outcome = finished(&handle.events);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stdout, "done\n");
    let _ = std::fs::remove_dir_all(root);
}

/// AK-12: the blocking run (agents, `run_config`) and a run on the host of a remote project have
/// no input: a reading program sees its end at once.
#[test]
fn blocking_and_host_runs_read_nothing() {
    let root = workspace("blocking", "cat; echo after");
    let configs = load_run_configs(&root);
    let outcomes = run_config(&root, &configs, "p").unwrap();
    assert_eq!(outcomes.last().unwrap().stdout, "after\n");

    let handle = start_host_run(&root, &configs, "p").unwrap();
    assert!(!handle.stdin_open());
    let outcome = finished(&handle.events);
    assert_eq!(outcome.stdout, "after\n");
    assert!(!handle.send_eof());
    let _ = std::fs::remove_dir_all(root);
}
