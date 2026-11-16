//! Signals that ask the window process to end (`SIGTERM`, `SIGINT`, `SIGHUP`) take the normal quit
//! path instead of ending it on the spot (`SPEC-0262` behaviour 3): they set a flag the window polls.

use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Catches the signals from now on; a thread waits for them and sets [`requested`].
pub fn catch() -> std::io::Result<()> {
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
        catch().unwrap();
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
}
