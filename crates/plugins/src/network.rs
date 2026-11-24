//! Network for connector plugins (`SPEC-0314`, ADR-0024): a plugin with the permission
//! `profile-network` may open TCP connections to the host and port of the connection the user
//! picked, and nowhere else, after a grant per plugin and host. Every connection is written to a
//! local audit log that never leaves the machine. v2: the connection's SSL mode decides whether a
//! connection without TLS is allowed; elsewhere the host makes the TLS itself (`SPEC-0314` v2
//! behaviour 10, `SPEC-0016` v4 behaviour 14).

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Manifest, Permission, Plugin, PluginError};

/// File in the plugins folder with the network grants.
pub const NETWORK_GRANTS: &str = "network-grants.json";
/// File in the plugins folder with the audit log of plugin connections.
pub const NETWORK_AUDIT: &str = "network-audit.jsonl";
/// Most connections one call may hold open.
pub const MAX_CONNECTIONS: usize = 4;
/// Most bytes one `net_recv` returns.
pub const MAX_RECV: usize = 1 << 20;
/// Time limit of a connection attempt.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Time limit of one read or write.
pub const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// `net_*` answer: the call has no network access.
pub const NET_NO_ACCESS: i32 = -1;
/// `net_connect` answer: not the granted host and port.
pub const NET_REFUSED: i32 = -2;
/// `net_*` answer: resolving, connecting, reading or writing failed.
pub const NET_FAILED: i32 = -3;
/// `net_connect` answer: too many open connections.
pub const NET_TOO_MANY: i32 = -4;
/// `net_*` answer: a bad argument or handle.
pub const NET_BAD: i32 = -5;
/// `net_connect` answer: the connection's SSL mode needs TLS for this host; `net_connect_tls`
/// makes it (`SPEC-0314` v2 behaviour 10).
pub const NET_NEEDS_TLS: i32 = -6;
/// `net_connect_tls` answer: the TLS handshake failed, or the CA file could not be read.
pub const NET_TLS_FAILED: i32 = -7;

/// The SSL mode of the connection a plugin call serves, as profiles write it (`SPEC-0016`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetMode {
    /// No TLS.
    Disable,
    /// TLS when possible; without it only on the loopback.
    Prefer,
    /// TLS without certificate checks.
    Require,
    /// TLS; the certificate's chain is checked, not the host name.
    VerifyCa,
    /// TLS; certificate chain and host name are checked. The default, the strictest.
    #[default]
    VerifyFull,
}

/// What the connection's SSL mode asks of a plugin's connections (`SPEC-0314` v2 behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetTls {
    /// The profile's SSL mode.
    pub mode: NetMode,
    /// The profile's CA file (PEM), which replaces the system's trusted roots.
    pub ca_file: Option<PathBuf>,
}

/// Whether `host` (as [`Target`] keeps it) is this machine's loopback: `localhost`, an address in
/// 127.0.0.0/8 or `::1` (`SPEC-0016` v3 behaviour 3).
pub fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

impl NetTls {
    /// Whether `net_connect`, without TLS, may reach `target`: `disable` on any host, `prefer` on
    /// the loopback, nothing else.
    pub fn allows_plain(&self, target: &Target) -> bool {
        match self.mode {
            NetMode::Disable => true,
            NetMode::Prefer => is_loopback(&target.host),
            NetMode::Require | NetMode::VerifyCa | NetMode::VerifyFull => false,
        }
    }

    /// The TLS settings of `net_connect_tls`: TLS 1.2 at least, the mode's checks, the CA file
    /// instead of the system's roots.
    fn connector(&self) -> Result<native_tls::TlsConnector, String> {
        let mut builder = native_tls::TlsConnector::builder();
        builder.min_protocol_version(Some(native_tls::Protocol::Tlsv12));
        match self.mode {
            NetMode::VerifyFull => {}
            NetMode::VerifyCa => {
                builder.danger_accept_invalid_hostnames(true);
            }
            NetMode::Disable | NetMode::Prefer | NetMode::Require => {
                builder
                    .danger_accept_invalid_certs(true)
                    .danger_accept_invalid_hostnames(true);
            }
        }
        if let Some(file) = &self.ca_file {
            let shown = file.display();
            let text = std::fs::read_to_string(file).map_err(|e| format!("{shown}: {e}"))?;
            let mut found = false;
            for block in text
                .split_inclusive("-----END CERTIFICATE-----")
                .filter(|b| b.contains("-----BEGIN CERTIFICATE-----"))
            {
                let cert = native_tls::Certificate::from_pem(block.trim().as_bytes())
                    .map_err(|e| format!("{shown}: {e}"))?;
                builder.add_root_certificate(cert);
                found = true;
            }
            if !found {
                return Err(format!("{shown}: no PEM certificate found"));
            }
            builder.disable_built_in_roots(true);
        }
        builder.build().map_err(|e| e.to_string())
    }
}

/// A host and port, the host lowercased, without a trailing dot or IPv6 brackets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Target {
    /// Host name or address.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

impl Target {
    /// The target `host`:`port`, normalised.
    pub fn new(host: &str, port: u16) -> Self {
        let host = host.trim();
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        Self {
            host: host.trim_end_matches('.').to_ascii_lowercase(),
            port,
        }
    }

    /// Reads `host:port` or `[v6]:port`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (host, port) = if let Some(rest) = text.strip_prefix('[') {
            let (host, port) = rest.split_once("]:")?;
            (host, port)
        } else {
            let (host, port) = text.rsplit_once(':')?;
            if host.contains(':') {
                return None;
            }
            (host, port)
        };
        let port: u16 = port.parse().ok()?;
        if host.is_empty() || port == 0 || host.chars().any(|c| c.is_control() || c == ' ') {
            return None;
        }
        Some(Self::new(host, port))
    }

    /// `host:port`, IPv6 addresses in brackets.
    pub fn label(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// Who a grant is for: the person at the window or agents over MCP (ADR-0023 rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Holder {
    /// The user in the Ferris IDEA window.
    User,
    /// Agents over MCP.
    Agent,
}

