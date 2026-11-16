//! IDECraft's own quick fixes in Python files where pyright offers none (`SPEC-0098` v9 behaviours
//! 17–19, OBS-0034 part 14, Q1–Q3): at an unused import, at a local variable that is never read
//! and at a name that is not defined in a function. Each comes from a finding of IDECraft's own
//! analysis of the file — not from the server's diagnostic — whose default level is "no
//! highlighting, only fix": pyright already marks these places, and a place IDECraft gets wrong
//! is then never shown as a problem. Each fix is one undo step on the editor's text.
//!
//! The analysis is conservative: where Python could bind or read a name in a way the syntax does
//! not show (`import *`, `globals()`, `locals()`, `exec`, `eval`), nothing is reported.

use std::collections::HashSet;
use std::path::Path;

use tree_sitter::Node;

use crate::inspections::{Finding, Fix, Level};
use crate::jvm::TextEdit;
use crate::python_extract::{
    bound_of, children, field, free_into, function_locals, import_names, named, param_names, word,
};

/// Ids and display names of the rules of this module.
pub const RULES: &[(&str, &str)] = &[
    ("python.unused-import", "Unused import"),
    ("python.unused-variable", "Local variable is never read"),
    ("python.unresolved-name", "Name is not defined"),
];

/// The title of the fix that runs Optimize Imports.
pub const OPTIMIZE_IMPORTS: &str = "Optimize imports";

/// Python's built-in names (3.12 and 3.13) and the names every module has.
const BUILTINS: &[&str] = &[
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "BaseException",
    "BaseExceptionGroup",
    "BlockingIOError",
    "BrokenPipeError",
    "BufferError",
    "BytesWarning",
    "ChildProcessError",
    "ConnectionAbortedError",
    "ConnectionError",
    "ConnectionRefusedError",
    "ConnectionResetError",
    "DeprecationWarning",
    "EOFError",
    "Ellipsis",
    "EncodingWarning",
    "EnvironmentError",
    "Exception",
    "ExceptionGroup",
    "False",
    "FileExistsError",
    "FileNotFoundError",
    "FloatingPointError",
    "FutureWarning",
    "GeneratorExit",
    "IOError",
    "ImportError",
    "ImportWarning",
    "IndentationError",
    "IndexError",
    "InterruptedError",
    "IsADirectoryError",
    "KeyError",
    "KeyboardInterrupt",
    "LookupError",
    "MemoryError",
    "ModuleNotFoundError",
    "NameError",
    "None",
    "NotADirectoryError",
    "NotImplemented",
    "NotImplementedError",
    "OSError",
    "OverflowError",
    "PendingDeprecationWarning",
    "PermissionError",
    "ProcessLookupError",
    "PythonFinalizationError",
    "RecursionError",
    "ReferenceError",
    "ResourceWarning",
    "RuntimeError",
    "RuntimeWarning",
    "StopAsyncIteration",
    "StopIteration",
    "SyntaxError",
    "SyntaxWarning",
    "SystemError",
    "SystemExit",
    "TabError",
    "TimeoutError",
    "True",
    "TypeError",
    "UnboundLocalError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "UnicodeError",
    "UnicodeTranslateError",
    "UnicodeWarning",
    "UserWarning",
    "ValueError",
    "Warning",
    "ZeroDivisionError",
    "__build_class__",
    "__debug__",
    "__doc__",
    "__import__",
    "__loader__",
    "__name__",
    "__package__",
    "__spec__",
    "__file__",
    "__builtins__",
    "__path__",
    "__cached__",
    "__annotations__",
    "__dict__",
    "__class__",
    "__module__",
    "__qualname__",
    "abs",
    "aiter",
    "all",
    "anext",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "copyright",
    "credits",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "exit",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "license",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "quit",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
];

/// Text that lets Python bind or read names the syntax does not show.
fn dynamic(text: &str) -> bool {
    [
        "import *",
        "globals(",
        "locals(",
        "vars(",
        "exec(",
        "eval(",
        "__getattr__",
        "__builtins__",
    ]
    .iter()
    .any(|w| text.contains(w))
}

