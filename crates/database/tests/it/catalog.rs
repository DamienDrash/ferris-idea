//! SPEC-0089 v3 (schemas), SPEC-0014 v2 (fingerprint) and SPEC-0192 (default schema, row counts)
//! against SQLite (always) and PostgreSQL/MySQL/MariaDB when `IDECRAFT_TEST_PG` /
//! `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB` are set. Work happens in a scratch schema that
//! is dropped afterwards.

use crate::common;

use idecraft_database::{
    ConnectionProfile, OnError, Outcome, Session, complete, count_rows, current_schema, introspect,
    introspect_schema, list_schemas, object_definition, schema_fingerprint, schema_objects,
    sequences, show_ddl_in,
};

async fn run(s: &Session, sql: &str) {
    let run = s.run_script(sql, OnError::Stop).await;
    for o in &run.outcomes {
        assert!(
            !matches!(o.outcome, Outcome::Error(_)),
            "{:?} in\n{sql}",
            o.outcome
        );
    }
}

async fn connect(url: &str) -> Session {
    let profile = ConnectionProfile::from_url("catalog", url).unwrap();
    Session::connect(&profile, &common::credentials(&profile))
        .await
        .unwrap()
}

/// `watcher` caches the scratch schema; `other` renames a column; the fingerprint changes and the
/// schema read again completes the new name.
async fn exercise(watcher: &Session, other: &Session, scratch: &str, default: &str) {
    let schemas = list_schemas(watcher).await.unwrap();
    assert!(
        schemas.iter().any(|s| s.name == scratch),
        "{scratch} missing in {schemas:?}"
    );
    assert!(
        schemas.iter().any(|s| s.name == default && s.current),
        "{default} not current in {schemas:?}"
    );
    assert!(
        !schemas
            .iter()
            .any(|s| s.name == "information_schema" || s.name.starts_with("pg_")),
        "{schemas:?}"
    );
    assert_eq!(current_schema(watcher).await.unwrap(), default);

    let tables = introspect_schema(watcher, Some(scratch)).await.unwrap();
    assert_eq!(tables.len(), 1, "{tables:?}");
    assert_eq!(tables[0].name, "probe");
    assert_eq!(
        count_rows(watcher, &format!("{scratch}.probe"))
            .await
            .unwrap(),
        2
    );

    // The watcher now works in the scratch schema, as a console of it would.
    let before = schema_fingerprint(other).await.unwrap();
    let cached = introspect(other).await.unwrap();
    assert!(cached.iter().any(|t| t.name == "probe"));
    run(watcher, "alter table probe rename column label to title").await;
    let after = schema_fingerprint(other).await.unwrap();
    assert_ne!(before, after);
    let fresh = introspect(other).await.unwrap();
    let names: Vec<String> = complete("select  from probe", 7, other.dialect(), &fresh)
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert!(names.contains(&"title".to_owned()), "{names:?}");
    assert_eq!(schema_fingerprint(other).await.unwrap(), after);
}

#[tokio::test]
async fn sqlite_schemas_fingerprint_and_counts() {
    let dir = std::env::temp_dir().join(format!("idecraft-catalog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.db");
    rusqlite::Connection::open(&file).unwrap();
    let s = connect(&format!("sqlite:{}", file.display())).await;
    run(
        &s,
        "create table probe (id integer primary key, label text);
         insert into probe (label) values ('a'), ('b')",
    )
    .await;
    assert_eq!(current_schema(&s).await.unwrap(), "main");
    let attached = dir.join("aux.db");
    rusqlite::Connection::open(&attached).unwrap();
    run(
        &s,
        &format!(
            "attach database '{}' as aux; create table aux.other (x int)",
            attached.display()
        ),
    )
    .await;
    let schemas = list_schemas(&s).await.unwrap();
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["aux", "main"]);
    let aux = introspect_schema(&s, Some("aux")).await.unwrap();
    assert_eq!(aux[0].name, "other");
    assert_eq!(aux[0].columns[0].name, "x");
    assert_eq!(count_rows(&s, "probe").await.unwrap(), 2);
    let before = schema_fingerprint(&s).await.unwrap();
    run(&s, "alter table probe rename column label to title").await;
    assert_ne!(schema_fingerprint(&s).await.unwrap(), before);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn postgres_schemas_fingerprint_and_counts() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let scratch = format!("idecraft_cat_{}", std::process::id());
    let watcher = connect(&url).await;
    let other = connect(&url).await;
    run(
        &watcher,
        &format!(
            "drop schema if exists {scratch} cascade; create schema {scratch};
             create table {scratch}.probe (id integer primary key, label text);
             insert into {scratch}.probe values (1, 'a'), (2, 'b')"
        ),
    )
    .await;
    let default = current_schema(&watcher).await.unwrap();
    run(&other, &format!("set search_path to {scratch}")).await;
    exercise_switching(&watcher, &other, &scratch, &default, "set search_path to").await;
    run(&watcher, &format!("drop schema {scratch} cascade")).await;
}

#[tokio::test]
async fn mysql_schemas_fingerprint_and_counts() {
    server_like_mysql("IDECRAFT_TEST_MYSQL").await;
}

#[tokio::test]
async fn mariadb_schemas_fingerprint_and_counts() {
    server_like_mysql("IDECRAFT_TEST_MARIADB").await;
}

