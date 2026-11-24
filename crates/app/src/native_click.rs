//! Real mouse clicks for the window self-test (`SPEC-0275` behaviour 12, `SPEC-0319` behaviours 20a
//! and 20c).
//!
//! On macOS the clicks are native `NSEvent`s posted to the application's event queue, so they take
//! the same way as a click of the user: the window, the toolkit's view (`rightMouseDown:`,
//! `mouseDown:` with Control held) and the toolkit's translation into its own events. Nothing
//! leaves the process: the events are posted to this application only, not to the system.
//!
//! On Linux (X11 and Wayland) the clicks are the toolkit's own pointer events, handed to the
//! window's input handler — the handler gpui's X11 and Wayland layers call for a real click — after
//! the current frame, as the platform would deliver them. Nothing goes to the X server or the
//! compositor: no XTEST, no event sent through the X server, no virtual pointer, so no other client
//! and no other window can receive them (UISEC-9). The hook is
//! `Window::dispatch_in_process_pointer_input` of the vendored gpui (`vendor/gpui/FERRIS_IDEA.md`),
//! called in this file only.

/// The kind of click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// The primary button, `count` times in a row (2: a double click).
    Left(usize),
    /// The secondary button (right button, or a two-finger click on a trackpad).
    Right,
    /// The primary button with Control held: a secondary click on macOS.
    ControlLeft,
}

/// Posts `click` at `x`, `y` (window coordinates with the origin at the top left of the content)
/// to the key window, or the first window. Returns whether the events were posted.
#[cfg(target_os = "macos")]
pub fn post(_window: &mut gpui::Window, _cx: &mut gpui::App, x: f32, y: f32, click: Click) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::NSPoint;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(window) = app
        .keyWindow()
        .or_else(|| app.mainWindow())
        .or_else(|| app.windows().firstObject())
    else {
        return false;
    };
    let Some(content) = window.contentView() else {
        return false;
    };
    // AppKit counts from the bottom left.
    let at = NSPoint::new(f64::from(x), content.frame().size.height - f64::from(y));
    let number = window.windowNumber();
    let (down, up, flags, count) = match click {
        Click::Left(n) => (
            NSEventType::LeftMouseDown,
            NSEventType::LeftMouseUp,
            NSEventModifierFlags::empty(),
            n.max(1),
        ),
        Click::Right => (
            NSEventType::RightMouseDown,
            NSEventType::RightMouseUp,
            NSEventModifierFlags::empty(),
            1,
        ),
        Click::ControlLeft => (
            NSEventType::LeftMouseDown,
            NSEventType::LeftMouseUp,
            NSEventModifierFlags::Control,
            1,
        ),
    };
    let event = |kind: NSEventType, clicks: usize| {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            at,
            flags,
            0.0,
            number,
            None,
            0,
            clicks as isize,
            1.0,
        )
    };
    let button = i64::from(click == Click::Right);
    for clicks in 1..=count {
        for kind in [down, up] {
            let Some(e) = event(kind, clicks) else {
                return false;
            };
            // An event made by `mouseEventWithType` reports button number 0 whatever its type;
            // the hardware sends 1 for the secondary button, so it is set the way it would be.
            let Some(e) = with_button_number(&e, button) else {
                return false;
            };
            app.postEvent_atStart(&e, false);
        }
    }
    true
}

/// A copy of `event` with the button number `button`, as the hardware sets it.
#[cfg(target_os = "macos")]
fn with_button_number(
    event: &objc2_app_kit::NSEvent,
    button: i64,
) -> Option<objc2::rc::Retained<objc2_app_kit::NSEvent>> {
    use objc2::encode::{Encoding, RefEncode};
    use objc2::runtime::AnyClass;
    use std::ffi::c_void;

    /// The opaque `CGEvent`, typed so the runtime's type check of the message send passes.
    #[repr(C)]
    struct CgEvent([u8; 0]);
    // SAFETY: the encoding of `CGEventRef` (`^{__CGEvent=}`).
    unsafe impl RefEncode for CgEvent {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateCopy(event: *const c_void) -> *mut c_void;
        fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
        fn CFRelease(cf: *const c_void);
    }
    /// `kCGMouseEventButtonNumber`.
    const BUTTON_NUMBER: u32 = 3;
    // SAFETY: `CGEvent` returns the event's own CGEventRef (not retained, valid while `event`
    // lives); the copy is ours and released after `eventWithCGEvent:` retained what it needs.
    unsafe {
        let cg: *const CgEvent = objc2::msg_send![event, CGEvent];
        if cg.is_null() {
            return None;
        }
        let copy = CGEventCreateCopy(cg.cast());
        if copy.is_null() {
            return None;
        }
        CGEventSetIntegerValueField(copy, BUTTON_NUMBER, button);
        let class = AnyClass::get(c"NSEvent")?;
        let made: Option<objc2::rc::Retained<objc2_app_kit::NSEvent>> =
            objc2::msg_send![class, eventWithCGEvent: copy.cast::<CgEvent>().cast_const()];
        CFRelease(copy);
        made
    }
}

