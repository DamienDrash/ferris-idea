//! Connection profiles (`SPEC-0008`).
//!
//! Profiles are plain data shared through `.idecraft/connections.toml`. They
//! never hold passwords; a profile may point at a keychain entry instead. The
//! user name belongs to the profile so it survives any restart.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use std::collections::BTreeSet;

use crate::Dialect;
use crate::permission::{Permission, default_permissions};

/// Workspace-relative location of the profile file.
pub const PROFILE_FILE: &str = ".idecraft/connections.toml";

/// Transport security requested for a server connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    /// Never use TLS.
    Disable,
    /// Use TLS when the server offers it.
    #[default]
    Prefer,
    /// Require TLS without checking the certificate.
    Require,
    /// Require TLS and check that the certificate chains to a trusted root, not the host name
    /// (`SPEC-0016` v2).
    VerifyCa,
    /// Require TLS and check certificate and host name.
    VerifyFull,
}

impl SslMode {
    /// Whether the server's certificate is checked (`verify-ca`, `verify-full`).
    pub fn verifies(self) -> bool {
        matches!(self, Self::VerifyCa | Self::VerifyFull)
    }

    /// The mode new connections start with (`SPEC-0016` v2 behaviour 8, IMP-0370): the
    /// certificate and host name are checked.
    pub const SECURE_DEFAULT: Self = Self::VerifyFull;
}

/// Where the password of a profile is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    /// Keychain service name.
    pub service: String,
    /// Keychain account name.
    pub account: String,
}

/// An SSH tunnel to reach the database server (`SPEC-0108` v4 behaviour 8): IDECraft runs the
/// system `ssh` in batch mode with strict host key checking and forwards a port on `127.0.0.1` to
/// the profile's host and port as the SSH host sees them. No password or passphrase is kept or
/// asked for: keys, the SSH agent and `~/.ssh/config` apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshTunnel {
    /// SSH host: an alias from `~/.ssh/config` or a host name.
    pub host: String,
    /// SSH port, when not the one of the config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// SSH user, when not the one of the config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// Certificate files for TLS (`SPEC-0016` v2): paths on this machine, `~` for the home folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsFiles {
    /// PEM file with the certificate authorities to trust instead of the system's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<PathBuf>,
    /// Client certificate: PEM (with `key_file`) or a PKCS#12 bundle (`.p12`, `.pfx`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_file: Option<PathBuf>,
    /// PEM file with the client certificate's private key (PKCS#8, not encrypted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<PathBuf>,
}

impl TlsFiles {
    /// Whether no file is set.
    pub fn is_empty(&self) -> bool {
        self.ca_file.is_none() && self.cert_file.is_none() && self.key_file.is_none()
    }
}

/// Further options of a connection (`SPEC-0108` v4 behaviour 10).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionOptions {
    /// Whether consoles and data editors of this connection start in automatic commit mode.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub auto_commit: bool,
    /// SQL run after every connect, before anything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_script: Option<String>,
    /// Seconds allowed for connecting (default 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout_secs: Option<u32>,
    /// Seconds a statement may run before the server ends it (PostgreSQL, MySQL, MariaDB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_timeout_secs: Option<u32>,
    /// Seconds between TCP keep-alive probes of an idle connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_alive_secs: Option<u32>,
    /// Schemas shown besides the current one; `*` matches any characters. Empty shows all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemas: Vec<String>,
    /// Whether the startup script may run: `false` for a profile read from a project that is not
    /// trusted, whose files run no code in safe mode (`SPEC-0278`, `SPEC-0108` v4 behaviour 10).
    /// Never written.
    #[serde(skip, default = "yes")]
    pub startup_allowed: bool,
    /// The workspace folder the profile was read from: a relative database file is found from
    /// there, not from the folder IDECraft happened to be started in (`SPEC-0008` v5 behaviour
    /// 16). Never written, and no part of what makes two profiles equal.
    #[serde(skip)]
    pub base: Option<PathBuf>,
}

impl PartialEq for ConnectionOptions {
    fn eq(&self, other: &Self) -> bool {
        self.auto_commit == other.auto_commit
            && self.startup_script == other.startup_script
            && self.connect_timeout_secs == other.connect_timeout_secs
            && self.query_timeout_secs == other.query_timeout_secs
            && self.keep_alive_secs == other.keep_alive_secs
            && self.schemas == other.schemas
            && self.startup_allowed == other.startup_allowed
    }
}

impl Eq for ConnectionOptions {}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            auto_commit: true,
            startup_script: None,
            connect_timeout_secs: None,
            query_timeout_secs: None,
            keep_alive_secs: None,
            schemas: Vec::new(),
            startup_allowed: true,
            base: None,
        }
    }
}

impl ConnectionOptions {
    /// Whether every stored option has its default.
    pub fn is_default(&self) -> bool {
        Self {
            startup_allowed: true,
            ..self.clone()
        } == Self::default()
    }

    /// Time allowed for connecting (default 10 s).
    pub fn connect_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.connect_timeout_secs.map_or(10, u64::from))
    }

    /// Whether schema `name` is shown under the connection (the schema filter).
    pub fn shows_schema(&self, name: &str) -> bool {
        self.schemas.is_empty()
            || self
                .schemas
                .iter()
                .any(|pattern| glob_match(pattern.trim(), name))
    }
}

