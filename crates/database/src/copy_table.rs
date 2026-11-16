//! Copying a table to another schema or name (`SPEC-0191`): the statements with the kept
//! constraints, reading the source fresh from the database, and running the copy.

use serde::Serialize;

use crate::ddl::default_expr;
use crate::edit::run_one;
use crate::{DbError, Dialect, OnError, Outcome, Session, Table, introspect, quote_identifier};

/// What the copy keeps (behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CopyOptions {
    /// The primary key.
    pub primary_key: bool,
    /// Unique constraints, from the unique indexes.
    pub unique: bool,
    /// NOT NULL columns.
    pub not_null: bool,
    /// Column defaults.
    pub defaults: bool,
    /// CHECK constraints.
    pub checks: bool,
    /// Foreign keys to the same referenced tables (off by default).
    pub foreign_keys: bool,
    /// The rows.
    pub rows: bool,
}

impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            primary_key: true,
            unique: true,
            not_null: true,
            defaults: true,
            checks: true,
            foreign_keys: false,
            rows: true,
        }
    }
}

/// The table to copy, as read from the database (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CopySource {
    /// Structure.
    pub table: Table,
    /// CHECK constraints as `CHECK (…)` clauses.
    pub checks: Vec<String>,
    /// Columns that number themselves (MySQL `AUTO_INCREMENT`, SQL Server `IDENTITY`).
    pub identity: Vec<String>,
}

/// Where the copy goes (behaviour 2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CopyTarget {
    /// Schema; `None` or empty for the source's schema.
    pub schema: Option<String>,
    /// Table name.
    pub table: String,
}

impl CopyTarget {
    /// The default target for `table` in `schema` (behaviour 2): `<table>_copy` in the same
    /// schema, else the same name.
    pub fn default_for(table: &str, schema: Option<&str>) -> Self {
        match schema.filter(|s| !s.trim().is_empty()) {
            Some(s) => Self {
                schema: Some(s.trim().to_owned()),
                table: table.to_owned(),
            },
            None => Self {
                schema: None,
                table: format!("{table}_copy"),
            },
        }
    }

    /// The quoted, possibly qualified name.
    pub fn name(&self, d: Dialect) -> String {
        match self.schema.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() => {
                format!(
                    "{}.{}",
                    quote_identifier(s, d),
                    quote_identifier(&self.table, d)
                )
            }
            _ => quote_identifier(&self.table, d),
        }
    }
}

