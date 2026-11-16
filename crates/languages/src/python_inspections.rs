//! IDECraft's own Python inspections that need the scopes of a file (`SPEC-0098` v5 behaviour 12):
//! a parameter or local name of a function that shadows a name of an enclosing function or of the
//! module. The binding rules follow The Python Language Reference, "Naming and binding". And a
//! method that does not use `self` (`SPEC-0098` v6 behaviour 14), with the quick fix "Make method
//! static" (v7 behaviour 15).

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::inspections::{Finding, Fix, Level};
use crate::jvm::TextEdit;

/// Ids and display names of the Python rules of this module.
pub const PYTHON_RULES: &[(&str, &str)] = &[
    (
        "python.shadows-name",
        "Name shadows a name of an outer scope",
    ),
    (
        "python.method-could-be-static",
        "Method does not use 'self'",
    ),
];

/// How a name is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Binding {
    /// Assignment, `for` target, `with … as` target or `:=`: a local variable.
    Variable,
    /// `def`, `class` or `import`: counts as an outer name, is not reported itself.
    Declaration,
}

/// What a scope's own statements bind and declare, in document order.
#[derive(Default)]
struct Bound<'t> {
    names: Vec<(Node<'t>, Binding)>,
    /// Names of `global` and `nonlocal` statements.
    outer: HashSet<String>,
}

fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

fn children(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children(&mut cursor).collect()
}

/// The identifiers a target binds: a name, or the names of a tuple, list or starred target.
/// Attribute and subscript targets bind no name.
fn targets<'t>(n: Node<'t>, out: &mut Vec<(Node<'t>, Binding)>) {
    match n.kind() {
        "identifier" => out.push((n, Binding::Variable)),
        "pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern"
        | "as_pattern_target" => {
            for c in children(n) {
                targets(c, out);
            }
        }
        _ => {}
    }
}

/// The first name of a dotted name (`import a.b` binds `a`).
fn first_name(n: Node) -> Option<Node> {
    match n.kind() {
        "identifier" => Some(n),
        "dotted_name" => children(n).into_iter().find(|c| c.kind() == "identifier"),
        _ => None,
    }
}

/// Collects what the statements below `n` bind for the scope `n` belongs to, without entering
/// nested functions, classes, lambdas and comprehensions.
fn collect<'t>(n: Node<'t>, text: &str, bound: &mut Bound<'t>) {
    for c in children(n) {
        collect_one(c, text, bound);
    }
}

/// [`collect`] for the node `n` itself.
fn collect_one<'t>(n: Node<'t>, text: &str, bound: &mut Bound<'t>) {
    match n.kind() {
        "function_definition" | "class_definition" => {
            if let Some(name) = n.child_by_field_name("name") {
                bound.names.push((name, Binding::Declaration));
            }
        }
        "lambda"
        | "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {}
        "assignment" => {
            // `x: int` without a value binds nothing; `a = b = 1` binds both.
            if let Some(right) = n.child_by_field_name("right") {
                if let Some(left) = n.child_by_field_name("left") {
                    targets(left, &mut bound.names);
                }
                collect_one(right, text, bound);
            }
        }
        "augmented_assignment" => {
            if let Some(right) = n.child_by_field_name("right") {
                collect_one(right, text, bound);
            }
        }
        "named_expression" => {
            if let Some(name) = n.child_by_field_name("name") {
                targets(name, &mut bound.names);
            }
            if let Some(value) = n.child_by_field_name("value") {
                collect_one(value, text, bound);
            }
        }
        "for_statement" => {
            if let Some(left) = n.child_by_field_name("left") {
                targets(left, &mut bound.names);
            }
            for part in ["right", "body", "alternative"] {
                if let Some(p) = n.child_by_field_name(part) {
                    collect_one(p, text, bound);
                }
            }
        }
        "as_pattern" => {
            // `with open(p) as f` binds `f`; `except E as e` is left out (out of scope).
            let in_with = n.parent().is_some_and(|p| p.kind() == "with_item");
            for part in children(n) {
                if part.kind() == "as_pattern_target" {
                    if in_with {
                        targets(part, &mut bound.names);
                    }
                } else {
                    collect_one(part, text, bound);
                }
            }
        }
        "import_statement" | "import_from_statement" | "future_import_statement" => {
            let mut cursor = n.walk();
            for name in n.children_by_field_name("name", &mut cursor) {
                let bound_name = if name.kind() == "aliased_import" {
                    name.child_by_field_name("alias")
                } else if n.kind() == "import_statement" {
                    // `import a.b` binds `a`.
                    first_name(name)
                } else {
                    children(name)
                        .into_iter()
                        .rfind(|p| p.kind() == "identifier")
                        .or_else(|| first_name(name))
                };
                if let Some(b) = bound_name.filter(|b| b.kind() == "identifier") {
                    bound.names.push((b, Binding::Declaration));
                }
            }
        }
        "global_statement" | "nonlocal_statement" => {
            for name in children(n) {
                if name.kind() == "identifier" {
                    bound.outer.insert(word(name, text).to_owned());
                }
            }
        }
        _ => collect(n, text, bound),
    }
}

