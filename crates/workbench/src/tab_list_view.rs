//! The list of a group's tabs behind the "Show hidden tabs" chevron and `TabList`, and the
//! `Switcher` popup (`SPEC-0059` v9 behaviours 15 and 19). Typing filters the rows (characters in
//! order, ignoring case), ↑/↓ move, Enter or a click chooses, Escape closes. In the Switcher, Tab
//! and ⇧Tab move while ⌃ is held and releasing ⌃ chooses the selected row.
//!
//! The Switcher shows its rows in two columns (`SPEC-0059` v9.4 behaviour 27): the tool windows
//! with their characters and Recent Locations on the left, the files on the right, the folder of
//! the selected file at the bottom. ← and → change the column, ⌃ with a row's character chooses
//! that row, ⇧Enter or the split control of the selected file opens it in a split.

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, ModifiersChangedEvent, Window, div,
    prelude::*, px,
};

use crate::metrics::{MENU_ITEM_MARGIN_X, MENU_ITEM_PADDING_X};
use crate::palette::Palette;
use crate::workbench_view::c;

/// One row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabListRow {
    /// The name shown.
    pub label: String,
    /// Icon key.
    pub icon: &'static str,
    /// The file has unsaved changes (` *`).
    pub modified: bool,
    /// A heading row: not selectable.
    pub heading: bool,
    /// Switcher: the row stands in the left column (tool windows, Recent Locations).
    pub left: bool,
    /// Switcher: the character that chooses the row while ⌃ is held, shown beside it.
    pub character: Option<char>,
    /// Switcher: the shortcut shown at the row's right edge (Recent Locations).
    pub shortcut: String,
    /// Switcher: the folder line while the row is selected (a file's folder).
    pub folder: String,
    /// Switcher: a file, which can open in a split.
    pub split: bool,
    /// Switcher: a thin line stands above the row.
    pub separated: bool,
}

/// What the popup asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabListEvent {
    /// Row `i` (of the rows given) was chosen.
    Chosen(usize),
    /// Row `i`, a file of the Switcher, is to open in a split on the right.
    ChosenInSplit(usize),
    /// Close without a choice.
    Close,
}

/// The popup.
pub struct TabListView {
    title: String,
    rows: Vec<TabListRow>,
    filter: String,
    /// Index into `rows` of the selected row.
    selected: Option<usize>,
    /// The Switcher: ⌃ release chooses.
    switcher: bool,
    focus: FocusHandle,
}

impl EventEmitter<TabListEvent> for TabListView {}

