//! The tree and the preview of the Find tool window (`SPEC-0293` behaviours 4–6, 8 and v3
//! behaviour 14), without toolkit; of v9 the filters by read access, write access and import
//! statements, the nested folders, the module groups, excluded usages and the text export
//! (behaviours 22–26).

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use idecraft_languages::context::{PlaceContext, UsageKind};

use crate::nav_model::Place;

/// Lines shown before and after a usage in the preview (behaviour 6).
pub const PREVIEW_CONTEXT: usize = 6;

/// One usage with its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Where it starts.
    pub place: Place,
    /// Path relative to the workspace, or absolute outside it.
    pub shown_path: String,
    /// The line's text, trimmed.
    pub text: String,
    /// The symbol in `text`, in chars.
    pub mark: Range<usize>,
    /// What the usage does, when known (v3 behaviour 14).
    pub kind: Option<UsageKind>,
    /// Module, package, class and method, when known (v3 behaviour 14).
    pub context: Option<PlaceContext>,
}

/// A grouping level of the tree (v3 behaviour 14), outer to inner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    /// Calls, writes, imports, other usages.
    Kind,
    /// The module's folder.
    Module,
    /// The package or folder.
    Package,
    /// One folder of the nested folders (v9 behaviour 23).
    Dir,
    /// A group of modules by a part of their dotted names (v9 behaviour 24).
    ModuleGroup,
    /// The file.
    File,
    /// The type around the usage.
    Class,
    /// The function around the usage.
    Method,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Kind => "k",
            Level::Module => "m",
            Level::Package => "p",
            Level::Dir => "d",
            // Sorted with the modules: a group and the modules below it stay together.
            Level::ModuleGroup => "m",
            Level::File => "f",
            Level::Class => "c",
            Level::Method => "fn",
        }
    }
}

/// A row's node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    /// The searched symbol.
    Target,
    /// `Usages in Project Files` (or the text occurrences).
    Group,
    /// A group of usages, by the keys of its levels from the outside in.
    Bucket(Vec<String>),
    /// One usage, by its index in [`UsageTree::hits`].
    Usage(usize),
}

/// One visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// What it shows.
    pub node: Node,
    /// The level of a group row.
    pub level: Option<Level>,
    /// Indentation level.
    pub depth: usize,
    /// Main text.
    pub label: String,
    /// Grey text after it (counts, places).
    pub detail: String,
    /// Whether it has rows below it.
    pub expandable: bool,
    /// Whether those rows are shown.
    pub expanded: bool,
    /// Whether the usage, or every usage below the group, is excluded (v9 behaviour 25).
    pub excluded: bool,
}

/// `1 usage` / `N usages`, or occurrences for text matches.
pub fn count_label(n: usize, text: bool) -> String {
    match (n, text) {
        (1, false) => "1 usage".into(),
        (n, false) => format!("{n} usages"),
        (1, true) => "1 occurrence".into(),
        (n, true) => format!("{n} occurrences"),
    }
}

/// Where a search looked: the file of `FindUsagesInFile` (v7 behaviour 19), else the project.
pub fn scope_label(in_file: Option<&str>) -> &str {
    in_file.unwrap_or("Project Files")
}

/// The tab's title: `<symbol> in Project Files`, or `<symbol> in <file name>` (behaviours 1, 19).
pub fn search_title(symbol: &str, in_file: Option<&str>) -> String {
    format!("{symbol} in {}", scope_label(in_file))
}

/// What the window and the status bar say when a search ended without a usage: `No usages of
/// <symbol> in project files`, or `… in <file name>` (behaviours 9 and 19).
pub fn no_usages(symbol: &str, in_file: Option<&str>) -> String {
    format!(
        "No usages of {symbol} in {}",
        in_file.unwrap_or("project files")
    )
}

/// How long the window shows a search as running before it ends it itself: the search's own
/// deadline and as much again for the text search that follows it (v8 behaviour 21).
pub const GIVE_UP_AFTER: std::time::Duration =
    std::time::Duration::from_secs(2 * idecraft_languages::find_usages::DEADLINE.as_secs());

/// Whether a search shown as running for `elapsed` is ended by the window (v8 behaviour 21).
pub fn overdue(elapsed: std::time::Duration) -> bool {
    elapsed >= GIVE_UP_AFTER
}

/// What the window says when it ended a search itself (v8 behaviour 21).
pub fn gave_up(symbol: &str) -> String {
    format!(
        "The search for usages of {symbol} did not end within {} s and was stopped. Rerun searches again.",
        GIVE_UP_AFTER.as_secs()
    )
}

/// What the window shows for a finished search (v8 behaviour 21): the tree, or without a usage
/// in it the words of [`no_usages`] — never an empty tree, never a running search.
pub fn finished(tree: &UsageTree) -> Option<String> {
    tree.hits
        .is_empty()
        .then(|| no_usages(&tree.symbol, tree.in_file.as_deref()))
}

/// One level of a hit's place in the tree: level, key and label.
type Step = (Level, String, String);

