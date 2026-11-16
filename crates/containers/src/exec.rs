//! The bridge between a terminal and an interactive exec (`SPEC-0240` behaviour 2): the IDECraft
//! binary runs it in a terminal tab as `idecraft container-exec <engine> <container>`.

use std::io::{Read, Write};
use std::time::Duration;

use crate::{DockerError, DockerHost, SHELL_CHOOSER};

/// How often the terminal size is checked.
const RESIZE_EVERY: Duration = Duration::from_millis(250);

/// The command an interactive shell runs: `bash` when present, else `sh`.
pub fn shell_command() -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), SHELL_CHOOSER.into()]
}

/// Copies `input` to the shell and the shell's output to `output` until the shell ends, sending
/// the size `size` reports whenever it changes; returns the shell's exit code.
pub fn bridge(
    host: &DockerHost,
    container: &str,
    input: impl Read + Send + 'static,
    mut output: impl Write,
    size: impl Fn() -> Option<(u16, u16)> + Send + 'static,
) -> Result<i64, DockerError> {
    let (rows, cols) = size().unwrap_or((24, 80));
    let session = host.exec_attach(container, &shell_command(), rows, cols)?;
    let exec = session.id.clone();
    let (mut from_shell, mut to_shell) = session.split();
    let mut input = input;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = input.read(&mut buf) {
            if n == 0 || to_shell.write_all(&buf[..n]).is_err() {
                break;
            }
            let _ = to_shell.flush();
        }
    });
    let resizer = host.clone();
    let resize_exec = exec.clone();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop = done.clone();
    std::thread::spawn(move || {
        let mut last = Some((rows, cols));
        while !stop.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::sleep(RESIZE_EVERY);
            let now = size();
            if now.is_some() && now != last {
                last = now;
                if let Some((r, c)) = now {
                    let _ = resizer.exec_resize(&resize_exec, r, c);
                }
            }
        }
    });
    let mut buf = [0u8; 8192];
    loop {
        match from_shell.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if output.write_all(&buf[..n]).is_err() {
                    break;
                }
                let _ = output.flush();
            }
        }
    }
    done.store(true, std::sync::atomic::Ordering::Release);
    // The daemon records the exit code shortly after the stream ends.
    for _ in 0..20 {
        if let Some(code) = host.exec_exit_code(&exec)? {
            return Ok(code);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(-1)
}

/// Raw mode for the terminal on standard input, restored when dropped.
#[cfg(unix)]
struct RawMode(Option<rustix::termios::Termios>);

#[cfg(unix)]
impl RawMode {
    fn enable() -> Self {
        use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};
        let stdin = std::io::stdin();
        let Ok(saved) = tcgetattr(&stdin) else {
            return Self(None);
        };
        let mut raw = saved.clone();
        raw.make_raw();
        match tcsetattr(&stdin, OptionalActions::Now, &raw) {
            Ok(()) => Self(Some(saved)),
            Err(_) => Self(None),
        }
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(t) = &self.0 {
            let _ = rustix::termios::tcsetattr(
                std::io::stdin(),
                rustix::termios::OptionalActions::Now,
                t,
            );
        }
    }
}

/// Size of the terminal on standard input as rows and columns.
#[cfg(unix)]
fn terminal_size() -> Option<(u16, u16)> {
    let w = rustix::termios::tcgetwinsize(std::io::stdin()).ok()?;
    (w.ws_row > 0).then_some((w.ws_row, w.ws_col))
}

/// Runs an interactive shell in `container` on this process's terminal and returns the exit code
/// to end with (`idecraft container-exec`).
#[cfg(unix)]
pub fn run_in_terminal(host: &DockerHost, container: &str) -> i32 {
    let raw = RawMode::enable();
    let result = bridge(
        host,
        container,
        std::io::stdin(),
        std::io::stdout(),
        terminal_size,
    );
    drop(raw);
    match result {
        Ok(code) => i32::try_from(code).unwrap_or(1),
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_prefers_bash() {
        let c = shell_command();
        assert_eq!(c[..2], ["/bin/sh", "-c"]);
        assert!(c[2].contains("command -v bash") && c[2].ends_with("else exec sh; fi"));
    }
}
