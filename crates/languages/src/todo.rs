//! TODO items (`SPEC-0334` behaviours 1–11): the notes the two built-in patterns find in the
//! comments of a text, with their continuation lines, and the scan of the project's own files
//! that keeps what it found for files that did not change.
//!
//! What a comment is comes from the file's grammar ([`crate::syntax::comment_ranges_of`]); a
//! plain-text file is searched as a whole, a file without a grammar is not searched. The rules
//! follow the open platform's pattern search (see `docs/legal/PROVENANCE.md`); the code is
//! Ferris IDEA's own.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use ferris_idea_core::{SearchScope, Workspace, search_candidates};
use serde::Serialize;

use crate::syntax::{MAX_SYNTAX_BYTES, comment_ranges_of};

/// Most items a scan of the project keeps (behaviour 7).
pub const MAX_ITEMS: usize = 5_000;

/// Most characters of an item's text and of a row's line text (behaviour 7).
pub const MAX_TEXT_CHARS: usize = 300;

/// Most continuation lines of an item (behaviour 7).
pub const MAX_CONTINUATION_LINES: usize = 20;

/// Characters that may stand, besides spaces and tabs, between the begin of a comment in a
/// continuation line and the item's column (behaviour 6).
const LEADERS: [char; 7] = ['/', '*', '#', '-', ';', '!', '%'];

/// The closers of block comments that are not part of an item's text (behaviour 4).
const CLOSERS: [&str; 2] = ["*/", "-->"];

/// A built-in pattern (behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Pattern {
    /// `\btodo\b.*`, not case-sensitive.
    Todo,
    /// `\bfixme\b.*`, not case-sensitive.
    Fixme,
}

/// The patterns in their order.
pub const PATTERNS: [Pattern; 2] = [Pattern::Todo, Pattern::Fixme];

impl Pattern {
    /// The pattern's name: `todo` or `fixme`.
    pub fn name(self) -> &'static str {
        match self {
            Pattern::Todo => "todo",
            Pattern::Fixme => "fixme",
        }
    }

    /// The pattern of a name.
    pub fn from_name(name: &str) -> Option<Self> {
        PATTERNS.into_iter().find(|p| p.name() == name)
    }

    /// The regular expression the pattern stands for (matched without regard to case).
    pub fn expression(self) -> &'static str {
        match self {
            Pattern::Todo => r"\btodo\b.*",
            Pattern::Fixme => r"\bfixme\b.*",
        }
    }
}

/// One TODO item (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 0-based line of the match.
    pub line: usize,
    /// 0-based column of the match, in characters.
    pub column: usize,
    /// The pattern that matched.
    pub pattern: Pattern,
    /// The match with its continuation lines, cut after [`MAX_TEXT_CHARS`] characters.
    pub text: String,
    /// The item's line without the white space at its ends, cut the same way.
    pub line_text: String,
    /// The characters of the match in `line_text`.
    pub mark: Range<usize>,
    /// How many continuation lines belong to the item.
    pub continuation: usize,
}

/// What searching one text gave.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileScan {
    /// The items, by line and column.
    pub items: Vec<Item>,
    /// The text holds a match but the file has no known comment syntax: nothing was looked for
    /// (behaviour 3).
    pub not_checked: bool,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte offsets in `segment` where a pattern matches, with the pattern, in text order: the
/// word without regard to case, with no word character before or after it.
fn matches_in(segment: &str) -> Vec<(usize, Pattern)> {
    let bytes = segment.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let hit = PATTERNS.into_iter().find(|p| {
            let word = p.name().as_bytes();
            bytes.len() - at >= word.len()
                && bytes[at..at + word.len()].eq_ignore_ascii_case(word)
                && !segment[..at].chars().next_back().is_some_and(is_word)
                && !segment[at + word.len()..]
                    .chars()
                    .next()
                    .is_some_and(is_word)
        });
        match hit {
            Some(p) => {
                found.push((at, p));
                at += p.name().len();
            }
            None => {
                // The next character: the words are ASCII, so a match starts on a boundary.
                at += 1;
                while at < bytes.len() && !segment.is_char_boundary(at) {
                    at += 1;
                }
            }
        }
    }
    found
}

