//! Keymaps (`SPEC-0028`).
//!
//! Reads keymap XML in the format of the bundled upstream keymaps
//! (`keymaps/`, Apache-2.0, see `keymaps/README.md`), resolves parent
//! keymaps and answers lookups in both directions.

use std::collections::BTreeMap;
use std::fmt;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::Serialize;

/// Bundled default keymap (Windows/Linux).
pub const DEFAULT_XML: &str = include_str!("../keymaps/$default.xml");
/// Bundled macOS keymap (`$default` as parent).
pub const MACOS_XML: &str = include_str!("../keymaps/Mac OS X 10.5+.xml");
/// Every shortcut by which the bundled macOS keymap differs from the upstream files, with the
/// reason (`SPEC-0028` v4 behaviour 7); `scripts/keymap-coverage.py` reads the same file.
pub const MACOS_DEVIATIONS_JSON: &str = include_str!("../keymaps/macos-deviations.json");

/// One shortcut by which the bundled macOS keymap differs from the upstream files.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
pub struct Deviation {
    /// Action id.
    pub action: String,
    /// `add`: Ferris IDEA binds it and upstream does not; `remove`: the other way round.
    pub change: String,
    /// The shortcut as [`Shortcut::to_entry`] writes it.
    pub shortcut: String,
    /// The specification or improvement entry that decides it.
    pub decided_by: String,
    /// Why, in one sentence.
    pub reason: String,
}

/// The listed deviations of the bundled macOS keymap ([`MACOS_DEVIATIONS_JSON`]).
pub fn macos_deviations() -> Vec<Deviation> {
    serde_json::from_str(MACOS_DEVIATIONS_JSON).expect("bundled deviation list parses")
}

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Modifier {
    /// Control.
    Ctrl,
    /// Alt / Option.
    Alt,
    /// Shift.
    Shift,
    /// Meta / Command.
    Meta,
    /// AltGr.
    AltGraph,
}

/// One key with modifiers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Keystroke {
    /// Sorted modifiers.
    pub modifiers: Vec<Modifier>,
    /// Key name in upper case, e.g. `J`, `F12`, `BACK_SPACE`.
    pub key: String,
}

/// Why a keystroke or keymap could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    /// A keystroke without key or with an unknown modifier.
    #[error("invalid keystroke `{0}`")]
    Keystroke(String),
    /// The XML is malformed.
    #[error("invalid keymap XML: {0}")]
    Xml(String),
}

impl Keystroke {
    /// Parses the Java `KeyStroke` text form, e.g. `control shift F`.
    pub fn parse(text: &str) -> Result<Self, KeymapError> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let (key, mods) = words
            .split_last()
            .ok_or_else(|| KeymapError::Keystroke(text.into()))?;
        let mut modifiers = Vec::new();
        for m in mods {
            // Upstream keymaps also write modifiers capitalised (`Alt Shift O`).
            modifiers.push(match m.to_ascii_lowercase().as_str() {
                "shift" => Modifier::Shift,
                "control" | "ctrl" => Modifier::Ctrl,
                "alt" => Modifier::Alt,
                "meta" => Modifier::Meta,
                "altgraph" => Modifier::AltGraph,
                "pressed" | "released" | "typed" => continue,
                _ => return Err(KeymapError::Keystroke(text.into())),
            });
        }
        if matches!(*key, "pressed" | "released" | "typed") {
            return Err(KeymapError::Keystroke(text.into()));
        }
        modifiers.sort();
        modifiers.dedup();
        Ok(Self {
            modifiers,
            key: key.to_uppercase(),
        })
    }

    /// A keystroke from a toolkit key event: `key` as the toolkit names it (`a`, `up`, `backspace`,
    /// `pageup`, `f12`, `/`), and the held modifiers (`meta` is ⌘ on macOS, the Windows/Super key
    /// elsewhere). Returns `None` for keys without a keymap name.
    pub fn from_platform(
        key: &str,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) -> Option<Self> {
        let named = match key {
            "up" => "UP",
            "down" => "DOWN",
            "left" => "LEFT",
            "right" => "RIGHT",
            "backspace" => "BACK_SPACE",
            "delete" => "DELETE",
            "enter" | "return" => "ENTER",
            "escape" => "ESCAPE",
            "tab" => "TAB",
            "home" => "HOME",
            "end" => "END",
            "pageup" => "PAGE_UP",
            "pagedown" => "PAGE_DOWN",
            "insert" => "INSERT",
            "space" | " " => "SPACE",
            "/" => "SLASH",
            "\\" => "BACK_SLASH",
            "," => "COMMA",
            "." => "PERIOD",
            ";" => "SEMICOLON",
            "=" => "EQUALS",
            "-" => "MINUS",
            "[" => "OPEN_BRACKET",
            "]" => "CLOSE_BRACKET",
            "'" => "QUOTE",
            "`" => "BACK_QUOTE",
            "+" => "ADD",
            // The keypad's `*`, the first key of the folding levels (SPEC-0083 v8 behaviour 23).
            "*" => "MULTIPLY",
            _ => "",
        };
        let key = if !named.is_empty() {
            named.to_owned()
        } else if key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()) {
            key.to_ascii_uppercase()
        } else {
            let n = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())?;
            format!("F{n}")
        };
        let mut modifiers = Vec::new();
        for (on, m) in [
            (ctrl, Modifier::Ctrl),
            (alt, Modifier::Alt),
            (shift, Modifier::Shift),
            (meta, Modifier::Meta),
        ] {
            if on {
                modifiers.push(m);
            }
        }
        modifiers.sort();
        Some(Self { modifiers, key })
    }

    /// Display text for macOS (`true`) or other systems.
    pub fn display(&self, macos: bool) -> String {
        let key = display_key(&self.key, macos);
        if macos {
            let mut out = String::new();
            for (m, symbol) in [
                (Modifier::Ctrl, "⌃"),
                (Modifier::Alt, "⌥"),
                (Modifier::Shift, "⇧"),
                (Modifier::Meta, "⌘"),
                (Modifier::AltGraph, "⌥"),
            ] {
                if self.modifiers.contains(&m) && !out.contains(symbol) {
                    out.push_str(symbol);
                }
            }
            out + &key
        } else {
            let mut parts: Vec<String> = self
                .modifiers
                .iter()
                .map(|m| {
                    match m {
                        Modifier::Ctrl => "Ctrl",
                        Modifier::Alt => "Alt",
                        Modifier::Shift => "Shift",
                        Modifier::Meta => "Meta",
                        Modifier::AltGraph => "AltGr",
                    }
                    .to_owned()
                })
                .collect();
            parts.push(key);
            parts.join("+")
        }
    }
}

/// A text naming keys a dialog or popup handles itself, written with the macOS symbols, in the
/// notation of the system (`SPEC-0028` v7 behaviour 12): unchanged on macOS; elsewhere each key
/// with its modifiers becomes `Ctrl+Alt+Shift+Key` (⌘ and ⌃ as `Ctrl`, since such dialogs take
/// either), and ↩ ⌫ ⌦ ⎋ ⇥ become `Enter`, `Backspace`, `Delete`, `Esc`, `Tab`. Arrows stay.
pub fn local_keys(text: &str, macos: bool) -> String {
    if macos {
        return text.to_owned();
    }
    let named = |c: char| match c {
        '↩' => Some("Enter"),
        '⌫' => Some("Backspace"),
        '⌦' => Some("Delete"),
        '⎋' => Some("Esc"),
        '⇥' => Some("Tab"),
        '↑' => Some("↑"),
        '↓' => Some("↓"),
        '←' => Some("←"),
        '→' => Some("→"),
        _ => None,
    };
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        let start = i;
        while i < chars.len() {
            match chars[i] {
                '⌘' | '⌃' => ctrl = true,
                '⌥' => alt = true,
                '⇧' => shift = true,
                _ => break,
            }
            i += 1;
        }
        let modified = i > start;
        let key = match chars.get(i) {
            Some(&c) if named(c).is_some() => {
                i += 1;
                named(c).map(str::to_owned)
            }
            // After modifiers: a letter, a digit, F1–F12 or a word such as `Enter`.
            Some(&c) if modified && c.is_alphanumeric() => {
                let from = i;
                while i < chars.len() && chars[i].is_alphanumeric() {
                    i += 1;
                }
                let word: String = chars[from..i].iter().collect();
                Some(if word.chars().count() == 1 {
                    word.to_uppercase()
                } else {
                    word
                })
            }
            Some(&c) if modified && !c.is_whitespace() => {
                i += 1;
                Some(c.to_string())
            }
            _ => None,
        };
        match key {
            Some(key) => {
                let mut parts = Vec::new();
                if ctrl {
                    parts.push("Ctrl".to_owned());
                }
                if alt {
                    parts.push("Alt".to_owned());
                }
                if shift {
                    parts.push("Shift".to_owned());
                }
                parts.push(key);
                out.push_str(&parts.join("+"));
            }
            None if modified => out.extend(&chars[start..i]),
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

fn display_key(key: &str, macos: bool) -> String {
    let named = match key {
        "BACK_SPACE" if macos => "⌫",
        "DELETE" if macos => "⌦",
        "ENTER" if macos => "↩",
        "ESCAPE" if macos => "⎋",
        "TAB" if macos => "⇥",
        "UP" if macos => "↑",
        "DOWN" if macos => "↓",
        "LEFT" if macos => "←",
        "RIGHT" if macos => "→",
        "BACK_SPACE" => "Backspace",
        "ESCAPE" => "Esc",
        "PAGE_UP" => "Page Up",
        "PAGE_DOWN" => "Page Down",
        "SPACE" => "Space",
        "COMMA" => ",",
        "PERIOD" => ".",
        "SLASH" => "/",
        "BACK_SLASH" => "\\",
        "SEMICOLON" => ";",
        "MINUS" => "-",
        "EQUALS" => "=",
        "OPEN_BRACKET" => "[",
        "CLOSE_BRACKET" => "]",
        "QUOTE" => "'",
        "BACK_QUOTE" => "`",
        other => {
            let mut s = other.to_lowercase();
            if let Some(first) = s.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            return s.replace('_', " ");
        }
    };
    named.to_owned()
}

impl fmt::Display for Keystroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(false))
    }
}

