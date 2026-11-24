//! Kotlin inspections with types (`SPEC-0140`): a small type model on the syntax tree — literals,
//! parameters (`vararg` as arrays), properties with a declared type or an initializer, `this.x`,
//! constructor and array-factory calls, well-known methods of `String`, numbers, `BigDecimal` and
//! `BigInteger` — and the rules that need it. Rules stay quiet wherever a type is unknown.
//! Since v2 also the unused private declarations and parameters of a file, by name.

use tree_sitter::Node;

use crate::inspections::{Finding, Fix, Level};
use crate::jvm::TextEdit;

/// Ids and display names of the Kotlin rules with types.
pub const KOTLIN_RULES: &[(&str, &str)] = &[
    ("kotlin.ignored-result", "Result of method call ignored"),
    ("kotlin.identity-equality", "Identity comparison of values"),
    ("kotlin.array-equality", "Arrays compared with '=='"),
    ("kotlin.array-to-string", "Array printed as reference"),
    (
        "kotlin.equals-incompatible",
        "equals() between unrelated types",
    ),
    (
        "kotlin.integer-division",
        "Integer division converted to floating point",
    ),
    ("kotlin.unused-private", "Unused private declaration"),
    ("kotlin.unused-parameter", "Unused parameter"),
    // The unused declarations of the project's reference index (SPEC-0316 v4 behaviour 9).
    ("kotlin.unused-declaration", "Unused declaration"),
];

/// Immutable types whose methods return a new value, and those methods.
const IMMUTABLE: &[(&str, &[&str])] = &[
    (
        "String",
        &[
            "trim",
            "trimStart",
            "trimEnd",
            "trimIndent",
            "trimMargin",
            "uppercase",
            "lowercase",
            "capitalize",
            "decapitalize",
            "replace",
            "replaceFirst",
            "substring",
            "plus",
            "padStart",
            "padEnd",
            "removePrefix",
            "removeSuffix",
            "removeSurrounding",
            "reversed",
            "repeat",
            "drop",
            "dropLast",
            "take",
            "takeLast",
        ],
    ),
    (
        "BigDecimal",
        &[
            "add",
            "subtract",
            "multiply",
            "divide",
            "remainder",
            "negate",
            "abs",
            "pow",
            "setScale",
            "plus",
            "minus",
            "times",
            "div",
            "rem",
        ],
    ),
    (
        "BigInteger",
        &[
            "add",
            "subtract",
            "multiply",
            "divide",
            "mod",
            "remainder",
            "negate",
            "abs",
            "pow",
            "shiftLeft",
            "shiftRight",
            "plus",
            "minus",
            "times",
            "div",
            "rem",
        ],
    ),
];

/// Value types whose `equals` holds only for the same type.
const VALUE_TYPES: &[&str] = &[
    "String",
    "Int",
    "Long",
    "Short",
    "Byte",
    "Double",
    "Float",
    "Char",
    "Boolean",
    "BigDecimal",
    "BigInteger",
    "StringBuilder",
];

/// Primitive array types and their factories (`intArrayOf`, `IntArray(n)`).
const PRIMITIVE_ARRAYS: &[&str] = &[
    "IntArray",
    "LongArray",
    "ShortArray",
    "ByteArray",
    "DoubleArray",
    "FloatArray",
    "CharArray",
    "BooleanArray",
];

fn text_of<'a>(n: Node, text: &'a str) -> &'a str {
    &text[n.byte_range()]
}

fn children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

fn named(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c)
        .filter(|c| !c.kind().ends_with("comment"))
        .collect()
}

fn is_array(ty: &str) -> bool {
    ty == "Array" || ty.starts_with("Array<") || PRIMITIVE_ARRAYS.contains(&ty)
}

fn is_integral(ty: &str) -> bool {
    matches!(ty, "Int" | "Long" | "Short" | "Byte")
}

fn is_numeric(ty: &str) -> bool {
    is_integral(ty) || matches!(ty, "Double" | "Float")
}

/// A written type as the model compares it: `kotlin.String?` → `String`, `Array<IntArray>` kept
/// with its argument (nested arrays print differently), other generics dropped.
fn written_type(n: Node, text: &str) -> Option<String> {
    let n = match n.kind() {
        "nullable_type" => named(n).into_iter().next()?,
        _ => n,
    };
    if n.kind() != "user_type" {
        return None;
    }
    let parts = named(n);
    let name = parts
        .iter()
        .rfind(|p| p.kind() == "identifier")
        .map(|p| text_of(*p, text))?;
    if name == "Array" {
        let arg = parts
            .iter()
            .find(|p| p.kind() == "type_arguments")
            .and_then(|a| named(*a).into_iter().next())
            .and_then(|p| named(p).into_iter().next())
            .and_then(|t| written_type(t, text));
        return Some(match arg {
            Some(a) => format!("Array<{a}>"),
            None => "Array".into(),
        });
    }
    Some(name.to_owned())
}

