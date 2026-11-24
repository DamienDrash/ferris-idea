//! The window's live handle on a remote project (`SPEC-0317` v3 behaviours 6, 9, 12–16): the
//! session behind a lock that typing never waits for, a supervisor thread that pumps it, sends a
//! heartbeat and reconnects by itself after a break (1, 2, 4, 8, 16, then every 30 s, as long as
//! the backend's reattach period lasts), the reconciliation of an editor's text with the
//! session's replica, and restoring recovery copies after a crash of the window.
//!
//! Typing never waits: the editor keeps its own text; [`LiveProject::sync_doc`] meets it with the
//! replica only when the lock is free at once. What both sides did since they last agreed is
//! transformed against each other (the replica's change, from the backend, first), so neither
//! text is lost and both end equal. While the lock is busy (a reconnection, a save, an open), the
//! editor's newest text is written to the recovery copy instead.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use ferris_idea_session::{
    ClientSession, Connection, ErrorCode, Op, Reconnected, Recovery, RecoveryCopy, RpcError, TextOp,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::project::{RemoteProject, Unsaved};
use crate::ssh::RemoteError;

/// What the window needs of a remote project's channel: the SSH project ([`RemoteProject`]) and
/// the in-process one ([`crate::inprocess::InProcessProject`]) provide it.
pub trait ProjectLink: Send {
    /// The window half of the session.
    fn session(&mut self) -> &mut ClientSession;
    /// The same, to read.
    fn session_ref(&self) -> &ClientSession;
    /// Resumes the session, or starts a new backend and reconciles the documents.
    fn reconnect(&mut self) -> Result<Reconnected, RemoteError>;
    /// Breaks the channel as a network failure would.
    fn break_connection(&mut self);
    /// Ends the session; refuses while files are unsaved unless told to save or discard them.
    fn close(&mut self, unsaved: Unsaved) -> Result<Vec<String>, RemoteError>;
    /// State for agents and the status bar (never the resume token).
    fn describe(&mut self) -> Value;
    /// The connection log.
    fn connection_log(&self) -> Vec<String>;
    /// Host label.
    fn host(&self) -> String;
    /// Project folder on the host.
    fn folder(&self) -> String;
}

impl ProjectLink for crate::paired::PairedProject {
    fn session(&mut self) -> &mut ClientSession {
        &mut self.session
    }
    fn session_ref(&self) -> &ClientSession {
        &self.session
    }
    fn reconnect(&mut self) -> Result<Reconnected, RemoteError> {
        crate::paired::PairedProject::reconnect(self)
    }
    fn break_connection(&mut self) {
        crate::paired::PairedProject::break_connection(self);
    }
    fn close(&mut self, unsaved: Unsaved) -> Result<Vec<String>, RemoteError> {
        crate::paired::PairedProject::close(self, unsaved)
    }
    fn describe(&mut self) -> Value {
        crate::paired::PairedProject::describe(self)
    }
    fn connection_log(&self) -> Vec<String> {
        self.session.log.clone()
    }
    fn host(&self) -> String {
        self.label().to_owned()
    }
    fn folder(&self) -> String {
        crate::paired::PairedProject::folder(self).to_owned()
    }
}

impl ProjectLink for RemoteProject {
    fn session(&mut self) -> &mut ClientSession {
        &mut self.session
    }
    fn session_ref(&self) -> &ClientSession {
        &self.session
    }
    fn reconnect(&mut self) -> Result<Reconnected, RemoteError> {
        RemoteProject::reconnect(self)
    }
    fn break_connection(&mut self) {
        RemoteProject::break_connection(self);
    }
    fn close(&mut self, unsaved: Unsaved) -> Result<Vec<String>, RemoteError> {
        RemoteProject::close(self, unsaved)
    }
    fn describe(&mut self) -> Value {
        RemoteProject::describe(self)
    }
    fn connection_log(&self) -> Vec<String> {
        RemoteProject::connection_log(self)
    }
    fn host(&self) -> String {
        self.host.clone()
    }
    fn folder(&self) -> String {
        self.folder.clone()
    }
}

/// Timing of the supervisor (`SPEC-0317` v3 behaviour 14); tests shorten it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    /// Waits before the first attempts after a break.
    pub delays: Vec<Duration>,
    /// Wait between later attempts.
    pub then: Duration,
    /// How long the backend keeps a broken session (`ADR-0025` §8: 10 minutes).
    pub reattach: Duration,
    /// Time between heartbeats while connected.
    pub heartbeat: Duration,
    /// A heartbeat without an answer within this time counts as a break.
    pub heartbeat_limit: Duration,
    /// How often the supervisor looks.
    pub tick: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            delays: [1, 2, 4, 8, 16].map(Duration::from_secs).to_vec(),
            then: Duration::from_secs(30),
            reattach: Duration::from_secs(600),
            heartbeat: Duration::from_secs(5),
            heartbeat_limit: Duration::from_secs(10),
            tick: Duration::from_millis(50),
        }
    }
}

impl Timing {
    /// The wait before attempt `n` (1-based) after a break: 1, 2, 4, 8, 16, then 30 s.
    pub fn delay(&self, attempt: u32) -> Duration {
        let i = attempt.saturating_sub(1) as usize;
        self.delays.get(i).copied().unwrap_or(self.then)
    }
}

