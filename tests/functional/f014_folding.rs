//! F-014 · Code folding: blocks fold and unfold (SPEC-0030 AK-1, AK-2, AK-5, SPEC-0083).

use crate::harness::*;
use serde_json::json;

#[test]
fn python_class_and_function_bodies_fold() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    let ranges = ide.ok("folding_ranges", json!({"path": "src/inventory/model.py"}));
    let pairs: Vec<(u64, u64)> = ranges
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["start_line"].as_u64().unwrap(),
                r["end_line"].as_u64().unwrap(),
            )
        })
        .collect();
    // class Item (0-based line 4) to its last method line, def restock (14) to the raise (19).
    assert!(pairs.contains(&(4, 11)), "{ranges}");
    assert!(pairs.contains(&(14, 19)), "{ranges}");
}

#[test]
fn rust_blocks_comments_and_imports_fold_but_one_line_blocks_do_not() {
    let p = Project::empty();
    p.write(
        "lib.rs",
        "use a::b;\nuse c::d;\n\n/* one\n two */\nfn f() {\n    1;\n}\nfn g() { 2; }\n",
    );
    let mut ide = Ide::stdio(&p);
    let ranges = ide.ok("folding_ranges", json!({"path": "lib.rs"}));
    let all = ranges.as_array().unwrap();
    let has = |s: u64, kind: &str| {
        all.iter()
            .any(|r| r["start_line"] == s && r["kind"] == kind)
    };
    assert!(has(0, "imports"), "{ranges}");
    assert!(has(3, "comment"), "{ranges}");
    assert!(has(5, "block"), "{ranges}");
    assert!(!all.iter().any(|r| r["start_line"] == 8), "{ranges}");
}
