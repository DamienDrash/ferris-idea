//! Ferris IDEA's own Extract Variable for JavaScript and TypeScript (`SPEC-0320` v7 behaviours 23–27):
//! used when the language server offers no extraction there, as TypeScript 7's `tsc --lsp` offers
//! none (OBS-0028 T4). The selected expression becomes `const name = expression;` before its
//! statement, and the expression is replaced by the name. Wherever moving the evaluation in front of
//! the statement could change what the program does, it refuses and says why, instead of writing
//! code that behaves differently.

use std::collections::HashSet;
use std::ops::Range as Bytes;
use std::path::Path;

use tree_sitter::Node;

use crate::LanguageId;
use crate::actions::{Refactoring, RefactoringTarget};
use crate::insight::Range;
use crate::python_extract::{
    action, byte_of, children, edit, eol, field, indent_at, is_field, line_start, message, named,
    range_of, selection, starts_its_line, word,
};
use crate::rename::Edit;
use crate::syntax::SyntaxDocument;

/// The title of the own action (no server uses it, so a server's action of that title is the
/// server's).
pub const EXTRACT_VARIABLE: &str = "Extract variable";

/// At most this many places for a bare caret.
const MAX_PLACES: usize = 8;

/// Kinds of expressions that can be extracted.
const EXPRESSIONS: &[&str] = &[
    "identifier",
    "this",
    "number",
    "string",
    "template_string",
    "regex",
    "true",
    "false",
    "null",
    "undefined",
    "object",
    "array",
    "function_expression",
    "function",
    "arrow_function",
    "generator_function",
    "class",
    "call_expression",
    "new_expression",
    "member_expression",
    "subscript_expression",
    "await_expression",
    "unary_expression",
    "binary_expression",
    "ternary_expression",
    "parenthesized_expression",
    "sequence_expression",
    "as_expression",
    "satisfies_expression",
    "non_null_expression",
    "type_assertion",
    "jsx_element",
    "jsx_self_closing_element",
    "meta_property",
];

/// Kinds that can have side effects when evaluated (behaviour 25).
const EFFECTS: &[&str] = &[
    "call_expression",
    "new_expression",
    "await_expression",
    "yield_expression",
    "assignment_expression",
    "augmented_assignment_expression",
    "update_expression",
];

/// Kinds that hold statements; a statement directly in one of them can have a line before it.
const BLOCKS: &[&str] = &["program", "statement_block"];

/// Names that cannot be or should not be a new variable's name.
const RESERVED: &[&str] = &[
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "NaN",
    "Infinity",
    "globalThis",
    "window",
    "document",
    "console",
    "of",
    "async",
    "type",
];

/// Whether `file` is a JavaScript or TypeScript file and `refactoring` the own one (or Refactor
/// This, which lists it).
pub fn offers(file: &Path, refactoring: Refactoring) -> bool {
    crate::ts_structure::is_script(file)
        && matches!(refactoring, Refactoring::Variable | Refactoring::Any)
}

/// Whether `title` is the own action of a JavaScript or TypeScript `file`.
pub fn is_own(file: &Path, title: &str) -> bool {
    crate::ts_structure::is_script(file) && title == EXTRACT_VARIABLE
}

fn parse(file: &Path, text: &str) -> Result<SyntaxDocument, String> {
    let language = if crate::ts_structure::is_typescript(file) {
        LanguageId::TypeScript
    } else {
        LanguageId::JavaScript
    };
    let tsx = file.extension().is_some_and(|e| e == "tsx");
    SyntaxDocument::new(language, tsx, text).map_err(|_| "the syntax is not available".to_owned())
}

/// The edits of Extract Variable for `range` of `file` with content `text`, or the reason in a
/// sentence (behaviour 27).
pub fn extract_variable(file: &Path, text: &str, range: &Range) -> Result<Vec<Edit>, String> {
    let sel = selection(text, range);
    let doc = parse(file, text).map_err(|why| message(text, &sel, "a variable", &why))?;
    variable_edits(doc.tree().root_node(), text, &sel)
        .map_err(|why| message(text, &sel, "a variable", &why))
}

