//! `SPEC-0261` v4 behaviour 17 / `SPEC-0262` v2 behaviour 3: a decided quit that does not finish
//! ends the process within the deadline.

use std::process::Command;
use std::time::{Duration, Instant};

const CHILD: &str = "IDECRAFT_QUIT_DEADLINE_CHILD";

/// In the child: arms the deadline and then hangs, as a quit the platform never carries out.
#[test]
fn hanging_quit_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    assert!(idecraft_core::arm_quit_deadline());
    std::thread::sleep(Duration::from_secs(60));
    // Reaching this line means the deadline did not end the process.
    std::process::exit(3);
}

#[test]
fn a_quit_that_hangs_ends_the_process_within_the_deadline() {
    let start = Instant::now();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "quit_deadline::hanging_quit_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let took = start.elapsed();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(took >= idecraft_core::QUIT_DEADLINE, "ended after {took:?}");
    assert!(took < Duration::from_secs(20), "ended after {took:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("quitting did not finish within 5 s"),
        "{out:?}"
    );
}
