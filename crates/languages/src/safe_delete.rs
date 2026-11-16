//! Safe Delete (`SPEC-0095` v12 behaviours 15 and 16): the declaration of a symbol and its usages
//! from the bounded search of Find Usages, the lines the declaration covers in its file, and the
//! whole-word mentions of a file's name in the project's other files.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Serialize;

use crate::find_usages::{UsageSource, find_usages_in};
use crate::insight::{Location, Symbol, symbols_in};

/// Most mentions of a file's name looked for (behaviour 16).
pub const MAX_MENTIONS: usize = 100;

/// Larger files are not read for mentions.
const MAX_MENTION_FILE: u64 = 1 << 20;

/// What Safe Delete would delete, and what still uses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SafeDeletePlan {
    /// The declared name.
    pub name: String,
    /// The declaration's symbol kind (`function`, `struct`, …).
    pub kind: String,
    /// The declaration's file.
    pub path: PathBuf,
    /// That file's text when the plan was made.
    #[serde(skip)]
    pub text: String,
    /// The deleted lines, 0-based, first and last.
    pub lines: (usize, usize),
    /// The deleted bytes of `text`.
    #[serde(skip)]
    pub range: Range<usize>,
    /// The usages, without the declaration itself.
    pub usages: Vec<Location>,
    /// Where the usages came from.
    pub source: UsageSource,
}

impl SafeDeletePlan {
    /// The file's text without the declaration.
    pub fn new_text(&self) -> String {
        let mut out = self.text.clone();
        out.replace_range(self.range.clone(), "");
        out
    }
}

/// What the status bar says without a declaration in the project (behaviour 15b).
pub fn no_declaration(name: &str) -> String {
    format!("Safe Delete found no declaration of `{name}` in the project")
}

/// What it says for a symbol the file's structure does not list.
pub fn not_in_structure(name: &str) -> String {
    format!(
        "Safe Delete works on the declarations File Structure shows; `{name}` is not one of them"
    )
}

/// What it says for a declaration that shares a line with other code.
pub fn shared_line(name: &str) -> String {
    format!(
        "Safe Delete deletes only declarations on lines of their own; `{name}` shares a line with other code"
    )
}

/// The symbol of `symbols` that declares `name` at 0-based `line` and `character`: the innermost
/// one named so at exactly that place, else the innermost one named so that contains it. A name
/// with its parameter list (Python and script functions, `SPEC-0079` v6) counts by the name.
pub fn pick_symbol<'a>(
    symbols: &'a [Symbol],
    name: &str,
    line: u64,
    character: u64,
) -> Option<&'a Symbol> {
    let named = |s: &&Symbol| {
        s.name == name
            || s.name
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('('))
    };
    let deepest = |it: Vec<&'a Symbol>| it.into_iter().max_by_key(|s| s.depth);
    deepest(
        symbols
            .iter()
            .filter(named)
            .filter(|s| s.line == line && s.character == character)
            .collect(),
    )
    .or_else(|| {
        deepest(
            symbols
                .iter()
                .filter(named)
                .filter(|s| s.contains(line, character))
                .collect(),
        )
    })
}

/// Whether a line directly above a declaration belongs to it: a comment, documentation,
/// attribute, annotation, decorator or template line (behaviour 15b).
fn leads(line: &str) -> bool {
    let t = line.trim_start();
    ["//", "/*", "*", "#[", "@", "template"]
        .iter()
        .any(|p| t.starts_with(p))
}

/// The 0-based first and last line Safe Delete removes for the symbol whose name starts at
/// `name_line`/`name_character` and whose body ends at `end_line`/`end_character` (characters):
/// its own lines, the leading lines of [`leads`] directly above, and one blank line after it (or,
/// at the end of the file, before it) where the other side is blank too. `None` when the
/// declaration shares its first or last line with other code.
pub fn declaration_lines(
    text: &str,
    name_line: usize,
    name_character: usize,
    end_line: usize,
    end_character: usize,
) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    // The lines a text ending in a newline really has.
    let real = if text.ends_with('\n') {
        lines.len() - 1
    } else {
        lines.len()
    };
    if name_line >= real || end_line >= real || end_line < name_line {
        return None;
    }
    let before: String = lines[name_line].chars().take(name_character).collect();
    if before.contains([';', '{', '}']) {
        return None;
    }
    let after: String = lines[end_line].chars().skip(end_character).collect();
    if !after
        .trim()
        .trim_start_matches([';', ','])
        .trim()
        .is_empty()
    {
        return None;
    }
    let mut first = name_line;
    while first > 0 && leads(lines[first - 1]) {
        first -= 1;
    }
    let mut last = end_line;
    let blank = |i: usize| lines[i].trim().is_empty();
    let blank_before = first == 0 || blank(first - 1);
    if blank_before && last + 1 < real && blank(last + 1) {
        last += 1;
    } else if last + 1 == real && first > 0 && blank(first - 1) {
        first -= 1;
    }
    Some((first, last))
}

