//! Creating, renaming, deleting, copying and moving files and folders of a workspace (`SPEC-0275`
//! behaviours 3–7 and 19–22), shared by the project tree and the MCP tools `create_file`,
//! `rename_file`, `delete_file`, `copy_file` and `move_file`.
//!
//! Names are checked before anything touches the disk; an existing file is never overwritten; the
//! workspace folder itself can be neither renamed, moved nor deleted; a deleted file keeps a Local
//! History revision so it can be restored (IMP-0275); copies and moves are recorded in the Local
//! History under a label (IMP-0280).

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::LocalHistory;

/// What "New" creates (behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NewKind {
    /// A file with the name as typed.
    File,
    /// A folder; `a/b` creates both levels.
    Directory,
    /// `.sql`
    SqlFile,
    /// `.http`
    HttpRequest,
    /// `.java` with a class of the file's name.
    JavaClass,
    /// `.kt`
    KotlinFile,
    /// `.py`
    PythonFile,
    /// `.ts`
    TypeScriptFile,
    /// `.js`
    JavaScriptFile,
}

impl NewKind {
    /// Every kind in the order of the New menu.
    pub const ALL: [NewKind; 9] = [
        NewKind::File,
        NewKind::Directory,
        NewKind::JavaClass,
        NewKind::KotlinFile,
        NewKind::PythonFile,
        NewKind::TypeScriptFile,
        NewKind::JavaScriptFile,
        NewKind::SqlFile,
        NewKind::HttpRequest,
    ];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            NewKind::File => "File",
            NewKind::Directory => "Directory",
            NewKind::SqlFile => "SQL File",
            NewKind::HttpRequest => "HTTP Request",
            NewKind::JavaClass => "Java Class",
            NewKind::KotlinFile => "Kotlin File",
            NewKind::PythonFile => "Python File",
            NewKind::TypeScriptFile => "TypeScript File",
            NewKind::JavaScriptFile => "JavaScript File",
        }
    }

    /// The id used by menus and the MCP tool.
    pub fn id(self) -> &'static str {
        match self {
            NewKind::File => "file",
            NewKind::Directory => "directory",
            NewKind::SqlFile => "sql",
            NewKind::HttpRequest => "http",
            NewKind::JavaClass => "java",
            NewKind::KotlinFile => "kotlin",
            NewKind::PythonFile => "python",
            NewKind::TypeScriptFile => "typescript",
            NewKind::JavaScriptFile => "javascript",
        }
    }

    /// The kind with this id.
    pub fn from_id(id: &str) -> Option<NewKind> {
        NewKind::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The extension added when the name lacks it, without the dot.
    pub fn extension(self) -> Option<&'static str> {
        match self {
            NewKind::File | NewKind::Directory => None,
            NewKind::SqlFile => Some("sql"),
            NewKind::HttpRequest => Some("http"),
            NewKind::JavaClass => Some("java"),
            NewKind::KotlinFile => Some("kt"),
            NewKind::PythonFile => Some("py"),
            NewKind::TypeScriptFile => Some("ts"),
            NewKind::JavaScriptFile => Some("js"),
        }
    }

    /// The icon of the menu entry (a file of that kind).
    pub fn sample_name(self) -> &'static str {
        match self {
            NewKind::File => "file.txt",
            NewKind::Directory => "",
            NewKind::SqlFile => "a.sql",
            NewKind::HttpRequest => "a.http",
            NewKind::JavaClass => "A.java",
            NewKind::KotlinFile => "a.kt",
            NewKind::PythonFile => "a.py",
            NewKind::TypeScriptFile => "a.ts",
            NewKind::JavaScriptFile => "a.js",
        }
    }

    /// Whether the name must be a class name (letters, digits, `_`, `$`, not starting with a digit).
    fn class_name(self) -> bool {
        matches!(self, NewKind::JavaClass)
    }
}

/// Why a file operation was refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOpError {
    /// The name is empty.
    EmptyName,
    /// The name cannot be used; the reason in words.
    InvalidName(String),
    /// Something with that name is already there.
    Exists(PathBuf),
    /// The path is not inside the workspace.
    OutsideWorkspace(PathBuf),
    /// The workspace folder itself was named.
    WorkspaceRoot,
    /// Nothing is at the path.
    NotFound(PathBuf),
    /// A folder was to be copied or moved into itself or a folder below it.
    IntoItself(PathBuf),
    /// The file system refused.
    Io(String),
}

impl fmt::Display for FileOpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileOpError::EmptyName => write!(f, "Enter a name"),
            FileOpError::InvalidName(why) => write!(f, "Invalid name: {why}"),
            FileOpError::Exists(p) => write!(f, "{} already exists", display_name(p)),
            FileOpError::OutsideWorkspace(p) => {
                write!(f, "{} is not inside the project", p.display())
            }
            FileOpError::WorkspaceRoot => {
                write!(
                    f,
                    "The project folder itself cannot be renamed, moved or deleted"
                )
            }
            FileOpError::NotFound(p) => write!(f, "{} does not exist", p.display()),
            FileOpError::IntoItself(p) => write!(
                f,
                "\"{}\" cannot be pasted into itself or a folder inside it",
                display_name(p)
            ),
            FileOpError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FileOpError {}

pub(crate) fn display_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

pub(crate) fn io(e: std::io::Error) -> FileOpError {
    FileOpError::Io(e.to_string())
}

/// Checks one path segment (a file or folder name).
pub fn validate_segment(name: &str) -> Result<(), FileOpError> {
    if name.is_empty() {
        return Err(FileOpError::EmptyName);
    }
    let invalid = |why: &str| Err(FileOpError::InvalidName(why.to_owned()));
    if name == "." || name == ".." {
        return invalid("`.` and `..` are not names");
    }
    if name.trim() != name {
        return invalid("starts or ends with a space");
    }
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, '/' | '\\' | ':' | '\0') || c.is_control())
    {
        return invalid(&format!("contains {c:?}"));
    }
    if name.len() > 255 {
        return invalid("longer than 255 bytes");
    }
    Ok(())
}

