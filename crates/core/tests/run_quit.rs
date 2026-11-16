//! SPEC-0261 v3 AK-6: quitting ends the programs of running run configurations with their process
//! groups. Its own test binary, since ending all runs would end other tests' runs.
#![cfg(unix)]

use std::time::{Duration, Instant};

use idecraft_core::{RunEvent, end_all_runs, live_runs, load_run_configs, start_run};

fn alive(pid: i32) -> bool {
    // Signal 0 only checks that the process exists.
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

#[test]
fn ending_all_runs_ends_programs_and_their_children() {
    let root = std::env::temp_dir().join(format!("idecraft-run-quit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    let child_pid = root.join("child.pid");
    std::fs::write(
        root.join(".idecraft/run.toml"),
        format!(
            r#"
[[run]]
name = "server"
command = ["/bin/sh", "-c", "sleep 600 & echo $! > '{}'; echo started; wait"]

[[run]]
name = "ignores-term"
command = ["/bin/sh", "-c", "trap '' TERM; echo started; while true; do sleep 1; done"]

[[run]]
name = "after"
command = ["/bin/sh", "-c", "echo never"]
before = ["ignores-term"]
"#,
            child_pid.display()
        ),
    )
    .unwrap();
    let configs = load_run_configs(&root);
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    assert_eq!(
        end_all_runs(Duration::from_millis(10)),
        0,
        "nothing runs yet"
    );

    let server = start_run(&root, &configs, "server").unwrap();
    let chain = start_run(&root, &configs, "after").unwrap();
    for h in [&server, &chain] {
        assert!(matches!(h.events.recv().unwrap(), RunEvent::Started { .. }));
        assert!(matches!(h.events.recv().unwrap(), RunEvent::Output { .. }));
    }
    assert_eq!(live_runs(), 2);
    let sleeper: i32 = std::fs::read_to_string(&child_pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(sleeper));

    let at = Instant::now();
    assert_eq!(end_all_runs(Duration::from_millis(300)), 2);
    assert!(at.elapsed() < Duration::from_secs(5));
    // The runners reap what was killed in their own time; wait for that, not for a clock.
    let reaped = Instant::now() + Duration::from_secs(30);
    while live_runs() > 0 && Instant::now() < reaped {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(live_runs(), 0);
    // The program's own child (the `sleep 600` of the hand test) is gone too.
    let gone = Instant::now() + Duration::from_secs(2);
    while alive(sleeper) && Instant::now() < gone {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(sleeper), "the child in the process group still runs");
    for h in [server, chain] {
        let rest: Vec<RunEvent> = h.events.iter().collect();
        let finished: Vec<_> = rest
            .iter()
            .filter_map(|e| match e {
                RunEvent::Finished(o) => Some(o),
                _ => None,
            })
            .collect();
        assert_eq!(finished.len(), 1, "the chain's next step does not start");
        assert!(finished[0].stopped, "{:?}", finished[0]);
        assert!(
            !rest
                .iter()
                .any(|e| matches!(e, RunEvent::Output { text, .. } if text == "never"))
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
