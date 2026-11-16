//! F-058 · Schema compare and DDL generation: the differences of two schemas as DDL (SPEC-0018 AK-1,
//! AK-3, AK-4, AK-5).

use crate::harness::*;
use serde_json::json;

#[test]
fn differences_script_and_round_trip_on_sqlite() {
    let p = Project::empty();
    p.sqlite("src").sqlite("dst");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "src");
    ide.ok("run_sql", json!({"apply": true, "connection": "dst", "sql": "create table item (sku text primary key, name text, extra int)"}));
    let diff = ide.ok("compare_schemas", json!({"source": "src", "target": "dst"}));
    let d = &diff["differences"];
    assert!(
        text(&d["added"]).contains("stock") && text(&d["added"]).contains("warehouse"),
        "{d}"
    );
    // Dropping `extra` loses data: listed, but not selected without include_destructive.
    let drop = diff["statements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["data_loss"] == true)
        .unwrap_or_else(|| panic!("{diff}"));
    assert_eq!(drop["selected"], false, "{drop}");
    let full = ide.ok(
        "compare_schemas",
        json!({"source": "src", "target": "dst", "include_destructive": true}),
    );
    ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "dst", "sql": full["script"]}),
    );
    let again = ide.ok("compare_schemas", json!({"source": "src", "target": "dst"}));
    let d = &again["differences"];
    assert!(
        d["added"] == json!([]) && d["removed"] == json!([]) && d["changed"] == json!([]),
        "after the migration: {again}"
    );
}

#[test]
fn create_script_rebuilds_the_schema() {
    let p = Project::empty();
    p.sqlite("src").sqlite("empty");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "src");
    let ddl = ide.ok("generate_ddl", json!({"connection": "src"}));
    let script: Vec<String> = ddl["statements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| text(&s["sql"]))
        .collect();
    ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "empty", "sql": script.join(";\n")}),
    );
    let diff = ide.ok(
        "compare_schemas",
        json!({"source": "src", "target": "empty"}),
    );
    assert_eq!(diff["differences"]["changed"], json!([]), "{diff}");
    assert_eq!(diff["differences"]["added"], json!([]), "{diff}");
}

#[test]
fn different_dialects_are_refused() {
    let p = Project::empty();
    p.sqlite("lite");
    if !live_connection(&p, "pg", "IDECRAFT_TEST_PG") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    ide.refused("compare_schemas", json!({"source": "lite", "target": "pg"}));
}
