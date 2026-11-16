//! Starting a program that is still open for writing somewhere.

use std::io;
use std::process::{Child, Command, Output};
use std::time::Duration;

/// The error number of "Text file busy" (`ETXTBSY`, Linux and macOS).
const TEXT_FILE_BUSY: i32 = 26;

/// How often and how long apart a start that met "Text file busy" is tried again: up to 1 s.
const TRIES: u32 = 20;
const PAUSE: Duration = Duration::from_millis(50);

/// `run` again while it fails with "Text file busy": the program is still open for writing — it is
/// being updated, or another thread of this process started a program between writing and closing
/// it, and the started program keeps the file open until it replaces itself. That is no answer
/// about the program, which runs a moment later.
fn when_free<T>(mut run: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut result = run();
    for _ in 0..TRIES {
        match &result {
            Err(e) if e.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                std::thread::sleep(PAUSE);
                result = run();
            }
            _ => break,
        }
    }
    result
}

/// [`Command::spawn`], tried again while the program is busy being written.
pub fn spawn(cmd: &mut Command) -> io::Result<Child> {
    when_free(|| cmd.spawn())
}

/// [`Command::output`], tried again while the program is busy being written.
pub fn output(cmd: &mut Command) -> io::Result<Output> {
    when_free(|| cmd.output())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_start_is_tried_again_and_other_errors_are_not() {
        let mut calls = 0;
        let r = when_free(|| {
            calls += 1;
            if calls < 3 {
                Err(io::Error::from_raw_os_error(TEXT_FILE_BUSY))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(r.unwrap(), 3);
        let mut calls = 0;
        let r: io::Result<()> = when_free(|| {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert!(r.is_err());
        assert_eq!(calls, 1);
    }
}
