//! Schema comparison and DDL generation (`SPEC-0018`).
//!
//! Two schemas of one dialect (as read by `SPEC-0013`) are compared, and the
//! statements that turn the target into the source are generated in an order
//! where every statement only refers to objects that exist at that point.
//! v2: views are compared and dropped before what they use, destructive
//! statements are left out unless chosen, and the script is transactional
//! where the database allows it. v6: check and unique constraints stay
//! constraints, the steps that only exist for a table drop are chosen with it,
//! and view bodies move from the source schema to the target schema. v7: the
//! script pins its target schema with its first statement (on PostgreSQL only for
//! its transaction), and names in its comments stay on one line.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::er::view_dependencies;
use crate::refactor::requalify;
use crate::{
    Column, Dialect, ForeignKey, Index, Table, TableConstraint, ViewDef, literal, quote_identifier,
};

/// How the target schema differs from the source schema.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct SchemaDiff {
    /// Tables only in the source.
    pub added: Vec<String>,
    /// Tables only in the target.
    pub removed: Vec<String>,
    /// Tables in both that differ.
    pub changed: Vec<TableDiff>,
    /// Views only in the source (v2).
    pub views_added: Vec<String>,
    /// Views only in the target.
    pub views_removed: Vec<String>,
    /// Views in both whose definitions differ.
    pub views_changed: Vec<String>,
}

impl SchemaDiff {
    /// Whether the schemas are equal.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
            && self.views_added.is_empty()
            && self.views_removed.is_empty()
            && self.views_changed.is_empty()
    }
}

/// Differences of one table; "added" means present in the source only.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TableDiff {
    /// Table name.
    pub name: String,
    /// Columns only in the source.
    pub columns_added: Vec<Column>,
    /// Columns only in the target.
    pub columns_removed: Vec<Column>,
    /// Columns in both that differ.
    pub columns_changed: Vec<ColumnChange>,
    /// Primary key as (target, source) when it differs.
    pub primary_key: Option<(Vec<String>, Vec<String>)>,
    /// Storage parameters as (target, source) when they differ (v9 behaviour 21).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<(Vec<String>, Vec<String>)>,
    /// Indexes only in the source.
    pub indexes_added: Vec<Index>,
    /// Indexes only in the target.
    pub indexes_removed: Vec<Index>,
    /// Foreign keys only in the source.
    pub foreign_keys_added: Vec<ForeignKey>,
    /// Foreign keys only in the target.
    pub foreign_keys_removed: Vec<ForeignKey>,
    /// Check and unique constraints only in the source (v6 behaviour 16).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints_added: Vec<TableConstraint>,
    /// Check and unique constraints only in the target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints_removed: Vec<TableConstraint>,
}

impl TableDiff {
    fn is_empty(&self) -> bool {
        self.constraints_added.is_empty()
            && self.constraints_removed.is_empty()
            && self.columns_added.is_empty()
            && self.columns_removed.is_empty()
            && self.columns_changed.is_empty()
            && self.primary_key.is_none()
            && self.indexes_added.is_empty()
            && self.indexes_removed.is_empty()
            && self.foreign_keys_added.is_empty()
            && self.foreign_keys_removed.is_empty()
            && self.storage.is_none()
    }
}

/// A column as it is in the target (`from`) and should be (`to`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ColumnChange {
    /// Column in the target.
    pub from: Column,
    /// Column in the source.
    pub to: Column,
}

/// One generated statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DdlStatement {
    /// SQL text without trailing `;`.
    pub sql: String,
    /// Whether running it can lose data.
    pub data_loss: bool,
    /// For a destructive statement (v2 behaviour 13): what is lost, e.g. `all rows of table item`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive: Option<String>,
    /// Destructive statements with the same group are chosen together (a SQLite rebuild; a dropped
    /// table and the dropped views that use it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<usize>,
}

impl DdlStatement {
    /// Whether it is in the script unless chosen otherwise: everything that is not destructive.
    pub fn selected_by_default(&self) -> bool {
        self.destructive.is_none()
    }
}

fn keep(sql: String) -> DdlStatement {
    DdlStatement {
        sql,
        data_loss: false,
        destructive: None,
        group: None,
    }
}

fn lose(sql: String, what: String) -> DdlStatement {
    DdlStatement {
        sql,
        data_loss: true,
        destructive: Some(what),
        group: None,
    }
}

fn tables_by_name(tables: &[Table]) -> BTreeMap<&str, &Table> {
    tables
        .iter()
        .filter(|t| t.kind == "table")
        .map(|t| (t.name.as_str(), t))
        .collect()
}

/// Indexes that are compared and generated: not the primary key's own index,
/// not MySQL's index for a foreign key of the same name, not the index behind a
/// unique constraint of the same name (v6 behaviour 16).
fn own_indexes(table: &Table, dialect: Dialect) -> Vec<&Index> {
    table
        .indexes
        .iter()
        .filter(|i| !(i.is_key() && i.columns == table.primary_key) && i.name != "PRIMARY")
        .filter(|i| {
            !table
                .constraints
                .iter()
                .any(|c| c.is_unique() && c.name == i.name)
        })
        .filter(|i| {
            !dialect.mysql_like()
                || !table
                    .foreign_keys
                    .iter()
                    .any(|f| f.name.as_deref() == Some(i.name.as_str()))
        })
        .collect()
}

fn same_index(a: &Index, b: &Index) -> bool {
    a.unique == b.unique
        && a.columns == b.columns
        && a.condition == b.condition
        && a.method == b.method
        && (0..a.columns.len()).all(|i| a.order(i) == b.order(i) && a.prefix(i) == b.prefix(i))
        && (0..a.columns.len())
            .all(|i| a.collation(i).map(str::to_lowercase) == b.collation(i).map(str::to_lowercase))
}

fn same_foreign_key(a: &ForeignKey, b: &ForeignKey) -> bool {
    a.columns == b.columns
        && a.referenced_table == b.referenced_table
        && a.referenced_columns == b.referenced_columns
        && a.on_delete == b.on_delete
        && a.on_update == b.on_update
}

/// Constraints match by kind and definition, case and white space folded, names ignored.
fn same_constraint(a: &TableConstraint, b: &TableConstraint) -> bool {
    a.kind == b.kind && same_type(&a.definition, &b.definition)
}

