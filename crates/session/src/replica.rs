//! The window's half of a remote project (`SPEC-0317` behaviours 8–15, `ADR-0026` §5 and §8):
//! documents applied optimistically with a pending queue (one edit in flight, later typing
//! composed into a buffer), the client side of the transformation, hash checks per revision,
//! recovery copies of unsaved text, and resuming or replacing a lost backend without dropping a
//! character.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use ropey::Rope;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::client::{Connection, Inbound};
use crate::hello::{MAJOR, PROTOCOL, Token};
use crate::ot::{Op, TextOp, text_hash};
use crate::paths::check_relative;
use crate::rpc::{ErrorCode, RpcError};
use crate::transport::Transport;

/// Time limit of a request from the window.
pub const CALL_LIMIT: Duration = Duration::from_secs(30);

/// Notification prefixes of the families bound on the host (`SPEC-0318`); the window drains them
/// from [`ClientSession::host_events`].
pub const HOST_FAMILIES: [&str; 8] = [
    "lsp.", "git.", "term.", "run.", "port.", "dap.", "search.", "room.",
];

/// Host notifications kept until the window drains them.
pub const HOST_EVENTS_KEPT: usize = 10_000;

/// A new value of [`RemoteDoc::stamp`], unique in this process.
fn next_stamp() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Which numbered notifications were handled (`SPEC-0317` v5 behaviour 12a). Bulk notifications
/// travel behind interactive ones, so a higher `seq` can arrive before a lower one: what counts
/// for a resume and for acknowledgements is the end of the part without a gap, and a replay of
/// what already arrived is ignored.
#[derive(Debug, Default)]
struct Received {
    upto: u64,
    ahead: BTreeSet<u64>,
}

impl Received {
    /// Notes `seq`; `false` when it was handled before.
    fn first_time(&mut self, seq: u64) -> bool {
        if seq <= self.upto || !self.ahead.insert(seq) {
            return false;
        }
        while self.ahead.remove(&(self.upto + 1)) {
            self.upto += 1;
        }
        // A gap that never fills (it cannot on a working connection) must not grow without end;
        // the ring buffer does not reach further back anyway.
        if self.ahead.len() > crate::events::RING_ENTRIES
            && let Some(last) = self.ahead.iter().next_back().copied()
        {
            self.restart(last);
        }
        true
    }

    /// Everything up to `seq` counts as handled (after a resync).
    fn restart(&mut self, seq: u64) {
        self.upto = seq;
        self.ahead.clear();
    }
}

#[derive(Debug, Clone)]
struct Outstanding {
    cseq: u64,
    base: u64,
    /// As sent (a resend repeats it unchanged).
    sent_op: TextOp,
    /// Transformed against changes that arrived meanwhile (for the window's own bookkeeping).
    op: TextOp,
    sent: bool,
}

/// One remote document in the window.
#[derive(Debug, Clone)]
pub struct RemoteDoc {
    /// Id: path relative to the project root.
    pub doc: String,
    /// The last revision of the backend the window knows.
    pub server_rev: u64,
    /// Changes whenever the text changes by anything but the user's own typing; unique in this
    /// process, so it also tells apart the documents of an old and a new backend, whose revisions
    /// may be equal.
    pub stamp: u64,
    server_text: Rope,
    text: Rope,
    outstanding: Option<Outstanding>,
    buffer: Option<TextOp>,
    /// Unsaved changes (in the window or on the backend).
    pub modified: bool,
    /// Hash of the host file when it was loaded or last saved.
    pub saved_hash: String,
    /// The window's own text kept after a divergence or a changed host file: never discarded,
    /// the user compares.
    pub local_version: Option<String>,
    /// The host file changed while the document had unsaved changes.
    pub disk_changed: bool,
    /// A save asked for while disconnected, carried out after the resume.
    pub save_requested: bool,
}

/// The window's text no longer matches the backend's at a revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diverged(pub String);

impl RemoteDoc {
    /// A document as `doc.open` returned it.
    pub fn new(doc: &str, revision: u64, text: &str, modified: bool) -> Self {
        let rope = Rope::from_str(text);
        Self {
            doc: doc.to_owned(),
            server_rev: revision,
            stamp: next_stamp(),
            server_text: rope.clone(),
            text: rope,
            outstanding: None,
            buffer: None,
            modified,
            saved_hash: text_hash(&Rope::from_str(text)),
            local_version: None,
            disk_changed: false,
            save_requested: false,
        }
    }

    /// The text the window shows.
    pub fn text(&self) -> String {
        self.text.to_string()
    }

    /// Edits not acknowledged yet (0, 1 or 2: the one in flight and the buffer).
    pub fn pending(&self) -> usize {
        usize::from(self.outstanding.is_some()) + usize::from(self.buffer.is_some())
    }

    /// The pending edits in protocol form (for recovery copies).
    pub fn pending_ops(&self) -> Vec<Vec<Op>> {
        self.outstanding
            .iter()
            .map(|o| o.op.to_ops())
            .chain(self.buffer.iter().map(TextOp::to_ops))
            .collect()
    }

    /// Applies the user's edit at once and queues it.
    pub fn local_edit(&mut self, op: TextOp) -> Result<(), RpcError> {
        if op.is_noop() {
            return Ok(());
        }
        op.apply(&mut self.text)?;
        self.modified = true;
        match (&mut self.outstanding, &mut self.buffer) {
            (None, _) => {
                self.outstanding = Some(Outstanding {
                    cseq: 0,
                    base: self.server_rev,
                    sent_op: op.clone(),
                    op,
                    sent: false,
                });
            }
            (Some(_), Some(b)) => *b = b.compose(&op)?,
            (Some(_), None) => self.buffer = Some(op),
        }
        Ok(())
    }

    /// The `doc.edit` parameters of the edit to send now, if any; numbers it from `next_cseq`.
    pub fn take_send(&mut self, next_cseq: &mut u64) -> Option<Value> {
        let o = self.outstanding.as_mut().filter(|o| !o.sent)?;
        if o.cseq == 0 {
            *next_cseq += 1;
            o.cseq = *next_cseq;
        }
        o.sent = true;
        Some(json!({"doc": self.doc, "base": o.base, "cseq": o.cseq, "ops": o.sent_op.to_ops()}))
    }

    /// The connection broke: the edit in flight is sent again (same `cseq`) after the resume.
    pub fn unsend(&mut self) {
        if let Some(o) = self.outstanding.as_mut() {
            o.sent = false;
        }
    }

