//! The mouse and keyboard behaviour every tree shares (`SPEC-0275` behaviours 8–11), without
//! toolkit: a view describes its visible rows, asks what a click or key means, and carries out the
//! answer on its own model.
//!
//! - A click on the disclosure arrow toggles the node; a click on the row selects it.
//! - A double click on a leaf opens it, on a node with children toggles it.
//! - → expands, or moves to the first child; ← collapses, or moves to the parent.
//! - ↑/↓ move the selection, Home/End go to the first/last row, Enter opens a leaf and toggles a
//!   node, ⌘↓ (F4) opens.
//! - Numpad + / − expand / collapse and numpad * expands everything below: the keymap actions
//!   `ExpandTreeNode`, `CollapseTreeNode` and `FullyExpandTreeNode`, on whatever keys the keymap in
//!   use binds them to (v13 behaviour 32).
//! - Several rows (v3 behaviour 24): ⇧-click selects a range from the anchor, ⌘-click (Ctrl
//!   elsewhere) adds or removes a row, ⇧↑/⇧↓ extend from the anchor, ⌘A selects every row.

/// What a tree needs to know of one visible row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavRow {
    /// Nesting level.
    pub depth: usize,
    /// Whether the row can be expanded (it has children, or they are not read yet).
    pub expandable: bool,
    /// Whether it is expanded.
    pub expanded: bool,
}

impl NavRow {
    /// A row.
    pub fn new(depth: usize, expandable: bool, expanded: bool) -> Self {
        Self {
            depth,
            expandable,
            expanded,
        }
    }
}

/// A key the trees understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKey {
    /// ↑
    Up,
    /// ↓
    Down,
    /// ←
    Left,
    /// →
    Right,
    /// Home
    Home,
    /// End
    End,
    /// Enter
    Enter,
    /// ⌘↓ (Jump to Source) or F4.
    Open,
    /// `ExpandTreeNode` (numpad +).
    Expand,
    /// `CollapseTreeNode` (numpad −).
    Collapse,
    /// `FullyExpandTreeNode` (numpad *).
    ExpandFully,
}

/// The keymap action that expands the selected node (v13 behaviour 32).
pub const EXPAND_NODE: &str = "ExpandTreeNode";
/// The keymap action that collapses the selected node, or goes to its parent.
pub const COLLAPSE_NODE: &str = "CollapseTreeNode";
/// The keymap action that expands the selected node with everything below it.
pub const EXPAND_FULLY: &str = "FullyExpandTreeNode";

/// How many nodes `FullyExpandTreeNode` opens at most.
pub const FULL_EXPAND_LIMIT: usize = 200;

/// The tree key of a keymap action, if it is one of the three.
pub fn action_key(id: &str) -> Option<TreeKey> {
    match id {
        EXPAND_NODE => Some(TreeKey::Expand),
        COLLAPSE_NODE => Some(TreeKey::Collapse),
        EXPAND_FULLY => Some(TreeKey::ExpandFully),
        _ => None,
    }
}

/// The tree key of a pressed key: `bound` are the keymap actions of the keystroke (see
/// `keymaps::tree_actions_in`), which decide expanding and collapsing with or without modifiers;
/// the other keys are fixed (v13 behaviour 32).
pub fn tree_key_bound(
    bound: &[&str],
    key: &str,
    platform: bool,
    other_modifiers: bool,
) -> Option<TreeKey> {
    bound
        .iter()
        .find_map(|id| action_key(id))
        .or_else(|| tree_key(key, platform, other_modifiers))
}

/// The next node to open while everything below row `at` is expanded: the first row of `at` and
/// the rows below it that can be expanded and is not. `None` when all are open.
pub fn next_to_expand(rows: &[NavRow], at: usize) -> Option<usize> {
    let depth = rows.get(at)?.depth;
    let end = (at + 1..rows.len())
        .find(|&j| rows[j].depth <= depth)
        .unwrap_or(rows.len());
    (at..end).find(|&j| rows[j].expandable && !rows[j].expanded)
}

