//! Smart type completion (`SPEC-0072` v8 behaviours 25–26): the type expected at the caret, from
//! the text and the language server's hover, signature help and document symbols, and the
//! completion items whose type fits it. Assignability is decided from type names (no compiler):
//! same types, type variables, Java widening and boxing, a table of common JDK supertypes and the
//! supertypes declared in the open file.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::LanguageId;
use crate::insight::{self, CompletionItem};
use crate::lsp::LspError;

/// Where the expected type was found (behaviour 25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// `Type name = |` or `name: Type = |`.
    Declaration,
    /// `name = |`: the type of `name` from the hover.
    Assignment,
    /// `call(…, |`: the parameter's type from the signature help.
    Argument,
    /// `return |`: the enclosing method's return type.
    Return,
    /// `if (|` / `while (|`: a condition.
    Condition,
}

/// What the text before the caret says about the expected type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Context {
    /// A declared type, or `boolean` for a condition.
    Known(String, Origin),
    /// An assignment to the name at this 0-based character of the caret's line.
    Assignment(usize),
    /// An argument of a call.
    Argument,
    /// A return statement.
    Return,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Splits `text` at its last top-level whitespace (outside `<>`, `()`, `[]`).
fn split_last_space(text: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    let mut at = None;
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => at = Some(i),
            _ => {}
        }
    }
    let i = at?;
    Some((text[..i].trim(), text[i..].trim()))
}

/// The context of the caret at char `character` of `line` (behaviour 25), judged from the text
/// before the identifier being typed.
pub fn context_at(line: &str, character: usize) -> Option<Context> {
    let chars: Vec<char> = line.chars().collect();
    let mut start = character.min(chars.len());
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let before: String = chars[..start].iter().collect();
    let t = before.trim_end();
    if t.ends_with("return") && !t[..t.len() - 6].ends_with(is_word) {
        return Some(Context::Return);
    }
    if let Some(lhs) = t.strip_suffix('=')
        && !lhs.ends_with(['=', '!', '<', '>', '+', '-', '*', '/', '%', '&', '|', '^'])
    {
        let lhs = lhs.trim_end();
        // The statement starts after the last `;`, `{` or `}` (and `(` of a `for`).
        let from = lhs.rfind([';', '{', '}', '(']).map_or(0, |i| i + 1);
        let stmt = lhs[from..].trim();
        // TypeScript: `const name: Type`.
        if let Some((head, ty)) = stmt.split_once(':')
            && head
                .trim()
                .rsplit(char::is_whitespace)
                .next()
                .is_some_and(|n| n.chars().all(is_word))
        {
            let ty = ty.trim();
            return (!ty.is_empty()).then(|| Context::Known(ty.to_owned(), Origin::Declaration));
        }
        let modifiers = [
            "final",
            "static",
            "private",
            "public",
            "protected",
            "volatile",
            "transient",
        ];
        let mut stmt = stmt;
        while let Some((m, rest)) = stmt.split_once(char::is_whitespace) {
            if modifiers.contains(&m) {
                stmt = rest.trim_start();
            } else {
                break;
            }
        }
        if let Some((ty, name)) = split_last_space(stmt)
            && name.chars().all(is_word)
            && !ty.is_empty()
        {
            if matches!(ty, "var" | "let" | "const" | "val") {
                return None;
            }
            return Some(Context::Known(ty.to_owned(), Origin::Declaration));
        }
        // `name = ` or `this.name = `: the last name before `=`.
        let end = lhs.chars().count();
        let lhs_chars: Vec<char> = lhs.chars().collect();
        let mut s = end;
        while s > 0 && is_word(lhs_chars[s - 1]) {
            s -= 1;
        }
        return (s < end).then_some(Context::Assignment(s));
    }
    // An open parenthesis before the caret: a condition or a call.
    let mut depth = 0i32;
    for (i, c) in before.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' if depth == 0 => {
                let head = before[..i].trim_end();
                let word: String = head
                    .chars()
                    .rev()
                    .take_while(|c| is_word(*c))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                return match word.as_str() {
                    "if" | "while" => Some(Context::Known("boolean".into(), Origin::Condition)),
                    "for" | "switch" | "catch" | "synchronized" | "" => None,
                    _ => Some(Context::Argument),
                };
            }
            '(' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The type in a hover text for the variable `name`: `List<String> other - demo.A.m(…)` (jdtls),
