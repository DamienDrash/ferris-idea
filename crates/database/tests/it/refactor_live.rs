//! SPEC-0164/AK-4: the statements of renaming views and indexes and of changing a column's type run
//! on the reference PostgreSQL, MySQL and MariaDB servers (`FERRIS_IDEA_TEST_PG`,
//! `FERRIS_IDEA_TEST_MYSQL`, `FERRIS_IDEA_TEST_MARIADB`; without them nothing is done), in scratch tables
//! that are dropped afterwards.
//! SPEC-0019/AK-14 (v5, OBS-0068 L3): renamed WITH queries still run on PostgreSQL.

use crate::common;

use ferris_idea_database::{
    ConnectionProfile, OnError, Outcome, Session, change_type_ddl, ddl_in_transaction, introspect,
    rename, rename_index, run_refactoring,
};

async fn session(var: &str) -> Option<(Session, ConnectionProfile)> {
    let url = std::env::var(var).ok()?;
    let profile = ConnectionProfile::from_url("reference", &url).unwrap();
    let s = Session::connect(&profile, &common::credentials(&profile))
        .await
        .unwrap();
    Some((s, profile))
}

async fn ok(s: &Session, sql: &str) {
    let run = s.run_script(sql, OnError::Stop).await;
    for o in &run.outcomes {
        assert!(
            !matches!(o.outcome, Outcome::Error(_)),
            "{sql}: {:?}",
            o.outcome
        );
    }
}

async fn check(var: &str) {
    let Some((s, profile)) = session(var).await else {
        return;
    };
    // Leftovers of an earlier run that stopped half-way.
    for t in introspect(&s).await.unwrap() {
        if t.name.starts_with("rf_") {
            let kw = if t.kind == "view" { "view" } else { "table" };
            let _ = s
                .run_script(
                    &format!("drop {kw} if exists {}", t.name),
                    OnError::Continue,
                )
                .await;
        }
    }
    let tag = std::process::id();
    let (t, v, i) = (
        format!("rf_t{tag}"),
        format!("rf_v{tag}"),
        format!("rf_i{tag}"),
    );
    let _ = s
        .run_script(
            &format!("drop view if exists {v}; drop view if exists {v}x; drop table if exists {t}"),
            OnError::Continue,
        )
        .await;
    ok(
        &s,
        &format!("create table {t} (id int not null, qty int not null default 0)"),
    )
    .await;
    ok(&s, &format!("create index {i} on {t} (qty)")).await;
    ok(&s, &format!("create view {v} as select id from {t}")).await;
    let schema = introspect(&s).await.unwrap();
    let dialect = profile.dialect;
    // Change type: keeps NOT NULL and the default.
    ok(
        &s,
        &change_type_ddl(dialect, &schema, &t, "qty", "bigint").unwrap(),
    )
    .await;
    // Rename the index and the view.
    ok(
        &s,
        &rename_index("", dialect, &schema, &i, &format!("{i}x"))
            .unwrap()
            .ddl,
    )
    .await;
    ok(
        &s,
        &rename("", dialect, &schema, &v, None, &format!("{v}x"))
            .unwrap()
            .ddl,
    )
    .await;
    let after = introspect(&s).await.unwrap();
    let table = after.iter().find(|x| x.name == t).unwrap();
    let qty = table.columns.iter().find(|c| c.name == "qty").unwrap();
    assert!(qty.data_type.contains("bigint"), "{qty:?}");
    assert!(!qty.nullable, "{qty:?}");
    assert!(
        table.indexes.iter().any(|x| x.name == format!("{i}x")),
        "{:?}",
        table.indexes
    );
    assert!(
        after
            .iter()
            .any(|x| x.name == format!("{v}x") && x.kind == "view")
    );
    ok(&s, &format!("drop view {v}x; drop table {t}")).await;
}

