//! The usages popup (`SPEC-0073`): every use of a symbol, Enter jumps to one.

use std::ops::Range;

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, uniform_list,
};

use crate::icon_images::icon;
use crate::nav_model::Place;
use crate::palette::Palette;
use crate::usages_model::Usage;
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
pub enum UsagesEvent {
    /// Jump to a usage.
    Open(Place),
    /// Close the popup.
    Close,
}

/// Usages of one symbol.
pub struct UsagesView {
    title: String,
    usages: Vec<Usage>,
    selected: usize,
    focus: FocusHandle,
}

impl EventEmitter<UsagesEvent> for UsagesView {}

impl UsagesView {
    /// A popup titled after `symbol` listing `usages`.
    pub fn new(symbol: &str, usages: Vec<Usage>, cx: &mut Context<Self>) -> Self {
        Self {
            title: format!("Usages of {symbol} ({})", usages.len()),
            usages,
            selected: 0,
            focus: cx.focus_handle(),
        }
    }

    /// A popup with its own `title`, e.g. a chooser of implementations (SPEC-0081).
    pub fn titled(title: String, usages: Vec<Usage>, cx: &mut Context<Self>) -> Self {
        Self {
            title,
            usages,
            selected: 0,
            focus: cx.focus_handle(),
        }
    }

    /// The popup's title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Focus handle of the list.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The usages listed.
    pub fn usages(&self) -> &[Usage] {
        &self.usages
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        if let Some(u) = self.usages.get(self.selected) {
            cx.emit(UsagesEvent::Open(u.place.clone()));
        }
    }

    /// Up/Down select with wrap-around, Enter jumps, Escape closes (behaviour 3).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last usage (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.usages.len()) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift {
            return;
        }
        let n = self.usages.len().max(1);
        match k.key.as_str() {
            "down" => self.selected = (self.selected + 1) % n,
            "up" => self.selected = (self.selected + n - 1) % n,
            "enter" => self.choose(cx),
            "escape" => cx.emit(UsagesEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for UsagesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let usages = self.usages.clone();
        let selected = self.selected;
        let list = uniform_list(
            "usages",
            usages.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let u = &usages[i];
                        let name = std::path::Path::new(&u.shown_path)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let dim = c(if i == selected { p.ui_muted } else { p.ui_dim });
                        // A chooser row names the class, its package and module (SPEC-0081 v2).
                        if let Some(o) = &u.owner {
                            return div()
                                .id(("usage", i))
                                .flex()
                                .items_center()
                                .gap_2()
                                .h(px(24.))
                                .w_full()
                                .px_2()
                                .rounded(px(4.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .when(i == selected, |d| d.bg(c(p.tree_selection)))
                                .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                .child(icon(idecraft_ui::icons::symbol_icon(&o.kind)))
                                .child(div().flex_none().child(o.name.clone()))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .text_color(dim)
                                        .child(
                                            o.package
                                                .as_ref()
                                                .map(|p| format!("({p})"))
                                                .unwrap_or_default(),
                                        ),
                                )
                                .child(div().flex_none().text_color(dim).child(o.module.clone()))
                                .on_click(cx.listener(
                                    move |this, _: &gpui::ClickEvent, _, cx| {
                                        this.selected = i;
                                        this.choose(cx);
                                    },
                                ));
                        }
                        div()
                            .id(("usage", i))
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(24.))
                            .w_full()
                            .px_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            // One line per row: the code ends in "…" when longer (SPEC-0073 v4).
                            .whitespace_nowrap()
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(icon(idecraft_ui::icons::file_icon(&name)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .font_family(crate::fonts::editor_family())
                                    .child(u.text.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(c(if i == selected {
                                        p.ui_muted
                                    } else {
                                        p.ui_dim
                                    }))
                                    .child(format!("{}:{}", u.shown_path, u.place.line + 1)),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                this.choose(cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .px_1()
        .h(px((self.usages.len().clamp(1, 14) as f32) * 24.0));
        div()
            .id("usages-popup")
            .track_focus(&self.focus)
            .occlude()
            .w(px(640.))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_color(c(p.ui_muted))
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .py_1()
                    .child(list),
            )
    }
}
