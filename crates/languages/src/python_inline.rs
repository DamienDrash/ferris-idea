//! Inline Variable for Python, computed from the syntax tree (`SPEC-0330` v4 behaviours 17 to
//! 21): the Python language servers offer no inline refactoring, so `Inline` falls back to this
//! own action. Every read of a local variable that is assigned once is replaced by the assigned
//! expression and the assignment is removed. The expression then runs where the reads stand:
//! later, and as often as there are reads. Whatever could make that visible is refused with its
//! reason; a refusal is preferred to code that could behave differently.

use std::collections::HashSet;
use std::ops::Range as Bytes;

use tree_sitter::Node;

use crate::actions::{CodeAction, RefactoringTarget};
use crate::insight::Range;
use crate::python_extract::{
    COMPREHENSIONS, all_identifiers, bound_of, byte_of, children, comprehension_targets,
    contains_own, edit, ends_its_line, evaluated_before, evaluated_once, field, first_of,
    free_into, in_place, is_field, line_start, named, param_names, parse, range_of, selection,
    starts_its_line, statement_of, syntax_check, word,
};
use crate::rename::Edit;

/// Title of the own Inline Variable action.
pub const INLINE_VARIABLE: &str = "Inline variable";

/// What Inline can do, said with every refusal of another kind of name.
const SUPPORTED: &str = "Inline supports a local variable of a function that is assigned once, \
                         by `name = expression`";

/// What can have a side effect where it is evaluated (behaviour 8).
const EFFECTS: &[&str] = &["call", "await", "yield", "named_expression"];

/// What creates a new object each time it is evaluated.
const FRESH: &[&str] = &[
    "list",
    "dictionary",
    "set",
    "list_comprehension",
    "dictionary_comprehension",
    "set_comprehension",
    "generator_expression",
    "lambda",
];

fn shown(n: Node, text: &str) -> String {
    let t = word(n, text);
    let line = t.lines().next().unwrap_or_default();
    if line.chars().count() > 40 || line.len() < t.len() {
        format!("{}…", line.chars().take(40).collect::<String>())
    } else {
        t.to_owned()
    }
}

/// The 1-based line of byte `at`.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

fn has_kind<'t>(n: Node<'t>, kinds: &[&str]) -> Option<Node<'t>> {
    if kinds.contains(&n.kind()) {
        return Some(n);
    }
    children(n).into_iter().find_map(|c| has_kind(c, kinds))
}

/// The name the caret touches or the selection covers.
fn name_at<'t>(root: Node<'t>, text: &str, range: &Range) -> Option<Node<'t>> {
    let sel = selection(text, range);
    let identifier = |r: Bytes<usize>| {
        root.descendant_for_byte_range(r.start, r.end)
            .filter(|n| n.kind() == "identifier")
    };
    if !sel.is_empty() {
        return identifier(sel.clone()).filter(|n| n.byte_range() == sel);
    }
    let at = byte_of(text, range.start_line, range.start_character);
    identifier(at..(at + 1).min(text.len()))
        .or_else(|| identifier(at.saturating_sub(1)..at))
        .filter(|n| n.start_byte() <= at && at <= n.end_byte())
}

/// The assignment statement and its value when `binding` is the target of a plain `name =
/// expression`; else what binds the name instead.
fn plain_assignment<'t>(binding: Node<'t>) -> Result<(Node<'t>, Node<'t>), String> {
    let mut c = binding;
    while let Some(p) = c.parent() {
        let reason = match p.kind() {
            "assignment" if p.id() == binding.parent().map_or(0, |x| x.id()) => {
                let statement = p.parent().filter(|s| {
                    s.kind() == "expression_statement"
                        && named(*s).len() == 1
                        && s.parent().is_some_and(|b| b.kind() == "block")
                });
                match (statement, field(p, "right")) {
                    _ if field(p, "type").is_some() => "it is declared with an annotation",
                    (Some(s), Some(right)) if right.kind() != "assignment" => {
                        return Ok((s, right));
                    }
                    _ => "it is assigned in a chained assignment",
                }
            }
            "assignment" => "it is assigned together with other names",
            "augmented_assignment" => "it is changed by an augmented assignment",
            "for_statement" | "for_in_clause" => "it is a loop variable",
            "as_pattern"
            | "as_pattern_target"
            | "with_item"
            | "except_clause"
            | "except_group_clause" => "it is bound by the `as` of a `with` or `except`",
            "import_statement" | "import_from_statement" | "future_import_statement" => {
                "it is an imported name"
            }
            "named_expression" => "it is assigned by `:=`",
            "function_definition" => {
                return Err(format!(
                    "it is a function, and inlining a function is not supported; {SUPPORTED}"
                ));
            }
            "class_definition" => "it is a class",
            "block" | "module" => break,
            _ => "",
        };
        if !reason.is_empty() {
            return Err(format!("{reason}; {SUPPORTED}"));
        }
        c = p;
    }
    Err(format!("it is not assigned by a statement; {SUPPORTED}"))
}

/// What the function body says about the variable `name` bound at `binding`.
#[derive(Default)]
struct Uses<'t> {
    reads: Vec<Node<'t>>,
    /// Names nested functions declare `nonlocal` or `global`.
    declared: HashSet<String>,
    /// Attributes and subscripts that are assigned or deleted: the name they start with, where,
    /// and their text.
    stores: Vec<(String, usize, String)>,
    refusal: Option<String>,
}

impl<'t> Uses<'t> {
    fn refuse(&mut self, why: String) {
        if self.refusal.is_none() {
            self.refusal = Some(why);
        }
    }

    /// The attributes and subscripts the target `t` writes through.
    fn store(&mut self, t: Node<'t>, text: &str) {
        match t.kind() {
            "attribute" | "subscript" => {
                let mut root = t;
                while let Some(inner) = field(root, "object").or_else(|| field(root, "value")) {
                    root = inner;
                }
                if root.kind() == "identifier" {
                    self.stores
                        .push((word(root, text).to_owned(), t.start_byte(), shown(t, text)));
                }
            }
            "identifier" => {}
            _ => {
                for c in named(t) {
                    self.store(c, text);
                }
            }
        }
    }

    fn walk(
        &mut self,
        n: Node<'t>,
        text: &str,
        name: &str,
        binding: Node<'t>,
        nested: Option<&'static str>,
    ) {
        let mut nested = nested;
        match n.kind() {
            "function_definition" => nested = nested.or(Some("a nested function")),
            "lambda" => nested = nested.or(Some("a `lambda`")),
            "class_definition" => nested = nested.or(Some("a class body")),
            "generator_expression" => nested = nested.or(Some("a generator expression")),
            "match_statement" => self.refuse(
                "the function has a `match` statement, whose captures are not followed".into(),
            ),
            "call" => {
                if let Some(f) = field(n, "function")
                    && f.kind() == "identifier"
                    && matches!(word(f, text), "locals" | "vars" | "eval" | "exec" | "dir")
                {
                    self.refuse(format!(
                        "the function calls `{}`, which sees its local names",
                        word(f, text)
                    ));
                }
            }
            "global_statement" | "nonlocal_statement" if nested.is_some() => {
                self.declared
                    .extend(named(n).into_iter().map(|c| word(c, text).to_owned()));
                return;
            }
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause"
                if nested.is_none() =>
            {
                if let Some(left) = field(n, "left") {
                    self.store(left, text);
                }
            }
            "as_pattern_target" if nested.is_none() => self.store(n, text),
            "delete_statement" if nested.is_none() => {
                for c in named(n) {
                    let targets = if c.kind() == "expression_list" {
                        named(c)
                    } else {
                        vec![c]
                    };
                    for t in targets {
                        if t.kind() == "identifier" {
                            // `del` of a name unbinds it: as a store of the name itself.
                            self.stores.push((
                                word(t, text).to_owned(),
                                t.start_byte(),
                                format!("del {}", word(t, text)),
                            ));
                        } else {
                            self.store(t, text);
                        }
                    }
                }
            }
            "identifier" if word(n, text) == name && n.id() != binding.id() => {
                let p = n.parent();
                let not_a_name = p.is_some_and(|p| {
                    (p.kind() == "attribute" && is_field(p, "attribute", n))
                        || (p.kind() == "keyword_argument" && is_field(p, "name", n))
                });
                if not_a_name {
                    return;
                }
                if let Some(scope) = nested {
                    self.refuse(format!(
                        "it is also used in {scope} (line {}), which reads the variable when it \
                         runs, not where it stands",
                        line_of(text, n.start_byte())
                    ));
                } else {
                    self.reads.push(n);
                }
                return;
            }
            _ => {}
        }
        for c in children(n) {
            self.walk(c, text, name, binding, nested);
        }
    }
}

