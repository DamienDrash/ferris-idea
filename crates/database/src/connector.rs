//! Database connectors (`SPEC-0300`, ADR-0020): one database system on top of a protocol family,
//! described by a declarative, versioned TOML definition. The built-in systems are such files,
//! compiled in; users add more in their connectors folder. A definition is data only — it never
//! names anything IDECraft would execute or fetch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use serde::Serialize;

use crate::{Dialect, SslMode};

/// Version of the definition format this build reads.
pub const FORMAT: i64 = 1;
/// Largest definition file read (behaviour 4).
pub const MAX_DEFINITION_BYTES: usize = 64 * 1024;
/// Largest SQL text in a definition.
const MAX_SQL_BYTES: usize = 16 * 1024;

/// The built-in definitions, in catalogue order inside each category.
const BUILT_IN: &[(&str, &str)] = &[
    ("postgresql", include_str!("../connectors/postgresql.toml")),
    ("mysql", include_str!("../connectors/mysql.toml")),
    ("mariadb", include_str!("../connectors/mariadb.toml")),
    ("sqlserver", include_str!("../connectors/sqlserver.toml")),
    ("oracle", include_str!("../connectors/oracle.toml")),
    ("sqlite", include_str!("../connectors/sqlite.toml")),
    ("duckdb", include_str!("../connectors/duckdb.toml")),
    (
        "cockroachdb",
        include_str!("../connectors/cockroachdb.toml"),
    ),
    ("yugabytedb", include_str!("../connectors/yugabytedb.toml")),
    ("tidb", include_str!("../connectors/tidb.toml")),
    (
        "timescaledb",
        include_str!("../connectors/timescaledb.toml"),
    ),
    ("greenplum", include_str!("../connectors/greenplum.toml")),
    (
        "materialize",
        include_str!("../connectors/materialize.toml"),
    ),
    ("redshift", include_str!("../connectors/redshift.toml")),
    (
        "aurora-postgresql",
        include_str!("../connectors/aurora-postgresql.toml"),
    ),
    ("alloydb", include_str!("../connectors/alloydb.toml")),
    ("neon", include_str!("../connectors/neon.toml")),
    ("supabase", include_str!("../connectors/supabase.toml")),
    (
        "aurora-mysql",
        include_str!("../connectors/aurora-mysql.toml"),
    ),
    (
        "planetscale",
        include_str!("../connectors/planetscale.toml"),
    ),
    (
        "singlestore",
        include_str!("../connectors/singlestore.toml"),
    ),
    ("starrocks", include_str!("../connectors/starrocks.toml")),
    ("doris", include_str!("../connectors/doris.toml")),
    ("azure-sql", include_str!("../connectors/azure-sql.toml")),
    (
        "azure-sql-mi",
        include_str!("../connectors/azure-sql-mi.toml"),
    ),
    ("synapse", include_str!("../connectors/synapse.toml")),
    ("clickhouse", include_str!("../connectors/clickhouse.toml")),
    ("db2", include_str!("../connectors/db2.toml")),
    ("sap-ase", include_str!("../connectors/sap-ase.toml")),
    ("exasol", include_str!("../connectors/exasol.toml")),
    ("vertica", include_str!("../connectors/vertica.toml")),
    ("hive", include_str!("../connectors/hive.toml")),
    ("snowflake", include_str!("../connectors/snowflake.toml")),
    ("bigquery", include_str!("../connectors/bigquery.toml")),
    // Non-relational stores (ADR-0022, SPEC-0310).
    ("mongodb", include_str!("../connectors/mongodb.toml")),
    ("documentdb", include_str!("../connectors/documentdb.toml")),
    ("couchbase", include_str!("../connectors/couchbase.toml")),
    ("redis", include_str!("../connectors/redis.toml")),
    ("valkey", include_str!("../connectors/valkey.toml")),
    ("dynamodb", include_str!("../connectors/dynamodb.toml")),
    ("cassandra", include_str!("../connectors/cassandra.toml")),
];

/// URL schemes the families keep for themselves.
const FAMILY_SCHEMES: &[&str] = &[
    "postgresql",
    "postgres",
    "mysql",
    "mariadb",
    "sqlserver",
    "mssql",
    "oracle",
    "sqlite",
    "duckdb",
    "mongodb",
    "mongodb+srv",
    "redis",
    "rediss",
];

/// Protocol family of a connector: an IDECraft driver, or the generic ODBC route (ADR-0020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// One of IDECraft's own drivers.
    Native(Dialect),
    /// Through an ODBC driver manager; not connectable yet.
    Odbc,
    /// A non-relational store family of `idecraft-nosql` (ADR-0022): `mongodb` or `redis`.
    /// Connected through store profiles (SPEC-0310), not through SQL sessions.
    Store(&'static str),
    /// A non-relational system planned in ADR-0022 (`cassandra`, `dynamodb`, `couchbase`);
    /// listed, not connectable yet.
    Planned(&'static str),
    /// A WebAssembly connector plugin (`SPEC-0314`, ADR-0024) speaks the protocol; the dialect
    /// is the SQL family whose lexis profiles and the editor use. No native session.
    Plugin(Dialect),
}

impl Serialize for Family {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl Family {
    /// The dialect of profiles and sessions, `None` for routes that are not built yet; for a
    /// plugin connector the SQL family of its lexis (it has no native session, `SPEC-0314`).
    pub fn dialect(self) -> Option<Dialect> {
        match self {
            Family::Native(d) | Family::Plugin(d) => Some(d),
            Family::Odbc | Family::Store(_) | Family::Planned(_) => None,
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "postgresql" => Family::Native(Dialect::PostgreSql),
            "mysql" => Family::Native(Dialect::MySql),
            "mariadb" => Family::Native(Dialect::MariaDb),
            "sqlserver" => Family::Native(Dialect::SqlServer),
            "oracle" => Family::Native(Dialect::Oracle),
            "sqlite" => Family::Native(Dialect::Sqlite),
            "duckdb" => Family::Native(Dialect::DuckDb),
            "odbc" => Family::Odbc,
            "mongodb" => Family::Store("mongodb"),
            "redis" => Family::Store("redis"),
            "cassandra" => Family::Planned("cassandra"),
            "dynamodb" => Family::Planned("dynamodb"),
            "couchbase" => Family::Planned("couchbase"),
            // The lexis family comes from `[plugin] lexis` (`SPEC-0314` behaviour 2).
            "plugin" => Family::Plugin(Dialect::PostgreSql),
            _ => return None,
        })
    }

    /// Name as written in definitions.
    pub fn name(self) -> &'static str {
        match self {
            Family::Native(d) => family_name(d),
            Family::Odbc => "odbc",
            Family::Store(n) | Family::Planned(n) => n,
            Family::Plugin(_) => "plugin",
        }
    }
}

/// The family name of `dialect` as written in definitions and profiles.
pub fn family_name(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::PostgreSql => "postgresql",
        Dialect::MySql => "mysql",
        Dialect::MariaDb => "mariadb",
        Dialect::SqlServer => "sqlserver",
        Dialect::Oracle => "oracle",
        Dialect::Sqlite => "sqlite",
        Dialect::DuckDb => "duckdb",
    }
}

/// Group of the New Data Source list (behaviour 5), in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// Classic relational servers.
    Relational,
    /// Distributed SQL databases.
    DistributedSql,
    /// Managed cloud services.
    Cloud,
    /// Analytical databases and warehouses.
    Analytics,
    /// Time-series databases.
    TimeSeries,
    /// Databases inside the process or in a file.
    Embedded,
    /// Anything else.
    Other,
    /// Document stores (ADR-0022).
    Document,
    /// Key-value stores (ADR-0022).
    KeyValue,
    /// Wide-column stores (ADR-0022).
    WideColumn,
}

