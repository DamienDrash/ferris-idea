//! What the context actions (⌥↩) offer besides the language server's actions, shared by the
//! window and the MCP tools (`SPEC-0076` v11): the run marker's targets on the caret's line
//! (behaviour 14), the submenu of an intention (behaviour 15) and the preview as before/after
//! blocks (behaviour 16).

use std::path::Path;

use ferris_idea_core::{ProjectModel, RunConfig};
use serde::Serialize;

use crate::actions::CodeAction;
use crate::jvm::{MarkerKind, RunMarker};
use crate::rename::{Edit, FileEdits, apply_edits};

/// The kind of a run row in the list; no language server uses it.
pub const RUN_KIND: &str = "ferris-idea.run";

/// What a run row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// Runs the configuration.
    Run,
    /// Debugs it.
    Debug,
    /// Profiles its CPU.
    Profile,
}

/// A run row: its title, what it does and the configuration it starts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunEntry {
    /// The row's title, "Run '<name>'".
    pub title: String,
    /// What it does.
    #[serde(rename = "action")]
    pub mode: RunMode,
    /// The configuration, named as in the ▶ menu.
    #[serde(rename = "configuration")]
    pub config: RunConfig,
}

impl RunEntry {
    /// The row as an action of the list.
    pub fn as_action(&self) -> CodeAction {
        CodeAction {
            title: self.title.clone(),
            kind: Some(RUN_KIND.into()),
            preferred: false,
            disabled: None,
            fixes: Vec::new(),
        }
    }

    /// The preview of the row: the command and the folder it runs in.
    pub fn preview(&self) -> Vec<String> {
        let mut out = vec![format!("  $ {}", self.config.command.join(" "))];
        if let Some(cwd) = self.config.cwd.as_deref().filter(|d| !d.is_empty()) {
            out.push(format!("  in {cwd}"));
        }
        out
    }
}

/// Whether `a` is a run row.
pub fn is_run(a: &CodeAction) -> bool {
    a.kind.as_deref() == Some(RUN_KIND)
}

/// The ▶ markers of the file `rel` (workspace-relative) of `root`, as the editor's gutter shows
/// them: Java and Kotlin (SPEC-0122), Vitest and Jest with a runner (SPEC-0045 v4), pytest, Go and
/// Rust tests and the `main` of Go and Rust (SPEC-0045 v5, SPEC-0097 v3).
pub fn markers_of(root: &Path, rel: &str, text: &str) -> Vec<RunMarker> {
    if let Some(lang) = crate::jvm::Jvm::of(rel) {
        return crate::jvm::run_markers(lang, text);
    }
    if ferris_idea_core::is_js_test_file(rel)
        && crate::js_tests::has_test_runner(root, &root.join(rel))
    {
        return crate::js_tests::markers(rel, text);
    }
    if crate::test_markers::test_file(rel).is_some() || crate::main_markers::applies(rel) {
        return crate::main_markers::with_tests(rel, text);
    }
    Vec::new()
}

/// The configuration the ▶ `marker` of `rel` starts (`SPEC-0122` behaviour 1, `SPEC-0097` v3
/// behaviour 8, `SPEC-0045`), as its menu does.
pub fn marker_config(
    model: &ProjectModel,
    rel: &str,
    text: &str,
    marker: &RunMarker,
) -> Option<RunConfig> {
    match (&marker.kind, &marker.method) {
        (MarkerKind::Main, _) if crate::main_markers::applies(rel) => {
            ferris_idea_core::context_config(model, rel, text)
        }
        _ if crate::test_markers::test_file(rel).is_some() => ferris_idea_core::single_test_config(
            model,
            rel,
            text,
            &marker.class,
            marker.method.as_deref(),
        ),
        (MarkerKind::TestMethod, Some(m)) => {
            ferris_idea_core::test_method_config(model, rel, text, m)
        }
        (kind, None) if ferris_idea_core::is_js_test_file(rel) => ferris_idea_core::js_test_config(
            model,
            rel,
            text,
            &marker.class,
            *kind == MarkerKind::TestClass,
        ),
        _ => ferris_idea_core::context_config(model, rel, text),
    }
}

