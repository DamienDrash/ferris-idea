//! Editing table data (`SPEC-0017`).
//!
//! Changes are collected first, rendered as SQL for review, and applied in one
//! transaction. `UPDATE` and `DELETE` also match the values that were loaded,
//! so a row changed by someone else in the meantime is a conflict, not lost.

use std::fmt;

use crate::{DbError, Dialect, OnError, Outcome, Session, Table, quote_identifier};

/// Column values by name, in the order given; `None` is SQL NULL.
pub type Values = Vec<(String, Option<String>)>;

/// Why a table or a change cannot be edited.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// Views and other non-tables are not edited.
    #[error("`{0}` is a {1}, only tables can be edited")]
    NotATable(String, String),
    /// No primary key and no unique index over NOT NULL columns.
    #[error(
        "`{0}` has no primary key and no unique index over NOT NULL columns, so rows cannot be identified"
    )]
    NoRowIdentity(String),
    /// A column that the table does not have.
    #[error("`{table}` has no column `{column}`")]
    UnknownColumn {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
    },
    /// The loaded row lacks a value for a key column.
    #[error("the row has no value for key column `{0}`")]
    MissingKey(String),
    /// An insert without any value.
    #[error("an inserted row needs at least one value")]
    EmptyInsert,
}

/// What can be edited in a table: its name, row identity and columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditTarget {
    /// Table name.
    pub table: String,
    /// Dialect used for quoting and comparison.
    pub dialect: Dialect,
    /// Columns identifying a row.
    pub key: Vec<String>,
    columns: Vec<String>,
}

impl EditTarget {
    /// Checks that `table` can be edited and picks the row identity: the
    /// primary key, else the first unique index whose columns are NOT NULL.
    pub fn new(table: &Table, dialect: Dialect) -> Result<Self, EditError> {
        if table.kind != "table" {
            return Err(EditError::NotATable(table.name.clone(), table.kind.clone()));
        }
        let not_null = |name: &String| table.columns.iter().any(|c| &c.name == name && !c.nullable);
        let key = if !table.primary_key.is_empty() {
            table.primary_key.clone()
        } else {
            table
                .indexes
                .iter()
                .find(|i| i.is_key() && !i.columns.is_empty() && i.columns.iter().all(not_null))
                .map(|i| i.columns.clone())
                .ok_or_else(|| EditError::NoRowIdentity(table.name.clone()))?
        };
        Ok(Self {
            table: table.name.clone(),
            dialect,
            key,
            columns: table.columns.iter().map(|c| c.name.clone()).collect(),
        })
    }

    /// Whether the table has column `name`.
    pub fn has_column(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c == name)
    }

    fn check_columns<'a>(
        &self,
        names: impl IntoIterator<Item = &'a String>,
    ) -> Result<(), EditError> {
        for name in names {
            if !self.columns.contains(name) {
                return Err(EditError::UnknownColumn {
                    table: self.table.clone(),
                    column: name.clone(),
                });
            }
        }
        Ok(())
    }

    fn key_of(&self, row: &Values) -> Result<Values, EditError> {
        self.check_columns(row.iter().map(|(c, _)| c))?;
        self.key
            .iter()
            .map(|k| match row.iter().find(|(c, _)| c == k) {
                Some((_, Some(v))) => Ok((k.clone(), Some(v.clone()))),
                _ => Err(EditError::MissingKey(k.clone())),
            })
            .collect()
    }
}

/// One pending change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Change cells of one row.
    Update {
        /// Row identity.
        key: Values,
        /// Changed columns: (column, value loaded, new value).
        cells: Vec<(String, Option<String>, Option<String>)>,
    },
    /// Add a row with these values; other columns get their defaults.
    Insert {
        /// Column values.
        values: Values,
    },
    /// Remove one row.
    Delete {
        /// Row identity.
        key: Values,
        /// Values loaded for the other columns, checked before deleting.
        loaded: Values,
    },
}

