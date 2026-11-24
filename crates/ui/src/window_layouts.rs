//! Named tool window layouts (`SPEC-0333`): the arrangements of the tool windows the user saved
//! under a name, the selected one, the rules for names, the entries of the Layouts submenu and the
//! user's file they are kept in (`window-layouts.json` in the settings folder).
//!
//! The rules follow the layout actions of the open IntelliJ platform (logic template, own code;
//! `docs/legal/PROVENANCE.md`). Toolkit-free so the workbench and the MCP server share it.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::tool_windows::{Layout, ToolWindow};

/// The file in the user's settings folder (behaviour 15).
pub const LAYOUTS_FILE: &str = "window-layouts.json";
/// The format version written (behaviour 16).
pub const LAYOUTS_VERSION: u32 = 1;
/// Longest name, in characters (behaviour 5).
pub const MAX_NAME: usize = 64;
/// Most saved layouts (behaviour 4).
pub const MAX_LAYOUTS: usize = 50;
/// Title of the default layout; no saved layout has this name (behaviour 2).
pub const DEFAULT_TITLE: &str = "Default";

/// Prefix of the action that applies the layout named after it (behaviour 6).
pub const APPLY: &str = "FerrisIDEA.Layout.Apply:";
/// Prefix of the action that renames the layout named after it (behaviour 10).
pub const RENAME: &str = "FerrisIDEA.Layout.Rename:";
/// Prefix of the action that deletes the layout named after it (behaviour 11).
pub const DELETE: &str = "FerrisIDEA.Layout.Delete:";

/// The layouts file of this user, if there is a settings folder.
pub fn layouts_file() -> Option<PathBuf> {
    ferris_idea_core::user_config_dir().map(|d| d.join(LAYOUTS_FILE))
}

/// What a saved layout holds (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrangement {
    /// Sides, halves, order, sizes, view modes, stripe buttons, frames and the stripes.
    pub layout: Layout,
    /// The shown tool windows, in the order they were given.
    pub shown: Vec<ToolWindow>,
    /// The tool window that had the keyboard focus; always one of `shown`.
    pub active: Option<ToolWindow>,
}

impl Arrangement {
    /// The arrangement of a window: its layout without what only lasts while it runs, the shown
    /// windows once each, and the focused one when it is shown.
    pub fn new(layout: &Layout, shown: &[ToolWindow], active: Option<ToolWindow>) -> Self {
        let mut once = Vec::new();
        for w in shown {
            if !once.contains(w) {
                once.push(*w);
            }
        }
        Self {
            layout: Layout::parse(&layout.to_json()),
            active: active.filter(|a| once.contains(a)),
            shown: once,
        }
    }
}

/// What applying a layout does to a window (behaviours 6 and 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The layout's name; `None` for the default layout.
    pub name: Option<String>,
    /// The layout of the tool windows.
    pub layout: Layout,
    /// The windows to show, all others hidden; `None` keeps the shown windows (default layout).
    pub shown: Option<Vec<ToolWindow>>,
    /// The window that gets the keyboard focus, else the editor.
    pub active: Option<ToolWindow>,
}

/// Why a layout action was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    /// Nothing but white space.
    #[error("A layout name cannot be empty")]
    Empty,
    /// More than [`MAX_NAME`] characters.
    #[error("A layout name has at most {} characters", MAX_NAME)]
    TooLong,
    /// A path separator.
    #[error("A layout name cannot contain / or \\")]
    Separator,
    /// A line break, a tab or another control character.
    #[error("A layout name cannot contain line breaks or other control characters")]
    Control,
    /// The default layout's title.
    #[error("\"Default\" is the name of the default layout")]
    Reserved,
    /// Another layout has the name, capitals ignored.
    #[error("A layout named \"{0}\" already exists")]
    Taken(String),
    /// No saved layout has the name.
    #[error("There is no layout named \"{0}\"")]
    Unknown(String),
    /// Saving over, renaming or deleting the default layout.
    #[error("The default layout cannot be changed; save the current layout under a new name")]
    DefaultLayout,
    /// Deleting the selected layout.
    #[error("Layout \"{0}\" is selected; select another layout before deleting it")]
    Selected(String),
    /// [`MAX_LAYOUTS`] layouts exist.
    #[error("At most {} layouts are kept; delete one first", MAX_LAYOUTS)]
    Full,
}

/// Why the layouts file could not be used (behaviour 18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Written by a newer Ferris IDEA: never overwritten.
    Newer(u64),
    /// Not a layouts file: kept once as `.bak` before the next save.
    Broken(String),
}

impl Problem {
    /// The problem in words for the status line and the MCP result.
    pub fn message(&self) -> String {
        match self {
            Problem::Newer(v) => format!(
                "The saved layouts were written by a newer Ferris IDEA (format {v}; this version reads up to {LAYOUTS_VERSION}); the default layout is used and the file is left as it is"
            ),
            Problem::Broken(why) => {
                format!("The saved layouts could not be read ({why}); the default layout is used")
            }
        }
    }
}

