//! Registry of the files open in editors, with their unsaved text (`SPEC-0220`).
//!
//! The window publishes every change here; MCP tools on other threads read it, so an agent sees the
//! text the user sees (readiness decision A2). An agent's change of an open file reaches the window as
//! a [`DocumentEdit`], which the window applies to the editor as one undo step ([`apply_edit`],
//! `SPEC-0220` v2 behaviours 7 and 8).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::{TextBuffer, TextSnapshot};

/// An agent's change of an open file, for the window to apply (`SPEC-0220` v2 behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentEdit {
    /// The registry key of the file ([`DocumentRegistry::key`]).
    pub path: PathBuf,
    /// The revision the agent's text was read at; the window refuses the edit at another one
    /// (behaviour 8).
    pub revision: u64,
    /// The whole new text.
    pub text: String,
    /// Save after applying: the buffer had no unsaved changes when the text was read.
    pub save: bool,
}

/// An agent's change of several open files, applied all or nothing (`SPEC-0220` v3 behaviour 14),
/// and what the window keeps of it for its own Undo (`SPEC-0220` v4 behaviour 15, `SPEC-0193` v5
/// behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentChange {
    /// One edit per open file.
    pub edits: Vec<DocumentEdit>,
    /// The refactoring's name ("Rename add to sum"): the window keeps the change as an undo group
    /// under it. `None` keeps no group.
    pub name: Option<String>,
    /// The files of the change written to disk (not open), with their text before and after.
    pub on_disk: Vec<DiskChange>,
}

/// A file of a [`DocumentChange`] written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskChange {
    /// The file.
    pub path: PathBuf,
    /// Its text before the change.
    pub before: String,
    /// Its text after the change.
    pub after: String,
}

/// The state of the file after the window applied a [`DocumentEdit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentEdited {
    /// The revision after the edit.
    pub revision: u64,
    /// Whether the text still differs from the last save.
    pub modified: bool,
}

/// Turns the text of `buffer` into `new` as one undo step that replaces only what differs
/// (`SPEC-0220` v2 behaviour 7). Returns whether anything changed.
pub fn apply_edit(buffer: &mut TextBuffer, new: &str) -> bool {
    let old = buffer.text();
    let edits = crate::reformat::reformat_edits(&old, new);
    if edits.is_empty() {
        return false;
    }
    buffer.begin_group();
    // From the end, so the char ranges of `old` stay valid.
    for (range, text) in edits.iter().rev() {
        let _ = buffer.replace(range.clone(), text);
    }
    buffer.end_group();
    true
}

/// One open file as a reader sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The path, resolved through links.
    pub path: PathBuf,
    /// Current text, saved or not.
    pub text: TextSnapshot,
    /// Starts at 1 and grows with every change of the text (behaviour 4).
    pub revision: u64,
    /// Whether the text differs from the last save.
    pub modified: bool,
}

struct Entry {
    text: TextSnapshot,
    revision: u64,
    modified: bool,
    /// Editor views showing the file; the entry goes with the last one.
    views: usize,
}

/// Open files by resolved path. Cloning shares the registry.
#[derive(Clone, Default)]
pub struct DocumentRegistry(Arc<Mutex<HashMap<PathBuf, Entry>>>);

