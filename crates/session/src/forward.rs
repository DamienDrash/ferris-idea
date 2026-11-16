//! The window's end of a port forward (`SPEC-0318` behaviour 27, `ADR-0025` §6, SEC-7): a
//! listener on loopback only (`127.0.0.1`, and `[::1]` where available), on the host's port
//! number when it is free, else on a free one. Each accepted connection asks the backend for a
//! stream (`port.connect`) and relays the bytes in both directions with credit. There is no way to
//! listen on another interface.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;

use crate::client::Connection;
use crate::rpc::RpcError;

/// A running local end of a forward; dropping it (or [`LocalForward::close`]) stops it.
pub struct LocalForward {
    /// The forward's id on the backend.
    pub forward: String,
    /// The local addresses listened on (always loopback).
    pub addresses: Vec<SocketAddr>,
    stop: Arc<AtomicBool>,
}

impl LocalForward {
    /// Listens on loopback for `forward` (port `wanted` if free, else any) and relays every
    /// accepted connection over `conn`.
    pub fn start(conn: Connection, forward: &str, wanted: u16) -> std::io::Result<Self> {
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, wanted))
            .or_else(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))?;
        let port = v4.local_addr()?.port();
        let mut listeners = vec![v4];
        if let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            listeners.push(v6);
        }
        let addresses = listeners
            .iter()
            .filter_map(|l| l.local_addr().ok())
            .collect::<Vec<_>>();
        debug_assert!(addresses.iter().all(|a| a.ip().is_loopback()));
        let stop = Arc::new(AtomicBool::new(false));
        for listener in listeners {
            listener.set_nonblocking(true)?;
            let (conn, stop, forward) = (conn.clone(), Arc::clone(&stop), forward.to_owned());
            std::thread::Builder::new()
                .name("port-forward".into())
                .spawn(move || accept(listener, conn, &forward, &stop))?;
        }
        Ok(Self {
            forward: forward.to_owned(),
            addresses,
            stop,
        })
    }

    /// The local port.
    pub fn port(&self) -> u16 {
        self.addresses.first().map_or(0, SocketAddr::port)
    }

    /// Stops listening; open connections end when the backend closes the forward.
    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Whether it still listens.
    pub fn running(&self) -> bool {
        !self.stop.load(Ordering::SeqCst)
    }
}

impl Drop for LocalForward {
    fn drop(&mut self) {
        self.close();
    }
}

fn accept(listener: TcpListener, conn: Connection, forward: &str, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) && conn.alive() {
        match listener.accept() {
            Ok((socket, peer)) => {
                // Loopback only, also for what connects (a listener on 127.0.0.1 gets nothing
                // else, the check keeps it so if that ever changes).
                if !peer.ip().is_loopback() {
                    let _ = socket.shutdown(Shutdown::Both);
                    continue;
                }
                let (conn, forward) = (conn.clone(), forward.to_owned());
                std::thread::spawn(move || {
                    let _ = relay(socket, &conn, &forward);
                });
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn relay(socket: TcpStream, conn: &Connection, forward: &str) -> Result<(), RpcError> {
    let _ = socket.set_nonblocking(false);
    let opened = conn.call(
        "port.connect",
        json!({ "forward": forward }),
        Duration::from_secs(10),
    );
    let stream = match opened.map(|v| v["stream"].as_u64()) {
        Ok(Some(s)) => s as u32,
        other => {
            let _ = socket.shutdown(Shutdown::Both);
            return other.map(drop);
        }
    };
    conn.open_sending(stream);
    let mut reader = socket
        .try_clone()
        .map_err(|e| crate::rpc::RpcError::new(crate::rpc::ErrorCode::Internal, e.to_string()))?;
    let up = conn.clone();
    let upload = std::thread::spawn(move || {
        let mut buf = [0u8; 32 << 10];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if up
                        .send_data(stream, &buf[..n], Duration::from_secs(45))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        up.close_stream(stream, "done");
    });
    let mut writer = socket;
    loop {
        let (data, done) = conn.read_stream(stream, Duration::from_millis(200));
        if !data.is_empty() && writer.write_all(&data).is_err() {
            break;
        }
        if done.is_some() {
            break;
        }
    }
    let _ = writer.shutdown(Shutdown::Both);
    let _ = upload.join();
    conn.drop_stream(stream);
    Ok(())
}
