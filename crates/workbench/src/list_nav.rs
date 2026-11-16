//! First and last row of a list (`SPEC-0275` v19 behaviours 47 and 48): the keymap actions
//! `List-selectFirstRow`, `List-selectLastRow` and their two variants that extend the selection,
//! and the fixed keys Home and End, decided in one place for every list with keyboard
//! selection. A view asks [`edge`] (or one of its variants) with the pressed key and the number
//! of its rows and selects the row it gets.
//!
//! The chords come from the keymap in use (⌥↑, ⌥↓, ⌥⇧↑ and ⌥⇧↓ in the bundled macOS keymap, none
//! in the default one); Home, End, ⇧Home and ⇧End are fixed, as ↑ and ↓ are.

use idecraft_ui::keymap::{Keymap, Keystroke};

/// The keymap action that selects the first row of the focused list.
pub const SELECT_FIRST_ROW: &str = "List-selectFirstRow";
/// The keymap action that selects the last row of the focused list.
pub const SELECT_LAST_ROW: &str = "List-selectLastRow";
/// The keymap action that extends the selection to the first row.
pub const SELECT_FIRST_ROW_EXTEND: &str = "List-selectFirstRowExtendSelection";
/// The keymap action that extends the selection to the last row.
pub const SELECT_LAST_ROW_EXTEND: &str = "List-selectLastRowExtendSelection";

/// The four actions, in the order of the keymap files.
pub const ACTIONS: [&str; 4] = [
    SELECT_FIRST_ROW,
    SELECT_LAST_ROW,
    SELECT_FIRST_ROW_EXTEND,
    SELECT_LAST_ROW_EXTEND,
];

/// What a key means in a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    /// Select the first row.
    First,
    /// Select the last row.
    Last,
    /// Extend the selection to the first row.
    ExtendFirst,
    /// Extend the selection to the last row.
    ExtendLast,
}

impl ListKey {
    /// The key of a keymap action, if it is one of the four.
    pub fn of_action(id: &str) -> Option<Self> {
        match id {
            SELECT_FIRST_ROW => Some(Self::First),
            SELECT_LAST_ROW => Some(Self::Last),
            SELECT_FIRST_ROW_EXTEND => Some(Self::ExtendFirst),
            SELECT_LAST_ROW_EXTEND => Some(Self::ExtendLast),
            _ => None,
        }
    }

    /// The keymap action of the key.
    pub fn action(self) -> &'static str {
        match self {
            Self::First => SELECT_FIRST_ROW,
            Self::Last => SELECT_LAST_ROW,
            Self::ExtendFirst => SELECT_FIRST_ROW_EXTEND,
            Self::ExtendLast => SELECT_LAST_ROW_EXTEND,
        }
    }

    /// Whether the key goes to the first row (else to the last).
    pub fn to_first(self) -> bool {
        matches!(self, Self::First | Self::ExtendFirst)
    }

    /// Whether the key extends the selection instead of moving it.
    pub fn extends(self) -> bool {
        matches!(self, Self::ExtendFirst | Self::ExtendLast)
    }

    /// The row the key goes to in a list of `rows` rows; `None` for an empty list.
    pub fn row(self, rows: usize) -> Option<usize> {
        match rows {
            0 => None,
            _ if self.to_first() => Some(0),
            n => Some(n - 1),
        }
    }
}

/// Which keys a list takes for its first and last row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    /// The chords the keymap binds to the four actions.
    pub chords: bool,
    /// The fixed keys Home and End (with Shift for the extending variants).
    pub home_end: bool,
}

impl Keys {
    /// A list that has the keyboard to itself.
    pub const ALL: Self = Self {
        chords: true,
        home_end: true,
    };
    /// A list below a text field with text in it: Home and End move the caret in the field.
    pub const CHORDS: Self = Self {
        chords: true,
        home_end: false,
    };
    /// A list whose rows ⌥↑ and ⌥↓ move (the commits of an interactive rebase).
    pub const HOME_END: Self = Self {
        chords: false,
        home_end: true,
    };

    /// The keys of a list below a text field: Home and End only while the field is empty.
    pub fn beside_field(field_empty: bool) -> Self {
        if field_empty { Self::ALL } else { Self::CHORDS }
    }
}