/// One binding of an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Shortcut {
    /// One or two keystrokes.
    Keyboard {
        /// First keystroke.
        first: Keystroke,
        /// Optional second keystroke.
        second: Option<Keystroke>,
    },
    /// Mouse click with modifiers, as written in the keymap.
    Mouse {
        /// Text, e.g. `alt button1 doubleClick`.
        keystroke: String,
    },
    /// Modifier gesture such as double-shift.
    Gesture {
        /// Text, e.g. `shift SHIFT`.
        keystroke: String,
        /// Gesture, e.g. `dblClick`.
        modifier: String,
    },
}

impl Keystroke {
    fn swap_control_meta(&self) -> Self {
        let mut modifiers: Vec<Modifier> = self
            .modifiers
            .iter()
            .map(|m| match m {
                Modifier::Ctrl => Modifier::Meta,
                Modifier::Meta => Modifier::Ctrl,
                other => *other,
            })
            .collect();
        modifiers.sort();
        Self {
            modifiers,
            key: self.key.clone(),
        }
    }
}

impl Shortcut {
    /// The shortcut with Control and Meta exchanged; gestures stay as they are.
    pub fn swap_control_meta(&self) -> Self {
        match self {
            Self::Keyboard { first, second } => Self::Keyboard {
                first: first.swap_control_meta(),
                second: second.as_ref().map(Keystroke::swap_control_meta),
            },
            Self::Mouse { keystroke } => Self::Mouse {
                keystroke: keystroke
                    .split_whitespace()
                    .map(|w| match w.to_ascii_lowercase().as_str() {
                        "control" | "ctrl" => "meta",
                        "meta" => "control",
                        _ => w,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            },
            Self::Gesture { .. } => self.clone(),
        }
    }

    /// Display text; mouse and gestures in words.
    pub fn display(&self, macos: bool) -> String {
        match self {
            Self::Keyboard { first, second } => match second {
                Some(s) => format!("{}, {}", first.display(macos), s.display(macos)),
                None => first.display(macos),
            },
            Self::Mouse { keystroke } => format!("mouse: {keystroke}"),
            Self::Gesture {
                keystroke,
                modifier,
            } => {
                let key = keystroke.split_whitespace().last().unwrap_or_default();
                if modifier == "dblClick" {
                    format!("double {}", display_key(key, false))
                } else {
                    format!("{modifier} {key}")
                }
            }
        }
    }
}

/// A keymap as written in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapFile {
    /// Keymap name.
    pub name: String,
    /// Parent keymap name.
    pub parent: Option<String>,
    /// Shortcuts by action id (empty list = no shortcut).
    pub actions: BTreeMap<String, Vec<Shortcut>>,
}

impl KeymapFile {
    /// Reads keymap XML.
    pub fn parse(xml: &str) -> Result<Self, KeymapError> {
        let mut reader = Reader::from_str(xml);
        let mut file = KeymapFile {
            name: String::new(),
            parent: None,
            actions: BTreeMap::new(),
        };
        let mut current: Option<String> = None;
        loop {
            let event = reader
                .read_event()
                .map_err(|e| KeymapError::Xml(e.to_string()))?;
            let (tag, closed) = match &event {
                Event::Start(t) => (Some(t.clone()), false),
                Event::Empty(t) => (Some(t.clone()), true),
                Event::End(t) if t.name().as_ref() == b"action" => {
                    current = None;
                    continue;
                }
                Event::Eof => break,
                _ => continue,
            };
            let Some(tag) = tag else { continue };
            let mut attrs = BTreeMap::new();
            for a in tag.attributes() {
                let a = a.map_err(|e| KeymapError::Xml(e.to_string()))?;
                let value = a
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map_err(|e| KeymapError::Xml(e.to_string()))?;
                attrs.insert(
                    String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                    value.into_owned(),
                );
            }
            let get = |k: &str| attrs.get(k).cloned();
            match tag.name().as_ref() {
                b"keymap" => {
                    file.name = get("name").unwrap_or_default();
                    file.parent = get("parent");
                }
                b"action" => {
                    let id =
                        get("id").ok_or_else(|| KeymapError::Xml("action without id".into()))?;
                    // A later entry of the same action replaces the earlier one, as the
                    // reference reads its keymap files (`SPEC-0028` v3 behaviour 3): the macOS
                    // file lists `RunClass` with ⌃⇧F10 and later with ⌃⇧R, and only ⌃⇧R counts.
                    file.actions.insert(id.clone(), Vec::new());
                    current = (!closed).then_some(id);
                }
                name
                @ (b"keyboard-shortcut" | b"mouse-shortcut" | b"keyboard-gesture-shortcut") => {
                    let Some(id) = &current else { continue };
                    let shortcut = match name {
                        b"keyboard-shortcut" => Shortcut::Keyboard {
                            first: Keystroke::parse(&get("first-keystroke").unwrap_or_default())?,
                            second: get("second-keystroke")
                                .map(|s| Keystroke::parse(&s))
                                .transpose()?,
                        },
                        b"mouse-shortcut" => Shortcut::Mouse {
                            keystroke: get("keystroke").unwrap_or_default(),
                        },
                        _ => Shortcut::Gesture {
                            keystroke: get("keystroke").unwrap_or_default(),
                            modifier: get("modifier").unwrap_or_default(),
                        },
                    };
                    file.actions.entry(id.clone()).or_default().push(shortcut);
                }
                _ => {}
            }
        }
        Ok(file)
    }
}

impl Keystroke {
    /// The Java `KeyStroke` text form, e.g. `control shift F` (read back by [`Keystroke::parse`]).
    pub fn to_text(&self) -> String {
        let mut words: Vec<&str> = self
            .modifiers
            .iter()
            .map(|m| match m {
                Modifier::Ctrl => "control",
                Modifier::Alt => "alt",
                Modifier::Shift => "shift",
                Modifier::Meta => "meta",
                Modifier::AltGraph => "altGraph",
            })
            .collect();
        words.push(&self.key);
        words.join(" ")
    }
}

fn xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl KeymapFile {
    /// An empty keymap `name` over `parent`, as the reference writes a user keymap.
    pub fn user(name: &str, parent: &str) -> Self {
        Self {
            name: name.to_owned(),
            parent: Some(parent.to_owned()),
            actions: BTreeMap::new(),
        }
    }

    /// The user keymap in `file`, or an empty one over the platform's keymap when there is none or
    /// it cannot be read.
    ///
    /// `file` is the TOML keymap (`SPEC-0303` behaviour 14); while it does not exist, the XML
    /// keymap older versions wrote beside it (same name, `.xml`) is read. A file given with `.xml`
    /// is read as XML.
    pub fn read_user(file: Option<&std::path::Path>) -> Self {
        let read = |f: &std::path::Path| -> Option<Self> {
            let text = std::fs::read_to_string(f).ok()?;
            if f.extension().is_some_and(|e| e == "xml") {
                Self::parse(&text).ok()
            } else {
                Self::parse_toml(&text).ok()
            }
        };
        file.and_then(|f| {
            if f.exists() {
                read(f)
            } else {
                read(&ferris_idea_core::legacy_keymap_file(f))
            }
        })
        .map(|mut k| {
            // A keymap saved under the product's former name keeps its content (`SPEC-0356`
            // behaviour 8).
            k.name = ferris_idea_core::takeover::current_keymap_name(&k.name).to_owned();
            k
        })
        .unwrap_or_else(|| Self::user("Ferris IDEA", platform_parent()))
    }

    /// Gives `action` exactly `shortcuts` (an empty list removes its shortcuts), or with `None`
    /// drops the change so the parent's shortcuts apply again.
    pub fn set_action(&mut self, action: &str, shortcuts: Option<Vec<Shortcut>>) {
        match shortcuts {
            Some(s) => {
                self.actions.insert(action.to_owned(), s);
            }
            None => {
                self.actions.remove(action);
            }
        }
    }

    /// Writes the keymap to `file` as TOML with the envelope, creating its folder; the XML file of
    /// older versions beside it is kept once as `.xml.bak` (`SPEC-0303` behaviours 14–15). A
    /// file of a newer format is not replaced.
    pub fn write(&self, file: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        ferris_idea_core::format::prepare_write(file, "keymap")?;
        write_atomic(file, self.to_toml_text())?;
        let legacy = ferris_idea_core::legacy_keymap_file(file);
        if legacy != file {
            ferris_idea_core::format::retire(&legacy)?;
        }
        Ok(())
    }

    /// The keymap as XML in the reference's format (read back by [`KeymapFile::parse`]).
    pub fn to_xml(&self) -> String {
        let mut out = format!("<keymap version=\"1\" name=\"{}\"", xml_attr(&self.name));
        if let Some(parent) = &self.parent {
            out.push_str(&format!(" parent=\"{}\"", xml_attr(parent)));
        }
        out.push_str(">\n");
        for (id, shortcuts) in &self.actions {
            if shortcuts.is_empty() {
                out.push_str(&format!("  <action id=\"{}\"/>\n", xml_attr(id)));
                continue;
            }
            out.push_str(&format!("  <action id=\"{}\">\n", xml_attr(id)));
            for s in shortcuts {
                let line = match s {
                    Shortcut::Keyboard { first, second } => match second {
                        Some(second) => format!(
                            "<keyboard-shortcut first-keystroke=\"{}\" second-keystroke=\"{}\"/>",
                            xml_attr(&first.to_text()),
                            xml_attr(&second.to_text())
                        ),
                        None => format!(
                            "<keyboard-shortcut first-keystroke=\"{}\"/>",
                            xml_attr(&first.to_text())
                        ),
                    },
                    Shortcut::Mouse { keystroke } => {
                        format!("<mouse-shortcut keystroke=\"{}\"/>", xml_attr(keystroke))
                    }
                    Shortcut::Gesture {
                        keystroke,
                        modifier,
                    } => format!(
                        "<keyboard-gesture-shortcut keystroke=\"{}\" modifier=\"{}\"/>",
                        xml_attr(keystroke),
                        xml_attr(modifier)
                    ),
                };
                out.push_str(&format!("    {line}\n"));
            }
            out.push_str("  </action>\n");
        }
        out.push_str("</keymap>\n");
        out
    }
}

/// Name of the platform's bundled keymap, the parent of a user keymap.
pub fn platform_parent() -> &'static str {
    if cfg!(target_os = "macos") {
        "Mac OS X 10.5+"
    } else {
        "$default"
    }
}

