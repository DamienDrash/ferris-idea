//! The startup animation's window in the test harness (`SPEC-0355` v2 AK-7): it opens at
//! 480 × 160 pt, lets the start-up work go on without waiting for its own frame for more than a
//! moment, fades and closes once the first window is ready, and never stays past the cap of 1.5 s.
//! Reduce Motion shows the still logo; the setting off shows nothing. The clock is the test
//! executor's; nothing is drawn on a screen and no settings are read or written.

#![cfg(target_os = "macos")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use ferris_idea_workbench::splash_view::{
    SPLASH_SIZE, after_first_frame, close_when_painted, first_window_ready, show, splash_age,
    splash_window,
};
use ferris_idea_workbench::startup_animation::{CAP, FADE, Frame, Mode};
use gpui::{AnyWindowHandle, Render, TestAppContext, Window, div, prelude::*};

/// A stand-in for the first window (project or Welcome).
struct FirstWindow;

impl Render for FirstWindow {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn open(cx: &mut TestAppContext) -> bool {
    cx.update(|cx| splash_window(cx)).is_some()
}

fn frame(cx: &mut TestAppContext) -> Option<Frame> {
    let handle = cx.update(|cx| splash_window(cx))?;
    handle.update(cx, |v, _, cx| v.frame_now(cx)).ok().flatten()
}

fn wait(cx: &mut TestAppContext, by: Duration) {
    cx.executor().advance_clock(by);
    cx.run_until_parked();
}

/// SPEC-0355 v2 AK-7: shown while start-up runs, gone once the first window is ready.
#[gpui::test]
fn the_splash_closes_when_the_first_window_is_ready(cx: &mut TestAppContext) {
    let handle = cx
        .update(|cx| show(Mode::Animated, None, cx))
        .expect("the splash opens");
    let bounds = handle.update(cx, |_, window, _| window.bounds()).unwrap();
    assert_eq!(
        (f32::from(bounds.size.width), f32::from(bounds.size.height)),
        SPLASH_SIZE
    );
    // The rest of the start-up work runs without the splash's own frame holding it back for
    // more than a moment (the test platform draws no frames).
    let rest_ran = Rc::new(Cell::new(false));
    let ran = rest_ran.clone();
    cx.update(|cx| after_first_frame(move |_| ran.set(true), cx));
    wait(cx, ms(60));
    assert!(rest_ran.get(), "the first window is not delayed");

    // The animation runs while the first window is not ready.
    wait(cx, ms(240));
    let drawing = frame(cx).expect("shown");
    assert!(drawing.symbol > 0.0 && drawing.symbol < 1.0, "{drawing:?}");
    assert_eq!(drawing.opacity, 1.0);

    // The first window is ready at 0.3 s: the splash fades and is gone 150 ms later.
    cx.update(first_window_ready);
    wait(cx, ms(75));
    assert!(frame(cx).expect("fading").opacity < 1.0);
    let age = cx
        .update(|cx| splash_age(cx))
        .expect("still open while fading");
    assert!(age < ms(300) + FADE);
    wait(cx, ms(80));
    assert!(!open(cx), "closed after the fade");
    assert!(cx.update(|cx| splash_age(cx)).is_none());
}

/// SPEC-0355 v2 AK-7: without a ready window the splash goes at the cap, the fade included.
#[gpui::test]
fn the_splash_never_stays_past_the_cap(cx: &mut TestAppContext) {
    cx.update(|cx| show(Mode::Animated, None, cx))
        .expect("the splash opens");
    wait(cx, ms(1100));
    assert_eq!(frame(cx), Some(Frame::FINAL), "the horizontal logo rests");
    wait(cx, CAP - ms(1100) - ms(1));
    assert!(open(cx));
    wait(cx, ms(1));
    assert!(!open(cx), "closed at 1.5 s");
    // A first window that becomes ready afterwards finds nothing to close.
    cx.update(first_window_ready);
    cx.run_until_parked();
    assert!(!open(cx));
}

/// SPEC-0355 v2 AK-7: a first window that cannot be reached closes the splash at once; Reduce
/// Motion shows the still logo and closes without a fade; the setting off opens nothing.
#[gpui::test]
fn still_logo_and_no_splash(cx: &mut TestAppContext) {
    // Reduce Motion: the final logo, still, until the first window is ready.
    cx.update(|cx| show(Mode::Still, None, cx)).expect("opens");
    wait(cx, ms(10));
    assert_eq!(frame(cx), Some(Frame::FINAL));
    let first = cx.add_window(|_, _| FirstWindow);
    let first = AnyWindowHandle::from(first);
    // Its frame never comes in the harness; once the window is gone the splash goes at once.
    first
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| close_when_painted(first, cx));
    cx.run_until_parked();
    assert!(!open(cx), "no fade for the still logo");

    // The setting off: nothing opens and the start-up work runs at once.
    assert!(cx.update(|cx| show(Mode::Off, None, cx)).is_none());
    let rest_ran = Rc::new(Cell::new(false));
    let ran = rest_ran.clone();
    cx.update(|cx| after_first_frame(move |_| ran.set(true), cx));
    assert!(rest_ran.get());
    assert!(!open(cx));
}