/// The run rows of the ▶ marker on 0-based `line` of `rel` (behaviour 14): Run; Debug for a
/// `main` and a Jest test; Profile for a `main` whose configuration `profilable` accepts. None
/// without a marker on the line or without a configuration.
pub fn run_entries(
    model: &ProjectModel,
    root: &Path,
    rel: &str,
    text: &str,
    line: usize,
    profilable: impl Fn(&RunConfig) -> bool,
) -> Vec<RunEntry> {
    let Some(marker) = markers_of(root, rel, text)
        .into_iter()
        .find(|m| m.line == line)
    else {
        return Vec::new();
    };
    let Some(config) = marker_config(model, rel, text, &marker) else {
        return Vec::new();
    };
    let main = marker.kind == MarkerKind::Main;
    let mut out = vec![RunEntry {
        title: format!("Run '{}'", config.name),
        mode: RunMode::Run,
        config: config.clone(),
    }];
    if main || crate::debug_plan::is_npx_jest(&config.command) {
        out.push(RunEntry {
            title: format!("Debug '{}'", config.name),
            mode: RunMode::Debug,
            config: config.clone(),
        });
    }
    if main && profilable(&config) {
        out.push(RunEntry {
            title: format!("Profile '{}'", config.name),
            mode: RunMode::Profile,
            config,
        });
    }
    out
}

/// The byte offset of 0-based `line` and `character` (Unicode scalar values) in `text`, clamped to
/// the line's end.
pub fn byte_at(text: &str, line: usize, character: usize) -> usize {
    let mut at = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i == line {
            let body = l.strip_suffix('\n').unwrap_or(l);
            return at
                + body
                    .char_indices()
                    .nth(character)
                    .map_or(body.len(), |(b, _)| b);
        }
        at += l.len();
    }
    text.len()
}

/// The label of the option that turns an intention off.
pub fn disable_label(title: &str) -> String {
    format!("Disable '{title}'")
}

/// The label of the option that turns a disabled intention on again (MCP only).
pub fn enable_label(title: &str) -> String {
    format!("Enable '{title}'")
}

/// The label of the option that opens the intention settings.
pub const SETTINGS_LABEL: &str = "Edit intention settings";

/// An option of an intention's submenu (behaviour 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentionOption {
    /// Turn the intention off.
    Disable,
    /// Turn it on again.
    Enable,
    /// Open the intention settings.
    Settings,
}

impl IntentionOption {
    /// The option `label` stands for in the submenu of the intention titled `title`.
    pub fn of_label(title: &str, label: &str) -> Option<Self> {
        if label == disable_label(title) {
            Some(Self::Disable)
        } else if label == enable_label(title) {
            Some(Self::Enable)
        } else if label == SETTINGS_LABEL {
            Some(Self::Settings)
        } else {
            None
        }
    }
}

/// Whether `a` is an intention of the language server: no quick fix and no run row.
pub fn is_intention(a: &CodeAction) -> bool {
    let fix = !a.fixes.is_empty() || a.kind.as_deref().is_some_and(|k| k.starts_with("quickfix"));
    !fix && !is_run(a)
}

/// The submenu of a language server's action (behaviour 15): Disable and the settings for an
/// intention, nothing for a quick fix or a run row.
pub fn intention_options(a: &CodeAction) -> Vec<String> {
    if is_intention(a) {
        vec![disable_label(&a.title), SETTINGS_LABEL.to_owned()]
    } else {
        Vec::new()
    }
}

/// One changed block of a file: its first 0-based line, the lines before and after; `after` is
/// `None` when the file changed since the action was planned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    /// First 0-based line of the block.
    pub line: usize,
    /// The lines before.
    pub before: Vec<String>,
    /// The lines after, or `None` when the edits no longer fit the text.
    pub after: Option<Vec<String>>,
}

/// The preview of one file (behaviour 16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewFile {
    /// The path, relative to the workspace where it lies inside.
    pub path: String,
    /// The changed blocks; `None` when the file cannot be read.
    pub hunks: Option<Vec<Hunk>>,
}

