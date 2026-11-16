//! The undo histories of files whose last editor closed (`SPEC-0057` v8 behaviour 31): kept for
//! this session of the project window, in memory, and handed to the next editor of the file when
//! the file still holds the text the history ended with.

use std::path::{Path, PathBuf};

use idecraft_editor::TextBuffer;

/// How many closed files keep their history.
pub const MAX_CLOSED: usize = 32;

/// The kept histories, oldest first.
#[derive(Default)]
pub struct ClosedHistories {
    kept: Vec<(PathBuf, TextBuffer)>,
}

fn key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

impl ClosedHistories {
    /// Keeps `buffer` (text and history) as the history of `path`; the oldest goes when more than
    /// [`MAX_CLOSED`] are kept.
    pub fn keep(&mut self, path: &Path, buffer: TextBuffer) {
        let path = key(path);
        self.kept.retain(|(p, _)| *p != path);
        self.kept.push((path, buffer));
        if self.kept.len() > MAX_CLOSED {
            self.kept.remove(0);
        }
    }

    /// The history kept for `path`, if the file's text is still `text`; it is no longer kept
    /// either way: a history is never applied to another text.
    pub fn take(&mut self, path: &Path, text: &str) -> Option<TextBuffer> {
        let path = key(path);
        let at = self.kept.iter().position(|(p, _)| *p == path)?;
        let (_, buffer) = self.kept.remove(at);
        (buffer.text() == text).then_some(buffer)
    }

    /// How many histories are kept.
    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// Whether none is kept.
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited(text: &str, more: &str) -> TextBuffer {
        let mut b = TextBuffer::from_text(text);
        b.insert(0, more).unwrap();
        b
    }

    /// SPEC-0057 v8 AK-30 (behaviour 31): bounded, the newest kept, never onto another text.
    #[test]
    fn the_oldest_history_goes_first() {
        let mut kept = ClosedHistories::default();
        for i in 0..MAX_CLOSED + 3 {
            kept.keep(Path::new(&format!("/nowhere/f{i}.txt")), edited("x", "a"));
        }
        assert_eq!(kept.len(), MAX_CLOSED);
        assert!(kept.take(Path::new("/nowhere/f0.txt"), "ax").is_none());
        assert!(kept.take(Path::new("/nowhere/f2.txt"), "ax").is_none());
        let mut newest = kept
            .take(
                Path::new(&format!("/nowhere/f{}.txt", MAX_CLOSED + 2)),
                "ax",
            )
            .expect("the newest is kept");
        assert!(newest.undo());
        assert_eq!(newest.text(), "x");
        assert_eq!(kept.len(), MAX_CLOSED - 1, "taken once");

        // Another text on disk: the history is dropped, not applied.
        assert!(kept.take(Path::new("/nowhere/f5.txt"), "other").is_none());
        assert!(kept.take(Path::new("/nowhere/f5.txt"), "ax").is_none());
        // Kept again for the same file: one entry.
        kept.keep(Path::new("/nowhere/f6.txt"), edited("y", "b"));
        assert_eq!(kept.len(), MAX_CLOSED - 2);
        assert!(kept.take(Path::new("/nowhere/f6.txt"), "by").is_some());
        assert!(!kept.is_empty());
    }
}
