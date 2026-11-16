//! Objects of a schema besides tables and views (`SPEC-0013` v6 behaviours 18 and 19): the kinds a
//! database has and its sequences, routines, triggers and types, read from the catalogue through
//! the ordinary session, and their definitions (`SPEC-0013` v7 behaviour 20). Nothing is changed.

use serde::Serialize;

use crate::introspect::{query, sequence_query};
use crate::{DbError, Dialect, Session};

/// One object of a schema that is neither a table nor a view.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SchemaObject {
    /// Its name.
    pub name: String,
    /// `sequence`, `function`, `procedure`, `aggregate`, `trigger` or `type`.
    pub kind: String,
    /// The table of a trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// What tells it apart: a routine's arguments (PostgreSQL, so overloads differ), when a
    /// trigger fires (`AFTER INSERT OR UPDATE`), what a type is (`enum`, `domain`, `range`,
    /// `composite`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The kinds of objects a schema of `dialect` can hold, tables and views first, in the order
/// [`schema_objects`] lists them.
pub fn object_kinds(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::PostgreSql => &[
            "table",
            "view",
            "sequence",
            "function",
            "procedure",
            "aggregate",
            "trigger",
            "type",
        ],
        Dialect::MySql => &["table", "view", "function", "procedure", "trigger"],
        Dialect::MariaDb => &[
            "table",
            "view",
            "sequence",
            "function",
            "procedure",
            "trigger",
        ],
        Dialect::Sqlite => &["table", "view", "trigger"],
        Dialect::DuckDb | Dialect::Oracle | Dialect::SqlServer => &["table", "view", "sequence"],
    }
}

/// What a catalogue query reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogPart {
    /// One name per row.
    Sequences,
    /// Name, kind, arguments.
    Routines,
    /// Name, table, timing and events (PostgreSQL, MySQL, MariaDB) or the statement (SQLite).
    Triggers,
    /// Name and what the type is.
    Types,
}

/// The catalogue queries of `schema` (the current one with `None`) in `dialect`, one per part
/// it has.
pub fn object_queries(dialect: Dialect, schema: Option<&str>) -> Vec<(CatalogPart, String)> {
    let lit = |name: &str| format!("'{}'", name.replace('\'', "''"));
    let mut out = Vec::new();
    if let Some(sql) = sequence_query(dialect, schema) {
        out.push((CatalogPart::Sequences, sql));
    }
    match dialect {
        Dialect::PostgreSql => {
            let s = schema.map_or_else(|| "current_schema()".into(), lit);
            // Members of extensions belong to the extension, not to the schema's own objects; the
            // constructors PostgreSQL makes itself for a range type (internal dependency) belong
            // to the type.
            out.push((
                CatalogPart::Routines,
                format!(
                    "select p.proname, case p.prokind when 'p' then 'procedure' \
                    when 'a' then 'aggregate' else 'function' end, \
                    pg_get_function_identity_arguments(p.oid) \
                    from pg_proc p join pg_namespace n on n.oid = p.pronamespace \
                    where n.nspname = {s} and not exists (select 1 from pg_depend d \
                    where d.classid = 'pg_proc'::regclass and d.objid = p.oid and d.deptype in ('e', 'i')) \
                    order by 1, 3"
                ),
            ));
            out.push((
                CatalogPart::Triggers,
                format!(
                    "select t.tgname, c.relname, \
                    case when t.tgtype::int & 2 = 2 then 'BEFORE' \
                    when t.tgtype::int & 64 = 64 then 'INSTEAD OF' else 'AFTER' end || ' ' || \
                    concat_ws(' OR ', case when t.tgtype::int & 4 = 4 then 'INSERT' end, \
                    case when t.tgtype::int & 16 = 16 then 'UPDATE' end, \
                    case when t.tgtype::int & 8 = 8 then 'DELETE' end, \
                    case when t.tgtype::int & 32 = 32 then 'TRUNCATE' end) \
                    from pg_trigger t join pg_class c on c.oid = t.tgrelid \
                    join pg_namespace n on n.oid = c.relnamespace \
                    where n.nspname = {s} and not t.tgisinternal order by 1, 2"
                ),
            ));
            out.push((
                CatalogPart::Types,
                format!(
                    "select t.typname, case t.typtype when 'e' then 'enum' when 'd' then 'domain' \
                    when 'r' then 'range' else 'composite' end \
                    from pg_type t join pg_namespace n on n.oid = t.typnamespace \
                    where n.nspname = {s} and (t.typtype in ('e', 'd', 'r') \
                    or (t.typtype = 'c' and exists (select 1 from pg_class c \
                    where c.oid = t.typrelid and c.relkind = 'c'))) \
                    and not exists (select 1 from pg_depend d \
                    where d.classid = 'pg_type'::regclass and d.objid = t.oid and d.deptype = 'e') \
                    order by 1"
                ),
            ));
        }
        Dialect::MySql | Dialect::MariaDb => {
            let s = schema.map_or_else(|| "database()".into(), lit);
            out.push((
                CatalogPart::Routines,
                format!(
                    "select ROUTINE_NAME, lower(ROUTINE_TYPE), null \
                    from information_schema.ROUTINES where ROUTINE_SCHEMA = {s} \
                    and ROUTINE_TYPE in ('FUNCTION', 'PROCEDURE') order by 1"
                ),
            ));
            out.push((
                CatalogPart::Triggers,
                format!(
                    "select TRIGGER_NAME, EVENT_OBJECT_TABLE, \
                    concat(ACTION_TIMING, ' ', EVENT_MANIPULATION) \
                    from information_schema.TRIGGERS where TRIGGER_SCHEMA = {s} order by 1"
                ),
            ));
        }
        Dialect::Sqlite => {
            let master = match schema {
                Some(name) => format!(
                    "{}.sqlite_master",
                    crate::quote_identifier(name, Dialect::Sqlite)
                ),
                None => "sqlite_master".into(),
            };
            out.push((
                CatalogPart::Triggers,
                format!(
                    "select name, tbl_name, sql from {master} where type = 'trigger' order by 1"
                ),
            ));
        }
        Dialect::DuckDb | Dialect::Oracle | Dialect::SqlServer => {}
    }
    out
}

