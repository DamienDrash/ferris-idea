//! MCP over Streamable HTTP for the running window (`SPEC-0221`): loopback only, a bearer token per
//! start, `Host`/`Origin` checks against DNS rebinding and a 0600 discovery file in the user's
//! settings folder.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio_util::sync::CancellationToken;
use tower_service::Service;

use crate::IdeServer;

/// Largest request body (behaviour 8).
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

type McpService = StreamableHttpService<IdeServer, LocalSessionManager>;

/// One served project.
struct Route {
    root: PathBuf,
    service: McpService,
}

/// What every connection needs.
struct Shared {
    port: u16,
    token: String,
    cancel: CancellationToken,
    /// Projects by id, and the id of the first one (`/mcp`).
    routes: RwLock<(Option<String>, HashMap<String, Route>)>,
}

/// A running HTTP server; stops when dropped.
pub struct HttpServer {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
    discovery: Option<PathBuf>,
    started: u64,
}

impl HttpServer {
    /// Starts listening on `127.0.0.1` at a port the system picks (behaviour 1). The discovery file
    /// is written to `discovery_dir` (the user's settings folder in the window; `None` writes none).
    pub fn start(discovery_dir: Option<&Path>) -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            port,
            token: new_token()?,
            cancel: CancellationToken::new(),
            routes: RwLock::new((None, HashMap::new())),
        });
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ferris-idea-mcp-http")
            .enable_all()
            .build()?;
        let serving = shared.clone();
        let thread = std::thread::Builder::new()
            .name("ferris-idea-mcp-http".into())
            .spawn(move || {
                runtime.block_on(accept_loop(listener, serving));
                // Open event streams end with the runtime.
                runtime.shutdown_timeout(std::time::Duration::from_secs(1));
            })?;
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let discovery = match discovery_dir {
            Some(dir) => {
                let folder = dir.join("mcp-http");
                remove_stale(&folder);
                Some(folder.join(format!("{}.json", std::process::id())))
            }
            None => None,
        };
        let server = Self {
            shared,
            thread: Some(thread),
            discovery,
            started,
        };
        server.write_discovery()?;
        Ok(server)
    }

    /// The port listened on.
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// The address listened on: always on `127.0.0.1`.
    pub fn local_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.shared.port))
    }

    /// The bearer token of this start.
    pub fn token(&self) -> &str {
        &self.shared.token
    }

    /// The URL of the first project.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.shared.port)
    }

    /// The discovery file, if one is written.
    pub fn discovery_file(&self) -> Option<&Path> {
        self.discovery.as_deref()
    }

    /// Serves `server`'s workspace and returns its URL; a workspace served already keeps its route.
    pub fn add_workspace(&self, server: IdeServer) -> std::io::Result<String> {
        let root = server.root().to_path_buf();
        let id = workspace_id(&root);
        {
            let mut routes = self
                .shared
                .routes
                .write()
                .unwrap_or_else(|e| e.into_inner());
            if !routes.1.contains_key(&id) {
                let config = StreamableHttpServerConfig::default()
                    .with_json_response(true)
                    .with_max_request_body_bytes(MAX_BODY_BYTES)
                    .with_allowed_origins(allowed_origins(self.shared.port))
                    .with_cancellation_token(self.shared.cancel.child_token());
                let base = server;
                let service = StreamableHttpService::new(
                    move || Ok(base.with_current_permissions()),
                    Arc::new(LocalSessionManager::default()),
                    config,
                );
                routes.1.insert(id.clone(), Route { root, service });
                routes.0.get_or_insert_with(|| id.clone());
            }
        }
        self.write_discovery()?;
        Ok(format!("{}/{id}", self.url()))
    }

    /// Stops serving the workspace at `root` (its window closed, `SPEC-0094` v3 behaviour 8):
    /// its URL answers 404 and the discovery file no longer lists it. Whether it was served.
    pub fn remove_workspace(&self, root: &Path) -> std::io::Result<bool> {
        let id = workspace_id(root);
        let removed = {
            let mut routes = self
                .shared
                .routes
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let removed = routes.1.remove(&id).is_some();
            if routes.0.as_deref() == Some(id.as_str()) {
                let mut ids: Vec<&String> = routes.1.keys().collect();
                ids.sort();
                routes.0 = ids.first().map(|i| (*i).clone());
            }
            removed
        };
        if removed {
            self.write_discovery()?;
        }
        Ok(removed)
    }

    /// Writes the discovery file: mode 0600 in a 0700 folder, through a new file renamed into place
    /// (behaviour 4).
    fn write_discovery(&self) -> std::io::Result<()> {
        let Some(file) = &self.discovery else {
            return Ok(());
        };
        // The file holds the bearer token (`SPEC-0321` v6 behaviour 8a).
        if cfg!(not(unix)) {
            return Err(not_private());
        }
        let folder = file.parent().expect("file in a folder");
        create_private_dir(folder)?;
        let projects: Vec<serde_json::Value> = {
            let routes = self.shared.routes.read().unwrap_or_else(|e| e.into_inner());
            let mut list: Vec<_> = routes
                .1
                .iter()
                .map(|(id, r)| {
                    serde_json::json!({
                        "root": r.root,
                        "url": format!("{}/{id}", self.url()),
                    })
                })
                .collect();
            list.sort_by(|a, b| a["url"].as_str().cmp(&b["url"].as_str()));
            list
        };
        let body = serde_json::json!({
            "pid": std::process::id(),
            "started": self.started,
            "port": self.shared.port,
            "url": self.url(),
            "token": self.shared.token,
            "header": "Authorization: Bearer <token>",
            "projects": projects,
        });
        let tmp = file.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut f = options.open(&tmp)?;
            f.write_all(serde_json::to_string_pretty(&body)?.as_bytes())?;
        }
        std::fs::rename(&tmp, file)
    }

    /// Stops listening, ends open streams and removes the discovery file.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.shared.cancel.cancel();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(file) = &self.discovery {
            let _ = std::fs::remove_file(file);
        }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn accept_loop(listener: std::net::TcpListener, shared: Arc<Shared>) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        return;
    };
    loop {
        let stream = tokio::select! {
            _ = shared.cancel.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
        };
        let shared = shared.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| handle(req, shared.clone()));
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                .await;
        });
    }
}