/// A declaration of `wanted` seen from `at`: its type and whether it is a `var`.
fn declaration(at: Node, wanted: &str, text: &str) -> Option<(Option<String>, bool)> {
    let property = |p: Node| -> Option<(Option<String>, bool)> {
        let kids = children(p);
        let mutable = kids.iter().any(|k| k.kind() == "var");
        let decl = kids.iter().find(|k| k.kind() == "variable_declaration")?;
        let parts = named(*decl);
        let name = parts.iter().find(|x| x.kind() == "identifier")?;
        if text_of(*name, text) != wanted {
            return None;
        }
        let declared = parts.iter().find_map(|x| written_type(*x, text));
        let ty = declared.or_else(|| {
            let eq = kids.iter().position(|k| k.kind() == "=")?;
            kids.get(eq + 1..)?
                .iter()
                .find(|k| k.is_named())
                .and_then(|v| type_of(*v, text))
        });
        Some((ty, mutable))
    };
    let parameter = |p: Node, vararg: bool| -> Option<(Option<String>, bool)> {
        let parts = named(p);
        let name = parts.iter().find(|x| x.kind() == "identifier")?;
        if text_of(*name, text) != wanted {
            return None;
        }
        let ty = parts.iter().find_map(|x| written_type(*x, text));
        let ty = if vararg {
            ty.map(|t| match t.as_str() {
                "Int" | "Long" | "Short" | "Byte" | "Double" | "Float" | "Char" | "Boolean" => {
                    format!("{t}Array")
                }
                _ => format!("Array<{t}>"),
            })
        } else {
            ty
        };
        let mutable = children(p).iter().any(|k| k.kind() == "var");
        Some((ty, mutable))
    };
    let mut scope = at.parent();
    while let Some(s) = scope {
        let kids = children(s);
        for (i, child) in kids.iter().enumerate() {
            let found = match child.kind() {
                "function_value_parameters" | "class_parameters" => {
                    let list = children(*child);
                    list.iter().enumerate().find_map(|(j, p)| {
                        if !matches!(p.kind(), "parameter" | "class_parameter") {
                            return None;
                        }
                        let vararg = j > 0
                            && list[j - 1].kind() == "parameter_modifiers"
                            && text_of(list[j - 1], text).contains("vararg");
                        parameter(*p, vararg)
                    })
                }
                "primary_constructor" => named(*child)
                    .into_iter()
                    .find(|c| c.kind() == "class_parameters")
                    .and_then(|cp| {
                        children(cp)
                            .into_iter()
                            .filter(|p| p.kind() == "class_parameter")
                            .find_map(|p| parameter(p, false))
                    }),
                "property_declaration" => {
                    // Locals only before the use; class properties anywhere.
                    let local = s.kind() != "class_body";
                    if local && child.start_byte() >= at.start_byte() {
                        None
                    } else {
                        property(*child)
                    }
                }
                _ => None,
            };
            let _ = i;
            if found.is_some() {
                return found;
            }
        }
        scope = s.parent();
    }
    None
}

fn class_body(n: Node) -> Option<Node> {
    let mut at = n.parent();
    while let Some(p) = at {
        if p.kind() == "class_declaration" || p.kind() == "object_declaration" {
            return Some(p);
        }
        at = p.parent();
    }
    None
}

/// The receiver and name of a call `x.f(…)`, or the callee name of `f(…)`.
fn callee<'t>(call: Node<'t>, text: &'t str) -> Option<(Option<Node<'t>>, &'t str)> {
    let target = named(call).into_iter().next()?;
    match target.kind() {
        "identifier" => Some((None, text_of(target, text))),
        "navigation_expression" => {
            let parts = named(target);
            let name = parts.last().filter(|p| p.kind() == "identifier")?;
            Some((parts.first().copied(), text_of(*name, text)))
        }
        _ => None,
    }
}

