//! The Search Everywhere popup (`SPEC-0067`): one field for files, classes, symbols, actions,
//! text and HTTP endpoints.

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use idecraft_core::Workspace;
use idecraft_languages::endpoints::{Endpoint, EndpointKind, project_endpoints, search_endpoints};
use idecraft_languages::everywhere::{TextOptions, search_everywhere};

use crate::EditorView;
use crate::code_preview::{ExcerptLine, excerpt};
use crate::everywhere_model::{Filter, Results, Row};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;

/// Id prefix of the settings pages offered among the actions (`SPEC-0067` v5 behaviour 16).
pub const SETTINGS_PREFIX: &str = "Settings:";

/// Height of a result row.
const ROW: f32 = 26.;

/// Lines of the preview above and below the hit (v5 behaviour 12).
const PREVIEW_CONTEXT: usize = 6;

/// What the popup asks the workbench to do.
pub enum EverywhereEvent {
    /// Open a file (absolute path), optionally at a 0-based line and column.
    Open(PathBuf, Option<usize>, Option<usize>),
    /// Open a file in the right split (⇧↩, v5 behaviour 11).
    OpenRight(PathBuf, Option<usize>, Option<usize>),
    /// Run an action by id.
    Action(String),
    /// Open the Settings dialog on the page with this title (v5 behaviour 16).
    Settings(String),
    /// Show the text search in the Find in Files panel, kept open (v5 behaviour 17).
    FindInFiles(String, TextOptions),
    /// Copy a calculator result (v5 behaviour 15).
    Copy(String),
    /// Close the popup.
    Close,
}

/// What a search ran for; a new key starts a new search.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchKey {
    query: String,
    filter: Filter,
    everything: bool,
    options: TextOptions,
    hidden_groups: BTreeSet<&'static str>,
}

/// Query, tab and results of the popup.
pub struct EverywhereView {
    root: PathBuf,
    /// In a remote project: the stand-in folder and the folder on the host.
    remote_folders: Option<(PathBuf, String)>,
    query: Entity<EditorView>,
    filter: Filter,
    results: Results,
    /// Workspace files, listed once per popup.
    files: Option<Arc<Vec<PathBuf>>>,
    /// Workspace files including excluded folders (non-project items), listed when asked for.
    all_files: Option<Arc<Vec<PathBuf>>>,
    /// The project's own files for Files and Go to File, with the line on what they leave out
    /// (`SPEC-0067` v6 behaviour 13, `SPEC-0031` v5 behaviour 12); listed once per popup.
    own_files: Option<Arc<(Vec<PathBuf>, Option<String>)>>,
    /// Files open in editors, workspace-relative: found whatever they are.
    open: Arc<Vec<PathBuf>>,
    /// The rows of the Endpoints model, scanned once per popup (v9 behaviour 20).
    endpoints: Option<Arc<Vec<Endpoint>>>,
    /// What the files of the last search left out, in one line.
    not_listed: Option<String>,
    /// Cancels the running search when a newer query arrives.
    running: Option<Arc<AtomicBool>>,
    searched: Option<SearchKey>,
    /// Plugin actions and settings pages: id and title (SPEC-0104, v5 behaviour 16).
    extra_actions: Arc<Vec<(String, String)>>,
    /// Actions that do not apply now and are not offered (`SPEC-0004` v6 behaviour 16).
    hidden_actions: BTreeSet<String>,
    /// Recently opened files, workspace-relative, most recent first, without the current one.
    recent: Arc<Vec<PathBuf>>,
    /// "Include non-project items" (v5 behaviour 13).
    everything: bool,
    /// Options of the Text tab.
    text_options: TextOptions,
    /// File types turned off in the filter (v5 behaviour 14).
    hidden_types: BTreeSet<String>,
    /// Groups of the All tab turned off in the filter.
    hidden_groups: BTreeSet<&'static str>,
    /// Whether the preview is shown (v5 behaviour 12).
    preview: bool,
    /// Whether the filter menu is open.
    filter_menu: bool,
    /// Line and column asked for with `name:line[:column]`.
    goto: (Option<usize>, Option<usize>),
    /// The preview of the selected hit: path, line and its lines.
    preview_cache: Option<(String, usize, Vec<ExcerptLine>)>,
    /// The hit whose preview was asked of the host (`SPEC-0318` v5 behaviour 41).
    preview_asked: Option<(String, usize)>,
    scroll: UniformListScrollHandle,
    _watch: Subscription,
    /// Files, classes, symbols and text come from the host of a remote project (`SPEC-0318` v2
    /// detail 6); actions and the calculator stay the window's own.
    remote: Option<crate::workbench_view::HostCall>,
}

impl EventEmitter<EverywhereEvent> for EverywhereView {}

/// The host's hits of `categories` for `query` (`search.everywhere`, behaviour 14), with their
/// highlight positions (protocol detail 8).
/// The characters of a hit to mark in the preview.
fn hit_mark(row: &Row, text: &str) -> Option<Range<usize>> {
    match row {
        Row::Text(t) => Some(t.column..t.column + t.length),
        Row::Class(s) | Row::Symbol(s) => {
            let l = text.split('\n').nth(s.line)?;
            let b = l.find(&s.name)?;
            let start = l[..b].chars().count();
            Some(start..start + s.name.chars().count())
        }
        _ => None,
    }
}