/// Checks a name typed into "New": segments separated by `/` (folders created on the way); a
/// class name for a Java class.
pub fn validate_name(name: &str, kind: NewKind) -> Result<(), FileOpError> {
    if name.trim().is_empty() {
        return Err(FileOpError::EmptyName);
    }
    if name.starts_with('/') || name.ends_with('/') {
        return Err(FileOpError::InvalidName(
            "starts or ends with `/`".to_owned(),
        ));
    }
    for segment in name.split('/') {
        validate_segment(segment)?;
    }
    if kind.class_name() {
        let last = name.rsplit('/').next().unwrap_or(name);
        let class = last.strip_suffix(".java").unwrap_or(last);
        let mut chars = class.chars();
        let ok = chars
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$');
        if !ok {
            return Err(FileOpError::InvalidName(format!(
                "`{class}` is not a class name"
            )));
        }
    }
    Ok(())
}

/// The name with the kind's extension added unless it already ends with it (any case).
pub fn with_extension(name: &str, kind: NewKind) -> String {
    match kind.extension() {
        Some(ext)
            if !name
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", ext.to_ascii_lowercase())) =>
        {
            format!("{name}.{ext}")
        }
        _ => name.to_owned(),
    }
}

/// Checks that `target`, which need not exist yet, stays inside `root` once every folder on its way
/// is followed: the nearest part of it that exists, with its links resolved, must lie inside the
/// root. A folder of the workspace that is a link to a place outside it is no way out
/// (`SPEC-0006` v57 behaviour 5).
pub fn stays_inside(root: &Path, target: &Path) -> Result<(), FileOpError> {
    let canonical_root = root.canonicalize().map_err(io)?;
    let outside = || FileOpError::OutsideWorkspace(target.to_path_buf());
    if target
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(outside());
    }
    let mut existing = target;
    while existing.symlink_metadata().is_err() {
        existing = existing.parent().ok_or_else(outside)?;
    }
    let resolved = existing.canonicalize().map_err(io)?;
    if resolved.starts_with(&canonical_root) {
        Ok(())
    } else {
        Err(outside())
    }
}

/// `path` made absolute against `root` and checked to lie inside it (without following a link
/// at the last component).
pub(crate) fn inside(root: &Path, path: &Path) -> Result<PathBuf, FileOpError> {
    let root = root.canonicalize().map_err(io)?;
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if joined
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(FileOpError::OutsideWorkspace(path.to_path_buf()));
    }
    let parent = joined.parent().unwrap_or(&joined);
    let parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    let resolved = match joined.file_name() {
        Some(name) => parent.join(name),
        None => parent,
    };
    if !resolved.starts_with(&root) {
        return Err(FileOpError::OutsideWorkspace(path.to_path_buf()));
    }
    Ok(resolved)
}

/// The package of a Java or Kotlin file in folder `dir`: the folders below a source root, joined
/// with dots. The source root is, in this order (`SPEC-0275` v11 behaviour 3): the Maven layout
/// on the way (`src/main/java`, `src/test/kotlin`, …), the source folder the files around say
/// (`SPEC-0048` v6 behaviour 14), a folder `src` on the way, a source folder derived for the
/// project.
pub fn package_of(root: &Path, dir: &Path) -> Option<String> {
    let rel = dir.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let below = |start: usize| {
        let package = parts.get(start..)?.join(".");
        (!package.is_empty()).then_some(package)
    };
    if let Some(i) = parts.windows(3).position(|w| {
        w[0] == "src" && matches!(w[1].as_str(), "main" | "test") && {
            matches!(w[2].as_str(), "java" | "kotlin")
        }
    }) {
        return below(i + 3);
    }
    if let Some(package) = crate::source_roots::package_from_neighbours(root, dir) {
        return Some(package);
    }
    if let Some(i) = parts.iter().position(|p| p == "src") {
        return below(i + 1);
    }
    // A package folder that holds only sub-packages: the project's derived source folders say
    // where the packages start.
    crate::source_roots::derived(root)
        .iter()
        .filter(|(_, info)| {
            matches!(
                info.role,
                crate::buildtools::FolderRole::Sources | crate::buildtools::FolderRole::Tests
            )
        })
        .find_map(|(source, _)| dir.strip_prefix(source).ok())
        .map(|inside| {
            inside
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(".")
        })
        .filter(|package| !package.is_empty())
}

/// What a new file of `kind` named `file` (with extension) in folder `dir` starts with.
pub fn template(root: &Path, dir: &Path, file: &str, kind: NewKind) -> String {
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let package = package_of(root, dir);
    match kind {
        NewKind::JavaClass => {
            let head = package
                .map(|p| format!("package {p};\n\n"))
                .unwrap_or_default();
            format!("{head}public class {stem} {{\n}}\n")
        }
        NewKind::KotlinFile => package
            .map(|p| format!("package {p}\n\n"))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Creates `name` of `kind` in folder `dir` of the workspace at `root` and returns its path.
/// Missing folders on the way (`a/b/c.txt`) are created; nothing existing is overwritten.
pub fn create(root: &Path, dir: &Path, name: &str, kind: NewKind) -> Result<PathBuf, FileOpError> {
    let name = name.trim_end();
    validate_name(name, kind)?;
    let dir = inside(root, dir)?;
    if !dir.is_dir() {
        return Err(FileOpError::NotFound(dir));
    }
    let name = if kind == NewKind::Directory {
        name.to_owned()
    } else {
        with_extension(name, kind)
    };
    let target = dir.join(&name);
    if target.symlink_metadata().is_ok() {
        return Err(FileOpError::Exists(target));
    }
    // The folders of `name` may run through a link; nothing is created outside the root.
    let in_folder = if kind == NewKind::Directory {
        target.clone()
    } else {
        target.parent().unwrap_or(&dir).to_path_buf()
    };
    stays_inside(root, &in_folder)?;
    std::fs::create_dir_all(&in_folder).map_err(io)?;
    // Checked again once the folders exist, in case one was replaced by a link meanwhile.
    stays_inside(root, &in_folder)?;
    if kind == NewKind::Directory {
        return Ok(target);
    }
    let parent = in_folder;
    let file = display_name(&target);
    let root = root.canonicalize().map_err(io)?;
    let text = template(&root, &parent, &file, kind);
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                FileOpError::Exists(target.clone())
            } else {
                io(e)
            }
        })?;
    f.write_all(text.as_bytes()).map_err(io)?;
    Ok(target)
}