fn list(columns: &[String], d: Dialect) -> String {
    columns
        .iter()
        .map(|c| quote_identifier(c, d))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The statements of the copy (behaviour 5): `CREATE TABLE` with the kept constraints, then the
/// `INSERT … SELECT` when rows are copied.
pub fn copy_statements(
    source: &CopySource,
    target: &CopyTarget,
    options: CopyOptions,
    d: Dialect,
) -> Vec<String> {
    let t = &source.table;
    let mut parts: Vec<String> = t
        .columns
        .iter()
        .map(|c| {
            let mut def = format!("{} {}", quote_identifier(&c.name, d), c.data_type);
            let identity = source.identity.contains(&c.name);
            if identity && d == Dialect::SqlServer {
                def.push_str(" IDENTITY(1,1)");
            }
            if options.not_null && !c.nullable {
                def.push_str(" NOT NULL");
            }
            if options.defaults
                && let Some(default) = &c.default
            {
                def.push_str(" DEFAULT ");
                def.push_str(&default_expr(default, d));
            }
            if identity && d.mysql_like() {
                def.push_str(" AUTO_INCREMENT");
            }
            def
        })
        .collect();
    // MySQL needs a key on an AUTO_INCREMENT column, so the primary key stays with it.
    let keep_key = options.primary_key || (d.mysql_like() && !source.identity.is_empty());
    if keep_key && !t.primary_key.is_empty() {
        parts.push(format!("PRIMARY KEY ({})", list(&t.primary_key, d)));
    }
    if options.unique {
        let column = |c: &String| t.columns.iter().any(|col| &col.name == c);
        let mut seen: Vec<&Vec<String>> = Vec::new();
        for i in t.indexes.iter().filter(|i| {
            i.is_key()
                && i.name != "PRIMARY"
                && i.columns != t.primary_key
                && i.columns.iter().all(column)
        }) {
            if !seen.contains(&&i.columns) {
                seen.push(&i.columns);
                parts.push(format!("UNIQUE ({})", list(&i.columns, d)));
            }
        }
    }
    if options.checks {
        parts.extend(source.checks.iter().cloned());
    }
    if options.foreign_keys {
        parts.extend(t.foreign_keys.iter().map(|f| {
            format!(
                "FOREIGN KEY ({}) REFERENCES {} ({})",
                list(&f.columns, d),
                quote_identifier(&f.referenced_table, d),
                list(&f.referenced_columns, d)
            )
        }));
    }
    let name = target.name(d);
    let mut out = vec![format!(
        "CREATE TABLE {name} (\n    {}\n)",
        parts.join(",\n    ")
    )];
    if options.rows {
        let columns: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
        let insert = format!(
            "INSERT INTO {name} ({0}) SELECT {0} FROM {1}",
            list(&columns, d),
            quote_identifier(&t.name, d)
        );
        if d == Dialect::SqlServer && !source.identity.is_empty() {
            out.push(format!("SET IDENTITY_INSERT {name} ON"));
            out.push(insert);
            out.push(format!("SET IDENTITY_INSERT {name} OFF"));
        } else {
            out.push(insert);
        }
    }
    out
}

/// A note for the preview when defaults draw from the source's sequence (behaviour 3).
pub fn sequence_note(source: &CopySource, options: CopyOptions) -> Option<String> {
    let shared: Vec<&str> = source
        .table
        .columns
        .iter()
        .filter(|c| {
            options.defaults
                && c.default
                    .as_deref()
                    .is_some_and(|d| d.trim_start().starts_with("nextval("))
        })
        .map(|c| c.name.as_str())
        .collect();
    (!shared.is_empty()).then(|| {
        format!(
            "{} keep(s) the default of `{}`: the copy draws from the same sequence",
            shared.join(", "),
            source.table.name
        )
    })
}

/// The `CHECK (…)` clauses in SQLite's `CREATE TABLE` text: every `CHECK` keyword outside quotes
/// with its parenthesised condition (AK-1).
pub fn sqlite_checks(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '\'' | '"' | '`' | '[') {
            let close = if c == '[' { ']' } else { c };
            i += 1;
            while i < chars.len() && chars[i] != close {
                i += 1;
            }
            i += 1;
            continue;
        }
        let word_start = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        let is_check = word_start
            && chars.len() >= i + 5
            && chars[i..i + 5]
                .iter()
                .collect::<String>()
                .eq_ignore_ascii_case("check")
            && chars
                .get(i + 5)
                .is_none_or(|n| !(n.is_alphanumeric() || *n == '_'));
        if !is_check {
            i += 1;
            continue;
        }
        let mut j = i + 5;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if chars.get(j) != Some(&'(') {
            i += 5;
            continue;
        }
        let start = j;
        let mut depth = 0;
        let mut quote: Option<char> = None;
        while j < chars.len() {
            let ch = chars[j];
            match quote {
                Some(q) if ch == q => quote = None,
                Some(_) => {}
                None => match ch {
                    '\'' | '"' | '`' => quote = Some(ch),
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                },
            }
            j += 1;
        }
        let condition: String = chars[start..=j.min(chars.len() - 1)].iter().collect();
        out.push(format!("CHECK {condition}"));
        i = j + 1;
    }
    out
}

async fn rows(session: &Session, sql: &str) -> Result<Vec<Vec<Option<String>>>, DbError> {
    let run = session.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { rows, .. }) => Ok(rows),
        Some(Outcome::Error(e)) => Err(e),
        _ => Ok(Vec::new()),
    }
}

