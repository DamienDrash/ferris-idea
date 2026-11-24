//! The theme popup of Quick Switch Scheme (`SPEC-0060` v5): the bundled themes, the chosen one
//! marked; Enter switches.

use ferris_idea_ui::theme::ThemeId;
use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::palette::{Palette, current_theme};
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
pub enum ThemesEvent {
    /// Switch to this theme.
    Choose(ThemeId),
    /// Close the popup.
    Close,
}

/// The bundled themes (behaviour 10).
pub struct ThemesView {
    selected: usize,
    focus: FocusHandle,
}

impl EventEmitter<ThemesEvent> for ThemesView {}

impl ThemesView {
    /// A popup with the chosen theme selected.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let now = current_theme();
        Self {
            selected: ThemeId::ALL.iter().position(|t| *t == now).unwrap_or(0),
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the popup.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The rows as shown, the chosen theme marked with "✓".
    pub fn rows(&self) -> Vec<String> {
        let now = current_theme();
        ThemeId::ALL
            .iter()
            .map(|t| {
                if *t == now {
                    format!("✓ {}", t.name())
                } else {
                    t.name().to_owned()
                }
            })
            .collect()
    }

    /// Index of the selected row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    fn choose(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(t) = ThemeId::ALL.get(i) {
            cx.emit(ThemesEvent::Choose(*t));
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let n = ThemeId::ALL.len();
        // The first and the last theme (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(&event.keystroke, n) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.selected = (self.selected + 1) % n,
            "up" => self.selected = (self.selected + n - 1) % n,
            "enter" => self.choose(self.selected, cx),
            "escape" => cx.emit(ThemesEvent::Close),
            k => match k.parse::<usize>() {
                // Digits choose a row as in the reference's quick lists.
                Ok(d) if (1..=n).contains(&d) => self.choose(d - 1, cx),
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for ThemesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let now = current_theme();
        div()
            .id("themes-popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(px(300.))
            .py_1()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .px_3()
                    .pt_1()
                    .pb_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("Theme"),
            )
            .children(ThemeId::ALL.iter().enumerate().map(|(i, t)| {
                div()
                    .id(("theme-row", i))
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(28.))
                    .mx_1()
                    .px_2()
                    .rounded(px(4.))
                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                    .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .child(if *t == now { "✓" } else { "" }),
                    )
                    .child(div().flex_1().child(t.name()))
                    .child(div().text_color(c(p.ui_dim)).child(format!("{}", i + 1)))
                    .on_click(
                        cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.choose(i, cx)),
                    )
            }))
    }
}