async fn server_like_mysql(var: &str) {
    let Ok(url) = std::env::var(var) else {
        return;
    };
    let scratch = format!("idecraft_cat_{}", std::process::id());
    let watcher = connect(&url).await;
    let other = connect(&url).await;
    run(
        &watcher,
        &format!(
            "drop database if exists {scratch}; create database {scratch};
             create table {scratch}.probe (id integer primary key, label varchar(20));
             insert into {scratch}.probe values (1, 'a'), (2, 'b')"
        ),
    )
    .await;
    let default = current_schema(&watcher).await.unwrap();
    run(&other, &format!("use {scratch}")).await;
    exercise_switching(&watcher, &other, &scratch, &default, "use").await;
    run(&watcher, &format!("drop database {scratch}")).await;
}

/// Runs [`exercise`] with the watcher in its default schema for the listing, then switched to the
/// scratch schema (with `switch`, e.g. `use`) for the rename.
async fn exercise_switching(
    watcher: &Session,
    other: &Session,
    scratch: &str,
    default: &str,
    switch: &str,
) {
    let schemas = list_schemas(watcher).await.unwrap();
    assert!(schemas.iter().any(|s| s.name == default && s.current));
    assert_eq!(current_schema(watcher).await.unwrap(), default);
    run(watcher, &format!("{switch} {scratch}")).await;
    exercise(watcher, other, scratch, scratch).await;
}

/// SPEC-0089 v5 AK-11: sequences are listed for PostgreSQL only (the serial column's sequence);
/// SQLite has none.
#[tokio::test]
async fn sequences_of_a_schema() {
    let dir = std::env::temp_dir().join(format!("idecraft-seq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.db");
    rusqlite::Connection::open(&file).unwrap();
    let s = connect(&format!("sqlite:{}", file.display())).await;
    run(
        &s,
        "create table probe (id integer primary key autoincrement)",
    )
    .await;
    assert!(sequences(&s, None).await.unwrap().is_empty());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);

    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let scratch = format!("idecraft_seq_{}", std::process::id());
    let pg = connect(&url).await;
    run(
        &pg,
        &format!(
            "drop schema if exists {scratch} cascade; create schema {scratch};
             create table {scratch}.probe (id serial primary key)"
        ),
    )
    .await;
    assert_eq!(
        sequences(&pg, Some(&scratch)).await.unwrap(),
        ["probe_id_seq"]
    );
    run(&pg, &format!("drop schema {scratch} cascade")).await;
}

