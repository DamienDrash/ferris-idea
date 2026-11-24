//! Java inspections on the syntax tree (`SPEC-0125`): common mistakes and leftovers the compiler
//! accepts, with a quick fix where the change is safe without type information; and rules that need
//! types, decided with the file's type model and only where both types are known (`SPEC-0139`);
//! the v2 rules, suppression by comment and annotation and the suppression edits of `SPEC-0125`
//! v2 (behaviours 6–8).

use std::collections::{BTreeSet, HashMap};

use tree_sitter::Node;

use crate::inspections::{Finding, Fix, Level, SuppressScope};
use crate::java_types::{IMMUTABLE, is_integral, type_of};
use crate::jvm::TextEdit;

/// Ids and display names of the Java rules (Inspect Code shows the names).
pub const JAVA_RULES: &[(&str, &str)] = &[
    ("java.unused-import", "Unused import"),
    ("java.unused-local", "Unused local variable"),
    ("java.unused-private", "Unused private member"),
    (
        "java.boolean-literal-comparison",
        "Comparison with boolean literal",
    ),
    ("java.size-zero", "Size compared with zero"),
    ("java.self-assignment", "Variable assigned to itself"),
    ("java.empty-if", "'if' statement with empty body"),
    ("java.constant-condition", "Constant condition"),
    (
        "java.duplicate-condition",
        "Duplicate condition in 'if' chain",
    ),
    ("java.print-stack-trace", "Call to printStackTrace()"),
    ("java.boxing-constructor", "Deprecated boxing constructor"),
    (
        "java.empty-block",
        "Empty 'finally' or 'synchronized' block",
    ),
    (
        "java.equals-hash-code",
        "equals() and hashCode() not paired",
    ),
    ("java.string-equality", "String comparison with '=='"),
    ("java.boxed-equality", "Boxed values compared with '=='"),
    ("java.ignored-result", "Result of method call ignored"),
    (
        "java.equals-incompatible",
        "equals() between unrelated types",
    ),
    ("java.array-to-string", "Array printed as reference"),
    ("java.array-equals", "Arrays compared with equals()"),
    (
        "java.integer-division",
        "Integer division in floating-point context",
    ),
    ("java.redundant-cast", "Redundant type cast"),
    ("java.field-may-be-final", "Field may be 'final'"),
    ("java.unused-parameter", "Unused parameter"),
    ("java.null-dereference", "Dereference of a null value"),
    // The lambda simplifications of SPEC-0098 v11 (`java_lambdas`).
    (
        "java.anonymous-to-lambda",
        "Anonymous class can be a lambda",
    ),
    (
        "java.statement-lambda",
        "Statement lambda can be an expression lambda",
    ),
    (
        "java.lambda-to-method-ref",
        "Lambda can be a method reference",
    ),
    // The global inspection of SPEC-0316 (Inspect Code only).
    ("java.unused-declaration", "Unused declaration"),
];

/// Value types whose `equals` is only true for the same type.
const VALUE_TYPES: &[&str] = &[
    "String",
    "StringBuilder",
    "BigDecimal",
    "BigInteger",
    "Integer",
    "Long",
    "Short",
    "Byte",
    "Double",
    "Float",
    "Boolean",
    "Character",
];

/// `Objects` or `Arrays` as the file can name it: the simple name when imported (also with
/// `java.util.*`), else qualified.
fn util(class: &str, text: &str) -> String {
    let imported = text.contains(&format!("import java.util.{class};"))
        || text.contains("import java.util.*;");
    if imported {
        class.to_owned()
    } else {
        format!("java.util.{class}")
    }
}

/// `a == b` and `a != b` on two Strings or two boxed values: reference comparison (SPEC-0139).
fn reference_equality(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some((l, op, r)) = binary(n, text) else {
        return;
    };
    if op != "==" && op != "!=" {
        return;
    }
    // A string literal on one side is enough (the SPEC-0098 rule this replaces); otherwise both
    // types must be known.
    let literal = |x: Node| x.kind() == "string_literal";
    let null = |x: Node| x.kind() == "null_literal";
    let lt = type_of(l, text).unwrap_or_default();
    let rt = type_of(r, text).unwrap_or_default();
    let strings =
        (lt == "String" && rt == "String") || ((literal(l) || literal(r)) && !null(l) && !null(r));
    // `if (a == b) return …;` is the identity shortcut before a full comparison: meant as it is.
    let mut cond = n;
    while let Some(p) = cond
        .parent()
        .filter(|p| p.kind() == "parenthesized_expression")
    {
        cond = p;
    }
    let shortcut = cond.parent().is_some_and(|p| {
        p.kind() == "if_statement"
            && p.child_by_field_name("condition")
                .is_some_and(|c| c.id() == cond.id())
            && p.child_by_field_name("consequence").is_some_and(|c| {
                // Comments in the block do not count.
                let statements: Vec<Node> = children(c)
                    .into_iter()
                    .filter(|s| s.is_named() && !s.kind().ends_with("comment"))
                    .collect();
                c.kind() == "return_statement"
                    || (c.kind() == "block"
                        && statements.len() == 1
                        && statements[0].kind() == "return_statement")
            })
    });
    if shortcut && !(literal(l) || literal(r)) {
        return;
    }
    let (id, what) = if strings {
        ("java.string-equality", "Strings")
    } else if crate::java_types::BOXES.contains(&lt.as_str()) && lt == rt {
        ("java.boxed-equality", "boxed values")
    } else {
        return;
    };
    let (lx, rx) = (text_of(l, text), text_of(r, text));
    // A literal first is never null, so `"a".equals(x)` keeps the null case; otherwise
    // `Objects.equals` does.
    let call = if l.kind() == "string_literal" {
        format!("{lx}.equals({rx})")
    } else if r.kind() == "string_literal" {
        format!("{rx}.equals({lx})")
    } else {
        format!("{}.equals({lx}, {rx})", util("Objects", text))
    };
    let with = if op == "==" { call } else { format!("!{call}") };
    out.push(finding(
        id,
        n,
        Level::Warning,
        format!("{what} compared with '{op}' compare references, not values"),
        replace(format!("Replace with '{with}'"), n, with.clone()),
    ));
}

/// `double d = a / b;` with integral `a` and `b`: the fraction is lost before the conversion.
fn integer_division(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some((l, op, r)) = binary(n, text) else {
        return;
    };
    if op != "/" {
        return;
    }
    let integral = |x: Node| {
        type_of(x, text)
            .is_some_and(|t| is_integral(&t) || matches!(t.as_str(), "Integer" | "Long"))
    };
    if !integral(l) || !integral(r) {
        return;
    }
    // The type the result goes into.
    let floating = |t: &str| matches!(t, "double" | "float" | "Double" | "Float");
    let mut context = n;
    while let Some(p) = context
        .parent()
        .filter(|p| p.kind() == "parenthesized_expression")
    {
        context = p;
    }
    let target = context.parent().and_then(|p| match p.kind() {
        "variable_declarator"
            if p.child_by_field_name("value")
                .is_some_and(|v| v.id() == context.id()) =>
        {
            p.parent()
                .and_then(|d| d.child_by_field_name("type"))
                .map(|t| text_of(t, text).to_owned())
        }
        "assignment_expression"
            if p.child_by_field_name("right")
                .is_some_and(|v| v.id() == context.id()) =>
        {
            p.child_by_field_name("left").and_then(|x| type_of(x, text))
        }
        "return_statement" => {
            let mut m = p.parent();
            while let Some(x) = m {
                if x.kind() == "method_declaration" {
                    return x
                        .child_by_field_name("type")
                        .map(|t| text_of(t, text).to_owned());
                }
                if x.kind() == "lambda_expression" {
                    return None;
                }
                m = x.parent();
            }
            None
        }
        _ => None,
    });
    let Some(target) = target.filter(|t| floating(t)) else {
        return;
    };
    let cast = if target.starts_with('f') || target.starts_with('F') {
        "float"
    } else {
        "double"
    };
    let with = format!("({cast}) {} / {}", text_of(l, text), text_of(r, text));
    out.push(finding(
        "java.integer-division",
        n,
        Level::Warning,
        format!("Integer division in a {target} context: the fraction is lost"),
        replace(format!("Replace with '{with}'"), n, with.clone()),
    ));
}

/// A statement that calls a method of an immutable type and drops the new value: `s.trim();`.
fn ignored_result(statement: Node, text: &str, out: &mut Vec<Finding>) {
    let Some(call) = statement
        .named_child(0)
        .filter(|c| c.kind() == "method_invocation")
    else {
        return;
    };
    let (Some(object), Some(name)) = (
        call.child_by_field_name("object"),
        call.child_by_field_name("name"),
    ) else {
        return;
    };
    let Some(receiver) = type_of(object, text) else {
        return;
    };
    let method = text_of(name, text);
    let returns_new = IMMUTABLE
        .iter()
        .any(|(t, methods)| *t == receiver && methods.contains(&method));
    if !returns_new {
        return;
    }
    let fix = (object.kind() == "identifier").then(|| {
        let var = text_of(object, text);
        Fix {
            learn: None,
            keep: None,
            bundle_key: None,
            action: None,
            title: format!("Assign the result to '{var}'"),
            edits: vec![crate::jvm::TextEdit {
                range: call.start_byte()..call.start_byte(),
                text: format!("{var} = "),
            }],
        }
    });
    out.push(finding(
        "java.ignored-result",
        call,
        Level::Warning,
        format!("Result of '{receiver}.{method}()' is ignored: {receiver} values do not change"),
        fix,
    ));
}