/// One entry of the Layouts submenu (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    /// An action.
    Action {
        /// The text of the item.
        label: String,
        /// The action id; the per-layout ones end in the layout's name.
        id: String,
        /// Shown with a tick.
        checked: bool,
        /// Whether it can be chosen.
        enabled: bool,
    },
    /// A line between groups.
    Separator,
    /// The submenu of one saved layout.
    Sub {
        /// The layout's name, after a tick when it is the selected one.
        label: String,
        /// Its entries.
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    fn action(label: &str, id: impl Into<String>) -> Self {
        Self::Action {
            label: label.to_owned(),
            id: id.into(),
            checked: false,
            enabled: true,
        }
    }

    fn enabled(self, on: bool) -> Self {
        match self {
            Self::Action {
                label, id, checked, ..
            } => Self::Action {
                label,
                id,
                checked,
                enabled: on,
            },
            other => other,
        }
    }

    fn checked(self, on: bool) -> Self {
        match self {
            Self::Action {
                label, id, enabled, ..
            } => Self::Action {
                label,
                id,
                checked: on,
                enabled,
            },
            other => other,
        }
    }
}

/// The saved layouts of the user and the selected one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layouts {
    /// The saved layouts by name, capitals ignored.
    list: Vec<(String, Arrangement)>,
    /// The selected layout's name; `None` is the default layout.
    active: Option<String>,
    /// Why the file could not be used, if it could not.
    pub problem: Option<Problem>,
    /// Entries of the file that were skipped (behaviour 17).
    pub skipped: usize,
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Layouts {
    /// The names of the saved layouts, in menu order.
    pub fn names(&self) -> Vec<&str> {
        self.list.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// The saved layouts with their arrangements, in menu order.
    pub fn saved(&self) -> impl Iterator<Item = (&str, &Arrangement)> {
        self.list.iter().map(|(n, a)| (n.as_str(), a))
    }

    /// The saved layout `name`, capitals ignored, under the name it was saved with.
    pub fn get(&self, name: &str) -> Option<(&str, &Arrangement)> {
        self.list
            .iter()
            .find(|(n, _)| same(n, name.trim()))
            .map(|(n, a)| (n.as_str(), a))
    }

    /// The selected layout's name; `None` while the default layout is selected.
    pub fn selected(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// The selected layout's title: its name, or "Default".
    pub fn selected_title(&self) -> &str {
        self.active.as_deref().unwrap_or(DEFAULT_TITLE)
    }

    /// Whether `name` means the default layout.
    pub fn is_default_name(name: &str) -> bool {
        same(name.trim(), DEFAULT_TITLE)
    }

    /// `name` as it would be saved (behaviour 5): trimmed and checked. `renaming` is the layout
    /// that gets the name, whose own name does not count as taken.
    pub fn valid_name(&self, name: &str, renaming: Option<&str>) -> Result<String, LayoutError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LayoutError::Empty);
        }
        if name.chars().any(char::is_control) {
            return Err(LayoutError::Control);
        }
        if name.chars().count() > MAX_NAME {
            return Err(LayoutError::TooLong);
        }
        if name.contains(['/', '\\']) {
            return Err(LayoutError::Separator);
        }
        if Self::is_default_name(name) {
            return Err(LayoutError::Reserved);
        }
        if let Some((taken, _)) = self.get(name)
            && !renaming.is_some_and(|own| same(own, taken))
        {
            return Err(LayoutError::Taken(taken.to_owned()));
        }
        Ok(name.to_owned())
    }

    fn sort(&mut self) {
        self.list
            .sort_by(|(a, _), (b, _)| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    }

    /// Saves `arrangement` as a new layout and selects it (behaviour 4); returns its name.
    pub fn save_new(
        &mut self,
        name: &str,
        arrangement: Arrangement,
    ) -> Result<String, LayoutError> {
        let name = self.valid_name(name, None)?;
        if self.list.len() >= MAX_LAYOUTS {
            return Err(LayoutError::Full);
        }
        self.list.push((name.clone(), arrangement));
        self.sort();
        self.active = Some(name.clone());
        Ok(name)
    }

    /// Saves `arrangement` into the selected layout (behaviour 9); returns its name.
    pub fn save_changes(&mut self, arrangement: Arrangement) -> Result<String, LayoutError> {
        let name = self.active.clone().ok_or(LayoutError::DefaultLayout)?;
        match self.list.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = arrangement,
            None => return Err(LayoutError::Unknown(name)),
        }
        Ok(name)
    }

    fn applied(&self, name: Option<&str>) -> Applied {
        match name.and_then(|n| self.get(n)) {
            Some((name, a)) => Applied {
                name: Some(name.to_owned()),
                layout: a.layout.clone(),
                shown: Some(a.shown.clone()),
                active: a.active,
            },
            None => Applied {
                name: None,
                layout: Layout::default(),
                shown: None,
                active: None,
            },
        }
    }

    /// Selects the layout `name` — `None` or "Default" for the default layout — and returns what
    /// to apply (behaviours 6 and 7).
    pub fn select(&mut self, name: Option<&str>) -> Result<Applied, LayoutError> {
        let name = name.filter(|n| !Self::is_default_name(n));
        match name {
            None => self.active = None,
            Some(n) => match self.get(n) {
                Some((saved, _)) => self.active = Some(saved.to_owned()),
                None => return Err(LayoutError::Unknown(n.trim().to_owned())),
            },
        }
        Ok(self.current())
    }

    /// The selected layout as it was saved (behaviour 8).
    pub fn current(&self) -> Applied {
        self.applied(self.active.as_deref())
    }

    /// Renames the layout `old` (behaviour 10); returns the new name, `None` when it is the old
    /// one. The selected layout stays selected.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<Option<String>, LayoutError> {
        if Self::is_default_name(old) {
            return Err(LayoutError::DefaultLayout);
        }
        let Some(saved) = self.get(old).map(|(n, _)| n.to_owned()) else {
            return Err(LayoutError::Unknown(old.trim().to_owned()));
        };
        let new = self.valid_name(new, Some(&saved))?;
        if new == saved {
            return Ok(None);
        }
        if let Some(entry) = self.list.iter_mut().find(|(n, _)| *n == saved) {
            entry.0 = new.clone();
        }
        if self.active.as_deref() == Some(saved.as_str()) {
            self.active = Some(new.clone());
        }
        self.sort();
        Ok(Some(new))
    }

    /// Whether the layout `name` may be deleted: it exists and is not selected (behaviour 11).
    /// Returns the name it was saved with.
    pub fn deletable(&self, name: &str) -> Result<String, LayoutError> {
        if Self::is_default_name(name) {
            return Err(LayoutError::DefaultLayout);
        }
        let Some((saved, _)) = self.get(name) else {
            return Err(LayoutError::Unknown(name.trim().to_owned()));
        };
        if self.active.as_deref() == Some(saved) {
            return Err(LayoutError::Selected(saved.to_owned()));
        }
        Ok(saved.to_owned())
    }

    /// Deletes the layout `name` (behaviour 11); returns the name it was saved with.
    pub fn delete(&mut self, name: &str) -> Result<String, LayoutError> {
        let saved = self.deletable(name)?;
        self.list.retain(|(n, _)| *n != saved);
        Ok(saved)
    }

    /// The entries of the Layouts submenu (behaviour 12).
    pub fn menu(&self) -> Vec<MenuItem> {
        let mut out = vec![
            MenuItem::action(DEFAULT_TITLE, "RestoreFactoryDefaultLayout")
                .checked(self.active.is_none()),
            MenuItem::Separator,
        ];
        for (name, _) in &self.list {
            let selected = self.active.as_deref() == Some(name.as_str());
            let mut items = if selected {
                vec![
                    MenuItem::action("Restore", "RestoreDefaultLayout"),
                    MenuItem::action("Save Changes", "StoreDefaultLayout"),
                ]
            } else {
                vec![MenuItem::action("Apply", format!("{APPLY}{name}"))]
            };
            items.extend([
                MenuItem::action("Rename…", format!("{RENAME}{name}")),
                MenuItem::Separator,
                MenuItem::action("Delete…", format!("{DELETE}{name}")).enabled(!selected),
            ]);
            out.push(MenuItem::Sub {
                label: if selected {
                    format!("✓ {name}")
                } else {
                    name.clone()
                },
                items,
            });
        }
        if !self.list.is_empty() {
            out.push(MenuItem::Separator);
        }
        out.extend([
            MenuItem::action("Restore Current Layout", "RestoreDefaultLayout"),
            MenuItem::action("Save Changes in Current Layout", "StoreDefaultLayout")
                .enabled(self.active.is_some()),
            MenuItem::action("Save Current Layout as New…", "StoreNewLayout"),
        ]);
        out
    }

    fn broken(why: impl Into<String>) -> Self {
        Self {
            problem: Some(Problem::Broken(why.into())),
            ..Self::default()
        }
    }

    /// The layouts in the text of a layouts file (behaviours 16–18). Never fails: what cannot be
    /// read is skipped or named in `problem`.
    pub fn parse(text: &str) -> Self {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                return Self::broken(format!(
                    "no JSON at line {}, column {}",
                    e.line(),
                    e.column()
                ));
            }
        };
        let Some(file) = value.as_object() else {
            return Self::broken("not a JSON object");
        };
        match file.get("version") {
            None => {}
            Some(v) => match v.as_u64() {
                Some(v) if v > u64::from(LAYOUTS_VERSION) => {
                    return Self {
                        problem: Some(Problem::Newer(v)),
                        ..Self::default()
                    };
                }
                Some(_) => {}
                None => return Self::broken("`version` is not a number"),
            },
        }
        let none = Vec::new();
        let entries = match file.get("layouts") {
            None => &none,
            Some(Value::Array(a)) => a,
            Some(_) => return Self::broken("`layouts` is not a list"),
        };
        let mut out = Self::default();
        for entry in entries {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .and_then(|n| out.valid_name(n, None).ok());
            let Some(name) = name.filter(|_| out.list.len() < MAX_LAYOUTS) else {
                out.skipped += 1;
                continue;
            };
            let layout = entry
                .get("layout")
                .filter(|l| l.is_object())
                .map(|l| Layout::parse(&l.to_string()))
                .unwrap_or_default();
            let ids = |v: &Value| -> Vec<ToolWindow> {
                v.as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .filter_map(ToolWindow::from_id)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let shown = entry.get("shown").map(ids).unwrap_or_default();
            let active = entry
                .get("active_window")
                .and_then(Value::as_str)
                .and_then(ToolWindow::from_id);
            out.list
                .push((name, Arrangement::new(&layout, &shown, active)));
        }
        out.sort();
        out.active = file
            .get("active")
            .and_then(Value::as_str)
            .and_then(|n| out.get(n))
            .map(|(n, _)| n.to_owned());
        out
    }

    /// The text of the layouts file (behaviour 16).
    pub fn to_json(&self) -> String {
        let layouts: Vec<Value> = self
            .list
            .iter()
            .map(|(name, a)| {
                let layout: Value = serde_json::from_str(&a.layout.to_json()).unwrap_or_default();
                let mut entry = json!({
                    "name": name,
                    "layout": layout,
                    "shown": a.shown.iter().map(|w| w.id()).collect::<Vec<_>>(),
                });
                if let (Some(w), Some(map)) = (a.active, entry.as_object_mut()) {
                    map.insert("active_window".into(), json!(w.id()));
                }
                entry
            })
            .collect();
        let mut file = json!({ "version": LAYOUTS_VERSION, "layouts": layouts });
        if let (Some(active), Some(map)) = (&self.active, file.as_object_mut()) {
            map.insert("active".into(), json!(active));
        }
        serde_json::to_string_pretty(&sorted(file)).unwrap_or_default() + "\n"
    }

    /// Reads the layouts file; a missing file means no saved layouts, a file that cannot be read
    /// gives the default layout and a problem (behaviour 18).
    pub fn load(file: &Path) -> Self {
        match std::fs::read_to_string(file) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => Self::broken(e.kind().to_string()),
        }
    }

    /// Writes the layouts file whole and atomically. A file of a newer version is not
    /// overwritten; a broken one is first kept as `<file>.bak` (behaviour 18).
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        match &self.problem {
            Some(p @ Problem::Newer(_)) => return Err(std::io::Error::other(p.message())),
            Some(Problem::Broken(_)) if file.is_file() => {
                let mut bak = file.as_os_str().to_owned();
                bak.push(".bak");
                std::fs::copy(file, PathBuf::from(bak))?;
            }
            _ => {}
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::keymap::write_atomic(file, self.to_json())
    }
}