/// The parameter names of a function, in order.
fn parameters<'t>(function: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let Some(list) = function.child_by_field_name("parameters") else {
        return out;
    };
    for p in children(list) {
        let name = match p.kind() {
            "identifier" => Some(p),
            "default_parameter" | "typed_default_parameter" => p.child_by_field_name("name"),
            "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                // `x: T`, `*args`, `**kwargs`, `*args: T`.
                let mut inner = p;
                loop {
                    let next = children(inner).into_iter().find(|c| {
                        matches!(
                            c.kind(),
                            "identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
                        )
                    });
                    match next {
                        Some(n) if n.kind() == "identifier" => break Some(n),
                        Some(n) => inner = n,
                        None => break None,
                    }
                }
            }
            _ => None,
        };
        out.extend(name.filter(|n| n.kind() == "identifier"));
    }
    out
}

/// The names of one function scope for the functions nested in it.
struct FunctionScope {
    name: String,
    /// Name → 0-based line of its first binding.
    names: HashMap<String, usize>,
}

fn first_lines(names: &[(Node, Binding)], text: &str) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for (n, _) in names {
        out.entry(word(*n, text).to_owned())
            .or_insert(n.start_position().row);
    }
    out
}

fn visit(
    n: Node,
    text: &str,
    module: &HashMap<String, usize>,
    enclosing: &mut Vec<FunctionScope>,
    out: &mut Vec<Finding>,
) {
    for c in children(n) {
        if c.kind() != "function_definition" {
            visit(c, text, module, enclosing, out);
            continue;
        }
        let params = parameters(c);
        let mut bound = Bound::default();
        if let Some(body) = c.child_by_field_name("body") {
            collect(body, text, &mut bound);
        }
        let mut seen = HashSet::new();
        let candidates = params
            .iter()
            .map(|p| (*p, "Parameter"))
            .chain(
                bound
                    .names
                    .iter()
                    .filter(|(_, b)| *b == Binding::Variable)
                    .map(|(v, _)| (*v, "Local variable")),
            )
            .collect::<Vec<_>>();
        for (node, kind) in candidates {
            let name = word(node, text);
            if !seen.insert(name) {
                continue;
            }
            if name == "self"
                || name == "cls"
                || name.starts_with('_')
                || bound.outer.contains(name)
            {
                continue;
            }
            let outer = enclosing
                .iter()
                .rev()
                .find_map(|s| {
                    s.names
                        .get(name)
                        .map(|line| (format!("of the enclosing function '{}'", s.name), *line))
                })
                .or_else(|| {
                    module
                        .get(name)
                        .map(|line| ("of the module".to_owned(), *line))
                });
            if let Some((scope, line)) = outer {
                out.push(Finding {
                    id: "python.shadows-name",
                    start: node.start_byte(),
                    end: node.end_byte(),
                    level: Level::Weak,
                    message: format!(
                        "{kind} '{name}' shadows the name '{name}' {scope} (line {})",
                        line + 1
                    ),
                    fix: None,
                });
            }
        }
        // What this function binds, for the functions nested in it; names it declares `global`
        // or `nonlocal` belong to the outer scopes.
        let mut all: Vec<(Node, Binding)> =
            params.iter().map(|p| (*p, Binding::Variable)).collect();
        all.extend(bound.names.iter().copied());
        let mut names = first_lines(&all, text);
        names.retain(|k, _| !bound.outer.contains(k));
        enclosing.push(FunctionScope {
            name: c
                .child_by_field_name("name")
                .map(|x| word(x, text).to_owned())
                .unwrap_or_default(),
            names,
        });
        visit(c, text, module, enclosing, out);
        enclosing.pop();
    }
}

