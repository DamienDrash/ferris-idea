//! Change Signature (`SPEC-0113`): Ferris IDEA's own refactoring. The language server only finds the
//! declaration and the references; the parameter list of the declaration and the argument list of
//! every call are rewritten here, on the text, for C-like, Rust, Go, Python, JavaScript/TypeScript,
//! Java, Kotlin, PHP and Ruby syntax.

use std::ops::Range as Span;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::LanguageId;
use crate::insight::{LocationKind, Range, locations_in};
use crate::lsp::LspError;
use crate::rename::{Edit, FileEdits};

/// Where a parameter's name sits in its declaration text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameAt {
    /// `name: Type` or an untyped `name` (Rust, Python, TypeScript, Kotlin …).
    BeforeColon,
    /// `Type name` (C, C++, Java, PHP `Type $name`).
    Last,
    /// `name Type` (Go).
    First,
}

fn name_at(language: LanguageId) -> NameAt {
    match language {
        LanguageId::C | LanguageId::Cpp | LanguageId::Java | LanguageId::Php => NameAt::Last,
        LanguageId::Go => NameAt::First,
        _ => NameAt::BeforeColon,
    }
}

/// The items of a bracketed list whose `(` is at byte `open`: the span inside the brackets and the
/// span of each comma-separated item (trimmed), at top level only. Strings are skipped; with `types`
/// angle brackets count as brackets too (generic types in parameters), and `'` is not a quote
/// (Rust lifetimes). `None` when the list is not closed.
pub fn split_list(text: &str, open: usize, types: bool) -> Option<(Span<usize>, Vec<Span<usize>>)> {
    let bytes = text.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut items = Vec::new();
    let mut start = open + 1;
    let mut i = open + 1;
    let push = |items: &mut Vec<Span<usize>>, a: usize, b: usize| {
        let slice = &text[a..b];
        let lead = slice.len() - slice.trim_start().len();
        let trail = slice.len() - slice.trim_end().len();
        if a + lead < b - trail {
            items.push(a + lead..b - trail);
        }
    };
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b'"' | b'`' => i = skip_string(bytes, i, ch),
            b'\'' if !types => i = skip_string(bytes, i, ch),
            b'(' | b'[' | b'{' => depth += 1,
            b'<' if types => depth += 1,
            b'>' if types && depth > 0 && bytes.get(i.wrapping_sub(1)) != Some(&b'-') => depth -= 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    if ch != b')' {
                        return None;
                    }
                    push(&mut items, start, i);
                    return Some((open + 1..i, items));
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                push(&mut items, start, i);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Index of the closing quote of the string starting at `i` (backslash escapes skipped).
fn skip_string(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    i += 1;
    while i < bytes.len() && bytes[i] != quote {
        if bytes[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i.min(bytes.len().saturating_sub(1))
}

/// Byte index of the `(` that opens the list after a name ending at byte `end`: spaces, a generic
/// argument list (`<T>`, `::<T>`) and Kotlin/Java type arguments may come in between.
pub fn open_paren_after(text: &str, end: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = end;
    let mut angle = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' if angle == 0 => return Some(i),
            b'<' => angle += 1,
            b'>' if angle > 0 => angle -= 1,
            b':' if angle == 0 && bytes.get(i + 1) == Some(&b':') => i += 1,
            b' ' | b'\t' => {}
            _ if angle > 0 => {}
            _ => return None,
        }
        i += 1;
    }
    None
}

/// The `(` of the call a reference at `span` of `text` makes to `function`: right after the span,
/// or, for a server whose reference covers more than the name (jdtls for a qualified call
/// `r.addiere(…)`, hand test 40, D26), right after the last occurrence of the name as a word inside
/// the span.
pub fn call_after(text: &str, span: std::ops::Range<usize>, function: &str) -> Option<usize> {
    if let Some(open) = open_paren_after(text, span.end) {
        return Some(open);
    }
    let inside = text.get(span.clone())?;
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    inside
        .match_indices(function)
        .filter(|(i, _)| {
            let before = inside[..*i].chars().next_back();
            let after = inside[i + function.len()..].chars().next();
            !before.is_some_and(word) && !after.is_some_and(word)
        })
        .filter_map(|(i, _)| open_paren_after(text, span.start + i + function.len()))
        .find(|open| *open < span.end)
}

/// The name a parameter declares.
pub fn param_name(param: &str, language: LanguageId) -> String {
    let declared = param.split('=').next().unwrap_or(param).trim();
    let ident = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect::<String>()
            .trim_start_matches('$')
            .to_owned()
    };
    let words: Vec<&str> = declared
        .split(|c: char| c.is_whitespace() || c == '*' || c == '&')
        .filter(|w| !w.is_empty())
        .collect();
    match name_at(language) {
        NameAt::BeforeColon => {
            let before = declared.split(':').next().unwrap_or(declared);
            let last = before
                .split_whitespace()
                .rfind(|w| !matches!(*w, "mut" | "val" | "var" | "vararg" | "readonly"))
                .unwrap_or(before);
            ident(last.trim_start_matches(['*', '&']))
        }
        NameAt::Last => {
            let without_array = declared.trim_end_matches("[]").trim_end();
            let last = without_array
                .rsplit(|c: char| c.is_whitespace() || c == '*' || c == '&')
                .find(|w| !w.is_empty())
                .unwrap_or(without_array);
            ident(last)
        }
        NameAt::First => words.first().map_or_else(String::new, |w| ident(w)),
    }
}

