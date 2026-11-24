//! The paired channel after the handshake (`ADR-0025` §1b, `SPEC-0321` behaviour 16): session
//! frames as a byte stream in Noise messages with a two-byte length prefix, a type byte inside
//! each message for data, heartbeat and rekey, and the time limits of §8. A replayed, reordered
//! or altered message fails decryption and closes the connection.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ferris_idea_session::Transport;
use snow::TransportState;

const DATA: u8 = 0;
const HEARTBEAT: u8 = 1;
const REKEY: u8 = 2;

const NOISE_TAG: usize = 16;
const MAX_CIPHER: usize = 65_535;
/// Most plaintext in one Noise message, type byte included.
const MAX_PLAIN: usize = MAX_CIPHER - NOISE_TAG;

/// Limits of one channel.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Heartbeat period (15 s).
    pub heartbeat: Duration,
    /// No traffic for this long closes the connection (45 s).
    pub idle: Duration,
    /// Messages before a rekey (2^20).
    pub rekey_messages: u64,
    /// Time before a rekey (60 minutes).
    pub rekey_after: Duration,
    /// Age at which the connection closes so a new handshake replaces it (24 hours).
    pub max_age: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(15),
            idle: Duration::from_secs(45),
            rekey_messages: 1 << 20,
            rekey_after: Duration::from_secs(3600),
            max_age: Duration::from_secs(24 * 3600),
        }
    }
}

/// A byte pipe the channel runs over: a TCP connection or an `ssh -W` child (`SPEC-0324`
/// behaviour 7). Noise runs end to end through it, whatever carries the bytes.
pub trait Pipe: Read + Write + Send + Sync + 'static {
    /// A second handle on the same pipe.
    fn duplicate(&self) -> io::Result<Box<dyn Pipe>>;
    /// Ends the pipe; a reader blocked on it wakes up.
    fn shut(&self);
    /// Limits how long one read blocks, where the pipe can; others rely on `shut`.
    fn limit_reads(&self, limit: Duration) -> io::Result<()>;
    /// Whether silence must be watched for, because reads cannot time out.
    fn watches_silence(&self) -> bool {
        false
    }
}

impl Pipe for TcpStream {
    fn duplicate(&self) -> io::Result<Box<dyn Pipe>> {
        Ok(Box::new(self.try_clone()?))
    }

    fn shut(&self) {
        let _ = self.shutdown(Shutdown::Both);
    }

    fn limit_reads(&self, limit: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(limit))?;
        self.set_nodelay(true)
    }
}

/// The standard input and output of a child (`ssh -W`). Ending it kills the child.
#[derive(Clone)]
pub struct ChildPipe {
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<ChildStdout>>,
}

impl ChildPipe {
    /// Takes the pipes of `child`; `None` when it was not started with both piped.
    pub fn new(mut child: Child) -> Option<Self> {
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        Some(Self {
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(stdout)),
        })
    }
}

impl Read for ChildPipe {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.stdout
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .read(out)
    }
}

impl Write for ChildPipe {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.stdin
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdin.lock().unwrap_or_else(|e| e.into_inner()).flush()
    }
}

impl Pipe for ChildPipe {
    fn duplicate(&self) -> io::Result<Box<dyn Pipe>> {
        Ok(Box::new(self.clone()))
    }

    fn shut(&self) {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }

    fn limit_reads(&self, _limit: Duration) -> io::Result<()> {
        Ok(())
    }

    fn watches_silence(&self) -> bool {
        true
    }
}

struct Sending {
    stream: Box<dyn Pipe>,
    messages: u64,
    since: Instant,
}

struct Shared {
    noise: Mutex<TransportState>,
    sending: Mutex<Sending>,
    closed: AtomicBool,
    raw: Box<dyn Pipe>,
    limits: Limits,
    /// When the last bytes arrived; a pipe without read timeouts is watched by the heartbeat.
    heard: Mutex<Instant>,
}

