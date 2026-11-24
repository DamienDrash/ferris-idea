//! A run on the host for the window (`SPEC-0318` behaviours 21–24, v2 window details 3–4): a
//! [`RunHandle`] like a local run's — its events are the host's `run.event` notifications and the
//! lines of its two output streams — so the Run tool window shows host runs as it shows local
//! ones. After a break the run reattaches by itself (`run.attach` with the bytes it has per
//! stream, protocol detail 5) and says how much output was lost, if any.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use ferris_idea_core::{RunEvent, RunHandle, RunOutcome, TestReport};
use ferris_idea_session::{Connection, ErrorCode, RpcError};
use serde_json::{Value, json};

use crate::live::{ConnState, LiveProject};

const LIMIT: Duration = Duration::from_secs(60);

/// What to run on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunTarget {
    /// A configuration of the host project by name.
    Name(String),
    /// What a run marker or ⌃⇧R stands for at a 0-based line of a document (behaviour 22).
    Context {
        /// The document (relative to the project folder).
        doc: String,
        /// 0-based line.
        line: usize,
    },
}

/// A run started on the host: its id there, its configuration's name and the handle the Run tool
/// window reads.
pub struct HostRun {
    /// The host's id of the run (`r1` …).
    pub id: String,
    /// The configuration's name.
    pub name: String,
    /// Events and Stop, as for a local run.
    pub handle: RunHandle,
}

/// Starts `target` on the host of `live` (needs a trusted host project: an untrusted one answers
/// with the trust message, behaviour 5).
pub fn start_host_run(live: &LiveProject, target: &RunTarget) -> Result<HostRun, RpcError> {
    // Listening first, so that no event of the run is missed.
    let events = live.listen_host("run.event");
    let params = match target {
        RunTarget::Name(n) => json!({ "name": n }),
        RunTarget::Context { doc, line } => json!({ "context": { "doc": doc, "line": line } }),
    };
    let started = live.host_call("run.start", params, LIMIT)?;
    let id = started["run"].as_str().unwrap_or_default().to_owned();
    let name = started["name"].as_str().unwrap_or_default().to_owned();
    let streams = [
        started["stdout"].as_u64().unwrap_or_default() as u32,
        started["stderr"].as_u64().unwrap_or_default() as u32,
    ];
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker = Worker {
        live: live.clone(),
        id: id.clone(),
        name: name.clone(),
        events,
        tx,
        stop: Arc::clone(&stop),
        conn: live.host_connection(),
        streams: [
            Stream::new(streams[0], false),
            Stream::new(streams[1], true),
        ],
        started: false,
        held: Vec::new(),
        since: Instant::now(),
    };
    std::thread::Builder::new()
        .name("host-run".into())
        .spawn(move || worker.run())
        .ok();
    Ok(HostRun {
        id,
        name,
        handle: RunHandle::from_parts(stop, rx),
    })
}

struct Stream {
    id: u32,
    stderr: bool,
    /// Bytes of this stream the window has.
    seen: u64,
    /// The start of a line not ended yet.
    partial: Vec<u8>,
    done: bool,
}

impl Stream {
    fn new(id: u32, stderr: bool) -> Self {
        Self {
            id,
            stderr,
            seen: 0,
            partial: Vec::new(),
            done: false,
        }
    }

    /// The complete lines of `data` (the rest waits for its line break).
    fn lines(&mut self, data: &[u8]) -> Vec<String> {
        self.seen += data.len() as u64;
        self.partial.extend_from_slice(data);
        let mut out = Vec::new();
        while let Some(i) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=i).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]);
            out.push(text.trim_end_matches('\r').to_owned());
        }
        out
    }

    fn flush(&mut self) -> Option<String> {
        (!self.partial.is_empty()).then(|| {
            let text = String::from_utf8_lossy(&self.partial).into_owned();
            self.partial.clear();
            text
        })
    }
}

