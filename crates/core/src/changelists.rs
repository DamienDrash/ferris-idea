//! The changelists of a project (`SPEC-0335`): named lists the changed files are sorted into, one
//! of them active and receiving new changes, kept for one user in the workspace's IDECraft
//! settings (`.idecraft/changelists.toml`). A changelist is a thing of the IDE; nothing of it is
//! written into the repository.
//!
//! Names and paths in the state file are data: a path is only ever compared with the paths the
//! repository's status reports, and what a hand-edited file holds beyond the rules is left out.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Where the changelists of a workspace are kept, relative to its root.
pub const CHANGELISTS_FILE: &str = ".idecraft/changelists.toml";

/// The name of the list every project starts with, and of the list that replaces the last one.
pub const DEFAULT_NAME: &str = "Changes";

/// The name a new list is offered (behaviour 8).
pub const NEW_NAME: &str = "New changelist";

/// The longest name, in characters.
pub const MAX_NAME_CHARS: usize = 100;

/// The most lists a project has.
pub const MAX_LISTS: usize = 100;

/// The longest path that is kept, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// The largest file that is read, in bytes.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// The format this build writes and reads.
pub const FORMAT_VERSION: u32 = 1;

/// Why a name is refused (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but white space.
    Blank,
    /// More than [`MAX_NAME_CHARS`] characters.
    TooLong,
    /// A control character.
    Control,
    /// Another list has the name.
    Taken(String),
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NameError::Blank => f.write_str("A changelist needs a name"),
            NameError::TooLong => write!(
                f,
                "A changelist's name has at most {MAX_NAME_CHARS} characters"
            ),
            NameError::Control => f.write_str("A changelist's name cannot hold control characters"),
            NameError::Taken(name) => write!(f, "A changelist named '{name}' already exists"),
        }
    }
}

/// Why a change of the lists is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListError {
    /// The name breaks a rule.
    Name(NameError),
    /// No list has this name.
    Unknown(String),
    /// The project has [`MAX_LISTS`] lists already.
    TooMany,
    /// Deleting the active list needs the list that becomes active.
    NeedsNewActive,
    /// The list that should become active is the one being deleted.
    SameList,
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListError::Name(e) => e.fmt(f),
            ListError::Unknown(name) => write!(f, "There is no changelist named '{name}'"),
            ListError::TooMany => write!(f, "A project has at most {MAX_LISTS} changelists"),
            ListError::NeedsNewActive => f.write_str(
                "Deleting the active changelist needs the changelist that becomes active",
            ),
            ListError::SameList => {
                f.write_str("The changelist that becomes active cannot be the deleted one")
            }
        }
    }
}

impl From<NameError> for ListError {
    fn from(e: NameError) -> Self {
        ListError::Name(e)
    }
}

/// The name as it is kept: without the white space around it, checked for length and control
/// characters (behaviour 5). Whether it is taken depends on the lists.
pub fn valid_name(name: &str) -> Result<String, NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NameError::Blank);
    }
    if name.chars().any(char::is_control) {
        return Err(NameError::Control);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(NameError::TooLong);
    }
    Ok(name.to_owned())
}

/// Whether a path of the state file is kept: relative, `/`-separated, without `.` or `..` parts,
/// backslashes or control characters, not longer than [`MAX_PATH_BYTES`] (behaviour 20).
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Why a state file was not used. The text never quotes the file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    /// Not a regular file (a link or a folder), or it cannot be read.
    NotAFile,
    /// Larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// Not the expected TOML.
    Broken,
    /// Written in another format version.
    Version(u32),
}

impl Unreadable {
    /// Whether the file must stay as it is: a file of a newer version and something that is not
    /// a regular file are never written over or through (behaviour 21).
    pub fn keeps_the_file(self) -> bool {
        match self {
            Unreadable::NotAFile => true,
            Unreadable::Version(v) => v > FORMAT_VERSION,
            Unreadable::TooLarge | Unreadable::Broken => false,
        }
    }
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::NotAFile => f.write_str("it is not a regular file or cannot be read"),
            Unreadable::TooLarge => write!(f, "it is larger than {} MiB", MAX_FILE_BYTES >> 20),
            Unreadable::Broken => f.write_str("its content is not a list of changelists"),
            Unreadable::Version(v) => write!(
                f,
                "it has format version {v}, this IDECraft reads version {FORMAT_VERSION}"
            ),
        }
    }
}

/// One list of the file.
#[derive(Serialize, Deserialize)]
struct StoredList {
    name: String,
    #[serde(default)]
    files: Vec<String>,
}

/// The file's content.
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<String>,
    #[serde(default)]
    list: Vec<StoredList>,
}

/// Only the version of a file, to see whether it may be replaced.
#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

/// One changelist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changelist {
    /// Its name.
    pub name: String,
    /// Its files: paths relative to the repository root, `/`-separated.
    pub files: BTreeSet<String>,
}

/// What deleting a list did (behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deleted {
    /// The list is gone; its `files` joined the list `to`, which is the active one now.
    Removed {
        /// The list that took the files.
        to: String,
        /// How many files moved.
        files: usize,
    },
    /// It was the only list: the active list [`DEFAULT_NAME`] took its place and its `files`.
    Replaced {
        /// How many files it took.
        files: usize,
    },
    /// It was the only list and is named [`DEFAULT_NAME`]: it stays.
    Kept,
}