/// `a.equals(b)` on unrelated value types or arrays, `arr.toString()`, `String.valueOf(arr)` and
/// `System.out.println(arr)` (SPEC-0139).
fn typed_invocation(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some(name) = n.child_by_field_name("name").map(|m| text_of(m, text)) else {
        return;
    };
    let args: Vec<Node> = n
        .child_by_field_name("arguments")
        .map(|a| {
            let mut c = a.walk();
            a.named_children(&mut c).collect()
        })
        .unwrap_or_default();
    let object = n.child_by_field_name("object");
    let to_string = |arr: Node, ty: &str| {
        let f = if ty.matches("[]").count() > 1 {
            "deepToString"
        } else {
            "toString"
        };
        format!("{}.{f}({})", util("Arrays", text), text_of(arr, text))
    };
    match (name, args.as_slice(), object) {
        ("equals", [arg], Some(obj)) => {
            let (Some(a), Some(b)) = (type_of(obj, text), type_of(*arg, text)) else {
                return;
            };
            if a.ends_with("[]") {
                let with = format!(
                    "{}.equals({}, {})",
                    util("Arrays", text),
                    text_of(obj, text),
                    text_of(*arg, text)
                );
                out.push(finding(
                    "java.array-equals",
                    n,
                    Level::Warning,
                    "equals() on an array compares references, not contents".into(),
                    replace(format!("Replace with '{with}'"), n, with.clone()),
                ));
                return;
            }
            let (a, b) = (crate::java_types::boxed(&a), crate::java_types::boxed(&b));
            if a != b && VALUE_TYPES.contains(&a) && VALUE_TYPES.contains(&b) {
                out.push(finding(
                    "java.equals-incompatible",
                    n,
                    Level::Warning,
                    format!("'equals()' between {a} and {b} is always false"),
                    None,
                ));
            }
        }
        ("toString", [], Some(obj)) => {
            if let Some(ty) = type_of(obj, text).filter(|t| t.ends_with("[]")) {
                let with = to_string(obj, &ty);
                out.push(finding(
                    "java.array-to-string",
                    n,
                    Level::Warning,
                    format!("'toString()' on an array prints its type and hash, not '{with}'"),
                    replace(format!("Replace with '{with}'"), n, with.clone()),
                ));
            }
        }
        ("valueOf" | "println" | "print", [arg], Some(obj))
            if (name == "valueOf" && text_of(obj, text) == "String")
                || (name != "valueOf"
                    && matches!(text_of(obj, text), "System.out" | "System.err")) =>
        {
            // `char[]` has its own overloads that print the characters.
            if let Some(ty) = type_of(*arg, text).filter(|t| t.ends_with("[]") && t != "char[]") {
                let with = to_string(*arg, &ty);
                out.push(finding(
                    "java.array-to-string",
                    *arg,
                    Level::Warning,
                    format!("The array is printed as its type and hash; use '{with}'"),
                    replace(format!("Replace with '{with}'"), *arg, with.clone()),
                ));
            }
        }
        _ => {}
    }
}

/// `"x" + arr`: string concatenation with an array operand.
fn array_concatenation(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some((l, op, r)) = binary(n, text) else {
        return;
    };
    if op != "+" || type_of(n, text).as_deref() != Some("String") {
        return;
    }
    for side in [l, r] {
        if let Some(ty) = type_of(side, text).filter(|t| t.ends_with("[]")) {
            let f = if ty.matches("[]").count() > 1 {
                "deepToString"
            } else {
                "toString"
            };
            let with = format!("{}.{f}({})", util("Arrays", text), text_of(side, text));
            out.push(finding(
                "java.array-to-string",
                side,
                Level::Warning,
                format!("The array is concatenated as its type and hash; use '{with}'"),
                replace(format!("Replace with '{with}'"), side, with.clone()),
            ));
        }
    }
}

const BOXES: &[&str] = &[
    "Integer",
    "Long",
    "Short",
    "Byte",
    "Double",
    "Float",
    "Boolean",
    "Character",
];

/// Methods serialization calls by name, so they count as used.
const SERIALIZATION: &[&str] = &[
    "readObject",
    "writeObject",
    "readResolve",
    "writeReplace",
    "readObjectNoData",
];

fn text_of<'a>(n: Node, text: &'a str) -> &'a str {
    &text[n.byte_range()]
}

fn children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

fn finding(id: &'static str, n: Node, level: Level, message: String, fix: Option<Fix>) -> Finding {
    Finding {
        id,
        start: n.start_byte(),
        end: n.end_byte(),
        level,
        message,
        fix,
    }
}

/// The range of a whole statement line: from the line start to after its newline when the
/// statement is alone on its line, else the statement itself.
fn statement_line(n: Node, text: &str) -> std::ops::Range<usize> {
    let line_start = text[..n.start_byte()].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[n.end_byte()..]
        .find('\n')
        .map_or(text.len(), |i| n.end_byte() + i + 1);
    let alone = text[line_start..n.start_byte()].trim().is_empty()
        && text[n.end_byte()..line_end].trim().is_empty();
    if alone {
        line_start..line_end
    } else {
        n.byte_range()
    }
}

fn delete(title: &str, range: std::ops::Range<usize>) -> Option<Fix> {
    Some(Fix {
        learn: None,
        keep: None,
        bundle_key: None,
        action: None,
        title: title.to_owned(),
        edits: vec![TextEdit {
            range,
            text: String::new(),
        }],
    })
}

fn replace(title: String, n: Node, with: String) -> Option<Fix> {
    Some(Fix {
        learn: None,
        keep: None,
        bundle_key: None,
        action: None,
        title,
        edits: vec![TextEdit {
            range: n.byte_range(),
            text: with,
        }],
    })
}

/// An expression that can take a `!` without parentheses.
fn simple(n: Node) -> bool {
    matches!(
        n.kind(),
        "identifier"
            | "method_invocation"
            | "field_access"
            | "parenthesized_expression"
            | "true"
            | "false"
            | "array_access"
            | "this"
    )
}

fn negated(n: Node, text: &str) -> String {
    if simple(n) {
        format!("!{}", text_of(n, text))
    } else {
        format!("!({})", text_of(n, text))
    }
}

fn binary<'t>(n: Node<'t>, text: &str) -> Option<(Node<'t>, String, Node<'t>)> {
    let l = n.child_by_field_name("left")?;
    let r = n.child_by_field_name("right")?;
    let op = n.child_by_field_name("operator")?;
    Some((l, text_of(op, text).to_owned(), r))
}

/// `x == true`, `x != false`, … (weak) with the simplification as fix.
fn boolean_literal(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some((l, op, r)) = binary(n, text) else {
        return;
    };
    if op != "==" && op != "!=" {
        return;
    }
    let lit = |x: Node| matches!(x.kind(), "true" | "false");
    let (other, value) = match (lit(l), lit(r)) {
        (false, true) => (l, r.kind() == "true"),
        (true, false) => (r, l.kind() == "true"),
        _ => return,
    };
    let keep = (op == "==") == value;
    let with = if keep {
        text_of(other, text).to_owned()
    } else {
        negated(other, text)
    };
    out.push(finding(
        "java.boolean-literal-comparison",
        n,
        Level::Weak,
        format!("'{}' can be simplified to '{with}'", text_of(n, text)),
        replace(format!("Simplify to '{with}'"), n, with.clone()),
    ));
}

