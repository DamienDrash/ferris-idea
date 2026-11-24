//! Editor tabs and nested editor groups in the workbench (`SPEC-0059` v9 behaviours 15–22,
//! `SPEC-0112` v5 behaviours 10–12): the groups of the layout tree with their tab bars, the
//! split, move, close and unsplit commands, the tab actions (close several, pin, reopen, tab list,
//! Switcher, maximize), the editor area remembered per project and the window side of the MCP
//! tool `editor_tabs`.
//!
//! The models are toolkit-free: [`crate::tabs`] (one group's tabs) and [`crate::editor_layout`]
//! (the tree of groups).

use std::path::{Path, PathBuf};

use ferris_idea_core::{CloseButtonPlace, EditorTabSettings, TabPlacement, TabRows};
use ferris_idea_ui::tool_windows::ToolWindow;
use gpui::{Context, Entity, MouseButton, Window, div, prelude::*, px};

use super::{EditorEvent, EditorView, Split, WorkbenchView, c, icon, island, problem_mark};
use crate::editor_layout::{GroupId, Layout, MAIN, Node, SavedEditors, SavedGroup, SplitPath};
use crate::palette::Palette;
use crate::tabs::{CloseScope, ClosedTab, TabOptions, Tabs};
use crate::tips::{Tipped, declared};
use crate::{TabListEvent, TabListRow, TabListView};

/// Where a tab bar last scrolled to, so that it scrolls again only when the active tab, the
/// number of tabs or the bar's width changed (`SPEC-0059` v9 behaviour 15); a wheel scroll in
/// between stays.
#[derive(Default)]
pub(super) struct TabScroll {
    pub(super) handle: gpui::ScrollHandle,
    key: Option<(Option<PathBuf>, usize, i32)>,
    /// The tab indices of the scrolled row in row order (pinned tabs in their own row left out).
    shown: Vec<usize>,
}

/// What a row of the tab list or the Switcher opens.
#[derive(Debug, Clone)]
pub(super) enum ListTarget {
    /// A tab of a group.
    Tab(GroupId, PathBuf),
    /// A recently used file (opened in the main group).
    File(PathBuf),
    /// A tool window.
    Tool(ToolWindow),
    /// An action of the workbench (Recent Locations in the Switcher).
    Action(&'static str),
}

/// The open tab list or Switcher popup.
pub(super) struct TabListPopup {
    pub(super) view: Entity<TabListView>,
    targets: Vec<Option<ListTarget>>,
    /// Window position of its top right corner; `None`: centred under the toolbar.
    at: Option<gpui::Point<gpui::Pixels>>,
    _events: gpui::Subscription,
}

/// A divider being dragged (`SPEC-0112` v5 behaviour 10).
#[derive(Clone)]
pub(super) struct DividerDrag(pub(super) SplitPath);

impl Render for DividerDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl WorkbenchView {
    // ---------------------------------------------------------------- groups

    /// The Editor Tabs settings (`SPEC-0059` v9 behaviour 17).
    pub(super) fn tab_settings(cx: &gpui::App) -> EditorTabSettings {
        crate::appearance::ide_settings(cx).editor.tabs
    }

    /// What opening and closing tabs follow now.
    pub(super) fn tab_options(cx: &gpui::App) -> TabOptions {
        TabOptions::of(&Self::tab_settings(cx))
    }

    /// The tabs of group `gid`.
    pub(super) fn group_tabs(&self, gid: GroupId) -> Option<&Tabs> {
        if gid == MAIN {
            Some(&self.tabs)
        } else {
            self.groups.iter().find(|g| g.id == gid).map(|g| &g.tabs)
        }
    }

    pub(super) fn group_tabs_mut(&mut self, gid: GroupId) -> Option<&mut Tabs> {
        if gid == MAIN {
            Some(&mut self.tabs)
        } else {
            self.groups
                .iter_mut()
                .find(|g| g.id == gid)
                .map(|g| &mut g.tabs)
        }
    }

    fn group_mut(&mut self, gid: GroupId) -> Option<&mut Split> {
        self.groups.iter_mut().find(|g| g.id == gid)
    }

    /// The view of `path` in group `gid`.
    pub(super) fn group_editor(&self, gid: GroupId, path: &Path) -> Option<Entity<EditorView>> {
        if gid == MAIN {
            self.editors.get(path).cloned()
        } else {
            self.groups
                .iter()
                .find(|g| g.id == gid)
                .and_then(|g| g.editors.get(path).cloned())
        }
    }

    /// The other groups' views of `path` (`SPEC-0112` v5).
    pub(super) fn twins(&self, path: &Path) -> Vec<Entity<EditorView>> {
        self.groups
            .iter()
            .filter_map(|g| g.editors.get(path).cloned())
            .collect()
    }

    /// Whether the document of `path` has unsaved changes, whichever view holds it.
    pub(super) fn doc_modified(&self, path: &Path, cx: &gpui::App) -> bool {
        self.editors
            .get(path)
            .into_iter()
            .chain(self.groups.iter().filter_map(|g| g.editors.get(path)))
            .any(|e| e.read(cx).state.buffer.is_modified())
            || self.remote_unsaved(path)
    }

    /// The icon of the file `path` on its tab and in the project tree (`SPEC-0131` v3
    /// behaviour 7): for a Java or Kotlin file the kind of its primary declaration — from the
    /// editor's text while the file is open, else from the file on disk — and the file type's
    /// icon otherwise.
    ///
    /// A file whose language an override, a pattern or its hashbang line decided shows that
    /// language's icon (`SPEC-0004` v6 behaviour 15).
    ///
    /// A file whose name gives no icon shows the text icon when it opens as text, and the
    /// unknown icon only when it does not (a binary file, `SPEC-0004` v12 behaviour 15).
    pub fn icon_of(&self, path: &Path, cx: &gpui::App) -> &'static str {
        let decided = self.file_type_icon(path);
        let plain =
            || crate::file_type_icons::name_icon(path, decided.is_some_and(|t| t.opens_as_text));
        if let Some(t) = decided
            && let Some(icon) = t.icon
        {
            let stays_jvm = matches!(
                t.language,
                Some(
                    ferris_idea_languages::LanguageId::Java
                        | ferris_idea_languages::LanguageId::Kotlin
                )
            );
            if !(stays_jvm && crate::decl_icon_model::has_kinds(path)) {
                return icon;
            }
        }
        if !crate::decl_icon_model::has_kinds(path) {
            return plain();
        }
        let editor = self
            .editors
            .get(path)
            .or_else(|| self.groups.iter().find_map(|g| g.editors.get(path)));
        let mut icons = self.decl_icons.borrow_mut();
        match editor {
            Some(editor) => {
                let e = editor.read(cx);
                icons.of_open(path, e.revision(), || e.state.buffer.text())
            }
            // A remote project's files are not on this machine's disk.
            None if self.remote_project.is_some() => plain(),
            None => {
                icons.closed(path);
                icons.of_file(path, std::time::Instant::now())
            }
        }
    }

    /// What decided the language of the local file `path`, kept for a second (`SPEC-0004` v6
    /// behaviour 15); `None` in a remote project, whose files are not on this machine.
    fn file_type_icon(&self, path: &Path) -> Option<crate::file_type_icons::FileTypeIcon> {
        if self.remote_project.is_some() || !path.is_absolute() {
            return None;
        }
        Some(
            self.file_type_icons
                .borrow_mut()
                .of(path, std::time::Instant::now()),
        )
    }

    /// Whether the file `path` has a file type override: its icon carries the mark
    /// (`SPEC-0004` v6 behaviour 15).
    pub fn file_type_overridden(&self, path: &Path) -> bool {
        self.file_type_icon(path).is_some_and(|t| t.overridden)
    }

    /// The icon element of the file `path` on its tab and in the project tree: its icon with
    /// the override mark when it has an override.
    pub(crate) fn file_icon_element(&self, path: &Path, cx: &gpui::App) -> gpui::AnyElement {
        crate::file_type_icons::with_override_mark(
            crate::icon_images::icon(self.icon_of(path, cx)),
            self.file_type_overridden(path),
        )
    }

    /// The group with the focus.
    pub fn focused_group(&self) -> GroupId {
        self.focused_group
    }

    /// Whether the last frame drew the focused group's selected tab in the active colours: the
    /// keyboard focus is in its editor and the window is active (`SPEC-0061` v19 behaviour 6a).
    pub fn editor_tabs_active(&self) -> bool {
        self.tabs_have_focus
    }

    /// Whether a group other than the main group has the focus.
    pub fn split_active(&self) -> bool {
        self.focused_group != MAIN
    }

    /// The groups in reading order (`SPEC-0112` v5 behaviour 10).
    pub fn group_ids(&self) -> Vec<GroupId> {
        self.layout.groups()
    }

    /// The file names of group `gid`'s tabs, for checks.
    pub fn group_titles(&self, gid: GroupId) -> Vec<String> {
        self.group_tabs(gid).map(Tabs::titles).unwrap_or_default()
    }

    /// Path of the active tab of the group with the focus.
    pub fn active_path(&self) -> Option<PathBuf> {
        self.group_tabs(self.focused_group)
            .or(Some(&self.tabs))
            .and_then(|t| t.active_path().cloned())
    }

    /// The editor with the focus: in the focused group (SPEC-0112 behaviour 4).
    pub(super) fn active_editor(&self) -> Option<Entity<EditorView>> {
        if self.focused_group == MAIN {
            return self.main_editor();
        }
        let path = self.group_tabs(self.focused_group)?.active_path()?.clone();
        self.group_editor(self.focused_group, &path)
    }

    /// The first group after the main group in reading order.
    fn second_group(&self) -> Option<GroupId> {
        self.layout.groups().into_iter().find(|g| *g != MAIN)
    }

    /// The second group's file names and whether it lies below, if the editor is split (the first
    /// group after the main one in reading order).
    pub fn split_state(&self) -> Option<(Vec<String>, bool)> {
        let gid = self.second_group()?;
        let down = self.layout.place(gid).last() == Some(&"bottom");
        Some((self.group_titles(gid), down))
    }

    /// The second group's tab labels: file name, and ` *` while the document has unsaved changes,
    /// as on the main group's tabs (D8b).
    pub fn split_tab_labels(&self, cx: &gpui::App) -> Vec<String> {
        let Some(tabs) = self.second_group().and_then(|g| self.group_tabs(g)) else {
            return Vec::new();
        };
        tabs.titles()
            .into_iter()
            .zip(tabs.paths())
            .map(|(title, path)| {
                if self.doc_modified(path, cx) {
                    format!("{title} *")
                } else {
                    title
                }
            })
            .collect()
    }

    /// Texts of the main group's view and the focused (else the second) group's view of that
    /// group's active file, for checks.
    pub fn split_texts(&self, cx: &gpui::App) -> Option<(String, String)> {
        let gid = Some(self.focused_group)
            .filter(|g| *g != MAIN)
            .or_else(|| self.second_group())?;
        let path = self.group_tabs(gid)?.active_path()?.clone();
        let text = |e: &Entity<EditorView>| e.read(cx).state.buffer.text();
        Some((
            text(self.editors.get(&path)?),
            text(&self.group_editor(gid, &path)?),
        ))
    }

