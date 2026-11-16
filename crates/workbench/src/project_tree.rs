//! Project tree model (`SPEC-0059` behaviours 1 and 2), without toolkit; several selected rows
//! (`SPEC-0275` v3 behaviour 24); the top-level nodes "External Libraries" and "Scratches and
//! Consoles" and the view "Project Files" without them (`SPEC-0061` v4 behaviours 11 and 16);
//! packages in source folders compacted to one row (`SPEC-0275` v10 behaviour 30).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use idecraft_core::buildtools::{FolderInfo, FolderRole};

/// Names hidden like in the reference's default ignore list (behaviour 1).
const IGNORED_NAMES: &[&str] = &[
    ".git",
    ".hg",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".svn",
    "CVS",
    "_svn",
    ".DS_Store",
    "__pycache__",
    "vssver.scc",
    "vssver2.scc",
];
const IGNORED_SUFFIXES: &[&str] = &[".pyc", ".pyo", ".rbc", ".yarb", "~"];

/// Most folders "Expand All" reads at once.
pub const EXPAND_ALL_LIMIT: usize = 200;

/// Whether a file or folder name is hidden from the tree.
pub fn is_ignored(name: &str) -> bool {
    IGNORED_NAMES.contains(&name) || IGNORED_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// What a node stands for (`SPEC-0061` v4 behaviour 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A file or folder on disk.
    File,
    /// "External Libraries".
    Libraries,
    /// One library or toolchain below it (no file).
    Library,
    /// "Scratches and Consoles".
    Scratches,
}

impl NodeKind {
    /// Icon of the node when it is not a plain file or folder.
    pub fn icon(self) -> Option<&'static str> {
        match self {
            NodeKind::File => None,
            NodeKind::Libraries | NodeKind::Library => Some("nodes/library"),
            NodeKind::Scratches => Some("fileTypes/scratches"),
        }
    }
}

/// Where the extra top-level nodes look (`SPEC-0061` v4 behaviour 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraRoots {
    /// The user's scratches folder (`scratches` in the IDECraft settings folder).
    pub scratches: Option<PathBuf>,
    /// The project's database consoles (`.idecraft/consoles`).
    pub consoles: PathBuf,
}

impl ExtraRoots {
    /// The folders for the project at `root`.
    pub fn for_project(root: &Path) -> Self {
        Self {
            scratches: idecraft_core::user_config_dir().map(|d| d.join("scratches")),
            consoles: root.join(".idecraft/consoles"),
        }
    }
}

/// Name of the libraries node.
pub const LIBRARIES: &str = "External Libraries";
/// Name of the scratches node.
pub const SCRATCHES: &str = "Scratches and Consoles";

/// The entries of "External Libraries" for the project at `root` (`SPEC-0061` v4 behaviour 16):
/// the resolved libraries of Maven and Gradle modules and the JDK's folder (`SPEC-0048` v3
/// behaviour 11).
pub fn library_entries(root: &Path) -> Vec<String> {
    let files = idecraft_core::Workspace::open(root)
        .ok()
        .and_then(|w| w.files().ok())
        .unwrap_or_default();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    idecraft_core::buildtools::library_entries(
        root,
        &idecraft_core::read_project(root, &files),
        &|feature| {
            idecraft_core::buildtools::jdk_home_for(root, feature)
                .map(|h| crate::chrome::tilde(Path::new(&h), home.as_deref()))
        },
    )
}

/// The roles of the folders the project declares: those of its `.idea` modules (`SPEC-0049` v2
/// behaviour 10) and of its Maven and Gradle modules (`SPEC-0048` v3 behaviour 10), which win.
pub fn folder_roles(root: &Path) -> HashMap<PathBuf, FolderInfo> {
    // The modules of the reference IDE's `.idea` first; a build file wins for its folders
    // (`SPEC-0049` v2 behaviour 10).
    let mut roles: HashMap<PathBuf, FolderInfo> = idecraft_core::read_idea_project(root)
        .map(|p| {
            idecraft_core::idea_folder_roles(root, &p)
                .into_iter()
                .collect()
        })
        .unwrap_or_default();
    roles.extend(build_folder_roles(root));
    roles
}

/// The roles the Maven and Gradle modules give the folders (`SPEC-0048` v3 behaviour 10); empty
/// unless the project folder has a POM or a Gradle build file, so other projects are not read
/// for it.
fn build_folder_roles(root: &Path) -> HashMap<PathBuf, FolderInfo> {
    let jvm = [
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|f| root.join(f).is_file());
    if !jvm {
        return HashMap::new();
    }
    let files = idecraft_core::Workspace::open(root)
        .ok()
        .and_then(|w| w.files().ok())
        .unwrap_or_default();
    idecraft_core::buildtools::folder_roles(root, &idecraft_core::read_project(root, &files))
        .into_iter()
        .collect()
}

/// Listed folders of a remote project: entries (folder?, name) by folder.
pub type Listings = HashMap<PathBuf, Vec<(bool, String)>>;

/// The folders of a remote project (`SPEC-0317` v3 behaviour 7): listed through the session in
/// the background and kept here by the folder's path in the window (below the project's own
/// state folder, which stands for the host folder). A folder not listed yet is asked for and
/// shows its entries when the listing arrives and the tree is read again.
#[derive(Clone, Default)]
pub struct RemoteListing {
    /// Entries (folder?, name) by folder.
    pub dirs: std::sync::Arc<std::sync::Mutex<Listings>>,
    /// Folders asked for and not listed yet.
    pub wanted: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>,
}

impl std::fmt::Debug for RemoteListing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoteListing")
    }
}

impl RemoteListing {
    /// The entries of `dir`, or `None` (and `dir` asked for) when it was not listed yet.
    fn entries(&self, dir: &Path) -> Option<Vec<(bool, String)>> {
        let got = self.dirs.lock().ok().and_then(|d| d.get(dir).cloned());
        if got.is_none()
            && let Ok(mut w) = self.wanted.lock()
            && !w.iter().any(|p| p == dir)
        {
            w.push(dir.to_path_buf());
        }
        got
    }

    /// Stores the listing of `dir`.
    pub fn put(&self, dir: PathBuf, entries: Vec<(bool, String)>) {
        if let Ok(mut d) = self.dirs.lock() {
            d.insert(dir, entries);
        }
    }

    /// Forgets the listing of `dir`, so it is read again.
    pub fn forget(&self, dir: &Path) {
        if let Ok(mut d) = self.dirs.lock() {
            d.remove(dir);
        }
    }

    /// The folders asked for since the last call.
    pub fn take_wanted(&self) -> Vec<PathBuf> {
        self.wanted
            .lock()
            .map(|mut w| std::mem::take(&mut *w))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
struct Node {
    path: PathBuf,
    name: String,
    dir: bool,
    depth: usize,
    parent: Option<usize>,
    /// `None` until the folder was read.
    children: Option<Vec<usize>>,
    kind: NodeKind,
}

/// One visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Node id, stable while the tree lives.
    pub id: usize,
    /// Display name.
    pub name: String,
    /// Nesting level, 0 for the root.
    pub depth: usize,
    /// Folder or file.
    pub dir: bool,
    /// Whether the folder is expanded.
    pub expanded: bool,
    /// What the node stands for.
    pub kind: NodeKind,
    /// The outermost folder of a row that stands for several folders (`SPEC-0275` v10 behaviour
    /// 30); the row's own id otherwise. `id` is the innermost one.
    pub head: usize,
}

/// The tree of one workspace folder.
#[derive(Debug, Clone)]
pub struct ProjectTree {
    nodes: Vec<Node>,
    expanded: HashSet<usize>,
    /// Selected node: the lead of a selection of several rows.
    pub selected: usize,
    /// The nodes of a selection of several rows; it counts only while it holds `selected`, so
    /// selecting one node by setting `selected` ends it.
    marked: Vec<usize>,
    /// Where ⇧-click and ⇧↑/⇧↓ ranges start.
    anchor: usize,
    /// The extra top-level nodes, when the tree has them.
    extras: Option<ExtraRoots>,
    /// The view "Project Files": the project folder only.
    project_files: bool,
    /// Roles of the folders: those of Maven and Gradle modules (`SPEC-0048` v3 behaviour 10) and
    /// the source and test folders recognised from the files (v6 behaviour 14).
    roles: HashMap<PathBuf, FolderInfo>,
    /// The roles the build files declare, as [`folder_roles`] read them.
    declared: HashMap<PathBuf, FolderInfo>,
    /// The source and test folders recognised from the files where no build file declares them
    /// (`SPEC-0048` v6 behaviour 14), as last read; `None` before the first reading. Reading the
    /// tree again keeps them ([`Self::reloaded`]).
    derived: Option<BTreeMap<PathBuf, FolderInfo>>,
    /// Counts how often the tree was read again from disk ([`Self::reloaded`]).
    generation: u64,
    /// The folders of a remote project, instead of this machine's disk (`SPEC-0317` v3).
    remote: Option<RemoteListing>,
    /// "Compact Middle Packages" (`SPEC-0275` v10 behaviour 30): on by default.
    compact: bool,
}

impl ProjectTree {
    /// A tree rooted at `root`, with the root expanded.
    pub fn new(root: &Path) -> Self {
        let mut tree = Self::blank(root);
        tree.expand(0);
        tree
    }

    /// The tree of a remote project: `root` stands for the host folder, `listing` holds what the
    /// session listed (`SPEC-0317` v3 behaviour 7).
    pub fn remote(root: &Path, listing: RemoteListing) -> Self {
        let mut tree = Self::blank(root);
        tree.remote = Some(listing);
        tree.expand(0);
        tree
    }

    /// Whether this is a remote project's tree.
    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    fn blank(root: &Path) -> Self {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        Self {
            nodes: vec![Node {
                path: root.to_path_buf(),
                name,
                dir: true,
                depth: 0,
                parent: None,
                children: None,
                kind: NodeKind::File,
            }],
            expanded: HashSet::new(),
            selected: 0,
            marked: Vec::new(),
            anchor: 0,
            extras: None,
            project_files: false,
            roles: HashMap::new(),
            declared: HashMap::new(),
            derived: None,
            generation: 0,
            remote: None,
            compact: true,
        }
    }

    // ---------------------------------------------------------------- compacted packages

    /// Whether "Compact Middle Packages" is on.
    pub fn compact(&self) -> bool {
        self.compact
    }

