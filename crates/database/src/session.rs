//! What all database sessions share (`SPEC-0009`, `SPEC-0010`): the error type
//! and the checks that run before any network access.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::profile::{TlsFiles, expand_home, is_pkcs12};
use crate::tunnel::Tunnel;
use crate::{ConnectionProfile, Credentials, Dialect, SslMode};

/// Name of an SSL mode as written in profiles.
pub(crate) fn mode_name(mode: SslMode) -> &'static str {
    match mode {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        SslMode::VerifyCa => "verify-ca",
        SslMode::VerifyFull => "verify-full",
    }
}

/// Whether `host` reaches its server through this machine's loopback (`SPEC-0016` v3 behaviour 3):
/// `localhost` in any case, an address in 127.0.0.0/8, `::1` (also `[::1]`), or a PostgreSQL
/// socket folder. No host means the drivers' default, `localhost`. Anything else — also
/// `0.0.0.0`, a name that merely starts with `localhost` or an IPv4 address written as IPv6 —
/// counts as another machine.
pub fn is_loopback(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return true;
    };
    if host.starts_with('/') || host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// What a profile's SSL mode asks of the connection on its host (`SPEC-0016` v3 behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsPlan {
    /// No TLS (`disable`).
    Off,
    /// TLS when the server offers it, and one retry without it after TLS itself failed: `prefer`
    /// for a server on the loopback.
    Optional,
    /// TLS or no connection: `require`, `verify-ca`, `verify-full`, and `prefer` for a server off
    /// the loopback.
    Required,
}

/// The [`TlsPlan`] of `profile`: its mode on its host. Through an SSH tunnel the profile's host is
/// where the database is as the SSH server sees it, so that host decides, not the tunnel's end.
pub fn tls_plan(profile: &ConnectionProfile) -> TlsPlan {
    match profile.ssl {
        SslMode::Disable => TlsPlan::Off,
        SslMode::Prefer if is_loopback(profile.host.as_deref()) => TlsPlan::Optional,
        _ => TlsPlan::Required,
    }
}

/// How to give a server of `dialect` TLS (`SPEC-0016` v3 behaviour 13).
fn server_tls_hint(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::PostgreSql => {
            "set `ssl = on` with a certificate and its key (`ssl_cert_file`, `ssl_key_file`) in \
             postgresql.conf"
        }
        Dialect::MySql | Dialect::MariaDb => {
            "give the server a certificate and its key (`ssl_cert`, `ssl_key` in its option file; \
             MySQL 8 and MariaDB 11.4 or newer make their own at the first start)"
        }
        Dialect::SqlServer => {
            "install a certificate for the instance (`mssql-conf set network.tlscert` and \
             `network.tlskey` on Linux, the SQL Server Configuration Manager on Windows)"
        }
        Dialect::Oracle => {
            "set up a TCPS listener with a wallet and put its port in the profile, if the server \
             has none"
        }
        Dialect::Sqlite | Dialect::DuckDb => "",
    }
}

/// The error of a `prefer` connection to a server off the loopback that got no TLS (`SPEC-0016` v3
/// behaviour 13): the address, the reason, that the server needs TLS and how to give it TLS —
/// never a weaker mode.
pub(crate) fn prefer_needs_tls(
    profile: &ConnectionProfile,
    address: &str,
    reason: &str,
) -> SessionError {
    SessionError::Tls {
        profile: profile.name.clone(),
        mode: mode_name(SslMode::Prefer),
        message: format!(
            "{address}: {reason}. The server is not on this machine, so `prefer` connects only \
             with TLS, as `require` does. To give the server TLS: {}",
            server_tls_hint(profile.dialect)
        ),
    }
}

/// The certificate checks of `mode`, and never below TLS 1.2 (SPEC-0016 v2 behaviour 9).
fn checks(builder: &mut native_tls::TlsConnectorBuilder, mode: SslMode) {
    builder.min_protocol_version(Some(native_tls::Protocol::Tlsv12));
    match mode {
        SslMode::VerifyFull => {}
        SslMode::VerifyCa => {
            builder.danger_accept_invalid_hostnames(true);
        }
        _ => {
            builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }
    }
}

