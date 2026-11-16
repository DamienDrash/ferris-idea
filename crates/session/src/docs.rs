//! The backend's documents (`ADR-0026` §5, `SPEC-0317` behaviours 8–10): the backend owns the
//! text of every open file, orders all edits (server-sequenced operational transformation),
//! applies each `(client, cseq)` once, answers with the new revision and its hash, saves through
//! a temporary file with a change check and mirrors every document into the document registry of
//! `SPEC-0220`, so agents on the host read the same text.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use idecraft_editor::TextSnapshot;
use idecraft_editor::documents::DocumentRegistry;
use ropey::Rope;
use serde_json::{Value, json};

use crate::ot::{Op, TextOp, text_hash};
use crate::paths::resolve;
use crate::rpc::{ErrorCode, RpcError};

/// Operations kept per document for transforming late edits.
pub const LOG_LIMIT: usize = 10_000;

/// The largest file opened as a document (larger ones are refused as in the local editor).
pub const MAX_DOCUMENT: u64 = 32 << 20;

/// What the last edit of a client changed (for answering a resent edit again).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Applied {
    cseq: u64,
    revision: u64,
    hash: String,
}

/// What the file on disk looked like when it was last loaded or saved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiskState {
    len: u64,
    modified: Option<SystemTime>,
    hash: String,
}

fn disk_state(path: &Path) -> Option<(DiskState, Vec<u8>)> {
    let meta = std::fs::metadata(path).ok()?;
    let bytes = std::fs::read(path).ok()?;
    let hash = crate::hello::hex(&<sha2::Sha256 as sha2::Digest>::digest(&bytes)[..8]);
    Some((
        DiskState {
            len: meta.len(),
            modified: meta.modified().ok(),
            hash,
        },
        bytes,
    ))
}

struct Doc {
    path: PathBuf,
    text: Rope,
    revision: u64,
    modified: bool,
    /// Applied edits: revision, edit, author, the author's `cseq` (0 for the disk).
    log: VecDeque<(u64, TextOp, String, u64)>,
    applied: HashMap<String, Applied>,
    disk: Option<DiskState>,
    clients: BTreeSet<String>,
    crlf: bool,
    /// The text and revision last known to equal the file (`SPEC-0323` behaviour 5).
    synced: Rope,
    synced_rev: u64,
    /// The file is gone from disk; the document lives on in memory (`SPEC-0323` behaviour 6).
    deleted: bool,
}

/// A document as `doc.open` and `doc.snapshot` return it.
#[derive(Debug, Clone, PartialEq)]
pub struct DocInfo {
    /// Id: the path relative to the project root.
    pub doc: String,
    /// Current revision (from 1).
    pub revision: u64,
    /// Unsaved changes.
    pub modified: bool,
    /// `lf` or `crlf`.
    pub line_ending: &'static str,
    /// The text.
    pub text: String,
    /// [`text_hash`] of the text.
    pub hash: String,
}

impl DocInfo {
    /// The JSON answer (text inline).
    pub fn to_json(&self) -> Value {
        json!({
            "doc": self.doc, "revision": self.revision, "modified": self.modified,
            "line_ending": self.line_ending, "text": self.text, "hash": self.hash,
        })
    }
}

/// The result of an edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    /// The revision after the edit.
    pub revision: u64,
    /// The hash of the text at that revision.
    pub hash: String,
    /// The ranges as applied (transformed), relative to the previous revision.
    pub ops: Vec<Op>,
    /// The edit was applied before; nothing changed now.
    pub duplicate: bool,
    /// The edit changed the text (an edit can vanish against a concurrent one).
    pub changed: bool,
}

/// A change of an open document's file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskEvent {
    /// The document had no unsaved changes and follows the disk: a new revision.
    Reloaded {
        /// Document id.
        doc: String,
        /// New revision.
        revision: u64,
        /// The change, relative to the previous revision.
        ops: Vec<Op>,
        /// Hash of the new text.
        hash: String,
    },
    /// The document has unsaved changes; the window decides (`doc.disk_changed`).
    Changed {
        /// Document id.
        doc: String,
        /// The file is gone.
        deleted: bool,
    },
}

/// The client holding a room's documents open while no person has them open (`SPEC-0323`).
pub const ROOM_CLIENT: &str = "room";

/// A change of the document text as the store applied it (`SPEC-0323` behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    /// Document id.
    pub doc: String,
    /// The revision after the change.
    pub revision: u64,
    /// The change.
    pub op: TextOp,
    /// What takes it back at that revision.
    pub inverse: TextOp,
    /// Hash of the text after the change.
    pub hash: String,
}

/// A change of the file tree as the store applied or saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// `created`, `created-dir`, `renamed`, `deleted`, `changed`.
    pub kind: &'static str,
    /// The path now (for a deletion, the path it had).
    pub path: String,
    /// The former path of a rename.
    pub from: Option<String>,
    /// The stored contents of a deleted file (`SPEC-0323` behaviour 9).
    pub blob: Option<String>,
}

/// What a room is told, in the store's order (`SPEC-0323`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreEvent {
    /// A document changed.
    Text(TextChange),
    /// The file tree changed.
    File(FileChange),
}

/// An ordered change with its author and, for a change set, its group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRecord {
    /// The session's client id, a person id for a change set, or `disk`.
    pub author: String,
    /// The change set, when it belongs to one.
    pub group: Option<u64>,
    /// The change.
    pub event: StoreEvent,
}

/// Where a room receives the store's changes; called with the store locked.
pub type StoreSink = Arc<dyn Fn(StoreRecord) + Send + Sync>;

