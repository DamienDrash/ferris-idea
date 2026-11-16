//! A small mutation fuzzer for the stable toolchain: no coverage feedback, but seeds, a per-target
//! dictionary and byte-level mutations find the shallow crashes quickly and run anywhere
//! `cargo test` runs. Deep runs use `cargo fuzz` (libFuzzer) on the same targets.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::targets::Target;

/// How long one target runs: whichever limit comes first.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// Most inputs tried.
    pub iterations: u64,
    /// Most wall-clock time.
    pub time: Duration,
}

impl Budget {
    /// At most `iterations` inputs, without a time limit.
    pub fn iterations(iterations: u64) -> Self {
        Self {
            iterations,
            time: Duration::MAX,
        }
    }

    /// As many inputs as fit in `time`.
    pub fn time(time: Duration) -> Self {
        Self {
            iterations: u64::MAX,
            time,
        }
    }
}

/// An input that made a target panic.
#[derive(Debug, Clone)]
pub struct Crash {
    /// The input.
    pub input: Vec<u8>,
    /// The panic message (from whichever thread panicked).
    pub message: String,
}

/// The outcome of one target's run.
#[derive(Debug, Clone)]
pub struct Report {
    /// Target name.
    pub target: &'static str,
    /// Inputs tried.
    pub iterations: u64,
    /// Time taken.
    pub elapsed: Duration,
    /// Inputs that panicked; one per distinct message.
    pub crashes: Vec<Crash>,
    /// The slowest input and its time (a hang shows up here when it ends at all).
    pub slowest: (Duration, Vec<u8>),
}

/// Panic messages seen while a target runs, from any thread (a server task panics off-thread).
static PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// xorshift64*: deterministic for a seed, good enough to pick mutations.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

const INTERESTING: &[u8] = &[
    0, 1, 0x7f, 0x80, 0xff, b'\n', b'\r', b'\0', b'"', b'\\', 0x1b,
];
const MAX_LEN: usize = 16 << 10;

fn mutate(rng: &mut Rng, input: &mut Vec<u8>, target: &Target, pool: &[Vec<u8>]) {
    for _ in 0..1 + rng.below(6) {
        let len = input.len();
        match rng.below(10) {
            0 if len > 0 => {
                let i = rng.below(len);
                input[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                input[i] = INTERESTING[rng.below(INTERESTING.len())];
            }
            2 => {
                let at = rng.below(len + 1);
                let n = 1 + rng.below(8);
                let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                input.splice(at..at, bytes);
            }
            3 if len > 0 => {
                let at = rng.below(len);
                let n = 1 + rng.below((len - at).min(32));
                input.drain(at..at + n);
            }
            4 if len > 0 => {
                // Repeat a slice: long runs, deep nesting, big numbers.
                let at = rng.below(len);
                let n = 1 + rng.below((len - at).min(16));
                let piece = input[at..at + n].to_vec();
                for _ in 0..1 + rng.below(16) {
                    input.splice(at..at, piece.iter().copied());
                }
            }
            5 | 6 if !target.dict.is_empty() => {
                let word = target.dict[rng.below(target.dict.len())];
                let at = rng.below(len + 1);
                input.splice(at..at, word.iter().copied());
            }
            7 if !pool.is_empty() => {
                // Splice: a prefix of this input and a suffix of another.
                let other = &pool[rng.below(pool.len())];
                let cut = rng.below(len + 1);
                let from = rng.below(other.len() + 1);
                input.truncate(cut);
                input.extend_from_slice(&other[from..]);
            }
            8 if len > 1 => {
                let at = rng.below(len);
                input.truncate(at.max(1));
            }
            _ if len > 0 => {
                // Digits: lengths, counts and coordinates.
                let i = rng.below(len);
                input[i] = b'0' + rng.below(10) as u8;
            }
            _ => input.push(rng.next() as u8),
        }
    }
    input.truncate(MAX_LEN);
}

/// Runs `target` within `budget`, starting from its seeds; `seed` makes a run repeatable.
pub fn run(target: &Target, budget: Budget, seed: u64, extra_seeds: &[Vec<u8>]) -> Report {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let text = match info.payload().downcast_ref::<&str>() {
            Some(s) => (*s).to_owned(),
            None => info
                .payload()
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "panic".into()),
        };
        let at = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        if let Ok(mut panics) = PANICS.lock() {
            panics.push(format!("{text}{at}"));
        }
    }));
    let mut pool: Vec<Vec<u8>> = target.seeds.iter().map(|s| s.to_vec()).collect();
    pool.extend(extra_seeds.iter().cloned());
    if pool.is_empty() {
        pool.push(Vec::new());
    }
    let mut rng = Rng(seed | 1);
    let start = Instant::now();
    let mut report = Report {
        target: target.name,
        iterations: 0,
        elapsed: Duration::ZERO,
        crashes: Vec::new(),
        slowest: (Duration::ZERO, Vec::new()),
    };
    let seeds = pool.len();
    while report.iterations < budget.iterations && start.elapsed() < budget.time {
        let n = report.iterations as usize;
        let input = if n < seeds {
            pool[n].clone()
        } else {
            let mut input = pool[rng.below(pool.len())].clone();
            mutate(&mut rng, &mut input, target, &pool);
            input
        };
        PANICS.lock().map(|mut p| p.clear()).ok();
        let t = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| (target.run)(&input)));
        let took = t.elapsed();
        if took > report.slowest.0 {
            report.slowest = (took, input.clone());
        }
        let panics: Vec<String> = PANICS.lock().map(|p| p.clone()).unwrap_or_default();
        if outcome.is_err() || !panics.is_empty() {
            let message = panics.first().cloned().unwrap_or_else(|| "panic".into());
            let site = |m: &str| m.rsplit(" at ").next().map(str::to_owned);
            if !report
                .crashes
                .iter()
                .any(|c| site(&c.message) == site(&message))
            {
                report.crashes.push(Crash {
                    input: input.clone(),
                    message,
                });
            }
        } else if n >= seeds && pool.len() < 256 && rng.below(64) == 0 {
            // Keep a few survivors as new starting points.
            pool.push(input);
        }
        report.iterations += 1;
    }
    report.elapsed = start.elapsed();
    std::panic::set_hook(previous);
    report
}
