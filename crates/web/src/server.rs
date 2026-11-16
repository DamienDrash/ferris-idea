//! The listener (`SPEC-0325`): TLS first, then the request head with host and origin checks, then
//! the credential, then the session protocol's bytes in WebSocket binary messages.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use idecraft_session::{Backend, Peer, Right, Transport};
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use zeroize::Zeroizing;

use crate::http::{self, Head, HeadError};
use crate::tls::{normalize_authority, normalize_origin};

const CHECK_HTML: &[u8] = include_bytes!("../assets/check.html");
const CHECK_JS: &[u8] = include_bytes!("../assets/check.js");

/// A refusal says the same for every reason (behaviour 11).
const REFUSED: &str = "not authenticated";

/// The longest message; a session frame is at most 1 MiB, so this leaves room for overhead.
pub const MAX_MESSAGE: usize = 2 << 20;

/// The longest credential message.
const MAX_CREDENTIAL: usize = 4 << 10;

/// Limits of the endpoint (behaviours 7, 11, 14, 15). Tests shorten the times.
#[derive(Debug, Clone)]
pub struct Limits {
    /// TLS handshake plus request head.
    pub head: Duration,
    /// From the upgrade to the credential.
    pub credential: Duration,
    /// Failures per address in `window` before it is blocked.
    pub failures: u32,
    /// The failure window.
    pub window: Duration,
    /// How long an address stays blocked.
    pub block: Duration,
    /// Connections that are not yet authenticated.
    pub unauthenticated: usize,
    /// Authenticated connections.
    pub authenticated: usize,
    /// Between pings.
    pub ping: Duration,
    /// Silence that ends a connection.
    pub silence: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            head: Duration::from_secs(10),
            credential: Duration::from_secs(10),
            failures: 20,
            window: Duration::from_secs(60),
            block: Duration::from_secs(60),
            unauthenticated: 32,
            authenticated: 64,
            ping: Duration::from_secs(30),
            silence: Duration::from_secs(90),
        }
    }
}

/// What the listener needs.
pub struct WebConfig {
    /// Address to bind.
    pub addr: SocketAddr,
    /// `--allow-network`.
    pub allow_network: bool,
    /// `--wss-origin` values, already normalized with [`normalize_origin`].
    pub origins: Vec<String>,
    /// From `load_server_config`: the only way to make a listener.
    pub tls: Arc<rustls::ServerConfig>,
    /// `--wss-rights`: the rights of the owner credential.
    pub owner_rights: Vec<Right>,
    /// Limits.
    pub limits: Limits,
}

/// Why the listener did not start.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    /// A non-loopback address without `--allow-network` (behaviour 2).
    #[error("--listen {0} is not a loopback address; add --allow-network to listen on the network")]
    NetworkNotAllowed(SocketAddr),
    /// A network listener needs named origins (behaviour 2).
    #[error("a listener on the network needs at least one --wss-origin https://name[:port]")]
    NoOrigin,
    /// The backend has no fixed folder (behaviour 1).
    #[error("--wss needs a folder: a connection is bound to one project")]
    NoFolder,
    /// Binding or starting failed.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// No randomness.
    #[error("no random numbers")]
    Random,
}

#[derive(Default)]
struct Failures {
    map: HashMap<IpAddr, Entry>,
}

#[derive(Clone, Copy)]
struct Entry {
    count: u32,
    since: Instant,
    blocked_until: Option<Instant>,
}

struct Inner {
    backend: Backend,
    allowed: Vec<String>,
    strict: bool,
    owner_hash: [u8; 32],
    owner_rights: Vec<Right>,
    limits: Limits,
    failures: Mutex<Failures>,
    pre_auth: Arc<Semaphore>,
    live: Arc<Semaphore>,
    acceptor: TlsAcceptor,
    stop: Notify,
}

/// A running endpoint.
pub struct Running {
    addr: SocketAddr,
    secret: Mutex<Option<Zeroizing<String>>>,
    inner: Arc<Inner>,
    stopped: Arc<AtomicBool>,
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
}