/// One member of a change set (`room.apply`, `SPEC-0323` behaviour 4). Edits name files as they
/// are before the set; the file operations run first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SetChange {
    /// Change the text of a file.
    Edit {
        /// The file.
        path: String,
        /// The revision the ranges refer to; the current one when absent.
        #[serde(default)]
        base: Option<u64>,
        /// The ranges.
        ops: Vec<Op>,
    },
    /// Create a file or folder.
    Create {
        /// The new path.
        path: String,
        /// A folder rather than a file.
        #[serde(default)]
        dir: bool,
        /// The contents of a new file.
        #[serde(default)]
        text: Option<String>,
        /// Stored contents of a file brought back (`room.undo`, `room.restore`).
        #[serde(default)]
        blob: Option<String>,
    },
    /// Rename or move.
    Rename {
        /// The path now.
        from: String,
        /// The new path.
        to: String,
    },
    /// Delete a file or folder (with its contents).
    Delete {
        /// The path.
        path: String,
    },
}

/// What a change set did: for each changed document its outcome, for each moved one its names.
#[derive(Debug, Default)]
pub struct SetOutcome {
    /// Documents changed by edits: id and outcome.
    pub edits: Vec<(String, EditOutcome)>,
    /// Documents that moved: old id, new id.
    pub moved: Vec<(String, String)>,
}

/// The most files one deletion stores for being brought back (`SPEC-0323` behaviour 9).
const MAX_STASH: usize = 5000;

/// The documents of one workspace.
pub struct DocStore {
    root: PathBuf,
    docs: BTreeMap<String, Doc>,
    registry: DocumentRegistry,
    log_limit: usize,
    /// A room is open: changes are recorded and disk changes merged (`SPEC-0323`).
    room: Option<StoreSink>,
    /// Where deleted files are kept for undo (the room's blob folder).
    blobs: Option<PathBuf>,
    /// Former ids of moved documents, so a late edit still arrives (`SPEC-0323` behaviour 6).
    aliases: HashMap<String, String>,
    /// The change set being applied; set by [`DocStore::apply_set`].
    group: Option<u64>,
}

fn not_open(doc: &str) -> RpcError {
    RpcError::new(ErrorCode::NotFound, format!("{doc} is not open")).hint("open it with doc.open")
}

impl DocStore {
    /// The documents of the workspace at canonical `root`, mirrored into `registry`.
    pub fn new(root: PathBuf, registry: DocumentRegistry) -> Self {
        Self {
            root,
            docs: BTreeMap::new(),
            registry,
            log_limit: LOG_LIMIT,
            room: None,
            blobs: None,
            aliases: HashMap::new(),
            group: None,
        }
    }

    /// The same store keeping at most `n` operations per document (tests).
    pub fn with_log_limit(mut self, n: usize) -> Self {
        self.log_limit = n.max(1);
        self
    }

    /// The project root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Switches to room mode (`SPEC-0323`): `sink` hears every change, disk changes merge,
    /// documents stay open and deleted files are stored in `blobs`.
    pub fn enter_room(&mut self, sink: StoreSink, blobs: PathBuf) {
        self.room = Some(sink);
        self.blobs = Some(blobs);
        for d in self.docs.values_mut() {
            d.clients.insert(ROOM_CLIENT.to_owned());
        }
    }

    /// Whether a room is open.
    pub fn in_room(&self) -> bool {
        self.room.is_some()
    }

    fn emit(&self, author: &str, event: StoreEvent) {
        if let Some(sink) = &self.room {
            sink(StoreRecord {
                author: author.to_owned(),
                group: self.group,
                event,
            });
        }
    }

    /// The current id of `doc`, which may have been moved.
    pub fn canonical(&self, doc: &str) -> String {
        if self.docs.contains_key(doc) {
            return doc.to_owned();
        }
        self.aliases
            .get(doc)
            .cloned()
            .unwrap_or_else(|| doc.to_owned())
    }

    /// The edit of `ops` made on revision `base`, transformed to the current text.
    fn merged(d: &Doc, doc: &str, base: u64, ops: &[Op]) -> Result<TextOp, RpcError> {
        if base > d.revision || base == 0 {
            return Err(RpcError::new(
                ErrorCode::Invalid,
                format!(
                    "revision {base} of {doc} does not exist (now {})",
                    d.revision
                ),
            ));
        }
        let oldest = d.log.front().map_or(d.revision, |(r, ..)| r - 1);
        if base < oldest {
            return Err(RpcError::new(
                ErrorCode::StaleRevision,
                format!("revision {base} of {doc} is too old to merge (oldest {oldest})"),
            )
            .hint("read doc.snapshot and edit on its revision"));
        }
        let base_len = d
            .log
            .iter()
            .find(|(r, ..)| *r == base + 1)
            .map_or(d.text.len_chars(), |(_, op, ..)| op.base_len());
        let mut op = TextOp::from_ops(base_len, ops)?;
        for (_, logged, ..) in d.log.iter().filter(|(r, ..)| *r > base) {
            op = TextOp::transform(logged, &op)?.1;
        }
        Ok(op)
    }

    /// Applies an edit already merged to the current text and records it.
    fn commit(
        &mut self,
        doc: &str,
        op: TextOp,
        author: &str,
        cseq: u64,
        modified: bool,
    ) -> Result<(), RpcError> {
        let limit = self.log_limit;
        let in_room = self.room.is_some();
        let d = self.docs.get_mut(doc).ok_or_else(|| not_open(doc))?;
        let inverse = in_room.then(|| op.invert(&d.text));
        op.apply(&mut d.text)?;
        d.revision += 1;
        d.modified = d.modified || modified;
        d.log
            .push_back((d.revision, op.clone(), author.to_owned(), cseq));
        while d.log.len() > limit {
            d.log.pop_front();
        }
        self.registry.publish(
            &DocumentRegistry::key(&d.path),
            TextSnapshot::from_rope(d.text.clone()),
            d.modified,
        );
        if let Some(inverse) = inverse {
            let (revision, hash) = (d.revision, text_hash(&d.text));
            self.emit(
                author,
                StoreEvent::Text(TextChange {
                    doc: doc.to_owned(),
                    revision,
                    op,
                    inverse,
                    hash,
                }),
            );
        }
        Ok(())
    }

