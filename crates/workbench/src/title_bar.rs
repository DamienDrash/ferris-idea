//! The controls that share the title bar with the main toolbar (`SPEC-0061` v15 behaviour 61).
//!
//! On macOS the main toolbar lies in the window's title bar, and the system counts the clicks in
//! a title bar itself: two within its double-click time zoom (or minimise) the window — also when
//! both hit a button, whose second click is then lost. A control of the toolbar therefore tells
//! the application of every press on it, and the application makes the system start counting
//! anew: each press on a control is a click of its own, the control gets both clicks and the
//! window keeps its size. Two clicks on the empty part of the title bar stay the system's
//! gesture.
//!
//! This crate does not call AppKit: the application registers what a press does ([`register`]),
//! as it does for the window buttons (`traffic_lights`).

use std::sync::OnceLock;

use gpui::{DispatchPhase, IntoElement, MouseButton, MouseDownEvent, Styled};

/// Where a control was painted, in window coordinates (points, origin at the top left of the
/// window's content).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Place {
    /// Whether the point lies on the control.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Whether a press belongs to the control painted at `place` and not to the title bar: the
/// primary button, on the control.
pub fn press_on_control(place: Place, x: f32, y: f32, primary: bool) -> bool {
    primary && place.contains(x, y)
}

/// Called with the window for every press on a control of its title bar; set once by the
/// application.
pub type Pressed = fn(&gpui::Window);

static PRESSED: OnceLock<Pressed> = OnceLock::new();

/// Registers what a press on a title-bar control does (the application does at start). Until
/// then the controls tell nothing.
pub fn register(pressed: Pressed) {
    let _ = PRESSED.set(pressed);
}

/// An invisible element filling its parent that makes the parent a control of the title bar: a
/// press of the primary button on it is told to the application while the press is dispatched.
/// Nothing while no application listens.
pub fn control() -> Option<impl IntoElement> {
    let pressed = *PRESSED.get()?;
    Some(
        gpui::canvas(
            |bounds, _, _| bounds,
            move |_, bounds, window, _| {
                let place = Place {
                    x: f32::from(bounds.origin.x),
                    y: f32::from(bounds.origin.y),
                    w: f32::from(bounds.size.width),
                    h: f32::from(bounds.size.height),
                };
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, _| {
                    // Before any listener of the control itself can end the dispatch.
                    if phase == DispatchPhase::Capture
                        && press_on_control(
                            place,
                            f32::from(e.position.x),
                            f32::from(e.position.y),
                            e.button == MouseButton::Left,
                        )
                    {
                        pressed(window);
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0061 v15 AK-v15: a press of the primary button on a control is the control's; one
    /// beside it, in the gap between two controls or with another button is not.
    #[test]
    fn a_press_on_a_control_is_not_the_title_bars() {
        let run_widget = Place {
            x: 1000.,
            y: 5.,
            w: 120.,
            h: 30.,
        };
        // On the widget, on its first and before its last pixel.
        assert!(press_on_control(run_widget, 1000., 5., true));
        assert!(press_on_control(run_widget, 1060., 20., true));
        assert!(press_on_control(run_widget, 1119.9, 34.9, true));
        // Beside it: the gap to the next button, the empty title bar, under the toolbar.
        assert!(!press_on_control(run_widget, 1121., 20., true));
        assert!(!press_on_control(run_widget, 600., 20., true));
        assert!(!press_on_control(run_widget, 1060., 36., true));
        assert!(!press_on_control(run_widget, 1060., 4.9, true));
        // The secondary button opens a menu and is no click the system counts.
        assert!(!press_on_control(run_widget, 1060., 20., false));
    }
}