/// `c.size() == 0`, `s.length() != 0`, `c.size() > 0` (weak) → `isEmpty()`.
fn size_zero(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some((l, op, r)) = binary(n, text) else {
        return;
    };
    let zero = |x: Node| x.kind() == "decimal_integer_literal" && text_of(x, text) == "0";
    let call = |x: Node| {
        x.kind() == "method_invocation"
            && x.child_by_field_name("name")
                .is_some_and(|m| matches!(text_of(m, text), "size" | "length"))
            && x.child_by_field_name("arguments")
                .is_some_and(|a| a.named_child_count() == 0)
    };
    let (inv, empty) = match (call(l), zero(r), zero(l), call(r)) {
        (true, true, _, _) => (
            l,
            match op.as_str() {
                "==" | "<=" => true,
                "!=" | ">" => false,
                _ => return,
            },
        ),
        (_, _, true, true) => (
            r,
            match op.as_str() {
                "==" | ">=" => true,
                "!=" | "<" => false,
                _ => return,
            },
        ),
        _ => return,
    };
    let method = inv
        .child_by_field_name("name")
        .map_or("size", |m| text_of(m, text));
    // Only where the receiver's declared type is known to have isEmpty(): String for length()
    // (CharSequence and StringBuilder have it only since Java 15), collections and maps for size().
    let Some(object) = inv.child_by_field_name("object") else {
        return;
    };
    let Some(ty) = declared_type(object, text) else {
        return;
    };
    let base = ty
        .split('<')
        .next()
        .unwrap_or(&ty)
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim();
    let fits = match method {
        "length" => base == "String",
        _ => ["List", "Set", "Map", "Collection", "Queue", "Deque"]
            .iter()
            .any(|s| base.ends_with(s)),
    };
    if !fits {
        return;
    }
    let call_text = match inv.child_by_field_name("object") {
        Some(o) => format!("{}.isEmpty()", text_of(o, text)),
        None => "isEmpty()".to_owned(),
    };
    let with = if empty {
        call_text
    } else {
        format!("!{call_text}")
    };
    out.push(finding(
        "java.size-zero",
        n,
        Level::Weak,
        format!("'{method}()' compared with 0 can be replaced with '{with}'"),
        replace(format!("Replace with '{with}'"), n, with.clone()),
    ));
}

/// The declared type of a plain name: the nearest parameter, earlier local variable or field of
/// that name around `name` (`None` for anything else, e.g. a method call).
fn declared_type(name: Node, text: &str) -> Option<String> {
    if name.kind() != "identifier" {
        return None;
    }
    let wanted = text_of(name, text);
    let declares = |decl: Node| -> Option<String> {
        let ty = decl.child_by_field_name("type")?;
        let mut c = decl.walk();
        let hit = match decl.kind() {
            "formal_parameter" => decl
                .child_by_field_name("name")
                .is_some_and(|n| text_of(n, text) == wanted),
            _ => decl.children_by_field_name("declarator", &mut c).any(|d| {
                d.child_by_field_name("name")
                    .is_some_and(|n| text_of(n, text) == wanted)
            }),
        };
        hit.then(|| text_of(ty, text).to_owned())
    };
    let mut at = name.parent();
    while let Some(scope) = at {
        // Variables these scopes declare themselves: a typed loop variable, or names whose type
        // is not written down (lambda parameters without types, catch parameters).
        match scope.kind() {
            "enhanced_for_statement"
                if scope
                    .child_by_field_name("name")
                    .is_some_and(|n| text_of(n, text) == wanted) =>
            {
                return scope
                    .child_by_field_name("type")
                    .map(|t| text_of(t, text).to_owned());
            }
            "lambda_expression" => {
                let shadowed = scope.child_by_field_name("parameters").is_some_and(|p| {
                    p.kind() == "identifier" && text_of(p, text) == wanted
                        || (p.kind() == "inferred_parameters"
                            && children(p)
                                .iter()
                                .any(|c| c.kind() == "identifier" && text_of(*c, text) == wanted))
                });
                if shadowed {
                    return None;
                }
            }
            "catch_clause" => {
                let shadowed = children(scope)
                    .into_iter()
                    .filter(|c| c.kind() == "catch_formal_parameter")
                    .any(|c| {
                        c.child_by_field_name("name")
                            .is_some_and(|n| text_of(n, text) == wanted)
                    });
                if shadowed {
                    return None;
                }
            }
            _ => {}
        }
        for child in children(scope) {
            let found = match child.kind() {
                "formal_parameters" => children(child)
                    .into_iter()
                    .filter(|p| p.kind() == "formal_parameter")
                    .find_map(declares),
                "local_variable_declaration" if child.start_byte() < name.start_byte() => {
                    declares(child)
                }
                "field_declaration" => declares(child),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        at = scope.parent();
    }
    None
}

fn assignment(n: Node, text: &str, out: &mut Vec<Finding>) {
    let (Some(l), Some(r)) = (
        n.child_by_field_name("left"),
        n.child_by_field_name("right"),
    ) else {
        return;
    };
    let op = n
        .child_by_field_name("operator")
        .map_or("=", |o| text_of(o, text));
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    if op == "=" && squash(text_of(l, text)) == squash(text_of(r, text)) {
        let stmt = n.parent().filter(|p| p.kind() == "expression_statement");
        out.push(finding(
            "java.self-assignment",
            n,
            Level::Warning,
            format!("'{}' is assigned to itself", text_of(l, text)),
            stmt.and_then(|s| delete("Delete the assignment", statement_line(s, text))),
        ));
    }
}

fn condition_value(n: Node<'_>) -> Option<(Node<'_>, bool)> {
    let c = n.child_by_field_name("condition")?;
    let inner = if c.kind() == "parenthesized_expression" {
        c.named_child(0)?
    } else {
        c
    };
    match inner.kind() {
        "true" => Some((c, true)),
        "false" => Some((c, false)),
        _ => None,
    }
}

fn if_statement(n: Node, text: &str, out: &mut Vec<Finding>) {
    if n.child_by_field_name("consequence")
        .is_none_or(|c| c.kind() == ";")
    {
        let head_end = n
            .child_by_field_name("condition")
            .map_or(n.end_byte(), |c| c.end_byte());
        out.push(Finding {
            id: "java.empty-if",
            start: n.start_byte(),
            end: head_end,
            level: Level::Warning,
            message: "'if' statement has an empty body: the ';' ends it".into(),
            fix: None,
        });
    }
    if let Some((c, value)) = condition_value(n) {
        out.push(finding(
            "java.constant-condition",
            c,
            Level::Warning,
            format!("Condition is always {value}"),
            None,
        ));
    }
    // Duplicate conditions along an if / else-if chain, from its head.
    let is_tail = n.parent().is_some_and(|p| {
        p.kind() == "if_statement" && p.child_by_field_name("alternative") == Some(n)
    });
    if is_tail {
        return;
    }
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    let mut seen = BTreeSet::new();
    let mut at = Some(n);
    while let Some(i) = at {
        if let Some(c) = i.child_by_field_name("condition")
            && !seen.insert(squash(text_of(c, text)))
        {
            out.push(finding(
                "java.duplicate-condition",
                c,
                Level::Warning,
                format!(
                    "Condition '{}' was already checked above in this 'if' chain",
                    text_of(c, text).trim_matches(|ch| ch == '(' || ch == ')')
                ),
                None,
            ));
        }
        at = i
            .child_by_field_name("alternative")
            .filter(|a| a.kind() == "if_statement");
    }
}

fn loop_condition(n: Node, out: &mut Vec<Finding>) {
    // `while (true)` is the usual endless loop; `while (false)` never runs.
    if let Some((c, false)) = condition_value(n) {
        out.push(finding(
            "java.constant-condition",
            c,
            Level::Warning,
            "Condition is always false: the loop never runs".into(),
            None,
        ));
    }
}

fn invocation(n: Node, text: &str, out: &mut Vec<Finding>) {
    let named = n
        .child_by_field_name("name")
        .is_some_and(|m| text_of(m, text) == "printStackTrace");
    let no_args = n
        .child_by_field_name("arguments")
        .is_some_and(|a| a.named_child_count() == 0);
    if named && no_args {
        out.push(finding(
            "java.print-stack-trace",
            n,
            Level::Weak,
            "Call to printStackTrace(): the trace goes to standard error, not to the log".into(),
            None,
        ));
    }
}

fn creation(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some(ty) = n.child_by_field_name("type") else {
        return;
    };
    let name = text_of(ty, text);
    if !BOXES.contains(&name) || n.child_by_field_name("body").is_some() {
        return;
    }
    let args = n
        .child_by_field_name("arguments")
        .map_or("()", |a| text_of(a, text));
    let with = format!("{name}.valueOf{args}");
    out.push(finding(
        "java.boxing-constructor",
        n,
        Level::Warning,
        format!("'new {name}(…)' is deprecated; use '{name}.valueOf(…)'"),
        replace(format!("Replace with '{name}.valueOf(…)'"), n, with),
    ));
}

fn empty_body(n: Node, what: &str, out: &mut Vec<Finding>) {
    let body = n
        .child_by_field_name("body")
        .or_else(|| children(n).into_iter().find(|c| c.kind() == "block"));
    if let Some(b) = body
        && b.named_child_count() == 0
    {
        out.push(finding(
            "java.empty-block",
            b,
            Level::Warning,
            format!("Empty '{what}' block"),
            None,
        ));
    }
}

/// Every `identifier`/`type_identifier` text with its node starts, plus names used in Javadoc.
fn identifier_uses(root: Node, text: &str) -> (HashMap<String, Vec<usize>>, BTreeSet<String>) {
    let mut uses: HashMap<String, Vec<usize>> = HashMap::new();
    let mut doc = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "identifier" | "type_identifier" => uses
                .entry(text_of(n, text).to_owned())
                .or_default()
                .push(n.start_byte()),
            k if k.contains("comment") => {
                for word in text_of(n, text).split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() {
                        doc.insert(word.to_owned());
                    }
                }
            }
            _ => {}
        }
        stack.extend(children(n));
    }
    (uses, doc)
}