/// `pattern` with `*` for any characters matches `text`, ignoring case.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.to_lowercase(), text.to_lowercase());
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == t;
    }
    let Some(mut rest) = t.strip_prefix(parts[0]) else {
        return false;
    };
    let last = parts[parts.len() - 1];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// The colour that marks a connection in the tree and on its consoles (`SPEC-0108` v4
/// behaviour 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionColor {
    /// Red, e.g. production.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green, e.g. local.
    Green,
    /// Blue.
    Blue,
    /// Violet.
    Violet,
    /// Grey.
    Gray,
}

impl ConnectionColor {
    /// Every colour in menu order.
    pub const ALL: [Self; 7] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Violet,
        Self::Gray,
    ];

    /// The name as written in profiles.
    pub fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Violet => "violet",
            Self::Gray => "gray",
        }
    }

    /// The colour called `name`, if there is one.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The colour after `color` in the menu; `None` (no colour) after the last.
    pub fn next(color: Option<Self>) -> Option<Self> {
        match color {
            None => Some(Self::ALL[0]),
            Some(c) => {
                let i = Self::ALL.iter().position(|x| *x == c).unwrap_or(0);
                Self::ALL.get(i + 1).copied()
            }
        }
    }

    /// RGB of the colour, readable in light and dark themes.
    pub fn rgb(self) -> u32 {
        match self {
            Self::Red => 0xd9_4f_4f,
            Self::Orange => 0xe0_8a_3c,
            Self::Yellow => 0xd6_b6_2d,
            Self::Green => 0x5a_a8_5a,
            Self::Blue => 0x4a_8a_d9,
            Self::Violet => 0x9a_6a_d0,
            Self::Gray => 0x8c_8c_8c,
        }
    }
}

/// `path` with a leading `~` replaced by the home folder.
pub fn expand_home(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => std::env::var_os("HOME")
            .map(PathBuf::from)
            .map_or_else(|| path.to_path_buf(), |home| home.join(rest)),
        Err(_) => path.to_path_buf(),
    }
}

/// One named database connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionProfile {
    /// Stable identifier.
    pub id: String,
    /// Display name, unique within the workspace.
    pub name: String,
    /// Database dialect: the protocol family (ADR-0020).
    pub dialect: Dialect,
    /// Database system on top of the family (`SPEC-0300`); `None` is the family's own system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Server host (not for SQLite).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Server port; the dialect's default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Database name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// User name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Database file (SQLite only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Transport security.
    #[serde(default)]
    pub ssl: SslMode,
    /// Refuse statements that change data or schema.
    #[serde(default)]
    pub read_only: bool,
    /// Keychain entry holding the password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<SecretRef>,
    /// SSH tunnel to the server (`SPEC-0108` v4 behaviour 8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnel>,
    /// Certificate files for TLS (`SPEC-0016` v2).
    #[serde(default, skip_serializing_if = "TlsFiles::is_empty")]
    pub tls: TlsFiles,
    /// Further options (`SPEC-0108` v4 behaviour 10).
    #[serde(default, skip_serializing_if = "ConnectionOptions::is_default")]
    pub options: ConnectionOptions,
    /// Colour that marks the connection (`SPEC-0108` v4 behaviour 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ConnectionColor>,
    /// A note about the connection, shown in the tree's tooltip (`SPEC-0108` v4 behaviour 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// What agents may do through MCP (`SPEC-0012`); reading only by default.
    #[serde(
        default = "default_permissions",
        skip_serializing_if = "is_default_permissions"
    )]
    pub ai: BTreeSet<Permission>,
}

impl ConnectionProfile {
    /// The database file of a file-based profile (SQLite, DuckDB): a relative path is taken from
    /// the workspace folder the profile belongs to (`SPEC-0008` v5 behaviour 16).
    pub fn file_path(&self) -> PathBuf {
        let path = self.path.clone().unwrap_or_default();
        match &self.options.base {
            Some(base) if path.is_relative() && !path.as_os_str().is_empty() => base.join(path),
            _ => path,
        }
    }
}

impl ConnectionProfile {
    /// What this connection reaches, as the line a user's grant for agents is bound to
    /// (`SPEC-0012` v3, "Target"): the URL without password, `#` and a digest of everything that
    /// decides where it connects and what runs on connecting. A grant holds only while the
    /// project file keeps all of it.
    pub fn agent_target(&self) -> String {
        let text = |o: &Option<String>| o.clone().unwrap_or_default();
        let path = |o: &Option<PathBuf>| {
            o.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        };
        let file = if self.dialect.file_based() {
            let file = self.file_path();
            fs::canonicalize(&file)
                .unwrap_or(file)
                .display()
                .to_string()
        } else {
            String::new()
        };
        let secret = self
            .secret
            .as_ref()
            .map(|s| format!("{}\n{}", s.service, s.account))
            .unwrap_or_default();
        let ssh = self
            .ssh
            .as_ref()
            .map(|s| format!("{}\n{:?}\n{:?}", s.host, s.port, s.user))
            .unwrap_or_default();
        let parts = [
            format!("{:?}", self.dialect),
            text(&self.system),
            text(&self.host),
            format!("{:?}", self.effective_port()),
            text(&self.database),
            text(&self.user),
            file,
            format!("{:?}", self.ssl),
            secret,
            ssh,
            path(&self.tls.ca_file),
            path(&self.tls.cert_file),
            path(&self.tls.key_file),
            text(&self.options.startup_script),
        ];
        let shown = match &self.ssh {
            Some(ssh) => format!("{} via ssh {}", self.to_url(), ssh.host),
            None => self.to_url(),
        };
        idecraft_core::connection_target(&shown, &parts)
    }
}