fn same_type(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn column_differs(a: &Column, b: &Column) -> bool {
    !same_type(&a.data_type, &b.data_type)
        || a.nullable != b.nullable
        || a.default.as_deref().map(str::trim) != b.default.as_deref().map(str::trim)
}

/// Compares `target` against `source` (both of `dialect`); views are ignored.
pub fn compare(source: &[Table], target: &[Table], dialect: Dialect) -> SchemaDiff {
    compare_with_views(source, target, &[], &[], dialect)
}

/// A view definition for comparison: case and white space folded, trailing `;` dropped.
fn same_definition(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .trim_end_matches(';')
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// Like [`compare`], also comparing the views whose definitions are given (v2 behaviour 11).
pub fn compare_with_views(
    source: &[Table],
    target: &[Table],
    source_views: &[ViewDef],
    target_views: &[ViewDef],
    dialect: Dialect,
) -> SchemaDiff {
    compare_between(source, target, source_views, target_views, dialect, None)
}

/// The definition of source view `definition` as the script creates it in the target: names
/// qualified with the source schema qualified with the target schema (v6 behaviour 18).
fn in_target(definition: &str, dialect: Dialect, schemas: Option<(&str, &str)>) -> String {
    match schemas {
        Some((from, to)) if from != to => requalify(definition, dialect, from, Some(to)),
        _ => definition.to_owned(),
    }
}

/// A definition as compared between two schemas: a qualifier of the target schema counts as none.
fn unqualified_in(definition: &str, dialect: Dialect, schemas: Option<(&str, &str)>) -> String {
    match schemas {
        Some((from, to)) if from != to => requalify(definition, dialect, to, None),
        _ => definition.to_owned(),
    }
}

/// Like [`compare_with_views`] between the schemas `(source, target)` when they are known: view
/// bodies are compared as the script would write them in the target (v6 behaviour 18).
pub fn compare_between(
    source: &[Table],
    target: &[Table],
    source_views: &[ViewDef],
    target_views: &[ViewDef],
    dialect: Dialect,
    schemas: Option<(&str, &str)>,
) -> SchemaDiff {
    let mut diff = compare_tables(source, target, dialect);
    let find = |views: &[ViewDef], n: &str| views.iter().find(|v| v.name == n).cloned();
    let source_text = |v: &ViewDef| {
        unqualified_in(
            &in_target(&v.definition, dialect, schemas),
            dialect,
            schemas,
        )
    };
    for v in source_views {
        match find(target_views, &v.name) {
            None => diff.views_added.push(v.name.clone()),
            Some(t)
                if !same_definition(
                    &unqualified_in(&t.definition, dialect, schemas),
                    &source_text(v),
                ) =>
            {
                diff.views_changed.push(v.name.clone());
            }
            Some(_) => {}
        }
    }
    diff.views_removed = target_views
        .iter()
        .filter(|v| find(source_views, &v.name).is_none())
        .map(|v| v.name.clone())
        .collect();
    for list in [
        &mut diff.views_added,
        &mut diff.views_removed,
        &mut diff.views_changed,
    ] {
        list.sort();
    }
    diff
}

fn compare_tables(source: &[Table], target: &[Table], dialect: Dialect) -> SchemaDiff {
    let src = tables_by_name(source);
    let tgt = tables_by_name(target);
    let mut diff = SchemaDiff {
        added: src
            .keys()
            .filter(|n| !tgt.contains_key(*n))
            .map(|n| n.to_string())
            .collect(),
        removed: tgt
            .keys()
            .filter(|n| !src.contains_key(*n))
            .map(|n| n.to_string())
            .collect(),
        ..SchemaDiff::default()
    };
    for (name, s) in &src {
        let Some(t) = tgt.get(name) else { continue };
        let column =
            |table: &Table, name: &str| table.columns.iter().find(|c| c.name == name).cloned();
        let s_idx = own_indexes(s, dialect);
        let t_idx = own_indexes(t, dialect);
        let td = TableDiff {
            name: name.to_string(),
            columns_added: s
                .columns
                .iter()
                .filter(|c| column(t, &c.name).is_none())
                .cloned()
                .collect(),
            columns_removed: t
                .columns
                .iter()
                .filter(|c| column(s, &c.name).is_none())
                .cloned()
                .collect(),
            columns_changed: s
                .columns
                .iter()
                .filter_map(|to| {
                    let from = column(t, &to.name)?;
                    column_differs(&from, to).then(|| ColumnChange {
                        from,
                        to: to.clone(),
                    })
                })
                .collect(),
            primary_key: (s.primary_key != t.primary_key)
                .then(|| (t.primary_key.clone(), s.primary_key.clone())),
            storage: (s.storage != t.storage).then(|| (t.storage.clone(), s.storage.clone())),
            indexes_added: s_idx
                .iter()
                .filter(|i| !t_idx.iter().any(|j| same_index(i, j)))
                .map(|i| (*i).clone())
                .collect(),
            indexes_removed: t_idx
                .iter()
                .filter(|i| !s_idx.iter().any(|j| same_index(i, j)))
                .map(|i| (*i).clone())
                .collect(),
            foreign_keys_added: s
                .foreign_keys
                .iter()
                .filter(|f| !t.foreign_keys.iter().any(|g| same_foreign_key(f, g)))
                .cloned()
                .collect(),
            foreign_keys_removed: t
                .foreign_keys
                .iter()
                .filter(|f| !s.foreign_keys.iter().any(|g| same_foreign_key(f, g)))
                .cloned()
                .collect(),
            constraints_added: s
                .constraints
                .iter()
                .filter(|c| !t.constraints.iter().any(|o| same_constraint(c, o)))
                .cloned()
                .collect(),
            constraints_removed: t
                .constraints
                .iter()
                .filter(|c| !s.constraints.iter().any(|o| same_constraint(c, o)))
                .cloned()
                .collect(),
        };
        if !td.is_empty() {
            diff.changed.push(td);
        }
    }
    diff
}

/// The `CREATE` script of `tables`: the migration from an empty schema.
pub fn create_script(tables: &[Table], dialect: Dialect) -> Vec<DdlStatement> {
    migration(tables, &[], dialect)
}

/// The definition of the table or view `name` as SQL, as the Database tool window's DDL tab and the
/// MCP tool `table_ddl` show it (`SPEC-0089` v5 behaviour 11): a table's `CREATE TABLE` with its
/// columns (type, `NOT NULL`, default), primary key and foreign keys, then its own indexes; a
/// view's `CREATE VIEW … AS` with the definition in `views`, or a comment that the definition is
/// not available. `None` when there is no such table or view.
pub fn object_ddl(
    tables: &[Table],
    views: &[ViewDef],
    name: &str,
    dialect: Dialect,
) -> Option<String> {
    let d = dialect;
    let object = tables.iter().find(|t| t.name == name);
    let is_view = object.map_or_else(
        || views.iter().any(|v| v.name == name),
        |t| t.kind == "view",
    );
    if is_view {
        return Some(match views.iter().find(|v| v.name == name) {
            Some(v) => format!(
                "CREATE VIEW {} AS\n{};\n",
                quote_identifier(name, d),
                v.definition.trim().trim_end_matches(';').trim()
            ),
            None => format!("-- The definition of view {name} is not available.\n"),
        });
    }
    let t = object?;
    let mut out = format!("{};\n", create_table(t, d, true, &t.name));
    let indexes = own_indexes(t, d);
    if !indexes.is_empty() {
        out.push('\n');
    }
    for i in indexes {
        out.push_str(&create_index(t, i, d));
        out.push_str(";\n");
    }
    let comments = comment_statements(t, d);
    if !comments.is_empty() {
        out.push('\n');
        out.push_str(&comments);
    }
    Some(out)
}

/// Statements that turn `target` into `source` (`SPEC-0018` behaviour 5–8); views are ignored.
pub fn migration(source: &[Table], target: &[Table], dialect: Dialect) -> Vec<DdlStatement> {
    migration_with_views(source, target, &[], &[], dialect)
}

/// Whether SQLite can make change `td` of `source`'s table in place: only nullable or defaulted
/// columns added outside the primary key, maybe with indexes; anything else rebuilds the table.
fn sqlite_in_place(source: &Table, td: &TableDiff) -> bool {
    let addable =
        |c: &Column| (c.nullable || c.default.is_some()) && !source.primary_key.contains(&c.name);
    td.columns_removed.is_empty()
        && td.columns_changed.is_empty()
        && td.primary_key.is_none()
        && td.foreign_keys_added.is_empty()
        && td.foreign_keys_removed.is_empty()
        && td.columns_added.iter().all(addable)
}

/// Views in an order where each comes after the views it uses (`reverse` = dependants first).
fn view_order(names: &BTreeSet<String>, views: &[ViewDef], reverse: bool) -> Vec<String> {
    let all: Vec<&str> = views.iter().map(|v| v.name.as_str()).collect();
    let deps = view_dependencies(views, &all);
    let mut left: Vec<String> = names.iter().cloned().collect();
    let mut out = Vec::new();
    while !left.is_empty() {
        // Next: a view that uses no other view still left (cycles: just the first).
        let pos = left
            .iter()
            .position(|v| {
                !deps
                    .iter()
                    .any(|d| &d.view == v && d.uses != *v && left.contains(&d.uses))
            })
            .unwrap_or(0);
        out.push(left.remove(pos));
    }
    if reverse {
        out.reverse();
    }
    out
}

/// Whether view `definition` names any of `columns` as a word (case ignored, quotes stripped), or
/// selects all columns (`*` or `t.*` in a select list, not a product or `count(*)`), so a change of
/// those columns can break it.
fn names_any(definition: &str, columns: &[&str]) -> bool {
    let star = definition.match_indices('*').any(|(i, _)| {
        let before = definition[..i].trim_end();
        before.ends_with('.')
            || before.ends_with(',')
            || before.to_ascii_lowercase().ends_with("select")
            || before.to_ascii_lowercase().ends_with("distinct")
    });
    star || definition
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .any(|w| columns.iter().any(|c| c.eq_ignore_ascii_case(w)))
}

/// Like [`migration`], with views (v2 behaviours 11–13): views that go away, change, or use a table
/// that is dropped or whose columns change are dropped first (dependants first) and the source's
/// views created last (what they use first).
pub fn migration_with_views(
    source: &[Table],
    target: &[Table],
    source_views: &[ViewDef],
    target_views: &[ViewDef],
    dialect: Dialect,
) -> Vec<DdlStatement> {
    migration_between(source, target, source_views, target_views, dialect, None)
}

/// Like [`migration_with_views`] between the schemas `(source, target)` when they are known: the
/// source's view bodies are written for the target schema (v6 behaviour 18).
pub fn migration_between(
    source: &[Table],
    target: &[Table],
    source_views: &[ViewDef],
    target_views: &[ViewDef],
    dialect: Dialect,
    schemas: Option<(&str, &str)>,
) -> Vec<DdlStatement> {
    let d = dialect;
    let diff = compare_between(source, target, source_views, target_views, d, schemas);
    // Tables whose change can break a view using them: `None` for the whole table (dropped, a
    // SQLite rebuild, a new primary key), else the columns dropped or changed (v3: a view that does
    // not name any of them stays).
    let mut touched: BTreeMap<&str, Option<Vec<&str>>> =
        diff.removed.iter().map(|n| (n.as_str(), None)).collect();
    for t in &diff.changed {
        let columns: Vec<&str> = t
            .columns_removed
            .iter()
            .map(|c| c.name.as_str())
            .chain(t.columns_changed.iter().map(|c| c.from.name.as_str()))
            .collect();
        // A SQLite table changed in place (ALTER TABLE … ADD COLUMN) keeps its views; a rebuild
        // touches them all (v5, hand test 121 retest).
        let rebuilt = d == Dialect::Sqlite
            && source
                .iter()
                .find(|s| s.name == t.name)
                .is_none_or(|s| !sqlite_in_place(s, t));
        if rebuilt || t.primary_key.is_some() {
            touched.insert(t.name.as_str(), None);
        } else if !columns.is_empty() {
            touched.insert(t.name.as_str(), Some(columns));
        }
    }
    let mut names: Vec<&str> = target.iter().map(|t| t.name.as_str()).collect();
    names.extend(target_views.iter().map(|v| v.name.as_str()));
    let deps = view_dependencies(target_views, &names);
    let breaks = |view: &str, table: &str| match touched.get(table) {
        Some(None) => true,
        Some(Some(columns)) => target_views
            .iter()
            .find(|v| v.name == view)
            .is_none_or(|v| names_any(&v.definition, columns)),
        None => false,
    };
    let mut drop_views: BTreeSet<String> = diff
        .views_removed
        .iter()
        .chain(&diff.views_changed)
        .cloned()
        .collect();
    loop {
        let before = drop_views.len();
        for dep in &deps {
            if breaks(&dep.view, &dep.uses) || drop_views.contains(&dep.uses) {
                drop_views.insert(dep.view.clone());
            }
        }
        if drop_views.len() == before {
            break;
        }
    }
    let mut create_views: BTreeSet<String> = diff
        .views_added
        .iter()
        .chain(&diff.views_changed)
        .cloned()
        .collect();
    create_views.extend(
        drop_views
            .iter()
            .filter(|v| source_views.iter().any(|s| &s.name == *v))
            .cloned(),
    );
    let mut out = Vec::new();
    for v in view_order(&drop_views, target_views, true) {
        let sql = format!("DROP VIEW {}", quote_identifier(&v, d));
        out.push(if diff.views_removed.contains(&v) {
            // Destructive (the view is gone), but no rows are lost.
            DdlStatement {
                data_loss: false,
                ..lose(sql, format!("view {v} (its definition; it holds no rows)"))
            }
        } else {
            keep(sql)
        });
    }
    let tables = table_migration(source, target, d, &diff);
    // SQLite: the rebuild's PRAGMA stays first.
    let pragma = tables.first().is_some_and(|s| s.sql.starts_with("PRAGMA"));
    let mut tables = tables.into_iter();
    if pragma {
        out.insert(0, tables.next().expect("pragma"));
    }
    out.extend(tables);
    let tail = if pragma { out.pop() } else { None };
    for v in view_order(&create_views, source_views, false) {
        let def = &source_views
            .iter()
            .find(|s| s.name == v)
            .expect("source view")
            .definition;
        let def = in_target(def, d, schemas);
        out.push(keep(format!(
            "CREATE VIEW {} AS {}",
            quote_identifier(&v, d),
            def.trim().trim_end_matches(';').trim()
        )));
    }
    out.extend(tail);
    // Groups: a dropped table with the dropped views that use it.
    let mut next = out
        .iter()
        .filter_map(|s| s.group)
        .max()
        .map_or(0, |g| g + 1);
    for (i, st) in out.clone().iter().enumerate() {
        let Some(table) = diff
            .removed
            .iter()
            .find(|t| st.sql == format!("DROP TABLE {}", quote_identifier(t, d)))
        else {
            continue;
        };
        let users: Vec<usize> = deps
            .iter()
            .filter(|dep| dep.uses == *table && diff.views_removed.contains(&dep.view))
            .filter_map(|dep| {
                let sql = format!("DROP VIEW {}", quote_identifier(&dep.view, d));
                out.iter().position(|s| s.sql == sql)
            })
            .collect();
        if users.is_empty() {
            continue;
        }
        let group = users
            .iter()
            .chain([&i])
            .find_map(|k| out[*k].group)
            .unwrap_or_else(|| {
                next += 1;
                next - 1
            });
        for k in users.into_iter().chain([i]) {
            if let Some(old) = out[k].group.filter(|g| *g != group) {
                for s in &mut out {
                    if s.group == Some(old) {
                        s.group = Some(group);
                    }
                }
            }
            out[k].group = Some(group);
        }
    }
    out
}

/// Which statements end up in the script after `toggled` is switched to `on`, starting from
/// `selected`: statements of the same group follow (v2 behaviour 13).
pub fn toggle_statement(
    statements: &[DdlStatement],
    selected: &[bool],
    toggled: usize,
    on: bool,
) -> Vec<bool> {
    let mut out = selected.to_vec();
    out.resize(statements.len(), true);
    let group = statements.get(toggled).and_then(|s| s.group);
    for (i, s) in statements.iter().enumerate() {
        if i == toggled || (group.is_some() && s.group == group) {
            out[i] = on;
        }
    }
    out
}

/// The script of the `selected` statements (v2 behaviour 14): each ends in `;`; destructive ones are
/// preceded by a comment naming what is lost; left-out destructive ones are comment lines; one
/// transaction where the dialect runs DDL in transactions. The script does not name its target
/// schema; [`migration_script_in`] does (v7 behaviour 19).
pub fn migration_script(
    statements: &[DdlStatement],
    selected: &[bool],
    dialect: Dialect,
) -> String {
    migration_script_in(statements, selected, dialect, None)
}

/// The statement, without `;`, that pins a migration script to the schema `target` (v7 behaviour
/// 19): PostgreSQL `SET LOCAL search_path`, which ends with the script's transaction, with
/// `pg_temp` last so a temporary table cannot stand in for a target table; DuckDB
/// `SET search_path`, MySQL and MariaDB `USE` and Oracle `ALTER SESSION SET CURRENT_SCHEMA`, which
/// stay for the session. `None` on SQL Server and SQLite, which have no such statement, and for an
/// empty name.
pub fn target_pin(dialect: Dialect, target: &str) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    let name = quote_identifier(target, dialect);
    match dialect {
        Dialect::PostgreSql => Some(format!("SET LOCAL search_path TO {name}, pg_temp")),
        Dialect::DuckDb => Some(format!("SET search_path TO {name}")),
        Dialect::MySql | Dialect::MariaDb => Some(format!("USE {name}")),
        Dialect::Oracle => Some(format!("ALTER SESSION SET CURRENT_SCHEMA = {name}")),
        Dialect::SqlServer | Dialect::Sqlite => None,
    }
}

/// Text for a `--` comment on one line: a line break would end the comment and turn the rest of a
/// name into a statement (v7 behaviour 14).
pub fn comment_text(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// Like [`migration_script`] for the target schema `target` when it is known (v7 behaviour 19):
/// the first statement the script runs pins it — on PostgreSQL right after `BEGIN`, for the
/// script's transaction only; elsewhere before the first change, after a comment saying that it
/// stays for the session.
pub fn migration_script_in(
    statements: &[DdlStatement],
    selected: &[bool],
    dialect: Dialect,
    target: Option<&str>,
) -> String {
    let chosen = |i: usize| {
        selected
            .get(i)
            .copied()
            .unwrap_or_else(|| statements[i].selected_by_default())
    };
    let mut out = String::new();
    let transactional = matches!(
        dialect,
        Dialect::PostgreSql | Dialect::Sqlite | Dialect::SqlServer
    );
    let any = (0..statements.len()).any(chosen);
    let pin = target.and_then(|t| target_pin(dialect, t)).filter(|_| any);
    if !transactional && any {
        out.push_str(
            "-- This database commits each DDL statement on its own; these statements cannot be rolled back together.\n",
        );
    }
    // A pin that outlasts a transaction comes first and says so.
    if !transactional && let Some(pin) = &pin {
        out.push_str(
            "-- The next statement makes the target schema current for this session; it stays so after the script.\n",
        );
        out.push_str(pin);
        out.push_str(";\n");
    }
    let is_pragma = |s: &DdlStatement| s.sql.starts_with("PRAGMA");
    let mut opened = false;
    let mut pending_close = false;
    for (i, s) in statements.iter().enumerate() {
        if transactional && any && !opened && !is_pragma(s) {
            out.push_str(if dialect == Dialect::SqlServer {
                "\nBEGIN TRANSACTION;\n"
            } else {
                "\nBEGIN;\n"
            });
            // PostgreSQL: valid until COMMIT or ROLLBACK, so the session's path is unchanged.
            if let Some(pin) = &pin {
                out.push_str(pin);
                out.push_str(";\n");
            }
            opened = true;
            pending_close = true;
        }
        if transactional && pending_close && is_pragma(s) && opened {
            out.push_str("\nCOMMIT;\n");
            pending_close = false;
        }
        out.push('\n');
        if !chosen(i) {
            let what = comment_text(s.destructive.as_deref().unwrap_or("not chosen"));
            out.push_str(&format!("-- Left out (loses {what}):\n"));
            // Split at a lone `\r` too (`lines` keeps it): it ends a `--` comment as well.
            for line in s.sql.split(['\r', '\n']).filter(|l| !l.is_empty()) {
                out.push_str(&format!("-- {line}\n"));
            }
            continue;
        }
        if let Some(what) = &s.destructive {
            out.push_str(&format!("-- Can lose data: {}.\n", comment_text(what)));
        } else if s.data_loss {
            out.push_str("-- Can lose data.\n");
        }
        out.push_str(&s.sql);
        out.push_str(";\n");
    }
    if pending_close {
        out.push_str("\nCOMMIT;\n");
    }
    out
}

/// The tables to be dropped that must be chosen together (v6 behaviour 17): those that reference
/// one another, and on SQLite those whose foreign keys one rebuild of a kept table removes. Each
/// dropped table's group number, counted from 0, and the next free number.
fn drop_groups(
    target: &BTreeMap<&str, &Table>,
    source: &BTreeMap<&str, &Table>,
    d: Dialect,
    diff: &SchemaDiff,
) -> (BTreeMap<String, usize>, usize) {
    fn find(root: &BTreeMap<String, String>, name: &str) -> String {
        let mut n = name.to_owned();
        while root[&n] != n {
            n.clone_from(&root[&n]);
        }
        n
    }
    let removed: BTreeSet<&str> = diff.removed.iter().map(String::as_str).collect();
    let mut root: BTreeMap<String, String> = removed
        .iter()
        .map(|n| ((*n).to_owned(), (*n).to_owned()))
        .collect();
    let mut join = |a: &str, b: &str| {
        let (ra, rb) = (find(&root, a), find(&root, b));
        if ra != rb {
            let (low, high) = if ra < rb { (ra, rb) } else { (rb, ra) };
            root.insert(high, low);
        }
    };
    for name in &removed {
        for f in &target[name].foreign_keys {
            if removed.contains(f.referenced_table.as_str()) {
                join(name, &f.referenced_table);
            }
        }
    }
    if d == Dialect::Sqlite {
        for td in &diff.changed {
            let rebuilt = source
                .get(td.name.as_str())
                .is_none_or(|s| !sqlite_in_place(s, td));
            let tied: Vec<&str> = td
                .foreign_keys_removed
                .iter()
                .map(|f| f.referenced_table.as_str())
                .filter(|t| removed.contains(t))
                .collect();
            if rebuilt {
                for pair in tied.windows(2) {
                    join(pair[0], pair[1]);
                }
            }
        }
    }
    let mut numbers: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups = BTreeMap::new();
    for name in &removed {
        let r = find(&root, name);
        let next = numbers.len();
        let g = *numbers.entry(r).or_insert(next);
        groups.insert((*name).to_owned(), g);
    }
    let next = numbers.len();
    (groups, next)
}

/// A statement that only exists so that `table` can be dropped: chosen with its drop, losing no
/// data itself (v6 behaviour 17).
fn tied(statement: DdlStatement, table: &str, group: usize) -> DdlStatement {
    DdlStatement {
        destructive: statement
            .destructive
            .or_else(|| Some(format!("all rows of table {table}"))),
        group: Some(group),
        ..statement
    }
}

fn table_migration(
    source: &[Table],
    target: &[Table],
    d: Dialect,
    diff: &SchemaDiff,
) -> Vec<DdlStatement> {
    let src = tables_by_name(source);
    let tgt = tables_by_name(target);
    let q = |name: &str| quote_identifier(name, d);
    let (mut drop_fks, mut drop_idx, mut drop_tables, mut create_tables) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut alter, mut create_idx, mut add_fks) = (Vec::new(), Vec::new(), Vec::new());
    let mut rebuild = false;
    let (groups, mut next_group) = drop_groups(&tgt, &src, d, diff);
    // The dropped table a foreign key points at, with its group, if it is dropped.
    let dropped = |f: &ForeignKey| {
        groups
            .get(&f.referenced_table)
            .map(|g| (f.referenced_table.clone(), *g))
    };

    for name in &diff.removed {
        let t = tgt[name.as_str()];
        let g = groups[name];
        if !matches!(d, Dialect::Sqlite | Dialect::DuckDb) {
            drop_fks.extend(
                t.foreign_keys
                    .iter()
                    .filter_map(|f| drop_foreign_key(t, f, d))
                    .map(|st| tied(st, name, g)),
            );
        }
        drop_tables.push(DdlStatement {
            group: Some(g),
            ..lose(
                format!("DROP TABLE {}", q(name)),
                format!("all rows of table {name}"),
            )
        });
    }
    for name in &diff.added {
        let s = src[name.as_str()];
        // DuckDB, like SQLite, only knows foreign keys inside CREATE TABLE.
        let inline_fks = matches!(d, Dialect::Sqlite | Dialect::DuckDb);
        // PostgreSQL serial columns default to `nextval('<sequence>')`; the sequence comes first.
        if d == Dialect::PostgreSql {
            create_tables.extend(
                s.columns
                    .iter()
                    .filter_map(|c| sequence_of(c.default.as_deref()?))
                    .map(|seq| keep(format!("CREATE SEQUENCE IF NOT EXISTS {seq}"))),
            );
        }
        create_tables.push(keep(create_table(s, d, inline_fks, &s.name)));
        create_idx.extend(
            own_indexes(s, d)
                .into_iter()
                .map(|i| keep(create_index(s, i, d))),
        );
        if !inline_fks {
            add_fks.extend(
                s.foreign_keys
                    .iter()
                    .map(|f| keep(add_foreign_key(s, f, d))),
            );
        }
    }
    for td in &diff.changed {
        let s = src[td.name.as_str()];
        let t = tgt[td.name.as_str()];
        let table = q(&td.name);
        if d == Dialect::Sqlite {
            if sqlite_in_place(s, td) {
                drop_idx.extend(td.indexes_removed.iter().map(|i| keep(drop_index(t, i, d))));
                alter.extend(td.columns_added.iter().map(|c| {
                    keep(format!(
                        "ALTER TABLE {table} ADD COLUMN {}",
                        column_def(c, d)
                    ))
                }));
                create_idx.extend(td.indexes_added.iter().map(|i| keep(create_index(s, i, d))));
            } else {
                rebuild = true;
                // A rebuild that removes a foreign key to a dropped table is chosen with that
                // table (v6 behaviour 17).
                let tie = td.foreign_keys_removed.iter().find_map(dropped);
                let g = match &tie {
                    Some((_, g)) => *g,
                    None => {
                        next_group += 1;
                        next_group - 1
                    }
                };
                let statements = rebuild_table(s, t, td, d, g);
                alter.extend(statements.into_iter().map(|st| match &tie {
                    Some((dropped_table, g)) => tied(st, dropped_table, *g),
                    None => st,
                }));
            }
            continue;
        }
        drop_fks.extend(td.foreign_keys_removed.iter().filter_map(|f| {
            let st = drop_foreign_key(t, f, d)?;
            // A kept table's foreign key to a dropped table goes only with that table.
            Some(match dropped(f) {
                Some((dropped_table, g)) => tied(st, &dropped_table, g),
                None => st,
            })
        }));
        // Constraints only in the target go before indexes and columns change (v6 behaviour 16).
        drop_idx.extend(td.constraints_removed.iter().map(|c| {
            keep(format!(
                "ALTER TABLE {table} DROP CONSTRAINT {}",
                q(&c.name)
            ))
        }));
        drop_idx.extend(td.indexes_removed.iter().map(|i| keep(drop_index(t, i, d))));
        if let Some((old, _)) = &td.primary_key
            && !old.is_empty()
        {
            let pk_index = t
                .indexes
                .iter()
                .find(|i| i.is_key() && i.columns == *old)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| format!("{}_pkey", t.name));
            alter.push(keep(match d {
                Dialect::MySql | Dialect::MariaDb => {
                    format!("ALTER TABLE {table} DROP PRIMARY KEY")
                }
                _ => format!("ALTER TABLE {table} DROP CONSTRAINT {}", q(&pk_index)),
            }));
        }
        for c in &td.columns_added {
            alter.push(keep(format!(
                "ALTER TABLE {table} {} {}",
                add_column(d),
                column_def(c, d)
            )));
        }
        for c in &td.columns_removed {
            // SQL Server refuses to drop a column while its default constraint exists.
            if d == Dialect::SqlServer && c.default.is_some() {
                alter.push(keep(drop_default_tsql(&td.name, &c.name)));
            }
            alter.push(lose(
                format!("ALTER TABLE {table} DROP COLUMN {}", q(&c.name)),
                format!("column {}.{} and its values", td.name, c.name),
            ));
        }
        for change in &td.columns_changed {
            alter.extend(change_column(&td.name, change, d));
        }
        // v9 behaviour 21: the source's storage parameters.
        if let Some((from, to)) = &td.storage {
            alter.extend(storage_statements(&table, from, to).into_iter().map(keep));
        }
        if let Some((_, new)) = &td.primary_key
            && !new.is_empty()
        {
            alter.push(keep(format!(
                "ALTER TABLE {table} ADD PRIMARY KEY ({})",
                column_list(new, d)
            )));
        }
        // After the columns, before the foreign keys that may need a unique constraint.
        create_idx.extend(td.constraints_added.iter().map(|c| {
            keep(format!(
                "ALTER TABLE {table} ADD {}",
                constraint_clause(c, d)
            ))
        }));
        create_idx.extend(td.indexes_added.iter().map(|i| keep(create_index(s, i, d))));
        add_fks.extend(
            td.foreign_keys_added
                .iter()
                .map(|f| keep(add_foreign_key(s, f, d))),
        );
    }

    let mut out = Vec::new();
    if rebuild {
        out.push(keep("PRAGMA foreign_keys = OFF".into()));
    }
    for group in [
        drop_fks,
        drop_idx,
        drop_tables,
        create_tables,
        alter,
        create_idx,
        add_fks,
    ] {
        out.extend(group);
    }
    if rebuild {
        out.push(keep("PRAGMA foreign_keys = ON".into()));
    }
    out
}