/// The changed blocks of `files` (behaviours 7 and 16): edits that share lines form one block.
/// `text_of` gives a file's current text (the editor's, else the disk's).
pub fn preview_files(
    files: &[FileEdits],
    root: &Path,
    text_of: impl Fn(&Path) -> Option<String>,
) -> Vec<PreviewFile> {
    files
        .iter()
        .map(|file| {
            let path = file
                .path
                .strip_prefix(root)
                .unwrap_or(&file.path)
                .display()
                .to_string();
            let hunks = text_of(&file.path).map(|text| hunks_of(&text, &file.edits));
            PreviewFile { path, hunks }
        })
        .collect()
}

fn hunks_of(text: &str, edits: &[Edit]) -> Vec<Hunk> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut groups: Vec<(usize, usize, Vec<&Edit>)> = Vec::new();
    for e in edits {
        let (a, b) = (e.range.start_line as usize, e.range.end_line as usize);
        match groups.last_mut() {
            Some((_, end, list)) if a <= *end => {
                *end = (*end).max(b);
                list.push(e);
            }
            _ => groups.push((a, b, vec![e])),
        }
    }
    groups
        .into_iter()
        .map(|(a, b, list)| {
            let b = b.min(lines.len().saturating_sub(1));
            let old = lines.get(a..=b).map(|l| l.join("\n")).unwrap_or_default();
            let shifted: Vec<Edit> = list
                .iter()
                .map(|e| {
                    let mut e = (*e).clone();
                    e.range.start_line -= a as u64;
                    e.range.end_line -= a as u64;
                    e
                })
                .collect();
            let after = apply_edits(&old, &shifted)
                .ok()
                .map(|new| new.split('\n').map(str::to_owned).collect());
            Hunk {
                line: a,
                before: old.split('\n').map(str::to_owned).collect(),
                after,
            }
        })
        .collect()
}

/// The changed blocks of byte `edits` of `text` (Ferris IDEA's own fixes, behaviour 17): edits on
/// the same or overlapping lines form one block.
pub fn byte_hunks(text: &str, edits: &[crate::jvm::TextEdit]) -> Vec<Hunk> {
    let mut sorted: Vec<&crate::jvm::TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| e.range.start);
    let len = text.len();
    let line_of = |b: usize| text[..b.min(len)].matches('\n').count();
    let mut groups: Vec<(usize, usize, Vec<&crate::jvm::TextEdit>)> = Vec::new();
    for e in sorted {
        let (a, b) = (line_of(e.range.start), line_of(e.range.end));
        match groups.last_mut() {
            Some((_, end, list)) if a <= *end => {
                *end = (*end).max(b);
                list.push(e);
            }
            _ => groups.push((a, b, vec![e])),
        }
    }
    groups
        .into_iter()
        .map(|(a, _, list)| {
            let first = list.first().map_or(0, |e| e.range.start.min(len));
            let last = list
                .iter()
                .map(|e| e.range.end.min(len))
                .max()
                .unwrap_or(first);
            let start = text[..first].rfind('\n').map_or(0, |i| i + 1);
            let end = text[last..].find('\n').map_or(len, |i| last + i);
            let mut after = String::new();
            let mut at = start;
            for e in &list {
                let (x, y) = (e.range.start.clamp(at, end), e.range.end.clamp(at, end));
                after.push_str(&text[at..x]);
                after.push_str(&e.text);
                at = y;
            }
            after.push_str(&text[at..end]);
            Hunk {
                line: a,
                before: text[start..end].split('\n').map(str::to_owned).collect(),
                after: Some(after.split('\n').map(str::to_owned).collect()),
            }
        })
        .collect()
}

/// A block of the preview pane (behaviour 16), read from the preview's lines: a file's path, a
/// changed block side by side, or a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewBlock {
    /// A file's path.
    File(String),
    /// The lines before and after.
    Change {
        /// Before.
        before: Vec<String>,
        /// After.
        after: Vec<String>,
    },
    /// A note, such as "… 4 more" or a command.
    Note(String),
}