/// Reads a private key or PKCS#12 file after checking that only its owner can read it
/// (`SPEC-0016` v2 behaviour 11), the way libpq refuses such keys; encrypted keys are refused
/// because IDECraft never asks for a passphrase.
pub(crate) fn read_private(path: &Path) -> Result<Vec<u8>, String> {
    let shown = path.display();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?;
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(format!(
                "{shown} can be read by other users (mode {mode:04o}); restrict it with chmod 600"
            ));
        }
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    if String::from_utf8_lossy(&bytes).contains("ENCRYPTED") {
        return Err(format!(
            "{shown} is protected by a passphrase, which IDECraft does not ask for; use a key \
             without passphrase kept with mode 600"
        ));
    }
    Ok(bytes)
}

/// The certificates of a PEM file.
pub(crate) fn pem_certificates(path: &Path) -> Result<Vec<native_tls::Certificate>, String> {
    let shown = path.display();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    let certs: Vec<_> = text
        .split_inclusive("-----END CERTIFICATE-----")
        .filter(|block| block.contains("-----BEGIN CERTIFICATE-----"))
        .map(|block| {
            native_tls::Certificate::from_pem(block.trim().as_bytes())
                .map_err(|e| format!("{shown}: {e}"))
        })
        .collect::<Result<_, _>>()?;
    if certs.is_empty() {
        return Err(format!("{shown}: no PEM certificate found"));
    }
    Ok(certs)
}

/// The client identity of `files`: a PKCS#12 bundle without password, or a PEM certificate with
/// its PKCS#8 key.
pub(crate) fn client_identity(files: &TlsFiles) -> Result<Option<native_tls::Identity>, String> {
    let Some(cert) = files.cert_file.as_deref().map(expand_home) else {
        return Ok(None);
    };
    if is_pkcs12(&cert) {
        let der = read_private(&cert)?;
        return native_tls::Identity::from_pkcs12(&der, "")
            .map(Some)
            .map_err(|e| format!("{}: {e}", cert.display()));
    }
    let key = files
        .key_file
        .as_deref()
        .map(expand_home)
        .ok_or_else(|| "a PEM client certificate needs its key file".to_owned())?;
    let key_pem = read_private(&key)?;
    let cert_pem = std::fs::read(&cert).map_err(|e| format!("{}: {e}", cert.display()))?;
    native_tls::Identity::from_pkcs8(&cert_pem, &key_pem)
        .map(Some)
        .map_err(|e| format!("{} / {}: {e}", cert.display(), key.display()))
}

/// TLS settings for `profile` (SPEC-0016 v2): the mode's checks, its CA file instead of the
/// system's trusted roots, its client certificate.
pub(crate) fn profile_tls_connector(
    profile: &ConnectionProfile,
) -> Result<native_tls::TlsConnector, String> {
    let mut builder = native_tls::TlsConnector::builder();
    checks(&mut builder, profile.ssl);
    if let Some(ca) = profile.tls.ca_file.as_deref().map(expand_home) {
        builder.disable_built_in_roots(true);
        for cert in pem_certificates(&ca)? {
            builder.add_root_certificate(cert);
        }
    }
    if let Some(identity) = client_identity(&profile.tls)? {
        builder.identity(identity);
    }
    builder.build().map_err(|e| e.to_string())
}

/// Where a driver connects when an SSH tunnel is in between (`SPEC-0108` v4 behaviour 8): the
/// tunnel's local address; TLS still checks the profile's host name.
pub(crate) type Via = Option<std::net::SocketAddr>;