/// The type of an expression as far as the model knows it.
pub fn type_of(n: Node, text: &str) -> Option<String> {
    let t = text_of(n, text);
    Some(match n.kind() {
        "string_literal" | "multiline_string_literal" => "String".into(),
        "character_literal" => "Char".into(),
        "boolean_literal" => "Boolean".into(),
        "null" | "null_literal" => "null".into(),
        "number_literal" => {
            if t.ends_with(['u', 'U']) || t.ends_with("uL") || t.ends_with("UL") {
                return None;
            } else if t.ends_with('L') {
                "Long".into()
            } else {
                "Int".into()
            }
        }
        "float_literal" => {
            if t.ends_with(['f', 'F']) {
                "Float".into()
            } else {
                "Double".into()
            }
        }
        "parenthesized_expression" => type_of(named(n).into_iter().next()?, text)?,
        "identifier" => declaration(n, t, text)?.0?,
        "navigation_expression" => {
            let parts = named(n);
            let (receiver, name) = (parts.first()?, parts.last()?);
            let name = text_of(*name, text);
            if receiver.kind() == "this_expression" {
                let class = class_body(n)?;
                // A property of this class: look it up from inside the class body.
                let body = named(class).into_iter().find(|c| c.kind() == "class_body");
                let probe = body
                    .and_then(|b| named(b).into_iter().last())
                    .unwrap_or(class);
                return declaration(probe, name, text)
                    .or_else(|| declaration(class, name, text))?
                    .0;
            }
            let rt = type_of(*receiver, text)?;
            match name {
                "length" if rt == "String" => "Int".into(),
                "size" if is_array(&rt) => "Int".into(),
                _ => return None,
            }
        }
        "call_expression" => {
            let (receiver, name) = callee(n, text)?;
            match receiver {
                None => match name {
                    "arrayOf" | "arrayOfNulls" | "emptyArray" => "Array".into(),
                    "intArrayOf" => "IntArray".into(),
                    "longArrayOf" => "LongArray".into(),
                    "doubleArrayOf" => "DoubleArray".into(),
                    "floatArrayOf" => "FloatArray".into(),
                    "charArrayOf" => "CharArray".into(),
                    "booleanArrayOf" => "BooleanArray".into(),
                    "byteArrayOf" => "ByteArray".into(),
                    "shortArrayOf" => "ShortArray".into(),
                    a if PRIMITIVE_ARRAYS.contains(&a) => a.into(),
                    "BigDecimal" | "BigInteger" | "StringBuilder" | "String" => name.into(),
                    _ => return None,
                },
                Some(r) => {
                    let rt = type_of(r, text)?;
                    if let Some((ty, _)) = IMMUTABLE
                        .iter()
                        .find(|(ty, methods)| *ty == rt && methods.contains(&name))
                    {
                        return Some((*ty).into());
                    }
                    match name {
                        "toString" | "contentToString" | "contentDeepToString" => "String".into(),
                        "toInt" => "Int".into(),
                        "toLong" => "Long".into(),
                        "toDouble" => "Double".into(),
                        "toFloat" => "Float".into(),
                        "equals" | "contentEquals" | "isEmpty" | "isBlank" | "startsWith"
                        | "endsWith" | "contains" => "Boolean".into(),
                        _ => return None,
                    }
                }
            }
        }
        "binary_expression" => {
            let op = text_of(n.child_by_field_name("operator")?, text);
            let l = n.child_by_field_name("left")?;
            let r = n.child_by_field_name("right")?;
            match op {
                "==" | "!=" | "===" | "!==" | "<" | ">" | "<=" | ">=" | "&&" | "||" => {
                    "Boolean".into()
                }
                "+" if type_of(l, text).as_deref() == Some("String") => "String".into(),
                "+" | "-" | "*" | "/" | "%" => {
                    let (a, b) = (type_of(l, text)?, type_of(r, text)?);
                    if !is_numeric(&a) || !is_numeric(&b) {
                        return None;
                    }
                    let rank = |t: &str| match t {
                        "Double" => 4,
                        "Float" => 3,
                        "Long" => 2,
                        _ => 1,
                    };
                    match rank(&a).max(rank(&b)) {
                        4 => "Double",
                        3 => "Float",
                        2 => "Long",
                        _ => "Int",
                    }
                    .into()
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn finding(id: &'static str, n: Node, message: String, fix: Option<Fix>) -> Finding {
    Finding {
        id,
        start: n.start_byte(),
        end: n.end_byte(),
        level: Level::Warning,
        message,
        fix,
    }
}

fn replace(title: String, range: std::ops::Range<usize>, with: String) -> Option<Fix> {
    Some(Fix {
        learn: None,
        keep: None,
        bundle_key: None,
        action: None,
        title,
        edits: vec![TextEdit { range, text: with }],
    })
}

/// `arr.contentToString()` or `contentDeepToString()` for arrays of arrays.
fn content_string(expr: &str, ty: &str) -> String {
    let deep = ty.starts_with("Array<") && is_array(&ty[6..ty.len() - 1]);
    let f = if deep {
        "contentDeepToString"
    } else {
        "contentToString"
    };
    let needs_parens = !expr
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'));
    if needs_parens {
        format!("({expr}).{f}()")
    } else {
        format!("{expr}.{f}()")
    }
}

/// Whether a statement's value is used: the last statement of a block that is not a function's
/// body (an `if`/`when` branch or a lambda gives its last expression as value).
fn value_used(statement: Node) -> bool {
    let Some(parent) = statement.parent() else {
        return true;
    };
    match parent.kind() {
        "block" => {
            let last = named(parent).last().map(|l| l.id()) == Some(statement.id());
            let function = parent.parent().is_some_and(|p| p.kind() == "function_body");
            last && !function
        }
        "statements" | "source_file" | "class_body" => {
            // Lambda bodies hold `statements`; their last one is the value.
            let last = named(parent).last().map(|l| l.id()) == Some(statement.id());
            last && parent
                .parent()
                .is_some_and(|p| p.kind() == "lambda_literal")
        }
        _ => true,
    }
}

fn ignored_result(call: Node, text: &str, out: &mut Vec<Finding>) {
    if value_used(call) {
        return;
    }
    let Some((Some(receiver), name)) = callee(call, text) else {
        return;
    };
    // `x?.trim()` is a navigation with a safe call; the type is still the receiver's.
    let Some(rt) = type_of(receiver, text) else {
        return;
    };
    if !IMMUTABLE
        .iter()
        .any(|(ty, methods)| *ty == rt && methods.contains(&name))
    {
        return;
    }
    let fix = (receiver.kind() == "identifier")
        .then(|| text_of(receiver, text))
        .filter(|var| declaration(receiver, var, text).is_some_and(|(_, mutable)| mutable))
        .and_then(|var| {
            replace(
                format!("Assign the result to '{var}'"),
                call.start_byte()..call.start_byte(),
                format!("{var} = "),
            )
        });
    out.push(finding(
        "kotlin.ignored-result",
        call,
        format!("Result of '{rt}.{name}()' is ignored: {rt} values do not change"),
        fix,
    ));
}

fn binary(n: Node, text: &str, out: &mut Vec<Finding>) {
    let (Some(l), Some(op), Some(r)) = (
        n.child_by_field_name("left"),
        n.child_by_field_name("operator"),
        n.child_by_field_name("right"),
    ) else {
        return;
    };
    let op_text = text_of(op, text);
    let (lt, rt) = (type_of(l, text), type_of(r, text));
    match op_text {
        "===" | "!==" => {
            let (Some(a), Some(b)) = (lt, rt) else {
                return;
            };
            let values = (a == "String" && b == "String")
                || (is_numeric(&a) && is_numeric(&b))
                || (a == b && matches!(a.as_str(), "Char" | "Boolean"));
            if values {
                let with = if op_text == "===" { "==" } else { "!=" };
                out.push(finding(
                    "kotlin.identity-equality",
                    n,
                    format!("'{op_text}' compares the identity of {a} values, not their value"),
                    replace(
                        format!("Replace with '{with}'"),
                        op.byte_range(),
                        with.into(),
                    ),
                ));
            }
        }
        "==" | "!=" => {
            let (Some(a), Some(b)) = (lt, rt) else {
                return;
            };
            if is_array(&a) && is_array(&b) {
                let call = format!("{}.contentEquals({})", text_of(l, text), text_of(r, text));
                let with = if op_text == "==" {
                    call
                } else {
                    format!("!{call}")
                };
                out.push(finding(
                    "kotlin.array-equality",
                    n,
                    "Arrays compared with '==' compare references, not contents".into(),
                    replace(
                        format!("Replace with '{with}'"),
                        n.byte_range(),
                        with.clone(),
                    ),
                ));
            }
        }
        "+" if lt.as_deref() == Some("String") => {
            if let Some(ty) = rt.filter(|t| is_array(t)) {
                let with = content_string(text_of(r, text), &ty);
                out.push(finding(
                    "kotlin.array-to-string",
                    r,
                    format!("The array is concatenated as its type and hash; use '{with}'"),
                    replace(
                        format!("Replace with '{with}'"),
                        r.byte_range(),
                        with.clone(),
                    ),
                ));
            }
        }
        _ => {}
    }
}

fn call(n: Node, text: &str, out: &mut Vec<Finding>) {
    let Some((receiver, name)) = callee(n, text) else {
        return;
    };
    let args: Vec<Node> = named(n)
        .into_iter()
        .find(|c| c.kind() == "value_arguments")
        .map(|a| {
            named(a)
                .into_iter()
                .filter_map(|v| named(v).into_iter().last())
                .collect()
        })
        .unwrap_or_default();
    match (receiver, name, args.as_slice()) {
        (Some(r), "equals", [arg]) => {
            let (Some(a), Some(b)) = (type_of(r, text), type_of(*arg, text)) else {
                return;
            };
            if is_array(&a) && is_array(&b) {
                let with = format!(
                    "{}.contentEquals({})",
                    text_of(r, text),
                    text_of(*arg, text)
                );
                out.push(finding(
                    "kotlin.array-equality",
                    n,
                    "equals() on an array compares references, not contents".into(),
                    replace(
                        format!("Replace with '{with}'"),
                        n.byte_range(),
                        with.clone(),
                    ),
                ));
            } else if a != b
                && VALUE_TYPES.contains(&a.as_str())
                && VALUE_TYPES.contains(&b.as_str())
            {
                out.push(finding(
                    "kotlin.equals-incompatible",
                    n,
                    format!("'equals()' between {a} and {b} is always false"),
                    None,
                ));
            }
        }
        (Some(r), "toString", []) => {
            if let Some(ty) = type_of(r, text).filter(|t| is_array(t)) {
                let with = content_string(text_of(r, text), &ty);
                out.push(finding(
                    "kotlin.array-to-string",
                    n,
                    format!("'toString()' on an array gives its type and hash; use '{with}'"),
                    replace(
                        format!("Replace with '{with}'"),
                        n.byte_range(),
                        with.clone(),
                    ),
                ));
            }
        }
        // `println(CharArray)` prints the characters.
        (None, "println" | "print", [arg]) => {
            if let Some(ty) = type_of(*arg, text).filter(|t| is_array(t) && t != "CharArray") {
                let with = content_string(text_of(*arg, text), &ty);
                out.push(finding(
                    "kotlin.array-to-string",
                    *arg,
                    format!("The array is printed as its type and hash; use '{with}'"),
                    replace(
                        format!("Replace with '{with}'"),
                        arg.byte_range(),
                        with.clone(),
                    ),
                ));
            }
        }
        (Some(r), "toDouble" | "toFloat", []) => {
            let mut inner = r;
            while inner.kind() == "parenthesized_expression" {
                match named(inner).into_iter().next() {
                    Some(i) => inner = i,
                    None => return,
                }
            }
            if inner.kind() != "binary_expression"
                || inner
                    .child_by_field_name("operator")
                    .is_none_or(|o| text_of(o, text) != "/")
            {
                return;
            }
            let (Some(a), Some(b)) = (
                inner.child_by_field_name("left"),
                inner.child_by_field_name("right"),
            ) else {
                return;
            };
            let integral = |x: Node| type_of(x, text).is_some_and(|t| is_integral(&t));
            if !integral(a) || !integral(b) {
                return;
            }
            let lhs = text_of(a, text);
            let lhs = if lhs
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                lhs.to_owned()
            } else {
                format!("({lhs})")
            };
            let with = format!("{lhs}.{name}() / {}", text_of(b, text));
            out.push(finding(
                "kotlin.integer-division",
                n,
                format!("Integer division before '{name}()': the fraction is lost"),
                replace(
                    format!("Replace with '{with}'"),
                    n.byte_range(),
                    with.clone(),
                ),
            ));
        }
        _ => {}
    }
}

/// `"… $arr …"` and `"… ${arr} …"` with an array.
fn template(n: Node, text: &str, out: &mut Vec<Finding>) {
    let kids = children(n);
    for (i, k) in kids.iter().enumerate() {
        // `${arr}`
        if k.kind() == "interpolation" {
            if let Some(e) = named(*k).into_iter().next()
                && let Some(ty) = type_of(e, text).filter(|t| is_array(t))
            {
                let with = format!("${{{}}}", content_string(text_of(e, text), &ty));
                out.push(finding(
                    "kotlin.array-to-string",
                    *k,
                    "The array is put into the string as its type and hash".into(),
                    replace(
                        format!("Replace with '{with}'"),
                        k.byte_range(),
                        with.clone(),
                    ),
                ));
            }
            continue;
        }
        // `$arr`: a `$` content node followed by content that starts with the name.
        if k.kind() == "string_content" && text_of(*k, text) == "$" {
            let Some(next) = kids.get(i + 1).filter(|x| x.kind() == "string_content") else {
                continue;
            };
            let rest = text_of(*next, text);
            let len = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let name = &rest[..len];
            if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let Some(ty) = declaration(n, name, text)
                .and_then(|d| d.0)
                .filter(|t| is_array(t))
            else {
                continue;
            };
            let range = k.start_byte()..next.start_byte() + len;
            let with = format!("${{{}}}", content_string(name, &ty));
            out.push(Finding {
                id: "kotlin.array-to-string",
                start: range.start,
                end: range.end,
                level: Level::Warning,
                message: "The array is put into the string as its type and hash".into(),
                fix: replace(format!("Replace with '{with}'"), range, with.clone()),
            });
        }
    }
}

/// Methods the Java serialization calls by name.
const SERIALIZATION: &[&str] = &[
    "readObject",
    "writeObject",
    "readResolve",
    "writeReplace",
    "readObjectNoData",
];

/// The modifier words of a declaration and whether it carries an annotation.
fn modifiers(n: Node, text: &str) -> (Vec<String>, bool) {
    let Some(m) = children(n).into_iter().find(|c| c.kind() == "modifiers") else {
        return (Vec::new(), false);
    };
    let parts = children(m);
    let annotated = parts.iter().any(|c| c.kind().contains("annotation"));
    let words = parts
        .iter()
        .filter(|c| !c.kind().contains("annotation"))
        .map(|c| text_of(*c, text).trim().to_owned())
        .collect();
    (words, annotated)
}

/// Counts every name written below `n`: identifiers, and `$name` in string templates.
fn count_names<'a>(n: Node, text: &'a str, names: &mut std::collections::HashMap<&'a str, usize>) {
    let mut stack = vec![n];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "identifier" => *names.entry(text_of(n, text)).or_default() += 1,
            "string_literal" => {
                let s = text_of(n, text);
                let mut rest = s;
                while let Some(i) = rest.find('$') {
                    rest = &rest[i + 1..];
                    let end = rest
                        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .unwrap_or(rest.len());
                    if end > 0 {
                        *names.entry(&rest[..end]).or_default() += 1;
                    }
                }
            }
            _ => {}
        }
        stack.extend(children(n));
    }
}

fn unused_finding(id: &'static str, name: Node, message: String) -> Finding {
    Finding {
        id,
        start: name.start_byte(),
        end: name.end_byte(),
        level: Level::Weak,
        message,
        fix: None,
    }
}

/// Unused private declarations and unused parameters of private functions (`SPEC-0140` v2
/// behaviours 4 and 5), by name within the file: any other mention of the name counts as a use.
/// Annotated declarations, declarations inside an annotated type, overrides, operators and `main`
/// are never reported.
fn unused(root: Node, text: &str, out: &mut Vec<Finding>) {
    let mut names = std::collections::HashMap::new();
    count_names(root, text, &mut names);
    let used = |name: &str| names.get(name).copied().unwrap_or(0) > 1;
    // (node, inside an annotated or data type, inside an interface)
    let mut stack = vec![(root, false, false, false)];
    while let Some((n, annotated_owner, data_owner, interface)) = stack.pop() {
        let (words, annotated) = modifiers(n, text);
        let has = |w: &str| words.iter().any(|x| x == w);
        let private = has("private");
        let quiet = annotated || annotated_owner || has("override") || has("operator");
        let mut below = (annotated_owner, data_owner, interface);
        match n.kind() {
            "class_declaration" | "object_declaration" => {
                let name = n
                    .child_by_field_name("name")
                    .or_else(|| children(n).into_iter().find(|c| c.kind() == "identifier"));
                if let Some(name) = name
                    && private
                    && !quiet
                    && !used(text_of(name, text))
                {
                    out.push(unused_finding(
                        "kotlin.unused-private",
                        name,
                        format!("Private class '{}' is never used", text_of(name, text)),
                    ));
                }
                below = (
                    annotated_owner || annotated,
                    has("data"),
                    children(n).iter().any(|c| c.kind() == "interface"),
                );
            }
            "companion_object" => below = (annotated_owner || annotated, false, false),
            "class_parameter" => {
                let parts = children(n);
                let property = parts.iter().any(|c| matches!(c.kind(), "val" | "var"));
                let name = parts.iter().find(|c| c.kind() == "identifier");
                if let Some(name) = name
                    && property
                    && private
                    && !quiet
                    && !data_owner
                    && !used(text_of(*name, text))
                {
                    out.push(unused_finding(
                        "kotlin.unused-private",
                        *name,
                        format!("Private property '{}' is never used", text_of(*name, text)),
                    ));
                }
            }
            "property_declaration" => {
                let name = children(n)
                    .into_iter()
                    .find(|c| c.kind() == "variable_declaration")
                    .and_then(|v| children(v).into_iter().find(|c| c.kind() == "identifier"));
                if let Some(name) = name
                    && private
                    && !quiet
                    && !used(text_of(name, text))
                {
                    out.push(unused_finding(
                        "kotlin.unused-private",
                        name,
                        format!("Private property '{}' is never used", text_of(name, text)),
                    ));
                }
            }
            "function_declaration" => {
                let name = n.child_by_field_name("name");
                let fname = name.map(|x| text_of(x, text)).unwrap_or_default();
                let special = fname == "main" || SERIALIZATION.contains(&fname);
                if let Some(name) = name
                    && private
                    && !quiet
                    && !special
                    && !used(fname)
                {
                    out.push(unused_finding(
                        "kotlin.unused-private",
                        name,
                        format!("Private function '{fname}' is never used"),
                    ));
                }
                // Parameters: only where no caller outside the file and no supertype fixes the
                // signature, and the function is not passed as a reference (`::name`).
                let body = children(n).into_iter().any(|c| c.kind() == "function_body");
                let fixed = interface
                    || has("open")
                    || has("abstract")
                    || has("external")
                    || has("expect")
                    || has("actual")
                    || text.contains(&format!("::{fname}"));
                if private && !quiet && !special && body && !fixed {
                    let mut inside = std::collections::HashMap::new();
                    count_names(n, text, &mut inside);
                    let params = children(n)
                        .into_iter()
                        .find(|c| c.kind() == "function_value_parameters");
                    for p in params.into_iter().flat_map(children) {
                        if p.kind() != "parameter" {
                            continue;
                        }
                        let Some(pname) =
                            children(p).into_iter().find(|c| c.kind() == "identifier")
                        else {
                            continue;
                        };
                        let word = text_of(pname, text);
                        if !word.starts_with('_') && inside.get(word).copied().unwrap_or(0) <= 1 {
                            out.push(unused_finding(
                                "kotlin.unused-parameter",
                                pname,
                                format!("Parameter '{word}' is never used"),
                            ));
                        }
                    }
                }
                // Local declarations are the compiler's to report.
                continue;
            }
            _ => {}
        }
        for c in children(n) {
            if c.is_named() {
                stack.push((c, below.0, below.1, below.2));
            }
        }
    }
}

/// Every Kotlin rule with types over the tree, and the unused declarations of the file.
pub fn analyze(root: Node, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    unused(root, text, &mut out);
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "call_expression" => {
                ignored_result(n, text, &mut out);
                call(n, text, &mut out);
            }
            "binary_expression" => binary(n, text, &mut out),
            "string_literal" => template(n, text, &mut out),
            _ => {}
        }
        stack.extend(children(n));
    }
    out
}