/// SPEC-0013 v6 AK-14: the triggers of a SQLite database and of an attached one, with their table
/// and when they fire; reading them changes nothing.
#[tokio::test]
async fn triggers_of_a_sqlite_schema() {
    let dir = std::env::temp_dir().join(format!("idecraft-objects-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.db");
    rusqlite::Connection::open(&file).unwrap();
    let other = dir.join("other.db");
    rusqlite::Connection::open(&other).unwrap();
    let s = connect(&format!("sqlite:{}", file.display())).await;
    run(
        &s,
        &format!(
            "create table stock (sku text, qty integer);
             create trigger b_ins before insert on stock begin select 1; end;
             create trigger a_upd after update of qty on stock begin select 1; end;
             attach database '{}' as other;
             create table other.t (id integer);
             create trigger other.t_del delete on t begin select 1; end",
            other.display()
        ),
    )
    .await;
    let before = schema_fingerprint(&s).await.unwrap();
    let main = schema_objects(&s, None).await.unwrap();
    let names: Vec<_> = main
        .iter()
        .map(|o| (o.name.as_str(), o.table.as_deref(), o.detail.as_deref()))
        .collect();
    assert_eq!(
        names,
        [
            ("a_upd", Some("stock"), Some("AFTER UPDATE OF qty")),
            ("b_ins", Some("stock"), Some("BEFORE INSERT")),
        ]
    );
    let attached = schema_objects(&s, Some("other")).await.unwrap();
    assert_eq!(attached.len(), 1);
    assert_eq!(attached[0].name, "t_del");
    assert_eq!(attached[0].detail.as_deref(), Some("BEFORE DELETE"));
    // SPEC-0013 v7 AK-17: the definitions as SQLite stores them, also in the attached database.
    assert_eq!(
        object_definition(&s, None, &main[0])
            .await
            .unwrap()
            .as_deref(),
        Some("CREATE TRIGGER a_upd after update of qty on stock begin select 1; end;")
    );
    assert_eq!(
        object_definition(&s, Some("other"), &attached[0])
            .await
            .unwrap()
            .as_deref(),
        Some("CREATE TRIGGER t_del delete on t begin select 1; end;")
    );
    let mut gone = main[0].clone();
    gone.name = "gone".into();
    assert_eq!(object_definition(&s, None, &gone).await.unwrap(), None);
    assert_eq!(schema_fingerprint(&s).await.unwrap(), before);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Objects of every kind in `scratch` of a PostgreSQL server: overloads, a procedure, aggregates,
/// triggers with their function, the four kinds of types and a sequence.
const PG_OBJECTS: &str =
    "create table stock (sku text primary key, qty integer not null default 0);
    create sequence ticket_seq;
    create function add_tax(amount numeric) returns numeric language sql immutable
        as $$ select amount * 1.19 $$;
    create function add_tax(amount numeric, rate numeric) returns numeric language sql
        as $$ select amount * (1 + rate) $$;
    create function add_tax() returns numeric language sql as $$ select 0::numeric $$;
    create procedure restock(p_sku text, p_qty integer) language plpgsql
        as $$ begin update stock set qty = qty + p_qty where sku = p_sku; end $$;
    create function sum_sq_step(state numeric, v numeric) returns numeric language sql immutable
        as $$ select state + v * v $$;
    create function sum_sq_final(state numeric) returns numeric language sql immutable
        as $$ select sqrt(state) $$;
    create aggregate norm(numeric) (sfunc = sum_sq_step, stype = numeric,
        finalfunc = sum_sq_final, initcond = '0');
    create aggregate plain_sum(integer) (sfunc = int4pl, stype = integer);
    create function stock_guard() returns trigger language plpgsql
        as $$ begin if new.qty < 0 then raise exception 'negative'; end if; return new; end $$;
    create trigger stock_check before insert or update on stock
        for each row execute function stock_guard();
    create trigger stock_audit after delete on stock
        for each statement execute function stock_guard();
    create type mood as enum ('sad', 'ok', 'it''s fine');
    create domain positive_qty as integer default 1 not null
        constraint qty_positive check (value > 0);
    create type price_range as range (subtype = numeric);
    create type address as (street text, zip varchar(10), \"House No\" integer)";

/// SPEC-0013 v6 AK-14 (live, `IDECRAFT_TEST_PG`; skipped without it): every kind of a PostgreSQL
/// schema with arguments, tables and timings; the routines PostgreSQL makes itself for a range
/// type and the row type of the table are not listed.
#[tokio::test]
async fn postgres_schema_objects() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let scratch = format!("idecraft_obj_{}", std::process::id());
    let pg = connect(&url).await;
    run(
        &pg,
        &format!(
            "drop schema if exists {scratch} cascade; create schema {scratch};
             set search_path to {scratch}; {PG_OBJECTS}; set search_path to default"
        ),
    )
    .await;
    let listed = schema_objects(&pg, Some(&scratch)).await;
    run(&pg, &format!("drop schema {scratch} cascade")).await;
    let got: Vec<(String, String, Option<String>, Option<String>)> = listed
        .unwrap()
        .into_iter()
        .map(|o| (o.kind, o.name, o.table, o.detail))
        .collect();
    let o = |kind: &str, name: &str, table: Option<&str>, detail: Option<&str>| {
        (
            kind.to_owned(),
            name.to_owned(),
            table.map(str::to_owned),
            detail.map(str::to_owned),
        )
    };
    assert_eq!(
        got,
        [
            o("sequence", "ticket_seq", None, None),
            o("function", "add_tax", None, Some("()")),
            o("function", "add_tax", None, Some("(amount numeric)")),
            o(
                "function",
                "add_tax",
                None,
                Some("(amount numeric, rate numeric)")
            ),
            o("function", "stock_guard", None, Some("()")),
            o("function", "sum_sq_final", None, Some("(state numeric)")),
            o(
                "function",
                "sum_sq_step",
                None,
                Some("(state numeric, v numeric)")
            ),
            o(
                "procedure",
                "restock",
                None,
                Some("(IN p_sku text, IN p_qty integer)")
            ),
            o("aggregate", "norm", None, Some("(numeric)")),
            o("aggregate", "plain_sum", None, Some("(integer)")),
            o(
                "trigger",
                "stock_audit",
                Some("stock"),
                Some("AFTER DELETE")
            ),
            o(
                "trigger",
                "stock_check",
                Some("stock"),
                Some("BEFORE INSERT OR UPDATE")
            ),
            o("type", "address", None, Some("composite")),
            o("type", "mood", None, Some("enum")),
            o("type", "positive_qty", None, Some("domain")),
            o("type", "price_range", None, Some("range")),
        ]
    );
}

/// Routines and triggers in `scratch` of a MySQL or MariaDB server; each statement is one line
/// so it needs no `DELIMITER`.
fn mysql_objects(scratch: &str) -> String {
    format!(
        "create table {scratch}.stock (sku varchar(20) primary key, qty int not null default 0);
         create function {scratch}.add_tax(amount decimal(10,2)) returns decimal(10,2) deterministic return amount * 1.19;
         create procedure {scratch}.restock(in p_sku varchar(20), in p_qty int) update {scratch}.stock set qty = qty + p_qty where sku = p_sku;
         create trigger {scratch}.stock_check before insert on {scratch}.stock for each row set new.qty = greatest(new.qty, 0);
         create trigger {scratch}.stock_audit after update on {scratch}.stock for each row set @last_sku = new.sku"
    )
}

/// SPEC-0013 v6 AK-14 (live, `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB`; skipped without
/// them): the functions, procedures and triggers of a database with table and timing.
#[tokio::test]
async fn mysql_schema_objects() {
    server_like_mysql_objects("IDECRAFT_TEST_MYSQL").await;
}

#[tokio::test]
async fn mariadb_schema_objects() {
    server_like_mysql_objects("IDECRAFT_TEST_MARIADB").await;
}

async fn server_like_mysql_objects(var: &str) {
    let Ok(url) = std::env::var(var) else {
        return;
    };
    let scratch = format!("idecraft_obj_{}", std::process::id());
    let my = connect(&url).await;
    run(
        &my,
        &format!(
            "drop database if exists {scratch}; create database {scratch}; {}",
            mysql_objects(&scratch)
        ),
    )
    .await;
    let listed = schema_objects(&my, Some(&scratch)).await;
    run(&my, &format!("drop database {scratch}")).await;
    let got: Vec<(String, String, Option<String>, Option<String>)> = listed
        .unwrap()
        .into_iter()
        .map(|o| (o.kind, o.name, o.table, o.detail))
        .collect();
    let o = |kind: &str, name: &str, table: Option<&str>, detail: Option<&str>| {
        (
            kind.to_owned(),
            name.to_owned(),
            table.map(str::to_owned),
            detail.map(str::to_owned),
        )
    };
    assert_eq!(
        got,
        [
            o("function", "add_tax", None, None),
            o("procedure", "restock", None, None),
            o(
                "trigger",
                "stock_audit",
                Some("stock"),
                Some("AFTER UPDATE")
            ),
            o(
                "trigger",
                "stock_check",
                Some("stock"),
                Some("BEFORE INSERT")
            ),
        ]
    );
}

/// The first cell of the first row `sql` returns on `s`.
async fn first_cell(s: &Session, sql: &str) -> String {
    let run = s.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { rows, .. }) => rows[0][0].clone().unwrap_or_default(),
        other => panic!("{sql}: {other:?}"),
    }
}

