//! A window that cannot be opened ends the start with an error (for instance on Linux without
//! `DISPLAY` or `WAYLAND_DISPLAY`).
//!
//! On Linux, gpui calls the launch callback before its event loop runs, and the loop clears a stop
//! asked for before it starts: a `cx.quit()` straight from the callback was lost and the process
//! kept running without a window, then ended with success. The quit is therefore asked for from a
//! task, which runs inside the loop, and the failure is kept for the exit code.

use std::sync::Mutex;

use gpui::App;

static FAILURE: Mutex<Option<String>> = Mutex::new(None);

/// Records that no window could be opened and ends the app once its loop runs.
pub fn failed(error: impl std::fmt::Display, cx: &mut App) {
    record(format!("cannot open a window: {error}"));
    cx.spawn(async move |cx| {
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

fn record(message: String) {
    let mut f = FAILURE.lock().unwrap_or_else(|e| e.into_inner());
    f.get_or_insert(message);
}

/// After the app's loop: an error when a window could not be opened.
pub fn outcome() -> Result<(), Box<dyn std::error::Error>> {
    match FAILURE.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(message) => Err(message.into()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_failure_is_the_outcome_once() {
        assert!(outcome().is_ok());
        record("cannot open a window: no display".into());
        record("cannot open a window: later".into());
        let e = outcome().unwrap_err();
        assert_eq!(e.to_string(), "cannot open a window: no display");
        assert!(outcome().is_ok());
    }
}