impl DocumentRegistry {
    /// The registry of this process, which the window publishes to.
    pub fn global() -> &'static DocumentRegistry {
        static GLOBAL: OnceLock<DocumentRegistry> = OnceLock::new();
        GLOBAL.get_or_init(DocumentRegistry::default)
    }

    /// The key of `path`: resolved through links and `..` when it exists.
    pub fn key(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Entry>> {
        // A panic while holding the lock leaves plain data behind; go on with it.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A view of the file at `key` opened with `text` (behaviour 2).
    pub fn open(&self, key: &Path, text: TextSnapshot, modified: bool) {
        let mut entries = self.entries();
        match entries.get_mut(key) {
            Some(e) => {
                e.views += 1;
                if e.text != text {
                    e.text = text;
                    e.revision += 1;
                }
                e.modified = modified;
            }
            None => {
                entries.insert(
                    key.to_path_buf(),
                    Entry {
                        text,
                        revision: 1,
                        modified,
                        views: 1,
                    },
                );
            }
        }
    }

    /// The text of an open file changed or was saved; unknown files are ignored.
    pub fn publish(&self, key: &Path, text: TextSnapshot, modified: bool) {
        if let Some(e) = self.entries().get_mut(key) {
            if e.text != text {
                e.text = text;
                e.revision += 1;
            }
            e.modified = modified;
        }
    }

    /// A view of the file at `key` went away.
    pub fn close(&self, key: &Path) {
        let mut entries = self.entries();
        if let Some(e) = entries.get_mut(key) {
            e.views -= 1;
            if e.views == 0 {
                entries.remove(key);
            }
        }
    }

    /// The open file at `path`, if any.
    pub fn get(&self, path: &Path) -> Option<Document> {
        let key = Self::key(path);
        self.entries().get(&key).map(|e| Document {
            path: key.clone(),
            text: e.text.clone(),
            revision: e.revision,
            modified: e.modified,
        })
    }

    /// Whether `edit` still applies (`SPEC-0220` v2 behaviour 8): the file is open and still at
    /// the revision the agent read; the window checks this on its own thread before applying it.
    pub fn check_edit(&self, edit: &DocumentEdit) -> Result<Document, String> {
        let doc = self
            .get(&edit.path)
            .ok_or("the file is no longer open in an editor")?;
        if doc.revision != edit.revision {
            return Err(format!(
                "the file changed in the editor meanwhile (revision {} instead of {}); read it again",
                doc.revision, edit.revision
            ));
        }
        Ok(doc)
    }

    /// Every open file, sorted by path.
    pub fn list(&self) -> Vec<Document> {
        let mut out: Vec<Document> = self
            .entries()
            .iter()
            .map(|(k, e)| Document {
                path: k.clone(),
                text: e.text.clone(),
                revision: e.revision,
                modified: e.modified,
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }
}

/// One view's registration of a file: publishes to the registry and unregisters when dropped.
pub struct OpenDocument {
    registry: DocumentRegistry,
    key: PathBuf,
}

impl OpenDocument {
    /// Registers a view of `path` showing `text`.
    pub fn open(registry: &DocumentRegistry, path: &Path, text: TextSnapshot) -> Self {
        let key = DocumentRegistry::key(path);
        registry.open(&key, text, false);
        Self {
            registry: registry.clone(),
            key,
        }
    }

    /// Publishes the view's text and saved state.
    pub fn publish(&self, text: TextSnapshot, modified: bool) {
        self.registry.publish(&self.key, text, modified);
    }
}

impl Drop for OpenDocument {
    fn drop(&mut self) {
        self.registry.close(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0220 v2 AK-4: an agent's edit of a buffer with unsaved changes keeps them and is one
    /// undo step.
    #[test]
    fn an_agent_edit_is_one_undo_step() {
        let mut buf = TextBuffer::from_text("one\ntwo\nthree\n");
        buf.insert(0, "user ").unwrap();
        let typed = buf.text();
        assert!(buf.is_modified());
        let agent = typed.replace("three", "3") + "four\n";
        assert!(apply_edit(&mut buf, &agent));
        assert_eq!(buf.text(), agent);
        assert!(buf.undo());
        assert_eq!(buf.text(), typed);
        // The same text again changes nothing and makes no step.
        assert!(!apply_edit(&mut buf, &typed));
    }

    /// SPEC-0220 v2 AK-4: an edit at a revision the file has left is refused, as is one of a
    /// file no longer open.
    #[test]
    fn a_stale_edit_is_refused() {
        let reg = DocumentRegistry::default();
        let key = PathBuf::from("/nowhere/ferris-idea-stale.txt");
        reg.open(&key, TextSnapshot::from_text("a"), false);
        let edit = DocumentEdit {
            path: key.clone(),
            revision: 1,
            text: "b".into(),
            save: true,
        };
        assert_eq!(reg.check_edit(&edit).unwrap().revision, 1);
        reg.publish(&key, TextSnapshot::from_text("a typed"), true);
        assert!(reg.check_edit(&edit).unwrap_err().contains("meanwhile"));
        reg.close(&key);
        assert!(
            reg.check_edit(&edit)
                .unwrap_err()
                .contains("no longer open")
        );
    }

    #[test]
    fn revisions_modified_flag_and_shared_views() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-docs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "disk").unwrap();
        let reg = DocumentRegistry::default();
        let key = DocumentRegistry::key(&dir.join("a.txt"));
        let mut buf = TextBuffer::from_text("disk");
        reg.open(&key, buf.snapshot(), false);
        // `..` and the plain path name the same entry.
        let doc = reg.get(&dir.join("sub/../a.txt")).unwrap();
        assert_eq!((doc.revision, doc.modified), (1, false));

        buf.insert(4, " typed").unwrap();
        reg.publish(&key, buf.snapshot(), buf.is_modified());
        let doc = reg.get(&key).unwrap();
        assert_eq!(doc.text.to_string(), "disk typed");
        assert_eq!((doc.revision, doc.modified), (2, true));

        // A save without a change of text only clears the flag.
        buf.mark_saved();
        reg.publish(&key, buf.snapshot(), buf.is_modified());
        let doc = reg.get(&key).unwrap();
        assert_eq!((doc.revision, doc.modified), (2, false));

        // A second view shares the entry, which stays until both are closed.
        reg.open(&key, buf.snapshot(), false);
        reg.close(&key);
        assert!(reg.get(&key).is_some());
        assert_eq!(reg.list().len(), 1);
        reg.close(&key);
        assert!(reg.get(&key).is_none());
        // Unknown files are ignored.
        reg.publish(&key, buf.snapshot(), true);
        reg.close(&key);
        assert!(reg.list().is_empty());
        // A view's registration goes when the view goes.
        let view = OpenDocument::open(&reg, &dir.join("a.txt"), buf.snapshot());
        view.publish(TextSnapshot::from_text("x"), true);
        assert_eq!(reg.get(&key).unwrap().text.to_string(), "x");
        drop(view);
        assert!(reg.get(&key).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
