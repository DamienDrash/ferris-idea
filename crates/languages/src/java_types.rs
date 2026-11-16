//! A small type model for Java expressions on the syntax tree (`SPEC-0139`): enough for the
//! inspections that need to know a type — literals, declared names (parameters, locals with `var`,
//! fields), casts, `new`, string concatenation, the file's own methods and well-known methods of
//! the JDK's value types. Anything else is unknown, and rules stay quiet on unknown types.

use tree_sitter::Node;

/// Boxed primitive types.
pub const BOXES: &[&str] = &[
    "Integer",
    "Long",
    "Short",
    "Byte",
    "Double",
    "Float",
    "Boolean",
    "Character",
];

/// Immutable JDK types whose methods return a new value instead of changing the receiver, and
/// those methods.
pub const IMMUTABLE: &[(&str, &[&str])] = &[
    (
        "String",
        &[
            "trim",
            "strip",
            "stripLeading",
            "stripTrailing",
            "toUpperCase",
            "toLowerCase",
            "replace",
            "replaceAll",
            "replaceFirst",
            "substring",
            "concat",
            "repeat",
            "intern",
            "formatted",
            "indent",
            "translateEscapes",
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
            "round",
            "plus",
            "stripTrailingZeros",
            "movePointLeft",
            "movePointRight",
            "max",
            "min",
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
            "and",
            "or",
            "xor",
            "not",
            "max",
            "min",
            "gcd",
        ],
    ),
];

fn text_of<'a>(n: Node, text: &'a str) -> &'a str {
    &text[n.byte_range()]
}

fn children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

/// A written type as the model compares it: without generics, annotations and package, arrays
/// kept (`java.util.List<String>` → `List`, `int[]` → `int[]`).
pub fn normalize(ty: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for ch in ty.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth -= 1,
            c if depth == 0 && !c.is_whitespace() => out.push(c),
            _ => {}
        }
    }
    let dims = out.matches("[]").count();
    let base = out.trim_end_matches("[]").replace("...", "");
    let base = base.rsplit('.').next().unwrap_or(&base).to_owned();
    let varargs = ty.contains("...");
    format!("{base}{}", "[]".repeat(dims + usize::from(varargs)))
}

/// Whether `ty` is a primitive numeric type.
pub fn is_numeric_primitive(ty: &str) -> bool {
    matches!(
        ty,
        "int" | "long" | "short" | "byte" | "char" | "double" | "float"
    )
}

/// Whether `ty` is an integral primitive type.
pub fn is_integral(ty: &str) -> bool {
    matches!(ty, "int" | "long" | "short" | "byte" | "char")
}

/// The boxed type of a primitive, or the type itself.
pub fn boxed(ty: &str) -> &str {
    match ty {
        "int" => "Integer",
        "long" => "Long",
        "short" => "Short",
        "byte" => "Byte",
        "double" => "Double",
        "float" => "Float",
        "boolean" => "Boolean",
        "char" => "Character",
        t => t,
    }
}

