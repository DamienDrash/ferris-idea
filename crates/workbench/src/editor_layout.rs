//! Nested editor groups (`SPEC-0112` v5 behaviours 10–12), without toolkit: a binary tree of
//! splits whose leaves are editor groups, the reading order `NextSplitter` cycles through, the
//! neighbour that takes an emptied group's space, divider ratios, and the layout with its tabs as
//! remembered per project (`SPEC-0059` v9.1 behaviour 22e).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tabs::SavedTabs;

/// An editor group's id; [`MAIN`] is the group files open in.
pub type GroupId = usize;

/// The main group (`SPEC-0112` behaviour 2).
pub const MAIN: GroupId = 0;

/// Smallest and largest share of a divider's first side (v5.1 behaviour 12).
pub const RATIOS: (f32, f32) = (0.1, 0.9);

/// A node of the layout: a group, or two nodes side by side (`down`: one above the other).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    /// An editor group.
    Group {
        /// Its id.
        id: GroupId,
    },
    /// A divided area.
    Split {
        /// One above the other instead of side by side.
        down: bool,
        /// Share of the first (left or upper) side.
        ratio: f32,
        /// Left or upper side.
        first: Box<Node>,
        /// Right or lower side.
        second: Box<Node>,
    },
}

/// Where a split is: the sides taken from the top, `false` for first and `true` for second.
pub type SplitPath = Vec<bool>;

impl Node {
    fn groups_into(&self, out: &mut Vec<GroupId>) {
        match self {
            Node::Group { id } => out.push(*id),
            Node::Split { first, second, .. } => {
                first.groups_into(out);
                second.groups_into(out);
            }
        }
    }

    fn at_mut(&mut self, path: &[bool]) -> Option<&mut Node> {
        match (path.split_first(), self) {
            (None, n) => Some(n),
            (Some((side, rest)), Node::Split { first, second, .. }) => {
                if *side { second } else { first }.at_mut(rest)
            }
            _ => None,
        }
    }

    fn path_of(&self, id: GroupId, path: &mut SplitPath) -> bool {
        match self {
            Node::Group { id: g } => *g == id,
            Node::Split { first, second, .. } => {
                path.push(false);
                if first.path_of(id, path) {
                    return true;
                }
                path.pop();
                path.push(true);
                if second.path_of(id, path) {
                    return true;
                }
                path.pop();
                false
            }
        }
    }

    fn splits_into(&self, path: &mut SplitPath, out: &mut Vec<(SplitPath, bool, f32)>) {
        if let Node::Split {
            down,
            ratio,
            first,
            second,
        } = self
        {
            out.push((path.clone(), *down, *ratio));
            path.push(false);
            first.splits_into(path, out);
            path.pop();
            path.push(true);
            second.splits_into(path, out);
            path.pop();
        }
    }
}

/// The layout of the editor area.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    root: Node,
    next: GroupId,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            root: Node::Group { id: MAIN },
            next: MAIN + 1,
        }
    }
}

impl Layout {
    /// The tree.
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// The groups in reading order (left to right, top to bottom within each split).
    pub fn groups(&self) -> Vec<GroupId> {
        let mut out = Vec::new();
        self.root.groups_into(&mut out);
        out
    }

    /// Whether the area is split.
    pub fn is_split(&self) -> bool {
        matches!(self.root, Node::Split { .. })
    }

    /// Whether `id` is a group of the layout.
    pub fn contains(&self, id: GroupId) -> bool {
        self.groups().contains(&id)
    }

    /// Divides group `id` (`SplitVertically`: right of it, `SplitHorizontally` with `down`: below
    /// it); returns the new group's id (v5 behaviour 10, v5.1 behaviour 12).
    pub fn split(&mut self, id: GroupId, down: bool) -> Option<GroupId> {
        let mut path = Vec::new();
        if !self.root.path_of(id, &mut path) {
            return None;
        }
        let new = self.next;
        self.next += 1;
        let leaf = self.root.at_mut(&path)?;
        *leaf = Node::Split {
            down,
            ratio: 0.5,
            first: Box::new(Node::Group { id }),
            second: Box::new(Node::Group { id: new }),
        };
        Some(new)
    }

