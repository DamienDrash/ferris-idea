//! Typed text for lists that narrow as one types (`SPEC-0004` v6 behaviour 17, as `SPEC-0105` v3
//! behaviour 3a for the settings search): the text the platform's text input commits, not the
//! key behind it. A key event names the key the layout puts at its key code; synthetic input
//! with a Unicode string, dead keys and input methods commit other text than that (on macOS
//! synthetic input arrives with key code 0, which is `a`). A list leaves printable keys to the
//! text input and takes the committed text through [`committed_text_input!`].

use gpui::{App, Bounds, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Pixels};
use gpui::{IntoElement, Styled, canvas};

/// Whether `text`, committed by the text input, is text a list's filter takes: not empty and no
/// control characters (Enter, Tab, Escape and ⌫ are the list's keys).
pub fn filter_text(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

/// Whether a key event with `key_char` and these modifiers is left to the text input: it types
/// something and is no shortcut.
pub fn leave_to_text_input(key_char: Option<&str>, command: bool) -> bool {
    !command && key_char.is_some_and(filter_text)
}

/// An element filling its parent that gives the window's text input to `view` while `focus` is
/// focused.
pub fn input_area<V: EntityInputHandler>(focus: &FocusHandle, view: Entity<V>) -> impl IntoElement {
    let focus = focus.clone();
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, cx: &mut App| {
            window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
        },
    )
    .absolute()
    .size_full()
}

/// Implements [`EntityInputHandler`] for a list `$ty` whose method `$take(&mut self, &str,
/// &mut Context<Self>)` takes committed text; composition is shown by the input method's own
/// window and only the commit counts.
macro_rules! committed_text_input {
    ($ty:ty, $take:ident) => {
        impl gpui::EntityInputHandler for $ty {
            fn text_for_range(
                &mut self,
                _range: std::ops::Range<usize>,
                _actual: &mut Option<std::ops::Range<usize>>,
                _window: &mut gpui::Window,
                _cx: &mut gpui::Context<Self>,
            ) -> Option<String> {
                None
            }

            fn selected_text_range(
                &mut self,
                _ignore_disabled_input: bool,
                _window: &mut gpui::Window,
                _cx: &mut gpui::Context<Self>,
            ) -> Option<gpui::UTF16Selection> {
                Some(gpui::UTF16Selection {
                    range: 0..0,
                    reversed: false,
                })
            }

            fn marked_text_range(
                &self,
                _window: &mut gpui::Window,
                _cx: &mut gpui::Context<Self>,
            ) -> Option<std::ops::Range<usize>> {
                None
            }

            fn unmark_text(&mut self, _window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) {}

            fn replace_text_in_range(
                &mut self,
                _range: Option<std::ops::Range<usize>>,
                text: &str,
                _window: &mut gpui::Window,
                cx: &mut gpui::Context<Self>,
            ) {
                if $crate::committed_text::filter_text(text) {
                    self.$take(text, cx);
                }
            }

            fn replace_and_mark_text_in_range(
                &mut self,
                _range: Option<std::ops::Range<usize>>,
                _text: &str,
                _new_selected_range: Option<std::ops::Range<usize>>,
                _window: &mut gpui::Window,
                _cx: &mut gpui::Context<Self>,
            ) {
            }

            fn bounds_for_range(
                &mut self,
                _range_utf16: std::ops::Range<usize>,
                element_bounds: gpui::Bounds<gpui::Pixels>,
                _window: &mut gpui::Window,
                _cx: &mut gpui::Context<Self>,
            ) -> Option<gpui::Bounds<gpui::Pixels>> {
                Some(element_bounds)
            }

            fn character_index_for_point(
                &mut self,
                _point: gpui::Point<gpui::Pixels>,
                _window: &mut gpui::Window,
                _cx: &mut gpui::Context<Self>,
            ) -> Option<usize> {
                None
            }
        }
    };
}
pub(crate) use committed_text_input;

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0004 v6 AK-11: committed text goes into a filter, control characters stay keys; a
    /// printable key without ⌘/⌃ is left to the text input.
    #[test]
    fn committed_text_goes_into_the_filter() {
        for text in ["H", "html", "ü", "äö", " ", "@"] {
            assert!(filter_text(text), "{text:?}");
        }
        for text in ["", "\n", "\r", "\t", "\u{1b}", "\u{7f}", "\u{8}"] {
            assert!(!filter_text(text), "{text:?}");
        }
        assert!(leave_to_text_input(Some("a"), false));
        assert!(!leave_to_text_input(Some("a"), true), "⌘A stays a shortcut");
        assert!(!leave_to_text_input(Some("\n"), false));
        assert!(!leave_to_text_input(None, false));
    }
}
