//! Schema introspection (`SPEC-0013`).
//!
//! Catalog queries per dialect run through the ordinary session, so the
//! results arrive as text rows and are assembled into one model.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::{DbError, Dialect, OnError, Outcome, Session};

/// A table or view with its structure. `Default` is an empty table without a name, for tests that
/// spell out only the fields they need.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Table {
    /// Table name.
    pub name: String,
    /// `table` or `view`.
    pub kind: String,
    /// Columns in definition order.
    pub columns: Vec<Column>,
    /// Primary key columns in key order (empty if none).
    pub primary_key: Vec<String>,
    /// Foreign keys, sorted by name, then columns.
    pub foreign_keys: Vec<ForeignKey>,
    /// Indexes, sorted by name.
    pub indexes: Vec<Index>,
    /// Storage parameters as the database lists them, sorted: PostgreSQL's `reloptions` of the table
    /// (`autovacuum_enabled=false`) and of its TOAST table (`toast.autovacuum_enabled=false`)
    /// (`SPEC-0018` v9 behaviour 21); empty where none are set or the database's are not read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub storage: Vec<String>,
    /// Columns a virtual table has but hides (SQLite: FTS5's `rank` and the column named after the
    /// table, FTS4's `docid`): a query may name them, but they are no columns of the table's
    /// structure (`SPEC-0015` v3 behaviour 11).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hidden_columns: Vec<String>,
    /// The database's message when it refuses to describe the object, so it is listed without
    /// columns: a SQLite view that names a dropped table or column (`SPEC-0013` v4 behaviour 14).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
    /// Check and unique constraints, sorted by name (PostgreSQL, `SPEC-0013` v5 behaviour 16); empty
    /// where the database system's are not read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<TableConstraint>,
}

/// A check or unique constraint of a table (`SPEC-0013` v5 behaviour 16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableConstraint {
    /// Constraint name.
    pub name: String,
    /// `check` or `unique`.
    pub kind: String,
    /// A unique constraint's columns in key order; empty for a check.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// The definition as the database writes it: `CHECK (qty >= 0)`, `UNIQUE (body)`.
    pub definition: String,
}

impl TableConstraint {
    /// Whether it is a unique constraint (whose index has its name).
    pub fn is_unique(&self) -> bool {
        self.kind == "unique"
    }
}

/// One column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Column {
    /// Column name.
    pub name: String,
    /// Type as the database spells it.
    pub data_type: String,
    /// Whether NULL is allowed.
    pub nullable: bool,
    /// Default expression, if any.
    pub default: Option<String>,
    /// `virtual` or `stored` for a column the database computes from others (SQLite generated
    /// columns, `SPEC-0013` v3 behaviour 8); such a column takes no value on insert or update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<String>,
}

/// One foreign key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForeignKey {
    /// Constraint name (SQLite has none).
    pub name: Option<String>,
    /// Referencing columns.
    pub columns: Vec<String>,
    /// Referenced table.
    pub referenced_table: String,
    /// Referenced columns.
    pub referenced_columns: Vec<String>,
}

/// One index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Index {
    /// Index name.
    pub name: String,
    /// Whether values must be unique.
    pub unique: bool,
    /// Columns or expressions in index order.
    pub columns: Vec<String>,
    /// The condition of a partial index: the text after `WHERE` of its statement, as written
    /// (SQLite, `SPEC-0013` v4 behaviour 12). `None` for an index over all rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// The collation of each term in the order of `columns` where it is not the default one
    /// (SQLite, `SPEC-0013` v4 behaviour 13); empty when no term has one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collations: Vec<Option<String>>,
}

impl Index {
    /// Whether the index makes its columns a key of the table: unique and over all rows. A
    /// unique partial index says nothing about the rows outside its condition.
    pub fn is_key(&self) -> bool {
        self.unique && self.condition.is_none()
    }

    /// The collation of term `i`, if it is not the default one.
    pub fn collation(&self, i: usize) -> Option<&str> {
        self.collations.get(i).and_then(|c| c.as_deref())
    }

    /// The terms as a person reads them: `city collate nocase` for a term with a collation
    /// (`SPEC-0089` v9 behaviour 20).
    pub fn terms(&self) -> Vec<String> {
        self.columns
            .iter()
            .enumerate()
            .map(|(i, column)| match self.collation(i) {
                Some(collation) => format!("{column} collate {}", collation.to_lowercase()),
                None => column.clone(),
            })
            .collect()
    }

    /// The condition on one line (runs of white space as one blank), for a row of text.
    pub fn condition_line(&self) -> Option<String> {
        self.condition
            .as_ref()
            .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

type Rows = Vec<Vec<Option<String>>>;

/// Reads all tables and views of the session's current schema.
pub async fn introspect(session: &Session) -> Result<Vec<Table>, DbError> {
    introspect_schema(session, None).await
}

/// A catalogue query of the current schema of `dialect` made to read schema `name` instead:
/// PostgreSQL's and DuckDB's `current_schema()`, MySQL's `database()`, SQL Server's
/// `schema_id()`, SQLite's `sqlite_master` of an attached database. Oracle's queries read the
/// user's own objects and stay as they are.
fn in_schema(sql: &str, dialect: Dialect, name: &str) -> String {
    let literal = format!("'{}'", name.replace('\'', "''"));
    match dialect {
        Dialect::PostgreSql | Dialect::DuckDb => sql.replace("current_schema()", &literal),
        Dialect::MySql | Dialect::MariaDb => sql.replace("database()", &literal),
        Dialect::SqlServer => sql.replace("schema_id()", &format!("schema_id({literal})")),
        Dialect::Sqlite => sql
            .replace(
                "sqlite_master",
                &format!("{}.sqlite_master", crate::quote_identifier(name, dialect)),
            )
            .replace("(m.name)", &format!("(m.name, {literal})"))
            .replace("(l.name)", &format!("(l.name, {literal})"))
            .replace("schema = 'main'", &format!("schema = {literal}")),
        Dialect::Oracle => sql.to_owned(),
    }
}

/// Reads the tables and views of `schema`, or of the current schema with `None` (`SPEC-0089` v3
/// behaviour 8). Oracle reads only the user's own schema.
pub async fn introspect_schema(
    session: &Session,
    schema: Option<&str>,
) -> Result<Vec<Table>, DbError> {
    let dialect = session.dialect();
    let queries = match dialect {
        Dialect::PostgreSql => &POSTGRES,
        Dialect::MySql => &MYSQL,
        Dialect::MariaDb => &MARIADB,
        Dialect::SqlServer => &SQLSERVER,
        Dialect::Sqlite => &SQLITE,
        Dialect::Oracle => &ORACLE,
        Dialect::DuckDb => &DUCKDB,
    };
    let named = |sql: &'static str| -> String {
        match schema {
            Some(name) => in_schema(sql, dialect, name),
            None => sql.to_owned(),
        }
    };
    // A connector's own catalogue queries (`SPEC-0300` behaviour 3) take `{schema}`.
    let custom = session
        .system()
        .map(|c| c.introspection.clone())
        .unwrap_or_default();
    let needs_current = [&custom.columns, &custom.keys, &custom.indexes]
        .iter()
        .any(|q| q.as_ref().is_some_and(|q| q.contains("{schema}")));
    let target = match schema {
        Some(name) => Some(name.to_owned()),
        None if needs_current => Some(crate::current_schema(session).await?),
        None => None,
    };
    let pick = |custom: &Option<String>, family: &'static str| match custom {
        Some(q) => crate::connector::with_schema(q, target.as_deref().unwrap_or("")),
        None => named(family),
    };
    let builtin_sqlite = dialect == Dialect::Sqlite;
    let mut broken_views = Vec::new();
    let mut columns = match query(session, &pick(&custom.columns, queries.columns)).await {
        Ok(rows) => rows,
        // One view over a dropped table or column makes SQLite refuse the whole query; the
        // tables and the sound views are then read one by one (SPEC-0013 v3 behaviour 10).
        Err(_) if builtin_sqlite && custom.columns.is_none() => {
            let sql = named(queries.columns);
            let (rows, broken) = sqlite_columns_one_by_one(session, &sql, &named).await?;
            broken_views = broken;
            rows
        }
        Err(e) => return Err(e),
    };
    // Oracle keeps a default's text as written, with trailing white space and line breaks.
    if session.dialect() == Dialect::Oracle {
        for row in &mut columns {
            if let Some(Some(d)) = row.get_mut(5) {
                *d = d.trim().to_owned();
            }
        }
    }
    let keys = query(session, &pick(&custom.keys, queries.keys)).await?;
    let mut indexes = query(session, &pick(&custom.indexes, queries.indexes)).await?;
    // SQLite's own queries join names with a separator no identifier holds, so a name with a
    // comma stays one name (SPEC-0013 v3 behaviour 9); a connector's queries use commas.
    let separators = Separators {
        keys: if builtin_sqlite && custom.keys.is_none() {
            SQLITE_SEPARATOR
        } else {
            ','
        },
        indexes: if builtin_sqlite && custom.indexes.is_none() {
            // Only a database that has expression or partial indexes pays for reading their
            // statements.
            if indexes
                .iter()
                .any(|row| text(row, 3).contains(SQLITE_EXPRESSION) || text(row, 4) == "1")
            {
                let stored = query(
                    session,
                    &named(
                        "select name, sql from sqlite_master where type = 'index' and sql is not null",
                    ),
                )
                .await?;
                sqlite_index_details(&mut indexes, &stored);
            }
            SQLITE_SEPARATOR
        } else {
            ','
        },
    };
    let mut tables = assemble_with(columns, keys, indexes, separators);
    // Checks and unique constraints, through PostgreSQL's own catalogue only (SPEC-0013 v5
    // behaviour 16): a connector with queries of its own may not have `pg_get_constraintdef`.
    let builtin = custom.columns.is_none() && custom.keys.is_none() && custom.indexes.is_none();
    if dialect == Dialect::PostgreSql && builtin {
        let rows = query(session, &named(POSTGRES_CONSTRAINTS)).await?;
        attach_constraints(&mut tables, &rows);
    }
    for (name, reason) in broken_views {
        let at = tables.partition_point(|t| t.name < name);
        tables.insert(
            at,
            Table {
                storage: Vec::new(),
                hidden_columns: Vec::new(),
                name,
                kind: "view".into(),
                columns: Vec::new(),
                primary_key: Vec::new(),
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
                unreadable: Some(reason),
                constraints: Vec::new(),
            },
        );
    }
    // Storage parameters, through PostgreSQL's own catalogue only (SPEC-0018 v9 behaviour 21). A
    // server that speaks PostgreSQL's protocol without its catalogue still has its tables read.
    if dialect == Dialect::PostgreSql
        && custom.columns.is_none()
        && let Ok(rows) = query(session, &named(POSTGRES_STORAGE)).await
    {
        attach_storage(&mut tables, &rows);
    }
    Ok(tables)
}

