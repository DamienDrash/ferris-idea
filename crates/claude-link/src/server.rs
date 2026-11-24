//! The link's server (`SPEC-0358` behaviours 1–11): a WebSocket listener on 127.0.0.1 that speaks
//! JSON-RPC 2.0 in the shape of MCP, accepts only clients that present the token of the lock file,
//! and answers Claude Code's requests through the window ([`Editor`]).

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::editor::{DiffDecision, Editor, Selection};
use crate::lockfile::{LockContent, LockFile};
use crate::paths::to_uri;

/// The header that carries the token (behaviour 8).
pub const AUTH_HEADER: &str = "x-claude-code-ide-authorization";

/// The largest message the link reads (behaviour 9).
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Clients connected at once (behaviour 9).
pub const MAX_CLIENTS: usize = 8;

/// Tool calls one client may have running at once (behaviour 9).
pub const MAX_CALLS: usize = 16;

/// How long the opening handshake may take.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// How often a connection looks for messages to send and for the stop.
const POLL: Duration = Duration::from_millis(25);

/// The MCP versions the link answers in; another asked version is answered with the first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// Where and for whom a link starts.
#[derive(Debug, Clone)]
pub struct LinkConfig {
    /// The project folder the link serves.
    pub workspace: PathBuf,
    /// The folder of the lock file (behaviour 3); tests use a temporary folder.
    pub lock_dir: PathBuf,
    /// The name Claude Code shows for the IDE.
    pub ide_name: String,
    /// The version in the answer to `initialize`.
    pub ide_version: String,
}

/// A connected client as the link reports it (behaviour 23); never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClientState {
    /// The client's name from `initialize` (`claude-code`), until then empty.
    pub name: String,
    /// Its version from `initialize`.
    pub version: String,
    /// The process id it reported with `ide_connected`.
    pub pid: Option<u32>,
    /// Seconds since it connected.
    pub connected_seconds: u64,
}

/// The state of a link (behaviour 23); never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkState {
    /// The project folder.
    pub workspace: PathBuf,
    /// The port on 127.0.0.1.
    pub port: u16,
    /// The listener runs.
    pub listening: bool,
    /// The lock file.
    pub lock_file: PathBuf,
    /// The clients connected now.
    pub clients: Vec<ClientState>,
    /// Diffs waiting for the user.
    pub pending_diffs: usize,
}

pub(crate) struct Client {
    pub id: u64,
    pub name: String,
    pub version: String,
    pub pid: Option<u32>,
    pub since: Instant,
    pub initialized: bool,
    pub outbox: Sender<String>,
}

pub(crate) struct PendingDiff {
    pub id: u64,
    pub client: u64,
    pub decide: Sender<DiffDecision>,
}

/// What the listener, the connections and the tool calls share.
pub(crate) struct Shared {
    pub workspace: PathBuf,
    pub port: u16,
    pub editor: Arc<dyn Editor>,
    pub ide_name: String,
    pub ide_version: String,
    token: String,
    pub stopping: AtomicBool,
    pub clients: Mutex<Vec<Client>>,
    pub next_id: AtomicU64,
    pub diffs: Mutex<HashMap<String, PendingDiff>>,
    pub latest: Mutex<Option<Selection>>,
    sent: Mutex<Option<Selection>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A running link: listening until dropped; dropping stops it and removes the lock file.
pub struct LinkServer {
    shared: Arc<Shared>,
    lock_file: Option<LockFile>,
    accept: Option<JoinHandle<()>>,
}

impl LinkServer {
    /// Listens on a port the system picks on 127.0.0.1, removes stale lock files of this IDE, and
    /// writes the lock file with a fresh token (behaviours 1–6).
    pub fn start(config: LinkConfig, editor: Arc<dyn Editor>) -> std::io::Result<Self> {
        let workspace = std::fs::canonicalize(&config.workspace)?;
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let token = new_token()?;
        crate::lockfile::remove_stale(&config.lock_dir, &config.ide_name, std::process::id());
        let folder = workspace.to_string_lossy().into_owned();
        let lock_file = LockFile::write(
            &config.lock_dir,
            port,
            &LockContent {
                pid: std::process::id(),
                workspace_folders: vec![folder],
                ide_name: &config.ide_name,
                transport: "ws",
                auth_token: &token,
            },
        )?;
        let shared = Arc::new(Shared {
            workspace,
            port,
            editor,
            ide_name: config.ide_name,
            ide_version: config.ide_version,
            token,
            stopping: AtomicBool::new(false),
            clients: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            diffs: Mutex::new(HashMap::new()),
            latest: Mutex::new(None),
            sent: Mutex::new(None),
        });
        let accepting = shared.clone();
        let accept = std::thread::Builder::new()
            .name("claude-link-accept".into())
            .spawn(move || accept_loop(listener, accepting))?;
        Ok(Self {
            shared,
            lock_file: Some(lock_file),
            accept: Some(accept),
        })
    }

