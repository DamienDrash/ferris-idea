//! SQL Server sessions (`SPEC-0116`, ADR-0011).
//!
//! T-SQL over TDS with the pure-Rust driver `tiberius`. Scripts are split at `;` and `GO` lines;
//! each statement is sent as its own batch, so session options (`SET …`) stay in force for the
//! next one. Statements that change rows go through `sp_executesql` to learn the row count.

use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, FromSql, QueryItem};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::session::{CONNECT_TIMEOUT, SessionError, mode_name, prepare};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, SslMode,
    StatementClass, classify, split_statements,
};

type Conn = Client<Compat<TcpStream>>;

/// One result set: column names and rows as text.
pub type ResultSet = (Vec<String>, Vec<Vec<Option<String>>>);

/// An open connection to a SQL Server.
pub struct SqlServerSession {
    conn: Mutex<Conn>,
    read_only: bool,
}

impl SqlServerSession {
    /// Opens a session for `profile` with already resolved `credentials`.
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        let password = prepare(profile, credentials, Dialect::SqlServer)?;
        let host = profile.host.clone().unwrap_or_else(|| "localhost".into());
        let port = profile.effective_port().unwrap_or(1433);
        let mut config = Config::new();
        config.host(&host);
        config.port(port);
        config.application_name("IDECraft");
        if let Some(database) = &profile.database {
            config.database(database);
        }
        config.authentication(AuthMethod::sql_server(
            profile.user.clone().unwrap_or_default(),
            password.unwrap_or_default(),
        ));
        let connect_error = |message: String| SessionError::Connect {
            profile: profile.name.clone(),
            address: format!("{host}:{port}"),
            message,
        };
        let mode = profile.ssl;
        let open = |encrypt: bool| {
            let mut config = config.clone();
            if encrypt {
                config.encryption(EncryptionLevel::Required);
                // verify-ca is checked as fully as verify-full here (tiberius checks both or neither).
                if !mode.verifies() {
                    config.trust_cert();
                }
            } else {
                config.encryption(EncryptionLevel::NotSupported);
            }
            async move {
                let attempt = async {
                    let tcp = TcpStream::connect(config.get_addr())
                        .await
                        .map_err(|e| e.to_string())?;
                    tcp.set_nodelay(true).map_err(|e| e.to_string())?;
                    Client::connect(config, tcp.compat_write())
                        .await
                        .map_err(|e| e.to_string())
                };
                match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
                    Ok(result) => result,
                    Err(_) => Err(format!("no answer within {} s", CONNECT_TIMEOUT.as_secs())),
                }
            }
        };
        let conn = if mode == SslMode::Disable {
            open(false).await.map_err(connect_error)?
        } else {
            match open(true).await {
                Ok(conn) => conn,
                // `prefer` tolerates a server without TLS: try once without it.
                Err(_) if mode == SslMode::Prefer => open(false).await.map_err(connect_error)?,
                Err(message) => {
                    return Err(SessionError::Tls {
                        profile: profile.name.clone(),
                        mode: mode_name(mode),
                        message: format!("{host}:{port}: {message}"),
                    });
                }
            }
        };
        Ok(Self {
            conn: Mutex::new(conn),
            read_only: profile.read_only,
        })
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let statements = split_statements(script, Dialect::SqlServer);
        let mut run = ScriptRun::default();
        let mut conn = self.conn.lock().await;
        for (index, statement) in statements.iter().enumerate() {
            let outcome = match refusal(&statement.text, self.read_only) {
                Some(error) => Outcome::Error(error),
                None => execute(&mut conn, &statement.text).await,
            };
            let failed = run.push(script, statement, outcome);
            if failed && on_error == OnError::Stop {
                run.not_run = statements.len() - index - 1;
                break;
            }
        }
        run
    }

    /// Every result set of one batch, in order (plans arrive after the query's own rows).
    pub async fn result_sets(&self, sql: &str) -> Result<Vec<ResultSet>, DbError> {
        if let Some(error) = refusal(sql, self.read_only) {
            return Err(error);
        }
        let mut conn = self.conn.lock().await;
        result_sets(&mut conn, sql).await
    }
}

/// SQL Server has no read-only session mode: a read-only profile refuses everything that is not
/// known to only read, except session options (`SET …`).
fn refusal(sql: &str, read_only: bool) -> Option<DbError> {
    let first = crate::script::code_words(sql, Dialect::SqlServer)
        .into_iter()
        .next();
    (read_only
        && classify(sql, Dialect::SqlServer) != StatementClass::ReadData
        && first.as_deref() != Some("set"))
    .then(|| DbError {
        message: "the connection is read-only; only queries and SET options run".into(),
        sqlstate: None,
        position: None,
    })
}

async fn execute(conn: &mut Conn, sql: &str) -> Outcome {
    if classify(sql, Dialect::SqlServer) == StatementClass::ModifyData && !returns_rows(sql) {
        return match conn.execute(sql, &[]).await {
            Ok(result) => Outcome::UpdateCount(result.total()),
            Err(e) => Outcome::Error(to_db_error(e, sql)),
        };
    }
    match result_sets(conn, sql).await {
        Ok(sets) => match sets.into_iter().next() {
            Some((columns, rows)) => Outcome::ResultSet { columns, rows },
            None => Outcome::UpdateCount(0),
        },
        Err(e) => Outcome::Error(e),
    }
}