impl TabListView {
    /// A list headed `title`; `selected` is preselected when it is a row.
    pub fn new(
        title: String,
        rows: Vec<TabListRow>,
        selected: Option<usize>,
        switcher: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            title,
            rows,
            filter: String::new(),
            selected: None,
            switcher,
            focus: cx.focus_handle(),
        };
        view.selected = selected
            .filter(|&i| view.rows.get(i).is_some_and(|r| !r.heading))
            .or_else(|| view.visible().first().copied());
        view
    }

    /// Indices of the rows shown under the filter (headings only without a filter).
    pub fn visible(&self) -> Vec<usize> {
        let titles: Vec<String> = self.rows.iter().map(|r| r.label.clone()).collect();
        crate::tabs::tab_list(&titles, &self.filter)
            .into_iter()
            .filter(|&i| self.filter.is_empty() || !self.rows[i].heading)
            .collect()
    }

    /// The labels shown, for checks.
    pub fn labels(&self) -> Vec<String> {
        self.visible()
            .into_iter()
            .map(|i| self.rows[i].label.clone())
            .collect()
    }

    /// The label of row `i`.
    pub fn label(&self, i: usize) -> &str {
        self.rows.get(i).map_or("", |r| r.label.as_str())
    }

    /// The selected row.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Whether this is the Switcher.
    pub fn is_switcher(&self) -> bool {
        self.switcher
    }

    /// The rows shown in the Switcher's left (`true`) or right column.
    pub fn column(&self, left: bool) -> Vec<usize> {
        self.visible()
            .into_iter()
            .filter(|&i| !self.rows[i].heading && self.rows[i].left == left)
            .collect()
    }

    /// Row `i` as given.
    pub fn row(&self, i: usize) -> Option<&TabListRow> {
        self.rows.get(i)
    }

    /// The Switcher's folder line: the folder of the selected file, empty for other rows.
    pub fn folder_line(&self) -> &str {
        self.selected
            .and_then(|i| self.rows.get(i))
            .map_or("", |r| r.folder.as_str())
    }

    /// Selects the row at the same height in the other column, or that column's last row (← and
    /// → in the Switcher).
    pub fn step_column(&mut self, to_left: bool, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else {
            return;
        };
        if self.rows[selected].left == to_left {
            return;
        }
        let from = self.column(!to_left);
        let to = self.column(to_left);
        let at = from.iter().position(|&i| i == selected).unwrap_or(0);
        if let Some(&row) = to.get(at).or(to.last()) {
            self.selected = Some(row);
            cx.notify();
        }
    }

    /// Chooses the row whose character is `character` (⌃ with the character in the Switcher);
    /// false when no shown row has it.
    pub fn choose_character(&mut self, character: char, cx: &mut Context<Self>) -> bool {
        let wanted: Vec<char> = character.to_uppercase().collect();
        let row = self.visible().into_iter().find(|&i| {
            self.rows[i]
                .character
                .is_some_and(|c| c.to_uppercase().collect::<Vec<_>>() == wanted)
        });
        match row {
            Some(i) => {
                self.selected = Some(i);
                cx.emit(TabListEvent::Chosen(i));
                true
            }
            None => false,
        }
    }

    /// Opens the selected file in a split (⇧Enter, the split control); any other row is chosen
    /// as Enter does.
    pub fn accept_in_split(&mut self, cx: &mut Context<Self>) {
        match self.selected {
            Some(i) if self.rows[i].split => cx.emit(TabListEvent::ChosenInSplit(i)),
            _ => self.accept(cx),
        }
    }

    /// Focus handle of the popup.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The typed filter.
    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    /// Types `text` into the filter (also for self-tests and the platform's text input).
    pub fn type_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter.push_str(text);
        self.reselect();
        cx.notify();
    }

    fn reselect(&mut self) {
        let shown = self.visible();
        if !self.selected.is_some_and(|s| shown.contains(&s)) {
            self.selected = shown.into_iter().find(|&i| !self.rows[i].heading);
        }
    }

    /// Moves the selection down (or up), skipping headings, wrapping.
    pub fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let shown: Vec<usize> = self
            .visible()
            .into_iter()
            .filter(|&i| !self.rows[i].heading)
            .collect();
        if shown.is_empty() {
            return;
        }
        let n = shown.len();
        let at = self
            .selected
            .and_then(|s| shown.iter().position(|&i| i == s));
        let next = match (at, down) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(k), true) => (k + 1) % n,
            (Some(k), false) => (k + n - 1) % n,
        };
        self.selected = Some(shown[next]);
        cx.notify();
    }

    /// Chooses the selected row.
    pub fn accept(&mut self, cx: &mut Context<Self>) {
        match self.selected {
            Some(i) => cx.emit(TabListEvent::Chosen(i)),
            None => cx.emit(TabListEvent::Close),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The shortcut of `OpenInRightSplit` (⇧Enter) opens the Switcher's file in a split; ⌃,
        // held while the Switcher is used, does not count (`SPEC-0059` v9.4 behaviour 27e).
        if self.switcher {
            let m = &k.modifiers;
            let split = ferris_idea_ui::keymap::Keystroke::from_platform(
                &k.key, false, m.alt, m.shift, m.platform,
            )
            .is_some_and(|stroke| {
                crate::keymap_actions::is_shortcut_of(
                    &crate::keymaps::current(),
                    crate::keymap_actions::OPEN_IN_RIGHT_SPLIT,
                    &stroke,
                )
            });
            if split {
                self.accept_in_split(cx);
                cx.stop_propagation();
                return;
            }
            // Iterate Recent Files (⌘E in the bundled macOS keymap, Ctrl+E in the default one)
            // moves the selection down as ⌃⇥ does (`SPEC-0059` v9.10 behaviour 27h).
            let iterate = ferris_idea_ui::keymap::Keystroke::from_platform(
                &k.key, false, m.alt, m.shift, m.platform,
            )
            .is_some_and(|stroke| {
                crate::keymap_actions::is_shortcut_of(
                    &crate::keymaps::current(),
                    crate::keymap_actions::SWITCHER_ITERATE,
                    &stroke,
                )
            });
            if iterate {
                self.step(true, cx);
                cx.stop_propagation();
                return;
            }
        }
        // The first and the last row that is no heading (`SPEC-0275` v19 behaviour 47); ⌃, held
        // while the Switcher is used, does not count.
        let mut pressed = k.clone();
        if self.switcher {
            pressed.modifiers.control = false;
        }
        let shown = self.visible();
        if let Some(to) =
            crate::list_nav::edge_among(&pressed, shown.len(), crate::list_nav::Keys::ALL, |i| {
                !self.rows[shown[i]].heading
            })
        {
            self.selected = Some(shown[to]);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match k.key.as_str() {
            "down" => self.step(true, cx),
            "up" => self.step(false, cx),
            "tab" => self.step(!k.modifiers.shift, cx),
            "left" if self.switcher => self.step_column(true, cx),
            "right" if self.switcher => self.step_column(false, cx),
            "enter" => self.accept(cx),
            "escape" => cx.emit(TabListEvent::Close),
            "backspace" => {
                self.filter.pop();
                self.reselect();
                cx.notify();
            }
            // ⌃ with a row's character chooses the row (behaviour 27b).
            key if self.switcher
                && k.modifiers.control
                && !k.modifiers.platform
                && !k.modifiers.alt
                && key.chars().count() == 1 =>
            {
                let character = key.chars().next().unwrap_or_default();
                if !self.choose_character(character, cx) {
                    return;
                }
            }
            // Typed text arrives through the platform's text input, not as the key behind it
            // (`SPEC-0004` v8 behaviour 17b).
            _ => return,
        }
        cx.stop_propagation();
    }

    fn on_modifiers(&mut self, e: &ModifiersChangedEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Releasing ⌃ opens the Switcher's selection (behaviour 19).
        if self.switcher && !e.modifiers.control {
            self.accept(cx);
        }
    }
}

crate::committed_text::committed_text_input!(TabListView, type_filter);

impl Render for TabListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let filter = (!self.filter.is_empty()).then(|| {
            div()
                .px(px(MENU_ITEM_MARGIN_X + MENU_ITEM_PADDING_X))
                .pb_1()
                .text_color(c(p.ui_text))
                .child(format!("🔍 {}", self.filter))
        });
        if self.switcher {
            return self.render_switcher(filter, cx).into_any_element();
        }
        crate::metrics::menu_frame()
            .bg(c(p.island))
            .id("tab-list-popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            .w(px(320.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .child(
                div()
                    .px(px(MENU_ITEM_MARGIN_X + MENU_ITEM_PADDING_X))
                    .pt_1()
                    .pb_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(self.title.clone()),
            )
            .children(filter)
            .children(self.visible().into_iter().map(|i| {
                let row = &self.rows[i];
                let label = if row.modified {
                    format!("{} *", row.label)
                } else {
                    row.label.clone()
                };
                if row.heading {
                    return div()
                        .id(("tab-list-heading", i))
                        .px(px(MENU_ITEM_MARGIN_X + MENU_ITEM_PADDING_X))
                        .pt_1()
                        .text_xs()
                        .text_color(c(p.ui_dim))
                        .child(label);
                }
                crate::metrics::menu_item(("tab-list-row", i), Some(i) == selected)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(crate::icon_images::icon(row.icon))
                    .child(label)
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.selected = Some(i);
                        cx.emit(TabListEvent::Chosen(i));
                    }))
            }))
            .into_any_element()
    }
}

