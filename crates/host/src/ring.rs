//! Output kept for a window that reattaches (`SPEC-0318` behaviours 19 and 23): the last 1 MiB of
//! a terminal's or a run's output, and the stream it currently goes to. Offsets count the bytes
//! produced since the start, so a window can reattach where it stopped (v2 protocol detail 5).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use idecraft_session::StreamOut;

/// Bytes kept per output.
pub const KEPT: usize = 1 << 20;

#[derive(Default)]
struct Inner {
    kept: VecDeque<u8>,
    /// Bytes produced since the start; `kept` holds the last of them.
    total: u64,
    out: Option<Arc<StreamOut>>,
    ended: Option<String>,
}

impl Inner {
    /// Offset of the oldest kept byte.
    fn first(&self) -> u64 {
        self.total - self.kept.len() as u64
    }
}

/// One output: what was produced lately and where it goes now.
#[derive(Clone, Default)]
pub struct Output(Arc<Mutex<Inner>>);

impl Output {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keeps `bytes` and sends them to the current stream; a stream that fails is dropped (the
    /// window reattaches later).
    pub fn push(&self, bytes: &[u8]) {
        let out = {
            let mut g = self.lock();
            g.kept.extend(bytes);
            g.total += bytes.len() as u64;
            let over = g.kept.len().saturating_sub(KEPT);
            g.kept.drain(..over);
            g.out.clone()
        };
        if let Some(out) = out
            && !out.write(bytes)
        {
            let mut g = self.lock();
            if g.out.as_ref().is_some_and(|o| Arc::ptr_eq(o, &out)) {
                g.out = None;
            }
        }
    }

    /// Where a stream attached at `from` starts: `from` when it is still kept, else the oldest kept
    /// byte (`None`: everything kept).
    pub fn start_for(&self, from: Option<u64>) -> u64 {
        let g = self.lock();
        let first = g.first();
        from.map_or(first, |f| f.clamp(first, g.total))
    }

    /// Sends the output from offset `start` (see [`Output::start_for`]) to `out`, then everything
    /// new; replaces an earlier stream. Output produced while the kept part is sent is sent too:
    /// the stream takes over under the lock only once it has caught up.
    pub fn attach(&self, out: StreamOut, start: u64) {
        let out = Arc::new(out);
        let mut sent = {
            let mut g = self.lock();
            if let Some(old) = g.out.take() {
                old.close("replaced");
            }
            start.clamp(g.first(), g.total)
        };
        loop {
            let chunk = {
                let mut g = self.lock();
                if sent < g.first() {
                    // More than the kept size arrived meanwhile: go on from the oldest kept byte.
                    sent = g.first();
                }
                if sent == g.total {
                    match g.ended.clone() {
                        Some(reason) => {
                            drop(g);
                            out.close(&reason);
                        }
                        None => g.out = Some(Arc::clone(&out)),
                    }
                    return;
                }
                let skip = (sent - g.first()) as usize;
                let chunk: Vec<u8> = g.kept.iter().skip(skip).copied().collect();
                sent = g.total;
                chunk
            };
            if !out.write(&chunk) {
                return;
            }
        }
    }

    /// The producer ended: the stream is closed with `reason`.
    pub fn end(&self, reason: &str) {
        let out = {
            let mut g = self.lock();
            g.ended = Some(reason.to_owned());
            g.out.take()
        };
        if let Some(out) = out {
            out.close(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_count_everything_produced_and_start_inside_what_is_kept() {
        let o = Output::default();
        o.push(b"hello ");
        o.push(b"world");
        assert_eq!(o.lock().total, 11);
        assert_eq!(o.start_for(None), 0);
        assert_eq!(o.start_for(Some(6)), 6);
        assert_eq!(o.start_for(Some(99)), 11);
        let big = vec![b'x'; KEPT];
        o.push(&big);
        assert_eq!(o.start_for(Some(3)), 11, "the oldest kept byte");
    }
}