    fn check(&self, hash: &str) -> Result<(), Diverged> {
        if text_hash(&self.server_text) == hash {
            Ok(())
        } else {
            Err(Diverged(format!(
                "{}: the window's text differs from the backend's at revision {}",
                self.doc, self.server_rev
            )))
        }
    }

    /// Another client's (or the disk's) change at `revision`, transformed against the pending
    /// queue and merged into the text.
    pub fn remote_change(
        &mut self,
        revision: u64,
        ops: &[Op],
        hash: Option<&str>,
    ) -> Result<(), Diverged> {
        if revision <= self.server_rev {
            return Ok(());
        }
        if revision != self.server_rev + 1 {
            return Err(Diverged(format!(
                "{}: revision {revision} arrived after {}",
                self.doc, self.server_rev
            )));
        }
        let fail = |e: RpcError| Diverged(format!("{}: {e}", self.doc));
        let mut op = TextOp::from_ops(self.server_text.len_chars(), ops).map_err(fail)?;
        op.apply(&mut self.server_text).map_err(fail)?;
        self.server_rev = revision;
        if let Some(h) = hash {
            self.check(h)?;
        }
        if let Some(o) = self.outstanding.as_mut() {
            let (op2, o2) = TextOp::transform(&op, &o.op).map_err(fail)?;
            o.op = o2;
            op = op2;
        }
        if let Some(b) = self.buffer.as_mut() {
            let (op2, b2) = TextOp::transform(&op, b).map_err(fail)?;
            *b = b2;
            op = op2;
        }
        self.stamp = next_stamp();
        op.apply(&mut self.text).map_err(fail)
    }

    /// The backend acknowledged edit `cseq` at `revision`; returns whether it was the one in
    /// flight. The buffered edit becomes the next one to send.
    pub fn acked(
        &mut self,
        cseq: u64,
        revision: u64,
        hash: Option<&str>,
    ) -> Result<bool, Diverged> {
        let Some(o) = self
            .outstanding
            .as_ref()
            .filter(|o| o.cseq == cseq && o.cseq != 0)
        else {
            return Ok(false);
        };
        if revision < self.server_rev || revision > self.server_rev + 1 {
            return Err(Diverged(format!(
                "{}: acknowledged at revision {revision}, the window knows {}",
                self.doc, self.server_rev
            )));
        }
        o.op.apply(&mut self.server_text)
            .map_err(|e| Diverged(e.to_string()))?;
        self.server_rev = revision;
        self.outstanding = self.buffer.take().map(|op| Outstanding {
            cseq: 0,
            base: revision,
            sent_op: op.clone(),
            op,
            sent: false,
        });
        if let Some(h) = hash {
            self.check(h)?;
        }
        Ok(true)
    }

    /// Takes the backend's text after a divergence; the window's own text is kept as a local
    /// version when it differs, so nothing is lost.
    pub fn adopt(&mut self, revision: u64, text: &str, modified: bool) {
        let mine = self.text();
        if mine != text {
            self.local_version = Some(mine);
        }
        let rope = Rope::from_str(text);
        self.server_text = rope.clone();
        self.text = rope;
        self.server_rev = revision;
        self.stamp = next_stamp();
        self.outstanding = None;
        self.buffer = None;
        self.modified = modified || self.local_version.is_some();
    }
}

/// A recovery copy of a document with unsaved or unacknowledged edits (`SPEC-0317` behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryCopy {
    /// Host label.
    pub host: String,
    /// Project folder on the host.
    pub folder: String,
    /// Document path relative to the folder.
    pub path: String,
    /// The last acknowledged revision.
    pub base_revision: u64,
    /// Hash of the host file at load or last save.
    pub base_hash: String,
    /// Edits not acknowledged yet.
    pub pending: Vec<Vec<Op>>,
    /// The window's text.
    pub text: String,
}

/// Where a window keeps recovery copies (its own state folder, 0700, files 0600).
#[derive(Debug, Clone)]
pub struct Recovery {
    /// The folder.
    pub dir: PathBuf,
    /// Host label.
    pub host: String,
    /// Project folder on the host.
    pub folder: String,
}

/// One job of the recovery writer: the newest copy of a file, or its removal.
enum Job {
    Write {
        dir: PathBuf,
        file: PathBuf,
        copy: RecoveryCopy,
        text: Rope,
    },
    Remove(PathBuf),
}

/// The writer of recovery copies (`SPEC-0317` v3 behaviour 13): copies are written on a thread of
/// their own, newest text per file only, so typing never waits for the disk (a durable write on
/// macOS takes several milliseconds) and a burst of keystrokes costs one write.
struct Writer {
    tx: Mutex<mpsc::Sender<Job>>,
    /// Jobs queued and not done yet.
    open: Arc<(Mutex<usize>, Condvar)>,
}

fn writer() -> &'static Writer {
    static W: OnceLock<Writer> = OnceLock::new();
    W.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        let open = Arc::new((Mutex::new(0usize), Condvar::new()));
        let done = Arc::clone(&open);
        std::thread::Builder::new()
            .name("recovery-copies".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let mut batch = vec![first];
                    batch.extend(rx.try_iter());
                    let n = batch.len();
                    // The newest job per file wins.
                    let mut latest: Vec<(PathBuf, Job)> = Vec::new();
                    for job in batch {
                        let key = match &job {
                            Job::Write { file, .. } | Job::Remove(file) => file.clone(),
                        };
                        latest.retain(|(k, _)| *k != key);
                        latest.push((key, job));
                    }
                    for (_, job) in latest {
                        if let Err(e) = run_job(job) {
                            eprintln!("recovery copy: {e}");
                        }
                    }
                    let (count, cv) = &*done;
                    let mut c = count.lock().unwrap_or_else(|e| e.into_inner());
                    *c = c.saturating_sub(n);
                    cv.notify_all();
                }
            })
            .ok();
        Writer {
            tx: Mutex::new(tx),
            open,
        }
    })
}

fn run_job(job: Job) -> std::io::Result<()> {
    match job {
        Job::Remove(file) => match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        Job::Write {
            dir,
            file,
            mut copy,
            text,
        } => {
            copy.text = text.to_string();
            let bytes = serde_json::to_vec_pretty(&copy).map_err(std::io::Error::other)?;
            #[cfg(unix)]
            {
                crate::reattach::private_dir(&dir)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                crate::reattach::write_private(&file, &bytes)
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(&dir)?;
                idecraft_core::write_atomic(&file, bytes)
            }
        }
    }
}