// ------------------------------------------------- suppression (SPEC-0098 v13 behaviour 28)

/// Declarations a `@Suppress` can stand on.
fn is_declaration(n: Node) -> bool {
    matches!(
        n.kind(),
        "class_declaration"
            | "object_declaration"
            | "companion_object"
            | "function_declaration"
            | "property_declaration"
            | "class_parameter"
            | "secondary_constructor"
    )
}

/// Classes, interfaces and objects.
fn is_class_like(n: Node) -> bool {
    matches!(
        n.kind(),
        "class_declaration" | "object_declaration" | "companion_object"
    )
}

fn ancestors(n: Node) -> impl Iterator<Item = Node> {
    std::iter::successors(Some(n), |x| x.parent())
}

/// Whether the annotation (or file annotation) `a` is `@Suppress` or `@kotlin.Suppress`.
fn is_suppress(a: Node, text: &str) -> bool {
    let mut stack = vec![a];
    while let Some(x) = stack.pop() {
        if x.kind() == "user_type" {
            let written: String = text_of(x, text)
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            return matches!(written.as_str(), "Suppress" | "kotlin.Suppress");
        }
        if x.kind() == "value_arguments" {
            continue;
        }
        stack.extend(children(x));
    }
    false
}

/// The `value_arguments` of an annotation, if written.
fn annotation_arguments(a: Node) -> Option<Node> {
    let mut stack = vec![a];
    while let Some(x) = stack.pop() {
        if x.kind() == "value_arguments" {
            return Some(x);
        }
        stack.extend(children(x));
    }
    None
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

/// The `@Suppress` annotations on the declaration `el`, or of the file for the root.
fn suppress_annotations<'t>(el: Node<'t>, text: &str) -> Vec<Node<'t>> {
    let found: Vec<Node> = if el.kind() == "source_file" {
        children(el)
            .into_iter()
            .filter(|c| c.kind() == "file_annotation")
            .collect()
    } else {
        children(el)
            .into_iter()
            .filter(|c| c.kind() == "modifiers")
            .flat_map(children)
            .filter(|a| a.kind() == "annotation")
            .collect()
    };
    found
        .into_iter()
        .filter(|a| is_suppress(*a, text))
        .collect()
}