/// Expands row `at` and every row below it (`FullyExpandTreeNode`), at most
/// [`FULL_EXPAND_LIMIT`] nodes: `rows` gives the visible rows after each step and `expand` opens
/// one. A node that stays closed after `expand` (its children are still being read) ends it.
/// Returns how many nodes were opened.
pub fn fully_expand<T>(
    tree: &mut T,
    at: usize,
    rows: impl Fn(&T) -> Vec<NavRow>,
    mut expand: impl FnMut(&mut T, usize),
) -> usize {
    let mut opened = 0;
    let mut last = None;
    while opened < FULL_EXPAND_LIMIT {
        let Some(next) = next_to_expand(&rows(tree), at) else {
            break;
        };
        if last == Some(next) {
            break;
        }
        expand(tree, next);
        last = Some(next);
        opened += 1;
    }
    opened
}

/// What the view does with its model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeCommand {
    /// Nothing.
    None,
    /// Select row `n`.
    Select(usize),
    /// Expand row `n` (it stays selected).
    Expand(usize),
    /// Expand row `n` and every row below it ([`fully_expand`]).
    ExpandFully(usize),
    /// Collapse row `n`.
    Collapse(usize),
    /// Open row `n` (a file, a table, a test, a problem…).
    Open(usize),
}

/// The fixed key a keystroke means in a tree: `key` as the toolkit names it (`up`, `left`,
/// `enter`, `home`, `f4`), with the modifiers. `platform` is ⌘ on macOS, Ctrl elsewhere. Expanding
/// and collapsing come from the keymap ([`tree_key_bound`]).
pub fn tree_key(key: &str, platform: bool, other_modifiers: bool) -> Option<TreeKey> {
    if other_modifiers {
        return None;
    }
    if platform {
        return (key == "down").then_some(TreeKey::Open);
    }
    Some(match key {
        "up" => TreeKey::Up,
        "down" => TreeKey::Down,
        "left" => TreeKey::Left,
        "right" => TreeKey::Right,
        "home" => TreeKey::Home,
        "end" => TreeKey::End,
        "enter" => TreeKey::Enter,
        "f4" => TreeKey::Open,
        _ => return None,
    })
}

/// The parent of row `i`: the nearest row above with a smaller depth.
pub fn parent(rows: &[NavRow], i: usize) -> Option<usize> {
    let depth = rows.get(i)?.depth;
    (0..i).rev().find(|&j| rows[j].depth < depth)
}

fn toggle(rows: &[NavRow], i: usize) -> TreeCommand {
    match rows.get(i) {
        Some(r) if r.expandable && r.expanded => TreeCommand::Collapse(i),
        Some(r) if r.expandable => TreeCommand::Expand(i),
        _ => TreeCommand::None,
    }
}

/// What `key` does with row `selected` selected.
pub fn on_key(rows: &[NavRow], selected: usize, key: TreeKey) -> TreeCommand {
    if rows.is_empty() {
        return TreeCommand::None;
    }
    let at = selected.min(rows.len() - 1);
    let row = rows[at];
    match key {
        TreeKey::Up => TreeCommand::Select(at.saturating_sub(1)),
        TreeKey::Down => TreeCommand::Select((at + 1).min(rows.len() - 1)),
        TreeKey::Home => TreeCommand::Select(0),
        TreeKey::End => TreeCommand::Select(rows.len() - 1),
        TreeKey::Right if row.expandable && !row.expanded => TreeCommand::Expand(at),
        TreeKey::Right => match rows.get(at + 1) {
            Some(next) if row.expanded && next.depth > row.depth => TreeCommand::Select(at + 1),
            _ => TreeCommand::None,
        },
        TreeKey::Left if row.expandable && row.expanded => TreeCommand::Collapse(at),
        TreeKey::Left => parent(rows, at).map_or(TreeCommand::None, TreeCommand::Select),
        TreeKey::Enter if row.expandable => toggle(rows, at),
        TreeKey::Enter | TreeKey::Open => TreeCommand::Open(at),
        TreeKey::Expand if row.expandable && !row.expanded => TreeCommand::Expand(at),
        TreeKey::Collapse if row.expandable && row.expanded => TreeCommand::Collapse(at),
        TreeKey::Collapse => parent(rows, at).map_or(TreeCommand::None, TreeCommand::Select),
        TreeKey::Expand => TreeCommand::None,
        TreeKey::ExpandFully if row.expandable => TreeCommand::ExpandFully(at),
        TreeKey::ExpandFully => TreeCommand::None,
    }
}

