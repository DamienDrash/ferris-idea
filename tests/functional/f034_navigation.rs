//! F-034 · Go to definition and usages: navigation by symbols (SPEC-0024 AK-3/AK-4, SPEC-0071,
//! SPEC-0073, SPEC-0077, SPEC-0079, SPEC-0082).

use crate::harness::*;
use serde_json::json;

#[test]
fn definition_references_hover_symbols_and_callers() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    // `Item` in test_model.py line 5 (0-based 4) → class Item in model.py.
    let def = ide.call(
        "find_symbol_locations",
        json!({"path": "test_model.py", "line": 4, "character": 12, "kind": "definition"}),
    );
    if lsp_unavailable(&def) {
        return;
    }
    let def = def.unwrap();
    assert_eq!(def[0]["path"], "src/inventory/model.py", "{def}");
    assert_eq!(def[0]["range"]["start_line"], 4);

    let refs = ide.ok(
        "find_symbol_locations",
        json!({"path": "src/inventory/model.py", "line": 14, "character": 5, "kind": "references"}),
    );
    // Declaration, import and call of `restock`.
    assert_eq!(refs.as_array().unwrap().len(), 3, "{refs}");

    let hover = ide.ok(
        "hover_symbol",
        json!({"path": "test_model.py", "line": 4, "character": 12}),
    );
    assert!(text(&hover["text"]).contains("Item"), "{hover}");

    let symbols = ide.ok(
        "document_symbols",
        json!({"path": "src/inventory/model.py"}),
    );
    let names: Vec<String> = symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|s| text(&s["name"]))
        .collect();
    assert!(
        names.starts_with(&["Item".into()]) && names.contains(&"restock".into()),
        "{symbols}"
    );

    let callers = ide.ok(
        "call_hierarchy",
        json!({"path": "src/inventory/model.py", "line": 14, "character": 5, "direction": "callers"}),
    );
    assert!(text(&callers).contains("test_restock"), "{callers}");
}
