//! The application icon at run time (`SPEC-0060` v6 behaviour 12, ADR-0018).
//!
//! Started as a bare binary (`ferris-idea ui <folder>`), Ferris IDEA has no app bundle whose icon the
//! system could show, so the Dock would show a generic one. On macOS the brand icon embedded in the
//! binary ([`ferris_idea_ui::brand::APP_ICON_ICNS`]) becomes the application icon once the application
//! runs. On Linux the window's application id (`ferris-idea`, see
//! `ferris_idea_workbench::project_windows::window_options`) lets the desktop find the installed icon
//! through `ferris-idea.desktop`; GPUI offers no call to set a window icon there.

/// Sets the Dock icon from the embedded brand icon. Returns whether it was set; call on the main
/// thread after the application started (inside `Application::run`).
#[cfg(target_os = "macos")]
pub fn install() -> bool {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let data = NSData::with_bytes(ferris_idea_ui::brand::APP_ICON_ICNS);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: called on the main thread (checked by `MainThreadMarker`) with a valid, retained
    // image; the method is only marked unsafe because it also accepts `None`.
    unsafe { app.setApplicationIconImage(Some(&image)) };
    true
}

/// Nothing to set: the desktop takes the icon from `ferris-idea.desktop` by application id.
#[cfg(not(target_os = "macos"))]
pub fn install() -> bool {
    false
}