struct Worker {
    live: LiveProject,
    id: String,
    name: String,
    events: Receiver<(String, Value)>,
    tx: Sender<RunEvent>,
    stop: Arc<AtomicBool>,
    conn: Option<Connection>,
    streams: [Stream; 2],
    /// Output waits for the run's `started` (at most [`HOLD`]), so the console shows the command
    /// line first as for a local run.
    started: bool,
    held: Vec<(bool, String)>,
    since: Instant,
}

/// How long output waits for the `started` notification.
const HOLD: Duration = Duration::from_secs(2);

impl Worker {
    fn output(&mut self, stderr: bool, text: String) {
        if !self.started && self.since.elapsed() < HOLD {
            self.held.push((stderr, text));
            return;
        }
        self.flush_held();
        let _ = self.tx.send(RunEvent::Output { stderr, text });
    }

    fn flush_held(&mut self) {
        for (stderr, text) in std::mem::take(&mut self.held) {
            let _ = self.tx.send(RunEvent::Output { stderr, text });
        }
    }

    /// Reads what arrived on both streams, waiting up to `wait` on each; returns whether
    /// something came and whether the connection broke.
    fn read_streams(&mut self, c: &Connection, wait: Duration) -> (bool, bool) {
        let (mut got, mut broken) = (false, false);
        for i in 0..2 {
            if self.streams[i].done {
                continue;
            }
            let (data, done) = c.read_stream(self.streams[i].id, wait);
            if !data.is_empty() {
                got = true;
                let stderr = self.streams[i].stderr;
                for line in self.streams[i].lines(&data) {
                    self.output(stderr, line);
                }
            }
            match done.as_deref() {
                None => {}
                Some("done") => {
                    let s = &mut self.streams[i];
                    s.done = true;
                    let stderr = s.stderr;
                    if let Some(rest) = s.flush() {
                        self.output(stderr, rest);
                    }
                }
                Some(_) => {
                    c.drop_stream(self.streams[i].id);
                    broken = true;
                }
            }
        }
        (got, broken)
    }

    /// The run's notifications received so far.
    fn take_run_events(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok((_, p)) = self.events.try_recv() {
            if p["run"].as_str() == Some(self.id.as_str()) {
                out.push(p);
            }
        }
        out
    }

