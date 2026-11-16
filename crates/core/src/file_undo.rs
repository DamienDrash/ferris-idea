//! Undo and redo of the file operations of a project window (`SPEC-0275` v16 behaviours 37–43):
//! New, Rename, Paste as copy, Paste as move and Delete of the project tree are kept as steps
//! that can be taken back and made again.
//!
//! The rules that keep this safe (behaviours 40 and 41): a step is carried out only while every
//! item of it is still what the operation left (same entries, same bytes), nothing existing is
//! ever overwritten, and an item that leaves the project — a deleted one, a created one whose
//! creation is undone — is moved into the undo store `.idecraft/history/undo/` inside the
//! workspace and comes back only from there, only to the path it left. The store's folders must be
//! real folders inside the workspace; links are never followed. An item larger than
//! [`MAX_KEPT_BYTES`] is not kept.

use std::fmt;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::file_ops::{
    self, DeletePlan, FileOpError, NewKind, Transfer, TransferPlan, display_name, inside, io,
    record_deleted, relative,
};

/// The undo store of a workspace, relative to its folder (behaviour 41).
pub const UNDO_DIR: &str = ".idecraft/history/undo";

/// The largest item (sum of its files) the store keeps (behaviour 41).
pub const MAX_KEPT_BYTES: u64 = 64 * 1024 * 1024;

/// How many steps the history holds (behaviour 37).
pub const MAX_STEPS: usize = 100;

/// The largest moved file whose bytes before and after the move are kept (a changed package
/// line, behaviour 39).
const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

/// What a step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// New file or folder.
    Create,
    /// Rename in place.
    Rename,
    /// Paste of cut items.
    Move,
    /// Paste of copied items.
    Copy,
    /// Delete.
    Delete,
}

impl StepKind {
    /// The id the MCP tool reports.
    pub fn id(self) -> &'static str {
        match self {
            StepKind::Create => "create",
            StepKind::Rename => "rename",
            StepKind::Move => "move",
            StepKind::Copy => "copy",
            StepKind::Delete => "delete",
        }
    }
}

/// Why undo or redo of a step was refused; nothing on disk changed (behaviour 40). Paths are
/// relative to the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoError {
    /// The history has no such step.
    Nothing,
    /// The item is not what the operation left.
    Changed(String),
    /// Something is at the place the item goes back to.
    Occupied(String),
    /// The item is no longer where the operation left it.
    Gone(String),
    /// The kept copy in the store is not what was put there.
    StoreChanged(String),
    /// The kept copy is gone from the store.
    StoreGone(String),
    /// The store is not a real folder inside the workspace, or cannot be written.
    NoStore,
    /// The file system refused.
    Io(String),
}

impl fmt::Display for UndoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UndoError::Nothing => write!(f, "there is no such step"),
            UndoError::Changed(p) => {
                write!(
                    f,
                    "{p} was changed since the operation; nothing was touched"
                )
            }
            UndoError::Occupied(p) => write!(f, "{p} already exists; nothing was touched"),
            UndoError::Gone(p) => write!(f, "{p} is no longer there; nothing was touched"),
            UndoError::StoreChanged(p) => write!(
                f,
                "the kept copy of {p} was changed in the undo store; nothing was touched"
            ),
            UndoError::StoreGone(p) => {
                write!(f, "the kept copy of {p} is gone from the undo store")
            }
            UndoError::NoStore => write!(
                f,
                "the undo store ({UNDO_DIR}) is not a folder inside the project"
            ),
            UndoError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for UndoError {}

/// Why a deleted item was not kept for undo (behaviour 41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotKept {
    /// It is larger than the store's limit.
    TooLarge {
        /// The sum of its files.
        bytes: u64,
        /// The limit.
        limit: u64,
    },
    /// The store cannot be used.
    NoStore,
}

impl fmt::Display for NotKept {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotKept::TooLarge { limit, .. } => write!(
                f,
                "larger than {} MB, not kept: this cannot be undone",
                limit / (1024 * 1024)
            ),
            NotKept::NoStore => write!(
                f,
                "the undo store cannot be used, not kept: this cannot be undone"
            ),
        }
    }
}

/// What [`FileUndo::delete`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deleted {
    /// What was deleted.
    pub plan: DeletePlan,
    /// Why the item cannot come back by undo, if it cannot.
    pub not_kept: Option<NotKept>,
}

/// A step as the window and the MCP tool show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInfo {
    /// What it did (the kind of its first item).
    pub kind: StepKind,
    /// Short name for the Edit menu and the status bar, e.g. `New File a.txt`.
    pub label: String,
    /// The paths the step touches, relative to the workspace: where each item is while the
    /// operation stands (for a deleted item: where it was).
    pub paths: Vec<String>,
    /// For a rename or move of one item: the path before it.
    pub from: Option<String>,
    /// Whether the first item is a folder.
    pub dir: bool,
}

impl StepInfo {
    /// The question before the step is taken back (`redo` false) or made again, and the line
    /// that says what will happen (behaviour 38).
    pub fn question(&self, redo: bool) -> (String, String) {
        let verb = if redo { "Redo" } else { "Undo" };
        let title = title(self.kind, self.dir);
        let message = match (self.paths.as_slice(), &self.from) {
            ([to], Some(from)) => format!("{verb} {title} \"{from}\" to \"{to}\"?"),
            ([one], None) => format!("{verb} {title} \"{one}\"?"),
            (many, _) => format!("{verb} {title} of {} items?", many.len()),
        };
        let it = match (self.paths.len(), self.dir) {
            (1, true) => "The folder",
            (1, false) => "The file",
            _ => "They",
        };
        let one = self.paths.len() == 1;
        let does =
            |single: &str, several: &str| format!("{it} {}", if one { single } else { several });
        let mut detail = match (self.kind, redo) {
            (StepKind::Create, false) if self.dir || !one => does(
                "is taken out of the project.",
                "are taken out of the project.",
            ),
            (StepKind::Create, false) => {
                "The file is taken out of the project and its tab closes.".to_owned()
            }
            (StepKind::Copy, false) => does(
                "is taken out of the project.",
                "are taken out of the project.",
            ),
            (StepKind::Create | StepKind::Copy, true) => {
                does("is put back as it was.", "are put back as they were.")
            }
            (StepKind::Rename, false) => {
                does("gets its old name back.", "get their old names back.")
            }
            (StepKind::Rename, true) => {
                does("gets the new name again.", "get the new names again.")
            }
            (StepKind::Move, false) => {
                does("goes back to where it was.", "go back to where they were.")
            }
            (StepKind::Move, true) => does("is moved again.", "are moved again."),
            (StepKind::Delete, false) => does(
                "is put back where it was, with its content.",
                "are put back where they were, with their content.",
            ),
            (StepKind::Delete, true) => does(
                "is deleted again and stays kept for Undo.",
                "are deleted again and stay kept for Undo.",
            ),
        };
        if !one {
            for p in self.paths.iter().take(10) {
                detail.push('\n');
                detail.push_str(p);
            }
            if self.paths.len() > 10 {
                detail.push_str(&format!("\nand {} more", self.paths.len() - 10));
            }
        }
        (message, detail)
    }
}