/// Reads `table` with its CHECK constraints and self-numbering columns from the database
/// (behaviour 4).
pub async fn read_source(session: &Session, table: &str) -> Result<CopySource, DbError> {
    let d = session.dialect();
    let found = introspect(session)
        .await?
        .into_iter()
        .find(|t| t.name == table && t.kind == "table")
        .ok_or_else(|| DbError {
            message: format!("table `{table}` not found"),
            ..DbError::default()
        })?;
    let name = crate::literal(&Some(table.to_owned()), d);
    let first = |rows: Vec<Vec<Option<String>>>| -> Vec<String> {
        rows.into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect()
    };
    let (checks, identity) = match d {
        Dialect::PostgreSql => (
            first(
                rows(
                    session,
                    &format!(
                        "select pg_get_constraintdef(con.oid) from pg_constraint con \
                         join pg_class c on c.oid = con.conrelid \
                         join pg_namespace n on n.oid = c.relnamespace \
                         where con.contype = 'c' and c.relname = {name} \
                         and n.nspname = current_schema() order by con.conname"
                    ),
                )
                .await?,
            ),
            Vec::new(),
        ),
        Dialect::MySql | Dialect::MariaDb => {
            let checks = rows(
                session,
                &format!(
                    "select cc.CHECK_CLAUSE from information_schema.TABLE_CONSTRAINTS tc \
                     join information_schema.CHECK_CONSTRAINTS cc \
                       on cc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA \
                      and cc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
                     where tc.CONSTRAINT_TYPE = 'CHECK' and tc.TABLE_SCHEMA = database() \
                       and tc.TABLE_NAME = {name} order by tc.CONSTRAINT_NAME"
                ),
            )
            .await
            .map(first)
            .unwrap_or_default();
            let identity = first(
                rows(
                    session,
                    &format!(
                        "select COLUMN_NAME from information_schema.COLUMNS \
                         where TABLE_SCHEMA = database() and TABLE_NAME = {name} \
                         and EXTRA like '%auto_increment%'"
                    ),
                )
                .await?,
            );
            (
                checks
                    .into_iter()
                    .map(|c| format!("CHECK ({})", strip_outer(&c)))
                    .collect(),
                identity,
            )
        }
        Dialect::SqlServer => (
            first(
                rows(
                    session,
                    &format!(
                        "select 'CHECK ' + definition from sys.check_constraints \
                         where parent_object_id = object_id({name}) order by name"
                    ),
                )
                .await?,
            ),
            first(
                rows(
                    session,
                    &format!(
                        "select name from sys.identity_columns where object_id = object_id({name})"
                    ),
                )
                .await?,
            ),
        ),
        Dialect::Oracle => (
            first(
                rows(
                    session,
                    &format!(
                        "select search_condition_vc from user_constraints \
                         where constraint_type = 'C' and table_name = {name} \
                         and generated = 'USER NAME' order by constraint_name"
                    ),
                )
                .await?,
            )
            .into_iter()
            .map(|c| format!("CHECK ({c})"))
            .collect(),
            Vec::new(),
        ),
        Dialect::DuckDb => (
            first(
                rows(
                    session,
                    &format!(
                        "select constraint_text from duckdb_constraints() \
                         where constraint_type = 'CHECK' and table_name = {name} \
                         and schema_name = current_schema() order by constraint_index"
                    ),
                )
                .await?,
            ),
            Vec::new(),
        ),
        Dialect::Sqlite => (
            first(
                rows(
                    session,
                    &format!(
                        "select sql from sqlite_master where type = 'table' and name = {name}"
                    ),
                )
                .await?,
            )
            .first()
            .map(|sql| sqlite_checks(sql))
            .unwrap_or_default(),
            Vec::new(),
        ),
    };
    Ok(CopySource {
        table: found,
        checks,
        identity,
    })
}

/// `(x)` → `x` when the outer parentheses enclose everything.
fn strip_outer(text: &str) -> &str {
    let t = text.trim();
    if t.starts_with('(') && t.ends_with(')') {
        let mut depth = 0;
        for (i, c) in t.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && i != t.len() - 1 {
                        return t;
                    }
                }
                _ => {}
            }
        }
        return &t[1..t.len() - 1];
    }
    t
}

/// How a copy ended (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CopyReport {
    /// The statements, as run or as they would run.
    pub statements: Vec<String>,
    /// Rows copied, when it succeeded.
    pub rows: Option<u64>,
    /// The failing statement (index; `None` before or after the statements) and the message.
    pub error: Option<(Option<usize>, String)>,
    /// Notes for the user (shared sequences, dropped table after a failure).
    pub notes: Vec<String>,
}

/// Whether `target` exists: a query on it succeeds (behaviour 2).
pub async fn target_exists(session: &Session, target: &CopyTarget) -> bool {
    let d = session.dialect();
    let probe = match d {
        Dialect::SqlServer => format!("SELECT TOP 0 * FROM {}", target.name(d)),
        _ => format!("SELECT * FROM {} WHERE 1 = 0", target.name(d)),
    };
    rows(session, &probe).await.is_ok()
}