impl Category {
    /// Every category in display order.
    pub const ALL: [Category; 10] = [
        Category::Relational,
        Category::DistributedSql,
        Category::Cloud,
        Category::Analytics,
        Category::TimeSeries,
        Category::Embedded,
        Category::Other,
        Category::Document,
        Category::KeyValue,
        Category::WideColumn,
    ];

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "relational" => Category::Relational,
            "distributed" | "distributed-sql" => Category::DistributedSql,
            "cloud" => Category::Cloud,
            "analytics" => Category::Analytics,
            "time-series" | "timeseries" => Category::TimeSeries,
            "embedded" => Category::Embedded,
            "other" => Category::Other,
            "document" => Category::Document,
            "key-value" => Category::KeyValue,
            "wide-column" => Category::WideColumn,
            _ => return None,
        })
    }

    /// Heading shown in the list.
    pub fn label(self) -> &'static str {
        match self {
            Category::Relational => "Relational",
            Category::DistributedSql => "Distributed SQL",
            Category::Cloud => "Cloud",
            Category::Analytics => "Analytics",
            Category::TimeSeries => "Time series",
            Category::Embedded => "Embedded",
            Category::Other => "Other",
            Category::Document => "Document",
            Category::KeyValue => "Key-value",
            Category::WideColumn => "Wide column",
        }
    }
}

/// How far IDECraft vouches for a system (behaviour 4, ADR-0020 decision 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Built in and tested against a real server.
    Supported,
    /// Built in, not testable here.
    Beta,
    /// A file the user put into the connectors folder.
    User,
    /// A file brought in by import (SPEC-0302).
    Imported,
}

impl Status {
    /// Badge text in the list; none for supported systems.
    pub fn badge(self) -> Option<&'static str> {
        match self {
            Status::Supported => None,
            Status::Beta => Some("Beta"),
            Status::User => Some("User"),
            Status::Imported => Some("Imported"),
        }
    }
}

/// Where a definition came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "path")]
pub enum Origin {
    /// Compiled into IDECraft.
    BuiltIn,
    /// A file in the connectors folder written or copied by the user.
    User(PathBuf),
    /// A file stored by import.
    Imported(PathBuf),
}

/// A field of the connection form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    /// Server host.
    Host,
    /// Server port.
    Port,
    /// Database name.
    Database,
    /// User name.
    User,
    /// Password (kept in the keychain).
    Password,
    /// SSL mode.
    Ssl,
    /// Database file.
    File,
}

impl Field {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "host" => Field::Host,
            "port" => Field::Port,
            "database" => Field::Database,
            "user" => Field::User,
            "password" => Field::Password,
            "ssl" => Field::Ssl,
            "file" => Field::File,
            _ => return None,
        })
    }
}

/// How a plan statement's result is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PlanFormat {
    /// The family's own plan reader.
    #[default]
    Family,
    /// Each result row is one line of a text plan.
    Text,
}

/// `[connection]` of a definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ConnectionSpec {
    /// URL schemes that select the connector.
    pub schemes: Vec<String>,
    /// Port used when the profile names none.
    pub default_port: Option<u16>,
    /// Database proposed for a new profile.
    pub default_database: Option<String>,
    /// User proposed for a new profile.
    pub default_user: Option<String>,
    /// SSL mode proposed for a new profile.
    pub ssl: Option<SslMode>,
    /// Fields of the connection form.
    pub fields: Vec<Field>,
}

/// `[sql]` of a definition; `None` inherits from the family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct SqlSpec {
    /// Character that opens a quoted identifier.
    pub identifier_quote: Option<char>,
    /// Completion keywords added to the family's.
    pub keywords: Vec<String>,
    /// Estimate statement with `{statement}`.
    pub explain: Option<String>,
    /// Measuring statement with `{statement}`.
    pub explain_analyze: Option<String>,
    /// How the plan result is read.
    pub plan_format: PlanFormat,
    /// Statement of Test Connection.
    pub test_query: Option<String>,
    /// Statement answering the server version.
    pub version_query: Option<String>,
    /// Statement answering the current schema.
    pub current_schema: Option<String>,
}

/// `[introspection]` of a definition; `None` inherits from the family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct IntrospectionSpec {
    /// Schemas: name, is current, table count.
    pub schemas: Option<String>,
    /// Columns: table, kind, column, type, not null, default, position, pk position.
    pub columns: Option<String>,
    /// Keys: table, constraint, `p`/`f`, columns, referenced table, referenced columns.
    pub keys: Option<String>,
    /// Indexes: table, index, unique, columns.
    pub indexes: Option<String>,
    /// Views: name, definition.
    pub views: Option<String>,
}

/// One database system (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connector {
    /// Stable id, also the profile's `system`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Protocol family.
    pub family: Family,
    /// Group in the list.
    pub category: Category,
    /// Supported, Beta, User or Imported.
    pub status: Status,
    /// What is unverified, one line.
    pub note: Option<String>,
    /// Further search terms.
    pub aliases: Vec<String>,
    /// Vendor page, shown only.
    pub homepage: Option<String>,
    /// Where problem reports go (SPEC-0301), `{title}` and `{body}` filled in.
    pub report_url: Option<String>,
    /// Version tested (supported systems).
    pub tested: Option<String>,
    /// Connection defaults and form fields.
    pub connection: ConnectionSpec,
    /// SQL overrides.
    pub sql: SqlSpec,
    /// Catalogue queries.
    pub introspection: IntrospectionSpec,
    /// The plugin module of a `plugin` connector (`SPEC-0314` behaviour 2).
    pub plugin: Option<String>,
    /// Where the definition came from.
    pub origin: Origin,
    /// The definition text as read.
    #[serde(skip)]
    pub source: String,
}

impl Connector {
    /// Whether this is the family's own system (`postgresql`, `mysql`, …).
    pub fn is_family_system(&self) -> bool {
        match self.family {
            Family::Store(n) => n == self.id,
            f => f.dialect().is_some_and(|d| family_name(d) == self.id),
        }
    }

    /// Whether this is a non-relational store connected through a store profile (SPEC-0310).
    pub fn is_store(&self) -> bool {
        matches!(self.family, Family::Store(_))
    }

    /// Whether a session can be opened today (the ODBC route is not built yet); a plugin
    /// connector connects through its plugin.
    pub fn connectable(&self) -> bool {
        self.family.dialect().is_some()
    }

    /// How far IDECraft vouches for the system, one line for the connection dialog
    /// (`SPEC-0116` v3 behaviour 10, `SPEC-0300` behaviour 4).
    pub fn test_level(&self) -> String {
        match self.status {
            Status::Supported => match &self.tested {
                Some(v) => format!("Tested with {v}"),
                None => "Supported".into(),
            },
            Status::Beta => "Beta: IDECraft has not tested this system against a server".into(),
            Status::User => "Your own definition; IDECraft has not tested it".into(),
            Status::Imported => "Imported definition; IDECraft has not tested it".into(),
        }
    }

    /// The plugin module of a connector that connects through a WebAssembly plugin.
    pub fn plugin_module(&self) -> Option<&str> {
        match self.family {
            Family::Plugin(_) => self.plugin.as_deref(),
            _ => None,
        }
    }