fn title(kind: StepKind, dir: bool) -> &'static str {
    match (kind, dir) {
        (StepKind::Create, true) => "New Folder",
        (StepKind::Create, false) => "New File",
        (StepKind::Rename, _) => "Rename",
        (StepKind::Move, _) => "Move",
        (StepKind::Copy, _) => "Copy",
        (StepKind::Delete, _) => "Delete",
    }
}

/// What undo or redo of a step changed, for the window to follow (behaviour 39). Paths are
/// absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The step.
    pub info: StepInfo,
    /// Items that left the project: their tabs close.
    pub gone: Vec<PathBuf>,
    /// Items that changed their path: tabs follow.
    pub moved: Vec<(PathBuf, PathBuf)>,
    /// Items that are in the project again or at a new path: the tree selects them.
    pub back: Vec<PathBuf>,
}

/// What an item is: a hash over its entries, their kinds and their content (behaviour 40).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint(String);

/// One item of a step.
#[derive(Debug, Clone)]
struct Part {
    kind: StepKind,
    /// Where the item is in the project while the operation stands; for a deleted item, where it
    /// was.
    path: PathBuf,
    /// Rename and move: the path before.
    old: Option<PathBuf>,
    /// Create, copy and delete: the item's place in the store, once it has one.
    kept: Option<PathBuf>,
    dir: bool,
    /// Folders the step created on the way to `path`, outermost first.
    made: Vec<PathBuf>,
    /// The bytes of a moved file before and after the move, where the move changed them.
    texts: Option<(Vec<u8>, Vec<u8>)>,
    print: Fingerprint,
}

impl Part {
    /// Whether the item is in the store (not in the project) while the step is done or undone.
    fn in_store(&self, done: bool) -> bool {
        match self.kind {
            StepKind::Create | StepKind::Copy => !done,
            StepKind::Delete => done,
            StepKind::Rename | StepKind::Move => false,
        }
    }
}

/// A step: one operation, on one item or several.
#[derive(Debug, Clone)]
struct Step {
    parts: Vec<Part>,
}

static SESSIONS: AtomicU64 = AtomicU64::new(0);

/// The history of file operations of one project window (behaviour 37).
#[derive(Debug)]
pub struct FileUndo {
    /// The workspace folder, canonical where it could be resolved.
    root: PathBuf,
    /// This history's folder name inside the store.
    session: String,
    undo: Vec<Step>,
    redo: Vec<Step>,
    limit: u64,
    slots: u64,
}