impl Shortcut {
    /// A keyboard shortcut in keymap notation: `control alt S`, or two keystrokes separated by a
    /// comma (`control K, control C`).
    pub fn parse_keyboard(text: &str) -> Result<Self, KeymapError> {
        let mut parts = text.splitn(2, ',');
        let first = Keystroke::parse(parts.next().unwrap_or_default())?;
        let second = parts
            .next()
            .map(|s| Keystroke::parse(s.trim()))
            .transpose()?;
        Ok(Self::Keyboard { first, second })
    }

    /// The shortcut as one string of a TOML keymap (`SPEC-0303` behaviour 9): keymap notation for
    /// keyboard shortcuts, `mouse: …` and `gesture: <modifier> <keystroke>` otherwise.
    pub fn to_entry(&self) -> String {
        match self {
            Self::Keyboard { first, second } => match second {
                Some(s) => format!("{}, {}", first.to_text(), s.to_text()),
                None => first.to_text(),
            },
            Self::Mouse { keystroke } => format!("mouse: {keystroke}"),
            Self::Gesture {
                keystroke,
                modifier,
            } => format!("gesture: {modifier} {keystroke}"),
        }
    }

    /// Reads a string written by [`Shortcut::to_entry`].
    pub fn from_entry(text: &str) -> Result<Self, KeymapError> {
        if let Some(rest) = text.strip_prefix("mouse:") {
            let keystroke = rest.trim();
            if keystroke.is_empty() {
                return Err(KeymapError::Keystroke(text.into()));
            }
            return Ok(Self::Mouse {
                keystroke: keystroke.into(),
            });
        }
        if let Some(rest) = text.strip_prefix("gesture:") {
            let mut words = rest.split_whitespace();
            let modifier = words.next().unwrap_or_default();
            let keystroke = words.collect::<Vec<_>>().join(" ");
            if modifier.is_empty() || keystroke.is_empty() {
                return Err(KeymapError::Keystroke(text.into()));
            }
            return Ok(Self::Gesture {
                keystroke,
                modifier: modifier.into(),
            });
        }
        Self::parse_keyboard(text)
    }
}

impl KeymapFile {
    /// The keymap as the body table of a TOML keymap: `name`, `parent`, `actions`.
    pub fn to_toml(&self) -> toml::Table {
        let mut t = toml::Table::new();
        t.insert("name".into(), self.name.clone().into());
        if let Some(p) = &self.parent {
            t.insert("parent".into(), p.clone().into());
        }
        let actions: toml::Table = self
            .actions
            .iter()
            .map(|(id, list)| {
                (
                    id.clone(),
                    toml::Value::Array(list.iter().map(|s| s.to_entry().into()).collect()),
                )
            })
            .collect();
        t.insert("actions".into(), toml::Value::Table(actions));
        t
    }

    /// Reads the `keymap` table of a TOML keymap; problems name their key path under `prefix`.
    pub fn from_toml(
        table: &toml::Table,
        prefix: &str,
    ) -> Result<Self, Vec<ferris_idea_core::format::Problem>> {
        use ferris_idea_core::format::Problem;
        let mut problems = Vec::new();
        let mut file = KeymapFile {
            name: table
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            parent: table
                .get("parent")
                .and_then(toml::Value::as_str)
                .map(str::to_owned),
            actions: BTreeMap::new(),
        };
        if let Some(actions) = table.get("actions").and_then(toml::Value::as_table) {
            for (id, list) in actions {
                let mut shortcuts = Vec::new();
                for (i, s) in list.as_array().into_iter().flatten().enumerate() {
                    match s.as_str().map(Shortcut::from_entry) {
                        Some(Ok(s)) => shortcuts.push(s),
                        Some(Err(e)) => problems.push(Problem::new(
                            format!("{prefix}.actions.{id}[{i}]"),
                            e.to_string(),
                        )),
                        None => problems.push(Problem::new(
                            format!("{prefix}.actions.{id}[{i}]"),
                            "expected a string",
                        )),
                    }
                }
                file.actions.insert(id.clone(), shortcuts);
            }
        }
        if problems.is_empty() {
            Ok(file)
        } else {
            Err(problems)
        }
    }

    /// The text of the user keymap file (`SPEC-0303`): envelope and `[keymap]`.
    pub fn to_toml_text(&self) -> String {
        let mut body = toml::Table::new();
        body.insert("keymap".into(), toml::Value::Table(self.to_toml()));
        ferris_idea_core::format::write(
            &ferris_idea_core::format::Envelope::current("keymap"),
            &body,
        )
        .unwrap_or_default()
    }

    /// Reads a keymap file of kind `keymap`.
    pub fn parse_toml(text: &str) -> Result<Self, Vec<ferris_idea_core::format::Problem>> {
        let doc = ferris_idea_core::format::read(
            text,
            Some("keymap"),
            ferris_idea_core::format::Use::Project,
        )?;
        let table = doc
            .body
            .get("keymap")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        Self::from_toml(&table, "keymap")
    }
}

/// A keymap with its parents resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    /// Keymap name.
    pub name: String,
    /// Whether shortcuts are shown with macOS symbols.
    pub macos: bool,
    actions: BTreeMap<String, Vec<Shortcut>>,
}

impl Keymap {
    /// Resolves `child` over `parent` (`SPEC-0028` behaviour 3).
    pub fn resolve(parent: &KeymapFile, child: Option<&KeymapFile>, macos: bool) -> Self {
        let mut actions = parent.actions.clone();
        if let Some(child) = child {
            for (id, shortcuts) in &child.actions {
                actions.insert(id.clone(), shortcuts.clone());
            }
        }
        actions.retain(|_, s| !s.is_empty());
        Self {
            name: child.map_or(&parent.name, |c| &c.name).clone(),
            macos,
            actions,
        }
    }

    /// Gives `action` the shortcut `keys` when nothing else uses it: shortcuts of actions that
    /// come with the reference's bundled plugins, not with its keymap files.
    fn add_if_free(&mut self, action: &str, keys: &str) {
        let Ok(stroke) = Keystroke::parse(keys) else {
            return;
        };
        if self.actions_for(&stroke, None).is_empty() {
            self.actions
                .entry(action.into())
                .or_default()
                .push(Shortcut::Keyboard {
                    first: stroke,
                    second: None,
                });
        }
    }

    /// Adds `keys` to `action` even when another action has it too; the workbench decides which
    /// one runs (`SPEC-0059` v9.1 behaviour 22b).
    fn add_shared(&mut self, action: &str, keys: &str) {
        let Ok(stroke) = Keystroke::parse(keys) else {
            return;
        };
        let shortcuts = self.actions.entry(action.into()).or_default();
        let shortcut = Shortcut::Keyboard {
            first: stroke,
            second: None,
        };
        if !shortcuts.contains(&shortcut) {
            shortcuts.push(shortcut);
        }
    }

    /// Next and previous inline completion suggestion (`SPEC-0162` behaviour 2): ⌥] and ⌥[ as the
    /// reference's actions declare them (intellij-community `intellij.platform.lang.impl.actions.xml`),
    /// plus `next`/`previous`, which need no bracket key: on German and other layouts `[` and `]`
    /// are typed with ⌥ and cannot be part of a shortcut (IMP-0050).
    fn add_inline_suggestion_keys(&mut self, next: &str, previous: &str) {
        self.add_if_free("NextInlineCompletionSuggestionAction", "alt CLOSE_BRACKET");
        self.add_if_free("NextInlineCompletionSuggestionAction", next);
        self.add_if_free("PrevInlineCompletionSuggestionAction", "alt OPEN_BRACKET");
        self.add_if_free("PrevInlineCompletionSuggestionAction", previous);
    }

    /// The bundled Windows/Linux keymap.
    pub fn bundled_default() -> Self {
        let file = KeymapFile::parse(DEFAULT_XML).expect("bundled keymap parses");
        let mut keymap = Self::resolve(&file, None, false);
        // Reformat with Prettier (SPEC-0123).
        keymap.add_if_free("ReformatWithPrettierAction", "control alt shift P");
        // Commit (hand test 195: ⌘K did nothing).
        keymap.add_if_free("CheckinProject", "control K");
        // Update Project and Push come with the reference's VCS plugin, not with `$default`
        // (OBS-0022 re-check: ⌘T and ⇧⌘K did nothing, SPEC-0109 v3).
        keymap.add_if_free("Vcs.UpdateProject", "control T");
        keymap.add_if_free("Vcs.Push", "control shift K");
        // Search in selection of the find bar (SPEC-0065 v4 behaviour 12). The upstream files give
        // this key to `ToggleFindInSelection`, the same command under the reference's id, so the
        // key is shared (SPEC-0028 v4 behaviour 8); `add_if_free` never bound it.
        keymap.add_shared("ToggleInSelection", "control alt E");
        // Sync the changed build files: the reference's Maven and Gradle plugins bring it, not
        // its keymap (SPEC-0100 v2 behaviour 9, OBS-0024).
        keymap.add_if_free("ExternalSystem.ProjectRefreshAction", "control shift O");
        // Step Out of Code Block and Reload Changed Classes have no shortcut in the reference's
        // keymaps; Ferris IDEA gives them free ones (SPEC-0091 v12 behaviours 25, 37).
        keymap.add_if_free("StepOutOfBlock", "control alt shift F10");
        keymap.add_if_free("Hotswap", "control alt shift F9");
        // Show DDL of the Database tool window's table or view; the reference's Edit Source key
        // already opens Properties there (SPEC-0089 v5 behaviour 11).
        keymap.add_if_free("FerrisIDEA.Database.ShowDdl", "control alt shift D");
        keymap.add_inline_suggestion_keys("alt PERIOD", "alt COMMA");
        // Editor tabs (SPEC-0059 v9 behaviour 19, v9.9 behaviour 22b): Reopen Closed Tab has no
        // shortcut here (Ctrl+Shift+T is Go to Test, Ctrl+Alt+Shift+T Refactor This); Maximize
        // Editor shares the tool window's key.
        keymap.add_shared("MaximizeEditorInSplit", "control shift QUOTE");
        // Exclude of the Find tool window: the keymap files bind only Include; Exclude shares
        // the Delete keys and runs while that window has the focus (SPEC-0293 v9 behaviour 25).
        keymap.add_shared("UsageView.Exclude", "DELETE");
        keymap.add_shared("UsageView.Exclude", "BACK_SPACE");
        keymap
    }