/// `let other: string` (TypeScript).
pub fn type_from_hover(hover: &str, name: &str) -> Option<String> {
    let line = hover
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```"))?;
    let line = line.split(" - ").next()?.trim();
    if let Some(at) = line
        .find(&format!("{name}:"))
        .or_else(|| line.find(&format!("{name}?:")))
    {
        let rest = &line[at + name.len()..];
        let ty = rest.trim_start_matches('?').trim_start_matches(':').trim();
        return (!ty.is_empty()).then(|| ty.to_owned());
    }
    let (ty, last) = split_last_space(line)?;
    let last_name = last.rsplit('.').next().unwrap_or(last);
    (last_name == name && !ty.is_empty()).then(|| {
        // Modifiers jdtls may show.
        ty.trim_start_matches("final ").trim().to_owned()
    })
}

/// The type of a parameter label: `String e` (Java), `sku: string` (TypeScript), `E... elements`
/// (varargs, the element type).
pub fn parameter_type(label: &str) -> Option<String> {
    let label = label.trim();
    if let Some((name, ty)) = label.split_once(':')
        && name
            .trim()
            .trim_start_matches("...")
            .chars()
            .all(|c| is_word(c) || c == '?')
    {
        let ty = ty.trim().split('=').next()?.trim();
        return (!ty.is_empty()).then(|| ty.to_owned());
    }
    let (ty, _) = split_last_space(label)?;
    let ty = ty.trim_start_matches("final ").trim();
    Some(ty.strip_suffix("...").unwrap_or(ty).trim().to_owned())
}

/// A type name without spaces and package names: `java.util.List<java.lang.String>` →
/// `List<String>`.
pub fn normalize(ty: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        out.push_str(token.rsplit('.').next().unwrap_or(token));
        token.clear();
    };
    for c in ty.chars() {
        if is_word(c) || c == '.' {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            if !c.is_whitespace() {
                out.push(c);
            } else if out.ends_with(|c: char| is_word(c)) {
                // `? extends X` keeps a space between words.
                out.push(' ');
            }
        }
    }
    flush(&mut token, &mut out);
    out.replace(" >", ">").replace("< ", "<").trim().to_owned()
}

/// Raw name and type arguments of a normalized type; `T[]` becomes `Array` with `T`.
fn shape(ty: &str) -> (String, Vec<String>) {
    let ty = ty.trim().trim_start_matches("readonly ").trim();
    if let Some(e) = ty.strip_suffix("[]") {
        return ("Array".into(), vec![e.to_owned()]);
    }
    let Some(open) = ty.find('<') else {
        return (ty.to_owned(), Vec::new());
    };
    let raw = ty[..open].to_owned();
    let inner = &ty[open + 1..ty.rfind('>').unwrap_or(ty.len())];
    let mut args = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in inner.chars() {
        match c {
            '<' => {
                depth += 1;
                cur.push(c)
            }
            '>' => {
                depth -= 1;
                cur.push(c)
            }
            ',' if depth == 0 => args.push(std::mem::take(&mut cur).trim().to_owned()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        args.push(cur.trim().to_owned());
    }
    (raw, args)
}

/// Whether `ty` is a type variable or wildcard (`E`, `T`, `K2`, `?`, `? extends X`).
fn is_type_variable(ty: &str) -> bool {
    let ty = ty.trim();
    ty.starts_with('?')
        || (ty.len() <= 2
            && ty.starts_with(|c: char| c.is_ascii_uppercase())
            && ty.chars().skip(1).all(|c| c.is_ascii_digit()))
}

const PRIMITIVES: &[(&str, &str)] = &[
    ("byte", "Byte"),
    ("short", "Short"),
    ("char", "Character"),
    ("int", "Integer"),
    ("long", "Long"),
    ("float", "Float"),
    ("double", "Double"),
    ("boolean", "Boolean"),
];

/// Java primitive widening.
fn widens(from: &str, to: &str) -> bool {
    let order = ["byte", "short", "int", "long", "float", "double"];
    let pos = |t: &str| order.iter().position(|o| *o == t);
    match (from, to) {
        ("char", t) => matches!(t, "int" | "long" | "float" | "double"),
        (f, t) => matches!((pos(f), pos(t)), (Some(a), Some(b)) if a < b),
    }
}

/// Supertypes of common JDK types (raw names), each list closed under its own entries.
const JDK_SUPERS: &[(&str, &[&str])] = &[
    ("String", &["CharSequence", "Comparable", "Serializable"]),
    ("StringBuilder", &["CharSequence", "Appendable"]),
    ("StringBuffer", &["CharSequence", "Appendable"]),
    ("Integer", &["Number", "Comparable", "Serializable"]),
    ("Long", &["Number", "Comparable", "Serializable"]),
    ("Short", &["Number", "Comparable", "Serializable"]),
    ("Byte", &["Number", "Comparable", "Serializable"]),
    ("Double", &["Number", "Comparable", "Serializable"]),
    ("Float", &["Number", "Comparable", "Serializable"]),
    ("BigDecimal", &["Number", "Comparable"]),
    ("BigInteger", &["Number", "Comparable"]),
    ("Collection", &["Iterable"]),
    ("List", &["Collection", "Iterable", "SequencedCollection"]),
    ("Set", &["Collection", "Iterable"]),
    ("SortedSet", &["Set", "Collection", "Iterable"]),
    (
        "NavigableSet",
        &["SortedSet", "Set", "Collection", "Iterable"],
    ),
    ("Queue", &["Collection", "Iterable"]),
    ("Deque", &["Queue", "Collection", "Iterable"]),
    (
        "ArrayList",
        &["List", "Collection", "Iterable", "RandomAccess"],
    ),
    (
        "LinkedList",
        &["List", "Deque", "Queue", "Collection", "Iterable"],
    ),
    ("Vector", &["List", "Collection", "Iterable"]),
    ("CopyOnWriteArrayList", &["List", "Collection", "Iterable"]),
    ("HashSet", &["Set", "Collection", "Iterable"]),
    ("LinkedHashSet", &["Set", "Collection", "Iterable"]),
    (
        "TreeSet",
        &["NavigableSet", "SortedSet", "Set", "Collection", "Iterable"],
    ),
    ("ArrayDeque", &["Deque", "Queue", "Collection", "Iterable"]),
    ("PriorityQueue", &["Queue", "Collection", "Iterable"]),
    ("SortedMap", &["Map"]),
    ("NavigableMap", &["SortedMap", "Map"]),
    ("HashMap", &["Map"]),
    ("LinkedHashMap", &["HashMap", "Map"]),
    ("TreeMap", &["NavigableMap", "SortedMap", "Map"]),
    ("ConcurrentHashMap", &["ConcurrentMap", "Map"]),
    ("EnumMap", &["Map"]),
    ("Hashtable", &["Map"]),
    ("Exception", &["Throwable"]),
    ("Error", &["Throwable"]),
    ("RuntimeException", &["Exception", "Throwable"]),
    ("IOException", &["Exception", "Throwable"]),
    (
        "IllegalArgumentException",
        &["RuntimeException", "Exception", "Throwable"],
    ),
    (
        "IllegalStateException",
        &["RuntimeException", "Exception", "Throwable"],
    ),
    (
        "NullPointerException",
        &["RuntimeException", "Exception", "Throwable"],
    ),
    (
        "UnsupportedOperationException",
        &["RuntimeException", "Exception", "Throwable"],
    ),
    ("Path", &["Comparable", "Iterable"]),
    ("Optional", &[]),
];

/// The `extends`/`implements` clauses of the types declared in `text` (Java, TypeScript):
/// raw type name → direct supertypes (raw names).
pub fn declared_supertypes(text: &str) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == '{')
        .filter(|w| !w.is_empty())
        .collect();
    let mut i = 0;
    while i + 1 < words.len() {
        if matches!(words[i], "class" | "interface" | "record" | "enum") {
            let name = words[i + 1]
                .split(['<', '('])
                .next()
                .unwrap_or_default()
                .to_owned();
            let mut supers = Vec::new();
            let mut j = i + 2;
            let mut in_clause = false;
            // Skip type parameters and a record's components up to the clauses.
            while j < words.len() && j < i + 40 {
                let w = words[j];
                if matches!(w, "extends" | "implements") {
                    in_clause = true;
                } else if in_clause {
                    for part in w.split(',') {
                        let raw = normalize(part.split('<').next().unwrap_or_default());
                        if !raw.is_empty() && raw.chars().all(is_word) {
                            supers.push(raw);
                        }
                    }
                    if !w.ends_with(',') && !words.get(j + 1).is_some_and(|n| n.starts_with(',')) {
                        // The clause ends unless another keyword follows.
                        if !words
                            .get(j + 1)
                            .is_some_and(|n| matches!(*n, "extends" | "implements"))
                        {
                            break;
                        }
                        in_clause = false;
                    }
                } else if w.contains(';') || w.contains('}') || w == "class" {
                    break;
                }
                j += 1;
            }
            if !name.is_empty() && !supers.is_empty() {
                out.entry(name).or_default().extend(supers);
            }
        }
        i += 1;
    }
    out
}

/// All supertypes (raw names) of raw type `raw`.
fn supertypes(raw: &str, declared: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut todo = vec![raw.to_owned()];
    while let Some(t) = todo.pop() {
        let direct: Vec<String> = declared
            .get(&t)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(
                JDK_SUPERS
                    .iter()
                    .find(|(n, _)| *n == t)
                    .map(|(_, s)| s.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
                    .unwrap_or_default(),
            )
            .collect();
        for s in direct {
            if !out.contains(&s) && s != raw {
                out.push(s.clone());
                todo.push(s);
            }
        }
    }
    out
}

/// How well a value of type `actual` fits where `expected` is wanted (behaviour 26): `Some(0)` the
/// same type, `Some(1)` assignable, `None` not (or not known to be).
pub fn fits(expected: &str, actual: &str, declared: &HashMap<String, Vec<String>>) -> Option<u8> {
    let (e, a) = (normalize(expected), normalize(actual));
    if e.is_empty() || a.is_empty() || a == "void" || e == "void" {
        return None;
    }
    if e == a {
        return Some(0);
    }
    // Unions: any member of the expected union; every member of the actual one.
    if top_level_split(&e, '|').len() > 1 {
        return top_level_split(&e, '|')
            .iter()
            .filter_map(|m| fits(m, &a, declared))
            .min()
            .map(|r| r.max(1));
    }
    if top_level_split(&a, '|').len() > 1 {
        let parts = top_level_split(&a, '|');
        let ranks: Vec<Option<u8>> = parts.iter().map(|m| fits(&e, m, declared)).collect();
        return ranks.iter().all(Option::is_some).then_some(1);
    }
    if matches!(e.as_str(), "any" | "unknown") || is_type_variable(&e) || is_type_variable(&a) {
        return Some(1);
    }
    if a == "any" {
        return Some(1);
    }
    let primitive = |t: &str| PRIMITIVES.iter().find(|(p, b)| *p == t || *b == t).copied();
    match (primitive(&e), primitive(&a)) {
        (None, Some((pa, _))) if a != pa => {
            // A box into a reference type: as any other reference type below.
        }
        (Some((pe, be)), Some((pa, _))) => {
            // Boxing/unboxing of the same kind, widening between primitives.
            if pe == pa || (e == pe && a == pa && widens(pa, pe)) {
                return Some(1);
            }
            return (e == pe && widens(pa, pe) && a != be).then_some(1);
        }
        (None, Some((_, boxed))) => {
            // A primitive into a reference type: through its box.
            return fits(&e, boxed, declared).map(|r| r.max(1));
        }
        (Some(_), None) => return None,
        (None, None) => {}
    }
    if e == "Object" || e == "object" {
        return Some(1);
    }
    // TypeScript literals.
    if (e == "string" && a.starts_with(['"', '\'', '`']))
        || (e == "number" && a.parse::<f64>().is_ok())
        || (e == "boolean" && matches!(a.as_str(), "true" | "false"))
    {
        return Some(1);
    }
    let (er, ea) = shape(&e);
    let (ar, aa) = shape(&a);
    let args_fit = |ea: &[String], aa: &[String]| {
        ea.is_empty()
            || aa.is_empty()
            || (ea.len() == aa.len()
                && ea.iter().zip(aa).all(|(x, y)| {
                    is_type_variable(x) || is_type_variable(y) || normalize(x) == normalize(y)
                }))
    };
    if er == ar {
        return args_fit(&ea, &aa).then_some(if ea == aa { 0 } else { 1 });
    }
    if supertypes(&ar, declared).contains(&er) {
        return args_fit(&ea, &aa).then_some(1);
    }
    None
}