/// Whether the first parameter is the receiver that a method call passes before the dot (Rust
/// `self`, `&self`, `&mut self`; Python `self` and `cls`).
fn is_receiver(param: &str, language: LanguageId) -> bool {
    let name = param_name(param, language);
    match language {
        LanguageId::Rust => name == "self",
        LanguageId::Python => name == "self" || name == "cls",
        _ => false,
    }
}

/// Where an argument of the new list comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// The argument given for old parameter `i`.
    Old(usize),
    /// A value for a new parameter.
    Value(String),
}

/// The new parameter list as typed: the declaration of each parameter, and for each argument of a
/// call where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    /// Parameter declarations as they go into the declaration.
    declarations: Vec<String>,
    /// For each new parameter, where a call's argument comes from.
    sources: Vec<Source>,
    /// Names of the old parameters (without the receiver).
    old_names: Vec<String>,
}

/// Reads `new_params` against the old parameters (`old`, without a receiver): a parameter whose
/// name exists keeps the matching argument; a new one needs `= value`, the value existing calls get
/// (the `= value` does not go into the declaration).
pub fn mapping(old: &[String], new_params: &str, language: LanguageId) -> Result<Mapping, String> {
    let wrapped = format!("({new_params})");
    let (_, items) =
        split_list(&wrapped, 0, true).ok_or("the parameter list has unbalanced brackets")?;
    let old_names: Vec<String> = old.iter().map(|p| param_name(p, language)).collect();
    let mut declarations = Vec::new();
    let mut sources = Vec::new();
    for span in items {
        let item = &wrapped[span];
        let name = param_name(item, language);
        if name.is_empty() {
            return Err(format!("`{item}` declares no parameter name"));
        }
        match old_names.iter().position(|n| *n == name) {
            Some(i) => {
                if sources.contains(&Source::Old(i)) {
                    return Err(format!("parameter `{name}` appears twice"));
                }
                sources.push(Source::Old(i));
                declarations.push(item.to_owned());
            }
            None => {
                let (declared, value) = match top_level_eq(item) {
                    Some(at) => (item[..at].trim_end(), item[at + 1..].trim()),
                    None => {
                        return Err(format!(
                            "new parameter `{name}` needs a value for existing calls: `{item} = value`"
                        ));
                    }
                };
                if value.is_empty() {
                    return Err(format!("new parameter `{name}` has an empty value"));
                }
                sources.push(Source::Value(value.to_owned()));
                declarations.push(declared.to_owned());
            }
        }
    }
    Ok(Mapping {
        declarations,
        sources,
        old_names,
    })
}

/// Byte index of the first `=` that is not part of `==`, `=>`, `<=`, `>=`, `!=`.
fn top_level_eq(item: &str) -> Option<usize> {
    let b = item.as_bytes();
    (0..b.len()).find(|&i| {
        b[i] == b'='
            && b.get(i + 1) != Some(&b'=')
            && b.get(i + 1) != Some(&b'>')
            && !matches!(
                i.checked_sub(1).map(|j| b[j]),
                Some(b'=' | b'<' | b'>' | b'!')
            )
    })
}

