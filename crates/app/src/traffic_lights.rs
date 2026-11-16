//! The macOS window buttons 20 pt apart (`SPEC-0061` v7 behaviour 44): GPUI 0.2.2 sets only the
//! close button's position and keeps AppKit's pitch (23 pt on macOS 27). This moves the minimize
//! and zoom buttons onto the close button's line, `pitch` after the button before them; the
//! workbench calls it when a window opens and after its frame or activation changed.

/// Places the minimize and zoom buttons of `window` `pitch` points apart.
#[cfg(target_os = "macos")]
pub fn space(window: &gpui::Window, pitch: f32) {
    use objc2_app_kit::{NSView, NSWindowButton};
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
    let Some(ns_window) = view.window() else {
        return;
    };
    let Some(close) = ns_window.standardWindowButton(NSWindowButton::CloseButton) else {
        return;
    };
    let start = close.frame().origin;
    for (i, kind) in [
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(button) = ns_window.standardWindowButton(kind) {
            let mut origin = button.frame().origin;
            origin.x = start.x + f64::from(pitch) * (i as f64 + 1.0);
            origin.y = start.y;
            if origin != button.frame().origin {
                button.setFrameOrigin(origin);
            }
        }
    }
}

/// Other systems draw their own window buttons.
#[cfg(not(target_os = "macos"))]
pub fn space(_window: &gpui::Window, _pitch: f32) {}