fn unused_imports(
    root: Node,
    text: &str,
    uses: &HashMap<String, Vec<usize>>,
    doc: &BTreeSet<String>,
    out: &mut Vec<Finding>,
) {
    for n in children(root)
        .into_iter()
        .filter(|n| n.kind() == "import_declaration")
    {
        let raw = text_of(n, text)
            .trim_start_matches("import")
            .trim()
            .trim_end_matches(';')
            .trim();
        let path = raw.strip_prefix("static ").unwrap_or(raw).trim();
        if path.ends_with('*') {
            continue;
        }
        let name = path.rsplit('.').next().unwrap_or(path);
        // Uses outside the import itself.
        let inside = |b: &usize| *b >= n.start_byte() && *b < n.end_byte();
        let used = uses
            .get(name)
            .is_some_and(|starts| starts.iter().any(|b| !inside(b)))
            || doc.contains(name);
        if !used {
            out.push(finding(
                "java.unused-import",
                n,
                Level::Weak,
                format!("Unused import '{path}'"),
                delete("Remove unused import", statement_line(n, text)),
            ));
        }
    }
}

fn has_annotation(member: Node) -> bool {
    children(member)
        .into_iter()
        .find(|c| c.kind() == "modifiers")
        .is_some_and(|m| children(m).iter().any(|c| c.kind().contains("annotation")))
}

fn is_private(member: Node, text: &str) -> bool {
    children(member)
        .into_iter()
        .find(|c| c.kind() == "modifiers")
        .is_some_and(|m| text_of(m, text).split_whitespace().any(|w| w == "private"))
}

/// Uses of `name` outside the node `except`.
fn used_elsewhere(uses: &HashMap<String, Vec<usize>>, name: &str, except: Node) -> bool {
    uses.get(name)
        .is_some_and(|starts| starts.iter().any(|b| *b != except.start_byte()))
}

fn modifier_words<'a>(member: Node, text: &'a str) -> Vec<&'a str> {
    children(member)
        .into_iter()
        .find(|c| c.kind() == "modifiers")
        .map(|m| {
            children(m)
                .into_iter()
                .filter(|c| !c.kind().contains("annotation"))
                .map(|c| text_of(c, text))
                .collect()
        })
        .unwrap_or_default()
}

/// Names assigned anywhere in the file: the left side of an assignment (a name or the field of a
/// field access) and the operand of `++`/`--`.
fn assigned_names(root: Node, text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let target = |x: Node| match x.kind() {
        "identifier" => Some(text_of(x, text).to_owned()),
        "field_access" => x
            .child_by_field_name("field")
            .map(|f| text_of(f, text).to_owned()),
        _ => None,
    };
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let assigned = match n.kind() {
            "assignment_expression" => n.child_by_field_name("left").and_then(target),
            "update_expression" => n.named_child(0).and_then(target),
            _ => None,
        };
        out.extend(assigned);
        stack.extend(children(n));
    }
    out
}

/// A private field without `final` (and not `volatile`) whose declarators all have an
/// initializer and are never assigned in the file (`SPEC-0125` v2 behaviour 6).
fn field_may_be_final(
    field: Node,
    text: &str,
    names: &[(Node, bool, bool)],
    assigned: &BTreeSet<String>,
    out: &mut Vec<Finding>,
) {
    let words = modifier_words(field, text);
    if names.is_empty() || words.contains(&"final") || words.contains(&"volatile") {
        return;
    }
    let qualifies = names
        .iter()
        .all(|(name, _, init)| *init && !assigned.contains(text_of(*name, text)));
    let Some(modifiers) = children(field)
        .into_iter()
        .find(|c| c.kind() == "modifiers")
    else {
        return;
    };
    if !qualifies {
        return;
    }
    for (name, unused, _) in names {
        if *unused {
            continue;
        }
        let n = text_of(*name, text);
        out.push(finding(
            "java.field-may-be-final",
            *name,
            Level::Warning,
            format!("Field '{n}' may be 'final'"),
            Some(Fix {
                learn: None,
                keep: None,
                bundle_key: None,
                action: None,
                title: "Add 'final' modifier".into(),
                edits: vec![TextEdit {
                    range: modifiers.end_byte()..modifiers.end_byte(),
                    text: " final".into(),
                }],
            }),
        ));
    }
}

/// Parameters of a private method or constructor that its body never mentions (`SPEC-0125` v2
/// behaviour 6); no fix, since the callers would change.
fn unused_parameters(m: Node, text: &str, out: &mut Vec<Finding>) {
    if !is_private(m, text) {
        return;
    }
    if m.kind() == "method_declaration"
        && m.child_by_field_name("name")
            .is_some_and(|n| SERIALIZATION.contains(&text_of(n, text)))
    {
        return;
    }
    let (Some(body), Some(params)) = (
        m.child_by_field_name("body"),
        m.child_by_field_name("parameters"),
    ) else {
        return;
    };
    let mut mentioned = BTreeSet::new();
    let mut stack = vec![body];
    while let Some(x) = stack.pop() {
        if x.kind() == "identifier" {
            mentioned.insert(text_of(x, text));
        }
        stack.extend(children(x));
    }
    for p in children(params) {
        let name = match p.kind() {
            "formal_parameter" => p.child_by_field_name("name"),
            "spread_parameter" => children(p)
                .into_iter()
                .find(|c| c.kind() == "variable_declarator")
                .and_then(|d| d.child_by_field_name("name")),
            _ => None,
        };
        let Some(name) = name else { continue };
        let n = text_of(name, text);
        if !mentioned.contains(n) {
            out.push(finding(
                "java.unused-parameter",
                name,
                Level::Weak,
                format!("Parameter '{n}' is never used"),
                None,
            ));
        }
    }
}

/// `(T) x` where the plain name `x` is declared in the file with exactly the type `T`
/// (`SPEC-0125` v2 behaviour 6).
fn redundant_cast(n: Node, text: &str, out: &mut Vec<Finding>) {
    let mut c = n.walk();
    let types: Vec<Node> = n.children_by_field_name("type", &mut c).collect();
    let (Some(value), [ty]) = (n.child_by_field_name("value"), types.as_slice()) else {
        return;
    };
    if value.kind() != "identifier" {
        return;
    }
    let Some(declared) = declared_type(value, text) else {
        return;
    };
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    if squash(&declared) != squash(text_of(*ty, text)) {
        return;
    }
    let name = text_of(value, text);
    out.push(finding(
        "java.redundant-cast",
        n,
        Level::Warning,
        format!(
            "Casting '{name}' to '{}' is redundant",
            text_of(*ty, text).trim()
        ),
        replace("Remove redundant cast".into(), n, name.to_owned()),
    ));
}

fn unparenthesized(mut n: Node) -> Node {
    while n.kind() == "parenthesized_expression" {
        match n.named_child(0) {
            Some(inner) => n = inner,
            None => break,
        }
    }
    n
}

/// The first mention of the variable `name` in `n`, in source order; the member part of
/// `a.name` and `a.name()` is no mention.
fn first_mention<'t>(n: Node<'t>, name: &str, text: &str) -> Option<Node<'t>> {
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        if x.kind() == "identifier" && text_of(x, text) == name {
            let member = x.parent().is_some_and(|p| {
                (p.kind() == "field_access" && p.child_by_field_name("field") == Some(x))
                    || (p.kind() == "method_invocation" && p.child_by_field_name("name") == Some(x))
            });
            if !member {
                return Some(x);
            }
        }
        let mut kids = children(x);
        kids.reverse();
        stack.extend(kids);
    }
    None
}

/// A local variable initialised with `null` (or a conditional with a `null` branch) whose next
/// mention in its block is the receiver of a call or field access (`SPEC-0125` v2 behaviour 6).
fn null_dereference(decl: Node, text: &str, out: &mut Vec<Finding>) {
    let Some(block) = decl
        .parent()
        .filter(|p| matches!(p.kind(), "block" | "switch_block_statement_group"))
    else {
        return;
    };
    let null = |x: Node| unparenthesized(x).kind() == "null_literal";
    let mut c = decl.walk();
    for d in decl.children_by_field_name("declarator", &mut c) {
        let (Some(name), Some(value)) = (
            d.child_by_field_name("name"),
            d.child_by_field_name("value"),
        ) else {
            continue;
        };
        let value = unparenthesized(value);
        let maybe = if value.kind() == "null_literal" {
            false
        } else if value.kind() == "ternary_expression"
            && ["consequence", "alternative"]
                .iter()
                .any(|f| value.child_by_field_name(f).is_some_and(null))
        {
            true
        } else {
            continue;
        };
        let n = text_of(name, text);
        let Some(mention) = children(block)
            .into_iter()
            .filter(|s| s.start_byte() >= decl.end_byte())
            .find_map(|s| first_mention(s, n, text))
        else {
            continue;
        };
        let Some(parent) = mention.parent() else {
            continue;
        };
        let (member, call) = match parent.kind() {
            "method_invocation" if parent.child_by_field_name("object") == Some(mention) => {
                (parent.child_by_field_name("name"), true)
            }
            "field_access" if parent.child_by_field_name("object") == Some(mention) => {
                (parent.child_by_field_name("field"), false)
            }
            _ => continue,
        };
        let Some(member) = member else { continue };
        let m = text_of(member, text);
        let what = if call {
            format!("calling '{m}()'")
        } else {
            format!("reading '{m}'")
        };
        let message = if maybe {
            format!("'{n}' may be null here: {what} may throw NullPointerException")
        } else {
            format!("'{n}' is null here: {what} throws NullPointerException")
        };
        out.push(finding(
            "java.null-dereference",
            member,
            Level::Warning,
            message,
            None,
        ));
    }
}