/// SPEC-0013 v7 AK-16 (live, `IDECRAFT_TEST_PG`; skipped without it): the definition of every
/// object of a PostgreSQL schema — routines and triggers as the server's own functions give them,
/// the overload chosen by its arguments, aggregates and types written from the catalogue; none for
/// a sequence.
#[tokio::test]
async fn postgres_object_definitions() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let scratch = format!("idecraft_def_{}", std::process::id());
    let pg = connect(&url).await;
    run(
        &pg,
        &format!(
            "drop schema if exists {scratch} cascade; create schema {scratch};
             set search_path to {scratch}; {PG_OBJECTS}; set search_path to default"
        ),
    )
    .await;
    let mut got = Vec::new();
    let mut server = Vec::new();
    for o in schema_objects(&pg, Some(&scratch)).await.unwrap() {
        let ddl = object_definition(&pg, Some(&scratch), &o).await.unwrap();
        let own = match o.kind.as_str() {
            "function" | "procedure" => Some(
                first_cell(
                    &pg,
                    // The overloads here differ in their number of arguments.
                    &format!(
                        "select pg_get_functiondef(oid) from pg_proc \
                         where pronamespace = '{scratch}'::regnamespace and proname = '{}' \
                         and pronargs = {}",
                        o.name,
                        match o.detail.as_deref().unwrap_or("()") {
                            "()" => 0,
                            args => args.matches(',').count() + 1,
                        }
                    ),
                )
                .await,
            ),
            "trigger" => Some(
                first_cell(
                    &pg,
                    &format!(
                        "select pg_get_triggerdef(oid, true) || ';' from pg_trigger \
                         where tgname = '{}' and tgrelid = '{scratch}.stock'::regclass",
                        o.name
                    ),
                )
                .await,
            ),
            _ => None,
        };
        if let Some(own) = own {
            server.push((o.name.clone(), ddl.clone(), Some(own.trim_end().to_owned())));
        }
        got.push((o.kind, o.name, ddl));
    }
    run(&pg, &format!("drop schema {scratch} cascade")).await;
    for (name, ours, theirs) in server {
        assert_eq!(ours, theirs, "{name}");
    }
    let s = &scratch;
    let text = |kind: &str, name: &str| {
        got.iter()
            .find(|(k, n, _)| k == kind && n == name)
            .and_then(|(_, _, d)| d.clone())
            .unwrap_or_else(|| panic!("{kind} {name}: {got:?}"))
    };
    assert!(
        got.iter().any(|(k, _, d)| k == "sequence" && d.is_none()),
        "{got:?}"
    );
    let overloads: Vec<&String> = got
        .iter()
        .filter(|(_, n, _)| n == "add_tax")
        .filter_map(|(_, _, d)| d.as_ref())
        .collect();
    assert_eq!(overloads.len(), 3);
    assert!(
        overloads[0].contains(&format!("FUNCTION {s}.add_tax()")),
        "{overloads:?}"
    );
    assert!(
        overloads[1].contains("add_tax(amount numeric)\n"),
        "{overloads:?}"
    );
    assert!(
        overloads[2].contains("add_tax(amount numeric, rate numeric)"),
        "{overloads:?}"
    );
    assert!(text("procedure", "restock").starts_with(&format!(
        "CREATE OR REPLACE PROCEDURE {s}.restock(IN p_sku text, IN p_qty integer)"
    )));
    assert_eq!(
        text("aggregate", "norm"),
        format!(
            "CREATE AGGREGATE {s}.norm(numeric) (SFUNC = {s}.sum_sq_step, STYPE = numeric, \
             FINALFUNC = {s}.sum_sq_final, INITCOND = '0');"
        )
    );
    assert_eq!(
        text("aggregate", "plain_sum"),
        format!("CREATE AGGREGATE {s}.plain_sum(integer) (SFUNC = int4pl, STYPE = integer);")
    );
    assert_eq!(
        text("trigger", "stock_check"),
        format!(
            "CREATE TRIGGER stock_check BEFORE INSERT OR UPDATE ON {s}.stock FOR EACH ROW \
             EXECUTE FUNCTION {s}.stock_guard();"
        )
    );
    assert_eq!(
        text("type", "mood"),
        format!("CREATE TYPE {s}.mood AS ENUM ('sad', 'ok', 'it''s fine');")
    );
    // NOT NULL once, also on PostgreSQL 17 and later, which keep it as a constraint of the domain.
    assert_eq!(
        text("type", "positive_qty"),
        format!(
            "CREATE DOMAIN {s}.positive_qty AS integer DEFAULT 1 NOT NULL \
             CONSTRAINT qty_positive CHECK ((VALUE > 0));"
        )
    );
    assert_eq!(
        text("type", "price_range"),
        format!("CREATE TYPE {s}.price_range AS RANGE (SUBTYPE = numeric);")
    );
    assert_eq!(
        text("type", "address"),
        format!(
            "CREATE TYPE {s}.address AS (street text, zip character varying(10), \"House No\" integer);"
        )
    );
}

