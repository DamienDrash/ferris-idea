//! Bookmarks of a project (`SPEC-0331`): lines of project files, each with an optional mnemonic
//! and description, newest first; how they follow edits, renamed and deleted files, and the
//! project file `.ferris-idea/bookmarks.toml` they are kept in. Toolkit-free so the window and the
//! MCP server share it.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Where the bookmarks of a project are kept (behaviour 10).
pub const BOOKMARKS_FILE: &str = ".ferris-idea/bookmarks.toml";

/// The kind in the file's envelope.
pub const KIND: &str = "bookmarks";

/// The mnemonic characters in the order of the grid (behaviour 2).
pub const MNEMONICS: [char; 36] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

/// Longest description, in characters (behaviour 3).
pub const MAX_DESCRIPTION: usize = 200;

/// Most bookmarks read from a file (behaviour 11).
pub const MAX_BOOKMARKS: usize = 10_000;

/// Largest file whose line texts are read for a list.
const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;

/// `c` as a mnemonic: a digit or a letter, letters as capitals; `None` for anything else
/// (behaviour 2).
pub fn mnemonic(c: char) -> Option<char> {
    let c = c.to_ascii_uppercase();
    MNEMONICS.contains(&c).then_some(c)
}

/// `text` as a description: one line, trimmed, at most [`MAX_DESCRIPTION`] characters; `None`
/// when nothing is left (behaviour 3).
pub fn description(text: &str) -> Option<String> {
    let line: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let short: String = line.trim().chars().take(MAX_DESCRIPTION).collect();
    let short = short.trim_end();
    (!short.is_empty()).then(|| short.to_owned())
}

/// A bookmark on a line of a project file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// The file, relative to the project, `/` between the parts.
    pub file: String,
    /// 0-based line.
    pub line: usize,
    /// The mnemonic, unique in the project.
    pub mnemonic: Option<char>,
    /// The description.
    pub description: Option<String>,
}

impl Bookmark {
    /// `file:line` with the 1-based line, as messages name a bookmark.
    pub fn place(&self) -> String {
        format!("{}:{}", self.file, self.line + 1)
    }
}

/// What to do with an optional property of a bookmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change<T> {
    /// Leave it as it is.
    Keep,
    /// Remove it.
    Clear,
    /// Give it this value.
    Set(T),
}

impl<T> Change<T> {
    fn apply(self, value: &mut Option<T>) {
        match self {
            Change::Keep => {}
            Change::Clear => *value = None,
            Change::Set(v) => *value = Some(v),
        }
    }
}

/// What `ToggleBookmark0`…`9` did (behaviour 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MnemonicToggle {
    /// The line's bookmark had the mnemonic and was removed.
    Removed,
    /// The line has the mnemonic now; it was taken from the bookmark at this file and 0-based
    /// line, if another one had it.
    Set(Option<(String, usize)>),
}

/// One edit of a text as the marked lines see it (behaviours 5–7): the line it starts on and
/// what it deleted and inserted there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    /// 0-based line of the edit's start.
    pub line: usize,
    /// Nothing but white space stands before the edit on its line.
    pub head_blank: bool,
    /// After the edit, nothing follows the inserted text on its line.
    pub tail_empty: bool,
    /// The deleted text.
    pub deleted: String,
    /// The inserted text.
    pub inserted: String,
}

/// Where a bookmark on the 0-based line `at` is after `edit`; `None` when its line was deleted
/// (behaviours 5–7).
pub fn moved_line(at: usize, edit: &LineEdit) -> Option<usize> {
    let removed = edit.deleted.matches('\n').count();
    let added = edit.inserted.matches('\n').count();
    let first = edit.line;
    if at < first {
        return Some(at);
    }
    if at > first + removed {
        return Some(at + added - removed);
    }
    let i = at - first;
    if removed == 0 {
        // A line break put before the line's text pushes the text, and its bookmark, down.
        let pushed = added > 0 && edit.head_blank && !edit.tail_empty;
        return Some(if pushed { first + added } else { first });
    }
    let deleted: Vec<&str> = edit.deleted.split('\n').collect();
    // What is left of the last touched line: nothing of it was deleted, or text follows.
    let last_kept = deleted[removed].is_empty() || !edit.tail_empty;
    if edit.inserted.is_empty() {
        return match i {
            0 => (!edit.head_blank).then_some(first),
            _ if i < removed => None,
            _ => last_kept.then_some(first),
        };
    }
    if i == 0 && !edit.head_blank {
        return Some(first);
    }
    if i == removed && last_kept {
        return Some(first + added);
    }
    // A line replaced as a whole: the nearest new line with the same content, else the same
    // distance from the start of the replaced part.
    let inserted: Vec<&str> = edit.inserted.split('\n').collect();
    let wanted = deleted[i].trim();
    let same = (0..=added)
        .filter(|&j| (j > 0 || edit.head_blank) && (j < added || edit.tail_empty))
        .filter(|&j| !wanted.is_empty() && inserted[j].trim() == wanted)
        .min_by_key(|&j| (j.abs_diff(i), j));
    Some(first + same.unwrap_or(i.min(added)))
}

