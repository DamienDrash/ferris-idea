//! The comment marks of the project room in the editor's gutter (`SPEC-0323` v8 behaviour 53):
//! a lane at the gutter's left, there while the document has comments, with one mark per line
//! that has comments, the count in it when there are several, in the accent colour while a
//! thread is open and grey when all are resolved. A click asks the window for the thread popover
//! (`EditorEvent::RoomComments`); a tooltip names the count.

use gpui::{
    Bounds, Context, Pixels, ShapedLine, TextRun, Window, div, fill, point, prelude::*, px, size,
};

use super::{EditorEvent, EditorView, Frame, PaintedLine, color, editor_font, line_height};
use crate::palette::Palette;
use crate::room_comments_model::CommentMark;

/// Width of the comment lane.
const LANE: f32 = 22.0;
/// Size of a mark.
const MARK_W: f32 = 16.0;
const MARK_H: f32 = 12.0;

/// The test id and the name of the mark on 0-based `line`: `room-comment-` and the line counted
/// from 1, as the editor shows it.
pub(super) fn comment_mark_id(line: usize) -> String {
    format!("room-comment-{}", line + 1)
}

impl EditorView {
    /// Shows the room's comment marks of this document (behaviour 53); redraws only when they
    /// changed.
    pub fn set_comment_marks(&mut self, marks: Vec<CommentMark>, cx: &mut Context<Self>) {
        if self.comment_marks != marks {
            self.comment_marks = marks;
            cx.notify();
        }
    }

    /// The comment marks shown, in line order.
    pub fn comment_marks(&self) -> &[CommentMark] {
        &self.comment_marks
    }

    /// The width of the comment lane: there while the document has comments.
    pub(super) fn comment_width(&self) -> Pixels {
        if self.field || self.comment_marks.is_empty() {
            px(0.0)
        } else {
            px(LANE)
        }
    }

    /// Places the marks of the painted lines in the lane starting at `left`.
    pub(super) fn layout_comment_marks(
        &mut self,
        painted: &[PaintedLine],
        left: Pixels,
        font_size: Pixels,
        frame: &mut Frame,
        window: &mut Window,
    ) {
        self.comment_hits.clear();
        if self.comment_width() == px(0.0) {
            return;
        }
        let p = Palette::current();
        let text_system = window.text_system().clone();
        for line in painted.iter().filter(|l| !l.above) {
            let Some(mark) = self.comment_marks.iter().find(|m| m.line == line.line) else {
                continue;
            };
            let b = Bounds::new(
                point(
                    left + (px(LANE) - px(MARK_W)) / 2.0,
                    line.origin.y + (px(line_height()) - px(MARK_H)) / 2.0,
                ),
                size(px(MARK_W), px(MARK_H)),
            );
            let label = mark.label().map(|text| {
                text_system.shape_line(
                    text.clone().into(),
                    font_size * 0.7,
                    &[TextRun {
                        len: text.len(),
                        font: editor_font(),
                        color: color(p.accent_text.rgb),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
            });
            let fill_colour = if mark.open {
                p.accent.rgb
            } else {
                p.line_number
            };
            frame.comment_marks.push((b, label, fill_colour));
            self.comment_hits.push((b, *mark));
        }
    }

    /// A click on a mark: the window opens its line's threads. Returns whether it was one.
    pub(super) fn comment_mark_click(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let lane = self.comment_width();
        if lane == px(0.0) {
            return false;
        }
        let left = self.last.costs_x;
        if position.x < left || position.x >= left + lane {
            return false;
        }
        let Some(line) = self.line_at_y(position.y) else {
            return false;
        };
        if !self.comment_marks.iter().any(|m| m.line == line) {
            return false;
        }
        cx.emit(EditorEvent::RoomComments(line));
        cx.stop_propagation();
        true
    }

    /// Invisible boxes over the marks of the last frame with their tooltips; they take no clicks.
    pub(super) fn comment_tip_overlays(&self) -> Vec<gpui::AnyElement> {
        use crate::tips::Tipped;
        let origin = self.last.origin;
        self.comment_hits
            .iter()
            .map(|(b, mark)| {
                let tip = ferris_idea_ui::tooltip::Tip {
                    title: mark.tooltip(),
                    shortcut: String::new(),
                    details: Vec::new(),
                };
                let name = comment_mark_id(mark.line);
                div()
                    .id(gpui::SharedString::from(format!("{name}-tip")))
                    .absolute()
                    .left(b.origin.x - origin.x)
                    .top(b.origin.y - origin.y)
                    .w(b.size.width)
                    .h(b.size.height)
                    .tip_text(name, tip)
                    .into_any_element()
            })
            .collect()
    }
}

/// Paints the marks: a rounded box in their colour with the count in it.
pub(super) fn paint(
    marks: &[(Bounds<Pixels>, Option<ShapedLine>, u32)],
    window: &mut Window,
    cx: &mut gpui::App,
) {
    for (b, label, rgb) in marks {
        window.paint_quad(fill(*b, color(*rgb)).corner_radii(px(3.0)));
        if let Some(label) = label {
            let at = point(b.origin.x + (b.size.width - label.width) / 2.0, b.origin.y);
            let _ = label.paint(at, b.size.height, window, cx);
        }
    }
}
