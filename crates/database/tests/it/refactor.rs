//! SPEC-0019/AK-5: renaming on the SQLite reference database keeps query results.

use std::path::Path;

use ferris_idea_database::{
    ConnectionProfile, Credentials, Dialect, OnError, Outcome, Session, introspect, rename,
};

const QUERIES: &str = "\
select i.sku, i.name, s.qty from item i join stock s on s.sku = i.sku order by 1;
select name, unit_price_cents from item where unit_price_cents > 5 order by name;
select count(*) from movement m join item on item.sku = m.sku;
select w.city, sum(s.qty * i.unit_price_cents) from stock s join item i on i.sku = s.sku
  join warehouse w on w.id = s.warehouse_id group by w.city order by 1;
select * from stock_value order by 1";

async fn results(s: &Session, sql: &str) -> Vec<Vec<Vec<Option<String>>>> {
    let run = s.run_script(sql, OnError::Stop).await;
    run.outcomes
        .into_iter()
        .map(|o| match o.outcome {
            Outcome::ResultSet { rows, .. } => rows,
            other => panic!("{other:?} in\n{sql}"),
        })
        .collect()
}

async fn ddl(s: &Session, sql: &str) {
    let run = s.run_script(sql, OnError::Stop).await;
    assert!(
        matches!(run.outcomes[0].outcome, Outcome::UpdateCount(_)),
        "{run:?}"
    );
}

#[tokio::test]
async fn sqlite_rename_keeps_query_results() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sql =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/sql-basic");
    let seed: String = ["schema.sql", "seed.sql", "seed-movements.sqlite.sql"]
        .iter()
        .map(|f| std::fs::read_to_string(sql.join(f)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let file = dir.join("ref.sqlite");
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch(&seed)
        .unwrap();
    let profile =
        ConnectionProfile::from_url("ref", &format!("sqlite:{}", file.display())).unwrap();
    let s = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();
    let before = results(&s, QUERIES).await;

    let column = rename(
        QUERIES,
        Dialect::Sqlite,
        &introspect(&s).await.unwrap(),
        "item",
        Some("unit_price_cents"),
        "price_cents",
    )
    .unwrap();
    assert!(column.ambiguous.is_empty(), "{:?}", column.ambiguous);
    assert_eq!(column.edits.len(), 3);
    ddl(&s, &column.ddl).await;

    let table = rename(
        &column.sql,
        Dialect::Sqlite,
        &introspect(&s).await.unwrap(),
        "item",
        None,
        "article",
    )
    .unwrap();
    assert_eq!(table.edits.len(), 5);
    ddl(&s, &table.ddl).await;

    assert!(!table.sql.contains("item") && !table.sql.contains("unit_price"));
    assert_eq!(results(&s, &table.sql).await, before);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0019/AK-7 (v2): renaming `item.unit_price_cents` for a PostgreSQL connection finds the
/// usages in every SQL file of `sql-basic`, each analysed in its own dialect; `mysql/schema.sql`
/// is marked as another dialect.
#[test]
fn sql_basic_usages_in_every_file_and_dialect() {
    use ferris_idea_database::{Refactoring, file_dialect, refactor};
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/sql-basic");
    // The schema as the SQLite build of the same files reads it.
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(&std::fs::read_to_string(root.join("schema.sql")).unwrap())
        .unwrap();
    let file =
        std::env::temp_dir().join(format!("ferris-idea-usages-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&file);
    db.execute("vacuum into ?1", [file.display().to_string()])
        .unwrap();
    let tables = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let profile =
            ConnectionProfile::from_url("ref", &format!("sqlite:{}", file.display())).unwrap();
        let s = Session::connect(&profile, &Credentials::NotNeeded)
            .await
            .unwrap();
        introspect(&s).await.unwrap()
    });
    let mut hits = Vec::new();
    for rel in [
        "schema.sql",
        "seed.sql",
        "mysql/schema.sql",
        "queries/grid.sql",
        "seed-movements.postgresql.sql",
    ] {
        let text = std::fs::read_to_string(root.join(rel)).unwrap();
        let dialect = file_dialect(Path::new(rel)).unwrap_or(Dialect::PostgreSql);
        let r = refactor(
            &text,
            dialect,
            &tables,
            "item",
            Some("unit_price_cents"),
            &Refactoring::Rename,
            "price_cents",
        )
        .unwrap();
        if !r.edits.is_empty() {
            hits.push((rel, dialect != Dialect::PostgreSql));
        }
    }
    assert!(hits.contains(&("schema.sql", false)), "{hits:?}");
    assert!(hits.contains(&("seed.sql", false)), "{hits:?}");
    assert!(hits.contains(&("mysql/schema.sql", true)), "{hits:?}");
    let _ = std::fs::remove_file(&file);
}