/// The connection as the status bar shows it (`SPEC-0317` behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnState {
    /// Working; the round trip of the last heartbeat.
    Connected {
        /// Milliseconds.
        round_trip_ms: Option<u64>,
    },
    /// Broken; the window tries again by itself.
    Reconnecting {
        /// The attempt that comes next (1-based).
        attempt: u32,
        /// Seconds until it.
        next_in_s: u64,
        /// Seconds left of the backend's reattach period.
        left_s: u64,
    },
    /// Broken; no attempt by itself (switched off by the user, or the reattach period passed).
    Disconnected {
        /// Why.
        reason: String,
    },
    /// The project was closed.
    Closed,
}

impl ConnState {
    /// The state's name: Connected, Reconnecting, Disconnected or Closed.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Connected { .. } => "Connected",
            Self::Reconnecting { .. } => "Reconnecting",
            Self::Disconnected { .. } => "Disconnected",
            Self::Closed => "Closed",
        }
    }

    /// The status bar widget's text for `host`.
    pub fn widget_text(&self, host: &str) -> String {
        match self {
            Self::Connected {
                round_trip_ms: Some(ms),
            } => format!("{host} · {ms} ms"),
            Self::Connected { .. } => host.to_owned(),
            Self::Reconnecting { attempt, .. } => {
                format!("{host} · Reconnecting (attempt {attempt})")
            }
            Self::Disconnected { .. } => format!("{host} · Disconnected"),
            Self::Closed => format!("{host} · Closed"),
        }
    }

    /// The widget's tooltip: state, timing and the number of pending edits.
    pub fn tooltip(&self, host: &str, pending: usize) -> String {
        let state = match self {
            Self::Connected {
                round_trip_ms: Some(ms),
            } => format!("Connected to {host}, round trip {ms} ms"),
            Self::Connected { .. } => format!("Connected to {host}"),
            Self::Reconnecting {
                attempt,
                next_in_s,
                left_s,
            } => format!(
                "Reconnecting to {host}: attempt {attempt} in {next_in_s} s, the host keeps the session for {}:{:02} more",
                left_s / 60,
                left_s % 60
            ),
            Self::Disconnected { reason } => format!("Disconnected from {host}: {reason}"),
            Self::Closed => format!("{host}: closed"),
        };
        match pending {
            0 => state,
            1 => format!("{state}\n1 edit waits for the host"),
            n => format!("{state}\n{n} edits wait for the host"),
        }
    }
}

struct Watch {
    state: ConnState,
    broke_at: Option<Instant>,
    attempt: u32,
    next_at: Option<Instant>,
    last_ping: Option<Instant>,
    rtt: Option<u64>,
}

struct Shared {
    link: Mutex<Box<dyn ProjectLink>>,
    watch: Mutex<Watch>,
    host: String,
    folder: String,
    timing: Timing,
    recovery: Option<Recovery>,
    stop: AtomicBool,
    now: AtomicBool,
    hold: AtomicBool,
    /// Counts state changes and reconnections, so a window knows when to look again.
    generation: AtomicU64,
    log: Mutex<Vec<String>>,
    /// What the window shows of the host for agents (`SPEC-0318` v2 detail 13): its terminals and
    /// runs by kind, and the stand-in folder its tool windows are registered under.
    window: Mutex<WindowState>,
}

#[derive(Default)]
struct WindowState {
    parts: std::collections::BTreeMap<String, Value>,
    root: Option<std::path::PathBuf>,
    /// Requests of agents to the window (`room_follow`, `SPEC-0323` v5 behaviour 36), oldest
    /// first, and the window's answers by request number.
    asks: std::collections::VecDeque<(u64, String, Value)>,
    answers: HashMap<u64, Value>,
    next_ask: u64,
}

/// Unanswered requests to a window kept at most: a window that never looks drops the oldest.
const WINDOW_ASKS_KEPT: usize = 64;

fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One document as the window's editor last agreed with the session's replica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSync {
    /// Document id (path relative to the project folder).
    pub doc: String,
    /// The text both had when they last agreed.
    pub synced: String,
    /// The replica's unsaved state then.
    pub modified: bool,
    /// The last revision of the backend then.
    pub base_revision: u64,
    /// The replica's [`ferris_idea_session::RemoteDoc::stamp`] then.
    pub stamp: u64,
    /// Hash of the host file at load or last save then.
    pub base_hash: String,
}

/// A document's state as the window shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocState {
    /// Unsaved changes (in the window or on the backend).
    pub modified: bool,
    /// Edits not acknowledged yet.
    pub pending: usize,
    /// The host file changed under unsaved changes: saving asks first.
    pub disk_changed: bool,
    /// A save waits for the connection.
    pub save_requested: bool,
    /// The backend's revision the window knows.
    pub revision: u64,
    /// The replica's [`ferris_idea_session::RemoteDoc::stamp`].
    pub stamp: u64,
}

/// When a window's editor has to meet the session's replica again (`SPEC-0317` behaviour 9): the
/// one decision the window's frame loop takes for each open document, kept here so that it is
/// tested without a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Follow {
    editor_rev: u64,
}

