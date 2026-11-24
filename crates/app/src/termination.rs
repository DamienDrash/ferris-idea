//! Signals that ask the window process to end (`SIGTERM`, `SIGINT`, `SIGHUP`) take the normal quit
//! path instead of ending it on the spot (`SPEC-0262` behaviour 3): they set a flag the window polls.
//! The thread that receives them also sets the deadline, so the process ends within 5 s even when
//! the window's thread is blocked and never sees the flag (v3).

use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Catches the signals from now on; a thread waits for them, sets [`requested`] and arms the
/// signal deadline (`SPEC-0262` v3 behaviour 3).
pub fn catch() -> std::io::Result<()> {
    catch_with(true)
}

/// [`catch`]; `deadline` false only reports, for the test that sends the signal to itself.
fn catch_with(deadline: bool) -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?;
    // Registered before returning, so a signal right after this call is caught.
    let (mut term, mut int, mut hup) = runtime.block_on(async {
        Ok::<_, std::io::Error>((
            signal(SignalKind::terminate())?,
            signal(SignalKind::interrupt())?,
            signal(SignalKind::hangup())?,
        ))
    })?;
    std::thread::Builder::new()
        .name("termination".into())
        .spawn(move || {
            runtime.block_on(async {
                loop {
                    tokio::select! {
                        _ = term.recv() => {}
                        _ = int.recv() => {}
                        _ = hup.recv() => {}
                    }
                    REQUESTED.store(true, Ordering::SeqCst);
                    if deadline {
                        ferris_idea_core::arm_signal_deadline();
                    }
                }
            });
        })?;
    Ok(())
}

/// Whether a caught signal asked the process to end.
pub fn requested() -> bool {
    REQUESTED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigterm_is_reported_instead_of_ending_the_process() {
        catch_with(false).unwrap();
        assert!(!requested());
        let sent = std::process::Command::new("kill")
            .args(["-TERM", &std::process::id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
        let start = std::time::Instant::now();
        while !requested() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Set in the child of [`a_signal_ends_a_process_whose_window_thread_is_blocked`].
    const CHILD: &str = "FERRIS_IDEA_TERMINATION_TEST_CHILD";

    /// SPEC-0262 AK-5 (v3, behaviour 3): a process that caught the signals and whose main thread
    /// then blocks (as a stalled window thread that never polls the flag) has ended with status 0
    /// within 5 s of `SIGTERM`.
    #[test]
    fn a_signal_ends_a_process_whose_window_thread_is_blocked() {
        use std::io::BufRead;
        if std::env::var_os(CHILD).is_some() {
            catch().unwrap();
            println!("ready");
            loop {
                std::thread::park();
            }
        }
        let name = concat!(
            module_path!(),
            "::a_signal_ends_a_process_whose_window_thread_is_blocked"
        );
        let name = name.split_once("::").map_or(name, |(_, rest)| rest);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        while !line.contains("ready") {
            line.clear();
            assert!(
                out.read_line(&mut line).unwrap() > 0,
                "the child ended before it was ready"
            );
        }
        let sent = std::process::Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
        let start = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > std::time::Duration::from_secs(5) {
                let _ = child.kill();
                panic!("still running 5 s after SIGTERM");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(status.success(), "{status:?}");
    }
}