impl Holder {
    /// "you" or "agents", as Settings › Plugins says it.
    pub fn label(self) -> &'static str {
        match self {
            Holder::User => "you",
            Holder::Agent => "agents",
        }
    }
}

/// One network grant (`SPEC-0314` behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkGrant {
    /// Plugin file stem.
    pub plugin: String,
    /// Host as in the profile, normalised.
    pub host: String,
    /// Port.
    pub port: u16,
    /// For whom.
    pub holder: Holder,
    /// When, RFC 3339 UTC.
    pub granted: String,
}

impl NetworkGrant {
    /// The granted target.
    pub fn target(&self) -> Target {
        Target::new(&self.host, self.port)
    }
}

/// Every network grant in `dir`.
pub fn network_grants(dir: &Path) -> Vec<NetworkGrant> {
    std::fs::read_to_string(dir.join(NETWORK_GRANTS))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_grants(dir: &Path, grants: &[NetworkGrant]) -> Result<(), PluginError> {
    let io = |e: std::io::Error| PluginError::Io(NETWORK_GRANTS.into(), e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let text = serde_json::to_string_pretty(grants)
        .map_err(|e| PluginError::Io(NETWORK_GRANTS.into(), e.to_string()))?;
    let part = dir.join(format!("{NETWORK_GRANTS}.part"));
    std::fs::write(&part, text).map_err(io)?;
    std::fs::rename(&part, dir.join(NETWORK_GRANTS)).map_err(io)
}

/// Whether `holder` may let plugin `plugin` of `dir` connect to `target`.
pub fn network_granted(dir: &Path, plugin: &str, target: &Target, holder: Holder) -> bool {
    network_grants(dir)
        .iter()
        .any(|g| g.plugin == plugin && g.holder == holder && &g.target() == target)
}

/// Grants `holder` network access of plugin `plugin` to `target`; an older grant of the same
/// plugin, target and holder is replaced.
pub fn grant_network(
    dir: &Path,
    plugin: &str,
    target: &Target,
    holder: Holder,
) -> Result<NetworkGrant, PluginError> {
    let mut all = network_grants(dir);
    all.retain(|g| !(g.plugin == plugin && g.holder == holder && &g.target() == target));
    let grant = NetworkGrant {
        plugin: plugin.to_owned(),
        host: target.host.clone(),
        port: target.port,
        holder,
        granted: now_rfc3339(),
    };
    all.push(grant.clone());
    save_grants(dir, &all)?;
    Ok(grant)
}

/// Removes the grants of plugin `plugin`, only those for `target` and `holder` when given. How
/// many were removed.
pub fn revoke_network(
    dir: &Path,
    plugin: &str,
    target: Option<&Target>,
    holder: Option<Holder>,
) -> Result<usize, PluginError> {
    let mut all = network_grants(dir);
    let before = all.len();
    all.retain(|g| {
        !(g.plugin == plugin
            && target.is_none_or(|t| &g.target() == t)
            && holder.is_none_or(|h| g.holder == h))
    });
    let removed = before - all.len();
    if removed > 0 {
        save_grants(dir, &all)?;
    }
    Ok(removed)
}

/// What one call may reach (`SPEC-0314` behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetAccess {
    /// Plugin file stem, as the audit names it.
    pub plugin: String,
    /// The only host and port.
    pub target: Target,
    /// Whose grant this call uses.
    pub holder: Holder,
    /// Audit file; none writes no audit (never used by Ferris IDEA itself).
    pub audit: Option<PathBuf>,
    /// What the connection's SSL mode allows (`SPEC-0314` v2 behaviour 10).
    pub tls: NetTls,
}

/// One line of the audit log (`SPEC-0314` behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// RFC 3339 UTC.
    pub time: String,
    /// Plugin file stem.
    pub plugin: String,
    /// Whose grant.
    pub holder: Holder,
    /// What the plugin asked for (`host:port`).
    pub target: String,
    /// The address connected to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// `opened`, `refused`, `failed` or `closed`.
    pub outcome: String,
    /// Bytes sent (on `closed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<u64>,
    /// Bytes received (on `closed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received: Option<u64>,
    /// Whether the host made TLS for the connection (on `opened` and `closed`, `SPEC-0314` v2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<bool>,
    /// Why a connection was refused for want of TLS or its TLS failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The last `limit` audit records of `dir`, oldest first; only plugin `plugin`'s when given.
pub fn audit_records(dir: &Path, plugin: Option<&str>, limit: usize) -> Vec<AuditRecord> {
    let text = std::fs::read_to_string(dir.join(NETWORK_AUDIT)).unwrap_or_default();
    let all: Vec<AuditRecord> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<AuditRecord>(l).ok())
        .filter(|r| plugin.is_none_or(|p| r.plugin == p))
        .collect();
    let skip = all.len().saturating_sub(limit);
    all.into_iter().skip(skip).collect()
}

fn append(file: &Path, record: &AuditRecord) {
    let Ok(line) = serde_json::to_string(record) else {
        return;
    };
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// A plugin's socket: plain, or inside TLS the host made.
enum Stream {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.write_all(bytes).and_then(|()| s.flush()),
            Stream::Tls(s) => s.write_all(bytes).and_then(|()| s.flush()),
        }
    }

    fn shutdown(&mut self) {
        match self {
            Stream::Plain(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            Stream::Tls(s) => {
                let _ = s.shutdown();
                let _ = s.get_ref().shutdown(std::net::Shutdown::Both);
            }
        }
    }

    fn tls(&self) -> bool {
        matches!(self, Stream::Tls(_))
    }
}

struct Connection {
    stream: Stream,
    address: SocketAddr,
    target: String,
    sent: u64,
    received: u64,
}

/// What one audit line says besides plugin, holder and time.
#[derive(Default)]
struct Entry<'a> {
    address: Option<SocketAddr>,
    bytes: Option<(u64, u64)>,
    tls: Option<bool>,
    error: Option<&'a str>,
}

