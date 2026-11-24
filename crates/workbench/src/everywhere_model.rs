//! Rows, filter and selection of the Search Everywhere popup (`SPEC-0067` behaviours 2–4), without
//! toolkit.

use std::path::Path;

use ferris_idea_languages::endpoints::EndpointHit;
use ferris_idea_languages::everywhere::{
    ActionHit, Categories, Everywhere, FileHit, SymbolHit, TextHit,
};

/// The keymap action that opens the popup on the Endpoints tab (v9 behaviour 20).
pub const GOTO_ENDPOINT: &str = "GotoUrlAction";

/// The keymap action that switches to the next tab (Tab in the bundled keymaps; v7 behaviour 18).
pub const NEXT_TAB: &str = "SearchEverywhere.NextTab";
/// The keymap action that switches to the previous tab (⇧Tab in the bundled keymaps).
pub const PREV_TAB: &str = "SearchEverywhere.PrevTab";
/// The keymap action that chooses the selected result (Enter in the bundled keymaps).
pub const SELECT_ITEM: &str = "SearchEverywhere.SelectItem";

/// The keymap action that goes to the next group of results (Page Down and ⌘↓ in the bundled
/// macOS keymap; v10 behaviour 21).
pub const NEXT_GROUP: &str = "SearchEverywhere.NavigateToNextGroup";
/// The keymap action that goes to the previous group of results (Page Up and ⌘↑).
pub const PREV_GROUP: &str = "SearchEverywhere.NavigateToPrevGroup";

/// The keymap action that opens the selected result in the right split (⇧Enter in the bundled
/// keymaps; v8 behaviour 19).
pub const OPEN_RIGHT: &str = "OpenInRightSplit";

/// What a key does in the popup (behaviours 1, 3–5 and 11; v7 behaviour 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKey {
    /// Select the previous result.
    Up,
    /// Select the next result.
    Down,
    /// Choose the selected result, `right` in the right split (⇧Enter).
    Choose {
        /// Open in the right split.
        right: bool,
    },
    /// Switch to the next tab.
    NextTab,
    /// Switch to the previous tab.
    PrevTab,
    /// Select the first result of the next group (v10 behaviour 21).
    NextGroup,
    /// Select the first result of this or the previous group.
    PrevGroup,
    /// Close the filter menu or the popup.
    Escape,
}

/// The popup's operation for the keymap action `id`, if it is one of its four (v7 behaviour 18,
/// v8 behaviour 19).
pub fn popup_action(id: &str) -> Option<PopupKey> {
    match id {
        SELECT_ITEM => Some(PopupKey::Choose { right: false }),
        OPEN_RIGHT => Some(PopupKey::Choose { right: true }),
        NEXT_TAB => Some(PopupKey::NextTab),
        PREV_TAB => Some(PopupKey::PrevTab),
        NEXT_GROUP => Some(PopupKey::NextGroup),
        PREV_GROUP => Some(PopupKey::PrevGroup),
        _ => None,
    }
}

/// What the popup does with `key` (toolkit name), `bound` being the keymap actions of the pressed
/// keystroke: choosing, opening in the right split and switching tabs follow the keymap, with
/// or without modifiers (v7 behaviour 18, v8 behaviour 19); Up, Down and Escape are fixed keys,
/// taken only without Control, Alt and the platform key (`other_modifiers`).
pub fn popup_key(
    key: &str,
    shift: bool,
    other_modifiers: bool,
    bound: &[&str],
) -> Option<PopupKey> {
    if let Some(action) = [
        SELECT_ITEM,
        OPEN_RIGHT,
        NEXT_TAB,
        PREV_TAB,
        NEXT_GROUP,
        PREV_GROUP,
    ]
    .into_iter()
    .find(|id| bound.contains(id))
    .and_then(popup_action)
    {
        return Some(action);
    }
    if other_modifiers {
        return None;
    }
    match (key, shift) {
        ("up", false) => Some(PopupKey::Up),
        ("down", false) => Some(PopupKey::Down),
        ("escape", false) => Some(PopupKey::Escape),
        _ => None,
    }
}

/// The tab of the popup: all categories or one of them (behaviour 1, v5 tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Filter {
    /// Files, symbols, actions and text.
    #[default]
    All,
    /// Class-like symbols only (v5).
    Classes,
    /// Workspace files only.
    Files,
    /// Symbols only.
    Symbols,
    /// Actions and settings pages only.
    Actions,
    /// Lines of text (v5).
    Text,
    /// The HTTP endpoints of the Endpoints tool window (v9 behaviour 20).
    Endpoints,
}

