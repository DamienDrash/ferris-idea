//! F-057 · Database refactorings: rename a table or column including dependent SQL (SPEC-0019 AK-1,
//! AK-4, AK-6, AK-7, SPEC-0164).

use crate::harness::*;
use serde_json::json;

#[test]
fn renaming_a_table_rewrites_sql_files_and_returns_the_ddl() {
    let p = Project::reference("sql-basic");
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let r = ide.ok(
        "rename_sql_object",
        json!({"connection": "lite", "table": "item", "new_name": "article"}),
    );
    assert_eq!(r["ddl"], "ALTER TABLE item RENAME TO article");
    let files: Vec<String> = r["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| text(&f["path"]))
        .collect();
    assert!(
        files.contains(&"schema.sql".into()) && files.contains(&"seed.sql".into()),
        "{files:?}"
    );
    let mysql = r["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "mysql/schema.sql")
        .unwrap();
    assert_eq!(mysql["other_dialect"], true);
    // Given SQL is rewritten: names, not literals or comments.
    let r = ide.ok(
        "rename_sql_object",
        json!({"connection": "lite", "table": "item", "new_name": "article",
               "sql": "select i.sku, 'item' from item i -- item\njoin stock s on s.sku = i.sku"}),
    );
    let all = text(&r);
    assert!(all.contains("from article i"), "{all}");
    assert!(all.contains("'item'") && all.contains("-- item"), "{all}");
}

#[test]
fn ambiguous_columns_are_reported_and_unknown_objects_refused() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let r = ide.ok(
        "rename_sql_object",
        json!({"connection": "lite", "table": "item", "column": "sku", "new_name": "code",
               "sql": "select sku from item join stock on stock.sku = item.sku"}),
    );
    assert!(text(&r).contains("ambiguous"), "{r}");
    ide.refused(
        "rename_sql_object",
        json!({"connection": "lite", "table": "nope", "new_name": "x"}),
    );
    ide.refused(
        "rename_sql_object",
        json!({"connection": "lite", "table": "item", "new_name": "stock"}),
    );
}

#[test]
fn applying_on_sqlite_keeps_the_data() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    ide.ok("rename_sql_object", json!({"connection": "lite", "table": "item", "column": "name", "new_name": "title", "apply": true}));
    let r = ide.ok(
        "run_sql",
        json!({"apply": true, "connection": "lite", "sql": "select title from item order by sku"}),
    );
    assert_eq!(
        r["results"][0]["rows"],
        json!([["Bolt; zinc"], ["Nut 'M6'"]])
    );
}