    /// Switches "Compact Middle Packages" (`SPEC-0275` v10 behaviour 30). Nothing visible
    /// disappears: switching it off shows the folders of every visible compacted row expanded
    /// down to its innermost folder, switching it on moves a selection on a folder that is now
    /// inside a row to that row.
    pub fn set_compact(&mut self, on: bool) {
        if self.compact == on {
            return;
        }
        if on {
            self.compact = true;
            self.settle_chains();
        } else {
            for row in self.rows() {
                let mut folders = self.folders(row.id);
                folders.pop();
                self.expanded.extend(folders);
            }
            self.compact = false;
        }
    }

    /// Whether folder `id` holds packages: it is a source or test folder of the project model, or
    /// lies in one (the nearest folder with a role, from `id` upwards, decides).
    fn holds_packages(&self, id: usize) -> bool {
        let mut at = Some(id);
        while let Some(i) = at {
            if let Some(info) = self.roles.get(&self.nodes[i].path) {
                return matches!(info.role, FolderRole::Sources | FolderRole::Tests);
            }
            at = self.nodes[i].parent;
        }
        false
    }

    /// The one sub-folder that folder `id` is shown in one row with: `id` is a package (a folder
    /// without a role inside a source or test folder) that holds nothing but that sub-folder.
    fn joined(&self, id: usize) -> Option<usize> {
        if !self.compact || self.roles.is_empty() {
            return None;
        }
        let n = &self.nodes[id];
        let plain =
            |n: &Node| n.dir && n.kind == NodeKind::File && !self.roles.contains_key(&n.path);
        let [child] = n.children.as_deref()? else {
            return None;
        };
        (plain(n) && plain(&self.nodes[*child]) && self.holds_packages(n.parent?)).then_some(*child)
    }

    /// The node whose row shows node `id`: the innermost folder of its compacted row.
    fn tail(&self, mut id: usize) -> usize {
        while let Some(next) = self.joined(id) {
            id = next;
        }
        id
    }

    /// The outermost folder of the row of node `id`.
    fn head(&self, mut id: usize) -> usize {
        while let Some(parent) = self.nodes[id].parent
            && self.joined(parent) == Some(id)
        {
            id = parent;
        }
        id
    }

    /// The folders the row of node `id` stands for, outermost first; the node alone when its row
    /// is not compacted.
    pub fn folders(&self, id: usize) -> Vec<usize> {
        let mut out = vec![self.head(id)];
        while let Some(next) = out.last().and_then(|&last| self.joined(last)) {
            out.push(next);
        }
        out
    }

    /// The paths of [`Self::folders`].
    pub fn folder_paths(&self, id: usize) -> Vec<PathBuf> {
        self.folders(id)
            .into_iter()
            .map(|f| self.nodes[f].path.clone())
            .collect()
    }

    /// Reads the packages below folder `id` as far as they hold nothing but one sub-folder, so the
    /// rows know where a compacted row ends.
    fn load_chains(&mut self, id: usize) {
        if !self.compact || self.roles.is_empty() || !self.holds_packages(id) {
            return;
        }
        for child in self.nodes[id].children.clone().unwrap_or_default() {
            let mut at = child;
            loop {
                let n = &self.nodes[at];
                if !n.dir || n.kind != NodeKind::File || self.roles.contains_key(&n.path) {
                    break;
                }
                self.load(at);
                match self.joined(at) {
                    Some(next) => at = next,
                    None => break,
                }
            }
        }
    }

    /// After the roles or the option changed: reads the chains below every expanded folder and
    /// moves the selection from folders inside a compacted row to the row.
    fn settle_chains(&mut self) {
        let mut open: Vec<usize> = self.expanded.iter().copied().collect();
        open.sort_unstable();
        for id in open {
            self.load_chains(id);
        }
        self.selected = self.tail(self.selected);
        self.anchor = self.tail(self.anchor);
        let mut marked = Vec::new();
        for id in std::mem::take(&mut self.marked) {
            let id = self.tail(id);
            if !marked.contains(&id) {
                marked.push(id);
            }
        }
        self.marked = marked;
    }

    /// The role of folder `id` (a module, sources, tests, resources, excluded output), if it has
    /// one.
    pub fn folder_info(&self, id: usize) -> Option<&FolderInfo> {
        let n = &self.nodes[id];
        (n.dir && n.kind == NodeKind::File)
            .then(|| self.roles.get(&n.path))
            .flatten()
    }

    /// The role the tree shows for the folder at `path` (absolute), if it has one.
    pub fn role_of(&self, path: &Path) -> Option<&FolderInfo> {
        self.roles.get(path)
    }

    /// The project's source folders, sorted: sources, tests and their resources, as the build
    /// files declare them or as they were derived from the files (`SPEC-0332` v2 behaviour 13a).
    pub fn source_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = self
            .roles
            .iter()
            .filter(|(_, info)| idecraft_core::source_roots::is_source_role(info.role))
            .map(|(path, _)| path.clone())
            .collect();
        roots.sort();
        roots
    }

    /// Reads the folder roles of the project at `root` again, e.g. after a build file was linked
    /// or a module unlinked (`SPEC-0048` v5 behaviour 12): the declared ones and the ones derived
    /// from the files (v6 behaviour 14). Walks the project's own sources.
    pub fn reload_roles(&mut self, root: &Path) {
        self.declared = folder_roles(root);
        if self.remote.is_none() {
            self.derived = Some(idecraft_core::source_roots::derived(root));
        }
        self.merge_roles();
        self.settle_chains();
    }

    /// The roles shown: the derived source folders and the declared roles; where both name a
    /// folder the build file wins.
    fn merge_roles(&mut self) {
        let mut roles: HashMap<PathBuf, FolderInfo> = self
            .derived
            .iter()
            .flatten()
            .map(|(path, info)| (path.clone(), info.clone()))
            .collect();
        roles.extend(
            self.declared
                .iter()
                .map(|(path, info)| (path.clone(), info.clone())),
        );
        self.roles = roles;
    }

    /// Whether the source folders derived from the files were read for this tree (`SPEC-0048` v6
    /// behaviour 14). A remote project has none to read.
    pub fn derived_roots_known(&self) -> bool {
        self.derived.is_some() || self.remote.is_some()
    }

    /// Takes the source and test folders derived from the files, as
    /// `idecraft_core::source_roots::derived` read them (absolute paths), and says whether a row
    /// changed: packages below them join to rows, packages below a folder that is none any more
    /// stand alone again. Expanded folders and the selection stay.
    pub fn set_derived_roots(&mut self, derived: BTreeMap<PathBuf, FolderInfo>) -> bool {
        if self.remote.is_some() || self.derived.as_ref() == Some(&derived) {
            return false;
        }
        let nothing_before = self.derived.as_ref().is_none_or(BTreeMap::is_empty);
        let nothing_now = derived.is_empty();
        self.derived = Some(derived);
        if nothing_before && nothing_now {
            return false;
        }
        self.merge_roles();
        self.settle_chains();
        true
    }

    /// How often the tree was read again from disk: a file operation in the IDE, a sync, Reload.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A tree rooted at `root` with the nodes "External Libraries" and "Scratches and Consoles"
    /// below the project folder's subtree (`SPEC-0061` v4 behaviour 16).
    pub fn with_extras(root: &Path, extras: ExtraRoots) -> Self {
        let mut tree = Self::new(root);
        tree.extras = Some(extras);
        // The declared roles; the folders derived from the files come from the window's
        // background reading or `reload_roles`, not from here: this runs on the UI thread.
        tree.declared = folder_roles(root);
        tree.merge_roles();
        tree.load_chains(0);
        for (name, kind) in [
            (LIBRARIES, NodeKind::Libraries),
            (SCRATCHES, NodeKind::Scratches),
        ] {
            tree.nodes.push(Node {
                path: PathBuf::new(),
                name: name.to_owned(),
                dir: true,
                depth: 0,
                parent: None,
                children: None,
                kind,
            });
        }
        tree
    }