/// Whether `text` holds one of the words at all, comment or not: the cheap look before a tree
/// is built.
fn may_match(text: &str) -> bool {
    let bytes = text.as_bytes();
    PATTERNS.into_iter().any(|p| {
        let word = p.name().as_bytes();
        bytes.windows(word.len()).any(|w| {
            // The first letter decides quickly for most windows.
            w[0].eq_ignore_ascii_case(&word[0]) && w.eq_ignore_ascii_case(word)
        })
    })
}

/// `text` cut after [`MAX_TEXT_CHARS`] characters, with `…` when something was cut.
pub fn cut(text: &str) -> String {
    match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// `part`, which ends at `end` of the text, without the white space at its end and, when it
/// ends where its comment ends, without the block comment's closer.
fn without_closer(part: &str, end: usize, comment_end: usize) -> &str {
    let part = part.trim_end();
    if end != comment_end {
        return part;
    }
    CLOSERS
        .iter()
        .find_map(|c| part.strip_suffix(c))
        .map_or(part, str::trim_end)
}

/// The lines of a text: the byte range of each without its line break.
fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            let end = if i > start && text.as_bytes()[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            lines.push(start..end);
            start = i + 1;
        }
    }
    lines.push(start..text.len());
    lines
}

/// The byte offset of character `column` of `line`, when the line has that many characters.
fn byte_of_column(line: &str, column: usize) -> Option<usize> {
    line.char_indices().nth(column).map(|(i, _)| i)
}

/// The continuation lines of an item that starts at character `column` of line `line` inside
/// comment `comment` (behaviour 6): the text each adds.
fn continuation(
    text: &str,
    lines: &[Range<usize>],
    comments: &[Range<usize>],
    mut comment: usize,
    line: usize,
    column: usize,
) -> Vec<String> {
    let first = &lines[line];
    // Where the item's comment begins in the item's line, in characters.
    let comment_column = text[first.start..comments[comment].start.max(first.start)]
        .chars()
        .count();
    let mut added = Vec::new();
    for next in lines.iter().skip(line + 1).take(MAX_CONTINUATION_LINES) {
        let line_text = &text[next.clone()];
        let Some(at) = byte_of_column(line_text, column) else {
            break;
        };
        // A space or tab at the item's column, then text: indented deeper than the word.
        let rest = &line_text[at..];
        let begin = rest.trim_start_matches([' ', '\t']);
        if begin.len() == rest.len() || begin.is_empty() {
            break;
        }
        let begin = next.start + at + (rest.len() - begin.len());
        if begin >= comments[comment].end {
            // A run of line comments: the text may stand in the comment that follows.
            comment += 1;
            if comments
                .get(comment)
                .is_none_or(|c| begin < c.start || begin >= c.end)
            {
                break;
            }
        }
        let range = &comments[comment];
        let comment_begin = range.start.max(next.start);
        if next.start + at < comment_begin
            || text[next.start..comment_begin].chars().count() > comment_column
            || !text[comment_begin..next.start + at]
                .chars()
                .all(|c| c == ' ' || c == '\t' || LEADERS.contains(&c))
        {
            break;
        }
        let end = next.end.min(range.end);
        // A line with a match of its own starts an item of its own.
        if !matches_in(&text[comment_begin..end]).is_empty() {
            break;
        }
        added.push(without_closer(&text[begin..end], end, range.end).to_owned());
    }
    added
}

