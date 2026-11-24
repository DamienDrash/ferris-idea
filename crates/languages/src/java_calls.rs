//! Two places where jdtls answers less than a Java developer expects, mended from the syntax tree
//! (`SPEC-0024` v2 behaviour 8, `SPEC-0082` v8 behaviour 19; OBS-0071 O4):
//!
//! - On the `::` of a method reference or the first character of the name after it, jdtls
//!   describes the functional interface's method (`ToIntFunction.applyAsInt`) instead of the
//!   referenced one; one character further into the name it names the referenced method.
//!   [`method_reference_character`] gives that position.
//! - The outgoing calls of a method leave out `new R(…)` when `R`'s constructor is not written in
//!   the source (a record's canonical constructor). [`creations`] lists the `new` expressions of a
//!   method's body, so the caller can add the ones the server did not report.
//!
//! Positions are 0-based lines and characters counted in Unicode scalar values, as in
//! [`crate::insight`].

use crate::insight::{Range, line_of};

/// Whether `c` belongs to a Java name.
fn name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The character to hover at instead of `character` in `line`, when `character` is on the `::` of
/// a method reference (`Item::totalCents`, `String::length`, `Item::new`) or on the first character
/// of the name after it: the second character of that name, or the end of a one-letter name.
/// `None` anywhere else, the position then stays as it is.
pub fn method_reference_character(line: &str, character: u64) -> Option<u64> {
    let chars: Vec<char> = line.chars().collect();
    let at = usize::try_from(character).ok()?;
    if at >= chars.len() {
        return None;
    }
    // The start of the name the position belongs to, when it is on `::` or the name's start.
    let name_start = if chars[at] == ':' {
        let colons = if chars.get(at + 1) == Some(&':') {
            at
        } else if at > 0 && chars[at - 1] == ':' {
            at - 1
        } else {
            return None;
        };
        let mut i = colons + 2;
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        i
    } else if name_char(chars[at]) && (at == 0 || !name_char(chars[at - 1])) {
        // The name must follow `::`, spaces allowed between.
        let mut i = at;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        if i < 2 || chars[i - 1] != ':' || chars[i - 2] != ':' {
            return None;
        }
        at
    } else {
        return None;
    };
    if !chars.get(name_start).copied().is_some_and(name_char) {
        return None;
    }
    Some(name_start as u64 + 1)
}

/// A `new` expression in a method body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creation {
    /// The whole expression, from `new` (or the outer instance of `outer.new Inner()`) to the
    /// closing parenthesis or class body.
    pub range: Range,
    /// The start of the created type's simple name (`Rec` in `new demo.Rec<>(…)`), where the server
    /// is asked what the expression calls.
    pub name_line: u64,
    /// See [`Self::name_line`].
    pub name_character: u64,
}

/// The character of byte column `column` on line `row` of `text`.
fn character_of(text: &str, row: usize, column: usize) -> u64 {
    let line = line_of(text, row as u64);
    line.get(..column.min(line.len()))
        .map_or(0, |s| s.chars().count() as u64)
}

/// The byte offset of (`line`, `character`) in `text`; the text's end past it.
fn offset_of(text: &str, line: u64, character: u64) -> usize {
    let mut offset = 0;
    for (n, l) in text.split('\n').enumerate() {
        if n as u64 == line {
            return offset
                + l.char_indices()
                    .nth(usize::try_from(character).unwrap_or(usize::MAX))
                    .map_or(l.len(), |(i, _)| i);
        }
        offset += l.len() + 1;
    }
    text.len()
}

