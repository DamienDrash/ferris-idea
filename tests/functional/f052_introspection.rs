//! F-052 · Schema introspection: schemas, tables, columns, indexes, keys (SPEC-0013 AK-1…AK-5).

use crate::harness::*;
use serde_json::json;

fn check_stock(ide: &mut Ide, conn: &str) {
    let tables = ide.ok("list_tables", json!({"connection": conn}));
    let names: Vec<String> = tables
        .as_array()
        .unwrap()
        .iter()
        .map(|t| text(&t["name"]))
        .collect();
    for n in ["item", "stock", "stock_value", "warehouse"] {
        assert!(names.contains(&n.to_owned()), "{conn}: {tables}");
    }
    let view = tables
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "stock_value")
        .unwrap();
    assert_eq!(view["kind"], "view");
    let stock = ide.ok(
        "describe_table",
        json!({"connection": conn, "table": "stock"}),
    );
    let cols: Vec<String> = stock["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| text(&c["name"]))
        .collect();
    assert_eq!(cols, ["warehouse_id", "sku", "qty"], "{conn}");
    assert!(
        stock["columns"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["nullable"] == false)
    );
    assert_eq!(stock["columns"][2]["default"], "0");
    assert_eq!(stock["primary_key"], json!(["warehouse_id", "sku"]));
    assert!(
        stock["indexes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["unique"] == true && i["columns"] == json!(["warehouse_id", "sku"])),
        "{stock}"
    );
}

#[test]
fn sqlite_reference_schema() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    check_stock(&mut ide, "lite");
    let stock = ide.ok(
        "describe_table",
        json!({"connection": "lite", "table": "stock"}),
    );
    let fks = text(&stock["foreign_keys"]);
    assert!(fks.contains("warehouse") && fks.contains("item"), "{fks}");
}

#[test]
fn postgresql_reference_schema() {
    let p = Project::empty();
    if !live_connection(&p, "pg", "IDECRAFT_TEST_PG") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    check_stock(&mut ide, "pg");
    let schemas = ide.ok("list_schemas", json!({"connection": "pg"}));
    assert!(text(&schemas).contains("public"), "{schemas}");
}

#[test]
fn mysql_reference_schema() {
    let p = Project::empty();
    if !live_connection(&p, "my", "IDECRAFT_TEST_MYSQL") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    check_stock(&mut ide, "my");
}

#[test]
fn without_read_schema_both_tools_are_refused() {
    let p = Project::empty();
    let file = p.root.join("n.sqlite");
    std::fs::write(&file, b"").unwrap();
    p.connection(
        "n",
        &format!("dialect = \"sqlite\"\npath = \"{}\"", file.display()),
        &["read-data"],
    );
    let mut ide = Ide::stdio(&p);
    assert!(
        ide.refused("list_tables", json!({"connection": "n"}))
            .contains("read-schema")
    );
    assert!(
        ide.refused("describe_table", json!({"connection": "n", "table": "x"}))
            .contains("read-schema")
    );
}
