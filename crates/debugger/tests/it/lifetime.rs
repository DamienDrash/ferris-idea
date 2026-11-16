//! SPEC-0263 v2 AK-3: a process started for a debug session ends with what it started, also when
//! its owner is gone without ending it. Plain shell and `sleep` stand in for js-debug and Chrome.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use idecraft_debugger::lifetime::{ExitGuard, children, end_tree};

/// A shell that starts a `sleep` of its own and waits for it; returns the shell and the sleep's id.
fn parent_with_child() -> (Child, u32) {
    let mut shell = Command::new("/bin/sh")
        .args(["-c", "sleep 300 & echo $!; wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(shell.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    (shell, line.trim().parse().unwrap())
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn gone_within(pid: u32, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    while alive(pid) {
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

#[test]
fn ending_a_process_ends_what_it_started() {
    let (mut shell, sleeper) = parent_with_child();
    assert!(children(shell.id()).iter().any(|c| c.pid == sleeper));
    end_tree(&mut shell, Duration::from_secs(2));
    assert!(
        gone_within(sleeper, Duration::from_secs(3)),
        "{sleeper} still runs"
    );
}

#[test]
fn a_guard_ends_the_process_when_its_owner_is_gone() {
    let (mut shell, sleeper) = parent_with_child();
    let guard = ExitGuard::watch(shell.id()).expect("a guard");
    // As if IDECraft had ended without ending the process.
    guard.trip();
    let _ = shell.wait();
    assert!(
        gone_within(sleeper, Duration::from_secs(5)),
        "{sleeper} still runs"
    );

    // A guard that stands down leaves everything alone.
    let (mut shell, sleeper) = parent_with_child();
    drop(ExitGuard::watch(shell.id()).expect("a guard"));
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(shell.id()) && alive(sleeper));
    end_tree(&mut shell, Duration::from_secs(2));
    assert!(gone_within(sleeper, Duration::from_secs(3)));
}
