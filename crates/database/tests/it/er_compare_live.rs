//! Live checks against the reference servers (`IDECRAFT_TEST_PG`, `IDECRAFT_TEST_MYSQL`,
//! `IDECRAFT_TEST_MARIADB`; without them nothing is done). Changes happen only in throwaway
//! schemas (PostgreSQL) or databases (MySQL, MariaDB) that are dropped afterwards.
//! - SPEC-0013/AK-6, SPEC-0022/AK-10: view definitions and what `stock_value` uses; constraint names.
//! - SPEC-0018/AK-6: a target with a view on tables the source lacks migrates without error.
//! - SPEC-0019/AK-9: a column and its check constraint renamed on PostgreSQL.
//! - SPEC-0013/AK-13, SPEC-0018/AK-15 (v5, v6; OBS-0068 L5–L7): constraints and definitions
//!   relative to their schema; the comparison of OBS-0068 migrates both ways on PostgreSQL.
//! - SPEC-0018/AK-17 (v7, IMP-0415): the script pins its target schema whatever schema the session
//!   is in; on PostgreSQL only for its transaction.

use crate::common;

use idecraft_database::{
    ConnectionProfile, DdlStatement, Dialect, OnError, Outcome, Session, Table, ViewDef,
    compare_between, compare_with_views, constraint_names, derived_renames, er_model_with_views,
    introspect, introspect_schema, migration_between, migration_script, migration_script_in,
    migration_with_views, rename, view_definitions, view_definitions_in,
};