/// The blocks of the preview's lines (`- old`, `+ new`, a path, an indented note): a `-` line
/// after a `+` line starts a new block.
pub fn blocks_of_lines(lines: &[String]) -> Vec<PreviewBlock> {
    let mut out: Vec<PreviewBlock> = Vec::new();
    for l in lines {
        let (minus, plus) = (l.strip_prefix("- "), l.strip_prefix("+ "));
        if minus.is_some() || plus.is_some() {
            let open = matches!(
                out.last(),
                Some(PreviewBlock::Change { after, .. }) if minus.is_none() || after.is_empty()
            );
            if !open {
                out.push(PreviewBlock::Change {
                    before: Vec::new(),
                    after: Vec::new(),
                });
            }
            if let Some(PreviewBlock::Change { before, after }) = out.last_mut() {
                match (minus, plus) {
                    (Some(m), _) => before.push(m.to_owned()),
                    (_, Some(p)) => after.push(p.to_owned()),
                    _ => {}
                }
            }
        } else if l.starts_with(' ') {
            out.push(PreviewBlock::Note(l.trim().to_owned()));
        } else {
            out.push(PreviewBlock::File(l.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVENTORY: &str = "package shop;\n\npublic class Inventory {\n    public static void main(String[] args) {\n        System.out.println(1);\n    }\n\n    int count() { return 0; }\n}\n";

    fn model() -> ProjectModel {
        ProjectModel::default()
    }

    /// SPEC-0076 v11 AK-11: the class line and the `main` line offer Run, Debug and, when the
    /// profiler can run it, Profile with the ▶ menu's configuration; another line none.
    #[test]
    fn a_main_class_offers_run_debug_and_profile_at_its_marker() {
        let root = Path::new("/nowhere");
        let rel = "src/shop/Inventory.java";
        for line in [2, 3] {
            let rows = run_entries(&model(), root, rel, INVENTORY, line, |_| true);
            let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
            assert_eq!(
                titles,
                [
                    "Run 'Inventory'",
                    "Debug 'Inventory'",
                    "Profile 'Inventory'"
                ],
                "line {line}"
            );
            // A class in a source folder runs with its module compiled (SPEC-0097 v4
            // behaviour 10), as the ▶ menu runs it.
            let command = &rows[0].config.command;
            assert_eq!(command[..2], ["sh", "-c"], "{command:?}");
            assert!(command[2].ends_with("java -cp 'out/production/main' 'shop.Inventory'"));
            assert_eq!(
                rows.iter().map(|r| r.mode).collect::<Vec<_>>(),
                [RunMode::Run, RunMode::Debug, RunMode::Profile]
            );
            assert!(rows.iter().all(|r| is_run(&r.as_action())));
        }
        let rows = run_entries(&model(), root, rel, INVENTORY, 3, |_| false);
        assert_eq!(rows.len(), 2, "no Profile when the profiler cannot run it");
        for line in [0, 4, 7] {
            assert!(run_entries(&model(), root, rel, INVENTORY, line, |_| true).is_empty());
        }
        let shown = &rows[0].preview()[0];
        assert!(
            shown.starts_with("  $ ") && shown.contains("javac"),
            "{shown}"
        );
    }

    /// A Go program's `main` and a Rust test: the test only runs.
    #[test]
    fn a_test_offers_only_run() {
        let root = Path::new("/nowhere");
        let text = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {}\n}\n";
        let rows = run_entries(&model(), root, "src/lib.rs", text, 3, |_| true);
        let modes: Vec<RunMode> = rows.iter().map(|r| r.mode).collect();
        assert!(modes.is_empty() || modes == [RunMode::Run], "{rows:?}");
    }

    /// SPEC-0076 v11 AK-12: an intention has Disable and the settings, a quick fix and a run row
    /// nothing; the labels map back to their option.
    #[test]
    fn intentions_have_disable_and_settings() {
        let a = |title: &str, kind: Option<&str>, fixes: &[&str]| CodeAction {
            title: title.into(),
            kind: kind.map(str::to_owned),
            preferred: false,
            disabled: None,
            fixes: fixes.iter().map(|f| (*f).to_owned()).collect(),
        };
        let javadoc = a("Add Javadoc for 'Inventory'", Some("source"), &[]);
        assert_eq!(
            intention_options(&javadoc),
            [
                "Disable 'Add Javadoc for 'Inventory''",
                "Edit intention settings"
            ]
        );
        assert!(intention_options(&a("Insert ';'", Some("quickfix"), &[])).is_empty());
        assert!(intention_options(&a("Change type", None, &["Type mismatch"])).is_empty());
        let run = run_entries(
            &model(),
            Path::new("/nowhere"),
            "src/shop/Inventory.java",
            INVENTORY,
            2,
            |_| true,
        )
        .remove(0);
        assert!(intention_options(&run.as_action()).is_empty());
        let t = &javadoc.title;
        assert_eq!(
            IntentionOption::of_label(t, &disable_label(t)),
            Some(IntentionOption::Disable)
        );
        assert_eq!(
            IntentionOption::of_label(t, &enable_label(t)),
            Some(IntentionOption::Enable)
        );
        assert_eq!(
            IntentionOption::of_label(t, SETTINGS_LABEL),
            Some(IntentionOption::Settings)
        );
        assert_eq!(IntentionOption::of_label(t, "Disable 'other'"), None);
    }

    /// SPEC-0076 v11 AK-13: the preview's lines become before/after blocks per file.
    #[test]
    fn preview_lines_become_blocks() {
        let lines: Vec<String> = [
            "src/a.c",
            "-     int x = 1",
            "+     int x = 1;",
            "- a",
            "- b",
            "+ ab",
            "src/b.c",
            "  … 4 more",
            "  $ java A.java",
        ]
        .map(String::from)
        .to_vec();
        let change = |b: &[&str], a: &[&str]| PreviewBlock::Change {
            before: b.iter().map(|s| (*s).to_owned()).collect(),
            after: a.iter().map(|s| (*s).to_owned()).collect(),
        };
        assert_eq!(
            blocks_of_lines(&lines),
            [
                PreviewBlock::File("src/a.c".into()),
                change(&["    int x = 1"], &["    int x = 1;"]),
                change(&["a", "b"], &["ab"]),
                PreviewBlock::File("src/b.c".into()),
                PreviewBlock::Note("… 4 more".into()),
                PreviewBlock::Note("$ java A.java".into()),
            ]
        );
    }

    #[test]
    fn byte_edits_become_blocks() {
        let text = "a = 1\nb = 2\nc = 3\n";
        let edit = |from: usize, to: usize, with: &str| crate::jvm::TextEdit {
            range: from..to,
            text: with.into(),
        };
        let hunks = byte_hunks(text, &[edit(16, 17, "4"), edit(4, 5, "9")]);
        assert_eq!(hunks.len(), 2);
        assert_eq!(
            (hunks[0].line, hunks[0].before.clone()),
            (0, vec!["a = 1".to_owned()])
        );
        assert_eq!(hunks[0].after, Some(vec!["a = 9".to_owned()]));
        assert_eq!(hunks[1].after, Some(vec!["c = 4".to_owned()]));
        let removed = byte_hunks(text, &[edit(6, 12, "")]);
        assert_eq!(removed[0].before, ["b = 2", "c = 3"]);
        assert_eq!(removed[0].after, Some(vec!["c = 3".to_owned()]));
    }

    #[test]
    fn preview_files_compute_blocks_without_applying() {
        use crate::insight::Range;
        let edit = Edit {
            range: Range {
                start_line: 1,
                start_character: 13,
                end_line: 1,
                end_character: 13,
            },
            old_text: String::new(),
            new_text: ";".into(),
        };
        let files = vec![FileEdits {
            path: "/p/src/a.c".into(),
            edits: vec![edit],
        }];
        let text = "int main() {\n    int x = 1\n}\n";
        let out = preview_files(&files, Path::new("/p"), |_| Some(text.to_owned()));
        assert_eq!(out[0].path, "src/a.c");
        assert_eq!(
            out[0].hunks.as_deref(),
            Some(
                &[Hunk {
                    line: 1,
                    before: vec!["    int x = 1".into()],
                    after: Some(vec!["    int x = 1;".into()]),
                }][..]
            )
        );
        let changed = preview_files(&files, Path::new("/p"), |_| Some("x\ny\n".into()));
        assert_eq!(changed[0].hunks.as_ref().unwrap()[0].after, None);
        assert_eq!(
            preview_files(&files, Path::new("/p"), |_| None)[0].hunks,
            None
        );
    }
}