/// PostgreSQL's check and unique constraints of the current schema's tables: table, name, `c`/`u`,
/// columns (comma-separated, key order), definition written relative to the schema read.
const POSTGRES_CONSTRAINTS: &str = "select x.* from \
    (select set_config('search_path', quote_ident(current_schema()), true) as path) s \
    cross join lateral (select c.relname, con.conname, con.contype, \
    array_to_string(array(select a.attname from unnest(con.conkey) with ordinality k(n, o) \
      join pg_attribute a on a.attrelid = con.conrelid and a.attnum = k.n order by k.o), ',') \
      as columns, \
    pg_get_constraintdef(con.oid, true) as definition \
    from pg_constraint con join pg_class c on c.oid = con.conrelid \
    join pg_namespace n on n.oid = c.relnamespace \
    where s.path is not null and n.nspname = current_schema() and con.contype in ('c', 'u') \
    and c.relkind in ('r', 'p')) x \
    order by 1, 2";

/// Attaches catalogue rows of `POSTGRES_CONSTRAINTS` to their tables; rows of an unknown table or
/// of a view are ignored.
fn attach_constraints(tables: &mut [Table], rows: &Rows) {
    for row in rows {
        let Some(table) = tables
            .iter_mut()
            .find(|t| t.name == text(row, 0) && t.kind == "table")
        else {
            continue;
        };
        let unique = text(row, 2) == "u";
        table.constraints.push(TableConstraint {
            name: text(row, 1),
            kind: if unique { "unique" } else { "check" }.to_owned(),
            columns: if unique {
                list(text(row, 3), ',')
            } else {
                Vec::new()
            },
            definition: text(row, 4),
        });
    }
    for table in tables {
        table.constraints.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

/// PostgreSQL's storage parameters of the current schema's tables: table, then its own parameters
/// and those of its TOAST table (prefixed `toast.`), sorted and joined by the unit separator.
const POSTGRES_STORAGE: &str = "select c.relname, array_to_string(array(\
    select o from unnest(coalesce(c.reloptions, '{}'::text[])) o \
    union all select 'toast.' || o from unnest(coalesce(t.reloptions, '{}'::text[])) o \
    order by 1), chr(31)) \
    from pg_class c join pg_namespace n on n.oid = c.relnamespace \
    left join pg_class t on t.oid = c.reltoastrelid \
    where n.nspname = current_schema() and c.relkind in ('r', 'p') \
    and (c.reloptions is not null or t.reloptions is not null) \
    order by 1";

/// Attaches catalogue rows of `POSTGRES_STORAGE` to their tables; rows of an unknown table or of a
/// view are ignored.
fn attach_storage(tables: &mut [Table], rows: &Rows) {
    for row in rows {
        if let Some(table) = tables
            .iter_mut()
            .find(|t| t.name == text(row, 0) && t.kind == "table")
        {
            table.storage = list(text(row, 1), SQLITE_SEPARATOR);
            table.storage.sort();
        }
    }
}

/// Separator between the names of a list in SQLite's own catalogue queries (unit separator).
const SQLITE_SEPARATOR: char = '\u{1f}';
/// Stands for an expression in the column list of a SQLite index (record separator).
const SQLITE_EXPRESSION: &str = "\u{1e}";

/// What separates the names in the list columns of the key and index rows.
#[derive(Clone, Copy)]
struct Separators {
    keys: char,
    indexes: char,
}

/// The column rows of a SQLite schema when the query over all objects failed: the tables in one
/// query, then each view by itself. Returns the rows and the views that cannot be read (they
/// name a table or column that no longer exists), each with SQLite's message.
async fn sqlite_columns_one_by_one(
    session: &Session,
    sql: &str,
    named: &impl Fn(&'static str) -> String,
) -> Result<(Rows, Vec<(String, String)>), DbError> {
    let both = "m.type in ('table', 'virtual', 'view')";
    let mut rows = query(
        session,
        &sql.replace(both, "m.type in ('table', 'virtual')"),
    )
    .await?;
    let views = query(
        session,
        &named("select name from sqlite_master where type = 'view' order by 1"),
    )
    .await?;
    let mut broken = Vec::new();
    for view in views.iter().map(|r| text(r, 0)) {
        let one = format!(
            "m.type = 'view' and m.name = '{}'",
            view.replace('\'', "''")
        );
        match query(session, &sql.replace(both, &one)).await {
            Ok(found) => rows.extend(found),
            Err(e) => {
                // SQLite's own words, without the result code the session puts before them.
                let reason = match e.message.split_once("): ") {
                    Some((code, said)) if code.starts_with("SQLITE_") => said.to_owned(),
                    _ => e.message,
                };
                broken.push((view, reason));
            }
        }
    }
    rows.sort_by_key(|row| text(row, 0));
    Ok((rows, broken))
}

/// Completes SQLite index rows (table, index, unique, columns, partial, collations) from the
/// stored statements (`stored`: index name, `CREATE INDEX`): the placeholders of expressions
/// become the expression's text, the flag of a partial index its condition.
fn sqlite_index_details(indexes: &mut Rows, stored: &Rows) {
    let statements: BTreeMap<String, String> = stored
        .iter()
        .map(|row| (text(row, 0), text(row, 1)))
        .collect();
    for row in indexes {
        let statement = statements.get(&text(row, 1));
        if text(row, 4) == "1" {
            let condition = statement
                .and_then(|sql| index_condition(sql))
                .unwrap_or_else(|| "<condition>".to_owned());
            row[4] = Some(condition);
        } else if let Some(cell) = row.get_mut(4) {
            *cell = None;
        }
        let columns = text(row, 3);
        if !columns.contains(SQLITE_EXPRESSION) {
            continue;
        }
        let written = statement.map(|sql| index_terms(sql)).unwrap_or_default();
        let names: Vec<&str> = columns.split(SQLITE_SEPARATOR).collect();
        let filled: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                if *name != SQLITE_EXPRESSION {
                    (*name).to_owned()
                } else if written.len() == names.len() {
                    written[i].clone()
                } else {
                    "<expression>".to_owned()
                }
            })
            .collect();
        if let Some(cell) = row.get_mut(3) {
            *cell = Some(filled.join(&SQLITE_SEPARATOR.to_string()));
        }
    }
}

/// The terms of the column list of a stored `CREATE INDEX … ON table (…)`, each without its
/// `ASC`/`DESC`; empty when the text cannot be read.
pub(crate) fn index_terms(sql: &str) -> Vec<String> {
    index_list(sql).map(|(terms, _)| terms).unwrap_or_default()
}

/// The condition of a stored `CREATE INDEX … ON table (…) WHERE condition`: the text after
/// `WHERE` as written, without the closing `;`. `None` when there is none or the text cannot be
/// read.
pub(crate) fn index_condition(sql: &str) -> Option<String> {
    let (_, end) = index_list(sql)?;
    let rest = sql.get(end..)?.trim_start();
    let keyword = rest.get(..5)?;
    let after = &rest[5..];
    if !keyword.eq_ignore_ascii_case("where")
        || !after.starts_with(|c: char| c.is_whitespace() || c == '(')
    {
        return None;
    }
    let condition = after.trim().trim_end_matches(';').trim_end();
    (!condition.is_empty()).then(|| condition.to_owned())
}

/// The terms of the column list of a stored `CREATE INDEX` and the offset after its closing
/// bracket.
fn index_list(sql: &str) -> Option<(Vec<String>, usize)> {
    let bytes = sql.as_bytes();
    let mut terms = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\'' | b'"' | b'`' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b {
                        if bytes.get(i + 1) == Some(&b) {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    i += 1;
                }
            }
            b'[' => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
            }
            b'(' => {
                depth += 1;
                if depth == 1 {
                    start = Some(i + 1);
                }
            }
            b',' | b')' if depth == 1 => {
                if let Some(from) = start {
                    terms.push(index_term(&sql[from..i]));
                }
                start = Some(i + 1);
                if b == b')' {
                    return Some((terms, i + 1));
                }
            }
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn index_term(term: &str) -> String {
    let term = term.trim();
    let lower = term.to_ascii_lowercase();
    for order in [" desc", " asc"] {
        if lower.ends_with(order) {
            return term[..term.len() - order.len()].trim_end().to_owned();
        }
    }
    term.to_owned()
}

/// The definition of a table or view as SQL (`SPEC-0089` v7 behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectDdl {
    /// `table` or `view`.
    pub kind: String,
    /// The statements, each ending in `;`.
    pub ddl: String,
}