/// The target line of a connection that is no [`ConnectionProfile`] (a store, `SPEC-0310`):
/// `shown` and a digest of `parts`, as [`ConnectionProfile::agent_target`] builds it.
pub fn target_line(shown: &str, parts: &[String]) -> String {
    idecraft_core::connection_target(shown, parts)
}

fn is_default_permissions(set: &BTreeSet<Permission>) -> bool {
    *set == default_permissions()
}

/// Errors from parsing, validating, loading or saving profiles.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// The URL could not be understood.
    #[error("invalid connection URL: {0}")]
    Url(String),
    /// The URL carried a password.
    #[error("connection URLs must not contain a password; store it in the system keychain instead")]
    PasswordInUrl,
    /// A profile field is missing or out of range.
    #[error("connection `{name}`: {field} {reason}")]
    Invalid {
        /// Profile name.
        name: String,
        /// Offending field.
        field: &'static str,
        /// What is wrong.
        reason: &'static str,
    },
    /// The profile file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// File involved.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// The profile file is not valid.
    #[error("{path}: {message}")]
    Parse {
        /// File involved.
        path: PathBuf,
        /// Parser message.
        message: String,
    },
}

impl ConnectionProfile {
    /// Port to connect to, applying the dialect default.
    pub fn effective_port(&self) -> Option<u16> {
        let system = self.system.as_deref().and_then(|id| {
            crate::connector::catalogue()
                .get(id)
                .and_then(|c| c.connection.default_port)
        });
        self.port.or(system).or(match self.dialect {
            Dialect::PostgreSql => Some(5432),
            Dialect::MySql | Dialect::MariaDb => Some(3306),
            Dialect::SqlServer => Some(1433),
            Dialect::Oracle => Some(1521),
            Dialect::Sqlite | Dialect::DuckDb => None,
        })
    }

