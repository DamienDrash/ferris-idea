//! Java lambda simplifications (`SPEC-0098` v11 behaviours 22–25): an anonymous class that can be
//! a lambda, a block lambda with one statement, and a lambda that can be a method reference, each
//! with its quick fix, decided on the syntax tree alone. All three are weak warnings (IMP-0410);
//! the fix of an anonymous class gives the simplest form at once (IMP-0411).

use tree_sitter::Node;

use crate::inspections::{Finding, Fix, Level};
use crate::jvm::TextEdit;

/// Functional interfaces of the JDK known by name (behaviour 22).
const FUNCTIONAL: &[&str] = &[
    "Runnable",
    "Callable",
    "Comparator",
    "ThreadFactory",
    "FileFilter",
    "FilenameFilter",
    "PathMatcher",
    "UncaughtExceptionHandler",
    "ActionListener",
    "PropertyChangeListener",
    // java.util.function
    "BiConsumer",
    "BiFunction",
    "BinaryOperator",
    "BiPredicate",
    "BooleanSupplier",
    "Consumer",
    "DoubleBinaryOperator",
    "DoubleConsumer",
    "DoubleFunction",
    "DoublePredicate",
    "DoubleSupplier",
    "DoubleToIntFunction",
    "DoubleToLongFunction",
    "DoubleUnaryOperator",
    "Function",
    "IntBinaryOperator",
    "IntConsumer",
    "IntFunction",
    "IntPredicate",
    "IntSupplier",
    "IntToDoubleFunction",
    "IntToLongFunction",
    "IntUnaryOperator",
    "LongBinaryOperator",
    "LongConsumer",
    "LongFunction",
    "LongPredicate",
    "LongSupplier",
    "LongToDoubleFunction",
    "LongToIntFunction",
    "LongUnaryOperator",
    "ObjDoubleConsumer",
    "ObjIntConsumer",
    "ObjLongConsumer",
    "Predicate",
    "Supplier",
    "ToDoubleBiFunction",
    "ToDoubleFunction",
    "ToIntBiFunction",
    "ToIntFunction",
    "ToLongBiFunction",
    "ToLongFunction",
    "UnaryOperator",
];

/// Calls whose overloads take both a lambda that returns nothing and one that returns a value
/// (`Runnable` and `Callable`): a shorter lambda there could choose the other overload.
const OVERLOAD_SENSITIVE: &[&str] = &["submit", "schedule", "invokeAll", "invokeAny"];

/// Messages name the result up to this length (behaviour 23).
const SHORT: usize = 40;

/// A lambda parameter: its name and its declared type, when written.
struct Param {
    name: String,
    ty: Option<String>,
}

fn text_of<'a>(n: Node, text: &'a str) -> &'a str {
    &text[n.byte_range()]
}