/// What a move did (behaviour 12).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moved {
    /// The target was created.
    pub created: bool,
    /// Files that changed their list.
    pub moved: Vec<String>,
    /// Files that were in the target already.
    pub unchanged: Vec<String>,
    /// Paths no list holds: not changed files.
    pub skipped: Vec<String>,
}

/// The lists a move offers and the one it preselects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveTargets {
    /// The names offered, in the order of the lists.
    pub offered: Vec<String>,
    /// The name the chooser starts with.
    pub preselected: String,
}

/// The changelists of one workspace: at least one list, exactly one of them active, in the order
/// they are shown (by name, capitals ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changelists {
    lists: Vec<Changelist>,
    active: String,
}

impl Default for Changelists {
    /// The one active list [`DEFAULT_NAME`] (behaviour 1).
    fn default() -> Self {
        Self {
            lists: vec![Changelist {
                name: DEFAULT_NAME.to_owned(),
                files: BTreeSet::new(),
            }],
            active: DEFAULT_NAME.to_owned(),
        }
    }
}

impl Changelists {
    /// The lists in the order they are shown.
    pub fn lists(&self) -> &[Changelist] {
        &self.lists
    }

    /// The name of the active list.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// The list named `name`.
    pub fn get(&self, name: &str) -> Option<&Changelist> {
        self.lists.iter().find(|l| l.name == name)
    }

    /// The name of the list that holds `path`.
    pub fn list_of(&self, path: &str) -> Option<&str> {
        self.lists
            .iter()
            .find(|l| l.files.contains(path))
            .map(|l| l.name.as_str())
    }