/// The sockets of one call; whatever is still open closes (and is logged) when the call ends.
#[derive(Default)]
pub(crate) struct Sockets {
    access: Option<NetAccess>,
    open: Vec<Option<Connection>>,
}

impl Sockets {
    pub(crate) fn new(access: Option<NetAccess>) -> Self {
        Self {
            access,
            open: Vec::new(),
        }
    }

    fn log(&self, target: &str, outcome: &str, entry: Entry<'_>) {
        let Some(access) = &self.access else {
            return;
        };
        let Some(file) = &access.audit else {
            return;
        };
        append(
            file,
            &AuditRecord {
                time: now_rfc3339(),
                plugin: access.plugin.clone(),
                holder: access.holder,
                target: target.to_owned(),
                address: entry.address.map(|a| a.to_string()),
                outcome: outcome.to_owned(),
                sent: entry.bytes.map(|b| b.0),
                received: entry.bytes.map(|b| b.1),
                tls: entry.tls,
                error: entry.error.map(str::to_owned),
            },
        );
    }

    /// `net_connect` (`SPEC-0314` behaviour 3): no TLS, where the connection's SSL mode allows it.
    pub(crate) fn connect(&mut self, text: &str) -> i32 {
        self.open_to(text, false)
    }

    /// `net_connect_tls` (`SPEC-0314` v2 behaviours 3 and 10): TLS the host makes.
    pub(crate) fn connect_tls(&mut self, text: &str) -> i32 {
        self.open_to(text, true)
    }

    fn open_to(&mut self, text: &str, tls: bool) -> i32 {
        let Some(access) = self.access.clone() else {
            return NET_NO_ACCESS;
        };
        let shown: String = text.chars().take(300).collect();
        let Some(asked) = Target::parse(text) else {
            self.log(&shown, "refused", Entry::default());
            return NET_BAD;
        };
        if asked != access.target {
            // Nothing is resolved for a target that is not the profile's.
            self.log(&asked.label(), "refused", Entry::default());
            return NET_REFUSED;
        }
        if !tls && !access.tls.allows_plain(&access.target) {
            // Nor for a connection without TLS the mode does not allow (behaviour 10).
            let error = Some("needs TLS");
            self.log(
                &asked.label(),
                "refused",
                Entry {
                    error,
                    ..Entry::default()
                },
            );
            return NET_NEEDS_TLS;
        }
        if self.open.iter().flatten().count() >= MAX_CONNECTIONS {
            self.log(&asked.label(), "refused", Entry::default());
            return NET_TOO_MANY;
        }
        let connector = if tls {
            match access.tls.connector() {
                Ok(c) => Some(c),
                Err(e) => {
                    let error = Some(e.as_str());
                    self.log(
                        &asked.label(),
                        "failed",
                        Entry {
                            error,
                            ..Entry::default()
                        },
                    );
                    return NET_TLS_FAILED;
                }
            }
        } else {
            None
        };
        let addresses: Vec<SocketAddr> = (access.target.host.as_str(), access.target.port)
            .to_socket_addrs()
            .map(Iterator::collect)
            .unwrap_or_default();
        for address in addresses {
            if let Ok(stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_nodelay(true);
                let stream = match &connector {
                    None => Stream::Plain(stream),
                    Some(c) => match c.connect(&access.target.host, stream) {
                        Ok(s) => Stream::Tls(Box::new(s)),
                        Err(e) => {
                            let error = match e {
                                native_tls::HandshakeError::Failure(e) => e.to_string(),
                                native_tls::HandshakeError::WouldBlock(_) => {
                                    "the TLS handshake did not finish".into()
                                }
                            };
                            let entry = Entry {
                                address: Some(address),
                                tls: Some(true),
                                error: Some(&error),
                                ..Entry::default()
                            };
                            self.log(&asked.label(), "failed", entry);
                            return NET_TLS_FAILED;
                        }
                    },
                };
                let entry = Entry {
                    address: Some(address),
                    tls: Some(stream.tls()),
                    ..Entry::default()
                };
                self.log(&asked.label(), "opened", entry);
                let conn = Connection {
                    stream,
                    address,
                    target: asked.label(),
                    sent: 0,
                    received: 0,
                };
                let slot = self.open.iter().position(Option::is_none);
                let handle = match slot {
                    Some(i) => {
                        self.open[i] = Some(conn);
                        i
                    }
                    None => {
                        self.open.push(Some(conn));
                        self.open.len() - 1
                    }
                };
                return i32::try_from(handle).unwrap_or(NET_BAD);
            }
        }
        self.log(&asked.label(), "failed", Entry::default());
        NET_FAILED
    }

    fn conn(&mut self, handle: i32) -> Option<&mut Connection> {
        let i = usize::try_from(handle).ok()?;
        self.open.get_mut(i)?.as_mut()
    }

    /// `net_send`.
    pub(crate) fn send(&mut self, handle: i32, bytes: &[u8]) -> i32 {
        let Some(c) = self.conn(handle) else {
            return NET_BAD;
        };
        match c.stream.write_all(bytes) {
            Ok(()) => {
                c.sent += bytes.len() as u64;
                i32::try_from(bytes.len()).unwrap_or(NET_BAD)
            }
            Err(_) => NET_FAILED,
        }
    }

    /// `net_recv`: up to `max` bytes, empty when the server closed.
    pub(crate) fn recv(&mut self, handle: i32, max: i32) -> Result<Vec<u8>, i32> {
        let Ok(max) = usize::try_from(max) else {
            return Err(NET_BAD);
        };
        let Some(c) = self.conn(handle) else {
            return Err(NET_BAD);
        };
        let mut buf = vec![0; max.min(MAX_RECV)];
        match c.stream.read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                c.received += n as u64;
                Ok(buf)
            }
            Err(_) => Err(NET_FAILED),
        }
    }

    /// `net_close`.
    pub(crate) fn close(&mut self, handle: i32) -> i32 {
        let Some(i) = usize::try_from(handle).ok() else {
            return NET_BAD;
        };
        let Some(c) = self.open.get_mut(i).and_then(Option::take) else {
            return NET_BAD;
        };
        let mut c = c;
        c.stream.shutdown();
        let entry = Entry {
            address: Some(c.address),
            bytes: Some((c.sent, c.received)),
            tls: Some(c.stream.tls()),
            error: None,
        };
        self.log(&c.target, "closed", entry);
        0
    }
}