/// Usages of one symbol as a tree: target, group, then the levels of v3 behaviour 14, usages
/// (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageTree {
    /// The symbol.
    pub symbol: String,
    /// Where it is declared: `path:line`, or the library name, or empty.
    pub declared: String,
    /// The declaration's kind, shown before the name (`method`), when known.
    pub target_kind: Option<String>,
    /// Whether the hits are text occurrences (behaviour 10).
    pub text: bool,
    /// The name of the file a search of `FindUsagesInFile` is restricted to (v7 behaviour 19).
    pub in_file: Option<String>,
    /// All hits, sorted by path, line and column, each once.
    pub hits: Vec<Hit>,
    /// Files level shown (behaviour 5).
    pub by_file: bool,
    /// Usage kind level shown (v3).
    pub by_kind: bool,
    /// Module level shown (v3).
    pub by_module: bool,
    /// Package level shown (v3).
    pub by_package: bool,
    /// Class and method levels shown (v3).
    pub by_structure: bool,
    /// Usages that read the symbol shown (v9 behaviour 22).
    pub show_read: bool,
    /// Usages that write the symbol shown (v9 behaviour 22).
    pub show_write: bool,
    /// Usages in import statements shown (v9 behaviour 22).
    pub show_imports: bool,
    /// One row per folder instead of the package row (v9 behaviour 23).
    pub nested_dirs: bool,
    /// Modules as one level; off, a module with a dotted name sits below the groups its name
    /// parts form (v9 behaviour 24).
    pub flatten_modules: bool,
    excluded: HashSet<usize>,
    collapsed: HashSet<Node>,
}

impl UsageTree {
    /// A tree for `symbol` with `hits` (sorted and de-duplicated here).
    pub fn new(symbol: &str, declared: String, text: bool, mut hits: Vec<Hit>) -> Self {
        hits.sort_by(|a, b| {
            (&a.shown_path, a.place.line, a.place.column).cmp(&(
                &b.shown_path,
                b.place.line,
                b.place.column,
            ))
        });
        hits.dedup_by(|a, b| a.place == b.place);
        Self {
            symbol: symbol.to_owned(),
            declared,
            target_kind: None,
            text,
            in_file: None,
            hits,
            by_file: true,
            by_kind: true,
            by_module: true,
            by_package: true,
            by_structure: true,
            show_read: true,
            show_write: true,
            show_imports: true,
            nested_dirs: false,
            flatten_modules: true,
            excluded: HashSet::new(),
            collapsed: HashSet::new(),
        }
    }

    /// The group row's text.
    pub fn group_label(&self) -> String {
        let what = if self.text {
            "Text occurrences"
        } else {
            "Usages"
        };
        format!("{what} in {}", scope_label(self.in_file.as_deref()))
    }

    /// The number of files with hits.
    pub fn file_count(&self) -> usize {
        self.first_hits().len()
    }

    /// The first hit of each file, in path order.
    pub fn first_hits(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for (i, h) in self.hits.iter().enumerate() {
            if out
                .last()
                .is_none_or(|&j| self.hits[j].shown_path != h.shown_path)
            {
                out.push(i);
            }
        }
        out
    }

    /// Where hit `i` sits: one step per shown level that applies to it.
    fn steps(&self, i: usize) -> Vec<Step> {
        let h = &self.hits[i];
        let c = h.context.as_ref();
        let mut out: Vec<Step> = Vec::new();
        if self.by_kind
            && let Some(k) = h.kind
        {
            // Kinds keep their own order: imports, calls, writes, others.
            out.push((Level::Kind, format!("{:?}", k as u8), k.label().into()));
        }
        let module = c.map(|c| &c.module).filter(|m| !m.is_empty());
        if self.by_module
            && let Some(m) = module
        {
            if !self.flatten_modules {
                // `a.b.core` sits below the groups `a` and `a.b` (v9 behaviour 24).
                let parts: Vec<&str> = m.split('.').collect();
                if parts.iter().all(|p| !p.is_empty()) {
                    for n in 1..parts.len() {
                        let group = parts[..n].join(".");
                        out.push((Level::ModuleGroup, format!("{group}."), group));
                    }
                }
            }
            out.push((Level::Module, m.clone(), m.clone()));
        }
        if self.nested_dirs {
            // One row per folder of the file's path, from the outside in; the module's own
            // folder is not repeated below its row (v9 behaviour 23).
            let shown = h.shown_path.replace('\\', "/");
            let mut folders: Vec<&str> = shown.split('/').collect();
            folders.pop();
            let module_row = self.by_module && module.is_some();
            if module_row && folders.first().copied() == module.map(String::as_str) {
                folders.remove(0);
            }
            let mut path = String::new();
            for f in folders.into_iter().filter(|f| !f.is_empty()) {
                path.push_str(f);
                path.push('/');
                out.push((Level::Dir, path.clone(), f.to_owned()));
            }
        } else if self.by_package
            && let Some(p) = c.and_then(|c| c.package.clone())
        {
            out.push((Level::Package, p.clone(), p));
        }
        let in_type = self.by_structure && c.is_some_and(|c| c.container.is_some());
        if self.by_file && !in_type {
            // Below a module or package the file name is enough.
            let label = if out
                .iter()
                .any(|(l, _, _)| matches!(l, Level::Module | Level::Package | Level::Dir))
            {
                Path::new(&h.shown_path)
                    .file_name()
                    .map_or(h.shown_path.clone(), |n| n.to_string_lossy().into_owned())
            } else {
                h.shown_path.clone()
            };
            out.push((Level::File, h.shown_path.clone(), label));
        }
        if self.by_structure
            && let Some(c) = c
        {
            if let Some(t) = &c.container {
                out.push((Level::Class, t.clone(), t.clone()));
            }
            if let Some(m) = &c.method {
                out.push((Level::Method, m.clone(), m.clone()));
            }
        }
        out
    }

    fn key(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .map(|(l, k, _)| format!("{}:{k}", l.tag()))
            .collect()
    }

