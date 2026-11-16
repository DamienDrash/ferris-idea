//! The window's history of copied text for Paste from History (`SPEC-0057` v3 behaviour 20).
//!
//! Editors record what they copy, cut or paste from the system clipboard; the popup and the MCP
//! tool `paste_history` read it. One history per process, newest first.

use std::sync::{Arc, Mutex, OnceLock};

/// Most entries kept.
pub const LIMIT: usize = 30;

/// Longest row text in the popup, in characters.
pub const ROW_CHARS: usize = 80;

/// Copied texts, newest first. Cloning shares the history.
#[derive(Clone, Default)]
pub struct PasteHistory(Arc<Mutex<Vec<String>>>);

impl PasteHistory {
    /// The history of this process, which the window records into.
    pub fn global() -> &'static PasteHistory {
        static GLOBAL: OnceLock<PasteHistory> = OnceLock::new();
        GLOBAL.get_or_init(PasteHistory::default)
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts `text` on top; a text already present moves up; empty texts are not kept.
    pub fn record(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut entries = self.entries();
        entries.retain(|t| t != text);
        entries.insert(0, text.to_owned());
        entries.truncate(LIMIT);
    }

    /// The entries, newest first.
    pub fn list(&self) -> Vec<String> {
        self.entries().clone()
    }
}

/// The popup row of entry `index` (0-based): its number, then the text on one line with breaks
/// shown as `↵`, at most [`ROW_CHARS`] characters.
pub fn row(index: usize, text: &str) -> String {
    let flat: String = text
        .trim_end_matches(['\n', '\r'])
        .replace("\r\n", "\n")
        .split('\n')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ↵ ");
    let mut shown: String = flat.chars().take(ROW_CHARS).collect();
    if flat.chars().count() > ROW_CHARS {
        shown.push('…');
    }
    format!("{}  {shown}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0057 v3 behaviour 20 (AK-15).
    #[test]
    fn newest_first_repeats_move_up_and_the_limit_holds() {
        let h = PasteHistory::default();
        h.record("a");
        h.record("b");
        h.record("");
        h.record("a");
        assert_eq!(h.list(), ["a", "b"]);
        for i in 0..40 {
            h.record(&format!("t{i}"));
        }
        let list = h.list();
        assert_eq!(list.len(), LIMIT);
        assert_eq!(list[0], "t39");
    }

    #[test]
    fn rows_are_numbered_one_line_and_short() {
        assert_eq!(row(0, "a\n    b\n"), "1  a ↵ b");
        let long = "x".repeat(100);
        let r = row(2, &long);
        assert!(r.starts_with("3  xxx"));
        assert_eq!(r.chars().count(), 3 + ROW_CHARS + 1);
    }
}
