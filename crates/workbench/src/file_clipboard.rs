//! Cut, Copy and Paste of files and folders in the project tree (`SPEC-0275` v2 behaviours 17–21),
//! without toolkit: what the file clipboard holds, when it is stale, where Paste goes and where open
//! tabs of moved files go. The copying and moving itself is [`ferris_idea_core::file_ops::transfer`],
//! the same code as the MCP tools `copy_file` and `move_file`. Several selected items go on the
//! clipboard together (v3 behaviour 25).

use std::path::{Path, PathBuf};

use ferris_idea_core::file_ops::Transfer;

/// What Cut or Copy put on the file clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClipboard {
    /// The selected files and folders, items inside a selected folder left out.
    pub paths: Vec<PathBuf>,
    /// Copy after ⌘C, Move after ⌘X.
    pub mode: Transfer,
}

impl FileClipboard {
    /// The clipboard of the selection `paths` (nested items once).
    pub fn new(paths: &[PathBuf], mode: Transfer) -> Self {
        Self {
            paths: ferris_idea_core::file_ops::outermost(paths),
            mode,
        }
    }

    /// The text put on the system clipboard alongside: the absolute paths, one per line
    /// (behaviours 18 and 25).
    pub fn text(&self) -> String {
        paths_text(&self.paths)
    }

    /// Whether Paste still means these files: the system clipboard holds the text Cut or Copy put
    /// there. Once the user copied something else, Paste in the tree pastes nothing (behaviour 18).
    pub fn current(&self, system_text: Option<&str>) -> bool {
        system_text.is_some_and(|t| t == self.text())
    }

    /// The status line after Cut or Copy.
    pub fn status(&self) -> String {
        if let [one] = self.paths.as_slice() {
            let name = one
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return match self.mode {
                Transfer::Copy => format!("Copied {name}: Paste into a folder copies it"),
                Transfer::Move => format!("Cut {name}: Paste into a folder moves it"),
            };
        }
        let n = self.paths.len();
        match self.mode {
            Transfer::Copy => format!("Copied {n} items: Paste into a folder copies them"),
            Transfer::Move => format!("Cut {n} items: Paste into a folder moves them"),
        }
    }
}

/// Paths as text, one per line (Copy Path of several items, behaviour 25).
pub fn paths_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The status line after a paste of several items.
pub fn pasted_status(mode: Transfer, count: usize, dir: &Path, root: &Path) -> String {
    format!(
        "{} {count} items to {}",
        mode.past(),
        folder_name(dir, root)
    )
}

fn folder_name(dir: &Path, root: &Path) -> String {
    dir.strip_prefix(root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| {
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string())
        })
}

/// The folder Paste goes into: the selection when it is a folder, else its folder (behaviour 19).
pub fn paste_dir(selected: &Path, selected_is_dir: bool, root: &Path) -> PathBuf {
    if selected_is_dir {
        selected.to_path_buf()
    } else {
        selected
            .parent()
            .map_or_else(|| root.to_path_buf(), Path::to_path_buf)
    }
}

/// Where an open file `open` goes when `from` moves to `to`: itself or a file below a moved folder
/// (behaviour 21); `None` when the move does not touch it.
pub fn moved_path(open: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    open.strip_prefix(from).ok().map(|rest| {
        if rest.as_os_str().is_empty() {
            to.to_path_buf()
        } else {
            to.join(rest)
        }
    })
}

/// The title of the name field of Paste, e.g. `Copy a.txt to src`.
pub fn paste_title(mode: Transfer, from: &Path, dir: &Path, root: &Path) -> String {
    let name = from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{} {name} to {}", mode.verb(), folder_name(dir, root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clipboard_is_stale_once_other_text_was_copied() {
        let clip = FileClipboard::new(&[PathBuf::from("/p/src/a.txt")], Transfer::Copy);
        assert_eq!(clip.text(), "/p/src/a.txt");
        assert!(clip.current(Some("/p/src/a.txt")));
        assert!(!clip.current(Some("let x = 1;")));
        assert!(!clip.current(None));
        assert!(clip.status().starts_with("Copied a.txt"));
        let cut = FileClipboard {
            mode: Transfer::Move,
            ..clip
        };
        assert!(cut.status().starts_with("Cut a.txt"));
    }

    /// SPEC-0275 v3 behaviour 25 (AK-14).
    #[test]
    fn several_items_on_the_clipboard() {
        let picked = [
            PathBuf::from("/p/src"),
            PathBuf::from("/p/src/a.txt"),
            PathBuf::from("/p/b.txt"),
        ];
        let clip = FileClipboard::new(&picked, Transfer::Move);
        assert_eq!(
            clip.paths,
            [PathBuf::from("/p/src"), PathBuf::from("/p/b.txt")]
        );
        assert_eq!(clip.text(), "/p/src\n/p/b.txt");
        assert!(clip.current(Some("/p/src\n/p/b.txt")));
        assert!(!clip.current(Some("/p/src")));
        assert_eq!(clip.status(), "Cut 2 items: Paste into a folder moves them");
        assert_eq!(
            pasted_status(Transfer::Copy, 2, Path::new("/p/lib"), Path::new("/p")),
            "Copied 2 items to lib"
        );
    }

    #[test]
    fn paste_goes_into_the_selected_folder_or_the_files_folder() {
        let root = Path::new("/p");
        assert_eq!(
            paste_dir(Path::new("/p/src"), true, root),
            PathBuf::from("/p/src")
        );
        assert_eq!(
            paste_dir(Path::new("/p/src/a.txt"), false, root),
            PathBuf::from("/p/src")
        );
        assert_eq!(
            paste_title(
                Transfer::Copy,
                Path::new("/p/a.txt"),
                Path::new("/p/src/lib"),
                root
            ),
            "Copy a.txt to src/lib"
        );
        assert_eq!(
            paste_title(Transfer::Move, Path::new("/p/src/a.txt"), root, root),
            "Move a.txt to p"
        );
    }

    #[test]
    fn open_tabs_follow_a_moved_file_or_folder() {
        let (from, to) = (Path::new("/p/src"), Path::new("/p/lib/src"));
        assert_eq!(
            moved_path(Path::new("/p/src/a/b.rs"), from, to),
            Some(PathBuf::from("/p/lib/src/a/b.rs"))
        );
        assert_eq!(moved_path(from, from, to), Some(to.to_path_buf()));
        assert_eq!(moved_path(Path::new("/p/srcx/a.rs"), from, to), None);
        assert_eq!(moved_path(Path::new("/p/other.rs"), from, to), None);
    }
}