/// `OUTPUT` clauses and procedure calls can return rows; they run as plain batches.
fn returns_rows(sql: &str) -> bool {
    let words = crate::script::code_words(sql, Dialect::SqlServer);
    words
        .first()
        .is_some_and(|w| matches!(w.as_str(), "exec" | "execute"))
        || words.iter().any(|w| w == "output")
}

async fn result_sets(conn: &mut Conn, sql: &str) -> Result<Vec<ResultSet>, DbError> {
    let mut stream = conn
        .simple_query(sql)
        .await
        .map_err(|e| to_db_error(e, sql))?;
    let mut sets: Vec<ResultSet> = Vec::new();
    while let Some(item) = stream.try_next().await.map_err(|e| to_db_error(e, sql))? {
        match item {
            QueryItem::Metadata(meta) => {
                sets.push((
                    meta.columns().iter().map(|c| c.name().to_owned()).collect(),
                    Vec::new(),
                ));
            }
            QueryItem::Row(row) => {
                if let Some((_, rows)) = sets.last_mut() {
                    rows.push(row.into_iter().map(|data| text_of(&data)).collect());
                }
            }
        }
    }
    Ok(sets)
}

/// Renders one value as text; binary data becomes `0x…`.
fn text_of(data: &ColumnData<'static>) -> Option<String> {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
    let date_time = || -> Option<String> {
        match data {
            ColumnData::Date(_) => NaiveDate::from_sql(data).ok()?.map(|d| d.to_string()),
            ColumnData::Time(_) => NaiveTime::from_sql(data).ok()?.map(|t| t.to_string()),
            ColumnData::DateTimeOffset(_) => DateTime::<FixedOffset>::from_sql(data)
                .ok()?
                .map(|t| t.to_string()),
            _ => NaiveDateTime::from_sql(data)
                .ok()?
                .map(|t| t.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
        }
    };
    match data {
        ColumnData::U8(v) => v.map(|v| v.to_string()),
        ColumnData::I16(v) => v.map(|v| v.to_string()),
        ColumnData::I32(v) => v.map(|v| v.to_string()),
        ColumnData::I64(v) => v.map(|v| v.to_string()),
        ColumnData::F32(v) => v.map(|v| v.to_string()),
        ColumnData::F64(v) => v.map(|v| v.to_string()),
        ColumnData::Bit(v) => v.map(|v| if v { "1" } else { "0" }.to_owned()),
        ColumnData::String(v) => v.as_ref().map(|v| v.to_string()),
        ColumnData::Guid(v) => v.map(|v| v.to_string().to_uppercase()),
        ColumnData::Binary(v) => v.as_ref().map(|bytes| {
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("0x{hex}")
        }),
        ColumnData::Numeric(v) => v.map(|v| v.to_string()),
        ColumnData::Xml(v) => v.as_ref().map(|v| v.to_string()),
        ColumnData::DateTime(_)
        | ColumnData::SmallDateTime(_)
        | ColumnData::Time(_)
        | ColumnData::Date(_)
        | ColumnData::DateTime2(_)
        | ColumnData::DateTimeOffset(_) => date_time(),
    }
}

fn to_db_error(error: tiberius::error::Error, sql: &str) -> DbError {
    match error {
        tiberius::error::Error::Server(token) => DbError {
            message: format!(
                "SQL Server error {} (line {}): {}",
                token.code(),
                token.line(),
                token.message()
            ),
            sqlstate: None,
            position: crate::outcome::position_of_line(sql, token.line() as usize),
        },
        other => DbError {
            message: other.to_string(),
            sqlstate: None,
            position: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_render_as_text() {
        assert_eq!(text_of(&ColumnData::I32(Some(7))).as_deref(), Some("7"));
        assert_eq!(text_of(&ColumnData::Bit(Some(true))).as_deref(), Some("1"));
        assert_eq!(text_of(&ColumnData::I64(None)), None);
        assert_eq!(
            text_of(&ColumnData::Binary(Some(vec![0xff, 0x00].into()))).as_deref(),
            Some("0xff00")
        );
        assert_eq!(
            text_of(&ColumnData::String(Some("Grüße".into()))).as_deref(),
            Some("Grüße")
        );
    }

    #[test]
    fn read_only_refuses_changes_but_not_options() {
        assert!(refusal("select 1", true).is_none());
        assert!(refusal("set showplan_xml on", true).is_none());
        assert!(refusal("delete from t", true).is_some());
        assert!(refusal("drop table t", true).is_some());
        assert!(refusal("delete from t", false).is_none());
    }

    #[test]
    fn output_clauses_and_procedures_return_rows() {
        assert!(returns_rows("exec sp_who"));
        assert!(returns_rows("delete from t output deleted.id where id = 1"));
        assert!(!returns_rows("update t set a = 'output' where id = 1"));
    }

    #[tokio::test]
    async fn refuses_other_dialects() {
        let p = ConnectionProfile::from_url("p", "mysql://u@127.0.0.1:1/db").unwrap();
        let err = SqlServerSession::connect(&p, &Credentials::NotNeeded)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, SessionError::WrongDialect(..)), "{err}");
    }

    #[tokio::test]
    async fn unreachable_server_is_a_connect_error() {
        let p =
            ConnectionProfile::from_url("p", "sqlserver://sa@127.0.0.1:1/master?sslmode=disable")
                .unwrap();
        let err = SqlServerSession::connect(&p, &Credentials::NotNeeded)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, SessionError::Connect { .. }), "{err}");
    }
}