/// The list key of a pressed key. `bound` are the keymap actions of the keystroke, which go
/// first; `key` is the key as the toolkit names it, `shift` and `others` (Control, Alt or the
/// platform key) the modifiers held with it.
pub fn list_key(
    bound: &[&str],
    key: &str,
    shift: bool,
    others: bool,
    keys: Keys,
) -> Option<ListKey> {
    if keys.chords
        && let Some(found) = bound.iter().find_map(|id| ListKey::of_action(id))
    {
        return Some(found);
    }
    if !keys.home_end || others {
        return None;
    }
    match (key, shift) {
        ("home", false) => Some(ListKey::First),
        ("end", false) => Some(ListKey::Last),
        ("home", true) => Some(ListKey::ExtendFirst),
        ("end", true) => Some(ListKey::ExtendLast),
        _ => None,
    }
}

/// [`list_key`] of a key event with the actions `keymap` binds to it.
pub fn key_in(keymap: &Keymap, k: &gpui::Keystroke, keys: Keys) -> Option<ListKey> {
    let m = &k.modifiers;
    let bound = Keystroke::from_platform(&k.key, m.control, m.alt, m.shift, m.platform)
        .map(|s| keymap.actions_for(&s, None))
        .unwrap_or_default();
    list_key(
        &bound,
        &k.key,
        m.shift,
        m.control || m.alt || m.platform,
        keys,
    )
}

/// The list key of a key event in the keymap in use.
pub fn key_of(k: &gpui::Keystroke, keys: Keys) -> Option<ListKey> {
    key_in(&crate::keymaps::current(), k, keys)
}

/// The row a key event selects in a list of `rows` rows with one selected row: the first or the
/// last. The variants that extend a selection select the same row there: one row is selected,
/// so the selection that reaches to the first row is the first row (behaviour 48). `None` for
/// every other key and for an empty list. The key is noted as carried out.
pub fn edge_with(k: &gpui::Keystroke, rows: usize, keys: Keys) -> Option<usize> {
    edge_among(k, rows, keys, |_| true)
}

/// [`edge_with`] for a list that has the keyboard to itself.
pub fn edge(k: &gpui::Keystroke, rows: usize) -> Option<usize> {
    edge_with(k, rows, Keys::ALL)
}

/// [`edge_with`] for a list with rows that cannot be selected (headings, separators): the
/// first or the last row `selectable` allows.
pub fn edge_among(
    k: &gpui::Keystroke,
    rows: usize,
    keys: Keys,
    selectable: impl Fn(usize) -> bool,
) -> Option<usize> {
    let key = key_of(k, keys)?;
    let row = selectable_edge(rows, key.to_first(), selectable)?;
    crate::key_diagnostics::note_ran(key.action(), "list");
    Some(row)
}

/// The first (`first`) or last of the rows `selectable` says can be selected, among `rows`
/// rows: for lists with headings or separators between their rows.
pub fn selectable_edge(
    rows: usize,
    first: bool,
    selectable: impl Fn(usize) -> bool,
) -> Option<usize> {
    if first {
        (0..rows).find(|&i| selectable(i))
    } else {
        (0..rows).rev().find(|&i| selectable(i))
    }
}

