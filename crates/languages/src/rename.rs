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

/// The one edit that turns `before` into `after`: the part between their common start and common
/// end (characters), so an editor keeps everything around it, the caret included. Used by fixes
/// that compute a file's new text, such as adding a missing bundle key (`SPEC-0163` v3).
pub fn edit_between(before: &str, after: &str) -> Edit {
    let prefix: usize = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let suffix: usize = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let position = |byte: usize| {
        let head = &before[..byte];
        let line_start = head.rfind('\n').map_or(0, |i| i + 1);
        (
            head.matches('\n').count() as u64,
            head[line_start..].chars().count() as u64,
        )
    };
    let (start_line, start_character) = position(prefix);
    let (end_line, end_character) = position(before.len() - suffix);
    Edit {
        range: Range {
            start_line,
            start_character,
            end_line,
            end_character,
        },
        old_text: before[prefix..before.len() - suffix].to_owned(),
        new_text: after[prefix..after.len() - suffix].to_owned(),
    }
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
    // A class or id of a stylesheet or an HTML file is renamed from Ferris IDEA's own syntax trees,
    // in every stylesheet and HTML file with its mark kept (`SPEC-0042` v22 behaviour 38).
    if crate::html_css::is_stylesheet(file) || crate::html_css::is_html(file) {
        let content = match text {
            Some(t) => Some(t.to_owned()),
            None => crate::shown::read(file).ok(),
        };
        if let Some(own) = content
            .and_then(|t| crate::html_css::rename_plan(root, file, &t, line, character, new_name))
        {
            return own.map_err(LspError::Refused);
        }
    }
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

/// Why nothing can be renamed at a place (`SPEC-0074` v2 behaviour 5): the status bar's message.
pub const NOT_RENAMEABLE: &str = "Cannot rename this element: the caret should be on an identifier";

/// The keyword, literal or built-in type at (`line`, `character`) of `text` (the file at `path`), as
/// the file's syntax tree highlights it (`SPEC-0074` v2 behaviour 5): nothing to rename there.
/// `None` on a name, or when the file has no grammar. A contextual keyword used as a name (Java
/// `var record = …`) is a name in the tree and so not refused.
pub fn keyword_at(path: &Path, text: &str, line: usize, character: usize) -> Option<String> {
    let language = crate::LanguageId::detect(path)?;
    let tsx = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tsx"));
    let doc = crate::syntax::SyntaxDocument::new(language, tsx, text).ok()?;
    let line_start: usize = text
        .split_inclusive('\n')
        .take(line)
        .map(|l| l.chars().count())
        .sum();
    let line_len = text
        .split_inclusive('\n')
        .nth(line)
        .map_or(0, |l| l.chars().count());
    let at = line_start + character;
    // The whole line, so a span is not cut at the caret; right after a word still means it.
    let spans = doc.highlight_range(line_start..line_start + line_len);
    let span = spans
        .iter()
        .find(|s| s.start <= at && at < s.end)
        .or_else(|| spans.iter().find(|s| s.end == at))?;
    let not_a_name = span.kind.starts_with("keyword")
        || matches!(
            span.kind.as_str(),
            "type.builtin" | "constant.builtin" | "boolean"
        );
    not_a_name.then(|| {
        text.chars()
            .skip(span.start)
            .take(span.end - span.start)
            .collect()
    })
}

/// Asks the server whether the symbol at (`line`, `character`) of `file` can be renamed
/// (`textDocument/prepareRename`, `SPEC-0074` v2 behaviour 5): `Some(false)` when it answers
/// `null` or with an error (the protocol's way to say there is nothing to rename there),
/// `Some(true)` when it names a range, `None` when it cannot tell (no server, no
/// `prepareProvider`, no answer in time, the document changed meanwhile).
pub async fn can_rename(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Option<bool> {
    let o = Opened::open_text(root, file, text, wait).await.ok()?;
    let answer = async {
        // A server without rename leaves the place to Ferris IDEA's own text rename.
        if o.capabilities["renameProvider"]["prepareProvider"] != true {
            return None;
        }
        let position = o.position(line, character);
        match o.ask("textDocument/prepareRename", position).await {
            Ok(answer) => Some(!answer.is_null()),
            // Content modified (-32801) and cancelled (-32800) say nothing about the place.
            Err(LspError::Response { code, .. }) if code != -32801 && code != -32800 => Some(false),
            Err(_) => None,
        }
    }
    .await;
    o.close().await;
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0163 v3 behaviour 3: the edit of a computed new text is only the changed part.
    #[test]
    fn edit_between_two_texts() {
        let before = "a=1\nä=2";
        let after = "a=1\nä=2\nkey=\n";
        let e = edit_between(before, after);
        assert_eq!((e.range.start_line, e.range.start_character), (1, 3));
        assert_eq!((e.range.end_line, e.range.end_character), (1, 3));
        assert_eq!(e.old_text, "");
        assert_eq!(e.new_text, "\nkey=\n");
        assert_eq!(apply_edits(before, &[e]).unwrap(), after);
        let same = edit_between("x", "x");
        assert!(same.old_text.is_empty() && same.new_text.is_empty());
    }

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
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-java-file-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("ferris-idea-rename-{}", std::process::id()));
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

    /// SPEC-0074 v2 AK-4 (window check A12, R1): keywords, literals and built-in types are not
    /// offered for renaming, names are; a contextual keyword used as a name is a name.
    #[test]
    fn keywords_are_not_renameable() {
        let java = Path::new("/p/src/Lab.java");
        let text = "package lab;\n\npublic record Item(String name, int count) {\n    void f() { var record = 1; boolean b = true; }\n}\n";
        // `record` (line 2, column 7), on its first letter and right after it.
        assert_eq!(keyword_at(java, text, 2, 7).as_deref(), Some("record"));
        assert_eq!(keyword_at(java, text, 2, 10).as_deref(), Some("record"));
        assert_eq!(keyword_at(java, text, 2, 0).as_deref(), Some("public"));
        assert_eq!(keyword_at(java, text, 2, 32).as_deref(), Some("int"));
        // The record's name and a parameter are names.
        assert_eq!(keyword_at(java, text, 2, 15), None);
        assert_eq!(keyword_at(java, text, 2, 26), None);
        // `var record = 1`: here `record` is a variable.
        let line3 = "    void f() { var record = 1; boolean b = true; }";
        let at = line3.find("record").unwrap();
        assert_eq!(keyword_at(java, text, 3, at + 1), None);
        let at = line3.find("true").unwrap();
        assert_eq!(keyword_at(java, text, 3, at).as_deref(), Some("true"));
        let rust = Path::new("/p/src/lib.rs");
        assert_eq!(
            keyword_at(rust, "pub fn calc() {}\n", 0, 4).as_deref(),
            Some("fn")
        );
        assert_eq!(keyword_at(rust, "pub fn calc() {}\n", 0, 8), None);
        let python = Path::new("/p/a.py");
        assert_eq!(
            keyword_at(python, "def calc():\n    return None\n", 1, 6).as_deref(),
            Some("return")
        );
        assert_eq!(
            keyword_at(python, "def calc():\n    return None\n", 0, 5),
            None
        );
        // A file without a grammar: nothing to say.
        assert_eq!(keyword_at(Path::new("/p/notes.txt"), "class x", 0, 1), None);
    }
}
