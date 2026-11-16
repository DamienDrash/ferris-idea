//! What the TODO tool window shows (`SPEC-0334` behaviours 14–19, 22): the rows of its tree for
//! a grouping and a filter, their counts and labels, and the way through the items. No window
//! is needed here; the items come from [`idecraft_languages::todo`], the view is
//! [`crate::todo_view`].

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use idecraft_languages::todo::{Item, MAX_ITEMS, Pattern};

/// The filter of the toolbar (behaviour 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// Every item.
    #[default]
    All,
    /// The items of one pattern.
    Only(Pattern),
}

impl Filter {
    /// The choices in the order of the toolbar.
    pub const ALL: [Filter; 3] = [
        Filter::All,
        Filter::Only(Pattern::Todo),
        Filter::Only(Pattern::Fixme),
    ];

    /// The label of the choice.
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Only(p) => p.name(),
        }
    }

    /// The name used in element ids: `all`, `todo`, `fixme`.
    pub fn id(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Only(p) => p.name(),
        }
    }

    /// Whether the filter shows `item`.
    pub fn shows(self, item: &Item) -> bool {
        match self {
            Filter::All => true,
            Filter::Only(p) => item.pattern == p,
        }
    }
}

/// The scope of the window (behaviour 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// The project's own files.
    #[default]
    Project,
    /// The file of the active editor.
    File,
}

impl Scope {
    /// The title of the scope's tab.
    pub fn title(self) -> &'static str {
        match self {
            Scope::Project => "Project",
            Scope::File => "Current File",
        }
    }
}

/// The grouping toggles (behaviour 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grouping {
    /// A row per module.
    pub modules: bool,
    /// The folders as rows.
    pub packages: bool,
    /// The folders not nested; needs `packages`.
    pub flatten: bool,
}

impl Default for Grouping {
    fn default() -> Self {
        Self {
            modules: false,
            packages: true,
            flatten: false,
        }
    }
}

/// The items of one file with the module it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The file relative to the project, with `/` between its parts (absolute for a current
    /// file outside the project).
    pub path: String,
    /// The folder of the file's module relative to the project; empty for the project itself.
    pub module_dir: String,
    /// The items, by line and column.
    pub items: Vec<Item>,
}

/// A visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// The top row: the project with the totals.
    Project {
        /// The project folder's name.
        name: String,
        /// Items shown.
        items: usize,
        /// Files with items shown.
        files: usize,
        /// Whether its rows are shown.
        open: bool,
    },
    /// A module (behaviour 16).
    Module {
        /// The module's folder relative to the project.
        dir: String,
        /// The folder's name, or the project's name for the project itself.
        name: String,
        /// Items below it.
        items: usize,
        /// Whether its rows are shown.
        open: bool,
    },
    /// A folder or a chain of folders.
    Folder {
        /// The module it stands in (empty without module rows or for the project).
        module: String,
        /// The folder relative to the module or project.
        dir: String,
        /// What the row shows: the last part or parts, or the whole path when flattened.
        name: String,
        /// Items below it.
        items: usize,
        /// Indentation level.
        depth: usize,
        /// Whether its rows are shown.
        open: bool,
    },
    /// A file with items.
    File {
        /// The file, as [`Entry::path`].
        path: String,
        /// Its name.
        name: String,
        /// Its folder, shown dimmed when the folders are no rows; empty otherwise.
        folder: String,
        /// Its items shown.
        items: usize,
        /// Indentation level.
        depth: usize,
        /// Whether its rows are shown.
        open: bool,
    },
    /// An item.
    Item {
        /// The file.
        path: String,
        /// 0-based line.
        line: usize,
        /// 0-based column, in characters.
        column: usize,
        /// The item's line, trimmed and cut.
        text: String,
        /// The characters of the match in `text`.
        mark: Range<usize>,
        /// Indentation level.
        depth: usize,
    },
}

impl Node {
    /// A key that stays the same while the node exists, for the selection, the closed nodes and
    /// the element ids.
    pub fn key(&self) -> String {
        match self {
            Node::Project { .. } => "project".into(),
            Node::Module { dir, .. } => format!("m:{dir}"),
            Node::Folder { module, dir, .. } => format!("d:{module}|{dir}"),
            Node::File { path, .. } => format!("f:{path}"),
            Node::Item {
                path, line, column, ..
            } => format!("i:{path}:{}:{}", line + 1, column + 1),
        }
    }

