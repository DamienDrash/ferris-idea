//! What a usage of a symbol in a JavaScript or TypeScript file is, from the syntax tree
//! (`SPEC-0293` v4 behaviours 14a and 16): a call, a read, a write, an import, a use as a type or a
//! declaration; and whether it declares a member, which is an implementation or an override of the
//! searched member and no usage (OBS-0028 T5); and whether it lies in a string or a comment, which
//! a text search finds and no usage is (`SPEC-0293` v11 behaviour 10a).

use std::path::Path;

use tree_sitter::{Node, Point};

use crate::context::UsageKind;
use crate::python_extract::{field, is_field};

/// One usage as the syntax tree sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptUsage {
    /// What it does with the symbol.
    pub kind: UsageKind,
    /// It is the name of a method, field, accessor or property signature of a class, interface or
    /// object literal: another member's declaration (an implementation or an override).
    pub member_declaration: bool,
    /// It lies inside a string, a template string's text or a comment: a text occurrence, no
    /// usage (`SPEC-0293` v11 behaviour 10a).
    pub in_text: bool,
}

/// Kinds whose `name` declares a member.
const MEMBERS: &[&str] = &[
    "method_definition",
    "method_signature",
    "abstract_method_signature",
    "public_field_definition",
    "field_definition",
    "property_signature",
];

/// Kinds whose `name` declares something else.
const DECLARATIONS: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "function_signature",
    "class_declaration",
    "abstract_class_declaration",
    "interface_declaration",
    "type_alias_declaration",
    "enum_declaration",
    "variable_declarator",
    "internal_module",
    "module",
];

/// Whether the usages of `path` are read from its syntax tree: JavaScript and TypeScript.
pub fn supported(path: &Path) -> bool {
    crate::ts_structure::is_script(path)
}

/// The usages at `positions` (0-based line and character) of `text`, the content of `path`; `None`
/// for a position without a name or when the file's language is not [`supported`].
pub fn classify(path: &Path, text: &str, positions: &[(u64, u64)]) -> Vec<Option<ScriptUsage>> {
    let Some(doc) = parse(path, text) else {
        return vec![None; positions.len()];
    };
    let root = doc.tree().root_node();
    let lines: Vec<&str> = text.split('\n').collect();
    positions
        .iter()
        .map(|&(line, character)| {
            let l = lines.get(line as usize)?;
            let byte = l
                .char_indices()
                .nth(character as usize)
                .map_or(l.len(), |(b, _)| b);
            let at = Point::new(line as usize, byte);
            let n = root.descendant_for_point_range(at, at)?;
            Some(of_node(n))
        })
        .collect()
}

fn parse(path: &Path, text: &str) -> Option<crate::syntax::SyntaxDocument> {
    if !crate::ts_structure::is_script(path) {
        return None;
    }
    let language = if crate::ts_structure::is_typescript(path) {
        crate::LanguageId::TypeScript
    } else {
        crate::LanguageId::JavaScript
    };
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    crate::syntax::SyntaxDocument::new(language, tsx, text).ok()
}