fn suppressed_names(el: Node, text: &str) -> Vec<String> {
    suppress_annotations(el, text)
        .into_iter()
        .flat_map(|a| annotation_strings(a, text))
        .collect()
}

/// Whether the Kotlin finding `f` lies in a declaration, or a file, whose `@Suppress` names it
/// (`SPEC-0098` v13 behaviour 28).
pub(crate) fn suppressed(root: Node, text: &str, f: &Finding) -> bool {
    let matches = |el: Node| {
        suppressed_names(el, text)
            .iter()
            .any(|n| crate::inspections::suppression_matches(n, f.id))
    };
    if matches(root) {
        return true;
    }
    let Some(at) = root.descendant_for_byte_range(f.start, f.end) else {
        return false;
    };
    ancestors(at).filter(|n| is_declaration(*n)).any(matches)
}

/// Adds `name` to the `@Suppress` annotation `a`: before its closing parenthesis, or as its
/// arguments when it has none.
fn add_to(a: Node, text: &str, name: &str) -> TextEdit {
    let quoted = format!("\"{name}\"");
    match annotation_arguments(a) {
        Some(args) => {
            let close = args.end_byte().saturating_sub(1);
            let empty = text_of(args, text)
                .trim_matches(|c: char| c == '(' || c == ')' || c.is_whitespace())
                .is_empty();
            TextEdit {
                range: close..close,
                text: if empty { quoted } else { format!(", {quoted}") },
            }
        }
        None => TextEdit {
            range: a.end_byte()..a.end_byte(),
            text: format!("({quoted})"),
        },
    }
}