fn finding(id: &'static str, n: Node, message: String, fix: Option<Fix>) -> Finding {
    Finding {
        id,
        start: n.start_byte(),
        end: n.end_byte(),
        level: Level::Info,
        message,
        fix,
    }
}

fn edit_fix(title: String, edits: Vec<TextEdit>) -> Fix {
    Fix {
        title,
        edits,
        learn: None,
        keep: None,
        action: None,
    }
}

/// The findings of this module's rules in the Python file `path` with the tree `root` of `text`.
pub fn analyze(path: &Path, root: Node, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if dynamic(text) {
        return out;
    }
    unused_imports(path, root, text, &mut out);
    let module = bound_of(root, text);
    let module_names: HashSet<String> = module.names.iter().map(|(n, _)| n.clone()).collect();
    functions(root, text, &module_names, &mut Vec::new(), &mut out);
    out
}

// ---------------------------------------------------------------------------------------------
// Q3: unused imports (behaviour 17)
// ---------------------------------------------------------------------------------------------

/// Whether `name` occurs as a whole word in a string or a `# type:` comment of `root`: a name in
/// `__all__`, a string annotation or a type comment counts as used.
fn named_in_text(root: Node, text: &str, name: &str) -> bool {
    let whole = |s: &str| {
        s.match_indices(name).any(|(i, _)| {
            let before = s[..i].chars().next_back();
            let after = s[i + name.len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            !word(before) && !word(after)
        })
    };
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "string" => {
                if whole(word(n, text)) {
                    return true;
                }
            }
            "comment" => {
                let c = word(n, text);
                if c.contains("type:") && whole(c) {
                    return true;
                }
            }
            _ => stack.extend(children(n)),
        }
    }
    false
}

fn unused_imports(path: &Path, root: Node, text: &str, out: &mut Vec<Finding>) {
    // A package's `__init__.py` offers what it imports (as Optimize Imports without ruff).
    if crate::python_tools::is_package_init(path) {
        return;
    }
    let mut reads = Vec::new();
    free_into(root, text, &mut reads);
    let read: HashSet<String> = reads.into_iter().map(|(n, _)| n).collect();
    for statement in named(root) {
        if !matches!(
            statement.kind(),
            "import_statement" | "import_from_statement"
        ) {
            continue;
        }
        for bound in import_names(statement) {
            let name = word(bound, text);
            if read.contains(name) || named_in_text(root, text, name) {
                continue;
            }
            let at = position(text, bound.start_byte());
            let cleaned = crate::python_tools::remove_unused_imports(text, &[at]);
            let remove = (!cleaned.removed.is_empty())
                .then(|| crate::js_tools::minimal_edit(text, &cleaned.text))
                .flatten()
                .map(|e| edit_fix("Remove unused import".into(), vec![e]));
            let message = format!("Unused import '{name}'");
            let optimize = Fix {
                title: OPTIMIZE_IMPORTS.into(),
                edits: Vec::new(),
                learn: None,
                keep: None,
                action: Some("OptimizeImports"),
            };
            match remove {
                Some(fix) => {
                    out.push(finding(
                        "python.unused-import",
                        bound,
                        message.clone(),
                        Some(fix),
                    ));
                    out.push(finding(
                        "python.unused-import",
                        bound,
                        message,
                        Some(optimize),
                    ));
                }
                None => out.push(finding(
                    "python.unused-import",
                    bound,
                    message,
                    Some(optimize),
                )),
            }
        }
    }
}

/// 0-based line and character (Unicode scalar values) of byte `at`.
fn position(text: &str, at: usize) -> (u64, u64) {
    let before = &text[..at];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (
        before.matches('\n').count() as u64,
        before[line_start..].chars().count() as u64,
    )
}

// ---------------------------------------------------------------------------------------------
// Functions: Q2 unused locals (behaviour 18), Q1 names not defined (behaviour 19)
// ---------------------------------------------------------------------------------------------

/// What an enclosing scope offers to a nested function.
struct Enclosing {
    names: HashSet<String>,
}

