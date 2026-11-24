//! The notifications of a project window (`SPEC-0061` v17 behaviour 64): the notes the workbench
//! showed in its status bar, newest first, kept while the window is open.
//!
//! The window records here; the Notifications tool window and the MCP tool `notifications` on
//! other threads read the same log through [`NotificationRegistry::global`], one log per project
//! root. Nothing is written to disk.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// How many notifications a project window keeps.
pub const LIMIT: usize = 100;

/// Longest text kept for one notification, in characters.
pub const TEXT_CHARS: usize = 2000;

/// What a notification reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// What did not happen and why.
    Error,
    /// An outcome or a hint.
    Info,
}

impl Kind {
    /// The name over MCP.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Error => "error",
            Kind::Info => "info",
        }
    }
}

/// One notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The note as the status bar showed it.
    pub text: String,
    /// Error or info.
    pub kind: Kind,
    /// How often it was shown in a row (1 the first time).
    pub count: u32,
    /// When it was shown last.
    pub time: SystemTime,
}

impl Notification {
    /// Seconds since the Unix epoch of [`Notification::time`].
    pub fn unix_time(&self) -> u64 {
        self.time
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// The notifications of one project window, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    entries: VecDeque<Notification>,
}

impl Log {
    /// Adds `text` as shown at `time`; the same text as the newest entry counts as a repeat.
    /// Blank texts are not kept. Returns whether the log changed.
    pub fn record(&mut self, text: &str, kind: Kind, time: SystemTime) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        let text: String = text.chars().take(TEXT_CHARS).collect();
        if let Some(newest) = self.entries.front_mut()
            && newest.text == text
        {
            newest.count = newest.count.saturating_add(1);
            newest.time = time;
            newest.kind = kind;
            return true;
        }
        self.entries.push_front(Notification {
            text,
            kind,
            count: 1,
            time,
        });
        self.entries.truncate(LIMIT);
        true
    }

    /// Every notification, newest first.
    pub fn list(&self) -> Vec<Notification> {
        self.entries.iter().cloned().collect()
    }

    /// How many there are.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Removes every notification.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// The logs of the project windows of this process, by project root. Cloning shares them.
#[derive(Clone, Default)]
pub struct NotificationRegistry(Arc<Mutex<HashMap<PathBuf, Log>>>);

impl NotificationRegistry {
    /// The registry of this process, which the windows record to.
    pub fn global() -> &'static NotificationRegistry {
        static GLOBAL: OnceLock<NotificationRegistry> = OnceLock::new();
        GLOBAL.get_or_init(NotificationRegistry::default)
    }

    fn key(root: &Path) -> PathBuf {
        root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
    }

    fn logs(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Log>> {
        // A panic while holding the lock leaves plain data behind; go on with it.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records `text` for the window of `root` (see [`Log::record`]).
    pub fn record(&self, root: &Path, text: &str, kind: Kind) -> bool {
        self.logs()
            .entry(Self::key(root))
            .or_default()
            .record(text, kind, SystemTime::now())
    }

    /// The notifications of `root`'s window, newest first; none without a window.
    pub fn list(&self, root: &Path) -> Vec<Notification> {
        self.logs()
            .get(&Self::key(root))
            .map(Log::list)
            .unwrap_or_default()
    }

    /// Empties the log of `root`'s window.
    pub fn clear(&self, root: &Path) {
        if let Some(log) = self.logs().get_mut(&Self::key(root)) {
            log.clear();
        }
    }

    /// Drops the log of `root`'s window when the window goes.
    pub fn forget(&self, root: &Path) {
        self.logs().remove(&Self::key(root));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// SPEC-0061 AK-v17b: newest first, a repeat of the newest counts, blank notes are not kept.
    #[test]
    fn repeats_count_and_the_newest_comes_first() {
        let mut log = Log::default();
        assert!(log.record("Saved", Kind::Info, at(10)));
        assert!(log.record("  Saved ", Kind::Info, at(20)));
        assert!(!log.record("   ", Kind::Info, at(25)));
        assert!(log.record("Push failed: rejected", Kind::Error, at(30)));
        assert!(log.record("Saved", Kind::Info, at(40)));
        let list = log.list();
        assert_eq!(list.len(), 3);
        assert_eq!(
            (list[0].text.as_str(), list[0].count, list[0].unix_time()),
            ("Saved", 1, 40)
        );
        assert_eq!(
            (list[1].text.as_str(), list[1].kind, list[1].kind.name()),
            ("Push failed: rejected", Kind::Error, "error")
        );
        assert_eq!(
            (list[2].count, list[2].unix_time(), list[2].kind.name()),
            (2, 20, "info")
        );
    }

    /// SPEC-0061 AK-v17b: the newest 100 are kept, long texts are cut, clearing empties.
    #[test]
    fn the_newest_hundred_are_kept_and_clear_empties() {
        let mut log = Log::default();
        for i in 0..150 {
            log.record(&format!("note {i}"), Kind::Info, at(i));
        }
        assert_eq!(log.len(), LIMIT);
        assert_eq!(log.list()[0].text, "note 149");
        assert_eq!(log.list()[LIMIT - 1].text, "note 50");
        log.record(&"x".repeat(TEXT_CHARS + 50), Kind::Info, at(200));
        assert_eq!(log.list()[0].text.chars().count(), TEXT_CHARS);
        log.clear();
        assert!(log.is_empty());
    }

    /// SPEC-0061 AK-v17b: one log per project window; clearing or forgetting one leaves the
    /// others.
    #[test]
    fn one_log_per_project_window() {
        let registry = NotificationRegistry::default();
        let a = std::env::temp_dir().join(format!("ferris-idea-notes-a-{}", std::process::id()));
        let b = std::env::temp_dir().join(format!("ferris-idea-notes-b-{}", std::process::id()));
        assert!(registry.record(&a, "Saved", Kind::Info));
        assert!(registry.record(&b, "Cannot push", Kind::Error));
        assert_eq!(registry.list(&a).len(), 1);
        assert_eq!(registry.list(&b)[0].kind, Kind::Error);
        registry.clear(&a);
        assert!(registry.list(&a).is_empty());
        assert_eq!(registry.list(&b).len(), 1);
        registry.forget(&b);
        assert!(registry.list(&b).is_empty());
        let none = std::env::temp_dir().join("ferris-idea-notes-none");
        assert!(registry.list(&none).is_empty());
    }
}
