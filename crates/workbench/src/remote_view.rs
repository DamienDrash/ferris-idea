//! The Remote Host tool window (`SPEC-0121`): the SSH hosts of the user's config, then the folders
//! and files of the chosen host; Enter opens a file for editing, the field takes a host or a path.
//! New File, New Folder, Rename and Delete change the folder (`SPEC-0253`); the list is a tree
//! whose folders open in place, typing jumps to a name, and names sort without regard to case
//! (`SPEC-0253` v3 behaviour 9); with a backend, `@`,
//! `!` and `>` list symbols, problems and run configurations of the host's project (`SPEC-0158`).
//! Add Host… and Remove Host change Ferris IDEA's own host list; the hosts of `~/.ssh/config` stay
//! read-only (`SPEC-0121` v5 behaviours 1b and 5).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use ferris_idea_remote::{EntryKind, HostInfo, HostSource, OwnHost, RemoteEntry};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, rgb,
};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// What the window asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteEvent {
    /// Hide the window.
    Hide,
    /// Connect to a host alias or `[user@]host[:port]`.
    Connect(String),
    /// Show this folder of the connected host (absolute, or starting with `~`).
    Browse(String),
    /// Open this file of the connected host.
    Open(String),
    /// Read the folder again.
    Refresh,
    /// Open an SSH terminal in the folder shown.
    Terminal,
    /// Leave the host and show the host list.
    Hosts,
    /// Start Ferris IDEA's backend on the host for the folder shown (`SPEC-0158`).
    StartBackend,
    /// Search the host's project through the backend.
    Search(String),
    /// Open this file of the host at a 0-based line (a search hit).
    OpenAt(String, usize),
    /// Create a file (false) or folder (true) at this path (`SPEC-0253`).
    Create(String, bool),
    /// Rename the first path to the second.
    Rename(String, String),
    /// Ask before deleting this path (the workbench counts what it holds).
    AskDelete(String),
    /// Delete this path; `true` with everything inside.
    Delete(String, bool),
    /// Put the focus into the field.
    FocusField,
    /// Find symbols of the host's project through the backend (`SPEC-0158` behaviour 7).
    Symbols(String),
    /// List the problems the backend found in the host file checked last (behaviour 8).
    Problems,
    /// List the run configurations of the host's project (behaviour 9).
    RunConfigs,
    /// Run this configuration of the host's project in an SSH terminal.
    RunConfig(String),
    /// Open this folder of this host as a remote project (`SPEC-0317` v3 behaviour 1).
    OpenProject(String, String),
    /// Stop opening the remote project (`SPEC-0317` behaviour 3).
    CancelOpen,
    /// Read this folder to show it open in place in the tree (`SPEC-0253` v3 behaviour 9).
    Expand(String),
    /// Add this host to Ferris IDEA's own host list (`SPEC-0121` v5 behaviour 1b).
    AddHost(OwnHost),
    /// Remove this alias from Ferris IDEA's own host list.
    RemoveHost(String),
}

/// Typed letters older than this start a new name to jump to (`SPEC-0253` v3 behaviour 9).
const TYPE_PAUSE: Duration = Duration::from_secs(1);

/// Indentation of one tree level in the row texts.
const INDENT: &str = "  ";

/// A change that waits for a name or a confirmation in the field (`SPEC-0253` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// A new file in this folder.
    NewFile(String),
    /// A new folder in this folder.
    NewDir(String),
    /// Renaming this path.
    Rename(String),
    /// Deleting this path, which holds this many entries.
    Delete(String, u64),
    /// Removing this alias from Ferris IDEA's own hosts (`SPEC-0121` v5 behaviour 5).
    RemoveHost(String),
}

/// The labels of the Add Host… form's fields, in order (`SPEC-0121` v5 behaviour 5).
pub const ADD_HOST_FIELDS: [&str; 5] = ["Alias", "Host name", "Port", "User", "Identity file"];

/// The Add Host… form: its fields, the one with the focus, and a refusal.
struct AddForm {
    fields: Vec<Entity<EditorView>>,
    at: usize,
    error: Option<String>,
    /// Sent to the workbench, waiting for its answer.
    sent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Host(HostInfo),
    Up,
    /// An entry of the folder shown or of a folder open in the tree: the entry, the folder it is
    /// in, its depth below the folder shown (`SPEC-0253` v3 behaviour 9).
    Entry(RemoteEntry, String, usize),
    /// A note in place of an open folder's entries ("reading…", empty): text and depth.
    Note(String, usize),
    /// A search hit: absolute path on the host, 0-based line, the line's text.
    Hit(String, usize, String),
    /// A run configuration of the host's project: name and command.
    Config(String, String),
    /// A recent remote project (Remote Development, `SPEC-0317` v3 behaviour 1).
    Recent(ferris_idea_remote::RecentRemote),
    /// Open the folder shown as a remote project.
    OpenHere(String),
}

/// The window.
pub struct RemoteView {
    hosts: Vec<HostInfo>,
    /// Connected host label and folder shown.
    place: Option<(String, String)>,
    entries: Vec<RemoteEntry>,
    rows: Vec<Row>,
    selected: usize,
    busy: Option<String>,
    error: Option<String>,
    field: Entity<EditorView>,
    focus: FocusHandle,
    /// The backend running for the folder shown: its name and the folder (`SPEC-0158`).
    backend: Option<(String, String)>,
    /// Search hits shown instead of the folder.
    hits: Option<Vec<(String, usize, String)>>,
    /// Run configurations shown instead of the folder.
    configs: Option<Vec<(String, String)>>,
    /// The field's text when the last query was sent to the backend (`?text`, `@name`, `!`, `>`).
    asked: Option<String>,
    /// The field's text that the hits or configurations shown answer: Enter with this text acts
    /// on the selected row instead of asking again (`SPEC-0158` v4 behaviour 10).
    listed_for: Option<String>,
    /// A change waiting for a name or a confirmation.
    pending: Option<Pending>,
    /// Entry to select once the folder is read again.
    select_after: Option<String>,
    /// Remote Development: choosing a host and a folder opens it as a remote project; the host
    /// list starts with the recent remote projects (`SPEC-0317` v3 behaviour 1).
    project_mode: bool,
    recent: Vec<ferris_idea_remote::RecentRemote>,
    /// Only choosing a folder to open as a remote project: no file actions, terminal or backend
    /// (the Welcome window's SSH section, `SPEC-0355` v3 behaviour 12e).
    chooser_only: bool,
    /// Folders open in place in the tree with their entries, `None` while they are read
    /// (`SPEC-0253` v3 behaviour 9).
    expanded: BTreeMap<String, Option<Vec<RemoteEntry>>>,
    /// Letters typed in the list to jump to a name, and when the last one came.
    typed: String,
    typed_at: Option<Instant>,
    /// The Add Host… form, while shown.
    adding: Option<AddForm>,
}

impl EventEmitter<RemoteEvent> for RemoteView {}

fn host_text(h: &HostInfo) -> String {
    let mut s = host_target(h);
    s.push_str("  · ");
    s.push_str(h.source.label());
    s
}

fn host_target(h: &HostInfo) -> String {
    let mut s = h.alias.clone();
    if let Some(name) = &h.hostname {
        s.push_str("  ");
        if let Some(user) = &h.user {
            s.push_str(&format!("{user}@"));
        }
        s.push_str(name);
        if let Some(port) = h.port.filter(|p| *p != 22) {
            s.push_str(&format!(":{port}"));
        }
    }
    s
}

fn row_text(r: &Row) -> String {
    match r {
        Row::Entry(_, _, depth) | Row::Note(_, depth) if *depth > 0 => {
            format!("{}{}", INDENT.repeat(*depth), row_label(r))
        }
        _ => row_label(r),
    }
}

/// Depth of a row in the tree (0 for everything but open folders' entries).
fn row_depth(r: &Row) -> usize {
    match r {
        Row::Entry(_, _, d) | Row::Note(_, d) => *d,
        _ => 0,
    }
}

