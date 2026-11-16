//! Long-session soak test (M8.2): drives a real `idecraft mcp <workspace>` process for minutes or
//! hours and watches it from outside.
//!
//! Each round saves a workspace file from outside (as another editor would), reads it back, types
//! and undoes in an editor, records and restores local history, queries a SQLite connection,
//! runs and renders terminal output, and now and then asks the language server for diagnostics.
//! Every few rounds a second, short-lived server starts, uses a language server and a terminal,
//! and ends; none of its child processes may outlive it.
//!
//! Watched: resident memory, open file descriptors and child processes of the long-lived server,
//! and the time every request takes (a request without an answer within the timeout counts as a
//! stuck task). The outcome lists every failed check.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// What to run and how strictly to judge it.
#[derive(Debug, Clone)]
pub struct Config {
    /// The `idecraft` binary.
    pub server: PathBuf,
    /// How long the long-lived server is driven.
    pub duration: Duration,
    /// Picks the order of operations.
    pub seed: u64,
    /// Workspace folder (created and filled); a temporary one when `None`.
    pub workspace: Option<PathBuf>,
    /// Time between resource samples.
    pub sample_every: Duration,
    /// Rounds between short-lived server sessions.
    pub churn_every: u64,
    /// Longest wait for any answer.
    pub request_timeout: Duration,
    /// Allowed growth of resident memory from the warmed-up level to the end, in MiB.
    pub max_rss_growth_mb: u64,
    /// Allowed growth of open file descriptors from the warmed-up level to the end.
    pub max_fd_growth: usize,
    /// Ask language servers for diagnostics (needs Pyright).
    pub language_servers: bool,
    /// One line per sample, as CSV.
    pub csv: Option<PathBuf>,
}

impl Config {
    /// Defaults for `server` and `duration`.
    pub fn new(server: PathBuf, duration: Duration) -> Self {
        Self {
            server,
            duration,
            seed: 0x50a4,
            workspace: None,
            sample_every: Duration::from_secs(10),
            churn_every: 40,
            request_timeout: Duration::from_secs(60),
            max_rss_growth_mb: 96,
            max_fd_growth: 24,
            language_servers: true,
            csv: None,
        }
    }
}

/// Resources of the long-lived server at one moment.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Seconds since the start.
    pub elapsed: f64,
    /// Rounds done.
    pub rounds: u64,
    /// Resident memory in KiB.
    pub rss_kb: u64,
    /// Open file descriptors.
    pub fds: usize,
    /// Processes below the server.
    pub children: usize,
}

/// What a soak run found.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Rounds done.
    pub rounds: u64,
    /// Requests answered.
    pub requests: u64,
    /// Short-lived sessions started and ended.
    pub sessions: u64,
    /// Slowest answer.
    pub slowest: Duration,
    /// Name of the slowest request.
    pub slowest_request: String,
    /// Resource samples in order.
    pub samples: Vec<Sample>,
    /// Whether language-server diagnostics ran.
    pub language_servers: bool,
    /// Every failed check.
    pub failures: Vec<String>,
}

impl Outcome {
    /// Whether every check passed.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// xorshift64*, as in the fuzz driver.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) % n.max(1)
    }
}

/// One `idecraft mcp` process and its stdio.
struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next: u64,
    timeout: Duration,
    requests: u64,
    slowest: (Duration, String),
}