/// The declaration of `wanted` visible at `at`: the nearest parameter, earlier local variable,
/// catch or resource or enhanced-for variable, or field, with its declared type node and, for a
/// variable with an initializer, that expression.
fn declaration<'t>(at: Node<'t>, wanted: &str, text: &str) -> Option<(String, Option<Node<'t>>)> {
    let in_decl = |decl: Node<'t>| -> Option<(String, Option<Node<'t>>)> {
        match decl.kind() {
            // `catch (A | B e)`: the type is the `catch_type` child.
            "catch_formal_parameter" => {
                let name = decl.child_by_field_name("name")?;
                let ty = children(decl)
                    .into_iter()
                    .find(|c| c.kind() == "catch_type")?;
                (text_of(name, text) == wanted).then(|| (text_of(ty, text).to_owned(), None))
            }
            // `String... rest` is an array.
            "spread_parameter" => {
                let declarator = children(decl)
                    .into_iter()
                    .find(|c| c.kind() == "variable_declarator")?;
                let name = declarator.child_by_field_name("name")?;
                let ty = children(decl).into_iter().find(|c| {
                    c.is_named()
                        && !matches!(
                            c.kind(),
                            "modifiers"
                                | "annotation"
                                | "marker_annotation"
                                | "variable_declarator"
                        )
                })?;
                (text_of(name, text) == wanted).then(|| (format!("{}[]", text_of(ty, text)), None))
            }
            "formal_parameter" | "enhanced_for_statement" | "resource" => {
                let ty = decl.child_by_field_name("type")?;
                let name = decl.child_by_field_name("name")?;
                (text_of(name, text) == wanted).then(|| (text_of(ty, text).to_owned(), None))
            }
            _ => {
                let ty = decl.child_by_field_name("type")?;
                let mut c = decl.walk();
                let found = decl.children_by_field_name("declarator", &mut c).find(|d| {
                    d.child_by_field_name("name")
                        .is_some_and(|n| text_of(n, text) == wanted)
                });
                found.map(|d| (text_of(ty, text).to_owned(), d.child_by_field_name("value")))
            }
        }
    };
    let mut scope = at.parent();
    let mut inner = at;
    while let Some(s) = scope {
        if matches!(
            s.kind(),
            "enhanced_for_statement" | "catch_clause" | "resource_specification"
        ) {
            let found = match s.kind() {
                "catch_clause" => children(s)
                    .into_iter()
                    .find(|c| c.kind() == "catch_formal_parameter")
                    .and_then(in_decl),
                "resource_specification" => children(s)
                    .into_iter()
                    .filter(|c| c.kind() == "resource" && c.start_byte() < at.start_byte())
                    .find_map(in_decl),
                _ => in_decl(s),
            };
            if found.is_some() {
                return found;
            }
        }
        for child in children(s) {
            let found = match child.kind() {
                "formal_parameters" | "lambda_parameters" => children(child)
                    .into_iter()
                    .filter(|p| matches!(p.kind(), "formal_parameter" | "spread_parameter"))
                    .find_map(in_decl),
                "local_variable_declaration"
                    if child.start_byte() < at.start_byte() && child.id() != inner.id() =>
                {
                    in_decl(child)
                }
                "field_declaration" | "constant_declaration" => in_decl(child),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        inner = s;
        scope = s.parent();
    }
    None
}

/// The class body around `n`.
fn enclosing_class(n: Node) -> Option<Node> {
    let mut at = n.parent();
    while let Some(p) = at {
        if matches!(
            p.kind(),
            "class_body" | "enum_body" | "interface_body" | "record_declaration"
        ) {
            return Some(p);
        }
        at = p.parent();
    }
    None
}

/// The return type of the file's own method `name` called without receiver, when every method of
/// that name in the class returns the same type.
fn own_method_type(call: Node, name: &str, text: &str) -> Option<String> {
    let body = enclosing_class(call)?;
    let types: Vec<String> = children(body)
        .into_iter()
        .filter(|m| m.kind() == "method_declaration")
        .filter(|m| {
            m.child_by_field_name("name")
                .is_some_and(|n| text_of(n, text) == name)
        })
        .filter_map(|m| {
            m.child_by_field_name("type")
                .map(|t| normalize(text_of(t, text)))
        })
        .collect();
    let first = types.first()?.clone();
    types.iter().all(|t| *t == first).then_some(first)
}

/// Methods of well-known JDK types and what they return.
fn jdk_method(receiver: &str, method: &str, args: usize) -> Option<&'static str> {
    if let Some((_, methods)) = IMMUTABLE.iter().find(|(t, _)| *t == receiver)
        && methods.contains(&method)
    {
        return Some(match receiver {
            "String" => "String",
            "BigDecimal" => "BigDecimal",
            _ => "BigInteger",
        });
    }
    Some(match (receiver, method, args) {
        (_, "toString", 0) => "String",
        (_, "equals", 1) => "boolean",
        (_, "hashCode", 0) => "int",
        ("String", "length" | "indexOf" | "lastIndexOf" | "compareTo", _) => "int",
        ("String", "charAt", 1) => "char",
        (
            "String",
            "isEmpty" | "isBlank" | "startsWith" | "endsWith" | "contains" | "matches"
            | "equalsIgnoreCase",
            _,
        ) => "boolean",
        ("String", "toCharArray", 0) => "char[]",
        ("String", "split", _) => "String[]",
        ("String", "valueOf" | "format" | "join" | "copyValueOf", _) => "String",
        ("StringBuilder", "append" | "insert" | "reverse", _) => "StringBuilder",
        ("Integer", "valueOf", 1) => "Integer",
        ("Long", "valueOf", 1) => "Long",
        ("Integer", "parseInt", _) => "int",
        ("Long", "parseLong", _) => "long",
        ("Double", "parseDouble", 1) => "double",
        _ => return None,
    })
}