impl Drop for Sockets {
    fn drop(&mut self) {
        for i in 0..self.open.len() {
            if self.open[i].is_some() {
                let _ = self.close(i32::try_from(i).unwrap_or(i32::MAX));
            }
        }
    }
}

/// The warning before a plugin's first connection (`SPEC-0314` behaviour 6), for the window
/// and, as data, for agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetworkWarning {
    /// Plugin name from its manifest (the file stem when it does not load).
    pub plugin: String,
    /// Plugin file stem.
    pub file: String,
    /// Version from the manifest.
    pub version: String,
    /// Author from the manifest, or "unknown author".
    pub author: String,
    /// Connector display name.
    pub connector: String,
    /// `beta`, `user` or `imported`.
    pub status: String,
    /// Target host, if a connection is picked.
    pub host: Option<String>,
    /// Target port, if a connection is picked.
    pub port: Option<u16>,
    /// Always false: Ferris IDEA does not review plugin code.
    pub reviewed: bool,
    /// Always true: the plugin receives the connection's user and password.
    pub receives_credentials: bool,
    /// Dialog title.
    pub title: String,
    /// The lines under the title.
    pub lines: Vec<String>,
    /// Title and lines as one text.
    pub text: String,
}

/// What the status of a plugin connector means, for the warning.
pub fn status_meaning(status: &str) -> String {
    match status {
        "beta" => "Beta: built into Ferris IDEA, but not tested against a real server".into(),
        "user" => {
            "User: a definition from your connectors folder; Ferris IDEA makes no statement about it"
                .into()
        }
        "imported" => {
            "Imported: brought in from a file or link; Ferris IDEA makes no statement about it".into()
        }
        "supported" => "Supported: tested by Ferris IDEA against a real server".into(),
        other => other.to_owned(),
    }
}

/// Builds the warning for plugin `file` (its `manifest` if it loads), connector `connector` with
/// `status` (`beta`, `user`, `imported`) and the connection's `target`, if known.
pub fn network_warning(
    manifest: Option<&Manifest>,
    file: &str,
    connector: &str,
    status: &str,
    target: Option<&Target>,
) -> NetworkWarning {
    let plugin = manifest.map_or_else(|| file.to_owned(), |m| m.name.clone());
    let version = manifest.map(|m| m.version.clone()).unwrap_or_default();
    let author = manifest
        .map(|m| m.author.trim().to_owned())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "unknown author".into());
    let title = format!("Allow network access for plugin “{plugin}”?");
    let who = if version.is_empty() {
        format!("Plugin “{plugin}” by {author}")
    } else {
        format!("Plugin “{plugin}” {version} by {author}")
    };
    let place = target.map_or_else(
        || "the host and port of the connection you pick".to_owned(),
        Target::label,
    );
    let lines = vec![
        format!("{who} wants to connect to {place}."),
        format!(
            "It gets network access to {} only, and receives the connection's user name and password.",
            if target.is_some() {
                "this host and port"
            } else {
                "that host and port"
            }
        ),
        format!("Its connector “{connector}” is {}.", status_meaning(status)),
        "Ferris IDEA does not review the code of plugins. Allow it only if you trust the author."
            .into(),
    ];
    let text = std::iter::once(title.clone())
        .chain(lines.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");
    NetworkWarning {
        plugin,
        file: file.to_owned(),
        version,
        author,
        connector: connector.to_owned(),
        status: status.to_owned(),
        host: target.map(|t| t.host.clone()),
        port: target.map(|t| t.port),
        reviewed: false,
        receives_credentials: true,
        title,
        lines,
        text,
    }
}

/// What a `connector` action gets (`SPEC-0314` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ConnectorRequest {
    /// `test`.
    pub op: String,
    /// Profile host.
    pub host: String,
    /// Profile port.
    pub port: u16,
    /// Database, if any.
    pub database: Option<String>,
    /// User, if any.
    pub user: Option<String>,
    /// Password, if stored or typed.
    pub password: Option<String>,
    /// The profile's SSL mode (`SPEC-0314` v2 behaviour 4); the host keeps the plugin to it.
    pub ssl: NetMode,
    /// Whether `net_connect`, without TLS, is allowed for this connection; the host sets it.
    pub plain: bool,
    /// The profile's CA file; read by the host, never shown to the plugin.
    #[serde(skip)]
    pub ca_file: Option<PathBuf>,
}

/// What a `connector` action answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorAnswer {
    /// Whether it worked.
    pub ok: bool,
    /// What the user reads on success.
    #[serde(default)]
    pub note: Option<String>,
    /// Why not.
    #[serde(default)]
    pub error: Option<String>,
}

impl Plugin {
    /// The plugin's first `connector` action, if any.
    pub fn connector_action(&self) -> Option<&crate::ActionInfo> {
        self.manifest
            .actions
            .iter()
            .find(|a| a.kind == crate::ActionKind::Connector)
    }

    /// Runs the `connector` action with `request` and network access only to `access.target`
    /// (`SPEC-0314` behaviour 4). The caller has checked the grants.
    pub fn run_connector(
        &self,
        request: &ConnectorRequest,
        access: NetAccess,
    ) -> Result<ConnectorAnswer, PluginError> {
        let name = self.manifest.name.clone();
        if !self
            .manifest
            .permissions
            .contains(&Permission::ProfileNetwork)
        {
            return Err(PluginError::Failed(
                name,
                "it does not ask for `profile-network`".into(),
            ));
        }
        let Some(action) = self.connector_action() else {
            return Err(PluginError::Failed(
                name,
                "it has no connector action".into(),
            ));
        };
        let (request, access) = prepared(request, access);
        let input = serde_json::to_string(&request)
            .map_err(|e| PluginError::Failed(name.clone(), e.to_string()))?;
        let output = self.run_in(
            &action.id.clone(),
            &input,
            crate::FUEL,
            crate::Access {
                network: Some(access),
                ..crate::Access::default()
            },
        )?;
        serde_json::from_str(&output)
            .map_err(|e| PluginError::Failed(name, format!("its answer: {e}")))
    }
}

