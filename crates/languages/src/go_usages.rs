//! What a usage of a symbol in a Go file is, from the syntax tree (`SPEC-0293` v12 behaviour
//! 14b): the type of a method's receiver, the type of a composite literal (a struct
//! initialization), a call, an import, a use as a type, the name of a declaration, or a mention in
//! a comment (behaviour 30); a place inside a string is text, no usage.

use std::path::Path;

use tree_sitter::{Node, Point};

use crate::context::UsageKind;
use crate::insight::Location;
use crate::python_extract::is_field;
use crate::script_usages::ScriptUsage;

/// Whether the usages of `path` are read from the Go syntax tree.
pub fn supported(path: &Path) -> bool {
    crate::LanguageId::detect(path) == Some(crate::LanguageId::Go)
}

/// The usages at `positions` (0-based line and character) of `text`, the content of the Go file
/// `path`; `None` where the tree has no rule and the line rule of behaviour 14 stays.
pub fn classify(path: &Path, text: &str, positions: &[(u64, u64)]) -> Vec<Option<ScriptUsage>> {
    if !supported(path) || text.len() > crate::syntax::MAX_SYNTAX_BYTES {
        return vec![None; positions.len()];
    }
    let Ok(doc) = crate::syntax::SyntaxDocument::new(crate::LanguageId::Go, false, text) else {
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
            of_node(n)
        })
        .collect()
}

/// The whole-word mentions of `symbol` in the comments of the project's Go files (`SPEC-0293` v12
/// behaviour 30), when `declaration` is a function, method or type declared at the top of a Go
/// file of the project (not a parameter or a local name); `file` is searched in `text` when given.
pub fn comment_mentions(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    declaration: &Location,
    symbol: &str,
) -> Vec<Location> {
    if !supported(file) || !supported(&declaration.path) {
        return Vec::new();
    }
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let own = canonical(file);
    let read = |p: &Path| match text {
        Some(t) if canonical(p) == own => Some(t.to_owned()),
        _ => std::fs::read_to_string(p).ok(),
    };
    let Some(content) = read(&declaration.path) else {
        return Vec::new();
    };
    let Ok(doc) = crate::syntax::SyntaxDocument::new(crate::LanguageId::Go, false, &content) else {
        return Vec::new();
    };
    let top = doc.definitions().into_iter().any(|d| {
        d.name == symbol
            && d.line as u64 == declaration.range.start_line
            && d.character as u64 == declaration.range.start_character
    });
    if !top {
        return Vec::new();
    }
    let found = crate::find_usages::text_occurrences(
        root,
        file,
        text,
        symbol,
        crate::find_usages::MAX_TEXT_OCCURRENCES,
    );
    let mut out = Vec::new();
    let mut paths: Vec<&std::path::PathBuf> = found.iter().map(|l| &l.path).collect();
    paths.dedup();
    for path in paths {
        let Some(content) = read(path) else { continue };
        let here: Vec<&Location> = found.iter().filter(|l| l.path == *path).collect();
        let positions: Vec<(u64, u64)> = here
            .iter()
            .map(|l| (l.range.start_line, l.range.start_character))
            .collect();
        for (l, usage) in here.into_iter().zip(classify(path, &content, &positions)) {
            if usage.is_some_and(|u| u.kind == UsageKind::Comment) {
                out.push(l.clone());
            }
        }
    }
    out
}

fn usage(kind: UsageKind) -> Option<ScriptUsage> {
    Some(ScriptUsage {
        kind,
        member_declaration: false,
        in_text: false,
    })
}

/// Kinds whose `name` declares something.
const DECLARATIONS: &[&str] = &[
    "type_spec",
    "type_alias",
    "function_declaration",
    "method_declaration",
    "const_spec",
    "var_spec",
    "field_declaration",
    "method_elem",
];

fn of_node(n: Node) -> Option<ScriptUsage> {
    let mut up = Some(n);
    while let Some(a) = up {
        match a.kind() {
            "comment" => return usage(UsageKind::Comment),
            "interpreted_string_literal" | "raw_string_literal" if !in_import(a) => {
                return Some(ScriptUsage {
                    kind: UsageKind::Other,
                    member_declaration: false,
                    in_text: true,
                });
            }
            _ => {}
        }
        up = a.parent();
    }
    if in_import(n) {
        return usage(UsageKind::Import);
    }
    // The name inside the type around it: `pkg.Rect`, `Box[T]`, `*Rect`, `(Rect)`.
    let mut e = n;
    while let Some(p) = e.parent() {
        let wraps = match p.kind() {
            "qualified_type" => is_field(p, "name", e),
            "generic_type" => is_field(p, "type", e),
            "pointer_type" | "parenthesized_type" => true,
            "selector_expression" => is_field(p, "field", e),
            _ => false,
        };
        if !wraps {
            break;
        }
        e = p;
    }
    let p = e.parent()?;
    if DECLARATIONS.contains(&p.kind()) && is_field(p, "name", e) {
        return usage(UsageKind::Declaration);
    }
    // `func (r Rect) Area()`: the type of the receiver's parameter.
    if p.kind() == "parameter_declaration"
        && is_field(p, "type", e)
        && let Some(list) = p.parent()
        && let Some(method) = list.parent()
        && method.kind() == "method_declaration"
        && is_field(method, "receiver", list)
    {
        return usage(UsageKind::Receiver);
    }
    match p.kind() {
        "composite_literal" if is_field(p, "type", e) => {
            return usage(UsageKind::StructInit);
        }
        "call_expression" if is_field(p, "function", e) => return usage(UsageKind::Call),
        "inc_statement" | "dec_statement" => return usage(UsageKind::Write),
        "expression_list" => {
            if let Some(q) = p.parent()
                && q.kind() == "assignment_statement"
                && is_field(q, "left", p)
            {
                return usage(UsageKind::Write);
            }
        }
        _ => {}
    }
    if matches!(n.kind(), "type_identifier" | "package_identifier") || e.kind().ends_with("_type") {
        return usage(UsageKind::Type);
    }
    None
}