/// Delivers `click` at `x`, `y` (window coordinates, origin at the top left of the content) into
/// `window` after the current frame, through the window's own input handler (module comment).
/// Returns whether the events were queued.
#[cfg(not(target_os = "macos"))]
pub fn post(window: &mut gpui::Window, cx: &mut gpui::App, x: f32, y: f32, click: Click) -> bool {
    deliver_later(window, cx, pointer_inputs(x, y, Some(click)));
    true
}

/// The toolkit events a click (or, with `None`, a pointer coming to rest) at `x`, `y` consists of,
/// in the order gpui's X11 and Wayland layers hand them to the window for real input: the pointer
/// moves there, then each press and release with its click count (a double click is a press and
/// release with count 1, then with count 2). A Control-click carries Control on its events and
/// ends with a move without it, as releasing the key would leave the window's modifiers.
pub fn pointer_inputs(x: f32, y: f32, click: Option<Click>) -> Vec<gpui::PlatformInput> {
    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, point,
        px,
    };
    let position = point(px(x), px(y));
    let rest = |modifiers: Modifiers| {
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        })
    };
    let Some(click) = click else {
        return vec![rest(Modifiers::default())];
    };
    let (button, count, modifiers) = match click {
        Click::Left(n) => (MouseButton::Left, n.max(1), Modifiers::default()),
        Click::Right => (MouseButton::Right, 1, Modifiers::default()),
        Click::ControlLeft => (
            MouseButton::Left,
            1,
            Modifiers {
                control: true,
                ..Modifiers::default()
            },
        ),
    };
    let mut events = vec![rest(modifiers)];
    for click_count in 1..=count {
        events.push(PlatformInput::MouseDown(MouseDownEvent {
            button,
            position,
            modifiers,
            click_count,
            first_mouse: false,
        }));
        events.push(PlatformInput::MouseUp(MouseUpEvent {
            button,
            position,
            modifiers,
            click_count,
        }));
    }
    if modifiers != Modifiers::default() {
        events.push(rest(Modifiers::default()));
    }
    events
}

/// The window level of the UI driver for pointer actions (`SPEC-0319` v7 behaviour 16b), on every
/// platform: the toolkit events of a click (or, with `None`, the pointer coming to rest) at `x`,
/// `y` handed to `window`'s own input handler once the current frame is done, marked as
/// synthesised so they never count as the user's input. Nothing reaches the system's event queue,
/// the display server or another application.
pub fn deliver_in_process(
    window: &mut gpui::Window,
    cx: &mut gpui::App,
    x: f32,
    y: f32,
    click: Option<Click>,
) {
    let events = pointer_inputs(x, y, click);
    window.defer(cx, move |window, cx| {
        ferris_idea_workbench::semantics::synthesizing(|| {
            for event in events {
                window.dispatch_in_process_pointer_input(event, cx);
            }
        });
        window.refresh();
    });
}

/// Hands `events` to `window`'s input handler once the current frame is done (a click inside a
/// frame would run the window's handlers while it draws), then asks for a frame, as a platform
/// event does.
#[cfg(not(target_os = "macos"))]
fn deliver_later(window: &mut gpui::Window, cx: &mut gpui::App, events: Vec<gpui::PlatformInput>) {
    window.defer(cx, move |window, cx| {
        for event in events {
            window.dispatch_in_process_pointer_input(event, cx);
        }
        window.refresh();
    });
}