/// Time allowed for establishing a connection.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a session could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The profile names a database system that is not in the catalogue.
    #[error("unknown database system `{0}`; import its definition")]
    UnknownSystem(String),
    /// The profile belongs to another database system.
    #[error("connection `{0}` is not a {1:?} connection")]
    WrongDialect(String, Dialect),
    /// The system connects only through its WebAssembly plugin (`SPEC-0314`).
    #[error(
        "{0} connects through the WebAssembly plugin `{1}`; use Test Connection or the MCP tool test_connection"
    )]
    ThroughPlugin(String, String),
    /// The profile's password is not in the keychain.
    #[error("connection `{profile}`: password missing in keychain entry {service}/{account}")]
    MissingCredentials {
        /// Profile name.
        profile: String,
        /// Keychain service.
        service: String,
        /// Keychain account.
        account: String,
    },
    /// TLS could not be set up as the profile's SSL mode requires.
    #[error("connection `{profile}`: SSL mode `{mode}`: {message}")]
    Tls {
        /// Profile name.
        profile: String,
        /// SSL mode that could not be met.
        mode: &'static str,
        /// Why.
        message: String,
    },
    /// The SSH tunnel could not be opened (`SPEC-0108` v4 behaviour 8).
    #[error("connection `{profile}`: SSH tunnel: {message}")]
    Tunnel {
        /// Profile name.
        profile: String,
        /// Why, with what to do.
        message: String,
    },
    /// The connection's startup script failed (`SPEC-0108` v4 behaviour 10).
    #[error("connection `{profile}`: startup script: {message}")]
    Startup {
        /// Profile name.
        profile: String,
        /// The database's message.
        message: String,
    },
    /// Connecting failed.
    #[error("connection `{profile}` ({address}): {message}")]
    Connect {
        /// Profile name.
        profile: String,
        /// `host:port` that was tried.
        address: String,
        /// Driver message.
        message: String,
    },
}

/// Checks dialect, credentials and TLS before connecting; returns the password
/// to use, if any.
pub(crate) fn prepare(
    profile: &ConnectionProfile,
    credentials: &Credentials,
    dialect: Dialect,
) -> Result<Option<String>, SessionError> {
    if profile.dialect != dialect {
        return Err(SessionError::WrongDialect(profile.name.clone(), dialect));
    }
    let password = match credentials {
        Credentials::NotNeeded => None,
        Credentials::Available(secret) => Some(secret.expose().to_owned()),
        Credentials::Missing { profile, reference } => {
            return Err(SessionError::MissingCredentials {
                profile: profile.clone(),
                service: reference.service.clone(),
                account: reference.account.clone(),
            });
        }
    };
    Ok(password)
}

/// A session with any supported database, chosen by the profile's dialect.
pub struct Session {
    /// The family's driver session.
    pub driver: Driver,
    /// The connector of a profile with a `system` (`SPEC-0300` behaviour 9).
    system: Option<Arc<crate::Connector>>,
    /// The SSH tunnel the driver connects through; dropped after the driver (`SPEC-0108` v4).
    tunnel: Option<Tunnel>,
    /// How to cancel a running statement from another task (`SPEC-0093` v3 behaviour 12).
    cancel: Option<CancelHandle>,
    /// Set for a session opened for an agent: the workspace its file access is limited to
    /// (`SPEC-0012` v3 behaviour 15).
    agent_root: Option<std::path::PathBuf>,
}

impl std::ops::Deref for Session {
    type Target = Driver;
    fn deref(&self) -> &Driver {
        &self.driver
    }
}

impl From<Driver> for Session {
    fn from(driver: Driver) -> Self {
        Self {
            driver,
            system: None,
            tunnel: None,
            cancel: None,
            agent_root: None,
        }
    }
}

/// The driver session of a protocol family.
pub enum Driver {
    /// PostgreSQL (`SPEC-0009`).
    Postgres(crate::PgSession),
    /// MySQL (`SPEC-0010`).
    MySql(crate::MySqlSession),
    /// SQLite (`SPEC-0011`).
    Sqlite(crate::SqliteSession),
    /// SQL Server (`SPEC-0116`).
    SqlServer(crate::SqlServerSession),
    /// Oracle (`SPEC-0142`).
    Oracle(crate::OracleSession),
    /// DuckDB (`SPEC-0304`).
    DuckDb(crate::DuckDbSession),
}

