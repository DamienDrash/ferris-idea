//! Long-lived language servers shared by the editor and MCP (`SPEC-0090`).
//!
//! Without the pool every question starts a server, opens the file, asks and shuts the server down
//! again. With [`enable`] one server per workspace and server command keeps running: documents stay
//! open and follow the editor through `didChange`, and the diagnostics the server publishes while the
//! user types are collected for [`diagnostics_since`].

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};

use crate::LanguageId;
use crate::lsp::{Diagnostic, LspClient, LspError, Notification, diagnostic, file_uri};
use crate::servers::Resolved;

/// One running server of the pool.
pub(crate) struct Server {
    pub(crate) client: LspClient,
    pub(crate) capabilities: Value,
    diagnoses_first: bool,
    /// The build's diagnostics come first, refined ones after (`SPEC-0076` v6 behaviour 10).
    refines_diagnostics: bool,
    /// It publishes an opened file's diagnostics twice; the second list is the answer
    /// (`SPEC-0042` v20 behaviour 36). How many lists came for each open file.
    publishes_twice: bool,
    publication_counts: StdMutex<HashMap<String, u32>>,
    loading_text: Option<&'static str>,
    resend_on_open: bool,
    /// The server lists the project's files before its first diagnostics (`SPEC-0042` v14
    /// behaviour 22), and whether that first wait is over.
    lists_files_first: bool,
    listed: AtomicBool,
    /// The server reads the saved files after starting, over the first opened text, which is then
    /// sent again (`SPEC-0042` v18 behaviour 34), and whether that is done.
    rereads_saved_files: bool,
    reread: AtomicBool,
    /// The candidate's program name (`jdtls`), also when it is started through another program.
    program: &'static str,
    /// Open documents: version and text as the server has them, and the publication number when
    /// that text was sent (publications after it belong to this text).
    docs: Mutex<HashMap<String, (i64, String, u64)>>,
    /// Raw diagnostics last published per document URI, with the publication number.
    published: StdMutex<HashMap<String, (u64, Vec<Value>)>>,
    publications: AtomicU64,
    ready: AtomicBool,
    /// Whether the server says when it has loaded the workspace (`ready`).
    reports_status: bool,
    /// The `$/progress` tokens of work that loads the project and has not ended (`SPEC-0095` v10
    /// behaviour 12).
    loading_work: StdMutex<std::collections::HashSet<String>>,
    /// The indexing a server reports as work-done progress (`SPEC-0042` v16 behaviour 25).
    indexing: StdMutex<crate::lsp::Indexing>,
    /// The server indexes only when its stored index is out of date (intelephense), whether it
    /// has begun an indexing, and when it last said anything (`SPEC-0042` v19 behaviour 25).
    skips_indexing_when_cached: bool,
    indexing_begun: AtomicBool,
    /// Between `indexingStarted` and `indexingEnded`: what it publishes then is about a project it
    /// has not read and is no answer (`SPEC-0042` v20 behaviour 36).
    indexing_now: AtomicBool,
    last_heard: StdMutex<std::time::Instant>,
    /// Whether the server checks the project in the background after loading it and publishes
    /// what the compiler says only then (rust-analyzer outside safe mode, `SPEC-0042` v16
    /// behaviour 27), with the state of those checks.
    checks_in_background: bool,
    checks: StdMutex<Checks>,
    changed: Notify,
    /// When the server was started (`SPEC-0072` v8: TypeScript answers empty while loading).
    pub(crate) started: std::time::Instant,
}

impl Server {
    /// Number of the last publication for `uri` (0: none yet).
    pub(crate) fn publication(&self, uri: &str) -> u64 {
        lock(&self.published).get(uri).map_or(0, |(n, _)| *n)
    }