async fn session(var: &str) -> Option<Session> {
    let url = std::env::var(var).ok()?;
    let profile = ConnectionProfile::from_url("reference", &url).unwrap();
    Some(
        Session::connect(&profile, &common::credentials(&profile))
            .await
            .unwrap(),
    )
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

/// The reference schema's view and its dependencies.
async fn reference_view(var: &str) {
    let Some(s) = session(var).await else {
        return;
    };
    let tables = introspect(&s).await.unwrap();
    let views = view_definitions(&s).await.unwrap();
    let v = views
        .iter()
        .find(|v| v.name == "stock_value")
        .expect("stock_value");
    assert!(v.definition.contains("stock") && v.definition.contains("item"));
    assert!(
        !v.definition.contains("`inventory`."),
        "no database qualifier"
    );
    let model = er_model_with_views(&tables, &views, None, 0).unwrap();
    let deps: Vec<(&str, &str)> = model
        .dependencies
        .iter()
        .map(|d| (d.view.as_str(), d.uses.as_str()))
        .collect();
    assert!(deps.contains(&("stock_value", "stock")), "{deps:?}");
    assert!(deps.contains(&("stock_value", "item")), "{deps:?}");
    let constraints = constraint_names(&s).await.unwrap();
    assert!(
        constraints
            .iter()
            .any(|c| c.table == "item" && c.kind == "check"),
        "{constraints:?}"
    );
    // MySQL ignores the column-level REFERENCES of the reference schema (SPEC-0013 notes).
    if s.dialect() == Dialect::PostgreSql {
        assert!(
            constraints
                .iter()
                .any(|c| c.table == "stock" && c.kind == "foreign key"),
            "{constraints:?}"
        );
        assert!(
            constraints
                .iter()
                .any(|c| c.name == "item_unit_price_cents_check" && c.kind == "check"),
            "{constraints:?}"
        );
    }
}

#[tokio::test]
async fn reference_views_and_constraints() {
    reference_view("IDECRAFT_TEST_PG").await;
    reference_view("IDECRAFT_TEST_MYSQL").await;
    reference_view("IDECRAFT_TEST_MARIADB").await;
}

/// Opens two throwaway namespaces named after `label` on the server of `var`: (source, target,
/// cleanup statements).
async fn namespaces(var: &str, label: &str) -> Option<(Session, Session, Vec<String>)> {
    let src = session(var).await?;
    let tgt = session(var).await?;
    let tag = std::process::id();
    let (a, b) = (
        format!("m57_{label}_src_{tag}"),
        format!("m57_{label}_tgt_{tag}"),
    );
    let pg = src.dialect() == Dialect::PostgreSql;
    let mut cleanup = Vec::new();
    for (s, name) in [(&src, &a), (&tgt, &b)] {
        if pg {
            let _ = s
                .run_script(
                    &format!("drop schema if exists {name} cascade"),
                    OnError::Continue,
                )
                .await;
            ok(
                s,
                &format!("create schema {name}; set search_path to {name}"),
            )
            .await;
            cleanup.push(format!("drop schema if exists {name} cascade"));
        } else {
            let _ = s
                .run_script(
                    &format!("drop database if exists {name}"),
                    OnError::Continue,
                )
                .await;
            ok(s, &format!("create database {name}; use {name}")).await;
            cleanup.push(format!("drop database if exists {name}"));
        }
    }
    Some((src, tgt, cleanup))
}

async fn migrate_with_views(var: &str) {
    let Some((src, tgt, cleanup)) = namespaces(var, "mig").await else {
        return;
    };
    ok(
        &src,
        "create table w (id integer primary key, city varchar(40) not null)",
    )
    .await;
    ok(
        &tgt,
        "create table w (id integer primary key, city varchar(20)); \
         create table t (id integer primary key, w_id integer); \
         insert into w values (1, 'x'); insert into t values (1, 1); \
         create view v as select t.id, w.city from t join w on w.id = t.w_id",
    )
    .await;
    let d = src.dialect();
    let read = async |s: &Session| {
        (
            introspect(s).await.unwrap(),
            view_definitions(s).await.unwrap(),
        )
    };
    let (st, sv) = read(&src).await;
    let (tt, tv) = read(&tgt).await;
    let statements = migration_with_views(&st, &tt, &sv, &tv, d);
    let sql: Vec<&str> = statements.iter().map(|s| s.sql.as_str()).collect();
    assert!(sql[0].starts_with("DROP VIEW"), "{sql:?}");
    assert!(
        sql.iter().position(|s| s.starts_with("DROP TABLE")) > Some(0),
        "{sql:?}"
    );
    assert!(
        statements
            .iter()
            .filter(|s| s.sql.starts_with("DROP"))
            .all(|s| !s.selected_by_default())
    );
    // Without the destructive statements the script runs and the table with its rows stays.
    let defaults: Vec<bool> = statements.iter().map(|s| s.selected_by_default()).collect();
    ok(&tgt, &migration_script(&statements, &defaults, d)).await;
    let (tt, tv) = read(&tgt).await;
    assert!(tt.iter().any(|t| t.name == "t"));
    // With them, nothing differs afterwards.
    let statements = migration_with_views(&st, &tt, &sv, &tv, d);
    let all = vec![true; statements.len()];
    ok(&tgt, &migration_script(&statements, &all, d)).await;
    let (tt, tv) = read(&tgt).await;
    let diff = compare_with_views(&st, &tt, &sv, &tv, d);
    assert!(diff.is_empty(), "{diff:?}");
    for c in cleanup {
        ok(&src, &c).await;
    }
}

/// Hand test 121 retest (SPEC-0018 v3, v5): `compare_b` has `item.discontinued`, a table `extra`
/// and a view on it; `stock_value` is the same on both sides and uses neither, so neither direction
/// drops and creates it again, and each script leaves no differences.
async fn unchanged_view_stays(var: &str) {
    let Some((src, tgt, cleanup)) = namespaces(var, "ht121").await else {
        return;
    };
    let inventory = "create table item (id integer primary key, sku varchar(20) not null, \
         unit_price_cents integer not null); \
         create table movement (id integer primary key, item_id integer not null, \
         quantity integer not null); \
         create view stock_value as select m.item_id, i.sku, m.quantity * i.unit_price_cents \
         as value_cents from movement m join item i on i.id = m.item_id";
    ok(&src, inventory).await;
    ok(
        &tgt,
        &format!(
            "{inventory}; alter table item add column discontinued integer; \
             create table extra (id integer primary key); \
             create view extra_v as select id from extra"
        ),
    )
    .await;
    let d = src.dialect();
    let read = async |s: &Session| {
        (
            introspect(s).await.unwrap(),
            view_definitions(s).await.unwrap(),
        )
    };
    for (from, to) in [(&src, &tgt), (&tgt, &src)] {
        let (st, sv) = read(from).await;
        let (tt, tv) = read(to).await;
        let statements = migration_with_views(&st, &tt, &sv, &tv, d);
        let sql: Vec<&str> = statements.iter().map(|s| s.sql.as_str()).collect();
        assert!(!sql.is_empty(), "{var}");
        assert!(
            !sql.iter().any(|s| s.contains("stock_value")),
            "{var}: the unchanged view stays: {sql:?}"
        );
    }
    // Both scripts run; afterwards nothing differs.
    for (from, to) in [(&src, &tgt), (&tgt, &src)] {
        let (st, sv) = read(from).await;
        let (tt, tv) = read(to).await;
        let statements = migration_with_views(&st, &tt, &sv, &tv, d);
        let all = vec![true; statements.len()];
        ok(to, &migration_script(&statements, &all, d)).await;
        let (tt, tv) = read(to).await;
        assert!(
            compare_with_views(&st, &tt, &sv, &tv, d).is_empty(),
            "{var}"
        );
    }
    for c in cleanup {
        ok(&src, &c).await;
    }
}

/// SPEC-0018 v9 AK-20 (OBS-0070 V5): PostgreSQL storage parameters reach the target. The source's
/// `pd_move` has `autovacuum_enabled` and the TOAST table's off, the target's has neither; the
/// target's `pd_note` has a fill factor the source's lacks; `pd_new` is only in the source.
#[tokio::test]
async fn storage_parameters_reach_the_target() {
    let Some((src, tgt, cleanup)) = namespaces("IDECRAFT_TEST_PG", "storage").await else {
        return;
    };
    ok(
        &src,
        "create table pd_move (id integer primary key, body text) \
         with (autovacuum_enabled = false, toast.autovacuum_enabled = false); \
         create table pd_note (id integer primary key); \
         create table pd_new (id integer primary key) with (fillfactor = 80)",
    )
    .await;
    ok(
        &tgt,
        "create table pd_move (id integer primary key, body text); \
         create table pd_note (id integer primary key) with (fillfactor = 70)",
    )
    .await;
    let d = Dialect::PostgreSql;
    let read = async |s: &Session| {
        (
            introspect(s).await.unwrap(),
            view_definitions(s).await.unwrap(),
        )
    };
    let (st, sv) = read(&src).await;
    let (tt, tv) = read(&tgt).await;
    let table = |tables: &[idecraft_database::Table], name: &str| {
        tables
            .iter()
            .find(|t| t.name == name)
            .unwrap()
            .storage
            .clone()
    };
    assert_eq!(
        table(&st, "pd_move"),
        ["autovacuum_enabled=false", "toast.autovacuum_enabled=false"]
    );
    assert_eq!(table(&tt, "pd_note"), ["fillfactor=70"]);
    let statements = migration_with_views(&st, &tt, &sv, &tv, d);
    let sql: Vec<&str> = statements.iter().map(|s| s.sql.as_str()).collect();
    assert!(
        sql.contains(
            &"ALTER TABLE \"pd_move\" SET (autovacuum_enabled=false, toast.autovacuum_enabled=false)"
        ),
        "{sql:?}"
    );
    assert!(
        sql.contains(&"ALTER TABLE \"pd_note\" RESET (fillfactor)"),
        "{sql:?}"
    );
    assert!(
        sql.iter().any(
            |s| s.starts_with("CREATE TABLE \"pd_new\"") && s.ends_with("WITH (fillfactor=80)")
        ),
        "{sql:?}"
    );
    let defaults: Vec<bool> = statements.iter().map(|s| s.selected_by_default()).collect();
    ok(&tgt, &migration_script(&statements, &defaults, d)).await;
    let (tt, tv) = read(&tgt).await;
    assert!(
        compare_with_views(&st, &tt, &sv, &tv, d).is_empty(),
        "{tt:?}"
    );
    assert_eq!(table(&tt, "pd_new"), ["fillfactor=80"]);
    for c in cleanup {
        ok(&src, &c).await;
    }
}

#[tokio::test]
async fn hand_test_121_unchanged_view_stays_on_the_servers() {
    unchanged_view_stays("IDECRAFT_TEST_PG").await;
    unchanged_view_stays("IDECRAFT_TEST_MYSQL").await;
    unchanged_view_stays("IDECRAFT_TEST_MARIADB").await;
}

#[tokio::test]
async fn migration_with_views_runs_on_the_servers() {
    migrate_with_views("IDECRAFT_TEST_PG").await;
    migrate_with_views("IDECRAFT_TEST_MYSQL").await;
    migrate_with_views("IDECRAFT_TEST_MARIADB").await;
}

#[tokio::test]
async fn postgres_renames_a_column_and_its_check_constraint() {
    let Some((s, _, cleanup)) = namespaces("IDECRAFT_TEST_PG", "ren").await else {
        return;
    };
    ok(
        &s,
        "create table item (sku text primary key, unit_price_cents integer not null check (unit_price_cents >= 0))",
    )
    .await;
    let tables = introspect(&s).await.unwrap();
    let constraints = constraint_names(&s).await.unwrap();
    let main = rename(
        "",
        Dialect::PostgreSql,
        &tables,
        "item",
        Some("unit_price_cents"),
        "price_cents",
    )
    .unwrap();
    let derived = derived_renames(
        Dialect::PostgreSql,
        &tables,
        &constraints,
        "item",
        Some("unit_price_cents"),
        "price_cents",
    );
    assert_eq!(derived.len(), 1, "{derived:?}");
    ok(&s, &main.ddl).await;
    ok(&s, derived[0].ddl.as_deref().unwrap()).await;
    let after = constraint_names(&s).await.unwrap();
    assert!(
        after.iter().any(|c| c.name == "item_price_cents_check"),
        "{after:?}"
    );
    let item = introspect(&s).await.unwrap();
    assert!(item[0].columns.iter().any(|c| c.name == "price_cents"));
    for c in cleanup {
        ok(&s, &c).await;
    }
}

/// The first value of the first row `sql` returns.
async fn value(s: &Session, sql: &str) -> String {
    let run = s.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { rows, .. }) => rows
            .first()
            .and_then(|r| r.first().cloned().flatten())
            .unwrap_or_default(),
        other => panic!("{sql}: {other:?}"),
    }
}