    /// The hits in the order shown: by their group keys, then by path, line and column.
    pub fn order(&self) -> Vec<usize> {
        let mut order: Vec<(Vec<String>, usize)> = (0..self.hits.len())
            .filter(|&i| self.shown(i))
            .map(|i| (Self::key(&self.steps(i)), i))
            .collect();
        order.sort();
        order.into_iter().map(|(_, i)| i).collect()
    }

    /// Whether the filters show hit `i` (v9 behaviour 22): a usage known to read the symbol
    /// needs Show Read Access, one that writes it Show Write Access, one in an import statement
    /// Show Import Statements; every other usage is always shown.
    pub fn shown(&self, i: usize) -> bool {
        match self.hits.get(i).and_then(|h| h.kind) {
            Some(UsageKind::Read) => self.show_read,
            Some(UsageKind::Write) => self.show_write,
            Some(UsageKind::Import) => self.show_imports,
            _ => true,
        }
    }

    /// The usages a filter hides.
    pub fn filtered_out(&self) -> usize {
        (0..self.hits.len()).filter(|&i| !self.shown(i)).count()
    }

    /// Whether hit `i` is excluded (v9 behaviour 25).
    pub fn is_excluded(&self, i: usize) -> bool {
        self.excluded.contains(&i)
    }

    /// The shown usages that are not excluded: what the counts name and Next and Previous visit.
    pub fn included(&self) -> Vec<usize> {
        self.order()
            .into_iter()
            .filter(|i| !self.excluded.contains(i))
            .collect()
    }

    /// The shown usages a row stands for: itself, those of its group, all for the group row
    /// of the whole result; the row of the searched symbol stands for none.
    pub fn hits_below(&self, node: &Node) -> Vec<usize> {
        match node {
            Node::Usage(i) => Some(*i).filter(|&i| self.shown(i)).into_iter().collect(),
            Node::Bucket(prefix) => self
                .order()
                .into_iter()
                .filter(|&i| Self::key(&self.steps(i)).starts_with(prefix))
                .collect(),
            Node::Group => self.order(),
            Node::Target => Vec::new(),
        }
    }

    /// Excludes (or includes again) the usages `node` stands for; `false` when that changes
    /// nothing (v9 behaviour 25).
    pub fn set_excluded(&mut self, node: &Node, exclude: bool) -> bool {
        let mut changed = false;
        for i in self.hits_below(node) {
            changed |= if exclude {
                self.excluded.insert(i)
            } else {
                self.excluded.remove(&i)
            };
        }
        changed
    }

    /// Takes over what `old`, a tree of the same search, had excluded, by the places of the
    /// usages: a rerun keeps the excluded usages that are still found.
    pub fn keep_excluded(&mut self, old: &UsageTree) {
        let places: HashSet<&Place> = old
            .excluded
            .iter()
            .filter_map(|&i| old.hits.get(i).map(|h| &h.place))
            .collect();
        self.excluded = (0..self.hits.len())
            .filter(|&i| places.contains(&self.hits[i].place))
            .collect();
    }

    /// The rows as shown, top to bottom.
    pub fn rows(&self) -> Vec<Row> {
        let mut out = vec![Row {
            node: Node::Target,
            level: None,
            depth: 0,
            label: self.symbol.clone(),
            detail: self.declared.clone(),
            expandable: false,
            expanded: false,
            excluded: false,
        }];
        if self.hits.is_empty() {
            return out;
        }
        let order = self.order();
        let included = order.iter().filter(|i| !self.excluded.contains(i)).count();
        let group_open = !self.collapsed.contains(&Node::Group);
        out.push(Row {
            node: Node::Group,
            level: None,
            depth: 0,
            label: self.group_label(),
            detail: count_label(included, self.text),
            expandable: !order.is_empty(),
            expanded: group_open && !order.is_empty(),
            excluded: included == 0 && !order.is_empty(),
        });
        if !group_open {
            return out;
        }
        let all: Vec<Vec<Step>> = (0..self.hits.len()).map(|i| self.steps(i)).collect();
        // Per group the usages that count: the shown ones that are not excluded.
        let mut counts: HashMap<Vec<String>, usize> = HashMap::new();
        for &i in &order {
            let key = Self::key(&all[i]);
            let counted = usize::from(!self.excluded.contains(&i));
            for d in 1..=key.len() {
                *counts.entry(key[..d].to_vec()).or_default() += counted;
            }
        }
        let mut previous: Vec<String> = Vec::new();
        for i in order {
            let steps = &all[i];
            let key = Self::key(steps);
            let common = previous
                .iter()
                .zip(&key)
                .take_while(|(a, b)| a == b)
                .count();
            // Rows below a closed group are hidden.
            let mut hidden = false;
            for d in 0..key.len() {
                let node = Node::Bucket(key[..=d].to_vec());
                let open = !self.collapsed.contains(&node);
                if d >= common && !hidden {
                    out.push(Row {
                        node: node.clone(),
                        level: Some(steps[d].0),
                        depth: d + 1,
                        label: steps[d].2.clone(),
                        detail: count_label(counts[&key[..=d]], self.text),
                        expandable: true,
                        expanded: open,
                        excluded: counts[&key[..=d]] == 0,
                    });
                }
                hidden |= !open;
            }
            if !hidden {
                let h = &self.hits[i];
                let has_file = steps.iter().any(|(l, _, _)| *l == Level::File);
                out.push(Row {
                    node: Node::Usage(i),
                    level: None,
                    depth: key.len() + 1,
                    label: format!("{}  {}", h.place.line + 1, h.text),
                    detail: if has_file {
                        String::new()
                    } else {
                        format!("{}:{}", h.shown_path, h.place.line + 1)
                    },
                    expandable: false,
                    expanded: false,
                    excluded: self.excluded.contains(&i),
                });
            }
            previous = key;
        }
        out
    }

