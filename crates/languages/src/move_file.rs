//! Move a file to another folder (`SPEC-0114`) and keep what refers to it working: the language
//! server's edits for the move (`workspace/willRenameFiles`, e.g. Rust modules, Java packages,
//! TypeScript imports), and IDECraft's own rewriting of relative `#include "…"` in C/C++ and relative
//! import paths in JavaScript/TypeScript, which servers there do not update. The package line of a
//! moved Java or Kotlin file follows its folder where the server's edits leave it unchanged
//! (`SPEC-0275` v3 behaviour 26). Only the project's own sources are changed; edits of other files
//! are listed apart (`SPEC-0114` v3 behaviour 8).

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::insight::{Opened, Range};
use crate::lsp::{LspError, file_uri};
use crate::rename::{Edit, FileEdits, apply_files, file_edits};
use crate::rename_text::OutsideFile;

/// What moving a file changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MovePlan {
    /// The file now.
    pub from: PathBuf,
    /// The file after the move.
    pub to: PathBuf,
    /// Edits of the project's own files and of the moved file (at its old path), sorted by path.
    pub files: Vec<FileEdits>,
    /// Edits of files that are not the project's own (v3 behaviour 8), sorted by path: listed,
    /// not applied unless [`include_outside`] puts them into `files`.
    #[serde(skip)]
    pub outside: Vec<OutsideFile>,
    /// Whether the language server contributed edits.
    pub server: bool,
}

/// Lexically normalised `path`: `.` dropped, `..` removing the previous component.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `to` relative to the folder `dir`, with `/` separators (`../b/c.h`, `c.h`).
pub fn relative(dir: &Path, to: &Path) -> String {
    let (dir, to) = (normalize(dir), normalize(to));
    let a: Vec<_> = dir.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".into(); a.len() - common];
    parts.extend(
        b[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// Kinds of reference IDECraft rewrites itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// `#include "path"` in C and C++.
    Include,
    /// Relative module specifiers in JavaScript and TypeScript.
    Import,
}

/// The kind of reference a file of this name can contain.
pub fn ref_kind(path: &Path) -> Option<RefKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "inl" => Some(RefKind::Include),
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "vue" | "svelte" => {
            Some(RefKind::Import)
        }
        _ => None,
    }
}

const SCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "d.ts"];

/// The quoted specifiers of `text` with their byte spans (without quotes).
fn specifiers(text: &str, kind: RefKind) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let candidates: Vec<usize> = match kind {
            RefKind::Include => {
                let directive = trimmed.strip_prefix('#').map(str::trim_start);
                match directive {
                    Some(rest) if rest.starts_with("include") => {
                        line.find('"').into_iter().collect()
                    }
                    _ => Vec::new(),
                }
            }
            RefKind::Import => {
                let mentions = ["from", "import", "require(", "import("];
                if mentions.iter().any(|m| line.contains(m)) {
                    line.char_indices()
                        .filter(|(_, c)| *c == '\'' || *c == '"')
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    Vec::new()
                }
            }
        };
        let mut skip_until = 0;
        for open in candidates {
            if open < skip_until {
                continue;
            }
            let quote = line.as_bytes()[open];
            let Some(len) = line[open + 1..].find(quote as char) else {
                continue;
            };
            let spec = &line[open + 1..open + 1 + len];
            skip_until = open + len + 2;
            let relative = spec.starts_with("./") || spec.starts_with("../");
            if kind == RefKind::Include || relative {
                out.push((offset + open + 1..offset + open + 1 + len, spec.to_owned()));
            }
        }
        offset += line.len();
    }
    out
}

/// Whether `spec`, written in a file in `dir`, names `target` (for imports also without its
/// extension or as the folder of an `index` file).
fn names(dir: &Path, spec: &str, target: &Path, kind: RefKind) -> bool {
    let base = normalize(&dir.join(spec));
    if base == normalize(target) {
        return true;
    }
    if kind == RefKind::Import {
        let target = normalize(target);
        return SCRIPT_EXTENSIONS.iter().any(|ext| {
            let file = PathBuf::from(format!("{}.{ext}", base.display()));
            file == target || base.join(format!("index.{ext}")) == target
        });
    }
    false
}