    /// Whether this is what a project starts with apart from the files: only the list
    /// [`DEFAULT_NAME`]. Such lists need no file (behaviour 19).
    pub fn only_the_default_list(&self) -> bool {
        self.lists.len() == 1 && self.lists[0].name == DEFAULT_NAME
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Changelist> {
        self.lists.iter_mut().find(|l| l.name == name)
    }

    fn sort(&mut self) {
        self.lists.sort_by(|a, b| {
            (a.name.to_lowercase(), &a.name).cmp(&(b.name.to_lowercase(), &b.name))
        });
    }

    /// `name` as a new name: trimmed, within the rules, not taken by a list other than `own`.
    pub fn checked_name(&self, name: &str, own: Option<&str>) -> Result<String, NameError> {
        let name = valid_name(name)?;
        if Some(name.as_str()) != own && self.get(&name).is_some() {
            return Err(NameError::Taken(name));
        }
        Ok(name)
    }

    /// The name a new list is offered: [`NEW_NAME`], or with `-2`, `-3` … when taken.
    pub fn free_name(&self) -> String {
        if self.get(NEW_NAME).is_none() {
            return NEW_NAME.to_owned();
        }
        (2..)
            .map(|n| format!("{NEW_NAME}-{n}"))
            .find(|n| self.get(n).is_none())
            .expect("a free number")
    }

    /// Brings the lists in line with the changed files (behaviours 2–4): a renamed file keeps
    /// the list of its old path, a file whose change is gone leaves its list, a changed file no
    /// list holds joins the active list. `renames` are `(old, new)` paths. `true` when a list
    /// changed.
    pub fn sync(&mut self, changed: &[String], renames: &[(String, String)]) -> bool {
        let now: BTreeSet<&str> = changed.iter().map(String::as_str).collect();
        let mut touched = false;
        for (old, new) in renames {
            if !now.contains(new.as_str()) || self.list_of(new).is_some() {
                continue;
            }
            if let Some(list) = self.lists.iter_mut().find(|l| l.files.contains(old)) {
                list.files.remove(old);
                list.files.insert(new.clone());
                touched = true;
            }
        }
        for list in &mut self.lists {
            let before = list.files.len();
            list.files.retain(|f| now.contains(f.as_str()));
            touched |= list.files.len() != before;
        }
        let new: Vec<String> = changed
            .iter()
            .filter(|p| self.list_of(p).is_none())
            .cloned()
            .collect();
        if !new.is_empty() {
            let active = self.active.clone();
            let list = self.get_mut(&active).expect("the active list exists");
            list.files.extend(new);
            touched = true;
        }
        touched
    }

    /// Creates an empty list and returns its name as kept; with `set_active` it becomes the
    /// active list (behaviour 8).
    pub fn create(&mut self, name: &str, set_active: bool) -> Result<String, ListError> {
        let name = self.checked_name(name, None)?;
        if self.lists.len() >= MAX_LISTS {
            return Err(ListError::TooMany);
        }
        self.lists.push(Changelist {
            name: name.clone(),
            files: BTreeSet::new(),
        });
        self.sort();
        if set_active {
            self.active = name.clone();
        }
        Ok(name)
    }

    /// Renames the list `name`; its files stay. Returns the new name as kept, `None` when it is
    /// the name the list has (behaviour 9).
    pub fn rename(&mut self, name: &str, new_name: &str) -> Result<Option<String>, ListError> {
        if self.get(name).is_none() {
            return Err(ListError::Unknown(name.to_owned()));
        }
        let new_name = self.checked_name(new_name, Some(name))?;
        if new_name == name {
            return Ok(None);
        }
        if self.active == name {
            self.active = new_name.clone();
        }
        self.get_mut(name).expect("checked above").name = new_name.clone();
        self.sort();
        Ok(Some(new_name))
    }

    /// Makes the list `name` the active one; `false` when it is active already (behaviour 10).
    pub fn set_active(&mut self, name: &str) -> Result<bool, ListError> {
        if self.get(name).is_none() {
            return Err(ListError::Unknown(name.to_owned()));
        }
        if self.active == name {
            return Ok(false);
        }
        self.active = name.to_owned();
        Ok(true)
    }

    /// Whether deleting the list `name` has to be told which list becomes active: it is the
    /// active list, and either more than one other list exists or it has files and another list
    /// exists (behaviour 11).
    pub fn delete_asks_for_active(&self, name: &str) -> bool {
        let Some(list) = self.get(name) else {
            return false;
        };
        self.active == name
            && self.lists.len() > 1
            && (self.lists.len() > 2 || !list.files.is_empty())
    }

    /// Deletes the list `name` (behaviour 11). The files of a list that is not active join the
    /// active list. The active list hands its files to `new_active`, which becomes active;
    /// without `new_active` that is the other list when there is exactly one other. The only
    /// list is replaced by the list [`DEFAULT_NAME`], or stays when it has that name.
    pub fn delete(&mut self, name: &str, new_active: Option<&str>) -> Result<Deleted, ListError> {
        let Some(index) = self.lists.iter().position(|l| l.name == name) else {
            return Err(ListError::Unknown(name.to_owned()));
        };
        if self.lists.len() == 1 {
            if name == DEFAULT_NAME {
                return Ok(Deleted::Kept);
            }
            let files = self.lists[0].files.len();
            self.lists[0].name = DEFAULT_NAME.to_owned();
            self.active = DEFAULT_NAME.to_owned();
            return Ok(Deleted::Replaced { files });
        }
        let to = if self.active != name {
            self.active.clone()
        } else {
            match new_active {
                Some(n) if n == name => return Err(ListError::SameList),
                Some(n) if self.get(n).is_none() => {
                    return Err(ListError::Unknown(n.to_owned()));
                }
                Some(n) => n.to_owned(),
                None if self.lists.len() == 2 => self.lists[1 - index].name.clone(),
                None => return Err(ListError::NeedsNewActive),
            }
        };
        let gone = self.lists.remove(index);
        let files = gone.files.len();
        self.get_mut(&to)
            .expect("the target exists")
            .files
            .extend(gone.files);
        self.active = to.clone();
        Ok(Deleted::Removed { to, files })
    }

    /// The lists a move of `paths` offers and the one it starts with (behaviour 12): all lists
    /// but the one the files are in when they come from one list, the active list when that
    /// leaves none; preselected is the active list if offered, else the first empty one, else
    /// the first.
    pub fn move_targets(&self, paths: &[String]) -> MoveTargets {
        let from: BTreeSet<&str> = paths.iter().filter_map(|p| self.list_of(p)).collect();
        let mut offered: Vec<&Changelist> = if from.len() == 1 {
            self.lists
                .iter()
                .filter(|l| !from.contains(l.name.as_str()))
                .collect()
        } else {
            self.lists.iter().collect()
        };
        if offered.is_empty() {
            offered = self.get(&self.active).into_iter().collect();
        }
        let others: Vec<&Changelist> = offered
            .iter()
            .copied()
            .filter(|l| !from.contains(l.name.as_str()))
            .collect();
        let pool = if others.is_empty() { &offered } else { &others };
        let preselected = pool
            .iter()
            .find(|l| l.name == self.active)
            .or_else(|| pool.iter().find(|l| l.files.is_empty()))
            .or_else(|| pool.first())
            .map(|l| l.name.clone())
            .unwrap_or_else(|| self.active.clone());
        MoveTargets {
            offered: offered.into_iter().map(|l| l.name.clone()).collect(),
            preselected,
        }
    }

    /// Moves `paths` into the list `to` (behaviour 12). With `create` a list of that name is
    /// made when none exists. Paths no list holds are not changed files and are skipped.
    pub fn move_files(
        &mut self,
        paths: &[String],
        to: &str,
        create: bool,
    ) -> Result<Moved, ListError> {
        let mut out = Moved::default();
        let to = to.trim();
        let target = if self.get(to).is_some() {
            to.to_owned()
        } else if create {
            out.created = true;
            self.create(to, false)?
        } else {
            return Err(ListError::Unknown(to.to_owned()));
        };
        for path in paths {
            match self.list_of(path).map(str::to_owned) {
                None => out.skipped.push(path.clone()),
                Some(from) if from == target => out.unchanged.push(path.clone()),
                Some(from) => {
                    self.get_mut(&from)
                        .expect("it holds the path")
                        .files
                        .remove(path);
                    self.get_mut(&target)
                        .expect("the target exists")
                        .files
                        .insert(path.clone());
                    out.moved.push(path.clone());
                }
            }
        }
        Ok(out)
    }

    /// Reads the lists of the workspace at `root`. A missing file is the one list
    /// [`DEFAULT_NAME`]; so is a file that cannot be used, with the reason (behaviour 21).
    pub fn load(root: &Path) -> (Self, Option<Unreadable>) {
        let file = root.join(CHANGELISTS_FILE);
        // A link is not followed: the file is IDECraft's own or it is not used.
        let meta = match std::fs::symlink_metadata(&file) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(_) => return (Self::default(), Some(Unreadable::NotAFile)),
        };
        if !meta.is_file() {
            return (Self::default(), Some(Unreadable::NotAFile));
        }
        if meta.len() > MAX_FILE_BYTES {
            return (Self::default(), Some(Unreadable::TooLarge));
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            return (Self::default(), Some(Unreadable::NotAFile));
        };
        match Self::parse(&text) {
            Ok(lists) => (lists, None),
            Err(why) => (Self::default(), Some(why)),
        }
    }