/// The type of expression `n` as far as the model knows it, normalized (see [`normalize`]);
/// `null` for the null literal.
pub fn type_of(n: Node, text: &str) -> Option<String> {
    Some(match n.kind() {
        "string_literal" | "text_block" => "String".into(),
        "character_literal" => "char".into(),
        "true" | "false" => "boolean".into(),
        "null_literal" => "null".into(),
        "decimal_integer_literal"
        | "hex_integer_literal"
        | "octal_integer_literal"
        | "binary_integer_literal" => {
            if text_of(n, text).ends_with(['l', 'L']) {
                "long".into()
            } else {
                "int".into()
            }
        }
        "decimal_floating_point_literal" | "hex_floating_point_literal" => {
            if text_of(n, text).ends_with(['f', 'F']) {
                "float".into()
            } else {
                "double".into()
            }
        }
        "parenthesized_expression" => type_of(n.named_child(0)?, text)?,
        "cast_expression" => normalize(text_of(n.child_by_field_name("type")?, text)),
        "object_creation_expression" => normalize(text_of(n.child_by_field_name("type")?, text)),
        "array_creation_expression" => {
            let base = normalize(text_of(n.child_by_field_name("type")?, text));
            let dims = text_of(n, text).matches('[').count();
            format!("{base}{}", "[]".repeat(dims))
        }
        "identifier" => {
            let (written, init) = declaration(n, text_of(n, text), text)?;
            if written == "var" {
                type_of(init?, text)?
            } else if written.contains('|') {
                // A multi-catch parameter has no single type.
                return None;
            } else {
                normalize(&written)
            }
        }
        "field_access" => {
            let object = n.child_by_field_name("object")?;
            if object.kind() != "this" {
                return None;
            }
            let field = n.child_by_field_name("field")?;
            let body = enclosing_class(n)?;
            let decl = children(body).into_iter().find(|c| {
                c.kind() == "field_declaration" && {
                    let mut cur = c.walk();
                    c.children_by_field_name("declarator", &mut cur).any(|d| {
                        d.child_by_field_name("name")
                            .is_some_and(|x| text_of(x, text) == text_of(field, text))
                    })
                }
            })?;
            normalize(text_of(decl.child_by_field_name("type")?, text))
        }
        "array_access" => {
            let array = type_of(n.child_by_field_name("array")?, text)?;
            array.strip_suffix("[]")?.to_owned()
        }
        "binary_expression" => {
            let op = n
                .child_by_field_name("operator")
                .map(|o| text_of(o, text))?;
            match op {
                "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" | "instanceof" => {
                    "boolean".into()
                }
                "+" => {
                    let l = type_of(n.child_by_field_name("left")?, text);
                    let r = type_of(n.child_by_field_name("right")?, text);
                    if l.as_deref() == Some("String") || r.as_deref() == Some("String") {
                        "String".into()
                    } else {
                        numeric_result(l?, r?)?
                    }
                }
                "-" | "*" | "/" | "%" => numeric_result(
                    type_of(n.child_by_field_name("left")?, text)?,
                    type_of(n.child_by_field_name("right")?, text)?,
                )?,
                _ => return None,
            }
        }
        "method_invocation" => {
            let name = text_of(n.child_by_field_name("name")?, text);
            let args = n
                .child_by_field_name("arguments")
                .map_or(0, |a| a.named_child_count());
            match n.child_by_field_name("object") {
                None => own_method_type(n, name, text)?,
                Some(object) => {
                    // A static call names the class; otherwise the receiver's type.
                    let receiver = type_of(object, text).or_else(|| {
                        let t = text_of(object, text);
                        t.chars()
                            .next()
                            .is_some_and(char::is_uppercase)
                            .then(|| normalize(t))
                    })?;
                    jdk_method(&receiver, name, args)?.to_owned()
                }
            }
        }
        _ => return None,
    })
}