/// The byte range of the 0-based lines `first` to `last` of `text`, with their line breaks.
pub fn line_bytes(text: &str, first: usize, last: usize) -> Range<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let start = starts.get(first).copied().unwrap_or(text.len());
    let end = starts.get(last + 1).copied().unwrap_or(text.len());
    start..end
}

/// Plans Safe Delete of the symbol `symbol` at 0-based `line` and `character` of `file` (content
/// `text`, else the disk): the declaration and usages from Find Usages' bounded search, the
/// declaration's lines from its file's structure. `open` holds the texts of files open in an
/// editor. The error is the sentence for the status bar.
#[allow(clippy::too_many_arguments)]
pub async fn plan_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    symbol: &str,
    open: &HashMap<PathBuf, String>,
    deadline: Duration,
    stop: Option<Arc<AtomicBool>>,
) -> Result<SafeDeletePlan, String> {
    let search = find_usages_in(root, file, text, line, character, symbol, deadline, stop)
        .await
        .map_err(|e| e.to_string())?;
    let name = search.target.name.clone();
    let Some(declaration) = search.target.declaration.clone() else {
        return Err(no_declaration(&name));
    };
    let same = |a: &Path, b: &Path| {
        a == b
            || a.canonicalize()
                .ok()
                .zip(b.canonicalize().ok())
                .is_some_and(|(x, y)| x == y)
    };
    let decl_text = match open.iter().find(|(p, _)| same(p, &declaration.path)) {
        Some((_, t)) => t.clone(),
        None if same(file, &declaration.path) && text.is_some() => {
            text.unwrap_or_default().to_owned()
        }
        None => std::fs::read_to_string(&declaration.path)
            .map_err(|e| format!("{}: {e}", declaration.path.display()))?,
    };
    let symbols = symbols_in(root, &declaration.path, Some(&decl_text), deadline)
        .await
        .map_err(|e| e.to_string())?;
    plan_from(
        &name,
        declaration,
        decl_text,
        &symbols,
        search.usages,
        search.source,
    )
}

/// The plan for the declaration at `declaration` in the file whose text is `text`, with its
/// file's `symbols` and the found `usages` (also for checks without a language server).
pub fn plan_from(
    name: &str,
    declaration: Location,
    text: String,
    symbols: &[Symbol],
    usages: Vec<Location>,
    source: UsageSource,
) -> Result<SafeDeletePlan, String> {
    let r = &declaration.range;
    let Some(symbol) = pick_symbol(symbols, name, r.start_line, r.start_character) else {
        return Err(not_in_structure(name));
    };
    let Some(lines) = declaration_lines(
        &text,
        symbol.line as usize,
        symbol.character as usize,
        symbol.end_line as usize,
        symbol.end_character as usize,
    ) else {
        return Err(shared_line(name));
    };
    Ok(SafeDeletePlan {
        name: name.to_owned(),
        kind: symbol.kind.clone(),
        path: declaration.path,
        range: line_bytes(&text, lines.0, lines.1),
        text,
        lines,
        usages,
        source,
    })
}

/// One whole-word occurrence of a file's name in another file (behaviour 16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mention {
    /// The file it is in.
    pub path: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based character.
    pub character: usize,
    /// The line, trimmed.
    pub text: String,
}

/// Names that say nothing about a file's content: Safe Delete looks for no mention of them.
pub fn generic_name(stem: &str) -> bool {
    matches!(
        stem,
        "mod" | "lib" | "main" | "index" | "__init__" | "__main__"
    )
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The whole-word occurrences of `word` in `files` (path and text), at most [`MAX_MENTIONS`].
pub fn mentions<'a>(
    word: &str,
    files: impl IntoIterator<Item = (&'a Path, &'a str)>,
) -> Vec<Mention> {
    let mut out = Vec::new();
    if word.is_empty() {
        return out;
    }
    for (path, text) in files {
        for (n, line) in text.lines().enumerate() {
            for (at, _) in line.match_indices(word) {
                let before = line[..at].chars().next_back();
                let after = line[at + word.len()..].chars().next();
                if before.is_some_and(word_char) || after.is_some_and(word_char) {
                    continue;
                }
                out.push(Mention {
                    path: path.to_path_buf(),
                    line: n,
                    character: line[..at].chars().count(),
                    text: line.trim().to_owned(),
                });
                if out.len() >= MAX_MENTIONS {
                    return out;
                }
            }
        }
    }
    out
}