fn named(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

fn is_comment(n: Node) -> bool {
    matches!(n.kind(), "line_comment" | "block_comment")
}

fn ancestors(n: Node) -> impl Iterator<Item = Node> {
    std::iter::successors(n.parent(), |p| p.parent())
}

fn fix(title: &str, range: std::ops::Range<usize>, with: String) -> Option<Fix> {
    Some(Fix {
        learn: None,
        keep: None,
        action: None,
        title: title.to_owned(),
        edits: vec![TextEdit { range, text: with }],
    })
}

/// The type name without type arguments: `Map.Entry<K, V>` → `Map.Entry`, `List<>` → `List`.
fn raw_type(ty: Node, text: &str) -> Option<String> {
    match ty.kind() {
        "type_identifier" | "scoped_type_identifier" => Some(text_of(ty, text).to_owned()),
        "generic_type" => raw_type(ty.named_child(0)?, text),
        _ => None,
    }
}

/// The last segment of a type name: `java.util.function.Supplier` → `Supplier`.
fn simple_name(raw: &str) -> &str {
    raw.rsplit('.').next().unwrap_or(raw)
}

/// The parameters of a lambda (`x`, `(a, b)`, `(String s)`); `None` for varargs.
fn lambda_params(lambda: Node, text: &str) -> Option<Vec<Param>> {
    let params = lambda.child_by_field_name("parameters")?;
    match params.kind() {
        "identifier" => Some(vec![Param {
            name: text_of(params, text).to_owned(),
            ty: None,
        }]),
        "inferred_parameters" => Some(
            named(params)
                .into_iter()
                .map(|p| Param {
                    name: text_of(p, text).to_owned(),
                    ty: None,
                })
                .collect(),
        ),
        "formal_parameters" => formal_params(params, text),
        _ => None,
    }
}

/// The parameters of a method's or lambda's parameter list; `None` for varargs or receiver
/// parameters.
fn formal_params(params: Node, text: &str) -> Option<Vec<Param>> {
    let mut out = Vec::new();
    for p in named(params) {
        if p.kind() != "formal_parameter" {
            return None;
        }
        let name = p.child_by_field_name("name")?;
        if name.kind() != "identifier" {
            return None;
        }
        let ty = p.child_by_field_name("type").and_then(|t| {
            if t.kind() == "type_identifier"
                || t.kind() == "scoped_type_identifier"
                || t.kind() == "generic_type"
            {
                raw_type(t, text)
            } else {
                None
            }
        });
        out.push(Param {
            name: text_of(name, text).to_owned(),
            ty,
        });
    }
    Some(out)
}

/// `x`, `()`, `(a, b)`: the parameter list of a lambda with inferred types.
fn param_list(params: &[Param]) -> String {
    match params {
        [one] => one.name.clone(),
        _ => format!(
            "({})",
            params
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The only expression of a lambda or method body: the body itself when it is an expression,
/// else `return e;` or `e;` alone in the block without a comment. The flag says whether it came
/// from an expression statement.
fn single_expression(body: Node) -> Option<(Node, bool)> {
    if body.kind() != "block" {
        return Some((body, false));
    }
    let kids = named(body);
    if kids.len() != 1 || kids.iter().any(|k| is_comment(*k)) {
        return None;
    }
    let only = kids[0];
    match only.kind() {
        "return_statement" => only.named_child(0).map(|e| (e, false)),
        "expression_statement" => only.named_child(0).map(|e| (e, true)),
        _ => None,
    }
}

/// Whether the lambda (or anonymous class) is an argument of a call whose overloads could tell
/// a shorter form apart from the longer one.
fn overload_sensitive(n: Node, text: &str) -> bool {
    n.parent()
        .filter(|a| a.kind() == "argument_list")
        .and_then(|a| a.parent())
        .filter(|c| c.kind() == "method_invocation")
        .and_then(|c| c.child_by_field_name("name"))
        .is_some_and(|name| OVERLOAD_SENSITIVE.contains(&text_of(name, text)))
}

/// Whether `n` is `this`, `super`, a name or a chain of names that names none of `params`.
fn plain_qualifier(n: Node, text: &str, params: &[Param]) -> bool {
    match n.kind() {
        "this" | "super" => true,
        "identifier" => !params.iter().any(|p| p.name == text_of(n, text)),
        "field_access" => {
            n.child_by_field_name("object")
                .is_some_and(|o| plain_qualifier(o, text, params))
                && n.child_by_field_name("field")
                    .is_some_and(|f| f.kind() == "identifier")
        }
        _ => false,
    }
}

/// Whether the argument list holds exactly the names `names`, in order.
fn arguments_are(args: Node, text: &str, names: &[&str]) -> bool {
    let list = named(args);
    list.len() == names.len()
        && list
            .iter()
            .zip(names)
            .all(|(a, n)| a.kind() == "identifier" && text_of(*a, text) == *n)
}

/// The innermost class body around `n` and the name of its class (`None` for an anonymous one).
fn enclosing_class<'t>(n: Node<'t>, text: &str) -> Option<(Node<'t>, Option<String>)> {
    let body = ancestors(n).find(|a| matches!(a.kind(), "class_body" | "enum_body"))?;
    let owner = body.parent()?;
    let name = match owner.kind() {
        "class_declaration" | "enum_declaration" | "record_declaration" => owner
            .child_by_field_name("name")
            .map(|x| text_of(x, text).to_owned()),
        _ => None,
    };
    Some((body, name))
}

fn has_modifier(member: Node, text: &str, word: &str) -> bool {
    named(member)
        .into_iter()
        .filter(|m| m.kind() == "modifiers")
        .any(|m| text_of(m, text).split_whitespace().any(|w| w == word))
}

/// The method reference a lambda with `params` and body expression `e` can become, deciding an
/// unqualified call in the class around `context` (behaviour 24).
fn method_reference(e: Node, params: &[Param], context: Node, text: &str) -> Option<String> {
    let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    match e.kind() {
        "method_invocation" => {
            if e.child_by_field_name("type_arguments").is_some() {
                return None;
            }
            let name = text_of(e.child_by_field_name("name")?, text);
            let args = e.child_by_field_name("arguments")?;
            match e.child_by_field_name("object") {
                // `q.m(a, b)` → `q::m`.
                Some(q)
                    if plain_qualifier(q, text, params) && arguments_are(args, text, &names) =>
                {
                    Some(format!("{}::{name}", text_of(q, text)))
                }
                // `a.m(b)` with `a` of a declared type `T` → `T::m`.
                Some(q)
                    if q.kind() == "identifier"
                        && params.first().is_some_and(|p| p.name == text_of(q, text))
                        && arguments_are(args, text, &names[1..]) =>
                {
                    Some(format!("{}::{name}", params[0].ty.as_deref()?))
                }
                Some(_) => None,
                // `m(a, b)` → `this::m` or `C::m`, for a method of the class around.
                None => {
                    if !arguments_are(args, text, &names) {
                        return None;
                    }
                    let (body, class) = enclosing_class(context, text)?;
                    let method = named(body).into_iter().find(|m| {
                        m.kind() == "method_declaration"
                            && m.child_by_field_name("name")
                                .is_some_and(|x| text_of(x, text) == name)
                    })?;
                    if has_modifier(method, text, "static") {
                        Some(format!("{}::{name}", class?))
                    } else {
                        Some(format!("this::{name}"))
                    }
                }
            }
        }
        // `new C(a, b)` → `C::new`.
        "object_creation_expression" => {
            if named(e).iter().any(|k| k.kind() == "class_body")
                || e.child_by_field_name("type_arguments").is_some()
            {
                return None;
            }
            let ty = e.child_by_field_name("type")?;
            let shown = match ty.kind() {
                "generic_type" => {
                    let args = named(ty)
                        .into_iter()
                        .find(|k| k.kind() == "type_arguments")?;
                    if named(args).is_empty() {
                        raw_type(ty, text)?
                    } else {
                        text_of(ty, text).to_owned()
                    }
                }
                "type_identifier" | "scoped_type_identifier" => text_of(ty, text).to_owned(),
                _ => return None,
            };
            arguments_are(e.child_by_field_name("arguments")?, text, &names)
                .then(|| format!("{shown}::new"))
        }
        _ => None,
    }
}

/// `java.statement-lambda` and `java.lambda-to-method-ref` at a lambda (behaviours 23, 24).
pub fn lambda(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some(body) = n.child_by_field_name("body") else {
        return;
    };
    let Some(params) = lambda_params(n, text) else {
        return;
    };
    let sensitive = overload_sensitive(n, text);
    let Some((e, statement)) = single_expression(body) else {
        return;
    };
    if sensitive && (statement || body.kind() != "block") {
        return;
    }
    let head_end = body.start_byte();
    let head = text[n.start_byte()..head_end].trim_end();
    if body.kind() == "block" {
        let result = format!("{} -> {}", param_list_text(n, text), text_of(e, text));
        let message = if result.len() <= SHORT && !result.contains('\n') {
            format!("Statement lambda can be an expression lambda: {result}")
        } else {
            "Statement lambda can be an expression lambda".to_owned()
        };
        out.push(Finding {
            id: "java.statement-lambda",
            start: n.start_byte(),
            end: n.start_byte() + head.len(),
            level: Level::Weak,
            message,
            fix: fix(
                "Replace with expression lambda",
                body.byte_range(),
                text_of(e, text).to_owned(),
            ),
        });
    }
    if let Some(reference) = method_reference(e, &params, n, text) {
        out.push(Finding {
            id: "java.lambda-to-method-ref",
            start: e.start_byte(),
            end: e.end_byte(),
            level: Level::Weak,
            message: format!("Lambda can be a method reference: {reference}"),
            fix: fix("Replace with method reference", n.byte_range(), reference),
        });
    }
}

/// The parameters of a lambda as written (`n`, `(a, b)`, `(String s)`).
fn param_list_text<'a>(lambda: Node, text: &'a str) -> &'a str {
    lambda
        .child_by_field_name("parameters")
        .map_or("()", |p| text_of(p, text))
}

/// Whether `name` is an interface of this file with exactly one abstract method and no
/// constants, defaults or static methods (behaviour 22).
fn functional_in_file(root: Node, name: &str, text: &str) -> bool {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.kind() == "interface_declaration"
            && n.child_by_field_name("name")
                .is_some_and(|x| text_of(x, text) == name)
        {
            if n.child_by_field_name("type_parameters").is_none()
                && named(n).iter().any(|k| k.kind() == "extends_interfaces")
            {
                return false;
            }
            let Some(body) = n.child_by_field_name("body") else {
                return false;
            };
            let members: Vec<Node> = named(body)
                .into_iter()
                .filter(|m| !is_comment(*m))
                .collect();
            return members.len() == 1
                && members[0].kind() == "method_declaration"
                && members[0].child_by_field_name("body").is_none()
                && !has_modifier(members[0], text, "default")
                && !has_modifier(members[0], text, "static");
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    false
}

/// Every name a subtree declares as parameter or local.
fn declared_names(n: Node, text: &str, out: &mut Vec<String>) {
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        declared_names_here(x, text, out);
        let mut c = x.walk();
        stack.extend(x.children(&mut c));
    }
}

/// Whether the subtree holds `this`, `super`, an unqualified call of `own` (the method itself),
/// or an unqualified call of a method no class of the file around declares (it could be one of
/// the interface's own methods).
fn body_needs_the_instance(body: Node, own: &str, outer: &[String], text: &str) -> bool {
    let mut stack = vec![body];
    while let Some(x) = stack.pop() {
        match x.kind() {
            "this" | "super" => return true,
            "method_invocation" if x.child_by_field_name("object").is_none() => {
                let name = x
                    .child_by_field_name("name")
                    .map_or("", |n| text_of(n, text));
                if name == own || !outer.iter().any(|m| m == name) {
                    return true;
                }
            }
            _ => {}
        }
        let mut c = x.walk();
        stack.extend(x.children(&mut c));
    }
    false
}

/// The names of the methods of every class body around `n`.
fn outer_methods(n: Node, text: &str) -> Vec<String> {
    ancestors(n)
        .filter(|a| matches!(a.kind(), "class_body" | "enum_body" | "interface_body"))
        .flat_map(named)
        .filter(|m| m.kind() == "method_declaration")
        .filter_map(|m| m.child_by_field_name("name"))
        .map(|x| text_of(x, text).to_owned())
        .collect()
}

/// The indentation of the line `at` lies on.
fn indent_of(at: usize, text: &str) -> &str {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// `block` with every line after the first moved left by `by` characters of indentation.
fn dedent(block: &str, by: usize) -> String {
    let mut out = String::with_capacity(block.len());
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            let lead = line.len() - line.trim_start_matches([' ', '\t']).len();
            out.push_str(&line[lead.min(by)..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// `java.anonymous-to-lambda` at an object creation (behaviour 22).
pub fn anonymous(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some(class_body) = named(n).into_iter().find(|k| k.kind() == "class_body") else {
        return;
    };
    let Some(ty) = n.child_by_field_name("type") else {
        return;
    };
    let Some(raw) = raw_type(ty, text) else {
        return;
    };
    let simple = simple_name(&raw);
    let root = ancestors(n).last().unwrap_or(n);
    if !FUNCTIONAL.contains(&simple) && !functional_in_file(root, simple, text) {
        return;
    }
    let Some(args) = n.child_by_field_name("arguments") else {
        return;
    };
    if !named(args).is_empty() {
        return;
    }
    let members = named(class_body);
    if members.len() != 1 || members[0].kind() != "method_declaration" {
        return;
    }
    let method = members[0];
    if method.child_by_field_name("type_parameters").is_some()
        || has_modifier(method, text, "static")
        || has_modifier(method, text, "default")
    {
        return;
    }
    let (Some(body), Some(name), Some(plist)) = (
        method.child_by_field_name("body"),
        method.child_by_field_name("name"),
        method.child_by_field_name("parameters"),
    ) else {
        return;
    };
    let Some(params) = formal_params(plist, text) else {
        return;
    };
    let outer = outer_methods(n, text);
    if body_needs_the_instance(body, text_of(name, text), &outer, text) {
        return;
    }
    // A lambda cannot declare a name the code around it already declares.
    let mut inner = Vec::new();
    declared_names(method, text, &mut inner);
    let member = ancestors(n).find(|a| {
        a.parent()
            .is_some_and(|p| matches!(p.kind(), "class_body" | "enum_body"))
    });
    if let Some(member) = member {
        let mut around = Vec::new();
        let mut stack = vec![member];
        while let Some(x) = stack.pop() {
            if x == n {
                continue;
            }
            declared_names_here(x, text, &mut around);
            let mut c = x.walk();
            stack.extend(x.children(&mut c));
        }
        if inner.iter().any(|i| around.contains(i)) {
            return;
        }
    }
    // The simplest form at once (IMP-0411), moved left by the indentation the class added.
    let by = indent_of(method.start_byte(), text)
        .len()
        .saturating_sub(indent_of(n.start_byte(), text).len());
    let sensitive = overload_sensitive(n, text);
    let lambda_text = match single_expression(body) {
        Some((e, statement)) if !(sensitive && statement) => {
            match method_reference(e, &params, n, text) {
                Some(reference) if !sensitive => reference,
                _ => format!(
                    "{} -> {}",
                    param_list(&params),
                    dedent(text_of(e, text), by)
                ),
            }
        }
        _ => format!(
            "{} -> {}",
            param_list(&params),
            dedent(text_of(body, text), by)
        ),
    };
    let head_end = args.end_byte();
    out.push(Finding {
        id: "java.anonymous-to-lambda",
        start: n.start_byte(),
        end: head_end,
        level: Level::Weak,
        message: format!("Anonymous {simple} can be a lambda"),
        fix: fix("Replace with lambda", n.byte_range(), lambda_text),
    });
}

/// The name one node declares itself (no recursion).
fn declared_names_here(x: Node, text: &str, out: &mut Vec<String>) {
    let name = match x.kind() {
        "variable_declarator"
        | "formal_parameter"
        | "catch_formal_parameter"
        | "resource"
        | "enhanced_for_statement" => x.child_by_field_name("name"),
        "lambda_expression" => x
            .child_by_field_name("parameters")
            .filter(|p| p.kind() == "identifier"),
        "inferred_parameters" => {
            out.extend(named(x).iter().map(|i| text_of(*i, text).to_owned()));
            None
        }
        _ => None,
    };
    if let Some(name) = name {
        out.push(text_of(name, text).to_owned());
    }
}

#[cfg(test)]
mod tests {
    use crate::LanguageId;
    use crate::inspections::inspect;
    use crate::jvm::apply;
    use crate::syntax::SyntaxDocument;

    const IDS: [&str; 3] = [
        "java.anonymous-to-lambda",
        "java.statement-lambda",
        "java.lambda-to-method-ref",
    ];

    fn found(text: &str) -> Vec<String> {
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        inspect(&doc)
            .into_iter()
            .filter(|f| IDS.contains(&f.id))
            .map(|f| format!("{} {}", f.id, &text[f.start..f.end]))
            .collect()
    }

    fn fixed(text: &str, id: &str, nth: usize) -> String {
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        let f = inspect(&doc)
            .into_iter()
            .filter(|f| f.id == id)
            .nth(nth)
            .unwrap();
        apply(text, &f.fix.unwrap().edits)
    }

    fn in_method(body: &str) -> String {
        format!(
            "import java.util.*;\nimport java.util.function.*;\nclass A {{\n    void helper(String s) {{}}\n    static void shared(String s) {{}}\n    void f(List<String> names) {{\n{body}\n    }}\n}}\n"
        )
    }

    #[test]
    fn anonymous_classes_that_can_be_lambdas() {
        let src = in_method(
            "        Runnable r = new Runnable() {\n            @Override\n            public void run() {\n                System.out.println(1);\n                System.out.println(2);\n            }\n        };",
        );
        assert_eq!(found(&src), ["java.anonymous-to-lambda new Runnable()"]);
        let after = fixed(&src, "java.anonymous-to-lambda", 0);
        assert!(
            after.contains("        Runnable r = () -> {\n            System.out.println(1);\n            System.out.println(2);\n        };"),
            "{after}"
        );
        // One statement: an expression lambda; a call that allows it: the method reference.
        let one = in_method(
            "        Comparator<String> c = new Comparator<String>() {\n            public int compare(String a, String b) { return a.compareTo(b); }\n        };\n        Consumer<String> k = new Consumer<>() {\n            public void accept(String s) { helper(s); }\n        };",
        );
        assert!(
            fixed(&one, "java.anonymous-to-lambda", 0)
                .contains("Comparator<String> c = String::compareTo;")
        );
        assert!(
            fixed(&one, "java.anonymous-to-lambda", 1)
                .contains("Consumer<String> k = this::helper;")
        );
        // Near misses: arguments, two members, `this`, recursion, an unknown type, a name the
        // method already declares, a call to a method of the interface.
        for quiet in [
            "        Thread t = new Thread() { public void run() {} };",
            "        Runnable r = new Runnable() { int n; public void run() { n++; } };",
            "        Runnable r = new Runnable() { public void run() { System.out.println(this); } };",
            "        Runnable r = new Runnable() { public void run() { run(); } };",
            "        Object o = new Object() { public String toString() { return \"\"; } };",
            "        Consumer<String> k = new Consumer<String>() { public void accept(String names) {} };",
            "        Comparator<String> c = new Comparator<String>() { public int compare(String a, String b) { return reversed().compare(a, b); } };",
            "        Runnable r = new Runnable() { public <T> void run() {} };",
        ] {
            assert!(found(&in_method(quiet)).is_empty(), "{quiet}");
        }
        // An interface of the file with one abstract method counts; one with two does not.
        let own = "interface Op { int apply(int x); }\ninterface Two { void a(); void b(); }\nclass B {\n  Op op = new Op() { public int apply(int x) { return x + 1; } };\n  Two two = new Two() { public void a() {} };\n}\n";
        assert_eq!(found(own), ["java.anonymous-to-lambda new Op()"]);
        assert!(fixed(own, "java.anonymous-to-lambda", 0).contains("Op op = x -> x + 1;"));
    }

    #[test]
    fn block_lambdas_with_one_statement() {
        let src = in_method(
            "        names.forEach(n -> { helper(n + 1); });\n        Function<String, Integer> f = s -> { return s.length() * 2; };\n        names.forEach(n -> { helper(n); helper(n); });\n        names.forEach(n -> {\n            // keep\n            helper(n + 2);\n        });\n        executor.submit(() -> { helper(\"x\"); });",
        );
        assert_eq!(
            found(&src),
            ["java.statement-lambda n ->", "java.statement-lambda s ->",]
        );
        assert!(
            fixed(&src, "java.statement-lambda", 0).contains("names.forEach(n -> helper(n + 1));")
        );
        assert!(
            fixed(&src, "java.statement-lambda", 1)
                .contains("Function<String, Integer> f = s -> s.length() * 2;")
        );
        let doc = SyntaxDocument::new(LanguageId::Java, false, &src).unwrap();
        let first = inspect(&doc)
            .into_iter()
            .find(|f| f.id == "java.statement-lambda")
            .unwrap();
        assert_eq!(first.level, crate::inspections::Level::Weak);
        assert!(
            first.message.ends_with(": n -> helper(n + 1)"),
            "{}",
            first.message
        );
    }

    #[test]
    fn lambdas_that_can_be_method_references() {
        let src = in_method(
            "        names.forEach(n -> System.out.println(n));\n        names.forEach(s -> helper(s));\n        names.forEach(s -> shared(s));\n        Function<String, Integer> len = (String s) -> s.length();\n        Function<String, StringBuilder> mk = s -> new StringBuilder(s);\n        Supplier<List<String>> list = () -> new ArrayList<>();\n        names.forEach(n -> make().use(n));\n        BiFunction<String, String, String> twice = (a, b) -> a.concat(a);\n        Function<String, Integer> inferred = s -> s.length();\n        names.forEach(n -> Collections.<String>emptyList().add(n));",
        );
        let refs: Vec<String> = {
            let doc = SyntaxDocument::new(LanguageId::Java, false, &src).unwrap();
            inspect(&doc)
                .into_iter()
                .filter(|f| f.id == "java.lambda-to-method-ref")
                .map(|f| f.message)
                .collect()
        };
        assert_eq!(
            refs,
            [
                "Lambda can be a method reference: System.out::println",
                "Lambda can be a method reference: this::helper",
                "Lambda can be a method reference: A::shared",
                "Lambda can be a method reference: String::length",
                "Lambda can be a method reference: StringBuilder::new",
                "Lambda can be a method reference: ArrayList::new",
            ]
        );
        assert!(
            fixed(&src, "java.lambda-to-method-ref", 0)
                .contains("names.forEach(System.out::println);")
        );
    }

    /// SPEC-0098 AK-14: the file of OBS-0050 gives one finding of each id at lines 37, 40 and 41.
    #[test]
    fn the_fold_file_of_the_observation() {
        let doc = SyntaxDocument::new(LanguageId::Java, false, FOLD).unwrap();
        let all = inspect(&doc);
        let line = |at: usize| FOLD[..at].matches('\n').count() + 1;
        let got: Vec<(&str, usize)> = all
            .iter()
            .filter(|f| IDS.contains(&f.id))
            .map(|f| (f.id, line(f.start)))
            .collect();
        assert_eq!(
            got,
            [
                ("java.anonymous-to-lambda", 37),
                ("java.statement-lambda", 40),
                ("java.lambda-to-method-ref", 41),
            ]
        );
        assert!(
            all.iter()
                .all(|f| f.level != crate::inspections::Level::Warning),
            "{all:?}"
        );
        let after = fixed(FOLD, "java.anonymous-to-lambda", 0);
        assert!(
            after.contains("    Runnable task = () -> names.forEach(n -> {\n            System.out.println(n);\n        });\n"),
            "{after}"
        );
    }

    pub(crate) const FOLD: &str = "/*
 * Header comment
 * over three lines.
 */
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * A class to fold.
 * Second line of the doc.
 */
public class Fold {
    // region Fields
    private final List<String> names = new ArrayList<>();
    private int count;
    // endregion

    // one
    // two
    // three
    public int getCount() {
        return count;
    }

    public void add(String name) {
        if (name != null) {
            for (int i = 0; i < 2; i++) {
                names.add(name + i);
            }
        } else {
            count--;
        }
        count++;
    }

    Runnable task = new Runnable() {
        @Override
        public void run() {
            names.forEach(n -> {
                System.out.println(n);
            });
        }
    };

    static class Inner {
        Map<String, Integer> sizes = Map.of(
                \"a\", 1,
                \"b\", 2);
    }
}
";
}
