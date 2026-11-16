//! SPEC-0018 round trips: generated DDL, run on a real database and read back, leaves no
//! differences. SQLite always; PostgreSQL/MySQL when `IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB`
//! are set (work happens in a scratch schema/database that is dropped afterwards).

use crate::common;

use std::path::{Path, PathBuf};

use idecraft_database::{
    ConnectionProfile, DdlStatement, Dialect, OnError, Outcome, Session, Table, compare,
    create_script, introspect, migration,
};

/// Target → source exercises every kind of change of behaviour 5.
const SOURCE: &str = "\
create table warehouse (id integer primary key, city varchar(40) not null);
create table item (id integer primary key, name varchar(60) not null default 'x', wid integer,
  qty integer default 0, constraint item_wh_fk foreign key (wid) references warehouse (id));
create index item_name_idx on item (name);
create unique index item_qty_idx on item (qty);
create table tag (id integer primary key, item_id integer not null,
  constraint tag_item_fk foreign key (item_id) references item (id))";

const TARGET: &str = "\
create table warehouse (id integer primary key, city varchar(40) not null);
create table item (id integer primary key, name varchar(40), legacy varchar(40), wid integer);
create index item_legacy_idx on item (legacy);
create table old (id integer primary key)";

async fn run(s: &Session, sql: &str) {
    let run = s.run_script(sql, OnError::Stop).await;
    for o in &run.outcomes {
        assert!(
            !matches!(o.outcome, Outcome::Error(_)),
            "{:?} in\n{sql}",
            o.outcome
        );
    }
    assert_eq!(run.not_run, 0);
}

