//! Byte streams under the frames (`ADR-0026` §1) and the shared sending side: an outgoing queue
//! with two priorities and a cap, stream credit, cancellation handles, and the in-process pipe
//! the window (and most tests) use instead of SSH.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Each side caps its outgoing queue at this many bytes; more is a break (`ADR-0026` §9).
pub const QUEUE_CAP: usize = 32 << 20;

/// Credit each stream starts with, per direction.
pub const INITIAL_CREDIT: u64 = 256 << 10;

/// Message priority on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// `doc.*`, `session.*`, cancellation, answers.
    Interactive,
    /// `fs.changed` batches, stream data, progress.
    Bulk,
}

/// An ordered, reliable, already authenticated byte stream: SSH standard input and output, a
/// reattach socket or an in-process pipe.
pub struct Transport {
    /// The incoming bytes.
    pub reader: Box<dyn Read + Send>,
    /// The outgoing bytes.
    pub writer: Box<dyn Write + Send>,
    /// Ends the connection (shuts a socket down, closes a pipe).
    pub closer: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct Queue {
    hi: VecDeque<Vec<u8>>,
    lo: VecDeque<Vec<u8>>,
    bytes: usize,
    closed: bool,
    finishing: bool,
}

/// The outgoing frames of one connection; a writer thread sends them, interactive ones first.
#[derive(Default)]
pub struct Outbox {
    q: Mutex<Queue>,
    cv: Condvar,
}

impl Outbox {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.q.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Queues encoded frame bytes; `false` when the connection is closed or the queue is over its
    /// cap (the connection is then closed: a peer that stopped reading is a break).
    pub fn push(&self, bytes: Vec<u8>, priority: Priority) -> bool {
        let mut q = self.lock();
        if q.closed || q.finishing {
            return false;
        }
        if q.bytes + bytes.len() > QUEUE_CAP {
            q.closed = true;
            self.cv.notify_all();
            return false;
        }
        q.bytes += bytes.len();
        match priority {
            Priority::Interactive => q.hi.push_back(bytes),
            Priority::Bulk => q.lo.push_back(bytes),
        }
        self.cv.notify_all();
        true
    }

    /// Closes at once; queued frames are dropped.
    pub fn close(&self) {
        self.lock().closed = true;
        self.cv.notify_all();
    }

    /// Sends what is queued, then closes.
    pub fn finish(&self) {
        self.lock().finishing = true;
        self.cv.notify_all();
    }

    /// Whether the connection is closed.
    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// The next frame to send, waiting for one; `None` once closed.
    fn pop(&self) -> Option<Vec<u8>> {
        let mut q = self.lock();
        loop {
            if q.closed {
                return None;
            }
            if let Some(b) = q.hi.pop_front().or_else(|| q.lo.pop_front()) {
                q.bytes -= b.len();
                return Some(b);
            }
            if q.finishing {
                q.closed = true;
                return None;
            }
            q = self.cv.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Starts the thread that writes the queue to `writer`; it calls `closer` when done.
    pub fn spawn_writer(
        self: &Arc<Self>,
        mut writer: Box<dyn Write + Send>,
        closer: Arc<dyn Fn() + Send + Sync>,
    ) {
        let me = Arc::clone(self);
        std::thread::spawn(move || {
            while let Some(bytes) = me.pop() {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    me.close();
                    break;
                }
            }
            let _ = writer.flush();
            drop(writer);
            closer();
        });
    }
}

/// Credit of the streams one side sends on.
#[derive(Default)]
pub struct Credits {
    map: Mutex<HashMap<u32, u64>>,
    cv: Condvar,
}

impl Credits {
    /// A new stream with the initial credit.
    pub fn open(&self, stream: u32) {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(stream, INITIAL_CREDIT);
    }

    /// The peer granted `bytes` more.
    pub fn grant(&self, stream: u32, bytes: u32) {
        if let Some(c) = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&stream)
        {
            *c += u64::from(bytes);
        }
        self.cv.notify_all();
    }

    /// Takes up to `want` bytes of credit, waiting for some; `0` after `limit` or when the stream
    /// is gone. A sender without credit stops instead of buffering without bound.
    pub fn take(&self, stream: u32, want: usize, limit: Duration) -> usize {
        let deadline = std::time::Instant::now() + limit;
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match map.get_mut(&stream) {
                None => return 0,
                Some(c) if *c > 0 => {
                    let n = (*c).min(want as u64);
                    *c -= n;
                    return n as usize;
                }
                Some(_) => {}
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return 0;
            }
            map = self
                .cv
                .wait_timeout(map, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// The stream ended.
    pub fn remove(&self, stream: u32) {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&stream);
        self.cv.notify_all();
    }

    /// The connection is gone: every stream ends, and senders waiting for credit stop at once.
    pub fn close(&self) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.cv.notify_all();
    }

    /// Credit left (tests).
    pub fn left(&self, stream: u32) -> Option<u64> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&stream)
            .copied()
    }
}