fn functions(
    n: Node,
    text: &str,
    module: &HashSet<String>,
    enclosing: &mut Vec<Enclosing>,
    out: &mut Vec<Finding>,
) {
    for c in children(n) {
        match c.kind() {
            "function_definition" => {
                function(c, text, module, enclosing, out);
                let mut names = function_locals(c, text);
                if let Some(p) = field(c, "parameters") {
                    names.extend(param_names(p, text));
                }
                enclosing.push(Enclosing { names });
                if let Some(body) = field(c, "body") {
                    functions(body, text, module, enclosing, out);
                }
                enclosing.pop();
            }
            // A class body's names are not seen from its methods; they are left alone as if they
            // were (no report rather than a wrong one).
            "class_definition" => {
                let mut names = HashSet::new();
                if let Some(body) = field(c, "body") {
                    names.extend(bound_of(body, text).names.into_iter().map(|(n, _)| n));
                }
                enclosing.push(Enclosing { names });
                if let Some(body) = field(c, "body") {
                    functions(body, text, module, enclosing, out);
                }
                enclosing.pop();
            }
            _ => functions(c, text, module, enclosing, out),
        }
    }
}

fn function(
    f: Node,
    text: &str,
    module: &HashSet<String>,
    enclosing: &[Enclosing],
    out: &mut Vec<Finding>,
) {
    let Some(body) = field(f, "body") else {
        return;
    };
    let locals = function_locals(f, text);
    let params: HashSet<String> = field(f, "parameters")
        .map(|p| param_names(p, text).into_iter().collect())
        .unwrap_or_default();
    let bound = bound_of(body, text);
    // Every name the body mentions, nested functions' own names left out.
    let mut mentioned = Vec::new();
    free_into(body, text, &mut mentioned);
    // A plain binding is no read; `x += 1` reads `x`.
    let mut augmented = HashSet::new();
    collect_augmented(body, &mut augmented);
    let binding: HashSet<usize> = bound
        .names
        .iter()
        .map(|(_, at)| *at)
        .filter(|at| !augmented.contains(at))
        .collect();
    let read: HashSet<&str> = mentioned
        .iter()
        .filter(|(_, at)| !binding.contains(at))
        .map(|(n, _)| n.as_str())
        .collect();
    unused_locals(body, text, &params, &bound.outer, &read, out);
    // Q1: names the body reads that no scope binds.
    let mut reported = HashSet::new();
    for (name, at) in &mentioned {
        if locals.contains(name)
            || params.contains(name)
            || bound.outer.contains(name)
            || module.contains(name)
            || BUILTINS.contains(&name.as_str())
            || enclosing.iter().any(|e| e.names.contains(name))
            || !reported.insert(*at)
        {
            continue;
        }
        let Some(n) = f
            .descendant_for_byte_range(*at, *at + name.len())
            .filter(|n| n.kind() == "identifier")
        else {
            continue;
        };
        let fix = create_parameter(f, text, name);
        out.push(finding(
            "python.unresolved-name",
            n,
            format!("Name '{name}' is not defined"),
            fix,
        ));
    }
}

fn collect_augmented(n: Node, out: &mut HashSet<usize>) {
    if n.kind() == "augmented_assignment"
        && let Some(l) = field(n, "left")
    {
        out.insert(l.start_byte());
    }
    for c in children(n) {
        if !matches!(
            c.kind(),
            "function_definition" | "class_definition" | "lambda"
        ) {
            collect_augmented(c, out);
        }
    }
}

/// `Create parameter 'name'` (behaviour 19): appended to a parameter list of plain names only —
/// with a default value, `*`, `/`, `*args` or `**kwargs` a new last parameter would take an
/// argument the callers pass to another one. Callers are not changed.
fn create_parameter(f: Node, text: &str, name: &str) -> Option<Fix> {
    let list = field(f, "parameters")?;
    let params = named(list);
    let plain = params.iter().all(|p| {
        matches!(p.kind(), "identifier" | "typed_parameter") && !word(*p, text).starts_with('*')
    });
    if !plain {
        return None;
    }
    let edit = match params.last() {
        Some(last) => TextEdit {
            range: last.end_byte()..last.end_byte(),
            text: format!(", {name}"),
        },
        None => {
            let open = list.start_byte() + 1;
            TextEdit {
                range: open..open,
                text: name.to_owned(),
            }
        }
    };
    Some(edit_fix(format!("Create parameter '{name}'"), vec![edit]))
}