/// Reads the definition of table or view `name` of the session's current schema; `None` when
/// there is none. SQLite gives the statements the database itself stores (the table or view,
/// then its indexes and triggers), so nothing the catalogue queries cannot see is lost: checks,
/// generated columns, collations, foreign key actions, `WITHOUT ROWID`, `STRICT`, virtual tables.
/// The other dialects generate the statements from the introspected structure
/// ([`crate::object_ddl`]).
pub async fn show_ddl(session: &Session, name: &str) -> Result<Option<ObjectDdl>, DbError> {
    show_ddl_in(session, None, name).await
}

/// [`show_ddl`] of table or view `name` of `schema` (the current schema with `None`), such as an
/// attached SQLite database (`SPEC-0089` v8 behaviour 19).
pub async fn show_ddl_in(
    session: &Session,
    schema: Option<&str>,
    name: &str,
) -> Result<Option<ObjectDdl>, DbError> {
    if session.dialect() == Dialect::Sqlite {
        return sqlite_stored_ddl(session, schema, name).await;
    }
    let tables = introspect_schema(session, schema).await?;
    // Without view definitions a view's DDL says so instead of failing.
    let views = view_definitions_in(session, schema)
        .await
        .unwrap_or_default();
    let kind = tables
        .iter()
        .find(|t| t.name == name)
        .map_or_else(|| "view".to_owned(), |t| t.kind.clone());
    Ok(crate::object_ddl(&tables, &views, name, session.dialect())
        .map(|ddl| ObjectDdl { kind, ddl }))
}

async fn sqlite_stored_ddl(
    session: &Session,
    schema: Option<&str>,
    name: &str,
) -> Result<Option<ObjectDdl>, DbError> {
    let literal = format!("'{}'", name.replace('\'', "''"));
    let master = schema.map_or_else(
        || "sqlite_master".to_owned(),
        |s| {
            format!(
                "{}.sqlite_master",
                crate::quote_identifier(s, Dialect::Sqlite)
            )
        },
    );
    let rows = query(
        session,
        &format!(
            "select type, sql from {master} where sql is not null and \
            ((type in ('table', 'view') and name = {literal}) \
            or (type in ('index', 'trigger') and tbl_name = {literal} collate nocase)) \
            order by case type when 'index' then 1 when 'trigger' then 2 else 0 end, name"
        ),
    )
    .await?;
    let Some(kind) = rows
        .first()
        .map(|r| text(r, 0))
        .filter(|kind| kind == "table" || kind == "view")
    else {
        return Ok(None);
    };
    let mut ddl = String::new();
    for (i, row) in rows.iter().enumerate() {
        if i == 1 {
            ddl.push('\n');
        }
        ddl.push_str(text(row, 1).trim().trim_end_matches(';').trim_end());
        ddl.push_str(";\n");
    }
    Ok(Some(ObjectDdl { kind, ddl }))
}

/// The stored query of a view (`SPEC-0013` v2 behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewDef {
    /// View name.
    pub name: String,
    /// The query after `AS`, as the database returns it.
    pub definition: String,
}

/// A named constraint of a table (`SPEC-0013` v2 behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConstraintName {
    /// Table name.
    pub table: String,
    /// Constraint name.
    pub name: String,
    /// `primary key`, `unique`, `check` or `foreign key`.
    pub kind: String,
}

/// Reads the definitions of the views of the session's current schema, sorted by name.
pub async fn view_definitions(session: &Session) -> Result<Vec<ViewDef>, DbError> {
    view_definitions_in(session, None).await
}

/// [`view_definitions`] of `schema` (the current schema with `None`).
pub async fn view_definitions_in(
    session: &Session,
    schema: Option<&str>,
) -> Result<Vec<ViewDef>, DbError> {
    let custom = session.system().and_then(|c| c.introspection.views.clone());
    let custom = match custom {
        Some(q) if q.contains("{schema}") => {
            let target = match schema {
                Some(name) => name.to_owned(),
                None => crate::current_schema(session).await?,
            };
            Some(crate::connector::with_schema(&q, &target))
        }
        other => other,
    };
    let family = match session.dialect() {
        // Written relative to the schema read (SPEC-0013 v5 behaviour 17): the search path is that
        // schema while the definitions are printed, and only until the end of this one statement
        // (`set_config(…, true)`); the lateral join reads them after it is set.
        Dialect::PostgreSql => {
            "select v.relname, v.definition from \
            (select set_config('search_path', quote_ident(current_schema()), true) as path) s \
            cross join lateral (select c.relname, pg_get_viewdef(c.oid, true) as definition \
            from pg_class c join pg_namespace n on n.oid = c.relnamespace \
            where s.path is not null and n.nspname = current_schema() and c.relkind in ('v', 'm')) v \
            order by 1"
        }
        Dialect::MySql | Dialect::MariaDb => {
            // The server qualifies names with the database; without it the view can be
            // created in another database (SPEC-0018 v2).
            "select TABLE_NAME, replace(VIEW_DEFINITION, concat('`', database(), '`.'), '') \
            from information_schema.VIEWS where TABLE_SCHEMA = database() order by 1"
        }
        Dialect::Sqlite => "select name, sql from sqlite_master where type = 'view' order by 1",
        Dialect::SqlServer => {
            "select o.name, m.definition from sys.views o join sys.sql_modules m \
            on m.object_id = o.object_id where o.schema_id = schema_id() order by 1"
        }
        Dialect::Oracle => "SELECT view_name, text FROM user_views ORDER BY 1",
        Dialect::DuckDb => {
            "select view_name, sql from duckdb_views() where schema_name = current_schema() \
            and database_name = current_database() and not internal order by 1"
        }
    };
    let family = match schema {
        Some(name) => in_schema(family, session.dialect(), name),
        None => family.to_owned(),
    };
    let rows = query(session, custom.as_deref().unwrap_or(&family)).await?;
    Ok(rows
        .iter()
        .map(|r| ViewDef {
            name: text(r, 0),
            definition: view_body(&text(r, 1)),
        })
        .collect())
}

