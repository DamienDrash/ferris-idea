//! SQL Server sessions (`SPEC-0116`, ADR-0011).
//!
//! T-SQL over TDS with the pure-Rust driver `tiberius`. Scripts are split at `;` and `GO` lines;
//! each statement is sent as its own batch, so session options (`SET …`) stay in force for the
//! next one; a statement with a variable or control of flow holds the rest of its batch
//! (`SPEC-0005` v6 behaviour 11a). Every result set of a batch is an outcome of its own
//! (`SPEC-0116` v5 behaviour 3). Statements that change rows go through `sp_executesql` to learn
//! the row count.

use std::sync::Arc;

use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, FromSql, QueryItem};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::profile::expand_home;
use crate::session::{
    CONNECT_TIMEOUT, SessionError, TlsPlan, mode_name, pem_certificates, prefer_needs_tls, prepare,
    tls_plan,
};
use crate::tds_tls::{Anchors, HandshakeFrames, Seen, too_old, version_name};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, SslMode,
    StatementClass, classify, split_statements,
};

type Conn = Client<HandshakeFrames<Compat<TcpStream>>>;

/// Why an attempt to connect failed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    /// TLS did not come about: the server declined it, the handshake broke, or the server chose a
    /// version below TLS 1.2.
    Tls(String),
    /// Anything else: the address, the login, the time limit.
    Other(String),
}

impl Failure {
    fn text(&self) -> &str {
        match self {
            Self::Tls(m) | Self::Other(m) => m,
        }
    }

    /// Sorts a driver error with what the handshake showed: the CA file's refusal (`SPEC-0116` v7
    /// behaviour 15), the version the server chose in its ServerHello, if it got that far.
    fn of(e: &tiberius::error::Error, seen: &Seen) -> Self {
        if let Some(refusal) = seen.refusal() {
            return Self::Tls(refusal);
        }
        let version = seen.version();
        if too_old(version) {
            return Self::Tls(format!(
                "the server chose {}; Ferris IDEA needs TLS 1.2 or newer, enable it on the server \
                 (`mssql-conf set network.tlsprotocols 1.2` on Linux)",
                version_name(version)
            ));
        }
        match e {
            tiberius::error::Error::Tls(m) => Self::Tls(format!("TLS handshake failed: {m}")),
            tiberius::error::Error::Protocol(m) if m.contains("encryption") => {
                Self::Tls(format!("the server does not offer encryption ({m})"))
            }
            other => Self::Other(other.to_string()),
        }
    }
}

/// Whether a failed encrypted attempt is tried once more without TLS (`SPEC-0116` v6 behaviour 2):
/// only for `prefer` on the loopback, and only when TLS itself failed.
fn retry_without_tls(plan: TlsPlan, failure: &Failure) -> bool {
    plan == TlsPlan::Optional && matches!(failure, Failure::Tls(_))
}

/// One result set: column names and rows as text.
pub type ResultSet = (Vec<String>, Vec<Vec<Option<String>>>);

