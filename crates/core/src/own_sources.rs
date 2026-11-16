//! The project's own sources (`SPEC-0148` v3 behaviour 7): which files below a project folder are
//! the project's to change, and which are installed packages, environments, build output, data of
//! version control or of an IDE, excluded by the project model or ignored by Git. A file Git
//! tracks is the project's own even when an ignore rule matches it (v4). A path is judged by the
//! names it has on disk, not by how it was spelled, the fixed folder names count in any spelling of
//! upper and lower case, and on a file system that ignores case the ignore rules do too (v5).
//!
//! One place for the decision, so that project-wide actions (a rename with text occurrences
//! first, Find in Files and Replace in Files since `SPEC-0029` v2) do not each keep a list. When unsure, a file is not the project's own.
//! Ignore rules and Git's index are read from the files; no `git` process is started.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Serialize;

use crate::EXCLUDED_DIRS;

/// Why a file is not one of the project's own sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotOwn {
    /// Not below the project folder.
    OutsideProject,
    /// Reached through a symbolic link, which may lead anywhere.
    SymbolicLink,
    /// Data of a version control system (`.git`).
    VersionControl,
    /// Data of an IDE (`.idecraft`, `.idea`).
    IdeData,
    /// A virtual environment or the packages of an interpreter (`.venv`, `site-packages`).
    VirtualEnvironment,
    /// Installed dependencies (`node_modules`, `vendor`).
    Dependencies,
    /// Build output or a cache (`target`, `__pycache__`).
    BuildOutput,
    /// A folder the project model marks as excluded.
    ExcludedByProject,
    /// Matched by an ignore rule of Git.
    Ignored,
}

impl NotOwn {
    /// The reason in words, for a preview.
    pub fn reason(self) -> &'static str {
        match self {
            NotOwn::OutsideProject => "outside the project folder",
            NotOwn::SymbolicLink => "behind a symbolic link",
            NotOwn::VersionControl => "version control data",
            NotOwn::IdeData => "IDE data",
            NotOwn::VirtualEnvironment => "in a virtual environment",
            NotOwn::Dependencies => "in installed dependencies",
            NotOwn::BuildOutput => "build output",
            NotOwn::ExcludedByProject => "in a folder the project excludes",
            NotOwn::Ignored => "ignored by .gitignore",
        }
    }
}

const VERSION_CONTROL: &[&str] = &[".git", ".hg", ".svn", ".jj", "CVS"];
const IDE_DATA: &[&str] = &[".idecraft", ".idea"];
const ENVIRONMENTS: &[&str] = &[
    ".venv",
    "venv",
    "site-packages",
    ".tox",
    ".nox",
    "__pypackages__",
];
const DEPENDENCIES: &[&str] = &[
    "node_modules",
    "bower_components",
    ".gradle",
    "Pods",
    ".bundle",
];
const OUTPUT: &[&str] = &[
    "target",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
];
/// Build output, unless a folder named `src` lies above (a package may be called `build`).
const OUTPUT_OUTSIDE_SRC: &[&str] = &["build", "out", "dist"];
/// A `vendor` folder holds dependencies when one of these lies next to it …
const VENDOR_MANIFESTS: &[&str] = &["go.mod", "composer.json", "Gemfile", "Cargo.toml"];
/// … or one of these inside it.
const VENDOR_MARKERS: &[&str] = &["modules.txt", "autoload.php"];

/// `name` as a file system that ignores case compares it: upper and lower case of every letter
/// are one (`SPEC-0148` v5 behaviour 8). Upper case first, so that letters with only a lower-case
/// form of their own (`ſ`, `ı`) meet the letter they stand for.
pub fn fold_name(name: &str) -> String {
    name.to_uppercase().to_lowercase()
}

/// Whether `name` is one of the fixed folder names in `list`, in any spelling of upper and lower
/// case, on every platform (`SPEC-0148` v5 behaviour 8).
pub fn is_fixed_name(name: &str, list: &[&str]) -> bool {
    list.contains(&name) || {
        let folded = fold_name(name);
        list.iter().any(|n| fold_name(n) == folded)
    }
}

/// Whether the file system at `root` (canonical) ignores upper and lower case: the folder is
/// asked for under its name with the case of every ASCII letter flipped, and it is the same entry
/// (`SPEC-0148` v5 behaviour 8). A name without such a letter passes the question to the folder
/// above; without any, the answer is the platform's usual one.
fn ignores_case(root: &Path) -> bool {
    for dir in root.ancestors() {
        let (Some(name), Some(parent)) = (dir.file_name().and_then(|n| n.to_str()), dir.parent())
        else {
            continue;
        };
        let flipped: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_lowercase() {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();
        if flipped != name {
            return same_entry(dir, &parent.join(flipped));
        }
    }
    cfg!(any(target_os = "macos", windows))
}

/// Whether two spellings name one entry of a folder; a symbolic link is not followed.
#[cfg(unix)]
fn same_entry(a: &Path, b: &Path) -> bool {
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(a), Ok(b)) => same_file(&a, &b),
        _ => false,
    }
}

/// Whether two spellings name one entry of a folder.
#[cfg(not(unix))]
fn same_entry(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Whether `a` and `b` describe the same file: on Unix the same device and inode. Elsewhere the
/// standard library gives no identity, and kind, size and times are compared, which a prepared
/// swap can meet (`SPEC-0156` v4 behaviour 14 names the gap).
pub fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        a.file_type() == b.file_type()
            && a.len() == b.len()
            && a.modified().ok() == b.modified().ok()
            && a.created().ok() == b.created().ok()
    }
}

/// Opens the regular file at `path` for reading without following a symbolic link at its last
/// name, and returns it with what the handle says about it (`SPEC-0156` v4 behaviour 14): a link
/// put there after a check is not followed, and a caller compares the answer with what it checked
/// ([`same_file`]). On Windows a link or junction is opened as itself and refused. A folder, a
/// pipe or a device is refused; opening one does not wait.
pub fn open_regular(path: &Path) -> std::io::Result<(std::fs::File, std::fs::Metadata)> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: the link itself, not what it points to.
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok((file, meta))
}

/// Why a program is not started in a folder ([`folder_below`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderProblem {
    /// Not a relative path of plain names.
    Form,
    /// A symbolic link stands on the way, or the folder is one.
    SymbolicLink,
    /// Outside the project folder after resolving.
    Outside,
    /// Nothing is there (yet).
    Missing,
    /// Something else than a folder.
    NotAFolder,
}

impl std::fmt::Display for FolderProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Form => "is not a relative path of plain names",
            Self::SymbolicLink => "is reached through a symbolic link",
            Self::Outside => "leads out of the project folder",
            Self::Missing => "does not exist",
            Self::NotAFolder => "is not a folder",
        })
    }
}