/// SPEC-0013 v7 AK-16 (live, `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB`; skipped without
/// them): functions, procedures and triggers as the server's `SHOW CREATE` states them.
#[tokio::test]
async fn mysql_object_definitions() {
    server_like_mysql_definitions("IDECRAFT_TEST_MYSQL").await;
}

#[tokio::test]
async fn mariadb_object_definitions() {
    server_like_mysql_definitions("IDECRAFT_TEST_MARIADB").await;
}

async fn server_like_mysql_definitions(var: &str) {
    let Ok(url) = std::env::var(var) else {
        return;
    };
    let scratch = format!("idecraft_def_{}", std::process::id());
    let my = connect(&url).await;
    run(
        &my,
        &format!(
            "drop database if exists {scratch}; create database {scratch}; {}",
            mysql_objects(&scratch)
        ),
    )
    .await;
    let mut got = Vec::new();
    for o in schema_objects(&my, Some(&scratch)).await.unwrap() {
        let ddl = object_definition(&my, Some(&scratch), &o).await.unwrap();
        let run = my
            .run_script(
                &format!("show create {} {scratch}.{}", o.kind, o.name),
                OnError::Stop,
            )
            .await;
        let own = match run.outcomes.into_iter().next().map(|x| x.outcome) {
            Some(Outcome::ResultSet { rows, .. }) => rows[0][2].clone(),
            other => panic!("{other:?}"),
        };
        got.push((o.kind, o.name, ddl, own.map(|t| t.trim_end().to_owned())));
    }
    run(&my, &format!("drop database {scratch}")).await;
    assert_eq!(got.len(), 4, "{got:?}");
    for (kind, name, ours, theirs) in &got {
        assert_eq!(ours, theirs, "{kind} {name}");
        // MySQL rewrites the head (`TRIGGER `name``); MariaDB keeps a trigger as it was written,
        // keywords in their case and the name qualified by the database.
        let text = ours.as_deref().unwrap_or_default();
        let upper = text.to_uppercase();
        assert!(
            upper.starts_with("CREATE ")
                && (upper.contains(&format!(
                    "{} `{}`",
                    kind.to_uppercase(),
                    name.to_uppercase()
                )) || upper.contains(&format!(
                    "{} {}.{}",
                    kind.to_uppercase(),
                    scratch.to_uppercase(),
                    name.to_uppercase()
                ))),
            "{text}"
        );
    }
}

/// The foreign keys of `table` as `(columns, on delete, on update)`, sorted by columns.
fn actions(
    tables: &[idecraft_database::Table],
    table: &str,
) -> Vec<(String, Option<String>, Option<String>)> {
    let t = tables.iter().find(|t| t.name == table).unwrap();
    let mut out: Vec<_> = t
        .foreign_keys
        .iter()
        .map(|f| {
            (
                f.columns.join(","),
                f.on_delete.clone(),
                f.on_update.clone(),
            )
        })
        .collect();
    out.sort();
    out
}

fn some(text: &str) -> Option<String> {
    Some(text.to_owned())
}