fn top_level_split(text: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        if c == sep && depth == 0 {
            out.push(std::mem::take(&mut cur).trim().to_owned());
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().to_owned());
    out.retain(|p| !p.is_empty());
    out
}

/// The type of a completion item, if it is a value (behaviour 26): its label description, else
/// what follows the last ` : ` of its detail, else the type in a resolved TypeScript detail.
pub fn item_type(item: &CompletionItem) -> Option<String> {
    let kind = item.kind.as_deref().unwrap_or_default();
    if matches!(
        kind,
        "class"
            | "interface"
            | "enum"
            | "module"
            | "keyword"
            | "snippet"
            | "struct"
            | "type-parameter"
            | "text"
            | "file"
            | "folder"
    ) {
        return None;
    }
    if let Some(d) = item.description.as_deref().filter(|d| !d.contains('/')) {
        return Some(d.to_owned());
    }
    let detail = item.detail.as_deref()?;
    if let Some((_, ty)) = detail.rsplit_once(" : ") {
        return Some(ty.trim().to_owned());
    }
    crate::script_completion::detail_parts(&item.label, detail).1
}

/// Keeps the items of `items` that fit one of `expected`, best first (same type, then assignable,
/// each in the server's ranking); `true`/`false` and `null` are kept where they fit. The order is
/// written into the sort text.
pub fn filter_fitting(
    items: Vec<CompletionItem>,
    expected: &[String],
    declared: &HashMap<String, Vec<String>>,
    rank_offset: u8,
) -> Vec<CompletionItem> {
    let boolean = expected.iter().any(|e| {
        matches!(
            normalize(e).as_str(),
            "boolean" | "Boolean" | "Object" | "any" | "unknown"
        )
    });
    let reference = expected
        .iter()
        .any(|e| !PRIMITIVES.iter().any(|(p, _)| *p == normalize(e)));
    let mut kept: Vec<(u8, CompletionItem)> = items
        .into_iter()
        .filter_map(|item| {
            let label = item.label.trim();
            let rank = if item.kind.as_deref() == Some("keyword") || item.kind.is_none() {
                match label {
                    "true" | "false" if boolean => Some(1),
                    "null" if reference => Some(1),
                    _ => None,
                }
            } else {
                let ty = item_type(&item)?;
                expected.iter().filter_map(|e| fits(e, &ty, declared)).min()
            }?;
            Some((rank + rank_offset, item))
        })
        .collect();
    kept.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.preselect.cmp(&b.1.preselect).reverse())
            .then_with(|| a.1.sort_text.cmp(&b.1.sort_text))
    });
    kept.into_iter()
        .map(|(rank, mut item)| {
            item.sort_text = Some(format!(
                "{rank}|{}",
                item.sort_text.as_deref().unwrap_or("~")
            ));
            item.preselect = false;
            item
        })
        .collect()
}

