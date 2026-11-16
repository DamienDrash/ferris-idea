//! Without a display, the window commands end at once with an error instead of running on
//! without a window (Linux: neither `DISPLAY` nor `WAYLAND_DISPLAY`).
#![cfg(target_os = "linux")]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Runs `idecraft <args>` without a display; `None` when it is still running after `limit`.
fn without_display(args: &[&str], limit: Duration) -> Option<(i32, String)> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_idecraft"))
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("HOME", std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            use std::io::Read as _;
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut err)
                .unwrap();
            return Some((status.code().unwrap_or(-1), err));
        }
        if t0.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn window_commands_without_a_display_fail_instead_of_hanging() {
    let dir = std::env::temp_dir().join(format!("idecraft-no-display-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let folder = dir.to_str().unwrap();
    // The limit only catches the hang this test is about; the command ends in well under a second.
    for args in [vec!["ui", folder], vec!["spike-editor"]] {
        let (code, err) = without_display(&args, Duration::from_secs(60)).unwrap_or_else(|| {
            panic!(
                "`idecraft {}` kept running without a window",
                args.join(" ")
            )
        });
        assert_ne!(code, 0, "{args:?}: {err}");
        assert!(err.contains("cannot open a window"), "{args:?}: {err}");
        assert_eq!(
            err.matches("cannot open a window").count(),
            1,
            "{args:?}: {err}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
