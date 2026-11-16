//! Extract Variable and Extract Method for Python, computed from the syntax tree (`SPEC-0330`):
//! pyright and basedpyright offer no extract refactoring, so the refactoring shortcuts of
//! `SPEC-0095` fall back to these own actions. Extract Constant is in `python_constant` and Inline
//! Variable in `python_inline`; this module is the entry point of all four ([`find`], [`plan`]). Scoping follows The Python Language Reference,
//! "Naming and binding"; what is refused, and why, is listed in the specification (behaviours 4,
//! 7 and 13). A refusal is preferred to code that could behave differently. The new function's
//! types (behaviour 16) come from the file's own annotations and, where a caller passes them in,
//! from the language server's hover answers.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range as Bytes;
use std::path::Path;

use tree_sitter::Node;

use crate::LanguageId;
use crate::actions::{CodeAction, Refactoring, RefactoringTarget};
use crate::insight::Range;
use crate::rename::Edit;
use crate::syntax::SyntaxDocument;

/// Title of the own Extract Variable action.
pub const EXTRACT_VARIABLE: &str = "Extract variable";
/// Title of the own Extract Method action.
pub const EXTRACT_METHOD: &str = "Extract method";

pub use crate::python_constant::EXTRACT_CONSTANT;
pub use crate::python_inline::INLINE_VARIABLE;

/// How many places a bare caret offers at most (behaviour 3).
pub(crate) const MAX_PLACES: usize = 8;

/// Node kinds that are an expression of their own.
pub(crate) const EXPRESSIONS: &[&str] = &[
    "identifier",
    "binary_operator",
    "boolean_operator",
    "comparison_operator",
    "not_operator",
    "unary_operator",
    "call",
    "attribute",
    "subscript",
    "integer",
    "float",
    "string",
    "concatenated_string",
    "true",
    "false",
    "none",
    "ellipsis",
    "list",
    "dictionary",
    "set",
    "tuple",
    "parenthesized_expression",
    "list_comprehension",
    "dictionary_comprehension",
    "set_comprehension",
    "generator_expression",
    "conditional_expression",
    "lambda",
    "await",
    "named_expression",
    "expression_list",
];

pub(crate) const COMPREHENSIONS: &[&str] = &[
    "list_comprehension",
    "dictionary_comprehension",
    "set_comprehension",
    "generator_expression",
];

pub(crate) const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield", "match", "case", "type",
];

/// Built-in names a new variable should not hide.
const BUILTINS: &[&str] = &[
    "abs",
    "all",
    "any",
    "bool",
    "bytes",
    "callable",
    "chr",
    "dict",
    "dir",
    "enumerate",
    "filter",
    "float",
    "format",
    "hash",
    "id",
    "input",
    "int",
    "iter",
    "len",
    "list",
    "map",
    "max",
    "min",
    "next",
    "object",
    "open",
    "ord",
    "print",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "slice",
    "sorted",
    "str",
    "sum",
    "super",
    "tuple",
    "vars",
    "zip",
];

/// Whether `file` is a Python file and `refactoring` one of the own ones.
pub fn offers(file: &Path, refactoring: Refactoring) -> bool {
    LanguageId::detect(file) == Some(LanguageId::Python)
        && matches!(
            refactoring,
            Refactoring::Variable
                | Refactoring::Method
                | Refactoring::Constant
                | Refactoring::Inline
                | Refactoring::Any
        )
}

/// Whether `title` is one of the own actions of a Python `file`.
pub fn is_own(file: &Path, title: &str) -> bool {
    LanguageId::detect(file) == Some(LanguageId::Python)
        && [
            EXTRACT_VARIABLE,
            EXTRACT_METHOD,
            EXTRACT_CONSTANT,
            INLINE_VARIABLE,
        ]
        .contains(&title)
}

pub(crate) fn children(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children(&mut cursor).collect()
}

/// The named children without comments.
pub(crate) fn named(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect()
}

pub(crate) fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

pub(crate) fn field<'t>(n: Node<'t>, name: &str) -> Option<Node<'t>> {
    n.child_by_field_name(name)
}

/// Whether `c` is the child of `p` in field `name`.
pub(crate) fn is_field(p: Node, name: &str, c: Node) -> bool {
    let mut cursor = p.walk();
    p.children_by_field_name(name, &mut cursor)
        .any(|x| x.id() == c.id())
}

pub(crate) fn byte_of(text: &str, line: u64, character: u64) -> usize {
    let mut at = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i as u64 == line {
            return at
                + l.char_indices()
                    .nth(character as usize)
                    .map_or(l.len(), |(b, _)| b);
        }
        at += l.len() + 1;
    }
    text.len()
}

pub(crate) fn range_of(text: &str, bytes: Bytes<usize>) -> Range {
    let pos = |b: usize| {
        let before = &text[..b];
        let line = before.matches('\n').count() as u64;
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        (line, text[start..b].chars().count() as u64)
    };
    let (a, b) = (pos(bytes.start), pos(bytes.end));
    Range {
        start_line: a.0,
        start_character: a.1,
        end_line: b.0,
        end_character: b.1,
    }
}

pub(crate) fn line_start(text: &str, b: usize) -> usize {
    text[..b].rfind('\n').map_or(0, |i| i + 1)
}

/// The end of the line holding byte `b`, before its line break.
pub(crate) fn line_end(text: &str, b: usize) -> usize {
    let end = text[b..].find('\n').map_or(text.len(), |i| b + i);
    if end > b && text.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

pub(crate) fn eol(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The white space a line starts with.
pub(crate) fn indent_at(text: &str, b: usize) -> &str {
    let start = line_start(text, b);
    let line = &text[start..];
    let len = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    &line[..len]
}

pub(crate) fn edit(text: &str, bytes: Bytes<usize>, new_text: String) -> Edit {
    Edit {
        range: range_of(text, bytes.clone()),
        old_text: text[bytes].to_owned(),
        new_text,
    }
}

/// The selection in bytes, without white space around it.
pub(crate) fn selection(text: &str, range: &Range) -> Bytes<usize> {
    let a = byte_of(text, range.start_line, range.start_character);
    let b = byte_of(text, range.end_line, range.end_character);
    let (a, b) = (a.min(b), a.max(b));
    let part = &text[a..b];
    let lead = part.len() - part.trim_start().len();
    let trimmed = part.trim().len();
    a + lead..a + lead + trimmed
}

pub(crate) fn parse(text: &str) -> Result<SyntaxDocument, String> {
    SyntaxDocument::new(LanguageId::Python, false, text)
        .map_err(|_| "Python syntax is not available".to_owned())
}

pub(crate) fn is_statement(n: Node) -> bool {
    n.parent()
        .is_some_and(|p| matches!(p.kind(), "block" | "module"))
        && !matches!(n.kind(), "comment" | "case_clause")
}

/// The statement holding `n` (or `n` itself).
pub(crate) fn statement_of(n: Node) -> Option<Node> {
    let mut c = n;
    loop {
        if is_statement(c) {
            return Some(c);
        }
        c = c.parent()?;
    }
}

pub(crate) fn top_level_of(n: Node) -> Node {
    let mut c = n;
    while let Some(p) = c.parent() {
        if p.kind() == "module" {
            break;
        }
        c = p;
    }
    c
}

/// Whether `n` contains a node of one of `kinds`, nested functions, lambdas and classes left out.
pub(crate) fn contains_own(n: Node, kinds: &[&str]) -> bool {
    if kinds.contains(&n.kind()) {
        return true;
    }
    children(n).into_iter().any(|c| {
        !matches!(
            c.kind(),
            "function_definition" | "lambda" | "class_definition"
        ) && contains_own(c, kinds)
    })
}

/// The expression that is exactly the bytes `sel`.
pub(crate) fn expression_at<'t>(root: Node<'t>, sel: &Bytes<usize>) -> Result<Node<'t>, String> {
    const PART: &str = "the selection is not one complete expression";
    if sel.is_empty() {
        return Err("nothing is selected".into());
    }
    let mut n = root
        .descendant_for_byte_range(sel.start, sel.end)
        .ok_or(PART)?;
    // A generator expression's parentheses belong to it, also where they are a call's
    // (`sum(x for x in xs)`): selecting what is between them means the generator.
    if n.kind() == "generator_expression"
        && sel.start > 0
        && n.byte_range() == (sel.start - 1..sel.end + 1)
    {
        return Ok(n);
    }
    if n.byte_range() != *sel {
        return Err(PART.into());
    }
    loop {
        if n.is_named() && EXPRESSIONS.contains(&n.kind()) {
            return Ok(n);
        }
        match n.parent() {
            Some(p) if p.byte_range() == *sel => n = p,
            _ => return Err(PART.into()),
        }
    }
}

/// Refuses an expression that stands where no value of its own is computed (behaviour 4).
fn value_place(n: Node) -> Result<(), String> {
    value_place_for(n, false)
}

/// As [`value_place`]; with `defaults` a default value of a parameter is allowed (Extract
/// Constant, behaviour 22: its line stands before the `def` in any case).
pub(crate) fn value_place_for(n: Node, defaults: bool) -> Result<(), String> {
    const TARGET: &str = "it is, or is part of, the target of an assignment, `for`, `with`, `del` \
                          or `except`";
    if contains_own(n, &["yield"]) {
        return Err("it contains `yield`".into());
    }
    if contains_own(n, &["named_expression"]) {
        return Err("it contains an assignment expression (`:=`)".into());
    }
    let mut c = n;
    // Inside an allowed default value: the parameter list and the `def` around it are passed.
    let mut default = false;
    while let Some(p) = c.parent() {
        let reason = match p.kind() {
            "block" | "module" => break,
            "parameters" | "lambda_parameters" if default => "",
            "function_definition" | "lambda" if default => {
                default = false;
                ""
            }
            "attribute" if is_field(p, "attribute", c) => "a name after a dot is no expression",
            "keyword_argument" if is_field(p, "name", c) => {
                "the name of a keyword argument is no expression"
            }
            "named_expression" if is_field(p, "name", c) => TARGET,
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause"
                if is_field(p, "left", c) =>
            {
                TARGET
            }
            "as_pattern_target" | "delete_statement" => TARGET,
            "except_clause" | "except_group_clause" if is_field(p, "alias", c) => TARGET,
            "default_parameter" | "typed_default_parameter" if is_field(p, "value", c) => {
                if defaults {
                    default = true;
                    ""
                } else {
                    "it is in a default value of a parameter, which is evaluated when the \
                     function is defined"
                }
            }
            "parameters"
            | "lambda_parameters"
            | "default_parameter"
            | "typed_default_parameter"
            | "typed_parameter" => "a parameter is no expression",
            "type" => "it is in an annotation",
            "decorator" => "it is in a decorator",
            "import_statement"
            | "import_from_statement"
            | "future_import_statement"
            | "global_statement"
            | "nonlocal_statement"
            | "dotted_name"
            | "aliased_import" => "an imported or declared name is no expression",
            "function_definition" | "class_definition" if !is_field(p, "body", c) => {
                "it is in the header of a `def` or `class`"
            }
            "case_clause" if !is_field(p, "consequence", c) => "it is in a `case` pattern or guard",
            _ => "",
        };
        if !reason.is_empty() {
            return Err(reason.into());
        }
        c = p;
    }
    Ok(())
}

pub(crate) fn syntax_check(n: Node) -> Result<(), String> {
    if top_level_of(n).has_error() {
        return Err("the code around it has syntax errors".into());
    }
    Ok(())
}

/// Whether the statement `s` has its first line to itself up to its start.
pub(crate) fn starts_its_line(text: &str, s: Node) -> bool {
    text[line_start(text, s.start_byte())..s.start_byte()]
        .trim()
        .is_empty()
}

/// Whether nothing but a comment follows byte `b` on its line.
pub(crate) fn ends_its_line(text: &str, b: usize) -> bool {
    let rest = text[b..line_end(text, b)].trim();
    rest.is_empty() || rest.starts_with('#')
}

pub(crate) fn all_identifiers(n: Node, text: &str, attributes: bool, out: &mut HashSet<String>) {
    if n.kind() == "identifier" {
        let skip = !attributes
            && n.parent().is_some_and(|p| {
                (p.kind() == "attribute" && is_field(p, "attribute", n))
                    || (p.kind() == "keyword_argument" && is_field(p, "name", n))
            });
        if !skip {
            out.insert(word(n, text).to_owned());
        }
        return;
    }
    for c in children(n) {
        all_identifiers(c, text, attributes, out);
    }
}

pub(crate) fn snake(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let after_lower = i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_numeric());
            let before_lower = i > 0
                && chars[i - 1].is_uppercase()
                && chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if after_lower || before_lower {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(*c);
        }
    }
    out
}

/// The name an expression suggests (behaviour 6), before it is made unique.
pub(crate) fn suggested(n: Node, text: &str) -> Option<String> {
    match n.kind() {
        "call" => {
            let f = field(n, "function")?;
            let name = if f.kind() == "identifier" {
                word(f, text).to_owned()
            } else {
                suggested(f, text)?
            };
            Some(
                name.strip_prefix("get_")
                    .filter(|r| !r.is_empty())
                    .unwrap_or(&name)
                    .to_owned(),
            )
        }
        "attribute" => Some(word(field(n, "attribute")?, text).to_owned()),
        "subscript" => suggested(field(n, "value")?, text),
        "binary_operator" | "boolean_operator" => field(n, "right")
            .and_then(|r| suggested(r, text))
            .or_else(|| field(n, "left").and_then(|l| suggested(l, text))),
        "comparison_operator" => {
            let k = named(n);
            k.last()
                .and_then(|r| suggested(*r, text))
                .or_else(|| k.first().and_then(|l| suggested(*l, text)))
        }
        "parenthesized_expression" | "await" | "not_operator" | "unary_operator" => {
            named(n).last().and_then(|c| suggested(*c, text))
        }
        _ => None,
    }
}

pub(crate) fn unique(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_owned();
    }
    (1..)
        .map(|i| format!("{base}{i}"))
        .find(|c| !taken.contains(c))
        .expect("a free name")
}

fn variable_name(n: Node, text: &str, scope: Node) -> String {
    let base = suggested(n, text)
        .map(|s| snake(s.trim_start_matches('_')))
        .filter(|s| {
            s.chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && s.chars().all(|c| c.is_alphanumeric() || c == '_')
        })
        .unwrap_or_else(|| "value".to_owned());
    let base = if KEYWORDS.contains(&base.as_str()) || BUILTINS.contains(&base.as_str()) {
        format!("{base}_")
    } else {
        base
    };
    let mut taken = HashSet::new();
    all_identifiers(scope, text, false, &mut taken);
    unique(&base, &taken)
}

/// The text that replaces the expression `n` by `new`: a generator expression that is a call's
/// only argument keeps the call's parentheses, and a word is kept apart from its neighbours.
pub(crate) fn in_place(n: Node, text: &str, new: &str) -> String {
    let mut out = new.to_owned();
    if n.kind() == "generator_expression"
        && n.parent()
            .is_some_and(|p| p.kind() == "call" && is_field(p, "arguments", n))
    {
        out = format!("({out})");
    }
    let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    if wordy(text[..n.start_byte()].chars().next_back()) && wordy(out.chars().next()) {
        out.insert(0, ' ');
    }
    if wordy(text[n.end_byte()..].chars().next()) && wordy(out.chars().next_back()) {
        out.push(' ');
    }
    out
}

/// The expression's text as the right side of an assignment or of `return`.
pub(crate) fn standalone(n: Node, text: &str) -> String {
    let t = word(n, text);
    if t.contains('\n') && n.kind() != "parenthesized_expression" {
        format!("({t})")
    } else {
        t.to_owned()
    }
}

/// Why the evaluation of `n` cannot move in front of its statement `s` (behaviour 7).
fn movable(n: Node, s: Node, text: &str) -> Result<(), String> {
    evaluated_once(n, s)?;
    if s.parent()
        .and_then(|b| b.parent())
        .is_some_and(|p| p.kind() == "class_definition")
    {
        return Err(
            "the statement is directly in a class body, where the name would become a class \
             attribute"
                .into(),
        );
    }
    if !starts_its_line(text, s) {
        return Err("the statement does not start its line".into());
    }
    // A name bound by `:=` earlier in the same statement.
    let mut reads = HashSet::new();
    all_identifiers(n, text, false, &mut reads);
    let mut stack = vec![s];
    while let Some(x) = stack.pop() {
        if x.kind() == "named_expression"
            && let Some(name) = field(x, "name")
            && reads.contains(word(name, text))
        {
            return Err(format!(
                "it reads `{}`, which `:=` binds in the same statement",
                word(name, text)
            ));
        }
        stack.extend(children(x).into_iter().filter(|c| c.kind() != "block"));
    }
    in_order(n, s, text)
}

/// Why `n` is not evaluated exactly once each time its statement `s` runs (behaviour 7): the
/// places that are evaluated only sometimes, several times or later.
pub(crate) fn evaluated_once(n: Node, s: Node) -> Result<(), String> {
    const SOMETIMES: &str = "is evaluated only sometimes there";
    let mut c = n;
    let mut via_right = false;
    while c.id() != s.id() {
        let p = c.parent().ok_or("it is in no statement")?;
        let reason = match p.kind() {
            "lambda" if is_field(p, "body", c) => {
                "it is in the body of a `lambda`, which is evaluated at each call".into()
            }
            "for_in_clause" => {
                via_right = is_field(p, "right", c);
                String::new()
            }
            k if COMPREHENSIONS.contains(&k) => {
                let first = children(p)
                    .into_iter()
                    .find(|x| x.kind() == "for_in_clause")
                    .is_some_and(|f| f.id() == c.id());
                if first && via_right {
                    String::new()
                } else {
                    "it is inside a comprehension, where it is evaluated for every element".into()
                }
            }
            "boolean_operator" if is_field(p, "right", c) => {
                format!("the right side of `and`/`or` {SOMETIMES}")
            }
            "conditional_expression" => {
                let k = named(p);
                if k.len() == 3 && k[1].id() != c.id() {
                    format!("a branch of a conditional expression {SOMETIMES}")
                } else {
                    String::new()
                }
            }
            "comparison_operator" => {
                if named(p).iter().position(|x| x.id() == c.id()).unwrap_or(0) >= 2 {
                    format!("a later operand of a chained comparison {SOMETIMES}")
                } else {
                    String::new()
                }
            }
            "while_statement" if is_field(p, "condition", c) => {
                "the condition of `while` is evaluated before every round".into()
            }
            "elif_clause" if is_field(p, "condition", c) => {
                "the condition of `elif` is evaluated only when the conditions before it are false"
                    .into()
            }
            "except_clause" | "except_group_clause" => {
                "the expression of `except` is evaluated only after an error".into()
            }
            "assert_statement" => {
                "an `assert` is skipped when Python runs optimised, and its message is evaluated \
                 only on failure"
                    .into()
            }
            "with_clause" => {
                if named(p).first().is_some_and(|f| f.id() != c.id()) {
                    "a later item of `with` is evaluated after the items before it were entered"
                        .into()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        if !reason.is_empty() {
            return Err(reason);
        }
        c = p;
    }
    Ok(())
}

/// The parts of statement `s` that Python evaluates before `n` (behaviour 8): at each step from
/// `n` up to `s`, the siblings evaluated before the step. That is left to right, except that the
/// right side of an assignment comes before its targets and annotation, the iterable of `for` (and
/// of a comprehension's first `for`) before the target and the element, and the condition of a
/// conditional expression before its branches.
pub(crate) fn evaluated_before<'t>(n: Node<'t>, s: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut c = n;
    while c.id() != s.id() {
        let Some(p) = c.parent() else {
            break;
        };
        let source_order = |c: Node<'t>| {
            named(p)
                .into_iter()
                .filter(|x| x.end_byte() <= c.start_byte() && x.kind() != "block")
                .collect::<Vec<_>>()
        };
        let before = match p.kind() {
            "assignment" if is_field(p, "right", c) => Vec::new(),
            "assignment" => field(p, "right").into_iter().collect(),
            "for_statement" | "for_in_clause" if is_field(p, "right", c) => Vec::new(),
            "conditional_expression" if named(p).get(1).is_some_and(|x| x.id() == c.id()) => {
                Vec::new()
            }
            k if COMPREHENSIONS.contains(&k)
                && children(p)
                    .into_iter()
                    .find(|x| x.kind() == "for_in_clause")
                    .is_some_and(|f| f.id() == c.id()) =>
            {
                Vec::new()
            }
            _ => source_order(c),
        };
        out.extend(before);
        c = p;
    }
    out
}

/// The first part of `n`, in source order, that is one of `kinds`; the body of a `lambda` is not
/// looked into, since it does not run where it stands.
pub(crate) fn first_of<'t>(n: Node<'t>, kinds: &[&str]) -> Option<Node<'t>> {
    if kinds.contains(&n.kind()) {
        return Some(n);
    }
    children(n)
        .into_iter()
        .filter(|c| !(n.kind() == "lambda" && is_field(n, "body", *c)))
        .find_map(|c| first_of(c, kinds))
}

