//! Python completion on top of pyright (`SPEC-0072` v9 behaviours 34–42): signature and type from
//! the resolved documentation, call parentheses, IDECraft's own postfix and live templates, and
//! imports from the standard library that the server does not offer.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::insight::{CompletionEdit, CompletionItem, Range};

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The first Python code block of a Markdown text, without its fences.
fn first_code_block(doc: &str) -> Option<&str> {
    let start = doc.find("```")?;
    let after = &doc[start + 3..];
    let body_start = after.find('\n')? + 1;
    let body = &after[body_start..];
    let end = body.find("```").unwrap_or(body.len());
    Some(&body[..end])
}

/// The first declaration in a pyright documentation (behaviour 34): the first statement of its
/// first code block on one line, without a trailing `: ...` (an overload's body):
/// `def join(\n    a: StrPath,\n    *paths: StrPath\n) -> str: ...` → `def join(a: StrPath,
/// *paths: StrPath) -> str`; `qty: int` stays. `None` for an import line or prose only.
pub fn declaration(doc: &str) -> Option<String> {
    let block = first_code_block(doc)?;
    let mut lines = Vec::new();
    let mut depth = 0i32;
    for line in block.lines() {
        let t = line.trim();
        if t.is_empty() || (t.starts_with('@') && depth == 0) {
            if lines.is_empty() {
                continue;
            }
            break;
        }
        lines.push(t.to_owned());
        depth += t.matches(['(', '[']).count() as i32 - t.matches([')', ']']).count() as i32;
        if depth <= 0 {
            break;
        }
    }
    let mut text = lines.join(" ");
    for (from, to) in [("( ", "("), (" )", ")"), (", )", ")"), (",)", ")")] {
        text = text.replace(from, to);
    }
    let text = text.trim_end_matches(" ...").trim_end_matches(": ...");
    let text = text.strip_suffix(':').unwrap_or(text).trim().to_owned();
    (!text.is_empty() && !text.starts_with("from ") && !text.starts_with("import ")).then_some(text)
}

/// The parameters of a Python signature `(…)` that a caller passes: without `self`, `cls`, `/`
/// and `*` (behaviour 37).
pub fn passed_parameters(signature: &str) -> usize {
    let inner = signature
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut current = String::new();
    for c in inner.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    parts.push(current);
    parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty() && !matches!(*p, "/" | "*"))
        .filter(|p| {
            let name = p.split([':', '=']).next().unwrap_or_default().trim();
            !matches!(name, "self" | "cls")
        })
        .count()
}

/// The documentation of a Python item made easier to read beside the list (behaviour 42): of
/// several overloads in the code block only the first stays, followed by how many more there
/// are; the text after the block stays as it is.
pub fn tidy_documentation(doc: &str) -> String {
    let Some(open) = doc.find("```") else {
        return doc.to_owned();
    };
    let Some(nl) = doc[open..].find('\n') else {
        return doc.to_owned();
    };
    let body_start = open + nl + 1;
    let Some(close) = doc[body_start..].find("```") else {
        return doc.to_owned();
    };
    let body_end = body_start + close;
    let body = &doc[body_start..body_end];
    let is_decl =
        |l: &str| l.starts_with("def ") || l.starts_with("async def ") || l.starts_with("class ");
    let count = body.split('\n').filter(|l| is_decl(l)).count();
    if count < 2 {
        return doc.to_owned();
    }
    // The first declaration with its lines (and decorators before it), up to the next one.
    let mut first = Vec::new();
    let mut seen = 0;
    for line in body.split('\n') {
        if is_decl(line) {
            seen += 1;
            if seen == 2 {
                break;
            }
        }
        if seen == 1 || !line.trim().is_empty() {
            first.push(line);
        }
    }
    while first.last().is_some_and(|l| l.trim().is_empty()) {
        first.pop();
    }
    let more = count - 1;
    let note = if more == 1 {
        "(1 more overload)".to_owned()
    } else {
        format!("({more} more overloads)")
    };
    let rest = &doc[body_end + 3..];
    format!(
        "{}{}\n```\n{note}\n{rest}",
        &doc[..body_start],
        first.join("\n")
    )
}