/// `CONSTRAINT name definition` of a check or unique constraint (v6 behaviour 16).
fn constraint_clause(c: &TableConstraint, d: Dialect) -> String {
    format!(
        "CONSTRAINT {} {}",
        quote_identifier(&c.name, d),
        c.definition.trim()
    )
}

fn column_list(columns: &[String], d: Dialect) -> String {
    columns
        .iter()
        .map(|c| quote_identifier(c, d))
        .collect::<Vec<_>>()
        .join(", ")
}

/// MySQL reports defaults as values; numbers, NULL and CURRENT_TIMESTAMP forms
/// stay as they are, everything else becomes a string literal.
pub(crate) fn default_expr(value: &str, d: Dialect) -> String {
    // SQLite reports an expression default without the brackets its grammar asks for.
    if d == Dialect::Sqlite {
        return if sqlite_default_is_plain(value) {
            value.to_owned()
        } else {
            format!("({value})")
        };
    }
    // MariaDB, like the others, reports defaults as SQL expressions.
    if d != Dialect::MySql {
        return value.to_owned();
    }
    let upper = value.to_ascii_uppercase();
    if value.parse::<f64>().is_ok()
        || upper == "NULL"
        || upper.starts_with("CURRENT_TIMESTAMP")
        || value.starts_with('(')
    {
        value.to_owned()
    } else {
        literal(&Some(value.to_owned()), d)
    }
}

/// Whether a SQLite default is a literal, a signed number or a keyword constant, which stand
/// after `DEFAULT` as they are; anything else is an expression.
fn sqlite_default_is_plain(value: &str) -> bool {
    let v = value.trim();
    let upper = v.to_ascii_uppercase();
    let quoted = |q: char| {
        v.len() >= 2 && v.starts_with(q) && v.ends_with(q) && {
            let inner = &v[1..v.len() - 1];
            !inner.replace(&format!("{q}{q}"), "").contains(q)
        }
    };
    let blob = (upper.starts_with("X'") && v.ends_with('\''))
        && v[2..v.len() - 1].chars().all(|c| c.is_ascii_hexdigit());
    (v.starts_with('(') && v.ends_with(')'))
        || v.trim_start_matches(['+', '-']).parse::<f64>().is_ok()
        || quoted('\'')
        || quoted('"')
        || blob
        || matches!(
            upper.as_str(),
            "NULL" | "TRUE" | "FALSE" | "CURRENT_TIME" | "CURRENT_DATE" | "CURRENT_TIMESTAMP"
        )
}

/// The sequence a PostgreSQL default `nextval('name'::regclass)` draws from, as written there
/// (quotes and schema kept).
fn sequence_of(default: &str) -> Option<String> {
    let rest = default.trim().strip_prefix("nextval('")?;
    // The literal ends at a quote that is not doubled.
    let mut name = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
            } else {
                return (!name.is_empty()).then_some(name);
            }
        }
        name.push(c);
    }
    None
}

fn column_def(c: &Column, d: Dialect) -> String {
    let mut out = format!("{} {}", quote_identifier(&c.name, d), c.data_type);
    // SPEC-0013 v9 behaviour 26: a generated column with its expression where it is read, right
    // after the type as MySQL's grammar asks.
    let computed = match (&c.generated, &c.expression) {
        (Some(kind), Some(expression)) => {
            out.push_str(&format!(
                " GENERATED ALWAYS AS {} {}",
                bracketed(expression),
                kind.to_ascii_uppercase()
            ));
            true
        }
        _ => false,
    };
    // Oracle takes the default and the identity clause before the constraints (`SPEC-0135` v2
    // behaviour 5): `NOT NULL DEFAULT …` is a syntax error there.
    let constraints_first = d != Dialect::Oracle;
    if constraints_first && !c.nullable {
        out.push_str(" NOT NULL");
    }
    if let Some(default) = c.default.as_ref().filter(|_| !computed) {
        out.push_str(" DEFAULT ");
        out.push_str(&default_expr(default, d));
    }
    if let Some(identity) = &c.identity {
        out.push(' ');
        out.push_str(identity);
    }
    if !constraints_first && !c.nullable {
        out.push_str(" NOT NULL");
    }
    if let (Some(kind), false) = (&c.generated, computed) {
        // The catalogue does not give the expression (SPEC-0018 v4 behaviour 15).
        out.push_str(&format!(
            " /* generated ({kind}) in the source: written here as an ordinary column */"
        ));
    }
    // v9 behaviour 27: MySQL and MariaDB keep a column's comment in its definition.
    if let Some(comment) = c.comment.as_ref().filter(|_| d.mysql_like()) {
        out.push_str(" COMMENT ");
        out.push_str(&literal(&Some(comment.clone()), d));
    }
    out
}

/// `text` bracketed once: `(qty * 2)` stays, `qty * 2` becomes `(qty * 2)`.
fn bracketed(text: &str) -> String {
    let text = text.trim();
    if crate::introspect::enclosed(text) {
        text.to_owned()
    } else {
        format!("({text})")
    }
}

/// The comments of table `t` as `COMMENT ON` statements, each ending in `;` and a line break
/// (PostgreSQL, `SPEC-0013` v9 behaviour 27; Oracle, `SPEC-0142` v4 behaviour 5, which says
/// `TABLE` for views too); empty on the other systems, which keep them in the definition or do not
/// read them.
fn comment_statements(t: &Table, d: Dialect) -> String {
    if !matches!(d, Dialect::PostgreSql | Dialect::Oracle) {
        return String::new();
    }
    let table = quote_identifier(&t.name, d);
    let mut out = String::new();
    let kind = if t.kind == "view" && d == Dialect::PostgreSql {
        "VIEW"
    } else {
        "TABLE"
    };
    if let Some(comment) = &t.comment {
        out.push_str(&format!(
            "COMMENT ON {kind} {table} IS {};\n",
            literal(&Some(comment.clone()), d)
        ));
    }
    for c in &t.columns {
        if let Some(comment) = &c.comment {
            out.push_str(&format!(
                "COMMENT ON COLUMN {table}.{} IS {};\n",
                quote_identifier(&c.name, d),
                literal(&Some(comment.clone()), d)
            ));
        }
    }
    out
}

fn foreign_key_clause(f: &ForeignKey, d: Dialect) -> String {
    let constraint = match (&f.name, d) {
        (
            Some(name),
            Dialect::PostgreSql
            | Dialect::MySql
            | Dialect::MariaDb
            | Dialect::SqlServer
            | Dialect::Oracle,
        ) => {
            format!("CONSTRAINT {} ", quote_identifier(name, d))
        }
        _ => String::new(),
    };
    // SPEC-0013 v8 behaviour 21: with the referential actions the database keeps.
    format!(
        "{constraint}FOREIGN KEY ({}) REFERENCES {} ({}){}",
        column_list(&f.columns, d),
        quote_identifier(&f.referenced_table, d),
        column_list(&f.referenced_columns, d),
        f.actions_sql()
    )
}

fn create_table(t: &Table, d: Dialect, with_fks: bool, name: &str) -> String {
    let mut parts: Vec<String> = t.columns.iter().map(|c| column_def(c, d)).collect();
    if !t.primary_key.is_empty() {
        parts.push(format!("PRIMARY KEY ({})", column_list(&t.primary_key, d)));
    }
    // Checks and unique constraints stay constraints (v6 behaviour 16).
    parts.extend(t.constraints.iter().map(|c| constraint_clause(c, d)));
    if with_fks {
        parts.extend(t.foreign_keys.iter().map(|f| foreign_key_clause(f, d)));
    }
    let create = format!(
        "CREATE TABLE {} (\n    {}\n)",
        quote_identifier(name, d),
        parts.join(",\n    ")
    );
    // SPEC-0013 v9 behaviour 27: MySQL and MariaDB keep the table's comment in its definition.
    let create = match t.comment.as_ref().filter(|_| d.mysql_like()) {
        Some(comment) => format!("{create} COMMENT={}", literal(&Some(comment.clone()), d)),
        None => create,
    };
    // v9 behaviour 21: with its storage parameters.
    if t.storage.is_empty() {
        create
    } else {
        format!("{create} WITH ({})", storage_list(&t.storage))
    }
}

