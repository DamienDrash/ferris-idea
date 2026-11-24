//! Two quick clicks on a control of the title bar press the control twice (`SPEC-0061` v15
//! behaviour 61). The main toolbar lies in the macOS title bar. AppKit counts the clicks in a
//! title bar itself, with click recognisers on the window's frame view that see every press
//! before the toolkit's view does: two clicks within the double-click time zoom or minimise the
//! window, whatever the toolkit drew under the pointer, and the second click is lost to the
//! resize.
//!
//! The workbench tells of every press on a control of the toolbar while the press is dispatched
//! (`ferris_idea_workbench::title_bar`). The recognisers that wait for more than one click are then
//! switched off and on again, which makes them forget the clicks they counted: the press on the
//! control is never the first or the second click of the title bar's double click. A press
//! beside the controls tells nothing, so a double click on the empty title bar zooms the window
//! as the system does.

thread_local! {
    /// Presses on title-bar controls so far (the window self-test reads it).
    static PRESSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many presses on title-bar controls reached the window so far.
pub fn presses() -> usize {
    PRESSES.with(std::cell::Cell::get)
}

/// A press on a control of the title bar of `window`: the system's count of clicks in the title
/// bar starts anew.
#[cfg(target_os = "macos")]
pub fn pressed(window: &gpui::Window) {
    PRESSES.with(|p| p.set(p.get() + 1));
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // SAFETY: the handle is GPUI's live view of this window, borrowed with the window on the
    // main thread for the length of this call.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    // The frame view: the view around the window's content view.
    let Some(frame) = view
        .window()
        .and_then(|w| w.contentView())
        .and_then(|c| unsafe { c.superview() })
    else {
        return;
    };
    // SAFETY: `gestureRecognizers` is an array of `NSGestureRecognizer`; `numberOfClicksRequired`
    // is asked of click recognisers only, `enabled` is a property of every recogniser.
    unsafe {
        let recognisers: Retained<AnyObject> = msg_send![&*frame, gestureRecognizers];
        let count: usize = msg_send![&*recognisers, count];
        for i in 0..count {
            let r: Retained<AnyObject> = msg_send![&*recognisers, objectAtIndex: i];
            let click: bool = msg_send![&*r, isKindOfClass: class!(NSClickGestureRecognizer)];
            if !click {
                continue;
            }
            let clicks: isize = msg_send![&*r, numberOfClicksRequired];
            let enabled: bool = msg_send![&*r, isEnabled];
            if clicks > 1 && enabled {
                let _: () = msg_send![&*r, setEnabled: false];
                let _: () = msg_send![&*r, setEnabled: true];
            }
        }
    }
}

/// Other systems draw their own title bar: nothing to do.
#[cfg(not(target_os = "macos"))]
pub fn pressed(_window: &gpui::Window) {
    PRESSES.with(|p| p.set(p.get() + 1));
}