/// Q2: `name = expression` of a function whose name the function never reads (behaviour 18).
fn unused_locals(
    body: Node,
    text: &str,
    params: &HashSet<String>,
    outer: &HashSet<String>,
    read: &HashSet<&str>,
    out: &mut Vec<Finding>,
) {
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        for c in children(n) {
            match c.kind() {
                "function_definition" | "class_definition" | "lambda" => {}
                "expression_statement" => {
                    if let Some(f) = unused_local(c, text, params, outer, read) {
                        out.extend(f);
                    }
                }
                _ => stack.push(c),
            }
        }
    }
}

fn unused_local(
    statement: Node,
    text: &str,
    params: &HashSet<String>,
    outer: &HashSet<String>,
    read: &HashSet<&str>,
) -> Option<Vec<Finding>> {
    let inner = named(statement);
    let [assignment] = inner.as_slice() else {
        return None;
    };
    if assignment.kind() != "assignment" {
        return None;
    }
    let left = field(*assignment, "left")?;
    let right = field(*assignment, "right")?;
    let name = word(left, text);
    if left.kind() != "identifier"
        || right.kind() == "assignment"
        || name.starts_with('_')
        || params.contains(name)
        || outer.contains(name)
        || read.contains(name)
    {
        return None;
    }
    let message = format!("Local variable '{name}' is never read");
    // `name = f()` → `f()`: the expression keeps running.
    let target = edit_fix(
        "Remove assignment target".into(),
        vec![TextEdit {
            range: left.start_byte()..right.start_byte(),
            text: String::new(),
        }],
    );
    let mut found = vec![finding(
        "python.unused-variable",
        left,
        message.clone(),
        Some(target),
    )];
    if let Some(edits) = remove_statement(statement, right, text) {
        found.push(finding(
            "python.unused-variable",
            left,
            message,
            Some(edit_fix("Remove statement".into(), edits)),
        ));
    }
    Some(found)
}

/// Whether evaluating `n` cannot do anything: literals, containers of them and `lambda`s. A name,
/// an attribute, a subscript or an operator can run code or raise, so the statement stays.
fn inert(n: Node) -> bool {
    match n.kind() {
        "integer" | "float" | "true" | "false" | "none" | "ellipsis" | "lambda" => true,
        "string" | "concatenated_string" => !children(n)
            .iter()
            .any(|c| c.kind() == "interpolation" || !inert_string_part(*c)),
        "tuple"
        | "list"
        | "set"
        | "dictionary"
        | "pair"
        | "parenthesized_expression"
        | "expression_list" => named(n).into_iter().all(inert),
        "unary_operator" => named(n)
            .into_iter()
            .all(|c| matches!(c.kind(), "integer" | "float")),
        _ => false,
    }
}

fn inert_string_part(n: Node) -> bool {
    match n.kind() {
        "interpolation" => false,
        "string" => !children(n).iter().any(|c| c.kind() == "interpolation"),
        _ => true,
    }
}

