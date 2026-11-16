//! F-050 · Split an SQL script into statements: run the current statement or the whole script
//! (SPEC-0005 AK-1…AK-7).

use crate::harness::*;
use serde_json::json;

fn texts(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| text(&s["text"]))
        .collect()
}

#[test]
fn literals_comments_and_dialect_rules_do_not_split() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "split_sql_statements",
        json!({"sql": "insert into t values ('a;b'); -- c;d\nselect \"x;y\" from t; /* ; */ ;", "dialect": "postgresql"}),
    );
    let t = texts(&r);
    assert_eq!(t.len(), 2, "{r}");
    assert_eq!(t[0], "insert into t values ('a;b')");
    let r = ide.ok(
        "split_sql_statements",
        json!({"sql": "select $$a;b$$; select 2", "dialect": "postgresql"}),
    );
    assert_eq!(texts(&r), ["select $$a;b$$", "select 2"]);
    let r = ide.ok(
        "split_sql_statements",
        json!({"sql": "select `a;b`; # x;y\nselect 'it\\'s;'", "dialect": "mysql"}),
    );
    assert_eq!(texts(&r).len(), 2, "{r}");
    // Dollar quotes mean nothing outside PostgreSQL.
    let r = ide.ok(
        "split_sql_statements",
        json!({"sql": "select $$a;b$$", "dialect": "sqlite"}),
    );
    assert_eq!(texts(&r).len(), 2, "{r}");
}

#[test]
fn ranges_index_into_the_script() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let sql = "select 1;\n  select 'ü';";
    let r = ide.ok(
        "split_sql_statements",
        json!({"sql": sql, "dialect": "sqlite"}),
    );
    for s in r.as_array().unwrap() {
        let (a, b) = (
            s["start"].as_u64().unwrap() as usize,
            s["end"].as_u64().unwrap() as usize,
        );
        assert_eq!(&sql[a..b], text(&s["text"]));
    }
}

#[test]
fn the_whole_reference_script_runs_statement_by_statement() {
    let p = Project::reference("sql-basic");
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    load_reference_schema(&mut ide, "lite");
    let r = ide.ok("run_sql", json!({"apply": true, "connection": "lite", "sql": "select count(*) from item; select count(*) from stock"}));
    assert_eq!(r["results"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(r["results"][0]["rows"], json!([["2"]]));
    // A failing statement stops the script and says how many were not run.
    let r = ide.call(
        "run_sql",
        json!({"apply": true, "connection": "lite", "sql": "select nope from item; select 1"}),
    );
    match r {
        Ok(v) => assert_eq!(v["not_run"], 1, "{v}"),
        Err(e) => assert!(e.contains("nope"), "{e}"),
    }
}