/// Behaviour 8: the new line runs before everything else of its statement. Refused when a part
/// evaluated before `n` can have side effects (a call, `await`, `yield` or `:=`; reading an
/// attribute or a subscript does not count) and `n` itself calls, awaits or yields. A name bound
/// by `:=` in the statement and read by `n` is refused before (behaviour 7); `n` itself never
/// holds `:=` (behaviour 4).
fn in_order(n: Node, s: Node, text: &str) -> Result<(), String> {
    const EFFECTS: &[&str] = &["call", "await", "yield", "named_expression"];
    if first_of(n, &["call", "await", "yield"]).is_none() {
        return Ok(());
    }
    let shown = |x: Node| {
        let t = word(x, text);
        let line = t.lines().next().unwrap_or_default();
        if line.chars().count() > 40 || line.len() < t.len() {
            format!("{}…", line.chars().take(40).collect::<String>())
        } else {
            t.to_owned()
        }
    };
    for part in evaluated_before(n, s) {
        if let Some(effect) = first_of(part, EFFECTS) {
            return Err(format!(
                "it would then run before `{}`, which comes first in the statement and can have \
                 side effects",
                shown(effect)
            ));
        }
    }
    Ok(())
}

fn variable_edits(root: Node, text: &str, sel: &Bytes<usize>) -> Result<Vec<Edit>, String> {
    let n = expression_at(root, sel)?;
    syntax_check(n)?;
    value_place(n)?;
    let s = statement_of(n).ok_or("it is in no statement")?;
    movable(n, s, text)?;
    let scope = {
        let mut c = s;
        loop {
            match c.parent() {
                Some(p) if p.kind() == "function_definition" => break p,
                Some(p) => c = p,
                None => break c,
            }
        }
    };
    let name = variable_name(n, text, scope);
    let value = standalone(n, text);
    if s.kind() == "expression_statement" && s.byte_range() == n.byte_range() {
        return Ok(vec![edit(
            text,
            n.byte_range(),
            format!("{name} = {value}"),
        )]);
    }
    let at = line_start(text, s.start_byte());
    let indent = indent_at(text, s.start_byte());
    if at == n.start_byte() {
        // The expression starts the line: one edit, not two at the same place.
        return Ok(vec![edit(
            text,
            n.byte_range(),
            format!("{name} = {value}{}{}", eol(text), in_place(n, text, &name)),
        )]);
    }
    Ok(vec![
        edit(
            text,
            at..at,
            format!("{indent}{name} = {value}{}", eol(text)),
        ),
        edit(text, n.byte_range(), in_place(n, text, &name)),
    ])
}

// ---------------------------------------------------------------------------------------------
// Names and scopes
// ---------------------------------------------------------------------------------------------

/// What the own statements of a scope bind.
#[derive(Default)]
pub(crate) struct Bound {
    /// Name and byte of each binding.
    pub(crate) names: Vec<(String, usize)>,
    /// Names of `global` and `nonlocal` statements.
    pub(crate) outer: HashSet<String>,
    /// Names of `global` statements.
    pub(crate) globals: HashSet<String>,
}

/// The identifiers a target binds.
pub(crate) fn target_names<'t>(n: Node<'t>, out: &mut Vec<Node<'t>>) {
    match n.kind() {
        "identifier" => out.push(n),
        "pattern_list"
        | "tuple_pattern"
        | "list_pattern"
        | "list_splat_pattern"
        | "as_pattern_target"
        | "parenthesized_expression"
        | "tuple"
        | "list"
        | "expression_list"
        | "list_splat" => {
            for c in named(n) {
                target_names(c, out);
            }
        }
        _ => {}
    }
}

/// The identifiers an import statement binds.
pub(crate) fn import_names(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children_by_field_name("name", &mut cursor)
        .filter_map(|c| match c.kind() {
            "dotted_name" => named(c).into_iter().next(),
            "aliased_import" => field(c, "alias"),
            _ => None,
        })
        .collect()
}

fn bound_into(n: Node, text: &str, b: &mut Bound, comprehension: bool) {
    let push = |b: &mut Bound, list: Vec<Node>| {
        for t in list {
            b.names.push((word(t, text).to_owned(), t.start_byte()));
        }
    };
    let mut targets = Vec::new();
    match n.kind() {
        "function_definition" | "class_definition" => {
            push(b, field(n, "name").into_iter().collect());
            return;
        }
        "lambda" => return,
        k if COMPREHENSIONS.contains(&k) => {
            for c in children(n) {
                bound_into(c, text, b, true);
            }
            return;
        }
        "named_expression" => push(b, field(n, "name").into_iter().collect()),
        "assignment" if !comprehension => {
            if let (Some(l), Some(_)) = (field(n, "left"), field(n, "right")) {
                target_names(l, &mut targets);
            }
        }
        "augmented_assignment" | "for_statement" if !comprehension => {
            if let Some(l) = field(n, "left") {
                target_names(l, &mut targets);
            }
        }
        "as_pattern_target" if !comprehension => target_names(n, &mut targets),
        "except_clause" | "except_group_clause" => {
            if let Some(a) = field(n, "alias") {
                target_names(a, &mut targets);
            }
        }
        "import_statement" | "import_from_statement" | "future_import_statement" => {
            push(b, import_names(n));
            return;
        }
        "global_statement" | "nonlocal_statement" => {
            for c in named(n) {
                b.outer.insert(word(c, text).to_owned());
                if n.kind() == "global_statement" {
                    b.globals.insert(word(c, text).to_owned());
                }
            }
            return;
        }
        _ => {}
    }
    push(b, targets);
    for c in children(n) {
        bound_into(c, text, b, comprehension);
    }
}

pub(crate) fn bound_of(body: Node, text: &str) -> Bound {
    let mut b = Bound::default();
    for c in children(body) {
        bound_into(c, text, &mut b, false);
    }
    b
}

pub(crate) fn param_names(parameters: Node, text: &str) -> Vec<String> {
    fn name<'a>(p: Node, text: &'a str) -> Option<&'a str> {
        match p.kind() {
            "identifier" => Some(word(p, text)),
            "default_parameter" | "typed_default_parameter" => {
                field(p, "name").and_then(|n| name(n, text))
            }
            "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                named(p).into_iter().find_map(|c| name(c, text))
            }
            _ => None,
        }
    }
    named(parameters)
        .into_iter()
        .filter_map(|p| name(p, text).map(str::to_owned))
        .collect()
}

/// The names of a comprehension's `for` targets.
pub(crate) fn comprehension_targets(n: Node, text: &str) -> HashSet<String> {
    let mut list = Vec::new();
    for c in children(n) {
        if c.kind() == "for_in_clause"
            && let Some(l) = field(c, "left")
        {
            target_names(l, &mut list);
        }
    }
    list.into_iter().map(|t| word(t, text).to_owned()).collect()
}

pub(crate) fn function_locals(f: Node, text: &str) -> HashSet<String> {
    let mut out: HashSet<String> = field(f, "parameters")
        .map(|p| param_names(p, text))
        .unwrap_or_default()
        .into_iter()
        .collect();
    if let Some(body) = field(f, "body") {
        let b = bound_of(body, text);
        out.extend(b.names.into_iter().map(|(n, _)| n));
        out.retain(|n| !b.outer.contains(n));
    }
    out
}

/// What the parameter list evaluates where the function is defined: defaults and annotations.
fn params_outside(parameters: Node, text: &str, out: &mut Vec<(String, usize)>) {
    for p in named(parameters) {
        for f in ["value", "type"] {
            if let Some(v) = field(p, f) {
                free_into(v, text, out);
            }
        }
    }
}

/// The names `n` refers to that are not bound inside nested scopes of `n`: every name of the
/// code of `n` itself, and of each nested function, lambda and comprehension its free names.
pub(crate) fn free_into(n: Node, text: &str, out: &mut Vec<(String, usize)>) {
    match n.kind() {
        "identifier" => out.push((word(n, text).to_owned(), n.start_byte())),
        "comment"
        | "import_statement"
        | "import_from_statement"
        | "future_import_statement"
        | "global_statement"
        | "nonlocal_statement" => {}
        "attribute" => {
            if let Some(o) = field(n, "object") {
                free_into(o, text, out);
            }
        }
        "keyword_argument" => {
            if let Some(v) = field(n, "value") {
                free_into(v, text, out);
            }
        }
        "function_definition" | "lambda" => {
            let mut locals = HashSet::new();
            if let Some(p) = field(n, "parameters") {
                params_outside(p, text, out);
                locals.extend(param_names(p, text));
            }
            if n.kind() == "function_definition" {
                locals = function_locals(n, text);
                if let Some(r) = field(n, "return_type") {
                    free_into(r, text, out);
                }
            }
            if let Some(body) = field(n, "body") {
                let mut inner = Vec::new();
                free_into(body, text, &mut inner);
                out.extend(inner.into_iter().filter(|(x, _)| !locals.contains(x)));
            }
        }
        "class_definition" => {
            for f in ["superclasses", "body"] {
                if let Some(c) = field(n, f) {
                    free_into(c, text, out);
                }
            }
        }
        k if COMPREHENSIONS.contains(&k) => {
            let targets = comprehension_targets(n, text);
            let mut inner = Vec::new();
            let mut first = true;
            for c in named(n) {
                if c.kind() == "for_in_clause" {
                    if let Some(r) = field(c, "right") {
                        free_into(r, text, if first { &mut *out } else { &mut inner });
                    }
                    first = false;
                } else {
                    free_into(c, text, &mut inner);
                }
            }
            out.extend(inner.into_iter().filter(|(x, _)| !targets.contains(x)));
        }
        _ => {
            for c in named(n) {
                free_into(c, text, out);
            }
        }
    }
}

/// Where the flow of control stands after a piece of code.
#[derive(Clone, Debug)]
enum St {
    /// It goes on; the names assigned on every path so far.
    Reach(HashSet<String>),
    /// It left through `break` or `continue`.
    Jump,
    /// It left through `return` or `raise`.
    Exit,
}

fn join(a: St, b: St) -> St {
    match (a, b) {
        (St::Reach(x), St::Reach(y)) => St::Reach(x.intersection(&y).cloned().collect()),
        (St::Reach(x), _) | (_, St::Reach(x)) => St::Reach(x),
        (St::Jump, _) | (_, St::Jump) => St::Jump,
        _ => St::Exit,
    }
}

/// Reads and writes of the names of one scope in the order of evaluation, with what is assigned
/// on every path (definite assignment over the statements' structure).
struct Flow<'a> {
    text: &'a str,
    /// Name, byte, and whether it was assigned on every path before.
    reads: Vec<(String, usize, bool)>,
    /// Name and byte.
    writes: Vec<(String, usize)>,
}

impl<'a> Flow<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            reads: Vec::new(),
            writes: Vec::new(),
        }
    }

    fn read_name(&mut self, name: String, at: usize, da: &HashSet<String>) {
        let assigned = da.contains(&name);
        self.reads.push((name, at, assigned));
    }

    fn write(&mut self, n: Node, da: &mut HashSet<String>, definite: bool) {
        let name = word(n, self.text).to_owned();
        self.writes.push((name.clone(), n.start_byte()));
        if definite {
            da.insert(name);
        }
    }

    fn free_reads(&mut self, n: Node, da: &HashSet<String>) {
        let mut free = Vec::new();
        free_into(n, self.text, &mut free);
        for (name, at) in free {
            self.read_name(name, at, da);
        }
    }

    fn target(&mut self, n: Node, da: &mut HashSet<String>, definite: bool) {
        match n.kind() {
            "identifier" => self.write(n, da, definite),
            "pattern_list"
            | "tuple_pattern"
            | "list_pattern"
            | "list_splat_pattern"
            | "as_pattern_target"
            | "parenthesized_expression"
            | "tuple"
            | "list"
            | "expression_list"
            | "list_splat" => {
                for c in named(n) {
                    self.target(c, da, definite);
                }
            }
            _ => self.expr(n, da, definite),
        }
    }

    fn expr(&mut self, n: Node, da: &mut HashSet<String>, definite: bool) {
        match n.kind() {
            "identifier" => self.read_name(word(n, self.text).to_owned(), n.start_byte(), da),
            "comment" => {}
            "attribute" => {
                if let Some(o) = field(n, "object") {
                    self.expr(o, da, definite);
                }
            }
            "keyword_argument" => {
                if let Some(v) = field(n, "value") {
                    self.expr(v, da, definite);
                }
            }
            "assignment" => {
                if let Some(t) = field(n, "type") {
                    self.expr(t, da, definite);
                }
                if let Some(r) = field(n, "right") {
                    self.expr(r, da, definite);
                    if let Some(l) = field(n, "left") {
                        self.target(l, da, definite);
                    }
                }
            }
            "augmented_assignment" => {
                if let Some(l) = field(n, "left") {
                    self.expr(l, da, definite);
                }
                if let Some(r) = field(n, "right") {
                    self.expr(r, da, definite);
                }
                if let Some(l) = field(n, "left") {
                    self.target(l, da, definite);
                }
            }
            "named_expression" => {
                if let Some(v) = field(n, "value") {
                    self.expr(v, da, definite);
                }
                if let Some(name) = field(n, "name") {
                    self.write(name, da, definite);
                }
            }
            "lambda" | "function_definition" | "class_definition" => self.free_reads(n, da),
            k if COMPREHENSIONS.contains(&k) => {
                // The first iterable is evaluated here and now, the rest in the comprehension.
                let targets = comprehension_targets(n, self.text);
                let mut first = true;
                for c in named(n) {
                    if c.kind() == "for_in_clause" && first {
                        first = false;
                        if let Some(r) = field(c, "right") {
                            self.expr(r, da, definite);
                        }
                        continue;
                    }
                    let part = if c.kind() == "for_in_clause" {
                        field(c, "right")
                    } else {
                        Some(c)
                    };
                    let Some(part) = part else { continue };
                    let mut free = Vec::new();
                    free_into(part, self.text, &mut free);
                    for (name, at) in free {
                        if !targets.contains(&name) {
                            self.read_name(name, at, da);
                        }
                    }
                    // `:=` inside a comprehension binds in the enclosing function.
                    let mut b = Bound::default();
                    bound_into(part, self.text, &mut b, true);
                    self.writes.extend(b.names);
                }
            }
            "conditional_expression" => {
                let k = named(n);
                if k.len() == 3 {
                    self.expr(k[1], da, definite);
                    self.expr(k[0], da, false);
                    self.expr(k[2], da, false);
                } else {
                    for c in k {
                        self.expr(c, da, false);
                    }
                }
            }
            "boolean_operator" => {
                if let Some(l) = field(n, "left") {
                    self.expr(l, da, definite);
                }
                if let Some(r) = field(n, "right") {
                    self.expr(r, da, false);
                }
            }
            "comparison_operator" => {
                for (i, c) in named(n).into_iter().enumerate() {
                    self.expr(c, da, definite && i < 2);
                }
            }
            _ => {
                for c in named(n) {
                    self.expr(c, da, definite);
                }
            }
        }
    }

    fn block(&mut self, b: Node, st: &mut St) {
        for c in named(b) {
            self.stmt(c, st);
        }
    }

    /// A block run from a copy of `da`.
    fn branch(&mut self, b: Option<Node>, da: &HashSet<String>) -> St {
        let mut s = St::Reach(da.clone());
        if let Some(b) = b {
            self.block(b, &mut s);
        }
        s
    }

    fn definition(&mut self, n: Node, da: &mut HashSet<String>) {
        let def = if n.kind() == "decorated_definition" {
            for d in children(n) {
                if d.kind() == "decorator" {
                    self.expr(d, da, true);
                }
            }
            field(n, "definition")
        } else {
            Some(n)
        };
        let Some(def) = def else { return };
        self.free_reads(def, da);
        if let Some(name) = field(def, "name") {
            self.write(name, da, true);
        }
    }

    fn stmt(&mut self, n: Node, st: &mut St) {
        let St::Reach(da) = st else { return };
        match n.kind() {
            "return_statement" | "raise_statement" => {
                for c in named(n) {
                    self.expr(c, da, true);
                }
                *st = St::Exit;
            }
            "break_statement" | "continue_statement" => *st = St::Jump,
            "pass_statement" | "global_statement" | "nonlocal_statement" | "comment" => {}
            "delete_statement" => {
                let mut list = Vec::new();
                for c in named(n) {
                    if c.kind() == "expression_list" {
                        list.extend(named(c));
                    } else {
                        list.push(c);
                    }
                }
                for t in list {
                    self.expr(t, da, true);
                    if t.kind() == "identifier" {
                        da.remove(word(t, self.text));
                    }
                }
            }
            "import_statement" | "import_from_statement" | "future_import_statement" => {
                for name in import_names(n) {
                    self.write(name, da, true);
                }
            }
            "if_statement" => {
                if let Some(c) = field(n, "condition") {
                    self.expr(c, da, true);
                }
                let mut cur = da.clone();
                let mut out = self.branch(field(n, "consequence"), &cur);
                let mut has_else = false;
                let mut cursor = n.walk();
                let alternatives: Vec<Node> = n
                    .children_by_field_name("alternative", &mut cursor)
                    .collect();
                for alt in alternatives {
                    if alt.kind() == "elif_clause" {
                        if let Some(c) = field(alt, "condition") {
                            self.expr(c, &mut cur, true);
                        }
                        let s = self.branch(field(alt, "consequence"), &cur);
                        out = join(out, s);
                    } else {
                        has_else = true;
                        let s = self.branch(field(alt, "body"), &cur);
                        out = join(out, s);
                    }
                }
                if !has_else {
                    out = join(out, St::Reach(cur));
                }
                *st = out;
            }
            "for_statement" => {
                if let Some(r) = field(n, "right") {
                    self.expr(r, da, true);
                }
                let mut inner = da.clone();
                if let Some(l) = field(n, "left") {
                    self.target(l, &mut inner, true);
                }
                self.branch(field(n, "body"), &inner);
                let pre = da.clone();
                self.branch(field(n, "alternative").and_then(|a| field(a, "body")), &pre);
            }
            "while_statement" => {
                if let Some(c) = field(n, "condition") {
                    self.expr(c, da, true);
                }
                let pre = da.clone();
                self.branch(field(n, "body"), &pre);
                self.branch(field(n, "alternative").and_then(|a| field(a, "body")), &pre);
            }
            "try_statement" => {
                let pre = da.clone();
                let mut body = self.branch(field(n, "body"), &pre);
                let mut handlers = Vec::new();
                let mut finally = None;
                for c in named(n) {
                    match c.kind() {
                        "except_clause" | "except_group_clause" => {
                            let mut inner = pre.clone();
                            self.handler_head(c, &mut inner);
                            let mut h = St::Reach(inner);
                            for b in children(c) {
                                if b.kind() == "block" {
                                    self.block(b, &mut h);
                                }
                            }
                            handlers.push(h);
                        }
                        "else_clause" => {
                            if let Some(b) = field(c, "body") {
                                self.block(b, &mut body);
                            }
                        }
                        "finally_clause" => {
                            let mut f = St::Reach(pre.clone());
                            for b in children(c) {
                                if b.kind() == "block" {
                                    self.block(b, &mut f);
                                }
                            }
                            finally = Some(f);
                        }
                        _ => {}
                    }
                }
                let mut out = body;
                for h in handlers {
                    out = join(out, h);
                }
                if matches!(finally, Some(St::Exit)) {
                    out = St::Exit;
                }
                *st = out;
            }
            "with_statement" => {
                self.with_items(n, da);
                let entered = da.clone();
                if let Some(b) = field(n, "body") {
                    self.block(b, st);
                }
                // A context manager may swallow an error, so a body that always returns or
                // raises does not make the statement do so.
                if matches!(st, St::Exit) {
                    *st = St::Reach(entered);
                }
            }
            "match_statement" => {
                if let Some(s) = field(n, "subject") {
                    self.expr(s, da, true);
                }
                let pre = da.clone();
                let mut stack = vec![n];
                while let Some(x) = stack.pop() {
                    if x.kind() == "case_clause" {
                        let mut inner = pre.clone();
                        if let Some(g) = field(x, "guard") {
                            self.expr(g, &mut inner, false);
                        }
                        self.branch(field(x, "consequence"), &inner);
                    } else if x.id() == n.id() || x.kind() == "block" {
                        stack.extend(children(x));
                    }
                }
            }
            "function_definition" | "class_definition" | "decorated_definition" => {
                self.definition(n, da)
            }
            _ => {
                for c in named(n) {
                    self.expr(c, da, true);
                }
            }
        }
    }

    /// The expression and the name of an `except` clause.
    fn handler_head(&mut self, clause: Node, da: &mut HashSet<String>) {
        for c in named(clause) {
            if c.kind() == "block" {
                continue;
            }
            if is_field(clause, "alias", c) {
                self.target(c, da, true);
            } else if c.kind() == "as_pattern" {
                self.as_pattern(c, da);
            } else {
                self.expr(c, da, true);
            }
        }
    }

    fn as_pattern(&mut self, n: Node, da: &mut HashSet<String>) {
        let alias = field(n, "alias");
        for c in named(n) {
            if alias.is_some_and(|a| a.id() == c.id()) {
                self.target(c, da, true);
            } else {
                self.expr(c, da, true);
            }
        }
    }

    fn with_items(&mut self, with: Node, da: &mut HashSet<String>) {
        let mut stack: Vec<Node> = children(with)
            .into_iter()
            .filter(|c| c.kind() != "block")
            .rev()
            .collect();
        while let Some(x) = stack.pop() {
            match x.kind() {
                "with_item" => {
                    for v in named(x) {
                        if v.kind() == "as_pattern" {
                            self.as_pattern(v, da);
                        } else {
                            self.expr(v, da, true);
                        }
                    }
                }
                _ => stack.extend(children(x).into_iter().rev()),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Extract Method
// ---------------------------------------------------------------------------------------------

/// What the selection's own code (nested functions, lambdas and classes left out) contains.
#[derive(Default)]
struct Scan {
    returns: bool,
    awaits: bool,
    refusal: Option<String>,
    /// `super` or `__class__`.
    class_cell: bool,
}

fn scan(n: Node, text: &str, in_loop: bool, s: &mut Scan) {
    let refuse = |s: &mut Scan, why: &str| {
        if s.refusal.is_none() {
            s.refusal = Some(why.to_owned());
        }
    };
    let outer = in_loop;
    let mut in_loop = in_loop;
    match n.kind() {
        "function_definition" | "lambda" | "class_definition" => return,
        "return_statement" => s.returns = true,
        "yield" => refuse(
            s,
            "it contains `yield`, which would turn the new function into a generator",
        ),
        "await" => s.awaits = true,
        "async" => s.awaits = true,
        "break_statement" | "continue_statement" if !in_loop => refuse(
            s,
            "it contains `break` or `continue` of a loop outside the selection",
        ),
        "global_statement" | "nonlocal_statement" => {
            refuse(s, "it contains a `global` or `nonlocal` statement")
        }
        "delete_statement" => {
            let names = named(n).into_iter().any(|c| {
                c.kind() == "identifier"
                    || (c.kind() == "expression_list"
                        && named(c).iter().any(|x| x.kind() == "identifier"))
            });
            if names {
                refuse(s, "it deletes a name with `del`");
            }
        }
        "future_import_statement" => refuse(s, "it contains `from __future__ import`"),
        "wildcard_import" => refuse(s, "it contains `from … import *`"),
        "for_statement" | "while_statement" => in_loop = true,
        "call" => {
            if let Some(f) = field(n, "function")
                && f.kind() == "identifier"
                && matches!(word(f, text), "locals" | "vars" | "eval" | "exec" | "dir")
            {
                refuse(
                    s,
                    &format!(
                        "it calls `{}`, which sees the local names of the function it runs in",
                        word(f, text)
                    ),
                );
            }
        }
        "identifier" if matches!(word(n, text), "super" | "__class__") => s.class_cell = true,
        _ => {}
    }
    for c in children(n) {
        // A `break` in a loop's `else` block belongs to the loop around it.
        let inside = if c.kind() == "else_clause" {
            outer
        } else {
            in_loop
        };
        scan(c, text, inside, s);
    }
}

fn private_name(n: Node, text: &str) -> Option<String> {
    if n.kind() == "identifier" {
        let w = word(n, text);
        return (w.starts_with("__") && !w.ends_with("__")).then(|| w.to_owned());
    }
    children(n).into_iter().find_map(|c| private_name(c, text))
}

/// The scopes around a place: the enclosing function `k` and which names are local where.
struct Scopes<'t> {
    k: Option<Node<'t>>,
    /// Names of lambdas and comprehensions between the place and `k`.
    inner: HashSet<String>,
    k_locals: HashSet<String>,
    /// Names `k` declares `global` or `nonlocal`.
    k_declared: HashSet<String>,
    /// Local names of the functions around `k`.
    outer: HashSet<String>,
    in_class: bool,
}

fn scopes<'t>(from: Node<'t>, text: &str) -> Scopes<'t> {
    let mut s = Scopes {
        k: None,
        inner: HashSet::new(),
        k_locals: HashSet::new(),
        k_declared: HashSet::new(),
        outer: HashSet::new(),
        in_class: false,
    };
    let mut blocked: HashSet<String> = HashSet::new();
    let mut c = from;
    let mut via_right = false;
    while let Some(p) = c.parent() {
        match p.kind() {
            "function_definition" if is_field(p, "body", c) => {
                let b = bound_of(c, text);
                let mut locals: HashSet<String> = field(p, "parameters")
                    .map(|x| param_names(x, text))
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                locals.extend(b.names.iter().map(|(n, _)| n.clone()));
                locals.retain(|n| !b.outer.contains(n) && !blocked.contains(n));
                if s.k.is_none() {
                    s.k = Some(p);
                    s.k_locals = locals;
                    s.k_declared = b.outer.clone();
                } else {
                    s.outer.extend(locals);
                }
                blocked.extend(b.globals);
            }
            "lambda" if is_field(p, "body", c) && s.k.is_none() => {
                if let Some(x) = field(p, "parameters") {
                    s.inner.extend(param_names(x, text));
                }
            }
            "lambda" if is_field(p, "body", c) => {
                if let Some(x) = field(p, "parameters") {
                    s.outer.extend(
                        param_names(x, text)
                            .into_iter()
                            .filter(|n| !blocked.contains(n)),
                    );
                }
            }
            "for_in_clause" => via_right = is_field(p, "right", c),
            k if COMPREHENSIONS.contains(&k) => {
                let first = children(p)
                    .into_iter()
                    .find(|x| x.kind() == "for_in_clause")
                    .is_some_and(|f| f.id() == c.id());
                if !(first && via_right) {
                    let names = comprehension_targets(p, text);
                    if s.k.is_none() {
                        s.inner.extend(names);
                    } else {
                        s.outer
                            .extend(names.into_iter().filter(|n| !blocked.contains(n)));
                    }
                }
            }
            "class_definition" => s.in_class = true,
            _ => {}
        }
        c = p;
    }
    s
}

