//! F-072 · Rename: all usages follow (SPEC-0040 AK-2/AK-3, SPEC-0074, SPEC-0148, IMP-0017).

use crate::harness::*;
use serde_json::json;

#[test]
fn preview_then_apply_across_files() {
    let p = Project::reference("python-basic");
    let before = p.read("test_model.py");
    let mut ide = Ide::stdio(&p);
    let plan = ide.call(
        "rename_symbol",
        json!({"path": "src/inventory/model.py", "line": 14, "character": 5, "new_name": "refill"}),
    );
    if lsp_unavailable(&plan) {
        return;
    }
    let plan = plan.unwrap();
    assert_eq!(plan["applied"], false);
    let files: Vec<String> = plan["plan"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| text(&f["path"]))
        .collect();
    assert!(files.contains(&"test_model.py".into()), "{plan}");
    assert_eq!(p.read("test_model.py"), before, "preview writes nothing");
    ide.ok(
        "rename_symbol",
        json!({"path": "src/inventory/model.py", "line": 14, "character": 5, "new_name": "refill", "dry_run": false}),
    );
    assert!(p.read("src/inventory/model.py").contains("def refill("));
    assert!(p.read("test_model.py").contains("import Item, refill"));
}

#[test]
fn comments_and_strings_on_request() {
    let p = Project::reference("python-basic");
    p.write("notes.txt", "call restock when low\n");
    let mut ide = Ide::stdio(&p);
    let plan = ide.call(
        "rename_symbol",
        json!({"path": "src/inventory/model.py", "line": 14, "character": 5, "new_name": "refill",
               "search_text_occurrences": true}),
    );
    if lsp_unavailable(&plan) {
        return;
    }
    assert!(text(&plan.unwrap()).contains("notes.txt"));
}