    /// `beta`, `user`, `imported` or `supported`, as the plugin network warning names it.
    pub fn status_name(&self) -> &'static str {
        match self.status {
            Status::Supported => "supported",
            Status::Beta => "beta",
            Status::User => "user",
            Status::Imported => "imported",
        }
    }

    /// The note shown under the form: the definition's, or why it cannot connect.
    pub fn shown_note(&self) -> Option<String> {
        match self.family {
            Family::Odbc => {
                return Some("Needs the ODBC route, which IDECraft does not have yet.".into());
            }
            Family::Planned(_) => {
                return Some(
                    self.note
                        .clone()
                        .unwrap_or_else(|| "Planned (ADR-0022); not connectable yet.".into()),
                );
            }
            Family::Plugin(_) => {
                return Some(format!(
                    "Connects through the WebAssembly plugin `{}`; IDECraft does not review its code.",
                    self.plugin.as_deref().unwrap_or("?")
                ));
            }
            Family::Store(_) if self.note.is_none() => {
                return Some(
                    "Non-relational store: add it from its URL as a store (SPEC-0310).".into(),
                );
            }
            _ => {}
        }
        self.note.clone()
    }

    /// Whether `query` (lowercase) matches name, id or an alias (behaviour 7).
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || self.name.to_lowercase().contains(&q)
            || self.id.contains(&q)
            || self.aliases.iter().any(|a| a.to_lowercase().contains(&q))
    }

    /// Whether the form shows `field`.
    pub fn has_field(&self, field: Field) -> bool {
        self.connection.fields.contains(&field)
    }
}

/// One problem of a definition, with the key it concerns (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// Dotted key path, e.g. `connection.default_port`; empty for the whole file.
    pub path: String,
    /// What is wrong.
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

struct Reader {
    problems: Vec<Problem>,
}

impl Reader {
    fn problem(&mut self, path: &str, message: impl Into<String>) {
        self.problems.push(Problem {
            path: path.to_owned(),
            message: message.into(),
        });
    }

    fn unknown_keys(&mut self, table: &toml::Table, prefix: &str, known: &[&str]) {
        for key in table.keys() {
            if !known.contains(&key.as_str()) {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                self.problem(&path, "unknown key");
            }
        }
    }

    fn string(&mut self, table: &toml::Table, prefix: &str, key: &str) -> Option<String> {
        let path = join(prefix, key);
        match table.get(key)? {
            toml::Value::String(s) => Some(s.clone()),
            _ => {
                self.problem(&path, "must be a string");
                None
            }
        }
    }

    fn strings(&mut self, table: &toml::Table, prefix: &str, key: &str) -> Vec<String> {
        let path = join(prefix, key);
        match table.get(key) {
            None => Vec::new(),
            Some(toml::Value::Array(items)) => {
                let mut out = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    match item {
                        toml::Value::String(s) => out.push(s.clone()),
                        _ => self.problem(&format!("{path}[{i}]"), "must be a string"),
                    }
                }
                out
            }
            Some(_) => {
                self.problem(&path, "must be a list of strings");
                Vec::new()
            }
        }
    }

    fn sql(&mut self, table: &toml::Table, prefix: &str, key: &str) -> Option<String> {
        let text = self.string(table, prefix, key)?;
        let path = join(prefix, key);
        if text.len() > MAX_SQL_BYTES {
            self.problem(&path, "is longer than 16 KiB");
        }
        if text.trim().is_empty() {
            self.problem(&path, "must not be empty");
            return None;
        }
        Some(text)
    }

    fn sub<'t>(&mut self, table: &'t toml::Table, key: &str) -> Option<&'t toml::Table> {
        match table.get(key)? {
            toml::Value::Table(t) => Some(t),
            _ => {
                self.problem(key, "must be a table");
                None
            }
        }
    }
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=41).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn valid_module(m: &str) -> bool {
    let b = m.as_bytes();
    (1..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
}

fn valid_scheme(s: &str) -> bool {
    !s.is_empty()
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.')
        })
}

/// Whether `sql` is one statement that only reads: `SELECT`, `WITH`, `SHOW` or `PRAGMA`.
fn reads_only(sql: &str, dialect: Dialect) -> bool {
    let statements = crate::split_statements(sql, dialect);
    let [only] = statements.as_slice() else {
        return false;
    };
    let first = only
        .text
        .split(|c: char| c.is_whitespace() || c == '(')
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(first.as_str(), "select" | "with" | "show" | "pragma")
}

/// Reads and checks a definition (behaviours 3 and 4). `origin` decides the status of files that
/// are not built in. Every problem is reported at once.
pub fn parse_definition(text: &str, origin: Origin) -> Result<Connector, Vec<Problem>> {
    let mut r = Reader {
        problems: Vec::new(),
    };
    if text.len() > MAX_DEFINITION_BYTES {
        r.problem("", "the file is larger than 64 KiB");
        return Err(r.problems);
    }
    let table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => {
            r.problem("", format!("not valid TOML: {}", e.message()));
            return Err(r.problems);
        }
    };
    // The exchange layout of ADR-0021 / SPEC-0302 behaviour 0 is read into the plain layout; its
    // problems are reported with the table they belong to.
    let envelope = table.contains_key("idecraft");
    let Some(table) = unwrap_envelope(table, &mut r) else {
        return Err(r.problems);
    };
    if !envelope {
        return parse_table(table, origin, text, r);
    }
    // Problems of the envelope keep their paths; those of the body name its table.
    let early = std::mem::take(&mut r.problems);
    let result = parse_table(table, origin, text, r);
    let result = match (result, early.is_empty()) {
        (Ok(c), true) => return Ok(c),
        (Ok(_), false) => Err(Vec::new()),
        (Err(p), _) => Err(p),
    };
    result.map_err(|problems| {
        let mapped = problems.into_iter().map(|mut p| {
            let head = p.path.split(['.', '[']).next().unwrap_or("");
            if matches!(head, "id" | "name" | "homepage") {
                p.path = format!("idecraft.{}", p.path);
            } else if !p.path.is_empty()
                && !matches!(
                    head,
                    "connection" | "sql" | "introspection" | "plugin" | "idecraft"
                )
            {
                p.path = format!("connector.{}", p.path);
            }
            p
        });
        early.into_iter().chain(mapped).collect()
    })
}

/// Body keys of the exchange layout's `[connector]` table.
pub(crate) const CONNECTOR_KEYS: &[&str] = &[
    "family",
    "category",
    "status",
    "note",
    "aliases",
    "report_url",
    "tested",
];

/// The envelope keys SPEC-0303 allows.
const ENVELOPE_KEYS: &[&str] = &[
    "kind",
    "format",
    "id",
    "name",
    "author",
    "licence",
    "homepage",
    "min_version",
];