    /// The main group's file names, for checks.
    pub fn main_tab_titles(&self) -> Vec<String> {
        self.tabs.titles()
    }

    /// Focuses group `gid`.
    pub fn focus_group(&mut self, gid: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout.contains(gid) {
            self.focused_group = gid;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    // ---------------------------------------------------------------- views

    /// A view of `path` in group `gid` sharing `source`'s document, with the caret where it was
    /// (`SPEC-0112` behaviours 1, 3 and 9).
    fn new_twin(
        &mut self,
        gid: GroupId,
        path: &Path,
        source: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (buffer, caret, encoding) = {
            let e = source.read(cx);
            (e.state.buffer.clone(), e.caret(), e.encoding())
        };
        let twin = cx.new(|cx| {
            let mut e = EditorView::new(&buffer.text(), Some(path.to_path_buf()), cx);
            // The second view reads and writes the file as the first does (SPEC-0336).
            e.set_encoding(encoding);
            e.follow_file_attribute();
            e.set_save_all_in_window(true);
            e.adopt(&buffer, &[], cx);
            e.select_range(caret..caret, cx);
            e
        });
        // The second view shows what the first shows (SPEC-0112 v4 behaviour 9).
        let d = source.read(cx).decorations();
        twin.update(cx, |t, cx| t.set_decorations(d, cx));
        // Its own check of the project's unused declarations (SPEC-0098 v12 behaviour 26).
        if !source.read(cx).is_remote_document() && self.remote_project.is_none() {
            let root = self.root.clone();
            twin.update(cx, |t, cx| t.set_project_root(&root, cx));
        }
        self.show_bookmark_marks(&twin, cx);
        self.show_profile_lines(&twin, cx);
        let sub = cx.subscribe_in(&twin, window, {
            let path = path.to_path_buf();
            move |this, editor, event: &EditorEvent, window, cx| match event {
                EditorEvent::GotoDeclaration => this.goto_declaration(window, cx),
                EditorEvent::BreakpointsChanged => {}
                EditorEvent::BookmarkEdits(edits) => this.bookmark_edits(&path, edits, cx),
                EditorEvent::EditBreakpoint(line) => {
                    this.open_breakpoints(Some((path.clone(), *line)), window, cx)
                }
                EditorEvent::AltClickBreakpoint(line) => {
                    this.alt_click_breakpoint(&path, *line, cx)
                }
                EditorEvent::Edited(edits) => {
                    this.keep_preview(&path);
                    this.mirror(&path, Some(editor), edits, cx)
                }
                EditorEvent::Saved if this.remote_project.is_some() => {
                    this.mirror(&path, Some(editor), &[], cx);
                    this.remote_save(&path, cx);
                }
                EditorEvent::Saved => {
                    this.mirror(&path, Some(editor), &[], cx);
                    this.upload_remote(&path, cx);
                    this.editorconfig_saved(&path, cx);
                }
                EditorEvent::SaveAll => this.save_all_action(Some(editor.clone()), cx),
                EditorEvent::RunMarker(line) => this.run_marker_menu(&path, *line, window, cx),
                EditorEvent::DeclarationMark(line) => {
                    this.declaration_mark(&path, *line, window, cx)
                }
                EditorEvent::FrameworkMark(line) => this.framework_mark(&path, *line, window, cx),
                // A click on an annotation of the second view (SPEC-0277 v2 behaviour 13).
                EditorEvent::ShowCommit(id) => this.show_commit_in_log(id, window, cx),
                EditorEvent::ProfileLine(line) => {
                    this.reveal_profile_line(&path, *line, window, cx)
                }
                EditorEvent::ChangeMarker(index, at) => {
                    this.open_change_bar(&path, editor, *index, *at, window, cx)
                }
                EditorEvent::ToggleAnnotations => {
                    if let Some(g) = this
                        .groups
                        .iter()
                        .find(|g| g.editors.get(&path) == Some(editor))
                    {
                        this.focused_group = g.id;
                    }
                    this.toggle_annotations_of(&path, editor, cx)
                }
                EditorEvent::UsagesOf(line) => {
                    if let Some(g) = this
                        .groups
                        .iter()
                        .find(|g| g.editors.get(&path) == Some(editor))
                    {
                        this.focused_group = g.id;
                    }
                    this.find_usages_of_count(&path, editor, *line, window, cx)
                }
                EditorEvent::InlineAlternative(_)
                | EditorEvent::GroupUndo(..)
                | EditorEvent::Undid(..)
                | EditorEvent::ProblemHover(_)
                | EditorEvent::DocHover(_) => {}
                EditorEvent::Note(note) => {
                    this.status = Some(note.clone());
                    cx.notify();
                }
                EditorEvent::RoomComments(line) => {
                    this.room_comment_mark_clicked(&path, *line, window, cx)
                }
            }
        });
        if let Some(g) = self.group_mut(gid) {
            g.editors.insert(path.to_path_buf(), twin);
            g._events.insert(path.to_path_buf(), sub);
        }
    }

    /// Any view of `path`: the main group's first.
    fn any_view(&self, path: &Path) -> Option<Entity<EditorView>> {
        self.editors
            .get(path)
            .cloned()
            .or_else(|| self.twins(path).into_iter().next())
    }

    /// Adds `path` as the active tab of extra group `gid`, with a view sharing the document.
    fn add_to_group(
        &mut self,
        gid: GroupId,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if gid == MAIN {
            self.show_in_main(path, window, cx);
            return;
        }
        let options = Self::tab_options(cx);
        let Some(group) = self.group_mut(gid) else {
            return;
        };
        // The document stays in the other views: nothing is lost when a tab leaves the group.
        let opened = group.tabs.open_with(path, &options, |_| true);
        if let Some(gone) = opened.evicted {
            group.editors.remove(&gone);
            group._events.remove(&gone);
        }
        let has_view = group.editors.contains_key(path);
        if !has_view && let Some(source) = self.any_view(path) {
            self.new_twin(gid, path, &source, window, cx);
        }
    }

    /// Every group that shows `path` (the file itself or a file below the folder `path`) in a
    /// tab, with the tab's path: file operations reach the tabs of all groups (`SPEC-0275`
    /// behaviours 5, 6, 20 and 21; `SPEC-0112` v5 behaviour 10).
    pub(super) fn tabs_under(&self, path: &Path) -> Vec<(GroupId, PathBuf)> {
        let canonical = Self::canonical_or_gone(path);
        let mut out = Vec::new();
        for gid in self.layout.groups() {
            let Some(tabs) = self.group_tabs(gid) else {
                continue;
            };
            for p in tabs.paths() {
                if p.starts_with(&canonical) || p.starts_with(path) {
                    out.push((gid, p.clone()));
                }
            }
        }
        out
    }

    /// `path` as the tabs hold it: canonical; for a file that is gone (renamed, moved, deleted)
    /// its canonical folder with its name.
    fn canonical_or_gone(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| {
            match (
                path.parent().and_then(|d| d.canonicalize().ok()),
                path.file_name(),
            ) {
                (Some(dir), Some(name)) => dir.join(name),
                _ => path.to_path_buf(),
            }
        })
    }

    /// The tabs of a moved or renamed file — `from` itself, or the files below the folder
    /// `from` — show it at `to`, in every group that showed it (`SPEC-0275` behaviours 5 and 21).
    /// The file was written at `to` before; each group keeps its active tab unless that was the
    /// moved file, and the group with the focus keeps it. Returns whether a tab followed.
    pub(super) fn tabs_follow(
        &mut self,
        from: &Path,
        to: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let to = &to.canonicalize().unwrap_or(to.to_path_buf());
        let gone = Self::canonical_or_gone(from);
        let mut moved: Vec<(PathBuf, PathBuf, Vec<GroupId>)> = Vec::new();
        for (gid, old) in self.tabs_under(from) {
            let Some(new) = crate::file_clipboard::moved_path(&old, &gone, to)
                .or_else(|| crate::file_clipboard::moved_path(&old, from, to))
            else {
                continue;
            };
            match moved.iter_mut().find(|m| m.0 == old) {
                Some(m) => m.2.push(gid),
                None => moved.push((old, new, vec![gid])),
            }
        }
        if moved.is_empty() {
            return false;
        }
        let focused = self.focused_group;
        let active: Vec<(GroupId, PathBuf)> = self
            .layout
            .groups()
            .into_iter()
            .filter_map(|g| Some((g, self.group_tabs(g)?.active_path()?.clone())))
            .collect();
        for (old, new, groups) in &moved {
            // The document of the file at its new place, in the main group; the new tab is
            // added before the old one closes, so no group empties on the way.
            self.open(new, window, cx);
            for gid in groups.iter().filter(|g| **g != MAIN) {
                self.add_to_group(*gid, new, window, cx);
            }
            if !groups.contains(&MAIN)
                && let Some(i) = self.tabs.position(new)
            {
                // As after Split and Move: the main group keeps the document, not the tab.
                self.tabs.close(i);
            }
            for gid in groups {
                if let Some(i) = self.group_tabs(*gid).and_then(|t| t.position(old)) {
                    self.close_tab(*gid, i, window, cx);
                }
            }
            self.editors.remove(old);
        }
        for (gid, was) in active {
            let now = moved
                .iter()
                .find(|m| m.0 == was)
                .map_or(was, |m| m.1.clone());
            if let Some(tabs) = self.group_tabs_mut(gid)
                && let Some(i) = tabs.position(&now)
            {
                tabs.activate(i);
            }
        }
        if self.layout.contains(focused) {
            self.focused_group = focused;
        }
        cx.notify();
        true
    }

    /// After a view of `path` changed by `edits` (or was saved), every other view of the file
    /// adopts its buffer, so all show and undo the same document (SPEC-0112 behaviour 3); `from`
    /// is the changed view, `None` for the main group's.
    pub(super) fn mirror(
        &mut self,
        path: &Path,
        from: Option<&Entity<EditorView>>,
        edits: &[ferris_idea_editor::Edit],
        cx: &mut Context<Self>,
    ) {
        let Some(source) = from.cloned().or_else(|| self.editors.get(path).cloned()) else {
            return;
        };
        let targets: Vec<Entity<EditorView>> = self
            .editors
            .get(path)
            .cloned()
            .into_iter()
            .chain(self.twins(path))
            .filter(|e| e.entity_id() != source.entity_id())
            .collect();
        if targets.is_empty() {
            return;
        }
        let buffer = source.read(cx).state.buffer.clone();
        for to in targets {
            to.update(cx, |e, cx| e.adopt(&buffer, edits, cx));
        }
        cx.notify();
    }

    /// The preview tab of `path` becomes a normal tab (it was edited, `SPEC-0059` v9 behaviour 17).
    pub(super) fn keep_preview(&mut self, path: &Path) {
        self.tabs.keep(path);
    }

    // ---------------------------------------------------------------- split commands

    /// Divides the focused group and opens its active file in the new group, right of it or below
    /// it, focused there (`SplitVertically`, `SplitHorizontally`; SPEC-0112 behaviours 1–2, v5
    /// behaviour 10).
    pub fn split_editor(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(source)) = (self.active_path(), self.active_editor()) else {
            self.status = Some("Open a file to split".into());
            cx.notify();
            return;
        };
        let Some(gid) = self.layout.split(self.focused_group, down) else {
            return;
        };
        self.groups.push(Split {
            id: gid,
            tabs: Tabs::default(),
            editors: Default::default(),
            _events: Default::default(),
        });
        let options = Self::tab_options(cx);
        if let Some(g) = self.group_mut(gid) {
            let _ = g.tabs.open_with(&path, &options, |_| true);
        }
        self.new_twin(gid, &path, &source, window, cx);
        self.focused_group = gid;
        self.editor_maximized = false;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Closes the focused group (the next one when the main group has the focus); its files stay
    /// open in the main group (`Unsplit`, SPEC-0112 behaviour 5).
    pub fn unsplit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let gid = if self.focused_group == MAIN {
            self.second_group()
        } else {
            Some(self.focused_group)
        };
        if let Some(gid) = gid {
            self.gather_into_main(gid, window, cx);
        }
        self.focused_group = MAIN;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Gathers every tab into the main group; the focused group's active tab stays active
    /// (`UnsplitAll`, SPEC-0112 v5 behaviour 10).
    pub fn unsplit_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keep = self.active_path();
        for gid in self.layout.groups() {
            if gid != MAIN {
                self.gather_into_main(gid, window, cx);
            }
        }
        if let Some(path) = keep {
            self.show_in_main(&path, window, cx);
        }
        self.focused_group = MAIN;
        self.editor_maximized = false;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Moves every tab of extra group `gid` into the main group and removes the group.
    fn gather_into_main(&mut self, gid: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.groups.iter().position(|g| g.id == gid) else {
            return;
        };
        let paths = self.groups[at].tabs.paths().to_vec();
        for path in &paths {
            self.adopt_into_main(gid, path, window, cx);
        }
        self.groups.retain(|g| g.id != gid);
        self.layout.remove(gid);
    }

    /// Shows `path` in the main group, taking the document of group `gid`'s view when the main
    /// group has none, so unsaved text is never read again from disk.
    fn adopt_into_main(
        &mut self,
        gid: GroupId,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let twin = self.group_editor(gid, path);
        let had = self.editors.contains_key(path);
        self.show_in_main(path, window, cx);
        if !had && let (Some(twin), Some(main)) = (twin, self.editors.get(path).cloned()) {
            let buffer = twin.read(cx).state.buffer.clone();
            main.update(cx, |e, cx| e.adopt(&buffer, &[], cx));
        }
    }

    /// Shows `path` as the main group's active tab, reusing its document when there is one.
    pub(super) fn show_in_main(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focused_group = MAIN;
        if self.editors.contains_key(path) {
            let options = Self::tab_options(cx);
            let opened = self.tabs.open_with(path, &options, |_| true);
            if let Some(gone) = opened.evicted {
                self.editors.remove(&gone);
            }
        } else {
            self.open(path, window, cx);
        }
    }

    /// Moves the active tab of the focused group into the next group and focuses it there; the
    /// document stays as it is (`MoveEditorToOppositeTabGroup`, SPEC-0112 behaviours 6 and 8, v5
    /// behaviour 10).
    pub fn move_to_opposite_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(to) = self.layout.neighbour(self.focused_group) else {
            self.status = Some("The editor is not split".into());
            cx.notify();
            return;
        };
        self.move_active_to(to, window, cx);
    }