    /// The lists in `text`, the content of a state file. What breaks the rules is left out
    /// (behaviour 20).
    pub fn parse(text: &str) -> Result<Self, Unreadable> {
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(Unreadable::TooLarge);
        }
        let stored: Stored = match toml::from_str(text) {
            Ok(s) => s,
            Err(_) => {
                // A file of another version may have another shape: say the version then.
                return Err(match toml::from_str::<VersionOnly>(text) {
                    Ok(v) if v.version != FORMAT_VERSION => Unreadable::Version(v.version),
                    _ => Unreadable::Broken,
                });
            }
        };
        if stored.version != FORMAT_VERSION {
            return Err(Unreadable::Version(stored.version));
        }
        let mut out = Self {
            lists: Vec::new(),
            active: String::new(),
        };
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for list in stored.list {
            if out.lists.len() == MAX_LISTS {
                break;
            }
            // The name as written: one that needs trimming is not a name IDECraft wrote.
            let Ok(name) = valid_name(&list.name) else {
                continue;
            };
            if name != list.name || out.get(&name).is_some() {
                continue;
            }
            let files = list
                .files
                .into_iter()
                .filter(|f| valid_path(f) && seen.insert(f.clone()))
                .collect();
            out.lists.push(Changelist { name, files });
        }
        if out.lists.is_empty() {
            return Ok(Self::default());
        }
        out.active = stored
            .active
            .filter(|a| out.get(a).is_some())
            .unwrap_or_else(|| out.lists[0].name.clone());
        out.sort();
        Ok(out)
    }

    /// The content of the state file for these lists.
    pub fn to_toml(&self) -> String {
        let stored = Stored {
            version: FORMAT_VERSION,
            active: Some(self.active.clone()),
            list: self
                .lists
                .iter()
                .map(|l| StoredList {
                    name: l.name.clone(),
                    files: l.files.iter().cloned().collect(),
                })
                .collect(),
        };
        let mut text = String::from("# Changelists of this workspace (IDECraft)\n");
        text.push_str(&toml::to_string(&stored).expect("strings and lists of strings"));
        text
    }

    /// Writes the lists to the workspace at `root`, replacing the file as a whole. A link or a
    /// folder under the file's name is not written through, and a file of a newer version is not
    /// written over (behaviour 21).
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let file = root.join(CHANGELISTS_FILE);
        if let Ok(meta) = std::fs::symlink_metadata(&file) {
            if !meta.is_file() {
                return Err(std::io::Error::other(format!(
                    "{CHANGELISTS_FILE} is not a regular file"
                )));
            }
            let newer = meta.len() <= MAX_FILE_BYTES
                && std::fs::read_to_string(&file)
                    .ok()
                    .and_then(|t| toml::from_str::<VersionOnly>(&t).ok())
                    .is_some_and(|v| v.version > FORMAT_VERSION);
            if newer {
                return Err(std::io::Error::other(format!(
                    "{CHANGELISTS_FILE} was written by a newer IDECraft"
                )));
            }
        }
        crate::write_atomic(&file, self.to_toml())
    }

    /// Writes the lists when there is something to keep — more than the one list
    /// [`DEFAULT_NAME`] — or when the file exists already (behaviour 19). `Ok(false)` when
    /// nothing was written.
    pub fn save_if_kept(&self, root: &Path) -> std::io::Result<bool> {
        if !self.keeps_a_file(root) {
            return Ok(false);
        }
        self.save(root).map(|()| true)
    }

    /// Whether these lists are kept in a file at `root`: there is more than the one list
    /// [`DEFAULT_NAME`], or the file exists already.
    pub fn keeps_a_file(&self, root: &Path) -> bool {
        !self.only_the_default_list()
            || std::fs::symlink_metadata(root.join(CHANGELISTS_FILE)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-changelists-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    fn files(lists: &Changelists, name: &str) -> Vec<String> {
        lists.get(name).unwrap().files.iter().cloned().collect()
    }

    fn names(lists: &Changelists) -> Vec<&str> {
        lists.lists().iter().map(|l| l.name.as_str()).collect()
    }

    /// SPEC-0335 AK-1 (behaviour 1).
    #[test]
    fn a_project_starts_with_the_active_list_changes() {
        let lists = Changelists::default();
        assert_eq!(names(&lists), ["Changes"]);
        assert_eq!(lists.active(), "Changes");
        assert!(lists.only_the_default_list());
        assert_eq!(lists.list_of("a.rs"), None);
    }

    /// SPEC-0335 AK-1 (behaviour 2): a change seen for the first time joins the active list and
    /// stays there when another list becomes active.
    #[test]
    fn new_changes_join_the_active_list_and_stay() {
        let mut lists = Changelists::default();
        assert!(lists.sync(&paths(&["a.rs", "b.rs"]), &[]));
        assert_eq!(files(&lists, "Changes"), ["a.rs", "b.rs"]);
        // Nothing new: nothing changes.
        assert!(!lists.sync(&paths(&["a.rs", "b.rs"]), &[]));
        lists.create("Cart", true).unwrap();
        assert_eq!(lists.active(), "Cart");
        assert!(lists.sync(&paths(&["a.rs", "b.rs", "c.rs"]), &[]));
        assert_eq!(files(&lists, "Changes"), ["a.rs", "b.rs"]);
        assert_eq!(files(&lists, "Cart"), ["c.rs"]);
        // Every file is in exactly one list.
        for p in ["a.rs", "b.rs", "c.rs"] {
            let holders = lists.lists().iter().filter(|l| l.files.contains(p)).count();
            assert_eq!(holders, 1, "{p}");
        }
        assert_eq!(lists.list_of("c.rs"), Some("Cart"));
    }

    /// SPEC-0335 AK-1 (behaviour 3): a committed or reverted file leaves; the list stays.
    #[test]
    fn a_file_whose_change_is_gone_leaves_and_the_list_stays() {
        let mut lists = Changelists::default();
        lists.create("Cart", true).unwrap();
        lists.sync(&paths(&["a.rs", "b.rs"]), &[]);
        assert!(lists.sync(&paths(&["b.rs"]), &[]));
        assert_eq!(files(&lists, "Cart"), ["b.rs"]);
        assert!(lists.sync(&[], &[]));
        assert_eq!(names(&lists), ["Cart", "Changes"]);
        assert!(files(&lists, "Cart").is_empty());
        assert_eq!(lists.active(), "Cart");
    }

    /// SPEC-0335 AK-1 (behaviour 4): a renamed file keeps the list of its old path, also when
    /// that list is not the active one.
    #[test]
    fn a_renamed_file_keeps_its_list() {
        let mut lists = Changelists::default();
        lists.sync(&paths(&["old.rs", "x.rs"]), &[]);
        lists.create("Later", true).unwrap();
        let renames = [("old.rs".to_owned(), "new.rs".to_owned())];
        assert!(lists.sync(&paths(&["new.rs", "x.rs"]), &renames));
        assert_eq!(files(&lists, "Changes"), ["new.rs", "x.rs"]);
        assert!(files(&lists, "Later").is_empty());
        // Without the rename the new path is a new change: it joins the active list.
        let mut plain = Changelists::default();
        plain.sync(&paths(&["old.rs"]), &[]);
        plain.create("Later", true).unwrap();
        plain.sync(&paths(&["new.rs"]), &[]);
        assert_eq!(files(&plain, "Later"), ["new.rs"]);
        // A rename whose new path is not a change moves nothing.
        let mut gone = Changelists::default();
        gone.sync(&paths(&["old.rs"]), &[]);
        gone.sync(&[], &renames);
        assert!(files(&gone, "Changes").is_empty());
    }

    /// SPEC-0335 AK-2 (behaviours 8–10).
    #[test]
    fn create_rename_and_set_active() {
        let mut lists = Changelists::default();
        lists.sync(&paths(&["a.rs"]), &[]);
        assert_eq!(lists.create("  Cart ", false).unwrap(), "Cart");
        assert_eq!(lists.active(), "Changes");
        assert!(files(&lists, "Cart").is_empty());
        assert_eq!(lists.create("api", true).unwrap(), "api");
        // By name, capitals ignored.
        assert_eq!(names(&lists), ["api", "Cart", "Changes"]);
        assert_eq!(lists.active(), "api");

        // Rename keeps the files and the active list follows its name.
        assert_eq!(
            lists.rename("Changes", "Zebra").unwrap(),
            Some("Zebra".to_owned())
        );
        assert_eq!(files(&lists, "Zebra"), ["a.rs"]);
        assert_eq!(
            lists.rename("api", " API ").unwrap(),
            Some("API".to_owned())
        );
        assert_eq!(lists.active(), "API");
        assert_eq!(names(&lists), ["API", "Cart", "Zebra"]);
        // The name it has: nothing changes.
        assert_eq!(lists.rename("Cart", " Cart ").unwrap(), None);
        assert_eq!(
            lists.rename("Cart", "Zebra"),
            Err(ListError::Name(NameError::Taken("Zebra".into())))
        );
        assert_eq!(
            lists.rename("Nope", "X"),
            Err(ListError::Unknown("Nope".into()))
        );

        assert_eq!(lists.set_active("Cart"), Ok(true));
        assert_eq!(lists.set_active("Cart"), Ok(false));
        assert_eq!(
            lists.set_active("Nope"),
            Err(ListError::Unknown("Nope".into()))
        );
        assert_eq!(lists.active(), "Cart");
    }

    /// SPEC-0335 AK-2 (behaviour 11).
    #[test]
    fn delete_in_its_four_cases() {
        // Not active, empty: gone.
        let mut lists = Changelists::default();
        lists.create("Cart", false).unwrap();
        assert!(!lists.delete_asks_for_active("Cart"));
        assert_eq!(
            lists.delete("Cart", None),
            Ok(Deleted::Removed {
                to: "Changes".into(),
                files: 0
            })
        );
        assert_eq!(names(&lists), ["Changes"]);

        // Not active, with files: they join the active list.
        let mut lists = Changelists::default();
        lists.create("Cart", true).unwrap();
        lists.sync(&paths(&["a.rs", "b.rs"]), &[]);
        lists.set_active("Changes").unwrap();
        assert_eq!(
            lists.delete("Cart", None),
            Ok(Deleted::Removed {
                to: "Changes".into(),
                files: 2
            })
        );
        assert_eq!(files(&lists, "Changes"), ["a.rs", "b.rs"]);

        // Active with exactly one other list and no files: the other becomes active unasked.
        let mut lists = Changelists::default();
        lists.create("Cart", true).unwrap();
        assert!(!lists.delete_asks_for_active("Cart"));
        assert_eq!(
            lists.delete("Cart", None),
            Ok(Deleted::Removed {
                to: "Changes".into(),
                files: 0
            })
        );
        assert_eq!(lists.active(), "Changes");

        // Active with files, or with several others: the new active list is asked for.
        let mut lists = Changelists::default();
        lists.create("Cart", true).unwrap();
        lists.sync(&paths(&["a.rs"]), &[]);
        assert!(lists.delete_asks_for_active("Cart"));
        lists.create("Api", false).unwrap();
        assert_eq!(lists.delete("Cart", None), Err(ListError::NeedsNewActive));
        assert_eq!(lists.delete("Cart", Some("Cart")), Err(ListError::SameList));
        assert_eq!(
            lists.delete("Cart", Some("Nope")),
            Err(ListError::Unknown("Nope".into()))
        );
        assert_eq!(
            lists.delete("Cart", Some("Api")),
            Ok(Deleted::Removed {
                to: "Api".into(),
                files: 1
            })
        );
        assert_eq!(lists.active(), "Api");
        assert_eq!(files(&lists, "Api"), ["a.rs"]);
        assert_eq!(names(&lists), ["Api", "Changes"]);

        // The only list: one of another name is replaced by Changes, Changes itself stays.
        let mut lists = Changelists::default();
        lists.rename("Changes", "Cart").unwrap();
        lists.sync(&paths(&["a.rs"]), &[]);
        assert!(!lists.delete_asks_for_active("Cart"));
        assert_eq!(
            lists.delete("Cart", None),
            Ok(Deleted::Replaced { files: 1 })
        );
        assert_eq!(names(&lists), ["Changes"]);
        assert_eq!(lists.active(), "Changes");
        assert_eq!(files(&lists, "Changes"), ["a.rs"]);
        assert_eq!(lists.delete("Changes", None), Ok(Deleted::Kept));
        assert_eq!(files(&lists, "Changes"), ["a.rs"]);
        assert_eq!(
            lists.delete("Nope", None),
            Err(ListError::Unknown("Nope".into()))
        );
    }

    /// SPEC-0335 AK-2 (behaviour 12).
    #[test]
    fn move_files_with_and_without_a_new_list() {
        let mut lists = Changelists::default();
        lists.sync(&paths(&["a.rs", "b.rs", "c.rs"]), &[]);
        lists.create("Cart", false).unwrap();
        let moved = lists
            .move_files(&paths(&["a.rs", "nope.rs"]), "Cart", false)
            .unwrap();
        assert_eq!(moved.moved, ["a.rs"]);
        assert_eq!(moved.skipped, ["nope.rs"]);
        assert!(!moved.created);
        assert_eq!(files(&lists, "Cart"), ["a.rs"]);
        assert_eq!(files(&lists, "Changes"), ["b.rs", "c.rs"]);
        // Already there.
        let again = lists.move_files(&paths(&["a.rs"]), "Cart", false).unwrap();
        assert_eq!(again.unchanged, ["a.rs"]);
        assert!(again.moved.is_empty());
        // A list that does not exist: only with `create`, and the name is checked.
        assert_eq!(
            lists.move_files(&paths(&["b.rs"]), "Api", false),
            Err(ListError::Unknown("Api".into()))
        );
        assert_eq!(
            lists.move_files(&paths(&["b.rs"]), "  ", true),
            Err(ListError::Name(NameError::Blank))
        );
        let made = lists.move_files(&paths(&["b.rs"]), " Api ", true).unwrap();
        assert!(made.created);
        assert_eq!(files(&lists, "Api"), ["b.rs"]);
        // The active list did not change, and a moved file stays through the next look.
        assert_eq!(lists.active(), "Changes");
        assert!(!lists.sync(&paths(&["a.rs", "b.rs", "c.rs"]), &[]));
        assert_eq!(lists.list_of("a.rs"), Some("Cart"));
    }

    /// SPEC-0335 AK-2 (behaviour 12): what a move offers and where it starts.
    #[test]
    fn move_targets_and_the_preselected_list() {
        let mut lists = Changelists::default();
        lists.sync(&paths(&["a.rs"]), &[]);
        // No other list: the active one is offered.
        let t = lists.move_targets(&paths(&["a.rs"]));
        assert_eq!(t.offered, ["Changes"]);
        assert_eq!(t.preselected, "Changes");
        lists.create("Zoo", false).unwrap();
        lists.create("Api", true).unwrap();
        lists.sync(&paths(&["a.rs", "b.rs"]), &[]);
        // From Changes: the other two, the active one first choice.
        let t = lists.move_targets(&paths(&["a.rs"]));
        assert_eq!(t.offered, ["Api", "Zoo"]);
        assert_eq!(t.preselected, "Api");
        // From the active list: the first empty one.
        lists.create("Bar", false).unwrap();
        lists.move_files(&paths(&["a.rs"]), "Bar", false).unwrap();
        let t = lists.move_targets(&paths(&["b.rs"]));
        assert_eq!(t.offered, ["Bar", "Changes", "Zoo"]);
        assert_eq!(t.preselected, "Changes");
        // From two lists: all are offered, a list none of the files is in is preselected.
        let t = lists.move_targets(&paths(&["a.rs", "b.rs"]));
        assert_eq!(t.offered, ["Api", "Bar", "Changes", "Zoo"]);
        assert_eq!(t.preselected, "Changes");
    }

    /// SPEC-0335 AK-3 (behaviour 5).
    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(valid_name("  Cart\t"), Ok("Cart".to_owned()));
        for blank in ["", " ", "\n\t "] {
            assert_eq!(valid_name(blank), Err(NameError::Blank), "{blank:?}");
        }
        assert_eq!(valid_name("a\u{7}b"), Err(NameError::Control));
        assert_eq!(valid_name("two\nlines"), Err(NameError::Control));
        let longest = "ä".repeat(MAX_NAME_CHARS);
        assert_eq!(valid_name(&longest), Ok(longest.clone()));
        assert_eq!(valid_name(&format!("{longest}x")), Err(NameError::TooLong));

        let mut lists = Changelists::default();
        lists.create("Cart", false).unwrap();
        assert_eq!(
            lists.create(" Cart ", false),
            Err(ListError::Name(NameError::Taken("Cart".into())))
        );
        // Capitals make another name.
        assert_eq!(lists.create("cart", false).unwrap(), "cart");
        assert_eq!(names(&lists), ["Cart", "cart", "Changes"]);
        assert_eq!(
            lists.create("", false),
            Err(ListError::Name(NameError::Blank))
        );
        assert_eq!(lists.checked_name("Cart", Some("Cart")), Ok("Cart".into()));
        // The texts name the rule, and a taken name is the only one that is quoted.
        assert!(NameError::TooLong.to_string().contains("100"));
        assert!(
            NameError::Taken("Cart".into())
                .to_string()
                .contains("'Cart'")
        );
    }

    /// SPEC-0335 AK-3 (behaviour 6).
    #[test]
    fn at_most_a_hundred_lists() {
        let mut lists = Changelists::default();
        for i in 1..MAX_LISTS {
            lists.create(&format!("list {i}"), false).unwrap();
        }
        assert_eq!(lists.lists().len(), MAX_LISTS);
        assert_eq!(lists.create("one more", false), Err(ListError::TooMany));
        assert_eq!(
            lists.move_files(&[], "one more", true),
            Err(ListError::TooMany)
        );
        lists.delete("list 1", None).unwrap();
        assert!(lists.create("one more", false).is_ok());
    }

    /// SPEC-0335 AK-3 (behaviour 8).
    #[test]
    fn the_offered_name_is_free() {
        let mut lists = Changelists::default();
        assert_eq!(lists.free_name(), "New changelist");
        lists.create("New changelist", false).unwrap();
        assert_eq!(lists.free_name(), "New changelist-2");
        lists.create("New changelist-2", false).unwrap();
        assert_eq!(lists.free_name(), "New changelist-3");
    }

    /// SPEC-0335 AK-4 (behaviour 19).
    #[test]
    fn the_file_round_trips() {
        let dir = scratch("round");
        let mut lists = Changelists::default();
        lists.sync(&paths(&["README.md"]), &[]);
        lists.create("Checkout \"v2\" = [x]", true).unwrap();
        lists.sync(
            &paths(&["README.md", "src/cart.rs", "src/ünï cödé.rs"]),
            &[],
        );
        lists.save(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join(CHANGELISTS_FILE)).unwrap();
        assert!(text.contains("version = 1"), "{text}");
        assert!(text.contains("[[list]]"), "{text}");
        let (read, problem) = Changelists::load(&dir);
        assert_eq!(problem, None);
        assert_eq!(read, lists);
        assert_eq!(read.active(), "Checkout \"v2\" = [x]");
        // No temporary file is left behind.
        let left: Vec<_> = std::fs::read_dir(dir.join(".idecraft"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(left, ["changelists.toml"]);
        // The documented example reads as documented.
        let example = "version = 1\nactive = \"Checkout\"\n\n[[list]]\nname = \"Changes\"\n\
                       files = [\"README.md\"]\n\n[[list]]\nname = \"Checkout\"\n\
                       files = [\"src/cart.rs\", \"src/total.rs\"]\n";
        let read = Changelists::parse(example).unwrap();
        assert_eq!(read.active(), "Checkout");
        assert_eq!(files(&read, "Checkout"), ["src/cart.rs", "src/total.rs"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0335 AK-4 (behaviour 20): a hand-edited file is not trusted beyond the rules.
    #[test]
    fn entries_beyond_the_rules_are_left_out() {
        let text = r#"
version = 1
active = "Nope"

[[list]]
name = "Work"
files = ["a.rs", "/etc/passwd", "../up.rs", "x/./y.rs", "", "back\\slash.rs", "a.rs", "ok/b.rs"]

[[list]]
name = " padded "
files = ["c.rs"]

[[list]]
name = "Work"
files = ["d.rs"]

[[list]]
name = "Again"
files = ["a.rs", "e.rs"]
"#;
        let lists = Changelists::parse(text).unwrap();
        assert_eq!(names(&lists), ["Again", "Work"]);
        assert_eq!(files(&lists, "Work"), ["a.rs", "ok/b.rs"]);
        // A path an earlier list holds stays there.
        assert_eq!(files(&lists, "Again"), ["e.rs"]);
        // An active list that does not exist: the first of the file.
        assert_eq!(lists.active(), "Work");
        // No usable list: the list Changes.
        let none = Changelists::parse("version = 1\n[[list]]\nname = \"\"\n").unwrap();
        assert_eq!(none, Changelists::default());
        assert_eq!(
            Changelists::parse("version = 1\n").unwrap(),
            Changelists::default()
        );
        // More than a hundred lists: the first hundred.
        let mut many = String::from("version = 1\n");
        for i in 0..MAX_LISTS + 5 {
            many.push_str(&format!("[[list]]\nname = \"l{i:03}\"\n"));
        }
        assert_eq!(Changelists::parse(&many).unwrap().lists().len(), MAX_LISTS);
        // A path longer than the limit is left out.
        let long = format!(
            "version = 1\n[[list]]\nname = \"L\"\nfiles = [\"{}\", \"ok.rs\"]\n",
            "p".repeat(MAX_PATH_BYTES + 1)
        );
        assert_eq!(files(&Changelists::parse(&long).unwrap(), "L"), ["ok.rs"]);
    }

    /// SPEC-0335 AK-4 (behaviour 21): the reason never quotes the file.
    #[test]
    fn an_unusable_file_is_the_list_changes_with_the_reason() {
        let dir = scratch("unusable");
        std::fs::create_dir_all(dir.join(".idecraft")).unwrap();
        let file = dir.join(CHANGELISTS_FILE);
        // Missing: no problem.
        assert_eq!(Changelists::load(&dir), (Changelists::default(), None));
        for (content, why) in [
            (
                "version = 1\n[[list]\nname = \"s3cr3t\"\n",
                Unreadable::Broken,
            ),
            ("not toml at all: s3cr3t", Unreadable::Broken),
            ("list = []\n", Unreadable::Broken),
            (
                "version = 2\nlists = { s3cr3t = 1 }\n",
                Unreadable::Version(2),
            ),
            (
                "version = 7\n[[list]]\nname = \"s3cr3t\"\n",
                Unreadable::Version(7),
            ),
            ("version = 0\n", Unreadable::Version(0)),
        ] {
            std::fs::write(&file, content).unwrap();
            let (lists, problem) = Changelists::load(&dir);
            assert_eq!(lists, Changelists::default(), "{content}");
            assert_eq!(problem, Some(why), "{content}");
            assert!(!why.to_string().contains("s3cr3t"));
        }
        // Too large.
        std::fs::write(&file, vec![b'#'; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert_eq!(Changelists::load(&dir).1, Some(Unreadable::TooLarge));
        assert!(!Unreadable::TooLarge.keeps_the_file());
        assert!(!Unreadable::Broken.keeps_the_file());
        assert!(!Unreadable::Version(0).keeps_the_file());
        // A broken file is replaced by the next save.
        std::fs::write(&file, "broken [").unwrap();
        let mut lists = Changelists::default();
        lists.create("Cart", false).unwrap();
        lists.save(&dir).unwrap();
        assert_eq!(Changelists::load(&dir), (lists, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0335 AK-4 (behaviour 21).
    #[test]
    fn a_newer_file_and_a_link_are_not_written() {
        let dir = scratch("kept");
        std::fs::create_dir_all(dir.join(".idecraft")).unwrap();
        let file = dir.join(CHANGELISTS_FILE);
        let mut lists = Changelists::default();
        lists.create("Cart", true).unwrap();

        let newer = "version = 2\nshape = \"of the future\"\n";
        std::fs::write(&file, newer).unwrap();
        assert!(Unreadable::Version(2).keeps_the_file());
        assert!(lists.save(&dir).is_err());
        assert!(lists.save_if_kept(&dir).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
        std::fs::remove_file(&file).unwrap();

        // A folder under the name.
        std::fs::create_dir(&file).unwrap();
        assert_eq!(Changelists::load(&dir).1, Some(Unreadable::NotAFile));
        assert!(Unreadable::NotAFile.keeps_the_file());
        assert!(lists.save(&dir).is_err());
        std::fs::remove_dir(&file).unwrap();

        // A link is neither read nor written through.
        #[cfg(unix)]
        {
            let target = dir.join("elsewhere.toml");
            std::fs::write(&target, "version = 1\n[[list]]\nname = \"Linked\"\n").unwrap();
            std::os::unix::fs::symlink(&target, &file).unwrap();
            let (read, problem) = Changelists::load(&dir);
            assert_eq!(read, Changelists::default());
            assert_eq!(problem, Some(Unreadable::NotAFile));
            assert!(lists.save(&dir).is_err());
            assert!(std::fs::read_to_string(&target).unwrap().contains("Linked"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0335 AK-4 (behaviour 19): no file for a project that never used changelists.
    #[test]
    fn a_project_with_only_changes_gets_no_file() {
        let dir = scratch("none");
        let mut lists = Changelists::default();
        lists.sync(&paths(&["a.rs"]), &[]);
        assert!(!lists.save_if_kept(&dir).unwrap());
        assert!(!dir.join(CHANGELISTS_FILE).exists());
        assert!(!dir.join(".idecraft").exists());
        // With a second list there is something to keep.
        lists.create("Cart", false).unwrap();
        assert!(lists.save_if_kept(&dir).unwrap());
        // Back to the one list: the file that exists is kept current.
        lists.delete("Cart", None).unwrap();
        assert!(lists.only_the_default_list());
        assert!(lists.save_if_kept(&dir).unwrap());
        assert_eq!(Changelists::load(&dir), (lists, None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
