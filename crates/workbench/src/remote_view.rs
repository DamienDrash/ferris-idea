//! The Remote Host tool window (`SPEC-0121`): the SSH hosts of the user's config, then the folders
//! and files of the chosen host; Enter opens a file for editing, the field takes a host or a path.
//! New File, New Folder, Rename and Delete change the folder (`SPEC-0253`); with a backend, `@`,
//! `!` and `>` list symbols, problems and run configurations of the host's project (`SPEC-0158`).

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, rgb,
};
use idecraft_remote::{EntryKind, HostInfo, RemoteEntry};

use crate::editor_view::EditorView;
use crate::palette::Palette;
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
    /// Start IDECraft's backend on the host for the folder shown (`SPEC-0158`).
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
}

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Host(HostInfo),
    Up,
    Entry(RemoteEntry),
    /// A search hit: absolute path on the host, 0-based line, the line's text.
    Hit(String, usize, String),
    /// A run configuration of the host's project: name and command.
    Config(String, String),
    /// A recent remote project (Remote Development, `SPEC-0317` v3 behaviour 1).
    Recent(idecraft_remote::RecentRemote),
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
    /// A change waiting for a name or a confirmation.
    pending: Option<Pending>,
    /// Entry to select once the folder is read again.
    select_after: Option<String>,
    /// Remote Development: choosing a host and a folder opens it as a remote project; the host
    /// list starts with the recent remote projects (`SPEC-0317` v3 behaviour 1).
    project_mode: bool,
    recent: Vec<idecraft_remote::RecentRemote>,
}

impl EventEmitter<RemoteEvent> for RemoteView {}