impl FileUndo {
    /// An empty history for the workspace at `root`. What an earlier session left in the store
    /// is removed (behaviour 41); nothing is created.
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let session = format!(
            "{}-{nanos}-{}",
            std::process::id(),
            SESSIONS.fetch_add(1, Ordering::Relaxed)
        );
        let me = Self {
            root,
            session,
            undo: Vec::new(),
            redo: Vec::new(),
            limit: MAX_KEPT_BYTES,
            slots: 0,
        };
        if let Some(store) = me.store_dir(false)
            && let Ok(entries) = std::fs::read_dir(&store)
        {
            for entry in entries.flatten() {
                remove_item(&entry.path());
            }
        }
        me
    }

    /// The same history with another size limit for kept items (tests).
    pub fn with_limit(mut self, bytes: u64) -> Self {
        self.limit = bytes;
        self
    }

    /// The limit for kept items in bytes.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// The store folder `UNDO_DIR` if every folder on the way is a real folder inside the
    /// workspace; with `create` missing folders are made. Links are refused (behaviour 41).
    fn store_dir(&self, create: bool) -> Option<PathBuf> {
        let mut dir = self.root.clone();
        for segment in UNDO_DIR.split('/') {
            dir.push(segment);
            real_dir(&dir, create)?;
        }
        dir.canonicalize()
            .ok()
            .filter(|c| c.starts_with(&self.root))
            .map(|_| dir)
    }

    /// This history's folder in the store, made when needed.
    fn session_dir(&self) -> Option<PathBuf> {
        let dir = self.store_dir(true)?.join(&self.session);
        real_dir(&dir, true)?;
        Some(dir)
    }

    /// A free place in the store for an item called `name`.
    fn slot(&mut self, name: &std::ffi::OsStr) -> Option<PathBuf> {
        let session = self.session_dir()?;
        self.slots += 1;
        let dir = session.join(self.slots.to_string());
        std::fs::create_dir(&dir).ok()?;
        Some(dir.join(name))
    }

    /// Whether `kept` is a place of this history in the store, reached without a link.
    fn in_own_store(&self, kept: &Path) -> bool {
        let Some(session) = self.store_dir(false).map(|s| s.join(&self.session)) else {
            return false;
        };
        let Some(slot) = kept.parent() else {
            return false;
        };
        slot.parent() == Some(session.as_path())
            && real_dir(&session, false).is_some()
            && real_dir(slot, false).is_some()
    }

    fn rel(&self, path: &Path) -> String {
        relative(&self.root, path).unwrap_or_else(|| path.display().to_string())
    }

    fn push(&mut self, step: Step) {
        for old in std::mem::take(&mut self.redo) {
            self.discard(&old, false);
        }
        self.undo.push(step);
        while self.undo.len() > MAX_STEPS {
            let oldest = self.undo.remove(0);
            self.discard(&oldest, true);
        }
    }

    /// Removes from the store what a step that leaves the history kept there.
    fn discard(&self, step: &Step, done: bool) {
        for part in &step.parts {
            if let Some(kept) = part.kept.as_deref().filter(|_| part.in_store(done))
                && self.in_own_store(kept)
                && let Some(slot) = kept.parent()
            {
                remove_item(slot);
            }
        }
    }

    /// New (behaviours 3–4) as a step: [`file_ops::create`].
    pub fn create(
        &mut self,
        dir: &Path,
        name: &str,
        kind: NewKind,
    ) -> Result<PathBuf, FileOpError> {
        let base = inside(&self.root, dir)?;
        // The folders on the way that are not there yet.
        let mut missing = Vec::new();
        let mut at = base.clone();
        let segments: Vec<&str> = name.trim_end().split('/').collect();
        for segment in &segments[..segments.len().saturating_sub(1)] {
            at.push(segment);
            if at.symlink_metadata().is_err() {
                missing.push(at.clone());
            }
        }
        let path = file_ops::create(&self.root, dir, name, kind)?;
        let path = inside(&self.root, &path).unwrap_or(path);
        let made: Vec<PathBuf> = missing
            .into_iter()
            .filter(|m| m.is_dir() && path.starts_with(m) && *m != path)
            .collect();
        if let Ok((print, _)) = fingerprint(&path, self.limit) {
            self.push(Step {
                parts: vec![Part {
                    kind: StepKind::Create,
                    dir: kind == NewKind::Directory,
                    path: path.clone(),
                    old: None,
                    kept: None,
                    made,
                    texts: None,
                    print,
                }],
            });
        }
        Ok(path)
    }

    /// Rename (behaviour 5) as a step: [`file_ops::rename`].
    pub fn rename(&mut self, path: &Path, new_name: &str) -> Result<PathBuf, FileOpError> {
        let from = inside(&self.root, path)?;
        let to = file_ops::rename(&self.root, path, new_name)?;
        if to != from
            && let Ok((print, _)) = fingerprint(&to, self.limit)
        {
            let dir = to.symlink_metadata().is_ok_and(|m| m.is_dir());
            self.push(Step {
                parts: vec![Part {
                    kind: StepKind::Rename,
                    path: to.clone(),
                    old: Some(from),
                    kept: None,
                    dir,
                    made: Vec::new(),
                    texts: None,
                    print,
                }],
            });
        }
        Ok(to)
    }

    /// Paste (behaviours 19–21) as a step: [`file_ops::transfer`]. `after` runs once the item is
    /// at its new place and may change it (the package line of behaviour 26); a moved file's
    /// bytes before and after are kept so undo puts the old ones back.
    pub fn transfer(
        &mut self,
        from: &Path,
        to_dir: &Path,
        new_name: Option<&str>,
        mode: Transfer,
        after: impl FnOnce(&TransferPlan),
    ) -> Result<TransferPlan, FileOpError> {
        let plan = file_ops::transfer(&self.root, from, to_dir, new_name, mode)?;
        let small = !plan.dir
            && plan
                .to
                .symlink_metadata()
                .is_ok_and(|m| m.is_file() && m.len() <= MAX_TEXT_BYTES);
        let before = (mode == Transfer::Move && small)
            .then(|| std::fs::read(&plan.to).ok())
            .flatten();
        after(&plan);
        let texts = before.and_then(|b| {
            let now = std::fs::read(&plan.to).ok()?;
            (now != b).then_some((b, now))
        });
        if let Ok((print, _)) = fingerprint(&plan.to, self.limit) {
            let moved = mode == Transfer::Move;
            self.push(Step {
                parts: vec![Part {
                    kind: if moved {
                        StepKind::Move
                    } else {
                        StepKind::Copy
                    },
                    path: plan.to.clone(),
                    old: moved.then(|| plan.from.clone()),
                    kept: None,
                    dir: plan.dir,
                    made: Vec::new(),
                    texts,
                    print,
                }],
            });
        }
        Ok(plan)
    }

    /// Delete (behaviour 6) as a step: every file is recorded in the Local History as
    /// [`file_ops::delete`] does, then the item is moved into the store. An item that is too
    /// large, or a store that cannot be used, means a plain delete without a step (behaviour 41).
    pub fn delete(&mut self, path: &Path) -> Result<Deleted, FileOpError> {
        let plan = file_ops::delete_plan(&self.root, path)?;
        let (print, bytes) = fingerprint(&plan.path, self.limit).map_err(io)?;
        let not_kept = if bytes > self.limit {
            Some(NotKept::TooLarge {
                bytes,
                limit: self.limit,
            })
        } else {
            let name = plan.path.file_name().map(|n| n.to_os_string());
            match name.and_then(|n| self.slot(&n)) {
                None => Some(NotKept::NoStore),
                Some(kept) => {
                    record_deleted(&self.root, &plan.path);
                    match std::fs::rename(&plan.path, &kept) {
                        Ok(()) => {
                            crate::bookmarks::follow_delete(&self.root, &plan.path);
                            self.push(Step {
                                parts: vec![Part {
                                    kind: StepKind::Delete,
                                    path: plan.path.clone(),
                                    old: None,
                                    kept: Some(kept),
                                    dir: plan.dir,
                                    made: Vec::new(),
                                    texts: None,
                                    print,
                                }],
                            });
                            return Ok(Deleted {
                                plan,
                                not_kept: None,
                            });
                        }
                        Err(_) => {
                            if let Some(slot) = kept.parent() {
                                remove_item(slot);
                            }
                            Some(NotKept::NoStore)
                        }
                    }
                }
            }
        };
        let plan = file_ops::delete(&self.root, &plan.path)?;
        Ok(Deleted { plan, not_kept })
    }

    /// Makes the newest `count` steps one step: an operation on several items (behaviour 37).
    pub fn join_last(&mut self, count: usize) {
        if count < 2 || count > self.undo.len() {
            return;
        }
        let at = self.undo.len() - count;
        let parts = self
            .undo
            .drain(at..)
            .flat_map(|s| s.parts)
            .collect::<Vec<_>>();
        self.undo.push(Step { parts });
    }

    fn info(&self, step: &Step) -> StepInfo {
        let first = &step.parts[0];
        let name = display_name(&first.path);
        let what = title(first.kind, first.dir);
        let label = match (step.parts.len(), first.kind) {
            (1, StepKind::Rename) => format!(
                "{what} {} to {name}",
                first.old.as_deref().map(display_name).unwrap_or_default()
            ),
            (1, StepKind::Move) => {
                let dir = first.path.parent().map(|d| self.rel(d)).unwrap_or_default();
                format!(
                    "{what} {name} to {}",
                    if dir.is_empty() {
                        "the project folder"
                    } else {
                        &dir
                    }
                )
            }
            (1, _) => format!("{what} {name}"),
            (n, _) => format!("{what} {n} items"),
        };
        StepInfo {
            kind: first.kind,
            label,
            paths: step.parts.iter().map(|p| self.rel(&p.path)).collect(),
            from: first
                .old
                .as_deref()
                .filter(|_| step.parts.len() == 1)
                .map(|p| self.rel(p)),
            dir: first.dir,
        }
    }

    /// The step undo would take back.
    pub fn next_undo(&self) -> Option<StepInfo> {
        self.undo.last().map(|s| self.info(s))
    }

    /// The step redo would make again.
    pub fn next_redo(&self) -> Option<StepInfo> {
        self.redo.last().map(|s| self.info(s))
    }

    /// The steps undo would take back, the next one first, and those redo would make again.
    pub fn steps(&self) -> (Vec<StepInfo>, Vec<StepInfo>) {
        let list = |steps: &[Step]| steps.iter().rev().map(|s| self.info(s)).collect();
        (list(&self.undo), list(&self.redo))
    }

    /// The absolute paths in the project the next undo (or redo) step touches: where its items
    /// are now and where they go. The window refuses the step while an editor of one of them
    /// has unsaved changes (behaviour 40).
    pub fn touched(&self, redo: bool) -> Vec<PathBuf> {
        let step = if redo {
            self.redo.last()
        } else {
            self.undo.last()
        };
        step.map(|s| {
            s.parts
                .iter()
                .flat_map(|p| std::iter::once(p.path.clone()).chain(p.old.clone()))
                .collect()
        })
        .unwrap_or_default()
    }

    /// Where the item of `part` is now and where it goes when the step changes from `done`.
    fn places(&self, part: &Part, done: bool) -> Result<(PathBuf, PathBuf), UndoError> {
        let other = match part.kind {
            StepKind::Rename | StepKind::Move => part.old.clone(),
            _ => part.kept.clone(),
        }
        .ok_or(UndoError::NoStore)?;
        let in_project_now = match part.kind {
            StepKind::Delete => !done,
            _ => done,
        };
        Ok(if in_project_now {
            (part.path.clone(), other)
        } else {
            (other, part.path.clone())
        })
    }

    /// Checks one item before anything is touched (behaviours 40 and 41).
    fn check(&self, part: &Part, done: bool) -> Result<(), UndoError> {
        let (now, then) = self.places(part, done)?;
        let shown = self.rel(&part.path);
        let stored = part.in_store(done);
        if stored && !self.in_own_store(&now) {
            return Err(UndoError::NoStore);
        }
        if now.symlink_metadata().is_err() {
            return Err(if stored {
                UndoError::StoreGone(shown)
            } else {
                UndoError::Gone(self.rel(&now))
            });
        }
        match fingerprint(&now, self.limit) {
            Ok((print, _)) if print == part.print => {}
            Ok(_) if stored => return Err(UndoError::StoreChanged(shown)),
            Ok(_) => return Err(UndoError::Changed(self.rel(&now))),
            Err(e) => return Err(UndoError::Io(e.to_string())),
        }
        if part.in_store(!done) {
            if !self.in_own_store(&then) {
                return Err(UndoError::NoStore);
            }
        } else {
            // Only to a path inside the workspace, reached without leaving it.
            let resolved = inside(&self.root, &then).map_err(|e| UndoError::Io(e.to_string()))?;
            if !self.below_root(&resolved) {
                return Err(UndoError::Io(format!(
                    "{} is not inside the project",
                    then.display()
                )));
            }
        }
        // A rename that only changed the case on a file system that ignores case finds itself.
        let same = then
            .canonicalize()
            .ok()
            .zip(now.canonicalize().ok())
            .is_some_and(|(a, b)| a == b);
        if then.symlink_metadata().is_ok() && !same {
            return Err(UndoError::Occupied(self.rel(&then)));
        }
        Ok(())
    }

    /// Whether the nearest existing folder above `path` is inside the workspace once links are
    /// resolved.
    fn below_root(&self, path: &Path) -> bool {
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return false;
        }
        let mut at = path.parent();
        while let Some(dir) = at {
            if let Ok(real) = dir.canonicalize() {
                return real.starts_with(&self.root);
            }
            at = dir.parent();
        }
        false
    }

    /// Moves one checked item; `done` is the state it leaves.
    fn apply(&mut self, part: &mut Part, done: bool) -> Result<(), UndoError> {
        let fail = |e: std::io::Error| UndoError::Io(e.to_string());
        // An item that leaves the project for the first time gets its place in the store.
        if part.in_store(!done) && part.kept.is_none() {
            let name = part
                .path
                .file_name()
                .ok_or(UndoError::NoStore)?
                .to_os_string();
            part.kept = Some(self.slot(&name).ok_or(UndoError::NoStore)?);
            self.check(part, done)?;
        }
        let (now, then) = self.places(part, done)?;
        let leaves = part.in_store(!done);
        if leaves {
            record_deleted(&self.root, &now);
        } else if let Some(parent) = then.parent() {
            std::fs::create_dir_all(parent).map_err(fail)?;
        }
        std::fs::rename(&now, &then).map_err(fail)?;
        if let Some((before, after)) = &part.texts {
            // Undo puts the bytes from before the move back, redo those from after it.
            let bytes = if done { before } else { after };
            std::fs::write(&then, bytes).map_err(fail)?;
            if let Ok((print, _)) = fingerprint(&then, self.limit) {
                part.print = print;
            }
        }
        match part.kind {
            StepKind::Rename | StepKind::Move => {
                crate::bookmarks::follow_move(&self.root, &now, &then)
            }
            _ if leaves => crate::bookmarks::follow_delete(&self.root, &now),
            _ => {}
        }
        if leaves && part.kind == StepKind::Create {
            // The folders the step made on the way, innermost first, while they are empty.
            for made in part.made.iter().rev() {
                if std::fs::remove_dir(made).is_err() {
                    break;
                }
            }
        }
        Ok(())
    }

    fn turn(&mut self, redo: bool) -> Result<Outcome, UndoError> {
        let done = !redo;
        let step = if redo {
            self.redo.last()
        } else {
            self.undo.last()
        }
        .ok_or(UndoError::Nothing)?;
        for part in &step.parts {
            // An item without a place in the store yet is checked once it has one.
            if part.in_store(!done) && part.kept.is_none() {
                self.check_leaving(part)?;
            } else {
                self.check(part, done)?;
            }
        }
        let mut step = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        }
        .ok_or(UndoError::Nothing)?;
        let info = self.info(&step);
        let order: Vec<usize> = if redo {
            (0..step.parts.len()).collect()
        } else {
            (0..step.parts.len()).rev().collect()
        };
        let mut outcome = Outcome {
            info,
            gone: Vec::new(),
            moved: Vec::new(),
            back: Vec::new(),
        };
        let mut turned = Vec::new();
        let mut failed = None;
        for i in order {
            let mut part = step.parts[i].clone();
            match self.apply(&mut part, done) {
                Ok(()) => {
                    match part.kind {
                        StepKind::Rename | StepKind::Move => {
                            if let Ok((now, then)) = self.places(&part, !done) {
                                outcome.moved.push((then, now.clone()));
                                outcome.back.push(now);
                            }
                        }
                        _ if part.in_store(!done) => outcome.gone.push(part.path.clone()),
                        _ => outcome.back.push(part.path.clone()),
                    }
                    step.parts[i] = part;
                    turned.push(i);
                }
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        let Some(error) = failed else {
            if redo {
                self.undo.push(step);
            } else {
                self.redo.push(step);
            }
            return Ok(outcome);
        };
        // What was turned before the file system refused is a step of its own on the other
        // side; the rest stays where it was.
        let (mut turned_parts, mut rest) = (Vec::new(), Vec::new());
        for (i, part) in step.parts.into_iter().enumerate() {
            if turned.contains(&i) {
                turned_parts.push(part);
            } else {
                rest.push(part);
            }
        }
        let (same, other) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if !rest.is_empty() {
            same.push(Step { parts: rest });
        }
        if !turned_parts.is_empty() {
            other.push(Step {
                parts: turned_parts,
            });
        }
        Err(error)
    }

    /// [`FileUndo::check`] for an item that has no place in the store yet: everything but the
    /// store's side.
    fn check_leaving(&self, probe: &Part) -> Result<(), UndoError> {
        let now = &probe.path;
        if now.symlink_metadata().is_err() {
            return Err(UndoError::Gone(self.rel(now)));
        }
        match fingerprint(now, self.limit) {
            Ok((print, _)) if print == probe.print => Ok(()),
            Ok(_) => Err(UndoError::Changed(self.rel(now))),
            Err(e) => Err(UndoError::Io(e.to_string())),
        }
    }

    /// Takes the newest step back (behaviour 39); refused without touching anything when a
    /// check fails (behaviour 40).
    pub fn undo(&mut self) -> Result<Outcome, UndoError> {
        self.turn(false)
    }

    /// Makes the step undone last again.
    pub fn redo(&mut self) -> Result<Outcome, UndoError> {
        self.turn(true)
    }
}

