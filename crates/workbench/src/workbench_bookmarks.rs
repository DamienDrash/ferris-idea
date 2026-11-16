//! Bookmarks in the workbench (`SPEC-0331`): the actions that set, remove and visit them, the
//! marks in the editors' gutters, the popups and the Bookmarks tool window.
//!
//! The bookmarks, how their lines follow edits and their project file are
//! [`idecraft_core::bookmarks`]; what the popups and the tree show is
//! [`crate::bookmarks_model`]; the views are [`crate::bookmarks_view`].

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use gpui::{AppContext, Context, Entity, Window};
use idecraft_core::bookmarks::{Change, LineEdit, Listed, MnemonicToggle, listed, relative};
use idecraft_ui::tool_windows::ToolWindow;

use super::WorkbenchView;
use super::popups::Popup;
use crate::EditorView;
use crate::bookmarks_model::taken_message;
use crate::bookmarks_view::{BookmarkEvent, BookmarksPopup, BookmarksToolView};

/// The go-to and toggle actions of the digit mnemonics with their digit (behaviours 17, 18).
const DIGIT_ACTIONS: [(&str, &str, char); 10] = [
    ("GotoBookmark0", "ToggleBookmark0", '0'),
    ("GotoBookmark1", "ToggleBookmark1", '1'),
    ("GotoBookmark2", "ToggleBookmark2", '2'),
    ("GotoBookmark3", "ToggleBookmark3", '3'),
    ("GotoBookmark4", "ToggleBookmark4", '4'),
    ("GotoBookmark5", "ToggleBookmark5", '5'),
    ("GotoBookmark6", "ToggleBookmark6", '6'),
    ("GotoBookmark7", "ToggleBookmark7", '7'),
    ("GotoBookmark8", "ToggleBookmark8", '8'),
    ("GotoBookmark9", "ToggleBookmark9", '9'),
];

/// The shortcut of `action` in the keymap in use, as it is shown; empty without one.
fn shortcut_of(action: &str) -> String {
    crate::keymaps::current()
        .shortcuts(action)
        .iter()
        .find(|s| !matches!(s, idecraft_ui::keymap::Shortcut::Mouse { .. }))
        .map(|s| s.display(cfg!(target_os = "macos")))
        .unwrap_or_default()
}