/// What `key` does in a tree that may have no selected row yet (`selected` is `None` until the
/// user selects one, `SPEC-0192` v7 behaviour 3b), as commands in order: ↑, ↓ and Home select the
/// first row, End the last; any other key selects the first row and acts on it.
pub fn on_key_from(rows: &[NavRow], selected: Option<usize>, key: TreeKey) -> Vec<TreeCommand> {
    let mut out = match (selected, key) {
        (Some(at), key) => vec![on_key(rows, at, key)],
        (None, _) if rows.is_empty() => Vec::new(),
        (None, TreeKey::Up | TreeKey::Down | TreeKey::Home) => vec![TreeCommand::Select(0)],
        (None, TreeKey::End) => vec![TreeCommand::Select(rows.len() - 1)],
        (None, key) => vec![TreeCommand::Select(0), on_key(rows, 0, key)],
    };
    out.retain(|c| *c != TreeCommand::None);
    out
}

/// What a click with the primary button does on row `i`: the commands in order (select first).
/// `on_arrow` when the click hit the disclosure arrow; `clicks` is the click count.
pub fn on_click(rows: &[NavRow], i: usize, on_arrow: bool, clicks: usize) -> Vec<TreeCommand> {
    let Some(row) = rows.get(i) else {
        return Vec::new();
    };
    let mut out = vec![TreeCommand::Select(i)];
    if on_arrow {
        // Each click on the arrow toggles; a double click on it toggles twice, as in the reference.
        out.push(toggle(rows, i));
    } else if clicks == 2 {
        out.push(if row.expandable {
            toggle(rows, i)
        } else {
            TreeCommand::Open(i)
        });
    }
    out.retain(|c| *c != TreeCommand::None);
    out
}

/// How a click changes a selection of several rows (`SPEC-0275` v3 behaviour 24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMode {
    /// A plain click: only this row.
    Replace,
    /// ⇧-click: the rows from the anchor to this one.
    Range,
    /// ⌘-click (Ctrl elsewhere): this row in or out.
    Toggle,
}

/// The click mode of the modifiers; `platform` is ⌘ on macOS, Ctrl elsewhere.
pub fn click_mode(shift: bool, platform: bool) -> ClickMode {
    if shift {
        ClickMode::Range
    } else if platform {
        ClickMode::Toggle
    } else {
        ClickMode::Replace
    }
}

/// A selection key of a tree with several selected rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKey {
    /// ⇧↑
    ExtendUp,
    /// ⇧↓
    ExtendDown,
    /// ⌘A (Ctrl+A elsewhere)
    All,
}

/// The selection key a keystroke means: ⇧↑/⇧↓ without other modifiers, ⌘A (`platform`) alone.
pub fn select_key(key: &str, shift: bool, platform: bool, others: bool) -> Option<SelectKey> {
    if others {
        return None;
    }
    match (key, shift, platform) {
        ("up", true, false) => Some(SelectKey::ExtendUp),
        ("down", true, false) => Some(SelectKey::ExtendDown),
        ("a", false, true) => Some(SelectKey::All),
        _ => None,
    }
}

/// Selected rows of a tree (`SPEC-0275` v3 behaviour 24): the rows in order, the anchor of ⇧
/// ranges and the lead (the row clicked or reached last, always selected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSelection {
    /// Selected rows, ascending.
    pub rows: Vec<usize>,
    /// Where ⇧-click and ⇧↑/⇧↓ ranges start.
    pub anchor: usize,
    /// The row with the focus.
    pub lead: usize,
}

impl RowSelection {
    /// Row `i` alone.
    pub fn single(i: usize) -> Self {
        Self {
            rows: vec![i],
            anchor: i,
            lead: i,
        }
    }

    fn range(anchor: usize, lead: usize) -> Self {
        Self {
            rows: (anchor.min(lead)..=anchor.max(lead)).collect(),
            anchor,
            lead,
        }
    }