impl Session {
    /// Opens a session with the driver for the profile's dialect; a profile with a `system` must
    /// name a known, connectable connector of that family (`SPEC-0300` behaviour 1).
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        Self::connect_with(profile, credentials, &crate::tunnel::test_extra_args()).await
    }

    /// [`Session::connect`] for a call of an agent in the workspace `root` (`SPEC-0012` v3
    /// behaviour 15): what the session reaches through a file database stays inside `root`. A
    /// DuckDB session closes its access to other files before anything else runs; on SQLite
    /// every statement, the startup script included, is checked with
    /// [`crate::refused_file_access`]. A startup script that reaches a file outside keeps the
    /// session from opening.
    pub async fn connect_for_agent(
        profile: &ConnectionProfile,
        credentials: &Credentials,
        root: &std::path::Path,
    ) -> Result<Self, SessionError> {
        Self::connect_as(
            profile,
            credentials,
            &crate::tunnel::test_extra_args(),
            Some(root),
        )
        .await
    }

    /// [`Session::connect`] with extra `ssh` arguments for the profile's tunnel (tests pass
    /// `-F <config>` of their own SSH server).
    pub async fn connect_with(
        profile: &ConnectionProfile,
        credentials: &Credentials,
        ssh_extra: &[String],
    ) -> Result<Self, SessionError> {
        Self::connect_as(profile, credentials, ssh_extra, None).await
    }

    /// Connects; with `agent_root` as [`Session::connect_for_agent`].
    async fn connect_as(
        profile: &ConnectionProfile,
        credentials: &Credentials,
        ssh_extra: &[String],
        agent_root: Option<&std::path::Path>,
    ) -> Result<Self, SessionError> {
        let system = match &profile.system {
            None => None,
            Some(id) => {
                let catalogue = crate::connector::catalogue();
                let connector = catalogue
                    .get(id)
                    .ok_or_else(|| SessionError::UnknownSystem(id.clone()))?;
                if let Some(module) = connector.plugin_module() {
                    return Err(SessionError::ThroughPlugin(
                        connector.name.clone(),
                        module.to_owned(),
                    ));
                }
                if connector.family.dialect() != Some(profile.dialect) {
                    return Err(SessionError::WrongDialect(
                        profile.name.clone(),
                        profile.dialect,
                    ));
                }
                Some(Arc::new(connector.clone()))
            }
        };
        let tunnel = match &profile.ssh {
            None => None,
            Some(ssh) => {
                let host = profile.host.clone().unwrap_or_else(|| "localhost".into());
                let port = profile.effective_port().unwrap_or(0);
                let limit = profile.options.connect_timeout() + Duration::from_secs(5);
                let ssh = ssh.clone();
                let failed = |message: String| SessionError::Tunnel {
                    profile: profile.name.clone(),
                    message,
                };
                let extra = ssh_extra.to_vec();
                let opened = tokio::task::spawn_blocking(move || {
                    Tunnel::open_with(
                        &ssh,
                        &host,
                        port,
                        limit,
                        &extra,
                        idecraft_remote::SSH_PROGRAM,
                    )
                })
                .await
                .map_err(|e| failed(e.to_string()))?;
                Some(opened.map_err(|e| failed(Tunnel::explain(&e)))?)
            }
        };
        let via = tunnel.as_ref().map(Tunnel::local);
        let driver = match profile.dialect {
            Dialect::PostgreSql => {
                Driver::Postgres(crate::PgSession::connect_via(profile, credentials, via).await?)
            }
            Dialect::MySql | Dialect::MariaDb => {
                Driver::MySql(crate::MySqlSession::connect_via(profile, credentials, via).await?)
            }
            Dialect::SqlServer => {
                Driver::SqlServer(crate::SqlServerSession::connect(profile, credentials).await?)
            }
            Dialect::Sqlite => {
                Driver::Sqlite(crate::SqliteSession::connect(profile, credentials).await?)
            }
            Dialect::Oracle => {
                Driver::Oracle(crate::OracleSession::connect(profile, credentials).await?)
            }
            Dialect::DuckDb => {
                Driver::DuckDb(crate::DuckDbSession::connect(profile, credentials).await?)
            }
        };
        let cancel = CancelHandle::of(&driver, profile, credentials, via);
        let session = Self {
            driver,
            system,
            tunnel,
            cancel,
            agent_root: agent_root.map(std::path::Path::to_path_buf),
        };
        if agent_root.is_some()
            && let Driver::DuckDb(duck) = &session.driver
        {
            duck.close_external_access()
                .await
                .map_err(|message| SessionError::Connect {
                    profile: profile.name.clone(),
                    address: profile.file_path().display().to_string(),
                    message,
                })?;
        }
        // SPEC-0108 v4 behaviour 10: the startup script runs before anything else.
        if let Some(script) = profile
            .options
            .startup_script
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .filter(|_| profile.options.startup_allowed)
        {
            let run = session.run_script(script, crate::OnError::Stop).await;
            if let Some(e) = run.outcomes.iter().find_map(|o| match &o.outcome {
                crate::Outcome::Error(e) => Some(e.to_string()),
                _ => None,
            }) {
                return Err(SessionError::Startup {
                    profile: profile.name.clone(),
                    message: e,
                });
            }
        }
        Ok(session)
    }

    /// The local end of the SSH tunnel this session goes through, if any (always `127.0.0.1`).
    pub fn tunnel_address(&self) -> Option<std::net::SocketAddr> {
        self.tunnel.as_ref().map(Tunnel::local)
    }

    /// A handle that cancels the statement this session runs, from another task or thread
    /// (`SPEC-0093` v3 behaviour 12); `None` where the database offers no way.
    pub fn cancel_handle(&self) -> Option<CancelHandle> {
        self.cancel.clone()
    }

    /// The connector of this session's profile, `None` for a family's own system.
    pub fn system(&self) -> Option<&crate::Connector> {
        self.system.as_deref()
    }
}

