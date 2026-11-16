//! The Find in Files panel (`SPEC-0066`): query, options, file mask and the matches of the workspace.

use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use idecraft_core::{Query, Workspace};

use crate::EditorView;
use crate::find_bar::toggle_button;
use crate::find_in_files_model::{ResultList, ResultRow, Scope, SearchRequest};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::Tipped;
use crate::workbench_view::c;
use idecraft_languages::syntax::SearchContext;

/// Most matches listed (SPEC-0029 behaviour 5).
const MAX_MATCHES: usize = 1000;

/// What the panel asks the workbench to do.
pub enum FindInFilesEvent {
    /// Open a file (absolute path) and select a char range in it; `true` keeps the panel open
    /// (pinned, `SPEC-0066` v3 behaviour 10).
    Open(PathBuf, Range<usize>, bool),
    /// Close the panel.
    Close,
    /// Replace the matches of `request` — all of them, or only the match `only` (a
    /// workspace-relative file and the match's char range) — with `replacement` (`SPEC-0066` v2).
    /// `request` is what the result list is searched with: query, file mask, scope, filter and
    /// the toggle, so a replacement changes what the list shows (v5 behaviour 17).
    Replace {
        /// What the list shows the matches of.
        request: SearchRequest,
        /// Replacement text, with `$1` groups for regular expressions.
        replacement: String,
        /// Only this match.
        only: Option<(PathBuf, Range<usize>)>,
    },
}

/// The scope tabs of the panel (`SPEC-0066` v3 behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeTab {
    /// All project files.
    Project,
    /// A folder.
    Directory,
    /// The open files.
    OpenFiles,
}

/// Query and results of a search over the workspace files.
pub struct FindInFilesView {
    root: PathBuf,
    query: Entity<EditorView>,
    mask: Entity<EditorView>,
    /// Folder of the Directory scope, workspace-relative.
    directory: Entity<EditorView>,
    scope_tab: ScopeTab,
    /// Files open in editors (workspace-relative), for the Open Files scope.
    open_files: Vec<PathBuf>,
    /// Comments and string literals filter (v3 behaviour 11).
    context: SearchContext,
    /// Also search and replace in libraries and ignored files (v4 behaviour 15).
    include_non_project: bool,
    context_menu: bool,
    /// Opening a match keeps the panel open (v3 behaviour 10).
    pinned: bool,
    /// The preview of the selected match (v3 behaviour 9).
    preview: bool,
    preview_cache: Option<(PathBuf, usize, Vec<crate::code_preview::ExcerptLine>)>,
    /// The "Replace with" field and whether it is shown (Replace in Files).
    replacement: Entity<EditorView>,
    replacing: bool,
    match_case: bool,
    whole_words: bool,
    regex: bool,
    list: ResultList,
    error: Option<String>,
    searching: bool,
    /// The query or the options changed while a search ran.
    stale: bool,
    searched: Option<SearchRequest>,
    scroll: UniformListScrollHandle,
    _watches: Vec<Subscription>,
    /// Searches the host of a remote project instead of this machine (`SPEC-0318` v2 detail 5).
    remote: Option<crate::workbench_view::HostCall>,
}

impl EventEmitter<FindInFilesEvent> for FindInFilesView {}

/// The parameters of `search.text` for a request (`SPEC-0318` v8 behaviour 13, `SPEC-0066` v6
/// behaviour 18): query and options, the file mask, the toggle "Libraries and ignored files" as
/// `include_non_project`, the Directory scope as `folder` and the Open Files scope as `files`.
fn host_params(request: &SearchRequest) -> serde_json::Value {
    let query = &request.query;
    let mut params = serde_json::json!({
        "query": query.pattern, "regex": query.regex, "case_sensitive": query.match_case,
        "whole_words": query.whole_words, "max": MAX_MATCHES,
        "context": request.context.id(),
        "include_non_project": request.include_non_project,
    });
    let mask = request.mask.trim();
    if !mask.is_empty() {
        params["mask"] = serde_json::json!(mask);
    }
    match &request.scope {
        Scope::Project => {}
        Scope::Directory(d) => {
            let d = d.to_string_lossy().replace('\\', "/");
            let d = d.trim_matches('/');
            if !d.is_empty() {
                params["folder"] = serde_json::json!(d);
            }
        }
        // Only paths the host accepts: one it refuses would fail the whole search.
        Scope::OpenFiles(files) => {
            let files: Vec<String> = files
                .iter()
                .filter_map(|f| idecraft_session::paths::check_relative(&f.to_string_lossy()).ok())
                .filter(|f| f != ".")
                .collect();
            params["files"] = serde_json::json!(files);
        }
    }
    params
}