/// The JSON Schema of the layouts file, published as `docs/formats/window-layouts.schema.json`
/// (behaviour 16).
pub fn json_schema() -> Value {
    // In parts: one `json!` of the whole schema is deeper than the macro's recursion limit.
    let window = json!({
        "type": "object",
        "required": ["id", "side"],
        "properties": {
            "id": {
                "description": "Tool window id, e.g. project, terminal.",
                "type": "string"
            },
            "side": { "enum": ["left", "bottom", "right"] },
            "size": {
                "description": "Width (left, right) or height (bottom) in pixels, if the user set one.",
                "type": "integer",
                "minimum": 0
            },
            "split": {
                "description": "The second half of the side.",
                "type": "boolean"
            },
            "mode": {
                "description": "View mode other than docked and pinned.",
                "enum": ["docked", "unpinned", "undocked", "float", "window"]
            },
            "removed": {
                "description": "No stripe button.",
                "type": "boolean"
            },
            "bounds": {
                "description": "Last frame as a Float or Window window: x, y, width, height.",
                "type": "array",
                "items": { "type": "integer" },
                "minItems": 4,
                "maxItems": 4
            }
        }
    });
    let layout = json!({
        "description": "The arrangement, in the form of .ferris-idea/tool-windows.json (SPEC-0167).",
        "type": "object",
        "properties": {
            "version": {
                "description": "Version of the tool window layout form.",
                "type": "integer"
            },
            "stripes": {
                "description": "Whether the stripes are shown.",
                "type": "boolean"
            },
            "windows": {
                "description": "The tool windows in stripe order.",
                "type": "array",
                "items": window
            }
        }
    });
    let entry = json!({
        "type": "object",
        "required": ["name"],
        "properties": {
            "name": {
                "description": "1 to 64 characters without / and \\ and control characters, not \"Default\"; unique when capitals are ignored.",
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_NAME
            },
            "layout": layout,
            "shown": {
                "description": "Ids of the shown tool windows.",
                "type": "array",
                "items": { "type": "string" }
            },
            "active_window": {
                "description": "Id of the tool window that had the keyboard focus; one of `shown`.",
                "type": "string"
            }
        }
    });
    sorted(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Ferris IDEA window layouts",
        "description": "State file of the user (SPEC-0333): window-layouts.json in the settings folder. JSON; unknown keys and unknown tool window ids are ignored when it is read.",
        "type": "object",
        "required": ["version"],
        "properties": {
            "version": {
                "description": "Format version; a file of a newer version is not read and not overwritten.",
                "type": "integer",
                "minimum": 1,
                "maximum": LAYOUTS_VERSION
            },
            "active": {
                "description": "Name of the selected layout; missing means the default layout.",
                "type": "string"
            },
            "layouts": {
                "description": "The saved layouts.",
                "type": "array",
                "maxItems": MAX_LAYOUTS,
                "items": entry
            }
        }
    }))
}