/// The bookmarks of a project, newest first (behaviour 4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bookmarks {
    list: Vec<Bookmark>,
}

impl Bookmarks {
    /// The bookmarks, newest first.
    pub fn list(&self) -> &[Bookmark] {
        &self.list
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    fn index(&self, file: &str, line: usize) -> Option<usize> {
        self.list
            .iter()
            .position(|b| b.line == line && b.file == file)
    }

    /// The bookmark on the 0-based `line` of `file`.
    pub fn at(&self, file: &str, line: usize) -> Option<&Bookmark> {
        self.index(file, line).map(|i| &self.list[i])
    }

    /// The bookmark with the mnemonic `m`.
    pub fn with_mnemonic(&self, m: char) -> Option<&Bookmark> {
        let m = mnemonic(m)?;
        self.list.iter().find(|b| b.mnemonic == Some(m))
    }

    /// The bookmarks of `file`, by line.
    pub fn in_file(&self, file: &str) -> Vec<&Bookmark> {
        let mut found: Vec<&Bookmark> = self.list.iter().filter(|b| b.file == file).collect();
        found.sort_by_key(|b| b.line);
        found
    }

    /// The bookmarks of `file` alone, in their order.
    pub fn of_file(&self, file: &str) -> Bookmarks {
        Bookmarks {
            list: self
                .list
                .iter()
                .filter(|b| b.file == file)
                .cloned()
                .collect(),
        }
    }

    /// The mnemonics in use.
    pub fn mnemonics(&self) -> BTreeSet<char> {
        self.list.iter().filter_map(|b| b.mnemonic).collect()
    }

    /// Takes the mnemonic `m` from the bookmark that has it, unless that is the one on `line` of
    /// `file`; returns where it was taken from.
    fn take_mnemonic(&mut self, m: char, file: &str, line: usize) -> Option<(String, usize)> {
        let other = self
            .list
            .iter_mut()
            .find(|b| b.mnemonic == Some(m) && !(b.file == file && b.line == line))?;
        other.mnemonic = None;
        Some((other.file.clone(), other.line))
    }

    /// `ToggleBookmark` (behaviour 15): removes the bookmark on `line` of `file`, or sets one
    /// without mnemonic; returns whether the line has a bookmark now.
    pub fn toggle(&mut self, file: &str, line: usize) -> bool {
        if self.remove(file, line).is_some() {
            return false;
        }
        self.set(file, line, Change::Keep, Change::Keep);
        true
    }

    /// Sets a bookmark on `line` of `file` or changes the one that is there (behaviours 2, 3,
    /// 16, 29). A mnemonic another bookmark has is taken from it; returns that bookmark's place.
    /// A character that is no mnemonic and an empty description count as none.
    pub fn set(
        &mut self,
        file: &str,
        line: usize,
        mnemonic_change: Change<char>,
        description_change: Change<String>,
    ) -> Option<(String, usize)> {
        let mnemonic_change = match mnemonic_change {
            Change::Set(c) => mnemonic(c).map_or(Change::Clear, Change::Set),
            other => other,
        };
        let description_change = match description_change {
            Change::Set(d) => description(&d).map_or(Change::Clear, Change::Set),
            other => other,
        };
        let taken = match mnemonic_change {
            Change::Set(m) => self.take_mnemonic(m, file, line),
            _ => None,
        };
        let i = match self.index(file, line) {
            Some(i) => i,
            None => {
                self.list.insert(
                    0,
                    Bookmark {
                        file: file.to_owned(),
                        line,
                        mnemonic: None,
                        description: None,
                    },
                );
                0
            }
        };
        mnemonic_change.apply(&mut self.list[i].mnemonic);
        description_change.apply(&mut self.list[i].description);
        taken
    }

    /// `ToggleBookmark0`…`9` (behaviour 17): removes the line's bookmark when it has the mnemonic
    /// `m`, else gives the line a bookmark with it. `None` when `m` is no mnemonic.
    pub fn toggle_mnemonic(&mut self, file: &str, line: usize, m: char) -> Option<MnemonicToggle> {
        let m = mnemonic(m)?;
        if self.at(file, line).is_some_and(|b| b.mnemonic == Some(m)) {
            self.remove(file, line);
            return Some(MnemonicToggle::Removed);
        }
        Some(MnemonicToggle::Set(self.set(
            file,
            line,
            Change::Set(m),
            Change::Keep,
        )))
    }

    /// Removes the bookmark on `line` of `file` and returns it.
    pub fn remove(&mut self, file: &str, line: usize) -> Option<Bookmark> {
        self.index(file, line).map(|i| self.list.remove(i))
    }

    /// The line of the next bookmark of `file` below `line` (`forward`) or above it, going on
    /// from the other end; `None` without a bookmark in the file (behaviour 19).
    pub fn next_in_file(&self, file: &str, line: usize, forward: bool) -> Option<usize> {
        let lines: Vec<usize> = self.in_file(file).iter().map(|b| b.line).collect();
        if forward {
            lines.iter().find(|l| **l > line).or(lines.first()).copied()
        } else {
            lines
                .iter()
                .rev()
                .find(|l| **l < line)
                .or(lines.last())
                .copied()
        }
    }

    /// Moves the bookmarks of `file` with `edit` (behaviours 5–7); returns whether one changed.
    /// When two come to lie on one line, the one of the upper line stays.
    pub fn edited(&mut self, file: &str, edit: &LineEdit) -> bool {
        let mut own: Vec<usize> = (0..self.list.len())
            .filter(|&i| self.list[i].file == file)
            .collect();
        own.sort_by_key(|&i| self.list[i].line);
        let mut taken = BTreeSet::new();
        let mut gone = Vec::new();
        let mut changed = false;
        for i in own {
            match moved_line(self.list[i].line, edit) {
                Some(line) if taken.insert(line) => {
                    changed |= line != self.list[i].line;
                    self.list[i].line = line;
                }
                _ => gone.push(i),
            }
        }
        changed |= !gone.is_empty();
        gone.sort_unstable();
        for i in gone.into_iter().rev() {
            self.list.remove(i);
        }
        changed
    }

    /// The file or folder `from` is now `to` (both relative to the project): its bookmarks follow
    /// (behaviour 9). Returns whether one changed.
    pub fn path_moved(&mut self, from: &str, to: &str) -> bool {
        let mut changed = false;
        for b in &mut self.list {
            if let Some(file) = below(&b.file, from).map(|rest| format!("{to}{rest}")) {
                b.file = file;
                changed = true;
            }
        }
        if changed {
            // A line has one bookmark; the newer one stays.
            let mut seen = BTreeSet::new();
            self.list.retain(|b| seen.insert((b.file.clone(), b.line)));
        }
        changed
    }

    /// The file or folder `path` (relative to the project) was deleted: its bookmarks go
    /// (behaviour 9). Returns whether one went.
    pub fn path_deleted(&mut self, path: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|b| below(&b.file, path).is_none());
        self.list.len() != before
    }

