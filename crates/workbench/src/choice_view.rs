//! A popup that picks one of a few named items with ↑/↓ and Enter or a click; Escape closes it.
//! Used to choose the target connection of a schema comparison (`SPEC-0110` behaviour 3).
//! With check boxes it picks several (Override/Implement Methods, `SPEC-0122` v3 behaviour 6):
//! Space toggles the row, ⌘A checks all, Enter takes the checked rows; heading rows group them.
//! A popup that filters (`SPEC-0122` v5 behaviour 14) narrows its rows to those containing the
//! typed text; Backspace widens them again and Escape first clears the text.

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::metrics::{MENU_ITEM_HEIGHT, MENU_ITEM_MARGIN_X, MENU_ITEM_PADDING_X, MENU_PADDING_Y};
use crate::palette::Palette;
use crate::workbench_view::c;

/// Width of the popup.
pub const WIDTH: f32 = 360.;
/// Height of the popup without its rows: padding, border and the heading line (rounded up, so a
/// popup limited to a room never reaches out of it).
pub const CHROME: f32 = 2. * MENU_PADDING_Y + 2. + 26.;

/// The rows a typed `text` leaves (`SPEC-0122` v5 behaviour 14): those whose label contains it,
/// ignoring case, in their order; a heading stays while one of its rows does.
pub fn matching_rows(items: &[String], headings: &[bool], text: &str) -> Vec<usize> {
    let mut rows = Vec::new();
    let mut heading = None;
    for (i, label) in items.iter().enumerate() {
        if headings.get(i) == Some(&true) {
            heading = Some(i);
        } else if text.is_empty() || crate::speed_search::match_range(label, text).is_some() {
            rows.extend(heading.take());
            rows.push(i);
        }
    }
    rows
}

/// The row selected first among the shown `rows`: the one whose label is `text`, ignoring case
/// (`SPEC-0004` v6 behaviour 17), else the first whose label starts with it, else the first one;
/// headings never.
pub fn first_match(
    items: &[String],
    headings: &[bool],
    rows: &[usize],
    text: &str,
) -> Option<usize> {
    let text = text.to_lowercase();
    let rows: Vec<usize> = rows
        .iter()
        .copied()
        .filter(|&i| headings.get(i) != Some(&true))
        .collect();
    let label = |i: &usize| items[*i].to_lowercase();
    rows.iter()
        .find(|i| label(i) == text)
        .or_else(|| rows.iter().find(|i| label(i).starts_with(&text)))
        .or(rows.first())
        .copied()
}

/// What the popup asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceEvent {
    /// Item `i` was chosen.
    Chosen(usize),
    /// The checked items, in order (a popup with check boxes).
    ChosenMany(Vec<usize>),
    /// Close without a choice.
    Close,
}

/// The popup.
pub struct ChoiceView {
    title: String,
    items: Vec<String>,
    /// Icon keys per item, empty for a popup without icons (`SPEC-0059` v7 behaviour 14).
    icons: Vec<&'static str>,
    /// Rows shown at once (the rest scroll with the selection), `None` for all.
    limit: Option<usize>,
    /// First row shown.
    top: usize,
    selected: usize,
    /// Check boxes per item; empty for a popup that picks one.
    checked: Vec<bool>,
    /// Rows that only head a group: not selectable, no check box.
    headings: Vec<bool>,
    /// The typed text that narrows the rows; `None` for a popup that does not filter.
    filter: Option<String>,
    /// Mouse wheel movement that did not make a whole row yet.
    wheel_rest: f32,
    /// Dimmed text after each row's label (a file's folder after its name); empty for a popup
    /// without (`SPEC-0115` v6 behaviour 17).
    details: Vec<String>,
    /// The popup's width; [`WIDTH`] unless it was sized to its content.
    width: f32,
    /// Enter with no row checked takes none instead of the selected one (a list of options,
    /// `SPEC-0075` v6 behaviour 8).
    none_allowed: bool,
    focus: FocusHandle,
}

/// Width of a character of the popup's text, on average and rather too wide than too narrow:
/// the rows are not measured, and a row cut off hides the line number the user chooses by.
const CHAR_WIDTH: f32 = 7.5;

