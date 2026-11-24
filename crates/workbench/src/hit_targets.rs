//! Where named controls were last painted, in window coordinates, so the window self-test can click
//! them with real mouse events (`SPEC-0275` behaviour 12). Off unless the self-test turns it on;
//! then the marks cost one empty canvas per marked control.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use ferris_idea_ui::tool_windows::Rect;
use gpui::{IntoElement, Styled};

static ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TARGETS: RefCell<HashMap<String, Rect>> = RefCell::new(HashMap::new());
}

/// Records the bounds of marked controls from now on.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Where the control `name` was last painted.
pub fn get(name: &str) -> Option<Rect> {
    TARGETS.with(|t| t.borrow().get(name).copied())
}

/// Records `rect` as the place of `name` directly, for a spot that is no control, such as a word in
/// the editor the pointer should rest on (`SPEC-0077` v6 self-test); nothing unless recording is on.
pub fn set(name: impl Into<String>, rect: Rect) {
    if ENABLED.load(Ordering::Relaxed) {
        TARGETS.with(|t| t.borrow_mut().insert(name.into(), rect));
    }
}

/// Forgets every recorded control (before a step whose rows change).
pub fn clear() {
    TARGETS.with(|t| t.borrow_mut().clear());
}

/// An invisible element filling its (relatively positioned) parent that records the parent's
/// visible bounds under `name` (clipped by the scroll areas and windows around it, so that a row
/// scrolled out of its list is not clicked through to the window below); nothing unless recording
/// is on. While the semantic tree is built, the mark is also a node with the test id `name`
/// (`SPEC-0319` behaviour 42): the driver addresses it by that id.
pub fn mark(name: impl Into<String>) -> Option<impl IntoElement> {
    let marks = ENABLED.load(Ordering::Relaxed);
    if !marks && !crate::semantics::active() {
        return None;
    }
    let name = name.into();
    Some(
        gpui::canvas(
            move |b, window, _| record(window, &name, b),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

/// Records `b`, painted in this frame, as the place of the control `name`, clipped by the
/// content mask, like [`mark`] does for an element: for a spot an element paints itself, such as a
/// usage count in the editor (`SPEC-0292` v8 behaviour 15). While the semantic tree is built, the
/// spot is also a node with the test id `name` (`SPEC-0319` behaviour 42). Called while the frame is
/// prepainted; nothing unless recording or the semantic tree is on.
pub fn record(window: &gpui::Window, name: &str, b: gpui::Bounds<gpui::Pixels>) {
    let marks = ENABLED.load(Ordering::Relaxed);
    if !marks && !crate::semantics::active() {
        return;
    }
    let rect = |b: gpui::Bounds<gpui::Pixels>| Rect {
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        w: f32::from(b.size.width),
        h: f32::from(b.size.height),
    };
    let shown = visible(rect(b), rect(window.content_mask().bounds));
    if crate::semantics::active() {
        let painted = rect(b);
        crate::semantics::record_mark(
            window,
            name,
            ferris_idea_uidriver::Rect::new(painted.x, painted.y, painted.w, painted.h),
            shown.map(|r| ferris_idea_uidriver::Rect::new(r.x, r.y, r.w, r.h)),
        );
    }
    if !marks {
        return;
    }
    TARGETS.with(|t| match shown {
        Some(r) => {
            t.borrow_mut().insert(name.to_owned(), r);
        }
        None => {
            t.borrow_mut().remove(name);
        }
    });
}

/// The part of `r` inside `clip`, if any.
fn visible(r: Rect, clip: Rect) -> Option<Rect> {
    let x = r.x.max(clip.x);
    let y = r.y.max(clip.y);
    let right = (r.x + r.w).min(clip.x + clip.w);
    let bottom = (r.y + r.h).min(clip.y + clip.h);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        w: right - x,
        h: bottom - y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn rows_outside_their_list_are_not_targets() {
        // The Containers list in a 96 px bottom window shows 60 px of rows: the fourth row is
        // scrolled out and must not be clicked through to the window below.
        let list = r(352., 202., 896., 60.);
        assert_eq!(visible(r(356., 268., 888., 22.), list), None);
        // A row cut by the list's edge is clicked in its visible part.
        assert_eq!(
            visible(r(356., 246., 888., 22.), list),
            Some(r(356., 246., 888., 16.))
        );
        assert_eq!(
            visible(r(356., 202., 888., 22.), list),
            Some(r(356., 202., 888., 22.))
        );
    }
}
