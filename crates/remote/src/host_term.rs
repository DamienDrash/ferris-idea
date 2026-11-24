//! A terminal on the host for the window (`SPEC-0318` behaviours 17–20, v2 window detail 2): the
//! shell runs in the backend's pseudo terminal, its output arrives on a session stream, keystrokes
//! go back on it. One worker thread per terminal speaks the protocol, so typing never waits for
//! the network: while the connection is broken the terminal reattaches by itself once it is back
//! (`term.attach` at the offset it has, protocol detail 5) and sends what was typed meanwhile.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_session::ErrorCode;
use serde_json::{Value, json};

use crate::live::{ConnState, LiveProject};

/// Typing kept while the connection is broken (v2 window detail 2).
pub const PENDING_INPUT: usize = 64 << 10;

/// How long opening and reattaching wait for the host.
const LIMIT: Duration = Duration::from_secs(30);

/// What a host terminal produced, for the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermOutput {
    /// Output of the shell, in order.
    Data(Vec<u8>),
    /// Output was lost during a break: the screen starts again with what follows (the output the
    /// host kept).
    Redraw,
    /// The terminal ended: the shell's exit code when it exited, and a note for the screen.
    Ended(Option<i32>, String),
}

/// Where a host terminal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermPhase {
    /// `term.open` is on its way.
    Opening,
    /// Connected to its shell.
    Running,
    /// The connection is broken; it reattaches once it is back.
    Reconnecting,
    /// The shell exited, or the terminal ended with its session.
    Ended,
}

#[derive(Debug)]
struct Status {
    phase: TermPhase,
    term: Option<String>,
}

enum Input {
    Bytes(Vec<u8>),
    Resize(u16, u16),
    Close,
}

/// A shell on the host of a remote project.
pub struct HostTerminal {
    input: Sender<Input>,
    output: Mutex<Receiver<TermOutput>>,
    status: Arc<Mutex<Status>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl HostTerminal {
    /// Opens the user's shell on the host in `cwd` (relative to the project folder) with a
    /// `cols` × `rows` screen; returns at once, the shell starts in the background.
    pub fn open(live: LiveProject, cwd: &str, cols: u16, rows: u16) -> Self {
        let (input, input_rx) = mpsc::channel();
        let (out_tx, output) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status {
            phase: TermPhase::Opening,
            term: None,
        }));
        let worker = Worker {
            live,
            status: Arc::clone(&status),
            out: out_tx,
            input: input_rx,
            size: (cols.max(2), rows.max(1)),
            pending: Vec::new(),
        };
        let cwd = cwd.to_owned();
        std::thread::Builder::new()
            .name("host-terminal".into())
            .spawn(move || worker.run(&cwd))
            .ok();
        Self {
            input,
            output: Mutex::new(output),
            status,
        }
    }

    /// Sends keystrokes or pasted text; kept while the connection is broken.
    pub fn write(&self, bytes: &[u8]) {
        let _ = self.input.send(Input::Bytes(bytes.to_vec()));
    }

    /// Changes the size of the host's pseudo terminal.
    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.input.send(Input::Resize(cols.max(2), rows.max(1)));
    }

    /// Ends the shell and its process group on the host (`term.close`).
    pub fn close(&self) {
        let _ = self.input.send(Input::Close);
    }

    /// The next output, waiting up to `wait`; `None` when nothing came.
    pub fn read(&self, wait: Duration) -> Option<TermOutput> {
        match lock(&self.output).recv_timeout(wait) {
            Ok(o) => Some(o),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                Some(TermOutput::Ended(None, "Ended with the session".into()))
            }
        }
    }

    /// Where the terminal is.
    pub fn phase(&self) -> TermPhase {
        lock(&self.status).phase
    }

    /// The host's id of the terminal (`t1` …), once it is open.
    pub fn id(&self) -> Option<String> {
        lock(&self.status).term.clone()
    }
}

impl Drop for HostTerminal {
    fn drop(&mut self) {
        let _ = self.input.send(Input::Close);
    }
}

struct Worker {
    live: LiveProject,
    status: Arc<Mutex<Status>>,
    out: Sender<TermOutput>,
    input: Receiver<Input>,
    size: (u16, u16),
    pending: Vec<u8>,
}

impl Worker {
    fn set(&self, phase: TermPhase) {
        lock(&self.status).phase = phase;
    }

    fn end(&self, code: Option<i32>, note: &str) {
        self.set(TermPhase::Ended);
        let _ = self.out.send(TermOutput::Ended(code, note.to_owned()));
    }

