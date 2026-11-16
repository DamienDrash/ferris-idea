//! Bytes a terminal sends for mouse events (`SPEC-0118` behaviour 1): xterm's reporting modes and
//! encodings.

use crate::screen::{MouseEncoding, MouseMode};

/// The button of a mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    /// Primary button.
    Left,
    /// Middle button or wheel click.
    Middle,
    /// Secondary button.
    Right,
    /// Wheel turned away from the user.
    WheelUp,
    /// Wheel turned towards the user.
    WheelDown,
    /// No button (motion only).
    None,
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseAction {
    /// A button went down (or the wheel turned).
    Press,
    /// A button went up.
    Release,
    /// The mouse moved; `button` is the one held, if any.
    Move,
}

/// One mouse event on a cell (0-based row and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct MouseInput {
    /// What happened.
    pub action: MouseAction,
    /// Which button.
    pub button: MouseButton,
    /// Row, 0-based.
    pub row: usize,
    /// Column, 0-based.
    pub col: usize,
    /// Shift held.
    #[serde(default)]
    pub shift: bool,
    /// Alt (Meta) held.
    #[serde(default)]
    pub alt: bool,
    /// Control held.
    #[serde(default)]
    pub ctrl: bool,
}

/// The report for `input` under `mode` and `encoding`, or `None` when the mode does not report it
/// or the position cannot be encoded.
pub fn mouse_bytes(
    input: &MouseInput,
    mode: MouseMode,
    encoding: MouseEncoding,
) -> Option<Vec<u8>> {
    use MouseAction::{Move, Press, Release};
    use MouseButton as B;
    let wheel = matches!(input.button, B::WheelUp | B::WheelDown);
    let reported = match (mode, input.action) {
        (MouseMode::Off, _) => false,
        (MouseMode::X10, Press) => !wheel,
        (MouseMode::X10, _) => false,
        (_, Press) => input.button != B::None,
        (_, Release) => !wheel && input.button != B::None,
        (MouseMode::Drag, Move) => input.button != B::None && !wheel,
        (MouseMode::Motion, Move) => !wheel,
        (_, Move) => false,
    };
    if !reported {
        return None;
    }
    let mut code: u32 = match input.button {
        B::Left => 0,
        B::Middle => 1,
        B::Right => 2,
        B::None => 3,
        B::WheelUp => 64,
        B::WheelDown => 65,
    };
    // Without SGR a release does not say which button went up.
    if input.action == Release && encoding != MouseEncoding::Sgr {
        code = 3;
    }
    if input.action == Move {
        code += 32;
    }
    if mode != MouseMode::X10 {
        code += 4 * u32::from(input.shift) + 8 * u32::from(input.alt) + 16 * u32::from(input.ctrl);
    }
    let (x, y) = (input.col as u32 + 1, input.row as u32 + 1);
    match encoding {
        MouseEncoding::Sgr => {
            let end = if input.action == Release { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes())
        }
        MouseEncoding::Default => {
            if x > 223 || y > 223 {
                return None;
            }
            Some(vec![
                0x1b,
                b'[',
                b'M',
                (code + 32) as u8,
                (x + 32) as u8,
                (y + 32) as u8,
            ])
        }
        MouseEncoding::Utf8 => {
            let mut out = b"\x1b[M".to_vec();
            for v in [code + 32, x + 32, y + 32] {
                let ch = char::from_u32(v).filter(|_| v < 2048)?;
                let mut buf = [0; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MouseAction::*;

    fn at(action: MouseAction, button: MouseButton, row: usize, col: usize) -> MouseInput {
        MouseInput {
            action,
            button,
            row,
            col,
            shift: false,
            alt: false,
            ctrl: false,
        }
    }

    fn sgr(input: MouseInput, mode: MouseMode) -> Option<String> {
        mouse_bytes(&input, mode, MouseEncoding::Sgr).map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn modes_decide_what_is_reported() {
        let press = at(Press, MouseButton::Left, 0, 0);
        let drag = at(Move, MouseButton::Left, 1, 1);
        let hover = at(Move, MouseButton::None, 1, 1);
        let release = at(Release, MouseButton::Left, 0, 0);
        let wheel = at(Press, MouseButton::WheelUp, 0, 0);
        assert_eq!(sgr(press, MouseMode::Off), None);
        assert!(sgr(press, MouseMode::X10).is_some());
        assert_eq!(sgr(release, MouseMode::X10), None);
        assert_eq!(sgr(wheel, MouseMode::X10), None);
        assert!(sgr(release, MouseMode::Press).is_some());
        assert!(sgr(wheel, MouseMode::Press).is_some());
        assert_eq!(sgr(drag, MouseMode::Press), None);
        assert!(sgr(drag, MouseMode::Drag).is_some());
        assert_eq!(sgr(hover, MouseMode::Drag), None);
        assert_eq!(
            sgr(hover, MouseMode::Motion).as_deref(),
            Some("\x1b[<35;2;2M")
        );
    }

    #[test]
    fn sgr_keeps_buttons_modifiers_and_release() {
        let mut right = at(Press, MouseButton::Right, 4, 9);
        right.ctrl = true;
        right.shift = true;
        assert_eq!(
            sgr(right, MouseMode::Press).as_deref(),
            Some("\x1b[<22;10;5M")
        );
        right.action = Release;
        assert_eq!(
            sgr(right, MouseMode::Press).as_deref(),
            Some("\x1b[<22;10;5m")
        );
        assert_eq!(
            sgr(at(Press, MouseButton::WheelDown, 0, 300), MouseMode::Press).as_deref(),
            Some("\x1b[<65;301;1M")
        );
        assert_eq!(
            sgr(at(Move, MouseButton::Left, 2, 3), MouseMode::Drag).as_deref(),
            Some("\x1b[<32;4;3M")
        );
    }

    #[test]
    fn default_and_utf8_encodings() {
        let press = at(Press, MouseButton::Left, 0, 0);
        assert_eq!(
            mouse_bytes(&press, MouseMode::Press, MouseEncoding::Default).unwrap(),
            b"\x1b[M\x20\x21\x21"
        );
        let release = at(Release, MouseButton::Middle, 1, 2);
        assert_eq!(
            mouse_bytes(&release, MouseMode::Press, MouseEncoding::Default).unwrap(),
            b"\x1b[M\x23\x23\x22",
            "a release is button 3 without SGR"
        );
        let mut x10 = at(Press, MouseButton::Left, 0, 0);
        x10.ctrl = true;
        assert_eq!(
            mouse_bytes(&x10, MouseMode::X10, MouseEncoding::Default).unwrap(),
            b"\x1b[M\x20\x21\x21",
            "X10 has no modifiers"
        );
        let far = at(Press, MouseButton::Left, 0, 300);
        assert_eq!(
            mouse_bytes(&far, MouseMode::Press, MouseEncoding::Default),
            None
        );
        let utf8 = mouse_bytes(&far, MouseMode::Press, MouseEncoding::Utf8).unwrap();
        assert_eq!(utf8, "\x1b[M\u{20}\u{14d}\u{21}".as_bytes());
    }
}