/// Renames the file or folder `path` of the workspace at `root` to `new_name` in the same folder
/// and returns the new path. Only the case may change on a file system that ignores case.
pub fn rename(root: &Path, path: &Path, new_name: &str) -> Result<PathBuf, FileOpError> {
    validate_segment(new_name)?;
    let from = inside(root, path)?;
    if from == root.canonicalize().map_err(io)? {
        return Err(FileOpError::WorkspaceRoot);
    }
    if from.symlink_metadata().is_err() {
        return Err(FileOpError::NotFound(from));
    }
    let to = from.with_file_name(new_name);
    if to == from {
        return Ok(to);
    }
    let same_file = to
        .canonicalize()
        .ok()
        .zip(from.canonicalize().ok())
        .is_some_and(|(a, b)| a == b);
    if to.symlink_metadata().is_ok() && !same_file {
        return Err(FileOpError::Exists(to));
    }
    std::fs::rename(&from, &to).map_err(io)?;
    // Bookmarks stay with their file (SPEC-0331 behaviour 9).
    crate::bookmarks::follow_move(root, &from, &to);
    Ok(to)
}

/// What deleting a path takes with it, for the confirmation (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePlan {
    /// The path, absolute.
    pub path: PathBuf,
    /// Whether it is a folder.
    pub dir: bool,
    /// Files inside (the file itself for a file).
    pub files: usize,
    /// Folders inside, not counting the folder itself.
    pub dirs: usize,
}

impl DeletePlan {
    /// The question of the confirmation, e.g. `Delete folder "src" with 3 files and 1 folder?`.
    pub fn question(&self) -> String {
        let name = display_name(&self.path);
        if !self.dir {
            return format!("Delete file \"{name}\"?");
        }
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        if self.files == 0 && self.dirs == 0 {
            format!("Delete empty folder \"{name}\"?")
        } else {
            format!(
                "Delete folder \"{name}\" with {} and {}?",
                plural(self.files, "file", "files"),
                plural(self.dirs, "folder", "folders")
            )
        }
    }
}

/// Counts what deleting `path` would remove; refuses the workspace folder and paths outside it.
pub fn delete_plan(root: &Path, path: &Path) -> Result<DeletePlan, FileOpError> {
    let target = inside(root, path)?;
    if target == root.canonicalize().map_err(io)? {
        return Err(FileOpError::WorkspaceRoot);
    }
    let meta = target
        .symlink_metadata()
        .map_err(|_| FileOpError::NotFound(target.clone()))?;
    let dir = meta.is_dir();
    let (files, dirs) = count(&target, dir)?;
    Ok(DeletePlan {
        path: target,
        dir,
        files,
        dirs,
    })
}

/// Files and folders inside `path` (the file itself for a file); links count as files and are
/// never followed.
fn count(path: &Path, dir: bool) -> Result<(usize, usize), FileOpError> {
    let (mut files, mut dirs) = (usize::from(!dir), 0);
    if dir {
        let mut stack = vec![path.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).map_err(io)?.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    dirs += 1;
                    stack.push(entry.path());
                } else {
                    files += 1;
                }
            }
        }
    }
    Ok((files, dirs))
}

/// The regular files at or below `path`, links not followed.
fn files_below(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = p.symlink_metadata() else {
            continue;
        };
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                stack.extend(entries.flatten().map(|e| e.path()));
            }
        } else if meta.is_file() {
            out.push(p);
        }
    }
    out
}

