//! The backend half of the session protocol, the heart of `idecraft serve` (`ADR-0026`,
//! `SPEC-0317`, M10.3): one workspace, its documents, sessions that survive a broken connection
//! for the reattach period, rights per session, the numbered event stream, file watching, the
//! audit log and the reattach socket.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use idecraft_editor::documents::DocumentRegistry;
use serde_json::{Value, json};

use crate::docs::{DiskEvent, DocInfo, DocStore, EditOutcome};
use crate::events::EventLog;
use crate::frame::{Frame, FrameKind, read_frame};
use crate::hello::{
    CAPABILITIES, MINOR, PROTOCOL, Token, choose_major, random_id, supported_majors,
};
use crate::ot::Op;
use crate::rpc::{ErrorCode, Message, RpcError};
use crate::transport::{CancelToken, Credits, Outbox, Priority, Stop, Transport};

/// What a session may do (`ADR-0025` §4). An SSH session has all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Right {
    /// Read projects, files, documents.
    View,
    /// Change documents, save, change files.
    Edit,
    /// Run configurations and terminals.
    Run,
    /// Debug sessions.
    Debug,
    /// Commit, branch, merge.
    VcsWrite,
    /// Push to remotes.
    Push,
    /// Port forwarding.
    Ports,
    /// Start and steer agent tasks.
    Agents,
}

impl Right {
    /// Every right.
    pub const ALL: [Right; 8] = [
        Self::View,
        Self::Edit,
        Self::Run,
        Self::Debug,
        Self::VcsWrite,
        Self::Push,
        Self::Ports,
        Self::Agents,
    ];

    /// Its name on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Edit => "edit",
            Self::Run => "run",
            Self::Debug => "debug",
            Self::VcsWrite => "vcs-write",
            Self::Push => "push",
            Self::Ports => "ports",
            Self::Agents => "agents",
        }
    }
}

/// The right a built-in method needs (`SPEC-0317` v2 detail 8).
pub fn right_for(method: &str) -> Option<Right> {
    match method {
        "fs.list" | "fs.stat" | "fs.read" | "fs.watch" | "doc.open" | "doc.snapshot"
        | "doc.changes" | "doc.close" | "event.subscribe" | "device.whoami" | "notice.list" => {
            Some(Right::View)
        }
        "doc.edit" | "doc.save" | "fs.write" | "fs.create" | "fs.rename" | "fs.delete" => {
            Some(Right::Edit)
        }
        _ => None,
    }
}

/// The method family of the UI driver (`SPEC-0319` behaviour 31): reserved for the process that
/// owns the window (phase 4, paired devices with the grant `ui`). A backend never serves it, and a
/// host never operates a window (`ADR-0025` T5), so `ui.*` is refused with `denied`.
pub const RESERVED_UI_FAMILY: &str = "ui";

/// Whether `method` belongs to the reserved `ui` family.
pub fn is_ui_method(method: &str) -> bool {
    method
        .strip_prefix(RESERVED_UI_FAMILY)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// Methods written to the audit log (`SPEC-0317` behaviour 19).
fn audited(method: &str) -> bool {
    is_ui_method(method)
        || matches!(
            method,
            "session.hello"
                | "session.resume"
                | "session.bye"
                | "doc.save"
                | "fs.write"
                | "fs.create"
                | "fs.rename"
                | "fs.delete"
        )
}

/// What a paired device's request adds to the audit log (`SPEC-0324` behaviour 10): everything
/// except the keepalives; `doc.edit` is coalesced by the caller.
fn audited_for_device(method: &str) -> bool {
    !matches!(method, "session.ping" | "session.ack" | "$/cancel")
}

/// How long `doc.edit` of one document and session stays out of the log after a line.
const EDIT_AUDIT_EVERY: Duration = Duration::from_secs(10);
/// Notices the backend keeps (`SPEC-0324` behaviour 13).
const NOTICES: usize = 200;

/// How the backend runs.
#[derive(Clone)]
pub struct BackendConfig {
    /// Version and build reported in `session.hello`.
    pub build: String,
    /// The only folder this backend serves (`serve <folder>`), if fixed.
    pub folder: Option<PathBuf>,
    /// Rights of every session (an SSH session: all).
    pub rights: Vec<Right>,
    /// How long a broken session waits for reattachment (`ADR-0025` §8: 10 minutes).
    pub reattach: Duration,
    /// The first message must arrive within this time (10 s).
    pub hello_timeout: Duration,
    /// A connection without any frame for this long is a break (45 s).
    pub idle_timeout: Duration,
    /// Folder for `remote/audit.jsonl` (`None`: no audit log).
    pub state_dir: Option<PathBuf>,
    /// Folder of reattach sockets (`None`: no socket, e.g. in process).
    pub run_dir: Option<PathBuf>,
    /// The peer as written to the audit log (`ssh:<user>@<window host>`).
    pub peer: String,
    /// Where documents are mirrored for agents (`SPEC-0220`).
    pub registry: DocumentRegistry,
    /// Notifications kept per session (entries, bytes).
    pub ring: (usize, usize),
    /// File watching interval (coalescing window, 100 ms).
    pub watch_interval: Duration,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            build: format!("idecraft {}", env!("CARGO_PKG_VERSION")),
            folder: None,
            rights: Right::ALL.to_vec(),
            reattach: Duration::from_secs(600),
            hello_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(45),
            state_dir: None,
            run_dir: None,
            peer: "local".into(),
            registry: DocumentRegistry::default(),
            ring: (crate::events::RING_ENTRIES, crate::events::RING_BYTES),
            watch_interval: Duration::from_millis(100),
        }
    }
}

/// Who is on a connection that is not the user themself (`SPEC-0321` behaviour 18): a paired
/// device with its own rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// The name for the audit log (`device:<fingerprint>:<name>`).
    pub name: String,
    /// The device's rights.
    pub rights: Vec<Right>,
}

/// What a registered handler gets.
pub struct Call {
    /// The request's parameters.
    pub params: Value,
    /// Set by `$/cancel` or the deadline; long handlers check it.
    pub cancel: CancelToken,
    /// The calling session's client id.
    pub client: String,
    /// The project root.
    pub root: PathBuf,
    /// The calling session: notifications, streams, cleanup when it ends (`SPEC-0318`).
    pub session: SessionHandle,
    /// The calling session's rights (a method may need more for some of its options).
    pub rights: Vec<Right>,
}

/// A handler of a method family bound later (`lsp`, `term`, …).
pub type Handler = Arc<dyn Fn(&Call) -> Result<Value, RpcError> + Send + Sync>;

/// What arrives on a stream the window writes to (terminal input, a forwarded connection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamIn {
    /// Bytes from the window.
    Data(Vec<u8>),
    /// The window closed the stream, with its reason.
    Closed(String),
}

/// Receives what the window writes to one stream; it runs on the connection's reading thread, so
/// it must not block for long (the credit goes back once it returns).
pub type StreamSink = Arc<dyn Fn(StreamIn) + Send + Sync>;

struct Registered {
    right: Right,
    handler: Handler,
    audit: bool,
}

#[derive(Clone)]
struct Link {
    generation: u64,
    out: Arc<Outbox>,
    credits: Arc<Credits>,
    closer: Arc<dyn Fn() + Send + Sync>,
}

impl Link {
    fn send(&self, text: String, priority: Priority) {
        if let Ok(bytes) = Frame::message(text).encode() {
            self.out.push(bytes, priority);
        }
    }

    fn reply(&self, id: Value, result: Result<Value, RpcError>) {
        let text = Message::Response {
            id: id.clone(),
            result,
        }
        .to_text();
        match Frame::message(text).encode() {
            Ok(bytes) => {
                self.out.push(bytes, Priority::Interactive);
            }
            Err(_) => {
                let e = RpcError::new(ErrorCode::TooLarge, "the answer is larger than 1 MiB")
                    .hint("ask for less (a sub-folder, a stream)");
                self.send(
                    Message::Response { id, result: Err(e) }.to_text(),
                    Priority::Interactive,
                );
            }
        }
    }

    fn drop_now(&self) {
        self.out.close();
        (self.closer)();
    }
}

struct Session {
    token: Token,
    client: String,
    rights: Vec<Right>,
    /// The paired device of this session; `None` for the user themself.
    peer: Option<String>,
    events: EventLog,
    link: Option<Link>,
    detached: Option<Instant>,
    wrong_tokens: u32,
    watch: Option<Vec<String>>,
    socket: Option<(PathBuf, Arc<AtomicBool>)>,
    on_end: Vec<Box<dyn FnOnce() + Send>>,
}

struct Workspace {
    root: PathBuf,
    store: DocStore,
    _watch: Option<idecraft_core::WatchHandle>,
}

#[derive(Default)]
struct State {
    ws: Option<Workspace>,
    sessions: HashMap<String, Session>,
    by_client: HashMap<String, String>,
    next_client: u64,
    generation: u64,
    ever: bool,
}

impl State {
    fn notify(&mut self, sid: &str, method: &str, params: Value, priority: Priority) {
        if let Some(s) = self.sessions.get_mut(sid) {
            let (_, text) = s.events.push(method, params);
            if let Some(link) = &s.link {
                link.send(text, priority);
            }
        }
    }