    /// Parses a connection URL into a profile called `name`.
    pub fn from_url(name: &str, url: &str) -> Result<Self, ProfileError> {
        let mut profile = Self {
            id: slug(name),
            name: name.to_owned(),
            dialect: Dialect::Sqlite,
            system: None,
            host: None,
            port: None,
            database: None,
            user: None,
            path: None,
            ssl: SslMode::default(),
            read_only: false,
            secret: None,
            ssh: None,
            tls: TlsFiles::default(),
            options: ConnectionOptions::default(),
            color: None,
            comment: None,
            ai: default_permissions(),
        };
        if let Some(path) = url.strip_prefix("sqlite:") {
            let path = path.strip_prefix("//").unwrap_or(path);
            profile.path = Some(PathBuf::from(decode(path)?));
            return Ok(profile);
        }
        if let Some(path) = url.strip_prefix("duckdb:") {
            let path = path.strip_prefix("//").unwrap_or(path);
            profile.dialect = Dialect::DuckDb;
            profile.path = Some(PathBuf::from(decode(path)?));
            return Ok(profile);
        }
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| ProfileError::Url("missing scheme".into()))?;
        profile.dialect = match scheme.to_ascii_lowercase().as_str() {
            "postgresql" | "postgres" => Dialect::PostgreSql,
            "mysql" => Dialect::MySql,
            "mariadb" => Dialect::MariaDb,
            "sqlserver" | "mssql" => Dialect::SqlServer,
            "oracle" => Dialect::Oracle,
            other => {
                let catalogue = crate::connector::catalogue();
                let connector = catalogue
                    .by_scheme(other)
                    .ok_or_else(|| ProfileError::Url(format!("unsupported scheme `{other}`")))?;
                let dialect = connector.family.dialect().ok_or_else(|| {
                    ProfileError::Url(format!(
                        "{} needs the ODBC route, which IDECraft does not have yet",
                        connector.name
                    ))
                })?;
                profile.system = Some(connector.id.clone());
                if let Some(ssl) = connector.connection.ssl {
                    profile.ssl = ssl;
                }
                dialect
            }
        };
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (authority, database) = rest.split_once('/').unwrap_or((rest, ""));
        let host_port = match authority.rsplit_once('@') {
            Some((userinfo, host_port)) => {
                if userinfo.contains(':') {
                    return Err(ProfileError::PasswordInUrl);
                }
                profile.user = non_empty(decode(userinfo)?);
                host_port
            }
            None => authority,
        };
        let (host, port) = split_host_port(host_port)?;
        profile.host = non_empty(host);
        profile.port = port;
        profile.database = non_empty(decode(database)?);
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = decode(key)?;
            let value = decode(value)?;
            match (profile.dialect, key.as_str()) {
                (_, "password") => return Err(ProfileError::PasswordInUrl),
                (Dialect::PostgreSql | Dialect::SqlServer | Dialect::Oracle, "sslmode")
                | (Dialect::MySql | Dialect::MariaDb, "ssl-mode") => {
                    profile.ssl = parse_ssl(&value)?;
                }
                _ => {}
            }
        }
        Ok(profile)
    }

    /// Renders the profile as a URL without secrets.
    pub fn to_url(&self) -> String {
        if self.dialect.file_based() {
            let path = self.path.as_deref().unwrap_or(Path::new(""));
            let scheme = if self.dialect == Dialect::DuckDb {
                "duckdb"
            } else {
                "sqlite"
            };
            return format!("{scheme}:{}", encode_path(&path.to_string_lossy()));
        }
        let system_scheme = self.system.as_deref().and_then(|id| {
            crate::connector::catalogue()
                .get(id)
                .and_then(|c| c.connection.schemes.first().cloned())
        });
        let family_scheme = match self.dialect {
            Dialect::MySql => "mysql",
            Dialect::MariaDb => "mariadb",
            Dialect::SqlServer => "sqlserver",
            Dialect::Oracle => "oracle",
            _ => "postgresql",
        };
        let scheme = system_scheme.as_deref().unwrap_or(family_scheme);
        let mut url = format!("{scheme}://");
        if let Some(user) = &self.user {
            url.push_str(&encode(user));
            url.push('@');
        }
        let host = self.host.as_deref().unwrap_or("");
        if host.contains(':') {
            url.push_str(&format!("[{host}]"));
        } else {
            url.push_str(host);
        }
        if let Some(port) = self.port {
            url.push_str(&format!(":{port}"));
        }
        url.push('/');
        if let Some(database) = &self.database {
            url.push_str(&encode(database));
        }
        if self.ssl != SslMode::Prefer {
            let (key, value) = match (self.dialect, self.ssl) {
                (Dialect::MySql | Dialect::MariaDb, SslMode::Disable) => ("ssl-mode", "DISABLED"),
                (Dialect::MySql | Dialect::MariaDb, SslMode::Require) => ("ssl-mode", "REQUIRED"),
                (Dialect::MySql | Dialect::MariaDb, SslMode::VerifyCa) => ("ssl-mode", "VERIFY_CA"),
                (Dialect::MySql | Dialect::MariaDb, _) => ("ssl-mode", "VERIFY_IDENTITY"),
                (_, SslMode::Disable) => ("sslmode", "disable"),
                (_, SslMode::Require) => ("sslmode", "require"),
                (_, SslMode::VerifyCa) => ("sslmode", "verify-ca"),
                _ => ("sslmode", "verify-full"),
            };
            url.push_str(&format!("?{key}={value}"));
        }
        url
    }

    /// Checks the fields of one profile.
    pub fn validate(&self) -> Result<(), ProfileError> {
        let invalid = |field, reason| ProfileError::Invalid {
            name: self.name.clone(),
            field,
            reason,
        };
        if self.name.trim().is_empty() {
            return Err(invalid("name", "must not be empty"));
        }
        if let Some(system) = &self.system
            && let Some(c) = crate::connector::catalogue().get(system)
            && c.family.dialect() != Some(self.dialect)
        {
            return Err(invalid("system", "belongs to another dialect"));
        }
        match self.dialect {
            Dialect::Sqlite | Dialect::DuckDb => {
                if self.path.as_ref().is_none_or(|p| p.as_os_str().is_empty()) {
                    return Err(invalid("path", "must not be empty"));
                }
            }
            _ => {
                if self.host.as_ref().is_none_or(|h| h.trim().is_empty()) {
                    return Err(invalid("host", "is required"));
                }
                if self.port == Some(0) {
                    return Err(invalid("port", "must be between 1 and 65535"));
                }
            }
        }
        self.validate_more()
    }

    /// Checks SSH tunnel, TLS files and options (`SPEC-0108` v4 behaviour 11).
    fn validate_more(&self) -> Result<(), ProfileError> {
        let invalid = |field, reason| ProfileError::Invalid {
            name: self.name.clone(),
            field,
            reason,
        };
        if let Some(ssh) = &self.ssh {
            if !matches!(
                self.dialect,
                Dialect::PostgreSql | Dialect::MySql | Dialect::MariaDb
            ) {
                return Err(invalid(
                    "ssh",
                    "tunnels are available for PostgreSQL, MySQL and MariaDB",
                ));
            }
            if !ssh_name_ok(&ssh.host) {
                return Err(invalid(
                    "ssh host",
                    "must be a host name or alias (no spaces, not starting with `-`)",
                ));
            }
            if ssh
                .user
                .as_deref()
                .is_some_and(|u| !ssh_name_ok(u) || u.contains('@'))
            {
                return Err(invalid("ssh user", "must be a plain user name"));
            }
            if ssh.port == Some(0) {
                return Err(invalid("ssh port", "must be between 1 and 65535"));
            }
        }
        if !self.tls.is_empty() {
            // SQL Server takes a CA file only (`SPEC-0116` v6 behaviour 15).
            let sql_server_ca = self.dialect == Dialect::SqlServer
                && self.tls.cert_file.is_none()
                && self.tls.key_file.is_none();
            if self.dialect == Dialect::SqlServer && !sql_server_ca {
                return Err(invalid(
                    if self.tls.cert_file.is_some() {
                        "cert file"
                    } else {
                        "key file"
                    },
                    "is not taken by SQL Server, only a CA file",
                ));
            }
            if !sql_server_ca
                && !matches!(
                    self.dialect,
                    Dialect::PostgreSql | Dialect::MySql | Dialect::MariaDb
                )
            {
                return Err(invalid(
                    "tls",
                    "files are available for PostgreSQL, MySQL and MariaDB, a CA file for SQL \
                     Server",
                ));
            }
            if self.ssl == SslMode::Disable {
                return Err(invalid("tls", "files need an SSL mode other than disable"));
            }
        }
        if self.tls.ca_file.is_some() && !self.ssl.verifies() {
            return Err(invalid(
                "ca file",
                "is only checked with SSL mode verify-ca or verify-full",
            ));
        }
        let bundle = self.tls.cert_file.as_deref().is_some_and(is_pkcs12);
        match (&self.tls.cert_file, &self.tls.key_file) {
            (Some(_), None) if !bundle => {
                return Err(invalid(
                    "key file",
                    "is required with a PEM client certificate (or use a .p12 bundle)",
                ));
            }
            (Some(_), Some(_)) if bundle => {
                return Err(invalid(
                    "key file",
                    "is part of the .p12 bundle; leave it empty",
                ));
            }
            (None, Some(_)) => {
                return Err(invalid("cert file", "is required with a key file"));
            }
            _ => {}
        }
        if matches!(self.dialect, Dialect::MySql | Dialect::MariaDb)
            && self.tls.cert_file.is_some()
            && !bundle
        {
            return Err(invalid(
                "cert file",
                "must be a PKCS#12 bundle (.p12 or .pfx) for MySQL and MariaDB",
            ));
        }
        let o = &self.options;
        for (field, value) in [
            ("connect timeout", o.connect_timeout_secs),
            ("query timeout", o.query_timeout_secs),
            ("keep-alive", o.keep_alive_secs),
        ] {
            if value.is_some_and(|v| v == 0 || v > 86_400) {
                return Err(invalid(field, "must be between 1 and 86400 seconds"));
            }
        }
        if o.schemas.iter().any(|s| s.trim().is_empty()) {
            return Err(invalid("schemas", "must not contain an empty pattern"));
        }
        Ok(())
    }
}