    /// The port on 127.0.0.1.
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// The project folder, canonical.
    pub fn workspace(&self) -> &Path {
        &self.shared.workspace
    }

    /// The lock file.
    pub fn lock_file(&self) -> Option<&Path> {
        self.lock_file.as_ref().map(LockFile::path)
    }

    /// What the link reports (behaviour 23).
    pub fn state(&self) -> LinkState {
        let clients = lock(&self.shared.clients)
            .iter()
            .map(|c| ClientState {
                name: c.name.clone(),
                version: c.version.clone(),
                pid: c.pid,
                connected_seconds: c.since.elapsed().as_secs(),
            })
            .collect();
        LinkState {
            workspace: self.shared.workspace.clone(),
            port: self.shared.port,
            listening: !self.shared.stopping.load(Ordering::SeqCst),
            lock_file: self.lock_file().map(Path::to_path_buf).unwrap_or_default(),
            clients,
            pending_diffs: lock(&self.shared.diffs).len(),
        }
    }

    /// Whether a client that sent `initialize` is connected.
    pub fn connected(&self) -> bool {
        lock(&self.shared.clients).iter().any(|c| c.initialized)
    }

    /// The window's selection now (behaviour 14): kept as the latest, and sent as
    /// `selection_changed` to every initialized client when it differs from the one sent last.
    pub fn notify_selection(&self, selection: Option<Selection>) {
        let Some(selection) = selection else {
            return;
        };
        *lock(&self.shared.latest) = Some(selection.clone());
        {
            let mut sent = lock(&self.shared.sent);
            if sent.as_ref() == Some(&selection) {
                return;
            }
            *sent = Some(selection.clone());
        }
        let message = json!({
            "jsonrpc": "2.0",
            "method": "selection_changed",
            "params": selection_json(&selection),
        })
        .to_string();
        for c in lock(&self.shared.clients).iter().filter(|c| c.initialized) {
            let _ = c.outbox.send(message.clone());
        }
    }
}

impl Drop for LinkServer {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        // Every waiting diff is answered as rejected: nothing is written after the stop.
        for (_, d) in lock(&self.shared.diffs).drain() {
            let _ = d.decide.send(DiffDecision::Rejected);
        }
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
        self.lock_file = None;
    }
}

/// The selection as `selection_changed` and `getCurrentSelection` give it.
pub(crate) fn selection_json(s: &Selection) -> Value {
    json!({
        "text": s.text,
        "filePath": s.path,
        "fileUrl": to_uri(&s.path),
        "selection": {
            "start": {"line": s.start.line, "character": s.start.character},
            "end": {"line": s.end.line, "character": s.end.character},
            "isEmpty": s.start == s.end,
        },
    })
}

fn new_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compares in time that depends only on the length (behaviour 8).
fn same_secret(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut connections: Vec<JoinHandle<()>> = Vec::new();
    while !shared.stopping.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                connections.retain(|h| !h.is_finished());
                if !peer.ip().is_loopback() {
                    continue;
                }
                let s = shared.clone();
                if let Ok(h) = std::thread::Builder::new()
                    .name("claude-link-client".into())
                    .spawn(move || serve(stream, s))
                {
                    connections.push(h);
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(POLL),
            Err(_) => std::thread::sleep(POLL),
        }
    }
    for h in connections {
        let _ = h.join();
    }
}

/// Why an opening handshake is refused (behaviour 8): the status only, nothing about the token.
fn refuse(status: u16) -> ErrorResponse {
    let mut r = ErrorResponse::new(None);
    *r.status_mut() = tungstenite::http::StatusCode::from_u16(status)
        .unwrap_or(tungstenite::http::StatusCode::FORBIDDEN);
    r
}