/// Changes collected for one table, not yet applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSet {
    target: EditTarget,
    changes: Vec<Change>,
}

impl EditSet {
    /// An empty set of changes for `target`.
    pub fn new(target: EditTarget) -> Self {
        Self {
            target,
            changes: Vec::new(),
        }
    }

    /// The pending changes in order.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// Sets `column` of the loaded `row` to `value`. Cells of the same row are
    /// merged into one change; setting a cell twice keeps the first loaded value.
    pub fn set_cell(
        &mut self,
        row: &Values,
        column: &str,
        value: Option<String>,
    ) -> Result<(), EditError> {
        let key = self.target.key_of(row)?;
        let column = column.to_owned();
        self.target.check_columns([&column])?;
        let loaded = row
            .iter()
            .find(|(c, _)| *c == column)
            .and_then(|(_, v)| v.clone());
        for change in &mut self.changes {
            if let Change::Update { key: k, cells } = change
                && *k == key
            {
                match cells.iter_mut().find(|(c, _, _)| *c == column) {
                    Some(cell) => cell.2 = value,
                    None => cells.push((column, loaded, value)),
                }
                return Ok(());
            }
        }
        self.changes.push(Change::Update {
            key,
            cells: vec![(column, loaded, value)],
        });
        Ok(())
    }

    /// Adds a row with `values`.
    pub fn insert_row(&mut self, values: Values) -> Result<(), EditError> {
        if values.is_empty() {
            return Err(EditError::EmptyInsert);
        }
        self.target.check_columns(values.iter().map(|(c, _)| c))?;
        self.changes.push(Change::Insert { values });
        Ok(())
    }

    /// Deletes the loaded `row`; pending updates of that row are dropped.
    pub fn delete_row(&mut self, row: &Values) -> Result<(), EditError> {
        let key = self.target.key_of(row)?;
        let mut loaded: Values = row
            .iter()
            .filter(|(c, _)| !self.target.key.contains(c))
            .cloned()
            .collect();
        self.changes.retain(|change| match change {
            Change::Update { key: k, cells } if *k == key => {
                // Compare against what was loaded, not against the pending edit.
                for (column, old, _) in cells {
                    if let Some(entry) = loaded.iter_mut().find(|(c, _)| c == column) {
                        entry.1 = old.clone();
                    }
                }
                false
            }
            _ => true,
        });
        self.changes.push(Change::Delete { key, loaded });
        Ok(())
    }

    /// The statements that applying will run, one per change, in order.
    pub fn sql(&self) -> Vec<String> {
        self.changes.iter().map(|c| self.statement(c)).collect()
    }

    fn statement(&self, change: &Change) -> String {
        let d = self.target.dialect;
        let table = quote_identifier(&self.target.table, d);
        match change {
            Change::Update { key, cells } => {
                let set: Vec<String> = cells
                    .iter()
                    .map(|(c, _, new)| format!("{} = {}", quote_identifier(c, d), literal(new, d)))
                    .collect();
                let mut conditions = key_conditions(key, d);
                conditions.extend(
                    cells
                        .iter()
                        .filter(|(c, ..)| !self.target.key.contains(c))
                        .map(|(c, old, _)| same_value(c, old, d)),
                );
                format!(
                    "UPDATE {table} SET {} WHERE {}",
                    set.join(", "),
                    conditions.join(" AND ")
                )
            }
            Change::Insert { values } => {
                let columns: Vec<String> =
                    values.iter().map(|(c, _)| quote_identifier(c, d)).collect();
                let literals: Vec<String> = values.iter().map(|(_, v)| literal(v, d)).collect();
                format!(
                    "INSERT INTO {table} ({}) VALUES ({})",
                    columns.join(", "),
                    literals.join(", ")
                )
            }
            Change::Delete { key, loaded } => {
                let mut conditions = key_conditions(key, d);
                conditions.extend(loaded.iter().map(|(c, old)| same_value(c, old, d)));
                format!("DELETE FROM {table} WHERE {}", conditions.join(" AND "))
            }
        }
    }