    /// Brings `uri` to `text` on the server: `didOpen` the first time, `didChange` (whole text) when
    /// the text differs. Returns whether anything was sent.
    async fn sync(
        &self,
        uri: &str,
        language: LanguageId,
        text: &str,
        wait: Duration,
    ) -> Result<bool, LspError> {
        let mut docs = self.docs.lock().await;
        let before = self.publication(uri);
        let first = match docs.get_mut(uri) {
            Some((_, known, _)) if known == text => return Ok(false),
            Some((version, known, sent_at)) => {
                *version += 1;
                *known = text.to_owned();
                *sent_at = before;
                let version = *version;
                self.client
                    .notify(
                        "textDocument/didChange",
                        json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}),
                    )
                    .await?;
                false
            }
            None => {
                self.client
                    .notify(
                        "textDocument/didOpen",
                        json!({"textDocument": {"uri": uri, "languageId": language.lsp_id_for(uri), "version": 1, "text": text}}),
                    )
                    .await?;
                let mut version = 1;
                if self.resend_on_open {
                    version = 2;
                    self.client
                        .notify(
                            "textDocument/didChange",
                            json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": text}]}),
                        )
                        .await?;
                }
                docs.insert(uri.to_owned(), (version, text.to_owned(), before));
                true
            }
        };
        drop(docs);
        if first && let Some(loading) = self.loading_text {
            let deadline = tokio::time::Instant::now() + wait;
            while tokio::time::Instant::now() < deadline {
                let probe = self
                    .client
                    .request(
                        "textDocument/hover",
                        json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}}),
                    )
                    .await?;
                if !probe.to_string().contains(loading) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        // The first file opened in a server that lists the project's files first: its
        // diagnostics come after the listing (SPEC-0042 v14 behaviour 22).
        let listing = first && self.lists_files_first && !self.listed.load(Ordering::SeqCst);
        if self.diagnoses_first || listing {
            let list = self.published_after(uri, before, wait).await;
            let published = list.is_some();
            // jdtls: on opening, a list from its build may come first and the one with exact
            // ranges after it; later questions read the newest list anyway (SPEC-0076 v8
            // behaviour 10).
            if first
                && self.refines_diagnostics
                && list.is_some_and(|l| crate::insight::from_the_build(&l))
            {
                let first_list = self.publication(uri);
                let refine = crate::insight::REFINE_WAIT.min(wait);
                self.published_after(uri, first_list, refine).await;
            }
            self.listed.store(true, Ordering::SeqCst);
            // bash-language-server has meanwhile read the saved file over the opened text.
            if published && self.rereads_saved_files && !self.reread.swap(true, Ordering::SeqCst) {
                let saved =
                    crate::insight::uri_to_path(uri).and_then(|p| std::fs::read_to_string(p).ok());
                if saved.as_deref() != Some(text) {
                    let mut docs = self.docs.lock().await;
                    let after = self.publication(uri);
                    if let Some((version, _, sent_at)) = docs.get_mut(uri) {
                        *version += 1;
                        *sent_at = after;
                        let version = *version;
                        self.client
                            .notify(
                                "textDocument/didChange",
                                json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}),
                            )
                            .await?;
                    }
                    drop(docs);
                    self.published_after(uri, after, wait).await;
                }
            }
        }
        Ok(true)
    }

    /// The diagnostics published for `uri` after publication `after`, waiting at most `wait`; `None`
    /// when none came in time.
    pub(crate) async fn published_after(
        &self,
        uri: &str,
        after: u64,
        wait: Duration,
    ) -> Option<Vec<Value>> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let changed = self.changed.notified();
            if let Some((n, list)) = lock(&self.published).get(uri)
                && *n > after
            {
                return Some(list.clone());
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() || tokio::time::timeout(left, changed).await.is_err() {
                return None;
            }
        }
    }

    /// The diagnostics of `uri` published after publication `after`, as [`Self::published_after`];
    /// for a server that checks the project in the background the last ones published once that
    /// check has ended (`SPEC-0042` v16 behaviour 27), waiting at most `wait` in all.
    pub(crate) async fn diagnostics_after(
        &self,
        uri: &str,
        after: u64,
        wait: Duration,
    ) -> Option<Vec<Value>> {
        let deadline = tokio::time::Instant::now() + wait;
        let first = self.published_after(uri, after, wait).await;
        if self.publishes_twice
            && first.is_some()
            && lock(&self.publication_counts)
                .get(uri)
                .copied()
                .unwrap_or(0)
                < 2
        {
            let number = self.publication(uri);
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let refine = crate::insight::REFINE_WAIT.min(left);
            return self.published_after(uri, number, refine).await.or(first);
        }
        if !self.checks_in_background {
            return first;
        }
        while !lock(&self.checks).settled(std::time::Instant::now())
            && tokio::time::Instant::now() < deadline
            && !self.client.is_closed()
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        lock(&self.published)
            .get(uri)
            .filter(|(n, _)| *n > after)
            .map(|(_, list)| list.clone())
            .or(first)
    }

    /// The publication number when the server got the current text of `uri`: diagnostics published
    /// after it are about that text.
    pub(crate) async fn sent_at(&self, uri: &str) -> u64 {
        self.docs.lock().await.get(uri).map_or(0, |(_, _, n)| *n)
    }

    async fn close_document(&self, uri: &str) {
        if self.docs.lock().await.remove(uri).is_some() {
            // Opened again, it publishes twice again.
            lock(&self.publication_counts).remove(uri);
            let _ = self
                .client
                .notify(
                    "textDocument/didClose",
                    json!({"textDocument": {"uri": uri}}),
                )
                .await;
        }
    }

    fn take(&self, n: Notification) {
        *lock(&self.last_heard) = std::time::Instant::now();
        match n.method.as_str() {
            "textDocument/publishDiagnostics" => {
                let Some(uri) = n.params["uri"].as_str() else {
                    return;
                };
                if self.indexing_now.load(Ordering::SeqCst) {
                    return;
                }
                *lock(&self.publication_counts)
                    .entry(uri.to_owned())
                    .or_default() += 1;
                let list = n.params["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let number = self.publications.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some(path) = crate::insight::uri_to_path(uri) {
                    record(path, list.iter().map(diagnostic).collect());
                }
                lock(&self.published).insert(uri.to_owned(), (number, list));
            }
            "experimental/serverStatus" if n.params["quiescent"] == true => {
                self.ready.store(true, Ordering::SeqCst);
                lock(&self.checks)
                    .ready_at
                    .get_or_insert_with(std::time::Instant::now);
            }
            "language/status" if n.params["type"] == "ServiceReady" => {
                self.ready.store(true, Ordering::SeqCst);
            }
            // intelephense up to 1.12 says so instead of a progress (SPEC-0042 v19 behaviour 25).
            crate::lsp::INDEXING_STARTED => {
                self.indexing_begun.store(true, Ordering::SeqCst);
                self.indexing_now.store(true, Ordering::SeqCst);
                return;
            }
            crate::lsp::INDEXING_ENDED => {
                self.indexing_now.store(false, Ordering::SeqCst);
                self.ready.store(true, Ordering::SeqCst);
            }
            "$/progress" => {
                track_loading(&mut lock(&self.loading_work), &n.params);
                lock(&self.checks).take(&n.params);
                // Only the end of the indexing is news for those who wait.
                let ended = {
                    let mut indexing = lock(&self.indexing);
                    let ended = indexing.ended(&n.params);
                    if indexing.running() {
                        self.indexing_begun.store(true, Ordering::SeqCst);
                    }
                    ended
                };
                if !ended {
                    return;
                }
                self.ready.store(true, Ordering::SeqCst);
            }
            _ => return,
        }
        self.changed.notify_waiters();
    }

    /// Whether the server is still loading the project (`SPEC-0095` v10 behaviour 12): one that
    /// says when it is ready has not said so yet; another has announced loading work with
    /// `$/progress` and not ended it.
    fn loading(&self) -> bool {
        if self.reports_status {
            !self.ready.load(Ordering::SeqCst)
        } else {
            !lock(&self.loading_work).is_empty()
        }
    }

    async fn until_ready(&self, wait: Duration) {
        let deadline = tokio::time::Instant::now() + wait;
        while !self.ready.load(Ordering::SeqCst) {
            let changed = self.changed.notified();
            if self.ready.load(Ordering::SeqCst) {
                break;
            }
            let now = tokio::time::Instant::now();
            let mut until = deadline;
            // A server with its stored index begins no indexing and falls silent (SPEC-0042 v17
            // behaviour 25a).
            if self.skips_indexing_when_cached && !self.indexing_begun.load(Ordering::SeqCst) {
                let quiet_for = lock(&self.last_heard).elapsed();
                if quiet_for >= crate::lsp::INDEXING_SETTLE {
                    self.ready.store(true, Ordering::SeqCst);
                    break;
                }
                until = until.min(now + (crate::lsp::INDEXING_SETTLE - quiet_for));
            }
            if now >= deadline {
                break;
            }
            let _ = tokio::time::timeout(until - now, changed).await;
        }
    }
}