/// The sequences of `schema` (the current schema with `None`), sorted by name: PostgreSQL
/// (`information_schema.sequences`, `SPEC-0089` v5 behaviour 12), and since `SPEC-0116` v3
/// behaviour 11 MariaDB, DuckDB, Oracle and SQL Server; MySQL and SQLite have none.
pub async fn sequences(session: &Session, schema: Option<&str>) -> Result<Vec<String>, DbError> {
    let Some(sql) = sequence_query(session.dialect(), schema) else {
        return Ok(Vec::new());
    };
    let rows = query(session, &sql).await?;
    let mut names: Vec<String> = rows.iter().map(|r| text(r, 0)).collect();
    names.sort();
    Ok(names)
}

/// The query listing the sequences of `schema` (the current one with `None`) in `dialect`; `None`
/// for databases without sequences (MySQL, SQLite).
pub fn sequence_query(dialect: Dialect, schema: Option<&str>) -> Option<String> {
    let lit = |name: &str| format!("'{}'", name.replace('\'', "''"));
    Some(match dialect {
        Dialect::PostgreSql => format!(
            "select sequence_name from information_schema.sequences \
            where sequence_schema = {} order by 1",
            schema.map_or_else(|| "current_schema()".into(), lit)
        ),
        Dialect::MariaDb => format!(
            "select TABLE_NAME from information_schema.TABLES \
            where TABLE_TYPE = 'SEQUENCE' and TABLE_SCHEMA = {} order by 1",
            schema.map_or_else(|| "database()".into(), lit)
        ),
        Dialect::DuckDb => format!(
            "select sequence_name from duckdb_sequences() where schema_name = {} order by 1",
            schema.map_or_else(|| "current_schema()".into(), lit)
        ),
        Dialect::Oracle => match schema {
            Some(s) => format!(
                "select sequence_name from all_sequences where sequence_owner = {} order by 1",
                lit(s)
            ),
            None => "select sequence_name from user_sequences order by 1".into(),
        },
        Dialect::SqlServer => format!(
            "select name from sys.sequences where schema_id = {} order by 1",
            schema.map_or_else(
                || "schema_id()".into(),
                |s| format!("schema_id({})", lit(s))
            )
        ),
        Dialect::MySql | Dialect::Sqlite => return None,
    })
}

/// The query of a view: text after the `AS` that follows `CREATE … VIEW name` when the database
/// returns the whole statement (SQLite, SQL Server), else the text itself; trimmed, without `;`.
pub fn view_body(text: &str) -> String {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    let body = if lower.starts_with("create") {
        // The first ` as ` word outside quotes after the view name.
        let bytes = lower.as_bytes();
        let mut quote: Option<u8> = None;
        let mut found = None;
        for (i, &b) in bytes.iter().enumerate() {
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if matches!(b, b'"' | b'`' | b'[' | b'\'') => {
                    quote = Some(if b == b'[' { b']' } else { b });
                }
                None => {
                    let word = lower[i..].starts_with("as")
                        && i > 0
                        && bytes[i - 1].is_ascii_whitespace()
                        && bytes.get(i + 2).is_none_or(|c| c.is_ascii_whitespace());
                    if word {
                        found = Some(i + 2);
                        break;
                    }
                }
            }
        }
        found.map_or(trimmed, |i| &trimmed[i..])
    } else {
        trimmed
    };
    body.trim().trim_end_matches(';').trim().to_owned()
}

/// Reads the named constraints of the tables of the session's current schema (none on SQLite).
pub async fn constraint_names(session: &Session) -> Result<Vec<ConstraintName>, DbError> {
    let sql = match session.dialect() {
        Dialect::PostgreSql => {
            "select c.relname, con.conname, con.contype from pg_constraint con \
            join pg_class c on c.oid = con.conrelid join pg_namespace n on n.oid = c.relnamespace \
            where n.nspname = current_schema() and con.contype in ('p', 'u', 'c', 'f') order by 1, 2"
        }
        Dialect::MySql | Dialect::MariaDb => {
            "select TABLE_NAME, CONSTRAINT_NAME, case CONSTRAINT_TYPE when 'PRIMARY KEY' then 'p' \
            when 'UNIQUE' then 'u' when 'CHECK' then 'c' else 'f' end \
            from information_schema.TABLE_CONSTRAINTS where CONSTRAINT_SCHEMA = database() order by 1, 2"
        }
        Dialect::Sqlite => return Ok(Vec::new()),
        Dialect::DuckDb => {
            "select table_name, constraint_name, case constraint_type when 'PRIMARY KEY' then 'p' \
            when 'UNIQUE' then 'u' when 'CHECK' then 'c' else 'f' end from duckdb_constraints() \
            where schema_name = current_schema() and database_name = current_database() \
            and constraint_type in ('PRIMARY KEY', 'UNIQUE', 'CHECK', 'FOREIGN KEY') order by 1, 2"
        }
        Dialect::SqlServer => {
            "select t.name, o.name, case o.type when 'PK' then 'p' when 'UQ' then 'u' \
            when 'C' then 'c' else 'f' end from sys.objects o join sys.tables t \
            on t.object_id = o.parent_object_id where o.type in ('PK', 'UQ', 'C', 'F') \
            and t.schema_id = schema_id() order by 1, 2"
        }
        Dialect::Oracle => {
            "SELECT table_name, constraint_name, CASE constraint_type WHEN 'P' THEN 'p' \
            WHEN 'U' THEN 'u' WHEN 'R' THEN 'f' ELSE 'c' END FROM user_constraints \
            WHERE constraint_type IN ('P', 'U', 'C', 'R') AND constraint_name NOT LIKE 'SYS_%' \
            ORDER BY 1, 2"
        }
    };
    let rows = query(session, sql).await?;
    Ok(rows
        .iter()
        .map(|r| ConstraintName {
            table: text(r, 0),
            name: text(r, 1),
            kind: match text(r, 2).as_str() {
                "p" => "primary key",
                "u" => "unique",
                "c" => "check",
                _ => "foreign key",
            }
            .to_owned(),
        })
        .collect())
}

struct Queries {
    /// table, kind, column, type, not null (t/1/YES…), default, position, pk position (SQLite
    /// only), hidden (SQLite only: 2 virtual and 3 stored generated column)
    columns: &'static str,
    /// table, constraint name, `p`/`f`, columns (comma-separated), referenced table, referenced columns
    keys: &'static str,
    /// table, index name, unique (t/1), columns (comma-separated)
    indexes: &'static str,
}

/// DuckDB (`SPEC-0304`): `information_schema` for columns, the `duckdb_*` table functions for keys
/// and indexes.
const DUCKDB: Queries = Queries {
    columns: "select c.table_name, case when t.table_type = 'VIEW' then 'view' else 'table' end, \
        c.column_name, c.data_type, case when c.is_nullable = 'NO' then 'not null' else 'null' end, \
        c.column_default, c.ordinal_position, null \
        from information_schema.columns c join information_schema.tables t \
          on t.table_catalog = c.table_catalog and t.table_schema = c.table_schema \
          and t.table_name = c.table_name \
        where c.table_schema = current_schema() and c.table_catalog = current_database() \
        order by c.table_name, c.ordinal_position",
    keys: "select table_name, constraint_name, \
        case constraint_type when 'PRIMARY KEY' then 'p' else 'f' end, \
        array_to_string(constraint_column_names, ','), coalesce(referenced_table, ''), \
        coalesce(array_to_string(referenced_column_names, ','), '') \
        from duckdb_constraints() where schema_name = current_schema() \
        and database_name = current_database() \
        and constraint_type in ('PRIMARY KEY', 'FOREIGN KEY') order by 1, 2",
    indexes: "select table_name, index_name, case when is_unique then '1' else '0' end, \
        regexp_replace(trim(expressions::varchar, '[]'), '\\s*,\\s*', ',', 'g') \
        from duckdb_indexes() where schema_name = current_schema() \
        and database_name = current_database() order by 1, 2",
};