/// Checks the opening handshake (behaviour 8): the Host is this port on the loopback address, no
/// browser `Origin`, and the token matches.
fn check_handshake(shared: &Shared, request: &Request) -> Result<(), u16> {
    let headers = request.headers();
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    if !host_allowed(host, shared.port) {
        return Err(403);
    }
    if headers.contains_key("origin") {
        return Err(403);
    }
    let given = headers
        .get(AUTH_HEADER)
        .map(|v| v.as_bytes())
        .unwrap_or_default();
    if !same_secret(given, shared.token.as_bytes()) {
        return Err(401);
    }
    if lock(&shared.clients).len() >= MAX_CLIENTS {
        return Err(503);
    }
    Ok(())
}

/// The subprotocol the link answers: `mcp` when the client offers it, else the first one offered
/// (a token of letters, digits, `.`, `-` and `_`), else none.
fn subprotocol(request: &Request) -> Option<String> {
    let offered: Vec<String> = request
        .headers()
        .get_all("sec-websocket-protocol")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|p| p.trim().to_owned())
        .filter(|p| {
            !p.is_empty()
                && p.len() <= 64
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        })
        .collect();
    offered
        .iter()
        .find(|p| p.as_str() == "mcp")
        .or_else(|| offered.first())
        .cloned()
}

/// A Host header of the loopback address with this port (or none).
fn host_allowed(host: &str, port: u16) -> bool {
    let (name, given) = match host.rsplit_once(':') {
        Some((n, p)) if !n.ends_with(':') && (n.starts_with('[') == n.ends_with(']')) => {
            (n, Some(p))
        }
        _ => (host, None),
    };
    matches!(name, "127.0.0.1" | "localhost" | "[::1]")
        && given.is_none_or(|p| p == port.to_string())
}

// The handshake callback's error type is tungstenite's `ErrorResponse`, fixed by its `Callback`.
#[allow(clippy::result_large_err)]
fn serve(stream: TcpStream, shared: Arc<Shared>) {
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).is_err()
    {
        return;
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let checker = shared.clone();
    let callback =
        move |request: &Request, mut response: Response| match check_handshake(&checker, request) {
            Ok(()) => {
                // Claude Code offers a subprotocol and refuses a server that does not name one
                // (behaviour 8a): `mcp` when offered, else the first offered.
                if let Some(chosen) = subprotocol(request) {
                    response.headers_mut().insert(
                        "sec-websocket-protocol",
                        tungstenite::http::HeaderValue::from_str(&chosen)
                            .map_err(|_| refuse(400))?,
                    );
                }
                Ok(response)
            }
            Err(status) => Err(refuse(status)),
        };
    let Ok(mut ws) = tungstenite::accept_hdr_with_config(stream, callback, Some(config)) else {
        return;
    };
    if ws.get_ref().set_read_timeout(Some(POLL)).is_err() {
        return;
    }
    let (outbox, inbox) = channel::<String>();
    let id = shared.next_id.fetch_add(1, Ordering::SeqCst);
    {
        let mut clients = lock(&shared.clients);
        if clients.len() >= MAX_CLIENTS {
            return;
        }
        clients.push(Client {
            id,
            name: String::new(),
            version: String::new(),
            pid: None,
            since: Instant::now(),
            initialized: false,
            outbox: outbox.clone(),
        });
    }
    let calls = Arc::new(AtomicUsize::new(0));
    run_connection(&mut ws, &inbox, &outbox, &shared, id, &calls);
    let _ = ws.close(None);
    let _ = ws.flush();
    // The client left: its waiting diffs are answered as rejected and closed (behaviour 19).
    lock(&shared.clients).retain(|c| c.id != id);
    let left: Vec<(String, PendingDiff)> = {
        let mut diffs = lock(&shared.diffs);
        let names: Vec<String> = diffs
            .iter()
            .filter(|(_, d)| d.client == id)
            .map(|(n, _)| n.clone())
            .collect();
        names
            .into_iter()
            .filter_map(|n| diffs.remove(&n).map(|d| (n, d)))
            .collect()
    };
    for (name, d) in left {
        let _ = d.decide.send(DiffDecision::Rejected);
        shared.editor.close_diff(shared.workspace.clone(), name);
    }
    shared.editor.link_changed(shared.workspace.clone());
}