/// What `smart_complete_in` found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SmartCompletion {
    /// The expected types (several for overloads), as written in the source or by the server.
    pub expected: Vec<String>,
    /// Where they came from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// The fitting items, best first.
    pub items: Vec<CompletionItem>,
}

/// The enclosing method's return type at (line, character) from document symbols (jdtls: detail
/// ` : Type`).
fn return_type(symbols: &[insight::Symbol], line: u64, character: u64) -> Option<String> {
    symbols
        .iter()
        .filter(|s| matches!(s.kind.as_str(), "method" | "function") && s.contains(line, character))
        .max_by_key(|s| s.depth)
        .and_then(|s| s.detail.as_deref())
        .and_then(|d| d.trim().strip_prefix(':'))
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty() && t != "void")
}

/// The expected types at a position of `file` whose content is `text` (behaviour 25).
pub async fn expected_types_in(
    root: &Path,
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<(Vec<String>, Option<Origin>), LspError> {
    let line_text = text
        .split('\n')
        .nth(line as usize)
        .unwrap_or_default()
        .trim_end_matches('\r');
    let Some(context) = context_at(line_text, character as usize) else {
        return Ok((Vec::new(), None));
    };
    Ok(match context {
        Context::Known(ty, origin) => (vec![ty], Some(origin)),
        Context::Assignment(at) => {
            let name: String = line_text
                .chars()
                .skip(at)
                .take_while(|c| is_word(*c))
                .collect();
            let hover = insight::hover_in(root, file, Some(text), line, at as u64, wait).await?;
            let ty = hover.and_then(|h| type_from_hover(&h.text, &name));
            (ty.into_iter().collect(), Some(Origin::Assignment))
        }
        Context::Argument => {
            let help =
                insight::signature_help_in(root, file, Some(text), line, character, wait).await?;
            let mut types: Vec<String> = Vec::new();
            if let Some(help) = help {
                let index = help
                    .signatures
                    .get(help.active_signature)
                    .and_then(|s| s.active_parameter)
                    .unwrap_or(0);
                for s in &help.signatures {
                    let Some(range) = s.parameters.get(index) else {
                        continue;
                    };
                    let label: String = s
                        .label
                        .chars()
                        .skip(range.start)
                        .take(range.end - range.start)
                        .collect();
                    if let Some(ty) = parameter_type(&label)
                        && !types.contains(&ty)
                    {
                        types.push(ty);
                    }
                }
            }
            (types, Some(Origin::Argument))
        }
        Context::Return => {
            let symbols = insight::symbols_in(root, file, Some(text), wait).await?;
            let ty = return_type(&symbols, line, character);
            (ty.into_iter().collect(), Some(Origin::Return))
        }
    })
}

/// Smart type completion at a position of `file` whose content is `text` (behaviours 25–26).
pub async fn smart_complete_in(
    root: &Path,
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
    limit: usize,
    wait: Duration,
) -> Result<SmartCompletion, LspError> {
    let (expected, origin) = expected_types_in(root, file, text, line, character, wait).await?;
    if expected.is_empty() {
        return Ok(SmartCompletion {
            origin,
            ..Default::default()
        });
    }
    let language = LanguageId::detect(file);
    let declared = declared_supertypes(text);
    let mut items =
        insight::complete_in(root, file, Some(text), line, character, 5000, wait).await?;
    // TypeScript and other servers give the type only on resolve.
    let untyped: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| {
            item_type(i).is_none() && i.raw.is_some() && i.kind.as_deref() != Some("keyword")
        })
        .map(|(n, _)| n)
        .take(100)
        .collect();
    if !untyped.is_empty() && language != Some(LanguageId::Java) {
        let asked: Vec<CompletionItem> = untyped.iter().map(|&n| items[n].clone()).collect();
        let resolved = insight::resolve_details_in(root, file, Some(text), &asked, wait).await?;
        for (n, r) in untyped.iter().zip(resolved) {
            if let Some(r) = r {
                items[*n].detail = r.detail.or(items[*n].detail.take());
            }
        }
    }
    let mut fitting = filter_fitting(items, &expected, &declared, 0);
    // Java: static members of the expected class with the class name (`List.of`).
    if language == Some(LanguageId::Java) {
        let mut statics = Vec::new();
        for ty in &expected {
            let (raw, _) = shape(&normalize(ty));
            if raw.is_empty()
                || !raw.starts_with(|c: char| c.is_ascii_uppercase())
                || is_type_variable(&raw)
                || PRIMITIVES.iter().any(|(_, b)| *b == raw)
                || raw == "String"
                || raw == "Object"
            {
                continue;
            }
            let line_text = text.split('\n').nth(line as usize).unwrap_or_default();
            let chars: Vec<char> = line_text.chars().collect();
            let mut start = (character as usize).min(chars.len());
            while start > 0 && is_word(chars[start - 1]) {
                start -= 1;
            }
            let offset: usize = text
                .split('\n')
                .take(line as usize)
                .map(|l| l.chars().count() + 1)
                .sum::<usize>()
                + start;
            let qualifier = format!("{raw}.");
            let asked: String = text
                .chars()
                .take(offset)
                .chain(qualifier.chars())
                .chain(text.chars().skip(offset))
                .collect();
            let members = insight::complete_in(
                root,
                file,
                Some(&asked),
                line,
                character + qualifier.chars().count() as u64,
                500,
                wait,
            )
            .await
            .unwrap_or_default();
            let members: Vec<CompletionItem> = members
                .into_iter()
                .filter(|m| {
                    matches!(
                        m.kind.as_deref(),
                        Some("method" | "field" | "enum-member" | "constant")
                    )
                })
                .filter(|m| m.label != "class")
                .map(|mut m| {
                    m.label = format!("{qualifier}{}", m.label);
                    m.insert = format!("{qualifier}{}", m.insert);
                    m.edit_range = None;
                    m.additional_edits.clear();
                    m.raw = None;
                    m
                })
                .collect();
            statics.extend(filter_fitting(members, &expected, &declared, 2));
        }
        fitting.extend(statics);
    }
    fitting.truncate(limit);
    Ok(SmartCompletion {
        expected,
        origin,
        items: fitting,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0072 v8 AK-17: the expected type from the text before the caret.
    #[test]
    fn expected_type_from_context() {
        let known = |t: &str| Some(Context::Known(t.into(), Origin::Declaration));
        assert_eq!(
            context_at("        List<String> l = ", 25),
            known("List<String>")
        );
        assert_eq!(
            context_at("    final Map<String, Integer> m = co", 37),
            known("Map<String, Integer>")
        );
        assert_eq!(context_at("  const n: number = it", 22), known("number"));
        assert_eq!(
            context_at("  private static int count = ", 29),
            known("int")
        );
        assert_eq!(
            context_at("        other = ", 16),
            Some(Context::Assignment(8))
        );
        assert_eq!(
            context_at("        this.total = x", 22),
            Some(Context::Assignment(13))
        );
        assert_eq!(context_at("        var x = ", 16), None);
        assert_eq!(
            context_at("        names.add(", 18),
            Some(Context::Argument)
        );
        assert_eq!(
            context_at("        f(a, g(b), ", 19),
            Some(Context::Argument)
        );
        assert_eq!(context_at("        return ", 15), Some(Context::Return));
        assert_eq!(
            context_at("        if (", 12),
            Some(Context::Known("boolean".into(), Origin::Condition))
        );
        assert_eq!(
            context_at("        if (a == ", 17),
            Some(Context::Known("boolean".into(), Origin::Condition))
        );
        assert_eq!(context_at("        x += ", 13), None);
        assert_eq!(context_at("        foo", 11), None);
    }

    #[test]
    fn types_from_hover_and_parameters() {
        assert_eq!(
            type_from_hover(
                "List<String> other - demo.A.m(List<String>, String)\n\nSource: x",
                "other"
            )
            .as_deref(),
            Some("List<String>")
        );
        assert_eq!(
            type_from_hover("String A.name", "name").as_deref(),
            Some("String")
        );
        assert_eq!(
            type_from_hover("```typescript\nlet total: number\n```", "total").as_deref(),
            Some("number")
        );
        assert_eq!(parameter_type("String e").as_deref(), Some("String"));
        assert_eq!(parameter_type("Map<K, V> m").as_deref(), Some("Map<K, V>"));
        assert_eq!(parameter_type("E... elements").as_deref(), Some("E"));
        assert_eq!(parameter_type("sku: string").as_deref(), Some("string"));
        assert_eq!(parameter_type("n?: number = 1").as_deref(), Some("number"));
    }

    /// SPEC-0072 v8 AK-17: assignability from type names.
    #[test]
    fn assignability() {
        let none = HashMap::new();
        assert_eq!(
            fits("List<String>", "List<java.lang.String>", &none),
            Some(0)
        );
        assert_eq!(fits("List<String>", "List<E>", &none), Some(1));
        assert_eq!(fits("List<String>", "ArrayList<String>", &none), Some(1));
        assert_eq!(fits("List<String>", "ArrayList<Integer>", &none), None);
        assert_eq!(fits("List<String>", "String", &none), None);
        assert_eq!(fits("Collection<String>", "List<String>", &none), Some(1));
        assert_eq!(fits("CharSequence", "String", &none), Some(1));
        assert_eq!(fits("long", "int", &none), Some(1));
        assert_eq!(fits("int", "long", &none), None);
        assert_eq!(fits("int", "Integer", &none), Some(1));
        assert_eq!(fits("Integer", "int", &none), Some(1));
        assert_eq!(fits("Number", "int", &none), Some(1));
        assert_eq!(fits("Object", "Item", &none), Some(1));
        assert_eq!(fits("boolean", "void", &none), None);
        assert_eq!(fits("String", "boolean", &none), None);
        assert_eq!(fits("string | undefined", "string", &none), Some(1));
        assert_eq!(fits("number", "string | number", &none), None);
        assert_eq!(fits("Item[]", "Array<Item>", &none), Some(0));
        assert_eq!(fits("any", "Item", &none), Some(1));
        let declared = declared_supertypes(
            "class Animal {}\nclass Dog extends Animal implements Pet, Comparable<Dog> {\n}\ninterface Pet {}\n",
        );
        assert_eq!(declared["Dog"], ["Animal", "Pet", "Comparable"]);
        assert_eq!(fits("Animal", "Dog", &declared), Some(1));
        assert_eq!(fits("Pet", "Dog", &declared), Some(1));
        assert_eq!(fits("Dog", "Animal", &declared), None);
    }

    /// SPEC-0072 v8 AK-17: fitting items first, the rest left out.
    #[test]
    fn keeps_fitting_items() {
        let item = |label: &str, kind: &str, description: Option<&str>, detail: Option<&str>| {
            CompletionItem {
                label: label.into(),
                kind: Some(kind.into()),
                description: description.map(str::to_owned),
                detail: detail.map(str::to_owned),
                sort_text: Some("5".into()),
                ..Default::default()
            }
        };
        let items = vec![
            item(
                "List<java.lang.String>",
                "interface",
                Some("java.util"),
                None,
            ),
            item("s", "variable", Some("String"), None),
            item("names", "variable", Some("List<java.lang.String>"), None),
            item(
                "copy",
                "method",
                Some("ArrayList<String>"),
                Some("A.copy() : ArrayList<String>"),
            ),
            item("null", "keyword", None, None),
            item("hashCode", "method", Some("int"), None),
        ];
        let kept = filter_fitting(items, &["List<String>".into()], &HashMap::new(), 0);
        let labels: Vec<&str> = kept.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["names", "copy", "null"]);
        assert_eq!(kept[0].sort_text.as_deref(), Some("0|5"));
        assert_eq!(
            item_type(&item(
                "qty",
                "field",
                None,
                Some("(property) Item.qty: number")
            ))
            .as_deref(),
            Some("number")
        );
    }
}