impl Session {
    /// Runs `script` statement by statement (SPEC-0007 outcomes). In a session opened for an
    /// agent nothing of a script runs when one of its statements reaches a file outside the
    /// workspace (`SPEC-0012` v3 behaviour 15): that statement is answered with the reason, the
    /// others are counted as not run.
    pub async fn run_script(&self, script: &str, on_error: crate::OnError) -> crate::ScriptRun {
        if let Some(root) = &self.agent_root {
            let dialect = self.driver.dialect();
            let statements = crate::split_statements(script, dialect);
            let refused = statements.iter().find_map(|s| {
                crate::refused_file_access(&s.text, dialect, root).map(|why| (s, why))
            });
            if let Some((statement, why)) = refused {
                let mut run = crate::ScriptRun::default();
                run.push(
                    script,
                    statement,
                    crate::Outcome::Error(crate::DbError {
                        message: format!(
                            "refused for agents, nothing of the script was run: {why}"
                        ),
                        sqlstate: None,
                        position: None,
                    }),
                );
                run.not_run = statements.len() - 1;
                return run;
            }
        }
        self.driver.run_script(script, on_error).await
    }
}

impl Driver {
    /// The database system of this session.
    pub fn dialect(&self) -> Dialect {
        match self {
            Self::Postgres(_) => Dialect::PostgreSql,
            Self::MySql(s) => s.dialect(),
            Self::Sqlite(_) => Dialect::Sqlite,
            Self::SqlServer(_) => Dialect::SqlServer,
            Self::Oracle(_) => Dialect::Oracle,
            Self::DuckDb(_) => Dialect::DuckDb,
        }
    }

    /// Whether the connection is encrypted, as the server reports it; `None`
    /// for SQLite (no network) or when the server does not tell. SQL Server tells only a login
    /// that may see the server's connections; for any other the driver's agreement with the server
    /// answers (`SPEC-0116` v5 behaviour 13).
    pub async fn encrypted(&self) -> Option<bool> {
        let reported = self.reported_encryption().await;
        match self {
            Self::SqlServer(s) => reported.or(Some(s.encrypted())),
            _ => reported,
        }
    }

    /// What the server says about the encryption of this connection, if it says anything.
    async fn reported_encryption(&self) -> Option<bool> {
        let probe = match self {
            Self::Postgres(_) => "select ssl from pg_stat_ssl where pid = pg_backend_pid()",
            Self::MySql(_) => "show session status like 'Ssl_cipher'",
            Self::SqlServer(_) => {
                "select encrypt_option from sys.dm_exec_connections where session_id = @@spid"
            }
            Self::Oracle(_) => {
                "select case when sys_context('USERENV', 'NETWORK_PROTOCOL') = 'tcps' then 'TRUE' \
                 else 'FALSE' end from dual"
            }
            Self::Sqlite(_) | Self::DuckDb(_) => return None,
        };
        let run = self.run_script(probe, crate::OnError::Stop).await;
        match run.outcomes.into_iter().next().map(|o| o.outcome) {
            Some(crate::Outcome::ResultSet { rows, .. }) => {
                let row = rows.into_iter().next()?;
                Some(match self {
                    Self::Postgres(_) => row.first().cloned().flatten().as_deref() == Some("t"),
                    Self::SqlServer(_) | Self::Oracle(_) => {
                        row.first().cloned().flatten().as_deref() == Some("TRUE")
                    }
                    _ => row.get(1).cloned().flatten().is_some_and(|c| !c.is_empty()),
                })
            }
            _ => None,
        }
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: crate::OnError) -> crate::ScriptRun {
        match self {
            Self::Postgres(s) => s.run_script(script, on_error).await,
            Self::MySql(s) => s.run_script(script, on_error).await,
            Self::Sqlite(s) => s.run_script(script, on_error).await,
            Self::SqlServer(s) => s.run_script(script, on_error).await,
            Self::Oracle(s) => s.run_script(script, on_error).await,
            Self::DuckDb(s) => s.run_script(script, on_error).await,
        }
    }
}