/// The request as the plugin reads it and the access the host keeps it to: the granted target,
/// the connection's SSL mode and CA file with the grant, and whether a connection without TLS is
/// allowed (`SPEC-0314` v2 behaviours 4 and 10).
fn prepared(request: &ConnectorRequest, mut access: NetAccess) -> (ConnectorRequest, NetAccess) {
    let mut request = request.clone();
    request.host.clone_from(&access.target.host);
    request.port = access.target.port;
    access.tls = NetTls {
        mode: request.ssl,
        ca_file: request.ca_file.clone(),
    };
    request.plain = access.tls.allows_plain(&access.target);
    (request, access)
}

/// Where a connection through a plugin stands before it is made (`SPEC-0314` behaviours 5–8).
#[derive(Debug)]
pub enum Readiness {
    /// The module is not installed or enabled, or does not load.
    Missing(String),
    /// The plugin does not ask for `profile-network` or has no connector action, or the user has
    /// not granted the permission.
    NotPermitted(String, NetworkWarning),
    /// A grant for this holder and target is missing.
    NeedsGrant(NetworkWarning),
    /// Everything is in place.
    Ready(Box<Plugin>, NetAccess, NetworkWarning),
}

/// Checks plugin `module` of `dir` for a connection of `holder` to `target`; `extra` counts as a
/// grant (an agent's "allow once" in this MCP session).
pub fn readiness(
    dir: &Path,
    module: &str,
    connector: &str,
    status: &str,
    target: &Target,
    holder: Holder,
    extra: bool,
) -> Readiness {
    let path = dir.join(format!("{module}.wasm"));
    let plugin = match Plugin::load(&path) {
        Ok(p) => p,
        Err(e) => {
            return Readiness::Missing(if path.exists() {
                e.to_string()
            } else {
                format!("plugin `{module}` is not installed or not enabled")
            });
        }
    };
    let warning = network_warning(
        Some(&plugin.manifest),
        module,
        connector,
        status,
        Some(target),
    );
    if !plugin
        .manifest
        .permissions
        .contains(&Permission::ProfileNetwork)
        || plugin.connector_action().is_none()
    {
        return Readiness::NotPermitted(
            format!(
                "plugin `{module}` has no connector action with the permission `profile-network`"
            ),
            warning,
        );
    }
    if !crate::granted(dir, module, Permission::ProfileNetwork) {
        return Readiness::NotPermitted(
            format!(
                "the permission `profile-network` of plugin `{module}` is not granted (Settings › Plugins)"
            ),
            warning,
        );
    }
    if !extra && !network_granted(dir, module, target, holder) {
        return Readiness::NeedsGrant(warning);
    }
    let access = NetAccess {
        plugin: module.to_owned(),
        target: target.clone(),
        holder,
        audit: Some(dir.join(NETWORK_AUDIT)),
        // The strictest until `run_connector` takes the connection's mode.
        tls: NetTls::default(),
    };
    Readiness::Ready(Box::new(plugin), access, warning)
}

/// Now as RFC 3339 UTC, to the second.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs)
}