/// The text a snippet shows: placeholders with their default text, stops left out, tabs as four
/// spaces.
fn snippet_preview(snippet: &str) -> String {
    let chars: Vec<char> = snippet.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '$' && i + 1 < chars.len() {
            if chars[i + 1] == '{' {
                // `${n:text}`: the text stays.
                let mut j = i + 2;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ':' {
                    j += 1;
                }
                i = j;
                continue;
            }
            if chars[i + 1].is_ascii_digit() {
                i += 2;
                continue;
            }
        }
        match c {
            '}' => {}
            '\t' => out.push_str("    "),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// Gives Python functions and methods their parentheses (behaviour 37): the inserted text becomes
/// `name($0)` unless the text after the name already starts with `(`, the name follows `def`,
/// `class` or `import`, or the line is an import.
pub fn with_call_parentheses(items: &mut [CompletionItem], line: &str, character: usize) {
    let chars: Vec<char> = line.chars().collect();
    let mut end = character.min(chars.len());
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    let after: String = chars[end..].iter().collect();
    let mut start = character.min(chars.len());
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let before: String = chars[..start].iter().collect();
    let t = before.trim_start();
    if after.trim_start().starts_with('(')
        || t.starts_with("import ")
        || t.starts_with("from ")
        || t.ends_with("def ")
        || t.ends_with("class ")
        || t.starts_with('@')
    {
        return;
    }
    for item in items {
        if !matches!(item.kind.as_deref(), Some("function" | "method")) || item.snippet {
            continue;
        }
        if item.insert.contains('(') || !item.insert.chars().all(is_word) {
            continue;
        }
        item.insert = format!("{}($0)", item.insert);
        item.snippet = true;
    }
}

/// One postfix template: name, description, snippet with `{x}` for the expression.
const POSTFIX: &[(&str, &str, &str)] = &[
    ("if", "if expr:", "if {x}:\n\t$0"),
    ("ifn", "if expr is None:", "if {x} is None:\n\t$0"),
    ("ifnn", "if expr is not None:", "if {x} is not None:\n\t$0"),
    ("not", "not expr", "not {x}$0"),
    ("var", "name = expr", "${1:{name}} = {x}$0"),
    (
        "for",
        "for item in expr:",
        "for ${1:{element}} in {x}:\n\t$0",
    ),
    (
        "fori",
        "for i, item in enumerate(expr):",
        "for ${1:i}, ${2:{element}} in enumerate({x}):\n\t$0",
    ),
    ("while", "while expr:", "while {x}:\n\t$0"),
    (
        "with",
        "with expr as name:",
        "with {x} as ${1:{name}}:\n\t$0",
    ),
    ("print", "print(expr)", "print({x})$0"),
    ("return", "return expr", "return {x}$0"),
    ("yield", "yield expr", "yield {x}$0"),
    ("raise", "raise expr", "raise {x}$0"),
    ("par", "(expr)", "({x})$0"),
    ("len", "len(expr)", "len({x})$0"),
    ("str", "str(expr)", "str({x})$0"),
    ("list", "list(expr)", "list({x})$0"),
    ("set", "set(expr)", "set({x})$0"),
    ("tuple", "tuple(expr)", "tuple({x})$0"),
    ("dict", "dict(expr)", "dict({x})$0"),
    (
        "isinstance",
        "isinstance(expr, type)",
        "isinstance({x}, ${1:object})$0",
    ),
    (
        "main",
        "if __name__ == \"__main__\": expr",
        "if __name__ == \"__main__\":\n\t{x}$0",
    ),
];

/// Escapes text for an LSP snippet.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('$', "\\$")
        .replace('}', "\\}")
}

/// IDECraft's Python postfix templates for the name typed at char `character` of line `line_no`
/// in `line` (behaviour 38): those whose name starts with the typed name, each replacing
/// expression and name. Empty when the name does not follow `expression.`, after `import`/`from`
/// and on a number.
pub fn postfix_items(line: &str, line_no: u64, character: usize) -> Vec<CompletionItem> {
    let chars: Vec<char> = line.chars().collect();
    let character = character.min(chars.len());
    let mut start = character;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    if start == 0 || chars[start - 1] != '.' {
        return Vec::new();
    }
    let t = line.trim_start();
    if t.starts_with("import ") || t.starts_with("from ") {
        return Vec::new();
    }
    let Some(expr_start) = crate::script_completion::expression_start(line, start - 1) else {
        return Vec::new();
    };
    let expression: String = chars[expr_start..start - 1].iter().collect();
    if expression.is_empty() || expression == "self" {
        return Vec::new();
    }
    let typed: String = chars[start..character]
        .iter()
        .collect::<String>()
        .to_lowercase();
    let name = crate::names::suggest(crate::names::NameKind::Value, Some(&expression), None)
        .into_iter()
        .next()
        .unwrap_or_else(|| "value".into());
    let element = crate::names::suggest(crate::names::NameKind::Element, Some(&expression), None)
        .into_iter()
        .next()
        .unwrap_or_else(|| "item".into());
    POSTFIX
        .iter()
        .filter(|(n, _, _)| n.starts_with(&typed))
        .map(|(n, description, snippet)| CompletionItem {
            label: (*n).to_owned(),
            kind: Some("snippet".into()),
            detail: Some((*description).to_owned()),
            description: Some((*description).to_owned()),
            insert: snippet
                .replace("{x}", &escape(&expression))
                .replace("{name}", &escape(&name))
                .replace("{element}", &escape(&element)),
            snippet: true,
            // After the server's own items.
            sort_text: Some(format!("~postfix{n}")),
            edit_range: Some(Range {
                start_line: line_no,
                start_character: expr_start as u64,
                end_line: line_no,
                end_character: character as u64,
            }),
            ..Default::default()
        })
        .collect()
}

/// One live template: name, description, snippet, only at the top level of a module.
const LIVE: &[(&str, &str, &str, bool)] = &[
    (
        "main",
        "if __name__ == \"__main__\":",
        "if __name__ == \"__main__\":\n\t$0",
        true,
    ),
    (
        "iter",
        "for … in …",
        "for ${1:item} in ${2:iterable}:\n\t$0",
        false,
    ),
    (
        "itere",
        "for … in enumerate(…)",
        "for ${1:i}, ${2:item} in enumerate(${3:iterable}):\n\t$0",
        false,
    ),
    (
        "prop",
        "@property",
        "@property\ndef ${1:name}(self):\n\treturn self._${1:name}$0",
        false,
    ),
    (
        "super",
        "super().method(…)",
        "super().${1:__init__}($2)$0",
        false,
    ),
];

/// IDECraft's Python live templates for the name typed at char `character` of `line`
/// (behaviour 39): offered when the name stands alone at the start of a statement; `main` only
/// at the top level. A template named exactly as typed is preselected.
pub fn live_items(line: &str, character: usize) -> Vec<CompletionItem> {
    let chars: Vec<char> = line.chars().collect();
    let character = character.min(chars.len());
    let mut start = character;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    if start == character || !chars[..start].iter().all(|c| *c == ' ' || *c == '\t') {
        return Vec::new();
    }
    if chars[character..].iter().any(|c| !c.is_whitespace()) {
        return Vec::new();
    }
    let typed: String = chars[start..character].iter().collect();
    let lower = typed.to_lowercase();
    LIVE.iter()
        .filter(|(n, _, _, top)| n.starts_with(&lower) && (!top || start == 0))
        .map(|(n, description, snippet, _)| CompletionItem {
            label: (*n).to_owned(),
            kind: Some("snippet".into()),
            detail: Some((*description).to_owned()),
            description: Some((*description).to_owned()),
            documentation: Some(format!("```python\n{}\n```", snippet_preview(snippet))),
            insert: (*snippet).to_owned(),
            snippet: true,
            preselect: *n == typed,
            sort_text: Some(format!("~live{n}")),
            ..Default::default()
        })
        .collect()
}

/// One public top-level name of a standard library module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdlibName {
    /// The name, e.g. `Path`.
    pub name: String,
    /// The module, e.g. `pathlib`.
    pub module: String,
    /// `class`, `function` or `variable`.
    pub kind: &'static str,
}

