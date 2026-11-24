//! Go to Line and Recent Files without toolkit (`SPEC-0069`).

use std::path::{Path, PathBuf};

use ferris_idea_core::{fuzzy_match, rank};

pub use ferris_idea_core::local_state::{LOCAL_STATE_IGNORE, refresh_local_state_ignore};

/// Most files remembered (behaviour 3).
pub const RECENT_LIMIT: usize = 50;

/// Parses `line`, `line:column` or `:column` (1-based) into a 0-based position; a missing part keeps
/// the current one (`current` is 0-based). Separators `:` and `,` are accepted (behaviour 1).
pub fn parse_line_column(text: &str, current: (usize, usize)) -> Option<(usize, usize)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (line, column) = match text.split_once([':', ',']) {
        Some((l, c)) => (l.trim(), Some(c.trim())),
        None => (text, None),
    };
    let one_based = |s: &str| -> Option<usize> { s.parse::<usize>().ok().map(|n| n.max(1) - 1) };
    let line = if line.is_empty() {
        current.0
    } else {
        one_based(line)?
    };
    let column = match column {
        None => 0,
        Some("") => 0,
        Some(c) => one_based(c)?,
    };
    Some((line, column))
}

/// Recently opened files of a workspace, most recent first (behaviour 3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentFiles {
    files: Vec<PathBuf>,
}

impl RecentFiles {
    /// Reads the list from `.ferris-idea/recent-files` (one workspace-relative path per line); missing
    /// files are dropped.
    pub fn load(root: &Path) -> Self {
        let text =
            std::fs::read_to_string(root.join(".ferris-idea/recent-files")).unwrap_or_default();
        let files = text
            .lines()
            .map(|l| root.join(l))
            .filter(|p| p.is_file())
            .take(RECENT_LIMIT)
            .collect();
        Self { files }
    }

    /// Writes the list back; errors are ignored (the list is a convenience).
    pub fn save(&self, root: &Path) {
        let lines: Vec<String> = self
            .files
            .iter()
            .filter_map(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        if refresh_local_state_ignore(root) {
            let _ = std::fs::write(
                root.join(".ferris-idea/recent-files"),
                lines.join("\n") + "\n",
            );
        }
    }

    /// Moves `path` to the front.
    pub fn touch(&mut self, path: &Path) {
        self.files.retain(|p| p != path);
        self.files.insert(0, path.to_path_buf());
        self.files.truncate(RECENT_LIMIT);
    }

    /// All files, most recent first.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Files matching `filter` by name (fuzzy, best first); all in recency order when empty. The
    /// file shown in the editor (`current`) comes first, as in the reference; the popup selects the
    /// second row, so Enter on an empty filter switches to the previous file (v2 behaviour 4).
    pub fn matching(&self, filter: &str, current: Option<&Path>) -> Vec<PathBuf> {
        let filter = filter.trim();
        // A file deleted since it was opened is not offered (hand test 208), the open one always.
        let present = |p: &&PathBuf| p.is_file() || Some(p.as_path()) == current;
        let mut out: Vec<PathBuf> = if filter.is_empty() {
            self.files.iter().filter(present).cloned().collect()
        } else {
            let mut found: Vec<(i64, String, PathBuf)> = self
                .files
                .iter()
                .filter(present)
                .filter_map(|p| {
                    let name = p.file_name()?.to_string_lossy().into_owned();
                    fuzzy_match(filter, &name).map(|m| (m.score, name, p.clone()))
                })
                .collect();
            found.sort_by(|a, b| rank((a.0, &a.1), (b.0, &b.1)));
            found.into_iter().map(|(_, _, p)| p).collect()
        };
        if filter.is_empty()
            && let Some(cur) = current
            && let Some(i) = out.iter().position(|p| p == cur)
        {
            let c = out.remove(i);
            out.insert(0, c);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_column() {
        assert_eq!(parse_line_column("12", (0, 5)), Some((11, 0)));
        assert_eq!(parse_line_column("12:4", (0, 0)), Some((11, 3)));
        assert_eq!(parse_line_column(" 3 , 2 ", (0, 0)), Some((2, 1)));
        assert_eq!(parse_line_column(":7", (9, 0)), Some((9, 6)));
        assert_eq!(parse_line_column("0", (4, 4)), Some((0, 0)));
        assert_eq!(parse_line_column("x", (0, 0)), None);
        assert_eq!(parse_line_column("", (0, 0)), None);
    }

    #[test]
    fn recent_order_filter_and_persistence() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["a.rs", "b.rs", "notes.md"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let mut r = RecentFiles::default();
        for f in ["a.rs", "b.rs", "notes.md", "a.rs"] {
            r.touch(&dir.join(f));
        }
        let names = |v: Vec<PathBuf>| -> Vec<String> {
            v.iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(r.files().to_vec()), ["a.rs", "notes.md", "b.rs"]);
        assert_eq!(
            names(r.matching("", Some(&dir.join("a.rs")))),
            ["a.rs", "notes.md", "b.rs"],
            "the current file comes first (v2)"
        );
        assert_eq!(names(r.matching("nt", None)), ["notes.md"]);
        r.save(&dir);
        assert_eq!(
            std::fs::read_to_string(dir.join(".ferris-idea/.gitignore")).unwrap(),
            LOCAL_STATE_IGNORE
        );
        std::fs::remove_file(dir.join("b.rs")).unwrap();
        assert_eq!(
            names(r.matching("", None)),
            ["a.rs", "notes.md"],
            "a deleted file is not offered (hand test 208)"
        );
        assert_eq!(
            names(RecentFiles::load(&dir).files().to_vec()),
            ["a.rs", "notes.md"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