fn queue(job: Job) {
    let w = writer();
    {
        let (count, _) = &*w.open;
        *count.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    }
    let sent = w.tx.lock().map(|tx| tx.send(job).is_ok()).unwrap_or(false);
    if !sent {
        let (count, cv) = &*w.open;
        let mut c = count.lock().unwrap_or_else(|e| e.into_inner());
        *c = c.saturating_sub(1);
        cv.notify_all();
    }
}

impl Recovery {
    fn file(&self, doc: &str) -> PathBuf {
        let key = crate::ot::str_hash(&format!("{}\n{}\n{doc}", self.host, self.folder));
        self.dir.join(format!("{key}.json"))
    }

    /// Writes the copy of `d` (write-then-rename, `SPEC-0262`) on the writer thread.
    pub fn write(&self, d: &RemoteDoc) -> std::io::Result<()> {
        let copy = RecoveryCopy {
            host: self.host.clone(),
            folder: self.folder.clone(),
            path: d.doc.clone(),
            base_revision: d.server_rev,
            base_hash: d.saved_hash.clone(),
            pending: d.pending_ops(),
            text: String::new(),
        };
        queue(Job::Write {
            dir: self.dir.clone(),
            file: self.file(&d.doc),
            copy,
            text: d.text.clone(),
        });
        Ok(())
    }

    /// Writes `copy` as it is: the window's newest text while the session is busy elsewhere
    /// (`SPEC-0317` v3 behaviour 13).
    pub fn write_copy(&self, copy: &RecoveryCopy) -> std::io::Result<()> {
        let mut c = copy.clone();
        let text = Rope::from_str(&std::mem::take(&mut c.text));
        queue(Job::Write {
            dir: self.dir.clone(),
            file: self.file(&copy.path),
            copy: c,
            text,
        });
        Ok(())
    }

    /// Waits until every copy asked for is written or removed.
    pub fn flush() {
        let w = writer();
        let (count, cv) = &*w.open;
        let mut c = count.lock().unwrap_or_else(|e| e.into_inner());
        while *c > 0 {
            c = cv.wait(c).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The copies of this host and folder, offered for restoring after a crash of the window
    /// (`SPEC-0317` v3 behaviour 13).
    pub fn copies(&self) -> Vec<RecoveryCopy> {
        Self::list(&self.dir)
            .into_iter()
            .filter(|c| c.host == self.host && c.folder == self.folder)
            .collect()
    }

    /// Removes the copy of `doc` (saved on the host).
    pub fn remove(&self, doc: &str) {
        queue(Job::Remove(self.file(doc)));
    }

    /// Whether `doc` has a copy.
    pub fn exists(&self, doc: &str) -> bool {
        Self::flush();
        self.file(doc).exists()
    }

    /// Every copy in `dir` (offered for restoring after a crash of the window).
    pub fn list(dir: &Path) -> Vec<RecoveryCopy> {
        Self::flush();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<RecoveryCopy> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| std::fs::read(e.path()).ok())
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect();
        out.sort_by(|a, b| (&a.host, &a.folder, &a.path).cmp(&(&b.host, &b.folder, &b.path)));
        out
    }
}

/// What the backend said about itself in `session.hello` (never the token).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInfo {
    /// Session id.
    pub session: String,
    /// This window's client id.
    pub client: String,
    /// Protocol major.
    pub major: u32,
    /// Backend version and build.
    pub build: String,
    /// Project folder on the host.
    pub root: String,
    /// Host platform.
    pub platform: String,
    /// Granted rights.
    pub rights: Vec<String>,
    /// Capabilities.
    pub capabilities: Vec<String>,
}

/// How a reconnection ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Reconnected {
    /// The session was resumed; missed notifications were replayed.
    Resumed {
        /// Edits sent again.
        resent: usize,
        /// The replay was gone; documents were read again.
        resync: bool,
    },
    /// A new backend; documents were reopened (`followed`) or left for the user (`kept`).
    NewBackend {
        /// Documents whose edits went to the new backend.
        followed: Vec<String>,
        /// Documents whose host file changed: the window's text stays as a local version.
        kept: Vec<String>,
    },
}

/// A remote project's session in the window.
pub struct ClientSession {
    conn: Option<Connection>,
    inbox: Option<mpsc::Receiver<Inbound>>,
    /// The backend's answer to `session.hello`.
    pub info: SessionInfo,
    token: Token,
    project: String,
    build: String,
    label: String,
    received: Received,
    acked_seq: u64,
    last_ack: Instant,
    docs: BTreeMap<String, RemoteDoc>,
    edits: HashMap<u64, (String, u64)>,
    next_cseq: u64,
    /// File tree events received (`fs.changed`), for the tree to drain.
    pub tree_events: Vec<Value>,
    /// Paths from the backend that were refused because they leave the project.
    pub refused: Vec<String>,
    /// The connection log.
    pub log: Vec<String>,
    /// The file tree must be read again (after `session.resync`).
    pub tree_stale: bool,
    /// Notifications of the host families (`lsp`, `git`, `term`, `run`, `port`, `dap`;
    /// `SPEC-0318`), in arrival order, for the window to drain.
    pub host_events: Vec<(String, Value)>,
    /// Listeners of host notifications by method prefix (a host terminal or run of the window
    /// follows its own events; they get a copy, `host_events` keeps its own).
    host_listeners: Vec<(String, mpsc::Sender<(String, Value)>)>,
    recovery: Option<Recovery>,
    last_rtt: Option<Duration>,
}

fn gone() -> RpcError {
    RpcError::new(ErrorCode::NotFound, "the document is no longer open")
}

fn err_text(e: &RpcError) -> String {
    format!("{}: {}", e.code.as_str(), e.message)
}

