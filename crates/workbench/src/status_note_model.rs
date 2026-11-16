//! How long the workbench's own note stands in the status bar (`SPEC-0070` v13 behaviour 4d),
//! toolkit-free: a newer note replaces it at once; otherwise it goes after a fixed time, a failure
//! later than a hint or an outcome.

use std::time::Duration;

/// How long a hint or an outcome ("Saved", "Copied 3 lines") stands.
pub const NOTE_TIME: Duration = Duration::from_secs(10);
/// How long a failure stands.
pub const FAILURE_TIME: Duration = Duration::from_secs(30);

/// Whether `note` reports a failure: what did not happen and why.
pub fn is_failure(note: &str) -> bool {
    let n = note.to_lowercase();
    [
        "cannot",
        "can't",
        "could not",
        "couldn't",
        "failed",
        "failure",
        "error",
        "not found",
        "refused",
        "denied",
        "not granted",
        "invalid",
        "unknown",
        "no such",
        "nothing was",
        "not saved",
        "not allowed",
        "timed out",
        "no statement",
        "no connection",
    ]
    .iter()
    .any(|w| n.contains(w))
}

/// How long `note` stands when no newer note replaces it.
pub fn lifetime(note: &str) -> Duration {
    if is_failure(note) {
        FAILURE_TIME
    } else {
        NOTE_TIME
    }
}

/// Whether `note`, shown for `age`, is gone.
pub fn expired(note: &str, age: Duration) -> bool {
    age >= lifetime(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0070 v13 AK-12: failures stand longer than hints and outcomes; both go in the end.
    #[test]
    fn notes_go_after_a_fixed_time_failures_later() {
        for failure in [
            "No statement at the caret",
            "Cannot open /tmp/x: No such file or directory",
            "Push failed: rejected",
            "The clone timed out; nothing was kept.",
        ] {
            assert_eq!(lifetime(failure), FAILURE_TIME, "{failure}");
        }
        for note in [
            "Saved",
            "Copied 3 lines",
            "Cloned shop",
            "Committed 2 files",
        ] {
            assert_eq!(lifetime(note), NOTE_TIME, "{note}");
        }
        assert!(FAILURE_TIME > NOTE_TIME);
        assert!(!expired("Saved", Duration::from_secs(9)));
        assert!(expired("Saved", NOTE_TIME));
        assert!(!expired(
            "No statement at the caret",
            Duration::from_secs(20)
        ));
        assert!(expired("No statement at the caret", FAILURE_TIME));
    }
}
