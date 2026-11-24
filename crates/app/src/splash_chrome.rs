//! What the startup animation needs from the system (`SPEC-0355` v2 behaviour 23): whether the
//! user asked the system to reduce motion, and a splash window without window buttons. Both need
//! AppKit, which the workbench crate does not call.

/// Whether macOS's Accessibility › Display › Reduce Motion is on.
#[cfg(target_os = "macos")]
pub fn reduce_motion() -> bool {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    let Some(class) = AnyClass::get(c"NSWorkspace") else {
        return false;
    };
    // SAFETY: `sharedWorkspace` is a class method of NSWorkspace that returns the shared,
    // never-nil instance; `accessibilityDisplayShouldReduceMotion` is an instance method without
    // arguments returning BOOL (macOS 10.12 and later). Both may be called on any thread.
    unsafe {
        let workspace: *mut AnyObject = msg_send![class, sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        msg_send![workspace, accessibilityDisplayShouldReduceMotion]
    }
}

/// Other systems: the animation plays (the setting still switches it off).
#[cfg(not(target_os = "macos"))]
pub fn reduce_motion() -> bool {
    false
}

/// Hides the window buttons of the splash window, which has no title bar of its own.
#[cfg(target_os = "macos")]
pub fn hide_buttons(window: &gpui::Window) {
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
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = ns_window.standardWindowButton(kind) {
            button.setHidden(true);
        }
    }
}

/// Other systems: the window has no buttons to hide.
#[cfg(not(target_os = "macos"))]
pub fn hide_buttons(_window: &gpui::Window) {}

#[cfg(test)]
mod tests {
    /// The question reaches the system and answers without failing, whatever the setting is.
    #[test]
    fn reduce_motion_can_be_asked() {
        let first = super::reduce_motion();
        assert_eq!(super::reduce_motion(), first);
    }
}