/// The own action titled `title` for `range` (only Extract Variable).
pub fn plan(file: &Path, text: &str, range: &Range, title: &str) -> Result<Vec<Edit>, String> {
    if title != EXTRACT_VARIABLE {
        return Err(format!("no own action titled {title:?}"));
    }
    extract_variable(file, text, range)
}

/// The places of the own Extract Variable for `range` (a selection, or a bare caret widened to the
/// expressions around it, behaviour 23), and the reason when there is none.
pub fn find(
    file: &Path,
    text: &str,
    range: &Range,
    refactoring: Refactoring,
) -> (Vec<RefactoringTarget>, Option<String>) {
    if !offers(file, refactoring) {
        return (Vec::new(), None);
    }
    let Ok(doc) = parse(file, text) else {
        return (Vec::new(), None);
    };
    let root = doc.tree().root_node();
    let target = |r: &Bytes<usize>| RefactoringTarget {
        range: range_of(text, r.clone()),
        text: text[r.clone()].to_owned(),
        actions: vec![action(EXTRACT_VARIABLE)],
    };
    let sel = selection(text, range);
    let caret = byte_of(text, range.start_line, range.start_character)
        == byte_of(text, range.end_line, range.end_character);
    if !caret {
        return match variable_edits(root, text, &sel) {
            Ok(_) => (vec![target(&sel)], None),
            // Refactor This lists only what works.
            Err(_) if refactoring == Refactoring::Any => (Vec::new(), None),
            Err(why) => (Vec::new(), Some(message(text, &sel, "a variable", &why))),
        };
    }
    if refactoring == Refactoring::Any {
        return (Vec::new(), None);
    }
    let at = byte_of(text, range.start_line, range.start_character);
    let mut found = Vec::new();
    let mut first_refusal = None;
    for r in around(root, text, at) {
        match variable_edits(root, text, &r) {
            Ok(_) => found.push(target(&r)),
            Err(why) if first_refusal.is_none() => {
                first_refusal = Some(message(text, &r, "a variable", &why));
            }
            Err(_) => {}
        }
        if found.len() == MAX_PLACES {
            break;
        }
    }
    let none = found.is_empty().then(|| {
        first_refusal.unwrap_or_else(|| "There is no expression at the caret to extract.".into())
    });
    (found, none)
}

/// The expressions around the caret at byte `at`, from inside out, up to the statement: no plain
/// name, no callee, no repeat.
fn around(root: Node, text: &str, at: usize) -> Vec<Bytes<usize>> {
    let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
    let range = if wordy(text[..at].chars().next_back()) && !wordy(text[at..].chars().next()) {
        at - 1..at
    } else {
        at..(at + 1).min(text.len())
    };
    let mut out: Vec<Bytes<usize>> = Vec::new();
    let mut node = root.descendant_for_byte_range(range.start, range.end);
    while let Some(n) = node {
        if is_statement(n) {
            break;
        }
        node = n.parent();
        let callee = n
            .parent()
            .is_some_and(|p| p.kind() == "call_expression" && is_field(p, "function", n));
        if n.is_named()
            && EXPRESSIONS.contains(&n.kind())
            && n.kind() != "identifier"
            && !callee
            && out.last().is_none_or(|r| *r != n.byte_range())
        {
            out.push(n.byte_range());
        }
    }
    out
}

/// A statement: a node directly in a block, or the body of a statement without braces.
fn is_statement(n: Node) -> bool {
    n.kind().ends_with("_statement") && n.kind() != "parenthesized_expression"
        || n.kind().ends_with("_declaration")
        || n.parent().is_some_and(|p| BLOCKS.contains(&p.kind()))
}

