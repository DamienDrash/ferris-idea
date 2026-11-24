//! Settings › Advanced Settings (`SPEC-0337` behaviour 5): the settings of `settings.toml` that no
//! other page shows, as one flat list with label, key, value, default and reset. The dialog and
//! the MCP tools `ide_settings` and `set_ide_settings` read and change them through this list.
//!
//! Security first (behaviour 6): the list holds no setting whose change weakens security; the
//! SSH, MCP, UI automation and trust switches keep their own pages and warnings.

use crate::IdeSettings;

/// Default size budget of one chat attachment in characters (`ai.attachment_chars`,
/// `SPEC-0313` behaviour 7).
pub const DEFAULT_ATTACHMENT_CHARS: usize = 24_000;
/// Smallest and largest attachment budget.
pub const ATTACHMENT_CHARS_RANGE: (usize, usize) = (1_000, 200_000);

/// What kind of value a setting holds and how the dialog changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// On or off: Enter, Space or a click switch it.
    Switch,
    /// A whole number within `min..=max`; Left/Right change it by `step`.
    Number {
        /// Smallest value.
        min: u64,
        /// Largest value.
        max: u64,
        /// One step of Left/Right.
        step: u64,
    },
    /// A text, edited in a field and checked before it counts.
    Text,
}

/// A value of an advanced setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A switch.
    Switch(bool),
    /// A number.
    Number(u64),
    /// A text.
    Text(String),
}

impl Value {
    /// The value as the dialog shows it.
    pub fn shown(&self) -> String {
        match self {
            Value::Switch(true) => "on".into(),
            Value::Switch(false) => "off".into(),
            Value::Number(n) => n.to_string(),
            Value::Text(t) if t.is_empty() => "(empty)".into(),
            Value::Text(t) => t.clone(),
        }
    }

    /// The value as JSON (MCP).
    pub fn json(&self) -> serde_json::Value {
        match self {
            Value::Switch(b) => (*b).into(),
            Value::Number(n) => (*n).into(),
            Value::Text(t) => t.clone().into(),
        }
    }
}

/// One row of Advanced Settings.
pub struct AdvancedSetting {
    /// Key in `settings.toml`, e.g. `project_view.compact_middle_packages`.
    pub key: &'static str,
    /// What the row says.
    pub label: &'static str,
    /// One sentence of what it does.
    pub description: &'static str,
    /// Its kind.
    pub kind: Kind,
    get: fn(&IdeSettings) -> Value,
    put: fn(&mut IdeSettings, Value),
}

impl std::fmt::Debug for AdvancedSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdvancedSetting")
            .field("key", &self.key)
            .finish()
    }
}

/// The rows, in the order the page lists them (behaviour 5).
pub const ADVANCED: &[AdvancedSetting] = &[
    AdvancedSetting {
        key: "project_view.compact_middle_packages",
        label: "Compact middle packages in the project tree",
        description: "A package of a source folder that holds only one sub-package shares its row with it.",
        kind: Kind::Switch,
        get: |s| Value::Switch(s.project_view.compact_middle_packages),
        put: |s, v| {
            if let Value::Switch(b) = v {
                s.project_view.compact_middle_packages = b;
            }
        },
    },
    AdvancedSetting {
        key: "ai.attachment_chars",
        label: "Largest chat attachment",
        description: "How many characters of code or output one attachment to the AI chat may carry.",
        kind: Kind::Number {
            min: ATTACHMENT_CHARS_RANGE.0 as u64,
            max: ATTACHMENT_CHARS_RANGE.1 as u64,
            step: 1_000,
        },
        get: |s| Value::Number(attachment_chars(s.ai.attachment_chars) as u64),
        put: |s, v| {
            if let Value::Number(n) = v {
                // The default is kept as 0, as a file without the key has it.
                s.ai.attachment_chars = if n as usize == DEFAULT_ATTACHMENT_CHARS {
                    0
                } else {
                    n as usize
                };
            }
        },
    },
    AdvancedSetting {
        key: "database.report_url",
        label: "Address for database problem reports",
        description: "Empty for Ferris IDEA's issue template, off for copying the report only, else an https address with {title} and {body}.",
        kind: Kind::Text,
        get: |s| Value::Text(s.database.report_url.clone()),
        put: |s, v| {
            if let Value::Text(t) = v {
                s.database.report_url = t;
            }
        },
    },
];