/// A row's text without its indentation.
fn row_label(r: &Row) -> String {
    match r {
        Row::Host(h) => host_text(h),
        Row::Up => "..".into(),
        Row::Entry(e, _, _) => format!(
            "{}{}{}",
            e.name,
            if e.kind == EntryKind::Dir { "/" } else { "" },
            if e.link { "  →" } else { "" }
        ),
        Row::Note(text, _) => text.clone(),
        Row::Hit(path, line, text) => format!(
            "{}:{}  {}",
            path.rsplit('/').next().unwrap_or(path),
            line + 1,
            text.trim()
        ),
        Row::Config(name, command) => format!("▶ {name}  {command}"),
        Row::Recent(r) => format!("Recent: {}  {}", r.label(), r.folder),
        Row::OpenHere(folder) => format!("Open {folder} as a remote project"),
    }
}

/// The question or prompt of a pending change.
pub fn pending_prompt(p: &Pending) -> String {
    let base = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
    match p {
        Pending::NewFile(dir) => {
            format!("New file in {dir}: type a name, Enter creates, Escape cancels")
        }
        Pending::NewDir(dir) => {
            format!("New folder in {dir}: type a name, Enter creates, Escape cancels")
        }
        Pending::Rename(path) => {
            format!(
                "Rename {}: type the new name or path, Enter renames, Escape cancels",
                base(path)
            )
        }
        Pending::Delete(path, 0) => {
            format!("Delete {}? Enter deletes, Escape keeps it", base(path))
        }
        Pending::Delete(path, n) => format!(
            "Delete {}/ and its {n} {}? Enter deletes, Escape keeps it",
            base(path),
            if *n == 1 { "entry" } else { "entries" }
        ),
        Pending::RemoveHost(alias) => {
            format!("Remove {alias} from Ferris IDEA's hosts? Enter removes, Escape keeps it")
        }
    }
}

