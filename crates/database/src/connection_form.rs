//! Adding and editing connections in the window (`SPEC-0108`): the text of a form becomes a checked
//! profile, the password goes to the system keychain and never into the profile file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::profile::{
    ConnectionColor, ConnectionOptions, ConnectionProfile, ProfileError, SecretRef, SshTunnel,
    SslMode, TlsFiles, load_profiles, save_profiles, validate_all,
};
use crate::{
    Credentials, Dialect, Driver, OnError, Outcome, Secret, SecretStore, Session,
    resolve_credentials,
};

/// Keychain service of passwords saved from the connection dialog.
pub const KEYCHAIN_SERVICE: &str = "idecraft";

/// The fields of the connection dialog as text (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionForm {
    /// Display name.
    pub name: String,
    /// Database system: the protocol family.
    pub dialect: Dialect,
    /// Connector id on top of the family (`SPEC-0300`); `None` is the family's own system.
    pub system: Option<String>,
    /// Server host (not for SQLite).
    pub host: String,
    /// Port as typed; empty means the dialect's default.
    pub port: String,
    /// Database name.
    pub database: String,
    /// User name.
    pub user: String,
    /// Database file (SQLite only).
    pub path: String,
    /// Transport security.
    pub ssl: SslMode,
    /// Refuse statements that change data or schema.
    pub read_only: bool,
    /// SSH tunnel, TLS files, options, colour and comment (`SPEC-0108` v4 behaviours 8–10).
    pub extras: ConnectionExtras,
}

/// What the dialog's SSH/SSL, Options and General parts add to a profile (`SPEC-0108` v4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionExtras {
    /// SSH tunnel to the server.
    pub ssh: Option<SshTunnel>,
    /// Certificate files for TLS.
    pub tls: TlsFiles,
    /// Further options.
    pub options: ConnectionOptions,
    /// Colour that marks the connection.
    pub color: Option<ConnectionColor>,
    /// A note about the connection.
    pub comment: String,
}

impl ConnectionForm {
    /// An empty form for `connector` (`SPEC-0300` behaviour 7): its family, defaults and SSL mode.
    /// A family without an SQL dialect (a non-relational store such as Redis, SPEC-0310) always
    /// names its system, even its own family system: the dialect alone cannot say what it is.
    pub fn for_connector(connector: &crate::Connector) -> Self {
        let dialect = connector.family.dialect();
        let mut form = Self::new(dialect.unwrap_or(Dialect::PostgreSql));
        if dialect.is_none() || !connector.is_family_system() {
            form.system = Some(connector.id.clone());
        }
        let spec = &connector.connection;
        if !connector.has_field(crate::connector::Field::Host) {
            form.host.clear();
        }
        form.database = spec.default_database.clone().unwrap_or_default();
        form.user = spec.default_user.clone().unwrap_or_default();
        form.ssl = spec.ssl.unwrap_or(SslMode::SECURE_DEFAULT);
        form
    }

    /// An empty form for `dialect`, with `localhost` for servers.
    pub fn new(dialect: Dialect) -> Self {
        Self {
            name: String::new(),
            dialect,
            system: None,
            host: if dialect.file_based() {
                String::new()
            } else {
                "localhost".into()
            },
            port: String::new(),
            database: String::new(),
            user: String::new(),
            path: String::new(),
            // SPEC-0016 v2 behaviour 8 (IMP-0370): new connections check certificate and host.
            ssl: if dialect.file_based() {
                SslMode::default()
            } else {
                SslMode::SECURE_DEFAULT
            },
            read_only: false,
            extras: ConnectionExtras::default(),
        }
    }

    /// The form showing `profile`.
    pub fn from_profile(profile: &ConnectionProfile) -> Self {
        Self {
            name: profile.name.clone(),
            dialect: profile.dialect,
            system: profile.system.clone(),
            host: profile.host.clone().unwrap_or_default(),
            port: profile.port.map(|p| p.to_string()).unwrap_or_default(),
            database: profile.database.clone().unwrap_or_default(),
            user: profile.user.clone().unwrap_or_default(),
            path: profile
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            ssl: profile.ssl,
            read_only: profile.read_only,
            extras: ConnectionExtras {
                ssh: profile.ssh.clone(),
                tls: profile.tls.clone(),
                options: profile.options.clone(),
                color: profile.color,
                comment: profile.comment.clone().unwrap_or_default(),
            },
        }
    }

