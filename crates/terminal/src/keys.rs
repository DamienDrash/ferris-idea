//! Bytes a terminal sends for keys (`SPEC-0062` behaviour 3): xterm conventions, as the terminfo
//! entry `xterm-256color` describes them.

/// Bytes for a non-text key or a key with Control/Alt, as an xterm sends them; `None` for keys that
/// type text without modifiers (they go through the input method) and for keys with no terminal
/// meaning. `key` uses toolkit names (`enter`, `up`, `a`, `f5`).
pub fn key_bytes(
    key: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    // Modifier parameter of xterm: 1 + Shift (1) + Alt (2) + Control (4).
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    // Cursor keys and F1–F4: a final letter, `ESC O x` or `ESC [ x` plain, `ESC [ 1 ; m x` with
    // modifiers (in both cursor key modes).
    let letter = |c: char, ss3: bool| {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{c}")
        } else if ss3 {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
    };
    // Editing keys and F5–F12: `ESC [ n ~`, `ESC [ n ; m ~` with modifiers.
    let tilde = |n: u8| {
        if modifier > 1 {
            format!("\x1b[{n};{modifier}~")
        } else {
            format!("\x1b[{n}~")
        }
    };
    // Keys without a modifier form (modifyOtherKeys is not offered): Alt is an ESC in front.
    let meta = |seq: &str| {
        if alt {
            format!("\x1b{seq}")
        } else {
            seq.to_owned()
        }
    };
    let seq = match key {
        "enter" | "return" => meta("\r"),
        "backspace" => meta(if ctrl { "\x08" } else { "\x7f" }),
        "tab" => meta(if shift { "\x1b[Z" } else { "\t" }),
        "escape" => meta("\x1b"),
        "up" => letter('A', app_cursor),
        "down" => letter('B', app_cursor),
        "right" => letter('C', app_cursor),
        "left" => letter('D', app_cursor),
        "home" => letter('H', app_cursor),
        "end" => letter('F', app_cursor),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => letter('P', true),
        "f2" => letter('Q', true),
        "f3" => letter('R', true),
        "f4" => letter('S', true),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        _ => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if ctrl {
                // Control letters and the usual symbols: @ [ \ ] ^ _ ? and space, and the
                // digits and `/` an xterm maps to them.
                let byte = match c.to_ascii_lowercase() {
                    'a'..='z' => c.to_ascii_lowercase() as u8 - b'a' + 1,
                    ' ' | '@' | '2' => 0,
                    '[' | '3' => 0x1b,
                    '\\' | '4' => 0x1c,
                    ']' | '5' => 0x1d,
                    '^' | '6' => 0x1e,
                    '_' | '-' | '7' | '/' => 0x1f,
                    '?' | '8' => 0x7f,
                    _ => return None,
                };
                let mut out = Vec::new();
                if alt {
                    out.push(0x1b);
                }
                out.push(byte);
                return Some(out);
            }
            if alt {
                let ch = if shift { c.to_ascii_uppercase() } else { c };
                let mut out = vec![0x1b];
                out.extend(ch.to_string().as_bytes());
                return Some(out);
            }
            return None;
        }
    };
    Some(seq.into_bytes())
}

/// Text to send for a paste: line breaks as CR, wrapped when the program asked for bracketed paste.
///
/// Control characters other than tab and the line ends are left out (`SPEC-0062` behaviour 3):
/// pasted text is text, not key presses, and an escape character in it could end the bracketed
/// paste early (`ESC [201~`) so that the rest runs as typed commands.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let body: String = text
        .replace("\r\n", "\r")
        .replace('\n', "\r")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\t' | '\r'))
        .collect();
    if bracketed {
        format!("\x1b[200~{body}\x1b[201~").into_bytes()
    } else {
        body.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(key: &str, ctrl: bool, alt: bool, shift: bool, app: bool) -> Option<Vec<u8>> {
        key_bytes(key, ctrl, alt, shift, app)
    }

    #[test]
    fn special_keys_and_modifiers() {
        assert_eq!(k("enter", false, false, false, false), Some(b"\r".to_vec()));
        assert_eq!(k("backspace", false, false, false, false), Some(vec![0x7f]));
        assert_eq!(
            k("up", false, false, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(k("up", false, false, false, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            k("right", true, false, false, false),
            Some(b"\x1b[1;5C".to_vec())
        );
        assert_eq!(
            k("left", false, true, false, false),
            Some(b"\x1b[1;3D".to_vec())
        );
        assert_eq!(
            k("tab", false, false, true, false),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            k("f5", false, false, false, false),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(k("c", true, false, false, false), Some(vec![3]));
        assert_eq!(k("d", true, false, false, false), Some(vec![4]));
        assert_eq!(k("[", true, false, false, false), Some(vec![0x1b]));
        assert_eq!(k("b", false, true, false, false), Some(b"\x1bb".to_vec()));
        assert_eq!(
            k("a", false, false, false, false),
            None,
            "typed through the input method"
        );
        assert_eq!(k("ä", false, false, false, false), None);
    }

    #[test]
    fn paste() {
        assert_eq!(paste_bytes("a\nb", false), b"a\rb".to_vec());
        assert_eq!(paste_bytes("x", true), b"\x1b[200~x\x1b[201~".to_vec());
    }

    #[test]
    fn a_pasted_line_end_stays_inside_the_bracketed_paste_marks() {
        // With mode 2004 on (zsh) the shell gets the line end as pasted text, not as a Return
        // pressed after the paste: the command waits on the prompt (SPEC-0118 behaviour 4).
        let sent = paste_bytes("echo pasted-$((2+3))\n", true);
        assert_eq!(sent, b"\x1b[200~echo pasted-$((2+3))\r\x1b[201~".to_vec());
        assert!(sent.ends_with(b"\x1b[201~"));
        // Without the mode the line end is a Return and the shell runs the line.
        assert_eq!(
            paste_bytes("echo pasted-$((2+3))\n", false),
            b"echo pasted-$((2+3))\r".to_vec()
        );
    }
}
