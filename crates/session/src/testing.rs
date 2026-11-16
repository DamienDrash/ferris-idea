//! Helpers for property tests: a small seeded generator (no test crate needed, runs repeat
//! exactly for a seed), and a transport with added delay (a slow network, `SPEC-0317` AK-8).

use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::transport::Transport;

/// `transport` with `delay` added to everything it reads: a thread reads ahead and hands each
/// chunk on once it is `delay` old, so a request's answer arrives `delay` later (added round trip)
/// while the bytes keep their order and nothing is held back longer.
pub fn delayed(transport: Transport, delay: Duration) -> Transport {
    if delay.is_zero() {
        return transport;
    }
    let Transport {
        mut reader,
        writer,
        closer,
    } = transport;
    let (tx, rx) = mpsc::channel::<(Instant, Vec<u8>)>();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 64 << 10];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((Instant::now(), buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    Transport {
        reader: Box::new(Delayed {
            rx,
            delay,
            chunk: Vec::new(),
            at: 0,
        }),
        writer,
        closer,
    }
}

struct Delayed {
    rx: mpsc::Receiver<(Instant, Vec<u8>)>,
    delay: Duration,
    chunk: Vec<u8>,
    at: usize,
}

impl Read for Delayed {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.at >= self.chunk.len() {
            let Ok((arrived, bytes)) = self.rx.recv() else {
                return Ok(0);
            };
            let due = arrived + self.delay;
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            self.chunk = bytes;
            self.at = 0;
        }
        let n = out.len().min(self.chunk.len() - self.at);
        out[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// A xorshift64* generator.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator for `seed`.
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number below `n` (0 when `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// `n` characters, also beyond the Basic Multilingual Plane and line breaks.
    pub fn text(&mut self, n: usize) -> String {
        const CHARS: [char; 10] = ['a', 'b', 'c', ' ', '\n', 'ä', '€', '😀', '𝄞', 'z'];
        (0..n).map(|_| CHARS[self.below(CHARS.len())]).collect()
    }
}