/// The findings of the Python rules for the tree at `root` of `text`.
pub fn analyze(root: Node, text: &str) -> Vec<Finding> {
    let mut bound = Bound::default();
    collect(root, text, &mut bound);
    let module = first_lines(&bound.names, text);
    let mut out = Vec::new();
    visit(root, text, &module, &mut Vec::new(), &mut out);
    static_candidates(root, root, text, &mut out);
    out
}

/// The named children of `n` without comments.
fn statements(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect()
}

/// Whether the class names anything in its parentheses other than `object`: a base class, a
/// `metaclass=` or another keyword.
fn has_bases(class: Node, text: &str) -> bool {
    class
        .child_by_field_name("superclasses")
        .is_some_and(|list| {
            statements(list)
                .into_iter()
                .any(|b| !(b.kind() == "identifier" && word(b, text) == "object"))
        })
}

/// Whether the first parameter of `function` is a plain `self` (an annotation is allowed).
fn takes_self(function: Node, text: &str) -> bool {
    let Some(first) = function
        .child_by_field_name("parameters")
        .and_then(|list| statements(list).into_iter().next())
    else {
        return false;
    };
    let name = match first.kind() {
        "identifier" => Some(first),
        "typed_parameter" => first.named_child(0).filter(|n| n.kind() == "identifier"),
        _ => None,
    };
    name.is_some_and(|n| word(n, text) == "self")
}

/// Whether the body is only a docstring, `pass`, `...`, `raise NotImplementedError` or several
/// of these: a placeholder meant to be overridden.
fn is_placeholder(body: Node, text: &str) -> bool {
    statements(body).into_iter().all(|s| match s.kind() {
        "pass_statement" => true,
        "expression_statement" => {
            let parts = statements(s);
            parts.len() == 1
                && matches!(
                    parts[0].kind(),
                    "string" | "concatenated_string" | "ellipsis"
                )
        }
        "raise_statement" => statements(s).first().is_some_and(|raised| {
            let name = if raised.kind() == "call" {
                raised.child_by_field_name("function")
            } else {
                Some(*raised)
            };
            name.is_some_and(|n| n.kind() == "identifier" && word(n, text) == "NotImplementedError")
        }),
        _ => false,
    })
}

/// Whether anything below `n` names `self`, `super` or `__class__`, nested functions, lambdas
/// and comprehensions included.
fn needs_instance(n: Node, text: &str) -> bool {
    if n.kind() == "identifier" {
        return matches!(word(n, text), "self" | "super" | "__class__");
    }
    children(n).into_iter().any(|c| needs_instance(c, text))
}

/// Whether anything below `n` names `method` through `class` (`Shape.area`): such a call passes
/// the instance by hand.
fn named_through_class(n: Node, text: &str, class: &str, method: &str) -> bool {
    if n.kind() == "attribute" {
        let is = |field: &str, name: &str| {
            n.child_by_field_name(field)
                .is_some_and(|x| x.kind() == "identifier" && word(x, text) == name)
        };
        if is("object", class) && is("attribute", method) {
            return true;
        }
    }
    children(n)
        .into_iter()
        .any(|c| named_through_class(c, text, class, method))
}

/// The quick fix "Make method static" of `method` (`SPEC-0098` v7 behaviour 15): `@staticmethod`
/// above the `def` with its indentation, and `self` out of the parameter list. `None` where the
/// change cannot be made cleanly.
fn make_static(method: Node, text: &str) -> Option<Fix> {
    if method.has_error() {
        return None;
    }
    let start = method.start_byte();
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let indent = &text[line_start..start];
    if !indent.chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }
    let parts = children(method.child_by_field_name("parameters")?);
    let (open, close) = (parts.first()?, parts.last()?);
    if open.kind() != "(" || close.kind() != ")" {
        return None;
    }
    let is_parameter = |p: &Node| p.is_named() && p.kind() != "comment";
    let at = parts.iter().position(is_parameter)?;
    let (own, rest) = (parts[at], &parts[at + 1..]);
    let next = rest.iter().find(|p| is_parameter(p));
    if next.is_some_and(|p| p.kind() == "positional_separator") {
        return None;
    }
    let comment = parts.iter().any(|p| p.kind() == "comment");
    let range = match rest.first().filter(|p| p.kind() == ",") {
        // `self` alone: the parentheses are left empty.
        _ if next.is_none() && !comment => open.end_byte()..close.start_byte(),
        Some(comma) => {
            let after = &text[comma.end_byte()..];
            let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
            let end = comma.end_byte() + blanks;
            let own_line = text[..own.start_byte()].rfind('\n').map_or(0, |i| i + 1);
            let alone = text[own_line..own.start_byte()]
                .chars()
                .all(|c| c == ' ' || c == '\t');
            let line_end = ["\r\n", "\n"]
                .iter()
                .find(|nl| text[end..].starts_with(**nl))
                .map(|nl| end + nl.len());
            match line_end {
                // `self,` on a line of its own goes with its line.
                Some(line_end) if alone => own_line..line_end,
                _ => own.start_byte()..end,
            }
        }
        None => own.byte_range(),
    };
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    Some(Fix {
        learn: None,
        keep: None,
        action: None,
        title: "Make method static".to_owned(),
        edits: vec![
            TextEdit {
                range: start..start,
                text: format!("@staticmethod{newline}{indent}"),
            },
            TextEdit {
                range,
                text: String::new(),
            },
        ],
    })
}