/// Cancels the statement a session runs (`SPEC-0093` v3 behaviour 12): PostgreSQL's cancel
/// request, MySQL's and MariaDB's `KILL QUERY` on a second connection, SQLite's interrupt.
#[derive(Clone)]
pub enum CancelHandle {
    /// PostgreSQL: the server's cancel key, sent with the profile's TLS settings.
    Postgres(tokio_postgres::CancelToken, Box<ConnectionProfile>),
    /// MySQL, MariaDB: the connection id to `KILL QUERY`, and how to open a second connection.
    MySql {
        /// Connection id of the running statement's session.
        id: u32,
        /// The profile, to connect again.
        profile: Box<ConnectionProfile>,
        /// The credentials the session was opened with.
        credentials: Credentials,
        /// The tunnel's local address, when there is one.
        via: Via,
    },
    /// SQLite: interrupts the running statement.
    Sqlite(Arc<rusqlite::InterruptHandle>),
}

impl std::fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Postgres(..) => "CancelHandle::Postgres",
            Self::MySql { .. } => "CancelHandle::MySql",
            Self::Sqlite(_) => "CancelHandle::Sqlite",
        })
    }
}

impl CancelHandle {
    fn of(
        driver: &Driver,
        profile: &ConnectionProfile,
        credentials: &Credentials,
        via: Via,
    ) -> Option<Self> {
        match driver {
            Driver::Postgres(s) => {
                Some(Self::Postgres(s.cancel_token(), Box::new(profile.clone())))
            }
            Driver::MySql(s) => Some(Self::MySql {
                id: s.connection_id(),
                profile: Box::new(profile.clone()),
                credentials: credentials.clone(),
                via,
            }),
            Driver::Sqlite(s) => Some(Self::Sqlite(s.interrupt_handle())),
            _ => None,
        }
    }