type HttpResponse = Response<BoxBody<Bytes, Infallible>>;

fn plain(status: StatusCode, text: &str) -> HttpResponse {
    let mut r = Response::new(Full::new(Bytes::from(text.to_owned())).boxed());
    *r.status_mut() = status;
    r.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    r
}

/// Checks origin, host and token in that order, then hands the request to the project's MCP service.
async fn handle(req: Request<Incoming>, shared: Arc<Shared>) -> Result<HttpResponse, Infallible> {
    let header = |name: hyper::header::HeaderName| {
        req.headers()
            .get(name)
            .map(|v| v.to_str().unwrap_or("\u{0}").to_owned())
    };
    if let Some(origin) = header(hyper::header::ORIGIN)
        && !allowed_origins(shared.port).contains(&origin)
    {
        return Ok(plain(StatusCode::FORBIDDEN, "origin not allowed"));
    }
    let host_ok = header(hyper::header::HOST).is_some_and(|h| host_allowed(&h, shared.port));
    if !host_ok {
        return Ok(plain(StatusCode::FORBIDDEN, "host not allowed"));
    }
    let presented = header(hyper::header::AUTHORIZATION).unwrap_or_default();
    let expected = format!("Bearer {}", shared.token);
    if !same_secret(presented.as_bytes(), expected.as_bytes()) {
        let mut r = plain(StatusCode::UNAUTHORIZED, "missing or wrong bearer token");
        r.headers_mut().insert(
            hyper::header::WWW_AUTHENTICATE,
            hyper::header::HeaderValue::from_static("Bearer realm=\"ferris-idea\""),
        );
        return Ok(r);
    }
    let path = req.uri().path().trim_end_matches('/');
    let service = {
        let routes = shared.routes.read().unwrap_or_else(|e| e.into_inner());
        let id = match path.strip_prefix("/mcp") {
            Some("") => routes.0.clone(),
            Some(rest) => rest.strip_prefix('/').map(str::to_owned),
            None => None,
        };
        id.and_then(|id| routes.1.get(&id).map(|r| r.service.clone()))
    };
    let Some(mut service) = service else {
        return Ok(plain(StatusCode::NOT_FOUND, "no such MCP endpoint"));
    };
    if req.method() != hyper::Method::POST {
        return service.call(req).await;
    }
    // A request's answer streams back on its own connection. A client that goes away without a
    // word (its process ended, the connection closed, no `notifications/cancelled`, no `DELETE`)
    // ends its call at once, so a question for a right is withdrawn at once and nothing it would
    // have allowed runs (`SPEC-0053` v12 behaviour 8, OBS-0114 Q2).
    let (parts, body) = req.into_parts();
    let Ok(body) = http_body_util::Limited::new(body, MAX_BODY_BYTES)
        .collect()
        .await
        .map(|b| b.to_bytes())
    else {
        return Ok(plain(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request body too large",
        ));
    };
    let ids = request_ids(&body);
    let session = parts.headers.get("mcp-session-id").cloned();
    let host = parts.headers.get(hyper::header::HOST).cloned();
    let version = parts.headers.get("mcp-protocol-version").cloned();
    let uri = parts.uri.clone();
    let response = service
        .call(Request::from_parts(parts, Full::new(body)))
        .await?;
    let (Some(session), false) = (session, ids.is_empty()) else {
        return Ok(response);
    };
    let call = OpenCall {
        service,
        uri,
        host,
        version,
        session,
        ids,
    };
    Ok(response.map(|inner| {
        GuardedBody {
            inner,
            call: Some(call),
        }
        .boxed()
    }))
}