    /// Applies all changes in one transaction. Each statement must change
    /// exactly one row; otherwise, or on an error, everything is rolled back.
    pub async fn apply(&self, session: &Session) -> Applied {
        if let Err(error) = run_one(session, begin_sql(session.dialect())).await {
            return Applied::RolledBack {
                index: None,
                failure: Failure::Error(error),
            };
        }
        for (index, statement) in self.sql().iter().enumerate() {
            let failure = match run_one(session, statement).await {
                Ok(1) => continue,
                Ok(count) => Failure::Conflict(count),
                Err(error) => Failure::Error(error),
            };
            // A failed rollback leaves nothing committed either: the
            // transaction ends with the session.
            let _ = run_one(session, "ROLLBACK").await;
            return Applied::RolledBack {
                index: Some(index),
                failure,
            };
        }
        match run_one(session, "COMMIT").await {
            Ok(_) => Applied::Committed(self.changes.len()),
            Err(error) => {
                let _ = run_one(session, "ROLLBACK").await;
                Applied::RolledBack {
                    index: None,
                    failure: Failure::Error(error),
                }
            }
        }
    }
}

impl EditSet {
    /// Applies all changes inside the session's open transaction behind the savepoint `name`
    /// (`SPEC-0190` behaviour 9): on a conflict or error only this batch is rolled back to the
    /// savepoint; nothing is committed.
    pub async fn apply_in_transaction(&self, session: &Session, name: &str) -> Applied {
        let d = session.dialect();
        let (save, undo, release) = savepoint_sql(d, name);
        if let Err(error) = run_one(session, &save).await {
            return Applied::RolledBack {
                index: None,
                failure: Failure::Error(error),
            };
        }
        for (index, statement) in self.sql().iter().enumerate() {
            let failure = match run_one(session, statement).await {
                Ok(1) => continue,
                Ok(count) => Failure::Conflict(count),
                Err(error) => Failure::Error(error),
            };
            let _ = run_one(session, &undo).await;
            return Applied::RolledBack {
                index: Some(index),
                failure,
            };
        }
        if let Some(release) = release {
            let _ = run_one(session, &release).await;
        }
        Applied::Committed(self.changes.len())
    }
}

/// The statement that starts a transaction in `dialect`.
pub fn begin_sql(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::MySql | Dialect::MariaDb => "START TRANSACTION",
        Dialect::SqlServer => "BEGIN TRANSACTION",
        // Oracle starts transactions implicitly; this names the start (SPEC-0135).
        Dialect::Oracle => "SET TRANSACTION READ WRITE",
        Dialect::PostgreSql | Dialect::Sqlite | Dialect::DuckDb => "BEGIN",
    }
}

/// Isolation level of a transaction a data editor opens (`SPEC-0190` v3 behaviour 13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Isolation {
    /// Whatever the database uses by default.
    #[default]
    Default,
    /// `READ COMMITTED`.
    ReadCommitted,
    /// `REPEATABLE READ`.
    RepeatableRead,
    /// `SERIALIZABLE`.
    Serializable,
}

impl Isolation {
    /// Every level, in menu order.
    pub const ALL: [Isolation; 4] = [
        Isolation::Default,
        Isolation::ReadCommitted,
        Isolation::RepeatableRead,
        Isolation::Serializable,
    ];

    /// Menu text.
    pub fn label(self) -> &'static str {
        match self {
            Isolation::Default => "Database default",
            Isolation::ReadCommitted => "Read committed",
            Isolation::RepeatableRead => "Repeatable read",
            Isolation::Serializable => "Serializable",
        }
    }

    fn sql(self) -> Option<&'static str> {
        match self {
            Isolation::Default => None,
            Isolation::ReadCommitted => Some("READ COMMITTED"),
            Isolation::RepeatableRead => Some("REPEATABLE READ"),
            Isolation::Serializable => Some("SERIALIZABLE"),
        }
    }
}

