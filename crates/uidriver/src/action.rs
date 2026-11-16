//! Actions (`SPEC-0319` behaviours 16-21) and the input events they become. Input is delivered
//! only into IDECraft's own window, on one of three levels (ADR-0027 section 3); this module only
//! plans the events, the window delivers them.

use serde::{Deserialize, Serialize};

use crate::error::UiError;
use crate::tree::ElementId;

/// A pointer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    /// Primary.
    #[default]
    Left,
    /// Secondary.
    Right,
    /// Middle.
    Middle,
}

/// A key modifier as the keymap writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    /// ⌘ on macOS (`cmd`), the platform key elsewhere.
    Cmd,
    /// Control.
    Ctrl,
    /// ⌥ Option / Alt.
    Alt,
    /// Shift.
    Shift,
    /// Fn.
    Fn,
}

impl Modifier {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" | "super" | "platform" | "win" => Some(Self::Cmd),
            "ctrl" | "control" => Some(Self::Ctrl),
            "alt" | "option" | "opt" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "fn" | "function" => Some(Self::Fn),
            _ => None,
        }
    }

    /// The name as the keymap writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
            Self::Fn => "fn",
        }
    }
}

/// One key press: modifiers and the key, as the keymap writes it (`cmd-shift-a`, `alt-f12`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPress {
    /// Held modifiers, sorted.
    pub modifiers: Vec<Modifier>,
    /// The key (`a`, `[`, `f12`, `enter`, `escape`, `left`).
    pub key: String,
}

impl KeyPress {
    /// The keymap form, `cmd-shift-a`.
    pub fn to_text(&self) -> String {
        let mut parts: Vec<&str> = self.modifiers.iter().map(|m| m.name()).collect();
        parts.push(&self.key);
        parts.join("-")
    }

    /// Whether `m` is held.
    pub fn has(&self, m: Modifier) -> bool {
        self.modifiers.contains(&m)
    }

    /// Whether it pastes from the system clipboard (⌘V, Ctrl+V and their variants, Shift+Insert):
    /// reserved for the user (behaviour 35).
    pub fn is_paste(&self) -> bool {
        (self.key == "v" && (self.has(Modifier::Cmd) || self.has(Modifier::Ctrl)))
            || (self.key == "insert" && self.has(Modifier::Shift))
    }
}

/// Parses keys as the keymap writes them: `cmd-shift-a`, a sequence `cmd-k cmd-s`.
pub fn parse_keys(text: &str) -> Result<Vec<KeyPress>, UiError> {
    let mut out = Vec::new();
    for chord in text.split_whitespace() {
        let mut parts: Vec<&str> = chord.split('-').collect();
        // `cmd--` and `-`: the key is the minus sign.
        let key = match parts.pop() {
            Some("") if parts.last() == Some(&"") || parts.is_empty() => {
                parts.pop();
                "-".to_owned()
            }
            Some("") => return Err(UiError::invalid(format!("invalid: no key in `{chord}`"))),
            Some(k) => k.to_ascii_lowercase(),
            None => return Err(UiError::invalid("invalid: no keys")),
        };
        let mut modifiers = Vec::new();
        for p in parts {
            let m = Modifier::parse(p).ok_or_else(|| {
                UiError::invalid(format!("invalid: `{p}` in `{chord}` is not a modifier"))
            })?;
            if !modifiers.contains(&m) {
                modifiers.push(m);
            }
        }
        modifiers.sort();
        out.push(KeyPress { modifiers, key });
    }
    if out.is_empty() {
        return Err(UiError::invalid("invalid: no keys"));
    }
    Ok(out)
}

/// A point relative to an element's top left, or an offset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
}

/// Units of a scroll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollUnit {
    /// Lines.
    #[default]
    Lines,
    /// Pixels.
    Pixels,
}

fn one() -> u8 {
    1
}

