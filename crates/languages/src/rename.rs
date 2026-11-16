//! Rename through the language server (`SPEC-0040`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::insight::{Opened, Range, parse_range, uri_to_path};
use crate::lsp::LspError;

/// One replacement in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edit {
    /// Replaced range (0-based lines, characters in Unicode scalar values).
    pub range: Range,
    /// Text currently in the range.
    pub old_text: String,
    /// Replacement.
    pub new_text: String,
}

/// All edits of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEdits {
    /// The file.
    pub path: PathBuf,
    /// Edits in document order.
    pub edits: Vec<Edit>,
}

/// What a rename changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenamePlan {
    /// Current name of the symbol.
    pub old_name: String,
    /// New name.
    pub new_name: String,
    /// Affected files, sorted by path.
    pub files: Vec<FileEdits>,
}

/// Byte offset of (line, character) in `text`.
fn offset(text: &str, line: u64, character: u64) -> Option<usize> {
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n')? + 1;
    }
    let line_text = &text[start..text[start..].find('\n').map_or(text.len(), |e| start + e)];
    let mut chars = line_text.char_indices();
    let mut at = line_text.len();
    for _ in 0..character {
        chars.next()?;
    }
    if let Some((i, _)) = chars.next() {
        at = i;
    }
    Some(start + at)
}

/// Text of `range` in `text`.
pub fn range_text(text: &str, range: &Range) -> Option<String> {
    let a = offset(text, range.start_line, range.start_character)?;
    let b = offset(text, range.end_line, range.end_character)?;
    text.get(a..b).map(str::to_owned)
}

/// Applies `edits` to `text`, checking each edit's old text (behaviour 4).
pub fn apply_edits(text: &str, edits: &[Edit]) -> Result<String, String> {
    let mut spans = Vec::new();
    for e in edits {
        let a = offset(text, e.range.start_line, e.range.start_character);
        let b = offset(text, e.range.end_line, e.range.end_character);
        let (Some(a), Some(b)) = (a, b) else {
            return Err("an edit lies outside the file".into());
        };
        if text.get(a..b) != Some(e.old_text.as_str()) {
            return Err("the file changed since the preview".into());
        }
        spans.push((a, b, &e.new_text));
    }
    spans.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = text.to_owned();
    for (a, b, new) in spans {
        out.replace_range(a..b, new);
    }
    Ok(out)
}

/// Writes a plan: every file is checked first, then all are written (behaviour 4).
pub fn apply_plan(plan: &RenamePlan) -> Result<Vec<PathBuf>, String> {
    apply_files(&plan.files)
}

/// Writes file edits: every file is checked first, then all are written.
pub fn apply_files(files: &[FileEdits]) -> Result<Vec<PathBuf>, String> {
    let mut results = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file.path)
            .map_err(|e| format!("{}: {e}", file.path.display()))?;
        let new =
            apply_edits(&text, &file.edits).map_err(|e| format!("{}: {e}", file.path.display()))?;
        results.push((file.path.clone(), new));
    }
    for (path, text) in &results {
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(results.into_iter().map(|(p, _)| p).collect())
}

/// Where the Java file at `path` goes when the type `old` it is named after is renamed to `new`
/// (`SPEC-0148` v2 behaviour 6, D29): a `.java` file named `old.java` that declares `old` as a
/// class, interface, enum or record (`text` is its text) goes to `new.java` in the same folder,
/// unless that file exists. Java wants a public top-level type in the file of its name; jdtls
/// renames the type but not the file.
pub fn java_type_file(path: &Path, text: &str, old: &str, new: &str) -> Option<PathBuf> {
    if path.extension().is_none_or(|e| e != "java")
        || path.file_stem().is_none_or(|s| s != old)
        || old == new
        || new.is_empty()
    {
        return None;
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let declares = ["class", "interface", "enum", "record", "@interface"]
        .iter()
        .any(|kw| {
            text.match_indices(&format!("{kw} {old}")).any(|(i, m)| {
                let before = text[..i].chars().next_back();
                let after = text[i + m.len()..].chars().next();
                !before.is_some_and(word) && !after.is_some_and(word)
            })
        });
    let to = path.with_file_name(format!("{new}.java"));
    (declares && !to.exists()).then_some(to)
}

/// The Java files a rename moves with their type (see [`java_type_file`]), as (from, to), reading
/// each file's text with `text_of`.
pub fn java_type_files(
    plan: &RenamePlan,
    text_of: impl Fn(&Path) -> Option<String>,
) -> Vec<(PathBuf, PathBuf)> {
    plan.files
        .iter()
        .filter_map(|f| {
            let text = text_of(&f.path)?;
            let to = java_type_file(&f.path, &text, &plan.old_name, &plan.new_name)?;
            Some((f.path.clone(), to))
        })
        .collect()
}

/// Collects the text edits of a `WorkspaceEdit` per URI (behaviour 3).
fn workspace_edits(v: &Value) -> Result<BTreeMap<String, Vec<Value>>, LspError> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    if let Some(changes) = v["changes"].as_object() {
        for (uri, edits) in changes {
            out.entry(uri.clone())
                .or_default()
                .extend(edits.as_array().cloned().unwrap_or_default());
        }
    }
    for change in v["documentChanges"].as_array().into_iter().flatten() {
        if change.get("kind").is_some() {
            return Err(LspError::Protocol(format!(
                "the server wants to {} a file, which rename does not support",
                change["kind"].as_str().unwrap_or("change")
            )));
        }
        let uri = change["textDocument"]["uri"].as_str().unwrap_or_default();
        out.entry(uri.to_owned())
            .or_default()
            .extend(change["edits"].as_array().cloned().unwrap_or_default());
    }
    Ok(out)
}