/// The `new` expressions of the Java method or constructor whose name starts at (`line`,
/// `character`) in `text`, in source order; empty when no method or constructor is named there.
/// Expressions inside the body of an anonymous or local class are left out: they belong to that
/// class's members, not to this one.
pub fn creations(text: &str, line: u64, character: u64) -> Vec<Creation> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let at = offset_of(text, line, character);
    let mut node = tree.root_node().descendant_for_byte_range(at, at);
    let member = loop {
        match node {
            Some(n)
                if matches!(
                    n.kind(),
                    "method_declaration"
                        | "constructor_declaration"
                        | "compact_constructor_declaration"
                ) =>
            {
                break n;
            }
            Some(n) if n.kind() == "class_body" || n.kind() == "program" => return Vec::new(),
            Some(n) => node = n.parent(),
            None => return Vec::new(),
        }
    };
    let mut found = Vec::new();
    let mut stack = vec![member];
    while let Some(node) = stack.pop() {
        if node.kind() == "class_body" {
            continue;
        }
        if node.kind() == "object_creation_expression"
            && let Some(ty) = node.child_by_field_name("type")
            && let Some(name) = simple_name(ty)
        {
            let (s, e, n) = (
                node.start_position(),
                node.end_position(),
                name.start_position(),
            );
            found.push(Creation {
                range: Range {
                    start_line: s.row as u64,
                    start_character: character_of(text, s.row, s.column),
                    end_line: e.row as u64,
                    end_character: character_of(text, e.row, e.column),
                },
                name_line: n.row as u64,
                name_character: character_of(text, n.row, n.column),
            });
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    found
}

/// The simple name of a type node: `Rec` of `Rec`, `demo.Rec`, `Rec<String>`, `@A Rec`.
fn simple_name(ty: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    match ty.kind() {
        "type_identifier" => Some(ty),
        "generic_type" | "scoped_type_identifier" | "annotated_type" => {
            let mut cursor = ty.walk();
            let children: Vec<_> = ty.named_children(&mut cursor).collect();
            let last = match ty.kind() {
                // `Outer.Inner<T>`: the type before the arguments.
                "generic_type" => children.into_iter().find(|c| c.kind() != "type_arguments"),
                _ => children.into_iter().last(),
            }?;
            simple_name(last)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_references_hover_one_character_into_the_name() {
        let line = "        return items.stream().mapToInt(Item::totalCents).sum();";
        let colons = line.find("::").unwrap() as u64;
        let name = colons + 2;
        // On either colon and on the name's first character: the second character of the name.
        for at in [colons, colons + 1, name] {
            assert_eq!(method_reference_character(line, at), Some(name + 1), "{at}");
        }
        // Further into the name, on the type before `::`, elsewhere: unchanged.
        for at in [name + 1, name + 5, colons - 1, colons - 4, 0, 99] {
            assert_eq!(method_reference_character(line, at), None, "{at}");
        }
        // `::new`, a one-letter name, spaces after `::`.
        let line = "Supplier<StringBuilder> g = StringBuilder::new;";
        let name = line.find("new").unwrap() as u64;
        assert_eq!(method_reference_character(line, name), Some(name + 1));
        let line = "f = Shop::x;";
        let name = line.find('x').unwrap() as u64;
        assert_eq!(method_reference_character(line, name), Some(name + 1));
        assert_eq!(method_reference_character(line, name - 1), Some(name + 1));
        let line = "f = Shop:: x;";
        let name = line.find('x').unwrap() as u64;
        assert_eq!(method_reference_character(line, name), Some(name + 1));
        assert_eq!(method_reference_character(line, name - 3), Some(name + 1));
        // A label or a ternary colon is not a method reference.
        let line = "a ? b : c; loop: for (;;) {}";
        for (i, _) in line.char_indices() {
            assert_eq!(method_reference_character(line, i as u64), None, "{i}");
        }
        // Characters, not bytes: an umlaut before the reference.
        let line = "ü = Item::total;";
        let name = line[..line.find("total").unwrap()].chars().count() as u64;
        assert_eq!(name, 10);
        assert_eq!(method_reference_character(line, name), Some(name + 1));
    }

    #[test]
    fn creations_of_a_method_body() {
        let text = "class Shop {\n    record Rec(String name) {}\n    void main() {\n        Rec r = new Rec(\"x\");\n        var l = java.util.List.of(new demo.Item<>(1), new Outer.In());\n        Runnable run = new Runnable() { public void run() { new Hidden(); } };\n        über(new Rec(\"ü\"));\n    }\n    void other() { new Elsewhere(); }\n}\n";
        let found = creations(text, 2, 9);
        let shown: Vec<(u64, u64, u64, u64)> = found
            .iter()
            .map(|c| {
                (
                    c.range.start_line,
                    c.range.start_character,
                    c.name_line,
                    c.name_character,
                )
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                (3, 16, 3, 20),
                (4, 34, 4, 43),
                (4, 54, 4, 64),
                (5, 23, 5, 27),
                (6, 13, 6, 17),
            ],
            "{found:?}"
        );
        assert_eq!(found[0].range.end_character, 28);
        // A member of the class is not the class: `other` has its own.
        assert_eq!(creations(text, 8, 9).len(), 1);
        // On a type's name or a field there is no member to look into.
        assert!(creations(text, 1, 11).is_empty());
        assert!(creations(text, 0, 6).is_empty());
    }
}