    fn notify_client(&mut self, client: &str, method: &str, params: Value) {
        if let Some(sid) = self.by_client.get(client).cloned() {
            self.notify(&sid, method, params, Priority::Interactive);
        }
    }
}

struct Shared {
    config: BackendConfig,
    state: Mutex<State>,
    idle: Condvar,
    handlers: Mutex<HashMap<String, Registered>>,
    families: Mutex<BTreeSet<String>>,
    next_stream: AtomicU32,
    inputs: Mutex<HashMap<u32, (String, StreamSink)>>,
    room: std::sync::OnceLock<Arc<dyn RoomLink>>,
    /// The last audit line of `doc.edit` per session and document (`SPEC-0324` behaviour 10).
    edit_audit: Mutex<HashMap<(String, String), Instant>>,
    /// The newest notices, oldest first (`SPEC-0324` behaviour 13).
    notices: Mutex<std::collections::VecDeque<Value>>,
    next_notice: std::sync::atomic::AtomicU64,
    /// Called with every batch of the workspace watcher (`SPEC-0042` v11 behaviour 20).
    disk_listeners: Mutex<Vec<DiskListener>>,
}

/// Gets the workspace root and the changes of one watcher batch.
pub type DiskListener = Box<dyn Fn(&Path, &[idecraft_core::FileChange]) + Send + Sync>;

/// What a project room adds to a backend (`SPEC-0323`).
pub trait RoomLink: Send + Sync {
    /// A workspace opened; the sink and blob folder when that project already has a room. Called
    /// with the backend's state locked.
    fn workspace(&self, root: &Path) -> Option<(crate::docs::StoreSink, PathBuf)>;
    /// Decides whether `session` may call `method` while a room exists; joins a known member.
    fn gate(
        &self,
        backend: &Backend,
        session: &SessionHandle,
        peer: &str,
        method: &str,
    ) -> Result<(), RpcError>;
    /// A new undo group.
    fn next_group(&self) -> u64;
    /// Whether `link` is an anonymous link of the room of `root` that works now (`SPEC-0325`
    /// behaviour 13), whether or not that room is already loaded.
    fn anonymous_link_live(&self, root: &Path, link: &str) -> bool;
    /// The sessions that are guests of the room; they get no host notices (`SPEC-0323`
    /// behaviour 21a).
    fn guest_sessions(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The backend: cheap to clone, shared by every connection.
#[derive(Clone)]
pub struct Backend(Arc<Shared>);

/// A backend reference that does not keep it alive (threads that follow it).
#[derive(Clone)]
pub struct WeakBackend(Weak<Shared>);

impl WeakBackend {
    /// The backend, while it runs.
    pub fn upgrade(&self) -> Option<Backend> {
        self.0.upgrade().map(Backend)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn log(line: &str) {
    // Standard error only: on stdio, standard output carries nothing but frames.
    eprintln!("idecraft serve: {line}");
}

struct ConnectionLink {
    peer: Option<Peer>,
    out: Arc<Outbox>,
    credits: Arc<Credits>,
    closer: Arc<dyn Fn() + Send + Sync>,
}

impl Backend {
    /// A backend with `config`; its reaper ends sessions whose reattach period passed.
    pub fn new(config: BackendConfig) -> Self {
        let shared = Arc::new(Shared {
            config,
            state: Mutex::new(State::default()),
            idle: Condvar::new(),
            handlers: Mutex::new(HashMap::new()),
            families: Mutex::new(BTreeSet::new()),
            next_stream: AtomicU32::new(2),
            inputs: Mutex::new(HashMap::new()),
            room: std::sync::OnceLock::new(),
            edit_audit: Mutex::new(HashMap::new()),
            notices: Mutex::new(std::collections::VecDeque::new()),
            next_notice: std::sync::atomic::AtomicU64::new(1),
            disk_listeners: Mutex::default(),
        });
        let weak: Weak<Shared> = Arc::downgrade(&shared);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let Some(shared) = weak.upgrade() else {
                    break;
                };
                Backend(shared).reap();
            }
        });
        Self(shared)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Connects the project room (`SPEC-0323`).
    pub fn attach_room(&self, link: Arc<dyn RoomLink>) {
        let _ = self.0.room.set(link);
    }

    /// Puts the open workspace into room mode.
    pub fn enter_room(&self, sink: crate::docs::StoreSink, blobs: PathBuf) {
        if let Some(ws) = self.state().ws.as_mut() {
            ws.store.enter_room(sink, blobs);
        }
    }

    /// Who the session is, by the channel: the paired device, or the backend's peer.
    pub fn session_peer(&self, sid: &str) -> String {
        self.state()
            .sessions
            .get(sid)
            .and_then(|s| s.peer.clone())
            .unwrap_or_else(|| self.0.config.peer.clone())
    }

    /// The session's rights now.
    pub fn session_rights(&self, sid: &str) -> Vec<Right> {
        self.state()
            .sessions
            .get(sid)
            .map(|s| s.rights.clone())
            .unwrap_or_default()
    }

    /// Reduces the session's rights to those in `rights` (`SPEC-0323` behaviour 14).
    pub fn narrow_session(&self, sid: &str, rights: &[Right]) {
        if let Some(s) = self.state().sessions.get_mut(sid) {
            s.rights.retain(|r| rights.contains(r));
        }
    }

    /// Ends one session at once (`SPEC-0323` behaviour 16).
    pub fn end_session_now(&self, sid: &str, why: &str) {
        let mut st = self.state();
        self.end_session(&mut st, sid, why);
    }

    /// Opens `path` for the room and returns it.
    pub fn open_for_room(&self, path: &str) -> Result<DocInfo, RpcError> {
        self.with_store(|s| s.open_for_room(path))
    }

    /// Applies a change set as one ordered step and tells the clients; returns the documents'
    /// new revisions (`SPEC-0323` behaviour 4).
    pub fn apply_room_set(
        &self,
        author: &str,
        group: u64,
        changes: &[crate::docs::SetChange],
    ) -> Result<Value, RpcError> {
        let mut st = self.state();
        let ws = st
            .ws
            .as_mut()
            .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no workspace"))?;
        let done = ws.store.apply_set(author, group, changes)?;
        let mut notes = Vec::new();
        for (old, new) in &done.moved {
            for c in ws.store.clients(new) {
                notes.push((c, "doc.renamed", json!({"doc": old, "to": new})));
            }
        }
        let mut documents = serde_json::Map::new();
        for (doc, o) in &done.edits {
            documents.insert(doc.clone(), json!({"revision": o.revision, "hash": o.hash}));
            if o.changed {
                for c in ws.store.clients(doc) {
                    notes.push((
                        c,
                        "doc.changed",
                        json!({"doc": doc, "revision": o.revision, "ops": o.ops, "author": author, "hash": o.hash}),
                    ));
                }
            }
        }
        for (c, method, params) in notes {
            st.notify_client(&c, method, params);
        }
        Ok(Value::Object(documents))
    }

    /// Saves the room's modified documents (`SPEC-0323` behaviour 6).
    pub fn autosave_room(&self) {
        let mut st = self.state();
        let saved = match st.ws.as_mut() {
            Some(ws) if ws.store.in_room() => ws
                .store
                .autosave()
                .into_iter()
                .map(|(doc, revision)| (ws.store.clients(&doc), doc, revision))
                .collect::<Vec<_>>(),
            _ => return,
        };
        for (clients, doc, revision) in saved {
            for c in clients {
                st.notify_client(&c, "doc.saved", json!({"doc": doc, "revision": revision}));
            }
        }
    }

    /// A file operation of a session in a project with a room, ordered like an edit.
    fn room_fs(&self, client: &str, method: &str, params: &Value) -> Result<Value, RpcError> {
        use crate::docs::SetChange;
        let text_of = |name: &str| {
            params[name]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| RpcError::new(ErrorCode::Invalid, format!("`{name}` is missing")))
        };
        let path = params["path"].as_str().unwrap_or_default().to_owned();
        let change = match method {
            "fs.write" => {
                let text = text_of("text")?;
                match self.open_for_room(&path) {
                    Ok(info) => SetChange::Edit {
                        path: info.doc,
                        base: Some(info.revision),
                        ops: crate::ot::TextOp::diff(&info.text, &text).to_ops(),
                    },
                    Err(e) if e.code == ErrorCode::NotFound => SetChange::Create {
                        path: path.clone(),
                        dir: false,
                        text: Some(text),
                        blob: None,
                    },
                    Err(e) => return Err(e),
                }
            }
            "fs.create" => SetChange::Create {
                path: path.clone(),
                dir: params["kind"].as_str() == Some("dir"),
                text: None,
                blob: None,
            },
            "fs.rename" => SetChange::Rename {
                from: text_of("from")?,
                to: text_of("to")?,
            },
            _ => {
                let root = self
                    .root()
                    .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no workspace"))?;
                let full = crate::paths::resolve(&root, &path)?;
                if full.is_dir() && params["recursive"].as_bool() != Some(true) {
                    return Err(RpcError::new(
                        ErrorCode::Invalid,
                        format!("{path} is a folder; pass recursive: true"),
                    ));
                }
                SetChange::Delete { path: path.clone() }
            }
        };
        let group = self
            .0
            .room
            .get()
            .map(|r| r.next_group())
            .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no room"))?;
        self.apply_room_set(client, group, std::slice::from_ref(&change))?;
        match change {
            SetChange::Rename { from, to } => Ok(json!({ "from": from, "to": to })),
            SetChange::Edit { path, .. }
            | SetChange::Create { path, .. }
            | SetChange::Delete { path } => Ok(json!({ "path": path })),
        }
    }

    fn in_room(&self) -> bool {
        self.state().ws.as_ref().is_some_and(|w| w.store.in_room())
    }

    /// A reference that does not keep the backend alive.
    pub fn downgrade(&self) -> WeakBackend {
        WeakBackend(Arc::downgrade(&self.0))
    }

    /// The configuration.
    pub fn config(&self) -> &BackendConfig {
        &self.0.config
    }

    /// Binds `method` of capability `family` to `handler`, behind `right`. Handlers run on their
    /// own thread with a cancellation handle (`$/cancel`, `deadline_ms`).
    pub fn register(&self, family: &str, method: &str, right: Right, handler: Handler) {
        self.register_inner(family, method, right, false, handler);
    }

    /// [`Backend::register`] for a method that changes or runs something: each call is written
    /// to the audit log with its outcome (`ADR-0025` §4, `SPEC-0318` behaviour 6).
    pub fn register_audited(&self, family: &str, method: &str, right: Right, handler: Handler) {
        self.register_inner(family, method, right, true, handler);
    }

    fn register_inner(
        &self,
        family: &str,
        method: &str,
        right: Right,
        audit: bool,
        handler: Handler,
    ) {
        // The `ui` family is never bound on a backend (SPEC-0319 behaviour 31).
        if family == RESERVED_UI_FAMILY || is_ui_method(method) {
            debug_assert!(false, "the `ui` family is reserved: {method}");
            return;
        }
        self.0
            .handlers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                method.to_owned(),
                Registered {
                    right,
                    handler,
                    audit,
                },
            );
        self.0
            .families
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(family.to_owned());
    }