    /// The macOS keymap exactly as the upstream files give it, without Ferris IDEA's own shortcuts:
    /// what the bundled keymap is measured against (`SPEC-0028` v4 behaviour 7). Shortcuts
    /// inherited from `$default` swap Control and Meta, as the reference does for its macOS
    /// default keymaps (behaviour 3a).
    pub fn upstream_macos() -> Self {
        let mut parent = KeymapFile::parse(DEFAULT_XML).expect("bundled keymap parses");
        for shortcuts in parent.actions.values_mut() {
            for s in shortcuts.iter_mut() {
                *s = s.swap_control_meta();
            }
        }
        let child = KeymapFile::parse(MACOS_XML).expect("bundled keymap parses");
        Self::resolve(&parent, Some(&child), true)
    }

    /// The bundled macOS keymap: the upstream one ([`Keymap::upstream_macos`]) with Ferris IDEA's own
    /// shortcuts, each of them listed in [`MACOS_DEVIATIONS_JSON`].
    pub fn bundled_macos() -> Self {
        let mut keymap = Self::upstream_macos();
        // Reformat with Prettier (SPEC-0123).
        keymap.add_if_free("ReformatWithPrettierAction", "alt shift meta P");
        // Commit (hand test 195: ⌘K did nothing).
        keymap.add_if_free("CheckinProject", "meta K");
        // Update Project and Push (see `bundled_default`).
        keymap.add_if_free("Vcs.UpdateProject", "meta T");
        keymap.add_if_free("Vcs.Push", "shift meta K");
        // Search in selection of the find bar (see `bundled_default`).
        keymap.add_shared("ToggleInSelection", "control alt E");
        // Sync the changed build files (see `bundled_default`).
        keymap.add_if_free("ExternalSystem.ProjectRefreshAction", "shift meta I");
        // Step Out of Code Block and Reload Changed Classes (see `bundled_default`).
        keymap.add_if_free("StepOutOfBlock", "control alt shift F10");
        keymap.add_if_free("Hotswap", "control alt shift F9");
        // Show DDL (see `bundled_default`).
        keymap.add_if_free("FerrisIDEA.Database.ShowDdl", "alt shift meta D");
        keymap.add_inline_suggestion_keys("control alt PERIOD", "control alt COMMA");
        // Editor tabs (SPEC-0059 v9 behaviour 19, v9.1 behaviour 22b): ⇧⌘T reopens a closed tab
        // (the bundled keymap's other users of it are not carried out), ⇧⌘' maximizes the editor
        // while it has the focus.
        keymap.add_shared("ReopenClosedTab", "shift meta T");
        keymap.add_shared("MaximizeEditorInSplit", "shift meta QUOTE");
        // Exclude of the Find tool window (see `bundled_default`).
        keymap.add_shared("UsageView.Exclude", "DELETE");
        keymap.add_shared("UsageView.Exclude", "BACK_SPACE");
        // ⌃Space switches the input source on Macs with more than one keyboard layout and never
        // reaches the IDE; ⌥Esc, the Cocoa "complete" key, also starts completion (IMP-0032).
        let alt_escape = Keystroke::parse("alt ESCAPE").expect("valid keystroke");
        if keymap.actions_for(&alt_escape, None).is_empty() {
            keymap
                .actions
                .entry("CodeCompletion".into())
                .or_default()
                .push(Shortcut::Keyboard {
                    first: alt_escape,
                    second: None,
                });
        }
        keymap
    }

    /// The bundled keymap of the platform Ferris IDEA runs on.
    pub fn bundled_for_platform() -> Self {
        if cfg!(target_os = "macos") {
            Self::bundled_macos()
        } else {
            Self::bundled_default()
        }
    }

    /// The keymap with the user's changes over it: an action listed in `user` gets exactly the
    /// shortcuts listed there (none removes them), as the reference resolves a user keymap over
    /// its parent (`SPEC-0105`).
    pub fn with_overrides(mut self, user: &KeymapFile) -> Self {
        for (id, shortcuts) in &user.actions {
            self.actions.insert(id.clone(), shortcuts.clone());
        }
        self.actions.retain(|_, s| !s.is_empty());
        self.name = user.name.clone();
        self
    }

    /// Number of actions with at least one shortcut.
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Whether no action has a shortcut.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// All bound actions with their shortcuts, sorted by id.
    pub fn actions(&self) -> impl Iterator<Item = (&str, &[Shortcut])> {
        self.actions
            .iter()
            .map(|(id, s)| (id.as_str(), s.as_slice()))
    }

    /// Shortcuts of `action` (empty when unbound).
    pub fn shortcuts(&self, action: &str) -> &[Shortcut] {
        self.actions.get(action).map_or(&[], Vec::as_slice)
    }

    /// The shortcut text shown beside `action` in menus and tooltips: its first shortcut, written for
    /// macOS when `macos` (`SPEC-0275` v5 behaviour 14a, `SPEC-0294` behaviour 2); empty when it has
    /// none.
    pub fn shortcut_label(&self, action: &str, macos: bool) -> String {
        self.shown_shortcuts(action)
            .first()
            .map(|s| s.display(macos))
            .unwrap_or_default()
    }

    /// Whether `first` only starts shortcuts of two keystrokes: some shortcut has it as its
    /// first keystroke with a second one, and none is this keystroke alone (`SPEC-0083` v8
    /// behaviour 23).
    pub fn starts_two_strokes_only(&self, first: &Keystroke) -> bool {
        let mut starts = false;
        for s in self.actions.values().flatten() {
            if let Shortcut::Keyboard { first: f, second } = s
                && f == first
            {
                if second.is_none() {
                    return false;
                }
                starts = true;
            }
        }
        starts
    }

    /// Actions bound to `first` (and `second`, for two-stroke shortcuts), sorted by id. An action
    /// that yields the chord to another action bound to it is left out ([`yields_to`]).
    pub fn actions_for(&self, first: &Keystroke, second: Option<&Keystroke>) -> Vec<&str> {
        let bound: Vec<&str> = self
            .actions
            .iter()
            .filter(|(_, shortcuts)| {
                shortcuts.iter().any(|s| match s {
                    Shortcut::Keyboard {
                        first: f,
                        second: s,
                    } => f == first && (second.is_none() || s.as_ref() == second),
                    _ => false,
                })
            })
            .map(|(id, _)| id.as_str())
            .collect();
        bound
            .iter()
            .copied()
            .filter(|a| yields_to(a).is_none_or(|winner| !bound.contains(&winner)))
            .collect()
    }

    /// Actions whose mouse shortcut is a click of `button` (1 primary, 2 middle, 3 secondary) with
    /// exactly these modifiers, `double` for a double click (`SPEC-0130` v2 behaviour 5). `meta` is
    /// ⌘ on macOS. Sorted by id.
    pub fn mouse_actions(&self, button: u8, double: bool, held: MouseModifiers) -> Vec<&str> {
        self.actions
            .iter()
            .filter(|(_, shortcuts)| {
                shortcuts.iter().any(|s| match s {
                    Shortcut::Mouse { keystroke } => {
                        mouse_click(keystroke) == Some((button, double, held))
                    }
                    _ => false,
                })
            })
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// The shortcuts of `action` that run it: its shortcuts without the chords it yields to
    /// another action (`SPEC-0071` v10 behaviour 16). Menus, tooltips and Find Action show these.
    pub fn shown_shortcuts(&self, action: &str) -> Vec<&Shortcut> {
        let yields = yields_to(action).is_some();
        self.shortcuts(action)
            .iter()
            .filter(|s| match s {
                Shortcut::Keyboard { first, second } if yields => {
                    self.actions_for(first, second.as_ref()).contains(&action)
                }
                _ => true,
            })
            .collect()
    }
}

/// The modifiers held during a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseModifiers {
    /// Control.
    pub ctrl: bool,
    /// Alt (⌥).
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// Meta (⌘ on macOS).
    pub meta: bool,
}

/// What a mouse shortcut as the keymap files write it (`alt shift button1`, `alt button1
/// doubleClick`) means: the button, whether it is a double click, and the modifiers. `None` for a
/// text with a word that is none of these.
fn mouse_click(keystroke: &str) -> Option<(u8, bool, MouseModifiers)> {
    let mut held = MouseModifiers::default();
    let (mut button, mut double) = (None, false);
    for word in keystroke.split_whitespace() {
        match word.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => held.ctrl = true,
            "alt" => held.alt = true,
            "shift" => held.shift = true,
            "meta" => held.meta = true,
            "doubleclick" => double = true,
            other => {
                button = Some(other.strip_prefix("button")?.parse::<u8>().ok()?);
            }
        }
    }
    Some((button?, double, held))
}

/// The action that keeps a chord `action` is bound to as well: Ferris IDEA's keymap gives ⇧⌘T to
/// `ReopenClosedTab` as a named deviation, and the bundled keymap's `GotoTest` on the same chord
/// neither runs from it nor shows it (`SPEC-0071` v10 behaviour 16, `SPEC-0028` behaviour 7). No
/// shortcut is removed from the keymap: a chord without the other action runs `action`.
pub fn yields_to(action: &str) -> Option<&'static str> {
    match action {
        "GotoTest" => Some("ReopenClosedTab"),
        _ => None,
    }
}