    /// Asks the database to end the running statement; the statement then fails (PostgreSQL,
    /// SQLite) or ends early (MySQL).
    pub async fn cancel(&self) -> Result<(), String> {
        match self {
            Self::Postgres(token, profile) => {
                if profile.ssl == SslMode::Disable {
                    token
                        .cancel_query(tokio_postgres::NoTls)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    let connector = profile_tls_connector(profile)?;
                    token
                        .cancel_query(postgres_native_tls::MakeTlsConnector::new(connector))
                        .await
                        .map_err(|e| e.to_string())
                }
            }
            Self::MySql {
                id,
                profile,
                credentials,
                via,
            } => {
                let mut quiet = (**profile).clone();
                quiet.options.startup_script = None;
                let second = crate::MySqlSession::connect_via(&quiet, credentials, *via)
                    .await
                    .map_err(|e| e.to_string())?;
                let run = second
                    .run_script(&format!("KILL QUERY {id}"), crate::OnError::Stop)
                    .await;
                match run.outcomes.into_iter().next().map(|o| o.outcome) {
                    Some(crate::Outcome::Error(e)) => Err(e.to_string()),
                    _ => Ok(()),
                }
            }
            Self::Sqlite(handle) => {
                handle.interrupt();
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("idecraft-tls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// SPEC-0016 v2 AK-7: a key file others can read, an encrypted key, a CA file without
    /// certificates are refused with the reason and the path, never the content.
    #[cfg(unix)]
    #[test]
    fn private_files_are_checked() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("private");
        let key = d.join("client.key");
        std::fs::write(&key, "-----BEGIN PRIVATE KEY-----\nsecretbytes\n").unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        let err = read_private(&key).unwrap_err();
        assert!(
            err.contains("mode 0644") && err.contains("chmod 600"),
            "{err}"
        );
        assert!(!err.contains("secretbytes"));
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_private(&key).is_ok());
        std::fs::write(&key, "-----BEGIN ENCRYPTED PRIVATE KEY-----\nx\n").unwrap();
        assert!(read_private(&key).unwrap_err().contains("passphrase"));
        let ca = d.join("ca.pem");
        std::fs::write(&ca, "not a certificate").unwrap();
        assert!(
            pem_certificates(&ca)
                .err()
                .unwrap()
                .contains("no PEM certificate")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// SPEC-0016 v3 AK-10: `prefer` may end without TLS only on the loopback; every other mode
    /// means the same on every host.
    #[test]
    fn prefer_needs_tls_off_the_loopback() {
        let plan = |host: Option<&str>, mode: SslMode| {
            let mut p = ConnectionProfile::from_url("p", "postgresql://u@h/db").unwrap();
            p.host = host.map(str::to_owned);
            p.ssl = mode;
            tls_plan(&p)
        };
        let loopback = [
            Some("localhost"),
            Some("LocalHost"),
            Some("127.0.0.1"),
            Some("127.1.2.3"),
            Some("::1"),
            Some("[::1]"),
            Some("/var/run/postgresql"),
            None,
        ];
        let elsewhere = [
            "db.example",
            "10.0.0.5",
            "192.168.1.20",
            "localhost.example",
            "::ffff:10.0.0.5",
            "0.0.0.0",
            "",
        ];
        for host in loopback {
            assert!(is_loopback(host), "{host:?}");
            assert_eq!(plan(host, SslMode::Prefer), TlsPlan::Optional, "{host:?}");
        }
        for host in elsewhere {
            assert!(!is_loopback(Some(host)), "{host}");
            assert_eq!(
                plan(Some(host), SslMode::Prefer),
                TlsPlan::Required,
                "{host}"
            );
        }
        for host in loopback.into_iter().chain(elsewhere.map(Some)) {
            assert_eq!(plan(host, SslMode::Disable), TlsPlan::Off);
            for mode in [SslMode::Require, SslMode::VerifyCa, SslMode::VerifyFull] {
                assert_eq!(plan(host, mode), TlsPlan::Required, "{host:?} {mode:?}");
            }
        }
    }

    /// SPEC-0016 v3 AK-11: the error names `prefer`, the reason and the server's TLS setting of
    /// each system, and proposes no weaker mode.
    #[test]
    fn the_hint_says_how_to_give_the_server_tls() {
        for (url, setting) in [
            ("postgresql://u@db.example/d", "ssl = on"),
            ("mysql://u@db.example/d", "ssl_cert"),
            ("mariadb://u@db.example/d", "ssl_key"),
            ("sqlserver://u@db.example/d", "network.tlscert"),
            ("oracle://u@db.example:1521/FREEPDB1", "TCPS listener"),
        ] {
            let p = ConnectionProfile::from_url("remote", url).unwrap();
            let e =
                prefer_needs_tls(&p, "db.example:1", "the server does not offer TLS").to_string();
            assert!(e.contains("SSL mode `prefer`"), "{e}");
            assert!(
                e.contains("db.example:1: the server does not offer TLS"),
                "{e}"
            );
            assert!(
                e.contains("not on this machine") && e.contains(setting),
                "{e}"
            );
            assert!(!e.contains("disable"), "{e}");
        }
    }

    /// SPEC-0016 v2 AK-6: a CA file replaces the system's roots and is parsed.
    #[test]
    fn ca_files_are_read() {
        let Some(ca) = test_ca() else {
            return;
        };
        assert_eq!(pem_certificates(&ca).unwrap().len(), 1);
        let mut p =
            ConnectionProfile::from_url("p", "postgresql://u@h/db?sslmode=verify-full").unwrap();
        p.tls.ca_file = Some(ca);
        assert!(profile_tls_connector(&p).is_ok());
    }

    /// A self-signed CA made with the system's `openssl` in a temporary folder.
    fn test_ca() -> Option<std::path::PathBuf> {
        let d = dir("ca");
        let ok = std::process::Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=idecraft-test-ca",
                "-keyout",
            ])
            .arg(d.join("ca.key"))
            .arg("-out")
            .arg(d.join("ca.pem"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        ok.then(|| d.join("ca.pem"))
    }
}
