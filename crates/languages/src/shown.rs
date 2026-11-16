//! The texts the window shows for open files with unsaved changes, while a change that can touch
//! several files is planned (`SPEC-0220` v4 behaviour 13): the language server gets each of them
//! (`didOpen`, or `didChange` with a higher version) when it serves the file, and IDECraft's own
//! planners read them in place of the disk, so every edit of the change refers to the text the
//! window shows.
//!
//! The texts hold for the future given to [`scope`] (a task-local value, so calls of other
//! agents on other tasks are not affected); outside it [`read`] reads the disk.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Shown texts by resolved path.
pub(crate) type Texts = Arc<HashMap<PathBuf, String>>;

tokio::task_local! {
    static SHOWN: Texts;
}

/// The resolved form of `path` (links and `..`), as the document registry keys it.
fn key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Runs `planning` with `texts` (file, shown text) as the files' content for the language
/// servers it asks and for IDECraft's own planners (`SPEC-0220` v4 behaviour 13).
pub async fn scope<F: Future>(texts: Vec<(PathBuf, String)>, planning: F) -> F::Output {
    let map: HashMap<PathBuf, String> = texts.into_iter().map(|(p, t)| (key(&p), t)).collect();
    SHOWN.scope(Arc::new(map), planning).await
}

/// The texts of the current [`scope`] (empty outside one).
pub(crate) fn current() -> Texts {
    SHOWN.try_with(Arc::clone).unwrap_or_default()
}

/// The shown text of `path` in the current [`scope`], if it was given.
pub fn text_of(path: &Path) -> Option<String> {
    SHOWN
        .try_with(|texts| texts.get(&key(path)).cloned())
        .ok()
        .flatten()
}

/// The content of `path` for a planner: its shown text when the current [`scope`] gives it, else
/// the disk.
pub fn read(path: &Path) -> std::io::Result<String> {
    match text_of(path) {
        Some(text) => Ok(text),
        None => std::fs::read_to_string(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0220 v4 AK-10: within a scope the given texts are read in place of the disk, found by
    /// the resolved path; outside it the disk is read.
    #[tokio::test]
    async fn planners_read_the_given_texts() {
        let root = std::env::temp_dir().join(format!("idecraft-shown-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/a.c");
        std::fs::write(&file, "disk\n").unwrap();
        let other = root.join("src/b.c");
        std::fs::write(&other, "other\n").unwrap();
        let roundabout = root.join("src/../src/a.c");
        let inside = scope(vec![(roundabout, "shown\n".into())], async {
            (read(&file).unwrap(), read(&other).unwrap(), current().len())
        })
        .await;
        assert_eq!(inside, ("shown\n".into(), "other\n".into(), 1));
        assert_eq!(read(&file).unwrap(), "disk\n");
        assert!(current().is_empty());
        assert_eq!(text_of(&file), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