/// Copies `table` to `target` (behaviours 4 and 6): reads the source again, refuses an existing
/// target, runs the statements in one transaction where DDL is transactional and otherwise drops
/// the new table after a failure.
pub async fn copy_table(
    session: &Session,
    table: &str,
    target: &CopyTarget,
    options: CopyOptions,
) -> CopyReport {
    let d = session.dialect();
    let mut report = CopyReport {
        statements: Vec::new(),
        rows: None,
        error: None,
        notes: Vec::new(),
    };
    let source = match read_source(session, table).await {
        Ok(s) => s,
        Err(e) => {
            report.error = Some((None, e.to_string()));
            return report;
        }
    };
    report.statements = copy_statements(&source, target, options, d);
    report.notes.extend(sequence_note(&source, options));
    if target.table.trim().is_empty() {
        report.error = Some((None, "the target table needs a name".into()));
        return report;
    }
    if target_exists(session, target).await {
        report.error = Some((None, format!("{} exists already", target.name(d))));
        return report;
    }
    let transactional = matches!(
        d,
        Dialect::PostgreSql | Dialect::Sqlite | Dialect::SqlServer
    );
    if transactional {
        let begin = if d == Dialect::SqlServer {
            "BEGIN TRANSACTION"
        } else {
            "BEGIN"
        };
        if let Err(e) = run_one(session, begin).await {
            report.error = Some((None, e.to_string()));
            return report;
        }
    }
    let mut copied = 0;
    for (i, statement) in report.statements.iter().enumerate() {
        match run_one(session, statement).await {
            Ok(n) if statement.starts_with("INSERT") => copied = n,
            Ok(_) => {}
            Err(e) => {
                report.error = Some((Some(i), e.to_string()));
                break;
            }
        }
    }
    match (&report.error, transactional) {
        (None, true) => {
            if let Err(e) = run_one(session, "COMMIT").await {
                let _ = run_one(session, "ROLLBACK").await;
                report.error = Some((None, e.to_string()));
            }
        }
        (Some(_), true) => {
            let _ = run_one(session, "ROLLBACK").await;
        }
        (Some((Some(i), _)), false) if *i > 0 => {
            let dropped = run_one(session, &format!("DROP TABLE {}", target.name(d))).await;
            report.notes.push(match dropped {
                Ok(_) => format!("{} was dropped again", target.name(d)),
                Err(e) => format!("{} could not be dropped again: {e}", target.name(d)),
            });
        }
        _ => {}
    }
    if report.error.is_none() {
        report.rows = Some(if options.rows { copied } else { 0 });
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, ForeignKey, Index};

    fn source() -> CopySource {
        let column = |name: &str, ty: &str, nullable: bool, default: Option<&str>| Column {
            name: name.into(),
            data_type: ty.into(),
            nullable,
            default: default.map(str::to_owned),
            generated: None,
        };
        CopySource {
            table: Table {
                storage: Vec::new(),
                hidden_columns: Vec::new(),
                name: "item".into(),
                kind: "table".into(),
                columns: vec![
                    column(
                        "id",
                        "integer",
                        false,
                        Some("nextval('item_id_seq'::regclass)"),
                    ),
                    column("sku", "text", false, None),
                    column("qty", "integer", true, Some("0")),
                    column("warehouse_id", "integer", true, None),
                ],
                primary_key: vec!["id".into()],
                foreign_keys: vec![ForeignKey {
                    name: Some("item_wh_fk".into()),
                    columns: vec!["warehouse_id".into()],
                    referenced_table: "warehouse".into(),
                    referenced_columns: vec!["id".into()],
                }],
                indexes: vec![
                    Index {
                        name: "item_pkey".into(),
                        unique: true,
                        columns: vec!["id".into()],
                        condition: None,
                        collations: Vec::new(),
                    },
                    Index {
                        name: "item_sku_key".into(),
                        unique: true,
                        columns: vec!["sku".into()],
                        condition: None,
                        collations: Vec::new(),
                    },
                    Index {
                        name: "item_lower".into(),
                        unique: true,
                        columns: vec!["lower(sku)".into()],
                        condition: None,
                        collations: Vec::new(),
                    },
                ],
                unreadable: None,
                constraints: Vec::new(),
            },
            checks: vec!["CHECK ((qty >= 0))".into()],
            identity: Vec::new(),
        }
    }

    #[test]
    fn copy_keeps_constraints_by_default() {
        let s = source();
        let target = CopyTarget::default_for("item", Some("backup"));
        let sql = copy_statements(&s, &target, CopyOptions::default(), Dialect::PostgreSql);
        assert_eq!(
            sql[0],
            "CREATE TABLE \"backup\".\"item\" (\n    \"id\" integer NOT NULL DEFAULT nextval('item_id_seq'::regclass),\n    \"sku\" text NOT NULL,\n    \"qty\" integer DEFAULT 0,\n    \"warehouse_id\" integer,\n    PRIMARY KEY (\"id\"),\n    UNIQUE (\"sku\"),\n    CHECK ((qty >= 0))\n)"
        );
        assert_eq!(
            sql[1],
            "INSERT INTO \"backup\".\"item\" (\"id\", \"sku\", \"qty\", \"warehouse_id\") SELECT \"id\", \"sku\", \"qty\", \"warehouse_id\" FROM \"item\""
        );
        assert!(sequence_note(&s, CopyOptions::default()).is_some());
        assert_eq!(CopyTarget::default_for("item", None).table, "item_copy");
        assert_eq!(
            CopyTarget::default_for("item", Some(" ")).table,
            "item_copy"
        );
    }

    #[test]
    fn each_switch_removes_its_part() {
        let s = source();
        let target = CopyTarget::default_for("item", None);
        let none = CopyOptions {
            primary_key: false,
            unique: false,
            not_null: false,
            defaults: false,
            checks: false,
            foreign_keys: false,
            rows: false,
        };
        let sql = copy_statements(&s, &target, none, Dialect::MySql);
        assert_eq!(sql.len(), 1);
        for part in [
            "NOT NULL",
            "DEFAULT",
            "PRIMARY KEY",
            "UNIQUE",
            "CHECK",
            "FOREIGN",
        ] {
            assert!(!sql[0].contains(part), "{part}");
        }
        let fks = CopyOptions {
            foreign_keys: true,
            ..CopyOptions::default()
        };
        let sql = copy_statements(&s, &target, fks, Dialect::MySql);
        assert!(sql[0].contains("FOREIGN KEY (`warehouse_id`) REFERENCES `warehouse` (`id`)"));
        assert!(sql[0].starts_with("CREATE TABLE `item_copy`"));
    }

    #[test]
    fn identity_columns_per_dialect() {
        let mut s = source();
        s.table.columns[0].default = None;
        s.identity = vec!["id".into()];
        let target = CopyTarget::default_for("item", None);
        let no_key = CopyOptions {
            primary_key: false,
            ..CopyOptions::default()
        };
        let my = copy_statements(&s, &target, no_key, Dialect::MySql);
        assert!(my[0].contains("`id` integer NOT NULL AUTO_INCREMENT"));
        assert!(
            my[0].contains("PRIMARY KEY (`id`)"),
            "AUTO_INCREMENT needs a key"
        );
        let ms = copy_statements(&s, &target, CopyOptions::default(), Dialect::SqlServer);
        assert!(ms[0].contains("[id] integer IDENTITY(1,1) NOT NULL"));
        assert_eq!(ms[1], "SET IDENTITY_INSERT [item_copy] ON");
        assert_eq!(ms[3], "SET IDENTITY_INSERT [item_copy] OFF");
    }

    #[test]
    fn sqlite_check_clauses_are_parsed() {
        let sql = "CREATE TABLE t (id INTEGER PRIMARY KEY, qty INT CHECK (qty >= 0), \
                   note TEXT DEFAULT 'check (x)', \"checkpoint\" INT, \
                   CONSTRAINT c CHECK(length(note) < 10 AND (qty < 100)))";
        assert_eq!(
            sqlite_checks(sql),
            vec![
                "CHECK (qty >= 0)".to_owned(),
                "CHECK (length(note) < 10 AND (qty < 100))".to_owned()
            ]
        );
        assert_eq!(strip_outer("(`qty` > 0)"), "`qty` > 0");
        assert_eq!(strip_outer("(a) and (b)"), "(a) and (b)");
    }
}