/// The ids of the requests in a POST body (one message or a batch); notifications and responses
/// have none.
fn request_ids(body: &[u8]) -> Vec<serde_json::Value> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let messages = match value {
        serde_json::Value::Array(all) => all,
        one => vec![one],
    };
    messages
        .into_iter()
        .filter(|m| m.get("method").is_some())
        .filter_map(|m| m.get("id").cloned())
        .collect()
}

/// The requests whose answer a response body carries, to cancel them when the body is dropped
/// unfinished: the client closed the connection.
struct OpenCall {
    service: McpService,
    uri: hyper::Uri,
    host: Option<hyper::header::HeaderValue>,
    version: Option<hyper::header::HeaderValue>,
    session: hyper::header::HeaderValue,
    ids: Vec<serde_json::Value>,
}

impl OpenCall {
    /// Cancels the requests in their session as the client would (`notifications/cancelled`).
    fn cancel(self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(async move {
            let mut service = self.service;
            for id in self.ids {
                let note = serde_json::json!({
                    "jsonrpc": "2.0", "method": "notifications/cancelled",
                    "params": {"requestId": id, "reason": "the client closed the connection"},
                });
                let mut request = Request::builder()
                    .method(hyper::Method::POST)
                    .uri(self.uri.clone())
                    .header(hyper::header::CONTENT_TYPE, "application/json")
                    .header(hyper::header::ACCEPT, "application/json, text/event-stream")
                    .header("mcp-session-id", self.session.clone());
                if let Some(host) = &self.host {
                    request = request.header(hyper::header::HOST, host.clone());
                }
                if let Some(version) = &self.version {
                    request = request.header("mcp-protocol-version", version.clone());
                }
                if let Ok(request) = request.body(Full::new(Bytes::from(note.to_string()))) {
                    let _ = service.call(request).await;
                }
            }
        });
    }
}

/// A response body that cancels its requests when it is dropped before its end.
struct GuardedBody {
    inner: BoxBody<Bytes, Infallible>,
    call: Option<OpenCall>,
}

impl hyper::body::Body for GuardedBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Bytes>, Infallible>>> {
        let this = self.get_mut();
        let polled = std::pin::Pin::new(&mut this.inner).poll_frame(cx);
        if let std::task::Poll::Ready(None) = polled {
            // Answered to the end: nothing to cancel.
            this.call = None;
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        hyper::body::Body::is_end_stream(&self.inner)
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        hyper::body::Body::size_hint(&self.inner)
    }
}

impl Drop for GuardedBody {
    fn drop(&mut self) {
        if let Some(call) = self.call.take()
            && !hyper::body::Body::is_end_stream(&self.inner)
        {
            call.cancel();
        }
    }
}

fn allowed_origins(port: u16) -> Vec<String> {
    vec![
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
    ]
}

/// `Host` names loopback, with this server's port when it has one (behaviour 3).
fn host_allowed(host: &str, port: u16) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let (name, given) = match host.rsplit_once(':') {
        Some((n, p)) if !n.ends_with(':') && (n.starts_with('[') == n.ends_with(']')) => {
            (n.to_owned(), Some(p.to_owned()))
        }
        _ => (host.clone(), None),
    };
    matches!(name.as_str(), "127.0.0.1" | "localhost" | "[::1]")
        && given.is_none_or(|p| p == port.to_string())
}

/// Compares in time that depends only on the length (behaviour 2).
fn same_secret(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn new_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// A stable id of a project path: 12 hex digits of its FNV-1a hash.
pub fn workspace_id(root: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in root.to_string_lossy().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:012x}", h >> 16)
}

/// Off Unix the discovery folder and file cannot be made private yet, so none is written
/// (`SPEC-0321` v6 behaviour 8a).
fn not_private() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "a private MCP discovery file is not supported on this platform yet",
    )
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    if cfg!(not(unix)) {
        return Err(not_private());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

/// Removes discovery files of processes that no longer run.
fn remove_stale(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Some(pid) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix(".json").unwrap_or(s).parse::<u32>().ok())
        else {
            continue;
        };
        if pid != std::process::id() && !process_alive(pid) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_tokens_and_ids() {
        assert!(host_allowed("127.0.0.1:4000", 4000));
        assert!(host_allowed("localhost", 4000));
        assert!(host_allowed("[::1]:4000", 4000));
        assert!(!host_allowed("127.0.0.1:4001", 4000));
        assert!(!host_allowed("evil.example:4000", 4000));
        assert!(!host_allowed("localhost.evil.example", 4000));
        assert!(same_secret(b"Bearer ab", b"Bearer ab"));
        assert!(!same_secret(b"Bearer ac", b"Bearer ab"));
        assert!(!same_secret(b"Bearer a", b"Bearer ab"));
        let t = new_token().unwrap();
        assert_eq!(t.len(), 64);
        assert_ne!(t, new_token().unwrap());
        let id = workspace_id(Path::new("/tmp/project"));
        assert_eq!(id.len(), 12);
        assert_eq!(id, workspace_id(Path::new("/tmp/project")));
        assert_ne!(id, workspace_id(Path::new("/tmp/other")));
    }
}