/// The new argument list for a call whose arguments are `args`, or why the call is left alone.
/// With `receiver`, a call that passes one argument more than the parameters (Rust `Type::f(x, …)`)
/// keeps its first argument first. Named arguments (`name = value`, `name: value`) follow their
/// parameter.
pub fn rewrite_args(mapping: &Mapping, args: &[&str], receiver: bool) -> Result<String, String> {
    let (lead, args) = match args.split_first() {
        Some((first, rest)) if receiver && args.len() == mapping.old_names.len() + 1 => {
            (Some(*first), rest)
        }
        _ => (None, args),
    };
    let named = |a: &str| -> Option<String> {
        let at = top_level_eq(a).or_else(|| {
            let colon = a.find(':')?;
            (a.as_bytes().get(colon + 1) != Some(&b':')).then_some(colon)
        })?;
        let name = a[..at].trim();
        (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .then(|| name.to_owned())
    };
    let positional = args.iter().take_while(|a| named(a).is_none()).count();
    if positional > mapping.old_names.len() {
        return Err(format!(
            "passes {} arguments for {} parameters",
            positional,
            mapping.old_names.len()
        ));
    }
    let mut out: Vec<String> = lead.map(str::to_owned).into_iter().collect();
    let mut keep_named = Vec::new();
    for source in &mapping.sources {
        match source {
            Source::Value(v) => out.push(v.clone()),
            Source::Old(i) if *i < positional => out.push(args[*i].to_owned()),
            Source::Old(i) => {
                let name = &mapping.old_names[*i];
                match args[positional..]
                    .iter()
                    .find(|a| named(a).as_deref() == Some(name.as_str()))
                {
                    Some(a) => keep_named.push((*a).to_owned()),
                    None => {
                        return Err(format!("passes no argument for `{name}`"));
                    }
                }
            }
        }
    }
    out.extend(keep_named);
    Ok(out.join(", "))
}

/// Whether the name starting at byte `start` is declared there rather than called: the word
/// before it is a type or a declaring keyword (`int add(`, `fn add(`, `def add(`), not a keyword
/// that starts an expression (`return add(`).
pub fn is_declaration(text: &str, start: usize) -> bool {
    let before = text[..start].trim_end_matches([' ', '\t']);
    let Some(last) = before.chars().last() else {
        return false;
    };
    if last == '*' || last == '&' {
        return before
            .trim_end_matches(['*', '&', ' '])
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '>');
    }
    if last == '>' {
        return true;
    }
    if !(last.is_alphanumeric() || last == '_') {
        return false;
    }
    let word: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    !matches!(
        word.as_str(),
        "return"
            | "else"
            | "case"
            | "throw"
            | "yield"
            | "await"
            | "new"
            | "in"
            | "not"
            | "and"
            | "or"
            | "do"
            | "then"
            | "echo"
            | "print"
            | "go"
            | "defer"
    )
}

/// A parameter or argument list to rewrite.
struct Target {
    /// Inside the brackets.
    inner: Span<usize>,
    /// The items (arguments of a call).
    items: Vec<Span<usize>>,
    /// A declaration gets the new parameter list; a call gets its arguments rearranged.
    declaration: bool,
    /// `path:line`, for skipped calls.
    place: String,
}

/// What rendering the lists of one file needs.
struct Context<'a> {
    text: &'a str,
    /// Sorted by start, outer before inner.
    targets: &'a [Target],
    map: &'a Mapping,
    decl_list: &'a str,
    receiver: bool,
}

/// The new text inside target `i`'s brackets, with calls nested in its arguments rewritten first;
/// `None` (and a note in `skipped`) when the call cannot be rewritten.
fn render(c: &Context, i: usize, skipped: &mut Vec<String>) -> Option<String> {
    let t = &c.targets[i];
    if t.declaration {
        return Some(c.decl_list.to_owned());
    }
    let mut args = Vec::new();
    for item in &t.items {
        let mut out = String::new();
        let mut at = item.start;
        let mut j = i + 1;
        while j < c.targets.len() && c.targets[j].inner.start < item.end {
            let inner = c.targets[j].inner.clone();
            if inner.start >= at && inner.end <= item.end {
                out.push_str(&c.text[at..inner.start]);
                let nested = render(c, j, skipped);
                out.push_str(nested.as_deref().unwrap_or(&c.text[inner.clone()]));
                at = inner.end;
            }
            j += 1;
        }
        out.push_str(&c.text[at..item.end]);
        args.push(out);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match rewrite_args(c.map, &refs, c.receiver) {
        Ok(a) => Some(a),
        Err(why) => {
            skipped.push(format!("{}: {why}", t.place));
            None
        }
    }
}

/// What Change Signature changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignaturePlan {
    /// The function's name.
    pub function: String,
    /// The parameter list before, without the receiver.
    pub old_parameters: String,
    /// The parameter list after, as it goes into the declaration.
    pub new_parameters: String,
    /// Edits per file, sorted by path.
    pub files: Vec<FileEdits>,
    /// References left unchanged, as `path:line: reason`.
    pub skipped: Vec<String>,
}