/// The first column of the rows `sql` returns.
async fn leftovers(s: &Session, sql: &str) -> Vec<String> {
    s.run_script(sql, OnError::Stop)
        .await
        .outcomes
        .into_iter()
        .flat_map(|o| match o.outcome {
            Outcome::ResultSet { rows, .. } => rows
                .into_iter()
                .filter_map(|r| r.into_iter().next().flatten())
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

async fn apply(s: &Session, statements: &[DdlStatement]) {
    let script: Vec<&str> = statements.iter().map(|d| d.sql.as_str()).collect();
    run(s, &script.join(";\n")).await;
}

async fn read(s: &Session) -> Vec<Table> {
    introspect(s).await.unwrap()
}

async fn connect(url: &str) -> Session {
    let profile = ConnectionProfile::from_url("ddl", url).unwrap();
    Session::connect(&profile, &common::credentials(&profile))
        .await
        .unwrap()
}

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("idecraft-ddl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    async fn sqlite(&self, name: &str) -> Session {
        let file = self.0.join(name);
        rusqlite::Connection::open(&file).unwrap();
        connect(&format!("sqlite:{}", file.display())).await
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn sqlite_migration_round_trip() {
    let dir = Dir::new("migrate");
    let (source, target) = (dir.sqlite("s.sqlite").await, dir.sqlite("t.sqlite").await);
    run(&source, SOURCE).await;
    run(&target, TARGET).await;
    let (s, t) = (read(&source).await, read(&target).await);
    assert!(!compare(&s, &t, Dialect::Sqlite).is_empty());
    let statements = migration(&s, &t, Dialect::Sqlite);
    assert!(statements.iter().any(|d| d.sql.contains("__idecraft_new")));
    apply(&target, &statements).await;
    let diff = compare(&s, &read(&target).await, Dialect::Sqlite);
    assert!(diff.is_empty(), "{diff:#?}");
}

#[tokio::test]
async fn sqlite_create_script_of_the_reference_schema() {
    let dir = Dir::new("create");
    let reference = dir.sqlite("ref.sqlite").await;
    let schema = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/reference-projects/sql-basic/schema.sql");
    run(&reference, &std::fs::read_to_string(schema).unwrap()).await;
    let tables = read(&reference).await;
    let copy = dir.sqlite("copy.sqlite").await;
    apply(&copy, &create_script(&tables, Dialect::Sqlite)).await;
    let diff = compare(&tables, &read(&copy).await, Dialect::Sqlite);
    assert!(diff.is_empty(), "{diff:#?}");
}

/// Runs both round trips on a server session. `scratch` creates and enters a scratch
/// namespace, `leave` drops it again and returns to the reference one.
async fn server_round_trips(
    s: &Session,
    dialect: Dialect,
    scratch: impl Fn(&str) -> String,
    enter: impl Fn(&str) -> String,
    drop: impl Fn(&str) -> String,
    home: &str,
) {
    let pid = std::process::id();
    // D22: scratch namespaces of earlier runs that ended early (a failed assertion skips the drop at
    // the end) are dropped first; those of runs still alive, e.g. in another worktree, stay.
    let list = match dialect {
        Dialect::PostgreSql => {
            "select nspname from pg_namespace where nspname like 'idecraft\\_ddl\\_%'"
        }
        _ => {
            "select schema_name from information_schema.schemata where schema_name like 'idecraft\\_ddl\\_%'"
        }
    };
    for name in leftovers(s, list).await {
        let alive = name
            .rsplit('_')
            .next()
            .and_then(|p| p.parse::<u32>().ok())
            .is_some_and(|p| {
                p == pid
                    || std::process::Command::new("kill")
                        .args(["-0", &p.to_string()])
                        .stderr(std::process::Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success())
            });
        if !alive {
            run(s, &drop(&name)).await;
        }
    }
    let names = [
        format!("idecraft_ddl_src_{pid}"),
        format!("idecraft_ddl_tgt_{pid}"),
        format!("idecraft_ddl_copy_{pid}"),
    ];
    for n in &names {
        run(s, &drop(n)).await;
        run(s, &scratch(n)).await;
    }

    // AK-4: the reference schema recreated from its CREATE script.
    run(s, &enter(home)).await;
    let reference = read(s).await;
    assert!(reference.iter().any(|t| t.name == "movement"));
    run(s, &enter(&names[2])).await;
    apply(s, &create_script(&reference, dialect)).await;
    let diff = compare(&reference, &read(s).await, dialect);
    assert!(diff.is_empty(), "{diff:#?}");

    // AK-3: migration target → source.
    run(s, &enter(&names[0])).await;
    run(s, SOURCE).await;
    let source = read(s).await;
    run(s, &enter(&names[1])).await;
    run(s, TARGET).await;
    let target = read(s).await;
    apply(s, &migration(&source, &target, dialect)).await;
    let diff = compare(&source, &read(s).await, dialect);
    assert!(diff.is_empty(), "{diff:#?}");

    run(s, &enter(home)).await;
    for n in &names {
        run(s, &drop(n)).await;
    }
}

#[tokio::test]
async fn postgres_round_trips() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let s = connect(&url).await;
    server_round_trips(
        &s,
        Dialect::PostgreSql,
        |n| format!("create schema {n}"),
        |n| format!("set search_path to {n}"),
        |n| format!("drop schema if exists {n} cascade"),
        "public",
    )
    .await;
}

#[tokio::test]
async fn mysql_round_trips() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_MYSQL") else {
        return;
    };
    let s = connect(&url).await;
    server_round_trips(
        &s,
        Dialect::MySql,
        |n| format!("create database {n}"),
        |n| format!("use {n}"),
        |n| format!("drop database if exists {n}"),
        "inventory",
    )
    .await;
}

#[tokio::test]
async fn mariadb_round_trips() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_MARIADB") else {
        return;
    };
    let s = connect(&url).await;
    server_round_trips(
        &s,
        Dialect::MariaDb,
        |n| format!("create database {n}"),
        |n| format!("use {n}"),
        |n| format!("drop database if exists {n}"),
        "inventory",
    )
    .await;
}