    /// The top-level nodes shown: the project folder, then the extra nodes unless the view is
    /// "Project Files".
    fn roots(&self) -> Vec<usize> {
        let mut roots = vec![0];
        if !self.project_files {
            roots.extend(
                self.nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.parent.is_none() && n.kind != NodeKind::File)
                    .map(|(id, _)| id),
            );
        }
        roots
    }

    /// Shows the view "Project Files" (true) or "Project" (false) (`SPEC-0061` v4 behaviour 11); a
    /// selection in a node that disappears goes back to the project folder.
    pub fn set_project_files(&mut self, on: bool) {
        self.project_files = on;
        if on && self.selection().into_iter().any(|id| self.top_of(id) != 0) {
            self.select_one(0);
        }
    }

    /// Whether the view is "Project Files".
    pub fn project_files(&self) -> bool {
        self.project_files
    }

    /// Whether the tree has the extra nodes (the view chooser offers "Project Files" then).
    pub fn has_extras(&self) -> bool {
        self.extras.is_some()
    }

    fn top_of(&self, mut id: usize) -> usize {
        while let Some(parent) = self.nodes[id].parent {
            id = parent;
        }
        id
    }

    /// What node `id` stands for.
    pub fn kind(&self, id: usize) -> NodeKind {
        self.nodes[id].kind
    }

    /// Whether node `id` cannot be renamed, deleted, cut, copied or pasted into: the project folder,
    /// the extra nodes, their libraries and their fixed folders (`SPEC-0061` v4 behaviour 16).
    pub fn is_fixed(&self, id: usize) -> bool {
        let n = &self.nodes[id];
        id == 0
            || n.kind != NodeKind::File
            || n.parent
                .is_some_and(|p| self.nodes[p].kind == NodeKind::Scratches)
    }

    /// Whether node `id` stands for something without a file (an extra node or a library).
    pub fn is_virtual(&self, id: usize) -> bool {
        self.nodes[id].kind != NodeKind::File
    }

    /// Whether any selected node is fixed.
    pub fn selection_fixed(&self) -> bool {
        self.selection().into_iter().any(|id| self.is_fixed(id))
    }

    /// The tree read again from disk, with the same folders expanded and the same path selected
    /// where they still exist (after files were moved, SPEC-0114).
    pub fn reloaded(&self) -> Self {
        // With every folder of a compacted row, so a row that splits (a file appeared in a package
        // in its middle) stays expanded down to where it was.
        let mut open: Vec<&Path> = self
            .expanded
            .iter()
            .flat_map(|&id| self.folders(id))
            .map(|id| self.nodes[id].path.as_path())
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        let open_extras: Vec<NodeKind> = self
            .expanded
            .iter()
            .map(|&id| self.nodes[id].kind)
            .filter(|k| *k != NodeKind::File)
            .collect();
        open.sort_by_key(|p| p.components().count());
        let selected = self.nodes[self.selected].path.clone();
        let marked: Vec<PathBuf> = self.selected_paths();
        let anchor = self.nodes[self.anchor].path.clone();
        let mut tree = match (&self.remote, &self.extras) {
            (Some(listing), _) => Self::remote(&self.nodes[0].path, listing.clone()),
            (None, Some(extras)) => Self::with_extras(&self.nodes[0].path, extras.clone()),
            (None, None) => Self::new(&self.nodes[0].path),
        };
        tree.project_files = self.project_files;
        tree.compact = self.compact;
        // The derived source folders are kept; the window reads them again in the background
        // when it sees that the tree was read again (SPEC-0048 v6 behaviour 14).
        tree.derived = self.derived.clone();
        tree.merge_roles();
        tree.generation = self.generation + 1;
        for kind in open_extras {
            if let Some(id) = tree.nodes.iter().position(|n| n.kind == kind) {
                tree.expand(id);
            }
        }
        for path in open {
            if let Some(id) = tree.nodes.iter().position(|n| n.path == path) {
                tree.expand(id);
            }
        }
        let find = |tree: &Self, path: &Path| tree.nodes.iter().position(|n| n.path == path);
        if let Some(id) = find(&tree, &selected) {
            tree.selected = id;
            tree.anchor = id;
        }
        if let Some(id) = find(&tree, &anchor) {
            tree.anchor = id;
        }
        if marked.len() > 1 {
            tree.marked = marked.iter().filter_map(|p| find(&tree, p)).collect();
        }
        tree.settle_chains();
        tree
    }

    /// The selected nodes in display order (behaviour 24): the visible marked nodes while the
    /// selection holds several, else the selected node.
    pub fn selection(&self) -> Vec<usize> {
        if self.marked.len() > 1 && self.marked.contains(&self.selected) {
            let rows = self.rows();
            let visible: Vec<usize> = rows
                .iter()
                .map(|r| r.id)
                .filter(|id| self.marked.contains(id))
                .collect();
            if !visible.is_empty() {
                return visible;
            }
        }
        vec![self.selected]
    }

    /// Paths of the selected nodes in display order.
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selection()
            .into_iter()
            .map(|id| self.nodes[id].path.clone())
            .collect()
    }

    /// Whether several nodes are selected.
    pub fn several(&self) -> bool {
        self.selection().len() > 1
    }

    /// The selection as visible row indexes for [`crate::tree_nav::RowSelection`].
    pub fn row_selection(&self) -> crate::tree_nav::RowSelection {
        let rows = self.rows();
        let row = |id: usize| rows.iter().position(|r| r.id == id);
        let lead = row(self.selected).unwrap_or(0);
        let mut picked: Vec<usize> = self.selection().into_iter().filter_map(row).collect();
        picked.sort_unstable();
        crate::tree_nav::RowSelection {
            rows: if picked.is_empty() {
                vec![lead]
            } else {
                picked
            },
            anchor: row(self.anchor).unwrap_or(lead),
            lead,
        }
    }

    /// Takes a selection of visible rows (after ⇧-click, ⌘-click, ⇧↑/⇧↓, ⌘A).
    pub fn set_row_selection(&mut self, selection: &crate::tree_nav::RowSelection) {
        let rows = self.rows();
        let id = |i: usize| rows.get(i).map(|r| r.id);
        let Some(lead) = id(selection.lead) else {
            return;
        };
        self.selected = lead;
        self.anchor = id(selection.anchor).unwrap_or(lead);
        self.marked = selection.rows.iter().filter_map(|&i| id(i)).collect();
        if self.marked.len() < 2 {
            self.marked.clear();
        }
    }

    /// One node alone, which also becomes the anchor.
    pub fn select_one(&mut self, id: usize) {
        self.selected = id;
        self.anchor = id;
        self.marked.clear();
    }

    /// A secondary click on node `id`: inside the selection it keeps the selection and becomes its
    /// lead, elsewhere it selects only that node (behaviour 24).
    pub fn secondary_click(&mut self, id: usize) {
        if self.selection().contains(&id) {
            self.selected = id;
        } else {
            self.select_one(id);
        }
    }

    /// Shows and selects every path of `paths` that is in the tree (after a paste of several).
    pub fn reveal_all(&mut self, paths: &[PathBuf]) {
        let mut ids = Vec::new();
        for p in paths {
            if self.reveal(p) {
                ids.push(self.selected);
            }
        }
        if let Some(&first) = ids.first() {
            self.anchor = first;
        }
        if ids.len() > 1 {
            self.marked = ids;
        }
    }

    /// Path of node `id`.
    pub fn path(&self, id: usize) -> &Path {
        &self.nodes[id].path
    }

    /// Whether node `id` is a folder.
    pub fn is_dir(&self, id: usize) -> bool {
        self.nodes[id].dir
    }

    fn load(&mut self, id: usize) {
        if self.nodes[id].children.is_some() || !self.nodes[id].dir {
            return;
        }
        match self.nodes[id].kind {
            NodeKind::Libraries => {
                let entries = library_entries(&self.nodes[0].path);
                self.add_children(
                    id,
                    entries
                        .into_iter()
                        .map(|name| (false, name, PathBuf::new(), NodeKind::Library))
                        .collect(),
                );
                return;
            }
            NodeKind::Scratches => {
                let folders = self
                    .extras
                    .iter()
                    .flat_map(|e| {
                        [
                            ("Scratches", e.scratches.clone()),
                            ("Database Consoles", Some(e.consoles.clone())),
                        ]
                    })
                    .filter_map(|(name, path)| {
                        let path = path.filter(|p| p.is_dir())?;
                        Some((true, name.to_owned(), path, NodeKind::File))
                    })
                    .collect();
                self.add_children(id, folders);
                return;
            }
            NodeKind::File | NodeKind::Library => {}
        }
        if let Some(listing) = &self.remote {
            let dir = self.nodes[id].path.clone();
            // Not listed yet: stays unread, the listing is asked for.
            let Some(entries) = listing.entries(&dir) else {
                return;
            };
            let mut entries: Vec<(bool, String)> = entries
                .into_iter()
                .filter(|(_, name)| !is_ignored(name))
                .collect();
            entries.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
            });
            self.add_children(
                id,
                entries
                    .into_iter()
                    .map(|(d, name)| (d, name.clone(), dir.join(&name), NodeKind::File))
                    .collect(),
            );
            return;
        }
        let mut entries: Vec<(bool, String, PathBuf)> = std::fs::read_dir(&self.nodes[id].path)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (!is_ignored(&name)).then(|| (e.path().is_dir(), name, e.path()))
            })
            .collect();
        entries.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        self.add_children(
            id,
            entries
                .into_iter()
                .map(|(dir, name, path)| (dir, name, path, NodeKind::File))
                .collect(),
        );
    }

    fn add_children(&mut self, id: usize, entries: Vec<(bool, String, PathBuf, NodeKind)>) {
        let depth = self.nodes[id].depth + 1;
        let mut children = Vec::with_capacity(entries.len());
        for (dir, name, path, kind) in entries {
            children.push(self.nodes.len());
            self.nodes.push(Node {
                path,
                name,
                dir,
                depth,
                parent: Some(id),
                children: None,
                kind,
            });
        }
        self.nodes[id].children = Some(children);
    }

    /// Expands a folder, reading it the first time.
    pub fn expand(&mut self, id: usize) {
        if self.nodes[id].dir {
            self.load(id);
            self.load_chains(id);
            self.expanded.insert(id);
        }
    }

    /// Collapses a folder.
    pub fn collapse(&mut self, id: usize) {
        self.expanded.remove(&id);
    }

    /// Collapses every folder but the project folder and selects it when the selection was hidden
    /// ("Collapse All" in the header, `SPEC-0061` v6 behaviour 26).
    pub fn collapse_all(&mut self) {
        self.expanded.retain(|id| *id == 0);
        let mut at = self.selected;
        while let Some(parent) = self.nodes.get(at).and_then(|n| n.parent) {
            if parent != 0 {
                at = parent;
            } else {
                break;
            }
        }
        let at = self.tail(at);
        if at != self.selected {
            self.select_one(at);
        }
    }

    /// Expands the selected folder (the project folder when a file is selected) and every folder
    /// below it, reading at most [`EXPAND_ALL_LIMIT`] folders ("Expand All", `SPEC-0061` v6
    /// behaviour 26).
    pub fn expand_all(&mut self) {
        let start = if self.nodes[self.selected].dir {
            self.selected
        } else {
            0
        };
        self.expand_below(start);
    }

    /// Expands the folder `start` and every folder below it but the ignored ones, reading at most
    /// [`EXPAND_ALL_LIMIT`] folders (`FullyExpandTreeNode`, `SPEC-0275` v13 behaviour 32).
    pub fn expand_below(&mut self, start: usize) {
        let mut todo = vec![start];
        let mut done = 0;
        while let Some(id) = todo.pop() {
            if done >= EXPAND_ALL_LIMIT {
                break;
            }
            if !self.nodes[id].dir || (is_ignored(&self.nodes[id].name) && id != start) {
                continue;
            }
            self.expand(id);
            done += 1;
            if let Some(children) = &self.nodes[id].children {
                todo.extend(children.iter().rev().copied());
            }
        }
    }

    /// Expands a collapsed folder or collapses an expanded one.
    pub fn toggle(&mut self, id: usize) {
        if self.expanded.contains(&id) {
            self.collapse(id);
        } else {
            self.expand(id);
        }
    }

    /// Visible rows in display order.
    pub fn rows(&self) -> Vec<Row> {
        let mut out = Vec::new();
        let mut stack: Vec<(usize, usize)> = self.roots().into_iter().map(|id| (id, 0)).collect();
        stack.reverse();
        while let Some((head, depth)) = stack.pop() {
            // A package that holds nothing but one sub-package shares its row with it: the row is
            // the innermost folder's, named with dots (`SPEC-0275` v10 behaviour 30).
            let mut id = head;
            let mut name = self.nodes[head].name.clone();
            while let Some(next) = self.joined(id) {
                id = next;
                name.push('.');
                name.push_str(&self.nodes[id].name);
            }
            let n = &self.nodes[id];
            let expanded = self.expanded.contains(&id);
            out.push(Row {
                id,
                name,
                depth,
                dir: n.dir,
                expanded,
                kind: n.kind,
                head,
            });
            if expanded && let Some(children) = &n.children {
                stack.extend(children.iter().rev().map(|&c| (c, depth + 1)));
            }
        }
        out
    }

    /// Per row of `rows` (from [`Self::rows`]), whether its name gets the problem mark: a file with
    /// an error or a folder above one (`SPEC-0059` v4 behaviour 10).
    pub fn problem_flags(
        &self,
        rows: &[Row],
        marks: &crate::problem_marks::ProblemFiles,
    ) -> Vec<bool> {
        if marks.is_empty() {
            return vec![false; rows.len()];
        }
        rows.iter()
            .map(|r| marks.marked(self.path(r.id), r.dir))
            .collect()
    }

    /// Moves the selection one visible row up or down.
    pub fn select_step(&mut self, down: bool) {
        let rows = self.rows();
        let at = rows.iter().position(|r| r.id == self.selected).unwrap_or(0);
        let next = if down {
            (at + 1).min(rows.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        self.selected = rows[next].id;
    }

    /// The visible rows as the shared tree behaviour sees them (`SPEC-0275` behaviours 8–11).
    pub fn nav_rows(&self) -> Vec<crate::tree_nav::NavRow> {
        self.rows()
            .iter()
            .map(|r| crate::tree_nav::NavRow::new(r.depth, r.dir, r.expanded))
            .collect()
    }

    /// The visible row index of the selection.
    pub fn selected_row(&self) -> usize {
        self.rows()
            .iter()
            .position(|r| r.id == self.selected)
            .unwrap_or(0)
    }

    /// Carries out a command of the shared tree behaviour on visible row indexes; returns the file
    /// to open for [`crate::tree_nav::TreeCommand::Open`].
    pub fn apply(&mut self, command: crate::tree_nav::TreeCommand) -> Option<PathBuf> {
        use crate::tree_nav::TreeCommand as C;
        let rows = self.rows();
        let id = |i: usize| rows.get(i).map(|r| r.id);
        match command {
            C::None => None,
            C::Select(i) => {
                if let Some(id) = id(i) {
                    self.select_one(id);
                }
                None
            }
            C::Expand(i) => {
                if let Some(id) = id(i) {
                    self.expand(id);
                }
                None
            }
            C::ExpandFully(i) => {
                if let Some(id) = id(i).filter(|id| self.nodes[*id].dir) {
                    self.expand_below(id);
                }
                None
            }
            C::Collapse(i) => {
                if let Some(id) = id(i) {
                    self.collapse(id);
                }
                None
            }
            C::Open(i) => {
                let id = id(i)?;
                self.select_one(id);
                let n = &self.nodes[id];
                (!n.dir && n.kind == NodeKind::File).then(|| n.path.clone())
            }
        }
    }

    /// Whether the folder at `path` is expanded (tests).
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.nodes
            .iter()
            .position(|n| n.path == path)
            .is_some_and(|id| self.expanded.contains(&self.tail(id)))
    }

    /// Path of the selected node.
    pub fn selected_path(&self) -> &Path {
        &self.nodes[self.selected].path
    }

    /// Expands the folders down to `path` and selects it; false when it is not in the tree. A
    /// folder inside a compacted row selects that row.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let Some(id) = self.walk(path) else {
            return false;
        };
        self.select_one(self.tail(id));
        true
    }

    /// Expands the folders above `path` and returns its node, if it is in the project folder.
    fn walk(&mut self, path: &Path) -> Option<usize> {
        let root = self.nodes[0].path.clone();
        let rel = path.strip_prefix(&root).ok()?;
        let mut id = 0;
        for part in rel.components() {
            self.expand(id);
            id = self.nodes[id].children.as_ref().and_then(|c| {
                c.iter()
                    .copied()
                    .find(|&k| self.nodes[k].path.file_name() == Some(part.as_os_str()))
            })?;
        }
        Some(id)
    }

    /// The tree as the MCP tool `project_tree` reports it (`SPEC-0275` v10 behaviour 31): the
    /// option, the view and the visible rows. A row has `name`, `path` (relative to the project
    /// folder where it lies in it, absolute otherwise, none for a node without a file), `depth`,
    /// `kind`, for folders `expanded`, `selected`, and `folders`: every folder the row stands
    /// for, outermost first (several for a compacted row).
    pub fn state(&self) -> serde_json::Value {
        let root = self.nodes[0].path.clone();
        let shown = |p: &Path| match p.strip_prefix(&root) {
            Ok(rel) if rel.as_os_str().is_empty() => ".".to_owned(),
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => p.display().to_string(),
        };
        let selection = self.selection();
        let rows: Vec<serde_json::Value> = self
            .rows()
            .into_iter()
            .map(|r| {
                let n = &self.nodes[r.id];
                let file = n.kind == NodeKind::File && !n.path.as_os_str().is_empty();
                let mut row = serde_json::json!({
                    "name": r.name,
                    "depth": r.depth,
                    "kind": match (n.kind, n.dir) {
                        (NodeKind::File, true) => "folder",
                        (NodeKind::File, false) => "file",
                        (NodeKind::Libraries, _) => "libraries",
                        (NodeKind::Library, _) => "library",
                        (NodeKind::Scratches, _) => "scratches",
                    },
                    "selected": selection.contains(&r.id),
                });
                if file {
                    row["path"] = shown(&n.path).into();
                }
                if n.dir {
                    row["expanded"] = r.expanded.into();
                }
                if file && n.dir {
                    row["folders"] = self
                        .folders(r.id)
                        .into_iter()
                        .map(|f| shown(&self.nodes[f].path))
                        .collect::<Vec<_>>()
                        .into();
                }
                if let Some(info) = self.folder_info(r.id) {
                    row["role"] = serde_json::to_value(info.role).unwrap_or_default();
                }
                row
            })
            .collect();
        serde_json::json!({
            "compact_middle_packages": self.compact,
            "view": if self.project_files { "project_files" } else { "project" },
            "rows": rows,
        })
    }

    /// One action of the MCP tool `project_tree` on the file or folder at `path`: `expand`,
    /// `collapse` or `select`. A folder inside a compacted row means that row. Errors name what
    /// is wrong.
    pub fn act(&mut self, action: &str, path: &Path) -> Result<(), String> {
        let id = self
            .walk(path)
            .ok_or_else(|| format!("`{}` is not in the project tree", path.display()))?;
        let id = self.tail(id);
        match action {
            "select" => self.select_one(id),
            "expand" | "collapse" | "expand_all" if !self.nodes[id].dir => {
                return Err(format!("`{}` is a file", path.display()));
            }
            "expand" => self.expand(id),
            "expand_all" => self.expand_below(id),
            "collapse" => self.collapse(id),
            other => return Err(format!("unknown action `{other}`")),
        }
        Ok(())
    }

    /// Right: expands the selected folder. Left: collapses it, or selects the parent (behaviour 2).
    pub fn select_side(&mut self, right: bool) {
        let id = self.selected;
        if right {
            self.expand(id);
        } else if self.expanded.contains(&id) {
            self.collapse(id);
        } else if let Some(parent) = self.nodes[self.head(id)].parent {
            self.selected = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_folders_are_listed_through_the_session() {
        // SPEC-0317 v3 behaviour 7: nothing is read from this machine's disk.
        let root = PathBuf::from("/nowhere/app");
        let listing = RemoteListing::default();
        let tree = ProjectTree::remote(&root, listing.clone());
        assert!(tree.is_remote());
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(listing.take_wanted(), std::slice::from_ref(&root));
        listing.put(
            root.clone(),
            vec![
                (false, "b.txt".into()),
                (true, "src".into()),
                (false, ".DS_Store".into()),
            ],
        );
        let mut tree = tree.reloaded();
        let names: Vec<String> = tree.rows().into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["app", "src", "b.txt"]);
        let src = tree.rows()[1].id;
        tree.expand(src);
        assert_eq!(listing.take_wanted(), [root.join("src")]);
        listing.put(root.join("src"), vec![(false, "main.rs".into())]);
        listing.forget(&root);
        let tree = tree.reloaded();
        assert_eq!(listing.take_wanted(), std::slice::from_ref(&root));
        assert!(tree.is_remote());
    }

    #[test]
    fn reloading_keeps_expanded_folders() {
        let root =
            std::env::temp_dir().join(format!("idecraft-tree-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/x.txt"), "").unwrap();
        let mut tree = ProjectTree::new(&root);
        let a = tree.rows().iter().find(|r| r.name == "a").unwrap().id;
        tree.expand(a);
        let b = tree.rows().iter().find(|r| r.name == "b").unwrap().id;
        tree.expand(b);
        std::fs::write(root.join("a/b/y.txt"), "").unwrap();
        let names: Vec<String> = tree.reloaded().rows().into_iter().map(|r| r.name).collect();
        assert!(names.contains(&"x.txt".to_owned()) && names.contains(&"y.txt".to_owned()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tree_rows_mark_problem_files_and_their_folders() {
        use crate::diagnostics_model::Severity;
        use crate::problem_marks::ProblemFiles;
        let root = fixture("problems");
        let mut tree = ProjectTree::new(&root);
        let src = tree.rows().iter().find(|r| r.name == "src").unwrap().id;
        tree.expand(src);
        let inner = tree.rows().iter().find(|r| r.name == "inner").unwrap().id;
        tree.expand(inner);
        let file = root.join("src/inner/deep.rs");
        let marked = |tree: &ProjectTree, marks: &ProblemFiles| -> Vec<String> {
            let rows = tree.rows();
            let flags = tree.problem_flags(&rows, marks);
            rows.into_iter()
                .zip(flags)
                .filter(|(_, on)| *on)
                .map(|(r, _)| r.name)
                .collect()
        };
        let root_name = tree.rows()[0].name.clone();
        let errors = ProblemFiles::from_severities([(file.as_path(), [Severity::Error])]);
        assert_eq!(
            marked(&tree, &errors),
            [root_name.as_str(), "src", "inner", "deep.rs"],
            "the file and every folder above it, nothing else"
        );
        let warnings = ProblemFiles::from_severities([(file.as_path(), [Severity::Warning])]);
        assert!(marked(&tree, &warnings).is_empty(), "warnings do not mark");
        // A collapsed folder still shows the error inside it.
        tree.collapse(src);
        assert_eq!(marked(&tree, &errors), [root_name.as_str(), "src"]);
        // Fixed: no mark is left.
        assert!(marked(&tree, &ProblemFiles::default()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A folder per test: tests run in parallel and must not share it.
    fn fixture(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("idecraft-tree-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["src/inner", "Docs", ".git", "__pycache__"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in [
            "b.txt",
            "A.md",
            "x.pyc",
            "src/main.rs",
            "src/inner/deep.rs",
            "backup~",
        ] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        dir
    }

    /// SPEC-0275 AK-30 (v13): expanding a folder fully opens the folders below it, not the ones
    /// beside it; the same through the tree's key and through `act`.
    #[test]
    fn expanding_a_folder_with_everything_below() {
        let dir = fixture("expanding_a_folder_fully");
        std::fs::create_dir_all(dir.join("Docs/more")).unwrap();
        std::fs::write(dir.join("Docs/more/inner.md"), "").unwrap();
        let names = |t: &ProjectTree| t.rows().iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        let mut t = ProjectTree::new(&dir);
        let src = t.rows().iter().position(|r| r.name == "src").unwrap();
        let command =
            crate::tree_nav::on_key(&t.nav_rows(), src, crate::tree_nav::TreeKey::ExpandFully);
        assert_eq!(command, crate::tree_nav::TreeCommand::ExpandFully(src));
        assert_eq!(t.apply(command), None);
        let shown = names(&t);
        assert!(shown.contains(&"deep.rs".to_owned()), "{shown:?}");
        assert!(!shown.contains(&"inner.md".to_owned()), "{shown:?}");
        // On a file the command does nothing.
        let file = t.rows().iter().position(|r| r.name == "deep.rs").unwrap();
        let before = names(&t);
        t.apply(crate::tree_nav::TreeCommand::ExpandFully(file));
        assert_eq!(names(&t), before);
        // MCP: `expand_all` with a path.
        let mut t = ProjectTree::new(&dir);
        t.act("expand_all", &dir.join("Docs")).unwrap();
        let shown = names(&t);
        assert!(shown.contains(&"inner.md".to_owned()), "{shown:?}");
        assert!(!shown.contains(&"deep.rs".to_owned()), "{shown:?}");
        assert!(
            t.act("expand_all", &dir.join("b.txt"))
                .unwrap_err()
                .contains("is a file")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0061 v6 AK-18: "Expand All" opens every folder below the selection, "Collapse All"
    /// closes all but the project folder and moves a hidden selection up.
    #[test]
    fn expand_all_and_collapse_all() {
        let dir = fixture("expand_all_and_collapse_all");
        let mut t = ProjectTree::new(&dir);
        t.expand_all();
        let names: Vec<String> = t.rows().iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"deep.rs".to_owned()), "{names:?}");
        let deep = t.rows().iter().find(|r| r.name == "deep.rs").unwrap().id;
        t.select_one(deep);
        t.collapse_all();
        let names: Vec<String> = t.rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(names[1..], ["Docs", "src", "A.md", "b.txt"]);
        assert_eq!(t.rows()[t.selected_row()].name, "src");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lazy_sorted_and_filtered() {
        let dir = fixture("lazy_sorted_and_filtered");
        let mut t = ProjectTree::new(&dir);
        let names: Vec<String> = t.rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(names[1..], ["Docs", "src", "A.md", "b.txt"]);
        assert_eq!(t.nodes.len(), 5, "only the root was read");
        let src = t.rows()[2].id;
        t.expand(src);
        let rows = t.rows();
        assert_eq!(rows[3].name, "inner");
        assert_eq!(rows[3].depth, 2);
        assert_eq!(rows[4].name, "main.rs");
        t.toggle(src);
        assert_eq!(t.rows().len(), 5);
        assert!(is_ignored("x.pyo") && is_ignored(".ruff_cache") && !is_ignored("git"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shared_tree_behaviour_and_reveal() {
        use crate::tree_nav::{TreeCommand, TreeKey, on_click, on_key};
        let dir = fixture("shared_tree_behaviour");
        let mut t = ProjectTree::new(&dir);
        // Rows: root, Docs, src, A.md, b.txt; one click on src's arrow expands it.
        for c in on_click(&t.nav_rows(), 2, true, 1) {
            t.apply(c);
        }
        assert!(t.is_expanded(&dir.join("src")));
        assert_eq!(t.selected_path(), dir.join("src"));
        // → on an expanded folder goes to its first child, ← back to the parent.
        let c = on_key(&t.nav_rows(), t.selected_row(), TreeKey::Right);
        t.apply(c);
        assert_eq!(t.selected_path(), dir.join("src/inner"));
        let c = on_key(&t.nav_rows(), t.selected_row(), TreeKey::Left);
        t.apply(c);
        assert_eq!(t.selected_path(), dir.join("src"));
        // A double click on a file opens it, a single click only selects.
        let rows = t.rows();
        let main = rows.iter().position(|r| r.name == "main.rs").unwrap();
        let commands = on_click(&t.nav_rows(), main, false, 1);
        assert_eq!(commands, [TreeCommand::Select(main)]);
        let open: Vec<_> = on_click(&t.nav_rows(), main, false, 2)
            .into_iter()
            .filter_map(|c| t.apply(c))
            .collect();
        assert_eq!(open, [dir.join("src/main.rs")]);
        // A double click on a folder toggles it.
        for c in on_click(&t.nav_rows(), 2, false, 2) {
            t.apply(c);
        }
        assert!(!t.is_expanded(&dir.join("src")));
        // Reveal expands down to a file.
        let mut t = ProjectTree::new(&dir);
        assert!(t.reveal(&dir.join("src/inner/deep.rs")));
        assert_eq!(t.selected_path(), dir.join("src/inner/deep.rs"));
        assert!(!t.reveal(&dir.join("nope.txt")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keyboard_navigation() {
        let dir = fixture("keyboard_navigation");
        let mut t = ProjectTree::new(&dir);
        t.select_step(true);
        t.select_step(true);
        assert_eq!(t.path(t.selected).file_name().unwrap(), "src");
        t.select_side(true);
        t.select_step(true);
        assert_eq!(t.path(t.selected).file_name().unwrap(), "inner");
        t.select_side(false);
        assert_eq!(t.path(t.selected).file_name().unwrap(), "src", "parent");
        t.select_side(false);
        assert_eq!(t.rows().len(), 5, "collapsed");
        t.select_step(false);
        t.select_step(false);
        t.select_step(false);
        assert_eq!(t.selected, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A project with a Cargo build file and a scratches folder outside it (SPEC-0061 v4 AK-8).
    fn extras_fixture(test: &str) -> (PathBuf, ExtraRoots) {
        let base =
            std::env::temp_dir().join(format!("idecraft-extras-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("proj");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nrust-version = \"1.85\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let scratches = base.join("config/scratches");
        std::fs::create_dir_all(&scratches).unwrap();
        std::fs::write(scratches.join("scratch.sql"), "select 1;\n").unwrap();
        let extras = ExtraRoots {
            scratches: Some(scratches),
            consoles: dir.join(".idecraft/consoles"),
        };
        (dir, extras)
    }

    #[test]
    fn extra_nodes_follow_the_project_and_open_scratches() {
        let (dir, extras) = extras_fixture("follow");
        let mut t = ProjectTree::with_extras(&dir, extras.clone());
        let names =
            |t: &ProjectTree| -> Vec<String> { t.rows().into_iter().map(|r| r.name).collect() };
        let top: Vec<String> = t
            .rows()
            .into_iter()
            .filter(|r| r.depth == 0)
            .map(|r| r.name)
            .collect();
        assert_eq!(top, ["proj", LIBRARIES, SCRATCHES]);
        assert_eq!(names(&t).last().map(String::as_str), Some(SCRATCHES));
        // External Libraries: the toolchain, then the dependency.
        let libs = t.rows().iter().find(|r| r.name == LIBRARIES).unwrap().id;
        assert_eq!(t.kind(libs), NodeKind::Libraries);
        t.expand(libs);
        let below: Vec<String> = t
            .rows()
            .into_iter()
            .filter(|r| r.kind == NodeKind::Library)
            .map(|r| r.name)
            .collect();
        assert_eq!(below, ["< rust 1.85 >", "Cargo: serde:1"]);
        // Scratches and Consoles: only the folders that exist; a scratch file opens.
        let scratch = t.rows().iter().find(|r| r.name == SCRATCHES).unwrap().id;
        t.expand(scratch);
        assert_eq!(names(&t).last().map(String::as_str), Some("Scratches"));
        let folder = t.rows().iter().find(|r| r.name == "Scratches").unwrap().id;
        assert!(t.is_fixed(folder), "the folder itself is fixed");
        t.expand(folder);
        let rows = t.rows();
        let file = rows.iter().position(|r| r.name == "scratch.sql").unwrap();
        assert!(!t.is_fixed(rows[file].id), "files in it are ordinary");
        assert_eq!(
            t.apply(crate::tree_nav::TreeCommand::Open(file)),
            Some(extras.scratches.clone().unwrap().join("scratch.sql"))
        );
        // A library opens nothing and is fixed.
        let rows = t.rows();
        let lib = rows
            .iter()
            .position(|r| r.kind == NodeKind::Library)
            .unwrap();
        assert_eq!(t.apply(crate::tree_nav::TreeCommand::Open(lib)), None);
        assert!(t.selection_fixed());
        assert!(t.is_fixed(0) && t.is_fixed(libs) && t.is_fixed(scratch));
        // Consoles appear once the folder exists.
        std::fs::create_dir_all(&extras.consoles).unwrap();
        let t2 = t.reloaded();
        let n2 = names(&t2);
        assert!(n2.contains(&"Database Consoles".to_owned()), "{n2:?}");
        assert!(
            n2.contains(&"scratch.sql".to_owned()),
            "kept expanded: {n2:?}"
        );
        assert!(
            n2.contains(&"Cargo: serde:1".to_owned()),
            "kept expanded: {n2:?}"
        );
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn extra_nodes_leave_the_project_files_view() {
        let (dir, extras) = extras_fixture("view");
        let mut t = ProjectTree::with_extras(&dir, extras);
        assert!(t.has_extras());
        let scratch = t.rows().iter().find(|r| r.name == SCRATCHES).unwrap().id;
        t.select_one(scratch);
        t.set_project_files(true);
        assert!(t.project_files());
        assert!(t.rows().iter().all(|r| r.kind == NodeKind::File));
        assert_eq!(
            t.selected, 0,
            "the selection goes back to the project folder"
        );
        let t = t.reloaded();
        assert!(t.project_files(), "a reload keeps the view");
        let mut t = t;
        t.set_project_files(false);
        assert!(t.rows().iter().any(|r| r.name == LIBRARIES));
        // Without extras (other trees) nothing changes.
        assert!(!ProjectTree::new(&dir).has_extras());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    /// SPEC-0275 v3 behaviour 24 (AK-13).
    #[test]
    fn several_selected_rows_survive_a_reload() {
        use crate::tree_nav::{ClickMode, SelectKey};
        let dir =
            std::env::temp_dir().join(format!("idecraft-tree-several-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        for f in ["a.txt", "b.txt", "c.txt", "src/inner.rs"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let mut t = ProjectTree::new(&dir);
        let names = |t: &ProjectTree| -> Vec<String> {
            t.selected_paths()
                .iter()
                .map(|p| p.strip_prefix(&dir).unwrap().display().to_string())
                .collect()
        };
        // Rows: root, src, a.txt, b.txt, c.txt.
        t.apply(crate::tree_nav::TreeCommand::Select(2));
        assert!(!t.several());
        let s = t.row_selection().click(4, ClickMode::Range);
        t.set_row_selection(&s);
        assert_eq!(names(&t), ["a.txt", "b.txt", "c.txt"]);
        // ⌘-click on src adds it; expanding it shows inner.rs unselected.
        let s = t.row_selection().click(1, ClickMode::Toggle);
        t.set_row_selection(&s);
        assert_eq!(names(&t), ["src", "a.txt", "b.txt", "c.txt"]);
        t.expand(1);
        assert_eq!(names(&t), ["src", "a.txt", "b.txt", "c.txt"]);
        // A reload keeps the selection by path.
        let mut t = t.reloaded();
        assert_eq!(names(&t), ["src", "a.txt", "b.txt", "c.txt"]);
        // A secondary click inside keeps it, outside selects one.
        let inner = t.rows()[2].id;
        let a = t.rows()[3].id;
        t.secondary_click(a);
        assert_eq!(t.selection().len(), 4);
        assert_eq!(t.selected, a);
        t.secondary_click(inner);
        assert_eq!(names(&t), ["src/inner.rs"]);
        // ⌘A, then a plain arrow selects one row again.
        let s = t.row_selection().key(SelectKey::All, t.rows().len());
        t.set_row_selection(&s);
        assert_eq!(t.selection().len(), 6);
        t.collapse(t.rows()[1].id);
        assert_eq!(t.selection().len(), 5, "hidden rows leave the selection");
        t.apply(crate::tree_nav::TreeCommand::Select(3));
        assert_eq!(names(&t), ["b.txt"]);
        t.reveal_all(&[dir.join("a.txt"), dir.join("src/inner.rs")]);
        assert_eq!(names(&t), ["src/inner.rs", "a.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `SPEC-0275` v10 behaviours 30 and 31: packages in source folders as one row.
#[cfg(test)]
mod compact_tests {
    use super::*;
    use crate::tree_nav::{self, ClickMode, SelectKey, TreeCommand, TreeKey};

    const POM: &str = "<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId>\
        <artifactId>shop</artifactId><version>1</version></project>";

    /// A Maven project: `lab.shop.core` with a class and an empty `util`, `lab.shop.app` only in
    /// the tests, resources and a folder outside the source folders with chains of their own.
    fn project(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-compact-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in [
            "src/main/java/lab/shop/core/util",
            "src/test/java/lab/shop/core",
            "src/test/java/lab/shop/app",
            "src/main/resources/db/migration",
            "docs/a/b",
        ] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for (f, text) in [
            ("pom.xml", POM),
            ("src/main/java/lab/shop/core/Foo.java", "class Foo {}"),
            (
                "src/test/java/lab/shop/core/FooTest.java",
                "class FooTest {}",
            ),
            (
                "src/test/java/lab/shop/app/AppTest.java",
                "class AppTest {}",
            ),
            ("src/main/resources/db/migration/V1.sql", ""),
            ("docs/a/b/c.txt", ""),
        ] {
            std::fs::write(dir.join(f), text).unwrap();
        }
        dir.canonicalize().unwrap()
    }

    fn tree(dir: &Path) -> ProjectTree {
        ProjectTree::with_extras(
            dir,
            ExtraRoots {
                scratches: None,
                consoles: dir.join(".idecraft/consoles"),
            },
        )
    }

    /// The rows below the project folder as `depth name`, extra nodes left out.
    fn shown(t: &ProjectTree) -> Vec<String> {
        t.rows()
            .into_iter()
            .skip(1)
            .filter(|r| r.kind == NodeKind::File)
            .map(|r| format!("{} {}", r.depth, r.name))
            .collect()
    }

    fn row(t: &ProjectTree, name: &str) -> Row {
        t.rows()
            .into_iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no row {name} in {:?}", shown(t)))
    }

    fn open(t: &mut ProjectTree, names: &[&str]) {
        for name in names {
            let id = row(t, name).id;
            t.expand(id);
        }
    }

    fn rel(dir: &Path, paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|p| p.strip_prefix(dir).unwrap().display().to_string())
            .collect()
    }

    /// SPEC-0275 v10 AK-25a: a chain of packages is one row below the source folder; a package with a file or
    /// with two sub-packages ends the row; folders above and outside source folders and in
    /// resource folders stay folders.
    #[test]
    fn packages_in_source_folders_are_one_row() {
        let dir = project("rows");
        let mut t = tree(&dir);
        assert!(t.compact());
        t.select_one(0);
        t.expand_all();
        assert_eq!(
            shown(&t),
            [
                "1 docs",
                "2 a",
                "3 b",
                "4 c.txt",
                "1 src",
                "2 main",
                "3 java",
                "4 lab.shop.core",
                "5 util",
                "5 Foo.java",
                "3 resources",
                "4 db",
                "5 migration",
                "6 V1.sql",
                "2 test",
                "3 java",
                "4 lab.shop",
                "5 app",
                "6 AppTest.java",
                "5 core",
                "6 FooTest.java",
                "1 pom.xml",
            ]
        );
        // The row is the innermost folder's and knows every folder it stands for.
        let core = row(&t, "lab.shop.core");
        assert_eq!(t.path(core.id), dir.join("src/main/java/lab/shop/core"));
        assert_eq!(t.path(core.head), dir.join("src/main/java/lab"));
        assert_eq!(
            rel(&dir, &t.folder_paths(core.id)),
            [
                "src/main/java/lab",
                "src/main/java/lab/shop",
                "src/main/java/lab/shop/core"
            ]
        );
        let util = row(&t, "util");
        assert_eq!(util.head, util.id);
        assert_eq!(t.folders(util.id), [util.id]);
        // The source folder itself keeps its row and its role.
        let java = t.rows().into_iter().find(|r| r.name == "java").unwrap();
        assert_eq!(
            t.folder_info(java.id).map(|i| i.role),
            Some(FolderRole::Sources)
        );
        assert!(t.folder_info(core.id).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25b: the row is known before it is expanded, and an empty innermost package is part of
    /// it.
    #[test]
    fn a_collapsed_row_already_has_its_full_name() {
        let dir = project("collapsed");
        std::fs::create_dir_all(dir.join("src/main/java/org/empty/leaf")).unwrap();
        let mut t = tree(&dir);
        open(&mut t, &["src", "main", "java"]);
        let rows = shown(&t);
        assert!(rows.contains(&"4 lab.shop.core".to_owned()), "{rows:?}");
        assert!(rows.contains(&"4 org.empty.leaf".to_owned()), "{rows:?}");
        assert!(!row(&t, "lab.shop.core").expanded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25c: the option off shows plain folders; switching keeps what was visible and the
    /// selection.
    #[test]
    fn the_option_switches_between_rows_and_folders() {
        let dir = project("option");
        let mut t = tree(&dir);
        open(&mut t, &["src", "main", "java", "lab.shop.core"]);
        let core = row(&t, "lab.shop.core").id;
        t.select_one(core);
        t.set_compact(false);
        assert!(!t.compact());
        let rows = shown(&t);
        let at = rows.iter().position(|r| r == "4 lab").unwrap();
        assert_eq!(
            rows[at..at + 5],
            ["4 lab", "5 shop", "6 core", "7 util", "7 Foo.java"],
            "every folder of the row is shown expanded"
        );
        assert_eq!(t.selected, core, "the innermost folder stays selected");
        // Select the middle folder and switch back: the selection is the row.
        let shop = row(&t, "shop").id;
        let lab = row(&t, "lab").id;
        let s = tree_nav::RowSelection {
            rows: vec![at + 1, at + 2],
            anchor: at + 1,
            lead: at + 2,
        };
        t.set_row_selection(&s);
        assert_eq!(t.selection(), [lab, shop]);
        t.set_compact(true);
        assert!(shown(&t).contains(&"4 lab.shop.core".to_owned()));
        assert_eq!(t.selection(), [core]);
        assert_eq!(t.selected_path(), dir.join("src/main/java/lab/shop/core"));
        // A reload keeps the option.
        t.set_compact(false);
        let t = t.reloaded();
        assert!(!t.compact());
        assert!(shown(&t).contains(&"5 shop".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25d: expand, collapse and the keys of the shared tree behaviour work on the row.
    #[test]
    fn expand_collapse_and_keys_on_a_compacted_row() {
        let dir = project("keys");
        let mut t = tree(&dir);
        open(&mut t, &["src", "main", "java"]);
        let at = t
            .rows()
            .iter()
            .position(|r| r.name == "lab.shop.core")
            .unwrap();
        t.apply(TreeCommand::Select(at));
        let key = |t: &mut ProjectTree, k: TreeKey| {
            let c = tree_nav::on_key(&t.nav_rows(), t.selected_row(), k);
            t.apply(c)
        };
        // → expands, → again goes to the first child, ← goes back to the row, ← collapses it.
        key(&mut t, TreeKey::Right);
        assert!(row(&t, "lab.shop.core").expanded);
        assert!(t.is_expanded(&dir.join("src/main/java/lab/shop/core")));
        assert!(
            t.is_expanded(&dir.join("src/main/java/lab")),
            "any folder of the row"
        );
        key(&mut t, TreeKey::Right);
        assert_eq!(
            t.selected_path(),
            dir.join("src/main/java/lab/shop/core/util")
        );
        key(&mut t, TreeKey::Left);
        assert_eq!(t.selected_row(), at);
        key(&mut t, TreeKey::Left);
        assert!(!row(&t, "lab.shop.core").expanded);
        // ← on the collapsed row goes to the source folder, ↓ comes back.
        key(&mut t, TreeKey::Left);
        assert_eq!(t.selected_path(), dir.join("src/main/java"));
        key(&mut t, TreeKey::Down);
        assert_eq!(t.selected_row(), at);
        // Enter toggles; a double click too; a file below opens.
        key(&mut t, TreeKey::Enter);
        assert!(row(&t, "lab.shop.core").expanded);
        for c in tree_nav::on_click(&t.nav_rows(), at, false, 2) {
            t.apply(c);
        }
        assert!(!row(&t, "lab.shop.core").expanded);
        for c in tree_nav::on_click(&t.nav_rows(), at, true, 1) {
            t.apply(c);
        }
        let foo = t.rows().iter().position(|r| r.name == "Foo.java").unwrap();
        assert_eq!(
            t.apply(TreeCommand::Open(foo)),
            Some(dir.join("src/main/java/lab/shop/core/Foo.java"))
        );
        // The model's own side step: left from the row's child is the row, left from the collapsed
        // row is the source folder.
        t.select_side(false);
        assert_eq!(t.selected_row(), at);
        t.select_side(false);
        t.select_side(false);
        assert_eq!(t.selected_path(), dir.join("src/main/java"));
        // Collapse All leaves the selection on a visible row.
        t.reveal(&dir.join("src/main/java/lab/shop/core/Foo.java"));
        t.collapse_all();
        assert_eq!(t.selected_path(), dir.join("src"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25e: "Select Opened File" and reveal: a file below a row is shown and selected; a folder
    /// in the middle of a row selects the row.
    #[test]
    fn reveal_selects_the_row() {
        let dir = project("reveal");
        let mut t = tree(&dir);
        assert!(t.reveal(&dir.join("src/main/java/lab/shop/core/Foo.java")));
        assert_eq!(
            t.selected_path(),
            dir.join("src/main/java/lab/shop/core/Foo.java")
        );
        let rows = shown(&t);
        assert!(rows.contains(&"4 lab.shop.core".to_owned()), "{rows:?}");
        assert!(rows.contains(&"5 Foo.java".to_owned()), "{rows:?}");
        assert!(t.reveal(&dir.join("src/main/java/lab/shop")));
        assert_eq!(t.selected, row(&t, "lab.shop.core").id);
        assert!(t.reveal(&dir.join("src/test/java/lab")));
        assert_eq!(t.selected, row(&t, "lab.shop").id);
        assert!(!t.reveal(&dir.join("src/main/java/nowhere")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25f: New, Rename, Delete, Cut, Copy and Paste take the row's path, the innermost folder;
    /// after each the tree read again shows the row as the folders now are.
    #[test]
    fn file_operations_use_the_innermost_folder() {
        let dir = project("ops");
        let java = dir.join("src/main/java");
        let mut t = tree(&dir);
        open(&mut t, &["src", "main", "java", "lab.shop.core"]);
        let core = row(&t, "lab.shop.core").id;
        t.select_one(core);
        assert_eq!(rel(&java, &t.selected_paths()), ["lab/shop/core"]);
        assert!(!t.is_fixed(core) && !t.is_virtual(core) && t.is_dir(core));
        // New in the row: the file lands in `core`, the row stays.
        std::fs::write(java.join("lab/shop/core/Bar.java"), "class Bar {}").unwrap();
        let mut t = t.reloaded();
        assert!(t.reveal(&java.join("lab/shop/core/Bar.java")));
        assert!(shown(&t).contains(&"5 Bar.java".to_owned()));
        // New folder in an empty innermost package: the row grows by it.
        std::fs::create_dir_all(java.join("lab/shop/core/util/text")).unwrap();
        let mut t = t.reloaded();
        assert!(t.reveal(&java.join("lab/shop/core/util/text")));
        assert_eq!(t.selected, row(&t, "util.text").id);
        // Rename renames the innermost folder only.
        std::fs::rename(java.join("lab/shop/core"), java.join("lab/shop/kern")).unwrap();
        let mut t = t.reloaded();
        assert!(t.reveal(&java.join("lab/shop/kern")));
        assert_eq!(t.selected, row(&t, "lab.shop.kern").id);
        assert!(t.rows().iter().all(|r| r.name != "lab.shop.core"));
        // A file in a package in the middle splits the row; it stays expanded down to where it
        // was.
        t.expand(t.selected);
        std::fs::write(java.join("lab/shop/package-info.java"), "").unwrap();
        let t = t.reloaded();
        let rows = shown(&t);
        let at = rows.iter().position(|r| r == "4 lab.shop").unwrap();
        assert_eq!(
            rows[at..at + 4],
            ["4 lab.shop", "5 kern", "6 util.text", "6 Bar.java"]
        );
        assert_eq!(t.selected_path(), java.join("lab/shop/kern"));
        // Delete removes the innermost folder; what is left of the row stays.
        std::fs::remove_file(java.join("lab/shop/package-info.java")).unwrap();
        std::fs::remove_dir_all(java.join("lab/shop/kern")).unwrap();
        let t = t.reloaded();
        let rows = shown(&t);
        assert!(rows.contains(&"4 lab.shop".to_owned()), "{rows:?}");
        assert!(rows.iter().all(|r| !r.contains("kern")), "{rows:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25g: several rows: ranges, ⌘-click and ⌘A count a compacted row once, with the
    /// innermost folder as its path.
    #[test]
    fn several_rows_with_compacted_rows() {
        let dir = project("several");
        let mut t = tree(&dir);
        open(&mut t, &["src", "test", "java", "lab.shop"]);
        let at =
            |t: &ProjectTree, name: &str| t.rows().iter().position(|r| r.name == name).unwrap();
        let (pkg, app, core) = (at(&t, "lab.shop"), at(&t, "app"), at(&t, "core"));
        t.apply(TreeCommand::Select(pkg));
        let s = t.row_selection().click(core, ClickMode::Range);
        t.set_row_selection(&s);
        assert_eq!(
            rel(&dir.join("src/test/java"), &t.selected_paths()),
            ["lab/shop", "lab/shop/app", "lab/shop/core"]
        );
        let s = t.row_selection().click(app, ClickMode::Toggle);
        t.set_row_selection(&s);
        assert_eq!(
            rel(&dir.join("src/test/java"), &t.selected_paths()),
            ["lab/shop", "lab/shop/core"]
        );
        // A reload keeps the selection by path.
        let mut t = t.reloaded();
        assert_eq!(t.selection().len(), 2);
        // ⇧↓ from the row extends over its children; ⌘A takes every row once.
        t.apply(TreeCommand::Select(pkg));
        let s = t.row_selection().key(SelectKey::ExtendDown, t.rows().len());
        t.set_row_selection(&s);
        assert_eq!(
            rel(&dir.join("src/test/java"), &t.selected_paths()),
            ["lab/shop", "lab/shop/app"]
        );
        let all = t.row_selection().key(SelectKey::All, t.rows().len());
        t.set_row_selection(&all);
        assert_eq!(t.selection().len(), t.rows().len());
        // A secondary click on the row inside the selection keeps it.
        let id = t.rows()[pkg].id;
        t.secondary_click(id);
        assert_eq!(t.selected, id);
        assert!(t.several());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25h: a folder with a role of its own (a module, a source folder) is never inside a row,
    /// so its icon, tint and bold name stay; errors below a row mark the row.
    #[test]
    fn roles_and_problem_marks_on_rows() {
        use crate::diagnostics_model::Severity;
        use crate::problem_marks::ProblemFiles;
        let dir = project("roles");
        let mut t = tree(&dir);
        let shop = dir.join("src/main/java/lab/shop");
        t.roles.insert(
            shop.clone(),
            FolderInfo {
                role: FolderRole::Module,
                module_name: Some("inner".into()),
            },
        );
        open(&mut t, &["src", "main", "java"]);
        let rows = shown(&t);
        assert!(rows.contains(&"4 lab".to_owned()), "{rows:?}");
        open(&mut t, &["lab"]);
        let module = row(&t, "shop");
        assert_eq!(
            t.folder_info(module.id).and_then(|i| i.module_name.clone()),
            Some("inner".to_owned())
        );
        // Back to the plain project: the row, and an error below it marks it.
        t.roles.remove(&shop);
        t.settle_chains();
        let core = row(&t, "lab.shop.core");
        let file = dir.join("src/main/java/lab/shop/core/Foo.java");
        let marks = ProblemFiles::from_severities([(file.as_path(), [Severity::Error])]);
        let rows = t.rows();
        let flags = t.problem_flags(&rows, &marks);
        let marked: Vec<&str> = rows
            .iter()
            .zip(&flags)
            .filter(|(_, on)| **on)
            .map(|(r, _)| r.name.as_str())
            .collect();
        assert!(marked.contains(&"lab.shop.core"), "{marked:?}");
        assert_eq!(core.id, row(&t, "lab.shop.core").id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-25i: a project without source folders of the project model, a plain tree
    /// and a remote project are never compacted.
    #[test]
    fn nothing_is_compacted_without_source_folders() {
        let dir = project("plain");
        std::fs::remove_file(dir.join("pom.xml")).unwrap();
        for mut t in [tree(&dir), ProjectTree::new(&dir)] {
            t.select_one(0);
            t.expand_all();
            let rows = shown(&t);
            assert!(rows.contains(&"4 lab".to_owned()), "{rows:?}");
            assert!(rows.iter().all(|r| !r.contains("lab.")), "{rows:?}");
        }
        let listing = RemoteListing::default();
        let host = PathBuf::from("/nowhere/shop");
        for (folder, sub) in [("", "src"), ("src", "main"), ("src/main", "java")] {
            listing.put(host.join(folder), vec![(true, sub.to_owned())]);
        }
        listing.put(host.join("src/main/java"), vec![(true, "lab".into())]);
        listing.put(host.join("src/main/java/lab"), vec![(true, "shop".into())]);
        let mut t = ProjectTree::remote(&host, listing);
        t.expand_all();
        assert_eq!(shown(&t), ["1 src", "2 main", "3 java", "4 lab", "5 shop"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The project of OBS-0031 (finding T1): Java sources in the Maven layout without any build
    /// file, with a deep package, an empty chain, a package with two sub-packages, tests, and a
    /// folder outside the sources.
    fn plain_project(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "idecraft-derived-roots-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        // The project is its own repository, wherever the temporary folder lies.
        for d in [".git", "src/main/java/lab/empty/a/b"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for (f, text) in [
            (
                "src/main/java/lab/inventory/Inventory.java",
                "package lab.inventory;\n\npublic class Inventory {}\n",
            ),
            (
                "src/main/java/lab/inventory/util/deep/Leaf.java",
                "package lab.inventory.util.deep;\n\nclass Leaf {}\n",
            ),
            (
                "src/main/java/lab/two/x/X.java",
                "package lab.two.x;\n\nclass X {}\n",
            ),
            (
                "src/main/java/lab/two/y/Y.java",
                "package lab.two.y;\n\nclass Y {}\n",
            ),
            (
                "src/test/java/lab/inventory/InventoryTest.java",
                "package lab.inventory;\n\nclass InventoryTest {}\n",
            ),
            ("docs/one/two/note.txt", ""),
        ] {
            let path = dir.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir.canonicalize().unwrap()
    }

    /// SPEC-0275 v11 AK-28 (SPEC-0048 v6 behaviour 14; OBS-0031 T1): in a project without a build
    /// file the source folders come from the files, and packages below them are rows as below a
    /// declared source folder.
    #[test]
    fn derived_source_folders_compact_packages() {
        let dir = plain_project("rows");
        let mut t = tree(&dir);
        // Before the derived folders are read: folder by folder, no role.
        assert!(!t.derived_roots_known());
        t.select_one(0);
        t.expand_all();
        let before = shown(&t);
        assert!(before.contains(&"5 empty".to_owned()), "{before:?}");
        assert!(
            !before
                .iter()
                .any(|r| r.contains('.') && !r.contains(".java") && !r.contains(".txt"))
        );
        t.reload_roles(&dir);
        assert!(t.derived_roots_known());
        t.select_one(0);
        t.expand_all();
        assert_eq!(
            shown(&t),
            [
                "1 docs",
                "2 one",
                "3 two",
                "4 note.txt",
                "1 src",
                "2 main",
                "3 java",
                "4 lab",
                "5 empty.a.b",
                "5 inventory",
                "6 util.deep",
                "7 Leaf.java",
                "6 Inventory.java",
                "5 two",
                "6 x",
                "7 X.java",
                "6 y",
                "7 Y.java",
                "2 test",
                "3 java",
                "4 lab.inventory",
                "5 InventoryTest.java",
            ]
        );
        // The MCP view: the roles of the derived folders and the folders of a row.
        let state = t.state();
        let rows = state["rows"].as_array().unwrap();
        let at = |path: &str| {
            rows.iter()
                .find(|r| r["path"] == path)
                .unwrap_or_else(|| panic!("no row at {path}"))
        };
        assert_eq!(at("src/main/java")["role"], "sources");
        assert_eq!(at("src/test/java")["role"], "tests");
        assert!(at("src").get("role").is_none());
        assert!(at("docs/one").get("role").is_none());
        assert_eq!(at("src/main/java/lab/empty/a/b")["name"], "empty.a.b");
        assert_eq!(
            at("src/main/java/lab/empty/a/b")["folders"],
            serde_json::json!([
                "src/main/java/lab/empty",
                "src/main/java/lab/empty/a",
                "src/main/java/lab/empty/a/b"
            ])
        );
        assert_eq!(
            at("src/main/java/lab/inventory/util/deep")["name"],
            "util.deep"
        );
        // Icon and tint as for a declared folder.
        let java = t.rows().into_iter().find(|r| r.name == "java").unwrap();
        assert_eq!(
            t.folder_info(java.id).map(|i| i.role),
            Some(FolderRole::Sources)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v11 AK-28: the derived folders are kept when the tree is read again, follow the
    /// files when they are read again, and give way to a build file.
    #[test]
    fn derived_folders_follow_the_files() {
        let dir = plain_project("refresh");
        let mut t = tree(&dir);
        let derived = || idecraft_core::source_roots::derived(&dir);
        assert!(t.set_derived_roots(derived()), "rows changed");
        assert!(!t.set_derived_roots(derived()), "the same folders again");
        t.select_one(0);
        t.expand_all();
        let row_at = |t: &ProjectTree, path: &str| {
            t.state()["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["path"] == path)
                .cloned()
                .unwrap_or_else(|| panic!("no row at {path}"))
        };
        // Reading the tree again keeps them and counts.
        let generation = t.generation();
        let mut t = t.reloaded();
        assert_eq!(t.generation(), generation + 1);
        assert!(t.derived_roots_known());
        assert!(
            shown(&t).contains(&"5 empty.a.b".to_owned()),
            "{:?}",
            shown(&t)
        );
        // A Kotlin file elsewhere: its source folder appears when the folders are read again.
        std::fs::create_dir_all(dir.join("tools/gen/lab/gen")).unwrap();
        std::fs::write(
            dir.join("tools/gen/lab/gen/Main.kt"),
            "package lab.gen\n\nfun main() {}\n",
        )
        .unwrap();
        let mut t2 = t.reloaded();
        t2.select_one(0);
        t2.expand_all();
        assert!(
            row_at(&t2, "tools/gen").get("role").is_none(),
            "not read yet"
        );
        assert!(t2.set_derived_roots(derived()));
        assert_eq!(row_at(&t2, "tools/gen")["role"], "sources");
        assert_eq!(row_at(&t2, "tools/gen/lab/gen")["name"], "lab.gen");
        // A file in the middle of a row splits it at the next reading of the tree.
        std::fs::write(
            dir.join("src/main/java/lab/empty/a/A.java"),
            "package lab.empty.a;\n\nclass A {}\n",
        )
        .unwrap();
        t = t2.reloaded();
        let rows = shown(&t);
        assert!(rows.contains(&"5 empty.a".to_owned()), "{rows:?}");
        assert!(rows.contains(&"6 b".to_owned()), "{rows:?}");
        // A build file always wins: the declared roles replace the derived ones.
        std::fs::write(dir.join("pom.xml"), POM).unwrap();
        t.reload_roles(&dir);
        assert_eq!(row_at(&t, "src/main/java")["role"], "sources");
        assert_eq!(row_at(&t, "src/test/java")["role"], "tests");
        assert!(row_at(&t, "tools/gen").get("role").is_none());
        assert!(
            shown(&t)
                .iter()
                .any(|r| r.ends_with(" lab") && !r.contains("lab.gen"))
        );
        assert!(idecraft_core::source_roots::derived(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v12 AK-29 (SPEC-0048 v7 behaviour 10a): a Maven module in a sub-folder of a
    /// project without a build file of its own gets its folder roles once the window has read
    /// them in the background, and its packages are joined to one row.
    #[test]
    fn a_module_in_a_sub_folder_gets_its_roles() {
        let dir =
            std::env::temp_dir().join(format!("idecraft-nested-module-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in [".git", "app/src/test/java"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::create_dir_all(dir.join("app/src/main/java/lab/shop/core")).unwrap();
        std::fs::write(dir.join("app/pom.xml"), POM).unwrap();
        std::fs::write(
            dir.join("app/src/main/java/lab/shop/core/Core.java"),
            "package lab.shop.core;\n\nclass Core {}\n",
        )
        .unwrap();
        let dir = dir.canonicalize().unwrap();
        // On the UI thread nothing is walked: no roles yet.
        let mut t = tree(&dir);
        t.select_one(0);
        t.expand_all();
        assert!(shown(&t).contains(&"5 lab".to_owned()), "{:?}", shown(&t));
        assert!(t.set_derived_roots(idecraft_core::source_roots::derived(&dir)));
        t.select_one(0);
        t.expand_all();
        let role = |t: &ProjectTree, path: &str| {
            t.state()["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["path"] == path)
                .and_then(|r| r.get("role").cloned())
        };
        assert_eq!(role(&t, "app"), Some(serde_json::json!("module")));
        assert_eq!(
            role(&t, "app/src/main/java"),
            Some(serde_json::json!("sources"))
        );
        assert_eq!(
            role(&t, "app/src/test/java"),
            Some(serde_json::json!("tests"))
        );
        assert!(
            shown(&t).contains(&"5 lab.shop.core".to_owned()),
            "{:?}",
            shown(&t)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0275 v10 AK-26: the MCP view names the folders of every row; its actions take any folder of a row.
    #[test]
    fn the_state_names_the_folders_of_a_row() {
        let dir = project("state");
        let mut t = tree(&dir);
        t.act("expand", &dir.join("src/main/java/lab")).unwrap();
        t.act("select", &dir.join("src/main/java/lab/shop"))
            .unwrap();
        let state = t.state();
        assert_eq!(state["compact_middle_packages"], true);
        assert_eq!(state["view"], "project");
        let rows = state["rows"].as_array().unwrap();
        let find = |name: &str| rows.iter().find(|r| r["name"] == name).unwrap();
        let core = find("lab.shop.core");
        assert_eq!(core["path"], "src/main/java/lab/shop/core");
        assert_eq!(core["kind"], "folder");
        assert_eq!(core["expanded"], true);
        assert_eq!(core["selected"], true);
        assert_eq!(core["depth"], 4);
        assert_eq!(
            core["folders"],
            serde_json::json!([
                "src/main/java/lab",
                "src/main/java/lab/shop",
                "src/main/java/lab/shop/core"
            ])
        );
        assert_eq!(find("java")["role"], "sources");
        assert_eq!(
            find("java")["folders"],
            serde_json::json!(["src/main/java"])
        );
        assert_eq!(find("Foo.java")["kind"], "file");
        assert!(find("Foo.java").get("folders").is_none());
        assert_eq!(rows[0]["path"], ".");
        assert_eq!(find(LIBRARIES)["kind"], "libraries");
        assert!(find(LIBRARIES).get("path").is_none());
        // Collapse through the outermost folder; errors name the problem.
        t.act("collapse", &dir.join("src/main/java/lab")).unwrap();
        assert!(!row(&t, "lab.shop.core").expanded);
        assert!(
            t.act("expand", &dir.join("pom.xml"))
                .unwrap_err()
                .contains("is a file")
        );
        assert!(
            t.act("select", &dir.join("nowhere"))
                .unwrap_err()
                .contains("not in the project tree")
        );
        assert!(
            t.act("open", &dir.join("pom.xml"))
                .unwrap_err()
                .contains("unknown action")
        );
        t.set_compact(false);
        let state = t.state();
        assert_eq!(state["compact_middle_packages"], false);
        let rows = state["rows"].as_array().unwrap();
        let lab = rows.iter().find(|r| r["name"] == "lab").unwrap();
        assert_eq!(lab["folders"], serde_json::json!(["src/main/java/lab"]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