/// The expression whose text is exactly the selection.
fn expression_at<'t>(root: Node<'t>, sel: &Bytes<usize>) -> Result<Node<'t>, String> {
    if sel.is_empty() {
        return Err("nothing is selected".into());
    }
    let mut n = root
        .descendant_for_byte_range(sel.start, sel.end)
        .ok_or("it is not one complete expression")?;
    // The outermost expression with exactly this text (`x` in `(x)` is not `(x)`).
    while let Some(p) = n.parent() {
        if p.byte_range() == n.byte_range() && EXPRESSIONS.contains(&p.kind()) {
            n = p;
        } else {
            break;
        }
    }
    if n.byte_range() != *sel || !n.is_named() {
        return Err("it is not one complete expression".into());
    }
    match n.kind() {
        k if EXPRESSIONS.contains(&k) => Ok(n),
        "assignment_expression" | "augmented_assignment_expression" | "update_expression" => {
            Err("it assigns a value; extract its right side instead".into())
        }
        "yield_expression" => Err("it contains `yield`".into()),
        "super" => Err("`super` is no value of its own".into()),
        _ => Err("it is not one complete expression".into()),
    }
}

/// Whether `n` is in a place where it is a value (behaviour 24), or why not.
fn value_place(n: Node) -> Result<(), String> {
    let Some(p) = n.parent() else {
        return Ok(());
    };
    let target = "it is the target of an assignment".to_owned();
    match p.kind() {
        "assignment_expression" | "augmented_assignment_expression" if is_field(p, "left", n) => {
            Err(target)
        }
        "update_expression" => Err(target),
        "variable_declarator" if is_field(p, "name", n) => Err("it is a declared name".into()),
        "for_in_statement" if is_field(p, "left", n) => Err(target),
        "pair" if is_field(p, "key", n) => Err("it is a key of an object literal".into()),
        // A default value is a value; behaviour 25 says why it stays where it is.
        "required_parameter" | "optional_parameter" if is_field(p, "value", n) => Ok(()),
        "assignment_pattern" if is_field(p, "right", n) => Ok(()),
        "required_parameter" | "optional_parameter" | "formal_parameters"
        | "assignment_pattern" | "array_pattern" | "object_pattern" | "rest_pattern"
        | "pair_pattern" => Err("it is part of a parameter or a pattern".into()),
        "import_specifier" | "export_specifier" | "namespace_import" | "import_clause" => {
            Err("it is an imported or exported name".into())
        }
        "decorator" => Err("it is a decorator".into()),
        "call_expression" | "new_expression"
            if is_field(p, "function", n) && n.kind() == "member_expression" =>
        {
            Err("it is a method being called; extracting it would lose its `this`".into())
        }
        "call_expression" if is_field(p, "function", n) && n.kind() == "identifier" => {
            Err("it is the name of the function called".into())
        }
        "unary_expression" => {
            let op = field(p, "operator").map(|o| o.kind()).unwrap_or_default();
            if op == "delete" {
                Err("it is the operand of `delete`, which needs the property itself".into())
            } else if op == "typeof" && n.kind() == "identifier" {
                Err("it is a name under `typeof`, which may not be declared".into())
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// Whether `node` holds `?.` in the chain it evaluates before `child` (its object or function
/// part, not inside the arguments of a call there).
fn chain_is_optional(node: Node, child: Node) -> bool {
    let mut stack: Vec<Node> = children(node)
        .into_iter()
        .filter(|c| c.end_byte() <= child.start_byte())
        .collect();
    while let Some(x) = stack.pop() {
        if x.kind() == "optional_chain" {
            return true;
        }
        if matches!(
            x.kind(),
            "member_expression"
                | "call_expression"
                | "subscript_expression"
                | "non_null_expression"
        ) {
            stack.extend(
                children(x)
                    .into_iter()
                    .filter(|c| !matches!(c.kind(), "arguments" | "template_string")),
            );
        }
    }
    false
}

/// Why the evaluation of `n` cannot move in front of its statement, which it returns
/// (behaviour 25).
fn statement_of<'t>(n: Node<'t>) -> Result<Node<'t>, String> {
    const SOMETIMES: &str = "is evaluated only sometimes there";
    let mut c = n;
    loop {
        let Some(p) = c.parent() else {
            return Err("it is in no statement".into());
        };
        if BLOCKS.contains(&p.kind()) {
            return Ok(c);
        }
        let reason: Option<String> = match p.kind() {
            "arrow_function" if is_field(p, "body", c) => {
                Some("it is the body of an arrow function, which is evaluated at each call".into())
            }
            "required_parameter" | "optional_parameter" | "assignment_pattern"
            | "formal_parameters" => {
                Some("it is a default value, evaluated only when no argument is given".into())
            }
            "binary_expression" if is_field(p, "right", c) => {
                let op = field(p, "operator").map(|o| o.kind()).unwrap_or_default();
                matches!(op, "&&" | "||" | "??")
                    .then(|| format!("the right side of `{op}` {SOMETIMES}"))
            }
            "augmented_assignment_expression" if is_field(p, "right", c) => {
                let op = field(p, "operator").map(|o| o.kind()).unwrap_or_default();
                matches!(op, "&&=" | "||=" | "??=")
                    .then(|| format!("the right side of `{op}` {SOMETIMES}"))
            }
            "ternary_expression" if !is_field(p, "condition", c) => {
                Some(format!("a branch of a conditional expression {SOMETIMES}"))
            }
            "call_expression" | "member_expression" | "subscript_expression"
                if chain_is_optional(p, c) =>
            {
                Some(format!("after `?.` it {SOMETIMES}"))
            }
            "while_statement" | "do_statement" if is_field(p, "condition", c) => {
                Some("a loop's condition is evaluated before every round".into())
            }
            "for_statement" if !is_field(p, "body", c) => {
                Some("it is in the head of a `for` loop".into())
            }
            "for_in_statement" if is_field(p, "left", c) => {
                Some("it is the variable of a loop".into())
            }
            "switch_case" if is_field(p, "value", c) => Some(
                "a `case` value is evaluated only when the cases before it do not match".into(),
            ),
            "switch_case" | "switch_default" => Some(
                "the statement is in a `case` without braces; put the case's statements in a \
                 block first"
                    .into(),
            ),
            "if_statement" | "else_clause" | "while_statement" | "do_statement"
            | "for_statement" | "for_in_statement" | "labeled_statement" | "with_statement"
                if !is_field(p, "condition", c) && is_statement(c) =>
            {
                Some(format!(
                    "the statement is the body of `{}` without braces; add braces first",
                    p.kind()
                        .trim_end_matches("_statement")
                        .trim_end_matches("_clause")
                ))
            }
            "if_statement" if is_field(p, "condition", c) => p
                .parent()
                .is_some_and(|g| g.kind() == "else_clause")
                .then(|| {
                    "the condition of `else if` is evaluated only when the conditions before it \
                     are false"
                        .to_owned()
                }),
            "class_body" | "public_field_definition" | "field_definition" | "class_heritage" => {
                Some("it is part of a class, not of a statement".into())
            }
            "decorator" => Some("it is in a decorator".into()),
            "function_expression"
            | "function"
            | "generator_function"
            | "method_definition"
            | "class" => Some("it is inside a function that is not called here".into()),
            _ => None,
        };
        if let Some(why) = reason {
            return Err(why);
        }
        c = p;
    }
}

/// The parts of statement `s` evaluated before `n`: left to right in JavaScript, also the target of
/// an assignment before its right side.
fn evaluated_before<'t>(n: Node<'t>, s: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut c = n;
    while c.id() != s.id() {
        let Some(p) = c.parent() else {
            break;
        };
        out.extend(
            named(p)
                .into_iter()
                .filter(|x| x.end_byte() <= c.start_byte() && x.kind() != "statement_block"),
        );
        c = p;
    }
    out
}

/// The first part of `n` that is one of `kinds`, not inside a function defined there (it does not
/// run where it stands).
fn first_of<'t>(n: Node<'t>, kinds: &[&str]) -> Option<Node<'t>> {
    if kinds.contains(&n.kind()) {
        return Some(n);
    }
    if matches!(
        n.kind(),
        "arrow_function" | "function_expression" | "function" | "generator_function" | "class"
    ) {
        return None;
    }
    children(n).into_iter().find_map(|c| first_of(c, kinds))
}