/// The `stdlib` folder of the typeshed stubs that come with the Python language server
/// `command` (pyright, basedpyright), symlinks followed.
pub fn typeshed_beside(command: &Path) -> Option<PathBuf> {
    let real = command.canonicalize().ok()?;
    for a in real.ancestors().take(8) {
        for sub in [
            "dist/typeshed-fallback/stdlib",
            "typeshed-fallback/stdlib",
            "lib/node_modules/pyright/dist/typeshed-fallback/stdlib",
            "node_modules/pyright/dist/typeshed-fallback/stdlib",
        ] {
            let p = a.join(sub);
            if p.join("builtins.pyi").is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The public top-level names declared in a stub `text` (behaviour 40): classes, functions and
/// annotated or assigned names at the start of a line, not starting with `_`, without imports and
/// without names only declared inside `if` blocks.
pub fn stub_names(text: &str) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let (rest, kind) = if let Some(r) = line.strip_prefix("class ") {
            (r, "class")
        } else if let Some(r) = line
            .strip_prefix("def ")
            .or_else(|| line.strip_prefix("async def "))
        {
            (r, "function")
        } else if line.starts_with(|c: char| c.is_alphabetic()) {
            (line, "variable")
        } else {
            continue;
        };
        let name: String = rest.chars().take_while(|c| is_word(*c)).collect();
        if name.is_empty() || name.starts_with('_') {
            continue;
        }
        if kind == "variable" {
            let after = rest[name.len()..].trim_start();
            if !(after.starts_with(':') || after.starts_with('=')) || after.starts_with("==") {
                continue;
            }
            if matches!(
                name.as_str(),
                "if" | "else" | "elif" | "from" | "import" | "try" | "except" | "with"
            ) {
                continue;
            }
        }
        if seen.insert(name.clone()) {
            out.push((name, kind));
        }
    }
    out
}

/// The module a stub file `path` under `stdlib` declares: `pathlib/__init__.pyi` → `pathlib`,
/// `os/path.pyi` → `os.path`; `None` for private modules (`_ast`) and `builtins`.
fn module_of(stdlib: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(stdlib).ok()?;
    let mut parts: Vec<String> = rel
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let last = parts.pop()?;
    let stem = last.strip_suffix(".pyi")?;
    if stem != "__init__" {
        parts.push(stem.to_owned());
    }
    if parts.is_empty()
        || parts
            .iter()
            .any(|p| p.starts_with('_') || p.contains('-') || p.contains('@'))
        || parts[0] == "builtins"
    {
        return None;
    }
    Some(parts.join("."))
}

/// All public names of the standard library stubs under `stdlib` (behaviour 40).
pub fn stdlib_index(stdlib: &Path) -> Vec<StdlibName> {
    let mut files = Vec::new();
    let mut stack = vec![stdlib.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "pyi") {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let Some(module) = module_of(stdlib, &f) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        for (name, kind) in stub_names(&text) {
            out.push(StdlibName {
                name,
                module: module.clone(),
                kind,
            });
        }
    }
    out
}

/// The standard library index of the installed Python language server, built once per process.
pub fn installed_stdlib() -> &'static [StdlibName] {
    static INDEX: OnceLock<Vec<StdlibName>> = OnceLock::new();
    INDEX.get_or_init(|| {
        crate::servers::resolve(crate::LanguageId::Python)
            .ok()
            .and_then(|r| typeshed_beside(&r.command))
            .map(|s| stdlib_index(&s))
            .unwrap_or_default()
    })
}

