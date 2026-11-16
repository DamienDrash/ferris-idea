//! Keyboard diagnostics (`SPEC-0270`): while the mode is on, every keystroke the window receives is
//! shown and logged as it arrived — key, character, modifiers, keyboard layout, the key a layout key
//! stands in for, the keymap's keystroke and actions, where the focus was and which action ran.
//! Free of the UI toolkit except for the plain values the window hands in.

use std::collections::VecDeque;
use std::sync::Mutex;

/// Rows the window shows.
pub const SHOWN: usize = 12;

/// One keystroke or change of the held modifier keys, as received.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyRecord {
    /// When, UTC (`2026-11-04T10:15:03.123Z`).
    pub time: String,
    /// `down`, `repeat` (key held) or `modifiers` (only modifier keys changed).
    pub event: &'static str,
    /// The toolkit's key name, e.g. `g`, `enter`, `f7`; hidden for typed text.
    pub key: String,
    /// The character the key types with these modifiers, if any; hidden for typed text.
    pub key_char: Option<String>,
    /// Held modifiers, `⌃⌥⇧⌘` and `fn` in that order (macOS names on every platform).
    pub modifiers: String,
    /// The keyboard layout's name and id.
    pub layout: String,
    /// The US key the layout's key stands in for when a shortcut is moved to the key in the same
    /// place (`SPEC-0071` v4 behaviour 6), if it differs.
    pub layout_key: Option<String>,
    /// The keystroke in keymap notation that was looked up, e.g. `control G`.
    pub stroke: Option<String>,
    /// The actions the keymap binds to it, in order.
    pub bound: Vec<String>,
    /// The action that ran and who ran it (`editor`, `workbench`, `menu bar`, …).
    pub ran: Option<(String, String)>,
    /// What had the keyboard focus: `editor`, `project tree`, `popup` or `other`.
    pub focus: String,
}

/// The modifiers in macOS notation.
pub fn modifiers(control: bool, alt: bool, shift: bool, platform: bool, function: bool) -> String {
    let mut out = String::new();
    for (on, sign) in [
        (control, "⌃"),
        (alt, "⌥"),
        (shift, "⇧"),
        (platform, "⌘"),
        (function, "fn"),
    ] {
        if on {
            out.push_str(sign);
        }
    }
    out
}

/// Whether a key is typed text rather than a shortcut: no ⌃, ⌘ or fn, a printable character, and
/// no keymap action. Its key and character are not shown or logged, so that the log never holds
/// what was typed, e.g. a password (behaviour 4).
pub fn is_typing(record: &KeyRecord) -> bool {
    let shortcut_modifier = ["⌃", "⌘", "fn"]
        .iter()
        .any(|m| record.modifiers.contains(m));
    let printable = record
        .key_char
        .as_deref()
        .is_some_and(|c| !c.is_empty() && c.chars().all(|ch| !ch.is_control()));
    !shortcut_modifier && printable && record.bound.is_empty()
}

impl KeyRecord {
    /// Hides key and character of typed text (behaviour 4).
    pub fn redacted(mut self) -> Self {
        if is_typing(&self) {
            self.key = "(typed)".into();
            self.key_char = Some("•".into());
        }
        self
    }

    /// The action column: the action and who ran it, `none`, or for typed text that it went to the
    /// text input.
    fn outcome(&self) -> String {
        match &self.ran {
            Some((id, by)) => format!("{id} ({by})"),
            None if self.event == "modifiers" => "-".into(),
            None if self.key == "(typed)" => "none (text input)".into(),
            None => "none".into(),
        }
    }

    /// One log line: tab-separated `name=value` fields after time and event (behaviour 3).
    pub fn line(&self) -> String {
        let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
        let char_shown = self
            .key_char
            .as_deref()
            .map(|c| format!("{c:?}"))
            .unwrap_or_else(|| "-".into());
        [
            self.time.clone(),
            self.event.to_owned(),
            format!("key={}", self.key),
            format!("char={char_shown}"),
            format!(
                "mods={}",
                if self.modifiers.is_empty() {
                    "-"
                } else {
                    &self.modifiers
                }
            ),
            format!("us_key={}", opt(&self.layout_key)),
            format!("stroke={}", opt(&self.stroke)),
            format!(
                "bound={}",
                if self.bound.is_empty() {
                    "-".into()
                } else {
                    self.bound.join(",")
                }
            ),
            format!("ran={}", self.outcome()),
            format!("focus={}", self.focus),
            format!("layout={}", self.layout),
        ]
        .join("\t")
    }

    /// One row of the window: what was pressed and what it did.
    pub fn row(&self) -> String {
        let time = self.time.get(11..23).unwrap_or(&self.time);
        if self.event == "modifiers" {
            let held = if self.modifiers.is_empty() {
                "none"
            } else {
                &self.modifiers
            };
            return format!("{time}  modifiers held: {held}");
        }
        let pressed = format!("{}{}", self.modifiers, self.key);
        let mut row = format!("{time}  {pressed}");
        if let Some(us) = &self.layout_key {
            row.push_str(&format!(" (as {us})"));
        }
        if let Some(stroke) = &self.stroke {
            row.push_str(&format!("  [{stroke}]"));
        }
        row.push_str(&format!("  → {}", self.outcome()));
        if self.ran.is_none() && !self.bound.is_empty() {
            row.push_str(&format!("; bound: {}", self.bound.join(", ")));
        }
        row.push_str(&format!("  · focus {}", self.focus));
        row
    }
}