fn of_node(n: Node) -> ScriptUsage {
    let usage = |kind| ScriptUsage {
        kind,
        member_declaration: false,
        in_text: false,
    };
    if in_text(n) {
        return ScriptUsage {
            kind: UsageKind::Other,
            member_declaration: false,
            in_text: true,
        };
    }
    let Some(p) = n.parent() else {
        return usage(UsageKind::Read);
    };
    // A member's name: `area` in `area(): number {`, `abstract area(): number;`, `x = 1;`.
    let named = |p: Node| is_field(p, "name", n) || is_field(p, "property", n);
    if MEMBERS.contains(&p.kind()) && named(p) {
        return ScriptUsage {
            kind: UsageKind::Declaration,
            member_declaration: true,
            in_text: false,
        };
    }
    // `{ area: () => 1 }` and `{ area() {} }` in an object literal.
    if p.kind() == "pair" && is_field(p, "key", n) {
        return ScriptUsage {
            kind: UsageKind::Declaration,
            member_declaration: true,
            in_text: false,
        };
    }
    if DECLARATIONS.contains(&p.kind()) && is_field(p, "name", n) {
        return usage(UsageKind::Declaration);
    }
    let mut up = Some(p);
    while let Some(a) = up {
        match a.kind() {
            "import_statement" | "import_specifier" | "namespace_import" | "import_clause" => {
                return usage(UsageKind::Import);
            }
            // Types: annotations, type arguments, `extends` and `implements` of a class or an
            // interface.
            "type_annotation"
            | "type_arguments"
            | "implements_clause"
            | "extends_type_clause"
            | "extends_clause"
            | "class_heritage"
            | "type_parameters"
            | "type_alias_declaration" => {
                return usage(UsageKind::Type);
            }
            // The first statement or expression boundary ends the search for a context.
            k if k.ends_with("_statement") || k == "statement_block" || k == "arguments" => break,
            _ => {}
        }
        up = a.parent();
    }
    if n.kind() == "type_identifier" {
        return usage(UsageKind::Type);
    }
    // `area` in `this.area()` is the member expression's property.
    let e = if matches!(p.kind(), "member_expression" | "subscript_expression")
        && (is_field(p, "property", n) || is_field(p, "index", n))
    {
        p
    } else {
        n
    };
    let Some(q) = e.parent() else {
        return usage(UsageKind::Read);
    };
    let kind = match q.kind() {
        "call_expression" if is_field(q, "function", e) => UsageKind::Call,
        "new_expression" if is_field(q, "constructor", e) => UsageKind::Call,
        "decorator" => UsageKind::Call,
        "assignment_expression" | "augmented_assignment_expression" if is_field(q, "left", e) => {
            UsageKind::Write
        }
        "update_expression" => UsageKind::Write,
        "for_in_statement" if is_field(q, "left", e) => UsageKind::Write,
        _ => UsageKind::Read,
    };
    // An optional call `a?.area()`: the member is the function too.
    if kind == UsageKind::Read
        && q.kind() == "call_expression"
        && field(q, "function").is_some_and(|f| f.id() == e.id())
    {
        return usage(UsageKind::Call);
    }
    usage(kind)
}

