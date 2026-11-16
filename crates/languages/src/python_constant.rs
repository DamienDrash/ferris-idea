//! Extract Constant for Python, computed from the syntax tree (`SPEC-0330` v4 behaviours 22 to
//! 25): the Python language servers offer no such refactoring, so `IntroduceConstant` falls back
//! to this own action. The constant's line is evaluated once, when the module is loaded, while
//! the selection may run later and many times; so only an expression whose value cannot depend on
//! when it is evaluated is accepted, and everything else is refused with its reason.

use std::collections::HashSet;
use std::ops::Range as Bytes;

use tree_sitter::Node;

use crate::insight::Range;
use crate::python_extract::{
    BUILTIN_TYPES, Bound, COMPREHENSIONS, all_identifiers, bound_of, children,
    comprehension_targets, edit, eol, expression_at, field, function_locals, in_place, is_field,
    line_start, named, param_names, parse, selection, snake, standalone, starts_its_line,
    suggested, syntax_check, top_level_of, unique, value_place_for, word,
};
use crate::rename::Edit;

/// Title of the own Extract Constant action.
pub const EXTRACT_CONSTANT: &str = "Extract constant";

/// The built-in functions a constant may call (behaviour 23): without side effects and with the
/// same result at every call for arguments that are constants themselves, and with a result that
/// cannot be changed in place.
const PURE_CALLS: &[&str] = &[
    "abs",
    "bin",
    "bool",
    "bytes",
    "chr",
    "complex",
    "divmod",
    "float",
    "frozenset",
    "hex",
    "int",
    "len",
    "max",
    "min",
    "oct",
    "ord",
    "pow",
    "range",
    "round",
    "str",
    "sum",
    "tuple",
];

/// What a name of the module is bound by.
#[derive(PartialEq)]
enum Kind {
    /// A plain assignment of a constant expression.
    Value,
    /// An import: a module or a name of one.
    Import,
    /// A `class` statement.
    Class,
    /// A `def` statement.
    Function,
    /// A built-in type or one of [`PURE_CALLS`], not bound in the file.
    Builtin,
}

/// What the module binds, for the names a constant reads.
struct Module<'a, 't> {
    root: Node<'t>,
    text: &'a str,
    bound: Bound,
    /// Names of `global` statements anywhere in the file.
    globals: HashSet<String>,
    /// Names deleted with `del` anywhere in the file.
    deleted: HashSet<String>,
    /// Whether the file has a `from … import *`.
    wildcard: bool,
}