/// Defaults are written relative to the schema read, like view definitions (`SPEC-0013` v5
/// behaviour 17): `nextval('pd_seq'::regclass)` for the schema's own sequence wherever the session's
/// search path points.
const POSTGRES: Queries = Queries {
    columns: "select x.* from \
        (select set_config('search_path', quote_ident(current_schema()), true) as path) s \
        cross join lateral (select c.relname, \
        case when c.relkind in ('v','m') then 'view' else 'table' end as kind, \
        a.attname, format_type(a.atttypid, a.atttypmod) as data_type, \
        case when a.attnotnull then 'not null' else 'null' end as nullable, \
        pg_get_expr(d.adbin, d.adrelid) as default_value, a.attnum, null as pk \
        from pg_class c join pg_namespace n on n.oid = c.relnamespace \
        join pg_attribute a on a.attrelid = c.oid and a.attnum > 0 and not a.attisdropped \
        left join pg_attrdef d on d.adrelid = c.oid and d.adnum = a.attnum \
        where s.path is not null and n.nspname = current_schema() \
        and c.relkind in ('r', 'p', 'v', 'm')) x \
        order by x.relname, x.attnum",
    keys: "select c.relname, con.conname, con.contype, \
        array_to_string(array(select a.attname from unnest(con.conkey) with ordinality k(n, o) \
          join pg_attribute a on a.attrelid = con.conrelid and a.attnum = k.n order by k.o), ','), \
        coalesce(fc.relname, ''), \
        coalesce(array_to_string(array(select a.attname from unnest(con.confkey) with ordinality k(n, o) \
          join pg_attribute a on a.attrelid = con.confrelid and a.attnum = k.n order by k.o), ','), '') \
        from pg_constraint con join pg_class c on c.oid = con.conrelid \
        join pg_namespace n on n.oid = c.relnamespace left join pg_class fc on fc.oid = con.confrelid \
        where n.nspname = current_schema() and con.contype in ('p', 'f') order by 1, 2",
    indexes: "select t.relname, i.relname, case when ix.indisunique then '1' else '0' end, \
        array_to_string(array(select pg_get_indexdef(ix.indexrelid, k + 1, true) \
          from generate_subscripts(ix.indkey, 1) k order by k), ',') \
        from pg_index ix join pg_class t on t.oid = ix.indrelid join pg_class i on i.oid = ix.indexrelid \
        join pg_namespace n on n.oid = t.relnamespace where n.nspname = current_schema() order by 1, 2",
};

const MYSQL: Queries = Queries {
    columns: "select c.TABLE_NAME, case when t.TABLE_TYPE = 'VIEW' then 'view' else 'table' end, \
        c.COLUMN_NAME, c.COLUMN_TYPE, case when c.IS_NULLABLE = 'NO' then 'not null' else 'null' end, \
        c.COLUMN_DEFAULT, c.ORDINAL_POSITION, null \
        from information_schema.COLUMNS c join information_schema.TABLES t \
          on t.TABLE_SCHEMA = c.TABLE_SCHEMA and t.TABLE_NAME = c.TABLE_NAME \
        where c.TABLE_SCHEMA = database() order by c.TABLE_NAME, c.ORDINAL_POSITION",
    keys: "select k.TABLE_NAME, k.CONSTRAINT_NAME, \
        case when tc.CONSTRAINT_TYPE = 'PRIMARY KEY' then 'p' else 'f' end, \
        group_concat(k.COLUMN_NAME order by k.ORDINAL_POSITION separator ','), \
        coalesce(max(k.REFERENCED_TABLE_NAME), ''), \
        coalesce(group_concat(k.REFERENCED_COLUMN_NAME order by k.ORDINAL_POSITION separator ','), '') \
        from information_schema.KEY_COLUMN_USAGE k join information_schema.TABLE_CONSTRAINTS tc \
          on tc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA and tc.TABLE_NAME = k.TABLE_NAME \
          and tc.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
        where k.TABLE_SCHEMA = database() and tc.CONSTRAINT_TYPE in ('PRIMARY KEY', 'FOREIGN KEY') \
        group by k.TABLE_NAME, k.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE order by 1, 2",
    indexes: "select TABLE_NAME, INDEX_NAME, case when NON_UNIQUE = 0 then '1' else '0' end, \
        group_concat(coalesce(COLUMN_NAME, EXPRESSION) order by SEQ_IN_INDEX separator ',') \
        from information_schema.STATISTICS where TABLE_SCHEMA = database() \
        group by TABLE_NAME, INDEX_NAME, NON_UNIQUE order by 1, 2",
};

/// MariaDB reports `NULL` as the default of a column without one and has no `EXPRESSION` column
/// for functional indexes; keys are read as in MySQL.
const MARIADB: Queries = Queries {
    columns: "select c.TABLE_NAME, case when t.TABLE_TYPE = 'VIEW' then 'view' else 'table' end, \
        c.COLUMN_NAME, c.COLUMN_TYPE, case when c.IS_NULLABLE = 'NO' then 'not null' else 'null' end, \
        nullif(c.COLUMN_DEFAULT, 'NULL'), c.ORDINAL_POSITION, null \
        from information_schema.COLUMNS c join information_schema.TABLES t \
          on t.TABLE_SCHEMA = c.TABLE_SCHEMA and t.TABLE_NAME = c.TABLE_NAME \
        where c.TABLE_SCHEMA = database() order by c.TABLE_NAME, c.ORDINAL_POSITION",
    keys: MYSQL.keys,
    indexes: "select TABLE_NAME, INDEX_NAME, case when NON_UNIQUE = 0 then '1' else '0' end, \
        group_concat(COLUMN_NAME order by SEQ_IN_INDEX separator ',') \
        from information_schema.STATISTICS where TABLE_SCHEMA = database() \
        group by TABLE_NAME, INDEX_NAME, NON_UNIQUE order by 1, 2",
};

/// SQL Server 2017 or later (`string_agg`); the current schema is the user's default schema.
/// The user's own schema (`SPEC-0142`). Unquoted names are stored in capitals.
const ORACLE: Queries = Queries {
    columns: "SELECT t.name, t.kind, c.column_name, \
        CASE WHEN c.data_type IN ('VARCHAR2', 'NVARCHAR2', 'CHAR', 'NCHAR') \
               THEN c.data_type || '(' || c.char_length || CASE c.char_used WHEN 'C' THEN ' CHAR' ELSE '' END || ')' \
             WHEN c.data_type = 'RAW' THEN 'RAW(' || c.data_length || ')' \
             WHEN c.data_type = 'NUMBER' AND c.data_precision IS NOT NULL \
               THEN 'NUMBER(' || c.data_precision || ',' || c.data_scale || ')' \
             ELSE c.data_type END, \
        CASE WHEN c.nullable = 'N' THEN 'not null' ELSE 'null' END, c.data_default, c.column_id, NULL \
        FROM user_tab_columns c JOIN ( \
          SELECT table_name AS name, 'table' AS kind FROM user_tables \
          UNION ALL SELECT view_name, 'view' FROM user_views) t ON t.name = c.table_name \
        ORDER BY t.name, c.column_id",
    keys: "SELECT uc.table_name, uc.constraint_name, 'p', \
          LISTAGG(ucc.column_name, ',') WITHIN GROUP (ORDER BY ucc.position), '', '' \
        FROM user_constraints uc JOIN user_cons_columns ucc ON ucc.constraint_name = uc.constraint_name \
        WHERE uc.constraint_type = 'P' GROUP BY uc.table_name, uc.constraint_name \
        UNION ALL \
        SELECT uc.table_name, uc.constraint_name, 'f', \
          LISTAGG(ucc.column_name, ',') WITHIN GROUP (ORDER BY ucc.position), MAX(r.table_name), \
          (SELECT LISTAGG(rc.column_name, ',') WITHIN GROUP (ORDER BY rc.position) \
             FROM user_cons_columns rc WHERE rc.constraint_name = uc.r_constraint_name) \
        FROM user_constraints uc JOIN user_cons_columns ucc ON ucc.constraint_name = uc.constraint_name \
        JOIN user_constraints r ON r.constraint_name = uc.r_constraint_name \
        WHERE uc.constraint_type = 'R' GROUP BY uc.table_name, uc.constraint_name, uc.r_constraint_name \
        ORDER BY 1, 2",
    indexes: "SELECT i.table_name, i.index_name, CASE WHEN i.uniqueness = 'UNIQUE' THEN '1' ELSE '0' END, \
          LISTAGG(ic.column_name, ',') WITHIN GROUP (ORDER BY ic.column_position) \
        FROM user_indexes i JOIN user_ind_columns ic ON ic.index_name = i.index_name \
        GROUP BY i.table_name, i.index_name, i.uniqueness ORDER BY 1, 2",
};

