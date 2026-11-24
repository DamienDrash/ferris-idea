//! The allowlist proxy (`SPEC-0347` behaviour 9): an HTTP `CONNECT` proxy on `127.0.0.1` that
//! opens a tunnel only to a destination of the policy, never to a local or private address
//! reached through a name. The sandbox lets the process reach this port and nothing else.

use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::policy::HostRule;

/// Longest request head.
pub const MAX_HEAD: usize = 8 * 1024;
/// Time for the request head, and for connecting to the destination.
pub const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
/// Most tunnels open at once.
pub const MAX_TUNNELS: usize = 64;
/// Most decisions kept for the summary; older ones are dropped.
const MAX_DECISIONS: usize = 1000;

/// One decision of the proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    /// The destination as asked for (`host:port`), or the request line when it was no `CONNECT`.
    pub destination: String,
    /// Whether the tunnel was opened.
    pub allowed: bool,
    /// Why.
    pub reason: String,
}

struct Shared {
    rules: Vec<HostRule>,
    stop: AtomicBool,
    tunnels: AtomicUsize,
    decisions: Mutex<Vec<Decision>>,
}

impl Shared {
    fn record(&self, destination: &str, allowed: bool, reason: &str) {
        let mut d = self.decisions.lock().unwrap_or_else(|e| e.into_inner());
        if d.len() == MAX_DECISIONS {
            d.remove(0);
        }
        d.push(Decision {
            destination: destination.to_string(),
            allowed,
            reason: reason.to_string(),
        });
    }
}

/// A running proxy; it stops accepting when dropped.
pub struct AllowlistProxy {
    port: u16,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl AllowlistProxy {
    /// Starts the proxy on a free port of `127.0.0.1` for these host entries.
    pub fn start(rules: Vec<HostRule>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            rules,
            stop: AtomicBool::new(false),
            tunnels: AtomicUsize::new(0),
            decisions: Mutex::new(Vec::new()),
        });
        let s = shared.clone();
        let accept = std::thread::Builder::new()
            .name("sandbox-proxy".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if s.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(conn) = conn else { continue };
                    let s = s.clone();
                    let _ = std::thread::Builder::new()
                        .name("sandbox-proxy-conn".into())
                        .spawn(move || serve(conn, &s));
                }
            })?;
        Ok(Self {
            port,
            shared,
            accept: Some(accept),
        })
    }

    /// The port on `127.0.0.1`.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The decisions so far, oldest first.
    pub fn decisions(&self) -> Vec<Decision> {
        self.shared
            .decisions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The environment that points a process at the proxy.
    pub fn env(&self) -> Vec<(String, String)> {
        let url = format!("http://127.0.0.1:{}", self.port);
        ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"]
            .iter()
            .flat_map(|k| [k.to_string(), k.to_ascii_lowercase()])
            .map(|k| (k, url.clone()))
            .collect()
    }
}

impl Drop for AllowlistProxy {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Wakes the accept loop.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)),
            Duration::from_secs(1),
        );
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
    }
}