impl RemoteView {
    /// The host list, empty until read.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            hosts: Vec::new(),
            place: None,
            entries: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            busy: None,
            error: None,
            field: cx.new(EditorView::field),
            focus: cx.focus_handle(),
            backend: None,
            hits: None,
            configs: None,
            asked: None,
            listed_for: None,
            pending: None,
            select_after: None,
            project_mode: false,
            recent: Vec::new(),
            chooser_only: false,
            expanded: BTreeMap::new(),
            typed: String::new(),
            typed_at: None,
            adding: None,
        }
    }

    /// Shows the Add Host… form with empty fields, the focus in the first (`SPEC-0121` v5
    /// behaviour 5).
    pub fn open_add_host(&mut self, cx: &mut Context<Self>) {
        if self.place.is_some() || self.chooser_only {
            return;
        }
        let fields = ADD_HOST_FIELDS
            .iter()
            .map(|_| cx.new(EditorView::field))
            .collect();
        self.adding = Some(AddForm {
            fields,
            at: 0,
            error: None,
            sent: false,
        });
        self.pending = None;
        self.error = None;
        cx.emit(RemoteEvent::FocusField);
        cx.notify();
    }

    /// Whether the Add Host… form is shown.
    pub fn adding_host(&self) -> bool {
        self.adding.is_some()
    }

    /// The refusal the Add Host… form shows, if any.
    pub fn add_host_error(&self) -> Option<&str> {
        self.adding.as_ref().and_then(|f| f.error.as_deref())
    }

    /// The texts of the Add Host… form's fields (for tests).
    pub fn add_host_values(&self, cx: &gpui::App) -> Vec<String> {
        self.adding
            .as_ref()
            .map(|f| {
                f.fields
                    .iter()
                    .map(|e| e.read(cx).state.buffer.text())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The answer to [`RemoteEvent::AddHost`]: `Ok` closes the form, a refusal stays in it with
    /// the values typed.
    pub fn host_added(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.adding = None;
                cx.emit(RemoteEvent::FocusField);
            }
            Err(e) => {
                if let Some(f) = &mut self.adding {
                    f.error = Some(e);
                    f.sent = false;
                }
            }
        }
        cx.notify();
    }

    /// The host of the selected row, when the host list is shown.
    fn selected_host(&self) -> Option<&HostInfo> {
        match (self.rows.get(self.selected), &self.place) {
            (Some(Row::Host(h)), None) => Some(h),
            _ => None,
        }
    }

    /// Remove Host on the selected row: asks for one of Ferris IDEA's hosts; says that a host of
    /// `~/.ssh/config` stays (`SPEC-0121` v5 behaviour 5).
    pub fn ask_remove_host(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(h) = self.selected_host().cloned() else {
            return false;
        };
        match h.source {
            HostSource::FerrisIdea => {
                self.pending = Some(Pending::RemoveHost(h.alias));
                self.error = None;
            }
            HostSource::SshConfig => {
                self.error = Some(format!(
                    "{} comes from ~/.ssh/config, which Ferris IDEA never changes; edit that file yourself",
                    h.alias
                ))
            }
        }
        cx.notify();
        true
    }

    /// Handles a key of the Add Host… form: Tab and ⇧Tab move between the fields, Enter adds,
    /// Escape leaves the form.
    fn press_form(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(form) = &mut self.adding else {
            return false;
        };
        let n = form.fields.len();
        match key {
            "tab" | "down" => form.at = (form.at + 1) % n,
            "shift-tab" | "up" => form.at = (form.at + n - 1) % n,
            "escape" => {
                self.adding = None;
            }
            "enter" => {
                if form.sent {
                    return true;
                }
                let v: Vec<String> = form
                    .fields
                    .iter()
                    .map(|e| e.read(cx).state.buffer.text().trim().to_owned())
                    .collect();
                let port = match v[2].as_str() {
                    "" => None,
                    p => match p.parse::<u16>() {
                        Ok(p) if p > 0 => Some(p),
                        _ => {
                            form.error = Some("Port: a number from 1 to 65535".into());
                            form.at = 2;
                            cx.emit(RemoteEvent::FocusField);
                            cx.notify();
                            return true;
                        }
                    },
                };
                let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
                form.sent = true;
                form.error = None;
                cx.emit(RemoteEvent::AddHost(OwnHost {
                    alias: v[0].clone(),
                    hostname: v[1].clone(),
                    port,
                    user: some(&v[3]),
                    identity_file: some(&v[4]),
                }));
            }
            _ => return false,
        }
        cx.emit(RemoteEvent::FocusField);
        cx.notify();
        true
    }

    /// Remote Development as a folder chooser only (`SPEC-0355` v3 behaviour 12e): choosing a
    /// folder opens it as a remote project; files, the terminal and the backend are left out.
    pub fn set_chooser_only(&mut self, cx: &mut Context<Self>) {
        self.chooser_only = true;
        self.set_project_mode(true, Vec::new(), cx);
    }

    /// Whether the window only chooses a folder.
    pub fn chooser_only(&self) -> bool {
        self.chooser_only
    }

    /// Remote Development (`SPEC-0317` v3 behaviour 1) with `recent` remote projects first, or
    /// the plain Remote Host window.
    pub fn set_project_mode(
        &mut self,
        on: bool,
        recent: Vec<ferris_idea_remote::RecentRemote>,
        cx: &mut Context<Self>,
    ) {
        self.project_mode = on;
        self.recent = if on { recent } else { Vec::new() };
        self.selected = 0;
        self.rebuild();
        cx.notify();
    }

    /// Whether choosing a folder opens it as a remote project.
    pub fn project_mode(&self) -> bool {
        self.project_mode
    }

    /// The change waiting for a name or a confirmation.
    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    /// Asks before deleting `path`, which holds `entries` entries (`SPEC-0253` behaviour 3).
    pub fn ask_delete(&mut self, path: String, entries: u64, cx: &mut Context<Self>) {
        self.pending = Some(Pending::Delete(path, entries));
        self.busy = None;
        self.error = None;
        cx.notify();
    }

    /// Selects entry `name` once the folder is read again.
    pub fn select_after_refresh(&mut self, name: String) {
        self.select_after = Some(name);
    }

    /// Shows run configurations of the host's project (`SPEC-0158` behaviour 9).
    pub fn show_configs(&mut self, configs: Vec<(String, String)>, cx: &mut Context<Self>) {
        self.configs = Some(configs);
        self.listed_for = self.asked.take();
        self.hits = None;
        self.busy = None;
        self.error = None;
        self.selected = 0;
        self.rebuild();
        cx.notify();
    }

    /// The backend runs for `folder` (`SPEC-0158` behaviour 4); `None` stops showing it.
    pub fn set_backend(&mut self, backend: Option<(String, String)>, cx: &mut Context<Self>) {
        self.backend = backend;
        cx.notify();
    }

    /// The running backend's name and folder.
    pub fn backend(&self) -> Option<&(String, String)> {
        self.backend.as_ref()
    }

    /// Shows search hits (absolute host paths, 0-based lines) instead of the folder.
    pub fn show_hits(&mut self, hits: Vec<(String, usize, String)>, cx: &mut Context<Self>) {
        self.hits = Some(hits);
        self.listed_for = self.asked.take();
        self.configs = None;
        self.busy = None;
        self.error = None;
        self.selected = 0;
        self.rebuild();
        cx.notify();
    }

    fn rebuild(&mut self) {
        if let Some(configs) = &self.configs {
            self.rows = configs
                .iter()
                .map(|(n, c)| Row::Config(n.clone(), c.clone()))
                .collect();
            self.selected = self.selected.min(self.rows.len().saturating_sub(1));
            return;
        }
        if let Some(hits) = &self.hits {
            self.rows = hits
                .iter()
                .map(|(p, l, t)| Row::Hit(p.clone(), *l, t.clone()))
                .collect();
            self.selected = self.selected.min(self.rows.len().saturating_sub(1));
            return;
        }
        self.rows = match &self.place {
            None => self
                .recent
                .iter()
                .cloned()
                .map(Row::Recent)
                .chain(self.hosts.iter().cloned().map(Row::Host))
                .collect(),
            Some((_, path)) => {
                let mut rows = Vec::new();
                if self.project_mode {
                    rows.push(Row::OpenHere(path.clone()));
                }
                if path != "/" {
                    rows.push(Row::Up);
                }
                tree_rows(&mut rows, path, &self.entries, 0, &self.expanded);
                rows
            }
        };
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    fn set_field(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Shows the configured hosts (and leaves a connected host).
    /// The selected host stays selected when the list is read again (`SPEC-0121` v7 behaviour
    /// 5a), as long as it is still there.
    pub fn show_hosts(&mut self, hosts: Vec<HostInfo>, cx: &mut Context<Self>) {
        let was = match self.rows.get(self.selected) {
            Some(Row::Host(h)) if self.place.is_none() => Some(h.alias.clone()),
            _ => None,
        };
        self.hosts = hosts;
        if self.place.is_none() {
            self.rebuild();
            if let Some(i) = was.and_then(|alias| {
                self.rows
                    .iter()
                    .position(|r| matches!(r, Row::Host(h) if h.alias == alias))
            }) {
                self.selected = i;
            }
        }
        cx.notify();
    }

    /// Back to the host list.
    pub fn leave(&mut self, cx: &mut Context<Self>) {
        self.place = None;
        self.backend = None;
        self.hits = None;
        self.configs = None;
        self.asked = None;
        self.listed_for = None;
        self.pending = None;
        self.entries.clear();
        self.expanded.clear();
        self.selected = 0;
        self.error = None;
        self.busy = None;
        self.set_field("", cx);
        self.rebuild();
        cx.notify();
    }

    /// Shows folder `path` of `host` with its entries.
    pub fn show_dir(
        &mut self,
        host: String,
        path: String,
        entries: Vec<RemoteEntry>,
        cx: &mut Context<Self>,
    ) {
        let same = self.place.as_ref().is_some_and(|(_, p)| *p == path);
        let mut entries = entries;
        ferris_idea_remote::sort_entries(&mut entries);
        if same {
            // F5: every folder open in the tree is read again too (behaviour 9).
            for folder in self.expanded.keys() {
                cx.emit(RemoteEvent::Expand(folder.clone()));
            }
        } else {
            self.expanded.clear();
        }
        self.set_field(&path, cx);
        self.place = Some((host, path));
        self.entries = entries;
        self.busy = None;
        self.error = None;
        if !same {
            self.selected = 0;
        }
        self.rebuild();
        if let Some(name) = self.select_after.take()
            && let Some(i) = entry_row(&self.rows, &name)
        {
            self.selected = i;
        }
        cx.notify();
    }

    /// Shows the entries of `folder`, open in place in the tree (`SPEC-0253` v3 behaviour 9);
    /// ignored when the folder was closed meanwhile.
    pub fn show_children(
        &mut self,
        folder: String,
        mut entries: Vec<RemoteEntry>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.expanded.get_mut(&folder) else {
            return;
        };
        ferris_idea_remote::sort_entries(&mut entries);
        *slot = Some(entries);
        let at = self.rows.get(self.selected).cloned();
        self.rebuild();
        if let Some(i) = at.and_then(|r| self.rows.iter().position(|x| *x == r)) {
            self.selected = i;
        }
        cx.notify();
    }

    /// `folder` could not be read: it closes again and the error is shown.
    pub fn show_children_error(&mut self, folder: String, error: String, cx: &mut Context<Self>) {
        self.expanded.remove(&folder);
        self.rebuild();
        self.show_error(error, cx);
    }

    /// Folders open in place in the tree (for tests).
    pub fn expanded(&self) -> Vec<String> {
        self.expanded.keys().cloned().collect()
    }

    /// The letters typed to jump to a name (`SPEC-0253` v3 behaviour 9).
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// One typed character in the list (for tests too): selects the first row whose name starts
    /// with the letters typed within [`TYPE_PAUSE`], without regard to case, else the first that
    /// contains them; the selection stays when none matches.
    pub fn type_char(&mut self, ch: &str, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self
            .typed_at
            .is_none_or(|at| now.duration_since(at) > TYPE_PAUSE)
        {
            self.typed.clear();
        }
        self.typed.push_str(ch);
        self.typed_at = Some(now);
        let names: Vec<Option<String>> = self.rows.iter().map(row_name).collect();
        if let Some(i) = jump_row(&names, &self.typed) {
            self.selected = i;
        }
        cx.notify();
    }

    fn forget_typed(&mut self) {
        self.typed.clear();
        self.typed_at = None;
    }

    /// Opens the selected folder in place (→), or closes it (←) — on an entry inside an open
    /// folder ← selects that folder. False when the key does nothing there.
    fn toggle_selected(&mut self, open: bool, cx: &mut Context<Self>) -> bool {
        let Some(Row::Entry(e, folder, depth)) = self.rows.get(self.selected).cloned() else {
            return false;
        };
        let path = ferris_idea_remote::join(&folder, &e.name);
        let is_open = self.expanded.contains_key(&path);
        match (open, e.kind == EntryKind::Dir, is_open) {
            (true, true, false) => {
                self.expanded.insert(path.clone(), None);
                cx.emit(RemoteEvent::Expand(path));
            }
            (true, true, true) => {
                // Already open: → goes to its first entry.
                if matches!(self.rows.get(self.selected + 1), Some(r) if row_depth(r) > depth) {
                    self.selected += 1;
                }
                return true;
            }
            (false, true, true) => {
                let inside = format!("{}/", path.trim_end_matches('/'));
                self.expanded
                    .retain(|k, _| *k != path && !k.starts_with(&inside));
            }
            (false, _, _) if depth > 0 => {
                // Select the open folder this entry is in.
                if let Some(i) = self.rows[..self.selected]
                    .iter()
                    .rposition(|r| row_depth(r) < depth && matches!(r, Row::Entry(..)))
                {
                    self.selected = i;
                }
                return true;
            }
            _ => return false,
        }
        self.rebuild();
        true
    }

    /// Shows that something is going on.
    pub fn show_busy(&mut self, what: String, cx: &mut Context<Self>) {
        self.busy = Some(what);
        cx.notify();
    }

    /// Clears the busy note.
    pub fn clear_busy(&mut self, cx: &mut Context<Self>) {
        self.busy = None;
        cx.notify();
    }

    /// Shows why the last request failed.
    pub fn show_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.asked = None;
        self.busy = None;
        self.error = Some(error);
        cx.notify();
    }

    /// Focus handle of the window.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Focus handle of the host/path field, or of the Add Host… form's current field.
    pub fn field_focus(&self, cx: &gpui::App) -> FocusHandle {
        match &self.adding {
            Some(f) => f.fields[f.at].read(cx).focus(),
            None => self.field.read(cx).focus(),
        }
    }

    /// Connected host and folder shown.
    pub fn place(&self) -> Option<&(String, String)> {
        self.place.as_ref()
    }

    /// The error shown, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether a request is going on.
    pub fn busy(&self) -> bool {
        self.busy.is_some()
    }

    /// The rows as text (for tests).
    pub fn texts(&self) -> Vec<String> {
        self.rows.iter().map(row_text).collect()
    }

    /// Text of the host/path field.
    pub fn field_text(&self, cx: &gpui::App) -> String {
        self.field.read(cx).state.buffer.text()
    }

    /// Puts `text` into the host/path field (for tests too).
    pub fn type_field(&mut self, text: &str, cx: &mut Context<Self>) {
        self.set_field(text, cx);
    }

    /// Selects the row whose text is `text`, else the first one starting with `text` (for
    /// tests). Returns false and keeps the selection when no row matches.
    pub fn select_text(&mut self, text: &str) -> bool {
        match row_matching(&self.texts(), text) {
            Some(i) => {
                self.selected = i;
                true
            }
            None => false,
        }
    }

    /// Text of the selected row (for tests).
    pub fn selected_text(&self) -> Option<String> {
        self.rows.get(self.selected).map(row_text)
    }

    /// Whether the rows shown are the hits or configurations that the field's text `typed`
    /// listed (or the field is empty), so Enter acts on them (`SPEC-0158` v4 behaviour 10).
    fn on_listed(&self, typed: &str) -> bool {
        (self.hits.is_some() || self.configs.is_some())
            && (typed.is_empty() || self.listed_for.as_deref() == Some(typed))
    }

    /// Opens the selected hit at its line or runs the selected configuration; false when the
    /// selected row is neither.
    fn act_on_listed(&mut self, cx: &mut Context<Self>) -> bool {
        match self.rows.get(self.selected).cloned() {
            Some(Row::Hit(path, line, _)) => cx.emit(RemoteEvent::OpenAt(path, line)),
            Some(Row::Config(name, _)) => cx.emit(RemoteEvent::RunConfig(name)),
            _ => return false,
        }
        true
    }

    fn path_from_field(&self, text: &str) -> String {
        let text = text.trim();
        match &self.place {
            Some((_, dir)) if !text.starts_with('/') && !text.starts_with('~') => {
                ferris_idea_remote::join(dir, text)
            }
            _ => text.to_owned(),
        }
    }

    /// Handles one key as the window does (for tests too): Enter connects, opens a folder or a
    /// file — or goes to the host or path typed in the field; Alt+↑ goes up; F5 reads again; Escape hides.
    pub fn press(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if self.adding.is_some() {
            return self.press_form(key, cx);
        }
        let n = self.rows.len();
        let typed = self.field_text(cx).trim().to_owned();
        if let Some(pending) = self.pending.clone() {
            match key {
                "enter" => {
                    let current = self
                        .place
                        .as_ref()
                        .map(|(_, p)| p.clone())
                        .unwrap_or_default();
                    match pending {
                        Pending::NewFile(_) | Pending::NewDir(_) | Pending::Rename(_)
                            if typed.is_empty() || typed == current =>
                        {
                            return true;
                        }
                        Pending::NewFile(_) => {
                            cx.emit(RemoteEvent::Create(self.path_from_field(&typed), false))
                        }
                        Pending::NewDir(_) => {
                            cx.emit(RemoteEvent::Create(self.path_from_field(&typed), true))
                        }
                        Pending::Rename(from) => {
                            cx.emit(RemoteEvent::Rename(from, self.path_from_field(&typed)))
                        }
                        Pending::Delete(path, entries) => {
                            cx.emit(RemoteEvent::Delete(path, entries > 0))
                        }
                        Pending::RemoveHost(alias) => cx.emit(RemoteEvent::RemoveHost(alias)),
                    }
                    self.pending = None;
                    self.set_field(&current, cx);
                }
                "escape" => {
                    self.pending = None;
                    let current = self
                        .place
                        .as_ref()
                        .map(|(_, p)| p.clone())
                        .unwrap_or_default();
                    self.set_field(&current, cx);
                }
                _ => return false,
            }
            cx.notify();
            return true;
        }
        // Any key but a typed letter ends the name being typed (behaviour 9).
        let typing = !self.typed.is_empty();
        self.forget_typed();
        if key == "escape" && typing {
            cx.notify();
            return true;
        }
        let selected_entry = match self.rows.get(self.selected) {
            Some(Row::Entry(e, folder, _)) if self.place.is_some() => {
                Some((ferris_idea_remote::join(folder, &e.name), e.name.clone()))
            }
            _ => None,
        };
        if self.chooser_only
            && matches!(
                key,
                "new-file" | "new-dir" | "rename" | "delete" | "add-host"
            )
        {
            return false;
        }
        match key {
            "add-host" => {
                if self.place.is_some() {
                    return false;
                }
                self.open_add_host(cx);
            }
            "delete" if self.place.is_none() => return self.ask_remove_host(cx),
            "new-file" | "new-dir" => {
                let Some((_, dir)) = &self.place else {
                    return false;
                };
                self.pending = Some(if key == "new-file" {
                    Pending::NewFile(dir.clone())
                } else {
                    Pending::NewDir(dir.clone())
                });
                self.hits = None;
                self.configs = None;
                self.rebuild();
                self.set_field("", cx);
                cx.emit(RemoteEvent::FocusField);
            }
            "rename" => {
                let Some((path, name)) = selected_entry else {
                    return false;
                };
                self.pending = Some(Pending::Rename(path));
                self.set_field(&name, cx);
                cx.emit(RemoteEvent::FocusField);
            }
            "delete" => {
                let Some((path, _)) = selected_entry else {
                    return false;
                };
                cx.emit(RemoteEvent::AskDelete(path));
            }
            // The rows a query listed: Enter acts on the selected one while the field still holds
            // that query, or nothing (SPEC-0158 v4 behaviour 10).
            "enter" if self.on_listed(&typed) => {
                if !self.act_on_listed(cx) {
                    return false;
                }
            }
            "enter" if typed.starts_with('@') && self.backend.is_some() => {
                let query = typed[1..].trim();
                if query.is_empty() {
                    return true;
                }
                let query = query.to_owned();
                self.asked = Some(typed);
                cx.emit(RemoteEvent::Symbols(query));
            }
            "enter" if typed == "!" && self.backend.is_some() => {
                self.asked = Some(typed);
                cx.emit(RemoteEvent::Problems);
            }
            "enter" if typed == ">" && self.backend.is_some() => {
                self.asked = Some(typed);
                cx.emit(RemoteEvent::RunConfigs);
            }
            "down" if n > 0 => self.selected = (self.selected + 1).min(n - 1),
            "up" => self.selected = self.selected.saturating_sub(1),
            "right" | "left" => {
                if !self.toggle_selected(key == "right", cx) {
                    return false;
                }
            }
            "enter" => {
                let current = self.place.as_ref().map(|(_, p)| p.clone());
                // `?text` searches the host's project through the backend (SPEC-0158).
                if let Some(query) = typed.strip_prefix('?').map(str::trim)
                    && !query.is_empty()
                    && self.backend.is_some()
                {
                    let query = query.to_owned();
                    self.asked = Some(typed.clone());
                    cx.emit(RemoteEvent::Search(query));
                } else if let Some(Row::Hit(path, line, _)) = self
                    .rows
                    .get(self.selected)
                    .cloned()
                    .filter(|_| typed.is_empty() || typed.starts_with(['?', '@', '!']))
                {
                    cx.emit(RemoteEvent::OpenAt(path, line));
                } else if !typed.is_empty() && Some(&typed) != current.as_ref() {
                    match &self.place {
                        None => match other_host(&typed) {
                            Some(Ok((host, folder))) if self.project_mode => {
                                cx.emit(RemoteEvent::OpenProject(host, folder))
                            }
                            Some(Ok(_)) => self.show_error(
                                "Containers, WSL and paired hosts open from Remote Development"
                                    .into(),
                                cx,
                            ),
                            Some(Err(e)) => self.show_error(e, cx),
                            None => cx.emit(RemoteEvent::Connect(typed)),
                        },
                        Some(_) => cx.emit(RemoteEvent::Browse(self.path_from_field(&typed))),
                    }
                } else {
                    match (self.rows.get(self.selected).cloned(), current) {
                        (Some(Row::Host(h)), _) => cx.emit(RemoteEvent::Connect(h.alias)),
                        (Some(Row::Recent(r)), _) => {
                            cx.emit(RemoteEvent::OpenProject(r.host, r.folder))
                        }
                        (Some(Row::OpenHere(folder)), _) => {
                            let host = self.place.as_ref().map(|(h, _)| h.clone());
                            if let Some(host) = host {
                                cx.emit(RemoteEvent::OpenProject(host, folder));
                            }
                        }
                        (Some(Row::Up), Some(dir)) => {
                            cx.emit(RemoteEvent::Browse(ferris_idea_remote::parent(&dir)))
                        }
                        (Some(Row::Entry(e, folder, _)), Some(_)) => {
                            let path = ferris_idea_remote::join(&folder, &e.name);
                            if e.kind == EntryKind::Dir {
                                cx.emit(RemoteEvent::Browse(path));
                            } else if self.chooser_only {
                                self.show_error(
                                    format!(
                                        "{} is a file: choose a folder to open as a remote project",
                                        e.name
                                    ),
                                    cx,
                                );
                            } else {
                                cx.emit(RemoteEvent::Open(path));
                            }
                        }
                        _ => return false,
                    }
                }
            }
            "up-dir" => match &self.place {
                Some((_, dir)) if dir != "/" => {
                    cx.emit(RemoteEvent::Browse(ferris_idea_remote::parent(dir)))
                }
                _ => return false,
            },
            "f5" => cx.emit(RemoteEvent::Refresh),
            // Escape while a remote project opens stops it (`SPEC-0317` behaviour 3).
            "escape" if self.project_mode && self.busy.is_some() => {
                cx.emit(RemoteEvent::CancelOpen)
            }
            "escape" if self.hits.is_some() || self.configs.is_some() => {
                self.hits = None;
                self.configs = None;
                self.listed_for = None;
                self.selected = 0;
                self.rebuild();
            }
            "escape" => cx.emit(RemoteEvent::Hide),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Taken before the field sees the key (`SPEC-0253` v2 behaviour 8): in the field only Enter,
    /// Escape, ↑ and ↓ act on the window — a typed host, path or name goes with Enter instead of
    /// becoming a line break — and every other key edits the field (⌘⌫ deletes text there, never
    /// a file).
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        // The Add Host… form: Tab, ⇧Tab, ↑, ↓, Enter and Escape act on the form, every other key
        // edits its field (`SPEC-0121` v5 behaviour 5).
        if self.adding.is_some() {
            let key = match k.key.as_str() {
                "tab" if m.shift && !(m.platform || m.control || m.alt) => "shift-tab",
                "tab" | "enter" | "escape" | "up" | "down"
                    if !(m.platform || m.control || m.alt || m.shift) =>
                {
                    k.key.as_str()
                }
                _ => return,
            };
            let key = key.to_owned();
            if self.press_form(&key, cx) {
                cx.stop_propagation();
            }
            return;
        }
        let in_field = self.field.read(cx).focus().is_focused(window);
        let plain = !(m.platform || m.control || m.alt || m.shift);
        if in_field && !(plain && matches!(k.key.as_str(), "enter" | "escape" | "up" | "down")) {
            return;
        }
        // Typing in the list jumps to a name (`SPEC-0253` v3 behaviour 9).
        if !in_field
            && !(m.platform || m.control || m.alt)
            && self.pending.is_none()
            && let Some(ch) = k.key_char.as_deref().filter(|c| typed_char(c))
        {
            self.type_char(ch, cx);
            cx.stop_propagation();
            return;
        }
        let Some(key) = command(&k.key, m, cfg!(target_os = "macos")) else {
            return;
        };
        let key = key.to_owned();
        if matches!(
            key.as_str(),
            "up" | "down"
                | "left"
                | "right"
                | "enter"
                | "f5"
                | "escape"
                | "up-dir"
                | "new-file"
                | "new-dir"
                | "rename"
                | "delete"
        ) && self.press(&key, cx)
        {
            cx.stop_propagation();
        }
    }
}

impl Render for RemoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let rows = if self.adding.is_some() {
            Vec::new()
        } else {
            self.rows.clone()
        };
        let connected = self.place.clone();
        let field_focused = self.field.read(cx).focus().is_focused(window);
        let empty_field = self.field_text(cx).is_empty();
        let placeholder = if connected.is_some() {
            "Path on the host (absolute, ~ or relative), Enter goes there"
        } else {
            "Host alias or user@host[:port], Enter connects"
        };
        let help = keys(if self.chooser_only && connected.is_some() {
            "Go to the project folder, then Enter on its Open row · Escape goes back"
        } else if self.project_mode && connected.is_some() {
            "Go to the project folder, then Enter on its Open row · Escape stops opening"
        } else if self.project_mode {
            "Recent remote projects, Ferris IDEA's hosts and ~/.ssh/config · Enter opens or connects"
        } else if connected.is_some() && self.backend.is_some() {
            "Enter opens · ⌘N file · ⇧⌘N folder · ⇧F6 rename · ⌘⌫ delete · ?text @symbol ! problems > run"
        } else if connected.is_some() {
            "Enter opens · ⌥↑ up · F5 reads again · ⌘N file · ⇧⌘N folder · ⇧F6 rename · ⌘⌫ delete"
        } else {
            "Ferris IDEA's hosts and ~/.ssh/config · Enter connects with your SSH keys and agent · ⌘⌫ removes one of Ferris IDEA's"
        });
        // Each button and row is annotated with its id, so window tests click it with real mouse
        // events (the semantic tree; nothing unless a consumer is active).
        let button = |id: &'static str, label: &'static str, effects: &'static [Effect]| {
            div()
                .id(id)
                .relative()
                .flex_none()
                .whitespace_nowrap()
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tab_hover)))
                .child(label)
                .semantics(
                    Semantics::new(Role::Button)
                        .name(label)
                        .test_id(id)
                        .action(AxAction::Click)
                        .effects(effects),
                )
        };
        let form = self.adding.as_ref().map(|f| {
            let hints = [
                "a name to connect with, e.g. test-vm",
                "name or address, e.g. 192.168.64.5",
                "optional, 22",
                "optional, your login name",
                "optional, e.g. ~/.ssh/id_ed25519 (else your keys and agent)",
            ];
            div()
                .id("remote-add-host-form")
                .flex()
                .flex_col()
                .gap_1()
                .px_2()
                .py_1()
                // The buttons and a refusal first, so a short tool window still shows them.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(button("remote-add-host-ok", "Add", &[Effect::Ssh]).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.press_form("enter", cx);
                            },
                        )))
                        .child(button("remote-add-host-cancel", "Cancel", &[Effect::None]).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.press_form("escape", cx);
                            }),
                        ))
                        .child(div().text_color(c(p.ui_muted)).child(keys(
                            "Add a host to Ferris IDEA's list · Tab next field · Enter adds · Escape cancels",
                        ))),
                )
                .when_some(f.error.clone(), |d, e| {
                    d.child(div().text_color(rgb(p.console_ansi_text[1])).child(e))
                })
                .children(f.fields.iter().enumerate().map(|(i, field)| {
                    let focused = field.read(cx).focus().is_focused(window);
                    let value = field.read(cx).state.buffer.text();
                    let empty = value.is_empty();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(px(96.))
                                .flex_none()
                                .text_color(c(p.ui_muted))
                                .child(ADD_HOST_FIELDS[i]),
                        )
                        .child(
                            div()
                                .id(("remote-add-host-field", i))
                                .flex_1()
                                .min_w_0()
                                .h(px(24.))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(if focused { c(p.focus) } else { c(p.separator) })
                                .relative()
                                .child(div().flex_1().min_w_0().h(px(18.)).child(field.clone()))
                                .when(empty, |d| {
                                    d.child(
                                        div()
                                            .absolute()
                                            .left(px(8.))
                                            .text_color(c(p.ui_dim))
                                            .child(hints[i]),
                                    )
                                })
                                // Typing edits the form; Enter in it adds the host as Add
                                // does (`ssh`, SPEC-0319 v7 behaviour 56).
                                .semantics(
                                    Semantics::new(Role::TextInput)
                                        .name(ADD_HOST_FIELDS[i])
                                        .value(value)
                                        .focused(focused)
                                        .test_id(format!("remote-add-host-field-{i}"))
                                        .action(AxAction::Focus)
                                        .effects(&[Effect::Ssh])
                                        .handles_keys(&[
                                            "tab",
                                            "shift-tab",
                                            "enter",
                                            "escape",
                                            "up",
                                            "down",
                                        ]),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(f) = &mut this.adding {
                                        f.at = i;
                                    }
                                    cx.emit(RemoteEvent::FocusField);
                                })),
                        )
                }))
        });
        let status = match (&self.busy, &self.error, &connected) {
            _ if self.adding.is_some() => None,
            _ if self.pending.is_some() => {
                self.pending.as_ref().map(|p| (pending_prompt(p), false))
            }
            _ if !self.typed.is_empty() => Some((format!("Jump to: {}", self.typed), false)),
            (Some(b), _, _) => Some((b.clone(), false)),
            (None, Some(e), _) => Some((e.clone(), true)),
            (None, None, None) if rows.is_empty() => Some((
                "No hosts yet: Add Host…, or type user@host into the field".into(),
                false,
            )),
            (None, None, Some(_)) if rows.len() <= 1 => {
                Some(("This folder is empty".into(), false))
            }
            _ => None,
        };
        let panel_focused = self.focus.is_focused(window);
        div()
            .id("remote-panel")
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_key))
            // Every key in the window may reach the host: Enter connects, opens or goes to a
            // path, ⌘N, ⇧⌘N, ⇧F6 and ⌘⌫ make, rename and delete over SSH, ⌘⌫ removes one of
            // Ferris IDEA's hosts (SPEC-0319 v7 behaviour 56).
            .relative()
            .semantics(
                Semantics::new(Role::Group)
                    .name(if self.project_mode {
                        "Remote Development"
                    } else {
                        "Remote Host"
                    })
                    .test_id("remote-panel")
                    .focused(panel_focused)
                    .effects(&[Effect::Ssh])
                    .handles_keys(if cfg!(target_os = "macos") {
                        &[
                            "enter",
                            "escape",
                            "tab",
                            "f5",
                            "alt-up",
                            "cmd-n",
                            "cmd-shift-n",
                            "shift-f6",
                            "cmd-backspace",
                        ]
                    } else {
                        &[
                            "enter",
                            "escape",
                            "tab",
                            "f5",
                            "alt-up",
                            "ctrl-n",
                            "ctrl-shift-n",
                            "shift-f6",
                            "ctrl-backspace",
                        ]
                    }),
            )
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title(if self.project_mode {
                        "Remote Development"
                    } else {
                        "Remote Host"
                    }))
                    // A long folder is shortened, never the buttons: they stay in view however
                    // narrow the window is (SPEC-0158 v4 behaviour 4).
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_color(c(p.ui_muted))
                            .child(match &connected {
                                Some((host, path)) => format!("{host}:{path}"),
                                None => "not connected".into(),
                            }),
                    )
                    // Ferris IDEA's own hosts (SPEC-0121 v5 behaviour 5).
                    .when(
                        connected.is_none() && !self.chooser_only && self.adding.is_none(),
                        |d| {
                            d.child(
                                button("remote-add-host", "Add Host…", &[Effect::None])
                                    .on_click(cx.listener(|this, _, _, cx| this.open_add_host(cx))),
                            )
                            .child(
                                button("remote-remove-host", "Remove Host", &[Effect::Ssh])
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.ask_remove_host(cx);
                                    })),
                            )
                        },
                    )
                    .when(connected.is_some() && !self.chooser_only, |d| {
                        d.child(
                            button(
                                "remote-terminal",
                                "SSH Terminal",
                                &[Effect::Ssh, Effect::Run],
                            )
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(RemoteEvent::Terminal))),
                        )
                        .child(
                            button("remote-hosts", "Hosts", &[Effect::None])
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(RemoteEvent::Hosts))),
                        )
                        .children(
                            [
                                ("remote-new-file", "New File", "new-file"),
                                ("remote-new-dir", "New Folder", "new-dir"),
                                ("remote-rename", "Rename", "rename"),
                                ("remote-delete", "Delete", "delete"),
                            ]
                            .map(|(id, label, key)| {
                                button(id, label, &[Effect::Ssh]).on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.press(key, cx);
                                    },
                                ))
                            }),
                        )
                        .child(match &self.backend {
                            // Ferris IDEA's backend on the host (SPEC-0158 behaviour 4).
                            Some((name, folder)) => div()
                                .min_w_0()
                                .flex_shrink()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(c(p.ui_muted))
                                .child(format!("Backend: {name} in {folder}"))
                                .into_any_element(),
                            None => button(
                                "remote-backend",
                                "Start Backend",
                                &[Effect::Ssh, Effect::Run],
                            )
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(RemoteEvent::StartBackend)))
                            .into_any_element(),
                        })
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child(help),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .pb_1()
                    .child(
                        div()
                            .w(px(36.))
                            .flex_none()
                            .text_color(c(p.ui_muted))
                            .child(if connected.is_some() { "Path" } else { "Host" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(24.))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if field_focused {
                                c(p.focus)
                            } else {
                                c(p.separator)
                            })
                            .relative()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(18.))
                                    .child(self.field.clone()),
                            )
                            // Enter in the field connects to the host or goes to the path over
                            // SSH (SPEC-0319 v7 behaviour 56).
                            .semantics(
                                Semantics::new(Role::TextInput)
                                    .name(if connected.is_some() { "Path" } else { "Host" })
                                    .value(self.field_text(cx))
                                    .focused(field_focused)
                                    .test_id("remote-field")
                                    .action(AxAction::Focus)
                                    .effects(&[Effect::Ssh])
                                    // The window takes these from the field; the others edit
                                    // it or go to the keymap.
                                    .handles_keys(&["enter", "escape", "up", "down"]),
                            )
                            .when(empty_field, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .left(px(8.))
                                        .text_color(c(p.ui_dim))
                                        .child(placeholder),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .id("remote-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .px_1()
                    .when_some(form, |d, f| d.child(f))
                    .when_some(status, |d, (text, bad)| {
                        d.child(
                            div()
                                .px_2()
                                .py_1()
                                .when(bad, |d| d.text_color(rgb(p.console_ansi_text[1])))
                                .when(!bad, |d| d.text_color(c(p.ui_muted)))
                                .child(text),
                        )
                    })
                    .children(rows.into_iter().enumerate().map(|(i, row)| {
                        let on = i == selected;
                        let dir = matches!(&row, Row::Up)
                            || matches!(&row, Row::Entry(e, _, _) if e.kind == EntryKind::Dir);
                        let depth = row_depth(&row);
                        // The triangle before a folder: open in place or close (behaviour 9).
                        let triangle = match &row {
                            Row::Entry(e, folder, _) if e.kind == EntryKind::Dir => {
                                let open = self
                                    .expanded
                                    .contains_key(&ferris_idea_remote::join(folder, &e.name));
                                Some(open)
                            }
                            _ => None,
                        };
                        div()
                            .id(("remote-row", i))
                            .relative()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .px_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .when(dir, |d| d.font_weight(gpui::FontWeight::MEDIUM))
                            .pl(px(8. + 16. * depth as f32))
                            .child(
                                div()
                                    .id(("remote-row-toggle", i))
                                    .w(px(14.))
                                    .flex_none()
                                    .text_color(c(p.ui_muted))
                                    .when_some(triangle, |d, open| {
                                        d.child(if open { "▾" } else { "▸" }).on_click(cx.listener(
                                            move |this, _, _, cx| {
                                                this.selected = i;
                                                this.toggle_selected(!open, cx);
                                                cx.stop_propagation();
                                                cx.notify();
                                            },
                                        ))
                                    }),
                            )
                            .child(row_label(&row))
                            .semantics(
                                Semantics::new(Role::ListItem)
                                    .name(row_text(&row))
                                    .selected(on)
                                    .test_id(format!("remote-row-{i}"))
                                    .action(AxAction::Click)
                                    // A double click connects, browses or opens over SSH.
                                    .effects(&[Effect::Ssh]),
                            )
                            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                // A double click acts on the row, not on the field: a hit or a
                                // configuration directly (SPEC-0158 v4 behaviour 10), anything
                                // else as Enter on the folder shown.
                                if ev.click_count() > 1 && !this.act_on_listed(cx) {
                                    let current = this.place.as_ref().map(|(_, p)| p.clone());
                                    this.set_field(current.as_deref().unwrap_or(""), cx);
                                    this.press("enter", cx);
                                }
                                cx.notify();
                            }))
                    })),
            )
    }
}