    /// Indentation level: 0 for the project.
    pub fn depth(&self) -> usize {
        match self {
            Node::Project { .. } => 0,
            Node::Module { .. } => 1,
            Node::Folder { depth, .. } | Node::File { depth, .. } | Node::Item { depth, .. } => {
                *depth
            }
        }
    }

    /// Whether the node is open; `None` for an item.
    pub fn open(&self) -> Option<bool> {
        match self {
            Node::Project { open, .. }
            | Node::Module { open, .. }
            | Node::Folder { open, .. }
            | Node::File { open, .. } => Some(*open),
            Node::Item { .. } => None,
        }
    }

    /// Whether the node is an item.
    pub fn is_item(&self) -> bool {
        matches!(self, Node::Item { .. })
    }
}

/// `1 item`, `3 items`.
pub fn count_label(items: usize) -> String {
    if items == 1 {
        "1 item".into()
    } else {
        format!("{items} items")
    }
}

/// The totals of the top row: `3 items in 2 files`.
pub fn totals_label(items: usize, files: usize) -> String {
    format!(
        "{} in {files} {}",
        count_label(items),
        if files == 1 { "file" } else { "files" }
    )
}

/// What an item row shows: the 1-based line number and the line's text.
pub fn item_label(line: usize, text: &str) -> String {
    format!("{} {text}", line + 1)
}

/// What the window says while the first scan runs.
pub const SCANNING: &str = "Scanning the project for TODO items…";
/// What the window says in a remote project (behaviour 23).
pub const REMOTE: &str = "TODO items are not available for remote projects.";
/// What the Current File scope says without a file.
pub const NO_FILE: &str = "No file is open in the editor.";
/// What a grouping action says when the window was never opened (behaviour 25).
pub const NOT_OPEN: &str = "The TODO tool window is not open";

/// What the window says without items.
pub fn nothing_found(filter: Filter) -> String {
    match filter {
        Filter::All => "No TODO items found.".into(),
        Filter::Only(p) => format!("No TODO items found for the pattern {}.", p.name()),
    }
}

/// The lines below the tree on what was left out (behaviour 22).
pub fn notes(truncated: bool, not_read: usize, not_checked: usize) -> Vec<String> {
    let files = |n: usize| {
        if n == 1 {
            "1 file".into()
        } else {
            format!("{n} files")
        }
    };
    let mut lines = Vec::new();
    if truncated {
        lines.push(format!(
            "The list is cut at {MAX_ITEMS} items; there may be more."
        ));
    }
    if not_read > 0 {
        lines.push(format!(
            "{} not read (binary, too large or unreadable).",
            files(not_read)
        ));
    }
    if not_checked > 0 {
        lines.push(format!(
            "{} with a match not checked: no known comment syntax.",
            files(not_checked)
        ));
    }
    lines
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// The order of names: without regard to case, then as written.
fn by_name(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b))
}

/// A file of a group with the items the filter shows.
struct Shown<'a> {
    entry: &'a Entry,
    /// The file's folder relative to the group's base.
    folder: String,
    items: Vec<&'a Item>,
}

/// A folder of the nested tree.
#[derive(Default)]
struct Dir<'a> {
    dirs: BTreeMap<String, Dir<'a>>,
    files: Vec<&'a Shown<'a>>,
}

impl Dir<'_> {
    fn items(&self) -> usize {
        self.files.iter().map(|f| f.items.len()).sum::<usize>()
            + self.dirs.values().map(Dir::items).sum::<usize>()
    }
}

struct Builder<'a> {
    closed: &'a BTreeSet<String>,
    rows: Vec<Node>,
}

impl<'a> Builder<'a> {
    /// Adds `node` and says whether its rows follow.
    fn push(&mut self, node: Node) -> bool {
        let open = node.open().unwrap_or(false);
        self.rows.push(node);
        open
    }

    fn open(&self, key: &str) -> bool {
        !self.closed.contains(key)
    }

    fn file(&mut self, shown: &Shown<'_>, with_folder: bool, depth: usize) {
        let path = &shown.entry.path;
        let open = self.open(&format!("f:{path}"));
        let follows = self.push(Node::File {
            path: path.clone(),
            name: name_of(path).to_owned(),
            folder: if with_folder {
                shown.folder.clone()
            } else {
                String::new()
            },
            items: shown.items.len(),
            depth,
            open,
        });
        if follows {
            for item in &shown.items {
                self.rows.push(Node::Item {
                    path: path.clone(),
                    line: item.line,
                    column: item.column,
                    text: item.line_text.clone(),
                    mark: item.mark.clone(),
                    depth: depth + 1,
                });
            }
        }
    }