/// Checks all profiles and that names and ids are unique.
pub fn validate_all(profiles: &[ConnectionProfile]) -> Result<(), ProfileError> {
    for (i, profile) in profiles.iter().enumerate() {
        profile.validate()?;
        for earlier in &profiles[..i] {
            let field = if earlier.name == profile.name {
                "name"
            } else if earlier.id == profile.id {
                "id"
            } else {
                continue;
            };
            return Err(ProfileError::Invalid {
                name: profile.name.clone(),
                field,
                reason: "is used by another connection",
            });
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    /// The envelope (`SPEC-0303` behaviour 16), checked separately.
    #[serde(default, rename = "idecraft", skip_serializing)]
    _envelope: Option<toml::Table>,
    #[serde(default, rename = "connection")]
    connections: Vec<ConnectionProfile>,
}

/// Loads the profiles of the workspace at `root`; a missing file gives none.
pub fn load_profiles(root: &Path) -> Result<Vec<ConnectionProfile>, ProfileError> {
    let path = root.join(PROFILE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(ProfileError::Io { path, source }),
    };
    if let Err(p) = idecraft_core::format::project_format(&text, "connections") {
        return Err(ProfileError::Parse {
            path,
            message: idecraft_core::format::problems_text(&p),
        });
    }
    let mut file: ProfileFile = toml::from_str(&text).map_err(|e| ProfileError::Parse {
        path: path.clone(),
        message: e.message().to_owned(),
    })?;
    validate_all(&file.connections)?;
    // A startup script is code of the project: it runs only when the project is trusted
    // (SPEC-0278 safe mode, SPEC-0108 v4 behaviour 10).
    let trusted = idecraft_core::trust::allows_project_code(root);
    for p in &mut file.connections {
        p.options.startup_allowed = trusted;
        p.options.base = Some(root.to_path_buf());
    }
    Ok(file.connections)
}

/// Saves `profiles` for the workspace at `root`, replacing the file atomically.
pub fn save_profiles(root: &Path, profiles: &[ConnectionProfile]) -> Result<(), ProfileError> {
    validate_all(profiles)?;
    let path = root.join(PROFILE_FILE);
    let text = idecraft_core::format::write(
        &idecraft_core::format::Envelope::current("connections"),
        &ProfileFile {
            _envelope: None,
            connections: profiles.to_vec(),
        },
    )
    .map_err(|message| ProfileError::Parse {
        path: path.clone(),
        message,
    })?;
    let io_err = |source| ProfileError::Io {
        path: path.clone(),
        source,
    };
    idecraft_core::format::prepare_write(&path, "connections").map_err(io_err)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_err)?;
    }
    let tmp = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    fs::write(&tmp, text).map_err(io_err)?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io_err(e)
    })
}