    /// The capabilities offered in `session.hello`.
    pub fn capabilities(&self) -> Vec<String> {
        let mut caps: Vec<String> = CAPABILITIES.iter().map(|c| (*c).to_owned()).collect();
        caps.extend(
            self.0
                .families
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned(),
        );
        caps
    }

    /// The methods bound on this backend, sorted (the surface [`crate::hello::MINOR`] stands for).
    pub fn method_names(&self) -> Vec<String> {
        let handlers = self.0.handlers.lock().unwrap_or_else(|e| e.into_inner());
        let mut names: Vec<String> = handlers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Live sessions (connected or waiting for reattachment).
    pub fn sessions(&self) -> usize {
        self.state().sessions.len()
    }

    /// Whether session `id` exists and is connected.
    pub fn connected(&self, id: &str) -> Option<bool> {
        self.state().sessions.get(id).map(|s| s.link.is_some())
    }

    /// Blocks until a session existed and none is left (bye, or reattach period over), or until
    /// `limit` passes; returns whether it became idle.
    pub fn wait_idle(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut st = self.state();
        loop {
            if st.ever && st.sessions.is_empty() {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            st = self
                .0
                .idle
                .wait_timeout(st, (deadline - now).min(Duration::from_millis(500)))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// The audit log file, when there is a state folder.
    pub fn audit_file(&self) -> Option<PathBuf> {
        self.0
            .config
            .state_dir
            .as_ref()
            .map(|d| d.join("remote/audit.jsonl"))
    }

    /// Whether `link` is an anonymous link of the fixed project's room that works now
    /// (`SPEC-0325` behaviour 13).
    pub fn anonymous_link_live(&self, link: &str) -> bool {
        match (self.0.room.get(), self.0.config.folder.as_ref()) {
            (Some(room), Some(folder)) => room.anonymous_link_live(folder, link),
            _ => false,
        }
    }

    /// Writes one line to the audit log (method, target, outcome; never content).
    pub fn audit_event(&self, method: &str, target: Option<&str>, outcome: &str) {
        self.audit(method, target, outcome);
    }

    /// Ends every session whose device name starts with `prefix` at once (revocation,
    /// `SPEC-0321` behaviour 19); nothing waits for a reattachment.
    pub fn end_sessions_of(&self, prefix: &str) {
        let mut st = self.state();
        let ids: Vec<String> = st
            .sessions
            .iter()
            .filter(|(_, s)| s.peer.as_deref().is_some_and(|p| p.starts_with(prefix)))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.end_session(&mut st, &id, "revoked");
        }
    }

    /// Ends the sessions of a device with `why` in the log (`SPEC-0324` behaviours 8, 11).
    pub fn end_sessions_why(&self, prefix: &str, why: &str) {
        let mut st = self.state();
        let ids: Vec<String> = st
            .sessions
            .iter()
            .filter(|(_, s)| s.peer.as_deref().is_some_and(|p| p.starts_with(prefix)))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.end_session(&mut st, &id, why);
        }
    }

    /// Gives the live sessions of a device new rights (`SPEC-0324` behaviour 8). A session that
    /// loses a right ends, so that no open stream keeps it; one that only gains rights keeps
    /// running and is told. Answers whether any session ended.
    pub fn set_rights_of(&self, prefix: &str, rights: &[Right]) -> bool {
        let mut st = self.state();
        let peers: Vec<(String, bool, bool)> = st
            .sessions
            .iter()
            .filter(|(_, s)| s.peer.as_deref().is_some_and(|p| p.starts_with(prefix)))
            .map(|(id, s)| {
                let narrowed = s.rights.iter().any(|r| !rights.contains(r));
                let changed = narrowed || rights.iter().any(|r| !s.rights.contains(r));
                (id.clone(), narrowed, changed)
            })
            .collect();
        let mut ended = false;
        for (id, narrowed, changed) in peers {
            if narrowed {
                self.end_session(&mut st, &id, "rights-reduced");
                ended = true;
            } else if changed {
                let names: Vec<&str> = rights.iter().map(|r| r.name()).collect();
                if let Some(session) = st.sessions.get_mut(&id) {
                    session.rights = rights.to_vec();
                }
                st.notify(
                    &id,
                    "device.rights",
                    json!({ "rights": names }),
                    Priority::Interactive,
                );
            }
        }
        ended
    }

    /// Posts a notice to every session and keeps it for `notice.list` (`SPEC-0324` behaviour 13).
    pub fn post_notice(&self, kind: &str, title: &str, detail: Option<&str>) {
        let seq = self.0.next_notice.fetch_add(1, Ordering::SeqCst);
        let notice = json!({
            "seq": seq, "time": now_ms(), "kind": kind, "title": title, "detail": detail,
        });
        if let Ok(mut notices) = self.0.notices.lock() {
            if notices.len() >= NOTICES {
                notices.pop_front();
            }
            notices.push_back(notice.clone());
        }
        // Guests of a room get no host notices; asked before the state lock is taken.
        let guests = self
            .0
            .room
            .get()
            .map(|r| r.guest_sessions())
            .unwrap_or_default();
        let mut st = self.state();
        let ids: Vec<String> = st
            .sessions
            .keys()
            .filter(|sid| !guests.contains(sid))
            .cloned()
            .collect();
        for sid in ids {
            st.notify(&sid, "notice.posted", notice.clone(), Priority::Bulk);
        }
    }

    /// The workspace's root, once a session opened it.
    pub fn root(&self) -> Option<PathBuf> {
        self.state().ws.as_ref().map(|w| w.root.clone())
    }

    /// The current state of open document `doc`.
    pub fn document(&self, doc: &str) -> Option<DocInfo> {
        self.state().ws.as_ref()?.store.snapshot(doc).ok()
    }

    /// The open documents with their revisions.
    pub fn documents(&self) -> Vec<(String, u64)> {
        let st = self.state();
        let Some(ws) = st.ws.as_ref() else {
            return Vec::new();
        };
        ws.store
            .ids()
            .into_iter()
            .filter_map(|id| ws.store.snapshot(&id).ok().map(|d| (id, d.revision)))
            .collect()
    }

    /// Sends `method` to every session that has `doc` open (bulk priority: diagnostics).
    pub fn notify_doc(&self, doc: &str, method: &str, params: Value) {
        let mut st = self.state();
        let clients = st
            .ws
            .as_ref()
            .map(|w| w.store.clients(doc))
            .unwrap_or_default();
        for c in clients {
            if let Some(sid) = st.by_client.get(&c).cloned() {
                st.notify(&sid, method, params.clone(), Priority::Bulk);
            }
        }
    }

    /// Sends `method` to every session.
    pub fn broadcast(&self, method: &str, params: Value) {
        let mut st = self.state();
        let ids: Vec<String> = st.sessions.keys().cloned().collect();
        for sid in ids {
            st.notify(&sid, method, params.clone(), Priority::Bulk);
        }
    }

    /// Changes open document `doc` on the backend's own behalf (a rename or code action of a
    /// language server, `SPEC-0318` behaviour 12): `ops` refer to its current text; every client
    /// receives `doc.changed` with `author`.
    pub fn host_edit(&self, doc: &str, author: &str, ops: &[Op]) -> Result<EditOutcome, RpcError> {
        let mut st = self.state();
        let ws = st
            .ws
            .as_mut()
            .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no workspace"))?;
        let o = ws.store.host_edit(doc, author, ops)?;
        let clients = ws.store.clients(doc);
        if o.changed {
            for c in clients {
                st.notify_client(
                    &c,
                    "doc.changed",
                    json!({"doc": doc, "revision": o.revision, "ops": o.ops, "author": author, "hash": o.hash}),
                );
            }
        }
        Ok(o)
    }

    fn audit(&self, method: &str, path: Option<&str>, outcome: &str) {
        self.audit_for(None, method, path, outcome);
    }

    /// Audits as the session `sid`, whose device (if any) is the peer.
    fn audit_session(&self, sid: &str, method: &str, path: Option<&str>, outcome: &str) {
        let peer = self.state().sessions.get(sid).and_then(|s| s.peer.clone());
        self.audit_for(peer.as_deref(), method, path, outcome);
    }

    fn audit_for(&self, peer: Option<&str>, method: &str, path: Option<&str>, outcome: &str) {
        let Some(dir) = &self.0.config.state_dir else {
            return;
        };
        let remote = dir.join("remote");
        if crate::reattach::private_dir(dir).is_ok()
            && crate::reattach::private_dir(&remote).is_ok()
        {
            let line = json!({
                "time": now_ms(), "peer": peer.unwrap_or(&self.0.config.peer), "method": method,
                "path": path, "outcome": outcome,
            })
            .to_string();
            if let Err(e) = crate::reattach::append_private(&remote.join("audit.jsonl"), &line) {
                log(&format!("audit log: {e}"));
            }
        }
    }

    fn reap(&self) {
        let limit = self.0.config.reattach;
        let mut st = self.state();
        let expired: Vec<String> = st
            .sessions
            .iter()
            .filter(|(_, s)| s.detached.is_some_and(|t| t.elapsed() >= limit))
            .map(|(k, _)| k.clone())
            .collect();
        for sid in expired {
            log(&format!("session {sid}: reattach period over, ended"));
            self.end_session(&mut st, &sid, "expired");
        }
    }

    fn end_session(&self, st: &mut State, sid: &str, why: &str) {
        let Some(mut s) = st.sessions.remove(sid) else {
            return;
        };
        // Recorded first: whoever sees the session end finds the reason in the audit already
        // (`SPEC-0324` behaviour 8; a narrowing's `rights-reduced` was written after the close).
        self.audit_for(s.peer.as_deref(), "session.end", None, why);
        // Terminals, runs and forwards of the session end with it; outside the lock, because
        // their cleanup may talk to the backend.
        let hooks = std::mem::take(&mut s.on_end);
        if !hooks.is_empty() {
            std::thread::spawn(move || hooks.into_iter().for_each(|h| h()));
        }
        if let Ok(mut inputs) = self.0.inputs.lock() {
            inputs.retain(|_, (owner, _)| owner != sid);
        }
        if let Ok(mut edits) = self.0.edit_audit.lock() {
            edits.retain(|(owner, _), _| owner != sid);
        }
        if let Some(link) = &s.link {
            link.credits.close();
        }
        st.by_client.remove(&s.client);
        if let Some(ws) = st.ws.as_mut() {
            ws.store.close_client(&s.client);
        }
        if let Some((path, stop)) = &s.socket {
            stop.store(true, Ordering::SeqCst);
            #[cfg(unix)]
            let _ = std::os::unix::net::UnixStream::connect(path);
            let _ = std::fs::remove_file(path);
        }
        if let Some(link) = &s.link {
            link.out.finish();
        }
        if st.sessions.is_empty() {
            self.0.idle.notify_all();
        }
    }

    /// Serves one connection until it ends: the first message is `session.hello` or
    /// `session.resume`; a session outlives its connection for the reattach period.
    pub fn serve(&self, transport: Transport) {
        self.serve_inner(transport, false, None);
    }

    /// Serves a connection of a paired device: its sessions have the device's rights and only
    /// that device can resume them (`SPEC-0321` behaviour 18).
    pub fn serve_as(&self, transport: Transport, peer: Peer) {
        self.serve_inner(transport, false, Some(peer));
    }

    fn serve_inner(&self, transport: Transport, resume_only: bool, peer: Option<Peer>) {
        let Transport {
            mut reader,
            writer,
            closer,
        } = transport;
        let out = Arc::new(Outbox::default());
        out.spawn_writer(writer, Arc::clone(&closer));
        let parts = ConnectionLink {
            peer,
            out: Arc::clone(&out),
            credits: Arc::new(Credits::default()),
            closer: Arc::clone(&closer),
        };
        let started = Arc::new(AtomicBool::new(false));
        {
            let (started, out, closer) = (Arc::clone(&started), Arc::clone(&out), closer.clone());
            let limit = self.0.config.hello_timeout;
            std::thread::spawn(move || {
                let t0 = Instant::now();
                while t0.elapsed() < limit {
                    if started.load(Ordering::SeqCst) || out.is_closed() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                if !started.load(Ordering::SeqCst) {
                    log(&format!(
                        "no session.hello within {} s; connection closed",
                        limit.as_secs()
                    ));
                    out.close();
                    closer();
                }
            });
        }
        let first = match read_frame(&mut reader) {
            Ok(Some(f)) => f,
            Ok(None) => {
                out.close();
                return;
            }
            Err(e) => {
                log(&format!("connection closed: {e}"));
                out.close();
                closer();
                return;
            }
        };
        let opened = match first.text().map(Message::parse) {
            Some(Ok(Message::Request { id, method, params })) => match method.as_str() {
                "session.hello" if !resume_only => self.hello(id, &params, &parts),
                "session.resume" => self.resume(id, &params, &parts),
                other => Err(format!("first message {other:?} is not hello or resume")),
            },
            _ => Err("the first frame is not a request".into()),
        };
        let (sid, generation) = match opened {
            Ok(x) => x,
            Err(why) => {
                // A refusal is sent before the connection closes; anything else gets no answer.
                log(&format!("connection closed: {why}"));
                out.finish();
                return;
            }
        };
        started.store(true, Ordering::SeqCst);
        let last = Arc::new(Mutex::new(Instant::now()));
        let ended = Arc::new(AtomicBool::new(false));
        {
            let (last, ended, me) = (Arc::clone(&last), Arc::clone(&ended), self.clone());
            let (sid, out, closer) = (sid.clone(), Arc::clone(&out), closer.clone());
            let limit = self.0.config.idle_timeout;
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(limit.min(Duration::from_millis(250)));
                    if ended.load(Ordering::SeqCst) || out.is_closed() {
                        return;
                    }
                    let quiet = last.lock().map(|t| t.elapsed()).unwrap_or_default();
                    if quiet >= limit {
                        log(&format!(
                            "session {sid}: no traffic for {} s, treated as a break",
                            limit.as_secs()
                        ));
                        me.detach(&sid, generation);
                        out.close();
                        closer();
                        return;
                    }
                }
            });
        }
        let inflight: Arc<Mutex<HashMap<String, CancelToken>>> = Arc::default();
        loop {
            let frame = match read_frame(&mut reader) {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(e) => {
                    log(&format!("session {sid}: connection closed: {e}"));
                    break;
                }
            };
            if let Ok(mut t) = last.lock() {
                *t = Instant::now();
            }
            match frame.kind {
                FrameKind::Message => {
                    let msg = match frame.text().map(Message::parse) {
                        Some(Ok(m)) => m,
                        _ => {
                            log(&format!(
                                "session {sid}: malformed message; connection closed"
                            ));
                            break;
                        }
                    };
                    let current = self.link_of(&sid, generation);
                    let Some(link) = current else {
                        break;
                    };
                    match msg {
                        Message::Request { id, method, params } => {
                            self.request(&sid, &link, id, &method, params, &inflight);
                            if method == "session.bye" {
                                ended.store(true, Ordering::SeqCst);
                                return;
                            }
                        }
                        Message::Notification { method, params } => match method.as_str() {
                            "$/cancel" => {
                                if let Some(t) = inflight
                                    .lock()
                                    .ok()
                                    .and_then(|m| m.get(&params["id"].to_string()).cloned())
                                {
                                    t.stop(Stop::Cancelled);
                                }
                            }
                            "session.ack" => self.ack(&sid, &params),
                            _ => {}
                        },
                        Message::Response { .. } => {}
                    }
                }
                FrameKind::Credit => {
                    if let (Some(n), Some(link)) =
                        (frame.credit_bytes(), self.link_of(&sid, generation))
                    {
                        link.credits.grant(frame.stream, n);
                    }
                }
                FrameKind::Data | FrameKind::Close => self.stream_in(&sid, generation, frame),
            }
        }
        ended.store(true, Ordering::SeqCst);
        for t in inflight
            .lock()
            .map(|m| m.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            t.stop(Stop::Cancelled);
        }
        self.detach(&sid, generation);
        out.close();
        closer();
    }