/// What to do with an element (or the window, for `press`).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Click with `button`, `count` times (1-3), with `modifiers`.
    Click {
        /// Button.
        #[serde(default)]
        button: Button,
        /// 1-3.
        #[serde(default = "one")]
        count: u8,
        /// Held modifiers.
        #[serde(default)]
        modifiers: Vec<Modifier>,
        /// Point relative to the element's top left (default: centre of its visible part).
        #[serde(default)]
        position: Option<Point>,
    },
    /// The secondary button; with level `native` and `control: true`, a Control-click.
    ContextClick {
        /// Point relative to the element.
        #[serde(default)]
        position: Option<Point>,
        /// Control + primary button instead of the secondary button (native level).
        #[serde(default)]
        control: bool,
    },
    /// The pointer moves onto the element and rests.
    Hover {
        /// Point relative to the element.
        #[serde(default)]
        position: Option<Point>,
    },
    /// Press on the element, move, release on `to` or at `offset`.
    Drag {
        /// Locator of the target (JSON or text form).
        #[serde(default)]
        to: Option<serde_json::Value>,
        /// Offset from the start point.
        #[serde(default)]
        offset: Option<Point>,
    },
    /// Scroll over the element.
    Scroll {
        /// Horizontal delta.
        #[serde(default)]
        dx: f32,
        /// Vertical delta.
        #[serde(default)]
        dy: f32,
        /// Lines or pixels.
        #[serde(default)]
        unit: ScrollUnit,
    },
    /// Scroll the element into view.
    ScrollIntoView,
    /// Keys as the keymap writes them; with an element, focused first.
    Press {
        /// `cmd-shift-a`, `alt-f12`, `cmd-k cmd-s`.
        keys: String,
    },
    /// Unicode text, committed through the window's input handler.
    Type {
        /// The text (logged only as its length).
        text: String,
        /// Select and delete the existing value first.
        #[serde(default)]
        clear: bool,
    },
    /// Move the focus to the element.
    Focus,
    /// Select it.
    Select,
    /// Expand it.
    Expand,
    /// Collapse it.
    Collapse,
    /// AccessKit `Click` without a pointer.
    Invoke,
    /// Open its context menu without a pointer.
    ShowContextMenu,
    /// One step up.
    Increment,
    /// One step down.
    Decrement,
    /// Set its value (logged only as its length).
    SetValue {
        /// The value.
        value: String,
    },
}

impl std::fmt::Debug for Action {
    /// Typed text and set values never appear, only their length (behaviour 38).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

/// The level input is delivered on (behaviour 20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// AccessKit action requests.
    Semantic,
    /// Toolkit input events dispatched into the window.
    #[default]
    Window,
    /// `NSEvent`s posted to IDECraft's own application queue (macOS, opt-in).
    Native,
}

/// What the action needs of its target before it runs (behaviour 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// In the tree.
    Attached,
    /// Non-empty bounds after clipping.
    Visible,
    /// Same bounds in two consecutive frames.
    Stable,
    /// Enabled.
    Enabled,
    /// Takes text.
    Editable,
    /// The hit test at the action point lands on it or a descendant.
    ReceivesEvents,
}

impl Check {
    /// The name in errors and traces.
    pub fn name(self) -> &'static str {
        match self {
            Self::Attached => "attached",
            Self::Visible => "visible",
            Self::Stable => "stable",
            Self::Enabled => "enabled",
            Self::Editable => "editable",
            Self::ReceivesEvents => "receives_events",
        }
    }
}

