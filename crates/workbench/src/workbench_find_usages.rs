//! Find Usages in the workbench: the Find tool window of `FindUsages` ⌥F7 (`SPEC-0293`), also for
//! the file alone with `FindUsagesInFile` ⌘F7 (v7 behaviour 19), and the
//! usages popup of `ShowUsages` ⌥⌘F7 (`SPEC-0073` v3), both on one bounded search that ends with a
//! result or a message and can be stopped.
//!
//! The tree, the preview and the search itself are in [`crate::find_usages_model`] and
//! [`idecraft_languages::find_usages`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{AppContext, Context, Entity, Window};
use idecraft_languages::find_usages::{DEADLINE, UsageSearch, UsageSource, find_usages_in};
use idecraft_languages::insight::Location;
use idecraft_languages::lsp::LspError;

use super::WorkbenchView;
use crate::find_usages_model::{Hit, UsageTree, hit};
use crate::nav_model::Place;
use crate::usages_model;
use crate::{FindUsagesEvent, FindUsagesView, UsagesView};

/// What a search is about: the place, the editor's text there and the symbol's name.
struct UsageQuery {
    origin: Place,
    text: Option<String>,
    symbol: String,
    /// Only the usages in the origin's file are shown (`FindUsagesInFile`, v7 behaviour 19).
    in_file: bool,
    /// The answer is known without a search: the usages of a configuration key, from the bean
    /// model (`SPEC-0115` v5 behaviour 12).
    ready: Option<UsageSearch>,
}

impl UsageQuery {
    /// The name of the file the search is restricted to, if it is.
    fn file_name(&self) -> Option<String> {
        self.in_file.then(|| {
            self.origin
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.origin.path.display().to_string())
        })
    }
}

/// Runs the bounded search on this thread (behaviour 2).
fn search_blocking(
    root: &Path,
    q: &UsageQuery,
    stop: Arc<AtomicBool>,
) -> Result<UsageSearch, LspError> {
    if let Some(search) = &q.ready {
        return Ok(search.clone());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| LspError::Io(e.to_string()))?;
    rt.block_on(find_usages_in(
        root,
        &q.origin.path,
        q.text.as_deref(),
        q.origin.line as u64,
        q.origin.column as u64,
        &q.symbol,
        DEADLINE,
        Some(stop),
    ))
}

fn place_of(l: &Location) -> Place {
    Place {
        path: l.path.clone(),
        line: l.range.start_line as usize,
        column: l.range.start_character as usize,
    }
}

fn length_of(l: &Location) -> usize {
    if l.range.end_line == l.range.start_line {
        l.range
            .end_character
            .saturating_sub(l.range.start_character) as usize
    } else {
        0
    }
}

/// The status line for a search without usages (behaviours 9, 10 and 19).
fn nothing_found(symbol: &str, source: UsageSource, in_file: Option<&str>) -> String {
    if in_file.is_some() {
        return crate::find_usages_model::no_usages(symbol, in_file);
    }
    match source {
        UsageSource::LanguageServer => crate::find_usages_model::no_usages(symbol, None),
        UsageSource::Text => format!(
            "No usages of {symbol} in project files (the language server did not answer within {} s and no text occurrence was found)",
            DEADLINE.as_secs()
        ),
    }
}

impl WorkbenchView {
    /// The symbol at the caret, where it is and the editor's text. Without a name at the caret
    /// there is nothing to search for: the status bar says so, nothing opens and no server is
    /// asked (`SPEC-0293` v8 behaviour 20).
    fn usage_query(&mut self, cx: &mut Context<Self>) -> Option<UsageQuery> {
        // A configuration key: the places that read it (SPEC-0115 v5 behaviour 12).
        if let Some((origin, search)) = self.config_key_usages(cx) {
            return Some(UsageQuery {
                origin,
                text: None,
                symbol: search.target.name.clone(),
                in_file: false,
                ready: Some(search),
            });
        }
        let here = self.here(cx)?;
        idecraft_languages::LanguageId::detect(&here.path)?;
        let editor = self.active_editor()?;
        let (subject, text) = {
            let e = editor.read(cx);
            let line = e.state.buffer.line(here.line).unwrap_or_default();
            let subject =
                usages_model::search_subject(line.trim_end_matches(['\n', '\r']), here.column);
            let text = subject.is_ok().then(|| e.state.buffer.text());
            (subject, text)
        };
        match subject {
            Ok(symbol) => Some(UsageQuery {
                text,
                origin: here,
                symbol,
                in_file: false,
                ready: None,
            }),
            Err(nothing) => {
                self.status = Some(nothing.to_owned());
                cx.notify();
                None
            }
        }
    }

