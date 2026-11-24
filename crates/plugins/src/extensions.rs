//! Extension points beyond actions (`SPEC-0210`): completion items, status bar text, rows of the
//! Plugins tool window, and file types that Ferris IDEA highlights itself from the manifest.

use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Access, ActionKind, EditorContext, FUEL, INSPECT_FUEL, Manifest, Plugin, PluginError};

/// Most completion items taken from one run (behaviour 1).
pub const MAX_ITEMS: usize = 200;
/// Most rows taken from one panel run (behaviour 5).
pub const MAX_ROWS: usize = 1_000;
/// Most keywords of one file type (behaviour 3).
pub const MAX_KEYWORDS: usize = 2_000;
/// Longest status bar text in characters (behaviour 4).
pub const MAX_STATUS: usize = 40;

/// What a `complete` action gets (behaviour 1). Offsets count characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteContext {
    /// File path relative to the project.
    pub path: String,
    /// Language id, if known.
    pub language: Option<String>,
    /// The whole text.
    pub text: String,
    /// The caret.
    pub offset: usize,
    /// The identifier characters before the caret.
    pub prefix: String,
}

/// One completion item of a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteItem {
    /// Shown in the list.
    pub label: String,
    /// Inserted; the label when missing.
    #[serde(default)]
    pub insert: Option<String>,
    /// Shown beside the label.
    #[serde(default)]
    pub detail: Option<String>,
}

/// What a `complete` action answers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CompleteResult {
    /// Its items.
    #[serde(default)]
    pub items: Vec<CompleteItem>,
}

/// What a `status` action answers (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StatusResult {
    /// Shown in the status bar (cut to [`MAX_STATUS`] characters).
    #[serde(default)]
    pub text: String,
    /// Shown on hover.
    #[serde(default)]
    pub tooltip: Option<String>,
}

/// What a `panel` action gets (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelContext {
    /// The project folder's name.
    pub project: String,
    /// The active file relative to the project.
    pub active: Option<String>,
}

/// One row of a plugin's section in the Plugins tool window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelRow {
    /// Shown.
    pub text: String,
    /// A project file opened by a click.
    #[serde(default)]
    pub path: Option<String>,
    /// 0-based line of that file.
    #[serde(default)]
    pub line: Option<usize>,
}

/// What a `panel` action answers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PanelResult {
    /// Its rows.
    #[serde(default)]
    pub rows: Vec<PanelRow>,
}

/// A file type a plugin declares (behaviour 3): Ferris IDEA highlights it, no plugin code runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileType {
    /// Name, e.g. `TOML`.
    pub name: String,
    /// Extensions without the dot.
    pub extensions: Vec<String>,
    /// Prefix of a line comment, e.g. `#`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_comment: Option<String>,
    /// Opening and closing text of a block comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_comment: Option<[String; 2]>,
    /// Keywords.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Whether keywords match case-sensitively (the default).
    #[serde(default = "yes")]
    pub case_sensitive: bool,
}

fn yes() -> bool {
    true
}

impl FileType {
    /// Whether this file type is for `path`.
    pub fn matches(&self, path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            self.extensions
                .iter()
                .any(|x| x.trim_start_matches('.').eq_ignore_ascii_case(e))
        })
    }
}