    fn files(&mut self, files: &[&Shown<'_>], with_folder: bool, depth: usize) {
        let mut sorted: Vec<&&Shown<'_>> = files.iter().collect();
        sorted.sort_by(|a, b| {
            by_name(name_of(&a.entry.path), name_of(&b.entry.path))
                .then(a.entry.path.cmp(&b.entry.path))
        });
        for shown in sorted {
            self.file(shown, with_folder, depth);
        }
    }

    /// The folders of `dir` and then its files; `prefix` is the path of `dir` below the base.
    fn nested(&mut self, module: &str, dir: &Dir<'_>, prefix: &str, depth: usize) {
        let mut names: Vec<&String> = dir.dirs.keys().collect();
        names.sort_by(|a, b| by_name(a, b));
        for first in names {
            // A folder without files of its own and with one folder is shown with that folder.
            let mut at = &dir.dirs[first];
            let mut name = first.clone();
            let mut path = if prefix.is_empty() {
                first.clone()
            } else {
                format!("{prefix}/{first}")
            };
            while at.files.is_empty() && at.dirs.len() == 1 {
                let (next, child) = at.dirs.iter().next().expect("one folder");
                name = format!("{name}/{next}");
                path = format!("{path}/{next}");
                at = child;
            }
            let open = self.open(&format!("d:{module}|{path}"));
            if self.push(Node::Folder {
                module: module.to_owned(),
                dir: path.clone(),
                name,
                items: at.items(),
                depth,
                open,
            }) {
                self.nested(module, at, &path, depth + 1);
            }
        }
        self.files(&dir.files, false, depth);
    }

    /// The rows of one group (a module or the whole project) from `depth` on.
    fn group(&mut self, module: &str, files: &[Shown<'_>], grouping: Grouping, depth: usize) {
        if !grouping.packages {
            self.files(&files.iter().collect::<Vec<_>>(), true, depth);
            return;
        }
        if grouping.flatten {
            let mut folders: BTreeMap<&str, Vec<&Shown<'_>>> = BTreeMap::new();
            for f in files {
                folders.entry(f.folder.as_str()).or_default().push(f);
            }
            let mut names: Vec<&str> = folders.keys().copied().filter(|n| !n.is_empty()).collect();
            names.sort_by(|a, b| by_name(a, b));
            for name in names {
                let inside = &folders[name];
                let open = self.open(&format!("d:{module}|{name}"));
                if self.push(Node::Folder {
                    module: module.to_owned(),
                    dir: name.to_owned(),
                    name: name.to_owned(),
                    items: inside.iter().map(|f| f.items.len()).sum(),
                    depth,
                    open,
                }) {
                    self.files(inside, false, depth + 1);
                }
            }
            if let Some(top) = folders.get("") {
                self.files(top, false, depth);
            }
            return;
        }
        let mut top = Dir::default();
        for f in files {
            let mut at = &mut top;
            for part in f.folder.split('/').filter(|p| !p.is_empty()) {
                at = at.dirs.entry(part.to_owned()).or_default();
            }
            at.files.push(f);
        }
        self.nested(module, &top, "", depth);
    }
}

/// The visible rows of the Project scope (behaviours 15–17): the project named `project`, the
/// files of `entries` with the items `filter` shows, grouped by `grouping`; the rows of the
/// nodes whose key is in `closed` are left out.
pub fn tree(
    project: &str,
    entries: &[Entry],
    grouping: Grouping,
    filter: Filter,
    closed: &BTreeSet<String>,
) -> Vec<Node> {
    let shown = |entry| {
        let entry: &Entry = entry;
        let items: Vec<&Item> = entry.items.iter().filter(|i| filter.shows(i)).collect();
        (!items.is_empty()).then(|| {
            let base = if grouping.modules {
                entry.module_dir.as_str()
            } else {
                ""
            };
            let folder = folder_of(&entry.path);
            // The folder below the module: `app/src` below `app`, not `application` below it.
            let folder = match folder.strip_prefix(base) {
                Some("") if !base.is_empty() => "",
                Some(rest) if !base.is_empty() && rest.starts_with('/') => &rest[1..],
                _ => folder,
            };
            Shown {
                entry,
                folder: folder.to_owned(),
                items,
            }
        })
    };
    let files: Vec<Shown<'_>> = entries.iter().filter_map(shown).collect();
    let mut builder = Builder {
        closed,
        rows: Vec::new(),
    };
    let open = builder.open("project");
    if !builder.push(Node::Project {
        name: project.to_owned(),
        items: files.iter().map(|f| f.items.len()).sum(),
        files: files.len(),
        open,
    }) {
        return builder.rows;
    }
    if !grouping.modules {
        builder.group("", &files, grouping, 1);
        return builder.rows;
    }
    let mut modules: BTreeMap<&str, Vec<Shown<'_>>> = BTreeMap::new();
    for f in files {
        modules
            .entry(f.entry.module_dir.as_str())
            .or_default()
            .push(f);
    }
    let name = |dir: &str| {
        if dir.is_empty() {
            project.to_owned()
        } else {
            name_of(dir).to_owned()
        }
    };
    let mut dirs: Vec<&str> = modules.keys().copied().collect();
    dirs.sort_by(|a, b| by_name(&name(a), &name(b)).then(a.cmp(b)));
    for dir in dirs {
        let inside = &modules[dir];
        let open = builder.open(&format!("m:{dir}"));
        if builder.push(Node::Module {
            dir: dir.to_owned(),
            name: name(dir),
            items: inside.iter().map(|f| f.items.len()).sum(),
            open,
        }) {
            builder.group(dir, inside, grouping, 2);
        }
    }
    builder.rows
}

/// The visible rows of the Current File scope (behaviour 15): the file and its items.
pub fn file_tree(entry: &Entry, filter: Filter, closed: &BTreeSet<String>) -> Vec<Node> {
    let items: Vec<&Item> = entry.items.iter().filter(|i| filter.shows(i)).collect();
    let mut builder = Builder {
        closed,
        rows: Vec::new(),
    };
    let shown = Shown {
        entry,
        folder: folder_of(&entry.path).to_owned(),
        items,
    };
    builder.file(&shown, true, 0);
    builder.rows
}

/// The row `NextOccurence` or `PreviousOccurence` goes to (behaviour 19): in `rows`, the tree
/// with every node open, the next item after the row with the key `selected`, or the previous
/// one before it; from no selection the first or the last. `None` at the ends.
pub fn step(rows: &[Node], selected: Option<&str>, forward: bool) -> Option<usize> {
    let at = selected.and_then(|key| rows.iter().position(|n| n.key() == key));
    let items = rows.iter().enumerate().filter(|(_, n)| n.is_item());
    match (at, forward) {
        (Some(at), true) => items.map(|(i, _)| i).find(|i| *i > at),
        (Some(at), false) => items.map(|(i, _)| i).rfind(|i| *i < at),
        (None, true) => items.map(|(i, _)| i).next(),
        (None, false) => items.map(|(i, _)| i).next_back(),
    }
}

/// The keys of the nodes above row `index` of `rows`, nearest first.
pub fn ancestors(rows: &[Node], index: usize) -> Vec<String> {
    let mut keys = Vec::new();
    let Some(mut depth) = rows.get(index).map(Node::depth) else {
        return keys;
    };
    for node in rows[..index].iter().rev() {
        if node.depth() < depth {
            depth = node.depth();
            keys.push(node.key());
        }
    }
    keys
}

/// The index of the node above row `index`, for Left on a closed node or an item.
pub fn parent(rows: &[Node], index: usize) -> Option<usize> {
    let depth = rows.get(index)?.depth();
    rows[..index].iter().rposition(|n| n.depth() < depth)
}

/// The keys of every node of `rows` that can be closed, the project left out (Collapse All).
pub fn closable(rows: &[Node]) -> BTreeSet<String> {
    rows.iter()
        .filter(|n| n.open().is_some() && !matches!(n, Node::Project { .. }))
        .map(Node::key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(line: usize, pattern: Pattern, text: &str) -> Item {
        Item {
            line,
            column: 3,
            pattern,
            text: text.to_owned(),
            line_text: format!("// {text}"),
            mark: 3..3 + text.chars().count(),
            continuation: 0,
        }
    }

    fn entry(path: &str, module_dir: &str, items: Vec<Item>) -> Entry {
        Entry {
            path: path.into(),
            module_dir: module_dir.into(),
            items,
        }
    }

    fn entries() -> Vec<Entry> {
        use Pattern::{Fixme, Todo};
        vec![
            entry("README.txt", "", vec![item(0, Todo, "TODO readme")]),
            entry(
                "app/src/main/java/shop/Cart.java",
                "app",
                vec![item(4, Todo, "TODO total"), item(9, Fixme, "FIXME clear")],
            ),
            entry(
                "app/src/main/java/shop/util/Money.java",
                "app",
                vec![item(1, Fixme, "FIXME rounding")],
            ),
            entry(
                "app/build.gradle",
                "app",
                vec![item(2, Todo, "TODO version")],
            ),
            entry("lib/src/b.rs", "lib", vec![item(0, Todo, "TODO b")]),
            entry("lib/src/A.rs", "lib", vec![item(7, Todo, "TODO a")]),
        ]
    }

    /// The rows as indented lines: name and count, or the item's label.
    fn lines(rows: &[Node]) -> Vec<String> {
        rows.iter()
            .map(|n| {
                let text = match n {
                    Node::Project {
                        name, items, files, ..
                    } => {
                        format!("{name} ({})", totals_label(*items, *files))
                    }
                    Node::Module { name, items, .. } => format!("[{name}] ({items})"),
                    Node::Folder { name, items, .. } => format!("{name}/ ({items})"),
                    Node::File {
                        name,
                        folder,
                        items,
                        ..
                    } if folder.is_empty() => format!("{name} ({items})"),
                    Node::File {
                        name,
                        folder,
                        items,
                        ..
                    } => format!("{name} <{folder}> ({items})"),
                    Node::Item { line, text, .. } => item_label(*line, text),
                };
                format!("{}{text}", "  ".repeat(n.depth()))
            })
            .collect()
    }

    fn shown(grouping: Grouping, filter: Filter, closed: &[&str]) -> Vec<String> {
        let closed = closed.iter().map(|k| (*k).to_owned()).collect();
        lines(&tree("shop", &entries(), grouping, filter, &closed))
    }

    /// SPEC-0334 AK-6.
    #[test]
    fn the_tree_follows_the_grouping_toggles() {
        // The default: folders nested, chains of single folders as one row, folders first.
        let default = Grouping::default();
        assert_eq!(
            (default.modules, default.packages, default.flatten),
            (false, true, false)
        );
        assert_eq!(
            shown(default, Filter::All, &[]),
            [
                "shop (7 items in 6 files)",
                "  app/ (4)",
                "    src/main/java/shop/ (3)",
                "      util/ (1)",
                "        Money.java (1)",
                "          2 // FIXME rounding",
                "      Cart.java (2)",
                "        5 // TODO total",
                "        10 // FIXME clear",
                "    build.gradle (1)",
                "      3 // TODO version",
                "  lib/src/ (2)",
                "    A.rs (1)",
                "      8 // TODO a",
                "    b.rs (1)",
                "      1 // TODO b",
                "  README.txt (1)",
                "    1 // TODO readme",
            ]
        );
        // Flattened: every folder with files, by its whole path.
        let flat = Grouping {
            flatten: true,
            ..default
        };
        assert_eq!(
            shown(flat, Filter::All, &["f:app/build.gradle", "f:lib/src/A.rs"]),
            [
                "shop (7 items in 6 files)",
                "  app/ (1)",
                "    build.gradle (1)",
                "  app/src/main/java/shop/ (2)",
                "    Cart.java (2)",
                "      5 // TODO total",
                "      10 // FIXME clear",
                "  app/src/main/java/shop/util/ (1)",
                "    Money.java (1)",
                "      2 // FIXME rounding",
                "  lib/src/ (2)",
                "    A.rs (1)",
                "    b.rs (1)",
                "      1 // TODO b",
                "  README.txt (1)",
                "    1 // TODO readme",
            ]
        );
        // Modules as a level; folders are relative to the module; the project is a module too.
        let modules = Grouping {
            modules: true,
            ..default
        };
        let closed_files = [
            "f:README.txt",
            "f:app/build.gradle",
            "f:app/src/main/java/shop/Cart.java",
            "f:app/src/main/java/shop/util/Money.java",
            "f:lib/src/A.rs",
            "f:lib/src/b.rs",
        ];
        assert_eq!(
            shown(modules, Filter::All, &closed_files),
            [
                "shop (7 items in 6 files)",
                "  [app] (4)",
                "    src/main/java/shop/ (3)",
                "      util/ (1)",
                "        Money.java (1)",
                "      Cart.java (2)",
                "    build.gradle (1)",
                "  [lib] (2)",
                "    src/ (2)",
                "      A.rs (1)",
                "      b.rs (1)",
                "  [shop] (1)",
                "    README.txt (1)",
            ]
        );
        // A folder that only begins like the module's folder is not below it.
        let odd = [entry(
            "application/x.rs",
            "app",
            vec![item(0, Pattern::Todo, "TODO x")],
        )];
        assert_eq!(
            lines(&tree("shop", &odd, modules, Filter::All, &BTreeSet::new()))[1..3],
            ["  [app] (1)", "    application/ (1)"]
        );
        // Without folders the files stand below the module or project with their folder dimmed.
        let files_only = Grouping {
            packages: false,
            ..default
        };
        assert_eq!(
            shown(files_only, Filter::All, &closed_files),
            [
                "shop (7 items in 6 files)",
                "  A.rs <lib/src> (1)",
                "  b.rs <lib/src> (1)",
                "  build.gradle <app> (1)",
                "  Cart.java <app/src/main/java/shop> (2)",
                "  Money.java <app/src/main/java/shop/util> (1)",
                "  README.txt (1)",
            ]
        );
        // Flatten has no effect without packages.
        let flat_files = Grouping {
            packages: false,
            flatten: true,
            ..default
        };
        assert_eq!(
            shown(flat_files, Filter::All, &closed_files),
            shown(files_only, Filter::All, &closed_files)
        );
        let module_files = Grouping {
            modules: true,
            packages: false,
            flatten: false,
        };
        assert_eq!(
            shown(module_files, Filter::All, &closed_files)[1..4],
            [
                "  [app] (4)",
                "    build.gradle (1)",
                "    Cart.java <src/main/java/shop> (2)",
            ]
        );
    }

    /// SPEC-0334 AK-6.
    #[test]
    fn the_filter_drops_and_recounts() {
        assert_eq!(
            shown(Grouping::default(), Filter::Only(Pattern::Fixme), &[]),
            [
                "shop (2 items in 2 files)",
                "  app/src/main/java/shop/ (2)",
                "    util/ (1)",
                "      Money.java (1)",
                "        2 // FIXME rounding",
                "    Cart.java (1)",
                "      10 // FIXME clear",
            ]
        );
        assert_eq!(
            shown(Grouping::default(), Filter::Only(Pattern::Todo), &[])[0],
            "shop (5 items in 5 files)"
        );
        assert_eq!(Filter::ALL.map(Filter::label), ["All", "todo", "fixme"]);
        assert_eq!(Filter::ALL.map(Filter::id), ["all", "todo", "fixme"]);
        // Nothing left: only the project with zeros, and the window's sentence.
        let none = tree(
            "shop",
            &entries()[..1],
            Grouping::default(),
            Filter::Only(Pattern::Fixme),
            &BTreeSet::new(),
        );
        assert_eq!(lines(&none), ["shop (0 items in 0 files)"]);
        assert_eq!(
            nothing_found(Filter::Only(Pattern::Fixme)),
            "No TODO items found for the pattern fixme."
        );
        assert_eq!(nothing_found(Filter::All), "No TODO items found.");
        assert_eq!(
            notes(true, 1, 2),
            [
                "The list is cut at 5000 items; there may be more.",
                "1 file not read (binary, too large or unreadable).",
                "2 files with a match not checked: no known comment syntax.",
            ]
        );
        assert!(notes(false, 0, 0).is_empty());
    }

    /// SPEC-0334 AK-6.
    #[test]
    fn closed_nodes_hide_their_rows() {
        assert_eq!(
            shown(
                Grouping::default(),
                Filter::All,
                &["d:|app/src/main/java/shop", "d:|lib/src", "f:README.txt"]
            ),
            [
                "shop (7 items in 6 files)",
                "  app/ (4)",
                "    src/main/java/shop/ (3)",
                "    build.gradle (1)",
                "      3 // TODO version",
                "  lib/src/ (2)",
                "  README.txt (1)",
            ]
        );
        assert_eq!(
            shown(Grouping::default(), Filter::All, &["project"]),
            ["shop (7 items in 6 files)"]
        );
        let modules = Grouping {
            modules: true,
            ..Grouping::default()
        };
        assert_eq!(
            shown(modules, Filter::All, &["m:app", "m:lib", "m:"]),
            [
                "shop (7 items in 6 files)",
                "  [app] (4)",
                "  [lib] (2)",
                "  [shop] (1)"
            ]
        );
        // Collapse All closes everything below the project.
        let all = tree(
            "shop",
            &entries(),
            Grouping::default(),
            Filter::All,
            &BTreeSet::new(),
        );
        let closed = closable(&all);
        assert!(!closed.contains("project") && closed.contains("d:|app"));
        assert_eq!(
            lines(&tree(
                "shop",
                &entries(),
                Grouping::default(),
                Filter::All,
                &closed
            )),
            [
                "shop (7 items in 6 files)",
                "  app/ (4)",
                "  lib/src/ (2)",
                "  README.txt (1)",
            ]
        );
        // The Current File scope: the file and its items.
        let file = &entries()[1];
        assert_eq!(
            lines(&file_tree(file, Filter::All, &BTreeSet::new())),
            [
                "Cart.java <app/src/main/java/shop> (2)",
                "  5 // TODO total",
                "  10 // FIXME clear",
            ]
        );
        assert_eq!(
            lines(&file_tree(
                file,
                Filter::Only(Pattern::Todo),
                &BTreeSet::new()
            )),
            [
                "Cart.java <app/src/main/java/shop> (1)",
                "  5 // TODO total"
            ]
        );
    }

    /// SPEC-0334 AK-7.
    #[test]
    fn next_and_previous_step_through_items() {
        let rows = tree(
            "shop",
            &entries(),
            Grouping::default(),
            Filter::All,
            &BTreeSet::new(),
        );
        let key = |i: usize| rows[i].key();
        // From nothing: the first item, or the last.
        let first = step(&rows, None, true).unwrap();
        assert_eq!(key(first), "i:app/src/main/java/shop/util/Money.java:2:4");
        let last = step(&rows, None, false).unwrap();
        assert_eq!(key(last), "i:README.txt:1:4");
        // Through the files in tree order.
        let mut at = first;
        let mut seen = vec![key(at)];
        while let Some(next) = step(&rows, Some(&key(at)), true) {
            at = next;
            seen.push(key(at));
        }
        assert_eq!(
            seen,
            [
                "i:app/src/main/java/shop/util/Money.java:2:4",
                "i:app/src/main/java/shop/Cart.java:5:4",
                "i:app/src/main/java/shop/Cart.java:10:4",
                "i:app/build.gradle:3:4",
                "i:lib/src/A.rs:8:4",
                "i:lib/src/b.rs:1:4",
                "i:README.txt:1:4",
            ]
        );
        // The ends: nothing further.
        assert_eq!(step(&rows, Some(&key(last)), true), None);
        assert_eq!(step(&rows, Some(&key(first)), false), None);
        // From a row that is no item: the next item below it, the previous above it.
        let cart = rows
            .iter()
            .position(|n| n.key() == "f:app/src/main/java/shop/Cart.java")
            .unwrap();
        assert_eq!(step(&rows, Some(&key(cart)), true), Some(cart + 1));
        assert_eq!(step(&rows, Some(&key(cart)), false), Some(first));
        // The nodes to open for an item, nearest first, and the node Left goes to.
        assert_eq!(
            ancestors(&rows, first),
            [
                "f:app/src/main/java/shop/util/Money.java",
                "d:|app/src/main/java/shop/util",
                "d:|app/src/main/java/shop",
                "d:|app",
                "project",
            ]
        );
        assert_eq!(parent(&rows, first), Some(first - 1));
        assert_eq!(parent(&rows, 0), None);
    }

    /// SPEC-0334 AK-7.
    #[test]
    fn an_item_row_is_line_and_text() {
        let rows = file_tree(&entries()[1], Filter::All, &BTreeSet::new());
        let Node::Item {
            line,
            column,
            text,
            mark,
            ..
        } = &rows[1]
        else {
            panic!("an item row");
        };
        assert_eq!((*line, *column), (4, 3));
        assert_eq!(item_label(*line, text), "5 // TODO total");
        assert_eq!(*mark, 3..13);
        assert_eq!(count_label(1), "1 item");
        assert_eq!(count_label(2), "2 items");
        assert_eq!(totals_label(1, 1), "1 item in 1 file");
        assert_eq!(Scope::Project.title(), "Project");
        assert_eq!(Scope::File.title(), "Current File");
    }
}
