//! F-053 · SQL editor with schema completion: table and column names from the live schema
//! (SPEC-0014 AK-1…AK-8, SPEC-0152).

use crate::harness::*;
use serde_json::json;

fn labels(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|c| text(&c["label"]))
        .collect()
}

fn complete(ide: &mut Ide, conn: &str, sql: &str) -> Vec<String> {
    let cursor = sql.find('|').unwrap();
    let sql = sql.replace('|', "");
    labels(&ide.ok(
        "complete_sql",
        json!({"connection": conn, "sql": sql, "cursor": cursor}),
    ))
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[test]
fn tables_after_from_and_columns_of_aliases() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let from = complete(&mut ide, "lite", "select * from |");
    assert_eq!(from, ["item", "stock", "stock_value", "warehouse"]);
    let cols = complete(&mut ide, "lite", "select s.| from stock s");
    assert_eq!(sorted(cols), ["qty", "sku", "warehouse_id"]);
    let joined = complete(&mut ide, "lite", "select * from stock s join item i on i.|");
    assert!(
        joined.contains(&"unit_price_cents".into()) && !joined.contains(&"qty".into()),
        "{joined:?}"
    );
    let second = complete(&mut ide, "lite", "select * from item; select | from stock");
    assert!(
        second.contains(&"qty".into()) && !second.contains(&"name".into()),
        "{second:?}"
    );
    assert!(complete(&mut ide, "lite", "select 'fro|").is_empty());
}

#[test]
fn completion_against_postgresql() {
    let p = Project::empty();
    if !live_connection(&p, "pg", "IDECRAFT_TEST_PG") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    let cols = complete(&mut ide, "pg", "select s.| from stock s");
    assert_eq!(sorted(cols), ["qty", "sku", "warehouse_id"]);
}

#[test]
fn sql_inspection_finds_unknown_names_and_unsafe_deletes() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let r = ide.ok(
        "inspect_sql",
        json!({"connection": "lite", "sql": "delete from item; select nope from item"}),
    );
    let all = text(&r);
    assert!(all.contains("WHERE") && all.contains("nope"), "{all}");
}