    /// Moves the focused group's active tab to group `to` and focuses it there.
    fn move_active_to(&mut self, to: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        let from = self.focused_group;
        let Some(path) = self.active_path() else {
            return;
        };
        if from == to || !self.layout.contains(to) {
            return;
        }
        if to == MAIN {
            self.adopt_into_main(from, &path, window, cx);
        } else {
            self.add_to_group(to, &path, window, cx);
        }
        // Take it out of its group; the main group keeps the document for the other views.
        if from == MAIN {
            let _ = self.tabs.close_active(false);
        } else if let Some(g) = self.group_mut(from) {
            if let Some(i) = g.tabs.position(&path) {
                g.tabs.close(i);
            }
            g.editors.remove(&path);
            g._events.remove(&path);
        }
        self.focused_group = to;
        self.close_emptied(from, window, cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Splits like Split Right/Down and takes the tab out of the focused group
    /// (`SplitAndMoveRight`, `SplitAndMoveDown`; SPEC-0112 behaviour 7).
    pub fn split_and_move(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let from = self.focused_group;
        if self.group_tabs(from).map_or(0, |t| t.paths().len()) < 2 {
            self.status = Some("Only one tab in the group: use Split Right or Split Down".into());
            cx.notify();
            return;
        }
        let Some(path) = self.active_path() else {
            return;
        };
        self.split_editor(down, window, cx);
        if from == MAIN {
            if let Some(i) = self.tabs.position(&path) {
                self.tabs.close(i);
            }
        } else if let Some(g) = self.group_mut(from) {
            if let Some(i) = g.tabs.position(&path) {
                g.tabs.close(i);
            }
            g.editors.remove(&path);
            g._events.remove(&path);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// A group without tabs closes and its neighbour takes its space; an emptied main group takes
    /// the tabs of the next group in reading order, which goes (SPEC-0112 behaviour 8, v5.1
    /// behaviour 12).
    fn close_emptied(&mut self, gid: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.layout.is_split() {
            return;
        }
        let empty = self.group_tabs(gid).is_none_or(|t| t.paths().is_empty());
        if !empty {
            return;
        }
        if gid != MAIN {
            self.groups.retain(|g| g.id != gid);
            self.layout.remove(gid);
            if self.focused_group == gid {
                self.focused_group = MAIN;
            }
            return;
        }
        let Some(next) = self.layout.neighbour(MAIN) else {
            return;
        };
        let active = self.group_tabs(next).and_then(|t| t.active_path().cloned());
        let paths = self
            .group_tabs(next)
            .map(|t| t.paths().to_vec())
            .unwrap_or_default();
        for path in &paths {
            self.adopt_into_main(next, path, window, cx);
        }
        if let Some(a) = active {
            self.show_in_main(&a, window, cx);
        }
        self.groups.retain(|g| g.id != next);
        // The main group takes the place of the group whose tabs it took.
        self.layout.remove(MAIN);
        self.layout.relabel(next, MAIN);
        self.focused_group = MAIN;
    }

    /// Moves the focus to the next (or previous) group in reading order (`NextSplitter`,
    /// `PrevSplitter`; SPEC-0112 behaviour 4, v5 behaviour 10).
    pub fn next_splitter(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout.is_split() {
            self.focused_group = self.layout.next(self.focused_group, forward);
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    // ---------------------------------------------------------------- closing

    /// Remembers `path` for `ReopenClosedTab` with the caret of its view in `gid` and its place
    /// in the row (`SPEC-0059` v9.2 behaviour 23); called before the tab closes.
    fn remember_closed(&mut self, gid: GroupId, path: &Path, cx: &gpui::App) {
        let caret = self
            .group_editor(gid, path)
            .map_or(0, |e| e.read(cx).caret());
        let tabs = self.group_tabs(gid);
        let index = tabs.and_then(|t| t.position(path)).unwrap_or(usize::MAX);
        let pinned = tabs.is_some_and(|t| t.is_pinned(index));
        self.closed_tabs
            .push(ClosedTab::new(path, caret, gid, index, pinned));
    }

    /// Closes tab `index` of group `gid` unconditionally (its close button, a middle click): the
    /// next active tab follows the setting, the file can be reopened, an emptied group closes.
    pub(super) fn close_tab(
        &mut self,
        gid: GroupId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let then = Self::tab_settings(cx).activate_on_close;
        let Some(path) = self
            .group_tabs(gid)
            .and_then(|t| t.paths().get(index).cloned())
        else {
            return;
        };
        self.remember_closed(gid, &path, cx);
        if gid == MAIN {
            self.tabs.close_with(index, then);
            self.editors.remove(&path);
        } else if let Some(g) = self.group_mut(gid) {
            g.tabs.close_with(index, then);
            g.editors.remove(&path);
            g._events.remove(&path);
        }
        self.close_emptied(gid, window, cx);
        if !self.layout.contains(self.focused_group) {
            self.focused_group = MAIN;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// `CloseContent` on the focused group's active tab: a tab whose unsaved changes would be
    /// lost closes only on the second try (behaviour 5).
    pub(super) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.focused_group;
        let Some(path) = self.active_path() else {
            return;
        };
        // Another view keeping the document: nothing is lost.
        let others = self.views_of(&path).len() > 1;
        let modified = !others && self.doc_modified(&path, cx);
        let then = Self::tab_settings(cx).activate_on_close;
        let caret = self.active_editor().map_or(0, |e| e.read(cx).caret());
        let (index, pinned) = self
            .group_tabs(gid)
            .and_then(|t| t.active().map(|i| (i, t.is_pinned(i))))
            .unwrap_or((usize::MAX, false));
        let closed = if gid == MAIN {
            self.tabs.close_active_with(modified, then)
        } else {
            self.group_mut(gid)
                .and_then(|g| g.tabs.close_active_with(modified, then))
        };
        match closed {
            Some(path) => {
                self.closed_tabs
                    .push(ClosedTab::new(&path, caret, gid, index, pinned));
                if gid == MAIN {
                    self.editors.remove(&path);
                } else if let Some(g) = self.group_mut(gid) {
                    g.editors.remove(&path);
                    g._events.remove(&path);
                }
                self.status = None;
                self.close_emptied(gid, window, cx);
                if !self.layout.contains(self.focused_group) {
                    self.focused_group = MAIN;
                }
            }
            None if modified => {
                self.status = Some("Unsaved changes: save, or close again to discard them.".into());
            }
            None => {}
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Closes the tabs of `scope` around the focused group's active tab; tabs with unsaved
    /// changes are asked about once, in one dialog (`SPEC-0059` v9 behaviours 18–20).
    pub fn close_several(
        &mut self,
        scope: CloseScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let gid = self.focused_group;
        let Some(tabs) = self.group_tabs(gid) else {
            return;
        };
        let index = tabs.active().unwrap_or(0);
        // A file another group still shows loses nothing when this tab closes.
        let plan = tabs.plan_close(scope, index, |p| {
            self.views_of(p).len() <= 1 && self.doc_modified(p, cx)
        });
        self.close_in_group(gid, &plan.close, window, cx);
        if plan.ask.is_empty() {
            return;
        }
        let names: Vec<String> = plan.ask.iter().map(|p| file_name(p)).collect();
        let message = format!(
            "Save changes to {} before closing?",
            if names.len() == 1 {
                names[0].clone()
            } else {
                format!("{} files", names.len())
            }
        );
        let detail = names.join("\n");
        let buttons = [
            gpui::PromptButton::ok("Save All"),
            gpui::PromptButton::new("Don't Save"),
            gpui::PromptButton::cancel("Cancel"),
        ];
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &message,
            Some(&detail),
            &buttons,
            cx,
        );
        let ask = plan.ask;
        cx.spawn_in(window, async move |this, cx| {
            let chosen = answer.await.ok();
            let _ = this.update_in(cx, |this, window, cx| match chosen {
                Some(0) => {
                    let mut left = Vec::new();
                    for p in &ask {
                        if !this.save_doc(p, cx) {
                            left.push(p.clone());
                        }
                    }
                    let saved: Vec<PathBuf> =
                        ask.iter().filter(|p| !left.contains(p)).cloned().collect();
                    this.close_in_group(gid, &saved, window, cx);
                    if !left.is_empty() {
                        this.status = Some(format!(
                            "Could not save {}; left open",
                            left.iter()
                                .map(|p| file_name(p))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
                Some(1) => this.close_in_group(gid, &ask, window, cx),
                _ => {}
            });
        })
        .detach();
    }

    /// Saves the document of `path`; whether it has no unsaved changes afterwards.
    fn save_doc(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let Some(view) = self.any_view(path) else {
            return true;
        };
        view.update(cx, |e, cx| {
            if e.is_modified() {
                e.run_action("SaveDocument", cx);
            }
        });
        self.mirror(path, Some(&view), &[], cx);
        !view.read(cx).is_modified()
    }

    /// Save All from the menu, its key or Find Action (`SPEC-0058` v4 behaviour 12): every editor
    /// of both groups with unsaved changes, each once and as `SaveDocument` saves it
    /// ([`Self::save_all_editors`]); `front`, the editor it was chosen in, is written also without
    /// changes, as before. What stays unsaved keeps its editor's note (`SPEC-0261` behaviour 3). A
    /// remote project's documents go to the host one by one as ⌘S sends one, each with its
    /// conflict check (`SPEC-0317` v3 behaviour 10).
    pub(super) fn save_all_action(
        &mut self,
        front: Option<Entity<EditorView>>,
        cx: &mut Context<Self>,
    ) {
        // Taken before the others are saved, so a file is never written twice in one Save All:
        // the second save of a file changed on disk would overwrite it.
        let front = front.filter(|e| !e.read(cx).is_modified());
        if self.remote_project.is_some() {
            let views = crate::split_model::views_to_save(
                self.editors.keys(),
                self.groups.iter().flat_map(|g| g.editors.keys()),
            );
            for (path, from_split) in views {
                let view = if from_split {
                    self.twins(&path).into_iter().next()
                } else {
                    self.editors.get(&path).cloned()
                };
                if let Some(view) = view.filter(|v| v.read(cx).is_modified()) {
                    view.update(cx, |e, cx| e.run_action("SaveDocument", cx));
                }
            }
        } else {
            self.save_all_editors(cx);
        }
        if let Some(front) = front {
            front.update(cx, |e, cx| e.run_action("SaveDocument", cx));
        }
    }

    /// Closes the tabs of `paths` in group `gid` (remembered for reopening).
    fn close_in_group(
        &mut self,
        gid: GroupId,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        for p in paths {
            self.remember_closed(gid, p, cx);
        }
        if gid == MAIN {
            for p in self.tabs.close_paths(paths) {
                self.editors.remove(&p);
            }
        } else if let Some(g) = self.group_mut(gid) {
            for p in g.tabs.close_paths(paths) {
                g.editors.remove(&p);
                g._events.remove(&p);
            }
        }
        self.close_emptied(gid, window, cx);
        if !self.layout.contains(self.focused_group) {
            self.focused_group = MAIN;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Reopens the most recently closed file at its last caret (`ReopenClosedTab`, `SPEC-0059`
    /// v9 behaviour 19), at the place it had: its group if that still exists, else the main
    /// group, at its former index (v9.2 behaviour 23).
    pub fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = |t: &ClosedTab, layout: &Layout| -> GroupId {
            if layout.contains(t.group) {
                t.group
            } else {
                MAIN
            }
        };
        let open: Vec<(GroupId, Vec<PathBuf>)> =
            std::iter::once((MAIN, self.tabs.paths().to_vec()))
                .chain(self.groups.iter().map(|g| (g.id, g.tabs.paths().to_vec())))
                .collect();
        let layout = &self.layout;
        let Some(closed) = self.closed_tabs.pop(|t| {
            let gid = target(t, layout);
            open.iter()
                .any(|(g, paths)| *g == gid && paths.contains(&t.path))
        }) else {
            self.status = Some("No closed tab to reopen".into());
            cx.notify();
            return;
        };
        let path = closed.path.clone();
        if !path.exists() && self.remote_project.is_none() {
            self.status = Some(format!("{} no longer exists", path.display()));
            cx.notify();
            return;
        }
        let gid = target(&closed, &self.layout);
        if gid == MAIN {
            self.focused_group = MAIN;
            self.open(&path, window, cx);
        } else {
            // Another group shows a view sharing the document: open it in the main group first
            // when no view of the file exists, and leave only its document there.
            let fresh = self.any_view(&path).is_none();
            let main_had = self.tabs.position(&path).is_some();
            if fresh {
                self.open(&path, window, cx);
            }
            self.add_to_group(gid, &path, window, cx);
            if fresh
                && !main_had
                && let Some(i) = self.tabs.position(&path)
            {
                self.tabs.close(i);
            }
            self.focused_group = gid;
        }
        let options = Self::tab_options(cx);
        if let Some(tabs) = self.group_tabs_mut(gid) {
            tabs.restore_place(&path, closed.index, closed.pinned, &options);
        }
        if let Some(e) = self.group_editor(gid, &path) {
            let len = e.read(cx).state.buffer.text().chars().count();
            let at = closed.caret.min(len);
            e.update(cx, |e, cx| e.select_range(at..at, cx));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Pins or unpins the focused group's active tab (`PinActiveEditorTab`, `SPEC-0059` v9
    /// behaviour 20).
    pub fn toggle_pin_active(&mut self, cx: &mut Context<Self>) {
        let gid = self.focused_group;
        if let Some(tabs) = self.group_tabs_mut(gid)
            && let Some(i) = tabs.active()
        {
            let pinned = tabs.toggle_pin(i);
            self.status = Some(if pinned { "Tab pinned" } else { "Tab unpinned" }.into());
        }
        cx.notify();
    }

    /// Whether the main group's tab of `path` is pinned (checks).
    pub fn tab_pinned(&self, path: &Path) -> bool {
        self.tabs
            .position(path)
            .is_some_and(|i| self.tabs.is_pinned(i))
    }

    /// Closes every tab of every group without asking, unsaved changes included (self-test
    /// set-up only).
    pub fn close_all_tabs_unchecked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.groups.clear();
        self.layout = Layout::default();
        self.focused_group = MAIN;
        self.tabs = Tabs::default();
        self.editors.clear();
        self.editor_maximized = false;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Whether group `gid`'s active tab was painted fully inside its tab row (`SPEC-0059` v9
    /// behaviour 15); `None` before the row was laid out. A pinned tab in the separate row of
    /// pinned tabs does not scroll and counts as visible.
    pub fn active_tab_visible(&self, gid: GroupId) -> Option<bool> {
        let tabs = self.group_tabs(gid)?;
        let active = tabs.active()?;
        let scrolls = self.tab_scrolls.borrow();
        let s = scrolls.get(&gid)?;
        let Some(item) = s.shown.iter().position(|&i| i == active) else {
            return Some(true);
        };
        let row = s.handle.bounds();
        let tab = s.handle.bounds_for_item(item)?;
        if f32::from(row.size.width) <= 0.0 {
            return None;
        }
        // The toolkit keeps the children's bounds as laid out, before the row's scroll offset.
        Some(crate::tabs::fully_visible(
            f32::from(tab.origin.x - row.origin.x),
            f32::from(tab.size.width),
            f32::from(row.size.width),
            -f32::from(s.handle.offset().x),
        ))
    }

    /// Whether group `gid`'s tabs overflow its row (the chevron shows).
    pub fn tab_bar_overflows(&self, gid: GroupId) -> bool {
        self.tab_scrolls
            .borrow()
            .get(&gid)
            .is_some_and(|s| f32::from(s.handle.max_offset().width) > 0.5)
    }

    /// How many closed files `ReopenClosedTab` can bring back.
    pub fn reopenable_tabs(&self) -> usize {
        self.closed_tabs.len()
    }

    /// Runs the tab actions of `SPEC-0059` v9 behaviour 19 and the split actions of `SPEC-0112`;
    /// returns whether `id` is one of them.
    pub(super) fn run_tab_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(scope) = CloseScope::of_action(id) {
            self.close_several(scope, window, cx);
            return true;
        }
        match id {
            "SplitVertically" => self.split_editor(false, window, cx),
            "SplitHorizontally" => self.split_editor(true, window, cx),
            "Unsplit" => self.unsplit(window, cx),
            "UnsplitAll" => self.unsplit_all(window, cx),
            "MoveEditorToOppositeTabGroup" => self.move_to_opposite_group(window, cx),
            "SplitAndMoveRight" => self.split_and_move(false, window, cx),
            "SplitAndMoveDown" => self.split_and_move(true, window, cx),
            "NextSplitter" => self.next_splitter(true, window, cx),
            "PrevSplitter" => self.next_splitter(false, window, cx),
            "ReopenClosedTab" => self.reopen_closed_tab(window, cx),
            "PinActiveEditorTab" => self.toggle_pin_active(cx),
            "TabList" => self.open_tab_list(self.focused_group, None, window, cx),
            "Switcher" => self.open_switcher(false, window, cx),
            "MaximizeEditorInSplit" => self.toggle_maximize_editor(window, cx),
            "FerrisIDEA.ConfigureEditorTabs" => {
                self.open_settings(crate::settings_model::Page::EditorTabs, window, cx)
            }
            "FerrisIDEA.OpenTabInNewWindow" => {
                self.status = Some(crate::context_menu::NO_NEW_WINDOW.into());
                cx.notify();
            }
            _ => return false,
        }
        true
    }

    // ---------------------------------------------------------------- tab list and Switcher

    /// Opens the list of group `gid`'s tabs, hidden ones included (the chevron, `TabList`;
    /// `SPEC-0059` v9 behaviour 15).
    pub fn open_tab_list(
        &mut self,
        gid: GroupId,
        at: Option<gpui::Point<gpui::Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tabs) = self.group_tabs(gid) else {
            return;
        };
        let titles = tabs.titles();
        let paths = tabs.paths().to_vec();
        let active = tabs.active();
        let rows: Vec<TabListRow> = titles
            .into_iter()
            .zip(&paths)
            .map(|(label, p)| TabListRow {
                icon: self.icon_of(p, cx),
                label,
                modified: self.doc_modified(p, cx),
                ..TabListRow::default()
            })
            .collect();
        let targets = paths
            .into_iter()
            .map(|p| Some(ListTarget::Tab(gid, p)))
            .collect();
        self.show_tab_list(
            "Open Tabs".into(),
            rows,
            targets,
            active,
            false,
            at,
            window,
            cx,
        );
    }

    /// The digit of `action`'s shortcut in the keymap in use, when it has a shortcut of one
    /// keystroke whose key is a digit (the Switcher's characters, `SPEC-0059` v9.4 behaviour 27b).
    fn shortcut_digit(action: &str) -> Option<char> {
        crate::keymaps::current()
            .shortcuts(action)
            .iter()
            .find_map(|s| match s {
                ferris_idea_ui::keymap::Shortcut::Keyboard {
                    first,
                    second: None,
                } => {
                    let mut key = first.key.chars();
                    match (key.next(), key.next()) {
                        (Some(d), None) if d.is_ascii_digit() => Some(d),
                        _ => None,
                    }
                }
                _ => None,
            })
    }

    /// The character of `action`'s shortcut when that is ⌃ with one character: the menu bar
    /// may take such a key before the Switcher sees it (behaviour 27b).
    fn control_character(action: &str) -> Option<char> {
        use ferris_idea_ui::keymap::{Modifier, Shortcut};
        crate::keymaps::current()
            .shortcuts(action)
            .iter()
            .find_map(|s| match s {
                Shortcut::Keyboard {
                    first,
                    second: None,
                } if first.modifiers == [Modifier::Ctrl] => {
                    let mut key = first.key.chars();
                    match (key.next(), key.next()) {
                        (Some(ch), None) => Some(ch),
                        _ => None,
                    }
                }
                _ => None,
            })
    }

    /// While the Switcher is open, an action whose shortcut is ⌃ with a character chooses the
    /// Switcher's row with that character instead of running; returns whether it did.
    pub(super) fn switcher_takes(&mut self, action: &str, cx: &mut Context<Self>) -> bool {
        let Some(view) = self
            .tab_list
            .as_ref()
            .map(|p| p.view.clone())
            .filter(|v| v.read(cx).is_switcher())
        else {
            return false;
        };
        Self::control_character(action)
            .is_some_and(|ch| view.update(cx, |v, cx| v.choose_character(ch, cx)))
    }

    /// Opens the Switcher (`Switcher`, `SPEC-0059` v9 behaviour 19, v9.4 behaviour 27): the
    /// recently used files in the right column, the tool windows with their characters and
    /// Recent Locations in the left one. The file used before the current one is selected, held
    /// ⌃ moves with Tab, releasing ⌃ opens the selection.
    pub fn open_switcher(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popup) = &self.tab_list
            && popup.view.read(cx).is_switcher()
        {
            let view = popup.view.clone();
            view.update(cx, |v, cx| v.step(!back, cx));
            return;
        }
        let mut files: Vec<PathBuf> = self.tabs.recent();
        for g in &self.groups {
            for p in g.tabs.recent() {
                if !files.contains(&p) {
                    files.push(p);
                }
            }
        }
        for p in self.recent.files() {
            let p = if p.is_absolute() {
                p.clone()
            } else {
                self.root.join(p)
            };
            if !files.contains(&p) {
                files.push(p);
            }
        }
        files.truncate(30);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let names = crate::tabs::switcher_names(&files);
        let mut rows = Vec::new();
        let mut targets = Vec::new();
        for (p, label) in files.iter().zip(names) {
            rows.push(TabListRow {
                label,
                icon: self.icon_of(p, cx),
                modified: self.doc_modified(p, cx),
                folder: crate::tabs::switcher_folder(p, &self.root, home.as_deref()),
                split: true,
                ..TabListRow::default()
            });
            targets.push(Some(ListTarget::File(p.clone())));
        }
        let titles: Vec<&str> = ToolWindow::ALL.iter().map(|w| w.title()).collect();
        let digits: Vec<Option<char>> = ToolWindow::ALL
            .iter()
            .map(|w| Self::shortcut_digit(w.action()))
            .collect();
        let characters = crate::tabs::switcher_characters(&titles, &digits);
        for (w, character) in ToolWindow::ALL.into_iter().zip(characters) {
            rows.push(TabListRow {
                label: w.title().into(),
                icon: w.icon(),
                left: true,
                character,
                ..TabListRow::default()
            });
            targets.push(Some(ListTarget::Tool(w)));
        }
        rows.push(TabListRow {
            label: "Recent Locations".into(),
            icon: "fileTypes/text",
            left: true,
            shortcut: crate::keymaps::current()
                .shortcut_label("RecentLocations", cfg!(target_os = "macos")),
            separated: true,
            ..TabListRow::default()
        });
        targets.push(Some(ListTarget::Action("RecentLocations")));
        let first = if back {
            files.len().saturating_sub(1)
        } else {
            usize::from(files.len() > 1)
        };
        self.show_tab_list(
            "Switcher".into(),
            rows,
            targets,
            Some(first),
            true,
            None,
            window,
            cx,
        );
    }

    /// The open Switcher for checks: `left` and `right` hold the rows of the columns (`label`,
    /// `character`, `shortcut`), `selected` the selected row's label and `folder` the folder
    /// line.
    pub fn switcher_state(&self, cx: &gpui::App) -> Option<serde_json::Value> {
        let view = self.tab_list.as_ref()?.view.read(cx);
        if !view.is_switcher() {
            return None;
        }
        let column = |left: bool| -> Vec<serde_json::Value> {
            view.column(left)
                .into_iter()
                .filter_map(|i| view.row(i))
                .map(|r| {
                    serde_json::json!({
                        "label": r.label,
                        "character": r.character.map(String::from),
                        "shortcut": r.shortcut,
                        "modified": r.modified,
                    })
                })
                .collect()
        };
        Some(serde_json::json!({
            "left": column(true),
            "right": column(false),
            "selected": view.selected().map(|i| view.label(i)),
            "folder": view.folder_line(),
        }))
    }

    /// Opens the Switcher's file `i` in a new group right of the main group, focused there; the
    /// main group gets no tab for a file it did not show (behaviour 27e).
    fn tab_list_chosen_in_split(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .tab_list
            .as_ref()
            .and_then(|p| p.targets.get(i).cloned().flatten());
        let Some(ListTarget::File(path)) = target else {
            self.tab_list_chosen(i, window, cx);
            return;
        };
        self.tab_list = None;
        let in_main = self.tabs.position(&path).is_some();
        self.focused_group = MAIN;
        self.open(&path, window, cx);
        if self.tabs.position(&path).is_none() {
            // The file could not be opened; the status bar says why.
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        if in_main || self.tabs.paths().len() < 2 {
            self.split_editor(false, window, cx);
        } else {
            self.split_and_move(false, window, cx);
        }
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn show_tab_list(
        &mut self,
        title: String,
        rows: Vec<TabListRow>,
        targets: Vec<Option<ListTarget>>,
        selected: Option<usize>,
        switcher: bool,
        at: Option<gpui::Point<gpui::Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| TabListView::new(title, rows, selected, switcher, cx));
        let events = cx.subscribe_in(
            &view,
            window,
            |this, _, e: &TabListEvent, window, cx| match e {
                TabListEvent::Chosen(i) => this.tab_list_chosen(*i, window, cx),
                TabListEvent::ChosenInSplit(i) => this.tab_list_chosen_in_split(*i, window, cx),
                TabListEvent::Close => {
                    this.tab_list = None;
                    this.focus_active(window, cx);
                    cx.notify();
                }
            },
        );
        window.focus(&view.read(cx).focus());
        self.context_menu = None;
        self.tab_list = Some(TabListPopup {
            view,
            targets,
            at,
            _events: events,
        });
        cx.notify();
    }

    fn tab_list_chosen(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .tab_list
            .take()
            .and_then(|p| p.targets.get(i).cloned().flatten());
        match target {
            Some(ListTarget::Tab(gid, path)) => {
                if let Some(tabs) = self.group_tabs_mut(gid)
                    && let Some(i) = tabs.position(&path)
                {
                    tabs.activate(i);
                }
                if gid == MAIN {
                    self.diff = None;
                    self.diff_file = None;
                }
                self.focused_group = gid;
                self.focus_active(window, cx);
            }
            Some(ListTarget::File(path)) => {
                let shown = self.layout.groups().into_iter().find(|g| {
                    self.group_tabs(*g)
                        .is_some_and(|t| t.position(&path).is_some())
                });
                match shown {
                    Some(gid) => {
                        if let Some(tabs) = self.group_tabs_mut(gid)
                            && let Some(i) = tabs.position(&path)
                        {
                            tabs.activate(i);
                        }
                        if gid == MAIN {
                            self.close_diff_for_file_tab();
                        }
                        self.focused_group = gid;
                        self.focus_active(window, cx);
                    }
                    None => {
                        self.focused_group = MAIN;
                        self.open(&path, window, cx);
                    }
                }
            }
            Some(ListTarget::Tool(w)) => self.menu_action(w.action(), window, cx),
            Some(ListTarget::Action(id)) => self.menu_action(id, window, cx),
            None => self.focus_active(window, cx),
        }
        cx.notify();
    }

    /// The rows of the open tab list or Switcher (checks).
    pub fn tab_list_labels(&self, cx: &gpui::App) -> Option<Vec<String>> {
        self.tab_list.as_ref().map(|p| p.view.read(cx).labels())
    }

    /// Chooses the row labelled `label` of the open tab list, as a click does (checks).
    pub fn choose_tab_list_row(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(popup) = &self.tab_list else {
            return false;
        };
        let view = popup.view.read(cx);
        let Some(i) = view.visible().into_iter().find(|&i| view.label(i) == label) else {
            return false;
        };
        self.tab_list_chosen(i, window, cx);
        true
    }

    /// The tab list or Switcher popup, drawn over the window.
    pub(super) fn tab_list_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let popup = self.tab_list.as_ref()?;
        let view = popup.view.clone();
        let body = div()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.tab_list = None;
                this.focus_active(window, cx);
                cx.notify();
            }))
            .child(view);
        Some(match popup.at {
            Some(at) => gpui::deferred(
                gpui::anchored()
                    .anchor(gpui::Corner::TopRight)
                    .position(at)
                    .snap_to_window()
                    .child(body),
            )
            .into_any_element(),
            None => div()
                .absolute()
                .top(px(super::toolbar_height() + 40.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(body)
                .into_any_element(),
        })
    }

    // ---------------------------------------------------------------- maximize

    /// Hides the tool windows and the other groups, or shows them again
    /// (`MaximizeEditorInSplit`, `SPEC-0059` v9 behaviour 19).
    pub fn toggle_maximize_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_maximized {
            self.editor_maximized = false;
            if std::mem::take(&mut self.maximize_hid_tools) {
                self.hide_all_tool_windows(window, cx);
            }
        } else {
            self.editor_maximized = true;
            self.maximize_hid_tools = !self.shown_tool_windows().is_empty() || self.show_tree;
            if self.maximize_hid_tools {
                self.hide_all_tool_windows(window, cx);
            }
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Whether the editor is maximized.
    pub fn editor_maximized(&self) -> bool {
        self.editor_maximized
    }

    // ---------------------------------------------------------------- remembered per project

    /// The editor area as remembered (`SPEC-0059` v9.1 behaviour 22e).
    pub(super) fn saved_editors(&self) -> SavedEditors {
        let mut groups = vec![SavedGroup {
            id: MAIN,
            tabs: self.tabs.saved(),
        }];
        for g in &self.groups {
            groups.push(SavedGroup {
                id: g.id,
                tabs: g.tabs.saved(),
            });
        }
        let mut shown: Vec<String> = self
            .shown_tool_windows()
            .into_iter()
            .map(|w| w.id().to_owned())
            .collect();
        if self.show_tree && !shown.iter().any(|id| id == ToolWindow::Project.id()) {
            shown.push(ToolWindow::Project.id().to_owned());
        }
        SavedEditors {
            layout: self.layout.root().clone(),
            groups,
            focused: self.focused_group,
            tool_windows: Some(shown),
        }
    }

    /// Writes the editor area to `.ferris-idea/editor-tabs.json` (not for a remote project, whose
    /// files live on the host).
    pub(super) fn save_editors(&self) {
        if self.editors_restored && self.remote_project.is_none() {
            let _ = self.saved_editors().save(&self.root);
        }
    }

    /// Opens the remembered tabs, pins, groups and layout once, when the window shows its first
    /// frame with no tab open yet.
    pub fn restore_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editors_restored {
            return;
        }
        self.editors_restored = true;
        if self.remote_project.is_some() {
            return;
        }
        let Some(saved) = SavedEditors::load(&self.root) else {
            return;
        };
        // The tool windows open at quitting open again (SPEC-0061 v13 behaviour 59), also when
        // the window opened with a file of its own.
        if let Some(ids) = &saved.tool_windows {
            self.restore_tool_windows(
                &crate::editor_layout::restored_tool_windows(ids),
                window,
                cx,
            );
        }
        if !self.tabs.paths().is_empty() {
            return;
        }
        let exists = |p: &Path| p.is_file();
        // Every document first opens in the main group (its language services live there).
        let main = saved.groups.iter().find(|g| g.id == MAIN);
        let mut wanted: Vec<PathBuf> = Vec::new();
        for g in &saved.groups {
            for p in g.tabs.paths.iter().filter(|p| exists(p)) {
                if !wanted.contains(p) {
                    wanted.push(p.clone());
                }
            }
        }
        if wanted.is_empty() {
            return;
        }
        for p in &wanted {
            self.open(p, window, cx);
        }
        let opened: Vec<PathBuf> = self.editors.keys().cloned().collect();
        let in_main = |p: &Path| opened.iter().any(|o| o == p);
        self.tabs = main
            .map(|g| Tabs::restored(&g.tabs, |p| exists(p) && in_main(p)))
            .unwrap_or_default();
        if let Some(layout) = Layout::from_node(saved.layout.clone()) {
            self.layout = layout;
            for gid in self.layout.groups().into_iter().filter(|g| *g != MAIN) {
                let Some(group) = saved.groups.iter().find(|g| g.id == gid) else {
                    continue;
                };
                self.groups.push(Split {
                    id: gid,
                    tabs: Tabs::restored(&group.tabs, |p| exists(p) && in_main(p)),
                    editors: Default::default(),
                    _events: Default::default(),
                });
                let paths = self
                    .group_tabs(gid)
                    .map(|t| t.paths().to_vec())
                    .unwrap_or_default();
                for p in paths {
                    if let Some(source) = self.editors.get(&p).cloned() {
                        self.new_twin(gid, &p, &source, window, cx);
                    }
                }
            }
            // Groups that came back empty close.
            for gid in self.layout.groups() {
                if gid != MAIN && self.group_tabs(gid).is_none_or(|t| t.paths().is_empty()) {
                    self.groups.retain(|g| g.id != gid);
                    self.layout.remove(gid);
                }
            }
            if self.tabs.paths().is_empty() {
                self.close_emptied(MAIN, window, cx);
            }
        }
        self.focused_group = if self.layout.contains(saved.focused) {
            saved.focused
        } else {
            MAIN
        };
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Shows exactly the tool windows `shown`: the others are hidden, a window that cannot be shown
    /// now (Run or Debug without a session) stays hidden (SPEC-0061 v13 behaviour 59).
    fn restore_tool_windows(
        &mut self,
        shown: &[ToolWindow],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for w in self.shown_tool_windows() {
            if !shown.contains(&w) {
                self.hide_tool_window(w);
            }
        }
        self.show_tree = shown.contains(&ToolWindow::Project);
        for w in shown {
            if *w != ToolWindow::Project
                && self.tool_window_exists(*w)
                && !self.tool_window_shown(*w)
            {
                self.toggle_tool_window(*w, window, cx);
            }
        }
    }

    /// Writes and reads the editor area again as when the project opens (self-test): every tab is
    /// closed first.
    pub fn reload_saved_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editors_restored = true;
        self.save_editors();
        self.tabs = Tabs::default();
        self.editors.clear();
        self.groups.clear();
        self.layout = Layout::default();
        self.focused_group = MAIN;
        self.editors_restored = false;
        self.restore_editors(window, cx);
    }

    // ---------------------------------------------------------------- drawing

    /// The tab bar of group `gid` (`SPEC-0059` behaviours 3, 9, v9 behaviours 15–20).
    pub(super) fn group_tab_bar(&self, gid: GroupId, cx: &mut Context<Self>) -> gpui::AnyElement {
        let settings = Self::tab_settings(cx);
        let Some(tabs) = self.group_tabs(gid) else {
            return div().into_any_element();
        };
        if settings.placement == TabPlacement::None {
            return div().into_any_element();
        }
        let main = gid == MAIN;
        let first_extra = self.second_group() == Some(gid);
        let titles = tabs.titles_with(
            settings.show_extension,
            settings.show_folder_for_equal_names,
        );
        let paths: Vec<PathBuf> = tabs.paths().to_vec();
        let active = tabs.active();
        let pinned = tabs.pinned_flags();
        let preview: Vec<bool> = (0..paths.len()).map(|i| tabs.is_preview(i)).collect();
        let modified: Vec<bool> = paths.iter().map(|p| self.doc_modified(p, cx)).collect();
        let colors: Vec<Option<gpui::Hsla>> = paths.iter().map(|p| self.file_color(p)).collect();
        // A file with errors is underlined on its tab (SPEC-0059 v4 behaviour 10).
        let problems = tabs.problem_flags(&self.problem_files(cx));
        let error_color = super::problem_mark_color();
        // Active colours only while the keyboard focus is in this group's editor (SPEC-0061 v19
        // behaviour 6a).
        let focused = self.focused_group == gid && self.tabs_have_focus;
        let diff_open = main && self.diff.is_some();
        let rel: Vec<String> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(&self.root)
                    .unwrap_or(p)
                    .display()
                    .to_string()
            })
            .collect();
        let p = Palette::current();
        let make_tab = |i: usize, cx: &mut Context<Self>| -> gpui::AnyElement {
            let on = Some(i) == active && !diff_open;
            let label = if modified[i] && settings.mark_modified {
                format!("{} *", titles[i])
            } else {
                titles[i].clone()
            };
            let hit = if main {
                format!("tab-{i}")
            } else if first_extra {
                format!("split-tab-{i}")
            } else {
                format!("group{gid}-tab-{i}")
            };
            let id: gpui::ElementId = if main {
                ("tab", i).into()
            } else {
                ("split-tab", i).into()
            };
            let hover_group = format!("tab-hover-{gid}-{i}");
            let show_close = settings.close_button != CloseButtonPlace::None || pinned[i];
            let close = show_close.then(|| {
                let pin = pinned[i];
                div()
                    .id(("tab-close", gid * 1000 + i))
                    .when(!pin, |d| d.tip(declared!(EDITOR_TABS, "tab-close")))
                    .group(format!("close-{gid}-{i}"))
                    .relative()
                    .flex_none()
                    .size(px(16.))
                    .when(!pin && !settings.close_button_on_all && !on, |d| {
                        d.opacity(0.)
                            .group_hover(hover_group.clone(), |s| s.opacity(1.))
                    })
                    .child(icon(if pin {
                        "ferris-idea/pin"
                    } else {
                        "general/closeSmall"
                    }))
                    .when(!pin, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .opacity(0.)
                                .group_hover(format!("close-{gid}-{i}"), |s| s.opacity(1.))
                                .child(icon("general/closeSmallHovered")),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if pin {
                            // A click on the pin unpins (behaviour 20).
                            if let Some(t) = this.group_tabs_mut(gid) {
                                t.set_pinned(i, false);
                            }
                            cx.notify();
                        } else {
                            this.close_tab(gid, i, window, cx);
                        }
                    }))
            });
            let left_close = settings.close_button == CloseButtonPlace::Left && !pinned[i];
            let name = div()
                .when_some(colors[i], |d, color| d.text_color(color))
                .when(problems[i], |d| problem_mark(d, error_color))
                .when(preview[i], |d| d.italic())
                .whitespace_nowrap()
                .when(settings.rows == TabRows::Squeeze, |d| {
                    d.overflow_hidden().text_ellipsis()
                })
                .child(label);
            let content = if settings.show_icon {
                crate::metrics::tab_content(self.file_icon_element(&paths[i], cx), name)
            } else {
                div().flex().items_center().child(name)
            };
            let tab = crate::metrics::tab(id, on, focused)
                .group(hover_group.clone())
                .pr(px(ferris_idea_ui::metrics::TAB_CLOSE_PADDING - 1.))
                .relative()
                .flex_none()
                .when(settings.rows == TabRows::Squeeze, |d| {
                    d.flex_shrink().min_w(px(48.)).overflow_hidden()
                });
            let tip = if settings.path_tooltip {
                rel[i].clone()
            } else {
                String::new()
            };
            let tab = tab.tip_text(hit, ferris_idea_ui::tooltip::Tip::plain(tip));
            let (close_left, close_right) = if left_close {
                (close, None)
            } else {
                (None, close)
            };
            tab.children(close_left)
                .child(content)
                .children(close_right)
                .on_mouse_up(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| this.close_tab(gid, i, window, cx)),
                )
                .map(|d| {
                    crate::mouse::on_secondary_click(
                        d,
                        cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                            this.open_tab_menu(gid, i, e.position, cx);
                        }),
                    )
                })
                .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                    if gid == MAIN {
                        // A file tab closes the diff viewer's tab (SPEC-0084 v2 behaviour 6).
                        this.diff = None;
                        this.diff_file = None;
                    }
                    if let Some(t) = this.group_tabs_mut(gid) {
                        t.activate(i);
                        // A double click keeps the preview tab (behaviour 17).
                        if e.click_count() > 1
                            && let Some(path) = t.paths().get(i).cloned()
                        {
                            t.keep(&path);
                        }
                    }
                    this.focused_group = gid;
                    this.focus_active(window, cx);
                    cx.notify();
                }))
                .into_any_element()
        };
        let order: Vec<usize> = (0..paths.len()).collect();
        let (pinned_tabs, other_tabs): (Vec<usize>, Vec<usize>) = if settings.pinned_row {
            order.into_iter().partition(|&i| pinned[i])
        } else {
            (Vec::new(), order)
        };
        let pinned_row = (!pinned_tabs.is_empty()).then(|| {
            crate::metrics::tab_bar()
                .children(pinned_tabs.iter().map(|&i| make_tab(i, cx)))
                .into_any_element()
        });
        let mut row_tabs: Vec<gpui::AnyElement> =
            other_tabs.iter().map(|&i| make_tab(i, cx)).collect();
        if main && let Some(diff) = self.diff_tab(cx) {
            row_tabs.push(diff);
        }
        let row = match settings.rows {
            TabRows::Scroll => {
                let scroll = self.tab_scroll_for(gid, &other_tabs, active, &paths);
                let overflow = scroll.1;
                div()
                    .flex()
                    .items_center()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .id(("tab-row", gid))
                            .flex()
                            .items_center()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .track_scroll(&scroll.0)
                            .children(row_tabs),
                    )
                    .when(overflow, |d| {
                        d.child(
                            div()
                                .id(("tab-list-chevron", gid))
                                .tip_text(
                                    if main {
                                        "tab-list".to_owned()
                                    } else {
                                        format!("tab-list-{gid}")
                                    },
                                    ferris_idea_ui::tooltip::Tip::plain(
                                        "Show Hidden Tabs".to_owned(),
                                    ),
                                )
                                .relative()
                                .flex_none()
                                .ml_1()
                                .rounded(px(3.))
                                .hover(|s| s.bg(c(p.tree_hover)))
                                .child(icon("general/chevronDown"))
                                .on_click(cx.listener(
                                    move |this, e: &gpui::ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        let at = e.position();
                                        this.open_tab_list(gid, Some(at), window, cx);
                                    },
                                )),
                        )
                    })
                    .into_any_element()
            }
            TabRows::Squeeze => div()
                .flex()
                .items_center()
                .gap_1()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .children(row_tabs)
                .into_any_element(),
            TabRows::Wrap => div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .flex_1()
                .min_w_0()
                .py_0p5()
                .children(row_tabs)
                .into_any_element(),
        };
        let bar = crate::metrics::tab_bar();
        let bar = if settings.rows == TabRows::Wrap {
            bar.h_auto()
        } else {
            bar
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .children(pinned_row)
            .child(bar.child(row))
            .into_any_element()
    }

    /// The scroll handle of group `gid`'s tab row, scrolled so that the active tab shows when the
    /// active tab, the number of tabs or the width changed (`SPEC-0059` v9 behaviour 15); and
    /// whether the tabs overflow (the chevron shows then).
    fn tab_scroll_for(
        &self,
        gid: GroupId,
        shown: &[usize],
        active: Option<usize>,
        paths: &[PathBuf],
    ) -> (gpui::ScrollHandle, bool) {
        let mut scrolls = self.tab_scrolls.borrow_mut();
        let s = scrolls.entry(gid).or_default();
        s.shown = shown.to_vec();
        let bar = f32::from(s.handle.bounds().size.width);
        // Each tab's share of the row: from its start to the next tab's start, so the gap between
        // tabs counts too (without it, 14 tabs left the active one ~50 px under the chevron).
        let bounds: Vec<Option<gpui::Bounds<gpui::Pixels>>> = (0..shown.len())
            .map(|k| s.handle.bounds_for_item(k))
            .collect();
        let widths: Vec<f32> = (0..bounds.len())
            .map(
                |k| match (bounds[k], bounds.get(k + 1).copied().flatten()) {
                    (Some(b), Some(next)) => f32::from(next.origin.x - b.origin.x).max(0.0),
                    (Some(b), None) => f32::from(b.size.width),
                    (None, _) => 0.0,
                },
            )
            .collect();
        let overflow =
            crate::tabs::overflows(&widths, bar) || f32::from(s.handle.max_offset().width) > 0.5;
        let active_shown = active.and_then(|a| shown.iter().position(|&i| i == a));
        let key = (
            active.and_then(|a| paths.get(a).cloned()),
            shown.len(),
            bar as i32,
        );
        if s.key.as_ref() != Some(&key) && widths.iter().all(|w| *w > 0.0) && bar > 0.0 {
            let current = -f32::from(s.handle.offset().x);
            let next = crate::tabs::scroll_offset(&widths, active_shown, bar, current);
            if (next - current).abs() > 0.5 {
                s.handle.set_offset(gpui::point(px(-next), px(0.)));
            }
            s.key = Some(key);
        } else if s.key.as_ref() != Some(&key) {
            // Not laid out yet: scroll by the toolkit, check again next frame.
            if let Some(a) = active_shown {
                s.handle.scroll_to_item(a);
            }
        }
        (s.handle.clone(), overflow)
    }

    /// The right-click menu of tab `index` of group `gid` (`SPEC-0059` v9 behaviour 18).
    pub(super) fn open_tab_menu(
        &mut self,
        gid: GroupId,
        index: usize,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let path = self
            .group_tabs(gid)
            .and_then(|t| t.paths().get(index).cloned());
        self.open_context_menu(
            super::menus::MenuPlace::Tab { group: gid, index },
            path,
            at,
            cx,
        );
    }

    /// The menu entries of tab `index` of group `gid`.
    pub(super) fn tab_menu_entries(
        &self,
        gid: GroupId,
        index: usize,
        cx: &gpui::App,
    ) -> Vec<crate::context_menu::MenuEntry> {
        let tabs = self.group_tabs(gid);
        let path = tabs.and_then(|t| t.paths().get(index).cloned());
        let runnable = path
            .as_deref()
            .filter(|p| super::menus::runnable(p))
            .map(|p| {
                let text = self
                    .group_editor(gid, p)
                    .map(|e| e.read(cx).state.buffer.text())
                    .unwrap_or_default();
                crate::context_menu::run_names(&file_name(p), &text, None)
            });
        let menu = crate::context_menu::tab_menu(&crate::context_menu::TabTarget {
            pinned: tabs.is_some_and(|t| t.is_pinned(index)),
            any_pinned: tabs.is_some_and(|t| t.pinned_count() > 0),
            split: self.layout.is_split(),
            runnable,
            annotated: path.as_deref().is_some_and(|p| self.annotated(p, cx)),
        });
        // SPEC-0004 v5 behaviour 13: after Rename File….
        let file_type = path
            .as_deref()
            .map(|p| self.file_type_target_of(p))
            .unwrap_or_default();
        crate::context_menu::with_file_type_entries_in_tab_menu(menu, file_type)
    }

    /// The open menu of the main group's tab, if any (SPEC-0275).
    pub fn tab_menu_open(&self) -> Option<usize> {
        self.open_tab_menu_target()
            .and_then(|(g, i)| (g == MAIN).then_some(i))
    }

    /// Carries out tab menu item `action` on tab `index` of group `gid`.
    pub(super) fn tab_menu_item(
        &mut self,
        gid: GroupId,
        index: usize,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .group_tabs(gid)
            .and_then(|t| t.paths().get(index).cloned())
        else {
            return;
        };
        if let Some(t) = self.group_tabs_mut(gid) {
            t.activate(index);
        }
        if gid == MAIN {
            self.diff = None;
            self.diff_file = None;
        }
        // The tab's group acts on its own view (SPEC-0277 v2).
        self.focused_group = gid;
        self.focus_active(window, cx);
        use crate::context_menu as m;
        match action {
            // The tab is the active one now: its caret's line and symbol (SPEC-0332 v2
            // behaviour 13e).
            m::COPY_PATH_WITH_LINE => self.copy_path_with_line(cx),
            m::COPY_REFERENCE => self.copy_reference(window, cx),
            m::COPY_PATH
            | m::COPY_RELATIVE_PATH
            | m::COPY_SOURCE_ROOT_PATH
            | m::COPY_FILE_NAME
            | m::REVEAL
            | m::OPEN_IN_TERMINAL
            | m::RENAME_FILE => self.tree_item(action, path, window, cx),
            _ => self.menu_action(action, window, cx),
        }
    }

    /// One extra group: its tab bar and its view of the active file (SPEC-0112 behaviour 2).
    fn group_island(&self, gid: GroupId, cx: &mut Context<Self>) -> gpui::AnyElement {
        let editor = self
            .group_tabs(gid)
            .and_then(|t| t.active_path().cloned())
            .and_then(|p| self.group_editor(gid, &p));
        let first_extra = self.second_group() == Some(gid);
        island()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .capture_any_mouse_down(cx.listener(move |this, _, _, cx| {
                if this.focused_group != gid {
                    this.focused_group = gid;
                    cx.notify();
                }
            }))
            .child(self.group_tab_bar(gid, cx))
            .when_some(editor, |d, e| {
                // A secondary click on the group's text opens the editor menu too (D54, hand
                // test 199; SPEC-0275 behaviour 14).
                d.child(
                    crate::mouse::on_secondary_click(
                        div().id(("split-editor-body", gid)),
                        cx.listener(move |this, ev: &gpui::MouseDownEvent, window, cx| {
                            this.focused_group = gid;
                            this.editor_secondary(ev, window, cx)
                        }),
                    )
                    .relative()
                    .children(crate::hit_targets::mark(if first_extra {
                        "split-editor-text".to_owned()
                    } else {
                        format!("group{gid}-editor-text")
                    }))
                    .flex_1()
                    .min_h_0()
                    .child(e.clone())
                    // Its own indicator, compact (SPEC-0070 v7 behaviour 12).
                    .child(self.compact_indicator(&e, true, cx)),
                )
            })
            .into_any_element()
    }

    /// The editor area: the main group's island (`main`) and the other groups, laid out as the
    /// tree of splits with draggable dividers (`SPEC-0112` v5 behaviour 10). A maximized editor
    /// shows the focused group alone.
    pub(super) fn editor_area(
        &self,
        main: gpui::AnyElement,
        covered: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut main = Some(main);
        if covered {
            return main
                .take()
                .map(|m| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .child(m)
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element());
        }
        if self.editor_maximized {
            let gid = self.focused_group;
            let body = if gid == MAIN {
                main.take().unwrap_or_else(|| div().into_any_element())
            } else {
                self.group_island(gid, cx)
            };
            return div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .child(body)
                .into_any_element();
        }
        let root = self.layout.root().clone();
        let mut path = Vec::new();
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .child(self.compose(&root, &mut path, &mut main, cx))
            .into_any_element()
    }

    fn compose(
        &self,
        node: &Node,
        path: &mut SplitPath,
        main: &mut Option<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match node {
            Node::Group { id } if *id == MAIN => main
                .take()
                .map(|m| div().size_full().flex().child(m).into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            Node::Group { id } => self.group_island(*id, cx),
            Node::Split {
                down,
                ratio,
                first,
                second,
            } => {
                let p = Palette::current();
                let here = path.clone();
                path.push(false);
                let a = self.compose(first, path, main, cx);
                path.pop();
                path.push(true);
                let b = self.compose(second, path, main, cx);
                path.pop();
                let bounds = self.split_bounds.clone();
                let key = here.clone();
                let measure = gpui::canvas(
                    move |b, _, _| {
                        bounds.borrow_mut().insert(
                            key.clone(),
                            ferris_idea_ui::tool_windows::Rect {
                                x: f32::from(b.origin.x),
                                y: f32::from(b.origin.y),
                                w: f32::from(b.size.width),
                                h: f32::from(b.size.height),
                            },
                        );
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full();
                let gap = px(p.island_gap);
                let divider_id: gpui::ElementId =
                    gpui::SharedString::from(format!("split-divider-{here:?}")).into();
                let drag = DividerDrag(here.clone());
                let divider = div()
                    .id(divider_id)
                    .flex_none()
                    .when(*down, |d| d.h(gap).w_full().cursor_row_resize())
                    .when(!*down, |d| d.w(gap).h_full().cursor_col_resize())
                    .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
                    .on_click(cx.listener({
                        let here = here.clone();
                        move |this, e: &gpui::ClickEvent, _, cx| {
                            // A double click makes both sides equal.
                            if e.click_count() > 1 {
                                this.layout.set_ratio(&here, 0.5);
                                cx.notify();
                            }
                        }
                    }));
                let side = |el: gpui::AnyElement, share: f32| {
                    div()
                        .flex_basis(gpui::relative(share))
                        .flex_grow()
                        .flex_shrink()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .child(el)
                };
                div()
                    .relative()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .when(*down, |d| d.flex_col())
                    .child(measure)
                    .child(side(a, *ratio))
                    .child(divider)
                    .child(side(b, 1.0 - *ratio))
                    .into_any_element()
            }
        }
    }

    /// A divider dragged to window position `at` sets its split's ratio.
    pub(super) fn drag_divider(
        &mut self,
        path: &SplitPath,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.split_bounds.borrow().get(path).copied() else {
            return;
        };
        let down = self
            .layout
            .splits()
            .into_iter()
            .find(|(p, _, _)| p == path)
            .is_some_and(|(_, d, _)| d);
        let ratio = if down {
            (f32::from(at.y) - node.y) / node.h.max(1.0)
        } else {
            (f32::from(at.x) - node.x) / node.w.max(1.0)
        };
        self.layout.set_ratio(path, ratio);
        cx.notify();
    }

    /// Settings that act on the open tabs at once: sorting switched on sorts every group.
    pub(super) fn follow_tab_settings(&mut self, cx: &gpui::App) {
        let now = Self::tab_settings(cx);
        if now.sort_alphabetically && !self.tab_settings_seen.sort_alphabetically {
            self.tabs.sort();
            for g in &mut self.groups {
                g.tabs.sort();
            }
        }
        self.tab_settings_seen = now;
    }

    // ---------------------------------------------------------------- MCP

    /// The groups with their tabs, as `editor_tabs` lists them (`SPEC-0059` v9 behaviour 21,
    /// `SPEC-0112` v5 behaviour 11).
    pub fn editor_tabs_state(&self, cx: &gpui::App) -> serde_json::Value {
        let groups: Vec<serde_json::Value> = self
            .layout
            .groups()
            .into_iter()
            .filter_map(|gid| {
                let tabs = self.group_tabs(gid)?;
                let active = tabs.active();
                let list: Vec<serde_json::Value> = tabs
                    .paths()
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        serde_json::json!({
                            "path": p.strip_prefix(&self.root).unwrap_or(p).display().to_string(),
                            "pinned": tabs.is_pinned(i),
                            "modified": self.doc_modified(p, cx),
                            "active": Some(i) == active,
                        })
                    })
                    .collect();
                Some(serde_json::json!({
                    "id": gid,
                    "place": self.layout.place(gid),
                    "focused": gid == self.focused_group,
                    "tabs": list,
                }))
            })
            .collect();
        serde_json::json!({
            "groups": groups,
            "focused": self.focused_group,
            "reopenable": self.closed_tabs.len(),
        })
    }

    /// Carries out one `editor_tabs` request; never closes a tab with unsaved changes
    /// (`SPEC-0059` v9.1 behaviour 22d).
    pub fn editor_tabs_call(
        &mut self,
        request: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        let action = request["action"].as_str().unwrap_or("list");
        let gid = request["group"]
            .as_u64()
            .map_or(self.focused_group, |g| g as usize);
        if !self.layout.contains(gid) {
            return Err(format!(
                "no group {gid}; the groups are {:?}",
                self.layout.groups()
            ));
        }
        let path = request["path"].as_str().map(PathBuf::from);
        if action == "open" {
            // A file of the workspace in a focused tab, the window in front (SPEC-0094 v7
            // behaviour 21); the MCP server checked the path.
            let file = path.filter(|p| p.is_file()).ok_or("`open` needs a file")?;
            self.focused_group = gid;
            self.open(&file, window, cx);
            window.activate_window();
            return Ok(self.editor_tabs_state(cx));
        }
        let index = match &path {
            Some(p) => self.group_tabs(gid).and_then(|t| t.position(p)),
            None => self.group_tabs(gid).and_then(Tabs::active),
        };
        let needs_tab = !matches!(action, "list" | "reopen_closed" | "unsplit_all");
        let index = match (index, needs_tab) {
            (Some(i), _) => Some(i),
            (None, false) => None,
            (None, true) => {
                return Err(match path {
                    Some(p) => format!("{} is not open in group {gid}", p.display()),
                    None => format!("group {gid} has no tab"),
                });
            }
        };
        let mut kept: Vec<String> = Vec::new();
        let rel = |p: &Path, root: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
        if let Some(i) = index
            && action != "list"
        {
            if let Some(t) = self.group_tabs_mut(gid) {
                t.activate(i);
            }
            if gid == MAIN && action == "activate" {
                self.close_diff_for_file_tab();
            }
            self.focused_group = gid;
        }
        match action {
            "list" => {}
            "activate" => self.focus_active(window, cx),
            "close" => {
                let i = index.unwrap_or(0);
                let p = self.group_tabs(gid).and_then(|t| t.paths().get(i).cloned());
                if let Some(p) = p {
                    if self.views_of(&p).len() <= 1 && self.doc_modified(&p, cx) {
                        kept.push(rel(&p, &self.root));
                    } else {
                        self.close_tab(gid, i, window, cx);
                    }
                }
            }
            "close_others" | "close_all" => {
                let scope = if action == "close_all" {
                    CloseScope::All
                } else {
                    CloseScope::Others
                };
                let plan = self.group_tabs(gid).map(|t| {
                    t.plan_close(scope, index.unwrap_or(0), |p| {
                        self.views_of(p).len() <= 1 && self.doc_modified(p, cx)
                    })
                });
                if let Some(plan) = plan {
                    kept = plan.ask.iter().map(|p| rel(p, &self.root)).collect();
                    self.close_in_group(gid, &plan.close, window, cx);
                }
            }
            "pin" | "unpin" => {
                if let (Some(t), Some(i)) = (self.group_tabs_mut(gid), index) {
                    t.set_pinned(i, action == "pin");
                }
            }
            "reopen_closed" => self.reopen_closed_tab(window, cx),
            "split_right" => self.split_editor(false, window, cx),
            "split_down" => self.split_editor(true, window, cx),
            "move_to_group" => {
                let to = match request["to_group"].as_u64() {
                    Some(t) => t as usize,
                    None => self
                        .layout
                        .neighbour(gid)
                        .ok_or("the editor is not split; there is no group to move to")?,
                };
                if !self.layout.contains(to) {
                    return Err(format!(
                        "no group {to}; the groups are {:?}",
                        self.layout.groups()
                    ));
                }
                self.move_active_to(to, window, cx);
            }
            "unsplit_all" => self.unsplit_all(window, cx),
            other => return Err(format!("unknown action `{other}`")),
        }
        cx.notify();
        let mut state = self.editor_tabs_state(cx);
        if !kept.is_empty() {
            state["kept_modified"] = serde_json::json!(kept);
        }
        Ok(state)
    }

    /// The selection of the active editor of the focused tab group (`SPEC-0006` v56 behaviour
    /// 11): `None` without an editor.
    pub fn editor_selection(&self, cx: &gpui::App) -> Option<EditorSelectionState> {
        let path = self.active_path()?;
        let editor = self.active_editor()?;
        let e = editor.read(cx);
        let primary = e.state.carets.primary();
        let (start, end) = (
            primary.anchor.min(primary.head),
            primary.anchor.max(primary.head),
        );
        let at = |offset: usize| {
            e.state
                .buffer
                .offset_to_position(offset)
                .map(|p| (p.line, p.column))
                .unwrap_or((0, 0))
        };
        let ((start_line, start_character), (end_line, end_character)) = (at(start), at(end));
        Some(EditorSelectionState {
            path,
            start_line,
            start_character,
            end_line,
            end_character,
            text: e.state.buffer.slice(start..end).unwrap_or_default(),
            carets: e.state.carets.carets().len(),
        })
    }

    /// `editor_selection` for the project window of `workspace` (MCP over HTTP).
    pub fn editor_selection_in(
        workspace: &Path,
        cx: &mut gpui::App,
    ) -> Result<Option<EditorSelectionState>, String> {
        let wanted = workspace.canonicalize().unwrap_or(workspace.to_path_buf());
        let bench = crate::project_windows::workbenches(cx)
            .into_iter()
            .find(|b| {
                let root = b.read(cx).root.clone();
                root.canonicalize().unwrap_or(root) == wanted
            })
            .ok_or("no project window is open for this workspace")?;
        Ok(bench.read(cx).editor_selection(cx))
    }

    /// `editor_tabs` for the project window of `workspace` (MCP over HTTP).
    pub fn editor_tabs_in(
        workspace: &Path,
        request: &serde_json::Value,
        cx: &mut gpui::App,
    ) -> Result<serde_json::Value, String> {
        let wanted = workspace.canonicalize().unwrap_or(workspace.to_path_buf());
        let bench = crate::project_windows::workbenches(cx)
            .into_iter()
            .find(|b| {
                let root = b.read(cx).root.clone();
                root.canonicalize().unwrap_or(root) == wanted
            })
            .ok_or("no project window is open for this workspace")?;
        let root = bench.read(cx).root.clone();
        let handle = crate::project_windows::project_window(&root, cx)
            .ok_or("the project window is not registered")?;
        handle
            .update(cx, |_, window, cx| {
                bench.update(cx, |b, cx| b.editor_tabs_call(request, window, cx))
            })
            .map_err(|e| e.to_string())?
    }
}

/// The selection of an editor as the window reports it to agents (`SPEC-0006` v56 behaviour 11):
/// 0-based lines and characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSelectionState {
    /// The editor's file.
    pub path: PathBuf,
    /// Where the selection starts.
    pub start_line: usize,
    /// See `start_line`.
    pub start_character: usize,
    /// Where it ends (the caret when nothing is selected).
    pub end_line: usize,
    /// See `end_line`.
    pub end_character: usize,
    /// The selected text.
    pub text: String,
    /// How many carets the editor has.
    pub carets: usize,
}