/// Text edits of several workspace edits, merged per URI.
pub(crate) fn merge_workspace_edits(
    edits: &[Value],
) -> Result<BTreeMap<String, Vec<Value>>, LspError> {
    let mut by_uri: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for edit in edits {
        for (uri, list) in workspace_edits(edit)? {
            by_uri.entry(uri).or_default().extend(list);
        }
    }
    Ok(by_uri)
}

/// Turns workspace edits into file edits with the current text of each range; several
/// workspace edits are merged per file (SPEC-0041 AK-2).
pub(crate) fn file_edits(o: &mut Opened, edits: &[Value]) -> Result<Vec<FileEdits>, LspError> {
    let mut files = Vec::new();
    for (uri, edits) in merge_workspace_edits(edits)? {
        let path = uri_to_path(&uri)
            .ok_or_else(|| LspError::Protocol(format!("unsupported URI {uri}")))?;
        // The opened files' texts are what the server saw (maybe unsaved editor text, SPEC-0220
        // v4 behaviour 13), not the disk.
        let text = match o.files.texts.get(&uri) {
            Some(t) => t.clone(),
            None => o
                .files
                .read(&path)
                .map_err(|e| LspError::Io(e.to_string()))?,
        };
        let mut out = Vec::new();
        for e in edits {
            let range = parse_range(&e["range"], &uri, &mut o.files)
                .ok_or_else(|| LspError::Protocol("edit without a range".into()))?;
            out.push(Edit {
                old_text: range_text(&text, &range).unwrap_or_default(),
                new_text: e["newText"].as_str().unwrap_or_default().to_owned(),
                range,
            });
        }
        out.sort_by_key(|e| (e.range.start_line, e.range.start_character));
        files.push(FileEdits { path, edits: out });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Plans renaming the symbol at (line, character) of `file` to `new_name` (behaviours 1–3).
pub async fn plan_rename(
    root: &Path,
    file: &Path,
    line: u64,
    character: u64,
    new_name: &str,
    wait: Duration,
) -> Result<RenamePlan, LspError> {
    plan_rename_in(root, file, None, line, character, new_name, wait).await
}

/// Like [`plan_rename`], on `text` instead of the saved file when given (unsaved editor text); edits
/// of that file refer to `text` (SPEC-0074).
pub async fn plan_rename_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    new_name: &str,
    wait: Duration,
) -> Result<RenamePlan, LspError> {
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let position = o.position(line, character);
        let mut old_name = None;
        // A server that says it cannot rename is not asked (`SPEC-0023` v3 behaviour 11).
        if o.capabilities["renameProvider"] == false {
            return Err(o.unsupported("textDocument/rename"));
        }
        if o.capabilities["renameProvider"]["prepareProvider"] == true {
            let answer = o
                .ask("textDocument/prepareRename", position.clone())
                .await?;
            if answer.is_null() {
                return Err(LspError::Protocol("nothing to rename here".into()));
            }
            old_name = answer["placeholder"].as_str().map(str::to_owned);
            if old_name.is_none() {
                let range_value = answer.get("range").unwrap_or(&answer);
                let uri = o.uri.clone();
                if let Some(r) = parse_range(range_value, &uri, &mut o.files) {
                    old_name = range_text(&o.text, &r);
                }
            }
        }
        let mut params = position;
        params["newName"] = json!(new_name);
        let answer = o.ask("textDocument/rename", params).await?;
        if answer.is_null() {
            return Err(LspError::Protocol("nothing to rename here".into()));
        }
        let files = file_edits(&mut o, &[answer])?;
        let old_name = old_name
            .or_else(|| {
                files
                    .iter()
                    .flat_map(|f| &f.edits)
                    .next()
                    .map(|e| e.old_text.clone())
            })
            .unwrap_or_default();
        Ok(RenamePlan {
            old_name,
            new_name: new_name.to_owned(),
            files,
        })
    }
    .await;
    o.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(sl: u64, sc: u64, el: u64, ec: u64, old: &str, new: &str) -> Edit {
        Edit {
            range: Range {
                start_line: sl,
                start_character: sc,
                end_line: el,
                end_character: ec,
            },
            old_text: old.into(),
            new_text: new.into(),
        }
    }

    #[test]
    fn applying_edits() {
        let text = "int add(int a);\nint x = add(1) + add(2);\näö add\n";
        let edits = [
            edit(1, 17, 1, 20, "add", "sum"),
            edit(0, 4, 0, 7, "add", "sum"),
            edit(1, 8, 1, 11, "add", "sum"),
            edit(2, 3, 2, 6, "add", "sum"),
        ];
        assert_eq!(
            apply_edits(text, &edits).unwrap(),
            "int sum(int a);\nint x = sum(1) + sum(2);\näö sum\n"
        );
        // Across lines.
        let joined = apply_edits("ab\ncd\n", &[edit(0, 1, 1, 1, "b\nc", "-")]).unwrap();
        assert_eq!(joined, "a-d\n");
        // The file changed since the preview.
        assert!(apply_edits("int sub;", &[edit(0, 4, 0, 7, "add", "sum")]).is_err());
        assert!(apply_edits("x", &[edit(5, 0, 5, 1, "", "y")]).is_err());
    }

    /// SPEC-0148 v2 behaviour 6 (hand test 74, D29).
    #[test]
    fn a_java_type_takes_its_file_along() {
        let dir = std::env::temp_dir().join(format!("idecraft-java-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lager = dir.join("Lager.java");
        let text = "package demo;\n\npublic class Lager {\n}\n";
        assert_eq!(
            java_type_file(&lager, text, "Lager", "Speicher"),
            Some(dir.join("Speicher.java"))
        );
        for t in [
            "interface Lager {}",
            "record Lager(int n) {}",
            "enum Lager { A }",
        ] {
            assert!(
                java_type_file(&lager, t, "Lager", "Speicher").is_some(),
                "{t}"
            );
        }
        // Another file, another type, a field of that type, Kotlin: stays.
        assert_eq!(
            java_type_file(&dir.join("App.java"), text, "Lager", "Speicher"),
            None
        );
        assert_eq!(
            java_type_file(&lager, "class Lagerhaus {}", "Lager", "Speicher"),
            None
        );
        assert_eq!(
            java_type_file(&lager, "class A { Lager l; }", "Lager", "Speicher"),
            None
        );
        assert_eq!(
            java_type_file(&dir.join("Lager.kt"), "class Lager", "Lager", "Speicher"),
            None
        );
        // The target exists.
        std::fs::write(dir.join("Speicher.java"), "").unwrap();
        assert_eq!(java_type_file(&lager, text, "Lager", "Speicher"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merging_workspace_edits() {
        let e = |l: u64| json!({"range": {"start": {"line": l, "character": 0}, "end": {"line": l, "character": 1}}, "newText": "x"});
        let a = json!({"changes": {"file:///a.c": [e(0)], "file:///b.c": [e(1)]}});
        let b = json!({"documentChanges": [{"textDocument": {"uri": "file:///a.c", "version": 1}, "edits": [e(2)]}]});
        let merged = merge_workspace_edits(&[a, b]).unwrap();
        assert_eq!(merged["file:///a.c"].len(), 2);
        assert_eq!(merged["file:///b.c"].len(), 1);
        let create = json!({"documentChanges": [{"kind": "create", "uri": "file:///c.c"}]});
        assert!(merge_workspace_edits(&[create]).is_err());
    }

    #[test]
    fn stale_file_refuses_whole_plan() {
        let dir = std::env::temp_dir().join(format!("idecraft-rename-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.c"), dir.join("b.c"));
        std::fs::write(&a, "add").unwrap();
        std::fs::write(&b, "changed").unwrap();
        let plan = RenamePlan {
            old_name: "add".into(),
            new_name: "sum".into(),
            files: vec![
                FileEdits {
                    path: a.clone(),
                    edits: vec![edit(0, 0, 0, 3, "add", "sum")],
                },
                FileEdits {
                    path: b.clone(),
                    edits: vec![edit(0, 0, 0, 3, "add", "sum")],
                },
            ],
        };
        assert!(apply_plan(&plan).is_err());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "add");
        std::fs::write(&b, "add").unwrap();
        assert_eq!(apply_plan(&plan).unwrap().len(), 2);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "sum");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