impl Action {
    /// Short name (`click`, `type`).
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Click { .. } => "click",
            Self::ContextClick { .. } => "context_click",
            Self::Hover { .. } => "hover",
            Self::Drag { .. } => "drag",
            Self::Scroll { .. } => "scroll",
            Self::ScrollIntoView => "scroll_into_view",
            Self::Press { .. } => "press",
            Self::Type { .. } => "type",
            Self::Focus => "focus",
            Self::Select => "select",
            Self::Expand => "expand",
            Self::Collapse => "collapse",
            Self::Invoke => "invoke",
            Self::ShowContextMenu => "show_context_menu",
            Self::Increment => "increment",
            Self::Decrement => "decrement",
            Self::SetValue { .. } => "set_value",
        }
    }

    /// The action for logs and traces: typed text and values only as their length.
    pub fn summary(&self) -> String {
        match self {
            Self::Type { text, clear } => format!(
                "type ({} characters{})",
                text.chars().count(),
                if *clear { ", clear" } else { "" }
            ),
            Self::SetValue { value } => format!("set_value ({} characters)", value.chars().count()),
            Self::Press { keys } => format!("press {keys}"),
            Self::Click {
                button,
                count,
                modifiers,
                ..
            } => {
                let mods: Vec<&str> = modifiers.iter().map(|m| m.name()).collect();
                format!(
                    "click {button:?} x{count}{}",
                    if mods.is_empty() {
                        String::new()
                    } else {
                        format!(" with {}", mods.join("-"))
                    }
                )
            }
            other => other.verb().to_owned(),
        }
    }

    /// Number of characters it types (for the audit), if it types.
    pub fn typed_len(&self) -> Option<usize> {
        match self {
            Self::Type { text, .. } => Some(text.chars().count()),
            Self::SetValue { value } => Some(value.chars().count()),
            _ => None,
        }
    }

    /// Whether it moves the pointer.
    pub fn is_pointer(&self) -> bool {
        matches!(
            self,
            Self::Click { .. }
                | Self::ContextClick { .. }
                | Self::Hover { .. }
                | Self::Drag { .. }
                | Self::Scroll { .. }
        )
    }

    /// Whether it needs an element (`press` does not).
    pub fn needs_element(&self) -> bool {
        !matches!(self, Self::Press { .. })
    }

    /// The level it uses when none is asked for: semantic actions use the semantic level.
    pub fn default_level(&self) -> Level {
        match self {
            Self::Focus
            | Self::Select
            | Self::Expand
            | Self::Collapse
            | Self::Invoke
            | Self::ShowContextMenu
            | Self::Increment
            | Self::Decrement
            | Self::SetValue { .. }
            | Self::ScrollIntoView => Level::Semantic,
            _ => Level::Window,
        }
    }

    /// The checks before it runs; `force` keeps only `attached` (behaviour 12).
    pub fn checks(&self, force: bool) -> Vec<Check> {
        use Check::*;
        if force {
            return vec![Attached];
        }
        match self {
            Self::Click { .. } | Self::ContextClick { .. } | Self::Drag { .. } => {
                vec![Attached, Visible, Stable, Enabled, ReceivesEvents]
            }
            Self::Hover { .. } | Self::Scroll { .. } => {
                vec![Attached, Visible, Stable, ReceivesEvents]
            }
            Self::Type { .. } | Self::SetValue { .. } => {
                vec![Attached, Visible, Enabled, Editable]
            }
            Self::Press { .. } => vec![Attached, Visible, Enabled],
            Self::ScrollIntoView | Self::Focus => vec![Attached],
            _ => vec![Attached, Enabled],
        }
    }

    /// Validates the arguments.
    pub fn validate(&self) -> Result<(), UiError> {
        match self {
            Self::Click { count, .. } if !(1..=3).contains(count) => {
                Err(UiError::invalid("invalid: `count` is 1, 2 or 3"))
            }
            Self::Press { keys } => parse_keys(keys).map(|_| ()),
            Self::Drag {
                to: None,
                offset: None,
            } => Err(UiError::invalid("invalid: `drag` needs `to` or `offset`")),
            _ => Ok(()),
        }
    }
}