    /// The connection URL of the form as it is, never with a password (`SPEC-0108` v4
    /// behaviour 9): what the URL field shows. `None` while the form is not a valid profile.
    pub fn url(&self) -> Option<String> {
        self.to_profile("url", None, None).ok().map(|p| p.to_url())
    }

    /// Takes the server, port, database, user and SSL mode (or the file) from `url`, as typed or
    /// pasted into the URL field (`SPEC-0108` v4 behaviour 9). A URL with a password is refused
    /// and changes nothing; the error never repeats it.
    pub fn apply_url(&mut self, url: &str) -> Result<(), ProfileError> {
        let p = ConnectionProfile::from_url(&self.name, url.trim())?;
        self.dialect = p.dialect;
        self.system = p.system;
        self.host = p.host.unwrap_or_default();
        self.port = p.port.map(|p| p.to_string()).unwrap_or_default();
        self.database = p.database.unwrap_or_default();
        self.user = p.user.unwrap_or_default();
        self.path = p.path.map(|p| p.display().to_string()).unwrap_or_default();
        self.ssl = p.ssl;
        Ok(())
    }

    /// The profile with id `id`; `base` keeps what the form does not show (agent permissions) and
    /// `secret` names the keychain entry of the password, if there is one.
    pub fn to_profile(
        &self,
        id: &str,
        base: Option<&ConnectionProfile>,
        secret: Option<SecretRef>,
    ) -> Result<ConnectionProfile, ProfileError> {
        let text = |s: &str| {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_owned())
        };
        let port = match self.port.trim() {
            "" => None,
            p => Some(p.parse::<u16>().map_err(|_| ProfileError::Invalid {
                name: self.name.clone(),
                field: "port",
                reason: "must be between 1 and 65535",
            })?),
        };
        let server = !self.dialect.file_based();
        let profile = ConnectionProfile {
            id: id.to_owned(),
            name: self.name.trim().to_owned(),
            dialect: self.dialect,
            system: self.system.clone(),
            host: text(&self.host).filter(|_| server),
            port: port.filter(|_| server),
            database: text(&self.database).filter(|_| server),
            user: text(&self.user).filter(|_| server),
            path: text(&self.path).filter(|_| !server).map(PathBuf::from),
            ssl: self.ssl,
            read_only: self.read_only,
            secret: secret.filter(|_| server),
            ssh: self.extras.ssh.clone().filter(|_| server),
            tls: if server {
                self.extras.tls.clone()
            } else {
                TlsFiles::default()
            },
            options: self.extras.options.clone(),
            color: self.extras.color,
            comment: text(&self.extras.comment),
            // A new profile gets the default agent permissions (reading only).
            ai: base.map_or_else(crate::permission::default_permissions, |b| b.ai.clone()),
        };
        profile.validate().map(|()| profile)
    }
}

/// An id for a new connection called `name`: its letters and digits, made unique among `profiles`.
pub fn unique_id(name: &str, profiles: &[ConnectionProfile]) -> String {
    let mut base: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if base.is_empty() {
        base = "connection".into();
    }
    let taken = |id: &str| profiles.iter().any(|p| p.id == id);
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .expect("an unused id")
}

/// The keychain entry for the password of the connection with id `id` (behaviour 4).
pub fn secret_ref(id: &str) -> SecretRef {
    SecretRef {
        service: KEYCHAIN_SERVICE.into(),
        account: format!("connection:{id}"),
    }
}

/// Puts `profile` into `profiles`, replacing the one with the same id, and checks the result
/// (unique names and ids, behaviour 3).
pub fn upsert(
    profiles: &[ConnectionProfile],
    profile: ConnectionProfile,
) -> Result<Vec<ConnectionProfile>, ProfileError> {
    let mut out = profiles.to_vec();
    match out.iter_mut().find(|p| p.id == profile.id) {
        Some(p) => *p = profile,
        None => out.push(profile),
    }
    validate_all(&out)?;
    Ok(out)
}

/// `profiles` without the connection with id `id` (behaviour 5).
pub fn remove(profiles: &[ConnectionProfile], id: &str) -> Vec<ConnectionProfile> {
    profiles.iter().filter(|p| p.id != id).cloned().collect()
}