fn cell(row: &[Option<String>], i: usize) -> Option<String> {
    row.get(i).cloned().flatten().filter(|v| !v.is_empty())
}

/// The objects in the rows of one catalogue query of `dialect`.
pub fn objects_from_rows(
    dialect: Dialect,
    part: CatalogPart,
    rows: &[Vec<Option<String>>],
) -> Vec<SchemaObject> {
    rows.iter()
        .filter_map(|r| {
            let name = cell(r, 0)?;
            Some(match part {
                CatalogPart::Sequences => SchemaObject {
                    name,
                    kind: "sequence".into(),
                    table: None,
                    detail: None,
                },
                CatalogPart::Routines => SchemaObject {
                    name,
                    kind: cell(r, 1).unwrap_or_else(|| "function".into()),
                    table: None,
                    // PostgreSQL gives the arguments, also when there are none: `()` tells the
                    // overloads apart.
                    detail: (dialect == Dialect::PostgreSql)
                        .then(|| format!("({})", cell(r, 2).unwrap_or_default())),
                },
                CatalogPart::Triggers => SchemaObject {
                    name,
                    kind: "trigger".into(),
                    table: cell(r, 1),
                    detail: if dialect == Dialect::Sqlite {
                        cell(r, 2).and_then(|sql| sqlite_trigger_event(&sql))
                    } else {
                        cell(r, 2)
                    },
                },
                CatalogPart::Types => SchemaObject {
                    name,
                    kind: "type".into(),
                    table: None,
                    detail: cell(r, 1),
                },
            })
        })
        .collect()
}

/// When a SQLite trigger fires, from its statement: `BEFORE INSERT`, `AFTER UPDATE OF qty`,
/// `INSTEAD OF DELETE`; SQLite's default timing is `BEFORE`.
pub fn sqlite_trigger_event(sql: &str) -> Option<String> {
    let words: Vec<&str> = sql.split_whitespace().collect();
    let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
    let event = lower
        .iter()
        .position(|w| matches!(w.as_str(), "insert" | "update" | "delete"))?;
    let timing = match lower.get(event.wrapping_sub(1)).map(String::as_str) {
        Some("after") => "AFTER",
        Some("of") if lower.get(event.wrapping_sub(2)).map(String::as_str) == Some("instead") => {
            "INSTEAD OF"
        }
        _ => "BEFORE",
    };
    let mut text = format!("{timing} {}", lower[event].to_ascii_uppercase());
    // `UPDATE OF a, b ON t`: the columns.
    if lower[event] == "update" && lower.get(event + 1).map(String::as_str) == Some("of") {
        let columns: Vec<&str> = words[event + 2..]
            .iter()
            .zip(&lower[event + 2..])
            .take_while(|(_, l)| l.as_str() != "on")
            .map(|(w, _)| *w)
            .collect();
        if !columns.is_empty() {
            text.push_str(" OF ");
            text.push_str(&columns.join(" "));
        }
    }
    Some(text)
}