const SQLSERVER: Queries = Queries {
    columns: "select t.name, case when t.type = 'V' then 'view' else 'table' end, c.name, \
        case when ty.name in ('varchar', 'char', 'varbinary', 'binary') \
               then ty.name + '(' + case when c.max_length = -1 then 'max' else cast(c.max_length as varchar(10)) end + ')' \
             when ty.name in ('nvarchar', 'nchar') \
               then ty.name + '(' + case when c.max_length = -1 then 'max' else cast(c.max_length / 2 as varchar(10)) end + ')' \
             when ty.name in ('decimal', 'numeric') \
               then ty.name + '(' + cast(c.precision as varchar(10)) + ',' + cast(c.scale as varchar(10)) + ')' \
             when ty.name in ('datetime2', 'time', 'datetimeoffset') \
               then ty.name + '(' + cast(c.scale as varchar(10)) + ')' \
             else ty.name end \
        + case when ic.column_id is not null then ' IDENTITY(' + cast(ic.seed_value as varchar(20)) + ',' \
               + cast(ic.increment_value as varchar(20)) + ')' else '' end, \
        case when c.is_nullable = 0 then 'not null' else 'null' end, dc.definition, c.column_id, null \
        from sys.objects t join sys.columns c on c.object_id = t.object_id \
        join sys.types ty on ty.user_type_id = c.user_type_id \
        left join sys.identity_columns ic on ic.object_id = c.object_id and ic.column_id = c.column_id \
        left join sys.default_constraints dc on dc.parent_object_id = c.object_id and dc.parent_column_id = c.column_id \
        where t.type in ('U', 'V') and t.is_ms_shipped = 0 and t.schema_id = schema_id() \
        order by t.name, c.column_id",
    keys: "select t.name, k.name, 'p', \
          string_agg(c.name, ',') within group (order by ic.key_ordinal), '', '' \
        from sys.key_constraints k join sys.tables t on t.object_id = k.parent_object_id \
        join sys.index_columns ic on ic.object_id = k.parent_object_id and ic.index_id = k.unique_index_id \
        join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id \
        where k.type = 'PK' and t.schema_id = schema_id() group by t.name, k.name \
        union all \
        select t.name, fk.name, 'f', \
          string_agg(pc.name, ',') within group (order by fkc.constraint_column_id), rt.name, \
          string_agg(rc.name, ',') within group (order by fkc.constraint_column_id) \
        from sys.foreign_keys fk join sys.tables t on t.object_id = fk.parent_object_id \
        join sys.tables rt on rt.object_id = fk.referenced_object_id \
        join sys.foreign_key_columns fkc on fkc.constraint_object_id = fk.object_id \
        join sys.columns pc on pc.object_id = fkc.parent_object_id and pc.column_id = fkc.parent_column_id \
        join sys.columns rc on rc.object_id = fkc.referenced_object_id and rc.column_id = fkc.referenced_column_id \
        where t.schema_id = schema_id() group by t.name, fk.name, rt.name order by 1, 2",
    indexes: "select t.name, i.name, case when i.is_unique = 1 then '1' else '0' end, \
          string_agg(c.name, ',') within group (order by ic.key_ordinal) \
        from sys.indexes i join sys.tables t on t.object_id = i.object_id \
        join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id \
          and ic.is_included_column = 0 \
        join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id \
        where i.type > 0 and i.is_hypothetical = 0 and t.schema_id = schema_id() \
        group by t.name, i.name, i.is_unique order by 1, 2",
};

const SQLITE: Queries = Queries {
    // `table_list` names every object once with its kind, so shadow tables of virtual tables
    // (internal storage like `sqlite_%`) are left out without a second pass; `table_xinfo` also
    // has the generated columns, `hidden = 1` are the internal columns of a virtual table.
    columns: "select m.name, case m.type when 'view' then 'view' else 'table' end, p.name, p.type, \
        case when p.\"notnull\" = 1 then 'not null' else 'null' end, p.dflt_value, p.cid, p.pk, p.hidden \
        from pragma_table_list m join pragma_table_xinfo(m.name) p \
        where m.schema = 'main' and m.type in ('table', 'virtual', 'view') \
        and m.name not like 'sqlite_%' order by m.name, p.cid",
    // Rows of the pragma come in `seq` order, so group_concat keeps column order.
    keys: "select m.name, null, 'f', group_concat(f.\"from\", char(31)), f.\"table\", group_concat(f.\"to\", char(31)) \
        from sqlite_master m join pragma_foreign_key_list(m.name) f \
        where m.type = 'table' group by m.name, f.id order by 1, 4",
    // An expression has no name: char(30) stands for it until the stored statement is read.
    // Then the flag of a partial index (its condition is read from the stored statement) and,
    // per term, the collation of a named column when it is not the default one.
    indexes: "select m.name, l.name, l.\"unique\", \
        (select group_concat(coalesce(name, char(30)), char(31)) from (select name from pragma_index_xinfo(l.name) where key = 1 order by seqno)), \
        l.partial, \
        (select group_concat(case when cid >= 0 and upper(coll) <> 'BINARY' then coll else '' end, char(31)) \
          from (select cid, coll from pragma_index_xinfo(l.name) where key = 1 order by seqno)) \
        from pragma_table_list m join pragma_index_list(m.name) l \
        where m.schema = 'main' and m.type in ('table', 'virtual') and m.name not like 'sqlite_%' \
        order by 1, 2",
};

pub(crate) async fn query(session: &Session, sql: &str) -> Result<Rows, DbError> {
    let run = session.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { rows, .. }) => Ok(rows),
        Some(Outcome::Error(e)) => Err(e),
        _ => Err(DbError {
            message: "catalog query returned no rows".into(),
            sqlstate: None,
            position: None,
        }),
    }
}