/// The items of `text` given the byte ranges of its comments, sorted and not overlapping.
fn items_in(text: &str, comments: &[Range<usize>]) -> Vec<Item> {
    let lines = line_ranges(text);
    let mut items = Vec::new();
    let mut line = 0;
    for (index, comment) in comments.iter().enumerate() {
        while line + 1 < lines.len() && lines[line + 1].start <= comment.start {
            line += 1;
        }
        let mut at = line;
        while at < lines.len() && lines[at].start < comment.end {
            let range = &lines[at];
            let start = range.start.max(comment.start);
            let end = range.end.min(comment.end);
            if start < end {
                for (offset, pattern) in matches_in(&text[start..end]) {
                    let begin = start + offset;
                    let column = text[range.start..begin].chars().count();
                    let matched = without_closer(&text[begin..end], end, comment.end);
                    let matched_chars = matched.chars().count();
                    let mut parts = vec![matched.to_owned()];
                    let more = continuation(text, &lines, comments, index, at, column);
                    let continuation = more.len();
                    parts.extend(more);
                    let whole = &text[range.clone()];
                    let trimmed = whole.trim();
                    let indent = whole.len() - whole.trim_start().len();
                    let mark_start = text[range.start + indent..begin].chars().count();
                    let mark_end = mark_start + matched_chars;
                    items.push(Item {
                        line: at,
                        column,
                        pattern,
                        text: cut(&parts.join(" ")),
                        line_text: cut(trimmed),
                        mark: mark_start.min(MAX_TEXT_CHARS)..mark_end.min(MAX_TEXT_CHARS),
                        continuation,
                    });
                }
            }
            at += 1;
        }
    }
    items.sort_by_key(|i| (i.line, i.column));
    items
}

/// Whether a file of this name is plain text, searched as a whole (behaviour 3).
fn is_plain_text(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("text"))
}

/// The TODO items of `text`, the content of a file named `path` (behaviours 2–6). A text over
/// [`MAX_SYNTAX_BYTES`] gives nothing.
pub fn scan_text(path: &Path, text: &str) -> FileScan {
    if text.len() > MAX_SYNTAX_BYTES || !may_match(text) {
        return FileScan::default();
    }
    if is_plain_text(path) {
        // One comment that is the whole text.
        let whole = 0..text.len();
        return FileScan {
            items: items_in(text, std::slice::from_ref(&whole)),
            not_checked: false,
        };
    }
    match comment_ranges_of(path, text) {
        Some(comments) => FileScan {
            items: items_in(text, &comments),
            not_checked: false,
        },
        None => FileScan {
            items: Vec::new(),
            not_checked: !matches_in(text).is_empty(),
        },
    }
}

/// The items of one file of a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileItems {
    /// The file, relative to the project folder, with `/` between its parts.
    pub path: String,
    /// Its items, by line and column; never empty.
    pub items: Vec<Item>,
}

/// The result of scanning the project (behaviours 1, 7, 8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scan {
    /// The files with items, by path.
    pub files: Vec<FileItems>,
    /// The scan stopped at [`MAX_ITEMS`]: items or files were left out.
    pub truncated: bool,
    /// Files not read: binary, too large or unreadable.
    pub not_read: usize,
    /// Files with a match but without a known comment syntax (behaviour 3).
    pub not_checked: usize,
    /// Files whose content was searched in this scan; the others came from the cache.
    pub searched: usize,
}

impl Scan {
    /// The number of items.
    pub fn count(&self) -> usize {
        self.files.iter().map(|f| f.items.len()).sum()
    }
}

/// What a scan found in a file, and the state of the file it was found in.
#[derive(Debug, Clone)]
struct Cached {
    modified: Option<SystemTime>,
    len: u64,
    found: Found,
}

#[derive(Debug, Clone)]
enum Found {
    Items(Vec<Item>),
    NotRead,
    NotChecked,
}

/// What earlier scans found, per file (behaviour 11).
#[derive(Debug, Default)]
pub struct Cache {
    files: HashMap<PathBuf, Cached>,
}

/// `path` relative to the project with `/` between its parts.
pub fn shown_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn search(path: &Path, text: &str) -> Found {
    if text.len() > MAX_SYNTAX_BYTES {
        return Found::NotRead;
    }
    let scan = scan_text(path, text);
    if scan.not_checked {
        Found::NotChecked
    } else {
        Found::Items(scan.items)
    }
}

