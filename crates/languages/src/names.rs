//! Variable name suggestions for a template placeholder (`SPEC-0072` v8 behaviour 32): from the
//! expression a value comes from, then from its type.

/// What the suggested name stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    /// The value of the expression itself (`.var`, `const`).
    Value,
    /// One element of the expression, a collection (`for (… : names)`).
    Element,
}

/// Words that are no variable names in Java or TypeScript.
const KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "function",
    "goto",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "int",
    "interface",
    "let",
    "long",
    "native",
    "new",
    "null",
    "of",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "volatile",
    "while",
    "with",
    "yield",
    "await",
    "delete",
];

/// The singular of a plural English noun in camel case (`names` → `name`, `entries` → `entry`,
/// `boxes` → `box`); `None` when `word` does not look plural.
pub fn singular(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    if lower.len() < 3 || lower.ends_with("ss") || lower.ends_with("us") || lower.ends_with("is") {
        return None;
    }
    if let Some(stem) = word.strip_suffix("ies") {
        return Some(format!("{stem}y"));
    }
    for ending in ["ses", "xes", "zes", "ches", "shes"] {
        if lower.ends_with(ending) {
            return Some(word[..word.len() - 2].to_owned());
        }
    }
    word.strip_suffix('s').map(str::to_owned)
}

/// The plural of a noun (`string` → `strings`, `entry` → `entries`, `box` → `boxes`).
pub fn plural(word: &str) -> String {
    let lower = word.to_lowercase();
    if lower.ends_with('y')
        && !lower.ends_with("ay")
        && !lower.ends_with("ey")
        && !lower.ends_with("oy")
        && !lower.ends_with("uy")
    {
        return format!("{}ies", &word[..word.len() - 1]);
    }
    if ["s", "x", "z", "ch", "sh"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        return format!("{word}es");
    }
    format!("{word}s")
}

