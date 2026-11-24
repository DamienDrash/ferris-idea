//! The ▶ marker on the `main` of a Go program and of a Rust binary (`SPEC-0097` v3 behaviour 8):
//! `func main()` of a `package main` file and the top-level `fn main` of `src/main.rs`,
//! `src/bin/*.rs`, `src/bin/*/main.rs`, `examples/*.rs` and `examples/*/main.rs`; and of a Python
//! script (`SPEC-0097` v5 behaviour 11): its top-level `if __name__ == "__main__":`. Read from the
//! syntax tree, without a language server; nothing runs to find them.
//!
//! The markers use [`RunMarker`] with [`MarkerKind::Main`]; `class` is empty (what runs is the
//! file's program, `SPEC-0097` behaviour 3).

use std::path::Path;

use tree_sitter::Node;

use crate::LanguageId;
use crate::jvm::{MarkerKind, RunMarker};
use crate::syntax::SyntaxDocument;

fn children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

/// Whether `rel` (workspace-relative, `/`-separated) is the root of a Cargo binary or example
/// target by Cargo's default layout.
pub fn is_rust_binary(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    matches!(
        parts.as_slice(),
        [.., "src", "main.rs"] | [.., "src", "bin", _, "main.rs"] | [.., "examples", _, "main.rs"]
    ) || matches!(
        parts.as_slice(),
        [.., "src", "bin", f] | [.., "examples", f] if f.ends_with(".rs")
    )
}

/// Whether `rel` can hold a `main` marker: a Go source that is not a test, a Rust binary root, or
/// a Python source that is not a pytest file.
pub fn applies(rel: &str) -> bool {
    let path = Path::new(rel);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    match path.extension().and_then(|e| e.to_str()) {
        Some("go") => !stem.ends_with("_test"),
        Some("rs") => is_rust_binary(rel),
        Some("py") => crate::test_markers::test_file(rel).is_none(),
        _ => false,
    }
}

fn main_marker(line: usize) -> RunMarker {
    RunMarker {
        line,
        kind: MarkerKind::Main,
        class: String::new(),
        method: None,
    }
}

/// The `main` marker of `rel` with content `text`, if it has one.
pub fn markers(rel: &str, text: &str) -> Vec<RunMarker> {
    if !applies(rel) {
        return Vec::new();
    }
    let language = if rel.ends_with(".go") {
        LanguageId::Go
    } else if rel.ends_with(".py") {
        LanguageId::Python
    } else {
        LanguageId::Rust
    };
    let Ok(doc) = SyntaxDocument::new(language, false, text) else {
        return Vec::new();
    };
    let root = doc.tree().root_node();
    let found = match language {
        LanguageId::Go => go(root, text),
        LanguageId::Python => python(root, text),
        _ => rust(root, text),
    };
    found.into_iter().map(main_marker).collect()
}

/// The markers of a Go or Rust file: its `main` and its tests (`SPEC-0045` v5 behaviour 13), in
/// document order.
pub fn with_tests(rel: &str, text: &str) -> Vec<RunMarker> {
    let mut all = markers(rel, text);
    all.extend(crate::test_markers::markers(rel, text));
    all.sort_by_key(|m| m.line);
    all.dedup_by_key(|m| m.line);
    all
}

/// Go runs `func main()` — no parameters, no results — of the package `main`.
fn go(root: Node, text: &str) -> Option<usize> {
    let items = children(root);
    let package = items
        .iter()
        .find(|n| n.kind() == "package_clause")
        .and_then(|p| {
            children(*p)
                .into_iter()
                .find(|c| c.kind() == "package_identifier")
        })
        .map(|p| word(p, text));
    if package != Some("main") {
        return None;
    }
    items.into_iter().find_map(|n| {
        let name = n.child_by_field_name("name")?;
        let params = n.child_by_field_name("parameters")?;
        (n.kind() == "function_declaration"
            && word(name, text) == "main"
            && n.child_by_field_name("type_parameters").is_none()
            && n.child_by_field_name("result").is_none()
            && word(params, text).split_whitespace().collect::<String>() == "()")
            .then(|| name.start_position().row)
    })
}

/// Python runs a script whose top level asks `if __name__ == "__main__":` (either quotes, either
/// side); the marker stands on that line.
fn python(root: Node, text: &str) -> Option<usize> {
    children(root).into_iter().find_map(|n| {
        let condition = n.child_by_field_name("condition")?;
        let plain: String = word(condition, text)
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| if c == '\'' { '"' } else { c })
            .collect();
        (n.kind() == "if_statement"
            && (plain == "__name__==\"__main__\"" || plain == "\"__main__\"==__name__"))
            .then(|| n.start_position().row)
    })
}