    /// The bookmarks in the text of a bookmarks file (behaviours 10–11); the reason when the
    /// file cannot be read.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = crate::format::read(text, Some(KIND), crate::format::Use::Project)
            .and_then(|d| d.body_as())
            .map_err(|p| crate::format::problems_text(&p))?;
        let mut out = Self::default();
        let mut used = BTreeSet::new();
        for e in file.bookmark {
            if out.list.len() == MAX_BOOKMARKS {
                break;
            }
            let Some(name) = clean(&e.file) else {
                continue;
            };
            if e.line == 0 || out.index(&name, e.line - 1).is_some() {
                continue;
            }
            let own = e
                .mnemonic
                .as_deref()
                .and_then(|m| {
                    let mut chars = m.chars();
                    chars.next().filter(|_| chars.next().is_none())
                })
                .and_then(mnemonic)
                .filter(|m| used.insert(*m));
            out.list.push(Bookmark {
                file: name,
                line: e.line - 1,
                mnemonic: own,
                description: e.description.as_deref().and_then(description),
            });
        }
        Ok(out)
    }

    /// The text of the bookmarks file.
    pub fn to_text(&self) -> Result<String, String> {
        let file = File {
            bookmark: self
                .list
                .iter()
                .map(|b| Entry {
                    file: b.file.clone(),
                    line: b.line + 1,
                    mnemonic: b.mnemonic.map(String::from),
                    description: b.description.clone(),
                })
                .collect(),
        };
        crate::format::write(&crate::format::Envelope::current(KIND), &file)
    }
}

/// The rest of `file` after the path `prefix` when `file` is `prefix` or lies below it.
fn below<'a>(file: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = file.strip_prefix(prefix)?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}

/// `file` as a path inside the project with `/` between its parts; `None` when it is absolute,
/// empty or leaves the project.
pub fn clean(file: &str) -> Option<String> {
    let path = Path::new(file);
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_str()?.to_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// One `[[bookmark]]` of the file.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    file: String,
    line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mnemonic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    bookmark: Vec<Entry>,
}

