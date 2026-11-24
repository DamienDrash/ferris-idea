//! Database tooling: dialects, connection profiles and SQL script handling.
//!
//! Implements `SPEC-0005` (splitting an SQL script into statements) and
//! `SPEC-0007` (script outcomes, error positions, stable order, value
//! summaries), `SPEC-0008` (connection profiles) and `SPEC-0009`–`SPEC-0011`
//! (PostgreSQL, MySQL and SQLite sessions), `SPEC-0013` (introspection) and
//! `SPEC-0014` (completion), `SPEC-0015` (inspections) `SPEC-0017` (data editing) and `SPEC-0018` (schema comparison, DDL) `SPEC-0019` (renaming in SQL) `SPEC-0020` (query plans) `SPEC-0021` (data export), `SPEC-0190` (table data editor queries), `SPEC-0191` (copying a table), `SPEC-0022` (ER model), `SPEC-0116` (MariaDB, SQL Server), `SPEC-0300` (connectors), `SPEC-0301` (problem reports), `SPEC-0302` (sharing connectors) and the catalog questions of `SPEC-0014` v2, `SPEC-0089` v3 and `SPEC-0192`.

mod agent_files;
mod catalog;
mod completion;
mod connection_form;
pub mod connector;
pub mod connector_share;
mod copy_table;
mod ddl;
mod duckdb;
mod edit;
mod er;
pub mod er_editors;
pub mod er_layout;
pub mod er_route;
mod export;
pub mod grid;
mod grid_edit;
mod history;
mod idea_sources;
mod inspect;
mod introspect;
mod mysql;
mod object_doc;
mod oracle;
mod order;
mod outcome;
mod permission;
mod plan;
mod postgres;
mod profile;
mod refactor;
pub mod report;
mod schema_objects;
mod script;
mod secret;
mod session;
mod sql_colours;
mod sql_files;
pub mod sql_format;
mod sqlite;
mod sqlserver;
mod summary;
mod table_query;
mod tds_tls;
mod tunnel;

pub use agent_files::refused_file_access;
pub use catalog::{SchemaInfo, count_rows, current_schema, list_schemas, schema_fingerprint};
pub use completion::{
    Candidate, CandidateKind, SchemaEntry, complete, complete_for, complete_in, complete_in_schema,
    is_system_schema, wanted_schema,
};
pub use connection_form::{
    ConnectionExtras, ConnectionForm, ConnectionTest, FileScope, KEYCHAIN_SERVICE,
    create_database_file, driver_label, remove, remove_connection, save_connection, secret_ref,
    test_connection, test_connection_for_agent, test_form, unique_id, upsert, with_tls_hint,
};
pub use connector::{Catalogue, Connector, catalogue, reload_catalogue};
pub use copy_table::{
    CopyOptions, CopyReport, CopySource, CopyTarget, copy_statements, copy_table, read_source,
    sequence_note, sqlite_checks, target_exists,
};
pub use ddl::{
    ColumnChange, DdlStatement, Narrowed, SchemaDiff, TableDiff, comment_text, compare,
    compare_between, compare_with_views, create_script, migration, migration_between,
    migration_script, migration_script_in, migration_with_views, narrow_to_objects, object_ddl,
    target_pin, toggle_statement,
};
pub use duckdb::DuckDbSession;
pub use edit::{
    Applied, Change, EditError, EditSet, EditTarget, Failure, Isolation, Values, begin_sql,
    begin_statements, editable_source, literal,
};
pub use er::{
    Dependency, Entity, ErColumn, ErError, ErModel, Relationship, er_model, er_model_with_views,
    to_dot, to_mermaid, to_plantuml, view_dependencies,
};
pub use export::{ExportError, ExportFormat, ExportOptions, export};
pub use grid_edit::{CellState, GridEdits};
pub use history::{HISTORY_LIMIT, HistoryEntry, QueryHistory, history_file, keeps_secret};
pub use idea_sources::{idea_source_form, idea_source_note, is_added};
pub use inspect::{Fix, Problem, Severity, inspect};
pub use introspect::{
    Column, ConstraintName, ForeignKey, Index, ObjectDdl, Table, TableConstraint, UNKNOWN_SCHEMA,
    ViewDef, constraint_names, introspect, introspect_schema, sequence_query, sequences, show_ddl,
    show_ddl_in, view_body, view_definitions, view_definitions_in,
};
pub use mysql::MySqlSession;
pub use object_doc::object_doc;
pub use oracle::{OracleSession, client_dir as oracle_client_dir};
pub use order::{Direction, SortKey, StableOrder, order_by_clause, quote_identifier, stable_order};
pub use outcome::{
    DbError, OnError, Outcome, ScriptRun, StatementOutcome, error_offset, run_script,
};
pub use permission::{
    Permission, Refusal, StatementClass, check_permissions, classify, classify_for_agent,
    default_permissions,
};
pub use plan::{
    ExplainError, NON_TRANSACTIONAL_HINT, Plan, PlanNode, Property, explain, fill_steps, hints,
    parse_mysql, parse_postgres, parse_sqlite, step_hints, wrapped,
};
pub use postgres::PgSession;
pub use profile::{
    ConnectionColor, ConnectionOptions, ConnectionProfile, PROFILE_FILE, ProfileError, SecretRef,
    SshTunnel, SslMode, TlsFiles, expand_home, glob_match, is_pkcs12, load_profiles, save_profiles,
    target_line, validate_all,
};
pub use refactor::{
    Ambiguity, DerivedRename, RefactorNote, RefactorRun, Refactoring, Rename, RenameError,
    SqlObject, TextEdit, WayBack, apply_edits, change_type_ddl, ddl_in_transaction,
    derived_name_edits, derived_renames, expand_wildcard, file_dialect, inline_cte, inverse_ddl,
    named_object_at, object_at, refactor, refactoring_notes, rename, rename_constraint, rename_ddl,
    rename_index, rename_routine, rename_sequence, rename_sequence_in, run_failure,
    run_refactoring, way_back,
};
pub use schema_objects::{
    CatalogPart, NO_DEFINITION_PRIVILEGE, SchemaObject, definition_from_rows, definition_query,
    object_definition, object_kinds, object_queries, objects_from_rows, schema_objects,
    sqlite_trigger_event,
};
pub use script::{Statement, block_comments, split_statements};
#[cfg(target_os = "linux")]
pub use secret::SecretServiceStore;
#[cfg(debug_assertions)]
pub use secret::test_store;
pub use secret::{
    Credentials, FormerKeychainStore, KeychainStore, KeyringStore, MemoryStore, RenamedStore,
    Secret, SecretError, SecretStore, SystemStore, UNREACHABLE as KEYCHAIN_UNREACHABLE,
    VAULT_KEY_SERVICE, current_secret_ref, is_vault_key_service, keychain_reachable,
    renamed_services, resolve_credentials,
};
pub use session::{
    CONNECT_TIMEOUT, CancelHandle, Driver, Session, SessionError, TlsPlan, is_loopback, tls_plan,
};
pub use sql_colours::{SqlColour, dialect_colours};
pub use sql_files::{
    CONSOLES_DIR, SQL_FILES, bind_file, bound_connection, create_console, next_console,
};
pub use sqlite::SqliteSession;
pub use sqlserver::SqlServerSession;
pub use summary::{Scope, ValueSummary, summarize_loaded};
pub use table_query::{ClauseError, TABLE_PAGE, TableQuery, check_clause, order_text, page_count};
pub use tunnel::{TUNNEL_BIND, Tunnel, TunnelError, tunnel_line};

