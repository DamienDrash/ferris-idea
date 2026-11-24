//! What the bookmark popups and the Bookmarks tool window show (`SPEC-0331` behaviours 16, 20,
//! 23), without the toolkit: the rows of the list, the tree of project, files and bookmarks, the
//! grid of mnemonic characters and what the status bar says. The bookmarks themselves, their
//! lines and their file are [`ferris_idea_core::bookmarks`].

use std::collections::BTreeSet;

use ferris_idea_core::bookmarks::{Listed, MNEMONICS};

/// A bookmark as a row: of the list popup, or below its file in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The file, relative to the project.
    pub file: String,
    /// 0-based line.
    pub line: usize,
    /// The mnemonic.
    pub mnemonic: Option<char>,
    /// The file's name.
    pub name: String,
    /// The file's folder, relative to the project; empty in the project folder.
    pub folder: String,
    /// The description, else the line's text.
    pub label: String,
}

impl Row {
    /// `line: label` as the tree shows a bookmark (behaviour 23).
    pub fn line_label(&self) -> String {
        format!("{}: {}", self.line + 1, self.label)
    }
}

/// Name and folder of the project file `file`.
fn name_and_folder(file: &str) -> (String, String) {
    match file.rsplit_once('/') {
        Some((folder, name)) => (name.to_owned(), folder.to_owned()),
        None => (file.to_owned(), String::new()),
    }
}

/// The rows of Go to Mnemonic (`SPEC-0331` v3 behaviour 33): the bookmarks with a mnemonic whose
/// file is there, by their mnemonics, digits before letters.
pub fn mnemonic_rows(listed: &[Listed]) -> Vec<Row> {
    let mut out: Vec<Row> = rows(listed)
        .into_iter()
        .filter(|r| r.mnemonic.is_some())
        .collect();
    out.sort_by_key(|r| r.mnemonic);
    out
}

impl Row {
    /// A row of Go to Mnemonic: the mnemonic, `file name:line` and the description or line.
    pub fn mnemonic_label(&self) -> String {
        format!(
            "{}   {}:{}   {}",
            self.mnemonic.map_or(String::new(), String::from),
            self.name,
            self.line + 1,
            self.label
        )
    }
}

/// The rows of the list popup: the bookmarks whose file is there, newest first (behaviours 8,
/// 20).
pub fn rows(listed: &[Listed]) -> Vec<Row> {
    listed
        .iter()
        .filter(|l| !l.missing)
        .map(|l| {
            let (name, folder) = name_and_folder(&l.bookmark.file);
            Row {
                file: l.bookmark.file.clone(),
                line: l.bookmark.line,
                mnemonic: l.bookmark.mnemonic,
                name,
                folder,
                label: l.label().to_owned(),
            }
        })
        .collect()
}

/// A node of the tool window's tree (behaviour 23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// The project, the top node.
    Project {
        /// The project folder's name.
        name: String,
        /// Its files are shown.
        open: bool,
    },
    /// A file with bookmarks.
    File {
        /// The file, relative to the project.
        file: String,
        /// Its name.
        name: String,
        /// Its folder, relative to the project.
        folder: String,
        /// Its bookmarks are shown.
        open: bool,
    },
    /// A bookmark.
    Bookmark(Row),
}

impl Node {
    /// 0 for the project, 1 for a file, 2 for a bookmark.
    pub fn depth(&self) -> usize {
        match self {
            Node::Project { .. } => 0,
            Node::File { .. } => 1,
            Node::Bookmark(_) => 2,
        }
    }

    /// What names the node while the tree changes: the selection and the closed nodes are kept
    /// by it. Empty for the project, the file for a file, `file:line` for a bookmark.
    pub fn key(&self) -> String {
        match self {
            Node::Project { .. } => String::new(),
            Node::File { file, .. } => file.clone(),
            Node::Bookmark(row) => format!("{}:{}", row.file, row.line),
        }
    }
}

/// The key of the project node in the set of closed nodes.
pub const PROJECT_KEY: &str = "";

/// The visible nodes of the tree: the project, below it the files with bookmarks ordered by
/// their newest bookmark, below each its bookmarks newest first; nodes in `closed` (by
/// [`Node::key`]) hide what is below them.
pub fn tree(project: &str, listed: &[Listed], closed: &BTreeSet<String>) -> Vec<Node> {
    let all = rows(listed);
    let open = !closed.contains(PROJECT_KEY);
    let mut out = vec![Node::Project {
        name: project.to_owned(),
        open,
    }];
    if !open {
        return out;
    }
    let mut files: Vec<&str> = Vec::new();
    for row in &all {
        if !files.contains(&row.file.as_str()) {
            files.push(&row.file);
        }
    }
    for file in files {
        let (name, folder) = name_and_folder(file);
        let open = !closed.contains(file);
        out.push(Node::File {
            file: file.to_owned(),
            name,
            folder,
            open,
        });
        if open {
            out.extend(
                all.iter()
                    .filter(|r| r.file == file)
                    .cloned()
                    .map(Node::Bookmark),
            );
        }
    }
    out
}