/// The root modules of the standard library in `index`.
fn stdlib_roots(index: &[StdlibName]) -> HashSet<&str> {
    index
        .iter()
        .map(|n| n.module.split('.').next().unwrap_or_default())
        .collect()
}

/// A top-level import statement of a module: its line, module and whether it is `from m import`.
struct ImportLine {
    line: usize,
    module: String,
    from: bool,
    simple: bool,
}

/// The import statements at the top of `text`, before the first other statement.
fn top_imports(text: &str) -> (Vec<ImportLine>, usize) {
    let mut out = Vec::new();
    let mut first_code = 0;
    let mut in_doc: Option<&str> = None;
    for (n, line) in text.split('\n').enumerate() {
        let t = line.trim_end_matches('\r');
        if let Some(q) = in_doc {
            if t.contains(q) {
                in_doc = None;
                first_code = n + 1;
            }
            continue;
        }
        if out.is_empty() && (t.starts_with("\"\"\"") || t.starts_with("'''")) {
            let q = &t[..3];
            if t.len() < 6 || !t[3..].contains(q) {
                in_doc = Some(q);
            } else {
                first_code = n + 1;
            }
            continue;
        }
        if t.trim().is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(r) = t.strip_prefix("import ") {
            let module = r.split([',', ' ']).next().unwrap_or_default().to_owned();
            out.push(ImportLine {
                line: n,
                module,
                from: false,
                simple: true,
            });
        } else if let Some(r) = t.strip_prefix("from ") {
            let module = r.split(' ').next().unwrap_or_default().to_owned();
            out.push(ImportLine {
                line: n,
                module,
                from: true,
                simple: !t.contains('(') && !t.ends_with('\\'),
            });
        } else {
            break;
        }
    }
    (out, first_code)
}

/// The edit that imports `name` from the standard library `module` into `text` (behaviour 41):
/// added to an existing `from module import …` line; else a new `from module import name` after
/// the last standard library import at the top of the file; else before the first import, with
/// an empty line when the imports there are not from the standard library; else after a module
/// docstring or at the top.
pub fn import_edit(
    text: &str,
    module: &str,
    name: &str,
    roots: &dyn Fn(&str) -> bool,
) -> CompletionEdit {
    from_import_edit(text, module, name, true, roots)
}

