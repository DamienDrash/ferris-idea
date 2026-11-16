//! Editor tab model (`SPEC-0059` behaviours 3 to 6 and v9 behaviours 15 to 20), without toolkit:
//! opening with the limit and its rule, pinned tabs, the preview tab, closing several tabs, the
//! files to reopen, the scroll offset that keeps the active tab visible and the hidden-tabs list.

use std::path::{Path, PathBuf};

use idecraft_core::{ActivateOnClose, EditorTabSettings, TabLimitRule};
use serde::{Deserialize, Serialize};

/// Most tabs kept open by default (v9 behaviour 16; before v9 a fixed 10).
pub const DEFAULT_TAB_LIMIT: usize = 30;

/// Most closed files remembered for `ReopenClosedTab` (v9 behaviour 19).
pub const REOPEN_LIMIT: usize = 30;

/// What opening and closing follow, taken from the Editor Tabs settings (v9 behaviour 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabOptions {
    /// Most tabs of the group.
    pub limit: usize,
    /// Which tab closes over the limit.
    pub rule: TabLimitRule,
    /// New tabs at the end instead of right of the active one.
    pub open_at_end: bool,
    /// Tabs sorted by name.
    pub sort: bool,
    /// Which tab becomes active when the active one closes.
    pub activate_on_close: ActivateOnClose,
}

impl Default for TabOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_TAB_LIMIT,
            rule: TabLimitRule::CloseUnchanged,
            open_at_end: false,
            sort: false,
            activate_on_close: ActivateOnClose::Recent,
        }
    }
}

impl TabOptions {
    /// The options of `settings`.
    pub fn of(settings: &EditorTabSettings) -> Self {
        Self {
            limit: settings.limit.max(1) as usize,
            rule: settings.limit_rule,
            open_at_end: settings.open_at_end,
            sort: settings.sort_alphabetically,
            activate_on_close: settings.activate_on_close,
        }
    }
}

/// Open tabs in display order, with activation history.
#[derive(Debug, Clone, Default)]
pub struct Tabs {
    paths: Vec<PathBuf>,
    /// Indices into `paths`, most recently activated last.
    history: Vec<usize>,
    /// Tabs the user tried to close once despite unsaved changes.
    warned: Option<PathBuf>,
    /// Pinned files (behaviour 20); they stand at the left end in pinning order.
    pinned: Vec<PathBuf>,
    /// The provisional tab of the preview setting (behaviour 17).
    preview: Option<PathBuf>,
}

/// What [`Tabs::open`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// Index of the now active tab.
    pub index: usize,
    /// Whether a new tab was added.
    pub added: bool,
    /// Tab closed to stay within the limit, or replaced as the preview tab.
    pub evicted: Option<PathBuf>,
    /// The closed tab had unsaved changes ("Close unused"): save it before dropping its view.
    pub evicted_modified: bool,
}

/// Which tabs a "close several" command closes (v9 behaviours 18 and 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseScope {
    /// Every tab but the given one (`CloseAllEditorsButActive`).
    Others,
    /// Every tab (`CloseAllEditors`).
    All,
    /// The tabs left of the given one (`CloseAllToTheLeft`).
    Left,
    /// The tabs right of the given one (`CloseAllToTheRight`).
    Right,
    /// The tabs without unsaved changes (`CloseAllUnmodifiedEditors`).
    Unmodified,
    /// Every tab that is not pinned (`CloseAllUnpinnedEditors`).
    Unpinned,
}

impl CloseScope {
    /// The scope of an action id, if it is one of these commands.
    pub fn of_action(id: &str) -> Option<Self> {
        Some(match id {
            "CloseAllEditorsButActive" => Self::Others,
            "CloseAllEditors" => Self::All,
            "CloseAllToTheLeft" => Self::Left,
            "CloseAllToTheRight" => Self::Right,
            "CloseAllUnmodifiedEditors" => Self::Unmodified,
            "CloseAllUnpinnedEditors" => Self::Unpinned,
            _ => return None,
        })
    }
}

/// The tabs a "close several" command closes: those without unsaved changes at once, those with
/// changes only after the one question (v9 behaviour 18).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClosePlan {
    /// Closed without asking.
    pub close: Vec<PathBuf>,
    /// Have unsaved changes: asked about once, in one dialog.
    pub ask: Vec<PathBuf>,
}