/// Row of entry `name` in a folder list that starts with `before` other rows (".." and the
/// remote project's Open row; `SPEC-0253` behaviour 4).
/// What was typed when it names a host other than SSH (`SPEC-0321` behaviour 24): `<host> <folder>`.
/// `None` for an SSH host.
fn other_host(typed: &str) -> Option<Result<(String, String), String>> {
    let (host, rest) = typed
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((typed.trim(), ""));
    if !host.contains("://") {
        return None;
    }
    let folder = rest.trim();
    Some(if folder.is_empty() {
        Err(format!(
            "Type the folder after the host: {host} /path/to/project"
        ))
    } else {
        Ok((host.to_owned(), folder.to_owned()))
    })
}

/// Reads `folder` on `remote` and shows it open in place in `view` (`SPEC-0253` v3 behaviour
/// 9), for whichever window owns the view.
pub fn expand<T: 'static>(
    remote: ferris_idea_remote::Remote,
    view: Entity<RemoteView>,
    folder: String,
    cx: &mut Context<T>,
) {
    let path = folder.clone();
    let task = cx
        .background_executor()
        .spawn(async move { remote.list(&path) });
    cx.spawn(async move |_, cx| {
        let result = task.await;
        let _ = view.update(cx, |v, cx| match result {
            Ok(entries) => v.show_children(folder, entries, cx),
            Err(e) => v.show_children_error(folder, e.to_string(), cx),
        });
    })
    .detach();
}