impl Filter {
    /// Every tab, in the order of the tab row.
    pub const ALL: [Filter; 7] = [
        Filter::All,
        Filter::Classes,
        Filter::Files,
        Filter::Symbols,
        Filter::Actions,
        Filter::Text,
        Filter::Endpoints,
    ];

    /// Categories searched for this tab.
    pub fn categories(self) -> Categories {
        match self {
            Filter::All => Categories::ALL,
            Filter::Classes => Categories {
                classes: true,
                ..Categories::NONE
            },
            Filter::Files => Categories {
                files: true,
                ..Categories::NONE
            },
            Filter::Symbols => Categories {
                symbols: true,
                ..Categories::NONE
            },
            Filter::Actions => Categories {
                actions: true,
                ..Categories::NONE
            },
            Filter::Text => Categories {
                text: true,
                ..Categories::NONE
            },
            // The endpoints are no category of `search_everywhere`: the tab searches the rows
            // of the Endpoints model (`ferris_idea_languages::endpoints::search_endpoints`).
            Filter::Endpoints => Categories::NONE,
        }
    }

    /// Most results per category: fewer on the All tab (behaviour 2).
    pub fn limit(self) -> usize {
        match self {
            Filter::All => 8,
            Filter::Text => 200,
            _ => 50,
        }
    }

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Classes => "Classes",
            Filter::Files => "Files",
            Filter::Symbols => "Symbols",
            Filter::Actions => "Actions",
            Filter::Text => "Text",
            Filter::Endpoints => "Endpoints",
        }
    }

    /// The next tab (Tab key), wrapping around.
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The previous tab (Shift+Tab), wrapping around.
    pub fn previous(self) -> Self {
        let i = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Whether the tab lists project items that "include non-project items" can widen (v5
    /// behaviour 13): files, classes, symbols, text and All. The endpoints are those of the
    /// project's own sources (v9 behaviour 20).
    pub fn has_scope(self) -> bool {
        !matches!(self, Filter::Actions | Filter::Endpoints)
    }
}

/// One row of the result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// Category heading (All tab only).
    Header(&'static str),
    /// The value of an arithmetic query (v5 behaviour 15).
    Calculator(String),
    /// A file.
    File(FileHit),
    /// A class-like symbol on the Classes tab or group.
    Class(SymbolHit),
    /// A symbol.
    Symbol(SymbolHit),
    /// An action or a settings page.
    Action(ActionHit),
    /// A line of text.
    Text(TextHit),
    /// An HTTP endpoint (v9 behaviour 20).
    Endpoint(EndpointHit),
}

impl Row {
    /// Workspace-relative path of a file, symbol or text row.
    pub fn path(&self) -> Option<&str> {
        match self {
            Row::File(f) => Some(&f.path),
            Row::Class(s) | Row::Symbol(s) => Some(&s.path),
            Row::Text(t) => Some(&t.path),
            Row::Endpoint(e) => e.endpoint.file.to_str(),
            _ => None,
        }
    }

    /// 0-based line a symbol or text row points at.
    pub fn line(&self) -> Option<usize> {
        match self {
            Row::Class(s) | Row::Symbol(s) => Some(s.line),
            Row::Text(t) => Some(t.line),
            Row::Endpoint(e) => Some(e.endpoint.line),
            _ => None,
        }
    }
}

/// Whether a hit's file passes the file-type filter: `hidden` holds the extensions turned off
/// (v5 behaviour 14); files without an extension count as `""`.
pub fn type_shown(path: &str, hidden: &std::collections::BTreeSet<String>) -> bool {
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    !hidden.contains(&ext)
}

/// Rows of a result with the selected one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Results {
    /// Headings (All tab) followed by their hits; empty categories have no heading.
    pub rows: Vec<Row>,
    /// Index of the selected hit.
    pub selected: Option<usize>,
}