/// The US key whose shortcut a key of the current keyboard layout stands in for, following the
/// macOS convention of moving shortcuts that a layout cannot type to the key in the same place
/// (e.g. on German layouts `⌘[` becomes `⌘ö`). `equivalents` maps US keys to the layout's keys as
/// the platform reports them (`SPEC-0071` v4 behaviour 6, IMP-0031).
pub fn us_key_for<S: std::hash::BuildHasher>(
    key: &str,
    equivalents: &std::collections::HashMap<char, char, S>,
) -> Option<char> {
    let mut chars = key.chars();
    let typed = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let mut found: Vec<char> = equivalents
        .iter()
        .filter(|&(_, &layout)| layout == typed)
        .map(|(&us, _)| us)
        .collect();
    found.sort_unstable();
    found.first().copied().or_else(|| {
        // Where the layout moves `` ` `` elsewhere (German: to `<`), the key in its place on the
        // keyboard, left of 1, types the dead key `^`; it stands for `` ` `` too, so ⌃` (Quick
        // Switch Scheme) works from the key a US keymap means (IMP-0283).
        (typed == '^' && equivalents.get(&'`').is_some_and(|&layout| layout != '`')).then_some('`')
    })
}

/// The key that types `c` together with Shift on a US keyboard, as the toolkit names it (`!` is
/// ⇧1, `?` is ⇧/); `None` for every other character. `+` is left out: it has the key name `ADD`
/// of its own (`SPEC-0028` v5 behaviour 10).
pub fn us_unshifted(c: char) -> Option<char> {
    Some(match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        _ => return None,
    })
}

/// The keystrokes one key event may stand for (`SPEC-0028` v5 behaviour 10); [`Self::resolve`]
/// picks the one a keymap is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventStrokes {
    /// The key with Shift that types the event's character: toolkits report ⌃⇧1 as the
    /// character the layout types with ⇧1 (`!`), without Shift.
    pub shifted: Option<Keystroke>,
    /// The event's key under its own name.
    pub typed: Option<Keystroke>,
    /// The US key the layout's key stands in for ([`us_key_for`]).
    pub layout: Option<Keystroke>,
}

impl EventStrokes {
    /// The keystrokes of a toolkit key event: `key` and the held modifiers as for
    /// [`Keystroke::from_platform`], `equivalents` the layout's keys for US keys as the platform
    /// reports them (`None` on a US layout and where the platform knows none).
    pub fn of<S: std::hash::BuildHasher>(
        key: &str,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
        equivalents: Option<&std::collections::HashMap<char, char, S>>,
    ) -> Self {
        let typed = Keystroke::from_platform(key, ctrl, alt, shift, meta);
        if !(ctrl || alt || meta) {
            // Without one of these the key types text; the layout decides what.
            return Self {
                typed,
                ..Self::default()
            };
        }
        let us = equivalents.and_then(|e| us_key_for(key, e));
        let layout =
            us.and_then(|us| Keystroke::from_platform(&us.to_string(), ctrl, alt, shift, meta));
        // The character as a US keyboard types it: the layout's stand-in, else the character
        // itself where the layout does not move that US character to another key.
        let mut chars = key.chars();
        let own = chars
            .next()
            .filter(|c| chars.next().is_none() && equivalents.is_none_or(|e| !e.contains_key(c)));
        // A toolkit that leaves Shift in the event names the key itself.
        let shifted = us
            .or(own)
            .filter(|_| !shift)
            .and_then(us_unshifted)
            .and_then(|base| Keystroke::from_platform(&base.to_string(), ctrl, alt, true, meta));
        Self {
            shifted,
            typed,
            layout,
        }
    }

    fn shifted_digit(&self) -> Option<&Keystroke> {
        self.shifted
            .as_ref()
            .filter(|s| s.key.chars().all(|c| c.is_ascii_digit()))
    }

    /// The keystroke `keymap` is asked for. A digit with Shift comes first where the keymap binds
    /// it, whatever character the layout types there; then the key under its own name where the
    /// keymap binds it or the layout offers no other; then the layout's US key; then the key with
    /// Shift of any other character.
    pub fn resolve(&self, keymap: &Keymap) -> Option<Keystroke> {
        let bound = |stroke: &Keystroke| !keymap.actions_for(stroke, None).is_empty();
        if let Some(digit) = self.shifted_digit().filter(|s| bound(s)) {
            return Some(digit.clone());
        }
        self.typed
            .clone()
            .filter(|typed| bound(typed) || self.layout.is_none())
            .or_else(|| self.layout.clone())
            .or_else(|| self.shifted.clone())
    }

    /// The keystroke to record as a new shortcut: a digit with Shift where the event is one, else
    /// the key under its own name, the layout's US key, the key with Shift of another character.
    pub fn recorded(&self) -> Option<Keystroke> {
        self.shifted_digit()
            .or(self.typed.as_ref())
            .or(self.layout.as_ref())
            .or(self.shifted.as_ref())
            .cloned()
    }
}