/// The method the function `k` is: its class's statement to put a method after, the name of its
/// first parameter and whether it is a class method. `None` for a function, a static method and a
/// method without a plain first parameter.
fn method_of<'t>(k: Node<'t>, text: &str) -> Option<(Node<'t>, String, bool)> {
    let stmt = match k.parent() {
        Some(p) if p.kind() == "decorated_definition" => p,
        _ => k,
    };
    let block = stmt.parent()?;
    if block.kind() != "block" || block.parent()?.kind() != "class_definition" {
        return None;
    }
    // Static or class methods without a decorator: their first parameter is no instance.
    if field(k, "name").is_some_and(|n| {
        matches!(
            word(n, text),
            "__new__" | "__init_subclass__" | "__class_getitem__"
        )
    }) {
        return None;
    }
    let mut class_method = false;
    for d in children(stmt) {
        if d.kind() == "decorator" {
            match word(d, text).trim_start_matches('@').trim() {
                "staticmethod" => return None,
                "classmethod" => class_method = true,
                _ => {}
            }
        }
    }
    let first = named(field(k, "parameters")?).into_iter().next()?;
    (first.kind() == "identifier").then(|| (stmt, word(first, text).to_owned(), class_method))
}

/// The free names of what a nested function, lambda or generator expression `n` runs later: its
/// body, without what is evaluated where it stands (default values, annotations, a generator's
/// first iterable).
fn deferred_free(n: Node, text: &str, out: &mut Vec<(String, usize)>) {
    let mut inner = Vec::new();
    let locals: HashSet<String> = match n.kind() {
        "function_definition" => function_locals(n, text),
        "lambda" => field(n, "parameters")
            .map(|p| param_names(p, text))
            .unwrap_or_default()
            .into_iter()
            .collect(),
        _ => comprehension_targets(n, text),
    };
    if n.kind() == "generator_expression" {
        let mut first = true;
        for c in named(n) {
            if c.kind() == "for_in_clause" {
                if !first && let Some(r) = field(c, "right") {
                    free_into(r, text, &mut inner);
                }
                first = false;
            } else {
                free_into(c, text, &mut inner);
            }
        }
    } else if let Some(body) = field(n, "body") {
        free_into(body, text, &mut inner);
    }
    out.extend(inner.into_iter().filter(|(x, _)| !locals.contains(x)));
}

/// The top-most nested functions, lambdas and generator expressions below `n`.
fn closures<'t>(n: Node<'t>, out: &mut Vec<Node<'t>>) {
    for c in children(n) {
        if matches!(
            c.kind(),
            "function_definition" | "lambda" | "generator_expression"
        ) {
            out.push(c);
        } else {
            closures(c, out);
        }
    }
}

fn indent_unit(root: Node, text: &str) -> String {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.kind() == "block"
            && let Some(first) = named(n).into_iter().next()
            && let Some(parent) = n.parent().and_then(statement_of)
        {
            let (inner, outer) = (
                indent_at(text, first.start_byte()),
                indent_at(text, parent.start_byte()),
            );
            if starts_its_line(text, first)
                && inner.len() > outer.len()
                && let Some(unit) = inner.strip_prefix(outer)
            {
                return unit.to_owned();
            }
        }
        stack.extend(children(n).into_iter().rev());
    }
    "    ".to_owned()
}

/// The lines `from..to` of `text` with the indentation `old` replaced by `new`; lines that start
/// inside one of the `strings` are left alone.
fn reindent(
    text: &str,
    lines: Bytes<usize>,
    old: &str,
    new: &str,
    strings: &[Bytes<usize>],
) -> String {
    let mut out = String::new();
    let mut at = lines.start;
    for (i, line) in text[lines].split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let inside = strings.iter().any(|r| r.start < at && at < r.end);
        match line.strip_prefix(old) {
            Some(rest) if !inside && !line.trim().is_empty() => {
                out.push_str(new);
                out.push_str(rest);
            }
            _ => out.push_str(line),
        }
        at += line.len() + 1;
    }
    out
}

fn multi_line_strings(n: Node, out: &mut Vec<Bytes<usize>>) {
    if n.kind() == "string" {
        if n.start_position().row != n.end_position().row {
            out.push(n.byte_range());
        }
        return;
    }
    for c in children(n) {
        multi_line_strings(c, out);
    }
}

/// What is selected for Extract Method.
enum Picked<'t> {
    /// Statements (and comments) of one block, in order.
    Statements(Vec<Node<'t>>),
    Expression(Node<'t>),
}

/// Whether `part` holds nothing but white space and comments.
fn only_comments(part: &str) -> bool {
    part.lines().all(|l| {
        let l = l.trim();
        l.is_empty() || l.starts_with('#')
    })
}

fn pick<'t>(root: Node<'t>, text: &str, sel: &Bytes<usize>) -> Result<Picked<'t>, String> {
    const WHAT: &str = "select one or more complete statements of one block, or one complete \
                        expression";
    if sel.is_empty() {
        return Err("nothing is selected".into());
    }
    // The first statement, after comment lines the selection starts with (comments above the
    // first statement of a block stand outside the block in the tree).
    let mut start = sel.start;
    while text[start..sel.end].starts_with('#') {
        let rest = &text[start..sel.end];
        let line = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let next = &rest[line..];
        start += line + (next.len() - next.trim_start().len());
    }
    let mut first = None;
    let mut node = root.descendant_for_byte_range(start, (start + 1).min(sel.end));
    while let Some(n) = node {
        if n.start_byte() != start {
            break;
        }
        if is_statement(n) {
            first = Some(n);
            break;
        }
        node = n.parent();
    }
    if let Some(first) = first {
        let mut items = Vec::new();
        let mut next = Some(first);
        while let Some(n) = next {
            if n.end_byte() > sel.end {
                break;
            }
            items.push(n);
            next = n.next_named_sibling();
        }
        while items.last().is_some_and(|l| l.kind() == "comment") {
            items.pop();
        }
        if let Some(last) = items.last()
            && only_comments(&text[last.end_byte()..sel.end])
        {
            return Ok(Picked::Statements(items));
        }
    }
    match expression_at(root, sel) {
        Ok(n) => Ok(Picked::Expression(n)),
        Err(_) => Err(WHAT.into()),
    }
}

/// The statements after the selection that may still run, walked outwards up to the block
/// `stop` (the function's body or the module): which names are read before they are assigned
/// again (`live`), and which are read in `except` or `finally` blocks of a `try` around the
/// selection (`handlers`).
struct After {
    live: HashSet<String>,
    handlers: HashSet<String>,
}

fn after(first: Node, last: Node, stop: Node, text: &str) -> After {
    let mut handlers = HashSet::new();
    let mut c = first;
    while let Some(p) = c.parent() {
        if c.id() == stop.id() {
            break;
        }
        if p.kind() == "try_statement" {
            let in_body = is_field(p, "body", c);
            for clause in named(p) {
                let read = match clause.kind() {
                    "except_clause" | "except_group_clause" => in_body,
                    "finally_clause" => clause.id() != c.id(),
                    _ => false,
                };
                if read {
                    let mut free = Vec::new();
                    free_into(clause, text, &mut free);
                    handlers.extend(free.into_iter().map(|(n, _)| n));
                }
            }
        }
        c = p;
    }

    let mut flow = Flow::new(text);
    let mut st = St::Reach(HashSet::new());
    let mut child = last;
    loop {
        // The rest of the block.
        let mut next = child.next_named_sibling();
        while let Some(n) = next {
            flow.stmt(n, &mut st);
            next = n.next_named_sibling();
        }
        if matches!(st, St::Exit) {
            break;
        }
        let Some(block) = child.parent() else { break };
        if block.id() == stop.id() {
            break;
        }
        // The statement the block belongs to, directly or through a clause.
        let mut clause = block;
        let Some(mut owner) = block.parent() else {
            break;
        };
        while !is_statement(owner) {
            clause = owner;
            match owner.parent() {
                Some(p) => owner = p,
                None => break,
            }
        }
        match owner.kind() {
            "for_statement" | "while_statement" => {
                // Another round: everything in the loop may run again, and after the loop
                // nothing is known.
                let mut again = St::Reach(HashSet::new());
                flow.stmt(owner, &mut again);
                st = St::Reach(HashSet::new());
            }
            "try_statement" => {
                if is_field(owner, "body", clause) {
                    for c in named(owner) {
                        if c.kind() == "else_clause"
                            && let Some(b) = field(c, "body")
                        {
                            flow.block(b, &mut st);
                        }
                    }
                }
                // After the `try` the code may have come through a handler.
                if let St::Reach(da) = &mut st {
                    da.clear();
                }
            }
            _ => {}
        }
        child = owner;
    }
    let live = flow
        .reads
        .iter()
        .filter(|r| !r.2)
        .map(|r| r.0.clone())
        .collect();
    After { live, handlers }
}

/// The names assigned on every path when the selection starts, inside the function body or
/// module `body` (parameters are added by the caller).
fn before(body: Node, start: usize, text: &str, da: HashSet<String>) -> HashSet<String> {
    let mut flow = Flow::new(text);
    let mut st = St::Reach(da);
    let mut block = body;
    'outer: loop {
        for c in named(block) {
            if c.start_byte() >= start {
                break 'outer;
            }
            if c.end_byte() <= start {
                let mut s = st.clone();
                flow.stmt(c, &mut s);
                // Code after a `return` on every path is not reached; keep what is known.
                if matches!(s, St::Reach(_)) {
                    st = s;
                }
                continue;
            }
            // The statement around the selection: what its header assigns, then its block.
            let St::Reach(da) = &mut st else { break 'outer };
            let inner = |n: Node| -> bool { n.start_byte() <= start && start < n.end_byte() };
            match c.kind() {
                "if_statement" => {
                    if let Some(x) = field(c, "condition") {
                        flow.expr(x, da, true);
                    }
                    if let Some(b) = field(c, "consequence").filter(|b| inner(*b)) {
                        block = b;
                        continue 'outer;
                    }
                    let mut cursor = c.walk();
                    let alternatives: Vec<Node> = c
                        .children_by_field_name("alternative", &mut cursor)
                        .collect();
                    for alt in alternatives {
                        if alt.kind() == "elif_clause"
                            && let Some(x) = field(alt, "condition")
                        {
                            flow.expr(x, da, true);
                        }
                        let b = field(alt, "consequence").or_else(|| field(alt, "body"));
                        if let Some(b) = b.filter(|b| inner(*b)) {
                            block = b;
                            continue 'outer;
                        }
                    }
                    break 'outer;
                }
                "for_statement" => {
                    if let Some(x) = field(c, "right") {
                        flow.expr(x, da, true);
                    }
                    if let Some(b) = field(c, "body").filter(|b| inner(*b)) {
                        if let Some(l) = field(c, "left") {
                            flow.target(l, da, true);
                        }
                        block = b;
                        continue 'outer;
                    }
                }
                "while_statement" => {
                    if let Some(x) = field(c, "condition") {
                        flow.expr(x, da, true);
                    }
                    if let Some(b) = field(c, "body").filter(|b| inner(*b)) {
                        block = b;
                        continue 'outer;
                    }
                }
                "with_statement" => {
                    flow.with_items(c, da);
                    if let Some(b) = field(c, "body").filter(|b| inner(*b)) {
                        block = b;
                        continue 'outer;
                    }
                }
                "try_statement" => {
                    if let Some(b) = field(c, "body").filter(|b| inner(*b)) {
                        block = b;
                        continue 'outer;
                    }
                    for clause in named(c) {
                        if !inner(clause) {
                            continue;
                        }
                        match clause.kind() {
                            "except_clause" | "except_group_clause" => {
                                flow.handler_head(clause, da)
                            }
                            "else_clause" => {
                                if let Some(b) = field(c, "body") {
                                    let mut s = St::Reach(da.clone());
                                    flow.block(b, &mut s);
                                    if let St::Reach(d) = s {
                                        *da = d;
                                    }
                                }
                            }
                            _ => {}
                        }
                        let b = field(clause, "body")
                            .or_else(|| children(clause).into_iter().find(|x| x.kind() == "block"));
                        if let Some(b) = b.filter(|b| inner(*b)) {
                            block = b;
                            continue 'outer;
                        }
                    }
                    break 'outer;
                }
                _ => break 'outer,
            }
            // A loop's or statement's `else` block, or another block below it.
            let alt = field(c, "alternative")
                .and_then(|a| field(a, "body"))
                .filter(|b| inner(*b));
            match alt {
                Some(b) => {
                    block = b;
                    continue 'outer;
                }
                None => break 'outer,
            }
        }
        break;
    }
    match st {
        St::Reach(da) => da,
        _ => HashSet::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// Types of the new function (behaviour 16)
// ---------------------------------------------------------------------------------------------

/// Built-in types an annotation may name without the file binding them.
pub(crate) const BUILTIN_TYPES: &[&str] = &[
    "int",
    "float",
    "complex",
    "bool",
    "str",
    "bytes",
    "bytearray",
    "memoryview",
    "list",
    "dict",
    "set",
    "frozenset",
    "tuple",
    "object",
    "type",
    "range",
    "slice",
    "BaseException",
    "Exception",
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "EOFError",
    "ImportError",
    "IndexError",
    "KeyError",
    "KeyboardInterrupt",
    "LookupError",
    "NotImplementedError",
    "OSError",
    "RuntimeError",
    "StopIteration",
    "TypeError",
    "ValueError",
    "ZeroDivisionError",
];

/// Built-in types the server prints without arguments when it does not know them.
const CONTAINERS: &[&str] = &["list", "dict", "set", "frozenset", "tuple", "type"];

/// The language server's hover texts by the byte of the hovered name, and the places a plan asked
/// for (behaviour 16, source 3). Without answers only the file's own annotations are used.
#[derive(Default)]
struct Hovers {
    answers: HashMap<usize, String>,
    asked: RefCell<Vec<usize>>,
}

impl Hovers {
    fn at(&self, byte: usize) -> Option<&str> {
        let mut asked = self.asked.borrow_mut();
        if !asked.contains(&byte) {
            asked.push(byte);
        }
        self.answers.get(&byte).map(String::as_str)
    }
}

/// The code of a hover in one line: its first fenced block, else the text up to the first empty
/// line (what follows is documentation).
fn hover_code(hover: &str) -> String {
    let lines: Vec<&str> = hover.lines().collect();
    let fence = |l: &&str| l.trim_start().starts_with("```");
    let code: Vec<&str> = match lines.iter().position(fence) {
        Some(open) => lines[open + 1..]
            .iter()
            .take_while(|l| !fence(l))
            .copied()
            .collect(),
        None => lines
            .iter()
            .skip_while(|l| l.trim().is_empty())
            .take_while(|l| !l.trim().is_empty())
            .copied()
            .collect(),
    };
    code.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ")
}

/// The type of `(variable) name: type` (also `parameter`, `constant` and, for an attribute,
/// `property`).
fn variable_hover<'c>(code: &'c str, name: &str, attribute: bool) -> Option<&'c str> {
    let kinds: &[&str] = if attribute {
        &["(variable) ", "(property) ", "(constant) "]
    } else {
        &["(variable) ", "(parameter) ", "(constant) "]
    };
    kinds
        .iter()
        .find_map(|k| code.strip_prefix(k))?
        .strip_prefix(name)?
        .strip_prefix(": ")
}