/// Finds the request's query in the host project through `search.text` (`SPEC-0318` behaviour
/// 13): the files with their relative paths, each match with its exact place (protocol detail 6),
/// and the host's line on what it did not search (v8).
fn host_search(
    host: &crate::workbench_view::HostCall,
    request: &SearchRequest,
) -> Result<(idecraft_core::WorkspaceSearch, Option<String>), String> {
    let v = host("search.text", host_params(request))?;
    let files = v["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let doc = idecraft_session::paths::check_relative(f["doc"].as_str()?).ok()?;
            let matches = f["matches"]
                .as_array()?
                .iter()
                .filter_map(|m| {
                    let start = m["start"].as_u64()? as usize;
                    Some(idecraft_core::Match {
                        start,
                        end: start + m["length"].as_u64().unwrap_or(0) as usize,
                        line: m["line"].as_u64()? as usize,
                        column: m["column"].as_u64().unwrap_or(0) as usize,
                        line_text: m["text"].as_str().unwrap_or_default().to_owned(),
                    })
                })
                .collect();
            Some(idecraft_core::FileMatches {
                path: PathBuf::from(doc),
                matches,
            })
        })
        .collect();
    let found = idecraft_core::WorkspaceSearch {
        files,
        skipped: v["skipped"].as_u64().unwrap_or(0) as usize,
        truncated: v["truncated"].as_bool() == Some(true),
        // The host says it in one line, below.
        not_searched: Vec::new(),
    };
    let not_searched = v["not_searched"]
        .as_str()
        .filter(|line| !line.is_empty())
        .map(str::to_owned);
    Ok((found, not_searched))
}

