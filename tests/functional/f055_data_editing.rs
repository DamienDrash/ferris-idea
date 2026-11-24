//! F-055 · Edit data: change cells, add and delete rows, all in one transaction (SPEC-0017 AK-1…AK-7,
//! IMP-0014).

use crate::harness::*;
use serde_json::json;

#[test]
fn preview_apply_and_conflict_on_a_second_apply() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "lite", "sql": "insert into item values ('D-4', 'Pin', 1)"}),
    );
    // Deleting B-2 breaks a foreign key of `stock`: the whole batch is rolled back.
    let broken = json!([
        {"kind": "update", "row": {"sku": "A-1", "name": "Bolt; zinc"}, "set": {"name": "Screw"}},
        {"kind": "delete", "row": {"sku": "B-2"}}
    ]);
    let r = ide.ok(
        "edit_table_data",
        json!({"connection": "lite", "table": "item", "changes": broken, "apply": true}),
    );
    assert_eq!(r["applied"], false, "{r}");
    assert_eq!(r["rolled_back"], true, "{r}");
    let untouched = ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "lite", "sql": "select name from item where sku = 'A-1'"}),
    );
    assert_eq!(untouched["results"][0]["rows"], json!([["Bolt; zinc"]]));

    let changes = json!([
        {"kind": "update", "row": {"sku": "A-1", "name": "Bolt; zinc"}, "set": {"name": "Screw"}},
        {"kind": "insert", "values": {"sku": "C-3", "name": "Washer", "unit_price_cents": 5}},
        {"kind": "delete", "row": {"sku": "D-4"}}
    ]);
    let preview = ide.ok(
        "edit_table_data",
        json!({"connection": "lite", "table": "item", "changes": changes}),
    );
    assert_eq!(preview["applied"], false);
    assert!(
        text(&preview["sql"]).contains("\\\"name\\\" IS 'Bolt; zinc'"),
        "SQLite compares with IS: {preview}"
    );
    ide.ok(
        "edit_table_data",
        json!({"connection": "lite", "table": "item", "changes": changes, "apply": true}),
    );
    let rows = ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "lite", "sql": "select sku, name from item order by sku"}),
    );
    assert_eq!(
        rows["results"][0]["rows"],
        json!([["A-1", "Screw"], ["B-2", "Nut 'M6'"], ["C-3", "Washer"]])
    );
    // Same changes again: the loaded values no longer match, nothing changes.
    let again = ide.ok(
        "edit_table_data",
        json!({"connection": "lite", "table": "item", "changes": changes, "apply": true}),
    );
    assert_eq!(again["applied"], false, "{again}");
    assert_eq!(
        again["failed_change"], 1,
        "the first change conflicts: {again}"
    );
    assert!(text(&again["reason"]).contains("changed"), "{again}");
    let rows2 = ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "lite", "sql": "select sku, name from item order by sku"}),
    );
    assert_eq!(rows2["results"][0]["rows"], rows["results"][0]["rows"]);
}

#[test]
fn applying_needs_modify_data_and_keyless_tables_are_refused() {
    let p = Project::empty();
    let file = p.root.join("r.sqlite");
    std::fs::write(&file, b"").unwrap();
    p.connection(
        "r",
        &format!("dialect = \"sqlite\"\npath = \"{}\"", file.display()),
        &["read-schema", "read-data"],
    );
    p.sqlite("w");
    let mut ide = Ide::stdio(&p);
    ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "w", "sql": "create table nokey (a text)"}),
    );
    let reason = ide.refused("edit_table_data", json!({"connection": "w", "table": "nokey", "changes": [{"kind": "delete", "row": {"a": "x"}}], "apply": true}));
    assert!(reason.to_lowercase().contains("key"), "{reason}");
    // The read-only rights of `r` refuse applying.
    let reason = ide.refused(
        "run_sql",
        json!({"apply": true, "connection": "r", "sql": "create table t (a int)"}),
    );
    assert!(reason.contains("modify"), "{reason}");
}

#[test]
fn postgresql_preview_uses_is_not_distinct_from() {
    let p = Project::empty();
    if !live_connection(&p, "pg", "FERRIS_IDEA_TEST_PG") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    let preview = ide.ok(
        "edit_table_data",
        json!({"connection": "pg", "table": "item",
               "changes": [{"kind": "update", "row": {"sku": "A-1", "name": "Bolt; zinc"}, "set": {"name": "Screw"}}]}),
    );
    assert!(text(&preview).contains("IS NOT DISTINCT FROM"), "{preview}");
    assert_eq!(preview["applied"], false);
}