    /// Applies an edit on revision `base` for `author` (no client or sequence number): an undo,
    /// a restore or a member of a change set (`SPEC-0323`).
    pub fn ordered_edit(
        &mut self,
        doc: &str,
        base: u64,
        author: &str,
        ops: &[Op],
    ) -> Result<EditOutcome, RpcError> {
        let id = self.canonical(doc);
        let d = self.docs.get(&id).ok_or_else(|| not_open(&id))?;
        let op = Self::merged(d, &id, base, ops)?;
        let changed = !op.is_noop();
        let list = if changed { op.to_ops() } else { Vec::new() };
        if changed {
            self.commit(&id, op, author, 0, true)?;
        }
        let d = self.docs.get(&id).ok_or_else(|| not_open(&id))?;
        Ok(EditOutcome {
            revision: d.revision,
            hash: text_hash(&d.text),
            ops: list,
            duplicate: false,
            changed,
        })
    }

    fn stash(&self, full: &Path, path: &str) -> Result<String, RpcError> {
        let Some(dir) = &self.blobs else {
            return Err(RpcError::new(ErrorCode::Unavailable, "no room is open"));
        };
        let bytes = std::fs::read(full)
            .map_err(|e| RpcError::new(ErrorCode::Denied, format!("{path}: {e}")))?;
        let name = crate::hello::hex(&<sha2::Sha256 as sha2::Digest>::digest(&bytes));
        let blob = dir.join(&name);
        if !blob.exists() {
            crate::reattach::write_private(&blob, &bytes)
                .map_err(|e| RpcError::new(ErrorCode::Internal, format!("{path}: {e}")))?;
        }
        Ok(name)
    }

    /// The contents kept in the room for `name`.
    fn blob_bytes(&self, name: &str) -> Result<Vec<u8>, RpcError> {
        let bad = || RpcError::new(ErrorCode::Invalid, format!("no stored contents {name}"));
        if name.len() != 64 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let dir = self.blobs.as_ref().ok_or_else(bad)?;
        std::fs::read(dir.join(name)).map_err(|_| bad())
    }

    fn check_edit(&self, change: &SetChange) -> Result<(), RpcError> {
        let SetChange::Edit { path, base, ops } = change else {
            return Ok(());
        };
        let id = self.canonical(path);
        let d = self.docs.get(&id).ok_or_else(|| not_open(&id))?;
        Self::merged(d, &id, base.unwrap_or(d.revision), ops).map(drop)
    }

    /// Checks the whole set and applies it as one step, for `author` in `group`; if anything is
    /// invalid nothing changes, and a file-system failure undoes the file operations already done
    /// (`SPEC-0323` behaviour 4).
    pub fn apply_set(
        &mut self,
        author: &str,
        group: u64,
        changes: &[SetChange],
    ) -> Result<SetOutcome, RpcError> {
        for c in changes {
            if let SetChange::Edit { path, .. } = c {
                let id = self.canonical(path);
                if !self.docs.contains_key(&id) {
                    self.open(&id, ROOM_CLIENT)?;
                }
            }
        }
        for c in changes {
            self.check_edit(c)?;
        }
        self.check_file_ops(changes)?;
        self.group = Some(group);
        let result = self.run_set(author, changes);
        self.group = None;
        result
    }

    /// Whether each file operation can run on the tree as it is, in order.
    fn check_file_ops(&self, changes: &[SetChange]) -> Result<(), RpcError> {
        let mut gone = BTreeSet::new();
        let mut made = BTreeSet::new();
        let exists = |rel: &str, full: &Path, gone: &BTreeSet<String>, made: &BTreeSet<String>| {
            made.contains(rel) || (full.exists() && !gone.contains(rel))
        };
        let mut edited = BTreeSet::new();
        for c in changes {
            if let SetChange::Edit { path, .. } = c {
                edited.insert(self.canonical(path));
            }
        }
        for c in changes {
            match c {
                SetChange::Edit { .. } => {}
                SetChange::Create {
                    path, text, blob, ..
                } => {
                    let full = resolve(&self.root, path)?;
                    let rel = crate::paths::check_relative(path)?;
                    if exists(&rel, &full, &gone, &made) {
                        return Err(RpcError::new(ErrorCode::Conflict, format!("{rel} exists")));
                    }
                    if text.is_some() && blob.is_some() {
                        return Err(invalid_set("give either text or stored contents"));
                    }
                    if let Some(b) = blob {
                        self.blob_bytes(b)?;
                    }
                    gone.remove(&rel);
                    made.insert(rel);
                }
                SetChange::Rename { from, to } => {
                    let (a, b) = (resolve(&self.root, from)?, resolve(&self.root, to)?);
                    let (rf, rt) = (
                        crate::paths::check_relative(from)?,
                        crate::paths::check_relative(to)?,
                    );
                    if a == self.root {
                        return Err(invalid_set("the project folder itself"));
                    }
                    if !exists(&rf, &a, &gone, &made) {
                        return Err(RpcError::new(
                            ErrorCode::NotFound,
                            format!("{rf} not found"),
                        ));
                    }
                    if exists(&rt, &b, &gone, &made) {
                        return Err(RpcError::new(ErrorCode::Conflict, format!("{rt} exists")));
                    }
                    gone.insert(rf.clone());
                    made.remove(&rf);
                    made.insert(rt);
                }
                SetChange::Delete { path } => {
                    let full = resolve(&self.root, path)?;
                    let rel = crate::paths::check_relative(path)?;
                    if full == self.root {
                        return Err(invalid_set("the project folder itself"));
                    }
                    if !exists(&rel, &full, &gone, &made) {
                        return Err(RpcError::new(
                            ErrorCode::NotFound,
                            format!("{rel} not found"),
                        ));
                    }
                    if edited.contains(&rel) {
                        return Err(invalid_set(format!(
                            "{rel} is both edited and deleted in the same change"
                        )));
                    }
                    gone.insert(rel.clone());
                    made.remove(&rel);
                }
            }
        }
        Ok(())
    }