/// A group's tabs as remembered per project (v9 behaviour 20, v9.1 behaviour 22e).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTabs {
    /// Open files in display order.
    pub paths: Vec<PathBuf>,
    /// Pinned files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<PathBuf>,
    /// The active file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<PathBuf>,
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Tabs {
    /// Open paths in display order.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// The tab of `from` shows `to` now, in its place (a file renamed with its class, SPEC-0148 v2
    /// behaviour 6). Returns whether there was such a tab.
    pub fn rename(&mut self, from: &Path, to: &Path) -> bool {
        for p in &mut self.pinned {
            if p.as_path() == from {
                *p = to.to_path_buf();
            }
        }
        if self.preview.as_deref() == Some(from) {
            self.preview = Some(to.to_path_buf());
        }
        match self.paths.iter_mut().find(|p| p.as_path() == from) {
            Some(p) => {
                *p = to.to_path_buf();
                true
            }
            None => false,
        }
    }

    /// Index of the active tab.
    pub fn active(&self) -> Option<usize> {
        self.history.last().copied()
    }

    /// The active file.
    pub fn active_path(&self) -> Option<&PathBuf> {
        self.paths.get(self.active()?)
    }

    /// Index of `path`'s tab.
    pub fn position(&self, path: &Path) -> Option<usize> {
        self.paths.iter().position(|p| p == path)
    }

    /// Files from the most recently to the least recently activated.
    pub fn recent(&self) -> Vec<PathBuf> {
        self.history
            .iter()
            .rev()
            .map(|&i| self.paths[i].clone())
            .collect()
    }

    fn touch(&mut self, index: usize) {
        self.history.retain(|i| *i != index);
        self.history.push(index);
    }

    /// Activates a tab.
    pub fn activate(&mut self, index: usize) {
        if index < self.paths.len() {
            self.touch(index);
        }
    }

    /// Whether tab `index` is pinned.
    pub fn is_pinned(&self, index: usize) -> bool {
        self.paths
            .get(index)
            .is_some_and(|p| self.pinned.contains(p))
    }

    /// Per tab, whether it is pinned.
    pub fn pinned_flags(&self) -> Vec<bool> {
        (0..self.paths.len()).map(|i| self.is_pinned(i)).collect()
    }

    /// Number of pinned tabs (they are the first ones).
    pub fn pinned_count(&self) -> usize {
        self.paths
            .iter()
            .filter(|p| self.pinned.contains(p))
            .count()
    }

    /// Whether tab `index` is the provisional preview tab (behaviour 17).
    pub fn is_preview(&self, index: usize) -> bool {
        self.paths
            .get(index)
            .is_some_and(|p| self.preview.as_ref() == Some(p))
    }

    /// The preview tab becomes a normal tab (editing it, a double click).
    pub fn keep(&mut self, path: &Path) {
        if self.preview.as_deref() == Some(path) {
            self.preview = None;
        }
    }

    /// Moves tab `from` to index `to`, keeping the activation history.
    fn move_tab(&mut self, from: usize, to: usize) {
        if from == to || from >= self.paths.len() || to >= self.paths.len() {
            return;
        }
        let path = self.paths.remove(from);
        self.paths.insert(to, path);
        let map = |i: usize| -> usize {
            if i == from {
                to
            } else if from < to && i > from && i <= to {
                i - 1
            } else if to < from && i >= to && i < from {
                i + 1
            } else {
                i
            }
        };
        for i in &mut self.history {
            *i = map(*i);
        }
    }

    /// Pins or unpins tab `index`: a pinned tab moves to the left end after the other pinned
    /// tabs; an unpinned one moves right after them (behaviour 20). Returns whether it is pinned.
    pub fn toggle_pin(&mut self, index: usize) -> bool {
        let Some(path) = self.paths.get(index).cloned() else {
            return false;
        };
        if self.pinned.contains(&path) {
            self.pinned.retain(|p| *p != path);
            let to = self.pinned_count();
            self.move_tab(index, to);
            false
        } else {
            let to = self.pinned_count();
            self.pinned.push(path.clone());
            self.keep(&path);
            self.move_tab(index, to);
            true
        }
    }

    /// Moves the open tab of `path` back to the place a closed tab had (v9.2 behaviour 23): a tab
    /// that was pinned is pinned again at `index` among the pinned tabs; one that was not stays
    /// right of them, at `index` clamped to the row; with sorting on it keeps its sorted place.
    /// Returns its index.
    pub fn restore_place(
        &mut self,
        path: &Path,
        index: usize,
        pinned: bool,
        options: &TabOptions,
    ) -> Option<usize> {
        let from = self.position(path)?;
        let to = if pinned {
            if !self.pinned.iter().any(|p| p == path) {
                let at = index.min(self.pinned.len());
                self.pinned.insert(at, path.to_path_buf());
                self.keep(path);
            }
            index.min(self.pinned_count() - 1)
        } else if self.is_pinned(from) || options.sort {
            from
        } else {
            index.clamp(self.pinned_count(), self.paths.len() - 1)
        };
        self.move_tab(from, to);
        Some(to)
    }

    /// Pins (`true`) or unpins tab `index`.
    pub fn set_pinned(&mut self, index: usize, pinned: bool) {
        if self.is_pinned(index) != pinned {
            self.toggle_pin(index);
        }
    }

    /// The tab the limit closes before a new one is added, or `None` when there is room or every
    /// candidate must stay (behaviour 16): pinned tabs never, modified ones only with
    /// "Close unused".
    pub fn limit_victim(
        &self,
        options: &TabOptions,
        modified: impl Fn(&Path) -> bool,
    ) -> Option<usize> {
        if self.paths.len() < options.limit.max(1) {
            return None;
        }
        self.history.iter().copied().find(|&i| {
            !self.is_pinned(i)
                && (options.rule == TabLimitRule::CloseUnused || !modified(&self.paths[i]))
        })
    }

    /// Where a new tab goes: sorted, at the end, or right of the active tab, never inside the
    /// pinned tabs.
    fn insert_index(&self, path: &Path, options: &TabOptions) -> usize {
        let pinned = self.pinned_count();
        if options.sort {
            let key = name_of(path).to_lowercase();
            return (pinned..self.paths.len())
                .find(|&i| name_of(&self.paths[i]).to_lowercase() > key)
                .unwrap_or(self.paths.len());
        }
        if options.open_at_end {
            return self.paths.len();
        }
        self.active()
            .map_or(self.paths.len(), |a| a + 1)
            .max(pinned)
    }

    fn insert(&mut self, at: usize, path: &Path) {
        self.paths.insert(at, path.to_path_buf());
        for i in &mut self.history {
            if *i >= at {
                *i += 1;
            }
        }
    }

    /// Opens `path` with the default options (behaviours 3 and 4).
    pub fn open(&mut self, path: &Path, modified: impl Fn(&Path) -> bool) -> Opened {
        self.open_with(path, &TabOptions::default(), modified)
    }

    /// Opens `path`: activates its tab, or adds one where `options` say and closes one over the
    /// limit; `modified` tells which open tabs have unsaved changes (behaviours 3, 4 and 16).
    /// Opening the preview tab's file this way keeps it.
    pub fn open_with(
        &mut self,
        path: &Path,
        options: &TabOptions,
        modified: impl Fn(&Path) -> bool,
    ) -> Opened {
        if let Some(i) = self.position(path) {
            self.keep(path);
            self.touch(i);
            return Opened {
                index: i,
                added: false,
                evicted: None,
                evicted_modified: false,
            };
        }
        let mut evicted = None;
        let mut evicted_modified = false;
        if let Some(victim) = self.limit_victim(options, &modified) {
            evicted_modified = modified(&self.paths[victim]);
            evicted = Some(self.remove(victim));
        }
        let at = self.insert_index(path, options);
        self.insert(at, path);
        self.touch(at);
        Opened {
            index: at,
            added: true,
            evicted,
            evicted_modified,
        }
    }

    /// Opens `path` as the preview tab (a single click in the project tree, behaviour 17): it
    /// replaces the previous preview tab in its place unless that one has unsaved changes.
    pub fn open_preview(
        &mut self,
        path: &Path,
        options: &TabOptions,
        modified: impl Fn(&Path) -> bool,
    ) -> Opened {
        if let Some(i) = self.position(path) {
            self.touch(i);
            return Opened {
                index: i,
                added: false,
                evicted: None,
                evicted_modified: false,
            };
        }
        if let Some(old) = self.preview.clone()
            && let Some(i) = self.position(&old)
            && !modified(&old)
        {
            self.paths[i] = path.to_path_buf();
            self.preview = Some(path.to_path_buf());
            self.touch(i);
            return Opened {
                index: i,
                added: true,
                evicted: Some(old),
                evicted_modified: false,
            };
        }
        let opened = self.open_with(path, options, modified);
        self.preview = Some(path.to_path_buf());
        opened
    }

    /// Adds `path` at the end without activating it (a tab the limit could not save stays open).
    pub fn push_back(&mut self, path: &Path) {
        if self.position(path).is_none() {
            self.paths.push(path.to_path_buf());
            let i = self.paths.len() - 1;
            self.history.insert(0, i);
        }
    }

    fn remove(&mut self, index: usize) -> PathBuf {
        let path = self.paths.remove(index);
        self.history.retain(|i| *i != index);
        for i in &mut self.history {
            if *i > index {
                *i -= 1;
            }
        }
        self.pinned.retain(|p| *p != path);
        if self.preview.as_ref() == Some(&path) {
            self.preview = None;
        }
        path
    }

    /// Closes tab `index` unconditionally; the previously active tab becomes active (behaviour 5).
    pub fn close(&mut self, index: usize) -> Option<PathBuf> {
        self.close_with(index, ActivateOnClose::Recent)
    }

    /// Closes tab `index`; when it was active, `then` chooses the next active tab (v9 behaviour
    /// 17).
    pub fn close_with(&mut self, index: usize, then: ActivateOnClose) -> Option<PathBuf> {
        if index >= self.paths.len() {
            return None;
        }
        let was_active = self.active() == Some(index);
        let path = self.remove(index);
        if was_active && !self.paths.is_empty() {
            match then {
                ActivateOnClose::Recent => {}
                ActivateOnClose::Left => self.touch(index.saturating_sub(1)),
                ActivateOnClose::Right => self.touch(index.min(self.paths.len() - 1)),
            }
        }
        Some(path)
    }

    /// Closes the tabs of `paths` that are open; the active tab stays active when it stays open.
    pub fn close_paths(&mut self, paths: &[PathBuf]) -> Vec<PathBuf> {
        let mut closed = Vec::new();
        for p in paths {
            if let Some(i) = self.position(p) {
                closed.push(self.remove(i));
            }
        }
        closed
    }

    /// `CloseContent` on the active tab: a tab with unsaved changes is closed only on the second try.
    /// Returns the closed path.
    pub fn close_active(&mut self, modified: bool) -> Option<PathBuf> {
        self.close_active_with(modified, ActivateOnClose::Recent)
    }

    /// [`Self::close_active`] with the setting for the next active tab.
    pub fn close_active_with(&mut self, modified: bool, then: ActivateOnClose) -> Option<PathBuf> {
        let index = self.active()?;
        if modified && self.warned.as_deref() != Some(self.paths[index].as_path()) {
            self.warned = Some(self.paths[index].clone());
            return None;
        }
        self.warned = None;
        self.close_with(index, then)
    }

    /// The tabs `scope` closes around tab `index`, split into those closed at once and those to
    /// ask about (v9 behaviours 18–20): pinned tabs are left open; tab `index` itself is closed
    /// only by `All`, `Unmodified` and `Unpinned`.
    pub fn plan_close(
        &self,
        scope: CloseScope,
        index: usize,
        modified: impl Fn(&Path) -> bool,
    ) -> ClosePlan {
        let mut plan = ClosePlan::default();
        for (i, p) in self.paths.iter().enumerate() {
            if self.is_pinned(i) {
                continue;
            }
            let chosen = match scope {
                CloseScope::Others => i != index,
                CloseScope::All | CloseScope::Unpinned => true,
                CloseScope::Left => i < index,
                CloseScope::Right => i > index,
                CloseScope::Unmodified => !modified(p),
            };
            if !chosen {
                continue;
            }
            if modified(p) {
                plan.ask.push(p.clone());
            } else {
                plan.close.push(p.clone());
            }
        }
        plan
    }

    /// Activates the next (or previous) tab, wrapping around (behaviour 6).
    pub fn step(&mut self, forward: bool) {
        let n = self.paths.len();
        if let Some(a) = self.active().filter(|_| n > 0) {
            self.touch(if forward {
                (a + 1) % n
            } else {
                (a + n - 1) % n
            });
        }
    }

    /// Sorts the tabs after the pinned ones by name (the sort setting switched on).
    pub fn sort(&mut self) {
        let pinned = self.pinned_count();
        let mut order: Vec<usize> = (pinned..self.paths.len()).collect();
        order.sort_by_key(|&i| name_of(&self.paths[i]).to_lowercase());
        let mut map = vec![0; self.paths.len()];
        for (i, slot) in map.iter_mut().enumerate().take(pinned) {
            *slot = i;
        }
        for (new, &old) in order.iter().enumerate() {
            map[old] = pinned + new;
        }
        let mut paths = self.paths.clone();
        for (old, p) in self.paths.iter().enumerate() {
            paths[map[old]] = p.clone();
        }
        self.paths = paths;
        for i in &mut self.history {
            *i = map[*i];
        }
    }

    /// Per tab, whether its name gets the problem mark: the file has an error (`SPEC-0059` v4
    /// behaviour 10).
    pub fn problem_flags(&self, marks: &crate::problem_marks::ProblemFiles) -> Vec<bool> {
        self.paths.iter().map(|p| marks.file(p)).collect()
    }

    /// Tab titles: the file name, with the parent folder when names repeat (behaviour 3).
    pub fn titles(&self) -> Vec<String> {
        self.titles_with(true, true)
    }

    /// Tab titles as the settings ask (v9 behaviour 17): with or without the extension, with or
    /// without the parent folder for names that repeat.
    pub fn titles_with(&self, extension: bool, folder_for_equal_names: bool) -> Vec<String> {
        let shown = |p: &Path| {
            if extension {
                name_of(p)
            } else {
                p.file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }
        };
        self.paths
            .iter()
            .map(|p| {
                let n = shown(p);
                if folder_for_equal_names
                    && self
                        .paths
                        .iter()
                        .filter(|q| name_of(q) == name_of(p))
                        .count()
                        > 1
                {
                    let parent = p.parent().map(name_of).unwrap_or_default();
                    format!("{n} — {parent}")
                } else {
                    n
                }
            })
            .collect()
    }

    /// The group as remembered per project.
    pub fn saved(&self) -> SavedTabs {
        SavedTabs {
            paths: self.paths.clone(),
            pinned: self
                .paths
                .iter()
                .filter(|p| self.pinned.contains(p))
                .cloned()
                .collect(),
            active: self.active_path().cloned(),
        }
    }

    /// The tabs of `saved` whose files `exists` accepts, with their pins and the active one.
    pub fn restored(saved: &SavedTabs, exists: impl Fn(&Path) -> bool) -> Self {
        let mut tabs = Self::default();
        for p in saved.paths.iter().filter(|p| exists(p)) {
            if tabs.position(p).is_none() {
                tabs.paths.push(p.clone());
            }
        }
        // Least recently activated first, the saved active one last.
        tabs.history = (0..tabs.paths.len()).collect();
        if let Some(i) = saved.active.as_deref().and_then(|a| tabs.position(a)) {
            tabs.touch(i);
        }
        for p in &saved.pinned {
            if let Some(i) = tabs.position(p) {
                tabs.set_pinned(i, true);
            }
        }
        tabs
    }
}