impl FindInFilesView {
    /// A panel searching the workspace at `root`.
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let query = cx.new(EditorView::field);
        let mask = cx.new(EditorView::field);
        let replacement = cx.new(EditorView::field);
        let directory = cx.new(EditorView::field);
        let watches = vec![
            cx.observe(&query, |this, _, cx| this.search(cx)),
            cx.observe(&mask, |this, _, cx| this.search(cx)),
            cx.observe(&directory, |this, _, cx| this.search(cx)),
        ];
        Self {
            root,
            query,
            mask,
            directory,
            scope_tab: ScopeTab::Project,
            open_files: Vec::new(),
            context: SearchContext::Anywhere,
            include_non_project: false,
            context_menu: false,
            pinned: false,
            preview: true,
            preview_cache: None,
            replacement,
            replacing: false,
            match_case: false,
            whole_words: false,
            regex: false,
            list: ResultList::default(),
            error: None,
            searching: false,
            stale: false,
            searched: None,
            scroll: UniformListScrollHandle::new(),
            _watches: watches,
            remote: None,
        }
    }

    /// Searches the host of a remote project from now on (`SPEC-0318` v2 detail 5).
    pub fn set_remote(&mut self, host: crate::workbench_view::HostCall, cx: &mut Context<Self>) {
        self.remote = Some(host);
        self.searched = None;
        self.search(cx);
    }

    /// Whether the panel searches a remote project's host.
    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// Focus handle of the query field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.query.read(cx).focus()
    }

    /// Puts `text` into the query with all of it selected (behaviour 1).
    pub fn set_query(&mut self, text: &str, cx: &mut Context<Self>) {
        self.query.update(cx, |q, cx| {
            q.set_text_selected(text, cx);
        });
    }

    /// Shows or hides the "Replace with" field (`ReplaceInPath` shows it, `FindInPath` hides it).
    pub fn set_replacing(&mut self, on: bool, cx: &mut Context<Self>) {
        self.replacing = on;
        cx.notify();
    }

    /// Whether the panel replaces.
    pub fn replacing(&self) -> bool {
        self.replacing
    }

    /// Puts `text` into the "Replace with" field.
    pub fn set_replacement(&mut self, text: &str, cx: &mut Context<Self>) {
        self.replacement.update(cx, |r, cx| r.set_text(text, cx));
    }

    /// Asks the workbench to replace all matches, or the selected one (behaviour 7).
    pub fn replace(&mut self, all: bool, cx: &mut Context<Self>) {
        let request = self.request(cx);
        if request.query.pattern.is_empty() {
            return;
        }
        let only = if all {
            None
        } else {
            match self.list.selected_hit() {
                Some(hit) => Some(hit),
                None => return,
            }
        };
        cx.emit(FindInFilesEvent::Replace {
            request,
            replacement: self.replacement.read(cx).state.buffer.text(),
            only,
        });
    }

    /// Whether libraries and ignored files are searched too (v4 behaviour 15).
    pub fn include_non_project(&self) -> bool {
        self.include_non_project
    }

    /// Searches libraries and ignored files too, or only the project's own sources.
    pub fn set_include_non_project(&mut self, on: bool, cx: &mut Context<Self>) {
        self.include_non_project = on;
        self.search(cx);
        cx.notify();
    }

    /// What the search left out as not the project's own, e.g. `2 folders not searched (libraries
    /// and ignored files)` (v4 behaviour 16); empty when nothing was left out.
    pub fn not_searched(&self) -> String {
        match &self.error {
            Some(_) => String::new(),
            None => self.list.not_searched.clone().unwrap_or_default(),
        }
    }

    /// Searches again after files changed (a replacement).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.searched = None;
        self.search(cx);
    }

    /// Text of the query field.
    pub fn query_text(&self, cx: &gpui::App) -> String {
        self.query.read(cx).state.buffer.text()
    }

    /// Summary of the results, e.g. `3 matches in 2 files` (behaviour 4).
    pub fn summary(&self) -> String {
        match &self.error {
            Some(e) => e.clone(),
            None if self.list.rows.is_empty() => String::new(),
            None => self.list.summary(),
        }
    }

    /// Painted sizes of the search, replacement and file mask fields.
    pub fn field_sizes(&self, cx: &gpui::App) -> [(f32, f32); 3] {
        [
            self.query.read(cx).painted_size(),
            self.replacement.read(cx).painted_size(),
            self.mask.read(cx).painted_size(),
        ]
    }

    /// The selected match, workspace-relative.
    pub fn selected(&self) -> Option<(PathBuf, Range<usize>)> {
        self.list.selected_hit()
    }

    /// What the panel searches for and where, as its fields and options stand: the one request
    /// behind the result list and behind a replacement (v5 behaviour 17).
    fn request(&self, cx: &gpui::App) -> SearchRequest {
        SearchRequest {
            query: Query {
                pattern: self.query.read(cx).state.buffer.text(),
                regex: self.regex,
                match_case: self.match_case,
                whole_words: self.whole_words,
            },
            mask: self.mask.read(cx).state.buffer.text(),
            scope: self.scope(cx),
            context: self.context,
            include_non_project: self.include_non_project,
        }
    }

    /// Sets the options (from Search Everywhere's Text tab, `SPEC-0067` v5 behaviour 17).
    pub fn set_options(
        &mut self,
        match_case: bool,
        whole_words: bool,
        regex: bool,
        cx: &mut Context<Self>,
    ) {
        (self.match_case, self.whole_words, self.regex) = (match_case, whole_words, regex);
        self.search(cx);
        cx.notify();
    }

    /// Whether opening a match keeps the panel open (v3 behaviour 10).
    pub fn pinned(&self) -> bool {
        self.pinned
    }

    /// Pins or unpins the panel.
    pub fn set_pinned(&mut self, on: bool, cx: &mut Context<Self>) {
        self.pinned = on;
        cx.notify();
    }

    /// The files open in editors, for the Open Files scope; `current_dir` (workspace-relative)
    /// fills an empty Directory field (v3 behaviour 8).
    pub fn set_open_files(
        &mut self,
        files: Vec<PathBuf>,
        current_dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.open_files = files;
        if self.directory.read(cx).state.buffer.is_empty()
            && let Some(dir) = current_dir
        {
            let text = dir.to_string_lossy().into_owned();
            self.directory.update(cx, |d, cx| d.set_text(&text, cx));
        }
    }

    /// Searches below `dir` (a workspace-relative folder or file): the Directory scope.
    pub fn set_directory(&mut self, dir: &str, cx: &mut Context<Self>) {
        self.directory.update(cx, |d, cx| d.set_text(dir, cx));
        self.set_scope(ScopeTab::Directory, cx);
    }

    /// Chooses the scope tab (v3 behaviour 8).
    pub fn set_scope(&mut self, tab: ScopeTab, cx: &mut Context<Self>) {
        self.scope_tab = tab;
        self.search(cx);
        cx.notify();
    }

    /// The comments/strings filter (v3 behaviour 11).
    pub fn set_context(&mut self, context: SearchContext, cx: &mut Context<Self>) {
        self.context = context;
        self.context_menu = false;
        self.search(cx);
        cx.notify();
    }

    /// Shows or hides the preview (v3 behaviour 9).
    pub fn set_preview(&mut self, on: bool, cx: &mut Context<Self>) {
        self.preview = on;
        cx.notify();
    }

    /// The preview of the selected match: its file, line and lines.
    pub fn preview_lines(&mut self) -> Option<Vec<crate::code_preview::ExcerptLine>> {
        if !self.preview {
            return None;
        }
        let (path, line, within) = match self.list.rows.get(self.list.selected?)? {
            ResultRow::Hit {
                path, line, text, ..
            } => (path.clone(), *line, text.clone()),
            _ => return None,
        };
        if let Some((p, l, lines)) = &self.preview_cache
            && *p == path
            && *l == line
        {
            return Some(lines.clone());
        }
        if self.remote.is_some() {
            // The match's line as the host sent it, at its line number (v2 detail 5).
            let mark = match self.list.rows.get(self.list.selected?)? {
                ResultRow::Hit { within: w, .. } => Some(w.clone()),
                _ => None,
            };
            let text = format!("{}{within}", "\n".repeat(line));
            let lines = crate::code_preview::excerpt(&path, &text, line, mark, 0, 0);
            self.preview_cache = Some((path, line, lines.clone()));
            return Some(lines);
        }
        let text = std::fs::read_to_string(self.root.join(&path)).ok()?;
        let full = text.split('\n').nth(line).unwrap_or_default();
        let cut = full.chars().count() - full.trim_start().chars().count();
        let mark = match self.list.rows.get(self.list.selected?)? {
            ResultRow::Hit { within: w, .. } => Some(w.start + cut..w.end + cut),
            _ => None,
        };
        let _ = within;
        let lines = crate::code_preview::excerpt(&path, &text, line, mark, 6, 6);
        self.preview_cache = Some((path, line, lines.clone()));
        Some(lines)
    }

    fn scope(&self, cx: &gpui::App) -> Scope {
        match self.scope_tab {
            ScopeTab::Project => Scope::Project,
            ScopeTab::Directory => Scope::Directory(PathBuf::from(
                self.directory.read(cx).state.buffer.text().trim(),
            )),
            ScopeTab::OpenFiles => Scope::OpenFiles(self.open_files.clone()),
        }
    }

    /// Inserts a line break into the query at the caret: a multi-line search (v3 behaviour 12).
    fn new_line(&mut self, cx: &mut Context<Self>) {
        self.query.update(cx, |q, cx| {
            let r = q.state.carets.primary().range();
            q.replace_ranges(&[(r, "\n".into())], cx);
        });
    }

    /// Searches the workspace in the background when the query, mask or options changed
    /// (behaviour 2).
    fn search(&mut self, cx: &mut Context<Self>) {
        let current = self.request(cx);
        if self.searched.as_ref() == Some(&current) {
            return;
        }
        if self.searching {
            self.stale = true;
            return;
        }
        self.searching = true;
        self.stale = false;
        self.searched = Some(current.clone());
        let root = self.root.clone();
        let remote = self.remote.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if current.query.pattern.is_empty() {
                        return Ok(Default::default());
                    }
                    if let Some(host) = remote {
                        // The comments/strings filter runs on the host (SPEC-0318 v5 behaviour
                        // 40), and so does the rule on the project's own sources (v8 behaviour 13).
                        let (mut found, not_searched) = host_search(&host, &current)?;
                        found.files.retain(|f| current.scope.contains(&f.path));
                        return Ok((found, not_searched));
                    }
                    let workspace = Workspace::open(&root).map_err(|e| e.to_string())?;
                    // The same request a replacement plans with (SPEC-0066 v5 behaviour 17).
                    current
                        .list(&workspace, MAX_MATCHES)
                        .map(|found| (found, None))
                        .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.searching = false;
                this.preview_cache = None;
                match result {
                    Ok((search, host_line)) => {
                        this.list = ResultList::from_search(&search);
                        // A remote project's host says what it left out (v6 behaviour 18).
                        if host_line.is_some() {
                            this.list.not_searched = host_line;
                        }
                        this.error = None;
                    }
                    Err(e) => {
                        this.list = ResultList::default();
                        this.error = Some(e);
                    }
                }
                if this.stale {
                    this.search(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some((path, range)) = self.list.selected_hit() {
            cx.emit(FindInFilesEvent::Open(
                self.root.join(path),
                range,
                self.pinned,
            ));
        }
    }

    /// Up/Down move through the matches, Enter opens one, Escape closes (behaviours 3 and 5).
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // Tab and Shift+Tab move between search text, replacement and file mask.
        if k.key == "tab" && !k.modifiers.control && !k.modifiers.alt && !k.modifiers.platform {
            let mut fields = vec![self.query.read(cx).focus()];
            if self.replacing {
                fields.push(self.replacement.read(cx).focus());
            }
            fields.push(self.mask.read(cx).focus());
            let at = fields
                .iter()
                .position(|f| f.is_focused(window))
                .unwrap_or(0);
            let n = fields.len();
            let next = if k.modifiers.shift {
                (at + n - 1) % n
            } else {
                (at + 1) % n
            };
            window.focus(&fields[next]);
            cx.stop_propagation();
            return;
        }
        // ⌘↩ pins the results and opens the match (v3 behaviour 10).
        if k.key == "enter" && k.modifiers.platform && !k.modifiers.shift {
            self.pinned = true;
            self.open_selected(cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // The first and the last match (`SPEC-0275` v19 behaviour 47): the chords of the keymap;
        // Home and End belong to the text fields of the popup.
        if let Some(to) = crate::list_nav::edge_among(
            k,
            self.list.rows.len(),
            crate::list_nav::Keys::CHORDS,
            |i| matches!(self.list.rows[i], ResultRow::Hit { .. }),
        ) {
            self.list.selected = Some(to);
            self.scroll.scroll_to_item(to, gpui::ScrollStrategy::Center);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift {
            return;
        }
        match k.key.as_str() {
            "up" | "down" => {
                self.list.step(k.key == "down");
                if let Some(i) = self.list.selected {
                    self.scroll.scroll_to_item(i, gpui::ScrollStrategy::Center);
                }
            }
            "enter" => self.open_selected(cx),
            "escape" => cx.emit(FindInFilesEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn flip(&mut self, which: u8, cx: &mut Context<Self>) {
        match which {
            0 => self.match_case = !self.match_case,
            1 => self.whole_words = !self.whole_words,
            _ => self.regex = !self.regex,
        }
        self.search(cx);
        cx.notify();
    }
}

impl Render for FindInFilesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let rows = self.list.rows.clone();
        let no_rows = rows.is_empty();
        let query_empty = self.query.read(cx).state.buffer.is_empty();
        let replacement_empty = self.replacement.read(cx).state.buffer.is_empty();
        let mask_empty = self.mask.read(cx).state.buffer.is_empty();
        let focused = |f: &Entity<EditorView>| f.read(cx).focus().is_focused(window);
        let (query_on, replacement_on, mask_on) = (
            focused(&self.query),
            focused(&self.replacement),
            focused(&self.mask),
        );
        // Each field is a visible box with a placeholder that says what goes in it; the focused
        // one has the accent border (hand test 6, 2026-11-01).
        let preview = self.preview_lines();
        let query_entity = self.query.clone();
        let query_lines = self
            .query
            .read(cx)
            .state
            .buffer
            .text()
            .split('\n')
            .count()
            .clamp(1, 4) as f32;
        let field_box =
            |field: Entity<EditorView>, placeholder: &'static str, empty: bool, on: bool| {
                let lines = if field == query_entity {
                    query_lines
                } else {
                    1.
                };
                div()
                    .h(px(4. + 24. * lines))
                    .flex()
                    .items_center()
                    .px_1()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(if on { c(p.focus) } else { c(p.separator) })
                    .bg(gpui::rgb(p.background))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(px(24. * lines))
                            .overflow_hidden()
                            .child(field)
                            .when(empty, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left(px(4.))
                                        .h(px(24.))
                                        .flex()
                                        .items_center()
                                        .text_color(c(p.ui_dim))
                                        .child(placeholder),
                                )
                            }),
                    )
            };
        let label = |text: &'static str| {
            div()
                .w(px(64.))
                .flex_none()
                .text_color(c(p.ui_muted))
                .child(text)
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
        let context_on = self.context != SearchContext::Anywhere;
        let scope_tab = |tab: ScopeTab, label: &'static str, cx: &mut Context<Self>| {
            let on = self.scope_tab == tab;
            div()
                .id(label)
                .px_2()
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tab_selected)).text_color(c(p.ui_text)))
                .when(!on, |d| d.text_color(c(p.ui_muted)))
                .hover(|s| s.bg(c(p.toolbar_hover)))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_scope(tab, cx)))
        };
        let directory_empty = self.directory.read(cx).state.buffer.is_empty();
        let directory_on = focused(&self.directory);
        let scope_row = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .pb_2()
            .child(label("In"))
            .child(scope_tab(ScopeTab::Project, "Project", cx))
            .child(scope_tab(ScopeTab::Directory, "Directory", cx))
            .child(scope_tab(ScopeTab::OpenFiles, "Open Files", cx))
            .when(self.scope_tab == ScopeTab::Directory, |d| {
                d.child(
                    field_box(
                        self.directory.clone(),
                        "Folder, relative to the project",
                        directory_empty,
                        directory_on,
                    )
                    .w(px(260.))
                    .flex_none(),
                )
            })
            .when(context_on, |d| {
                d.child(
                    div()
                        .text_color(c(p.ui_muted))
                        .child(format!("· {}", self.context.label())),
                )
            })
            // For a remote project too: its host takes the toggle (v6 behaviour 18).
            .map(|d| {
                let on = self.include_non_project;
                d.child(div().flex_1()).child(
                    div()
                        .id("fif-include-non-project")
                        .px_2()
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .when(on, |d| d.bg(c(p.tab_selected)).text_color(c(p.ui_text)))
                        .when(!on, |d| d.text_color(c(p.ui_muted)))
                        .hover(|s| s.bg(c(p.toolbar_hover)))
                        .child("Libraries and ignored files")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let on = !this.include_non_project;
                            this.set_include_non_project(on, cx);
                        })),
                )
            });
        let context_menu = self.context_menu.then(|| {
            div()
                .id("fif-context-menu")
                .absolute()
                .top(px(30.))
                .right(px(40.))
                .w(px(260.))
                .p_1()
                .bg(c(p.popup))
                .border_1()
                .border_color(c(p.popup_border))
                .rounded(px(6.))
                .shadow_lg()
                .flex()
                .flex_col()
                .children(SearchContext::ALL.map(|ctx| {
                    let on = ctx == self.context;
                    div()
                        .id(ctx.label())
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .h(px(22.))
                        .rounded(px(4.))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(
                            div()
                                .w(px(12.))
                                .flex_none()
                                .child(if on { "✓" } else { "" }),
                        )
                        .child(ctx.label())
                        .on_click(cx.listener(move |this, _, _, cx| this.set_context(ctx, cx)))
                }))
        });
        let selected = self.list.selected;
        let hit_bg = p
            .scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
            .unwrap_or(p.selection);
        let list = uniform_list(
            "find-in-files",
            rows.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let row = div()
                            .id(("fif", i))
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(22.))
                            .w_full()
                            .px_3()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .when(Some(i) == selected, |d| d.bg(c(p.tree_selection)))
                            .when(Some(i) != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))));
                        match &rows[i] {
                            ResultRow::File { path, count } => {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                let dir = path
                                    .parent()
                                    .map(|d| d.display().to_string())
                                    .unwrap_or_default();
                                row.child(icon(idecraft_ui::icons::file_icon(&name)))
                                    .child(name)
                                    .child(div().text_color(c(p.ui_dim)).child(dir))
                                    .child(
                                        div().text_color(c(p.ui_muted)).child(format!("{count}")),
                                    )
                            }
                            ResultRow::Hit {
                                line, text, within, ..
                            } => {
                                let chars: Vec<char> = text.chars().collect();
                                let part = |r: Range<usize>| -> String {
                                    chars[r.start.min(chars.len())..r.end.min(chars.len())]
                                        .iter()
                                        .collect()
                                };
                                row.pl(px(28.))
                                    .child(
                                        div()
                                            .w(px(40.))
                                            .flex_none()
                                            // Readable on the selection too (hand test 6).
                                            .text_color(if Some(i) == selected {
                                                c(p.ui_text)
                                            } else {
                                                c(p.ui_dim)
                                            })
                                            .child(format!("{}", line + 1)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .font_family(crate::fonts::editor_family())
                                            .child(part(0..within.start))
                                            .child(
                                                div()
                                                    .bg(gpui::rgb(hit_bg))
                                                    .child(part(within.clone())),
                                            )
                                            .child(part(within.end..chars.len())),
                                    )
                                    .on_click(cx.listener(
                                        move |this, e: &gpui::ClickEvent, _, cx| {
                                            this.list.selected = Some(i);
                                            if e.click_count() == 2 {
                                                this.open_selected(cx);
                                            }
                                            cx.notify();
                                        },
                                    ))
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .flex_1();
        let failed = self.error.is_some();
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title(if self.replacing {
                        "Replace in Files"
                    } else {
                        "Find in Files"
                    }))
                    .child(
                        div()
                            .text_color(if failed {
                                c(p.error_text)
                            } else {
                                c(p.ui_muted)
                            })
                            .child(self.summary()),
                    )
                    // What was left out as not the project's own (v4 behaviour 16).
                    .when(!self.not_searched().is_empty(), |d| {
                        d.child(
                            div()
                                .text_color(c(p.ui_muted))
                                .child(format!("· {}", self.not_searched())),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        tool("fif-filter", "idecraft/filter", self.context_menu || context_on)
                            .tip(crate::tips::declared!(FIND_BAR, "fif-filter"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.context_menu = !this.context_menu;
                                cx.notify();
                            })),
                    )
                    .child(
                        tool("fif-preview", "idecraft/preview", self.preview)
                            .tip(crate::tips::declared!(FIND_BAR, "fif-preview"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preview = !this.preview;
                                cx.notify();
                            })),
                    )
                    .child(
                        tool("fif-pin", "idecraft/pin", self.pinned)
                            .tip(crate::tips::declared!(FIND_BAR, "fif-pin"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pinned = !this.pinned;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("fif-close")
                            .tip(crate::tips::declared!(FIND_BAR, "fif-close"))
                            .size(px(16.))
                            .child(icon("general/closeSmall"))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(FindInFilesEvent::Close))),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .pb_2()
                    .child(label("Find"))
                    .child(
                        field_box(
                            self.query.clone(),
                            "Text to find in all files of the project",
                            query_empty,
                            query_on,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        tool("fif-newline", "idecraft/newLine", false)
                            .tip(crate::tips::declared!(FIND_BAR, "fif-newline"))
                            .on_click(cx.listener(|this, _, _, cx| this.new_line(cx))),
                    )
                    .child(toggle_button(
                        crate::tips::declared!(FIND_BAR, "fif-case"),
                        "Match case",
                        self.match_case,
                        cx,
                        |this: &mut Self, cx| this.flip(0, cx),
                    ))
                    .child(toggle_button(
                        crate::tips::declared!(FIND_BAR, "fif-words"),
                        "Words",
                        self.whole_words,
                        cx,
                        |this: &mut Self, cx| this.flip(1, cx),
                    ))
                    .child(toggle_button(
                        crate::tips::declared!(FIND_BAR, "fif-regex"),
                        "Regex",
                        self.regex,
                        cx,
                        |this: &mut Self, cx| this.flip(2, cx),
                    ))
                    .child(
                        field_box(self.mask.clone(), "Files, e.g. *.cpp", mask_empty, mask_on)
                            .w(px(150.))
                            .flex_none(),
                    ),
            )
            .child(scope_row)
            .when(self.replacing, |d| {
                let button = |id: &'static str, label: &'static str, all: bool| {
                    div()
                        .id(id)
                        .px_3()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c(p.separator))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(label)
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.replace(all, cx)
                        }))
                };
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .pb_2()
                        .child(label("Replace"))
                        .child(
                            field_box(
                                self.replacement.clone(),
                                "Replacement text",
                                replacement_empty,
                                replacement_on,
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(button("fif-replace", "Replace", false))
                        .child(button("fif-replace-all", "Replace All", true)),
                )
                .child(
                    div()
                        .px_3()
                        .pb_2()
                        .pl(px(84.))
                        .text_xs()
                        .text_color(c(p.ui_dim))
                        .child(
                            "Replace changes the selected match, Replace All every match. Open files change in \
                             the editor (unsaved, ⌘Z undoes it), closed files are written to disk.",
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .when(no_rows, |d| {
                        d.child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(c(p.ui_dim))
                                .child(if query_empty {
                                    "Type what to find; the matches appear here, grouped by file. \
                                     Tab moves between the fields, ↑↓ and Enter open a match."
                                } else {
                                    "No matches in the project"
                                }),
                        )
                    })
                    .when(!no_rows, |d| d.child(div().flex_1().min_w_0().flex().child(list)))
                    .when(!no_rows, |d| {
                        d.children(preview.map(|lines| {
                            div()
                                .id("fif-preview-pane")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_hidden()
                                .border_l_1()
                                .border_color(c(p.separator))
                                .bg(gpui::rgb(p.background))
                                .py_1()
                                .child(crate::code_preview::render(&lines))
                        }))
                    }),
            )
            .children(context_menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(scope: Scope, include_non_project: bool) -> SearchRequest {
        SearchRequest {
            query: Query {
                pattern: "needle".into(),
                ..Query::default()
            },
            mask: " *.py ".into(),
            scope,
            context: SearchContext::Anywhere,
            include_non_project,
        }
    }

    // SPEC-0066/AK-8 (v6, behaviour 18): what a remote project's host is asked.
    #[test]
    fn the_host_request_carries_scope_and_toggle() {
        let project = host_params(&request(Scope::Project, false));
        assert_eq!(project["query"], "needle");
        assert_eq!(project["mask"], "*.py");
        assert_eq!(project["include_non_project"], false);
        assert!(project.get("folder").is_none() && project.get("files").is_none());
        let everything = host_params(&request(Scope::Project, true));
        assert_eq!(everything["include_non_project"], true);
        let directory = host_params(&request(Scope::Directory("/.venv/lib/".into()), false));
        assert_eq!(directory["folder"], ".venv/lib");
        assert!(directory.get("files").is_none());
        // The open files as the host takes them; a path it would refuse is not sent.
        let open = host_params(&request(
            Scope::OpenFiles(vec![
                "node_modules/y.js".into(),
                "../elsewhere.py".into(),
                "/etc/hosts".into(),
            ]),
            false,
        ));
        assert_eq!(open["files"], serde_json::json!(["node_modules/y.js"]));
        assert!(open.get("folder").is_none());
        let none = host_params(&request(Scope::OpenFiles(Vec::new()), false));
        assert_eq!(
            none["files"],
            serde_json::json!([]),
            "no open file, no match"
        );

        // The host's answer: matches and its line on what it left out.
        let host: crate::workbench_view::HostCall = std::sync::Arc::new(|method, params| {
            assert_eq!(method, "search.text");
            assert_eq!(params["include_non_project"], false);
            Ok(serde_json::json!({
                "files": [{ "doc": "app.py", "unsaved": false, "matches": [
                    { "line": 0, "column": 0, "length": 6, "start": 0, "text": "needle = 1" },
                ]}],
                "matches": 1, "skipped": 0, "truncated": false,
                "not_searched": "2 folders and 1 file not searched (libraries and ignored files)",
            }))
        });
        let (found, line) = host_search(&host, &request(Scope::Project, false)).unwrap();
        assert_eq!(found.files.len(), 1);
        assert_eq!(found.files[0].path, PathBuf::from("app.py"));
        assert_eq!(
            line.as_deref(),
            Some("2 folders and 1 file not searched (libraries and ignored files)")
        );
    }
}
