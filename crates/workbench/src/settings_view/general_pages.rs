//! The pages of Settings › Appearance & Behavior › System Settings and Notifications, the editor
//! scheme row of Appearance and the Advanced Settings page (`SPEC-0337`).

use ferris_idea_core::advanced_settings::{AdvancedSetting, Kind, Value};
use gpui::{Context, Window, div, prelude::*, px};

use super::SettingsView;
use super::options::Mark;
use crate::palette::{EDITOR_SCHEMES, Palette};
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::settings_model::Page;
use crate::workbench_view::c;

/// What the scheme row shows for an empty choice.
pub(super) const SAME_AS_THEME: &str = "Same as theme";

/// A row of a settings page, selected or not.
fn row(id: (&'static str, usize), on: bool, p: &Palette) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .min_h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(4.))
        .when(on, |d| d.bg(c(p.tree_selection)))
        .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
}

fn check(on: bool) -> &'static str {
    if on { "☑" } else { "☐" }
}

impl SettingsView {
    /// The rows of System Settings: label, value and what the switch does (`SPEC-0337`
    /// behaviour 2).
    fn system_rows(&self) -> [(&'static str, bool, &'static str); 3] {
        let s = &self.ide.system;
        [
            (
                "Confirm before quitting",
                s.confirm_quit,
                // ⌘Q is a fixed key of the macOS application menu; other systems have no quit
                // key in the bundled keymap (`SPEC-0028` v9 behaviour 14).
                if cfg!(target_os = "macos") {
                    "⌘Q and the menu ask first also with nothing unsaved; agents and scripts are not asked"
                } else {
                    "the menu asks first also with nothing unsaved; agents and scripts are not asked"
                },
            ),
            (
                "Reopen the last project at start-up",
                s.reopen_last_project,
                "off: Ferris IDEA asks for a folder when it starts without one",
            ),
            (
                "Save files when the window loses the focus",
                s.save_on_frame_deactivation,
                "saves the window's modified files as Save All does; off by default",
            ),
        ]
    }

    /// Enter, Space or a click on row `row` of System Settings or Notifications.
    pub(super) fn toggle_general(&mut self, row: usize, cx: &mut Context<Self>) {
        let page = self.page;
        self.change_ide(
            |s| match (page, row) {
                (Page::SystemSettings, 0) => s.system.confirm_quit = !s.system.confirm_quit,
                (Page::SystemSettings, 1) => {
                    s.system.reopen_last_project = !s.system.reopen_last_project
                }
                (Page::SystemSettings, _) => {
                    s.system.save_on_frame_deactivation = !s.system.save_on_frame_deactivation
                }
                (Page::Notifications, _) => s.notifications.log_info = !s.notifications.log_info,
                _ => {}
            },
            cx,
        );
    }

    /// The scheme row of Appearance moves `by` through "Same as theme" and the bundled schemes
    /// (`SPEC-0337` behaviour 4).
    pub(super) fn cycle_scheme(&mut self, by: isize, cx: &mut Context<Self>) {
        self.change_ide(
            |s| {
                let names: Vec<&str> = std::iter::once("")
                    .chain(EDITOR_SCHEMES.iter().map(|(n, _)| *n))
                    .collect();
                let at = names
                    .iter()
                    .position(|n| *n == s.appearance.editor_scheme)
                    .unwrap_or(0) as isize;
                let next = (at + by).rem_euclid(names.len() as isize) as usize;
                s.appearance.editor_scheme = names[next].to_owned();
            },
            cx,
        );
    }

    /// The scheme the scheme row shows.
    pub fn editor_scheme_shown(&self) -> &str {
        match crate::palette::scheme_of(&self.ide.appearance.editor_scheme) {
            Some(_) => self.ide.appearance.editor_scheme.trim(),
            None => SAME_AS_THEME,
        }
    }