fn run_connection(
    ws: &mut WebSocket<TcpStream>,
    inbox: &Receiver<String>,
    outbox: &Sender<String>,
    shared: &Arc<Shared>,
    id: u64,
    calls: &Arc<AtomicUsize>,
) {
    loop {
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        loop {
            match inbox.try_recv() {
                Ok(text) => {
                    if ws.send(Message::text(text)).is_err() {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                handle_text(text.as_str(), outbox, shared, id, calls);
            }
            Ok(Message::Binary(_)) => {
                let _ = outbox.send(error_message(Value::Null, -32700, "Parse error"));
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}

fn error_message(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

fn result_message(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

/// A short printable text from a client (names and versions), at most 100 characters.
fn clean(text: Option<&str>) -> String {
    text.unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect()
}

fn handle_text(
    text: &str,
    outbox: &Sender<String>,
    shared: &Arc<Shared>,
    client: u64,
    calls: &Arc<AtomicUsize>,
) {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        let _ = outbox.send(error_message(Value::Null, -32700, "Parse error"));
        return;
    };
    let Some(object) = message.as_object() else {
        let _ = outbox.send(error_message(Value::Null, -32600, "Invalid Request"));
        return;
    };
    let id = object.get("id").cloned();
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        // An answer to something the link never asked: ignored.
        return;
    };
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        notification(method, &params, shared, client);
        return;
    };
    let initialized = lock(&shared.clients)
        .iter()
        .any(|c| c.id == client && c.initialized);
    let answer = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == asked)
                .copied()
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            if let Some(c) = lock(&shared.clients).iter_mut().find(|c| c.id == client) {
                c.name = clean(params["clientInfo"]["name"].as_str());
                c.version = clean(params["clientInfo"]["version"].as_str());
                c.initialized = true;
            }
            shared.editor.link_changed(shared.workspace.clone());
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"logging": {}, "tools": {"listChanged": true}},
                "serverInfo": {"name": shared.ide_name, "version": shared.ide_version},
            }))
        }
        "ping" => Ok(json!({})),
        _ if !initialized => Err((-32002, "Server not initialized".to_owned())),
        "tools/list" => Ok(json!({"tools": crate::tools::list()})),
        "prompts/list" => Ok(json!({"prompts": []})),
        "resources/list" => Ok(json!({"resources": []})),
        "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
        "tools/call" => {
            if calls.fetch_add(1, Ordering::SeqCst) >= MAX_CALLS {
                calls.fetch_sub(1, Ordering::SeqCst);
                Err((-32000, "too many calls at once".to_owned()))
            } else {
                let (s, out, running) = (shared.clone(), outbox.clone(), calls.clone());
                let spawned = std::thread::Builder::new()
                    .name("claude-link-call".into())
                    .spawn(move || {
                        let answer = crate::tools::call(&s, client, &params);
                        let _ = out.send(match answer {
                            Ok(result) => result_message(id, result),
                            Err((code, m)) => error_message(id, code, &m),
                        });
                        running.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    calls.fetch_sub(1, Ordering::SeqCst);
                }
                return;
            }
        }
        _ => Err((-32601, "Method not found".to_owned())),
    };
    let _ = outbox.send(match answer {
        Ok(result) => result_message(id, result),
        Err((code, m)) => error_message(id, code, &m),
    });
}

fn notification(method: &str, params: &Value, shared: &Shared, client: u64) {
    match method {
        "notifications/initialized" => {}
        // Claude Code names its process (behaviour 10).
        "ide_connected" => {
            if let Some(c) = lock(&shared.clients).iter_mut().find(|c| c.id == client) {
                c.pid = params["pid"].as_u64().and_then(|p| u32::try_from(p).ok());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_secrets() {
        assert!(host_allowed("127.0.0.1:4000", 4000));
        assert!(host_allowed("localhost", 4000));
        assert!(host_allowed("[::1]:4000", 4000));
        assert!(!host_allowed("127.0.0.1:4001", 4000));
        assert!(!host_allowed("evil.example:4000", 4000));
        assert!(!host_allowed("localhost.evil.example", 4000));
        assert!(same_secret(b"ab", b"ab"));
        assert!(!same_secret(b"ac", b"ab"));
        assert!(!same_secret(b"a", b"ab"));
        assert!(!same_secret(b"", b"ab"));
        let t = new_token().unwrap();
        assert_eq!(t.len(), 64);
        assert_ne!(t, new_token().unwrap());
    }
}
