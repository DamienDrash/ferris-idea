//! The Welcome window (`SPEC-0355`): shown at a start without a project and after the last
//! project window; recent projects with search, New Project, Open and Clone Repository,
//! Remote Development › SSH ([`crate::welcome_ssh_view`], v3), Customize, Plugins, Learn and the
//! gear menu. The content is in [`crate::welcome_model`]; this
//! file draws it and carries out what the user chooses with the code the project windows use.

use std::path::{Path, PathBuf};

use ferris_idea_core::RecentEntry;
use ferris_idea_ui::theme::ThemeId;
use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, FocusHandle, Global, KeyDownEvent,
    Subscription, Window, WindowBounds, WindowHandle, WindowOptions, div, img, prelude::*, px,
    size,
};

use crate::icon_images::icon;
use crate::palette::Palette;
use crate::projects_view::Form;
use crate::settings_model::Page;
use crate::welcome_model::{
    GearItem, LEARN, LearnTarget, MIN_SIZE, OpenRow, ProjectRow, Section, TITLE, WINDOW_SIZE,
    default_location, filter, import_refusal, keymap_name, open_row, project_rows, step,
};
use crate::welcome_ssh_view::{WelcomeSshEvent, WelcomeSshView};
use crate::workbench_view::c;
use crate::workbench_view::project_popup::{CloneThread, spawn_clone};
use crate::{
    ImportPreview, ImportPreviewEvent, ProjectsEvent, ProjectsView, SettingsEvent, SettingsView,
    WorkbenchView,
};

/// The Welcome window of this process, if it is open (behaviour 3).
#[derive(Default)]
struct WelcomeWindow(Option<WindowHandle<WelcomeView>>);

impl Global for WelcomeWindow {}

/// The plugins folder the Welcome window shows and manages instead of the user's (tests).
pub struct WelcomePluginsDir(pub PathBuf);

impl Global for WelcomePluginsDir {}

/// The open Welcome window, if any.
pub fn welcome_window(cx: &App) -> Option<WindowHandle<WelcomeView>> {
    let handle = cx.try_global::<WelcomeWindow>()?.0?;
    cx.windows()
        .contains(&AnyWindowHandle::from(handle))
        .then_some(handle)
}

/// Shows the Welcome window, or brings it to the front with its list read again (behaviours 1
/// to 3).
pub fn show(cx: &mut App) {
    if let Some(handle) = welcome_window(cx) {
        let _ = handle.update(cx, |view, window, cx| {
            view.reload(cx);
            window.activate_window();
        });
        return;
    }
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(TITLE.into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(MIN_SIZE.0), px(MIN_SIZE.1))),
        app_id: Some(ferris_idea_ui::brand::APP_ID.to_owned()),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        crate::x11_wake::until_first_frame(window);
        window.set_window_title(TITLE);
        crate::appearance::watch_os_appearance(window);
        // Closing it while no project window is open quits (behaviour 4).
        window.on_window_should_close(cx, |_, cx| {
            closed_by_user(cx);
            true
        });
        let view = cx.new(WelcomeView::new);
        window.focus(&view.read(cx).focus.clone());
        view
    });
    match opened {
        Ok(handle) => cx.set_global(WelcomeWindow(Some(handle))),
        Err(e) => eprintln!("error: the Welcome window could not open: {e}"),
    }
}