/// How a request ended early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// `$/cancel`.
    Cancelled,
    /// `deadline_ms` passed.
    Deadline,
}

/// The cancellation handle a handler receives (`ADR-0012` rule 2).
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicU8>);

impl CancelToken {
    /// Cancels (the first reason wins).
    pub fn stop(&self, why: Stop) {
        let v = match why {
            Stop::Cancelled => 1,
            Stop::Deadline => 2,
        };
        let _ = self
            .0
            .compare_exchange(0, v, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Why the request was stopped, if it was.
    pub fn stopped(&self) -> Option<Stop> {
        match self.0.load(Ordering::SeqCst) {
            1 => Some(Stop::Cancelled),
            2 => Some(Stop::Deadline),
            _ => None,
        }
    }
}

#[derive(Default)]
struct PipeState {
    buf: VecDeque<u8>,
    closed: bool,
}

#[derive(Default)]
struct PipeShared {
    state: Mutex<PipeState>,
    cv: Condvar,
}

/// The reading end of an in-process pipe.
pub struct PipeReader(Arc<PipeShared>);

/// The writing end of an in-process pipe.
pub struct PipeWriter(Arc<PipeShared>);

/// A one-way in-memory pipe.
pub fn pipe() -> (PipeReader, PipeWriter) {
    let shared = Arc::new(PipeShared::default());
    (PipeReader(Arc::clone(&shared)), PipeWriter(shared))
}

fn close_pipe(shared: &PipeShared) {
    shared
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .closed = true;
    shared.cv.notify_all();
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut s = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if !s.buf.is_empty() {
                let n = out.len().min(s.buf.len());
                for (i, b) in s.buf.drain(..n).enumerate() {
                    out[i] = b;
                }
                return Ok(n);
            }
            if s.closed {
                return Ok(0);
            }
            s = self.0.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        close_pipe(&self.0);
    }
}

impl Write for PipeWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut s = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"));
        }
        s.buf.extend(data);
        self.0.cv.notify_all();
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        close_pipe(&self.0);
    }
}

/// Two connected in-process transports (window side, backend side); closing either side ends
/// both directions.
pub fn in_process() -> (Transport, Transport) {
    let (a_read, b_write) = pipe();
    let (b_read, a_write) = pipe();
    let (ra, wa) = (Arc::clone(&a_read.0), Arc::clone(&b_write.0));
    let (rb, wb) = (Arc::clone(&b_read.0), Arc::clone(&a_write.0));
    let closer: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        close_pipe(&ra);
        close_pipe(&wa);
        close_pipe(&rb);
        close_pipe(&wb);
    });
    (
        Transport {
            reader: Box::new(a_read),
            writer: Box::new(a_write),
            closer: Arc::clone(&closer),
        },
        Transport {
            reader: Box::new(b_read),
            writer: Box::new(b_write),
            closer,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_frames_go_first_and_the_cap_closes() {
        let out = Outbox::default();
        assert!(out.push(b"bulk".to_vec(), Priority::Bulk));
        assert!(out.push(b"doc".to_vec(), Priority::Interactive));
        assert_eq!(out.pop().unwrap(), b"doc");
        assert_eq!(out.pop().unwrap(), b"bulk");
        assert!(!out.push(vec![0; QUEUE_CAP + 1], Priority::Bulk));
        assert!(out.is_closed());
        assert!(out.pop().is_none());
        let done = Outbox::default();
        done.push(b"last".to_vec(), Priority::Bulk);
        done.finish();
        assert!(!done.push(b"late".to_vec(), Priority::Bulk));
        assert_eq!(done.pop().unwrap(), b"last");
        assert!(done.pop().is_none());
    }

    #[test]
    fn credit_stops_a_sender_until_granted() {
        let c = Arc::new(Credits::default());
        c.open(2);
        assert_eq!(c.take(2, 1 << 20, Duration::ZERO), INITIAL_CREDIT as usize);
        assert_eq!(c.take(2, 10, Duration::from_millis(20)), 0);
        let granter = Arc::clone(&c);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            granter.grant(2, 100);
        });
        assert_eq!(c.take(2, 1000, Duration::from_secs(5)), 100);
        t.join().unwrap();
        c.remove(2);
        assert_eq!(c.take(2, 10, Duration::from_secs(5)), 0);
        assert_eq!(c.left(2), None);
    }

    #[test]
    fn pipes_carry_bytes_and_close_both_ways() {
        let (mut a, mut b) = in_process();
        a.writer.write_all(b"ping").unwrap();
        let mut buf = [0u8; 4];
        b.reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        (a.closer)();
        assert_eq!(b.reader.read(&mut buf).unwrap(), 0);
        assert!(b.writer.write_all(b"x").is_err());
        let t = CancelToken::default();
        t.stop(Stop::Deadline);
        t.stop(Stop::Cancelled);
        assert_eq!(t.stopped(), Some(Stop::Deadline));
    }
}