/// The statements that open a transaction at `isolation` in `dialect` (`SPEC-0190` v3 behaviour
/// 13), or why that level is not available there. MySQL, MariaDB and SQL Server set the level for
/// the next transaction before starting it; PostgreSQL and Oracle set it as the transaction's first
/// statement; SQLite and DuckDB transactions are always serializable.
pub fn begin_statements(dialect: Dialect, isolation: Isolation) -> Result<Vec<String>, String> {
    let begin = begin_sql(dialect).to_owned();
    let Some(level) = isolation.sql() else {
        return Ok(vec![begin]);
    };
    let set = format!("SET TRANSACTION ISOLATION LEVEL {level}");
    Ok(match dialect {
        Dialect::MySql | Dialect::MariaDb | Dialect::SqlServer => vec![set, begin],
        Dialect::PostgreSql => vec![begin, set],
        Dialect::Oracle => match isolation {
            Isolation::RepeatableRead => {
                return Err("Oracle offers read committed and serializable only".into());
            }
            _ => vec![set],
        },
        Dialect::Sqlite | Dialect::DuckDb => match isolation {
            Isolation::Serializable => vec![begin],
            _ => {
                return Err(format!(
                    "{} transactions are always serializable",
                    dialect.label()
                ));
            }
        },
    })
}

/// Setting, rolling back to and releasing savepoint `name` (no release in SQL Server and Oracle).
fn savepoint_sql(d: Dialect, name: &str) -> (String, String, Option<String>) {
    match d {
        Dialect::SqlServer => (
            format!("SAVE TRANSACTION {name}"),
            format!("ROLLBACK TRANSACTION {name}"),
            None,
        ),
        Dialect::Oracle => (
            format!("SAVEPOINT {name}"),
            format!("ROLLBACK TO SAVEPOINT {name}"),
            None,
        ),
        _ => (
            format!("SAVEPOINT {name}"),
            format!("ROLLBACK TO SAVEPOINT {name}"),
            Some(format!("RELEASE SAVEPOINT {name}")),
        ),
    }
}

/// The result of [`EditSet::apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// All changes were committed; the number of changes.
    Committed(usize),
    /// Nothing was changed.
    RolledBack {
        /// The change that failed; `None` for BEGIN or COMMIT.
        index: Option<usize>,
        /// Why.
        failure: Failure,
    },
}

/// Why applying stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The statement changed this many rows instead of one: the row was
    /// changed or deleted since it was loaded (or the key is not unique).
    Conflict(u64),
    /// The database reported an error.
    Error(DbError),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(0) => f.write_str("the row was changed or deleted since it was loaded"),
            Self::Conflict(n) => write!(f, "{n} rows matched instead of one"),
            Self::Error(e) => e.fmt(f),
        }
    }
}

pub(crate) async fn run_one(session: &Session, sql: &str) -> Result<u64, DbError> {
    let run = session.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::UpdateCount(n)) => Ok(n),
        Some(Outcome::ResultSet { rows, .. }) => Ok(rows.len() as u64),
        Some(Outcome::Error(e)) => Err(e),
        None => Err(DbError {
            message: "no statement was run".into(),
            ..DbError::default()
        }),
    }
}

