//! The import preview (`SPEC-0303` behaviour 11): what an exchange file contains and what it would
//! change, with its problems; Enter or "Import" confirms, "Share…" opens a prefilled issue,
//! Escape or "Cancel" closes without change.

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use idecraft_formats::Preview;

use crate::palette::Palette;
use crate::workbench_view::c;

/// What the preview asks the workbench to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPreviewEvent {
    /// Import the file.
    Import,
    /// Open the Share link in the browser; the preview stays.
    Share,
    /// Close without change.
    Close,
}

/// The preview dialog.
pub struct ImportPreview {
    preview: Preview,
    focus: FocusHandle,
}

impl EventEmitter<ImportPreviewEvent> for ImportPreview {}

impl ImportPreview {
    /// A dialog showing `preview`.
    pub fn new(preview: Preview, cx: &mut Context<Self>) -> Self {
        Self {
            preview,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the dialog.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// What the dialog shows.
    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    /// The lines as shown, heading first.
    pub fn lines(&self) -> Vec<String> {
        self.preview.lines()
    }

    /// Whether Import is enabled.
    pub fn can_import(&self) -> bool {
        self.preview.can_import()
    }

    /// Confirms (Enter, "Import"); does nothing while the file has problems.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.can_import() {
            cx.emit(ImportPreviewEvent::Import);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        }
        match k.key.as_str() {
            "enter" => self.confirm(cx),
            "escape" => cx.emit(ImportPreviewEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
    }
}

fn button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    let p = Palette::current();
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded(px(4.))
        .border_1()
        .border_color(c(p.separator))
        .when(primary && enabled, |d| d.bg(c(p.tree_selection)))
        .when(!enabled, |d| d.text_color(c(p.ui_dim)))
        .child(label)
}

impl Render for ImportPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let lines = self.preview.lines();
        let (heading, rest) = lines
            .split_first()
            .map_or((String::new(), &[][..]), |(h, r)| (h.clone(), r));
        let can = self.can_import();
        let rows: Vec<_> = rest
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let section = !line.starts_with(' ');
                div()
                    .id(("import-line", i))
                    .px_3()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .when(section, |d| d.pt_1().text_color(c(p.ui_muted)))
                    .when(
                        line.starts_with("  ")
                            && self
                                .preview
                                .problems
                                .iter()
                                .any(|pr| line.trim() == pr.to_string()),
                        |d| d.text_color(c(p.error_text)),
                    )
                    .child(line.clone())
            })
            .collect();
        div()
            .id("import-preview")
            .track_focus(&self.focus)
            .occlude()
            .on_key_down(cx.listener(Self::on_key))
            .w(px(720.))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(heading),
            )
            .child(
                div()
                    .id("import-lines")
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .py_1()
                    .children(rows),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .child(
                        button("import-share", "Share…", true, false).on_click(cx.listener(
                            |_, _: &gpui::ClickEvent, _, cx| cx.emit(ImportPreviewEvent::Share),
                        )),
                    )
                    .child(
                        button("import-cancel", "Cancel", true, false).on_click(cx.listener(
                            |_, _: &gpui::ClickEvent, _, cx| cx.emit(ImportPreviewEvent::Close),
                        )),
                    )
                    .child(button("import-confirm", "Import", can, true).on_click(
                        cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.confirm(cx)),
                    )),
            )
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(if can {
                        "Enter imports · Escape cancels · nothing changes before you import"
                    } else {
                        "This file cannot be imported: see the problems · Escape closes"
                    }),
            )
    }
}