fn class_members(
    class: Node,
    text: &str,
    uses: &HashMap<String, Vec<usize>>,
    assigned: &BTreeSet<String>,
    out: &mut Vec<Finding>,
) {
    let Some(body) = class.child_by_field_name("body") else {
        return;
    };
    let mut equals = false;
    let mut hash = false;
    for m in children(body) {
        match m.kind() {
            "method_declaration" => {
                let Some(name) = m.child_by_field_name("name") else {
                    continue;
                };
                let n = text_of(name, text);
                let params = m.child_by_field_name("parameters").map_or(0, |p| {
                    children(p)
                        .iter()
                        .filter(|c| c.kind() == "formal_parameter")
                        .count()
                });
                equals |= n == "equals" && params == 1;
                hash |= n == "hashCode" && params == 0;
                if is_private(m, text)
                    && !has_annotation(m)
                    && !SERIALIZATION.contains(&n)
                    && !used_elsewhere(uses, n, name)
                {
                    out.push(finding(
                        "java.unused-private",
                        name,
                        Level::Warning,
                        format!("Private method '{n}()' is never used"),
                        None,
                    ));
                }
            }
            "field_declaration" if is_private(m, text) => {
                let annotated = has_annotation(m);
                let mut c = m.walk();
                // (name, unused, has an initializer)
                let mut names = Vec::new();
                for d in m.children_by_field_name("declarator", &mut c) {
                    let Some(name) = d.child_by_field_name("name") else {
                        continue;
                    };
                    let n = text_of(name, text);
                    let unused =
                        !annotated && n != "serialVersionUID" && !used_elsewhere(uses, n, name);
                    if unused {
                        out.push(finding(
                            "java.unused-private",
                            name,
                            Level::Warning,
                            format!("Private field '{n}' is never used"),
                            None,
                        ));
                    }
                    names.push((name, unused, d.child_by_field_name("value").is_some()));
                }
                field_may_be_final(m, text, &names, assigned, out);
            }
            _ => {}
        }
    }
    if equals != hash
        && let Some(name) = class.child_by_field_name("name")
    {
        let message = if equals {
            "Class overrides equals() but not hashCode(): equal objects may get different hash codes"
        } else {
            "Class overrides hashCode() but not equals()"
        };
        out.push(finding(
            "java.equals-hash-code",
            name,
            Level::Warning,
            message.into(),
            None,
        ));
    }
}

/// Literals and plain names: removing such an initializer changes nothing else.
fn side_effect_free(n: Node) -> bool {
    n.kind().ends_with("_literal")
        || matches!(
            n.kind(),
            "true" | "false" | "null_literal" | "identifier" | "string_literal"
        )
}

fn unused_local(decl: Node, text: &str, out: &mut Vec<Finding>) {
    let Some(block) = decl
        .parent()
        .filter(|p| matches!(p.kind(), "block" | "switch_block_statement_group"))
    else {
        return;
    };
    let mut c = decl.walk();
    let declarators: Vec<Node> = decl.children_by_field_name("declarator", &mut c).collect();
    for d in &declarators {
        let Some(name) = d.child_by_field_name("name") else {
            continue;
        };
        let n = text_of(name, text);
        // Uses after the declaration, within the block that holds it.
        let mut used = false;
        let mut stack = vec![block];
        while let Some(x) = stack.pop() {
            if x.kind() == "identifier"
                && x.start_byte() > name.start_byte()
                && text_of(x, text) == n
            {
                used = true;
                break;
            }
            stack.extend(children(x));
        }
        if used {
            continue;
        }
        let fix = (declarators.len() == 1
            && d.child_by_field_name("value").is_none_or(side_effect_free))
        .then(|| {
            delete(
                &format!("Remove variable '{n}'"),
                statement_line(decl, text),
            )
        })
        .flatten();
        out.push(finding(
            "java.unused-local",
            name,
            Level::Warning,
            format!("Variable '{n}' is never used"),
            fix,
        ));
    }
}

/// Elements a `//noinspection` comment or `@SuppressWarnings` can stand on (`SPEC-0125` v2
/// behaviour 7).
fn suppressible(n: Node) -> bool {
    let k = n.kind();
    matches!(
        k,
        "local_variable_declaration"
            | "field_declaration"
            | "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration"
            | "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    ) || (k.ends_with("_statement")
        && n.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "block" | "switch_block_statement_group" | "constructor_body" | "program"
            )
        }))
}

fn is_class(n: Node) -> bool {
    matches!(
        n.kind(),
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    )
}

fn is_method(n: Node) -> bool {
    matches!(
        n.kind(),
        "method_declaration" | "constructor_declaration" | "compact_constructor_declaration"
    )
}

fn ancestors(n: Node) -> impl Iterator<Item = Node> {
    std::iter::successors(Some(n), |x| x.parent())
}

/// The `//noinspection` line comment directly above `el`, if any.
fn noinspection_comment<'t>(el: Node<'t>, text: &str) -> Option<Node<'t>> {
    let c = el.prev_sibling().filter(|c| c.kind() == "line_comment")?;
    let between = text.get(c.end_byte()..el.start_byte())?;
    let direct = between.trim().is_empty() && between.matches('\n').count() <= 1;
    (direct && text_of(c, text).starts_with("//noinspection")).then_some(c)
}

fn noinspection_names(comment: Node, text: &str) -> Vec<String> {
    text_of(comment, text)
        .trim_start_matches("//noinspection")
        .split([',', ' ', '\t', '\r', '\n'])
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The `@SuppressWarnings` annotations among the modifiers of `el`.
fn suppress_annotations<'t>(el: Node<'t>, text: &str) -> Vec<Node<'t>> {
    children(el)
        .into_iter()
        .filter(|c| c.kind() == "modifiers")
        .flat_map(children)
        .filter(|a| {
            matches!(a.kind(), "annotation" | "marker_annotation")
                && a.child_by_field_name("name").is_some_and(|n| {
                    matches!(
                        text_of(n, text),
                        "SuppressWarnings" | "java.lang.SuppressWarnings"
                    )
                })
        })
        .collect()
}

/// The string values of an annotation, without quotes.
fn annotation_strings(a: Node, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![a];
    while let Some(x) = stack.pop() {
        if x.kind() == "string_literal" {
            out.push(text_of(x, text).trim_matches('"').to_owned());
            continue;
        }
        stack.extend(children(x));
    }
    out
}

/// The names that `el` suppresses: its `//noinspection` comment and `@SuppressWarnings`.
fn suppressed_names(el: Node, text: &str) -> Vec<String> {
    let mut names: Vec<String> = noinspection_comment(el, text)
        .map(|c| noinspection_names(c, text))
        .unwrap_or_default();
    for a in suppress_annotations(el, text) {
        names.extend(annotation_strings(a, text));
    }
    names
}

/// Whether finding `f` lies in code that suppresses it (`SPEC-0125` v2 behaviour 7). The words
/// of a `//noinspection` comment are no prose either.
pub(crate) fn suppressed(root: Node, text: &str, f: &Finding) -> bool {
    let Some(at) = root.descendant_for_byte_range(f.start, f.end) else {
        return false;
    };
    if f.id.starts_with("grazie.")
        && ancestors(at)
            .any(|n| n.kind() == "line_comment" && text_of(n, text).starts_with("//noinspection"))
    {
        return true;
    }
    ancestors(at).filter(|n| suppressible(*n)).any(|el| {
        suppressed_names(el, text)
            .iter()
            .any(|n| crate::inspections::suppression_matches(n, f.id))
    })
}

/// The name a suppression of `id` at `at` writes: the short name (by the block for
/// `java.empty-block`), else the id.
fn suppression_name<'a>(id: &'a str, at: Node) -> &'a str {
    if id == "java.empty-block" {
        let synchronized = ancestors(at)
            .find(|n| matches!(n.kind(), "synchronized_statement" | "finally_clause"))
            .is_some_and(|n| n.kind() == "synchronized_statement");
        return if synchronized {
            "EmptySynchronizedStatement"
        } else {
            "EmptyFinallyBlock"
        };
    }
    crate::inspections::short_name(id).unwrap_or(id)
}