    fn run_set(&mut self, author: &str, changes: &[SetChange]) -> Result<SetOutcome, RpcError> {
        let mut out = SetOutcome::default();
        let mut undo: Vec<Box<dyn FnOnce()>> = Vec::new();
        let mut events = Vec::new();
        for c in changes {
            let done = self.run_file_op(c, &mut undo, &mut events, &mut out);
            if let Err(e) = done {
                for u in undo.into_iter().rev() {
                    u();
                }
                return Err(e);
            }
        }
        for e in events {
            self.emit(author, StoreEvent::File(e));
        }
        for c in changes {
            let SetChange::Edit { path, base, ops } = c else {
                continue;
            };
            let id = self.canonical(path);
            let base = match base {
                Some(b) => *b,
                None => self.docs.get(&id).ok_or_else(|| not_open(&id))?.revision,
            };
            let o = self.ordered_edit(&id, base, author, ops)?;
            out.edits.push((id, o));
        }
        Ok(out)
    }

    fn run_file_op(
        &mut self,
        c: &SetChange,
        undo: &mut Vec<Box<dyn FnOnce()>>,
        events: &mut Vec<FileChange>,
        out: &mut SetOutcome,
    ) -> Result<(), RpcError> {
        let failed = |path: &str, e: std::io::Error| {
            RpcError::new(ErrorCode::Internal, format!("{path}: {e}"))
        };
        match c {
            SetChange::Edit { .. } => {}
            SetChange::Create {
                path,
                dir,
                text,
                blob,
            } => {
                let full = resolve(&self.root, path)?;
                let rel = crate::paths::check_relative(path)?;
                if let Some(p) = full.parent() {
                    std::fs::create_dir_all(p).map_err(|e| failed(&rel, e))?;
                }
                if *dir {
                    std::fs::create_dir_all(&full).map_err(|e| failed(&rel, e))?;
                    let again = full.clone();
                    undo.push(Box::new(move || {
                        let _ = std::fs::remove_dir_all(&again);
                    }));
                    events.push(FileChange {
                        kind: "created-dir",
                        path: rel,
                        from: None,
                        blob: None,
                    });
                    return Ok(());
                }
                let bytes = match blob {
                    Some(b) => self.blob_bytes(b)?,
                    None => text.clone().unwrap_or_default().into_bytes(),
                };
                std::fs::write(&full, bytes).map_err(|e| failed(&rel, e))?;
                let again = full.clone();
                undo.push(Box::new(move || {
                    let _ = std::fs::remove_file(&again);
                }));
                self.revived(&rel);
                events.push(FileChange {
                    kind: "created",
                    path: rel,
                    from: None,
                    blob: None,
                });
            }
            SetChange::Rename { from, to } => {
                let a = resolve(&self.root, from)?;
                let b = resolve(&self.root, to)?;
                let (rf, rt) = (
                    crate::paths::check_relative(from)?,
                    crate::paths::check_relative(to)?,
                );
                if let Some(p) = b.parent() {
                    std::fs::create_dir_all(p).map_err(|e| failed(&rt, e))?;
                }
                std::fs::rename(&a, &b).map_err(|e| failed(&rf, e))?;
                let (ra, rb) = (a.clone(), b.clone());
                undo.push(Box::new(move || {
                    let _ = std::fs::rename(&rb, &ra);
                }));
                self.move_docs(&rf, &rt, &mut out.moved);
                events.push(FileChange {
                    kind: "renamed",
                    path: rt,
                    from: Some(rf),
                    blob: None,
                });
            }
            SetChange::Delete { path } => {
                let full = resolve(&self.root, path)?;
                let rel = crate::paths::check_relative(path)?;
                let files = walk_files(&full).map_err(|e| failed(&rel, e))?;
                if files.len() > MAX_STASH {
                    return Err(invalid_set(format!(
                        "{rel} holds more than {MAX_STASH} files; delete it from a terminal"
                    )));
                }
                let mut kept = Vec::new();
                for f in &files {
                    let name = crate::paths::relative(&self.root, f)
                        .ok_or_else(|| invalid_set("a file left the project"))?;
                    let blob = self.stash(f, &name)?;
                    kept.push((name, blob));
                }
                if full.is_dir() {
                    std::fs::remove_dir_all(&full)
                } else {
                    std::fs::remove_file(&full)
                }
                .map_err(|e| failed(&rel, e))?;
                let (root, list) = (self.root.clone(), kept.clone());
                let blobs = self.blobs.clone().unwrap_or_default();
                undo.push(Box::new(move || {
                    for (name, blob) in &list {
                        let to = root.join(name);
                        if let Some(p) = to.parent() {
                            let _ = std::fs::create_dir_all(p);
                        }
                        if let Ok(bytes) = std::fs::read(blobs.join(blob)) {
                            let _ = std::fs::write(to, bytes);
                        }
                    }
                }));
                let prefix = format!("{rel}/");
                for (id, d) in &mut self.docs {
                    if id == &rel || id.starts_with(&prefix) {
                        d.deleted = true;
                        d.disk = None;
                    }
                }
                for (name, blob) in kept {
                    events.push(FileChange {
                        kind: "deleted",
                        path: name,
                        from: None,
                        blob: Some(blob),
                    });
                }
            }
        }
        Ok(())
    }

    /// A file came back: its open document follows the disk again.
    fn revived(&mut self, id: &str) {
        if let Some(d) = self.docs.get_mut(id) {
            d.deleted = false;
            // Reads the file again, so the automatic save knows the disk state it writes over.
            let _ = self.disk_check(id);
        }
    }

    /// Moves open documents after a rename; old ids stay usable.
    fn move_docs(&mut self, from: &str, to: &str, moved: &mut Vec<(String, String)>) {
        let prefix = format!("{from}/");
        let hit: Vec<String> = self
            .docs
            .keys()
            .filter(|k| *k == from || k.starts_with(&prefix))
            .cloned()
            .collect();
        for old in hit {
            let new = format!("{to}{}", &old[from.len()..]);
            if let Some(mut d) = self.docs.remove(&old) {
                let key = DocumentRegistry::key(&d.path);
                self.registry.close(&key);
                d.path = self.root.join(&new);
                self.registry.open(
                    &DocumentRegistry::key(&d.path),
                    TextSnapshot::from_rope(d.text.clone()),
                    d.modified,
                );
                self.docs.insert(new.clone(), d);
                for v in self.aliases.values_mut() {
                    if *v == old {
                        v.clone_from(&new);
                    }
                }
                self.aliases.insert(old.clone(), new.clone());
                moved.push((old, new));
            }
        }
    }

