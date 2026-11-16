//! The ▶ markers of the tests in pytest, Go and Rust files (`SPEC-0045` v5 behaviour 13), and the
//! test the caret is in (OBS-0034 part 15, R1): a run from the context with the caret inside a test
//! runs only that test.
//!
//! The markers use [`RunMarker`] as the JVM and JavaScript ones do: `kind` is a test method (one
//! test) or a test class (a pytest class); `class` is the pytest class or the Rust module path
//! inside the file (`tests` of `mod tests`), `method` the test's name.

use std::path::Path;

use tree_sitter::Node;

use crate::LanguageId;
use crate::jvm::{MarkerKind, RunMarker};
use crate::syntax::SyntaxDocument;

/// Which test runner a file's tests belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestFile {
    /// `test_*.py` or `*_test.py`.
    Pytest,
    /// `*_test.go`.
    Go,
    /// A Rust file with `#[test]` functions.
    Rust,
}

/// The runner of `rel`'s tests, by its name (Rust: any `.rs` file, the markers decide).
pub fn test_file(rel: &str) -> Option<TestFile> {
    let path = Path::new(rel);
    let stem = path.file_stem()?.to_string_lossy();
    match path.extension()?.to_str()? {
        "py" if stem.starts_with("test_") || stem.ends_with("_test") => Some(TestFile::Pytest),
        "go" if stem.ends_with("_test") => Some(TestFile::Go),
        "rs" => Some(TestFile::Rust),
        _ => None,
    }
}

fn children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

fn name_of<'a>(n: Node, text: &'a str) -> Option<(&'a str, usize)> {
    let name = n.child_by_field_name("name")?;
    Some((word(name, text), name.start_position().row))
}

/// The test markers of `rel` with content `text`, in document order.
pub fn markers(rel: &str, text: &str) -> Vec<RunMarker> {
    let mut out = Vec::new();
    let Some(kind) = test_file(rel) else {
        return out;
    };
    let language = match kind {
        TestFile::Pytest => LanguageId::Python,
        TestFile::Go => LanguageId::Go,
        TestFile::Rust => LanguageId::Rust,
    };
    let Ok(doc) = SyntaxDocument::new(language, false, text) else {
        return out;
    };
    let root = doc.tree().root_node();
    match kind {
        TestFile::Pytest => pytest(root, text, &mut out),
        TestFile::Go => go(root, text, &mut out),
        TestFile::Rust => rust(root, text, &mut Vec::new(), &mut out),
    }
    out
}

fn test(line: usize, class: String, method: &str) -> RunMarker {
    RunMarker {
        line,
        kind: MarkerKind::TestMethod,
        class,
        method: Some(method.to_owned()),
    }
}