fn text(row: &[Option<String>], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

fn list(value: String, separator: char) -> Vec<String> {
    value
        .split(separator)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
fn assemble(columns: Rows, keys: Rows, indexes: Rows) -> Vec<Table> {
    assemble_with(
        columns,
        keys,
        indexes,
        Separators {
            keys: ',',
            indexes: ',',
        },
    )
}

fn assemble_with(columns: Rows, keys: Rows, indexes: Rows, separators: Separators) -> Vec<Table> {
    let mut tables: BTreeMap<String, Table> = BTreeMap::new();
    let mut sqlite_pk: BTreeMap<String, Vec<(u32, String)>> = BTreeMap::new();
    for row in &columns {
        let name = text(row, 0);
        let table = tables.entry(name.clone()).or_insert_with(|| Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: name.clone(),
            kind: text(row, 1),
            columns: Vec::new(),
            primary_key: Vec::new(),
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
            unreadable: None,
            constraints: Vec::new(),
        });
        let column = text(row, 2);
        // SQLite's hidden columns of a virtual table (`hidden = 1`).
        if text(row, 8) == "1" {
            table.hidden_columns.push(column);
            continue;
        }
        let generated = match text(row, 8).as_str() {
            "2" => Some("virtual".to_owned()),
            "3" => Some("stored".to_owned()),
            _ => None,
        };
        let mut data_type = text(row, 3);
        if generated.is_some() {
            // Older SQLite versions report the words of the clause as part of the type.
            let lower = data_type.to_ascii_lowercase();
            if let Some(at) = lower.find("generated always") {
                data_type = data_type[..at].trim_end().to_owned();
            }
        }
        table.columns.push(Column {
            name: column.clone(),
            data_type,
            nullable: text(row, 4) != "not null",
            default: row.get(5).cloned().flatten(),
            generated,
        });
        if let Ok(pk) = text(row, 7).parse::<u32>()
            && pk > 0
        {
            sqlite_pk.entry(name).or_default().push((pk, column));
        }
    }
    for (name, mut parts) in sqlite_pk {
        parts.sort();
        if let Some(table) = tables.get_mut(&name) {
            table.primary_key = parts.into_iter().map(|(_, c)| c).collect();
        }
    }
    for row in &keys {
        let Some(table) = tables.get_mut(&text(row, 0)) else {
            continue;
        };
        let columns = list(text(row, 3), separators.keys);
        if text(row, 2) == "p" {
            table.primary_key = columns;
        } else {
            table.foreign_keys.push(ForeignKey {
                name: row.get(1).cloned().flatten(),
                columns,
                referenced_table: text(row, 4),
                referenced_columns: list(text(row, 5), separators.keys),
            });
        }
    }
    for row in &indexes {
        if let Some(table) = tables.get_mut(&text(row, 0)) {
            let unique = matches!(text(row, 2).as_str(), "1" | "t" | "true");
            // Only SQLite's own query has the condition and the collations (v4).
            let sqlite = separators.indexes == SQLITE_SEPARATOR;
            let collations: Vec<Option<String>> = text(row, 5)
                .split(SQLITE_SEPARATOR)
                .map(|c| (!c.is_empty()).then(|| c.to_owned()))
                .collect();
            table.indexes.push(Index {
                name: text(row, 1),
                unique,
                columns: list(text(row, 3), separators.indexes),
                condition: row
                    .get(4)
                    .cloned()
                    .flatten()
                    .filter(|c| sqlite && !c.is_empty() && c != "0"),
                collations: if sqlite && collations.iter().any(Option::is_some) {
                    collations
                } else {
                    Vec::new()
                },
            });
        }
    }
    // A foreign key written without columns (`REFERENCES parent`) points at the primary key.
    let primary_keys: BTreeMap<String, Vec<String>> = tables
        .values()
        .map(|t| (t.name.clone(), t.primary_key.clone()))
        .collect();
    for table in tables.values_mut() {
        for key in &mut table.foreign_keys {
            if key.referenced_columns.is_empty()
                && let Some(primary) = primary_keys.get(&key.referenced_table)
                && primary.len() == key.columns.len()
            {
                key.referenced_columns.clone_from(primary);
            }
        }
    }
    let mut out: Vec<Table> = tables.into_values().collect();
    for table in &mut out {
        table
            .foreign_keys
            .sort_by(|a, b| (&a.name, &a.columns).cmp(&(&b.name, &b.columns)));
        table.indexes.sort_by(|a, b| a.name.cmp(&b.name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: &[Option<&str>]) -> Vec<Option<String>> {
        values.iter().map(|v| v.map(str::to_owned)).collect()
    }

    /// SPEC-0018 v9 AK-19 (OBS-0070 V5): storage parameters belong to their table, sorted; a view's
    /// or an unknown table's row is ignored; the query reads the named schema.
    #[test]
    fn storage_parameters_belong_to_their_table() {
        let column = |table: &str, kind: &str| {
            row(&[
                Some(table),
                Some(kind),
                Some("id"),
                Some("integer"),
                Some("not null"),
                None,
                Some("1"),
                None,
            ])
        };
        let mut tables = assemble(
            vec![
                column("pd_move", "table"),
                column("pd_note", "table"),
                column("pd_view", "view"),
            ],
            Vec::new(),
            Vec::new(),
        );
        let joined = |parts: &[&str]| parts.join("\u{1f}");
        let rows = vec![
            row(&[
                Some("pd_move"),
                Some(&joined(&[
                    "toast.autovacuum_enabled=false",
                    "autovacuum_enabled=false",
                ])),
            ]),
            row(&[Some("pd_view"), Some("security_barrier=true")]),
            row(&[Some("gone"), Some("fillfactor=70")]),
        ];
        attach_storage(&mut tables, &rows);
        assert_eq!(
            tables[0].storage,
            ["autovacuum_enabled=false", "toast.autovacuum_enabled=false"]
        );
        assert!(tables[1].storage.is_empty() && tables[2].storage.is_empty());
        // Serialised only when set.
        let json = serde_json::to_value(&tables).unwrap();
        assert_eq!(json[0]["storage"][0], "autovacuum_enabled=false");
        assert!(json[1].get("storage").is_none());
        // Another schema is read by name.
        let other = in_schema(POSTGRES_STORAGE, Dialect::PostgreSql, "pd_b");
        assert!(other.contains("n.nspname = 'pd_b'"), "{other}");
    }

    /// SPEC-0116 v3 AK-7: one sequence query per dialect that has sequences, quoted schema names.
    #[test]
    fn sequence_queries_per_dialect() {
        assert!(sequence_query(Dialect::MySql, None).is_none());
        assert!(sequence_query(Dialect::Sqlite, None).is_none());
        for d in [
            Dialect::PostgreSql,
            Dialect::MariaDb,
            Dialect::DuckDb,
            Dialect::Oracle,
            Dialect::SqlServer,
        ] {
            let q = sequence_query(d, Some("o'x")).unwrap();
            assert!(q.contains("'o''x'"), "{d:?}: {q}");
            assert!(q.starts_with("select "), "{d:?}");
            assert_eq!(crate::split_statements(&q, d).len(), 1);
        }
        assert!(
            sequence_query(Dialect::MariaDb, None)
                .unwrap()
                .contains("'SEQUENCE'")
        );
        assert!(
            sequence_query(Dialect::Oracle, None)
                .unwrap()
                .contains("user_sequences")
        );
        assert!(
            sequence_query(Dialect::SqlServer, None)
                .unwrap()
                .contains("schema_id()")
        );
    }

    #[test]
    fn assembles_columns_keys_and_indexes() {
        let columns = vec![
            row(&[
                Some("stock"),
                Some("table"),
                Some("warehouse_id"),
                Some("integer"),
                Some("not null"),
                None,
                Some("1"),
                None,
            ]),
            row(&[
                Some("stock"),
                Some("table"),
                Some("qty"),
                Some("integer"),
                Some("not null"),
                Some("0"),
                Some("3"),
                None,
            ]),
            row(&[
                Some("item"),
                Some("table"),
                Some("sku"),
                Some("text"),
                Some("null"),
                None,
                Some("1"),
                None,
            ]),
        ];
        let keys = vec![
            row(&[
                Some("stock"),
                Some("stock_pkey"),
                Some("p"),
                Some("warehouse_id,sku"),
                Some(""),
                Some(""),
            ]),
            row(&[
                Some("stock"),
                Some("fk_w"),
                Some("f"),
                Some("warehouse_id"),
                Some("warehouse"),
                Some("id"),
            ]),
        ];
        let indexes = vec![row(&[
            Some("stock"),
            Some("stock_pkey"),
            Some("t"),
            Some("warehouse_id,sku"),
        ])];
        let tables = assemble(columns, keys, indexes);
        assert_eq!(
            tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["item", "stock"]
        );
        let stock = &tables[1];
        assert_eq!(stock.columns[1].default.as_deref(), Some("0"));
        assert!(!stock.columns[0].nullable);
        assert_eq!(stock.primary_key, ["warehouse_id", "sku"]);
        assert_eq!(stock.foreign_keys[0].referenced_table, "warehouse");
        assert!(stock.indexes[0].unique);
    }

    #[test]
    fn sqlite_primary_key_comes_from_column_positions() {
        let columns = vec![
            row(&[
                Some("s"),
                Some("table"),
                Some("a"),
                Some("int"),
                Some("not null"),
                None,
                Some("0"),
                Some("2"),
            ]),
            row(&[
                Some("s"),
                Some("table"),
                Some("b"),
                Some("int"),
                Some("not null"),
                None,
                Some("1"),
                Some("1"),
            ]),
        ];
        let tables = assemble(columns, Vec::new(), Vec::new());
        assert_eq!(tables[0].primary_key, ["b", "a"]);
    }

    /// SPEC-0013 v3 behaviour 9: the terms of a stored `CREATE INDEX`, whatever they hold.
    #[test]
    fn index_terms_are_split_outside_brackets_and_quotes() {
        assert_eq!(
            index_terms("CREATE INDEX i ON t (city, amount * 2 DESC, name collate nocase)"),
            ["city", "amount * 2", "name collate nocase"]
        );
        assert_eq!(
            index_terms(
                "create unique index \"a (b\" on \"t,1\" (lower(\"x,y\"), coalesce(a, 'b)'), c asc) where c > 0"
            ),
            ["lower(\"x,y\")", "coalesce(a, 'b)')", "c"]
        );
        assert_eq!(
            index_terms("create index i on [t (] ([a,b], c)"),
            ["[a,b]", "c"]
        );
        assert!(index_terms("create index broken on t (a").is_empty());
        assert!(index_terms("").is_empty());
    }

    /// SPEC-0013 v3 behaviours 8 and 9 on SQLite's rows: generated columns are marked, names
    /// are split at SQLite's separator only, an expression gets its text, and a reference
    /// without columns takes the primary key.
    #[test]
    fn sqlite_rows_keep_commas_expressions_and_generated_columns() {
        let column = |table: &'static str,
                      name: &'static str,
                      ty: &'static str,
                      pk: &'static str,
                      hidden: &'static str| {
            row(&[
                Some(table),
                Some("table"),
                Some(name),
                Some(ty),
                Some("null"),
                None,
                Some("0"),
                Some(pk),
                Some(hidden),
            ])
        };
        let columns = vec![
            column("child", "a,b", "TEXT", "0", "0"),
            column("child", "total", "INT GENERATED ALWAYS", "0", "2"),
            column("child", "kept", "INT", "0", "3"),
            column("parent", "id", "INTEGER", "1", "0"),
        ];
        let keys = vec![row(&[
            Some("child"),
            None,
            Some("f"),
            Some("a,b"),
            Some("parent"),
            None,
        ])];
        let mut indexes = vec![row(&[
            Some("child"),
            Some("child_mix"),
            Some("0"),
            Some("a,b\u{1f}\u{1e}"),
        ])];
        let stored = vec![row(&[
            Some("child_mix"),
            Some("CREATE INDEX child_mix ON child (\"a,b\", lower(kept) desc)"),
        ])];
        sqlite_index_details(&mut indexes, &stored);
        let tables = assemble_with(
            columns,
            keys,
            indexes,
            Separators {
                keys: SQLITE_SEPARATOR,
                indexes: SQLITE_SEPARATOR,
            },
        );
        let child = &tables[0];
        assert_eq!(child.columns[0].generated, None);
        assert_eq!(child.columns[1].generated.as_deref(), Some("virtual"));
        assert_eq!(child.columns[1].data_type, "INT");
        assert_eq!(child.columns[2].generated.as_deref(), Some("stored"));
        assert_eq!(child.foreign_keys[0].columns, ["a,b"]);
        assert_eq!(child.foreign_keys[0].referenced_columns, ["id"]);
        assert_eq!(child.indexes[0].columns, ["a,b", "lower(kept)"]);
        // The generated mark is left out of the JSON of ordinary columns.
        let json = serde_json::to_value(&child.columns).unwrap();
        assert!(json[0].get("generated").is_none());
        assert_eq!(json[1]["generated"], "virtual");
        // Rows without the columns of v4 give an index without condition and collations.
        assert_eq!(child.indexes[0].condition, None);
        assert!(child.indexes[0].collations.is_empty());
    }

    /// SPEC-0013 v4 AK-10: the condition is the text after WHERE, as written.
    #[test]
    fn the_condition_of_a_partial_index_is_the_text_after_where() {
        assert_eq!(
            index_condition("CREATE INDEX a on t (city) where deleted = 0").as_deref(),
            Some("deleted = 0")
        );
        assert_eq!(
            index_condition("create unique index a on \"t (x)\" (lower(a), \"b)\")\n  WHERE(a > 1\n and b <> ')');")
                .as_deref(),
            Some("(a > 1\n and b <> ')')")
        );
        assert_eq!(index_condition("CREATE INDEX a on t (city)"), None);
        assert_eq!(index_condition("CREATE INDEX a on t (wherever)"), None);
        assert_eq!(index_condition("CREATE INDEX a on t (city) where "), None);
        assert_eq!(index_condition("not a statement"), None);
    }

    /// SPEC-0013 v4 AK-10: the partial flag becomes the stored condition, the collations stay
    /// with their terms, and an index over all rows with binary terms carries neither.
    #[test]
    fn sqlite_rows_carry_condition_and_collations() {
        let columns = vec![row(&[
            Some("t"),
            Some("table"),
            Some("city"),
            Some("TEXT"),
            Some("null"),
            None,
            Some("0"),
            Some("0"),
            Some("0"),
        ])];
        let index =
            |name: &'static str, unique, terms: &'static str, partial, coll: &'static str| {
                row(&[
                    Some("t"),
                    Some(name),
                    Some(unique),
                    Some(terms),
                    Some(partial),
                    Some(coll),
                ])
            };
        let mut indexes = vec![
            index("active", "1", "city", "1", ""),
            index("lost", "0", "city", "1", ""),
            index(
                "mixed",
                "0",
                "city\u{1f}\u{1e}\u{1f}name",
                "0",
                "\u{1f}\u{1f}nocase",
            ),
            index("plain", "0", "city\u{1f}name", "0", "\u{1f}"),
        ];
        let stored = vec![
            row(&[
                Some("active"),
                Some("CREATE UNIQUE INDEX active on t (city) where deleted = 0"),
            ]),
            row(&[
                Some("mixed"),
                Some("CREATE INDEX mixed on t (city, amount * 2 desc, name collate nocase)"),
            ]),
        ];
        sqlite_index_details(&mut indexes, &stored);
        let tables = assemble_with(
            columns,
            Vec::new(),
            indexes,
            Separators {
                keys: SQLITE_SEPARATOR,
                indexes: SQLITE_SEPARATOR,
            },
        );
        let found = &tables[0].indexes;
        assert_eq!(found[0].condition.as_deref(), Some("deleted = 0"));
        assert!(found[0].unique && !found[0].is_key());
        assert_eq!(found[1].condition.as_deref(), Some("<condition>"));
        assert_eq!(found[2].columns, ["city", "amount * 2", "name"]);
        assert_eq!(found[2].collations, [None, None, Some("nocase".to_owned())]);
        assert_eq!(
            found[2].terms(),
            ["city", "amount * 2", "name collate nocase"]
        );
        assert_eq!(found[2].condition, None);
        assert_eq!(
            (found[3].condition.clone(), found[3].collations.len()),
            (None, 0)
        );
        let json = serde_json::to_value(found).unwrap();
        assert_eq!(json[0]["condition"], "deleted = 0");
        assert!(json[0].get("collations").is_none());
        assert_eq!(
            json[2]["collations"],
            serde_json::json!([null, null, "nocase"])
        );
        assert!(json[3].get("condition").is_none() && json[3].get("collations").is_none());
    }

    /// SPEC-0013 v5 AK-12 (OBS-0068 L5).
    #[test]
    fn check_and_unique_constraints_belong_to_their_table() {
        let columns = vec![
            row(&[
                Some("pd_note"),
                Some("table"),
                Some("id"),
                Some("integer"),
                Some("not null"),
                None,
                Some("1"),
                None,
            ]),
            row(&[
                Some("pd_note"),
                Some("table"),
                Some("body"),
                Some("text"),
                Some("not null"),
                None,
                Some("2"),
                None,
            ]),
            row(&[
                Some("pd_note_big"),
                Some("view"),
                Some("body"),
                Some("text"),
                Some("null"),
                None,
                Some("1"),
                None,
            ]),
        ];
        let indexes = vec![row(&[
            Some("pd_note"),
            Some("pd_note_body_uq"),
            Some("1"),
            Some("body"),
        ])];
        let mut tables = assemble(columns, Vec::new(), indexes);
        let rows = vec![
            row(&[
                Some("pd_note"),
                Some("pd_note_qty_check"),
                Some("c"),
                Some("qty"),
                Some("CHECK (qty >= 0)"),
            ]),
            row(&[
                Some("pd_note"),
                Some("pd_note_body_uq"),
                Some("u"),
                Some("body,id"),
                Some("UNIQUE (body, id)"),
            ]),
            row(&[
                Some("pd_note_big"),
                Some("x"),
                Some("c"),
                Some(""),
                Some("CHECK (true)"),
            ]),
            row(&[
                Some("gone"),
                Some("y"),
                Some("c"),
                Some(""),
                Some("CHECK (true)"),
            ]),
        ];
        attach_constraints(&mut tables, &rows);
        let note = &tables[0];
        assert_eq!(
            note.constraints,
            [
                TableConstraint {
                    name: "pd_note_body_uq".into(),
                    kind: "unique".into(),
                    columns: vec!["body".into(), "id".into()],
                    definition: "UNIQUE (body, id)".into(),
                },
                TableConstraint {
                    name: "pd_note_qty_check".into(),
                    kind: "check".into(),
                    columns: Vec::new(),
                    definition: "CHECK (qty >= 0)".into(),
                },
            ]
        );
        assert!(note.constraints[0].is_unique());
        // The unique constraint's index stays an index.
        assert_eq!(note.indexes[0].name, "pd_note_body_uq");
        assert!(tables[1].constraints.is_empty(), "a view has none");
        // The query reads only checks and unique constraints of tables, by schema.
        assert!(POSTGRES_CONSTRAINTS.contains("con.contype in ('c', 'u')"));
        assert!(in_schema(POSTGRES_CONSTRAINTS, Dialect::PostgreSql, "pd_b").contains("'pd_b'"));
    }
}