impl Results {
    /// Rows in the order calculator, files, classes, symbols, actions, text; headings only on the
    /// All tab (v5 behaviour 3); hits of file types in `hidden` left out; the first hit is selected.
    pub fn from(
        found: Everywhere,
        filter: Filter,
        hidden: &std::collections::BTreeSet<String>,
    ) -> Self {
        let mut rows = Vec::new();
        let headed = filter == Filter::All;
        let mut group = |title: &'static str, hits: Vec<Row>| {
            if !hits.is_empty() {
                if headed {
                    rows.push(Row::Header(title));
                }
                rows.extend(hits);
            }
        };
        let shown = |p: &str| type_shown(p, hidden);
        if let Some(value) = found.calculator {
            group("Calculator", vec![Row::Calculator(value)]);
        }
        group(
            "Files",
            found
                .files
                .unwrap_or_default()
                .into_iter()
                .filter(|f| shown(&f.path))
                .map(Row::File)
                .collect(),
        );
        group(
            "Classes",
            found
                .classes
                .unwrap_or_default()
                .into_iter()
                .filter(|s| shown(&s.path))
                .map(Row::Class)
                .collect(),
        );
        group(
            "Symbols",
            found
                .symbols
                .unwrap_or_default()
                .into_iter()
                .filter(|s| shown(&s.path))
                .map(Row::Symbol)
                .collect(),
        );
        group(
            "Actions",
            found
                .actions
                .unwrap_or_default()
                .into_iter()
                .map(Row::Action)
                .collect(),
        );
        group(
            "Text",
            found
                .text
                .unwrap_or_default()
                .into_iter()
                .filter(|t| shown(&t.path))
                .map(Row::Text)
                .collect(),
        );
        let selected = rows.iter().position(|r| !matches!(r, Row::Header(_)));
        Self { rows, selected }
    }

    /// The rows of the Endpoints tab (v9 behaviour 20): the hits in their order, without a
    /// heading, those of file types in `hidden` left out; the first is selected.
    pub fn of_endpoints(
        hits: Vec<EndpointHit>,
        hidden: &std::collections::BTreeSet<String>,
    ) -> Self {
        let rows: Vec<Row> = hits
            .into_iter()
            .map(Row::Endpoint)
            .filter(|r| r.path().is_none_or(|p| type_shown(p, hidden)))
            .collect();
        let selected = (!rows.is_empty()).then_some(0);
        Self { rows, selected }
    }

    /// Moves the selection over hits, skipping headings, wrapping around (behaviour 3).
    pub fn step(&mut self, down: bool) {
        let hits: Vec<usize> = (0..self.rows.len())
            .filter(|i| !matches!(self.rows[*i], Row::Header(_)))
            .collect();
        let Some(at) = self
            .selected
            .and_then(|s| hits.iter().position(|h| *h == s))
        else {
            return;
        };
        let n = hits.len();
        let next = if down { (at + 1) % n } else { (at + n - 1) % n };
        self.selected = Some(hits[next]);
    }

    /// Moves the selection by a group of results (v10 behaviour 21), a group being the hits
    /// below one heading. Down: the first hit of the next group, in the last group the last
    /// hit. Up: the first hit of the group the selection is in, from there the first hit of the
    /// group before. A list without headings is one group. Never wraps around.
    pub fn step_group(&mut self, down: bool) {
        let Some(at) = self.selected else {
            return;
        };
        let hit = |i: usize| !matches!(self.rows[i], Row::Header(_));
        // A hit starts a group when it is the first row or follows a heading.
        let starts = |i: usize| hit(i) && (i == 0 || !hit(i - 1));
        let target = if down {
            (at + 1..self.rows.len())
                .find(|&i| starts(i))
                .or_else(|| (0..self.rows.len()).rev().find(|&i| hit(i)))
        } else {
            (0..at)
                .rev()
                .find(|&i| starts(i))
                .or_else(|| (0..self.rows.len()).find(|&i| hit(i)))
        };
        if let Some(i) = target {
            self.selected = Some(i);
        }
    }

    /// The selected hit.
    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected?)
    }
}

/// The double-Shift gesture (`SPEC-0067` v4 behaviour 1): two taps of Shift within 400 ms, a tap
/// being Shift pressed while no other modifier is held and released before any key or other
/// modifier. The Shift left over from ⌘⇧← or of a capital letter is no tap (OBS-0021 item 12).
#[derive(Debug, Clone)]
pub struct DoubleShift {
    last_tap: Option<std::time::Instant>,
    pressed_alone: bool,
    none_held: bool,
}

impl Default for DoubleShift {
    fn default() -> Self {
        Self {
            last_tap: None,
            pressed_alone: false,
            none_held: true,
        }
    }
}