/// The width a popup needs to show `title` and its rows whole (`SPEC-0115` v6 behaviour 17):
/// each row is a label and a dimmed detail after it. Never narrower than [`WIDTH`], never wider
/// than `max`.
pub fn content_width(title: &str, rows: &[(&str, &str)], max: f32) -> f32 {
    let chrome = 2. * (MENU_ITEM_MARGIN_X + MENU_ITEM_PADDING_X) + 16.;
    let row = |(label, detail): &(&str, &str)| {
        let gap = if detail.is_empty() { 0 } else { 2 };
        label.chars().count() + gap + detail.chars().count()
    };
    let chars = rows
        .iter()
        .map(row)
        .chain([title.chars().count()])
        .max()
        .unwrap_or(0);
    (chars as f32 * CHAR_WIDTH + chrome).min(max).max(WIDTH)
}

impl EventEmitter<ChoiceEvent> for ChoiceView {}

impl ChoiceView {
    /// A popup headed `title` listing `items` (not empty), the first one selected.
    pub fn new(title: String, items: Vec<String>, cx: &mut Context<Self>) -> Self {
        Self {
            title,
            items,
            icons: Vec::new(),
            limit: None,
            top: 0,
            selected: 0,
            checked: Vec::new(),
            headings: Vec::new(),
            filter: None,
            wheel_rest: 0.,
            details: Vec::new(),
            width: WIDTH,
            none_allowed: false,
            focus: cx.focus_handle(),
        }
    }

    /// Gives each row a dimmed detail after its label and sizes the popup to its content, at
    /// most `max` wide (`SPEC-0115` v6 behaviour 17).
    pub fn set_details(&mut self, details: Vec<String>, max: f32) {
        let rows: Vec<(&str, &str)> = self
            .items
            .iter()
            .zip(details.iter())
            .map(|(l, d)| (l.as_str(), d.as_str()))
            .collect();
        self.width = content_width(&self.title, &rows, max);
        self.details = details;
    }

    /// The dimmed details of the rows; empty for a popup without.
    pub fn details(&self) -> &[String] {
        &self.details
    }

    /// The popup's width.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// A popup with check boxes: `checked` per item, `headings` marks group rows (`SPEC-0122` v3
    /// behaviour 6). The first checked row (else the first item) is selected.
    pub fn with_checks(mut self, checked: Vec<bool>, headings: Vec<bool>) -> Self {
        self.checked = checked;
        self.headings = headings;
        self.selected = (0..self.items.len())
            .find(|&i| self.checked.get(i) == Some(&true) && !self.heading(i))
            .or_else(|| (0..self.items.len()).find(|&i| !self.heading(i)))
            .unwrap_or(0);
        self
    }

    /// Enter with no row checked takes no row: the rows are options, not items to pick
    /// (`SPEC-0075` v6 behaviour 8).
    pub fn with_none_allowed(mut self) -> Self {
        self.none_allowed = true;
        self
    }

    /// Typed characters narrow the rows (`SPEC-0122` v5 behaviour 14).
    pub fn with_filter(mut self) -> Self {
        self.filter = Some(String::new());
        self
    }

    /// Makes a popup that is already built filter its rows.
    pub fn enable_filter(&mut self) {
        self.filter.get_or_insert_with(String::new);
    }

    /// The typed text of a popup that filters.
    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    /// The rows shown now, as indexes of the items: all of them, or those the typed text leaves.
    pub fn shown(&self) -> Vec<usize> {
        match self.filter.as_deref() {
            Some(text) if !text.is_empty() => matching_rows(&self.items, &self.headings, text),
            _ => (0..self.items.len()).collect(),
        }
    }

    /// The labels of the rows shown now.
    pub fn shown_items(&self) -> Vec<String> {
        self.shown()
            .into_iter()
            .map(|i| self.items[i].clone())
            .collect()
    }

    /// Sets the typed text and selects the first row starting with it, else the first one shown.
    pub fn set_filter(&mut self, text: &str) {
        if self.filter.is_none() {
            return;
        }
        self.filter = Some(text.to_owned());
        let rows = self.shown();
        if text.is_empty() {
            // Back to all rows: the selection stays where it is.
            if !rows.contains(&self.selected) || self.heading(self.selected) {
                self.selected = first_match(&self.items, &self.headings, &rows, "").unwrap_or(0);
            }
        } else if let Some(i) = first_match(&self.items, &self.headings, &rows, text) {
            self.selected = i;
        }
        self.top = 0;
        self.follow();
    }