/// SPEC-0013 v5 AK-13 (OBS-0068 L5, L7): checks and unique constraints are read with their
/// definitions; views and defaults of a schema outside the search path are written relative to it.
#[tokio::test]
async fn postgres_reads_constraints_and_views_relative_to_their_schema() {
    let Some(s) = session("IDECRAFT_TEST_PG").await else {
        return;
    };
    let x = format!("m68_rel_{}", std::process::id());
    let _ = s
        .run_script(
            &format!("drop schema if exists {x} cascade"),
            OnError::Continue,
        )
        .await;
    let public_item = value(&s, "select to_regclass('public.item') is not null").await == "t";
    ok(
        &s,
        &format!(
            "create schema {x}; create sequence {x}.item_seq; \
             create table {x}.item (id integer not null default nextval('{x}.item_seq'), \
             sku text primary key, qty integer not null check (qty >= 0), \
             body text not null constraint {x}_body_uq unique); \
             create view {x}.own as select sku from {x}.item"
        ),
    )
    .await;
    if public_item {
        ok(
            &s,
            &format!("create view {x}.across as select i.sku from public.item i"),
        )
        .await;
    }
    let path = value(&s, "show search_path").await;
    let tables = introspect_schema(&s, Some(&x)).await.unwrap();
    let views = view_definitions_in(&s, Some(&x)).await.unwrap();
    assert_eq!(
        value(&s, "show search_path").await,
        path,
        "search path kept"
    );
    let item = tables.iter().find(|t| t.name == "item").unwrap();
    let kinds: Vec<(&str, &str)> = item
        .constraints
        .iter()
        .map(|c| (c.kind.as_str(), c.definition.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [("check", "CHECK (qty >= 0)"), ("unique", "UNIQUE (body)")],
        "{:?}",
        item.constraints
    );
    assert_eq!(item.constraints[1].columns, ["body"]);
    assert!(
        item.indexes
            .iter()
            .any(|i| i.name == format!("{x}_body_uq") && i.unique),
        "{:?}",
        item.indexes
    );
    let id = item.columns.iter().find(|c| c.name == "id").unwrap();
    assert_eq!(
        id.default.as_deref(),
        Some("nextval('item_seq'::regclass)"),
        "the own sequence without its schema"
    );
    let own = views.iter().find(|v| v.name == "own").unwrap();
    assert!(
        own.definition.contains("FROM item") && !own.definition.contains(&x),
        "{}",
        own.definition
    );
    if public_item {
        let across = views.iter().find(|v| v.name == "across").unwrap();
        assert!(
            across.definition.contains("FROM public.item"),
            "{}",
            across.definition
        );
    }
    ok(&s, &format!("drop schema {x} cascade")).await;
}

/// The schemas of OBS-0068 (`pd-setup.postgresql.sql`, shortened): `a` plays `public` with the
/// inventory tables, `pd_note` (check, unique constraint, index, a sequence default) and `pd_move`;
/// `b` plays `pd_b` with `warehouse`, `item` and its extra column and a view on its own `item`
/// written with the schema. The session works in `a`.
async fn obs_0068_schemas(s: &Session, a: &str, b: &str) {
    for name in [a, b] {
        let _ = s
            .run_script(
                &format!("drop schema if exists {name} cascade"),
                OnError::Continue,
            )
            .await;
    }
    ok(
        s,
        &format!(
            "create schema {a}; create schema {b}; set search_path to {a}; \
             create table warehouse (id integer primary key, city text not null); \
             create table item (sku text primary key, name text not null, \
               unit_price_cents integer not null check (unit_price_cents >= 0)); \
             create table stock (warehouse_id integer not null references warehouse (id), \
               sku text not null references item (sku), qty integer not null default 0, \
               primary key (warehouse_id, sku)); \
             create table movement (id integer primary key, \
               warehouse_id integer not null references warehouse (id), \
               sku text not null references item (sku), delta integer not null); \
             create view stock_value as select s.warehouse_id, sum(s.qty * i.unit_price_cents) \
               as value_cents from stock s join item i on i.sku = s.sku group by s.warehouse_id; \
             insert into warehouse values (1, 'Leipzig'); insert into item values ('A-1', 'Bolt', 25); \
             insert into stock values (1, 'A-1', 100); insert into movement values (1, 1, 'A-1', 5); \
             create table {b}.warehouse (like warehouse including all); \
             create table {b}.item (like item including all); \
             alter table {b}.item add column pd_discontinued boolean not null default false; \
             create view {b}.pd_item_names as select sku, name from {b}.item; \
             insert into {b}.item values ('Z-9', 'only in b', 1, false); \
             create sequence pd_seq; \
             create table pd_note (id integer primary key default nextval('pd_seq'), \
               body varchar(40) not null constraint pd_note_body_uq unique, \
               qty integer not null default 0 constraint pd_note_qty_check check (qty >= 0)); \
             create index pd_note_qty_idx on pd_note (qty); \
             create view pd_note_big as select id, body from pd_note where qty > 10; \
             insert into pd_note (body, qty) values ('first', 1), ('second', 12); \
             create table pd_move (id bigint primary key, sku text not null, delta integer not null)"
        ),
    )
    .await;
}

/// The tables and views of `schema` (the session's current one with `None`).
async fn schema_of(s: &Session, schema: Option<&str>) -> (Vec<Table>, Vec<ViewDef>) {
    (
        introspect_schema(s, schema).await.unwrap(),
        view_definitions_in(s, schema).await.unwrap(),
    )
}

/// The script for `selected` as the window writes it: pinned to the target schema `target`
/// (SPEC-0018 v7 behaviour 19), also when that is the session's current one.
fn script_in(statements: &[DdlStatement], selected: &[bool], target: &str) -> String {
    migration_script_in(statements, selected, Dialect::PostgreSql, Some(target))
}

/// SPEC-0018 v6 AK-15 (OBS-0068 L5, L6, L7): both directions of the comparison of OBS-0068 run,
/// keep constraints and foreign keys, write view bodies for the target and leave no differences.
#[tokio::test]
async fn obs_0068_schemas_migrate_both_ways() {
    let Some(s) = session("IDECRAFT_TEST_PG").await else {
        return;
    };
    let d = Dialect::PostgreSql;
    let tag = std::process::id();
    let (a, b) = (format!("m68_a_{tag}"), format!("m68_b_{tag}"));
    let defaults = |statements: &[DdlStatement]| -> Vec<bool> {
        statements.iter().map(|s| s.selected_by_default()).collect()
    };

    // From `a` (current) to `b` (C1, C3): pd_note keeps its check and unique constraint.
    obs_0068_schemas(&s, &a, &b).await;
    let schemas = Some((a.as_str(), b.as_str()));
    let (st, sv) = schema_of(&s, None).await;
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let statements = migration_between(&st, &tt, &sv, &tv, d, schemas);
    let create = statements
        .iter()
        .find(|s| s.sql.starts_with("CREATE TABLE \"pd_note\""))
        .expect("pd_note is created");
    assert!(
        create.sql.contains("CHECK (qty >= 0)") && create.sql.contains("UNIQUE (body)"),
        "{}",
        create.sql
    );
    assert!(
        !statements.iter().any(|s| s.sql.contains("UNIQUE INDEX")),
        "the unique constraint is no index"
    );
    ok(&s, &script_in(&statements, &defaults(&statements), &b)).await;
    ok(&s, &format!("set search_path to {a}")).await;
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let diff = compare_between(&st, &tt, &sv, &tv, d, schemas);
    assert!(
        diff.added.is_empty() && diff.views_added.is_empty() && diff.views_changed.is_empty(),
        "{diff:?}"
    );
    // Only what was left out differs: the view only in `b` and the extra column of `b.item`.
    assert_eq!(diff.views_removed, ["pd_item_names"], "{diff:?}");
    assert_eq!(diff.changed.len(), 1, "{diff:?}");
    assert_eq!(diff.changed[0].columns_removed[0].name, "pd_discontinued");
    let statements = migration_between(&st, &tt, &sv, &tv, d, schemas);
    ok(
        &s,
        &script_in(&statements, &vec![true; statements.len()], &b),
    )
    .await;
    ok(&s, &format!("set search_path to {a}")).await;
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let diff = compare_between(&st, &tt, &sv, &tv, d, schemas);
    assert!(diff.is_empty(), "a → b: {diff:?}");

    // From `b` to `a` (current; C2): the left-out drops keep every foreign key, the new view
    // reads `a.item`.
    obs_0068_schemas(&s, &a, &b).await;
    let schemas = Some((b.as_str(), a.as_str()));
    let (st, sv) = schema_of(&s, Some(&b)).await;
    let (tt, tv) = schema_of(&s, None).await;
    let statements = migration_between(&st, &tt, &sv, &tv, d, schemas);
    let script = script_in(&statements, &defaults(&statements), &a);
    for line in script.lines().filter(|l| l.contains("DROP CONSTRAINT")) {
        assert!(
            line.starts_with("-- "),
            "left out with its table: {line}\n{script}"
        );
    }
    ok(&s, &script).await;
    let (tt, tv) = schema_of(&s, None).await;
    for name in ["stock", "movement"] {
        let t = tt.iter().find(|t| t.name == name).unwrap();
        assert_eq!(t.foreign_keys.len(), 2, "{name} keeps its keys: {t:?}");
    }
    assert_eq!(
        value(&s, "select string_agg(sku, ',') from pd_item_names").await,
        "A-1"
    );
    let diff = compare_between(&st, &tt, &sv, &tv, d, schemas);
    assert_eq!(
        diff.removed,
        ["movement", "pd_move", "pd_note", "stock"],
        "{diff:?}"
    );
    assert_eq!(
        diff.views_removed,
        ["pd_note_big", "stock_value"],
        "{diff:?}"
    );
    assert!(
        diff.added.is_empty()
            && diff.changed.is_empty()
            && diff.views_added.is_empty()
            && diff.views_changed.is_empty(),
        "{diff:?}"
    );
    let statements = migration_between(&st, &tt, &sv, &tv, d, schemas);
    ok(
        &s,
        &script_in(&statements, &vec![true; statements.len()], &a),
    )
    .await;
    let (tt, tv) = schema_of(&s, None).await;
    let diff = compare_between(&st, &tt, &sv, &tv, d, schemas);
    assert!(diff.is_empty(), "b → a: {diff:?}");

    for name in [&a, &b] {
        ok(&s, &format!("drop schema {name} cascade")).await;
    }
}

/// The names of the relations of `schema` (tables, indexes, views), sorted and joined by `,`.
async fn relations_of(s: &Session, schema: &str) -> String {
    value(
        s,
        &format!(
            "select coalesce(string_agg(c.relname, ',' order by c.relname), '') from pg_class c \
             join pg_namespace n on n.oid = c.relnamespace where n.nspname = '{schema}'"
        ),
    )
    .await
}

/// The columns of `schema.table` in order, joined by `,` (`information_schema`, all dialects
/// tested here).
async fn columns_of(s: &Session, schema: &str, table: &str) -> String {
    let run = s
        .run_script(
            &format!(
                "select column_name from information_schema.columns where table_schema = '{schema}' \
                 and table_name = '{table}' order by ordinal_position"
            ),
            OnError::Stop,
        )
        .await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { rows, .. }) => rows
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect::<Vec<_>>()
            .join(","),
        other => panic!("columns of {schema}.{table}: {other:?}"),
    }
}

