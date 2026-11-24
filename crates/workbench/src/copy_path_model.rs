//! The rows of Copy Path/Reference… (`SPEC-0332` v2 behaviours 13 to 13f) for the file of the
//! active editor or the project tree's selection: the absolute path, the file name, the path with
//! the caret's line, the path below the project folder, the path below a source folder and Copy
//! Reference. Which rows there are and what each yields follows the open platform's copy popup
//! (intellij-community, `docs/legal/PROVENANCE.md`: logic template); the project folder is the
//! content root.

use std::path::{Path, PathBuf};

use ferris_idea_languages::reference_name;

/// The action that opens the popup.
pub const POPUP: &str = "CopyReferencePopupGroup";
/// Copies the absolute path.
pub const ABSOLUTE_PATH: &str = "CopyAbsolutePath";
/// Copies the file name.
pub const FILE_NAME: &str = "CopyFileName";
/// Copies the file's reference name with the caret's line.
pub const PATH_WITH_LINE: &str = "CopyPathWithLineNumber";
/// Copies the path below the project folder.
pub const CONTENT_ROOT_PATH: &str = "CopyContentRootPath";
/// Copies the path below the source folder.
pub const SOURCE_ROOT_PATH: &str = "CopySourceRootPath";
/// Copies the qualified name of the symbol around the caret, else file and line.
pub const REFERENCE: &str = "CopyReference";

/// The popup's title.
pub const TITLE: &str = "Copy";
/// What the status bar says without a file to copy a path of.
pub const NO_FILE: &str = "Copy Path/Reference: no file is open in the editor";
/// What the status bar says when Copy Reference has no editor to act on.
pub const NO_EDITOR: &str = "Copy Reference: no file is open in the editor";
/// The longest text the status bar repeats of what was copied, in characters.
pub const STATUS_CHARS: usize = 80;

/// What a copy acts on: the file of an editor with its caret, or the items selected in the
/// project tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    /// The files and folders, in the order their lines are copied.
    pub paths: Vec<PathBuf>,
    /// The 0-based line of the editor's primary caret; `None` for the tree's selection.
    pub line: Option<usize>,
}

impl Subject {
    /// The file of an editor with the caret on 0-based `line`.
    pub fn editor(path: PathBuf, line: usize) -> Self {
        Self {
            paths: vec![path],
            line: Some(line),
        }
    }

    /// The items selected in the project tree.
    pub fn tree(paths: Vec<PathBuf>) -> Self {
        Self { paths, line: None }
    }
}

/// The project the paths are taken relative to.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    /// The project folder.
    pub root: &'a Path,
    /// The project's source folders (`SPEC-0048`), absolute.
    pub source_roots: &'a [PathBuf],
    /// In a remote project the stand-in folder and the folder on the host it stands for: the
    /// paths are then those on the host, and there are no source folders.
    pub remote: Option<&'a (PathBuf, String)>,
}

impl Scope<'_> {
    fn absolute(&self, path: &Path) -> String {
        crate::chrome::shown_path(path, None, self.remote)
    }

    fn below_content_root(&self, path: &Path) -> Option<String> {
        match self.remote {
            Some((stand_in, _)) if path.starts_with(stand_in) => {
                reference_name::below(path, stand_in)
            }
            // The editor's path and the project folder may differ by a symbolic link.
            _ => reference_name::below(path, self.root),
        }
    }

    fn below_source_root(&self, path: &Path) -> Option<String> {
        if self.remote.is_some() {
            return None;
        }
        let source = ferris_idea_core::source_roots::containing(self.source_roots, path)?;
        reference_name::below(path, source)
    }

    /// The file's reference name (behaviour 13a): below its source folder, else below the
    /// project folder, else its absolute path.
    pub fn reference_name(&self, path: &Path) -> String {
        self.below_source_root(path)
            .or_else(|| self.below_content_root(path))
            .unwrap_or_else(|| self.absolute(path))
    }

    /// What Copy Reference copies where no symbol can be named: the file's reference name and
    /// the 1-based number of 0-based `line`.
    pub fn line_reference(&self, path: &Path, line: usize) -> String {
        reference_name::with_line(&self.reference_name(path), line)
    }
}