    /// The selection after a click on row `i`.
    pub fn click(&self, i: usize, mode: ClickMode) -> Self {
        match mode {
            ClickMode::Replace => Self::single(i),
            ClickMode::Range => Self::range(self.anchor, i),
            ClickMode::Toggle => {
                let mut rows = self.rows.clone();
                if let Some(at) = rows.iter().position(|&r| r == i) {
                    rows.remove(at);
                    // The lead stays on a selected row: the nearest to the click.
                    let Some(&lead) = rows.iter().min_by_key(|&&r| r.abs_diff(i)) else {
                        return Self::single(i);
                    };
                    Self {
                        rows,
                        anchor: i,
                        lead,
                    }
                } else {
                    rows.push(i);
                    rows.sort_unstable();
                    Self {
                        rows,
                        anchor: i,
                        lead: i,
                    }
                }
            }
        }
    }

    /// The selection after a selection key, in a tree of `len` visible rows.
    pub fn key(&self, key: SelectKey, len: usize) -> Self {
        if len == 0 {
            return self.clone();
        }
        let last = len - 1;
        match key {
            SelectKey::ExtendUp => Self::range(self.anchor.min(last), self.lead.saturating_sub(1)),
            SelectKey::ExtendDown => Self::range(self.anchor.min(last), (self.lead + 1).min(last)),
            SelectKey::All => Self {
                rows: (0..len).collect(),
                anchor: self.anchor.min(last),
                lead: self.lead.min(last),
            },
        }
    }

