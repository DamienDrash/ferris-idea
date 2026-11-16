//! ▶ markers at the tests of Vitest and Jest files (`SPEC-0045` v4 behaviour 12): `describe`, `it`
//! and `test` calls with a literal name, read from the syntax tree.

use std::path::Path;

use tree_sitter::Node;

use crate::LanguageId;
use crate::jvm::{MarkerKind, RunMarker};
use crate::syntax::SyntaxDocument;

/// A call with a test's name, and the last line of the call.
struct Found {
    marker: RunMarker,
    end_line: usize,
}

/// What a callee's leftmost name makes of the call: `Some(true)` for a suite.
fn suite_word(word: &str) -> Option<bool> {
    match word {
        "describe" | "suite" | "fdescribe" | "xdescribe" => Some(true),
        "it" | "test" | "fit" | "xit" => Some(false),
        _ => None,
    }
}

/// The leftmost identifier of `describe.only`, `it.each([..])` or `` test.each`..` ``.
fn callee_word<'a>(mut node: Node<'_>, text: &'a str) -> Option<&'a str> {
    loop {
        node = match node.kind() {
            "identifier" => return node.utf8_text(text.as_bytes()).ok(),
            "member_expression" => node.child_by_field_name("object")?,
            "call_expression" => node.child_by_field_name("function")?,
            _ => return None,
        };
    }
}

/// The text of a string literal or of a template without `${}`.
fn literal(node: Node<'_>, text: &str) -> Option<String> {
    if !matches!(node.kind(), "string" | "template_string") {
        return None;
    }
    let mut cursor = node.walk();
    if node
        .named_children(&mut cursor)
        .any(|c| c.kind() == "template_substitution")
    {
        return None;
    }
    let raw = node.utf8_text(text.as_bytes()).ok()?;
    let inner = raw.get(1..raw.len().checked_sub(1)?)?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    Some(out)
}

fn visit(node: Node<'_>, text: &str, suites: &mut Vec<String>, out: &mut Vec<Found>) {
    if node.kind() == "call_expression"
        && let Some(suite) = node
            .child_by_field_name("function")
            .and_then(|f| callee_word(f, text))
            .and_then(suite_word)
        && let Some(args) = node.child_by_field_name("arguments")
        && let Some(name) = args.named_child(0).and_then(|n| literal(n, text))
    {
        suites.push(name);
        out.push(Found {
            marker: RunMarker {
                line: node.start_position().row,
                kind: if suite {
                    MarkerKind::TestClass
                } else {
                    MarkerKind::TestMethod
                },
                class: suites.join(" "),
                method: None,
            },
            end_line: node.end_position().row,
        });
        let mut cursor = args.walk();
        for child in args.named_children(&mut cursor) {
            visit(child, text, suites, out);
        }
        suites.pop();
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(child, text, suites, out);
    }
}

fn scan(rel: &str, text: &str) -> Vec<Found> {
    if !idecraft_core::is_js_test_file(rel) {
        return Vec::new();
    }
    let ext = Path::new(rel)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let (language, tsx) = match ext.as_str() {
        "ts" | "mts" | "cts" => (LanguageId::TypeScript, false),
        "tsx" => (LanguageId::TypeScript, true),
        _ => (LanguageId::JavaScript, false),
    };
    let Ok(doc) = SyntaxDocument::new(language, tsx, text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    visit(doc.tree().root_node(), text, &mut Vec::new(), &mut out);
    out
}

/// The markers of test file `rel`; `class` is the test's full name, a suite's names and its own
/// joined by spaces. Of several tests on one line the first stays.
pub fn markers(rel: &str, text: &str) -> Vec<RunMarker> {
    let mut out: Vec<RunMarker> = scan(rel, text).into_iter().map(|f| f.marker).collect();
    out.sort_by_key(|m| m.line);
    out.dedup_by_key(|m| m.line);
    out
}

/// The innermost test or suite around 0-based `line` (⌃⇧R runs only it), if any.
pub fn marker_at(rel: &str, text: &str, line: usize) -> Option<RunMarker> {
    scan(rel, text)
        .into_iter()
        .filter(|f| (f.marker.line..=f.end_line).contains(&line))
        .max_by_key(|f| f.marker.line)
        .map(|f| f.marker)
}

/// Whether the `package.json` nearest to `file`, inside `root`, depends on Vitest or Jest.
pub fn has_test_runner(root: &Path, file: &Path) -> bool {
    let manifest = file
        .ancestors()
        .skip(1)
        .take_while(|d| d.starts_with(root))
        .map(|d| d.join("package.json"))
        .find(|p| p.is_file());
    let Some(json) = manifest
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        return false;
    };
    ["dependencies", "devDependencies"].iter().any(|group| {
        ["vitest", "jest"]
            .iter()
            .any(|name| json[group].get(name).is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r#"import { describe, it, test, expect } from "vitest";

describe("math", () => {
  it("adds", () => {
    expect(1).toBe(1);
  });
  describe.only('in depth', () => {
    test.skip(`deep (x)`, () => {});
  });
});

it.each([1, 2])("each %i", (n) => {});
test(`dynamic ${1}`, () => {});
const it2 = compute("x");
expect("not a test");
"#;

    fn summary(rel: &str, text: &str) -> Vec<(usize, MarkerKind, String)> {
        markers(rel, text)
            .into_iter()
            .map(|m| (m.line, m.kind, m.class))
            .collect()
    }

    /// SPEC-0045 v4 AK-11.
    #[test]
    fn suites_and_tests_with_their_full_names() {
        assert_eq!(
            summary("test/util.test.ts", TEXT),
            [
                (2, MarkerKind::TestClass, "math".to_owned()),
                (3, MarkerKind::TestMethod, "math adds".to_owned()),
                (6, MarkerKind::TestClass, "math in depth".to_owned()),
                (
                    7,
                    MarkerKind::TestMethod,
                    "math in depth deep (x)".to_owned()
                ),
                (11, MarkerKind::TestMethod, "each %i".to_owned()),
            ]
        );
    }

    #[test]
    fn javascript_and_tsx_files_too_other_files_not() {
        let text = "test('a', () => {});\n";
        assert_eq!(summary("a.spec.js", text).len(), 1);
        assert_eq!(
            summary("a.test.tsx", "test('a', () => <b>x</b>);\n").len(),
            1
        );
        assert!(summary("src/a.ts", text).is_empty());
    }

    #[test]
    fn the_test_around_a_line_is_the_innermost() {
        let at = |line| marker_at("a.test.ts", TEXT, line).map(|m| m.class);
        assert_eq!(at(4).as_deref(), Some("math adds"));
        assert_eq!(at(9).as_deref(), Some("math"));
        assert_eq!(at(7).as_deref(), Some("math in depth deep (x)"));
        assert_eq!(at(0), None);
    }

    #[test]
    fn the_nearest_manifest_names_the_runner() {
        let dir = std::env::temp_dir().join(format!("js-tests-{}", std::process::id()));
        let root = dir.as_path();
        std::fs::create_dir_all(root.join("pkg/test")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"devDependencies": {"vitest": "2"}}"#,
        )
        .unwrap();
        assert!(has_test_runner(root, &root.join("test.spec.js")));
        std::fs::write(root.join("pkg/package.json"), r#"{"dependencies": {}}"#).unwrap();
        assert!(!has_test_runner(root, &root.join("pkg/test/a.test.js")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