/// The value without parentheses that hold nothing but one expression on one line.
fn unwrapped<'t>(mut value: Node<'t>, text: &str) -> Node<'t> {
    while value.kind() == "parenthesized_expression"
        && !word(value, text).contains('\n')
        && children(value).iter().all(|c| c.kind() != "comment")
        && named(value).len() == 1
    {
        value = named(value)[0];
    }
    value
}

/// How tightly the expression `n` binds: 15 for one that never needs parentheses, -1 for one that
/// always does (The Python Language Reference, "Operator precedence").
fn precedence(n: Node) -> i8 {
    let operator = field(n, "operator").map(|o| o.kind()).unwrap_or_default();
    match n.kind() {
        "lambda" => 0,
        "conditional_expression" => 1,
        "boolean_operator" if operator == "or" => 2,
        "boolean_operator" => 3,
        "not_operator" => 4,
        "comparison_operator" => 5,
        "binary_operator" => match operator {
            "|" => 6,
            "^" => 7,
            "&" => 8,
            "<<" | ">>" => 9,
            "+" | "-" => 10,
            "**" => 13,
            _ => 11,
        },
        "unary_operator" => 12,
        "await" => 14,
        "identifier"
        | "integer"
        | "float"
        | "string"
        | "concatenated_string"
        | "true"
        | "false"
        | "none"
        | "ellipsis"
        | "call"
        | "attribute"
        | "subscript"
        | "list"
        | "dictionary"
        | "set"
        | "tuple"
        | "parenthesized_expression"
        | "list_comprehension"
        | "dictionary_comprehension"
        | "set_comprehension"
        | "generator_expression" => 15,
        _ => -1,
    }
}

/// Whether `value` needs parentheses where the read `read` stands (behaviour 20).
fn needs_parentheses(value: Node, read: Node, text: &str) -> bool {
    let v = precedence(value);
    if v < 0 || (v < 15 && word(value, text).contains('\n')) {
        return true;
    }
    let Some(p) = read.parent() else {
        return true;
    };
    let is = |name: &str| is_field(p, name, read);
    match p.kind() {
        "binary_operator" if field(p, "operator").is_some_and(|o| o.kind() == "**") => {
            if is("left") { v <= 13 } else { v < 12 }
        }
        "binary_operator" | "boolean_operator" => {
            if is("left") {
                v < precedence(p)
            } else {
                v <= precedence(p)
            }
        }
        "comparison_operator" => v <= 5,
        "not_operator" => v < 4,
        "unary_operator" => v < 12,
        "conditional_expression" => v < 2,
        "attribute" => v < 15 || matches!(value.kind(), "integer" | "float"),
        "call" if is("function") => v < 15,
        "subscript" if is("value") => v < 15,
        "pair" if is("key") => v < 2,
        "interpolation" => {
            v == 0
                || matches!(
                    value.kind(),
                    "dictionary" | "set" | "dictionary_comprehension" | "set_comprehension"
                )
        }
        "argument_list"
        | "keyword_argument"
        | "expression_statement"
        | "return_statement"
        | "parenthesized_expression"
        | "list"
        | "tuple"
        | "set"
        | "pair"
        | "subscript"
        | "expression_list"
        | "if_statement"
        | "elif_clause"
        | "while_statement" => false,
        "assignment" | "augmented_assignment" | "for_statement" => !is("right"),
        _ => v < 15,
    }
}

/// The start of the first block of statement `s` after byte `at`, or the end of `s`: where the
/// part of `s` ends that is evaluated together with a read in its head.
fn head_end(s: Node, at: usize) -> usize {
    let mut end = s.end_byte();
    for c in children(s) {
        let blocks = if c.kind() == "block" {
            vec![c]
        } else {
            children(c)
                .into_iter()
                .filter(|x| x.kind() == "block")
                .collect()
        };
        for b in blocks {
            if b.start_byte() >= at {
                end = end.min(b.start_byte());
            }
        }
    }
    end
}

/// The first part of `n` starting in `within` that may change what an attribute or item of one of
/// `roots` gives: a call that is handed one of them (as its object or in an argument), or an
/// `await` or `yield`, during which other code runs. Nested functions and lambdas only define.
fn touching<'t>(
    n: Node<'t>,
    roots: &HashSet<String>,
    within: &Bytes<usize>,
    text: &str,
) -> Option<Node<'t>> {
    if n.end_byte() <= within.start || n.start_byte() >= within.end {
        return None;
    }
    if within.contains(&n.start_byte()) {
        match n.kind() {
            "await" | "yield" => return Some(n),
            "call" => {
                let mut names = HashSet::new();
                all_identifiers(n, text, false, &mut names);
                if roots.iter().any(|r| names.contains(r)) {
                    return Some(n);
                }
            }
            _ => {}
        }
    }
    children(n)
        .into_iter()
        .filter(|c| !matches!(c.kind(), "function_definition" | "lambda"))
        .find_map(|c| touching(c, roots, within, text))
}

/// The names whose attributes or items `n` reads.
fn state_roots(n: Node, text: &str, out: &mut HashSet<String>) {
    if matches!(n.kind(), "attribute" | "subscript") {
        let mut root = n;
        while let Some(inner) = field(root, "object").or_else(|| field(root, "value")) {
            root = inner;
        }
        if root.kind() == "identifier" {
            out.insert(word(root, text).to_owned());
        }
    }
    for c in children(n) {
        state_roots(c, text, out);
    }
}

/// Whether nothing of the statement `s` can be seen from outside: `pass`, or an assignment of an
/// expression without side effects to plain names.
fn harmless(s: Node) -> bool {
    if s.kind() == "pass_statement" {
        return true;
    }
    let inner = named(s);
    if s.kind() != "expression_statement" || inner.len() != 1 || inner[0].kind() != "assignment" {
        return false;
    }
    let plain = |t: Node| {
        t.kind() == "identifier"
            || (matches!(t.kind(), "pattern_list" | "tuple_pattern")
                && named(t).iter().all(|x| x.kind() == "identifier"))
    };
    match (field(inner[0], "left"), field(inner[0], "right")) {
        (Some(left), Some(right)) => plain(left) && first_of(right, EFFECTS).is_none(),
        _ => false,
    }
}

/// The outermost loop or comprehension around `read`, below the statement `top`.
fn loop_of<'t>(read: Node<'t>, top: Node<'t>) -> Option<Node<'t>> {
    let mut found = None;
    let mut c = read;
    while c.id() != top.id() {
        let p = c.parent()?;
        let repeats = match p.kind() {
            "for_statement" => !is_field(p, "right", c),
            "while_statement" => true,
            k => COMPREHENSIONS.contains(&k),
        };
        if repeats {
            found = Some(p);
        }
        c = p;
    }
    found
}