/// Whether this search lists the project's files here (not on a remote host).
fn local_files_wanted(
    remote: &Option<crate::workbench_view::HostCall>,
    categories: idecraft_languages::everywhere::Categories,
) -> bool {
    remote.is_none() && categories.files
}

fn host_hits(
    host: &crate::workbench_view::HostCall,
    query: &str,
    categories: idecraft_languages::everywhere::Categories,
    limit: usize,
    everything: bool,
    known: &[PathBuf],
) -> Option<idecraft_languages::everywhere::Everywhere> {
    let mut wanted = Vec::new();
    for (on, name) in [
        (categories.files, "files"),
        (categories.classes, "classes"),
        (categories.symbols, "symbols"),
        (categories.text, "text"),
    ] {
        if on {
            wanted.push(name);
        }
    }
    if wanted.is_empty() || query.trim().is_empty() {
        return None;
    }
    let v = host(
        "search.everywhere",
        serde_json::json!({
            "query": query, "categories": wanted, "limit": limit,
            "include_non_project": everything,
            "recent": known.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect::<Vec<_>>(),
        }),
    )
    .ok()?;
    let mut hits: idecraft_languages::everywhere::Everywhere = serde_json::from_value(v).ok()?;
    // Only paths inside the project (behaviour 3, SEC-14).
    let inside = |p: &str| idecraft_session::paths::check_relative(p).is_ok();
    if let Some(f) = hits.files.as_mut() {
        f.retain(|h| inside(&h.path));
    }
    for list in [hits.classes.as_mut(), hits.symbols.as_mut()]
        .into_iter()
        .flatten()
    {
        list.retain(|h| inside(&h.path));
    }
    if let Some(t) = hits.text.as_mut() {
        t.retain(|h| inside(&h.path));
    }
    Some(hits)
}

/// Groups of the All tab the filter can turn off.
const GROUPS: [&str; 5] = ["Files", "Symbols", "Actions", "Text", "Calculator"];