impl Follow {
    /// A document just opened, shown by an editor at `editor_rev`.
    pub fn new(editor_rev: u64) -> Self {
        Self { editor_rev }
    }

    /// Whether the editor at `editor_rev` and the document in `state` have to meet.
    pub fn due(&self, sync: &DocSync, editor_rev: Option<u64>, state: Option<&DocState>) -> bool {
        editor_rev != Some(self.editor_rev)
            || state.is_some_and(|s| s.stamp > sync.stamp || s.modified != sync.modified)
    }

    /// Remembers a meeting of the editor at `editor_rev` that ended with `outcome`. A session
    /// that was busy is no meeting: nothing is remembered, so the next frame tries again. A
    /// failed meeting (reported to the user) is not repeated for the same change.
    pub fn met(
        &mut self,
        outcome: &Result<Synced, RpcError>,
        sync: &mut DocSync,
        editor_rev: u64,
        state: Option<&DocState>,
    ) {
        match (outcome, state) {
            (Ok(Synced::Busy), _) => return,
            (Err(_), Some(s)) => sync.stamp = sync.stamp.max(s.stamp),
            _ => {}
        }
        self.editor_rev = editor_rev;
    }
}

/// What [`LiveProject::sync_doc`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Synced {
    /// The session was busy; the editor's newest text went to the recovery copy (if it differs).
    Busy,
    /// Nothing to do, or the editor's edit went to the session.
    Same,
    /// The editor must apply these ranges of its current text (other clients', agents' or the
    /// host's changes); afterwards it shows [`DocSync::synced`].
    Apply(Vec<Op>),
}

/// How a recovery copy was restored (`SPEC-0317` behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Restored {
    /// The host file still equals the copy's base: the copy's text is the document's text now.
    Followed,
    /// The host file changed since the copy: the copy's text is the document's text now, and the
    /// host's version stays in the host's local history once it is saved; the user is told.
    HostChanged,
    /// The copy already equals the host file.
    Unchanged,
    /// It could not be opened.
    Failed(String),
}

/// A remote project shared by the window, its supervisor and agents.
#[derive(Clone)]
pub struct LiveProject(Arc<Shared>);

static OPEN: Mutex<Vec<Weak<Shared>>> = Mutex::new(Vec::new());

/// The remote projects open in this process's windows (`SPEC-0317` v3 behaviour 20: agents see
/// them through `remote_projects`).
pub fn open_projects() -> Vec<LiveProject> {
    let mut list = lock(&OPEN);
    list.retain(|w| w.strong_count() > 0);
    list.iter()
        .filter_map(Weak::upgrade)
        .map(LiveProject)
        .filter(|p| p.state() != ConnState::Closed)
        .collect()
}

/// The open remote project of `host` and `folder`, if there is one.
pub fn find(host: &str, folder: &str) -> Option<LiveProject> {
    open_projects()
        .into_iter()
        .find(|p| p.host() == host && p.folder() == folder)
}

