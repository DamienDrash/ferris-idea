//! F-051 · Manage connections: profiles, drivers, SSL; passwords only in the keychain (SPEC-0008,
//! SPEC-0006 AK-6).

use crate::harness::*;
use serde_json::json;

#[test]
fn a_profile_is_saved_without_password_and_listed_with_its_url() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    assert_eq!(ide.ok("list_connections", json!({})), json!([]));
    let preview = ide.ok(
        "save_connection",
        json!({"name": "inv", "dialect": "postgresql", "host": "127.0.0.1", "port": 54329,
               "database": "inventory", "user": "ferris-idea", "ssl": "disable"}),
    );
    assert_eq!(
        preview["url"],
        "postgresql://ferris-idea@127.0.0.1:54329/inventory?sslmode=disable"
    );
    assert!(
        !p.path(".ferris-idea/connections.toml").exists(),
        "dry run writes nothing"
    );
    ide.ok(
        "save_connection",
        json!({"name": "inv", "dialect": "postgresql", "host": "127.0.0.1", "port": 54329,
               "database": "inventory", "user": "ferris-idea", "ssl": "disable", "dry_run": false}),
    );
    let file = p.read(".ferris-idea/connections.toml");
    assert!(!file.to_lowercase().contains("password"), "{file}");
    let list = ide.ok("list_connections", json!({}));
    assert_eq!(list[0]["name"], "inv");
    assert_eq!(list[0]["dialect"], "postgresql");
    assert_eq!(list[0]["has_secret"], false);
}

#[test]
fn invalid_profiles_are_refused_naming_the_field() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let reason = ide.refused(
        "save_connection",
        json!({"name": "x", "dialect": "postgresql", "host": "h", "port": 0}),
    );
    assert!(reason.contains("port"), "{reason}");
    let reason = ide.refused("save_connection", json!({"name": "x", "dialect": "sqlite"}));
    assert!(reason.contains("path"), "{reason}");
}

#[test]
fn testing_a_sqlite_connection_and_removing_it() {
    let p = Project::empty();
    p.sqlite("lite");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("test_connection", json!({"id": "lite"}));
    assert_eq!(r["ok"], true, "{r}");
    ide.ok("remove_connection", json!({"id": "lite"}));
    assert_eq!(ide.ok("list_connections", json!({})), json!([]));
}

#[test]
fn testing_the_reference_postgresql() {
    let p = Project::empty();
    if !live_connection(&p, "pg", "FERRIS_IDEA_TEST_PG") {
        return;
    }
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("test_connection", json!({"id": "pg"}));
    assert_eq!(r["ok"], true, "{r}");
}

#[test]
fn a_read_only_connection_refuses_changes() {
    let p = Project::empty();
    let file = p.root.join("ro.sqlite");
    std::fs::write(&file, b"").unwrap();
    p.connection(
        "ro",
        &format!(
            "dialect = \"sqlite\"\npath = \"{}\"\nread_only = true",
            file.display()
        ),
        &["read-schema", "read-data", "modify-data", "modify-schema"],
    );
    let mut ide = Ide::stdio(&p);
    // Refused before running, or failed by the database opened read-only: nothing changes.
    if let Ok(r) = ide.call(
        "run_sql",
        json!({"apply": true, "connection": "ro", "sql": "create table t (a int)"}),
    ) {
        assert!(text(&r).to_lowercase().contains("readonly"), "{r}");
    }
    assert_eq!(
        ide.ok("list_tables", json!({"connection": "ro"})),
        json!([])
    );
}
