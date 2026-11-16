//! AI permissions and statement classes (`SPEC-0012`, IMP-0007).
//!
//! Every statement is classified before it runs; a connection grants each of
//! four permissions separately. Anything unknown counts as a schema change,
//! the strictest class.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::script::code_words;
use crate::{Dialect, Statement};

/// One thing an agent may be allowed to do on a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    /// Inspect tables, columns, keys.
    ReadSchema,
    /// Run queries that only read.
    ReadData,
    /// Insert, update, delete, call routines.
    ModifyData,
    /// Create, alter, drop and everything not known to be harmless.
    ModifySchema,
}

impl Permission {
    /// The name used in profile files and messages.
    pub fn name(self) -> &'static str {
        match self {
            Permission::ReadSchema => "read-schema",
            Permission::ReadData => "read-data",
            Permission::ModifyData => "modify-data",
            Permission::ModifySchema => "modify-schema",
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Permissions granted when a profile says nothing: reading only.
pub fn default_permissions() -> BTreeSet<Permission> {
    [Permission::ReadSchema, Permission::ReadData].into()
}

/// What a statement does, as far as permissions are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementClass {
    /// Only reads.
    ReadData,
    /// Changes rows or runs code.
    ModifyData,
    /// Changes the schema, or is not known to be harmless.
    ModifySchema,
}

impl StatementClass {
    /// Permission needed to run a statement of this class.
    pub fn required(self) -> Permission {
        match self {
            StatementClass::ReadData => Permission::ReadData,
            StatementClass::ModifyData => Permission::ModifyData,
            StatementClass::ModifySchema => Permission::ModifySchema,
        }
    }
}

const DML: &[&str] = &["insert", "update", "delete", "merge"];

/// Classifies one statement (SPEC-0012 behaviour 2).
pub fn classify(sql: &str, dialect: Dialect) -> StatementClass {
    let words = code_words(sql, dialect);
    let Some(first) = words.first() else {
        return StatementClass::ReadData;
    };
    // `BEGIN` opens a PL/SQL block in Oracle and a T-SQL block unless `BEGIN TRAN[SACTION]`
    // follows; such a block can do anything (SPEC-0142).
    if first == "begin" {
        let second = words.get(1).map(String::as_str);
        let block = match dialect {
            Dialect::Oracle => true,
            Dialect::SqlServer => !matches!(second, Some("tran" | "transaction" | "distributed")),
            _ => false,
        };
        if block {
            return StatementClass::ModifySchema;
        }
    }
    match first.as_str() {
        "select" | "values" | "table" | "show" | "describe" | "desc" | "begin" | "start"
        | "commit" | "rollback" | "savepoint" | "release" | "end" => StatementClass::ReadData,
        "explain" => {
            if words
                .iter()
                .skip(1)
                .take(3)
                .any(|w| w == "analyze" || w == "analyse")
            {
                StatementClass::ModifyData
            } else {
                StatementClass::ReadData
            }
        }
        "with" => {
            if words.iter().any(|w| DML.contains(&w.as_str())) {
                StatementClass::ModifyData
            } else {
                StatementClass::ReadData
            }
        }
        "insert" | "update" | "delete" | "merge" | "replace" | "upsert" | "copy" | "call"
        | "exec" | "execute" | "do" | "lock" => StatementClass::ModifyData,
        "pragma" if !sql.contains('=') && !sql.contains('(') => StatementClass::ReadData,
        _ => StatementClass::ModifySchema,
    }
}

/// Functions that change settings, sequences or large objects, or reach files and other servers:
/// a query that calls one is not a read for an agent (`SPEC-0012` v3 behaviour 2).
const CHANGING_FUNCTIONS: &[&str] = &[
    "set_config",
    "setval",
    "nextval",
    "lo_import",
    "lo_export",
    "lo_unlink",
    "dblink",
    "dblink_exec",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "load_file",
    "load_extension",
    "readfile",
    "writefile",
];

/// `sql` without the comments and blanks it starts with.
fn without_leading_comments(sql: &str) -> &str {
    let mut rest = sql.trim_start();
    loop {
        if let Some(line) = rest.strip_prefix("--") {
            rest = line.split_once('\n').map_or("", |(_, after)| after);
        } else if let Some(block) = rest.strip_prefix("/*") {
            rest = block.split_once("*/").map_or("", |(_, after)| after);
        } else {
            return rest;
        }
        rest = rest.trim_start();
    }
}

/// Classifies one statement an agent wants to run (`SPEC-0012` v3 behaviour 2): [`classify`], and
/// on top a reading statement counts as a schema change — the strictest class — when it contains
/// `INTO` (it creates a table or writes a file), calls one of [`CHANGING_FUNCTIONS`], or has text
/// but no keyword that can be read (an unterminated literal, punctuation alone).
pub fn classify_for_agent(sql: &str, dialect: Dialect) -> StatementClass {
    let class = classify(sql, dialect);
    if class != StatementClass::ReadData {
        return class;
    }
    let words = code_words(sql, dialect);
    if words.is_empty() {
        return if without_leading_comments(sql).is_empty() {
            StatementClass::ReadData
        } else {
            StatementClass::ModifySchema
        };
    }
    let changing = words
        .iter()
        .any(|w| w == "into" || CHANGING_FUNCTIONS.contains(&w.as_str()));
    if changing {
        StatementClass::ModifySchema
    } else {
        StatementClass::ReadData
    }
}

/// A statement that needs a permission the connection does not grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// 1-based position of the statement in the script.
    pub index: usize,
    /// What the statement does.
    pub class: StatementClass,
    /// The permission that is missing.
    pub missing: Permission,
}