/// `table` in the plain layout: unchanged without `[idecraft]`; with it, the envelope checked and
/// `[connector]` lifted to the top. `None` after a problem that makes the rest meaningless (wrong
/// kind or a newer format).
fn unwrap_envelope(mut table: toml::Table, r: &mut Reader) -> Option<toml::Table> {
    let Some(envelope) = table.remove("idecraft") else {
        return Some(table);
    };
    let toml::Value::Table(envelope) = envelope else {
        r.problem("idecraft", "must be a table");
        return None;
    };
    for key in envelope.keys() {
        if !ENVELOPE_KEYS.contains(&key.as_str()) {
            r.problem(&format!("idecraft.{key}"), "unknown key");
        }
    }
    match envelope.get("kind") {
        Some(toml::Value::String(k)) if k == "connector" => {}
        Some(toml::Value::String(k)) => {
            r.problem("idecraft.kind", format!("is a `{k}` file, not `connector`"));
            return None;
        }
        _ => {
            r.problem("idecraft.kind", "is missing; write kind = \"connector\"");
            return None;
        }
    }
    match envelope.get("format") {
        Some(toml::Value::Integer(FORMAT)) => {}
        Some(toml::Value::Integer(n)) if *n > FORMAT => {
            r.problem(
                "idecraft.format",
                format!(
                    "written by a newer IDECraft (format {n}); this version reads connector files up to format {FORMAT}"
                ),
            );
            return None;
        }
        _ => {
            r.problem("idecraft.format", "must be an integer ≥ 1");
            return None;
        }
    }
    if let Some(toml::Value::String(min)) = envelope.get("min_version")
        && newer_than_this(min)
    {
        r.problem(
            "idecraft.min_version",
            format!(
                "needs IDECraft {min} or newer; this is {}",
                env!("CARGO_PKG_VERSION")
            ),
        );
    }
    for key in ["author", "licence"] {
        if envelope.get(key).is_some_and(|v| !v.is_str()) {
            r.problem(&format!("idecraft.{key}"), "must be a string");
        }
    }
    let mut out = toml::Table::new();
    out.insert("format".into(), toml::Value::Integer(FORMAT));
    for (key, value) in table {
        match key.as_str() {
            "connector" => match value {
                toml::Value::Table(body) => {
                    for (k, v) in body {
                        if !CONNECTOR_KEYS.contains(&k.as_str()) {
                            r.problem(&format!("connector.{k}"), "unknown key");
                        } else {
                            out.insert(k, v);
                        }
                    }
                }
                _ => r.problem("connector", "must be a table"),
            },
            "connection" | "sql" | "introspection" | "plugin" => {
                out.insert(key, value);
            }
            "format" => r.problem("format", "give the format once, in [idecraft]"),
            other => r.problem(other, "unknown key"),
        }
    }
    for key in ["id", "name", "homepage"] {
        if let Some(v) = envelope.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    Some(out)
}

/// Whether version `min` (`a.b.c`) is newer than this IDECraft.
fn newer_than_this(min: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim()
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    parse(min) > parse(env!("CARGO_PKG_VERSION"))
}

fn parse_table(
    table: toml::Table,
    origin: Origin,
    text: &str,
    mut r: Reader,
) -> Result<Connector, Vec<Problem>> {
    r.unknown_keys(
        &table,
        "",
        &[
            "format",
            "id",
            "name",
            "family",
            "category",
            "status",
            "note",
            "aliases",
            "homepage",
            "report_url",
            "tested",
            "connection",
            "sql",
            "introspection",
            "plugin",
        ],
    );
    match table.get("format") {
        Some(toml::Value::Integer(FORMAT)) => {}
        Some(toml::Value::Integer(n)) => r.problem(
            "format",
            format!("format {n} is not known; this IDECraft reads format 1"),
        ),
        Some(_) => r.problem("format", "must be the number 1"),
        None => r.problem("format", "is missing; write `format = 1`"),
    }
    let id = r.string(&table, "", "id").unwrap_or_default();
    if !valid_id(&id) {
        r.problem(
            "id",
            "must be 2–41 characters: lowercase letters, digits and `-`, starting with a letter or digit",
        );
    }
    let name = r.string(&table, "", "name").unwrap_or_default();
    if name.trim().is_empty() {
        r.problem("name", "must not be empty");
    } else if name.chars().count() > 60 {
        r.problem("name", "is longer than 60 characters");
    }
    let family = match r.string(&table, "", "family") {
        Some(f) => Family::parse(&f).or_else(|| {
            r.problem(
                "family",
                format!(
                    "`{f}` is not a family; use postgresql, mysql, mariadb, sqlserver, oracle, sqlite, duckdb, odbc, mongodb, redis or plugin"
                ),
            );
            None
        }),
        None => {
            r.problem("family", "is missing");
            None
        }
    };
    let category = match r.string(&table, "", "category") {
        Some(c) => Category::parse(&c).unwrap_or_else(|| {
            r.problem(
                "category",
                format!("`{c}` is not a category; use relational, distributed, cloud, analytics, time-series, embedded, other, document, key-value or wide-column"),
            );
            Category::Other
        }),
        None => Category::Other,
    };
    let declared = r.string(&table, "", "status");
    let status = match &origin {
        Origin::BuiltIn => match declared.as_deref() {
            Some("supported") => Status::Supported,
            Some("beta") | None => Status::Beta,
            Some(other) => {
                r.problem(
                    "status",
                    format!("`{other}` is not a status; use supported or beta"),
                );
                Status::Beta
            }
        },
        // A file never claims to be tested here (behaviour 4).
        Origin::User(_) => Status::User,
        Origin::Imported(_) => Status::Imported,
    };
    let note = r.string(&table, "", "note");
    if note
        .as_ref()
        .is_some_and(|n| n.chars().count() > 160 || n.contains('\n'))
    {
        r.problem("note", "must be one line of at most 160 characters");
    }
    let aliases = r.strings(&table, "", "aliases");
    let https = |r: &mut Reader, key: &str| {
        let value = r.string(&table, "", key);
        if value.as_ref().is_some_and(|v| !v.starts_with("https://")) {
            r.problem(key, "must start with https://");
        }
        value
    };
    let homepage = https(&mut r, "homepage");
    let report_url = https(&mut r, "report_url");
    let tested = r.string(&table, "", "tested");
    if status == Status::Supported && tested.is_none() {
        r.problem(
            "tested",
            "a supported system names the version it was tested with",
        );
    }

    // A plugin connector names its module and the SQL family of its lexis (SPEC-0314).
    let mut family = family;
    let mut plugin = None;
    match (r.sub(&table, "plugin"), family) {
        (Some(t), Some(Family::Plugin(_))) => {
            r.unknown_keys(t, "plugin", &["module", "lexis"]);
            match r.string(t, "plugin", "module") {
                Some(m) if valid_module(&m) => plugin = Some(m),
                Some(_) => r.problem(
                    "plugin.module",
                    "must be a plugin file name without .wasm: lowercase letters, digits, - and _, at most 64",
                ),
                None => r.problem("plugin.module", "is missing"),
            }
            let lexis = r.string(t, "plugin", "lexis");
            match lexis.as_deref().map(Family::parse) {
                None => {}
                Some(Some(Family::Native(d))) if !d.file_based() => {
                    family = Some(Family::Plugin(d));
                }
                Some(_) => r.problem(
                    "plugin.lexis",
                    "must be a server SQL family: postgresql, mysql, mariadb, sqlserver or oracle",
                ),
            }
        }
        (None, Some(Family::Plugin(_))) => r.problem(
            "plugin",
            "is missing; a plugin connector names its module in [plugin]",
        ),
        (Some(_), _) => r.problem("plugin", "only for family = \"plugin\""),
        (None, _) => {}
    }
    let dialect = family.and_then(Family::dialect);
    let server = dialect.is_none_or(|d| !d.file_based());
    let mut connection = ConnectionSpec::default();
    if let Some(t) = r.sub(&table, "connection") {
        r.unknown_keys(
            t,
            "connection",
            &[
                "schemes",
                "default_port",
                "default_database",
                "default_user",
                "ssl",
                "fields",
            ],
        );
        connection.schemes = r.strings(t, "connection", "schemes");
        for (i, s) in connection.schemes.iter().enumerate() {
            let path = format!("connection.schemes[{i}]");
            if !valid_scheme(s) {
                r.problem(
                    &path,
                    format!("`{s}` is not a URL scheme (lowercase letters, digits, + - .)"),
                );
            } else if FAMILY_SCHEMES.contains(&s.as_str()) && family.is_none_or(|f| f.name() != id)
            {
                r.problem(&path, format!("`{s}` belongs to a family"));
            }
        }
        match t.get("default_port") {
            None => {}
            Some(toml::Value::Integer(p)) if (1..=65535).contains(p) => {
                connection.default_port = u16::try_from(*p).ok();
            }
            Some(_) => r.problem("connection.default_port", "must be between 1 and 65535"),
        }
        connection.default_database = r.string(t, "connection", "default_database");
        connection.default_user = r.string(t, "connection", "default_user");
        connection.ssl = match r.string(t, "connection", "ssl").as_deref() {
            None => None,
            Some("disable") => Some(SslMode::Disable),
            Some("prefer") => Some(SslMode::Prefer),
            Some("require") => Some(SslMode::Require),
            Some("verify-ca") => Some(SslMode::VerifyCa),
            Some("verify-full") => Some(SslMode::VerifyFull),
            Some(other) => {
                r.problem(
                    "connection.ssl",
                    format!(
                        "`{other}` is not an SSL mode; use disable, prefer, require, verify-ca or verify-full"
                    ),
                );
                None
            }
        };
        for (i, f) in r.strings(t, "connection", "fields").iter().enumerate() {
            match Field::parse(f) {
                Some(field) if !connection.fields.contains(&field) => connection.fields.push(field),
                Some(_) => {}
                None => r.problem(
                    &format!("connection.fields[{i}]"),
                    format!("`{f}` is not a field; use host, port, database, user, password, ssl or file"),
                ),
            }
        }
    }
    if connection.fields.is_empty() {
        connection.fields = if server {
            vec![
                Field::Host,
                Field::Port,
                Field::Database,
                Field::User,
                Field::Password,
                Field::Ssl,
            ]
        } else {
            vec![Field::File]
        };
    } else if server && !connection.fields.contains(&Field::Host) {
        r.problem(
            "connection.fields",
            "a server system needs the `host` field",
        );
    }

    let check_dialect = dialect.unwrap_or(Dialect::PostgreSql);
    let mut sql = SqlSpec::default();
    if let Some(t) = r.sub(&table, "sql") {
        r.unknown_keys(
            t,
            "sql",
            &[
                "identifier_quote",
                "keywords",
                "explain",
                "explain_analyze",
                "plan_format",
                "test_query",
                "version_query",
                "current_schema",
            ],
        );
        sql.identifier_quote = match r.string(t, "sql", "identifier_quote").as_deref() {
            None => None,
            Some("\"") => Some('"'),
            Some("`") => Some('`'),
            Some("[") => Some('['),
            Some(other) => {
                r.problem(
                    "sql.identifier_quote",
                    format!("`{other}` is not one of \" ` ["),
                );
                None
            }
        };
        sql.keywords = r.strings(t, "sql", "keywords");
        for (i, k) in sql.keywords.iter().enumerate() {
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                r.problem(
                    &format!("sql.keywords[{i}]"),
                    "must be one word of letters, digits and _",
                );
            }
        }
        for key in ["explain", "explain_analyze"] {
            let text = r.sql(t, "sql", key);
            if text.as_ref().is_some_and(|s| !s.contains("{statement}")) {
                r.problem(&join("sql", key), "must contain {statement}");
            }
            if key == "explain" {
                sql.explain = text;
            } else {
                sql.explain_analyze = text;
            }
        }
        sql.plan_format = match r.string(t, "sql", "plan_format").as_deref() {
            None | Some("family") => PlanFormat::Family,
            Some("text") => PlanFormat::Text,
            Some(other) => {
                r.problem(
                    "sql.plan_format",
                    format!("`{other}` is not family or text"),
                );
                PlanFormat::Family
            }
        };
        for key in ["test_query", "version_query", "current_schema"] {
            let text = r.sql(t, "sql", key);
            if text.as_ref().is_some_and(|s| !reads_only(s, check_dialect)) {
                r.problem(
                    &join("sql", key),
                    "must be one statement that only reads (SELECT, WITH, SHOW or PRAGMA)",
                );
            }
            match key {
                "test_query" => sql.test_query = text,
                "version_query" => sql.version_query = text,
                _ => sql.current_schema = text,
            }
        }
    }
    let mut introspection = IntrospectionSpec::default();
    if let Some(t) = r.sub(&table, "introspection") {
        let keys = ["schemas", "columns", "keys", "indexes", "views"];
        r.unknown_keys(t, "introspection", &keys);
        for key in keys {
            let text = r.sql(t, "introspection", key);
            let shown = text.as_ref().map(|s| s.replace("{schema}", "'x'"));
            if shown
                .as_ref()
                .is_some_and(|s| !reads_only(s, check_dialect))
            {
                r.problem(
                    &join("introspection", key),
                    "must be one statement that only reads (SELECT, WITH, SHOW or PRAGMA)",
                );
            }
            match key {
                "schemas" => introspection.schemas = text,
                "columns" => introspection.columns = text,
                "keys" => introspection.keys = text,
                "indexes" => introspection.indexes = text,
                _ => introspection.views = text,
            }
        }
    }

    if !r.problems.is_empty() {
        return Err(r.problems);
    }
    let Some(family) = family else {
        return Err(r.problems);
    };
    Ok(Connector {
        id,
        name,
        family,
        category,
        status,
        note,
        aliases,
        homepage,
        report_url,
        tested,
        connection,
        sql,
        introspection,
        plugin,
        origin,
        source: text.to_owned(),
    })
}