/// Storage parameters (`key=value`, as the catalogue lists them) written for `WITH (…)` and `SET
/// (…)`: a key that is no plain (optionally `toast.`-prefixed) name is quoted, a value that is not a
/// plain word or number becomes a string literal.
fn storage_list(parameters: &[String]) -> String {
    parameters
        .iter()
        .map(|p| {
            let (key, value) = p.split_once('=').unwrap_or((p, ""));
            format!("{}={}", storage_key(key), storage_value(value))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn storage_key(key: &str) -> String {
    let plain = |part: &str| {
        part.chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    if key.split('.').count() <= 2 && key.split('.').all(plain) {
        key.to_owned()
    } else {
        quote_identifier(key, Dialect::PostgreSql)
    }
}

fn storage_value(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '+'))
    {
        value.to_owned()
    } else {
        literal(&Some(value.to_owned()), Dialect::PostgreSql)
    }
}

/// The statements that give table `table` (quoted) the source's storage parameters `to` where it has
/// `from`: one `SET` for every parameter the source has with another value or the target lacks, one
/// `RESET` for the parameters only the target has (v9 behaviour 21).
fn storage_statements(table: &str, from: &[String], to: &[String]) -> Vec<String> {
    let key = |p: &String| p.split_once('=').map_or(p.clone(), |(k, _)| k.to_owned());
    let set: Vec<String> = to.iter().filter(|p| !from.contains(p)).cloned().collect();
    let reset: Vec<String> = from
        .iter()
        .map(key)
        .filter(|k| !to.iter().any(|p| key(p) == *k))
        .collect();
    let mut out = Vec::new();
    if !set.is_empty() {
        out.push(format!("ALTER TABLE {table} SET ({})", storage_list(&set)));
    }
    if !reset.is_empty() {
        let keys: Vec<String> = reset.iter().map(|k| storage_key(k)).collect();
        out.push(format!("ALTER TABLE {table} RESET ({})", keys.join(", ")));
    }
    out
}

fn index_name(t: &Table, i: &Index) -> String {
    // SQLite reserves `sqlite_` names for its own indexes (UNIQUE constraints).
    if i.name.starts_with("sqlite_") {
        format!("{}_{}_key", t.name, i.columns.join("_"))
    } else {
        i.name.clone()
    }
}

fn create_index(t: &Table, i: &Index, d: Dialect) -> String {
    let parts: Vec<String> = (0..i.columns.len())
        .map(|n| index_term(t, i, n, d, true))
        .collect();
    // SPEC-0013 v8 behaviour 22: an access method other than the default one is kept.
    let method = match (&i.method, d) {
        (Some(m), Dialect::PostgreSql) => format!(" USING {m}"),
        _ => String::new(),
    };
    format!(
        "CREATE {}INDEX {} ON {}{method} ({}){}",
        if i.unique { "UNIQUE " } else { "" },
        quote_identifier(&index_name(t, i), d),
        quote_identifier(&t.name, d),
        parts.join(", "),
        i.condition
            .as_ref()
            // A condition that ends in a line comment must not swallow the closing `;`.
            .map(|c| format!(" WHERE {c}{}", if c.contains("--") { "\n" } else { "" }))
            .unwrap_or_default()
    )
}

/// Term `n` of index `i` of table `t` as SQL: a column quoted, an expression bracketed; on MySQL
/// and MariaDB with its prefix length (`SPEC-0013` v9 behaviour 25); with `options` also its
/// collation (v4 behaviour 15, PostgreSQL since v9) and sort order (v9 behaviour 24).
pub(crate) fn index_term(t: &Table, i: &Index, n: usize, d: Dialect, options: bool) -> String {
    let c = &i.columns[n];
    let mut term = if t.columns.iter().any(|col| &col.name == c) {
        quote_identifier(c, d)
    } else {
        format!("({c})")
    };
    if let Some(length) = i.prefix(n).filter(|_| d.mysql_like()) {
        term.push_str(&format!("({length})"));
    }
    if !options {
        return term;
    }
    if let Some(collation) = i.collation(n) {
        term.push_str(&format!(" COLLATE {}", quote_identifier(collation, d)));
    }
    if let Some(order) = i.order(n) {
        term.push(' ');
        term.push_str(order);
    }
    term
}

fn drop_index(t: &Table, i: &Index, d: Dialect) -> String {
    match d {
        Dialect::MySql | Dialect::MariaDb | Dialect::SqlServer => format!(
            "DROP INDEX {} ON {}",
            quote_identifier(&i.name, d),
            quote_identifier(&t.name, d)
        ),
        _ => format!("DROP INDEX {}", quote_identifier(&i.name, d)),
    }
}

fn add_foreign_key(t: &Table, f: &ForeignKey, d: Dialect) -> String {
    format!(
        "ALTER TABLE {} ADD {}",
        quote_identifier(&t.name, d),
        foreign_key_clause(f, d)
    )
}

fn drop_foreign_key(t: &Table, f: &ForeignKey, d: Dialect) -> Option<DdlStatement> {
    let name = quote_identifier(f.name.as_deref()?, d);
    let table = quote_identifier(&t.name, d);
    Some(keep(match d {
        Dialect::MySql | Dialect::MariaDb => {
            format!("ALTER TABLE {table} DROP FOREIGN KEY {name}")
        }
        _ => format!("ALTER TABLE {table} DROP CONSTRAINT {name}"),
    }))
}

/// `ADD COLUMN`, or `ADD` in T-SQL and Oracle.
fn add_column(d: Dialect) -> &'static str {
    if matches!(d, Dialect::SqlServer | Dialect::Oracle) {
        "ADD"
    } else {
        "ADD COLUMN"
    }
}

/// T-SQL that drops the default constraint of `table.column`, whatever the server named it. One
/// batch without `;`, so the variable lives through the `EXEC`.
fn drop_default_tsql(table: &str, column: &str) -> String {
    let t = table.replace('\'', "''");
    let c = column.replace('\'', "''");
    format!(
        "DECLARE @df sysname = (SELECT name FROM sys.default_constraints \
         WHERE parent_object_id = OBJECT_ID(N'{t}') \
         AND parent_column_id = COLUMNPROPERTY(OBJECT_ID(N'{t}'), N'{c}', 'ColumnId')) \
         IF @df IS NOT NULL EXEC(N'ALTER TABLE {} DROP CONSTRAINT [' + @df + N']')",
        quote_identifier(table, Dialect::SqlServer).replace('\'', "''")
    )
}

fn change_column(table_name: &str, change: &ColumnChange, d: Dialect) -> Vec<DdlStatement> {
    let table = &quote_identifier(table_name, d);
    let (from, to) = (&change.from, &change.to);
    let type_changed = !same_type(&from.data_type, &to.data_type);
    let conv = || {
        format!(
            "values of {table_name}.{} that do not convert to {}",
            to.name, to.data_type
        )
    };
    if d == Dialect::SqlServer {
        // T-SQL restates type and nullability together; defaults are constraints of their own.
        let mut out = Vec::new();
        if type_changed || from.nullable != to.nullable {
            let sql = format!(
                "ALTER TABLE {table} ALTER COLUMN {} {} {}",
                quote_identifier(&to.name, d),
                to.data_type,
                if to.nullable { "NULL" } else { "NOT NULL" }
            );
            out.push(if type_changed {
                lose(sql, conv())
            } else {
                keep(sql)
            });
        }
        if from.default.as_deref().map(str::trim) != to.default.as_deref().map(str::trim) {
            if from.default.is_some() {
                out.push(keep(drop_default_tsql(table_name, &to.name)));
            }
            if let Some(default) = &to.default {
                out.push(keep(format!(
                    "ALTER TABLE {table} ADD DEFAULT {default} FOR {}",
                    quote_identifier(&to.name, d)
                )));
            }
        }
        return out;
    }
    if d == Dialect::Oracle {
        // Oracle restates only what changed: repeating NOT NULL on a NOT NULL column is an error
        // (SPEC-0135); `DEFAULT NULL` removes a default.
        let mut parts = vec![quote_identifier(&to.name, d)];
        if type_changed {
            parts.push(to.data_type.clone());
        }
        if from.default.as_deref().map(str::trim) != to.default.as_deref().map(str::trim) {
            parts.push(format!(
                "DEFAULT {}",
                to.default.as_deref().unwrap_or("NULL")
            ));
        }
        if from.nullable != to.nullable {
            parts.push(if to.nullable { "NULL" } else { "NOT NULL" }.into());
        }
        if parts.len() == 1 {
            return Vec::new();
        }
        let sql = format!("ALTER TABLE {table} MODIFY ({})", parts.join(" "));
        return vec![if type_changed {
            lose(sql, conv())
        } else {
            keep(sql)
        }];
    }
    if d.mysql_like() {
        let sql = format!("ALTER TABLE {table} MODIFY COLUMN {}", column_def(to, d));
        return vec![if type_changed {
            lose(sql, conv())
        } else {
            keep(sql)
        }];
    }
    let column = format!(
        "ALTER TABLE {table} ALTER COLUMN {}",
        quote_identifier(&to.name, d)
    );
    let mut out = Vec::new();
    if type_changed {
        out.push(lose(format!("{column} TYPE {}", to.data_type), conv()));
    }
    if from.nullable != to.nullable {
        out.push(keep(format!(
            "{column} {} NOT NULL",
            if to.nullable { "DROP" } else { "SET" }
        )));
    }
    if from.default.as_deref().map(str::trim) != to.default.as_deref().map(str::trim) {
        out.push(keep(match &to.default {
            Some(default) => format!("{column} SET DEFAULT {default}"),
            None => format!("{column} DROP DEFAULT"),
        }));
    }
    out
}

/// SQLite: create the new table under a temporary name, copy the common
/// columns, drop the old table, rename, recreate the indexes.
fn rebuild_table(
    s: &Table,
    t: &Table,
    td: &TableDiff,
    d: Dialect,
    group: usize,
) -> Vec<DdlStatement> {
    let temp = format!("{}__idecraft_new", s.name);
    let common: Vec<String> = s
        .columns
        .iter()
        .filter(|c| t.columns.iter().any(|o| o.name == c.name))
        .map(|c| c.name.clone())
        .collect();
    let loses = !td.columns_removed.is_empty()
        || td
            .columns_changed
            .iter()
            .any(|c| !same_type(&c.from.data_type, &c.to.data_type));
    let list = column_list(&common, d);
    let mut out = vec![
        keep(create_table(s, d, true, &temp)),
        keep(format!(
            "INSERT INTO {} ({list}) SELECT {list} FROM {}",
            quote_identifier(&temp, d),
            quote_identifier(&s.name, d)
        )),
        keep(format!("DROP TABLE {}", quote_identifier(&s.name, d))),
        keep(format!(
            "ALTER TABLE {} RENAME TO {}",
            quote_identifier(&temp, d),
            quote_identifier(&s.name, d)
        )),
    ];
    out.extend(
        own_indexes(s, d)
            .into_iter()
            .map(|i| keep(create_index(s, i, d))),
    );
    // A rebuild that loses columns or converts values is chosen as a whole (v2 behaviour 13).
    if loses {
        let mut lost: Vec<String> = td.columns_removed.iter().map(|c| c.name.clone()).collect();
        lost.extend(
            td.columns_changed
                .iter()
                .filter(|c| !same_type(&c.from.data_type, &c.to.data_type))
                .map(|c| c.to.name.clone()),
        );
        let what = format!(
            "values of {}.{{{}}} (table rebuilt)",
            s.name,
            lost.join(", ")
        );
        for (i, st) in out.iter_mut().enumerate() {
            st.destructive = Some(what.clone());
            st.group = Some(group);
            st.data_loss = i == 2;
        }
    }
    out
}

/// Two schemas narrowed to one object each (v8 behaviour 20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Narrowed {
    /// The source object, named like the target object.
    pub source: Vec<Table>,
    /// The target object.
    pub target: Vec<Table>,
    /// The source view's definition when the objects are views.
    pub source_views: Vec<ViewDef>,
    /// The target view's definition.
    pub target_views: Vec<ViewDef>,
}