/// `value` with the keys of every object in alphabetical order, so the published text does not
/// depend on how the JSON library keeps its maps.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_windows::{Side, ViewMode};

    fn review() -> Arrangement {
        let mut layout = Layout::default();
        layout.move_to(ToolWindow::Terminal, Side::Right);
        layout.set_size(ToolWindow::Project, 420.0);
        layout.set_mode(ToolWindow::Git, ViewMode::Undock);
        layout.remove(ToolWindow::Problems);
        layout.stripes = false;
        Arrangement::new(
            &layout,
            &[ToolWindow::Project, ToolWindow::Terminal],
            Some(ToolWindow::Terminal),
        )
    }

    fn plain() -> Arrangement {
        Arrangement::new(&Layout::default(), &[ToolWindow::Project], None)
    }

    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-layouts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(LAYOUTS_FILE)
    }

    /// SPEC-0333 AK-1: a saved layout is selected, comes back as saved when applied or
    /// restored, and the default layout keeps the shown windows.
    #[test]
    fn save_apply_restore() {
        let mut l = Layouts::default();
        assert_eq!(l.selected(), None);
        assert_eq!(l.selected_title(), "Default");
        let default = l.current();
        assert_eq!(default.name, None);
        assert_eq!(default.layout, Layout::default());
        assert_eq!(default.shown, None);

        assert_eq!(l.save_new("  Review ", review()).unwrap(), "Review");
        assert_eq!(l.selected(), Some("Review"));
        assert_eq!(l.save_new("coding", plain()).unwrap(), "coding");
        assert_eq!(l.selected(), Some("coding"));
        assert_eq!(l.names(), ["coding", "Review"]);

        let applied = l.select(Some("review")).unwrap();
        assert_eq!(applied.name.as_deref(), Some("Review"));
        assert_eq!(applied.layout, review().layout);
        assert_eq!(applied.layout.side(ToolWindow::Terminal), Side::Right);
        assert_eq!(applied.layout.size(ToolWindow::Project), Some(420.0));
        assert!(!applied.layout.stripes);
        assert_eq!(
            applied.shown,
            Some(vec![ToolWindow::Project, ToolWindow::Terminal])
        );
        assert_eq!(applied.active, Some(ToolWindow::Terminal));
        assert_eq!(l.selected(), Some("Review"));
        assert_eq!(l.current(), applied);

        // The default layout by its title or by no name.
        assert_eq!(l.select(Some("default")).unwrap().name, None);
        assert_eq!(l.selected(), None);
        l.select(Some("coding")).unwrap();
        assert_eq!(l.select(None).unwrap().shown, None);
        assert_eq!(
            l.select(Some("nope")),
            Err(LayoutError::Unknown("nope".into()))
        );
        assert_eq!(l.selected(), None);
    }

    /// SPEC-0333 AK-1: Save Changes writes into the selected layout and never into the default.
    #[test]
    fn save_changes_needs_a_saved_layout() {
        let mut l = Layouts::default();
        assert_eq!(l.save_changes(review()), Err(LayoutError::DefaultLayout));
        l.save_new("Review", plain()).unwrap();
        assert_eq!(l.save_changes(review()).unwrap(), "Review");
        assert_eq!(l.get("Review").unwrap().1, &review());
        assert_eq!(l.names().len(), 1);
    }

    /// SPEC-0333 AK-1: the selected layout follows its new name; the same name renames nothing.
    #[test]
    fn rename_keeps_the_selection() {
        let mut l = Layouts::default();
        l.save_new("Review", review()).unwrap();
        l.save_new("Coding", plain()).unwrap();
        assert_eq!(
            l.rename("review", " Audit ").unwrap().as_deref(),
            Some("Audit")
        );
        assert_eq!(l.names(), ["Audit", "Coding"]);
        assert_eq!(l.selected(), Some("Coding"));
        assert_eq!(
            l.rename("Coding", "Write").unwrap().as_deref(),
            Some("Write")
        );
        assert_eq!(l.selected(), Some("Write"));
        assert_eq!(l.rename("Write", "Write").unwrap(), None);
        assert_eq!(
            l.rename("Write", "audit"),
            Err(LayoutError::Taken("Audit".into()))
        );
        assert_eq!(
            l.rename("gone", "x"),
            Err(LayoutError::Unknown("gone".into()))
        );
        assert_eq!(l.rename("Default", "x"), Err(LayoutError::DefaultLayout));
        assert_eq!(l.get("Audit").unwrap().1, &review());
    }

    /// SPEC-0333 AK-1.
    #[test]
    fn the_selected_and_the_default_layout_are_not_deleted() {
        let mut l = Layouts::default();
        l.save_new("Review", review()).unwrap();
        l.save_new("Coding", plain()).unwrap();
        assert_eq!(
            l.delete("coding"),
            Err(LayoutError::Selected("Coding".into()))
        );
        assert_eq!(l.delete("Default"), Err(LayoutError::DefaultLayout));
        assert_eq!(l.delete("gone"), Err(LayoutError::Unknown("gone".into())));
        assert_eq!(l.delete("REVIEW").unwrap(), "Review");
        assert_eq!(l.names(), ["Coding"]);
        l.select(None).unwrap();
        assert_eq!(l.delete("Coding").unwrap(), "Coding");
        assert!(l.names().is_empty());
    }

    /// SPEC-0333 AK-1.
    #[test]
    fn at_most_fifty_layouts() {
        let mut l = Layouts::default();
        for i in 0..MAX_LAYOUTS {
            l.save_new(&format!("L{i}"), plain()).unwrap();
        }
        assert_eq!(l.save_new("one more", plain()), Err(LayoutError::Full));
        assert_eq!(l.names().len(), 50);
    }

    /// SPEC-0333 AK-2.
    #[test]
    fn names_are_trimmed_and_checked() {
        let mut l = Layouts::default();
        l.save_new("Review", plain()).unwrap();
        let check = |n: &str| l.valid_name(n, None);
        assert_eq!(check("  Coding\t").unwrap(), "Coding");
        assert_eq!(check(""), Err(LayoutError::Empty));
        assert_eq!(check("   "), Err(LayoutError::Empty));
        assert_eq!(check(&"x".repeat(64)).unwrap().chars().count(), 64);
        assert_eq!(check(&"ä".repeat(64)).unwrap().chars().count(), 64);
        assert_eq!(check(&"x".repeat(65)), Err(LayoutError::TooLong));
        assert_eq!(check("a/b"), Err(LayoutError::Separator));
        assert_eq!(check("a\\b"), Err(LayoutError::Separator));
        assert_eq!(check("a\nb"), Err(LayoutError::Control));
        assert_eq!(check("a\tb"), Err(LayoutError::Control));
        assert_eq!(check("default"), Err(LayoutError::Reserved));
        assert_eq!(check(" DEFAULT "), Err(LayoutError::Reserved));
        assert_eq!(check("rEVIEW"), Err(LayoutError::Taken("Review".into())));
        assert_eq!(check("Review 2").unwrap(), "Review 2");
        // Every refusal says which rule it is.
        assert_eq!(
            LayoutError::TooLong.to_string(),
            "A layout name has at most 64 characters"
        );
        assert_eq!(
            LayoutError::Taken("Review".into()).to_string(),
            "A layout named \"Review\" already exists"
        );
        assert_eq!(
            l.save_new("review", plain()),
            Err(LayoutError::Taken("Review".into()))
        );
    }

    /// SPEC-0333 AK-2.
    #[test]
    fn a_layout_may_change_the_capitals_of_its_own_name() {
        let mut l = Layouts::default();
        l.save_new("review", plain()).unwrap();
        assert_eq!(
            l.rename("review", "Review").unwrap().as_deref(),
            Some("Review")
        );
        assert_eq!(l.names(), ["Review"]);
        assert_eq!(l.selected(), Some("Review"));
    }

    /// SPEC-0333 AK-3.
    #[test]
    fn the_file_round_trips() {
        let mut l = Layouts::default();
        l.save_new("Review", review()).unwrap();
        l.save_new("Coding", plain()).unwrap();
        l.select(Some("Review")).unwrap();
        let text = l.to_json();
        assert_eq!(Layouts::parse(&text), l);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["active"], "Review");
        assert_eq!(v["layouts"][1]["name"], "Review");
        assert_eq!(v["layouts"][1]["shown"], json!(["project", "terminal"]));
        assert_eq!(v["layouts"][1]["active_window"], "terminal");
        assert_eq!(v["layouts"][1]["layout"]["stripes"], false);
        assert!(v["layouts"][0].get("active_window").is_none());
        // The default layout selected: no `active`.
        l.select(None).unwrap();
        let v: Value = serde_json::from_str(&l.to_json()).unwrap();
        assert!(v.get("active").is_none());
        assert_eq!(Layouts::parse(&l.to_json()), l);

        let file = temp("round");
        assert_eq!(Layouts::load(&file), Layouts::default());
        l.save(&file).unwrap();
        assert_eq!(Layouts::load(&file), l);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// SPEC-0333 AK-3.
    #[test]
    fn unknown_ids_and_broken_entries_are_skipped() {
        let text = r#"{
            "version": 1, "active": "REVIEW", "future": true,
            "layouts": [
                {"name": "Review", "more": 1,
                 "layout": {"version": 2, "stripes": true, "windows": [
                     {"id": "terminal", "side": "right"},
                     {"id": "hologram", "side": "left"},
                     {"id": "git", "side": "top"}]},
                 "shown": ["terminal", "hologram", "terminal", 7, "project"],
                 "active_window": "git"},
                {"name": "review"},
                {"name": "a/b"},
                {"name": ""},
                {"layout": {}},
                "text",
                {"name": "Bare"},
                {"name": "Odd", "layout": [], "shown": "all", "active_window": "hologram"}
            ]
        }"#;
        let l = Layouts::parse(text);
        assert_eq!(l.problem, None);
        assert_eq!(l.skipped, 5);
        assert_eq!(l.names(), ["Bare", "Odd", "Review"]);
        assert_eq!(l.selected(), Some("Review"));
        let (_, review) = l.get("Review").unwrap();
        assert_eq!(review.layout.side(ToolWindow::Terminal), Side::Right);
        assert_eq!(review.layout.side(ToolWindow::Git), Side::Bottom);
        assert_eq!(review.shown, [ToolWindow::Terminal, ToolWindow::Project]);
        // Git was focused but is not shown.
        assert_eq!(review.active, None);
        for bare in ["Bare", "Odd"] {
            let (_, a) = l.get(bare).unwrap();
            assert_eq!(a.layout, Layout::default());
            assert!(a.shown.is_empty());
            assert_eq!(a.active, None);
        }
        // An `active` naming no layout is the default layout; no version reads as version 1.
        let l = Layouts::parse(r#"{"active": "Gone", "layouts": [{"name": "A"}]}"#);
        assert_eq!((l.selected(), l.names()), (None, vec!["A"]));
        assert_eq!(Layouts::parse("{}"), Layouts::default());
    }

    /// SPEC-0333 AK-3: never a panic, always the default layout and a reason.
    #[test]
    fn a_broken_file_gives_the_default_layout() {
        for text in [
            "",
            "not json",
            "[]",
            "42",
            r#"{"version": "one"}"#,
            r#"{"version": 1, "layouts": {"name": "A"}}"#,
            "{\"version\": 1, \"layouts\": [",
            "\u{0}\u{1}",
        ] {
            let l = Layouts::parse(text);
            assert!(
                matches!(l.problem, Some(Problem::Broken(_))),
                "{text:?}: {:?}",
                l.problem
            );
            assert!(l.names().is_empty(), "{text:?}");
            assert_eq!(l.current().layout, Layout::default(), "{text:?}");
            assert!(
                l.problem
                    .as_ref()
                    .unwrap()
                    .message()
                    .ends_with("the default layout is used")
            );
        }
        let newer = Layouts::parse(r#"{"version": 2, "layouts": [{"name": "A"}]}"#);
        assert_eq!(newer.problem, Some(Problem::Newer(2)));
        assert!(newer.names().is_empty());
        assert!(newer.problem.unwrap().message().contains("format 2"));
    }

    /// SPEC-0333 AK-3.
    #[test]
    fn a_newer_file_is_not_overwritten() {
        let file = temp("newer");
        let text = r#"{"version": 9, "layouts": []}"#;
        std::fs::write(&file, text).unwrap();
        let mut l = Layouts::load(&file);
        assert_eq!(l.problem, Some(Problem::Newer(9)));
        l.save_new("Review", plain()).unwrap();
        let err = l.save(&file).unwrap_err();
        assert!(err.to_string().contains("newer Ferris IDEA"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// SPEC-0333 AK-3.
    #[test]
    fn a_broken_file_is_kept_once() {
        let file = temp("broken");
        std::fs::write(&file, "{ half").unwrap();
        let mut l = Layouts::load(&file);
        assert!(matches!(l.problem, Some(Problem::Broken(_))));
        l.save_new("Review", plain()).unwrap();
        l.save(&file).unwrap();
        let bak = file.with_file_name(format!("{LAYOUTS_FILE}.bak"));
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "{ half");
        let again = Layouts::load(&file);
        assert_eq!(again.problem, None);
        assert_eq!(again.names(), ["Review"]);
        // The next save leaves the kept file alone.
        again.save(&file).unwrap();
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "{ half");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    fn action(label: &str, id: &str, checked: bool, enabled: bool) -> MenuItem {
        MenuItem::Action {
            label: label.into(),
            id: id.into(),
            checked,
            enabled,
        }
    }

    /// SPEC-0333 AK-5.
    #[test]
    fn menu_without_saved_layouts() {
        assert_eq!(
            Layouts::default().menu(),
            [
                action("Default", "RestoreFactoryDefaultLayout", true, true),
                MenuItem::Separator,
                action(
                    "Restore Current Layout",
                    "RestoreDefaultLayout",
                    false,
                    true
                ),
                action(
                    "Save Changes in Current Layout",
                    "StoreDefaultLayout",
                    false,
                    false
                ),
                action("Save Current Layout as New…", "StoreNewLayout", false, true),
            ]
        );
    }

    /// SPEC-0333 AK-5.
    #[test]
    fn menu_with_a_selected_layout() {
        let mut l = Layouts::default();
        l.save_new("Review", review()).unwrap();
        l.save_new("coding", plain()).unwrap();
        assert_eq!(
            l.menu(),
            [
                action("Default", "RestoreFactoryDefaultLayout", false, true),
                MenuItem::Separator,
                MenuItem::Sub {
                    label: "✓ coding".into(),
                    items: vec![
                        action("Restore", "RestoreDefaultLayout", false, true),
                        action("Save Changes", "StoreDefaultLayout", false, true),
                        action("Rename…", "FerrisIDEA.Layout.Rename:coding", false, true),
                        MenuItem::Separator,
                        action("Delete…", "FerrisIDEA.Layout.Delete:coding", false, false),
                    ],
                },
                MenuItem::Sub {
                    label: "Review".into(),
                    items: vec![
                        action("Apply", "FerrisIDEA.Layout.Apply:Review", false, true),
                        action("Rename…", "FerrisIDEA.Layout.Rename:Review", false, true),
                        MenuItem::Separator,
                        action("Delete…", "FerrisIDEA.Layout.Delete:Review", false, true),
                    ],
                },
                MenuItem::Separator,
                action(
                    "Restore Current Layout",
                    "RestoreDefaultLayout",
                    false,
                    true
                ),
                action(
                    "Save Changes in Current Layout",
                    "StoreDefaultLayout",
                    false,
                    true
                ),
                action("Save Current Layout as New…", "StoreNewLayout", false, true),
            ]
        );
    }

    /// SPEC-0333 AK-4: the published schema is the one the code gives. With
    /// `FERRIS_IDEA_WRITE_SCHEMAS=1` the test writes it.
    #[test]
    fn published_schema_matches() {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/formats/window-layouts.schema.json");
        let text = serde_json::to_string_pretty(&json_schema()).unwrap() + "\n";
        if ferris_idea_core::product_var_os("FERRIS_IDEA_WRITE_SCHEMAS").is_some() {
            std::fs::write(&file, &text).unwrap();
        }
        let published =
            std::fs::read_to_string(&file).unwrap_or_else(|_| panic!("{} missing", file.display()));
        assert_eq!(
            published,
            text,
            "{} differs from the code; run with FERRIS_IDEA_WRITE_SCHEMAS=1",
            file.display()
        );
        // What the code writes fits the schema's shape: its keys are the schema's.
        let mut l = Layouts::default();
        l.save_new("Review", review()).unwrap();
        let written: Value = serde_json::from_str(&l.to_json()).unwrap();
        let schema = json_schema();
        for key in written.as_object().unwrap().keys() {
            assert!(schema["properties"].get(key).is_some(), "{key}");
        }
        let entry = &schema["properties"]["layouts"]["items"]["properties"];
        for key in written["layouts"][0].as_object().unwrap().keys() {
            assert!(entry.get(key).is_some(), "{key}");
        }
        let window = &entry["layout"]["properties"]["windows"]["items"]["properties"];
        for w in written["layouts"][0]["layout"]["windows"]
            .as_array()
            .unwrap()
        {
            for key in w.as_object().unwrap().keys() {
                assert!(window.get(key).is_some(), "{key}");
            }
        }
    }
}
