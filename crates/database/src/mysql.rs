//! MySQL sessions (`SPEC-0010`, ADR-0011).
//!
//! Statements run over the text protocol, so values arrive as text; errors
//! keep SQLSTATE and the MySQL error number.

use mysql_async::prelude::Queryable;
use mysql_async::{ClientIdentity, Conn, OptsBuilder, SslOpts, Value};
use tokio::sync::Mutex;

use crate::profile::expand_home;
use crate::session::{
    SessionError, TlsPlan, Via, mode_name, pem_certificates, prefer_needs_tls, prepare,
    read_private, tls_plan,
};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, SslMode,
    split_statements,
};

/// An open connection to a MySQL server.
pub struct MySqlSession {
    conn: Mutex<Conn>,
    dialect: Dialect,
    /// The server's id of this connection, for `KILL QUERY` (`SPEC-0093` v3 behaviour 12).
    id: u32,
}

/// The TLS options of `profile` (SPEC-0016 v2): checks of the mode, its CA file instead of the
/// system's roots, its PKCS#12 client certificate, the host name to check through a tunnel.
fn ssl_opts(profile: &ConnectionProfile, host: &str, tunnelled: bool) -> Result<SslOpts, String> {
    let mode = profile.ssl;
    let mut ssl = SslOpts::default();
    match mode {
        SslMode::VerifyFull => {}
        SslMode::VerifyCa => ssl = ssl.with_danger_skip_domain_validation(true),
        _ => {
            ssl = ssl
                .with_danger_accept_invalid_certs(true)
                .with_danger_skip_domain_validation(true);
        }
    }
    if let Some(ca) = profile.tls.ca_file.as_deref().map(expand_home) {
        // Parsed here, so a bad file is named before connecting.
        pem_certificates(&ca)?;
        ssl = ssl
            .with_root_certs(vec![ca.into()])
            .with_disable_built_in_roots(true);
    }
    if let Some(cert) = profile.tls.cert_file.as_deref().map(expand_home) {
        let der = read_private(&cert)?;
        ssl = ssl.with_client_identity(Some(ClientIdentity::new(der.into())));
    }
    if tunnelled {
        ssl = ssl.with_danger_tls_hostname_override(Some(host.to_owned()));
    }
    Ok(ssl)
}