/// A definition file that could not be used, with its problems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvalidFile {
    /// The file.
    pub path: PathBuf,
    /// Why it was not used.
    pub problems: Vec<Problem>,
}

/// All known systems: built-ins, then the connectors folder (behaviour 2).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Catalogue {
    /// Usable connectors in catalogue order.
    pub connectors: Vec<Connector>,
    /// Files that were not used.
    pub problems: Vec<InvalidFile>,
}

/// File in the connectors folder that records which definitions were imported and from where
/// (SPEC-0302; machine-written state, so JSON by ADR-0021).
pub const IMPORTED_FILE: &str = "imported.json";

/// Extension of shared connector files (ADR-0021).
pub const EXTENSION: &str = "idcconnector";

/// The user's connectors folder: `$IDECRAFT_CONNECTORS_DIR`, else `connectors/` in the IDECraft
/// settings folder (`$IDECRAFT_CONFIG_DIR`, `$XDG_CONFIG_HOME/idecraft`, `~/.config/idecraft`).
pub fn connectors_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("IDECRAFT_CONNECTORS_DIR") {
        return Some(PathBuf::from(d));
    }
    let config = if let Some(d) = std::env::var_os("IDECRAFT_CONFIG_DIR") {
        PathBuf::from(d)
    } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(d).join("idecraft")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config/idecraft")
    };
    Some(config.join("connectors"))
}

/// Ids of the definitions in `dir` that were imported, from [`IMPORTED_FILE`].
pub(crate) fn imported_ids(dir: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(dir.join(IMPORTED_FILE)) else {
        return BTreeSet::new();
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&text) else {
        return BTreeSet::new();
    };
    map.keys().cloned().collect()
}

impl Catalogue {
    /// The compiled-in systems only.
    pub fn built_in() -> Self {
        let connectors = BUILT_IN
            .iter()
            .map(|(id, text)| match parse_definition(text, Origin::BuiltIn) {
                Ok(c) => c,
                Err(p) => panic!("built-in connector {id} is invalid: {p:?}"),
            })
            .collect();
        Self {
            connectors,
            problems: Vec::new(),
        }
    }