/// SPEC-0018 v7 AK-17 on PostgreSQL: the session works in a wrong schema that has a table of the
/// target's name, and a temporary table has that name too; the script still creates and changes
/// everything in the target, and the session's `search_path` is the same afterwards, also after a
/// script that fails.
async fn pins_on_postgres() {
    let Some(s) = session("IDECRAFT_TEST_PG").await else {
        return;
    };
    let d = Dialect::PostgreSql;
    let tag = std::process::id();
    let (a, b, w) = (
        format!("pin_src_{tag}"),
        format!("pin_tgt_{tag}"),
        format!("pin_wrong_{tag}"),
    );
    for name in [&a, &b, &w] {
        let _ = s
            .run_script(
                &format!("drop schema if exists {name} cascade"),
                OnError::Continue,
            )
            .await;
    }
    ok(
        &s,
        &format!(
            "create schema {a}; create schema {b}; create schema {w}; \
             create table {a}.item (sku text primary key, name text, \
               price_cents integer not null default 0 \
               constraint item_price_check check (price_cents >= 0)); \
             create table {a}.stock (sku text not null references {a}.item (sku), \
               qty integer not null, constraint stock_sku_uq unique (sku)); \
             create index stock_qty_idx on {a}.stock (qty); \
             create view {a}.item_names as select sku, name from {a}.item; \
             create table {b}.item (sku text primary key); insert into {b}.item values ('A-1'); \
             create table {w}.item (sku text primary key)"
        ),
    )
    .await;
    let schemas = Some((a.as_str(), b.as_str()));
    let (st, sv) = schema_of(&s, Some(&a)).await;
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let statements = migration_between(&st, &tt, &sv, &tv, d, schemas);
    let script = migration_script_in(&statements, &vec![true; statements.len()], d, Some(&b));
    assert!(
        script.contains(&format!(
            "\nBEGIN;\nSET LOCAL search_path TO \"{b}\", pg_temp;\n"
        )),
        "{script}"
    );

    // The console is elsewhere: in `w`, as an earlier script or `SET` could have left it, with a
    // temporary `item` that PostgreSQL would search first.
    ok(
        &s,
        &format!(
            "set search_path to {w}; create temp table item (sku text primary key, note text)"
        ),
    )
    .await;
    let before = value(&s, "show search_path").await;
    assert_eq!(before, w);
    ok(&s, &script).await;
    assert_eq!(
        value(&s, "show search_path").await,
        before,
        "the session's path is unchanged"
    );
    assert_eq!(
        relations_of(&s, &b).await,
        "item,item_names,item_pkey,stock,stock_qty_idx,stock_sku_uq",
        "every object is in the target"
    );
    assert_eq!(columns_of(&s, &b, "item").await, "sku,name,price_cents");
    let constraints = value(
        &s,
        &format!(
            "select string_agg(conname || ':' || contype::text, ',' order by conname) \
             from pg_constraint \
             where connamespace = '{b}'::regnamespace and contype in ('c', 'f', 'u')"
        ),
    )
    .await;
    assert!(
        constraints.contains("item_price_check:c")
            && constraints.contains("stock_sku_uq:u")
            && constraints.contains(":f"),
        "{constraints}"
    );
    assert_eq!(
        relations_of(&s, &w).await,
        "item,item_pkey",
        "nothing in the wrong schema"
    );
    assert_eq!(columns_of(&s, &w, "item").await, "sku");
    assert_eq!(
        value(
            &s,
            "select string_agg(attname, ',' order by attnum) from pg_attribute \
             where attrelid = 'pg_temp.item'::regclass and attnum > 0 and not attisdropped"
        )
        .await,
        "sku,note",
        "the temporary table is not changed in the target's place"
    );
    assert_eq!(
        value(&s, &format!("select count(*) from {b}.item_names")).await,
        "1",
        "the view reads the target's item"
    );
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let diff = compare_between(&st, &tt, &sv, &tv, d, schemas);
    assert!(diff.is_empty(), "{diff:?}");

    // A script that fails: after its ROLLBACK the path is the same and nothing was created.
    ok(
        &s,
        &format!(
            "create table {a}.extra (id integer); \
             alter table {a}.stock add constraint stock_qty_check check (qty > 0); \
             insert into {b}.stock values ('A-1', 0)"
        ),
    )
    .await;
    let (st, sv) = schema_of(&s, Some(&a)).await;
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let statements = migration_between(&st, &tt, &sv, &tv, d, schemas);
    let failing = migration_script_in(&statements, &vec![true; statements.len()], d, Some(&b));
    let run = s.run_script(&failing, OnError::Stop).await;
    assert!(
        run.outcomes
            .iter()
            .any(|o| matches!(o.outcome, Outcome::Error(_))),
        "the new check fails on the target's row: {failing}"
    );
    ok(&s, "rollback").await;
    assert_eq!(value(&s, "show search_path").await, before);
    assert_eq!(
        value(
            &s,
            &format!(
                "select to_regclass('{b}.extra') is null and to_regclass('{w}.extra') is null"
            )
        )
        .await,
        "t",
        "rolled back"
    );

    ok(&s, "drop table pg_temp.item").await;
    for name in [&a, &b, &w] {
        ok(&s, &format!("drop schema {name} cascade")).await;
    }
}