/// Checks the parts of a manifest the extension points rely on.
pub(crate) fn check_manifest(manifest: &Manifest) -> Result<(), String> {
    // Run configuration types only with the reviewed capability (SPEC-0276 behaviour 1).
    if manifest
        .actions
        .iter()
        .any(|a| a.kind == ActionKind::RunConfig)
        && !manifest
            .permissions
            .contains(&crate::Permission::SuggestCommands)
    {
        return Err("a run-config action needs the permission suggest-commands".into());
    }
    // Connectors only with the reviewed network capability (SPEC-0314 behaviour 1).
    if manifest
        .actions
        .iter()
        .any(|a| a.kind == ActionKind::Connector)
        && !manifest
            .permissions
            .contains(&crate::Permission::ProfileNetwork)
    {
        return Err("a connector action needs the permission profile-network".into());
    }
    if manifest.author.chars().count() > 80 || manifest.author.contains('\n') {
        return Err("the author must be one line of at most 80 characters".into());
    }
    for t in &manifest.file_types {
        if t.name.trim().is_empty() || t.extensions.iter().all(|e| e.trim().is_empty()) {
            return Err("a file type needs a name and an extension".into());
        }
        if t.keywords.len() > MAX_KEYWORDS {
            return Err(format!(
                "file type {} has more than {MAX_KEYWORDS} keywords",
                t.name
            ));
        }
        if t.line_comment.as_deref() == Some("")
            || t.block_comment
                .as_ref()
                .is_some_and(|[o, c]| o.is_empty() || c.is_empty())
        {
            return Err(format!("file type {} has an empty comment marker", t.name));
        }
    }
    Ok(())
}

/// The file type of `path` among the enabled `plugins` (behaviour 3): none when Ferris IDEA has a
/// language of its own for it (`builtin`), else the first plugin's (by file name) that declares it.
pub fn file_type_for<'a>(
    plugins: &'a [Plugin],
    path: &Path,
    builtin: bool,
) -> Option<(&'a Plugin, &'a FileType)> {
    if builtin {
        return None;
    }
    plugins.iter().find_map(|p| {
        p.manifest
            .file_types
            .iter()
            .find(|t| t.matches(path))
            .map(|t| (p, t))
    })
}

/// What a run of highlighting colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    /// A line or block comment.
    Comment,
    /// A string in `"` or `'`.
    String,
    /// A number.
    Number,
    /// A keyword.
    Keyword,
}

impl TokenKind {
    /// The highlight capture name Ferris IDEA colours it with.
    pub fn capture(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::String => "string",
            Self::Number => "number",
            Self::Keyword => "keyword",
        }
    }
}

fn starts(chars: &[char], at: usize, s: &str) -> bool {
    (at..).zip(s.chars()).all(|(i, c)| chars.get(i) == Some(&c))
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Highlights `text` as file type `t` (behaviour 3): runs of characters with their kind, in order.
pub fn highlight(t: &FileType, text: &str) -> Vec<(Range<usize>, TokenKind)> {
    let keywords: HashSet<String> = t
        .keywords
        .iter()
        .map(|k| {
            if t.case_sensitive {
                k.clone()
            } else {
                k.to_lowercase()
            }
        })
        .collect();
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        if let Some([open, close]) = &t.block_comment
            && starts(&chars, i, open)
        {
            i += open.chars().count();
            while i < chars.len() && !starts(&chars, i, close) {
                i += 1;
            }
            i = (i + close.chars().count()).min(chars.len());
            out.push((start..i, TokenKind::Comment));
            continue;
        }
        if let Some(line) = &t.line_comment
            && starts(&chars, i, line)
        {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            out.push((start..i, TokenKind::Comment));
            continue;
        }
        let c = chars[i];
        if c == '"' || c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != c && chars[i] != '\n' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            // The closing quote belongs to the string, a line break does not.
            if chars.get(i) == Some(&c) {
                i += 1;
            }
            i = i.min(chars.len());
            out.push((start..i, TokenKind::String));
            continue;
        }
        if ident(c) {
            while i < chars.len() && ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if c.is_ascii_digit() {
                // Fractions and exponents belong to the number.
                while i < chars.len() && (ident(chars[i]) || chars[i] == '.') {
                    i += 1;
                }
                out.push((start..i, TokenKind::Number));
            } else if keywords.contains(&if t.case_sensitive {
                word
            } else {
                word.to_lowercase()
            }) {
                out.push((start..i, TokenKind::Keyword));
            }
            continue;
        }
        i += 1;
    }
    out
}

