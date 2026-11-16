//! The window's end of a connection (`ADR-0026`): requests with answers, notifications, streams
//! with credit, and a clean `unavailable` for everything in flight when the connection breaks.
//! Anything malformed from the backend closes the connection (`ADR-0025` SEC-14).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::frame::{Frame, FrameKind, read_frame};
use crate::rpc::{ErrorCode, Message, RpcError};
use crate::transport::{Credits, Outbox, Priority, Transport};

/// What arrives without being waited for.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// A notification of the backend.
    Notification {
        /// `domain.verb`.
        method: String,
        /// Parameters (with `seq`).
        params: Value,
    },
    /// The answer to a request sent with [`Connection::send`].
    Response {
        /// The request id.
        id: u64,
        /// Result or error.
        result: Result<Value, RpcError>,
    },
    /// A request of the backend to the window.
    Request {
        /// Its id.
        id: Value,
        /// `domain.verb`.
        method: String,
        /// Parameters.
        params: Value,
    },
    /// The connection ended, with the reason.
    Closed(String),
}

#[derive(Default)]
struct StreamBuf {
    data: Vec<u8>,
    done: Option<String>,
}

struct Shared {
    out: Arc<Outbox>,
    closer: Arc<dyn Fn() + Send + Sync>,
    waiting: Mutex<HashMap<u64, mpsc::Sender<Result<Value, RpcError>>>>,
    later: Mutex<HashSet<u64>>,
    next_id: AtomicU64,
    alive: AtomicBool,
    streams: Mutex<HashMap<u32, StreamBuf>>,
    stream_cv: Condvar,
    sending: Credits,
}

/// A connection to a backend. Cloning shares it.
#[derive(Clone)]
pub struct Connection(Arc<Shared>);

fn unavailable(why: &str) -> RpcError {
    RpcError::new(ErrorCode::Unavailable, why.to_owned())
        .hint("the connection to the backend is broken; it is resumed when it is back")
}

impl Connection {
    /// Starts reading `transport`; notifications, answers to [`Connection::send`] and the end
    /// arrive on the returned receiver.
    pub fn start(transport: Transport) -> (Self, mpsc::Receiver<Inbound>) {
        let Transport {
            mut reader,
            writer,
            closer,
        } = transport;
        let out = Arc::new(Outbox::default());
        out.spawn_writer(writer, Arc::clone(&closer));
        let shared = Arc::new(Shared {
            out,
            closer,
            waiting: Mutex::default(),
            later: Mutex::default(),
            next_id: AtomicU64::new(1),
            alive: AtomicBool::new(true),
            streams: Mutex::default(),
            stream_cv: Condvar::new(),
            sending: Credits::default(),
        });
        let (tx, rx) = mpsc::channel();
        let me = Arc::clone(&shared);
        std::thread::spawn(move || {
            let reason = loop {
                let frame = match read_frame(&mut reader) {
                    Ok(Some(f)) => f,
                    Ok(None) => break "the backend closed the connection".to_owned(),
                    Err(e) => break format!("connection closed: {e}"),
                };
                match frame.kind {
                    FrameKind::Message => {
                        let msg = match frame.text().map(Message::parse) {
                            Some(Ok(m)) => m,
                            _ => break "malformed message from the backend".to_owned(),
                        };
                        match msg {
                            Message::Response { id, result } => {
                                let Some(n) = id.as_u64() else {
                                    continue;
                                };
                                let waiter = me.waiting.lock().ok().and_then(|mut w| w.remove(&n));
                                match waiter {
                                    Some(w) => {
                                        let _ = w.send(result);
                                    }
                                    None => {
                                        if me.later.lock().is_ok_and(|mut l| l.remove(&n)) {
                                            let _ = tx.send(Inbound::Response { id: n, result });
                                        }
                                    }
                                }
                            }
                            Message::Notification { method, params } => {
                                let _ = tx.send(Inbound::Notification { method, params });
                            }
                            Message::Request { id, method, params } => {
                                let _ = tx.send(Inbound::Request { id, method, params });
                            }
                        }
                    }
                    FrameKind::Data => {
                        let n = frame.payload.len() as u32;
                        if let Ok(mut s) = me.streams.lock() {
                            s.entry(frame.stream)
                                .or_default()
                                .data
                                .extend(frame.payload);
                        }
                        me.stream_cv.notify_all();
                        // The data is consumed at once, so the credit goes back at once.
                        if let Ok(b) = Frame::credit(frame.stream, n).encode() {
                            me.out.push(b, Priority::Interactive);
                        }
                    }
                    FrameKind::Close => {
                        let reason = serde_json::from_slice::<Value>(&frame.payload)
                            .ok()
                            .and_then(|v| v["reason"].as_str().map(str::to_owned))
                            .unwrap_or_default();
                        if let Ok(mut s) = me.streams.lock() {
                            s.entry(frame.stream).or_default().done = Some(reason);
                        }
                        me.stream_cv.notify_all();
                    }
                    FrameKind::Credit => {
                        if let Some(n) = frame.credit_bytes() {
                            me.sending.grant(frame.stream, n);
                        }
                    }
                }
            };
            me.alive.store(false, Ordering::SeqCst);
            me.sending.close();
            if let Ok(mut w) = me.waiting.lock() {
                for (_, waiter) in w.drain() {
                    let _ = waiter.send(Err(unavailable(&reason)));
                }
            }
            me.stream_cv.notify_all();
            me.out.close();
            (me.closer)();
            let _ = tx.send(Inbound::Closed(reason));
        });
        (Self(shared), rx)
    }