/// `path` relative to the project at `root`, `/` between the parts; `None` for a path outside
/// the project (behaviour 1).
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let slash = |p: &Path| clean(&p.to_string_lossy().replace('\\', "/"));
    if let Some(rel) = path.strip_prefix(root).ok().and_then(slash) {
        return Some(rel);
    }
    // The folder or the path through a link, or with `..` in it.
    let root = root.canonicalize().ok()?;
    if let Some(rel) = path.strip_prefix(&root).ok().and_then(slash) {
        return Some(rel);
    }
    let path = path.canonicalize().ok()?;
    path.strip_prefix(&root).ok().and_then(slash)
}

/// The bookmarks saved for the project at `root`; none when the file is missing or cannot be
/// read (a newer format, another kind).
pub fn load(root: &Path) -> Bookmarks {
    std::fs::read_to_string(root.join(BOOKMARKS_FILE))
        .ok()
        .and_then(|t| Bookmarks::parse(&t).ok())
        .unwrap_or_default()
}

/// Saves `bookmarks` for the project at `root`; removes the file when there are none. A file of
/// a newer format or of another kind is not overwritten (behaviour 10).
pub fn save(root: &Path, bookmarks: &Bookmarks) -> std::io::Result<()> {
    let file = root.join(BOOKMARKS_FILE);
    crate::format::prepare_write(&file, KIND)?;
    if bookmarks.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let text = bookmarks.to_text().map_err(std::io::Error::other)?;
    crate::write_atomic(&file, text)
}

/// The file or folder `from` of the project at `root` was renamed or moved to `to`: the saved
/// bookmarks follow (behaviour 9). Called by the file operations; does nothing without bookmarks.
pub fn follow_move(root: &Path, from: &Path, to: &Path) {
    if !root.join(BOOKMARKS_FILE).is_file() {
        return;
    }
    let (Some(from), Some(to)) = (relative(root, from), relative(root, to)) else {
        return;
    };
    let mut bookmarks = load(root);
    if bookmarks.path_moved(&from, &to) {
        let _ = save(root, &bookmarks);
    }
}

/// The file or folder `path` of the project at `root` was deleted: its saved bookmarks go
/// (behaviour 9).
pub fn follow_delete(root: &Path, path: &Path) {
    if !root.join(BOOKMARKS_FILE).is_file() {
        return;
    }
    let Some(path) = relative(root, path) else {
        return;
    };
    let mut bookmarks = load(root);
    if bookmarks.path_deleted(&path) {
        let _ = save(root, &bookmarks);
    }
}

/// A bookmark as lists show it (behaviours 8, 20, 23, 28).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The bookmark.
    pub bookmark: Bookmark,
    /// Its line's text without the white space at its ends; `None` past the end of the file or
    /// when the file is not text that is read.
    pub text: Option<String>,
    /// The file is not there.
    pub missing: bool,
}

impl Listed {
    /// What a row says after the line number: the description, else the line's text.
    pub fn label(&self) -> &str {
        self.bookmark
            .description
            .as_deref()
            .or(self.text.as_deref())
            .unwrap_or_default()
    }
}

/// `bookmarks` with their line texts, newest first. `open` gives the text of a file that is open
/// in an editor (by its path relative to the project); other files are read from disk.
pub fn listed(
    root: &Path,
    bookmarks: &Bookmarks,
    open: &dyn Fn(&str) -> Option<String>,
) -> Vec<Listed> {
    let mut texts: std::collections::BTreeMap<&str, Option<Option<String>>> = Default::default();
    bookmarks
        .list()
        .iter()
        .map(|b| {
            // `None`: missing; `Some(None)`: there, but not read.
            let text = texts.entry(b.file.as_str()).or_insert_with(|| {
                open(&b.file).map(Some).or_else(|| {
                    let path = root.join(&b.file);
                    let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
                    Some(
                        (meta.len() <= MAX_TEXT_BYTES)
                            .then(|| std::fs::read_to_string(&path).ok())
                            .flatten(),
                    )
                })
            });
            Listed {
                bookmark: b.clone(),
                missing: text.is_none(),
                text: text
                    .as_ref()
                    .and_then(|t| t.as_deref())
                    .and_then(|t| t.split('\n').nth(b.line))
                    .map(|l| l.trim().to_owned()),
            }
        })
        .collect()
}

/// The bookmarks of an open project with the state of their file on disk, so a change another
/// writer made is noticed (behaviour 13).
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    bookmarks: Bookmarks,
    stamp: Option<(SystemTime, u64)>,
}