/// What a successful connection test found out (behaviour 6, v3): the database product and server
/// version, the driver, the round trip of the test query, whether the connection is encrypted and,
/// for a file database, the file. Holds no user name and never a password.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionTest {
    /// Display name of the tested connection.
    pub name: String,
    /// Database product: the connector's name, else the dialect's (MariaDB when a MySQL server
    /// says so in its version).
    pub product: String,
    /// Version the server reports; `None` when it could not be asked cheaply.
    pub server_version: Option<String>,
    /// The client library IDECraft talks to the database with.
    pub driver: String,
    /// Round trip of the test query (`SELECT 1` by default).
    pub ping: Duration,
    /// Whether the connection is encrypted; `None` for files or when the server does not tell.
    pub tls: Option<bool>,
    /// The database file (SQLite, DuckDB).
    pub file: Option<PathBuf>,
    /// The SSH host the connection went through (`SPEC-0108` v4 behaviour 8).
    pub tunnel: Option<String>,
}

impl ConnectionTest {
    /// The first line: "Connected to <name>".
    pub fn summary(&self) -> String {
        format!("Connected to {}", self.name)
    }

    /// The round trip in milliseconds, to a tenth.
    pub fn ping_ms(&self) -> f64 {
        (self.ping.as_secs_f64() * 10_000.0).round() / 10.0
    }

    /// The lines under the summary: server, driver, round trip, TLS (when known), file (when one).
    pub fn detail_lines(&self) -> Vec<String> {
        let mut lines = vec![match &self.server_version {
            Some(v) => format!("Server: {} {v}", self.product),
            None => format!("Server: {}", self.product),
        }];
        lines.push(format!("Driver: {}", self.driver));
        lines.push(format!("Round trip: {}", format_ms(self.ping)));
        if let Some(tls) = self.tls {
            lines.push(format!("TLS: {}", if tls { "yes" } else { "no" }));
        }
        if let Some(file) = &self.file {
            lines.push(format!("File: {}", file.display()));
        }
        if let Some(host) = &self.tunnel {
            lines.push(format!("SSH tunnel: {host} (local end 127.0.0.1)"));
        }
        lines
    }

    /// Summary and details as plain text, one per line, for the clipboard.
    pub fn to_text(&self) -> String {
        std::iter::once(self.summary())
            .chain(self.detail_lines())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl std::fmt::Display for ConnectionTest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

/// Earlier callers compared the test result with its note text.
impl PartialEq<&str> for ConnectionTest {
    fn eq(&self, other: &&str) -> bool {
        self.summary() == *other
    }
}

/// `d` as milliseconds: a tenth below 10 ms, whole above.
fn format_ms(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 10.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}

/// The client library of `driver`.
fn driver_name(driver: &Driver) -> &'static str {
    match driver {
        Driver::Postgres(_) => "tokio-postgres",
        Driver::MySql(_) => "mysql_async",
        Driver::Sqlite(_) => "rusqlite (bundled SQLite)",
        Driver::SqlServer(_) => "tiberius",
        Driver::Oracle(_) => "rust-oracle (Oracle Instant Client)",
        Driver::DuckDb(_) => "duckdb-rs",
    }
}

/// The cheap query for the server version of `driver`.
fn version_query(driver: &Driver) -> &'static str {
    match driver {
        Driver::Postgres(_) => "show server_version",
        Driver::MySql(_) | Driver::DuckDb(_) => "select version()",
        Driver::Sqlite(_) => "select sqlite_version()",
        Driver::SqlServer(_) => "select cast(serverproperty('ProductVersion') as nvarchar(128))",
        Driver::Oracle(_) => "select version_full from product_component_version",
    }
}

/// The first value of the first row `sql` returns, if any.
async fn first_value(session: &Session, sql: &str) -> Option<String> {
    let run = session.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { rows, .. }) => rows
            .into_iter()
            .next()?
            .into_iter()
            .next()
            .flatten()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty()),
        _ => None,
    }
}

/// Connects with `profile` and runs the system's test query, `SELECT 1` by default (behaviour 6):
/// the error message, or what the test found out (v3: version, driver, round trip, TLS, file).
pub async fn test_connection(
    profile: &ConnectionProfile,
    credentials: &Credentials,
) -> Result<ConnectionTest, String> {
    let session = Session::connect(profile, credentials)
        .await
        .map_err(|e| with_tls_hint(profile, e.to_string()))?;
    test_session(profile, session).await
}