/// The folder `relative` below the project folder `root` when a program may be started in it
/// (`SPEC-0276` v3 behaviour 3): its resolved path, or why not — not relative and made of plain
/// names, behind a symbolic link (the walk of [`OwnSources::readable`]), outside the project after
/// resolving, missing, or no folder. `.` is the project folder.
pub fn folder_below(root: &Path, relative: &Path) -> Result<PathBuf, FolderProblem> {
    let root = root.canonicalize().map_err(|_| FolderProblem::Missing)?;
    let mut at = root.clone();
    for part in relative.components() {
        match part {
            Component::CurDir => continue,
            Component::Normal(name) => at.push(name),
            _ => return Err(FolderProblem::Form),
        }
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(FolderProblem::SymbolicLink);
        }
    }
    let real = at.canonicalize().map_err(|_| FolderProblem::Missing)?;
    if !real.starts_with(&root) {
        return Err(FolderProblem::Outside);
    }
    if !real.is_dir() {
        return Err(FolderProblem::NotAFolder);
    }
    Ok(real)
}

/// A folder or a file a listing left out because it is not the project's own (`SPEC-0029` v2
/// behaviour 10): the topmost folder, or the single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LeftOut {
    /// Relative to the project folder.
    pub path: PathBuf,
    /// Whether it is a folder.
    pub folder: bool,
    /// Why it is not the project's own.
    pub why: NotOwn,
}

/// The project's own files with what was left out next to them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// The project's own files, relative to the project folder and sorted.
    pub files: Vec<PathBuf>,
    /// What is not the project's own, sorted by path. Version control data and IDECraft's own
    /// data are not named: nothing lists them ([`OwnSources::everything`] neither).
    pub left_out: Vec<LeftOut>,
}

impl Listing {
    /// Folders and files left out.
    pub fn left_out_counts(&self) -> (usize, usize) {
        let folders = self.left_out.iter().filter(|o| o.folder).count();
        (folders, self.left_out.len() - folders)
    }

    /// One line on what a list of files leaves out, `2 folders and 1 file not listed (libraries
    /// and ignored files)`; `None` when nothing was left out (Go to File and Search Everywhere,
    /// `SPEC-0031` v5 behaviour 12).
    pub fn not_listed_line(&self) -> Option<String> {
        let (folders, files) = self.left_out_counts();
        crate::search::left_out_line(folders, files, "not listed")
    }
}

/// Whether a folder of this name is listed by nothing, whatever is asked for: version control data
/// and IDECraft's own data (the local history's copies of the files).
pub(crate) fn never_listed(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| is_fixed_name(n, VERSION_CONTROL) || is_fixed_name(n, &[".idecraft"]))
}

/// The rule for one project folder.
#[derive(Debug, Clone)]
pub struct OwnSources {
    /// Canonical project folder.
    root: PathBuf,
    /// The project folder as it was given, when that is another spelling.
    given: PathBuf,
    /// Folders the project model excludes, relative to the project folder.
    excluded: Vec<PathBuf>,
    /// Ignore rules from outside the project folder, weakest first: the user's global file,
    /// `.git/info/exclude`, the `.gitignore` files of the folders above.
    above: Vec<Gitignore>,
    /// Files Git tracks, relative to the project folder (`SPEC-0148` v4 behaviour 7): an ignore
    /// rule does not make them foreign. Empty without a repository or when its index cannot be
    /// read.
    tracked: HashSet<PathBuf>,
    /// The folders above tracked files, relative to the project folder.
    tracked_folders: HashSet<PathBuf>,
    /// Whether the file system of the project folder ignores upper and lower case (`SPEC-0148` v5
    /// behaviour 8); asked once. Then the ignore rules do too, as Git's `core.ignorecase`, and
    /// `excluded`, `tracked` and `tracked_folders` hold folded names ([`fold_name`]).
    fold: bool,
}

/// The rules of the ignore file `file`, whose patterns are relative to `base`; with `fold` they
/// match whatever the case.
fn rules(base: &Path, file: &Path, fold: bool) -> Option<Gitignore> {
    if !file.is_file() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(base);
    let _ = builder.case_insensitive(fold);
    // A line that cannot be parsed is skipped; the others still count.
    let _ = builder.add(file);
    builder.build().ok().filter(|g| !g.is_empty())
}

/// The folder that holds a repository's shared data (`info/exclude`), from its `.git` entry: the
/// folder itself, or for a linked work tree or a submodule the folder its `.git` file points to.
fn git_common_dir(dot_git: &Path) -> Option<PathBuf> {
    if dot_git.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let text = std::fs::read_to_string(dot_git).ok()?;
    let dir = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    let dir = dot_git.parent()?.join(dir);
    match std::fs::read_to_string(dir.join("commondir")) {
        Ok(common) => Some(dir.join(common.trim())),
        Err(_) => Some(dir),
    }
}

/// The folder of a repository's own data (`index`), from its `.git` entry: the folder itself, or
/// for a linked work tree or a submodule the folder its `.git` file points to.
fn git_dir(dot_git: &Path) -> Option<PathBuf> {
    if dot_git.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let text = std::fs::read_to_string(dot_git).ok()?;
    let dir = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    Some(dot_git.parent()?.join(dir))
}

/// The files Git tracks in the work tree at `top`, below `root`, relative to `root`: read from the
/// index file, without a `git` process. `None` when the index cannot be read (another hash than
/// SHA-1, a damaged file); then nothing counts as tracked.
fn tracked_files(top: &Path, root: &Path) -> Option<HashSet<PathBuf>> {
    let index = git_dir(&top.join(".git"))?.join("index");
    let prefix = root.strip_prefix(top).ok()?.to_path_buf();
    let options = gix_index::decode::Options {
        // A damaged or hostile index must not take the memory.
        alloc_limit_bytes: Some(1 << 30),
        ..Default::default()
    };
    let read = std::panic::catch_unwind(|| {
        gix_index::File::at(&index, gix_index::hash::Kind::Sha1, true, options)
    });
    let file = read.ok()?.ok()?;
    let mut out = HashSet::new();
    for entry in file.entries() {
        // Only files and links; not a submodule, not a folder of a sparse index, not an entry a
        // split index removed.
        if entry.flags.contains(gix_index::entry::Flags::REMOVE)
            || entry.mode.is_sparse()
            || entry.mode.is_submodule()
        {
            continue;
        }
        let Some(path) = path_of(entry.path(&file)) else {
            continue;
        };
        if let Ok(below) = path.strip_prefix(&prefix) {
            out.insert(below.to_path_buf());
        }
    }
    Some(out)
}

/// A path of the index (`/`-separated bytes) as a path of this system.
fn path_of(bytes: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(std::str::from_utf8(bytes).ok()?);
    path.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(path)
}

/// `path` with every name folded ([`fold_name`]).
fn folded(path: &Path) -> PathBuf {
    PathBuf::from(fold_name(&path.to_string_lossy()))
}