    /// The selection extended from its anchor to row `to`, in a tree of `len` visible rows
    /// (`SPEC-0275` v19 behaviour 48): every row between them is selected, `to` becomes the
    /// lead and the anchor stays, so the other end can be chosen afterwards.
    pub fn extended_to(&self, to: usize, len: usize) -> Self {
        if len == 0 {
            return self.clone();
        }
        let last = len - 1;
        Self::range(self.anchor.min(last), to.min(last))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0192 v7 behaviour 3b (OBS-0070 G1): a tree without a selected row selects the first
    /// row on ↑/↓/Home and the last on End; another key selects the first row and acts on it.
    #[test]
    fn keys_in_a_tree_without_selection() {
        use TreeCommand as C;
        let rows = [
            NavRow::new(0, true, false),
            NavRow::new(0, true, false),
            NavRow::new(0, false, false),
        ];
        for key in [TreeKey::Down, TreeKey::Up, TreeKey::Home] {
            assert_eq!(on_key_from(&rows, None, key), [C::Select(0)], "{key:?}");
        }
        assert_eq!(on_key_from(&rows, None, TreeKey::End), [C::Select(2)]);
        assert_eq!(
            on_key_from(&rows, None, TreeKey::Right),
            [C::Select(0), C::Expand(0)]
        );
        assert_eq!(on_key_from(&rows, None, TreeKey::Left), [C::Select(0)]);
        assert!(on_key_from(&[], None, TreeKey::Down).is_empty());
        assert_eq!(on_key_from(&rows, Some(0), TreeKey::Down), [C::Select(1)]);
    }

    /// SPEC-0275 v3 behaviour 24 (AK-13).
    #[test]
    fn several_rows_are_selected_with_shift_and_command() {
        let one = RowSelection::single(2);
        let range = one.click(5, click_mode(true, false));
        assert_eq!(range.rows, [2, 3, 4, 5]);
        assert_eq!((range.anchor, range.lead), (2, 5));
        // ⇧-click again from the same anchor, upwards.
        assert_eq!(range.click(0, ClickMode::Range).rows, [0, 1, 2]);
        // ⌘-click adds a row and makes it the anchor; again removes it.
        let added = range.click(8, click_mode(false, true));
        assert_eq!(added.rows, [2, 3, 4, 5, 8]);
        assert_eq!((added.anchor, added.lead), (8, 8));
        let removed = added.click(4, ClickMode::Toggle);
        assert_eq!(removed.rows, [2, 3, 5, 8]);
        assert!(removed.rows.contains(&removed.lead), "lead stays selected");
        assert_eq!(removed.anchor, 4);
        assert_eq!(
            RowSelection::single(3).click(3, ClickMode::Toggle),
            RowSelection::single(3),
            "the last row stays"
        );
        // A plain click selects one row again.
        assert_eq!(
            added.click(1, click_mode(false, false)),
            RowSelection::single(1)
        );
        // ⇧↓ twice, ⇧↑ three times: grows down, then shrinks and grows up from the anchor.
        let mut s = RowSelection::single(3);
        s = s.key(SelectKey::ExtendDown, 6);
        s = s.key(SelectKey::ExtendDown, 6);
        assert_eq!(s.rows, [3, 4, 5]);
        assert_eq!(s.key(SelectKey::ExtendDown, 6).rows, [3, 4, 5], "last row");
        for _ in 0..3 {
            s = s.key(SelectKey::ExtendUp, 6);
        }
        assert_eq!(s.rows, [2, 3]);
        assert_eq!((s.anchor, s.lead), (3, 2));
        let all = s.key(SelectKey::All, 6);
        assert_eq!(all.rows, [0, 1, 2, 3, 4, 5]);
        assert_eq!(all.lead, 2);
        assert_eq!(
            select_key("up", true, false, false),
            Some(SelectKey::ExtendUp)
        );
        assert_eq!(
            select_key("down", true, false, false),
            Some(SelectKey::ExtendDown)
        );
        assert_eq!(select_key("a", false, true, false), Some(SelectKey::All));
        assert_eq!(select_key("a", false, false, false), None);
        assert_eq!(select_key("up", true, false, true), None);
        assert_eq!(select_key("up", false, false, false), None);
    }

    /// SPEC-0275 AK-43 (v19): the selection reaches from its anchor to the first or the last
    /// row; the anchor stays, so the other end can follow.
    #[test]
    fn a_selection_extends_to_the_first_and_the_last_row() {
        let s = RowSelection::single(3);
        let up = s.extended_to(0, 6);
        assert_eq!(up.rows, [0, 1, 2, 3]);
        assert_eq!((up.anchor, up.lead), (3, 0));
        let down = up.extended_to(5, 6);
        assert_eq!(down.rows, [3, 4, 5], "from the anchor, not from the lead");
        assert_eq!((down.anchor, down.lead), (3, 5));
        // A row behind the end is the last row; an empty tree keeps what it has.
        assert_eq!(s.extended_to(9, 6).rows, [3, 4, 5]);
        assert_eq!(s.extended_to(0, 0), s);
        assert_eq!(RowSelection::single(0).extended_to(0, 4).rows, [0]);
    }

    /// root(expanded) > a(expanded) > a1, a2 ; b(collapsed) ; c (leaf)
    fn rows() -> Vec<NavRow> {
        vec![
            NavRow::new(0, true, true),
            NavRow::new(1, true, true),
            NavRow::new(2, false, false),
            NavRow::new(2, false, false),
            NavRow::new(1, true, false),
            NavRow::new(1, false, false),
        ]
    }

    #[test]
    fn arrows_move_expand_and_collapse() {
        let r = rows();
        assert_eq!(on_key(&r, 0, TreeKey::Up), TreeCommand::Select(0));
        assert_eq!(on_key(&r, 0, TreeKey::Down), TreeCommand::Select(1));
        assert_eq!(on_key(&r, 5, TreeKey::Down), TreeCommand::Select(5));
        assert_eq!(on_key(&r, 4, TreeKey::Right), TreeCommand::Expand(4));
        assert_eq!(
            on_key(&r, 1, TreeKey::Right),
            TreeCommand::Select(2),
            "expanded: first child"
        );
        assert_eq!(on_key(&r, 2, TreeKey::Right), TreeCommand::None, "leaf");
        assert_eq!(on_key(&r, 1, TreeKey::Left), TreeCommand::Collapse(1));
        assert_eq!(
            on_key(&r, 3, TreeKey::Left),
            TreeCommand::Select(1),
            "parent"
        );
        assert_eq!(on_key(&r, 4, TreeKey::Left), TreeCommand::Select(0));
        assert_eq!(on_key(&r, 0, TreeKey::Left), TreeCommand::Collapse(0));
        assert_eq!(on_key(&r, 3, TreeKey::Home), TreeCommand::Select(0));
        assert_eq!(on_key(&r, 0, TreeKey::End), TreeCommand::Select(5));
        assert_eq!(on_key(&[], 0, TreeKey::Down), TreeCommand::None);
    }

    #[test]
    fn enter_opens_leaves_and_toggles_nodes() {
        let r = rows();
        assert_eq!(on_key(&r, 2, TreeKey::Enter), TreeCommand::Open(2));
        assert_eq!(on_key(&r, 4, TreeKey::Enter), TreeCommand::Expand(4));
        assert_eq!(on_key(&r, 1, TreeKey::Enter), TreeCommand::Collapse(1));
        assert_eq!(on_key(&r, 5, TreeKey::Open), TreeCommand::Open(5));
        assert_eq!(on_key(&r, 4, TreeKey::Expand), TreeCommand::Expand(4));
        assert_eq!(on_key(&r, 1, TreeKey::Expand), TreeCommand::None);
        assert_eq!(on_key(&r, 1, TreeKey::Collapse), TreeCommand::Collapse(1));
        assert_eq!(on_key(&r, 2, TreeKey::Collapse), TreeCommand::Select(1));
    }

    #[test]
    fn clicks_select_toggle_on_the_arrow_and_open_on_double_click() {
        let r = rows();
        assert_eq!(on_click(&r, 4, false, 1), [TreeCommand::Select(4)]);
        assert_eq!(
            on_click(&r, 4, true, 1),
            [TreeCommand::Select(4), TreeCommand::Expand(4)],
            "one click on the arrow expands"
        );
        assert_eq!(
            on_click(&r, 1, true, 1),
            [TreeCommand::Select(1), TreeCommand::Collapse(1)]
        );
        assert_eq!(
            on_click(&r, 4, false, 2),
            [TreeCommand::Select(4), TreeCommand::Expand(4)],
            "double click on a folder toggles"
        );
        assert_eq!(
            on_click(&r, 5, false, 2),
            [TreeCommand::Select(5), TreeCommand::Open(5)],
            "double click on a leaf opens"
        );
        assert!(on_click(&r, 9, false, 1).is_empty());
    }

    #[test]
    fn keys_from_keystrokes() {
        assert_eq!(tree_key("left", false, false), Some(TreeKey::Left));
        assert_eq!(tree_key("down", true, false), Some(TreeKey::Open));
        assert_eq!(tree_key("f4", false, false), Some(TreeKey::Open));
        assert_eq!(tree_key("up", true, false), None);
        assert_eq!(tree_key("up", false, true), None);
        assert_eq!(tree_key("a", false, false), None);
        assert_eq!(parent(&rows(), 3), Some(1));
        assert_eq!(parent(&rows(), 0), None);
    }

    /// SPEC-0275 AK-30 (v13): expanding and collapsing are the keymap's actions.
    #[test]
    fn expand_and_collapse_follow_the_keymap() {
        use idecraft_ui::keymap::{Keymap, KeymapFile, Keystroke, Shortcut, platform_parent};
        let key = |keymap: &Keymap, key: &str, platform: bool| {
            let bound = crate::keymaps::tree_actions_in(keymap, key, false, false, false, platform);
            let bound: Vec<&str> = bound.iter().map(String::as_str).collect();
            tree_key_bound(&bound, key, platform, false)
        };
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            assert_eq!(key(&keymap, "+", false), Some(TreeKey::Expand));
            assert_eq!(key(&keymap, "-", false), Some(TreeKey::Collapse));
            assert_eq!(key(&keymap, "*", false), Some(TreeKey::ExpandFully));
            // The fixed keys stay, and a letter is no tree key.
            assert_eq!(key(&keymap, "left", false), Some(TreeKey::Left));
            assert_eq!(key(&keymap, "a", false), None);
        }
        // With the platform key the same signs belong to Expand All and Collapse All.
        let mac = Keymap::bundled_macos();
        assert_eq!(key(&mac, "-", true), None);
        assert_eq!(key(&mac, "+", true), None);
        // A key the user gives the action does it with its modifiers; the old key no longer.
        let mut user = KeymapFile::user("mine", platform_parent());
        user.set_action(
            EXPAND_FULLY,
            Some(vec![Shortcut::Keyboard {
                first: Keystroke::parse("control alt X").unwrap(),
                second: None,
            }]),
        );
        user.set_action(COLLAPSE_NODE, Some(Vec::new()));
        let keymap = Keymap::bundled_default().with_overrides(&user);
        let bound = crate::keymaps::tree_actions_in(&keymap, "x", true, true, false, false);
        let bound: Vec<&str> = bound.iter().map(String::as_str).collect();
        assert_eq!(
            tree_key_bound(&bound, "x", false, true),
            Some(TreeKey::ExpandFully)
        );
        assert_eq!(key(&keymap, "*", false), None);
        assert_eq!(key(&keymap, "-", false), None);
        assert_eq!(key(&keymap, "+", false), Some(TreeKey::Expand));
        assert_eq!(action_key("ExpandAll"), None);
    }