/// Workspace-relative form of `path` with `/` separators, if it is inside `root`.
pub(crate) fn relative(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

/// Records every file at or below `path` in the Local History of the workspace at the canonical
/// `root`, as [`delete`] does before it removes them (IMP-0275); used where an item leaves the
/// project another way (`SPEC-0275` v16 behaviour 41).
pub(crate) fn record_deleted(root: &Path, path: &Path) {
    if let Ok(mut history) = LocalHistory::open(root) {
        for file in files_below(path) {
            if let Some(rel) = relative(root, &file) {
                let _ = history.record(&rel);
            }
        }
    }
}

/// Deletes `path` with everything in it; each file is first recorded in the Local History of the
/// workspace so it can be restored (IMP-0275). Returns what was deleted.
pub fn delete(root: &Path, path: &Path) -> Result<DeletePlan, FileOpError> {
    let plan = delete_plan(root, path)?;
    let root = root.canonicalize().map_err(io)?;
    record_deleted(&root, &plan.path);
    if plan.dir {
        std::fs::remove_dir_all(&plan.path).map_err(io)?;
    } else {
        std::fs::remove_file(&plan.path).map_err(io)?;
    }
    // A deleted file's bookmarks go with it (SPEC-0331 behaviour 9).
    crate::bookmarks::follow_delete(&root, &plan.path);
    Ok(plan)
}

/// The items of a selection without those inside another selected folder, in their order, each
/// once (`SPEC-0275` v3 behaviour 25): a folder takes what is inside it along.
pub fn outermost(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        let inside_other = paths.iter().any(|q| q != p && p.starts_with(q));
        if !inside_other && !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// The question of one confirmation for several items (behaviour 25), e.g.
/// `Delete 3 items: 5 files and 2 folders?`; one item asks as [`DeletePlan::question`].
pub fn delete_question(plans: &[DeletePlan]) -> String {
    if let [one] = plans {
        return one.question();
    }
    let files: usize = plans.iter().map(|p| p.files).sum();
    let dirs: usize = plans.iter().map(|p| p.dirs + usize::from(p.dir)).sum();
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "Delete {} items: {} and {}?",
        plans.len(),
        plural(files, "file", "files"),
        plural(dirs, "folder", "folders")
    )
}

/// Checks deleting every item of a selection (nested items once); nothing is deleted.
pub fn delete_plans(root: &Path, paths: &[PathBuf]) -> Result<Vec<DeletePlan>, FileOpError> {
    outermost(paths)
        .iter()
        .map(|p| delete_plan(root, p))
        .collect()
}

/// Deletes every item of a selection after checking all of them first (IMP-0281).
pub fn delete_all(root: &Path, paths: &[PathBuf]) -> Result<Vec<DeletePlan>, FileOpError> {
    let plans = delete_plans(root, paths)?;
    plans.iter().map(|p| delete(root, &p.path)).collect()
}

/// Copy or move, for Paste and the MCP tools (behaviours 19–23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transfer {
    /// The source stays; a copy is made.
    Copy,
    /// The source goes to the new place.
    Move,
}

impl Transfer {
    /// `Copy` or `Move`, for titles.
    pub fn verb(self) -> &'static str {
        match self {
            Transfer::Copy => "Copy",
            Transfer::Move => "Move",
        }
    }

    /// `Copied` or `Moved`, for the status bar and the Local History label.
    pub fn past(self) -> &'static str {
        match self {
            Transfer::Copy => "Copied",
            Transfer::Move => "Moved",
        }
    }
}

/// What copying or moving a path does (behaviours 19–21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPlan {
    /// Copy or move.
    pub mode: Transfer,
    /// The source, absolute.
    pub from: PathBuf,
    /// The new path, absolute.
    pub to: PathBuf,
    /// Whether the source is a folder.
    pub dir: bool,
    /// Files it takes along (the file itself for a file).
    pub files: usize,
    /// Folders inside, not counting the folder itself.
    pub dirs: usize,
}

impl TransferPlan {
    /// The Local History label and status line, e.g. `Moved src/a.txt to lib/a.txt`, with paths
    /// relative to `root`.
    pub fn summary(&self, root: &Path) -> String {
        let root = root.canonicalize().unwrap_or(root.to_path_buf());
        let show = |p: &Path| relative(&root, p).unwrap_or_else(|| p.display().to_string());
        format!(
            "{} {} to {}",
            self.mode.past(),
            show(&self.from),
            show(&self.to)
        )
    }
}

/// `name` if nothing of that name is in `dir`, else the first free `stem_copy.ext`,
/// `stem_copy2.ext`, … (folders and names without extension: `name_copy`, …) (behaviour 19).
pub fn free_name(dir: &Path, name: &str, is_dir: bool) -> String {
    free_name_besides(dir, name, is_dir, &[])
}

/// [`free_name`], also avoiding the names in `claimed` (earlier items of the same paste).
fn free_name_besides(dir: &Path, name: &str, is_dir: bool, claimed: &[String]) -> String {
    let taken = |n: &str| claimed.iter().any(|c| c == n) || dir.join(n).symlink_metadata().is_ok();
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 && !is_dir => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| {
            let suffix = if n == 1 {
                "_copy".to_owned()
            } else {
                format!("_copy{n}")
            };
            format!("{stem}{suffix}{ext}")
        })
        .find(|n| !taken(n))
        .unwrap_or_else(|| name.to_owned())
}

/// Checks copying or moving `from` into folder `to_dir` under `new_name` (default: its own name)
/// and counts what goes; nothing is written (behaviours 19–20). A folder into itself, the project
/// folder, a missing source or folder, a path outside the workspace and an existing name are
/// refused.
pub fn transfer_plan(
    root: &Path,
    from: &Path,
    to_dir: &Path,
    new_name: Option<&str>,
    mode: Transfer,
) -> Result<TransferPlan, FileOpError> {
    let canonical_root = root.canonicalize().map_err(io)?;
    let from = inside(root, from)?;
    let meta = from
        .symlink_metadata()
        .map_err(|_| FileOpError::NotFound(from.clone()))?;
    let dir = meta.is_dir();
    if from == canonical_root {
        return Err(match mode {
            Transfer::Move => FileOpError::WorkspaceRoot,
            Transfer::Copy => FileOpError::IntoItself(from),
        });
    }
    let target_dir = inside(root, to_dir)?;
    if !target_dir.is_dir() {
        return Err(FileOpError::NotFound(target_dir));
    }
    let target_dir = target_dir.canonicalize().map_err(io)?;
    if dir && target_dir.starts_with(&from) {
        return Err(FileOpError::IntoItself(from));
    }
    let own = display_name(&from);
    let name = new_name.map(str::trim_end).unwrap_or(&own);
    validate_segment(name)?;
    let to = target_dir.join(name);
    if to.symlink_metadata().is_ok() {
        return Err(FileOpError::Exists(to));
    }
    let (files, dirs) = count(&from, dir)?;
    Ok(TransferPlan {
        mode,
        from,
        to,
        dir,
        files,
        dirs,
    })
}