#[derive(Default)]
struct State {
    on: bool,
    pending: Option<KeyRecord>,
    recent: VecDeque<KeyRecord>,
}

static STATE: Mutex<State> = Mutex::new(State {
    on: false,
    pending: None,
    recent: VecDeque::new(),
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the mode is on.
pub fn is_on() -> bool {
    state().on
}

/// Turns the mode on or off; returns whether it is on now. Turning it on starts with an empty list.
pub fn set_on(on: bool) -> bool {
    let mut s = state();
    s.on = on;
    s.pending = None;
    if on {
        s.recent.clear();
    }
    on
}

/// A keystroke arrived; it stays open for [`note_ran`] until [`finish`].
pub fn begin(record: KeyRecord) {
    let mut s = state();
    if s.on {
        s.pending = Some(record);
    }
}

/// The action `id` ran for the open keystroke, by `by` (the first one counts).
pub fn note_ran(id: &str, by: &str) {
    let mut s = state();
    if let Some(p) = s.pending.as_mut()
        && p.ran.is_none()
    {
        p.ran = Some((id.to_owned(), by.to_owned()));
    }
}

/// Closes the open keystroke: redacts it, keeps it for the window and returns its log line.
pub fn finish() -> Option<String> {
    let mut s = state();
    let record = s.pending.take()?.redacted();
    let line = record.line();
    s.recent.push_back(record);
    while s.recent.len() > SHOWN {
        s.recent.pop_front();
    }
    Some(line)
}

/// The rows the window shows, oldest first.
pub fn rows() -> Vec<String> {
    state().recent.iter().map(KeyRecord::row).collect()
}

/// The keyboard layout of the latest record.
pub fn layout() -> Option<String> {
    state().recent.back().map(|r| r.layout.clone())
}

/// The latest record, for the status bar.
pub fn latest() -> Option<String> {
    state().recent.back().map(KeyRecord::row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_g() -> KeyRecord {
        KeyRecord {
            time: "2026-11-04T10:15:03.123Z".into(),
            event: "down",
            key: "g".into(),
            key_char: None,
            modifiers: modifiers(true, false, false, false, false),
            layout: "German (com.apple.keylayout.German)".into(),
            layout_key: None,
            stroke: Some("control G".into()),
            bound: vec!["SelectNextOccurrence".into()],
            ran: None,
            focus: "editor".into(),
        }
    }

    /// SPEC-0270 AK-1: line and row of a shortcut, of typed text and of modifier keys.
    #[test]
    fn records_shortcuts_and_hides_typed_text() {
        let mut r = ctrl_g();
        r.ran = Some(("SelectNextOccurrence".into(), "editor".into()));
        assert_eq!(
            r.line(),
            "2026-11-04T10:15:03.123Z\tdown\tkey=g\tchar=-\tmods=⌃\tus_key=-\tstroke=control G\t\
             bound=SelectNextOccurrence\tran=SelectNextOccurrence (editor)\tfocus=editor\t\
             layout=German (com.apple.keylayout.German)"
        );
        assert_eq!(
            r.row(),
            "10:15:03.123  ⌃g  [control G]  → SelectNextOccurrence (editor)  · focus editor"
        );
        let unbound = KeyRecord {
            ran: None,
            ..ctrl_g()
        };
        assert!(
            unbound
                .row()
                .ends_with("→ none; bound: SelectNextOccurrence  · focus editor")
        );

        // Typed text, also with ⌥ on a German layout (⌥L types @): neither key nor character.
        let typed = KeyRecord {
            key: "l".into(),
            key_char: Some("@".into()),
            modifiers: modifiers(false, true, false, false, false),
            stroke: Some("alt L".into()),
            bound: Vec::new(),
            ..ctrl_g()
        }
        .redacted();
        assert_eq!(typed.key, "(typed)");
        assert!(!typed.line().contains('@') && !typed.line().contains("key=l"));
        assert!(
            typed.row().contains("→ none (text input)"),
            "{}",
            typed.row()
        );
        // A bound ⌥ shortcut stays visible.
        let bound_alt = KeyRecord {
            key: "f7".into(),
            key_char: None,
            modifiers: "⌥".into(),
            bound: vec!["FindUsages".into()],
            ..ctrl_g()
        }
        .redacted();
        assert_eq!(bound_alt.key, "f7");

        let mods = KeyRecord {
            event: "modifiers",
            key: String::new(),
            modifiers: modifiers(true, true, false, false, false),
            stroke: None,
            bound: Vec::new(),
            ..ctrl_g()
        };
        assert_eq!(mods.row(), "10:15:03.123  modifiers held: ⌃⌥");
        assert_eq!(modifiers(true, true, true, true, true), "⌃⌥⇧⌘fn");
    }

    /// SPEC-0270 AK-1: the mode keeps the last rows and records only while on.
    #[test]
    fn keeps_the_last_rows_while_on() {
        set_on(false);
        begin(ctrl_g());
        assert_eq!(finish(), None, "off: nothing recorded");
        set_on(true);
        for _ in 0..SHOWN + 3 {
            begin(ctrl_g());
            note_ran("SelectNextOccurrence", "editor");
            note_ran("Other", "workbench");
            assert!(finish().is_some_and(|l| l.contains("ran=SelectNextOccurrence (editor)")));
        }
        assert_eq!(rows().len(), SHOWN);
        assert!(latest().is_some());
        set_on(false);
        assert!(!is_on());
    }
}