#[tokio::test]
async fn postgres_view_index_and_type() {
    check("FERRIS_IDEA_TEST_PG").await;
    // Routines: PostgreSQL renames them with ALTER ROUTINE.
    let Some((s, profile)) = session("FERRIS_IDEA_TEST_PG").await else {
        return;
    };
    let f = format!("rf_f{}", std::process::id());
    let _ = s
        .run_script(
            &format!("drop function if exists {f}x(int)"),
            OnError::Continue,
        )
        .await;
    ok(
        &s,
        &format!("create function {f}(p int) returns int as $$ select p + 1 $$ language sql"),
    )
    .await;
    let r = ferris_idea_database::rename_routine(
        &format!("select {f}(1)"),
        profile.dialect,
        &f,
        &format!("{f}x"),
    )
    .unwrap();
    ok(&s, &r.ddl).await;
    let run = s.run_script(&r.sql, OnError::Stop).await;
    assert!(
        matches!(&run.outcomes[0].outcome, Outcome::ResultSet { rows, .. } if rows[0][0].as_deref() == Some("2")),
        "{run:?}"
    );
    ok(&s, &format!("drop function {f}x(int)")).await;
}

#[tokio::test]
async fn mysql_view_index_and_type() {
    check("FERRIS_IDEA_TEST_MYSQL").await;
}

#[tokio::test]
async fn mariadb_view_index_and_type() {
    check("FERRIS_IDEA_TEST_MARIADB").await;
}

/// SPEC-0164 v5 AK-13 (OBS-0070 V3): after renaming a sequence, the file's `nextval('…')` and
/// `'…'::regclass` name the new one and run; a literal written with another schema stays.
#[tokio::test]
async fn a_renamed_sequence_follows_in_nextval() {
    let Some((s, profile)) = session("FERRIS_IDEA_TEST_PG").await else {
        return;
    };
    let q = format!("rf_q{}", std::process::id());
    let _ = s
        .run_script(
            &format!("drop sequence if exists {q}; drop sequence if exists {q}x"),
            OnError::Continue,
        )
        .await;
    ok(&s, &format!("create sequence {q}")).await;
    let home = ferris_idea_database::current_schema(&s).await.unwrap();
    let script = format!(
        "select nextval('{q}'), currval('{home}.{q}'::regclass), nextval('other_schema.{q}')"
    );
    let r = ferris_idea_database::rename_sequence_in(
        &script,
        profile.dialect,
        &q,
        Some(&home),
        &format!("{q}x"),
    )
    .unwrap();
    assert_eq!(r.edits.len(), 2, "{r:?}");
    assert!(r.ambiguous.is_empty(), "{r:?}");
    ok(&s, &r.ddl).await;
    // The changed text runs without the literal that names another schema's sequence.
    let runnable = r.sql.replace(&format!(", nextval('other_schema.{q}')"), "");
    let run = s.run_script(&runnable, OnError::Stop).await;
    assert!(
        matches!(&run.outcomes[0].outcome, Outcome::ResultSet { rows, .. } if rows[0][0].as_deref() == Some("1") && rows[0][1].as_deref() == Some("1")),
        "{runnable}: {run:?}"
    );
    ok(&s, &format!("drop sequence {q}x")).await;
}

