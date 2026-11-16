//! F-032 · Diagnostics and inspections: errors and warnings while typing (SPEC-0023, SPEC-0070,
//! SPEC-0125).

use crate::harness::*;
use serde_json::json;

#[test]
fn a_type_error_is_reported_with_its_range() {
    let p = Project::reference("python-basic");
    p.write("bad.py", "x: int = \"s\"\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.call(
        "file_diagnostics",
        json!({"path": "bad.py", "timeout_seconds": 120}),
    );
    if lsp_unavailable(&r) {
        return;
    }
    let r = r.unwrap();
    let first = &r[0];
    assert_eq!(first["severity"], "error", "{r}");
    assert_eq!(first["start_line"], 0);
    assert_eq!(first["start_character"], 9);
}

#[test]
fn own_inspections_find_unused_imports_and_locals() {
    let p = Project::reference("java-basic");
    p.write(
        "src/main/java/lab/inventory/U.java",
        "package lab.inventory;\nimport java.util.Map;\nclass U { void f() { int unused = 1; } }\n",
    );
    let mut ide = Ide::stdio(&p);
    let findings = ide.ok("inspect_code", json!({}));
    let ids: Vec<String> = findings["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| text(&f["id"]))
        .collect();
    assert!(ids.contains(&"java.unused-import".into()), "{findings}");
    assert!(ids.contains(&"java.unused-local".into()), "{findings}");
    // The reference project itself is clean.
    let clean = ide.ok(
        "inspect_code",
        json!({"path": "src/main/java/lab/inventory/Inventory.java"}),
    );
    assert_eq!(clean, json!({ "findings": [] }));
}