/// What `manifest` contributes, as the Plugins page lists it (`SPEC-0211` behaviour 1):
/// `N actions`, `completion`, `inspections`, `status bar`, `tool window`, `file types: …`.
pub fn contributions(manifest: &Manifest) -> Vec<String> {
    let mut out = Vec::new();
    let actions = manifest
        .actions
        .iter()
        .filter(|a| !a.kind.is_extension())
        .count();
    if actions > 0 {
        out.push(format!(
            "{actions} action{}",
            if actions == 1 { "" } else { "s" }
        ));
    }
    for (kind, name) in [
        (ActionKind::Complete, "completion"),
        (ActionKind::Inspect, "inspections"),
        (ActionKind::Status, "status bar"),
        (ActionKind::Panel, "tool window"),
        (ActionKind::RunConfig, "run configurations"),
        (ActionKind::Connector, "database connector"),
    ] {
        if manifest.actions.iter().any(|a| a.kind == kind) {
            out.push(name.to_owned());
        }
    }
    if !manifest.file_types.is_empty() {
        let types: Vec<String> = manifest
            .file_types
            .iter()
            .map(|t| {
                let exts: Vec<String> = t.extensions.iter().map(|e| format!(".{e}")).collect();
                format!("{} ({})", t.name, exts.join(", "))
            })
            .collect();
        out.push(format!("file types: {}", types.join(", ")));
    }
    out
}

impl Plugin {
    fn answer<T: for<'de> Deserialize<'de>>(
        &self,
        id: &str,
        kind: ActionKind,
        input: &impl Serialize,
        fuel: u64,
        access: Access,
    ) -> Result<T, PluginError> {
        let name = self.manifest.name.clone();
        if !self
            .manifest
            .actions
            .iter()
            .any(|a| a.id == id && a.kind == kind)
        {
            return Err(PluginError::UnknownAction(name, id.to_owned()));
        }
        let input = serde_json::to_string(input)
            .map_err(|e| PluginError::Failed(name.clone(), e.to_string()))?;
        let output = self.run_in(id, &input, fuel, access)?;
        serde_json::from_str(&output)
            .map_err(|e| PluginError::Failed(name, format!("its answer: {e}")))
    }

    /// Runs the `complete` action `id` (behaviour 1): the first [`MAX_ITEMS`] items, each with its
    /// text to insert.
    pub fn run_complete(
        &self,
        id: &str,
        context: &CompleteContext,
        access: Access,
    ) -> Result<Vec<CompleteItem>, PluginError> {
        let result: CompleteResult =
            self.answer(id, ActionKind::Complete, context, INSPECT_FUEL, access)?;
        Ok(result
            .items
            .into_iter()
            .filter(|i| !i.label.is_empty())
            .take(MAX_ITEMS)
            .map(|mut i| {
                i.insert.get_or_insert_with(|| i.label.clone());
                i
            })
            .collect())
    }

    /// Runs the `status` action `id` (behaviour 4): its text cut to [`MAX_STATUS`] characters, on
    /// one line.
    pub fn run_status(
        &self,
        id: &str,
        context: &EditorContext,
        access: Access,
    ) -> Result<StatusResult, PluginError> {
        let mut result: StatusResult =
            self.answer(id, ActionKind::Status, context, INSPECT_FUEL, access)?;
        let line = result.text.lines().next().unwrap_or_default();
        result.text = if line.chars().count() > MAX_STATUS {
            let mut cut: String = line.chars().take(MAX_STATUS - 1).collect();
            cut.push('…');
            cut
        } else {
            line.to_owned()
        };
        Ok(result)
    }

    /// Runs the `panel` action `id` (behaviour 5): the first [`MAX_ROWS`] rows.
    pub fn run_panel(
        &self,
        id: &str,
        context: &PanelContext,
        access: Access,
    ) -> Result<Vec<PanelRow>, PluginError> {
        let result: PanelResult = self.answer(id, ActionKind::Panel, context, FUEL, access)?;
        Ok(result.rows.into_iter().take(MAX_ROWS).collect())
    }
}
