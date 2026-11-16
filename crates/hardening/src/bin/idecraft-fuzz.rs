//! Runs the fuzz targets with the stable mutation driver (M8.2).
//!
//! ```text
//! idecraft-fuzz list
//! idecraft-fuzz run [TARGET…] [--secs N | --iterations N] [--seed N] [--corpus DIR] [--out DIR]
//! idecraft-fuzz replay TARGET FILE…
//! idecraft-fuzz seeds DIR          # writes every target's seeds to DIR/<target>/ for cargo fuzz
//! ```
//!
//! `run` exits with status 1 when a target panicked and writes each crashing input to
//! `--out` (default `fuzz-crashes/`).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use idecraft_hardening::{Budget, TARGETS, run, target};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => {
            for t in TARGETS {
                println!("{:<18} {}", t.name, t.about);
            }
            ExitCode::SUCCESS
        }
        Some("run") => run_targets(&args[1..]),
        Some("replay") if args.len() >= 3 => replay(&args[1], &args[2..]),
        Some("seeds") if args.len() == 2 => {
            for t in TARGETS {
                let dir = PathBuf::from(&args[1]).join(t.name);
                std::fs::create_dir_all(&dir).expect("seed folder");
                for (i, s) in t.seeds.iter().enumerate() {
                    std::fs::write(dir.join(format!("seed-{i}")), s).expect("seed file");
                }
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: idecraft-fuzz list | run [TARGET…] [--secs N|--iterations N] [--seed N] [--corpus DIR] [--out DIR] | replay TARGET FILE… | seeds DIR"
            );
            ExitCode::from(2)
        }
    }
}

fn run_targets(args: &[String]) -> ExitCode {
    let mut names = Vec::new();
    let mut budget = Budget::time(Duration::from_secs(10));
    let mut seed = 0x1dec_4af7_u64;
    let mut corpus: Option<PathBuf> = None;
    let mut out = PathBuf::from("fuzz-crashes");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().unwrap_or_default();
        match a.as_str() {
            "--secs" => {
                budget = Budget::time(Duration::from_secs_f64(value().parse().unwrap_or(10.0)))
            }
            "--iterations" => budget = Budget::iterations(value().parse().unwrap_or(1000)),
            "--seed" => seed = value().parse().unwrap_or(seed),
            "--corpus" => corpus = Some(PathBuf::from(value())),
            "--out" => out = PathBuf::from(value()),
            name => names.push(name.to_owned()),
        }
    }
    let selected: Vec<_> = if names.is_empty() {
        TARGETS.iter().collect()
    } else {
        names
            .iter()
            .map(|n| target(n).unwrap_or_else(|| panic!("no target `{n}`")))
            .collect()
    };
    let mut failed = false;
    for t in selected {
        let extra: Vec<Vec<u8>> = corpus
            .as_ref()
            .and_then(|c| std::fs::read_dir(c.join(t.name)).ok())
            .into_iter()
            .flatten()
            .filter_map(|e| std::fs::read(e.ok()?.path()).ok())
            .filter(|b| b.len() <= 64 << 10)
            .collect();
        let report = run(t, budget, seed, &extra);
        println!(
            "{:<18} {:>9} inputs in {:>6.1} s ({:>7.0}/s), slowest {:>7.1} ms, crashes {}",
            report.target,
            report.iterations,
            report.elapsed.as_secs_f64(),
            report.iterations as f64 / report.elapsed.as_secs_f64().max(1e-9),
            report.slowest.0.as_secs_f64() * 1000.0,
            report.crashes.len()
        );
        for (i, c) in report.crashes.iter().enumerate() {
            failed = true;
            std::fs::create_dir_all(&out).expect("crash folder");
            let file = out.join(format!("{}-{i}", t.name));
            std::fs::write(&file, &c.input).expect("crash file");
            println!("  crash: {}\n    input: {}", c.message, file.display());
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn replay(name: &str, files: &[String]) -> ExitCode {
    let Some(t) = target(name) else {
        eprintln!("no target `{name}`");
        return ExitCode::from(2);
    };
    for f in files {
        let input = std::fs::read(f).expect("input file");
        println!("{f}: {} bytes", input.len());
        (t.run)(&input);
    }
    println!("no panic");
    ExitCode::SUCCESS
}