/// Line and character (Unicode scalars) of byte `at` in `text`.
fn position(text: &str, at: usize) -> (u64, u64) {
    let before = &text[..at];
    let line = before.matches('\n').count() as u64;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, text[start..at].chars().count() as u64)
}

/// Byte offset of `(line, character)` in `text`.
fn byte_at(text: &str, line: u64, character: u64) -> Option<usize> {
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n')? + 1;
    }
    let rest = &text[start..];
    let end = rest.find('\n').unwrap_or(rest.len());
    let offset = rest[..end]
        .char_indices()
        .nth(character as usize)
        .map_or(end, |(i, _)| i);
    Some(start + offset)
}

fn edit(text: &str, span: Span<usize>, new_text: String) -> Edit {
    let (sl, sc) = position(text, span.start);
    let (el, ec) = position(text, span.end);
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

/// The parameters of the function declared with its name at `name` in `text`: the list's span, the
/// receiver (if any) and the other parameters.
pub fn declaration(
    text: &str,
    name: Span<usize>,
    language: LanguageId,
) -> Option<(Span<usize>, Option<String>, Vec<String>)> {
    let open = open_paren_after(text, name.end)?;
    let (inner, items) = split_list(text, open, true)?;
    let mut params: Vec<String> = items.into_iter().map(|s| text[s].to_owned()).collect();
    let receiver = match params.first() {
        Some(p) if is_receiver(p, language) => Some(params.remove(0)),
        _ => None,
    };
    Some((inner, receiver, params))
}

/// Plans Change Signature for the function at a position of `file` (its declaration or a call), with
/// the parameter list `new_params` (without the receiver). `text` is the unsaved text of `file` when
/// given. The language server finds the declaration and the references; nothing is written.
pub async fn plan_change_signature_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    new_params: Option<&str>,
    wait: Duration,
) -> Result<SignaturePlan, LspError> {
    let language = LanguageId::detect(file)
        .ok_or_else(|| LspError::Protocol("no language for this file".into()))?;
    let protocol = |m: String| LspError::Protocol(m);
    let read = |path: &Path| -> Result<String, LspError> {
        if path == file
            && let Some(t) = text
        {
            return Ok(t.to_owned());
        }
        // The shown text of another open file with unsaved changes (SPEC-0220 v4 behaviour 13).
        crate::shown::read(path).map_err(|e| protocol(format!("{}: {e}", path.display())))
    };
    let definitions = locations_in(
        root,
        file,
        text,
        line,
        character,
        LocationKind::Definition,
        wait,
    )
    .await?;
    let def = definitions
        .into_iter()
        .next()
        .ok_or_else(|| protocol("no function at the caret".into()))?;
    let def_path = def.path.canonicalize().unwrap_or(def.path.clone());
    let def_text = read(&def_path)?;
    let name_span = byte_at(&def_text, def.range.start_line, def.range.start_character)
        .zip(byte_at(
            &def_text,
            def.range.end_line,
            def.range.end_character,
        ))
        .map(|(a, b)| a..b)
        .ok_or_else(|| protocol("the declaration lies outside its file".into()))?;
    let function = def_text[name_span.clone()].to_owned();
    let (decl_inner, receiver, old) = declaration(&def_text, name_span.clone(), language)
        .ok_or_else(|| protocol(format!("`{function}` has no parameter list")))?;
    let old_parameters = old.join(", ");
    let Some(new_params) = new_params else {
        return Ok(SignaturePlan {
            function,
            old_parameters: old_parameters.clone(),
            new_parameters: old_parameters,
            files: Vec::new(),
            skipped: Vec::new(),
        });
    };
    let map = mapping(&old, new_params, language).map_err(protocol)?;
    let new_parameters = map.declarations.join(", ");
    let decl_list = receiver
        .iter()
        .cloned()
        .chain(map.declarations.iter().cloned())
        .collect::<Vec<_>>()
        .join(", ");
    let mut by_file: std::collections::BTreeMap<PathBuf, (String, Vec<Target>)> =
        std::collections::BTreeMap::new();
    by_file
        .entry(def_path.clone())
        .or_insert_with(|| (def_text.clone(), Vec::new()))
        .1
        .push(Target {
            inner: decl_inner,
            items: Vec::new(),
            declaration: true,
            place: String::new(),
        });
    let ref_text = (def_path.as_path() == file).then_some(def_text.as_str());
    let references = locations_in(
        root,
        &def_path,
        ref_text,
        def.range.start_line,
        def.range.start_character,
        LocationKind::References,
        wait,
    )
    .await?;
    let mut skipped = Vec::new();
    for r in references {
        let path = r.path.canonicalize().unwrap_or(r.path.clone());
        if path == def_path && r.range.start_line == def.range.start_line {
            continue;
        }
        if !by_file.contains_key(&path) {
            let t = read(&path)?;
            by_file.insert(path.clone(), (t, Vec::new()));
        }
        let entry = by_file.get_mut(&path).expect("inserted");
        let file_text = &entry.0;
        let place = format!("{}:{}", path.display(), r.range.start_line + 1);
        let (Some(start), Some(end)) = (
            byte_at(file_text, r.range.start_line, r.range.start_character),
            byte_at(file_text, r.range.end_line, r.range.end_character),
        ) else {
            skipped.push(format!("{place}: outside the file"));
            continue;
        };
        let Some((inner, items)) = call_after(file_text, start..end, &function)
            .and_then(|o| split_list(file_text, o, false))
        else {
            skipped.push(format!("{place}: not a call"));
            continue;
        };
        // Another declaration of the function (a C prototype, a header) gets the same new list.
        let declaration = is_declaration(file_text, start);
        entry.1.push(Target {
            inner,
            items,
            declaration,
            place,
        });
    }
    let mut files = Vec::new();
    for (path, (text, mut targets)) in by_file {
        targets.sort_by_key(|t| (t.inner.start, std::cmp::Reverse(t.inner.end)));
        targets.dedup_by_key(|t| t.inner.clone());
        let mut edits = Vec::new();
        let mut covered = 0;
        for i in 0..targets.len() {
            if targets[i].inner.start < covered {
                continue;
            }
            covered = targets[i].inner.end;
            let context = Context {
                text: &text,
                targets: &targets,
                map: &map,
                decl_list: &decl_list,
                receiver: receiver.is_some(),
            };
            match render(&context, i, &mut skipped) {
                Some(new_text) if new_text != text[targets[i].inner.clone()] => {
                    edits.push(edit(&text, targets[i].inner.clone(), new_text))
                }
                _ => {}
            }
        }
        if !edits.is_empty() {
            files.push(FileEdits { path, edits });
        }
    }
    Ok(SignaturePlan {
        function,
        old_parameters,
        new_parameters,
        files,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand test 40 (D26): a reference that covers the whole qualified call still finds its `(`.
    #[test]
    fn calls_are_found_after_a_wide_reference() {
        let text = "        System.out.println(r.addiere(1, \"abc\"));\n";
        let name = text.find("addiere").unwrap();
        // Just the name.
        assert_eq!(call_after(text, name..name + 7, "addiere"), Some(name + 7));
        // The whole invocation `r.addiere(1, "abc")`.
        let start = text.find("r.addiere").unwrap();
        let end = text.find("\")").unwrap() + 2;
        assert_eq!(call_after(text, start..end, "addiere"), Some(name + 7));
        // A method reference is no call.
        let text = "list.forEach(this::addiere);\n";
        let at = text.find("addiere").unwrap();
        assert_eq!(call_after(text, at..at + 7, "addiere"), None);
        // Inside a wide span, `addiereAlle(…)` is another function.
        let text = "x(r.addiereAlle(1));\n";
        assert_eq!(call_after(text, 0..18, "addiere"), None);
    }

    fn items(text: &str, types: bool) -> Vec<&str> {
        let open = text.find('(').unwrap();
        let (_, spans) = split_list(text, open, types).unwrap();
        spans.into_iter().map(|s| &text[s]).collect()
    }

    #[test]
    fn lists_split_at_top_level_commas() {
        assert_eq!(
            items("f(a, g(b, c), \"x, y\", [1, 2], 'z')", false),
            ["a", "g(b, c)", "\"x, y\"", "[1, 2]", "'z'"]
        );
        assert_eq!(
            items("fn f(m: HashMap<K, V>, s: &'a str)", true),
            ["m: HashMap<K, V>", "s: &'a str"]
        );
        assert_eq!(items("f(a < b, c > d)", false), ["a < b", "c > d"]);
        assert_eq!(items("f( )", false), Vec::<&str>::new());
        assert!(split_list("f(a, b", 1, false).is_none());
        assert_eq!(open_paren_after("foo::<u8>(1)", 3), Some(9));
        assert_eq!(open_paren_after("foo <T> (1)", 3), Some(8));
        assert_eq!(open_paren_after("foo = 1", 3), None);
    }

    #[test]
    fn parameter_names_per_language() {
        use LanguageId::*;
        assert_eq!(param_name("mut count: usize", Rust), "count");
        assert_eq!(param_name("&mut self", Rust), "self");
        assert_eq!(param_name("const std::string& name", Cpp), "name");
        assert_eq!(param_name("int values[]", C), "values");
        assert_eq!(param_name("final List<String> items", Java), "items");
        assert_eq!(param_name("?int $limit = 10", Php), "limit");
        assert_eq!(param_name("limit int", Go), "limit");
        assert_eq!(param_name("*args", Python), "args");
        assert_eq!(param_name("retries: int = 3", Python), "retries");
        assert_eq!(param_name("vararg names: String", Kotlin), "names");
        assert_eq!(param_name("opts", JavaScript), "opts");
        assert!(is_receiver("&self", Rust) && is_receiver("cls", Python));
        assert!(!is_receiver("self", Java));
    }

    #[test]
    fn calls_follow_the_new_order_and_get_values_for_new_parameters() {
        let old = [
            "a: i32".to_owned(),
            "b: &str".to_owned(),
            "c: bool".to_owned(),
        ];
        let m = mapping(&old, "b: &str, a: i32, flag: u8 = 7", LanguageId::Rust).unwrap();
        assert_eq!(m.declarations, ["b: &str", "a: i32", "flag: u8"]);
        assert_eq!(
            rewrite_args(&m, &["1", "\"x\"", "true"], false).unwrap(),
            "\"x\", 1, 7"
        );
        assert_eq!(
            rewrite_args(&m, &["obj", "1", "\"x\"", "true"], true).unwrap(),
            "obj, \"x\", 1, 7",
            "a receiver passed as the first argument stays first"
        );
        assert!(rewrite_args(&m, &["1"], false).unwrap_err().contains("`b`"));
        assert!(rewrite_args(&m, &["1", "2", "3", "4"], false).is_err());
        assert!(
            mapping(&old, "a: i32, d: u8", LanguageId::Rust)
                .unwrap_err()
                .contains("= value")
        );
        assert!(
            mapping(&old, "a: i32, a: i32", LanguageId::Rust)
                .unwrap_err()
                .contains("twice")
        );
    }

    #[test]
    fn named_arguments_follow_their_parameter() {
        let old = [
            "path".to_owned(),
            "retries=3".to_owned(),
            "verbose=False".to_owned(),
        ];
        let m = mapping(&old, "path, verbose=False", LanguageId::Python).unwrap();
        assert_eq!(
            rewrite_args(&m, &["p", "verbose=True", "retries=5"], false).unwrap(),
            "p, verbose=True"
        );
        assert_eq!(top_level_eq("a == b"), None);
        assert_eq!(top_level_eq("x => y"), None);
        assert_eq!(top_level_eq("n: u8 = 0"), Some(6));
    }

    #[test]
    fn declarations_are_told_from_calls() {
        let t = "int add(int a);\nint *mk(void);\nfn run(x: u8)\nreturn add(1);\nx = add(2);\nf(add(3))";
        let at = |pat: &str| t.find(pat).unwrap();
        assert!(is_declaration(t, at("add(int")));
        assert!(is_declaration(t, at("mk(")));
        assert!(is_declaration(t, at("run(")));
        assert!(!is_declaration(t, at("add(1")));
        assert!(!is_declaration(t, at("add(2")));
        assert!(!is_declaration(t, at("add(3")));
    }

    #[test]
    fn declarations_keep_the_receiver() {
        let text = "impl S { fn run(&mut self, a: i32, b: Vec<(u8, u8)>) {} }";
        let at = text.find("run").unwrap();
        let (inner, receiver, params) = declaration(text, at..at + 3, LanguageId::Rust).unwrap();
        assert_eq!(receiver.as_deref(), Some("&mut self"));
        assert_eq!(params, ["a: i32", "b: Vec<(u8, u8)>"]);
        assert_eq!(&text[inner], "&mut self, a: i32, b: Vec<(u8, u8)>");
        let e = edit("ab\ncdé(x)", 8..9, "y".into());
        assert_eq!(
            (
                e.range.start_line,
                e.range.start_character,
                e.old_text.as_str()
            ),
            (1, 4, "x")
        );
        assert_eq!(byte_at("ab\ncdé(x)", 1, 4), Some(8));
    }
}