/// The first line of `text`, cut after [`STATUS_CHARS`] characters, and how many lines follow.
pub fn shortened(text: &str) -> String {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let more = lines.count();
    let mut out: String = first.chars().take(STATUS_CHARS).collect();
    if first.chars().count() > STATUS_CHARS {
        out.push('…');
    }
    match more {
        0 => {}
        1 => out.push_str(" (and 1 more line)"),
        n => out.push_str(&format!(" (and {n} more lines)")),
    }
    out
}

/// What the status bar says after Copy Reference copied `text`; `no_symbols` when the language
/// server gave no symbols and file and line were copied instead.
pub fn reference_copied(text: &str, no_symbols: bool) -> String {
    let note = if no_symbols {
        " (file and line: no symbols from the language server)"
    } else {
        ""
    };
    format!("Reference copied: {}{note}", shortened(text))
}

/// One row of the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The action that copies this row on its own.
    pub action: &'static str,
    /// The row's name.
    pub title: &'static str,
    /// What choosing it puts on the clipboard, one line per item. Empty for Copy Reference in a
    /// language with naming rules: the name is asked of the language server when it is chosen.
    pub text: String,
}

impl Row {
    /// The row as the list shows it: its name and what it copies.
    pub fn label(&self) -> String {
        if self.text.is_empty() {
            return self.title.to_owned();
        }
        format!("{}    {}", self.title, self.text.replace('\n', "  "))
    }

    /// What the status bar says after the row was copied.
    pub fn copied(&self) -> String {
        if self.action == REFERENCE {
            return reference_copied(&self.text, false);
        }
        format!("{} copied: {}", self.title, shortened(&self.text))
    }
}

/// The rows for `subject` in the project `scope`, in the popup's order; a row is there only when
/// at least one item has that kind of path, and holds one line per item that has it. `reference`
/// is what the Copy Reference row shows — given for an editor, empty when the name is only known
/// once the row is chosen — and `None` leaves the row out.
pub fn rows(subject: &Subject, scope: &Scope<'_>, reference: Option<String>) -> Vec<Row> {
    let lines = |of: &dyn Fn(&Path) -> Option<String>| -> Option<String> {
        let found: Vec<String> = subject
            .paths
            .iter()
            .filter_map(|p| of(p))
            .filter(|line| !line.trim().is_empty())
            .collect();
        (!found.is_empty()).then(|| found.join("\n"))
    };
    let kinds: [(&'static str, &'static str, Option<String>); 5] = [
        (
            ABSOLUTE_PATH,
            "Absolute Path",
            lines(&|p| Some(scope.absolute(p))),
        ),
        (
            FILE_NAME,
            "File Name",
            lines(&|p| Some(p.file_name()?.to_string_lossy().into_owned())),
        ),
        (
            PATH_WITH_LINE,
            "Path with Line Number",
            subject
                .line
                .and_then(|line| lines(&|p| Some(scope.line_reference(p, line)))),
        ),
        (
            CONTENT_ROOT_PATH,
            "Path from Content Root",
            lines(&|p| scope.below_content_root(p)),
        ),
        (
            SOURCE_ROOT_PATH,
            "Path from Source Root",
            lines(&|p| scope.below_source_root(p)),
        ),
    ];
    let mut out: Vec<Row> = kinds
        .into_iter()
        .filter_map(|(action, title, text)| {
            Some(Row {
                action,
                title,
                text: text?,
            })
        })
        .collect();
    if let (Some(text), false) = (reference, subject.paths.is_empty()) {
        out.push(Row {
            action: REFERENCE,
            title: "Copy Reference",
            text,
        });
    }
    out
}

/// The row `action` copies, if `rows` has it.
pub fn row_of<'a>(rows: &'a [Row], action: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.action == action)
}