fn parse_ssl(value: &str) -> Result<SslMode, ProfileError> {
    match value.to_ascii_lowercase().replace('_', "-").as_str() {
        "disable" | "disabled" => Ok(SslMode::Disable),
        "allow" | "prefer" | "preferred" => Ok(SslMode::Prefer),
        "require" | "required" => Ok(SslMode::Require),
        "verify-ca" => Ok(SslMode::VerifyCa),
        "verify-full" | "verify-identity" => Ok(SslMode::VerifyFull),
        other => Err(ProfileError::Url(format!("unsupported SSL mode `{other}`"))),
    }
}

/// A host, alias or user name that `ssh` cannot take for an option or split into words.
fn ssh_name_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-@[]:%".contains(c))
}

/// Whether `path` names a PKCS#12 bundle (`.p12`, `.pfx`).
pub fn is_pkcs12(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("p12") || e.eq_ignore_ascii_case("pfx"))
}

fn split_host_port(host_port: &str) -> Result<(String, Option<u16>), ProfileError> {
    let (host, port) = if let Some(rest) = host_port.strip_prefix('[') {
        let (host, after) = rest
            .split_once(']')
            .ok_or_else(|| ProfileError::Url("unclosed `[` in host".into()))?;
        (host, after.strip_prefix(':'))
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    let port = match port {
        Some(p) => Some(
            p.parse::<u16>()
                .map_err(|_| ProfileError::Url(format!("invalid port `{p}`")))?,
        ),
        None => None,
    };
    Ok((decode(host)?, port))
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug.trim_matches('-').to_owned()
}

fn decode(s: &str) -> Result<String, ProfileError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| ProfileError::Url("invalid percent encoding".into()))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ProfileError::Url("percent encoding is not UTF-8".into()))
}