impl EverywhereView {
    /// A popup searching the workspace at `root`.
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let query = cx.new(EditorView::field);
        let watch = cx.observe(&query, |this, _, cx| this.search(cx));
        Self {
            root,
            remote_folders: None,
            query,
            filter: Filter::All,
            results: Results::default(),
            files: None,
            all_files: None,
            own_files: None,
            open: Arc::default(),
            endpoints: None,
            not_listed: None,
            running: None,
            searched: None,
            extra_actions: Arc::default(),
            hidden_actions: BTreeSet::new(),
            recent: Arc::default(),
            everything: false,
            text_options: TextOptions::default(),
            hidden_types: BTreeSet::new(),
            hidden_groups: BTreeSet::new(),
            preview: false,
            filter_menu: false,
            goto: (None, None),
            preview_cache: None,
            preview_asked: None,
            scroll: UniformListScrollHandle::new(),
            _watch: watch,
            remote: None,
        }
    }

    /// Replaces the query, as typing it does (the window self-test).
    pub fn set_query(&mut self, text: &str, cx: &mut Context<Self>) {
        self.query.update(cx, |q, cx| q.set_text(text, cx));
    }

    /// In a remote project: the stand-in folder and the host folder; the footer shows host paths.
    pub fn set_remote_folders(&mut self, folders: Option<(PathBuf, String)>) {
        self.remote_folders = folders;
    }

    /// Takes files, classes, symbols and text from the host of a remote project from now on.
    pub fn set_remote(&mut self, host: crate::workbench_view::HostCall) {
        self.remote = Some(host);
        self.files = Some(Arc::new(Vec::new()));
        self.all_files = Some(Arc::new(Vec::new()));
        self.searched = None;
    }

    /// Actions of plugins and settings pages the search also offers (SPEC-0104, v5 behaviour 16).
    pub fn set_extra_actions(&mut self, actions: Vec<(String, String)>) {
        self.extra_actions = Arc::new(actions);
    }

    /// Actions that do not apply now: the search leaves them out (`SPEC-0004` v6 behaviour 16).
    pub fn set_hidden_actions(&mut self, ids: impl IntoIterator<Item = String>) {
        self.hidden_actions = ids.into_iter().collect();
    }

    /// Recently opened files (absolute or workspace-relative), most recent first; `current` (the
    /// file in the editor) is left out (v5 behaviour 6).
    pub fn set_recent(&mut self, recent: &[PathBuf], current: Option<&Path>) {
        let relative = |p: &Path| p.strip_prefix(&self.root).unwrap_or(p).to_path_buf();
        let current = current.map(relative);
        self.recent = Arc::new(
            recent
                .iter()
                .map(|p| relative(p))
                .filter(|p| Some(p) != current.as_ref())
                .collect(),
        );
    }

    /// Files open in editors (absolute or workspace-relative); Files and Go to File find them
    /// whatever they are (`SPEC-0067` v6 behaviour 13).
    pub fn set_open(&mut self, open: &[PathBuf]) {
        let root = self.root.clone();
        self.open = Arc::new(
            open.iter()
                .filter_map(|p| match p.strip_prefix(&root) {
                    Ok(r) => Some(r.to_path_buf()),
                    Err(_) if p.is_relative() => Some(p.clone()),
                    Err(_) => None,
                })
                .collect(),
        );
    }

    /// The line on what Files and Go to File left out (libraries and ignored files), if anything.
    pub fn not_listed(&self) -> Option<&str> {
        self.not_listed.as_deref()
    }

    /// Focus handle of the query field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.query.read(cx).focus()
    }

    /// Shows the popup on `filter` with the query selected; the file list is read again and the
    /// scope is the project again (behaviour 1, IMP-0333).
    pub fn reset(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.files = None;
        self.all_files = None;
        self.own_files = None;
        self.endpoints = None;
        self.everything = false;
        self.filter_menu = false;
        self.searched = None;
        self.query.update(cx, |q, cx| {
            let len = q.state.buffer.len_chars();
            q.select_range(0..len, cx);
        });
        self.search(cx);
    }

    /// Current tab.
    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// The rows shown.
    pub fn results(&self) -> &Results {
        &self.results
    }

    /// Whether non-project items are included (v5 behaviour 13).
    pub fn everything(&self) -> bool {
        self.everything
    }

    /// Turns "include non-project items" on or off, as the checkbox and a second press of the tab's
    /// shortcut do (v5 behaviour 13).
    pub fn toggle_everything(&mut self, cx: &mut Context<Self>) {
        if self.filter.has_scope() {
            self.everything = !self.everything;
            self.search(cx);
            cx.notify();
        }
    }

    /// Whether the preview is shown.
    pub fn previewing(&self) -> bool {
        self.preview
    }

    /// Shows or hides the preview (v5 behaviour 12).
    pub fn set_preview(&mut self, on: bool, cx: &mut Context<Self>) {
        self.preview = on;
        cx.notify();
    }

    /// The preview lines of the selected hit, if shown (for tests and the window self-test).
    pub fn preview_lines(&mut self) -> Option<Vec<ExcerptLine>> {
        self.preview.then(|| self.excerpt()).flatten()
    }

    /// The footer text: the selected hit's path with `~` for the home folder (v5 behaviour 11).
    pub fn footer(&self) -> String {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        match self.results.selected_row() {
            Some(Row::Action(a)) if a.id.starts_with(SETTINGS_PREFIX) => "Settings".into(),
            Some(Row::Action(a)) => a.id.clone(),
            Some(Row::Calculator(_)) => "Enter copies the result".into(),
            Some(row) => row
                .path()
                .map(|p| {
                    crate::chrome::shown_path(
                        &self.root.join(p),
                        home.as_deref(),
                        self.remote_folders.as_ref(),
                    )
                })
                .unwrap_or_default(),
            None => String::new(),
        }
    }

    /// Searches in the background; a newer query cancels the running search (behaviour 2).
    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).state.buffer.text();
        let key = SearchKey {
            query: query.clone(),
            filter: self.filter,
            everything: self.everything,
            options: self.text_options,
            hidden_groups: self.hidden_groups.clone(),
        };
        if self.searched.as_ref() == Some(&key) {
            return;
        }
        self.searched = Some(key);
        if let Some(old) = self.running.take() {
            old.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.running = Some(cancel.clone());
        if self.filter == Filter::Endpoints {
            self.search_endpoints_tab(query, cancel, cx);
            return;
        }
        let root = self.root.clone();
        let everything = self.everything;
        let files = if everything {
            self.all_files.clone()
        } else {
            self.files.clone()
        };
        let filter = self.filter;
        let extra = self.extra_actions.clone();
        let recent = self.recent.clone();
        let open = self.open.clone();
        let own_files = self.own_files.clone();
        let options = self.text_options;
        let mut categories = filter.categories();
        if filter == Filter::All {
            categories.files &= !self.hidden_groups.contains("Files");
            categories.symbols &= !self.hidden_groups.contains("Symbols");
            categories.actions &= !self.hidden_groups.contains("Actions");
            categories.text &= !self.hidden_groups.contains("Text");
        }
        let no_calculator = self.hidden_groups.contains("Calculator");
        let remote = self.remote.clone();
        cx.spawn(async move |this, cx| {
            let (files, own, found) = cx
                .background_executor()
                .spawn(async move {
                    let files = match files {
                        Some(f) => f,
                        None => Arc::new(
                            Workspace::open(&root)
                                .and_then(|w| if everything { w.all_files() } else { w.files() })
                                .unwrap_or_default(),
                        ),
                    };
                    // Files and Go to File: the project's own sources, the open files and (in the
                    // search) the recent ones; with "include non-project items" every file.
                    let own = (!everything && local_files_wanted(&remote, categories)).then(|| {
                        own_files.unwrap_or_else(|| {
                            let listing = idecraft_core::OwnSources::new(&root).listing(None);
                            let line = listing.not_listed_line();
                            Arc::new((listing.files, line))
                        })
                    });
                    let file_names: Option<Vec<PathBuf>> = own.as_ref().map(|o| {
                        let known: std::collections::HashSet<&PathBuf> = o.0.iter().collect();
                        o.0.iter()
                            .chain(open.iter().filter(|p| !known.contains(p)))
                            .cloned()
                            .collect()
                    });
                    let keymap = crate::keymaps::current();
                    // A remote project's files, symbols and text are the host's.
                    let local = match &remote {
                        Some(_) => idecraft_languages::everywhere::Categories {
                            // (the files of a remote project are the host's, below)
                            actions: categories.actions,
                            ..idecraft_languages::everywhere::Categories::NONE
                        },
                        None => categories,
                    };
                    let mut found = search_everywhere(
                        idecraft_languages::everywhere::Sources {
                            root: &root,
                            files: &files,
                            keymap: &keymap,
                            extra_actions: &extra,
                            recent: &recent,
                            text_options: options,
                            file_names: file_names.as_deref(),
                        },
                        &query,
                        local,
                        filter.limit(),
                        Some(&cancel),
                    );
                    found.not_listed = own.as_ref().and_then(|o| o.1.clone());
                    let known_to_window: Vec<PathBuf> =
                        recent.iter().chain(open.iter()).cloned().collect();
                    if let Some(host) = &remote
                        && !cancel.load(Ordering::Relaxed)
                        && let Some(h) = host_hits(
                            host,
                            &query,
                            categories,
                            filter.limit(),
                            everything,
                            &known_to_window,
                        )
                    {
                        found.not_listed = h.not_listed.clone();
                        found.files = h.files.or(found.files);
                        found.classes = h.classes.or(found.classes);
                        found.symbols = h.symbols.or(found.symbols);
                        found.text = h.text.or(found.text);
                    }
                    if no_calculator {
                        found.calculator = None;
                    }
                    (
                        files,
                        own,
                        (!cancel.load(Ordering::Relaxed)).then_some(found),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if everything {
                    this.all_files = Some(files);
                } else {
                    this.files = Some(files);
                }
                if own.is_some() {
                    this.own_files = own;
                }
                if let Some(mut found) = found {
                    if let Some(actions) = found.actions.as_mut() {
                        actions.retain(|a| !this.hidden_actions.contains(&a.id));
                    }
                    this.not_listed = found.not_listed.clone();
                    this.goto = (found.line, found.column);
                    this.results = Results::from(found, filter, &this.hidden_types);
                    this.preview_cache = None;
                    this.preview_asked = None;
                    this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The search of the Endpoints tab (v9 behaviour 20): the rows of the Endpoints model
    /// (`SPEC-0103`), scanned in the background once per popup as the tool window scans when it
    /// opens, searched by path, a leading HTTP method and handler.
    fn search_endpoints_tab(
        &mut self,
        query: String,
        cancel: Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        let root = self.root.clone();
        let known = self.endpoints.clone();
        let limit = Filter::Endpoints.limit();
        let replaced = cancel.clone();
        cx.spawn(async move |this, cx| {
            let (model, hits) = cx
                .background_executor()
                .spawn(async move {
                    let model = known.unwrap_or_else(|| Arc::new(project_endpoints(&root)));
                    let hits = (!cancel.load(Ordering::Relaxed))
                        .then(|| search_endpoints(&model, &query, limit));
                    (model, hits)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.endpoints = Some(model);
                // The rows of a search a newer one replaced are dropped; the model is kept.
                if let Some(hits) = hits.filter(|_| !replaced.load(Ordering::Relaxed)) {
                    this.not_listed = None;
                    this.goto = (None, None);
                    this.results = Results::of_endpoints(hits, &this.hidden_types);
                    this.preview_cache = None;
                    this.preview_asked = None;
                    this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether the project's endpoints are read, and how many there are (for tests).
    pub fn endpoint_count(&self) -> Option<usize> {
        self.endpoints.as_ref().map(|e| e.len())
    }

    /// The line a file or symbol opens at: `name:line` wins (v5 behaviour 7).
    fn target(&self, row: &Row) -> Option<(PathBuf, Option<usize>, Option<usize>)> {
        let path = self.root.join(row.path()?);
        let line = self.goto.0.or(row.line());
        let column = match row {
            Row::Text(t) if self.goto.0.is_none() => Some(t.column),
            _ => self.goto.1,
        };
        Some((path, line, column))
    }

    fn choose(&mut self, right: bool, cx: &mut Context<Self>) {
        let Some(row) = self.results.selected_row().cloned() else {
            return;
        };
        let event = match &row {
            Row::Action(a) => match a.id.strip_prefix(SETTINGS_PREFIX) {
                Some(page) => EverywhereEvent::Settings(page.to_owned()),
                None => EverywhereEvent::Action(a.id.clone()),
            },
            Row::Calculator(v) => EverywhereEvent::Copy(v.clone()),
            Row::Header(_) => return,
            _ => {
                let Some((path, line, column)) = self.target(&row) else {
                    return;
                };
                if right {
                    EverywhereEvent::OpenRight(path, line, column)
                } else {
                    EverywhereEvent::Open(path, line, column)
                }
            }
        };
        cx.emit(event);
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.filter_menu = false;
        if !filter.has_scope() {
            self.everything = false;
        }
        self.search(cx);
        cx.notify();
    }

    /// Opens the Text tab's search in the Find in Files panel (v5 behaviour 17).
    fn open_in_find_window(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).state.buffer.text();
        if !query.trim().is_empty() {
            cx.emit(EverywhereEvent::FindInFiles(query, self.text_options));
        }
    }

    /// The selected hit's excerpt for the preview, computed once per hit.
    fn excerpt(&mut self) -> Option<Vec<ExcerptLine>> {
        let row = self.results.selected_row()?.clone();
        let path = row.path()?.to_owned();
        let line = row.line().unwrap_or(0);
        if let Some((p, l, lines)) = &self.preview_cache
            && *p == path
            && *l == line
        {
            return Some(lines.clone());
        }
        if self.remote.is_some() {
            // The host's lines arrive through `ask_host_preview`; until then a text hit shows
            // the line the host sent.
            let Row::Text(t) = &row else {
                return None;
            };
            let text = format!("{}{}", "\n".repeat(t.line), t.text);
            return Some(excerpt(
                Path::new(&path),
                &text,
                t.line,
                Some(t.column..t.column + t.length),
                0,
                0,
            ));
        }
        let text = std::fs::read_to_string(self.root.join(&path)).ok()?;
        let mark = hit_mark(&row, &text);
        let lines = excerpt(
            Path::new(&path),
            &text,
            line,
            mark,
            PREVIEW_CONTEXT,
            PREVIEW_CONTEXT,
        );
        self.preview_cache = Some((path, line, lines.clone()));
        Some(lines)
    }

    /// Asks the host for the lines around the selected hit (`SPEC-0318` v5 behaviour 41), once per
    /// hit.
    fn ask_host_preview(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.remote.clone() else {
            return;
        };
        let Some(row) = self.results.selected_row().cloned() else {
            return;
        };
        let Some(path) = row.path().map(str::to_owned) else {
            return;
        };
        let line = row.line().unwrap_or(0);
        let key = (path.clone(), line);
        if self.preview_asked.as_ref() == Some(&key)
            || self
                .preview_cache
                .as_ref()
                .is_some_and(|(p, l, _)| *p == path && *l == line)
        {
            return;
        }
        self.preview_asked = Some(key);
        let params = serde_json::json!({
            "path": path, "line": line, "context": PREVIEW_CONTEXT,
        });
        let task = cx
            .background_executor()
            .spawn(async move { host("search.excerpt", params) });
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |this, cx| {
                let Ok(v) = answer else {
                    return;
                };
                if this.preview_asked.as_ref() != Some(&(path.clone(), line)) {
                    return;
                }
                let first = v["first"].as_u64().unwrap_or(0) as usize;
                let body: Vec<&str> = v["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .collect();
                let text = format!("{}{}", "\n".repeat(first), body.join("\n"));
                let mark = hit_mark(&row, &text);
                let lines = excerpt(
                    Path::new(&path),
                    &text,
                    line,
                    mark,
                    PREVIEW_CONTEXT,
                    PREVIEW_CONTEXT,
                );
                this.preview_cache = Some((path, line, lines));
                cx.notify();
            });
        })
        .detach();
    }

    /// Up/Down select, Enter chooses (⇧Enter in the right split), Tab and ⇧Tab switch the tab,
    /// Escape closes (behaviours 1, 3, 5; v5 behaviour 11). Choosing and switching tabs are the
    /// keymap's actions, on whatever keys it binds them to (v7 behaviour 18).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let keymap = crate::keymaps::current();
        let bound = idecraft_ui::keymap::Keystroke::from_platform(
            &k.key, m.control, m.alt, m.shift, m.platform,
        )
        .map(|s| keymap.actions_for(&s, None))
        .unwrap_or_default();
        let Some(key) = crate::everywhere_model::popup_key(
            &k.key,
            m.shift,
            m.control || m.alt || m.platform,
            &bound,
        ) else {
            return;
        };
        self.act(key, cx);
        cx.stop_propagation();
        cx.notify();
    }

    /// Carries out one of the popup's own keymap actions by id; false for any other id (v7
    /// behaviour 18).
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(key) = crate::everywhere_model::popup_action(id) else {
            return false;
        };
        self.act(key, cx);
        cx.notify();
        true
    }

    fn act(&mut self, key: crate::everywhere_model::PopupKey, cx: &mut Context<Self>) {
        use crate::everywhere_model::PopupKey;
        match key {
            PopupKey::Up | PopupKey::Down => {
                self.results.step(key == PopupKey::Down);
                if let Some(i) = self.results.selected {
                    self.scroll.scroll_to_item(i, gpui::ScrollStrategy::Center);
                }
            }
            PopupKey::Choose { right } => self.choose(right, cx),
            PopupKey::NextGroup | PopupKey::PrevGroup => {
                self.results.step_group(key == PopupKey::NextGroup);
                if let Some(i) = self.results.selected {
                    self.scroll.scroll_to_item(i, gpui::ScrollStrategy::Center);
                }
            }
            PopupKey::NextTab => self.set_filter(self.filter.next(), cx),
            PopupKey::PrevTab => self.set_filter(self.filter.previous(), cx),
            PopupKey::Escape if self.filter_menu => self.filter_menu = false,
            PopupKey::Escape => cx.emit(EverywhereEvent::Close),
        }
    }

    /// The selected-row text with the matched chars marked (v5 behaviour 9).
    fn marked(text: &str, positions: &[usize], on: bool) -> gpui::AnyElement {
        if !on || positions.is_empty() {
            return div().flex_none().child(text.to_owned()).into_any_element();
        }
        let p = Palette::current();
        let bg = p
            .scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
            .unwrap_or(p.selection);
        let bytes: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        let byte = |i: usize| bytes.get(i).copied().unwrap_or(text.len());
        let highlights: Vec<(Range<usize>, gpui::HighlightStyle)> = positions
            .iter()
            .filter(|i| **i < bytes.len())
            .map(|i| {
                (
                    byte(*i)..byte(*i + 1),
                    gpui::HighlightStyle {
                        background_color: Some(gpui::rgb(bg).into()),
                        ..Default::default()
                    },
                )
            })
            .collect();
        div()
            .flex_none()
            .child(gpui::StyledText::new(text.to_owned()).with_highlights(highlights))
            .into_any_element()
    }

    /// A line of the Text tab with the match marked.
    fn text_line(t: &idecraft_languages::everywhere::TextHit) -> gpui::AnyElement {
        let trimmed = t.text.trim_start();
        let cut = t.text.chars().count() - trimmed.chars().count();
        let positions: Vec<usize> =
            (t.column.saturating_sub(cut)..(t.column + t.length).saturating_sub(cut)).collect();
        Self::marked(trimmed, &positions, true)
    }

    fn filter_menu_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let entry = |id: String, label: String, on: bool| {
            div()
                .id(gpui::SharedString::from(id))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(22.))
                .rounded(px(4.))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(div().w(px(14.)).child(if on { "☑" } else { "☐" }))
                .child(label)
        };
        let items: Vec<gpui::AnyElement> = if self.filter == Filter::All {
            GROUPS
                .iter()
                .map(|g| {
                    let g: &'static str = g;
                    entry(
                        format!("ew-group-{g}"),
                        g.to_owned(),
                        !self.hidden_groups.contains(g),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.hidden_groups.remove(g) {
                            this.hidden_groups.insert(g);
                        }
                        this.search(cx);
                        cx.notify();
                    }))
                    .into_any_element()
                })
                .collect()
        } else {
            // File types of the hits and those already turned off.
            let mut types: BTreeSet<String> = self.hidden_types.clone();
            for row in &self.results.rows {
                if let Some(path) = row.path() {
                    types.insert(
                        Path::new(path)
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default(),
                    );
                }
            }
            types
                .into_iter()
                .map(|t| {
                    let label = if t.is_empty() {
                        "(no extension)".to_owned()
                    } else {
                        format!("*.{t}")
                    };
                    let on = !self.hidden_types.contains(&t);
                    let key = t.clone();
                    entry(format!("ew-type-{t}"), label, on)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.hidden_types.remove(&key) {
                                this.hidden_types.insert(key.clone());
                            }
                            this.searched = None;
                            this.search(cx);
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect()
        };
        div()
            .id("ew-filter-menu")
            .absolute()
            .top(px(34.))
            .right(px(8.))
            .w(px(200.))
            .p_1()
            .bg(c(p.popup))
            .border_1()
            .border_color(c(p.popup_border))
            .rounded(px(6.))
            .shadow_lg()
            .flex()
            .flex_col()
            .children(items)
            .into_any_element()
    }
}

impl Render for EverywhereView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let rows = self.results.rows.clone();
        let selected = self.results.selected;
        let row_count = rows.len();
        let list = uniform_list(
            "everywhere",
            rows.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let on = Some(i) == selected;
                        // Grey hints stay readable on the selection (hand test 2026-11-01).
                        let hint = c(if on { p.ui_text } else { p.ui_muted });
                        let row = div()
                            .id(("ew", i))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(ROW))
                            .px_3()
                            .overflow_hidden()
                            .whitespace_nowrap();
                        let hit = |d: gpui::Stateful<gpui::Div>| {
                            d.mx_2()
                                .rounded(px(4.))
                                .when(on, |d| d.bg(c(p.tree_selection)))
                                .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                    this.results.selected = Some(i);
                                    if e.click_count() == 2 {
                                        this.choose(false, cx);
                                    }
                                    cx.notify();
                                }))
                        };
                        let file_name = |path: &str| {
                            Path::new(path)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        };
                        match &rows[i] {
                            Row::Header(title) => row
                                .mx_2()
                                .mt_1()
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(c(p.ui_muted))
                                .child(*title),
                            Row::Calculator(value) => hit(row
                                .child(div().flex_none().child(format!("= {value}")))
                                .child(div().flex_1())
                                .child(div().flex_none().text_color(hint).child("Calculator"))),
                            Row::File(f) => {
                                let path = Path::new(&f.path);
                                let name = file_name(&f.path);
                                let dir = path
                                    .parent()
                                    .map(|d| d.display().to_string())
                                    .unwrap_or_default();
                                hit(row
                                    .child(icon(idecraft_ui::icons::file_icon(&name)))
                                    .child(Self::marked(&name, &f.positions, on))
                                    .child(div().flex_none().text_color(hint).child(dir))
                                    .child(div().flex_1()))
                            }
                            Row::Class(s) | Row::Symbol(s) => {
                                let name = file_name(&s.path);
                                // Java packages and Python modules read "of …"; other
                                // containers are the file's path, which the right column shows.
                                let container = (!s.container.contains('/')
                                    && !s.container.is_empty())
                                .then(|| format!("of {}", s.container));
                                hit(row
                                    .child(icon(idecraft_ui::icons::symbol_icon(&s.kind)))
                                    // The parameters follow the name without a gap.
                                    .child(
                                        div()
                                            .flex_none()
                                            .flex()
                                            .child(Self::marked(&s.name, &s.positions, on))
                                            .children(s.params.clone()),
                                    )
                                    .children(
                                        container
                                            .map(|t| div().flex_none().text_color(hint).child(t)),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(hint)
                                            .child(format!("{name}:{}", s.line + 1)),
                                    ))
                            }
                            Row::Action(a) => {
                                let group = if a.id.starts_with(SETTINGS_PREFIX) {
                                    Some("Settings".to_owned())
                                } else {
                                    a.group.clone()
                                };
                                hit(row
                                    .child(Self::marked(&a.name, &a.positions, on))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_color(hint)
                                            .child(group.unwrap_or_default()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(hint)
                                            .child(a.shortcut.clone().unwrap_or_default()),
                                    ))
                            }
                            Row::Endpoint(e) => {
                                let at = &e.endpoint;
                                let place = format!(
                                    "{}:{}",
                                    file_name(&at.file.to_string_lossy()),
                                    at.line + 1
                                );
                                // Methods, path and handler as the Endpoints tool window
                                // shows them (SPEC-0103 v4 behaviour 7).
                                hit(row
                                    .child(
                                        div()
                                            .min_w(px(64.))
                                            .flex_none()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(at.method_label()),
                                    )
                                    .child(Self::marked(&at.path, &e.path_positions, on))
                                    .child(div().flex_none().text_color(hint).child(Self::marked(
                                        &at.handler,
                                        &e.handler_positions,
                                        on,
                                    )))
                                    .when(at.kind == EndpointKind::Client, |d| {
                                        d.child(div().flex_none().text_color(hint).child("client"))
                                    })
                                    .child(div().flex_1())
                                    .child(div().flex_none().text_color(hint).child(place)))
                            }
                            Row::Text(t) => {
                                let name = file_name(&t.path);
                                hit(row
                                    .child(icon(idecraft_ui::icons::file_icon(&name)))
                                    .child(
                                        div().flex_1().overflow_hidden().child(Self::text_line(t)),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(hint)
                                            .child(format!("{name} {}", t.line + 1)),
                                    ))
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .h(px((row_count as f32 * ROW + 4.).min(360.)));
        let tab = |f: Filter, cx: &mut Context<Self>| {
            let on = self.filter == f;
            div()
                .id(f.label())
                .px_2()
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tab_selected)).text_color(c(p.ui_text)))
                .when(!on, |d| d.text_color(c(p.ui_muted)))
                .hover(|s| s.bg(c(p.toolbar_hover)))
                .child(f.label())
                .on_click(cx.listener(move |this, _, _, cx| this.set_filter(f, cx)))
        };
        let tool = |id: &'static str, key: &'static str, on: bool| {
            div()
                .id(id)
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tab_selected)))
                .hover(|s| s.bg(c(p.toolbar_hover)))
                .child(icon(key))
        };
        let toggle = |id: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .px_1()
                .h(px(20.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_size(px(12.))
                .when(on, |d| d.bg(c(p.tab_selected)).text_color(c(p.ui_text)))
                .when(!on, |d| d.text_color(c(p.ui_muted)))
                .hover(|s| s.bg(c(p.toolbar_hover)))
                .child(label)
        };
        let query_text = self.query.read(cx).state.buffer.text();
        let query_lines = query_text.split('\n').count().clamp(1, 4) as f32;
        let empty_query = query_text.is_empty();
        let text_tab = self.filter == Filter::Text;
        let header = div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .pt_2()
            .children(Filter::ALL.map(|f| tab(f, cx)))
            .child(div().flex_1())
            .when(self.filter.has_scope(), |d| {
                d.child(
                    div()
                        .id("ew-scope")
                        .tip(declared!(FIND_BAR, "ew-scope"))
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_1()
                        .rounded(px(4.))
                        .text_size(px(12.))
                        .text_color(c(p.ui_muted))
                        .hover(|s| s.bg(c(p.toolbar_hover)))
                        .child(if self.everything { "☑" } else { "☐" })
                        .child("Include non-project items")
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_everything(cx))),
                )
            })
            .child(
                tool("ew-preview", "idecraft/preview", self.preview)
                    .tip(declared!(FIND_BAR, "ew-preview"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.preview = !this.preview;
                        cx.notify();
                    })),
            )
            .when(self.filter != Filter::Actions, |d| {
                d.child(
                    tool("ew-filter", "idecraft/filter", self.filter_menu)
                        .tip(declared!(FIND_BAR, "ew-filter"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filter_menu = !this.filter_menu;
                            cx.notify();
                        })),
                )
            })
            .when(text_tab, |d| {
                d.child(
                    tool("ew-find-window", "idecraft/openInFind", false)
                        .tip(declared!(FIND_BAR, "ew-find-window"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_in_find_window(cx))),
                )
            });
        let options = self.text_options;
        let field = div()
            .m_2()
            .min_h(px(28.))
            .h(px(10. + 18. * query_lines))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.focus))
            .child(icon("general/search"))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h(px(18. * query_lines))
                    .overflow_hidden()
                    .child(self.query.clone())
                    .when(empty_query, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .left(px(4.))
                                .h(px(18.))
                                .flex()
                                .items_center()
                                .text_color(c(p.ui_dim))
                                .child(if text_tab {
                                    "Text in project files"
                                } else if self.filter == Filter::Endpoints {
                                    "Path, handler, or a method and a path: GET /users"
                                } else {
                                    "Name, Name:line, *part or package.Name"
                                }),
                        )
                    }),
            )
            .when(text_tab, |d| {
                d.child(
                    toggle("ew-case", "Cc", options.match_case)
                        .tip(declared!(FIND_BAR, "ew-case"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.text_options.match_case = !this.text_options.match_case;
                            this.search(cx);
                        })),
                )
                .child(
                    toggle("ew-words", "W", options.whole_words)
                        .tip(declared!(FIND_BAR, "ew-words"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.text_options.whole_words = !this.text_options.whole_words;
                            this.search(cx);
                        })),
                )
                .child(
                    toggle("ew-regex", ".*", options.regex)
                        .tip(declared!(FIND_BAR, "ew-regex"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.text_options.regex = !this.text_options.regex;
                            this.search(cx);
                        })),
                )
            });
        let endpoints_tab = self.filter == Filter::Endpoints;
        // The Endpoints tab also says so when the project has no endpoint at all.
        let no_endpoints = endpoints_tab && self.endpoints.as_ref().is_some_and(|e| e.is_empty());
        let nothing = row_count == 0 && (!empty_query || no_endpoints);
        let preview = if self.preview {
            self.ask_host_preview(cx);
            self.excerpt()
        } else {
            None
        };
        let footer = self.footer();
        let path_row = self
            .results
            .selected_row()
            .is_some_and(|r| r.path().is_some());
        div()
            .id("everywhere-popup")
            .occlude()
            .relative()
            // Wide enough for the seven tabs beside the scope checkbox and the tools.
            .w(px(740.))
            .flex()
            .flex_col()
            .bg(c(p.popup))
            .border_1()
            .border_color(c(p.popup_border))
            .rounded(px(10.))
            .shadow_lg()
            .overflow_hidden()
            .capture_key_down(cx.listener(Self::on_key))
            .child(header)
            .child(field)
            .when(row_count > 0, |d| {
                d.child(
                    div()
                        .border_t_1()
                        .border_color(c(p.separator))
                        .py_1()
                        .child(list),
                )
            })
            .when(nothing, |d| {
                d.child(
                    div()
                        .border_t_1()
                        .border_color(c(p.separator))
                        .px_3()
                        .py_2()
                        .text_color(c(p.ui_muted))
                        .child(if no_endpoints {
                            "No endpoints found in this project".to_owned()
                        } else if endpoints_tab {
                            "No endpoint found".to_owned()
                        } else if self.filter == Filter::Text || self.everything {
                            "Nothing found".to_owned()
                        } else {
                            "Nothing found. Include non-project items or search the Text tab."
                                .to_owned()
                        }),
                )
            })
            // What the files left out, in one line (v6 behaviour 13).
            .children(
                self.not_listed
                    .clone()
                    .filter(|_| !self.everything && self.filter.has_scope())
                    .map(|line| {
                        div()
                            .id("ew-not-listed")
                            .border_t_1()
                            .border_color(c(p.separator))
                            .px_3()
                            .py_1()
                            .text_size(px(12.))
                            .text_color(c(p.ui_muted))
                            .child(line)
                    }),
            )
            .children(preview.map(|lines| {
                div()
                    .id("ew-preview-pane")
                    .border_t_1()
                    .border_color(c(p.separator))
                    .bg(gpui::rgb(p.background))
                    .py_1()
                    .child(crate::code_preview::render(&lines))
            }))
            .when(row_count > 0, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .h(px(24.))
                        .border_t_1()
                        .border_color(c(p.separator))
                        .text_size(px(11.))
                        .text_color(c(p.ui_muted))
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(footer),
                        )
                        .when(path_row, |d| {
                            d.child(
                                div()
                                    .id("ew-right-split")
                                    .tip(declared!(FIND_BAR, "ew-right-split"))
                                    .text_color(c(p.link))
                                    .child("Open in Right Split ⇧⏎")
                                    .on_click(cx.listener(|this, _, _, cx| this.choose(true, cx))),
                            )
                        }),
                )
            })
            .when(self.filter_menu, |d| d.child(self.filter_menu_view(cx)))
    }
}

