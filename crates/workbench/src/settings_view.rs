//! The Settings dialog (`SPEC-0105`): a search field, the page list and the page. Appearance
//! chooses the theme; Keymap lists the actions with their shortcuts and records new ones; Plugins
//! manages the installed plugins (`SPEC-0126`).

use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use std::ops::Range;
use std::path::{Path, PathBuf};

use ferris_idea_ui::keymap::{Keystroke, Shortcut};
use ferris_idea_ui::theme::ThemeId;
use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    KeyDownEvent, Pixels, UTF16Selection, Window, canvas, div, prelude::*, px, uniform_list,
};

mod edit_actions;
mod general_pages;
mod options;

use crate::palette::{Palette, current_theme};
use crate::settings_model::{KeymapRow, Page, keymap_rows, pages};
use crate::workbench_view::c;

/// What the dialog asks the workbench to do.
pub enum SettingsEvent {
    /// Switch to this theme.
    Theme(ThemeId),
    /// Give the action these shortcuts, or restore its bundled ones with `None`.
    Shortcuts(String, Option<Vec<Shortcut>>),
    /// The installed plugins changed: read them again.
    PluginsChanged,
    /// Apply and save these IDE settings (`SPEC-0129`).
    Ide(Box<ferris_idea_core::IdeSettings>),
    /// Run this command in a new terminal tab (signing in to a subscription, `SPEC-0160`).
    RunInTerminal(String),
    /// Open the user's own Claude Code in a new terminal tab of the project (`SPEC-0160` v2
    /// behaviour 7).
    OpenClaudeCode,
    /// A trusted location was removed: windows check their trust again (`SPEC-0278`).
    TrustChanged,
    /// The project's inspection profile changed: inspect the open editors again (`SPEC-0315`).
    InspectionsChanged,
    /// Close the dialog.
    Close,
}

/// The Settings dialog (behaviour 1).
pub struct SettingsView {
    query: String,
    /// The whole search is marked by Select All: typed text replaces it, ⌫ empties it
    /// (`SPEC-0105` v8 behaviour 10).
    query_marked: bool,
    /// The page list has the keys after a click on it: ↑/↓ go through the pages (`SPEC-0105` v8
    /// behaviour 11).
    list_has_keys: bool,
    /// The page list rolls to the page shown in the next frame.
    reveal_page: bool,
    /// The roll of the page list and of the page shown (`SPEC-0105` v8 behaviours 11, 12).
    page_list_scroll: gpui::ScrollHandle,
    page_scroll: gpui::ScrollHandle,
    page: Page,
    /// Selected row of the page: a theme or a keymap row.
    selected: usize,
    /// Actions without a bundled shortcut that are listed too.
    extra: Vec<&'static str>,
    rows: Vec<KeymapRow>,
    /// The next keystroke becomes a shortcut of the selected action (behaviour 4).
    recording: bool,
    /// A row of the keymap list was chosen (click, ↑/↓, Enter) since the search last changed:
    /// ⇧⌫ then restores also while the search holds text (`SPEC-0105` v4 behaviour 4).
    row_chosen: bool,
    /// The keymap list has the keys: a row was clicked or Tab left the search (v10 behaviour
    /// 16). ⌫ removes then, and Escape gives the keys back to the search.
    list_keys: bool,
    /// Note under the keymap list: conflicts or what was changed.
    note: Option<String>,
    /// The user's plugins folder and what it holds (Plugins page).
    plugins_dir: Option<PathBuf>,
    plugins: Vec<ferris_idea_plugins::Installed>,
    /// The plugin a first Delete asked to remove; a second Delete removes it.
    removing: Option<String>,
    /// A plugin file shown for review before it is installed (`SPEC-0211` behaviour 2).
    plugin_review: Option<(PathBuf, ferris_idea_plugins::Review)>,
    /// The permissions ticked in the review, granted when it installs (`SPEC-0276` behaviour 1).
    review_grants: Vec<ferris_idea_plugins::Permission>,
    /// The dictionary being downloaded (`SPEC-0159`).
    downloading: Option<&'static str>,
    /// IDE settings as the Editor, Font and Terminal pages show them (`SPEC-0129`).
    ide: ferris_idea_core::IdeSettings,
    /// The shell field of the Terminal page.
    shell: gpui::Entity<crate::editor_view::EditorView>,
    /// The protected branches field of the Version Control page (`SPEC-0150`).
    protected: gpui::Entity<crate::editor_view::EditorView>,
    /// Font families the Font page offers.
    fonts: Vec<String>,
    /// The Size and Line spacing fields of the Font page (`SPEC-0129` v7 behaviour 10).
    font_fields: [gpui::Entity<crate::editor_view::EditorView>; 2],
    /// The Font page's preview, a text of one's own in the editors' font.
    font_preview: gpui::Entity<crate::editor_view::EditorView>,
    /// The settings as the dialog found them when it opened: what "Revert changes" goes back to
    /// (`SPEC-0105` v7 behaviour 8).
    opened: ferris_idea_core::IdeSettings,
    /// The search field of Advanced Settings and its text (`SPEC-0337` v2 behaviour 5b).
    advanced_search: gpui::Entity<crate::editor_view::EditorView>,
    advanced_query: String,
    /// Advanced Settings shows only the settings changed from their default.
    advanced_modified_only: bool,
    /// Endpoint, model and API key fields of the AI Assistant page (`SPEC-0138`).
    ai_fields: [gpui::Entity<crate::editor_view::EditorView>; 3],
    /// Whether the keychain holds a key for the endpoint; read in the background when the page is
    /// shown (`None` meanwhile), never on the UI thread, so a system prompt for keychain access
    /// cannot block the window (OBS-0026 L26).
    ai_key_stored: Option<bool>,
    /// The endpoint whose key was read last (or is being read or written).
    ai_key_endpoint: Option<String>,
    /// Whether the user is signed in to the chosen subscription (`SPEC-0160`).
    ai_login: Option<ferris_idea_ai::subscription::Login>,
    /// The project whose JDKs and build tools the Build Tools page shows.
    root: Option<PathBuf>,
    /// Lines of the Build Tools and Languages pages, read when shown: text and whether it is a
    /// problem.
    info: Vec<(String, bool)>,
    /// Entries of the Trusted Locations page (`SPEC-0278` behaviour 8).
    trusted: Vec<ferris_idea_core::trust::TrustedLocation>,
    /// The text field of Advanced Settings and the key it edits (`SPEC-0337` behaviour 5).
    advanced_field: gpui::Entity<crate::editor_view::EditorView>,
    advanced_editing: Option<&'static str>,
    /// The project's inspection profile as the Inspections page shows it (`SPEC-0315`).
    inspection_profile: ferris_idea_languages::inspection_profile::Profile,
    /// The selected column of a file type's row on the Grammar and Style page
    /// (`SPEC-0155` v8 behaviour 14): 0 the file type, then comments, documentation, strings.
    grammar_column: usize,
    focus: FocusHandle,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    /// A dialog on `page`; `extra` are actions without a bundled shortcut.
    pub fn new(page: Page, extra: Vec<&'static str>, cx: &mut Context<Self>) -> Self {
        Self::with_plugins_dir(page, extra, ferris_idea_plugins::plugins_dir(), cx)
    }