/// The specifier for `target` written in a file in `dir`, in the style of `old` (imports keep or
/// drop the extension as `old` did and start with `./` or `../`).
fn respell(dir: &Path, target: &Path, old: &str, kind: RefKind) -> String {
    let mut rel = relative(dir, target);
    if kind == RefKind::Import {
        let had_extension = Path::new(old)
            .extension()
            .is_some_and(|e| SCRIPT_EXTENSIONS.contains(&e.to_string_lossy().as_ref()));
        if !had_extension {
            for ext in SCRIPT_EXTENSIONS {
                if let Some(stem) = rel.strip_suffix(&format!(".{ext}")) {
                    rel = stem.to_owned();
                    break;
                }
            }
        }
        if !rel.starts_with("../") {
            rel = format!("./{rel}");
        }
    }
    rel
}

/// IDECraft's own edits of `text` (the file at `path`) for moving `from` to `to`: references to the
/// moved file, and, in the moved file itself, its relative references, which now start elsewhere.
pub fn own_edits(
    text: &str,
    path: &Path,
    from: &Path,
    to: &Path,
) -> Vec<(std::ops::Range<usize>, String)> {
    let Some(kind) = ref_kind(path) else {
        return Vec::new();
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    let moved = normalize(path) == normalize(from);
    let new_dir = to.parent().unwrap_or(Path::new(""));
    let mut out = Vec::new();
    for (span, spec) in specifiers(text, kind) {
        if names(dir, &spec, from, kind) {
            let from_dir = if moved { new_dir } else { dir };
            out.push((span, respell(from_dir, to, &spec, kind)));
        } else if moved {
            let target = normalize(&dir.join(&spec));
            let exists = target.exists()
                || (kind == RefKind::Import
                    && SCRIPT_EXTENSIONS
                        .iter()
                        .any(|e| PathBuf::from(format!("{}.{e}", target.display())).exists()));
            if exists {
                let new = match kind {
                    RefKind::Include => relative(new_dir, &target),
                    RefKind::Import => {
                        let rel = relative(new_dir, &target);
                        if rel.starts_with("../") {
                            rel
                        } else {
                            format!("./{rel}")
                        }
                    }
                };
                if new != spec {
                    out.push((span, new));
                }
            }
        }
    }
    out
}

fn to_edit(text: &str, span: std::ops::Range<usize>, new_text: String) -> Edit {
    let pos = |at: usize| {
        let before = &text[..at];
        let line = before.matches('\n').count() as u64;
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        (line, text[start..at].chars().count() as u64)
    };
    let (sl, sc) = pos(span.start);
    let (el, ec) = pos(span.end);
    Edit {
        range: Range {
            start_line: sl,
            start_character: sc,
            end_line: el,
            end_character: ec,
        },
        old_text: text[span].to_owned(),
        new_text,
    }
}

/// The files that may declare the Rust modules of the folder `dir`: its crate roots and `mod.rs`,
/// and the sibling `dir.rs` (non-`mod.rs` layout).
fn rust_module_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = ["main.rs", "lib.rs", "mod.rs"]
        .iter()
        .map(|n| dir.join(n))
        .collect();
    if let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) {
        out.push(parent.join(format!("{}.rs", name.to_string_lossy())));
    }
    out.into_iter().filter(|p| p.is_file()).collect()
}

/// The folder a `#[path]` in `declaring` is relative to: its own folder for a crate root or
/// `mod.rs`, else the folder named after it.
fn rust_path_base(declaring: &Path) -> PathBuf {
    let dir = declaring.parent().unwrap_or(Path::new(""));
    let name = declaring
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    match name.as_deref() {
        Some("main.rs" | "lib.rs" | "mod.rs") => dir.to_path_buf(),
        _ => dir.join(declaring.file_stem().unwrap_or_default()),
    }
}