/// A closed tab `ReopenClosedTab` can bring back: the file, its caret (char offset) and the place
/// it had (v9 behaviour 19, v9.2 behaviour 23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedTab {
    /// The file.
    pub path: PathBuf,
    /// The caret when it closed.
    pub caret: usize,
    /// The editor group it was in (`editor_layout::GroupId`).
    pub group: usize,
    /// Its index in that group's row; `usize::MAX` when unknown (the end of the row).
    pub index: usize,
    /// Whether it was pinned.
    pub pinned: bool,
}

impl ClosedTab {
    /// A tab of group `group` closed at `index` (pinned or not) with the caret at `caret`.
    pub fn new(path: &Path, caret: usize, group: usize, index: usize, pinned: bool) -> Self {
        Self {
            path: path.to_path_buf(),
            caret,
            group,
            index,
            pinned,
        }
    }
}

/// The tabs `ReopenClosedTab` brings back, most recently closed last, at most [`REOPEN_LIMIT`]
/// (v9 behaviour 19).
#[derive(Debug, Clone, Default)]
pub struct ClosedTabs {
    closed: Vec<ClosedTab>,
}

impl ClosedTabs {
    /// Remembers a closed tab; an earlier entry of the same file is dropped.
    pub fn push(&mut self, tab: ClosedTab) {
        self.closed.retain(|t| t.path != tab.path);
        self.closed.push(tab);
        if self.closed.len() > REOPEN_LIMIT {
            self.closed.remove(0);
        }
    }