/// An open connection to a SQL Server.
pub struct SqlServerSession {
    conn: Mutex<Conn>,
    read_only: bool,
    /// Whether the driver and the server agreed on encryption (`SPEC-0116` v5 behaviour 13).
    encrypted: bool,
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
        config.application_name("Ferris IDEA");
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
        let plan = tls_plan(profile);
        // A CA file (`SPEC-0116` v6 behaviour 15) is read before anything is sent; v7: its
        // certificates are the only anchors the server's chain may lead to.
        let ca = match profile.tls.ca_file.as_deref().map(expand_home) {
            Some(path) => {
                let tls_error = |message| SessionError::Tls {
                    profile: profile.name.clone(),
                    mode: mode_name(mode),
                    message,
                };
                let certificates = pem_certificates(&path)
                    .and_then(|certs| {
                        certs
                            .iter()
                            .map(|c| c.to_der().map_err(|e| format!("{}: {e}", path.display())))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .map_err(tls_error)?;
                Some(Anchors {
                    file: path.display().to_string(),
                    certificates,
                })
            }
            None => None,
        };
        let open = |encrypt: bool| {
            let mut config = config.clone();
            let mut anchors = None;
            if encrypt {
                config.encryption(EncryptionLevel::Required);
                // verify-ca is checked as fully as verify-full here (tiberius checks both or neither).
                if !mode.verifies() {
                    config.trust_cert();
                } else if let Some(ca) = &ca {
                    // The driver adds the file to the system's roots; the stream below it lets the
                    // login leave only when the chain leads to the file (`SPEC-0116` v7 behaviour 15).
                    config.trust_cert_ca(&ca.file);
                    anchors = Some(ca.clone());
                }
            } else {
                config.encryption(EncryptionLevel::NotSupported);
            }
            async move {
                let seen = Arc::new(Seen::default());
                let attempt = async {
                    let tcp = TcpStream::connect(config.get_addr())
                        .await
                        .map_err(|e| Failure::Other(e.to_string()))?;
                    tcp.set_nodelay(true)
                        .map_err(|e| Failure::Other(e.to_string()))?;
                    // The handshake's records in one message per flight (`SPEC-0116` v6
                    // behaviour 14), the server's TLS version and, with a CA file, its chain
                    // checked (behaviour 15).
                    let stream =
                        HandshakeFrames::new(tcp.compat_write(), Arc::clone(&seen), anchors);
                    Client::connect(config, stream)
                        .await
                        .map_err(|e| Failure::of(&e, &seen))
                };
                match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
                    Ok(result) => result,
                    Err(_) => Err(Failure::Other(format!(
                        "no answer within {} s",
                        CONNECT_TIMEOUT.as_secs()
                    ))),
                }
            }
        };
        let address = format!("{host}:{port}");
        let (conn, encrypted) = match plan {
            TlsPlan::Off => (
                open(false)
                    .await
                    .map_err(|f| connect_error(f.text().to_owned()))?,
                false,
            ),
            TlsPlan::Optional | TlsPlan::Required => match open(true).await {
                Ok(conn) => (conn, true),
                // `prefer` on the loopback: once without TLS, after TLS itself failed.
                Err(failure) if retry_without_tls(plan, &failure) => (
                    open(false)
                        .await
                        .map_err(|f| connect_error(f.text().to_owned()))?,
                    false,
                ),
                // `prefer` off the loopback needs TLS (`SPEC-0016` v3 behaviour 13).
                Err(Failure::Tls(reason)) if mode == SslMode::Prefer => {
                    return Err(prefer_needs_tls(profile, &address, &reason));
                }
                Err(Failure::Other(message)) if mode == SslMode::Prefer => {
                    return Err(connect_error(message));
                }
                Err(failure) => {
                    return Err(SessionError::Tls {
                        profile: profile.name.clone(),
                        mode: mode_name(mode),
                        message: format!("{address}: {}", failure.text()),
                    });
                }
            },
        };
        Ok(Self {
            conn: Mutex::new(conn),
            read_only: profile.read_only,
            encrypted,
        })
    }

