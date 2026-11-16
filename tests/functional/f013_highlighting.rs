//! F-013 · Syntax highlighting: colours by language (SPEC-0027 AK-1, AK-2, AK-5).

use crate::harness::*;
use serde_json::json;

fn kinds_at(tokens: &serde_json::Value, line: u64, start: u64) -> Vec<String> {
    tokens
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["line"] == line && t["start_character"] == start)
        .map(|t| text(&t["kind"]))
        .collect()
}

#[test]
fn rust_keywords_functions_strings_and_comments_at_character_offsets() {
    let p = Project::empty();
    p.write(
        "src/main.rs",
        "// Grüße\nfn grüß() -> &'static str { let s = \"ü\"; s }\n",
    );
    let mut ide = Ide::stdio(&p);
    let tokens = ide.ok("highlight_file", json!({"path": "src/main.rs"}));
    assert!(
        kinds_at(&tokens, 0, 0).contains(&"comment".into()),
        "{tokens}"
    );
    assert!(
        kinds_at(&tokens, 1, 0).contains(&"keyword".into()),
        "{tokens}"
    );
    assert!(
        kinds_at(&tokens, 1, 3)
            .iter()
            .any(|k| k.starts_with("function")),
        "{tokens}"
    );
    // `let` after `{ ` at character 28, the literal at 36 (characters, not bytes).
    assert!(
        kinds_at(&tokens, 1, 28).contains(&"keyword".into()),
        "{tokens}"
    );
    assert!(
        kinds_at(&tokens, 1, 36).contains(&"string".into()),
        "{tokens}"
    );
}

#[test]
fn every_reference_language_highlights_and_lines_can_be_limited() {
    let cases = [
        ("python-basic", "src/inventory/model.py"),
        ("java-basic", "src/main/java/lab/inventory/Inventory.java"),
        ("typescript-basic", "src/inventory.ts"),
        ("php-basic", "src/Inventory.php"),
        ("sql-basic", "schema.sql"),
    ];
    for (project, file) in cases {
        let p = Project::reference(project);
        let mut ide = Ide::stdio(&p);
        let tokens = ide.ok("highlight_file", json!({"path": file}));
        let tokens = tokens.as_array().unwrap();
        assert!(
            tokens.iter().any(|t| t["kind"] == "keyword"),
            "{file}: no keyword"
        );
        let limited = ide.ok(
            "highlight_file",
            json!({"path": file, "start_line": 2, "end_line": 3}),
        );
        assert!(
            limited
                .as_array()
                .unwrap()
                .iter()
                .all(|t| (2..=3).contains(&t["line"].as_u64().unwrap())),
            "{file}: {limited}"
        );
    }
}

#[test]
fn a_file_without_highlighting_is_refused_with_a_reason() {
    let p = Project::empty();
    p.write("notes.unknownext", "x");
    let mut ide = Ide::stdio(&p);
    assert!(
        !ide.refused("highlight_file", json!({"path": "notes.unknownext"}))
            .is_empty()
    );
}