/// The definition of a top-level or class item (`@decorator` before it skipped).
fn definition(n: Node<'_>) -> Option<Node<'_>> {
    match n.kind() {
        "decorated_definition" => n.child_by_field_name("definition"),
        _ => Some(n),
    }
}

/// pytest collects `test*` functions of the module and `test*` methods of `Test*` classes without
/// an `__init__` (pytest's default naming rules).
fn pytest(root: Node, text: &str, out: &mut Vec<RunMarker>) {
    for top in children(root) {
        let Some(n) = definition(top) else {
            continue;
        };
        match n.kind() {
            "function_definition" => {
                if let Some((name, line)) = name_of(n, text)
                    && name.starts_with("test")
                {
                    out.push(test(line, String::new(), name));
                }
            }
            "class_definition" => {
                let Some((class, line)) = name_of(n, text) else {
                    continue;
                };
                let Some(body) = n.child_by_field_name("body") else {
                    continue;
                };
                let methods: Vec<Node> =
                    children(body).into_iter().filter_map(definition).collect();
                let has_init = methods.iter().any(|m| {
                    m.kind() == "function_definition"
                        && name_of(*m, text).is_some_and(|(n, _)| n == "__init__")
                });
                if !class.starts_with("Test") || has_init {
                    continue;
                }
                out.push(RunMarker {
                    line,
                    kind: MarkerKind::TestClass,
                    class: class.to_owned(),
                    method: None,
                });
                for m in methods {
                    if m.kind() == "function_definition"
                        && let Some((name, line)) = name_of(m, text)
                        && name.starts_with("test")
                    {
                        out.push(test(line, class.to_owned(), name));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Go runs `func TestXxx(t *testing.T)` of a `_test.go` file.
fn go(root: Node, text: &str, out: &mut Vec<RunMarker>) {
    for n in children(root) {
        if n.kind() != "function_declaration" {
            continue;
        }
        let Some((name, line)) = name_of(n, text) else {
            continue;
        };
        let after = name.strip_prefix("Test");
        // `Test` alone or `TestX` with X not a lower-case letter (Go's rule).
        let named = after.is_some_and(|a| !a.starts_with(|c: char| c.is_lowercase()));
        let params = n
            .child_by_field_name("parameters")
            .map(|p| word(p, text))
            .unwrap_or_default();
        if named && params.contains("testing.T") {
            out.push(test(line, String::new(), name));
        }
    }
}

/// Rust runs the functions with `#[test]` (also `#[tokio::test]` and the like), in the module
/// path of their inline `mod`s.
fn rust(n: Node, text: &str, modules: &mut Vec<String>, out: &mut Vec<RunMarker>) {
    let items = children(n);
    for (i, item) in items.iter().enumerate() {
        match item.kind() {
            "function_item" => {
                // The attributes stand right before the function.
                let attributes = items[..i]
                    .iter()
                    .rev()
                    .take_while(|a| matches!(a.kind(), "attribute_item" | "line_comment"))
                    .filter(|a| a.kind() == "attribute_item")
                    .map(|a| word(*a, text))
                    .collect::<Vec<_>>();
                let is_test = attributes.iter().any(|a| {
                    let inner = a.trim_start_matches("#[").trim_end_matches(']').trim();
                    inner == "test"
                        || inner.ends_with("::test")
                        || inner.starts_with("test(")
                        || inner.contains("::test(")
                });
                if is_test && let Some((name, line)) = name_of(*item, text) {
                    out.push(test(line, modules.join("::"), name));
                }
            }
            "mod_item" => {
                if let (Some((name, _)), Some(body)) =
                    (name_of(*item, text), item.child_by_field_name("body"))
                {
                    modules.push(name.to_owned());
                    rust(body, text, modules, out);
                    modules.pop();
                }
            }
            _ => {}
        }
    }
}

/// The test whose definition holds the 0-based `line` (a pytest class too); `None` outside one.
pub fn marker_at(rel: &str, text: &str, line: usize) -> Option<RunMarker> {
    let all = markers(rel, text);
    if all.is_empty() {
        return None;
    }
    let kind = test_file(rel)?;
    let language = match kind {
        TestFile::Pytest => LanguageId::Python,
        TestFile::Go => LanguageId::Go,
        TestFile::Rust => LanguageId::Rust,
    };
    let doc = SyntaxDocument::new(language, false, text).ok()?;
    let at = tree_sitter::Point::new(line, 0);
    let end = tree_sitter::Point::new(line, text.lines().nth(line).map_or(0, str::len));
    let mut node = doc.tree().root_node().descendant_for_point_range(at, end)?;
    // From the innermost definition outwards: the first one that is a marker.
    loop {
        if matches!(
            node.kind(),
            "function_definition" | "class_definition" | "function_declaration" | "function_item"
        ) && let Some((_, name_line)) = name_of(node, text)
            && let Some(m) = all.iter().find(|m| m.line == name_line)
        {
            return Some(m.clone());
        }
        node = node.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(markers: &[RunMarker]) -> Vec<(usize, &str, Option<&str>)> {
        markers
            .iter()
            .map(|m| (m.line, m.class.as_str(), m.method.as_deref()))
            .collect()
    }

    /// OBS-0034 item 62 (R1): `test_prices.py` with a test function, a test class and helpers.
    #[test]
    fn pytest_markers_and_the_test_at_the_caret() {
        let text = "import pytest\n\n\ndef helper():\n    return 1\n\n\ndef test_total():\n    assert helper() == 1\n\n\n@pytest.mark.slow\ndef test_slow():\n    pass\n\n\nclass TestCart:\n    def test_empty(self):\n        assert True\n\n    def helper(self):\n        pass\n\n\nclass TestWithInit:\n    def __init__(self):\n        pass\n\n    def test_never(self):\n        pass\n";
        let m = markers("tests/test_prices.py", text);
        assert_eq!(
            named(&m),
            [
                (7, "", Some("test_total")),
                (12, "", Some("test_slow")),
                (16, "TestCart", None),
                (17, "TestCart", Some("test_empty")),
            ]
        );
        assert_eq!(m[2].kind, MarkerKind::TestClass);
        // The caret in `test_total`, in `test_empty`, in the class line, outside any test.
        let at = |line| marker_at("tests/test_prices.py", text, line).map(|m| (m.class, m.method));
        assert_eq!(at(8), Some((String::new(), Some("test_total".into()))));
        assert_eq!(at(18), Some(("TestCart".into(), Some("test_empty".into()))));
        assert_eq!(at(21), Some(("TestCart".into(), None)));
        assert_eq!(at(0), None);
        assert_eq!(at(4), None);
        // Not a test file.
        assert!(markers("app/prices.py", text).is_empty());
    }

    #[test]
    fn go_and_rust_markers() {
        let go = "package shop\n\nimport \"testing\"\n\nfunc TestTotal(t *testing.T) {\n\tif total() != 1 {\n\t\tt.Fail()\n\t}\n}\n\nfunc Testable(t *testing.T) {}\n\nfunc helper() {}\n";
        assert_eq!(
            named(&markers("shop/total_test.go", go)),
            [(4, "", Some("TestTotal"))]
        );
        assert_eq!(
            marker_at("shop/total_test.go", go, 6).and_then(|m| m.method),
            Some("TestTotal".into())
        );
        assert!(markers("shop/total.go", go).is_empty());
        let rs = "pub fn add(a: i32) -> i32 { a }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn adds() {\n        assert_eq!(add(1), 1);\n    }\n\n    #[tokio::test]\n    async fn waits() {}\n\n    mod inner {\n        #[test]\n        fn deep() {}\n    }\n\n    fn helper() {}\n}\n\n#[test]\nfn top() {}\n";
        assert_eq!(
            named(&markers("src/lib.rs", rs)),
            [
                (7, "tests", Some("adds")),
                (12, "tests", Some("waits")),
                (16, "tests::inner", Some("deep")),
                (23, "", Some("top")),
            ]
        );
        assert_eq!(
            marker_at("src/lib.rs", rs, 8).map(|m| (m.class, m.method)),
            Some(("tests".into(), Some("adds".into())))
        );
        assert_eq!(marker_at("src/lib.rs", rs, 0), None);
        assert_eq!(marker_at("src/lib.rs", rs, 19), None);
    }
}
