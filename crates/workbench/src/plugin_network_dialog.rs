//! The warning before a connector plugin first connects to a host (`SPEC-0314` behaviour 7):
//! plugin, author, target, status and that Ferris IDEA does not review its code, with "Cancel"
//! (selected) and "Allow and connect".

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::palette::Palette;
use crate::workbench_view::c;

/// What the warning asks the workbench to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginNetworkDialogEvent {
    /// Store a grant for the user and connect.
    Allow,
    /// Do not connect.
    Cancel,
}

/// The warning dialog.
pub struct PluginNetworkDialog {
    warning: ferris_idea_plugins::NetworkWarning,
    allow_selected: bool,
    focus: FocusHandle,
}

impl EventEmitter<PluginNetworkDialogEvent> for PluginNetworkDialog {}

impl PluginNetworkDialog {
    /// The warning `warning`; Cancel is selected.
    pub fn new(warning: ferris_idea_plugins::NetworkWarning, cx: &mut Context<Self>) -> Self {
        Self {
            warning,
            allow_selected: false,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the dialog.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// What it shows (checks).
    pub fn warning(&self) -> &ferris_idea_plugins::NetworkWarning {
        &self.warning
    }

    /// Whether "Allow and connect" is selected (Cancel otherwise).
    pub fn allow_selected(&self) -> bool {
        self.allow_selected
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "left" | "right" | "tab" => self.allow_selected = !self.allow_selected,
            "enter" => cx.emit(if self.allow_selected {
                PluginNetworkDialogEvent::Allow
            } else {
                PluginNetworkDialogEvent::Cancel
            }),
            "escape" => cx.emit(PluginNetworkDialogEvent::Cancel),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for PluginNetworkDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let w = &self.warning;
        div()
            .id("plugin-network-dialog")
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
                    .flex()
                    .gap_2()
                    .child(div().text_color(c(p.error_text)).child("⚠"))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(w.title.clone()),
                    ),
            )
            .children(w.lines.iter().enumerate().map(|(i, line)| {
                div()
                    .id(("plugin-network-line", i))
                    // The last line, that nobody reviews the code, stands out.
                    .when(i + 1 == w.lines.len(), |d| {
                        d.font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .child(line.clone())
            }))
            .child(
                div().text_xs().text_color(c(p.ui_muted)).child(
                    "Grants are kept per plugin and host; revoke them in Settings › Plugins.",
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .pt_1()
                    .child(div().flex_1())
                    .child(
                        crate::metrics::button(
                            "plugin-network-cancel",
                            "Cancel",
                            !self.allow_selected,
                        )
                        .on_click(cx.listener(
                            |_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(PluginNetworkDialogEvent::Cancel)
                            },
                        )),
                    )
                    .child(
                        crate::metrics::button(
                            "plugin-network-allow",
                            "Allow and connect",
                            self.allow_selected,
                        )
                        .on_click(cx.listener(
                            |_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(PluginNetworkDialogEvent::Allow)
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("Tab or ←→ choose · Enter confirms · Escape cancels"),
            )
    }
}
