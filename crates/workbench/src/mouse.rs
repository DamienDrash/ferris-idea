//! Secondary clicks on GPUI elements (`SPEC-0275` behaviour 2): the right button, a two-finger
//! click on a trackpad and, on macOS, a Control-click all open the context menu.
//!
//! GPUI 0.2 on macOS already turns a Control-click into a right click before elements see it and
//! reports the trackpad's secondary click as the right button; the primary-button handler here
//! covers a Control-click that reaches the window untranslated (events posted by other programs).

use std::rc::Rc;

use gpui::{App, InteractiveElement, MouseButton, MouseDownEvent, Window};

use crate::context_menu::{Button, is_secondary_click};

/// The toolkit's button in the menu model's terms.
pub fn button(b: MouseButton) -> Button {
    match b {
        MouseButton::Left => Button::Primary,
        MouseButton::Right => Button::Secondary,
        _ => Button::Other,
    }
}

/// Whether `e` opens a context menu.
pub fn is_secondary(e: &MouseDownEvent) -> bool {
    is_secondary_click(
        button(e.button),
        e.modifiers.control,
        cfg!(target_os = "macos"),
    )
}

/// Runs `f` on a secondary click on `el`; the click goes no further.
pub fn on_secondary_click<E: InteractiveElement>(
    el: E,
    f: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> E {
    let f = Rc::new(f);
    let g = f.clone();
    el.on_mouse_down(MouseButton::Right, move |e, window, cx| {
        cx.stop_propagation();
        f(e, window, cx)
    })
    .on_mouse_down(MouseButton::Left, move |e, window, cx| {
        if is_secondary(e) {
            cx.stop_propagation();
            g(e, window, cx)
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::context_menu::{Button, is_secondary_click};

    /// SPEC-0275 v20 AK-44: on the Mac a Control-click with the primary button is a secondary
    /// click, so the editor's primary handler leaves it to the menu; elsewhere Control-click stays
    /// a primary click (Go to Declaration).
    #[test]
    fn a_control_click_is_no_primary_click_on_the_mac() {
        assert!(is_secondary_click(Button::Primary, true, true));
        assert!(!is_secondary_click(Button::Primary, true, false));
        assert!(!is_secondary_click(Button::Primary, false, true));
        assert!(is_secondary_click(Button::Secondary, false, false));
    }
}