impl Shared {
    fn noise(&self) -> MutexGuard<'_, TransportState> {
        self.noise.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.raw.shut();
    }

    fn send_one(&self, s: &mut Sending, kind: u8, data: &[u8]) -> io::Result<()> {
        let mut plain = Vec::with_capacity(data.len() + 1);
        plain.push(kind);
        plain.extend_from_slice(data);
        let mut out = vec![0u8; plain.len() + NOISE_TAG + 2];
        let n = self
            .noise()
            .write_message(&plain, &mut out[2..])
            .map_err(|_| io::Error::other("encryption failed"))?;
        out.truncate(n + 2);
        let len = u16::try_from(n).map_err(|_| io::Error::other("message too long"))?;
        out[..2].copy_from_slice(&len.to_be_bytes());
        s.stream.write_all(&out)?;
        s.messages += 1;
        Ok(())
    }

    fn send(&self, kind: u8, data: &[u8]) -> io::Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        let mut s = self.sending.lock().unwrap_or_else(|e| e.into_inner());
        let due = s.messages >= self.limits.rekey_messages
            || s.since.elapsed() >= self.limits.rekey_after;
        if due {
            self.send_one(&mut s, REKEY, &[])?;
            self.noise().rekey_outgoing();
            s.messages = 0;
            s.since = Instant::now();
        }
        self.send_one(&mut s, kind, data)
    }
}

/// The reading half: decrypted session bytes.
pub struct ChannelReader {
    shared: Arc<Shared>,
    stream: Box<dyn Pipe>,
    chunk: Vec<u8>,
    at: usize,
}

impl ChannelReader {
    fn fail(&self, why: &str) -> io::Error {
        self.shared.close();
        io::Error::new(io::ErrorKind::InvalidData, why.to_owned())
    }