/// Every identifier `n` reads (not property names after a dot).
fn names_read(n: Node, text: &str, out: &mut HashSet<String>) {
    if matches!(n.kind(), "identifier" | "shorthand_property_identifier") {
        out.insert(word(n, text).to_owned());
        return;
    }
    for c in children(n) {
        names_read(c, text, out);
    }
}

/// Names statement `s` declares or assigns, outside of functions defined in it.
fn names_set(n: Node, text: &str, out: &mut HashSet<String>) {
    let target = match n.kind() {
        "variable_declarator" => field(n, "name"),
        "assignment_expression" | "augmented_assignment_expression" => field(n, "left"),
        "update_expression" => field(n, "argument"),
        "arrow_function" | "function_expression" | "function" | "generator_function" | "class" => {
            return;
        }
        _ => None,
    };
    if let Some(t) = target {
        let mut names = HashSet::new();
        // Patterns name every variable they bind; `a.b = …` sets no name.
        if t.kind() != "member_expression" && t.kind() != "subscript_expression" {
            names_read(t, text, &mut names);
            if t.kind() == "identifier" {
                names.insert(word(t, text).to_owned());
            }
        }
        out.extend(names);
    }
    for c in children(n) {
        names_set(c, text, out);
    }
}

/// The new variable's name from the expression (behaviour 26): a call's function, a property, a
/// constructed class; else `value`. Unique in the file.
fn variable_name(n: Node, root: Node, text: &str) -> String {
    let base = suggested(n, text).unwrap_or_else(|| "value".to_owned());
    let mut taken = HashSet::new();
    names_read(root, text, &mut taken);
    collect_kind(root, text, "type_identifier", &mut taken);
    collect_kind(root, text, "property_identifier", &mut HashSet::new());
    let reserved = RESERVED.contains(&base.as_str());
    if !reserved && !taken.contains(&base) {
        return base;
    }
    (1..)
        .map(|i| format!("{base}{i}"))
        .find(|c| !taken.contains(c))
        .unwrap_or(base)
}