/// The edits that remove `statement` with its line, or `pass` when it is the only statement of
/// its block; `None` unless the value is inert and the statement stands alone on its lines.
fn remove_statement(statement: Node, value: Node, text: &str) -> Option<Vec<TextEdit>> {
    if !inert(value) {
        return None;
    }
    let line_start = text[..statement.start_byte()]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let line_end = text[statement.end_byte()..]
        .find('\n')
        .map_or(text.len(), |i| statement.end_byte() + i);
    let alone = text[line_start..statement.start_byte()].trim().is_empty()
        && text[statement.end_byte()..line_end].trim().is_empty();
    if !alone {
        return None;
    }
    let block = statement.parent()?;
    let only = named(block)
        .iter()
        .filter(|s| s.kind() != "comment")
        .count()
        == 1;
    if only {
        return Some(vec![TextEdit {
            range: statement.byte_range(),
            text: "pass".into(),
        }]);
    }
    let end = (line_end + 1).min(text.len());
    Some(vec![TextEdit {
        range: line_start..end,
        text: String::new(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LanguageId;
    use crate::syntax::SyntaxDocument;

    fn found(path: &str, text: &str) -> Vec<Finding> {
        let doc = SyntaxDocument::new(LanguageId::Python, false, text).unwrap();
        analyze(Path::new(path), doc.tree().root_node(), text)
    }

    /// The titles of the fixes at the first occurrence of `needle` and the text after `title`.
    fn fixes(text: &str, needle: &str) -> Vec<(String, Option<String>)> {
        let at = text.find(needle).unwrap_or_else(|| panic!("{needle}"));
        found("probe.py", text)
            .into_iter()
            .filter(|f| f.start <= at && at < f.end)
            .filter_map(|f| f.fix)
            .map(|fix| {
                let after = fix
                    .action
                    .is_none()
                    .then(|| crate::jvm::apply(text, &fix.edits));
                (fix.title, after)
            })
            .collect()
    }

    /// OBS-0034 item 61 (Q3): `order_total` imported and never used.
    #[test]
    fn unused_imports_are_removed_or_optimized() {
        let text = "from app.prices import order_total, unit_price\nimport os\nimport sys as system\n\n\ndef f():\n    return unit_price(1)\n";
        assert_eq!(
            fixes(text, "order_total"),
            [
                (
                    "Remove unused import".into(),
                    Some("from app.prices import unit_price\nimport os\nimport sys as system\n\n\ndef f():\n    return unit_price(1)\n".into())
                ),
                (OPTIMIZE_IMPORTS.into(), None),
            ]
        );
        let os_gone = fixes(text, "os\n");
        assert_eq!(
            os_gone[0].1.as_deref(),
            Some(
                "from app.prices import order_total, unit_price\nimport sys as system\n\n\ndef f():\n    return unit_price(1)\n"
            )
        );
        assert_eq!(fixes(text, "system\n").len(), 2);
        // Used names are not reported: read anywhere, in `__all__`, in a string annotation or in a
        // type comment; nor `__future__`, nor anything in a package's `__init__.py`.
        let used = "from __future__ import annotations\nimport os\nimport json\nimport re\nimport typing\n__all__ = [\"json\"]\n\n\ndef f(x: \"re.Pattern\"):\n    y = []  # type: typing.List\n    return os.sep, y\n";
        assert!(
            found("a.py", used)
                .iter()
                .all(|f| f.id != "python.unused-import"),
            "{:?}",
            found("a.py", used)
        );
        assert!(found("__init__.py", "import os\n").is_empty());
        // A statement in a block or with a comment keeps its import; Optimize imports remains.
        let kept =
            "try:\n    import yaml\nexcept ImportError:\n    pass\nimport csv  # for later\n";
        assert_eq!(fixes(kept, "csv"), [(OPTIMIZE_IMPORTS.into(), None)]);
        // Where names come and go unseen, nothing is reported.
        assert!(found("a.py", "from m import *\nimport os\n").is_empty());
    }

    /// OBS-0034 item 60 (Q2): `unused = 3` in a function.
    #[test]
    fn unused_locals_lose_their_target_or_their_statement() {
        let text = "def f(a):\n    unused = 3\n    kept = a * 2\n    return kept\n";
        assert_eq!(
            fixes(text, "unused"),
            [
                (
                    "Remove assignment target".into(),
                    Some("def f(a):\n    3\n    kept = a * 2\n    return kept\n".into())
                ),
                (
                    "Remove statement".into(),
                    Some("def f(a):\n    kept = a * 2\n    return kept\n".into())
                ),
            ]
        );
        // A value that can do something keeps running: only the target goes.
        let call = "def f(a):\n    result = save(a)\n    other = a.total\n    return 1\n";
        assert_eq!(
            fixes(call, "result"),
            [(
                "Remove assignment target".into(),
                Some("def f(a):\n    save(a)\n    other = a.total\n    return 1\n".into())
            )]
        );
        assert_eq!(fixes(call, "other").len(), 1);
        // The only statement of a block becomes `pass`.
        let only = "def f():\n    if True:\n        x = [1, (2, 'a'), {3: None}]\n    return 0\n";
        assert_eq!(
            fixes(only, "x =")[1].1.as_deref(),
            Some("def f():\n    if True:\n        pass\n    return 0\n")
        );
        // Not reported: read later, also by `+=`, in a nested function or in a loop; `_names`;
        // parameters; `global`; module level; tuples and chains; `locals()`.
        for source in [
            "def f():\n    x = 1\n    return x\n",
            "def f():\n    x = 1\n    x += 1\n",
            "def f():\n    x = 1\n    def g():\n        return x\n    return g\n",
            "def f(items):\n    for i in items:\n        if i:\n            print(last)\n        last = i\n",
            "def f():\n    _x = 1\n",
            "def f(a):\n    a = 1\n",
            "def f():\n    global g\n    g = 1\n",
            "x = 1\n",
            "def f():\n    a, b = 1, 2\n    c = d = 3\n",
            "def f():\n    x = 1\n    return locals()\n",
        ] {
            assert!(
                found("a.py", source)
                    .iter()
                    .all(|f| f.id != "python.unused-variable"),
                "{source:?}: {:?}",
                found("a.py", source)
            );
        }
    }

    /// OBS-0034 item 58 (Q1): `undefined_name` read in a function.
    #[test]
    fn names_not_defined_can_become_parameters() {
        let text = "def total(price, qty):\n    return price * qty * undefined_name\n";
        assert_eq!(
            fixes(text, "undefined_name"),
            [(
                "Create parameter 'undefined_name'".into(),
                Some("def total(price, qty, undefined_name):\n    return price * qty * undefined_name\n".into())
            )]
        );
        let none = "def f():\n    return rate\n";
        assert_eq!(
            fixes(none, "rate\n")[0].1.as_deref(),
            Some("def f(rate):\n    return rate\n")
        );
        let method = "class A:\n    def m(self, x: int):\n        return x + scale\n";
        assert_eq!(
            fixes(method, "scale\n")[0].1.as_deref(),
            Some("class A:\n    def m(self, x: int, scale):\n        return x + scale\n")
        );
        // A default, `*args`, `**kwargs`, `*` or `/`: the place is found, but no parameter is
        // created, since callers' arguments would move.
        for source in [
            "def f(a=1):\n    return b\n",
            "def f(*args):\n    return b\n",
            "def f(**kw):\n    return b\n",
            "def f(a, *, k):\n    return b\n",
            "def f(a, /):\n    return b\n",
        ] {
            let f = found("a.py", source);
            assert_eq!(f.len(), 1, "{source:?}: {f:?}");
            assert!(f[0].fix.is_none(), "{source:?}");
        }
        // Defined somewhere: locals, enclosing functions, the module (also later), builtins,
        // imports, class names, comprehension and `except` names, `nonlocal`, module dunders.
        let fine = "import os\nfrom m import helper\n\n\ndef outer(a):\n    b = 1\n    def inner():\n        nonlocal b\n        b = 2\n        return a + b + LATER + len([i for i in range(3)])\n    try:\n        pass\n    except ValueError as e:\n        print(e, __name__, __file__, os, helper, Shape)\n    return inner\n\n\nclass Shape:\n    pass\n\n\nLATER = 1\n";
        assert!(
            found("a.py", fine)
                .iter()
                .all(|f| f.id != "python.unresolved-name"),
            "{:?}",
            found("a.py", fine)
        );
        // A class attribute read in a method is left alone (no report rather than a wrong one).
        let class_attr = "class A:\n    size = 1\n\n    def m(self):\n        return size\n";
        assert!(found("a.py", class_attr).is_empty());
        // Where names come and go unseen, nothing is reported.
        assert!(found("a.py", "def f():\n    exec('x = 1')\n    return x\n").is_empty());
    }

    /// Every finding is "no highlighting, only fix" by default.
    #[test]
    fn findings_carry_only_fixes() {
        let text = "import os\n\n\ndef f():\n    unused = 3\n    return missing\n";
        let all = found("a.py", text);
        assert!(all.len() >= 4, "{all:?}");
        assert!(all.iter().all(|f| f.level == Level::Info));
    }
}