impl TabListView {
    /// One row of the Switcher: icon, name, and at the right edge the character or shortcut
    /// (left column) or, on the selected file, the split control.
    fn switcher_row(&self, i: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let row = &self.rows[i];
        let selected = Some(i) == self.selected;
        let label = if row.modified {
            format!("{} *", row.label)
        } else {
            row.label.clone()
        };
        let hint = match row.character {
            Some(ch) => ch.to_string(),
            None => row.shortcut.clone(),
        };
        let line = crate::metrics::menu_item(("switcher-row", i), selected)
            .overflow_hidden()
            .whitespace_nowrap()
            .gap_2()
            .child(crate::icon_images::icon(row.icon))
            .child(div().flex_1().min_w_0().overflow_hidden().child(label))
            .when(!hint.is_empty(), |d| {
                d.child(div().flex_none().text_color(c(p.ui_dim)).child(hint))
            })
            .when(selected && row.split, |d| {
                d.child(
                    div()
                        .id(("switcher-split", i))
                        .flex_none()
                        .px_1()
                        .text_color(c(p.ui_dim))
                        .hover(|s| s.text_color(c(p.ui_text)))
                        // The split control: two panes side by side.
                        .child("◫")
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.selected = Some(i);
                            cx.emit(TabListEvent::ChosenInSplit(i));
                        })),
                )
            })
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.selected = Some(i);
                cx.emit(TabListEvent::Chosen(i));
            }));
        if row.separated {
            div()
                .mt_1()
                .pt_1()
                .border_t_1()
                .border_color(c(p.popup_border))
                .child(line)
                .into_any_element()
        } else {
            line.into_any_element()
        }
    }

    /// The Switcher: two columns and the folder line (`SPEC-0059` v9.4 behaviour 27).
    fn render_switcher(
        &mut self,
        filter: Option<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let p = Palette::current();
        let left: Vec<gpui::AnyElement> = self
            .column(true)
            .into_iter()
            .map(|i| self.switcher_row(i, cx))
            .collect();
        let right: Vec<gpui::AnyElement> = self
            .column(false)
            .into_iter()
            .map(|i| self.switcher_row(i, cx))
            .collect();
        let folder = self.folder_line().to_owned();
        crate::metrics::menu_frame()
            .bg(c(p.island))
            .id("switcher-popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            .w(px(560.))
            .child(
                div()
                    .px(px(MENU_ITEM_MARGIN_X + MENU_ITEM_PADDING_X))
                    .pt_1()
                    .pb_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(self.title.clone()),
            )
            .children(filter)
            .child(
                div()
                    .flex()
                    .items_start()
                    .child(
                        div()
                            .id("switcher-left")
                            .w(px(230.))
                            .flex_none()
                            .max_h(px(560.))
                            .overflow_y_scroll()
                            .children(left),
                    )
                    .child(
                        div()
                            .id("switcher-right")
                            .flex_1()
                            .min_w_0()
                            .max_h(px(560.))
                            .overflow_y_scroll()
                            .children(right),
                    ),
            )
            .child(
                div()
                    .px(px(MENU_ITEM_MARGIN_X + MENU_ITEM_PADDING_X))
                    .pt_1()
                    .min_h(px(20.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(folder),
            )
            .into_any_element()
    }
}
