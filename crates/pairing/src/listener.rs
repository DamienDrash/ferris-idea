//! The listener of `idecraft serve --pair` (`SPEC-0321` behaviours 13–14, 17–19, 21): nothing but
//! the two hybrid handshakes, a pairing window for one device, rate limits per address, and
//! revocation that reaches live sessions within a second.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use idecraft_session::{Backend, Peer, Right};
use zeroize::Zeroizing;

use crate::channel::{Channel, Limits};
use crate::client::{NOT_STORED, STORED};
use crate::code;
use crate::keys::{KeyError, Store};
use crate::noise::{
    Accept, Finished, HandshakeError, Identity, Kind, fingerprint, pairing_key, prologue, respond,
    sas,
};

/// The time and rate limits of `ADR-0025` §8, adjustable for tests.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// A handshake completes within this time (10 s).
    pub handshake: Duration,
    /// A pairing window lasts this long (5 minutes).
    pub pairing_window: Duration,
    /// Attempts per address and minute (10).
    pub attempts_per_minute: u32,
    /// First wait after a failure (1 s), doubling to `backoff_cap`.
    pub backoff: Duration,
    /// Longest wait after failures (5 minutes).
    pub backoff_cap: Duration,
    /// Revocation check period (250 ms).
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(10),
            pairing_window: Duration::from_secs(300),
            attempts_per_minute: 10,
            backoff: Duration::from_secs(1),
            backoff_cap: Duration::from_secs(300),
            poll: Duration::from_millis(250),
        }
    }
}

/// The user at the console of `serve --pair`.
pub trait Console: Send + Sync {
    /// Shows one line.
    fn show(&self, line: &str);
    /// Asks whether the digits match; `false` declines.
    fn confirm(&self, device: &str, fingerprint: &str, digits: &str) -> bool;
    /// The rights for the new device (`view` is always among them).
    fn rights(&self, device: &str, offered: &[Right]) -> Vec<Right>;
    /// Shows a QR pairing link (`SPEC-0324` behaviour 3).
    fn pairing_link(&self, link: &str) {
        self.show(&format!("Pairing link: {link}"));
    }
}

/// How the listener runs.
pub struct ListenOptions {
    /// Address to bind.
    pub addr: SocketAddr,
    /// A non-loopback address was explicitly allowed.
    pub allow_network: bool,
    /// Open a pairing window even though devices are paired.
    pub new_device: bool,
    /// Pair with a QR link and a 256-bit secret instead of the short code (`SPEC-0324`).
    pub qr: bool,
    /// Limits.
    pub timing: Timing,
    /// Channel limits.
    pub limits: Limits,
}

/// Why the listener could not start.
#[derive(Debug, thiserror::Error)]
pub enum ListenError {
    /// A non-loopback address without `--allow-network`.
    #[error(
        "{0} is not a loopback address; pass --allow-network as well to listen on the network. \
         The channel is encrypted and only paired devices can use it, but the host is then \
         reachable from other machines."
    )]
    NotLoopback(SocketAddr),
    /// The address cannot be bound.
    #[error("cannot listen on {0}: {1}")]
    Bind(SocketAddr, String),
    /// The key store refused.
    #[error(transparent)]
    Keys(#[from] KeyError),
}

type Closer = Arc<dyn Fn() + Send + Sync>;

struct Rate {
    start: Instant,
    count: u32,
    failures: u32,
    until: Option<Instant>,
}

struct PairingWindow {
    key: Zeroizing<[u8; 32]>,
    deadline: Instant,
    failures: u32,
    busy: bool,
}

struct Inner {
    backend: Backend,
    store: Store,
    identity: Identity,
    console: Arc<dyn Console>,
    addr: SocketAddr,
    timing: Timing,
    limits: Limits,
    rates: Mutex<HashMap<IpAddr, Rate>>,
    pairing: Mutex<Option<PairingWindow>>,
    live: Mutex<HashMap<String, Vec<(u64, Closer)>>>,
    next_conn: AtomicU64,
    stop: AtomicBool,
}

/// A running listener.
pub struct Running {
    inner: Arc<Inner>,
    /// The address it listens on.
    pub addr: SocketAddr,
}

impl Running {
    /// Stops accepting.
    pub fn stop(&self) {
        self.inner.shut_down("stopped");
    }

    /// Whether the listener has ended (remote access was switched off).
    pub fn stopped(&self) -> bool {
        self.inner.stop.load(Ordering::SeqCst)
    }

    /// Whether a pairing window is open.
    pub fn pairing_open(&self) -> bool {
        let g = self.inner.pairing.lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref().is_some_and(|p| p.deadline > Instant::now())
    }
}