/// The edit of `text` (the file declaring the module of `from`) that keeps `mod name;` pointing at
/// the module after it moved to `to` (`SPEC-0114` v2 behaviour 7, D27): a `#[path = "…"]` line before
/// the declaration, replaced when there is one, removed when the file is back where the module
/// tree expects it. `None` when `text` declares no such module.
pub fn rust_mod_edit(
    text: &str,
    declaring: &Path,
    from: &Path,
    to: &Path,
) -> Option<(std::ops::Range<usize>, String)> {
    let name = from.file_stem()?.to_string_lossy().into_owned();
    if from.file_name()? == "mod.rs" {
        return None;
    }
    let mut offset = 0;
    let mut previous: Option<(usize, &str)> = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        let rest = t
            .strip_prefix("pub(crate) ")
            .or_else(|| t.strip_prefix("pub(super) "))
            .or_else(|| t.strip_prefix("pub "))
            .unwrap_or(t);
        let declares = rest
            .strip_prefix("mod ")
            .map(|r| r.trim_end_matches(';').trim() == name && r.trim_end().ends_with(';'))
            .unwrap_or(false);
        let base = rust_path_base(declaring);
        // The `#[path]` before the declaration and whether it names `from`.
        let pathed = previous.and_then(|(_, prev)| {
            let v = prev
                .trim()
                .strip_prefix("#[path")?
                .split('"')
                .nth(1)?
                .to_owned();
            Some(normalize(&base.join(v)) == normalize(from))
        });
        let natural_home = [format!("{name}.rs"), format!("{name}/mod.rs")]
            .iter()
            .any(|n| normalize(&base.join(n)) == normalize(from));
        if declares && (pathed == Some(true) || (pathed.is_none() && natural_home)) {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let rel = relative(&base, to);
            let natural = rel == format!("{name}.rs") || rel == format!("{name}/mod.rs");
            let attr = (!natural).then(|| format!("{indent}#[path = \"{rel}\"]\n"));
            // An existing `#[path]` right before the declaration is replaced.
            if let Some((start, prev)) = previous
                && prev.trim_start().starts_with("#[path")
            {
                return Some((start..start + prev.len(), attr.unwrap_or_default()));
            }
            return attr.map(|a| (offset..offset, a));
        }
        previous = Some((offset, line));
        offset += line.len();
    }
    None
}

/// Edits for the Rust module declaration of `from` moved to `to` (see [`rust_mod_edit`]), in the
/// files declaring the modules of its folder.
fn rust_module_edits(from: &Path, to: &Path) -> Vec<(PathBuf, Edit)> {
    if from.extension().is_none_or(|e| e != "rs") || from.parent() == to.parent() {
        return Vec::new();
    }
    // The folder's own module files, then those above it (a module moved there earlier is
    // declared with a `#[path]` higher up).
    let mut out = Vec::new();
    let candidates: Vec<PathBuf> = from
        .ancestors()
        .skip(1)
        .take(8)
        .flat_map(rust_module_files)
        .collect();
    for declaring in candidates {
        if normalize(&declaring) == normalize(from) {
            continue;
        }
        let Ok(text) = crate::shown::read(&declaring) else {
            continue;
        };
        if let Some((span, new)) = rust_mod_edit(&text, &declaring, from, to) {
            out.push((declaring, to_edit(&text, span, new)));
        }
    }
    out
}

/// Whether `path` is a Java or Kotlin source with a package line.
pub fn has_package_line(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "java" | "kt"))
}

/// The package declaration of a Java or Kotlin `text`: the byte span of its line (without the line
/// break) and the package name; `None` without one. Leading blank lines, comments and Kotlin
/// file annotations are skipped; the first other line decides.
pub fn package_declaration(text: &str) -> Option<(std::ops::Range<usize>, String)> {
    let mut offset = 0;
    let mut in_comment = false;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        let start = offset;
        offset += line.len();
        if in_comment {
            in_comment = !trimmed.contains("*/");
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('@') {
            continue;
        }
        if trimmed.starts_with("/*") {
            in_comment = !trimmed.contains("*/");
            continue;
        }
        let rest = trimmed.strip_prefix("package")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let name = rest.trim().trim_end_matches(';').trim().to_owned();
        return Some((start..start + body.len(), name));
    }
    None
}

