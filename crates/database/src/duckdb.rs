//! DuckDB sessions (`SPEC-0304`, ADR-0020 decision 2): an embedded analytical database in one file,
//! through the `duckdb` crate with the engine compiled in. Like SQLite it runs in-process and
//! blocking, so statements run on the blocking pool; a missing file is refused, never created.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use duckdb::types::{TimeUnit, Value};
use duckdb::{AccessMode, Config, Connection};

use crate::session::{SessionError, prepare};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, split_statements,
};

/// An open DuckDB database file.
pub struct DuckDbSession {
    conn: Arc<Mutex<Connection>>,
}

impl DuckDbSession {
    /// Opens the file of `profile` (read-only profiles open it read-only); the file must exist.
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        prepare(profile, credentials, Dialect::DuckDb)?;
        let path = profile.file_path();
        let open_error = |message: String| SessionError::Connect {
            profile: profile.name.clone(),
            address: path.display().to_string(),
            message,
        };
        if !path.is_file() {
            return Err(open_error(
                "file does not exist (DuckDB files are not created implicitly)".into(),
            ));
        }
        let read_only = profile.read_only;
        let opened: PathBuf = path.clone();
        let conn = tokio::task::spawn_blocking(move || {
            let mode = if read_only {
                AccessMode::ReadOnly
            } else {
                AccessMode::ReadWrite
            };
            // No extension is installed or loaded from the network behind the user's back.
            let config = Config::default()
                .access_mode(mode)?
                .enable_autoload_extension(false)?;
            Connection::open_with_flags(opened, config)
        })
        .await
        .map_err(|e| open_error(e.to_string()))?
        .map_err(|e| open_error(e.to_string()))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Closes this session's access to every file but the opened database, for good
    /// (`SPEC-0012` v3 behaviour 15): `ATTACH`, `COPY`, the file functions and extensions are
    /// then refused by the engine, and the configuration is locked so that no statement can
    /// open it again.
    pub async fn close_external_access(&self) -> Result<(), String> {
        let run = self
            .run_script(
                "SET enable_external_access = false; SET lock_configuration = true",
                OnError::Stop,
            )
            .await;
        if run.outcomes.len() != 2 || run.not_run != 0 {
            return Err("the database's file access could not be closed for the agent".into());
        }
        match run.outcomes.into_iter().find_map(|o| match o.outcome {
            Outcome::Error(e) => Some(e.to_string()),
            _ => None,
        }) {
            Some(e) => Err(format!(
                "the database's file access could not be closed for the agent: {e}"
            )),
            None => Ok(()),
        }
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let conn = Arc::clone(&self.conn);
        let script = script.to_owned();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let statements = split_statements(&script, Dialect::DuckDb);
            let mut run = ScriptRun::default();
            for (index, statement) in statements.iter().enumerate() {
                let failed = run.push(&script, statement, execute(&conn, &statement.text));
                if failed && on_error == OnError::Stop {
                    run.not_run = statements.len() - index - 1;
                    break;
                }
            }
            run
        })
        .await
        .unwrap_or_default()
    }
}

fn error(e: &duckdb::Error) -> DbError {
    DbError {
        message: format!("DuckDB: {e}"),
        sqlstate: None,
        position: None,
    }
}

fn execute(conn: &Connection, sql: &str) -> Outcome {
    let mut statement = match conn.prepare(sql) {
        Ok(statement) => statement,
        Err(e) => return Outcome::Error(error(&e)),
    };
    let mut rows = Vec::new();
    let mut cursor = match statement.query([]) {
        Ok(cursor) => cursor,
        Err(e) => return Outcome::Error(error(&e)),
    };
    loop {
        match cursor.next() {
            Ok(Some(row)) => {
                let stmt = row.as_ref();
                let width = stmt.column_count();
                rows.push(
                    (0..width)
                        .map(|i| {
                            // A column of type NULL (`select null`) cannot be read as a value.
                            if stmt.column_type(i) == duckdb::arrow::datatypes::DataType::Null {
                                return None;
                            }
                            row.get::<_, Value>(i).map(|v| text_of(&v)).unwrap_or(None)
                        })
                        .collect::<Vec<_>>(),
                );
            }
            Ok(None) => break,
            Err(e) => return Outcome::Error(error(&e)),
        }
    }
    drop(cursor);
    let columns: Vec<String> = statement.column_names();
    // DuckDB answers data changes with one row and one column `Count`.
    let lower = sql.trim_start().to_ascii_lowercase();
    let changes = ["insert", "update", "delete"]
        .iter()
        .any(|k| lower.starts_with(k))
        && !lower.contains(" returning ");
    if changes
        && columns.len() == 1
        && let [row] = rows.as_slice()
        && let Some(Some(n)) = row.first()
        && let Ok(n) = n.parse::<u64>()
    {
        return Outcome::UpdateCount(n);
    }
    // Statements that do not return rows (DDL, SET, transactions) report no count.
    let returns_rows = [
        "select",
        "with",
        "from",
        "show",
        "describe",
        "pragma",
        "explain",
        "summarize",
        "values",
        "table",
        "call",
    ]
    .iter()
    .any(|k| lower.starts_with(k));
    if columns.is_empty() || (rows.is_empty() && !returns_rows) {
        return Outcome::UpdateCount(0);
    }
    Outcome::ResultSet { columns, rows }
}

/// A DuckDB value as text, `None` for NULL (SPEC-0007: every type readable as text).
fn text_of(value: &Value) -> Option<String> {
    Some(match value {
        Value::Null => return None,
        Value::Boolean(b) => b.to_string(),
        Value::TinyInt(n) => n.to_string(),
        Value::SmallInt(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::BigInt(n) => n.to_string(),
        Value::HugeInt(n) => n.to_string(),
        Value::UTinyInt(n) => n.to_string(),
        Value::USmallInt(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::UBigInt(n) => n.to_string(),
        Value::Float(f) => format!("{f:?}"),
        Value::Double(f) => format!("{f:?}"),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => b.iter().map(|x| format!("{x:02x}")).collect::<String>(),
        Value::Date32(days) => chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
            .and_then(|d| d.checked_add_signed(chrono::TimeDelta::days(i64::from(*days))))
            .map_or_else(|| days.to_string(), |d| d.to_string()),
        Value::Timestamp(unit, n) => {
            let micros = match unit {
                TimeUnit::Second => n.saturating_mul(1_000_000),
                TimeUnit::Millisecond => n.saturating_mul(1_000),
                TimeUnit::Microsecond => *n,
                TimeUnit::Nanosecond => n / 1_000,
            };
            chrono::DateTime::from_timestamp_micros(micros)
                .map_or_else(|| n.to_string(), |t| t.naive_utc().to_string())
        }
        Value::Time64(unit, n) => {
            let micros = match unit {
                TimeUnit::Second => n.saturating_mul(1_000_000),
                TimeUnit::Millisecond => n.saturating_mul(1_000),
                TimeUnit::Microsecond => *n,
                TimeUnit::Nanosecond => n / 1_000,
            };
            chrono::NaiveTime::from_num_seconds_from_midnight_opt(
                u32::try_from(micros / 1_000_000).unwrap_or(0),
                u32::try_from((micros % 1_000_000) * 1_000).unwrap_or(0),
            )
            .map_or_else(|| n.to_string(), |t| t.to_string())
        }
        Value::Enum(s) => s.clone(),
        Value::List(items) | Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|v| text_of(v).unwrap_or_else(|| "NULL".into()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    })
}