/// Rust runs the top-level `fn main()` of a binary root (a return type such as `Result` is fine).
fn rust(root: Node, text: &str) -> Option<usize> {
    children(root).into_iter().find_map(|n| {
        let name = n.child_by_field_name("name")?;
        let params = n.child_by_field_name("parameters")?;
        (n.kind() == "function_item"
            && word(name, text) == "main"
            && n.child_by_field_name("type_parameters").is_none()
            && word(params, text).split_whitespace().collect::<String>() == "()")
            .then(|| name.start_position().row)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(m: &[RunMarker]) -> Vec<(usize, MarkerKind)> {
        m.iter().map(|m| (m.line, m.kind)).collect()
    }

    /// SPEC-0097 v3 AK-5 (behaviour 8): `func main()` of `package main` has a marker; another
    /// package, a test file, a method or a `main` with parameters do not.
    #[test]
    fn go_main() {
        let text = "package main\n\nimport \"fmt\"\n\ntype Rect struct{ W, H int }\n\nfunc (r Rect) main() {}\n\nfunc main() {\n\tfmt.Println(Rect{2, 3})\n}\n";
        assert_eq!(lines(&markers("shapes.go", text)), [(8, MarkerKind::Main)]);
        assert_eq!(lines(&markers("cmd/tool/main.go", text)).len(), 1);
        assert!(markers("shapes.go", &text.replace("package main", "package shapes")).is_empty());
        assert!(markers("shapes_test.go", text).is_empty());
        assert!(markers("shapes.go", "package main\n\nfunc main(x int) {}\n").is_empty());
        assert!(markers("shapes.go", "package main\n\nfunc helper() {}\n").is_empty());
        // Broken text does not panic and gives what it can.
        assert!(markers("shapes.go", "package main\nfunc main( {").len() <= 1);
    }

    /// SPEC-0097 v3 AK-5 (behaviour 8): the top-level `fn main` of binary roots only; tests of the
    /// same file keep their own markers.
    #[test]
    fn rust_main_in_binary_roots() {
        let text = "struct Rect { w: u32, h: u32 }\n\nfn main() -> Result<(), String> {\n    println!(\"{}\", Rect { w: 2, h: 3 }.w);\n    Ok(())\n}\n\n#[cfg(test)]\nmod tests {\n    fn main() {}\n\n    #[test]\n    fn area() {}\n}\n";
        for rel in [
            "src/main.rs",
            "crates/shapes/src/main.rs",
            "src/bin/tool.rs",
            "src/bin/tool/main.rs",
            "examples/demo.rs",
            "examples/demo/main.rs",
        ] {
            assert_eq!(lines(&markers(rel, text)), [(2, MarkerKind::Main)], "{rel}");
        }
        for rel in [
            "src/lib.rs",
            "src/shapes.rs",
            "tests/it.rs",
            "build.rs",
            "src/bin/tool/util.rs",
        ] {
            assert!(markers(rel, text).is_empty(), "{rel}");
        }
        assert_eq!(
            lines(&with_tests("src/main.rs", text)),
            [(2, MarkerKind::Main), (12, MarkerKind::TestMethod)]
        );
        assert!(markers("src/main.rs", "fn main(args: Vec<String>) {}\n").is_empty());
        assert!(markers("src/main.rs", "mod a { fn main() {} }\n").is_empty());
    }

    /// SPEC-0097 v5 AK-8 (behaviour 11): the top-level `if __name__ == "__main__":` of a Python
    /// script has the marker; one inside a function, another condition or a pytest file do not.
    #[test]
    fn python_main_clause() {
        let text = "from shop.inventory import total_value\n\n\nif __name__ == \"__main__\":\n    print(total_value([(4, 10)]))\n";
        assert_eq!(lines(&markers("main.py", text)), [(3, MarkerKind::Main)]);
        let single = "import sys\nif '__main__' == __name__ :\n    sys.exit(0)\n";
        assert_eq!(
            lines(&markers("tools/run.py", single)),
            [(1, MarkerKind::Main)]
        );
        assert!(
            markers(
                "main.py",
                "def f():\n    if __name__ == \"__main__\":\n        pass\n"
            )
            .is_empty()
        );
        assert!(markers("main.py", "if __name__ != \"__main__\":\n    pass\n").is_empty());
        assert!(markers("main.py", "print(1)\n").is_empty());
        assert!(markers("test_inventory.py", text).is_empty());
        assert_eq!(lines(&with_tests("main.py", text)), [(3, MarkerKind::Main)]);
    }

    #[test]
    fn which_files_apply() {
        assert!(applies("shapes.go"));
        assert!(!applies("shapes_test.go"));
        assert!(applies("src/main.rs"));
        assert!(!applies("src/lib.rs"));
        assert!(applies("main.py"));
        assert!(!applies("test_main.py"));
        assert!(!applies("calc_test.py"));
    }
}