impl Running {
    /// The address actually bound.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The owner credential, once (behaviour 12).
    pub fn take_owner_secret(&self) -> Option<String> {
        self.secret
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|s| s.as_str().to_owned())
    }

    /// Stops accepting.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.inner.stop.notify_waiters();
        if let Some(rt) = self
            .runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            rt.shutdown_background();
        }
    }

    /// Whether [`Running::stop`] was called.
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The authorities a loopback listener answers to.
fn loopback_authorities(port: u16) -> Vec<String> {
    vec![
        format!("localhost:{port}"),
        format!("127.0.0.1:{port}"),
        format!("[::1]:{port}"),
    ]
}

/// Starts the endpoint (behaviours 1–3). The backend must have a fixed folder.
pub fn listen(config: WebConfig, backend: Backend) -> Result<Running, WebError> {
    if backend.config().folder.is_none() {
        return Err(WebError::NoFolder);
    }
    if !config.addr.ip().is_loopback() {
        if !config.allow_network {
            return Err(WebError::NetworkNotAllowed(config.addr));
        }
        if config.origins.is_empty() {
            return Err(WebError::NoOrigin);
        }
    }
    let mut secret = Zeroizing::new([0u8; 32]);
    SystemRandom::new()
        .fill(secret.as_mut())
        .map_err(|_| WebError::Random)?;
    let secret = Zeroizing::new(idecraft_session::hello::hex(secret.as_ref()));
    let owner_hash = hash(&secret);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("web-endpoint")
        .enable_all()
        .build()?;
    let listener = runtime.block_on(TcpListener::bind(config.addr))?;
    let addr = listener.local_addr()?;
    let strict = !addr.ip().is_loopback();
    let mut allowed = config.origins;
    if addr.ip().is_loopback() {
        allowed.extend(loopback_authorities(addr.port()));
    }
    let limits = config.limits;
    let inner = Arc::new(Inner {
        backend,
        allowed,
        strict,
        owner_hash,
        owner_rights: config.owner_rights,
        failures: Mutex::new(Failures::default()),
        pre_auth: Arc::new(Semaphore::new(limits.unauthenticated)),
        live: Arc::new(Semaphore::new(limits.authenticated)),
        acceptor: TlsAcceptor::from(config.tls),
        stop: Notify::new(),
        limits,
    });
    runtime.spawn(accept_loop(listener, Arc::clone(&inner)));
    Ok(Running {
        addr,
        secret: Mutex::new(Some(secret)),
        inner,
        stopped: Arc::new(AtomicBool::new(false)),
        runtime: Mutex::new(Some(runtime)),
    })
}

fn hash(text: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(digest::digest(&digest::SHA256, text.as_bytes()).as_ref());
    out
}

async fn accept_loop(listener: TcpListener, inner: Arc<Inner>) {
    loop {
        tokio::select! {
            _ = inner.stop.notified() => return,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                if inner.is_blocked(peer.ip()) {
                    continue;
                }
                let Ok(permit) = Arc::clone(&inner.pre_auth).try_acquire_owned() else {
                    continue;
                };
                tokio::spawn(handle(stream, peer, Arc::clone(&inner), permit));
            }
        }
    }
}

impl Inner {
    fn is_blocked(&self, ip: IpAddr) -> bool {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = failures.map.get_mut(&ip) else {
            return false;
        };
        match entry.blocked_until {
            Some(until) if Instant::now() < until => true,
            Some(_) => {
                failures.map.remove(&ip);
                false
            }
            None => false,
        }
    }

    fn record_failure(&self, ip: IpAddr) {
        let now = Instant::now();
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        // The map cannot grow without bound: stale entries go first.
        if failures.map.len() >= 4096 {
            let (window, block) = (self.limits.window, self.limits.block);
            failures.map.retain(|_, e| {
                now.duration_since(e.since) < window + block
                    || e.blocked_until.is_some_and(|u| now < u)
            });
        }
        let entry = failures.map.entry(ip).or_insert(Entry {
            count: 0,
            since: now,
            blocked_until: None,
        });
        if now.duration_since(entry.since) >= self.limits.window {
            *entry = Entry {
                count: 0,
                since: now,
                blocked_until: None,
            };
        }
        entry.count += 1;
        if entry.count >= self.limits.failures {
            entry.blocked_until = Some(now + self.limits.block);
        }
    }

    fn audit(&self, peer: IpAddr, outcome: &str) {
        self.backend
            .audit_event("web.connect", Some(&peer.to_string()), outcome);
    }