/// `n` lies in the text of a string, of a template string outside its `${…}`, or in a comment.
fn in_text(n: Node) -> bool {
    let mut up = Some(n);
    while let Some(a) = up {
        match a.kind() {
            "template_substitution" => return false,
            "string" | "string_fragment" | "template_string" | "comment" => return true,
            _ => {}
        }
        up = a.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind and member flag at the first occurrence of `needle` (plus `plus` characters) in
    /// `text`.
    fn at(path: &str, text: &str, needle: &str) -> ScriptUsage {
        let b = text.find(needle).unwrap_or_else(|| panic!("{needle}"));
        let before = &text[..b];
        let line = before.matches('\n').count() as u64;
        let character = before.rsplit('\n').next().unwrap().chars().count() as u64;
        classify(Path::new(path), text, &[(line, character)])[0].unwrap()
    }

    const SHAPES: &str = "import { Shape, other as alias } from \"./shape\";\n\nexport interface Shape {\n  area(): number;\n}\n\nexport abstract class Base implements Shape {\n  abstract area(): number;\n  describe(): string {\n    return String(this.area());\n  }\n}\n\nexport class Circle extends Base {\n  r = 1;\n  area(): number {\n    this.r = 2;\n    this.r += 1;\n    const s: Shape = this;\n    return this.r * this.r;\n  }\n}\n\nconst o = { area: () => 1, size() { return 2; } };\nlet total = 0;\ntotal++;\nfunction f(a: string): void;\nfunction f(a: any) { return new Circle(); }\n";

    /// OBS-0028 T5: the declarations of `area` in `Base` and `Circle` are members, the call in
    /// `describe` is a call.
    #[test]
    fn kinds_from_the_syntax_tree() {
        let member = ScriptUsage {
            kind: UsageKind::Declaration,
            member_declaration: true,
            in_text: false,
        };
        let plain = |kind| ScriptUsage {
            kind,
            member_declaration: false,
            in_text: false,
        };
        for path in ["a.ts", "a.tsx"] {
            assert_eq!(at(path, SHAPES, "area(): number;\n}"), member);
            assert_eq!(at(path, SHAPES, "area(): number;\n  describe"), member);
            assert_eq!(at(path, SHAPES, "area(): number {"), member);
            assert_eq!(at(path, SHAPES, "r = 1;"), member);
            assert_eq!(at(path, SHAPES, "area: () => 1"), member);
            assert_eq!(at(path, SHAPES, "size() {"), member);
            assert_eq!(at(path, SHAPES, "area());"), plain(UsageKind::Call));
            assert_eq!(at(path, SHAPES, "Circle();"), plain(UsageKind::Call));
            assert_eq!(at(path, SHAPES, "r = 2;"), plain(UsageKind::Write));
            assert_eq!(at(path, SHAPES, "r += 1;"), plain(UsageKind::Write));
            assert_eq!(at(path, SHAPES, "total++"), plain(UsageKind::Write));
            assert_eq!(at(path, SHAPES, "r * this.r"), plain(UsageKind::Read));
            assert_eq!(at(path, SHAPES, "Shape, other"), plain(UsageKind::Import));
            assert_eq!(at(path, SHAPES, "alias }"), plain(UsageKind::Import));
            assert_eq!(
                at(path, SHAPES, "Shape {\n  abstract"),
                plain(UsageKind::Type)
            );
            assert_eq!(at(path, SHAPES, "Shape = this"), plain(UsageKind::Type));
            assert_eq!(at(path, SHAPES, "Base {\n  r"), plain(UsageKind::Type));
            assert_eq!(
                at(path, SHAPES, "Shape {\n  area"),
                plain(UsageKind::Declaration)
            );
            assert_eq!(
                at(path, SHAPES, "f(a: string)"),
                plain(UsageKind::Declaration)
            );
            assert_eq!(at(path, SHAPES, "total = 0"), plain(UsageKind::Declaration));
        }
        // JavaScript: the same for classes and calls.
        let js = "class A {\n  area() { return 1; }\n}\nclass B extends A {\n  area() { return super.area() + 1; }\n}\nnew B().area();\n";
        assert_eq!(at("a.js", js, "area() { return 1"), member);
        assert_eq!(at("a.js", js, "area() + 1"), plain(UsageKind::Call));
        assert_eq!(at("a.js", js, "area();\n"), plain(UsageKind::Call));
        assert_eq!(
            at("a.jsx", js, "A {\n  area() { return super"),
            plain(UsageKind::Type)
        );
        // Not supported, or no name there.
        assert_eq!(classify(Path::new("a.py"), "x = 1\n", &[(0, 0)]), [None]);
        assert_eq!(classify(Path::new("a.ts"), "x\n", &[(5, 0)]), [None]);
    }

    /// SPEC-0293 v11 behaviour 10a: a name in a string, in a template string's text or in a
    /// comment is text, no usage; inside `${…}` it is code again.
    #[test]
    fn strings_and_comments_are_no_usages() {
        let text =
            "const a = \"area\";\n// area here\n/* area */\nconst b = `area ${this.area()}`;\n";
        let text_only = ScriptUsage {
            kind: UsageKind::Other,
            member_declaration: false,
            in_text: true,
        };
        for path in ["a.ts", "a.js", "a.tsx"] {
            assert_eq!(at(path, text, "area\";"), text_only, "{path}");
            assert_eq!(at(path, text, "area here"), text_only, "{path}");
            assert_eq!(at(path, text, "area */"), text_only, "{path}");
            assert_eq!(at(path, text, "area ${"), text_only, "{path}");
            assert!(!at(path, text, "area()}").in_text, "{path}");
            assert_eq!(at(path, text, "area()}").kind, UsageKind::Call, "{path}");
        }
    }
}