impl DoubleShift {
    /// The modifiers changed to `shift_only` (Shift and nothing else) or `none` held, at `now`;
    /// `true` when this completes the gesture.
    pub fn modifiers(&mut self, shift_only: bool, none: bool, now: std::time::Instant) -> bool {
        let was_none = std::mem::replace(&mut self.none_held, none);
        if shift_only {
            self.pressed_alone = was_none;
            return false;
        }
        if !(none && std::mem::take(&mut self.pressed_alone)) {
            self.pressed_alone = false;
            return false;
        }
        match self.last_tap {
            Some(t) if now.duration_since(t) < std::time::Duration::from_millis(400) => {
                self.last_tap = None;
                true
            }
            _ => {
                self.last_tap = Some(now);
                false
            }
        }
    }

    /// A key was pressed: it ends a tap and the gesture.
    pub fn key(&mut self) {
        self.last_tap = None;
        self.pressed_alone = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0067 AK-12 (v11): the group actions go to the first result of a group and never
    /// wrap; their keys come from the keymap.
    #[test]
    fn group_steps_go_to_the_first_result_of_a_group() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        // Any row that is no heading is a result.
        let file = |name: &str| Row::Calculator(name.into());
        // Rows 0, 3 and 6 are headings.
        let mut r = Results {
            rows: vec![
                Row::Header("Files"),
                file("a"),
                file("b"),
                Row::Header("Classes"),
                file("c"),
                file("d"),
                Row::Header("Actions"),
                file("e"),
                file("f"),
            ],
            selected: Some(2),
        };
        let go = |r: &mut Results, down: bool| {
            r.step_group(down);
            r.selected.unwrap()
        };
        assert_eq!(go(&mut r, true), 4, "the first result of the next group");
        assert_eq!(go(&mut r, true), 7);
        assert_eq!(go(&mut r, true), 8, "in the last group: the last result");
        assert_eq!(go(&mut r, true), 8, "no wrap");
        assert_eq!(go(&mut r, false), 7, "the first result of its own group");
        assert_eq!(go(&mut r, false), 4, "from there: of the group before");
        assert_eq!(go(&mut r, false), 1);
        assert_eq!(go(&mut r, false), 1, "no wrap");
        // A list without headings is one group.
        let mut flat = Results {
            rows: vec![file("a"), file("b"), file("c")],
            selected: Some(1),
        };
        assert_eq!(go(&mut flat, true), 2);
        assert_eq!(go(&mut flat, false), 0);
        // An empty list stays as it is.
        let mut none = Results {
            rows: Vec::new(),
            selected: None,
        };
        none.step_group(true);
        assert_eq!(none.selected, None);

        let stroke = |text: &str| Keystroke::parse(text).expect("a keystroke");
        let mac = Keymap::bundled_macos();
        let default = Keymap::bundled_default();
        for (keymap, text, key) in [
            (&mac, "PAGE_DOWN", PopupKey::NextGroup),
            (&mac, "meta DOWN", PopupKey::NextGroup),
            (&mac, "PAGE_UP", PopupKey::PrevGroup),
            (&mac, "meta UP", PopupKey::PrevGroup),
            (&default, "control DOWN", PopupKey::NextGroup),
            (&default, "control UP", PopupKey::PrevGroup),
        ] {
            assert_eq!(
                popup_key("", false, true, &keymap.actions_for(&stroke(text), None)),
                Some(key),
                "{text}"
            );
        }
        assert_eq!(popup_action(NEXT_GROUP), Some(PopupKey::NextGroup));
        assert_eq!(popup_action(PREV_GROUP), Some(PopupKey::PrevGroup));
    }