/// The sequences, routines, triggers and types of `schema` (the current schema with `None`),
/// by kind in the order of [`object_kinds`], each kind sorted by name (`SPEC-0013` v6
/// behaviour 18). A database without a kind lists none of it; nothing is changed.
pub async fn schema_objects(
    session: &Session,
    schema: Option<&str>,
) -> Result<Vec<SchemaObject>, DbError> {
    let dialect = session.dialect();
    let mut objects = Vec::new();
    for (part, sql) in object_queries(dialect, schema) {
        let rows = query(session, &sql).await?;
        objects.extend(objects_from_rows(dialect, part, &rows));
    }
    let kinds = object_kinds(dialect);
    let rank = |k: &str| kinds.iter().position(|x| *x == k).unwrap_or(kinds.len());
    objects.sort_by(|a, b| {
        rank(&a.kind)
            .cmp(&rank(&b.kind))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.table.cmp(&b.table))
            .then_with(|| a.detail.cmp(&b.detail))
    });
    Ok(objects)
}

/// The catalogue query that gives the definition of `object` of `schema` (the current one with
/// `None`) in `dialect`, its text in the first row; `None` for a kind whose definition is not read
/// there (sequences, and every kind of DuckDB, Oracle and SQL Server) (`SPEC-0013` v7
/// behaviour 20).
pub fn definition_query(
    dialect: Dialect,
    schema: Option<&str>,
    object: &SchemaObject,
) -> Option<(String, usize)> {
    let lit = |name: &str| format!("'{}'", name.replace('\'', "''"));
    let name = lit(&object.name);
    match dialect {
        Dialect::PostgreSql => {
            let s = schema.map_or_else(|| "current_schema()".into(), lit);
            let routine = |columns: &str, from: &str| {
                let args = object
                    .detail
                    .as_deref()
                    .map(|d| d.trim_start_matches('(').trim_end_matches(')'))
                    .unwrap_or_default();
                format!(
                    "select {columns} from {from} join pg_namespace n on n.oid = p.pronamespace \
                    where n.nspname = {s} and p.proname = {name} \
                    and pg_get_function_identity_arguments(p.oid) = {}",
                    lit(args)
                )
            };
            let sql = match object.kind.as_str() {
                "function" | "procedure" => routine("pg_get_functiondef(p.oid)", "pg_proc p"),
                // pg_get_functiondef refuses aggregates: written from pg_aggregate.
                "aggregate" => routine(
                    "format('CREATE AGGREGATE %I.%I(%s) (SFUNC = %s, STYPE = %s%s%s%s);', \
                    n.nspname, p.proname, pg_get_function_identity_arguments(p.oid), \
                    a.aggtransfn::regproc, format_type(a.aggtranstype, null), \
                    case when a.aggfinalfn <> 0 then ', FINALFUNC = ' || a.aggfinalfn::regproc end, \
                    case when a.aggcombinefn <> 0 then ', COMBINEFUNC = ' || a.aggcombinefn::regproc end, \
                    case when a.agginitval is not null then ', INITCOND = ' || quote_literal(a.agginitval) end)",
                    "pg_aggregate a join pg_proc p on p.oid = a.aggfnoid",
                ),
                "trigger" => format!(
                    "select pg_get_triggerdef(t.oid, true) || ';' from pg_trigger t \
                    join pg_class c on c.oid = t.tgrelid join pg_namespace n on n.oid = c.relnamespace \
                    where n.nspname = {s} and t.tgname = {name} and c.relname = {} \
                    and not t.tgisinternal",
                    lit(object.table.as_deref().unwrap_or_default())
                ),
                "type" => {
                    let head = "from pg_type t join pg_namespace n on n.oid = t.typnamespace";
                    let at = format!("n.nspname = {s} and t.typname = {name}");
                    match object.detail.as_deref() {
                        Some("enum") => format!(
                            "select format('CREATE TYPE %I.%I AS ENUM (%s);', n.nspname, t.typname, \
                            string_agg(quote_literal(e.enumlabel), ', ' order by e.enumsortorder)) \
                            {head} join pg_enum e on e.enumtypid = t.oid where {at} \
                            group by n.nspname, t.typname"
                        ),
                        // PostgreSQL 17 and later also keep NOT NULL as a constraint of the
                        // domain: only the checks are listed, NOT NULL comes from typnotnull.
                        Some("domain") => format!(
                            "select format('CREATE DOMAIN %I.%I AS %s%s%s%s;', n.nspname, t.typname, \
                            format_type(t.typbasetype, t.typtypmod), \
                            case when t.typdefault is not null then ' DEFAULT ' || t.typdefault end, \
                            case when t.typnotnull then ' NOT NULL' end, \
                            (select string_agg(' CONSTRAINT ' || quote_ident(c.conname) || ' ' || \
                            pg_get_constraintdef(c.oid), '' order by c.conname) \
                            from pg_constraint c where c.contypid = t.oid and c.contype = 'c')) \
                            {head} where {at}"
                        ),
                        Some("range") => format!(
                            "select format('CREATE TYPE %I.%I AS RANGE (SUBTYPE = %s);', n.nspname, \
                            t.typname, format_type(r.rngsubtype, null)) {head} \
                            join pg_range r on r.rngtypid = t.oid where {at}"
                        ),
                        _ => format!(
                            "select format('CREATE TYPE %I.%I AS (%s);', n.nspname, t.typname, \
                            string_agg(quote_ident(a.attname) || ' ' || \
                            format_type(a.atttypid, a.atttypmod), ', ' order by a.attnum)) {head} \
                            join pg_attribute a on a.attrelid = t.typrelid \
                            where {at} and a.attnum > 0 and not a.attisdropped \
                            group by n.nspname, t.typname"
                        ),
                    }
                }
                _ => return None,
            };
            Some((sql, 0))
        }
        Dialect::MySql | Dialect::MariaDb => {
            let what = match object.kind.as_str() {
                "function" => "FUNCTION",
                "procedure" => "PROCEDURE",
                "trigger" => "TRIGGER",
                _ => return None,
            };
            let q = |n: &str| crate::quote_identifier(n, dialect);
            let target = match schema {
                Some(s) => format!("{}.{}", q(s), q(&object.name)),
                None => q(&object.name),
            };
            // `Create Function` / `Create Procedure` / `SQL Original Statement` is the third column.
            Some((format!("SHOW CREATE {what} {target}"), 2))
        }
        Dialect::Sqlite if object.kind == "trigger" => {
            let master = match schema {
                Some(name) => format!(
                    "{}.sqlite_master",
                    crate::quote_identifier(name, Dialect::Sqlite)
                ),
                None => "sqlite_master".into(),
            };
            Some((
                format!("select sql || ';' from {master} where type = 'trigger' and name = {name}"),
                0,
            ))
        }
        _ => None,
    }
}