/// An edit that puts `line` on its own line above `el`, indented like it.
fn insert_above(el: Node, text: &str, line: &str) -> TextEdit {
    let line_start = text[..el.start_byte()].rfind('\n').map_or(0, |i| i + 1);
    let prefix = &text[line_start..el.start_byte()];
    if prefix.trim().is_empty() {
        TextEdit {
            range: line_start..line_start,
            text: format!("{prefix}{line}\n"),
        }
    } else {
        TextEdit {
            range: el.start_byte()..el.start_byte(),
            text: format!("\n{line}\n"),
        }
    }
}

/// Adds `name` to the `@SuppressWarnings` of `el` or puts a new one above it.
fn annotate(el: Node, text: &str, name: &str) -> TextEdit {
    let quoted = format!("\"{name}\"");
    let Some(a) = suppress_annotations(el, text).into_iter().next() else {
        return insert_above(el, text, &format!("@SuppressWarnings({quoted})"));
    };
    let value = a
        .child_by_field_name("arguments")
        .and_then(|args| {
            children(args)
                .into_iter()
                .find(|c| c.is_named() && !c.kind().ends_with("comment"))
        })
        .map(|v| {
            if v.kind() == "element_value_pair" {
                v.child_by_field_name("value").unwrap_or(v)
            } else {
                v
            }
        });
    match value {
        None => TextEdit {
            range: a.byte_range(),
            text: format!("@SuppressWarnings({quoted})"),
        },
        Some(v) if v.kind() == "element_value_array_initializer" => {
            let close = v.end_byte().saturating_sub(1);
            let empty = children(v)
                .iter()
                .all(|c| !c.is_named() || c.kind().ends_with("comment"));
            TextEdit {
                range: close..close,
                text: if empty { quoted } else { format!(", {quoted}") },
            }
        }
        Some(v) => TextEdit {
            range: v.byte_range(),
            text: format!("{{{}, {quoted}}}", text_of(v, text)),
        },
    }
}

/// The edits of "Suppress for statement / method / class" for the finding of `id` at byte `at`
/// (`SPEC-0125` v2 behaviour 8); `None` without such an element or when it suppresses `id`
/// already.
pub(crate) fn suppress_edits(
    root: Node,
    text: &str,
    at: usize,
    id: &str,
    scope: SuppressScope,
) -> Option<Vec<TextEdit>> {
    let node = root.descendant_for_byte_range(at, at)?;
    let name = suppression_name(id, node);
    let el = match scope {
        SuppressScope::Statement => ancestors(node).find(|n| suppressible(*n)),
        SuppressScope::Method => ancestors(node).find(|n| is_method(*n)),
        SuppressScope::Class => ancestors(node).find(|n| is_class(*n)),
        // Java has no file-wide suppression.
        SuppressScope::File => None,
    }?;
    if suppressed_names(el, text)
        .iter()
        .any(|n| crate::inspections::suppression_matches(n, id))
    {
        return None;
    }
    let edit = match scope {
        SuppressScope::Statement => match noinspection_comment(el, text) {
            Some(c) => {
                let end = c.start_byte() + text_of(c, text).trim_end().len();
                TextEdit {
                    range: end..end,
                    text: format!(",{name}"),
                }
            }
            None => insert_above(el, text, &format!("//noinspection {name}")),
        },
        SuppressScope::Method | SuppressScope::Class | SuppressScope::File => {
            annotate(el, text, name)
        }
    };
    Some(vec![edit])
}