/// A string literal for `dialect`, or `NULL`.
pub fn literal(value: &Option<String>, dialect: Dialect) -> String {
    let Some(value) = value else {
        return "NULL".into();
    };
    let mut out = String::with_capacity(value.len() + 3);
    // T-SQL needs `N'…'` to keep characters outside the column's code page.
    if dialect == Dialect::SqlServer {
        out.push('N');
    }
    out.push('\'');
    for c in value.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' if dialect.mysql_like() => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

fn key_conditions(key: &Values, d: Dialect) -> Vec<String> {
    key.iter()
        .map(|(c, v)| format!("{} = {}", quote_identifier(c, d), literal(v, d)))
        .collect()
}

/// NULL-safe equality with the loaded value.
fn same_value(column: &str, old: &Option<String>, d: Dialect) -> String {
    let op = match d {
        Dialect::PostgreSql | Dialect::DuckDb => "IS NOT DISTINCT FROM",
        Dialect::MySql | Dialect::MariaDb => "<=>",
        Dialect::Sqlite => "IS",
        // `IS NOT DISTINCT FROM` needs SQL Server 2022; the loaded value is known, so plain
        // `=` or `IS NULL` says the same on every version.
        Dialect::SqlServer if old.is_none() => {
            return format!("{} IS NULL", quote_identifier(column, d));
        }
        Dialect::SqlServer => "=",
        // Oracle has no NULL-safe operator; `DECODE` treats two NULLs as equal (SPEC-0135).
        Dialect::Oracle => {
            return format!(
                "DECODE({}, {}, 1, 0) = 1",
                quote_identifier(column, d),
                literal(old, d)
            );
        }
    };
    format!("{} {op} {}", quote_identifier(column, d), literal(old, d))
}

/// The table a result can be edited in (`SPEC-0107` behaviour 1): the single table of a plain
/// `SELECT … FROM table [alias] [WHERE …] [ORDER BY …] [LIMIT …]`. Joins, several tables,
/// subqueries in `FROM`, grouping, `DISTINCT` and set operations give `None`. A schema prefix is
/// dropped and quotes are removed.
pub fn editable_source(sql: &str, dialect: crate::Dialect) -> Option<String> {
    use crate::completion::{Tok, tokenize};
    let tokens = tokenize(sql, 0, dialect)?;
    let text = |i: usize| &sql[tokens[i].range.clone()];
    let word = |i: usize| {
        tokens
            .get(i)
            .filter(|t| t.kind == Tok::Word)
            .map(|t| sql[t.range.clone()].to_ascii_lowercase())
    };
    if word(0).as_deref() != Some("select") {
        return None;
    }
    const REFUSED: [&str; 9] = [
        "join",
        "group",
        "union",
        "intersect",
        "except",
        "distinct",
        "having",
        "with",
        "into",
    ];
    if (0..tokens.len()).any(|i| word(i).is_some_and(|w| REFUSED.contains(&w.as_str()))) {
        return None;
    }
    let from = (0..tokens.len()).find(|&i| word(i).as_deref() == Some("from"))?;
    // The table name is read from the text: the tokens leave quoted identifiers out.
    let (name, end) = table_name(sql, tokens[from].range.end)?;
    let mut at = (from + 1..tokens.len())
        .find(|&i| tokens[i].range.start >= end)
        .unwrap_or(tokens.len());
    const AFTER: [&str; 6] = ["where", "order", "limit", "offset", "fetch", "for"];
    match word(at).as_deref() {
        Some("as") => at += 2,
        Some(w) if !AFTER.contains(&w) => at += 1,
        _ => {}
    }
    match tokens.get(at) {
        None => {}
        Some(t) if t.kind == Tok::Word && AFTER.contains(&word(at)?.as_str()) => {}
        Some(t) if t.kind == Tok::Other && text(at).trim() == ";" && at + 1 == tokens.len() => {}
        _ => return None,
    }
    if tokens[from..]
        .iter()
        .any(|t| t.kind == Tok::Other && sql[t.range.clone()].contains('('))
    {
        return None;
    }
    Some(name)
}

/// One identifier at `at` (after spaces): plain, or in `"…"`, `` `…` `` or `[…]`; returns its
/// unquoted text and the byte after it.
fn identifier(sql: &str, at: usize) -> Option<(String, usize)> {
    let start = at + sql[at..].len() - sql[at..].trim_start().len();
    let rest = &sql[start..];
    let close = match rest.chars().next()? {
        '"' => Some('"'),
        '`' => Some('`'),
        '[' => Some(']'),
        _ => None,
    };
    match close {
        Some(close) => {
            let len = rest[1..].find(close)?;
            Some((rest[1..1 + len].to_owned(), start + len + 2))
        }
        None => {
            let len = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(rest.len());
            (len > 0).then(|| (rest[..len].to_owned(), start + len))
        }
    }
}

/// The table after `FROM` with an optional schema prefix, which is dropped.
fn table_name(sql: &str, at: usize) -> Option<(String, usize)> {
    let (first, end) = identifier(sql, at)?;
    match sql[end..].strip_prefix('.') {
        Some(_) => identifier(sql, end + 1),
        None => Some((first, end)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn isolation_levels_per_dialect() {
        use crate::Dialect::{MySql, Oracle, PostgreSql, SqlServer, Sqlite};
        use crate::{Isolation, begin_statements};
        assert_eq!(
            begin_statements(PostgreSql, Isolation::Default).unwrap(),
            ["BEGIN"]
        );
        assert_eq!(
            begin_statements(PostgreSql, Isolation::RepeatableRead).unwrap(),
            ["BEGIN", "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"]
        );
        assert_eq!(
            begin_statements(MySql, Isolation::Serializable).unwrap(),
            [
                "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
                "START TRANSACTION"
            ]
        );
        assert_eq!(
            begin_statements(SqlServer, Isolation::ReadCommitted).unwrap()[0],
            "SET TRANSACTION ISOLATION LEVEL READ COMMITTED"
        );
        assert_eq!(
            begin_statements(Oracle, Isolation::Serializable).unwrap(),
            ["SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"]
        );
        assert!(begin_statements(Oracle, Isolation::RepeatableRead).is_err());
        assert_eq!(
            begin_statements(Sqlite, Isolation::Serializable).unwrap(),
            ["BEGIN"]
        );
        assert!(
            begin_statements(Sqlite, Isolation::ReadCommitted)
                .unwrap_err()
                .contains("always serializable")
        );
        assert_eq!(Isolation::ALL.len(), 4);
        assert_eq!(Isolation::RepeatableRead.label(), "Repeatable read");
    }

    #[test]
    fn editable_source_accepts_plain_selects_only() {
        use crate::Dialect::{MySql, PostgreSql, Sqlite};
        let src = |sql: &str| editable_source(sql, PostgreSql);
        assert_eq!(src("SELECT * FROM users").as_deref(), Some("users"));
        assert_eq!(
            src("select id, name from public.users u where id > 3 order by name limit 10;")
                .as_deref(),
            Some("users")
        );
        assert_eq!(
            src(r#"SELECT * FROM "Order Items" AS oi"#).as_deref(),
            Some("Order Items")
        );
        assert_eq!(
            editable_source("SELECT * FROM `t`", MySql).as_deref(),
            Some("t")
        );
        assert_eq!(
            editable_source("SELECT count(*) FROM t", Sqlite).as_deref(),
            Some("t"),
            "the key check rejects it later"
        );
        for sql in [
            "SELECT * FROM a JOIN b ON a.id = b.a",
            "SELECT * FROM a, b",
            "SELECT * FROM (SELECT * FROM a) x",
            "SELECT DISTINCT name FROM a",
            "SELECT name, count(*) FROM a GROUP BY name",
            "SELECT 1",
            "UPDATE a SET x = 1",
            "SELECT * FROM a UNION SELECT * FROM b",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "SELECT * FROM a WHERE id IN (SELECT id FROM b)",
        ] {
            assert_eq!(src(sql), None, "{sql}");
        }
    }

    use super::*;
    use crate::{Column, Index};

    fn column(name: &str, nullable: bool) -> Column {
        Column {
            name: name.into(),
            data_type: "text".into(),
            nullable,
            default: None,
            generated: None,
            expression: None,
            identity: None,
            comment: None,
        }
    }

    fn item() -> Table {
        Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: "item".into(),
            kind: "table".into(),
            columns: vec![
                column("sku", false),
                column("name", false),
                column("note", true),
            ],
            primary_key: vec!["sku".into()],
            foreign_keys: vec![],
            indexes: vec![],
            unreadable: None,
            constraints: Vec::new(),
            comment: None,
        }
    }

    fn row(pairs: &[(&str, Option<&str>)]) -> Values {
        pairs
            .iter()
            .map(|(c, v)| (c.to_string(), v.map(str::to_owned)))
            .collect()
    }

    fn loaded() -> Values {
        row(&[("sku", Some("A-1")), ("name", Some("Bolt")), ("note", None)])
    }

    fn set(dialect: Dialect) -> EditSet {
        EditSet::new(EditTarget::new(&item(), dialect).unwrap())
    }

    #[test]
    fn two_cells_of_one_row_give_one_update() {
        let mut s = set(Dialect::PostgreSql);
        s.set_cell(&loaded(), "name", Some("Bolt M6".into()))
            .unwrap();
        s.set_cell(&loaded(), "note", Some("new".into())).unwrap();
        assert_eq!(
            s.sql(),
            [
                "UPDATE \"item\" SET \"name\" = 'Bolt M6', \"note\" = 'new' WHERE \"sku\" = 'A-1' \
                 AND \"name\" IS NOT DISTINCT FROM 'Bolt' AND \"note\" IS NOT DISTINCT FROM NULL"
            ]
        );
    }

    #[test]
    fn dialects_use_their_quoting_and_null_safe_comparison() {
        let mut m = set(Dialect::MySql);
        m.set_cell(&loaded(), "note", None).unwrap();
        assert_eq!(
            m.sql(),
            ["UPDATE `item` SET `note` = NULL WHERE `sku` = 'A-1' AND `note` <=> NULL"]
        );
        let mut l = set(Dialect::Sqlite);
        l.set_cell(&loaded(), "note", Some("x".into())).unwrap();
        assert_eq!(
            l.sql(),
            ["UPDATE \"item\" SET \"note\" = 'x' WHERE \"sku\" = 'A-1' AND \"note\" IS NULL"]
        );
    }

    #[test]
    fn t_sql_uses_brackets_n_literals_and_plain_comparison() {
        let mut m = set(Dialect::SqlServer);
        m.set_cell(&loaded(), "note", Some("Grüße".into())).unwrap();
        m.set_cell(&loaded(), "name", None).unwrap();
        assert_eq!(
            m.sql(),
            [
                "UPDATE [item] SET [note] = N'Grüße', [name] = NULL WHERE [sku] = N'A-1' \
              AND [note] IS NULL AND [name] = N'Bolt'"
            ]
        );
        let mut d = set(Dialect::MariaDb);
        d.set_cell(&loaded(), "note", None).unwrap();
        assert_eq!(
            d.sql(),
            ["UPDATE `item` SET `note` = NULL WHERE `sku` = 'A-1' AND `note` <=> NULL"]
        );
    }

    #[test]
    fn oracle_compares_with_decode() {
        let mut o = set(Dialect::Oracle);
        o.set_cell(&loaded(), "note", Some("x".into())).unwrap();
        assert_eq!(
            o.sql(),
            ["UPDATE \"item\" SET \"note\" = 'x' WHERE \"sku\" = 'A-1' \
              AND DECODE(\"note\", NULL, 1, 0) = 1"]
        );
    }

    #[test]
    fn setting_a_cell_twice_keeps_the_loaded_value() {
        let mut s = set(Dialect::Sqlite);
        s.set_cell(&loaded(), "name", Some("a".into())).unwrap();
        s.set_cell(&loaded(), "name", Some("b".into())).unwrap();
        assert_eq!(
            s.sql(),
            ["UPDATE \"item\" SET \"name\" = 'b' WHERE \"sku\" = 'A-1' AND \"name\" IS 'Bolt'"]
        );
    }

    #[test]
    fn literals_escape_per_dialect() {
        let v = Some(r"it's C:\tmp".to_owned());
        assert_eq!(literal(&v, Dialect::PostgreSql), r"'it''s C:\tmp'");
        assert_eq!(literal(&v, Dialect::Sqlite), r"'it''s C:\tmp'");
        assert_eq!(literal(&v, Dialect::MySql), r"'it''s C:\\tmp'");
        assert_eq!(literal(&None, Dialect::MySql), "NULL");
        assert_eq!(literal(&v, Dialect::MariaDb), r"'it''s C:\\tmp'");
        assert_eq!(literal(&v, Dialect::SqlServer), r"N'it''s C:\tmp'");
    }

    #[test]
    fn insert_and_delete() {
        let mut s = set(Dialect::PostgreSql);
        s.insert_row(row(&[("sku", Some("C-3")), ("name", Some("Washer"))]))
            .unwrap();
        s.set_cell(&loaded(), "name", Some("ignored".into()))
            .unwrap();
        s.delete_row(&loaded()).unwrap();
        assert_eq!(
            s.sql(),
            [
                "INSERT INTO \"item\" (\"sku\", \"name\") VALUES ('C-3', 'Washer')",
                "DELETE FROM \"item\" WHERE \"sku\" = 'A-1' AND \"name\" IS NOT DISTINCT FROM \
                 'Bolt' AND \"note\" IS NOT DISTINCT FROM NULL",
            ]
        );
    }

    #[test]
    fn row_identity_falls_back_to_a_not_null_unique_index() {
        let mut t = item();
        t.primary_key.clear();
        t.indexes.push(Index {
            name: "item_note".into(),
            unique: true,
            columns: vec!["note".into()],
            condition: None,
            collations: Vec::new(),
            method: None,
            orders: Vec::new(),
            prefixes: Vec::new(),
        });
        assert_eq!(
            EditTarget::new(&t, Dialect::Sqlite).unwrap_err(),
            EditError::NoRowIdentity("item".into())
        );
        t.indexes.push(Index {
            name: "item_name".into(),
            unique: true,
            columns: vec!["name".into()],
            condition: None,
            collations: Vec::new(),
            method: None,
            orders: Vec::new(),
            prefixes: Vec::new(),
        });
        assert_eq!(EditTarget::new(&t, Dialect::Sqlite).unwrap().key, ["name"]);
    }

    /// SPEC-0013 v4 AK-10: a unique index over some rows only does not identify every row.
    #[test]
    fn a_partial_unique_index_is_no_row_identity() {
        let mut t = item();
        t.primary_key.clear();
        t.indexes.push(Index {
            name: "item_name_live".into(),
            unique: true,
            columns: vec!["name".into()],
            condition: Some("deleted = 0".into()),
            collations: Vec::new(),
            method: None,
            orders: Vec::new(),
            prefixes: Vec::new(),
        });
        assert_eq!(
            EditTarget::new(&t, Dialect::Sqlite).unwrap_err(),
            EditError::NoRowIdentity("item".into())
        );
    }

    #[test]
    fn refuses_views_unknown_columns_and_missing_keys() {
        let mut v = item();
        v.kind = "view".into();
        assert!(matches!(
            EditTarget::new(&v, Dialect::Sqlite),
            Err(EditError::NotATable(..))
        ));
        let mut s = set(Dialect::Sqlite);
        assert!(matches!(
            s.set_cell(&loaded(), "nope", None),
            Err(EditError::UnknownColumn { .. })
        ));
        assert_eq!(
            s.set_cell(&row(&[("name", Some("x"))]), "name", None),
            Err(EditError::MissingKey("sku".into()))
        );
        assert_eq!(s.insert_row(vec![]), Err(EditError::EmptyInsert));
    }
}