/// Row of entry `name` of the folder shown (depth 0; `SPEC-0253` behaviour 4).
fn entry_row(rows: &[Row], name: &str) -> Option<usize> {
    rows.iter()
        .position(|r| matches!(r, Row::Entry(e, _, 0) if e.name == name))
}

/// The rows of `entries` of `folder` at `depth`, each open folder followed by its own rows
/// (`SPEC-0253` v3 behaviour 9).
fn tree_rows(
    rows: &mut Vec<Row>,
    folder: &str,
    entries: &[RemoteEntry],
    depth: usize,
    expanded: &BTreeMap<String, Option<Vec<RemoteEntry>>>,
) {
    for e in entries {
        rows.push(Row::Entry(e.clone(), folder.to_owned(), depth));
        if e.kind != EntryKind::Dir || depth > 32 {
            continue;
        }
        let path = ferris_idea_remote::join(folder, &e.name);
        match expanded.get(&path) {
            None => {}
            Some(None) => rows.push(Row::Note("reading…".into(), depth + 1)),
            Some(Some(inner)) if inner.is_empty() => {
                rows.push(Row::Note("empty".into(), depth + 1))
            }
            Some(Some(inner)) => tree_rows(rows, &path, inner, depth + 1, expanded),
        }
    }
}

/// The name typing can jump to: an entry's or a host's name.
fn row_name(r: &Row) -> Option<String> {
    match r {
        Row::Entry(e, _, _) => Some(e.name.clone()),
        Row::Host(h) => Some(h.alias.clone()),
        _ => None,
    }
}

