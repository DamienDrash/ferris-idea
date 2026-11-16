//! SPEC-0089 v3 (schemas), SPEC-0014 v2 (fingerprint) and SPEC-0192 (default schema, row counts)
//! against SQLite (always) and PostgreSQL/MySQL/MariaDB when `IDECRAFT_TEST_PG` /
//! `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB` are set. Work happens in a scratch schema that
//! is dropped afterwards.

use crate::common;

use idecraft_database::{
    ConnectionProfile, OnError, Outcome, Session, complete, count_rows, current_schema, introspect,
    introspect_schema, list_schemas, object_definition, schema_fingerprint, schema_objects,
    sequences,
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