    /// Whether the connection still works.
    pub fn alive(&self) -> bool {
        self.0.alive.load(Ordering::SeqCst) && !self.0.out.is_closed()
    }

    fn push(&self, msg: &Message) -> Result<(), RpcError> {
        if !self.alive() {
            return Err(unavailable("not connected"));
        }
        let bytes = Frame::message(msg.to_text())
            .encode()
            .map_err(|_| RpcError::new(ErrorCode::TooLarge, "the request is larger than 1 MiB"))?;
        if self.0.out.push(bytes, Priority::Interactive) {
            Ok(())
        } else {
            Err(unavailable("the connection is closed"))
        }
    }

    /// Sends a request and waits up to `limit` for its answer.
    pub fn call(&self, method: &str, params: Value, limit: Duration) -> Result<Value, RpcError> {
        let id = self.0.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        if let Ok(mut w) = self.0.waiting.lock() {
            w.insert(id, tx);
        }
        if let Err(e) = self.push(&Message::request(id, method, params)) {
            if let Ok(mut w) = self.0.waiting.lock() {
                w.remove(&id);
            }
            return Err(e);
        }
        match rx.recv_timeout(limit) {
            Ok(r) => r,
            Err(_) => {
                if let Ok(mut w) = self.0.waiting.lock() {
                    w.remove(&id);
                }
                Err(unavailable(&format!(
                    "no answer to {method} within {} s",
                    limit.as_secs()
                )))
            }
        }
    }