/// SPEC-0013 v8 AK-18: the actions of foreign keys on SQLite, from `foreign_key_list`.
#[tokio::test]
async fn sqlite_foreign_key_actions() {
    let dir = std::env::temp_dir().join(format!("idecraft-fk-actions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.db");
    rusqlite::Connection::open(&file).unwrap();
    let s = connect(&format!("sqlite:{}", file.display())).await;
    run(
        &s,
        "create table parent (id integer primary key);
         create table child (id integer primary key,
           a integer references parent(id) on delete cascade on update set null,
           b integer references parent(id) on delete restrict,
           c integer references parent(id))",
    )
    .await;
    let tables = introspect(&s).await.unwrap();
    assert_eq!(
        actions(&tables, "child"),
        [
            ("a".into(), some("CASCADE"), some("SET NULL")),
            ("b".into(), some("RESTRICT"), None),
            ("c".into(), None, None),
        ]
    );
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0013 v8 AK-18, AK-19: live on PostgreSQL in a throwaway schema, the actions of foreign
/// keys, the condition of a partial index (no key of the table) and a GIN index's method, kept by
/// Show DDL; skipped without `IDECRAFT_TEST_PG`.
#[tokio::test]
async fn postgres_foreign_key_actions_and_index_details() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let scratch = format!("idecraft_fk_{}", std::process::id());
    let pg = connect(&url).await;
    run(
        &pg,
        &format!(
            "drop schema if exists {scratch} cascade; create schema {scratch};
             create table {scratch}.parent (id int primary key);
             create table {scratch}.child (id int primary key,
               a int references {scratch}.parent(id) on delete cascade on update set null,
               b int references {scratch}.parent(id) on delete restrict,
               c int references {scratch}.parent(id) on update set default,
               d int references {scratch}.parent(id),
               qty int not null default 0, deleted boolean not null default false, tags jsonb,
               note text);
             create unique index child_live on {scratch}.child (qty) where not deleted;
             create index child_tags on {scratch}.child using gin (tags);
             create index child_note on {scratch}.child (note) include (qty)"
        ),
    )
    .await;
    let tables = introspect_schema(&pg, Some(&scratch)).await;
    let ddl = show_ddl_in(&pg, Some(&scratch), "child").await;
    run(&pg, &format!("drop schema {scratch} cascade")).await;
    let tables = tables.unwrap();
    assert_eq!(
        actions(&tables, "child"),
        [
            ("a".into(), some("CASCADE"), some("SET NULL")),
            ("b".into(), some("RESTRICT"), None),
            ("c".into(), None, some("SET DEFAULT")),
            ("d".into(), None, None),
        ]
    );
    let child = tables.iter().find(|t| t.name == "child").unwrap();
    let index = |name: &str| child.indexes.iter().find(|i| i.name == name).unwrap();
    assert_eq!(
        index("child_live").condition.as_deref(),
        Some("NOT deleted")
    );
    assert!(!index("child_live").is_key());
    assert_eq!(index("child_tags").method.as_deref(), Some("gin"));
    assert_eq!(index("child_pkey").method, None);
    assert_eq!(index("child_pkey").condition, None);
    // Columns of an INCLUDE clause are no key terms.
    assert_eq!(index("child_note").columns, ["note"]);
    let ddl = ddl.unwrap().unwrap().ddl;
    assert!(
        ddl.contains("REFERENCES \"parent\" (\"id\") ON DELETE CASCADE ON UPDATE SET NULL"),
        "{ddl}"
    );
    assert!(
        ddl.contains(
            "CREATE UNIQUE INDEX \"child_live\" ON \"child\" (\"qty\") WHERE NOT deleted;"
        ),
        "{ddl}"
    );
    assert!(
        ddl.contains("CREATE INDEX \"child_tags\" ON \"child\" USING gin (\"tags\");"),
        "{ddl}"
    );
}

/// SPEC-0013 v8 AK-18, AK-20: live on MySQL, the actions of foreign keys and the check
/// constraints with their definitions, kept by Show DDL; skipped without `IDECRAFT_TEST_MYSQL`.
#[tokio::test]
async fn mysql_foreign_key_actions_and_checks() {
    server_like_mysql_checks("IDECRAFT_TEST_MYSQL").await;
}

/// The same on MariaDB; skipped without `IDECRAFT_TEST_MARIADB`.
#[tokio::test]
async fn mariadb_foreign_key_actions_and_checks() {
    server_like_mysql_checks("IDECRAFT_TEST_MARIADB").await;
}

async fn server_like_mysql_checks(var: &str) {
    let Ok(url) = std::env::var(var) else {
        return;
    };
    let mariadb = var.contains("MARIADB");
    let scratch = format!("idecraft_fk_{}", std::process::id());
    let my = connect(&url).await;
    run(
        &my,
        &format!(
            "drop database if exists {scratch}; create database {scratch};
             create table {scratch}.parent (id int primary key);
             create table {scratch}.child (id int primary key, a int, b int, c int, qty int not null,
               constraint child_a_fk foreign key (a) references {scratch}.parent(id)
                 on delete cascade on update set null,
               constraint child_b_fk foreign key (b) references {scratch}.parent(id)
                 on delete no action on update no action,
               constraint child_c_fk foreign key (c) references {scratch}.parent(id)
                 on delete cascade,
               constraint child_qty_ck check (qty >= 0),
               constraint child_pair_ck check ((qty < 1000) or (id > 0)))"
        ),
    )
    .await;
    let tables = introspect_schema(&my, Some(&scratch)).await;
    let ddl = show_ddl_in(&my, Some(&scratch), "child").await;
    run(&my, &format!("drop database {scratch}")).await;
    let tables = tables.unwrap();
    assert_eq!(
        actions(&tables, "child"),
        [
            ("a".into(), some("CASCADE"), some("SET NULL")),
            // `NO ACTION` written out is the default on both systems.
            ("b".into(), None, None),
            // An action left out: MySQL states `NO ACTION`, MariaDB `RESTRICT`.
            (
                "c".into(),
                some("CASCADE"),
                mariadb.then(|| "RESTRICT".to_owned())
            ),
        ]
    );
    let child = tables.iter().find(|t| t.name == "child").unwrap();
    let checks: Vec<(&str, &str)> = child
        .constraints
        .iter()
        .map(|c| (c.name.as_str(), c.kind.as_str()))
        .collect();
    assert_eq!(
        checks,
        [("child_pair_ck", "check"), ("child_qty_ck", "check")]
    );
    let qty = &child.constraints[1].definition;
    assert!(
        qty.starts_with("CHECK (") && qty.contains("qty") && qty.contains(">= 0"),
        "{qty}"
    );
    let pair = &child.constraints[0].definition;
    assert!(pair.starts_with("CHECK (") && pair.ends_with(')'), "{pair}");
    let ddl = ddl.unwrap().unwrap().ddl;
    assert!(
        ddl.contains("ON DELETE CASCADE ON UPDATE SET NULL"),
        "{ddl}"
    );
    assert!(ddl.contains("CONSTRAINT `child_qty_ck` CHECK ("), "{ddl}");
}

/// Runs `sql` and gives the first error instead of failing, so a test drops its scratch schema
/// before it checks anything.
async fn attempt(s: &Session, sql: &str) -> Result<(), String> {
    let run = s.run_script(sql, OnError::Stop).await;
    for o in &run.outcomes {
        if let Outcome::Error(e) = &o.outcome {
            return Err(format!("{e} in\n{sql}"));
        }
    }
    Ok(())
}

fn table<'a>(tables: &'a [idecraft_database::Table], name: &str) -> &'a idecraft_database::Table {
    tables
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no table {name} in {tables:#?}"))
}