/// Checks pasting several items into `to_dir`, each under its own name (`SPEC-0275` v3 behaviour
/// 25, IMP-0281): items inside another item count once; a copy whose name is taken, on disk or by
/// an earlier item, gets a free name; a move onto a taken name is refused naming it. Nothing is
/// written; the first item that cannot be pasted is the error.
pub fn transfer_plans(
    root: &Path,
    from: &[PathBuf],
    to_dir: &Path,
    mode: Transfer,
) -> Result<Vec<TransferPlan>, FileOpError> {
    let target = inside(root, to_dir)?;
    let mut claimed: Vec<String> = Vec::new();
    let mut plans = Vec::new();
    for item in outermost(from) {
        let own = display_name(&item);
        let is_dir = item.is_dir();
        let name = match mode {
            Transfer::Copy => free_name_besides(&target, &own, is_dir, &claimed),
            Transfer::Move if claimed.contains(&own) => {
                return Err(FileOpError::Exists(target.join(&own)));
            }
            Transfer::Move => own,
        };
        let plan = transfer_plan(root, &item, to_dir, Some(&name), mode)?;
        claimed.push(name);
        plans.push(plan);
    }
    Ok(plans)
}

/// Pastes several items as [`transfer_plans`] checked them: all are checked before the first is
/// written. Returns what was done.
pub fn transfer_all(
    root: &Path,
    from: &[PathBuf],
    to_dir: &Path,
    mode: Transfer,
) -> Result<Vec<TransferPlan>, FileOpError> {
    let plans = transfer_plans(root, from, to_dir, mode)?;
    plans
        .iter()
        .map(|p| {
            let name = display_name(&p.to);
            transfer(root, &p.from, to_dir, Some(&name), mode)
        })
        .collect()
}

/// Copies `from` to `to` (which must not exist): folders with everything in them, links as links.
fn copy_tree(from: &Path, to: &Path) -> Result<(), FileOpError> {
    let meta = from.symlink_metadata().map_err(io)?;
    let exists = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            FileOpError::Exists(to.to_path_buf())
        } else {
            io(e)
        }
    };
    if meta.file_type().is_symlink() {
        let link = std::fs::read_link(from).map_err(io)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&link, to).map_err(exists)?;
        #[cfg(windows)]
        if from.is_dir() {
            std::os::windows::fs::symlink_dir(&link, to).map_err(exists)?;
        } else {
            std::os::windows::fs::symlink_file(&link, to).map_err(exists)?;
        }
        return Ok(());
    }
    if meta.is_dir() {
        std::fs::create_dir(to).map_err(exists)?;
        let mut names: Vec<_> = std::fs::read_dir(from)
            .map_err(io)?
            .flatten()
            .map(|e| e.file_name())
            .collect();
        names.sort();
        for name in names {
            copy_tree(&from.join(&name), &to.join(&name))?;
        }
        return Ok(());
    }
    let mut source = std::fs::File::open(from).map_err(io)?;
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(exists)?;
    std::io::copy(&mut source, &mut target).map_err(io)?;
    let _ = target.set_permissions(meta.permissions());
    Ok(())
}

