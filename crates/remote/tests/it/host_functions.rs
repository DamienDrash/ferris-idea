//! SPEC-0318 v2 AK-13: the window's side of host terminals and host runs against a backend in this
//! process with the host functions bound — a host terminal echoes input, reattaches after a break
//! at its offset without repeating output and keeps what was typed during the break; a host run
//! handle reports start, both streams, the test report and the end as a local run handle does,
//! stops, and reattaches after a break without losing or repeating output.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use idecraft_core::RunEvent;
use idecraft_remote::inprocess::{HostSwitch, InProcessProject, Setup};
use idecraft_remote::{
    ConnState, HostTerminal, LiveProject, RunTarget, TermOutput, TermPhase, Timing, start_host_run,
};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-hostfn-{tag}-{}", std::process::id()));
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
        std::thread::sleep(Duration::from_millis(10));
    }
    f()
}

/// Reads the terminal until its text contains `needle`; returns the end if it came.
fn read_until(
    term: &HostTerminal,
    text: &mut String,
    needle: &str,
    limit: Duration,
) -> Option<(Option<i32>, String)> {
    let t0 = Instant::now();
    while t0.elapsed() < limit && !text.contains(needle) {
        match term.read(Duration::from_millis(50)) {
            Some(TermOutput::Data(b)) => text.push_str(&String::from_utf8_lossy(&b)),
            Some(TermOutput::Redraw) => text.push_str("<redraw>"),
            Some(TermOutput::Ended(code, note)) => return Some((code, note)),
            None => {}
        }
    }
    None
}

fn break_for(live: &LiveProject, switch: &HostSwitch, away: Duration) {
    switch.set_reachable(false);
    live.break_connection();
    assert!(wait_until(Duration::from_secs(5), || matches!(
        live.state(),
        ConnState::Reconnecting { .. }
    )));
    std::thread::sleep(away);
    switch.set_reachable(true);
}

#[test]
fn a_host_terminal_reattaches_at_its_offset_and_keeps_typing_from_the_break() {
    let dir = scratch("term");
    let (live, switch) = open(&dir);
    let term = HostTerminal::open(live.clone(), ".", 80, 24);
    let mut text = String::new();
    term.write(b"echo one-$((40+2))\n");
    assert_eq!(
        read_until(&term, &mut text, "one-42\r\n", Duration::from_secs(10)),
        None
    );
    assert!(text.contains("one-42"), "{text}");
    assert_eq!(term.phase(), TermPhase::Running);
    assert!(term.id().is_some_and(|t| t.starts_with('t')));

    // A break: what is typed meanwhile is kept and sent after the resume.
    switch.set_reachable(false);
    live.break_connection();
    assert!(wait_until(Duration::from_secs(5), || term.phase()
        == TermPhase::Reconnecting));
    term.write(b"echo two-$((40+3))\n");
    std::thread::sleep(Duration::from_millis(300));
    switch.set_reachable(true);
    assert_eq!(
        read_until(&term, &mut text, "two-43\r\n", Duration::from_secs(20)),
        None
    );
    assert!(text.contains("two-43"), "{text}");
    assert_eq!(
        text.matches("one-42\r\n").count(),
        1,
        "nothing is repeated after the reattach: {text}"
    );
    assert!(!text.contains("<redraw>"), "no output was lost: {text}");
    assert_eq!(term.phase(), TermPhase::Running);

    term.write(b"exit 3\n");
    let end = read_until(&term, &mut text, "\u{0}never", Duration::from_secs(10));
    assert_eq!(end.map(|e| e.0), Some(Some(3)), "{text}");
    assert_eq!(term.phase(), TermPhase::Ended);
    let _ = live.close(idecraft_remote::Unsaved::Discard);
}

#[test]
fn closing_a_host_terminal_ends_its_shell_on_the_host() {
    let dir = scratch("term-close");
    let (live, _) = open(&dir);
    let term = HostTerminal::open(live.clone(), ".", 80, 24);
    let mut text = String::new();
    term.write(b"echo ready-$((1+1))\n");
    read_until(&term, &mut text, "ready-2\r\n", Duration::from_secs(10));
    let id = term.id().unwrap();
    drop(term);
    assert!(wait_until(Duration::from_secs(10), || {
        live.host_call("term.list", serde_json::json!({}), Duration::from_secs(5))
            .ok()
            .is_some_and(|v| {
                !v["terminals"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["term"] == id.as_str())
            })
    }));
    let _ = live.close(idecraft_remote::Unsaved::Discard);
}

