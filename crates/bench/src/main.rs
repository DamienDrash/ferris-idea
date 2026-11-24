//! Headless benchmark harness of `SPEC-0260`.
//!
//!   ferris-idea-bench run [--out FILE]       all headless phases, checked against the budgets
//!   ferris-idea-bench check <mode> FILE...    checks metric files (`{"name": number, …}` or result
//!                                          lines with a `metrics` object) against one mode's budgets
//!   ferris-idea-bench phase <name> <dir>      one phase in this process (used by `run`)
//!
//! Each phase runs in a child process so that its peak resident memory and CPU time are its own;
//! the parent samples them with `ps` from outside, and a phase reports the CPU time of its parts
//! as deltas of its own `ps` CPU time (`SPEC-0260` v6). Fixtures (a 100 000-line source file, two files
//! over 50 MB, the `java-large` project of `scripts/gen-java-large.sh`) are generated
//! once into `$FERRIS_IDEA_BENCH_DIR` (default `~/.cache/ferris-idea-bench`), where the result history
//! (`history.jsonl`) is kept as well — outside the repository.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use ferris_idea_bench::{Mode, budgets, check, percentile_ms, report};
use ferris_idea_core::{Finder, Query, Workspace};
use ferris_idea_editor::{Carets, EditorState, Position, perform, type_text};
use ferris_idea_languages::LanguageId;
use ferris_idea_languages::everywhere::{Categories, Sources, search_everywhere};
use ferris_idea_languages::inspections::{inspect, inspect_text};
use ferris_idea_languages::syntax::{MAX_SYNTAX_BYTES, SyntaxDocument};

type Metrics = BTreeMap<String, f64>;
type Error = Box<dyn std::error::Error>;

