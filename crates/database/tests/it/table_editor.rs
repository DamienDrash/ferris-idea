//! SPEC-0190 (server pages, `WHERE`/`ORDER BY`, manual transactions) and SPEC-0191 (copying a
//! table) against SQLite (always) and PostgreSQL/MySQL/MariaDB when `FERRIS_IDEA_TEST_PG` /
//! `FERRIS_IDEA_TEST_MYSQL` / `FERRIS_IDEA_TEST_MARIADB` are set. Every run creates throwaway `m56_*`
//! tables (and, on servers, a throwaway schema), drops leftovers of a crashed run first and drops
//! them again at the end.

use crate::common;

use ferris_idea_database::{
    Applied, ConnectionProfile, CopyOptions, CopyTarget, Dialect, EditSet, EditTarget, Failure,
    OnError, Outcome, Session, TableQuery, Values, begin_sql, copy_table, introspect,
};

type Rows = Vec<Vec<Option<String>>>;

async fn run(s: &Session, sql: &str) -> Result<Rows, String> {
    let run = s.run_script(sql, OnError::Stop).await;
    let mut rows = Vec::new();
    for o in run.outcomes {
        match o.outcome {
            Outcome::ResultSet { rows: r, .. } => rows = r,
            Outcome::UpdateCount(_) => {}
            Outcome::Error(e) => return Err(format!("{sql}: {e}")),
        }
    }
    Ok(rows)
}

fn texts(rows: &Rows, col: usize) -> Vec<String> {
    rows.iter()
        .map(|r| r[col].clone().unwrap_or_default())
        .collect()
}

/// Server pages, filter and order (SPEC-0190 AK-3) on `table` with 1 200 rows `n = 1..1200`.
async fn pages(s: &Session, table: &str) {
    let d = s.dialect();
    let mut q = TableQuery::new(table, vec!["id".into()]);
    let page = run(s, &q.page_sql(d, 2, 500).unwrap()).await.unwrap();
    assert_eq!(page.len(), 200);
    assert_eq!(page[0][0].as_deref(), Some("1001"));
    let count = run(s, &q.count_sql(d).unwrap()).await.unwrap();
    assert_eq!(count[0][0].as_deref(), Some("1200"));
    q.filter = "n > 1100".into();
    q.order = "n DESC".into();
    let page = run(s, &q.page_sql(d, 0, 500).unwrap()).await.unwrap();
    assert_eq!(page.len(), 100);
    assert_eq!(page[0][0].as_deref(), Some("1200"));
    let count = run(s, &q.count_sql(d).unwrap()).await.unwrap();
    assert_eq!(count[0][0].as_deref(), Some("100"));
    q.filter = "1=1; drop table x".into();
    assert!(q.page_sql(d, 0, 500).is_err());
}

fn row(id: &str, n: &str) -> Values {
    vec![("id".into(), Some(id.into())), ("n".into(), Some(n.into()))]
}

/// Manual transaction mode (SPEC-0190 behaviour 9): a batch is visible in its own session only,
/// a conflicting batch rolls back to its savepoint, rollback discards everything, commit keeps it.
async fn manual(s: &Session, other: &Session, table: &str) {
    let d = s.dialect();
    let tables = introspect(s).await.unwrap();
    let t = tables.iter().find(|t| t.name == table).unwrap();
    let target = EditTarget::new(t, d).unwrap();
    let n_of = |id: &'static str| format!("select n from {table} where id = {id}");
    run(s, begin_sql(d)).await.unwrap();
    let mut first = EditSet::new(target.clone());
    first
        .set_cell(&row("1", "1"), "n", Some("-1".into()))
        .unwrap();
    assert_eq!(
        first.apply_in_transaction(s, "ferris_idea_1").await,
        Applied::Committed(1)
    );
    assert_eq!(texts(&run(s, &n_of("1")).await.unwrap(), 0), ["-1"]);
    assert_eq!(texts(&run(other, &n_of("1")).await.unwrap(), 0), ["1"]);
    // Stale loaded value: a conflict that only undoes this batch.
    let mut second = EditSet::new(target.clone());
    second
        .set_cell(&row("2", "2"), "n", Some("-2".into()))
        .unwrap();
    second
        .set_cell(&row("3", "999"), "n", Some("-3".into()))
        .unwrap();
    let applied = second.apply_in_transaction(s, "ferris_idea_2").await;
    assert!(
        matches!(
            applied,
            Applied::RolledBack {
                index: Some(_),
                failure: Failure::Conflict(0)
            }
        ),
        "{applied:?}"
    );
    assert_eq!(texts(&run(s, &n_of("2")).await.unwrap(), 0), ["2"]);
    assert_eq!(texts(&run(s, &n_of("1")).await.unwrap(), 0), ["-1"]);
    run(s, "ROLLBACK").await.unwrap();
    assert_eq!(texts(&run(s, &n_of("1")).await.unwrap(), 0), ["1"]);
    run(s, begin_sql(d)).await.unwrap();
    assert_eq!(
        first.apply_in_transaction(s, "ferris_idea_1").await,
        Applied::Committed(1)
    );
    run(s, "COMMIT").await.unwrap();
    assert_eq!(texts(&run(other, &n_of("1")).await.unwrap(), 0), ["-1"]);
}