    /// Takes text the platform's text input committed (`SPEC-0004` v6 behaviour 17): it extends
    /// the typed text of a popup that filters; a leading space is not typed.
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        let Some(text) = self.filter.clone() else {
            return;
        };
        if !crate::speed_search::types_into(&text, committed) {
            return;
        }
        self.set_filter(&format!("{text}{committed}"));
        cx.notify();
    }

    /// Checked rows the typed text hides.
    pub fn hidden_checked(&self) -> usize {
        let shown = self.shown();
        self.checked()
            .into_iter()
            .filter(|i| !shown.contains(i))
            .count()
    }

    /// The popup's height with every shown row.
    pub fn full_height(&self) -> f32 {
        CHROME + self.shown().len().max(1) as f32 * MENU_ITEM_HEIGHT
    }

    /// The rows shown at once at most, if limited.
    pub fn limit(&self) -> Option<usize> {
        self.limit
    }

    /// Whether a row is shown that Enter can take.
    fn selectable(&self) -> bool {
        self.shown().contains(&self.selected) && !self.heading(self.selected)
    }

    /// Keeps the selected row among the rows shown at once.
    fn follow(&mut self) {
        let Some(n) = self.limit else {
            self.top = 0;
            return;
        };
        let shown = self.shown();
        let at = shown.iter().position(|&i| i == self.selected).unwrap_or(0);
        // The heading above the first row of a group stays in view with it.
        let first = if at > 0 && self.heading(shown[at - 1]) {
            at - 1
        } else {
            at
        };
        if first < self.top {
            self.top = first;
        } else if at >= self.top + n {
            self.top = at + 1 - n;
        }
        self.top = self.top.min(shown.len().saturating_sub(n));
    }

    /// Scrolls a limited list by `rows` (the mouse wheel); the selection stays.
    pub fn scroll_by(&mut self, rows: i32) {
        let Some(n) = self.limit else {
            return;
        };
        let last = self.shown().len().saturating_sub(n) as i64;
        self.top = (self.top as i64 + i64::from(rows)).clamp(0, last) as usize;
    }

    fn heading(&self, i: usize) -> bool {
        self.headings.get(i) == Some(&true)
    }

    /// Whether the popup picks several items.
    pub fn multiple(&self) -> bool {
        !self.checked.is_empty()
    }

    /// The checked items.
    pub fn checked(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.checked.get(i) == Some(&true) && !self.heading(i))
            .collect()
    }

    /// Toggles item `i` (Space; also for self-tests).
    pub fn toggle(&mut self, i: usize) {
        if !self.heading(i)
            && let Some(c) = self.checked.get_mut(i)
        {
            *c = !*c;
        }
    }

    /// Checks every item (⌘A); with a typed text the shown ones.
    pub fn check_all(&mut self) {
        for i in self.shown() {
            if !self.heading(i) && i < self.checked.len() {
                self.checked[i] = true;
            }
        }
    }

    /// What Enter does: the checked items (the selected one when none is checked), or the
    /// selected item.
    pub fn accept(&self) -> ChoiceEvent {
        if self.multiple() {
            let many = self.checked();
            if many.is_empty() && !self.none_allowed {
                ChoiceEvent::ChosenMany(vec![self.selected])
            } else {
                ChoiceEvent::ChosenMany(many)
            }
        } else {
            ChoiceEvent::Chosen(self.selected)
        }
    }

    fn step(&mut self, down: bool) {
        let rows: Vec<usize> = self
            .shown()
            .into_iter()
            .filter(|&i| !self.heading(i))
            .collect();
        let n = rows.len();
        if n == 0 {
            return;
        }
        self.selected = match rows.iter().position(|&i| i == self.selected) {
            Some(at) if down => rows[(at + 1) % n],
            Some(at) => rows[(at + n - 1) % n],
            None => rows[0],
        };
    }

    /// Selects item `i` first (`SPEC-0192` behaviour 8: Cancel preselected).
    pub fn with_selected(mut self, i: usize) -> Self {
        self.selected = i.min(self.items.len().saturating_sub(1));
        self
    }

    /// Selects item `i` of a popup that is already shown (a risky choice's Cancel preselected).
    pub fn select(&mut self, i: usize) {
        self.selected = i.min(self.items.len().saturating_sub(1));
    }

    /// Shows `icons` (one key per item) before the items.
    pub fn set_icons(&mut self, icons: Vec<&'static str>) {
        self.icons = icons;
    }

    /// Shows at most `rows` rows at once; the list scrolls with the selection (the paste history,
    /// `SPEC-0057` v3 behaviour 20).
    pub fn set_limit(&mut self, rows: usize) {
        self.limit = Some(rows.max(1));
        self.follow();
    }

    /// The first row shown.
    pub fn top(&self) -> usize {
        self.top
    }

    /// The icon keys, one per item, or empty.
    pub fn icons(&self) -> &[&'static str] {
        &self.icons
    }

    /// Focus handle of the popup.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The heading.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The items as shown.
    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// Index of the selected item.
    pub fn selected(&self) -> usize {
        self.selected
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last row that is no heading (`SPEC-0275` v19 behaviour 47).
        let shown = self.shown();
        if let Some(to) =
            crate::list_nav::edge_among(k, shown.len(), crate::list_nav::Keys::ALL, |i| {
                !self.heading(shown[i])
            })
        {
            self.selected = shown[to];
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let command = k.modifiers.platform || k.modifiers.control;
        let typed = self.filter.clone().filter(|f| !f.is_empty());
        match k.key.as_str() {
            "down" => self.step(true),
            "up" => self.step(false),
            // With no row left Enter does nothing (`SPEC-0122` v5 behaviour 14).
            "enter" if typed.is_some() && !self.selectable() => {}
            "enter" => cx.emit(self.accept()),
            // Escape first clears the typed text.
            "escape" if typed.is_some() => self.set_filter(""),
            "escape" => cx.emit(ChoiceEvent::Close),
            "space" if self.multiple() => {
                if self.selectable() {
                    self.toggle(self.selected);
                }
            }
            "a" if self.multiple() && command => self.check_all(),
            "backspace" if self.filter.is_some() => {
                let mut text = typed.unwrap_or_default();
                text.pop();
                self.set_filter(&text);
            }
            // Typed text arrives through the platform's text input, not as the key behind it
            // (`SPEC-0004` v6 behaviour 17): the key is left to it.
            _ => return,
        }
        self.follow();
        cx.stop_propagation();
        cx.notify();
    }
}