/// `ai.attachment_chars` within its range; 0 (unset) is the default.
pub fn attachment_chars(setting: usize) -> usize {
    if setting == 0 {
        DEFAULT_ATTACHMENT_CHARS
    } else {
        setting.clamp(ATTACHMENT_CHARS_RANGE.0, ATTACHMENT_CHARS_RANGE.1)
    }
}

/// Checks a report address (`SPEC-0301`): empty, `off`, or an `https://` template with `{title}`
/// and `{body}`.
pub fn check_report_url(template: &str) -> Result<(), String> {
    let t = template.trim();
    if t.is_empty() || t == "off" {
        return Ok(());
    }
    if !t.starts_with("https://") {
        return Err("the report address must start with https://".into());
    }
    if !t.contains("{title}") || !t.contains("{body}") {
        return Err("the report address needs {title} and {body}".into());
    }
    Ok(())
}

/// The row of `key`.
pub fn find(key: &str) -> Option<&'static AdvancedSetting> {
    ADVANCED.iter().find(|a| a.key == key)
}

/// The rows the page lists for the dialog's search `query` (behaviour 5a): all for an empty query
/// or one that names the page, else those whose label, key or sentence contains it.
pub fn rows(query: &str) -> Vec<&'static AdvancedSetting> {
    let q = query.trim().to_lowercase();
    if q.is_empty() || "advanced settings".contains(&q) {
        return ADVANCED.iter().collect();
    }
    ADVANCED.iter().filter(|a| a.matches(&q)).collect()
}

impl AdvancedSetting {
    /// The value in `settings`.
    pub fn value(&self, settings: &IdeSettings) -> Value {
        (self.get)(settings)
    }

    /// The default value.
    pub fn default_value(&self) -> Value {
        (self.get)(&IdeSettings::default())
    }

    /// Whether `settings` hold another value than the default.
    pub fn modified(&self, settings: &IdeSettings) -> bool {
        self.value(settings) != self.default_value()
    }

    /// Whether the label, key or sentence contains `query` (lower case).
    pub fn matches(&self, query: &str) -> bool {
        [self.label, self.key, self.description]
            .iter()
            .any(|t| t.to_lowercase().contains(query))
    }

    /// Gives `settings` the value `value`: a number is clamped, a text checked; a value of the
    /// wrong kind or an invalid text is refused and nothing changes.
    pub fn set(&self, settings: &mut IdeSettings, value: Value) -> Result<(), String> {
        let value = match (self.kind, value) {
            (Kind::Switch, v @ Value::Switch(_)) => v,
            (Kind::Number { min, max, .. }, Value::Number(n)) => Value::Number(n.clamp(min, max)),
            (Kind::Text, Value::Text(t)) => {
                let t = t.trim().to_owned();
                if self.key == "database.report_url" {
                    check_report_url(&t)?;
                }
                Value::Text(t)
            }
            (kind, v) => {
                return Err(format!(
                    "`{}` takes {}, not {}",
                    self.key,
                    match kind {
                        Kind::Switch => "true or false",
                        Kind::Number { .. } => "a whole number",
                        Kind::Text => "a text",
                    },
                    v.shown()
                ));
            }
        };
        (self.put)(settings, value);
        Ok(())
    }

    /// Back to the default.
    pub fn reset(&self, settings: &mut IdeSettings) {
        let d = IdeSettings::default();
        (self.put)(settings, (self.get)(&d));
    }

    /// Enter, Space or Left/Right on the row: a switch turns over, a number moves `by` steps
    /// within its range; a text is edited in a field instead.
    pub fn step(&self, settings: &mut IdeSettings, by: i64) {
        match (self.kind, self.value(settings)) {
            (Kind::Switch, Value::Switch(b)) => (self.put)(settings, Value::Switch(!b)),
            (Kind::Number { min, max, step }, Value::Number(n)) => {
                let to = (n as i64 + by * step as i64).clamp(min as i64, max as i64) as u64;
                (self.put)(settings, Value::Number(to));
            }
            _ => {}
        }
    }

    /// `json` as a value of this setting (MCP).
    pub fn from_json(&self, json: &serde_json::Value) -> Result<Value, String> {
        match (self.kind, json) {
            (Kind::Switch, serde_json::Value::Bool(b)) => Ok(Value::Switch(*b)),
            (Kind::Number { .. }, serde_json::Value::Number(n)) => n
                .as_u64()
                .map(Value::Number)
                .ok_or_else(|| format!("`{}` takes a whole number", self.key)),
            (Kind::Text, serde_json::Value::String(t)) => Ok(Value::Text(t.clone())),
            _ => Err(format!("invalid value {json} for `{}`", self.key)),
        }
    }
}