/// One input event to deliver, toolkit-free.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "input", rename_all = "snake_case")]
pub enum Input {
    /// The pointer moves to `x`, `y` (window coordinates), `pressed` held.
    PointerMove {
        /// X.
        x: f32,
        /// Y.
        y: f32,
        /// Button held while moving.
        pressed: Option<Button>,
        /// Modifiers.
        modifiers: Vec<Modifier>,
    },
    /// A button goes down.
    PointerDown {
        /// X.
        x: f32,
        /// Y.
        y: f32,
        /// Button.
        button: Button,
        /// Click count (2 on the second press of a double click).
        count: u8,
        /// Modifiers.
        modifiers: Vec<Modifier>,
    },
    /// A button goes up.
    PointerUp {
        /// X.
        x: f32,
        /// Y.
        y: f32,
        /// Button.
        button: Button,
        /// Click count.
        count: u8,
        /// Modifiers.
        modifiers: Vec<Modifier>,
    },
    /// A scroll at `x`, `y`.
    Scroll {
        /// X.
        x: f32,
        /// Y.
        y: f32,
        /// Horizontal delta.
        dx: f32,
        /// Vertical delta.
        dy: f32,
        /// Unit.
        unit: ScrollUnit,
    },
    /// A key press (down and up), delivered as the key with its modifiers, never as the character
    /// a layout would type for an Option combination.
    Key(KeyPress),
    /// Text committed through the input handler.
    Text {
        /// The text.
        text: String,
    },
    /// An AccessKit action request.
    Semantic {
        /// Target node.
        element: ElementId,
        /// The action, by name (`click`, `focus`, `expand`, `set-value`).
        action: String,
        /// Its value (`set-value`).
        value: Option<String>,
    },
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary().to_string())
    }
}

impl Input {
    /// The event for traces: text and values only as their length.
    pub fn summary(&self) -> serde_json::Value {
        match self {
            Self::Text { text } => {
                serde_json::json!({"input": "text", "length": text.chars().count()})
            }
            Self::Semantic {
                element,
                action,
                value,
            } => serde_json::json!({
                "input": "semantic", "element": element, "action": action,
                "length": value.as_ref().map(|v| v.chars().count()),
            }),
            other => serde_json::to_value(other).unwrap_or_default(),
        }
    }
}

/// The input events of a pointer action at `(x, y)` (behaviour 16). `to` is the end of a drag.
pub fn pointer_inputs(action: &Action, x: f32, y: f32, to: Option<(f32, f32)>) -> Vec<Input> {
    let mv = |x, y, pressed, modifiers: &[Modifier]| Input::PointerMove {
        x,
        y,
        pressed,
        modifiers: modifiers.to_vec(),
    };
    match action {
        Action::Click {
            button,
            count,
            modifiers,
            ..
        } => {
            let mut out = vec![mv(x, y, None, modifiers)];
            for n in 1..=*count {
                out.push(Input::PointerDown {
                    x,
                    y,
                    button: *button,
                    count: n,
                    modifiers: modifiers.clone(),
                });
                out.push(Input::PointerUp {
                    x,
                    y,
                    button: *button,
                    count: n,
                    modifiers: modifiers.clone(),
                });
            }
            out
        }
        Action::ContextClick { control, .. } => {
            let (button, modifiers) = if *control {
                (Button::Left, vec![Modifier::Ctrl])
            } else {
                (Button::Right, Vec::new())
            };
            vec![
                mv(x, y, None, &modifiers),
                Input::PointerDown {
                    x,
                    y,
                    button,
                    count: 1,
                    modifiers: modifiers.clone(),
                },
                Input::PointerUp {
                    x,
                    y,
                    button,
                    count: 1,
                    modifiers,
                },
            ]
        }
        Action::Hover { .. } => vec![mv(x, y, None, &[])],
        Action::Drag { .. } => {
            let (tx, ty) = to.unwrap_or((x, y));
            let mut out = vec![
                mv(x, y, None, &[]),
                Input::PointerDown {
                    x,
                    y,
                    button: Button::Left,
                    count: 1,
                    modifiers: Vec::new(),
                },
            ];
            const STEPS: u8 = 5;
            for i in 1..=STEPS {
                let f = f32::from(i) / f32::from(STEPS);
                out.push(mv(
                    x + (tx - x) * f,
                    y + (ty - y) * f,
                    Some(Button::Left),
                    &[],
                ));
            }
            out.push(Input::PointerUp {
                x: tx,
                y: ty,
                button: Button::Left,
                count: 1,
                modifiers: Vec::new(),
            });
            out
        }
        Action::Scroll { dx, dy, unit } => vec![
            mv(x, y, None, &[]),
            Input::Scroll {
                x,
                y,
                dx: *dx,
                dy: *dy,
                unit: *unit,
            },
        ],
        _ => Vec::new(),
    }
}