fn is_loopback(a: &SocketAddr) -> bool {
    a.ip().is_loopback()
}

/// Starts the listener; returns once it is bound. The short code is only ever shown on `console`.
pub fn listen(
    options: ListenOptions,
    store: Store,
    backend: Backend,
    console: Arc<dyn Console>,
) -> Result<Running, ListenError> {
    if !is_loopback(&options.addr) && !options.allow_network {
        return Err(ListenError::NotLoopback(options.addr));
    }
    let identity = store.identity()?;
    let devices = store.devices()?;
    // An explicit start switches remote access on again (`SPEC-0324` behaviour 11).
    store.set_access_off(false)?;
    let listener = TcpListener::bind(options.addr)
        .map_err(|e| ListenError::Bind(options.addr, e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| ListenError::Bind(options.addr, e.to_string()))?;
    console.show(&format!(
        "idecraft serve: listening on {addr}; only paired devices can connect"
    ));
    if !is_loopback(&addr) {
        console.show("warning: this listener is reachable from other machines on the network");
    }
    console.show(&format!(
        "key fingerprint {} ({})",
        fingerprint(&identity.public),
        store.describe()
    ));
    let mut pairing = None;
    if devices.is_empty() || options.new_device {
        let random = || ListenError::Bind(addr, "no random source for a pairing code".to_owned());
        let minutes = options.timing.pairing_window.as_secs() / 60;
        let shown = if options.qr {
            let secret = code::secret().ok_or_else(random)?;
            let link = code::format_link(&addr.to_string(), &identity.public, &secret);
            console.show(&format!(
                "Pair a device within {minutes} minutes: scan the QR image or use this link once"
            ));
            console.pairing_link(&link);
            secret
        } else {
            let shown = code::generate().ok_or_else(random)?;
            console.show(&format!(
                "Pair a device within {minutes} minutes: enter address {addr} and code {shown} in the window"
            ));
            shown
        };
        pairing = Some(PairingWindow {
            key: pairing_key(&shown),
            deadline: Instant::now() + options.timing.pairing_window,
            failures: 0,
            busy: false,
        });
    }
    let inner = Arc::new(Inner {
        backend,
        store,
        identity,
        console,
        addr,
        timing: options.timing,
        limits: options.limits,
        rates: Mutex::new(HashMap::new()),
        pairing: Mutex::new(pairing),
        live: Mutex::new(HashMap::new()),
        next_conn: AtomicU64::new(0),
        stop: AtomicBool::new(false),
    });
    {
        let inner = Arc::clone(&inner);
        std::thread::spawn(move || accept_loop(listener, inner));
    }
    {
        let inner = Arc::clone(&inner);
        std::thread::spawn(move || revocation_loop(inner));
    }
    Ok(Running { inner, addr })
}

fn accept_loop(listener: TcpListener, inner: Arc<Inner>) {
    for conn in listener.incoming() {
        if inner.stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = conn else { continue };
        let Ok(peer) = stream.peer_addr() else {
            continue;
        };
        if !inner.admit(peer.ip()) {
            // Refused without a byte (`SPEC-0321` behaviour 21).
            continue;
        }
        let inner = Arc::clone(&inner);
        std::thread::spawn(move || inner.handle(stream, peer));
    }
}

fn revocation_loop(inner: Arc<Inner>) {
    let mut announced = HashSet::new();
    while !inner.stop.load(Ordering::SeqCst) {
        std::thread::sleep(inner.timing.poll);
        if inner.store.access_off().unwrap_or(false) {
            inner.shut_down("access-off");
            return;
        }
        inner.apply_rights();
        let Ok(revoked) = inner.store.revoked() else {
            continue;
        };
        let live: Vec<(String, Vec<Closer>)> = {
            let map = inner.live.lock().unwrap_or_else(|e| e.into_inner());
            map.iter()
                .filter(|(fp, _)| revoked.contains(*fp))
                .map(|(fp, conns)| {
                    (
                        fp.clone(),
                        conns.iter().map(|(_, c)| Arc::clone(c)).collect(),
                    )
                })
                .collect()
        };
        for (fp, conns) in live {
            for close in conns {
                close();
            }
            inner.backend.end_sessions_of(&peer_name_prefix(&fp));
            if announced.insert(fp.clone()) {
                inner
                    .backend
                    .post_notice("device", &format!("Device revoked: {fp}"), None);
            }
        }
    }
}

fn peer_name_prefix(fp: &str) -> String {
    format!("device:{fp}")
}

impl Inner {
    /// Stops accepting and closes every session of a device (`SPEC-0324` behaviour 11).
    fn shut_down(&self, why: &str) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        let conns: Vec<Closer> = {
            let map = self.live.lock().unwrap_or_else(|e| e.into_inner());
            map.values()
                .flat_map(|v| v.iter().map(|(_, c)| Arc::clone(c)))
                .collect()
        };
        conns.iter().for_each(|close| close());
        self.backend.end_sessions_why("device:", why);
        let _ = TcpStream::connect_timeout(&self.wake_address(), Duration::from_secs(1));
        self.console.show(&format!("remote access closed: {why}"));
    }

    /// An address that reaches the accept loop, also for a wildcard bind.
    fn wake_address(&self) -> SocketAddr {
        let mut a = self.addr;
        if a.ip().is_unspecified() {
            a.set_ip(IpAddr::from([127, 0, 0, 1]));
        }
        a
    }

    /// Applies a changed rights list to the live sessions (`SPEC-0324` behaviour 8).
    fn apply_rights(&self) {
        let Ok(devices) = self.store.devices() else {
            return;
        };
        for d in devices {
            let rights: Vec<Right> = Right::ALL
                .iter()
                .copied()
                .filter(|r| d.rights.iter().any(|n| n == r.name()))
                .collect();
            if self
                .backend
                .set_rights_of(&peer_name_prefix(&d.fingerprint), &rights)
            {
                let closers: Vec<Closer> = self
                    .live
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&d.fingerprint)
                    .map(|v| v.iter().map(|(_, c)| Arc::clone(c)).collect())
                    .unwrap_or_default();
                closers.iter().for_each(|close| close());
            }
        }
    }

    fn admit(&self, ip: IpAddr) -> bool {
        let mut rates = self.rates.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let r = rates.entry(ip).or_insert(Rate {
            start: now,
            count: 0,
            failures: 0,
            until: None,
        });
        if r.until.is_some_and(|u| u > now) {
            return false;
        }
        if now.duration_since(r.start) >= Duration::from_secs(60) {
            r.start = now;
            r.count = 0;
        }
        r.count += 1;
        r.count <= self.timing.attempts_per_minute
    }

    fn record(&self, ip: IpAddr, ok: bool) {
        let mut rates = self.rates.lock().unwrap_or_else(|e| e.into_inner());
        let Some(r) = rates.get_mut(&ip) else {
            return;
        };
        if ok {
            r.failures = 0;
            r.until = None;
            return;
        }
        r.failures += 1;
        let factor = 1u32 << r.failures.saturating_sub(1).min(16);
        let wait = self
            .timing
            .backoff
            .saturating_mul(factor)
            .min(self.timing.backoff_cap);
        r.until = Some(Instant::now() + wait);
    }

    /// Takes the pairing key if a window is open and free; marks it busy.
    fn claim_pairing(&self) -> Option<Zeroizing<[u8; 32]>> {
        let mut g = self.pairing.lock().unwrap_or_else(|e| e.into_inner());
        let window = g.as_mut()?;
        if window.deadline <= Instant::now() {
            *g = None;
            return None;
        }
        if window.busy {
            return None;
        }
        window.busy = true;
        Some(window.key.clone())
    }

    fn pairing_failed(&self) {
        let mut g = self.pairing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = g.as_mut() {
            w.busy = false;
            w.failures += 1;
            if w.failures >= 3 {
                *g = None;
                drop(g);
                self.console
                    .show("pairing window closed after three failed attempts");
            }
        }
    }

    fn release_pairing(&self) {
        let mut g = self.pairing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = g.as_mut() {
            w.busy = false;
        }
    }

    fn close_pairing(&self) {
        *self.pairing.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn handle(self: Arc<Self>, mut stream: TcpStream, peer: SocketAddr) {
        let ip = peer.ip();
        let _ = stream.set_read_timeout(Some(self.timing.handshake));
        let _ = stream.set_write_timeout(Some(self.timing.handshake));
        // A stalled peer cannot hold the connection past the limit as a whole.
        let deadline_guard = stream.try_clone().ok();
        let done = Arc::new(AtomicBool::new(false));
        if let Some(guard) = deadline_guard {
            let (done, limit) = (Arc::clone(&done), self.timing.handshake);
            std::thread::spawn(move || {
                let t0 = Instant::now();
                while t0.elapsed() < limit {
                    if done.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                if !done.load(Ordering::SeqCst) {
                    let _ = guard.shutdown(std::net::Shutdown::Both);
                }
            });
        }
        let key = self.claim_pairing();
        let accept_key = key.as_deref();
        let store = &self.store;
        let pinned = |p: &[u8; 32]| matches!(store.device_for(p), Ok(Some(_)));
        let accept = Accept {
            pairing: accept_key,
            pinned: &pinned,
        };
        let outcome = respond(
            &mut stream,
            &self.identity,
            &accept,
            &prologue("window", "serve"),
        );
        done.store(true, Ordering::SeqCst);
        let (finished, kind) = match outcome {
            Ok(x) => x,
            Err(e) => {
                if key.is_some() {
                    match e {
                        HandshakeError::Noise => self.pairing_failed(),
                        _ => self.release_pairing(),
                    }
                }
                self.record(ip, false);
                self.console
                    .show(&format!("refused a connection from {ip}: {e}"));
                let _ = stream.shutdown(std::net::Shutdown::Both);
                return;
            }
        };
        // A later connection must not leave the pairing window claimed.
        if kind == Kind::Known && key.is_some() {
            self.release_pairing();
        }
        self.record(ip, true);
        let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
        match kind {
            Kind::Pair => self.run_pairing(stream, finished),
            Kind::Known => self.run_session(stream, finished),
        }
    }

    fn run_pairing(&self, stream: TcpStream, done: Finished) {
        let fp = fingerprint(&done.peer);
        let digits = sas(&done.hash, &self.identity.public, &done.peer, &done.device);
        // The code has been used: from here on the pairing window closes however the
        // confirmation ends without storing (`SPEC-0321` v5 behaviour 17).
        let mut channel = match Channel::new(stream, done.transport, self.limits) {
            Ok(c) => c,
            Err(_) => {
                self.close_pairing();
                self.console
                    .show("the connection ended during pairing; nothing was stored");
                return;
            }
        };
        let confirmed = self.console.confirm(&done.device, &fp, &digits);
        if !confirmed {
            let _ = channel.write_line("no");
            channel.close();
            self.close_pairing();
            self.console.show("pairing declined; nothing was stored");
            return;
        }
        // The window may have declined and closed already: its `no` is still read when this
        // `ok` can no longer be sent.
        let sent = channel.write_line("ok");
        let reply = channel.read_line(32);
        self.close_pairing();
        match (reply, sent) {
            (Ok(reply), Ok(())) if reply == "ok" => {}
            (Ok(reply), _) if reply != "ok" => {
                channel.close();
                self.console
                    .show("the window declined the pairing; nothing was stored");
                return;
            }
            _ => {
                channel.close();
                self.console
                    .show("the connection ended during pairing; nothing was stored");
                return;
            }
        }
        let rights: Vec<String> = self
            .console
            .rights(&done.device, &Right::ALL)
            .iter()
            .map(|r| r.name().to_owned())
            .collect();
        // The window pins this host only on `stored`, sent once the device and its rights are
        // durably stored, so a connection made right after pairing is accepted (`SPEC-0321`
        // behaviour 17).
        match self.store.add_device(&done.peer, &done.device, &rights) {
            Ok(()) => {
                let told = channel.write_line(STORED).is_ok();
                channel.close();
                self.console.show(&format!(
                    "paired {} ({fp}) with rights: {}",
                    done.device,
                    rights.join(", ")
                ));
                if !told {
                    self.console.show(
                        "the window left before it was told that the device is stored; it has \
                         not pinned this host and must pair again",
                    );
                }
                self.backend.post_notice(
                    "device",
                    &format!("Device paired: {}", done.device),
                    None,
                );
            }
            Err(e) => {
                let _ = channel.write_line(NOT_STORED);
                channel.close();
                self.console.show(&format!("pairing failed: {e}"));
            }
        }
    }

    fn run_session(&self, stream: TcpStream, done: Finished) {
        let fp = fingerprint(&done.peer);
        let Ok(Some(device)) = self.store.device_for(&done.peer) else {
            return;
        };
        let rights: Vec<Right> = Right::ALL
            .iter()
            .copied()
            .filter(|r| device.rights.iter().any(|n| n == r.name()))
            .collect();
        let channel = match Channel::new(stream, done.transport, self.limits) {
            Ok(c) => c,
            Err(_) => return,
        };
        let closer = channel.closer();
        let id = self.next_conn.fetch_add(1, Ordering::SeqCst);
        self.live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(fp.clone())
            .or_default()
            .push((id, closer));
        let transport = channel.into_transport();
        let peer = Peer {
            name: format!("{}:{}", peer_name_prefix(&fp), device.name),
            rights,
        };
        self.backend.serve_as(transport, peer);
        let mut map = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conns) = map.get_mut(&fp) {
            conns.retain(|(i, _)| *i != id);
            if conns.is_empty() {
                map.remove(&fp);
            }
        }
    }
}