/// Where the focus goes when Search Everywhere closes or runs an action (`SPEC-0067` v10
/// behaviour 21): back to the tool window that had it when the popup opened, if that window is
/// still shown, so an action like Stretch to Top acts on it; otherwise to the editor.
pub fn focus_back(
    before: Option<idecraft_ui::tool_windows::ToolWindow>,
    shown: impl Fn(idecraft_ui::tool_windows::ToolWindow) -> bool,
) -> Option<idecraft_ui::tool_windows::ToolWindow> {
    before.filter(|w| shown(*w))
}

#[cfg(test)]
mod focus_back_tests {
    use super::focus_back;
    use idecraft_ui::tool_windows::ToolWindow;

    /// SPEC-0067 v10 AK-11: the tool window that had the focus gets it back; a window hidden in
    /// the meantime, or none, leaves it to the editor.
    #[test]
    fn the_focus_goes_back_to_the_tool_window_that_had_it() {
        let shown = |w: ToolWindow| w == ToolWindow::Terminal;
        assert_eq!(
            focus_back(Some(ToolWindow::Terminal), shown),
            Some(ToolWindow::Terminal)
        );
        assert_eq!(focus_back(Some(ToolWindow::Database), shown), None);
        assert_eq!(focus_back(None, shown), None);
    }
}