/// The views that select their first and last row through this module, by source file
/// (behaviour 47). The test below reads each file, so the list cannot name a view that does not.
pub const ADOPTERS: &[&str] = &[
    "attach_view.rs",
    "bookmarks_view.rs",
    "branches_view.rs",
    "choice_view.rs",
    "containers_view.rs",
    "endpoints_view.rs",
    "find_in_files_view.rs",
    "find_usages_view.rs",
    "git_log_view.rs",
    "goto_view.rs",
    "hierarchy_view.rs",
    "history_view.rs",
    "intentions_view.rs",
    "profiler_view.rs",
    "projects_view.rs",
    "python_view.rs",
    "rebase_view.rs",
    "rename_preview.rs",
    "run_view.rs",
    "spring_view.rs",
    "stash_view.rs",
    "structure_view.rs",
    "tab_list_view.rs",
    "themes_view.rs",
    "todo_view.rs",
    "usages_view.rs",
    "vcs_ops_view.rs",
    "workbench_view.rs",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(key: &str, shift: bool, alt: bool) -> gpui::Keystroke {
        gpui::Keystroke {
            modifiers: gpui::Modifiers {
                shift,
                alt,
                ..Default::default()
            },
            key: key.into(),
            key_char: None,
        }
    }

    /// SPEC-0275 AK-42 (v19): Home and End are fixed keys, the chords are the keymap's; a list
    /// below a text field with text leaves Home and End to the field, a list whose rows ⌥↑ and
    /// ⌥↓ move leaves the chords.
    #[test]
    fn home_end_and_the_keymap_chords_mean_first_and_last() {
        let none: [&str; 0] = [];
        let key = |k: &str, shift: bool| list_key(&none, k, shift, false, Keys::ALL);
        assert_eq!(key("home", false), Some(ListKey::First));
        assert_eq!(key("end", false), Some(ListKey::Last));
        assert_eq!(key("home", true), Some(ListKey::ExtendFirst));
        assert_eq!(key("end", true), Some(ListKey::ExtendLast));
        assert_eq!(key("up", false), None);
        // Home with Control, Alt or the platform key is another key.
        assert_eq!(list_key(&none, "home", false, true, Keys::ALL), None);
        // A keymap action goes first, whatever else the chord is bound to.
        let bound = ["EditorSelectWord", SELECT_FIRST_ROW];
        assert_eq!(
            list_key(&bound, "up", false, true, Keys::ALL),
            Some(ListKey::First)
        );
        assert_eq!(list_key(&bound, "up", false, true, Keys::HOME_END), None);
        assert_eq!(
            list_key(&none, "end", false, false, Keys::HOME_END),
            Some(ListKey::Last)
        );
        assert_eq!(list_key(&none, "home", false, false, Keys::CHORDS), None);
        assert_eq!(
            list_key(&[SELECT_LAST_ROW], "down", false, true, Keys::CHORDS),
            Some(ListKey::Last)
        );
        assert_eq!(Keys::beside_field(true), Keys::ALL);
        assert_eq!(Keys::beside_field(false), Keys::CHORDS);

        // The bundled keymaps: ⌥↑, ⌥↓, ⌥⇧↑ and ⌥⇧↓ on macOS, no chord in the default keymap.
        let (mac, default) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (key, shift, want) in [
            ("up", false, ListKey::First),
            ("down", false, ListKey::Last),
            ("up", true, ListKey::ExtendFirst),
            ("down", true, ListKey::ExtendLast),
        ] {
            let k = stroke(key, shift, true);
            assert_eq!(key_in(&mac, &k, Keys::ALL), Some(want), "{key}");
            assert_eq!(key_in(&mac, &k, Keys::HOME_END), None, "{key}");
            assert_eq!(key_in(&default, &k, Keys::ALL), None, "{key}");
        }
        for keymap in [&mac, &default] {
            assert_eq!(
                key_in(keymap, &stroke("home", false, false), Keys::ALL),
                Some(ListKey::First)
            );
            assert_eq!(
                key_in(keymap, &stroke("end", true, false), Keys::ALL),
                Some(ListKey::ExtendLast)
            );
        }
        for id in ACTIONS {
            assert_eq!(ListKey::of_action(id).map(ListKey::action), Some(id));
            assert!(!mac.shortcuts(id).is_empty(), "{id}");
            assert!(default.shortcuts(id).is_empty(), "{id}");
        }
        assert_eq!(ListKey::of_action("EditorUp"), None);
    }

    /// SPEC-0275 AK-42 (v19): the row a key goes to; nothing in an empty list; the first and
    /// last row that can be selected in a list with headings.
    #[test]
    fn keys_go_to_the_first_and_last_row() {
        assert_eq!(ListKey::First.row(5), Some(0));
        assert_eq!(ListKey::Last.row(5), Some(4));
        assert_eq!(ListKey::ExtendFirst.row(5), Some(0));
        assert_eq!(ListKey::ExtendLast.row(1), Some(0));
        assert_eq!(ListKey::First.row(0), None);
        assert_eq!(ListKey::Last.row(0), None);
        assert!(ListKey::ExtendLast.extends() && !ListKey::Last.extends());
        // Rows 0 and 3 are headings, row 5 a separator.
        let ok = |i: usize| !matches!(i, 0 | 3 | 5);
        assert_eq!(selectable_edge(6, true, ok), Some(1));
        assert_eq!(selectable_edge(6, false, ok), Some(4));
        assert_eq!(selectable_edge(6, true, |_| false), None);
        assert_eq!(selectable_edge(0, false, |_| true), None);
    }

    /// SPEC-0275 AK-42 (v19): every view the specification lists selects its first and last
    /// row through this module.
    #[test]
    fn every_listed_view_uses_the_shared_navigation() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sorted = ADOPTERS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, ADOPTERS, "sorted, each once");
        for file in ADOPTERS {
            let text = std::fs::read_to_string(src.join(file)).unwrap_or_else(|e| {
                panic!("{file}: {e}");
            });
            assert!(text.contains("list_nav::"), "{file}");
        }
    }
}