    /// Writes every modified document of a room that is safe to write; a file changed underneath
    /// waits for the merge (`SPEC-0323` behaviour 6).
    pub fn autosave(&mut self) -> Vec<(String, u64)> {
        let ids: Vec<String> = self
            .docs
            .iter()
            .filter(|(_, d)| d.modified && !d.deleted)
            .map(|(k, _)| k.clone())
            .collect();
        let mut saved = Vec::new();
        for id in ids {
            let Some(rev) = self.docs.get(&id).map(|d| d.revision) else {
                continue;
            };
            if self.save(&id, rev, false).is_ok() && self.docs.get(&id).is_some_and(|d| !d.modified)
            {
                saved.push((id, rev));
            }
        }
        saved
    }

    /// Opens `path` to hold it for the room.
    pub fn open_for_room(&mut self, path: &str) -> Result<DocInfo, RpcError> {
        self.open(path, ROOM_CLIENT)
    }

    /// Records a change of the tree seen from outside (the watcher).
    pub fn note_external(&self, kind: &'static str, path: &str, from: Option<&str>) {
        self.emit(
            "disk",
            StoreEvent::File(FileChange {
                kind,
                path: path.to_owned(),
                from: from.map(str::to_owned),
                blob: None,
            }),
        );
    }

    /// Merges the new file contents of `doc` as an ordered edit (`SPEC-0323` behaviour 5).
    fn merge_disk(&mut self, doc: &str, state: DiskState, new: &str) -> Option<DiskEvent> {
        let d = self.docs.get_mut(doc)?;
        let current = d.text.to_string();
        let clean = d.synced_rev == d.revision;
        let oldest = d.log.front().map_or(d.revision, |(r, ..)| r - 1);
        let op = if clean || d.synced_rev < oldest {
            TextOp::diff(&current, new)
        } else {
            let mut op = TextOp::diff(&d.synced.to_string(), new);
            for (_, logged, ..) in d.log.iter().filter(|(r, ..)| *r > d.synced_rev) {
                op = TextOp::transform(logged, &op).ok()?.1;
            }
            op
        };
        d.disk = Some(state);
        d.deleted = false;
        let changed = !op.is_noop();
        if changed {
            let modified = d.modified;
            self.commit(doc, op.clone(), "disk", 0, modified).ok()?;
        }
        let d = self.docs.get_mut(doc)?;
        let hash = text_hash(&d.text);
        let revision = d.revision;
        if clean {
            d.synced = d.text.clone();
            d.synced_rev = d.revision;
            d.modified = false;
        } else {
            // Written back only over the very file just read (`SPEC-0323` v3 behaviour 5): if a
            // program is still writing it, the save is refused and the next change merges again,
            // instead of replacing what that program writes.
            let _ = self.save(doc, revision, false);
        }
        changed.then(|| DiskEvent::Reloaded {
            doc: doc.to_owned(),
            revision,
            ops: op.to_ops(),
            hash,
        })
    }

    fn info(id: &str, d: &Doc) -> DocInfo {
        DocInfo {
            doc: id.to_owned(),
            revision: d.revision,
            modified: d.modified,
            line_ending: if d.crlf { "crlf" } else { "lf" },
            text: d.text.to_string(),
            hash: text_hash(&d.text),
        }
    }