/// The edit that gives `text` the package `new` (`None`: the default package), as a byte span and
/// its new text; `None` when the line is right already. A Java line ends with `;`, a Kotlin line
/// (`kotlin`) does not. A missing line goes before the first line of code, after leading comments.
pub fn package_edit(
    text: &str,
    new: Option<&str>,
    kotlin: bool,
) -> Option<(std::ops::Range<usize>, String)> {
    let line = |p: &str| {
        if kotlin {
            format!("package {p}")
        } else {
            format!("package {p};")
        }
    };
    match (package_declaration(text), new) {
        (Some((_, old)), Some(new)) if old == new => None,
        (Some((span, _)), Some(new)) => Some((span, line(new))),
        (Some((span, _)), None) => {
            // The line, its break and one blank line after it.
            let mut end = span.end;
            for _ in 0..2 {
                if text[end..].starts_with("\r\n") {
                    end += 2;
                } else if text[end..].starts_with('\n') {
                    end += 1;
                } else {
                    break;
                }
            }
            Some((span.start..end, String::new()))
        }
        (None, None) => None,
        (None, Some(new)) => {
            let at = code_start(text);
            Some((at..at, format!("{}\n\n", line(new))))
        }
    }
}

/// Byte offset of the first line that is neither blank nor a comment.
fn code_start(text: &str) -> usize {
    let mut offset = 0;
    let mut in_comment = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if in_comment || trimmed.starts_with("/*") {
            in_comment = !trimmed.contains("*/");
        } else if !(trimmed.is_empty() || trimmed.starts_with("//")) {
            return offset;
        }
        offset += line.len();
    }
    offset
}

/// IDECraft's own edit of a Java or Kotlin file moved from folder `old_dir` to `new_dir` of the
/// workspace at `root`: its package line to the new folder's package. Only when the line matched
/// the old folder (so an unusual source layout is never "corrected").
pub fn package_line_edit(
    root: &Path,
    file: &Path,
    text: &str,
    old_dir: &Path,
    new_dir: &Path,
) -> Option<(std::ops::Range<usize>, String)> {
    if !has_package_line(file) {
        return None;
    }
    let old = idecraft_core::file_ops::package_of(root, old_dir);
    let new = idecraft_core::file_ops::package_of(root, new_dir);
    let declared = package_declaration(text).map(|(_, name)| name);
    if declared != old || (old.is_none() && new.is_none()) {
        return None;
    }
    let kotlin = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("kt"));
    package_edit(text, new.as_deref(), kotlin)
}

