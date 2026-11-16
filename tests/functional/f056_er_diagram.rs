//! F-056 · ER diagrams: tables and foreign keys as a diagram (SPEC-0022 AK-2, AK-5, AK-6, AK-9).

use crate::harness::*;
use serde_json::json;

#[test]
fn reference_schema_as_mermaid_dot_and_selection() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let mermaid = text(&ide.ok("er_diagram", json!({"connection": "lite"})));
    assert!(mermaid.starts_with("erDiagram"), "{mermaid}");
    assert!(mermaid.contains("item ||--o{ stock"), "{mermaid}");
    assert!(mermaid.contains("warehouse ||--o{ stock"), "{mermaid}");
    assert!(mermaid.contains("stock_value"), "{mermaid}");
    let dot = text(&ide.ok("er_diagram", json!({"connection": "lite", "format": "dot"})));
    assert!(dot.contains("digraph") || dot.contains("graph"), "{dot}");
    // Only `stock` with depth 0: the referenced tables are listed as outside the selection.
    let model = ide.ok(
        "er_diagram",
        json!({"connection": "lite", "tables": ["stock"], "format": "json", "without_views": true}),
    );
    let all = text(&model);
    assert!(all.contains("stock"), "{all}");
    let entities = text(&model["entities"]);
    assert!(!entities.contains("\"warehouse\""), "{model}");
}

#[test]
fn png_is_written_only_inside_the_workspace() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    ide.ok(
        "er_diagram",
        json!({"connection": "lite", "format": "png", "path": "er.png"}),
    );
    let bytes = std::fs::read(p.path("er.png")).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    ide.refused(
        "er_diagram",
        json!({"connection": "lite", "format": "png", "path": "../er.png"}),
    );
    let svg = text(&ide.ok("er_diagram", json!({"connection": "lite", "format": "svg"})));
    assert!(svg.contains("<svg"), "{svg}");
}