impl MySqlSession {
    /// Opens a session for `profile` with already resolved `credentials`.
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        Self::connect_via(profile, credentials, None).await
    }

    /// [`MySqlSession::connect`] through an SSH tunnel's local address `via` (`SPEC-0108` v4
    /// behaviour 8): the socket goes to `via`, TLS still checks the profile's host name.
    pub async fn connect_via(
        profile: &ConnectionProfile,
        credentials: &Credentials,
        via: Via,
    ) -> Result<Self, SessionError> {
        // MariaDB speaks the same protocol; the profile's dialect picks catalog and plan queries.
        let dialect = if profile.dialect == Dialect::MariaDb {
            Dialect::MariaDb
        } else {
            Dialect::MySql
        };
        let password = prepare(profile, credentials, dialect)?;
        let host = profile.host.clone().unwrap_or_else(|| "localhost".into());
        let port = profile.effective_port().unwrap_or(3306);
        let limit = profile.options.connect_timeout();
        let mut init = Vec::new();
        if profile.read_only {
            init.push("SET SESSION TRANSACTION READ ONLY".to_owned());
        }
        // SPEC-0108 v4 behaviour 10: the server ends statements that run too long.
        if let Some(secs) = profile.options.query_timeout_secs {
            init.push(if dialect == Dialect::MariaDb {
                format!("SET SESSION max_statement_time = {secs}")
            } else {
                format!(
                    "SET SESSION max_execution_time = {}",
                    u64::from(secs) * 1000
                )
            });
        }
        let (connect_host, connect_port) = match via {
            Some(local) => (local.ip().to_string(), local.port()),
            None => (host.clone(), port),
        };
        let base = OptsBuilder::default()
            .ip_or_hostname(connect_host)
            .tcp_port(connect_port)
            .user(profile.user.clone())
            .pass(password)
            .db_name(profile.database.clone())
            .tcp_keepalive(
                profile
                    .options
                    .keep_alive_secs
                    .map(|s| std::time::Duration::from_secs(u64::from(s))),
            )
            .init(init);
        let address = match via {
            Some(local) => format!("{host}:{port} via SSH tunnel {local}"),
            None => format!("{host}:{port}"),
        };
        let connect_error = |message: String| SessionError::Connect {
            profile: profile.name.clone(),
            address: address.clone(),
            message,
        };
        let mode = profile.ssl;
        // `prefer` off the loopback needs TLS like `require` (`SPEC-0016` v3 behaviour 3).
        let plan = tls_plan(profile);
        // The message, and whether TLS itself failed (the server offers none, or the handshake
        // broke): only then may `prefer` on the loopback try once without it.
        let open = |opts: OptsBuilder| async move {
            match tokio::time::timeout(limit, Conn::new(opts)).await {
                Ok(result) => result.map_err(|e| {
                    let tls = matches!(
                        e,
                        mysql_async::Error::Driver(
                            mysql_async::DriverError::NoClientSslFlagFromServer
                        ) | mysql_async::Error::Io(mysql_async::IoError::Tls(_))
                    );
                    (e.to_string(), tls)
                }),
                Err(_) => Err((format!("no answer within {} s", limit.as_secs()), false)),
            }
        };
        let plain = |(message, _): (String, bool)| connect_error(message);
        let conn = if plan == TlsPlan::Off {
            open(base).await.map_err(plain)?
        } else {
            let ssl =
                ssl_opts(profile, &host, via.is_some()).map_err(|message| SessionError::Tls {
                    profile: profile.name.clone(),
                    mode: mode_name(mode),
                    message,
                })?;
            match open(base.clone().ssl_opts(Some(ssl))).await {
                Ok(conn) => conn,
                // `prefer` on the loopback: once without TLS, after TLS itself failed.
                Err((_, true)) if plan == TlsPlan::Optional => open(base).await.map_err(plain)?,
                // `prefer` off the loopback without TLS: what to do (`SPEC-0016` v3 behaviour 13).
                Err((message, true)) if mode == SslMode::Prefer => {
                    return Err(prefer_needs_tls(profile, &address, &message));
                }
                Err((message, false)) if mode == SslMode::Prefer => {
                    return Err(connect_error(message));
                }
                Err((message, _)) => {
                    return Err(SessionError::Tls {
                        profile: profile.name.clone(),
                        mode: mode_name(mode),
                        message: format!("{address}: {message}"),
                    });
                }
            }
        };
        let id = conn.id();
        Ok(Self {
            conn: Mutex::new(conn),
            dialect,
            id,
        })
    }

    /// The server's id of this connection (`SPEC-0093` v3 behaviour 12).
    pub fn connection_id(&self) -> u32 {
        self.id
    }
    /// MySQL or MariaDB, as the profile says.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let statements = split_statements(script, self.dialect);
        let mut run = ScriptRun::default();
        let mut conn = self.conn.lock().await;
        for (index, statement) in statements.iter().enumerate() {
            // Every result set of the statement (`SPEC-0005` v4 behaviour 18), the failure last.
            let mut failed = false;
            for outcome in execute(&mut conn, &statement.text, self.dialect).await {
                failed |= run.push(script, statement, outcome);
            }
            if failed && on_error == OnError::Stop {
                run.not_run = statements.len() - index - 1;
                break;
            }
        }
        run
    }
}