/// The rows with their values in `settings`, as `ide_settings` returns them (behaviour 7).
pub fn rows_json(settings: &IdeSettings) -> serde_json::Value {
    ADVANCED
        .iter()
        .map(|a| {
            serde_json::json!({
                "key": a.key,
                "label": a.label,
                "description": a.description,
                "value": a.value(settings).json(),
                "default": a.default_value().json(),
                "modified": a.modified(settings),
            })
        })
        .collect::<Vec<_>>()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0337 AK-1: every row reads, changes and resets its setting; numbers are clamped,
    /// addresses checked, values of the wrong kind refused.
    #[test]
    fn rows_read_change_and_reset_their_settings() {
        let mut s = IdeSettings::default();
        for a in ADVANCED {
            assert!(!a.modified(&s), "{}: the defaults are not modified", a.key);
            assert_eq!(find(a.key).map(|f| f.key), Some(a.key));
        }
        let compact = find("project_view.compact_middle_packages").unwrap();
        assert_eq!(compact.value(&s), Value::Switch(true));
        compact.step(&mut s, 0);
        assert!(!s.project_view.compact_middle_packages);
        assert!(compact.modified(&s));
        compact.reset(&mut s);
        assert!(s.project_view.compact_middle_packages);

        let chars = find("ai.attachment_chars").unwrap();
        assert_eq!(
            chars.value(&s),
            Value::Number(24_000),
            "0 shows the default"
        );
        chars.step(&mut s, 1);
        assert_eq!(s.ai.attachment_chars, 25_000);
        chars.set(&mut s, Value::Number(5)).unwrap();
        assert_eq!(s.ai.attachment_chars, 1_000, "clamped");
        chars.step(&mut s, -3);
        assert_eq!(s.ai.attachment_chars, 1_000, "the step stops at the limit");
        chars.reset(&mut s);
        assert_eq!(
            s.ai.attachment_chars, 0,
            "the default is kept as a file without the key"
        );
        chars.set(&mut s, Value::Number(24_000)).unwrap();
        assert_eq!(s, IdeSettings::default());

        let url = find("database.report_url").unwrap();
        assert!(
            url.set(&mut s, Value::Text("http://x/{title}{body}".into()))
                .is_err()
        );
        assert!(url.set(&mut s, Value::Text("https://x/".into())).is_err());
        assert_eq!(
            s.database.report_url, "",
            "a refused address changes nothing"
        );
        url.set(
            &mut s,
            Value::Text(" https://x/new?t={title}&b={body} ".into()),
        )
        .unwrap();
        assert_eq!(s.database.report_url, "https://x/new?t={title}&b={body}");
        assert!(url.modified(&s));
        url.set(&mut s, Value::Text("off".into())).unwrap();
        url.reset(&mut s);
        assert_eq!(s.database.report_url, "");

        assert!(compact.set(&mut s, Value::Number(1)).is_err());
        assert!(chars.from_json(&serde_json::json!("many")).is_err());
        assert_eq!(
            compact.from_json(&serde_json::json!(false)),
            Ok(Value::Switch(false))
        );
    }

    /// SPEC-0337 behaviour 5a: the search filters the rows by label, key and sentence.
    #[test]
    fn the_search_filters_the_rows() {
        assert_eq!(rows("").len(), ADVANCED.len());
        assert_eq!(rows("Advanced").len(), ADVANCED.len());
        let keys = |q: &str| rows(q).into_iter().map(|a| a.key).collect::<Vec<_>>();
        assert_eq!(keys("packages"), ["project_view.compact_middle_packages"]);
        assert_eq!(keys("ATTACHMENT_CHARS"), ["ai.attachment_chars"]);
        assert_eq!(keys("{title}"), ["database.report_url"]);
        assert!(keys("no such setting").is_empty());
        let json = rows_json(&IdeSettings::default());
        assert_eq!(json[1]["default"], 24_000);
        assert_eq!(json[0]["modified"], false);
    }

    /// SPEC-0337 behaviour 6: no security switch is listed here.
    #[test]
    fn no_security_switch_is_an_advanced_setting() {
        for a in ADVANCED {
            for kept in ["ssh", "mcp.", "ui.driver", "trust", "allow"] {
                assert!(!a.key.contains(kept), "{} belongs on its own page", a.key);
            }
        }
    }
}