/// The events of a run until its handle's channel closes.
fn events_of(handle: &idecraft_core::RunHandle, limit: Duration) -> Vec<RunEvent> {
    let t0 = Instant::now();
    let mut out = Vec::new();
    while t0.elapsed() < limit {
        match handle.events.recv_timeout(Duration::from_millis(50)) {
            Ok(e) => out.push(e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return out,
        }
    }
    panic!("the run did not end: {out:?}");
}

fn lines(events: &[RunEvent], stderr: bool) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Output { stderr: s, text } if *s == stderr => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_host_run_reports_like_a_local_run_stops_and_reattaches_after_a_break() {
    let dir = scratch("run");
    std::fs::create_dir_all(dir.join("project/.idecraft")).unwrap();
    std::fs::write(
        dir.join("project/.idecraft/run.toml"),
        r#"[[run]]
name = "hello"
command = ["sh", "-c", "echo to-out; echo to-err >&2; exit 4"]

[[run]]
name = "tests"
kind = "test"
report = "tap"
command = ["sh", "-c", "echo 1..2; echo ok 1 - first; echo not ok 2 - second"]

[[run]]
name = "ticks"
command = ["sh", "-c", "i=0; while [ $i -lt 30 ]; do echo tick-$i; i=$((i+1)); sleep 0.05; done"]

[[run]]
name = "slow"
command = ["sh", "-c", "echo started; sleep 30"]
"#,
    )
    .unwrap();
    let (live, switch) = open(&dir);

    let run = start_host_run(&live, &RunTarget::Name("hello".into())).unwrap();
    assert_eq!(run.name, "hello");
    let events = events_of(&run.handle, Duration::from_secs(20));
    assert!(
        matches!(events.first(), Some(RunEvent::Started { name, .. }) if name == "hello"),
        "{events:?}"
    );
    assert_eq!(lines(&events, false), ["to-out"]);
    assert_eq!(lines(&events, true), ["to-err"]);
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Finished(o) => Some(o.exit_code),
            _ => None,
        })
        .collect();
    assert_eq!(finished, [Some(4)], "{events:?}");

    let run = start_host_run(&live, &RunTarget::Name("tests".into())).unwrap();
    let events = events_of(&run.handle, Duration::from_secs(20));
    let report = events.iter().find_map(|e| match e {
        RunEvent::Finished(o) => o.report.clone(),
        _ => None,
    });
    let report = report.expect("the test report arrives with the end");
    assert_eq!((report.totals.passed, report.totals.failed), (1, 1));

    // A break in the middle of a run: the output goes on without a gap or a repeat.
    let run = start_host_run(&live, &RunTarget::Name("ticks".into())).unwrap();
    let mut seen = Vec::new();
    let t0 = Instant::now();
    while !seen
        .iter()
        .any(|e| matches!(e, RunEvent::Output { text, .. } if text == "tick-3"))
    {
        assert!(t0.elapsed() < Duration::from_secs(10), "{seen:?}");
        if let Ok(e) = run.handle.events.recv_timeout(Duration::from_millis(50)) {
            seen.push(e);
        }
    }
    break_for(&live, &switch, Duration::from_millis(400));
    seen.extend(events_of(&run.handle, Duration::from_secs(30)));
    let ticks = lines(&seen, false);
    let wanted: Vec<String> = (0..30).map(|i| format!("tick-{i}")).collect();
    assert_eq!(ticks, wanted, "every line once, in order");

    // Stop ends the run on the host.
    let run = start_host_run(&live, &RunTarget::Name("slow".into())).unwrap();
    let t0 = Instant::now();
    while !matches!(
        run.handle.events.recv_timeout(Duration::from_millis(50)),
        Ok(RunEvent::Output { .. })
    ) {
        assert!(t0.elapsed() < Duration::from_secs(10));
    }
    run.handle.stop();
    let events = events_of(&run.handle, Duration::from_secs(15));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, RunEvent::Finished(o) if o.stopped)),
        "{events:?}"
    );

    // A missing configuration is refused by the host.
    assert!(start_host_run(&live, &RunTarget::Name("missing".into())).is_err());
    let _ = live.close(idecraft_remote::Unsaved::Discard);
}