impl<'a, 't> Module<'a, 't> {
    fn new(root: Node<'t>, text: &'a str) -> Self {
        let mut m = Module {
            root,
            text,
            bound: bound_of(root, text),
            globals: HashSet::new(),
            deleted: HashSet::new(),
            wildcard: false,
        };
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "global_statement" => {
                    m.globals
                        .extend(named(n).into_iter().map(|c| word(c, text).to_owned()));
                }
                "delete_statement" => {
                    let mut names = HashSet::new();
                    all_identifiers(n, text, false, &mut names);
                    m.deleted.extend(names);
                }
                "wildcard_import" => m.wildcard = true,
                _ => {}
            }
            stack.extend(children(n));
        }
        m
    }

    /// What binds the name `id`, read by code that runs after byte `before` of the module's top
    /// level; or why its value may differ from one evaluation to the next.
    fn name(&self, id: Node<'t>, before: usize) -> Result<Kind, String> {
        let name = word(id, self.text);
        let bindings: Vec<usize> = self
            .bound
            .names
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, at)| *at)
            .collect();
        if self.globals.contains(name) {
            return Err(format!("a function assigns `{name}` with `global`"));
        }
        if self.deleted.contains(name) {
            return Err(format!("`{name}` is deleted with `del`"));
        }
        let at = match bindings.as_slice() {
            [] if self.wildcard => {
                return Err(format!(
                    "`{name}` is not bound in the file, and `from … import *` may bind it"
                ));
            }
            [] if PURE_CALLS.contains(&name) || BUILTIN_TYPES.contains(&name) => {
                return Ok(Kind::Builtin);
            }
            [] => {
                return Err(format!(
                    "`{name}` is not bound at the top level of the module"
                ));
            }
            [at] => *at,
            _ => {
                return Err(format!(
                    "`{name}` is bound {} times in the module",
                    bindings.len()
                ));
            }
        };
        const PLAIN: &str = "is not bound by a plain assignment, an import, `def` or `class` \
                             directly at the top level of the module";
        let binding = self
            .root
            .descendant_for_byte_range(at, at + name.len())
            .filter(|b| b.kind() == "identifier")
            .ok_or_else(|| format!("`{name}` {PLAIN}"))?;
        let top = top_level_of(binding);
        if top.end_byte() > before {
            return Err(format!(
                "`{name}` is not bound above the place of the constant"
            ));
        }
        let parent = binding
            .parent()
            .ok_or_else(|| format!("`{name}` {PLAIN}"))?;
        let directly = |n: Node| n.id() == top.id();
        match parent.kind() {
            "assignment"
                if is_field(parent, "left", binding)
                    && parent.parent().is_some_and(|s| {
                        s.kind() == "expression_statement" && directly(s) && named(s).len() == 1
                    }) =>
            {
                let right = field(parent, "right").ok_or_else(|| format!("`{name}` {PLAIN}"))?;
                self.constant(right, top.start_byte(), &HashSet::new())
                    .map_err(|why| format!("`{name}` is not a constant itself: {why}"))?;
                Ok(Kind::Value)
            }
            "function_definition" | "class_definition"
                if is_field(parent, "name", binding)
                    && (directly(parent)
                        || parent.parent().is_some_and(|d| {
                            d.kind() == "decorated_definition" && directly(d)
                        })) =>
            {
                Ok(if parent.kind() == "class_definition" {
                    Kind::Class
                } else {
                    Kind::Function
                })
            }
            "dotted_name" | "aliased_import" | "import_statement" | "import_from_statement"
                if matches!(top.kind(), "import_statement" | "import_from_statement") =>
            {
                Ok(Kind::Import)
            }
            _ => Err(format!("`{name}` {PLAIN}")),
        }
    }

    /// Whether the value of `n` is the same whenever it is evaluated after byte `before` of the
    /// module's top level (behaviour 23); `locals` are the names the scopes around `n` bind.
    fn constant(&self, n: Node<'t>, before: usize, locals: &HashSet<String>) -> Result<(), String> {
        let text = self.text;
        let shown = |n: Node| {
            let t = word(n, text);
            let line = t.lines().next().unwrap_or_default();
            if line.chars().count() > 40 || line.len() < t.len() {
                format!("{}…", line.chars().take(40).collect::<String>())
            } else {
                t.to_owned()
            }
        };
        let local = |id: Node| {
            let name = word(id, text);
            if locals.contains(name) {
                Err(format!(
                    "it reads `{name}`, which is a parameter or a local name of the function, \
                     class body, `lambda` or comprehension around it"
                ))
            } else {
                Ok(())
            }
        };
        match n.kind() {
            "integer" | "float" | "true" | "false" | "none" | "ellipsis" => Ok(()),
            "string" | "string_content" | "format_specifier" => {
                for c in children(n) {
                    match c.kind() {
                        "interpolation" => {
                            for part in named(c) {
                                if part.kind() != "type_conversion" {
                                    self.constant(part, before, locals)?;
                                }
                            }
                        }
                        "string_content" | "format_specifier" => {
                            self.constant(c, before, locals)?
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            "concatenated_string"
            | "parenthesized_expression"
            | "tuple"
            | "expression_list"
            | "unary_operator"
            | "not_operator"
            | "binary_operator"
            | "boolean_operator"
            | "comparison_operator"
            | "conditional_expression" => {
                for c in named(n) {
                    self.constant(c, before, locals)?;
                }
                Ok(())
            }
            "identifier" => {
                local(n)?;
                // An imported name, a class and a function are taken to stay what they are.
                self.name(n, before).map(|_| ())
            }
            "attribute" => {
                // A dotted name: its first part must be an import, a class or a built-in type.
                let mut root = n;
                while root.kind() == "attribute" {
                    root = field(root, "object").ok_or("an incomplete attribute")?;
                }
                if root.kind() != "identifier" {
                    return Err(format!(
                        "`{}` reads an attribute of a computed value",
                        shown(n)
                    ));
                }
                local(root)?;
                match self.name(root, before)? {
                    Kind::Import | Kind::Class | Kind::Builtin => Ok(()),
                    _ => Err(format!(
                        "`{}` reads an attribute of `{}`, which is neither an imported name nor \
                         a class; its attributes may change",
                        shown(n),
                        word(root, text)
                    )),
                }
            }
            "call" => {
                let callee = field(n, "function").ok_or("an incomplete call")?;
                let arguments = field(n, "arguments").ok_or("an incomplete call")?;
                let pure = callee.kind() == "identifier"
                    && PURE_CALLS.contains(&word(callee, text))
                    && !locals.contains(word(callee, text))
                    && arguments.kind() == "argument_list";
                if !pure || self.name(callee, before)? != Kind::Builtin {
                    return Err(format!(
                        "it calls `{}`, which may have side effects or give another result each \
                         time; only these built-in functions count as constant: {}",
                        shown(callee),
                        PURE_CALLS.join(", ")
                    ));
                }
                for a in named(arguments) {
                    match a.kind() {
                        "keyword_argument" => {
                            let v = field(a, "value").ok_or("an incomplete call")?;
                            self.constant(v, before, locals)?;
                        }
                        _ => self.constant(a, before, locals)?,
                    }
                }
                Ok(())
            }
            "list" | "dictionary" | "set" => Err(format!(
                "`{}` creates a new {} each time it is evaluated; a constant would share one \
                 object that can be changed",
                shown(n),
                match n.kind() {
                    "list" => "list",
                    "set" => "set",
                    _ => "dict",
                }
            )),
            k if COMPREHENSIONS.contains(&k) => Err(format!(
                "`{}` is a comprehension or generator expression, which creates a new object \
                 each time it is evaluated",
                shown(n)
            )),
            "lambda" => Err("it contains a `lambda`".into()),
            "subscript" => Err(format!(
                "`{}` reads from an object with `[…]`, whose content may change",
                shown(n)
            )),
            "await" => Err("it contains `await`".into()),
            _ => Err(format!(
                "`{}` is not a literal, a constant name of the module or an operator on them",
                shown(n)
            )),
        }
    }
}

/// The names bound by the scopes around `n`: parameters and locals of the functions, parameters
/// of the lambdas, variables of the comprehensions and the names of the class bodies.
fn enclosing_locals(n: Node, text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut c = n;
    while let Some(p) = c.parent() {
        match p.kind() {
            "function_definition" => out.extend(function_locals(p, text)),
            "lambda" => {
                if let Some(params) = field(p, "parameters") {
                    out.extend(param_names(params, text));
                }
            }
            "class_definition" => {
                if let Some(body) = field(p, "body") {
                    out.extend(bound_of(body, text).names.into_iter().map(|(n, _)| n));
                }
            }
            k if COMPREHENSIONS.contains(&k) => out.extend(comprehension_targets(p, text)),
            _ => {}
        }
        c = p;
    }
    out
}

/// The name of the new constant (behaviour 24), before it is made unique.
fn suggested_constant(n: Node, text: &str) -> Option<String> {
    let valid = |s: &String| {
        s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if let Some(s) = suggested(n, text)
        .map(|s| snake(s.trim_matches('_')).to_uppercase())
        .filter(valid)
    {
        return Some(s);
    }
    // A string literal without interpolation: its words.
    if n.kind() != "string" || named(n).iter().any(|c| c.kind() == "interpolation") {
        return None;
    }
    let content: String = named(n)
        .into_iter()
        .filter(|c| c.kind() == "string_content")
        .map(|c| word(c, text))
        .collect();
    let mut name = String::new();
    for c in content.chars() {
        if c.is_ascii_alphanumeric() {
            name.extend(c.to_uppercase());
        } else if !name.is_empty() && !name.ends_with('_') {
            name.push('_');
        }
    }
    let name = name.trim_end_matches('_').to_owned();
    Some(name).filter(|s| valid(s) && s.len() <= 30)
}

/// Whether the statement `s` is the docstring of its module, class or function.
fn is_docstring(s: Node) -> bool {
    s.kind() == "expression_statement"
        && named(s).len() == 1
        && named(s)[0].kind() == "string"
        && s.parent()
            .is_some_and(|b| named(b).first().is_some_and(|f| f.id() == s.id()))
}

/// The edits of Extract Constant for the bytes `sel` of `text`, or why it is refused.
pub(crate) fn constant_edits(
    root: Node,
    text: &str,
    sel: &Bytes<usize>,
) -> Result<Vec<Edit>, String> {
    let n = expression_at(root, sel)?;
    syntax_check(n)?;
    value_place_for(n, true)?;
    if n.parent().is_some_and(is_docstring) {
        return Err("it is a docstring".into());
    }
    let top = top_level_of(n);
    if !starts_its_line(text, top) {
        return Err("the top-level statement around it does not start its line".into());
    }
    let module = Module::new(root, text);
    module.constant(n, top.start_byte(), &enclosing_locals(n, text))?;

    let mut taken = HashSet::new();
    all_identifiers(root, text, true, &mut taken);
    let base = suggested_constant(n, text).unwrap_or_else(|| "CONSTANT".to_owned());
    let name = unique(&base, &taken);
    let value = standalone(n, text);
    let nl = eol(text);

    // Before the top-level statement, above the comment lines attached to it.
    let mut first = top;
    while let Some(prev) = first.prev_named_sibling() {
        if prev.kind() == "comment"
            && prev.end_position().row + 1 == first.start_position().row
            && starts_its_line(text, prev)
        {
            first = prev;
        } else {
            break;
        }
    }
    let at = line_start(text, first.start_byte());
    // Two empty lines before a top-level function or class.
    let gap = if matches!(
        top.kind(),
        "function_definition" | "class_definition" | "decorated_definition"
    ) {
        format!("{nl}{nl}")
    } else {
        String::new()
    };
    let line = format!("{name} = {value}{nl}{gap}");
    if at == n.start_byte() {
        // The expression starts the line: one edit, not two at the same place.
        return Ok(vec![edit(
            text,
            n.byte_range(),
            format!("{line}{}", in_place(n, text, &name)),
        )]);
    }
    Ok(vec![
        edit(text, at..at, line),
        edit(text, n.byte_range(), in_place(n, text, &name)),
    ])
}

/// The edits of Extract Constant for the selection `range` of `text`, or why it is refused.
pub fn extract_constant(text: &str, range: &Range) -> Result<Vec<Edit>, String> {
    let doc = parse(text)?;
    constant_edits(doc.tree().root_node(), text, &selection(text, range))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Refactoring;
    use crate::python_extract::tests::marked;
    use crate::rename::apply_edits;

    fn constant(source: &str) -> Result<String, String> {
        let (text, range) = marked(source);
        extract_constant(&text, &range).map(|e| apply_edits(&text, &e).unwrap())
    }

    #[track_caller]
    fn extracted(source: &str) -> String {
        constant(source).unwrap_or_else(|why| panic!("refused: {why}\n{source}"))
    }

    #[track_caller]
    fn refused(source: &str, reason: &str) {
        match constant(source) {
            Ok(text) => panic!("not refused ({reason}):\n{text}"),
            Err(why) => assert!(why.contains(reason), "{why:?} does not name {reason:?}"),
        }
    }

    #[test]
    fn the_line_goes_before_the_top_level_statement() {
        // From a function: before the `def`, with two empty lines between.
        assert_eq!(
            extracted("import time\n\n\ndef wait(n):\n    time.sleep(n + «60 * 60»)\n"),
            "import time\n\n\nCONSTANT = 60 * 60\n\n\ndef wait(n):\n    time.sleep(n + CONSTANT)\n"
        );
        // From a method, a nested function and a class body: still at module level.
        assert_eq!(
            extracted("class Clock:\n    def hours(self, s):\n        return s / «3600»\n"),
            "CONSTANT = 3600\n\n\nclass Clock:\n    def hours(self, s):\n        return s / CONSTANT\n"
        );
        assert_eq!(
            extracted(
                "x = 1\n\n\ndef f():\n    def g():\n        return «2 ** 10»\n    return g\n"
            ),
            "x = 1\n\n\nCONSTANT = 2 ** 10\n\n\ndef f():\n    def g():\n        return CONSTANT\n    return g\n"
        );
        assert_eq!(
            extracted("class Limits:\n    size = «4 * 1024»\n"),
            "CONSTANT = 4 * 1024\n\n\nclass Limits:\n    size = CONSTANT\n"
        );
        // Above the comment lines and decorators attached to the statement.
        assert_eq!(
            extracted(
                "import functools\n\n# Waits.\n# Twice.\n@functools.cache\ndef wait(n):\n    return n * «60»\n"
            ),
            "import functools\n\nCONSTANT = 60\n\n\n# Waits.\n# Twice.\n@functools.cache\ndef wait(n):\n    return n * CONSTANT\n"
        );
        // At module level: directly before the statement, also out of a loop or a branch.
        assert_eq!(
            extracted("a = 1\ntimeout = «60 * 60» + a\n"),
            "a = 1\nCONSTANT = 60 * 60\ntimeout = CONSTANT + a\n"
        );
        assert_eq!(
            extracted("for i in range(3):\n    if i:\n        print(i * «1.5»)\n"),
            "CONSTANT = 1.5\nfor i in range(3):\n    if i:\n        print(i * CONSTANT)\n"
        );
        // The expression starts the statement: one edit.
        let (text, range) = marked("«60 * 60» + f()\n");
        let edits = extract_constant(&text, &range).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(
            apply_edits(&text, &edits).unwrap(),
            "CONSTANT = 60 * 60\nCONSTANT + f()\n"
        );
        // A default value of a parameter.
        assert_eq!(
            extracted("def wait(timeout=«30 * 60»):\n    pass\n"),
            "CONSTANT = 30 * 60\n\n\ndef wait(timeout=CONSTANT):\n    pass\n"
        );
        // Over several lines: in parentheses. CRLF. A word stays apart.
        assert_eq!(
            extracted("def f():\n    return g(«1 +\n             2»)\n"),
            "CONSTANT = (1 +\n             2)\n\n\ndef f():\n    return g(CONSTANT)\n"
        );
        assert_eq!(
            extracted("def f():\r\n    return «60»\r\n"),
            "CONSTANT = 60\r\n\r\n\r\ndef f():\r\n    return CONSTANT\r\n"
        );
        assert_eq!(
            extracted("def f(a):\n    return a if«(1)»else 2\n"),
            "CONSTANT = (1)\n\n\ndef f(a):\n    return a if CONSTANT else 2\n"
        );
        // Only the selected occurrence is replaced.
        assert_eq!(
            extracted("def f():\n    return «60» + 60\n"),
            "CONSTANT = 60\n\n\ndef f():\n    return CONSTANT + 60\n"
        );
    }

    #[test]
    fn constant_names() {
        let name = |source: &str| {
            let (text, _) = marked(source);
            let after = extracted(source);
            crate::actions::introduced_name(&text, &after)
                .unwrap_or_else(|| panic!("no new name in\n{after}"))
                .name
        };
        // From the expression: the words of a string, the attribute, the called built-in.
        assert_eq!(
            name("def f():\n    return «\"application/json\"»\n"),
            "APPLICATION_JSON"
        );
        assert_eq!(
            name("def f():\n    return «'Hello, world!'»\n"),
            "HELLO_WORLD"
        );
        assert_eq!(
            name("import math\n\n\ndef f():\n    return «math.pi * 2»\n"),
            "PI"
        );
        assert_eq!(
            name("import os\n\n\ndef f():\n    return «os.path.sep»\n"),
            "SEP"
        );
        assert_eq!(
            name("from m import maxSize\n\n\ndef f():\n    return «maxSize.default»\n"),
            "DEFAULT"
        );
        assert_eq!(name("def f():\n    return «len(\"abc\")»\n"), "LEN");
        // Nothing to go by: `CONSTANT`.
        assert_eq!(name("def f():\n    return «60 * 60»\n"), "CONSTANT");
        assert_eq!(name("def f():\n    return «\"\"»\n"), "CONSTANT");
        assert_eq!(
            name("def f():\n    return «\"42 is the answer\"»\n"),
            "CONSTANT"
        );
        assert_eq!(
            name(
                "def f():\n    return «\"a very long sentence that would make a very long name\"»\n"
            ),
            "CONSTANT"
        );
        assert_eq!(
            name("B = 2\n\n\ndef f():\n    return «f\"{B}/api\"»\n"),
            "CONSTANT"
        );
        assert_eq!(
            name("LIMIT = 5\n\n\ndef f():\n    return «LIMIT * 2»\n"),
            "CONSTANT"
        );
        // Unique in the whole file, attributes included.
        assert_eq!(
            name("CONSTANT = 1\n\n\ndef f():\n    return «60»\n"),
            "CONSTANT1"
        );
        assert_eq!(
            name("def g(a):\n    return a.CONSTANT\n\n\ndef f(CONSTANT1):\n    return «60»\n"),
            "CONSTANT2"
        );
        assert_eq!(
            name("import math\n\n\ndef f(PI):\n    return «math.pi»\n"),
            "PI1"
        );
        // The introduced name is found for the name popup.
        let (text, range) =
            marked("import time\n\n\ndef wait(n):\n    time.sleep(n + «60 * 60»)\n");
        let after = apply_edits(&text, &extract_constant(&text, &range).unwrap()).unwrap();
        let name = crate::actions::introduced_name(&text, &after).unwrap();
        assert_eq!(
            (name.name.as_str(), name.line, name.character),
            ("CONSTANT", 3, 0)
        );
    }

    #[test]
    fn what_counts_as_constant() {
        let ok = |source: &str| {
            extracted(source);
        };
        // Literals and operators on them.
        for expression in [
            "1",
            "1.5",
            "1j",
            "\"a\"",
            "b\"a\"",
            "\"a\" \"b\"",
            "True",
            "None",
            "...",
            "-1",
            "not True",
            "1 + 2 * 3",
            "1 < 2 <= 3",
            "1 if True else 2",
            "True and False or None",
            "(1, 2, (3, \"a\"))",
            "\"a\" * 3",
            "\"%s-%d\" % (\"a\", 1)",
            "\"a\" in (\"a\", \"b\")",
            "f\"{1 + 2:>{3}}\"",
            "f\"{'a'!r}\"",
        ] {
            ok(&format!("def f():\n    return g(«{expression}»)\n"));
        }
        ok("def f():\n    x = «1, 2»\n    return x\n");
        // Names bound once at the top level above: by a constant assignment, an import, `def`,
        // `class`; also annotated, also through other constants.
        const HEAD: &str = "import math\nimport os.path\nfrom m import LIMIT, helper as aid\n\nBASE = 10\nSIZE: int = BASE * 2\nNAMES = (\"a\", \"b\")\nURL = f\"http://{BASE}\"\n\n\nclass Color:\n    RED = 1\n\n\ndef make():\n    pass\n\n\n";
        for expression in [
            "BASE",
            "BASE + 1",
            "SIZE * BASE",
            "NAMES + (\"c\",)",
            "URL + \"/x\"",
            "LIMIT * 2",
            "aid",
            "math.pi / 2",
            "os.path.sep",
            "Color.RED",
            "(Color, make, int)",
            "math",
            "int.__name__",
            "ValueError",
            "(KeyError, OSError)",
        ] {
            ok(&format!("{HEAD}def f():\n    return g(«{expression}»)\n"));
        }
        // The allowed built-in functions, on constants.
        for expression in [
            "len(NAMES)",
            "max(BASE, 3)",
            "min(1, 2)",
            "abs(-1)",
            "float(\"inf\")",
            "int(\"7\", base=8)",
            "str(BASE)",
            "round(1.5)",
            "range(BASE)",
            "tuple(range(3))",
            "frozenset((1, 2))",
            "sum((1, 2))",
            "pow(2, 8)",
            "divmod(7, 2)",
            "ord(\"a\")",
            "chr(97)",
            "bool(0)",
            "bytes(4)",
            "complex(1, 2)",
            "hex(255)",
            "oct(8)",
            "bin(2)",
        ] {
            ok(&format!("{HEAD}def f():\n    return g(«{expression}»)\n"));
        }
    }

    #[test]
    fn constant_refusals() {
        // Names of the scopes around the selection.
        const LOCAL: &str = "which is a parameter or a local name of the function, class body, `lambda` or comprehension around it";
        refused(
            "def f(n):\n    return «n * 60»\n",
            "it reads `n`, which is a parameter or a local name",
        );
        refused("def f(n):\n    return «n * 60»\n", LOCAL);
        refused("def f():\n    n = 2\n    return «n * 60»\n", "it reads `n`");
        refused(
            "class A:\n    def f(self):\n        return «self.size»\n",
            "it reads `self`",
        );
        refused(
            "N = 1\n\n\ndef f(xs):\n    return [«x * 2» for x in xs]\n",
            "it reads `x`",
        );
        refused(
            "N = 1\n\n\ndef f(xs):\n    return [«N * 2» for N in xs]\n",
            "it reads `N`",
        );
        refused("f = lambda n: «n + 1»\n", "it reads `n`");
        refused(
            "class A:\n    size = 3\n    area = «size * 2»\n",
            "it reads `size`",
        );
        refused(
            "N = 1\n\n\ndef f():\n    for N in range(3):\n        print(«N + 1»)\n",
            "it reads `N`",
        );
        refused("def f(len):\n    return «len(\"abc\")»\n", "it calls `len`");
        // A local name declared global is the module's, which a function then assigns.
        refused(
            "N = 1\n\n\ndef f():\n    global N\n    N = 2\n    return «N + 1»\n",
            "a function assigns `N` with `global`",
        );
        // Names of the module that are no constants.
        refused(
            "def f():\n    return «N + 1»\n\n\nN = 1\n",
            "`N` is not bound above the place of the constant",
        );
        refused(
            "N = 1\nN = 2\n\n\ndef f():\n    return «N + 1»\n",
            "`N` is bound 2 times in the module",
        );
        refused(
            "N = 1\nN += 1\n\n\ndef f():\n    return «N + 1»\n",
            "`N` is bound 2 times",
        );
        refused(
            "N = 1\n\n\ndef g():\n    global N\n    N = 2\n\n\ndef f():\n    return «N + 1»\n",
            "a function assigns `N` with `global`",
        );
        refused(
            "N = 1\ndel N\n\n\ndef f():\n    return «N + 1»\n",
            "`N` is deleted with `del`",
        );
        const PLAIN: &str = "is not bound by a plain assignment, an import, `def` or `class` directly at the top level of the module";
        refused(
            "if x:\n    N = 1\n\n\ndef f():\n    return «N + 1»\n",
            PLAIN,
        );
        refused(
            "try:\n    import json as N\nexcept ImportError:\n    pass\n\n\ndef f():\n    return «N.x»\n",
            PLAIN,
        );
        refused("N, M = 1, 2\n\n\ndef f():\n    return «N + 1»\n", PLAIN);
        refused("M = N = 1\n\n\ndef f():\n    return «N + 1»\n", PLAIN);
        refused(
            "for N in range(3):\n    pass\n\n\ndef f():\n    return «N + 1»\n",
            PLAIN,
        );
        refused(
            "with open(p) as N:\n    pass\n\n\ndef f():\n    return «N + 1»\n",
            PLAIN,
        );
        refused(
            "N = load()\n\n\ndef f():\n    return «N + 1»\n",
            "`N` is not a constant itself: it calls `load`",
        );
        refused(
            "N = []\n\n\ndef f():\n    return «N + N»\n",
            "`N` is not a constant itself: `[]` creates a new list",
        );
        refused(
            "M = load()\nN = M + 1\n\n\ndef f():\n    return «N + 1»\n",
            "`N` is not a constant itself: `M` is not a constant itself",
        );
        refused(
            "def f():\n    return «N + 1»\n",
            "`N` is not bound at the top level of the module",
        );
        refused(
            "def f():\n    return «print»\n",
            "`print` is not bound at the top level of the module",
        );
        refused(
            "from m import *\n\n\ndef f():\n    return «len(\"abc\")»\n",
            "`from … import *` may bind it",
        );
        refused(
            "from m import *\n\n\ndef f():\n    return «N + 1»\n",
            "`N` is not bound in the file, and `from … import *` may bind it",
        );
        // Calls.
        const CALLS: &str = "only these built-in functions count as constant: abs, bin, bool, bytes, chr, complex, divmod, float, frozenset, hex, int, len, max, min, oct, ord, pow, range, round, str, sum, tuple";
        refused(
            "def f():\n    return «load(1)»\n",
            "it calls `load`, which may have side effects or give another result each time",
        );
        refused("def f():\n    return «load(1)»\n", CALLS);
        refused(
            "import re\n\n\ndef f():\n    return «re.compile(\"a\")»\n",
            "it calls `re.compile`",
        );
        refused(
            "def f():\n    return «\"a\".upper()»\n",
            "it calls `\"a\".upper`",
        );
        refused(
            "def f():\n    return «sorted((2, 1))»\n",
            "it calls `sorted`",
        );
        refused("def f():\n    return «list((2, 1))»\n", "it calls `list`");
        refused("def f():\n    return «print(1)»\n", "it calls `print`");
        refused(
            "def len(x):\n    return 0\n\n\ndef f():\n    return «len(\"abc\")»\n",
            "it calls `len`",
        );
        refused(
            "len = max\n\n\ndef f():\n    return «len(\"abc\")»\n",
            "it calls `len`",
        );
        refused(
            "ARGS = (1, 2)\n\n\ndef f():\n    return «max(*ARGS)»\n",
            "`*ARGS` is not a literal",
        );
        refused(
            "def f():\n    return «sum(x for x in (1, 2))»\n",
            "it calls `sum`",
        );
        refused("def f():\n    return «max(1, load())»\n", "it calls `load`");
        // New objects each time, and state that may change.
        refused(
            "def f():\n    return «[1, 2]»\n",
            "`[1, 2]` creates a new list each time it is evaluated; a constant would share one object that can be changed",
        );
        refused("def f():\n    return «{1: 2}»\n", "creates a new dict");
        refused("def f():\n    return «{1, 2}»\n", "creates a new set");
        refused(
            "def f():\n    return «(1, [2])»\n",
            "`[2]` creates a new list",
        );
        refused(
            "def f():\n    return «[x for x in (1, 2)]»\n",
            "is a comprehension or generator expression",
        );
        refused(
            "def f():\n    return list(«x for x in (1, 2)»)\n",
            "is a comprehension or generator expression",
        );
        refused(
            "def f():\n    return «lambda: 1»\n",
            "it contains a `lambda`",
        );
        refused(
            "N = (1, 2)\n\n\ndef f():\n    return «N[0]»\n",
            "`N[0]` reads from an object with `[…]`, whose content may change",
        );
        refused(
            "N = 1\n\n\ndef f():\n    return «N.real»\n",
            "`N.real` reads an attribute of `N`, which is neither an imported name nor a class",
        );
        refused(
            "def make():\n    pass\n\n\ndef f():\n    return «make.count»\n",
            "reads an attribute of `make`",
        );
        refused(
            "def f():\n    return «(1).real»\n",
            "reads an attribute of a computed value",
        );
        refused(
            "async def f():\n    return «await 1»\n",
            "it contains `await`",
        );
        // Not an expression of its own, or in a place without a value of its own.
        refused(
            "def f():\n    return 1 + «2 + 3»\n",
            "the selection is not one complete expression",
        );
        refused("def f():\n    return «»1\n", "nothing is selected");
        refused(
            "def f():\n    «x = 1»\n",
            "the selection is not one complete expression",
        );
        refused("def f():\n    x = «(yield 1)»\n", "it contains `yield`");
        refused(
            "def f():\n    return «(n := 1)»\n",
            "it contains an assignment expression",
        );
        refused("def f():\n    x: «int» = 1\n", "it is in an annotation");
        refused("@«decorate»\ndef f():\n    pass\n", "it is in a decorator");
        refused("def f(a):\n    a.«b» = 1\n", "a name after a dot");
        refused("def f(a):\n    a[«0»] = 1\n", "the target of an assignment");
        refused(
            "class A(«object»):\n    pass\n",
            "it is in the header of a `def` or `class`",
        );
        refused(
            "def f():\n    «\"\"\"Doc.\"\"\"»\n    return 1\n",
            "it is a docstring",
        );
        refused("«\"\"\"Module doc.\"\"\"»\nx = 1\n", "it is a docstring");
        refused("class A:\n    «'Doc.'»\n", "it is a docstring");
        refused(
            "def f():\n    return «60» +\n",
            "the code around it has syntax errors",
        );
        refused(
            "x = 1; y = «60 * 60»\n",
            "the top-level statement around it does not start its line",
        );
        // A syntax error elsewhere does not matter.
        assert_eq!(
            extracted("y = «60 * 60»\n\n\nitem.\n"),
            "CONSTANT = 60 * 60\ny = CONSTANT\n\n\nitem.\n"
        );
    }

    #[test]
    fn places_and_messages() {
        let find = |source: &str, refactoring| {
            let (text, range) = marked(source);
            crate::python_extract::find(&text, &range, refactoring)
        };
        // A selection: the place and its action.
        let (found, message) = find(
            "def f(n):\n    return n + «60 * 60»\n",
            Refactoring::Constant,
        );
        assert_eq!(message, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "60 * 60");
        assert_eq!(found[0].actions[0].title, EXTRACT_CONSTANT);
        assert_eq!(
            found[0].actions[0].kind.as_deref(),
            Some("refactor.extract")
        );
        assert!(Refactoring::Constant.matches(&found[0].actions[0]));
        assert!(!Refactoring::Variable.matches(&found[0].actions[0]));
        assert!(crate::actions::introduces_name(EXTRACT_CONSTANT));
        // A bare caret: the constant expressions around it, inside out.
        let texts = |source: &str| -> Vec<String> {
            find(source, Refactoring::Constant)
                .0
                .into_iter()
                .map(|t| t.text)
                .collect()
        };
        assert_eq!(
            texts("def f(n):\n    return n * (6¦0 * 60 + 1)\n"),
            ["60", "60 * 60", "60 * 60 + 1", "(60 * 60 + 1)"]
        );
        // Refactor This lists it for a selection next to the others.
        let (found, _) = find("def f(n):\n    return n + «60 * 60»\n", Refactoring::Any);
        let titles: Vec<&str> = found[0].actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                crate::python_extract::EXTRACT_VARIABLE,
                crate::python_extract::EXTRACT_METHOD,
                EXTRACT_CONSTANT
            ]
        );
        // The messages.
        let message = |source: &str| find(source, Refactoring::Constant).1.unwrap();
        assert_eq!(
            message("def f(n):\n    return «n * 60»\n"),
            "Cannot extract `n * 60` into a constant: it reads `n`, which is a parameter or a local name of the function, class body, `lambda` or comprehension around it."
        );
        assert_eq!(
            message("def f(n):\n    return n¦ * g()\n"),
            "Cannot extract `n * g()` into a constant: it reads `n`, which is a parameter or a local name of the function, class body, `lambda` or comprehension around it."
        );
        assert_eq!(
            message("pa¦ss\n"),
            "There is no expression at the caret to extract."
        );
        // `plan` gives the same edits and fails with the same text.
        let (text, range) = marked("def f(n):\n    return «n * 60»\n");
        assert_eq!(
            crate::python_extract::plan(&text, &range, EXTRACT_CONSTANT).unwrap_err(),
            message("def f(n):\n    return «n * 60»\n")
        );
        let (text, range) = marked("x = «60 * 60»\n");
        let edits = crate::python_extract::plan(&text, &range, EXTRACT_CONSTANT).unwrap();
        assert_eq!(
            apply_edits(&text, &edits).unwrap(),
            "CONSTANT = 60 * 60\nx = CONSTANT\n"
        );
        let py = std::path::Path::new("a.py");
        assert!(crate::python_extract::is_own(py, EXTRACT_CONSTANT));
        assert!(crate::python_extract::offers(py, Refactoring::Constant));
        assert!(!crate::python_extract::offers(
            std::path::Path::new("a.rs"),
            Refactoring::Constant
        ));
    }
}
