//! Long-lived answers of the Engine API (`SPEC-0119` v5 behaviours 9, 10): the event stream and
//! followed logs. The connection reads with a short timeout so a stop request is seen within half a
//! second without a busy loop; a chunked body is decoded as it arrives.

use std::io::Read;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::DockerError;
use crate::docker::{message, parse_head, parse_response};

/// How long one read of a stream waits before it looks at its stop flag.
pub(crate) const POLL: Duration = Duration::from_millis(500);

/// Asks a stream or a watcher to end; clones share the flag. Waiting on it wakes at once when it
/// is set, so pauses between reconnects need no polling.
#[derive(Debug, Clone, Default)]
pub struct StopFlag(Arc<(Mutex<bool>, Condvar)>);

impl StopFlag {
    /// A flag that is not set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the flag and wakes every waiter.
    pub fn stop(&self) {
        let (lock, cv) = &*self.0;
        if let Ok(mut s) = lock.lock() {
            *s = true;
        }
        cv.notify_all();
    }

    /// Whether the flag is set.
    pub fn is_stopped(&self) -> bool {
        self.0.0.lock().map(|s| *s).unwrap_or(true)
    }

    /// Waits up to `d`; true when the flag was set meanwhile.
    pub fn wait(&self, d: Duration) -> bool {
        let (lock, cv) = &*self.0;
        let Ok(guard) = lock.lock() else { return true };
        match cv.wait_timeout_while(guard, d, |s| !*s) {
            Ok((s, _)) => *s,
            Err(_) => true,
        }
    }
}

/// Whether an I/O error is only the read timeout of a quiet stream.
pub(crate) fn idle(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}

/// Decodes a chunked body while it arrives; a read that times out keeps what was read so far, so
/// it can be called again.
pub(crate) struct Chunked<R> {
    inner: R,
    state: ChunkState,
}

enum ChunkState {
    /// Reading the size line.
    Size(Vec<u8>),
    /// Bytes left in the current chunk.
    Data(usize),
    /// Bytes of the CRLF after a chunk left to skip.
    End(usize),
    /// The last chunk was read.
    Done,
    /// Not chunked: bytes pass through.
    Plain,
}

impl<R: Read> Chunked<R> {
    pub(crate) fn new(inner: R, chunked: bool) -> Self {
        Self {
            inner,
            state: if chunked {
                ChunkState::Size(Vec::new())
            } else {
                ChunkState::Plain
            },
        }
    }
}

impl<R: Read> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match &mut self.state {
                ChunkState::Plain => return self.inner.read(buf),
                ChunkState::Done => return Ok(0),
                ChunkState::Size(line) => {
                    let mut b = [0u8; 1];
                    if self.inner.read(&mut b)? == 0 {
                        return Ok(0);
                    }
                    line.push(b[0]);
                    if line.len() > 1024 {
                        return Err(std::io::Error::other("chunk size line too long"));
                    }
                    if line.ends_with(b"\r\n") {
                        let text = String::from_utf8_lossy(&line[..line.len() - 2]).into_owned();
                        let size =
                            usize::from_str_radix(text.split(';').next().unwrap_or("").trim(), 16)
                                .map_err(|_| {
                                    std::io::Error::other(format!("bad chunk size {text:?}"))
                                })?;
                        self.state = if size == 0 {
                            ChunkState::Done
                        } else {
                            ChunkState::Data(size)
                        };
                    }
                }
                ChunkState::Data(left) => {
                    let want = (*left).min(buf.len());
                    if want == 0 {
                        return Ok(0);
                    }
                    let n = self.inner.read(&mut buf[..want])?;
                    if n == 0 {
                        return Ok(0);
                    }
                    *left -= n;
                    if *left == 0 {
                        self.state = ChunkState::End(2);
                    }
                    return Ok(n);
                }
                ChunkState::End(left) => {
                    let mut b = [0u8; 2];
                    let n = self.inner.read(&mut b[..*left])?;
                    if n == 0 {
                        return Ok(0);
                    }
                    *left -= n;
                    if *left == 0 {
                        self.state = ChunkState::Size(Vec::new());
                    }
                }
            }
        }
    }
}