/// Follows a `$/progress` with `params` in `work`, the tokens of unfinished work that loads the
/// project: a `begin` whose title says so ([`crate::lsp::loads_project`]) adds its token, the
/// `end` removes it (`SPEC-0095` v10 behaviour 12).
/// How long after a server said it is ready its background check may begin; when none has begun
/// by then, none is waited for (`SPEC-0042` v16 behaviour 27).
const CHECK_START: Duration = Duration::from_secs(3);

/// The background checks of a server (`SPEC-0042` v16 behaviour 27).
#[derive(Debug, Default)]
struct Checks {
    /// The `$/progress` tokens of the checks that run.
    running: std::collections::HashSet<String>,
    /// Whether a check has ended.
    done: bool,
    /// When the server said it had loaded the workspace.
    ready_at: Option<std::time::Instant>,
}

impl Checks {
    /// Takes the parameters of a `$/progress`: rust-analyzer reports each run of `cargo check`
    /// under a token that starts with `rust-analyzer/flycheck`.
    fn take(&mut self, params: &Value) {
        let Some(token) = params["token"]
            .as_str()
            .filter(|t| t.starts_with("rust-analyzer/flycheck"))
        else {
            return;
        };
        match params["value"]["kind"].as_str() {
            Some("begin") => {
                self.running.insert(token.to_owned());
            }
            Some("end") => {
                self.running.remove(token);
                self.done = true;
            }
            _ => {}
        }
    }

    /// Whether the diagnostics are complete at `now`: no check runs, and one has ended or none
    /// began within [`CHECK_START`] after the server was ready.
    fn settled(&self, now: std::time::Instant) -> bool {
        self.running.is_empty()
            && (self.done
                || self
                    .ready_at
                    .is_some_and(|at| now.duration_since(at) >= CHECK_START))
    }
}

fn track_loading(work: &mut std::collections::HashSet<String>, params: &Value) {
    let token = params["token"].to_string();
    let value = &params["value"];
    match value["kind"].as_str() {
        Some("begin") if crate::lsp::loads_project(value["title"].as_str().unwrap_or_default()) => {
            work.insert(token);
        }
        Some("end") => {
            work.remove(&token);
        }
        _ => {}
    }
}

