//! F-060 · More dialects (Oracle, SQL Server, MariaDB): as F-051…F-059 (SPEC-0116, SPEC-0135,
//! SPEC-0142).
//!
//! Live Oracle and SQL Server need a password from the system keychain, which the MCP server reads
//! only from there; the drivers are tested live in `crates/database/tests` with password files.
//! Headless here: profiles, URLs and script rules of these dialects, and MariaDB live.

use crate::harness::*;
use serde_json::json;

#[test]
fn oracle_and_sql_server_profiles() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let o = ide.ok("save_connection", json!({"name": "o", "dialect": "oracle", "host": "h", "port": 1521, "database": "FREEPDB1", "user": "u"}));
    // New connections check certificates (SPEC-0016 v2 behaviour 8).
    assert_eq!(o["url"], "oracle://u@h:1521/FREEPDB1?sslmode=verify-full");
    let m = ide.ok("save_connection", json!({"name": "m", "dialect": "sqlserver", "host": "h", "database": "inventory", "user": "u"}));
    assert_eq!(m["url"], "sqlserver://u@h/inventory?sslmode=verify-full");
}

#[test]
fn script_separators_of_sql_server_and_oracle() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "split_sql_statements",
        json!({"sql": "select 1\nGO\nselect 2", "dialect": "sqlserver"}),
    );
    assert_eq!(r.as_array().unwrap().len(), 2, "{r}");
    let r = ide.ok(
        "split_sql_statements",
        json!({"sql": "begin null; end;\n/\nselect 1 from dual", "dialect": "oracle"}),
    );
    assert_eq!(
        r[0]["text"], "begin null; end;",
        "a PL/SQL block stays whole: {r}"
    );
}

#[test]
fn mariadb_reference_schema() {
    let p = Project::empty();
    if !live_connection(&p, "maria", "IDECRAFT_TEST_MARIADB") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    let tables = ide.ok("list_tables", json!({"connection": "maria"}));
    assert!(text(&tables).contains("stock_value"), "{tables}");
}