    /// The most recently closed tab whose file is not open again (`open` tells).
    pub fn pop(&mut self, open: impl Fn(&ClosedTab) -> bool) -> Option<ClosedTab> {
        while let Some(t) = self.closed.pop() {
            if !open(&t) {
                return Some(t);
            }
        }
        None
    }

    /// Number of remembered files.
    pub fn len(&self) -> usize {
        self.closed.len()
    }

    /// Whether nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.closed.is_empty()
    }
}

/// The scroll offset of a one-row tab bar `bar` wide, with tabs `widths` wide, that shows the
/// active tab fully (v9 behaviour 15): unchanged while it is fully visible, else the least scroll
/// that shows it, never past the ends. A tab wider than the bar starts at the left edge.
pub fn scroll_offset(widths: &[f32], active: Option<usize>, bar: f32, current: f32) -> f32 {
    let total: f32 = widths.iter().sum();
    let max = (total - bar).max(0.0);
    let mut offset = current.clamp(0.0, max);
    if let Some(a) = active.filter(|&a| a < widths.len()) {
        let start: f32 = widths[..a].iter().sum();
        let end = start + widths[a];
        if end > offset + bar {
            offset = end - bar;
        }
        if start < offset {
            offset = start;
        }
    }
    offset.clamp(0.0, max)
}

/// Whether a tab `width` wide starting `start` from the left end of a row `bar` wide, both as laid
/// out before scrolling, is fully visible at scroll offset `offset` (v9 behaviour 15).
pub fn fully_visible(start: f32, width: f32, bar: f32, offset: f32) -> bool {
    start - offset >= -0.5 && start + width - offset <= bar + 0.5
}

/// Whether the tabs `widths` wide overflow a bar `bar` wide (the chevron shows then).
pub fn overflows(widths: &[f32], bar: f32) -> bool {
    widths.iter().sum::<f32>() > bar + 0.5
}

/// The tabs fully or partly hidden at scroll offset `offset` of a bar `bar` wide.
pub fn hidden_tabs(widths: &[f32], bar: f32, offset: f32) -> Vec<usize> {
    let mut start = 0.0;
    let mut out = Vec::new();
    for (i, w) in widths.iter().enumerate() {
        let end = start + w;
        if start < offset - 0.5 || end > offset + bar + 0.5 {
            out.push(i);
        }
        start = end;
    }
    out
}