impl Drop for FileUndo {
    /// The history ends: what it kept in the store goes (behaviour 41).
    fn drop(&mut self) {
        if let Some(store) = self.store_dir(false) {
            let session = store.join(&self.session);
            if real_dir(&session, false).is_some() {
                remove_item(&session);
            }
        }
    }
}

/// `dir` if it is a real folder (not a link); with `create` it is made when nothing is there.
fn real_dir(dir: &Path, create: bool) -> Option<()> {
    match dir.symlink_metadata() {
        Ok(meta) => meta.is_dir().then_some(()),
        Err(_) if create => {
            std::fs::create_dir(dir).ok()?;
            dir.symlink_metadata().ok()?.is_dir().then_some(())
        }
        Err(_) => None,
    }
}

/// Removes a file, a link (not what it points to) or a folder with everything in it.
fn remove_item(path: &Path) {
    match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

/// The fingerprint of the item at `path` and the sum of its files' sizes. Links are named with
/// their target and never followed. Up to `limit` bytes the files' content is hashed; above it
/// their sizes and modification times stand for it.
fn fingerprint(path: &Path, limit: u64) -> std::io::Result<(Fingerprint, u64)> {
    // (path below the item, kind, size, modification time, absolute path)
    let mut entries: Vec<(String, u8, u64, u128, PathBuf)> = Vec::new();
    let mut stack = vec![(String::new(), path.to_path_buf())];
    while let Some((rel, at)) = stack.pop() {
        let meta = at.symlink_metadata()?;
        let kind = if meta.file_type().is_symlink() {
            b'l'
        } else if meta.is_dir() {
            b'd'
        } else {
            b'f'
        };
        if kind == b'd' {
            for entry in std::fs::read_dir(&at)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let below = if rel.is_empty() {
                    name
                } else {
                    format!("{rel}/{name}")
                };
                stack.push((below, entry.path()));
            }
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let size = if kind == b'f' { meta.len() } else { 0 };
        entries.push((rel, kind, size, modified, at));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let total: u64 = entries.iter().map(|e| e.2).sum();
    let deep = total <= limit;
    let mut all = Sha256::new();
    all.update([u8::from(deep)]);
    for (rel, kind, size, modified, at) in &entries {
        all.update(rel.as_bytes());
        all.update([0, *kind]);
        match kind {
            b'l' => all.update(std::fs::read_link(at)?.to_string_lossy().as_bytes()),
            b'f' => {
                all.update(size.to_le_bytes());
                if deep {
                    let mut one = Sha256::new();
                    let mut file = std::fs::File::open(at)?;
                    let mut chunk = vec![0u8; 64 * 1024];
                    loop {
                        let n = file.read(&mut chunk)?;
                        if n == 0 {
                            break;
                        }
                        one.update(&chunk[..n]);
                    }
                    all.update(one.finalize());
                } else {
                    all.update(modified.to_le_bytes());
                }
            }
            _ => {}
        }
        all.update([0xff]);
    }
    let hex = all.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((Fingerprint(hex), total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-file-undo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("a.txt"), "alpha\n").unwrap();
        std::fs::write(dir.join("src/b.txt"), "beta\n").unwrap();
        dir.canonicalize().unwrap()
    }

    /// Every file of the workspace outside `.idecraft`, with its bytes, and every folder.
    fn disk(root: &Path) -> Vec<(String, Option<Vec<u8>>)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let rel = relative(root, &path).unwrap();
                if rel == ".idecraft" {
                    continue;
                }
                if path.is_dir() {
                    out.push((rel, None));
                    stack.push(path);
                } else {
                    out.push((rel, Some(std::fs::read(&path).unwrap())));
                }
            }
        }
        out.sort();
        out
    }

    fn kept_items(root: &Path) -> usize {
        let Ok(sessions) = std::fs::read_dir(root.join(UNDO_DIR)) else {
            return 0;
        };
        sessions
            .flatten()
            .map(|s| std::fs::read_dir(s.path()).map_or(0, |d| d.count()))
            .sum()
    }

    /// SPEC-0275 v16 AK-34: New with folders on the way.
    #[test]
    fn create_is_undone_and_redone() {
        let root = fixture("create");
        let before = disk(&root);
        let mut history = FileUndo::new(&root);
        let made = history
            .create(&root, "notes/deep/c.txt", NewKind::File)
            .unwrap();
        assert_eq!(made, root.join("notes/deep/c.txt"));
        let after = disk(&root);
        let step = history.next_undo().unwrap();
        assert_eq!(step.label, "New File c.txt");
        assert_eq!(step.kind, StepKind::Create);
        assert_eq!(step.paths, ["notes/deep/c.txt"]);
        assert_eq!(
            step.question(false).0,
            "Undo New File \"notes/deep/c.txt\"?"
        );
        assert!(step.question(false).1.contains("its tab closes"));

        let undone = history.undo().unwrap();
        assert_eq!(undone.gone, [root.join("notes/deep/c.txt")]);
        assert_eq!(disk(&root), before, "the folders made on the way go too");
        assert!(history.next_undo().is_none());
        assert_eq!(history.next_redo().unwrap().label, "New File c.txt");
        assert_eq!(history.undo(), Err(UndoError::Nothing));

        let redone = history.redo().unwrap();
        assert_eq!(redone.back, [root.join("notes/deep/c.txt")]);
        assert_eq!(disk(&root), after);
        assert_eq!(history.redo(), Err(UndoError::Nothing));

        // A folder, and a folder that is no longer empty.
        let mut history = FileUndo::new(&root);
        history.create(&root, "box", NewKind::Directory).unwrap();
        assert_eq!(history.next_undo().unwrap().label, "New Folder box");
        std::fs::write(root.join("box/in.txt"), "x").unwrap();
        assert_eq!(history.undo(), Err(UndoError::Changed("box".into())));
        std::fs::remove_file(root.join("box/in.txt")).unwrap();
        history.undo().unwrap();
        assert!(!root.join("box").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-34: Rename, a move that changed the text, a copy.
    #[test]
    fn rename_and_move_are_undone_and_redone() {
        let root = fixture("move");
        let start = disk(&root);
        let mut history = FileUndo::new(&root);
        let renamed = history.rename(&root.join("a.txt"), "z.txt").unwrap();
        assert_eq!(renamed, root.join("z.txt"));
        let step = history.next_undo().unwrap();
        assert_eq!(step.label, "Rename a.txt to z.txt");
        assert_eq!(
            step.question(false).0,
            "Undo Rename \"a.txt\" to \"z.txt\"?"
        );
        let after_rename = disk(&root);

        let plan = history
            .transfer(
                &root.join("src/b.txt"),
                &root.join("lib"),
                None,
                Transfer::Move,
                |plan| std::fs::write(&plan.to, "package lib\nbeta\n").unwrap(),
            )
            .unwrap();
        assert_eq!(plan.to, root.join("lib/b.txt"));
        assert_eq!(history.next_undo().unwrap().label, "Move b.txt to lib");
        let after_move = disk(&root);

        history
            .transfer(
                &root.join("z.txt"),
                &root.join("src"),
                None,
                Transfer::Copy,
                |_| {},
            )
            .unwrap();
        assert_eq!(history.next_undo().unwrap().label, "Copy z.txt");
        let after_copy = disk(&root);

        let copy = history.undo().unwrap();
        assert_eq!(copy.gone, [root.join("src/z.txt")]);
        assert_eq!(disk(&root), after_move);
        let moved = history.undo().unwrap();
        assert_eq!(
            moved.moved,
            [(root.join("lib/b.txt"), root.join("src/b.txt"))]
        );
        assert_eq!(
            disk(&root),
            after_rename,
            "with the bytes from before the move"
        );
        history.undo().unwrap();
        assert_eq!(disk(&root), start);

        history.redo().unwrap();
        assert_eq!(disk(&root), after_rename);
        history.redo().unwrap();
        assert_eq!(
            disk(&root),
            after_move,
            "with the bytes from after the move"
        );
        history.redo().unwrap();
        assert_eq!(disk(&root), after_copy);
        let (undo, redo) = history.steps();
        assert_eq!(undo.len(), 3);
        assert!(redo.is_empty());
        assert_eq!(undo[0].kind, StepKind::Copy);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-34: Delete of a file, of a folder with files and of an empty folder.
    #[test]
    fn a_deleted_file_and_folder_come_back() {
        let root = fixture("delete");
        std::fs::create_dir_all(root.join("src/inner/empty")).unwrap();
        std::fs::write(root.join("src/inner/bin.dat"), [0u8, 159, 255, 10]).unwrap();
        let start = disk(&root);
        let mut history = FileUndo::new(&root);

        let deleted = history.delete(&root.join("a.txt")).unwrap();
        assert_eq!(deleted.not_kept, None);
        assert!(!root.join("a.txt").exists());
        assert_eq!(
            kept_items(&root),
            1,
            "kept in the store inside the workspace"
        );
        let without_file = disk(&root);
        let deleted = history.delete(&root.join("src")).unwrap();
        assert_eq!(deleted.plan.files, 2);
        assert!(!root.join("src").exists());
        let without_both = disk(&root);
        assert_eq!(history.next_undo().unwrap().label, "Delete src");

        let back = history.undo().unwrap();
        assert_eq!(back.back, [root.join("src")]);
        assert_eq!(
            disk(&root),
            without_file,
            "the folder with everything in it"
        );
        history.undo().unwrap();
        assert_eq!(disk(&root), start, "byte for byte");
        assert_eq!(kept_items(&root), 2, "the places stay, empty, for redo");

        history.redo().unwrap();
        history.redo().unwrap();
        assert_eq!(disk(&root), without_both);
        history.undo().unwrap();
        history.undo().unwrap();
        assert_eq!(disk(&root), start);

        // The deleted text is in the Local History as before.
        let local = crate::LocalHistory::open(&root).unwrap();
        assert!(!local.revisions("a.txt").is_empty());

        // The history ends: the store is emptied.
        history.delete(&root.join("a.txt")).unwrap();
        drop(history);
        assert_eq!(kept_items(&root), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-34: several items are one step.
    #[test]
    fn several_items_are_one_step() {
        let root = fixture("several");
        let start = disk(&root);
        let mut history = FileUndo::new(&root);
        history.delete(&root.join("a.txt")).unwrap();
        history.delete(&root.join("src/b.txt")).unwrap();
        history.join_last(2);
        let step = history.next_undo().unwrap();
        assert_eq!(step.label, "Delete 2 items");
        assert_eq!(step.paths, ["a.txt", "src/b.txt"]);
        let (message, detail) = step.question(false);
        assert_eq!(message, "Undo Delete of 2 items?");
        assert!(detail.contains("a.txt") && detail.contains("src/b.txt"));
        let after = disk(&root);

        // One item's place is taken: nothing of the step is touched.
        std::fs::write(root.join("a.txt"), "other").unwrap();
        assert_eq!(history.undo(), Err(UndoError::Occupied("a.txt".into())));
        assert!(!root.join("src/b.txt").exists());
        std::fs::remove_file(root.join("a.txt")).unwrap();

        let back = history.undo().unwrap();
        assert_eq!(back.back.len(), 2);
        assert_eq!(disk(&root), start);
        assert!(history.next_undo().is_none());
        history.redo().unwrap();
        assert_eq!(disk(&root), after);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-34: a new operation drops the undone steps and what they kept.
    #[test]
    fn a_new_operation_drops_the_undone_steps() {
        let root = fixture("drops");
        let mut history = FileUndo::new(&root);
        history.create(&root, "new.txt", NewKind::File).unwrap();
        history.undo().unwrap();
        assert_eq!(kept_items(&root), 1, "the undone file waits for redo");
        history.rename(&root.join("a.txt"), "b.txt").unwrap();
        assert!(history.next_redo().is_none());
        assert_eq!(kept_items(&root), 0, "nothing of the dropped step stays");
        assert_eq!(history.redo(), Err(UndoError::Nothing));

        // The oldest steps go when the history is full, with what they kept.
        std::fs::write(root.join("old.txt"), "old").unwrap();
        history.delete(&root.join("old.txt")).unwrap();
        assert_eq!(kept_items(&root), 1);
        for i in 0..MAX_STEPS {
            let from = if i == 0 {
                "b.txt".to_owned()
            } else {
                format!("r{}.txt", i - 1)
            };
            history
                .rename(&root.join(from), &format!("r{i}.txt"))
                .unwrap();
        }
        assert_eq!(history.steps().0.len(), MAX_STEPS);
        assert_eq!(kept_items(&root), 0);

        // A new history removes what an earlier session left.
        let mut earlier = FileUndo::new(&root);
        earlier.delete(&root.join("src/b.txt")).unwrap();
        std::mem::forget(earlier);
        assert_eq!(kept_items(&root), 1);
        let _fresh = FileUndo::new(&root);
        assert_eq!(kept_items(&root), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-35: an item that changed, or is gone, is never taken back.
    #[test]
    fn a_changed_file_refuses_undo() {
        let root = fixture("changed");
        let mut history = FileUndo::new(&root);
        history.create(&root, "new.txt", NewKind::File).unwrap();
        std::fs::write(root.join("new.txt"), "typed and saved").unwrap();
        let before = disk(&root);
        assert_eq!(history.undo(), Err(UndoError::Changed("new.txt".into())));
        assert!(
            UndoError::Changed("new.txt".into())
                .to_string()
                .contains("changed since")
        );
        assert_eq!(disk(&root), before, "nothing was touched");
        assert_eq!(history.next_undo().unwrap().label, "New File new.txt");
        // The reason gone, the same undo works.
        std::fs::write(root.join("new.txt"), "").unwrap();
        history.undo().unwrap();
        assert!(!root.join("new.txt").exists());

        // A renamed file written with other bytes; a moved one that is gone.
        history.rename(&root.join("a.txt"), "z.txt").unwrap();
        std::fs::write(root.join("z.txt"), "ALPHA\n").unwrap();
        assert_eq!(history.undo(), Err(UndoError::Changed("z.txt".into())));
        assert!(root.join("z.txt").exists() && !root.join("a.txt").exists());
        std::fs::remove_file(root.join("z.txt")).unwrap();
        assert_eq!(history.undo(), Err(UndoError::Gone("z.txt".into())));

        // Redo checks too: a file put back by undo and then edited is not deleted again.
        let mut history = FileUndo::new(&root);
        history.delete(&root.join("src/b.txt")).unwrap();
        history.undo().unwrap();
        std::fs::write(root.join("src/b.txt"), "edited\n").unwrap();
        assert_eq!(history.redo(), Err(UndoError::Changed("src/b.txt".into())));
        assert_eq!(
            std::fs::read_to_string(root.join("src/b.txt")).unwrap(),
            "edited\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-35: nothing existing is overwritten.
    #[test]
    fn an_occupied_place_refuses_undo() {
        let root = fixture("occupied");
        let mut history = FileUndo::new(&root);
        history.delete(&root.join("a.txt")).unwrap();
        std::fs::write(root.join("a.txt"), "someone else's").unwrap();
        assert_eq!(history.undo(), Err(UndoError::Occupied("a.txt".into())));
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "someone else's"
        );
        std::fs::remove_file(root.join("a.txt")).unwrap();
        history.undo().unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "alpha\n"
        );

        history.rename(&root.join("a.txt"), "z.txt").unwrap();
        std::fs::create_dir(root.join("a.txt")).unwrap();
        assert_eq!(history.undo(), Err(UndoError::Occupied("a.txt".into())));
        assert!(root.join("z.txt").is_file());

        // The folder a deleted file was in is gone: it is made again, inside the workspace.
        let mut history = FileUndo::new(&root);
        history.delete(&root.join("src/b.txt")).unwrap();
        std::fs::remove_dir(root.join("src")).unwrap();
        history.undo().unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("src/b.txt")).unwrap(),
            "beta\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-35: a kept copy that was changed or removed is not restored.
    #[test]
    fn a_changed_store_refuses_undo() {
        let root = fixture("store");
        let mut history = FileUndo::new(&root);
        history.delete(&root.join("a.txt")).unwrap();
        let session = std::fs::read_dir(root.join(UNDO_DIR))
            .unwrap()
            .flatten()
            .next()
            .unwrap()
            .path();
        let kept = session.join("1/a.txt");
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "alpha\n");
        std::fs::write(&kept, "planted\n").unwrap();
        assert_eq!(history.undo(), Err(UndoError::StoreChanged("a.txt".into())));
        assert!(
            !root.join("a.txt").exists(),
            "a changed copy never comes back"
        );
        std::fs::remove_file(&kept).unwrap();
        assert_eq!(history.undo(), Err(UndoError::StoreGone("a.txt".into())));
        // A link planted in place of the kept copy is not followed.
        #[cfg(unix)]
        {
            let outside = root.with_extension("outside");
            std::fs::write(&outside, "alpha\n").unwrap();
            std::os::unix::fs::symlink(&outside, &kept).unwrap();
            assert_eq!(history.undo(), Err(UndoError::StoreChanged("a.txt".into())));
            assert!(!root.join("a.txt").exists());
            let _ = std::fs::remove_file(&outside);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-35: the size limit.
    #[test]
    fn too_large_a_delete_is_not_kept() {
        let root = fixture("large");
        std::fs::write(root.join("big.bin"), vec![7u8; 4096]).unwrap();
        let mut history = FileUndo::new(&root).with_limit(1024);
        let deleted = history.delete(&root.join("big.bin")).unwrap();
        assert_eq!(
            deleted.not_kept,
            Some(NotKept::TooLarge {
                bytes: 4096,
                limit: 1024
            })
        );
        assert!(!root.join("big.bin").exists(), "deleted all the same");
        assert!(history.next_undo().is_none(), "and no step for it");
        assert_eq!(kept_items(&root), 0);
        let said = NotKept::TooLarge {
            bytes: 100 << 20,
            limit: MAX_KEPT_BYTES,
        }
        .to_string();
        assert!(
            said.contains("64 MB") && said.contains("cannot be undone"),
            "{said}"
        );
        // A small one beside it is kept.
        assert_eq!(history.delete(&root.join("a.txt")).unwrap().not_kept, None);
        history.undo().unwrap();
        assert!(root.join("a.txt").exists());
        // A large item is still renamed and renamed back: sizes and times stand for its bytes.
        std::fs::write(root.join("big2.bin"), vec![7u8; 4096]).unwrap();
        history.rename(&root.join("big2.bin"), "big3.bin").unwrap();
        history.undo().unwrap();
        assert!(root.join("big2.bin").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0275 v16 AK-35: the store is a real folder inside the workspace or it is not used.
    #[cfg(unix)]
    #[test]
    fn the_store_never_leaves_the_workspace() {
        let root = fixture("link");
        let outside = root.with_extension("elsewhere");
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(root.join(".idecraft")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".idecraft/history")).unwrap();
        let mut history = FileUndo::new(&root);
        let deleted = history.delete(&root.join("a.txt")).unwrap();
        assert_eq!(deleted.not_kept, Some(NotKept::NoStore));
        assert!(!root.join("a.txt").exists());
        assert!(history.next_undo().is_none());
        assert!(
            !outside.join("undo").exists(),
            "nothing of the undo store is written outside the workspace"
        );
        // Undo of New needs the store too and is refused without touching the file.
        history.create(&root, "new.txt", NewKind::File).unwrap();
        assert_eq!(history.undo(), Err(UndoError::NoStore));
        assert!(root.join("new.txt").exists());
        assert_eq!(history.next_undo().unwrap().label, "New File new.txt");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