    /// Opens `path` for `client` (or adds the client to an open document).
    pub fn open(&mut self, path: &str, client: &str) -> Result<DocInfo, RpcError> {
        let full = resolve(&self.root, path)?;
        let id = crate::paths::relative(&self.root, &full)
            .ok_or_else(|| RpcError::new(ErrorCode::Invalid, "path outside the project"))?;
        if let Some(d) = self.docs.get_mut(&id) {
            d.clients.insert(client.to_owned());
            if self.room.is_some() {
                d.clients.insert(ROOM_CLIENT.to_owned());
            }
            return Ok(Self::info(&id, d));
        }
        let meta = std::fs::metadata(&full)
            .map_err(|e| RpcError::new(ErrorCode::NotFound, format!("{id}: {e}")))?;
        if !meta.is_file() {
            return Err(RpcError::new(
                ErrorCode::Invalid,
                format!("{id} is not a file"),
            ));
        }
        if meta.len() > MAX_DOCUMENT {
            return Err(RpcError::new(
                ErrorCode::TooLarge,
                format!("{id} has {} bytes, more than the editor opens", meta.len()),
            ));
        }
        let (disk, bytes) = disk_state(&full)
            .ok_or_else(|| RpcError::new(ErrorCode::Denied, format!("{id} cannot be read")))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| RpcError::new(ErrorCode::Invalid, format!("{id} is not UTF-8 text")))?;
        let rope = Rope::from_str(&text);
        let key = DocumentRegistry::key(&full);
        self.registry
            .open(&key, TextSnapshot::from_rope(rope.clone()), false);
        let doc = Doc {
            path: full,
            crlf: text.contains("\r\n"),
            text: rope,
            revision: 1,
            modified: false,
            log: VecDeque::new(),
            applied: HashMap::new(),
            disk: Some(disk),
            clients: BTreeSet::from([client.to_owned()]),
            synced: Rope::from_str(&text),
            synced_rev: 1,
            deleted: false,
        };
        let mut doc = doc;
        if self.room.is_some() {
            doc.clients.insert(ROOM_CLIENT.to_owned());
        }
        let info = Self::info(&id, &doc);
        self.docs.insert(id, doc);
        Ok(info)
    }

    /// The current state of `doc`.
    pub fn snapshot(&self, doc: &str) -> Result<DocInfo, RpcError> {
        let id = self.canonical(doc);
        self.docs
            .get(&id)
            .map(|d| Self::info(&id, d))
            .ok_or_else(|| not_open(doc))
    }

    /// Clients that have `doc` open.
    pub fn clients(&self, doc: &str) -> Vec<String> {
        self.docs
            .get(doc)
            .map(|d| d.clients.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Applies an edit of `client` with sequence `cseq` made on revision `base`.
    pub fn edit(
        &mut self,
        doc: &str,
        base: u64,
        client: &str,
        cseq: u64,
        ops: &[Op],
    ) -> Result<EditOutcome, RpcError> {
        let doc = &self.canonical(doc);
        let d = self.docs.get_mut(doc).ok_or_else(|| not_open(doc))?;
        if !d.clients.contains(client) {
            return Err(not_open(doc));
        }
        if let Some(last) = d.applied.get(client) {
            if cseq == last.cseq {
                return Ok(EditOutcome {
                    revision: last.revision,
                    hash: last.hash.clone(),
                    ops: Vec::new(),
                    duplicate: true,
                    changed: false,
                });
            }
            if cseq < last.cseq {
                return Err(RpcError::new(
                    ErrorCode::Invalid,
                    format!("edit {cseq} of {client} was applied before {}", last.cseq),
                ));
            }
        }
        let op = Self::merged(d, doc, base, ops)?;
        let changed = !op.is_noop();
        if changed {
            self.commit(doc, op.clone(), client, cseq, true)?;
        }
        let d = self.docs.get_mut(doc).ok_or_else(|| not_open(doc))?;
        let hash = text_hash(&d.text);
        d.applied.insert(
            client.to_owned(),
            Applied {
                cseq,
                revision: d.revision,
                hash: hash.clone(),
            },
        );
        Ok(EditOutcome {
            revision: d.revision,
            hash,
            ops: if changed { op.to_ops() } else { Vec::new() },
            duplicate: false,
            changed,
        })
    }

    /// Applies `ops` (on the current text) on the backend's own behalf as `author`: a new
    /// revision every client is told about, like an agent's edit (`SPEC-0318` behaviour 12).
    pub fn host_edit(
        &mut self,
        doc: &str,
        author: &str,
        ops: &[Op],
    ) -> Result<EditOutcome, RpcError> {
        let d = self.docs.get(doc).ok_or_else(|| not_open(doc))?;
        let op = TextOp::from_ops(d.text.len_chars(), ops)?;
        let changed = !op.is_noop();
        if changed {
            self.commit(doc, op.clone(), author, 0, true)?;
        }
        let d = self.docs.get(doc).ok_or_else(|| not_open(doc))?;
        Ok(EditOutcome {
            revision: d.revision,
            hash: text_hash(&d.text),
            ops: if changed { op.to_ops() } else { Vec::new() },
            duplicate: false,
            changed,
        })
    }

    /// Saves `doc` at `revision` through a temporary file renamed into place, keeping the file's
    /// mode and recording local history; refuses with `conflict` when the file changed on disk
    /// since it was loaded or saved, unless `overwrite`.
    pub fn save(&mut self, doc: &str, revision: u64, overwrite: bool) -> Result<u64, RpcError> {
        let root = self.root.clone();
        let in_room = self.room.is_some();
        let doc = &self.canonical(doc);
        let d = self.docs.get_mut(doc).ok_or_else(|| not_open(doc))?;
        if revision != d.revision {
            return Err(RpcError::new(
                ErrorCode::StaleRevision,
                format!("{doc} is at revision {}, not {revision}", d.revision),
            )
            .hint("save the current revision"));
        }
        let now = disk_state(&d.path).map(|(s, _)| s);
        if !overwrite && now.as_ref().map(|s| &s.hash) != d.disk.as_ref().map(|s| &s.hash) {
            // In a room the merge follows the file change; a save is never a question.
            if in_room {
                return Ok(d.revision);
            }
            return Err(RpcError::new(
                ErrorCode::Conflict,
                format!("{doc} changed on the host since it was loaded; nothing was written"),
            )
            .hint("compare, overwrite or reload"));
        }
        let mode = std::fs::metadata(&d.path).ok().map(|m| m.permissions());
        let mut name = d.path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".idecraft-save-{}", std::process::id()));
        let tmp = d.path.with_file_name(name);
        let text = d.text.to_string();
        let write = std::fs::write(&tmp, text.as_bytes())
            .and_then(|()| match &mode {
                Some(m) => std::fs::set_permissions(&tmp, m.clone()),
                None => Ok(()),
            })
            .and_then(|()| std::fs::rename(&tmp, &d.path));
        if let Err(e) = write {
            let _ = std::fs::remove_file(&tmp);
            return Err(RpcError::new(
                ErrorCode::Denied,
                format!("{doc} could not be written: {e}"),
            ));
        }
        d.disk = disk_state(&d.path).map(|(s, _)| s);
        d.modified = false;
        d.synced = d.text.clone();
        d.synced_rev = d.revision;
        self.registry.publish(
            &DocumentRegistry::key(&d.path),
            TextSnapshot::from_rope(d.text.clone()),
            false,
        );
        // A room's journal is the history (SPEC-0323 behaviour 24); autosaving must not flood it.
        if !in_room && let Ok(mut history) = idecraft_core::LocalHistory::open(&root) {
            let _ = history.record(doc);
        }
        Ok(d.revision)
    }

    /// `client` closed `doc`; the document goes with its last client.
    pub fn close(&mut self, doc: &str, client: &str) {
        let gone = match self.docs.get_mut(doc) {
            Some(d) => {
                d.clients.remove(client);
                d.clients.is_empty()
            }
            None => false,
        };
        if gone && let Some(d) = self.docs.remove(doc) {
            self.registry.close(&DocumentRegistry::key(&d.path));
        }
    }

    /// Closes every document of `client`.
    pub fn close_client(&mut self, client: &str) {
        let ids: Vec<String> = self
            .docs
            .iter()
            .filter(|(_, d)| d.clients.contains(client))
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            self.close(&id, client);
        }
    }

    /// Open documents with unsaved changes that `client` has open.
    pub fn unsaved(&self, client: &str) -> Vec<String> {
        self.docs
            .iter()
            .filter(|(_, d)| d.modified && d.clients.contains(client))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Ids of the open documents.
    pub fn ids(&self) -> Vec<String> {
        self.docs.keys().cloned().collect()
    }

    /// Looks at the file of `doc` on disk after a change event.
    pub fn disk_check(&mut self, doc: &str) -> Option<DiskEvent> {
        let d = self.docs.get_mut(doc)?;
        let now = disk_state(&d.path);
        let same = now.as_ref().map(|(s, _)| &s.hash) == d.disk.as_ref().map(|s| &s.hash);
        if same {
            return None;
        }
        let Some((state, bytes)) = now else {
            d.disk = None;
            if self.room.is_some() {
                d.deleted = true;
                return None;
            }
            return Some(DiskEvent::Changed {
                doc: doc.to_owned(),
                deleted: true,
            });
        };
        if self.room.is_some() {
            // A file that is not text keeps the document; the automatic save leaves it alone
            // because the disk state stays different.
            let text = String::from_utf8(bytes).ok()?;
            return self.merge_disk(doc, state, &text);
        }
        let text = match String::from_utf8(bytes) {
            Ok(t) if !d.modified => t,
            _ => {
                return Some(DiskEvent::Changed {
                    doc: doc.to_owned(),
                    deleted: false,
                });
            }
        };
        let op = TextOp::diff(&d.text.to_string(), &text);
        d.disk = Some(state);
        if op.is_noop() {
            return None;
        }
        self.commit(doc, op.clone(), "disk", 0, false).ok()?;
        let d = self.docs.get_mut(doc)?;
        d.synced = d.text.clone();
        d.synced_rev = d.revision;
        Some(DiskEvent::Reloaded {
            doc: doc.to_owned(),
            revision: d.revision,
            ops: op.to_ops(),
            hash: text_hash(&d.text),
        })
    }

    /// The changes of `doc` after revision `since` with their authors (`doc.changes`, for a
    /// window whose replay is gone); `stale_revision` when the log no longer reaches back.
    pub fn changes(&self, doc: &str, since: u64) -> Result<Value, RpcError> {
        let d = self.docs.get(doc).ok_or_else(|| not_open(doc))?;
        let oldest = d.log.front().map_or(d.revision, |(r, ..)| r - 1);
        if since < oldest || since > d.revision {
            return Err(RpcError::new(
                ErrorCode::StaleRevision,
                format!("the changes of {doc} after revision {since} are no longer kept"),
            )
            .hint("read doc.snapshot"));
        }
        let changes: Vec<Value> = d
            .log
            .iter()
            .filter(|(r, ..)| *r > since)
            .map(|(r, op, author, cseq)| {
                json!({"revision": r, "ops": op.to_ops(), "author": author, "cseq": cseq})
            })
            .collect();
        Ok(
            json!({"doc": doc, "revision": d.revision, "hash": text_hash(&d.text), "changes": changes}),
        )
    }

    /// The id of the open document at relative `path`, if open.
    pub fn id_of(&self, path: &str) -> Option<String> {
        self.docs.contains_key(path).then(|| path.to_owned())
    }
}