    /// Removes group `id`; its sibling takes its space. The last group stays.
    pub fn remove(&mut self, id: GroupId) -> bool {
        let mut path = Vec::new();
        if !self.root.path_of(id, &mut path) || path.is_empty() {
            return false;
        }
        let side = path.pop().unwrap_or(false);
        let Some(parent) = self.root.at_mut(&path) else {
            return false;
        };
        if let Node::Split { first, second, .. } = parent {
            let keep = if side { first } else { second };
            let keep = std::mem::replace(keep.as_mut(), Node::Group { id: MAIN });
            *parent = keep;
            return true;
        }
        false
    }

    /// Gives group `old` the id `new` (the main group taking an emptied place, v5.1 behaviour 12).
    pub fn relabel(&mut self, old: GroupId, new: GroupId) {
        let mut path = Vec::new();
        if self.root.path_of(old, &mut path)
            && let Some(Node::Group { id }) = self.root.at_mut(&path)
        {
            *id = new;
        }
    }

    /// The group after (or before) `id` in reading order, wrapping (`NextSplitter`,
    /// `PrevSplitter`); `id` itself when it is the only one.
    pub fn next(&self, id: GroupId, forward: bool) -> GroupId {
        let groups = self.groups();
        let n = groups.len();
        match groups.iter().position(|g| *g == id) {
            Some(i) if n > 1 => {
                groups[if forward {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                }]
            }
            _ => id,
        }
    }

    /// The group a tab of `id` moves to (Move Tab to Opposite Group), if there is another one.
    pub fn neighbour(&self, id: GroupId) -> Option<GroupId> {
        Some(self.next(id, true)).filter(|g| *g != id)
    }

    /// Every split with its path, direction and ratio, outermost first.
    pub fn splits(&self) -> Vec<(SplitPath, bool, f32)> {
        let mut out = Vec::new();
        self.root.splits_into(&mut Vec::new(), &mut out);
        out
    }

    /// Sets the ratio of the split at `path`, within [`RATIOS`] (a dragged divider).
    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        if let Some(Node::Split { ratio, .. }) = self.root.at_mut(path)
            && value.is_finite()
        {
            *ratio = value.clamp(RATIOS.0, RATIOS.1);
        }
    }

    /// The place of group `id` from the top, as words: `left`/`right` across a side-by-side split,
    /// `top`/`bottom` across one above the other (`SPEC-0112` v5 behaviour 11).
    pub fn place(&self, id: GroupId) -> Vec<&'static str> {
        let mut path = Vec::new();
        if !self.root.path_of(id, &mut path) {
            return Vec::new();
        }
        let mut node = &self.root;
        let mut out = Vec::new();
        for side in path {
            if let Node::Split {
                down,
                first,
                second,
                ..
            } = node
            {
                out.push(match (*down, side) {
                    (false, false) => "left",
                    (false, true) => "right",
                    (true, false) => "top",
                    (true, true) => "bottom",
                });
                node = if side { second } else { first };
            }
        }
        out
    }

    /// A layout from a remembered tree: `None` when it does not hold the main group exactly once
    /// or repeats an id.
    pub fn from_node(root: Node) -> Option<Self> {
        let mut groups = Vec::new();
        root.groups_into(&mut groups);
        let mut sorted = groups.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != groups.len() || !groups.contains(&MAIN) {
            return None;
        }
        let mut layout = Self {
            next: sorted.last().copied().unwrap_or(MAIN) + 1,
            root,
        };
        for (path, _, ratio) in layout.splits() {
            layout.set_ratio(&path, ratio);
        }
        Some(layout)
    }
}

/// One group as remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedGroup {
    /// The group's id in the layout.
    pub id: GroupId,
    /// Its tabs.
    #[serde(flatten)]
    pub tabs: SavedTabs,
}

/// The editor area as remembered per project in `.ferris-idea/editor-tabs.json`: layout, tabs per
/// group with pins and the active tab, and the focused group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedEditors {
    /// The tree of splits.
    pub layout: Node,
    /// The groups' tabs; paths inside the project are relative to it.
    pub groups: Vec<SavedGroup>,
    /// The group with the focus.
    #[serde(default)]
    pub focused: GroupId,
    /// The ids of the tool windows shown when it was written (`SPEC-0061` v13 behaviour 59);
    /// `None` in a file of an earlier version, which leaves the tool windows as they start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_windows: Option<Vec<String>>,
}