    /// Checks the two headers that keep other pages out (behaviours 8, 9). Returns a status.
    fn check_host(&self, head: &Head) -> Result<(), &'static str> {
        let host = head
            .header("host")
            .and_then(normalize_authority)
            .ok_or("421 Misdirected Request")?;
        if self.allowed.contains(&host) {
            Ok(())
        } else {
            Err("421 Misdirected Request")
        }
    }

    fn check_origin(&self, head: &Head) -> bool {
        head.header("origin")
            .and_then(|o| normalize_origin(o).ok())
            .is_some_and(|o| self.allowed.contains(&o))
    }

    /// What a credential admits (behaviours 12, 13).
    fn identify(&self, credential: &str) -> Option<(Peer, &'static str)> {
        if hash(credential) == self.owner_hash {
            return Some((
                Peer {
                    name: "web:owner".into(),
                    rights: self.owner_rights.clone(),
                },
                "owner",
            ));
        }
        if self.backend.anonymous_link_live(credential) {
            let mut random = [0u8; 8];
            SystemRandom::new().fill(&mut random).ok()?;
            return Some((
                Peer {
                    name: format!("web-guest:{}", idecraft_session::hello::hex(&random)),
                    rights: vec![Right::View],
                },
                "guest",
            ));
        }
        None
    }
}

async fn handle(
    stream: TcpStream,
    peer: SocketAddr,
    inner: Arc<Inner>,
    permit: OwnedSemaphorePermit,
) {
    let _ = stream.set_nodelay(true);
    let limits = inner.limits.clone();
    let tls = match tokio::time::timeout(limits.head, inner.acceptor.accept(stream)).await {
        Ok(Ok(tls)) => tls,
        _ => return,
    };
    let mut tls = tls;
    let head = match tokio::time::timeout(limits.head, http::read_head(&mut tls)).await {
        Ok(Ok(head)) => head,
        Ok(Err(HeadError::TooLarge)) => {
            let _ = refuse(
                &mut tls,
                "431 Request Header Fields Too Large",
                inner.strict,
            )
            .await;
            return;
        }
        Ok(Err(_)) | Err(_) => return,
    };
    if head.method != "GET" {
        let _ = refuse(&mut tls, "405 Method Not Allowed", inner.strict).await;
        return;
    }
    if let Err(status) = inner.check_host(&head) {
        let _ = refuse(&mut tls, status, inner.strict).await;
        return;
    }
    if !head.is_upgrade() {
        let (status, ctype, body): (&str, &str, &[u8]) = match head.path.as_str() {
            "/" => ("200 OK", "text/html; charset=utf-8", CHECK_HTML),
            "/check.js" => ("200 OK", "text/javascript; charset=utf-8", CHECK_JS),
            _ => ("404 Not Found", "text/plain; charset=utf-8", b"not found"),
        };
        let bytes = http::response(status, ctype, body, inner.strict);
        let _ = tls.write_all(&bytes).await;
        let _ = tls.shutdown().await;
        return;
    }
    if !inner.check_origin(&head) {
        inner.audit(peer.ip(), "origin_refused");
        let _ = refuse(&mut tls, "403 Forbidden", inner.strict).await;
        return;
    }
    let key = match (
        head.header("sec-websocket-version"),
        head.header("sec-websocket-key"),
    ) {
        (Some("13"), Some(key)) => key.to_owned(),
        _ => {
            let _ = refuse(&mut tls, "426 Upgrade Required", inner.strict).await;
            return;
        }
    };
    let accept = derive_accept_key(key.as_bytes());
    if tls
        .write_all(&http::switching(&accept, inner.strict))
        .await
        .is_err()
    {
        return;
    }
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_MESSAGE);
    config.max_frame_size = Some(MAX_MESSAGE);
    let mut ws = WebSocketStream::from_raw_socket(tls, Role::Server, Some(config)).await;
    let credential = match tokio::time::timeout(limits.credential, first_message(&mut ws)).await {
        Ok(Some(c)) => c,
        _ => {
            inner.record_failure(peer.ip());
            inner.audit(peer.ip(), "no_credential");
            let _ = close(&mut ws).await;
            return;
        }
    };
    let Some((session_peer, kind)) = inner.identify(&credential) else {
        inner.record_failure(peer.ip());
        inner.audit(peer.ip(), "credential_refused");
        let _ = close(&mut ws).await;
        return;
    };
    drop(credential);
    let Ok(live) = Arc::clone(&inner.live).try_acquire_owned() else {
        let _ = close(&mut ws).await;
        return;
    };
    if ws
        .send(Message::text(json!({"ok": true, "as": kind}).to_string()))
        .await
        .is_err()
    {
        return;
    }
    inner.audit(peer.ip(), "accepted");
    drop(permit);
    let transport = bridge(
        ws,
        limits.ping,
        limits.silence,
        tokio::runtime::Handle::current(),
    );
    let backend = inner.backend.clone();
    std::thread::spawn(move || {
        backend.serve_as(transport, session_peer);
        drop(live);
    });
}

