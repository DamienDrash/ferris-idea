//! F-016 · Search Everywhere: one field finds files, symbols and actions (SPEC-0031 AK-4, AK-5,
//! SPEC-0067).

use crate::harness::*;
use serde_json::json;

#[test]
fn files_symbols_and_actions_by_name() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("search_everywhere", json!({"query": "model"}));
    assert_eq!(r["files"][0]["path"], "src/inventory/model.py", "{r}");
    let r = ide.ok("search_everywhere", json!({"query": "restock"}));
    let symbols = r["symbols"].as_array().unwrap();
    assert_eq!(symbols[0]["name"], "restock", "{r}");
    assert_eq!(symbols[0]["path"], "src/inventory/model.py");
    let r = ide.ok(
        "search_everywhere",
        json!({"query": "EditorDuplicate", "categories": ["actions"]}),
    );
    assert!(
        r.get("files").is_none() && r.get("symbols").is_none(),
        "{r}"
    );
    let actions = r["actions"].as_array().unwrap();
    assert!(actions.iter().any(|a| a["id"] == "EditorDuplicate"), "{r}");
}

#[test]
fn camel_humps_find_a_class() {
    let p = Project::reference("java-basic");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "search_everywhere",
        json!({"query": "totVal", "categories": ["symbols"]}),
    );
    assert_eq!(r["symbols"][0]["name"], "totalValue", "{r}");
}