    /// Whether the connection was opened encrypted, as the driver agreed it with the server. SQL
    /// Server tells an ordinary login nothing about its connection (`SPEC-0116` v5 behaviour 13).
    pub fn encrypted(&self) -> bool {
        self.encrypted
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let statements = split_statements(script, Dialect::SqlServer);
        let mut run = ScriptRun::default();
        let mut conn = self.conn.lock().await;
        for (index, statement) in statements.iter().enumerate() {
            let outcomes = match refusal(&statement.text, self.read_only) {
                Some(error) => vec![Outcome::Error(error)],
                None => execute(&mut conn, &statement.text).await,
            };
            // Every result set of the batch, the failure last (`SPEC-0005` v4 behaviour 18).
            let mut failed = false;
            for outcome in outcomes {
                failed |= run.push(script, statement, outcome);
            }
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
        match result_sets(&mut conn, sql).await {
            (sets, None) => Ok(sets),
            (_, Some(error)) => Err(error),
        }
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

/// Runs one batch and returns an outcome per result set, in the order the server sends them: an
/// `EXEC` of a procedure with two `SELECT`s, two `SELECT`s without `;` between them, the queries
/// of an `IF … BEGIN … END` (`SPEC-0116` v5 behaviour 3). A batch without a result set gives its
/// row count. A failure ends the list, after the sets read before it.
async fn execute(conn: &mut Conn, sql: &str) -> Vec<Outcome> {
    if classify(sql, Dialect::SqlServer) == StatementClass::ModifyData && !returns_rows(sql) {
        return vec![match conn.execute(sql, &[]).await {
            Ok(result) => Outcome::UpdateCount(result.total()),
            Err(e) => Outcome::Error(to_db_error(e, sql)),
        }];
    }
    let (sets, error) = result_sets(conn, sql).await;
    let mut out: Vec<Outcome> = sets
        .into_iter()
        .map(|(columns, rows)| Outcome::ResultSet { columns, rows })
        .collect();
    match error {
        Some(e) => out.push(Outcome::Error(e)),
        None if out.is_empty() => out.push(Outcome::UpdateCount(0)),
        None => {}
    }
    out
}

/// `OUTPUT` clauses and procedure calls can return rows; they run as plain batches.
fn returns_rows(sql: &str) -> bool {
    let words = crate::script::code_words(sql, Dialect::SqlServer);
    words
        .first()
        .is_some_and(|w| matches!(w.as_str(), "exec" | "execute"))
        || words.iter().any(|w| w == "output")
}

/// Every result set of the batch `sql` in order, and the error that ended it early, if one did:
/// the sets read before a failure are kept.
async fn result_sets(conn: &mut Conn, sql: &str) -> (Vec<ResultSet>, Option<DbError>) {
    let mut stream = match conn.simple_query(sql).await {
        Ok(stream) => stream,
        Err(e) => return (Vec::new(), Some(to_db_error(e, sql))),
    };
    let mut sets: Vec<ResultSet> = Vec::new();
    loop {
        let item = match stream.try_next().await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(e) => return (sets, Some(to_db_error(e, sql))),
        };
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
    (sets, None)
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
        ColumnData::Numeric(v) => v.map(|v| decimal_text(v.value(), v.scale())),
        ColumnData::Xml(v) => v.as_ref().map(|v| v.to_string()),
        ColumnData::DateTime(_)
        | ColumnData::SmallDateTime(_)
        | ColumnData::Time(_)
        | ColumnData::Date(_)
        | ColumnData::DateTime2(_)
        | ColumnData::DateTimeOffset(_) => date_time(),
    }
}

/// A `DECIMAL`/`NUMERIC` value with exactly its scale's digits after the point, none and no point
/// for scale 0 (`SPEC-0116` v10 behaviour 16): `4`, `0.05`, `-1.50`.
pub(crate) fn decimal_text(value: i128, scale: u8) -> String {
    let digits = value.unsigned_abs().to_string();
    let scale = usize::from(scale);
    let digits = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = digits.split_at(digits.len() - scale);
    let sign = if value < 0 { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
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

    /// SPEC-0116 v10 AK-14 (OBS-0120 X3): decimals keep their scale; scale 0 has no point.
    #[test]
    fn decimals_keep_their_scale() {
        assert_eq!(decimal_text(4, 0), "4");
        assert_eq!(decimal_text(5, 2), "0.05");
        assert_eq!(decimal_text(-150, 2), "-1.50");
        assert_eq!(decimal_text(-5, 3), "-0.005");
        assert_eq!(decimal_text(0, 0), "0");
        assert!(decimal_text(i128::MIN, 38).starts_with("-1.7014"));
        assert_eq!(
            decimal_text(99_999_999_999_999_999_999_999_999_999_999_999_999, 0),
            "99999999999999999999999999999999999999"
        );
    }

    /// SPEC-0116 v6 AK-10: only `prefer` on the loopback tries again without TLS, and only after a
    /// TLS failure; a login or address failure is never repeated in clear.
    #[test]
    fn prefer_retries_only_after_tls_failed() {
        let tls = Failure::Tls("TLS handshake failed: closed".into());
        let login = Failure::Other("Login failed for user 'u'".into());
        assert!(retry_without_tls(TlsPlan::Optional, &tls));
        assert!(!retry_without_tls(TlsPlan::Optional, &login));
        assert!(!retry_without_tls(TlsPlan::Required, &tls));
        assert!(!retry_without_tls(TlsPlan::Off, &tls));
        // How driver errors are sorted.
        let declined = tiberius::error::Error::Protocol(
            "Server does not allow the requested encryption level.".into(),
        );
        let nothing = Seen::default();
        assert!(
            matches!(Failure::of(&declined, &nothing), Failure::Tls(m) if m.contains("does not offer"))
        );
        let broken = tiberius::error::Error::Tls("connection closed via error".into());
        assert!(matches!(
            Failure::of(&broken, &Seen::with_version(0x0303)),
            Failure::Tls(_)
        ));
        let old = Failure::of(&broken, &Seen::with_version(0x0301));
        assert!(
            matches!(&old, Failure::Tls(m) if m.contains("TLS 1.0") && m.contains("tlsprotocols")),
            "{old:?}"
        );
        let refused = tiberius::error::Error::Io {
            kind: std::io::ErrorKind::ConnectionRefused,
            message: "refused".into(),
        };
        assert!(matches!(Failure::of(&refused, &nothing), Failure::Other(_)));
    }

    /// SPEC-0116 v7 AK-11: when the stream stopped the handshake for the CA file, its reason is
    /// the error, whatever the driver made of the broken stream.
    #[test]
    fn the_ca_files_refusal_is_the_error() {
        let seen = Seen::with_version(0x0303);
        seen.set_refusal(
            "the server's certificate was not issued by a certificate in the CA file ca.pem",
        );
        let io = tiberius::error::Error::Io {
            kind: std::io::ErrorKind::InvalidData,
            message: "x".into(),
        };
        assert!(matches!(Failure::of(&io, &seen), Failure::Tls(m) if m.contains("CA file ca.pem")));
    }

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
