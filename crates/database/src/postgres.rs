//! PostgreSQL sessions (`SPEC-0009`, ADR-0011).
//!
//! Statements run over the simple query protocol, so every value arrives as
//! text whatever its type, and server errors keep SQLSTATE and position.

use std::sync::{Arc, Mutex};
use tokio_postgres::error::ErrorPosition;

use tokio_postgres::{AsyncMessage, Client, Connection, NoTls, SimpleQueryMessage};

use crate::session::{SessionError, Via, mode_name, prepare, profile_tls_connector};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, SslMode,
    split_statements,
};

/// An open connection to a PostgreSQL server.
pub struct PgSession {
    client: Client,
    /// The server's notices (`RAISE NOTICE`, warnings) since the running statement began
    /// (`SPEC-0093` v8 behaviour 6a).
    notices: Notices,
}

/// Notices the connection task collects, as `SEVERITY: message`.
type Notices = Arc<Mutex<Vec<String>>>;

/// Drives `connection` until the client is dropped or the server closes it, keeping the server's
/// notices for the statement that caused them. A notice reaches the connection before the end of
/// its statement's answer, so it is kept before the statement's caller sees the answer.
fn drive<S, T>(mut connection: Connection<S, T>, notices: Notices)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                Some(Ok(AsyncMessage::Notice(notice))) => {
                    if let Ok(mut kept) = notices.lock() {
                        kept.push(format!("{}: {}", notice.severity(), notice.message()));
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => break,
            }
        }
    });
}

/// What went wrong while connecting, in words a user can act on (D11): the driver's own text names
/// only the kind ("invalid configuration"), the cause sits in its source.
fn connect_message(e: &tokio_postgres::Error) -> String {
    let cause = std::error::Error::source(e).map(ToString::to_string);
    match cause.as_deref() {
        Some("password missing") => {
            "the server asks for a password and none was given; type it in the Password field"
                .into()
        }
        Some(c) if !e.to_string().contains(c) => format!("{e}: {c}"),
        _ => e.to_string(),
    }
}

impl PgSession {
    /// Opens a session for `profile` with already resolved `credentials`.
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        Self::connect_via(profile, credentials, None).await
    }

    /// [`PgSession::connect`] through an SSH tunnel's local address `via` (`SPEC-0108` v4
    /// behaviour 8): the socket goes to `via`, TLS still checks the profile's host name.
    pub async fn connect_via(
        profile: &ConnectionProfile,
        credentials: &Credentials,
        via: Via,
    ) -> Result<Self, SessionError> {
        let password = prepare(profile, credentials, Dialect::PostgreSql)?;

        let host = profile.host.clone().unwrap_or_else(|| "localhost".into());
        let port = profile.effective_port().unwrap_or(5432);
        let limit = profile.options.connect_timeout();
        let mut config = tokio_postgres::Config::new();
        config
            .host(&host)
            .connect_timeout(limit)
            .application_name("IDECraft");
        // Host and port are lists in tokio-postgres: set each once.
        match via {
            Some(local) => config.hostaddr(local.ip()).port(local.port()),
            None => config.port(port),
        };
        if let Some(database) = &profile.database {
            config.dbname(database);
        }
        if let Some(user) = &profile.user {
            config.user(user);
        }
        if let Some(password) = password {
            config.password(password);
        }
        // SPEC-0108 v4 behaviour 10: read-only and the statement time limit as session settings.
        let mut options = Vec::new();
        if profile.read_only {
            options.push("-c default_transaction_read_only=on".to_owned());
        }
        if let Some(secs) = profile.options.query_timeout_secs {
            options.push(format!("-c statement_timeout={}", u64::from(secs) * 1000));
        }
        if !options.is_empty() {
            config.options(options.join(" "));
        }
        if let Some(secs) = profile.options.keep_alive_secs {
            config
                .keepalives(true)
                .keepalives_idle(std::time::Duration::from_secs(u64::from(secs)));
        }
        let address = match via {
            Some(local) => format!("{host}:{port} via SSH tunnel {local}"),
            None => format!("{host}:{port}"),
        };

        let timeout = || format!("no answer within {} s", limit.as_secs());
        let mode = profile.ssl;
        // Connection tasks end when the client is dropped or the server closes the connection.
        let notices = Notices::default();
        let result: Result<Client, String> = if mode == SslMode::Disable {
            config.ssl_mode(tokio_postgres::config::SslMode::Disable);
            match tokio::time::timeout(limit, config.connect(NoTls)).await {
                Ok(Ok((client, connection))) => {
                    drive(connection, notices.clone());
                    Ok(client)
                }
                Ok(Err(e)) => Err(connect_message(&e)),
                Err(_) => Err(timeout()),
            }
        } else {
            config.ssl_mode(if mode == SslMode::Prefer {
                tokio_postgres::config::SslMode::Prefer
            } else {
                tokio_postgres::config::SslMode::Require
            });
            let connector =
                profile_tls_connector(profile).map_err(|message| SessionError::Tls {
                    profile: profile.name.clone(),
                    mode: mode_name(mode),
                    message,
                })?;
            let tls = postgres_native_tls::MakeTlsConnector::new(connector);
            match tokio::time::timeout(limit, config.connect(tls)).await {
                Ok(Ok((client, connection))) => {
                    drive(connection, notices.clone());
                    Ok(client)
                }
                Ok(Err(e)) if mode != SslMode::Prefer => {
                    return Err(SessionError::Tls {
                        profile: profile.name.clone(),
                        mode: mode_name(mode),
                        message: format!("{address}: {}", connect_message(&e)),
                    });
                }
                Ok(Err(e)) => Err(connect_message(&e)),
                Err(_) => Err(timeout()),
            }
        };
        let client = result.map_err(|message| SessionError::Connect {
            profile: profile.name.clone(),
            address,
            message,
        })?;
        Ok(Self { client, notices })
    }

    /// The key to cancel the running statement from elsewhere (`SPEC-0093` v3 behaviour 12).
    pub fn cancel_token(&self) -> tokio_postgres::CancelToken {
        self.client.cancel_token()
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let statements = split_statements(script, Dialect::PostgreSql);
        let mut run = ScriptRun::default();
        for (index, statement) in statements.iter().enumerate() {
            if let Ok(mut kept) = self.notices.lock() {
                kept.clear();
            }
            let outcome = self.execute(&statement.text).await;
            let failed = run.push(script, statement, outcome);
            // What the server said while the statement ran (`SPEC-0093` v8 behaviour 6a).
            if let (Some(last), Ok(mut kept)) = (run.outcomes.last_mut(), self.notices.lock()) {
                last.notices = std::mem::take(&mut *kept);
            }
            if failed && on_error == OnError::Stop {
                run.not_run = statements.len() - index - 1;
                break;
            }
        }
        run
    }

    async fn execute(&self, sql: &str) -> Outcome {
        let messages = match self.client.simple_query(sql).await {
            Ok(messages) => messages,
            Err(e) => return Outcome::Error(to_db_error(&e)),
        };
        let mut columns: Option<Vec<String>> = None;
        let mut rows = Vec::new();
        let mut affected = 0;
        for message in messages {
            match message {
                SimpleQueryMessage::RowDescription(described) => {
                    columns = Some(described.iter().map(|c| c.name().to_owned()).collect());
                }
                SimpleQueryMessage::Row(row) => {
                    rows.push(
                        (0..row.len())
                            .map(|i| row.get(i).map(str::to_owned))
                            .collect(),
                    );
                }
                SimpleQueryMessage::CommandComplete(count) => affected = count,
                _ => {}
            }
        }
        match columns {
            Some(columns) => Outcome::ResultSet { columns, rows },
            None => Outcome::UpdateCount(affected),
        }
    }
}