    /// Shows or hides the rows below `node`.
    pub fn set_expanded(&mut self, node: &Node, open: bool) {
        if open {
            self.collapsed.remove(node);
        } else {
            self.collapsed.insert(node.clone());
        }
    }

    /// Expands everything (behaviour 7).
    pub fn expand_all(&mut self) {
        self.collapsed.clear();
    }

    /// Collapses every group.
    pub fn collapse_all(&mut self) {
        self.collapsed.insert(Node::Group);
        for i in 0..self.hits.len() {
            let key = Self::key(&self.steps(i));
            for d in 1..=key.len() {
                self.collapsed.insert(Node::Bucket(key[..d].to_vec()));
            }
        }
    }

    /// Expands what is needed to show `hit` and returns its row.
    pub fn reveal(&mut self, hit: usize) -> Option<usize> {
        self.collapsed.remove(&Node::Group);
        if hit < self.hits.len() {
            let key = Self::key(&self.steps(hit));
            for d in 1..=key.len() {
                self.collapsed.remove(&Node::Bucket(key[..d].to_vec()));
            }
        }
        self.rows().iter().position(|r| r.node == Node::Usage(hit))
    }

    /// The first hit shown below a group.
    fn first_in(&self, prefix: &[String]) -> Option<usize> {
        self.order()
            .into_iter()
            .find(|&i| Self::key(&self.steps(i)).starts_with(prefix))
    }

    /// The usage a row stands for in the preview: itself, a group's first, else the first of all.
    pub fn hit_of(&self, node: &Node) -> Option<usize> {
        match node {
            Node::Usage(i) => Some(*i),
            Node::Bucket(prefix) => self.first_in(prefix),
            Node::Target | Node::Group => self.order().first().copied(),
        }
    }

    /// The next (or previous) usage after the one at `node` in the shown order, wrapping at the
    /// ends (behaviour 8). From a group row it is that group's first usage going forward, the one
    /// before it going back; from the target or group row the first (or last) of all.
    ///
    /// Excluded usages are passed over (v9 behaviour 25): from one of them it is the next (or
    /// previous) usage that is not excluded.
    pub fn step(&self, node: Option<&Node>, forward: bool) -> Option<usize> {
        let all = self.order();
        let order = self.included();
        let n = order.len();
        if n == 0 {
            return None;
        }
        // The included usages before hit `i` in the shown order.
        let before = |i: usize| {
            let at = all.iter().position(|&o| o == i).unwrap_or(0);
            all[..at]
                .iter()
                .filter(|o| !self.excluded.contains(o))
                .count()
        };
        let k = match (node, forward) {
            (Some(Node::Usage(i)), true) if !self.excluded.contains(i) => (before(*i) + 1) % n,
            (Some(Node::Usage(i)), true) => before(*i) % n,
            (Some(Node::Usage(i)), false) => (before(*i) + n - 1) % n,
            (Some(Node::Bucket(p)), true) => self.first_in(p).map_or(0, |i| before(i) % n),
            (Some(Node::Bucket(p)), false) => {
                self.first_in(p).map_or(n - 1, |i| (before(i) + n - 1) % n)
            }
            (_, true) => 0,
            (_, false) => n - 1,
        };
        Some(order[k])
    }

    /// The rows of the fully expanded tree, whatever is collapsed.
    pub fn rows_open(&self) -> Vec<Row> {
        let mut open = self.clone();
        open.collapsed.clear();
        open.rows()
    }

    /// The usages as a plain text (v9 behaviour 26): the symbol, the group row with its count,
    /// then every row of the fully expanded tree, two blanks deeper a level, a group with its
    /// count; excluded usages and what a filter hides are left out.
    pub fn export_text(&self) -> String {
        let mut out = String::new();
        for row in self.rows_open().into_iter().filter(|r| !r.excluded) {
            out.push_str(&"  ".repeat(row.depth));
            out.push_str(&row.label);
            if !row.detail.is_empty() {
                out.push_str("  (");
                out.push_str(&row.detail);
                out.push(')');
            }
            out.push('\n');
        }
        out
    }
}

/// A hit for a usage at `place`, `length` chars long, with its line's text from `line_text`.
pub fn hit(
    root: &Path,
    place: Place,
    length: usize,
    line_text: impl Fn(&Path, usize) -> Option<String>,
) -> Hit {
    let shown_path = place
        .path
        .strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| place.path.display().to_string());
    let line = line_text(&place.path, place.line).unwrap_or_default();
    let line = line.trim_end_matches(['\n', '\r']);
    let lead = line.chars().take_while(|c| c.is_whitespace()).count();
    let text = line.trim().to_owned();
    let n = text.chars().count();
    let start = place.column.saturating_sub(lead).min(n);
    let mark = start..(start + name_length(&text, start, length)).min(n);
    Hit {
        place,
        shown_path,
        text,
        mark,
        kind: None,
        context: None,
    }
}

/// The marked length of a usage at char `start` of `line`: the name there when the server's range
/// (`length` chars) covers more, as jdtls does for a method call (`SPEC-0293` v2 behaviour 13).
fn name_length(line: &str, start: usize, length: usize) -> usize {
    let name = line
        .chars()
        .skip(start)
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .count();
    if name > 0 { name.min(length) } else { length }
}