/// Copying (SPEC-0191 AK-2): constraints and rows kept, a violating insert fails, an existing
/// target is refused, a failing copy leaves no table.
async fn copy(s: &Session, table: &str, schema: Option<&str>) {
    let d = s.dialect();
    let target = CopyTarget::default_for(table, schema);
    let report = copy_table(s, table, &target, CopyOptions::default()).await;
    assert_eq!(report.error, None, "{report:?}");
    assert_eq!(report.rows, Some(3), "{report:?}");
    let create = &report.statements[0];
    for part in ["PRIMARY KEY", "NOT NULL", "DEFAULT", "UNIQUE", "CHECK"] {
        assert!(create.contains(part), "{part} missing in {create}");
    }
    let name = target.name(d);
    let rows = run(s, &format!("select id, sku, qty from {name} order by id"))
        .await
        .unwrap();
    assert_eq!(texts(&rows, 1), ["a", "b", "c"]);
    for violating in [
        format!("insert into {name} (id, sku, qty) values (10, 'z', -5)"),
        format!("insert into {name} (id, sku, qty) values (11, 'a', 1)"),
        format!("insert into {name} (id, sku, qty) values (1, 'y', 1)"),
        format!("insert into {name} (id, qty) values (12, 1)"),
    ] {
        assert!(run(s, &violating).await.is_err(), "{violating} succeeded");
    }
    let defaulted = run(s, &format!("insert into {name} (id, sku) values (13, 'd')")).await;
    assert!(defaulted.is_ok(), "{defaulted:?}");
    let qty = run(s, &format!("select qty from {name} where id = 13"))
        .await
        .unwrap();
    assert_eq!(texts(&qty, 0), ["7"]);
    let again = copy_table(s, table, &target, CopyOptions::default()).await;
    assert!(
        again
            .error
            .as_ref()
            .is_some_and(|(_, m)| m.contains("exists")),
        "{again:?}"
    );
    run(s, &format!("drop table {name}")).await.unwrap();
    // A failing copy (the target schema does not exist) leaves no table behind.
    let missing = CopyTarget {
        schema: Some("ferris_idea_no_such_schema".into()),
        table: table.into(),
    };
    let failed = copy_table(s, table, &missing, CopyOptions::default()).await;
    assert!(failed.error.is_some(), "{failed:?}");
    assert!(
        run(s, &format!("select * from {}", missing.name(d)))
            .await
            .is_err(),
        "no table after a failed copy"
    );
}

async fn exercise(s: &Session, other: &Session, suffix: &str, schema: Option<&str>) {
    let d = s.dialect();
    let big = format!("m56_pages_{suffix}");
    let items = format!("m56_item_{suffix}");
    let numbers = match d {
        Dialect::PostgreSql => {
            format!("insert into {big} (id, n) select g, g from generate_series(1, 1200) g")
        }
        // MySQL and MariaDB stop recursion after 1 000 steps: 600 rows, then 600 more.
        _ => format!(
            "insert into {big} (id, n) with recursive r(i) as (select 1 union all select i + 1 from r where i < 600) select i, i from r"
        ),
    };
    let doubled = (d != Dialect::PostgreSql)
        .then(|| format!("insert into {big} (id, n) select id + 600, n + 600 from {big}"));
    let sku = if d == Dialect::Sqlite {
        "text"
    } else {
        "varchar(20)"
    };
    for sql in [
        format!("drop table if exists {big}"),
        format!("drop table if exists {items}"),
        format!("create table {big} (id integer primary key, n integer not null)"),
        numbers,
        format!(
            "create table {items} (id integer primary key, sku {sku} not null unique, \
             qty integer default 7 check (qty >= 0))"
        ),
        format!("insert into {items} (id, sku, qty) values (1, 'a', 1), (2, 'b', 2), (3, 'c', 3)"),
    ]
    .into_iter()
    .chain(doubled)
    {
        run(s, &sql).await.unwrap();
    }
    pages(s, &big).await;
    manual(s, other, &big).await;
    copy(s, &items, schema).await;
    run(s, &format!("drop table {big}")).await.unwrap();
    run(s, &format!("drop table {items}")).await.unwrap();
}

async fn connect(url: &str) -> Session {
    let profile = ConnectionProfile::from_url("m56", url).unwrap();
    Session::connect(&profile, &common::credentials(&profile))
        .await
        .unwrap()
}

#[tokio::test]
async fn sqlite_table_editor_and_copy() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-m56-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("m56.sqlite");
    let _ = std::fs::remove_file(&file);
    rusqlite::Connection::open(&file).unwrap();
    let url = format!("sqlite:{}", file.display());
    let s = connect(&url).await;
    let other = connect(&url).await;
    exercise(&s, &other, "lite", None).await;
    drop((s, other));
    let _ = std::fs::remove_dir_all(&dir);
}

async fn server(var: &str, create_schema: &str, drop_schema: &str) {
    let Ok(url) = std::env::var(var) else {
        return;
    };
    let s = connect(&url).await;
    let other = connect(&url).await;
    // Fixed names: a run that crashed earlier left them behind, so they are dropped first.
    let suffix = "probe";
    let schema = format!("m56_copy_{suffix}");
    run(&s, &drop_schema.replace("{}", &schema)).await.unwrap();
    run(&s, &create_schema.replace("{}", &schema))
        .await
        .unwrap();
    exercise(&s, &other, suffix, Some(&schema)).await;
    run(&s, &drop_schema.replace("{}", &schema)).await.unwrap();
}

#[tokio::test]
async fn postgres_table_editor_and_copy() {
    server(
        "FERRIS_IDEA_TEST_PG",
        "create schema {}",
        "drop schema if exists {} cascade",
    )
    .await;
}

#[tokio::test]
async fn mysql_table_editor_and_copy() {
    server(
        "FERRIS_IDEA_TEST_MYSQL",
        "create database {}",
        "drop database if exists {}",
    )
    .await;
}

#[tokio::test]
async fn mariadb_table_editor_and_copy() {
    server(
        "FERRIS_IDEA_TEST_MARIADB",
        "create database {}",
        "drop database if exists {}",
    )
    .await;
}