/// [`import_edit`] for a standard library (`std`) or a project module (`SPEC-0098` v14
/// behaviour 19a): a project module's new line goes after the last import at the top, after an
/// empty line when that one is from the standard library.
pub fn from_import_edit(
    text: &str,
    module: &str,
    name: &str,
    std: bool,
    roots: &dyn Fn(&str) -> bool,
) -> CompletionEdit {
    let lines: Vec<&str> = text.split('\n').collect();
    let (imports, _) = top_imports(text);
    if let Some(i) = imports
        .iter()
        .find(|i| i.from && i.simple && i.module == module)
    {
        let line = lines[i.line].trim_end_matches('\r');
        let end = line.find('#').unwrap_or(line.len());
        let end_chars = line[..end].trim_end().chars().count() as u64;
        return edit_at(i.line, end_chars, format!(", {name}"));
    }
    statement_edit(text, &format!("from {module} import {name}"), std, roots)
}

/// The edit that adds `import module` for a standard library module to `text` (`SPEC-0098` v14
/// behaviour 19a), placed as [`import_edit`] places a new line; `None` when the file already has
/// a top-level `import module`.
pub fn module_import_edit(
    text: &str,
    module: &str,
    roots: &dyn Fn(&str) -> bool,
) -> Option<CompletionEdit> {
    let (imports, _) = top_imports(text);
    if imports.iter().any(|i| !i.from && i.module == module) {
        return None;
    }
    Some(statement_edit(
        text,
        &format!("import {module}"),
        true,
        roots,
    ))
}

fn edit_at(line: usize, ch: u64, text: String) -> CompletionEdit {
    CompletionEdit {
        range: Range {
            start_line: line as u64,
            start_character: ch,
            end_line: line as u64,
            end_character: ch,
        },
        text,
    }
}

/// Where a new import `statement` goes: see [`import_edit`] (`std`) and [`from_import_edit`].
fn statement_edit(
    text: &str,
    statement: &str,
    std: bool,
    roots: &dyn Fn(&str) -> bool,
) -> CompletionEdit {
    let lines: Vec<&str> = text.split('\n').collect();
    let len = |n: usize| {
        lines
            .get(n)
            .map(|l| l.trim_end_matches('\r').chars().count())
            .unwrap_or(0) as u64
    };
    let (imports, first_code) = top_imports(text);
    let std_root = |m: &str| roots(m.split('.').next().unwrap_or_default());
    if !std {
        return match imports.last() {
            Some(last) => {
                let gap = if std_root(&last.module) { "\n" } else { "" };
                edit_at(last.line, len(last.line), format!("\n{gap}{statement}"))
            }
            None => edit_at(first_code, 0, format!("{statement}\n\n")),
        };
    }
    if let Some(last) = imports.iter().rev().find(|i| std_root(&i.module)) {
        return edit_at(last.line, len(last.line), format!("\n{statement}"));
    }
    if let Some(first) = imports.first() {
        let gap = if std_root(&first.module) { "" } else { "\n" };
        return edit_at(first.line, 0, format!("{statement}\n{gap}"));
    }
    edit_at(first_code, 0, format!("{statement}\n\n"))
}

/// The root modules of the standard library in `index`, for [`import_edit`].
pub fn roots_of(index: &[StdlibName]) -> HashSet<String> {
    stdlib_roots(index).into_iter().map(str::to_owned).collect()
}