/// Writes `contents` through a temporary file and a rename, so the file is never half written
/// (`SPEC-0262` behaviour 2).
pub(crate) fn write_atomic(
    path: &std::path::Path,
    contents: impl AsRef<[u8]>,
) -> std::io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}", std::process::id()));
    let tmp = path.with_file_name(name);
    let written = std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0028 v7 AK-14: keys a dialog handles itself, per platform.
    #[test]
    fn local_keys_follow_the_platform() {
        let text = "More (⇧⌘F8) · ⌥K · ⇧↩ collapse · ⌘↩ done · ⌘N · ↑↓ recent · Evaluate (↩) · ⌘⌫ · ⌥[ · ⎋ ⇥ ⌦ · ⌃⇧R · ⌥Enter";
        assert_eq!(local_keys(text, true), text);
        assert_eq!(
            local_keys(text, false),
            "More (Ctrl+Shift+F8) · Alt+K · Shift+Enter collapse · Ctrl+Enter done · Ctrl+N · ↑↓ recent · Evaluate (Enter) · Ctrl+Backspace · Alt+[ · Esc Tab Delete · Ctrl+Shift+R · Alt+Enter"
        );
        assert_eq!(local_keys("⌥e", false), "Alt+E");
        assert_eq!(local_keys("no keys here", false), "no keys here");
    }

    #[test]
    fn layout_keys_stand_in_for_us_shortcut_keys() {
        let german: std::collections::HashMap<char, char> =
            [('[', 'ö'), (']', 'ä'), ('/', 'ß'), (';', 'ü')]
                .into_iter()
                .collect();
        assert_eq!(us_key_for("ö", &german), Some('['));
        assert_eq!(us_key_for("ä", &german), Some(']'));
        assert_eq!(us_key_for("a", &german), None);
        assert_eq!(us_key_for("left", &german), None);
        assert_eq!(us_key_for("", &german), None);
        // ⌃` on German layouts (project owner, 2026-11-05: nothing happens in the reference): the key
        // macOS moves `` ` `` to and the dead key in its place both reach `` ` ``.
        let german: std::collections::HashMap<char, char> =
            [('`', '<'), ('<', ';'), ('^', '&'), ('[', 'ö')]
                .into_iter()
                .collect();
        assert_eq!(us_key_for("<", &german), Some('`'));
        assert_eq!(us_key_for("^", &german), Some('`'));
        let us: std::collections::HashMap<char, char> = [('[', '[')].into_iter().collect();
        assert_eq!(us_key_for("^", &us), None, "a layout that types ` keeps ^");
        let backquote = Keystroke::from_platform("`", true, false, false, false).unwrap();
        assert_eq!(
            Keymap::bundled_macos().actions_for(&backquote, None),
            ["QuickChangeScheme"]
        );
        assert_eq!(
            Keymap::bundled_macos().actions_for(&Keystroke::parse("meta K").unwrap(), None),
            ["CheckinProject"]
        );
        // SPEC-0100 v2 AK-8: ⇧⌘I (⌃⇧O) syncs the changed build files, when nothing else has it.
        let sync = |k: &Keymap, keys: &str| {
            k.actions_for(&Keystroke::parse(keys).unwrap(), None)
                .contains(&"ExternalSystem.ProjectRefreshAction")
        };
        assert!(sync(&Keymap::bundled_macos(), "shift meta I"));
        assert!(sync(&Keymap::bundled_default(), "control shift O"));
        // SPEC-0091 v12: ⌃⌥⇧F10 steps out of the code block, ⌃⌥⇧F9 reloads changed classes.
        for k in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            let on = |keys: &str| k.actions_for(&Keystroke::parse(keys).unwrap(), None);
            assert_eq!(on("control alt shift F10"), ["StepOutOfBlock"]);
            assert_eq!(on("control alt shift F9"), ["Hotswap"]);
        }
        // SPEC-0089 v5 AK-10: ⌥⇧⌘D (Ctrl+Alt+Shift+D) shows the DDL; nothing else had it.
        let on = |k: &Keymap, keys: &str| {
            k.actions_for(&Keystroke::parse(keys).unwrap(), None)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            on(&Keymap::bundled_macos(), "alt shift meta D"),
            ["FerrisIDEA.Database.ShowDdl"]
        );
        assert_eq!(
            on(&Keymap::bundled_default(), "control alt shift D"),
            ["FerrisIDEA.Database.ShowDdl"]
        );
    }

    // SPEC-0109 v3 behaviour 11: ⌘T updates, ⇧⌘K pushes, as the reference's VCS plugin binds them.
    #[test]
    fn update_and_push_have_the_reference_shortcuts() {
        let mac = Keymap::bundled_macos();
        assert_eq!(
            mac.actions_for(&Keystroke::parse("meta T").unwrap(), None),
            ["Vcs.UpdateProject"]
        );
        assert_eq!(
            mac.actions_for(&Keystroke::parse("shift meta K").unwrap(), None),
            ["Vcs.Push"]
        );
        assert_eq!(mac.shortcut_label("Vcs.UpdateProject", true), "⌘T");
        assert_eq!(mac.shortcut_label("Vcs.Push", true), "⇧⌘K");
        let default = Keymap::bundled_default();
        assert_eq!(
            default.actions_for(&Keystroke::parse("control T").unwrap(), None),
            ["Vcs.UpdateProject"]
        );
    }

    /// SPEC-0071 AK-12 (v10), SPEC-0124 AK-7 (v5): on ⇧⌘T of the macOS keymap Go to Test yields to
    /// Reopen Closed Tab and shows no key; the keymap itself is unchanged, and the default
    /// keymap, where Reopen Closed Tab has another chord, runs Go to Test from Ctrl+Shift+T.
    #[test]
    fn goto_test_yields_the_chord_of_reopen_closed_tab() {
        let mac = Keymap::bundled_macos();
        let chord = k("shift meta T");
        let on = mac.actions_for(&chord, None);
        assert!(on.contains(&"ReopenClosedTab"), "{on:?}");
        assert!(!on.contains(&"GotoTest"), "{on:?}");
        assert!(mac.shortcuts("GotoTest").contains(&Shortcut::Keyboard {
            first: chord,
            second: None
        }));
        assert!(mac.shown_shortcuts("GotoTest").is_empty());
        assert_eq!(mac.shortcut_label("GotoTest", true), "");
        assert_eq!(
            mac.shown_shortcuts("ReopenClosedTab").len(),
            mac.shortcuts("ReopenClosedTab").len()
        );
        assert_eq!(mac.shortcut_label("ReopenClosedTab", true), "⇧⌘T");
        let default = Keymap::bundled_default();
        let chord = k("control shift T");
        assert!(default.actions_for(&chord, None).contains(&"GotoTest"));
        assert!(
            !default
                .actions_for(&chord, None)
                .contains(&"ReopenClosedTab")
        );
        assert_eq!(default.shown_shortcuts("GotoTest").len(), 1);
        assert_eq!(yields_to("ReopenClosedTab"), None);
    }

    /// SPEC-0059 v9.9 behaviour 22b: the default keymap gives Reopen Closed Tab no shortcut, as the
    /// reference's; Ctrl+Alt+Shift+T is Refactor This there (the Linux window self-test pressed
    /// it for Reopen Closed Tab and got nothing).
    #[test]
    fn reopen_closed_tab_has_no_shortcut_in_the_default_keymap() {
        let default = Keymap::bundled_default();
        assert!(default.shortcuts("ReopenClosedTab").is_empty());
        assert_eq!(
            default.actions_for(&k("control alt shift T"), None),
            ["Refactorings.QuickListPopupAction"]
        );
    }

    fn k(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    /// SPEC-0083 v8 behaviour 23: the keypad's `*` starts the folding levels and nothing else.
    #[test]
    fn the_level_shortcuts_start_with_a_key_of_their_own() {
        let mac = Keymap::bundled_macos();
        let first = Keystroke::from_platform("*", false, true, false, true).unwrap();
        assert_eq!(first, Keystroke::parse("alt meta MULTIPLY").unwrap());
        assert!(mac.starts_two_strokes_only(&first));
        let two = Keystroke::from_platform("2", false, false, false, false).unwrap();
        assert_eq!(mac.actions_for(&first, Some(&two)), ["ExpandAllToLevel2"]);
        let to = Keystroke::from_platform("*", false, false, false, true).unwrap();
        assert_eq!(mac.actions_for(&to, Some(&two)), ["ExpandToLevel2"]);
        // A key with a shortcut of its own does not wait for a second one.
        assert!(!mac.starts_two_strokes_only(&Keystroke::parse("meta MINUS").unwrap()));
        assert!(!mac.starts_two_strokes_only(&Keystroke::parse("meta F19").unwrap()));
    }

    #[test]
    fn keystroke_parsing() {
        assert_eq!(
            k("control shift F"),
            Keystroke {
                modifiers: vec![Modifier::Ctrl, Modifier::Shift],
                key: "F".into()
            }
        );
        assert_eq!(k("ctrl pressed F"), k("control F"));
        assert_eq!(k("alt down").key, "DOWN");
        assert_eq!(k("Alt Shift O"), k("alt shift O"));
        assert!(Keystroke::parse("").is_err());
        assert!(Keystroke::parse("hyper X").is_err());
        assert!(Keystroke::parse("shift pressed").is_err());
    }

    #[test]
    fn bundled_default_keymap() {
        let file = KeymapFile::parse(DEFAULT_XML).unwrap();
        assert_eq!(file.name, "$default");
        assert_eq!(file.actions.len(), 422);
        let map = Keymap::bundled_default();
        assert_eq!(
            map.shortcuts("SelectNextOccurrence"),
            [Shortcut::Keyboard {
                first: k("alt J"),
                second: None
            }]
        );
        assert!(matches!(
            &map.shortcuts("SearchEverywhere")[0],
            Shortcut::Gesture { modifier, .. } if modifier == "dblClick"
        ));
        assert_eq!(
            map.shortcuts("SearchEverywhere")[0].display(false),
            "double Shift"
        );
        assert!(
            map.shortcuts("ExpandToLevel1")
                .contains(&Shortcut::Keyboard {
                    first: k("control MULTIPLY"),
                    second: Some(k("1"))
                })
        );
        assert!(map.shortcuts("NoSuchAction").is_empty());
    }

    type Layout = std::collections::HashMap<char, char>;

    /// The German layout's keys for US keys as the toolkit reports them on macOS.
    fn german() -> Layout {
        [
            ('"', '`'),
            ('#', '§'),
            ('&', '/'),
            ('(', ')'),
            (')', '='),
            ('*', '('),
            ('/', 'ß'),
            (':', 'Ü'),
            (';', 'ü'),
            ('<', ';'),
            ('=', '*'),
            ('>', ':'),
            ('@', '"'),
            ('[', 'ö'),
            ('\'', '´'),
            ('\\', '#'),
            (']', 'ä'),
            ('^', '&'),
            ('`', '<'),
            ('{', 'Ö'),
            ('|', '\''),
            ('}', 'Ä'),
            ('~', '>'),
        ]
        .into_iter()
        .collect()
    }

    /// The name the toolkit gives a keymap key, the reverse of [`Keystroke::from_platform`].
    fn toolkit_key(key: &str) -> Option<String> {
        Some(match key {
            "UP" | "DOWN" | "LEFT" | "RIGHT" | "DELETE" | "ENTER" | "ESCAPE" | "TAB" | "HOME"
            | "END" | "INSERT" | "SPACE" => key.to_lowercase(),
            "BACK_SPACE" => "backspace".into(),
            "PAGE_UP" => "pageup".into(),
            "PAGE_DOWN" => "pagedown".into(),
            "SLASH" => "/".into(),
            "BACK_SLASH" => "\\".into(),
            "COMMA" => ",".into(),
            "PERIOD" => ".".into(),
            "SEMICOLON" => ";".into(),
            "EQUALS" => "=".into(),
            "MINUS" => "-".into(),
            "OPEN_BRACKET" => "[".into(),
            "CLOSE_BRACKET" => "]".into(),
            "QUOTE" => "'".into(),
            "BACK_QUOTE" => "`".into(),
            "ADD" => "+".into(),
            k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => k.to_lowercase(),
            k if k.starts_with('F') && k[1..].parse::<u8>().is_ok() => k.to_lowercase(),
            _ => return None,
        })
    }

    /// The event of `key` with the modifiers of `stroke` other than Shift.
    fn event(key: &str, stroke: &Keystroke, shift: bool, layout: Option<&Layout>) -> EventStrokes {
        let has = |m| stroke.modifiers.contains(&m);
        EventStrokes::of(
            key,
            has(Modifier::Ctrl),
            has(Modifier::Alt),
            shift,
            has(Modifier::Meta),
            layout,
        )
    }

    /// Every keystroke of the keyboard shortcuts of `keymap`, first and second ones.
    fn keystrokes(keymap: &Keymap) -> std::collections::BTreeSet<Keystroke> {
        let mut out = std::collections::BTreeSet::new();
        for (_, shortcuts) in keymap.actions() {
            for s in shortcuts {
                if let Shortcut::Keyboard { first, second } = s {
                    out.insert(first.clone());
                    out.extend(second.clone());
                }
            }
        }
        out
    }

    /// The character Shift types on the US key `key`, the reverse of [`us_unshifted`].
    fn shifted_us(key: &str) -> Option<char> {
        let key = key.chars().next().filter(|_| key.chars().count() == 1)?;
        "!@#$%^&*()_{}|:\"<>?~"
            .chars()
            .find(|c| us_unshifted(*c) == Some(key))
    }

    // SPEC-0028 v5 AK-9: ⌃⇧1 … ⌃⇧0 arrive as the characters the layout types with Shift, without
    // Shift, and reach the digit's chord on a US and on a German layout.
    #[test]
    fn shift_and_a_digit_is_the_digits_chord_on_us_and_german_layouts() {
        let german = german();
        let digits = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'];
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            for (layout, typed) in [(None, "!@#$%^&*()"), (Some(&german), "!\"§$%&/()=")] {
                for (c, digit) in typed.chars().zip(digits) {
                    let chord = k(&format!("control shift {digit}"));
                    // As macOS and Linux report it: the character, Shift taken out.
                    let strokes = event(&c.to_string(), &chord, false, layout);
                    assert_eq!(strokes.shifted.as_ref(), Some(&chord), "{c}");
                    let stroke = strokes.resolve(&keymap).unwrap();
                    assert_eq!(stroke, chord, "{c}");
                    assert_eq!(
                        keymap.actions_for(&stroke, None),
                        [format!("ToggleBookmark{digit}")],
                        "{c}"
                    );
                    assert_eq!(strokes.recorded(), Some(chord.clone()), "{c}");
                    // A toolkit that names the digit and keeps Shift gives the same chord.
                    let kept = event(&digit.to_string(), &chord, true, layout);
                    assert_eq!(kept.resolve(&keymap), Some(chord), "{digit}");
                }
                // Without Shift the digit keys keep their chords.
                for digit in digits {
                    let chord = k(&format!("control {digit}"));
                    let stroke = event(&digit.to_string(), &chord, false, layout).resolve(&keymap);
                    assert_eq!(stroke, Some(chord), "{digit}");
                }
            }
        }
        // ⇧⌘8 (column selection mode) on both layouts.
        let mac = Keymap::bundled_macos();
        let chord = k("shift meta 8");
        for (c, layout) in [("*", None), ("(", Some(&german))] {
            let stroke = event(c, &chord, false, layout).resolve(&mac).unwrap();
            assert_eq!(mac.actions_for(&stroke, None), ["EditorToggleColumnMode"]);
        }
        // A layout with the digits on Shift (French): ⌃⇧1 arrives as `1`, ⌃1 as `&`.
        let azerty: Layout = [('!', '1'), ('1', '&'), ('&', '7'), ('7', 'è')]
            .into_iter()
            .collect();
        let on = |key: &str| {
            EventStrokes::of(key, true, false, false, false, Some(&azerty)).resolve(&mac)
        };
        assert_eq!(on("1"), Some(k("control shift 1")));
        assert_eq!(on("&"), Some(k("control 1")));
        // Typing is not a chord: without ⌃, ⌥ or ⌘ the character stands for no key.
        let no_layout: Option<&Layout> = None;
        let plain = EventStrokes::of("!", false, false, false, false, no_layout);
        assert_eq!(plain, EventStrokes::default());
        assert_eq!(
            EventStrokes::of("\"", false, false, false, false, Some(&german)).resolve(&mac),
            None
        );
        // The other characters of Shift on a US keyboard name their key with Shift, where the
        // event has no keystroke otherwise.
        assert_eq!(
            EventStrokes::of("{", false, false, false, true, no_layout).resolve(&mac),
            Some(k("shift meta OPEN_BRACKET"))
        );
        assert_eq!(
            EventStrokes::of("?", true, false, false, false, no_layout).resolve(&mac),
            Some(k("control shift SLASH"))
        );
        assert_eq!(us_unshifted('+'), None);
        assert_eq!(us_unshifted('a'), None);
    }

    // SPEC-0028 v5 AK-10: every keystroke of the two bundled keymaps resolves as before from the
    // event that names its key, on a US and on a German layout; and every one with Shift and a
    // key whose character Shift changes also from the event that carries that character.
    #[test]
    fn bundled_keymaps_resolve_every_chord_as_before() {
        let german = german();
        // `/` and `=` are ⇧7 and ⇧0 on a German layout and nothing else; where a keymap binds
        // both the key and the digit's chord, the digit's wins there. The macOS keymap has no
        // such pair.
        let default_pairs = [
            "Ctrl+= -> Ctrl+Shift+0",
            "Ctrl+/ -> Ctrl+Shift+7",
            "Alt+/ -> Alt+Shift+7",
        ];
        for (keymap, pairs) in [
            (Keymap::bundled_macos(), &[][..]),
            (Keymap::bundled_default(), &default_pairs[..]),
        ] {
            let all = keystrokes(&keymap);
            let (mut named, mut from_characters) = (0, 0);
            let mut taken_by_digits = Vec::new();
            for stroke in &all {
                let Some(key) = toolkit_key(&stroke.key) else {
                    continue;
                };
                if stroke.modifiers.contains(&Modifier::AltGraph) {
                    continue;
                }
                named += 1;
                let shift = stroke.modifiers.contains(&Modifier::Shift);
                // Before v5: the key under its own name, which the keymap binds.
                let before = Keystroke::from_platform(
                    &key,
                    stroke.modifiers.contains(&Modifier::Ctrl),
                    stroke.modifiers.contains(&Modifier::Alt),
                    shift,
                    stroke.modifiers.contains(&Modifier::Meta),
                );
                assert_eq!(before.as_ref(), Some(stroke));
                assert_eq!(
                    event(&key, stroke, shift, None).resolve(&keymap),
                    before,
                    "{stroke}"
                );
                // On a German layout too, but for the pairs above.
                let resolved = event(&key, stroke, shift, Some(&german)).resolve(&keymap);
                if resolved != before {
                    taken_by_digits.push(format!("{stroke} -> {}", resolved.unwrap()));
                }
                // As the toolkit reports Shift and a digit or a punctuation key.
                if shift && let Some(c) = shifted_us(&key) {
                    from_characters += 1;
                    assert_eq!(
                        event(&c.to_string(), stroke, false, None).resolve(&keymap),
                        Some(stroke.clone()),
                        "{stroke} from {c}"
                    );
                }
            }
            assert!(named > 300, "{named} of {}", all.len());
            assert!(from_characters >= 10, "{from_characters}");
            assert_eq!(taken_by_digits, pairs);
        }
    }

    #[test]
    fn keystrokes_from_toolkit_events() {
        let k = |key, c, a, s, m| Keystroke::from_platform(key, c, a, s, m);
        assert_eq!(
            k("up", false, true, true, false),
            Keystroke::parse("shift alt UP").ok()
        );
        assert_eq!(
            k("backspace", false, false, false, true),
            Keystroke::parse("meta BACK_SPACE").ok()
        );
        assert_eq!(
            k("c", true, false, false, false),
            Keystroke::parse("control C").ok()
        );
        assert_eq!(
            k("/", true, false, false, false),
            Keystroke::parse("control SLASH").ok()
        );
        assert_eq!(
            k("f12", false, false, false, false),
            Keystroke::parse("F12").ok()
        );
        assert_eq!(
            k("pagedown", false, false, true, false),
            Keystroke::parse("shift PAGE_DOWN").ok()
        );
        assert_eq!(k("ä", false, false, false, false), None);
    }

    /// SPEC-0028 v4 behaviour 7, AK-6: the bundled macOS keymap differs from the upstream files
    /// exactly by the listed deviations, and the upstream files are the pinned ones.
    #[test]
    fn bundled_macos_keymap_differs_from_upstream_only_as_listed() {
        use std::collections::BTreeSet;
        let upstream = Keymap::upstream_macos();
        let ours = Keymap::bundled_macos();
        let entries = |k: &Keymap, id: &str| -> BTreeSet<String> {
            k.shortcuts(id).iter().map(Shortcut::to_entry).collect()
        };
        let ids: BTreeSet<&str> = upstream
            .actions()
            .chain(ours.actions())
            .map(|(id, _)| id)
            .collect();
        let mut found = BTreeSet::new();
        for id in ids {
            let (theirs, mine) = (entries(&upstream, id), entries(&ours, id));
            for s in mine.difference(&theirs) {
                found.insert((id.to_owned(), "add".to_owned(), s.clone()));
            }
            for s in theirs.difference(&mine) {
                found.insert((id.to_owned(), "remove".to_owned(), s.clone()));
            }
        }
        let listed = macos_deviations();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(listed, sorted, "keep macos-deviations.json sorted");
        for d in &listed {
            assert!(
                !d.decided_by.is_empty() && !d.reason.is_empty(),
                "{} needs a deciding document and a reason",
                d.action
            );
            // The entry is in the form the keymap writes, so that it compares.
            assert_eq!(
                Shortcut::from_entry(&d.shortcut).map(|s| s.to_entry()),
                Ok(d.shortcut.clone()),
                "{}",
                d.action
            );
        }
        let listed: BTreeSet<_> = listed
            .into_iter()
            .map(|d| (d.action, d.change, d.shortcut))
            .collect();
        let unlisted: Vec<_> = found.difference(&listed).collect();
        let stale: Vec<_> = listed.difference(&found).collect();
        assert!(
            unlisted.is_empty() && stale.is_empty(),
            "the bundled macOS keymap drifted from the pinned upstream keymap.\n\
             Not listed in crates/ui/keymaps/macos-deviations.json: {unlisted:#?}\n\
             Listed but not in the keymap: {stale:#?}\n\
             A deviation needs its entry there and in SPEC-0028; then run \
             `python3 -I scripts/keymap-coverage.py`."
        );
        // The two files are the copies taken at the pinned commit (keymaps/README.md): length
        // and FNV-1a of their bytes. A newer upstream revision changes these together with
        // README.md and docs/legal/PROVENANCE.md.
        let fnv = |text: &str| {
            text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            })
        };
        assert_eq!(
            (DEFAULT_XML.len(), fnv(DEFAULT_XML)),
            (45_895, 5_238_297_091_666_937_508),
            "$default.xml is not the pinned copy"
        );
        assert_eq!(
            (MACOS_XML.len(), fnv(MACOS_XML)),
            (27_043, 6_991_213_235_868_442_379),
            "Mac OS X 10.5+.xml is not the pinned copy"
        );
        // The in-selection toggle of the find bar has the key of the reference's action
        // (behaviour 8); before v4 it had none.
        let key = Keystroke::parse("control alt E").unwrap();
        for k in [&ours, &Keymap::bundled_default()] {
            let bound = k.actions_for(&key, None);
            assert!(bound.contains(&"ToggleFindInSelection"), "{bound:?}");
            assert!(bound.contains(&"ToggleInSelection"), "{bound:?}");
        }
        assert!(upstream.shortcuts("ToggleInSelection").is_empty());
    }

    #[test]
    fn prettier_has_the_plugin_shortcut() {
        let mac = Keymap::bundled_macos();
        let k = Keystroke::parse("alt shift meta P").unwrap();
        assert_eq!(mac.actions_for(&k, None), ["ReformatWithPrettierAction"]);
        let other = Keystroke::parse("control alt shift P").unwrap();
        assert_eq!(
            Keymap::bundled_default().actions_for(&other, None),
            ["ReformatWithPrettierAction"]
        );
    }

    #[test]
    fn completion_also_on_alt_escape_on_macos() {
        let mac = Keymap::bundled_macos();
        let alt_escape = Keystroke::parse("alt ESCAPE").unwrap();
        assert_eq!(mac.actions_for(&alt_escape, None), ["CodeCompletion"]);
        let ctrl_space = Keystroke::parse("control SPACE").unwrap();
        assert!(
            mac.actions_for(&ctrl_space, None)
                .contains(&"CodeCompletion")
        );
        assert!(
            Keymap::bundled_default()
                .actions_for(&alt_escape, None)
                .is_empty()
        );
    }

    #[test]
    fn resolved_macos_keymap() {
        let mac = Keymap::bundled_macos();
        assert_eq!(mac.name, "Mac OS X 10.5+");
        assert_eq!(mac.shortcuts("SelectNextOccurrence")[0].display(true), "⌃G");
        assert!(
            mac.shortcuts("GotoDeclaration")
                .iter()
                .any(|s| s.display(true) == "⌘B")
        );
        let child = KeymapFile::parse(MACOS_XML).unwrap();
        let default = Keymap::bundled_default();
        let removed: Vec<&String> = child
            .actions
            .iter()
            .filter(|(id, s)| s.is_empty() && !default.shortcuts(id).is_empty())
            .map(|(id, _)| id)
            .collect();
        assert!(!removed.is_empty());
        assert!(
            removed.iter().all(|id| mac.shortcuts(id).is_empty()),
            "listed empty in the macOS file"
        );
        // Listed several times in the macOS file: the last entry counts (SPEC-0028 v3).
        assert_eq!(mac.shortcuts("RunClass").len(), 1);
        assert_eq!(mac.shortcuts("RunClass")[0].display(true), "⌃⇧R");
        assert_eq!(mac.shortcuts("DebugClass")[0].display(true), "⌃⇧D");
        assert_eq!(mac.shortcuts("DebugClass").len(), 1);
        let find_next: Vec<String> = mac
            .shortcuts("FindNext")
            .iter()
            .map(|s| s.display(true))
            .collect();
        assert_eq!(find_next, ["⌘G"]);
        let inherited = Keymap::bundled_default()
            .actions
            .keys()
            .find(|id| !child.actions.contains_key(*id))
            .unwrap()
            .clone();
        let swapped: Vec<Shortcut> = Keymap::bundled_default()
            .shortcuts(&inherited)
            .iter()
            .map(Shortcut::swap_control_meta)
            .collect();
        assert_eq!(mac.shortcuts(&inherited), swapped.as_slice());
        // Inherited shortcuts swap Control and Meta: Ctrl+C becomes Cmd+C.
        let copy = Keystroke::parse("meta C").unwrap();
        assert!(mac.actions_for(&copy, None).contains(&"$Copy"));
        assert!(
            !mac.actions_for(&Keystroke::parse("control C").unwrap(), None)
                .contains(&"$Copy")
        );
        assert!(
            mac.actions_for(&Keystroke::parse("meta Z").unwrap(), None)
                .contains(&"$Undo")
        );
        let mouse = Shortcut::Mouse {
            keystroke: "control button1".into(),
        };
        assert_eq!(
            mouse.swap_control_meta(),
            Shortcut::Mouse {
                keystroke: "meta button1".into()
            }
        );
    }

    /// SPEC-0130 AK-5 (v2): the editor's two clicks are mouse shortcuts of the keymap.
    #[test]
    fn mouse_shortcuts_name_their_actions() {
        const COLUMN: &str = "EditorCreateRectangularSelectionOnMouseDrag";
        const CARET: &str = "EditorAddOrRemoveCaret";
        let held = |alt: bool, shift: bool| MouseModifiers {
            alt,
            shift,
            ..MouseModifiers::default()
        };
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            let alt = keymap.mouse_actions(1, false, held(true, false));
            assert!(alt.contains(&COLUMN) && !alt.contains(&CARET), "{alt:?}");
            let both = keymap.mouse_actions(1, false, held(true, true));
            assert!(both.contains(&CARET) && !both.contains(&COLUMN), "{both:?}");
            for other in [
                keymap.mouse_actions(1, false, held(false, false)),
                keymap.mouse_actions(1, false, held(false, true)),
                keymap.mouse_actions(1, true, held(true, true)),
                keymap.mouse_actions(3, false, held(true, true)),
            ] {
                assert!(
                    !other.contains(&COLUMN) && !other.contains(&CARET),
                    "{other:?}"
                );
            }
            // The middle button is the action's second mouse shortcut.
            assert!(
                keymap
                    .mouse_actions(2, false, held(false, false))
                    .contains(&COLUMN)
            );
            // A double click is a shortcut of its own.
            assert!(
                keymap
                    .mouse_actions(1, true, held(true, false))
                    .contains(&"OpenInRightSplit")
            );
        }
        // A mouse shortcut the user gives is followed, and the bundled one is gone.
        let mut user = KeymapFile::user("mine", platform_parent());
        user.set_action(
            CARET,
            Some(vec![Shortcut::Mouse {
                keystroke: "meta alt button1".into(),
            }]),
        );
        let keymap = Keymap::bundled_macos().with_overrides(&user);
        let meta_alt = MouseModifiers {
            alt: true,
            meta: true,
            ..MouseModifiers::default()
        };
        assert!(keymap.mouse_actions(1, false, meta_alt).contains(&CARET));
        assert!(
            !keymap
                .mouse_actions(1, false, held(true, true))
                .contains(&CARET)
        );
        assert_eq!(mouse_click("alt wheel"), None);
        assert_eq!(mouse_click("alt"), None);
    }

    #[test]
    fn reverse_lookup_and_display() {
        let map = Keymap::bundled_default();
        assert!(
            map.actions_for(&k("alt J"), None)
                .contains(&"SelectNextOccurrence")
        );
        let level = map.actions_for(&k("control MULTIPLY"), Some(&k("2")));
        assert_eq!(level, ["ExpandToLevel2"]);
        assert_eq!(
            k("control alt shift meta BACK_SPACE").display(false),
            "Ctrl+Alt+Shift+Meta+Backspace"
        );
        assert_eq!(
            k("meta shift alt control BACK_SPACE").display(true),
            "⌃⌥⇧⌘⌫"
        );
        assert_eq!(k("control F12").display(false), "Ctrl+F12");
        assert_eq!(k("control PAGE_DOWN").display(false), "Ctrl+Page Down");
        let two = Shortcut::Keyboard {
            first: k("control K"),
            second: Some(k("control C")),
        };
        assert_eq!(two.display(false), "Ctrl+K, Ctrl+C");
    }

    #[test]
    fn user_keymap_round_trips_and_overrides() {
        for text in ["control shift F", "meta alt BACK_QUOTE", "F12"] {
            let k = Keystroke::parse(text).unwrap();
            assert_eq!(Keystroke::parse(&k.to_text()).unwrap(), k);
        }
        let mut user = KeymapFile::user("Ferris IDEA", "$default");
        user.actions.insert(
            "SaveAll".into(),
            vec![Shortcut::Keyboard {
                first: Keystroke::parse("control alt S").unwrap(),
                second: Some(Keystroke::parse("control alt A").unwrap()),
            }],
        );
        user.actions.insert("ShowSettings".into(), Vec::new());
        user.actions.insert(
            "EditorSelectWord".into(),
            vec![Shortcut::Mouse {
                keystroke: "alt button1 doubleClick".into(),
            }],
        );
        assert_eq!(
            Shortcut::parse_keyboard("control K, control C").unwrap(),
            Shortcut::Keyboard {
                first: Keystroke::parse("control K").unwrap(),
                second: Some(Keystroke::parse("control C").unwrap()),
            }
        );
        let back = KeymapFile::parse(&user.to_xml()).unwrap();
        assert_eq!(back, user);
        user.actions.insert(
            "GotoClass".into(),
            vec![Shortcut::Gesture {
                keystroke: "shift SHIFT".into(),
                modifier: "dblClick".into(),
            }],
        );
        let toml_back = KeymapFile::parse_toml(&user.to_toml_text()).unwrap();
        assert_eq!(toml_back, user);
        user.actions.remove("GotoClass");
        let keymap = Keymap::bundled_default();
        assert!(!keymap.shortcuts("ShowSettings").is_empty());
        let custom = keymap.with_overrides(&user);
        assert!(custom.shortcuts("ShowSettings").is_empty(), "removed");
        assert_eq!(custom.shortcuts("SaveAll").len(), 1);
        assert_eq!(custom.name, "Ferris IDEA");
        let ctrl_alt_s = Keystroke::parse("control alt S").unwrap();
        assert!(custom.actions_for(&ctrl_alt_s, None).contains(&"SaveAll"));
    }

    /// SPEC-0356 AK-6 (behaviour 8): a user keymap saved under the product's former name keeps
    /// its shortcuts and reads with today's name.
    #[test]
    fn a_keymap_saved_under_the_former_name_reads_with_todays_name() {
        let dir = std::env::temp_dir().join(format!("keymap-former-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("keymaps/user.toml");
        let former = "IDECraft"; // rebrand:keep
        let mut old = KeymapFile::user(former, "$default");
        old.set_action(
            "SaveAll",
            Some(vec![Shortcut::parse_keyboard("control alt S").unwrap()]),
        );
        old.write(&file).unwrap();
        let read = KeymapFile::read_user(Some(&file));
        assert_eq!(read.name, ferris_idea_core::takeover::USER_KEYMAP_NAME);
        assert_eq!(read.actions, old.actions);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0303 AK-5: the XML keymap of older versions is read until the TOML keymap is saved,
    /// then kept once as `FerrisIDEA.xml.bak`.
    #[test]
    fn xml_keymaps_migrate_to_toml() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-keymap-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("keymaps")).unwrap();
        let file = dir.join("keymaps/FerrisIDEA.toml");
        let mut old = KeymapFile::user("Ferris IDEA", "$default");
        old.set_action(
            "SaveAll",
            Some(vec![Shortcut::parse_keyboard("control alt S").unwrap()]),
        );
        std::fs::write(dir.join("keymaps/FerrisIDEA.xml"), old.to_xml()).unwrap();
        let mut read = KeymapFile::read_user(Some(&file));
        // The keymap of an older version is read under the current name (takeover, rename step 2).
        assert_eq!(
            read.actions, old.actions,
            "the XML file is read while there is no TOML file"
        );
        assert_eq!(read.name, ferris_idea_core::takeover::USER_KEYMAP_NAME);
        read.set_action("ShowSettings", Some(vec![]));
        read.write(&file).unwrap();
        assert!(!dir.join("keymaps/FerrisIDEA.xml").exists());
        assert!(dir.join("keymaps/FerrisIDEA.xml.bak").exists());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.starts_with("[ferris-idea]\nkind = \"keymap\"\nformat = 1\n"),
            "{text}"
        );
        assert!(text.contains("SaveAll = [\"control alt S\"]"), "{text}");
        assert_eq!(KeymapFile::read_user(Some(&file)), read);
        assert!(Shortcut::from_entry("control banana K").is_err());
        assert!(Shortcut::from_entry("mouse:").is_err());
        std::fs::write(&file, text.replace("format = 1", "format = 4")).unwrap();
        assert!(read.write(&file).is_err(), "a newer keymap is not replaced");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