/// The tool windows to show again from the saved ids: known ones once each, in order; unknown ids
/// are left out (`SPEC-0061` v13 behaviour 59).
pub fn restored_tool_windows(ids: &[String]) -> Vec<ferris_idea_ui::tool_windows::ToolWindow> {
    let mut out = Vec::new();
    for w in ids
        .iter()
        .filter_map(|id| ferris_idea_ui::tool_windows::ToolWindow::from_id(id))
    {
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// Where the editor area is remembered.
pub fn saved_editors_file(root: &Path) -> PathBuf {
    root.join(".ferris-idea").join("editor-tabs.json")
}

fn relative(root: &Path, p: &Path) -> PathBuf {
    p.strip_prefix(root)
        .map_or(p.to_path_buf(), Path::to_path_buf)
}

impl SavedEditors {
    /// Writes the editor area of the project at `root`, paths relative to it.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let mut rel = self.clone();
        for g in &mut rel.groups {
            for p in g.tabs.paths.iter_mut().chain(g.tabs.pinned.iter_mut()) {
                *p = relative(root, p);
            }
            if let Some(a) = &mut g.tabs.active {
                *a = relative(root, a);
            }
        }
        let file = saved_editors_file(root);
        // The open tabs are state of one user: the generated ignore file lists them before
        // they are written (`SPEC-0069` behaviour 3a).
        ferris_idea_core::local_state::refresh_local_state_ignore(root);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&rel).map_err(std::io::Error::other)?;
        ferris_idea_core::write_atomic(&file, text)
    }

    /// The remembered editor area of the project at `root`, paths made absolute; `None` when
    /// there is none or it cannot be read. A layout that does not match its groups falls back to
    /// the main group alone.
    pub fn load(root: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(saved_editors_file(root)).ok()?;
        let mut saved: Self = serde_json::from_str(&text).ok()?;
        for g in &mut saved.groups {
            for p in g.tabs.paths.iter_mut().chain(g.tabs.pinned.iter_mut()) {
                if p.is_relative() {
                    *p = root.join(&*p);
                }
            }
            if let Some(a) = &mut g.tabs.active
                && a.is_relative()
            {
                *a = root.join(&*a);
            }
        }
        let ids: Vec<GroupId> = saved.groups.iter().map(|g| g.id).collect();
        let fits = Layout::from_node(saved.layout.clone())
            .is_some_and(|l| l.groups().iter().all(|g| ids.contains(g)));
        if !fits {
            saved.layout = Node::Group { id: MAIN };
            saved.groups.retain(|g| g.id == MAIN);
            saved.focused = MAIN;
        }
        Some(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0112 AK-6: Split Right then Split Down in the right group gives a left group and a
    /// right column of two; NextSplitter cycles through all in reading order.
    #[test]
    fn nested_splits_and_reading_order() {
        let mut l = Layout::default();
        assert!(!l.is_split());
        let right = l.split(MAIN, false).unwrap();
        let below = l.split(right, true).unwrap();
        assert_eq!(l.groups(), [MAIN, right, below]);
        assert_eq!(l.place(MAIN), ["left"]);
        assert_eq!(l.place(below), ["right", "bottom"]);
        assert_eq!(l.next(MAIN, true), right);
        assert_eq!(l.next(below, true), MAIN, "wraps");
        assert_eq!(l.next(MAIN, false), below);
        assert_eq!(l.neighbour(right), Some(below));
        assert_eq!(l.splits().len(), 2);
        assert!(l.split(42, false).is_none());
    }

    /// SPEC-0112 AK-6: closing a middle group gives its space to its sibling.
    #[test]
    fn closing_a_middle_group() {
        let mut l = Layout::default();
        let a = l.split(MAIN, false).unwrap();
        let b = l.split(a, false).unwrap();
        let c = l.split(b, true).unwrap();
        assert_eq!(l.groups(), [MAIN, a, b, c]);
        assert!(l.remove(b));
        assert_eq!(l.groups(), [MAIN, a, c]);
        assert_eq!(l.place(c), ["right", "right"], "c took b's place");
        assert!(l.remove(a));
        assert!(l.remove(c));
        assert_eq!(l.groups(), [MAIN]);
        assert!(!l.remove(MAIN), "the last group stays");
        // The main group can leave; another group then takes the main id.
        let d = l.split(MAIN, true).unwrap();
        assert!(l.remove(MAIN));
        l.relabel(d, MAIN);
        assert_eq!(l.groups(), [MAIN]);
    }

    /// SPEC-0112 AK-6: ratios are clamped and survive a save and reload, with the tabs.
    #[test]
    fn ratios_survive_a_save_and_reload() {
        let mut l = Layout::default();
        let right = l.split(MAIN, false).unwrap();
        let below = l.split(right, true).unwrap();
        l.set_ratio(&[], 0.3);
        l.set_ratio(&[true], 0.99);
        assert_eq!(l.splits()[1].2, RATIOS.1);
        let dir = std::env::temp_dir().join(format!("ferris-idea-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tabs = |names: &[&str]| SavedTabs {
            paths: names.iter().map(|n| dir.join(n)).collect(),
            pinned: vec![dir.join(names[0])],
            active: Some(dir.join(names[names.len() - 1])),
        };
        let saved = SavedEditors {
            layout: l.root().clone(),
            groups: vec![
                SavedGroup {
                    id: MAIN,
                    tabs: tabs(&["a.rs", "b.rs"]),
                },
                SavedGroup {
                    id: right,
                    tabs: tabs(&["c.rs"]),
                },
                SavedGroup {
                    id: below,
                    tabs: tabs(&["a.rs"]),
                },
            ],
            focused: below,
            tool_windows: Some(vec!["terminal".into(), "database".into()]),
        };
        saved.save(&dir).unwrap();
        // SPEC-0069 AK-6 (v4): writing the tabs makes sure the ignore file lists them.
        let ignore = std::fs::read_to_string(dir.join(".ferris-idea/.gitignore")).unwrap();
        assert!(ignore.lines().any(|l| l == "editor-tabs.json"), "{ignore}");
        let text = std::fs::read_to_string(saved_editors_file(&dir)).unwrap();
        assert!(
            !text.contains(&*dir.to_string_lossy()),
            "paths relative to the project"
        );
        let back = SavedEditors::load(&dir).unwrap();
        assert_eq!(back, saved);
        let again = Layout::from_node(back.layout).unwrap();
        assert_eq!(again.splits()[0].2, 0.3);
        assert_eq!(again.groups(), [MAIN, right, below]);
        // A new split after reloading gets a fresh id.
        let mut again = again;
        assert!(again.split(MAIN, true).unwrap() > below);
        // A layout naming a group without tabs falls back to the main group.
        let broken = SavedEditors {
            groups: vec![saved.groups[0].clone()],
            ..saved.clone()
        };
        broken.save(&dir).unwrap();
        let fallback = SavedEditors::load(&dir).unwrap();
        assert_eq!(fallback.layout, Node::Group { id: MAIN });
        assert_eq!(fallback.groups.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(Layout::from_node(Node::Group { id: 3 }).is_none());
    }

    /// SPEC-0061 v13 AK-v13: the tool windows shown at quitting come back, each once, unknown ids
    /// left out; a file of an earlier version has none and leaves the start as it is.
    #[test]
    fn the_shown_tool_windows_come_back() {
        use ferris_idea_ui::tool_windows::ToolWindow;
        let ids: Vec<String> = ["terminal", "database", "no-such-window", "terminal"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            restored_tool_windows(&ids),
            [ToolWindow::Terminal, ToolWindow::Database]
        );
        // A file of an earlier version has no such field and reads as `None`.
        let earlier = SavedEditors {
            layout: Node::Group { id: MAIN },
            groups: Vec::new(),
            focused: MAIN,
            tool_windows: None,
        };
        let old = serde_json::to_string(&earlier).unwrap();
        assert!(!old.contains("tool_windows"), "{old}");
        let read: SavedEditors = serde_json::from_str(&old).unwrap();
        assert_eq!(read.tool_windows, None);
        let text = serde_json::to_string(&SavedEditors {
            tool_windows: Some(vec!["terminal".into()]),
            ..read
        })
        .unwrap();
        assert!(text.contains(r#""tool_windows":["terminal"]"#), "{text}");
    }
}