/// SQL dialects targeted first (M5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    /// PostgreSQL
    PostgreSql,
    /// MySQL
    MySql,
    /// SQLite
    Sqlite,
    /// MariaDB (`SPEC-0116`): the MySQL protocol and lexis, its own catalog and plans.
    MariaDb,
    /// Microsoft SQL Server (`SPEC-0116`): T-SQL over TDS.
    SqlServer,
    /// Oracle Database (`SPEC-0135`, `SPEC-0142`): SQL and PL/SQL through Oracle Instant Client.
    Oracle,
    /// DuckDB (`SPEC-0304`): an embedded analytical database file, PostgreSQL-like lexis.
    #[serde(rename = "duckdb")]
    DuckDb,
}

impl Dialect {
    /// Every dialect, in the order the connection dialog offers them.
    pub const ALL: [Dialect; 7] = [
        Dialect::PostgreSql,
        Dialect::MySql,
        Dialect::MariaDb,
        Dialect::SqlServer,
        Dialect::Oracle,
        Dialect::Sqlite,
        Dialect::DuckDb,
    ];

    /// MySQL or MariaDB: backticks, backslash escapes, `#` comments.
    pub fn mysql_like(self) -> bool {
        matches!(self, Dialect::MySql | Dialect::MariaDb)
    }

    /// Name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Dialect::PostgreSql => "PostgreSQL",
            Dialect::MySql => "MySQL",
            Dialect::MariaDb => "MariaDB",
            Dialect::SqlServer => "SQL Server",
            Dialect::Oracle => "Oracle",
            Dialect::Sqlite => "SQLite",
            Dialect::DuckDb => "DuckDB",
        }
    }

    /// Whether a statement makes another schema current in a session, so two schemas of one
    /// connection can be compared (`SPEC-0110` v7 behaviour 16).
    pub fn switches_schema(self) -> bool {
        matches!(
            self,
            Dialect::PostgreSql | Dialect::DuckDb | Dialect::MySql | Dialect::MariaDb
        )
    }

    /// PostgreSQL or DuckDB: dollar-quoted strings and the PostgreSQL lexis.
    pub fn postgres_like(self) -> bool {
        matches!(self, Dialect::PostgreSql | Dialect::DuckDb)
    }

    /// A database in a local file (SQLite, DuckDB): no server fields, no password.
    pub fn file_based(self) -> bool {
        matches!(self, Dialect::Sqlite | Dialect::DuckDb)
    }
}