crate::committed_text::committed_text_input!(ChoiceView, take_committed);

impl Render for ChoiceView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let shown = self.shown();
        let typed = self.filter.clone().filter(|f| !f.is_empty());
        let hidden = self.hidden_checked();
        let nothing = typed.is_some() && shown.is_empty();
        // Menu metrics of Jewel (SPEC-0154 behaviour 3).
        crate::metrics::menu_frame()
            .bg(c(p.island))
            .id("choice-popup")
            // Also where it is drawn without the popup registry (the SQL editor's popup): a
            // click or the wheel on it never reaches the editor under it (SPEC-0275 v14 28b).
            .occlude()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // A popup that filters takes committed text (`SPEC-0004` v6 behaviour 17).
            .when(self.filter.is_some(), |d| {
                d.relative()
                    .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            })
            .w(px(self.width))
            .on_scroll_wheel(cx.listener(|this, e: &gpui::ScrollWheelEvent, _, cx| {
                if this.limit.is_none() {
                    return;
                }
                this.wheel_rest -= f32::from(e.delta.pixel_delta(px(MENU_ITEM_HEIGHT)).y);
                let rows = (this.wheel_rest / MENU_ITEM_HEIGHT).trunc();
                this.wheel_rest -= rows * MENU_ITEM_HEIGHT;
                this.scroll_by(rows as i32);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                div()
                    .px(px(MENU_ITEM_MARGIN_X + MENU_ITEM_PADDING_X))
                    .pt_1()
                    .pb_1()
                    .flex()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(self.title.clone())
                    // The typed text, and how many checked rows it hides.
                    .when_some(typed, |d, text| {
                        d.child(div().text_color(c(p.ui_text)).child(text))
                    })
                    .when(hidden > 0, |d| {
                        d.child(format!("· {hidden} checked not shown"))
                    }),
            )
            .when(nothing, |d| {
                d.child(
                    crate::metrics::menu_item("choice-none", false)
                        .text_color(c(p.ui_dim))
                        .child("No matching row"),
                )
            })
            .children(
                shown
                    .into_iter()
                    .map(|i| (i, &self.items[i]))
                    .skip(self.top)
                    .take(self.limit.unwrap_or(usize::MAX))
                    .map(|(i, text)| {
                        let icon = self.icons.get(i).copied();
                        let detail = self.details.get(i).filter(|d| !d.is_empty()).cloned();
                        let heading = self.heading(i);
                        let check = (!heading)
                            .then(|| self.checked.get(i).copied())
                            .flatten()
                            .map(|on| if on { "☑" } else { "☐" });
                        crate::metrics::menu_item(("choice-row", i), i == selected && !heading)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(heading, |d| d.text_color(c(p.ui_dim)))
                            .when(!heading && self.multiple(), |d| d.pl(px(24.)))
                            .when_some(check, |d, mark| d.flex().items_center().gap_2().child(mark))
                            .when_some(icon, |d, key| {
                                d.flex()
                                    .items_center()
                                    .gap_2()
                                    .child(crate::icon_images::icon(key))
                            })
                            .child(text.clone())
                            // The folder after the file, dimmed (SPEC-0115 v6 behaviour 17).
                            .when_some(detail, |d, detail| {
                                d.flex()
                                    .items_center()
                                    .child(div().pl_3().text_color(c(p.ui_dim)).child(detail))
                            })
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                if this.heading(i) {
                                    return;
                                }
                                this.selected = i;
                                if this.multiple() {
                                    // A click toggles; a double click takes the checked rows.
                                    if e.click_count() > 1 {
                                        cx.emit(this.accept());
                                    } else {
                                        this.toggle(i);
                                        cx.notify();
                                    }
                                } else {
                                    cx.emit(ChoiceEvent::Chosen(i));
                                }
                            }))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    // SPEC-0122 v5 AK-16.
    #[test]
    fn a_text_keeps_matching_rows_and_their_headings() {
        let items = labels(&[
            "List",
            "add(E)",
            "addAll(Collection)",
            "Object",
            "toString()",
        ]);
        let headings = [true, false, false, true, false];
        // Upper and lower case alike, anywhere in the label, in the items' order.
        assert_eq!(matching_rows(&items, &headings, "ALL"), [0, 2]);
        assert_eq!(matching_rows(&items, &headings, "string"), [3, 4]);
        // A heading's own text does not match, and a heading without rows goes.
        assert_eq!(matching_rows(&items, &headings, "obj"), Vec::<usize>::new());
        assert_eq!(matching_rows(&items, &headings, ""), [0, 1, 2, 3, 4]);
        // Without headings every item is a row.
        let plain = labels(&["Constructor", "Getter", "Setter", "Getter and Setter"]);
        assert_eq!(matching_rows(&plain, &[], "sett"), [2, 3]);
        assert_eq!(matching_rows(&plain, &[], "x"), Vec::<usize>::new());
    }

    /// SPEC-0115 AK-20 (v6): the popup is as wide as its longest row with its detail, never
    /// narrower than the usual popup and never wider than the room it is given.
    #[test]
    fn the_popup_is_sized_to_its_rows() {
        // Short rows keep the usual width.
        assert_eq!(content_width("Usages", &[("A.java:1", "src")], 900.), WIDTH);
        let label = "ShopProperties.java:5  shop";
        let detail = "src/main/java/com/example/shop/config";
        let wide = content_width(
            "Usages of shop.name",
            &[(label, detail), ("B.java:2", "")],
            900.,
        );
        let chars = (label.chars().count() + 2 + detail.chars().count()) as f32;
        assert!(wide > chars * 7., "{wide} holds {chars} chars");
        assert!(wide < 900.);
        // The room limits it; a long title counts like a row.
        assert_eq!(content_width("Usages", &[(label, detail)], 400.), 400.);
        let title = "x".repeat(100);
        assert!(content_width(&title, &[], 2000.) > 100. * 7.);
    }

    /// SPEC-0004 v6 AK-11: the row whose label is the typed text, ignoring case, wins over rows
    /// before it that only start with it or contain it.
    #[test]
    fn the_exact_match_is_selected() {
        let items = labels(&["C++", "CSS", "Objective-C", "C", "SCSS"]);
        let rows = matching_rows(&items, &[], "c");
        assert_eq!(rows, [0, 1, 2, 3, 4]);
        assert_eq!(first_match(&items, &[], &rows, "c"), Some(3));
        assert_eq!(first_match(&items, &[], &rows, "C"), Some(3));
        let rows = matching_rows(&items, &[], "css");
        assert_eq!(first_match(&items, &[], &rows, "css"), Some(1));
        let rows = matching_rows(&items, &[], "c+");
        assert_eq!(first_match(&items, &[], &rows, "c+"), Some(0));
    }

    #[test]
    fn the_row_starting_with_the_text_is_selected() {
        let items = labels(&["Getter and Setter", "Setter", "toString()"]);
        let rows = matching_rows(&items, &[], "set");
        assert_eq!(rows, [0, 1]);
        // "Setter" starts with the text, so it wins over the row before it.
        assert_eq!(first_match(&items, &[], &rows, "set"), Some(1));
        // No row starts with it: the first shown.
        let rows = matching_rows(&items, &[], "ter");
        assert_eq!(first_match(&items, &[], &rows, "ter"), Some(0));
        // Headings are never selected, and nothing shown selects nothing.
        let grouped = labels(&["Setters", "setName(String)"]);
        let rows = matching_rows(&grouped, &[true, false], "set");
        assert_eq!(first_match(&grouped, &[true, false], &rows, "set"), Some(1));
        assert_eq!(first_match(&items, &[], &[], "q"), None);
    }
}