    /// Built-ins plus the `*.toml` files of `dir` (a missing folder adds nothing).
    pub fn load(dir: Option<&Path>) -> Self {
        let mut catalogue = Self::built_in();
        let Some(dir) = dir else {
            return catalogue;
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return catalogue;
        };
        let imported = imported_ids(dir);
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "toml" || e == EXTENSION))
            .collect();
        files.sort();
        for path in files {
            let result = read_file(&path).and_then(|text| {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let stem = stem.strip_suffix(".idcconnector").unwrap_or(stem);
                let origin = if imported.contains(stem) {
                    Origin::Imported(path.clone())
                } else {
                    Origin::User(path.clone())
                };
                parse_definition(&text, origin)
            });
            match result.and_then(|c| catalogue.check_new(&c).map(|()| c)) {
                Ok(c) => catalogue.connectors.push(c),
                Err(problems) => catalogue.problems.push(InvalidFile { path, problems }),
            }
        }
        catalogue
    }

    /// Problems that keep `connector` out of this catalogue: a taken id or scheme.
    pub fn check_new(&self, connector: &Connector) -> Result<(), Vec<Problem>> {
        let mut problems = Vec::new();
        if let Some(other) = self.get(&connector.id) {
            problems.push(Problem {
                path: "id".into(),
                message: if other.origin == Origin::BuiltIn {
                    format!(
                        "`{}` is a built-in system and cannot be replaced",
                        connector.id
                    )
                } else {
                    format!("`{}` is already defined by another file", connector.id)
                },
            });
        }
        for (i, scheme) in connector.connection.schemes.iter().enumerate() {
            if let Some(other) = self.by_scheme(scheme) {
                problems.push(Problem {
                    path: format!("connection.schemes[{i}]"),
                    message: format!("`{scheme}` is already used by {}", other.name),
                });
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// The connector with `id`.
    pub fn get(&self, id: &str) -> Option<&Connector> {
        self.connectors.iter().find(|c| c.id == id)
    }

    /// The connector whose URL schemes include `scheme`.
    pub fn by_scheme(&self, scheme: &str) -> Option<&Connector> {
        let scheme = scheme.to_ascii_lowercase();
        self.connectors
            .iter()
            .find(|c| c.connection.schemes.contains(&scheme))
    }

    /// The connector of a profile: its `system`, else the family's own.
    pub fn for_profile(&self, dialect: Dialect, system: Option<&str>) -> Option<&Connector> {
        self.get(system.unwrap_or(family_name(dialect)))
    }

    /// Connectors matching `query`, grouped by category in display order; family systems first
    /// in their group, then by name (behaviour 7).
    pub fn grouped(&self, query: &str) -> Vec<(Category, Vec<&Connector>)> {
        Category::ALL
            .iter()
            .filter_map(|&cat| {
                let mut items: Vec<&Connector> = self
                    .connectors
                    .iter()
                    .filter(|c| c.category == cat && c.matches(query))
                    .collect();
                items.sort_by_key(|c| (!c.is_family_system(), c.name.to_lowercase()));
                (!items.is_empty()).then_some((cat, items))
            })
            .collect()
    }
}

fn read_file(path: &Path) -> Result<String, Vec<Problem>> {
    let fail = |message: String| {
        vec![Problem {
            path: String::new(),
            message,
        }]
    };
    let size = std::fs::metadata(path)
        .map_err(|e| fail(e.to_string()))?
        .len();
    if size > MAX_DEFINITION_BYTES as u64 {
        return Err(fail("the file is larger than 64 KiB".into()));
    }
    std::fs::read_to_string(path).map_err(|e| fail(e.to_string()))
}

fn shared() -> &'static RwLock<Arc<Catalogue>> {
    static SHARED: OnceLock<RwLock<Arc<Catalogue>>> = OnceLock::new();
    SHARED.get_or_init(|| RwLock::new(Arc::new(Catalogue::load(connectors_dir().as_deref()))))
}

/// The catalogue of this process, read once from the connectors folder.
pub fn catalogue() -> Arc<Catalogue> {
    shared()
        .read()
        .map(|c| Arc::clone(&c))
        .unwrap_or_else(|e| Arc::clone(&e.into_inner()))
}

/// Reads the connectors folder again (after an import or removal) and returns the new catalogue.
pub fn reload_catalogue() -> Arc<Catalogue> {
    let fresh = Arc::new(Catalogue::load(connectors_dir().as_deref()));
    match shared().write() {
        Ok(mut c) => *c = Arc::clone(&fresh),
        Err(e) => *e.into_inner() = Arc::clone(&fresh),
    }
    fresh
}

/// Replaces `{schema}` in a catalogue query by `schema` as a quoted SQL literal (behaviour 3).
pub fn with_schema(query: &str, schema: &str) -> String {
    query.replace("{schema}", &format!("'{}'", schema.replace('\'', "''")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Result<Connector, Vec<Problem>> {
        parse_definition(text, Origin::User(PathBuf::from("x.toml")))
    }

    fn paths(problems: &[Problem]) -> Vec<&str> {
        problems.iter().map(|p| p.path.as_str()).collect()
    }

    /// SPEC-0116 v3 AK-6: every built-in system says how far it is tested; SQL Server and Oracle
    /// say that their live tests do not run on every build.
    #[test]
    fn every_system_says_how_far_it_is_tested() {
        let c = Catalogue::built_in();
        for k in &c.connectors {
            let line = k.test_level();
            match k.status {
                Status::Supported => assert!(line.starts_with("Tested with "), "{}: {line}", k.id),
                _ => assert!(line.contains("not tested"), "{}: {line}", k.id),
            }
        }
        for id in ["sqlserver", "oracle"] {
            let k = c.connectors.iter().find(|k| k.id == id).unwrap();
            assert!(
                k.shown_note()
                    .unwrap()
                    .contains("do not run on every build"),
                "{id}"
            );
        }
        let mut user = c.connectors[0].clone();
        user.status = Status::User;
        assert!(user.test_level().starts_with("Your own definition"));
    }

    #[test]
    fn built_ins_are_valid_unique_and_honest() {
        let c = Catalogue::built_in();
        let mut ids = BTreeSet::new();
        let mut schemes = BTreeSet::new();
        for k in &c.connectors {
            assert!(ids.insert(k.id.clone()), "duplicate id {}", k.id);
            for s in &k.connection.schemes {
                assert!(schemes.insert(s.clone()), "duplicate scheme {s}");
            }
            assert!(matches!(k.status, Status::Supported | Status::Beta));
            if k.status == Status::Supported {
                assert!(k.tested.is_some(), "{} names no tested version", k.id);
            } else {
                assert!(k.note.is_some(), "beta {} has no note", k.id);
            }
            assert!(
                !k.is_family_system() || k.status == Status::Supported,
                "{}",
                k.id
            );
            // Stores and planned systems use their family's URLs (SPEC-0310).
            if !k.is_family_system() && k.connectable() {
                assert!(!k.connection.schemes.is_empty(), "{} has no scheme", k.id);
                assert!(k.connection.default_port.is_some() || !k.has_field(Field::Host));
            }
        }
        for d in Dialect::ALL {
            assert!(
                c.get(family_name(d))
                    .is_some_and(Connector::is_family_system)
            );
        }
        assert_eq!(c.by_scheme("tidb").map(|k| k.id.as_str()), Some("tidb"));
        assert_eq!(
            c.get("tidb").unwrap().family,
            Family::Native(Dialect::MySql)
        );
    }

    #[test]
    fn plugin_connectors_name_their_module_and_never_claim_support() {
        // SPEC-0314 AK-3 and behaviour 2.
        let good = "format = 1\nid = \"acmedb\"\nname = \"AcmeDB\"\nfamily = \"plugin\"\n\
                    status = \"supported\"\n[plugin]\nmodule = \"acme_db\"\nlexis = \"mysql\"\n\
                    [connection]\nschemes = [\"acmedb\"]\ndefault_port = 7000\n";
        let c = parse_definition(good, Origin::Imported("a.toml".into())).unwrap();
        assert_eq!(c.family, Family::Plugin(Dialect::MySql));
        assert_eq!(c.family.name(), "plugin");
        assert_eq!(c.plugin_module(), Some("acme_db"));
        assert_eq!(c.status, Status::Imported);
        assert_eq!(c.status_name(), "imported");
        assert!(c.connectable());
        assert!(c.shown_note().unwrap().contains("does not review its code"));
        let paths = |text: &str| -> Vec<String> {
            parse_definition(text, Origin::User("b.toml".into()))
                .unwrap_err()
                .into_iter()
                .map(|p| p.path)
                .collect()
        };
        let base = "format = 1\nid = \"acmedb\"\nname = \"AcmeDB\"\n";
        assert_eq!(paths(&format!("{base}family = \"plugin\"\n")), ["plugin"]);
        assert_eq!(
            paths(&format!(
                "{base}family = \"plugin\"\n[plugin]\nmodule = \"../x\"\nlexis = \"sqlite\"\nrun = 1\n"
            )),
            ["plugin.run", "plugin.module", "plugin.lexis"]
        );
        assert_eq!(
            paths(&format!(
                "{base}family = \"postgresql\"\n[plugin]\nmodule = \"x\"\n"
            )),
            ["plugin"]
        );
    }

    #[test]
    fn live_tested_family_systems_are_supported() {
        // SPEC-0304 AK-2; CockroachDB after PO-STATUS question 24 (licence accepted for tests).
        let c = Catalogue::built_in();
        for id in ["timescaledb", "tidb", "yugabytedb", "cockroachdb"] {
            let k = c.get(id).unwrap();
            assert_eq!(k.status, Status::Supported, "{id}");
            assert!(k.tested.is_some(), "{id}");
        }
        assert!(
            c.get("cockroachdb")
                .unwrap()
                .tested
                .as_deref()
                .unwrap()
                .contains("v26.3")
        );
    }

    #[test]
    fn non_relational_stores_and_planned_systems() {
        // SPEC-0310 AK-8.
        let c = Catalogue::built_in();
        let status = |id: &str| c.get(id).map(|k| (k.status, k.category, k.family.name()));
        assert_eq!(
            status("mongodb"),
            Some((Status::Supported, Category::Document, "mongodb"))
        );
        assert_eq!(
            status("redis"),
            Some((Status::Supported, Category::KeyValue, "redis"))
        );
        assert_eq!(
            status("valkey"),
            Some((Status::Beta, Category::KeyValue, "redis"))
        );
        assert_eq!(
            status("documentdb"),
            Some((Status::Beta, Category::Document, "mongodb"))
        );
        assert_eq!(
            status("cassandra"),
            Some((Status::Beta, Category::WideColumn, "cassandra"))
        );
        for id in ["dynamodb", "couchbase", "cassandra"] {
            let k = c.get(id).unwrap();
            assert!(!k.connectable() && !k.is_store(), "{id}");
            assert!(k.shown_note().unwrap().contains("Planned"), "{id}");
        }
        for id in ["mongodb", "redis", "valkey", "documentdb"] {
            let k = c.get(id).unwrap();
            assert!(k.is_store() && !k.connectable(), "{id}");
        }
        assert!(c.get("mongodb").unwrap().is_family_system());
        assert_eq!(
            c.by_scheme("mongodb+srv").map(|k| k.id.as_str()),
            Some("mongodb")
        );
        assert_eq!(c.by_scheme("rediss").map(|k| k.id.as_str()), Some("redis"));
        let groups: Vec<Category> = c.grouped("").into_iter().map(|(g, _)| g).collect();
        assert_eq!(
            &groups[groups.len() - 3..],
            [Category::Document, Category::KeyValue, Category::WideColumn]
        );
        assert_eq!(
            c.grouped("mongo")
                .into_iter()
                .flat_map(|(_, ks)| ks)
                .map(|k| k.id.as_str())
                .collect::<Vec<_>>(),
            ["mongodb", "documentdb"]
        );
    }

    #[test]
    fn odbc_systems_are_listed_but_not_connectable() {
        let c = Catalogue::built_in();
        for id in [
            "db2",
            "sap-ase",
            "exasol",
            "vertica",
            "hive",
            "snowflake",
            "bigquery",
        ] {
            let k = c.get(id).unwrap_or_else(|| panic!("{id} missing"));
            assert_eq!(k.family, Family::Odbc, "{id}");
            assert!(
                !k.connectable() && k.shown_note().unwrap().contains("ODBC"),
                "{id}"
            );
        }
        assert!(
            crate::ConnectionProfile::from_url("s", "snowflake://u@acct/db")
                .unwrap_err()
                .to_string()
                .contains("ODBC route")
        );
        assert_eq!(
            c.get("clickhouse").unwrap().family,
            Family::Native(Dialect::MySql)
        );
    }

    #[test]
    fn reports_every_problem_with_its_key() {
        let text = r#"
            id = "Bad Id"
            name = ""
            family = "db2"
            category = "fun"
            colour = "red"
            homepage = "http://example.com"
            [connection]
            schemes = ["postgres", "OK"]
            default_port = 70000
            fields = ["port", "server"]
            [sql]
            explain = "EXPLAIN"
            test_query = "DELETE FROM t"
            [introspection]
            columns = "SELECT 1; DROP TABLE t"
            tables = "SELECT 1"
        "#;
        let problems = user(text).unwrap_err();
        let p = paths(&problems);
        for expected in [
            "format",
            "id",
            "name",
            "family",
            "category",
            "colour",
            "homepage",
            "connection.schemes[0]",
            "connection.schemes[1]",
            "connection.default_port",
            "connection.fields[1]",
            "sql.explain",
            "sql.test_query",
            "introspection.columns",
            "introspection.tables",
        ] {
            assert!(p.contains(&expected), "{expected} missing in {p:?}");
        }
        let too_big = format!("format = 1\nnote = \"{}\"", "x".repeat(70 * 1024));
        assert_eq!(
            user(&too_big).unwrap_err()[0].message,
            "the file is larger than 64 KiB"
        );
        assert!(
            user("format = [").unwrap_err()[0]
                .message
                .starts_with("not valid TOML")
        );
    }

    #[test]
    fn user_files_never_claim_supported() {
        let text = "format = 1\nid = \"my-pg\"\nname = \"My PG\"\nfamily = \"postgresql\"\n\
                    status = \"supported\"\n[connection]\nschemes = [\"mypg\"]\ndefault_port = 6543\n";
        let c = user(text).unwrap();
        assert_eq!(c.status, Status::User);
        let c = parse_definition(text, Origin::Imported(PathBuf::from("a"))).unwrap();
        assert_eq!(c.status, Status::Imported);
        assert_eq!(c.connection.fields.len(), 6);
        let odbc = user("format = 1\nid = \"db2\"\nname = \"Db2\"\nfamily = \"odbc\"\n").unwrap();
        assert!(!odbc.connectable());
        assert!(odbc.shown_note().unwrap().contains("ODBC"));
    }

    #[test]
    fn folder_adds_valid_files_and_lists_the_rest() {
        let dir = std::env::temp_dir().join(format!("idecraft-connectors-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let def = |id: &str, scheme: &str| {
            format!(
                "format = 1\nid = \"{id}\"\nname = \"{id} db\"\nfamily = \"mysql\"\naliases = [\"zz{id}\"]\n\
                 [connection]\nschemes = [\"{scheme}\"]\ndefault_port = 1234\n"
            )
        };
        std::fs::write(dir.join("a.toml"), def("acme", "acme")).unwrap();
        std::fs::write(dir.join("b.toml"), def("acme", "acme2")).unwrap();
        std::fs::write(dir.join("c.toml"), def("tidb", "tidb9")).unwrap();
        std::fs::write(dir.join("d.toml"), def("other", "tidb")).unwrap();
        std::fs::write(dir.join("e.toml"), "garbage = ").unwrap();
        std::fs::write(dir.join("f.toml"), def("imp", "imp")).unwrap();
        std::fs::write(dir.join(IMPORTED_FILE), r#"{"imp": {"source": "x"}}"#).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let c = Catalogue::load(Some(&dir));
        assert_eq!(c.get("acme").unwrap().status, Status::User);
        // The file is named f.toml, so its id is not among the imported ones.
        assert_eq!(c.get("imp").unwrap().status, Status::User);
        let bad: Vec<_> = c
            .problems
            .iter()
            .map(|p| p.path.file_name().unwrap().to_owned())
            .collect();
        assert_eq!(bad, ["b.toml", "c.toml", "d.toml", "e.toml"]);
        assert!(c.problems[1].problems[0].message.contains("built-in"));
        std::fs::rename(dir.join("f.toml"), dir.join("imp.idcconnector")).unwrap();
        let c = Catalogue::load(Some(&dir));
        assert_eq!(c.get("imp").unwrap().status, Status::Imported);
        let groups = c.grouped("zzacm");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1[0].id, "acme");
        let all = c.grouped("");
        let order: Vec<Category> = all.iter().map(|(cat, _)| *cat).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
        assert_eq!(all[0].1[0].id, "mariadb", "family systems first, by name");
        assert!(all[0].1[..5].iter().all(|k| k.is_family_system()));
        assert!(
            c.grouped("TIDB")
                .iter()
                .any(|(_, v)| v.iter().any(|k| k.id == "tidb"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exchange_layout_reads_like_the_plain_one() {
        let text = "[idecraft]\nkind = \"connector\"\nformat = 1\nid = \"acme\"\nname = \"Acme DB\"\n\
                    author = \"Ada\"\nlicence = \"MIT\"\n\n[connector]\nfamily = \"mysql\"\n\
                    category = \"cloud\"\n\n[connection]\nschemes = [\"acme\"]\ndefault_port = 3307\n";
        let c = user(text).unwrap();
        assert_eq!((c.id.as_str(), c.name.as_str()), ("acme", "Acme DB"));
        assert_eq!(c.family, Family::Native(Dialect::MySql));
        assert_eq!(c.connection.default_port, Some(3307));
        let problems =
            user(&text.replace("family = \"mysql\"", "family = \"db9\"\ncolour = 1")).unwrap_err();
        assert_eq!(paths(&problems), ["connector.colour", "connector.family"]);
        let wrong = user(&text.replace("kind = \"connector\"", "kind = \"keymap\"")).unwrap_err();
        assert_eq!(
            wrong[0].to_string(),
            "idecraft.kind: is a `keymap` file, not `connector`"
        );
        let newer = user(&text.replace("format = 1", "format = 2")).unwrap_err();
        assert_eq!(
            newer[0].message,
            "written by a newer IDECraft (format 2); this version reads connector files up to format 1"
        );
        let both = user(&format!("format = 1\n{text}")).unwrap_err();
        assert_eq!(both[0].path, "format");
        let future = user(&text.replace("author", "min_version = \"99.0.0\"\nauthor")).unwrap_err();
        assert_eq!(future[0].path, "idecraft.min_version");
        let odd = user(&text.replace("author", "colour = 1\nauthor")).unwrap_err();
        assert_eq!(odd[0].path, "idecraft.colour");
    }

    #[test]
    fn schema_placeholder_is_a_literal() {
        assert_eq!(
            with_schema("select * from t where s = {schema}", "o'b"),
            "select * from t where s = 'o''b'"
        );
    }

    #[test]
    fn connector_keywords_complete() {
        let items = crate::complete_for(
            "ups",
            3,
            Dialect::PostgreSql,
            Some("cockroachdb"),
            &[],
            &[],
            None,
        );
        assert!(
            items
                .iter()
                .any(|c| c.label == "upsert" && c.detail == "CockroachDB")
        );
        let items = crate::complete_for("ups", 3, Dialect::PostgreSql, None, &[], &[], None);
        assert!(!items.iter().any(|c| c.label == "upsert"));
    }

    #[test]
    fn urls_select_connectors_and_come_back() {
        use crate::ConnectionProfile;
        let p = ConnectionProfile::from_url("t", "tidb://root@db.local/shop").unwrap();
        assert_eq!(
            (p.dialect, p.system.as_deref()),
            (Dialect::MySql, Some("tidb"))
        );
        assert_eq!(p.effective_port(), Some(4000));
        assert_eq!(p.to_url(), "tidb://root@db.local/shop");
        let p = ConnectionProfile::from_url("c", "cockroachdb://root@h:26000/defaultdb").unwrap();
        assert_eq!((p.dialect, p.port), (Dialect::PostgreSql, Some(26000)));
        assert_eq!(ConnectionProfile::from_url("c", &p.to_url()).unwrap(), p);
        // A system's default SSL mode is kept in the URL.
        let p = ConnectionProfile::from_url("r", "redshift://u@cluster/dev").unwrap();
        assert_eq!(p.ssl, SslMode::Require);
        assert_eq!(p.to_url(), "redshift://u@cluster/dev?sslmode=require");
        // Profiles without a system read and write as before.
        let p = ConnectionProfile::from_url("p", "postgresql://u@h/db").unwrap();
        assert_eq!(p.system, None);
        let text = toml::to_string(&p).unwrap();
        assert!(!text.contains("system"));
        assert!(ConnectionProfile::from_url("x", "nosuchdb://h/db").is_err());
        // A system of another family is refused.
        let mut wrong = ConnectionProfile::from_url("w", "postgresql://u@h/db").unwrap();
        wrong.system = Some("tidb".into());
        assert!(wrong.validate().is_err());
    }

    #[tokio::test]
    async fn unknown_system_is_refused_before_connecting() {
        let mut p =
            crate::ConnectionProfile::from_url("u", "postgresql://u@127.0.0.1:1/db").unwrap();
        p.system = Some("not-imported".into());
        let err = crate::Session::connect(&p, &crate::Credentials::NotNeeded)
            .await
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "unknown database system `not-imported`; import its definition"
        );
    }

    #[test]
    fn text_plans_are_one_step_per_row() {
        let rows = vec![
            vec![Some("distribution: local".into())],
            vec![Some("• scan".into()), Some("table: t@t_pkey".into())],
            vec![Some(String::new())],
        ];
        let nodes = crate::plan::text_plan(&rows);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1].operation, "• scan  table: t@t_pkey");
    }
}