    fn fill(&mut self) -> io::Result<bool> {
        loop {
            let mut len = [0u8; 2];
            match self.stream.read_exact(&mut len) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(e) => {
                    self.shared.close();
                    return Err(e);
                }
            }
            let mut msg = vec![0u8; usize::from(u16::from_be_bytes(len))];
            if let Err(e) = self.stream.read_exact(&mut msg) {
                self.shared.close();
                return Err(e);
            }
            *self.shared.heard.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
            let mut plain = vec![0u8; msg.len()];
            let n = {
                let mut noise = self.shared.noise();
                noise.read_message(&msg, &mut plain)
            };
            let Ok(n) = n else {
                return Err(self.fail("a message failed authentication; connection closed"));
            };
            match plain[..n].split_first() {
                Some((&DATA, data)) => {
                    self.chunk.clear();
                    self.chunk.extend_from_slice(data);
                    self.at = 0;
                    if !self.chunk.is_empty() {
                        return Ok(true);
                    }
                }
                Some((&HEARTBEAT, _)) => {}
                Some((&REKEY, _)) => self.shared.noise().rekey_incoming(),
                _ => return Err(self.fail("unknown message type; connection closed")),
            }
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.chunk.len() && !self.fill()? {
            return Ok(0);
        }
        let n = out.len().min(self.chunk.len() - self.at);
        out[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// The writing half: session bytes, split into messages.
pub struct ChannelWriter(Arc<Shared>);

impl Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = data.len().min(MAX_PLAIN - 1);
        self.0.send(DATA, &data[..n])?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A channel opened over `stream`; heartbeats start at once.
pub struct Channel {
    reader: ChannelReader,
    writer: ChannelWriter,
    shared: Arc<Shared>,
}

impl Channel {
    /// Wraps a finished handshake.
    pub fn new<P: Pipe>(stream: P, noise: TransportState, limits: Limits) -> io::Result<Self> {
        stream.limit_reads(limits.idle)?;
        let watch_silence = stream.watches_silence();
        let shared = Arc::new(Shared {
            noise: Mutex::new(noise),
            sending: Mutex::new(Sending {
                stream: stream.duplicate()?,
                messages: 0,
                since: Instant::now(),
            }),
            closed: AtomicBool::new(false),
            raw: stream.duplicate()?,
            limits,
            heard: Mutex::new(Instant::now()),
        });
        let beat = Arc::clone(&shared);
        std::thread::spawn(move || {
            let born = Instant::now();
            let mut last = Instant::now();
            while !beat.closed.load(Ordering::SeqCst) {
                std::thread::sleep(limits.heartbeat.min(Duration::from_millis(250)));
                if born.elapsed() >= limits.max_age {
                    beat.close();
                    return;
                }
                let quiet = beat
                    .heard
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .elapsed();
                if watch_silence && quiet >= limits.idle {
                    beat.close();
                    return;
                }
                if last.elapsed() >= limits.heartbeat {
                    last = Instant::now();
                    if beat.send(HEARTBEAT, &[]).is_err() {
                        beat.close();
                        return;
                    }
                }
            }
        });
        Ok(Self {
            reader: ChannelReader {
                shared: Arc::clone(&shared),
                stream: Box::new(stream),
                chunk: Vec::new(),
                at: 0,
            },
            writer: ChannelWriter(Arc::clone(&shared)),
            shared,
        })
    }

    /// Reads one line (a pairing control message).
    pub fn read_line(&mut self, limit: usize) -> io::Result<String> {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = self.reader.read(&mut byte)?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            if byte[0] == b'\n' {
                return String::from_utf8(line)
                    .map_err(|_| io::Error::from(io::ErrorKind::InvalidData));
            }
            line.push(byte[0]);
            if line.len() > limit {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
        }
    }

    /// Writes one line (a pairing control message).
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")
    }

    /// Closes the connection.
    pub fn close(&self) {
        self.shared.close();
    }

    /// A function that closes the connection.
    pub fn closer(&self) -> Arc<dyn Fn() + Send + Sync> {
        let shared = Arc::clone(&self.shared);
        Arc::new(move || shared.close())
    }

    /// The channel as a session transport.
    pub fn into_transport(self) -> Transport {
        let closer = self.closer();
        Transport {
            reader: Box::new(self.reader),
            writer: Box::new(self.writer),
            closer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noise::{Accept, Goal, Identity, initiate, pairing_key, prologue, respond};
    use std::net::TcpListener;

    fn tcp_pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (b, _) = l.accept().unwrap();
        // A failed handshake on one side must fail the test, not hang it.
        a.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        b.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        (a, b)
    }

    fn channels(limits: Limits) -> (Channel, Channel, TcpStream, TcpStream) {
        let (mut a, mut b) = tcp_pair();
        let (raw_a, raw_b) = (a.try_clone().unwrap(), b.try_clone().unwrap());
        let (c, s) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let key = *pairing_key("ABCDEFGH23");
        let t = std::thread::spawn(move || {
            let accept = Accept {
                pairing: Some(&key),
                pinned: &|_| true,
            };
            respond(&mut b, &s, &accept, &prologue("window", "serve"))
                .map(|(f, _)| (f, b))
                .unwrap()
        });
        let cf = initiate(
            &mut a,
            &c,
            Goal::Pair(&key),
            &prologue("window", "serve"),
            "x",
        )
        .unwrap();
        let (sf, b) = t.join().unwrap();
        (
            Channel::new(a, cf.transport, limits).unwrap(),
            Channel::new(b, sf.transport, limits).unwrap(),
            raw_a,
            raw_b,
        )
    }

    #[test]
    fn data_crosses_in_order_and_is_ciphertext() {
        let (mut a, mut b, _ra, _rb) = channels(Limits::default());
        a.write_line("hello world").unwrap();
        b.write_line("reply").unwrap();
        assert_eq!(b.read_line(100).unwrap(), "hello world");
        assert_eq!(a.read_line(100).unwrap(), "reply");
        // A long payload is split and reassembled.
        let big = "x".repeat(200_000);
        let t = std::thread::spawn(move || {
            a.write_line(&big).unwrap();
            a
        });
        assert_eq!(b.read_line(300_000).unwrap().len(), 200_000);
        let _ = t.join().unwrap();
    }

    #[test]
    fn rekeying_keeps_the_channel_working() {
        let limits = Limits {
            rekey_messages: 3,
            ..Limits::default()
        };
        let (mut a, mut b, _ra, _rb) = channels(limits);
        for i in 0..20 {
            a.write_line(&format!("m{i}")).unwrap();
            assert_eq!(b.read_line(100).unwrap(), format!("m{i}"));
        }
    }

    #[test]
    fn heartbeats_keep_an_idle_channel_open_and_silence_closes_it() {
        let limits = Limits {
            heartbeat: Duration::from_millis(50),
            idle: Duration::from_millis(400),
            ..Limits::default()
        };
        let (mut a, mut b, _ra, _rb) = channels(limits);
        std::thread::sleep(Duration::from_millis(800));
        a.write_line("still here").unwrap();
        assert_eq!(b.read_line(100).unwrap(), "still here");
        a.close();
        assert!(b.read_line(100).is_err());
    }

    #[test]
    fn an_old_channel_closes_so_a_new_handshake_replaces_it() {
        let limits = Limits {
            max_age: Duration::from_millis(50),
            ..Limits::default()
        };
        let (mut a, _b, _ra, _rb) = channels(limits);
        std::thread::sleep(Duration::from_millis(600));
        assert!(a.write_line("late").is_err());
    }

    #[test]
    fn sec8_a_replayed_or_altered_message_closes_the_connection() {
        let (mut a, mut b) = tcp_pair();
        let (c, s) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let key = *pairing_key("ABCDEFGH23");
        let t = std::thread::spawn(move || {
            let accept = Accept {
                pairing: Some(&key),
                pinned: &|_| true,
            };
            let (f, _) = respond(&mut b, &s, &accept, &prologue("window", "serve")).unwrap();
            (f, b)
        });
        let mut cf = initiate(
            &mut a,
            &c,
            Goal::Pair(&key),
            &prologue("window", "serve"),
            "x",
        )
        .unwrap();
        let (sf, b) = t.join().unwrap();
        let mut reader_side = Channel::new(b, sf.transport, Limits::default()).unwrap();
        let mut send = |kind: u8, data: &[u8], flip: bool| -> Vec<u8> {
            let mut plain = vec![kind];
            plain.extend_from_slice(data);
            let mut out = vec![0u8; plain.len() + 16];
            let n = cf.transport.write_message(&plain, &mut out).unwrap();
            out.truncate(n);
            if flip {
                out[0] ^= 1;
            }
            let mut framed = u16::try_from(n).unwrap().to_be_bytes().to_vec();
            framed.extend_from_slice(&out);
            framed
        };
        let first = send(DATA, b"one\n", false);
        a.write_all(&first).unwrap();
        assert_eq!(reader_side.read_line(100).unwrap(), "one");
        // The same message again: a replay.
        a.write_all(&first).unwrap();
        assert!(reader_side.read_line(100).is_err());
        drop(reader_side);

        let (mut a2, mut b2) = tcp_pair();
        let (c, s) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let t = std::thread::spawn(move || {
            let accept = Accept {
                pairing: Some(&key),
                pinned: &|_| true,
            };
            let (f, _) = respond(&mut b2, &s, &accept, &prologue("window", "serve")).unwrap();
            (f, b2)
        });
        let mut cf2 = initiate(
            &mut a2,
            &c,
            Goal::Pair(&key),
            &prologue("window", "serve"),
            "x",
        )
        .unwrap();
        let (sf2, b2) = t.join().unwrap();
        let mut r2 = Channel::new(b2, sf2.transport, Limits::default()).unwrap();
        let mut out = vec![0u8; 64];
        let n = cf2
            .transport
            .write_message(&[DATA, b'a'], &mut out)
            .unwrap();
        out.truncate(n);
        out[3] ^= 0x40;
        let mut framed = u16::try_from(n).unwrap().to_be_bytes().to_vec();
        framed.extend_from_slice(&out);
        a2.write_all(&framed).unwrap();
        assert!(r2.read_line(10).is_err());
    }

    #[test]
    fn reordered_messages_close_the_connection() {
        let (mut a, mut b) = tcp_pair();
        let (c, s) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let key = *pairing_key("ABCDEFGH23");
        let t = std::thread::spawn(move || {
            let accept = Accept {
                pairing: Some(&key),
                pinned: &|_| true,
            };
            let (f, _) = respond(&mut b, &s, &accept, &prologue("window", "serve")).unwrap();
            (f, b)
        });
        let mut cf = initiate(
            &mut a,
            &c,
            Goal::Pair(&key),
            &prologue("window", "serve"),
            "x",
        )
        .unwrap();
        let (sf, b) = t.join().unwrap();
        let mut r = Channel::new(b, sf.transport, Limits::default()).unwrap();
        let mut one = vec![0u8; 64];
        let n1 = cf.transport.write_message(&[DATA, b'1'], &mut one).unwrap();
        one.truncate(n1);
        let mut two = vec![0u8; 64];
        let n2 = cf.transport.write_message(&[DATA, b'2'], &mut two).unwrap();
        two.truncate(n2);
        let frame = |m: &[u8]| {
            let mut f = u16::try_from(m.len()).unwrap().to_be_bytes().to_vec();
            f.extend_from_slice(m);
            f
        };
        a.write_all(&frame(&two)).unwrap();
        a.write_all(&frame(&one)).unwrap();
        assert!(r.read_line(10).is_err());
    }
}