impl Store {
    /// The saved bookmarks of the project at `root`.
    pub fn open(root: &Path) -> Self {
        let mut store = Self {
            root: root.to_path_buf(),
            bookmarks: Bookmarks::default(),
            stamp: None,
        };
        store.read();
        store
    }

    fn stamp_now(&self) -> Option<(SystemTime, u64)> {
        let meta = std::fs::metadata(self.root.join(BOOKMARKS_FILE)).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    fn read(&mut self) {
        self.stamp = self.stamp_now();
        self.bookmarks = load(&self.root);
    }

    /// The project folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The bookmarks.
    pub fn bookmarks(&self) -> &Bookmarks {
        &self.bookmarks
    }

    /// Reads the file again when it changed on disk; returns whether the bookmarks differ now.
    pub fn refresh(&mut self) -> bool {
        if self.stamp_now() == self.stamp {
            return false;
        }
        let before = std::mem::take(&mut self.bookmarks);
        self.read();
        before != self.bookmarks
    }

    /// Changes the bookmarks with `change` and saves them when they differ afterwards; returns
    /// what `change` returned and whether they differ.
    pub fn change<R>(&mut self, change: impl FnOnce(&mut Bookmarks) -> R) -> (R, bool) {
        let before = self.bookmarks.clone();
        let result = change(&mut self.bookmarks);
        let changed = before != self.bookmarks;
        if changed {
            let _ = save(&self.root, &self.bookmarks);
            self.stamp = self.stamp_now();
        }
        (result, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-bookmarks-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn edit(line: usize, head_blank: bool, tail_empty: bool, del: &str, ins: &str) -> LineEdit {
        LineEdit {
            line,
            head_blank,
            tail_empty,
            deleted: del.into(),
            inserted: ins.into(),
        }
    }

    fn lines(b: &Bookmarks, file: &str) -> Vec<usize> {
        b.in_file(file).iter().map(|b| b.line).collect()
    }

    /// SPEC-0331 AK-1: lines inserted above shift a bookmark, a line deleted as a whole takes its
    /// bookmark with it.
    #[test]
    fn bookmarks_follow_inserted_lines_and_go_with_a_deleted_line() {
        let mut b = Bookmarks::default();
        for line in [6, 11, 15] {
            b.toggle("src/Inventory.java", line);
        }
        b.toggle("src/Other.java", 11);
        // Two lines typed at the end of line 9 (0-based 8).
        assert!(b.edited(
            "src/Inventory.java",
            &edit(8, false, true, "", "\n    int a;\n    int b;")
        ));
        assert_eq!(lines(&b, "src/Inventory.java"), [6, 13, 17]);
        // Line 7 (0-based 6) deleted from its start to the start of the next line.
        assert!(b.edited(
            "src/Inventory.java",
            &edit(6, true, false, "    int count;\n", "")
        ));
        assert_eq!(lines(&b, "src/Inventory.java"), [12, 16]);
        assert_eq!(lines(&b, "src/Other.java"), [11], "other files stay");
        // An edit below every bookmark, and one inside a line, change nothing.
        assert!(!b.edited("src/Inventory.java", &edit(40, false, true, "", "\n")));
        assert!(!b.edited("src/Inventory.java", &edit(12, false, false, "ab", "c")));
    }

    /// SPEC-0331 AK-2: line breaks at the start of a line, joins, deleted blocks and replaced
    /// blocks.
    #[test]
    fn a_bookmark_stays_with_the_text_of_its_line() {
        // Enter at the start of the line and after its indentation: the text goes down.
        assert_eq!(moved_line(4, &edit(4, true, false, "", "\n")), Some(5));
        assert_eq!(moved_line(4, &edit(4, true, false, "", "\n    ")), Some(5));
        // Enter in the middle or at the end: the bookmark stays on the first part.
        assert_eq!(moved_line(4, &edit(4, false, false, "", "\n")), Some(4));
        assert_eq!(moved_line(4, &edit(4, false, true, "", "\n")), Some(4));
        // Enter on an empty line keeps the bookmark where it is.
        assert_eq!(moved_line(4, &edit(4, true, true, "", "\n")), Some(4));
        // Backspace at the start of line 5 joins it to line 4: both stay on the joined line.
        let join = edit(4, false, false, "\n", "");
        assert_eq!(moved_line(4, &join), Some(4));
        assert_eq!(moved_line(5, &join), Some(4));
        assert_eq!(moved_line(6, &join), Some(5));
        // A block deleted from the middle of line 2 to the middle of line 5.
        let block = edit(2, false, false, "bc\nline 3\nline 4\nde", "");
        assert_eq!(moved_line(2, &block), Some(2));
        assert_eq!(moved_line(3, &block), None);
        assert_eq!(moved_line(4, &block), None);
        assert_eq!(moved_line(5, &block), Some(2));
        assert_eq!(moved_line(6, &block), Some(3));
        // From the end of line 2 through the end of line 3: line 3 is gone with its content.
        let through = edit(2, false, true, "\nline 3", "");
        assert_eq!(moved_line(2, &through), Some(2));
        assert_eq!(moved_line(3, &through), None);
        // Whole lines 2 and 3 deleted: line 4 moves up untouched.
        let whole = edit(2, true, false, "line 2\nline 3\n", "");
        assert_eq!(moved_line(2, &whole), None);
        assert_eq!(moved_line(3, &whole), None);
        assert_eq!(moved_line(4, &whole), Some(2));
        // A replaced block: the line with the same text, else the same distance, at most the end.
        let replaced = edit(
            0,
            true,
            true,
            "a();\n  b();\nc();\nd();",
            "x();\na();\nb();\nq();\nr();",
        );
        assert_eq!(
            moved_line(0, &replaced),
            Some(1),
            "a() is on the second line"
        );
        assert_eq!(moved_line(1, &replaced), Some(2), "b() without its indent");
        assert_eq!(
            moved_line(2, &replaced),
            Some(2),
            "c() is gone: same distance"
        );
        assert_eq!(moved_line(3, &replaced), Some(3));
        assert_eq!(moved_line(9, &replaced), Some(10));
        let shorter = edit(1, true, true, "a\nb\nc\nd", "z");
        assert_eq!(moved_line(4, &shorter), Some(1));
        // The part of the first and of the last line that stays keeps the bookmark.
        let inner = edit(1, false, false, "x\nfoo\ny", "1\n2\n3\n4");
        assert_eq!(moved_line(1, &inner), Some(1));
        assert_eq!(moved_line(3, &inner), Some(4));
        assert_eq!(moved_line(2, &inner), Some(2));

        // Two bookmarks on one line: the upper one stays.
        let mut b = Bookmarks::default();
        b.set("a.rs", 4, Change::Set('1'), Change::Keep);
        b.set("a.rs", 5, Change::Set('2'), Change::Keep);
        assert!(b.edited("a.rs", &join));
        assert_eq!(b.list().len(), 1);
        assert_eq!(b.at("a.rs", 4).unwrap().mnemonic, Some('1'));
    }

    /// SPEC-0331 AK-3: a mnemonic is unique in the project.
    #[test]
    fn a_mnemonic_is_taken_from_the_bookmark_that_had_it() {
        let mut b = Bookmarks::default();
        assert_eq!(b.set("a.rs", 2, Change::Set('3'), Change::Keep), None);
        assert_eq!(
            b.set(
                "b.rs",
                9,
                Change::Set('3'),
                Change::Set("  the\nloop ".into())
            ),
            Some(("a.rs".into(), 2))
        );
        assert_eq!(b.at("a.rs", 2).unwrap().mnemonic, None, "still a bookmark");
        assert_eq!(b.with_mnemonic('3').unwrap().place(), "b.rs:10");
        assert_eq!(
            b.at("b.rs", 9).unwrap().description.as_deref(),
            Some("the loop")
        );
        // Letters in either case, kept as capitals; other characters are no mnemonic.
        b.set("a.rs", 2, Change::Set('q'), Change::Keep);
        assert_eq!(b.at("a.rs", 2).unwrap().mnemonic, Some('Q'));
        assert_eq!(b.with_mnemonic('q').unwrap().file, "a.rs");
        assert_eq!(mnemonic('!'), None);
        assert_eq!(mnemonic('ä'), None);
        assert_eq!(b.toggle_mnemonic("a.rs", 2, '?'), None);
        // Setting the same mnemonic on its own line takes nothing away.
        assert_eq!(b.set("a.rs", 2, Change::Set('Q'), Change::Keep), None);
        assert_eq!(b.mnemonics().into_iter().collect::<String>(), "3Q");
        // The newest is first; changing a bookmark keeps its place.
        assert_eq!(b.list()[0].file, "b.rs");
        // A description is one line of at most 200 characters.
        assert_eq!(description(" \n "), None);
        assert_eq!(description(&"x".repeat(300)).unwrap().len(), 200);
        b.set("a.rs", 2, Change::Keep, Change::Set(" ".into()));
        assert_eq!(b.at("a.rs", 2).unwrap().description, None);
    }

    /// SPEC-0331 AK-6: the toggles and the next and previous bookmark of a file.
    #[test]
    fn toggles_and_the_way_through_a_file() {
        let mut b = Bookmarks::default();
        assert!(b.toggle("a.rs", 6));
        assert!(!b.toggle("a.rs", 6));
        assert!(b.is_empty());
        // ToggleBookmark3: sets, moves the mnemonic, removes.
        assert_eq!(
            b.toggle_mnemonic("a.rs", 6, '3'),
            Some(MnemonicToggle::Set(None))
        );
        b.toggle("a.rs", 11);
        assert_eq!(
            b.toggle_mnemonic("a.rs", 11, '3'),
            Some(MnemonicToggle::Set(Some(("a.rs".into(), 6))))
        );
        assert_eq!(b.list().len(), 2);
        assert_eq!(
            b.toggle_mnemonic("a.rs", 11, '3'),
            Some(MnemonicToggle::Removed)
        );
        assert_eq!(lines(&b, "a.rs"), [6]);
        // A plain toggle removes a bookmark with a mnemonic too.
        b.set("a.rs", 6, Change::Set('7'), Change::Keep);
        assert!(!b.toggle("a.rs", 6));
        for line in [15, 6, 11] {
            b.toggle("a.rs", line);
        }
        assert_eq!(b.next_in_file("a.rs", 6, true), Some(11));
        assert_eq!(b.next_in_file("a.rs", 15, true), Some(6), "wraps around");
        assert_eq!(b.next_in_file("a.rs", 0, false), Some(15), "wraps around");
        assert_eq!(b.next_in_file("a.rs", 12, false), Some(11));
        assert_eq!(b.next_in_file("b.rs", 0, true), None);
    }

    /// SPEC-0331 AK-4: the file, its envelope, what a reload gives and what reading drops.
    #[test]
    fn bookmarks_survive_a_reload() {
        let root = project("reload");
        let mut store = Store::open(&root);
        assert!(store.bookmarks().is_empty());
        store.change(|b| {
            b.toggle("src/main/java/lab/Inventory.java", 6);
            b.toggle("src/main/java/lab/Inventory.java", 11);
            b.set(
                "src/main/java/lab/Inventory.java",
                15,
                Change::Set('1'),
                Change::Set("where the stock is counted".into()),
            );
        });
        let text = std::fs::read_to_string(root.join(BOOKMARKS_FILE)).unwrap();
        assert!(text.starts_with("[ferris-idea]\nkind = \"bookmarks\"\nformat = 1\n"));
        assert!(text.contains("line = 16\nmnemonic = \"1\"\n"));
        // Another process reads the same: lines, mnemonics, descriptions, newest first.
        let again = Store::open(&root);
        assert_eq!(again.bookmarks(), store.bookmarks());
        let order: Vec<usize> = again.bookmarks().list().iter().map(|b| b.line).collect();
        assert_eq!(order, [15, 11, 6]);
        // A change by another writer is noticed.
        let mut other = Store::open(&root);
        other.change(|b| b.remove("src/main/java/lab/Inventory.java", 11));
        assert!(store.refresh());
        assert_eq!(store.bookmarks(), other.bookmarks());
        assert!(!store.refresh());
        // The last bookmark takes the file with it.
        store.change(|b| {
            b.toggle("src/main/java/lab/Inventory.java", 6);
            b.toggle("src/main/java/lab/Inventory.java", 15);
        });
        assert!(!root.join(BOOKMARKS_FILE).exists());

        // Reading keeps only what it can stand for.
        let read = Bookmarks::parse(
            "[ferris-idea]\nkind = \"bookmarks\"\nformat = 1\n\n\
             [[bookmark]]\nfile = \"a.rs\"\nline = 3\nmnemonic = \"x\"\n\n\
             [[bookmark]]\nfile = \"../out.rs\"\nline = 1\n\n\
             [[bookmark]]\nfile = \"/etc/passwd\"\nline = 1\n\n\
             [[bookmark]]\nfile = \"a.rs\"\nline = 3\ndescription = \"again\"\n\n\
             [[bookmark]]\nfile = \"b.rs\"\nline = 1\nmnemonic = \"X\"\n\n\
             [[bookmark]]\nfile = \"./c.rs\"\nline = 2\nmnemonic = \"!!\"\n",
        )
        .unwrap();
        let kept: Vec<(String, Option<char>)> = read
            .list()
            .iter()
            .map(|b| (b.place(), b.mnemonic))
            .collect();
        assert_eq!(
            kept,
            [
                ("a.rs:3".to_owned(), Some('X')),
                ("b.rs:1".to_owned(), None),
                ("c.rs:2".to_owned(), None),
            ]
        );
        // A newer format and another kind are refused, and not overwritten.
        let newer = "[ferris-idea]\nkind = \"bookmarks\"\nformat = 99\n";
        assert!(Bookmarks::parse(newer).is_err());
        assert!(Bookmarks::parse("[ferris-idea]\nkind = \"breakpoints\"\nformat = 1\n").is_err());
        std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
        std::fs::write(root.join(BOOKMARKS_FILE), newer).unwrap();
        let mut b = Bookmarks::default();
        b.toggle("a.rs", 0);
        assert!(save(&root, &b).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join(BOOKMARKS_FILE)).unwrap(),
            newer
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0331 AK-5: renamed, moved and deleted files and folders, through the file
    /// operations.
    #[test]
    fn bookmarks_follow_a_renamed_file_and_go_with_a_deleted_one() {
        let root = project("rename");
        std::fs::create_dir_all(root.join("src/shop")).unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("src/shop/Cart.java"), "a\nb\nc\n").unwrap();
        std::fs::write(root.join("src/shop/Cartoon.java"), "a\nb\n").unwrap();
        std::fs::write(root.join("notes.txt"), "n\n").unwrap();
        let mut store = Store::open(&root);
        store.change(|b| {
            b.set("src/shop/Cart.java", 1, Change::Set('1'), Change::Keep);
            b.toggle("src/shop/Cartoon.java", 0);
            b.toggle("notes.txt", 0);
        });
        let files = |store: &mut Store| -> Vec<String> {
            store.refresh();
            store
                .bookmarks()
                .list()
                .iter()
                .map(Bookmark::place)
                .collect()
        };
        // A file renamed in its folder: only that file, not one whose name starts the same.
        crate::file_ops::rename(&root, &root.join("src/shop/Cart.java"), "Basket.java").unwrap();
        assert_eq!(
            files(&mut store),
            [
                "notes.txt:1",
                "src/shop/Cartoon.java:1",
                "src/shop/Basket.java:2"
            ]
        );
        assert_eq!(store.bookmarks().with_mnemonic('1').unwrap().line, 1);
        // Its folder renamed, then moved into another folder.
        crate::file_ops::rename(&root, &root.join("src/shop"), "store").unwrap();
        crate::file_ops::transfer(
            &root,
            &root.join("src/store"),
            &root.join("lib"),
            None,
            crate::file_ops::Transfer::Move,
        )
        .unwrap();
        assert_eq!(
            files(&mut store),
            [
                "notes.txt:1",
                "lib/store/Cartoon.java:1",
                "lib/store/Basket.java:2"
            ]
        );
        // A copy keeps the bookmarks where they are.
        crate::file_ops::transfer(
            &root,
            &root.join("notes.txt"),
            &root.join("lib"),
            None,
            crate::file_ops::Transfer::Copy,
        )
        .unwrap();
        assert_eq!(files(&mut store).len(), 3);
        // A deleted file, then a deleted folder.
        crate::file_ops::delete(&root, &root.join("lib/store/Cartoon.java")).unwrap();
        assert_eq!(
            files(&mut store),
            ["notes.txt:1", "lib/store/Basket.java:2"]
        );
        crate::file_ops::delete(&root, &root.join("lib")).unwrap();
        assert_eq!(files(&mut store), ["notes.txt:1"]);
        // Outside the project nothing is a bookmark's file.
        assert_eq!(relative(&root, Path::new("/etc/hosts")), None);
        assert_eq!(
            relative(&root, &root.join("src/../notes.txt")).as_deref(),
            Some("notes.txt")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0331 AK-7 (core part): rows with the line's text, the description in its place, a
    /// line past the end, a missing file, and the text of an open editor.
    #[test]
    fn listed_bookmarks_carry_their_line_text() {
        let root = project("listed");
        std::fs::write(root.join("a.rs"), "fn main() {\n    run();  \n}\n").unwrap();
        let mut b = Bookmarks::default();
        b.toggle("gone.rs", 0);
        b.toggle("a.rs", 40);
        b.set("a.rs", 0, Change::Keep, Change::Set("entry".into()));
        b.set("a.rs", 1, Change::Set('2'), Change::Keep);
        let rows = listed(&root, &b, &|_| None);
        let shown: Vec<(String, &str, bool)> = rows
            .iter()
            .map(|r| (r.bookmark.place(), r.label(), r.missing))
            .collect();
        assert_eq!(
            shown,
            [
                ("a.rs:2".to_owned(), "run();", false),
                ("a.rs:1".to_owned(), "entry", false),
                ("a.rs:41".to_owned(), "", false),
                ("gone.rs:1".to_owned(), "", true),
            ]
        );
        assert_eq!(rows[1].text.as_deref(), Some("fn main() {"));
        // An open editor's text wins over the file on disk.
        let open = listed(&root, &b, &|f| {
            (f == "a.rs").then(|| "x\n  unsaved()\n".to_owned())
        });
        assert_eq!(open[0].label(), "unsaved()");
        let _ = std::fs::remove_dir_all(&root);
    }
}
