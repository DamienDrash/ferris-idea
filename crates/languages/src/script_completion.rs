//! TypeScript and JavaScript completion on top of typescript-language-server (`SPEC-0072` v8
//! behaviours 28–30): signature and type from a resolved detail, call parentheses, and Ferris IDEA's
//! own postfix templates (the server has none).

use crate::insight::{CompletionItem, Range};

/// Signature and type of a resolved detail for the item `label`: `(property) Item.qty: number` →
/// (`None`, `number`); `function bySku<T>(items: T[]): Map<string, T>` → (`(items: T[])`,
/// `Map<string, T>`). Lines such as `Auto import from './x'` are skipped (behaviour 28).
pub fn detail_parts(label: &str, detail: &str) -> (Option<String>, Option<String>) {
    let label = label.trim().trim_end_matches('?');
    for line in detail.lines() {
        let chars: Vec<char> = line.chars().collect();
        let name: Vec<char> = label.chars().collect();
        if name.is_empty() {
            break;
        }
        let mut at = 0;
        while at + name.len() <= chars.len() {
            let whole = chars[at..at + name.len()] == name[..]
                && (at == 0 || !is_word(chars[at - 1]))
                && chars.get(at + name.len()).is_none_or(|c| !is_word(*c));
            if whole {
                let rest: String = chars[at + name.len()..].iter().collect();
                if let Some(parts) = after_name(&rest) {
                    return parts;
                }
            }
            at += 1;
        }
    }
    (None, None)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// What follows a name in a declaration: type parameters, a parameter list, `: type`.
fn after_name(rest: &str) -> Option<(Option<String>, Option<String>)> {
    let mut rest = rest.trim_start_matches('?');
    if rest.starts_with('<') {
        rest = &rest[balanced(rest, '<', '>')?..];
    }
    let mut signature = None;
    if rest.starts_with('(') {
        let end = balanced(rest, '(', ')')?;
        signature = Some(rest[..end].to_owned());
        rest = &rest[end..];
    }
    let ty = rest
        .trim_start()
        .strip_prefix(':')
        .or_else(|| rest.trim_start().strip_prefix("=>"))
        // Python: `def f(…) -> T` (`SPEC-0072` v9 behaviour 34).
        .or_else(|| rest.trim_start().strip_prefix("->"))
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());
    (signature.is_some() || ty.is_some()).then_some((signature, ty))
}

/// The byte index after the bracket that closes the one `text` starts with.
fn balanced(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i + c.len_utf8());
            }
        }
    }
    None
}

/// Whether the identifier starting at char `start` of `line` is inside the braces of an
/// `import`/`export` list (`import { by| } from …`).
fn in_import_list(line: &str, start: usize) -> bool {
    let before: String = line.chars().take(start).collect();
    let t = before.trim_start();
    (t.starts_with("import") || t.starts_with("export"))
        && before
            .rfind('{')
            .is_some_and(|o| before.rfind('}').is_none_or(|c| c < o))
}

/// Gives functions and methods of a TypeScript/JavaScript list their parentheses (behaviour 29):
/// the inserted text becomes the snippet `name($0)` unless the text after the identifier at
/// `character` of `line` already starts with `(` or the name is completed in an import list.
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
    if after.trim_start().starts_with('(') || in_import_list(line, start) {
        return;
    }
    for item in items {
        if !matches!(item.kind.as_deref(), Some("function" | "method")) || item.snippet {
            continue;
        }
        if item.insert.contains('(') {
            continue;
        }
        let escaped = item
            .insert
            .replace('\\', "\\\\")
            .replace('$', "\\$")
            .replace('}', "\\}");
        item.insert = format!("{escaped}($0)");
        item.snippet = true;
    }
}

/// Where the expression before the `.` at char `dot` of `line` starts: a member-access chain of
/// names, `this`, calls, index brackets, `?.` and `!` (behaviour 30).
pub fn expression_start(line: &str, dot: usize) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    if chars.get(dot) != Some(&'.') {
        return None;
    }
    let mut at = dot;
    loop {
        // `?.` and `!` before the dot.
        if at > 0 && chars[at - 1] == '?' {
            at -= 1;
        }
        while at > 0 && chars[at - 1] == '!' {
            at -= 1;
        }
        // Calls and index brackets.
        while at > 0 && matches!(chars[at - 1], ')' | ']') {
            let close = chars[at - 1];
            let open = if close == ')' { '(' } else { '[' };
            let mut depth = 0usize;
            let mut k = at;
            loop {
                if k == 0 {
                    return None;
                }
                k -= 1;
                if chars[k] == close {
                    depth += 1;
                } else if chars[k] == open {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            at = k;
        }
        let word_end = at;
        while at > 0 && is_word(chars[at - 1]) {
            at -= 1;
        }
        if at == word_end {
            // A call or bracket without a name before it, e.g. `(a + b).`: the parentheses
            // themselves are the expression.
            if at < dot && matches!(chars[at], '(' | '[') {
                return Some(at);
            }
            return None;
        }
        if chars[at].is_ascii_digit() {
            return None;
        }
        if at > 0 && chars[at - 1] == '.' {
            // `a.b` and `a?.b`: go on before the dot (`?` is taken at the top of the loop).
            at -= 1;
            continue;
        }
        return Some(at);
    }
}

/// One postfix template: name, description, snippet with `{x}` for the expression.
const POSTFIX: &[(&str, &str, &str)] = &[
    ("const", "const name = expr", "const ${1:{name}} = {x};$0"),
    ("let", "let name = expr", "let ${1:{name}} = {x};$0"),
    ("var", "var name = expr", "var ${1:{name}} = {x};$0"),
    ("if", "if (expr)", "if ({x}) {\n\t$0\n}"),
    ("else", "if (!expr)", "if (!{x}) {\n\t$0\n}"),
    ("not", "!expr", "!{x}$0"),
    ("null", "if (expr === null)", "if ({x} === null) {\n\t$0\n}"),
    (
        "notnull",
        "if (expr !== null && expr !== undefined)",
        "if ({x} !== null && {x} !== undefined) {\n\t$0\n}",
    ),
    ("return", "return expr", "return {x};$0"),
    (
        "for",
        "for (const item of expr)",
        "for (const ${1:{element}} of {x}) {\n\t$0\n}",
    ),
    (
        "forin",
        "for (const key in expr)",
        "for (const ${1:key} in {x}) {\n\t$0\n}",
    ),
    (
        "fori",
        "for (let i = 0; i < expr.length; i++)",
        "for (let ${1:i} = 0; ${1:i} < {x}.length; ${1:i}++) {\n\t$0\n}",
    ),
    ("log", "console.log(expr)", "console.log({x});$0"),
    ("par", "(expr)", "({x})$0"),
    ("await", "await expr", "await {x}$0"),
    ("typeof", "typeof expr", "typeof {x}$0"),
];

/// Escapes text for an LSP snippet.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('$', "\\$")
        .replace('}', "\\}")
}