impl WorkbenchView {
    /// Carries out a bookmark action by id; returns whether `id` is one (behaviours 15–22).
    pub(super) fn run_bookmark_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match id {
            "ToggleBookmark" => self.toggle_bookmark(cx),
            "ToggleBookmarkWithMnemonic" => self.open_bookmark_mnemonics(window, cx),
            "ShowBookmarks" => self.open_bookmarks_popup(window, cx),
            "ShowTypeBookmarks" => self.open_mnemonic_bookmarks(window, cx),
            "GotoNextBookmark" => self.goto_adjacent_bookmark(true, cx),
            "GotoPreviousBookmark" => self.goto_adjacent_bookmark(false, cx),
            "ActivateBookmarksToolWindow" => {
                self.toggle_tool_window(ToolWindow::Bookmarks, window, cx)
            }
            _ => {
                let Some((goto, _, digit)) = DIGIT_ACTIONS
                    .iter()
                    .find(|(goto, toggle, _)| *goto == id || *toggle == id)
                else {
                    return false;
                };
                if *goto == id {
                    self.goto_bookmark(*digit, window, cx);
                } else {
                    self.toggle_bookmark_mnemonic(*digit, cx);
                }
            }
        }
        true
    }

    /// The project's bookmarks, newest first (checks).
    pub fn bookmarks(&self) -> &idecraft_core::bookmarks::Bookmarks {
        self.bookmark_store.bookmarks()
    }

    /// The open bookmark popup: the mnemonic chooser, the list or the description field.
    pub fn bookmarks_popup(&self) -> Option<Entity<BookmarksPopup>> {
        self.bookmarks_popup.clone()
    }

    /// The Bookmarks tool window's tree, once the window was shown.
    pub fn bookmarks_tool_view(&self) -> Option<Entity<BookmarksToolView>> {
        self.bookmarks_tool.clone()
    }

    /// Every editor view: the main group's and the other groups'.
    fn editors_with_bookmarks(&self) -> Vec<Entity<EditorView>> {
        self.editors
            .values()
            .chain(self.groups.iter().flat_map(|g| g.editors.values()))
            .cloned()
            .collect()
    }

    /// The texts of the open editors whose file has a bookmark, by project file: lists show the
    /// lines as the editor has them.
    fn open_bookmark_texts(&self, cx: &gpui::App) -> BTreeMap<String, String> {
        let bookmarks = self.bookmark_store.bookmarks();
        self.editors
            .iter()
            .filter_map(|(path, editor)| {
                let file = relative(&self.root, path)?;
                (!bookmarks.in_file(&file).is_empty())
                    .then(|| (file, editor.read(cx).state.buffer.text()))
            })
            .collect()
    }

    /// The bookmarks with their line texts, newest first.
    fn listed_bookmarks(&self, cx: &gpui::App) -> Vec<Listed> {
        let open = self.open_bookmark_texts(cx);
        listed(&self.root, self.bookmark_store.bookmarks(), &|file| {
            open.get(file).cloned()
        })
    }

    /// The texts of the files of `listed`, for the list's preview: from the open editor, else
    /// from disk.
    fn bookmark_file_texts(&self, shown: &[Listed], cx: &gpui::App) -> BTreeMap<String, Arc<str>> {
        let open = self.open_bookmark_texts(cx);
        let mut texts = BTreeMap::new();
        for l in shown.iter().filter(|l| !l.missing) {
            let file = &l.bookmark.file;
            if texts.contains_key(file) {
                continue;
            }
            let text = open.get(file).cloned().or_else(|| {
                let path = self.root.join(file);
                std::fs::metadata(&path)
                    .ok()
                    .filter(|m| m.len() <= 16 * 1024 * 1024)
                    .and_then(|_| std::fs::read_to_string(&path).ok())
            });
            if let Some(text) = text {
                texts.insert(file.clone(), Arc::from(text));
            }
        }
        texts
    }

    /// Shows the bookmarks of `editor`'s file in its gutter (behaviour 14).
    pub(super) fn show_bookmark_marks(&self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        let file = editor
            .read(cx)
            .file_path()
            .and_then(|p| relative(&self.root, p));
        let (file, marks) = match file {
            Some(file) => {
                let marks = self.bookmark_store.bookmarks().of_file(&file);
                (file, marks)
            }
            None => Default::default(),
        };
        editor.update(cx, |e, cx| e.set_bookmarks(file, marks, cx));
    }

    /// Reads the bookmarks again when their file changed on disk — an agent through MCP, a file
    /// operation that carried bookmarks along — and shows the new state (behaviour 13).
    pub(super) fn sync_bookmarks(&mut self, cx: &mut Context<Self>) {
        if self.bookmark_store.refresh() {
            self.bookmarks_changed(cx);
        }
    }

    /// The bookmarks changed: gutters, the list popup and the tool window follow (behaviour 25).
    fn bookmarks_changed(&mut self, cx: &mut Context<Self>) {
        for editor in self.editors_with_bookmarks() {
            self.show_bookmark_marks(&editor, cx);
        }
        let list_open = self
            .bookmarks_popup
            .as_ref()
            .is_some_and(|p| p.read(cx).is_list());
        if self.bookmarks_tool.is_some() || list_open {
            let shown = self.listed_bookmarks(cx);
            if let Some(popup) = self.bookmarks_popup.clone().filter(|_| list_open) {
                let texts = self.bookmark_file_texts(&shown, cx);
                popup.update(cx, |p, cx| p.set_listed(&shown, texts, cx));
            }
            if let Some(tool) = &self.bookmarks_tool {
                tool.update(cx, |t, cx| t.set_listed(shown, cx));
            }
        }
        cx.notify();
    }

    /// The editor of `path` was edited: the project's bookmarks of the file move with `edits`
    /// (behaviours 5–7).
    pub(super) fn bookmark_edits(
        &mut self,
        path: &Path,
        edits: &[LineEdit],
        cx: &mut Context<Self>,
    ) {
        let Some(file) = relative(&self.root, path) else {
            return;
        };
        let (_, changed) = self.bookmark_store.change(|b| {
            for edit in edits {
                b.edited(&file, edit);
            }
        });
        if changed {
            self.bookmarks_changed(cx);
        }
    }

    /// The active editor with its project file and the 0-based line of its caret; without a
    /// project file the status bar says why there is no bookmark (behaviour 1).
    fn bookmark_place(&mut self, cx: &mut Context<Self>) -> Option<(String, usize)> {
        let why = if self.remote_project.is_some() {
            "Bookmarks are not available in a remote project yet"
        } else {
            match self.active_editor() {
                None => "No editor: a bookmark belongs to a line of a file",
                Some(editor) => {
                    let e = editor.read(cx);
                    let line = e.status().line.saturating_sub(1);
                    match e.file_path().and_then(|p| relative(&self.root, p)) {
                        Some(file) => return Some((file, line)),
                        None => "Bookmarks are for files of the project",
                    }
                }
            }
        };
        self.status = Some(why.into());
        cx.notify();
        None
    }

    /// `ToggleBookmark` (behaviour 15).
    fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
        self.sync_bookmarks(cx);
        let Some((file, line)) = self.bookmark_place(cx) else {
            return;
        };
        self.bookmark_store.change(|b| b.toggle(&file, line));
        self.bookmarks_changed(cx);
    }

    /// `ToggleBookmark0`…`9` (behaviour 17).
    fn toggle_bookmark_mnemonic(&mut self, m: char, cx: &mut Context<Self>) {
        self.sync_bookmarks(cx);
        let Some((file, line)) = self.bookmark_place(cx) else {
            return;
        };
        let (done, _) = self
            .bookmark_store
            .change(|b| b.toggle_mnemonic(&file, line, m));
        if let Some(MnemonicToggle::Set(from)) = done {
            self.status = taken_message(m, from.as_ref());
        }
        self.bookmarks_changed(cx);
    }

    /// `ToggleBookmarkWithMnemonic` (behaviour 16): the popup for the caret's line.
    fn open_bookmark_mnemonics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_bookmarks(cx);
        let Some((file, line)) = self.bookmark_place(cx) else {
            return;
        };
        let bookmarks = self.bookmark_store.bookmarks();
        let (current, used) = (bookmarks.at(&file, line).cloned(), bookmarks.mnemonics());
        let place = format!("{}:{}", file.rsplit('/').next().unwrap_or(&file), line + 1);
        let view = cx.new(|cx| BookmarksPopup::mnemonic(place, current.as_ref(), used, cx));
        self.bookmark_popup_line = Some((file, line));
        self.show_bookmarks_popup(view, window, cx);
    }

    /// `ShowBookmarks` (behaviour 20): the list of all bookmarks with a preview.
    fn open_bookmarks_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_project.is_some() {
            self.status = Some("Bookmarks are not available in a remote project yet".into());
            cx.notify();
            return;
        }
        self.sync_bookmarks(cx);
        let shown = self.listed_bookmarks(cx);
        let texts = self.bookmark_file_texts(&shown, cx);
        let root = self.root.clone();
        let view = cx
            .new(|cx| BookmarksPopup::list(root, &shown, texts, shortcut_of("ToggleBookmark"), cx));
        self.show_bookmarks_popup(view, window, cx);
    }

    /// `ShowTypeBookmarks`, Go to Mnemonic… (v3 behaviour 33): the bookmarks that have a
    /// mnemonic in a list; the chosen one is opened as `GotoBookmark0`…`9` opens it.
    fn open_mnemonic_bookmarks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_project.is_some() {
            self.status = Some("Bookmarks are not available in a remote project yet".into());
            cx.notify();
            return;
        }
        self.sync_bookmarks(cx);
        let rows = crate::bookmarks_model::mnemonic_rows(&self.listed_bookmarks(cx));
        if rows.is_empty() {
            self.status = Some(crate::keymap_actions::NO_MNEMONIC_BOOKMARKS.into());
            cx.notify();
            return;
        }
        let items = rows.iter().map(|r| r.mnemonic_label()).collect();
        self.choose_from(
            "Go to Mnemonic".into(),
            items,
            0,
            window,
            cx,
            move |this, i, window, cx| {
                if let Some(r) = rows.get(i) {
                    this.jump_to_bookmark(&r.file, r.line, window, cx);
                }
            },
        );
    }

    /// Shows a bookmark popup and carries out what it asks for.
    fn show_bookmarks_popup(
        &mut self,
        view: Entity<BookmarksPopup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_editor_popups();
        self._bookmarks_popup_events = Some(cx.subscribe_in(
            &view,
            window,
            |this, _, event: &BookmarkEvent, window, cx| this.bookmark_event(event, window, cx),
        ));
        window.focus(&view.read(cx).focus(cx));
        self.bookmarks_popup = Some(view);
        cx.notify();
    }

    /// Closes the bookmark popup and gives the keys back: to the tool window when it has none of
    /// its own popups left and is shown, else to the editor.
    fn close_bookmarks_popup(
        &mut self,
        to_tool: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_popup(Popup::Bookmarks);
        match self.bookmarks_tool.as_ref() {
            Some(tool) if to_tool && self.show_bookmarks_tool => {
                window.focus(&tool.read(cx).focus())
            }
            _ => self.focus_active(window, cx),
        }
        cx.notify();
    }

    /// What a bookmark popup or the tool window asked for (behaviours 16, 21, 24).
    fn bookmark_event(
        &mut self,
        event: &BookmarkEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BookmarkEvent::Set {
                mnemonic,
                description,
            } => {
                let Some((file, line)) = self.bookmark_popup_line.take() else {
                    return;
                };
                let (from, _) = self.bookmark_store.change(|b| {
                    b.set(
                        &file,
                        line,
                        mnemonic.map_or(Change::Clear, Change::Set),
                        Change::Set(description.clone()),
                    )
                });
                if let Some(m) = mnemonic {
                    self.status = taken_message(*m, from.as_ref());
                }
                self.close_bookmarks_popup(false, window, cx);
                self.bookmarks_changed(cx);
            }
            BookmarkEvent::Jump(file, line) => {
                self.close_popup(Popup::Bookmarks);
                self.jump_to_bookmark(file, *line, window, cx);
            }
            BookmarkEvent::Remove(file, line) => {
                let (gone, _) = self.bookmark_store.change(|b| b.remove(file, *line));
                if let Some(gone) = gone {
                    self.status = Some(format!("Bookmark removed from {}", gone.place()));
                }
                self.bookmarks_changed(cx);
            }
            BookmarkEvent::Describe(file, line, text) => {
                if self.bookmark_store.bookmarks().at(file, *line).is_some() {
                    self.bookmark_store
                        .change(|b| b.set(file, *line, Change::Keep, Change::Set(text.clone())));
                }
                self.close_bookmarks_popup(true, window, cx);
                self.bookmarks_changed(cx);
            }
            BookmarkEvent::AskDescription(file, line) => {
                let Some(now) = self.bookmark_store.bookmarks().at(file, *line).cloned() else {
                    return;
                };
                let view = cx.new(|cx| {
                    BookmarksPopup::describe(now.file, now.line, now.description.as_deref(), cx)
                });
                self.show_bookmarks_popup(view, window, cx);
            }
            BookmarkEvent::OpenFile(file) => {
                let path = self.root.join(file);
                if path.is_file() {
                    self.remember_place(cx);
                    self.open(&path, window, cx);
                    self.focus_active(window, cx);
                }
            }
            BookmarkEvent::Close => {
                let describing = self
                    .bookmarks_popup
                    .as_ref()
                    .is_some_and(|p| !p.read(cx).is_list() && self.bookmark_popup_line.is_none());
                self.bookmark_popup_line = None;
                self.close_bookmarks_popup(describing, window, cx);
            }
        }
    }

    /// Opens the bookmark's file and puts the caret at the start of its line; the place left is
    /// remembered for Back (behaviour 18).
    fn jump_to_bookmark(
        &mut self,
        file: &str,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.root.join(file);
        if !path.is_file() {
            self.status = Some(format!("{file} is not there"));
            cx.notify();
            return;
        }
        self.remember_place(cx);
        self.open(&path, window, cx);
        if let Some(editor) = self.editors.get(&path) {
            editor.update(cx, |e, cx| e.go_to(line, 0, cx));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// `GotoBookmark0`…`9` (behaviour 18).
    fn goto_bookmark(&mut self, m: char, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_bookmarks(cx);
        match self.bookmark_store.bookmarks().with_mnemonic(m).cloned() {
            Some(b) => self.jump_to_bookmark(&b.file, b.line, window, cx),
            None => {
                self.status = Some(format!("No bookmark has the mnemonic {m}"));
                cx.notify();
            }
        }
    }

    /// `GotoNextBookmark` and `GotoPreviousBookmark` (behaviour 19).
    fn goto_adjacent_bookmark(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.sync_bookmarks(cx);
        let Some((file, line)) = self.bookmark_place(cx) else {
            return;
        };
        let next = self
            .bookmark_store
            .bookmarks()
            .next_in_file(&file, line, forward);
        match (next, self.active_editor()) {
            (Some(next), Some(editor)) => {
                self.remember_place(cx);
                editor.update(cx, |e, cx| e.go_to(next, 0, cx));
            }
            _ => {
                self.status = Some("No bookmarks in this file".into());
            }
        }
        cx.notify();
    }

    /// Shows the Bookmarks tool window and focuses its tree, or hides it
    /// (`ActivateBookmarksToolWindow`, behaviour 22).
    pub(super) fn toggle_bookmarks_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_bookmarks_tool {
            self.show_bookmarks_tool = false;
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        self.sync_bookmarks(cx);
        let shown = self.listed_bookmarks(cx);
        let tool = match self.bookmarks_tool.clone() {
            Some(tool) => {
                tool.update(cx, |t, cx| t.set_listed(shown, cx));
                tool
            }
            None => {
                let project = self
                    .root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let tool = cx.new(|cx| {
                    BookmarksToolView::new(project, shown, shortcut_of("ToggleBookmark"), cx)
                });
                self._bookmarks_tool_events = Some(cx.subscribe_in(
                    &tool,
                    window,
                    |this, _, event: &BookmarkEvent, window, cx| {
                        this.bookmark_event(event, window, cx)
                    },
                ));
                self.bookmarks_tool = Some(tool.clone());
                tool
            }
        };
        self.show_bookmarks_tool = true;
        window.focus(&tool.read(cx).focus());
        cx.notify();
    }
}