/// The first row whose name starts with `typed`, without regard to case, else the first whose
/// name contains it (`SPEC-0253` v3 behaviour 9).
fn jump_row(names: &[Option<String>], typed: &str) -> Option<usize> {
    let typed = typed.to_lowercase();
    if typed.is_empty() {
        return None;
    }
    let lower: Vec<Option<String>> = names
        .iter()
        .map(|n| n.as_ref().map(|n| n.to_lowercase()))
        .collect();
    lower
        .iter()
        .position(|n| n.as_ref().is_some_and(|n| n.starts_with(&typed)))
        .or_else(|| {
            lower
                .iter()
                .position(|n| n.as_ref().is_some_and(|n| n.contains(&typed)))
        })
}

/// Whether a key's text is a character typed to jump to a name: one visible character, no
/// space or control character.
fn typed_char(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_control() && !c.is_whitespace())
}

/// Row whose text is `text`, else the first row starting with `text`.
fn row_matching(texts: &[String], text: &str) -> Option<usize> {
    texts
        .iter()
        .position(|t| t == text)
        .or_else(|| texts.iter().position(|t| t.starts_with(text)))
}

/// What a key does in the list: the key itself, or the dialog's command for a shortcut; `None`
/// for any other key with a modifier. ⌘ is Ctrl outside macOS (`SPEC-0028` v8 behaviour 13).
fn command<'a>(key: &'a str, m: &gpui::Modifiers, macos: bool) -> Option<&'a str> {
    let primary = if macos { m.platform } else { m.control };
    Some(match key {
        "up" if m.alt => "up-dir",
        "n" if primary && m.shift => "new-dir",
        "n" if primary => "new-file",
        "f6" if m.shift => "rename",
        "backspace" if primary => "delete",
        _ if m.platform || m.control || m.alt => return None,
        other => other,
    })
}