async fn refuse<S: AsyncWrite + Unpin>(tls: &mut S, status: &str, strict: bool) -> io::Result<()> {
    let bytes = http::response(status, "text/plain; charset=utf-8", b"refused", strict);
    tls.write_all(&bytes).await?;
    tls.shutdown().await
}

async fn close<S: AsyncRead + AsyncWrite + Unpin>(
    ws: &mut WebSocketStream<S>,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    ws.close(Some(CloseFrame {
        code: CloseCode::Policy,
        reason: REFUSED.into(),
    }))
    .await
}

/// The credential out of the first message; anything else is `None`.
async fn first_message<S: AsyncRead + AsyncWrite + Unpin>(
    ws: &mut WebSocketStream<S>,
) -> Option<Zeroizing<String>> {
    let message = ws.next().await?.ok()?;
    let Message::Text(text) = message else {
        return None;
    };
    if text.len() > MAX_CREDENTIAL {
        return None;
    }
    let value: Value = serde_json::from_str(text.as_str()).ok()?;
    value["credential"]
        .as_str()
        .map(|s| Zeroizing::new(s.to_owned()))
}

/// Puts a WebSocket under a [`Transport`] of the session protocol (behaviour 15): binary messages
/// are the bytes in both directions, message boundaries mean nothing, text closes the connection.
pub fn bridge<S>(
    ws: WebSocketStream<S>,
    ping: Duration,
    silence: Duration,
    runtime: tokio::runtime::Handle,
) -> Transport
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (incoming_tx, incoming_rx) = mpsc::channel::<Vec<u8>>(64);
    let (outgoing_tx, outgoing_rx) = mpsc::channel::<Vec<u8>>(64);
    let stop = Arc::new(Notify::new());
    runtime.spawn(pump(
        ws,
        incoming_tx,
        outgoing_rx,
        Arc::clone(&stop),
        ping,
        silence,
    ));
    let closer: Arc<dyn Fn() + Send + Sync> = Arc::new(move || stop.notify_one());
    Transport {
        reader: Box::new(ChannelReader {
            rx: incoming_rx,
            pending: Vec::new(),
            at: 0,
        }),
        writer: Box::new(ChannelWriter { tx: outgoing_tx }),
        closer,
    }
}

async fn pump<S>(
    mut ws: WebSocketStream<S>,
    incoming: mpsc::Sender<Vec<u8>>,
    mut outgoing: mpsc::Receiver<Vec<u8>>,
    stop: Arc<Notify>,
    ping: Duration,
    silence: Duration,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut ticks = tokio::time::interval(ping);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticks.tick().await;
    let mut heard = Instant::now();
    loop {
        tokio::select! {
            () = stop.notified() => break,
            _ = ticks.tick() => {
                if heard.elapsed() > silence {
                    break;
                }
                if ws.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            bytes = outgoing.recv() => {
                let Some(bytes) = bytes else { break };
                if ws.send(Message::binary(bytes)).await.is_err() {
                    break;
                }
            }
            message = ws.next() => {
                heard = Instant::now();
                match message {
                    Some(Ok(Message::Binary(bytes))) => {
                        if incoming.send(bytes.to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                    _ => break,
                }
            }
        }
    }
    drop(incoming);
    let _ = ws.close(None).await;
}

struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    at: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.pending.len() {
            match self.rx.blocking_recv() {
                Some(bytes) => {
                    self.pending = bytes;
                    self.at = 0;
                }
                None => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len() - self.at);
        buf[..n].copy_from_slice(&self.pending[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

struct ChannelWriter {
    tx: mpsc::Sender<Vec<u8>>,
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tx
            .blocking_send(buf.to_vec())
            .map(|()| buf.len())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