/// All Java findings of the tree rooted at `root` over `text`.
pub fn analyze(root: Node, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let (uses, doc) = identifier_uses(root, text);
    let assigned = assigned_names(root, text);
    unused_imports(root, text, &uses, &doc, &mut out);
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "binary_expression" => {
                boolean_literal(n, text, &mut out);
                size_zero(n, text, &mut out);
                reference_equality(n, text, &mut out);
                integer_division(n, text, &mut out);
                array_concatenation(n, text, &mut out);
            }
            "expression_statement" => ignored_result(n, text, &mut out),
            "assignment_expression" => assignment(n, text, &mut out),
            "if_statement" => if_statement(n, text, &mut out),
            "while_statement" => loop_condition(n, &mut out),
            "method_invocation" => {
                invocation(n, text, &mut out);
                typed_invocation(n, text, &mut out);
            }
            "object_creation_expression" => {
                creation(n, text, &mut out);
                crate::java_lambdas::anonymous(n, text, &mut out);
            }
            "lambda_expression" => crate::java_lambdas::lambda(n, text, &mut out),
            "finally_clause" => empty_body(n, "finally", &mut out),
            "synchronized_statement" => empty_body(n, "synchronized", &mut out),
            "class_declaration" | "enum_declaration" | "record_declaration" => {
                class_members(n, text, &uses, &assigned, &mut out)
            }
            "local_variable_declaration" => {
                unused_local(n, text, &mut out);
                null_dereference(n, text, &mut out);
            }
            "method_declaration" | "constructor_declaration" => {
                unused_parameters(n, text, &mut out)
            }
            "cast_expression" => redundant_cast(n, text, &mut out),
            _ => {}
        }
        stack.extend(children(n));
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::LanguageId;
    use crate::inspections::inspect;
    use crate::jvm::apply;
    use crate::syntax::SyntaxDocument;

    fn found(text: &str) -> Vec<(&'static str, String)> {
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        inspect(&doc)
            .into_iter()
            .filter(|f| f.id.starts_with("java."))
            .map(|f| (f.id, text[f.start..f.end].to_owned()))
            .collect()
    }

    fn fixed(text: &str, id: &str) -> String {
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        let f = inspect(&doc).into_iter().find(|f| f.id == id).unwrap();
        apply(text, &f.fix.unwrap().edits)
    }

    #[test]
    fn expressions() {
        let src = "class A { void f(boolean b, java.util.List<String> l, String s) {\n  if (b == true) {}\n  if (false != b) {}\n  if (l.size() == 0) {}\n  if (s.length() > 0) {}\n  if (0 == size()) {}\n  CharSequence cs = s;\n  if (cs.length() == 0) {}\n  Thing t = null;\n  if (t.size() == 0) {}\n  if (l.size() == 1) {}\n  Integer i = new Integer(5);\n  Object o = new Object();\n  try { g(); } catch (Exception e) { e.printStackTrace(); }\n  e.printStackTrace(System.out);\n} }";
        let ids: Vec<_> = found(src)
            .into_iter()
            .map(|(id, t)| format!("{id} {t}"))
            .collect();
        assert_eq!(
            ids,
            [
                "java.boolean-literal-comparison b == true",
                "java.boolean-literal-comparison false != b",
                "java.size-zero l.size() == 0",
                "java.size-zero s.length() > 0",
                "java.null-dereference size",
                "java.unused-local i",
                "java.boxing-constructor new Integer(5)",
                "java.unused-local o",
                "java.print-stack-trace e.printStackTrace()",
            ]
        );
        assert!(fixed(src, "java.boolean-literal-comparison").contains("if (b) {}"));
        assert!(fixed(src, "java.size-zero").contains("if (l.isEmpty()) {}"));
        assert!(fixed(src, "java.boxing-constructor").contains("Integer i = Integer.valueOf(5);"));
        let neg = "class A { void f(boolean b, java.util.List<String> l) { if (b == false) {} if (l.size() != 0) {} } }";
        assert!(fixed(neg, "java.boolean-literal-comparison").contains("if (!b) {}"));
        assert!(fixed(neg, "java.size-zero").contains("if (!l.isEmpty()) {}"));
        let compound = "class A { void f(int a, int b) { if (a > b == false) {} } }";
        assert!(fixed(compound, "java.boolean-literal-comparison").contains("if (!(a > b)) {}"));
    }

    #[test]
    fn statements() {
        let src = "class A {\n  int x;\n  void f(boolean b, Object o) {\n    x = x;\n    this.x = this.x;\n    x = 2;\n    if (b);\n    if (true) {}\n    while (false) {}\n    while (true) { break; }\n    if (b) {} else if (o == null) {} else if (b) {}\n    synchronized (o) {}\n    try { g(); } finally {}\n    try { g(); } finally { h(); }\n  }\n}\n";
        let ids: Vec<_> = found(src)
            .into_iter()
            .map(|(id, t)| format!("{id} {t}"))
            .collect();
        assert_eq!(
            ids,
            [
                "java.self-assignment x = x",
                "java.self-assignment this.x = this.x",
                "java.empty-if if (b)",
                "java.constant-condition (true)",
                "java.constant-condition (false)",
                "java.duplicate-condition (b)",
                "java.empty-block {}",
                "java.empty-block {}",
            ]
        );
        let after = fixed(src, "java.self-assignment");
        assert!(
            after.contains("  void f(boolean b, Object o) {\n    this.x = this.x;\n"),
            "{after}"
        );
    }

    #[test]
    fn unused_declarations() {
        let src = "package p;\n\nimport java.util.List;\nimport java.util.Map;\nimport java.util.Set;\nimport static java.lang.Math.max;\nimport java.util.*;\n\n/** Uses {@link Set}. */\nclass A {\n  private static final long serialVersionUID = 1L;\n  private int unusedField;\n  private int used;\n  @Inject private Object injected;\n  private void helper() {}\n  private void called() {}\n  @Test private void annotated() {}\n  private void readObject(java.io.ObjectInputStream in) {}\n  public void f(List<String> l) {\n    int unused = 1;\n    int a = 2, b = 3;\n    String s = compute();\n    called();\n    used = a;\n    for (int i = 0; i < 1; i++) {}\n  }\n}\n";
        let ids: Vec<_> = found(src)
            .into_iter()
            .map(|(id, t)| format!("{id} {t}"))
            .collect();
        assert_eq!(
            ids,
            [
                "java.unused-import import java.util.Map;",
                "java.unused-import import static java.lang.Math.max;",
                "java.unused-private unusedField",
                "java.unused-private helper",
                "java.unused-local unused",
                "java.unused-local b",
                "java.unused-local s",
            ]
        );
        let after = fixed(src, "java.unused-import");
        assert!(
            after.contains("import java.util.List;\nimport java.util.Set;\n"),
            "{after}"
        );
        let after = fixed(src, "java.unused-local");
        assert!(!after.contains("int unused = 1;") && after.contains("    int a = 2, b = 3;\n"));
        let doc = SyntaxDocument::new(LanguageId::Java, false, src).unwrap();
        let fixes: Vec<_> = inspect(&doc)
            .into_iter()
            .filter(|f| f.id == "java.unused-local")
            .map(|f| f.fix.is_some())
            .collect();
        assert_eq!(
            fixes,
            [true, false, false],
            "several declarators or a call keep the code"
        );
    }

    #[test]
    fn equals_and_hash_code_pair() {
        let one = "class A { public boolean equals(Object o) { return true; } }";
        assert_eq!(found(one), [("java.equals-hash-code", "A".to_owned())]);
        let both = "class A { public boolean equals(Object o) { return true; } public int hashCode() { return 1; } }";
        assert!(found(both).is_empty());
        let other = "class A { public int hashCode() { return 1; } }";
        assert_eq!(found(other)[0].0, "java.equals-hash-code");
    }

    fn typed(text: &str) -> Vec<String> {
        found(text)
            .into_iter()
            .filter(|(id, _)| {
                [
                    "java.string-equality",
                    "java.boxed-equality",
                    "java.ignored-result",
                    "java.equals-incompatible",
                    "java.array-to-string",
                    "java.array-equals",
                    "java.integer-division",
                ]
                .contains(id)
            })
            .map(|(id, t)| format!("{id} {t}"))
            .collect()
    }

    #[test]
    fn type_aware_rules() {
        let src = "import java.math.BigDecimal;\nclass A {\n  double avg(int sum, int n) { return sum / n; }\n  void f(String a, String b, Integer x, Integer y, int i, int[] xs, int[][] grid, char[] cs, BigDecimal m, Object o) {\n    if (a == b) {}\n    if (a != \"q\") {}\n    if (a == null) {}\n    if (o == a) {}\n    if (x == y) {}\n    if (x == i) {}\n    a.trim();\n    m.add(m);\n    a = a.trim();\n    o.toString();\n    a.equals(x);\n    a.equals(\"b\");\n    x.equals(i);\n    a.equals(o);\n    xs.equals(xs);\n    xs.toString();\n    System.out.println(xs);\n    System.out.println(cs);\n    String s = \"v\" + grid;\n    double d = i / 2;\n    double e = i / 2.0;\n    int k = i / 2;\n    var w = a + b;\n    if (w == a) {}\n  }\n}\n";
        assert_eq!(
            typed(src),
            [
                "java.integer-division sum / n",
                "java.string-equality a == b",
                "java.string-equality a != \"q\"",
                "java.boxed-equality x == y",
                "java.ignored-result a.trim()",
                "java.ignored-result m.add(m)",
                "java.equals-incompatible a.equals(x)",
                "java.array-equals xs.equals(xs)",
                "java.array-to-string xs.toString()",
                "java.array-to-string xs",
                "java.array-to-string grid",
                "java.integer-division i / 2",
                "java.string-equality w == a",
            ]
        );
        let fix = |id: &str| fixed(src, id);
        assert!(fix("java.string-equality").contains("if (java.util.Objects.equals(a, b)) {}"));
        assert!(fix("java.ignored-result").contains("    a = a.trim();\n    m.add(m);"));
        assert!(fix("java.integer-division").contains("return (double) sum / n;"));
        assert!(fix("java.array-equals").contains("java.util.Arrays.equals(xs, xs);"));
        assert!(fix("java.array-to-string").contains("java.util.Arrays.toString(xs);"));
        // A literal goes first; imports shorten the helper; `!=` negates.
        let lit = "import java.util.*;\nclass A { void f(String a, String[][] g) { if (a != \"q\") {} String s = \"\" + g; } }";
        assert!(fixed(lit, "java.string-equality").contains("if (!\"q\".equals(a)) {}"));
        assert!(fixed(lit, "java.array-to-string").contains("\"\" + Arrays.deepToString(g)"));
        // Unknown types stay quiet.
        let unknown = "class A { void f(Thing a, Thing b, String x, String y) { if (a == b) {} a.trim(); double d = a.n / b.n; if (x == y) { // fast path\n return 0; } } }";
        assert!(typed(unknown).is_empty(), "{:?}", typed(unknown));
    }

    fn rule(text: &str, id: &str) -> Vec<String> {
        found(text)
            .into_iter()
            .filter(|(i, _)| *i == id)
            .map(|(_, t)| t)
            .collect()
    }

    /// SPEC-0125/AK-5: the v2 rules report their case and stay quiet on the near miss.
    #[test]
    fn redundant_cast() {
        let src = "class A {\n  Object field;\n  void f(String s, Object o, java.util.List<String> l) {\n    use((String) s);\n    use((String) o);\n    use((Object) field);\n    use((java.util.List<String>) l);\n    use((String) s.trim());\n    for (Integer i : xs) { use((Integer) i); }\n    xs.forEach(s -> use((String) s));\n  }\n}\n";
        assert_eq!(
            rule(src, "java.redundant-cast"),
            [
                "(String) s",
                "(Object) field",
                "(java.util.List<String>) l",
                "(Integer) i"
            ]
        );
        assert!(fixed(src, "java.redundant-cast").contains("    use(s);\n    use((String) o);"));
        let doc = SyntaxDocument::new(LanguageId::Java, false, src).unwrap();
        let f = inspect(&doc)
            .into_iter()
            .find(|f| f.id == "java.redundant-cast")
            .unwrap();
        assert_eq!(f.fix.unwrap().title, "Remove redundant cast");
    }

    #[test]
    fn field_may_be_final() {
        let src = "class A {\n  private int limit = 10;\n  private int count = 0;\n  private int hits = 0;\n  private int later;\n  private final int done = 1;\n  private volatile int flag = 0;\n  int open = 1;\n  private int a = 1, b = 2;\n  private int unused = 3;\n  void f() { count = 2; this.hits++; use(limit + flag + open + a + b + later + done); }\n}\n";
        assert_eq!(rule(src, "java.field-may-be-final"), ["limit", "a", "b"]);
        assert!(found(src).contains(&("java.unused-private", "unused".to_owned())));
        let after = fixed(src, "java.field-may-be-final");
        assert!(
            after.contains("  private final int limit = 10;\n"),
            "{after}"
        );
        let compound = "class A { private int n = 0; void f() { n += 1; } int g() { return n; } }";
        assert!(rule(compound, "java.field-may-be-final").is_empty());
    }

    #[test]
    fn unused_parameter() {
        let src = "class A {\n  private int twice(int x, int unused) { return 2 * x; }\n  public int open(int ignored) { return 1; }\n  private A(String name, String... rest) { use(name); }\n  private void readObject(java.io.ObjectInputStream in) {}\n  private int g(int x) { return twice(x, 0) + new A(\"a\").hashCode(); }\n}\n";
        assert_eq!(rule(src, "java.unused-parameter"), ["unused", "rest"]);
        let doc = SyntaxDocument::new(LanguageId::Java, false, src).unwrap();
        let f = inspect(&doc)
            .into_iter()
            .find(|f| f.id == "java.unused-parameter")
            .unwrap();
        assert_eq!(f.level, crate::inspections::Level::Weak);
        assert!(f.fix.is_none());
        assert!(crate::inspections::is_unused(f.id));
    }

    #[test]
    fn null_dereference() {
        let src = "class A {\n  void f(boolean c) {\n    String s = null;\n    int n = s.length();\n    String t = c ? \"x\" : null;\n    use(t.length);\n    String u = null;\n    if (u != null) { u.trim(); }\n    String v = null;\n    v = \"x\";\n    v.trim();\n    String w = \"x\";\n    w.trim();\n    String x = null;\n    use(x);\n    x.trim();\n  }\n}\n";
        assert_eq!(rule(src, "java.null-dereference"), ["length", "length"]);
        let doc = SyntaxDocument::new(LanguageId::Java, false, src).unwrap();
        let messages: Vec<_> = inspect(&doc)
            .into_iter()
            .filter(|f| f.id == "java.null-dereference")
            .map(|f| (f.level, f.message, f.fix.is_none()))
            .collect();
        assert_eq!(
            messages,
            [
                (
                    crate::inspections::Level::Warning,
                    "'s' is null here: calling 'length()' throws NullPointerException".to_owned(),
                    true
                ),
                (
                    crate::inspections::Level::Warning,
                    "'t' may be null here: reading 'length' may throw NullPointerException"
                        .to_owned(),
                    true
                ),
            ]
        );
    }

    /// `java.unused-local` covers exactly the variable's name.
    #[test]
    fn unused_local_reports_the_name() {
        let src = "class A { void f() { int counter = 1; } }";
        let doc = SyntaxDocument::new(LanguageId::Java, false, src).unwrap();
        let f = inspect(&doc)
            .into_iter()
            .find(|f| f.id == "java.unused-local")
            .unwrap();
        assert_eq!(&src[f.start..f.end], "counter");
    }

    /// SPEC-0125/AK-5: suppression by comment and annotation, by id, short name, `unused`, `ALL`.
    #[test]
    fn suppression() {
        let quiet = |src: &str, id: &str| rule(src, id).is_empty();
        let base = "class A {\n  void f(String s) {\n    if (s == \"x\") {}\n  }\n}\n";
        assert!(!quiet(base, "java.string-equality"));
        for name in ["StringEquality", "java.string-equality", "ALL"] {
            let comment = base.replace(
                "    if (s",
                &format!("    //noinspection {name}\n    if (s"),
            );
            assert!(quiet(&comment, "java.string-equality"), "{comment}");
            let method = base.replace(
                "  void f",
                &format!("  @SuppressWarnings(\"{name}\")\n  void f"),
            );
            assert!(quiet(&method, "java.string-equality"), "{method}");
        }
        let list = base.replace(
            "class A",
            "@SuppressWarnings({\"unused\", \"StringEquality\"})\nclass A",
        );
        assert!(quiet(&list, "java.string-equality"));
        // Another name, or a comment that is not directly above, does not suppress.
        let other = base.replace("    if (s", "    //noinspection RedundantCast\n    if (s");
        assert!(!quiet(&other, "java.string-equality"));
        let far = base.replace(
            "    if (s",
            "    //noinspection StringEquality\n\n    if (s",
        );
        assert!(!quiet(&far, "java.string-equality"));
        // `unused` covers the unused rules; the comment itself is no prose.
        let unused = "import java.util.Map;\nclass A {\n  //noinspection unused\n  private int x;\n  void f() {\n    @SuppressWarnings(\"unused\") int y = 1;\n    //noinspection unused\n    int z = 2;\n  }\n}\n";
        let ids: Vec<_> = found(unused).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["java.unused-import"]);
        let doc = SyntaxDocument::new(LanguageId::Java, false, unused).unwrap();
        assert!(inspect(&doc).iter().all(|f| !f.id.starts_with("grazie.")));
        // A field annotation suppresses inside the field only.
        let field = "class A {\n  @SuppressWarnings(\"FieldMayBeFinal\") private int a = 1;\n  private int b = 2;\n  int g() { return a + b; }\n}\n";
        assert_eq!(rule(field, "java.field-may-be-final"), ["b"]);
    }

    fn suppressed_text(src: &str, id: &str, scope: crate::inspections::SuppressScope) -> String {
        let doc = SyntaxDocument::new(LanguageId::Java, false, src).unwrap();
        let f = inspect(&doc).into_iter().find(|f| f.id == id).unwrap();
        let edits = crate::inspections::suppress(src, f.start, id, scope).unwrap();
        let after = apply(src, &edits);
        let doc = SyntaxDocument::new(LanguageId::Java, false, &after).unwrap();
        assert!(
            inspect(&doc).iter().all(|g| g.id != id),
            "still reported: {after}"
        );
        after
    }

    /// SPEC-0125/AK-5: the suppression edits for statement, method (with and without an existing
    /// annotation) and class.
    #[test]
    fn suppression_edits() {
        use crate::inspections::SuppressScope::{Class, Method, Statement};
        let src = "class A {\n    void f(String s) {\n        if (s == \"x\") {}\n    }\n}\n";
        assert_eq!(
            suppressed_text(src, "java.string-equality", Statement),
            "class A {\n    void f(String s) {\n        //noinspection StringEquality\n        if (s == \"x\") {}\n    }\n}\n"
        );
        assert_eq!(
            suppressed_text(src, "java.string-equality", Method),
            "class A {\n    @SuppressWarnings(\"StringEquality\")\n    void f(String s) {\n        if (s == \"x\") {}\n    }\n}\n"
        );
        assert_eq!(
            suppressed_text(src, "java.string-equality", Class),
            "@SuppressWarnings(\"StringEquality\")\nclass A {\n    void f(String s) {\n        if (s == \"x\") {}\n    }\n}\n"
        );
        let one = src.replace(
            "    void f",
            "    @SuppressWarnings(\"unchecked\")\n    void f",
        );
        assert!(
            suppressed_text(&one, "java.string-equality", Method)
                .contains("    @SuppressWarnings({\"unchecked\", \"StringEquality\"})\n    void f")
        );
        let list = src.replace(
            "    void f",
            "    @SuppressWarnings({\"a\", \"b\"})\n    void f",
        );
        assert!(
            suppressed_text(&list, "java.string-equality", Method)
                .contains("@SuppressWarnings({\"a\", \"b\", \"StringEquality\"})")
        );
        let comment = src.replace(
            "        if (s",
            "        //noinspection RedundantCast\n        if (s",
        );
        assert!(
            suppressed_text(&comment, "java.string-equality", Statement)
                .contains("        //noinspection RedundantCast,StringEquality\n        if (s")
        );
        // Without a short name the id is used; a field has no method around it.
        let typed = "class A {\n  void f(Integer a, Integer b) {\n    if (a == b) {}\n  }\n}\n";
        assert!(
            suppressed_text(typed, "java.boxed-equality", Statement)
                .contains("    //noinspection java.boxed-equality\n")
        );
        let field = "class A {\n  private int a = 1;\n  int g() { return a; }\n}\n";
        let at = field.find("a = 1").unwrap();
        assert!(
            crate::inspections::suppress(field, at, "java.field-may-be-final", Method).is_none()
        );
        let sync = "class A { void f(Object o) {\n  synchronized (o) {}\n} }";
        assert!(
            suppressed_text(sync, "java.empty-block", Statement)
                .contains("  //noinspection EmptySynchronizedStatement\n  synchronized")
        );
    }

    /// SPEC-0125/AK-5: "fix all" over two findings.
    #[test]
    fn fix_all_in_file() {
        let src = "class A {\n  void f(boolean a, boolean b) {\n    if (a == true) {}\n    if (b != false) {}\n    if (a) {}\n  }\n}\n";
        let path = std::path::Path::new("A.java");
        let edits = crate::inspections::fix_all(path, src, "java.boolean-literal-comparison");
        assert_eq!(edits.len(), 2);
        assert!(edits.windows(2).all(|w| w[0].range.end <= w[1].range.start));
        assert_eq!(
            apply(src, &edits),
            "class A {\n  void f(boolean a, boolean b) {\n    if (a) {}\n    if (b) {}\n    if (a) {}\n  }\n}\n"
        );
        // Two fields of one declaration share one edit, applied once.
        let pair = "class A { private int a = 1, b = 2; int g() { return a + b; } }";
        let edits = crate::inspections::fix_all(path, pair, "java.field-may-be-final");
        assert_eq!(edits.len(), 1);
        assert!(apply(pair, &edits).contains("private final int a = 1, b = 2;"));
        assert!(crate::inspections::fix_all(path, src, "java.empty-if").is_empty());
    }

    /// SPEC-0125/AK-8: "Suppress all inspections for class" inserts or extends the annotation with
    /// `ALL`, and nothing is reported in the class afterwards.
    #[test]
    fn suppress_all_for_class() {
        let src = "class A {\n  void f(boolean b) {\n    if (b == true) {}\n  }\n}\n";
        let path = std::path::Path::new("A.java");
        let at = src.find("==").unwrap();
        let edits = crate::inspections::suppress_all(src, at).unwrap();
        let done = apply(src, &edits);
        assert!(
            done.starts_with("@SuppressWarnings(\"ALL\")\nclass A {"),
            "{done}"
        );
        assert!(crate::inspections::inspect_text(path, &done).is_empty());
        let at = done.find("==").unwrap();
        assert!(
            crate::inspections::suppress_all(&done, at).is_none(),
            "already ALL"
        );
        let named = "@SuppressWarnings(\"unused\")\nclass A {\n  void f(boolean b) {\n    if (b == true) {}\n  }\n}\n";
        let at = named.find("==").unwrap();
        let extended = apply(named, &crate::inspections::suppress_all(named, at).unwrap());
        assert!(
            extended.starts_with("@SuppressWarnings({\"unused\", \"ALL\"})"),
            "{extended}"
        );
    }
}