/// The edits of Inline Variable for the name `id`, or why it is refused (behaviours 18 to 20).
fn inline_edits(root: Node, text: &str, id: Node) -> Result<Vec<Edit>, String> {
    let name = word(id, text);
    syntax_check(id)?;
    let function = || {
        Err(format!(
            "it is a function, and inlining a function is not supported; {SUPPORTED}"
        ))
    };
    let callee = id
        .parent()
        .is_some_and(|p| p.kind() == "call" && is_field(p, "function", id));
    if let Some(p) = id.parent() {
        let parameters = |n: Node| matches!(n.kind(), "parameters" | "lambda_parameters");
        let parameter = match p.kind() {
            "default_parameter" | "typed_default_parameter" => is_field(p, "name", id),
            "typed_parameter" => true,
            "list_splat_pattern" | "dictionary_splat_pattern" => p
                .parent()
                .is_some_and(|g| parameters(g) || g.kind() == "typed_parameter"),
            _ => parameters(p),
        };
        if parameter {
            return Err(format!("it is a parameter; {SUPPORTED}"));
        }
        match p.kind() {
            "function_definition" if is_field(p, "name", id) => return function(),
            "class_definition" if is_field(p, "name", id) => {
                return Err(format!("it is a class; {SUPPORTED}"));
            }
            "attribute" if is_field(p, "attribute", id) => {
                if p.parent()
                    .is_some_and(|c| c.kind() == "call" && is_field(c, "function", p))
                {
                    return function();
                }
                return Err(format!("it is an attribute; {SUPPORTED}"));
            }
            "keyword_argument" if is_field(p, "name", id) => {
                return Err(format!("it is the name of a keyword argument; {SUPPORTED}"));
            }
            _ => {}
        }
    }

    // The function whose local variable the name is.
    let mut owner = None;
    let mut c = id;
    while let Some(p) = c.parent() {
        match p.kind() {
            "lambda" => {
                if field(p, "parameters")
                    .is_some_and(|ps| param_names(ps, text).contains(&name.to_owned()))
                {
                    return Err(format!("it is a parameter of a `lambda`; {SUPPORTED}"));
                }
            }
            k if COMPREHENSIONS.contains(&k) => {
                if comprehension_targets(p, text).contains(name) {
                    return Err(format!(
                        "it is the variable of a comprehension; {SUPPORTED}"
                    ));
                }
            }
            "class_definition" if is_field(p, "body", c) => {
                if bound_of(c, text).names.iter().any(|(n, _)| n == name) {
                    return Err(format!("it is a name of a class body; {SUPPORTED}"));
                }
            }
            "function_definition" if is_field(p, "body", c) => {
                let bound = bound_of(c, text);
                if bound.outer.contains(name) {
                    return Err(format!(
                        "it is declared `global` or `nonlocal`; {SUPPORTED}"
                    ));
                }
                if field(p, "parameters")
                    .is_some_and(|ps| param_names(ps, text).contains(&name.to_owned()))
                {
                    return Err(format!("it is a parameter; {SUPPORTED}"));
                }
                if bound.names.iter().any(|(n, _)| n == name) {
                    owner = Some((p, c, bound));
                    break;
                }
            }
            _ => {}
        }
        c = p;
    }
    let Some((_, body, bound)) = owner else {
        let module = bound_of(root, text);
        let bindings: Vec<Node> = module
            .names
            .iter()
            .filter(|(n, _)| n == name)
            .filter_map(|(_, at)| root.descendant_for_byte_range(*at, at + name.len()))
            .collect();
        let defined = bindings.iter().any(|b| {
            b.parent()
                .is_some_and(|p| p.kind() == "function_definition")
        });
        if callee || defined {
            return function();
        }
        if bindings
            .iter()
            .any(|b| b.parent().is_some_and(|p| p.kind() == "class_definition"))
        {
            return Err(format!("it is a class; {SUPPORTED}"));
        }
        if bindings.is_empty() {
            return Err(format!("it is not a variable of this file; {SUPPORTED}"));
        }
        return Err(format!(
            "it is a module-level name, which other modules and every function of the file can \
             read; {SUPPORTED}"
        ));
    };

    // Exactly one assignment, a plain one.
    let bindings: Vec<usize> = bound
        .names
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, at)| *at)
        .collect();
    let node_at = |at: usize| {
        root.descendant_for_byte_range(at, at + name.len())
            .filter(|n| n.kind() == "identifier")
            .ok_or_else(|| format!("its assignment was not found; {SUPPORTED}"))
    };
    if bindings.len() != 1 {
        for at in &bindings {
            let b = node_at(*at)?;
            if b.parent()
                .is_some_and(|p| p.kind() == "function_definition")
            {
                return function();
            }
        }
        return Err(format!(
            "it is assigned {} times (lines {}); {SUPPORTED}",
            bindings.len(),
            bindings
                .iter()
                .map(|at| line_of(text, *at).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let binding = node_at(bindings[0])?;
    let (assignment, value) = plain_assignment(binding)?;
    if !starts_its_line(text, assignment) || !ends_its_line(text, assignment.end_byte()) {
        return Err("its assignment shares its line with other code".into());
    }
    let value = unwrapped(value, text);
    if value.kind() == "yield" || contains_own(value, &["yield"]) {
        return Err("its value contains `yield`".into());
    }
    if contains_own(value, &["named_expression"]) {
        return Err("its value contains an assignment expression (`:=`)".into());
    }

    // The reads, and what else the function does with names.
    let mut uses = Uses::default();
    uses.walk(body, text, name, binding, None);
    if let Some(why) = uses.refusal {
        return Err(why);
    }
    // A declaration `name: type` without a value binds nothing but would be left behind.
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        if n.kind() == "assignment"
            && field(n, "type").is_some()
            && field(n, "left").is_some_and(|l| l.kind() == "identifier" && word(l, text) == name)
        {
            return Err(format!("it is declared with an annotation; {SUPPORTED}"));
        }
        stack.extend(children(n).into_iter().filter(|c| {
            !matches!(
                c.kind(),
                "function_definition" | "lambda" | "class_definition"
            )
        }));
    }
    if let Some((_, at, what)) = uses.stores.iter().find(|(root, _, _)| root == name) {
        return Err(format!(
            "it is changed through `{what}` (line {})",
            line_of(text, *at)
        ));
    }
    if uses.reads.is_empty() {
        return Err("it is never read".into());
    }
    let block = assignment.parent().ok_or("its assignment is in no block")?;
    let after = assignment.end_byte();

    // Every read is reached only through the assignment: later in the same block.
    let mut tops = Vec::new();
    for read in &uses.reads {
        let mut c = *read;
        let mut top = None;
        while let Some(p) = c.parent() {
            if p.id() == block.id() {
                top = Some(c);
                break;
            }
            c = p;
        }
        let line = line_of(text, read.start_byte());
        match top {
            Some(t) if t.start_byte() >= after => tops.push(t),
            Some(t) if t.id() == assignment.id() => {
                return Err("its own assignment reads it".into());
            }
            _ => {
                return Err(format!(
                    "the read on line {line} can be reached without the assignment on line {}: \
                     it stands before it, or outside the block the assignment is in",
                    line_of(text, assignment.start_byte())
                ));
            }
        }
    }

    let count = uses.reads.len();

    // A value with side effects runs exactly once and at the same time as before (behaviour 19).
    if let Some(effect) = first_of(value, &["call", "await"]) {
        let effect = shown(effect, text);
        if count != 1 {
            return Err(format!(
                "its value contains `{effect}`, which would then run {count} times instead of \
                 once (it is read on lines {})",
                uses.reads
                    .iter()
                    .map(|r| line_of(text, r.start_byte()).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let (read, top) = (uses.reads[0], tops[0]);
        let line = line_of(text, read.start_byte());
        let statement = statement_of(read).ok_or("the read is in no statement")?;
        if statement.id() != top.id() {
            return Err(format!(
                "its value contains `{effect}`, and the read on line {line} is inside a block \
                 that does not run exactly once after the assignment"
            ));
        }
        evaluated_once(read, statement).map_err(|why| {
            format!(
                "its value contains `{effect}`, and the read on line {line} is not evaluated \
                 exactly once: {}",
                why.strip_prefix("it is ").unwrap_or(&why)
            )
        })?;
        for between in named(block) {
            if between.start_byte() >= after
                && between.end_byte() <= top.start_byte()
                && !harmless(between)
            {
                return Err(format!(
                    "its value contains `{effect}`, which would then run after `{}` (line {})",
                    shown(between, text),
                    line_of(text, between.start_byte())
                ));
            }
        }
        for part in evaluated_before(read, statement) {
            if let Some(first) = first_of(part, EFFECTS) {
                return Err(format!(
                    "its value contains `{effect}`, which would then run after `{}`, which comes \
                     first in the statement and can have side effects",
                    shown(first, text)
                ));
            }
        }
    }

    // A value that is a new object each time is read once, and not in a loop.
    if let Some(fresh) = has_kind(value, FRESH) {
        let fresh = shown(fresh, text);
        if count != 1 {
            return Err(format!(
                "its value creates a new object (`{fresh}`) each time it is evaluated; the \
                 {count} reads would no longer share one"
            ));
        }
        if loop_of(uses.reads[0], tops[0]).is_some() {
            return Err(format!(
                "its value creates a new object (`{fresh}`) each time it is evaluated, and the \
                 read on line {} is inside a loop",
                line_of(text, uses.reads[0].start_byte())
            ));
        }
    }

    // What the value reads is the same at every read.
    let mut free = Vec::new();
    free_into(value, text, &mut free);
    let inputs: HashSet<String> = free.into_iter().map(|(n, _)| n).collect();
    if let Some(n) = inputs.iter().find(|n| uses.declared.contains(*n)) {
        return Err(format!(
            "its value reads `{n}`, which a nested function assigns (`nonlocal` or `global`)"
        ));
    }
    let value_text = word(value, text);
    let mut roots = HashSet::new();
    state_roots(value, text, &mut roots);
    for (read, top) in uses.reads.iter().zip(&tops) {
        let line = line_of(text, read.start_byte());
        let statement = statement_of(*read).ok_or("the read is in no statement")?;
        let around = loop_of(*read, *top);
        let end = match around {
            Some(l) => l.end_byte(),
            None => head_end(statement, read.end_byte()),
        };
        // An attribute or item the value reads may be changed by a call that gets the object.
        let changer = match around {
            Some(l) => touching(body, &roots, &(after..l.end_byte()), text),
            None => touching(body, &roots, &(after..statement.start_byte()), text).or_else(|| {
                evaluated_before(*read, statement)
                    .into_iter()
                    .find_map(|part| touching(part, &roots, &(0..text.len()), text))
            }),
        };
        if let Some(c) = changer.filter(|_| !roots.is_empty()) {
            return Err(format!(
                "its value reads an attribute or item of an object that `{}` on line {} may \
                 change before the read on line {line} is done",
                shown(c, text),
                line_of(text, c.start_byte())
            ));
        }
        let between = |at: usize| at >= after && at < end;
        if let Some((n, at)) = bound
            .names
            .iter()
            .find(|(n, at)| inputs.contains(n) && between(*at))
        {
            return Err(format!(
                "its value reads `{n}`, which is assigned again on line {} before the read on \
                 line {line} is done",
                line_of(text, *at)
            ));
        }
        if let Some((n, at, what)) = uses
            .stores
            .iter()
            .find(|(n, at, _)| inputs.contains(n) && between(*at))
        {
            return Err(format!(
                "its value reads `{n}`, and `{what}` on line {} changes it before the read on \
                 line {line} is done",
                line_of(text, *at)
            ));
        }
        // Scopes around the read, up to the function.
        let mut c = *read;
        while c.id() != top.id() {
            let Some(p) = c.parent() else { break };
            if COMPREHENSIONS.contains(&p.kind())
                && let Some(hidden) = comprehension_targets(p, text)
                    .into_iter()
                    .find(|t| inputs.contains(t))
            {
                return Err(format!(
                    "the read on line {line} is inside a comprehension whose variable \
                     `{hidden}` would hide the `{hidden}` its value reads"
                ));
            }
            if p.kind() == "type" {
                return Err(format!("the read on line {line} is in an annotation"));
            }
            if p.kind() == "interpolation" {
                if value_text.contains(['"', '\'', '\\', '\n', '#']) {
                    return Err(format!(
                        "the read on line {line} is inside an f-string, and its value has a \
                         quote, a backslash, a comment or a line break, which an f-string \
                         cannot hold before Python 3.12"
                    ));
                }
                if children(p).iter().any(|x| x.kind() == "=") {
                    return Err(format!(
                        "the read on line {line} is in an f-string with `=`, which prints the \
                         text of the expression"
                    ));
                }
            }
            c = p;
        }
    }

    // The assignment's lines go, each read becomes the value.
    let start = line_start(text, assignment.start_byte());
    let stop = text[after..]
        .find('\n')
        .map_or(text.len(), |i| after + i + 1);
    let mut edits = vec![edit(text, start..stop, String::new())];
    for read in &uses.reads {
        let new = if needs_parentheses(value, *read, text) {
            format!("({value_text})")
        } else {
            value_text.to_owned()
        };
        edits.push(edit(text, read.byte_range(), in_place(*read, text, &new)));
    }
    Ok(edits)
}

fn message(name: &str, why: &str) -> String {
    format!("Cannot inline `{name}`: {why}.")
}

const NO_NAME: &str = "There is no variable at the caret to inline.";

/// Where Inline Variable can act for `range` of `text`: the name under the caret or the selected
/// name, or the message that says why not (behaviours 17 and 21).
pub(crate) fn find(
    root: Node,
    text: &str,
    range: &Range,
) -> (Vec<RefactoringTarget>, Option<String>) {
    let Some(id) = name_at(root, text, range) else {
        return (Vec::new(), Some(NO_NAME.into()));
    };
    match inline_edits(root, text, id) {
        Ok(_) => (
            vec![RefactoringTarget {
                range: range_of(text, id.byte_range()),
                text: word(id, text).to_owned(),
                actions: vec![CodeAction {
                    title: INLINE_VARIABLE.to_owned(),
                    kind: Some("refactor.inline".into()),
                    preferred: false,
                    disabled: None,
                    fixes: Vec::new(),
                }],
            }],
            None,
        ),
        Err(why) => (Vec::new(), Some(message(word(id, text), &why))),
    }
}

/// The edits of Inline Variable for the name at `range` of `text` (a caret on the name or the
/// name selected), or why it is refused, without the message's frame.
pub fn inline_variable(text: &str, range: &Range) -> Result<Vec<Edit>, String> {
    let doc = parse(text)?;
    let root = doc.tree().root_node();
    let id = name_at(root, text, range).ok_or("there is no variable at the caret")?;
    inline_edits(root, text, id)
}

/// As [`inline_variable`]; a refusal is the message the status bar shows.
pub(crate) fn plan(text: &str, range: &Range) -> Result<Vec<Edit>, String> {
    let doc = parse(text)?;
    let root = doc.tree().root_node();
    let id = name_at(root, text, range).ok_or(NO_NAME)?;
    inline_edits(root, text, id).map_err(|why| message(word(id, text), &why))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Refactoring;
    use crate::python_extract::tests::marked;
    use crate::rename::apply_edits;

    fn inline(source: &str) -> Result<String, String> {
        let (text, range) = marked(source);
        inline_variable(&text, &range).map(|e| apply_edits(&text, &e).unwrap())
    }

    /// The body of `def f(...)` after inlining, for a body given without the `def` line.
    #[track_caller]
    fn body(lines: &str) -> String {
        let source = format!("def f(a, b, c, xs, self):\n{lines}");
        let after = inline(&source).unwrap_or_else(|why| panic!("refused: {why}\n{source}"));
        after
            .strip_prefix("def f(a, b, c, xs, self):\n")
            .expect("the def line stays")
            .to_owned()
    }

    #[track_caller]
    fn refused(source: &str, reason: &str) {
        match inline(source) {
            Ok(text) => panic!("not refused ({reason}):\n{text}"),
            Err(why) => assert!(why.contains(reason), "{why:?} does not name {reason:?}"),
        }
    }

    /// As [`refused`], for a body of `def f(a, b, c, xs, self):`.
    #[track_caller]
    fn refused_in(lines: &str, reason: &str) {
        refused(&format!("def f(a, b, c, xs, self):\n{lines}"), reason);
    }

    #[test]
    fn the_observed_variable() {
        // The case of the gap: one read in an f-string, a plain assignment in between.
        let source = "def describe(item: Item, factor: float | int = 1) -> str:\n    \"\"\"Return a label for an item.\"\"\"\n    va¦lue = item.total() * factor\n    unused = 3\n    return f\"{item.name}: {value}\" + undefined_name\n";
        assert_eq!(
            inline(source).unwrap(),
            "def describe(item: Item, factor: float | int = 1) -> str:\n    \"\"\"Return a label for an item.\"\"\"\n    unused = 3\n    return f\"{item.name}: {item.total() * factor}\" + undefined_name\n"
        );
    }

    #[test]
    fn caret_and_selection() {
        const AFTER: &str = "    return a * b + 1\n";
        // On the assignment and on the read, inside the name, before and after it, or selected.
        for lines in [
            "    ¦total = a * b\n    return total + 1\n",
            "    to¦tal = a * b\n    return total + 1\n",
            "    total¦ = a * b\n    return total + 1\n",
            "    total = a * b\n    return ¦total + 1\n",
            "    total = a * b\n    return total¦ + 1\n",
            "    total = a * b\n    return «total» + 1\n",
            "    «total» = a * b\n    return total + 1\n",
        ] {
            assert_eq!(body(lines), AFTER, "{lines}");
        }
        // Not on a name, or more than the name selected.
        refused(
            "def f(a):\n    total = a ¦* 2\n    return total\n",
            "no variable at the caret",
        );
        refused(
            "def f(a):\n    total = a * 2\n    return «total + 1»\n",
            "no variable at the caret",
        );
    }

    #[test]
    fn every_read_is_replaced() {
        assert_eq!(
            body(
                "    t¦ = a * b\n    print(t)\n    if t > c:\n        return t\n    return [t, t]\n"
            ),
            "    print(a * b)\n    if a * b > c:\n        return a * b\n    return [a * b, a * b]\n"
        );
        // In a loop and in a comprehension, for a value without side effects.
        assert_eq!(
            body(
                "    k¦ = a * 2\n    for x in xs:\n        print(x * k)\n    return [x + k for x in xs]\n"
            ),
            "    for x in xs:\n        print(x * (a * 2))\n    return [x + a * 2 for x in xs]\n"
        );
        // The assignment in a block, the reads later in that block.
        assert_eq!(
            body(
                "    if a:\n        t¦ = b + 1\n        print(t)\n        while c:\n            c = c - t\n    return c\n"
            ),
            "    if a:\n        print(b + 1)\n        while c:\n            c = c - (b + 1)\n    return c\n"
        );
        // Not a keyword argument's name and not an attribute of the same name.
        assert_eq!(
            body("    t¦ = a + 1\n    return self.t + g(t=t)\n"),
            "    return self.t + g(t=a + 1)\n"
        );
    }

    #[test]
    fn the_assignment_line_goes() {
        // With its comment, over several lines, with CRLF.
        assert_eq!(
            body("    t¦ = a + 1  # one more\n    return t\n"),
            "    return a + 1\n"
        );
        assert_eq!(
            body("    t¦ = g(\n        a,\n        b,\n    )\n    return t\n"),
            "    return g(\n        a,\n        b,\n    )\n"
        );
        assert_eq!(
            inline("def f(a):\r\n    t¦ = a + 1\r\n    return t\r\n").unwrap(),
            "def f(a):\r\n    return a + 1\r\n"
        );
        // Comment lines and empty lines around it stay.
        assert_eq!(
            body("    # the sum\n    t¦ = a + 1\n\n    return t\n"),
            "    # the sum\n\n    return a + 1\n"
        );
    }

    #[test]
    fn parentheses_where_precedence_needs_them() {
        let one = |value: &str, read: &str| {
            let after = body(&format!("    v¦ = {value}\n    return {read}\n"));
            after
                .strip_prefix("    return ")
                .and_then(|r| r.strip_suffix('\n'))
                .unwrap()
                .to_owned()
        };
        // Operators: the weaker one inside gets parentheses, the same one only on the right.
        assert_eq!(one("a + b", "v * c"), "(a + b) * c");
        assert_eq!(one("a * b", "v + c"), "a * b + c");
        assert_eq!(one("a - b", "v - c"), "a - b - c");
        assert_eq!(one("a - b", "c - v"), "c - (a - b)");
        assert_eq!(one("a * b", "c / v"), "c / (a * b)");
        assert_eq!(one("a or b", "v and c"), "(a or b) and c");
        assert_eq!(one("a and b", "v or c"), "a and b or c");
        assert_eq!(one("a and b", "c and v"), "c and (a and b)");
        assert_eq!(one("a < b", "v == c"), "(a < b) == c");
        assert_eq!(one("a < b", "not v"), "not a < b");
        assert_eq!(one("a or b", "not v"), "not (a or b)");
        assert_eq!(one("a + b", "v < c"), "a + b < c");
        assert_eq!(one("a | b", "v & c"), "(a | b) & c");
        assert_eq!(one("a << b", "v + c"), "(a << b) + c");
        // Power binds tighter than a sign on its left, not on its right.
        assert_eq!(one("-a", "v ** 2"), "(-a) ** 2");
        assert_eq!(one("-a", "2 ** v"), "2 ** -a");
        assert_eq!(one("a ** b", "v ** 2"), "(a ** b) ** 2");
        assert_eq!(one("a ** b", "2 ** v"), "2 ** a ** b");
        assert_eq!(one("a ** b", "-v"), "-a ** b");
        assert_eq!(one("a + b", "-v"), "-(a + b)");
        // A conditional expression and a lambda.
        assert_eq!(one("a if b else c", "v + 1"), "(a if b else c) + 1");
        assert_eq!(one("a if b else c", "g(v)"), "g(a if b else c)");
        assert_eq!(
            one("a if b else c", "1 if v else 2"),
            "1 if (a if b else c) else 2"
        );
        assert_eq!(one("lambda: a", "g(v, 1)"), "g(lambda: a, 1)");
        assert_eq!(one("lambda: a", "v()"), "(lambda: a)()");
        // Attribute, call and subscript need a primary.
        assert_eq!(one("a + b", "v.real"), "(a + b).real");
        assert_eq!(one("a.b", "v.c"), "a.b.c");
        assert_eq!(one("5", "v.real"), "(5).real");
        assert_eq!(one("a + b", "v[0]"), "(a + b)[0]");
        assert_eq!(one("a + b", "xs[v]"), "xs[a + b]");
        assert_eq!(one("\"a\" \"b\"", "v.upper()"), "\"a\" \"b\".upper()");
        assert_eq!(one("a[0]", "v(1)"), "a[0](1)");
        // A tuple without parentheses always gets them; one with them keeps them.
        assert_eq!(one("a, b", "g(v)"), "g((a, b))");
        assert_eq!(one("(a, b)", "g(v)"), "g((a, b))");
        assert_eq!(one("a, b", "v"), "(a, b)");
        // Parentheses that held one expression on one line are dropped where not needed.
        assert_eq!(one("(a + b)", "g(v)"), "g(a + b)");
        assert_eq!(one("(a + b)", "v * 2"), "(a + b) * 2");
        assert_eq!(one("((a))", "v * 2"), "a * 2");
        // Where any expression may stand.
        assert_eq!(
            one("a + b", "[v, {v: v}, (v), xs[v:v]]"),
            "[a + b, {a + b: a + b}, (a + b), xs[(a + b):(a + b)]]"
        );
        assert_eq!(one("a + b", "g(k=v)"), "g(k=a + b)");
        assert_eq!(one("a if b else c", "{v: 1}"), "{(a if b else c): 1}");
        assert_eq!(one("a + b", "c if v else 1"), "c if a + b else 1");
        assert_eq!(one("a + b", "await v"), "await (a + b)");
        assert_eq!(one("a + b", "g(*v)"), "g(*(a + b))");
        // A word stays apart from its neighbours.
        assert_eq!(one("a + b", "c if(v)else 1"), "c if(a + b)else 1");
        // In an f-string.
        assert_eq!(one("a + b", "f\"{v}\""), "f\"{a + b}\"");
        assert_eq!(one("a + b", "f\"{v!r:>10}\""), "f\"{a + b!r:>10}\"");
        assert_eq!(one("a + b", "f\"{v * 2}\""), "f\"{(a + b) * 2}\"");
        assert_eq!(one("lambda: a", "f\"{v}\""), "f\"{(lambda: a)}\"");
        assert_eq!(one("{a: b}", "f\"{v}\""), "f\"{({a: b})}\"");
        // Statements.
        assert_eq!(
            body(
                "    v¦ = a + b\n    if v:\n        pass\n    while v:\n        pass\n    for x in v:\n        pass\n    c = v\n    c += v\n    assert v\n"
            ),
            "    if a + b:\n        pass\n    while a + b:\n        pass\n    for x in a + b:\n        pass\n    c = a + b\n    c += a + b\n    assert (a + b)\n"
        );
        // A value over several lines keeps its parentheses, and gets them where it has none.
        assert_eq!(
            body("    v¦ = (a +\n         b)\n    return g(v)\n"),
            "    return g((a +\n         b))\n"
        );
        assert_eq!(
            body("    v¦ = a + \\\n        b\n    return v\n"),
            "    return (a + \\\n        b)\n"
        );
    }

    #[test]
    fn a_value_with_side_effects_is_read_once_and_at_once() {
        // Right in the next statement, or after plain assignments without side effects.
        assert_eq!(body("    v¦ = g(a)\n    return v\n"), "    return g(a)\n");
        assert_eq!(
            body(
                "    v¦ = g(a)\n    # then\n    c = b + 1\n    x, y = b, 2\n    pass\n    return h(v, c)\n"
            ),
            "    # then\n    c = b + 1\n    x, y = b, 2\n    pass\n    return h(g(a), c)\n"
        );
        assert_eq!(
            body("    v¦ = await g(a)\n    return v + 1\n"),
            "    return await g(a) + 1\n"
        );
        // In the head of a compound statement, which is evaluated once.
        assert_eq!(
            body("    v¦ = g(a)\n    if v:\n        return 1\n"),
            "    if g(a):\n        return 1\n"
        );
        assert_eq!(
            body("    v¦ = g(a)\n    for x in v:\n        print(x)\n"),
            "    for x in g(a):\n        print(x)\n"
        );
        assert_eq!(
            body("    v¦ = g(a)\n    with v as h:\n        pass\n"),
            "    with g(a) as h:\n        pass\n"
        );
        assert_eq!(
            body("    v¦ = g(a)\n    return [x for x in v if x]\n"),
            "    return [x for x in g(a) if x]\n"
        );
        // After parts of the statement without side effects.
        assert_eq!(
            body("    v¦ = g(a)\n    return self.h(b + 1, v)\n"),
            "    return self.h(b + 1, g(a))\n"
        );
        assert_eq!(
            body("    v¦ = g(a)\n    self.total = v\n"),
            "    self.total = g(a)\n"
        );
        assert_eq!(
            body("    v¦ = g(a)\n    return b if v else c\n"),
            "    return b if g(a) else c\n"
        );

        // Read twice: it would run twice.
        refused_in(
            "    v¦ = g(a)\n    return v + v\n",
            "its value contains `g(a)`, which would then run 2 times instead of once (it is read on lines 3, 3)",
        );
        refused_in(
            "    v¦ = a.b.c()\n    print(v)\n    return v\n",
            "run 2 times",
        );
        // Inside a block: not exactly once.
        refused_in(
            "    v¦ = g(a)\n    if b:\n        return v\n",
            "the read on line 4 is inside a block that does not run exactly once",
        );
        refused_in(
            "    v¦ = g(a)\n    for x in xs:\n        print(v)\n",
            "inside a block",
        );
        refused_in(
            "    v¦ = g(a)\n    try:\n        print(v)\n    finally:\n        pass\n",
            "inside a block",
        );
        // Evaluated only sometimes, or several times.
        refused_in(
            "    v¦ = g(a)\n    return b and v\n",
            "the read on line 3 is not evaluated exactly once: the right side of `and`/`or` is evaluated only sometimes there",
        );
        refused_in(
            "    v¦ = g(a)\n    return b if c else v\n",
            "a branch of a conditional expression",
        );
        refused_in(
            "    v¦ = g(a)\n    return a < b < v\n",
            "a later operand of a chained comparison",
        );
        refused_in(
            "    v¦ = g(a)\n    while v:\n        pass\n",
            "the condition of `while`",
        );
        refused_in(
            "    v¦ = g(a)\n    if b:\n        pass\n    elif v:\n        pass\n",
            "the condition of `elif`",
        );
        refused_in(
            "    v¦ = g(a)\n    return [x * v for x in xs]\n",
            "not evaluated exactly once: inside a comprehension",
        );
        refused_in("    v¦ = g(a)\n    assert v\n", "an `assert` is skipped");
        refused_in(
            "    v¦ = g(a)\n    with b as x, v as y:\n        pass\n",
            "a later item of `with`",
        );
        // Something with side effects would then run before it.
        refused_in(
            "    v¦ = g(a)\n    print(b)\n    return v\n",
            "its value contains `g(a)`, which would then run after `print(b)` (line 3)",
        );
        refused_in(
            "    v¦ = g(a)\n    c = h()\n    return v\n",
            "run after `c = h()`",
        );
        refused_in(
            "    v¦ = g(a)\n    self.x = 1\n    return v\n",
            "run after `self.x = 1`",
        );
        refused_in(
            "    v¦ = g(a)\n    xs[0] = 1\n    return v\n",
            "run after `xs[0] = 1`",
        );
        refused_in(
            "    v¦ = g(a)\n    import os\n    return v\n",
            "run after `import os`",
        );
        refused_in(
            "    v¦ = g(a)\n    if b:\n        c = 1\n    return v\n",
            "run after `if b:…`",
        );
        refused_in(
            "    v¦ = g(a)\n    return h() + v\n",
            "which would then run after `h()`, which comes first in the statement and can have side effects",
        );
        refused_in("    v¦ = g(a)\n    return k(h(), v)\n", "run after `h()`");
        refused_in(
            "    v¦ = g(a)\n    return f\"{h()} {v}\"\n",
            "run after `h()`",
        );
        refused_in(
            "    v¦ = g(a)\n    return [(n := b), v]\n",
            "run after `n := b`",
        );
        // `yield` and `:=` in the value.
        refused_in(
            "    v¦ = yield a\n    return v\n",
            "its value contains `yield`",
        );
        refused_in(
            "    v¦ = (yield)\n    return v\n",
            "its value contains `yield`",
        );
        refused_in(
            "    v¦ = (n := a) + 1\n    return v\n",
            "an assignment expression",
        );
    }

    #[test]
    fn a_new_object_is_read_once() {
        assert_eq!(body("    v¦ = []\n    return v\n"), "    return []\n");
        assert_eq!(
            body("    v¦ = {a: b}\n    print(c)\n    return g(v)\n"),
            "    print(c)\n    return g({a: b})\n"
        );
        assert_eq!(
            body("    key¦ = lambda x: x.b\n    return sorted(xs, key=key)\n"),
            "    return sorted(xs, key=lambda x: x.b)\n"
        );
        refused_in(
            "    v¦ = []\n    v.append(a)\n    return v\n",
            "its value creates a new object (`[]`) each time it is evaluated; the 2 reads would no longer share one",
        );
        refused_in(
            "    v¦ = {a: b}\n    return v, v\n",
            "creates a new object (`{a: b}`)",
        );
        refused_in(
            "    v¦ = (x for x in xs)\n    return list(v), list(v)\n",
            "creates a new object",
        );
        refused_in(
            "    v¦ = a + [b]\n    return v, v\n",
            "creates a new object (`[b]`)",
        );
        refused_in(
            "    v¦ = []\n    for x in xs:\n        v.append(x)\n",
            "and the read on line 4 is inside a loop",
        );
        refused_in(
            "    v¦ = set()\n    v.add(a)\n    return v\n",
            "run 2 times",
        );
    }

    #[test]
    fn what_the_value_reads_stays_the_same() {
        // A name assigned again between the assignment and a read.
        refused_in(
            "    v¦ = a + 1\n    a = 5\n    return v\n",
            "its value reads `a`, which is assigned again on line 3 before the read on line 4 is done",
        );
        refused_in(
            "    v¦ = a + 1\n    a += 1\n    return v\n",
            "assigned again on line 3",
        );
        refused_in(
            "    v¦ = a + 1\n    for a in xs:\n        pass\n    return v\n",
            "assigned again on line 3",
        );
        refused_in(
            "    v¦ = a + 1\n    with c as a:\n        pass\n    return v\n",
            "assigned again on line 3",
        );
        refused_in(
            "    v¦ = a + 1\n    import a\n    return v\n",
            "assigned again on line 3",
        );
        refused_in(
            "    v¦ = a + 1\n    if c:\n        a = 0\n    return v\n",
            "assigned again on line 4",
        );
        refused_in(
            "    v¦ = a + 1\n    return [(a := 2), v]\n",
            "assigned again on line 3",
        );
        refused_in(
            "    v¦ = a + 1\n    print(v)\n    a = 2\n    return v\n",
            "before the read on line 5",
        );
        refused_in(
            "    v¦ = a + 1\n    del a\n    return v\n",
            "`del a` on line 3 changes it",
        );
        // In a loop around the read, also after the read.
        refused_in(
            "    v¦ = a + 1\n    for x in xs:\n        print(v)\n        a = x\n",
            "its value reads `a`, which is assigned again on line 5",
        );
        refused_in(
            "    v¦ = a + 1\n    for a in xs:\n        print(v)\n",
            "assigned again on line 3",
        );
        refused_in(
            "    v¦ = a + 1\n    while c:\n        print(v)\n        a -= 1\n",
            "assigned again on line 5",
        );
        // After the last read it may change.
        assert_eq!(
            body("    v¦ = a + 1\n    print(v)\n    a = 5\n    return a\n"),
            "    print(a + 1)\n    a = 5\n    return a\n"
        );
        assert_eq!(
            body("    v¦ = a + 1\n    if v:\n        a = 5\n    return a\n"),
            "    if a + 1:\n        a = 5\n    return a\n"
        );
        // A nested function that assigns it.
        refused(
            "def f(a):\n    def g():\n        nonlocal a\n        a = 2\n    v¦ = a + 1\n    g()\n    return v\n",
            "its value reads `a`, which a nested function assigns",
        );
        // An attribute or item assigned between.
        refused_in(
            "    v¦ = self.price * 2\n    self.price = 0\n    return v\n",
            "its value reads `self`, and `self.price` on line 3 changes it before the read on line 4 is done",
        );
        refused_in(
            "    v¦ = xs[0]\n    xs[0] += 1\n    return v\n",
            "`xs[0]` on line 3 changes it",
        );
        refused_in(
            "    v¦ = xs[0]\n    del xs[0]\n    return v\n",
            "`xs[0]` on line 3 changes it",
        );
        refused_in(
            "    v¦ = a.b.c\n    a.b = c\n    return v\n",
            "`a.b` on line 3 changes it",
        );
        refused_in(
            "    v¦ = len(xs)\n    for xs[0] in c:\n        pass\n    return v\n",
            "run after",
        );
        // A call that is handed the object whose attribute or item the value reads.
        refused_in(
            "    v¦ = self.count\n    self.bump()\n    return v\n",
            "its value reads an attribute or item of an object that `self.bump()` on line 3 may change before the read on line 4 is done",
        );
        refused_in(
            "    v¦ = xs[0]\n    g(xs)\n    return v\n",
            "`g(xs)` on line 3 may change",
        );
        refused_in(
            "    v¦ = xs[0]\n    return g(xs) + v\n",
            "`g(xs)` on line 3 may change",
        );
        refused_in(
            "    v¦ = self.count\n    for x in c:\n        print(v)\n        self.bump()\n",
            "`self.bump()` on line 5",
        );
        refused(
            "async def f(self, g):\n    v¦ = self.count\n    await g\n    return v\n",
            "`await g` on line 3 may change",
        );
        // Other calls, and calls that only get the variable's value, are taken not to.
        assert_eq!(
            body("    v¦ = self.count\n    print(a)\n    g(v)\n    return v + c\n"),
            "    print(a)\n    g(self.count)\n    return self.count + c\n"
        );
        assert_eq!(
            body("    v¦ = self.count\n    return g(v, self)\n"),
            "    return g(self.count, self)\n"
        );
        // A comprehension variable would hide the name.
        refused_in(
            "    v¦ = a + 1\n    return [v for a in xs]\n",
            "the read on line 3 is inside a comprehension whose variable `a` would hide the `a` its value reads",
        );
    }

    #[test]
    fn reads_reached_only_through_the_assignment() {
        refused_in(
            "    print(v)\n    v¦ = a + 1\n    return v\n",
            "the read on line 2 can be reached without the assignment on line 3",
        );
        refused_in(
            "    if c:\n        v¦ = a + 1\n    return v\n",
            "the read on line 4 can be reached without the assignment on line 3",
        );
        refused_in(
            "    for x in xs:\n        v¦ = x + 1\n    return v\n",
            "can be reached without the assignment",
        );
        refused_in(
            "    for x in xs:\n        print(v)\n        v¦ = x + 1\n",
            "can be reached without the assignment",
        );
        refused_in(
            "    try:\n        v¦ = a + 1\n    except E:\n        print(v)\n",
            "can be reached without the assignment",
        );
        refused_in(
            "    try:\n        v¦ = a + 1\n    finally:\n        print(v)\n",
            "can be reached without the assignment",
        );
        refused_in(
            "    with c:\n        v¦ = a + 1\n    return v\n",
            "can be reached without the assignment",
        );
        refused_in(
            "    v¦ = v + 1\n    return v\n",
            "its own assignment reads it",
        );
        refused_in("    v¦ = a + 1\n    return a\n", "it is never read");
    }

    #[test]
    fn not_a_plain_local_variable() {
        const WHAT: &str = "Inline supports a local variable of a function that is assigned once, by `name = expression`";
        // A function: at its `def`, at a call, as a method, defined locally or imported.
        for source in [
            "def he¦lp(a):\n    return a\n",
            "def help(a):\n    return a\n\n\nprint(he¦lp(1))\n",
            "def f(a):\n    return a.he¦lp()\n",
            "def f(a):\n    def help():\n        return a\n    return he¦lp()\n",
            "from m import help\n\n\ndef f(a):\n    return he¦lp(a)\n",
            "def help(a):\n    return a\n\n\ng = he¦lp\n",
        ] {
            refused(
                source,
                "it is a function, and inlining a function is not supported",
            );
            refused(source, WHAT);
        }
        let cases = [
            ("class Ite¦m:\n    pass\n", "it is a class"),
            ("class Item:\n    pass\n\n\nx = Ite¦m\n", "it is a class"),
            ("def f(a):\n    return a.na¦me\n", "it is an attribute"),
            (
                "def f(a):\n    return g(ke¦y=a)\n",
                "it is the name of a keyword argument",
            ),
            ("def f(a¦):\n    return a\n", "it is a parameter"),
            ("def f(a):\n    return a¦\n", "it is a parameter"),
            (
                "def f(a=1, *ar¦gs):\n    return args\n",
                "it is a parameter",
            ),
            (
                "def f(a: int, b¦: int = 2):\n    return b\n",
                "it is a parameter",
            ),
            (
                "def f(a):\n    return lambda x: x¦ + a\n",
                "it is a parameter of a `lambda`",
            ),
            (
                "def f(xs):\n    return [x¦ for x in xs]\n",
                "it is the variable of a comprehension",
            ),
            (
                "def f(xs):\n    return [x for x¦ in xs]\n",
                "it is the variable of a comprehension",
            ),
            (
                "class A:\n    size = 3\n    area = si¦ze * 2\n",
                "it is a name of a class body",
            ),
            (
                "def f():\n    global total\n    to¦tal = 1\n    return total\n",
                "it is declared `global` or `nonlocal`",
            ),
            (
                "def f():\n    t = 0\n    def g():\n        nonlocal t\n        t¦ = 1\n        return t\n    return g\n",
                "it is declared `global` or `nonlocal`",
            ),
            (
                "total = 3\nprint(to¦tal)\n",
                "it is a module-level name, which other modules and every function of the file can read",
            ),
            ("_total = 3\nprint(_to¦tal)\n", "it is a module-level name"),
            (
                "total = 3\n\n\ndef f():\n    return to¦tal\n",
                "it is a module-level name",
            ),
            (
                "def f(a):\n    return le¦n\n",
                "it is not a variable of this file",
            ),
            (
                "def f(a):\n    t¦ = 1\n    t = 2\n    return t\n",
                "it is assigned 2 times (lines 2, 3)",
            ),
            (
                "def f(a):\n    t = 1\n    t¦ += a\n    return t\n",
                "it is assigned 2 times",
            ),
            (
                "def f(a):\n    if a:\n        t = 1\n    else:\n        t = 2\n    return t¦\n",
                "it is assigned 2 times (lines 3, 5)",
            ),
            (
                "def f(a):\n    t¦, u = a\n    return t\n",
                "it is assigned together with other names",
            ),
            (
                "def f(a):\n    [t¦] = a\n    return t\n",
                "it is assigned together with other names",
            ),
            (
                "def f(a):\n    u = t¦ = a\n    return t\n",
                "it is assigned in a chained assignment",
            ),
            (
                "def f(a):\n    t¦ = u = a\n    return t\n",
                "it is assigned in a chained assignment",
            ),
            (
                "def f(a):\n    t¦: int = a\n    return t\n",
                "it is declared with an annotation",
            ),
            (
                "def f(a):\n    t: int\n    t¦ = a\n    return t\n",
                "it is declared with an annotation",
            ),
            (
                "def f(xs):\n    for x¦ in xs:\n        print(x)\n",
                "it is a loop variable",
            ),
            (
                "def f(a):\n    with a as h¦:\n        return h\n",
                "it is bound by the `as` of a `with` or `except`",
            ),
            (
                "def f(a):\n    try:\n        pass\n    except E as e¦:\n        return e\n",
                "it is bound by the `as` of a `with` or `except`",
            ),
            (
                "def f(a):\n    import os\n    return o¦s\n",
                "it is an imported name",
            ),
            (
                "def f(a):\n    import numpy as np\n    return n¦p\n",
                "it is an imported name",
            ),
            (
                "def f(a):\n    if (n¦ := a):\n        return n\n",
                "it is assigned by `:=`",
            ),
            (
                "def f(a):\n    class Lo¦cal:\n        pass\n    return Local\n",
                "it is a class",
            ),
            (
                "def f(a):\n    t¦ = a; u = 1\n    return t\n",
                "its assignment shares its line with other code",
            ),
            (
                "def f(a):\n    if a: t¦ = a + 1\n    return 1\n",
                "its assignment shares its line with other code",
            ),
        ];
        for (source, reason) in cases {
            refused(source, reason);
        }
    }

    #[test]
    fn what_else_the_function_does_with_the_name() {
        refused_in(
            "    v¦ = a + 1\n    return lambda: v\n",
            "it is also used in a `lambda` (line 3), which reads the variable when it runs, not where it stands",
        );
        refused_in(
            "    v¦ = a + 1\n    def g():\n        return v\n    return g\n",
            "it is also used in a nested function (line 4)",
        );
        refused_in(
            "    v¦ = a + 1\n    class K:\n        size = v\n    return K\n",
            "it is also used in a class body (line 4)",
        );
        refused_in(
            "    v¦ = a + 1\n    return (x + v for x in xs)\n",
            "it is also used in a generator expression (line 3)",
        );
        refused_in(
            "    v¦ = a + 1\n    def g(x=v):\n        return x\n    return g\n",
            "a nested function",
        );
        refused_in(
            "    v¦ = a + 1\n    del v\n",
            "it is changed through `del v` (line 3)",
        );
        refused_in(
            "    v¦ = a.b\n    v.c = 1\n    return v\n",
            "it is changed through `v.c` (line 3)",
        );
        refused_in(
            "    v¦ = a.b\n    v[0] += 1\n    return v\n",
            "it is changed through `v[0]` (line 3)",
        );
        refused_in(
            "    v¦ = a.b\n    for v.i in xs:\n        pass\n    return v\n",
            "it is changed through `v.i`",
        );
        refused_in(
            "    v¦ = a + 1\n    print(locals())\n    return v\n",
            "the function calls `locals`, which sees its local names",
        );
        for seeing in ["vars()", "eval('v')", "exec('print(v)')", "dir()"] {
            refused_in(
                &format!("    v¦ = a + 1\n    {seeing}\n    return v\n"),
                "which sees its local names",
            );
        }
        refused_in(
            "    v¦ = a + 1\n    match a:\n        case 1:\n            return v\n",
            "the function has a `match` statement",
        );
        // In an f-string: what it cannot hold, and `=`.
        refused_in(
            "    v¦ = a[\"k\"]\n    return f\"{v}\"\n",
            "the read on line 3 is inside an f-string, and its value has a quote",
        );
        refused_in(
            "    v¦ = a + '\\n'\n    return f\"{v}\"\n",
            "inside an f-string",
        );
        refused_in(
            "    v¦ = a + 1\n    return f\"{v=}\"\n",
            "the read on line 3 is in an f-string with `=`",
        );
        // Syntax errors around it.
        refused(
            "def f(a):\n    v¦ = a + 1\n    return v +\n",
            "syntax errors",
        );
        // Errors elsewhere in the file do not matter.
        assert_eq!(
            inline("def f(a):\n    v¦ = a + 1\n    return v\n\n\nitem.\n").unwrap(),
            "def f(a):\n    return a + 1\n\n\nitem.\n"
        );
    }

    #[test]
    fn places_and_messages() {
        let find = |source: &str, refactoring| {
            let (text, range) = marked(source);
            crate::python_extract::find(&text, &range, refactoring)
        };
        const SOURCE: &str = "def f(a):\n    total = a * 2\n    return to¦tal + 1\n";
        let (found, message) = find(SOURCE, Refactoring::Inline);
        assert_eq!(message, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "total");
        assert_eq!(
            (found[0].range.start_line, found[0].range.start_character),
            (2, 11)
        );
        assert_eq!(found[0].actions.len(), 1);
        assert_eq!(found[0].actions[0].title, INLINE_VARIABLE);
        assert_eq!(found[0].actions[0].kind.as_deref(), Some("refactor.inline"));
        assert!(Refactoring::Inline.matches(&found[0].actions[0]));
        assert!(!Refactoring::Variable.matches(&found[0].actions[0]));
        assert!(!crate::actions::introduces_name(INLINE_VARIABLE));
        // Refactor This lists it on a variable's name, for a caret and for the selected name.
        let (found, _) = find(SOURCE, Refactoring::Any);
        let titles: Vec<&str> = found[0].actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, [INLINE_VARIABLE]);
        let (found, _) = find(
            "def f(a):\n    total = a * 2\n    return «total» + 1\n",
            Refactoring::Any,
        );
        let titles: Vec<&str> = found[0].actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                crate::python_extract::EXTRACT_VARIABLE,
                crate::python_extract::EXTRACT_METHOD,
                INLINE_VARIABLE
            ]
        );
        // The messages.
        let message = |source: &str| find(source, Refactoring::Inline).1.unwrap();
        assert_eq!(
            message("def f(a):\n    total = g(a)\n    return to¦tal + total\n"),
            "Cannot inline `total`: its value contains `g(a)`, which would then run 2 times instead of once (it is read on lines 3, 3)."
        );
        assert_eq!(
            message("def he¦lp(a):\n    return a\n"),
            "Cannot inline `help`: it is a function, and inlining a function is not supported; Inline supports a local variable of a function that is assigned once, by `name = expression`."
        );
        assert_eq!(
            message("def f(a):\n    return a ¦+ 1\n"),
            "There is no variable at the caret to inline."
        );
        // `plan` fails with the same text and gives the same edits.
        let (text, range) = marked("def he¦lp(a):\n    return a\n");
        assert_eq!(
            crate::python_extract::plan(&text, &range, INLINE_VARIABLE).unwrap_err(),
            message("def he¦lp(a):\n    return a\n")
        );
        let (text, range) = marked(SOURCE);
        let edits = crate::python_extract::plan(&text, &range, INLINE_VARIABLE).unwrap();
        assert_eq!(
            apply_edits(&text, &edits).unwrap(),
            "def f(a):\n    return a * 2 + 1\n"
        );
        assert!(crate::python_extract::is_own(
            std::path::Path::new("a.py"),
            INLINE_VARIABLE
        ));
        assert!(crate::python_extract::offers(
            std::path::Path::new("a.py"),
            Refactoring::Inline
        ));
        assert!(!crate::python_extract::offers(
            std::path::Path::new("a.ts"),
            Refactoring::Inline
        ));
    }
}