/// Gives a Java or Kotlin file that was copied or moved plainly from folder `old_dir` the package
/// line of its folder now (`SPEC-0275` v3 behaviour 26); whether it changed the file.
pub fn update_package_line(root: &Path, file: &Path, old_dir: &Path) -> Result<bool, String> {
    if !has_package_line(file) {
        return Ok(false);
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let new_dir = file.parent().unwrap_or(&root);
    let Some((span, new)) = package_line_edit(&root, file, &text, old_dir, new_dir) else {
        return Ok(false);
    };
    let mut out = text;
    out.replace_range(span, &new);
    std::fs::write(file, out).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(true)
}

/// Where `file` goes when moved into `target_dir` (workspace-relative or absolute), refusing a
/// target outside the workspace or one that exists.
pub fn destination(root: &Path, file: &Path, target_dir: &str) -> Result<PathBuf, String> {
    let dir = normalize(&root.join(target_dir.trim().trim_end_matches('/')));
    if !dir.starts_with(normalize(root)) {
        return Err(format!("{target_dir} lies outside the workspace"));
    }
    let name = file.file_name().ok_or("not a file")?;
    let to = dir.join(name);
    if normalize(&to) == normalize(file) {
        return Err("the file is already there".into());
    }
    if to.exists() {
        return Err(format!("{} exists", to.display()));
    }
    Ok(to)
}

/// Plans moving `file` into `target_dir` of the workspace at `root`; nothing is written. Within a
/// [`crate::shown::scope`] the files' shown texts are read in place of the disk (`SPEC-0220` v4
/// behaviour 16).
pub async fn plan_move(
    root: &Path,
    file: &Path,
    target_dir: &str,
    wait: Duration,
) -> Result<MovePlan, String> {
    let from = file
        .canonicalize()
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let to = destination(&root, &from, target_dir)?;
    let mut files: std::collections::BTreeMap<PathBuf, Vec<Edit>> = Default::default();
    let mut server = false;
    if let Ok(found) = server_edits(&root, &from, &to, wait).await {
        server = !found.is_empty();
        for f in found {
            let path = f.path.canonicalize().unwrap_or(f.path);
            files.entry(path).or_default().extend(f.edits);
        }
    }
    // The moved file's package line, unless the server changed that line (behaviour 26).
    if has_package_line(&from)
        && let Ok(text) = crate::shown::read(&from)
        && let (Some(old_dir), Some(new_dir)) = (from.parent(), to.parent())
        && let Some((span, new)) = package_line_edit(&root, &from, &text, old_dir, new_dir)
    {
        let edit = to_edit(&text, span, new);
        let edits = files.entry(from.clone()).or_default();
        let line = edit.range.start_line;
        let touched = edits
            .iter()
            .any(|e| e.range.start_line <= line && line <= e.range.end_line);
        if !touched {
            edits.push(edit);
        }
    }
    // Rust: rust-analyzer updates no `mod` declaration for a move into another folder.
    for (path, edit) in rust_module_edits(&from, &to) {
        let edits = files.entry(path).or_default();
        let line = edit.range.start_line;
        if !edits
            .iter()
            .any(|e| e.range.start_line <= line && line <= e.range.end_line)
        {
            edits.push(edit);
        }
    }
    let listed = idecraft_core::Workspace::open(&root)
        .and_then(|w| w.files())
        .map_err(|e| e.to_string())?;
    for rel in listed {
        let path = root.join(&rel);
        if ref_kind(&path).is_none() {
            continue;
        }
        // The text the window shows for an open file with unsaved changes (SPEC-0220 v4
        // behaviour 13).
        let Ok(text) = crate::shown::read(&path) else {
            continue;
        };
        let own = own_edits(&text, &path, &from, &to);
        if !own.is_empty() {
            let edits = files.entry(path).or_default();
            for (span, new) in own {
                let e = to_edit(&text, span, new);
                // The server may have rewritten the same reference already.
                let overlaps = |x: &Edit| {
                    let start = (x.range.start_line, x.range.start_character);
                    let end = (x.range.end_line, x.range.end_character);
                    start < (e.range.end_line, e.range.end_character)
                        && (e.range.start_line, e.range.start_character) < end
                };
                if !edits.iter().any(overlaps) {
                    edits.push(e);
                }
            }
        }
    }
    // Only the project's own files are changed; the others are listed apart (v3 behaviour 8).
    // The moved file is taken as it is: the user chose it.
    let own = idecraft_core::OwnSources::new(&root);
    let mut outside = Vec::new();
    let mut kept = Vec::new();
    for (path, mut edits) in files {
        edits.sort_by_key(|e| (e.range.start_line, e.range.start_character));
        match Some(&path)
            .filter(|p| **p != from)
            .and_then(|p| own.not_own(p))
        {
            Some(why) => outside.push(OutsideFile {
                path,
                edits,
                why,
                reason: why.reason().to_owned(),
            }),
            None => kept.push(FileEdits { path, edits }),
        }
    }
    Ok(MovePlan {
        from,
        to,
        files: kept,
        outside,
        server,
    })
}

/// Puts the entries `indices` of `plan.outside` into the edits to apply: files that are not the
/// project's own, changed on purpose (v3 behaviour 8). `Err` names an index that does not exist.
pub fn include_outside(plan: &mut MovePlan, indices: &[usize]) -> Result<(), String> {
    if let Some(i) = indices.iter().find(|i| **i >= plan.outside.len()) {
        return Err(format!(
            "include_outside: no entry {i} in outside_project ({} entries)",
            plan.outside.len()
        ));
    }
    for (i, f) in plan.outside.iter().enumerate() {
        if indices.contains(&i) && !plan.files.iter().any(|p| p.path == f.path) {
            plan.files.push(FileEdits {
                path: f.path.clone(),
                edits: f.edits.clone(),
            });
        }
    }
    plan.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(())
}

/// The language server's edits for the move, when it supports `workspace/willRenameFiles`.
async fn server_edits(
    root: &Path,
    from: &Path,
    to: &Path,
    wait: Duration,
) -> Result<Vec<FileEdits>, LspError> {
    let mut o = Opened::open_text(root, from, None, wait).await?;
    let result = async {
        if o.capabilities["workspace"]["fileOperations"]["willRename"].is_null() {
            return Ok(Vec::new());
        }
        let params = json!({"files": [{"oldUri": file_uri(from), "newUri": file_uri(to)}]});
        let answer = o.ask("workspace/willRenameFiles", params).await?;
        if answer.is_null() {
            return Ok(Vec::new());
        }
        file_edits(&mut o, &[answer])
    }
    .await;
    o.close().await;
    result
}

/// Applies the edits, then moves the file (creating the target folder).
pub fn apply_move(plan: &MovePlan) -> Result<(), String> {
    apply_files(&plan.files)?;
    if let Some(dir) = plan.to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::rename(&plan.from, &plan.to)
        .map_err(|e| format!("{} → {}: {e}", plan.from.display(), plan.to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0114 v2 behaviour 7 (hand test 41, D27): `mod geld;` follows `geld.rs` into a folder.
    #[test]
    fn rust_modules_follow_the_moved_file() {
        let main = Path::new("/p/src/main.rs");
        let (from, to) = (
            Path::new("/p/src/geld.rs"),
            Path::new("/p/src/finanz/geld.rs"),
        );
        let text = "mod geld;\n\nfn main() {}\n";
        let (span, new) = rust_mod_edit(text, main, from, to).unwrap();
        assert_eq!(
            (span, new.as_str()),
            (0..0, "#[path = \"finanz/geld.rs\"]\n")
        );
        // A second move replaces the attribute; back home it goes.
        let moved = "pub mod geld;\n";
        let text = format!("#[path = \"finanz/geld.rs\"]\n{moved}");
        let back = rust_mod_edit(
            &text,
            main,
            Path::new("/p/src/finanz/geld.rs"),
            Path::new("/p/src/geld.rs"),
        );
        assert_eq!(back, Some((0..27, String::new())));
        let other = rust_mod_edit(&text, main, to, Path::new("/p/src/x/geld.rs")).unwrap();
        assert_eq!(other, (0..27, "#[path = \"x/geld.rs\"]\n".to_owned()));
        // In a non-`mod.rs` module file the path is relative to its folder.
        let konto = Path::new("/p/src/bank.rs");
        let (_, new) = rust_mod_edit(
            "    mod geld;\n",
            konto,
            Path::new("/p/src/bank/geld.rs"),
            Path::new("/p/src/finanz/geld.rs"),
        )
        .unwrap();
        assert_eq!(new, "    #[path = \"../finanz/geld.rs\"]\n");
        // Other modules, `mod geld { … }` and a `mod geld;` of another folder are left alone.
        assert_eq!(
            rust_mod_edit("mod gelder;\nmod geld {}\n", main, from, to),
            None
        );
        let elsewhere = Path::new("/p/src/x/geld.rs");
        assert_eq!(rust_mod_edit("mod geld;\n", main, elsewhere, to), None);
    }

    fn apply(text: &str, edits: Vec<(std::ops::Range<usize>, String)>) -> String {
        let mut out = text.to_owned();
        for (span, new) in edits.into_iter().rev() {
            out.replace_range(span, &new);
        }
        out
    }

    /// SPEC-0275 v3 behaviour 26 (AK-15).
    #[test]
    fn the_package_line_follows_the_folder() {
        let java =
            "/* Licence */\npackage com.acme;\n\nimport java.util.List;\n\npublic class A {}\n";
        assert_eq!(
            package_declaration(java).map(|(_, n)| n),
            Some("com.acme".into())
        );
        let out = apply(
            java,
            package_edit(java, Some("com.acme.util"), false)
                .into_iter()
                .collect(),
        );
        assert!(out.contains("\npackage com.acme.util;\n\nimport"), "{out}");
        assert_eq!(
            package_edit(java, Some("com.acme"), false),
            None,
            "right already"
        );
        let out = apply(java, package_edit(java, None, false).into_iter().collect());
        assert_eq!(
            out,
            "/* Licence */\nimport java.util.List;\n\npublic class A {}\n"
        );
        let bare = "// header\n\nclass B\n";
        let out = apply(
            bare,
            package_edit(bare, Some("org.x"), true)
                .into_iter()
                .collect(),
        );
        assert_eq!(out, "// header\n\npackage org.x\n\nclass B\n");
        let kotlin = "@file:JvmName(\"Util\")\npackage org.x\n\nfun f() = 1\n";
        let out = apply(
            kotlin,
            package_edit(kotlin, Some("org.y"), true)
                .into_iter()
                .collect(),
        );
        assert_eq!(
            out,
            "@file:JvmName(\"Util\")\npackage org.y\n\nfun f() = 1\n"
        );
        assert_eq!(package_declaration("packages = 1\n"), None);
        // Only when the line matched the old folder.
        let root = Path::new("/w");
        let old = Path::new("/w/src/main/java/com/acme");
        let new = Path::new("/w/src/main/java/com/acme/util");
        let file = Path::new("/w/src/main/java/com/acme/util/A.java");
        assert!(package_line_edit(root, file, java, old, new).is_some());
        let odd = "package elsewhere;\nclass A {}\n";
        assert_eq!(
            package_line_edit(root, file, odd, old, new),
            None,
            "layout differs"
        );
        let flat = Path::new("/w/app");
        assert_eq!(
            package_line_edit(
                root,
                Path::new("/w/app2/A.java"),
                "class A {}\n",
                flat,
                Path::new("/w/app2")
            ),
            None,
            "no package before or after"
        );
        assert_eq!(
            package_line_edit(root, Path::new("/w/a.txt"), java, old, new),
            None
        );
    }

    #[test]
    fn paths_normalise_and_relate() {
        assert_eq!(
            normalize(Path::new("/w/a/./b/../c.h")),
            Path::new("/w/a/c.h")
        );
        assert_eq!(
            relative(Path::new("/w/src"), Path::new("/w/include/x.h")),
            "../include/x.h"
        );
        assert_eq!(relative(Path::new("/w"), Path::new("/w/x.h")), "x.h");
        let root = Path::new("/w");
        assert_eq!(
            destination(root, Path::new("/w/a.h"), "include/").unwrap(),
            Path::new("/w/include/a.h")
        );
        assert!(destination(root, Path::new("/w/a.h"), "../elsewhere").is_err());
        assert!(destination(root, Path::new("/w/a.h"), ".").is_err());
    }

    #[test]
    fn includes_follow_the_moved_header() {
        let from = Path::new("/w/src/util.h");
        let to = Path::new("/w/include/util.h");
        let main = "#include <stdio.h>\n#include \"util.h\"\n#  include \"other.h\"\n";
        let out = apply(main, own_edits(main, Path::new("/w/src/main.c"), from, to));
        assert_eq!(
            out,
            "#include <stdio.h>\n#include \"../include/util.h\"\n#  include \"other.h\"\n"
        );
        let nested = "#include \"../src/util.h\"\n";
        let out = apply(nested, own_edits(nested, Path::new("/w/lib/x.c"), from, to));
        assert_eq!(out, "#include \"../include/util.h\"\n");
    }

    #[test]
    fn imports_keep_their_style() {
        let from = Path::new("/w/src/lib/math.ts");
        let to = Path::new("/w/src/util/math.ts");
        let app = "import { add } from './lib/math';\nconst m = require(\"./lib/math.ts\");\nimport x from 'react';\n";
        let out = apply(app, own_edits(app, Path::new("/w/src/app.ts"), from, to));
        assert_eq!(
            out,
            "import { add } from './util/math';\nconst m = require(\"./util/math.ts\");\nimport x from 'react';\n"
        );
        let sibling = "export * from '../lib/math'\n";
        let out = apply(
            sibling,
            own_edits(sibling, Path::new("/w/src/util/index.ts"), from, to),
        );
        assert_eq!(out, "export * from './math'\n");
        assert_eq!(ref_kind(Path::new("a.rs")), None);
    }
}