/// Phases in order; each one is a child process (`SPEC-0260` v6: the editor's steps for unused
/// declarations have their own phase, apart from indexing).
const PHASES: [&str; 4] = ["typing", "large", "index", "editor"];
/// Rows of a visible editor page (900 px window, 15 px lines).
const PAGE: usize = 60;
/// Size every large-file fixture reaches at least.
const LARGE_BYTES: usize = 50 * 1024 * 1024;
/// Further measurements of a phase with a failing value; the best value of each metric counts.
const RETRIES: usize = 2;
/// Load average per core above which failing wall-clock budgets are reported, not failed.
const OVERLOAD: f64 = 2.0;
/// Keystrokes applied by one background parse in the typing phase.
const COALESCED: usize = 5;
/// Bump when a fixture's content changes, so old fixtures are regenerated.
const FIXTURE_VERSION: &str = "1";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["run", rest @ ..] => run(rest),
        ["check", mode, files @ ..] if !files.is_empty() => check_files(mode, files),
        ["phase", name, dir] => phase(name, Path::new(dir)).map(|m| {
            println!("{}", serde_json::to_string(&m).expect("numbers serialise"));
            true
        }),
        _ => {
            eprintln!(
                "usage: ferris-idea-bench run [--out FILE] | check <headless|window> FILE... | phase <name> <dir>"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn bench_dir() -> PathBuf {
    ferris_idea_core::product_var_os("FERRIS_IDEA_BENCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
                .join(".cache/ferris-idea-bench")
        })
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs every phase, measures a phase again (up to `RETRIES` times) while one of its values fails
/// (the best value counts: a loaded machine makes timings only slower), appends the result to the
/// history and checks the headless budgets; on an overloaded machine failing wall-clock budgets are
/// reported as noisy instead of failing.
fn run(args: &[&str]) -> Result<bool, Error> {
    let out = match args {
        [] => None,
        ["--out", file] => Some(PathBuf::from(file)),
        _ => return Err("usage: ferris-idea-bench run [--out FILE]".into()),
    };
    let dir = bench_dir().join("fixtures");
    fixtures(&dir)?;
    let budgets = budgets();
    let mut metrics = Metrics::new();
    for name in PHASES {
        let mut m = child_phase(name, &dir)?;
        for retry in 1..=RETRIES {
            let failing = check(&budgets, Mode::Headless, &m)
                .iter()
                .any(|(_, v)| v.fails());
            if !failing {
                break;
            }
            eprintln!("phase {name} over budget, measuring again ({retry}/{RETRIES})");
            for (k, v) in child_phase(name, &dir)? {
                m.entry(k).and_modify(|old| *old = old.min(v)).or_insert(v);
            }
        }
        metrics.extend(m);
    }
    let line = serde_json::json!({
        "date": now(),
        "commit": git(&["rev-parse", "--short", "HEAD"]),
        "host": command_out("hostname", &["-s"]),
        "load_avg": load_average(),
        "mode": "headless",
        "metrics": metrics,
    });
    println!("{line}");
    let history = bench_dir().join("history.jsonl");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&history)?
        .write_all(format!("{line}\n").as_bytes())?;
    if let Some(out) = out {
        std::fs::write(out, format!("{line}\n"))?;
    }
    let results = check(&budgets, Mode::Headless, &metrics);
    eprint!("{}", report(&results));
    let missing = results
        .iter()
        .any(|(_, v)| *v == ferris_idea_bench::Verdict::Missing);
    // On a machine loaded far beyond its cores, wall-clock times say more about the other
    // processes than about Ferris IDEA; memory and CPU time still count.
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let load = load_average().unwrap_or(0.0);
    let overloaded = load > OVERLOAD * cores as f64;
    let mut failed = false;
    for (b, v) in &results {
        if !v.fails() {
            continue;
        }
        if overloaded && is_wall_clock(&b.metric) {
            eprintln!(
                "noisy {}: not counted, load {load:.1} on {cores} cores (SPEC-0260 behaviour 1)",
                b.metric
            );
        } else {
            failed = true;
        }
    }
    Ok(!missing && !failed)
}

/// Whether a metric is a wall-clock time (`…_ms`, `…_ms_p95`), as opposed to memory or CPU time.
fn is_wall_clock(metric: &str) -> bool {
    metric.ends_with("_ms") || metric.contains("_ms_")
}

/// Checks metric files written by the window job (or by `run`).
fn check_files(mode: &str, files: &[&str]) -> Result<bool, Error> {
    let mode = match mode {
        "headless" => Mode::Headless,
        "window" => Mode::Window,
        _ => return Err(format!("unknown mode {mode}").into()),
    };
    let mut metrics = Metrics::new();
    for f in files {
        for line in std::fs::read_to_string(f)?
            .lines()
            .filter(|l| l.starts_with('{'))
        {
            let v: serde_json::Value = serde_json::from_str(line)?;
            let map = v.get("metrics").unwrap_or(&v);
            if let Some(obj) = map.as_object() {
                for (k, x) in obj {
                    if let Some(x) = x.as_f64() {
                        metrics.insert(k.clone(), x);
                    }
                }
            }
        }
    }
    let budgets = budgets();
    let results = check(&budgets, mode, &metrics);
    print!("{}", report(&results));
    Ok(!results.iter().any(|(_, v)| v.fails()))
}

/// Runs one phase as a child and samples its resident memory and CPU time every 50 ms.
fn child_phase(name: &str, dir: &Path) -> Result<Metrics, Error> {
    let mut child = Command::new(std::env::current_exe()?)
        .args(["phase", name])
        .arg(dir)
        .stdout(Stdio::piped())
        .spawn()?;
    let pid = child.id().to_string();
    let (mut peak_kib, mut cpu) = (0u64, 0.0f64);
    while child.try_wait()?.is_none() {
        if let Some((rss, time)) = ps_sample(&pid) {
            peak_kib = peak_kib.max(rss);
            cpu = cpu.max(time);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!("phase {name} failed: {}", output.status).into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .rfind(|l| l.starts_with('{'))
        .ok_or("phase printed no result")?;
    let mut m: Metrics = serde_json::from_str(line)?;
    m.insert(format!("{name}_peak_rss_mib"), peak_kib as f64 / 1024.0);
    m.insert(format!("{name}_cpu_s"), cpu);
    Ok(m)
}

/// Resident memory (KiB) and CPU time (s) of a process, from `ps`.
fn ps_sample(pid: &str) -> Option<(u64, f64)> {
    let out = command_out("ps", &["-o", "rss=,time=", "-p", pid])?;
    let mut it = out.split_whitespace();
    let rss = it.next()?.parse().ok()?;
    Some((rss, cpu_seconds(it.next()?)?))
}

/// Seconds of a CPU time as `ps` writes it: `[[dd-]hh:]mm:ss.cc`.
fn cpu_seconds(time: &str) -> Option<f64> {
    let (days, rest) = time.split_once('-').unwrap_or(("0", time));
    let secs = rest.split(':').try_fold(0.0, |acc, part| {
        part.parse::<f64>().ok().map(|p| acc * 60.0 + p)
    })?;
    Some(days.parse::<f64>().ok()? * 86_400.0 + secs)
}

/// CPU time (user and system, all threads) this process has used so far, in seconds, read with
/// `ps` the way the parent samples a phase (`SPEC-0260` v6: the CPU time of a phase's parts).
fn own_cpu_s() -> Result<f64, Error> {
    ps_sample(&std::process::id().to_string())
        .map(|(_, cpu)| cpu)
        .ok_or_else(|| "ps gave no CPU time".into())
}

/// Seconds rounded to the 10 ms that `ps` resolves, for a difference of two [`own_cpu_s`].
fn centis(seconds: f64) -> f64 {
    (seconds * 100.0).round() / 100.0
}

fn command_out(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn now() -> Option<String> {
    command_out("date", &["+%Y-%m-%dT%H:%M:%S%z"])
}

/// One-minute load average, so noisy runs can be told apart in the history.
fn load_average() -> Option<f64> {
    let text = command_out("sysctl", &["-n", "vm.loadavg"])
        .or_else(|| std::fs::read_to_string("/proc/loadavg").ok())?;
    text.trim_start_matches(['{', ' '])
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

// ---------------------------------------------------------------------------------------------
// Fixtures

/// 100 000 lines of code-like Rust with umlauts (the text of the M3.4 editor spike).
fn hundred_k_lines() -> String {
    (0..100_000)
        .map(|i| match i % 5 {
            0 => format!("fn function_{i}(value: u64) -> Result<u64, Error> {{"),
            1 => format!("    let doubled = value.checked_mul(2).ok_or(Error::Overflow({i}))?;"),
            2 => "    // Grüße: äöüß — a longer comment line that fills most of the width".into(),
            3 => format!("    Ok(doubled + {i})"),
            _ => "}".into(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A 100 000-line Java class whose members use names of `java-large` and each other, held in
/// memory only (`SPEC-0260` v4): the editor's text for the step of its unused declarations.
fn hundred_k_java_lines() -> String {
    // 4 header lines, 8 lines per member, padding, the closing line: 100 000 lines.
    let mut out = String::from("package org.ferrisidea.sample;\n\npublic class Big {\n\n");
    let members = (100_000 - 5) / 8;
    for i in 0..members {
        // Every second member is used by the next one; the others are unused.
        let call = if i % 2 == 1 {
            format!("method{}(sum)", i - 1)
        } else {
            "0".to_owned()
        };
        out.push_str(&format!(
            "    public int field{i};\n    public int method{i}(int value) {{\n        int sum = value + field{i} + {call};\n        sum += new org.ferrisidea.sample.p00.Item{:03}().compute{}(sum);\n        // Grüße: äöüß — a comment line\n        return sum;\n    }}\n\n",
            i % 100,
            i % 8
        ));
    }
    for _ in 4 + members * 8..99_999 {
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

/// The Kotlin counterpart of [`hundred_k_java_lines`] (`SPEC-0260` v5): a 100 000-line class whose
/// members use names of `java-large` and each other, held in memory only.
fn hundred_k_kotlin_lines() -> String {
    // 4 header lines, 8 lines per member, padding, the closing line: 100 000 lines.
    let mut out = String::from("package org.ferrisidea.sample\n\nclass BigKt {\n\n");
    let members = (100_000 - 5) / 8;
    for i in 0..members {
        // Every second member is used by the next one; the others are unused.
        let call = if i % 2 == 1 {
            format!("method{}(sum)", i - 1)
        } else {
            "0".to_owned()
        };
        out.push_str(&format!(
            "    var field{i}: Int = 0\n    fun method{i}(value: Int): Int {{\n        var sum = value + field{i} + {call}\n        sum += org.ferrisidea.sample.p00.Item{:03}().compute{}(sum)\n        // Grüße: äöüß — a comment line\n        return sum\n    }}\n\n",
            i % 100,
            i % 8
        ));
    }
    for _ in 4 + members * 8..99_999 {
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

/// Writes `line(i)` for i = 0, 1, … until `LARGE_BYTES` is reached.
fn write_large(path: &Path, line: impl Fn(usize) -> String) -> Result<(), Error> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    let (mut written, mut i) = (0, 0);
    while written < LARGE_BYTES {
        let l = line(i);
        out.write_all(l.as_bytes())?;
        out.write_all(b"\n")?;
        written += l.len() + 1;
        i += 1;
    }
    out.flush()?;
    Ok(())
}

fn fixtures(dir: &Path) -> Result<(), Error> {
    let stamp = dir.join("VERSION");
    if std::fs::read_to_string(&stamp).is_ok_and(|v| v == FIXTURE_VERSION) {
        return Ok(());
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    eprintln!("generating fixtures in {}", dir.display());
    std::fs::write(dir.join("typing.rs"), hundred_k_lines())?;
    // A server log and an SQL dump: plain text and a language with a syntax tree.
    write_large(&dir.join("large.log"), |i| {
        format!(
            "2026-11-03T12:{:02}:{:02}.{:03}Z INFO  [worker-{}] request {i} handled in {} ms path=/api/items/{} user=u{}",
            i / 60 % 60,
            i % 60,
            i % 1000,
            i % 32,
            i % 97,
            i % 10_000,
            i % 500
        )
    })?;
    write_large(&dir.join("large.sql"), |i| {
        format!(
            "INSERT INTO movement (id, item_id, quantity, note) VALUES ({i}, {}, {}, 'restock batch {} for shelf {}');",
            i % 10_000,
            i % 50,
            i / 100,
            i % 400
        )
    })?;
    let status = Command::new("sh")
        .arg(repo_root().join("scripts/gen-java-large.sh"))
        .arg(dir.join("java-large"))
        .status()?;
    if !status.success() {
        return Err("gen-java-large.sh failed".into());
    }
    std::fs::write(stamp, FIXTURE_VERSION)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Phases

fn phase(name: &str, dir: &Path) -> Result<Metrics, Error> {
    match name {
        "typing" => typing(&dir.join("typing.rs")),
        "large" => {
            let mut m = large(
                &dir.join("large.log"),
                "large_log",
                "handled in 4",
                r"u4\d\b",
            )?;
            m.extend(large(
                &dir.join("large.sql"),
                "large_sql",
                "restock batch 4",
                r"shelf 39\d'",
            )?);
            Ok(m)
        }
        "index" => index(&dir.join("java-large")),
        "editor" => editor(&dir.join("java-large")),
        _ => Err(format!("unknown phase {name}").into()),
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn page_range(state: &EditorState, first: usize) -> std::ops::Range<usize> {
    let at = |line: usize| {
        state
            .buffer
            .position_to_offset(Position::new(line, 0))
            .unwrap_or(state.buffer.len_chars())
    };
    at(first)..at(first + PAGE)
}

/// What the editor view does for each keystroke in the middle of a 100 000-line file, split by
/// thread: the synchronous part before the frame (edit, snapshot for the document registry,
/// highlighting the visible page) and the background part (re-parse, inspections, fold regions),
/// whose results the main thread only installs.
fn typing(file: &Path) -> Result<Metrics, Error> {
    let text = std::fs::read_to_string(file)?;
    let mut state = EditorState::new(&text);
    state.buffer.track_changes();
    let mut doc = SyntaxDocument::new(LanguageId::Rust, false, &text)?;
    // The start of a statement inside a function body, in the middle of the file.
    let middle = state.buffer.position_to_offset(Position::new(50_001, 4))?;
    state.carets = Carets::new(middle);
    let mut keys: Vec<&str> = Vec::new();
    let typed: Vec<String> = "let grüße = 42;"
        .chars()
        .cycle()
        .take(120)
        .map(String::from)
        .collect();
    keys.extend(typed.iter().map(String::as_str));
    keys.extend(std::iter::repeat_n("EditorBackSpace", 60));
    keys.extend(std::iter::repeat_n("EditorDown", 30));
    keys.extend(std::iter::repeat_n("EditorEnter", 10));
    let (mut sync, mut background) = (Vec::new(), Vec::new());
    // Edits queue up while a parse runs and the next parse applies them together; with typing
    // much faster than a parse of this file that is several keystrokes per parse.
    let mut queued = Vec::new();
    let last = keys.len() - 1;
    for (n, key) in keys.into_iter().enumerate() {
        let t0 = Instant::now();
        if key.starts_with("Editor") {
            perform(&mut state, key);
        } else {
            type_text(&mut state, key);
        }
        let edits = state.buffer.take_changes();
        let _snapshot = state.buffer.snapshot();
        let line = state
            .buffer
            .offset_to_position(state.carets.primary().head)?
            .line;
        let range = page_range(&state, line.saturating_sub(PAGE / 2));
        let _spans = doc.highlight_range(range);
        sync.push(t0.elapsed());
        queued.extend(edits);
        if queued.is_empty() || (queued.len() < COALESCED && n < last) {
            continue;
        }
        let t1 = Instant::now();
        for e in std::mem::take(&mut queued) {
            let end = e.offset + e.deleted.chars().count();
            doc.edit(e.offset..end, &e.inserted)?;
        }
        let _found = inspect(&doc);
        let _folds = doc.folding();
        background.push(t1.elapsed());
    }
    let mut m = Metrics::new();
    for (name, v) in [
        ("typing_sync_ms", &sync),
        ("typing_background_ms", &background),
    ] {
        for (p, q) in [("p50", 0.5), ("p95", 0.95), ("p99", 0.99)] {
            m.insert(format!("{name}_{p}"), percentile_ms(v, q));
        }
    }
    Ok(m)
}

/// Opening, scrolling and searching a file over 50 MB, the way the editor view does it: the
/// synchronous open (read, buffer, the copy kept for reconciling with the disk, plain-text
/// inspections; no tree above `MAX_SYNTAX_BYTES`), page jumps, and find-all as the find bar runs it.
fn large(file: &Path, prefix: &str, search: &str, regex: &str) -> Result<Metrics, Error> {
    let mut m = Metrics::new();
    let t0 = Instant::now();
    let text = std::fs::read_to_string(file)?;
    let mut state = EditorState::new(&text);
    state.buffer.track_changes();
    let _disk = text.to_owned();
    let language = LanguageId::detect(file).filter(|_| text.len() <= MAX_SYNTAX_BYTES);
    if language.is_none() {
        let _found = inspect_text(file, &text);
    }
    m.insert(format!("{prefix}_open_ms"), ms(t0.elapsed()));
    let syntax = match language {
        Some(l) => {
            let t = Instant::now();
            let doc = SyntaxDocument::new(l, false, &text)?;
            let _found = inspect(&doc);
            let _folds = doc.folding();
            m.insert(format!("{prefix}_tree_ms"), ms(t.elapsed()));
            Some(doc)
        }
        None => None,
    };
    drop(text);
    // Scroll: 200 jumps to pseudo-random pages, each laid out and highlighted.
    let lines = state.buffer.len_lines();
    let mut jumps = Vec::new();
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..200 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let first = (seed % lines.saturating_sub(PAGE).max(1) as u64) as usize;
        let t = Instant::now();
        let range = page_range(&state, first);
        for l in first..first + PAGE {
            let _ = state.buffer.line(l);
        }
        if let Some(doc) = &syntax {
            let _ = doc.highlight_range(range);
        }
        jumps.push(t.elapsed());
    }
    m.insert(
        format!("{prefix}_scroll_ms_p95"),
        percentile_ms(&jumps, 0.95),
    );
    // Search: the find bar takes the buffer's text and lists every match.
    for (name, pattern, regex) in [("search", search, false), ("regex", regex, true)] {
        let t = Instant::now();
        let text = state.buffer.text();
        let finder = Finder::new(&Query {
            pattern: pattern.into(),
            regex,
            ..Query::default()
        })?;
        let hits = finder.find(&text).len();
        m.insert(format!("{prefix}_{name}_ms"), ms(t.elapsed()));
        m.insert(format!("{prefix}_{name}_hits"), hits as f64);
    }
    Ok(m)
}

/// Indexing `java-large` (5000 files): the file list, the first symbol search (every file parsed
/// for its tree-sitter definitions, as Search Everywhere does), a second one (definitions kept per
/// unchanged file) and the reference index the window builds when it opens its first Java file
/// are what opening the project and its first Java file cost (`SPEC-0260` v6: all of them count
/// toward `index_cpu_s`; the CPU time of the searches and of the reference index is reported too).
fn index(root: &Path) -> Result<Metrics, Error> {
    let mut m = Metrics::new();
    let cpu0 = own_cpu_s()?;
    let t0 = Instant::now();
    let workspace = Workspace::open(root)?;
    let files = workspace.files()?;
    m.insert("index_files_ms".into(), ms(t0.elapsed()));
    m.insert("index_files".into(), files.len() as f64);
    let keymap = ferris_idea_ui::keymap::Keymap::bundled_default();
    let sources = Sources {
        root: workspace.root(),
        files: &files,
        keymap: &keymap,
        extra_actions: &[],
        recent: &[],
        text_options: Default::default(),
        file_names: None,
    };
    let symbols = Categories {
        symbols: true,
        ..Categories::NONE
    };
    let t1 = Instant::now();
    let found = search_everywhere(sources, "Item042", symbols, 100, None);
    m.insert("index_symbols_ms".into(), ms(t1.elapsed()));
    // A second query, as typing on in Search Everywhere: files unchanged since the first.
    let t2 = Instant::now();
    let _ = search_everywhere(sources, "compute3", symbols, 100, None);
    m.insert("index_query_ms".into(), ms(t2.elapsed()));
    let hits = found
        .symbols
        .unwrap_or_default()
        .iter()
        .filter(|s| s.name == "Item042")
        .count();
    m.insert("index_symbol_hits".into(), hits as f64);
    let cpu1 = own_cpu_s()?;
    m.insert("index_symbols_cpu_s".into(), centis(cpu1 - cpu0));
    // v4: the reference index, built once as the window does in the background when it opens
    // its first Java file; v6: part of `index_cpu_s`.
    let t3 = Instant::now();
    build_reference_index(root);
    m.insert("index_reference_ms".into(), ms(t3.elapsed()));
    m.insert("index_reference_cpu_s".into(), centis(own_cpu_s()? - cpu1));
    Ok(m)
}

/// The reference index of the project at `root` over its files, built once and held in memory as
/// the window's background refresh leaves it (not saved to the user cache).
fn build_reference_index(root: &Path) {
    let rels = ferris_idea_languages::reference_index::project_files(root);
    let mut index = ferris_idea_languages::reference_index::ReferenceIndex::new(root);
    index.refresh(&rels);
    ferris_idea_languages::reference_index::hold(index);
}

/// The editor's step for the unused declarations of a 100 000-line Java file after each of 20
/// edits (`SPEC-0260` v4, SPEC-0098 v12 behaviour 26) and of a 100 000-line Kotlin file (v5,
/// SPEC-0098 v13 behaviour 27), against the reference index of `java-large`, which this phase
/// builds first because the steps need it held in the same process (v6: its own phase, so that
/// `index_cpu_s` is what opening the project costs). The CPU time of each file's 20 edits,
/// re-parses and steps is reported without a budget.
fn editor(root: &Path) -> Result<Metrics, Error> {
    let mut m = Metrics::new();
    build_reference_index(root);
    for (prefix, rel, language, text) in [
        (
            "editor_unused",
            "src/main/java/org/ferrisidea/sample/Big.java",
            LanguageId::Java,
            hundred_k_java_lines(),
        ),
        (
            "editor_unused_kotlin",
            "src/main/kotlin/org/ferrisidea/sample/BigKt.kt",
            LanguageId::Kotlin,
            hundred_k_kotlin_lines(),
        ),
    ] {
        let steps = editor_unused_steps(root, Path::new(rel), language, &text)?;
        m.insert(format!("{prefix}_ms_p95"), steps.p95_ms);
        m.insert(format!("{prefix}_findings"), steps.findings as f64);
        m.insert(format!("{prefix}_cpu_s"), steps.cpu_s);
    }
    Ok(m)
}

/// What [`editor_unused_steps`] measured.
struct Steps {
    /// p95 of the timed steps, in milliseconds.
    p95_ms: f64,
    /// Findings of the last step.
    findings: usize,
    /// CPU time of the 20 edits with their re-parses and steps, in seconds.
    cpu_s: f64,
}

/// 20 edits of `text` (the editor's file `rel` of `language`), each re-parsed and followed by the
/// timed step for its unused declarations against the held index of `root` (`SPEC-0260` v4, v5);
/// the first parse of the text, as opening the file does it, is not counted.
fn editor_unused_steps(
    root: &Path,
    rel: &Path,
    language: LanguageId,
    text: &str,
) -> Result<Steps, Error> {
    let mut doc = SyntaxDocument::new(language, false, text)?;
    let cpu0 = own_cpu_s()?;
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut steps = Vec::new();
    let mut found = 0;
    for n in 0..20usize {
        // A space at the start of a line in the middle of the file, re-parsed (the re-parse
        // budget's share, not timed here).
        let mid = doc.text().len() / 2 + 40 * n;
        let at = (mid..doc.text().len())
            .find(|&b| doc.text().as_bytes()[b] == b'\n')
            .map_or(0, |b| b + 1);
        let char_at = doc.text()[..at].chars().count();
        doc.edit(char_at..char_at, " ")?;
        let snapshot = doc.snapshot();
        let t = Instant::now();
        let findings =
            ferris_idea_languages::reference_index::editor_unused_of(root, rel, &snapshot, &cancel)
                .ok_or("the reference index is not ready")?;
        steps.push(t.elapsed());
        found = findings.len();
    }
    Ok(Steps {
        p95_ms: percentile_ms(&steps, 0.95),
        findings: found,
        cpu_s: centis(own_cpu_s()? - cpu0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_clock_metrics_are_told_from_memory_and_cpu_time() {
        for m in [
            "large_log_open_ms",
            "typing_background_ms_p95",
            "index_query_ms",
            "index_reference_ms",
            "editor_unused_ms_p95",
            "editor_unused_kotlin_ms_p95",
        ] {
            assert!(is_wall_clock(m), "{m}");
        }
        for m in [
            "large_peak_rss_mib",
            "index_cpu_s",
            "index_peak_rss_mib",
            "index_symbols_cpu_s",
            "index_reference_cpu_s",
            "editor_cpu_s",
            "editor_peak_rss_mib",
            "editor_unused_cpu_s",
            "editor_unused_kotlin_cpu_s",
        ] {
            assert!(!is_wall_clock(m), "{m}");
        }
        // Every headless budget is one or the other on purpose.
        assert!(
            budgets()
                .iter()
                .filter(|b| b.mode == Mode::Headless)
                .any(|b| !is_wall_clock(&b.metric))
        );
    }

    #[test]
    fn phase_wide_budgets_name_a_phase() {
        // `child_phase` writes `{phase}_cpu_s` and `{phase}_peak_rss_mib`: a headless budget of
        // either kind is only measured when its phase exists (SPEC-0260 v6: `editor`).
        let phase_wide: Vec<String> = budgets()
            .into_iter()
            .filter(|b| b.mode == Mode::Headless)
            .map(|b| b.metric)
            .filter(|m| m.ends_with("_peak_rss_mib") || m.ends_with("_cpu_s"))
            .collect();
        for m in &phase_wide {
            let phase = m
                .strip_suffix("_peak_rss_mib")
                .or_else(|| m.strip_suffix("_cpu_s"))
                .expect("filtered above");
            assert!(PHASES.contains(&phase), "{m}");
        }
        for m in ["index_cpu_s", "index_peak_rss_mib", "editor_peak_rss_mib"] {
            assert!(phase_wide.iter().any(|x| x == m), "{m}");
        }
        // The editor's steps are their own phase; indexing keeps the file list, the symbol
        // searches and the reference index.
        assert_eq!(PHASES.last(), Some(&"editor"));
    }

    #[test]
    fn cpu_time_of_ps_is_read_in_seconds() {
        // macOS `mm:ss.cc` and `hh:mm:ss.cc`, procps `[dd-]hh:mm:ss`.
        assert_eq!(cpu_seconds("0:03.45"), Some(3.45));
        assert_eq!(cpu_seconds("1:02:03.50"), Some(3723.5));
        assert_eq!(cpu_seconds("00:00:16"), Some(16.0));
        assert_eq!(cpu_seconds("2-01:00:00"), Some(2.0 * 86_400.0 + 3600.0));
        assert_eq!(cpu_seconds("soon"), None);
        assert_eq!(centis(0.1 + 0.2), 0.3);
        // This process, as a phase reads its parts.
        assert!(own_cpu_s().is_ok_and(|s| s >= 0.0));
    }
}
