//! The macOS window buttons ("traffic lights") of a workbench window (`SPEC-0061` v7 behaviour
//! 44). GPUI 0.2.2 places the close button (`TitlebarOptions::traffic_light_position`) and keeps
//! the pitch AppKit gives the three buttons, 23 pt on macOS 27; the reference has 20 pt. The
//! application registers a function that sets the minimize and zoom buttons
//! [`TRAFFIC_LIGHT_PITCH`] after the button before them (it needs AppKit, which this crate does
//! not call); GPUI keeps a pitch it finds when it moves the buttons again after a resize.

use std::sync::OnceLock;

pub use idecraft_ui::metrics::{TRAFFIC_LIGHT_PITCH, TRAFFIC_LIGHT_POSITION};

/// Places a window's buttons; set once by the application.
pub type Spacer = fn(&gpui::Window, f32);

static SPACER: OnceLock<Spacer> = OnceLock::new();

/// Registers the function that places the buttons (the application does at start).
pub fn register(spacer: Spacer) {
    let _ = SPACER.set(spacer);
}

/// Places the minimize and zoom buttons of `window` [`TRAFFIC_LIGHT_PITCH`] apart, if the
/// application registered how.
pub fn space(window: &gpui::Window) {
    if let Some(spacer) = SPACER.get() {
        spacer(window, TRAFFIC_LIGHT_PITCH);
    }
}