/// `n` lies in an import declaration.
fn in_import(n: Node) -> bool {
    let mut up = Some(n);
    while let Some(a) = up {
        if matches!(a.kind(), "import_spec" | "import_declaration") {
            return true;
        }
        up = a.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, needle: &str, plus: usize) -> Option<ScriptUsage> {
        let b = text.find(needle).unwrap_or_else(|| panic!("{needle}")) + plus;
        let before = &text[..b];
        let line = before.matches('\n').count() as u64;
        let character = before.rsplit('\n').next().unwrap().chars().count() as u64;
        classify(Path::new("main.go"), text, &[(line, character)])[0]
    }

    fn kind(text: &str, needle: &str, plus: usize) -> Option<UsageKind> {
        at(text, needle, plus).map(|u| u.kind)
    }

    const SHAPES: &str = "package main\n\nimport \"fmt\"\n\n// Rect is a rectangle.\ntype Rect struct{ W, H float64 }\n\nfunc (r Rect) Area() float64 { return r.W * r.H }\n\nfunc (r *Rect) Grow() { r.W++ }\n\nfunc scale(f float64, s Rect) Rect {\n\treturn Rect{W: s.W * f, H: s.H * f}\n}\n\nfunc main() {\n\tr := Rect{W: 2, H: 3}\n\tp := &Rect{}\n\tvar many []Rect\n\tmany = append(many, scale(2, r))\n\tfmt.Println(r.Area(), p, \"Rect\", many)\n}\n";

    /// SPEC-0293 v12 AK-18 (behaviour 14b): receiver, struct initialization, type, call,
    /// declaration, import, write and comment from the Go syntax tree; a string is text.
    #[test]
    fn go_kinds_from_the_syntax_tree() {
        assert_eq!(kind(SHAPES, "Rect) Area", 0), Some(UsageKind::Receiver));
        assert_eq!(kind(SHAPES, "Rect) Grow", 0), Some(UsageKind::Receiver));
        assert_eq!(kind(SHAPES, "Rect{W: 2", 0), Some(UsageKind::StructInit));
        assert_eq!(kind(SHAPES, "Rect{W: s.W", 0), Some(UsageKind::StructInit));
        assert_eq!(kind(SHAPES, "Rect{}", 0), Some(UsageKind::StructInit));
        assert_eq!(kind(SHAPES, "Rect) Rect", 0), Some(UsageKind::Type));
        assert_eq!(kind(SHAPES, ") Rect {", 2), Some(UsageKind::Type));
        assert_eq!(kind(SHAPES, "Rect\n\tmany", 0), Some(UsageKind::Type));
        assert_eq!(kind(SHAPES, "Rect struct", 0), Some(UsageKind::Declaration));
        assert_eq!(
            kind(SHAPES, "Area() float64", 0),
            Some(UsageKind::Declaration)
        );
        assert_eq!(kind(SHAPES, "scale(2", 0), Some(UsageKind::Call));
        assert_eq!(kind(SHAPES, "Area(), p", 0), Some(UsageKind::Call));
        assert_eq!(kind(SHAPES, "Println", 0), Some(UsageKind::Call));
        assert_eq!(kind(SHAPES, "many = append", 0), Some(UsageKind::Write));
        assert_eq!(kind(SHAPES, "W++", 0), Some(UsageKind::Write));
        assert_eq!(kind(SHAPES, "fmt\"", 0), Some(UsageKind::Import));
        assert_eq!(kind(SHAPES, "Rect is a", 0), Some(UsageKind::Comment));
        let text = at(SHAPES, "Rect\", many", 0).unwrap();
        assert!(text.in_text);
        // A read of a variable keeps the line rule.
        assert_eq!(kind(SHAPES, "many)", 0), None);
        // Other languages are not read here.
        assert_eq!(
            classify(Path::new("a.rs"), "struct Rect;", &[(0, 7)]),
            [None]
        );
    }
}