    /// Sends a request whose answer arrives later as [`Inbound::Response`]; returns its id.
    pub fn send(&self, method: &str, params: Value) -> Result<u64, RpcError> {
        let id = self.0.next_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut l) = self.0.later.lock() {
            l.insert(id);
        }
        self.push(&Message::request(id, method, params))?;
        Ok(id)
    }

    /// Answers a request the other side sent.
    pub fn respond(&self, id: Value, result: Result<Value, RpcError>) -> Result<(), RpcError> {
        self.push(&Message::Response { id, result })
    }

    /// Sends a notification.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        self.push(&Message::notification(method, params))
    }

    /// Cancels request `id` (`$/cancel`).
    pub fn cancel(&self, id: u64) -> Result<(), RpcError> {
        self.notify("$/cancel", json!({ "id": id }))
    }

    /// The whole content of backend stream `id`, waiting up to `limit`.
    pub fn stream(&self, id: u32, limit: Duration) -> Result<Vec<u8>, RpcError> {
        let deadline = Instant::now() + limit;
        let mut s = self.0.streams.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(buf) = s.get(&id)
                && let Some(reason) = &buf.done
            {
                if reason != "done" {
                    return Err(unavailable(&format!("stream {id} ended: {reason}")));
                }
                return Ok(s.remove(&id).map(|b| b.data).unwrap_or_default());
            }
            let now = Instant::now();
            if now >= deadline || !self.0.alive.load(Ordering::SeqCst) {
                return Err(unavailable("the stream did not finish"));
            }
            s = self
                .0
                .stream_cv
                .wait_timeout(s, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// What arrived on backend stream `id` so far (taken out), waiting up to `wait` for the first
    /// bytes, and the close reason once the stream ended (`done` for a normal end). For streams
    /// that go on (a terminal, a run, a forwarded connection).
    pub fn read_stream(&self, id: u32, wait: Duration) -> (Vec<u8>, Option<String>) {
        let deadline = Instant::now() + wait;
        let mut s = self.0.streams.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let ready = s
                .get(&id)
                .is_some_and(|b| !b.data.is_empty() || b.done.is_some());
            let gone = !self.0.alive.load(Ordering::SeqCst);
            if ready || gone || Instant::now() >= deadline {
                let Some(buf) = s.get_mut(&id) else {
                    return (
                        Vec::new(),
                        gone.then(|| "the connection is closed".to_owned()),
                    );
                };
                let data = std::mem::take(&mut buf.data);
                let done = buf.done.clone();
                if done.is_some() {
                    s.remove(&id);
                }
                let done = done.or_else(|| gone.then(|| "the connection is closed".to_owned()));
                return (data, done);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            s = self
                .0
                .stream_cv
                .wait_timeout(s, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Forgets what is buffered of stream `id` (the window stopped reading it).
    pub fn drop_stream(&self, id: u32) {
        if let Ok(mut s) = self.0.streams.lock() {
            s.remove(&id);
        }
        self.0.sending.remove(id);
    }

    /// Starts sending on backend stream `id` (terminal input, a forwarded connection): it gets
    /// the initial credit of `ADR-0026` §9.
    pub fn open_sending(&self, id: u32) {
        self.0.sending.open(id);
    }

    /// Writes `bytes` to stream `id`, waiting up to `limit` for the backend's credit.
    pub fn send_data(&self, id: u32, bytes: &[u8], limit: Duration) -> Result<(), RpcError> {
        let mut sent = 0;
        while sent < bytes.len() {
            if !self.alive() {
                return Err(unavailable("not connected"));
            }
            let want = (bytes.len() - sent).min(64 << 10);
            let n = self.0.sending.take(id, want, limit);
            if n == 0 {
                return Err(unavailable(&format!("no credit on stream {id}")));
            }
            let frame = Frame::data(id, bytes[sent..sent + n].to_vec())
                .encode()
                .map_err(|_| RpcError::new(ErrorCode::TooLarge, "chunk too large"))?;
            if !self.0.out.push(frame, Priority::Interactive) {
                return Err(unavailable("the connection is closed"));
            }
            sent += n;
        }
        Ok(())
    }

    /// Ends the window's side of stream `id` with `reason`.
    pub fn close_stream(&self, id: u32, reason: &str) {
        if let Ok(b) = Frame::close(id, reason).encode() {
            self.0.out.push(b, Priority::Interactive);
        }
        self.0.sending.remove(id);
    }

    /// Ends the connection.
    pub fn close(&self) {
        self.0.out.close();
        (self.0.closer)();
    }

    /// Queues raw bytes as they are (tests of malformed input).
    pub fn send_raw(&self, bytes: Vec<u8>) -> bool {
        self.0.out.push(bytes, Priority::Interactive)
    }
}
