//! The options of the settings pages (`SPEC-0105` v11 behaviour 17): one row with a check box (or
//! the mark of a choice it switches) and the option's label. A click anywhere on the row, on the
//! box as on the label, switches the option, selects its row and gives the keys to the page, so
//! that Space and Enter then switch the selected option and never type into the search.

use gpui::{Context, Window, div, prelude::*, px};

use super::{SettingsView, hit_label};
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::settings_model::Page;
use crate::workbench_view::c;
use ferris_idea_ui::theme::ThemeId;

/// What an option row shows in front of its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    /// A check box, ticked or not.
    Check(bool),
    /// A choice of two that Space and a click move to the other value.
    Choice,
}

impl Mark {
    /// The glyph of the box.
    pub(super) fn glyph(self) -> &'static str {
        match self {
            Mark::Check(true) => "☑",
            Mark::Check(false) => "☐",
            Mark::Choice => "⇄",
        }
    }
}

impl SettingsView {
    /// Row `i` of the page shown as an option: the box, the label (framed while the search
    /// matches it) and, after them, whatever the caller adds. `test_id` names the row in the
    /// semantic tree; its label is `<test_id>-label`. An option that weakens a protection is
    /// [`user_only`]: UI automation by an agent or script cannot switch it.
    pub(super) fn option_row(
        &self,
        id: (&'static str, usize),
        test_id: String,
        mark: Mark,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current();
        let i = id.1;
        let on = i == self.selected;
        let effect = if user_only(self.page, i) {
            Effect::Trust
        } else {
            Effect::Write
        };
        let semantics = Semantics::new(Role::CheckBox)
            .name(label)
            .test_id(test_id.clone())
            .selected(on)
            .action(AxAction::Click)
            .effects(&[effect]);
        let semantics = match mark {
            Mark::Check(b) => semantics.checked(b),
            Mark::Choice => semantics,
        };
        div()
            .id(id)
            .relative()
            // The rows of the pages are one line high, as before (System Settings and
            // Notifications grow with their text).
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(4.))
            .cursor_pointer()
            .when(on, |d| d.bg(c(p.tree_selection)))
            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
            .child(div().flex_none().child(mark.glyph()))
            .child(
                div()
                    .relative()
                    .child(hit_label(label, &self.query, p))
                    .semantics(
                        Semantics::new(Role::Label)
                            .name(label)
                            .test_id(format!("{test_id}-label")),
                    ),
            )
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.click_option(i, window, cx);
            }))
            .semantics(semantics)
    }

    /// A click on option `i`, on its box or its label: the row is selected, the page takes the
    /// keys and the option switches (behaviour 17).
    pub(super) fn click_option(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.give_page_keys(i, window);
        self.switch_option(cx);
        cx.notify();
    }

    /// Row `i` is selected and the page has the keys: the search keeps its text but no longer
    /// takes Space, Enter or ⌫ until it is clicked, typed into or the page changes (behaviour 17,
    /// as the keymap list in behaviour 16).
    pub(super) fn give_page_keys(&mut self, i: usize, window: &mut Window) {
        self.selected = i;
        self.row_chosen = true;
        self.list_keys = true;
        self.list_has_keys = false;
        self.query_marked = false;
        window.focus(&self.focus);
    }

    /// Space on the selected row: switches it when it is an option or a choice; `false` when the
    /// row has nothing to switch.
    pub(super) fn switch_option(&mut self, cx: &mut Context<Self>) -> bool {
        let row = self.selected;
        let themes = ThemeId::ALL.len();
        match self.page {
            Page::Appearance if row == themes => self.cycle_scheme(1, cx),
            Page::Appearance if row == themes + 1 => self.toggle_sync(cx),
            Page::Appearance if row == themes + 2 => self.toggle_compact(cx),
            Page::Appearance if row == themes + 3 => self.toggle_startup_animation(cx),
            Page::SystemSettings | Page::Notifications => self.toggle_general(row, cx),
            Page::Editor | Page::EditorTabs => self.adjust(0.0, cx),
            Page::Font if row == 3 => self.adjust(0.0, cx),
            Page::CodeStyle if row != 0 => self.adjust(0.0, cx),
            Page::VersionControl if row != 2 => self.adjust(0.0, cx),
            Page::Ai if row == 0 => self.toggle_ai(cx),
            Page::Ai if row == 2 => self.toggle_inline_completion(cx),
            Page::Ai if row == 4 => self.toggle_mcp_http(cx),
            Page::Ai if row == 6 => self.toggle_ui_driver_external(cx),
            Page::Advanced => self.step_advanced(0, cx),
            _ => return false,
        }
        true
    }
}

/// Whether option `row` of `page` weakens a protection or lets others in: a classical SSH key
/// exchange or an own ProxyCommand (ADR-0025), MCP over HTTP, agents operating the window
/// (`SPEC-0319`). These are the user's own: their row declares the effect reserved for the user,
/// so UI automation cannot switch them (`SPEC-0105` v11 behaviour 17).
pub(super) fn user_only(page: Page, row: usize) -> bool {
    matches!(
        (page, row),
        (Page::VersionControl, 3 | 4) | (Page::Ai, 4 | 6)
    )
}

/// Whether a key goes on with the search: printable text while the search holds text and has the
/// keys (`SPEC-0105` D9, v11 behaviour 17). Space typed after a click on an option switches the
/// option instead.
pub(super) fn types_into_search(query: &str, ch: &str, command: bool, page_keys: bool) -> bool {
    !page_keys && super::extends_query(query, ch, command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_after_a_click_on_an_option_is_no_search_text() {
        // The search has the keys: a space goes on with a typed search.
        assert!(types_into_search("save", " ", false, false));
        // An empty search takes no space through this path: the page's Space switches.
        assert!(!types_into_search("", " ", false, false));
        // After a click on an option the page has the keys: Space is the option's.
        assert!(!types_into_search("trailing", " ", false, true));
        assert!(!types_into_search("trailing", "x", false, true));
        // Command keys never type.
        assert!(!types_into_search("save", "a", true, false));
    }

    #[test]
    fn options_that_weaken_a_protection_are_the_users() {
        assert!(user_only(Page::VersionControl, 3));
        assert!(user_only(Page::VersionControl, 4));
        assert!(user_only(Page::Ai, 4));
        assert!(user_only(Page::Ai, 6));
        assert!(!user_only(Page::VersionControl, 1));
        assert!(!user_only(Page::Editor, 0));
        assert!(!user_only(Page::Ai, 0));
    }

    #[test]
    fn marks_show_their_state() {
        assert_eq!(Mark::Check(true).glyph(), "☑");
        assert_eq!(Mark::Check(false).glyph(), "☐");
        assert_eq!(Mark::Choice.glyph(), "⇄");
    }
}