    /// [`Self::new`] managing the plugins of `plugins_dir` from the start, so the user's folder is
    /// never read when another is meant (the Welcome window's tests, `SPEC-0355`).
    pub fn with_plugins_dir(
        page: Page,
        extra: Vec<&'static str>,
        plugins_dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            query: String::new(),
            query_marked: false,
            list_has_keys: false,
            reveal_page: true,
            page_list_scroll: gpui::ScrollHandle::new(),
            page_scroll: gpui::ScrollHandle::new(),
            page,
            selected: 0,
            extra,
            rows: Vec::new(),
            recording: false,
            row_chosen: false,
            list_keys: false,
            note: None,
            plugins_dir,
            plugins: Vec::new(),
            plugin_review: None,
            review_grants: Vec::new(),
            removing: None,
            downloading: None,
            ide: crate::appearance::ide_settings(cx),
            shell: cx.new(crate::editor_view::EditorView::field),
            protected: cx.new(crate::editor_view::EditorView::field),
            fonts: Vec::new(),
            font_fields: [
                cx.new(crate::editor_view::EditorView::field),
                cx.new(crate::editor_view::EditorView::field),
            ],
            font_preview: cx.new(|cx| {
                let mut f = crate::editor_view::EditorView::field(cx);
                f.set_text(FONT_PREVIEW, cx);
                f
            }),
            opened: Default::default(),
            advanced_search: cx.new(crate::editor_view::EditorView::field),
            advanced_query: String::new(),
            advanced_modified_only: false,
            ai_fields: [
                cx.new(crate::editor_view::EditorView::field),
                cx.new(crate::editor_view::EditorView::field),
                cx.new(crate::editor_view::EditorView::password_field),
            ],
            ai_key_stored: None,
            ai_key_endpoint: None,
            ai_login: None,
            root: None,
            info: Vec::new(),
            trusted: Vec::new(),
            advanced_field: cx.new(crate::editor_view::EditorView::field),
            advanced_editing: None,
            inspection_profile: Default::default(),
            grammar_column: 0,
            focus: cx.focus_handle(),
        };
        view.opened = view.ide.clone();
        view.sync_fields(cx);
        // The page's own search follows its field as one types (SPEC-0337 v2 behaviour 5b).
        cx.observe(&view.advanced_search, |this: &mut Self, f, cx| {
            let text = f.read(cx).state.buffer.text();
            if text != this.advanced_query {
                this.advanced_query = text;
                this.selected = 0;
                cx.notify();
            }
        })
        .detach();
        let installed = cx.text_system().all_font_names();
        view.fonts = crate::fonts::font_choices(&installed, &view.ide.editor.font_family);
        let ai = view.ide.ai.clone();
        view.ai_fields[0].update(cx, |f, cx| f.set_text(&ai.endpoint, cx));
        view.ai_fields[1].update(cx, |f, cx| f.set_text(&ai.model, cx));
        view.read_info(cx);
        view.refresh();
        if page == Page::Appearance {
            view.selected = ThemeId::ALL
                .iter()
                .position(|t| *t == current_theme())
                .unwrap_or(0);
        }
        view
    }

    /// Focus handle of the dialog.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The page shown.
    pub fn page(&self) -> Page {
        self.page
    }

    /// The pages the search leaves in the page list.
    pub fn pages(&self) -> Vec<Page> {
        pages(&self.query)
    }

    /// The keymap rows shown.
    pub fn rows(&self) -> &[KeymapRow] {
        &self.rows
    }

    /// Index of the selected row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Whether the next keystroke is recorded as a shortcut.
    pub fn recording(&self) -> bool {
        self.recording
    }

    /// The note under the keymap list.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Re-reads the keymap rows and the installed plugins, e.g. after a shortcut changed.
    pub fn refresh(&mut self) {
        let keymap = crate::keymaps::current();
        // The chosen keymap row stays chosen and listed when its shortcuts are removed, so that
        // Shift+Delete restores the same action (v10 behaviour 16).
        let kept = (self.page == Page::Keymap && self.row_chosen)
            .then(|| self.rows.get(self.selected).map(|r| r.action.clone()))
            .flatten();
        let mut extra: Vec<&str> = self.extra.clone();
        extra.extend(kept.as_deref());
        self.rows = keymap_rows(&keymap, &extra, &self.query);
        if let Some(i) = kept.and_then(|a| self.rows.iter().position(|r| r.action == a)) {
            self.selected = i;
        }
        self.plugins = self
            .plugins_dir
            .as_deref()
            .map(ferris_idea_plugins::installed)
            .unwrap_or_default();
        let n = self.count();
        if self.page != Page::Appearance {
            self.selected = self.selected.min(n.saturating_sub(1));
        }
    }

    /// Manages the plugins of `dir` instead of the user's folder (the self-test's own folder).
    pub fn set_plugins_dir(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.plugins_dir = Some(dir);
        self.refresh();
        cx.notify();
    }

    /// Shows the JDKs and build tools of the project at `root` on the Build Tools page.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.root = Some(root);
        self.read_info(cx);
        self.read_inspection_profile();
        cx.notify();
    }

    fn read_inspection_profile(&mut self) {
        if let Some(root) = &self.root {
            self.inspection_profile =
                ferris_idea_languages::inspection_profile::load(root).unwrap_or_default();
        }
    }

    /// Selects the inspection `id` on the Inspections page (`SPEC-0315` behaviour 5).
    pub fn select_inspection(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(i) = crate::settings_model::inspection_rows(&self.query)
            .iter()
            .position(|r| r.id == id)
        {
            self.selected = i;
        }
        cx.notify();
    }

    /// The ids of the inspections the Inspections page lists (tests).
    pub fn inspection_ids(&self) -> Vec<&'static str> {
        crate::settings_model::inspection_rows(&self.query)
            .iter()
            .map(|r| r.id)
            .collect()
    }

    /// Turns the selected inspection on or off, or (with `cycle`) gives it the next severity,
    /// and writes the profile (`SPEC-0315` behaviours 2, 4).
    pub fn change_inspection(&mut self, cycle: bool, cx: &mut Context<Self>) {
        use ferris_idea_languages::inspections::Level;
        let Some(root) = self.root.clone() else {
            return;
        };
        let rows = crate::settings_model::inspection_rows(&self.query);
        let Some(row) = rows.get(self.selected) else {
            return;
        };
        let (enabled, level) = self.inspection_profile.state(row.id);
        if cycle {
            let next = match level {
                Level::Error => Level::Warning,
                Level::Warning => Level::Weak,
                // "No highlighting, only fix" (SPEC-0315 v2 behaviour 7).
                Level::Weak => Level::Info,
                Level::Info => Level::Error,
            };
            self.inspection_profile.set(row.id, None, Some(next));
        } else {
            self.inspection_profile.set(row.id, Some(!enabled), None);
        }
        self.save_inspection_profile(&root, cx);
    }

    /// Space or Enter on the Grammar and Style page (`SPEC-0155` v8 behaviour 14): turns the
    /// selected switch over — commit messages, a file type or one of its contexts (the selected
    /// column), saved in the IDE settings; or a writing rule, saved in the project's inspection
    /// profile as on the Inspections page.
    pub fn toggle_grammar(&mut self, cx: &mut Context<Self>) {
        use crate::settings_model::{GrammarRow, grammar_rows, toggle_grammar};
        let Some(row) = grammar_rows().get(self.selected).copied() else {
            return;
        };
        match row {
            GrammarRow::Rule(id) => {
                let Some(root) = self.root.clone() else {
                    self.note = Some("The rules belong to a project: open one first".into());
                    cx.notify();
                    return;
                };
                let (enabled, _) = self.inspection_profile.state(id);
                self.inspection_profile.set(id, Some(!enabled), None);
                self.save_inspection_profile(&root, cx);
            }
            row => {
                let column = self.grammar_column;
                self.change_ide(
                    |s| _ = toggle_grammar(&mut s.editor.grammar, row, column),
                    cx,
                );
            }
        }
    }

    /// The selected column of the Grammar and Style page (tests).
    pub fn set_grammar_column(&mut self, column: usize) {
        self.grammar_column = column.min(crate::settings_model::GRAMMAR_COLUMNS.len() - 1);
    }

    /// Returns the selected inspection to its default (`SPEC-0315` behaviour 4).
    fn reset_inspection(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if let Some(row) = crate::settings_model::inspection_rows(&self.query).get(self.selected) {
            self.inspection_profile.reset(row.id);
            self.save_inspection_profile(&root, cx);
        }
    }

    fn save_inspection_profile(&mut self, root: &std::path::Path, cx: &mut Context<Self>) {
        match ferris_idea_languages::inspection_profile::save(root, &self.inspection_profile) {
            Ok(()) => self.note = None,
            Err(e) => self.note = Some(format!("Cannot write the inspection profile: {e}")),
        }
        cx.emit(SettingsEvent::InspectionsChanged);
        cx.notify();
    }

    /// The IDE settings the pages show.
    pub fn ide(&self) -> &ferris_idea_core::IdeSettings {
        &self.ide
    }

    /// The lines of the Build Tools or Languages page.
    pub fn info(&self) -> &[(String, bool)] {
        &self.info
    }

    /// Changes the IDE settings and asks the workbench to apply and save them (behaviour 1).
    pub fn change_ide(
        &mut self,
        change: impl FnOnce(&mut ferris_idea_core::IdeSettings),
        cx: &mut Context<Self>,
    ) {
        let mut ide = self.ide.clone();
        change(&mut ide);
        let ide = ide.clamped();
        if ide != self.ide {
            let font = (self.ide.editor.font_size, self.ide.editor.line_spacing)
                != (ide.editor.font_size, ide.editor.line_spacing);
            self.ide = ide.clone();
            if font {
                self.sync_font_fields(cx);
            }
            cx.emit(SettingsEvent::Ide(Box::new(ide)));
            cx.notify();
        }
    }

    /// Sets the text fields of the pages from the settings in the dialog: after Revert changes or
    /// Restore defaults they show what the page holds now.
    fn sync_fields(&mut self, cx: &mut Context<Self>) {
        let shell = self.ide.terminal.shell.clone().unwrap_or_default();
        self.shell.update(cx, |f, cx| f.set_text(&shell, cx));
        let protected = self.ide.vcs.protected_branches.clone();
        self.protected
            .update(cx, |f, cx| f.set_text(&protected, cx));
        self.sync_font_fields(cx);
    }

    /// The Size and Line spacing fields show the values in effect.
    fn sync_font_fields(&mut self, cx: &mut Context<Self>) {
        let size = format!("{}", self.ide.editor.font_size);
        let spacing = format!("{:.1}", self.ide.editor.line_spacing);
        self.font_fields[0].update(cx, |f, cx| f.set_text(&size, cx));
        self.font_fields[1].update(cx, |f, cx| f.set_text(&spacing, cx));
    }

    /// Takes the number typed into the Size (`i` 0) or Line spacing (1) field (`SPEC-0129` v7
    /// behaviour 10); a value out of range is clamped, a text that is no number is refused with a
    /// note and the field shows the value in effect again. Whether it was a number.
    pub fn apply_font_field(&mut self, i: usize, cx: &mut Context<Self>) -> bool {
        let text = self.font_fields[i].read(cx).state.buffer.text();
        let Ok(n) = text.trim().replace(',', ".").parse::<f32>() else {
            self.note = Some(format!("“{}” is not a number", text.trim()));
            self.sync_font_fields(cx);
            cx.notify();
            return false;
        };
        self.change_ide(
            |s| {
                if i == 0 {
                    s.editor.font_size = n;
                } else {
                    s.editor.line_spacing = n;
                }
            },
            cx,
        );
        self.note = None;
        self.sync_font_fields(cx);
        cx.notify();
        true
    }

    /// Chooses font family `k` of the list (`SPEC-0129` v7 behaviour 10).
    pub fn choose_font(&mut self, k: usize, cx: &mut Context<Self>) {
        if let Some(f) = self.fonts.get(k).cloned() {
            self.change_ide(|s| s.editor.font_family = f, cx);
        }
    }

    /// Whether the page shown differs from the settings the dialog opened with: "Revert changes"
    /// is offered (`SPEC-0105` v7 behaviour 8).
    pub fn page_changed(&self) -> bool {
        crate::settings_model::page_differs(self.page, &self.ide, &self.opened)
    }

    /// Whether the page shown holds a setting changed from its default: "Restore defaults" is
    /// offered.
    pub fn page_not_default(&self) -> bool {
        crate::settings_model::page_modified(self.page, &self.ide)
    }

    /// Puts the page shown back to the settings the dialog opened with (`SPEC-0105` v7 behaviour
    /// 8); the other pages keep their changes.
    pub fn revert_page(&mut self, cx: &mut Context<Self>) {
        let (page, opened) = (self.page, self.opened.clone());
        self.change_ide(
            |s| {
                crate::settings_model::copy_page(page, &opened, s);
            },
            cx,
        );
        self.sync_fields(cx);
        self.note = Some(format!("{} reverted", page.title()));
        cx.notify();
    }

    /// Puts the page shown back to the defaults (`SPEC-0105` v7 behaviour 8).
    pub fn restore_page_defaults(&mut self, cx: &mut Context<Self>) {
        let page = self.page;
        self.change_ide(
            |s| {
                crate::settings_model::copy_page(
                    page,
                    &ferris_idea_core::IdeSettings::default(),
                    s,
                );
            },
            cx,
        );
        self.sync_fields(cx);
        self.note = Some(format!("{}: defaults restored", page.title()));
        cx.notify();
    }

    /// Switches "Show modified only" of Advanced Settings (`SPEC-0337` v2 behaviour 5b).
    pub fn toggle_advanced_modified_only(&mut self, cx: &mut Context<Self>) {
        self.advanced_modified_only = !self.advanced_modified_only;
        self.selected = 0;
        cx.notify();
    }

    /// The text of the Font page's preview.
    pub fn font_preview_text(&self, cx: &gpui::App) -> String {
        self.font_preview.read(cx).state.buffer.text()
    }

    /// The text of the Advanced Settings search.
    pub fn advanced_query(&self) -> &str {
        &self.advanced_query
    }

    /// Font families the Font page offers.
    pub fn font_choices(&self) -> &[String] {
        &self.fonts
    }

    /// Selects row `i` of the page (tests).
    pub fn select_row(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = i.min(self.count().saturating_sub(1));
        self.row_chosen = true;
        cx.notify();
    }

    /// Reads the IDE settings in effect again (after a theme was chosen).
    pub fn reload_ide(&mut self, cx: &mut Context<Self>) {
        self.ide = crate::appearance::ide_settings(cx);
        cx.notify();
    }

    /// Switches compact mode (`SPEC-0165`).
    pub fn toggle_compact(&mut self, cx: &mut Context<Self>) {
        self.change_ide(
            |s| s.appearance.compact_mode = !s.appearance.compact_mode,
            cx,
        );
    }

    /// Switches the startup animation (`SPEC-0355` v2 behaviour 23).
    pub fn toggle_startup_animation(&mut self, cx: &mut Context<Self>) {
        self.change_ide(
            |s| s.appearance.startup_animation = !s.appearance.startup_animation,
            cx,
        );
    }

    /// Switches following the system's light or dark appearance (`SPEC-0131` behaviour 1).
    pub fn toggle_sync(&mut self, cx: &mut Context<Self>) {
        self.change_ide(
            |s| s.appearance.sync_with_os = !s.appearance.sync_with_os,
            cx,
        );
    }

    /// Takes the shell field's text as the terminal's shell (Enter on the Terminal page).
    pub fn apply_shell(&mut self, cx: &mut Context<Self>) {
        let text = self.shell.read(cx).state.buffer.text().trim().to_owned();
        self.change_ide(
            |s| s.terminal.shell = (!text.is_empty()).then_some(text),
            cx,
        );
        self.note = Some(match &self.ide.terminal.shell {
            Some(sh) => format!("New terminals start {sh}"),
            None => "New terminals start $SHELL".into(),
        });
        cx.notify();
    }

    /// Takes the AI Assistant fields (Enter on its page, `SPEC-0138` behaviour 1): endpoint and
    /// model go to the settings; a typed API key goes to the system keychain for that endpoint
    /// and the field is emptied.
    pub fn apply_ai(&mut self, cx: &mut Context<Self>) {
        let text = |f: &gpui::Entity<crate::editor_view::EditorView>, cx: &mut Context<Self>| {
            f.read(cx).state.buffer.text().trim().to_owned()
        };
        let endpoint = text(&self.ai_fields[0], cx);
        let model = text(&self.ai_fields[1], cx);
        let key = text(&self.ai_fields[2], cx);
        self.change_ide(
            |s| {
                s.ai.endpoint = endpoint.clone();
                s.ai.model = model.clone();
            },
            cx,
        );
        let note = if ferris_idea_ai::claude_in_terminal(&self.ide.ai) {
            if ferris_idea_ai::provider_of(&endpoint) == Some("Anthropic") {
                format!(
                    "Claude's plan runs in the Terminal; questions go to {endpoint} with your API key"
                )
            } else {
                "Claude's plan runs in the Terminal; nothing is sent from AI Chat without the Anthropic API".to_owned()
            }
        } else if let Some(cli) =
            ferris_idea_ai::subscription::Cli::from_id(&self.ide.ai.subscription)
        {
            format!("Questions go to {}", cli.name())
        } else if endpoint.is_empty() {
            "No endpoint: nothing can be sent".to_owned()
        } else {
            format!("Questions go to {endpoint}")
        };
        if !key.is_empty() {
            // The keychain is written off the UI thread (OBS-0026 L26): a system prompt for
            // keychain access must not block the window. The model list waits for the key.
            self.ai_fields[2].update(cx, |f, cx| f.set_text("", cx));
            self.ai_key_stored = None;
            self.ai_key_endpoint = Some(endpoint.clone());
            self.note = Some(format!(
                "{note}; storing the API key in the keychain… (if the system asks for keychain access, allow it)"
            ));
            let target = endpoint.clone();
            let task = cx.background_executor().spawn(async move {
                use ferris_idea_database::SecretStore;
                ferris_idea_ai::key_ref(&target)
                    .map_err(|e| e.to_string())
                    .and_then(|r| {
                        ferris_idea_database::SystemStore
                            .set(&r, &ferris_idea_database::Secret::new(key))
                            .map_err(|e| e.to_string())
                    })
            });
            cx.spawn(async move |this, cx| {
                let stored = task.await;
                let _ = this.update(cx, |this, cx| {
                    if this.ai_key_endpoint.as_deref() == Some(endpoint.as_str()) {
                        this.ai_key_stored = Some(stored.is_ok());
                    }
                    let note = match stored {
                        Ok(()) => format!("{note}; API key stored in the keychain"),
                        Err(e) => format!("{note}; API key not stored: {e}"),
                    };
                    this.note = Some(note.clone());
                    if !endpoint.is_empty()
                        && (this.ide.ai.subscription.is_empty()
                            || ferris_idea_ai::claude_in_terminal(&this.ide.ai))
                    {
                        this.list_models(endpoint, note, cx);
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }
        // Another endpoint may have another key: read it in the background.
        self.read_ai_key(cx);
        self.note = Some(note.clone());
        if !endpoint.is_empty()
            && (self.ide.ai.subscription.is_empty()
                || ferris_idea_ai::claude_in_terminal(&self.ide.ai))
        {
            self.list_models(endpoint, note, cx);
        }
        cx.notify();
    }

    /// Reads in the background whether the keychain holds a key for the endpoint (OBS-0026 L26),
    /// once per endpoint; the page shows "checking" meanwhile and is drawn again with the answer.
    fn read_ai_key(&mut self, cx: &mut Context<Self>) {
        let endpoint = self.ide.ai.endpoint.clone();
        if self.ai_key_endpoint.as_deref() == Some(endpoint.as_str()) {
            return;
        }
        self.ai_key_endpoint = Some(endpoint.clone());
        self.ai_key_stored = None;
        let target = endpoint.clone();
        let task = cx.background_executor().spawn(async move {
            ferris_idea_ai::stored_key(&ferris_idea_database::SystemStore, &target).is_some()
        });
        cx.spawn(async move |this, cx| {
            let stored = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.ai_key_endpoint.as_deref() == Some(endpoint.as_str()) {
                    this.ai_key_stored = Some(stored);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Whether the keychain holds a key for the endpoint: `None` while it is being read (tests).
    pub fn ai_key_stored(&self) -> Option<bool> {
        self.ai_key_stored
    }

    /// Fetches the endpoint's model list in the background and appends it to the note
    /// (`SPEC-0143`); no model is chosen for the user.
    fn list_models(&mut self, endpoint: String, note: String, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async move {
            let key = ferris_idea_ai::stored_key(&ferris_idea_database::SystemStore, &endpoint);
            ferris_idea_ai::list_models(&endpoint, key.as_deref())
        });
        cx.spawn(async move |this, cx| {
            let models = task.await;
            let _ = this.update(cx, |this, cx| {
                // A later apply replaced the note: its own fetch reports.
                if this.note.as_deref() != Some(note.as_str()) {
                    return;
                }
                this.note = Some(models_note(&note, models));
                cx.notify();
            });
        })
        .detach();
    }

    /// Chooses the next provider (`SPEC-0143`, `SPEC-0160`): first the chat subscriptions, whose
    /// programs Ferris IDEA runs, then the API templates, which fill the endpoint.
    pub fn cycle_provider(&mut self, cx: &mut Context<Self>) {
        use ferris_idea_ai::subscription::{CLIS, Cli};
        let n = CLIS.len() + ferris_idea_ai::PROVIDERS.len();
        let current = match Cli::from_id(&self.ide.ai.subscription) {
            Some(cli) => CLIS.iter().position(|c| *c == cli),
            None => {
                let endpoint = self.ai_fields[0].read(cx).state.buffer.text();
                ferris_idea_ai::provider_of(&endpoint).and_then(|name| {
                    ferris_idea_ai::PROVIDERS
                        .iter()
                        .position(|(n, _)| *n == name)
                        .map(|i| i + CLIS.len())
                })
            }
        };
        let next = current.map_or(0, |i| (i + 1) % n);
        if let Some(cli) = CLIS.get(next).copied() {
            self.change_ide(|s| s.ai.subscription = cli.id().to_owned(), cx);
            self.note = Some(format!(
                "{}: Ferris IDEA runs `{}` with your subscription; the API key is not used",
                cli.name(),
                cli.program()
            ));
            self.check_login(cx);
        } else {
            let (name, url) = ferris_idea_ai::PROVIDERS[next - CLIS.len()];
            self.change_ide(|s| s.ai.subscription.clear(), cx);
            self.ai_login = None;
            self.ai_fields[0].update(cx, |f, cx| f.set_text(url, cx));
            self.note = Some(format!(
                "{name}: endpoint {url}; enter the model and API key, then Enter applies"
            ));
        }
        cx.notify();
    }

    /// Fills the Anthropic API template (`SPEC-0143`): Claude in AI Chat with the user's own API
    /// key, which goes to the keychain as for every endpoint (`SPEC-0160` v2 behaviour 6). The
    /// subscription stays: with Claude's plan in the Terminal the chat then uses this endpoint.
    /// Model and key wait for the user; Enter applies.
    pub fn use_anthropic_api(&mut self, cx: &mut Context<Self>) {
        let Some((name, url)) = ferris_idea_ai::PROVIDERS
            .iter()
            .copied()
            .find(|(n, _)| *n == "Anthropic")
        else {
            return;
        };
        self.selected = 1;
        self.ai_fields[0].update(cx, |f, cx| f.set_text(url, cx));
        self.note = Some(format!(
            "{name}: endpoint {url}; enter the model and your Anthropic API key, then Enter applies"
        ));
        cx.notify();
    }

    /// Switches how Claude with the plan is used (`SPEC-0160` v2 behaviour 6): Chat (the default)
    /// or Terminal.
    pub fn toggle_claude_plan_mode(&mut self, cx: &mut Context<Self>) {
        use ferris_idea_core::ClaudePlanMode;
        let next = match self.ide.ai.claude_plan_mode {
            ClaudePlanMode::Chat => ClaudePlanMode::Terminal,
            ClaudePlanMode::Terminal => ClaudePlanMode::Chat,
        };
        self.change_ide(|s| s.ai.claude_plan_mode = next, cx);
        self.note = Some(match next {
            ClaudePlanMode::Chat => "Claude plan: Chat — AI Chat runs Claude Code with your plan".into(),
            ClaudePlanMode::Terminal => "Claude plan: Terminal — Sign in opens Claude Code in a terminal; AI Chat uses Claude with your Anthropic API key".into(),
        });
        cx.notify();
    }

    /// Asks the chosen subscription's program in the background whether the user is signed in.
    fn check_login(&mut self, cx: &mut Context<Self>) {
        let Some(cli) = ferris_idea_ai::subscription::Cli::from_id(&self.ide.ai.subscription)
        else {
            self.ai_login = None;
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { ferris_idea_ai::subscription::login(cli) });
        cx.spawn(async move |this, cx| {
            let login = task.await;
            let _ = this.update(cx, |this, cx| {
                if ferris_idea_ai::subscription::Cli::from_id(&this.ide.ai.subscription)
                    == Some(cli)
                {
                    this.ai_login = Some(login);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Sign in (Enter on its row): runs the subscription program's sign-in in a new terminal tab,
    /// where the program asks for the account itself (`SPEC-0160` behaviour 1).
    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        use ferris_idea_ai::subscription::{Cli, Login};
        if ferris_idea_ai::claude_in_terminal(&self.ide.ai)
            && self.ai_login != Some(Login::NotInstalled)
        {
            // Claude's plan in the Terminal: Claude Code itself, where it signs in by itself
            // (SPEC-0160 v2 behaviour 7).
            cx.emit(SettingsEvent::OpenClaudeCode);
            return;
        }
        let Some(cli) = Cli::from_id(&self.ide.ai.subscription) else {
            self.note = Some("Sign in is for a subscription; choose one as the provider".into());
            cx.notify();
            return;
        };
        if self.ai_login == Some(Login::NotInstalled) {
            self.note = Some(format!(
                "{} is not installed; {}",
                cli.program(),
                cli.install_hint()
            ));
            cx.notify();
            return;
        }
        cx.emit(SettingsEvent::RunInTerminal(cli.login_command().to_owned()));
    }

    /// Whether the user is signed in to the chosen subscription, once checked (tests).
    pub fn ai_login(&self) -> Option<ferris_idea_ai::subscription::Login> {
        self.ai_login
    }

    /// Sets the AI Assistant's endpoint and model fields (tests).
    pub fn set_ai_fields(&mut self, endpoint: &str, model: &str, cx: &mut Context<Self>) {
        self.ai_fields[0].update(cx, |f, cx| f.set_text(endpoint, cx));
        self.ai_fields[1].update(cx, |f, cx| f.set_text(model, cx));
    }

    /// Switches the AI Assistant on or off.
    pub fn toggle_ai(&mut self, cx: &mut Context<Self>) {
        self.change_ide(|s| s.ai.enabled = !s.ai.enabled, cx);
    }

    /// Enter on the Natural Languages page: uses the selected dictionary, downloading it first
    /// when it is neither there nor built in (`SPEC-0159` v2 behaviours 2–3).
    pub fn use_dictionary(&mut self, cx: &mut Context<Self>) {
        use ferris_idea_languages::dictionaries as d;
        let Some(entry) = d::CATALOG.get(self.selected) else {
            return;
        };
        let code = entry.code;
        let dir = d::dictionaries_dir();
        let installed = dir.as_deref().is_some_and(|dir| d::is_installed(dir, code));
        if installed || d::is_built_in(code) {
            self.change_ide(|s| s.editor.spelling_dictionary = code.to_owned(), cx);
            self.note = Some(if installed {
                format!("{} is used for typos", entry.name)
            } else {
                format!("{} is built in and used for typos", entry.name)
            });
            cx.notify();
            return;
        }
        let Some(dir) = dir else {
            return;
        };
        if self.downloading.is_some() {
            return;
        }
        self.downloading = Some(code);
        self.note = Some(format!(
            "Downloading {} from the LibreOffice project ({} KB)…",
            entry.name,
            (entry.dic.0 + entry.aff.0) / 1024
        ));
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            d::install(&dir, code, &|url| {
                let request = ferris_idea_http::ResolvedRequest {
                    name: None,
                    line: 0,
                    method: "GET".into(),
                    url: url.to_owned(),
                    headers: Vec::new(),
                    body: String::new(),
                    unresolved: Vec::new(),
                };
                let response = ferris_idea_http::send(&request, std::time::Duration::from_secs(60))
                    .map_err(|e| e.to_string())?;
                if response.status != 200 {
                    return Err(format!("{url}: HTTP {}", response.status));
                }
                Ok(response.body)
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.downloading = None;
                match result {
                    Ok(()) => {
                        this.change_ide(|s| s.editor.spelling_dictionary = code.to_owned(), cx);
                        this.note = Some(format!(
                            "{} downloaded, checked and used for typos",
                            d::entry(code).map_or(code, |e| e.name)
                        ));
                    }
                    Err(e) => this.note = Some(format!("Not downloaded: {e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Delete on the Natural Languages page: removes the selected dictionary's files; a built-in
    /// one stays, only a file of its code left by an earlier download goes (`SPEC-0159` v4
    /// behaviour 2).
    pub fn remove_dictionary(&mut self, cx: &mut Context<Self>) {
        use ferris_idea_languages::dictionaries as d;
        let (Some(dir), Some(entry)) = (d::dictionaries_dir(), d::CATALOG.get(self.selected))
        else {
            return;
        };
        if d::is_built_in(entry.code) && !d::is_installed(&dir, entry.code) {
            self.note = Some(format!("{} is built in; nothing to remove", entry.name));
            cx.notify();
            return;
        }
        self.note = Some(match d::remove(&dir, entry.code) {
            Ok(()) if d::is_built_in(entry.code) => format!(
                "The downloaded copy of {} removed; the built-in one stays",
                entry.name
            ),
            Ok(()) => format!("{} removed", entry.name),
            Err(e) => e,
        });
        // The removed dictionary leaves the ones used (SPEC-0159 v4); a built-in one stays.
        let chosen = d::chosen_codes(&self.ide.editor.spelling_dictionary);
        if chosen.contains(&entry.code) && !d::is_built_in(entry.code) {
            let rest: Vec<&str> = chosen.into_iter().filter(|c| *c != entry.code).collect();
            self.change_ide(|s| s.editor.spelling_dictionary = rest.join(","), cx);
        }
        cx.notify();
    }

    /// Space on the Natural Languages page: adds the selected dictionary to the ones used for
    /// typos, or takes it away; the last one stays (`SPEC-0159` v4 behaviour 3). A dictionary that
    /// is neither built in nor downloaded is downloaded with Enter first.
    pub fn toggle_dictionary(&mut self, cx: &mut Context<Self>) {
        use ferris_idea_languages::dictionaries as d;
        let Some(entry) = d::CATALOG.get(self.selected) else {
            return;
        };
        let dir = d::dictionaries_dir();
        if !d::usable(dir.as_deref(), entry.code) {
            self.note = Some(format!("Press Enter to download {} first", entry.name));
            cx.notify();
            return;
        }
        let next = d::toggled(
            dir.as_deref(),
            &self.ide.editor.spelling_dictionary,
            entry.code,
        );
        let names: Vec<&str> = d::chosen_codes(&next)
            .iter()
            .filter_map(|c| d::entry(c).map(|e| e.name))
            .collect();
        self.note = Some(format!("Used for typos: {}", names.join(", ")));
        self.change_ide(|s| s.editor.spelling_dictionary = next, cx);
        cx.notify();
    }

    /// The dictionary being downloaded (tests).
    pub fn downloading(&self) -> Option<&'static str> {
        self.downloading
    }

    /// Switches the window's MCP server over HTTP on or off (`SPEC-0221` behaviour 9).
    pub fn toggle_mcp_http(&mut self, cx: &mut Context<Self>) {
        self.change_ide(|s| s.mcp.http = !s.mcp.http, cx);
    }

    /// Lets agents and scripts operate the window, or not (`SPEC-0319` behaviour 32): only here,
    /// by the user; no MCP tool writes it.
    pub fn toggle_ui_driver_external(&mut self, cx: &mut Context<Self>) {
        self.change_ide(|s| s.ui.driver.external = !s.ui.driver.external, cx);
    }

    /// Switches inline completion on or off (`SPEC-0157` behaviour 3).
    pub fn toggle_inline_completion(&mut self, cx: &mut Context<Self>) {
        self.change_ide(|s| s.ai.inline_completion = !s.ai.inline_completion, cx);
    }

    /// Puts the keyboard focus into the shell field (checks).
    pub fn focus_shell(&self, window: &mut Window, cx: &App) {
        window.focus(&self.shell.read(cx).focus());
    }

    /// The shell field's text (checks).
    pub fn shell_text(&self, cx: &App) -> String {
        self.shell.read(cx).state.buffer.text()
    }

    /// Sets the shell field (tests).
    pub fn set_shell_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.shell.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Reads what the Build Tools and Languages pages show, and starts reading whether an AI key
    /// is stored (in the background).
    fn read_info(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Ai {
            self.read_ai_key(cx);
        }
        self.info = match self.page {
            Page::BuildTools => build_tools_info(self.root.as_deref()),
            Page::Languages => languages_info(),
            Page::TrustedLocations => self.trust_info(),
            _ => Vec::new(),
        };
    }

    /// Reads the trusted locations and says whether the current project is trusted and why
    /// (`SPEC-0278` behaviour 8).
    fn trust_info(&mut self) -> Vec<(String, bool)> {
        let settings = ferris_idea_core::trust::TrustSettings::process();
        let mut out = Vec::new();
        match settings.locations() {
            Ok(list) => self.trusted = list,
            Err(e) => {
                self.trusted.clear();
                out.push((format!("The trust file cannot be read: {e}"), true));
            }
        }
        self.selected = self.selected.min(self.trusted.len().saturating_sub(1));
        if let Some(root) = &self.root {
            let status = settings.status(root);
            out.push(match &status.source {
                Some(source) => (
                    format!("This project is trusted: {}", source.describe()),
                    false,
                ),
                None => (
                    "This project is in safe mode: nothing of it runs until you trust it".into(),
                    true,
                ),
            });
        }
        if let Some(env) = &settings.env {
            out.push((
                format!(
                    "{}={env} trusts projects for this session",
                    ferris_idea_core::trust::TRUST_ENV
                ),
                false,
            ));
        }
        out
    }

    /// Delete on the Trusted Locations page: removes the selected entry (`SPEC-0278` behaviour 8).
    fn remove_trusted(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.trusted.get(self.selected).cloned() else {
            return;
        };
        let Some(file) = ferris_idea_core::trust::trust_file() else {
            return;
        };
        self.note = Some(
            match ferris_idea_core::trust::remove_location(&file, &entry.path) {
                Ok(_) => {
                    cx.emit(SettingsEvent::TrustChanged);
                    format!(
                        "{} removed: its projects open in safe mode",
                        entry.path.display()
                    )
                }
                Err(e) => e,
            },
        );
        self.read_info(cx);
        cx.notify();
    }

    /// Left/Right or −/+ on a Font row, Enter on an Editor row (behaviour 2–3).
    fn adjust(&mut self, step: f32, cx: &mut Context<Self>) {
        let row = self.selected;
        match self.page {
            Page::Font => {
                let fonts = self.fonts.clone();
                self.change_ide(
                    |s| match row {
                        0 => {
                            let n = fonts.len().max(1) as isize;
                            let at = fonts
                                .iter()
                                .position(|f| *f == s.editor.font_family)
                                .unwrap_or(0) as isize;
                            let next = (at + step.signum() as isize).rem_euclid(n) as usize;
                            if let Some(f) = fonts.get(next) {
                                s.editor.font_family = f.clone();
                            }
                        }
                        1 => s.editor.font_size += step,
                        2 => s.editor.line_spacing += step / 10.0,
                        // Any key or click switches ligatures (SPEC-0150 v2 behaviour 1a).
                        _ => s.editor.ligatures = !s.editor.ligatures,
                    },
                    cx,
                )
            }
            // Left/Right change the number, Enter or Space switch (SPEC-0150 behaviour 2).
            Page::CodeStyle => self.change_ide(
                |s| {
                    let cs = &mut s.code_style;
                    match row {
                        0 => cs.right_margin = (cs.right_margin as f32 + step).max(0.0) as u32,
                        1 => cs.show_right_margin = !cs.show_right_margin,
                        _ => {
                            let Some(lang) = ferris_idea_core::CODE_STYLE_LANGUAGES.get(row - 2)
                            else {
                                return;
                            };
                            let mut style = cs.indent_for(lang);
                            if step == 0.0 {
                                style.use_tabs = !style.use_tabs;
                            } else {
                                style.indent = (style.indent as f32 + step).max(1.0) as u32;
                                style.tab = style.indent;
                            }
                            cs.indents.insert((*lang).to_owned(), style);
                        }
                    }
                },
                cx,
            ),
            Page::VersionControl => self.change_ide(
                |s| match row {
                    0 => {
                        s.vcs.update_method = match s.vcs.update_method {
                            ferris_idea_core::UpdateMethod::Merge => {
                                ferris_idea_core::UpdateMethod::Rebase
                            }
                            ferris_idea_core::UpdateMethod::Rebase => {
                                ferris_idea_core::UpdateMethod::Merge
                            }
                        }
                    }
                    1 => s.vcs.allow_force_push = !s.vcs.allow_force_push,
                    // Git over SSH to Azure DevOps (ADR-0025 section 1a).
                    3 => {
                        let a = &mut s.git.ssh.allow_classic_kex_for_azure_devops;
                        *a = !*a;
                    }
                    // An own ProxyCommand on SSH paths (ADR-0025 v5 section 1a).
                    4 => {
                        let a = &mut s.ssh.allow_own_proxy_command;
                        *a = !*a;
                    }
                    _ => {}
                },
                cx,
            ),
            // A disabled intention's row turns it on again (SPEC-0076 v11 behaviour 15).
            Page::Editor if row >= 12 => {
                let title = self.ide.intentions.disabled.get(row - 12).cloned();
                if let Some(title) = title {
                    self.change_ide(|s| _ = s.intentions.set_enabled(&title, true), cx);
                    let last = self.count().saturating_sub(1);
                    self.selected = self.selected.min(last);
                }
            }
            Page::Editor => self.change_ide(
                |s| match row {
                    0 => s.editor.strip_trailing_spaces = !s.editor.strip_trailing_spaces,
                    1 => s.editor.ensure_line_break_at_end = !s.editor.ensure_line_break_at_end,
                    2 => s.editor.semantic_highlighting = !s.editor.semantic_highlighting,
                    3 => s.editor.parameter_hints = !s.editor.parameter_hints,
                    4 => s.editor.usage_counts = !s.editor.usage_counts,
                    5 => s.editor.usage_counts_above = !s.editor.usage_counts_above,
                    // SPEC-0292 v4 behaviour 7f.
                    6 => s.editor.code_authors = !s.editor.code_authors,
                    // SPEC-0295 behaviour 12.
                    7 => {
                        s.editor.highlight_usages_at_caret = !s.editor.highlight_usages_at_caret;
                    }
                    8 => s.editor.fold_one_line_methods = !s.editor.fold_one_line_methods,
                    // SPEC-0083 v4 behaviour 10.
                    9 => s.editor.fold_imports = !s.editor.fold_imports,
                    // SPEC-0115 v5 behaviour 13.
                    10 => s.editor.config_values = !s.editor.config_values,
                    // SPEC-0083 v8 behaviour 22: always / on hover.
                    _ => {
                        use ferris_idea_core::FoldingArrows::{Always, Hover};
                        s.editor.folding_arrows = match s.editor.folding_arrows {
                            Always => Hover,
                            Hover => Always,
                        };
                    }
                },
                cx,
            ),
            // Editor Tabs (SPEC-0059 v9 behaviour 17): Enter and Space move forward.
            Page::EditorTabs => self.change_ide(
                |s| {
                    let by = if step == 0.0 { 1 } else { step as i32 };
                    // A switch toggles once for Left and Right alike.
                    crate::settings_model::change_tab_setting(&mut s.editor.tabs, row, by)
                },
                cx,
            ),
            // Terminal lines to attach, in steps of 50 (`SPEC-0313` behaviour 2).
            Page::Ai if row == 5 => self.change_ide(
                |s| {
                    let now = ferris_idea_ai::context::terminal_lines(s.ai.terminal_lines) as f32;
                    s.ai.terminal_lines = ferris_idea_ai::context::terminal_lines(
                        (now + step * 50.0).max(1.0) as usize,
                    );
                },
                cx,
            ),
            _ => {}
        }
    }

    /// The plugins the Plugins page lists.
    pub fn plugins(&self) -> &[ferris_idea_plugins::Installed] {
        &self.plugins
    }

    /// Shows the plugin module at `path` for review before installing it (Install Plugin from
    /// Disk…, SPEC-0126 behaviour 2, SPEC-0211 behaviour 2); a file that does not load is refused.
    pub fn install_plugin(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(dir) = self.plugins_dir.clone() else {
            self.note = Some("No settings folder for plugins".into());
            cx.notify();
            return;
        };
        self.page = Page::Plugins;
        match ferris_idea_plugins::review(&dir, path) {
            Ok(review) => {
                self.note = None;
                // What is granted now stays ticked; nothing new is ticked by itself.
                self.review_grants = review
                    .permissions
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(p, _)| *p)
                    .collect();
                self.plugin_review = Some((path.to_path_buf(), review));
            }
            Err(e) => {
                self.plugin_review = None;
                self.note = Some(format!("Not installed: {e}"));
            }
        }
        cx.notify();
    }

    /// The plugin file under review (tests).
    pub fn plugin_review(&self) -> Option<&ferris_idea_plugins::Review> {
        self.plugin_review.as_ref().map(|(_, r)| r)
    }

    /// Enter on a review: installs the reviewed file (SPEC-0211 behaviour 2).
    pub fn confirm_plugin_install(&mut self, cx: &mut Context<Self>) {
        let (Some(dir), Some((_, review))) = (self.plugins_dir.clone(), self.plugin_review.take())
        else {
            return;
        };
        let ticked = std::mem::take(&mut self.review_grants);
        // Exactly the module the user reviewed, not the file again (SPEC-0211 v3 behaviour 7).
        self.note = Some(match ferris_idea_plugins::install_reviewed(&dir, &review) {
            Ok(m) => {
                // Granted at install: what the user ticked in the review (SPEC-0276 behaviour 1).
                for p in &m.permissions {
                    let _ = ferris_idea_plugins::set_granted(
                        &dir,
                        &review.file,
                        *p,
                        ticked.contains(p),
                    );
                }
                if !ticked.contains(&ferris_idea_plugins::Permission::SuggestCommands) {
                    forget_plugin_runs(&review.file);
                }
                cx.emit(SettingsEvent::PluginsChanged);
                let done = review.headline();
                let done = done
                    .replacen("Install ", "Installed ", 1)
                    .replacen("Update ", "Updated ", 1)
                    .replacen("Reinstall ", "Reinstalled ", 1)
                    .replacen("Downgrade ", "Downgraded ", 1)
                    .replacen("Replace ", "Replaced ", 1);
                let what = ferris_idea_plugins::contributions(&m);
                if what.is_empty() {
                    done
                } else {
                    format!("{done}: {}", what.join(", "))
                }
            }
            Err(e) => format!("Not installed: {e}"),
        });
        self.refresh();
        cx.notify();
    }

    /// Space or a click on a review: ticks or unticks the permissions it asks for, all of them or
    /// only `only` (SPEC-0276 behaviour 1).
    pub fn toggle_review_grant(
        &mut self,
        only: Option<ferris_idea_plugins::Permission>,
        cx: &mut Context<Self>,
    ) {
        let Some((_, review)) = &self.plugin_review else {
            return;
        };
        let asked: Vec<ferris_idea_plugins::Permission> = review
            .permissions
            .iter()
            .map(|(p, _)| *p)
            .filter(|p| only.is_none_or(|o| o == *p))
            .collect();
        let on = asked.iter().any(|p| !self.review_grants.contains(p));
        self.review_grants.retain(|p| !asked.contains(p));
        if on {
            self.review_grants.extend(asked);
        }
        cx.notify();
    }

    /// The permissions ticked in the review (tests).
    pub fn review_grants(&self) -> &[ferris_idea_plugins::Permission] {
        &self.review_grants
    }

    /// Escape on a review: drops it.
    pub fn cancel_plugin_review(&mut self, cx: &mut Context<Self>) {
        if self.plugin_review.take().is_some() {
            self.note = Some("Nothing installed".into());
            cx.notify();
        }
    }

    /// Asks for a `.wasm` file and installs it.
    fn choose_plugin_file(&mut self, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Install Plugin".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| this.install_plugin(&path, cx));
            }
        })
        .detach();
    }

    /// The review of a plugin file: what installing does, what it contributes and asks for, and
    /// the buttons (SPEC-0211 behaviour 2).
    fn plugin_review_box(
        &self,
        review: &ferris_idea_plugins::Review,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        // An older version, or another plugin under the same file name, is a warning (SPEC-0211
        // v3 behaviour 8).
        let downgrade = matches!(
            review.kind,
            ferris_idea_plugins::ReviewKind::Downgrade { .. }
                | ferris_idea_plugins::ReviewKind::Replace { .. }
        );
        let warning = c(p.status_text(crate::palette::Status::Warning));
        // Each permission with a tick: Install grants what is ticked (SPEC-0276 behaviour 1).
        let asks: Vec<(ferris_idea_plugins::Permission, bool)> = review
            .permissions
            .iter()
            .map(|(perm, _)| (*perm, self.review_grants.contains(perm)))
            .collect();
        div()
            .id("plugin-review")
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.separator))
            .child(
                div()
                    .relative()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .when(downgrade, |d| d.text_color(warning))
                    .semantics(
                        crate::semantics::Semantics::new(crate::semantics::Role::Heading)
                            .name(review.headline())
                            .test_id("plugin-review-headline"),
                    )
                    .child(review.headline()),
            )
            .when(!review.manifest.description.is_empty(), |d| {
                d.child(review.manifest.description.clone())
            })
            .child(div().text_color(c(p.ui_muted)).child(format!(
                "Contributes: {}",
                if review.contributions.is_empty() {
                    "nothing".to_owned()
                } else {
                    review.contributions.join(", ")
                }
            )))
            .when(asks.is_empty(), |d| {
                d.child(
                    div()
                        .text_color(c(p.ui_muted))
                        .child("Asks for no permissions"),
                )
            })
            .when(!asks.is_empty(), |d| {
                d.child(
                    div()
                        .text_color(c(p.ui_muted))
                        .child("Permissions (Space or click to grant at install):"),
                )
                .children(asks.into_iter().enumerate().map(|(i, (perm, on))| {
                    div()
                        .id(("plugin-review-grant", i))
                        .relative()
                        // Granting a plugin's rights is the user's decision (SPEC-0319 UISEC-4).
                        .semantics(
                            crate::semantics::Semantics::new(crate::semantics::Role::CheckBox)
                                .name(perm.describe())
                                .test_id(format!("plugin-review-grant-{i}"))
                                .checked(on)
                                .action(crate::semantics::AxAction::Click)
                                .effects(&[crate::semantics::Effect::None])
                                .reserved(crate::semantics::Reserved::Decision),
                        )
                        .pl_2()
                        .flex()
                        .gap_2()
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(if on { "☑" } else { "☐" })
                        .child(format!(
                            "{} ({})",
                            perm.describe(),
                            if on { "grant" } else { "not granted" }
                        ))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.toggle_review_grant(Some(perm), cx)
                        }))
                }))
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        crate::metrics::button("plugin-review-cancel", "Cancel", false)
                            .relative()
                            .semantics(
                                crate::semantics::Semantics::new(crate::semantics::Role::Button)
                                    .name("Cancel")
                                    .test_id("plugin-review-cancel")
                                    .action(crate::semantics::AxAction::Click)
                                    .effects(&[crate::semantics::Effect::None]),
                            )
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.cancel_plugin_review(cx)
                            })),
                    )
                    .child(
                        crate::metrics::button("plugin-review-install", "Install", true)
                            .relative()
                            // Installing a plugin is the user's decision (SPEC-0319 UISEC-4).
                            .semantics(
                                crate::semantics::Semantics::new(crate::semantics::Role::Button)
                                    .name("Install")
                                    .test_id("plugin-review-install")
                                    .action(crate::semantics::AxAction::Click)
                                    .effects(&[crate::semantics::Effect::None])
                                    .reserved(crate::semantics::Reserved::Decision),
                            )
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.confirm_plugin_install(cx)
                            })),
                    ),
            )
    }

    /// Enter on the Plugins page: enables or disables the selected plugin (behaviour 3).
    fn toggle_plugin(&mut self, cx: &mut Context<Self>) {
        let (Some(dir), Some(p)) = (self.plugins_dir.clone(), self.plugins.get(self.selected))
        else {
            return;
        };
        let (file, enable) = (p.file.clone(), !p.enabled);
        self.note = Some(
            match ferris_idea_plugins::set_enabled(&dir, &file, enable) {
                Ok(()) => {
                    cx.emit(SettingsEvent::PluginsChanged);
                    format!("{file} {}", if enable { "enabled" } else { "disabled" })
                }
                Err(e) => e.to_string(),
            },
        );
        self.refresh();
    }

    /// Space on the Plugins page: grants or withdraws the permissions the selected plugin asks for
    /// — all of them when one is missing, else none (`SPEC-0156` behaviour 3, `SPEC-0276`
    /// behaviour 1). Withdrawing `suggest-commands` forgets every "always allow" of the plugin.
    pub fn toggle_plugin_grant(&mut self, cx: &mut Context<Self>) {
        let (Some(dir), Some(p)) = (self.plugins_dir.clone(), self.plugins.get(self.selected))
        else {
            return;
        };
        let asks: Vec<ferris_idea_plugins::Permission> = p
            .manifest
            .as_ref()
            .map(|m| m.permissions.clone())
            .unwrap_or_default();
        let file = p.file.clone();
        if asks.is_empty() {
            self.note = Some(format!("{file} asks for no permissions"));
            cx.notify();
            return;
        }
        let on = asks
            .iter()
            .any(|perm| !ferris_idea_plugins::granted(&dir, &file, *perm));
        let result = asks
            .iter()
            .try_for_each(|perm| ferris_idea_plugins::set_granted(&dir, &file, *perm, on));
        if !on {
            forget_plugin_runs(&file);
        }
        let what: Vec<&str> = asks.iter().map(|perm| perm.describe()).collect();
        self.note = Some(match result {
            Ok(()) => format!(
                "{file} {}: {}",
                if on { "granted" } else { "withdrawn" },
                what.join(", ")
            ),
            Err(e) => e.to_string(),
        });
        self.refresh();
        cx.notify();
    }

    /// ⇧Delete on the Plugins page: revokes every network grant of the selected plugin
    /// (`SPEC-0314` behaviour 7).
    pub fn revoke_plugin_network(&mut self, cx: &mut Context<Self>) {
        let (Some(dir), Some(p)) = (self.plugins_dir.clone(), self.plugins.get(self.selected))
        else {
            return;
        };
        let file = p.file.clone();
        self.note = Some(
            match ferris_idea_plugins::revoke_network(&dir, &file, None, None) {
                Ok(0) => format!("{file} has no network grants"),
                Ok(n) => format!(
                    "{file}: {n} network grant{} revoked",
                    if n == 1 { "" } else { "s" }
                ),
                Err(e) => e.to_string(),
            },
        );
        self.refresh();
        cx.notify();
    }

    /// The network grants of plugin `file` as the Plugins page shows them (`SPEC-0314`).
    pub fn plugin_network_lines(&self, file: &str) -> Vec<String> {
        self.plugins_dir
            .as_deref()
            .map(ferris_idea_plugins::network_grants)
            .unwrap_or_default()
            .into_iter()
            .filter(|g| g.plugin == file)
            .map(|g| {
                format!(
                    "network: {} ({}, {})",
                    g.target().label(),
                    g.holder.label(),
                    g.granted.get(..10).unwrap_or(&g.granted)
                )
            })
            .collect()
    }

    /// Whether the plugin `file` was granted project access (tests).
    pub fn plugin_granted(&self, file: &str) -> bool {
        self.plugins_dir.as_deref().is_some_and(|d| {
            ferris_idea_plugins::granted(d, file, ferris_idea_plugins::Permission::ProjectRead)
        })
    }

    /// Delete on the Plugins page: asks once, removes on the second Delete (behaviour 4).
    fn remove_plugin(&mut self, cx: &mut Context<Self>) {
        let (Some(dir), Some(p)) = (self.plugins_dir.clone(), self.plugins.get(self.selected))
        else {
            return;
        };
        let file = p.file.clone();
        if self.removing.as_deref() != Some(file.as_str()) {
            self.note = Some(format!("Press Delete again to remove {file}"));
            self.removing = Some(file);
            return;
        }
        self.removing = None;
        self.note = Some(match ferris_idea_plugins::uninstall(&dir, &file) {
            Ok(()) => {
                forget_plugin_runs(&file);
                cx.emit(SettingsEvent::PluginsChanged);
                format!("{file} removed")
            }
            Err(e) => e.to_string(),
        });
        self.refresh();
    }

    /// Shows `page` (behaviour 2).
    pub fn show(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.page {
            self.page_scroll.set_offset(gpui::Point::default());
            // A page's note belongs to that page (`SPEC-0105` v10 behaviour 15).
            self.note = None;
        }
        self.list_keys = false;
        self.page = page;
        self.reveal_page = true;
        self.selected = 0;
        self.recording = false;
        self.row_chosen = false;
        self.refresh();
        self.read_info(cx);
        if page == Page::Ai {
            self.check_login(cx);
        }
        cx.notify();
    }

    fn count(&self) -> usize {
        match self.page {
            // The themes, the editor scheme (SPEC-0337), Sync with OS (SPEC-0131), Compact
            // mode (SPEC-0165) and the startup animation (SPEC-0355).
            Page::Appearance => ThemeId::ALL.len() + 4,
            // Confirm before quitting, Reopen the last project, Save on focus loss (SPEC-0337).
            Page::SystemSettings => 3,
            Page::Notifications => 1,
            Page::Advanced => self.advanced_rows().len(),
            Page::Keymap => self.rows.len(),
            Page::Plugins => self.plugins.len(),
            // Two On Save options, then the code insight switches (SPEC-0290, SPEC-0295).
            // Then one row per disabled intention (SPEC-0076 v11 behaviour 15).
            Page::Editor => 12 + self.ide.intentions.disabled.len(),
            // Font, Size, Line spacing, then Enable ligatures (SPEC-0150 v2).
            Page::Font => 4,
            Page::EditorTabs => {
                crate::settings_model::tab_setting_rows(&self.ide.editor.tabs).len()
            }
            // Right margin, Show it, then one row per language.
            Page::CodeStyle => 2 + ferris_idea_core::CODE_STYLE_LANGUAGES.len(),
            // Update method, Allow force push, Protected branches, Azure DevOps key exchange,
            // own ProxyCommand (ADR-0025 v5).
            Page::VersionControl => 5,
            Page::NaturalLanguages => ferris_idea_languages::dictionaries::CATALOG.len(),
            Page::Grammar => crate::settings_model::grammar_rows().len(),
            Page::Inspections => crate::settings_model::inspection_rows(&self.query).len(),
            Page::BuildTools | Page::Languages | Page::Terminal => 0,
            Page::TrustedLocations => self.trusted.len(),
            // Enable, then Provider (SPEC-0143).
            // Enable, Provider, Inline completion, Sign in (SPEC-0160), MCP over HTTP (SPEC-0221),
            // Terminal lines to attach (SPEC-0313), Allow agents and scripts to operate the
            // window (SPEC-0319), Claude plan (SPEC-0160 v2).
            Page::Ai => 8,
        }
    }

    fn selected_action(&self) -> Option<String> {
        (self.page == Page::Keymap)
            .then(|| self.rows.get(self.selected).map(|r| r.action.clone()))
            .flatten()
    }

    /// Enter: chooses the theme or starts recording a shortcut for the selected action.
    fn activate(&mut self, cx: &mut Context<Self>) {
        match self.page {
            Page::Appearance => {
                if let Some(t) = ThemeId::ALL.get(self.selected) {
                    cx.emit(SettingsEvent::Theme(*t));
                } else if self.selected == ThemeId::ALL.len() {
                    self.cycle_scheme(1, cx);
                } else if self.selected == ThemeId::ALL.len() + 1 {
                    self.toggle_sync(cx);
                } else if self.selected == ThemeId::ALL.len() + 2 {
                    self.toggle_compact(cx);
                } else {
                    self.toggle_startup_animation(cx);
                }
            }
            Page::Keymap => {
                if self.selected_action().is_some() {
                    self.row_chosen = true;
                    self.recording = true;
                    self.note = Some("Press the shortcut (Escape cancels)".into());
                }
            }
            Page::Plugins => self.toggle_plugin(cx),
            Page::SystemSettings | Page::Notifications => self.toggle_general(self.selected, cx),
            Page::Advanced => self.step_advanced(0, cx),
            Page::NaturalLanguages => self.use_dictionary(cx),
            Page::Grammar => self.toggle_grammar(cx),
            Page::Inspections => self.change_inspection(true, cx),
            Page::Editor | Page::EditorTabs => self.adjust(0.0, cx),
            Page::Font if self.selected == 3 => self.adjust(0.0, cx),
            Page::CodeStyle if self.selected != 0 => self.adjust(0.0, cx),
            Page::VersionControl if self.selected != 2 => self.adjust(0.0, cx),
            Page::Terminal => self.apply_shell(cx),
            Page::Ai if self.selected == 1 => self.cycle_provider(cx),
            Page::Ai if self.selected == 2 => self.toggle_inline_completion(cx),
            Page::Ai if self.selected == 3 => self.sign_in(cx),
            Page::Ai if self.selected == 4 => self.toggle_mcp_http(cx),
            Page::Ai if self.selected == 6 => self.toggle_ui_driver_external(cx),
            Page::Ai if self.selected == 7 => self.toggle_claude_plan_mode(cx),
            Page::Ai => self.toggle_ai(cx),
            _ => {}
        }
    }

    /// Adds `stroke` to the selected action's shortcuts and notes the actions that share it
    /// (behaviour 4).
    fn record(&mut self, stroke: Keystroke, cx: &mut Context<Self>) {
        self.recording = false;
        let Some(action) = self.selected_action() else {
            return;
        };
        let keymap = crate::keymaps::current();
        let others: Vec<String> = keymap
            .actions_for(&stroke, None)
            .into_iter()
            .filter(|id| *id != action)
            .map(str::to_owned)
            .collect();
        // A chord macOS takes itself never arrives; the note says so (v12 behaviour 18).
        let system = keymap
            .macos
            .then(|| crate::settings_model::macos_reserved(&stroke))
            .flatten()
            .map(|what| format!("; macOS takes it itself ({what}), so it will not arrive"))
            .unwrap_or_default();
        let mut shortcuts = keymap.shortcuts(&action).to_vec();
        let new = Shortcut::Keyboard {
            first: stroke,
            second: None,
        };
        let shown = new.display(keymap.macos);
        if !shortcuts.contains(&new) {
            shortcuts.push(new);
        }
        let title = ferris_idea_languages::everywhere::action_title(&action).0;
        self.note = Some(if others.is_empty() {
            format!("{shown} added to {title}{system}")
        } else {
            format!(
                "{shown} added to {title}; also used by {}{system}",
                others.join(", ")
            )
        });
        cx.emit(SettingsEvent::Shortcuts(action, Some(shortcuts)));
    }

    /// Delete removes the selected action's shortcuts; Shift+Delete restores the bundled ones.
    fn remove(&mut self, restore: bool, cx: &mut Context<Self>) {
        let Some(action) = self.selected_action() else {
            return;
        };
        let title = ferris_idea_languages::everywhere::action_title(&action).0;
        self.note = Some(if restore {
            format!("{title}: bundled shortcuts restored")
        } else {
            format!("{title}: shortcuts removed")
        });
        cx.emit(SettingsEvent::Shortcuts(action, (!restore).then(Vec::new)));
    }

    /// Keys of the text fields, before the field itself sees them (D19, hand test 56): Enter
    /// applies the field, Escape and Tab leave it; every other key types into the field.
    fn on_field_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The AI fields keep their typing; Enter applies them, Escape and Tab leave them.
        if self
            .ai_fields
            .iter()
            .any(|f| f.read(cx).focus().is_focused(window))
        {
            match k.key.as_str() {
                "enter" => self.apply_ai(cx),
                "escape" | "tab" => window.focus(&self.focus),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // The Size and Line spacing fields keep their typing; Enter applies, Escape and Tab
        // leave (SPEC-0129 v7 behaviour 10).
        if let Some(i) = self
            .font_fields
            .iter()
            .position(|f| f.read(cx).focus().is_focused(window))
        {
            match k.key.as_str() {
                "enter" => {
                    if self.apply_font_field(i, cx) {
                        window.focus(&self.focus);
                    }
                }
                "escape" | "tab" => {
                    self.sync_font_fields(cx);
                    window.focus(&self.focus);
                }
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // The preview and the Advanced Settings search keep their typing; Escape and Tab leave,
        // and Enter leaves the search.
        let preview = self.font_preview.read(cx).focus().is_focused(window);
        let search = self.advanced_search.read(cx).focus().is_focused(window);
        if preview || search {
            match k.key.as_str() {
                "escape" | "tab" => window.focus(&self.focus),
                "enter" if search => window.focus(&self.focus),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // The Advanced Settings field keeps its typing; Enter applies it (SPEC-0337 behaviour 5).
        if self.advanced_field.read(cx).focus().is_focused(window) {
            match k.key.as_str() {
                "enter" => {
                    if self.apply_advanced_field(cx) {
                        self.leave_advanced_field(window);
                    }
                }
                "escape" | "tab" => self.leave_advanced_field(window),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // The protected branches field keeps its typing; Enter applies it (SPEC-0150 behaviour 4).
        if self.protected.read(cx).focus().is_focused(window) {
            match k.key.as_str() {
                "enter" => {
                    let text = self.protected.read(cx).state.buffer.text();
                    self.change_ide(|s| s.vcs.protected_branches = text.trim().to_owned(), cx);
                    self.note = Some("Protected branches saved".into());
                    window.focus(&self.focus);
                }
                "escape" | "tab" => window.focus(&self.focus),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // The shell field keeps its typing; Enter applies it, Escape and Tab leave it.
        if self.shell.read(cx).focus().is_focused(window) {
            match k.key.as_str() {
                "enter" => self.apply_shell(cx),
                "escape" | "tab" => window.focus(&self.focus),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        // A plugin under review takes Enter and Escape (SPEC-0211 behaviour 2).
        if self.page == Page::Plugins && self.plugin_review.is_some() {
            match k.key.as_str() {
                "enter" => self.confirm_plugin_install(cx),
                "escape" => self.cancel_plugin_review(cx),
                "space" => self.toggle_review_grant(None, cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if self.recording {
            if k.key == "escape" {
                self.recording = false;
                self.note = None;
            } else if let Some(stroke) = ferris_idea_ui::keymap::EventStrokes::of(
                &k.key,
                m.control,
                m.alt,
                m.shift,
                m.platform,
                cx.keyboard_mapper().get_key_equivalents(),
            )
            .recorded()
            {
                // ⌃⇧1 is recorded as the digit's chord on every layout (SPEC-0028 v5
                // behaviour 10).
                self.record(stroke, cx);
            } else {
                // A modifier alone: wait for the key.
                return;
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // While a search is typed, printable keys extend it; a space no longer switches a setting
        // of the page the search moved to (D9, hand test 30: "save all" became "saveall").
        // The text itself arrives through the input handler (`SPEC-0105` v3 behaviour 3a).
        // After a click on an option the page has the keys: Space switches it (v11 behaviour 17).
        if k.key_char.as_deref().is_some_and(|ch| {
            options::types_into_search(&self.query, ch, m.control || m.platform, self.list_keys)
        }) {
            return;
        }
        // Select All, Copy, Cut, Paste, Undo and Redo act on the search here, before the menu
        // bar would take them (`SPEC-0105` v8 behaviour 10, OBS-0062 S6).
        if let Some(id) = edit_actions::edit_action_of(event)
            && self.edit_action(&id, window, cx)
        {
            crate::key_diagnostics::note_ran(&id, "settings search");
            cx.stop_propagation();
            return;
        }
        // Any other key ends the mark; ⌫ or ⌦ empty the marked search. An event without a key
        // (the test harness sends one before typed text) is no key.
        if k.key.is_empty() {
            return;
        }
        let marked = std::mem::take(&mut self.query_marked);
        if marked {
            cx.notify();
        }
        if marked
            && !(m.shift || m.alt || m.control || m.platform)
            && matches!(k.key.as_str(), "backspace" | "delete")
        {
            self.query.clear();
            self.after_query(cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Mac keyboards have no forward Delete key without fn: with the search empty, ⌫ removes
        // and ⇧⌫ restores like Delete and ⇧Delete (D10, hand test 30; D18). With a search typed,
        // ⇧⌫ still restores once a keymap row was chosen; ⌫ alone keeps editing the search.
        // While the keymap list or an option has the keys, ⌫ is theirs too (v10 behaviour 16,
        // v11 behaviour 17).
        let on_row = self.list_keys || self.page == Page::Keymap && m.shift && self.row_chosen;
        // Escape on the keymap list, or on an option with a search typed, gives the keys back to
        // the search instead of closing the dialog (v10 behaviour 16, v11 behaviour 17).
        if self.list_keys
            && (self.page == Page::Keymap || !self.query.is_empty())
            && k.key == "escape"
            && !self.list_has_keys
        {
            self.list_keys = false;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let key = list_key(&k.key, &self.query, on_row);
        let n = self.count();
        if key != "delete" {
            self.removing = None;
        }
        // After a click on the page list, ↑/↓ go through the pages; Enter and → give the keys
        // back to the page, whose values the list's keys leave alone (`SPEC-0105` v8
        // behaviour 11).
        let handled = if self.list_has_keys {
            match key {
                "down" | "up" => {
                    let list = self.pages();
                    if let Some(next) =
                        crate::settings_model::step_page(&list, self.page, key == "down")
                    {
                        self.show(next, cx);
                    }
                    true
                }
                "enter" | "right" => {
                    self.list_has_keys = false;
                    true
                }
                "left" | "space" | "delete" | "-" | "+" | "=" => true,
                "escape" | "tab" => self.on_page_key(key, m, n, window, cx),
                "backspace" => {
                    self.query.pop();
                    self.after_query(cx);
                    true
                }
                // Printable keys type into the search through the input handler.
                _ => false,
            }
        } else {
            self.on_page_key(key, m, n, window, cx)
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// A key on the page shown, for its rows and values (`key` as [`list_key`] gives it, `n`
    /// rows); `false` for keys the page leaves to others, such as printable ones, which type into
    /// the search through the input handler.
    fn on_page_key(
        &mut self,
        key: &str,
        m: &gpui::Modifiers,
        n: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match key {
            "left" | "-" if self.page == Page::Font => self.adjust(-1.0, cx),
            // The columns of a file type's row (SPEC-0155 v8 behaviour 14).
            "left" if self.page == Page::Grammar => {
                self.grammar_column = self.grammar_column.saturating_sub(1);
                cx.notify();
            }
            "right" if self.page == Page::Grammar => {
                self.grammar_column =
                    (self.grammar_column + 1).min(crate::settings_model::GRAMMAR_COLUMNS.len() - 1);
                cx.notify();
            }
            "left" | "-" if self.page == Page::EditorTabs => self.adjust(-1.0, cx),
            "right" | "+" | "=" if self.page == Page::EditorTabs => self.adjust(1.0, cx),
            "left" | "-" if self.page == Page::Ai && self.selected == 5 => self.adjust(-1.0, cx),
            "right" | "+" | "=" if self.page == Page::Ai && self.selected == 5 => {
                self.adjust(1.0, cx)
            }
            "right" | "+" | "=" if self.page == Page::Font => self.adjust(1.0, cx),
            "left" | "-" if self.page == Page::CodeStyle && self.selected != 1 => {
                self.adjust(-1.0, cx)
            }
            "right" | "+" | "=" if self.page == Page::CodeStyle && self.selected != 1 => {
                self.adjust(1.0, cx)
            }
            "enter" if self.page == Page::VersionControl && self.selected == 2 => {
                let f = self.protected.read(cx).focus();
                window.focus(&f);
            }
            // SPEC-0337 behaviours 4 and 5.
            "left" | "-"
                if self.page == Page::Appearance && self.selected == ThemeId::ALL.len() =>
            {
                self.cycle_scheme(-1, cx)
            }
            "right" | "+" | "=" | "space"
                if self.page == Page::Appearance && self.selected == ThemeId::ALL.len() =>
            {
                self.cycle_scheme(1, cx)
            }
            "left" | "-" if self.page == Page::Advanced => self.step_advanced(-1, cx),
            "right" | "+" | "=" if self.page == Page::Advanced => self.step_advanced(1, cx),
            "enter" if self.page == Page::Advanced && self.edit_advanced(window, cx) => {}
            "delete" if self.page == Page::Advanced => self.reset_advanced(cx),
            // Space switches the selected option of every page (v11 behaviour 17).
            "space" if self.switch_option(cx) => {}
            // The rows stop at the first and the last one: ↑ on the first row no longer jumped to
            // the end and changed the value there (hand test 76).
            "down" if n > 0 => {
                self.selected = (self.selected + 1).min(n - 1);
                self.row_chosen = true;
            }
            "up" if n > 0 => {
                self.selected = self.selected.saturating_sub(1);
                self.row_chosen = true;
            }
            "enter" => self.activate(cx),
            "escape" => cx.emit(SettingsEvent::Close),
            // Tab from the search gives the keys to the keymap list (v10 behaviour 16).
            "tab" if self.page == Page::Keymap && !self.query.is_empty() && !self.list_keys => {
                self.list_keys = true;
                self.row_chosen = true;
            }
            "tab" => {
                let list = self.pages();
                let at = list.iter().position(|p| *p == self.page).unwrap_or(0);
                if let Some(next) = list.get((at + 1) % list.len().max(1)).copied() {
                    self.show(next, cx);
                }
            }
            "delete" if self.page == Page::Plugins && m.shift => self.revoke_plugin_network(cx),
            "delete" if self.page == Page::Plugins => self.remove_plugin(cx),
            "delete" if self.page == Page::NaturalLanguages => self.remove_dictionary(cx),
            "delete" | "backspace" if self.page == Page::TrustedLocations => {
                self.remove_trusted(cx)
            }
            "space" if self.page == Page::Plugins => self.toggle_plugin_grant(cx),
            "space" if self.page == Page::NaturalLanguages => self.toggle_dictionary(cx),
            "space" if self.page == Page::Grammar => self.toggle_grammar(cx),
            "space" if self.page == Page::Inspections => self.change_inspection(false, cx),
            "delete" | "backspace" if self.page == Page::Inspections && self.query.is_empty() => {
                self.reset_inspection(cx)
            }
            "o" if self.page == Page::Plugins && (m.platform || m.control) => {
                self.choose_plugin_file(cx)
            }
            "delete" => self.remove(m.shift, cx),
            "backspace" => {
                self.query.pop();
                self.after_query(cx);
            }
            // Printable keys type into the search through the input handler.
            _ => return false,
        }
        true
    }

    /// The search moves to the first page it leaves (behaviour 3).
    fn after_query(&mut self, cx: &mut Context<Self>) {
        let list = self.pages();
        let before = self.page;
        self.reveal_page = true;
        if !self.query.is_empty() {
            // A page found by its name or keywords before the keymap that always searches.
            self.page = list
                .iter()
                .copied()
                .find(|p| *p != Page::Keymap)
                .unwrap_or(Page::Keymap);
        } else if !list.contains(&self.page) {
            self.page = list.first().copied().unwrap_or(Page::Keymap);
        }
        if self.page != before {
            self.page_scroll.set_offset(gpui::Point::default());
            self.note = None;
        }
        // Editing the search gives the keys back to it (v10 behaviour 16).
        self.list_keys = false;
        self.selected = 0;
        self.row_chosen = false;
        self.refresh();
        self.read_info(cx);
    }
}

impl EntityInputHandler for SettingsView {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _actual: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reviewing = self.page == Page::Plugins && self.plugin_review.is_some();
        if self.recording || reviewing || !searchable_text(text) {
            return;
        }
        // A search never starts with a space: Space on a row that switches nothing no longer
        // lands in the empty search (`SPEC-0105` v11 behaviour 17, window check G1).
        if (self.query.is_empty() || self.query_marked) && text.trim().is_empty() {
            return;
        }
        // Text typed over the marked search replaces it (`SPEC-0105` v8 behaviour 10).
        if std::mem::take(&mut self.query_marked) {
            self.query.clear();
        }
        self.query.push_str(text);
        self.after_query(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        _text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Composition is shown by the input method's own window; only the commit counts.
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// Whether committed text goes into the search: no control characters.
fn searchable_text(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

/// Whether typed text `ch` goes on with a search already typed (D9).
fn extends_query(query: &str, ch: &str, command: bool) -> bool {
    !query.is_empty() && !command && !ch.is_empty() && !ch.chars().any(char::is_control)
}

/// The key the settings list acts on: with the search empty, ⌫ counts as Delete (D10). `on_row`
/// says that ⇧ is held on the keymap page after a row was chosen: then ⇧⌫ counts as ⇧Delete also
/// while the search holds text (hand test 30: ⇧⌫ on the row found by "save all" took a letter
/// off the search instead of restoring).
fn list_key<'a>(key: &'a str, query: &str, on_row: bool) -> &'a str {
    match key {
        "backspace" if query.is_empty() || on_row => "delete",
        other => other,
    }
}

/// A small button beside a value (‹ and ›).
fn small_button(
    id: (&'static str, usize),
    label: &'static str,
    p: &Palette,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(22.))
        .h(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(c(p.separator))
        .hover(|s| s.bg(c(p.tree_hover)))
        .child(label)
}

/// The Build Tools page: JDKs with the one chosen for the project, its Java requirements and
/// problems, and how Maven and Gradle run (`SPEC-0129` behaviour 5).
fn build_tools_info(root: Option<&Path>) -> Vec<(String, bool)> {
    let Some(root) = root else {
        return vec![("No project".into(), false)];
    };
    let env = ferris_idea_core::java_environment(&ferris_idea_core::JavaSearch::from_env(root));
    let mut out = vec![("JDKs".into(), false)];
    if env.jdks.is_empty() {
        out.push((
            "  none found (JAVA_HOME, /Library/Java, SDKMAN, PATH)".into(),
            true,
        ));
    }
    for (i, j) in env.jdks.iter().enumerate() {
        out.push((
            format!(
                "{} {} {} — {}{}",
                if env.selected == Some(i) {
                    "●"
                } else {
                    "○"
                },
                j.feature
                    .map(|f| f.to_string())
                    .unwrap_or_else(|| "?".into()),
                j.vendor.clone().unwrap_or_default(),
                j.home,
                if j.has_compiler { "" } else { " (no javac)" }
            ),
            false,
        ));
    }
    if let Some(reason) = &env.reason {
        out.push((format!("Chosen: {reason}"), false));
    }
    for p in &env.problems {
        out.push((p.clone(), true));
    }
    let on_path = |p: &str| {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(p).is_file()))
    };
    out.push(("Build tools".into(), false));
    for (wrapper, tool, file) in [
        ("mvnw", "mvn", "pom.xml"),
        ("gradlew", "gradle", "build.gradle"),
    ] {
        let used = root.join(file).exists() || root.join(format!("{file}.kts")).exists();
        let how = if root.join(wrapper).is_file() {
            format!("{tool}: the project's ./{wrapper}")
        } else if on_path(tool) {
            format!("{tool}: from PATH")
        } else {
            format!("{tool}: not installed")
        };
        out.push((
            format!(
                "  {how}{}",
                if used { " (used by this project)" } else { "" }
            ),
            used && how.ends_with("not installed"),
        ));
    }
    out
}

/// The Languages & Frameworks page: each language's server, or what to install (`SPEC-0129`
/// behaviour 5, IMP-0019).
fn languages_info() -> Vec<(String, bool)> {
    ferris_idea_languages::servers::overview()
        .into_iter()
        .map(|l| match l.selected {
            Some(program) => (format!("{}: {program}", l.language), false),
            None => {
                let hint = l
                    .candidates
                    .first()
                    .map(|c| c.candidate.install)
                    .unwrap_or("no server known");
                (format!("{}: missing — {hint}", l.language), true)
            }
        })
        .collect()
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let page = self.page;
        let selected = self.selected;
        // The row of the page shown has the focused selection's colour while the list has the
        // keys (`SPEC-0105` v8 behaviour 11).
        let list_selection = if self.list_has_keys {
            p.tree_selection
        } else {
            p.tree_selection_inactive
        };
        // Pages of a group stand under its name, indented, as in the reference's tree.
        let mut last_group = None;
        let mut page_list = Vec::new();
        let mut shown_row = None;
        for pg in self.pages() {
            if pg.group() != last_group
                && let Some(g) = pg.group()
            {
                page_list.push(
                    div()
                        .h(px(24.))
                        .px_3()
                        .mx_1()
                        .flex()
                        .items_center()
                        .text_color(c(p.ui_muted))
                        .child(g)
                        .into_any_element(),
                );
            }
            last_group = pg.group();
            if pg == page {
                shown_row = Some(page_list.len());
            }
            page_list.push(
                div()
                    .id(pg.title())
                    .h(px(26.))
                    .px_3()
                    .when(pg.group().is_some(), |d| d.pl_6())
                    .mx_1()
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .when(pg == page, |d| d.bg(c(list_selection)))
                    .when(pg != page, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(pg.title())
                    // A page whose settings go with the project (`SPEC-0105` v9 behaviour 13).
                    .when(pg.scope().0 == crate::settings_model::Scope::Project, |d| {
                        d.child(
                            div()
                                .pl_2()
                                .text_xs()
                                .text_color(c(p.ui_muted))
                                .child("project")
                                .semantics(
                                    Semantics::new(Role::Label)
                                        .name("Settings of this project")
                                        .test_id(format!("settings-page-project-{}", pg.title())),
                                ),
                        )
                    })
                    // A dot after a page with a setting changed from its default (`SPEC-0105` v7
                    // behaviour 9).
                    .when(crate::settings_model::page_modified(pg, &self.ide), |d| {
                        d.child(
                            div().pl_1().text_color(c(p.link)).child("•").semantics(
                                Semantics::new(Role::Label)
                                    .name("Modified")
                                    .test_id(format!("settings-page-modified-{}", pg.title())),
                            ),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.list_has_keys = true;
                        this.show(pg, cx)
                    }))
                    .semantics(
                        Semantics::new(Role::ListItem)
                            .name(pg.title())
                            .test_id(format!("settings-page-{}", pg.title()))
                            .selected(pg == page)
                            .action(AxAction::Click)
                            .effects(&[Effect::None]),
                    )
                    .into_any_element(),
            );
        }
        let row = |id: (&'static str, usize), on: bool| {
            div()
                .id(id)
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tree_selection)))
                .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
        };
        let small_button = |id: (&'static str, usize), label: &'static str| {
            div()
                .id(id)
                .w(px(22.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(label)
        };
        let info_list = |lines: &[(String, bool)]| {
            let error = c(p.status_text(crate::palette::Status::Error));
            // The lines roll with their page (`SPEC-0129` v8 behaviour 11).
            div()
                .id("settings-info")
                .flex()
                .flex_col()
                .gap_0p5()
                .children(lines.iter().map(|(text, problem)| {
                    let heading = !text.starts_with(' ')
                        && !text.contains(':')
                        && !text.starts_with('●')
                        && !text.starts_with('○');
                    div()
                        .when(*problem, |d| d.text_color(error))
                        .when(heading, |d| d.pt_1().text_color(c(p.ui_muted)))
                        .child(text.clone())
                }))
        };
        let ide = self.ide.clone();
        let query = self.query.clone();
        let content = match page {
            Page::EditorTabs => {
                let rows = crate::settings_model::tab_setting_rows(&ide.editor.tabs);
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(rows.into_iter().enumerate().map(|(i, (label, value))| {
                        let switch = value == "☑" || value == "☐";
                        // A switch is an option (v11 behaviour 17).
                        if switch {
                            return self.option_row(
                                ("settings-tabs", i),
                                format!("settings-tabs-{i}"),
                                options::Mark::Check(value == "☑"),
                                label,
                                cx,
                            );
                        }
                        row(("settings-tabs", i), i == selected)
                                .child(div().w(px(260.)).child(label))
                                    .child(small_button(("tabs-less", i), "‹").on_click(
                                        cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.selected = i;
                                            this.adjust(-1.0, cx);
                                        }),
                                    ))
                                    .child(div().min_w(px(150.)).child(value.clone()))
                                    .child(small_button(("tabs-more", i), "›").on_click(
                                        cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.selected = i;
                                            this.adjust(1.0, cx);
                                        }),
                                    ))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                                this.give_page_keys(i, window);
                                cx.notify();
                            }))
                    }))
                    .child(
                        div()
                            .pt_2()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child("Enter or Space switches, Left and Right change the value; every group follows at once"),
                    )
                    .into_any_element()
            }
            Page::Editor => {
                // SPEC-0083 v8 behaviour 22: the last row is a choice of two, shown by its value.
                let arrows = match ide.editor.folding_arrows {
                    ferris_idea_core::FoldingArrows::Always => "Show code folding arrows: always",
                    ferris_idea_core::FoldingArrows::Hover => "Show code folding arrows: on hover",
                };
                let checks = [
                    (
                        "Remove trailing spaces on save (not on the caret line)",
                        ide.editor.strip_trailing_spaces,
                    ),
                    (
                        "Ensure every saved file ends with a line break",
                        ide.editor.ensure_line_break_at_end,
                    ),
                    (
                        "Semantic highlighting from the language server",
                        ide.editor.semantic_highlighting,
                    ),
                    ("Show parameter name hints", ide.editor.parameter_hints),
                    ("Show usages of declarations", ide.editor.usage_counts),
                    (
                        "Show usages above the declaration (else at the line end)",
                        ide.editor.usage_counts_above,
                    ),
                    ("Show code author", ide.editor.code_authors),
                    // SPEC-0295 behaviour 12.
                    (
                        "Highlight usages of the symbol at the caret",
                        ide.editor.highlight_usages_at_caret,
                    ),
                    (
                        "Fold one-line methods when a file opens",
                        ide.editor.fold_one_line_methods,
                    ),
                    // SPEC-0083 v4 behaviour 10.
                    (
                        "Fold the imports of Java and Kotlin files when a file opens",
                        ide.editor.fold_imports,
                    ),
                    // SPEC-0115 v5 behaviour 13.
                    (
                        "Show configuration values in place of ${…} placeholders",
                        ide.editor.config_values,
                    ),
                    (arrows, true),
                ];
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(c(p.ui_muted)).pb_1().child("On Save"))
                    .children(checks.into_iter().enumerate().map(|(i, (label, on))| {
                        let title = match i {
                            2 => Some("Code Insight"),
                            8 => Some("Code Folding"),
                            _ => None,
                        };
                        let heading = title.map(|t| {
                            div()
                                .text_color(c(p.ui_muted))
                                .pt_2()
                                .pb_1()
                                .child(t)
                        });
                        // The last row is the choice of the folding arrows (SPEC-0083 v8
                        // behaviour 22); every other row is a check box (v11 behaviour 17).
                        let mark = if i == 11 {
                            options::Mark::Choice
                        } else {
                            options::Mark::Check(on)
                        };
                        div().children(heading).child(self.option_row(
                            ("settings-editor", i),
                            format!("settings-editor-{i}"),
                            mark,
                            label,
                            cx,
                        ))
                    }))
                    // The disabled intentions (SPEC-0076 v11 behaviour 15).
                    .when(!ide.intentions.disabled.is_empty(), |d| {
                        d.child(
                            div()
                                .text_color(c(p.ui_muted))
                                .pt_2()
                                .pb_1()
                                .child("Disabled Intentions"),
                        )
                        .children(ide.intentions.disabled.iter().enumerate().map(|(k, title)| {
                            let i = 12 + k;
                            row(("settings-editor", i), i == selected)
                                .child("☐")
                                .child(title.clone())
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = i;
                                    this.adjust(0.0, cx);
                                }))
                        }))
                    })
                    .child(
                        div()
                            .pt_2()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child("Enter or Space switches the selected option; on a disabled intention it turns it on again"),
                    )
                    .into_any_element()
            }
            Page::Font => {
                // Each control in the semantic tree (`SPEC-0129` v6 behaviour 9); the family from a
                // list, size and line spacing in fields, a preview of one's own text (v7
                // behaviour 10).
                let step = |more: bool, i: usize, label: &str| {
                    Semantics::new(Role::Button)
                        .name(format!(
                            "{} {label}",
                            if more { "Increase" } else { "Decrease" }
                        ))
                        .test_id(format!(
                            "settings-font-{}-{i}",
                            if more { "more" } else { "less" }
                        ))
                        .action(AxAction::Click)
                        .effects(&[Effect::None])
                };
                let family = ide.editor.font_family.clone();
                let families = div()
                    .id("settings-font-families")
                    .flex_1()
                    .max_h(px(96.))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(c(p.separator))
                    .rounded(px(4.))
                    .children(self.fonts.iter().enumerate().map(|(k, name)| {
                        let on = *name == family;
                        div()
                            .id(("settings-font-family", k))
                            .px_2()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(name.clone())
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = 0;
                                this.choose_font(k, cx);
                            }))
                            .semantics(
                                Semantics::new(Role::ListItem)
                                    .name(name.clone())
                                    .test_id(format!("settings-font-family-{k}"))
                                    .selected(on)
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            )
                    }));
                let number_row = |i: usize, label: &'static str, value: String, field: gpui::Entity<crate::editor_view::EditorView>, cx: &mut Context<Self>| {
                    row(("settings-font", i), i == selected)
                        .child(div().w(px(110.)).child(hit_label(label, &query, p)))
                        .child(
                            small_button(("font-less", i), "−")
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = i;
                                    this.adjust(-1.0, cx);
                                }))
                                .semantics(step(false, i, label)),
                        )
                        .child(
                            crate::metrics::field_frame()
                                .w(px(64.))
                                .child(field)
                                .semantics(
                                    Semantics::new(Role::TextInput)
                                        .name(label)
                                        .value(value)
                                        .test_id(format!("settings-font-value-{i}")),
                                ),
                        )
                        .child(
                            small_button(("font-more", i), "+")
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = i;
                                    this.adjust(1.0, cx);
                                }))
                                .semantics(step(true, i, label)),
                        )
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        row(("settings-font", 0), selected == 0)
                            .h(px(104.))
                            .items_start()
                            .child(div().w(px(110.)).pt_1().child(hit_label("Font", &query, p)))
                            .child(families),
                    )
                    .child(number_row(
                        1,
                        "Size",
                        format!("{}", ide.editor.font_size),
                        self.font_fields[0].clone(),
                        cx,
                    ))
                    .child(number_row(
                        2,
                        "Line spacing",
                        format!("{:.1}", ide.editor.line_spacing),
                        self.font_fields[1].clone(),
                        cx,
                    ))
                    // The ligatures checkbox under the font rows (SPEC-0150 v2 behaviour 1a).
                    .child(
                        self.option_row(
                            ("settings-font", 3),
                            "settings-font-ligatures".into(),
                            options::Mark::Check(ide.editor.ligatures),
                            "Enable ligatures",
                            cx,
                        ),
                    )
                    // A preview of one's own text, in the editors' font (v7 behaviour 10).
                    .child(div().pt_1().text_color(c(p.ui_muted)).child("Preview"))
                    .child(
                        div()
                            .h(px(96.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(p.separator))
                            .overflow_hidden()
                            .child(self.font_preview.clone())
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, window, cx| {
                                    // Also below the text: the dialog would take the focus.
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    window.focus(&this.font_preview.read(cx).focus());
                                }),
                            )
                            .semantics(
                                Semantics::new(Role::TextInput)
                                    .name("Preview")
                                    .test_id("settings-font-preview"),
                            ),
                    )
                    .child(div().pt_1().text_xs().text_color(c(p.ui_dim)).child(
                        "Left/Right or −/+ change the selected value, Enter in a field applies it; editors follow at once",
                    ))
                    .into_any_element()
            }
            Page::Inspections => {
                use ferris_idea_languages::inspections::{Level, description};
                let rows = crate::settings_model::inspection_rows(&self.query);
                let level_name = |l: Level| match l {
                    Level::Error => "Error",
                    Level::Warning => "Warning",
                    Level::Weak => "Weak Warning",
                    Level::Info => "No highlighting, only fix",
                };
                let chosen = rows.get(selected).cloned();
                let list = div()
                    .id("settings-inspections")
                    .w(px(340.))
                    .flex_none()
                    .h_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(rows.iter().enumerate().map(|(i, r)| {
                        let (enabled, level) = self.inspection_profile.state(r.id);
                        let changed = self.inspection_profile.is_changed(r.id);
                        row(("settings-inspection", i), selected == i)
                            .when(changed, |d| d.text_color(c(p.link)))
                            .child(
                                div()
                                    .id(("settings-inspection-check", i))
                                    .w(px(14.))
                                    .child(if enabled { "☑" } else { "☐" })
                                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        this.selected = i;
                                        this.change_inspection(false, cx);
                                    })),
                            )
                            .child(div().flex_1().min_w_0().text_ellipsis().child(r.name))
                            .child(div().w(px(64.)).text_xs().text_color(c(p.ui_dim)).child(r.language))
                            .child(
                                div()
                                    .w(px(84.))
                                    .text_xs()
                                    .text_color(c(p.ui_muted))
                                    .child(level_name(level)),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                cx.notify();
                            }))
                    }));
                let details = chosen.map(|r| {
                    let (enabled, level) = self.inspection_profile.state(r.id);
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(r.name))
                        .child(
                            div()
                                .text_xs()
                                .text_color(c(p.ui_dim))
                                .child(match r.short_name {
                                    Some(s) => format!("{} · {s}", r.id),
                                    None => r.id.to_owned(),
                                }),
                        )
                        .children(
                            crate::doc_model::description_blocks(description(r.id).unwrap_or_default())
                                .into_iter()
                                .map(|b| match b {
                                    crate::doc_model::DocBlock::Code(t) => div()
                                        .font_family(crate::fonts::editor_family())
                                        .text_color(c(p.ui_muted))
                                        .children(t.lines().map(|l| div().child(l.to_owned())).collect::<Vec<_>>()),
                                    crate::doc_model::DocBlock::Text(t, _) => div().child(t),
                                    _ => div(),
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .items_center()
                                .child(div().text_color(c(p.ui_muted)).child("Severity:"))
                                .child(
                                    div()
                                        .id("settings-inspection-severity")
                                        .px_2()
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(c(p.separator))
                                        .hover(|s| s.bg(c(p.tree_hover)))
                                        .child(level_name(level))
                                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                            this.change_inspection(true, cx)
                                        })),
                                )
                                .child(div().text_color(c(p.ui_dim)).child(if enabled { "on" } else { "off" }))
                                .child(
                                    div()
                                        .id("settings-inspection-reset")
                                        .text_color(c(p.link))
                                        .cursor_pointer()
                                        .child("Reset")
                                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                            this.reset_inspection(cx)
                                        })),
                                ),
                        )
                });
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(c(p.ui_muted)).child(
                        "Profile: Project Default · stored in project (.idea/inspectionProfiles)",
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .gap_3()
                            .child(list)
                            .children(details),
                    )
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(
                        "Space turns the selected inspection on or off, Enter changes its severity, \
                         Delete resets it; type to filter",
                    ))
                    .when_some(self.note.clone(), |d, note| {
                        d.child(div().text_color(c(p.ui_muted)).child(note))
                    })
                    .into_any_element()
            }
            Page::Grammar => {
                use crate::settings_model::{GrammarRow, grammar_rows};
                let g = ide.editor.grammar.clone();
                let column = self.grammar_column;
                let mark = |on: bool| if on { "☑" } else { "☐" };
                let rows = grammar_rows();
                let list = div()
                    .id("settings-grammar")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(rows.into_iter().enumerate().map(|(i, r)| {
                        let on = selected == i;
                        let cell = |k: usize, label: &'static str, value: bool| {
                            div()
                                .id(("settings-grammar-cell", i * 8 + k))
                                .w(px(if k == 0 { 170. } else { 130. }))
                                .flex()
                                .gap_1()
                                .when(on && column == k, |d| d.text_color(c(p.link)))
                                .child(mark(value))
                                .child(label)
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = i;
                                    this.grammar_column = k;
                                    this.toggle_grammar(cx);
                                }))
                        };
                        let line = row(("settings-grammar", i), on);
                        match r {
                            GrammarRow::CommitMessages => {
                                line.child(cell(0, "Commit messages", g.commit_messages))
                            }
                            GrammarRow::FileType(ty) => {
                                let scope = g.scope(ty);
                                line.child(cell(0, ty, scope.enabled))
                                    .child(cell(1, "Comments", scope.comments))
                                    .child(cell(2, "Documentation", scope.documentation))
                                    .child(cell(3, "String literals", scope.string_literals))
                            }
                            GrammarRow::Rule(id) => {
                                let (enabled, _) = self.inspection_profile.state(id);
                                line.child(cell(
                                    0,
                                    ferris_idea_languages::inspections::name_of(id),
                                    enabled,
                                ))
                            }
                        }
                    }));
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .h_full()
                    .child(div().text_color(c(p.ui_muted)).pb_1().child(
                        "Where grammar and spelling are checked: commit messages, then per file type its comments, documentation and string literals; the writing rules below are those of the project's inspection profile.",
                    ))
                    .child(list)
                    .child(div().pt_1().text_xs().text_color(c(p.ui_dim)).child(
                        "Space or Enter turns the selected switch over; Left and Right choose a column of a file type; open editors follow at once",
                    ))
                    .when_some(self.note.clone(), |d, note| {
                        d.child(div().text_color(c(p.ui_muted)).child(note))
                    })
                    .into_any_element()
            }
            Page::NaturalLanguages => {
                use ferris_idea_languages::dictionaries as d;
                let dir = d::dictionaries_dir();
                let active = d::active_all(dir.as_deref(), &ide.editor.spelling_dictionary);
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(c(p.ui_muted)).pb_1().child(
                        "Grammar in comments, Markdown and text: repeated words and a/an. Typos with English word lists; English (United States) and English (United Kingdom) are built in:",
                    ))
                    .children(d::CATALOG.iter().enumerate().map(|(i, e)| {
                        let built_in = d::is_built_in(e.code);
                        let installed = !built_in
                            && dir.as_deref().is_some_and(|dir| d::is_installed(dir, e.code));
                        let used = active.contains(&e.code);
                        let state = if self.downloading == Some(e.code) {
                            "downloading…".to_owned()
                        } else if used && built_in {
                            "built in, used".to_owned()
                        } else if used {
                            "used".to_owned()
                        } else if installed {
                            "downloaded".to_owned()
                        } else if built_in {
                            "built in".to_owned()
                        } else {
                            format!("{} KB", (e.dic.0 + e.aff.0) / 1024)
                        };
                        row(("settings-dict", i), selected == i)
                            .child(div().w(px(14.)).child(if used { "●" } else { "○" }))
                            .child(div().w(px(200.)).child(e.name))
                            .child(div().w(px(110.)).text_color(c(p.ui_muted)).child(state))
                            .child(div().text_xs().text_color(c(p.ui_dim)).child(e.license))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                this.use_dictionary(cx);
                            }))
                    }))
                    .child(div().pt_1().text_xs().text_color(c(p.ui_dim)).child(format!(
                        "Enter uses only the selected dictionary, downloading it first unless it is built in; \
                         Space adds it to the ones used or takes it away; Delete removes a downloaded one. The \
                         built-in ones come from SCOWL; the others are Hunspell dictionaries of the LibreOffice \
                         project, checked by size and SHA-256 and kept in {}.",
                        dir.map(|d| d.display().to_string()).unwrap_or_else(|| "the settings folder".into()),
                    )))
                    .when_some(self.note.clone(), |d, note| {
                        d.child(div().text_color(c(p.ui_muted)).child(note))
                    })
                    .into_any_element()
            }
            Page::CodeStyle => {
                let cs = ide.code_style.clone();
                let margin = row(("settings-style", 0usize), selected == 0)
                    .child(div().w(px(140.)).child(hit_label("Hard wrap at", &query, p)))
                    .child(format!("{} columns", cs.right_margin))
                    .child(div().text_xs().text_color(c(p.ui_dim)).child("Left/Right"));
                let show = self.option_row(
                    ("settings-style", 1usize),
                    "settings-style-1".into(),
                    options::Mark::Check(cs.show_right_margin),
                    "Show the right margin in the editor",
                    cx,
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(margin)
                    .child(show)
                    .child(div().pt_2().text_color(c(p.ui_muted)).child("Indentation"))
                    .children(
                        ferris_idea_core::CODE_STYLE_LANGUAGES
                            .iter()
                            .enumerate()
                            .map(|(i, lang)| {
                                let st = cs.indent_for(lang);
                                let at = i + 2;
                                row(("settings-style", at), selected == at)
                                    .child(div().w(px(140.)).child(*lang))
                                    .child(format!(
                                        "{} · indent {} · tab {}",
                                        if st.use_tabs { "Tabs" } else { "Spaces" },
                                        st.indent,
                                        st.tab
                                    ))
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, window, cx| {
                                            this.give_page_keys(at, window);
                                            cx.notify();
                                        },
                                    ))
                            }),
                    )
                    .child(div().pt_1().text_xs().text_color(c(p.ui_dim)).child(
                        "Left/Right change the indent, Space or Enter switches tabs and spaces. \
                         An .editorconfig wins over these for its files; Reformat Code and Tab use them.",
                    ))
                    .into_any_element()
            }
            Page::VersionControl => {
                let v = ide.vcs.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        row(("settings-vcs", 0usize), selected == 0)
                            .child(div().w(px(160.)).child(hit_label("Update method", &query, p)))
                            .child(match v.update_method {
                                ferris_idea_core::UpdateMethod::Merge => "Merge",
                                ferris_idea_core::UpdateMethod::Rebase => "Rebase",
                            })
                            .child(div().text_xs().text_color(c(p.ui_dim)).child("Enter switches"))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.click_option(0, window, cx);
                            })),
                    )
                    .child(
                        self.option_row(
                            ("settings-vcs", 1usize),
                            "settings-vcs-1".into(),
                            options::Mark::Check(v.allow_force_push),
                            "Allow force push (with lease)",
                            cx,
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(160.))
                                    .when(selected == 2, |d| d.text_color(c(p.ui_muted)))
                                    .child(hit_label("Protected branches", &query, p)),
                            )
                            .child(
                                crate::metrics::field_frame()
                                    .flex_1()
                                    .child(self.protected.clone())
                                    .semantics(
                                        Semantics::new(Role::TextInput)
                                            .name("Protected branches")
                                            .value(ide.vcs.protected_branches.clone())
                                            .test_id("settings-protected-branches"),
                                    ),
                            ),
                    )
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(
                        "Comma-separated, * matches any text. Protected branches are never force pushed, \
                         from the Push dialog or through MCP. Enter on the field edits it, Enter again saves.",
                    ))
                    .child(
                        self.option_row(
                            ("settings-vcs", 3usize),
                            "settings-vcs-3".into(),
                            options::Mark::Check(ide.git.ssh.allow_classic_kex_for_azure_devops),
                            "Allow a classical SSH key exchange for Azure DevOps",
                            cx,
                        ),
                    )
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(
                        "Git over SSH needs a post-quantum key exchange. Azure DevOps offers none yet; \
                         with this on, Git to ssh.dev.azure.com and vs-ssh.visualstudio.com also accepts \
                         its strongest classical one and warns. HTTPS with a token avoids it.",
                    ))
                    .child(
                        self.option_row(
                            ("settings-vcs", 4usize),
                            "settings-vcs-4".into(),
                            options::Mark::Check(ide.ssh.allow_own_proxy_command),
                            "Allow a ProxyCommand from the SSH configuration",
                            cx,
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(c(if ide.ssh.allow_own_proxy_command {
                                p.error_text
                            } else {
                                p.ui_dim
                            }))
                            .child(if ide.ssh.allow_own_proxy_command {
                                "Warning: a ProxyCommand runs a program Ferris IDEA cannot hold to the \
                                 post-quantum key exchange; its hops may be classical. Each use is \
                                 logged in ssh-proxy-command.log. ProxyJump needs no exception."
                            } else {
                                "Off: hosts whose SSH configuration runs its own ProxyCommand are \
                                 refused. ProxyJump works and every jump host gets the post-quantum \
                                 key exchange (ADR-0025)."
                            }),
                    )
                    .when_some(self.note.clone(), |d, note| {
                        d.child(div().text_color(c(p.ui_muted)).child(note))
                    })
                    .into_any_element()
            }
            Page::BuildTools | Page::Languages => info_list(&self.info).into_any_element(),
            Page::SystemSettings => self.system_page(cx),
            Page::Notifications => self.notifications_page(cx),
            Page::Advanced => self.advanced_page(cx),
            // SPEC-0278 behaviour 8.
            Page::TrustedLocations => div()
                .flex()
                .flex_col()
                .w_full()
                .gap_1()
                .child(div().text_color(c(p.ui_muted)).child(
                    "Projects in these locations may run their code: build scripts, build-tool sync, run configurations, tests and tasks.",
                ))
                .child(
                    div()
                        .text_color(c(p.ui_dim))
                        .text_xs()
                        .child("Delete removes the selected location; its projects open in safe mode"),
                )
                .when(self.trusted.is_empty(), |d| {
                    d.child(
                        div()
                            .py_2()
                            .text_color(c(p.ui_dim))
                            .child("No trusted locations."),
                    )
                })
                .children(self.trusted.iter().enumerate().map(|(i, l)| {
                    let kind = match l.kind {
                        ferris_idea_core::trust::TrustKind::Project => "project",
                        ferris_idea_core::trust::TrustKind::Folder => "folder and its projects",
                    };
                    row(("trusted-row", i), i == selected)
                        // One line, cut with an ellipsis: a wrapped long path overlapped the
                        // rows (hand test 197).
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(l.path.display().to_string()),
                        )
                        .child(div().text_xs().text_color(c(p.ui_muted)).child(kind))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.selected = i;
                            cx.notify();
                        }))
                }))
                .child(div().h(px(8.)))
                .child(info_list(&self.info))
                .when_some(self.note.clone(), |d, note| {
                    d.child(div().text_color(c(p.ui_muted)).child(note))
                })
                .into_any_element(),
            Page::Ai => {
                let has_key = self.ai_key_stored;
                let labels = ["Endpoint", "Model", "API key"];
                let subscription = ferris_idea_ai::subscription::Cli::from_id(&ide.ai.subscription);
                let provider = match subscription {
                    Some(cli) => cli.name(),
                    None => {
                        ferris_idea_ai::provider_of(&self.ai_fields[0].read(cx).state.buffer.text())
                            .unwrap_or("Custom")
                    }
                };
                let claude_in_terminal = ferris_idea_ai::claude_in_terminal(&ide.ai);
                let login = match (subscription, self.ai_login) {
                    (Some(_), Some(l)) if claude_in_terminal && l != ferris_idea_ai::subscription::Login::NotInstalled => {
                        "Claude Code signs in by itself — Enter opens it in a terminal".to_owned()
                    }
                    (None, _) => "for a subscription".to_owned(),
                    (Some(_), None) => "checking…".to_owned(),
                    (Some(_), Some(ferris_idea_ai::subscription::Login::SignedIn)) => {
                        "signed in".to_owned()
                    }
                    (Some(_), Some(ferris_idea_ai::subscription::Login::SignedOut)) => {
                        "not signed in — Enter opens the sign-in in a terminal".to_owned()
                    }
                    (Some(_), Some(ferris_idea_ai::subscription::Login::Unknown)) => {
                        "shows at the first question".to_owned()
                    }
                    (Some(cli), Some(ferris_idea_ai::subscription::Login::NotInstalled)) => {
                        format!("not installed — {}", cli.install_hint())
                    }
                };
                div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            self.option_row(
                                ("settings-ai", 0usize),
                                "settings-ai-0".into(),
                                options::Mark::Check(ide.ai.enabled),
                                "Enable AI Assistant",
                                cx,
                            )
                        )
                        .child(
                            row(("settings-ai", 1usize), selected == 1)
                                .relative()
                                .semantics(crate::semantics::Semantics::new(crate::semantics::Role::ListItem).name("Provider").value(provider).test_id("settings-ai-1").action(crate::semantics::AxAction::Click).effects(&[crate::semantics::Effect::None]))
                                .child(div().w(px(80.)).text_color(c(p.ui_muted)).child("Provider"))
                                .child(provider)
                                .child(div().text_xs().text_color(c(p.ui_dim)).child("Enter cycles"))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = 1;
                                    this.cycle_provider(cx);
                                })),
                        )
                        .child(
                            self.option_row(
                                ("settings-ai", 2usize),
                                "settings-ai-2".into(),
                                options::Mark::Check(ide.ai.inline_completion),
                                "Inline completion",
                                cx,
                            )
                                .child(div().text_xs().text_color(c(p.ui_dim)).child(
                                    "suggests code at the caret after a pause; Tab accepts",
                                ))
                        )
                        .child(
                            row(("settings-ai", 3usize), selected == 3)
                                .relative()
                                .semantics(crate::semantics::Semantics::new(crate::semantics::Role::ListItem).name("Sign in").test_id("settings-ai-3").action(crate::semantics::AxAction::Click).effects(&[crate::semantics::Effect::None]))
                                .child(div().w(px(80.)).text_color(c(p.ui_muted)).child("Sign in"))
                                .child(div().text_color(c(p.ui_muted)).child(login))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = 3;
                                    this.sign_in(cx);
                                })),
                        )
                        .child(
                            self.option_row(
                                ("settings-ai", 4usize),
                                "settings-ai-4".into(),
                                options::Mark::Check(ide.mcp.http),
                                "Serve MCP to local agents over HTTP",
                                cx,
                            )
                                .child(div().text_xs().text_color(c(p.ui_dim)).child(
                                    "127.0.0.1 only, with a token per start; address and token in mcp-http/ of the settings folder",
                                ))
                        )
                        .child(
                            row(("settings-ai", 5usize), selected == 5)
                                .relative()
                                .semantics(crate::semantics::Semantics::new(crate::semantics::Role::ListItem).name("Terminal lines to attach").test_id("settings-ai-5").action(crate::semantics::AxAction::Click).effects(&[crate::semantics::Effect::None]))
                                .child(div().text_color(c(p.ui_muted)).child("Terminal lines to attach"))
                                .child(format!(
                                    "{}",
                                    ferris_idea_ai::context::terminal_lines(ide.ai.terminal_lines)
                                ))
                                .child(div().text_xs().text_color(c(p.ui_dim)).child(
                                    "←/→ by 50; terminals are read only when you attach or share a tab",
                                ))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = 5;
                                    cx.notify();
                                })),
                        )
                        .child(
                            self.option_row(
                                ("settings-ai", 6usize),
                                "settings-ai-6".into(),
                                options::Mark::Check(ide.ui.driver.external),
                                "Allow agents and scripts to operate the window",
                                cx,
                            )
                                .child(div().text_xs().text_color(c(p.ui_dim)).child(
                                    "with the right \"ui\" per project; marked while it happens; Esc or Stop ends it and switches this off",
                                ))
                        )
                        .child(
                            // SPEC-0160 v2 behaviour 6: how Claude with the plan is used.
                            row(("settings-ai", 7usize), selected == 7)
                                .relative()
                                .semantics(crate::semantics::Semantics::new(crate::semantics::Role::ListItem).name("Claude plan").value(match ide.ai.claude_plan_mode { ferris_idea_core::ClaudePlanMode::Chat => "Chat", ferris_idea_core::ClaudePlanMode::Terminal => "Terminal" }).test_id("settings-ai-7").action(crate::semantics::AxAction::Click).effects(&[crate::semantics::Effect::None]))
                                .child(div().w(px(80.)).text_color(c(p.ui_muted)).child("Claude plan"))
                                .child(match ide.ai.claude_plan_mode {
                                    ferris_idea_core::ClaudePlanMode::Chat => "Chat",
                                    ferris_idea_core::ClaudePlanMode::Terminal => "Terminal",
                                })
                                .child(div().text_xs().text_color(c(p.ui_dim)).child("Enter switches"))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.selected = 7;
                                    this.toggle_claude_plan_mode(cx);
                                })),
                        )
                        .child(div().text_xs().text_color(c(p.ui_dim)).child(
                            ferris_idea_ai::subscription::CLAUDE_PLAN_MODE_NOTE,
                        ))
                        .children(self.ai_fields.iter().zip(labels).map(|(f, label)| {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(80.)).text_color(c(p.ui_muted)).child(label))
                                .child(
                                    crate::metrics::field_frame()
                                        .flex_1()
                                        .relative()
                                        .semantics(crate::semantics::Semantics::new(crate::semantics::Role::TextInput).name(label).test_id(format!("settings-ai-field-{label}")).action(crate::semantics::AxAction::Click).effects(&[crate::semantics::Effect::None]))
                                        .child(f.clone()),
                                )
                        }))
                        .child(div().text_xs().text_color(c(p.ui_dim)).child(match subscription {
                            Some(_) if claude_in_terminal => format!(
                                "Claude's plan runs in the Terminal as Claude Code (Sign in opens it). AI Chat uses \
                                 Claude only through the Anthropic API template with your own API key: {}; \
                                 without it nothing is sent.",
                                match has_key {
                                    Some(true) => "stored in the keychain; type to replace",
                                    Some(false) => "none yet",
                                    None => "checking the keychain…",
                                }
                            ),
                            Some(cli) => format!(
                                "With a subscription Ferris IDEA runs the provider's own program (`{}`) as a plain chat: \
                                 no tools, in an empty folder, nothing kept. It uses your plan's limits; Ferris IDEA never \
                                 sees its sign-in. Model: empty for the program's default. Endpoint and API key are \
                                 not used. Questions and attached code are sent only when the assistant is on.",
                                cli.program()
                            ),
                            None => format!(
                                "Any OpenAI-compatible API: a provider template, a local server or an endpoint \
                                 of your choice. API use is billed by the provider, separately from a chat \
                                 subscription. Questions and the \
                                 code you attach are sent to this endpoint only when the assistant is on. \
                                 API key: {}. Enter applies.",
                                match has_key {
                                    Some(true) => "stored in the keychain; type to replace",
                                    Some(false) => "none (not needed for local servers)",
                                    None => "checking the keychain… (if the system asks for keychain access, allow it)",
                                }
                            ),
                        }))
                        .when_some(self.note.clone(), |d, note| {
                            d.child(div().text_color(c(p.ui_muted)).child(note))
                        })
                        .into_any_element()
            }
            Page::Terminal => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(c(p.ui_muted)).child(hit_label("Shell path", &query, p)))
                .child(crate::metrics::field_frame().child(self.shell.clone()))
                .child(div().text_xs().text_color(c(p.ui_dim)).child(format!(
                    "Empty: $SHELL ({}). Enter applies; new terminals use it.",
                    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
                )))
                .when_some(self.note.clone(), |d, note| {
                    d.child(div().text_color(c(p.ui_muted)).child(note))
                })
                .into_any_element(),
            Page::Appearance => {
                let now = current_theme();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(c(p.ui_muted)).pb_1().child(hit_label("Theme", &query, p)))
                    .children(ThemeId::ALL.iter().enumerate().map(|(i, t)| {
                        let t = *t;
                        div()
                            .id(("settings-theme", i))
                            .h(px(26.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(4.))
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(div().w(px(14.)).child(if t == now { "●" } else { "○" }))
                            .child(t.name())
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                cx.emit(SettingsEvent::Theme(t));
                            }))
                    }))
                    .child(self.scheme_row(ThemeId::ALL.len(), cx))
                    .child({
                        let i = ThemeId::ALL.len() + 1;
                        let a = &self.ide.appearance;
                        self.option_row(
                            ("settings-theme", i),
                            format!("settings-theme-{i}"),
                            options::Mark::Check(a.sync_with_os),
                            "Sync with OS",
                            cx,
                        )
                        .mt_2()
                            .child(div().text_color(c(p.ui_dim)).child(format!(
                                "dark: {} · light: {}",
                                a.dark_theme, a.light_theme
                            )))
                    })
                    .child({
                        let i = ThemeId::ALL.len() + 2;
                        let on = self.ide.appearance.compact_mode;
                        self.option_row(
                            ("settings-theme", i),
                            format!("settings-theme-{i}"),
                            options::Mark::Check(on),
                            "Compact mode",
                            cx,
                        )
                            .child(div().text_color(c(p.ui_dim)).child(
                                "lower rows and toolbar, as the theme's compact values set them",
                            ))
                    })
                    .child({
                        let i = ThemeId::ALL.len() + 3;
                        let on = self.ide.appearance.startup_animation;
                        self.option_row(
                            ("settings-theme", i),
                            format!("settings-theme-{i}"),
                            options::Mark::Check(on),
                            "Startup animation",
                            cx,
                        )
                            .child(div().text_color(c(p.ui_dim)).child(
                                "while Ferris IDEA starts; Reduce Motion shows the logo still",
                            ))
                    })
                    .child(
                        div().pt_1().text_xs().text_color(c(p.ui_dim)).child(
                            "With Sync with OS, choosing a theme makes it the one for its kind",
                        ),
                    )
                    .into_any_element()
            }
            Page::Keymap => {
                let rows = self.rows.clone();
                let count = rows.len();
                let recording = self.recording;
                let list = uniform_list(
                    "settings-keymap",
                    count,
                    cx.processor(move |_this, range: Range<usize>, _window, cx| {
                        range
                            .map(|i| {
                                let r = &rows[i];
                                div()
                                    .id(("keymap-row", i))
                                    .h(px(24.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded(px(4.))
                                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                                    .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(r.title.clone()),
                                    )
                                    .child(div().text_color(c(p.ui_muted)).child(
                                        if recording && i == selected {
                                            "Press a key…".to_owned()
                                        } else {
                                            r.shortcuts.clone()
                                        },
                                    ))
                                    // A chord macOS takes itself (SPEC-0105 v12 behaviour 18).
                                    .when_some(r.system.clone(), |d, note| {
                                        d.child(
                                            div()
                                                .text_xs()
                                                .text_color(c(p.ui_dim))
                                                .whitespace_nowrap()
                                                .child(note),
                                        )
                                    })
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, window, cx| {
                                            this.selected = i;
                                            this.row_chosen = true;
                                            // The list takes the keys (v10 behaviour 16).
                                            this.list_keys = true;
                                            this.list_has_keys = false;
                                            window.focus(&this.focus);
                                            cx.notify();
                                        },
                                    ))
                                    .relative()
                                    .semantics(
                                        Semantics::new(Role::ListItem)
                                            .name(r.title.clone())
                                            .value(r.shortcuts.clone())
                                            .test_id(format!("settings-keymap-row-{i}"))
                                            .selected(i == selected)
                                            .action(AxAction::Click)
                                            .effects(&[Effect::None]),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1();
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .gap_1()
                    .child(
                        div().text_color(c(p.ui_dim)).text_xs().child(
                            "Enter adds a shortcut · Delete removes · Shift+Delete restores",
                        ),
                    )
                    .child(list)
                    .when_some(self.note.clone(), |d, note| {
                        d.child(div().text_color(c(p.ui_muted)).child(note))
                    })
                    .into_any_element()
            }
            Page::Plugins => {
                let folder = self
                    .plugins_dir
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "no settings folder".into());
                // At least the area's height, so the folder stands at its foot; taller, the page
                // rolls (`SPEC-0105` v8 behaviour 12).
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_h_full()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().text_color(c(p.ui_muted)).child("Installed"))
                            .child(
                                crate::metrics::button(
                                    "install-plugin",
                                    "Install Plugin from Disk…",
                                    false,
                                )
                                .relative()
                                // Opens the file dialog; installing waits for the review.
                                .semantics(
                                    crate::semantics::Semantics::new(crate::semantics::Role::Button)
                                        .name("Install Plugin from Disk…")
                                        .test_id("settings-plugins-install-from-disk")
                                        .action(crate::semantics::AxAction::Click)
                                        .effects(&[crate::semantics::Effect::None]),
                                )
                                .on_click(cx.listener(
                                    |this, _: &gpui::ClickEvent, _, cx| this.choose_plugin_file(cx),
                                )),
                            ),
                    )
                    .child(
                        div().text_color(c(p.ui_dim)).text_xs().child(
                            ferris_idea_ui::keymap::local_keys(
                                "Enter enables or disables · Space grants or withdraws its permissions · ⇧Delete revokes network grants · Delete twice removes · ⌘O installs",
                                cfg!(target_os = "macos"),
                            ),
                        ),
                    )
                    .when_some(self.plugin_review.clone(), |d, (_, review)| {
                        d.child(self.plugin_review_box(&review, cx))
                    })
                    .when(self.plugins.is_empty(), |d| {
                        d.child(
                            div()
                                .py_2()
                                .text_color(c(p.ui_dim))
                                .child("No plugins installed."),
                        )
                    })
                    .children(self.plugins.iter().enumerate().map(|(i, pl)| {
                        let (title, detail) = match &pl.manifest {
                            Some(m) => (
                                format!("{} {}", m.name, m.version),
                                format!(
                                    "{} · {}{}{}",
                                    if m.description.is_empty() {
                                        pl.file.as_str()
                                    } else {
                                        m.description.as_str()
                                    },
                                    // What it contributes (SPEC-0211 behaviour 1).
                                    ferris_idea_plugins::contributions(m).join(" · "),
                                    // The access it asks for and whether it has it (SPEC-0156,
                                    // SPEC-0276).
                                    m.permissions
                                        .iter()
                                        .map(|perm| {
                                            let on = self.plugins_dir.as_deref().is_some_and(|d| {
                                                ferris_idea_plugins::granted(d, &pl.file, *perm)
                                            });
                                            format!(
                                                " · {} ({})",
                                                perm.describe(),
                                                if on { "granted" } else { "not granted" }
                                            )
                                        })
                                        .collect::<String>(),
                                    if pl.enabled { "" } else { " · disabled" }
                                ),
                            ),
                            None => (pl.file.clone(), pl.problem.clone().unwrap_or_default()),
                        };
                        // Hosts it may reach, for whom (SPEC-0314 behaviour 7).
                        let network = self.plugin_network_lines(&pl.file);
                        let mark = match (&pl.problem, pl.enabled) {
                            (Some(_), _) => "⚠",
                            (None, true) => "☑",
                            (None, false) => "☐",
                        };
                        div()
                            .id(("plugin-row", i))
                            .px_2()
                            .py_1()
                            .flex()
                            .gap_2()
                            .rounded(px(4.))
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(div().w(px(14.)).child(mark))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .child(title)
                                    .child(div().text_xs().text_color(c(p.ui_muted)).child(detail))
                                    .children(network.into_iter().map(|line| {
                                        div().text_xs().text_color(c(p.ui_muted)).child(line)
                                    })),
                            )
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                if e.click_count() == 2 {
                                    this.toggle_plugin(cx);
                                }
                                cx.notify();
                            }))
                    }))
                    .child(div().flex_1())
                    .when_some(self.note.clone(), |d, note| {
                        d.child(div().text_color(c(p.ui_muted)).child(note))
                    })
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(folder))
                    .into_any_element()
            }
        };
        // The page list rolls to the page shown (`SPEC-0105` v8 behaviour 11). Before the list's
        // first layout its height is not known yet: the next frame rolls it.
        if self.reveal_page {
            if let Some(row) = shown_row {
                self.page_list_scroll.scroll_to_item(row);
            }
            if f32::from(self.page_list_scroll.bounds().size.height) > 0. {
                self.reveal_page = false;
            } else {
                cx.spawn(async move |this, cx| {
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        }
        // The dialog's size from the window (behaviour 12).
        let viewport = window.viewport_size();
        let (width, height) = crate::settings_model::dialog_size(
            (f32::from(viewport.width), f32::from(viewport.height)),
            dialog_top(),
        );
        let thumb = c(p.ui_dim).opacity(0.6);
        // A page taller than its area rolls as a whole; Keymap and Inspections roll their lists.
        let body = if crate::settings_model::page_rolls_itself(page) {
            div()
                .flex_1()
                .min_h_0()
                .p_3()
                .child(content)
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .id("settings-page-area")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.page_scroll)
                        .p_3()
                        .child(content),
                )
                .child(crate::scroll_bar::vertical(
                    "settings-page-scrollbar",
                    "Page",
                    &self.page_scroll,
                    thumb,
                ))
                .into_any_element()
        };
        let input = cx.entity();
        let input_focus = self.focus.clone();
        div()
            .id("settings-dialog")
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &input_focus,
                            ElementInputHandler::new(bounds, input.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_field_key))
            .on_key_down(cx.listener(Self::on_key))
            .w(px(width))
            .h(px(height))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .child(
                div()
                    .h(px(36.))
                    .px_3()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .child(div().flex_1().child("Settings")),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(200.))
                            .flex_none()
                            .pt_2()
                            .border_r_1()
                            .border_color(c(p.separator))
                            .flex()
                            .flex_col()
                            // The search field above the page list (`SPEC-0105` v7 behaviour 7).
                            .child(
                                div()
                                    .id("settings-search")
                                    .flex_none()
                                    .mx_2()
                                    .mb_2()
                                    .px_2()
                                    .h(px(26.))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(c(if self.query.is_empty() || self.list_keys {
                                        p.separator
                                    } else {
                                        p.accent
                                    }))
                                    .child(div().text_color(c(p.ui_dim)).child("⌕"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .when(self.query.is_empty(), |d| {
                                                d.text_color(c(p.ui_dim)).child("Search settings")
                                            })
                                            // The text marked by Select All in the editor's
                                            // selection colour (v8 behaviour 10).
                                            .when(!self.query.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .when(self.query_marked, |d| {
                                                            d.bg(gpui::rgb(p.selection))
                                                        })
                                                        .child(self.query.clone()),
                                                )
                                            }),
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &gpui::ClickEvent, window, cx| {
                                            this.query_marked = false;
                                            this.list_keys = false;
                                            window.focus(&this.focus);
                                            cx.notify();
                                        },
                                    ))
                                    .semantics(
                                        Semantics::new(Role::SearchInput)
                                            .name("Search settings")
                                            .value(self.query.clone())
                                            .selected(self.query_marked)
                                            .test_id("settings-search")
                                            .action(AxAction::Click)
                                            .effects(&[Effect::None]),
                                    ),
                            )
                            // The list rolls below the search, with a scroll bar (v8
                            // behaviour 11).
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .child(
                                        div()
                                            .id("settings-pages")
                                            .flex_1()
                                            .min_w_0()
                                            .h_full()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.page_list_scroll)
                                            .pb_2()
                                            .children(page_list),
                                    )
                                    .child(crate::scroll_bar::vertical(
                                        "settings-pages-scrollbar",
                                        "Pages",
                                        &self.page_list_scroll,
                                        thumb,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            // A click on the page gives the keys back to it (v8 behaviour 11).
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                                    if std::mem::take(&mut this.list_has_keys) {
                                        cx.notify();
                                    }
                                }),
                            )
                            .child(self.page_header(cx))
                            .child(body),
                    ),
            )
    }
}