/// Posts a pointer move to `x`, `y` (window coordinates, origin at the top left of the content) to
/// the key window, or the first window, as the hardware sends it when the pointer comes to rest
/// there (`SPEC-0294` AK-7). Returns whether the event was posted.
#[cfg(target_os = "macos")]
pub fn post_move(_window: &mut gpui::Window, _cx: &mut gpui::App, x: f32, y: f32) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::NSPoint;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(window) = app
        .keyWindow()
        .or_else(|| app.mainWindow())
        .or_else(|| app.windows().firstObject())
    else {
        return false;
    };
    let Some(content) = window.contentView() else {
        return false;
    };
    let at = NSPoint::new(f64::from(x), content.frame().size.height - f64::from(y));
    let Some(event) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::MouseMoved,
        at,
        NSEventModifierFlags::empty(),
        0.0,
        window.windowNumber(),
        None,
        0,
        0,
        0.0,
    ) else {
        return false;
    };
    app.postEvent_atStart(&event, false);
    true
}

/// Delivers a pointer move to `x`, `y` into `window` after the current frame, as gpui's X11 and
/// Wayland layers deliver the pointer coming to rest there (`SPEC-0294` AK-7). Returns whether the
/// event was queued.
#[cfg(not(target_os = "macos"))]
pub fn post_move(window: &mut gpui::Window, cx: &mut gpui::App, x: f32, y: f32) -> bool {
    deliver_later(window, cx, pointer_inputs(x, y, None));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, MouseButton, PlatformInput};

    /// A short form of an event: kind, button, click count, Control held.
    fn shape(e: &PlatformInput) -> (&'static str, Option<MouseButton>, usize, bool) {
        match e {
            PlatformInput::MouseMove(m) => ("move", None, 0, m.modifiers.control),
            PlatformInput::MouseDown(d) => {
                ("down", Some(d.button), d.click_count, d.modifiers.control)
            }
            PlatformInput::MouseUp(u) => ("up", Some(u.button), u.click_count, u.modifiers.control),
            _ => ("other", None, 0, false),
        }
    }

    #[test]
    fn a_double_click_is_a_move_then_two_presses_with_rising_counts() {
        let events = pointer_inputs(10., 20., Some(Click::Left(2)));
        let shapes: Vec<_> = events.iter().map(shape).collect();
        let left = Some(MouseButton::Left);
        assert_eq!(
            shapes,
            [
                ("move", None, 0, false),
                ("down", left, 1, false),
                ("up", left, 1, false),
                ("down", left, 2, false),
                ("up", left, 2, false),
            ]
        );
        for e in &events {
            let at = match e {
                PlatformInput::MouseMove(m) => m.position,
                PlatformInput::MouseDown(d) => d.position,
                PlatformInput::MouseUp(u) => u.position,
                _ => unreachable!(),
            };
            assert_eq!(at, gpui::point(gpui::px(10.), gpui::px(20.)));
        }
    }

    #[test]
    fn a_secondary_and_a_control_click_and_a_hover() {
        let right = Some(MouseButton::Right);
        let shapes: Vec<_> = pointer_inputs(1., 1., Some(Click::Right))
            .iter()
            .map(shape)
            .collect();
        assert_eq!(
            shapes,
            [
                ("move", None, 0, false),
                ("down", right, 1, false),
                ("up", right, 1, false)
            ]
        );
        let left = Some(MouseButton::Left);
        let shapes: Vec<_> = pointer_inputs(1., 1., Some(Click::ControlLeft))
            .iter()
            .map(shape)
            .collect();
        // Control on the click, and the window's modifiers are clear again afterwards.
        assert_eq!(
            shapes,
            [
                ("move", None, 0, true),
                ("down", left, 1, true),
                ("up", left, 1, true),
                ("move", None, 0, false),
            ]
        );
        let hover = pointer_inputs(3., 4., None);
        assert!(matches!(
            hover.as_slice(),
            [PlatformInput::MouseMove(m)] if m.pressed_button.is_none() && m.modifiers == Modifiers::default()
        ));
    }

    /// UISEC-9 on Linux, live on an X server (`xvfb-run cargo test -p ferris-idea --bin ferris-idea
    /// native_click`); returns early without an X11 session. A gpui window of this process gets the
    /// clicks `post` delivers, while an observer with its own X connection — a window under the
    /// whole screen that takes button and motion events, and the raw input of every device on the
    /// root window (XInput 2.2, which XTEST input and every real device produce) — sees no input,
    /// and the pointer of the display has not moved.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_click_reaches_this_window_and_no_other_x_client() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::time::Duration;

        use gpui::{
            AppContext as _, Bounds, Context, InteractiveElement as _, IntoElement,
            ParentElement as _, Render, Styled as _, Window, WindowBounds, WindowOptions, div,
            point, px, size,
        };
        use x11rb::connection::Connection;
        use x11rb::protocol::Event;
        use x11rb::protocol::xinput::{self, ConnectionExt as _};
        use x11rb::protocol::xproto::{
            ConnectionExt as _, CreateWindowAux, EventMask, WindowClass,
        };
        use x11rb::wrapper::ConnectionExt as _;

        if !ferris_idea_workbench::x11_wake::uses_x11(
            std::env::var_os("DISPLAY").as_deref(),
            std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        ) {
            eprintln!("skipped: no X11 session");
            return;
        }

        // The observer: another client of the same display.
        let (obs, screen) = x11rb::connect(None).expect("X connection");
        let setup = obs.setup().roots[screen].clone();
        let root = setup.root;
        let version = obs.xinput_xi_query_version(2, 2).unwrap().reply().unwrap();
        assert!(
            (version.major_version, version.minor_version) >= (2, 2),
            "XInput 2.2"
        );
        let raw = xinput::XIEventMask::RAW_BUTTON_PRESS
            | xinput::XIEventMask::RAW_BUTTON_RELEASE
            | xinput::XIEventMask::RAW_MOTION;
        obs.xinput_xi_select_events(
            root,
            &[xinput::EventMask {
                deviceid: xinput::Device::ALL.into(),
                mask: vec![raw],
            }],
        )
        .unwrap();
        let under = obs.generate_id().unwrap();
        obs.create_window(
            0,
            under,
            root,
            0,
            0,
            setup.width_in_pixels,
            setup.height_in_pixels,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            ),
        )
        .unwrap();
        obs.map_window(under).unwrap();
        obs.sync().unwrap();
        while obs.poll_for_event().unwrap().is_some() {}
        let pointer = |c: &x11rb::rust_connection::RustConnection| {
            let p = c.query_pointer(root).unwrap().reply().unwrap();
            (p.root_x, p.root_y, p.mask)
        };
        let before = pointer(&obs);

        // This process's window, which records what reaches it.
        struct Probe(Rc<RefCell<Vec<String>>>);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let seen = |what: &'static str, log: &Rc<RefCell<Vec<String>>>| {
                    let log = log.clone();
                    move |e: &gpui::MouseDownEvent, _: &mut Window, _: &mut gpui::App| {
                        log.borrow_mut().push(format!("{what} {}", e.click_count));
                    }
                };
                let up = self.0.clone();
                div()
                    .size_full()
                    .on_mouse_down(MouseButton::Left, seen("left", &self.0))
                    .on_mouse_down(MouseButton::Right, seen("right", &self.0))
                    .on_mouse_up(MouseButton::Left, move |e, _, _| {
                        up.borrow_mut().push(format!("up {}", e.click_count));
                    })
                    .child("probe")
            }
        }
        let log = Rc::new(RefCell::new(Vec::new()));
        let in_app = log.clone();
        gpui::Application::new().run(move |cx| {
            let bounds = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.)));
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..WindowOptions::default()
                },
                move |window, cx| {
                    ferris_idea_workbench::x11_wake::until_first_frame(window);
                    window.on_next_frame(|window, cx| {
                        assert!(post(window, cx, 50., 50., Click::Left(2)));
                        assert!(post(window, cx, 60., 60., Click::Right));
                        assert!(post_move(window, cx, 70., 70.));
                        // The frame after the delivery, and one more, then the end.
                        window.on_next_frame(|window, _| {
                            window.on_next_frame(|_, cx| cx.quit());
                            window.refresh();
                        });
                    });
                    cx.new(|_| Probe(in_app))
                },
            )
            .expect("window");
            // A run that hangs fails the test instead of holding it.
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        });

        assert_eq!(
            *log.borrow(),
            ["left 1", "up 1", "left 2", "up 2", "right 1"],
            "the clicks reach this process's window"
        );
        obs.sync().unwrap();
        let mut input = Vec::new();
        while let Some(e) = obs.poll_for_event().unwrap() {
            if matches!(
                e,
                Event::ButtonPress(_)
                    | Event::ButtonRelease(_)
                    | Event::MotionNotify(_)
                    | Event::XinputRawButtonPress(_)
                    | Event::XinputRawButtonRelease(_)
                    | Event::XinputRawMotion(_)
            ) {
                input.push(format!("{e:?}"));
            }
        }
        assert!(input.is_empty(), "input reached the X server: {input:?}");
        assert_eq!(
            pointer(&obs),
            before,
            "the display's pointer stays where it was"
        );
    }
}