    /// SPEC-0067 v7 AK-10: choosing and switching tabs follow the keymap; ⇧Enter, Up, Down and
    /// Escape are fixed keys.
    #[test]
    fn popup_keys_follow_the_keymap() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        let stroke = |text: &str| Keystroke::parse(text).expect("a keystroke");
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            let of = |key: &str, shift: bool, text: &str| {
                popup_key(key, shift, false, &keymap.actions_for(&stroke(text), None))
            };
            assert_eq!(of("tab", false, "TAB"), Some(PopupKey::NextTab));
            assert_eq!(of("tab", true, "shift TAB"), Some(PopupKey::PrevTab));
            assert_eq!(
                of("enter", false, "ENTER"),
                Some(PopupKey::Choose { right: false })
            );
            assert_eq!(
                of("enter", true, "shift ENTER"),
                Some(PopupKey::Choose { right: true })
            );
            assert_eq!(of("up", false, "UP"), Some(PopupKey::Up));
            assert_eq!(of("down", false, "DOWN"), Some(PopupKey::Down));
            assert_eq!(of("escape", false, "ESCAPE"), Some(PopupKey::Escape));
            assert_eq!(of("a", false, "A"), None);
        }
        // A rebound key acts with its modifier; a key that lost its action does nothing.
        assert_eq!(
            popup_key("j", false, true, &[NEXT_TAB]),
            Some(PopupKey::NextTab)
        );
        assert_eq!(popup_key("tab", false, false, &["EditorTab"]), None);
        assert_eq!(popup_key("enter", false, false, &["EditorEnter"]), None);
        // v8 behaviour 19 (AK-9): the right split is the keymap's `OpenInRightSplit`; ⇧Enter
        // without it does nothing, a key rebound to it does it.
        assert_eq!(
            popup_key("enter", true, false, &["EditorStartNewLine"]),
            None
        );
        assert_eq!(
            popup_key("o", false, true, &[OPEN_RIGHT]),
            Some(PopupKey::Choose { right: true })
        );
        assert_eq!(
            popup_action(OPEN_RIGHT),
            Some(PopupKey::Choose { right: true })
        );
        // The fixed keys are not taken with Control, Alt or the platform key.
        assert_eq!(popup_key("down", false, true, &[]), None);
        assert_eq!(popup_key("up", true, false, &[]), None);
        assert_eq!(
            popup_action(SELECT_ITEM),
            Some(PopupKey::Choose { right: false })
        );
        assert_eq!(popup_action("SearchEverywhere"), None);
    }

    /// SPEC-0067 v4 AK-4: two taps open the popup; ⌘⇧ released in any order and a capital letter
    /// do not count.
    #[test]
    fn double_shift_counts_only_taps_alone() {
        let t0 = std::time::Instant::now();
        let ms = |n: u64| t0 + std::time::Duration::from_millis(n);
        // Shift, Shift.
        let mut d = DoubleShift::default();
        assert!(!d.modifiers(true, false, ms(0)));
        assert!(!d.modifiers(false, true, ms(50)));
        assert!(!d.modifiers(true, false, ms(150)));
        assert!(d.modifiers(false, true, ms(200)));
        // Too slow.
        let mut d = DoubleShift::default();
        d.modifiers(true, false, ms(0));
        d.modifiers(false, true, ms(50));
        d.modifiers(true, false, ms(600));
        assert!(!d.modifiers(false, true, ms(650)));
        // ⌘⇧← with ⌘ released first, then ⇧P: no gesture.
        let mut d = DoubleShift::default();
        d.modifiers(true, false, ms(0)); // Shift
        d.modifiers(false, false, ms(10)); // ⌘ and Shift
        d.key(); // ←
        d.modifiers(true, false, ms(60)); // ⌘ released
        d.modifiers(false, true, ms(70)); // Shift released
        d.modifiers(true, false, ms(150)); // Shift for P
        d.key(); // P
        assert!(!d.modifiers(false, true, ms(200)));
        // Two capital letters.
        let mut d = DoubleShift::default();
        for at in [0, 200] {
            d.modifiers(true, false, ms(at));
            d.key();
            assert!(!d.modifiers(false, true, ms(at + 50)));
        }
    }

    fn action(id: &str, name: &str) -> ActionHit {
        ActionHit {
            id: id.into(),
            name: name.into(),
            group: None,
            shortcut: None,
            positions: Vec::new(),
        }
    }

    fn found() -> Everywhere {
        Everywhere {
            files: Some(vec![FileHit {
                path: "src/main.rs".into(),
                positions: Vec::new(),
            }]),
            symbols: Some(Vec::new()),
            actions: Some(vec![
                action("GotoFile", "Goto File"),
                action("GotoClass", "Goto Class"),
            ]),
            ..Everywhere::default()
        }
    }

    #[test]
    fn rows_skip_empty_groups_and_selection_wraps() {
        let none = std::collections::BTreeSet::new();
        let mut r = Results::from(found(), Filter::All, &none);
        assert_eq!(
            r.rows.len(),
            5,
            "Files heading, file, Actions heading, two actions"
        );
        assert_eq!(r.selected, Some(1));
        r.step(true);
        assert_eq!(r.selected, Some(3), "the Actions heading is skipped");
        r.step(true);
        r.step(true);
        assert_eq!(r.selected, Some(1), "wraps to the first hit");
        r.step(false);
        assert!(matches!(r.selected_row(), Some(Row::Action(a)) if a.id == "GotoClass"));
        assert_eq!(
            Results::from(Everywhere::default(), Filter::All, &none).selected,
            None
        );
        // v5: single tabs have no headings; the calculator comes first; hidden file types drop out.
        let r = Results::from(found(), Filter::Actions, &none);
        assert!(r.rows.iter().all(|row| !matches!(row, Row::Header(_))));
        let calc = Everywhere {
            calculator: Some("84".into()),
            ..found()
        };
        let r = Results::from(calc, Filter::All, &none);
        assert_eq!(r.rows[1], Row::Calculator("84".into()));
        let rs: std::collections::BTreeSet<String> = ["rs".to_owned()].into();
        let r = Results::from(found(), Filter::All, &rs);
        assert!(!r.rows.iter().any(|row| matches!(row, Row::File(_))));
    }

    #[test]
    fn filters() {
        assert_eq!(Filter::All.categories(), Categories::ALL);
        let files = Filter::Files.categories();
        assert!(files.files && !files.symbols && !files.actions && !files.text);
        assert!(Filter::Classes.categories().classes);
        assert!(Filter::Text.categories().text);
        assert_eq!(Filter::Actions.next(), Filter::Text);
        assert_eq!(Filter::Text.next(), Filter::Endpoints);
        assert_eq!(Filter::Endpoints.next(), Filter::All);
        assert_eq!(Filter::All.previous(), Filter::Endpoints);
        assert!(Filter::All.limit() < Filter::Files.limit());
        assert!(!Filter::Actions.has_scope() && Filter::Files.has_scope());
        assert!(type_shown("a/b.java", &std::collections::BTreeSet::new()));
    }

    /// SPEC-0067 v9 AK-10: the Endpoints tab is the last one, searches no category of the
    /// shared search, and lists its hits without a heading.
    #[test]
    fn endpoints_tab_rows() {
        use ferris_idea_languages::endpoints::{Endpoint, EndpointKind, search_endpoints};
        assert_eq!(Filter::ALL.len(), 7);
        assert_eq!(Filter::ALL[6], Filter::Endpoints);
        assert_eq!(Filter::Endpoints.label(), "Endpoints");
        assert_eq!(Filter::Endpoints.categories(), Categories::NONE);
        assert_eq!(Filter::Endpoints.limit(), 50);
        assert!(!Filter::Endpoints.has_scope());
        assert_eq!(Filter::Endpoints.previous(), Filter::Text);
        assert_eq!(popup_action(GOTO_ENDPOINT), None);

        let row = |method: &str, path: &str, handler: &str, file: &str, line: usize| Endpoint {
            framework: "Spring",
            kind: EndpointKind::Server,
            methods: vec![method.to_owned()],
            path: path.into(),
            handler: handler.into(),
            file: file.into(),
            line,
        };
        let model = [
            row(
                "GET",
                "/api/orders",
                "OrderApi.list",
                "src/OrderApi.java",
                4,
            ),
            row(
                "POST",
                "/api/orders",
                "OrderApi.create",
                "src/OrderApi.java",
                9,
            ),
            row("GET", "/api/users", "users", "app/users.py", 2),
        ];
        let none = std::collections::BTreeSet::new();
        let mut r = Results::of_endpoints(search_endpoints(&model, "POST orders", 50), &none);
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.selected, Some(0));
        let chosen = r.selected_row().expect("a row");
        assert_eq!(chosen.path(), Some("src/OrderApi.java"));
        assert_eq!(chosen.line(), Some(9));
        r.step(true);
        assert_eq!(r.selected, Some(0), "one row: the selection stays");

        let mut all = Results::of_endpoints(search_endpoints(&model, "", 50), &none);
        assert!(all.rows.iter().all(|row| matches!(row, Row::Endpoint(_))));
        assert_eq!(all.rows.len(), 3);
        all.step(false);
        assert_eq!(all.selected, Some(2), "wraps to the last row");
        // The file-type filter of the funnel (behaviour 14).
        let java: std::collections::BTreeSet<String> = ["java".to_owned()].into();
        let py = Results::of_endpoints(search_endpoints(&model, "", 50), &java);
        assert_eq!(py.rows.len(), 1);
        assert_eq!(py.selected_row().and_then(Row::path), Some("app/users.py"));
        // No endpoints: nothing selected.
        let empty = Results::of_endpoints(Vec::new(), &none);
        assert_eq!((empty.rows.len(), empty.selected), (0, None));
    }
}