/// `secs` since the Unix epoch as RFC 3339 UTC.
fn rfc3339(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ferris-idea-net-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn targets_parse_and_normalise() {
        assert_eq!(
            Target::parse("DB.Example.com.:5432"),
            Some(Target::new("db.example.com", 5432))
        );
        assert_eq!(
            Target::parse("[::1]:26257"),
            Some(Target::new("::1", 26257))
        );
        assert_eq!(Target::new("[::1]", 1).label(), "[::1]:1");
        assert_eq!(Target::parse("::1:80"), None);
        assert_eq!(Target::parse("host:0"), None);
        assert_eq!(Target::parse("host"), None);
        assert_eq!(Target::parse("a b:1"), None);
        assert_ne!(Target::new("localhost", 1), Target::new("127.0.0.1", 1));
    }

    #[test]
    fn dates_are_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_793_884_581), "2026-11-05T13:16:21Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn grants_per_plugin_host_and_holder_are_revocable() {
        // SPEC-0314 AK-1.
        let dir = temp("grants");
        let db = Target::new("db.example.com", 5432);
        let other = Target::new("db.example.com", 5433);
        assert!(!network_granted(&dir, "mydb", &db, Holder::User));
        grant_network(&dir, "mydb", &db, Holder::User).unwrap();
        assert!(network_granted(&dir, "mydb", &db, Holder::User));
        assert!(network_granted(
            &dir,
            "mydb",
            &Target::new("DB.example.com.", 5432),
            Holder::User
        ));
        // A user's grant is not an agent's, another port is another grant, another plugin too.
        assert!(!network_granted(&dir, "mydb", &db, Holder::Agent));
        assert!(!network_granted(&dir, "mydb", &other, Holder::User));
        assert!(!network_granted(&dir, "otherdb", &db, Holder::User));
        // Granting again replaces, never duplicates.
        grant_network(&dir, "mydb", &db, Holder::User).unwrap();
        grant_network(&dir, "mydb", &db, Holder::Agent).unwrap();
        grant_network(&dir, "mydb", &other, Holder::User).unwrap();
        grant_network(&dir, "otherdb", &db, Holder::User).unwrap();
        assert_eq!(network_grants(&dir).len(), 4);
        assert!(
            network_grants(&dir)
                .iter()
                .all(|g| g.granted.ends_with('Z'))
        );
        // Revoking one target for one holder.
        assert_eq!(
            revoke_network(&dir, "mydb", Some(&db), Some(Holder::Agent)).unwrap(),
            1
        );
        assert!(!network_granted(&dir, "mydb", &db, Holder::Agent));
        assert!(network_granted(&dir, "mydb", &db, Holder::User));
        // Revoking a plugin takes all of its grants and leaves the others.
        assert_eq!(revoke_network(&dir, "mydb", None, None).unwrap(), 2);
        assert_eq!(network_grants(&dir).len(), 1);
        assert_eq!(network_grants(&dir)[0].plugin, "otherdb");
        // Withdrawing the permission and uninstalling remove grants too.
        crate::set_granted(&dir, "otherdb", Permission::ProfileNetwork, true).unwrap();
        crate::set_granted(&dir, "otherdb", Permission::ProfileNetwork, false).unwrap();
        assert!(network_grants(&dir).is_empty());
        grant_network(&dir, "gone", &db, Holder::Agent).unwrap();
        std::fs::write(dir.join("gone.wasm"), b"x").unwrap();
        crate::uninstall(&dir, "gone").unwrap();
        assert!(network_grants(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A connector plugin whose action connects to the target given as its input, sends `ping`
    /// and answers what came back, or `E<n>` for error `-n`.
    fn probe() -> Vec<u8> {
        probe_through("net_connect")
    }

    /// [`probe`] connecting through the host function `connect`.
    fn probe_through(connect: &str) -> Vec<u8> {
        let manifest = r#"{"name":"Net probe","version":"1.0","author":"Ferris IDEA tests","permissions":["profile-network"],"actions":[{"id":"connect","title":"Connect","kind":"connector"}]}"#;
        let wat = format!(
            r#"(module
  (import "ferris-idea" "{connect}" (func $connect (param i32 i32) (result i32)))
  (import "ferris-idea" "net_send" (func $send (param i32 i32 i32) (result i32)))
  (import "ferris-idea" "net_recv" (func $recv (param i32 i32) (result i64)))
  (import "ferris-idea" "net_close" (func $close (param i32) (result i32)))
  (memory (export "memory") 1)
  (global $next (mut i32) (i32.const 1024))
  (data (i32.const 0) "{data}")
  (data (i32.const 600) "ping")
  (func (export "ferris_idea_alloc") (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $n)))
    (local.get $p))
  (func (export "ferris_idea_manifest") (result i64) (i64.const {len}))
  (func (export "ferris_idea_run") (param i32 i32 i32 i32) (result i64)
    (local $h i32) (local $r i64)
    (local.set $h (call $connect (local.get 2) (local.get 3)))
    (if (i32.lt_s (local.get $h) (i32.const 0))
      (then
        (i32.store8 (i32.const 700) (i32.const 69))
        (i32.store8 (i32.const 701) (i32.sub (i32.const 48) (local.get $h)))
        (return (i64.or (i64.shl (i64.const 700) (i64.const 32)) (i64.const 2)))))
    (drop (call $send (local.get $h) (i32.const 600) (i32.const 4)))
    (local.set $r (call $recv (local.get $h) (i32.const 64)))
    (drop (call $close (local.get $h)))
    (local.get $r)))"#,
            data = manifest.replace('"', "\\\""),
            len = manifest.len()
        );
        wat::parse_str(wat).unwrap()
    }

    /// A server on 127.0.0.1 that answers `pong` to one connection; its port.
    fn pong_server() -> (u16, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0; 4];
            s.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, b"ping");
            s.write_all(b"pong").unwrap();
        });
        (port, t)
    }

    #[test]
    fn a_plugin_reaches_only_the_profiles_host_and_port() {
        // SPEC-0314 AK-2.
        let dir = temp("reach");
        let plugin = Plugin::from_bytes(&dir.join("netprobe.wasm"), &probe()).unwrap();
        assert_eq!(plugin.manifest.author, "Ferris IDEA tests");
        let (port, server) = pong_server();
        let target = Target::new("127.0.0.1", port);
        let access = |holder| crate::Access {
            network: Some(NetAccess {
                plugin: "netprobe".into(),
                target: target.clone(),
                holder,
                audit: Some(dir.join(NETWORK_AUDIT)),
                tls: NetTls {
                    mode: NetMode::Prefer,
                    ca_file: None,
                },
            }),
            ..crate::Access::default()
        };
        let run = |input: &str, a: crate::Access| {
            plugin.run_in("connect", input, crate::FUEL, a).unwrap()
        };
        // Without network access for the call nothing connects.
        assert_eq!(run(&target.label(), crate::Access::default()), "E1");
        // Another port on the same host, the same host by another name, another host.
        let other_port = format!("127.0.0.1:{}", if port == 65535 { 1 } else { port + 1 });
        assert_eq!(run(&other_port, access(Holder::User)), "E2");
        assert_eq!(
            run(&format!("localhost:{port}"), access(Holder::User)),
            "E2"
        );
        assert_eq!(run("example.com:80", access(Holder::User)), "E2");
        assert_eq!(run("not a target", access(Holder::User)), "E5");
        // The profile's own host and port.
        assert_eq!(
            run(&format!("127.0.0.1:{port}"), access(Holder::Agent)),
            "pong"
        );
        server.join().unwrap();
        let log = audit_records(&dir, Some("netprobe"), 100);
        let outcomes: Vec<&str> = log.iter().map(|r| r.outcome.as_str()).collect();
        assert_eq!(
            outcomes,
            [
                "refused", "refused", "refused", "refused", "opened", "closed"
            ]
        );
        assert_eq!(log[1].target, format!("localhost:{port}"));
        assert!(log[0..4].iter().all(|r| r.address.is_none()));
        let closed = &log[5];
        assert_eq!((closed.sent, closed.received), (Some(4), Some(4)));
        assert_eq!(closed.holder, Holder::Agent);
        assert_eq!(
            closed.address.as_deref(),
            Some(format!("127.0.0.1:{port}").as_str())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn readiness_needs_permission_and_a_grant_of_the_holder() {
        let dir = temp("ready");
        let target = Target::new("127.0.0.1", 9);
        let check =
            |holder, extra| readiness(&dir, "netprobe", "Probe", "user", &target, holder, extra);
        assert!(matches!(check(Holder::User, false), Readiness::Missing(_)));
        std::fs::write(dir.join("netprobe.wasm"), probe()).unwrap();
        assert!(matches!(
            check(Holder::User, false),
            Readiness::NotPermitted(..)
        ));
        crate::set_granted(&dir, "netprobe", Permission::ProfileNetwork, true).unwrap();
        let Readiness::NeedsGrant(w) = check(Holder::User, false) else {
            panic!("a grant is needed first");
        };
        assert_eq!(
            (w.plugin.as_str(), w.author.as_str()),
            ("Net probe", "Ferris IDEA tests")
        );
        assert_eq!((w.host.as_deref(), w.port), (Some("127.0.0.1"), Some(9)));
        grant_network(&dir, "netprobe", &target, Holder::User).unwrap();
        assert!(matches!(check(Holder::User, false), Readiness::Ready(..)));
        // The user's grant does not carry over to agents; an agent's "allow once" does.
        assert!(matches!(
            check(Holder::Agent, false),
            Readiness::NeedsGrant(_)
        ));
        assert!(matches!(check(Holder::Agent, true), Readiness::Ready(..)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_warning_names_plugin_author_target_status_and_no_review() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"name":"Acme DB","version":"1.2.0","author":"Acme Labs","actions":[]}"#,
        )
        .unwrap();
        let w = network_warning(
            Some(&manifest),
            "acmedb",
            "AcmeDB",
            "imported",
            Some(&Target::new("db.acme.test", 7000)),
        );
        assert_eq!(w.title, "Allow network access for plugin “Acme DB”?");
        assert!(w.lines[0].contains("“Acme DB” 1.2.0 by Acme Labs"), "{w:?}");
        assert!(w.lines[0].contains("db.acme.test:7000"));
        assert!(w.lines[1].contains("this host and port only"));
        assert!(w.lines[2].contains("Imported"));
        assert!(
            w.text
                .contains("Ferris IDEA does not review the code of plugins")
        );
        assert!(!w.reviewed && w.receives_credentials);
        let none = network_warning(None, "x", "X", "user", None);
        assert_eq!(none.author, "unknown author");
        assert!(none.lines[0].contains("the host and port of the connection you pick"));
        assert!(none.host.is_none() && none.port.is_none());
    }

    /// The access of a probe call to `target` under `mode` with `ca_file`.
    fn access_with(
        dir: &Path,
        target: &Target,
        mode: NetMode,
        ca_file: Option<PathBuf>,
    ) -> crate::Access {
        crate::Access {
            network: Some(NetAccess {
                plugin: "netprobe".into(),
                target: target.clone(),
                holder: Holder::User,
                audit: Some(dir.join(NETWORK_AUDIT)),
                tls: NetTls { mode, ca_file },
            }),
            ..crate::Access::default()
        }
    }

    /// SPEC-0314 v2 AK-6, SPEC-0016 v4 AK-15: a connection without TLS only where the
    /// connection's SSL mode allows plain text; elsewhere `-6` before anything is resolved.
    #[test]
    fn plain_connections_follow_the_ssl_mode() {
        let dir = temp("plain");
        let plugin = Plugin::from_bytes(&dir.join("netprobe.wasm"), &probe()).unwrap();
        let run = |target: &Target, mode| {
            plugin
                .run_in(
                    "connect",
                    &target.label(),
                    crate::FUEL,
                    access_with(&dir, target, mode, None),
                )
                .unwrap()
        };
        let local = Target::new("127.0.0.1", 9);
        for mode in [NetMode::Require, NetMode::VerifyCa, NetMode::VerifyFull] {
            assert_eq!(run(&local, mode), "E6", "{mode:?}");
        }
        let remote = Target::new("db.example.invalid", 5432);
        assert_eq!(run(&remote, NetMode::Prefer), "E6");
        let log = audit_records(&dir, Some("netprobe"), 100);
        assert_eq!(log.len(), 4);
        assert!(log.iter().all(|r| r.outcome == "refused"
            && r.error.as_deref() == Some("needs TLS")
            && r.address.is_none()));
        // `prefer` on the loopback and `disable` on any host may go without TLS.
        let (port, server) = pong_server();
        assert_eq!(
            run(&Target::new("127.0.0.1", port), NetMode::Prefer),
            "pong"
        );
        server.join().unwrap();
        let (port, server) = pong_server();
        assert_eq!(
            run(&Target::new("127.0.0.1", port), NetMode::Disable),
            "pong"
        );
        server.join().unwrap();
        let opened: Vec<_> = audit_records(&dir, Some("netprobe"), 100)
            .into_iter()
            .filter(|r| r.outcome == "opened")
            .collect();
        assert_eq!(opened.len(), 2);
        assert!(opened.iter().all(|r| r.tls == Some(false)));
        let off = |mode| NetTls {
            mode,
            ca_file: None,
        };
        assert!(off(NetMode::Disable).allows_plain(&remote));
        assert!(!off(NetMode::Prefer).allows_plain(&remote));
        for host in ["localhost", "127.0.0.1", "127.8.9.10", "::1"] {
            assert!(
                off(NetMode::Prefer).allows_plain(&Target::new(host, 1)),
                "{host}"
            );
        }
        for host in [
            "0.0.0.0",
            "::ffff:127.0.0.1",
            "localhost.example",
            "10.0.0.5",
        ] {
            assert!(
                !off(NetMode::Prefer).allows_plain(&Target::new(host, 1)),
                "{host}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A test certificate authority and a server certificate for `localhost` it signed, valid
    /// from today for less than a year (macOS refuses server certificates valid for longer).
    struct TestPki {
        ca_pem: String,
        chain: Vec<rustls::pki_types::CertificateDer<'static>>,
        key: Vec<u8>,
    }

    fn test_pki(name: &str) -> TestPki {
        let today = now_rfc3339();
        let year: i32 = today[..4].parse().unwrap();
        let month: u8 = today[5..7].parse().unwrap();
        let day: u8 = today[8..10].parse().unwrap();
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca.distinguished_name.push(rcgen::DnType::CommonName, name);
        ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];
        let ca_cert = ca.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::from_params(&ca, &ca_key);
        let key = rcgen::KeyPair::generate().unwrap();
        let mut leaf = rcgen::CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        leaf.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        leaf.not_before = rcgen::date_time_ymd(year, month, day);
        leaf.not_after = rcgen::date_time_ymd(year + 1, month, 1);
        let leaf = leaf.signed_by(&key, &issuer).unwrap();
        TestPki {
            ca_pem: ca_cert.pem(),
            chain: vec![leaf.der().clone(), ca_cert.der().clone()],
            key: key.serialize_der(),
        }
    }

    /// A TLS server on 127.0.0.1 with `pki`'s certificate that answers `pong` to `ping` on each of
    /// `count` connections; a connection whose handshake fails is counted too.
    fn tls_server(pki: &TestPki, count: usize) -> (u16, std::thread::JoinHandle<()>) {
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(pki.key.clone().into());
        let config = std::sync::Arc::new(
            rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(pki.chain.clone(), key)
                .unwrap(),
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            for _ in 0..count {
                let Ok((tcp, _)) = listener.accept() else {
                    return;
                };
                let conn = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut s = rustls::StreamOwned::new(conn, tcp);
                let mut buf = [0; 4];
                if s.read_exact(&mut buf).is_ok() && &buf == b"ping" {
                    let _ = s.write_all(b"pong");
                    let _ = s.flush();
                }
            }
        });
        (port, t)
    }

    /// A server on 127.0.0.1 without TLS that answers anything with `pong` once.
    fn plain_server() -> (u16, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0; 512];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"pong");
            }
        });
        (port, t)
    }

    /// SPEC-0314 v2 AK-6, SPEC-0016 v4 AK-15: `net_connect_tls` makes TLS in every mode, with the
    /// mode's checks and the CA file instead of the system's roots; a failed handshake is `-7`
    /// and logged with its reason.
    #[test]
    fn tls_made_by_the_host_checks_as_the_mode_says() {
        let dir = temp("tls");
        let plugin = Plugin::from_bytes(
            &dir.join("netprobe.wasm"),
            &probe_through("net_connect_tls"),
        )
        .unwrap();
        let ours = test_pki("Ferris IDEA plugin test CA");
        let ca = dir.join("ca.pem");
        std::fs::write(&ca, &ours.ca_pem).unwrap();
        let other = dir.join("other-ca.pem");
        std::fs::write(&other, test_pki("Another CA").ca_pem).unwrap();
        let empty = dir.join("empty.pem");
        std::fs::write(&empty, "no certificate\n").unwrap();
        let run = |target: &Target, mode, ca_file: Option<&Path>| {
            plugin
                .run_in(
                    "connect",
                    &target.label(),
                    crate::FUEL,
                    access_with(&dir, target, mode, ca_file.map(Path::to_path_buf)),
                )
                .unwrap()
        };
        // `require`: encrypted, the certificate not checked.
        let (port, server) = tls_server(&ours, 1);
        assert_eq!(
            run(&Target::new("127.0.0.1", port), NetMode::Require, None),
            "pong"
        );
        server.join().unwrap();
        // `verify-full` with the CA file that signed the certificate, for its host name.
        let (port, server) = tls_server(&ours, 3);
        let named = Target::new("localhost", port);
        assert_eq!(run(&named, NetMode::VerifyFull, Some(&ca)), "pong");
        // Without it the system's roots do not know the test CA; another CA file neither.
        assert_eq!(run(&named, NetMode::VerifyFull, None), "E7");
        assert_eq!(run(&named, NetMode::VerifyFull, Some(&other)), "E7");
        server.join().unwrap();
        // A CA file without certificates fails before anything is resolved.
        assert_eq!(run(&named, NetMode::VerifyFull, Some(&empty)), "E7");
        // A server without TLS.
        let (port, server) = plain_server();
        assert_eq!(
            run(&Target::new("127.0.0.1", port), NetMode::Require, None),
            "E7"
        );
        server.join().unwrap();
        let log = audit_records(&dir, Some("netprobe"), 100);
        let outcomes: Vec<&str> = log.iter().map(|r| r.outcome.as_str()).collect();
        assert_eq!(
            outcomes,
            [
                "opened", "closed", "opened", "closed", "failed", "failed", "failed", "failed"
            ]
        );
        assert!(log[..4].iter().all(|r| r.tls == Some(true)));
        assert!(
            log[4..]
                .iter()
                .all(|r| r.error.as_deref().is_some_and(|e| !e.is_empty()))
        );
        assert!(
            log[6]
                .error
                .as_deref()
                .unwrap()
                .contains("no PEM certificate")
        );
        assert!(log[6].address.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0314 v2 AK-6: the plugin's input names the SSL mode and whether it may go without
    /// TLS, never the CA file; the host keeps the mode and the file with the grant.
    #[test]
    fn the_request_names_the_mode() {
        let request = ConnectorRequest {
            op: "test".into(),
            host: "ignored".into(),
            ssl: NetMode::Prefer,
            ca_file: Some(PathBuf::from("/home/u/ca.pem")),
            ..ConnectorRequest::default()
        };
        let access = |host: &str| NetAccess {
            plugin: "netprobe".into(),
            target: Target::new(host, 5432),
            holder: Holder::User,
            audit: None,
            tls: NetTls::default(),
        };
        let (local, kept) = prepared(&request, access("127.0.0.1"));
        assert!(local.plain);
        assert_eq!(local.host, "127.0.0.1");
        assert_eq!(kept.tls.mode, NetMode::Prefer);
        assert_eq!(kept.tls.ca_file, Some(PathBuf::from("/home/u/ca.pem")));
        let (remote, _) = prepared(&request, access("db.example.com"));
        assert!(!remote.plain);
        let json = serde_json::to_value(&remote).unwrap();
        assert_eq!(json["ssl"], "prefer");
        assert_eq!(json["plain"], false);
        assert!(json.get("ca_file").is_none(), "{json}");
        assert_eq!(NetTls::default().mode, NetMode::VerifyFull);
    }
}