fn host_text(h: &HostInfo) -> String {
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
        Row::Host(h) => host_text(h),
        Row::Up => "..".into(),
        Row::Entry(e) => format!(
            "{}{}{}",
            e.name,
            if e.kind == EntryKind::Dir { "/" } else { "" },
            if e.link { "  →" } else { "" }
        ),
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
            pending: None,
            select_after: None,
            project_mode: false,
            recent: Vec::new(),
        }
    }

    /// Remote Development (`SPEC-0317` v3 behaviour 1) with `recent` remote projects first, or
    /// the plain Remote Host window.
    pub fn set_project_mode(
        &mut self,
        on: bool,
        recent: Vec<idecraft_remote::RecentRemote>,
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
                rows.extend(self.entries.iter().cloned().map(Row::Entry));
                rows
            }
        };
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    fn set_field(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Shows the configured hosts (and leaves a connected host).
    pub fn show_hosts(&mut self, hosts: Vec<HostInfo>, cx: &mut Context<Self>) {
        self.hosts = hosts;
        if self.place.is_none() {
            self.rebuild();
        }
        cx.notify();
    }

    /// Back to the host list.
    pub fn leave(&mut self, cx: &mut Context<Self>) {
        self.place = None;
        self.backend = None;
        self.hits = None;
        self.configs = None;
        self.pending = None;
        self.entries.clear();
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
        self.set_field(&path, cx);
        self.place = Some((host, path));
        self.entries = entries;
        self.busy = None;
        self.error = None;
        if !same {
            self.selected = 0;
        }
        self.rebuild();
        if let Some(name) = self.select_after.take() {
            let before = self
                .rows
                .iter()
                .take_while(|r| matches!(r, Row::Up | Row::OpenHere(_)))
                .count();
            if let Some(i) = entry_row(&self.entries, before, &name) {
                self.selected = i;
            }
        }
        cx.notify();
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
        self.busy = None;
        self.error = Some(error);
        cx.notify();
    }

    /// Focus handle of the window.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Focus handle of the host/path field.
    pub fn field_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.field.read(cx).focus()
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

    fn path_from_field(&self, text: &str) -> String {
        let text = text.trim();
        match &self.place {
            Some((_, dir)) if !text.starts_with('/') && !text.starts_with('~') => {
                idecraft_remote::join(dir, text)
            }
            _ => text.to_owned(),
        }
    }

    /// Handles one key as the window does (for tests too): Enter connects, opens a folder or a
    /// file — or goes to the host or path typed in the field; Alt+↑ goes up; F5 reads again; Escape hides.
    pub fn press(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
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
        let selected_entry = match (self.rows.get(self.selected), &self.place) {
            (Some(Row::Entry(e)), Some((_, dir))) => {
                Some((idecraft_remote::join(dir, &e.name), e.name.clone()))
            }
            _ => None,
        };
        match key {
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
            "enter" if typed.starts_with('@') && self.backend.is_some() => {
                let query = typed[1..].trim();
                if query.is_empty() {
                    return true;
                }
                cx.emit(RemoteEvent::Symbols(query.to_owned()));
            }
            "enter" if typed == "!" && self.backend.is_some() => cx.emit(RemoteEvent::Problems),
            "enter" if typed == ">" && self.backend.is_some() => cx.emit(RemoteEvent::RunConfigs),
            "enter" if self.configs.is_some() => {
                if let Some(Row::Config(name, _)) = self.rows.get(self.selected) {
                    cx.emit(RemoteEvent::RunConfig(name.clone()));
                }
            }
            "down" if n > 0 => self.selected = (self.selected + 1).min(n - 1),
            "up" => self.selected = self.selected.saturating_sub(1),
            "enter" => {
                let current = self.place.as_ref().map(|(_, p)| p.clone());
                // `?text` searches the host's project through the backend (SPEC-0158).
                if let Some(query) = typed.strip_prefix('?').map(str::trim)
                    && !query.is_empty()
                    && self.backend.is_some()
                {
                    cx.emit(RemoteEvent::Search(query.to_owned()));
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
                            cx.emit(RemoteEvent::Browse(idecraft_remote::parent(&dir)))
                        }
                        (Some(Row::Entry(e)), Some(dir)) => {
                            let path = idecraft_remote::join(&dir, &e.name);
                            if e.kind == EntryKind::Dir {
                                cx.emit(RemoteEvent::Browse(path));
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
                    cx.emit(RemoteEvent::Browse(idecraft_remote::parent(dir)))
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
        let in_field = self.field.read(cx).focus().is_focused(window);
        let plain = !(m.platform || m.control || m.alt || m.shift);
        if in_field && !(plain && matches!(k.key.as_str(), "enter" | "escape" | "up" | "down")) {
            return;
        }
        let key = match k.key.as_str() {
            "up" if m.alt => "up-dir",
            "n" if m.platform && m.shift => "new-dir",
            "n" if m.platform => "new-file",
            "f6" if m.shift => "rename",
            "backspace" if m.platform => "delete",
            other if m.platform || m.control || m.alt => {
                let _ = other;
                return;
            }
            other => other,
        }
        .to_owned();
        if matches!(
            key.as_str(),
            "up" | "down"
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
        let rows = self.rows.clone();
        let connected = self.place.clone();
        let field_focused = self.field.read(cx).focus().is_focused(window);
        let empty_field = self.field_text(cx).is_empty();
        let placeholder = if connected.is_some() {
            "Path on the host (absolute, ~ or relative), Enter goes there"
        } else {
            "Host alias or user@host[:port], Enter connects"
        };
        let help = if self.project_mode && connected.is_some() {
            "Go to the project folder, then Enter on its Open row · Escape stops opening"
        } else if self.project_mode {
            "Recent remote projects and hosts from ~/.ssh/config · Enter opens or connects"
        } else if connected.is_some() && self.backend.is_some() {
            "Enter opens · ⌘N file · ⇧⌘N folder · ⇧F6 rename · ⌘⌫ delete · ?text @symbol ! problems > run"
        } else if connected.is_some() {
            "Enter opens · ⌥↑ up · F5 reads again · ⌘N file · ⇧⌘N folder · ⇧F6 rename · ⌘⌫ delete"
        } else {
            "Hosts from ~/.ssh/config · Enter connects with your SSH keys and agent"
        };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tab_hover)))
                .child(label)
        };
        let status = match (&self.busy, &self.error, &connected) {
            _ if self.pending.is_some() => {
                self.pending.as_ref().map(|p| (pending_prompt(p), false))
            }
            (Some(b), _, _) => Some((b.clone(), false)),
            (None, Some(e), _) => Some((e.clone(), true)),
            (None, None, None) if rows.is_empty() => Some((
                "No hosts in ~/.ssh/config: type user@host into the field".into(),
                false,
            )),
            (None, None, Some(_)) if rows.len() <= 1 => {
                Some(("This folder is empty".into(), false))
            }
            _ => None,
        };
        div()
            .id("remote-panel")
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_key))
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
                    .child(div().text_color(c(p.ui_muted)).child(match &connected {
                        Some((host, path)) => format!("{host}:{path}"),
                        None => "not connected".into(),
                    }))
                    .when(connected.is_some(), |d| {
                        d.child(
                            button("remote-terminal", "SSH Terminal").on_click(
                                cx.listener(|_, _, _, cx| cx.emit(RemoteEvent::Terminal)),
                            ),
                        )
                        .child(
                            button("remote-hosts", "Hosts")
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
                                button(id, label).on_click(cx.listener(move |this, _, _, cx| {
                                    this.press(key, cx);
                                }))
                            }),
                        )
                        .child(match &self.backend {
                            // IDECraft's backend on the host (SPEC-0158 behaviour 4).
                            Some((name, folder)) => div()
                                .text_color(c(p.ui_muted))
                                .child(format!("Backend: {name} in {folder}"))
                                .into_any_element(),
                            None => button("remote-backend", "Start Backend")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(RemoteEvent::StartBackend)),
                                )
                                .into_any_element(),
                        })
                    })
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(help)),
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
                            || matches!(&row, Row::Entry(e) if e.kind == EntryKind::Dir);
                        div()
                            .id(("remote-row", i))
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
                            .child(row_text(&row))
                            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                if ev.click_count() > 1 {
                                    // A double click acts on the row, not on the field.
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

fn entry_row(entries: &[RemoteEntry], before: usize, name: &str) -> Option<usize> {
    let i = entries.iter().position(|e| e.name == name)?;
    Some(i + before)
}

/// Row whose text is `text`, else the first row starting with `text`.
fn row_matching(texts: &[String], text: &str) -> Option<usize> {
    texts
        .iter()
        .position(|t| t == text)
        .or_else(|| texts.iter().position(|t| t.starts_with(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        assert_eq!(row_text(&Row::Host(h)), "dev  me@10.0.0.5:2222");
        let e = RemoteEntry {
            name: "src".into(),
            kind: EntryKind::Dir,
            link: true,
        };
        assert_eq!(row_text(&Row::Entry(e)), "src/  →");
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
        assert_eq!(entry_row(&entries, 1, "made.txt"), Some(2));
        assert_eq!(entry_row(&entries, 0, "made.txt"), Some(1));
        assert_eq!(entry_row(&entries, 1, "gone.txt"), None);
        // Remote Development puts its Open row before "..".
        assert_eq!(entry_row(&entries, 2, "made.txt"), Some(3));
    }

    #[test]
    fn remote_development_lists_recent_projects_and_opens_the_folder_shown() {
        let recent = idecraft_remote::RecentRemote {
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