    /// Turns notifications into events; returns how many `finished` there were.
    fn forward(&mut self, events: Vec<Value>, report: &mut Option<TestReport>) -> usize {
        let mut finished = 0;
        for p in events {
            match p["kind"].as_str().unwrap_or_default() {
                "started" => {
                    let command = p["command"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| s.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    let name = p["name"].as_str().unwrap_or(&self.name).to_owned();
                    let _ = self.tx.send(RunEvent::Started { name, command });
                    self.started = true;
                    self.flush_held();
                }
                "tests" => *report = serde_json::from_value(p["report"].clone()).ok(),
                "chain_stopped" => {
                    self.flush_held();
                    let failed = p["failed"].as_str().unwrap_or_default().to_owned();
                    let _ = self.tx.send(RunEvent::ChainStopped { failed });
                }
                "finished" => {
                    self.flush_held();
                    finished += 1;
                    let _ = self
                        .tx
                        .send(RunEvent::Finished(Box::new(outcome(&p, report.take()))));
                }
                _ => {}
            }
        }
        finished
    }

    fn run(mut self) {
        let mut report = None;
        let mut finished = 0;
        let mut stop_sent = false;
        let mut broken = false;
        let mut quiet_since: Option<Instant> = None;
        loop {
            if self.stop.load(Ordering::SeqCst) && !stop_sent {
                stop_sent = true;
                let _ = self
                    .live
                    .host_call("run.stop", json!({ "run": self.id }), LIMIT);
            }
            if broken {
                std::thread::sleep(Duration::from_millis(50));
                match self.reattach() {
                    Reattach::Done => broken = false,
                    Reattach::Wait => {}
                    Reattach::Gone => {
                        let events = self.take_run_events();
                        finished += self.forward(events, &mut report);
                        self.flush_held();
                        if finished == 0 {
                            let p = json!({ "name": self.name });
                            let _ = self
                                .tx
                                .send(RunEvent::Finished(Box::new(outcome(&p, None))));
                        }
                        return;
                    }
                }
                continue;
            }
            let Some(c) = self.conn.clone() else {
                broken = true;
                continue;
            };
            let (got, broke) = self.read_streams(&c, Duration::from_millis(10));
            if broke {
                broken = true;
                continue;
            }
            let events = self.take_run_events();
            if events.iter().any(|p| p["kind"] == "finished") {
                // The output before the end is on the streams already.
                let _ = self.read_streams(&c, Duration::ZERO);
            }
            let before = finished;
            finished += self.forward(events, &mut report);
            if self.streams.iter().all(|s| s.done) {
                // The end arrived on the streams; the last notifications follow at the next pump.
                if finished > before || got {
                    quiet_since = Some(Instant::now());
                }
                let quiet = quiet_since.get_or_insert_with(Instant::now).elapsed();
                if (finished > 0 && quiet >= Duration::from_millis(300))
                    || quiet >= Duration::from_secs(5)
                {
                    self.flush_held();
                    if finished == 0 {
                        let state = self.final_state();
                        let _ = self
                            .tx
                            .send(RunEvent::Finished(Box::new(outcome(&state, report))));
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// What `run.list` says of this run (the end when its notification did not arrive).
    fn final_state(&self) -> Value {
        let mut v = self
            .live
            .host_call("run.list", json!({}), LIMIT)
            .ok()
            .and_then(|l| {
                l["runs"]
                    .as_array()?
                    .iter()
                    .find(|r| r["run"].as_str() == Some(self.id.as_str()))
                    .cloned()
            })
            .unwrap_or_else(|| json!({}));
        v["name"] = json!(self.name);
        v
    }

    fn reattach(&mut self) -> Reattach {
        match self.live.state() {
            ConnState::Closed => return Reattach::Gone,
            ConnState::Connected { .. } => {}
            _ => return Reattach::Wait,
        }
        let Some(c) = self.live.host_connection().filter(Connection::alive) else {
            return Reattach::Wait;
        };
        let asked = json!({
            "run": self.id,
            "stdout_from": self.streams[0].seen,
            "stderr_from": self.streams[1].seen,
        });
        match c.call("run.attach", asked, LIMIT) {
            Ok(v) => {
                for (i, (stream, from)) in [("stdout", "stdout_from"), ("stderr", "stderr_from")]
                    .into_iter()
                    .enumerate()
                {
                    let s = &mut self.streams[i];
                    s.id = v[stream].as_u64().unwrap_or_default() as u32;
                    s.done = false;
                    let start = v[from].as_u64().unwrap_or(s.seen);
                    if start > s.seen {
                        let lost = start - s.seen;
                        s.seen = start;
                        s.partial.clear();
                        let stderr = s.stderr;
                        self.output(
                            stderr,
                            format!("… {lost} bytes of output were lost during the break"),
                        );
                    }
                }
                self.conn = Some(c);
                Reattach::Done
            }
            Err(e) if matches!(e.code, ErrorCode::NotFound | ErrorCode::Denied) => Reattach::Gone,
            Err(_) => Reattach::Wait,
        }
    }
}

enum Reattach {
    Done,
    Wait,
    Gone,
}

/// A step's outcome from a `finished` notification (or the run's listed state).
fn outcome(p: &Value, report: Option<TestReport>) -> RunOutcome {
    RunOutcome {
        name: p["name"].as_str().unwrap_or_default().to_owned(),
        exit_code: p["exit_code"].as_i64().map(|c| c as i32),
        timed_out: p["timed_out"].as_bool() == Some(true),
        stopped: p["stopped"].as_bool() == Some(true),
        start_error: p["start_error"].as_str().map(str::to_owned),
        stdout: String::new(),
        stderr: String::new(),
        millis: p["millis"].as_u64().unwrap_or_default(),
        report,
        report_error: None,
        browser: Vec::new(),
    }
}