/// The index of the parent of node `at` in `nodes`.
pub fn parent(nodes: &[Node], at: usize) -> Option<usize> {
    let depth = nodes.get(at)?.depth();
    (0..at).rev().find(|&i| nodes[i].depth() < depth)
}

/// Characters in a row of the mnemonic grid: the digits fill the first row (behaviour 16).
pub const GRID_COLUMNS: usize = 10;

/// The cell of the mnemonic grid reached from `selected` with an arrow key (`left`, `right`,
/// `up`, `down`); the first cell when none was selected. Left and right go on in the next row,
/// up and down stay inside the grid.
pub fn grid_step(selected: Option<usize>, key: &str) -> Option<usize> {
    let n = MNEMONICS.len();
    let Some(at) = selected else {
        return matches!(key, "left" | "right" | "up" | "down").then_some(0);
    };
    Some(match key {
        "left" => (at + n - 1) % n,
        "right" => (at + 1) % n,
        "up" if at >= GRID_COLUMNS => at - GRID_COLUMNS,
        "down" if at + GRID_COLUMNS < n => at + GRID_COLUMNS,
        "up" | "down" => at,
        _ => return None,
    })
}

/// The status bar after `ToggleBookmark0`…`9` or the mnemonic popup gave a line the mnemonic
/// `m`, which was taken from `from` (file and 0-based line) when another bookmark had it.
pub fn taken_message(m: char, from: Option<&(String, usize)>) -> Option<String> {
    from.map(|(file, line)| {
        format!(
            "Mnemonic {m} taken from the bookmark at {file}:{}",
            line + 1
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_core::bookmarks::{Bookmarks, Change, listed};

    fn sample() -> Vec<Listed> {
        let mut b = Bookmarks::default();
        b.toggle("README.md", 0);
        b.toggle("src/main/java/lab/inventory/Inventory.java", 6);
        b.toggle("src/main/java/lab/inventory/Inventory.java", 11);
        b.toggle("gone/Old.java", 2);
        b.toggle("src/Other.java", 3);
        b.set(
            "src/main/java/lab/inventory/Inventory.java",
            15,
            Change::Set('1'),
            Change::Keep,
        );
        b.set(
            "README.md",
            0,
            Change::Keep,
            Change::Set("start here".into()),
        );
        let text = |file: &str| -> Option<String> {
            (file != "gone/Old.java").then(|| {
                (1..=20)
                    .map(|n| format!("  line {n} of {file}\n"))
                    .collect()
            })
        };
        // No file of the sample is on disk: the texts come from "open editors".
        listed(std::path::Path::new("/nonexistent"), &b, &text)
    }

    /// SPEC-0331 AK-11 (v3): Go to Mnemonic lists the bookmarks with a mnemonic, by it.
    #[test]
    fn go_to_mnemonic_lists_the_mnemonics_in_order() {
        let mut b = Bookmarks::default();
        b.toggle("src/Plain.java", 1);
        b.set("src/Zed.java", 4, Change::Set('B'), Change::Keep);
        b.set(
            "src/Late.java",
            9,
            Change::Set('7'),
            Change::Set("the loop".into()),
        );
        b.set("gone/Old.java", 2, Change::Set('A'), Change::Keep);
        let text = |file: &str| -> Option<String> {
            (file != "gone/Old.java").then(|| {
                (1..=20)
                    .map(|n| format!("  line {n} of {file}\n"))
                    .collect()
            })
        };
        let listed = listed(std::path::Path::new("/nonexistent"), &b, &text);
        let rows = mnemonic_rows(&listed);
        let shown: Vec<String> = rows.iter().map(Row::mnemonic_label).collect();
        assert_eq!(
            shown,
            [
                "7   Late.java:10   the loop",
                "B   Zed.java:5   line 5 of src/Zed.java",
            ]
        );
        assert!(mnemonic_rows(&[]).is_empty());
    }

    /// SPEC-0331 AK-7: the rows of the list popup.
    #[test]
    fn the_list_has_file_line_text_and_mnemonic_newest_first() {
        let rows = rows(&sample());
        let shown: Vec<String> = rows
            .iter()
            .map(|r| {
                format!(
                    "{} {} {} [{}] {}",
                    r.mnemonic.map_or("-".to_owned(), String::from),
                    r.name,
                    r.line + 1,
                    r.folder,
                    r.label
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                "1 Inventory.java 16 [src/main/java/lab/inventory] line 16 of src/main/java/lab/inventory/Inventory.java",
                "- Other.java 4 [src] line 4 of src/Other.java",
                "- Inventory.java 12 [src/main/java/lab/inventory] line 12 of src/main/java/lab/inventory/Inventory.java",
                "- Inventory.java 7 [src/main/java/lab/inventory] line 7 of src/main/java/lab/inventory/Inventory.java",
                "- README.md 1 [] start here",
            ],
            "newest first, the missing file left out, the description in place of the text"
        );
    }

    /// SPEC-0331 AK-7: the tree of the tool window.
    #[test]
    fn the_tree_has_the_project_its_files_and_their_bookmarks() {
        let listed = sample();
        let label = |n: &Node| match n {
            Node::Project { name, open } => format!("{name} {open}"),
            Node::File {
                name, folder, open, ..
            } => format!("  {name} ({folder}) {open}"),
            Node::Bookmark(r) => format!(
                "    {}{}",
                r.mnemonic.map_or(String::new(), |m| format!("[{m}] ")),
                r.line_label()
            ),
        };
        let all = tree("cmp-java-ij", &listed, &BTreeSet::new());
        assert_eq!(
            all.iter().map(label).collect::<Vec<_>>(),
            [
                "cmp-java-ij true",
                "  Inventory.java (src/main/java/lab/inventory) true",
                "    [1] 16: line 16 of src/main/java/lab/inventory/Inventory.java",
                "    12: line 12 of src/main/java/lab/inventory/Inventory.java",
                "    7: line 7 of src/main/java/lab/inventory/Inventory.java",
                "  Other.java (src) true",
                "    4: line 4 of src/Other.java",
                "  README.md () true",
                "    1: start here",
            ]
        );
        assert_eq!(all[0].key(), PROJECT_KEY);
        assert_eq!(all[5].key(), "src/Other.java");
        assert_eq!(all[6].key(), "src/Other.java:3");
        assert_eq!(parent(&all, 3), Some(1));
        assert_eq!(parent(&all, 5), Some(0));
        assert_eq!(parent(&all, 0), None);
        // A closed file hides its bookmarks, a closed project everything.
        let closed: BTreeSet<String> = ["src/Other.java".to_owned()].into();
        let some = tree("cmp-java-ij", &listed, &closed);
        assert_eq!(some.len(), 8);
        assert_eq!(label(&some[5]), "  Other.java (src) false");
        let closed: BTreeSet<String> = [PROJECT_KEY.to_owned()].into();
        assert_eq!(
            tree("cmp-java-ij", &listed, &closed)
                .iter()
                .map(label)
                .collect::<Vec<_>>(),
            ["cmp-java-ij false"]
        );
        // Without bookmarks the project stands alone.
        assert_eq!(tree("p", &[], &BTreeSet::new()).len(), 1);
    }

    /// SPEC-0331 behaviour 16: the arrow keys in the grid of characters.
    #[test]
    fn arrows_move_through_the_grid_of_characters() {
        assert_eq!(MNEMONICS[0], '1');
        assert_eq!(MNEMONICS[9], '0');
        assert_eq!(MNEMONICS[10], 'A');
        assert_eq!(grid_step(None, "down"), Some(0));
        assert_eq!(grid_step(None, "enter"), None);
        assert_eq!(grid_step(Some(0), "right"), Some(1));
        assert_eq!(grid_step(Some(0), "left"), Some(35));
        assert_eq!(grid_step(Some(35), "right"), Some(0));
        assert_eq!(grid_step(Some(3), "down"), Some(13));
        assert_eq!(grid_step(Some(13), "up"), Some(3));
        assert_eq!(grid_step(Some(3), "up"), Some(3));
        assert_eq!(grid_step(Some(28), "down"), Some(28), "no cell below");
        assert_eq!(grid_step(Some(25), "down"), Some(35));
        assert_eq!(grid_step(Some(4), "tab"), None);
        assert_eq!(
            taken_message('3', Some(&("a.rs".into(), 6))).as_deref(),
            Some("Mnemonic 3 taken from the bookmark at a.rs:7")
        );
        assert_eq!(taken_message('3', None), None);
    }
}