fn encode_with(s: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) || keep(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn encode(s: &str) -> String {
    encode_with(s, |_| false)
}

fn encode_path(s: &str) -> String {
    encode_with(s, |b| b == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("idecraft-profiles-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn pg() -> ConnectionProfile {
        ConnectionProfile::from_url(
            "ref-pg-local",
            "postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=disable",
        )
        .unwrap()
    }

    #[test]
    fn parses_postgres_url() {
        let p = pg();
        assert_eq!(p.id, "ref-pg-local");
        assert_eq!(p.dialect, Dialect::PostgreSql);
        assert_eq!(p.user.as_deref(), Some("idecraft"));
        assert_eq!(p.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(p.port, Some(54329));
        assert_eq!(p.database.as_deref(), Some("inventory"));
        assert_eq!(p.ssl, SslMode::Disable);
    }

    #[test]
    fn parses_mysql_defaults_and_sqlite_paths() {
        let m = ConnectionProfile::from_url("m", "mysql://root@localhost/inventory").unwrap();
        assert_eq!(m.dialect, Dialect::MySql);
        assert_eq!(m.port, None);
        assert_eq!(m.effective_port(), Some(3306));
        assert_eq!(m.ssl, SslMode::Prefer);
        let r = ConnectionProfile::from_url("m", "mysql://h/db?ssl-mode=REQUIRED").unwrap();
        assert_eq!(r.ssl, SslMode::Require);

        let s = ConnectionProfile::from_url("s", "sqlite:/tmp/inv.sqlite").unwrap();
        assert_eq!(s.dialect, Dialect::Sqlite);
        assert_eq!(s.path.as_deref(), Some(Path::new("/tmp/inv.sqlite")));
        assert_eq!(s.effective_port(), None);
    }

    #[test]
    fn parses_oracle_urls() {
        let o =
            ConnectionProfile::from_url("o", "oracle://scott@db.example:1522/FREEPDB1").unwrap();
        assert_eq!(o.dialect, Dialect::Oracle);
        assert_eq!(o.port, Some(1522));
        assert_eq!(o.database.as_deref(), Some("FREEPDB1"));
        assert_eq!(o.to_url(), "oracle://scott@db.example:1522/FREEPDB1");
        let d = ConnectionProfile::from_url("d", "oracle://scott@h/XE").unwrap();
        assert_eq!(d.effective_port(), Some(1521));
    }

    #[test]
    fn parses_mariadb_and_sql_server_urls() {
        let m = ConnectionProfile::from_url("m", "mariadb://root@h/db?ssl-mode=DISABLED").unwrap();
        assert_eq!(m.dialect, Dialect::MariaDb);
        assert_eq!(m.effective_port(), Some(3306));
        assert_eq!(m.ssl, SslMode::Disable);
        assert_eq!(m.to_url(), "mariadb://root@h/db?ssl-mode=DISABLED");
        let s =
            ConnectionProfile::from_url("s", "mssql://sa@h:14330/shop?sslmode=require").unwrap();
        assert_eq!(s.dialect, Dialect::SqlServer);
        assert_eq!(s.port, Some(14330));
        assert_eq!(s.ssl, SslMode::Require);
        assert_eq!(s.to_url(), "sqlserver://sa@h:14330/shop?sslmode=require");
        let d = ConnectionProfile::from_url("d", "sqlserver://sa@h/shop").unwrap();
        assert_eq!(d.effective_port(), Some(1433));
    }

    #[test]
    fn decodes_percent_encoding() {
        let p = ConnectionProfile::from_url("p", "postgres://us%40er@h/my%20db").unwrap();
        assert_eq!(p.user.as_deref(), Some("us@er"));
        assert_eq!(p.database.as_deref(), Some("my db"));
        assert!(ConnectionProfile::from_url("p", "postgres://h/%zz").is_err());
    }

    #[test]
    fn rejects_passwords_without_echoing_them() {
        for url in ["postgresql://u:secret@h/db", "mysql://h/db?password=secret"] {
            let err = ConnectionProfile::from_url("p", url).unwrap_err();
            let text = err.to_string();
            assert!(matches!(err, ProfileError::PasswordInUrl), "{url}");
            assert!(text.contains("keychain"));
            assert!(!text.contains("secret"));
        }
    }

    #[test]
    fn urls_round_trip() {
        for url in [
            "postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=disable",
            "postgresql://us%40er@[::1]:5433/my%20db?sslmode=verify-full",
            "mysql://root@localhost/inventory",
            "mysql://root@db.example:3307/inv?ssl-mode=REQUIRED",
            "sqlite:/Users/me/My%20Data/inv.sqlite",
        ] {
            let p = ConnectionProfile::from_url("x", url).unwrap();
            assert_eq!(p.to_url(), url);
            assert_eq!(ConnectionProfile::from_url("x", &p.to_url()).unwrap(), p);
        }
    }

    #[test]
    fn save_and_load_keep_fields_atomically() {
        let dir = TempDir::new("roundtrip");
        let mut p = pg();
        p.read_only = true;
        p.secret = Some(SecretRef {
            service: "idecraft".into(),
            account: "ref-pg-local".into(),
        });
        let s = ConnectionProfile::from_url("ref-sqlite", "sqlite:/tmp/inv.sqlite").unwrap();
        save_profiles(&dir.0, &[p.clone(), s.clone()]).unwrap();

        assert_eq!(load_profiles(&dir.0).unwrap(), [p, s]);
        let text = fs::read_to_string(dir.0.join(PROFILE_FILE)).unwrap();
        assert!(!text.contains("password"));
        let leftovers: Vec<_> = fs::read_dir(dir.0.join(".idecraft"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["connections.toml"]);
    }

    #[test]
    fn missing_file_is_empty_and_damaged_file_is_kept() {
        let dir = TempDir::new("damaged");
        assert!(load_profiles(&dir.0).unwrap().is_empty());

        let path = dir.0.join(PROFILE_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[[connection]\nname = ").unwrap();
        let err = load_profiles(&dir.0).unwrap_err();
        assert!(err.to_string().contains("connections.toml"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[[connection]\nname = ");

        // A stored password field is refused, not silently kept.
        fs::write(
            &path,
            "[[connection]]\nid = \"a\"\nname = \"a\"\ndialect = \"postgresql\"\nhost = \"h\"\npassword = \"x\"\n",
        )
        .unwrap();
        assert!(load_profiles(&dir.0).is_err());
    }

    #[test]
    fn validation_names_profile_and_field() {
        let field_of = |result: Result<(), ProfileError>| match result {
            Err(ProfileError::Invalid { name, field, .. }) => (name, field),
            other => panic!("expected invalid profile, got {other:?}"),
        };

        let mut no_host = pg();
        no_host.host = None;
        assert_eq!(
            field_of(no_host.validate()),
            ("ref-pg-local".into(), "host")
        );

        let mut port_zero = pg();
        port_zero.port = Some(0);
        assert_eq!(field_of(port_zero.validate()).1, "port");

        let mut no_path = ConnectionProfile::from_url("lite", "sqlite:/x").unwrap();
        no_path.path = Some(PathBuf::new());
        assert_eq!(field_of(no_path.validate()), ("lite".into(), "path"));

        let mut twin = pg();
        twin.id = "other".into();
        assert_eq!(
            field_of(validate_all(&[pg(), twin])),
            ("ref-pg-local".into(), "name")
        );
    }

    /// SPEC-0108 v4 AK-10: tunnel, TLS files and options survive saving; a profile without them
    /// writes none of their keys.
    #[test]
    fn tunnel_tls_and_options_round_trip() {
        let dir = TempDir::new("v4");
        let plain = pg();
        let mut p = pg();
        p.id = "tunnelled".into();
        p.name = "tunnelled".into();
        p.ssl = SslMode::VerifyCa;
        p.ssh = Some(SshTunnel {
            host: "bastion".into(),
            port: Some(2222),
            user: Some("ops".into()),
        });
        p.tls = TlsFiles {
            ca_file: Some("~/certs/ca.pem".into()),
            cert_file: Some("~/certs/client.crt".into()),
            key_file: Some("~/certs/client.key".into()),
        };
        p.options = ConnectionOptions {
            auto_commit: false,
            startup_script: Some("set search_path = app".into()),
            connect_timeout_secs: Some(5),
            query_timeout_secs: Some(30),
            keep_alive_secs: Some(60),
            schemas: vec!["app*".into()],
            startup_allowed: true,
            base: None,
        };
        p.color = Some(ConnectionColor::Red);
        p.comment = Some("production replica".into());
        save_profiles(&dir.0, &[plain.clone(), p.clone()]).unwrap();
        assert_eq!(load_profiles(&dir.0).unwrap(), [plain, p.clone()]);
        let text = fs::read_to_string(dir.0.join(PROFILE_FILE)).unwrap();
        let first = text.split("[[connection]]").nth(1).unwrap();
        for key in ["ssh", "tls", "options", "color", "comment", "auto_commit"] {
            assert!(!first.contains(key), "{key} written for a plain profile");
        }
        assert!(text.contains("ssl = \"verify-ca\""));
        // Whether a startup script may run follows the project's trust and is never written.
        assert!(!text.contains("startup_allowed"));
        let blocked = ConnectionOptions {
            startup_allowed: false,
            ..ConnectionOptions::default()
        };
        assert!(blocked.is_default());
        assert_eq!(
            p.to_url(),
            "postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=verify-ca"
        );
    }

    /// SPEC-0108 v4 AK-10: what is refused, naming the field.
    #[test]
    fn tunnel_tls_and_options_are_checked() {
        let field = |p: &ConnectionProfile| match p.validate() {
            Err(ProfileError::Invalid { field, .. }) => field,
            other => panic!("expected a refusal, got {other:?}"),
        };
        let tunnel = |host: &str| {
            let mut p = pg();
            p.ssh = Some(SshTunnel {
                host: host.into(),
                port: None,
                user: None,
            });
            p
        };
        assert!(tunnel("bastion.example").validate().is_ok());
        assert!(tunnel("ops@bastion").validate().is_ok());
        // Nothing that ssh could read as an option or a second word.
        assert_eq!(field(&tunnel("-oProxyCommand=sh")), "ssh host");
        assert_eq!(field(&tunnel("a b")), "ssh host");
        assert_eq!(field(&tunnel("")), "ssh host");
        let mut lite = ConnectionProfile::from_url("l", "sqlite:/tmp/x").unwrap();
        lite.ssh = tunnel("h").ssh;
        assert_eq!(field(&lite), "ssh");

        let mut ca = pg();
        ca.tls.ca_file = Some("/ca.pem".into());
        ca.ssl = SslMode::Require;
        assert_eq!(field(&ca), "ca file");
        ca.ssl = SslMode::VerifyFull;
        assert!(ca.validate().is_ok());
        ca.ssl = SslMode::Disable;
        assert_eq!(field(&ca), "tls");

        let mut cert = pg();
        cert.ssl = SslMode::VerifyFull;
        cert.tls.cert_file = Some("/c.pem".into());
        assert_eq!(field(&cert), "key file");
        cert.tls.cert_file = Some("/c.p12".into());
        assert!(cert.validate().is_ok());
        cert.tls.key_file = Some("/k.pem".into());
        assert_eq!(field(&cert), "key file");
        let mut my = ConnectionProfile::from_url("m", "mysql://u@h/db?ssl-mode=VERIFY_CA").unwrap();
        assert_eq!(my.ssl, SslMode::VerifyCa);
        my.tls.cert_file = Some("/c.pem".into());
        my.tls.key_file = Some("/k.pem".into());
        assert_eq!(field(&my), "cert file");
        // SPEC-0116 v6 AK-10: SQL Server takes a CA file, checked as for the others, and no
        // client certificate.
        let mut ms =
            ConnectionProfile::from_url("s", "sqlserver://u@h/db?sslmode=verify-full").unwrap();
        ms.tls.ca_file = Some("/ca.pem".into());
        assert!(ms.validate().is_ok());
        ms.ssl = SslMode::Require;
        assert_eq!(field(&ms), "ca file");
        ms.ssl = SslMode::VerifyFull;
        ms.tls.cert_file = Some("/c.p12".into());
        assert_eq!(field(&ms), "cert file");
        ms.tls.cert_file = None;
        ms.tls.key_file = Some("/k.pem".into());
        assert_eq!(field(&ms), "key file");
        let mut ora = ConnectionProfile::from_url("o", "oracle://u@h:1521/X").unwrap();
        ora.ssl = SslMode::VerifyFull;
        ora.tls.ca_file = Some("/ca.pem".into());
        assert_eq!(field(&ora), "tls");

        let mut slow = pg();
        slow.options.query_timeout_secs = Some(0);
        assert_eq!(field(&slow), "query timeout");
        slow.options.query_timeout_secs = Some(30);
        slow.options.schemas = vec![" ".into()];
        assert_eq!(field(&slow), "schemas");
    }

    #[test]
    fn schema_filter_and_colours() {
        let mut o = ConnectionOptions::default();
        assert!(o.shows_schema("anything"));
        o.schemas = vec!["app*".into(), "audit".into(), "*_v*_x".into()];
        assert!(o.shows_schema("app"));
        assert!(o.shows_schema("App_Data"));
        assert!(o.shows_schema("AUDIT"));
        assert!(o.shows_schema("a_v2_x"));
        assert!(!o.shows_schema("audit_log"));
        assert!(!o.shows_schema("public"));
        assert_eq!(o.connect_timeout(), std::time::Duration::from_secs(10));

        assert_eq!(ConnectionColor::parse(" Red "), Some(ConnectionColor::Red));
        assert_eq!(ConnectionColor::parse("pink"), None);
        assert_eq!(ConnectionColor::next(None), Some(ConnectionColor::Red));
        assert_eq!(ConnectionColor::next(Some(ConnectionColor::Gray)), None);
        assert_eq!(expand_home(Path::new("/abs")), PathBuf::from("/abs"));
    }
}