impl LiveProject {
    /// Takes over `link` (connected) and starts its supervisor.
    pub fn start(link: Box<dyn ProjectLink>, timing: Timing) -> Self {
        let host = link.host();
        let folder = link.folder();
        let recovery = link.session_ref().recovery().cloned();
        let shared = Arc::new(Shared {
            link: Mutex::new(link),
            watch: Mutex::new(Watch {
                state: ConnState::Connected {
                    round_trip_ms: None,
                },
                broke_at: None,
                attempt: 0,
                next_at: None,
                last_ping: None,
                rtt: None,
            }),
            host,
            folder,
            timing,
            recovery,
            stop: AtomicBool::new(false),
            now: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            log: Mutex::new(Vec::new()),
            window: Mutex::new(WindowState::default()),
        });
        let weak = Arc::downgrade(&shared);
        let tick = shared.timing.tick;
        std::thread::Builder::new()
            .name("remote-project".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(tick);
                    let Some(s) = weak.upgrade() else { break };
                    if s.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    supervise(&s);
                }
            })
            .ok();
        Self(shared)
    }

    /// Makes the project visible to agents as one open in a window.
    pub fn publish(&self) {
        lock(&OPEN).push(Arc::downgrade(&self.0));
    }

    /// Host label.
    pub fn host(&self) -> String {
        self.0.host.clone()
    }

    /// Project folder on the host.
    pub fn folder(&self) -> String {
        self.0.folder.clone()
    }

    /// The connection now.
    pub fn state(&self) -> ConnState {
        lock(&self.0.watch).state.clone()
    }

    /// Changes whenever the state changes or a reconnection happened.
    pub fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::SeqCst)
    }

    /// Runs `f` on the project, waiting for the lock (requests, saves, opening).
    pub fn with<R>(&self, f: impl FnOnce(&mut dyn ProjectLink) -> R) -> R {
        let mut g = lock(&self.0.link);
        f(g.as_mut())
    }

    /// Runs `f` on the project if the lock is free at once (what typing and frames use).
    pub fn try_with<R>(&self, f: impl FnOnce(&mut dyn ProjectLink) -> R) -> Option<R> {
        let mut g = match self.0.link.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        Some(f(g.as_mut()))
    }

    /// Tries to reconnect at once (the widget's Reconnect, `remote_project_reconnect`); automatic
    /// attempts resume after a Disconnect.
    pub fn reconnect_now(&self) {
        self.0.hold.store(false, Ordering::SeqCst);
        self.0.now.store(true, Ordering::SeqCst);
    }

    /// Breaks the connection and stops reconnecting by itself (the widget's Disconnect).
    pub fn disconnect(&self) {
        self.0.hold.store(true, Ordering::SeqCst);
        self.with(|l| l.break_connection());
        self.set(ConnState::Disconnected {
            reason: "disconnected by the user; Reconnect connects again".into(),
        });
    }

    /// Breaks the connection as a network failure would; the supervisor reconnects (tests and
    /// the self-test).
    pub fn break_connection(&self) {
        self.with(|l| l.break_connection());
    }

    /// Reconnects now and waits for the outcome (agents: `remote_project_reconnect`).
    pub fn reconnect_wait(&self) -> Result<Reconnected, RemoteError> {
        self.0.hold.store(false, Ordering::SeqCst);
        let r = self.with(|l| {
            // A closed project stays closed (SPEC-0317 v9 detail 2).
            if self.0.stop.load(Ordering::SeqCst) {
                return Err(RemoteError::Failed("the project was closed".into()));
            }
            if l.session_ref().connected() {
                l.break_connection();
            }
            l.reconnect()
        });
        match &r {
            Ok(outcome) => {
                self.note(format!("reconnected: {outcome:?}"));
                let mut w = lock(&self.0.watch);
                if closed(&self.0, &w) {
                    return r;
                }
                w.broke_at = None;
                w.attempt = 0;
                w.last_ping = None;
                w.state = ConnState::Connected {
                    round_trip_ms: w.rtt,
                };
            }
            Err(e) => self.note(format!("reconnect failed: {e}")),
        }
        self.0.generation.fetch_add(1, Ordering::SeqCst);
        r
    }

    /// Ends the session as `unsaved` says and stops the supervisor.
    ///
    /// Closing is final (SPEC-0317 v9 detail 2): `stop` is set while the link is still locked,
    /// so no reconnection starts after it, and no state the supervisor or a reconnection computed
    /// before the close replaces `Closed` afterwards — a closed project never waits for a
    /// reattachment and leaves [`open_projects`] at once.
    pub fn close(&self, unsaved: Unsaved) -> Result<Vec<String>, RemoteError> {
        let r = self.with(|l| {
            let r = l.close(unsaved);
            if r.is_ok() {
                self.0.stop.store(true, Ordering::SeqCst);
            }
            r
        })?;
        self.set(ConnState::Closed);
        Ok(r)
    }

    /// State for agents (`remote_projects`): the project's own description with the window's
    /// connection state, attempt and reattach time. Never the resume token.
    pub fn describe(&self) -> Value {
        let mut v = self.with(|l| l.describe());
        let state = self.state();
        v["state"] = json!(state.label().to_lowercase());
        v["connection"] = serde_json::to_value(&state).unwrap_or(Value::Null);
        v["window"] = json!(true);
        let w = lock(&self.0.window);
        for (k, part) in &w.parts {
            v[k.as_str()] = part.clone();
        }
        v["forwards"] = json!(crate::ports::list(&self.0.host, &self.0.folder));
        v
    }

    /// Publishes what the window shows of `kind` (`terminals`, `runs`) for agents.
    pub fn set_window_part(&self, kind: &str, value: Value) {
        lock(&self.0.window).parts.insert(kind.to_owned(), value);
    }

    /// What the window last published of `kind` (see [`LiveProject::set_window_part`]).
    pub fn window_part(&self, kind: &str) -> Option<Value> {
        lock(&self.0.window).parts.get(kind).cloned()
    }

    /// Hands the window a request of an agent (`kind` names what it asks, `SPEC-0323` v5
    /// behaviour 36); the window answers on its next look. Returns the request's number.
    pub fn ask_window(&self, kind: &str, value: Value) -> u64 {
        let mut w = lock(&self.0.window);
        w.next_ask += 1;
        let id = w.next_ask;
        w.asks.push_back((id, kind.to_owned(), value));
        while w.asks.len() > WINDOW_ASKS_KEPT {
            w.asks.pop_front();
        }
        id
    }

    /// The requests of agents since the window last looked, oldest first.
    pub fn take_window_asks(&self) -> Vec<(u64, String, Value)> {
        lock(&self.0.window).asks.drain(..).collect()
    }

    /// The window's answer to request `id`.
    pub fn answer_window_ask(&self, id: u64, answer: Value) {
        let mut w = lock(&self.0.window);
        w.answers.insert(id, answer);
        // Answers nobody fetches do not pile up.
        let oldest = id.saturating_sub(WINDOW_ASKS_KEPT as u64);
        w.answers.retain(|k, _| *k > oldest);
    }

    /// The window's answer to request `id` once it is there; taken only once.
    pub fn take_window_answer(&self, id: u64) -> Option<Value> {
        lock(&self.0.window).answers.remove(&id)
    }

    /// The folder of this machine the window's tool windows of this project are registered under
    /// (its stand-in folder), once a window shows it.
    pub fn window_root(&self) -> Option<std::path::PathBuf> {
        lock(&self.0.window).root.clone()
    }

    /// Sets the window's stand-in folder (see [`LiveProject::window_root`]).
    pub fn set_window_root(&self, root: &std::path::Path) {
        lock(&self.0.window).root = Some(root.to_path_buf());
    }

    /// Copies of the host's notifications whose method starts with `prefix`, from now on and also
    /// after a resume (a host terminal or run follows its own events this way).
    pub fn listen_host(&self, prefix: &str) -> std::sync::mpsc::Receiver<(String, Value)> {
        self.with(|l| l.session().listen_host(prefix))
    }

    /// The documents for `remote_documents`.
    pub fn documents(&self) -> Value {
        self.with(|l| {
            l.session().pump(Duration::ZERO);
            json!({
                "host": l.host(), "folder": l.folder(),
                "documents": l.session_ref().describe_docs(), "window": true,
            })
        })
    }

    /// Unsaved documents and pending edits (what closing would lose).
    pub fn unsaved(&self) -> Vec<String> {
        self.with(|l| {
            l.session_ref()
                .docs()
                .filter(|d| d.modified || d.pending() > 0)
                .map(|d| d.doc.clone())
                .collect()
        })
    }

    /// The connection log: the project's lines, then the supervisor's.
    pub fn log(&self) -> Vec<String> {
        let mut out = self.with(|l| l.connection_log());
        out.extend(lock(&self.0.log).iter().cloned());
        out
    }

    /// Edits not acknowledged yet, if the lock is free.
    pub fn pending_edits(&self) -> Option<usize> {
        self.try_with(|l| l.session_ref().pending_edits())
    }

    fn note(&self, line: String) {
        lock(&self.0.log).push(line);
    }

    fn set(&self, state: ConnState) {
        let mut w = lock(&self.0.watch);
        // Nothing replaces `Closed` (SPEC-0317 v9 detail 2); `close` itself sets it once `stop` is set.
        if matches!(w.state, ConnState::Closed) {
            return;
        }
        if w.state != state {
            w.state = state;
            self.0.generation.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Opens a document: its id, text, unsaved state on the backend, and the agreement to keep.
    pub fn open_doc(&self, path: &str) -> Result<(String, String, bool, DocSync), RpcError> {
        self.with(|l| {
            let s = l.session();
            let d = match s.doc(path) {
                Some(d) => d.clone(),
                None => s.open_doc(path)?.clone(),
            };
            let text = d.text();
            Ok((
                d.doc.clone(),
                text.clone(),
                d.modified,
                DocSync {
                    doc: d.doc.clone(),
                    synced: text,
                    modified: d.modified,
                    base_revision: d.server_rev,
                    stamp: d.stamp,
                    base_hash: d.saved_hash.clone(),
                },
            ))
        })
    }

    /// Meets the editor's `editor` text with the replica (`SPEC-0317` behaviour 9): the editor's
    /// change since `sync` goes to the session, the replica's comes back as ranges for the editor.
    /// Never waits for the lock.
    pub fn sync_doc(&self, sync: &mut DocSync, editor: &str) -> Result<Synced, RpcError> {
        let done = self.try_with(|l| meet(l.session(), sync, editor));
        match done {
            Some(r) => r,
            None => {
                if editor != sync.synced
                    && let Some(r) = &self.0.recovery
                {
                    let _ = r.write_copy(&RecoveryCopy {
                        host: self.0.host.clone(),
                        folder: self.0.folder.clone(),
                        path: sync.doc.clone(),
                        base_revision: sync.base_revision,
                        base_hash: sync.base_hash.clone(),
                        pending: Vec::new(),
                        text: editor.to_owned(),
                    });
                }
                Ok(Synced::Busy)
            }
        }
    }

    /// Saves `doc` on the host (waits for the lock and the acknowledgements; while disconnected
    /// the save is remembered and the answer says so).
    pub fn save(&self, doc: &str, overwrite: bool) -> Result<u64, RpcError> {
        self.with(|l| l.session().save(doc, overwrite))
    }

    /// A request to the backend (`fs.*`).
    pub fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.with(|l| l.session_ref().call(method, params))
    }

    /// Lists a folder of the project (entries leaving it are refused).
    pub fn list(&self, path: &str) -> Result<Vec<Value>, RpcError> {
        self.with(|l| l.session().list(path))
    }

    /// File tree events received since the last call, and whether the tree must be read again.
    pub fn take_tree_events(&self) -> Option<(Vec<Value>, bool)> {
        self.try_with(|l| {
            let s = l.session();
            let events = std::mem::take(&mut s.tree_events);
            let stale = std::mem::replace(&mut s.tree_stale, false);
            (events, stale)
        })
    }

    /// The recovery copies of this project left by an earlier window (`SPEC-0317` behaviour 13).
    pub fn recovery_copies(&self) -> Vec<RecoveryCopy> {
        self.0
            .recovery
            .as_ref()
            .map(Recovery::copies)
            .unwrap_or_default()
    }

    /// Restores `copies`: each document is opened and takes the copy's text as an edit, so it is
    /// unsaved until the user saves it.
    pub fn restore(&self, copies: &[RecoveryCopy]) -> Vec<(String, Restored)> {
        self.with(|l| {
            let s = l.session();
            copies
                .iter()
                .map(|c| {
                    let host_text = match s.doc(&c.path) {
                        Some(d) => Ok(d.text()),
                        None => s.open_doc(&c.path).map(|d| d.text()),
                    };
                    let outcome = match host_text {
                        Err(e) => Restored::Failed(format!("{}: {}", e.code.as_str(), e.message)),
                        Ok(t) if t == c.text => Restored::Unchanged,
                        Ok(t) => {
                            let op = TextOp::diff(&t, &c.text);
                            match s.edit(&c.path, &op.to_ops()) {
                                Err(e) => Restored::Failed(e.message),
                                Ok(()) if ferris_idea_session::ot::str_hash(&t) == c.base_hash => {
                                    Restored::Followed
                                }
                                Ok(()) => Restored::HostChanged,
                            }
                        }
                    };
                    (c.path.clone(), outcome)
                })
                .collect()
        })
    }

    /// Removes `copies` without restoring them (the user chose so).
    pub fn discard_copies(&self, copies: &[RecoveryCopy]) {
        if let Some(r) = &self.0.recovery {
            for c in copies {
                r.remove(&c.path);
            }
        }
    }

    /// The document states for the window, if the lock is free.
    pub fn doc_states(&self) -> Option<HashMap<String, DocState>> {
        self.try_with(|l| {
            l.session_ref()
                .docs()
                .map(|d| {
                    (
                        d.doc.clone(),
                        DocState {
                            modified: d.modified || d.local_version.is_some(),
                            pending: d.pending(),
                            disk_changed: d.disk_changed,
                            save_requested: d.save_requested,
                            revision: d.server_rev,
                            stamp: d.stamp,
                        },
                    )
                })
                .collect()
        })
    }

    /// The session's connection while connected: long host requests and streams go through it.
    pub fn host_connection(&self) -> Option<Connection> {
        self.with(|l| l.session_ref().connection())
    }

    /// A request to the host's functions (`SPEC-0318`: language servers, search, Git, terminals,
    /// runs, ports) that may take long: it goes through the connection without holding the
    /// project, so typing and the supervisor go on meanwhile. `unavailable` while disconnected.
    pub fn host_call(
        &self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, RpcError> {
        match self.host_connection() {
            Some(c) => c.call(method, params, limit),
            None => Err(RpcError::new(
                ErrorCode::Unavailable,
                format!("reconnecting to {}", self.0.host),
            )
            .hint("the request is possible again once the connection is back")),
        }
    }

    /// The host's notifications (`lsp.diagnostics`, `term.exited`, `run.event`, `port.detected`,
    /// `git.changed` …) received since the last call; empty while the project is busy.
    pub fn take_host_events(&self) -> Vec<(String, Value)> {
        self.try_with(|l| std::mem::take(&mut l.session().host_events))
            .unwrap_or_default()
    }

    /// The revision of `doc` once the window's edits of it are acknowledged (at most `limit`):
    /// position-bearing host requests are computed on it (`SPEC-0318` behaviour 4).
    pub fn settled_revision(&self, doc: &str, limit: Duration) -> Option<u64> {
        let deadline = Instant::now() + limit;
        loop {
            let r = self.try_with(|l| {
                let s = l.session();
                s.pump(Duration::ZERO);
                s.doc(doc).map(|d| (d.pending(), d.server_rev))
            });
            match r.flatten() {
                Some((0, rev)) => return Some(rev),
                None if Instant::now() >= deadline => return None,
                _ => {}
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A request to the backend if the session is free at once (the tree's New, Rename and
    /// Delete); `unavailable` while it is busy or disconnected.
    pub fn try_call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.try_with(|l| l.session_ref().call(method, params))
            .unwrap_or_else(|| {
                Err(RpcError::new(
                    ErrorCode::Unavailable,
                    format!(
                        "{} is busy reconnecting; try again in a moment",
                        self.0.host
                    ),
                ))
            })
    }

    /// Backend info for the widget's menu: version, platform, folder, session id. Never the
    /// resume token.
    pub fn backend_info(&self) -> Vec<String> {
        self.with(|l| {
            let i = &l.session_ref().info;
            vec![
                format!("Backend: {}", i.build),
                format!("Host platform: {}", i.platform),
                format!("Folder: {}", i.root),
                format!("Session: {}", i.session),
            ]
        })
    }
}

/// The diamond of `SPEC-0317` v3 behaviour 9: `a` is the editor's change since `sync`, `b` the
/// replica's; `b` (sequenced by the backend) goes first.
fn meet(s: &mut ClientSession, sync: &mut DocSync, editor: &str) -> Result<Synced, RpcError> {
    s.pump(Duration::ZERO);
    let Some(d) = s.doc(&sync.doc) else {
        return Err(RpcError::new(
            ErrorCode::NotFound,
            format!("{} is not open", sync.doc),
        ));
    };
    let replica = d.text();
    let mut out = Synced::Same;
    if d.local_version.is_some() {
        // A new backend or a divergence: the replica holds the host's text, the editor still
        // shows the window's — which goes to the backend as an edit; saving asks first.
        let _ = s.take_local_version(&sync.doc);
        let op = TextOp::diff(&replica, editor);
        if !op.is_noop() {
            s.edit(&sync.doc, &op.to_ops())?;
        }
    } else if editor != sync.synced || replica != sync.synced {
        let a = TextOp::diff(&sync.synced, editor);
        let b = TextOp::diff(&sync.synced, &replica);
        let (to_replica, to_editor) = if b.is_noop() {
            (Some(a), None)
        } else if a.is_noop() {
            (None, Some(b))
        } else {
            let (b2, a2) = TextOp::transform(&b, &a)?;
            (Some(a2), Some(b2))
        };
        if let Some(op) = to_replica.filter(|o| !o.is_noop()) {
            s.edit(&sync.doc, &op.to_ops())?;
        }
        if let Some(op) = to_editor.filter(|o| !o.is_noop()) {
            out = Synced::Apply(op.to_ops());
        }
    }
    if let Some(d) = s.doc(&sync.doc) {
        sync.synced = d.text();
        sync.modified = d.modified || d.pending() > 0;
        sync.base_revision = d.server_rev;
        sync.stamp = d.stamp;
        sync.base_hash = d.saved_hash.clone();
    }
    Ok(out)
}

/// One look of the supervisor: pump, heartbeat, and reconnecting on the schedule.
/// Whether the project was closed: `stop` is set under the link's lock before `Closed` is.
fn closed(s: &Shared, w: &Watch) -> bool {
    s.stop.load(Ordering::SeqCst) || matches!(w.state, ConnState::Closed)
}

/// The end of a round that found the connection up: the state becomes Connected — unless the
/// user disconnected or closed the project meanwhile, since the connection was looked at without
/// the state's lock.
fn found_up(s: &Shared) {
    let mut w = lock(&s.watch);
    // Disconnect sets `hold` before its state: a user's Disconnect while this round found the
    // connection up stays (it was overwritten with Connected under load, CI 2026-11-13). A close
    // while this round found it up stays closed alike (it was listed as connected, CI 2026-11-15).
    if s.hold.load(Ordering::SeqCst) || closed(s, &w) {
        return;
    }
    if w.broke_at.is_some() || !matches!(w.state, ConnState::Connected { .. }) {
        w.broke_at = None;
        w.attempt = 0;
        s.generation.fetch_add(1, Ordering::SeqCst);
    }
    let state = ConnState::Connected {
        round_trip_ms: w.rtt,
    };
    if w.state != state {
        w.state = state;
        s.generation.fetch_add(1, Ordering::SeqCst);
    }
}

fn supervise(s: &Shared) {
    let t = &s.timing;
    let connected = {
        let mut l = lock(&s.link);
        l.session().pump(Duration::ZERO);
        l.session_ref().connected()
    };
    let now = Instant::now();
    if connected {
        let due = lock(&s.watch)
            .last_ping
            .is_none_or(|p| now.duration_since(p) >= t.heartbeat);
        if due {
            let rtt = lock(&s.link).session().ping_within(t.heartbeat_limit);
            let mut w = lock(&s.watch);
            w.last_ping = Some(Instant::now());
            match rtt {
                Ok(d) => w.rtt = Some(d.as_millis() as u64),
                Err(e) => {
                    drop(w);
                    lock(&s.log).push(format!("no heartbeat answer: {}", e.message));
                    lock(&s.link).break_connection();
                    return;
                }
            }
        }
        found_up(s);
        return;
    }
    let asked = s.now.swap(false, Ordering::SeqCst);
    if s.hold.load(Ordering::SeqCst) && !asked {
        return;
    }
    let mut w = lock(&s.watch);
    if closed(s, &w) {
        return;
    }
    let fresh = w.broke_at.is_none();
    let broke_at = *w.broke_at.get_or_insert(now);
    if fresh {
        w.attempt = 0;
        w.next_at = Some(broke_at + t.delay(1));
        lock(&s.log).push("connection lost; reconnecting".into());
    }
    let left = t.reattach.saturating_sub(now.duration_since(broke_at));
    if left.is_zero() && !asked {
        let state = ConnState::Disconnected {
            reason: "the host kept the session for its reattach period; Reconnect starts a new backend and keeps every unsaved edit".into(),
        };
        if w.state != state {
            w.state = state;
            s.generation.fetch_add(1, Ordering::SeqCst);
        }
        return;
    }
    let next_at = if asked { now } else { w.next_at.unwrap_or(now) };
    if now < next_at {
        let state = ConnState::Reconnecting {
            attempt: w.attempt + 1,
            next_in_s: next_at.duration_since(now).as_secs_f64().ceil() as u64,
            left_s: left.as_secs(),
        };
        if w.state != state {
            w.state = state;
            s.generation.fetch_add(1, Ordering::SeqCst);
        }
        return;
    }
    w.attempt += 1;
    let attempt = w.attempt;
    w.state = ConnState::Reconnecting {
        attempt,
        next_in_s: 0,
        left_s: left.as_secs(),
    };
    s.generation.fetch_add(1, Ordering::SeqCst);
    drop(w);
    let r = {
        let mut l = lock(&s.link);
        // Closed since this round began: no reconnection of a closed project.
        if s.stop.load(Ordering::SeqCst) {
            return;
        }
        l.reconnect()
    };
    let mut w = lock(&s.watch);
    if closed(s, &w) {
        return;
    }
    // Disconnected by the user while the attempt ran: its outcome does not change the state.
    if s.hold.load(Ordering::SeqCst) {
        if r.is_ok() {
            lock(&s.log).push(format!(
                "attempt {attempt}: connected after Disconnect; left"
            ));
        }
        return;
    }
    match r {
        Ok(outcome) => {
            lock(&s.log).push(format!("attempt {attempt}: {outcome:?}"));
            w.broke_at = None;
            w.attempt = 0;
            w.next_at = None;
            w.last_ping = None;
            w.state = ConnState::Connected {
                round_trip_ms: w.rtt,
            };
        }
        Err(e) => {
            lock(&s.log).push(format!("attempt {attempt} failed: {e}"));
            w.next_at = Some(Instant::now() + t.delay(attempt + 1));
        }
    }
    s.generation.fetch_add(1, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project open in a window whose supervisor never runs by itself (the test plays it).
    fn idle_project(tag: &str) -> (LiveProject, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-close-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        std::fs::write(dir.join("project/a.txt"), "a\n").unwrap();
        let p = crate::inprocess::InProcessProject::open(
            "close-host",
            &dir.join("project"),
            &dir.join("state"),
            Duration::from_secs(600),
            None,
            Duration::ZERO,
        )
        .unwrap();
        let timing = Timing {
            tick: Duration::from_secs(3600),
            ..Timing::default()
        };
        let live = LiveProject::start(Box::new(p), timing);
        live.publish();
        (live, dir)
    }

    fn listed(live: &LiveProject) -> bool {
        open_projects().iter().any(|p| Arc::ptr_eq(&p.0, &live.0))
    }

    /// SPEC-0317 v9 AK-12 (detail 2), the race of CI 2026-11-15 played in order: a supervisor
    /// round finds the connection up, the project is closed, then the round ends. The project
    /// stays closed and unlisted; neither the supervisor nor an agent's Reconnect reconnects it.
    #[test]
    fn a_closed_project_stays_closed_whatever_a_round_found_before() {
        let (live, dir) = idle_project("stale");
        assert!(listed(&live));
        // The round's first half: the connection is up.
        assert!(live.with(|l| l.session_ref().connected()));
        live.close(Unsaved::Discard).unwrap();
        // The round's second half, after the close.
        found_up(&live.0);
        assert_eq!(live.state(), ConnState::Closed);
        assert!(!listed(&live), "a closed project leaves the list at once");
        // Later rounds and a Reconnect do not bring it back or wait for a reattachment.
        supervise(&live.0);
        supervise(&live.0);
        assert!(live.reconnect_wait().is_err());
        live.reconnect_now();
        supervise(&live.0);
        assert_eq!(live.state(), ConnState::Closed);
        assert!(
            !live.log().iter().any(|l| l.contains("attempt")),
            "{:?}",
            live.log()
        );
        assert!(!listed(&live));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same with real threads: a supervisor that runs round after round (a heartbeat in
    /// every one) while the project is closed; at no point after the close is it listed again.
    #[test]
    fn closing_during_running_rounds_ends_closed() {
        for round in 0..20 {
            let (live, dir) = idle_project(&format!("rounds-{round}"));
            let shared = live.0.clone();
            let done = Arc::new(AtomicBool::new(false));
            let rounds = {
                let (shared, done) = (shared.clone(), done.clone());
                std::thread::spawn(move || {
                    while !done.load(Ordering::SeqCst) {
                        lock(&shared.watch).last_ping = None;
                        supervise(&shared);
                    }
                })
            };
            std::thread::sleep(Duration::from_millis(round % 5));
            live.close(Unsaved::Discard).unwrap();
            for _ in 0..50 {
                assert!(
                    !listed(&live),
                    "listed again after the close (round {round})"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            done.store(true, Ordering::SeqCst);
            rounds.join().unwrap();
            assert_eq!(live.state(), ConnState::Closed, "round {round}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn the_schedule_is_one_two_four_eight_sixteen_then_thirty_seconds() {
        let t = Timing::default();
        let waits: Vec<u64> = (1..=8).map(|n| t.delay(n).as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(t.reattach, Duration::from_secs(600));
    }

    #[test]
    fn the_widget_names_host_state_and_pending_edits() {
        let c = ConnState::Connected {
            round_trip_ms: Some(12),
        };
        assert_eq!(c.widget_text("build"), "build · 12 ms");
        assert_eq!(c.label(), "Connected");
        let r = ConnState::Reconnecting {
            attempt: 3,
            next_in_s: 4,
            left_s: 541,
        };
        assert_eq!(r.widget_text("build"), "build · Reconnecting (attempt 3)");
        assert_eq!(
            r.tooltip("build", 2),
            "Reconnecting to build: attempt 3 in 4 s, the host keeps the session for 9:01 more\n2 edits wait for the host"
        );
        let d = ConnState::Disconnected { reason: "x".into() };
        assert_eq!(d.widget_text("h"), "h · Disconnected");
        assert_eq!(
            serde_json::to_value(&r).unwrap()["state"],
            json!("reconnecting")
        );
    }
}