/// The mentions of the name of `file` (absolute, below `root`) without its extension in the
/// project's own files other than itself; none for a generic name (behaviour 16).
pub fn file_mentions(root: &Path, file: &Path) -> Vec<Mention> {
    let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    if generic_name(stem) {
        return Vec::new();
    }
    let me = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let texts: Vec<(PathBuf, String)> = idecraft_core::OwnSources::new(root)
        .files()
        .into_iter()
        .map(|rel| root.join(rel))
        .filter(|p| p.canonicalize().map(|c| c != me).unwrap_or(true))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= MAX_MENTION_FILE))
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
        .collect();
    mentions(stem, texts.iter().map(|(p, t)| (p.as_path(), t.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insight::Range as LspRange;

    fn symbol(name: &str, line: u64, character: u64, end: (u64, u64), depth: usize) -> Symbol {
        Symbol {
            name: name.into(),
            kind: "function".into(),
            detail: None,
            line,
            character,
            depth,
            end_line: end.0,
            end_character: end.1,
            inherited_from: None,
            path: None,
        }
    }

    /// SPEC-0095 AK-20 (v12): a declaration's lines with the documentation and attributes
    /// above it and one blank line after it; at the end of the file the blank line before it.
    #[test]
    fn declaration_lines_take_comments_attributes_and_one_blank_line() {
        let text = "fn a() {}\n\n/// Doc.\n#[inline]\nfn b() {\n    1\n}\n\nfn c() {}\n";
        assert_eq!(declaration_lines(text, 4, 3, 6, 1), Some((2, 7)));
        let r = line_bytes(text, 2, 7);
        let mut out = text.to_owned();
        out.replace_range(r, "");
        assert_eq!(out, "fn a() {}\n\nfn c() {}\n");
        // The last declaration takes the blank line before it.
        assert_eq!(declaration_lines(text, 8, 3, 8, 9), Some((7, 8)));
        // Java: an annotation and a field with its semicolon.
        let java = "class A {\n    @Deprecated\n    int x;\n    int y;\n}\n";
        assert_eq!(declaration_lines(java, 2, 8, 2, 10), Some((1, 2)));
        // Out of the text.
        assert_eq!(declaration_lines(java, 9, 0, 9, 1), None);
    }

    /// SPEC-0095 AK-20 (v12): a declaration that shares a line is refused.
    #[test]
    fn a_shared_line_is_refused() {
        let text = "int a; int b;\nint c; // c\nvoid f() {} void g() {}\n";
        assert_eq!(declaration_lines(text, 0, 11, 0, 13), None, "code before");
        assert_eq!(declaration_lines(text, 0, 4, 0, 6), None, "code after");
        assert_eq!(declaration_lines(text, 2, 5, 2, 11), None);
        assert_eq!(declaration_lines("int a; int b;\n", 0, 4, 0, 5), None);
        assert_eq!(
            declaration_lines("struct S {\n    x: i32,\n}\n", 1, 4, 1, 10),
            Some((1, 1))
        );
    }

    /// SPEC-0095 AK-20 (v12): the innermost symbol of the name at the place, else the innermost
    /// one of the name containing it; a Python name with its parameters counts.
    #[test]
    fn the_innermost_symbol_of_the_name_is_chosen() {
        let symbols = vec![
            symbol("Shop", 0, 6, (9, 1), 0),
            symbol("total", 2, 8, (4, 5), 1),
            symbol("total", 6, 8, (8, 5), 1),
            symbol("price(x)", 10, 4, (11, 0), 0),
        ];
        assert_eq!(
            pick_symbol(&symbols, "total", 6, 8).map(|s| s.line),
            Some(6)
        );
        assert_eq!(
            pick_symbol(&symbols, "total", 3, 0).map(|s| s.line),
            Some(2)
        );
        assert_eq!(
            pick_symbol(&symbols, "price", 10, 4).map(|s| s.line),
            Some(10)
        );
        assert_eq!(pick_symbol(&symbols, "pri", 10, 4), None);
        assert_eq!(pick_symbol(&symbols, "missing", 2, 8), None);
        let plan = plan_from(
            "total",
            Location {
                path: "a.rs".into(),
                range: LspRange {
                    start_line: 2,
                    start_character: 8,
                    end_line: 2,
                    end_character: 13,
                },
            },
            "a\nb\n    fn total() {\n    }\n".into(),
            &[symbol("total", 2, 7, (3, 5), 0)],
            Vec::new(),
            UsageSource::LanguageServer,
        )
        .unwrap();
        assert_eq!(plan.lines, (2, 3));
        assert_eq!(plan.new_text(), "a\nb\n");
        assert_eq!(plan.kind, "function");
    }

    /// SPEC-0095 AK-22 (v12): mentions are whole words, generic names are not looked for.
    #[test]
    fn mentions_are_whole_words_in_other_files() {
        let a = Path::new("a.py");
        let b = Path::new("b.rs");
        let found = mentions(
            "pricing",
            [
                (
                    a,
                    "import pricing\nfrom pricing_old import x\nrepricing = pricing.total",
                ),
                (b, "mod pricing;"),
            ],
        );
        let at: Vec<(String, usize, usize)> = found
            .iter()
            .map(|m| (m.path.display().to_string(), m.line, m.character))
            .collect();
        assert_eq!(
            at,
            [
                ("a.py".into(), 0, 7),
                ("a.py".into(), 2, 12),
                ("b.rs".into(), 0, 4)
            ]
        );
        assert_eq!(found[0].text, "import pricing");
        assert!(generic_name("mod") && generic_name("__init__") && !generic_name("pricing"));
        assert!(mentions("", [(a, "x")]).is_empty());
    }
}