fn collect_kind(n: Node, text: &str, kind: &str, out: &mut HashSet<String>) {
    if n.kind() == kind {
        out.insert(word(n, text).to_owned());
    }
    for c in children(n) {
        collect_kind(c, text, kind, out);
    }
}

fn camel(name: &str) -> Option<String> {
    let name = name.trim_start_matches(['_', '$', '#']);
    let name = name
        .strip_prefix("get")
        .filter(|r| r.starts_with(|c: char| c.is_uppercase()))
        .unwrap_or(name);
    // An ALL_CAPS constant suggests nothing.
    if !name.chars().any(char::is_lowercase) {
        return None;
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    if !first.is_alphabetic() {
        return None;
    }
    Some(first.to_lowercase().chain(chars).collect())
}

fn suggested(n: Node, text: &str) -> Option<String> {
    match n.kind() {
        "call_expression" | "new_expression" => {
            let f = field(n, "function").or_else(|| field(n, "constructor"))?;
            if f.kind() == "identifier" {
                camel(word(f, text))
            } else {
                suggested(f, text)
            }
        }
        "member_expression" => field(n, "property").and_then(|p| camel(word(p, text))),
        "identifier" => None,
        "subscript_expression" => field(n, "object").and_then(|o| suggested(o, text)),
        "await_expression"
        | "parenthesized_expression"
        | "non_null_expression"
        | "as_expression"
        | "satisfies_expression"
        | "unary_expression" => named(n)
            .into_iter()
            .find(|c| EXPRESSIONS.contains(&c.kind()))
            .and_then(|c| suggested(c, text)),
        "binary_expression" => {
            let op = field(n, "operator").map(|o| o.kind()).unwrap_or_default();
            if matches!(
                op,
                "<" | ">"
                    | "<="
                    | ">="
                    | "=="
                    | "==="
                    | "!="
                    | "!=="
                    | "&&"
                    | "||"
                    | "instanceof"
                    | "in"
            ) {
                return Some("condition".into());
            }
            field(n, "right")
                .and_then(|r| suggested(r, text))
                .or_else(|| field(n, "left").and_then(|l| suggested(l, text)))
        }
        "string" | "template_string" => Some("text".into()),
        "array" => Some("items".into()),
        "arrow_function" | "function_expression" | "function" => Some("callback".into()),
        _ => None,
    }
}

/// Behaviours 24 and 25: the checks of the place, then the edits.
fn variable_edits(root: Node, text: &str, sel: &Bytes<usize>) -> Result<Vec<Edit>, String> {
    let n = expression_at(root, sel)?;
    if root.has_error() {
        // A file that does not parse is not changed.
        let mut c = n;
        while let Some(p) = c.parent() {
            if p.is_error() {
                return Err("the file has syntax errors there".into());
            }
            c = p;
        }
    }
    value_place(n)?;
    if first_of(n, &["yield_expression"]).is_some() {
        return Err("it contains `yield`".into());
    }
    if first_of(
        n,
        &[
            "assignment_expression",
            "augmented_assignment_expression",
            "update_expression",
        ],
    )
    .is_some()
    {
        return Err("it assigns a value inside".into());
    }
    let s = statement_of(n)?;
    if !starts_its_line(text, s) {
        return Err("the statement does not start its line".into());
    }
    // A name the statement declares or assigns: its value would not be there yet.
    let mut reads = HashSet::new();
    names_read(n, text, &mut reads);
    let mut set = HashSet::new();
    names_set(s, text, &mut set);
    if let Some(name) = reads.iter().filter(|r| set.contains(*r)).min() {
        return Err(format!(
            "it reads `{name}`, which the same statement declares or assigns"
        ));
    }
    // The order of evaluation inside the statement (behaviour 25).
    if first_of(
        n,
        &["call_expression", "new_expression", "await_expression"],
    )
    .is_some()
    {
        for part in evaluated_before(n, s) {
            if let Some(effect) = first_of(part, EFFECTS) {
                let t = word(effect, text);
                let line = t.lines().next().unwrap_or_default();
                let shown: String = line.chars().take(40).collect();
                let more = if shown.len() < t.len() { "…" } else { "" };
                return Err(format!(
                    "it would then run before `{shown}{more}`, which comes first in the statement \
                     and can have side effects"
                ));
            }
        }
    }
    let name = variable_name(n, root, text);
    let value = if n.kind() == "sequence_expression" {
        format!("({})", word(n, text))
    } else {
        word(n, text).to_owned()
    };
    // A whole expression statement becomes the declaration itself.
    if s.kind() == "expression_statement"
        && named(s)
            .first()
            .is_some_and(|e| e.byte_range() == n.byte_range())
    {
        let semicolon = if text[s.byte_range()].trim_end().ends_with(';') {
            ""
        } else {
            ";"
        };
        return Ok(vec![edit(
            text,
            n.byte_range(),
            format!("const {name} = {value}{semicolon}"),
        )]);
    }
    let at = line_start(text, s.start_byte());
    let indent = indent_at(text, s.start_byte());
    // Always with `;`: a next line starting with `(` or `[` would continue the declaration.
    Ok(vec![
        edit(
            text,
            at..at,
            format!("{indent}const {name} = {value};{}", eol(text)),
        ),
        edit(text, n.byte_range(), name),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rename::apply_edits;

    /// The text with `«…»` removed and the range between them (one line or several).
    fn marked(source: &str) -> (String, Range) {
        let start = source.find('«').unwrap();
        let end = source.find('»').unwrap();
        let text = source.replace(['«', '»'], "");
        let pos = |b: usize| {
            let before = &text[..b];
            let line = before.matches('\n').count() as u64;
            let col = before.rsplit('\n').next().unwrap().chars().count() as u64;
            (line, col)
        };
        let (a, b) = (pos(start), pos(end - '«'.len_utf8()));
        (
            text,
            Range {
                start_line: a.0,
                start_character: a.1,
                end_line: b.0,
                end_character: b.1,
            },
        )
    }

    fn extract(file: &str, source: &str) -> Result<String, String> {
        let (text, range) = marked(source);
        let edits = extract_variable(Path::new(file), &text, &range)?;
        Ok(apply_edits(&text, &edits).unwrap())
    }

    /// OBS-0028 item 4: `x>3` in `if(x>3) return x`.
    #[test]
    fn the_observed_condition() {
        assert_eq!(
            extract(
                "fmt.ts",
                "function f(x: number) {\n  if(«x>3») return x\n  return 0\n}\n"
            )
            .unwrap(),
            "function f(x: number) {\n  const condition = x>3;\n  if(condition) return x\n  return 0\n}\n"
        );
    }

    #[test]
    fn the_cases_of_the_task_in_every_dialect() {
        let cases = [
            (
                "const s = a+«b*2»\n",
                "const value = b*2;\nconst s = a+value\n",
            ),
            (
                "const l = «[1,2,3]»\n",
                "const items = [1,2,3];\nconst l = items\n",
            ),
            (
                "function f() {\n  «console.log(1)»;\n}\n",
                "function f() {\n  const log = console.log(1);\n}\n",
            ),
            (
                "function f() {\n  console.log(1)\n  «g()»\n}\n",
                "function f() {\n  console.log(1)\n  const g1 = g();\n}\n",
            ),
            (
                "const total = «order.getTotal()» * 2;\n",
                "const total1 = order.getTotal();\nconst total = total1 * 2;\n",
            ),
            (
                "async function f() {\n\treturn «await load()» + 1;\n}\n",
                "async function f() {\n\tconst load1 = await load();\n\treturn load1 + 1;\n}\n",
            ),
            (
                "export default «new Shop()»;\n",
                "const shop = new Shop();\nexport default shop;\n",
            ),
            ("f(«(a, b)»);\n", "const value = (a, b);\nf(value);\n"),
        ];
        for file in ["a.ts", "a.tsx", "a.js", "a.jsx", "a.mts"] {
            for (source, expected) in cases {
                assert_eq!(
                    extract(file, source).unwrap_or_else(|e| panic!("{file} {source:?}: {e}")),
                    expected,
                    "{file} {source:?}"
                );
            }
        }
        // JSX in a `.tsx` file.
        assert_eq!(
            extract(
                "a.tsx",
                "function A() {\n  return <div>{«items.length»}</div>;\n}\n"
            )
            .unwrap(),
            "function A() {\n  const length = items.length;\n  return <div>{length}</div>;\n}\n"
        );
    }

    #[test]
    fn refusals_name_their_reason() {
        let cases = [
            ("const s = «a+b»*2\n", "not one complete expression"),
            ("const s = a«+b»\n", "not one complete expression"),
            ("«x» = 1;\n", "target of an assignment"),
            ("«x»++;\n", "target of an assignment"),
            ("const «x» = 1;\n", "declared name"),
            ("const o = {«a»: 1};\n", "not one complete expression"),
            ("function f(a = «g()») {}\n", "default value"),
            ("const f = (x: number) => «x * 2»;\n", "arrow function"),
            ("const v = a && «b()»;\n", "`&&`"),
            ("const v = a ?? «b()»;\n", "`??`"),
            ("const v = c ? «a()» : b;\n", "branch of a conditional"),
            ("const v = a?.b(«c()»);\n", "after `?.`"),
            ("const v = a?.b.c(«d»);\n", "after `?.`"),
            ("while («more()») {}\n", "loop's condition"),
            ("for (let i = 0; «i < n»; i++) {}\n", "head of a `for`"),
            ("if (a) {} else if («b()») {}\n", "`else if`"),
            ("if (a) return «f(x)»;\n", "without braces"),
            (
                "switch (v) {\n  case 1:\n    g(«h()»);\n}\n",
                "`case` without braces",
            ),
            ("switch (v) { case «k()»: break; }\n", "`case` value"),
            ("class A {\n  x = «f()»;\n}\n", "part of a class"),
            ("«obj.run»();\n", "lose its `this`"),
            ("delete «o.p»;\n", "`delete`"),
            ("const t = typeof «x»;\n", "`typeof`"),
            ("const a = 1, b = «a + 1»;\n", "reads `a`"),
            ("x = 1, y = «x + 1»;\n", "reads `x`"),
            ("const v = first() + «second()»;\n", "before `first()`"),
            ("const v = [first(), «second()»];\n", "before `first()`"),
            ("a[g()] = «f()»;\n", "before `g()`"),
            ("function* g() {\n  const v = «(yield 1)»;\n}\n", "`yield`"),
            ("const v = «(a = 2)» + 1;\n", "assigns a value"),
            (
                "if (x) { a(); } const v = 1; g(«h()»);\n",
                "does not start its line",
            ),
            ("import { «a» } from \"m\";\n", "imported or exported"),
            ("const v = «»1;\n", "nothing is selected"),
        ];
        for (source, reason) in cases {
            match extract("a.ts", source) {
                Ok(after) => panic!("{source:?} is not refused: {after}"),
                Err(why) => assert!(why.contains(reason), "{source:?}: {why}"),
            }
        }
    }

    #[test]
    fn allowed_neighbours_of_the_refusals() {
        let cases = [
            "const v = «a()» && b;\n",
            "const v = «c()» ? a : b;\n",
            "const v = «a?.b»;\n",
            "for (const x of «items()») {}\n",
            "switch («kind()») { case 1: break; }\n",
            "if («f(x)») { g(); } else if (y) {}\n",
            "const v = first() + «a.b * c.d»;\n",
            "d[k] = «make()»;\n",
            "const v = obj.run(«g()»);\n",
            "const v = [() => first(), «g()»];\n",
            "const f = (x: number) => {\n  return «x * 2»;\n};\n",
            "class A {\n  m() {\n    return «this.f(1)» + 2;\n  }\n}\n",
            "const t = typeof «a.b»;\n",
        ];
        for source in cases {
            let after = extract("a.ts", source).unwrap_or_else(|e| panic!("{source:?}: {e}"));
            let (text, _) = marked(source);
            assert_ne!(after, text);
            let doc = parse(Path::new("a.ts"), &after).unwrap();
            assert!(!doc.tree().root_node().has_error(), "{after}");
        }
    }

    #[test]
    fn names_are_unique_and_no_keywords() {
        assert_eq!(
            extract("a.ts", "const value = 1;\nconst s = a+«b*2»;\n").unwrap(),
            "const value = 1;\nconst value1 = b*2;\nconst s = a+value1;\n"
        );
        assert_eq!(
            extract("a.ts", "const v = «o.default» + 1;\n").unwrap(),
            "const default1 = o.default;\nconst v = default1 + 1;\n"
        );
        assert_eq!(
            extract("a.ts", "const v = «o.MAX» + 1;\n").unwrap(),
            "const value = o.MAX;\nconst v = value + 1;\n"
        );
    }

    #[test]
    fn a_bare_caret_offers_the_expressions_around_it() {
        let (text, _) = marked("const v = a + order.getTotal(«»1) * 2;\n");
        let at = text.find("1)").unwrap();
        let caret = range_of(&text, at..at);
        let (found, none) = find(Path::new("a.ts"), &text, &caret, Refactoring::Variable);
        assert_eq!(none, None);
        let texts: Vec<&str> = found.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "1",
                "order.getTotal(1)",
                "order.getTotal(1) * 2",
                "a + order.getTotal(1) * 2"
            ]
        );
        // Nothing at the caret to extract.
        let (found, none) = find(
            Path::new("a.ts"),
            "let x;\n",
            &range_of("let x;\n", 4..4),
            Refactoring::Variable,
        );
        assert!(found.is_empty());
        assert!(none.is_some());
        // Refactor This lists the own entry for a selection that works, nothing otherwise.
        let (text, range) = marked("const v = «a + 1»;\n");
        assert_eq!(
            find(Path::new("a.ts"), &text, &range, Refactoring::Any)
                .0
                .len(),
            1
        );
        let (text, range) = marked("«x» = 1;\n");
        let (found, none) = find(Path::new("a.ts"), &text, &range, Refactoring::Any);
        assert!(found.is_empty() && none.is_none());
        // Not a script file.
        assert!(!offers(Path::new("a.py"), Refactoring::Variable));
        assert!(!offers(Path::new("a.ts"), Refactoring::Method));
    }
}