    /// SPEC-0275 AK-30 (v13): everything below the node opens, nothing beside it.
    #[test]
    fn fully_expanding_opens_what_is_below() {
        // A tree by parent: 0 root, 1 and 4 below it, 2 and 3 below 1, 5 below 4; 3 and 5 leaves
        // with nothing below, 2 a node with one leaf (6).
        struct Tree {
            parent: Vec<Option<usize>>,
            open: Vec<bool>,
        }
        impl Tree {
            fn children(&self, n: usize) -> Vec<usize> {
                (0..self.parent.len())
                    .filter(|&c| self.parent[c] == Some(n))
                    .collect()
            }
            fn walk(&self, n: usize, depth: usize, out: &mut Vec<(usize, NavRow)>) {
                let kids = self.children(n);
                out.push((n, NavRow::new(depth, !kids.is_empty(), self.open[n])));
                if self.open[n] {
                    for c in kids {
                        self.walk(c, depth + 1, out);
                    }
                }
            }
            fn rows(&self) -> Vec<(usize, NavRow)> {
                let mut out = Vec::new();
                self.walk(0, 0, &mut out);
                out
            }
        }
        let nav = |t: &Tree| t.rows().into_iter().map(|(_, r)| r).collect::<Vec<_>>();
        let expand = |t: &mut Tree, row: usize| {
            let node = t.rows()[row].0;
            t.open[node] = true;
        };
        let mut tree = Tree {
            parent: vec![None, Some(0), Some(1), Some(1), Some(0), Some(4), Some(2)],
            open: vec![true, false, false, false, false, false, false],
        };
        // Row 1 is node 1: it opens with node 2 below it; node 4 beside it stays closed.
        assert_eq!(
            on_key(&nav(&tree), 1, TreeKey::ExpandFully),
            TreeCommand::ExpandFully(1)
        );
        assert_eq!(fully_expand(&mut tree, 1, nav, expand), 2);
        assert_eq!(tree.open, [true, true, true, false, false, false, false]);
        assert_eq!(fully_expand(&mut tree, 1, nav, expand), 0, "all open");
        // On a leaf the key does nothing.
        let leaf = nav(&tree).iter().position(|r| !r.expandable).unwrap();
        assert_eq!(
            on_key(&nav(&tree), leaf, TreeKey::ExpandFully),
            TreeCommand::None
        );
        // A node that does not open (its children are still read) ends the walk.
        let mut lazy = Tree {
            parent: vec![None, Some(0)],
            open: vec![false, false],
        };
        assert_eq!(fully_expand(&mut lazy, 0, nav, |_, _| {}), 1);
        // No more than the limit: a chain of 300 nodes.
        let n = 300;
        let mut chain = Tree {
            parent: (0..n).map(|i: usize| i.checked_sub(1)).collect(),
            open: vec![false; n],
        };
        assert_eq!(fully_expand(&mut chain, 0, nav, expand), FULL_EXPAND_LIMIT);
        assert_eq!(chain.open.iter().filter(|o| **o).count(), FULL_EXPAND_LIMIT);
    }
}