/// `python.method-could-be-static` (`SPEC-0098` v6 behaviour 14): the methods of the classes
/// below `n` that do not use `self`. `root` is the tree's root.
fn static_candidates(root: Node, n: Node, text: &str, out: &mut Vec<Finding>) {
    for c in children(n) {
        static_candidates(root, c, text, out);
        if c.kind() != "class_definition" {
            continue;
        }
        let class_name = c
            .child_by_field_name("name")
            .map(|x| word(x, text))
            .unwrap_or_default();
        if class_name.starts_with("Test") || has_bases(c, text) {
            continue;
        }
        let Some(body) = c.child_by_field_name("body") else {
            continue;
        };
        // Only the statements of the class body itself; a decorated method is a
        // `decorated_definition` and so left out.
        for method in children(body) {
            if method.kind() != "function_definition" {
                continue;
            }
            let (Some(name), Some(method_body)) = (
                method.child_by_field_name("name"),
                method.child_by_field_name("body"),
            ) else {
                continue;
            };
            let method_name = word(name, text);
            if (method_name.starts_with("__") && method_name.ends_with("__"))
                || !takes_self(method, text)
                || is_placeholder(method_body, text)
                || needs_instance(method_body, text)
            {
                continue;
            }
            out.push(Finding {
                id: "python.method-could-be-static",
                start: name.start_byte(),
                end: name.end_byte(),
                level: Level::Weak,
                message: format!(
                    "Method '{method_name}' does not use 'self'; it could be a static method or a function"
                ),
                fix: make_static(method, text)
                    .filter(|_| !named_through_class(root, text, class_name, method_name)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LanguageId;
    use crate::syntax::SyntaxDocument;

    /// `line: message` of each finding, with the reported text checked against the name.
    fn found(text: &str) -> Vec<String> {
        let doc = SyntaxDocument::new(LanguageId::Python, false, text).unwrap();
        let mut out = analyze(doc.tree().root_node(), text);
        out.retain(|f| f.id == "python.shadows-name");
        out.sort_by_key(|f| f.start);
        out.iter()
            .map(|f| {
                let name = &text[f.start..f.end];
                assert!(f.message.contains(&format!("'{name}'")), "{}", f.message);
                assert_eq!(f.level, Level::Weak);
                let line = text[..f.start].matches('\n').count() + 1;
                format!("{line}: {}", f.message)
            })
            .collect()
    }

    #[test]
    fn parameter_shadows_a_module_name_bound_below() {
        let text =
            "def describe(item, factor=1):\n    return item\n\n\nitem = 3\nprint(describe(item))\n";
        assert_eq!(
            found(text),
            ["1: Parameter 'item' shadows the name 'item' of the module (line 5)"]
        );
    }

    #[test]
    fn every_kind_of_parameter() {
        let text = "a = b = c = d = e = f = 1\n\n\ndef g(a, b=1, c: int = 2, *d, e: str, **f):\n    pass\n";
        let lines = found(text);
        assert_eq!(lines.len(), 6, "{lines:?}");
        assert!(lines.iter().all(|l| l.starts_with("4: Parameter '")));
        let typed = "args = 1\nkwargs = 2\n\n\ndef g(*args: int, **kwargs: str):\n    pass\n";
        assert_eq!(found(typed).len(), 2);
    }

    #[test]
    fn local_bindings_once_at_the_first() {
        let text = "\
import os
from pathlib import Path as P, PurePath

total = 0
if total:
    flag = True


def helper():
    pass


def run(items):
    total = 1
    total = 2
    for flag in items:
        pass
    with open('x') as os:
        pass
    (P, helper) = (1, 2)
    if (PurePath := 3):
        pass
";
        assert_eq!(
            found(text),
            [
                "14: Local variable 'total' shadows the name 'total' of the module (line 4)",
                "16: Local variable 'flag' shadows the name 'flag' of the module (line 6)",
                "18: Local variable 'os' shadows the name 'os' of the module (line 1)",
                "20: Local variable 'P' shadows the name 'P' of the module (line 2)",
                "20: Local variable 'helper' shadows the name 'helper' of the module (line 9)",
                "21: Local variable 'PurePath' shadows the name 'PurePath' of the module (line 2)",
            ]
        );
    }

    #[test]
    fn enclosing_function_before_the_module() {
        let text = "\
count = 0


def report(rows, count):
    width = 1

    def line(row, width, count):
        rows = row
        return rows

    return line
";
        assert_eq!(
            found(text),
            [
                "4: Parameter 'count' shadows the name 'count' of the module (line 1)",
                "7: Parameter 'width' shadows the name 'width' of the enclosing function 'report' (line 5)",
                "7: Parameter 'count' shadows the name 'count' of the enclosing function 'report' (line 4)",
                "8: Local variable 'rows' shadows the name 'rows' of the enclosing function 'report' (line 4)",
            ]
        );
    }

    #[test]
    fn quiet_for_self_cls_underscore_global_and_nonlocal() {
        let text = "\
self = cls = _private = _ = counter = state = 1


def f(self, cls, _private, _):
    global counter
    counter = 2

    def inner():
        nonlocal state
        state = 3

    state = 4
    return inner


def g():
    state = 5

    def inner():
        nonlocal state
        state = 6
";
        assert_eq!(
            found(text),
            [
                "12: Local variable 'state' shadows the name 'state' of the module (line 1)",
                "17: Local variable 'state' shadows the name 'state' of the module (line 1)",
            ]
        );
    }

    #[test]
    fn quiet_for_targets_that_bind_no_name() {
        let text = "\
x = d = k = n = 1


def f(obj):
    obj.x = 1
    d[k] = 2
    n += 1
    x: int
    print(x)
";
        assert_eq!(found(text), Vec::<String>::new());
    }

    #[test]
    fn class_bodies_bind_nothing_for_methods_and_are_not_reported() {
        let text = "\
limit = 10


class Box:
    limit = 5
    value = 0

    def set(self, value, limit):
        self.value = value

    @property
    def size(self):
        size = 1
        return size


def size():
    class Local:
        size = 2
";
        assert_eq!(
            found(text),
            [
                "8: Parameter 'limit' shadows the name 'limit' of the module (line 1)",
                "13: Local variable 'size' shadows the name 'size' of the module (line 17)",
            ]
        );
    }

    #[test]
    fn comprehensions_lambdas_and_other_bindings_are_left_alone() {
        let text = "\
x = e = mod = inner = 1


def f(values):
    squares = [x * x for x in values]
    key = lambda x: x
    try:
        pass
    except ValueError as e:
        pass
    import mod

    def inner():
        pass

    return squares, key
";
        assert_eq!(found(text), Vec::<String>::new());
    }

    #[test]
    fn names_of_a_nested_function_scope_count_for_deeper_functions() {
        let text = "\
def outer():
    def helper():
        pass

    def use(helper):
        return helper
";
        assert_eq!(
            found(text),
            [
                "5: Parameter 'helper' shadows the name 'helper' of the enclosing function 'outer' (line 2)"
            ]
        );
    }

    /// `line: name` of each method reported as not using `self`.
    fn could_be_static(text: &str) -> Vec<String> {
        let doc = SyntaxDocument::new(LanguageId::Python, false, text).unwrap();
        let mut out = analyze(doc.tree().root_node(), text);
        out.retain(|f| f.id == "python.method-could-be-static");
        out.sort_by_key(|f| f.start);
        out.iter()
            .map(|f| {
                let name = &text[f.start..f.end];
                assert_eq!(f.level, Level::Weak);
                assert_eq!(
                    f.message,
                    format!(
                        "Method '{name}' does not use 'self'; it could be a static method or a function"
                    )
                );
                let line = text[..f.start].matches('\n').count() + 1;
                format!("{line}: {name}")
            })
            .collect()
    }

    /// The findings of methods that do not use `self`, in document order.
    fn candidates(text: &str) -> Vec<Finding> {
        let doc = SyntaxDocument::new(LanguageId::Python, false, text).unwrap();
        let mut out = analyze(doc.tree().root_node(), text);
        out.retain(|f| f.id == "python.method-could-be-static");
        out.sort_by_key(|f| f.start);
        out
    }

    /// `text` after "Make method static" on its first reported method; the result parses and
    /// no longer reports that method.
    fn made_static(text: &str) -> String {
        let found = candidates(text);
        let fix = found[0].fix.clone().expect("a fix");
        assert_eq!(fix.title, "Make method static");
        let new = crate::jvm::apply(text, &fix.edits);
        let doc = SyntaxDocument::new(LanguageId::Python, false, &new).unwrap();
        assert!(!doc.tree().root_node().has_error(), "{new}");
        assert_eq!(candidates(&new).len(), found.len() - 1, "{new}");
        new
    }

    /// SPEC-0098/AK-10: the decorator above the `def` and `self` out of the list.
    #[test]
    fn make_method_static() {
        assert_eq!(
            made_static("class Shape:\n    def area(self):\n        return 0\n"),
            "class Shape:\n    @staticmethod\n    def area():\n        return 0\n"
        );
        assert_eq!(
            made_static("class Shape:\n    def scale(self, by, unit='m'):\n        return by\n"),
            "class Shape:\n    @staticmethod\n    def scale(by, unit='m'):\n        return by\n"
        );
        assert_eq!(
            made_static(
                "class Shape:\n    async def load(self: 'Shape', path):\n        return path\n"
            ),
            "class Shape:\n    @staticmethod\n    async def load(path):\n        return path\n"
        );
        // Keyword-only parameters after `self`.
        assert_eq!(
            made_static("class A:\n    def f(self, *, key):\n        return key\n"),
            "class A:\n    @staticmethod\n    def f(*, key):\n        return key\n"
        );
        // A class nested in a function: the method's own indentation.
        assert_eq!(
            made_static(
                "def outer():\n    class Local:\n        def g(self):\n            return 2\n"
            ),
            "def outer():\n    class Local:\n        @staticmethod\n        def g():\n            return 2\n"
        );
        // Calls through an instance stay as they are.
        let text = "class A:\n    def f(self):\n        return 1\n\n    def g(self):\n        return self.f()\n\n\nprint(A().f())\n";
        assert_eq!(
            made_static(text),
            "class A:\n    @staticmethod\n    def f():\n        return 1\n\n    def g(self):\n        return self.f()\n\n\nprint(A().f())\n"
        );
    }

    /// SPEC-0098/AK-10: tabs, `\r\n`, lists over several lines, spaces and trailing commas.
    #[test]
    fn make_method_static_keeps_the_layout() {
        assert_eq!(
            made_static("class A:\n\tdef f(self):\n\t\treturn 1\n"),
            "class A:\n\t@staticmethod\n\tdef f():\n\t\treturn 1\n"
        );
        assert_eq!(
            made_static("class A:\r\n    def f(self, a):\r\n        return a\r\n"),
            "class A:\r\n    @staticmethod\r\n    def f(a):\r\n        return a\r\n"
        );
        assert_eq!(
            made_static("class A :\n  def f( self ):\n    return 1\n"),
            "class A :\n  @staticmethod\n  def f():\n    return 1\n"
        );
        assert_eq!(
            made_static("class A:\n    def f(self,):\n        return 1\n"),
            "class A:\n    @staticmethod\n    def f():\n        return 1\n"
        );
        assert_eq!(
            made_static("class A:\n    def f( self , a ):\n        return a\n"),
            "class A:\n    @staticmethod\n    def f( a ):\n        return a\n"
        );
        assert_eq!(
            made_static(
                "class A:\n    def f(\n        self,\n        a,\n    ):\n        return a\n"
            ),
            "class A:\n    @staticmethod\n    def f(\n        a,\n    ):\n        return a\n"
        );
        // A comment after the comma stays.
        assert_eq!(
            made_static(
                "class A:\n    def f(self,  # the instance\n          a):\n        return a\n"
            ),
            "class A:\n    @staticmethod\n    def f(# the instance\n          a):\n        return a\n"
        );
        assert_eq!(
            made_static("class A:\n    def f(self  # alone\n          ):\n        return 1\n"),
            "class A:\n    @staticmethod\n    def f(  # alone\n          ):\n        return 1\n"
        );
    }

    /// SPEC-0098/AK-10: reported, but without a fix.
    #[test]
    fn no_fix_where_it_is_not_safe() {
        let fixes = |text: &str| -> Vec<bool> {
            candidates(text).iter().map(|f| f.fix.is_some()).collect()
        };
        // The positional-only marker after `self`.
        assert_eq!(
            fixes("class A:\n    def f(self, /, a):\n        return a\n"),
            [false]
        );
        // The file calls the method through its class; another method of the class and the
        // same method name of another class keep their fix.
        let text = "class A:\n    def f(self):\n        return 1\n\n    def g(self):\n        return 2\n\n\nclass B:\n    def f(self):\n        return 3\n\n\nprint(A.f(A()))\n";
        assert_eq!(fixes(text), [false, true, true]);
    }

    /// SPEC-0098/AK-9: what is reported.
    #[test]
    fn methods_that_do_not_use_self() {
        let text = "\
class Shape :
  sides=4
  def area( self ):
    return 0
  def name(self) :
    return \"shape\"
  async def load(self: 'Shape', path):
    return path
  def _helper(self, a):
    # a comment that names self
    return a * 2
  def __mangled(self):
    return 1


class Old(object):
    def f(self):
        return 1


def outer():
    class Local:
        def g(self):
            return 2
";
        assert_eq!(
            could_be_static(text),
            [
                "3: area",
                "5: name",
                "7: load",
                "9: _helper",
                "12: __mangled",
                "17: f",
                "23: g"
            ]
        );
    }

    /// SPEC-0098/AK-9: uses of the instance keep a method quiet.
    #[test]
    fn methods_that_use_the_instance_are_quiet() {
        let text = "\
class Box:
    def read(self):
        return self.value

    def write(self, value):
        self.value = value

    def rebind(self):
        self = None

    def nested(self):
        def inner():
            return self
        return inner

    def in_lambda(self):
        return lambda: self.value

    def in_comprehension(self, items):
        return [self.value + i for i in items]

    def in_string(self):
        return f\"{self.value}\"

    def passed_on(self):
        return helper(self)

    def with_super(self):
        return super().__repr__()

    def with_class_cell(self):
        return __class__
";
        assert_eq!(could_be_static(text), [] as [&str; 0]);
    }

    /// SPEC-0098/AK-9: decorators, dunder methods, placeholders, base classes, test classes,
    /// another first parameter and nested functions are never reported.
    #[test]
    fn methods_that_are_never_reported() {
        let text = "\
import abc


class Plain:
    @staticmethod
    def static():
        return 1

    @classmethod
    def make(cls):
        return 1

    @property
    def value(self):
        return 1

    @abc.abstractmethod
    def must(self):
        return 1

    @unknown(3)
    def wrapped(self):
        return 1

    def __len__(self):
        return 0

    def todo(self):
        pass

    def later(self):
        ...

    def documented(self):
        \"\"\"Subclasses say what this does.\"\"\"

    def documented_pass(self):
        \"\"\"Nothing yet.\"\"\"
        pass

    def not_yet(self):
        raise NotImplementedError

    def not_yet_called(self):
        \"\"\"Subclasses do it.\"\"\"
        raise NotImplementedError(\"later\")

    def other(this):
        return 1

    def no_parameters():
        return 1

    def starred(*self):
        return 1

    def defaulted(self=None):
        return 1

    def outer(self):
        def inner(self):
            return 1
        return inner(self)

    if True:
        def conditional(self):
            return 1


class Child(Plain):
    def area(self):
        return 0


class Meta(metaclass=abc.ABCMeta):
    def area(self):
        return 0


class Generic(Base[int], object):
    def area(self):
        return 0


class TestShape:
    def test_area(self):
        assert 1 == 1

    def helper(self):
        return 1
";
        assert_eq!(could_be_static(text), [] as [&str; 0]);
        // A raise of another error is a real body.
        let other = "class A:\n    def f(self):\n        raise ValueError('x')\n";
        assert_eq!(could_be_static(other), ["2: f"]);
    }
}
