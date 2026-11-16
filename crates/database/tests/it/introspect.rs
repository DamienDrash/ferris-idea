//! SPEC-0013 against the reference databases (PostgreSQL/MySQL via `IDECRAFT_TEST_PG` /
//! `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB`; SQLite built from the seed files).

use crate::common;

use std::path::Path;

use idecraft_database::{ConnectionProfile, Dialect, Session, Table, introspect};

async fn tables_of(url: &str) -> Vec<Table> {
    let profile = ConnectionProfile::from_url("reference", url).unwrap();
    let session = Session::connect(&profile, &common::credentials(&profile))
        .await
        .unwrap();
    introspect(&session).await.unwrap()
}

fn sqlite_reference() -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("idecraft-introspect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sql =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/sql-basic");
    let script: String = ["schema.sql", "seed.sql", "seed-movements.sqlite.sql"]
        .iter()
        .map(|f| std::fs::read_to_string(sql.join(f)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let file = dir.join("inventory.sqlite");
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch(&script)
        .unwrap();
    let url = format!("sqlite:{}", file.display());
    (dir, url)
}

const REFERENCE_OBJECTS: [(&str, &str); 5] = [
    ("item", "table"),
    ("movement", "table"),
    ("stock", "table"),
    ("stock_value", "view"),
    ("warehouse", "table"),
];

fn check_common(tables: &[Table], dialect: Dialect) {
    let mut names: Vec<(&str, &str)> = tables
        .iter()
        .map(|t| (t.name.as_str(), t.kind.as_str()))
        .collect();
    // The server databases are shared with the live tests of other modules, which create and drop
    // scratch tables (`rf_*`, the table editor's copies) while this runs; only SQLite, built in a
    // file of its own, is checked for nothing else.
    if dialect != Dialect::Sqlite {
        names.retain(|(name, _)| REFERENCE_OBJECTS.iter().any(|(r, _)| r == name));
    }
    assert_eq!(names, REFERENCE_OBJECTS, "{dialect:?}");
    let stock = tables.iter().find(|t| t.name == "stock").unwrap();
    let columns: Vec<(&str, bool)> = stock
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c.nullable))
        .collect();
    assert_eq!(
        columns,
        [("warehouse_id", false), ("sku", false), ("qty", false)],
        "{dialect:?}"
    );
    assert_eq!(
        stock.columns[2]
            .default
            .as_deref()
            .map(|d| d.trim_matches('\'')),
        Some("0"),
        "{dialect:?}"
    );
    assert_eq!(stock.primary_key, ["warehouse_id", "sku"], "{dialect:?}");
    assert!(
        stock
            .indexes
            .iter()
            .any(|i| i.unique && i.columns == ["warehouse_id", "sku"]),
        "{dialect:?}: {:?}",
        stock.indexes
    );
}

fn references(table: &Table) -> Vec<(Vec<String>, String, Vec<String>)> {
    table
        .foreign_keys
        .iter()
        .map(|f| {
            (
                f.columns.clone(),
                f.referenced_table.clone(),
                f.referenced_columns.clone(),
            )
        })
        .collect()
}

fn expected_stock_references() -> Vec<(Vec<String>, String, Vec<String>)> {
    vec![
        (vec!["sku".into()], "item".into(), vec!["sku".into()]),
        (
            vec!["warehouse_id".into()],
            "warehouse".into(),
            vec!["id".into()],
        ),
    ]
}

#[tokio::test]
async fn sqlite_reference_schema() {
    let (dir, url) = sqlite_reference();
    let tables = tables_of(&url).await;
    check_common(&tables, Dialect::Sqlite);
    let stock = tables.iter().find(|t| t.name == "stock").unwrap();
    let mut refs = references(stock);
    refs.sort();
    assert_eq!(refs, expected_stock_references());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn postgres_reference_schema() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let tables = tables_of(&url).await;
    check_common(&tables, Dialect::PostgreSql);
    let stock = tables.iter().find(|t| t.name == "stock").unwrap();
    let mut refs = references(stock);
    refs.sort();
    assert_eq!(refs, expected_stock_references());
}

#[tokio::test]
async fn mysql_reference_schema() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_MYSQL") else {
        return;
    };
    let tables = tables_of(&url).await;
    check_common(&tables, Dialect::MySql);
    let stock = tables.iter().find(|t| t.name == "stock").unwrap();
    // MySQL ignores column-level REFERENCES, so the reference schema declares the foreign keys as
    // table constraints (since #414; the database script of the reference environment adds them to
    // an older database). Introspection reports exactly those.
    let mut refs = references(stock);
    refs.sort();
    assert_eq!(refs, expected_stock_references());
}

#[tokio::test]
async fn mariadb_reference_schema() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_MARIADB") else {
        return;
    };
    let tables = tables_of(&url).await;
    check_common(&tables, Dialect::MariaDb);
    // Unlike MySQL, MariaDB turns column-level REFERENCES into foreign keys (named `1`, `2`, …).
    let stock = tables.iter().find(|t| t.name == "stock").unwrap();
    let mut refs = references(stock);
    refs.sort();
    assert_eq!(refs, expected_stock_references());
    // A column without default is reported as `NULL` by MariaDB and must read as none.
    let item = tables.iter().find(|t| t.name == "item").unwrap();
    assert!(item.columns.iter().all(|c| c.default.is_none()), "{item:?}");
}

/// SPEC-0116 v3 AK-7: a sequence is listed on MariaDB (where the reference server runs) and DuckDB
/// (bundled engine); the scratch sequence is dropped afterwards.
#[tokio::test]
async fn sequences_are_listed() {
    use idecraft_database::{OnError, Outcome, sequences};
    let name = format!("rf_seq{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("idecraft-seq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("seq.duckdb");
    duckdb::Connection::open(&file).unwrap();
    let mut urls = vec![format!("duckdb:{}", file.display())];
    urls.extend(std::env::var("IDECRAFT_TEST_MARIADB").ok());
    for url in urls {
        let profile = ConnectionProfile::from_url("reference", &url).unwrap();
        let session = Session::connect(&profile, &common::credentials(&profile))
            .await
            .unwrap();
        let run = session
            .run_script(
                &format!("drop sequence if exists {name}; create sequence {name}"),
                OnError::Stop,
            )
            .await;
        assert!(
            run.outcomes
                .iter()
                .all(|o| !matches!(o.outcome, Outcome::Error(_))),
            "{url}: {:?}",
            run.outcomes
        );
        let listed = sequences(&session, None).await.unwrap();
        let _ = session
            .run_script(&format!("drop sequence {name}"), OnError::Continue)
            .await;
        assert!(listed.contains(&name), "{url}: {listed:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
