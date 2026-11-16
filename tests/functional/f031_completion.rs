//! F-031 · Code completion: context-dependent proposals (SPEC-0024 AK-3/AK-4, SPEC-0072, SPEC-0080).

use crate::harness::*;
use serde_json::json;

#[test]
fn members_of_a_python_class_are_proposed() {
    let p = Project::reference("python-basic");
    p.write(
        "use.py",
        "from src.inventory.model import Item\nItem(\"a\", \"b\", 1, 2).to\n",
    );
    let mut ide = Ide::stdio(&p);
    let r = ide.call(
        "complete_code",
        json!({"path": "use.py", "line": 1, "character": 24}),
    );
    if lsp_unavailable(&r) {
        return;
    }
    let labels = text(&r.unwrap());
    assert!(labels.contains("total_cents"), "{labels}");
}

#[test]
fn parameter_info_of_a_call() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    // Inside `restock(items, "A-1", 2)` of test_model.py.
    let r = ide.call(
        "signature_help",
        json!({"path": "test_model.py", "line": 9, "character": 19}),
    );
    if lsp_unavailable(&r) {
        return;
    }
    let r = r.unwrap();
    let label = text(&r["signatures"][0]["label"]);
    assert!(label.contains("items") && label.contains("amount"), "{r}");
}

#[test]
fn paths_outside_the_workspace_are_refused() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    ide.refused(
        "complete_code",
        json!({"path": "../../etc/hosts", "line": 0, "character": 0}),
    );
}