/// The rows of the "Show hidden tabs" list (v9 behaviour 15): every tab whose title contains each
/// character of `filter` in order (ignoring case), in tab order.
pub fn tab_list(titles: &[String], filter: &str) -> Vec<usize> {
    let f: Vec<char> = filter
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    titles
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            let mut wanted = f.iter().peekable();
            for c in t.to_lowercase().chars() {
                if wanted.peek() == Some(&&c) {
                    wanted.next();
                }
            }
            wanted.peek().is_none()
        })
        .map(|(i, _)| i)
        .collect()
}

/// The characters the tool windows show in the Switcher (`SPEC-0059` v9.4 behaviour 27b), in the
/// order of `titles`: the digit of the keymap's shortcut where `digits` has one, else the first
/// letter of the title that no tool window before it took, in upper case; `None` when every
/// letter is taken.
pub fn switcher_characters(titles: &[&str], digits: &[Option<char>]) -> Vec<Option<char>> {
    let mut taken: Vec<char> = Vec::new();
    titles
        .iter()
        .enumerate()
        .map(|(i, title)| {
            if let Some(d) = digits.get(i).copied().flatten() {
                return Some(d);
            }
            let free = title
                .chars()
                .filter(|c| c.is_alphabetic())
                .flat_map(char::to_uppercase)
                .find(|c| !taken.contains(c))?;
            taken.push(free);
            Some(free)
        })
        .collect()
}

/// The names of the Switcher's files (behaviour 27a): the file name, with the parent folder when
/// two of them share it, as tabs do (behaviour 3).
pub fn switcher_names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| {
            let n = name_of(p);
            if paths.iter().filter(|q| name_of(q) == n).count() > 1 {
                let parent = p.parent().map(name_of).unwrap_or_default();
                format!("{n} — {parent}")
            } else {
                n
            }
        })
        .collect()
}