    /// Routes what the window wrote to one of the session's streams, and gives the credit back.
    fn stream_in(&self, sid: &str, generation: u64, frame: Frame) {
        let sink = self
            .0
            .inputs
            .lock()
            .ok()
            .and_then(|m| m.get(&frame.stream).cloned())
            .filter(|(owner, _)| owner == sid)
            .map(|(_, s)| s);
        let Some(sink) = sink else {
            log(&format!(
                "session {sid}: data on stream {} that takes no input; dropped",
                frame.stream
            ));
            return;
        };
        match frame.kind {
            FrameKind::Data => {
                let n = frame.payload.len() as u32;
                sink(StreamIn::Data(frame.payload));
                if let (Some(link), Ok(b)) = (
                    self.link_of(sid, generation),
                    Frame::credit(frame.stream, n).encode(),
                ) {
                    link.out.push(b, Priority::Interactive);
                }
            }
            _ => {
                let reason = serde_json::from_slice::<Value>(&frame.payload)
                    .ok()
                    .and_then(|v| v["reason"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                if let Ok(mut m) = self.0.inputs.lock() {
                    m.remove(&frame.stream);
                }
                sink(StreamIn::Closed(reason));
            }
        }
    }

    fn link_of(&self, sid: &str, generation: u64) -> Option<Link> {
        self.state()
            .sessions
            .get(sid)
            .and_then(|s| s.link.clone())
            .filter(|l| l.generation == generation)
    }

    fn detach(&self, sid: &str, generation: u64) {
        let mut st = self.state();
        if let Some(s) = st.sessions.get_mut(sid)
            && s.link.as_ref().is_some_and(|l| l.generation == generation)
        {
            if let Some(l) = s.link.take() {
                l.credits.close();
            }
            s.detached = Some(Instant::now());
            log(&format!(
                "session {sid}: connection lost; waiting {} s for reattachment",
                self.0.config.reattach.as_secs()
            ));
        }
    }

    fn ack(&self, sid: &str, params: &Value) {
        if let Some(seq) = params["seq"].as_u64()
            && let Some(s) = self.state().sessions.get_mut(sid)
        {
            s.events.ack(seq);
        }
    }

    fn hello(
        &self,
        id: Value,
        params: &Value,
        parts: &ConnectionLink,
    ) -> Result<(String, u64), String> {
        let refuse = |e: RpcError| {
            let link = Link {
                generation: 0,
                out: Arc::clone(&parts.out),
                credits: Arc::clone(&parts.credits),
                closer: Arc::clone(&parts.closer),
            };
            link.reply(id.clone(), Err(e.clone()));
            self.audit_for(
                parts.peer.as_ref().map(|p| p.name.as_str()),
                "session.hello",
                params["project"].as_str(),
                e.code.as_str(),
            );
            Err(e.to_string())
        };
        if params["protocol"] != PROTOCOL {
            return refuse(RpcError::new(
                ErrorCode::UnsupportedVersion,
                format!("protocol {} is not {PROTOCOL}", params["protocol"]),
            ));
        }
        let offered: Vec<u32> = params["majors"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.as_u64())
                    .map(|m| m as u32)
                    .collect()
            })
            .unwrap_or_default();
        let major = match choose_major(&offered, &supported_majors()) {
            Ok(m) => m,
            Err(e) => return refuse(e),
        };
        let Some(project) = params["project"].as_str() else {
            return refuse(RpcError::new(ErrorCode::Invalid, "`project` is missing"));
        };
        let root = match std::fs::canonicalize(project) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return refuse(RpcError::new(ErrorCode::Denied, format!("{project}: {e}")));
            }
            Err(e) => {
                return refuse(RpcError::new(
                    ErrorCode::NotFound,
                    format!("{project}: {e}"),
                ));
            }
        };
        if !root.is_dir() {
            return refuse(RpcError::new(
                ErrorCode::Invalid,
                format!("{project} is not a folder"),
            ));
        }
        if let Err(e) = std::fs::read_dir(&root) {
            return refuse(RpcError::new(ErrorCode::Denied, format!("{project}: {e}")));
        }
        // A device is bound to one project; it cannot name another one (`SPEC-0324` behaviour 2).
        if parts.peer.is_some() && self.0.config.folder.is_none() {
            return refuse(
                RpcError::new(
                    ErrorCode::Denied,
                    "a paired device needs a backend with a fixed folder",
                )
                .hint("start it as `idecraft serve --pair <folder>`"),
            );
        }
        if let Some(fixed) = &self.0.config.folder
            && std::fs::canonicalize(fixed).ok().as_deref() != Some(root.as_path())
        {
            return refuse(RpcError::new(
                ErrorCode::Invalid,
                format!("this backend serves {}, not {project}", fixed.display()),
            ));
        }
        let mut st = self.state();
        if let Some(ws) = &st.ws
            && ws.root != root
        {
            let other = ws.root.display().to_string();
            drop(st);
            return refuse(RpcError::new(
                ErrorCode::Invalid,
                format!("this backend already serves {other}"),
            ));
        }
        if st.ws.is_none() {
            let mut store = DocStore::new(root.clone(), self.0.config.registry.clone());
            if let Some((sink, blobs)) = self.0.room.get().and_then(|r| r.workspace(&root)) {
                store.enter_room(sink, blobs);
            }
            st.ws = Some(Workspace {
                root: root.clone(),
                store,
                _watch: self.start_watch(&root),
            });
        }
        let sid = random_id();
        st.next_client += 1;
        st.generation += 1;
        let client = format!("c{}", st.next_client);
        let token = Token::random();
        let link = Link {
            generation: st.generation,
            out: Arc::clone(&parts.out),
            credits: Arc::clone(&parts.credits),
            closer: Arc::clone(&parts.closer),
        };
        let socket = self.listen(&sid);
        let rights = parts
            .peer
            .as_ref()
            .map_or_else(|| self.0.config.rights.clone(), |p| p.rights.clone());
        let peer_name = parts.peer.as_ref().map(|p| p.name.clone());
        let result = json!({
            "protocol": PROTOCOL,
            "major": major,
            "minor": MINOR,
            "build": self.0.config.build,
            "session": sid,
            "token": token.expose(),
            "client": client,
            "rights": rights.iter().map(|r| r.name()).collect::<Vec<_>>(),
            "capabilities": self.capabilities(),
            "root": root.display().to_string(),
            "name": root.file_name().map(|n| n.to_string_lossy().into_owned()),
            "platform": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            "reattach_seconds": self.0.config.reattach.as_secs(),
            "socket": socket.is_some(),
        });
        let (ring_entries, ring_bytes) = self.0.config.ring;
        st.sessions.insert(
            sid.clone(),
            Session {
                token,
                client: client.clone(),
                rights,
                peer: peer_name.clone(),
                events: EventLog::with_limits(ring_entries, ring_bytes),
                link: Some(link.clone()),
                detached: None,
                wrong_tokens: 0,
                watch: None,
                socket,
                on_end: Vec::new(),
            },
        );
        st.by_client.insert(client, sid.clone());
        st.ever = true;
        link.reply(id, Ok(result));
        drop(st);
        self.audit_for(peer_name.as_deref(), "session.hello", Some(project), "ok");
        Ok((sid, link.generation))
    }

    fn resume(
        &self,
        id: Value,
        params: &Value,
        parts: &ConnectionLink,
    ) -> Result<(String, u64), String> {
        let temp = Link {
            generation: 0,
            out: Arc::clone(&parts.out),
            credits: Arc::clone(&parts.credits),
            closer: Arc::clone(&parts.closer),
        };
        let sid = params["session"].as_str().unwrap_or_default().to_owned();
        let token = params["token"].as_str().and_then(Token::parse);
        let last_seq = params["last_seq"].as_u64().unwrap_or(0);
        let peer_name = parts.peer.as_ref().map(|p| p.name.clone());
        let mut st = self.state();
        let Some(s) = st.sessions.get_mut(&sid) else {
            drop(st);
            let e = RpcError::new(ErrorCode::NotFound, "the session ended or is unknown")
                .hint("start a new backend; unsaved text stays in the window");
            temp.reply(id, Err(e));
            self.audit("session.resume", None, "not_found");
            return Err("resume of an unknown session".into());
        };
        // Another device cannot take a session over, even with its token.
        if s.peer != peer_name {
            drop(st);
            temp.reply(
                id,
                Err(RpcError::new(
                    ErrorCode::Denied,
                    "this session belongs to another device",
                )),
            );
            self.audit_for(peer_name.as_deref(), "session.resume", None, "denied");
            return Err("resume by another device".into());
        }
        if !token.as_ref().is_some_and(|t| s.token.matches(t)) {
            s.wrong_tokens += 1;
            let ended = s.wrong_tokens >= 3;
            if ended {
                self.end_session(&mut st, &sid, "three wrong resume tokens");
            }
            drop(st);
            let e = RpcError::new(ErrorCode::Denied, "wrong resume token").hint(if ended {
                "the session was ended after three wrong tokens"
            } else {
                "only the window that opened the session can resume it"
            });
            temp.reply(id, Err(e));
            self.audit("session.resume", None, "denied");
            return Err("wrong resume token".into());
        }
        st.generation += 1;
        let generation = st.generation;
        let Some(s) = st.sessions.get_mut(&sid) else {
            return Err("session gone".into());
        };
        if let Some(old) = s.link.take() {
            old.credits.close();
            old.drop_now();
        }
        let link = Link { generation, ..temp };
        s.link = Some(link.clone());
        s.detached = None;
        s.wrong_tokens = 0;
        let replay = s.events.since(last_seq);
        let resync = replay.is_err();
        link.reply(
            id,
            Ok(json!({
                "resumed": true, "client": s.client, "last_seq": s.events.last(), "resync": resync,
            })),
        );
        match replay {
            Ok(texts) => {
                for t in texts {
                    link.send(t, Priority::Interactive);
                }
            }
            Err(_) => st.notify(&sid, "session.resync", json!({}), Priority::Interactive),
        }
        drop(st);
        self.audit_for(peer_name.as_deref(), "session.resume", None, "ok");
        Ok((sid, generation))
    }

    #[cfg(unix)]
    fn listen(&self, sid: &str) -> Option<(PathBuf, Arc<AtomicBool>)> {
        use std::os::unix::fs::PermissionsExt;
        let dir = self.0.config.run_dir.as_ref()?;
        let dir = match crate::reattach::private_dir(dir) {
            Ok(d) => d,
            Err(e) => {
                log(&format!("no reattach socket: {e}"));
                return None;
            }
        };
        let path = crate::reattach::socket_path(&dir, sid);
        let _ = std::fs::remove_file(&path);
        let listener = match std::os::unix::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log(&format!("no reattach socket {}: {e}", path.display()));
                return None;
            }
        };
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, me) = (Arc::clone(&stop), self.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else {
                    continue;
                };
                let own = crate::reattach::own_uid();
                let peer = crate::reattach::peer_uid(&stream).unwrap_or(u32::MAX);
                if let Err(e) = crate::reattach::check_peer(peer, own) {
                    log(&format!("reattach socket: {e}"));
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    continue;
                }
                let Ok(reader) = stream.try_clone() else {
                    continue;
                };
                let Ok(closing) = stream.try_clone() else {
                    continue;
                };
                let me = me.clone();
                std::thread::spawn(move || {
                    me.serve_inner(
                        Transport {
                            reader: Box::new(reader),
                            writer: Box::new(stream),
                            closer: Arc::new(move || {
                                let _ = closing.shutdown(std::net::Shutdown::Both);
                            }),
                        },
                        true,
                        None,
                    );
                });
            }
        });
        Some((path, stop))
    }

    #[cfg(not(unix))]
    fn listen(&self, _sid: &str) -> Option<(PathBuf, Arc<AtomicBool>)> {
        None
    }

    /// The reattach socket of session `id`, if it has one.
    pub fn socket_of(&self, id: &str) -> Option<PathBuf> {
        self.state()
            .sessions
            .get(id)
            .and_then(|s| s.socket.as_ref().map(|(p, _)| p.clone()))
    }

    /// Calls `listener` with every batch of changes the workspace watcher finds, on the watcher's
    /// thread, before the sessions hear of them (`SPEC-0042` v11 behaviour 20: the host's language
    /// servers learn of files changed on disk without a second watcher).
    pub fn on_disk_changes(&self, listener: DiskListener) {
        if let Ok(mut listeners) = self.0.disk_listeners.lock() {
            listeners.push(listener);
        }
    }

    fn start_watch(&self, root: &Path) -> Option<idecraft_core::WatchHandle> {
        let ws = idecraft_core::Workspace::open(root).ok()?;
        let (handle, rx) = idecraft_core::watch(ws, self.0.config.watch_interval).ok()?;
        let weak = Arc::downgrade(&self.0);
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            while let Ok(changes) = rx.recv() {
                let Some(shared) = weak.upgrade() else {
                    break;
                };
                if let Ok(listeners) = shared.disk_listeners.lock() {
                    for listener in listeners.iter() {
                        listener(&root, &changes);
                    }
                }
                Backend(shared).file_changes(&root, &changes);
            }
        });
        Some(handle)
    }

    fn file_changes(&self, root: &Path, changes: &[idecraft_core::FileChange]) {
        use idecraft_core::FileChange as C;
        let rel = |p: &Path| crate::paths::relative(root, &root.join(p));
        let mut events = Vec::new();
        let mut touched = Vec::new();
        for c in changes {
            let (kind, path, from) = match c {
                C::Created(p) => ("created", rel(p), None),
                C::Modified(p) => ("changed", rel(p), None),
                C::Deleted(p) => ("deleted", rel(p), None),
                C::Renamed { from, to } => ("renamed", rel(to), rel(from)),
            };
            let Some(path) = path else {
                continue;
            };
            touched.push(path.clone());
            if let Some(f) = &from {
                touched.push(f.clone());
            }
            let mut e = json!({ "kind": kind, "path": path });
            if let Some(f) = from {
                e["from"] = json!(f);
            }
            events.push(e);
        }
        let mut st = self.state();
        let mut disk = Vec::new();
        if let Some(ws) = st.ws.as_mut() {
            if ws.store.in_room() {
                for e in &events {
                    let path = e["path"].as_str().unwrap_or_default();
                    if ws.store.id_of(path).is_some() && e["kind"] == "changed" {
                        continue;
                    }
                    let kind = match e["kind"].as_str() {
                        Some("created") => "created",
                        Some("deleted") => "deleted",
                        Some("renamed") => "renamed",
                        _ => "changed",
                    };
                    ws.store.note_external(kind, path, e["from"].as_str());
                }
            }
            for path in &touched {
                if let Some(doc) = ws.store.id_of(path)
                    && let Some(ev) = ws.store.disk_check(&doc)
                {
                    disk.push((ws.store.clients(&doc), ev));
                }
            }
        }
        for (clients, ev) in disk {
            for c in clients {
                match &ev {
                    DiskEvent::Reloaded {
                        doc,
                        revision,
                        ops,
                        hash,
                    } => st.notify_client(
                        &c,
                        "doc.changed",
                        json!({"doc": doc, "revision": revision, "ops": ops, "author": "disk", "hash": hash}),
                    ),
                    DiskEvent::Changed { doc, deleted } => st.notify_client(
                        &c,
                        "doc.disk_changed",
                        json!({"doc": doc, "deleted": deleted}),
                    ),
                }
            }
        }
        let watchers: Vec<(String, Vec<String>)> = st
            .sessions
            .iter()
            .filter_map(|(k, s)| s.watch.clone().map(|ex| (k.clone(), ex)))
            .collect();
        for (sid, exclude) in watchers {
            let mine: Vec<Value> = events
                .iter()
                .filter(|e| {
                    let p = e["path"].as_str().unwrap_or_default();
                    !exclude
                        .iter()
                        .any(|x| p == x || p.starts_with(&format!("{x}/")))
                })
                .cloned()
                .collect();
            if !mine.is_empty() {
                st.notify(
                    &sid,
                    "fs.changed",
                    json!({ "events": mine }),
                    Priority::Bulk,
                );
            }
        }
    }

    fn stream_bytes(&self, link: &Link, bytes: Vec<u8>) -> u32 {
        let id = self.0.next_stream.fetch_add(2, Ordering::SeqCst);
        link.credits.open(id);
        let link = link.clone();
        std::thread::spawn(move || {
            let mut sent = 0;
            while sent < bytes.len() {
                let want = (bytes.len() - sent).min(64 << 10);
                let n = link.credits.take(id, want, Duration::from_secs(45));
                if n == 0 {
                    if let Ok(b) = Frame::close(id, "stalled").encode() {
                        link.out.push(b, Priority::Bulk);
                    }
                    link.credits.remove(id);
                    return;
                }
                if let Ok(b) = Frame::data(id, bytes[sent..sent + n].to_vec()).encode() {
                    link.out.push(b, Priority::Bulk);
                }
                sent += n;
            }
            if let Ok(b) = Frame::close(id, "done").encode() {
                link.out.push(b, Priority::Bulk);
            }
            link.credits.remove(id);
        });
        id
    }

    fn request(
        &self,
        sid: &str,
        link: &Link,
        id: Value,
        method: &str,
        params: Value,
        inflight: &Arc<Mutex<HashMap<String, CancelToken>>>,
    ) {
        let registered = self.0.handlers.lock().ok().and_then(|h| {
            h.get(method)
                .map(|r| (r.right, Arc::clone(&r.handler), r.audit))
        });
        let need = right_for(method).or(registered.as_ref().map(|r| r.0));
        if let Some(room) = self.0.room.get()
            && let Some(handle) = self.handle_of(sid)
        {
            let peer = self.session_peer(sid);
            if let Err(e) = room.gate(self, &handle, &peer, method) {
                self.audit_session(sid, method, params["path"].as_str(), "not_granted");
                link.reply(id, Err(e));
                return;
            }
        }
        let (client, rights) = match self.state().sessions.get(sid) {
            Some(s) => (s.client.clone(), s.rights.clone()),
            None => return,
        };
        if let Some(right) = need
            && !rights.contains(&right)
        {
            self.audit_session(sid, method, params["path"].as_str(), "not_granted");
            link.reply(
                id,
                Err(RpcError::new(
                    ErrorCode::NotGranted,
                    format!("not granted: {method} needs the right `{}`", right.name()),
                )
                .hint("this session was opened without that right")),
            );
            return;
        }
        let path_of = |p: &Value| {
            p["path"]
                .as_str()
                .or(p["doc"].as_str())
                .or(p["from"].as_str())
                .map(str::to_owned)
        };
        let result = match method {
            "session.ping" => Ok(json!({ "time_ms": now_ms() })),
            "session.ack" => {
                self.ack(sid, &params);
                Ok(json!({}))
            }
            "session.bye" => {
                let mut st = self.state();
                let unsaved = st
                    .ws
                    .as_ref()
                    .map(|w| w.store.unsaved(&client))
                    .unwrap_or_default();
                link.reply(id, Ok(json!({ "unsaved": unsaved })));
                self.end_session(&mut st, sid, "bye");
                drop(st);
                self.audit("session.bye", None, "ok");
                return;
            }
            "session.hello" | "session.resume" => Err(RpcError::new(
                ErrorCode::Invalid,
                "this connection already has a session",
            )),
            "doc.open" => self.doc_open(link, &client, &params),
            "doc.edit" => {
                // Answer and notifications go out under the lock, so every client sees one order.
                self.doc_edit(sid, link, id, &client, &params);
                if self.device_logs(sid, method, &params) {
                    self.audit_session(sid, method, path_of(&params).as_deref(), "ok");
                }
                return;
            }
            "doc.save" => self.doc_save(&client, &params),
            "doc.snapshot" => self.with_store(|s| {
                s.snapshot(params["doc"].as_str().unwrap_or_default())
                    .map(|d| d.to_json())
            }),
            "doc.changes" => self.with_store(|s| {
                s.changes(
                    params["doc"].as_str().unwrap_or_default(),
                    params["since"].as_u64().unwrap_or(0),
                )
            }),
            "doc.close" => self.with_store(|s| {
                s.close(params["doc"].as_str().unwrap_or_default(), &client);
                Ok(json!({}))
            }),
            "fs.list" => self.with_root(|r| crate::fsops::list(r, &params)),
            "fs.stat" => self.with_root(|r| crate::fsops::stat(r, &params)),
            "fs.read" => {
                self.with_root(|r| crate::fsops::read(r, &params))
                    .map(|(mut v, bytes)| {
                        if let Some(b) = bytes {
                            v["stream"] = json!(self.stream_bytes(link, b));
                        }
                        v
                    })
            }
            "fs.write" | "fs.create" | "fs.rename" | "fs.delete" if self.in_room() => {
                self.room_fs(&client, method, &params)
            }
            "fs.write" => self.with_root(|r| crate::fsops::write(r, &params)),
            "fs.create" => self.with_root(|r| crate::fsops::create(r, &params)),
            "fs.rename" => self.with_root(|r| crate::fsops::rename(r, &params)),
            "fs.delete" => self.with_root(|r| crate::fsops::delete(r, &params)),
            "fs.watch" => {
                let exclude: Vec<String> = params["exclude"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let bad = exclude
                    .iter()
                    .find_map(|x| crate::paths::check_relative(x).err());
                match bad {
                    Some(e) => Err(e),
                    None => {
                        if let Some(s) = self.state().sessions.get_mut(sid) {
                            s.watch = Some(exclude);
                        }
                        Ok(
                            json!({ "watching": true, "coalesce_ms": self.0.config.watch_interval.as_millis() as u64 }),
                        )
                    }
                }
            }
            "event.subscribe" => Ok(json!({ "kinds": [] })),
            "device.whoami" => Ok(self.whoami(sid, &rights)),
            "notice.list" => Ok(self.notice_list(params["since"].as_u64().unwrap_or(0))),
            _ => match registered {
                Some((_, handler, audit)) => {
                    let device = self.is_device_session(sid);
                    let audit = (audit || (device && audited_for_device(method)))
                        .then(|| method.to_owned());
                    self.spawn_handler(
                        sid,
                        link,
                        id,
                        (&client, &rights),
                        params,
                        (handler, audit),
                        inflight,
                    );
                    return;
                }
                None if is_ui_method(method) => Err(RpcError::new(
                    ErrorCode::Denied,
                    format!("denied: {method}: a host never operates the window"),
                )
                .hint("the `ui` family is served only by the process that owns the window (SPEC-0319 behaviour 31)")),
                None => Err(RpcError::method_not_found(method)),
            },
        };
        if audited(method) || self.device_logs(sid, method, &params) {
            let outcome = match &result {
                Ok(_) => "ok",
                Err(e) => e.code.as_str(),
            };
            self.audit_session(sid, method, path_of(&params).as_deref(), outcome);
        }
        link.reply(id, result);
    }

    fn is_device_session(&self, sid: &str) -> bool {
        self.state()
            .sessions
            .get(sid)
            .is_some_and(|s| s.peer.is_some())
    }

    /// Whether this request of a device goes to the log; `doc.edit` once per 10 s and document
    /// (`SPEC-0324` behaviour 10).
    fn device_logs(&self, sid: &str, method: &str, params: &Value) -> bool {
        if !self.is_device_session(sid) || !audited_for_device(method) {
            return false;
        }
        if method != "doc.edit" {
            return true;
        }
        let Ok(mut map) = self.0.edit_audit.lock() else {
            return true;
        };
        let key = (
            sid.to_owned(),
            params["doc"].as_str().unwrap_or_default().to_owned(),
        );
        let due = map
            .get(&key)
            .is_none_or(|t| t.elapsed() >= EDIT_AUDIT_EVERY);
        if due {
            map.insert(key, Instant::now());
        }
        due
    }

    fn whoami(&self, sid: &str, rights: &[Right]) -> Value {
        let peer = self.session_peer(sid);
        let names: Vec<&str> = rights.iter().map(|r| r.name()).collect();
        let parts: Vec<&str> = peer.splitn(3, ':').collect();
        match parts.as_slice() {
            ["device", fp, name] => json!({"name": name, "fingerprint": fp, "rights": names}),
            _ => json!({"name": peer, "fingerprint": null, "rights": names}),
        }
    }

    fn notice_list(&self, since: u64) -> Value {
        let notices: Vec<Value> = self
            .0
            .notices
            .lock()
            .map(|n| {
                n.iter()
                    .filter(|v| v["seq"].as_u64().unwrap_or(0) > since)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        json!({ "notices": notices })
    }

    fn with_store<T>(
        &self,
        f: impl FnOnce(&mut DocStore) -> Result<T, RpcError>,
    ) -> Result<T, RpcError> {
        let mut st = self.state();
        match st.ws.as_mut() {
            Some(ws) => f(&mut ws.store),
            None => Err(RpcError::new(ErrorCode::Unavailable, "no workspace")),
        }
    }

    fn with_root<T>(&self, f: impl FnOnce(&Path) -> Result<T, RpcError>) -> Result<T, RpcError> {
        let root = self.state().ws.as_ref().map(|w| w.root.clone());
        match root {
            Some(r) => f(&r),
            None => Err(RpcError::new(ErrorCode::Unavailable, "no workspace")),
        }
    }

    fn doc_open(&self, link: &Link, client: &str, params: &Value) -> Result<Value, RpcError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| RpcError::new(ErrorCode::Invalid, "`path` is missing"))?;
        let info = self.with_store(|s| s.open(path, client))?;
        let mut v = info.to_json();
        if info.text.len() > crate::fsops::INLINE_LIMIT {
            v["text"] = Value::Null;
            v["bytes"] = json!(info.text.len());
            v["stream"] = json!(self.stream_bytes(link, info.text.into_bytes()));
        }
        Ok(v)
    }

    fn doc_edit(&self, sid: &str, link: &Link, id: Value, client: &str, params: &Value) {
        let doc = params["doc"].as_str().unwrap_or_default().to_owned();
        let base = params["base"].as_u64().unwrap_or(0);
        let cseq = params["cseq"].as_u64().unwrap_or(0);
        let ops: Result<Vec<Op>, _> = serde_json::from_value(params["ops"].clone());
        let Ok(ops) = ops else {
            link.reply(
                id,
                Err(RpcError::new(
                    ErrorCode::Invalid,
                    "`ops` are not edit ranges",
                )),
            );
            return;
        };
        let mut st = self.state();
        let doc = st
            .ws
            .as_ref()
            .map_or_else(|| doc.clone(), |w| w.store.canonical(&doc));
        let outcome = match st.ws.as_mut() {
            Some(ws) => ws
                .store
                .edit(&doc, base, client, cseq, &ops)
                .map(|o| (o, ws.store.clients(&doc))),
            None => Err(RpcError::new(ErrorCode::Unavailable, "no workspace")),
        };
        match outcome {
            Ok((o, clients)) => {
                if !o.duplicate {
                    if o.changed {
                        for c in clients.iter().filter(|c| c.as_str() != client) {
                            st.notify_client(
                                c,
                                "doc.changed",
                                json!({"doc": doc, "revision": o.revision, "ops": o.ops, "author": client, "hash": o.hash}),
                            );
                        }
                    }
                    st.notify(
                        sid,
                        "doc.acked",
                        json!({"doc": doc, "cseq": cseq, "revision": o.revision, "hash": o.hash}),
                        Priority::Interactive,
                    );
                }
                link.reply(
                    id,
                    Ok(json!({"revision": o.revision, "hash": o.hash, "duplicate": o.duplicate})),
                );
            }
            Err(e) => link.reply(id, Err(e)),
        }
    }

    fn doc_save(&self, client: &str, params: &Value) -> Result<Value, RpcError> {
        let doc = params["doc"].as_str().unwrap_or_default().to_owned();
        let revision = params["revision"].as_u64().unwrap_or(0);
        let overwrite = params["overwrite"].as_bool() == Some(true);
        let mut st = self.state();
        let ws = st
            .ws
            .as_mut()
            .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no workspace"))?;
        let saved = ws.store.save(&doc, revision, overwrite)?;
        let others: Vec<String> = ws
            .store
            .clients(&doc)
            .into_iter()
            .filter(|c| c != client)
            .collect();
        for c in others {
            st.notify_client(&c, "doc.saved", json!({"doc": doc, "revision": saved}));
        }
        Ok(json!({ "doc": doc, "revision": saved, "saved": true }))
    }

    fn handle_of(&self, sid: &str) -> Option<SessionHandle> {
        let client = self.state().sessions.get(sid)?.client.clone();
        Some(SessionHandle {
            shared: Arc::downgrade(&self.0),
            sid: sid.to_owned(),
            client,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_handler(
        &self,
        sid: &str,
        link: &Link,
        id: Value,
        (client, rights): (&str, &[Right]),
        params: Value,
        (handler, audit): (Handler, Option<String>),
        inflight: &Arc<Mutex<HashMap<String, CancelToken>>>,
    ) {
        let token = CancelToken::default();
        let key = id.to_string();
        if let Ok(mut m) = inflight.lock() {
            m.insert(key.clone(), token.clone());
        }
        if let Some(ms) = params["deadline_ms"].as_u64() {
            let t = token.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                t.stop(Stop::Deadline);
            });
        }
        let root = self
            .state()
            .ws
            .as_ref()
            .map(|w| w.root.clone())
            .unwrap_or_default();
        let call = Call {
            params,
            cancel: token.clone(),
            client: client.to_owned(),
            root,
            session: SessionHandle {
                shared: Arc::downgrade(&self.0),
                sid: sid.to_owned(),
                client: client.to_owned(),
            },
            rights: rights.to_vec(),
        };
        let (link, inflight, me) = (link.clone(), Arc::clone(inflight), self.clone());
        let sid = sid.to_owned();
        std::thread::spawn(move || {
            let result = handler(&call);
            if let Some(method) = audit {
                let target = [
                    "path", "doc", "name", "term", "run", "forward", "tool", "adapter", "session",
                ]
                .iter()
                .find_map(|k| match &call.params[*k] {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                });
                let outcome = match &result {
                    Ok(_) => "ok",
                    Err(e) => e.code.as_str(),
                };
                me.audit_session(&sid, &method, target.as_deref(), outcome);
            }
            let result = match token.stopped() {
                Some(Stop::Cancelled) => Err(RpcError::new(ErrorCode::Cancelled, "cancelled")),
                Some(Stop::Deadline) => Err(RpcError::new(
                    ErrorCode::DeadlineExceeded,
                    "the request's deadline passed",
                )),
                None => result,
            };
            if let Ok(mut m) = inflight.lock() {
                m.remove(&key);
            }
            link.reply(id, result);
        });
    }
}

/// The session a handler serves: notifications to it, streams on its connection, and cleanup when
/// it ends (`SPEC-0318`: terminals, runs and forwards live as long as their session).
#[derive(Clone)]
pub struct SessionHandle {
    shared: Weak<Shared>,
    sid: String,
    client: String,
}

impl SessionHandle {
    fn backend(&self) -> Option<Backend> {
        self.shared.upgrade().map(Backend)
    }

    /// The session id.
    pub fn id(&self) -> &str {
        &self.sid
    }

    /// The session's client id.
    pub fn client(&self) -> &str {
        &self.client
    }

    /// The backend serving the session, while it runs.
    pub fn backend_now(&self) -> Option<Backend> {
        self.backend()
    }

    /// Whether the session still exists (connected or waiting for reattachment).
    pub fn alive(&self) -> bool {
        self.backend()
            .is_some_and(|b| b.state().sessions.contains_key(&self.sid))
    }

    /// Whether the session has a connection now.
    pub fn connected(&self) -> bool {
        self.backend()
            .and_then(|b| b.connected(&self.sid))
            .unwrap_or(false)
    }

    /// Sends a numbered notification to the session (kept for replay after a break).
    pub fn notify(&self, method: &str, params: Value) {
        if let Some(b) = self.backend() {
            b.state()
                .notify(&self.sid, method, params, Priority::Interactive);
        }
    }

    /// [`SessionHandle::notify`] behind interactive traffic (diagnostics, progress).
    pub fn notify_bulk(&self, method: &str, params: Value) {
        if let Some(b) = self.backend() {
            b.state().notify(&self.sid, method, params, Priority::Bulk);
        }
    }

    /// Posts a notice to every session of the backend (`SPEC-0324` behaviour 13).
    pub fn post_notice(&self, kind: &str, title: &str, detail: Option<&str>) {
        if let Some(b) = self.backend() {
            b.post_notice(kind, title, detail);
        }
    }

    /// A new stream on the session's current connection; `None` while it is disconnected.
    pub fn open_stream(&self) -> Option<StreamOut> {
        let b = self.backend()?;
        let link = b.state().sessions.get(&self.sid)?.link.clone()?;
        let id = b.0.next_stream.fetch_add(2, Ordering::SeqCst);
        link.credits.open(id);
        Some(StreamOut { id, link })
    }

    /// Routes what the window writes to `stream` to `sink` (terminal input, a forwarded
    /// connection's bytes). Data on a stream without a sink is dropped.
    pub fn on_input(&self, stream: u32, sink: StreamSink) {
        if let Some(b) = self.backend()
            && let Ok(mut m) = b.0.inputs.lock()
        {
            m.insert(stream, (self.sid.clone(), sink));
        }
    }

    /// Stops routing input of `stream`.
    pub fn drop_input(&self, stream: u32) {
        if let Some(b) = self.backend()
            && let Ok(mut m) = b.0.inputs.lock()
        {
            m.remove(&stream);
        }
    }

    /// Runs `hook` when the session ends (bye, reattach period over); at once when it has
    /// ended already.
    pub fn on_end(&self, hook: Box<dyn FnOnce() + Send>) {
        let Some(b) = self.backend() else {
            hook();
            return;
        };
        let mut st = b.state();
        match st.sessions.get_mut(&self.sid) {
            Some(s) => s.on_end.push(hook),
            None => {
                drop(st);
                hook();
            }
        }
    }

    /// Writes one line to the audit log.
    pub fn audit(&self, method: &str, target: Option<&str>, outcome: &str) {
        if let Some(b) = self.backend() {
            b.audit_session(self.id(), method, target, outcome);
        }
    }
}

/// A stream from the backend to the window on one connection (`ADR-0026` §9): data waits for the
/// window's credit, so a window that stops reading stops the producer instead of filling memory.
pub struct StreamOut {
    id: u32,
    link: Link,
}

impl StreamOut {
    /// The stream id.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Whether the connection carrying it is still open.
    pub fn is_open(&self) -> bool {
        !self.link.out.is_closed()
    }

    /// Sends `bytes`, waiting for credit; `false` when the connection is gone or the window gave
    /// no credit for 45 s (the stream is then dead; the producer keeps its output for `attach`).
    pub fn write(&self, bytes: &[u8]) -> bool {
        let mut sent = 0;
        while sent < bytes.len() {
            if self.link.out.is_closed() {
                return false;
            }
            let want = (bytes.len() - sent).min(64 << 10);
            let n = self
                .link
                .credits
                .take(self.id, want, Duration::from_secs(45));
            if n == 0 {
                return false;
            }
            match Frame::data(self.id, bytes[sent..sent + n].to_vec()).encode() {
                Ok(b) => {
                    if !self.link.out.push(b, Priority::Bulk) {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            sent += n;
        }
        true
    }

    /// Ends the stream with `reason` (`done` for a normal end).
    pub fn close(&self, reason: &str) {
        if let Ok(b) = Frame::close(self.id, reason).encode() {
            self.link.out.push(b, Priority::Bulk);
        }
        self.link.credits.remove(self.id);
    }
}
