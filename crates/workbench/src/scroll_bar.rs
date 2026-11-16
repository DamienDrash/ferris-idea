//! A vertical scroll bar beside a list or a page that rolls (`SPEC-0105` v8 behaviours 11 and
//! 12): a narrow track whose thumb is the part shown. Dragging the thumb rolls with the pointer, a
//! click beside it rolls by the height shown. The thumb is painted from the scroll state of the
//! frame being drawn, so it never lags behind the list it belongs to.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Bounds, Div, Hsla, MouseButton, MouseDownEvent, Pixels, ScrollHandle, Stateful, canvas, div,
    point, prelude::*, px, size,
};

use crate::semantics::{Annotated, Role, Semantics};

/// Width of the track in points.
pub const WIDTH: f32 = 8.0;
/// The thumb is never shorter, so it can always be taken.
const MIN_THUMB: f32 = 20.0;

/// The thumb on a track `track` points high, as top and height: for a viewport of `viewport`
/// points whose content is `max` points longer, rolled `rolled` points down; `None` while
/// everything fits.
pub fn thumb(track: f32, viewport: f32, max: f32, rolled: f32) -> Option<(f32, f32)> {
    if max <= 0.5 || track <= 0.0 || viewport <= 0.0 {
        return None;
    }
    let height = (track * viewport / (viewport + max)).clamp(MIN_THUMB.min(track), track);
    let top = (track - height) * (rolled / max).clamp(0.0, 1.0);
    Some((top, height))
}

/// How far to roll so that a thumb `height` points high stands `top` points below the top of a
/// track `track` points high, for content `max` points longer than the viewport.
pub fn rolled_for(top: f32, track: f32, height: f32, max: f32) -> f32 {
    let room = track - height;
    if room <= 0.0 {
        return 0.0;
    }
    (top / room).clamp(0.0, 1.0) * max
}

/// Where a click `y` points below the track's top rolls to: one viewport up above the thumb, one
/// down below it, nowhere on it.
pub fn page_click(y: f32, thumb: (f32, f32), viewport: f32, max: f32, rolled: f32) -> f32 {
    if y < thumb.0 {
        (rolled - viewport).max(0.0)
    } else if y > thumb.0 + thumb.1 {
        (rolled + viewport).min(max)
    } else {
        rolled
    }
}

/// The viewport's height, how much longer the content is and how far it is rolled, in points.
fn state(handle: &ScrollHandle) -> (f32, f32, f32) {
    (
        f32::from(handle.bounds().size.height),
        f32::from(handle.max_offset().height),
        -f32::from(handle.offset().y),
    )
}

fn roll_to(handle: &ScrollHandle, rolled: f32) {
    let x = handle.offset().x;
    handle.set_offset(point(x, px(-rolled)));
}

/// What a drag on a thumb carries: the bar it started on and where the pointer holds the thumb.
#[derive(Clone)]
struct ThumbDrag {
    bar: &'static str,
    grab: Rc<Cell<f32>>,
}

/// The invisible view that follows the pointer while a thumb is dragged.
struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The scroll bar of the element that tracks `handle`, placed beside it at the same height: `id`
/// is its element id and test id, `name` names what it rolls (a node of the semantic tree,
/// `SPEC-0319`), `color` is the thumb's.
pub fn vertical(
    id: &'static str,
    name: &'static str,
    handle: &ScrollHandle,
    color: Hsla,
) -> Stateful<Div> {
    // The track's bounds as last painted, for the clicks.
    let track: Rc<Cell<Bounds<Pixels>>> = Rc::default();
    let painted = track.clone();
    let paint_handle = handle.clone();
    let click_handle = handle.clone();
    let start_handle = handle.clone();
    let move_handle = handle.clone();
    div()
        .id(id)
        .flex_none()
        .w(px(WIDTH))
        .h_full()
        .relative()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    painted.set(bounds);
                    let (viewport, max, rolled) = state(&paint_handle);
                    let track_h = f32::from(bounds.size.height);
                    if let Some((top, height)) = thumb(track_h, viewport, max, rolled) {
                        let at = point(bounds.origin.x + px(2.), bounds.origin.y + px(top));
                        let quad =
                            gpui::fill(Bounds::new(at, size(px(WIDTH - 4.), px(height))), color)
                                .corner_radii(px((WIDTH - 4.) / 2.));
                        window.paint_quad(quad);
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, _| {
            let bounds = track.get();
            let (viewport, max, rolled) = state(&click_handle);
            let Some(t) = thumb(f32::from(bounds.size.height), viewport, max, rolled) else {
                return;
            };
            let y = f32::from(e.position.y - bounds.origin.y);
            let to = page_click(y, t, viewport, max, rolled);
            if (to - rolled).abs() > 0.5 {
                roll_to(&click_handle, to);
                window.refresh();
            }
        })
        .on_drag(
            ThumbDrag {
                bar: id,
                grab: Rc::default(),
            },
            move |drag, at, _, cx| {
                // `at` is the pointer below the track's top: the thumb is held there, or by its
                // middle when the drag started beside it.
                let (viewport, max, rolled) = state(&start_handle);
                let track_h = f32::from(start_handle.bounds().size.height);
                let y = f32::from(at.y);
                let grab = match thumb(track_h, viewport, max, rolled) {
                    Some((top, height)) if y >= top && y <= top + height => y - top,
                    Some((_, height)) => height / 2.,
                    None => 0.,
                };
                drag.grab.set(grab);
                cx.new(|_| DragGhost)
            },
        )
        .on_drag_move::<ThumbDrag>(move |e, window, cx| {
            let drag = e.drag(cx);
            if drag.bar != id {
                return;
            }
            let track_h = f32::from(e.bounds.size.height);
            let (viewport, max, _) = state(&move_handle);
            let Some((_, height)) = thumb(track_h, viewport, max, 0.) else {
                return;
            };
            let top = f32::from(e.event.position.y - e.bounds.origin.y) - drag.grab.get();
            roll_to(&move_handle, rolled_for(top, track_h, height, max));
            window.refresh();
        })
        .semantics(Semantics::new(Role::ScrollBar).name(name).test_id(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0105 v8 AK-13: the thumb is the part shown and moves with the roll; a drag puts the
    /// roll where the thumb is; a click beside the thumb rolls by a viewport.
    #[test]
    fn the_thumb_shows_the_part_shown_and_follows_a_drag() {
        assert_eq!(thumb(200., 200., 0., 0.), None, "everything fits");
        // Half the content shown: half the track.
        assert_eq!(thumb(200., 200., 200., 0.), Some((0., 100.)));
        assert_eq!(thumb(200., 200., 200., 200.), Some((100., 100.)));
        assert_eq!(thumb(200., 200., 200., 100.), Some((50., 100.)));
        // A long list keeps a thumb that can be taken.
        assert_eq!(
            thumb(200., 200., 100_000., 0.).map(|t| t.1),
            Some(MIN_THUMB)
        );
        // A drag: the thumb's top at the track's middle of its room rolls half way.
        assert_eq!(rolled_for(50., 200., 100., 200.), 100.);
        assert_eq!(rolled_for(-10., 200., 100., 200.), 0.);
        assert_eq!(rolled_for(500., 200., 100., 200.), 200.);
        assert_eq!(rolled_for(0., 100., 100., 200.), 0., "no room");
        // A click above the thumb rolls up, below it down, on it nowhere.
        let t = (50., 100.);
        assert_eq!(page_click(10., t, 200., 200., 100.), 0.);
        assert_eq!(page_click(180., t, 200., 200., 100.), 200.);
        assert_eq!(page_click(90., t, 200., 200., 100.), 100.);
    }
}
