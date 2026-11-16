//! The confirmation before a command suggested by a plugin first starts (`SPEC-0276`
//! behaviour 5): the exact program, arguments, folder and environment, and Cancel, Run Once or
//! Always Allow in This Workspace.

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, font, prelude::*, px};

use crate::palette::Palette;
use crate::plugin_run_model::{ConfirmChoice, Confirmation, PluginRunEntry};
use crate::workbench_view::c;

/// What the confirmation asks the workbench to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRunDialogEvent {
    /// The user chose this.
    Chosen(ConfirmChoice),
}

/// The confirmation dialog.
pub struct PluginRunDialog {
    entry: PluginRunEntry,
    confirmation: Confirmation,
    focus: FocusHandle,
}

impl EventEmitter<PluginRunDialogEvent> for PluginRunDialog {}

impl PluginRunDialog {
    /// The confirmation of `entry` in the project at `root`; Cancel is selected.
    pub fn new(entry: PluginRunEntry, root: &std::path::Path, cx: &mut Context<Self>) -> Self {
        let confirmation = Confirmation::new(&entry, root);
        Self {
            entry,
            confirmation,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the dialog.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The suggestion it asks about.
    pub fn entry(&self) -> &PluginRunEntry {
        &self.entry
    }

    /// What it shows (checks).
    pub fn confirmation(&self) -> &Confirmation {
        &self.confirmation
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let known = matches!(
            k.key.as_str(),
            "up" | "down" | "left" | "right" | "tab" | "enter" | "escape"
        );
        if !known {
            return;
        }
        if let Some(choice) = self.confirmation.key(&k.key, k.modifiers.shift) {
            cx.emit(PluginRunDialogEvent::Chosen(choice));
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for PluginRunDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.confirmation.choice();
        div()
            .id("plugin-run-dialog")
            .track_focus(&self.focus)
            .occlude()
            .on_key_down(cx.listener(Self::on_key))
            .w(px(560.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(8.))
            .border_1()
            .border_color(c(p.separator))
            .bg(c(p.island))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.confirmation.title.clone()),
            )
            .child(div().text_color(c(p.ui_muted)).text_xs().child(
                "Plugins cannot start programs. IDECraft starts exactly this, without a shell:",
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(self.confirmation.lines.iter().enumerate().map(
                        |(i, (label, value))| {
                            div()
                                .id(("plugin-run-line", i))
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .w(px(96.))
                                        .flex_none()
                                        .text_color(c(p.ui_muted))
                                        .child(label.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font(font(crate::fonts::editor_family()))
                                        .child(value.clone()),
                                )
                        },
                    )),
            )
            .child(div().flex().gap_2().pt_1().child(div().flex_1()).children(
                ConfirmChoice::ALL.into_iter().map(|choice| {
                    crate::metrics::button(
                        ("plugin-run-choice", choice as usize),
                        choice.label(),
                        choice == selected,
                    )
                    .on_click(cx.listener(
                        move |_, _: &gpui::ClickEvent, _, cx| {
                            cx.emit(PluginRunDialogEvent::Chosen(choice))
                        },
                    ))
                }),
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("↑↓ or Tab choose · Enter confirms · Escape cancels"),
            )
    }
}