/// The edits of "Suppress for declaration / class / file" for the Kotlin finding of `id` at byte
/// `at` (`SPEC-0098` v13 behaviour 28); `None` without such a place or when it suppresses `id`
/// already. The method scope is not offered for Kotlin and means the declaration.
pub(crate) fn suppress_edits(
    root: Node,
    text: &str,
    at: usize,
    id: &str,
    scope: crate::inspections::SuppressScope,
) -> Option<Vec<TextEdit>> {
    use crate::inspections::SuppressScope;
    let node = root.descendant_for_byte_range(at, at)?;
    let name = crate::inspections::short_name(id).unwrap_or(id);
    let el = match scope {
        SuppressScope::Statement | SuppressScope::Method => {
            ancestors(node).find(|n| is_declaration(*n))?
        }
        SuppressScope::Class => ancestors(node).find(|n| is_class_like(*n))?,
        SuppressScope::File => root,
    };
    if suppressed_names(el, text)
        .iter()
        .any(|n| crate::inspections::suppression_matches(n, id))
    {
        return None;
    }
    if let Some(a) = suppress_annotations(el, text).into_iter().next() {
        return Some(vec![add_to(a, text, name)]);
    }
    if el.kind() == "source_file" {
        // Before `package`, after a leading comment block.
        let first = children(el)
            .into_iter()
            .find(|c| !c.kind().contains("comment") && !c.kind().contains("shebang"));
        let at = first.map_or(text.len(), |c| c.start_byte());
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        return Some(vec![TextEdit {
            range: line_start..line_start,
            text: format!("@file:Suppress(\"{name}\")\n\n"),
        }]);
    }
    let line_start = text[..el.start_byte()].rfind('\n').map_or(0, |i| i + 1);
    let prefix = &text[line_start..el.start_byte()];
    let annotation = format!("@Suppress(\"{name}\")");
    // A constructor property, or a declaration that does not start its line: in front of it.
    let edit = if el.kind() == "class_parameter" || !prefix.trim().is_empty() {
        TextEdit {
            range: el.start_byte()..el.start_byte(),
            text: format!("{annotation} "),
        }
    } else {
        TextEdit {
            range: line_start..line_start,
            text: format!("{prefix}{annotation}\n"),
        }
    };
    Some(vec![edit])
}