/// The declared result of `(function) def f(…) -> type` or `(method) def …`, when the hover shows
/// exactly one signature.
fn result_hover(code: &str) -> Option<&str> {
    let rest = code
        .strip_prefix("(function) ")
        .or_else(|| code.strip_prefix("(method) "))?;
    if !rest.starts_with("def ") || rest.matches("def ").count() != 1 {
        return None;
    }
    rest.rfind(") -> ").map(|i| &rest[i + ") -> ".len()..])
}

/// Whether `n` holds a `|` operator outside a string.
fn has_pipe(n: Node) -> bool {
    match n.kind() {
        "string" => false,
        "binary_operator" if field(n, "operator").is_some_and(|o| o.kind() == "|") => true,
        _ => children(n).into_iter().any(has_pipe),
    }
}

/// Whether a function's signature in `n` writes a `|` union in an annotation: the file then runs
/// on a Python that can evaluate one.
fn signature_pipe(n: Node) -> bool {
    if n.kind() == "function_definition" {
        let returns = field(n, "return_type").is_some_and(has_pipe);
        let params = field(n, "parameters").is_some_and(|p| {
            named(p)
                .into_iter()
                .any(|x| field(x, "type").is_some_and(has_pipe))
        });
        if returns || params {
            return true;
        }
    }
    children(n).into_iter().any(signature_pipe)
}

/// Every identifier inside `n`.
fn identifiers(n: Node, text: &str, out: &mut HashSet<String>) {
    if n.kind() == "identifier" {
        out.insert(word(n, text).to_owned());
    }
    for c in children(n) {
        identifiers(c, text, out);
    }
}

/// How often the name `name` stands in `n`.
fn occurrences(n: Node, text: &str, name: &str) -> usize {
    let own = usize::from(n.kind() == "identifier" && word(n, text) == name);
    own + children(n)
        .into_iter()
        .map(|c| occurrences(c, text, name))
        .sum::<usize>()
}

/// The `return` statements of `n` itself, not those of nested functions.
fn own_returns<'t>(n: Node<'t>, out: &mut Vec<Node<'t>>) {
    match n.kind() {
        "function_definition" | "lambda" | "class_definition" => {}
        "return_statement" => out.push(n),
        _ => {
            for c in children(n) {
                own_returns(c, out);
            }
        }
    }
}

/// The annotated assignments and declarations of `name` among the own statements of a scope.
fn annotations_of<'t>(n: Node<'t>, text: &str, name: &str, out: &mut Vec<Node<'t>>) {
    for c in children(n) {
        match c.kind() {
            "function_definition" | "lambda" | "class_definition" => continue,
            "assignment" => {
                if let (Some(l), Some(t)) = (field(c, "left"), field(c, "type"))
                    && l.kind() == "identifier"
                    && word(l, text) == name
                {
                    out.push(t);
                }
            }
            _ => {}
        }
        annotations_of(c, text, name, out);
    }
}

/// The names an annotation refers to, and whether Python evaluates them (those inside a string
/// are only read by type checkers). Names after a dot and the strings of `Literal[…]` do not
/// count.
fn annotation_names(n: Node, text: &str, literal: bool, out: &mut Vec<(String, bool)>) {
    match n.kind() {
        "identifier" => out.push((word(n, text).to_owned(), true)),
        "attribute" => {
            if let Some(o) = field(n, "object") {
                annotation_names(o, text, literal, out);
            }
        }
        "string" => {
            if literal {
                return;
            }
            for c in children(n) {
                if c.kind() != "string_content" {
                    continue;
                }
                let content = word(c, text);
                let mut start = None;
                for (i, ch) in content.char_indices().chain([(content.len(), ' ')]) {
                    let wordy = ch.is_alphanumeric() || ch == '_';
                    match (start, wordy) {
                        (None, true) => start = Some(i),
                        (Some(s), false) => {
                            let w = &content[s..i];
                            let dotted = content[..s].ends_with('.');
                            let name = !w.starts_with(|c: char| c.is_ascii_digit());
                            if name && !dotted && !matches!(w, "None" | "True" | "False") {
                                out.push((w.to_owned(), false));
                            }
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        "subscript" => {
            let value = field(n, "value");
            let is_literal = value.is_some_and(|v| {
                let t = word(v, text);
                t == "Literal" || t.ends_with(".Literal")
            });
            for c in children(n) {
                annotation_names(c, text, literal || is_literal, out);
            }
        }
        _ => {
            for c in children(n) {
                annotation_names(c, text, literal, out);
            }
        }
    }
}

/// Where an annotation was written.
#[derive(Clone, Copy, PartialEq)]
enum Written {
    /// On a parameter of the function the selection is in.
    Parameter,
    /// On a variable of that function.
    Variable,
}

/// What decides the types of the new function's parameters and its return type.
struct Types<'a, 't> {
    text: &'a str,
    hovers: &'a Hovers,
    root: Node<'t>,
    sel: Bytes<usize>,
    k: Option<Node<'t>>,
    k_body: Node<'t>,
    /// The new function is a method of the class the selection's method is in.
    method: bool,
    /// Names whose declaration the function `k` (or the module) holds: not those of lambdas and
    /// comprehensions around the selection.
    own: HashSet<String>,
    k_params: HashSet<String>,
    /// Bindings at the top level of the file: name, byte, and whether only type checkers see it
    /// (under `if TYPE_CHECKING:`).
    module: Vec<(String, usize, bool)>,
    /// Start of the top-level statement the selection is in.
    top: usize,
    /// `from __future__ import annotations`: annotations are not evaluated.
    future: bool,
    /// A `|` union can be written.
    pipes: bool,
    /// Local names, parameters and type parameters of the enclosing functions.
    hidden: HashSet<String>,
    /// Type parameters of `k`.
    k_type_params: HashSet<String>,
    /// Names bound in the bodies of the enclosing classes.
    class_names: HashSet<String>,
    reads: &'a [(String, usize, bool)],
    writes: &'a [(String, usize)],
    assigned_at_end: &'a HashSet<String>,
}

impl<'a, 't> Types<'a, 't> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        root: Node<'t>,
        text: &'a str,
        sel: &Bytes<usize>,
        first: Node<'t>,
        sc: &Scopes<'t>,
        k_body: Node<'t>,
        method: bool,
        hovers: &'a Hovers,
        flow: &'a Flow<'a>,
        assigned_at_end: &'a HashSet<String>,
    ) -> Self {
        let checked = |at: usize| {
            let mut c = root.descendant_for_byte_range(at, at);
            while let Some(n) = c {
                if n.kind() == "if_statement"
                    && field(n, "condition")
                        .is_some_and(|x| word(x, text).contains("TYPE_CHECKING"))
                {
                    return true;
                }
                c = n.parent();
            }
            false
        };
        let module = bound_of(root, text)
            .names
            .into_iter()
            .map(|(n, at)| (n, at, checked(at)))
            .collect();
        let future = children(root).into_iter().any(|c| {
            c.kind() == "future_import_statement" && word(c, text).contains("annotations")
        });
        let mut hidden: HashSet<String> = sc.k_locals.clone();
        hidden.extend(sc.inner.iter().cloned());
        hidden.extend(sc.outer.iter().cloned());
        let mut k_type_params = HashSet::new();
        let mut class_names = HashSet::new();
        let mut c = first;
        while let Some(p) = c.parent() {
            let params = field(p, "type_parameters");
            match p.kind() {
                "function_definition" => {
                    if let Some(tp) = params {
                        identifiers(tp, text, &mut hidden);
                        if sc.k.is_some_and(|k| k.id() == p.id()) {
                            identifiers(tp, text, &mut k_type_params);
                        }
                    }
                }
                "class_definition" => {
                    if let Some(tp) = params
                        && !method
                    {
                        identifiers(tp, text, &mut hidden);
                    }
                    if let Some(b) = field(p, "body") {
                        class_names.extend(bound_of(b, text).names.into_iter().map(|(n, _)| n));
                    }
                }
                _ => {}
            }
            c = p;
        }
        let k_params: HashSet<String> =
            sc.k.and_then(|k| field(k, "parameters"))
                .map(|p| param_names(p, text))
                .unwrap_or_default()
                .into_iter()
                .collect();
        let own = match sc.k {
            Some(_) => sc
                .k_locals
                .iter()
                .filter(|n| !sc.inner.contains(*n))
                .cloned()
                .collect(),
            // At module level every name that no lambda or comprehension binds.
            None => {
                let mut all = HashSet::new();
                identifiers(root, text, &mut all);
                all.retain(|n| !sc.inner.contains(n));
                all
            }
        };
        Self {
            text,
            hovers,
            root,
            sel: sel.clone(),
            k: sc.k,
            k_body,
            method,
            own,
            k_params,
            module,
            top: top_level_of(first).start_byte(),
            future,
            pipes: future || signature_pipe(root),
            hidden,
            k_type_params,
            class_names,
            reads: &flow.reads,
            writes: &flow.writes,
            assigned_at_end,
        }
    }

    /// Whether the module binds `name` where an annotation of the new function can use it:
    /// `evaluated` annotations need the binding above the selection's top-level statement and at
    /// run time.
    fn module_name(&self, name: &str, evaluated: bool, once: bool) -> bool {
        let all: Vec<&(String, usize, bool)> =
            self.module.iter().filter(|(n, _, _)| n == name).collect();
        if all.is_empty() {
            return BUILTIN_TYPES.contains(&name);
        }
        if once && all.len() != 1 {
            return false;
        }
        !evaluated
            || self.future
            || all
                .iter()
                .any(|(_, at, checked)| *at < self.top && !*checked)
    }

    /// The text of the annotation `node` when it means the same on the new function.
    fn copied(&self, node: Node, from: Written) -> Option<String> {
        let mut names = Vec::new();
        annotation_names(node, self.text, false, &mut names);
        let shown = word(node, self.text).to_owned();
        if self.method && from == Written::Parameter {
            // Same class, right after the method: only the method's own type parameters are gone.
            return names
                .iter()
                .all(|(n, _)| !self.k_type_params.contains(n))
                .then_some(shown);
        }
        // A variable's annotation is read from inside the function, where the class body does
        // not count; on the new method it would. Outside the class its names are gone.
        names
            .iter()
            .all(|(n, evaluated)| {
                !self.hidden.contains(n)
                    && !self.class_names.contains(n)
                    && self.module_name(n, *evaluated, false)
            })
            .then_some(shown)
    }

    /// The type the file itself declares for `name` (sources 1 and 2): `None` when it declares
    /// none, `Some(None)` when it does but the text cannot be written on the new function.
    fn declared(&self, name: &str) -> Option<Option<String>> {
        if !self.own.contains(name) {
            return None;
        }
        if self.k_params.contains(name) {
            let parameters = self.k.and_then(|k| field(k, "parameters"))?;
            let annotation = named(parameters).into_iter().find_map(|p| {
                let n = match p.kind() {
                    "typed_parameter" => named(p).into_iter().next(),
                    "typed_default_parameter" => field(p, "name"),
                    _ => None,
                }?;
                (n.kind() == "identifier" && word(n, self.text) == name)
                    .then(|| field(p, "type"))
                    .flatten()
            })?;
            return Some(self.copied(annotation, Written::Parameter));
        }
        let mut found = Vec::new();
        annotations_of(self.k_body, self.text, name, &mut found);
        let first = *found.first()?;
        if found
            .iter()
            .any(|t| word(*t, self.text) != word(first, self.text))
        {
            return None;
        }
        Some(self.copied(first, Written::Variable))
    }

    /// A name the server's type uses, when the file can write it.
    fn server_name(&self, name: &str) -> Option<String> {
        let rebound =
            BUILTIN_TYPES.contains(&name) && self.module.iter().any(|(n, _, _)| n == name);
        (!matches!(name, "Unknown" | "Any")
            && !rebound
            && !self.hidden.contains(name)
            && !self.class_names.contains(name)
            && self.module_name(name, true, true))
        .then(|| name.to_owned())
    }

    /// The type `n` of a hover as an annotation, or `None` when it is not known for certain or
    /// cannot be written with the file's names.
    fn rendered(&self, n: Node, text: &str) -> Option<String> {
        match n.kind() {
            "identifier" => {
                let name = word(n, text);
                (!CONTAINERS.contains(&name))
                    .then(|| self.server_name(name))
                    .flatten()
            }
            "none" => Some("None".into()),
            "parenthesized_expression" => match named(n).as_slice() {
                [inner] => self.rendered(*inner, text),
                _ => None,
            },
            "binary_operator" if field(n, "operator").is_some_and(|o| o.kind() == "|") => {
                if !self.pipes {
                    return None;
                }
                let l = self.rendered(field(n, "left")?, text)?;
                let r = self.rendered(field(n, "right")?, text)?;
                Some(if l == r { l } else { format!("{l} | {r}") })
            }
            "subscript" => {
                let value = field(n, "value")?;
                if value.kind() != "identifier" {
                    return None;
                }
                let mut cursor = n.walk();
                let args: Vec<Node> = n.children_by_field_name("subscript", &mut cursor).collect();
                if args.is_empty() {
                    return None;
                }
                if word(value, text) == "Literal" {
                    // Widened to the type of the values, when they are of one kind.
                    let kind = |a: &Node| match a.kind() {
                        "integer" => Some("int"),
                        "unary_operator"
                            if named(*a).first().is_some_and(|x| x.kind() == "integer") =>
                        {
                            Some("int")
                        }
                        "true" | "false" => Some("bool"),
                        "string" => {
                            let bytes = children(*a)
                                .first()
                                .is_some_and(|s| word(*s, text).to_ascii_lowercase().contains('b'));
                            Some(if bytes { "bytes" } else { "str" })
                        }
                        _ => None,
                    };
                    let first = kind(&args[0])?;
                    return args
                        .iter()
                        .all(|a| kind(a) == Some(first))
                        .then(|| self.server_name(first))
                        .flatten();
                }
                let name = self.server_name(word(value, text))?;
                let mut parts = Vec::new();
                for a in args {
                    parts.push(if a.kind() == "ellipsis" {
                        "...".to_owned()
                    } else {
                        self.rendered(a, text)?
                    });
                }
                Some(format!("{name}[{}]", parts.join(", ")))
            }
            _ => None,
        }
    }

    /// The server's type text `shown` as an annotation (source 3).
    fn server_type(&self, shown: &str) -> Option<String> {
        let source = format!("({})\n", shown.trim());
        let doc = parse(&source).ok()?;
        let root = doc.tree().root_node();
        if root.has_error() {
            return None;
        }
        let [statement] = named(root)[..] else {
            return None;
        };
        let [expression] = named(statement)[..] else {
            return None;
        };
        (statement.kind() == "expression_statement")
            .then(|| self.rendered(expression, &source))
            .flatten()
    }

    /// The byte of the first read of `name` in the selection, when that read is the first use of
    /// the name the selection evaluates: the server's type there is the type at the call.
    fn first_read(&self, name: &str) -> Option<usize> {
        let at = self
            .reads
            .iter()
            .filter(|(n, _, assigned)| n == name && !assigned)
            .map(|(_, at, _)| *at)
            .min()?;
        let node = self
            .root
            .descendant_for_byte_range(at, at + name.len())
            .filter(|n| n.kind() == "identifier")?;
        let mut c = node;
        while let Some(p) = c.parent() {
            if p.start_byte() < self.sel.start || p.end_byte() > self.sel.end {
                break;
            }
            // The value of a conditional expression and the element of a comprehension stand
            // first and run after a condition that may have narrowed the name.
            let late = match p.kind() {
                "conditional_expression" => named(p).first().is_some_and(|v| v.id() == c.id()),
                k if COMPREHENSIONS.contains(&k) => {
                    !matches!(c.kind(), "for_in_clause" | "if_clause")
                }
                _ => false,
            };
            if late && occurrences(p, self.text, name) > 1 {
                return None;
            }
            c = p;
        }
        Some(at)
    }

    /// The type of the parameter `name` of the new function.
    fn parameter(&self, name: &str) -> Option<String> {
        if let Some(declared) = self.declared(name) {
            return declared;
        }
        let at = self.first_read(name)?;
        let code = hover_code(self.hovers.at(at)?);
        self.server_type(variable_hover(&code, name, false)?)
    }

    /// The type of the result `name`, which the selection assigns.
    fn result(&self, name: &str) -> Option<String> {
        if let Some(declared) = self.declared(name) {
            return declared;
        }
        let writes: Vec<usize> = self
            .writes
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, at)| *at)
            .collect();
        let [at] = writes[..] else {
            return None;
        };
        if !self.assigned_at_end.contains(name) {
            return None;
        }
        let code = hover_code(self.hovers.at(at)?);
        self.server_type(variable_hover(&code, name, false)?)
    }

    /// The type of the returned expression `n`.
    fn expression(&self, n: Node) -> Option<String> {
        match n.kind() {
            "parenthesized_expression" => match named(n).as_slice() {
                [inner] => self.expression(*inner),
                _ => None,
            },
            "identifier" => {
                let name = word(n, self.text);
                if let Some(declared) = self.declared(name) {
                    return declared;
                }
                let code = hover_code(self.hovers.at(n.start_byte())?);
                self.server_type(variable_hover(&code, name, false)?)
            }
            "attribute" => {
                let a = field(n, "attribute")?;
                let code = hover_code(self.hovers.at(a.start_byte())?);
                self.server_type(variable_hover(&code, word(a, self.text), true)?)
            }
            "call" => self.server_type(&self.called(n)?),
            "await" => {
                let [call] = named(n)[..] else {
                    return None;
                };
                if call.kind() != "call" {
                    return None;
                }
                let shown = self.called(call)?;
                let inner = ["CoroutineType[Any, Any, ", "Coroutine[Any, Any, "]
                    .iter()
                    .find_map(|p| shown.strip_prefix(p))?
                    .strip_suffix(']')?;
                self.server_type(inner)
            }
            _ => None,
        }
    }

    /// The declared result of the function a call calls, as the server shows it.
    fn called(&self, call: Node) -> Option<String> {
        let f = field(call, "function")?;
        let name = match f.kind() {
            "identifier" => f,
            "attribute" => field(f, "attribute")?,
            _ => return None,
        };
        let code = hover_code(self.hovers.at(name.start_byte())?);
        result_hover(&code).map(str::to_owned)
    }

    /// The return type for the returned `parts`, each typed by itself.
    fn returned(&self, parts: Vec<Option<String>>) -> Option<String> {
        let parts: Vec<String> = parts.into_iter().collect::<Option<_>>()?;
        match parts.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            _ => Some(format!(
                "{}[{}]",
                self.server_name("tuple")?,
                parts.join(", ")
            )),
        }
    }

    /// The parts of a returned expression: the items of a tuple, else the expression.
    fn parts(&self, n: Node) -> Vec<Option<String>> {
        match n.kind() {
            "expression_list" | "tuple" => {
                named(n).into_iter().map(|c| self.expression(c)).collect()
            }
            _ => vec![self.expression(n)],
        }
    }
}