/// Returns every statement of `statements` that `granted` does not allow an agent
/// ([`classify_for_agent`]).
pub fn check_permissions(
    statements: &[Statement],
    dialect: Dialect,
    granted: &BTreeSet<Permission>,
) -> Vec<Refusal> {
    statements
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let class = classify_for_agent(&s.text, dialect);
            let missing = class.required();
            (!granted.contains(&missing)).then_some(Refusal {
                index: i + 1,
                class,
                missing,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split_statements;

    fn class(sql: &str) -> StatementClass {
        classify(sql, Dialect::PostgreSql)
    }

    /// SPEC-0012 v3 AK-1: what counts as a change for an agent on top of the classes.
    #[test]
    fn agents_get_the_stricter_class() {
        use StatementClass::*;
        let agent = |sql: &str| classify_for_agent(sql, Dialect::PostgreSql);
        for sql in [
            "select * into t2 from t",
            "select a from t into outfile '/tmp/x'",
            "select set_config('default_transaction_read_only', 'off', false)",
            "with x as (select nextval('s')) select * from x",
            "select pg_read_file('/etc/passwd')",
            "select load_extension('x')",
            "'",
            ";;(",
            "\"unterminated",
        ] {
            assert_eq!(agent(sql), ModifySchema, "{sql}");
            // The read-only guard of a connection keeps its classes.
        }
        assert_eq!(class("select * into t2 from t"), ReadData);
        for sql in [
            "select 1",
            "select 'into'",
            "select \"into\" from t",
            "-- into\nselect 1",
            "-- a comment alone",
            "/* a block */  ",
            "",
            "explain select 1",
            "begin",
        ] {
            assert_eq!(agent(sql), ReadData, "{sql}");
        }
        assert_eq!(agent("insert into t values (1)"), ModifyData);
        assert_eq!(agent("explain analyze select 1"), ModifyData);
        assert_eq!(agent("create table t (v int)"), ModifySchema);
        let statements = split_statements("select 1; select * into t2 from t", Dialect::PostgreSql);
        let refused = check_permissions(&statements, Dialect::PostgreSql, &default_permissions());
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].index, 2);
        assert_eq!(refused[0].missing, Permission::ModifySchema);
    }

    #[test]
    fn classifies_statements() {
        use StatementClass::*;
        for sql in [
            "select 1",
            "with x as (select 1) select * from x",
            "explain select 1",
            "begin",
            "-- delete\nselect 'drop table t'",
            "pragma table_info(t)",
        ] {
            let expected = if sql.starts_with("pragma") {
                // `(` makes a pragma call; treated as potentially changing.
                ModifySchema
            } else {
                ReadData
            };
            assert_eq!(class(sql), expected, "{sql}");
        }
        for sql in [
            "with x as (delete from t returning *) select * from x",
            "insert into t values (1)",
            "explain analyze select 1",
            "call p()",
        ] {
            assert_eq!(class(sql), ModifyData, "{sql}");
        }
        for sql in [
            "create table t (v int)",
            "drop table t",
            "set x = 1",
            "frobnicate",
        ] {
            assert_eq!(class(sql), ModifySchema, "{sql}");
        }
        assert_eq!(classify("pragma user_version", Dialect::Sqlite), ReadData);
        // Blocks are not transactions (SPEC-0142).
        assert_eq!(
            classify("begin delete from t; end;", Dialect::Oracle),
            ModifySchema
        );
        assert_eq!(
            classify("begin delete from t end", Dialect::SqlServer),
            ModifySchema
        );
        assert_eq!(classify("begin transaction", Dialect::SqlServer), ReadData);
        assert_eq!(classify("begin", Dialect::PostgreSql), ReadData);
        assert_eq!(
            classify("pragma user_version = 3", Dialect::Sqlite),
            ModifySchema
        );
    }

    #[test]
    fn refusals_name_statement_class_and_permission() {
        let statements = split_statements("select 1; delete from t", Dialect::PostgreSql);
        assert_eq!(
            check_permissions(&statements, Dialect::PostgreSql, &default_permissions()),
            [Refusal {
                index: 2,
                class: StatementClass::ModifyData,
                missing: Permission::ModifyData,
            }]
        );
        let mut all = default_permissions();
        all.insert(Permission::ModifyData);
        assert!(check_permissions(&statements, Dialect::PostgreSql, &all).is_empty());
    }
}