/// Narrows the schemas `(source, source_views)` and `(target, target_views)` to the objects
/// `source_object` and `target_object` (v8 behaviour 20): the source object takes the target
/// object's name, so [`compare_between`] and [`migration_between`] change the target object in
/// place and touch nothing else; a foreign key of the source object to itself follows the name.
/// `Err` names the side whose object is missing, or the two kinds when they differ.
pub fn narrow_to_objects(
    source: &[Table],
    source_views: &[ViewDef],
    source_object: &str,
    target: &[Table],
    target_views: &[ViewDef],
    target_object: &str,
) -> Result<Narrowed, String> {
    let find = |tables: &[Table], name: &str, side: &str| {
        tables
            .iter()
            .find(|t| t.name == name)
            .cloned()
            .ok_or_else(|| format!("the {side} has no table or view `{name}`"))
    };
    let mut from = find(source, source_object, "source")?;
    let to = find(target, target_object, "target")?;
    if from.kind != to.kind {
        return Err(format!(
            "`{source_object}` is a {} but `{target_object}` is a {}: only a table and a table, or a view and a view, are compared",
            from.kind, to.kind
        ));
    }
    from.name = to.name.clone();
    for fk in &mut from.foreign_keys {
        if fk.referenced_table == source_object {
            fk.referenced_table = to.name.clone();
        }
    }
    let view = |views: &[ViewDef], name: &str, as_name: &str| -> Vec<ViewDef> {
        views
            .iter()
            .filter(|v| v.name == name)
            .map(|v| ViewDef {
                name: as_name.to_owned(),
                definition: v.definition.clone(),
            })
            .collect()
    };
    Ok(Narrowed {
        source_views: view(source_views, source_object, &to.name),
        target_views: view(target_views, target_object, &to.name),
        source: vec![from],
        target: vec![to],
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn serial_defaults_name_their_sequence() {
        assert_eq!(
            super::sequence_of("nextval('scratch_note_id_seq'::regclass)").as_deref(),
            Some("scratch_note_id_seq")
        );
        assert_eq!(
            super::sequence_of("nextval('\"Odd\"''s_seq'::regclass)").as_deref(),
            Some("\"Odd\"'s_seq")
        );
        assert_eq!(super::sequence_of("0"), None);
    }

    use super::*;

    fn col(name: &str, data_type: &str, nullable: bool, default: Option<&str>) -> Column {
        Column {
            name: name.into(),
            data_type: data_type.into(),
            nullable,
            default: default.map(str::to_owned),
            generated: None,
            expression: None,
            identity: None,
            comment: None,
        }
    }

    fn index(name: &str, unique: bool, columns: &[&str]) -> Index {
        Index {
            name: name.into(),
            unique,
            columns: columns.iter().map(|c| c.to_string()).collect(),
            condition: None,
            collations: Vec::new(),
            method: None,
            orders: Vec::new(),
            prefixes: Vec::new(),
        }
    }

    fn fk(name: Option<&str>, column: &str, table: &str) -> ForeignKey {
        ForeignKey {
            name: name.map(str::to_owned),
            columns: vec![column.into()],
            referenced_table: table.into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }
    }

    fn table(name: &str, columns: Vec<Column>, pk: &[&str]) -> Table {
        Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: name.into(),
            kind: "table".into(),
            columns,
            primary_key: pk.iter().map(|c| c.to_string()).collect(),
            foreign_keys: vec![],
            indexes: vec![],
            unreadable: None,
            constraints: Vec::new(),
            comment: None,
        }
    }

    /// Target: warehouse(id, city), item(id, name), old(id).
    fn target() -> Vec<Table> {
        let mut item = table(
            "item",
            vec![
                col("id", "integer", false, None),
                col("name", "text", true, None),
                col("legacy", "text", true, None),
                col("wid", "integer", true, None),
            ],
            &["id"],
        );
        item.indexes = vec![
            index("item_pkey", true, &["id"]),
            index("item_name_old", false, &["name"]),
            index("item_legacy", false, &["legacy"]),
        ];
        item.foreign_keys = vec![fk(Some("item_old_fk"), "wid", "old")];
        vec![
            table(
                "warehouse",
                vec![
                    col("id", "integer", false, None),
                    col("city", "text", false, None),
                ],
                &["id"],
            ),
            item,
            table("old", vec![col("id", "integer", false, None)], &["id"]),
        ]
    }

    /// Source: item.name not null with default and type varchar, legacy gone,
    /// qty added, index renamed only, new index, FK to warehouse; old gone; tag added.
    fn source() -> Vec<Table> {
        let mut item = table(
            "item",
            vec![
                col("id", "integer", false, None),
                col("name", "varchar(20)", false, Some("'?'")),
                col("wid", "integer", true, None),
                col("qty", "integer", true, Some("0")),
            ],
            &["id"],
        );
        item.indexes = vec![
            index("item_pkey", true, &["id"]),
            index("item_name_new", false, &["name"]),
            index("item_qty", true, &["qty"]),
        ];
        item.foreign_keys = vec![fk(Some("item_wh_fk"), "wid", "warehouse")];
        let mut tag = table(
            "tag",
            vec![
                col("id", "integer", false, None),
                col("item_id", "integer", false, None),
            ],
            &["id"],
        );
        tag.foreign_keys = vec![fk(Some("tag_item_fk"), "item_id", "item")];
        vec![
            table(
                "warehouse",
                vec![
                    col("id", "INTEGER", false, None),
                    col("city", "text", false, None),
                ],
                &["id"],
            ),
            item,
            tag,
        ]
    }

    fn sql(statements: &[DdlStatement]) -> Vec<String> {
        statements
            .iter()
            .map(|s| format!("{}{}", if s.data_loss { "!" } else { "" }, s.sql))
            .collect()
    }

    #[test]
    fn lists_differences_and_ignores_renamed_identical_objects() {
        let diff = compare(&source(), &target(), Dialect::PostgreSql);
        assert_eq!(diff.added, ["tag"]);
        assert_eq!(diff.removed, ["old"]);
        assert_eq!(diff.changed.len(), 1, "warehouse only differs in type case");
        let item = &diff.changed[0];
        assert_eq!(item.columns_added[0].name, "qty");
        assert_eq!(item.columns_removed[0].name, "legacy");
        assert_eq!(item.columns_changed[0].to.name, "name");
        assert_eq!(item.primary_key, None);
        assert_eq!(item.indexes_added, [index("item_qty", true, &["qty"])]);
        assert_eq!(
            item.indexes_removed,
            [index("item_legacy", false, &["legacy"])]
        );
        assert_eq!(item.foreign_keys_added[0].referenced_table, "warehouse");
        assert_eq!(item.foreign_keys_removed[0].referenced_table, "old");
        assert!(compare(&source(), &source(), Dialect::MySql).is_empty());
    }

    #[test]
    fn postgres_migration_in_dependency_order() {
        assert_eq!(
            sql(&migration(&source(), &target(), Dialect::PostgreSql)),
            [
                "ALTER TABLE \"item\" DROP CONSTRAINT \"item_old_fk\"",
                "DROP INDEX \"item_legacy\"",
                "!DROP TABLE \"old\"",
                "CREATE TABLE \"tag\" (\n    \"id\" integer NOT NULL,\n    \"item_id\" integer NOT NULL,\n    PRIMARY KEY (\"id\")\n)",
                "ALTER TABLE \"item\" ADD COLUMN \"qty\" integer DEFAULT 0",
                "!ALTER TABLE \"item\" DROP COLUMN \"legacy\"",
                "!ALTER TABLE \"item\" ALTER COLUMN \"name\" TYPE varchar(20)",
                "ALTER TABLE \"item\" ALTER COLUMN \"name\" SET NOT NULL",
                "ALTER TABLE \"item\" ALTER COLUMN \"name\" SET DEFAULT '?'",
                "CREATE UNIQUE INDEX \"item_qty\" ON \"item\" (\"qty\")",
                "ALTER TABLE \"tag\" ADD CONSTRAINT \"tag_item_fk\" FOREIGN KEY (\"item_id\") REFERENCES \"item\" (\"id\")",
                "ALTER TABLE \"item\" ADD CONSTRAINT \"item_wh_fk\" FOREIGN KEY (\"wid\") REFERENCES \"warehouse\" (\"id\")",
            ]
        );
    }

    #[test]
    fn oracle_migration() {
        assert_eq!(
            sql(&migration(&source(), &target(), Dialect::Oracle)),
            [
                "ALTER TABLE \"item\" DROP CONSTRAINT \"item_old_fk\"",
                "DROP INDEX \"item_legacy\"",
                "!DROP TABLE \"old\"",
                "CREATE TABLE \"tag\" (\n    \"id\" integer NOT NULL,\n    \"item_id\" integer NOT NULL,\n    PRIMARY KEY (\"id\")\n)",
                "ALTER TABLE \"item\" ADD \"qty\" integer DEFAULT 0",
                "!ALTER TABLE \"item\" DROP COLUMN \"legacy\"",
                "!ALTER TABLE \"item\" MODIFY (\"name\" varchar(20) DEFAULT '?' NOT NULL)",
                "CREATE UNIQUE INDEX \"item_qty\" ON \"item\" (\"qty\")",
                "ALTER TABLE \"tag\" ADD CONSTRAINT \"tag_item_fk\" FOREIGN KEY (\"item_id\") REFERENCES \"item\" (\"id\")",
                "ALTER TABLE \"item\" ADD CONSTRAINT \"item_wh_fk\" FOREIGN KEY (\"wid\") REFERENCES \"warehouse\" (\"id\")",
            ]
        );
        // Only what changed is restated; a dropped default becomes `DEFAULT NULL`.
        let mut s = target();
        s[1].columns[1].default = Some("'x'".into());
        let t = target();
        let mut back = s.clone();
        back[1].columns[1].default = None;
        assert_eq!(
            sql(&migration(&back, &s, Dialect::Oracle)),
            ["ALTER TABLE \"item\" MODIFY (\"name\" DEFAULT NULL)"]
        );
        assert!(migration(&t, &target(), Dialect::Oracle).is_empty());
    }

    #[test]
    fn sql_server_migration() {
        assert_eq!(
            sql(&migration(&source(), &target(), Dialect::SqlServer)),
            [
                "ALTER TABLE [item] DROP CONSTRAINT [item_old_fk]",
                "DROP INDEX [item_legacy] ON [item]",
                "!DROP TABLE [old]",
                "CREATE TABLE [tag] (\n    [id] integer NOT NULL,\n    [item_id] integer NOT NULL,\n    PRIMARY KEY ([id])\n)",
                "ALTER TABLE [item] ADD [qty] integer DEFAULT 0",
                "!ALTER TABLE [item] DROP COLUMN [legacy]",
                "!ALTER TABLE [item] ALTER COLUMN [name] varchar(20) NOT NULL",
                "ALTER TABLE [item] ADD DEFAULT '?' FOR [name]",
                "CREATE UNIQUE INDEX [item_qty] ON [item] ([qty])",
                "ALTER TABLE [tag] ADD CONSTRAINT [tag_item_fk] FOREIGN KEY ([item_id]) REFERENCES [item] ([id])",
                "ALTER TABLE [item] ADD CONSTRAINT [item_wh_fk] FOREIGN KEY ([wid]) REFERENCES [warehouse] ([id])",
            ]
        );
    }

    #[test]
    fn sql_server_drops_default_constraints_by_lookup() {
        let mut s = source();
        let mut t = source();
        s[1].columns[3].default = Some("((1))".into());
        t[1].columns.push(col("gone", "int", true, Some("((0))")));
        let statements = sql(&migration(&s, &t, Dialect::SqlServer));
        assert_eq!(statements.len(), 4, "{statements:#?}");
        assert!(
            statements[0]
                .starts_with("DECLARE @df sysname = (SELECT name FROM sys.default_constraints")
        );
        assert!(statements[0].contains("OBJECT_ID(N'item'), N'gone'"));
        assert!(
            statements[0].ends_with("EXEC(N'ALTER TABLE [item] DROP CONSTRAINT [' + @df + N']')")
        );
        assert_eq!(statements[1], "!ALTER TABLE [item] DROP COLUMN [gone]");
        assert!(statements[2].contains("N'qty'"));
        assert_eq!(
            statements[3],
            "ALTER TABLE [item] ADD DEFAULT ((1)) FOR [qty]"
        );
        // The batch must survive the T-SQL splitter as one statement.
        assert_eq!(
            crate::split_statements(&statements[0], Dialect::SqlServer).len(),
            1
        );
    }

    #[test]
    fn mariadb_keeps_reported_default_expressions() {
        assert_eq!(default_expr("'abc'", Dialect::MariaDb), "'abc'");
        assert_eq!(
            default_expr("current_timestamp()", Dialect::MariaDb),
            "current_timestamp()"
        );
        let statements = sql(&migration(&source(), &target(), Dialect::MariaDb));
        assert_eq!(
            statements[0],
            "ALTER TABLE `item` DROP FOREIGN KEY `item_old_fk`"
        );
        assert!(statements.contains(&"DROP INDEX `item_legacy` ON `item`".to_owned()));
    }

    #[test]
    fn mysql_modifies_with_full_definition() {
        let statements = sql(&migration(&source(), &target(), Dialect::MySql));
        assert_eq!(
            statements[0],
            "ALTER TABLE `item` DROP FOREIGN KEY `item_old_fk`"
        );
        assert_eq!(statements[1], "DROP INDEX `item_legacy` ON `item`");
        assert!(statements.contains(
            &"!ALTER TABLE `item` MODIFY COLUMN `name` varchar(20) NOT NULL DEFAULT '''?'''".into()
        ));
        assert_eq!(
            default_expr("CURRENT_TIMESTAMP", Dialect::MySql),
            "CURRENT_TIMESTAMP"
        );
        assert_eq!(default_expr("1.5", Dialect::MySql), "1.5");
        assert_eq!(default_expr("abc", Dialect::MySql), "'abc'");
    }

    #[test]
    fn primary_key_changes() {
        let t = vec![table(
            "a",
            vec![col("x", "int", false, None), col("y", "int", false, None)],
            &["x"],
        )];
        let mut s = t.clone();
        s[0].primary_key = vec!["x".into(), "y".into()];
        let mut with_index = t.clone();
        with_index[0].indexes = vec![index("a_pk", true, &["x"])];
        assert_eq!(
            sql(&migration(&s, &with_index, Dialect::PostgreSql)),
            [
                "ALTER TABLE \"a\" DROP CONSTRAINT \"a_pk\"",
                "ALTER TABLE \"a\" ADD PRIMARY KEY (\"x\", \"y\")"
            ]
        );
        assert_eq!(
            sql(&migration(&s, &t, Dialect::MySql)),
            [
                "ALTER TABLE `a` DROP PRIMARY KEY",
                "ALTER TABLE `a` ADD PRIMARY KEY (`x`, `y`)"
            ]
        );
    }

    /// SPEC-0018 v9 AK-19 (OBS-0070 V5): storage parameters are compared, created with a new table,
    /// set and reset on a changed one, and shown with the table's DDL.
    #[test]
    fn storage_parameters_are_compared_and_written() {
        let plain = table("pd_move", vec![col("id", "integer", false, None)], &["id"]);
        let mut tuned = plain.clone();
        tuned.storage = vec![
            "autovacuum_enabled=false".into(),
            "toast.autovacuum_enabled=false".into(),
        ];
        let mut other = plain.clone();
        other.storage = vec!["autovacuum_enabled=true".into(), "fillfactor=70".into()];
        let d = Dialect::PostgreSql;
        // A new table carries them in its CREATE TABLE.
        assert_eq!(
            sql(&migration(std::slice::from_ref(&tuned), &[], d)),
            [
                "CREATE TABLE \"pd_move\" (\n    \"id\" integer NOT NULL,\n    PRIMARY KEY (\"id\")\n) \
              WITH (autovacuum_enabled=false, toast.autovacuum_enabled=false)"
            ]
        );
        // Equal parameters are no difference.
        assert!(
            compare(
                std::slice::from_ref(&tuned),
                std::slice::from_ref(&tuned),
                d
            )
            .is_empty()
        );
        // A changed table: SET for new and changed values, RESET for what only the target has.
        let diff = compare(
            std::slice::from_ref(&tuned),
            std::slice::from_ref(&other),
            d,
        );
        assert_eq!(
            diff.changed[0].storage,
            Some((other.storage.clone(), tuned.storage.clone()))
        );
        let statements = migration(
            std::slice::from_ref(&tuned),
            std::slice::from_ref(&other),
            d,
        );
        assert_eq!(
            sql(&statements),
            [
                "ALTER TABLE \"pd_move\" SET (autovacuum_enabled=false, toast.autovacuum_enabled=false)",
                "ALTER TABLE \"pd_move\" RESET (fillfactor)",
            ]
        );
        assert!(
            statements
                .iter()
                .all(|s| !s.data_loss && s.selected_by_default())
        );
        // Taking all of them back is one RESET.
        assert_eq!(
            sql(&migration(
                std::slice::from_ref(&plain),
                std::slice::from_ref(&tuned),
                d
            )),
            ["ALTER TABLE \"pd_move\" RESET (autovacuum_enabled, toast.autovacuum_enabled)"]
        );
        // The table's DDL shows them; an odd value becomes a literal, an odd key is quoted.
        let mut odd = plain.clone();
        odd.storage = vec!["Odd Key=a b".into()];
        assert!(
            object_ddl(&[odd], &[], "pd_move", d)
                .unwrap()
                .contains(") WITH (\"Odd Key\"='a b');")
        );
        // Serialised only when they differ.
        let json = serde_json::to_value(&diff).unwrap();
        assert_eq!(
            json["changed"][0]["storage"][1][0],
            "autovacuum_enabled=false"
        );
        let json = serde_json::to_value(compare(
            std::slice::from_ref(&plain),
            &[table(
                "pd_move",
                vec![col("id", "bigint", false, None)],
                &["id"],
            )],
            d,
        ))
        .unwrap();
        assert!(json["changed"][0].get("storage").is_none());
    }

    #[test]
    fn sqlite_adds_in_place_and_rebuilds_otherwise() {
        let t = vec![table("a", vec![col("id", "integer", false, None)], &["id"])];
        let mut s = t.clone();
        s[0].columns.push(col("n", "text", true, None));
        s[0].indexes
            .push(index("sqlite_autoindex_a_1", true, &["n"]));
        assert_eq!(
            sql(&migration(&s, &t, Dialect::Sqlite)),
            [
                "ALTER TABLE \"a\" ADD COLUMN \"n\" text",
                "CREATE UNIQUE INDEX \"a_n_key\" ON \"a\" (\"n\")"
            ]
        );
        let mut changed = s.clone();
        changed[0].columns[0].data_type = "bigint".into();
        changed[0].foreign_keys.push(fk(None, "n", "b"));
        assert_eq!(
            sql(&migration(&changed, &s, Dialect::Sqlite)),
            [
                "PRAGMA foreign_keys = OFF",
                "CREATE TABLE \"a__idecraft_new\" (\n    \"id\" bigint NOT NULL,\n    \"n\" text,\n    PRIMARY KEY (\"id\"),\n    FOREIGN KEY (\"n\") REFERENCES \"b\" (\"id\")\n)",
                "INSERT INTO \"a__idecraft_new\" (\"id\", \"n\") SELECT \"id\", \"n\" FROM \"a\"",
                "!DROP TABLE \"a\"",
                "ALTER TABLE \"a__idecraft_new\" RENAME TO \"a\"",
                "CREATE UNIQUE INDEX \"a_n_key\" ON \"a\" (\"n\")",
                "PRAGMA foreign_keys = ON",
            ]
        );
    }

    #[test]
    fn create_script_and_expression_indexes() {
        let mut t = table("a", vec![col("name", "text", true, None)], &[]);
        t.indexes.push(index("a_lower", false, &["lower(name)"]));
        t.foreign_keys.push(fk(Some("a_fk"), "name", "b"));
        assert_eq!(
            sql(&create_script(&[t], Dialect::PostgreSql)),
            [
                "CREATE TABLE \"a\" (\n    \"name\" text\n)",
                "CREATE INDEX \"a_lower\" ON \"a\" ((lower(name)))",
                "ALTER TABLE \"a\" ADD CONSTRAINT \"a_fk\" FOREIGN KEY (\"name\") REFERENCES \"b\" (\"id\")",
            ]
        );
    }

    fn vdef(name: &str, definition: &str) -> ViewDef {
        ViewDef {
            name: name.into(),
            definition: definition.into(),
        }
    }

    fn view_table(name: &str) -> Table {
        let mut v = table(name, vec![col("x", "integer", true, None)], &[]);
        v.kind = "view".into();
        v
    }

    /// SPEC-0018/AK-6: views on dropped tables go first; destructive statements are left out and
    /// chosen together with the views that use them.
    #[test]
    fn views_on_dropped_tables_are_dropped_first_and_left_out() {
        let a = table("a", vec![col("id", "integer", false, None)], &["id"]);
        let b = table("b", vec![col("id", "integer", false, None)], &["id"]);
        let keep_t = table("k", vec![col("id", "integer", false, None)], &["id"]);
        let target = vec![a, b, keep_t.clone(), view_table("v")];
        let tv = [vdef("v", "select a.id from a join b on b.id = a.id")];
        let st = migration_with_views(&[keep_t], &target, &[], &tv, Dialect::PostgreSql);
        assert_eq!(
            sql(&st),
            ["DROP VIEW \"v\"", "!DROP TABLE \"a\"", "!DROP TABLE \"b\""]
        );
        assert!(st.iter().all(|s| !s.selected_by_default()));
        assert_eq!(st[1].destructive.as_deref(), Some("all rows of table a"));
        assert!(
            st[0]
                .destructive
                .as_deref()
                .unwrap()
                .contains("holds no rows")
        );
        assert!(st[0].group.is_some() && st.iter().all(|s| s.group == st[0].group));
        let none = vec![false; 3];
        assert_eq!(toggle_statement(&st, &none, 2, true), [true, true, true]);
        let diff = compare_with_views(&[], &[], &[], &tv, Dialect::PostgreSql);
        assert_eq!(diff.views_removed, ["v"]);
        assert!(!diff.is_empty());
    }

    /// SPEC-0018/AK-7: a changed column used by a view; a changed view; views on views.
    /// Hand test 121: a column dropped from `item` that the view does not use leaves the view alone.
    #[test]
    fn views_that_do_not_use_a_dropped_column_stay() {
        let item = |extra: bool| {
            let mut columns = vec![
                col("sku", "text", false, None),
                col("unit_price_cents", "integer", false, None),
            ];
            if extra {
                columns.push(col("discontinued", "boolean", true, None));
            }
            vec![table("item", columns, &["sku"]), view_table("stock_value")]
        };
        let views = [vdef(
            "stock_value",
            "SELECT sum(s.qty * i.unit_price_cents) AS value_cents FROM item i",
        )];
        let st = migration_with_views(
            &item(false),
            &item(true),
            &views,
            &views,
            Dialect::PostgreSql,
        );
        assert_eq!(
            sql(&st),
            ["!ALTER TABLE \"item\" DROP COLUMN \"discontinued\""]
        );
        // A view selecting every column, or naming the dropped one, is still re-created.
        for def in ["SELECT i.* FROM item i", "SELECT discontinued FROM item"] {
            let views = [vdef("stock_value", def)];
            let st = migration_with_views(
                &item(false),
                &item(true),
                &views,
                &views,
                Dialect::PostgreSql,
            );
            assert_eq!(sql(&st)[0], "DROP VIEW \"stock_value\"", "{def}");
        }
    }

    /// SPEC-0018 v5 (hand test 121 retest): SQLite adds a column in place and leaves the views on
    /// the table alone; a change that rebuilds the table still re-creates them.
    #[test]
    fn sqlite_keeps_views_when_a_column_is_added_in_place() {
        let item = |extra: bool| {
            let mut columns = vec![
                col("sku", "text", false, None),
                col("unit_price_cents", "integer", false, None),
            ];
            if extra {
                columns.push(col("discontinued", "integer", true, None));
            }
            vec![table("item", columns, &["sku"]), view_table("stock_value")]
        };
        let views = [vdef(
            "stock_value",
            "SELECT i.sku, i.unit_price_cents FROM item i",
        )];
        let added =
            migration_with_views(&item(true), &item(false), &views, &views, Dialect::Sqlite);
        assert_eq!(
            sql(&added),
            ["ALTER TABLE \"item\" ADD COLUMN \"discontinued\" integer"]
        );
        let dropped =
            migration_with_views(&item(false), &item(true), &views, &views, Dialect::Sqlite);
        let s = sql(&dropped);
        assert!(s.iter().any(|l| l == "DROP VIEW \"stock_value\""), "{s:?}");
        assert!(
            s.iter()
                .any(|l| l.starts_with("CREATE VIEW \"stock_value\"")),
            "{s:?}"
        );
    }

    #[test]
    fn views_around_changed_columns_and_views_on_views() {
        let t = |ty: &str| {
            vec![
                table(
                    "t",
                    vec![col("id", "integer", false, None), col("x", ty, true, None)],
                    &["id"],
                ),
                view_table("v"),
                view_table("w"),
            ]
        };
        let views = [vdef("w", "select x from v"), vdef("v", "select x from t")];
        let st = migration_with_views(
            &t("bigint"),
            &t("integer"),
            &views,
            &views,
            Dialect::PostgreSql,
        );
        assert_eq!(
            sql(&st),
            [
                "DROP VIEW \"w\"",
                "DROP VIEW \"v\"",
                "!ALTER TABLE \"t\" ALTER COLUMN \"x\" TYPE bigint",
                "CREATE VIEW \"v\" AS select x from t",
                "CREATE VIEW \"w\" AS select x from v",
            ]
        );
        assert!(st[0].selected_by_default() && !st[2].selected_by_default());
        // Only an added column: views stay.
        let mut wider = t("integer");
        wider[0].columns.push(col("y", "integer", true, None));
        assert_eq!(
            sql(&migration_with_views(
                &wider,
                &t("integer"),
                &views,
                &views,
                Dialect::PostgreSql
            )),
            ["ALTER TABLE \"t\" ADD COLUMN \"y\" integer"]
        );
        // A changed view and what uses it are re-created; white space and case do not count.
        let changed = [
            vdef("w", "SELECT x  FROM v;"),
            vdef("v", "select x from t where x > 0"),
        ];
        let diff = compare_with_views(
            &t("integer"),
            &t("integer"),
            &changed,
            &views,
            Dialect::PostgreSql,
        );
        assert_eq!(diff.views_changed, ["v"]);
        assert_eq!(
            sql(&migration_with_views(
                &t("integer"),
                &t("integer"),
                &changed,
                &views,
                Dialect::PostgreSql
            )),
            [
                "DROP VIEW \"w\"",
                "DROP VIEW \"v\"",
                "CREATE VIEW \"v\" AS select x from t where x > 0",
                "CREATE VIEW \"w\" AS SELECT x  FROM v",
            ]
        );
        // SQLite: the rebuild's PRAGMA lines frame everything, views included.
        let st = migration_with_views(&t("bigint"), &t("integer"), &views, &views, Dialect::Sqlite);
        let text = sql(&st);
        assert_eq!(
            text.first().map(String::as_str),
            Some("PRAGMA foreign_keys = OFF")
        );
        assert_eq!(
            text.last().map(String::as_str),
            Some("PRAGMA foreign_keys = ON")
        );
        assert_eq!(text[1], "DROP VIEW \"w\"");
        let group = st[3].group;
        assert!(
            group.is_some() && !st[3].selected_by_default(),
            "the rebuild is one choice"
        );
        assert_eq!(st.iter().filter(|s| s.group == group).count(), 4);
    }

    /// SPEC-0018/AK-8: script text per dialect.
    #[test]
    fn script_is_transactional_where_possible() {
        let st = vec![
            keep("CREATE TABLE \"n\" (\"id\" integer)".into()),
            lose("DROP TABLE \"o\"".into(), "all rows of table o".into()),
        ];
        let defaults: Vec<bool> = st.iter().map(DdlStatement::selected_by_default).collect();
        assert_eq!(
            migration_script(&st, &defaults, Dialect::PostgreSql),
            "\nBEGIN;\n\nCREATE TABLE \"n\" (\"id\" integer);\n\n-- Left out (loses all rows of table o):\n-- DROP TABLE \"o\"\n\nCOMMIT;\n"
        );
        assert_eq!(
            migration_script(&st, &[true, true], Dialect::SqlServer),
            "\nBEGIN TRANSACTION;\n\nCREATE TABLE \"n\" (\"id\" integer);\n\n-- Can lose data: all rows of table o.\nDROP TABLE \"o\";\n\nCOMMIT;\n"
        );
        let mysql = migration_script(&st, &defaults, Dialect::MySql);
        assert!(mysql.starts_with("-- This database commits each DDL statement on its own"));
        assert!(!mysql.contains("BEGIN"));
        let mut sqlite = vec![keep("PRAGMA foreign_keys = OFF".into())];
        sqlite.extend(st.clone());
        sqlite.push(keep("PRAGMA foreign_keys = ON".into()));
        assert_eq!(
            migration_script(&sqlite, &[true, true, false, true], Dialect::Sqlite),
            "\nPRAGMA foreign_keys = OFF;\n\nBEGIN;\n\nCREATE TABLE \"n\" (\"id\" integer);\n\n-- Left out (loses all rows of table o):\n-- DROP TABLE \"o\"\n\nCOMMIT;\n\nPRAGMA foreign_keys = ON;\n"
        );
        assert_eq!(migration_script(&[], &[], Dialect::PostgreSql), "");
    }

    /// SPEC-0018 v7 AK-16 (behaviour 19, behaviour 14): the script's first statement pins its
    /// target; names in comments stay on one line.
    #[test]
    fn the_script_pins_its_target() {
        let st = vec![
            keep("CREATE TABLE \"n\" (\"id\" integer)".into()),
            lose("DROP TABLE \"o\"".into(), "all rows of table o".into()),
        ];
        let defaults: Vec<bool> = st.iter().map(DdlStatement::selected_by_default).collect();
        // PostgreSQL: inside the transaction, so it ends with COMMIT or ROLLBACK.
        assert_eq!(
            migration_script_in(&st, &defaults, Dialect::PostgreSql, Some("old")),
            "\nBEGIN;\nSET LOCAL search_path TO \"old\", pg_temp;\n\nCREATE TABLE \"n\" (\"id\" integer);\n\n-- Left out (loses all rows of table o):\n-- DROP TABLE \"o\"\n\nCOMMIT;\n"
        );
        let pg = migration_script_in(&st, &defaults, Dialect::PostgreSql, Some("public"));
        let first: Vec<String> = crate::split_statements(&pg, Dialect::PostgreSql)
            .into_iter()
            .take(2)
            .map(|s| s.text)
            .collect();
        assert_eq!(
            first,
            ["BEGIN", "SET LOCAL search_path TO \"public\", pg_temp"],
            "the current schema is pinned too"
        );
        assert_eq!(
            target_pin(Dialect::PostgreSql, "Audit \"x\""),
            Some("SET LOCAL search_path TO \"Audit \"\"x\"\"\", pg_temp".into())
        );
        // Session-wide pins come first, after a comment saying that they stay.
        let stays = "-- The next statement makes the target schema current for this session; it stays so after the script.\n";
        for (d, pin) in [
            (Dialect::DuckDb, "SET search_path TO \"old\";"),
            (Dialect::MySql, "USE `old`;"),
            (Dialect::MariaDb, "USE `old`;"),
            (
                Dialect::Oracle,
                "ALTER SESSION SET CURRENT_SCHEMA = \"old\";",
            ),
        ] {
            let script = migration_script_in(&st, &defaults, d, Some("old"));
            let lines: Vec<&str> = script.lines().collect();
            assert!(lines[0].starts_with("-- This database commits"), "{script}");
            assert_eq!(format!("{}\n", lines[1]), stays, "{d:?}");
            assert_eq!(lines[2], pin, "{d:?}");
            assert_eq!(
                crate::split_statements(&script, d)[0].text,
                pin.trim_end_matches(';'),
                "{d:?}: the first statement run"
            );
        }
        // No such statement on SQL Server and SQLite: the script is the one without a target.
        for d in [Dialect::SqlServer, Dialect::Sqlite] {
            assert_eq!(target_pin(d, "dbo"), None);
            assert_eq!(
                migration_script_in(&st, &defaults, d, Some("dbo")),
                migration_script(&st, &defaults, d)
            );
        }
        // Nothing to run, or no name: no pin.
        assert_eq!(
            migration_script_in(&st, &[false, false], Dialect::PostgreSql, Some("old")),
            migration_script(&st, &[false, false], Dialect::PostgreSql)
        );
        assert_eq!(
            migration_script_in(&[], &[], Dialect::MySql, Some("old")),
            ""
        );
        assert_eq!(target_pin(Dialect::MySql, ""), None);

        // A name with a line break cannot end a comment and become a statement.
        let odd = "x\nDROP TABLE important;--\r";
        let drop = format!("DROP TABLE {}", quote_identifier(odd, Dialect::PostgreSql));
        let st = vec![lose(drop.clone(), format!("all rows of table {odd}"))];
        let texts = |script: &str| -> Vec<String> {
            crate::split_statements(script, Dialect::PostgreSql)
                .into_iter()
                .map(|s| s.text)
                .collect()
        };
        // Left out: comment lines only (PostgreSQL also ends a `--` comment at `\r`).
        let left_out = migration_script_in(&st, &[false], Dialect::PostgreSql, Some("old"));
        assert!(!left_out.contains('\r'), "{left_out:?}");
        assert!(
            left_out
                .lines()
                .all(|l| l.is_empty() || l.starts_with("-- ")),
            "{left_out:?}"
        );
        assert!(texts(&left_out).is_empty(), "{left_out:?}");
        // Chosen: what it loses is one comment line; the statement keeps the exact name.
        let chosen = migration_script_in(&st, &[true], Dialect::PostgreSql, Some("old"));
        assert!(
            chosen.contains("\n-- Can lose data: all rows of table x DROP TABLE important;-- .\n"),
            "{chosen:?}"
        );
        assert_eq!(
            texts(&chosen),
            [
                "BEGIN",
                "SET LOCAL search_path TO \"old\", pg_temp",
                drop.as_str(),
                "COMMIT"
            ]
        );
        assert_eq!(comment_text("a\r\nb\nc"), "a  b c");
    }

    // SPEC-0089 v5 AK-9: the DDL of one table or view per dialect.
    fn shop() -> (Vec<Table>, Vec<ViewDef>) {
        let mut orders = table(
            "orders",
            vec![
                col(
                    "id",
                    "integer",
                    false,
                    Some("nextval('orders_id_seq'::regclass)"),
                ),
                col("customer_id", "integer", false, None),
                col("total", "numeric(10,2)", true, Some("0")),
            ],
            &["id"],
        );
        orders.foreign_keys = vec![fk(Some("orders_customer_fk"), "customer_id", "customers")];
        orders.indexes = vec![
            index("orders_pkey", true, &["id"]),
            index("orders_customer_idx", false, &["customer_id"]),
        ];
        let mut view = table("big_orders", vec![col("id", "integer", true, None)], &[]);
        view.kind = "view".into();
        let views = vec![ViewDef {
            name: "big_orders".into(),
            definition: " select id from orders where total > 100; ".into(),
        }];
        (vec![orders, view], views)
    }

    #[test]
    fn object_ddl_of_a_postgres_table() {
        let (tables, views) = shop();
        assert_eq!(
            object_ddl(&tables, &views, "orders", Dialect::PostgreSql).unwrap(),
            "CREATE TABLE \"orders\" (\n    \"id\" integer NOT NULL DEFAULT nextval('orders_id_seq'::regclass),\n    \"customer_id\" integer NOT NULL,\n    \"total\" numeric(10,2) DEFAULT 0,\n    PRIMARY KEY (\"id\"),\n    CONSTRAINT \"orders_customer_fk\" FOREIGN KEY (\"customer_id\") REFERENCES \"customers\" (\"id\")\n);\n\nCREATE INDEX \"orders_customer_idx\" ON \"orders\" (\"customer_id\");\n"
        );
        assert_eq!(
            object_ddl(&tables, &views, "nothing", Dialect::PostgreSql),
            None
        );
    }

    #[test]
    fn object_ddl_of_a_mysql_table() {
        let (mut tables, views) = shop();
        tables[0].columns[0].default = None;
        // MySQL's index of the foreign key carries the key's name and is not repeated.
        tables[0]
            .indexes
            .push(index("orders_customer_fk", false, &["customer_id"]));
        tables[0].indexes[0].name = "PRIMARY".into();
        tables[0]
            .columns
            .push(col("state", "varchar(10)", false, Some("new")));
        assert_eq!(
            object_ddl(&tables, &views, "orders", Dialect::MySql).unwrap(),
            "CREATE TABLE `orders` (\n    `id` integer NOT NULL,\n    `customer_id` integer NOT NULL,\n    `total` numeric(10,2) DEFAULT 0,\n    `state` varchar(10) NOT NULL DEFAULT 'new',\n    PRIMARY KEY (`id`),\n    CONSTRAINT `orders_customer_fk` FOREIGN KEY (`customer_id`) REFERENCES `customers` (`id`)\n);\n\nCREATE INDEX `orders_customer_idx` ON `orders` (`customer_id`);\n"
        );
    }

    #[test]
    fn object_ddl_of_a_sqlite_table_and_views() {
        let (mut tables, views) = shop();
        tables[0].columns[0].default = None;
        tables[0].foreign_keys[0].name = None;
        tables[0].indexes.remove(0);
        assert_eq!(
            object_ddl(&tables, &views, "orders", Dialect::Sqlite).unwrap(),
            "CREATE TABLE \"orders\" (\n    \"id\" integer NOT NULL,\n    \"customer_id\" integer NOT NULL,\n    \"total\" numeric(10,2) DEFAULT 0,\n    PRIMARY KEY (\"id\"),\n    FOREIGN KEY (\"customer_id\") REFERENCES \"customers\" (\"id\")\n);\n\nCREATE INDEX \"orders_customer_idx\" ON \"orders\" (\"customer_id\");\n"
        );
        assert_eq!(
            object_ddl(&tables, &views, "big_orders", Dialect::Sqlite).unwrap(),
            "CREATE VIEW \"big_orders\" AS\nselect id from orders where total > 100;\n"
        );
        assert_eq!(
            object_ddl(&tables, &[], "big_orders", Dialect::Sqlite).unwrap(),
            "-- The definition of view big_orders is not available.\n"
        );
    }

    /// SPEC-0013 v4 AK-10: a generated `CREATE INDEX` keeps the collation of a term and the
    /// condition of a partial index, a changed condition or collation is a difference, and a
    /// SPEC-0013 v8 AK-21: generated DDL keeps referential actions and PostgreSQL's access method;
    /// a changed action or method is a difference.
    #[test]
    fn generated_ddl_keeps_actions_and_methods() {
        let (mut tables, views) = shop();
        tables[0].foreign_keys[0].on_delete = Some("CASCADE".into());
        tables[0].foreign_keys[0].on_update = Some("SET NULL".into());
        tables[0].indexes[1].method = Some("gin".into());
        let ddl = object_ddl(&tables, &views, "orders", Dialect::PostgreSql).unwrap();
        assert!(
            ddl.contains("REFERENCES \"customers\" (\"id\") ON DELETE CASCADE ON UPDATE SET NULL"),
            "{ddl}"
        );
        assert!(
            ddl.contains(
                "CREATE INDEX \"orders_customer_idx\" ON \"orders\" USING gin (\"customer_id\")"
            ),
            "{ddl}"
        );
        // Other systems have no USING clause in this place; the actions stay.
        let mysql = object_ddl(&tables, &views, "orders", Dialect::MySql).unwrap();
        assert!(!mysql.contains("USING"), "{mysql}");
        assert!(
            mysql.contains("ON DELETE CASCADE ON UPDATE SET NULL"),
            "{mysql}"
        );
        let (plain, _) = shop();
        let diff = compare(&tables, &plain, Dialect::PostgreSql);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].foreign_keys_added.len(), 1);
        assert_eq!(diff.changed[0].foreign_keys_removed.len(), 1);
        assert_eq!(diff.changed[0].indexes_added.len(), 1);
    }

    /// SPEC-0013 v9 AK-22 (OBS-0076 D4, D5): generated `CREATE INDEX` keeps a term's collation,
    /// sort order and (on MySQL and MariaDB) prefix length; a changed order or prefix is a
    /// difference.
    #[test]
    fn generated_indexes_keep_order_and_prefix() {
        let (mut tables, views) = shop();
        tables[0].columns.push(col("note", "text", true, None));
        tables[0].indexes.push(Index {
            orders: vec![Some("DESC".into()), Some("NULLS FIRST".into())],
            collations: vec![None, Some("C".into())],
            ..index("orders_mixed", false, &["total", "note"])
        });
        let ddl = object_ddl(&tables, &views, "orders", Dialect::PostgreSql).unwrap();
        assert!(
            ddl.contains(
                "CREATE INDEX \"orders_mixed\" ON \"orders\" (\"total\" DESC, \"note\" COLLATE \"C\" NULLS FIRST);"
            ),
            "{ddl}"
        );
        let mut mysql = tables.clone();
        mysql[0].indexes[2] = Index {
            orders: vec![Some("DESC".into()), None],
            prefixes: vec![None, Some(20)],
            ..index("orders_mixed", false, &["total", "note"])
        };
        let ddl = object_ddl(&mysql, &views, "orders", Dialect::MySql).unwrap();
        assert!(
            ddl.contains("CREATE INDEX `orders_mixed` ON `orders` (`total` DESC, `note`(20));"),
            "{ddl}"
        );
        // Only MySQL and MariaDB have prefix lengths.
        let pg = object_ddl(&mysql, &views, "orders", Dialect::PostgreSql).unwrap();
        assert!(pg.contains("(\"total\" DESC, \"note\");"), "{pg}");
        let mut other = mysql.clone();
        other[0].indexes[2].prefixes = vec![None, Some(10)];
        let diff = compare(&mysql, &other, Dialect::MySql);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].indexes_added, [mysql[0].indexes[2].clone()]);
        other[0].indexes[2].prefixes = mysql[0].indexes[2].prefixes.clone();
        other[0].indexes[2].orders.clear();
        assert!(!compare(&mysql, &other, Dialect::MySql).is_empty());
        assert!(compare(&mysql, &mysql, Dialect::MySql).is_empty());
    }

    /// SPEC-0013 v9 AK-24 (OBS-0076 D6, D7): generated DDL writes identity, generated columns with
    /// their expression and comments as each system keeps them.
    #[test]
    fn generated_ddl_keeps_identity_generation_and_comments() {
        let (mut tables, views) = shop();
        let orders = &mut tables[0];
        orders.comment = Some("the shop's orders".into());
        orders.columns[0] = Column {
            identity: Some("GENERATED ALWAYS AS IDENTITY".into()),
            comment: Some("the key".into()),
            ..col("id", "integer", false, None)
        };
        orders.columns.push(Column {
            generated: Some("stored".into()),
            expression: Some("(total * 2)".into()),
            // A default the catalogue reports beside the expression is not written.
            default: Some("(total * 2)".into()),
            ..col("twice", "numeric", true, None)
        });
        orders.columns.push(Column {
            generated: Some("virtual".into()),
            expression: Some("total + 1".into()),
            ..col("more", "numeric", false, None)
        });
        let ddl = object_ddl(&tables, &views, "orders", Dialect::PostgreSql).unwrap();
        assert!(
            ddl.contains("\"id\" integer NOT NULL GENERATED ALWAYS AS IDENTITY,"),
            "{ddl}"
        );
        assert!(
            ddl.contains("\"twice\" numeric GENERATED ALWAYS AS (total * 2) STORED,"),
            "{ddl}"
        );
        assert!(
            ddl.contains("\"more\" numeric GENERATED ALWAYS AS (total + 1) VIRTUAL NOT NULL,"),
            "{ddl}"
        );
        assert!(
            ddl.ends_with(
                "\nCOMMENT ON TABLE \"orders\" IS 'the shop''s orders';\nCOMMENT ON COLUMN \"orders\".\"id\" IS 'the key';\n"
            ),
            "{ddl}"
        );
        let mut mysql = tables.clone();
        mysql[0].columns[0].identity = Some("AUTO_INCREMENT".into());
        let ddl = object_ddl(&mysql, &views, "orders", Dialect::MySql).unwrap();
        assert!(
            ddl.contains("`id` integer NOT NULL AUTO_INCREMENT COMMENT 'the key',"),
            "{ddl}"
        );
        assert!(ddl.contains("\n) COMMENT='the shop''s orders';\n"), "{ddl}");
        assert!(!ddl.contains("COMMENT ON"), "{ddl}");
        // Without the expression (SQLite) the column stays an ordinary one with the note.
        let mut sqlite = tables.clone();
        sqlite[0].columns[3].expression = None;
        let ddl = object_ddl(&sqlite, &views, "orders", Dialect::Sqlite).unwrap();
        assert!(
            ddl.contains("DEFAULT (total * 2) /* generated (stored)"),
            "{ddl}"
        );
        assert!(!ddl.contains("COMMENT"), "{ddl}");
    }

    /// SPEC-0135 v2 AK (OBS-0083 L2): Oracle takes a column's default and identity clause
    /// before `NOT NULL`, and its comments as `COMMENT ON TABLE` statements, views included.
    #[test]
    fn oracle_ddl_puts_default_and_identity_before_not_null() {
        let (mut tables, views) = shop();
        let orders = &mut tables[0];
        orders.comment = Some("the shop's orders".into());
        orders.columns[0] = Column {
            identity: Some("GENERATED BY DEFAULT ON NULL AS IDENTITY".into()),
            comment: Some("the key".into()),
            ..col("id", "NUMBER", false, None)
        };
        orders
            .columns
            .push(col("state", "VARCHAR2(10)", false, Some("'new'")));
        let ddl = object_ddl(&tables, &views, "orders", Dialect::Oracle).unwrap();
        assert!(
            ddl.contains("\"id\" NUMBER GENERATED BY DEFAULT ON NULL AS IDENTITY NOT NULL,"),
            "{ddl}"
        );
        assert!(
            ddl.contains("\"state\" VARCHAR2(10) DEFAULT 'new' NOT NULL"),
            "{ddl}"
        );
        assert!(!ddl.contains("NOT NULL DEFAULT"), "{ddl}");
        assert!(
            ddl.ends_with(
                "\nCOMMENT ON TABLE \"orders\" IS 'the shop''s orders';\nCOMMENT ON COLUMN \"orders\".\"id\" IS 'the key';\n"
            ),
            "{ddl}"
        );
    }

    /// unique partial index over the key's columns is not taken for the key's own index.
    #[test]
    fn generated_indexes_keep_collation_and_condition() {
        let (mut tables, views) = shop();
        tables[0].indexes[1].condition = Some("total > 0".into());
        tables[0].indexes[1].collations = vec![Some("nocase".into())];
        let ddl = object_ddl(&tables, &views, "orders", Dialect::Sqlite).unwrap();
        assert!(
            ddl.ends_with(
                "CREATE INDEX \"orders_customer_idx\" ON \"orders\" (\"customer_id\" COLLATE \"nocase\") WHERE total > 0;\n"
            ),
            "{ddl}"
        );
        let (plain, _) = shop();
        let diff = compare(&tables, &plain, Dialect::Sqlite);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(
            diff.changed[0].indexes_added,
            [tables[0].indexes[1].clone()]
        );
        assert_eq!(
            diff.changed[0].indexes_removed,
            [plain[0].indexes[1].clone()]
        );
        // The spelling of a collation's name is no difference.
        let mut upper = tables.clone();
        upper[0].indexes[1].collations = vec![Some("NOCASE".into())];
        assert!(compare(&tables, &upper, Dialect::Sqlite).is_empty());
        upper[0].indexes[1].collations.clear();
        assert!(!compare(&tables, &upper, Dialect::Sqlite).is_empty());
        tables[0].indexes[1].condition = Some("total > 0 -- paid".into());
        let ddl = object_ddl(&tables, &views, "orders", Dialect::Sqlite).unwrap();
        assert!(ddl.ends_with("WHERE total > 0 -- paid\n;\n"), "{ddl}");
        // A unique index over the key's columns that holds for some rows only is generated.
        let (mut tables, views) = shop();
        tables[0].indexes[0].condition = Some("total is null".into());
        let ddl = object_ddl(&tables, &views, "orders", Dialect::Sqlite).unwrap();
        assert!(
            ddl.contains(
                "CREATE UNIQUE INDEX \"orders_pkey\" ON \"orders\" (\"id\") WHERE total is null;"
            ),
            "{ddl}"
        );
    }
    fn check(name: &str, definition: &str) -> TableConstraint {
        TableConstraint {
            name: name.into(),
            kind: "check".into(),
            columns: Vec::new(),
            definition: definition.into(),
        }
    }

    fn unique(name: &str, columns: &[&str]) -> TableConstraint {
        TableConstraint {
            name: name.into(),
            kind: "unique".into(),
            columns: columns.iter().map(|c| c.to_string()).collect(),
            definition: format!("UNIQUE ({})", columns.join(", ")),
        }
    }

    /// `pd_note` of OBS-0068 with its check, its unique constraint (and the index behind it) and
    /// a plain index.
    fn pd_note() -> Table {
        let mut t = table(
            "pd_note",
            vec![
                col("id", "integer", false, None),
                col("body", "character varying(40)", false, None),
                col("amount", "integer", false, Some("0")),
            ],
            &["id"],
        );
        t.indexes = vec![
            index("pd_note_body_uq", true, &["body"]),
            index("pd_note_pkey", true, &["id"]),
            index("pd_note_amount_idx", false, &["amount"]),
        ];
        t.constraints = vec![
            check("pd_note_amount_check", "CHECK (amount >= 0)"),
            unique("pd_note_body_uq", &["body"]),
        ];
        t
    }

    /// SPEC-0018 v6 AK-12 (OBS-0068 L5).
    #[test]
    fn check_and_unique_constraints_stay_constraints() {
        let d = Dialect::PostgreSql;
        // A new table: both constraints inside CREATE TABLE, no index for the unique one.
        let statements = migration(&[pd_note()], &[], d);
        let sql = sql(&statements);
        assert_eq!(
            sql[0],
            "CREATE TABLE \"pd_note\" (\n    \"id\" integer NOT NULL,\n    \
             \"body\" character varying(40) NOT NULL,\n    \
             \"amount\" integer NOT NULL DEFAULT 0,\n    PRIMARY KEY (\"id\"),\n    \
             CONSTRAINT \"pd_note_amount_check\" CHECK (amount >= 0),\n    \
             CONSTRAINT \"pd_note_body_uq\" UNIQUE (body)\n)"
        );
        assert_eq!(
            sql[1..],
            ["CREATE INDEX \"pd_note_amount_idx\" ON \"pd_note\" (\"amount\")"]
        );
        // A table in both: a check only in the source is added after the columns, one only in the
        // target dropped right after the foreign keys; the unique constraint is no index difference.
        let mut target = pd_note();
        target.columns.push(col("old", "integer", true, None));
        target.constraints[0] = check("pd_note_old_check", "CHECK (old > 0)");
        target.foreign_keys = vec![fk(Some("pd_note_old_fkey"), "old", "other")];
        let diff = compare(&[pd_note()], &[target.clone()], d);
        let td = &diff.changed[0];
        assert_eq!(
            td.constraints_added,
            [check("pd_note_amount_check", "CHECK (amount >= 0)")]
        );
        assert_eq!(
            td.constraints_removed,
            [check("pd_note_old_check", "CHECK (old > 0)")]
        );
        assert!(
            td.indexes_added.is_empty() && td.indexes_removed.is_empty(),
            "{td:?}"
        );
        assert_eq!(
            sql_of(&migration(&[pd_note()], &[target], d)),
            [
                "ALTER TABLE \"pd_note\" DROP CONSTRAINT \"pd_note_old_fkey\"",
                "ALTER TABLE \"pd_note\" DROP CONSTRAINT \"pd_note_old_check\"",
                "ALTER TABLE \"pd_note\" DROP COLUMN \"old\"",
                "ALTER TABLE \"pd_note\" ADD CONSTRAINT \"pd_note_amount_check\" CHECK (amount >= 0)",
            ]
        );
        // Renamed but identical constraints, white space and case aside, are no difference.
        let mut renamed = pd_note();
        renamed.constraints = vec![
            check("other_name", "check  (AMOUNT >= 0)"),
            unique("pd_note_body_key", &["body"]),
        ];
        renamed.indexes[0].name = "pd_note_body_key".into();
        assert!(compare(&[pd_note()], &[renamed], d).is_empty());
        // A plain unique index against a unique constraint: the index goes, the constraint comes.
        let mut plain = pd_note();
        plain.constraints.retain(|c| !c.is_unique());
        plain.indexes[0].name = "pd_note_body_idx".into();
        assert_eq!(
            sql_of(&migration(&[pd_note()], &[plain], d)),
            [
                "DROP INDEX \"pd_note_body_idx\"",
                "ALTER TABLE \"pd_note\" ADD CONSTRAINT \"pd_note_body_uq\" UNIQUE (body)",
            ]
        );
    }

    fn sql_of(statements: &[DdlStatement]) -> Vec<String> {
        statements.iter().map(|s| s.sql.clone()).collect()
    }

    /// SPEC-0018 v6 AK-13 (OBS-0068 L6).
    #[test]
    fn a_left_out_table_drop_keeps_its_foreign_keys() {
        let d = Dialect::PostgreSql;
        let item = table("item", vec![col("id", "integer", false, None)], &["id"]);
        let mut a = table(
            "a",
            vec![
                col("id", "integer", false, None),
                col("item_id", "integer", true, None),
            ],
            &["id"],
        );
        a.foreign_keys = vec![fk(Some("a_item_fkey"), "item_id", "item")];
        let mut b = table(
            "b",
            vec![
                col("id", "integer", false, None),
                col("a_id", "integer", true, None),
            ],
            &["id"],
        );
        b.foreign_keys = vec![fk(Some("b_a_fkey"), "a_id", "a")];
        let c_source = table(
            "c",
            vec![
                col("id", "integer", false, None),
                col("b_id", "integer", true, None),
            ],
            &["id"],
        );
        let mut c_target = c_source.clone();
        c_target.foreign_keys = vec![fk(Some("c_b_fkey"), "b_id", "b")];
        let source = vec![item.clone(), c_source];
        let target = vec![item, a, b, c_target, view_table("a_view")];
        let views = vec![vdef("a_view", "select id from a")];
        let statements = migration_with_views(&source, &target, &[], &views, d);
        assert_eq!(
            sql_of(&statements),
            [
                "DROP VIEW \"a_view\"",
                "ALTER TABLE \"a\" DROP CONSTRAINT \"a_item_fkey\"",
                "ALTER TABLE \"b\" DROP CONSTRAINT \"b_a_fkey\"",
                "ALTER TABLE \"c\" DROP CONSTRAINT \"c_b_fkey\"",
                "DROP TABLE \"a\"",
                "DROP TABLE \"b\"",
            ]
        );
        // One group, nothing selected, only the tables lose data.
        let group = statements[0].group;
        assert!(group.is_some());
        for s in &statements {
            assert_eq!(s.group, group, "{}", s.sql);
            assert!(!s.selected_by_default(), "{}", s.sql);
            assert_eq!(s.data_loss, s.sql.starts_with("DROP TABLE"), "{}", s.sql);
        }
        assert_eq!(
            statements[3].destructive.as_deref(),
            Some("all rows of table b")
        );
        // The script with the defaults keeps every key: the drops are comment lines only.
        let defaults: Vec<bool> = statements.iter().map(|s| s.selected_by_default()).collect();
        let script = migration_script(&statements, &defaults, d);
        assert!(
            script
                .lines()
                .filter(|l| l.contains("DROP"))
                .all(|l| l.starts_with("-- ")),
            "{script}"
        );
        // Choosing one chooses them all, the foreign keys before the tables.
        let all = toggle_statement(&statements, &defaults, 5, true);
        assert!(all.iter().all(|s| *s));
        // A kept table's foreign key to a kept table, or a dropped table's when nothing else is
        // dropped with it, is grouped with nothing else.
        let lone = migration(
            &[table(
                "item",
                vec![col("id", "integer", false, None)],
                &["id"],
            )],
            &[
                table("item", vec![col("id", "integer", false, None)], &["id"]),
                {
                    let mut m = table("movement", vec![col("item_id", "integer", true, None)], &[]);
                    m.foreign_keys = vec![fk(Some("movement_item_fkey"), "item_id", "item")];
                    m
                },
            ],
            d,
        );
        assert_eq!(lone.len(), 2);
        assert_eq!(lone[0].group, lone[1].group);
        assert!(lone.iter().all(|s| !s.selected_by_default()));
        // SQLite: a rebuild of a kept table that loses its foreign key to a dropped table joins it.
        let mut kept = table(
            "c",
            vec![
                col("id", "integer", false, None),
                col("b_id", "integer", true, None),
            ],
            &["id"],
        );
        let plain_c = kept.clone();
        kept.foreign_keys = vec![fk(None, "b_id", "b")];
        let lite = migration(
            &[plain_c],
            &[
                kept,
                table("b", vec![col("id", "integer", false, None)], &["id"]),
            ],
            Dialect::Sqlite,
        );
        let drop_b = lite.iter().find(|s| s.sql == "DROP TABLE \"b\"").unwrap();
        assert!(drop_b.group.is_some());
        let rebuild: Vec<&DdlStatement> = lite
            .iter()
            .filter(|s| s.sql.contains("c__idecraft_new") || s.sql == "DROP TABLE \"c\"")
            .collect();
        assert!(!rebuild.is_empty());
        assert!(
            rebuild
                .iter()
                .all(|s| s.group == drop_b.group && !s.selected_by_default())
        );
    }

    /// SPEC-0018 v6 AK-14 (OBS-0068 L7, IMP-0415).
    #[test]
    fn view_bodies_move_to_the_target_schema() {
        let d = Dialect::PostgreSql;
        let item = table("item", vec![col("sku", "text", false, None)], &["sku"]);
        let source_views = vec![vdef(
            "pd_item_names",
            "SELECT item.sku FROM pd_b.item JOIN \"pd_b\".w ON true JOIN PD_B.x ON true \
             JOIN other.item o ON true WHERE item.sku <> 'pd_b.item' -- pd_b.item\n",
        )];
        let source = vec![item.clone(), view_table("pd_item_names")];
        let schemas = Some(("pd_b", "public"));
        let statements = migration_between(
            &source,
            std::slice::from_ref(&item),
            &source_views,
            &[],
            d,
            schemas,
        );
        assert_eq!(
            statements.last().unwrap().sql,
            "CREATE VIEW \"pd_item_names\" AS SELECT item.sku FROM public.item JOIN \"public\".w ON true \
             JOIN public.x ON true JOIN other.item o ON true WHERE item.sku <> 'pd_b.item' -- pd_b.item"
        );
        // Without the schemas the body is kept as it is.
        let plain =
            migration_with_views(&source, std::slice::from_ref(&item), &source_views, &[], d);
        assert!(plain.last().unwrap().sql.contains("FROM pd_b.item"));
        // Compared between the schemas, the target's own view on `item` is the same view; one on
        // another schema's table is not.
        let source_views = vec![vdef("v", "SELECT item.sku FROM pd_b.item")];
        let views = vec![view_table("v"), item.clone()];
        for (target_view, same) in [
            ("SELECT item.sku FROM item", true),
            ("SELECT item.sku FROM public.item", true),
            ("SELECT item.sku FROM pd_b.item", false),
            ("SELECT item.sku FROM other.item", false),
        ] {
            let diff = compare_between(
                &views,
                &views,
                &source_views,
                &[vdef("v", target_view)],
                d,
                schemas,
            );
            assert_eq!(diff.is_empty(), same, "{target_view}: {diff:?}");
        }
        // A source view on the target schema's table is the target's own view there.
        let diff = compare_between(
            &views,
            &views,
            &[vdef("v", "SELECT item.sku FROM public.item")],
            &[vdef("v", "SELECT item.sku FROM item")],
            d,
            schemas,
        );
        assert!(diff.is_empty(), "{diff:?}");
    }

    /// SPEC-0018 v8 AK-18 (OBS-0068 L8): two single objects.
    #[test]
    fn two_single_tables_are_compared() {
        let mut copy = table(
            "item_copy",
            vec![
                col("id", "integer", false, None),
                col("name", "text", true, None),
                col("extra", "text", true, None),
            ],
            &["id"],
        );
        copy.foreign_keys = vec![fk(Some("copy_parent"), "parent", "item_copy")];
        let mut item = table(
            "item",
            vec![
                col("id", "integer", false, None),
                col("name", "text", true, None),
                col("parent", "integer", true, None),
            ],
            &["id"],
        );
        item.foreign_keys = vec![fk(Some("copy_parent"), "parent", "item")];
        let source = vec![
            item,
            table("other", vec![col("id", "integer", false, None)], &["id"]),
        ];
        let target = vec![
            copy,
            table(
                "only_target",
                vec![col("id", "integer", false, None)],
                &["id"],
            ),
        ];
        let n = narrow_to_objects(&source, &[], "item", &target, &[], "item_copy").unwrap();
        assert_eq!(n.source[0].name, "item_copy");
        assert_eq!(n.source[0].foreign_keys[0].referenced_table, "item_copy");
        let d = Dialect::PostgreSql;
        let diff = compare_between(&n.source, &n.target, &[], &[], d, None);
        assert!(diff.added.is_empty() && diff.removed.is_empty(), "{diff:?}");
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].name, "item_copy");
        assert_eq!(diff.changed[0].columns_added[0].name, "parent");
        assert_eq!(diff.changed[0].columns_removed[0].name, "extra");
        assert!(
            diff.changed[0].foreign_keys_added.is_empty(),
            "the self reference follows"
        );
        let statements = migration_between(&n.source, &n.target, &[], &[], d, None);
        assert!(
            statements.iter().all(|s| s.sql.contains("\"item_copy\"")),
            "{statements:?}"
        );
        assert!(
            !statements
                .iter()
                .any(|s| s.sql.contains("other") || s.sql.contains("only_target"))
        );
        let drop = statements
            .iter()
            .find(|s| s.sql.contains("DROP COLUMN"))
            .unwrap();
        assert!(drop.data_loss && !drop.selected_by_default());

        // Views with views.
        let mut v1 = table("v_a", vec![col("id", "integer", true, None)], &[]);
        v1.kind = "view".into();
        let mut v2 = v1.clone();
        v2.name = "v_b".into();
        let defs_a = [ViewDef {
            name: "v_a".into(),
            definition: "select 1 as id".into(),
        }];
        let defs_b = [ViewDef {
            name: "v_b".into(),
            definition: "select 2 as id".into(),
        }];
        let n = narrow_to_objects(&[v1.clone()], &defs_a, "v_a", &[v2.clone()], &defs_b, "v_b")
            .unwrap();
        let diff = compare_between(
            &n.source,
            &n.target,
            &n.source_views,
            &n.target_views,
            d,
            None,
        );
        assert_eq!(diff.views_changed, ["v_b"]);

        // Refusals.
        assert_eq!(
            narrow_to_objects(&source, &[], "nope", &target, &[], "item_copy").unwrap_err(),
            "the source has no table or view `nope`"
        );
        assert_eq!(
            narrow_to_objects(&source, &[], "item", &target, &[], "nope").unwrap_err(),
            "the target has no table or view `nope`"
        );
        assert!(
            narrow_to_objects(&source, &[], "item", &[v2], &defs_b, "v_b")
                .unwrap_err()
                .starts_with("`item` is a table but `v_b` is a view")
        );
    }
}
