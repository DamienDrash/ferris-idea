//! F-059 · Show the query plan: EXPLAIN as a tree with hints (SPEC-0020 AK-2, AK-4, AK-5, AK-6).

use crate::harness::*;
use serde_json::json;

const JOIN: &str = "select * from stock s join item i on i.sku = s.sku where s.qty > 5";

#[test]
fn sqlite_plan_tree_with_a_full_scan_hint() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let plan = ide.ok("explain_sql", json!({"connection": "lite", "sql": JOIN}));
    assert!(!plan["nodes"].as_array().unwrap().is_empty(), "{plan}");
    assert!(text(&plan["hints"]).contains("full scan"), "{plan}");
    // SQLite cannot measure; two statements and DDL are refused.
    ide.refused(
        "explain_sql",
        json!({"connection": "lite", "sql": JOIN, "analyze": true}),
    );
    ide.refused(
        "explain_sql",
        json!({"connection": "lite", "sql": "select 1; select 2"}),
    );
}

#[test]
fn postgresql_measured_plan_rolls_back_changes() {
    let p = Project::empty();
    if !live_connection(&p, "pg", "FERRIS_IDEA_TEST_PG") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    let plan = ide.ok("explain_sql", json!({"connection": "pg", "sql": JOIN}));
    let node = &plan["nodes"][0];
    assert!(
        !node["estimated_rows"].is_null() && !node["cost"].is_null(),
        "{plan}"
    );
    // A measured UPDATE needs modify-data; the connection has reading rights only.
    ide.refused(
        "explain_sql",
        json!({"connection": "pg", "sql": "update stock set qty = qty", "analyze": true}),
    );
}

#[test]
fn mysql_plan_is_read() {
    let p = Project::empty();
    if !live_connection(&p, "my", "FERRIS_IDEA_TEST_MYSQL") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    let plan = ide.ok("explain_sql", json!({"connection": "my", "sql": JOIN}));
    assert!(!plan["nodes"].as_array().unwrap().is_empty(), "{plan}");
}