/// How far the dialog's top stands below the window's: 40 points below the toolbar (`SPEC-0105`
/// v8 behaviour 12).
pub(crate) fn dialog_top() -> f32 {
    Palette::current().metrics.toolbar_height + 40.0
}

/// The example text the Font page's preview starts with; it can be edited.
const FONT_PREVIEW: &str = "fn main() -> Result<(), Error> {\n    println!(\"Hello, Ferris IDEA\"); // 0O 1lI != <= ->\n    Ok(())\n}\n";

/// A setting's label on its page; while the search text matches it, it is marked with a frame
/// and is a node of the semantic tree, test id `settings-hit` (`SPEC-0105` v7 behaviour 7).
pub(crate) fn hit_label(text: &'static str, query: &str, p: &Palette) -> gpui::AnyElement {
    if crate::settings_model::label_matches(text, query) {
        div()
            .px_1()
            .rounded(px(3.))
            .border_1()
            .border_color(c(p.accent))
            .child(text)
            .semantics(
                Semantics::new(Role::Label)
                    .name(text)
                    .test_id("settings-hit"),
            )
            .into_any_element()
    } else {
        div().child(text).into_any_element()
    }
}

/// The note after a model list arrived: its first ids, or why there is none.
fn models_note(note: &str, models: Result<Vec<String>, ferris_idea_ai::AiError>) -> String {
    match models {
        Ok(ids) if ids.is_empty() => format!("{note}; the endpoint lists no models"),
        Ok(ids) => {
            let shown: Vec<&str> = ids.iter().take(12).map(String::as_str).collect();
            let more = ids.len().saturating_sub(shown.len());
            let tail = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            format!("{note}; models: {}{tail}", shown.join(", "))
        }
        Err(e) => format!("{note}; no model list: {e}"),
    }
}

