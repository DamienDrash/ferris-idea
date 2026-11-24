//! SPEC-0087 v4 behaviour 5: Stop asks a program to end (SIGTERM to its process group) and kills
//! what still runs after the grace period.
#![cfg(unix)]

use std::time::{Duration, Instant};

use ferris_idea_core::{RunEvent, RunOutcome, STOP_GRACE, load_run_configs, start_run};

fn stopped_run(name: &str, script: &str) -> (RunOutcome, Duration) {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-run-stop-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    std::fs::write(
        root.join(".ferris-idea/run.toml"),
        format!("[[run]]\nname = \"p\"\ncommand = [\"/bin/sh\", \"-c\", \"{script}\"]\n"),
    )
    .unwrap();
    let configs = load_run_configs(&root);
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    let handle = start_run(&root, &configs, "p").unwrap();
    assert!(matches!(
        handle.events.recv().unwrap(),
        RunEvent::Started { .. }
    ));
    assert!(matches!(
        handle.events.recv().unwrap(),
        RunEvent::Output { .. }
    ));
    let t0 = Instant::now();
    handle
        .stopper()
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let outcome = loop {
        if let RunEvent::Finished(o) = handle.events.recv_timeout(Duration::from_secs(20)).unwrap()
        {
            break o;
        }
    };
    let _ = std::fs::remove_dir_all(root);
    (*outcome, t0.elapsed())
}

#[test]
fn a_program_that_handles_sigterm_ends_by_itself() {
    let (o, took) = stopped_run(
        "trap",
        "trap 'echo cleaned; exit 7' TERM; echo started; while true; do sleep 0.05; done",
    );
    assert!(o.stopped);
    assert_eq!(o.exit_code, Some(7));
    assert!(o.stdout.contains("cleaned"), "{}", o.stdout);
    assert!(took < STOP_GRACE, "{took:?}");
}

#[test]
fn a_program_without_a_handler_reports_the_signal() {
    let (o, took) = stopped_run("plain", "echo started; while true; do sleep 0.05; done");
    assert!(o.stopped);
    assert_eq!(o.exit_code, Some(143));
    assert!(took < STOP_GRACE, "{took:?}");
}

#[test]
fn a_program_that_ignores_sigterm_is_killed_after_the_grace_period() {
    let (o, took) = stopped_run(
        "ignore",
        "trap '' TERM; echo started; while true; do sleep 0.05; done",
    );
    assert!(o.stopped);
    assert_eq!(o.exit_code, Some(137));
    assert!(took >= STOP_GRACE, "{took:?}");
}

#[test]
fn a_child_left_behind_by_the_program_does_not_outlive_the_stop() {
    // The shell ends on SIGTERM at once; its child ignores the signal and holds the pipes.
    let (o, took) = stopped_run(
        "child",
        // "started" comes from the child after it has set its trap: printed by the shell beside
        // it, the stop could arrive before the trap and end the child at once.
        "(trap '' TERM; echo started; while true; do sleep 0.05; done) & wait",
    );
    assert!(o.stopped);
    assert!(
        took >= STOP_GRACE && took < STOP_GRACE + Duration::from_secs(5),
        "{took:?}"
    );
}

/// SPEC-0094 v4 AK-9: a run whose handle is dropped (its window went) is not ended by that; the
/// program keeps running until it is stopped or Ferris IDEA quits. So closing a window stops its runs
/// itself (behaviour 11).
#[test]
fn a_dropped_run_handle_leaves_its_program_running() {
    let root = std::env::temp_dir().join(format!("ferris-idea-run-dropped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let pid_file = root.join("pid");
    std::fs::write(
        root.join(".ferris-idea/run.toml"),
        format!(
            "[[run]]\nname = \"p\"\ncommand = [\"/bin/sh\", \"-c\", \"echo $$ > {}; echo up; exec sleep 30\"]\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let configs = load_run_configs(&root);
    let handle = start_run(&root, &configs, "p").unwrap();
    // Up once it has said so.
    while !matches!(handle.events.recv().unwrap(), RunEvent::Output { .. }) {}
    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .to_owned();
    drop(handle);
    let alive = |pid: &str| {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .status()
            .is_ok_and(|s| s.success())
    };
    std::thread::sleep(Duration::from_millis(500));
    let left_running = alive(&pid);
    // Cleaned up before asserting.
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid])
        .status();
    assert!(left_running, "the program ended with its handle");
}