fn lower_first(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The last name of an expression: `s.length()` → `length`, `this.items[0]` → `items`,
/// `getName()` → `name`, `isEmpty()` → `empty`.
fn last_name(expression: &str) -> Option<String> {
    let mut text = expression.trim().trim_end_matches(['!', ';']);
    // Drop trailing calls and index brackets.
    loop {
        let t = text.trim_end();
        let (close, open) = match t.chars().last() {
            Some(')') => (')', '('),
            Some(']') => (']', '['),
            _ => break,
        };
        let mut depth = 0usize;
        let mut cut = None;
        for (i, c) in t.char_indices().rev() {
            if c == close {
                depth += 1;
            } else if c == open {
                depth -= 1;
                if depth == 0 {
                    cut = Some(i);
                    break;
                }
            }
        }
        text = &t[..cut?];
    }
    let name: String = text
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || name == "this" {
        return None;
    }
    for prefix in ["get", "is", "has", "to"] {
        if let Some(rest) = name.strip_prefix(prefix)
            && rest.starts_with(|c: char| c.is_uppercase())
        {
            return Some(lower_first(rest));
        }
    }
    Some(name)
}

/// The camel-case words of a type name (`ArrayList` → `arrayList`, `list`).
fn camel_suffixes(name: &str) -> Vec<String> {
    let starts: Vec<usize> = name
        .char_indices()
        .filter(|(i, c)| *i == 0 || c.is_uppercase())
        .map(|(i, _)| i)
        .collect();
    starts.iter().map(|&s| lower_first(&name[s..])).collect()
}

/// The raw name of a type and its type arguments: `java.util.List<String>` → (`List`, [`String`]).
fn split_type(ty: &str) -> (String, Vec<String>) {
    let ty = ty.trim();
    let (raw, args) = match ty.find('<') {
        Some(at) => (&ty[..at], &ty[at + 1..ty.rfind('>').unwrap_or(ty.len())]),
        None => (ty, ""),
    };
    let raw = raw.rsplit('.').next().unwrap_or(raw).trim().to_owned();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in args.chars() {
        match c {
            '<' => {
                depth += 1;
                current.push(c);
            }
            '>' => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut current).trim().to_owned()),
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    (raw, out)
}

/// Names for a value of type `ty` (`int` → `i`, `String` → `s`, `str`, `List<String>` →
/// `strings`, `list`).
fn type_names(ty: &str) -> Vec<String> {
    let ty = ty.trim();
    if let Some(element) = ty.strip_suffix("[]") {
        let mut out: Vec<String> = type_names(element).iter().map(|n| plural(n)).collect();
        out.push("array".into());
        return out;
    }
    let (raw, args) = split_type(ty);
    match raw.as_str() {
        "int" | "Integer" | "short" | "Short" | "byte" | "Byte" | "number" => {
            return vec!["i".into(), "j".into(), "k".into()];
        }
        "long" | "Long" => return vec!["l".into()],
        "double" | "Double" | "float" | "Float" => return vec!["d".into()],
        "boolean" | "Boolean" => return vec!["b".into()],
        "char" | "Character" => return vec!["c".into()],
        "String" | "string" | "CharSequence" => return vec!["s".into(), "str".into()],
        "Object" | "object" | "any" | "unknown" | "" => return vec!["o".into()],
        _ => {}
    }
    let mut out = Vec::new();
    let collection = [
        "List",
        "Set",
        "Collection",
        "Iterable",
        "Array",
        "Queue",
        "Deque",
        "Stream",
    ]
    .iter()
    .any(|c| raw.ends_with(c));
    if collection && let Some(element) = args.first() {
        let (element, _) = split_type(element);
        if element.chars().count() > 1 {
            out.push(plural(&lower_first(&element)));
        }
    }
    if raw.ends_with("Map")
        && let (Some(key), Some(value)) = (args.first(), args.get(1))
    {
        let (k, _) = split_type(key);
        let (v, _) = split_type(value);
        if v.chars().count() > 1 && k.chars().count() > 1 {
            out.push(format!("{}By{k}", plural(&lower_first(&v))));
        }
    }
    out.extend(camel_suffixes(&raw));
    out
}

/// The element type of a collection type (`List<String>` → `String`, `Item[]` → `Item`).
fn element_type(ty: &str) -> Option<String> {
    if let Some(e) = ty.trim().strip_suffix("[]") {
        return Some(e.to_owned());
    }
    split_type(ty).1.into_iter().next()
}

/// Suggested names for a variable holding `expression` (of type `ty`, if known), best first,
/// without keywords and duplicates.
pub fn suggest(kind: NameKind, expression: Option<&str>, ty: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let from_expression = expression.and_then(last_name);
    match kind {
        NameKind::Value => {
            out.extend(from_expression);
            out.extend(ty.map(type_names).unwrap_or_default());
        }
        NameKind::Element => {
            if let Some(name) = from_expression.as_deref() {
                match singular(name) {
                    Some(one) => out.push(one),
                    None => out.push("element".into()),
                }
            }
            if let Some(element) = ty.and_then(element_type) {
                out.extend(type_names(&element));
            }
            out.push("item".into());
        }
    }
    let mut seen = Vec::new();
    out.into_iter()
        .map(|n| lower_first(&n))
        .filter(|n| !n.is_empty() && !KEYWORDS.contains(&n.as_str()))
        .filter(|n| {
            let new = !seen.contains(n);
            seen.push(n.clone());
            new
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0072 v8 AK-20: names from the expression, then from the type.
    #[test]
    fn suggestions_from_expression_and_type() {
        assert_eq!(
            suggest(NameKind::Value, Some("s.length()"), Some("int")),
            ["length", "i", "j", "k"]
        );
        assert_eq!(
            suggest(NameKind::Value, Some("p.getName()"), None),
            ["name"]
        );
        assert_eq!(
            suggest(NameKind::Value, Some("s.isEmpty()"), Some("boolean")),
            ["empty", "b"]
        );
        assert_eq!(
            suggest(NameKind::Value, None, Some("java.util.List<String>")),
            ["strings", "list"]
        );
        assert_eq!(
            suggest(NameKind::Value, None, Some("ArrayList<Item>")),
            ["items", "arrayList", "list"]
        );
        assert_eq!(suggest(NameKind::Value, None, Some("String")), ["s", "str"]);
        assert_eq!(suggest(NameKind::Value, Some("item.name"), None), ["name"]);
        // Elements of a collection.
        assert_eq!(
            suggest(NameKind::Element, Some("names"), Some("List<String>")),
            ["name", "s", "str", "item"]
        );
        assert_eq!(
            suggest(NameKind::Element, Some("this.entries"), None),
            ["entry", "item"]
        );
        assert_eq!(
            suggest(NameKind::Element, Some("data"), None),
            ["element", "item"]
        );
        // Keywords are left out.
        assert_eq!(
            suggest(NameKind::Value, Some("x.getClass()"), None),
            Vec::<String>::new()
        );
    }

    #[test]
    fn singular_and_plural() {
        assert_eq!(singular("names").as_deref(), Some("name"));
        assert_eq!(singular("entries").as_deref(), Some("entry"));
        assert_eq!(singular("boxes").as_deref(), Some("box"));
        assert_eq!(singular("class"), None);
        assert_eq!(singular("status"), None);
        assert_eq!(plural("entry"), "entries");
        assert_eq!(plural("key"), "keys");
        assert_eq!(plural("box"), "boxes");
    }
}