fn ignored(stack: &[Gitignore], path: &Path, is_dir: bool) -> bool {
    for rules in stack.iter().rev() {
        match rules.matched(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

impl OwnSources {
    /// The rule for the project at `root`: reads the project model's excluded folders and the
    /// ignore rules that apply from outside the folder.
    pub fn new(root: &Path) -> Self {
        let given = root.to_path_buf();
        let root = root.canonicalize().unwrap_or_else(|_| given.clone());
        let fold = ignores_case(&root);
        let key = |path: PathBuf| if fold { folded(&path) } else { path };
        let mut excluded = Vec::new();
        if let Some(model) = crate::read_idea_project(&root) {
            let folders = model
                .modules
                .iter()
                .flat_map(|m| &m.content_roots)
                .flat_map(|c| &c.excluded)
                .chain(model.output.iter());
            for folder in folders {
                let folder: PathBuf = Path::new(folder)
                    .components()
                    .filter(|c| !matches!(c, Component::CurDir))
                    .collect();
                // Only folders below the project; the project folder itself is never excluded.
                if !folder.as_os_str().is_empty()
                    && folder
                        .components()
                        .all(|c| matches!(c, Component::Normal(_)))
                {
                    excluded.push(key(folder));
                }
            }
        }
        let mut above = Vec::new();
        let mut tracked = HashSet::new();
        let mut global = GitignoreBuilder::new("");
        let _ = global.case_insensitive(fold);
        let (global, _) = global.build_global();
        if !global.is_empty() {
            above.push(global);
        }
        if let Some(top) = root.ancestors().find(|a| a.join(".git").exists()) {
            if let Some(common) = git_common_dir(&top.join(".git")) {
                above.extend(rules(top, &common.join("info/exclude"), fold));
            }
            tracked = tracked_files(top, &root)
                .unwrap_or_default()
                .into_iter()
                .map(key)
                .collect();
            let mut folders: Vec<&Path> = root
                .ancestors()
                .skip(1)
                .take_while(|a| a.starts_with(top))
                .collect();
            folders.reverse();
            for folder in folders {
                above.extend(rules(folder, &folder.join(".gitignore"), fold));
            }
        }
        let tracked_folders = tracked
            .iter()
            .flat_map(|f| f.ancestors().skip(1))
            .filter(|f| !f.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .collect();
        Self {
            root,
            given,
            excluded,
            above,
            tracked,
            tracked_folders,
            fold,
        }
    }

    /// Whether the file system of the project folder ignores upper and lower case.
    pub fn ignores_case(&self) -> bool {
        self.fold
    }

    /// `path` as the sets of this rule hold it: folded where the file system ignores case.
    fn key<'a>(&self, path: &'a Path) -> std::borrow::Cow<'a, Path> {
        if self.fold {
            folded(path).into()
        } else {
            path.into()
        }
    }

    /// The ignore rules of `file` for this project's file system.
    fn rules(&self, base: &Path, file: &Path) -> Option<Gitignore> {
        rules(base, file, self.fold)
    }

    /// `relative` with the names its folders and file have on disk, as far as they exist without a
    /// symbolic link on the way; the rest as given (`SPEC-0148` v5 behaviour 8). `Node_Modules/x.js`
    /// on a disk that ignores case, a short name of Windows and another Unicode form become the
    /// one name the rules then look at.
    fn on_disk(&self, relative: PathBuf) -> PathBuf {
        let mut at = self.root.clone();
        let mut existing = 0;
        for part in relative.components() {
            at.push(part);
            if std::fs::symlink_metadata(&at).is_ok_and(|m| !m.file_type().is_symlink()) {
                existing += 1;
            } else {
                at.pop();
                break;
            }
        }
        if existing == 0 {
            return relative;
        }
        let Ok(real) = at.canonicalize() else {
            return relative;
        };
        let Ok(real) = real.strip_prefix(&self.root) else {
            return relative;
        };
        if real.components().count() != existing {
            return relative;
        }
        let mut out = real.to_path_buf();
        out.extend(relative.components().skip(existing));
        out
    }

    /// Whether Git tracks the file at `relative`, or a file below the folder at `relative`.
    fn is_tracked(&self, relative: &Path, is_dir: bool) -> bool {
        let relative = self.key(relative);
        if is_dir {
            self.tracked_folders.contains(relative.as_ref())
        } else {
            self.tracked.contains(relative.as_ref())
        }
    }

    /// The canonical project folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Why the folder at `relative` (absolute: `folder`) is not the project's own by its name, its
    /// content or the project model; ignore rules are not looked at.
    fn folder_class(&self, relative: &Path, folder: &Path) -> Option<NotOwn> {
        let name = relative.file_name()?.to_str()?;
        if is_fixed_name(name, VERSION_CONTROL) {
            return Some(NotOwn::VersionControl);
        }
        if is_fixed_name(name, IDE_DATA) {
            return Some(NotOwn::IdeData);
        }
        if is_fixed_name(name, ENVIRONMENTS)
            || folder.join("pyvenv.cfg").is_file()
            || folder.join("conda-meta").is_dir()
        {
            return Some(NotOwn::VirtualEnvironment);
        }
        if is_fixed_name(name, DEPENDENCIES) {
            return Some(NotOwn::Dependencies);
        }
        if is_fixed_name(name, &["vendor"])
            && (VENDOR_MARKERS.iter().any(|m| folder.join(m).is_file())
                || folder
                    .parent()
                    .is_some_and(|p| VENDOR_MANIFESTS.iter().any(|m| p.join(m).is_file())))
        {
            return Some(NotOwn::Dependencies);
        }
        // The list of Find in Files stays part of the rule whatever is added to it.
        if is_fixed_name(name, OUTPUT) || is_fixed_name(name, EXCLUDED_DIRS) {
            return Some(NotOwn::BuildOutput);
        }
        if is_fixed_name(name, OUTPUT_OUTSIDE_SRC)
            && !relative
                .parent()
                .is_some_and(|above| above.components().any(|c| self.is_src(c.as_os_str())))
        {
            return Some(NotOwn::BuildOutput);
        }
        let keyed = self.key(relative);
        if self.excluded.iter().any(|e| keyed.as_ref() == e) {
            return Some(NotOwn::ExcludedByProject);
        }
        None
    }

    /// Whether a folder of this name is a `src` folder, below which `build`, `out` and `dist` are
    /// packages: by its exact name, and in any case only where the file system ignores case (an
    /// exception from the rule is not widened where `Src` and `src` are two folders).
    fn is_src(&self, name: &std::ffi::OsStr) -> bool {
        name == "src" || (self.fold && name.to_str().is_some_and(|n| fold_name(n) == "src"))
    }

    /// `path` relative to the project folder; `None` when it does not lie below it.
    fn relative(&self, path: &Path) -> Option<PathBuf> {
        let relative = if path.is_absolute() {
            match path
                .strip_prefix(&self.root)
                .or_else(|_| path.strip_prefix(&self.given))
            {
                Ok(r) => r.to_path_buf(),
                // Another spelling of the same place (`/tmp` and `/private/tmp`).
                Err(_) => path
                    .canonicalize()
                    .ok()?
                    .strip_prefix(&self.root)
                    .ok()?
                    .to_path_buf(),
            }
        } else {
            path.to_path_buf()
        };
        (!relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))))
        .then_some(relative)
    }

    /// Why `path` (absolute, or relative to the project folder) is not one of the project's own
    /// sources; `None` when it is.
    pub fn not_own(&self, path: &Path) -> Option<NotOwn> {
        let Some(relative) = self.relative(path) else {
            return Some(NotOwn::OutsideProject);
        };
        // Judged by the names on disk, not by the spelling asked for (v5 behaviour 8).
        let relative = self.on_disk(relative);
        let mut stack = self.above.clone();
        stack.extend(self.rules(&self.root, &self.root.join(".gitignore")));
        let parts: Vec<_> = relative.components().collect();
        let mut at = self.root.clone();
        let mut below = PathBuf::new();
        // Inside a folder an ignore rule matches, entered only for the tracked files in it.
        let mut in_ignored = false;
        for (i, part) in parts.iter().enumerate() {
            at.push(part);
            below.push(part);
            let meta = std::fs::symlink_metadata(&at).ok();
            if meta.as_ref().is_some_and(|m| m.file_type().is_symlink()) {
                return Some(NotOwn::SymbolicLink);
            }
            let is_dir = i + 1 < parts.len() || meta.is_some_and(|m| m.is_dir());
            if is_dir && let Some(why) = self.folder_class(&below, &at) {
                return Some(why);
            }
            let tracked = self.is_tracked(&below, is_dir);
            if !tracked && (in_ignored || ignored(&stack, &at, is_dir)) {
                return Some(NotOwn::Ignored);
            }
            if tracked && ignored(&stack, &at, is_dir) {
                in_ignored = true;
            }
            if is_dir {
                stack.extend(self.rules(&at, &at.join(".gitignore")));
            }
        }
        None
    }

    /// The project's own files, relative to the project folder and sorted. Folders that are not
    /// the project's own are not entered, symbolic links are not followed, a folder that cannot
    /// be read is skipped.
    pub fn files(&self) -> Vec<PathBuf> {
        self.listing(None).files
    }

    /// The project's own files below `below` (a folder or a file relative to the project folder;
    /// the whole project when `None`), and what was left out there (`SPEC-0029` v2 behaviours 7
    /// and 10). `below` is expected to be the project's own itself ([`OwnSources::not_own`]); when
    /// it is not, nothing is listed.
    pub fn listing(&self, below: Option<&Path>) -> Listing {
        let mut out = Listing::default();
        let mut stack = self.above.clone();
        self.walk(
            &self.root,
            Path::new(""),
            below,
            &mut stack,
            false,
            &mut out,
        );
        out.files.sort();
        out.left_out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    fn walk(
        &self,
        dir: &Path,
        relative: &Path,
        only: Option<&Path>,
        stack: &mut Vec<Gitignore>,
        in_ignored: bool,
        out: &mut Listing,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let depth = stack.len();
        stack.extend(self.rules(dir, &dir.join(".gitignore")));
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let below = relative.join(entry.file_name());
            // With a place asked for: what lies in it, and the folders on the way to it.
            let inside = only.is_none_or(|o| below.starts_with(o));
            if !inside && !only.is_some_and(|o| o.starts_with(&below)) {
                continue;
            }
            let why = if file_type.is_dir() {
                let tracked = self.is_tracked(&below, true);
                match self.folder_class(&below, &path) {
                    Some(why) => why,
                    // A folder an ignore rule matches is entered for the tracked files in it.
                    None if tracked && (in_ignored || ignored(stack, &path, true)) => {
                        self.walk(&path, &below, only, stack, true, out);
                        continue;
                    }
                    None if in_ignored || ignored(stack, &path, true) => NotOwn::Ignored,
                    None => {
                        self.walk(&path, &below, only, stack, false, out);
                        continue;
                    }
                }
            } else if file_type.is_file() && inside {
                if !self.is_tracked(&below, false) && (in_ignored || ignored(stack, &path, false)) {
                    NotOwn::Ignored
                } else {
                    out.files.push(below);
                    continue;
                }
            } else {
                continue;
            };
            if inside && !never_listed(&entry.file_name()) {
                out.left_out.push(LeftOut {
                    path: below,
                    folder: file_type.is_dir(),
                    why,
                });
            }
        }
        stack.truncate(depth);
    }

    /// The project's own files and the files that are left out of them only because an ignore
    /// rule of Git matches (generated sources of the project), relative to the project folder and
    /// sorted: what the reference index reads (`SPEC-0316` v2 behaviour 7). Virtual environments,
    /// installed dependencies, build output, version control and IDE data and folders the project
    /// model excludes are not entered, also inside an ignored folder; symbolic links are not
    /// followed.
    pub fn sources_and_ignored(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut pending = vec![PathBuf::new()];
        while let Some(relative) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(self.root.join(&relative)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                let below = relative.join(entry.file_name());
                if file_type.is_dir() {
                    if self.folder_class(&below, &entry.path()).is_none() {
                        pending.push(below);
                    }
                } else if file_type.is_file() {
                    out.push(below);
                }
            }
        }
        out.sort();
        out
    }

    /// `path` (absolute, or relative to the project folder) relative to the project folder and with
    /// the names it has on disk, when it may be read at all: `Err` for a path that does not lie below the project folder and for
    /// one with a symbolic link anywhere on the way, also inside a folder that is not the
    /// project's own ([`OwnSources::not_own`] stops at the first such folder and does not look
    /// further).
    pub fn readable(&self, path: &Path) -> Result<PathBuf, NotOwn> {
        let relative = self.relative(path).ok_or(NotOwn::OutsideProject)?;
        let mut at = self.root.clone();
        for part in relative.components() {
            at.push(part);
            if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(NotOwn::SymbolicLink);
            }
        }
        // The names on disk: what a listing below this place compares with (v5 behaviour 8).
        Ok(self.on_disk(relative))
    }

    /// The files a project-wide action works on (`SPEC-0029` v2 behaviours 7 and 8, `SPEC-0098` v8
    /// behaviour 16): the project's own files below `below` (a folder or a file, absolute or
    /// relative to the project folder; the whole project when `None`) with what was left out, or
    /// every file there when `everything` asks for it or when `below` is itself not the project's
    /// own — a place named on purpose wins. `Err` for a place outside the project folder or
    /// behind a symbolic link: nothing there is read.
    pub fn chosen(&self, below: Option<&Path>, everything: bool) -> Result<Listing, NotOwn> {
        let below = match below {
            None => None,
            Some(path) => {
                let plain: PathBuf = path
                    .components()
                    .filter(|c| !matches!(c, Component::CurDir))
                    .collect();
                // `.` and the project folder itself are the whole project.
                let whole = plain.as_os_str().is_empty()
                    || plain == self.root
                    || plain == self.given
                    || (plain.is_absolute()
                        && plain.canonicalize().is_ok_and(|real| real == self.root));
                if whole {
                    None
                } else {
                    Some(self.readable(&plain)?)
                }
            }
        };
        let below = below.as_deref();
        let all = |below| Listing {
            files: self.everything(below),
            left_out: Vec::new(),
        };
        match below.and_then(|place| self.not_own(place)) {
            Some(why @ (NotOwn::OutsideProject | NotOwn::SymbolicLink)) => Err(why),
            Some(_) => Ok(all(below)),
            None if everything => Ok(all(below)),
            None => Ok(self.listing(below)),
        }
    }

    /// Every file below `below` (a folder or a file relative to the project folder; the whole
    /// project when `None`), the project's own or not, sorted: what is searched on purpose
    /// (`SPEC-0029` v2 behaviour 8). Only version control data and IDECraft's own data below the
    /// starting place are not entered; symbolic links are not followed. The caller decides whether
    /// `below` may be read at all ([`OwnSources::not_own`]: not outside the project, not behind a
    /// symbolic link).
    pub fn everything(&self, below: Option<&Path>) -> Vec<PathBuf> {
        let start = below.unwrap_or(Path::new(""));
        let mut out = Vec::new();
        match std::fs::symlink_metadata(self.root.join(start)) {
            Ok(meta) if meta.is_file() => out.push(start.to_path_buf()),
            Ok(meta) if meta.is_dir() => {
                let mut pending = vec![start.to_path_buf()];
                while let Some(relative) = pending.pop() {
                    let Ok(entries) = std::fs::read_dir(self.root.join(&relative)) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        let Ok(file_type) = entry.file_type() else {
                            continue;
                        };
                        let below = relative.join(entry.file_name());
                        if file_type.is_dir() {
                            if !never_listed(&entry.file_name()) {
                                pending.push(below);
                            }
                        } else if file_type.is_file() {
                            out.push(below);
                        }
                    }
                }
            }
            _ => {}
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("idecraft-own-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn write(&self, rel: &str, content: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // SPEC-0029 v2 behaviour 8, SPEC-0098 v8 behaviour 16: a named place wins, the rest by the rule.
    #[test]
    fn a_chosen_place_wins_and_links_are_not_read() {
        let dir = TempDir::new("chosen");
        std::fs::create_dir_all(dir.0.join(".git")).unwrap();
        dir.write(".gitignore", "scratch.py\n");
        dir.write("app.py", "");
        dir.write("scratch.py", "");
        dir.write(".venv/lib/x.py", "");
        dir.write("node_modules/y.js", "");
        let own = OwnSources::new(&dir.0);
        let list = |below: Option<&str>, everything: bool| {
            own.chosen(below.map(Path::new), everything).map(|l| {
                let files: Vec<String> = l
                    .files
                    .iter()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .collect();
                (files, l.left_out.len())
            })
        };
        let own_only = (vec![".gitignore".to_owned(), "app.py".to_owned()], 3);
        assert_eq!(list(None, false), Ok(own_only.clone()));
        assert_eq!(list(Some("."), false), Ok(own_only.clone()));
        assert_eq!(
            own.chosen(Some(&dir.0), false).map(|l| l.files.len()),
            Ok(2),
            "the project folder itself is the whole project"
        );
        assert_eq!(list(None, true).map(|(f, n)| (f.len(), n)), Ok((5, 0)));
        assert_eq!(
            list(Some(".venv"), false),
            Ok((vec![".venv/lib/x.py".to_owned()], 0))
        );
        assert_eq!(
            list(Some("scratch.py"), false),
            Ok((vec!["scratch.py".to_owned()], 0))
        );
        assert_eq!(
            own.chosen(Some(&dir.0.join("node_modules/y.js")), false)
                .map(|l| l.files),
            Ok(vec![PathBuf::from("node_modules/y.js")]),
            "an absolute path below the project"
        );
        assert_eq!(list(Some("../x"), false), Err(NotOwn::OutsideProject));
        assert_eq!(
            own.chosen(Some(Path::new("/etc")), false).map(|l| l.files),
            Err(NotOwn::OutsideProject)
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.0.join(".venv"), dir.0.join("link")).unwrap();
            assert_eq!(
                list(Some("link/lib/x.py"), false),
                Err(NotOwn::SymbolicLink)
            );
            assert_eq!(list(Some("link"), true), Err(NotOwn::SymbolicLink));
            // Also a link inside a folder that is not the project's own, which is named.
            let outside = TempDir::new("chosen-outside");
            outside.write("secret.py", "");
            std::os::unix::fs::symlink(&outside.0, dir.0.join("node_modules/out")).unwrap();
            assert_eq!(
                list(Some("node_modules/out"), false),
                Err(NotOwn::SymbolicLink)
            );
            assert_eq!(
                list(Some("node_modules/out/secret.py"), true),
                Err(NotOwn::SymbolicLink)
            );
            assert_eq!(
                list(Some("node_modules"), false),
                Ok((vec!["node_modules/y.js".to_owned()], 0)),
                "the link below a named folder is not followed"
            );
        }
    }

    fn names(own: &OwnSources) -> Vec<String> {
        own.files()
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    // SPEC-0148/AK-7 (v3, OBS-0034 L14).
    #[test]
    fn environments_dependencies_and_ignored_files_are_not_own() {
        let dir = TempDir::new("mixed");
        dir.write(".git/HEAD", "ref: refs/heads/main\n");
        dir.write(".git/info/exclude", "local-notes.txt\n");
        dir.write(".gitignore", "*.log\n/generated/\n!keep.log\n");
        dir.write("shop/prices.py", "");
        dir.write("shop/.gitignore", "secret.py\n");
        dir.write("shop/secret.py", "");
        dir.write("keep.log", "");
        dir.write("run.log", "");
        dir.write("local-notes.txt", "");
        dir.write("generated/api.py", "");
        dir.write(".venv/lib/python3.12/site-packages/x.py", "");
        dir.write("tools/env312/pyvenv.cfg", "home = /usr/bin\n");
        dir.write("tools/env312/lib/y.py", "");
        dir.write("lib/python3.12/site-packages/z.py", "");
        dir.write("node_modules/y.js", "");
        dir.write("target/debug/out.rs", "");
        dir.write("build/gen.py", "");
        dir.write("src/build/own.py", "");
        dir.write("go.mod", "module shop\n");
        dir.write("vendor/dep/dep.go", "");
        dir.write("assets/vendor/own.js", "");
        let own = OwnSources::new(&dir.0);
        assert_eq!(
            names(&own),
            [
                ".gitignore",
                "assets/vendor/own.js",
                "go.mod",
                "keep.log",
                "shop/.gitignore",
                "shop/prices.py",
                "src/build/own.py",
            ]
        );
        let why = |p: &str| own.not_own(Path::new(p));
        assert_eq!(why("shop/prices.py"), None);
        assert_eq!(why("keep.log"), None);
        assert_eq!(why("src/build/own.py"), None);
        assert_eq!(
            why(".venv/lib/python3.12/site-packages/x.py"),
            Some(NotOwn::VirtualEnvironment)
        );
        assert_eq!(
            why("tools/env312/lib/y.py"),
            Some(NotOwn::VirtualEnvironment)
        );
        assert_eq!(
            why("lib/python3.12/site-packages/z.py"),
            Some(NotOwn::VirtualEnvironment)
        );
        assert_eq!(why("node_modules/y.js"), Some(NotOwn::Dependencies));
        assert_eq!(why("vendor/dep/dep.go"), Some(NotOwn::Dependencies));
        assert_eq!(why("target/debug/out.rs"), Some(NotOwn::BuildOutput));
        assert_eq!(why("build/gen.py"), Some(NotOwn::BuildOutput));
        assert_eq!(why(".git/HEAD"), Some(NotOwn::VersionControl));
        assert_eq!(why("run.log"), Some(NotOwn::Ignored));
        assert_eq!(why("generated/api.py"), Some(NotOwn::Ignored));
        assert_eq!(why("shop/secret.py"), Some(NotOwn::Ignored));
        assert_eq!(why("local-notes.txt"), Some(NotOwn::Ignored));
        // Absolute paths, and paths that leave the project.
        assert_eq!(own.not_own(&dir.0.join("shop/prices.py")), None);
        assert_eq!(
            own.not_own(&dir.0.join("node_modules/y.js")),
            Some(NotOwn::Dependencies)
        );
        assert_eq!(why("../elsewhere.py"), Some(NotOwn::OutsideProject));
        assert_eq!(
            own.not_own(&std::env::temp_dir().join("idecraft-own-elsewhere.py")),
            Some(NotOwn::OutsideProject)
        );
        assert_eq!(
            NotOwn::VirtualEnvironment.reason(),
            "in a virtual environment"
        );
    }

    // SPEC-0029/AK-6 (v2): what a listing left out, a listing of one place, and everything.
    #[test]
    fn listing_names_what_was_left_out() {
        let dir = TempDir::new("listing");
        dir.write(".git/HEAD", "ref: refs/heads/main\n");
        dir.write(".idecraft/history/1", "");
        dir.write(".gitignore", "*.log\n/src/generated/\n");
        dir.write("src/main.py", "");
        dir.write("src/run.log", "");
        dir.write("src/generated/api.py", "");
        dir.write("src/pkg/util.py", "");
        dir.write("docs/a.md", "");
        dir.write(".venv/lib/python3.12/site-packages/x.py", "");
        dir.write(".venv/lib/python3.12/site-packages/.git/HEAD", "");
        dir.write("node_modules/y.js", "");
        let own = OwnSources::new(&dir.0);
        let slashes = |p: &Path| p.to_string_lossy().replace('\\', "/");
        let left = |l: &Listing| {
            l.left_out
                .iter()
                .map(|o| (slashes(&o.path), o.folder, o.why))
                .collect::<Vec<_>>()
        };
        let all = own.listing(None);
        assert_eq!(all.files, own.files());
        assert_eq!(
            left(&all),
            [
                (".venv".to_owned(), true, NotOwn::VirtualEnvironment),
                ("node_modules".to_owned(), true, NotOwn::Dependencies),
                ("src/generated".to_owned(), true, NotOwn::Ignored),
                ("src/run.log".to_owned(), false, NotOwn::Ignored),
            ],
            "version control and IDECraft's data are not named"
        );
        // One place: the rules of the folders above still count, what lies elsewhere is not named.
        let src = own.listing(Some(Path::new("src")));
        assert_eq!(
            src.files,
            [
                PathBuf::from("src/main.py"),
                PathBuf::from("src/pkg/util.py")
            ]
        );
        assert_eq!(left(&src).len(), 2);
        let pkg = own.listing(Some(Path::new("src/pkg")));
        assert_eq!(pkg.files, [PathBuf::from("src/pkg/util.py")]);
        assert!(pkg.left_out.is_empty());
        let file = own.listing(Some(Path::new("src/main.py")));
        assert_eq!(file.files, [PathBuf::from("src/main.py")]);
        assert!(own.listing(Some(Path::new(".venv/lib"))).files.is_empty());
        assert!(own.listing(Some(Path::new("missing"))).files.is_empty());
        // Everything: all but version control and IDECraft's data.
        let names = |v: Vec<PathBuf>| v.iter().map(|p| slashes(p)).collect::<Vec<_>>();
        assert_eq!(
            names(own.everything(None)),
            [
                ".gitignore",
                ".venv/lib/python3.12/site-packages/x.py",
                "docs/a.md",
                "node_modules/y.js",
                "src/generated/api.py",
                "src/main.py",
                "src/pkg/util.py",
                "src/run.log",
            ]
        );
        assert_eq!(
            names(own.everything(Some(Path::new(".venv/lib")))),
            [".venv/lib/python3.12/site-packages/x.py"]
        );
        assert_eq!(
            names(own.everything(Some(Path::new("src/run.log")))),
            ["src/run.log"]
        );
        assert!(own.everything(Some(Path::new("missing"))).is_empty());
    }

    // SPEC-0148/AK-7 (v3): the project model's excluded folders and the rules of the folders
    // above a project that is part of a larger repository.
    #[test]
    fn the_project_model_and_rules_from_above_count() {
        let dir = TempDir::new("model");
        dir.write(".git/HEAD", "ref: refs/heads/main\n");
        dir.write(".gitignore", "*.tmp\n");
        dir.write("app/.idea/modules.xml", "<project version=\"4\"><component name=\"ProjectModuleManager\"><modules><module fileurl=\"file://$PROJECT_DIR$/app.iml\" filepath=\"$PROJECT_DIR$/app.iml\" /></modules></component></project>");
        dir.write("app/app.iml", "<module type=\"JAVA_MODULE\" version=\"4\"><component name=\"NewModuleRootManager\"><content url=\"file://$MODULE_DIR$\"><sourceFolder url=\"file://$MODULE_DIR$/src\" isTestSource=\"false\" /><excludeFolder url=\"file://$MODULE_DIR$/gen\" /></content></component></module>");
        dir.write("app/src/Main.java", "");
        dir.write("app/gen/Api.java", "");
        dir.write("app/notes.tmp", "");
        let own = OwnSources::new(&dir.0.join("app"));
        assert_eq!(names(&own), ["app.iml", "src/Main.java"]);
        assert_eq!(
            own.not_own(Path::new("gen/Api.java")),
            Some(NotOwn::ExcludedByProject)
        );
        assert_eq!(own.not_own(Path::new("notes.tmp")), Some(NotOwn::Ignored));
        assert_eq!(
            own.not_own(Path::new(".idea/modules.xml")),
            Some(NotOwn::IdeData)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_not_followed() {
        let dir = TempDir::new("links");
        let other = TempDir::new("links-target");
        other.write("lib.py", "");
        dir.write("main.py", "");
        std::os::unix::fs::symlink(&other.0, dir.0.join("linked")).unwrap();
        std::os::unix::fs::symlink(other.0.join("lib.py"), dir.0.join("lib.py")).unwrap();
        let own = OwnSources::new(&dir.0);
        assert_eq!(names(&own), ["main.py"]);
        assert_eq!(
            own.not_own(Path::new("linked/lib.py")),
            Some(NotOwn::SymbolicLink)
        );
        assert_eq!(own.not_own(Path::new("lib.py")), Some(NotOwn::SymbolicLink));
    }

    /// Runs `git` with fixed arguments in `dir`, without the user's or the system's settings.
    fn git(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    // SPEC-0148/AK-9 (v4): a file Git tracks is the project's own although an ignore rule matches
    // it (`git add -f`, `.git/info/exclude`, inside an ignored folder); untracked neighbours stay
    // ignored, and a tracked file of an environment stays in the environment.
    #[test]
    fn files_git_tracks_are_own_although_ignored() {
        let dir = TempDir::new("tracked");
        let d = &dir.0;
        if !git(d, &["init", "-q"]) {
            eprintln!("skipped: no git");
            return;
        }
        dir.write(".gitignore", "*.log\nlogs/\n");
        dir.write("main.py", "x = 1\n");
        dir.write("keep.log", "kept\n");
        dir.write("other.log", "not kept\n");
        dir.write("logs/keep.txt", "kept\n");
        dir.write("logs/other.txt", "not kept\n");
        dir.write("logs/deep/more.txt", "not kept\n");
        dir.write("secret.txt", "kept\n");
        dir.write(".git/info/exclude", "secret.txt\n");
        dir.write(".venv/lib/mod.py", "y = 2\n");
        assert!(git(
            d,
            &[
                "add",
                "-f",
                ".gitignore",
                "main.py",
                "keep.log",
                "logs/keep.txt",
                "secret.txt",
                ".venv/lib/mod.py",
            ]
        ));
        let own = OwnSources::new(d);
        assert_eq!(
            names(&own),
            [
                ".gitignore",
                "keep.log",
                "logs/keep.txt",
                "main.py",
                "secret.txt"
            ]
        );
        assert_eq!(own.not_own(Path::new("keep.log")), None);
        assert_eq!(own.not_own(Path::new("logs/keep.txt")), None);
        assert_eq!(own.not_own(&d.join("secret.txt")), None);
        assert_eq!(own.not_own(Path::new("logs")), None);
        assert_eq!(own.not_own(Path::new("other.log")), Some(NotOwn::Ignored));
        assert_eq!(
            own.not_own(Path::new("logs/other.txt")),
            Some(NotOwn::Ignored)
        );
        assert_eq!(
            own.not_own(Path::new("logs/deep/more.txt")),
            Some(NotOwn::Ignored)
        );
        assert_eq!(
            own.not_own(Path::new(".venv/lib/mod.py")),
            Some(NotOwn::VirtualEnvironment)
        );
        let left: Vec<(String, NotOwn)> = own
            .listing(None)
            .left_out
            .iter()
            .map(|l| (l.path.to_string_lossy().replace('\\', "/"), l.why))
            .collect();
        assert_eq!(
            left,
            [
                (".venv".to_owned(), NotOwn::VirtualEnvironment),
                ("logs/deep".to_owned(), NotOwn::Ignored),
                ("logs/other.txt".to_owned(), NotOwn::Ignored),
                ("other.log".to_owned(), NotOwn::Ignored),
            ]
        );
        // Below a project folder inside the work tree, paths are relative to that folder.
        dir.write("sub/.gitignore", "*.tmp\n");
        dir.write("sub/a.tmp", "kept\n");
        dir.write("sub/b.tmp", "not kept\n");
        assert!(git(d, &["add", "-f", "sub/a.tmp"]));
        assert_eq!(
            names(&OwnSources::new(&d.join("sub"))),
            [".gitignore", "a.tmp"]
        );
        // An index that cannot be read: the ignore rules alone, as before.
        std::fs::write(d.join(".git/index"), b"DIRC broken").unwrap();
        let before = [".gitignore", "main.py", "sub/.gitignore"];
        assert_eq!(names(&OwnSources::new(d)), before);
        std::fs::write(d.join(".git/index"), b"").unwrap();
        assert_eq!(names(&OwnSources::new(d)), before);
    }

    // SPEC-0316/AK-6 (v2): the reference index reads the own sources and merely ignored files.
    #[test]
    fn sources_and_ignored_files_without_environments_and_output() {
        let dir = TempDir::new("with-ignored");
        dir.write(".git/HEAD", "ref: refs/heads/main\n");
        dir.write(".gitignore", "generated/\n*.gen.java\n");
        dir.write("src/A.java", "class A {}\n");
        dir.write("src/B.gen.java", "class B {}\n");
        dir.write("generated/C.java", "class C {}\n");
        dir.write("generated/node_modules/d/D.java", "class D {}\n");
        dir.write(".venv/lib/python3.12/site-packages/x.java", "class X {}\n");
        dir.write("node_modules/y/Y.java", "class Y {}\n");
        dir.write("build/Z.java", "class Z {}\n");
        dir.write(".idea/workspace.xml", "<A/>\n");
        let own = OwnSources::new(&dir.0);
        assert_eq!(names(&own), [".gitignore", "src/A.java"]);
        let read: Vec<String> = own
            .sources_and_ignored()
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            read,
            [
                ".gitignore",
                "generated/C.java",
                "src/A.java",
                "src/B.gen.java"
            ]
        );
    }

    // SPEC-0148/AK-10 (v5): the comparison of names itself, on every platform.
    #[test]
    fn fixed_names_count_in_any_case() {
        assert_eq!(fold_name("Node_Modules"), "node_modules");
        assert_eq!(fold_name(".GIT"), ".git");
        // Letters that have no upper or lower case of their own still meet their letter.
        assert_eq!(fold_name(".\u{131}dea"), ".idea");
        assert_eq!(fold_name("\u{17f}ite-packages"), "site-packages");
        for (name, list) in [
            ("Node_Modules", DEPENDENCIES),
            ("PODS", DEPENDENCIES),
            (".GIT", VERSION_CONTROL),
            ("cvs", VERSION_CONTROL),
            (".Idea", IDE_DATA),
            (".IDECRAFT", IDE_DATA),
            (".VENV", ENVIRONMENTS),
            ("Site-Packages", ENVIRONMENTS),
            ("TARGET", OUTPUT),
            ("__PyCache__", OUTPUT),
            ("Build", OUTPUT_OUTSIDE_SRC),
        ] {
            assert!(is_fixed_name(name, list), "{name}");
        }
        for name in ["node_module", "git", ".gitignore", "targets", "venv2"] {
            for list in [DEPENDENCIES, VERSION_CONTROL, ENVIRONMENTS, OUTPUT] {
                assert!(!is_fixed_name(name, list), "{name}");
            }
        }
        assert!(never_listed(std::ffi::OsStr::new(".Git")));
        assert!(never_listed(std::ffi::OsStr::new(".IDECraft")));
        assert!(!never_listed(std::ffi::OsStr::new(".idea")));
        // By the path alone, nothing on disk: the reason does not depend on the file system.
        let dir = TempDir::new("names");
        let own = OwnSources::new(&dir.0);
        for (path, why) in [
            ("Node_Modules/x.js", NotOwn::Dependencies),
            (".VENV/lib/x.py", NotOwn::VirtualEnvironment),
            (".GIT/config", NotOwn::VersionControl),
            (".Idea/workspace.xml", NotOwn::IdeData),
            ("app/Target/debug/x", NotOwn::BuildOutput),
            ("BUILD/x.js", NotOwn::BuildOutput),
        ] {
            assert_eq!(own.not_own(Path::new(path)), Some(why), "{path}");
        }
        assert_eq!(own.not_own(Path::new("src/build/x.js")), None);
        // A folder that is on disk under another case is not entered by a listing either.
        dir.write("Bower_Components/z.js", "");
        dir.write(".Hg/store", "");
        dir.write("main.js", "");
        assert_eq!(names(&own), ["main.js"]);
        assert_eq!(own.sources_and_ignored(), [PathBuf::from("main.js")]);
        assert_eq!(
            own.everything(None),
            [
                PathBuf::from("Bower_Components/z.js"),
                PathBuf::from("main.js")
            ]
        );
    }

    // SPEC-0148/AK-11 (v5): where the disk ignores case, another spelling reaches the same file
    // and gets the same answer. Runs where the temporary folder ignores case (macOS, Windows).
    #[test]
    fn another_spelling_is_judged_by_the_name_on_disk() {
        let dir = TempDir::new("case");
        dir.write(".git/config", "");
        dir.write(".gitignore", "Logs/\n*.LOCAL\n");
        dir.write("node_modules/x.js", "");
        dir.write(".venv/lib/x.py", "");
        dir.write("logs/app.txt", "");
        dir.write("settings.local", "");
        dir.write("src/Main.py", "");
        dir.write("lib/build/x.py", "");
        let own = OwnSources::new(&dir.0);
        if !own.ignores_case() {
            eprintln!(
                "skipped: the file system of {} does not ignore case",
                dir.0.display()
            );
            assert!(!dir.0.join("NODE_MODULES").exists());
            return;
        }
        for (path, why) in [
            ("Node_Modules/x.js", NotOwn::Dependencies),
            ("NODE_MODULES/X.JS", NotOwn::Dependencies),
            (".VENV/lib/x.py", NotOwn::VirtualEnvironment),
            (".GIT/config", NotOwn::VersionControl),
            // The ignore rules follow the file system, as Git's `core.ignorecase`.
            ("logs/app.txt", NotOwn::Ignored),
            ("LOGS/APP.TXT", NotOwn::Ignored),
            ("settings.local", NotOwn::Ignored),
            ("LIB/BUILD/x.py", NotOwn::BuildOutput),
        ] {
            assert_eq!(own.not_own(Path::new(path)), Some(why), "{path}");
            assert_eq!(own.not_own(&dir.0.join(path)), Some(why), "absolute {path}");
        }
        // An own file under another spelling is own, and named as it is on disk.
        assert_eq!(own.not_own(Path::new("SRC/main.PY")), None);
        assert_eq!(
            own.readable(Path::new("SRC/main.PY")),
            Ok(PathBuf::from("src/Main.py"))
        );
        assert_eq!(
            own.chosen(Some(Path::new("SRC")), false).map(|l| l.files),
            Ok(vec![PathBuf::from("src/Main.py")])
        );
        assert_eq!(names(&own), [".gitignore", "src/Main.py"]);
    }

    // SPEC-0276/AK-9 (v3): the folder a suggested command starts in.
    #[test]
    fn a_folder_for_a_command_is_inside_and_behind_no_link() {
        let dir = TempDir::new("folder");
        let other = TempDir::new("folder-target");
        dir.write("tool/run.sh", "");
        dir.write("file.txt", "");
        assert_eq!(folder_below(&dir.0, Path::new(".")), Ok(dir.0.clone()));
        assert_eq!(
            folder_below(&dir.0, Path::new("./tool")),
            Ok(dir.0.join("tool"))
        );
        for (bad, why) in [
            ("../x", FolderProblem::Form),
            ("/tmp", FolderProblem::Form),
            ("tool/../..", FolderProblem::Form),
            ("missing", FolderProblem::Missing),
            ("file.txt", FolderProblem::NotAFolder),
        ] {
            assert_eq!(folder_below(&dir.0, Path::new(bad)), Err(why), "{bad}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&other.0, dir.0.join("out")).unwrap();
            std::os::unix::fs::symlink(dir.0.join("tool"), dir.0.join("inside")).unwrap();
            std::fs::create_dir_all(other.0.join("deep")).unwrap();
            for link in ["out", "out/deep", "inside"] {
                assert_eq!(
                    folder_below(&dir.0, Path::new(link)),
                    Err(FolderProblem::SymbolicLink),
                    "{link}"
                );
            }
        }
    }

    // SPEC-0156/AK-12 (v4): the handle a file is read from.
    #[test]
    fn a_file_is_opened_without_following_a_link() {
        use std::io::Read;
        let dir = TempDir::new("open");
        dir.write("a.txt", "own");
        dir.write("b.txt", "other");
        let checked = std::fs::symlink_metadata(dir.0.join("a.txt")).unwrap();
        let (mut file, meta) = open_regular(&dir.0.join("a.txt")).unwrap();
        assert!(same_file(&checked, &meta));
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "own");
        let (_, other) = open_regular(&dir.0.join("b.txt")).unwrap();
        #[cfg(unix)]
        assert!(!same_file(&checked, &other));
        let _ = other;
        assert!(open_regular(&dir.0).is_err(), "a folder");
        assert!(open_regular(&dir.0.join("missing")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.0.join("b.txt"), dir.0.join("link.txt")).unwrap();
            assert!(open_regular(&dir.0.join("link.txt")).is_err(), "a link");
        }
    }
}