/// Ferris IDEA's postfix templates for the name typed at char `character` of line `line_no` in
/// `line` (behaviour 30): those whose name starts with the typed name, each replacing expression
/// and name. Empty when the name does not follow `expression.`.
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
    let Some(expr_start) = expression_start(line, start - 1) else {
        return Vec::new();
    };
    let expression: String = chars[expr_start..start - 1].iter().collect();
    // `this.` alone or a number is no value to wrap.
    if expression.is_empty() || expression == "this" {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0072 v8 AK-19: signature and type from resolved details.
    #[test]
    fn signature_and_type_from_resolved_details() {
        assert_eq!(
            detail_parts("qty", "(property) Item.qty: number"),
            (None, Some("number".into()))
        );
        assert_eq!(
            detail_parts(
                "bySku",
                "Auto import from './inventory'\nfunction bySku<T extends { sku: string; }>(items: T[]): Map<string, T>"
            ),
            (Some("(items: T[])".into()), Some("Map<string, T>".into()))
        );
        assert_eq!(
            detail_parts("log", "(method) Console.log(...data: any[]): void"),
            (Some("(...data: any[])".into()), Some("void".into()))
        );
        assert_eq!(
            detail_parts("name?", "(property) Item.name?: string"),
            (None, Some("string".into()))
        );
        assert_eq!(detail_parts("Item", "interface Item"), (None, None));
        // `items` inside `itemsCount` is not the name.
        assert_eq!(
            detail_parts("items", "const itemsCount: number\nconst items: Item[]"),
            (None, Some("Item[]".into()))
        );
    }

    fn function(label: &str) -> CompletionItem {
        CompletionItem {
            label: label.into(),
            insert: label.into(),
            kind: Some("function".into()),
            ..Default::default()
        }
    }

    /// SPEC-0072 v8 AK-19: functions get parentheses, except before `(` and in import lists.
    #[test]
    fn functions_get_parentheses() {
        let mut items = vec![function("bySku"), {
            let mut v = function("items");
            v.kind = Some("variable".into());
            v
        }];
        with_call_parentheses(&mut items, "const m = bys", 13);
        assert_eq!(items[0].insert, "bySku($0)");
        assert!(items[0].snippet);
        assert_eq!(items[1].insert, "items");
        let mut items = vec![function("bySku")];
        with_call_parentheses(&mut items, "const m = bys(items)", 13);
        assert_eq!(items[0].insert, "bySku");
        let mut items = vec![function("bySku")];
        with_call_parentheses(&mut items, "import { Item, bys } from \"./inventory\";", 18);
        assert_eq!(items[0].insert, "bySku");
        let mut items = vec![function("$el")];
        with_call_parentheses(&mut items, "$e", 2);
        assert_eq!(items[0].insert, "\\$el($0)");
    }

    /// SPEC-0072 v8 AK-19: the expression before the dot.
    #[test]
    fn postfix_expression() {
        let at = |line: &str| {
            let dot = line.rfind('.').unwrap();
            expression_start(line, line[..dot].chars().count())
                .map(|s| line.chars().skip(s).take(dot - s).collect::<String>())
        };
        assert_eq!(at("  item.name.").as_deref(), Some("item.name"));
        assert_eq!(
            at("x = this.items[0]?.qty!.").as_deref(),
            Some("this.items[0]?.qty!")
        );
        assert_eq!(
            at("  bySku(items).get(\"A\").").as_deref(),
            Some("bySku(items).get(\"A\")")
        );
        assert_eq!(at("(a + b).").as_deref(), Some("(a + b)"));
        assert_eq!(at("x = 1."), None);
    }

    /// SPEC-0072 v8 AK-19: postfix templates replace expression and name.
    #[test]
    fn postfix_templates() {
        let items = postfix_items("  item.name.con", 4, 15);
        assert_eq!(items.len(), 1);
        let c = &items[0];
        assert_eq!(c.label, "const");
        assert_eq!(c.insert, "const ${1:name} = item.name;$0");
        let r = c.edit_range.clone().unwrap();
        assert_eq!(
            (r.start_line, r.start_character, r.end_character),
            (4, 2, 15)
        );
        let all = postfix_items("items.", 0, 6);
        assert_eq!(all.len(), POSTFIX.len());
        let f = all.iter().find(|i| i.label == "for").unwrap();
        assert_eq!(f.insert, "for (const ${1:item} of items) {\n\t$0\n}");
        assert!(postfix_items("const x = 1", 0, 11).is_empty());
        assert!(postfix_items("this.", 0, 5).is_empty());
    }
}