/// The type of an arithmetic operation on two known types (binary numeric promotion).
fn numeric_result(l: String, r: String) -> Option<String> {
    let unbox = |t: &str| -> Option<&'static str> {
        Some(match t {
            "int" | "Integer" | "short" | "Short" | "byte" | "Byte" | "char" | "Character" => "int",
            "long" | "Long" => "long",
            "float" | "Float" => "float",
            "double" | "Double" => "double",
            _ => return None,
        })
    };
    let (l, r) = (unbox(&l)?, unbox(&r)?);
    Some(
        if l == "double" || r == "double" {
            "double"
        } else if l == "float" || r == "float" {
            "float"
        } else if l == "long" || r == "long" {
            "long"
        } else {
            "int"
        }
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LanguageId;
    use crate::syntax::SyntaxDocument;

    /// The type of the expression of `T probe = <expr>;`-free form: the argument of `t(…)`.
    fn types(src: &str) -> Vec<Option<String>> {
        let doc = SyntaxDocument::new(LanguageId::Java, false, src).unwrap();
        let mut out = Vec::new();
        let mut stack = vec![doc.tree().root_node()];
        let mut found = Vec::new();
        while let Some(n) = stack.pop() {
            if n.kind() == "method_invocation"
                && n.child_by_field_name("name")
                    .is_some_and(|m| text_of(m, src) == "t")
                && let Some(arg) = n
                    .child_by_field_name("arguments")
                    .and_then(|a| a.named_child(0))
            {
                found.push(arg);
            }
            stack.extend(children(n));
        }
        found.sort_by_key(|n| n.start_byte());
        for arg in found {
            out.push(type_of(arg, src));
        }
        out
    }

    #[test]
    fn expressions_get_their_types() {
        let src = "import java.util.*;\nclass A {\n  private java.util.List<String> names;\n  long count() { return 0; }\n  void f(String s, int[] xs, Integer boxed, String... rest) {\n    var v = \"x\" + 1;\n    double d = 2;\n    StringBuilder sb = new StringBuilder();\n    for (String e : rest) { t(e); }\n    try { } catch (IllegalStateException ex) { t(ex); }\n    t(s); t(xs); t(boxed); t(rest); t(v); t(d); t(sb.append(1)); t(sb.toString());\n    t(s.trim().length()); t(xs[0]); t(1L * 2); t(1 / 2.0f); t((Object) s); t(this.names);\n    t(count()); t(String.valueOf(1)); t(unknown); t(other.call()); t(null); t(s == null);\n  }\n}\n";
        let got: Vec<String> = types(src)
            .into_iter()
            .map(|t| t.unwrap_or_else(|| "?".into()))
            .collect();
        assert_eq!(
            got,
            [
                "String",
                "IllegalStateException",
                "String",
                "int[]",
                "Integer",
                "String[]",
                "String",
                "double",
                "StringBuilder",
                "String",
                "int",
                "int",
                "long",
                "float",
                "Object",
                "List",
                "long",
                "String",
                "?",
                "?",
                "null",
                "boolean"
            ]
        );
        assert_eq!(normalize("java.util.Map<String, List<Integer>>"), "Map");
        assert_eq!(normalize("byte[][]"), "byte[][]");
    }
}