/// What the status bar says when `action` has nothing to copy for the subject.
pub fn unavailable(action: &str) -> &'static str {
    match action {
        CONTENT_ROOT_PATH => "Path from Content Root: not in the project folder",
        SOURCE_ROOT_PATH => "Path from Source Root: not in a source folder",
        PATH_WITH_LINE => "Path with Line Number: needs the caret of an editor",
        _ => NO_FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rows: &[Row]) -> Vec<(&'static str, &str)> {
        rows.iter().map(|r| (r.title, r.text.as_str())).collect()
    }

    fn scope<'a>(root: &'a str, source_roots: &'a [PathBuf]) -> Scope<'a> {
        Scope {
            root: Path::new(root),
            source_roots,
            remote: None,
        }
    }

    /// The rows of a file as the tree has it selected: no line, no reference.
    fn file_rows(path: &str, root: &str) -> Vec<Row> {
        rows(
            &Subject::tree(vec![PathBuf::from(path)]),
            &scope(root, &[]),
            None,
        )
    }

    #[test]
    fn rows_of_a_project_file() {
        let rows = file_rows("/work/shop/src/main/Cart.java", "/work/shop");
        assert_eq!(
            texts(&rows),
            [
                ("Absolute Path", "/work/shop/src/main/Cart.java"),
                ("File Name", "Cart.java"),
                ("Path from Content Root", "src/main/Cart.java"),
            ]
        );
        assert_eq!(
            rows[2].label(),
            "Path from Content Root    src/main/Cart.java"
        );
        assert_eq!(rows[1].copied(), "File Name copied: Cart.java");
    }

    #[test]
    fn a_file_outside_the_project_has_no_content_root_row() {
        let rows = file_rows("/usr/lib/jdk/src/String.java", "/work/shop");
        assert_eq!(
            texts(&rows),
            [
                ("Absolute Path", "/usr/lib/jdk/src/String.java"),
                ("File Name", "String.java"),
            ]
        );
        assert_eq!(row_of(&rows, CONTENT_ROOT_PATH), None);
        assert!(unavailable(CONTENT_ROOT_PATH).contains("not in the project folder"));
        assert!(unavailable(SOURCE_ROOT_PATH).contains("not in a source folder"));
        assert_eq!(unavailable(FILE_NAME), NO_FILE);
        // A folder whose name only begins like the project's is outside it.
        assert_eq!(file_rows("/work/shop-old/a.txt", "/work/shop").len(), 2);
    }

    #[test]
    fn the_action_of_a_row() {
        let rows = file_rows("/work/shop/a b/ü.txt", "/work/shop");
        assert_eq!(
            row_of(&rows, ABSOLUTE_PATH).map(|r| r.text.as_str()),
            Some("/work/shop/a b/ü.txt")
        );
        assert_eq!(
            row_of(&rows, FILE_NAME).map(|r| r.text.as_str()),
            Some("ü.txt")
        );
        assert_eq!(
            row_of(&rows, CONTENT_ROOT_PATH).map(|r| r.text.as_str()),
            Some("a b/ü.txt")
        );
        assert_eq!(row_of(&rows, POPUP), None);
        assert_eq!(row_of(&rows, PATH_WITH_LINE), None);
        assert_eq!(row_of(&rows, REFERENCE), None);
    }

    #[test]
    fn rows_of_an_editor() {
        // A file in no source folder, the caret on the twelfth line; a language without naming
        // rules shows what Copy Reference copies.
        let subject = Subject::editor(PathBuf::from("/work/shop/docs/notes.txt"), 11);
        let scope = scope("/work/shop", &[]);
        let fallback = scope.line_reference(&subject.paths[0], 11);
        let rows = rows(&subject, &scope, Some(fallback));
        assert_eq!(
            texts(&rows),
            [
                ("Absolute Path", "/work/shop/docs/notes.txt"),
                ("File Name", "notes.txt"),
                ("Path with Line Number", "docs/notes.txt:12"),
                ("Path from Content Root", "docs/notes.txt"),
                ("Copy Reference", "docs/notes.txt:12"),
            ]
        );
        assert_eq!(
            rows.iter().map(|r| r.action).collect::<Vec<_>>(),
            [
                ABSOLUTE_PATH,
                FILE_NAME,
                PATH_WITH_LINE,
                CONTENT_ROOT_PATH,
                REFERENCE
            ]
        );
        assert_eq!(rows[4].copied(), "Reference copied: docs/notes.txt:12");
        // The name of a symbol is known only once the row is chosen: no text in the list.
        let rows = super::rows(&subject, &scope, Some(String::new()));
        assert_eq!(rows[4].label(), "Copy Reference");
        // A file outside the project: its absolute path with the line.
        let outside = Subject::editor(PathBuf::from("/usr/lib/jdk/String.java"), 0);
        assert_eq!(
            row_of(&super::rows(&outside, &scope, None), PATH_WITH_LINE).map(|r| r.text.as_str()),
            Some("/usr/lib/jdk/String.java:1")
        );
    }

    #[test]
    fn rows_of_a_file_in_a_source_folder() {
        let roots = [
            PathBuf::from("/work/shop/src/main/java"),
            PathBuf::from("/work/shop/src/main/java/gen"),
            PathBuf::from("/work/shop/src/test/java"),
        ];
        let scope = scope("/work/shop", &roots);
        let subject = Subject::editor(PathBuf::from("/work/shop/src/main/java/shop/Cart.java"), 2);
        let rows = rows(&subject, &scope, Some(String::new()));
        assert_eq!(
            texts(&rows),
            [
                ("Absolute Path", "/work/shop/src/main/java/shop/Cart.java"),
                ("File Name", "Cart.java"),
                // The reference name of a file in a source folder starts there.
                ("Path with Line Number", "shop/Cart.java:3"),
                ("Path from Content Root", "src/main/java/shop/Cart.java"),
                ("Path from Source Root", "shop/Cart.java"),
                ("Copy Reference", ""),
            ]
        );
        // The innermost source folder counts.
        let inner = Subject::tree(vec![PathBuf::from("/work/shop/src/main/java/gen/a/B.java")]);
        assert_eq!(
            row_of(&super::rows(&inner, &scope, None), SOURCE_ROOT_PATH).map(|r| r.text.as_str()),
            Some("a/B.java")
        );
        assert_eq!(
            scope.line_reference(Path::new("/work/shop/src/test/java/shop/CartTest.java"), 0),
            "shop/CartTest.java:1"
        );
        assert_eq!(
            scope.reference_name(Path::new("/work/shop/pom.xml")),
            "pom.xml"
        );
    }

    #[test]
    fn the_tree_selection_copies_one_line_per_item() {
        let roots = [PathBuf::from("/work/shop/src")];
        let scope = scope("/work/shop", &roots);
        let subject = Subject::tree(vec![
            PathBuf::from("/work/shop/src/shop/Cart.java"),
            PathBuf::from("/work/shop/src/shop"),
            PathBuf::from("/work/shop/README.md"),
            PathBuf::from("/usr/lib/jdk/String.java"),
        ]);
        let rows = rows(&subject, &scope, None);
        assert_eq!(
            texts(&rows),
            [
                (
                    "Absolute Path",
                    "/work/shop/src/shop/Cart.java\n/work/shop/src/shop\n/work/shop/README.md\n/usr/lib/jdk/String.java"
                ),
                ("File Name", "Cart.java\nshop\nREADME.md\nString.java"),
                // Items without that kind of path give no line.
                (
                    "Path from Content Root",
                    "src/shop/Cart.java\nsrc/shop\nREADME.md"
                ),
                ("Path from Source Root", "shop/Cart.java\nshop"),
            ]
        );
        assert_eq!(
            rows[0].copied(),
            "Absolute Path copied: /work/shop/src/shop/Cart.java (and 3 more lines)"
        );
        assert_eq!(
            rows[3].label(),
            "Path from Source Root    shop/Cart.java  shop"
        );
        // A kind none of the items has is no row.
        let loose = Subject::tree(vec![
            PathBuf::from("/work/shop/README.md"),
            PathBuf::from("/work/shop/docs"),
        ]);
        assert_eq!(
            row_of(&super::rows(&loose, &scope, None), SOURCE_ROOT_PATH),
            None
        );
        assert!(super::rows(&Subject::tree(Vec::new()), &scope, Some("x".into())).is_empty());
    }

    #[test]
    fn a_folder_and_the_project_folder() {
        let roots = [PathBuf::from("/work/shop/src")];
        let scope = scope("/work/shop", &roots);
        let folder = rows(
            &Subject::tree(vec![PathBuf::from("/work/shop/src/shop")]),
            &scope,
            None,
        );
        assert_eq!(
            texts(&folder),
            [
                ("Absolute Path", "/work/shop/src/shop"),
                ("File Name", "shop"),
                ("Path from Content Root", "src/shop"),
                ("Path from Source Root", "shop"),
            ]
        );
        // A source folder is not below itself, the project folder not below itself.
        let source = rows(
            &Subject::tree(vec![PathBuf::from("/work/shop/src")]),
            &scope,
            None,
        );
        assert_eq!(row_of(&source, SOURCE_ROOT_PATH), None);
        let project = rows(
            &Subject::tree(vec![PathBuf::from("/work/shop")]),
            &scope,
            None,
        );
        assert_eq!(
            texts(&project),
            [("Absolute Path", "/work/shop"), ("File Name", "shop")]
        );
    }

    #[test]
    fn a_remote_document_has_the_paths_on_the_host() {
        let remote = (PathBuf::from("/tmp/stand-in"), "/srv/app".to_owned());
        // Source folders of this machine do not count for a remote project.
        let roots = [PathBuf::from("/tmp/stand-in/src")];
        let scope = Scope {
            root: Path::new("/tmp/stand-in"),
            source_roots: &roots,
            remote: Some(&remote),
        };
        let subject = Subject::editor(PathBuf::from("/tmp/stand-in/src/main.py"), 4);
        let fallback = scope.line_reference(&subject.paths[0], 4);
        let rows = rows(&subject, &scope, Some(fallback));
        assert_eq!(
            texts(&rows),
            [
                ("Absolute Path", "/srv/app/src/main.py"),
                ("File Name", "main.py"),
                ("Path with Line Number", "src/main.py:5"),
                ("Path from Content Root", "src/main.py"),
                ("Copy Reference", "src/main.py:5"),
            ]
        );
        assert!(rows.iter().all(|r| !r.text.contains("stand-in")));
        // Several remote items: one line each.
        let two = super::rows(
            &Subject::tree(vec![
                PathBuf::from("/tmp/stand-in/src/main.py"),
                PathBuf::from("/tmp/stand-in/docs"),
            ]),
            &scope,
            None,
        );
        assert_eq!(
            texts(&two),
            [
                ("Absolute Path", "/srv/app/src/main.py\n/srv/app/docs"),
                ("File Name", "main.py\ndocs"),
                ("Path from Content Root", "src/main.py\ndocs"),
            ]
        );
    }

    #[test]
    fn the_status_text_is_cut() {
        assert_eq!(shortened("src/a.txt"), "src/a.txt");
        let long = "ä".repeat(STATUS_CHARS + 5);
        assert_eq!(shortened(&long), format!("{}…", "ä".repeat(STATUS_CHARS)));
        assert_eq!(
            shortened(&"x".repeat(STATUS_CHARS)),
            "x".repeat(STATUS_CHARS)
        );
        assert_eq!(shortened("a\nb"), "a (and 1 more line)");
        assert_eq!(shortened("a\nb\nc"), "a (and 2 more lines)");
        assert_eq!(shortened(""), "");
        assert_eq!(
            reference_copied("shop.Cart#total", false),
            "Reference copied: shop.Cart#total"
        );
        assert_eq!(
            reference_copied("src/a.py:3", true),
            "Reference copied: src/a.py:3 (file and line: no symbols from the language server)"
        );
    }
}