impl ClientSession {
    /// Opens a session on `transport` for `project` on the backend (`session.hello`).
    pub fn open(
        transport: Transport,
        project: &str,
        build: &str,
        label: &str,
        recovery: Option<Recovery>,
    ) -> Result<Self, RpcError> {
        let (conn, inbox) = Connection::start(transport);
        let hello = conn.call(
            "session.hello",
            json!({
                "protocol": PROTOCOL, "majors": [MAJOR], "build": build, "client": "window",
                "project": project, "capabilities": ["doc.ops", "fs", "fs.watch", "event"],
            }),
            CALL_LIMIT,
        );
        let hello = match hello {
            Ok(h) => h,
            Err(e) => {
                conn.close();
                return Err(e);
            }
        };
        // A backend older than this window is refused here, not at the first method it lacks; the
        // session it just opened ends now so that nothing stays on the host.
        if let Err(e) = crate::hello::check_backend_minor(
            hello["minor"].as_u64(),
            hello["build"].as_str().unwrap_or("unknown build"),
        ) {
            let _ = conn.call("session.bye", json!({}), Duration::from_secs(5));
            conn.close();
            return Err(e);
        }
        let token = hello["token"]
            .as_str()
            .and_then(Token::parse)
            .ok_or_else(|| RpcError::new(ErrorCode::Invalid, "the backend sent no token"))?;
        let strings = |v: &Value| {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let info = SessionInfo {
            session: hello["session"].as_str().unwrap_or_default().to_owned(),
            client: hello["client"].as_str().unwrap_or_default().to_owned(),
            major: hello["major"].as_u64().unwrap_or(0) as u32,
            build: hello["build"].as_str().unwrap_or_default().to_owned(),
            root: hello["root"].as_str().unwrap_or_default().to_owned(),
            platform: hello["platform"].as_str().unwrap_or_default().to_owned(),
            rights: strings(&hello["rights"]),
            capabilities: strings(&hello["capabilities"]),
        };
        Ok(Self {
            conn: Some(conn),
            inbox: Some(inbox),
            log: vec![format!("connected: session {}", info.session)],
            info,
            token,
            project: project.to_owned(),
            build: build.to_owned(),
            label: label.to_owned(),
            received: Received::default(),
            acked_seq: 0,
            last_ack: Instant::now(),
            docs: BTreeMap::new(),
            edits: HashMap::new(),
            next_cseq: 0,
            tree_events: Vec::new(),
            refused: Vec::new(),
            tree_stale: false,
            host_events: Vec::new(),
            host_listeners: Vec::new(),
            recovery,
            last_rtt: None,
        })
    }

    /// Copies of the host notifications whose method starts with `prefix` from now on, as they
    /// arrive (also after a resume); the listener ends when its receiver is dropped.
    pub fn listen_host(&mut self, prefix: &str) -> mpsc::Receiver<(String, Value)> {
        let (tx, rx) = mpsc::channel();
        self.host_listeners.push((prefix.to_owned(), tx));
        rx
    }

    /// Whether the connection works.
    pub fn connected(&self) -> bool {
        self.conn.as_ref().is_some_and(Connection::alive)
    }

    /// The token, only to send it in `session.resume` over a new channel.
    pub fn resume_token(&self) -> &Token {
        &self.token
    }

    /// The notification `seq` up to which everything was handled.
    pub fn last_seq(&self) -> u64 {
        self.received.upto
    }

    /// Open documents.
    pub fn docs(&self) -> impl Iterator<Item = &RemoteDoc> {
        self.docs.values()
    }

    /// One open document.
    pub fn doc(&self, id: &str) -> Option<&RemoteDoc> {
        self.docs.get(id)
    }

    /// Takes the window's own text kept as a local version of `doc` (after a new backend or a
    /// divergence), so that the editor, which still shows it, sends it again as an edit; the
    /// document stays marked as changed on the host, so saving asks first (`SPEC-0317` v3
    /// behaviour 15).
    pub fn take_local_version(&mut self, doc: &str) -> Option<String> {
        let d = self.docs.get_mut(doc)?;
        let v = d.local_version.take();
        if v.is_some() {
            d.disk_changed = true;
        }
        v
    }

    /// Edits not acknowledged yet, over all documents.
    pub fn pending_edits(&self) -> usize {
        self.docs.values().map(RemoteDoc::pending).sum()
    }

    /// The round trip of the last heartbeat.
    pub fn rtt(&self) -> Option<Duration> {
        self.last_rtt
    }

    /// Whether `doc` has a recovery copy.
    pub fn has_recovery(&self, doc: &str) -> bool {
        self.recovery.as_ref().is_some_and(|r| r.exists(doc))
    }

    fn not_connected(&self) -> RpcError {
        RpcError::new(
            ErrorCode::Unavailable,
            format!("reconnecting to {}", self.label),
        )
        .hint("the request is possible again once the connection is back")
    }

    /// A request to the backend (fails at once with `unavailable` while disconnected).
    pub fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.call_within(method, params, CALL_LIMIT)
    }

    /// The connection, while there is one: long host requests (search, completion) and streams go
    /// through a clone of it, so they do not hold the session.
    pub fn connection(&self) -> Option<Connection> {
        self.conn.as_ref().filter(|c| c.alive()).cloned()
    }

    /// [`ClientSession::call`] waiting up to `limit`.
    pub fn call_within(
        &self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, RpcError> {
        match &self.conn {
            Some(c) if c.alive() => c.call(method, params, limit),
            _ => Err(self.not_connected()),
        }
    }

    /// `session.ping`; remembers the round trip.
    pub fn ping(&mut self) -> Result<Duration, RpcError> {
        self.ping_within(CALL_LIMIT)
    }

    /// `session.ping` answered within `limit` (the window's heartbeat: no answer in time counts
    /// as a break, `SPEC-0317` v3 behaviour 14); remembers the round trip.
    pub fn ping_within(&mut self, limit: Duration) -> Result<Duration, RpcError> {
        let t0 = Instant::now();
        match &self.conn {
            Some(c) if c.alive() => c.call("session.ping", json!({}), limit)?,
            _ => return Err(self.not_connected()),
        };
        let rtt = t0.elapsed();
        self.last_rtt = Some(rtt);
        Ok(rtt)
    }

    /// Where this session keeps recovery copies, if it does.
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }

    /// Lists a folder of the project; entries whose path would leave it are refused.
    pub fn list(&mut self, path: &str) -> Result<Vec<Value>, RpcError> {
        let path = check_relative(path)?;
        let v = self.call("fs.list", json!({ "path": path }))?;
        let mut out = Vec::new();
        for e in v["entries"].as_array().cloned().unwrap_or_default() {
            match e["path"].as_str().map(check_relative) {
                Some(Ok(_)) => out.push(e),
                _ => self.refused.push(e["path"].to_string()),
            }
        }
        Ok(out)
    }

    /// Subscribes to file tree events (`fs.watch`).
    pub fn watch(&self, exclude: &[&str]) -> Result<Value, RpcError> {
        self.call("fs.watch", json!({ "exclude": exclude }))
    }