/// SPEC-0164 v3 AK-8: a refused second statement undoes the first where the database rolls DDL
/// back (PostgreSQL, SQLite, DuckDB), and leaves it done, reported, where DDL commits on its own
/// (MySQL, MariaDB). SQLite and DuckDB always run; the servers where their variables are set.
#[tokio::test]
async fn a_refused_derived_statement_rolls_everything_back() {
    let tag = std::process::id();
    let dir = std::env::temp_dir().join(format!("ferris-idea-rf-tx-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let duck = dir.join("tx.duckdb");
    duckdb::Connection::open(&duck).unwrap();
    rusqlite::Connection::open(dir.join("tx.sqlite")).unwrap();
    let mut urls = vec![
        format!("sqlite:{}", dir.join("tx.sqlite").display()),
        format!("duckdb:{}", duck.display()),
    ];
    for var in [
        "FERRIS_IDEA_TEST_PG",
        "FERRIS_IDEA_TEST_MYSQL",
        "FERRIS_IDEA_TEST_MARIADB",
    ] {
        urls.extend(std::env::var(var).ok());
    }
    for url in urls {
        let profile = ConnectionProfile::from_url("reference", &url).unwrap();
        let s = Session::connect(&profile, &common::credentials(&profile))
            .await
            .unwrap();
        let (a, b) = (format!("rf_tx_a{tag}"), format!("rf_tx_b{tag}"));
        let _ = s
            .run_script(
                &format!("drop table if exists {a}; drop table if exists {b}"),
                OnError::Continue,
            )
            .await;
        ok(&s, &format!("create table {a} (id int)")).await;
        let main = format!("ALTER TABLE {a} RENAME TO {b}");
        let bad = vec![format!(
            "ALTER TABLE rf_tx_missing{tag} RENAME TO rf_tx_z{tag}"
        )];
        let run = run_refactoring(&s, &main, &bad).await;
        let names: Vec<String> = introspect(&s)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        if ddl_in_transaction(profile.dialect) {
            assert!(run.transaction, "{url}");
            assert_eq!(run.failed.as_ref().map(|f| &f.0), Some(&bad[0]), "{url}");
            assert!(
                names.contains(&a) && !names.contains(&b),
                "{url}: {names:?}"
            );
        } else {
            assert!(!run.transaction && run.failed.is_none(), "{url}: {run:?}");
            assert_eq!(run.refused.len(), 1, "{url}");
            assert!(
                names.contains(&b) && !names.contains(&a),
                "{url}: {names:?}"
            );
        }
        let _ = s
            .run_script(
                &format!("drop table if exists {a}; drop table if exists {b}"),
                OnError::Continue,
            )
            .await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The columns and rows of the one query of `sql`.
async fn result(s: &Session, sql: &str) -> (Vec<String>, Vec<Vec<Option<String>>>) {
    let run = s.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().last().map(|o| o.outcome) {
        Some(Outcome::ResultSet { columns, rows }) => (columns, rows),
        other => panic!("{sql}: {other:?}"),
    }
}

/// SPEC-0019 v5 AK-14 (OBS-0068 L3): after renaming `pd_note.body`, the rewritten WITH queries of
/// `pd-refactor.postgresql.sql` still run and return the same rows under the same column names.
#[tokio::test]
async fn a_renamed_with_query_still_runs() {
    let Some((s, _)) = session("FERRIS_IDEA_TEST_PG").await else {
        return;
    };
    let schema = format!("rf_cte_{}", std::process::id());
    common::drop_leftovers(&s, &["rf_cte_"]).await;
    let _ = s
        .run_script(
            &format!("drop schema if exists {schema} cascade"),
            OnError::Continue,
        )
        .await;
    ok(
        &s,
        &format!(
            "create schema {schema}; set search_path to {schema}; \
             create table pd_note (id integer primary key, body varchar(40) not null, \
             qty integer not null default 0); \
             insert into pd_note values (1, 'first', 1), (2, 'second', 12), (3, 'third', 30)"
        ),
    )
    .await;
    let queries = [
        "with big as (select id, body from pd_note where qty > 10)\nselect b.body from big b order by 1",
        "with big as (select * from pd_note where qty > 10) select b.body from big b order by 1",
        "select s.body from (select id, body from pd_note) s where s.id > 1 order by 1",
    ];
    let before = {
        let mut out = Vec::new();
        for q in queries {
            out.push(result(&s, q).await);
        }
        out
    };
    let tables = introspect(&s).await.unwrap();
    let script = queries.join(";\n");
    let r = rename(
        &script,
        ferris_idea_database::Dialect::PostgreSql,
        &tables,
        "pd_note",
        Some("body"),
        "text_body",
    )
    .unwrap();
    assert!(r.ambiguous.is_empty(), "{r:?}");
    ok(&s, &r.ddl).await;
    let rewritten: Vec<&str> = r.sql.split(";\n").collect();
    assert_eq!(rewritten.len(), queries.len(), "{}", r.sql);
    for (i, q) in rewritten.iter().enumerate() {
        let (columns, rows) = result(&s, q).await;
        assert_eq!(rows, before[i].1, "{q}");
        // The `*` variant passes the new name through; the others keep `body`.
        if i == 1 {
            assert_eq!(columns, ["text_body"], "{q}");
        } else {
            assert_eq!(columns, before[i].0, "{q}");
        }
    }
    assert!(
        rewritten[0].contains("text_body as body"),
        "{}",
        rewritten[0]
    );
    assert!(rewritten[1].contains("b.text_body"), "{}", rewritten[1]);
    ok(&s, &format!("drop schema {schema} cascade")).await;
}
