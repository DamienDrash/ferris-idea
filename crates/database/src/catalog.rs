//! Catalog questions beside introspection: the schemas of a database (`SPEC-0089` v3, IMP-0052),
//! the default schema of a new console (`SPEC-0192`, IMP-0060), a fingerprint telling whether
//! cached metadata is still current (`SPEC-0014` v2, IMP-0057), and row counts for confirming
//! unsafe changes (`SPEC-0192`, IMP-0062).

use serde::Serialize;

use crate::introspect::query;
use crate::{DbError, Dialect, Session, is_system_schema};

/// One schema of the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaInfo {
    /// Schema name.
    pub name: String,
    /// Whether the session works in it.
    pub current: bool,
}

/// The non-system schemas of the session's database, current one marked, sorted by name.
pub async fn list_schemas(session: &Session) -> Result<Vec<SchemaInfo>, DbError> {
    let dialect = session.dialect();
    let rows = query(session, schemas_query(session)).await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let name = row.first().cloned().flatten()?;
            let current = matches!(
                row.get(1).cloned().flatten().as_deref(),
                Some("1" | "t" | "true" | "TRUE")
            );
            (current || !is_system_schema(dialect, &name)).then_some(SchemaInfo { name, current })
        })
        .collect())
}

/// Whether the session's database has schema `name`, a system schema too (`SPEC-0013` v9
/// behaviour 28). PostgreSQL and DuckDB compare names exactly, the others ignoring case.
pub(crate) async fn schema_exists(session: &Session, name: &str) -> Result<bool, DbError> {
    let exact = matches!(session.dialect(), Dialect::PostgreSql | Dialect::DuckDb);
    let rows = query(session, schemas_query(session)).await?;
    Ok(rows.iter().any(|row| {
        row.first().cloned().flatten().is_some_and(|found| {
            if exact {
                found == name
            } else {
                found.eq_ignore_ascii_case(name)
            }
        })
    }))
}

/// The catalogue query of the schemas: name, `1` for the current one.
fn schemas_query(session: &Session) -> &str {
    let custom = session
        .system()
        .and_then(|c| c.introspection.schemas.as_deref());
    custom.unwrap_or(match session.dialect() {
        Dialect::PostgreSql => {
            "select nspname, case when nspname = current_schema() then '1' else '0' end \
             from pg_namespace order by 1"
        }
        Dialect::MySql | Dialect::MariaDb => {
            "select SCHEMA_NAME, case when SCHEMA_NAME = database() then '1' else '0' end \
             from information_schema.SCHEMATA order by 1"
        }
        Dialect::SqlServer => {
            "select name, case when schema_id = schema_id() then '1' else '0' end \
             from sys.schemas order by 1"
        }
        Dialect::Sqlite => {
            "select name, case when name = 'main' then '1' else '0' end \
             from pragma_database_list order by name"
        }
        Dialect::Oracle => "SELECT sys_context('USERENV', 'CURRENT_SCHEMA'), '1' FROM dual",
        Dialect::DuckDb => {
            "select schema_name, case when schema_name = current_schema() then '1' else '0' end \
             from information_schema.schemata where catalog_name = current_database() order by 1"
        }
    })
}

/// The schema a new session lands in: the default schema of the data source.
pub async fn current_schema(session: &Session) -> Result<String, DbError> {
    if let Some(sql) = session.system().and_then(|c| c.sql.current_schema.clone()) {
        return first_value(session, &sql).await;
    }
    let sql = match session.dialect() {
        Dialect::PostgreSql => "select current_schema()",
        Dialect::MySql | Dialect::MariaDb => "select database()",
        Dialect::SqlServer => "select schema_name()",
        Dialect::Sqlite => return Ok("main".into()),
        Dialect::Oracle => "SELECT sys_context('USERENV', 'CURRENT_SCHEMA') FROM dual",
        Dialect::DuckDb => "select current_schema()",
    };
    first_value(session, sql).await
}

/// A short text that changes whenever a table, view or column of the current schema is added,
/// dropped, renamed or retyped.
pub async fn schema_fingerprint(session: &Session) -> Result<String, DbError> {
    let sql = match session.dialect() {
        Dialect::PostgreSql => {
            "select count(*) || ':' || coalesce(md5(string_agg(c.relname || '.' || a.attname || ':' \
               || a.atttypid || ':' || a.atttypmod, ',' order by c.relname, a.attnum)), '') \
             from pg_class c join pg_namespace n on n.oid = c.relnamespace \
             join pg_attribute a on a.attrelid = c.oid and a.attnum > 0 and not a.attisdropped \
             where n.nspname = current_schema() and c.relkind in ('r', 'p', 'v', 'm')"
        }
        Dialect::MySql | Dialect::MariaDb => {
            "select concat(count(*), ':', coalesce(sum(crc32(concat(TABLE_NAME, '.', COLUMN_NAME, ':', \
               COLUMN_TYPE, ':', ORDINAL_POSITION))), 0)) \
             from information_schema.COLUMNS where TABLE_SCHEMA = database()"
        }
        Dialect::SqlServer => {
            "select cast(count(*) as varchar(20)) + ':' + cast(isnull(checksum_agg(checksum(t.name, \
               c.name, c.user_type_id, c.max_length, c.column_id)), 0) as varchar(20)) \
             from sys.objects t join sys.columns c on c.object_id = t.object_id \
             where t.type in ('U', 'V') and t.schema_id = schema_id()"
        }
        Dialect::Sqlite => "select schema_version from pragma_schema_version",
        Dialect::DuckDb => {
            "select count(*) || ':' || coalesce(md5(string_agg(table_name || '.' || column_name \
               || ':' || data_type || ':' || ordinal_position, ',' order by table_name, \
               ordinal_position)), '') from information_schema.columns \
             where table_schema = current_schema() and table_catalog = current_database()"
        }
        Dialect::Oracle => {
            "SELECT COUNT(*) || ':' || NVL(SUM(ORA_HASH(table_name || '.' || column_name || ':' \
               || data_type || ':' || column_id)), 0) FROM user_tab_columns"
        }
    };
    first_value(session, sql).await
}

/// Number of rows of `table`, written as in the statement (schema and quotes kept).
pub async fn count_rows(session: &Session, table: &str) -> Result<u64, DbError> {
    let value = first_value(session, &format!("select count(*) from {table}")).await?;
    value.trim().parse().map_err(|_| DbError {
        message: format!("row count `{value}` is no number"),
        sqlstate: None,
        position: None,
    })
}

async fn first_value(session: &Session, sql: &str) -> Result<String, DbError> {
    let rows = query(session, sql).await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|r| r.into_iter().next())
        .flatten()
        .unwrap_or_default())
}