/// Forgets every "always allow" of plugin `file` in the user's settings folder (`SPEC-0276`
/// behaviour 1: the grant withdrawn or the plugin removed).
fn forget_plugin_runs(file: &str) {
    if let Some(f) = ferris_idea_plugins::run_permissions_file() {
        let _ = ferris_idea_plugins::forget_plugin(&f, file);
    }
}

#[cfg(test)]
mod tests {
    use super::{extends_query, list_key, models_note, searchable_text};

    /// SPEC-0105 v3 AK-6: the committed text is what is searched, not the key behind it.
    #[test]
    fn committed_text_goes_into_the_search() {
        assert!(searchable_text("e"));
        assert!(searchable_text("ü"));
        assert!(searchable_text("äö"));
        assert!(!searchable_text(""));
        assert!(!searchable_text("\u{7f}"));
        assert!(!searchable_text("\n"));
    }

    /// D9 and D10 (hand test 30): a space goes on with the search; ⌫ with an empty search removes
    /// (⇧⌫ restores) instead of doing nothing.
    #[test]
    fn search_typing_and_mac_delete() {
        assert!(extends_query("save", " ", false));
        assert!(extends_query("s", "ü", false));
        assert!(
            !extends_query("", " ", false),
            "an empty search leaves Space to the list"
        );
        assert!(!extends_query("save", "\r", false));
        assert!(!extends_query("save", "a", true));
        assert_eq!(list_key("backspace", "", false), "delete");
        assert_eq!(list_key("backspace", "save", false), "backspace");
        assert_eq!(list_key("delete", "save", false), "delete");
        // SPEC-0105 v4: ⇧⌫ on a chosen keymap row restores although a search is typed.
        assert_eq!(list_key("backspace", "save all", true), "delete");
        assert_eq!(list_key("a", "save all", true), "a");
    }

    /// SPEC-0105 v5 AK-8: the labels the search knows are the texts this view draws; a label
    /// changed here without the model fails.
    #[test]
    fn the_searched_labels_are_the_drawn_ones() {
        let source = [
            include_str!("settings_view.rs"),
            include_str!("settings_view/general_pages.rs"),
        ]
        .concat();
        for page in crate::settings_model::Page::ALL {
            for label in page.fixed_labels() {
                assert!(
                    source.contains(&format!("\"{label}\"")),
                    "{page:?}: the view does not draw {label:?}"
                );
            }
        }
    }

    #[test]
    fn model_list_note() {
        let many: Vec<String> = (0..14).map(|i| format!("m{i:02}")).collect();
        assert_eq!(
            models_note("Questions go to x", Ok(vec!["a".into(), "b".into()])),
            "Questions go to x; models: a, b"
        );
        assert!(models_note("n", Ok(many)).ends_with("m11 and 2 more"));
        assert_eq!(
            models_note("n", Ok(Vec::new())),
            "n; the endpoint lists no models"
        );
    }
}