    /// The Advanced Settings rows shown for the search.
    pub fn advanced_rows(&self) -> Vec<&'static AdvancedSetting> {
        crate::settings_model::advanced_page_rows(
            &self.query,
            &self.advanced_query,
            self.advanced_modified_only,
            &self.ide,
        )
    }

    /// The bar above a page: its title and, where the page has settings of its own, "Revert
    /// changes" while it differs from what the dialog opened with and "Restore defaults" while it
    /// differs from the defaults (`SPEC-0105` v7 behaviour 8).
    pub(super) fn page_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .text_xs()
                .cursor_pointer()
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(label)
                .semantics(
                    Semantics::new(Role::Button)
                        .name(label)
                        .test_id(id)
                        .action(AxAction::Click)
                        .effects(&[Effect::Write]),
                )
        };
        div()
            .h(px(32.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(c(p.separator))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.page.title()),
            )
            // Where the page's settings belong (`SPEC-0105` v9 behaviour 13).
            .child({
                let scope = self.page.scope().1;
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(c(p.ui_muted))
                    .child(scope)
                    .semantics(
                        Semantics::new(Role::Label)
                            .name(scope)
                            .test_id("settings-page-scope"),
                    )
            })
            .when(self.page_changed(), |d| {
                d.child(button("settings-revert", "Revert changes").on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.revert_page(cx)),
                ))
            })
            .when(self.page_not_default(), |d| {
                d.child(
                    button("settings-restore-defaults", "Restore defaults").on_click(cx.listener(
                        |this, _: &gpui::ClickEvent, _, cx| this.restore_page_defaults(cx),
                    )),
                )
            })
            .into_any_element()
    }

    fn advanced_selected(&self) -> Option<&'static AdvancedSetting> {
        self.advanced_rows().get(self.selected).copied()
    }

    /// Enter, Space, a click (`by` 0) or Left/Right (`by` ±1) on the selected Advanced Settings
    /// row: a switch turns over, a number moves a step; a text row is edited in the field.
    pub(super) fn step_advanced(&mut self, by: i64, cx: &mut Context<Self>) {
        let Some(a) = self.advanced_selected() else {
            return;
        };
        if matches!(a.kind, Kind::Number { .. }) && by == 0 {
            return;
        }
        self.change_ide(|s| a.step(s, by), cx);
    }

    /// Delete on Advanced Settings: the selected row back to its default (behaviour 5).
    pub(super) fn reset_advanced(&mut self, cx: &mut Context<Self>) {
        let Some(a) = self.advanced_selected() else {
            return;
        };
        self.change_ide(|s| a.reset(s), cx);
        if a.kind == Kind::Text {
            let shown = a.value(&self.ide);
            if let Value::Text(t) = shown {
                self.advanced_field.update(cx, |f, cx| f.set_text(&t, cx));
            }
        }
        self.note = Some(format!("{} reset to its default", a.label));
        cx.notify();
    }

    /// Enter on a text row: its value goes into the field below the list, which takes the focus.
    pub(super) fn edit_advanced(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(a) = self.advanced_selected() else {
            return false;
        };
        let Value::Text(text) = a.value(&self.ide) else {
            return false;
        };
        self.advanced_editing = Some(a.key);
        self.advanced_field
            .update(cx, |f, cx| f.set_text(&text, cx));
        let focus = self.advanced_field.read(cx).focus();
        window.focus(&focus);
        cx.notify();
        true
    }

    /// Enter in the field: the text is checked and applied; a refused text changes nothing and
    /// the note says why (behaviour 5).
    pub(super) fn apply_advanced_field(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(a) = self
            .advanced_editing
            .and_then(ferris_idea_core::advanced_settings::find)
        else {
            return false;
        };
        let text = self.advanced_field.read(cx).state.buffer.text();
        let mut next = self.ide.clone();
        match a.set(&mut next, Value::Text(text)) {
            Ok(()) => {
                self.change_ide(|s| *s = next, cx);
                self.note = Some(format!("{} saved", a.label));
                true
            }
            Err(e) => {
                self.note = Some(format!("Not saved: {e}"));
                cx.notify();
                false
            }
        }
    }

    /// Sets the Advanced Settings field (tests).
    pub fn set_advanced_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.advanced_field.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// System Settings (`SPEC-0337` behaviour 2).
    pub(super) fn system_page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(
                self.system_rows()
                    .into_iter()
                    .enumerate()
                    .map(|(i, (label, on, hint))| {
                        self.option_row(
                            ("settings-system", i),
                            format!("settings-system-{i}"),
                            Mark::Check(on),
                            label,
                            cx,
                        )
                        .h_auto()
                        .min_h(px(28.))
                        .child(div().text_xs().text_color(c(p.ui_dim)).child(hint))
                    }),
            )
            .child(div().pt_2().text_xs().text_color(c(p.ui_dim)).child(
                "A project always opens in a window of its own; an open project's window comes to the front.",
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("Enter or Space switches the selected option"),
            )
            .into_any_element()
    }

    /// Notifications (`SPEC-0337` behaviour 3).
    pub(super) fn notifications_page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                self.option_row(
                    ("settings-notifications", 0),
                    "settings-notifications-0".into(),
                    Mark::Check(self.ide.notifications.log_info),
                    "Keep informational notes in the Notifications tool window",
                    cx,
                )
                .h_auto()
                .min_h(px(28.)),
            )
            .child(div().pt_2().text_xs().text_color(c(p.ui_dim)).child(
                "Failures are always kept, whatever this says. The status bar shows every note.",
            ))
            .into_any_element()
    }

    /// The scheme row of Appearance, row `i` of the page (`SPEC-0337` behaviour 4).
    pub(super) fn scheme_row(&self, i: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        div()
            .pt_2()
            .child(
                row(("settings-theme", i), i == self.selected, p)
                    .child(div().w(px(150.)).child(super::hit_label(
                        "Editor color scheme",
                        &self.query,
                        p,
                    )))
                    .child(div().min_w(px(120.)).child(self.editor_scheme_shown().to_owned()))
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(
                        "←/→ or Enter chooses; editors, consoles and diffs follow, the window keeps the theme",
                    ))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        this.click_option(i, window, cx);
                    })),
            )
            .into_any_element()
    }

    /// Advanced Settings (`SPEC-0337` behaviour 5).
    pub(super) fn advanced_page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let selected = self.selected;
        let rows = self.advanced_rows();
        let editing = self.advanced_editing;
        // Its own search and "Show modified only" (`SPEC-0337` v2 behaviour 5b).
        let only = self.advanced_modified_only;
        let tools = div()
            .flex()
            .items_center()
            .gap_2()
            .pb_1()
            .child(
                crate::metrics::field_frame()
                    .flex_1()
                    .child(self.advanced_search.clone())
                    .semantics(
                        Semantics::new(Role::SearchInput)
                            .name("Search advanced settings")
                            .value(self.advanced_query.clone())
                            .test_id("settings-advanced-search"),
                    ),
            )
            .child(
                div()
                    .id("settings-advanced-modified-only")
                    .flex()
                    .gap_1()
                    .cursor_pointer()
                    .child(check(only))
                    .child("Show modified only")
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.toggle_advanced_modified_only(cx)
                    }))
                    .semantics(
                        Semantics::new(Role::CheckBox)
                            .name("Show modified only")
                            .checked(only)
                            .test_id("settings-advanced-modified-only")
                            .action(AxAction::Click)
                            .effects(&[Effect::None]),
                    ),
            );
        // The page rolls as a whole when it is taller than its area (`SPEC-0105` v8 behaviour 12).
        div()
            .id("settings-advanced")
            .flex()
            .flex_col()
            .gap_1()
            .child(tools)
            .when(rows.is_empty(), |d| {
                d.child(
                    div()
                        .py_2()
                        .text_color(c(p.ui_dim))
                        .child("No advanced setting matches the search."),
                )
            })
            .children(rows.into_iter().enumerate().map(|(i, a)| {
                let value = a.value(&self.ide);
                let modified = a.modified(&self.ide);
                let shown = match &value {
                    Value::Switch(b) => check(*b).to_owned(),
                    other => other.shown(),
                };
                let number = matches!(a.kind, Kind::Number { .. });
                row(("settings-advanced-row", i), i == selected, p)
                    .flex_col()
                    .items_start()
                    .py_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(div().flex_1().min_w_0().child(a.label))
                            .when(number, |d| {
                                d.child(super::small_button(("advanced-less", i), "‹", p).on_click(
                                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        this.selected = i;
                                        this.step_advanced(-1, cx);
                                    }),
                                ))
                            })
                            .child(
                                div()
                                    .max_w(px(320.))
                                    .truncate()
                                    .when(editing == Some(a.key), |d| d.text_color(c(p.ui_muted)))
                                    .child(shown),
                            )
                            .when(number, |d| {
                                d.child(super::small_button(("advanced-more", i), "›", p).on_click(
                                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        this.selected = i;
                                        this.step_advanced(1, cx);
                                    }),
                                ))
                            })
                            .when(modified, |d| {
                                d.child(
                                    div()
                                        .id(("advanced-reset", i))
                                        .px_1()
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(c(p.separator))
                                        .text_xs()
                                        .hover(|s| s.bg(c(p.tree_hover)))
                                        .child("Reset")
                                        .on_click(cx.listener(
                                            move |this, _: &gpui::ClickEvent, _, cx| {
                                                cx.stop_propagation();
                                                this.selected = i;
                                                this.reset_advanced(cx);
                                            },
                                        )),
                                )
                            }),
                    )
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(format!(
                        "{} · default {}{}",
                        a.key,
                        match a.default_value() {
                            Value::Switch(b) => if b { "on" } else { "off" }.to_owned(),
                            other => other.shown(),
                        },
                        if modified { " · modified" } else { "" }
                    )))
                    .child(div().text_xs().text_color(c(p.ui_muted)).child(a.description))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        if a.kind == Kind::Text {
                            this.selected = i;
                            this.edit_advanced(window, cx);
                        } else {
                            // A switch or a number: the page takes the keys (`SPEC-0105` v11
                            // behaviour 17).
                            this.click_option(i, window, cx);
                        }
                    }))
            }))
            .child(
                div()
                    .pt_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_color(c(p.ui_muted))
                            .child(match editing.and_then(ferris_idea_core::advanced_settings::find) {
                                Some(a) => a.label,
                                None => "Text value",
                            }),
                    )
                    .child(
                        crate::metrics::field_frame()
                            .flex_1()
                            .child(self.advanced_field.clone()),
                    ),
            )
            .child(div().text_xs().text_color(c(p.ui_dim)).child(
                "Enter or Space switches, Left and Right change a number, Enter on a text puts it in the field \
                 (Enter there saves). Delete resets the selected setting.",
            ))
            .when_some(self.note.clone(), |d, note| {
                d.child(div().text_color(c(p.ui_muted)).child(note))
            })
            .into_any_element()
    }

    /// Closes the dialog's event flow for the field: Escape and Tab leave it (behaviour 5).
    pub(super) fn leave_advanced_field(&mut self, window: &mut Window) {
        self.advanced_editing = None;
        window.focus(&self.focus);
    }
}