/// Copies or moves `from` into folder `to_dir` under `new_name` (default: its own name) and
/// returns what was done (behaviours 19–22). Nothing existing is overwritten. The Local History
/// gets the old paths before a move, then the new files (and the old paths as deleted), and a
/// workspace label with [`TransferPlan::summary`].
pub fn transfer(
    root: &Path,
    from: &Path,
    to_dir: &Path,
    new_name: Option<&str>,
    mode: Transfer,
) -> Result<TransferPlan, FileOpError> {
    let plan = transfer_plan(root, from, to_dir, new_name, mode)?;
    let root = root.canonicalize().map_err(io)?;
    let mut history = LocalHistory::open(&root).ok();
    let old: Vec<String> = files_below(&plan.from)
        .iter()
        .filter_map(|f| relative(&root, f))
        .collect();
    let mut record = |paths: &[String]| {
        if let Some(h) = history.as_mut() {
            for p in paths {
                let _ = h.record(p);
            }
        }
    };
    if mode == Transfer::Move {
        record(&old);
    }
    match mode {
        Transfer::Copy => copy_tree(&plan.from, &plan.to)?,
        Transfer::Move => match std::fs::rename(&plan.from, &plan.to) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                copy_tree(&plan.from, &plan.to)?;
                if plan.dir {
                    std::fs::remove_dir_all(&plan.from).map_err(io)?;
                } else {
                    std::fs::remove_file(&plan.from).map_err(io)?;
                }
            }
            Err(e) => return Err(io(e)),
        },
    }
    let new: Vec<String> = files_below(&plan.to)
        .iter()
        .filter_map(|f| relative(&root, f))
        .collect();
    if mode == Transfer::Move {
        record(&old);
        // Bookmarks stay with their file (SPEC-0331 behaviour 9).
        crate::bookmarks::follow_move(&root, &plan.from, &plan.to);
    }
    record(&new);
    if let Some(h) = history.as_mut() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let _ = h.put_label_at(None, &plan.summary(&root), now);
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0275 v3 AK-14 and AK-16 (core part).
    #[test]
    fn outermost_items_and_the_question_for_several() {
        let root = fixture("several");
        std::fs::create_dir_all(root.join("src/inner")).unwrap();
        std::fs::write(root.join("src/inner/x.txt"), "x").unwrap();
        std::fs::write(root.join("src/y.txt"), "y").unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let picked = vec![
            root.join("src/inner/x.txt"),
            root.join("src"),
            root.join("a.txt"),
            root.join("src"),
        ];
        assert_eq!(outermost(&picked), [root.join("src"), root.join("a.txt")]);
        let plans = delete_plans(&root, &picked).unwrap();
        assert_eq!(
            delete_question(&plans),
            "Delete 2 items: 3 files and 6 folders?"
        );
        assert_eq!(
            delete_question(&plans[1..]),
            "Delete file \"a.txt\"?",
            "one item asks as before"
        );
        // Copies of two items of the same name get free names; nothing is written by the check.
        std::fs::write(root.join("lib/a.txt"), "other").unwrap();
        let both = vec![root.join("a.txt"), root.join("lib/a.txt")];
        let copies = transfer_plans(&root, &both, &root.join("src"), Transfer::Copy).unwrap();
        let names: Vec<String> = copies.iter().map(|p| display_name(&p.to)).collect();
        assert_eq!(names, ["a.txt", "a_copy.txt"]);
        assert!(!root.join("src/a.txt").exists());
        // A move of both is refused as a whole, naming the item; nothing moved.
        let err = transfer_all(&root, &both, &root.join("src"), Transfer::Move).unwrap_err();
        assert_eq!(err.to_string(), "a.txt already exists");
        assert!(root.join("a.txt").exists() && root.join("lib/a.txt").exists());
        // A clash with a file already there stops the whole move before anything is written.
        std::fs::write(root.join("lib/y.txt"), "clash").unwrap();
        let mixed = vec![root.join("a.txt"), root.join("src/y.txt")];
        assert!(transfer_all(&root, &mixed, &root.join("lib"), Transfer::Move).is_err());
        assert!(root.join("a.txt").exists(), "the first item stayed");
        // Without a clash all move; deleting several asks once and deletes all.
        let done = transfer_all(
            &root,
            &copies_from(&root),
            &root.join("lib"),
            Transfer::Copy,
        );
        assert_eq!(done.unwrap().len(), 2);
        let gone = delete_all(&root, &[root.join("lib/a_copy.txt"), root.join("src")]).unwrap();
        assert_eq!(gone.len(), 2);
        assert!(!root.join("src").exists() && !root.join("lib/a_copy.txt").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    fn copies_from(root: &Path) -> Vec<PathBuf> {
        vec![root.join("a.txt"), root.join("src/inner")]
    }

    #[test]
    fn free_names_never_overwrite() {
        let root = fixture("free");
        assert_eq!(free_name(&root, "b.txt", false), "b.txt");
        assert_eq!(free_name(&root, "a.txt", false), "a_copy.txt");
        std::fs::write(root.join("a_copy.txt"), "").unwrap();
        assert_eq!(free_name(&root, "a.txt", false), "a_copy2.txt");
        assert_eq!(free_name(&root, "src", true), "src_copy");
        std::fs::write(root.join(".env"), "").unwrap();
        assert_eq!(free_name(&root, ".env", false), ".env_copy");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn copies_files_and_folders_recursively() {
        let root = fixture("copy");
        let pkg = root.join("src/main/java/com/acme");
        std::fs::write(pkg.join("A.java"), "class A {}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("A.java", pkg.join("link.java")).unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let plan = transfer_plan(
            &root,
            &root.join("src"),
            &root.join("lib"),
            None,
            Transfer::Copy,
        )
        .unwrap();
        assert_eq!(plan.to, root.join("lib/src"));
        assert_eq!((plan.dir, plan.dirs), (true, 4));
        assert!(!root.join("lib/src").exists(), "a plan writes nothing");
        transfer(
            &root,
            &root.join("src"),
            &root.join("lib"),
            None,
            Transfer::Copy,
        )
        .unwrap();
        let copied = root.join("lib/src/main/java/com/acme");
        assert_eq!(
            std::fs::read_to_string(copied.join("A.java")).unwrap(),
            "class A {}"
        );
        assert!(pkg.join("A.java").is_file(), "the source stays");
        #[cfg(unix)]
        assert!(
            copied
                .join("link.java")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "a link stays a link"
        );
        // A file into its own folder under a free name.
        let name = free_name(&root, "a.txt", false);
        let done = transfer(
            &root,
            &root.join("a.txt"),
            &root,
            Some(&name),
            Transfer::Copy,
        )
        .unwrap();
        assert_eq!(done.to, root.join("a_copy.txt"));
        assert_eq!(std::fs::read_to_string(&done.to).unwrap(), "a");
        assert_eq!(
            done.summary(&root),
            "Copied a.txt to a_copy.txt",
            "the Local History label"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn moves_and_records_local_history() {
        let root = fixture("move");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let plan = transfer(
            &root,
            &root.join("a.txt"),
            &root.join("lib"),
            Some("b.txt"),
            Transfer::Move,
        )
        .unwrap();
        assert_eq!(plan.to, root.join("lib/b.txt"));
        assert!(!root.join("a.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("lib/b.txt")).unwrap(),
            "a"
        );
        let history = LocalHistory::open(&root).unwrap();
        let old = history.revisions("a.txt");
        assert!(
            old.iter().any(|r| r.deleted),
            "the old path is marked deleted"
        );
        let content = old
            .iter()
            .find(|r| !r.deleted && !r.workspace_label)
            .expect("the old content is kept");
        assert_eq!(
            history.content("a.txt", content.id).unwrap(),
            Some(b"a".to_vec())
        );
        assert!(
            old.iter()
                .any(|r| r.label.as_deref() == Some("Moved a.txt to lib/b.txt")),
            "the label marks the step: {old:?}"
        );
        assert!(!history.revisions("lib/b.txt").is_empty());
        // A folder with its files; the old paths keep their content.
        std::fs::write(root.join("src/main/java/com/acme/A.java"), "class A {}").unwrap();
        transfer(
            &root,
            &root.join("src/main"),
            &root.join("lib"),
            None,
            Transfer::Move,
        )
        .unwrap();
        assert!(root.join("lib/main/java/com/acme/A.java").is_file());
        assert!(!root.join("src/main").exists());
        let history = LocalHistory::open(&root).unwrap();
        assert!(
            !history
                .revisions("src/main/java/com/acme/A.java")
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_into_itself_root_and_clashes() {
        let root = fixture("refuse");
        let src = root.join("src");
        assert_eq!(
            transfer_plan(&root, &src, &src.join("main"), None, Transfer::Copy),
            Err(FileOpError::IntoItself(src.clone()))
        );
        assert_eq!(
            transfer_plan(&root, &src, &src, None, Transfer::Move),
            Err(FileOpError::IntoItself(src.clone()))
        );
        assert_eq!(
            FileOpError::IntoItself(src.clone()).to_string(),
            "\"src\" cannot be pasted into itself or a folder inside it"
        );
        assert_eq!(
            transfer_plan(&root, &root, &src, None, Transfer::Move),
            Err(FileOpError::WorkspaceRoot)
        );
        assert!(matches!(
            transfer_plan(&root, &root, &src, None, Transfer::Copy),
            Err(FileOpError::IntoItself(_))
        ));
        assert_eq!(
            transfer(&root, &root.join("a.txt"), &root, None, Transfer::Copy),
            Err(FileOpError::Exists(root.join("a.txt"))),
            "never overwrites"
        );
        std::fs::write(src.join("a.txt"), "other").unwrap();
        assert_eq!(
            transfer(&root, &root.join("a.txt"), &src, None, Transfer::Move),
            Err(FileOpError::Exists(src.join("a.txt")))
        );
        assert_eq!(std::fs::read_to_string(src.join("a.txt")).unwrap(), "other");
        assert!(root.join("a.txt").is_file());
        assert!(matches!(
            transfer_plan(
                &root,
                &root.join("a.txt"),
                &src,
                Some("x:y"),
                Transfer::Copy
            ),
            Err(FileOpError::InvalidName(_))
        ));
        assert!(matches!(
            transfer_plan(&root, &root.join("gone"), &src, None, Transfer::Copy),
            Err(FileOpError::NotFound(_))
        ));
        assert!(matches!(
            transfer_plan(
                &root,
                &root.join("a.txt"),
                Path::new("/tmp"),
                None,
                Transfer::Copy
            ),
            Err(FileOpError::OutsideWorkspace(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-file-ops-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/main/java/com/acme")).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn typed_kinds_add_their_extension_once() {
        assert_eq!(with_extension("query", NewKind::SqlFile), "query.sql");
        assert_eq!(with_extension("query.SQL", NewKind::SqlFile), "query.SQL");
        assert_eq!(with_extension("api", NewKind::HttpRequest), "api.http");
        assert_eq!(with_extension("Main", NewKind::KotlinFile), "Main.kt");
        assert_eq!(with_extension("tool", NewKind::PythonFile), "tool.py");
        assert_eq!(with_extension("app", NewKind::TypeScriptFile), "app.ts");
        assert_eq!(with_extension("app", NewKind::JavaScriptFile), "app.js");
        assert_eq!(with_extension("notes", NewKind::File), "notes");
        for kind in NewKind::ALL {
            assert_eq!(NewKind::from_id(kind.id()), Some(kind));
        }
    }

    #[test]
    fn invalid_names_are_refused() {
        for bad in [
            "", "  ", ".", "..", "a\\b", "a:b", " a", "a ", "/a", "a/", "a//b", "a\u{7}",
        ] {
            assert!(validate_name(bad, NewKind::File).is_err(), "{bad:?}");
        }
        assert!(validate_name("a/b/c.txt", NewKind::File).is_ok());
        assert!(validate_name(&"x".repeat(256), NewKind::File).is_err());
        assert!(validate_name("Foo", NewKind::JavaClass).is_ok());
        assert!(validate_name("Foo.java", NewKind::JavaClass).is_ok());
        assert!(validate_name("1Foo", NewKind::JavaClass).is_err());
        assert!(validate_name("Foo-Bar", NewKind::JavaClass).is_err());
        assert!(validate_name("com/acme/Foo", NewKind::JavaClass).is_ok());
    }

    #[test]
    fn creates_files_and_folders_and_refuses_collisions() {
        let root = fixture("create");
        let sql = create(&root, &root, "query", NewKind::SqlFile).unwrap();
        assert_eq!(sql, root.join("query.sql"));
        assert_eq!(std::fs::read_to_string(&sql).unwrap(), "");
        assert_eq!(
            create(&root, &root, "query", NewKind::SqlFile),
            Err(FileOpError::Exists(root.join("query.sql")))
        );
        assert_eq!(
            create(&root, &root, "a.txt", NewKind::File),
            Err(FileOpError::Exists(root.join("a.txt")))
        );
        let nested = create(&root, &root, "docs/notes/today.md", NewKind::File).unwrap();
        assert!(nested.is_file());
        let dir = create(&root, &root, "x/y", NewKind::Directory).unwrap();
        assert!(dir.is_dir());
        assert!(matches!(
            create(&root, &root, "x/y", NewKind::Directory),
            Err(FileOpError::Exists(_))
        ));
        assert_eq!(
            create(&root, &root, "a:b", NewKind::File),
            Err(FileOpError::InvalidName("contains ':'".into()))
        );
        assert!(matches!(
            create(&root, &root.join("missing"), "a", NewKind::File),
            Err(FileOpError::NotFound(_))
        ));
        assert!(matches!(
            create(&root, Path::new("/tmp"), "a", NewKind::File),
            Err(FileOpError::OutsideWorkspace(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    /// `SPEC-0006` v57 behaviour 5: a folder of the workspace that links outside it is no way
    /// out for the folders of a new name (OBS-0082).
    #[cfg(unix)]
    #[test]
    fn create_never_writes_through_a_link_to_outside() {
        let root = fixture("create-link");
        let outside = std::env::temp_dir().join(format!(
            "ferris-idea-file-ops-create-link-outside-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        for (name, kind) in [
            ("out/new/x.txt", NewKind::File),
            ("out/x.txt", NewKind::File),
            ("out/d1/d2", NewKind::Directory),
        ] {
            assert!(
                matches!(
                    create(&root, &root, name, kind),
                    Err(FileOpError::OutsideWorkspace(_))
                ),
                "{name}"
            );
        }
        assert_eq!(
            std::fs::read_dir(&outside).unwrap().count(),
            0,
            "nothing outside"
        );
        // A link to a folder inside the workspace stays usable.
        std::fs::create_dir(root.join("inner")).unwrap();
        std::os::unix::fs::symlink(root.join("inner"), root.join("in")).unwrap();
        assert!(create(&root, &root, "in/sub/y.txt", NewKind::File).is_ok());
        assert!(root.join("inner/sub/y.txt").is_file());
        assert!(stays_inside(&root, &root.join("brand/new/z.txt")).is_ok());
        assert!(stays_inside(&root, &root.join("out/z.txt")).is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn java_classes_get_their_package_and_class() {
        let root = fixture("java");
        let dir = root.join("src/main/java/com/acme");
        let file = create(&root, &dir, "Shop", NewKind::JavaClass).unwrap();
        assert_eq!(file, dir.join("Shop.java"));
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            "package com.acme;\n\npublic class Shop {\n}\n"
        );
        let kt = create(&root, &dir, "Util", NewKind::KotlinFile).unwrap();
        assert_eq!(std::fs::read_to_string(kt).unwrap(), "package com.acme\n\n");
        assert_eq!(package_of(&root, &root), None);
        let _ = std::fs::remove_dir_all(root);
    }

    // SPEC-0275/AK-28 (v11, behaviour 3; SPEC-0048 v6 behaviour 14).
    #[test]
    fn java_classes_get_their_package_from_the_files_around() {
        let root = fixture("java-around");
        let write = |file: &str, text: &str| {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        // The project is its own repository, wherever the temporary folder lies.
        std::fs::create_dir_all(root.join(".git")).unwrap();
        write(
            "app/code/lab/inventory/Inventory.java",
            "package lab.inventory;\npublic class Inventory {}\n",
        );
        write("src/app/shop/Shop.java", "package shop;\nclass Shop {}\n");
        // Beside a file with a package, without any `src` folder on the way.
        let dir = root.join("app/code/lab/inventory");
        let file = create(&root, &dir, "Stock", NewKind::JavaClass).unwrap();
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            "package lab.inventory;\n\npublic class Stock {\n}\n"
        );
        // In a new folder below it, and in a package folder that holds only sub-packages.
        let kt = create(&root, &dir, "util/Text", NewKind::KotlinFile).unwrap();
        assert_eq!(
            std::fs::read_to_string(kt).unwrap(),
            "package lab.inventory.util\n\n"
        );
        assert_eq!(
            package_of(&root, &root.join("app/code/lab")).as_deref(),
            Some("lab")
        );
        assert_eq!(package_of(&root, &root.join("app/code")), None);
        // The files around count before a folder named `src`: its packages start at `src/app`.
        assert_eq!(
            package_of(&root, &root.join("src/app/shop")).as_deref(),
            Some("shop")
        );
        // Without such files `src` is the source root, as before.
        assert_eq!(
            package_of(&root, &root.join("other/src/a/b")).as_deref(),
            Some("a.b")
        );
        assert_eq!(package_of(&root, &root.join("docs/notes")), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn renames_in_place_and_refuses_collisions() {
        let root = fixture("rename");
        std::fs::write(root.join("b.txt"), "b").unwrap();
        assert_eq!(
            rename(&root, &root.join("a.txt"), "b.txt"),
            Err(FileOpError::Exists(root.join("b.txt")))
        );
        let to = rename(&root, &root.join("a.txt"), "c.txt").unwrap();
        assert_eq!(to, root.join("c.txt"));
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "a");
        assert!(!root.join("a.txt").exists());
        assert!(rename(&root, &to, "x/y").is_err(), "a name, not a path");
        assert_eq!(rename(&root, &root, "z"), Err(FileOpError::WorkspaceRoot));
        assert!(matches!(
            rename(&root, &root.join("gone.txt"), "z"),
            Err(FileOpError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn deletes_with_a_count_and_keeps_local_history() {
        let root = fixture("delete");
        std::fs::write(root.join("src/main/java/com/acme/A.java"), "class A {}").unwrap();
        let plan = delete_plan(&root, &root.join("src")).unwrap();
        assert_eq!((plan.dir, plan.files, plan.dirs), (true, 1, 4));
        assert_eq!(
            plan.question(),
            "Delete folder \"src\" with 1 file and 4 folders?"
        );
        assert_eq!(
            delete_plan(&root, &root.join("a.txt")).unwrap().question(),
            "Delete file \"a.txt\"?"
        );
        assert_eq!(delete_plan(&root, &root), Err(FileOpError::WorkspaceRoot));
        delete(&root, &root.join("src")).unwrap();
        assert!(!root.join("src").exists());
        let history = LocalHistory::open(&root).unwrap();
        let revisions = history.revisions("src/main/java/com/acme/A.java");
        assert_eq!(revisions.len(), 1, "the deleted file can be restored");
        assert_eq!(
            history
                .content("src/main/java/com/acme/A.java", revisions[0].id)
                .unwrap(),
            Some(b"class A {}".to_vec())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