    /// The text of a line from the open editor when the file is open, else from disk.
    fn line_source(&self, cx: &gpui::App) -> impl Fn(&Path, usize) -> Option<String> + use<> {
        let open: HashMap<PathBuf, idecraft_editor::TextBuffer> = self
            .editors
            .iter()
            .map(|(p, e)| (p.clone(), e.read(cx).state.buffer.clone()))
            .collect();
        move |path: &Path, line: usize| match open.get(path) {
            Some(b) => b
                .line(line)
                .map(|t| t.trim_end_matches(['\n', '\r']).to_owned()),
            None => usages_model::disk_line(path, line),
        }
    }

    fn hits_of(&self, places: &[Location], cx: &gpui::App) -> Vec<Hit> {
        let line = self.line_source(cx);
        places
            .iter()
            .map(|l| hit(&self.root, place_of(l), length_of(l), &line))
            .collect()
    }

    /// `FindUsages` (⌥F7): opens the Find tool window and searches (SPEC-0293 behaviour 1).
    pub(super) fn find_usages_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(q) = self.usage_query(cx) {
            self.start_find_usages(q, window, cx);
        }
    }

    /// `FindUsagesInFile` (⌘F7): the same search, shown with the usages of the caret's file only
    /// (SPEC-0293 v7 behaviour 19).
    pub(super) fn find_usages_in_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut q) = self.usage_query(cx) {
            q.in_file = true;
            self.start_find_usages(q, window, cx);
        }
    }

    /// Searches again for the window's symbol at its place (Rerun).
    fn rerun_find_usages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.find_usages.clone() else {
            return;
        };
        let (origin, symbol, in_file) = {
            let v = view.read(cx);
            (v.origin.clone(), v.symbol().to_owned(), v.in_file())
        };
        let text = self
            .editors
            .get(&origin.path)
            .map(|e| e.read(cx).state.buffer.text());
        // A configuration key is answered by the bean model again while the caret is still on
        // it (SPEC-0115 v5 behaviour 12); no language server knows it.
        let ready = self
            .config_key_usages(cx)
            .filter(|(place, _)| *place == origin)
            .map(|(_, search)| search);
        self.start_find_usages(
            UsageQuery {
                origin,
                text,
                symbol,
                in_file,
                ready,
            },
            window,
            cx,
        );
    }

    pub(super) fn stop_find_usages(&mut self) {
        if let Some(stop) = self.find_usages_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
    }

    fn start_find_usages(&mut self, q: UsageQuery, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_find_usages();
        let stop = Arc::new(AtomicBool::new(false));
        self.find_usages_stop = Some(Arc::clone(&stop));
        self.find_usages_run += 1;
        let run = self.find_usages_run;
        let file = q.file_name();
        let view = match self.find_usages.clone() {
            Some(v) => {
                v.update(cx, |v, cx| {
                    v.restart(&q.symbol, file.as_deref(), q.origin.clone(), cx)
                });
                v
            }
            None => {
                let v = cx.new(|cx| {
                    FindUsagesView::searching(&q.symbol, file.as_deref(), q.origin.clone(), cx)
                });
                self.subscribe_find_usages(&v, window, cx);
                self.find_usages = Some(v.clone());
                v
            }
        };
        self.show_find_usages = true;
        window.focus(&view.read(cx).focus());
        self.status = Some(format!("Finding usages of {}…", q.symbol));
        cx.notify();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let symbol = q.symbol.clone();
            let found = cx
                .background_spawn(async move {
                    let found = search_blocking(&root, &q, stop);
                    // The usages in other files are left out (v7 behaviour 19).
                    match q.in_file {
                        true => found.map(|s| s.only_in(&q.origin.path)),
                        false => found,
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, _window, cx| {
                if run != this.find_usages_run {
                    return;
                }
                this.find_usages_stop = None;
                let Some(view) = this.find_usages.clone() else {
                    return;
                };
                let message = match found {
                    Ok(search) if search.usages.is_empty() => {
                        let m = nothing_found(&symbol, search.source, file.as_deref());
                        view.update(cx, |v, cx| v.message(m.clone(), cx));
                        m
                    }
                    Ok(search) => {
                        let mut tree = this.tree_of(&search, cx);
                        tree.in_file = file.clone();
                        let texts = this.texts_of(&tree, cx);
                        let n = tree.hits.len();
                        let note = search.note.clone();
                        view.update(cx, |v, cx| v.show(tree, note.clone(), texts, cx));
                        match note {
                            Some(note) => note,
                            None => format!(
                                "{symbol}: {}",
                                crate::find_usages_model::count_label(n, false)
                            ),
                        }
                    }
                    Err(LspError::Stopped(_)) => {
                        let m = "Search stopped.".to_owned();
                        view.update(cx, |v, cx| v.message(m.clone(), cx));
                        m
                    }
                    Err(e) => {
                        let m = format!("Find Usages failed: {e}");
                        view.update(cx, |v, cx| v.message(m.clone(), cx));
                        m
                    }
                };
                this.status = Some(message);
                cx.notify();
            });
        })
        .detach();
    }

    fn tree_of(&self, search: &UsageSearch, cx: &gpui::App) -> UsageTree {
        let declared = match (&search.target.declaration, &search.target.outside) {
            (Some(d), _) => {
                let shown = d
                    .path
                    .strip_prefix(&self.root)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| d.path.display().to_string());
                format!("{shown}:{}", d.range.start_line + 1)
            }
            (None, Some(name)) => name.clone(),
            (None, None) => String::new(),
        };
        let mut hits = self.hits_of(&search.usages, cx);
        // Kind, module, package, class and method of each usage (SPEC-0293 v3 behaviour 14).
        for (h, d) in hits.iter_mut().zip(&search.details) {
            h.kind = Some(d.kind);
            h.context = Some(d.context.clone());
        }
        let mut tree = UsageTree::new(
            &search.target.name,
            declared,
            search.source == UsageSource::Text,
            hits,
        );
        tree.target_kind = search.target.kind.clone();
        tree
    }

    /// Texts of the files with hits, for the preview: the open editor's, else the disk's.
    fn texts_of(&self, tree: &UsageTree, cx: &gpui::App) -> HashMap<PathBuf, String> {
        let mut out = HashMap::new();
        for first in tree.first_hits() {
            let Some(h) = tree.hits.get(first) else {
                continue;
            };
            let path = h.place.path.clone();
            let text = match self.editors.get(&path) {
                Some(e) => Some(e.read(cx).state.buffer.text()),
                None => std::fs::read_to_string(&path).ok(),
            };
            if let Some(t) = text {
                out.insert(path, t);
            }
        }
        out
    }

    fn subscribe_find_usages(
        &mut self,
        view: &Entity<FindUsagesView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._find_usages_events = Some(cx.subscribe_in(
            view,
            window,
            |this, _, event: &FindUsagesEvent, window, cx| {
                match event {
                    FindUsagesEvent::Open(place) => {
                        // The tool window stays open (behaviour 8).
                        this.remember_place(cx);
                        let place = place.clone();
                        this.show_place(&place, window, cx);
                    }
                    FindUsagesEvent::Rerun => this.rerun_find_usages(window, cx),
                    FindUsagesEvent::Stop => this.stop_find_usages(),
                    FindUsagesEvent::FocusEditor => this.focus_active(window, cx),
                    FindUsagesEvent::Export => {
                        this.export_usages(window, cx);
                    }
                    FindUsagesEvent::Close => {
                        this.stop_find_usages();
                        this.show_find_usages = false;
                        this.focus_active(window, cx);
                    }
                }
                cx.notify();
            },
        ));
    }

    /// Shows `tree` in the Find tool window, which takes the focus, as the end of a search
    /// does. For checks without a language server.
    pub fn show_usage_tree(
        &mut self,
        tree: UsageTree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_find_usages();
        let origin = tree
            .hits
            .first()
            .map(|h| h.place.clone())
            .unwrap_or_else(|| crate::usages_model::place(self.root.clone(), 0, 0));
        let view = match self.find_usages.clone() {
            Some(v) => v,
            None => {
                let v = cx.new(|cx| {
                    FindUsagesView::searching(&tree.symbol, tree.in_file.as_deref(), origin, cx)
                });
                self.subscribe_find_usages(&v, window, cx);
                self.find_usages = Some(v.clone());
                v
            }
        };
        let texts = self.texts_of(&tree, cx);
        view.update(cx, |v, cx| v.show(tree, None, texts, cx));
        self.show_find_usages = true;
        window.focus(&view.read(cx).focus());
        cx.notify();
    }

    /// `NextOccurence`/`PreviousOccurence` (⌥⌘↓/⌥⌘↑) while the Find tool window shows usages:
    /// opens the next or previous one (behaviour 8); `false` when there is none.
    pub(super) fn step_find_usages(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(view) = self.find_tool_window() else {
            return false;
        };
        view.update(cx, |v, cx| v.step(forward, cx))
    }

    /// The Find tool window, if it is shown (SPEC-0293).
    pub fn find_tool_window(&self) -> Option<Entity<FindUsagesView>> {
        self.find_usages.clone().filter(|_| self.show_find_usages)
    }

    /// Hides the Find tool window (⇧Esc); `false` when it was not shown.
    pub(super) fn hide_find_usages(&mut self) -> bool {
        if !self.show_find_usages {
            return false;
        }
        self.stop_find_usages();
        self.show_find_usages = false;
        true
    }

    /// `ShowUsages` (⌥⌘F7): the usages popup on the bounded search, without the declaration; a
    /// single usage is a hint, or a jump to it and a hint (SPEC-0073 v5).
    pub(super) fn show_usages_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(q) = self.usage_query(cx) else {
            return;
        };
        let caret = q.origin.clone();
        self.status = Some(format!("Finding usages of {}…", q.symbol));
        cx.notify();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let symbol = q.symbol.clone();
            let found = cx
                .background_spawn(async move {
                    search_blocking(&root, &q, Arc::new(AtomicBool::new(false)))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let search = match found {
                    Ok(s) => s,
                    Err(e) => {
                        this.status = Some(format!("Find Usages failed: {e}"));
                        cx.notify();
                        return;
                    }
                };
                // The declaration is not a usage (SPEC-0073 v5 behaviour 6).
                let places: Vec<Location> = search.usages.clone();
                if places.is_empty() {
                    this.status = Some(nothing_found(&symbol, search.source, None));
                    cx.notify();
                    return;
                }
                // One usage: a hint instead of a popup, after a jump when it is elsewhere
                // (behaviour 7).
                if let [only] = places.as_slice()
                    && search.source == UsageSource::LanguageServer
                {
                    let (jump, hint) =
                        usages_model::one_usage(&symbol, &place_of(only), length_of(only), &caret);
                    if jump {
                        this.remember_place(cx);
                        this.show_place(&place_of(only), window, cx);
                    }
                    this.status = None;
                    this.show_hint(hint, cx);
                    cx.notify();
                    return;
                }
                let line = this.line_source(cx);
                let list =
                    usages_model::usages(&this.root, places.iter().map(place_of).collect(), line);
                let (title, status) = match search.source {
                    UsageSource::LanguageServer => {
                        (format!("Usages of {symbol} ({})", list.len()), None)
                    }
                    UsageSource::Text => (
                        format!("Text occurrences of {symbol} ({})", list.len()),
                        search.note.clone(),
                    ),
                };
                this.status = status;
                let view = cx.new(|cx| UsagesView::titled(title, list, cx));
                this.show_usages(view, window, cx);
            });
        })
        .detach();
    }
}