impl Server {
    fn start(
        binary: &Path,
        root: &Path,
        config_dir: &Path,
        log: &Path,
        timeout: Duration,
    ) -> Result<Self, String> {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| format!("server log: {e}"))?;
        let mut child = Command::new(binary)
            .arg("mcp")
            .arg(root)
            .env("IDECRAFT_CONFIG_DIR", config_dir)
            // The soak project is trusted, so its tools run (SPEC-0278 behaviour 10).
            .env("IDECRAFT_TRUST_PROJECTS", "all")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .map_err(|e| format!("start {}: {e}", binary.display()))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut server = Self {
            child,
            stdin,
            lines,
            next: 0,
            timeout,
            requests: 0,
            slowest: (Duration::ZERO, String::new()),
        };
        server.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "soak", "version": "0"}}),
        )?;
        server.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        Ok(server)
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("server input closed: {e}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let label = match params.get("name").and_then(Value::as_str) {
            Some(tool) => tool.to_owned(),
            None => method.to_owned(),
        };
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let start = Instant::now();
        loop {
            let left = self.timeout.saturating_sub(start.elapsed());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if v["id"] != json!(id) {
                        continue;
                    }
                    let took = start.elapsed();
                    self.requests += 1;
                    if took > self.slowest.0 {
                        self.slowest = (took, label);
                    }
                    if let Some(e) = v.get("error") {
                        return Err(format!("{method} failed: {e}"));
                    }
                    return Ok(v["result"].clone());
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "stuck: no answer to {label} within {} s",
                        self.timeout.as_secs()
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("server ended during {label}"));
                }
            }
        }
    }

    /// Calls `tool`: the text of its answer, or the error text.
    fn call_text(&mut self, tool: &str, arguments: Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}))?;
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if result["isError"] == json!(true) {
            return Err(format!("{tool}: {text}"));
        }
        Ok(text)
    }

    /// Calls `tool`: its JSON answer (or text), or the error text.
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        let text = self.call_text(tool, arguments)?;
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// Ends the server by closing its input; the processes below it that are still alive 10 s
    /// after it ended.
    fn close(mut self) -> Vec<(u32, String)> {
        let below = descendants(self.pid());
        drop(self.stdin);
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            return vec![(
                self.child.id(),
                "the server itself: did not end within 20 s of its input closing".into(),
            )];
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let alive: Vec<(u32, String)> = below
                .iter()
                .filter(|(pid, _)| alive(*pid))
                .cloned()
                .collect();
            if alive.is_empty() || Instant::now() > deadline {
                return alive;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// `(pid, ppid, command)` of every process.
fn processes() -> Vec<(u32, u32, String)> {
    let out = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            Some((pid, ppid, it.collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

/// Every process below `pid`, with its command.
fn descendants(pid: u32) -> Vec<(u32, String)> {
    let all = processes();
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut parents = vec![pid];
    while let Some(parent) = parents.pop() {
        for (p, pp, comm) in &all {
            if *pp == parent && !out.iter().any(|(q, _)| q == p) {
                out.push((*p, comm.clone()));
                parents.push(*p);
            }
        }
    }
    out
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn rss_kb(pid: u32) -> u64 {
    Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

fn open_fds(pid: u32) -> usize {
    if let Ok(dir) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        return dir.count();
    }
    // macOS: one `f` line per descriptor.
    Command::new("lsof")
        .args(["-n", "-P", "-F", "f", "-p", &pid.to_string()])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.starts_with('f') && l[1..].chars().all(|c| c.is_ascii_digit()))
                .count()
        })
        .unwrap_or(0)
}

const FILES: usize = 24;

fn file_name(i: usize) -> String {
    match i % 4 {
        0 => format!("src/m{i}.py"),
        1 => format!("src/q{i}.sql"),
        2 => format!("docs/n{i}.md"),
        _ => format!("src/t{i}.txt"),
    }
}

fn file_text(i: usize, round: u64) -> String {
    match i % 4 {
        0 => format!(
            "def f{round}(a: int) -> int:\n    return a + {round}\n\n\nx: str = f{round}(1)\n"
        ),
        1 => format!("select {round};\nselect 'a;b' from t where id = {i};\n-- {round}\n"),
        2 => format!("# Note {i}\n\nRound {round}: ä ö ü 🙂\n"),
        _ => format!("line one\r\nline two {round}\r\n"),
    }
}

/// Creates the workspace: files, a SQLite database and the grant of the `run` right.
fn prepare(root: &Path) -> Result<PathBuf, String> {
    let io = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(root.join("src")).map_err(io)?;
    std::fs::create_dir_all(root.join("docs")).map_err(io)?;
    for i in 0..FILES {
        std::fs::write(root.join(file_name(i)), file_text(i, 0)).map_err(io)?;
    }
    let db = rusqlite::Connection::open(root.join("soak.db")).map_err(|e| e.to_string())?;
    db.execute_batch(
        "create table if not exists item(id integer primary key, name text, qty integer);
         delete from item;
         with recursive n(i) as (select 1 union all select i + 1 from n where i < 2000)
         insert into item select i, 'item ' || i, i % 97 from n;",
    )
    .map_err(|e| e.to_string())?;
    let config_dir = root.with_extension("config");
    std::fs::create_dir_all(&config_dir).map_err(io)?;
    let canonical = root.canonicalize().map_err(io)?;
    std::fs::write(
        config_dir.join(idecraft_core::GRANT_FILE),
        format!(
            "[[workspace]]\npath = {:?}\nallow = [\"run\", \"write\"]\n",
            canonical.display().to_string()
        ),
    )
    .map_err(io)?;
    Ok(config_dir)
}

/// One round of work on the long-lived server.
fn round(
    server: &mut Server,
    root: &Path,
    n: u64,
    rng: &mut Rng,
    lsp: &mut bool,
) -> Result<(), String> {
    let i = rng.below(FILES as u64) as usize;
    let rel = file_name(i);
    // Saved from outside, read back through MCP.
    let text = file_text(i, n);
    std::fs::write(root.join(&rel), &text).map_err(|e| e.to_string())?;
    let read = server.call_text("read_file", json!({"path": rel}))?;
    let expected: Vec<&str> = text.lines().collect();
    if read != expected.join("\n") {
        return Err(format!("read_file {rel}: not what was saved"));
    }
    // Typing and undoing it leaves the text as it was.
    let typed = server.call(
        "editor_keystrokes",
        json!({"path": rel, "keymap": "macos", "steps": [{"text": "zz"}, {"keys": "meta Z"}, {"keys": "meta Z"}]}),
    )?;
    if typed["text"].as_str() != Some(text.as_str()) {
        return Err(format!(
            "editor_keystrokes {rel}: typing and undo changed the text"
        ));
    }
    // Local history records the save; now and then a restore goes back one revision.
    let history = server.call("local_history", json!({"path": rel}))?;
    let revisions = history["revisions"].as_array().cloned().unwrap_or_default();
    if n % 5 == 4 && revisions.len() >= 2 {
        let id = revisions[1]["id"].clone();
        let restored = server.call(
            "restore_local_revision",
            json!({"path": rel, "revision": id, "dry_run": false}),
        )?;
        let on_disk = std::fs::read_to_string(root.join(&rel)).unwrap_or_default();
        if restored["content"].as_str() != Some(on_disk.as_str()) {
            return Err(format!(
                "restore_local_revision {rel}: disk differs from the revision"
            ));
        }
    }
    match n % 4 {
        0 => {
            let from = rng.below(1900) + 1;
            let rows = server.call(
                "run_sql",
                json!({"apply": true, "connection": "soak", "sql": format!("select id, name from item where id >= {from} order by id limit 50; select count(*) from item"), "max_rows": 20}),
            )?;
            if !rows.to_string().contains("2000") {
                return Err("run_sql: count of 2000 rows missing".into());
            }
        }
        1 => {
            let out = server.call(
                "terminal_command",
                json!({"command": format!("printf 'soak-%s\\n' {n}; ls src | wc -l"), "dry_run": false, "timeout": 20}),
            )?;
            if !out.to_string().contains(&format!("soak-{n}")) {
                return Err(format!("terminal_command: output without soak-{n}: {out}"));
            }
        }
        2 => {
            server.call(
                "terminal_render",
                json!({"output": format!("\u{1b}[2J\u{1b}[H\u{1b}[31mround {n}\u{1b}[0m\r\n\u{1b}]8;;http://x\u{1b}\\link\u{1b}]8;;\u{1b}\\\r\n"), "rows": 10, "cols": 40}),
            )?;
            server.call(
                "split_sql_statements",
                json!({"sql": file_text(1, n), "dialect": "sqlite"}),
            )?;
        }
        _ => {
            server.call(
                "search_in_files",
                json!({"pattern": "Round", "max_matches": 20}),
            )?;
            server.call("list_files", json!({}))?;
        }
    }
    if *lsp && n % 10 == 3 {
        let python = file_name(0);
        match server.call(
            "file_diagnostics",
            json!({"path": python, "timeout_seconds": 30}),
        ) {
            Ok(_) => {}
            Err(e) if e.contains("not found") || e.contains("missing") || e.contains("install") => {
                *lsp = false;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A short-lived server: a terminal command and (if enabled) a language server, then the end.
fn session(
    config: &Config,
    root: &Path,
    config_dir: &Path,
    log: &Path,
    lsp: bool,
) -> Result<(), String> {
    let mut s = Server::start(
        &config.server,
        root,
        config_dir,
        log,
        config.request_timeout,
    )?;
    s.call(
        "terminal_command",
        json!({"command": "sleep 0.2; echo churn", "dry_run": false, "timeout": 20}),
    )?;
    if lsp {
        let _ = s.call(
            "file_diagnostics",
            json!({"path": file_name(0), "timeout_seconds": 30}),
        );
    }
    s.call("read_file", json!({"path": file_name(2)}))?;
    let left = s.close();
    if left.is_empty() {
        Ok(())
    } else {
        Err(format!("processes outlived their server: {left:?}"))
    }
}

fn median(mut v: Vec<u64>) -> u64 {
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

/// Runs the soak test; `progress` gets every sample as it is taken.
pub fn run(config: &Config, mut progress: impl FnMut(&Sample)) -> Outcome {
    let mut outcome = Outcome::default();
    let root = config.workspace.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("idecraft-soak-{}", std::process::id()))
    });
    if config.workspace.is_none() {
        let _ = std::fs::remove_dir_all(&root);
    }
    let config_dir = match prepare(&root) {
        Ok(dir) => dir,
        Err(e) => {
            outcome.failures.push(format!("prepare workspace: {e}"));
            return outcome;
        }
    };
    let log = root.with_extension("log");
    let mut server = match Server::start(
        &config.server,
        &root,
        &config_dir,
        &log,
        config.request_timeout,
    ) {
        Ok(s) => s,
        Err(e) => {
            outcome.failures.push(e);
            return outcome;
        }
    };
    if let Err(e) = server.call(
        "save_connection",
        json!({"name": "soak", "dialect": "sqlite", "path": root.join("soak.db"), "dry_run": false}),
    ) {
        outcome.failures.push(format!("save_connection: {e}"));
    }
    // The user's grant for the connection (SPEC-0012 v3): reading rows, for the target the
    // saved profile has. What the project file says grants an agent nothing.
    let granted = idecraft_database::load_profiles(&root)
        .map_err(|e| e.to_string())
        .and_then(|profiles| {
            let soak = profiles
                .iter()
                .find(|p| p.name == "soak")
                .ok_or_else(|| "no connection `soak` was saved".to_owned())?;
            idecraft_core::add_connection_grant(
                &config_dir.join(idecraft_core::GRANT_FILE),
                &root,
                &soak.id,
                &soak.agent_target(),
                &["read-data"],
            )
        });
    if let Err(e) = granted {
        outcome
            .failures
            .push(format!("grant for the connection: {e}"));
    }
    let mut csv = config
        .csv
        .as_ref()
        .and_then(|p| std::fs::File::create(p).ok());
    if let Some(f) = &mut csv {
        let _ = writeln!(f, "elapsed_s,rounds,rss_kb,fds,children");
    }
    let mut rng = Rng(config.seed | 1);
    let mut lsp = config.language_servers;
    let start = Instant::now();
    let mut next_sample = start;
    let mut n = 0u64;
    while start.elapsed() < config.duration {
        if Instant::now() >= next_sample {
            let pid = server.pid();
            let sample = Sample {
                elapsed: start.elapsed().as_secs_f64(),
                rounds: n,
                rss_kb: rss_kb(pid),
                fds: open_fds(pid),
                children: descendants(pid).len(),
            };
            if let Some(f) = &mut csv {
                let _ = writeln!(
                    f,
                    "{:.1},{},{},{},{}",
                    sample.elapsed, sample.rounds, sample.rss_kb, sample.fds, sample.children
                );
            }
            progress(&sample);
            outcome.samples.push(sample);
            next_sample += config.sample_every;
        }
        if let Err(e) = round(&mut server, &root, n, &mut rng, &mut lsp) {
            let stuck = e.starts_with("stuck") || e.starts_with("server ended");
            outcome.failures.push(format!("round {n}: {e}"));
            if stuck || outcome.failures.len() > 20 {
                break;
            }
        }
        n += 1;
        if config.churn_every > 0 && n.is_multiple_of(config.churn_every) {
            match session(config, &root, &config_dir, &log, lsp) {
                Ok(()) => outcome.sessions += 1,
                Err(e) => outcome
                    .failures
                    .push(format!("session after round {n}: {e}")),
            }
        }
    }
    outcome.rounds = n;
    outcome.language_servers = lsp && config.language_servers;
    outcome.requests = server.requests;
    (outcome.slowest, outcome.slowest_request) = server.slowest.clone();
    // Growth from the warmed-up level (samples after the first fifth) to the last fifth.
    let samples = &outcome.samples;
    if samples.len() >= 5 {
        let fifth = samples.len() / 5;
        let warm = &samples[fifth..2 * fifth];
        let last = &samples[samples.len() - fifth..];
        let rss = |s: &[Sample]| median(s.iter().map(|x| x.rss_kb).collect());
        let fds = |s: &[Sample]| median(s.iter().map(|x| x.fds as u64).collect());
        let (rss_warm, rss_last) = (rss(warm), rss(last));
        if rss_last > rss_warm + config.max_rss_growth_mb * 1024 {
            outcome.failures.push(format!(
                "memory grew from {} MiB to {} MiB (allowed {} MiB)",
                rss_warm / 1024,
                rss_last / 1024,
                config.max_rss_growth_mb
            ));
        }
        let (fd_warm, fd_last) = (fds(warm), fds(last));
        if fd_last > fd_warm + config.max_fd_growth as u64 {
            outcome.failures.push(format!(
                "open file descriptors grew from {fd_warm} to {fd_last} (allowed {})",
                config.max_fd_growth
            ));
        }
        let children_warm = warm.iter().map(|s| s.children).max().unwrap_or(0);
        let children_last = last.iter().map(|s| s.children).min().unwrap_or(0);
        if children_last > children_warm + 1 {
            outcome.failures.push(format!(
                "child processes grew from {children_warm} to {children_last}"
            ));
        }
    }
    let left = server.close();
    if !left.is_empty() {
        outcome
            .failures
            .push(format!("processes outlived the server: {left:?}"));
    }
    if config.workspace.is_none() {
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&config_dir);
        let _ = std::fs::remove_file(&log);
    }
    outcome
}