/// Why a definition could not be shown, for a row without text: MySQL and MariaDB give the
/// statement only to the definer or with the `SHOW_ROUTINE`/`SELECT` privileges on `mysql.proc`.
pub const NO_DEFINITION_PRIVILEGE: &str =
    "the database gave no definition: the user may lack the privilege to see it";

/// The definition in the rows of [`definition_query`]: the text at `column` of the first row;
/// an error when the row is there but empty (no privilege), `None` without a row.
pub fn definition_from_rows(
    rows: &[Vec<Option<String>>],
    column: usize,
) -> Result<Option<String>, String> {
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    match cell(row, column) {
        Some(text) => Ok(Some(text.trim_end().to_owned())),
        None => Err(NO_DEFINITION_PRIVILEGE.into()),
    }
}

/// The definition of `object` of `schema` (the current schema with `None`) as the database states
/// it (`SPEC-0013` v7 behaviour 20); `Ok(None)` when it is gone or its kind has no definition to
/// read. Nothing is changed.
pub async fn object_definition(
    session: &Session,
    schema: Option<&str>,
    object: &SchemaObject,
) -> Result<Option<String>, DbError> {
    let Some((sql, column)) = definition_query(session.dialect(), schema, object) else {
        return Ok(None);
    };
    let rows = query(session, &sql).await?;
    definition_from_rows(&rows, column).map_err(|message| DbError {
        message,
        sqlstate: None,
        position: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[Option<&str>]) -> Vec<Option<String>> {
        cells.iter().map(|c| c.map(str::to_owned)).collect()
    }

    /// SPEC-0013 v6 AK-14: fixed catalogue rows of PostgreSQL and MySQL become objects.
    #[test]
    fn catalogue_rows_become_objects() {
        let routines = vec![
            row(&[Some("add_tax"), Some("function"), Some("amount numeric")]),
            row(&[Some("add_tax"), Some("function"), Some("")]),
            row(&[Some("restock"), Some("procedure"), None]),
            row(&[None, Some("function"), None]),
        ];
        let got = objects_from_rows(Dialect::PostgreSql, CatalogPart::Routines, &routines);
        assert_eq!(got.len(), 3, "a row without a name is left out");
        assert_eq!(got[0].detail.as_deref(), Some("(amount numeric)"));
        assert_eq!(got[1].detail.as_deref(), Some("()"), "overloads differ");
        assert_eq!(got[2].kind, "procedure");
        let mysql = objects_from_rows(Dialect::MySql, CatalogPart::Routines, &routines[..1]);
        assert_eq!(mysql[0].detail, None, "MySQL has no overloads");
        let triggers = vec![row(&[
            Some("stock_check"),
            Some("stock"),
            Some("BEFORE INSERT OR UPDATE"),
        ])];
        assert_eq!(
            objects_from_rows(Dialect::PostgreSql, CatalogPart::Triggers, &triggers),
            [SchemaObject {
                name: "stock_check".into(),
                kind: "trigger".into(),
                table: Some("stock".into()),
                detail: Some("BEFORE INSERT OR UPDATE".into()),
            }]
        );
        let types = vec![row(&[Some("mood"), Some("enum")])];
        let t = objects_from_rows(Dialect::PostgreSql, CatalogPart::Types, &types);
        assert_eq!(
            (t[0].kind.as_str(), t[0].detail.as_deref()),
            ("type", Some("enum"))
        );
        let restock = objects_from_rows(Dialect::MySql, CatalogPart::Routines, &routines[2..3]);
        let json = serde_json::to_value(&restock[0]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "name": "restock", "kind": "procedure" }),
            "empty fields are left out"
        );
    }

    /// SPEC-0013 v6 behaviour 18: the queries read only the catalogue, with the schema quoted.
    #[test]
    fn queries_per_dialect_read_the_catalogue() {
        let parts = |d| {
            object_queries(d, Some("o'x"))
                .into_iter()
                .map(|(p, _)| p)
                .collect::<Vec<_>>()
        };
        use CatalogPart::*;
        assert_eq!(
            parts(Dialect::PostgreSql),
            [Sequences, Routines, Triggers, Types]
        );
        assert_eq!(parts(Dialect::MySql), [Routines, Triggers]);
        assert_eq!(parts(Dialect::MariaDb), [Sequences, Routines, Triggers]);
        assert_eq!(parts(Dialect::Sqlite), [Triggers]);
        assert_eq!(parts(Dialect::Oracle), [Sequences]);
        for d in [
            Dialect::PostgreSql,
            Dialect::MySql,
            Dialect::MariaDb,
            Dialect::Sqlite,
        ] {
            for (_, sql) in object_queries(d, Some("o'x")) {
                let lower = sql.to_ascii_lowercase();
                assert!(lower.starts_with("select "), "{sql}");
                assert!(sql.contains("'o''x'") || sql.contains("\"o'x\""), "{sql}");
            }
        }
        assert!(
            object_queries(Dialect::Sqlite, None)[0]
                .1
                .contains(" sqlite_master ")
        );
        assert!(
            object_queries(Dialect::PostgreSql, None)[1]
                .1
                .contains("current_schema()")
        );
        // Extension members and the constructors of range types are left out.
        assert!(
            object_queries(Dialect::PostgreSql, None)[1]
                .1
                .contains("d.deptype in ('e', 'i')")
        );
        for d in [Dialect::PostgreSql, Dialect::MySql, Dialect::Sqlite] {
            assert_eq!(&object_kinds(d)[..2], ["table", "view"]);
        }
    }

    /// SPEC-0013 v6 behaviour 18: when a SQLite trigger fires.
    #[test]
    fn sqlite_trigger_events() {
        let e = |s| sqlite_trigger_event(s);
        assert_eq!(
            e("CREATE TRIGGER t AFTER INSERT ON stock BEGIN SELECT 1; END").as_deref(),
            Some("AFTER INSERT")
        );
        assert_eq!(
            e("create trigger t delete on stock begin select 1; end").as_deref(),
            Some("BEFORE DELETE")
        );
        assert_eq!(
            e("CREATE TRIGGER t INSTEAD OF UPDATE ON v BEGIN SELECT 1; END").as_deref(),
            Some("INSTEAD OF UPDATE")
        );
        assert_eq!(
            e("CREATE TRIGGER t BEFORE UPDATE OF qty, sku ON stock BEGIN SELECT 1; END").as_deref(),
            Some("BEFORE UPDATE OF qty, sku")
        );
        assert_eq!(e("CREATE TRIGGER t"), None);
    }

    fn object(kind: &str, name: &str, table: Option<&str>, detail: Option<&str>) -> SchemaObject {
        SchemaObject {
            name: name.into(),
            kind: kind.into(),
            table: table.map(str::to_owned),
            detail: detail.map(str::to_owned),
        }
    }

    /// SPEC-0013 v7 AK-16: one read-only catalogue query per kind and dialect, names as literals.
    #[test]
    fn definition_queries_per_kind() {
        let pg = |o: &SchemaObject| definition_query(Dialect::PostgreSql, Some("o'x"), o).unwrap();
        let (f, col) = pg(&object(
            "function",
            "add_tax",
            None,
            Some("(amount numeric)"),
        ));
        assert_eq!(col, 0);
        assert!(f.contains("pg_get_functiondef(p.oid)"), "{f}");
        assert!(
            f.contains("= 'amount numeric'") && f.contains("'o''x'"),
            "{f}"
        );
        let (a, _) = pg(&object("aggregate", "total", None, Some("(numeric)")));
        assert!(
            a.contains("CREATE AGGREGATE") && !a.contains("pg_get_functiondef"),
            "{a}"
        );
        let (t, _) = pg(&object("trigger", "stock_check", Some("stock"), None));
        assert!(
            t.contains("pg_get_triggerdef") && t.contains("c.relname = 'stock'"),
            "{t}"
        );
        for (detail, word) in [
            ("enum", "pg_enum"),
            ("domain", "c.contype = 'c'"),
            ("range", "pg_range"),
            ("composite", "pg_attribute"),
        ] {
            let (sql, _) = pg(&object("type", "mood", None, Some(detail)));
            assert!(sql.contains(word), "{detail}: {sql}");
        }
        assert!(
            definition_query(
                Dialect::PostgreSql,
                None,
                &object("sequence", "s", None, None)
            )
            .is_none()
        );
        let my = |kind| {
            definition_query(
                Dialect::MySql,
                Some("shop"),
                &object(kind, "add_tax", None, None),
            )
        };
        assert_eq!(
            my("function"),
            Some(("SHOW CREATE FUNCTION `shop`.`add_tax`".into(), 2))
        );
        assert_eq!(
            my("procedure").unwrap().0,
            "SHOW CREATE PROCEDURE `shop`.`add_tax`"
        );
        assert_eq!(
            my("trigger").unwrap().0,
            "SHOW CREATE TRIGGER `shop`.`add_tax`"
        );
        let (lite, _) = definition_query(
            Dialect::Sqlite,
            None,
            &object("trigger", "a'b", Some("t"), None),
        )
        .unwrap();
        assert!(
            lite.contains("from sqlite_master") && lite.contains("name = 'a''b'"),
            "{lite}"
        );
        assert!(
            definition_query(Dialect::Oracle, None, &object("trigger", "t", None, None)).is_none()
        );
        for d in [
            Dialect::PostgreSql,
            Dialect::MySql,
            Dialect::MariaDb,
            Dialect::Sqlite,
        ] {
            for o in [
                object("function", "f", None, Some("()")),
                object("trigger", "t", Some("x"), None),
                object("type", "y", None, Some("enum")),
            ] {
                if let Some((sql, _)) = definition_query(d, None, &o) {
                    let first = sql.split_whitespace().next().unwrap().to_ascii_lowercase();
                    assert!(matches!(first.as_str(), "select" | "show"), "{sql}");
                }
            }
        }
    }

    /// SPEC-0013 v7 AK-16: the text of fixed catalogue rows; a row without text names the privilege.
    #[test]
    fn definitions_from_catalogue_rows() {
        let show = vec![row(&[
            Some("add_tax"),
            Some("STRICT_TRANS_TABLES"),
            Some("CREATE FUNCTION `add_tax`(a int) RETURNS int RETURN a * 2\n"),
        ])];
        assert_eq!(
            definition_from_rows(&show, 2),
            Ok(Some(
                "CREATE FUNCTION `add_tax`(a int) RETURNS int RETURN a * 2".into()
            ))
        );
        let hidden = vec![row(&[Some("add_tax"), Some(""), None])];
        assert_eq!(
            definition_from_rows(&hidden, 2),
            Err(NO_DEFINITION_PRIVILEGE.into())
        );
        assert_eq!(definition_from_rows(&[], 0), Ok(None));
    }
}