    /// Opens a document.
    pub fn open_doc(&mut self, path: &str) -> Result<&RemoteDoc, RpcError> {
        let path = check_relative(path)?;
        let v = self.call("doc.open", json!({ "path": path }))?;
        let id = v["doc"].as_str().unwrap_or_default().to_owned();
        check_relative(&id)?;
        let text = match v["stream"].as_u64() {
            Some(stream) => {
                let conn = self.conn.as_ref().ok_or_else(|| self.not_connected())?;
                let bytes = conn.stream(stream as u32, Duration::from_secs(120))?;
                String::from_utf8(bytes)
                    .map_err(|_| RpcError::new(ErrorCode::Invalid, "the text is not UTF-8"))?
            }
            None => v["text"].as_str().unwrap_or_default().to_owned(),
        };
        if v["hash"].as_str() != Some(crate::ot::str_hash(&text).as_str()) {
            return Err(RpcError::new(
                ErrorCode::Internal,
                "the text arrived damaged",
            ));
        }
        let doc = RemoteDoc::new(
            &id,
            v["revision"].as_u64().unwrap_or(1),
            &text,
            v["modified"].as_bool().unwrap_or(false),
        );
        self.docs.insert(id.clone(), doc);
        Ok(&self.docs[&id])
    }

    fn write_recovery(&self, id: &str) {
        if let (Some(r), Some(d)) = (&self.recovery, self.docs.get(id))
            && (d.modified || d.pending() > 0)
            && let Err(e) = r.write(d)
        {
            eprintln!("recovery copy of {id}: {e}");
        }
    }