/// [`test_connection`] for a call of an agent in the workspace `root`: the session is opened
/// with [`Session::connect_for_agent`] (`SPEC-0012` v3 behaviours 8 and 15).
pub async fn test_connection_for_agent(
    profile: &ConnectionProfile,
    credentials: &Credentials,
    root: &Path,
) -> Result<ConnectionTest, String> {
    let session = Session::connect_for_agent(profile, credentials, root)
        .await
        .map_err(|e| with_tls_hint(profile, e.to_string()))?;
    test_session(profile, session).await
}

/// Runs the test query on an open `session` of `profile` and reports what it found out.
async fn test_session(
    profile: &ConnectionProfile,
    session: Session,
) -> Result<ConnectionTest, String> {
    let test = session
        .system()
        .and_then(|c| c.sql.test_query.clone())
        .unwrap_or_else(|| "SELECT 1".into());
    let started = Instant::now();
    let run = session.run_script(&test, OnError::Stop).await;
    let ping = started.elapsed();
    if let Some(Outcome::Error(e)) = run.outcomes.into_iter().next().map(|o| o.outcome) {
        return Err(e.to_string());
    }
    let version = session
        .system()
        .and_then(|c| c.sql.version_query.clone())
        .unwrap_or_else(|| version_query(&session.driver).into());
    let server_version = first_value(&session, &version).await;
    let mut product = session
        .system()
        .map_or_else(|| session.dialect().label().to_owned(), |c| c.name.clone());
    if session.system().is_none()
        && matches!(session.driver, Driver::MySql(_))
        && server_version
            .as_deref()
            .is_some_and(|v| v.contains("MariaDB"))
    {
        product = Dialect::MariaDb.label().into();
    }
    Ok(ConnectionTest {
        name: profile.name.clone(),
        product,
        server_version,
        driver: driver_name(&session.driver).into(),
        ping,
        tls: session.encrypted().await,
        file: profile
            .path
            .clone()
            .filter(|_| profile.dialect.file_based()),
        tunnel: session
            .tunnel_address()
            .and(profile.ssh.as_ref().map(|s| s.host.clone())),
    })
}

/// The client library IDECraft uses for `dialect` (`SPEC-0108` v4 behaviour 9: the dialog's
/// Driver line). Each family has one built-in driver; none is downloaded (IMP-0371).
pub fn driver_label(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::PostgreSql => "tokio-postgres (built in)",
        Dialect::MySql | Dialect::MariaDb => "mysql_async (built in)",
        Dialect::Sqlite => "rusqlite with bundled SQLite (built in)",
        Dialect::SqlServer => "tiberius (built in)",
        Dialect::Oracle => "rust-oracle with Oracle Instant Client",
        Dialect::DuckDb => "duckdb-rs (built in)",
    }
}

/// `error` of connecting with `profile`, with what to do when a server on this machine has no
/// TLS while the profile checks certificates (SPEC-0016 v2 behaviour 8): the secure default is
/// kept, the user decides.
pub fn with_tls_hint(profile: &ConnectionProfile, error: String) -> String {
    let local = profile.ssh.is_none()
        && profile
            .host
            .as_deref()
            .is_some_and(|h| matches!(h, "localhost" | "127.0.0.1" | "::1" | "[::1]"));
    let no_tls = {
        let e = error.to_lowercase();
        e.contains("ssl mode") && (e.contains("tls") || e.contains("ssl"))
    };
    if local && no_tls && profile.ssl.verifies() {
        format!(
            "{error}. The server on this machine may have no TLS certificate: choose SSL mode \
             `disable` only if it really has none (traffic stays on this machine)"
        )
    } else {
        error
    }
}