/// A shortcut of this dialog written for the platform: ⌘ is Ctrl and ⌥ is Alt outside macOS
/// (`SPEC-0028` v8 behaviour 13).
fn keys(text: &str) -> String {
    ferris_idea_ui::keymap::local_keys(text, cfg!(target_os = "macos"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0028 v8 AK-15: the list's shortcuts take Ctrl outside macOS, and its help says so.
    #[test]
    fn shortcuts_follow_the_platform() {
        let m = |control: bool, platform: bool, alt: bool, shift: bool| gpui::Modifiers {
            control,
            platform,
            alt,
            shift,
            ..Default::default()
        };
        assert_eq!(
            command("n", &m(true, false, false, false), false),
            Some("new-file")
        );
        assert_eq!(
            command("n", &m(true, false, false, true), false),
            Some("new-dir")
        );
        assert_eq!(
            command("backspace", &m(true, false, false, false), false),
            Some("delete")
        );
        assert_eq!(
            command("n", &m(false, true, false, false), true),
            Some("new-file")
        );
        assert_eq!(
            command("n", &m(true, false, false, false), true),
            None,
            "⌃N is not ⌘N"
        );
        assert_eq!(
            command("up", &m(false, false, true, false), false),
            Some("up-dir")
        );
        assert_eq!(command("x", &m(true, false, false, false), false), None);
        assert_eq!(
            command("down", &m(false, false, false, false), false),
            Some("down")
        );
        assert_eq!(
            ferris_idea_ui::keymap::local_keys(
                "Enter opens · ⌥↑ up · ⌘N file · ⇧⌘N folder · ⌘⌫ delete",
                false
            ),
            "Enter opens · Alt+↑ up · Ctrl+N file · Ctrl+Shift+N folder · Ctrl+Backspace delete"
        );
    }

    #[test]
    fn other_hosts_take_a_folder_after_the_host() {
        assert_eq!(other_host("dev"), None);
        assert_eq!(other_host("me@dev:2222"), None);
        assert_eq!(
            other_host("docker://web /app"),
            Some(Ok(("docker://web".into(), "/app".into())))
        );
        assert_eq!(
            other_host("paired://h:1#CODE C:\\work"),
            Some(Ok(("paired://h:1#CODE".into(), "C:\\work".into())))
        );
        assert!(matches!(other_host("wsl://Ubuntu"), Some(Err(_))));
    }

    #[test]
    fn rows_read_like_a_file_list() {
        let h = HostInfo {
            alias: "dev".into(),
            user: Some("me".into()),
            hostname: Some("10.0.0.5".into()),
            port: Some(2222),
            ..HostInfo::default()
        };
        assert_eq!(
            row_text(&Row::Host(h.clone())),
            "dev  me@10.0.0.5:2222  · ~/.ssh/config"
        );
        // SPEC-0121 v5 behaviour 5: every host row names its source.
        let own = HostInfo {
            source: HostSource::FerrisIdea,
            port: Some(22),
            ..h
        };
        assert_eq!(row_text(&Row::Host(own)), "dev  me@10.0.0.5  · Ferris IDEA");
        let e = RemoteEntry {
            name: "src".into(),
            kind: EntryKind::Dir,
            link: true,
        };
        assert_eq!(row_text(&Row::Entry(e.clone(), "/".into(), 0)), "src/  →");
        assert_eq!(row_text(&Row::Entry(e, "/srv".into(), 2)), "    src/  →");
        assert_eq!(row_text(&Row::Up), "..");
        assert_eq!(
            row_text(&Row::Config("app".into(), "python3 app.py".into())),
            "▶ app  python3 app.py"
        );
    }

    fn entry(name: &str, kind: EntryKind) -> RemoteEntry {
        RemoteEntry {
            name: name.into(),
            kind,
            link: false,
        }
    }

    #[test]
    fn the_new_entry_is_selected_below_the_up_row() {
        let entries = [
            entry("src", EntryKind::Dir),
            entry("made.txt", EntryKind::File),
            entry("notes.txt", EntryKind::File),
        ];
        let mut rows = vec![Row::Up];
        tree_rows(&mut rows, "/srv", &entries, 0, &BTreeMap::new());
        assert_eq!(entry_row(&rows, "made.txt"), Some(2));
        assert_eq!(entry_row(&rows, "gone.txt"), None);
        // Remote Development puts its Open row before "..".
        let mut rows = vec![Row::OpenHere("/srv".into()), Row::Up];
        tree_rows(&mut rows, "/srv", &entries, 0, &BTreeMap::new());
        assert_eq!(entry_row(&rows, "made.txt"), Some(3));
    }

    /// SPEC-0253 v3 AK-7: open folders show their entries indented below them, a folder being
    /// read says so, and an entry inside an open folder knows its own folder.
    #[test]
    fn open_folders_show_their_entries_in_place() {
        let top = [
            entry("alpha", EntryKind::Dir),
            entry("Beta", EntryKind::Dir),
        ];
        let mut expanded = BTreeMap::new();
        expanded.insert(
            "/srv/alpha".to_owned(),
            Some(vec![
                entry("deep", EntryKind::Dir),
                entry("in.txt", EntryKind::File),
            ]),
        );
        expanded.insert("/srv/alpha/deep".to_owned(), None);
        let mut rows = Vec::new();
        tree_rows(&mut rows, "/srv", &top, 0, &expanded);
        let texts: Vec<String> = rows.iter().map(row_text).collect();
        assert_eq!(
            texts,
            ["alpha/", "  deep/", "    reading…", "  in.txt", "Beta/"]
        );
        assert!(
            matches!(&rows[3], Row::Entry(e, folder, 1) if e.name == "in.txt" && folder == "/srv/alpha")
        );
    }

    /// SPEC-0253 v3 AK-7: typing jumps to the first name starting with the letters, without
    /// regard to case, else to the first containing them; nothing matches, nothing moves.
    #[test]
    fn typing_jumps_to_the_first_matching_name() {
        let names: Vec<Option<String>> = [
            None,
            Some("alpha"),
            Some("Beta"),
            Some("Zeta.txt"),
            Some("zz-beta"),
        ]
        .into_iter()
        .map(|n| n.map(str::to_owned))
        .collect();
        assert_eq!(jump_row(&names, "zet"), Some(3));
        assert_eq!(jump_row(&names, "BE"), Some(2));
        assert_eq!(jump_row(&names, "eta.t"), Some(3));
        assert_eq!(jump_row(&names, "q"), None);
        assert_eq!(jump_row(&names, ""), None);
        assert!(typed_char("z") && typed_char("Z") && typed_char(".") && typed_char("é"));
        assert!(!typed_char(" ") && !typed_char("\r") && !typed_char("ab") && !typed_char(""));
    }

    #[test]
    fn remote_development_lists_recent_projects_and_opens_the_folder_shown() {
        let recent = ferris_idea_remote::RecentRemote {
            host: "build".into(),
            folder: "/srv/app".into(),
        };
        assert_eq!(
            row_text(&Row::Recent(recent)),
            "Recent: app — build  /srv/app"
        );
        assert_eq!(
            row_text(&Row::OpenHere("/srv/app".into())),
            "Open /srv/app as a remote project"
        );
    }

    #[test]
    fn a_name_missing_from_a_stale_list_selects_nothing() {
        // The list read before New File finished: selecting the new name must not fall back to
        // another entry, or Rename would act on that one (self-test of 2026-11-05).
        let stale: Vec<String> = ["..", "src/", "notes.txt"].map(String::from).into();
        assert_eq!(row_matching(&stale, "made.txt"), None);
        let fresh: Vec<String> = ["..", "src/", "made.txt.bak", "made.txt"]
            .map(String::from)
            .into();
        assert_eq!(row_matching(&fresh, "made.txt"), Some(3));
        assert_eq!(row_matching(&fresh, "src"), Some(1));
    }

    #[test]
    fn prompts_name_what_happens() {
        assert_eq!(
            pending_prompt(&Pending::NewFile("/srv/app".into())),
            "New file in /srv/app: type a name, Enter creates, Escape cancels"
        );
        assert_eq!(
            pending_prompt(&Pending::Delete("/srv/app/notes.txt".into(), 0)),
            "Delete notes.txt? Enter deletes, Escape keeps it"
        );
        assert_eq!(
            pending_prompt(&Pending::Delete("/srv/app/src".into(), 42)),
            "Delete src/ and its 42 entries? Enter deletes, Escape keeps it"
        );
        assert!(pending_prompt(&Pending::Rename("/a/b.txt".into())).starts_with("Rename b.txt:"));
    }
}