/// The Switcher's folder line for the selected file (behaviour 27d): its folder from the project
/// folder `root` (the project folder's name for a file directly in it), or, outside the project,
/// the folder with `~` for the home folder.
pub fn switcher_folder(path: &Path, root: &Path, home: Option<&Path>) -> String {
    let Some(dir) = path.parent() else {
        return String::new();
    };
    match dir.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => name_of(root),
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => match home.and_then(|h| dir.strip_prefix(h).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.display()),
            None => dir.display().to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SPEC-0059` v9.4 behaviour 27b (AK-15).
    #[test]
    fn switcher_characters_are_digits_of_the_keymap_or_free_letters() {
        let titles = ["Project", "Commit", "Problems", "Terminal", "Plugins", "Pp"];
        let digits = [Some('1'), Some('0'), None, None, None, None];
        assert_eq!(
            switcher_characters(&titles, &digits),
            [
                Some('1'),
                Some('0'),
                Some('P'),
                Some('T'),
                // P is taken by Problems: the next letter of the title.
                Some('L'),
                None
            ]
        );
        // Titles with other characters before the first letter, and no digits at all.
        assert_eq!(
            switcher_characters(&["AI Chat", "(A)"], &[]),
            [Some('A'), None]
        );
    }

    /// `SPEC-0059` v9.4 behaviours 27a and 27d (AK-15).
    #[test]
    fn switcher_names_and_folder_line() {
        let paths = [
            PathBuf::from("/p/src/a/Main.java"),
            PathBuf::from("/p/src/b/Main.java"),
            PathBuf::from("/p/README.md"),
        ];
        assert_eq!(
            switcher_names(&paths),
            ["Main.java — a", "Main.java — b", "README.md"]
        );
        let (root, home) = (Path::new("/home/u/p"), Some(Path::new("/home/u")));
        assert_eq!(
            switcher_folder(Path::new("/home/u/p/src/a/Main.java"), root, home),
            "src/a"
        );
        // Directly in the project folder: its name.
        assert_eq!(
            switcher_folder(Path::new("/home/u/p/README.md"), root, home),
            "p"
        );
        // Outside the project: with `~` for the home folder, or as it is.
        assert_eq!(
            switcher_folder(Path::new("/home/u/notes/todo.txt"), root, home),
            "~/notes"
        );
        assert_eq!(
            switcher_folder(Path::new("/home/u/todo.txt"), root, home),
            "~"
        );
        assert_eq!(switcher_folder(Path::new("/etc/hosts"), root, home), "/etc");
    }

    #[test]
    fn the_tab_menu_offers_the_local_history_and_every_item_runs() {
        use crate::context_menu::{MenuEntry, TabTarget, flatten, tab_menu};
        let menu = tab_menu(&TabTarget {
            pinned: true,
            any_pinned: true,
            split: true,
            runnable: Some(crate::context_menu::run_names("App.java", "", None)),
            annotated: false,
        });
        let items: Vec<_> = menu
            .iter()
            .flat_map(|e| match e {
                MenuEntry::Item(i) => vec![i.clone()],
                MenuEntry::Sub(_, s) => flatten(s).into_iter().cloned().collect(),
                MenuEntry::Separator => vec![],
            })
            .collect();
        assert!(items.iter().any(|i| i.action == "LocalHistory.ShowHistory"));
        assert!(items.iter().any(|i| i.action == "Annotate"));
        let workbench = [
            include_str!("workbench_view.rs"),
            include_str!("workbench_menus.rs"),
            include_str!("workbench_tabs.rs"),
        ]
        .concat();
        for item in items.iter().filter(|i| i.enabled) {
            assert!(
                workbench.contains(&format!("\"{}\"", item.action))
                    || workbench.contains(&format!("{} =>", item.action))
                    || CloseScope::of_action(&item.action).is_some()
                    || item.action.starts_with("IDECraft.Copy")
                    || item.action.starts_with("Run")
                    || item.action.starts_with("Debug")
                    || matches!(
                        item.action.as_str(),
                        "CopyPaths" | "RevealIn" | "Terminal.OpenInTerminal" | "RenameElement"
                    ),
                "{} is in the tab menu but the workbench does not run it",
                item.action
            );
        }
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn a_renamed_file_keeps_its_tab() {
        let mut tabs = Tabs::default();
        tabs.open(&p("/a/Lager.java"), |_| false);
        tabs.open(&p("/a/App.java"), |_| false);
        tabs.toggle_pin(0);
        assert!(tabs.rename(&p("/a/Lager.java"), &p("/a/Speicher.java")));
        assert_eq!(tabs.paths(), [p("/a/Speicher.java"), p("/a/App.java")]);
        assert_eq!(tabs.active(), Some(1));
        assert!(tabs.is_pinned(0), "the pin follows the rename");
        assert!(!tabs.rename(&p("/a/Nope.java"), &p("/a/X.java")));
    }

    #[test]
    fn tab_problem_flags_follow_the_errors() {
        use crate::diagnostics_model::Severity;
        use crate::problem_marks::ProblemFiles;
        let mut t = Tabs::default();
        for f in ["/w/test.py", "/w/test.java", "/w/test.ts"] {
            t.open(&p(f), |_| false);
        }
        let java = p("/w/test.java");
        let ts = p("/w/test.ts");
        let marks = ProblemFiles::from_severities([
            (java.as_path(), vec![Severity::Error]),
            (ts.as_path(), vec![Severity::Warning]),
        ]);
        assert_eq!(t.problem_flags(&marks), [false, true, false]);
        // The flag moves with its tab.
        t.close(0);
        assert_eq!(t.problem_flags(&marks), [true, false]);
        // Fixed: the mark disappears.
        let fixed = ProblemFiles::from_severities([(java.as_path(), Vec::<Severity>::new())]);
        assert_eq!(t.problem_flags(&fixed), [false, false]);
    }

    #[test]
    fn open_activate_insert_and_titles() {
        let mut t = Tabs::default();
        t.open(&p("/a/x.rs"), |_| false);
        t.open(&p("/a/y.rs"), |_| false);
        t.activate(0);
        let o = t.open(&p("/b/x.rs"), |_| false);
        assert_eq!(o.index, 1, "right of the active tab");
        assert_eq!(t.paths(), [p("/a/x.rs"), p("/b/x.rs"), p("/a/y.rs")]);
        let again = t.open(&p("/a/y.rs"), |_| false);
        assert_eq!((again.index, again.added), (2, false));
        assert_eq!(t.titles(), ["x.rs — a", "x.rs — b", "y.rs"]);
        // v9 behaviour 17: without extension and without the folder.
        assert_eq!(t.titles_with(false, false), ["x", "x", "y"]);
    }

    /// v9 behaviour 16, AK-11: limit 30 with both rules; pinned tabs are spared.
    #[test]
    fn limit_spares_modified_tabs() {
        let mut t = Tabs::default();
        for i in 0..DEFAULT_TAB_LIMIT {
            t.open(&p(&format!("/f{i}")), |_| false);
        }
        assert_eq!(t.paths().len(), 30, "14 or 30 files all stay open");
        // f0 is the least recently activated but modified: f1 goes.
        let o = t.open(&p("/new"), |q| q == Path::new("/f0"));
        assert_eq!(o.evicted, Some(p("/f1")));
        assert!(!o.evicted_modified);
        assert_eq!(t.paths().len(), DEFAULT_TAB_LIMIT);
        let all = t.open(&p("/more"), |_| true);
        assert_eq!(all.evicted, None);
        assert_eq!(t.paths().len(), DEFAULT_TAB_LIMIT + 1);
    }

    #[test]
    fn the_limit_follows_its_setting_and_rule_and_spares_pinned_tabs() {
        let unused = TabOptions {
            limit: 3,
            rule: TabLimitRule::CloseUnused,
            ..TabOptions::default()
        };
        let mut t = Tabs::default();
        for f in ["/a", "/b", "/c"] {
            t.open_with(&p(f), &unused, |_| false);
        }
        t.toggle_pin(t.position(&p("/a")).unwrap());
        // "Close unused": /b is the least recently activated unpinned tab, modified or not; the
        // caller saves it first.
        let o = t.open_with(&p("/d"), &unused, |q| q == Path::new("/b"));
        assert_eq!(o.evicted, Some(p("/b")));
        assert!(o.evicted_modified);
        assert!(t.position(&p("/a")).is_some(), "pinned stays");
        // Every tab pinned: nothing closes.
        let mut all = Tabs::default();
        for f in ["/a", "/b", "/c"] {
            all.open_with(&p(f), &unused, |_| false);
        }
        for i in 0..3 {
            all.set_pinned(i, true);
        }
        assert_eq!(all.open_with(&p("/d"), &unused, |_| false).evicted, None);
        assert_eq!(TabOptions::of(&EditorTabSettings::default()).limit, 30);
    }

    #[test]
    fn close_and_step() {
        let mut t = Tabs::default();
        for f in ["/a", "/b", "/c"] {
            t.open(&p(f), |_| false);
        }
        t.activate(0);
        t.activate(2);
        assert_eq!(t.close_active(false), Some(p("/c")));
        assert_eq!(t.active(), Some(0), "previously active");
        assert_eq!(t.close_active(true), None, "modified: first try only warns");
        assert_eq!(t.close_active(true), Some(p("/a")));
        t.open(&p("/d"), |_| false);
        t.step(true);
        assert_eq!(t.paths()[t.active().unwrap()], p("/b"));
        t.step(false);
        assert_eq!(t.paths()[t.active().unwrap()], p("/d"));
    }

    #[test]
    fn closing_activates_left_right_or_recent_as_set() {
        let make = || {
            let mut t = Tabs::default();
            for f in ["/a", "/b", "/c", "/d"] {
                t.open(&p(f), |_| false);
            }
            t.activate(3);
            t.activate(1);
            t
        };
        let mut recent = make();
        recent.close_with(1, ActivateOnClose::Recent);
        assert_eq!(recent.active_path(), Some(&p("/d")));
        let mut left = make();
        left.close_with(1, ActivateOnClose::Left);
        assert_eq!(left.active_path(), Some(&p("/a")));
        let mut right = make();
        right.close_with(1, ActivateOnClose::Right);
        assert_eq!(right.active_path(), Some(&p("/c")));
    }

    /// v9 behaviours 18–20, AK-11: close-several asks once for modified tabs and leaves pinned
    /// ones; pinned tabs move left and survive Close Others.
    #[test]
    fn close_several_plans_and_pinned_tabs() {
        let mut t = Tabs::default();
        for f in ["/a", "/b", "/c", "/d", "/e"] {
            t.open(&p(f), |_| false);
        }
        let d = t.position(&p("/d")).unwrap();
        assert!(t.toggle_pin(d));
        assert_eq!(t.paths()[0], p("/d"), "a pinned tab moves to the left end");
        assert_eq!(
            t.active_path(),
            Some(&p("/e")),
            "pinning keeps the active tab"
        );
        let c = t.position(&p("/c")).unwrap();
        let modified = |q: &Path| q == Path::new("/b");
        let others = t.plan_close(CloseScope::Others, c, modified);
        assert_eq!(others.close, [p("/a"), p("/e")]);
        assert_eq!(others.ask, [p("/b")], "asked about once");
        let left = t.plan_close(CloseScope::Left, c, modified);
        assert_eq!((left.close, left.ask), (vec![p("/a")], vec![p("/b")]));
        let right = t.plan_close(CloseScope::Right, c, modified);
        assert_eq!(right.close, [p("/e")]);
        let unmodified = t.plan_close(CloseScope::Unmodified, c, modified);
        assert_eq!(unmodified.close, [p("/a"), p("/c"), p("/e")]);
        assert!(unmodified.ask.is_empty());
        let all = t.plan_close(CloseScope::All, c, |_| false);
        assert!(
            !all.close.contains(&p("/d")),
            "Close All leaves the pinned tab"
        );
        t.activate(c);
        t.close_paths(&others.close);
        assert_eq!(t.paths(), [p("/d"), p("/b"), p("/c")]);
        assert_eq!(t.active_path(), Some(&p("/c")));
        // Unpinning moves it right after the pinned tabs (none left): it stays first.
        assert!(!t.toggle_pin(0));
        assert_eq!(t.paths()[0], p("/d"));
        // A second pin goes after the first.
        t.toggle_pin(2);
        t.toggle_pin(2);
        assert_eq!(t.paths()[..2], [p("/c"), p("/b")]);
        assert_eq!(
            CloseScope::of_action("CloseAllToTheRight"),
            Some(CloseScope::Right)
        );
    }

    #[test]
    fn new_tabs_go_after_the_pinned_ones_at_the_end_or_sorted() {
        let mut t = Tabs::default();
        t.open(&p("/w/b.rs"), |_| false);
        t.toggle_pin(0);
        let o = t.open(&p("/w/z.rs"), |_| false);
        assert_eq!(o.index, 1, "never inside the pinned tabs");
        t.activate(0);
        let at_end = TabOptions {
            open_at_end: true,
            ..TabOptions::default()
        };
        assert_eq!(t.open_with(&p("/w/a.rs"), &at_end, |_| false).index, 2);
        let sorted = TabOptions {
            sort: true,
            ..TabOptions::default()
        };
        t.sort();
        assert_eq!(t.paths(), [p("/w/b.rs"), p("/w/a.rs"), p("/w/z.rs")]);
        assert_eq!(t.open_with(&p("/w/m.rs"), &sorted, |_| false).index, 2);
        assert_eq!(t.active_path(), Some(&p("/w/m.rs")));
    }

    #[test]
    fn the_preview_tab_is_replaced_until_kept() {
        let o = TabOptions::default();
        let mut t = Tabs::default();
        t.open(&p("/a"), |_| false);
        t.open_preview(&p("/b"), &o, |_| false);
        assert!(t.is_preview(1));
        let r = t.open_preview(&p("/c"), &o, |_| false);
        assert_eq!(
            r.evicted,
            Some(p("/b")),
            "the next single click replaces it"
        );
        assert_eq!(t.paths(), [p("/a"), p("/c")]);
        // Editing (or a double click) keeps it.
        t.keep(&p("/c"));
        t.open_preview(&p("/d"), &o, |_| false);
        assert_eq!(t.paths(), [p("/a"), p("/c"), p("/d")]);
        // A modified preview tab is not replaced.
        t.open_preview(&p("/e"), &o, |q| q == Path::new("/d"));
        assert_eq!(t.paths().len(), 4);
        // Opening it normally keeps it.
        t.open(&p("/e"), |_| false);
        assert!(!t.is_preview(3));
    }

    /// v9 behaviour 19, AK-11: reopen-closed restores the last file and caret.
    #[test]
    fn reopen_closed_restores_the_last_file_and_caret() {
        let t = |path: &str, caret: usize| ClosedTab::new(&p(path), caret, 0, 0, false);
        let mut c = ClosedTabs::default();
        c.push(t("/a", 3));
        c.push(t("/b", 7));
        c.push(t("/a", 9));
        assert_eq!(c.len(), 2);
        assert_eq!(c.pop(|_| false), Some(t("/a", 9)));
        // A file open again is skipped.
        c.push(t("/c", 1));
        assert_eq!(c.pop(|q| q.path == Path::new("/c")), Some(t("/b", 7)));
        assert!(c.pop(|_| false).is_none());
        for i in 0..40 {
            c.push(t(&format!("/f{i}"), i));
        }
        assert_eq!(c.len(), REOPEN_LIMIT);
    }

    /// v9.2 behaviour 23, AK-13: a closed tab reopens at its former index, clamped; a pinned one
    /// comes back pinned, an unpinned one never among the pinned tabs; sorting wins.
    #[test]
    fn reopen_closed_puts_the_tab_back_at_its_former_place() {
        let names = |t: &Tabs| -> Vec<String> {
            t.paths()
                .iter()
                .map(|q| q.to_string_lossy().into_owned())
                .collect()
        };
        let at_end = TabOptions {
            open_at_end: true,
            ..TabOptions::default()
        };
        let mut tabs = Tabs::default();
        for f in ["/1", "/2", "/3", "/4", "/5"] {
            tabs.open_with(&p(f), &at_end, |_| false);
        }
        // The third tab closes and comes back third, active.
        let i = tabs.position(&p("/3")).unwrap();
        tabs.close(i);
        tabs.open_with(&p("/3"), &at_end, |_| false);
        assert_eq!(tabs.restore_place(&p("/3"), i, false, &at_end), Some(2));
        assert_eq!(names(&tabs), ["/1", "/2", "/3", "/4", "/5"]);
        assert_eq!(tabs.active_path(), Some(&p("/3")));
        // Clamped to the row when tabs closed since.
        tabs.close(4);
        tabs.close(3);
        tabs.open_with(&p("/9"), &at_end, |_| false);
        assert_eq!(tabs.restore_place(&p("/9"), 7, false, &at_end), Some(3));
        // Never among the pinned tabs.
        tabs.set_pinned(0, true);
        tabs.set_pinned(1, true);
        let i = tabs.position(&p("/3")).unwrap();
        tabs.close(i);
        tabs.close(0);
        tabs.open_with(&p("/1"), &at_end, |_| false);
        assert_eq!(names(&tabs), ["/2", "/9", "/1"]);
        assert_eq!(tabs.restore_place(&p("/1"), 0, false, &at_end), Some(1));
        assert_eq!(names(&tabs), ["/2", "/1", "/9"]);
        assert!(!tabs.is_pinned(1));
        // A pinned tab comes back pinned at its index among the pinned ones.
        tabs.close(0);
        tabs.open_with(&p("/2"), &at_end, |_| false);
        assert_eq!(tabs.restore_place(&p("/2"), 0, true, &at_end), Some(0));
        assert_eq!(names(&tabs), ["/2", "/1", "/9"]);
        assert!(tabs.is_pinned(0) && tabs.pinned_count() == 1);
        // Sorting on: the sorted place wins.
        let sorted = TabOptions {
            sort: true,
            ..TabOptions::default()
        };
        let mut tabs = Tabs::default();
        for f in ["/a", "/b", "/c"] {
            tabs.open_with(&p(f), &sorted, |_| false);
        }
        tabs.close(0);
        tabs.open_with(&p("/a"), &sorted, |_| false);
        assert_eq!(tabs.restore_place(&p("/a"), 2, false, &sorted), Some(0));
        assert_eq!(names(&tabs), ["/a", "/b", "/c"]);
    }

    /// v9 behaviour 15, AK-11: the offset keeps the active tab fully visible for any bar width.
    #[test]
    fn overflow_offset_keeps_the_active_tab_visible_for_any_bar_width() {
        let widths: Vec<f32> = (0..14).map(|i| 80.0 + (i % 4) as f32 * 17.0).collect();
        for bar in [60.0, 100.0, 250.0, 333.0, 700.0, 2000.0] {
            for current in [0.0, 150.0, 999.0, 5000.0] {
                for active in 0..widths.len() {
                    let o = scroll_offset(&widths, Some(active), bar, current);
                    let start: f32 = widths[..active].iter().sum();
                    let end = start + widths[active];
                    let total: f32 = widths.iter().sum();
                    assert!(o >= 0.0 && o <= (total - bar).max(0.0) + 0.01);
                    if widths[active] <= bar {
                        assert!(
                            start >= o - 0.01 && end <= o + bar + 0.01,
                            "bar {bar}, current {current}, active {active}: offset {o}"
                        );
                        assert!(!hidden_tabs(&widths, bar, o).contains(&active));
                    } else {
                        assert!((o - start).abs() < 0.01, "a wide tab starts at the edge");
                    }
                }
            }
        }
        // Visible already: no jump.
        assert_eq!(scroll_offset(&[100.0; 5], Some(2), 300.0, 150.0), 150.0);
        assert!(overflows(&[100.0; 5], 300.0));
        assert!(!overflows(&[100.0; 3], 300.0));
        assert_eq!(hidden_tabs(&[100.0; 5], 300.0, 0.0), [3, 4]);
        // The visibility check counts the scroll offset: the last of 14 tabs scrolled to the end
        // is visible, unscrolled it is not (the self-test's first tab check).
        let total: f32 = widths.iter().sum();
        let last = total - widths[13];
        let end = scroll_offset(&widths, Some(13), 700.0, 0.0);
        assert!(fully_visible(last, widths[13], 700.0, end));
        assert!(!fully_visible(last, widths[13], 700.0, 0.0));
        assert!(fully_visible(0.0, widths[0], 700.0, 0.0));
        assert!(!fully_visible(0.0, widths[0], 700.0, end));
    }

    /// v9 behaviour 15, AK-11: the list holds every tab, hidden ones included, and filters.
    #[test]
    fn the_hidden_tabs_list_holds_every_tab_and_filters() {
        let titles: Vec<String> = (0..14).map(|i| format!("File{i}.java")).collect();
        assert_eq!(tab_list(&titles, ""), (0..14).collect::<Vec<_>>());
        assert_eq!(tab_list(&titles, "e13"), [13]);
        assert_eq!(tab_list(&titles, "F1J"), [1, 10, 11, 12, 13]);
    }

    #[test]
    fn saved_tabs_come_back_with_pins_and_the_active_one() {
        let mut t = Tabs::default();
        for f in ["/a", "/b", "/c"] {
            t.open(&p(f), |_| false);
        }
        t.toggle_pin(2);
        t.activate(1);
        let saved = t.saved();
        assert_eq!(saved.pinned, [p("/c")]);
        let json = serde_json::to_string(&saved).unwrap();
        let back: SavedTabs = serde_json::from_str(&json).unwrap();
        let r = Tabs::restored(&back, |q| q != Path::new("/b"));
        assert_eq!(r.paths(), [p("/c"), p("/a")], "a missing file is left out");
        assert!(r.is_pinned(0));
        let r = Tabs::restored(&back, |_| true);
        assert_eq!(r.active_path(), t.active_path());
    }
}