fn push_unique(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|n| n == name) {
        list.push(name.to_owned());
    }
}

/// The plan of an Extract Method: what the new function takes, gives back and looks like.
struct Method {
    params: Vec<String>,
    results: Vec<String>,
    /// Called as `return extracted(…)`.
    returns: bool,
    is_async: bool,
}

fn method_edits(
    root: Node,
    text: &str,
    sel: &Bytes<usize>,
    hovers: &Hovers,
) -> Result<Vec<Edit>, String> {
    let picked = pick(root, text, sel)?;
    let (first, last) = match &picked {
        Picked::Statements(items) => (items[0], *items.last().expect("an item")),
        Picked::Expression(n) => (*n, *n),
    };
    syntax_check(first)?;
    if let Picked::Expression(n) = &picked {
        value_place(*n)?;
    }
    let block = match &picked {
        Picked::Statements(_) => first.parent(),
        Picked::Expression(n) => statement_of(*n).and_then(|s| s.parent()),
    };
    if block
        .and_then(|b| b.parent())
        .is_some_and(|p| p.kind() == "class_definition")
    {
        return Err("code directly in a class body cannot become a function".into());
    }
    if let Picked::Statements(_) = &picked
        && (!text[line_start(text, sel.start)..sel.start]
            .trim()
            .is_empty()
            || !ends_its_line(text, sel.end))
    {
        return Err("the selection shares its first or last line with other code".into());
    }
    let nodes: Vec<Node> = match &picked {
        Picked::Statements(items) => items.clone(),
        Picked::Expression(n) => vec![*n],
    };
    let mut scanned = Scan::default();
    for n in &nodes {
        scan(*n, text, false, &mut scanned);
    }
    if let Some(why) = scanned.refusal {
        return Err(why);
    }
    let sc = scopes(first, text);
    let k_body = match sc.k {
        Some(k) => field(k, "body").ok_or("the function has no body")?,
        None => root,
    };
    if contains_own(k_body, &["match_statement"]) {
        return Err(
            "the enclosing function has a `match` statement, whose captured names are not followed"
                .into(),
        );
    }
    if scanned.returns && sc.k.is_none() {
        return Err("it contains `return` outside a function".into());
    }
    let method = sc.k.and_then(|k| method_of(k, text));
    let receiver = method.as_ref().map(|m| m.1.clone());
    // A module-level function for code from inside a class loses the class's private names and
    // its `super`.
    if method.is_none() && scanned.class_cell {
        return Err(
            "it uses `super` or `__class__`, which a function outside a class does not have".into(),
        );
    }
    if method.is_none()
        && sc.in_class
        && let Some(name) = nodes.iter().find_map(|n| private_name(*n, text))
    {
        return Err(format!(
            "it uses the private name `{name}`, which is only mangled inside the class"
        ));
    }

    // The selection's own reads and writes.
    let mut flow = Flow::new(text);
    let mut end = St::Reach(HashSet::new());
    match &picked {
        Picked::Statements(items) => {
            for i in items {
                flow.stmt(*i, &mut end);
            }
        }
        Picked::Expression(n) => {
            if let St::Reach(da) = &mut end {
                flow.expr(*n, da, true);
            }
        }
    }
    let written: Vec<String> = {
        let mut list = Vec::new();
        for (name, _) in &flow.writes {
            push_unique(&mut list, name);
        }
        list
    };
    if let Some(name) = written.iter().find(|n| sc.k_declared.contains(*n)) {
        return Err(format!(
            "it assigns `{name}`, which the function declares `global` or `nonlocal`"
        ));
    }
    if let Some(r) = &receiver
        && written.contains(r)
    {
        return Err(format!("it assigns `{r}`, the method's first parameter"));
    }
    if matches!(end, St::Jump) {
        return Err("it contains `break` or `continue` of a loop outside the selection".into());
    }
    let exits = matches!(end, St::Exit);
    if scanned.returns && !exits {
        return Err("it returns on only some paths".into());
    }
    let assigned_at_end: HashSet<String> = match &end {
        St::Reach(da) => da.clone(),
        _ => HashSet::new(),
    };

    // Where the names of the enclosing function are bound.
    let k_bound = bound_of(k_body, text);
    let k_params: HashSet<String> =
        sc.k.and_then(|k| field(k, "parameters"))
            .map(|p| param_names(p, text))
            .unwrap_or_default()
            .into_iter()
            .collect();
    let outer_loop = {
        let mut found = None;
        let mut c = first;
        while let Some(p) = c.parent() {
            if p.id() == k_body.id() {
                break;
            }
            if matches!(p.kind(), "for_statement" | "while_statement") {
                found = Some(p);
            }
            c = p;
        }
        found
    };
    let in_selection = |at: usize| sel.start <= at && at < sel.end;
    let in_loop = |at: usize| outer_loop.is_some_and(|l| l.start_byte() <= at && at < l.end_byte());
    let k_local = |name: &str| match sc.k {
        Some(_) => sc.k_locals.contains(name),
        None => written.iter().any(|w| w == name),
    };
    let assigned_before = before(k_body, sel.start, text, k_params.clone());
    let maybe_before = |name: &str| {
        k_params.contains(name)
            || k_bound
                .names
                .iter()
                .any(|(n, at)| n == name && !in_selection(*at) && (*at < sel.start || in_loop(*at)))
            || (outer_loop.is_some() && written.iter().any(|w| w == name))
    };

    // Parameters: local names read where the selection has not assigned them itself.
    let mut params: Vec<String> = Vec::new();
    for (name, _, assigned) in &flow.reads {
        if *assigned || params.contains(name) {
            continue;
        }
        if sc.inner.contains(name) {
            params.push(name.clone());
        } else if k_local(name) {
            if assigned_before.contains(name) {
                params.push(name.clone());
            } else if maybe_before(name) {
                return Err(format!(
                    "`{name}` may or may not have a value when the selection starts"
                ));
            } else if !written.contains(name) {
                // Without the function's local name the new function would look the name up
                // in the module.
                return Err(format!(
                    "`{name}` is a local name that is assigned only after the selection"
                ));
            }
        } else if sc.outer.contains(name) {
            // A name of a function further out is taken to have a value.
            params.push(name.clone());
        }
    }

    // Results.
    let mut results: Vec<String> = Vec::new();
    if let Picked::Statements(_) = &picked {
        let after = after(first, last, k_body, text);
        if let Some(name) = written.iter().find(|n| after.handlers.contains(*n)) {
            return Err(format!(
                "`{name}` is read in an `except` or `finally` block around the selection, where \
                 it would not be assigned after an error"
            ));
        }
        // Nested functions outside the selection that share a name the selection assigns; at
        // module level these are all functions of the file, which read its names directly.
        let mut nested = Vec::new();
        closures(k_body, &mut nested);
        let mut shared_outside: HashSet<String> = HashSet::new();
        let mut captured_inside: HashSet<String> = HashSet::new();
        for c in nested {
            let mut free = Vec::new();
            deferred_free(c, text, &mut free);
            let inside = in_selection(c.start_byte());
            for (name, _) in free {
                if inside {
                    captured_inside.insert(name);
                } else {
                    shared_outside.insert(name);
                }
            }
        }
        if let Some(name) = written.iter().find(|n| shared_outside.contains(*n)) {
            return Err(format!(
                "`{name}` is also used by a function, lambda or generator outside the selection, \
                 which would not see the new function's assignment"
            ));
        }
        if !exits {
            let module = sc.k.is_none();
            for name in &written {
                let live = after.live.contains(name);
                if assigned_at_end.contains(name) {
                    // At module level an assigned name stays a module name.
                    if live || module {
                        results.push(name.clone());
                    }
                } else if assigned_before.contains(name) {
                    if live || module {
                        push_unique(&mut params, name);
                        results.push(name.clone());
                    }
                } else if live {
                    return Err(format!(
                        "`{name}` is assigned on only some paths of the selection and is read \
                         after it"
                    ));
                }
            }
        }
        // Inside a `try` with handlers an error in the middle of the selection is caught and
        // the function goes on: what the selection had assigned by then would be missing.
        let caught = {
            let mut found = false;
            let mut c = first;
            while let Some(p) = c.parent() {
                if c.id() == k_body.id() {
                    break;
                }
                found |= p.kind() == "try_statement"
                    && is_field(p, "body", c)
                    && named(p)
                        .iter()
                        .any(|x| matches!(x.kind(), "except_clause" | "except_group_clause"));
                c = p;
            }
            found
        };
        if caught {
            // Safe only for a name assigned once, by the selection's last statement, a plain
            // assignment to names: nothing can fail after it.
            let plain = |n: Node| {
                let names = |l: Node| {
                    l.kind() == "identifier"
                        || (matches!(l.kind(), "pattern_list" | "tuple_pattern")
                            && named(l).iter().all(|x| x.kind() == "identifier"))
                };
                matches!(n.kind(), "assignment" | "augmented_assignment")
                    && field(n, "left").is_some_and(names)
                    && field(n, "right").is_some_and(|r| r.kind() != "assignment")
            };
            let last_assigns = last.kind() == "expression_statement"
                && named(last).first().is_some_and(|c| plain(*c));
            for name in &written {
                if !(after.live.contains(name) || results.contains(name)) {
                    continue;
                }
                let writes: Vec<usize> = flow
                    .writes
                    .iter()
                    .filter(|(n, _)| n == name)
                    .map(|(_, at)| *at)
                    .collect();
                if exits || !last_assigns || writes.len() != 1 || writes[0] < last.start_byte() {
                    return Err(format!(
                        "`{name}` is assigned inside a `try` before the selection ends; after an \
                         error in a later line the `except` block would go on without it"
                    ));
                }
            }
        }
        let rebound_later = |name: &str| {
            k_bound
                .names
                .iter()
                .any(|(n, at)| n == name && !in_selection(*at) && (*at >= sel.end || in_loop(*at)))
        };
        if let Some(name) = captured_inside
            .iter()
            .find(|n| results.contains(*n) || (k_local(n) && rebound_later(n)))
        {
            return Err(format!(
                "a nested function, lambda or generator inside the selection uses `{name}`, which \
                 is assigned again outside it"
            ));
        }
    } else {
        // An expression: a lambda or generator in it keeps the new function's copy of a name.
        let mut nested = Vec::new();
        closures(first, &mut nested);
        if matches!(first.kind(), "lambda" | "generator_expression") {
            nested.push(first);
        }
        let mut free = Vec::new();
        for c in nested {
            deferred_free(c, text, &mut free);
        }
        let rebound = |name: &str| {
            k_bound
                .names
                .iter()
                .any(|(n, at)| n == name && (*at >= sel.end || in_loop(*at)))
        };
        if let Some((name, _)) = free.iter().find(|(n, _)| k_local(n) && rebound(n)) {
            return Err(format!(
                "a lambda or generator inside the selection uses `{name}`, which is assigned \
                 again after it"
            ));
        }
    }
    if let Some(r) = &receiver {
        params.retain(|p| p != r);
    }
    let plan = Method {
        params,
        results,
        returns: scanned.returns && exits,
        is_async: scanned.awaits,
    };

    // The texts.
    let nl = eol(text);
    let mut words = HashSet::new();
    all_identifiers(root, text, true, &mut words);
    let name = unique("extracted", &words);
    let unit = indent_unit(root, text);
    let (def_indent, body_indent) = match (&method, sc.k) {
        (Some((stmt, _, _)), Some(k)) => {
            let def = indent_at(text, stmt.start_byte()).to_owned();
            let body = field(k, "body")
                .and_then(|b| named(b).into_iter().next())
                .filter(|s| starts_its_line(text, *s))
                .map(|s| indent_at(text, s.start_byte()).to_owned())
                .unwrap_or_else(|| format!("{def}{unit}"));
            (def, body)
        }
        _ => (String::new(), unit.clone()),
    };
    // Types known for certain (behaviour 16); the others stay out.
    let types = Types::new(
        root,
        text,
        sel,
        first,
        &sc,
        k_body,
        method.is_some(),
        hovers,
        &flow,
        &assigned_at_end,
    );
    let returned = match &picked {
        Picked::Expression(n) => types.returned(types.parts(*n)),
        Picked::Statements(_) if plan.returns => {
            let mut found = Vec::new();
            for n in &nodes {
                own_returns(*n, &mut found);
            }
            match found[..] {
                [only] => named(only)
                    .first()
                    .and_then(|v| types.returned(types.parts(*v))),
                _ => None,
            }
        }
        Picked::Statements(_) => {
            types.returned(plan.results.iter().map(|r| types.result(r)).collect())
        }
    }
    .map(|t| format!(" -> {t}"))
    .unwrap_or_default();
    let mut signature: Vec<String> = receiver.iter().cloned().collect();
    signature.extend(plan.params.iter().map(|p| match types.parameter(p) {
        Some(t) => format!("{p}: {t}"),
        None => p.clone(),
    }));
    let mut def = String::new();
    if let Some((_, _, true)) = &method {
        def.push_str(&format!("{def_indent}@classmethod{nl}"));
    }
    def.push_str(&format!(
        "{def_indent}{}def {name}({}){returned}:{nl}",
        if plan.is_async { "async " } else { "" },
        signature.join(", ")
    ));
    let mut call = format!(
        "{}{name}({})",
        receiver
            .as_ref()
            .map(|r| format!("{r}."))
            .unwrap_or_default(),
        plan.params.join(", ")
    );
    if plan.is_async {
        call = format!("await {call}");
    }
    let replaced: Bytes<usize>;
    let replacement: String;
    match &picked {
        Picked::Statements(_) => {
            // Comments the selection starts or ends with move along.
            let from = line_start(text, sel.start);
            let to = sel.end;
            let old = indent_at(text, first.start_byte()).to_owned();
            let mut strings = Vec::new();
            for n in &nodes {
                multi_line_strings(*n, &mut strings);
            }
            def.push_str(&reindent(text, from..to, &old, &body_indent, &strings));
            if !plan.results.is_empty() {
                def.push_str(&format!(
                    "{nl}{body_indent}return {}",
                    plan.results.join(", ")
                ));
            }
            replaced = from..to;
            replacement = if plan.returns {
                format!("{old}return {call}")
            } else if plan.results.is_empty() {
                format!("{old}{call}")
            } else {
                format!("{old}{} = {call}", plan.results.join(", "))
            };
        }
        Picked::Expression(n) => {
            def.push_str(&format!("{body_indent}return {}", standalone(*n, text)));
            replaced = n.byte_range();
            let call = if plan.is_async {
                format!("({call})")
            } else {
                call
            };
            replacement = in_place(*n, text, &call);
        }
    }
    match &method {
        Some((stmt, _, _)) => {
            // After the method, as a member of the same class.
            let at = line_end(text, stmt.end_byte());
            let method = format!("{nl}{nl}{def}");
            if at == replaced.end {
                // The selection ends the method: one edit, not two that touch.
                Ok(vec![edit(text, replaced, format!("{replacement}{method}"))])
            } else {
                Ok(vec![
                    edit(text, replaced, replacement),
                    edit(text, at..at, method),
                ])
            }
        }
        None => {
            // Before the top-level statement, above the comment lines attached to it.
            let mut top = top_level_of(first);
            while let Some(prev) = top.prev_named_sibling() {
                if prev.kind() == "comment"
                    && prev.end_position().row + 1 == top.start_position().row
                    && starts_its_line(text, prev)
                {
                    top = prev;
                } else {
                    break;
                }
            }
            let at = line_start(text, top.start_byte());
            // Two empty lines around a top-level function.
            let above = text[..at].trim_end_matches([' ', '\t']);
            let spaced = above.is_empty() || above.ends_with("\n\n") || above.ends_with("\n\r\n");
            let lead = if spaced {
                String::new()
            } else {
                format!("{nl}{nl}")
            };
            let def = format!("{lead}{def}{nl}{nl}{nl}");
            if at == replaced.start {
                Ok(vec![edit(text, replaced, format!("{def}{replacement}"))])
            } else {
                Ok(vec![
                    edit(text, at..at, def),
                    edit(text, replaced, replacement),
                ])
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------------------------

/// The edits of Extract Variable for the selection `range` of `text`, or why it is refused.
pub fn extract_variable(text: &str, range: &Range) -> Result<Vec<Edit>, String> {
    let doc = parse(text)?;
    variable_edits(doc.tree().root_node(), text, &selection(text, range))
}

/// The edits of Extract Method for the selection `range` of `text`, or why it is refused. The
/// new function's types come from the annotations of the text alone (behaviour 16, sources 1
/// and 2).
pub fn extract_method(text: &str, range: &Range) -> Result<Vec<Edit>, String> {
    let doc = parse(text)?;
    method_edits(
        doc.tree().root_node(),
        text,
        &selection(text, range),
        &Hovers::default(),
    )
}

/// The places (0-based line and character) whose hover text from the Python language server
/// would tell Extract Method a type the text does not declare (behaviour 16, source 3). Empty
/// when the extraction is refused or needs none.
pub fn type_questions(text: &str, range: &Range) -> Vec<(u64, u64)> {
    let Ok(doc) = parse(text) else {
        return Vec::new();
    };
    let hovers = Hovers::default();
    if method_edits(
        doc.tree().root_node(),
        text,
        &selection(text, range),
        &hovers,
    )
    .is_err()
    {
        return Vec::new();
    }
    hovers
        .asked
        .into_inner()
        .into_iter()
        .map(|at| {
            let r = range_of(text, at..at);
            (r.start_line, r.start_character)
        })
        .collect()
}

/// The edits of the own action titled `title`; a refusal is the message of [`refusal`].
pub fn plan(text: &str, range: &Range, title: &str) -> Result<Vec<Edit>, String> {
    plan_typed(text, range, title, &[])
}

/// As [`plan`], with the server's hover texts for places of [`type_questions`]: Extract Method
/// writes the types they give where they are certain and the file can name them.
pub fn plan_typed(
    text: &str,
    range: &Range,
    title: &str,
    answers: &[((u64, u64), String)],
) -> Result<Vec<Edit>, String> {
    if title == INLINE_VARIABLE {
        return crate::python_inline::plan(text, range);
    }
    let (result, what) = if title == EXTRACT_VARIABLE {
        (extract_variable(text, range), "a variable")
    } else if title == EXTRACT_CONSTANT {
        (
            crate::python_constant::extract_constant(text, range),
            "a constant",
        )
    } else {
        let hovers = Hovers {
            answers: answers
                .iter()
                .map(|((line, character), hover)| (byte_of(text, *line, *character), hover.clone()))
                .collect(),
            asked: RefCell::default(),
        };
        let edits = parse(text).and_then(|doc| {
            method_edits(
                doc.tree().root_node(),
                text,
                &selection(text, range),
                &hovers,
            )
        });
        (edits, "a method")
    };
    result.map_err(|why| message(text, &selection(text, range), what, &why))
}

pub(crate) fn message(text: &str, sel: &Bytes<usize>, what: &str, why: &str) -> String {
    let t = text[sel.clone()].trim();
    let first = t.lines().next().unwrap_or_default();
    let short: String = first.chars().take(40).collect();
    let shown = if short.len() < t.len() {
        format!("`{short}…`")
    } else {
        format!("`{short}`")
    };
    format!("Cannot extract {shown} into {what}: {why}.")
}

pub(crate) fn action(title: &str) -> CodeAction {
    CodeAction {
        title: title.to_owned(),
        kind: Some("refactor.extract".into()),
        preferred: false,
        disabled: None,
        fixes: Vec::new(),
    }
}

/// The expressions around the caret at byte `at`, inside out, and their statement last.
pub(crate) fn around(root: Node, text: &str, at: usize) -> Vec<(Bytes<usize>, bool)> {
    let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let range = if wordy(text[..at].chars().next_back()) && !wordy(text[at..].chars().next()) {
        at - 1..at
    } else {
        at..(at + 1).min(text.len())
    };
    let mut out: Vec<(Bytes<usize>, bool)> = Vec::new();
    let mut node = root.descendant_for_byte_range(range.start, range.end);
    while let Some(n) = node {
        node = n.parent();
        if is_statement(n) {
            out.push((n.byte_range(), true));
            break;
        }
        let callee = n
            .parent()
            .is_some_and(|p| p.kind() == "call" && is_field(p, "function", n));
        if n.is_named()
            && EXPRESSIONS.contains(&n.kind())
            && n.kind() != "identifier"
            && !callee
            && out.last().is_none_or(|(r, _)| *r != n.byte_range())
        {
            out.push((n.byte_range(), false));
        }
    }
    out
}

/// What the own refactorings find for `range` of `text`: the places and their actions, or the
/// message that says why there is none (behaviours 2, 3, 14, 17 and 22). `refactoring` is
/// [`Refactoring::Variable`], [`Refactoring::Method`], [`Refactoring::Constant`],
/// [`Refactoring::Inline`] or [`Refactoring::Any`]; others find nothing and have no message.
pub fn find(
    text: &str,
    range: &Range,
    refactoring: Refactoring,
) -> (Vec<RefactoringTarget>, Option<String>) {
    let Ok(doc) = parse(text) else {
        return (Vec::new(), None);
    };
    let root = doc.tree().root_node();
    let variable = |r: &Bytes<usize>| variable_edits(root, text, r);
    let hovers = Hovers::default();
    let method = |r: &Bytes<usize>| method_edits(root, text, r, &hovers);
    let constant = |r: &Bytes<usize>| crate::python_constant::constant_edits(root, text, r);
    let target = |r: &Bytes<usize>, titles: Vec<&str>| RefactoringTarget {
        range: range_of(text, r.clone()),
        text: text[r.clone()].to_owned(),
        actions: titles.into_iter().map(action).collect(),
    };
    let sel = selection(text, range);
    let caret = byte_of(text, range.start_line, range.start_character)
        == byte_of(text, range.end_line, range.end_character);
    match refactoring {
        Refactoring::Any => {
            let mut titles = Vec::new();
            if !caret && variable(&sel).is_ok() {
                titles.push(EXTRACT_VARIABLE);
            }
            if !caret && method(&sel).is_ok() {
                titles.push(EXTRACT_METHOD);
            }
            if !caret && constant(&sel).is_ok() {
                titles.push(EXTRACT_CONSTANT);
            }
            // Inline Variable is offered on a variable's name, selected or under the caret.
            let (inline, _) = crate::python_inline::find(root, text, range);
            if titles.is_empty() {
                (inline, None)
            } else {
                if inline
                    .first()
                    .is_some_and(|t| t.range == range_of(text, sel.clone()))
                {
                    titles.push(INLINE_VARIABLE);
                }
                (vec![target(&sel, titles)], None)
            }
        }
        Refactoring::Inline => crate::python_inline::find(root, text, range),
        Refactoring::Variable | Refactoring::Method | Refactoring::Constant => {
            let is_method = refactoring == Refactoring::Method;
            let (title, what) = match refactoring {
                Refactoring::Variable => (EXTRACT_VARIABLE, "a variable"),
                Refactoring::Constant => (EXTRACT_CONSTANT, "a constant"),
                _ => (EXTRACT_METHOD, "a method"),
            };
            let attempt = |r: &Bytes<usize>| match refactoring {
                Refactoring::Variable => variable(r),
                Refactoring::Constant => constant(r),
                _ => method(r),
            };
            if !caret {
                return match attempt(&sel) {
                    Ok(_) => (vec![target(&sel, vec![title])], None),
                    Err(why) => (Vec::new(), Some(message(text, &sel, what, &why))),
                };
            }
            let at = byte_of(text, range.start_line, range.start_character);
            let places = around(root, text, at);
            let mut found = Vec::new();
            let mut first_refusal = None;
            for (r, statement) in &places {
                if *statement && !is_method {
                    continue;
                }
                match attempt(r) {
                    Ok(_) => found.push(target(r, vec![title])),
                    Err(why) if first_refusal.is_none() && !statement => {
                        first_refusal = Some(message(text, r, what, &why))
                    }
                    Err(_) => {}
                }
                if found.len() == MAX_PLACES {
                    break;
                }
            }
            let none = found.is_empty().then(|| {
                first_refusal
                    .unwrap_or_else(|| "There is no expression at the caret to extract.".into())
            });
            (found, none)
        }
        _ => (Vec::new(), None),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::rename::apply_edits;

    /// The source without the selection marks `«` and `»` (or the caret mark `¦`), and the
    /// selection.
    pub(crate) fn marked(source: &str) -> (String, Range) {
        let text = source.replace(['«', '»', '¦'], "");
        let (a, b) = match source.find('¦') {
            Some(at) => (at, at),
            None => (
                source.find('«').expect("a start mark"),
                source.find('»').expect("an end mark") - '«'.len_utf8(),
            ),
        };
        let range = range_of(&text, a..b);
        (text, range)
    }

    fn variable(source: &str) -> Result<String, String> {
        let (text, range) = marked(source);
        extract_variable(&text, &range).map(|e| apply_edits(&text, &e).unwrap())
    }

    fn method(source: &str) -> Result<String, String> {
        let (text, range) = marked(source);
        extract_method(&text, &range).map(|e| apply_edits(&text, &e).unwrap())
    }

    #[track_caller]
    fn refused(result: Result<String, String>, reason: &str) {
        match result {
            Ok(text) => panic!("not refused ({reason}):\n{text}"),
            Err(why) => assert!(why.contains(reason), "{why:?} does not name {reason:?}"),
        }
    }

    const PRICES: &str =
        "class Item:\n    def total(self):\n        return «self.price * self.qty»\n";

    #[test]
    fn the_observed_expression() {
        let after = variable(PRICES).unwrap();
        assert_eq!(
            after,
            "class Item:\n    def total(self):\n        qty = self.price * self.qty\n        return qty\n"
        );
        let (before, _) = marked(PRICES);
        let name = crate::actions::introduced_name(&before, &after).unwrap();
        assert_eq!(
            (name.name.as_str(), name.line, name.character),
            ("qty", 2, 8)
        );
        // The selection may carry white space around it.
        let spaced = "class Item:\n    def total(self):\n        return« self.price * self.qty»\n";
        assert_eq!(variable(spaced).unwrap(), after);
    }

    #[test]
    fn variable_edit_shapes() {
        // The statement's indentation, also with tabs.
        assert_eq!(
            variable("def f(a):\n\tif a:\n\t\treturn «a * 2» + 1\n").unwrap(),
            "def f(a):\n\tif a:\n\t\tvalue = a * 2\n\t\treturn value + 1\n"
        );
        // Several lines get parentheses; parentheses of their own are enough.
        assert_eq!(
            variable("x = max(«a.p +\n        a.q», 3)\n").unwrap(),
            "q = (a.p +\n        a.q)\nx = max(q, 3)\n"
        );
        assert_eq!(
            variable("x = max(«(a.p +\n         a.q)», 3)\n").unwrap(),
            "q = (a.p +\n         a.q)\nx = max(q, 3)\n"
        );
        // A generator that is the only argument, selected with or without the parentheses.
        for source in [
            "print(sum(«x * 2 for x in xs»))\n",
            "print(sum«(x * 2 for x in xs)»)\n",
        ] {
            assert_eq!(
                variable(source).unwrap(),
                "value = (x * 2 for x in xs)\nprint(sum(value))\n"
            );
        }
        // The name does not run into a word.
        assert_eq!(
            variable("if not«(a.flag)»:\n    pass\n").unwrap(),
            "flag = (a.flag)\nif not flag:\n    pass\n"
        );
        assert_eq!(
            variable("x = d if«(a.b)»else c\n").unwrap(),
            "b = (a.b)\nx = d if b else c\n"
        );
        // A whole expression statement becomes the assignment.
        assert_eq!(
            variable("def f():\n    «self.run(1)»\n").unwrap(),
            "def f():\n    run = self.run(1)\n"
        );
        // The file's line ending.
        assert_eq!(
            variable("def f(a):\r\n    return «a.b + 1»\r\n").unwrap(),
            "def f(a):\r\n    b = a.b + 1\r\n    return b\r\n"
        );
        // Before the whole statement, not before the line of the expression.
        assert_eq!(
            variable("x = f(\n    1,\n    «a.b * 2»,\n)\n").unwrap(),
            "b = a.b * 2\nx = f(\n    1,\n    b,\n)\n"
        );
        // The expression starts its statement at the start of the line.
        assert_eq!(
            variable("«a.load()».save()\n").unwrap(),
            "load = a.load()\nload.save()\n"
        );
        assert_eq!(
            variable("def f(a):\n    «a.load()».save()\n").unwrap(),
            "def f(a):\n    load = a.load()\n    load.save()\n"
        );
        // Characters outside ASCII before the selection.
        assert_eq!(
            variable("s = 'größe' + «a.b * 2»\n").unwrap(),
            "b = a.b * 2\ns = 'größe' + b\n"
        );
    }

    #[test]
    fn variable_names() {
        let name = |expression: &str| {
            let after = variable(&format!(
                "def f(self, count, items):\n    return «{expression}»\n"
            ))
            .unwrap();
            let line = after.lines().nth(1).unwrap().trim();
            line.split(" = ").next().unwrap().to_owned()
        };
        assert_eq!(name("self.price * self.qty"), "qty");
        assert_eq!(name("self.get_total()"), "total");
        assert_eq!(name("self.get_()"), "get_");
        assert_eq!(name("Item(1)"), "item");
        assert_eq!(name("HTTPServer(1)"), "http_server");
        assert_eq!(name("self.items[0]"), "items1");
        assert_eq!(name("self.rows[0]"), "rows");
        assert_eq!(name("-self.x"), "x");
        assert_eq!(name("not self.done"), "done");
        assert_eq!(name("self.a > self.b"), "b");
        assert_eq!(name("self.a + 1"), "a");
        assert_eq!(name("(self.fetch())"), "fetch");
        assert_eq!(name("self._cache"), "cache");
        assert_eq!(name("self.__class__"), "class__");
        // No name in the expression.
        assert_eq!(name("count * 2"), "value");
        assert_eq!(name("[1, 2]"), "value");
        assert_eq!(name("items[0]"), "value");
        // Taken names: a number; keywords and built-in names: an underscore.
        assert_eq!(name("self.count"), "count1");
        assert_eq!(name("self.type"), "type_");
        assert_eq!(name("len(items)"), "len_");
        assert_eq!(name("self.pass_"), "pass_");
        // Names after a dot do not count as taken, names of the whole function do.
        let after =
            variable("def f(a):\n    value = a.size\n    return «a.size * 2» + a.value\n").unwrap();
        assert!(after.contains("    size = a.size * 2\n"), "{after}");
        let after = variable("def f(a):\n    return «a * 2»\n    value = 1\n").unwrap();
        assert!(after.contains("    value1 = a * 2\n"), "{after}");
        // At module level the whole file counts.
        let after = variable("def g():\n    return total\n\n\nx = «a.total + 1»\n").unwrap();
        assert!(
            after.contains("\ntotal1 = a.total + 1\nx = total1\n"),
            "{after}"
        );
    }

    #[test]
    fn variable_refusals() {
        const PART: &str = "not one complete expression";
        const TARGET: &str = "target of an assignment";
        let cases: &[(&str, &str)] = &[
            // Behaviour 4.
            ("x = a + «b + c»\n", PART),
            ("s = 'ab«c d»e'\n", PART),
            ("x = «a +» b\n", PART),
            ("f(«*args»)\n", PART),
            ("f(«**kw»)\n", PART),
            ("x = y[«1:2»]\n", PART),
            ("x = {«'a': 1»}\n", PART),
            ("«return»\n", PART),
            ("x = «»1\n", "nothing is selected"),
            ("x = a.«b»\n", "after a dot"),
            ("f(«k»=1)\n", "keyword argument"),
            ("def f(«a»):\n    pass\n", "parameter"),
            ("import «os»\n", "imported or declared"),
            ("from a import «b»\n", "imported or declared"),
            ("def f():\n    global «n»\n", "imported or declared"),
            ("def f():\n    x = «(yield 1)»\n", "`yield`"),
            ("if «(n := g())»:\n    pass\n", "`:=`"),
            ("«x» = 1\n", TARGET),
            ("a[«f(i)»] = 1\n", TARGET),
            ("«a.b» += 1\n", TARGET),
            ("for «i» in xs:\n    pass\n", TARGET),
            ("with open(p) as «fh»:\n    pass\n", TARGET),
            ("del «a[0]»\n", TARGET),
            ("try:\n    pass\nexcept E as «e»:\n    pass\n", TARGET),
            ("x = [y for «y» in ys]\n", TARGET),
            ("def f(a=«g()»):\n    pass\n", "default value"),
            ("f = lambda a=«g()»: a\n", "default value"),
            ("def f(a: «int»):\n    pass\n", "annotation"),
            ("x: «int» = 1\n", "annotation"),
            ("def f() -> «int»:\n    pass\n", "annotation"),
            ("@«deco(1)»\ndef f():\n    pass\n", "decorator"),
            ("class A(«Base»):\n    pass\n", "header"),
            ("def «f»():\n    pass\n", "header"),
            ("match v:\n    case «1»:\n        pass\n", "`case`"),
            ("match v:\n    case y if «y > 1»:\n        pass\n", "`case`"),
            ("def f(:\n    return «a + b»\n", "syntax errors"),
            // Behaviour 7.
            ("f = lambda x: «x * k» + 1\n", "`lambda`"),
            ("ys = [«x * k» for x in xs]\n", "comprehension"),
            ("ys = [x for x in xs if «x > k»]\n", "comprehension"),
            ("ys = [y for x in xs for y in «g(x)»]\n", "comprehension"),
            ("ys = {x: «g(k)» for x in xs}\n", "comprehension"),
            ("t = sum(«g(k)» for x in xs)\n", "comprehension"),
            ("x = a and «b()»\n", "`and`/`or`"),
            ("x = a or «b()»\n", "`and`/`or`"),
            ("x = «a()» if c else b\n", "conditional expression"),
            ("x = a if c else «b()»\n", "conditional expression"),
            ("x = a < b < «c()»\n", "chained comparison"),
            ("while «f(x)»:\n    pass\n", "`while`"),
            ("if a:\n    pass\nelif «f(x)»:\n    pass\n", "`elif`"),
            ("try:\n    pass\nexcept «errors()»:\n    pass\n", "`except`"),
            ("assert «f(x)», 'no'\n", "`assert`"),
            ("assert x, «g(x)»\n", "`assert`"),
            ("with a() as x, «b(x)» as y:\n    pass\n", "`with`"),
            ("class A:\n    x = «f(1)» + 2\n", "class body"),
            ("if x: return «f(y)»\n", "does not start its line"),
            ("a = 1; b = «f(a)»\n", "does not start its line"),
            ("print((n := 5), «n + 1»)\n", "`:=` binds"),
            // Behaviour 8: a part evaluated before can have side effects.
            ("total = first() + «second()»\n", "before `first()`"),
            ("print(first(), «second()»)\n", "before `first()`"),
            ("f(a=g(), b=«h()»)\n", "before `g()`"),
            ("first().m(«g()»)\n", "before `first()`"),
            ("x = [first(), «second()»]\n", "before `first()`"),
            ("x = d[first()] + «second()»\n", "before `first()`"),
            ("a[g()] += «f()»\n", "before `g()`"),
            ("x = {k(): «v()»}\n", "before `k()`"),
            ("x = a(1) < «b(2)»\n", "before `a(1)`"),
            (
                "async def f():\n    return await a() + «await b()»\n",
                "before `await a()`",
            ),
        ];
        for (source, reason) in cases {
            let (text, range) = marked(source);
            match extract_variable(&text, &range) {
                Ok(e) => panic!("{source:?} is not refused: {e:?}"),
                Err(why) => assert!(why.contains(reason), "{source:?}: {why:?}"),
            }
        }
    }

    #[test]
    fn variable_allowed_neighbours() {
        let cases = [
            // The first iterable of a comprehension, also of a generator.
            "ys = [x for x in «g(k)» if x]\n",
            "t = sum(x for x in «g(k)»)\n",
            // The condition of a conditional expression; the left side of `and`.
            "x = a if «c()» else b\n",
            "x = «a()» and b\n",
            // The first two operands of a chained comparison.
            "x = «a()» < b < c\n",
            "x = a < «b()» < c\n",
            // Conditions and iterables that are evaluated once.
            "if «f(x)»:\n    pass\nelif y:\n    pass\n",
            "for i in «f(x)»:\n    pass\n",
            "with «a()» as x, b(x) as y:\n    pass\n",
            "match «f(v)»:\n    case 1:\n        pass\n",
            // A lambda or a comprehension as a whole.
            "f = «lambda x: x * 2»\n",
            "ys = «[x * 2 for x in xs]»\n",
            // The right side of an assignment, an argument, a value in a display.
            "a.b = «f(1)»\n",
            "x = {'a': «f(1)»}\n",
            "x += «f(1)»\n",
            "return_ = g(k=«f(1)»)\n",
            // The first statement of a line with two.
            "b = «f(a)»; c = 2\n",
            // Inside a function inside a class.
            "class A:\n    def m(self):\n        return «self.f(1)» + 2\n",
            // `await`.
            "async def f():\n    return «await g()» + 1\n",
            // Behaviour 8: what runs before has no side effects, or the selection has none.
            "total = first() + «self.price * self.qty»\n",
            "x = «first()» + second()\n",
            "x = a.b + «g()»\n",
            "x = a[0] + «g()»\n",
            "d[k()] = «make()»\n",
            "x = obj.method(«g()»)\n",
            "x = [lambda: first(), «g()»]\n",
            "x = first() + «-n»\n",
        ];
        for source in cases {
            let (text, _) = marked(source);
            let after = variable(source).unwrap_or_else(|e| panic!("{source:?}: {e}"));
            assert_ne!(after, text);
            assert!(
                !parse(&after).unwrap().tree().root_node().has_error(),
                "{after}"
            );
        }
    }

    #[test]
    fn method_parameters_and_results() {
        // Read before, assigned and read after.
        assert_eq!(
            method("def f(a, b):\n    «total = a + b\n    total *= 2»\n    return total\n")
                .unwrap(),
            "def extracted(a, b):\n    total = a + b\n    total *= 2\n    return total\n\n\ndef f(a, b):\n    total = extracted(a, b)\n    return total\n"
        );
        // Nothing is read after: no result. A name of the module is no parameter.
        assert_eq!(
            method("import sys\n\n\ndef f(a):\n    «b = a * 2\n    print(b, file=sys.stderr)»\n")
                .unwrap(),
            "import sys\n\n\ndef extracted(a):\n    b = a * 2\n    print(b, file=sys.stderr)\n\n\ndef f(a):\n    extracted(a)\n"
        );
        // Two results in the order of their first assignment; parameters in the order read.
        let after =
            method("def f(q, p):\n    «hi = max(p, q)\n    lo = min(p)»\n    return hi - lo\n")
                .unwrap();
        assert!(after.contains("def extracted(p, q):\n"), "{after}");
        assert!(after.contains("    return hi, lo\n"), "{after}");
        assert!(after.contains("    hi, lo = extracted(p, q)\n"), "{after}");
        // A name the selection assigns before it reads it is no parameter, although it had a
        // value; one it reads first is.
        let after = method(
            "def f(n):\n    x = 1\n    y = 1\n    «x = n\n    y = y + x»\n    return x + y\n",
        )
        .unwrap();
        assert!(after.contains("def extracted(n, y):\n"), "{after}");
        assert!(after.contains("    x, y = extracted(n, y)\n"), "{after}");
        // Assigned on some paths with a value before: passed in and returned.
        let after = method("def f(n):\n    label = 'small'\n    «if n > 2:\n        label = 'big'»\n    return label\n").unwrap();
        assert!(after.contains("def extracted(n, label):\n"), "{after}");
        assert!(
            after.contains("    label = extracted(n, label)\n"),
            "{after}"
        );
        // Assigned again after the selection before it is read: no result.
        let after = method("def f(xs):\n    «for i in xs:\n        print(i)»\n    for i in xs:\n        print(i)\n").unwrap();
        assert!(after.contains("\n    extracted(xs)\n"), "{after}");
        // In a loop the next round reads what this round assigned.
        let after = method("def f(xs):\n    total = 0\n    for x in xs:\n        «total = total + x»\n    return 0\n").unwrap();
        assert!(
            after.contains("        total = extracted(total, x)\n"),
            "{after}"
        );
        // A loop variable and a `with` name of statements around are parameters.
        let after = method("def f(p):\n    with open(p) as fh:\n        for line in fh:\n            «print(line, fh.name)»\n").unwrap();
        assert!(after.contains("def extracted(line, fh):\n"), "{after}");
        // Names of a lambda, a comprehension and an outer function around an expression.
        let after =
            method("def f(k):\n    return [(lambda y: «x * y + k»)(1) for x in range(3)]\n")
                .unwrap();
        assert!(
            after.contains("def extracted(x, y, k):\n    return x * y + k\n"),
            "{after}"
        );
        assert!(
            after.contains("(lambda y: extracted(x, y, k))(1)"),
            "{after}"
        );
        // Names bound inside the expression are none.
        let after = method("def f(k):\n    return «[x * k for x in range(3)]»\n").unwrap();
        assert!(after.contains("def extracted(k):\n"), "{after}");
        // A name declared `global` is read as the module's.
        let after = method("def f():\n    global n\n    «x = n + 1»\n    return x\n").unwrap();
        assert!(after.contains("def extracted():\n"), "{after}");
        // A name of an enclosing function, also when declared `nonlocal` and only read.
        let after = method("def outer(k):\n    def inner(x):\n        nonlocal k\n        «y = x * k»\n        return y\n    return inner\n").unwrap();
        assert!(after.contains("def extracted(x, k):\n"), "{after}");
        // What a nested function of the selection reads from outside is a parameter.
        let after =
            method("def f(k):\n    «def add(x):\n        return x + k»\n    return add\n").unwrap();
        assert!(after.contains("def extracted(k):\n"), "{after}");
        assert!(
            after.contains("    return add\n\n\ndef f(k):\n    add = extracted(k)\n"),
            "{after}"
        );
        // A `try` whose handler leaves: the body's assignment holds after it.
        let after = method(
            "def f(s):\n    «try:\n        n = int(s)\n    except ValueError:\n        return None»\n    return n\n",
        );
        refused(after, "returns on only some paths");
        let after = method("def f(s):\n    «try:\n        n = int(s)\n    except ValueError:\n        raise KeyError(s)»\n    return n\n").unwrap();
        assert!(after.contains("    n = extracted(s)\n"), "{after}");
    }

    #[test]
    fn method_at_module_level() {
        // Assigned names stay module names; the function stands before the statements.
        assert_eq!(
            method("xs = [3, 1]\n«ys = sorted(xs)\nfirst = ys[0]»\n").unwrap(),
            "xs = [3, 1]\n\n\ndef extracted():\n    ys = sorted(xs)\n    first = ys[0]\n    return ys, first\n\n\nys, first = extracted()\n"
        );
        // A name read before its assignment is passed in.
        let after = method("count = 0\n«count = count + 1»\n").unwrap();
        assert!(after.contains("def extracted(count):\n"), "{after}");
        assert!(after.ends_with("count = extracted(count)\n"), "{after}");
        // A loop variable nothing reads afterwards is dropped.
        let after = method("xs = [1]\n«for x in xs:\n    print(x)»\nprint('done')\n").unwrap();
        assert!(
            after.contains("\n\n\nextracted()\nprint('done')\n"),
            "{after}"
        );
        refused(
            method("xs = [1]\n«for x in xs:\n    print(x)»\nprint(x)\n"),
            "`x` is assigned on only some paths",
        );
        // Inside `if __name__ …` the function stands before the `if`.
        let after = method("if __name__ == '__main__':\n    «a = 1\n    print(a)»\n").unwrap();
        assert!(
            after.starts_with(
                "def extracted():\n    a = 1\n    print(a)\n    return a\n\n\nif __name__"
            ),
            "{after}"
        );
        // An expression.
        assert_eq!(
            method("x = «a.b * 2» + 1\n").unwrap(),
            "def extracted():\n    return a.b * 2\n\n\nx = extracted() + 1\n"
        );
    }

    #[test]
    fn method_return_and_await() {
        assert_eq!(
            method("def f(n):\n    «if n > 2:\n        return 'big'\n    else:\n        return 'small'»\n").unwrap(),
            "def extracted(n):\n    if n > 2:\n        return 'big'\n    else:\n        return 'small'\n\n\ndef f(n):\n    return extracted(n)\n"
        );
        let after =
            method("def f(n):\n    «if n > 2:\n        return 'big'\n    raise ValueError(n)»\n")
                .unwrap();
        assert!(after.contains("    return extracted(n)\n"), "{after}");
        // Only `raise`: a plain call.
        let after = method("def f(n):\n    if n:\n        «m = str(n)\n        raise ValueError(m)»\n    return 1\n").unwrap();
        assert!(
            after.contains("        extracted(n)\n    return 1\n"),
            "{after}"
        );
        // `return` of a nested function is its own.
        let after = method("def f(n):\n    «def g():\n        return n»\n    return g\n").unwrap();
        assert!(after.contains("    g = extracted(n)\n"), "{after}");
        // `await` of its own makes the function `async`.
        assert_eq!(
            method("async def main(n):\n    «a = await twice(n)»\n    return a\n").unwrap(),
            "async def extracted(n):\n    a = await twice(n)\n    return a\n\n\nasync def main(n):\n    a = await extracted(n)\n    return a\n"
        );
        let after = method("async def main(n):\n    return «await twice(n)» + 1\n").unwrap();
        assert!(
            after.contains("    return (await extracted(n)) + 1\n"),
            "{after}"
        );
        let after =
            method("async def main(xs):\n    «async for x in xs:\n        print(x)»\n").unwrap();
        assert!(after.contains("async def extracted(xs):\n"), "{after}");
        let after =
            method("async def main(xs):\n    «ys = [x async for x in xs]»\n    return ys\n")
                .unwrap();
        assert!(after.contains("    ys = await extracted(xs)\n"), "{after}");
        // `await` of a nested function does not.
        let after = method("async def main(n):\n    «async def g():\n        return await twice(n)»\n    return await g()\n").unwrap();
        assert!(after.starts_with("def extracted(n):\n"), "{after}");
        assert!(after.contains("    g = extracted(n)\n"), "{after}");
    }

    #[test]
    fn method_in_a_class() {
        // A method after the current one, called through `self`, also without using it.
        assert_eq!(
            method("class Item:\n    def total(self, tax):\n        «net = self.price * self.qty\n        gross = net * (1 + tax)»\n        return gross\n\n    def name(self):\n        return 'item'\n").unwrap(),
            "class Item:\n    def total(self, tax):\n        gross = self.extracted(tax)\n        return gross\n\n    def extracted(self, tax):\n        net = self.price * self.qty\n        gross = net * (1 + tax)\n        return gross\n\n    def name(self):\n        return 'item'\n"
        );
        let after =
            method("class Box:\n    def area(this, w, h):\n        return «w * h»\n").unwrap();
        assert!(after.ends_with("        return this.extracted(w, h)\n\n    def extracted(this, w, h):\n        return w * h\n"), "{after}");
        // After a decorated method, behind its last line's comment; `super` stays usable.
        let after = method("class Box(Base):\n    @property\n    def area(self):\n        «a = super().area»\n        return a  # done\n").unwrap();
        assert!(after.ends_with("        return a  # done\n\n    def extracted(self):\n        a = super().area\n        return a\n"), "{after}");
        // A class method gives a class method.
        let after = method("class Box:\n    @classmethod\n    def make(cls, n):\n        «b = cls(n)»\n        return b\n").unwrap();
        assert!(after.contains("        b = cls.extracted(n)\n"), "{after}");
        assert!(after.contains("\n\n    @classmethod\n    def extracted(cls, n):\n        b = cls(n)\n        return b"), "{after}");
        // A static method and a function inside a method give a module-level function.
        let after = method("class Box:\n    @staticmethod\n    def double(n):\n        «m = n * 2»\n        return m\n").unwrap();
        assert!(
            after.starts_with("def extracted(n):\n    m = n * 2\n    return m\n\n\nclass Box:\n"),
            "{after}"
        );
        assert!(after.contains("        m = extracted(n)\n"), "{after}");
        let after = method("class Box:\n    def run(self):\n        def inner(x):\n            «y = x + self.k»\n            return y\n        return inner\n").unwrap();
        assert!(
            after.starts_with(
                "def extracted(x, self):\n    y = x + self.k\n    return y\n\n\nclass Box:\n"
            ),
            "{after}"
        );
        // A selection that ends the method: the call and the new method are one edit.
        let (text, range) = marked("class Box:\n    def run(self):\n        «print(self.w)»\n");
        let edits = extract_method(&text, &range).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(
            apply_edits(&text, &edits).unwrap(),
            "class Box:\n    def run(self):\n        self.extracted()\n\n    def extracted(self):\n        print(self.w)\n"
        );
        // The name is free in the whole file.
        let after =
            method("class Box:\n    def extracted(self):\n        return «self.w * 2»\n").unwrap();
        assert!(after.contains("return self.extracted1()"), "{after}");
    }

    #[test]
    fn method_text_shapes() {
        // Lines of a string over several lines keep their text; continuation lines move.
        assert_eq!(
            method("def f(name):\n    if name:\n        «text = \"\"\"Dear\n    {}\n  bye\"\"\".format(\n            name)»\n        return text\n").unwrap(),
            "def extracted(name):\n    text = \"\"\"Dear\n    {}\n  bye\"\"\".format(\n        name)\n    return text\n\n\ndef f(name):\n    if name:\n        text = extracted(name)\n        return text\n"
        );
        // Comments the selection starts and ends with move along; one behind it stays.
        let after = method("def f(a):\n    # first\n    «# doubles\n    b = a * 2  # twice\n    # last»\n    return b  # end\n").unwrap();
        assert!(after.starts_with("def extracted(a):\n    # doubles\n    b = a * 2  # twice\n    # last\n    return b\n\n\n"), "{after}");
        assert!(
            after.ends_with("def f(a):\n    # first\n    b = extracted(a)\n    return b  # end\n"),
            "{after}"
        );
        let after = method("def f(a):\n    «b = a * 2»  # twice\n    return b\n").unwrap();
        assert!(after.contains("    b = extracted(a)  # twice\n"), "{after}");
        // The file's indentation step and line ending.
        assert_eq!(
            method("def f(a):\r\n  if a:\r\n    «b = a * 2»\r\n    return b\r\n").unwrap(),
            "def extracted(a):\r\n  b = a * 2\r\n  return b\r\n\r\n\r\ndef f(a):\r\n  if a:\r\n    b = extracted(a)\r\n    return b\r\n"
        );
        let after = method("def f(a):\n\tif a:\n\t\t«b = a * 2»\n\t\treturn b\n").unwrap();
        assert!(
            after.starts_with("def extracted(a):\n\tb = a * 2\n\treturn b\n"),
            "{after}"
        );
        // Above the comment lines and decorators attached to the top-level statement.
        let after = method(
            "import os\n\n\n# The area.\n@cache\ndef area(r):\n    «sq = r * r»\n    return sq\n",
        )
        .unwrap();
        assert!(after.starts_with("import os\n\n\ndef extracted(r):\n    sq = r * r\n    return sq\n\n\n# The area.\n@cache\ndef area(r):\n"), "{after}");
        // An expression over several lines, a generator argument, a touching word.
        let after = method("def f(a, b):\n    return max(«a +\n               b», 3)\n").unwrap();
        assert!(
            after.starts_with("def extracted(a, b):\n    return (a +\n               b)\n"),
            "{after}"
        );
        let after = method("def f(xs):\n    return sum(«x * x for x in xs»)\n").unwrap();
        assert!(
            after.contains("    return (x * x for x in xs)\n"),
            "{after}"
        );
        assert!(after.contains("    return sum(extracted(xs))\n"), "{after}");
        let after = method("def f(a):\n    return not«(a)»\n").unwrap();
        assert!(after.contains("    return not extracted(a)\n"), "{after}");
        // A whole statement counts as a statement, with white space around the selection.
        let after = method("def f(a):\n«    print(a)\n»    return a\n").unwrap();
        assert!(
            after.starts_with(
                "def extracted(a):\n    print(a)\n\n\ndef f(a):\n    extracted(a)\n    return a\n"
            ),
            "{after}"
        );
    }

    #[test]
    fn method_refusals() {
        const WHAT: &str = "complete statements";
        let cases: &[(&str, &str)] = &[
            ("def f():\n    «x = 1\n    y» = 2\n", WHAT),
            ("def f(a):\n    if a:\n        «x = 1\n    y = 2»\n", WHAT),
            (
                "def f(a):\n    «if a:\n        x = 1\n    else:»\n        x = 2\n",
                WHAT,
            ),
            (
                "def f(v):\n    match v:\n        «case 1:\n            pass»\n",
                WHAT,
            ),
            (
                "def f():\n    x = 1; «y = 2»\n",
                "shares its first or last line",
            ),
            (
                "def f():\n    «x = 1»; y = 2\n",
                "shares its first or last line",
            ),
            (
                "def f(a):\n    if a: «return 1»\n",
                "shares its first or last line",
            ),
            ("class A:\n    «x = 1»\n", "class body"),
            ("class A:\n    x = «f(1)» + 2\n", "class body"),
            (
                "def f(a):\n    «if a:\n        return 1\n    b = 2»\n    return b\n",
                "returns on only some paths",
            ),
            (
                "def f(a):\n    «for x in a:\n        return x»\n",
                "returns on only some paths",
            ),
            (
                "def f(a):\n    «with a:\n        return 1»\n",
                "returns on only some paths",
            ),
            ("def f(a):\n    «yield a»\n", "`yield`"),
            ("def f(a):\n    «x = yield a»\n", "`yield`"),
            ("def f(a):\n    «yield from a»\n", "`yield`"),
            (
                "def f(xs):\n    for x in xs:\n        «if x:\n            break»\n",
                "`break` or `continue`",
            ),
            (
                "def f(xs):\n    for x in xs:\n        «if x:\n            continue\n        print(x)»\n",
                "`break` or `continue`",
            ),
            (
                "def f(xs):\n    for x in xs:\n        «for y in x:\n            pass\n        else:\n            break»\n",
                "`break` or `continue`",
            ),
            (
                "def f():\n    «global n\n    n = 1»\n",
                "`global` or `nonlocal` statement",
            ),
            (
                "def f():\n    n = 0\n    def g():\n        «nonlocal n\n        n = 1»\n",
                "`global` or `nonlocal` statement",
            ),
            (
                "def f():\n    global n\n    «n = 1»\n",
                "declares `global` or `nonlocal`",
            ),
            (
                "def f():\n    n = 0\n    def g():\n        nonlocal n\n        «n += 1»\n",
                "declares `global` or `nonlocal`",
            ),
            ("def f(a):\n    «del a»\n", "`del`"),
            ("def f(a, b):\n    «del a, b[0]»\n", "`del`"),
            (
                "«from __future__ import annotations»\n",
                "`from __future__ import`",
            ),
            ("«from os import *»\n", "`from … import *`"),
            ("def f(a):\n    «print(locals())»\n", "`locals`"),
            ("def f(a):\n    «x = eval('a')»\n    return x\n", "`eval`"),
            ("def f(a):\n    return «vars()»\n", "`vars`"),
            ("def f(a):\n    «exec('b = a')»\n", "`exec`"),
            ("def f(a):\n    return «dir()»\n", "`dir`"),
            (
                "def f(v):\n    match v:\n        case [x]:\n            «print(x)»\n",
                "`match`",
            ),
            (
                "def f(v):\n    match v:\n        case [x]:\n            pass\n    «print(v)»\n",
                "`match`",
            ),
            (
                "def f(c):\n    if c:\n        n = 1\n    «print(n)»\n",
                "`n` may or may not have a value",
            ),
            (
                "def f(xs):\n    for x in xs:\n        «if x:\n            n = x\n        print(n)»\n",
                "`n` may or may not have a value",
            ),
            (
                "def f(xs):\n    for x in xs:\n        «print(n)»\n        n = x\n",
                "`n` may or may not have a value",
            ),
            (
                "def f(c):\n    «if c:\n        n = 1»\n    if c:\n        print(n)\n",
                "`n` is assigned on only some paths",
            ),
            (
                "def f(xs):\n    «for x in xs:\n        pass»\n    return x\n",
                "`x` is assigned on only some paths",
            ),
            (
                "def f():\n    try:\n        «x = g()\n        y = h(x)»\n    except E:\n        print(x)\n",
                "`x` is read in an `except` or `finally`",
            ),
            (
                "def f():\n    try:\n        «fh = open()\n        return fh.read()»\n    finally:\n        fh.close()\n",
                "`fh` is read in an `except` or `finally`",
            ),
            (
                "def f():\n    try:\n        pass\n    except E:\n        «x = 1»\n    finally:\n        print(x)\n",
                "`x` is read in an `except` or `finally`",
            ),
            (
                "def f():\n    def show():\n        return n\n    «n = 5»\n    return show()\n",
                "`n` is also used by a function",
            ),
            (
                "def f():\n    «n = 5»\n    return lambda: n\n",
                "`n` is also used by a function",
            ),
            (
                "def f(xs):\n    «n = 5»\n    return (x * n for x in xs)\n",
                "`n` is also used by a function",
            ),
            (
                "def show():\n    return n\n\n\n«n = 5»\nshow()\n",
                "`n` is also used by a function",
            ),
            (
                "class A:\n    def m(self):\n        return n\n\n\n«n = 5»\n",
                "`n` is also used by a function",
            ),
            (
                "def f():\n    «n = 1\n    g = lambda: n»\n    return g, n\n",
                "uses `n`, which is assigned again",
            ),
            (
                "def f(n):\n    «g = lambda: n»\n    n = 2\n    return g\n",
                "uses `n`, which is assigned again",
            ),
            (
                "def f(xs, n):\n    for x in xs:\n        «g = lambda: n»\n        n = x\n    return g\n",
                "uses `n`, which is assigned again",
            ),
            (
                "def f(n):\n    g = «lambda: n»\n    n = 2\n    return g\n",
                "uses `n`, which is assigned again",
            ),
            (
                "def f(xs, n):\n    g = «(x * n for x in xs)»\n    n = 2\n    return g\n",
                "uses `n`, which is assigned again",
            ),
            (
                "class A:\n    def m(self, other):\n        «self = other»\n        return self\n",
                "the method's first parameter",
            ),
            (
                "class A:\n    @staticmethod\n    def m():\n        «x = A.__secret»\n        return x\n",
                "private name `__secret`",
            ),
            (
                "class A:\n    def m(self):\n        def inner():\n            «v = super(A, self).m()»\n            return v\n        return inner\n",
                "`super` or `__class__`",
            ),
            (
                "class A:\n    @staticmethod\n    def m():\n        return «__class__»\n",
                "`super` or `__class__`",
            ),
            ("def f():\n    return «super()»\n", "`super` or `__class__`"),
            ("«return 1»\n", "outside a function"),
            (
                "def f(xs):\n    a = 0\n    for x in xs:\n        try:\n            «a = g(x)\n            b = h(a)»\n        except E:\n            continue\n        print(b)\n    return a\n",
                "`a` is assigned inside a `try` before the selection ends",
            ),
            (
                "def f(x):\n    a = 0\n    try:\n        «a = 1\n        a = g(x)»\n    except E:\n        pass\n    return a\n",
                "`a` is assigned inside a `try` before the selection ends",
            ),
            (
                "def f(x):\n    a = 0\n    try:\n        «a = g(x)\n        raise E(a)»\n    except E:\n        pass\n    return a\n",
                "`a` is assigned inside a `try` before the selection ends",
            ),
            (
                "def f(x):\n    a = 0\n    try:\n        if x:\n            «a = b[0] = g(x)»\n    except E:\n        pass\n    return a\n",
                "`a` is assigned inside a `try` before the selection ends",
            ),
            (
                "def f():\n    «print(n)»\n    n = 1\n",
                "`n` is a local name that is assigned only after",
            ),
            (
                "def f():\n    «def a():\n        return b()»\n    def b():\n        return 1\n    return a\n",
                "`b` is a local name that is assigned only after",
            ),
            (
                "def f():\n    g = lambda: «helper(1)»\n    def helper(x):\n        return x\n    return g\n",
                "`helper` is a local name that is assigned only after",
            ),
            (
                "class A:\n    def __new__(cls):\n        «x = cls.__secret»\n        return x\n",
                "private name `__secret`",
            ),
            // What is no expression of its own (behaviour 4) holds for a method too.
            ("def f(a):\n    «a.b» = 1\n", "target of an assignment"),
            ("def f(a=«g()»):\n    pass\n", "default value"),
            ("def f(a):\n    return a if «(n := a)» else 0\n", "`:=`"),
            ("def f(:\n    «x = 1»\n", "syntax errors"),
        ];
        for (source, reason) in cases {
            let (text, range) = marked(source);
            match extract_method(&text, &range) {
                Ok(e) => panic!(
                    "{source:?} is not refused:\n{}",
                    apply_edits(&text, &e).unwrap()
                ),
                Err(why) => assert!(why.contains(reason), "{source:?}: {why:?}"),
            }
        }
    }

    #[test]
    fn caret_places() {
        let texts = |source: &str, refactoring| {
            let (text, range) = marked(source);
            let (found, message) = find(&text, &range, refactoring);
            assert_eq!(found.is_empty(), message.is_some(), "{message:?}");
            found
                .into_iter()
                .map(|t| {
                    assert_eq!(t.actions.len(), 1);
                    assert!(refactoring.matches(&t.actions[0]));
                    assert_eq!(crate::actions::text_of(&text, &t.range), t.text);
                    t.text
                })
                .collect::<Vec<_>>()
        };
        const LINE: &str = "def f(s):\n    return add(s.ar¦ea(), 1) + 2\n";
        // Inside out, without the plain name and the callee `s.area`.
        assert_eq!(
            texts(LINE, Refactoring::Variable),
            ["s.area()", "add(s.area(), 1)", "add(s.area(), 1) + 2"]
        );
        // Extract Method also offers the statement.
        assert_eq!(
            texts(LINE, Refactoring::Method),
            [
                "s.area()",
                "add(s.area(), 1)",
                "add(s.area(), 1) + 2",
                "return add(s.area(), 1) + 2"
            ]
        );
        // At the end of a word and before one.
        assert_eq!(
            texts("x = a.b¦ + 1\n", Refactoring::Variable),
            ["a.b", "a.b + 1"]
        );
        assert_eq!(
            texts("x = ¦a.b + 1\n", Refactoring::Variable),
            ["a.b", "a.b + 1"]
        );
        // Refused places are left out: the right side of `and` stays, the whole does not go.
        assert_eq!(
            texts("x = a and b.c¦() + 1\n", Refactoring::Variable),
            ["a and b.c() + 1"]
        );
        // Nothing to offer.
        assert!(texts("x = ¦1\n", Refactoring::Variable).len() == 1);
        assert!(texts("pa¦ss\n", Refactoring::Variable).is_empty());
        assert!(texts("\n¦\nx = 1\n", Refactoring::Method).is_empty());
        // Refactor This lists both for a selection, nothing of its own for a bare caret.
        let (text, range) = marked("def f(a):\n    return «a.b * 2»\n");
        let (found, message) = find(&text, &range, Refactoring::Any);
        assert!(message.is_none());
        let titles: Vec<&str> = found[0].actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, [EXTRACT_VARIABLE, EXTRACT_METHOD]);
        let (text, range) = marked("def f(a):\n    «x = a.b * 2»\n");
        let (found, _) = find(&text, &range, Refactoring::Any);
        let titles: Vec<&str> = found[0].actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, [EXTRACT_METHOD]);
        let (text, range) = marked("def f(a):\n    return a.b¦ * 2\n");
        assert!(find(&text, &range, Refactoring::Any).0.is_empty());
        // Extract Field has no own action.
        assert_eq!(find(&text, &range, Refactoring::Field), (Vec::new(), None));
    }

    #[test]
    fn messages() {
        let message = |source: &str, refactoring| {
            let (text, range) = marked(source);
            find(&text, &range, refactoring).1.unwrap()
        };
        assert_eq!(
            message("x = a and «b()»\n", Refactoring::Variable),
            "Cannot extract `b()` into a variable: the right side of `and`/`or` is evaluated only sometimes there."
        );
        assert_eq!(
            message("def f(a):\n    «yield a»\n", Refactoring::Method),
            "Cannot extract `yield a` into a method: it contains `yield`, which would turn the new function into a generator."
        );
        // Long and several lines: the first line, at most 40 characters.
        assert_eq!(
            message(
                "class A:\n    «total_of_everything = first_value + second_value\n    y = 2»\n",
                Refactoring::Method
            ),
            "Cannot extract `total_of_everything = first_value + seco…` into a method: code directly in a class body cannot become a function."
        );
        // A bare caret: the innermost expression's reason, or that there is none.
        assert_eq!(
            message("while a.b¦():\n    pass\n", Refactoring::Variable)
                .split(':')
                .next()
                .unwrap(),
            "Cannot extract `a.b()` into a variable"
        );
        assert_eq!(
            message("pa¦ss\n", Refactoring::Variable),
            "There is no expression at the caret to extract."
        );
        // `plan` fails with the same text.
        let (text, range) = marked("x = a and «b()»\n");
        assert_eq!(
            plan(&text, &range, EXTRACT_VARIABLE).unwrap_err(),
            message("x = a and «b()»\n", Refactoring::Variable)
        );
        let (text, range) = marked("def f(a):\n    «yield a»\n");
        assert!(
            plan(&text, &range, EXTRACT_METHOD)
                .unwrap_err()
                .contains("into a method")
        );
        // The titles are found as the refactorings they are, and introduce a name.
        for (title, refactoring) in [
            (EXTRACT_VARIABLE, Refactoring::Variable),
            (EXTRACT_METHOD, Refactoring::Method),
        ] {
            assert!(refactoring.matches(&action(title)));
            assert!(crate::actions::introduces_name(title));
            assert!(is_own(Path::new("a.py"), title));
            assert!(!is_own(Path::new("a.rs"), title));
        }
        assert!(offers(Path::new("a.pyi"), Refactoring::Any));
        assert!(!offers(Path::new("a.py"), Refactoring::Field));
        assert!(!offers(Path::new("a.ts"), Refactoring::Variable));
    }

    /// The line of the new function's `def` after Extract Method, with the server's hover
    /// `answers` by hovered name (SPEC-0330 v3 behaviour 16).
    #[track_caller]
    fn signature(source: &str, answers: &[(&str, &str)]) -> String {
        let (text, range) = marked(source);
        let hovers: Vec<((u64, u64), String)> = type_questions(&text, &range)
            .into_iter()
            .filter_map(|(line, character)| {
                let at = byte_of(&text, line, character);
                let word: String = text[at..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                answers
                    .iter()
                    .find(|(name, _)| *name == word)
                    .map(|(_, hover)| ((line, character), (*hover).to_owned()))
            })
            .collect();
        let edits = plan_typed(&text, &range, EXTRACT_METHOD, &hovers).unwrap();
        let after = apply_edits(&text, &edits).unwrap();
        after
            .lines()
            .find(|l| l.contains("def extracted("))
            .unwrap_or_else(|| panic!("no new function in\n{after}"))
            .trim()
            .to_owned()
    }

    #[test]
    fn method_types_from_the_text() {
        // A parameter's annotation and a variable's annotation are copied; a parameter without a
        // known type stays plain next to them; nothing returned, no return type.
        assert_eq!(
            signature(
                "def main(argv: list[str], n) -> int:\n    count: int = 0\n    «print(argv, n, count)»\n    return 0\n",
                &[]
            ),
            "def extracted(argv: list[str], n, count: int):"
        );
        // The order stays the order of the first read.
        assert_eq!(
            signature("def f(q: int, p: str = ''):\n    «print(p, q)»\n", &[]),
            "def extracted(p: str, q: int):"
        );
        // `*args: int` is the type of one element.
        assert_eq!(
            signature(
                "def f(*args: int, **kw: str):\n    «print(args, kw)»\n",
                &[]
            ),
            "def extracted(args, kw):"
        );
        // A class of the enclosing function is not there at module level.
        assert_eq!(
            signature(
                "def outer():\n    class P:\n        pass\n\n    def inner(x: P, y: int):\n        «print(x, y)»\n    return inner\n",
                &[]
            ),
            "def extracted(x, y: int):"
        );
        // A name bound below the function is not there yet when the `def` runs; a variable's
        // annotation is never evaluated where it stood. With the future import nothing is.
        let below =
            "def f(make):\n    x: Later = make()\n    «print(x)»\n\n\nclass Later:\n    pass\n";
        assert_eq!(signature(below, &[]), "def extracted(x):");
        assert_eq!(
            signature(
                &format!("from __future__ import annotations\n\n\n{below}"),
                &[]
            ),
            "def extracted(x: Later):"
        );
        // Imported for type checkers only.
        assert_eq!(
            signature(
                "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:\n    from a import Item\n\n\ndef f(make):\n    x: \"Item\" = make()\n    y: Item = make()\n    «print(x, y)»\n",
                &[]
            ),
            "def extracted(x: \"Item\", y):"
        );
        // A string is not evaluated; its names must still be the file's.
        assert_eq!(
            signature(
                "def f(n: \"int | None\", m: \"Missing\", k: \"list[Literal['r']]\"):\n    «print(n, m, k)»\n",
                &[]
            ),
            "def extracted(n: \"int | None\", m, k):"
        );
        // A new method keeps its parameter's annotation as it is; a variable's annotation that
        // the class body would give another meaning is not written.
        assert_eq!(
            signature(
                "class Unit:\n    pass\n\n\nclass C:\n    Unit = int\n\n    def m(self, a: Unit, c: \"C\"):\n        b: Unit = a\n        «print(a, b, c)»\n",
                &[]
            ),
            "def extracted(self, a: Unit, b, c: \"C\"):"
        );
        // Out of a static method the class body's names are gone.
        assert_eq!(
            signature(
                "class C:\n    Alias = int\n\n    @staticmethod\n    def s(n: Alias, m: int):\n        «print(n, m)»\n",
                &[]
            ),
            "def extracted(n, m: int):"
        );
        // A parameter without annotation is not typed by a later declaration of its name.
        assert_eq!(
            signature(
                "def f(x):\n    «print(x)»\n    x: int = 3\n    return x\n",
                &[]
            ),
            "def extracted(x):"
        );
        // Two declarations that differ decide nothing.
        assert_eq!(
            signature(
                "def f(c):\n    if c:\n        x: int = 1\n    else:\n        x: str = ''\n    «print(x)»\n",
                &[]
            ),
            "def extracted(x):"
        );
        // At module level, and a declared result as the return type.
        assert_eq!(
            signature(
                "count: int = 0\nfor step in (1, 2):\n    «count = count + step»\nprint(count)\n",
                &[]
            ),
            "def extracted(count: int) -> int:"
        );
        assert_eq!(
            signature(
                "def f(a: int, b):\n    «if b:\n        return a\n    raise ValueError(b)»\n",
                &[]
            ),
            "def extracted(b, a: int) -> int:"
        );
    }

    #[test]
    fn method_types_from_the_server() {
        // OBS-0034 item 52: the annotated parameter from the text, the list from the server, as
        // Markdown or plain text.
        let observed = "from app.prices import Item, order_total\n\n\ndef main(argv: list[str]) -> int:\n    items = [Item(\"pen\", 1.5, 4), Item(\"book\", 12.0)]\n    «print(f\"total {order_total(items):.2f} for {argv[1:]}\")»\n    return 0\n";
        for hover in [
            "```python\n(variable) items: list[Item]\n```",
            "(variable) items: list[Item]\n\nThe items.",
        ] {
            assert_eq!(
                signature(observed, &[("items", hover)]),
                "def extracted(items: list[Item], argv: list[str]):"
            );
        }
        // Without an answer the parameter stays plain.
        assert_eq!(
            signature(observed, &[]),
            "def extracted(items, argv: list[str]):"
        );
        let one = "from m import Item\n\n\ndef f(a):\n    «print(a)»\n";
        // Written.
        for (hover, written) in [
            ("(parameter) a: Item", "Item"),
            (
                "(parameter) a: dict[str, list[Item]]",
                "dict[str, list[Item]]",
            ),
            ("(parameter) a: tuple[int, ...]", "tuple[int, ...]"),
            ("(parameter) a: type[Item]", "type[Item]"),
            ("(parameter) a: None", "None"),
            ("(variable) a: Literal[5]", "int"),
            ("(variable) a: Literal[-1, 2]", "int"),
            ("(variable) a: Literal['a', 'b']", "str"),
            ("(variable) a: Literal[b'a']", "bytes"),
            ("(constant) a: Literal[True]", "bool"),
            (
                "(variable) a: tuple[Literal[1], Literal['a']]",
                "tuple[int, str]",
            ),
        ] {
            assert_eq!(
                signature(one, &[("a", hover)]),
                format!("def extracted(a: {written}):"),
                "{hover}"
            );
        }
        // Not written: unknown, partly unknown, not nameable in the file, not a variable.
        for hover in [
            "(parameter) a: Unknown",
            "(parameter) a: Any",
            "(parameter) a: list[Unknown]",
            "(parameter) a: dict[str, Any]",
            "(parameter) a: Item | Unknown",
            "(parameter) a: m.Item",
            "(parameter) a: list[m.Item]",
            "(parameter) a: Other",
            "(parameter) a: list[Other]",
            "(parameter) a: list",
            "(parameter) a: tuple",
            "(parameter) a: int | None",
            "(parameter) a: (x: int) -> str",
            "(parameter) a: Self@Item",
            "(parameter) a: Literal[Color.RED]",
            "(parameter) a: Literal[1, 'a']",
            "(parameter) a: Module(\"m\")",
            "(parameter) a: ...",
            "(parameter) b: int",
            "(function) def a(x: int) -> int",
            "(variable) def a(x: int) -> int",
            "(module) a",
            "class a()",
            "",
        ] {
            assert_eq!(
                signature(one, &[("a", hover)]),
                "def extracted(a):",
                "{hover}"
            );
        }
        // A name the file binds twice, binds below, or hides.
        let item = [("a", "(parameter) a: Item")];
        for source in [
            "try:\n    from m import Item\nexcept ImportError:\n    from n import Item\n\n\ndef f(a):\n    «print(a)»\n",
            "def f(a):\n    «print(a)»\n\n\nclass Item:\n    pass\n",
            "from m import Item\n\n\ndef f(a):\n    Item = 3\n    «print(a)»\n",
            "from m import Item\n\n\ndef f[Item](a):\n    «print(a)»\n",
            "from m import Item\n\n\nclass C:\n    class Item:\n        pass\n\n    def m(self, a):\n        «print(a)»\n",
            "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:\n    from m import Item\n\n\ndef f(a):\n    «print(a)»\n",
        ] {
            assert!(
                signature(source, &item).ends_with("a):"),
                "{source}: {}",
                signature(source, &item)
            );
        }
        assert_eq!(
            signature(
                "from __future__ import annotations\n\n\ndef f(a):\n    «print(a)»\n\n\nclass Item:\n    pass\n",
                &item
            ),
            "def extracted(a: Item):"
        );
        // A built-in name the file binds itself is no longer the built-in type.
        assert_eq!(
            signature(
                "str = 5\n\n\ndef f(a):\n    «print(a)»\n",
                &[("a", "(parameter) a: str")]
            ),
            "def extracted(a):"
        );
        // A `|` union only where the file shows that it can be evaluated.
        let union = [("a", "(variable) a: Literal['a'] | None")];
        assert_eq!(signature(one, &union), "def extracted(a):");
        assert_eq!(
            signature(
                "def g(x: int | None):\n    return x\n\n\ndef f(a):\n    «print(a)»\n",
                &union
            ),
            "def extracted(a: str | None):"
        );
        assert_eq!(
            signature(
                "from __future__ import annotations\n\n\ndef f(a):\n    «print(a)»\n",
                &[("a", "(parameter) a: (int | None)")]
            ),
            "def extracted(a: int | None):"
        );
        // A read the server may already have narrowed is not asked about: the condition runs
        // first although it stands behind.
        let narrowed = [("o", "(parameter) o: Item")];
        for source in [
            "from m import Item\n\n\ndef f(o):\n    «xs = [o.p for _ in range(3) if o is not None]\n    print(xs)»\n",
            "from m import Item\n\n\ndef f(o):\n    «w = o.p if o else 0\n    print(w)»\n",
        ] {
            let (text, range) = marked(source);
            assert_eq!(type_questions(&text, &range), Vec::new(), "{source}");
            assert_eq!(signature(source, &narrowed), "def extracted(o):");
        }
        // The first read in the order of evaluation is asked about.
        assert_eq!(
            signature(
                "from m import Item\n\n\ndef f(o):\n    «w = 0 if o is None else o.p\n    print(w)»\n",
                &[("o", "(parameter) o: Item")]
            ),
            "def extracted(o: Item):"
        );
        // A refused selection asks nothing.
        let (text, range) = marked("def f(a):\n    «yield a»\n");
        assert_eq!(type_questions(&text, &range), Vec::new());
    }

    #[test]
    fn method_return_types() {
        let total = ("total", "(method) def total() -> float\n\nThe total.");
        // A call of a method, a function, an attribute.
        assert_eq!(
            signature("def f(a):\n    return «a.total()» + 1\n", &[total]),
            "def extracted(a) -> float:"
        );
        assert_eq!(
            signature(
                "def f(a):\n    return «conv(a)» + 'x'\n",
                &[(
                    "conv",
                    "```python\n(function) def conv(\n    x: int\n) -> str\n```\n---\nConverts."
                )]
            ),
            "def extracted(a) -> str:"
        );
        assert_eq!(
            signature(
                "def f(a):\n    return «a.name» + 'x'\n",
                &[("name", "(property) name: str")]
            ),
            "def extracted(a) -> str:"
        );
        // Not written: an unknown result, several signatures, a class, an operator.
        for (source, answers) in [
            (
                "def f(a):\n    return «plain(a)» + 1\n",
                vec![(
                    "plain",
                    "(function) def plain(\n    a: Unknown\n) -> Unknown",
                )],
            ),
            (
                "def f(a):\n    return «conv(a)» + 1\n",
                vec![(
                    "conv",
                    "(function)\ndef conv(x: int) -> str\ndef conv(x: str) -> int",
                )],
            ),
            (
                "def f(a):\n    return «Item(a)».p\n",
                vec![("Item", "class Item(\n    p: float\n)")],
            ),
            (
                "def f(a):\n    return «a * 2» + 1\n",
                vec![("a", "(parameter) a: int")],
            ),
            (
                "def f(a):\n    return «twice(a)»\n",
                vec![(
                    "twice",
                    "(function) def twice(n: int) -> CoroutineType[Any, Any, int]",
                )],
            ),
        ] {
            let found = signature(source, &answers);
            assert!(!found.contains("->"), "{source}: {found}");
        }
        // An awaited call gives the coroutine's result.
        assert_eq!(
            signature(
                "async def g(n: int):\n    return «await twice(n)» + 1\n",
                &[(
                    "twice",
                    "(function) def twice(n: int) -> CoroutineType[Any, Any, int]"
                )]
            ),
            "async def extracted(n: int) -> int:"
        );
        // Results: each assigned once and on every path, typed by the server at its assignment.
        let results =
            "def f(xs: list[int]):\n    «lo = min(xs)\n    hi = str(lo)»\n    return hi, lo\n";
        assert_eq!(
            signature(
                results,
                &[("lo", "(variable) lo: int"), ("hi", "(variable) hi: str")]
            ),
            "def extracted(xs: list[int]) -> tuple[int, str]:"
        );
        assert_eq!(
            signature(results, &[("lo", "(variable) lo: int")]),
            "def extracted(xs: list[int]):"
        );
        // Assigned twice: the type at the end is not the one of an assignment.
        assert_eq!(
            signature(
                "def f(a: int, b: int):\n    «total = a + b\n    total *= 2»\n    return total\n",
                &[("total", "(variable) total: int")]
            ),
            "def extracted(a: int, b: int):"
        );
        // The selection's own single `return`; two of them decide nothing.
        assert_eq!(
            signature(
                "def f(a):\n    «b = a.total()\n    return b»\n",
                &[("b", "(variable) b: float")]
            ),
            "def extracted(a) -> float:"
        );
        assert_eq!(
            signature(
                "def f(a):\n    «if a:\n        return a.total()\n    return a.total()»\n",
                &[total]
            ),
            "def extracted(a):"
        );
        // A tuple is typed item by item.
        assert_eq!(
            signature("def f(a: int, b: str):\n    «return a, b»\n", &[]),
            "def extracted(a: int, b: str) -> tuple[int, str]:"
        );
    }
}
