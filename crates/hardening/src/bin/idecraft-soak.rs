//! Long-session soak test (M8.2): drives `idecraft mcp` and watches memory, file descriptors,
//! child processes and answer times.
//!
//! ```text
//! idecraft-soak --server target/debug/idecraft [--minutes N | --secs N] [--seed N]
//!               [--workspace DIR] [--sample-secs N] [--churn-every ROUNDS] [--timeout-secs N]
//!               [--max-rss-growth-mb N] [--max-fd-growth N] [--no-lsp] [--csv FILE]
//! ```
//!
//! Exits with status 1 when a check failed.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use idecraft_hardening::soak::{Config, run};

fn main() -> ExitCode {
    let mut config = Config::new(
        PathBuf::from("target/debug/idecraft"),
        Duration::from_secs(120),
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().unwrap_or_default();
        let number =
            |v: String| -> f64 { v.parse().unwrap_or_else(|_| panic!("{a} needs a number")) };
        match a.as_str() {
            "--server" => config.server = PathBuf::from(value()),
            "--minutes" => config.duration = Duration::from_secs_f64(number(value()) * 60.0),
            "--secs" => config.duration = Duration::from_secs_f64(number(value())),
            "--seed" => config.seed = number(value()) as u64,
            "--workspace" => config.workspace = Some(PathBuf::from(value())),
            "--sample-secs" => config.sample_every = Duration::from_secs_f64(number(value())),
            "--churn-every" => config.churn_every = number(value()) as u64,
            "--timeout-secs" => config.request_timeout = Duration::from_secs_f64(number(value())),
            "--max-rss-growth-mb" => config.max_rss_growth_mb = number(value()) as u64,
            "--max-fd-growth" => config.max_fd_growth = number(value()) as usize,
            "--no-lsp" => config.language_servers = false,
            "--csv" => config.csv = Some(PathBuf::from(value())),
            other => {
                eprintln!("unknown argument {other}; see the usage in the source header");
                return ExitCode::from(2);
            }
        }
    }
    println!(
        "soak: {} for {:.0} s, sample every {:.0} s",
        config.server.display(),
        config.duration.as_secs_f64(),
        config.sample_every.as_secs_f64()
    );
    let outcome = run(&config, |s| {
        println!(
            "{:>7.0} s  rounds {:>6}  rss {:>6.1} MiB  fds {:>4}  children {:>2}",
            s.elapsed,
            s.rounds,
            s.rss_kb as f64 / 1024.0,
            s.fds,
            s.children
        );
    });
    println!(
        "rounds {}, requests {}, short sessions {}, language servers {}, slowest answer {:.0} ms ({})",
        outcome.rounds,
        outcome.requests,
        outcome.sessions,
        if outcome.language_servers {
            "used"
        } else {
            "not used"
        },
        outcome.slowest.as_secs_f64() * 1000.0,
        outcome.slowest_request
    );
    if outcome.passed() {
        println!("soak passed");
        ExitCode::SUCCESS
    } else {
        for f in &outcome.failures {
            println!("FAILED: {f}");
        }
        ExitCode::FAILURE
    }
}