impl crate::DockerHost {
    /// Sends a request and returns the decoded body as a stream that reads with a [`POLL`]
    /// timeout (where the connection allows one), or the daemon's error.
    pub(crate) fn open_stream(
        &self,
        method: &str,
        path: &str,
    ) -> Result<Box<dyn Read + Send>, DockerError> {
        let mut conn = self.send(method, path, None, false)?;
        conn.read_timeout(Some(POLL));
        // The head byte by byte, so no byte of the body is taken with it.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        let mut waited = Duration::ZERO;
        while !head.ends_with(b"\r\n\r\n") {
            match conn.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                Err(e) if idle(&e) && waited < crate::docker::TIMEOUT => waited += POLL,
                _ => return Err(DockerError::Parse("no answer from Docker".into())),
            }
            if head.len() > 16384 {
                return Err(DockerError::Parse("answer head too long".into()));
            }
        }
        let (status, _) = parse_head(&head)?;
        let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        let chunked = text
            .lines()
            .any(|l| l.starts_with("transfer-encoding:") && l.contains("chunked"));
        if status >= 300 {
            conn.read_timeout(Some(crate::docker::TIMEOUT));
            let mut rest = head.clone();
            let _ = conn.read_to_end(&mut rest);
            let body = parse_response(&rest).map(|(_, b)| b).unwrap_or_default();
            return Err(DockerError::Http(status, message(&body)));
        }
        Ok(Box::new(Chunked::new(conn, chunked)))
    }
}

/// Reads a stream until it ends (or fails) or `stop` is set, giving each piece to `on`; a quiet
/// stream only makes it look at `stop` again.
pub(crate) fn read_chunks(
    mut stream: Box<dyn Read + Send>,
    stop: &StopFlag,
    mut on: impl FnMut(&[u8]),
) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        if stop.is_stopped() {
            return Ok(());
        }
        match stream.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => on(&buf[..n]),
            Err(e) if idle(&e) => {}
            Err(e) => return Err(e),
        }
    }
}

/// [`read_chunks`] split into lines; `on` gets each complete line without its line end.
pub(crate) fn read_lines(
    stream: Box<dyn Read + Send>,
    stop: &StopFlag,
    mut on: impl FnMut(&str),
) -> std::io::Result<()> {
    let mut pending: Vec<u8> = Vec::new();
    read_chunks(stream, stop, |bytes| {
        pending.extend_from_slice(bytes);
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            on(String::from_utf8_lossy(&line[..line.len() - 1]).trim_end_matches('\r'));
        }
        if pending.len() > 1 << 20 {
            pending.clear();
        }
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A reader that hands out its parts one per read and times out at each `None`.
    pub(crate) struct Parts(pub(crate) Vec<Option<Vec<u8>>>);

    impl Read for Parts {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            match self.0.remove(0) {
                None => Err(std::io::ErrorKind::WouldBlock.into()),
                Some(mut p) => {
                    let n = p.len().min(buf.len());
                    let rest = p.split_off(n);
                    buf[..n].copy_from_slice(&p);
                    if !rest.is_empty() {
                        self.0.insert(0, Some(rest));
                    }
                    Ok(n)
                }
            }
        }
    }

    #[test]
    fn chunked_bodies_survive_timeouts_anywhere() {
        let parts = vec![
            Some(b"5\r".to_vec()),
            None,
            Some(b"\nhel".to_vec()),
            None,
            Some(b"lo\r\n3;x=1\r\n w\n\r".to_vec()),
            None,
            Some(b"\n0\r\n\r\n".to_vec()),
        ];
        let mut r = Chunked::new(Parts(parts), true);
        let mut out = Vec::new();
        let mut buf = [0u8; 4];
        loop {
            match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if idle(&e) => {}
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(out, b"hello w\n");
    }

    #[test]
    fn lines_across_reads_and_the_stop_flag() {
        let stop = StopFlag::new();
        let mut seen = Vec::new();
        let stream: Box<dyn Read + Send> = Box::new(Parts(vec![
            Some(b"{\"a\":1}\n{\"b\"".to_vec()),
            None,
            Some(b":2}\r\nrest".to_vec()),
        ]));
        read_lines(stream, &stop, |l| seen.push(l.to_owned())).unwrap();
        assert_eq!(seen, ["{\"a\":1}", "{\"b\":2}"]);
        stop.stop();
        assert!(stop.is_stopped());
        assert!(
            stop.wait(Duration::from_secs(5)),
            "a set flag does not wait"
        );
        let quiet = StopFlag::new();
        let started = std::time::Instant::now();
        assert!(!quiet.wait(Duration::from_millis(30)));
        assert!(started.elapsed() >= Duration::from_millis(30));
    }
}
