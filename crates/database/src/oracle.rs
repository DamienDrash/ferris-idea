//! Oracle sessions (`SPEC-0142`, ADR-0011).
//!
//! Through the `oracle` crate (ODPI-C), which loads Oracle Instant Client at run time: from
//! `IDECRAFT_ORACLE_CLIENT`, else the newest `~/.local/opt/instantclient_*`, else the system's
//! library search path. The driver is blocking; every call runs on Tokio's blocking pool. Each
//! statement is committed after it ran, except inside a transaction opened with `SET
//! TRANSACTION` (editing, SPEC-0017), which ends with `COMMIT` or `ROLLBACK`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::session::{SessionError, mode_name, prepare};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, SslMode,
    StatementClass, classify, split_statements,
};

/// Rows of a query: column names and values as text.
pub type ResultSet = (Vec<String>, Vec<Vec<Option<String>>>);

struct Inner {
    conn: oracle::Connection,
    /// A transaction opened with `SET TRANSACTION` is running.
    in_transaction: bool,
}

/// An open connection to an Oracle database.
pub struct OracleSession {
    inner: Arc<Mutex<Inner>>,
    read_only: bool,
}

/// The Instant Client folder to load: `IDECRAFT_ORACLE_CLIENT`, else the newest
/// `~/.local/opt/instantclient_*` with a client library, else none (system search path).
pub fn client_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("IDECRAFT_ORACLE_CLIENT") {
        return Some(PathBuf::from(dir));
    }
    let opt = PathBuf::from(std::env::var_os("HOME")?).join(".local/opt");
    let mut found: Vec<PathBuf> = std::fs::read_dir(opt)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("instantclient_"))
                && (p.join("libclntsh.dylib").exists() || p.join("libclntsh.so").exists())
        })
        .collect();
    found.sort();
    found.pop()
}

/// Loads Instant Client once; later calls return the first result.
fn init_client() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        let mut params = oracle::InitParams::new();
        if let Some(dir) = client_dir() {
            params
                .oracle_client_lib_dir(&dir)
                .map_err(|e| e.to_string())?;
        }
        params.init().map(|_| ()).map_err(|e| {
            format!(
                "Oracle Instant Client could not be loaded ({e}); install it under \
                 ~/.local/opt/instantclient_* or set IDECRAFT_ORACLE_CLIENT"
            )
        })
    })
    .clone()
}

/// The EZConnect string: `//host:port/service`, or `tcps://…` when the profile asks for TLS.
pub fn connect_string(profile: &ConnectionProfile) -> String {
    let host = profile.host.clone().unwrap_or_else(|| "localhost".into());
    let port = profile.effective_port().unwrap_or(1521);
    let service = profile.database.clone().unwrap_or_default();
    match profile.ssl {
        SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => {
            format!("tcps://{host}:{port}/{service}")
        }
        SslMode::Disable | SslMode::Prefer => format!("//{host}:{port}/{service}"),
    }
}

fn to_db_error(e: &oracle::Error) -> DbError {
    let position = e
        .db_error()
        .map(|d| d.offset())
        .filter(|o| *o > 0)
        .map(|o| o as usize + 1);
    DbError {
        message: e.to_string(),
        sqlstate: None,
        position,
    }
}

impl OracleSession {
    /// Opens a session for `profile` with already resolved `credentials`.
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        let password = prepare(profile, credentials, Dialect::Oracle)?.unwrap_or_default();
        let user = profile.user.clone().unwrap_or_default();
        let target = connect_string(profile);
        let name = profile.name.clone();
        let address = target.clone();
        let tls = matches!(
            profile.ssl,
            SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull
        );
        let mode = profile.ssl;
        let opened = tokio::task::spawn_blocking(move || {
            init_client()?;
            oracle::Connection::connect(user, password, &target).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        let mut conn = opened.map_err(|message| {
            if tls {
                SessionError::Tls {
                    profile: name.clone(),
                    mode: mode_name(mode),
                    message: format!("{address}: {message}"),
                }
            } else {
                SessionError::Connect {
                    profile: name.clone(),
                    address: address.clone(),
                    message,
                }
            }
        })?;
        conn.set_autocommit(false);
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                conn,
                in_transaction: false,
            })),
            read_only: profile.read_only,
        })
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let statements = split_statements(script, Dialect::Oracle);
        let inner = Arc::clone(&self.inner);
        let read_only = self.read_only;
        let texts: Vec<String> = statements.iter().map(|s| s.text.clone()).collect();
        let outcomes = tokio::task::spawn_blocking(move || {
            let mut inner = match inner.lock() {
                Ok(i) => i,
                Err(p) => p.into_inner(),
            };
            let mut out = Vec::new();
            for sql in &texts {
                let outcome = match refusal(sql, read_only) {
                    Some(error) => Outcome::Error(error),
                    None => execute(&mut inner, sql),
                };
                let failed = matches!(outcome, Outcome::Error(_));
                out.push(outcome);
                if failed && on_error == OnError::Stop {
                    break;
                }
            }
            out
        })
        .await
        .unwrap_or_default();
        let mut run = ScriptRun::default();
        let count = outcomes.len();
        for (statement, outcome) in statements.iter().zip(outcomes) {
            run.push(script, statement, outcome);
        }
        if count < statements.len() {
            run.not_run = statements.len() - count;
        }
        run
    }

    /// The rows of one query.
    pub async fn query(&self, sql: &str) -> Result<ResultSet, DbError> {
        let inner = Arc::clone(&self.inner);
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || {
            let inner = match inner.lock() {
                Ok(i) => i,
                Err(p) => p.into_inner(),
            };
            rows(&inner.conn, &sql)
        })
        .await
        .map_err(|e| DbError {
            message: e.to_string(),
            ..DbError::default()
        })?
    }
}

