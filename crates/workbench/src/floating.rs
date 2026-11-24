//! Float and Window tool windows (`SPEC-0061` v7 behaviour 39): a separate window that shows one
//! tool window's header and content. The workbench keeps the content; this view draws it in its
//! own window and tells the workbench when the user closes the window.

use ferris_idea_ui::tool_windows::{Rect, ToolWindow};
use gpui::{Context, Entity, IntoElement, Render, WeakEntity, Window, div, prelude::*, px};

use crate::palette::Palette;
use crate::workbench_view::{WorkbenchView, c};

/// The view of a Float or Window tool window.
pub struct FloatingToolWindow {
    bench: WeakEntity<WorkbenchView>,
    tool: ToolWindow,
    _observe: gpui::Subscription,
}

impl FloatingToolWindow {
    /// The separate window of `tool`, drawn from `bench`'s state; it draws again whenever the
    /// workbench changes, and closing it hides the tool window.
    pub fn new(
        bench: Entity<WorkbenchView>,
        tool: ToolWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&bench, |_, _, cx| cx.notify());
        let weak = bench.downgrade();
        let closing = weak.clone();
        window.on_window_should_close(cx, move |window, cx| {
            let frame = rect(window.bounds());
            let _ = closing.update(cx, |b, cx| b.floating_closed(tool, frame, cx));
            true
        });
        Self {
            bench: weak,
            tool,
            _observe: observe,
        }
    }
}

/// A window frame as a layout rectangle.
pub fn rect(b: gpui::Bounds<gpui::Pixels>) -> Rect {
    Rect {
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        w: f32::from(b.size.width),
        h: f32::from(b.size.height),
    }
}

impl Render for FloatingToolWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let tool = self.tool;
        let body = self
            .bench
            .update(cx, |b, cx| b.floating_body(tool, window, cx))
            .ok()
            .flatten();
        div()
            .id(gpui::SharedString::from(format!("floating-{}", tool.id())))
            .size_full()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .text_color(c(p.ui_text))
            .text_size(px(13.))
            .children(body)
    }
}