/// Closes the Welcome window if it is open: a project window opened (behaviour 4).
pub fn close(cx: &mut App) {
    if let Some(handle) = welcome_window(cx) {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
    cx.set_global(WelcomeWindow(None));
}

/// The user closed the Welcome window: without a project window Ferris IDEA quits (behaviour 4).
fn closed_by_user(cx: &mut App) {
    cx.set_global(WelcomeWindow(None));
    if crate::project_windows::opened_windows(cx).is_empty() {
        cx.defer(WorkbenchView::quit_decided);
    }
}

/// Points of the window's width a row's path does not get: the left column (210), the content's
/// padding (32), the row's padding (16), the initials (32), two gaps (24) and the Remove button.
const PATH_CHROME: f32 = 434.;
/// Width of a character of the path's small text, rounded up so a shortened path fits.
const PATH_CHAR_WIDTH: f32 = 7.;

/// The Welcome window's view.
pub struct WelcomeView {
    /// Room for a row's path in points, from the window's width (`SPEC-0355` v4 behaviour 24).
    path_width: f32,
    section: Section,
    entries: Vec<RecentEntry>,
    rows: Vec<ProjectRow>,
    query: String,
    /// The selected row among the shown ones.
    selected: usize,
    /// The line under the list: why something did not happen, or a clone's progress.
    note: Option<String>,
    /// The form of New Project or Clone Repository (behaviour 7).
    form: Option<Entity<ProjectsView>>,
    /// The running clone.
    clone: Option<(CloneThread, String)>,
    /// The Settings dialog over the window (behaviour 17).
    settings: Option<Entity<SettingsView>>,
    /// The import preview over the window (behaviour 17).
    import: Option<Entity<ImportPreview>>,
    /// The gear menu, open with its selected entry (behaviour 21).
    gear: Option<usize>,
    plugins_dir: Option<PathBuf>,
    plugins: Vec<ferris_idea_plugins::Installed>,
    /// Remote Development › SSH (behaviour 12, v3).
    ssh: Entity<WelcomeSshView>,
    /// The SSH section gave the keys back to the list: take the focus at the next frame.
    refocus: bool,
    focus: FocusHandle,
    _ssh_events: Subscription,
    _form_events: Option<Subscription>,
    _settings_events: Option<Subscription>,
    _import_events: Option<Subscription>,
}

impl WelcomeView {
    /// The view with the recent projects and plugins read now.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let plugins_dir = match cx.try_global::<WelcomePluginsDir>() {
            Some(d) => Some(d.0.clone()),
            None => ferris_idea_plugins::plugins_dir(),
        };
        let ssh = cx.new(WelcomeSshView::new);
        let ssh_events = cx.subscribe(&ssh, |this: &mut Self, _, event, cx| {
            match event {
                // A remote project window opened (behaviour 4).
                WelcomeSshEvent::Opened => cx.defer(close),
                WelcomeSshEvent::FocusList => this.refocus = true,
            }
            cx.notify();
        });
        let mut view = Self {
            path_width: 800. - PATH_CHROME,
            section: Section::Projects,
            entries: Vec::new(),
            rows: Vec::new(),
            query: String::new(),
            selected: 0,
            note: None,
            form: None,
            clone: None,
            settings: None,
            import: None,
            gear: None,
            plugins_dir,
            plugins: Vec::new(),
            ssh,
            refocus: false,
            focus: cx.focus_handle(),
            _ssh_events: ssh_events,
            _form_events: None,
            _settings_events: None,
            _import_events: None,
        };
        view.reload(cx);
        view
    }

    /// Reads the recent projects and the installed plugins again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.entries = crate::project_windows::recent_file(cx)
            .map(|f| ferris_idea_core::load_recent_entries(&f))
            .unwrap_or_default();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        self.rows = project_rows(&self.entries, home.as_deref());
        self.plugins = self
            .plugins_dir
            .as_deref()
            .map(ferris_idea_plugins::installed)
            .unwrap_or_default();
        self.selected = self.selected.min(self.shown().len().saturating_sub(1));
        if self.section == Section::Ssh {
            self.ssh.update(cx, |s, cx| s.reload(cx));
        }
        cx.notify();
    }

    /// The section Remote Development › SSH (behaviour 12, v3).
    pub fn ssh_section(&self) -> Entity<WelcomeSshView> {
        self.ssh.clone()
    }

    /// The section shown.
    pub fn section(&self) -> Section {
        self.section
    }

    /// Shows `section` (behaviour 5).
    pub fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        self.gear = None;
        if section == Section::Ssh {
            // The configuration is read when the section is first shown, not at start-up.
            self.ssh.update(cx, |s, cx| s.ensure_loaded(cx));
        }
        cx.notify();
    }

    /// Every row of the recent list.
    pub fn rows(&self) -> &[ProjectRow] {
        &self.rows
    }

    /// The indexes of the rows the search leaves (behaviour 10).
    pub fn shown(&self) -> Vec<usize> {
        filter(&self.rows, &self.query)
    }

    /// The rows shown, as text: the name, " (missing)" for a gone folder (tests, agents).
    pub fn shown_names(&self) -> Vec<String> {
        self.shown()
            .into_iter()
            .map(|i| {
                let r = &self.rows[i];
                if r.missing {
                    format!("{} (missing)", r.name)
                } else {
                    r.name.clone()
                }
            })
            .collect()
    }

    /// The selected row among the shown ones.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The search text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The line under the list.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The installed plugins shown under Plugins.
    pub fn plugins(&self) -> &[ferris_idea_plugins::Installed] {
        &self.plugins
    }

    /// The New Project or Clone Repository form, if shown.
    pub fn form(&self) -> Option<Entity<ProjectsView>> {
        self.form.clone()
    }

    /// The Settings dialog, if shown.
    pub fn settings_dialog(&self) -> Option<Entity<SettingsView>> {
        self.settings.clone()
    }

    /// The import preview, if shown.
    pub fn import_preview(&self) -> Option<Entity<ImportPreview>> {
        self.import.clone()
    }

    /// Whether the gear menu is open.
    pub fn gear_open(&self) -> bool {
        self.gear.is_some()
    }

    /// Sets the search text (behaviour 10).
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_owned();
        self.selected = 0;
        cx.notify();
    }

    /// Opens the shown row `shown` (behaviours 8 and 9).
    pub fn open_shown(&mut self, shown: usize, cx: &mut Context<Self>) {
        let Some(&i) = self.shown().get(shown) else {
            return;
        };
        self.selected = shown;
        match open_row(&self.rows[i]) {
            OpenRow::Open(path) => self.open_path(&path, cx),
            OpenRow::Missing(why) => self.note = Some(why),
        }
        cx.notify();
    }

    /// Opens `path` in a project window, or brings its window to the front; the Welcome window
    /// then closes (behaviour 4).
    pub fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        match crate::project_windows::open_project(path, cx) {
            Ok(_) => {
                self.note = None;
                cx.defer(close);
            }
            Err(e) => self.note = Some(format!("Cannot open {}: {e}", path.display())),
        }
        cx.notify();
    }

    /// Takes the shown row `shown` out of the recent list; the folder is not touched
    /// (behaviour 9).
    pub fn remove_shown(&mut self, shown: usize, cx: &mut Context<Self>) {
        let Some(&i) = self.shown().get(shown) else {
            return;
        };
        let path = self.rows[i].path.clone();
        let name = self.rows[i].name.clone();
        let removed = crate::project_windows::recent_file(cx)
            .map(|f| ferris_idea_core::forget_project(&f, &path))
            .unwrap_or(Ok(false));
        self.note = Some(match removed {
            Ok(_) => format!("Removed {name} from the list; the folder was not touched."),
            Err(e) => format!("The list could not be written: {e}"),
        });
        self.reload(cx);
    }

    /// Shows the system's folder chooser and opens the chosen folder (behaviour 7).
    pub fn browse(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| this.open_path(&path, cx));
            }
        })
        .detach();
    }

    /// Shows the form of New Project or Clone Repository in the content area (behaviour 7).
    pub fn show_form(&mut self, form: Form, window: &mut Window, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let location = default_location(&self.entries, home.as_deref());
        let cloning = self.clone.as_ref().map(|(_, text)| text.clone());
        let view = cx.new(|cx| {
            let mut v = ProjectsView::form_only(form, location, window, cx);
            v.set_cloning(cloning, None, cx);
            v
        });
        self._form_events = Some(cx.subscribe_in(&view, window, Self::on_form));
        self.section = Section::Projects;
        self.form = Some(view);
        cx.notify();
    }

    fn on_form(
        &mut self,
        view: &Entity<ProjectsView>,
        event: &ProjectsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ProjectsEvent::Clone(source, target) => {
                self.start_clone(source.clone(), target.clone(), cx)
            }
            ProjectsEvent::CancelClone => {
                if let Some((job, _)) = &self.clone {
                    job.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            ProjectsEvent::NewProject(target, git) => {
                match ferris_idea_vcs::new_project(target, *git) {
                    Ok(()) => {
                        self.form = None;
                        // Made by Ferris IDEA a moment ago: no trust question (window check A16, NP2).
                        if let Err(e) = crate::project_popup_model::trust_new_project(
                            crate::project_popup_model::new_project_trust_file().as_deref(),
                            target,
                        ) {
                            self.note = Some(format!("Not trusted: {e}"));
                        }
                        self.open_path(target, cx);
                    }
                    Err(e) => view.update(cx, |v, cx| v.set_cloning(None, Some(e.to_string()), cx)),
                }
            }
            ProjectsEvent::Close => {
                self.form = None;
                window.focus(&self.focus);
            }
            ProjectsEvent::Browse | ProjectsEvent::Open(_) | ProjectsEvent::RemoteDevelopment => {}
        }
        cx.notify();
    }

    /// Clones on a thread of its own with the rules of `SPEC-0094` behaviour 13; the form and the
    /// line under the list show the progress; the new folder opens in a project window.
    fn start_clone(
        &mut self,
        source: ferris_idea_vcs::CloneSource,
        target: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.clone.is_some() {
            return;
        }
        let name = source.folder_name.clone();
        let first = crate::project_popup_model::progress_text(&name, None);
        self.clone = Some((spawn_clone(source, target), first.clone()));
        self.note = Some(first.clone());
        if let Some(v) = &self.form {
            v.update(cx, |v, cx| v.set_cloning(Some(first), None, cx));
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let more = this.update(cx, |this, cx| this.poll_clone(&name, cx));
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Shows the clone's progress; whether it still runs.
    fn poll_clone(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let Some((job, text)) = &mut self.clone else {
            return false;
        };
        let (step, finished) = job.poll();
        match finished {
            None => {
                *text = crate::project_popup_model::progress_text(name, step.as_ref());
                let shown = text.clone();
                self.note = Some(shown.clone());
                if let Some(v) = &self.form {
                    v.update(cx, |v, cx| v.set_cloning(Some(shown), None, cx));
                }
                cx.notify();
                true
            }
            Some(Ok(folder)) => {
                self.clone = None;
                self.form = None;
                self.open_path(&folder, cx);
                false
            }
            Some(Err(e)) => {
                self.clone = None;
                self.note = e.lines().next().map(str::to_owned);
                if let Some(v) = &self.form {
                    v.update(cx, |v, cx| v.set_cloning(None, Some(e.clone()), cx));
                }
                cx.notify();
                false
            }
        }
    }

    /// Opens the Settings dialog on `page` over the window (behaviours 16 to 18).
    pub fn open_settings(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        let extra = ferris_idea_languages::everywhere::UNBOUND_ACTIONS.to_vec();
        let dir = self.plugins_dir.clone();
        let view = cx.new(|cx| SettingsView::with_plugins_dir(page, extra, dir, cx));
        self._settings_events = Some(cx.subscribe_in(&view, window, Self::on_settings));
        window.focus(&view.read(cx).focus());
        self.gear = None;
        self.settings = Some(view);
        cx.notify();
    }

    fn on_settings(
        &mut self,
        view: &Entity<SettingsView>,
        event: &SettingsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SettingsEvent::Theme(theme) => {
                self.choose_theme(*theme, cx);
                view.update(cx, |v, cx| v.reload_ide(cx));
            }
            SettingsEvent::Shortcuts(action, shortcuts) => {
                if let Err(e) = crate::keymaps::set_shortcuts(action, shortcuts.clone(), cx) {
                    self.note = Some(format!("Keymap not saved: {e}"));
                }
                crate::menu_bar::install(cx);
                view.update(cx, |v, cx| {
                    v.refresh();
                    cx.notify();
                });
            }
            SettingsEvent::PluginsChanged => self.reload(cx),
            SettingsEvent::Ide(settings) => self.save_settings((**settings).clone(), cx),
            SettingsEvent::Close => {
                self.settings = None;
                window.focus(&self.focus);
            }
            SettingsEvent::RunInTerminal(_) | SettingsEvent::OpenClaudeCode => {
                self.note = Some("Open a project to use its terminal.".into());
            }
            // Nothing of a project is open here.
            SettingsEvent::TrustChanged | SettingsEvent::InspectionsChanged => {}
        }
        cx.notify();
    }

    /// Import Settings…: an exchange file's preview; only the user's own settings are taken
    /// without a project (behaviour 17).
    pub fn import_settings(&mut self, cx: &mut Context<Self>) {
        self.gear = None;
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        let handle = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await
                && let Some(path) = paths.into_iter().next()
                && let Some(window) = cx.update(|cx| welcome_window(cx)).ok().flatten()
            {
                let _ = window.update(cx, |_, window, cx| {
                    let _ = handle.update(cx, |this, cx| this.preview_import(&path, window, cx));
                });
            }
        })
        .detach();
    }

    /// Where an import from the Welcome window writes: the user's files; no project.
    fn import_targets(cx: &App) -> ferris_idea_formats::Targets {
        ferris_idea_formats::Targets {
            // Never written: kinds with project data are refused before (behaviour 17).
            root: std::env::temp_dir().join("ferris-idea-welcome-no-project"),
            keymap_file: crate::keymaps::keymap_file(cx),
            settings_file: crate::appearance::ide_settings_file(cx),
            legacy_theme_file: crate::appearance::theme_file(cx),
        }
    }

    /// Shows the preview of the exchange file at `path`, or why it is not taken here.
    pub fn preview_import(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let preview = ferris_idea_formats::preview_file(path, &Self::import_targets(cx));
        if let Some(why) = import_refusal(preview.kind.as_deref()) {
            self.note = Some(why);
            cx.notify();
            return;
        }
        let view = cx.new(|cx| ImportPreview::new(preview, cx));
        self._import_events = Some(cx.subscribe_in(&view, window, Self::on_import));
        window.focus(&view.read(cx).focus());
        self.import = Some(view);
        cx.notify();
    }

    fn on_import(
        &mut self,
        view: &Entity<ImportPreview>,
        event: &ImportPreviewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preview = view.read(cx).preview().clone();
        match event {
            ImportPreviewEvent::Share => {
                let kind = preview.kind.clone().unwrap_or_else(|| "file".into());
                let (url, _) = ferris_idea_formats::share_url(&kind, &preview.file, &preview.text);
                cx.open_url(&url);
                return;
            }
            ImportPreviewEvent::Import => {
                self.note = Some(
                    match ferris_idea_formats::apply(&preview, &Self::import_targets(cx)) {
                        Ok(written) => {
                            crate::keymaps::load(cx);
                            crate::appearance::apply_saved_theme(cx);
                            crate::appearance::apply_saved_settings(cx);
                            cx.refresh_windows();
                            format!("Imported {}: {} changed.", preview.file, written.join(", "))
                        }
                        Err(e) => format!("Nothing imported: {e}"),
                    },
                );
            }
            ImportPreviewEvent::Close => {
                self.note = Some("Import cancelled; nothing changed.".into());
            }
        }
        self.import = None;
        window.focus(&self.focus);
        cx.notify();
    }

    /// Chooses `theme` for every window and saves it, as the theme popup does (behaviour 14).
    pub fn choose_theme(&mut self, theme: ThemeId, cx: &mut Context<Self>) {
        if let Err(e) = crate::appearance::choose_theme(theme, cx) {
            self.note = Some(format!("Theme not saved: {e}"));
        }
        cx.notify();
    }

    /// Applies and saves `settings` as Settings does (behaviours 14 and 15).
    fn save_settings(&mut self, settings: ferris_idea_core::IdeSettings, cx: &mut Context<Self>) {
        if let Err(e) = crate::appearance::set_ide_settings(settings, cx) {
            self.note = Some(format!("Settings not saved: {e}"));
        }
        cx.notify();
    }

    /// Switches Sync with OS (behaviour 14).
    pub fn toggle_sync(&mut self, cx: &mut Context<Self>) {
        let mut s = crate::appearance::ide_settings(cx);
        s.appearance.sync_with_os = !s.appearance.sync_with_os;
        self.save_settings(s, cx);
    }

    /// Chooses the editor colour scheme; empty: the theme's own (behaviour 15).
    pub fn set_scheme(&mut self, scheme: &str, cx: &mut Context<Self>) {
        let mut s = crate::appearance::ide_settings(cx);
        s.appearance.editor_scheme = scheme.to_owned();
        self.save_settings(s, cx);
    }

    /// The editor font size one point larger (`up`) or smaller (behaviour 15).
    pub fn step_font_size(&mut self, up: bool, cx: &mut Context<Self>) {
        let mut s = crate::appearance::ide_settings(cx);
        let delta = if up { 1.0 } else { -1.0 };
        if let Some(size) = ferris_idea_core::font_size_step(s.editor.font_size, delta) {
            s.editor.font_size = size;
            self.save_settings(s, cx);
        }
    }

    /// Switches compact mode (behaviour 15).
    pub fn toggle_compact(&mut self, cx: &mut Context<Self>) {
        let mut s = crate::appearance::ide_settings(cx);
        s.appearance.compact_mode = !s.appearance.compact_mode;
        self.save_settings(s, cx);
    }

    /// Carries out a gear menu entry (behaviour 21).
    pub fn gear_choose(&mut self, item: GearItem, window: &mut Window, cx: &mut Context<Self>) {
        self.gear = None;
        match item {
            GearItem::AllSettings => self.open_settings(Page::Appearance, window, cx),
            GearItem::ImportSettings => self.import_settings(cx),
            GearItem::Keymap => self.open_settings(Page::Keymap, window, cx),
            GearItem::Plugins => self.open_settings(Page::Plugins, window, cx),
        }
        cx.notify();
    }

    /// Opens Learn's entry `i`: the Keymap page, or the address in the browser — only on the
    /// user's click or key (behaviour 19).
    pub fn open_learn(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        match LEARN.get(i).map(|e| e.2) {
            Some(LearnTarget::Keymap) => self.open_settings(Page::Keymap, window, cx),
            Some(LearnTarget::Address(a)) => cx.open_url(a),
            None => {}
        }
    }

    /// The user closed the window with ⌘W (behaviour 4).
    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        cx.defer(closed_by_user);
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Keys of the form, the Settings dialog and the preview stay theirs.
        if !self.focus.is_focused(window) {
            return;
        }
        let k = &event.keystroke;
        let key = k.key.as_str();
        let m = &k.modifiers;
        if let Some(i) = self.gear {
            let n = GearItem::ALL.len();
            match key {
                "down" => self.gear = Some(step(i, n, true)),
                "up" => self.gear = Some(step(i, n, false)),
                "enter" => self.gear_choose(GearItem::ALL[i], window, cx),
                "escape" => self.gear = None,
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if m.platform && !m.control && !m.alt {
            match key {
                "w" => self.close_window(window, cx),
                "n" if self.section == Section::Ssh => {
                    self.ssh.update(cx, |s, cx| s.show_form("", window, cx))
                }
                "n" => self.show_form(Form::NewProject, window, cx),
                "o" => self.browse(cx),
                "backspace" if self.section == Section::Projects => {
                    self.remove_shown(self.selected, cx)
                }
                "backspace" if self.section == Section::Ssh => self.ssh.update(cx, |s, cx| {
                    let selected = s.selected();
                    s.remove_shown(selected, cx)
                }),
                digit => match Section::of_digit(digit) {
                    Some(s) => self.set_section(s, cx),
                    None => return,
                },
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.section == Section::Ssh {
            if self.ssh.update(cx, |s, cx| s.list_key(k, window, cx)) {
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        if self.section != Section::Projects {
            return;
        }
        let n = self.shown().len();
        let field_empty = self.query.is_empty();
        if let Some(to) =
            crate::list_nav::edge_with(k, n, crate::list_nav::Keys::beside_field(field_empty))
        {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match key {
            "down" => self.selected = step(self.selected, n, true),
            "up" => self.selected = step(self.selected, n, false),
            "enter" => self.open_shown(self.selected, cx),
            "escape" => {
                self.query.clear();
                self.selected = 0;
            }
            "backspace" => {
                self.query.pop();
                self.selected = 0;
            }
            _ => match k.key_char.as_deref() {
                Some(ch) if !m.control && !ch.chars().any(char::is_control) => {
                    self.query.push_str(ch);
                    self.selected = 0;
                }
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// A plain button of the window.
fn button(
    id: impl Into<gpui::ElementId>,
    text: &'static str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    let p = Palette::current();
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded(px(4.))
        .flex_none()
        .when(primary, |d| d.bg(c(p.accent)).text_color(c(p.accent_text)))
        .when(!primary, |d| {
            d.border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tree_hover)))
        })
        .child(text)
}

/// A heading inside a section.
fn heading(text: &'static str) -> gpui::Div {
    let p = Palette::current();
    div()
        .pt_3()
        .pb_1()
        .text_color(c(p.ui_muted))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(text)
}

/// A choice among several, the chosen one marked.
fn chip(id: impl Into<gpui::ElementId>, text: String, chosen: bool) -> gpui::Stateful<gpui::Div> {
    let p = Palette::current();
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(px(4.))
        .border_1()
        .border_color(c(if chosen { p.focus } else { p.separator }))
        .when(chosen, |d| d.bg(c(p.tree_selection)))
        .when(!chosen, |d| d.hover(|s| s.bg(c(p.tree_hover))))
        .child(text)
}

impl WelcomeView {
    fn render_column(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let mut column = div()
            .w(px(210.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(c(p.separator))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .child(
                        img(crate::icon_images::app_icon())
                            .size(px(40.))
                            .flex_none(),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Ferris IDEA"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(c(p.ui_dim))
                                    .child(env!("CARGO_PKG_VERSION")),
                            ),
                    ),
            );
        for section in Section::ALL {
            if let Some(group) = section.group() {
                column = column.child(
                    div()
                        .px_4()
                        .pt_2()
                        .text_xs()
                        .text_color(c(p.ui_dim))
                        .child(group),
                );
            }
            let chosen = section == self.section;
            let badge = (section == Section::Plugins && !self.plugins.is_empty())
                .then(|| self.plugins.len().to_string());
            column = column.child(
                div()
                    .id(("welcome-section", section as usize))
                    .mx_2()
                    .px_2()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(4.))
                    .when(chosen, |d| d.bg(c(p.tree_selection)))
                    .when(!chosen, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .when(section.group().is_some(), |d| d.pl_5())
                    .child(icon(section.icon()))
                    .child(div().flex_1().child(section.title()))
                    .when_some(badge, |d, n| {
                        d.child(
                            div()
                                .px_2()
                                .rounded(px(8.))
                                .text_xs()
                                .bg(c(p.accent))
                                .text_color(c(p.accent_text))
                                .child(n),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        this.set_section(section, cx);
                        window.focus(&this.focus);
                    })),
            );
        }
        let gear_menu = self.gear.map(|selected| {
            let mut menu = div()
                .absolute()
                .bottom(px(40.))
                .left(px(8.))
                .w(px(190.))
                .py_1()
                .bg(c(p.popup))
                .border_1()
                .border_color(c(p.popup_border))
                .rounded(px(6.))
                .shadow_lg();
            for (i, item) in GearItem::ALL.into_iter().enumerate() {
                menu = menu.child(
                    div()
                        .id(("welcome-gear-item", i))
                        .mx_1()
                        .px_2()
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .when(i == selected, |d| d.bg(c(p.tree_selection)))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(item.title())
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                            this.gear_choose(item, window, cx)
                        })),
                );
            }
            menu
        });
        column
            .child(div().flex_1())
            .child(
                div()
                    .relative()
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .id("welcome-gear")
                            .size(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(icon("general/settings"))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.gear = match this.gear {
                                    Some(_) => None,
                                    None => Some(0),
                                };
                                window.focus(&this.focus);
                                cx.notify();
                            })),
                    )
                    .children(gear_menu),
            )
            .into_any_element()
    }

    fn render_projects(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let search = div()
            .id("welcome-search")
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(4.))
            .border_1()
            .border_color(c(p.focus))
            .child(icon("general/search"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(self.query.is_empty(), |d| {
                        d.text_color(c(p.ui_dim)).child("Search projects")
                    })
                    .when(!self.query.is_empty(), |d| d.child(self.query.clone())),
            )
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                window.focus(&this.focus);
                cx.notify();
            }));
        let actions = div()
            .flex()
            .items_center()
            .gap_2()
            .child(search)
            .child(
                button("welcome-new-project", "New Project", true).on_click(cx.listener(
                    |this, _: &gpui::ClickEvent, window, cx| {
                        this.show_form(Form::NewProject, window, cx)
                    },
                )),
            )
            .child(
                button("welcome-open", "Open", false)
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.browse(cx))),
            )
            .child(
                button("welcome-clone", "Clone Repository", false).on_click(cx.listener(
                    |this, _: &gpui::ClickEvent, window, cx| {
                        this.show_form(Form::Clone, window, cx)
                    },
                )),
            );
        let mut content = div()
            .flex()
            .flex_col()
            .size_full()
            .p_4()
            .gap_2()
            .child(actions);
        if let Some(form) = self.form.clone() {
            return content
                .child(div().pt_2().flex().justify_center().child(form))
                .into_any_element();
        }
        let shown = self.shown();
        let mut list = div()
            .id("welcome-recent")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if self.rows.is_empty() {
            list =
                list.child(div().pt_4().text_color(c(p.ui_dim)).child(
                    "No recent projects yet: create one, open a folder or clone a repository.",
                ));
        } else if shown.is_empty() {
            list = list.child(
                div()
                    .pt_4()
                    .text_color(c(p.ui_dim))
                    .child("No matching projects."),
            );
        }
        for (n, &i) in shown.iter().enumerate() {
            let r = self.rows[i].clone();
            let selected = n == self.selected;
            let text = if r.missing { p.ui_dim } else { p.ui_text };
            list = list.child(
                div()
                    .id(("welcome-row", n))
                    .group("welcome-row")
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .when(selected, |d| d.bg(c(p.tree_selection)))
                    .when(!selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded(px(6.))
                            .bg(c(if r.missing { p.separator } else { p.accent }))
                            .text_color(c(p.accent_text))
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(r.initials.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_color(c(text))
                                    .child(r.name.clone())
                                    .when(r.missing, |d| {
                                        d.child(
                                            div()
                                                .text_xs()
                                                .text_color(c(p.error_text))
                                                .child("missing"),
                                        )
                                    })
                                    .when_some(r.branch.clone(), |d, b| {
                                        d.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .text_xs()
                                                .text_color(c(p.ui_dim))
                                                .child(icon("general/vcs"))
                                                .child(b),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(c(p.ui_dim))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(crate::welcome_model::middle_path(
                                        &r.shown,
                                        self.path_width,
                                        PATH_CHAR_WIDTH,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id(("welcome-remove", n))
                            .flex_none()
                            .px_2()
                            .rounded(px(4.))
                            .text_xs()
                            .border_1()
                            .border_color(c(p.separator))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .when(!selected && !r.missing, |d| {
                                d.invisible().group_hover("welcome-row", |s| s.visible())
                            })
                            .child(if r.missing {
                                "Remove"
                            } else {
                                "Remove from list"
                            })
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.remove_shown(n, cx)
                            })),
                    )
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        window.focus(&this.focus);
                        this.open_shown(n, cx)
                    })),
            );
        }
        content = content.child(list);
        if let Some(note) = self.note.clone() {
            content = content.child(div().text_xs().text_color(c(p.ui_muted)).child(note));
        }
        content.into_any_element()
    }

    fn render_customize(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let settings = crate::appearance::ide_settings(cx);
        let current = crate::palette::current_theme();
        let mut themes = div().flex().flex_wrap().gap_2();
        for (i, theme) in ThemeId::ALL.into_iter().enumerate() {
            themes = themes.child(
                chip(
                    ("welcome-theme", i),
                    theme.name().to_owned(),
                    theme == current,
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.choose_theme(theme, cx)
                })),
            );
        }
        let sync = settings.appearance.sync_with_os;
        let compact = settings.appearance.compact_mode;
        let scheme = settings.appearance.editor_scheme.clone();
        let mut schemes = div().flex().flex_wrap().gap_2().child(
            chip(
                "welcome-scheme-own",
                "Theme's own".to_owned(),
                scheme.is_empty(),
            )
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.set_scheme("", cx))),
        );
        for (i, name) in ferris_idea_core::EDITOR_SCHEMES.into_iter().enumerate() {
            schemes = schemes.child(
                chip(("welcome-scheme", i), name.to_owned(), scheme == name).on_click(
                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.set_scheme(name, cx)),
                ),
            );
        }
        let check = |id: &'static str, on: bool, text: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .py_1()
                .child(if on { "☑" } else { "☐" })
                .child(text)
        };
        div()
            .id("welcome-customize")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .child(heading("Theme"))
            .child(themes)
            .child(
                check("welcome-sync", sync, "Sync with OS").on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_sync(cx)),
                ),
            )
            .child(heading("Editor color scheme"))
            .child(schemes)
            .child(heading("Editor font size"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        button("welcome-font-smaller", "−", false).on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, _, cx| this.step_font_size(false, cx),
                        )),
                    )
                    .child(
                        div()
                            .w(px(48.))
                            .flex()
                            .justify_center()
                            .child(format!("{} pt", settings.editor.font_size)),
                    )
                    .child(
                        button("welcome-font-larger", "+", false).on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, _, cx| this.step_font_size(true, cx),
                        )),
                    ),
            )
            .child(
                check("welcome-compact", compact, "Compact mode").on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_compact(cx)),
                ),
            )
            .child(heading("Keymap"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(keymap_name())
                    .child(
                        button("welcome-keymap", "Configure…", false).on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, window, cx| {
                                this.open_settings(Page::Keymap, window, cx)
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .pt_4()
                    .flex()
                    .gap_4()
                    .child(
                        div()
                            .id("welcome-import")
                            .text_color(c(p.link))
                            .child("Import Settings…")
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.import_settings(cx)
                            })),
                    )
                    .child(
                        div()
                            .id("welcome-all-settings")
                            .text_color(c(p.link))
                            .child("All Settings…")
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.open_settings(Page::Appearance, window, cx)
                            })),
                    ),
            )
            .when_some(self.note.clone(), |d, note| {
                d.child(div().pt_3().text_xs().text_color(c(p.ui_muted)).child(note))
            })
            .into_any_element()
    }

    fn render_plugins(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let mut list = div().flex().flex_col().gap_1();
        if self.plugins.is_empty() {
            list =
                list.child(div().text_color(c(p.ui_dim)).child(
                    "No plugins installed. Plugins are WebAssembly files installed from disk.",
                ));
        }
        for plugin in &self.plugins {
            let (name, version) = match &plugin.manifest {
                Some(m) => (m.name.clone(), m.version.clone()),
                None => (plugin.file.clone(), String::new()),
            };
            let state = match (&plugin.problem, plugin.enabled) {
                (Some(_), _) => "cannot load",
                (None, true) => "enabled",
                (None, false) => "disabled",
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .py_1()
                    .child(icon("nodes/module"))
                    .child(div().flex_1().child(name))
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(version))
                    .child(div().text_xs().text_color(c(p.ui_muted)).child(state)),
            );
        }
        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(heading("Installed"))
            .child(list)
            .child(div().pt_2().child(
                button("welcome-manage-plugins", "Manage Plugins…", false).on_click(cx.listener(
                    |this, _: &gpui::ClickEvent, window, cx| {
                        this.open_settings(Page::Plugins, window, cx)
                    },
                )),
            ))
            .into_any_element()
    }

    fn render_learn(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let mut list = div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(heading("Learn Ferris IDEA"));
        for (i, (title, what, target)) in LEARN.into_iter().enumerate() {
            let place = match target {
                LearnTarget::Keymap => "Settings › Keymap".to_owned(),
                LearnTarget::Address(a) => a.to_owned(),
            };
            list = list.child(
                div()
                    .id(("welcome-learn", i))
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_1()
                    .rounded(px(4.))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(div().text_color(c(p.link)).child(title))
                    .child(div().text_xs().text_color(c(p.ui_muted)).child(what))
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(place))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        this.open_learn(i, window, cx)
                    })),
            );
        }
        list.into_any_element()
    }
}

impl Render for WelcomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        // The row's path gets what the column, the padding, the initials and the Remove button
        // leave of the window's width (SPEC-0355 v4 behaviour 24).
        self.path_width = (f32::from(window.viewport_size().width) - PATH_CHROME).max(80.);
        if std::mem::take(&mut self.refocus) {
            window.focus(&self.focus);
        }
        let content = match self.section {
            Section::Projects => self.render_projects(cx),
            Section::Ssh => self.ssh.clone().into_any_element(),
            Section::Customize => self.render_customize(cx),
            Section::Plugins => self.render_plugins(cx),
            Section::Learn => self.render_learn(cx),
        };
        div()
            .id("welcome")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .size_full()
            .flex()
            .bg(c(p.window))
            .text_color(c(p.ui_text))
            .text_sm()
            .child(self.render_column(cx))
            .child(div().flex_1().min_w_0().h_full().child(content))
            .when_some(self.settings.clone(), |d, dialog| {
                d.child(
                    div()
                        .absolute()
                        .top(px(crate::settings_view::dialog_top()))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(dialog),
                )
            })
            .when_some(self.import.clone(), |d, preview| {
                d.child(
                    div()
                        .absolute()
                        .top(px(60.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(preview),
                )
            })
    }
}