/// The primary modifier of this platform (⌘ on macOS, Ctrl elsewhere).
pub fn primary() -> Modifier {
    if cfg!(target_os = "macos") {
        Modifier::Cmd
    } else {
        Modifier::Ctrl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_as_the_keymap_writes_them() {
        let k = parse_keys("cmd-shift-a").unwrap();
        assert_eq!(k[0].modifiers, vec![Modifier::Cmd, Modifier::Shift]);
        assert_eq!(k[0].key, "a");
        let seq = parse_keys("cmd-k cmd-s").unwrap();
        assert_eq!(seq.len(), 2);
        assert_eq!(seq[1].to_text(), "cmd-s");
        assert_eq!(parse_keys("alt-F12").unwrap()[0].to_text(), "alt-f12");
        assert_eq!(parse_keys("option-left").unwrap()[0].to_text(), "alt-left");
        assert_eq!(parse_keys("cmd--").unwrap()[0].to_text(), "cmd--");
        assert_eq!(parse_keys("cmd-[").unwrap()[0].key, "[");
        assert!(parse_keys("hyper-a").is_err());
        assert!(parse_keys("  ").is_err());
        assert!(parse_keys("cmd-v").unwrap()[0].is_paste());
        assert!(parse_keys("ctrl-shift-v").unwrap()[0].is_paste());
        assert!(parse_keys("shift-insert").unwrap()[0].is_paste());
        assert!(!parse_keys("alt-v").unwrap()[0].is_paste());
    }

    #[test]
    fn typed_text_never_shows_in_debug_or_summaries() {
        let a = Action::Type {
            text: "the-known-typed-text".into(),
            clear: true,
        };
        assert!(!format!("{a:?}").contains("known"));
        assert_eq!(a.summary(), "type (20 characters, clear)");
        let i = Input::Text {
            text: "the-known-typed-text".into(),
        };
        assert!(!format!("{i:?}").contains("known"));
        assert_eq!(i.summary()["length"], 20);
    }

    #[test]
    fn actions_parse_from_json() {
        let a: Action = serde_json::from_value(
            serde_json::json!({"action": "click", "count": 2, "modifiers": ["alt"]}),
        )
        .unwrap();
        assert_eq!(
            a,
            Action::Click {
                button: Button::Left,
                count: 2,
                modifiers: vec![Modifier::Alt],
                position: None
            }
        );
        let inputs = pointer_inputs(&a, 5., 6., None);
        assert_eq!(inputs.len(), 5);
        assert!(matches!(inputs[3], Input::PointerDown { count: 2, .. }));
        let p: Action =
            serde_json::from_value(serde_json::json!({"action": "press", "keys": "alt-f12"}))
                .unwrap();
        assert!(p.validate().is_ok());
        assert_eq!(p.default_level(), Level::Window);
        assert_eq!(Action::Expand.default_level(), Level::Semantic);
        let bad: Action =
            serde_json::from_value(serde_json::json!({"action": "click", "count": 4})).unwrap();
        assert!(bad.validate().is_err());
        let drag = Action::Drag {
            to: None,
            offset: Some(Point { x: 10., y: 0. }),
        };
        let d = pointer_inputs(&drag, 0., 0., Some((10., 0.)));
        assert!(matches!(d.last(), Some(Input::PointerUp { x, .. }) if (*x - 10.).abs() < 0.01));
        assert_eq!(
            Action::Click {
                button: Button::Left,
                count: 1,
                modifiers: vec![],
                position: None
            }
            .checks(true),
            vec![Check::Attached]
        );
    }
}