    /// The user's edit of `doc`: `ops` are ranges of the window's current text. Applied at once;
    /// sent when possible.
    pub fn edit(&mut self, doc: &str, ops: &[Op]) -> Result<(), RpcError> {
        let d = self
            .docs
            .get_mut(doc)
            .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("{doc} is not open")))?;
        let op = TextOp::from_ops(d.text.len_chars(), ops)?;
        d.local_edit(op)?;
        self.write_recovery(doc);
        self.flush();
        Ok(())
    }

    fn flush(&mut self) {
        let Some(conn) = self.conn.clone().filter(Connection::alive) else {
            return;
        };
        let mut sends = Vec::new();
        for d in self.docs.values_mut() {
            if let Some(params) = d.take_send(&mut self.next_cseq) {
                sends.push((d.doc.clone(), params));
            }
        }
        for (doc, params) in sends {
            let cseq = params["cseq"].as_u64().unwrap_or(0);
            match conn.send("doc.edit", params) {
                Ok(id) => {
                    self.edits.insert(id, (doc, cseq));
                }
                Err(_) => {
                    if let Some(d) = self.docs.get_mut(&doc) {
                        d.unsend();
                    }
                }
            }
        }
        let handled = self.received.upto;
        if handled > self.acked_seq && self.last_ack.elapsed() >= Duration::from_secs(1) {
            let _ = conn.notify("session.ack", json!({ "seq": handled }));
            self.acked_seq = handled;
            self.last_ack = Instant::now();
        }
    }

    fn diverged(&mut self, doc: &str, why: &str) {
        self.log.push(format!("{why}; reading the backend's text"));
        let snap = self.call("doc.snapshot", json!({ "doc": doc }));
        if let (Ok(s), Some(d)) = (snap, self.docs.get_mut(doc)) {
            d.adopt(
                s["revision"].as_u64().unwrap_or(1),
                s["text"].as_str().unwrap_or_default(),
                s["modified"].as_bool().unwrap_or(true),
            );
        }
        self.write_recovery(doc);
    }

    fn handle(&mut self, msg: Inbound) {
        match msg {
            Inbound::Notification { method, params } => {
                if let Some(seq) = params["seq"].as_u64()
                    && !self.received.first_time(seq)
                {
                    return;
                }
                let doc = params["doc"].as_str().unwrap_or_default().to_owned();
                let result = match method.as_str() {
                    "doc.changed" => {
                        let ops: Vec<Op> =
                            serde_json::from_value(params["ops"].clone()).unwrap_or_default();
                        let rev = params["revision"].as_u64().unwrap_or(0);
                        let from_disk = params["author"] == "disk";
                        let other = !from_disk && params["author"] != self.info.client.as_str();
                        self.docs.get_mut(&doc).map(|d| {
                            let fresh = rev > d.server_rev;
                            let r = d.remote_change(rev, &ops, params["hash"].as_str());
                            if from_disk && r.is_ok() && d.pending() == 0 {
                                d.saved_hash = text_hash(&d.server_text);
                                d.modified = false;
                            } else if other && fresh && r.is_ok() && !ops.is_empty() {
                                // Another author's edit (an agent, a rename of the host's
                                // language server) leaves the host document unsaved too.
                                d.modified = true;
                            }
                            r
                        })
                    }
                    "doc.acked" => self.docs.get_mut(&doc).map(|d| {
                        d.acked(
                            params["cseq"].as_u64().unwrap_or(0),
                            params["revision"].as_u64().unwrap_or(0),
                            params["hash"].as_str(),
                        )
                        .map(drop)
                    }),
                    "doc.saved" => {
                        if let Some(d) = self.docs.get_mut(&doc)
                            && d.pending() == 0
                            && d.server_rev == params["revision"].as_u64().unwrap_or(0)
                        {
                            d.modified = false;
                            d.saved_hash = text_hash(&d.server_text);
                        }
                        None
                    }
                    "doc.disk_changed" => {
                        if let Some(d) = self.docs.get_mut(&doc) {
                            d.disk_changed = true;
                        }
                        None
                    }
                    "fs.changed" => {
                        for e in params["events"].as_array().cloned().unwrap_or_default() {
                            let ok = ["path", "from"].iter().all(|k| {
                                e.get(*k).is_none_or(|p| {
                                    p.as_str().is_some_and(|p| check_relative(p).is_ok())
                                })
                            });
                            if ok {
                                self.tree_events.push(e);
                            } else {
                                self.refused.push(e.to_string());
                                self.log
                                    .push(format!("refused a path from the backend: {e}"));
                            }
                        }
                        None
                    }
                    "session.resync" => {
                        self.tree_stale = true;
                        None
                    }
                    m if HOST_FAMILIES.iter().any(|f| m.starts_with(f)) => {
                        self.host_listeners.retain(|(prefix, tx)| {
                            !method.starts_with(prefix.as_str())
                                || tx.send((method.clone(), params.clone())).is_ok()
                        });
                        self.host_events.push((method.clone(), params.clone()));
                        // Nobody may drain them (a headless agent session): the oldest go.
                        let over = self.host_events.len().saturating_sub(HOST_EVENTS_KEPT);
                        self.host_events.drain(..over);
                        return;
                    }
                    _ => None,
                };
                if let Some(Err(Diverged(why))) = result {
                    self.diverged(&doc, &why);
                } else if self.docs.contains_key(&doc) {
                    self.write_recovery(&doc);
                }
            }
            Inbound::Response { id, result } => {
                let Some((doc, cseq)) = self.edits.remove(&id) else {
                    return;
                };
                match result {
                    Ok(v) => {
                        let r = self.docs.get_mut(&doc).map(|d| {
                            d.acked(
                                cseq,
                                v["revision"].as_u64().unwrap_or(0),
                                v["hash"].as_str(),
                            )
                        });
                        if let Some(Err(Diverged(why))) = r {
                            self.diverged(&doc, &why);
                        }
                    }
                    Err(e) => {
                        let why = format!("edit of {doc} refused: {}", err_text(&e));
                        self.diverged(&doc, &why);
                    }
                }
            }
            // A host never operates the window (SPEC-0319 behaviour 31, ADR-0025 T5).
            Inbound::Request { id, method, .. } if crate::backend::is_ui_method(&method) => {
                self.log.push(format!(
                    "refused {method} from the backend: a host never operates the window"
                ));
                self.refused.push(method.clone());
                if let Some(c) = &self.conn {
                    let _ = c.respond(
                        id,
                        Err(crate::rpc::RpcError::new(
                            crate::rpc::ErrorCode::Denied,
                            format!("denied: {method}: a host never operates the window"),
                        )),
                    );
                }
            }
            Inbound::Request { .. } => {}
            Inbound::Closed(reason) => {
                self.log.push(format!("disconnected: {reason}"));
                for d in self.docs.values_mut() {
                    d.unsend();
                }
                self.edits.clear();
            }
        }
    }

    /// Handles what arrived, waiting up to `wait` for the first message, then sends queued
    /// edits and the acknowledgement of handled notifications.
    pub fn pump(&mut self, wait: Duration) {
        let mut first = true;
        loop {
            let next = match &self.inbox {
                Some(rx) if first && !wait.is_zero() => rx.recv_timeout(wait).ok(),
                Some(rx) => rx.try_recv().ok(),
                None => None,
            };
            first = false;
            let Some(msg) = next else {
                break;
            };
            self.handle(msg);
        }
        self.flush();
    }

    /// Pumps until every edit is acknowledged or `limit` passes; returns whether it settled.
    pub fn settle(&mut self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while self.pending_edits() > 0 && self.connected() {
            if Instant::now() >= deadline {
                return false;
            }
            self.pump(Duration::from_millis(20));
        }
        self.pump(Duration::ZERO);
        self.pending_edits() == 0
    }

    /// Saves `doc` on the host after its edits are acknowledged. While disconnected the save is
    /// remembered and carried out after the resume, and the answer says so.
    pub fn save(&mut self, doc: &str, overwrite: bool) -> Result<u64, RpcError> {
        if !self.docs.contains_key(doc) {
            return Err(RpcError::new(
                ErrorCode::NotFound,
                format!("{doc} is not open"),
            ));
        }
        if !self.connected() {
            if let Some(d) = self.docs.get_mut(doc) {
                d.save_requested = true;
            }
            return Err(RpcError::new(
                ErrorCode::Unavailable,
                format!(
                    "Not saved — reconnecting to {}; the file is saved when the connection is back",
                    self.label
                ),
            ));
        }
        if !self.settle(CALL_LIMIT) {
            return Err(RpcError::new(
                ErrorCode::Unavailable,
                format!("{doc}: edits not acknowledged yet"),
            ));
        }
        let rev = self.docs[doc].server_rev;
        let saved = self.call(
            "doc.save",
            json!({"doc": doc, "revision": rev, "overwrite": overwrite}),
        );
        let d = self.docs.get_mut(doc).ok_or_else(gone)?;
        match saved {
            Ok(v) => {
                d.modified = false;
                d.save_requested = false;
                d.disk_changed = false;
                d.saved_hash = text_hash(&d.server_text);
                if let Some(r) = &self.recovery {
                    r.remove(doc);
                }
                Ok(v["revision"].as_u64().unwrap_or(rev))
            }
            Err(e) => {
                if e.code == ErrorCode::Conflict {
                    d.disk_changed = true;
                }
                Err(e)
            }
        }
    }

    /// Closes a document.
    pub fn close_doc(&mut self, doc: &str) -> Result<(), RpcError> {
        self.call("doc.close", json!({ "doc": doc }))?;
        self.docs.remove(doc);
        Ok(())
    }

    /// Ends the session (`session.bye`); returns the documents unsaved on the backend.
    pub fn bye(&mut self) -> Result<Vec<String>, RpcError> {
        let v = self.call("session.bye", json!({}))?;
        if let Some(c) = &self.conn {
            c.close();
        }
        self.conn = None;
        Ok(v["unsaved"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Breaks the connection without ending the session (tests, sleep).
    pub fn drop_connection(&mut self) {
        if let Some(c) = &self.conn {
            c.close();
        }
        self.pump(Duration::from_millis(50));
    }

    /// Resumes the session over a new channel to the same backend (`session.resume` as the first
    /// message), replays what was missed and sends pending edits again (applied once).
    pub fn resume(&mut self, transport: Transport) -> Result<Reconnected, RpcError> {
        if let Some(c) = self.conn.take() {
            c.close();
        }
        // Whatever the old connection still delivered is handled first.
        self.pump(Duration::ZERO);
        let (conn, inbox) = Connection::start(transport);
        let answer = conn.call(
            "session.resume",
            json!({
                "session": self.info.session, "token": self.token.expose(), "last_seq": self.received.upto,
            }),
            CALL_LIMIT,
        );
        let answer = match answer {
            Ok(a) => a,
            Err(e) => {
                conn.close();
                self.log.push(format!("resume refused: {}", err_text(&e)));
                return Err(e);
            }
        };
        self.conn = Some(conn);
        self.inbox = Some(inbox);
        let resync = answer["resync"].as_bool() == Some(true);
        if resync {
            self.received
                .restart(answer["last_seq"].as_u64().unwrap_or(0));
        }
        let resent = self
            .docs
            .values()
            .filter(|d| d.outstanding.is_some())
            .count();
        self.log.push(format!(
            "resumed session {} ({resent} edits sent again{})",
            self.info.session,
            if resync { ", state read again" } else { "" }
        ));
        if resync {
            self.resync_docs()?;
            self.tree_stale = true;
        }
        self.pump(Duration::from_millis(50));
        self.settle(CALL_LIMIT);
        self.saves_after_resume();
        Ok(Reconnected::Resumed { resent, resync })
    }

    fn saves_after_resume(&mut self) {
        let wanted: Vec<String> = self
            .docs
            .values()
            .filter(|d| d.save_requested)
            .map(|d| d.doc.clone())
            .collect();
        for doc in wanted {
            match self.save(&doc, false) {
                Ok(_) => self.log.push(format!("saved {doc} after the resume")),
                Err(e) => self.log.push(format!("{doc} not saved: {}", err_text(&e))),
            }
        }
    }

    /// The replay is gone: each document reads the changes it missed from the backend's log
    /// (`doc.changes`) and merges them as it would have live — its own edit in flight counts as
    /// acknowledged when it is among them, otherwise it is sent again (applied once). When the
    /// log no longer reaches back, the backend's text is taken and the window's text kept as a
    /// local version.
    fn resync_docs(&mut self) -> Result<(), RpcError> {
        let ids: Vec<String> = self.docs.keys().cloned().collect();
        let me = self.info.client.clone();
        for id in ids {
            let since = self.docs.get(&id).map_or(0, |d| d.server_rev);
            let changes = self.call("doc.changes", json!({ "doc": id, "since": since }));
            let d = self.docs.get_mut(&id).ok_or_else(gone)?;
            let merged = changes.map_err(|e| Diverged(err_text(&e))).and_then(|v| {
                for c in v["changes"].as_array().cloned().unwrap_or_default() {
                    let rev = c["revision"].as_u64().unwrap_or(0);
                    let cseq = c["cseq"].as_u64().unwrap_or(0);
                    if c["author"] == me.as_str() && d.acked(cseq, rev, None)? {
                        continue;
                    }
                    let ops: Vec<Op> = serde_json::from_value(c["ops"].clone()).unwrap_or_default();
                    d.remote_change(rev, &ops, None)?;
                }
                d.check(v["hash"].as_str().unwrap_or_default())
            });
            if let Err(Diverged(why)) = merged {
                self.diverged(&id, &why);
            }
            self.write_recovery(&id);
        }
        self.flush();
        Ok(())
    }

    /// The old backend is gone: opens a new session on `transport` and reconciles every
    /// document. A document whose host file still equals the text its edits were based on
    /// follows the new backend with the window's changes sent as an edit; otherwise the window's
    /// text stays as an unsaved local version for the user to compare (`SPEC-0317` behaviour 15).
    pub fn replace_backend(&mut self, transport: Transport) -> Result<Reconnected, RpcError> {
        let mut fresh = ClientSession::open(
            transport,
            &self.project,
            &self.build,
            &self.label,
            self.recovery.clone(),
        )?;
        let (mut followed, mut kept) = (Vec::new(), Vec::new());
        let old: Vec<RemoteDoc> = self.docs.values().cloned().collect();
        for d in old {
            let unsaved = d.modified || d.pending() > 0 || d.local_version.is_some();
            let mine = d.text();
            let opened = fresh.open_doc(&d.doc).map(|n| n.text());
            let host = match opened {
                Ok(t) => t,
                Err(e) => {
                    fresh.log.push(format!("{}: {}", d.doc, err_text(&e)));
                    continue;
                }
            };
            if !unsaved {
                continue;
            }
            if crate::ot::str_hash(&host) == d.saved_hash {
                let op = TextOp::diff(&host, &mine);
                fresh.edit(&d.doc, &op.to_ops())?;
                if let Some(n) = fresh.docs.get_mut(&d.doc) {
                    n.save_requested = d.save_requested;
                    n.local_version = d.local_version.clone();
                }
                followed.push(d.doc.clone());
            } else if let Some(n) = fresh.docs.get_mut(&d.doc) {
                n.local_version = Some(mine);
                n.modified = true;
                n.disk_changed = true;
                kept.push(d.doc.clone());
            }
        }
        fresh.settle(CALL_LIMIT);
        let mut log = std::mem::take(&mut self.log);
        log.push(format!(
            "new backend, session {}: followed {followed:?}, kept as local versions {kept:?}",
            fresh.info.session
        ));
        log.append(&mut fresh.log);
        fresh.log = log;
        for id in fresh.docs.keys().cloned().collect::<Vec<_>>() {
            fresh.write_recovery(&id);
        }
        *self = fresh;
        Ok(Reconnected::NewBackend { followed, kept })
    }

    /// One line per document for status displays and MCP (never the token).
    pub fn describe_docs(&self) -> Vec<Value> {
        self.docs
            .values()
            .map(|d| {
                json!({
                    "path": d.doc, "revision": d.server_rev, "pending_edits": d.pending(),
                    "modified": d.modified, "recovery_copy": self.has_recovery(&d.doc),
                    "local_version": d.local_version.is_some(), "disk_changed": d.disk_changed,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Rng;

    #[test]
    fn notifications_that_overtook_others_are_replayed_only_where_needed() {
        let mut r = Received::default();
        assert!(r.first_time(1));
        // 3 (interactive) arrives before 2 (bulk): a resume asks from 1, so 2 is not lost …
        assert!(r.first_time(3));
        assert_eq!(r.upto, 1);
        // … and when the replay brings 2 and 3, only 2 is new.
        assert!(r.first_time(2));
        assert_eq!(r.upto, 3);
        assert!(!r.first_time(3));
        assert!(!r.first_time(2));
        assert!(r.first_time(9));
        r.restart(20);
        assert_eq!(r.upto, 20);
        assert!(!r.first_time(9));
        assert!(r.first_time(21));
    }

    fn op(at: usize, delete: usize, insert: &str) -> Op {
        Op {
            at,
            delete,
            insert: insert.into(),
        }
    }

    #[test]
    fn typing_is_local_and_the_queue_holds_two_edits() {
        let mut d = RemoteDoc::new("a.txt", 1, "abc", false);
        let mut cseq = 0;
        d.local_edit(TextOp::from_ops(3, &[op(3, 0, "d")]).unwrap())
            .unwrap();
        assert_eq!(d.text(), "abcd");
        let sent = d.take_send(&mut cseq).unwrap();
        assert_eq!(sent["cseq"], 1);
        assert_eq!(sent["base"], 1);
        assert!(d.take_send(&mut cseq).is_none());
        d.local_edit(TextOp::from_ops(4, &[op(4, 0, "e")]).unwrap())
            .unwrap();
        d.local_edit(TextOp::from_ops(5, &[op(0, 1, "")]).unwrap())
            .unwrap();
        assert_eq!((d.text().as_str(), d.pending()), ("bcde", 2));
        // Another client typed at the start meanwhile (revision 2).
        d.remote_change(2, &[op(0, 0, "X")], Some(&crate::ot::str_hash("Xabc")))
            .unwrap();
        assert_eq!(d.text(), "Xbcde");
        // Our edit is acknowledged at revision 3; the buffer is next.
        let ok = d.acked(1, 3, Some(&crate::ot::str_hash("Xabcd"))).unwrap();
        assert!(ok);
        let next = d.take_send(&mut cseq).unwrap();
        assert_eq!(
            (next["cseq"].as_u64(), next["base"].as_u64()),
            (Some(2), Some(3))
        );
        // A wrong hash is a divergence; adopting keeps the window's text.
        assert!(d.acked(2, 4, Some("0000000000000000")).is_err());
        d.adopt(4, "server", true);
        assert_eq!(d.local_version.as_deref(), Some("Xbcde"));
        assert_eq!((d.text().as_str(), d.pending()), ("server", 0));
        // A gap in revisions is a divergence too.
        assert!(d.remote_change(9, &[], None).is_err());
    }

    /// Two windows and an agent edit at random with random delivery; everything converges to
    /// the backend's text and hash (`SPEC-0317` AK-2).
    #[test]
    fn random_sessions_converge_through_the_backend() {
        use crate::docs::DocStore;
        let dir = std::env::temp_dir().join(format!("idecraft-conv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = std::fs::canonicalize(&dir).unwrap();
        for seed in 0..40u64 {
            let mut rng = Rng::new(seed);
            let name = format!("f{seed}.txt");
            std::fs::write(root.join(&name), rng.text(6)).unwrap();
            let mut store = DocStore::new(
                root.clone(),
                idecraft_editor::documents::DocumentRegistry::default(),
            );
            let clients = ["w1", "w2", "agent"];
            let mut replicas: Vec<RemoteDoc> = clients
                .iter()
                .map(|c| {
                    let i = store.open(&name, c).unwrap();
                    RemoteDoc::new(&name, i.revision, &i.text, false)
                })
                .collect();
            let mut cseqs = [0u64; 3];
            // Messages on the way to each client, in order: (kind, payload).
            let mut wire: Vec<std::collections::VecDeque<(bool, Value)>> =
                vec![Default::default(); 3];
            let mut to_backend: std::collections::VecDeque<(usize, Value)> = Default::default();
            for _ in 0..200 {
                match rng.below(4) {
                    0 => {
                        let i = rng.below(3);
                        let len = replicas[i].text.len_chars();
                        replicas[i]
                            .local_edit(TextOp::random(len, &mut rng))
                            .unwrap();
                    }
                    1 => {
                        for i in 0..3 {
                            if let Some(p) = replicas[i].take_send(&mut cseqs[i]) {
                                to_backend.push_back((i, p));
                            }
                        }
                    }
                    2 => {
                        if let Some((i, p)) = to_backend.pop_front() {
                            let ops: Vec<Op> = serde_json::from_value(p["ops"].clone()).unwrap();
                            let out = store
                                .edit(
                                    &name,
                                    p["base"].as_u64().unwrap(),
                                    clients[i],
                                    p["cseq"].as_u64().unwrap(),
                                    &ops,
                                )
                                .unwrap();
                            for (j, w) in wire.iter_mut().enumerate() {
                                if j == i {
                                    w.push_back((true, json!({"cseq": p["cseq"], "revision": out.revision, "hash": out.hash})));
                                } else if out.changed {
                                    w.push_back((false, json!({"revision": out.revision, "ops": out.ops, "hash": out.hash})));
                                }
                            }
                        }
                    }
                    _ => {
                        let j = rng.below(3);
                        if let Some((ack, v)) = wire[j].pop_front() {
                            if ack {
                                replicas[j]
                                    .acked(
                                        v["cseq"].as_u64().unwrap(),
                                        v["revision"].as_u64().unwrap(),
                                        v["hash"].as_str(),
                                    )
                                    .unwrap();
                            } else {
                                let ops: Vec<Op> =
                                    serde_json::from_value(v["ops"].clone()).unwrap();
                                replicas[j]
                                    .remote_change(
                                        v["revision"].as_u64().unwrap(),
                                        &ops,
                                        v["hash"].as_str(),
                                    )
                                    .unwrap();
                            }
                        }
                    }
                }
            }
            // Deliver everything that is left.
            loop {
                let mut moved = false;
                for i in 0..3 {
                    if let Some(p) = replicas[i].take_send(&mut cseqs[i]) {
                        to_backend.push_back((i, p));
                    }
                }
                while let Some((i, p)) = to_backend.pop_front() {
                    moved = true;
                    let ops: Vec<Op> = serde_json::from_value(p["ops"].clone()).unwrap();
                    let out = store
                        .edit(
                            &name,
                            p["base"].as_u64().unwrap(),
                            clients[i],
                            p["cseq"].as_u64().unwrap(),
                            &ops,
                        )
                        .unwrap();
                    for (j, w) in wire.iter_mut().enumerate() {
                        if j == i {
                            w.push_back((true, json!({"cseq": p["cseq"], "revision": out.revision, "hash": out.hash})));
                        } else if out.changed {
                            w.push_back((
                                false,
                                json!({"revision": out.revision, "ops": out.ops, "hash": out.hash}),
                            ));
                        }
                    }
                }
                for j in 0..3 {
                    while let Some((ack, v)) = wire[j].pop_front() {
                        moved = true;
                        if ack {
                            replicas[j]
                                .acked(
                                    v["cseq"].as_u64().unwrap(),
                                    v["revision"].as_u64().unwrap(),
                                    v["hash"].as_str(),
                                )
                                .unwrap();
                        } else {
                            let ops: Vec<Op> = serde_json::from_value(v["ops"].clone()).unwrap();
                            replicas[j]
                                .remote_change(
                                    v["revision"].as_u64().unwrap(),
                                    &ops,
                                    v["hash"].as_str(),
                                )
                                .unwrap();
                        }
                    }
                }
                if !moved {
                    break;
                }
            }
            let truth = store.snapshot(&name).unwrap();
            for r in &replicas {
                assert_eq!(r.pending(), 0);
                assert_eq!(r.text(), truth.text, "seed {seed}");
                assert_eq!(text_hash(&r.text), truth.hash);
                assert_eq!(r.server_rev, truth.revision);
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