/// Oracle has no read-only session that also refuses DDL: a read-only profile refuses everything
/// that is not known to only read, except `SET` statements.
fn refusal(sql: &str, read_only: bool) -> Option<DbError> {
    let first = crate::script::code_words(sql, Dialect::Oracle)
        .into_iter()
        .next();
    (read_only
        && classify(sql, Dialect::Oracle) != StatementClass::ReadData
        && first.as_deref() != Some("set"))
    .then(|| DbError {
        message: "the connection is read-only; only queries and SET statements run".into(),
        sqlstate: None,
        position: None,
    })
}

fn rows(conn: &oracle::Connection, sql: &str) -> Result<ResultSet, DbError> {
    let result = conn.query(sql, &[]).map_err(|e| to_db_error(&e))?;
    let columns: Vec<String> = result
        .column_info()
        .iter()
        .map(|c| c.name().to_owned())
        .collect();
    let mut out = Vec::new();
    for row in result {
        let row = row.map_err(|e| to_db_error(&e))?;
        let values = (0..columns.len())
            .map(|i| row.get::<usize, Option<String>>(i).unwrap_or(None))
            .collect();
        out.push(values);
    }
    Ok((columns, out))
}

fn execute(inner: &mut Inner, sql: &str) -> Outcome {
    let words = crate::script::code_words(sql, Dialect::Oracle);
    let first = words.first().map(String::as_str);
    let second = words.get(1).map(String::as_str);
    // Transaction control goes to the driver, so its state stays consistent.
    match (first, second) {
        (Some("commit"), _) => {
            inner.in_transaction = false;
            return match inner.conn.commit() {
                Ok(()) => Outcome::UpdateCount(0),
                Err(e) => Outcome::Error(to_db_error(&e)),
            };
        }
        (Some("rollback"), None) => {
            inner.in_transaction = false;
            return match inner.conn.rollback() {
                Ok(()) => Outcome::UpdateCount(0),
                Err(e) => Outcome::Error(to_db_error(&e)),
            };
        }
        (Some("set"), Some("transaction")) => inner.in_transaction = true,
        _ => {}
    }
    let statement = inner.conn.statement(sql).build();
    let mut statement = match statement {
        Ok(s) => s,
        Err(e) => return Outcome::Error(to_db_error(&e)),
    };
    if statement.is_query() {
        drop(statement);
        return match rows(&inner.conn, sql) {
            Ok((columns, rows)) => Outcome::ResultSet { columns, rows },
            Err(e) => Outcome::Error(e),
        };
    }
    let outcome = match statement.execute(&[]) {
        Ok(()) => Outcome::UpdateCount(statement.row_count().unwrap_or(0)),
        Err(e) => return Outcome::Error(to_db_error(&e)),
    };
    if !inner.in_transaction
        && let Err(e) = inner.conn.commit()
    {
        return Outcome::Error(to_db_error(&e));
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_strings() {
        let mut p = ConnectionProfile::from_url("o", "oracle://scott@db:1522/FREEPDB1").unwrap();
        assert_eq!(connect_string(&p), "//db:1522/FREEPDB1");
        p.ssl = SslMode::Require;
        assert_eq!(connect_string(&p), "tcps://db:1522/FREEPDB1");
        let d = ConnectionProfile::from_url("d", "oracle://scott@h/XE").unwrap();
        assert_eq!(connect_string(&d), "//h:1521/XE");
    }

    #[test]
    fn read_only_refuses_changes_but_not_queries() {
        assert!(refusal("select 1 from dual", true).is_none());
        assert!(refusal("set transaction read only", true).is_none());
        assert!(refusal("delete from t", true).is_some());
        assert!(refusal("begin null; end;", true).is_some());
        assert!(refusal("delete from t", false).is_none());
    }
}
