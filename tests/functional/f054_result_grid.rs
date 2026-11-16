//! F-054 · Result grid: sort, filter, paginate, export (SPEC-0007, SPEC-0021).

use crate::harness::*;
use serde_json::json;

#[test]
fn sort_filter_and_page_a_result() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let r = ide.ok(
        "read_table_data",
        json!({"connection": "lite", "table": "item", "order_by": "sku desc"}),
    );
    assert_eq!(r["rows"][0][0], "B-2");
    assert_eq!(r["total_rows"], 2);
    let r = ide.ok(
        "read_table_data",
        json!({"connection": "lite", "table": "item", "where": "unit_price_cents > 20"}),
    );
    assert_eq!(r["rows"], json!([["A-1", "Bolt; zinc", "25"]]));
    ide.refused(
        "read_table_data",
        json!({"connection": "lite", "table": "item", "where": "1=1; drop table item"}),
    );
    let r = ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "lite", "sql": "select sku from item", "max_rows": 1}),
    );
    assert_eq!(r["results"][0]["rows"].as_array().unwrap().len(), 1);
    assert_eq!(r["results"][0]["truncated"], true, "{r}");
}

#[test]
fn export_keeps_null_and_empty_string_apart() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    ide.ok("run_sql", json!({"apply": true, "connection": "lite", "sql": "create table t (a text, b text); insert into t values (null, ''), ('x;y', 'q\"r')"}));
    let csv = ide.ok(
        "export_data",
        json!({"connection": "lite", "sql": "select a, b from t", "format": "csv"}),
    );
    let csv = text(&csv);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "a,b");
    assert_eq!(lines[1], ",\"\"", "NULL is empty, '' is quoted");
    let json_out = ide.ok(
        "export_data",
        json!({"connection": "lite", "sql": "select a, b from t", "format": "json"}),
    );
    assert!(text(&json_out).contains("null"), "{json_out}");
    let inserts = ide.ok("export_data", json!({"connection": "lite", "sql": "select a, b from t", "format": "sql-insert", "table": "t"}));
    assert!(
        text(&inserts).to_uppercase().contains("INSERT INTO"),
        "{inserts}"
    );
}