#[cfg(test)]
mod tests {
    use crate::LanguageId;
    use crate::inspections::inspect;
    use crate::jvm::apply;
    use crate::syntax::SyntaxDocument;

    fn found(text: &str) -> Vec<String> {
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, text).unwrap();
        inspect(&doc)
            .into_iter()
            .filter(|f| f.id != "kotlin.empty-catch")
            .map(|f| format!("{} {}", f.id, &text[f.start..f.end]))
            .collect()
    }

    fn fixed(text: &str, id: &str) -> String {
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, text).unwrap();
        let fix = inspect(&doc)
            .into_iter()
            .filter(|f| f.id == id)
            .find_map(|f| f.fix)
            .unwrap();
        apply(text, &fix.edits)
    }

    const SRC: &str = r#"import java.math.BigDecimal
class A(val name: String) {
    var s: String = "x"
    fun f(a: String, b: String, xs: IntArray, arr: Array<String>, grid: Array<IntArray>, cs: CharArray, m: BigDecimal, i: Int, n: Int, vararg rest: Int) {
        var u = "q"
        a.trim()
        u.uppercase()
        m.add(m)
        this.s.trim()
        val t = a.trim()
        if (a === b) {}
        if (i !== n) {}
        if (xs == xs) {}
        if (arr != arr) {}
        println(xs)
        println(cs)
        val msg = "v $xs ${grid} " + arr
        a.equals(i)
        a.equals(b)
        rest.equals(xs)
        xs.toString()
        val d = (i / n).toDouble()
        val e = (i / 2.0).toDouble()
        val w = listOf(1).map { it.toString().trim() }
        val v = if (true) { a.trim() } else { b }
    }
}
"#;

    #[test]
    fn rules_with_types() {
        assert_eq!(
            found(SRC),
            [
                "kotlin.ignored-result a.trim()",
                "kotlin.ignored-result u.uppercase()",
                "kotlin.ignored-result m.add(m)",
                "kotlin.ignored-result this.s.trim()",
                "kotlin.identity-equality a === b",
                "kotlin.identity-equality i !== n",
                "kotlin.array-equality xs == xs",
                "kotlin.array-equality arr != arr",
                "kotlin.array-to-string xs",
                "kotlin.array-to-string $xs",
                "kotlin.array-to-string ${grid}",
                "kotlin.array-to-string arr",
                "kotlin.equals-incompatible a.equals(i)",
                "kotlin.array-equality rest.equals(xs)",
                "kotlin.array-to-string xs.toString()",
                "kotlin.integer-division (i / n).toDouble()",
            ]
        );
        assert!(
            fixed(SRC, "kotlin.ignored-result")
                .contains("        a.trim()\n        u = u.uppercase()")
        );
        assert!(fixed(SRC, "kotlin.identity-equality").contains("if (a == b) {}"));
        assert!(fixed(SRC, "kotlin.array-equality").contains("if (xs.contentEquals(xs)) {}"));
        let strings = fixed(SRC, "kotlin.array-to-string");
        assert!(
            strings.contains("println(xs.contentToString())"),
            "{strings}"
        );
        assert!(fixed(SRC, "kotlin.integer-division").contains("val d = i.toDouble() / n"));
        let nested = "fun f(g: Array<IntArray>) { val s = \"${g}\" }";
        assert!(fixed(nested, "kotlin.array-to-string").contains("\"${g.contentDeepToString()}\""));
        let simple = "fun f(xs: IntArray) { val s = \"a $xs.\" }";
        assert!(fixed(simple, "kotlin.array-to-string").contains("\"a ${xs.contentToString()}.\""));
        // Unknown types stay quiet.
        let unknown = "fun f(a: Thing, b: Thing) { a.trim(); if (a === b) {}; println(a); val d = (a / b).toDouble() }";
        assert!(found(unknown).is_empty(), "{:?}", found(unknown));
    }

    /// SPEC-0140 AK-6 (v2): unused private declarations and parameters of private functions.
    #[test]
    fn unused_private_declarations_and_parameters() {
        let src = r#"package zoo

private const val LIMIT = 3
private val spare = 4
private fun helper(n: Int, label: String, _skip: Int): Int = n + LIMIT
private fun lonely() {}
private class Hidden
private class Shown

class Person(val name: String, private val age: Int, private var seen: Boolean) {
    private val nick = "x"
    private var count = 0
    private fun greet(greeting: String, loud: Boolean) = "$greeting $name"
    private fun idle(times: Int) {}
    fun touch() { count += helper(1, "a", 2); seen = true; greet("hi", false) }
    fun make(): Any = Shown()
}
"#;
        assert_eq!(
            found(src),
            [
                "kotlin.unused-private spare",
                "kotlin.unused-parameter label",
                "kotlin.unused-private lonely",
                "kotlin.unused-private Hidden",
                "kotlin.unused-private age",
                "kotlin.unused-private nick",
                "kotlin.unused-parameter loud",
                "kotlin.unused-private idle",
                "kotlin.unused-parameter times",
            ]
        );
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, src).unwrap();
        let unused: Vec<_> = inspect(&doc)
            .into_iter()
            .filter(|f| f.id.starts_with("kotlin.unused"))
            .collect();
        assert!(
            unused
                .iter()
                .all(|f| f.level == crate::inspections::Level::Weak)
        );
        assert!(unused.iter().all(|f| crate::inspections::is_unused(f.id)));
        assert_eq!(unused[0].message, "Private property 'spare' is never used");
        assert_eq!(unused[1].message, "Parameter 'label' is never used");
        assert_eq!(unused[2].message, "Private function 'lonely' is never used");
        assert_eq!(unused[3].message, "Private class 'Hidden' is never used");
    }

    /// SPEC-0140 AK-6 (v2): what is never reported.
    #[test]
    fn unused_stays_quiet_for_public_api_and_special_declarations() {
        let quiet = r#"package zoo

import kotlin.reflect.KProperty

fun api(unusedParam: Int) {}
val shared = 1
class Public(val a: Int, b: Int)
fun main(args: Array<String>) {}
private fun main() {}

@Suppress("unused") private fun kept() {}
@JvmStatic private fun byName(x: Int) {}

@Entity
class Row {
    private var id: Long = 0
    private fun hook(event: String) {}
}

data class Point(private val x: Int, private val y: Int)

abstract class Base {
    abstract fun run(arg: Int)
    open fun step(arg: Int) {}
}

class Impl : Base() {
    override fun run(arg: Int) {}
    private operator fun getValue(thisRef: Any?, property: KProperty<*>): Int = 1
    private fun readObject(stream: Any) {}
    private fun callback(event: String) {}
    private fun used(text: String) = "$text"
    fun wire(): Any = listOf(::callback, used("a"))
}

interface Port {
    fun send(payload: String) {}
}
"#;
        assert!(found(quiet).is_empty(), "{:?}", found(quiet));
    }

    /// SPEC-0098 AK-17 (v13 behaviour 28): `@Suppress` on a declaration, a class around it and
    /// the file; the three options write the annotations.
    #[test]
    fn suppress_kotlin_findings() {
        use crate::inspections::{SuppressScope, suppress};
        let src = "// header\npackage zoo\n\nclass Person(private val age: Int) {\n    private fun lonely() {}\n    fun same(a: String, b: String) {\n        if (a === b) {}\n    }\n}\n";
        assert_eq!(
            found(src),
            [
                "kotlin.unused-private age",
                "kotlin.unused-private lonely",
                "kotlin.identity-equality a === b",
            ]
        );
        // On the class around it, by id; on the file, by the short name.
        let on_class = src.replace(
            "class Person",
            "@Suppress(\"kotlin.identity-equality\")\nclass Person",
        );
        assert!(
            !found(&on_class)
                .iter()
                .any(|f| f.starts_with("kotlin.identity"))
        );
        let on_function = src.replace(
            "    fun same",
            "    @kotlin.Suppress(\"ALL\")\n    fun same",
        );
        assert_eq!(found(&on_function).len(), 2);
        let on_file = format!("@file:Suppress(\"unused\")\n{src}");
        assert_eq!(found(&on_file), ["kotlin.identity-equality a === b"]);
        // The options.
        let at = |s: &str, w: &str| s.find(w).unwrap();
        let apply = |s: &str, at: usize, id: &str, scope| {
            crate::jvm::apply(s, &suppress(s, at, id, scope).unwrap())
        };
        assert!(
            apply(
                src,
                at(src, "lonely"),
                "kotlin.unused-private",
                SuppressScope::Statement
            )
            .contains("    @Suppress(\"unused\")\n    private fun lonely")
        );
        assert!(
            apply(
                src,
                at(src, "age:"),
                "kotlin.unused-private",
                SuppressScope::Statement
            )
            .contains("class Person(@Suppress(\"unused\") private val age")
        );
        assert!(
            apply(
                src,
                at(src, "lonely"),
                "kotlin.unused-declaration",
                SuppressScope::Class
            )
            .contains("// header\npackage zoo\n\n@Suppress(\"unused\")\nclass Person(")
        );
        assert!(
            apply(
                src,
                at(src, "lonely"),
                "kotlin.unused-declaration",
                SuppressScope::File
            )
            .starts_with("// header\n@file:Suppress(\"unused\")\n\npackage zoo\n")
        );
        // A name is added to an existing annotation; a place that suppresses already has none.
        assert!(
            apply(
                &on_class,
                at(&on_class, "lonely"),
                "kotlin.unused-private",
                SuppressScope::Class
            )
            .contains("@Suppress(\"kotlin.identity-equality\", \"unused\")\nclass Person")
        );
        assert!(
            apply(
                &on_file,
                at(&on_file, "lonely"),
                "kotlin.identity-equality",
                SuppressScope::File
            )
            .starts_with("@file:Suppress(\"unused\", \"kotlin.identity-equality\")\n")
        );
        assert_eq!(
            suppress(
                &on_file,
                at(&on_file, "lonely"),
                "kotlin.unused-private",
                SuppressScope::File
            ),
            None
        );
        let top = "fun f() {}\n";
        assert_eq!(
            suppress(top, 4, "kotlin.unused-declaration", SuppressScope::Class),
            None
        );
    }
}