fn to_db_error(error: &tokio_postgres::Error) -> DbError {
    match error.as_db_error() {
        Some(db) => DbError {
            message: db.message().to_owned(),
            sqlstate: Some(db.code().code().to_owned()),
            position: match db.position() {
                Some(ErrorPosition::Original(pos)) => Some(*pos as usize),
                _ => None,
            },
        },
        None => DbError {
            message: error.to_string(),
            sqlstate: None,
            position: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretRef;

    fn profile(url: &str) -> ConnectionProfile {
        ConnectionProfile::from_url("ref-pg", url).unwrap()
    }

    #[tokio::test]
    async fn required_tls_failure_names_the_mode() {
        let p = profile("postgresql://u@127.0.0.1:1/db?sslmode=require");
        let err = PgSession::connect(&p, &Credentials::NotNeeded)
            .await
            .err()
            .unwrap();
        assert!(
            matches!(
                err,
                SessionError::Tls {
                    mode: "require",
                    ..
                }
            ),
            "{err}"
        );
    }

    #[tokio::test]
    async fn refuses_missing_credentials_naming_the_entry() {
        let p = profile("postgresql://u@127.0.0.1:1/db");
        let missing = Credentials::Missing {
            profile: "ref-pg".into(),
            reference: SecretRef {
                service: "idecraft".into(),
                account: "ref-pg".into(),
            },
        };
        let err = PgSession::connect(&p, &missing).await.err().unwrap();
        assert_eq!(
            err.to_string(),
            "connection `ref-pg`: password missing in keychain entry idecraft/ref-pg"
        );
    }

    /// D11: a server that wants a password the form left empty says so, not "invalid configuration".
    #[tokio::test]
    async fn missing_password_is_named() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut len = [0u8; 4];
            socket.read_exact(&mut len).await.unwrap();
            let mut rest = vec![0u8; u32::from_be_bytes(len) as usize - 4];
            socket.read_exact(&mut rest).await.unwrap();
            // AuthenticationCleartextPassword.
            socket
                .write_all(&[b'R', 0, 0, 0, 8, 0, 0, 0, 3])
                .await
                .unwrap();
            let _ = socket.read(&mut [0u8; 64]).await;
        });
        let p = profile(&format!(
            "postgresql://u@127.0.0.1:{port}/db?sslmode=disable"
        ));
        let err = PgSession::connect(&p, &Credentials::NotNeeded)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("asks for a password"), "{err}");
        assert!(!err.contains("invalid configuration"), "{err}");
    }

    #[tokio::test]
    async fn unreachable_server_names_profile_and_address() {
        let p = profile("postgresql://u@127.0.0.1:1/db");
        let err = PgSession::connect(&p, &Credentials::NotNeeded)
            .await
            .err()
            .unwrap();
        let text = err.to_string();
        assert!(
            text.starts_with("connection `ref-pg` (127.0.0.1:1):"),
            "{text}"
        );
    }
}