/// SPEC-0018 v7 AK-17 on MySQL and MariaDB: after `USE` of a wrong database the script creates
/// its objects in the target, which `USE` leaves the session's database (said in the script).
async fn pins_with_use(var: &str) {
    let Some(s) = session(var).await else {
        return;
    };
    let d = s.dialect();
    let tag = std::process::id();
    let (a, b, w) = (
        format!("pin_src_{tag}"),
        format!("pin_tgt_{tag}"),
        format!("pin_wrong_{tag}"),
    );
    for name in [&a, &b, &w] {
        let _ = s
            .run_script(
                &format!("drop database if exists {name}"),
                OnError::Continue,
            )
            .await;
    }
    ok(
        &s,
        &format!(
            "create database {a}; create database {b}; create database {w}; \
             create table {a}.item (sku varchar(20) primary key, name varchar(40)); \
             create table {a}.stock (sku varchar(20) not null, qty integer not null, \
               constraint stock_item_fk foreign key (sku) references {a}.item (sku)); \
             create index stock_qty_idx on {a}.stock (qty); \
             create view {a}.item_names as select sku, name from {a}.item; \
             create table {b}.item (sku varchar(20) primary key); \
             insert into {b}.item values ('A-1'); \
             create table {w}.item (sku varchar(20) primary key)"
        ),
    )
    .await;
    let schemas = Some((a.as_str(), b.as_str()));
    let (st, sv) = schema_of(&s, Some(&a)).await;
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let statements = migration_between(&st, &tt, &sv, &tv, d, schemas);
    let script = migration_script_in(&statements, &vec![true; statements.len()], d, Some(&b));
    assert!(script.contains(&format!("\nUSE `{b}`;\n")), "{script}");
    ok(&s, &format!("use {w}")).await;
    ok(&s, &script).await;
    assert_eq!(
        value(&s, "select database()").await,
        b,
        "USE stays for the session"
    );
    let tables = |schema: &str| {
        format!(
            "select group_concat(table_name order by table_name) from information_schema.tables \
             where table_schema = '{schema}'"
        )
    };
    assert_eq!(value(&s, &tables(&b)).await, "item,item_names,stock");
    assert_eq!(columns_of(&s, &b, "item").await, "sku,name");
    assert_eq!(
        value(&s, &tables(&w)).await,
        "item",
        "nothing in the wrong database"
    );
    assert_eq!(columns_of(&s, &w, "item").await, "sku");
    assert_eq!(
        value(&s, &format!("select count(*) from {b}.item_names")).await,
        "1",
        "the view reads the target's item"
    );
    let (tt, tv) = schema_of(&s, Some(&b)).await;
    let diff = compare_between(&st, &tt, &sv, &tv, d, schemas);
    assert!(diff.is_empty(), "{var}: {diff:?}");
    for name in [&a, &b, &w] {
        ok(&s, &format!("drop database {name}")).await;
    }
}

/// SPEC-0018 v7 AK-17 (behaviour 19, IMP-0415): the script pins its target schema whatever schema
/// the session is in.
#[tokio::test]
async fn the_script_pins_its_target_schema() {
    pins_on_postgres().await;
    pins_with_use("IDECRAFT_TEST_MYSQL").await;
    pins_with_use("IDECRAFT_TEST_MARIADB").await;
}