type Slot = Arc<Mutex<Option<Arc<Server>>>>;

/// The pool: its own runtime thread and the running servers by workspace and command.
pub(crate) struct Pool {
    handle: tokio::runtime::Handle,
    servers: StdMutex<HashMap<(PathBuf, PathBuf), Slot>>,
    /// The servers being started (until initialised and, if they report it, ready), with their
    /// program names.
    starting: StdMutex<HashMap<(PathBuf, PathBuf), &'static str>>,
    /// The servers that were started, to ask their state without waiting for their slot.
    started: StdMutex<HashMap<(PathBuf, PathBuf), std::sync::Weak<Server>>>,
}

/// Marks a server as starting until dropped.
struct Starting(&'static Pool, (PathBuf, PathBuf));

impl Drop for Starting {
    fn drop(&mut self) {
        lock(&self.0.starting).remove(&self.1);
    }
}

static POOL: OnceLock<Pool> = OnceLock::new();

/// The newest update number and, per file, the update that set its diagnostics and the diagnostics.
type Published = (u64, HashMap<PathBuf, (u64, Vec<Diagnostic>)>);

static DIAGNOSTICS: OnceLock<StdMutex<Published>> = OnceLock::new();

fn lock<T>(m: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn record(path: PathBuf, found: Vec<Diagnostic>) {
    let mut d = lock(DIAGNOSTICS.get_or_init(Default::default));
    d.0 += 1;
    let n = d.0;
    d.1.insert(path, (n, found));
}

/// Turns the pool on for this process: later language questions reuse running servers. Idempotent.
pub fn enable() {
    POOL.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("idecraft-lsp-pool".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("the language server pool needs a runtime");
                let _ = tx.send(rt.handle().clone());
                rt.block_on(std::future::pending::<()>());
            })
            .expect("the language server pool needs a thread");
        Pool {
            handle: rx.recv().expect("the pool thread sends its runtime"),
            servers: StdMutex::default(),
            starting: StdMutex::default(),
            started: StdMutex::default(),
        }
    });
}

/// Whether [`enable`] was called.
pub fn enabled() -> bool {
    POOL.get().is_some()
}

pub(crate) fn pool() -> Option<&'static Pool> {
    POOL.get()
}

impl Pool {
    /// Runs `future` on the pool's runtime, where its servers' connections live.
    pub(crate) async fn run<F>(&self, future: F) -> Result<F::Output, LspError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.handle
            .spawn(future)
            .await
            .map_err(|e| LspError::Closed(e.to_string()))
    }

    /// The running server for `root` and `server`, started (and initialised, and ready if it reports
    /// readiness) when there is none or the old one stopped.
    pub(crate) async fn server(
        &'static self,
        root: &Path,
        server: Resolved,
        wait: Duration,
    ) -> Result<Arc<Server>, LspError> {
        let slot = lock(&self.servers)
            .entry((root.to_path_buf(), server.command.clone()))
            .or_default()
            .clone();
        let root = root.to_path_buf();
        let key = (root.clone(), server.command.clone());
        self.run(async move {
            let mut slot = slot.lock().await;
            if let Some(s) = slot.as_ref()
                && !s.client.is_closed()
            {
                return Ok(Arc::clone(s));
            }
            lock(&self.starting).insert(key.clone(), server.candidate.program);
            let _starting = Starting(self, key.clone());
            let mut client = server.spawn(&root)?;
            client.timeout = wait;
            client.init_options = server.initialization_options(&root);
            client.set_settings(server.settings(&root));
            let capabilities = match client.initialize(&root).await {
                Ok(c) => c,
                Err(e) => {
                    client.shutdown().await;
                    return Err(e);
                }
            };
            let (held, mut notifications) = client.take_notifications();
            let s = Arc::new(Server {
                client,
                capabilities,
                diagnoses_first: server.candidate.diagnoses_first,
                refines_diagnostics: server.candidate.refines_diagnostics,
                publishes_twice: server.candidate.publishes_twice,
                publication_counts: StdMutex::default(),
                loading_text: server.candidate.loading_text,
                resend_on_open: server.candidate.resend_on_open,
                lists_files_first: server.candidate.lists_files_first,
                rereads_saved_files: server.candidate.rereads_saved_files,
                reread: AtomicBool::new(false),
                listed: AtomicBool::new(false),
                program: server.candidate.program,
                docs: Mutex::default(),
                published: StdMutex::default(),
                publications: AtomicU64::new(0),
                ready: AtomicBool::new(false),
                reports_status: server.candidate.reports_status,
                loading_work: StdMutex::default(),
                indexing: StdMutex::default(),
                skips_indexing_when_cached: server.candidate.skips_indexing_when_cached,
                indexing_begun: AtomicBool::new(false),
                indexing_now: AtomicBool::new(false),
                last_heard: StdMutex::new(std::time::Instant::now()),
                checks_in_background: server.candidate.program == "rust-analyzer" && !server.safe,
                checks: StdMutex::default(),
                changed: Notify::new(),
                started: std::time::Instant::now(),
            });
            for n in held {
                s.take(n);
            }
            lock(&self.started).insert(key, Arc::downgrade(&s));
            let pump = Arc::downgrade(&s);
            tokio::spawn(async move {
                while let Some(n) = notifications.recv().await {
                    match pump.upgrade() {
                        Some(s) => s.take(n),
                        None => break,
                    }
                }
            });
            if server.candidate.reports_status {
                s.until_ready(wait).await;
            }
            *slot = Some(Arc::clone(&s));
            Ok(s)
        })
        .await?
    }

    /// Brings `file` to `text` on its running server (see [`Server::sync`]).
    pub(crate) async fn sync(
        &'static self,
        server: &Arc<Server>,
        file: &Path,
        language: LanguageId,
        text: &str,
        wait: Duration,
    ) -> Result<bool, LspError> {
        let (s, uri, text) = (Arc::clone(server), file_uri(file), text.to_owned());
        self.run(async move { s.sync(&uri, language, &text, wait).await })
            .await?
    }
}