/// SPEC-0013 v9 AK-23, AK-25, AK-26 (OBS-0076 D4, D6–D8): live on PostgreSQL in throwaway schemas,
/// the sort order and collation of index terms, identity and generated columns and comments are
/// read and kept by Show DDL: its script run in a second schema reads the same; a schema that does
/// not exist is named as such. Skipped without `IDECRAFT_TEST_PG`.
#[tokio::test]
async fn postgres_index_orders_identity_generated_and_comments() {
    let Ok(url) = std::env::var("IDECRAFT_TEST_PG") else {
        return;
    };
    let scratch = format!("idecraft_d4_{}", std::process::id());
    let again = format!("{scratch}_b");
    let pg = connect(&url).await;
    let made = attempt(
        &pg,
        &format!(
            "drop schema if exists {scratch} cascade; drop schema if exists {again} cascade;
             create schema {scratch}; create schema {again};
             create table {scratch}.child (id int generated always as identity primary key,
               seq int generated by default as identity, qty int not null default 0, body text,
               name text, twice int generated always as (qty * 2) stored);
             comment on table {scratch}.child is 'the child''s note';
             comment on column {scratch}.child.qty is 'how many';
             create index child_d on {scratch}.child
               (qty desc, body collate \"POSIX\", name nulls first, lower(name) desc nulls last)"
        ),
    )
    .await;
    let tables = introspect_schema(&pg, Some(&scratch)).await;
    let ddl = show_ddl_in(&pg, Some(&scratch), "child").await;
    // The script of Show DDL, run in the second schema.
    let replayed = match &ddl {
        Ok(Some(object)) => {
            attempt(&pg, &format!("set search_path to {again};\n{}", object.ddl)).await
        }
        other => Err(format!("{other:?}")),
    };
    let _ = attempt(&pg, "reset search_path").await;
    let replayed_tables = introspect_schema(&pg, Some(&again)).await;
    let _ = attempt(&pg, &format!("drop table {again}.child")).await;
    let empty_schema = introspect_schema(&pg, Some(&again)).await;
    let missing = introspect_schema(&pg, Some("idecraft_no_such_schema")).await;
    let missing_ddl = show_ddl_in(&pg, Some("idecraft_no_such_schema"), "child").await;
    run(
        &pg,
        &format!("drop schema {scratch} cascade; drop schema {again} cascade"),
    )
    .await;
    made.unwrap();
    let tables = tables.unwrap();
    let child = table(&tables, "child");
    let index = child.indexes.iter().find(|i| i.name == "child_d").unwrap();
    assert_eq!(
        index.orders,
        [
            some("DESC"),
            None,
            some("NULLS FIRST"),
            some("DESC NULLS LAST")
        ]
    );
    assert_eq!(index.collations, [None, some("POSIX"), None, None]);
    assert!(index.prefixes.is_empty());
    let primary = child
        .indexes
        .iter()
        .find(|i| i.name == "child_pkey")
        .unwrap();
    assert!(primary.orders.is_empty() && primary.collations.is_empty());
    let column = |name: &str| child.columns.iter().find(|c| c.name == name).unwrap();
    assert_eq!(
        column("id").identity.as_deref(),
        Some("GENERATED ALWAYS AS IDENTITY")
    );
    assert_eq!(
        column("seq").identity.as_deref(),
        Some("GENERATED BY DEFAULT AS IDENTITY")
    );
    assert_eq!(column("qty").identity, None);
    assert_eq!(column("qty").default.as_deref(), Some("0"));
    assert_eq!(column("qty").comment.as_deref(), Some("how many"));
    let twice = column("twice");
    assert_eq!(twice.generated.as_deref(), Some("stored"));
    assert_eq!(twice.expression.as_deref(), Some("(qty * 2)"));
    assert_eq!(twice.default, None);
    assert_eq!(child.comment.as_deref(), Some("the child's note"));
    let ddl = ddl.unwrap().unwrap().ddl;
    for part in [
        "\"qty\" DESC, \"body\" COLLATE \"POSIX\", \"name\" NULLS FIRST, (lower(name)) DESC NULLS LAST",
        "\"id\" integer NOT NULL GENERATED ALWAYS AS IDENTITY",
        "\"seq\" integer NOT NULL GENERATED BY DEFAULT AS IDENTITY",
        "\"twice\" integer GENERATED ALWAYS AS (qty * 2) STORED",
        "COMMENT ON TABLE \"child\" IS 'the child''s note';",
        "COMMENT ON COLUMN \"child\".\"qty\" IS 'how many';",
    ] {
        assert!(ddl.contains(part), "{part} missing in\n{ddl}");
    }
    replayed.unwrap();
    let replayed_tables = replayed_tables.unwrap();
    assert_eq!(table(&replayed_tables, "child"), child, "{ddl}");
    // An existing schema without tables is no error, one that does not exist is named.
    assert_eq!(empty_schema.unwrap(), []);
    assert_eq!(
        missing.unwrap_err().message,
        "schema `idecraft_no_such_schema` does not exist"
    );
    assert_eq!(
        missing_ddl.unwrap_err().message,
        "schema `idecraft_no_such_schema` does not exist"
    );
}

/// SPEC-0013 v9 AK-23, AK-25, AK-26 (OBS-0076 D4–D8): live on MySQL in throwaway databases, the
/// sort order and prefix length of index terms, `AUTO_INCREMENT`, generated columns and comments
/// are read and kept by Show DDL: its script run in a second database reads the same; a database
/// that does not exist is named as such. Skipped without `IDECRAFT_TEST_MYSQL`.
#[tokio::test]
async fn mysql_index_details_identity_generated_and_comments() {
    server_like_mysql_details("IDECRAFT_TEST_MYSQL").await;
}

/// The same on MariaDB; skipped without `IDECRAFT_TEST_MARIADB`.
#[tokio::test]
async fn mariadb_index_details_identity_generated_and_comments() {
    server_like_mysql_details("IDECRAFT_TEST_MARIADB").await;
}

async fn server_like_mysql_details(var: &str) {
    let Ok(url) = std::env::var(var) else {
        return;
    };
    let scratch = format!("idecraft_d4_{}", std::process::id());
    let again = format!("{scratch}_b");
    let my = connect(&url).await;
    let made = attempt(
        &my,
        &format!(
            "drop database if exists {scratch}; drop database if exists {again};
             create database {scratch}; create database {again};
             create table {scratch}.child (id int not null auto_increment primary key
                 comment 'the child''s key',
               qty int not null default 0, body text, name varchar(40),
               twice int as (qty * 2) stored, more int generated always as (qty + 1) virtual,
               key child_d (qty desc, body(20), name(10)),
               unique key child_body (body(30)))
               comment = 'the child''s note'"
        ),
    )
    .await;
    let tables = introspect_schema(&my, Some(&scratch)).await;
    let ddl = show_ddl_in(&my, Some(&scratch), "child").await;
    // The script of Show DDL, run in the second database.
    let replayed = match &ddl {
        Ok(Some(object)) => attempt(&my, &format!("use {again};\n{}", object.ddl)).await,
        other => Err(format!("{other:?}")),
    };
    let replayed_tables = introspect_schema(&my, Some(&again)).await;
    let _ = attempt(&my, &format!("drop table {again}.child")).await;
    let empty_schema = introspect_schema(&my, Some(&again)).await;
    let missing = introspect_schema(&my, Some("idecraft_no_such_schema")).await;
    let missing_ddl = show_ddl_in(&my, Some("idecraft_no_such_schema"), "child").await;
    run(
        &my,
        &format!("drop database {scratch}; drop database {again}"),
    )
    .await;
    made.unwrap();
    let tables = tables.unwrap();
    let child = table(&tables, "child");
    let index = |name: &str| child.indexes.iter().find(|i| i.name == name).unwrap();
    assert_eq!(index("child_d").orders, [some("DESC"), None, None]);
    assert_eq!(index("child_d").prefixes, [None, Some(20), Some(10)]);
    assert!(index("child_d").collations.is_empty());
    assert_eq!(index("child_body").prefixes, [Some(30)]);
    assert!(index("PRIMARY").orders.is_empty() && index("PRIMARY").prefixes.is_empty());
    let column = |name: &str| child.columns.iter().find(|c| c.name == name).unwrap();
    assert_eq!(column("id").identity.as_deref(), Some("AUTO_INCREMENT"));
    assert_eq!(column("id").comment.as_deref(), Some("the child's key"));
    assert_eq!(column("qty").identity, None);
    assert_eq!(column("qty").comment, None);
    let twice = column("twice");
    assert_eq!(twice.generated.as_deref(), Some("stored"));
    assert!(
        twice
            .expression
            .as_deref()
            .is_some_and(|e| e.contains("qty") && e.contains("* 2")),
        "{twice:?}"
    );
    assert_eq!(column("more").generated.as_deref(), Some("virtual"));
    assert_eq!(child.comment.as_deref(), Some("the child's note"));
    let ddl = ddl.unwrap().unwrap().ddl;
    for part in [
        "(`qty` DESC, `body`(20), `name`(10));",
        "CREATE UNIQUE INDEX `child_body` ON `child` (`body`(30));",
        "NOT NULL AUTO_INCREMENT COMMENT 'the child''s key',",
        "GENERATED ALWAYS AS (",
        ") STORED,",
        ") VIRTUAL",
        ") COMMENT='the child''s note';",
    ] {
        assert!(ddl.contains(part), "{part} missing in\n{ddl}");
    }
    replayed.unwrap();
    let replayed_tables = replayed_tables.unwrap();
    assert_eq!(table(&replayed_tables, "child"), child, "{ddl}");
    assert_eq!(empty_schema.unwrap(), []);
    assert_eq!(
        missing.unwrap_err().message,
        "schema `idecraft_no_such_schema` does not exist"
    );
    assert_eq!(
        missing_ddl.unwrap_err().message,
        "schema `idecraft_no_such_schema` does not exist"
    );
}

/// SPEC-0013 v9 AK-26 (OBS-0076 D8): against SQLite files built in the test, an attached database
/// that does not exist is named as such, for the tables and for Show DDL; the main database and
/// an attached one without tables are no error.
#[tokio::test]
async fn an_unknown_schema_is_named() {
    let dir = std::env::temp_dir().join(format!("idecraft-no-schema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.db");
    rusqlite::Connection::open(&file).unwrap();
    rusqlite::Connection::open(dir.join("spare.db")).unwrap();
    let s = connect(&format!("sqlite:{}", file.display())).await;
    run(
        &s,
        &format!(
            "create table item (id integer primary key); attach database '{}' as spare",
            dir.join("spare.db").display()
        ),
    )
    .await;
    assert_eq!(introspect_schema(&s, Some("main")).await.unwrap().len(), 1);
    assert_eq!(introspect_schema(&s, Some("spare")).await.unwrap(), []);
    assert_eq!(
        introspect_schema(&s, Some("nope"))
            .await
            .unwrap_err()
            .message,
        "schema `nope` does not exist"
    );
    assert_eq!(
        show_ddl_in(&s, Some("nope"), "item")
            .await
            .unwrap_err()
            .message,
        "schema `nope` does not exist"
    );
    assert_eq!(show_ddl_in(&s, Some("spare"), "item").await.unwrap(), None);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