/// Standard library names for the identifier typed at char `character` of line `line` in `text`
/// that the server's `items` do not already offer (behaviour 40): at most `limit`, names whose
/// lower case starts with the typed name's, each with the module at the right and the import as
/// an additional edit. Empty after a member access, in an import line or for a name shorter
/// than two characters.
pub fn stdlib_items(
    index: &[StdlibName],
    text: &str,
    line: u64,
    character: usize,
    items: &[CompletionItem],
    limit: usize,
) -> Vec<CompletionItem> {
    let line_text = text.split('\n').nth(line as usize).unwrap_or_default();
    let chars: Vec<char> = line_text.trim_end_matches('\r').chars().collect();
    let character = character.min(chars.len());
    let mut start = character;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let typed: String = chars[start..character].iter().collect();
    if typed.chars().count() < 2 || (start > 0 && chars[start - 1] == '.') {
        return Vec::new();
    }
    let t = line_text.trim_start();
    if t.starts_with("import ") || t.starts_with("from ") || t.starts_with('#') {
        return Vec::new();
    }
    let lower = typed.to_lowercase();
    // Names the file or the server already offer: from the same module, or without a module
    // (local names, builtins, already imported names).
    let offered: HashSet<(String, String)> = items
        .iter()
        .map(|i| (i.label.clone(), i.description.clone().unwrap_or_default()))
        .collect();
    let local: HashSet<&str> = items
        .iter()
        .filter(|i| i.description.is_none())
        .map(|i| i.label.as_str())
        .collect();
    let roots = stdlib_roots(index);
    let is_std = |m: &str| roots.contains(m);
    let mut found: BTreeMap<(bool, usize, String, String), &StdlibName> = BTreeMap::new();
    for n in index {
        if !n.name.to_lowercase().starts_with(&lower)
            || local.contains(n.name.as_str())
            || offered.contains(&(n.name.clone(), n.module.clone()))
        {
            continue;
        }
        // Exact case first, then shorter names, then shallower modules.
        let key = (
            !n.name.starts_with(&typed),
            n.name.len(),
            format!("{:02}{}", n.module.matches('.').count(), n.module),
            n.name.clone(),
        );
        found.entry(key).or_insert(n);
    }
    found
        .into_values()
        .take(limit)
        .map(|n| {
            let edit = import_edit(text, &n.module, &n.name, &is_std);
            CompletionItem {
                label: n.name.clone(),
                kind: Some(n.kind.to_owned()),
                detail: Some("Auto-import".into()),
                description: Some(n.module.clone()),
                insert: n.name.clone(),
                documentation: Some(format!(
                    "```python\nfrom {} import {}\n```",
                    n.module, n.name
                )),
                // Ranked like the server's own imports (`15.9999.Name.NN.module`).
                sort_text: Some(format!("15.9999.{}.99.{}", n.name, n.module)),
                additional_edits: vec![edit],
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0072 v9 AK-22: the first declaration of a pyright documentation.
    #[test]
    fn declaration_from_documentation() {
        assert_eq!(
            declaration("```python\nqty: int\n```").as_deref(),
            Some("qty: int")
        );
        assert_eq!(
            declaration("```python\ndef total_cents() -> int\n```").as_deref(),
            Some("def total_cents() -> int")
        );
        let join = "```python\ndef join(\n    a: LiteralString,\n    /,\n    *paths: LiteralString\n) -> LiteralString: ...\n\ndef join(\n    a: StrPath\n) -> str: ...\n```\n---\nJoin two or more pathname components.";
        assert_eq!(
            declaration(join).as_deref(),
            Some("def join(a: LiteralString, /, *paths: LiteralString) -> LiteralString")
        );
        assert_eq!(
            declaration("```python\n@overload\ndef f(x: int) -> int\n```").as_deref(),
            Some("def f(x: int) -> int")
        );
        assert_eq!(
            declaration("```\nfrom types import MappingProxyType\n```"),
            None
        );
        assert_eq!(declaration("Only prose"), None);
        // The parts the list shows (behaviour 34).
        assert_eq!(
            crate::script_completion::detail_parts("join", &declaration(join).unwrap()),
            (
                Some("(a: LiteralString, /, *paths: LiteralString)".into()),
                Some("LiteralString".into())
            )
        );
        assert_eq!(
            crate::script_completion::detail_parts("qty", "qty: int"),
            (None, Some("int".into()))
        );
        assert_eq!(
            crate::script_completion::detail_parts("total_cents", "def total_cents() -> int"),
            (Some("()".into()), Some("int".into()))
        );
    }

    /// SPEC-0072 v9 AK-22: parameters a caller passes.
    #[test]
    fn passed_parameter_count() {
        assert_eq!(passed_parameters("()"), 0);
        assert_eq!(passed_parameters("(self)"), 0);
        assert_eq!(passed_parameters("(cls, /)"), 0);
        assert_eq!(
            passed_parameters("(a: LiteralString, /, *paths: LiteralString)"),
            2
        );
        assert_eq!(passed_parameters("(self, d: dict[str, int] = {})"), 1);
    }

    /// SPEC-0072 v9 AK-22: overloads collapsed to the first.
    #[test]
    fn documentation_with_overloads_collapsed() {
        let doc = "```python\ndef join(\n    a: str\n) -> str: ...\n\ndef join(\n    a: bytes\n) -> bytes: ...\n```\n---\nJoin two paths.";
        let tidy = tidy_documentation(doc);
        assert!(
            tidy.contains("def join(\n    a: str\n) -> str: ..."),
            "{tidy}"
        );
        assert!(!tidy.contains("bytes"), "{tidy}");
        assert!(tidy.contains("Join two paths."), "{tidy}");
        assert!(tidy.contains("(1 more overload)"), "{tidy}");
        assert_eq!(
            tidy_documentation("```python\nqty: int\n```"),
            "```python\nqty: int\n```"
        );
    }

    fn function(label: &str, kind: &str) -> CompletionItem {
        CompletionItem {
            label: label.into(),
            insert: label.into(),
            kind: Some(kind.into()),
            ..Default::default()
        }
    }

    /// SPEC-0072 v9 AK-23: call parentheses and their exceptions.
    #[test]
    fn python_functions_get_parentheses() {
        let mut items = vec![
            function("getcwd", "function"),
            function("total_cents", "method"),
            function("Path", "class"),
            function("qty", "variable"),
        ];
        with_call_parentheses(&mut items, "    os.getc", 11);
        assert_eq!(items[0].insert, "getcwd($0)");
        assert!(items[0].snippet);
        assert_eq!(items[1].insert, "total_cents($0)");
        assert_eq!(items[2].insert, "Path");
        assert_eq!(items[3].insert, "qty");
        for (line, at) in [
            ("    os.getc()", 11),
            ("from os import getc", 19),
            ("import getc", 11),
            ("def getc", 8),
            ("@getc", 5),
        ] {
            let mut items = vec![function("getcwd", "function")];
            with_call_parentheses(&mut items, line, at);
            assert_eq!(items[0].insert, "getcwd", "{line}");
        }
    }

    /// SPEC-0072 v9 AK-23: postfix templates replace expression and name.
    #[test]
    fn python_postfix_templates() {
        let items = postfix_items("    item.qty.if", 7, 15);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["if", "ifn", "ifnn"]);
        assert_eq!(items[0].insert, "if item.qty:\n\t$0");
        let r = items[0].edit_range.clone().unwrap();
        assert_eq!(
            (r.start_line, r.start_character, r.end_character),
            (7, 4, 15)
        );
        let print = postfix_items("    item.name.print", 0, 19);
        assert_eq!(print[0].insert, "print(item.name)$0");
        let all = postfix_items("    os.listdir().", 0, 17);
        assert_eq!(all.len(), POSTFIX.len());
        let f = all.iter().find(|i| i.label == "for").unwrap();
        assert!(f.insert.starts_with("for ${1:"), "{}", f.insert);
        assert!(
            f.insert.ends_with(" in os.listdir():\n\t$0"),
            "{}",
            f.insert
        );
        assert!(postfix_items("x = 1.", 0, 6).is_empty());
        assert!(postfix_items("from os.pa", 0, 10).is_empty());
        assert!(postfix_items("    self.", 0, 9).is_empty());
    }

    /// SPEC-0072 v9 AK-23: live templates at the start of a statement.
    #[test]
    fn python_live_templates() {
        let top = live_items("main", 4);
        assert_eq!(top.len(), 1);
        assert!(top[0].preselect);
        assert_eq!(top[0].insert, "if __name__ == \"__main__\":\n\t$0");
        assert!(
            live_items("    main", 8).is_empty(),
            "main only at the top level"
        );
        let iter: Vec<String> = live_items("    iter", 8)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(iter, ["iter", "itere"]);
        assert!(live_items("    x = iter", 12).is_empty());
        assert!(live_items("    it(x)", 6).is_empty());
    }

    /// SPEC-0072 v9 AK-24: names of a stub and the module of a stub file.
    #[test]
    fn stub_names_and_modules() {
        let stub = "import sys\nfrom os import PathLike as PathLike\n__all__ = [\"Path\"]\n\nclass PurePath:\n    def x(self): ...\nclass Path(PurePath): ...\ndef fspath(p): ...\nsep: str\naltsep = None\n_private: int\nif sys.platform == \"win32\":\n    def only_windows(): ...\n";
        let names: Vec<(String, &str)> = stub_names(stub);
        assert_eq!(
            names,
            vec![
                ("PurePath".to_owned(), "class"),
                ("Path".to_owned(), "class"),
                ("fspath".to_owned(), "function"),
                ("sep".to_owned(), "variable"),
                ("altsep".to_owned(), "variable"),
            ]
        );
        let root = Path::new("/t/stdlib");
        assert_eq!(
            module_of(root, Path::new("/t/stdlib/pathlib/__init__.pyi")).as_deref(),
            Some("pathlib")
        );
        assert_eq!(
            module_of(root, Path::new("/t/stdlib/os/path.pyi")).as_deref(),
            Some("os.path")
        );
        assert_eq!(module_of(root, Path::new("/t/stdlib/_ast.pyi")), None);
        assert_eq!(module_of(root, Path::new("/t/stdlib/builtins.pyi")), None);
    }

    fn index() -> Vec<StdlibName> {
        let n = |name: &str, module: &str, kind: &'static str| StdlibName {
            name: name.into(),
            module: module.into(),
            kind,
        };
        vec![
            n("Path", "pathlib", "class"),
            n("Path", "zipfile", "class"),
            n("PathLike", "os", "class"),
            n("path", "os", "variable"),
            n("datetime", "datetime", "class"),
            n("restock", "nowhere", "function"),
        ]
    }

    /// SPEC-0072 v9 AK-24: where the import goes.
    #[test]
    fn stdlib_import_placement() {
        let std = |m: &str| matches!(m, "os" | "pathlib" | "datetime" | "sys");
        let text = "import os\n\nfrom src.inventory.model import Item\n\n\ndef report(item: Item):\n    Pa\n";
        let e = import_edit(text, "pathlib", "Path", &std);
        assert_eq!((e.range.start_line, e.range.start_character), (0, 9));
        assert_eq!(e.text, "\nfrom pathlib import Path");
        // An existing `from pathlib import …` line grows.
        let e = import_edit(
            "from pathlib import PurePath  # x\nx = 1\n",
            "pathlib",
            "Path",
            &std,
        );
        assert_eq!((e.range.start_line, e.range.start_character), (0, 28));
        assert_eq!(e.text, ", Path");
        // Only project imports: before them, with an empty line.
        let e = import_edit("from src.x import y\n\nz = 1\n", "pathlib", "Path", &std);
        assert_eq!((e.range.start_line, e.range.start_character), (0, 0));
        assert_eq!(e.text, "from pathlib import Path\n\n");
        // No imports: after the module docstring.
        let e = import_edit(
            "\"\"\"Doc.\n\nMore.\n\"\"\"\nx = 1\n",
            "pathlib",
            "Path",
            &std,
        );
        assert_eq!(
            (e.range.start_line, e.text.as_str()),
            (4, "from pathlib import Path\n\n")
        );
    }

    /// SPEC-0098 v14 AK-18: where a project module's and a whole module's import go.
    #[test]
    fn project_and_module_import_placement() {
        let std = |m: &str| matches!(m, "os" | "collections" | "json" | "typing");
        let text = "import os\nfrom typing import List\n\n\ndef f():\n    pass\n";
        let e = from_import_edit(text, "util", "helper", false, &std);
        assert_eq!((e.range.start_line, e.range.start_character), (1, 23));
        assert_eq!(e.text, "\n\nfrom util import helper");
        let e = from_import_edit(
            "import os\n\nfrom app import a\n\nx = 1\n",
            "util",
            "h",
            false,
            &std,
        );
        assert_eq!(
            (e.range.start_line, e.text.as_str()),
            (2, "\nfrom util import h")
        );
        let e = from_import_edit("from util import a\nx = 1\n", "util", "h", false, &std);
        assert_eq!(
            (e.range.start_line, e.range.start_character, e.text.as_str()),
            (0, 18, ", h")
        );
        let e = from_import_edit("x = 1\n", "util", "h", false, &std);
        assert_eq!(
            (e.range.start_line, e.text.as_str()),
            (0, "from util import h\n\n")
        );
        let e = module_import_edit(text, "json", &std).unwrap();
        assert_eq!((e.range.start_line, e.text.as_str()), (1, "\nimport json"));
        assert!(module_import_edit("import json\n", "json", &std).is_none());
    }

    /// SPEC-0072 v9 AK-24: standard library names the server did not offer.
    #[test]
    fn stdlib_names_offered() {
        let text = "import os\n\ndef f():\n    Path\n";
        let server = vec![CompletionItem {
            label: "PathLike".into(),
            description: Some("os".into()),
            ..Default::default()
        }];
        let items = stdlib_items(&index(), text, 3, 8, &server, 10);
        let labels: Vec<(String, String)> = items
            .iter()
            .map(|i| (i.label.clone(), i.description.clone().unwrap()))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("Path".to_owned(), "pathlib".to_owned()),
                ("Path".to_owned(), "zipfile".to_owned()),
                ("path".to_owned(), "os".to_owned()),
            ]
        );
        assert_eq!(
            items[0].additional_edits[0].text,
            "\nfrom pathlib import Path"
        );
        // Not after a member access, not for one character, not when the name is local.
        assert!(stdlib_items(&index(), "x.Path", 0, 6, &[], 10).is_empty());
        assert!(stdlib_items(&index(), "P", 0, 1, &[], 10).is_empty());
        let local = vec![CompletionItem {
            label: "datetime".into(),
            ..Default::default()
        }];
        assert!(stdlib_items(&index(), "datet", 0, 5, &local, 10).is_empty());
    }
}