/// Answers one client connection.
fn serve(mut client: TcpStream, shared: &Shared) {
    let (head, rest) = match read_head(&mut client) {
        Ok(h) => h,
        Err(reason) => {
            shared.record("", false, &reason);
            let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
            return;
        }
    };
    let line = head.lines().next().unwrap_or_default().to_string();
    let mut words = line.split_whitespace();
    let (method, target) = (
        words.next().unwrap_or_default(),
        words.next().unwrap_or_default(),
    );
    if method != "CONNECT" {
        shared.record(&line, false, "only CONNECT is served");
        let _ = client.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n");
        return;
    }
    let refuse = |client: &mut TcpStream, reason: &str| {
        shared.record(target, false, reason);
        let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n");
    };
    let Some((host, port)) = split_target(target) else {
        return refuse(&mut client, "not host:port");
    };
    if !shared.rules.iter().any(|r| r.matches(&host, port)) {
        return refuse(&mut client, "not in the allowlist");
    }
    let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        // An address is reached only because it is itself an entry (`HostRule::matches`).
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => match (host.as_str(), port).to_socket_addrs() {
            Ok(a) => a.collect(),
            Err(e) => return refuse(&mut client, &format!("cannot be resolved: {e}")),
        },
    };
    if host.parse::<IpAddr>().is_err()
        && let Some(local) = addrs.iter().find(|a| local_or_private(a.ip()))
    {
        return refuse(
            &mut client,
            &format!("resolves to the local or private address {}", local.ip()),
        );
    }
    if shared.tunnels.fetch_add(1, Ordering::SeqCst) >= MAX_TUNNELS {
        shared.tunnels.fetch_sub(1, Ordering::SeqCst);
        shared.record(target, false, "too many tunnels");
        let _ = client.write_all(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n");
        return;
    }
    let upstream = addrs
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, HEAD_TIMEOUT).ok());
    let Some(mut upstream) = upstream else {
        shared.tunnels.fetch_sub(1, Ordering::SeqCst);
        shared.record(target, false, "cannot connect");
        let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n");
        return;
    };
    shared.record(target, true, "in the allowlist");
    let opened = client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .and_then(|()| upstream.write_all(&rest))
        .and_then(|()| client.set_read_timeout(None));
    if opened.is_ok() {
        tunnel(client, upstream);
    }
    shared.tunnels.fetch_sub(1, Ordering::SeqCst);
}

/// Reads the request head within [`HEAD_TIMEOUT`] and [`MAX_HEAD`]; answers it and the bytes
/// that came after it.
fn read_head(client: &mut TcpStream) -> Result<(String, Vec<u8>), String> {
    let deadline = Instant::now() + HEAD_TIMEOUT;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            return Ok((head, buf[end + 4..].to_vec()));
        }
        if buf.len() > MAX_HEAD {
            return Err("request head too long".into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("request head too slow".into());
        }
        client
            .set_read_timeout(Some(left))
            .map_err(|e| e.to_string())?;
        match client.read(&mut chunk) {
            Ok(0) => return Err("closed before the request head ended".into()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(format!("reading the request head: {e}")),
        }
    }
}

/// `host:port` or `[v6]:port`; the host in lower case without brackets.
fn split_target(target: &str) -> Option<(String, u16)> {
    let (host, port) = if let Some(rest) = target.strip_prefix('[') {
        let (h, p) = rest.split_once("]:")?;
        (h, p)
    } else {
        target.rsplit_once(':')?
    };
    let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_ascii_lowercase(), port))
}

/// Addresses a name must not lead to (behaviour 9).
pub fn local_or_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_local(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return v4_local(v4);
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || v6 == Ipv6Addr::LOCALHOST
        }
    }
}

fn v4_local(v4: Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    v4.is_loopback()
        || v4.is_unspecified()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_broadcast()
        || a == 0
        || (a == 100 && (b & 0xc0) == 64)
}

/// Copies both ways until both directions have ended.
fn tunnel(client: TcpStream, upstream: TcpStream) {
    let (Ok(mut c2), Ok(mut u2)) = (client.try_clone(), upstream.try_clone()) else {
        return;
    };
    let (mut c1, mut u1) = (client, upstream);
    let up = std::thread::spawn(move || {
        let _ = io::copy(&mut c1, &mut u1);
        let _ = u1.shutdown(Shutdown::Write);
    });
    let _ = io::copy(&mut u2, &mut c2);
    let _ = c2.shutdown(Shutdown::Write);
    let _ = up.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_private_addresses() {
        for a in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "ff02::1",
        ] {
            assert!(local_or_private(a.parse().unwrap()), "{a}");
        }
        for a in ["93.184.216.34", "100.128.0.1", "2001:4860::8888", "8.8.8.8"] {
            assert!(!local_or_private(a.parse().unwrap()), "{a}");
        }
    }

    #[test]
    fn targets_are_split() {
        assert_eq!(
            split_target("Example.com:443"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(split_target("[::1]:80"), Some(("::1".into(), 80)));
        assert_eq!(split_target("example.com"), None);
        assert_eq!(split_target("example.com:0"), None);
        assert_eq!(split_target(":80"), None);
    }
}