/// Saves the dialog's form for the workspace at `root` (behaviours 3–4): a new connection gets a
/// unique id, an edited one keeps its id and agent permissions; a typed password goes to `store` and
/// the profile names its entry, an empty one keeps the stored password. Returns the saved name.
pub fn save_connection(
    root: &Path,
    id: Option<&str>,
    form: &ConnectionForm,
    password: Option<&str>,
    store: &dyn SecretStore,
) -> Result<String, String> {
    let profiles = load_profiles(root).map_err(|e| e.to_string())?;
    let base = id.and_then(|id| profiles.iter().find(|p| p.id == id));
    let id = id.map_or_else(|| unique_id(&form.name, &profiles), str::to_owned);
    let mut secret = base.and_then(|b| b.secret.clone());
    // Check the form before anything is written to the keychain.
    form.to_profile(&id, base, None)
        .map_err(|e| e.to_string())?;
    if let Some(pw) = password.filter(|_| !form.dialect.file_based()) {
        let reference = secret.clone().unwrap_or_else(|| secret_ref(&id));
        store
            .set(&reference, &Secret::new(pw))
            .map_err(|e| e.to_string())?;
        secret = Some(reference);
    }
    let profile = form
        .to_profile(&id, base, secret)
        .map_err(|e| e.to_string())?;
    let name = profile.name.clone();
    let profiles = upsert(&profiles, profile).map_err(|e| e.to_string())?;
    save_profiles(root, &profiles).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Removes connection `id` of the workspace at `root` and its password in `store` (behaviour 5).
/// Returns the removed name.
pub fn remove_connection(root: &Path, id: &str, store: &dyn SecretStore) -> Result<String, String> {
    let profiles = load_profiles(root).map_err(|e| e.to_string())?;
    let profile = profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("no connection `{id}`"))?;
    if let Some(reference) = &profile.secret {
        store.delete(reference).map_err(|e| e.to_string())?;
    }
    let name = profile.name.clone();
    save_profiles(root, &remove(&profiles, id)).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Tests the dialog's form (behaviour 6): with a typed password, else the stored one of connection
/// `id`, else none. Builds its own runtime, so it runs on any thread.
pub fn test_form(
    root: &Path,
    id: Option<&str>,
    form: &ConnectionForm,
    password: Option<&str>,
    store: &dyn SecretStore,
) -> Result<ConnectionTest, String> {
    let profiles = load_profiles(root).unwrap_or_default();
    let base = id.and_then(|id| profiles.iter().find(|p| p.id == id));
    let mut profile = form
        .to_profile(
            id.unwrap_or("test"),
            base,
            base.and_then(|b| b.secret.clone()),
        )
        .map_err(|e| e.to_string())?;
    // A relative database file is found from the workspace (SPEC-0008 v5 behaviour 16).
    profile.options.base = Some(root.to_path_buf());
    let credentials = match password {
        Some(pw) => Credentials::Available(Secret::new(pw)),
        None => resolve_credentials(&profile, store).map_err(|e| e.to_string())?,
    };
    if let Credentials::Missing { reference, .. } = &credentials {
        return Err(format!(
            "no password in the keychain for {}/{}; type it into the dialog",
            reference.service, reference.account
        ));
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(test_connection(&profile, &credentials))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> ConnectionForm {
        ConnectionForm {
            name: "Shop DB".into(),
            port: "5433".into(),
            database: "shop".into(),
            user: "app".into(),
            ..ConnectionForm::new(Dialect::PostgreSql)
        }
    }

    #[test]
    fn form_becomes_a_checked_profile_and_back() {
        let p = form()
            .to_profile("shop-db", None, Some(secret_ref("shop-db")))
            .unwrap();
        assert_eq!(p.host.as_deref(), Some("localhost"));
        assert_eq!(p.port, Some(5433));
        assert_eq!(p.secret.as_ref().unwrap().account, "connection:shop-db");
        assert_eq!(p.ai, crate::permission::default_permissions());
        assert_eq!(ConnectionForm::from_profile(&p), form());
        let bad_port = ConnectionForm {
            port: "70000".into(),
            ..form()
        };
        assert!(bad_port.to_profile("x", None, None).is_err());
        let no_host = ConnectionForm {
            host: " ".into(),
            ..form()
        };
        assert!(no_host.to_profile("x", None, None).is_err());
        let sqlite = ConnectionForm {
            name: "local".into(),
            path: "data/app.db".into(),
            host: "ignored".into(),
            ..ConnectionForm::new(Dialect::Sqlite)
        };
        let s = sqlite
            .to_profile("local", None, Some(secret_ref("local")))
            .unwrap();
        assert_eq!(s.path, Some(PathBuf::from("data/app.db")));
        assert!(
            s.host.is_none() && s.secret.is_none(),
            "files need no server or password"
        );
    }

    #[test]
    fn ids_are_unique_and_upsert_checks_names() {
        let a = form().to_profile("shop-db", None, None).unwrap();
        assert_eq!(unique_id("Shop DB", &[]), "shop-db");
        assert_eq!(unique_id("Shop DB", std::slice::from_ref(&a)), "shop-db-2");
        assert_eq!(unique_id("  ", &[]), "connection");
        let list = upsert(&[], a.clone()).unwrap();
        let renamed = ConnectionForm {
            name: "Shop".into(),
            ..form()
        }
        .to_profile("shop-db", Some(&a), None)
        .unwrap();
        let list = upsert(&list, renamed).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Shop");
        let twin = ConnectionForm {
            name: "Shop".into(),
            ..form()
        }
        .to_profile("other", None, None)
        .unwrap();
        assert!(upsert(&list, twin).is_err(), "names stay unique");
        assert!(remove(&list, "shop-db").is_empty());
    }

    #[test]
    fn save_and_remove_keep_passwords_in_the_store_only() {
        let root = std::env::temp_dir().join(format!("idecraft-conn-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::MemoryStore::default();
        let name = save_connection(&root, None, &form(), Some("s3cret"), &store).unwrap();
        assert_eq!(name, "Shop DB");
        let saved = load_profiles(&root).unwrap();
        let reference = saved[0].secret.clone().unwrap();
        assert_eq!(store.get(&reference).unwrap().unwrap().expose(), "s3cret");
        let file = std::fs::read_to_string(root.join(crate::PROFILE_FILE)).unwrap();
        assert!(
            !file.contains("s3cret"),
            "the password is never in the file"
        );
        // Editing without a password keeps the stored one and the id.
        let renamed = ConnectionForm {
            name: "Shop".into(),
            ..form()
        };
        save_connection(&root, Some("shop-db"), &renamed, None, &store).unwrap();
        let saved = load_profiles(&root).unwrap();
        assert_eq!(
            (saved.len(), saved[0].id.as_str(), saved[0].name.as_str()),
            (1, "shop-db", "Shop")
        );
        assert_eq!(saved[0].secret.as_ref(), Some(&reference));
        // A second connection with the same name is refused before the keychain is touched.
        assert!(save_connection(&root, None, &renamed, Some("x"), &store).is_err());
        assert_eq!(remove_connection(&root, "shop-db", &store).unwrap(), "Shop");
        assert!(load_profiles(&root).unwrap().is_empty());
        assert!(
            store.get(&reference).unwrap().is_none(),
            "the password goes too"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0108 v3 AK-6: the detail lines and the clipboard text of a test.
    #[test]
    fn test_details_are_formatted_without_secrets() {
        let server = ConnectionTest {
            name: "Shop DB".into(),
            product: "PostgreSQL".into(),
            server_version: Some("18.6".into()),
            driver: "tokio-postgres".into(),
            ping: Duration::from_micros(2_345),
            tls: Some(false),
            file: None,
            tunnel: None,
        };
        assert_eq!(server.summary(), "Connected to Shop DB");
        assert_eq!(server.to_string(), "Connected to Shop DB");
        assert!(server == "Connected to Shop DB");
        assert_eq!(
            server.detail_lines(),
            [
                "Server: PostgreSQL 18.6",
                "Driver: tokio-postgres",
                "Round trip: 2.3 ms",
                "TLS: no",
            ]
        );
        assert!((server.ping_ms() - 2.3).abs() < 1e-9);
        assert_eq!(
            server.to_text(),
            "Connected to Shop DB\nServer: PostgreSQL 18.6\nDriver: tokio-postgres\n\
             Round trip: 2.3 ms\nTLS: no"
        );
        let slow = ConnectionTest {
            ping: Duration::from_millis(42),
            tls: None,
            server_version: None,
            ..server.clone()
        };
        let lines = slow.detail_lines();
        assert_eq!(lines[0], "Server: PostgreSQL", "no version, no number");
        assert_eq!(lines[2], "Round trip: 42 ms");
        assert!(
            !lines.iter().any(|l| l.starts_with("TLS")),
            "unknown TLS is left out"
        );
        let file = ConnectionTest {
            product: "SQLite".into(),
            server_version: Some("3.46.0".into()),
            file: Some(PathBuf::from("/tmp/app.db")),
            tls: None,
            ..server
        };
        assert_eq!(file.detail_lines().last().unwrap(), "File: /tmp/app.db");
        let tunnelled = ConnectionTest {
            tunnel: Some("bastion".into()),
            ..file
        };
        assert_eq!(
            tunnelled.detail_lines().last().unwrap(),
            "SSH tunnel: bastion (local end 127.0.0.1)"
        );
    }

    /// SPEC-0108 v4 AK-12: new forms check certificates; the URL field shows the form without a
    /// password and fills it from a pasted URL; a URL with a password changes nothing.
    #[test]
    fn secure_default_and_url_field() {
        assert_eq!(
            ConnectionForm::new(Dialect::PostgreSql).ssl,
            SslMode::VerifyFull
        );
        assert_eq!(ConnectionForm::new(Dialect::Sqlite).ssl, SslMode::Prefer);
        let mut f = form();
        assert_eq!(
            f.url().as_deref(),
            Some("postgresql://app@localhost:5433/shop?sslmode=verify-full")
        );
        f.apply_url("mysql://root@db.example:3307/inv?ssl-mode=VERIFY_CA")
            .unwrap();
        assert_eq!(
            (
                f.dialect,
                f.host.as_str(),
                f.port.as_str(),
                f.user.as_str(),
                f.ssl
            ),
            (
                Dialect::MySql,
                "db.example",
                "3307",
                "root",
                SslMode::VerifyCa
            )
        );
        let before = f.clone();
        let err = f.apply_url("mysql://root:hunter2@h/db").unwrap_err();
        assert!(!err.to_string().contains("hunter2"));
        assert_eq!(f, before);
        assert!(driver_label(Dialect::PostgreSql).contains("tokio-postgres"));
        // A loopback server without TLS gets a hint; the mode stays as chosen.
        let local = form().to_profile("x", None, None).unwrap();
        let hinted = with_tls_hint(
            &local,
            "connection `x`: SSL mode `verify-full`: server does not support TLS".into(),
        );
        assert!(
            hinted.contains("choose SSL mode `disable` only if"),
            "{hinted}"
        );
        let mut remote = local.clone();
        remote.host = Some("db.example".into());
        assert!(!with_tls_hint(&remote, "SSL mode `verify-full`: x TLS".into()).contains("choose"));
    }

    /// SPEC-0108 v4 AK-10: tunnel, TLS files, options, colour and comment go from the form into
    /// the profile and back; a SQLite form drops tunnel and TLS files.
    #[test]
    fn extras_round_trip() {
        let mut f = form();
        f.extras = ConnectionExtras {
            ssh: Some(SshTunnel {
                host: "bastion".into(),
                port: None,
                user: Some("ops".into()),
            }),
            tls: TlsFiles {
                ca_file: Some("/ca.pem".into()),
                ..TlsFiles::default()
            },
            options: ConnectionOptions {
                auto_commit: false,
                query_timeout_secs: Some(30),
                ..ConnectionOptions::default()
            },
            color: Some(ConnectionColor::Green),
            comment: " local copy ".into(),
        };
        let p = f.to_profile("shop", None, None).unwrap();
        assert_eq!(p.comment.as_deref(), Some("local copy"));
        assert_eq!(p.ssh.as_ref().unwrap().host, "bastion");
        let back = ConnectionForm::from_profile(&p);
        assert_eq!(back.extras.comment, "local copy");
        assert_eq!(back.extras.ssh, f.extras.ssh);
        assert_eq!(back.extras.options, f.extras.options);
        let lite = ConnectionForm {
            name: "lite".into(),
            path: "/tmp/x.db".into(),
            extras: f.extras.clone(),
            ..ConnectionForm::new(Dialect::Sqlite)
        };
        let lp = lite.to_profile("lite", None, None).unwrap();
        assert!(lp.ssh.is_none() && lp.tls.is_empty());
    }

    #[test]
    fn a_store_picked_from_the_list_keeps_its_system() {
        let cat = crate::catalogue();
        let system = |id: &str| ConnectionForm::for_connector(cat.get(id).unwrap()).system;
        // Redis and MongoDB are their families' own systems but have no SQL dialect: without the
        // system the form would be a PostgreSQL connection (SPEC-0310 AK-9 self-test).
        assert_eq!(system("redis").as_deref(), Some("redis"));
        assert_eq!(system("mongodb").as_deref(), Some("mongodb"));
        assert_eq!(system("valkey").as_deref(), Some("valkey"));
        assert_eq!(
            system("postgresql"),
            None,
            "SQL family systems stay implicit"
        );
        assert_eq!(system("tidb").as_deref(), Some("tidb"));
    }
}