/// The program name of the language server of `file` in the project `root` when the pool is
/// starting it or it is still loading the project (`SPEC-0095` v10 behaviour 12): its answers are
/// not final yet. `None` for a server that is ready, that is not running, or with the pool off.
pub fn loading(root: &Path, file: &Path) -> Option<String> {
    let pool = pool()?;
    let language = LanguageId::detect(file)?;
    let of_language = |program: &str| {
        crate::servers::candidates(language)
            .iter()
            .any(|c| c.program == program)
    };
    if let Some(program) = lock(&pool.starting)
        .iter()
        .find(|((r, _), program)| r == root && of_language(program))
        .map(|(_, program)| *program)
    {
        return Some(program.to_owned());
    }
    let running: Vec<(PathBuf, PathBuf)> = lock(&pool.servers).keys().cloned().collect();
    lock(&pool.started)
        .iter()
        .filter(|(key, _)| key.0 == root && running.contains(key))
        .filter_map(|(_, s)| s.upgrade())
        .find(|s| of_language(s.program) && !s.client.is_closed() && s.loading())
        .map(|s| s.program.to_owned())
}

/// Waits at most `wait` until the server of `file` in `root` is neither starting nor loading
/// ([`loading`]); returns whether it is so.
pub async fn loaded_within(root: &Path, file: &Path, wait: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + wait;
    while loading(root, file).is_some() {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    true
}

/// Sends the editor's text of `file` to its server so diagnostics follow typing; starts the server
/// when needed. Does nothing for files without a language server or with the pool off.
pub async fn sync_text(root: &Path, file: &Path, text: &str) -> Result<(), LspError> {
    let Some(pool) = pool() else {
        return Ok(());
    };
    let language =
        LanguageId::detect(file).ok_or_else(|| LspError::NoServer(file.display().to_string()))?;
    let resolved = crate::servers::resolve_for(language, root)?;
    let wait = Duration::from_secs(60);
    let server = pool.server(root, resolved, wait).await?;
    pool.sync(&server, file, language, text, wait).await?;
    Ok(())
}

/// Closes `file` on the servers that have it open (the editor tab was closed).
pub async fn close_text(file: &Path) {
    let Some(pool) = pool() else {
        return;
    };
    let slots: Vec<Slot> = lock(&pool.servers).values().cloned().collect();
    let uri = file_uri(file);
    let _ = pool
        .run(async move {
            for slot in slots {
                if let Some(s) = slot.lock().await.clone() {
                    s.close_document(&uri).await;
                }
            }
        })
        .await;
    let mut d = lock(DIAGNOSTICS.get_or_init(Default::default));
    d.1.remove(file);
}

/// What the servers hear when `from` was moved to `to` (renamed files and folders): the
/// `workspace/didRenameFiles` the client announces, and the same as deleted and created through
/// `workspace/didChangeWatchedFiles`, so a server that indexes the disk (jdtls) knows the new
/// place (hand test 201: the moved class stayed unknown to jdtls and its import stayed an error).
pub fn moved_notifications(from: &Path, to: &Path) -> [(&'static str, Value); 2] {
    let (old, new) = (file_uri(from), file_uri(to));
    [
        (
            "workspace/didRenameFiles",
            json!({"files": [{"oldUri": old, "newUri": new}]}),
        ),
        (
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": old, "type": 3}, {"uri": new, "type": 1}]}),
        ),
    ]
}

/// Tells the running servers of `root` that `from` was moved to `to` ([`moved_notifications`])
/// and closes the old document on them; the editor opens the new one. Waits at most 5 s.
pub fn files_moved(root: &Path, from: &Path, to: &Path) {
    let Some(pool) = pool() else {
        return;
    };
    let slots: Vec<Slot> = lock(&pool.servers)
        .iter()
        .filter(|((r, _), _)| from.starts_with(r) || r.starts_with(root))
        .map(|(_, s)| s.clone())
        .collect();
    if slots.is_empty() {
        return;
    }
    let notifications = moved_notifications(from, to);
    let old = file_uri(from);
    let (tx, rx) = std::sync::mpsc::channel();
    pool.handle.spawn(async move {
        for slot in slots {
            let server = slot.lock().await.clone();
            if let Some(s) = server.filter(|s| !s.client.is_closed()) {
                s.close_document(&old).await;
                for (method, params) in &notifications {
                    let _ = s.client.notify(method, params.clone()).await;
                }
            }
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_secs(5));
    lock(DIAGNOSTICS.get_or_init(Default::default))
        .1
        .remove(from);
}

/// The changes collected for one project until their flush, and the order of flushes.
#[derive(Default)]
struct DiskQueue {
    pending: crate::watched_files::Pending,
    /// Whether a flush is waiting for its time.
    scheduled: bool,
    /// Held while a flush sends, so two flushes never overtake each other.
    sending: Arc<Mutex<()>>,
}

static DISK: OnceLock<StdMutex<HashMap<PathBuf, DiskQueue>>> = OnceLock::new();
static FORWARDED: AtomicU64 = AtomicU64::new(0);

/// Whether the workspace folder of a server and the project `root` lie in one another, however
/// the two are spelled (`/tmp` and `/private/tmp`).
fn concerns(folder: &Path, root: &Path) -> bool {
    if folder.starts_with(root) || root.starts_with(folder) {
        return true;
    }
    let (folder, root) = (canonical(folder), canonical(root));
    folder.starts_with(&root) || root.starts_with(&folder)
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Whether a language server of the project `root` runs or is starting, so that looking at the
/// disk for changes is worth it (`SPEC-0042` v11 behaviour 20).
pub fn watches(root: &Path) -> bool {
    pool().is_some_and(|p| {
        lock(&p.servers).iter().any(|((folder, _), slot)| {
            concerns(folder, root)
                && slot
                    .try_lock()
                    .map(|s| s.as_ref().is_some_and(|s| !s.client.is_closed()))
                    .unwrap_or(true)
        })
    })
}

/// How many file changes were told to servers with `workspace/didChangeWatchedFiles` through
/// [`disk_changed`] since the start (one per server and file).
pub fn forwarded_events() -> u64 {
    FORWARDED.load(Ordering::SeqCst)
}

/// Files below `root` were created, changed or deleted on disk by something other than an editor
/// buffer (`SPEC-0042` v11 behaviour 20). The paths may be relative to `root`. Paths outside the
/// project, behind links or in excluded folders are dropped; the rest is collected, per path only
/// the net effect, and told to the running servers that care once nothing new came for
/// [`crate::watched_files::QUIET`]. Returns at once.
pub fn disk_changed(root: &Path, changes: Vec<(PathBuf, crate::watched_files::Kind)>) {
    use crate::watched_files::{inside, told};
    let Some(pool) = pool() else {
        return;
    };
    let now = std::time::Instant::now();
    let kept: Vec<_> = changes
        .into_iter()
        .filter_map(|(path, kind)| {
            let relative = inside(root, &path)?;
            told(root, &relative).then_some((relative, kind))
        })
        .collect();
    {
        let mut queues = lock(DISK.get_or_init(Default::default));
        let queue = queues.entry(root.to_path_buf()).or_default();
        for (path, kind) in kept {
            queue.pending.add(path, kind, now);
        }
        if queue.pending.is_empty() || queue.scheduled {
            return;
        }
        queue.scheduled = true;
    }
    let root = root.to_path_buf();
    pool.handle.spawn(async move {
        let queues = DISK.get_or_init(Default::default);
        loop {
            let due = lock(queues).get(&root).and_then(|q| q.pending.due());
            match due {
                Some(due) if due > std::time::Instant::now() => {
                    tokio::time::sleep_until(due.into()).await;
                }
                _ => break,
            }
        }
        let sending = lock(queues).get(&root).map(|q| Arc::clone(&q.sending));
        let _turn = match &sending {
            Some(s) => Some(s.lock().await),
            None => None,
        };
        // Taken only now: what came while an earlier flush was sending goes along.
        let changes = lock(queues)
            .get_mut(&root)
            .map(|q| {
                q.scheduled = false;
                q.pending.take()
            })
            .unwrap_or_default();
        tell_disk_changes(pool, &root, &changes).await;
    });
}

/// [`disk_changed`] for the changes of a snapshot comparison (`SPEC-0026`).
pub fn disk_changes(root: &Path, changes: &[idecraft_core::FileChange]) {
    disk_changed(root, crate::watched_files::kinds(changes));
}

/// Sends `changes` (relative to `root`) to every running server of the project that cares.
async fn tell_disk_changes(
    pool: &'static Pool,
    root: &Path,
    changes: &[(PathBuf, crate::watched_files::Kind)],
) {
    use crate::watched_files::{PAUSE, RESTART_ABOVE, interested, notifications};
    if changes.is_empty() {
        return;
    }
    let slots: Vec<((PathBuf, PathBuf), Slot)> = lock(&pool.servers)
        .iter()
        .filter(|((folder, _), _)| concerns(folder, root))
        .map(|(key, slot)| (key.clone(), slot.clone()))
        .collect();
    for (key, slot) in slots {
        // A server that is starting is waited for: it may have read the disk before the change.
        let server = slot.lock().await.clone();
        let Some(server) = server.filter(|s| !s.client.is_closed()) else {
            continue;
        };
        // Relative to the server's workspace folder, so the URIs are spelled as the server
        // knows its folder.
        let (folder, project) = (canonical(&key.0), canonical(root));
        let mine: Vec<_> = changes
            .iter()
            .filter(|(path, _)| interested(server.program, path))
            .filter_map(|(path, kind)| {
                let within = project.join(path).strip_prefix(&folder).ok()?.to_path_buf();
                Some((within, *kind))
            })
            .collect();
        if mine.is_empty() {
            continue;
        }
        if mine.len() > RESTART_ABOVE {
            // Too much to tell: the server reads the disk afresh when the next question starts it.
            let dropped = lock(&pool.servers).remove(&key);
            stop_slots(pool, dropped.into_iter().collect(), false);
            continue;
        }
        let mut told = 0;
        for (i, params) in notifications(&key.0, &mine).into_iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(PAUSE).await;
            }
            let n = params["changes"].as_array().map_or(0, Vec::len);
            if server
                .client
                .notify("workspace/didChangeWatchedFiles", params)
                .await
                .is_err()
            {
                break;
            }
            told += n as u64;
        }
        FORWARDED.fetch_add(told, Ordering::SeqCst);
    }
}

/// The diagnostics published since update `since`: the newest update number and, per file, its
/// current diagnostics. Start with 0.
pub fn diagnostics_since(since: u64) -> (u64, Vec<(PathBuf, Vec<Diagnostic>)>) {
    let d = lock(DIAGNOSTICS.get_or_init(Default::default));
    let changed =
        d.1.iter()
            .filter(|(_, (n, _))| *n > since)
            .map(|(p, (_, list))| (p.clone(), list.clone()))
            .collect();
    (d.0, changed)
}

/// Number of running servers (for tests and the MCP status).
pub fn running() -> usize {
    pool().map_or(0, |p| {
        lock(&p.servers)
            .values()
            .filter(|slot| {
                slot.try_lock()
                    .map(|s| s.as_ref().is_some_and(|s| !s.client.is_closed()))
                    .unwrap_or(true)
            })
            .count()
    })
}

/// Gives the running servers of `root` whose program is named `program` new `settings` with
/// `workspace/didChangeConfiguration` (`SPEC-0170` behaviour 4); returns how many got them.
/// Waits at most 5 s.
pub fn change_settings(root: &Path, program: &str, settings: Value) -> usize {
    let Some(pool) = pool() else {
        return 0;
    };
    let slots: Vec<Slot> = lock(&pool.servers)
        .iter()
        .filter(|((r, command), _)| r == root && command.file_name().is_some_and(|n| n == program))
        .map(|(_, s)| s.clone())
        .collect();
    if slots.is_empty() {
        return 0;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    pool.handle.spawn(async move {
        let mut changed = 0;
        for slot in slots {
            let server = slot.lock().await.clone();
            if let Some(s) = server.filter(|s| !s.client.is_closed())
                && s.client.change_settings(settings.clone()).await.is_ok()
            {
                changed += 1;
            }
        }
        let _ = tx.send(changed);
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap_or(0)
}

/// Shuts all servers down (`shutdown`, `exit`, kill after 2 s); call when the IDE quits.
pub fn shutdown() {
    let Some(pool) = pool() else {
        return;
    };
    let slots: Vec<Slot> = lock(&pool.servers).drain().map(|(_, s)| s).collect();
    stop_slots(pool, slots, true);
}

/// Shuts down the servers of workspaces at or below `root`, so the next question starts them
/// again — with full options after the project was trusted, in safe mode after trust was taken
/// away (`SPEC-0278` behaviour 7). Returns how many slots were dropped.
pub fn restart_root(root: &Path) -> usize {
    let Some(pool) = pool() else {
        return 0;
    };
    let slots: Vec<Slot> = {
        let mut servers = lock(&pool.servers);
        let keys: Vec<_> = servers
            .keys()
            .filter(|(r, _)| r.starts_with(root))
            .cloned()
            .collect();
        keys.iter().filter_map(|k| servers.remove(k)).collect()
    };
    let n = slots.len();
    stop_slots(pool, slots, false);
    n
}

/// Shuts down the servers of `language` for the workspace `root` (every candidate program), so
/// the next question starts them again and they read the environment afresh — after packages
/// were installed into the project's Python environment (`SPEC-0117` v3 behaviour 7, D25).
/// Returns how many slots were dropped.
pub fn restart_language(root: &Path, language: LanguageId) -> usize {
    let Some(pool) = pool() else {
        return 0;
    };
    let programs: Vec<String> = crate::servers::candidates(language)
        .iter()
        .map(|c| c.program.to_owned())
        .collect();
    let slots: Vec<Slot> = {
        let mut servers = lock(&pool.servers);
        let keys: Vec<_> = servers
            .keys()
            .filter(|(r, command)| {
                r == root
                    && command
                        .file_name()
                        .is_some_and(|n| programs.iter().any(|p| n.to_string_lossy() == *p))
            })
            .cloned()
            .collect();
        keys.iter().filter_map(|k| servers.remove(k)).collect()
    };
    let n = slots.len();
    stop_slots(pool, slots, false);
    n
}

/// Stops the servers in `slots`; with `wait` until they ended (at most 3 s), else in the background.
fn stop_slots(pool: &'static Pool, slots: Vec<Slot>, wait: bool) {
    if slots.is_empty() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    pool.handle.spawn(async move {
        let mut stops = Vec::new();
        for slot in slots {
            if let Some(s) = slot.lock().await.take() {
                stops.push(tokio::spawn(async move {
                    let _ = s.client.request("shutdown", Value::Null).await;
                    let _ = s.client.notify("exit", Value::Null).await;
                }));
            }
        }
        for stop in stops {
            let _ = tokio::time::timeout(Duration::from_secs(2), stop).await;
        }
        let _ = tx.send(());
    });
    if wait {
        let _ = rx.recv_timeout(Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    /// SPEC-0042/AK-28: the diagnostics of a server that checks in the background are complete
    /// when its check has ended, or when none began in time.
    #[test]
    fn background_checks_settle() {
        use super::{CHECK_START, Checks};
        use serde_json::json;
        let now = std::time::Instant::now();
        let mut checks = Checks::default();
        assert!(!checks.settled(now), "not ready yet");
        checks.ready_at = Some(now);
        assert!(!checks.settled(now), "a check may still begin");
        assert!(checks.settled(now + CHECK_START), "none began in time");
        let token = "rust-analyzer/flycheck/0";
        checks.take(&json!({"token": token, "value": {"kind": "begin", "title": "cargo check"}}));
        assert!(!checks.settled(now + CHECK_START), "a check runs");
        // Other work of the server does not count.
        checks.take(&json!({"token": "rustAnalyzer/Indexing", "value": {"kind": "end"}}));
        assert!(!checks.settled(now + CHECK_START));
        checks.take(&json!({"token": token, "value": {"kind": "end"}}));
        assert!(checks.settled(now), "the check has ended");
    }

    /// SPEC-0095 v10 AK-17: loading work announced with `$/progress` counts until its end; a
    /// check on a loaded project does not; without the pool no server is loading.
    #[test]
    fn progress_that_loads_the_project_is_followed_to_its_end() {
        use serde_json::json;
        let mut work = std::collections::HashSet::new();
        let progress =
            |token: &str, value: serde_json::Value| json!({"token": token, "value": value});
        super::track_loading(
            &mut work,
            &progress("a", json!({"kind": "begin", "title": "Loading packages"})),
        );
        super::track_loading(
            &mut work,
            &progress("b", json!({"kind": "begin", "title": "cargo check"})),
        );
        super::track_loading(
            &mut work,
            &progress("a", json!({"kind": "report", "message": "3/7"})),
        );
        assert_eq!(work.len(), 1);
        super::track_loading(&mut work, &progress("b", json!({"kind": "end"})));
        assert_eq!(work.len(), 1);
        super::track_loading(&mut work, &progress("a", json!({"kind": "end"})));
        assert!(work.is_empty());
        for title in [
            "Indexing",
            "Importing Maven project(s)",
            "Initializing JS/TS language features",
            "Setting up workspace",
        ] {
            assert!(crate::lsp::loads_project(title), "{title}");
        }
        for title in ["cargo check", "Building", "Formatting"] {
            assert!(!crate::lsp::loads_project(title), "{title}");
        }
    }

    // Hand test 201: a move is announced as a rename and as a delete plus a create.
    #[test]
    fn a_move_is_a_rename_and_a_watched_delete_and_create() {
        let [(rename, r), (watched, w)] = super::moved_notifications(
            std::path::Path::new("/p/src/com/acme/A.java"),
            std::path::Path::new("/p/src/com/acme/util/A.java"),
        );
        assert_eq!(rename, "workspace/didRenameFiles");
        assert_eq!(r["files"][0]["oldUri"], "file:///p/src/com/acme/A.java");
        assert_eq!(
            r["files"][0]["newUri"],
            "file:///p/src/com/acme/util/A.java"
        );
        assert_eq!(watched, "workspace/didChangeWatchedFiles");
        assert_eq!(w["changes"][0]["type"], 3);
        assert_eq!(w["changes"][1]["uri"], "file:///p/src/com/acme/util/A.java");
        assert_eq!(w["changes"][1]["type"], 1);
    }
}