/// Runs one statement and returns an outcome per result set, in the order the server sends them
/// (`SPEC-0005` v4 behaviour 18: a `CALL` of a procedure with several `SELECT`s); the status after
/// a procedure's result sets is no outcome of its own, and a statement without a result set gives
/// its update count. A failure ends the list, after the sets read before it.
async fn execute(conn: &mut Conn, sql: &str, dialect: Dialect) -> Vec<Outcome> {
    let mut result = match conn.query_iter(sql).await {
        Ok(result) => result,
        Err(e) => return vec![Outcome::Error(to_db_error(e, dialect, sql))],
    };
    let mut sets = Vec::new();
    // A statement without rows (`UPDATE`) is "empty" at once: its count is read before the loop.
    let mut affected = result.affected_rows();
    while !result.is_empty() {
        let columns: Option<Vec<String>> = result
            .columns()
            .filter(|c| !c.is_empty())
            .map(|c| c.iter().map(|col| col.name_str().into_owned()).collect());
        let rows = match result.collect::<mysql_async::Row>().await {
            Ok(rows) => rows,
            Err(e) => {
                sets.push(Outcome::Error(to_db_error(e, dialect, sql)));
                return sets;
            }
        };
        affected = result.affected_rows();
        if let Some(columns) = columns {
            sets.push(Outcome::ResultSet {
                columns,
                rows: rows
                    .into_iter()
                    .map(|row| row.unwrap().into_iter().map(text_of).collect())
                    .collect(),
            });
        }
    }
    if let Err(e) = result.drop_result().await {
        sets.push(Outcome::Error(to_db_error(e, dialect, sql)));
        return sets;
    }
    if sets.is_empty() {
        sets.push(Outcome::UpdateCount(affected));
    }
    sets
}

/// Renders a text-protocol value; non-UTF-8 bytes become `0x…`.
fn text_of(value: Value) -> Option<String> {
    match value {
        Value::NULL => None,
        Value::Bytes(bytes) => Some(String::from_utf8(bytes).unwrap_or_else(|e| {
            let hex: String = e.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
            format!("0x{hex}")
        })),
        other => Some(other.as_sql(true).trim_matches('\'').to_owned()),
    }
}

fn to_db_error(error: mysql_async::Error, dialect: Dialect, sql: &str) -> DbError {
    match error {
        mysql_async::Error::Server(server) => DbError {
            position: syntax_location(&server.message)
                .and_then(|(near, line)| crate::outcome::position_near(sql, line, near)),
            message: format!(
                "{} error {}: {}",
                dialect.label(),
                server.code,
                server.message
            ),
            sqlstate: Some(server.state),
        },
        other => DbError {
            message: other.to_string(),
            sqlstate: None,
            position: None,
        },
    }
}

/// The text a syntax error names and its line: `… near 'frm t' at line 2` (SPEC-0010 v2
/// behaviour 4).
fn syntax_location(message: &str) -> Option<(&str, usize)> {
    let (_, rest) = message.split_once(" near '")?;
    let (near, line) = rest.rsplit_once("' at line ")?;
    let digits = line.trim_end_matches(|c: char| !c.is_ascii_digit());
    Some((near, digits.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_values_render_as_hex() {
        assert_eq!(
            text_of(Value::Bytes(b"abc".to_vec())).as_deref(),
            Some("abc")
        );
        assert_eq!(
            text_of(Value::Bytes(vec![0xff, 0x00])).as_deref(),
            Some("0xff00")
        );
        assert_eq!(text_of(Value::NULL), None);
    }

    #[tokio::test]
    async fn refuses_other_dialects() {
        let p = ConnectionProfile::from_url("p", "postgresql://u@127.0.0.1:1/db").unwrap();
        let err = MySqlSession::connect(&p, &Credentials::NotNeeded)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, SessionError::WrongDialect(..)), "{err}");
    }

    #[test]
    fn syntax_errors_name_their_text_and_line() {
        let m = "You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version for the right syntax to use near 'frm movement' at line 2";
        assert_eq!(syntax_location(m), Some(("frm movement", 2)));
        let m = "You have an error in your SQL syntax; ... near '' at line 1";
        assert_eq!(syntax_location(m), Some(("", 1)));
        assert_eq!(syntax_location("Unknown column 'x' in 'field list'"), None);
    }
}