fn invalid_set(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Invalid, message)
}

/// The files below `path` (or `path` itself), never through links.
fn walk_files(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            if e.file_type()?.is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Rng;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("idecraft-docs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(dir).unwrap()
    }

    fn op(at: usize, delete: usize, insert: &str) -> Op {
        Op {
            at,
            delete,
            insert: insert.into(),
        }
    }

    #[test]
    fn edits_transform_dedupe_and_mirror_into_the_registry() {
        let root = scratch("edit");
        std::fs::write(root.join("a.txt"), "hello").unwrap();
        let registry = DocumentRegistry::default();
        let mut store = DocStore::new(root.clone(), registry.clone());
        let opened = store.open("a.txt", "c1").unwrap();
        assert_eq!((opened.revision, opened.text.as_str()), (1, "hello"));
        store.open("./a.txt", "c2").unwrap();
        assert_eq!(store.clients("a.txt"), ["c1", "c2"]);

        // Two edits on revision 1: the second is transformed, the earlier insert goes first.
        let e1 = store.edit("a.txt", 1, "c1", 1, &[op(5, 0, "!")]).unwrap();
        let e2 = store.edit("a.txt", 1, "c2", 1, &[op(5, 0, "?")]).unwrap();
        assert_eq!((e1.revision, e2.revision), (2, 3));
        assert_eq!(e2.ops, [op(6, 0, "?")]);
        let snap = store.snapshot("a.txt").unwrap();
        assert_eq!(snap.text, "hello!?");
        assert_eq!(snap.hash, crate::ot::str_hash("hello!?"));
        assert!(snap.modified);
        // The registry shows the backend's text to agents on the host.
        let doc = registry.get(&root.join("a.txt")).unwrap();
        assert_eq!(doc.text.to_string(), "hello!?");
        assert!(doc.modified);

        // A resent edit (same cseq) is acknowledged again without being applied twice.
        let again = store.edit("a.txt", 1, "c1", 1, &[op(5, 0, "!")]).unwrap();
        assert!(again.duplicate);
        assert_eq!((again.revision, again.hash.as_str()), (2, e1.hash.as_str()));
        assert_eq!(store.snapshot("a.txt").unwrap().text, "hello!?");
        assert!(store.edit("a.txt", 1, "c1", 0, &[]).is_err());
        // Bad revisions and unknown documents.
        assert_eq!(
            store.edit("a.txt", 9, "c1", 2, &[]).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            store.edit("b.txt", 1, "c1", 2, &[]).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            store.edit("a.txt", 1, "c9", 1, &[]).unwrap_err().code,
            ErrorCode::NotFound
        );
        store.close("a.txt", "c1");
        store.close("a.txt", "c2");
        assert!(registry.get(&root.join("a.txt")).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn old_bases_are_stale_and_binary_files_refused() {
        let root = scratch("stale");
        std::fs::write(root.join("a.txt"), "x").unwrap();
        std::fs::write(root.join("b.bin"), [0xff, 0x00]).unwrap();
        let mut store = DocStore::new(root.clone(), DocumentRegistry::default()).with_log_limit(2);
        store.open("a.txt", "c1").unwrap();
        for i in 1..=4 {
            store.edit("a.txt", i, "c1", i, &[op(0, 0, "a")]).unwrap();
        }
        let e = store.edit("a.txt", 1, "c1", 9, &[]).unwrap_err();
        assert_eq!(e.code, ErrorCode::StaleRevision);
        assert!(store.edit("a.txt", 3, "c1", 9, &[op(0, 0, "b")]).is_ok());
        assert_eq!(
            store.open("b.bin", "c1").unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            store.open("../x", "c1").unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            store.open("none.txt", "c1").unwrap_err().code,
            ErrorCode::NotFound
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saving_keeps_the_mode_records_history_and_refuses_conflicts() {
        let root = scratch("save");
        let file = root.join("run.sh");
        std::fs::write(&file, "echo a\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o750)).unwrap();
        }
        let mut store = DocStore::new(root.clone(), DocumentRegistry::default());
        store.open("run.sh", "c1").unwrap();
        store.edit("run.sh", 1, "c1", 1, &[op(5, 1, "b")]).unwrap();
        assert_eq!(store.unsaved("c1"), ["run.sh"]);
        assert_eq!(
            store.save("run.sh", 1, false).unwrap_err().code,
            ErrorCode::StaleRevision
        );
        assert_eq!(store.save("run.sh", 2, false).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "echo b\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o750);
        }
        assert!(store.unsaved("c1").is_empty());
        // A save without a change keeps the revision.
        assert_eq!(store.save("run.sh", 2, false).unwrap(), 2);
        let history = idecraft_core::LocalHistory::open(&root).unwrap();
        assert!(!history.revisions("run.sh").is_empty());
        // Our own save is not a disk change.
        assert_eq!(store.disk_check("run.sh"), None);

        // Changed on disk while unmodified: reloaded as a new revision by "disk".
        std::fs::write(&file, "echo c\n").unwrap();
        match store.disk_check("run.sh") {
            Some(DiskEvent::Reloaded { revision, ops, .. }) => {
                assert_eq!(revision, 3);
                assert_eq!(ops, [op(5, 1, "c")]);
            }
            other => panic!("{other:?}"),
        }
        // Changed on disk while modified: the window decides, saving refuses.
        store.edit("run.sh", 3, "c1", 2, &[op(0, 0, "#")]).unwrap();
        std::fs::write(&file, "echo d\n").unwrap();
        assert_eq!(
            store.disk_check("run.sh"),
            Some(DiskEvent::Changed {
                doc: "run.sh".into(),
                deleted: false
            })
        );
        assert_eq!(
            store.save("run.sh", 4, false).unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "echo d\n");
        // Overwrite is the window's choice.
        store.save("run.sh", 4, true).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "#echo c\n");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Random concurrent edits of two clients and an agent, each on a stale revision, end in the
    /// text and hash of applying the backend's ordered log (`SPEC-0317` AK-2, server half).
    #[test]
    fn random_concurrent_edits_follow_the_log() {
        let root = scratch("random");
        let mut rng = Rng::new(11);
        for round in 0..30 {
            let name = format!("f{round}.txt");
            std::fs::write(root.join(&name), rng.text(8)).unwrap();
            let mut store = DocStore::new(root.clone(), DocumentRegistry::default());
            let first = store.open(&name, "a").unwrap();
            store.open(&name, "b").unwrap();
            store.open(&name, "agent").unwrap();
            let mut replay = first.text.clone();
            // Each client knows some revision and its text.
            let mut known: HashMap<&str, (u64, String)> = ["a", "b", "agent"]
                .into_iter()
                .map(|c| (c, (1, first.text.clone())))
                .collect();
            let mut history = vec![first.text.clone()];
            let mut cseq: HashMap<&str, u64> = HashMap::new();
            for _ in 0..40 {
                let client = ["a", "b", "agent"][rng.below(3)];
                // Sometimes the client catches up to a newer revision first.
                if rng.below(3) == 0 {
                    let rev = 1 + rng.below(history.len());
                    known.insert(client, (rev as u64, history[rev - 1].clone()));
                }
                let (base, text) = known[client].clone();
                let edit = TextOp::random(text.chars().count(), &mut rng);
                let seq = cseq.entry(client).or_default();
                *seq += 1;
                let out = store
                    .edit(&name, base, client, *seq, &edit.to_ops())
                    .unwrap();
                if out.changed {
                    let applied = TextOp::from_ops(replay.chars().count(), &out.ops).unwrap();
                    replay = applied.apply_str(&replay).unwrap();
                    history.push(replay.clone());
                }
                assert_eq!(out.hash, crate::ot::str_hash(&replay));
            }
            assert_eq!(store.snapshot(&name).unwrap().text, replay);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// `SPEC-0323` v3 behaviour 5, AK-4: a merge read the file truncated by a writer, and the
    /// writer has written since; the merge must not write over the writer's text, which arrives
    /// with the next change instead.
    #[test]
    fn a_half_written_file_is_not_written_over() {
        let root = scratch("half");
        let path = root.join("a.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut store = DocStore::new(root.clone(), DocumentRegistry::default());
        store.enter_room(Arc::new(|_| {}), root.join("blobs"));
        store.open("a.txt", "c1").unwrap();
        store
            .edit("a.txt", 1, "c1", 1, &[op(0, 0, "START ")])
            .unwrap();
        // What the merge read: the file truncated, nothing written yet.
        std::fs::write(&path, "").unwrap();
        let (truncated, bytes) = disk_state(&path).unwrap();
        // The writer finishes before the merge writes back.
        std::fs::write(&path, "one\ntwo\n").unwrap();
        store.merge_disk("a.txt", truncated, &String::from_utf8(bytes).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        // The writer's change merges with the unsaved edit and is written back.
        store.disk_check("a.txt");
        let text = store.snapshot("a.txt").unwrap().text;
        assert!(
            text.starts_with("START ") && text.contains("two"),
            "{text:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        let _ = std::fs::remove_dir_all(&root);
    }
}