    /// Takes what the view sent; false when the terminal is to close.
    fn take_input(&mut self, resize_due: &mut bool) -> bool {
        loop {
            match self.input.try_recv() {
                Ok(Input::Bytes(b)) => {
                    self.pending.extend_from_slice(&b);
                    let over = self.pending.len().saturating_sub(PENDING_INPUT);
                    self.pending.drain(..over);
                }
                Ok(Input::Resize(c, r)) => {
                    self.size = (c, r);
                    *resize_due = true;
                }
                Ok(Input::Close) | Err(TryRecvError::Disconnected) => return false,
                Err(TryRecvError::Empty) => return true,
            }
        }
    }

    fn run(mut self, cwd: &str) {
        let opened = self.live.host_call(
            "term.open",
            json!({ "cwd": cwd, "cols": self.size.0, "rows": self.size.1 }),
            LIMIT,
        );
        let (term, mut stream) = match opened {
            Ok(v) => (
                v["term"].as_str().unwrap_or_default().to_owned(),
                v["stream"].as_u64().unwrap_or_default() as u32,
            ),
            Err(e) => {
                let note = format!(
                    "Cannot open a terminal on {}: {}",
                    self.live.host(),
                    e.message
                );
                let _ = self
                    .out
                    .send(TermOutput::Data(format!("{note}\r\n").into_bytes()));
                self.end(None, &note);
                return;
            }
        };
        lock(&self.status).term = Some(term.clone());
        let mut conn = self.live.host_connection();
        if let Some(c) = &conn {
            c.open_sending(stream);
        }
        self.set(TermPhase::Running);
        // Bytes of the shell's output this terminal has (protocol detail 5).
        let mut seen: u64 = 0;
        let mut resize_due = false;
        let mut phase = TermPhase::Running;
        loop {
            if !self.take_input(&mut resize_due) {
                if let Some(c) = conn.as_ref().filter(|c| c.alive()) {
                    let _ = c.call("term.close", json!({ "term": term }), LIMIT);
                } else {
                    let _ = self
                        .live
                        .host_call("term.close", json!({ "term": term }), LIMIT);
                }
                self.end(None, "Closed");
                return;
            }
            match phase {
                TermPhase::Running => {
                    let Some(c) = conn.clone() else {
                        phase = TermPhase::Reconnecting;
                        self.set(phase);
                        continue;
                    };
                    if !self.pending.is_empty()
                        && c.send_data(stream, &self.pending, Duration::from_secs(2))
                            .is_ok()
                    {
                        self.pending.clear();
                    }
                    if resize_due
                        && c.call(
                            "term.resize",
                            json!({ "term": term, "cols": self.size.0, "rows": self.size.1 }),
                            LIMIT,
                        )
                        .is_ok()
                    {
                        resize_due = false;
                    }
                    let (data, done) = c.read_stream(stream, Duration::from_millis(20));
                    if !data.is_empty() {
                        seen += data.len() as u64;
                        let _ = self.out.send(TermOutput::Data(data));
                    }
                    match done.as_deref() {
                        None => {}
                        Some("exited") => {
                            let code = exit_code(&c, &term);
                            self.end(code, &exit_note(code));
                            return;
                        }
                        // The connection broke (or another attach took the stream).
                        Some(_) => {
                            c.drop_stream(stream);
                            phase = TermPhase::Reconnecting;
                            self.set(phase);
                        }
                    }
                }
                TermPhase::Reconnecting => {
                    std::thread::sleep(Duration::from_millis(50));
                    match self.live.state() {
                        ConnState::Closed => {
                            self.end(None, "Ended with the session");
                            return;
                        }
                        ConnState::Connected { .. } => {}
                        _ => continue,
                    }
                    let Some(c) = self.live.host_connection().filter(|c| c.alive()) else {
                        continue;
                    };
                    match c.call("term.attach", json!({ "term": term, "from": seen }), LIMIT) {
                        Ok(v) => {
                            stream = v["stream"].as_u64().unwrap_or_default() as u32;
                            let start = v["from"].as_u64().unwrap_or(seen);
                            if start != seen {
                                let _ = self.out.send(TermOutput::Redraw);
                                seen = start;
                            }
                            c.open_sending(stream);
                            conn = Some(c);
                            resize_due = true;
                            phase = TermPhase::Running;
                            self.set(phase);
                        }
                        Err(e) if matches!(e.code, ErrorCode::NotFound | ErrorCode::Denied) => {
                            self.end(None, "Ended with the session");
                            return;
                        }
                        Err(_) => {}
                    }
                }
                TermPhase::Opening | TermPhase::Ended => return,
            }
        }
    }
}

/// The exit code of `term` as the host lists it.
fn exit_code(c: &ferris_idea_session::Connection, term: &str) -> Option<i32> {
    let list = c.call("term.list", json!({}), LIMIT).ok()?;
    list["terminals"]
        .as_array()?
        .iter()
        .find(|t| t["term"] == Value::String(term.to_owned()))
        .and_then(|t| t["exit_code"].as_i64())
        .map(|c| c as i32)
}

fn exit_note(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("Process finished with exit code {c}"),
        None => "Process finished".into(),
    }
}