/// Scans the project's own files (behaviours 1, 7–11). `unsaved` holds the text of files with
/// unsaved changes, by their path relative to the project folder; `cache` is what earlier scans
/// found and is brought up to date. `None` when `stop` was set: nothing is returned then.
pub fn scan_project(
    workspace: &Workspace,
    unsaved: &BTreeMap<PathBuf, String>,
    cache: &mut Cache,
    stop: &AtomicBool,
) -> Option<Scan> {
    let (candidates, _) = search_candidates(workspace, &SearchScope::default());
    let root = workspace.root();
    let mut scan = Scan::default();
    let mut seen = Vec::with_capacity(candidates.len());
    let mut total = 0;
    for (index, relative) in candidates.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let found = if let Some(text) = unsaved.get(relative) {
            scan.searched += 1;
            search(relative, text)
        } else {
            // The listing names no symbolic link; a link put there since is not followed.
            let Ok(meta) = std::fs::symlink_metadata(root.join(relative)) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().ok();
            let known = cache
                .files
                .get(relative)
                .filter(|c| c.modified == modified && c.len == meta.len());
            match known {
                Some(c) => c.found.clone(),
                None => {
                    scan.searched += 1;
                    let found = match ferris_idea_core::searchable_text(workspace, relative) {
                        Some(text) => search(relative, &text),
                        None => Found::NotRead,
                    };
                    cache.files.insert(
                        relative.clone(),
                        Cached {
                            modified,
                            len: meta.len(),
                            found: found.clone(),
                        },
                    );
                    found
                }
            }
        };
        seen.push(relative);
        match found {
            Found::NotRead => scan.not_read += 1,
            Found::NotChecked => scan.not_checked += 1,
            Found::Items(mut items) if !items.is_empty() => {
                let room = MAX_ITEMS - total;
                let cut = items.len() > room;
                items.truncate(room);
                total += items.len();
                scan.files.push(FileItems {
                    path: shown_path(relative),
                    items,
                });
                if total == MAX_ITEMS {
                    scan.truncated = cut || index + 1 < candidates.len();
                    // What the files after this one hold stays as it is in the cache.
                    return Some(scan);
                }
            }
            Found::Items(_) => {}
        }
    }
    // Files that are gone are forgotten.
    let seen: std::collections::HashSet<&PathBuf> = seen.into_iter().collect();
    cache.files.retain(|path, _| seen.contains(path));
    Some(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places(scan: &FileScan) -> Vec<(usize, usize, &'static str, &str)> {
        scan.items
            .iter()
            .map(|i| (i.line, i.column, i.pattern.name(), i.text.as_str()))
            .collect()
    }

    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("ferris-idea-todo-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root.canonicalize().unwrap())
        }

        fn write(&self, path: &str, text: &str) {
            let at = self.0.join(path);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// SPEC-0334 AK-1.
    #[test]
    fn only_matches_in_comments_are_items() {
        let rust = "\
fn main() {
    // TODO: first thing
    let todo = \"TODO in a string\";
    let x = 1; // fixme later
    /* ToDo block */
    // todos and mytodo and TODO_x are no matches
    todo!();
}
";
        assert_eq!(
            places(&scan_text(Path::new("src/main.rs"), rust)),
            [
                (1, 7, "todo", "TODO: first thing"),
                (3, 18, "fixme", "fixme later"),
                (4, 7, "todo", "ToDo block"),
            ]
        );
        let java = "\
class A {
    /** Fixme: javadoc */
    String s = \"todo\"; // TODO x, FIXME y
}
";
        assert_eq!(
            places(&scan_text(Path::new("A.java"), java)),
            [
                (1, 8, "fixme", "Fixme: javadoc"),
                (2, 26, "todo", "TODO x, FIXME y"),
                (2, 34, "fixme", "FIXME y"),
            ]
        );
        let python = "# todo: one\nx = 'fixme'\n\"\"\"TODO in a docstring\"\"\"\n";
        assert_eq!(
            places(&scan_text(Path::new("a.py"), python)),
            [(0, 2, "todo", "todo: one")]
        );
        let html =
            "<p>todo text</p>\n<!-- TODO: markup -->\n<script>\n// FIXME script\n</script>\n";
        assert_eq!(
            places(&scan_text(Path::new("a.html"), html)),
            [
                (1, 5, "todo", "TODO: markup"),
                (3, 3, "fixme", "FIXME script"),
            ]
        );
        // The row's line and the place of the match in it.
        let scan = scan_text(Path::new("src/main.rs"), rust);
        assert_eq!(scan.items[1].line_text, "let x = 1; // fixme later");
        assert_eq!(scan.items[1].mark, 14..25);
        assert_eq!(scan.items[2].line_text, "/* ToDo block */");
        assert_eq!(scan.items[2].mark, 3..13);
        assert_eq!(Pattern::Todo.expression(), r"\btodo\b.*");
        assert_eq!(Pattern::from_name("fixme"), Some(Pattern::Fixme));
        assert_eq!(Pattern::from_name("TODO"), None);
    }

    /// SPEC-0334 AK-2.
    #[test]
    fn plain_text_is_searched_whole_and_unknown_types_are_not_checked() {
        let text = "notes\n  TODO buy milk\nnothing\n";
        let plain = scan_text(Path::new("notes.txt"), text);
        assert_eq!(places(&plain), [(1, 2, "todo", "TODO buy milk")]);
        assert!(!plain.not_checked);
        let unknown = scan_text(Path::new("notes.xyzzy"), text);
        assert!(unknown.items.is_empty() && unknown.not_checked);
        // Without a match there is nothing to say about an unknown type.
        let quiet = scan_text(Path::new("notes.xyzzy"), "todos only\n");
        assert!(quiet.items.is_empty() && !quiet.not_checked);
    }

    /// SPEC-0334 AK-3.
    #[test]
    fn continuation_lines_join_the_item() {
        let rust = "\
// TODO first
//      second
// third
fn a() {}
    /* FIXME block one
             block two */
// TODO outer
//       FIXME inner
//   not deep enough
";
        let scan = scan_text(Path::new("a.rs"), rust);
        assert_eq!(
            places(&scan),
            [
                (0, 3, "todo", "TODO first second"),
                (4, 7, "fixme", "FIXME block one block two"),
                (6, 3, "todo", "TODO outer"),
                (7, 9, "fixme", "FIXME inner"),
            ]
        );
        assert_eq!(
            scan.items
                .iter()
                .map(|i| i.continuation)
                .collect::<Vec<_>>(),
            [1, 1, 0, 0]
        );
        // Code between the comments ends the item; so does a comment that begins further right.
        let broken =
            "// TODO a\nlet x = 1; //   b\n// TODO c\n        //     d\n# TODO e\n#      f\n";
        assert_eq!(
            places(&scan_text(Path::new("a.rs"), broken)),
            [(0, 3, "todo", "TODO a"), (2, 3, "todo", "TODO c"),]
        );
        // Hash comments continue the same way.
        assert_eq!(
            places(&scan_text(Path::new("a.py"), "# TODO e\n#      f\nx = 1\n")),
            [(0, 2, "todo", "TODO e f")]
        );
        // At most twenty lines.
        let mut long = String::from("// TODO start\n");
        for i in 0..30 {
            long.push_str(&format!("//      l{i}\n"));
        }
        let scan = scan_text(Path::new("a.rs"), &long);
        assert_eq!(scan.items.len(), 1);
        assert_eq!(scan.items[0].continuation, MAX_CONTINUATION_LINES);
    }

    /// SPEC-0334 AK-4.
    #[test]
    fn texts_are_cut_columns_count_characters_and_the_scan_is_capped() {
        let long = format!("// TODO {}\n", "x".repeat(1000));
        let scan = scan_text(Path::new("a.rs"), &long);
        assert_eq!(scan.items[0].text.chars().count(), MAX_TEXT_CHARS + 1);
        assert!(scan.items[0].text.ends_with('…'));
        assert_eq!(scan.items[0].line_text.chars().count(), MAX_TEXT_CHARS + 1);
        assert_eq!(scan.items[0].mark, 3..MAX_TEXT_CHARS);

        let wide = "let ä = \"日本\"; // TODO prüfen — später\r\n/* 日本 fixme: 直す */\r\n";
        let scan = scan_text(Path::new("a.rs"), wide);
        assert_eq!(
            places(&scan),
            [
                (0, 17, "todo", "TODO prüfen — später"),
                (1, 6, "fixme", "fixme: 直す"),
            ]
        );
        assert_eq!(
            scan.items[0].line_text,
            "let ä = \"日本\"; // TODO prüfen — später"
        );
        // A word that only ends in the letters is no match, whatever the alphabet.
        assert!(
            scan_text(Path::new("a.rs"), "// étodo and todoé\n")
                .items
                .is_empty()
        );

        // More items than the cap: exactly the cap is kept, in path order.
        let dir = Dir::new("cap");
        let many = "// TODO x\n".repeat(MAX_ITEMS / 2 + 10);
        dir.write("a.rs", &many);
        dir.write("b.rs", &many);
        dir.write("c.rs", "// TODO never read\n");
        let workspace = Workspace::open(&dir.0).unwrap();
        let scan = scan_project(
            &workspace,
            &BTreeMap::new(),
            &mut Cache::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(scan.count(), MAX_ITEMS);
        assert!(scan.truncated);
        assert_eq!(
            scan.files
                .iter()
                .map(|f| (f.path.as_str(), f.items.len()))
                .collect::<Vec<_>>(),
            [("a.rs", MAX_ITEMS / 2 + 10), ("b.rs", MAX_ITEMS / 2 - 10)]
        );
    }

    /// SPEC-0334 AK-5.
    #[test]
    fn the_scan_reads_own_files_only_and_only_what_changed() {
        let dir = Dir::new("own");
        let outside = Dir::new("own-outside");
        outside.write("secret.rs", "// TODO outside the project\n");
        dir.write(".gitignore", "ignored/\n");
        dir.write("src/b.rs", "// FIXME in b\nfn b() {}\n");
        dir.write("src/a.rs", "fn a() {} // TODO in a\n");
        dir.write("ignored/x.rs", "// TODO ignored\n");
        dir.write("node_modules/pkg/index.js", "// TODO dependency\n");
        dir.write("data.xyzzy", "TODO in an unknown type\n");
        std::fs::write(dir.0.join("blob.rs"), b"// TODO \0 binary\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.0.join("secret.rs"), dir.0.join("src/link.rs")).unwrap();
        let workspace = Workspace::open(&dir.0).unwrap();
        let mut cache = Cache::default();
        let go = AtomicBool::new(false);
        let none = BTreeMap::new();
        let first = scan_project(&workspace, &none, &mut cache, &go).unwrap();
        let shown = |scan: &Scan| -> Vec<String> {
            scan.files
                .iter()
                .flat_map(|f| {
                    f.items
                        .iter()
                        .map(move |i| format!("{}:{} {}", f.path, i.line + 1, i.text))
                })
                .collect()
        };
        assert_eq!(
            shown(&first),
            ["src/a.rs:1 TODO in a", "src/b.rs:1 FIXME in b"]
        );
        assert_eq!((first.not_read, first.not_checked), (1, 1));
        assert!(!first.truncated && first.searched >= 4);

        // Nothing changed: nothing is read again, the result is the same.
        let again = scan_project(&workspace, &none, &mut cache, &go).unwrap();
        assert_eq!(again.searched, 0);
        assert_eq!(shown(&again), shown(&first));
        assert_eq!((again.not_read, again.not_checked), (1, 1));

        // One file changes on disk, one goes: only the changed one is read.
        dir.write("src/a.rs", "// TODO new on top\nfn a() {} // TODO in a\n");
        std::fs::remove_file(dir.0.join("src/b.rs")).unwrap();
        let third = scan_project(&workspace, &none, &mut cache, &go).unwrap();
        assert_eq!(third.searched, 1);
        assert_eq!(
            shown(&third),
            ["src/a.rs:1 TODO new on top", "src/a.rs:2 TODO in a"]
        );

        // Unsaved text of an editor wins over the file and is not kept for the next scan.
        let mut unsaved = BTreeMap::new();
        unsaved.insert(PathBuf::from("src/a.rs"), "// FIXME typed\n".to_owned());
        let typed = scan_project(&workspace, &unsaved, &mut cache, &go).unwrap();
        assert_eq!(shown(&typed), ["src/a.rs:1 FIXME typed"]);
        let saved = scan_project(&workspace, &none, &mut cache, &go).unwrap();
        assert_eq!(shown(&saved), shown(&third));
        assert_eq!(saved.searched, 0);

        // A scan that was told to stop returns nothing.
        assert_eq!(
            scan_project(&workspace, &none, &mut cache, &AtomicBool::new(true)),
            None
        );
    }
}