/// One line of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewLine {
    /// 1-based line number.
    pub number: usize,
    /// The line's text, untrimmed.
    pub text: String,
    /// Whether the usage is on this line.
    pub current: bool,
    /// The symbol on the usage's line, in chars.
    pub mark: Option<Range<usize>>,
}

/// The lines of `text` around a usage at 0-based `line` and char `column`, `length` chars long,
/// `context` lines before and after, clamped to the file (behaviour 6).
pub fn preview(
    text: &str,
    line: usize,
    column: usize,
    length: usize,
    context: usize,
) -> Vec<PreviewLine> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let line = line.min(lines.len() - 1);
    let first = line.saturating_sub(context);
    let last = (line + context).min(lines.len() - 1);
    (first..=last)
        .map(|i| {
            let t = lines[i].trim_end_matches('\r').to_owned();
            let n = t.chars().count();
            PreviewLine {
                number: i + 1,
                current: i == line,
                mark: (i == line).then(|| {
                    let start = column.min(n);
                    start..(start + name_length(&t, start, length)).min(n)
                }),
                text: t,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn h(path: &str, line: usize, column: usize) -> Hit {
        Hit {
            place: Place {
                path: PathBuf::from(format!("/w/{path}")),
                line,
                column,
            },
            shown_path: path.into(),
            text: format!("line {line}"),
            mark: 0..4,
            kind: None,
            context: None,
        }
    }

    fn tree() -> UsageTree {
        UsageTree::new(
            "Item",
            "Inventory.java:6".into(),
            false,
            vec![
                h("b/B.java", 4, 1),
                h("a/A.java", 9, 0),
                h("a/A.java", 2, 3),
                h("a/A.java", 2, 3),
            ],
        )
    }

    fn shown(t: &UsageTree) -> Vec<(usize, String, String)> {
        t.rows()
            .into_iter()
            .map(|r| (r.depth, r.label, r.detail))
            .collect()
    }

    fn kinds() -> UsageTree {
        let hit = |path: &str, line: usize, kind: UsageKind, module: &str| {
            let mut hit = h(path, line, 0);
            hit.kind = Some(kind);
            hit.context = Some(PlaceContext {
                module: module.into(),
                ..PlaceContext::default()
            });
            hit
        };
        let mut t = UsageTree::new(
            "count",
            String::new(),
            false,
            vec![
                hit("shop.api/src/a/A.java", 1, UsageKind::Import, "shop.api"),
                hit("shop.api/src/a/A.java", 5, UsageKind::Read, "shop.api"),
                hit("shop.api/src/a/b/B.java", 7, UsageKind::Write, "shop.api"),
                hit("tools/src/T.java", 3, UsageKind::Call, "tools"),
            ],
        );
        t.by_kind = false;
        t.by_structure = false;
        t
    }

    fn labels(t: &UsageTree) -> Vec<String> {
        t.rows()
            .into_iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.label))
            .collect()
    }

    /// SPEC-0293 AK-14 (v9, behaviour 22): a filter hides the usages of its kind only, and
    /// the counts, the order and the stepping follow.
    #[test]
    fn filters_hide_only_their_kind() {
        let mut t = kinds();
        assert_eq!(t.order().len(), 4);
        t.show_read = false;
        assert_eq!(t.filtered_out(), 1);
        assert_eq!(t.rows()[1].detail, "3 usages");
        assert!(!labels(&t).iter().any(|l| l.contains("5  line 5")));
        t.show_write = false;
        t.show_imports = false;
        // The call is no read, write or import: it stays.
        assert_eq!(t.order(), [3]);
        assert_eq!(t.step(None, true), Some(3));
        t.show_read = true;
        t.show_write = true;
        t.show_imports = true;
        assert_eq!(t.order().len(), 4);

        // With every usage hidden the group row stays and says so.
        let mut only = UsageTree::new("x", String::new(), false, {
            let mut hit = h("a/A.java", 1, 0);
            hit.kind = Some(UsageKind::Read);
            vec![hit]
        });
        only.show_read = false;
        let rows = only.rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].detail, "0 usages");
        assert!(!rows[1].expandable);
        assert_eq!(only.step(None, true), None);
        // A usage of unknown kind is never hidden.
        let mut unknown = tree();
        unknown.show_read = false;
        unknown.show_write = false;
        unknown.show_imports = false;
        assert_eq!(unknown.filtered_out(), 0);
    }

    /// SPEC-0293 AK-14 (v9, behaviours 23 and 24): one row per folder below the module's row,
    /// and a dotted module name below its groups.
    #[test]
    fn nested_folders_and_module_groups() {
        let mut t = kinds();
        t.by_package = false;
        assert_eq!(
            labels(&t),
            [
                "count",
                "Usages in Project Files",
                "  shop.api",
                "    A.java",
                "      2  line 1",
                "      6  line 5",
                "    B.java",
                "      8  line 7",
                "  tools",
                "    T.java",
                "      4  line 3",
            ]
        );
        t.nested_dirs = true;
        assert_eq!(
            labels(&t),
            [
                "count",
                "Usages in Project Files",
                "  shop.api",
                "    src",
                "      a",
                // A folder stands before the files beside it.
                "        b",
                "          B.java",
                "            8  line 7",
                "        A.java",
                "          2  line 1",
                "          6  line 5",
                "  tools",
                "    src",
                "      T.java",
                "        4  line 3",
            ]
        );
        let folder = t.rows().into_iter().find(|r| r.label == "src").unwrap();
        assert_eq!(folder.level, Some(Level::Dir));
        assert_eq!(folder.detail, "3 usages");
        // Without the module level the module's folder is a folder like the others.
        t.by_module = false;
        assert_eq!(labels(&t)[2], "  shop.api");
        assert_eq!(t.rows()[2].level, Some(Level::Dir));

        let mut t = kinds();
        t.by_package = false;
        t.flatten_modules = false;
        let shown = labels(&t);
        assert_eq!(shown[2..5], ["  shop", "    shop.api", "      A.java"]);
        // A name without dots has no group.
        assert!(shown.contains(&"  tools".to_owned()));
        assert_eq!(t.rows()[2].level, Some(Level::ModuleGroup));
        assert_eq!(t.rows()[2].detail, "3 usages");
    }

    /// SPEC-0293 AK-14 (v9, behaviour 25): excluded usages stay in the tree, are not counted
    /// and are passed by Next and Previous; a rerun keeps them.
    #[test]
    fn excluded_usages_are_passed_and_not_counted() {
        let mut t = kinds();
        t.by_package = false;
        let rows = t.rows();
        let usage = |t: &UsageTree, line: &str| {
            t.rows()
                .into_iter()
                .find(|r| r.label.starts_with(line))
                .unwrap()
        };
        // One usage.
        let read = usage(&t, "6  ");
        assert!(t.set_excluded(&read.node, true));
        assert!(!t.set_excluded(&read.node, true), "nothing changes twice");
        assert!(usage(&t, "6  ").excluded);
        assert_eq!(t.rows().len(), rows.len(), "it stays in the tree");
        assert_eq!(t.rows()[1].detail, "3 usages");
        assert_eq!(t.rows()[2].detail, "2 usages");
        assert_eq!(t.included().len(), 3);
        // Next from the usage before it passes it; so does Previous from the one after.
        let import = usage(&t, "2  ");
        let write = usage(&t, "8  ");
        assert_eq!(t.step(Some(&import.node), true), t.hit_of(&write.node));
        assert_eq!(t.step(Some(&write.node), false), t.hit_of(&import.node));
        // From the excluded usage itself: the next one that counts.
        assert_eq!(t.step(Some(&read.node), true), t.hit_of(&write.node));
        assert_eq!(t.step(Some(&read.node), false), t.hit_of(&import.node));

        // A group: everything below it, and the group row is marked.
        let module = t.rows()[2].clone();
        assert_eq!(module.label, "shop.api");
        assert!(t.set_excluded(&module.node, true));
        let module = t.rows()[2].clone();
        assert!(module.excluded);
        assert_eq!(module.detail, "0 usages");
        assert_eq!(t.rows()[1].detail, "1 usage");
        assert_eq!(t.step(None, true), Some(3));
        assert_eq!(t.step(Some(&Node::Usage(3)), true), Some(3));

        // A rerun of the same search keeps what is found again.
        let mut again = kinds();
        again.keep_excluded(&t);
        assert_eq!(again.included(), [3]);
        // Include takes it back; with nothing excluded it changes nothing.
        assert!(t.set_excluded(&Node::Group, false));
        assert!(!t.set_excluded(&Node::Group, false));
        // The row of the searched symbol stands for no usage.
        assert!(!t.set_excluded(&Node::Target, true));
        assert_eq!(t.included().len(), 4);
        // Everything excluded: nothing to step to.
        assert!(t.set_excluded(&Node::Group, true));
        assert_eq!(t.step(None, true), None);
        assert!(t.rows()[1].excluded);
    }

    /// SPEC-0293 AK-15 (v9, behaviour 26): the exported text is the open tree without what is
    /// excluded or hidden by a filter.
    #[test]
    fn the_export_leaves_out_what_is_excluded_or_filtered() {
        let mut t = kinds();
        t.by_package = false;
        t.declared = "A.java:3".into();
        t.set_expanded(&Node::Group, false);
        assert_eq!(
            t.export_text(),
            "count  (A.java:3)\n\
             Usages in Project Files  (4 usages)\n\
             \x20 shop.api  (3 usages)\n\
             \x20   A.java  (2 usages)\n\
             \x20     2  line 1\n\
             \x20     6  line 5\n\
             \x20   B.java  (1 usage)\n\
             \x20     8  line 7\n\
             \x20 tools  (1 usage)\n\
             \x20   T.java  (1 usage)\n\
             \x20     4  line 3\n"
        );
        t.show_imports = false;
        let write = t
            .rows_open()
            .into_iter()
            .find(|r| r.label.starts_with("8  "))
            .unwrap();
        t.set_excluded(&write.node, true);
        assert_eq!(
            t.export_text(),
            "count  (A.java:3)\n\
             Usages in Project Files  (2 usages)\n\
             \x20 shop.api  (1 usage)\n\
             \x20   A.java  (1 usage)\n\
             \x20     6  line 5\n\
             \x20 tools  (1 usage)\n\
             \x20   T.java  (1 usage)\n\
             \x20     4  line 3\n"
        );
    }

    /// SPEC-0293/AK-1: target, group with count, files with counts, usages sorted and once.
    #[test]
    fn groups_by_file_with_counts() {
        let t = tree();
        assert_eq!(
            shown(&t),
            [
                (0, "Item".into(), "Inventory.java:6".into()),
                (0, "Usages in Project Files".into(), "3 usages".into()),
                (1, "a/A.java".into(), "2 usages".into()),
                (2, "3  line 2".into(), String::new()),
                (2, "10  line 9".into(), String::new()),
                (1, "b/B.java".into(), "1 usage".into()),
                (2, "5  line 4".into(), String::new()),
            ]
        );
        let text = UsageTree::new(
            "String",
            "java.lang.String".into(),
            true,
            vec![h("a.java", 0, 0)],
        );
        assert_eq!(text.rows()[1].label, "Text occurrences in Project Files");
        assert_eq!(text.rows()[1].detail, "1 occurrence");
        let none = UsageTree::new("X", String::new(), false, Vec::new());
        assert_eq!(none.rows().len(), 1, "only the target without usages");
    }

    /// SPEC-0293 AK-13 (v8): a search that ends with nothing ends with words, and a search
    /// shown as running is ended by the window after twice the deadline.
    #[test]
    fn a_search_without_a_result_ends_with_words() {
        let mut empty = UsageTree::new("total", String::new(), false, Vec::new());
        assert_eq!(
            finished(&empty).as_deref(),
            Some("No usages of total in project files")
        );
        empty.in_file = Some("Cart.java".into());
        assert_eq!(
            finished(&empty).as_deref(),
            Some("No usages of total in Cart.java")
        );
        assert_eq!(no_usages("x", None), "No usages of x in project files");
        let secs = std::time::Duration::from_secs;
        assert_eq!(GIVE_UP_AFTER, secs(40));
        assert!(!overdue(secs(0)) && !overdue(secs(39)));
        assert!(overdue(secs(40)) && overdue(secs(4000)));
        assert_eq!(
            gave_up("total"),
            "The search for usages of total did not end within 40 s and was stopped. Rerun searches again."
        );
    }

    /// SPEC-0293 v7 AK-12: a search in one file names the file in the group row and the title.
    #[test]
    fn a_file_search_names_the_file() {
        let mut t = tree();
        t.in_file = Some("A.java".into());
        assert_eq!(
            shown(&t)[1],
            (0, "Usages in A.java".into(), "3 usages".into())
        );
        t.text = true;
        assert_eq!(t.rows()[1].label, "Text occurrences in A.java");
        assert_eq!(search_title("Item", Some("A.java")), "Item in A.java");
        assert_eq!(search_title("Item", None), "Item in Project Files");
    }

    /// SPEC-0293/AK-1: flat mode, collapse, expand, reveal.
    #[test]
    fn flat_collapse_and_reveal() {
        let mut t = tree();
        t.by_file = false;
        assert_eq!(
            shown(&t)[2..],
            [
                (1, "3  line 2".into(), "a/A.java:3".into()),
                (1, "10  line 9".into(), "a/A.java:10".into()),
                (1, "5  line 4".into(), "b/B.java:5".into()),
            ]
        );
        t.by_file = true;
        t.collapse_all();
        assert_eq!(t.rows().len(), 2);
        t.set_expanded(&Node::Group, true);
        assert_eq!(t.rows().len(), 4, "group open, files closed");
        assert_eq!(t.reveal(2), Some(4), "B.java opened for its usage");
        assert!(!t.rows()[2].expanded, "A.java stays closed");
        t.expand_all();
        assert_eq!(t.rows().len(), 7);
    }

    /// SPEC-0293/AK-1: next and previous wrap; rows stand for usages in the preview.
    #[test]
    fn steps_wrap_and_rows_have_hits() {
        let t = tree();
        assert_eq!(t.step(Some(&Node::Usage(2)), true), Some(0));
        assert_eq!(t.step(Some(&Node::Usage(0)), false), Some(2));
        let b = Node::Bucket(vec!["f:b/B.java".into()]);
        assert_eq!(t.step(Some(&b), true), Some(2));
        assert_eq!(t.step(Some(&b), false), Some(1));
        assert_eq!(t.step(Some(&Node::Target), true), Some(0));
        assert_eq!(t.step(None, false), Some(2));
        assert_eq!(t.hit_of(&b), Some(2));
        assert_eq!(t.hit_of(&Node::Group), Some(0));
        let none = UsageTree::new("X", String::new(), false, Vec::new());
        assert_eq!(none.step(None, true), None);
        assert_eq!(none.hit_of(&Node::Target), None);
    }

    fn ctx(module: &str, package: &str, class: Option<&str>, method: Option<&str>) -> PlaceContext {
        PlaceContext {
            module: module.into(),
            package: Some(package.into()),
            container: class.map(Into::into),
            method: method.map(Into::into),
            ..PlaceContext::default()
        }
    }

    fn java_tree() -> UsageTree {
        let mut a = h("src/lab/Discounts.java", 16, 20);
        a.kind = Some(UsageKind::Call);
        a.context = Some(ctx(
            "shop",
            "lab",
            Some("Discounts"),
            Some("priceOf(Item, Discount)"),
        ));
        let mut b = h("src/lab/Extra.java", 4, 30);
        b.kind = Some(UsageKind::Other);
        b.context = Some(ctx("shop", "lab", Some("Extra"), None));
        let mut c = h("src/lab/Discounts.java", 21, 8);
        c.kind = Some(UsageKind::Call);
        c.context = Some(ctx(
            "shop",
            "lab",
            Some("Discounts"),
            Some("bothPrices(Item)"),
        ));
        let mut d = h("main.c", 1, 25);
        d.kind = Some(UsageKind::Call);
        d.context = Some(PlaceContext {
            module: "shop".into(),
            method: Some("main".into()),
            ..PlaceContext::default()
        });
        UsageTree::new("apply", "Discount.java:5".into(), false, vec![a, b, c, d])
    }

    /// SPEC-0293 v3 AK-8: kind › module › package › class › method, no file level inside a type.
    #[test]
    fn groups_by_kind_module_package_class_and_method() {
        let t = java_tree();
        let rows: Vec<(usize, String, String)> = shown(&t)[2..].to_vec();
        assert_eq!(
            rows,
            [
                (1, "Calls".into(), "3 usages".into()),
                (2, "shop".into(), "3 usages".into()),
                (3, "main.c".into(), "1 usage".into()),
                (4, "main".into(), "1 usage".into()),
                (5, "2  line 1".into(), String::new()),
                (3, "lab".into(), "2 usages".into()),
                (4, "Discounts".into(), "2 usages".into()),
                (5, "bothPrices(Item)".into(), "1 usage".into()),
                (6, "22  line 21".into(), "src/lab/Discounts.java:22".into()),
                (5, "priceOf(Item, Discount)".into(), "1 usage".into()),
                (6, "17  line 16".into(), "src/lab/Discounts.java:17".into()),
                (1, "Other usages".into(), "1 usage".into()),
                (2, "shop".into(), "1 usage".into()),
                (3, "lab".into(), "1 usage".into()),
                (4, "Extra".into(), "1 usage".into()),
                (5, "5  line 4".into(), "src/lab/Extra.java:5".into()),
            ]
        );
        // Stepping follows the shown order: main.c, bothPrices, priceOf, Extra.
        let order = t.order();
        assert_eq!(t.step(Some(&Node::Usage(order[0])), true), Some(order[1]));
        assert_eq!(t.hits[order[1]].place.line, 21);
        assert_eq!(t.step(Some(&Node::Usage(order[3])), true), Some(order[0]));
    }

    /// SPEC-0293 v3 AK-8: each level can be switched off; collapsing hides what is below.
    #[test]
    fn levels_can_be_switched_off() {
        let mut t = java_tree();
        t.by_kind = false;
        t.by_module = false;
        t.by_package = false;
        t.by_structure = false;
        let labels: Vec<String> = t.rows()[2..].iter().map(|r| r.label.clone()).collect();
        assert_eq!(
            labels,
            [
                "main.c",
                "2  line 1",
                "src/lab/Discounts.java",
                "17  line 16",
                "22  line 21",
                "src/lab/Extra.java",
                "5  line 4"
            ]
        );
        t.by_structure = true;
        t.by_kind = true;
        let calls = t.rows()[2].node.clone();
        assert_eq!(t.rows()[2].level, Some(Level::Kind));
        t.set_expanded(&calls, false);
        assert_eq!(t.rows()[3].label, "Other usages", "calls collapsed");
        let last = t.order()[0];
        assert!(t.reveal(last).is_some());
        assert!(t.rows()[2].expanded);
        t.collapse_all();
        assert_eq!(t.rows().len(), 2);
        assert_eq!(t.file_count(), 3);
    }

    /// SPEC-0293/AK-1: a hit's mark sits on the symbol in the trimmed line.
    #[test]
    fn hit_marks_the_symbol_in_the_trimmed_line() {
        let place = Place {
            path: PathBuf::from("/w/src/A.java"),
            line: 5,
            column: 23,
        };
        let line = |_: &Path, _: usize| Some("    public record Item(String sku) {\n".to_owned());
        let hit = hit(Path::new("/w"), place, 6, line);
        assert_eq!(hit.shown_path, "src/A.java");
        assert_eq!(hit.text, "public record Item(String sku) {");
        let marked: String = hit
            .text
            .chars()
            .skip(hit.mark.start)
            .take(hit.mark.len())
            .collect();
        assert_eq!(marked, "String");
    }

    /// SPEC-0293 v2 AK-7: a range wider than the name marks the name only.
    #[test]
    fn only_the_name_is_marked() {
        let place = Place {
            path: PathBuf::from("/w/D.java"),
            line: 0,
            column: 20,
        };
        let text = "        return rule.apply(item.totalCents());";
        let line = |_: &Path, _: usize| Some(text.to_owned());
        let hit = hit(Path::new("/w"), place, 24, line);
        let marked: String = hit
            .text
            .chars()
            .skip(hit.mark.start)
            .take(hit.mark.len())
            .collect();
        assert_eq!(marked, "apply");
        assert_eq!(preview(text, 0, 20, 24, 2)[0].mark, Some(20..25));
        // A range that does not start on a name keeps its length.
        assert_eq!(name_length("a + b", 2, 1), 1);
        assert_eq!(name_length("ab", 0, 1), 1);
    }

    /// SPEC-0293/AK-2: lines around the usage, clamped at both ends of the file.
    #[test]
    fn preview_around_the_usage() {
        let text: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let p = preview(&text, 9, 5, 2, PREVIEW_CONTEXT);
        assert_eq!(p.len(), 13);
        assert_eq!((p[0].number, p[12].number), (4, 16));
        let current: Vec<&PreviewLine> = p.iter().filter(|l| l.current).collect();
        assert_eq!(current.len(), 1);
        assert_eq!(
            (current[0].number, current[0].mark.clone()),
            (10, Some(5..7))
        );
        let top = preview(&text, 1, 0, 4, PREVIEW_CONTEXT);
        assert_eq!((top[0].number, top.len()), (1, 8));
        let bottom = preview(&text, 19, 0, 4, PREVIEW_CONTEXT);
        assert_eq!((bottom.last().unwrap().number, bottom.len()), (20, 7));
        assert!(preview("", 0, 0, 1, 3).is_empty());
        assert_eq!(
            preview("ab\n", 0, 1, 9, 3)[0].mark,
            Some(1..2),
            "mark clamped"
        );
    }
}
