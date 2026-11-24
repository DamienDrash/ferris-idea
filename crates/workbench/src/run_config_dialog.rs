//! The run configuration editor (`SPEC-0327`), a dialog drawn in the window: the configurations
//! on the left (those of `run.toml` with their folders, then "Temporary", then the detected ones
//! greyed), a toolbar to add with a type, remove, copy, move and make a folder, one form for
//! every configuration on the right with its problems under the fields, a pane for a folder
//! (rename, remove), the templates per type (v3 behaviour 16), and Run, Debug, Cancel, Apply and
//! OK.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_core::{RunConfig, RunField, RunTemplate};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Subscription, Window, div,
    prelude::*, px,
};

use crate::editor_view::{EditorEvent, EditorView};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::run_editor_model::{EditorModel, Row, Source};
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::tips::Tipped;
use crate::workbench_view::c;

/// How often the dialog looks whether `run.toml` changed on disk (behaviour 7).
const WATCH: Duration = Duration::from_millis(700);

/// Height of the dialog while its content needs no more (`SPEC-0327` v4 behaviour 20).
pub const DIALOG_HEIGHT: f32 = 580.;
/// Distance of the dialog's top edge from the main toolbar.
pub const DIALOG_TOP: f32 = 40.;
/// Space kept between the dialog and the bottom edge of the window.
const DIALOG_BOTTOM_MARGIN: f32 = 16.;
/// The dialog never gets lower than this, however low the window is.
const DIALOG_LEAST_HEIGHT: f32 = 240.;

/// The least and the most height of the dialog when `room` points lie between its top edge and
/// the bottom edge of the window (`SPEC-0327` v4 behaviour 20): it is [`DIALOG_HEIGHT`] high,
/// grows with a form that needs more so that every row is shown, and ends
/// [`DIALOG_BOTTOM_MARGIN`] above the window's edge at the latest — from there on the form
/// scrolls and the buttons under it stay in view.
pub fn dialog_heights(room: f32) -> (f32, f32) {
    let most = (room - DIALOG_BOTTOM_MARGIN).max(DIALOG_LEAST_HEIGHT);
    (DIALOG_HEIGHT.min(most), most)
}

/// What the dialog asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunConfigDialogEvent {
    /// Close the dialog.
    Close,
    /// Cancel or Escape with changes: ask before closing (behaviour 5).
    AskDiscard,
    /// `run.toml` was written; read it again.
    Saved,
    /// The session's temporary configurations are now these (behaviour 9).
    Temporary(Vec<RunConfig>),
    /// Start configuration `name` (run or debug) and close (behaviour 6).
    Start {
        /// Configuration name.
        name: String,
        /// Debug instead of run.
        debug: bool,
    },
    /// A line for the status bar.
    Status(String),
}

/// What a control of the dialog is for the UI driver (`SPEC-0319`): it changes the dialog only;
/// nothing is written before Apply.
fn control(role: Role, id: impl Into<String>, name: impl Into<String>) -> Semantics {
    Semantics::new(role)
        .name(name)
        .test_id(id)
        .action(AxAction::Click)
        .effects(&[Effect::None])
}

/// The form's text fields, in Tab order.
const FIELDS: [&str; 12] = [
    "Name",
    "Command line",
    "Working directory",
    "Environment",
    "Report format",
    "Report file",
    "Timeout (seconds)",
    "Folder",
    "Before-launch tasks",
    "Interpreter",
    "Options",
    "Folder name",
];
const NAME: usize = 0;
const COMMAND: usize = 1;
const CWD: usize = 2;
const ENV: usize = 3;
const REPORT: usize = 4;
const REPORT_FILE: usize = 5;
const TIMEOUT: usize = 6;
const FOLDER: usize = 7;
const TASKS: usize = 8;
/// Only in a type's template (v3 behaviour 16).
const INTERPRETER: usize = 9;
const OPTIONS: usize = 10;
/// Only in a folder's pane (v3 behaviour 15).
const FOLDER_NAME: usize = 11;

/// The problems shown under a field.
fn field_of(i: usize) -> Option<RunField> {
    Some(match i {
        NAME => RunField::Name,
        COMMAND => RunField::Command,
        CWD => RunField::Cwd,
        ENV => RunField::Env,
        REPORT | REPORT_FILE => RunField::Report,
        TIMEOUT => RunField::Timeout,
        FOLDER => RunField::Folder,
        OPTIONS => RunField::Command,
        // The problems of the tasks are shown under Before launch as a whole.
        _ => return None,
    })
}

fn trace(what: &str) {
    crate::workbench_view::run_editor_trace(&format!("dialog: {what}"));
}

/// The configuration editor.
pub struct RunConfigDialog {
    root: PathBuf,
    model: EditorModel,
    fields: Vec<Entity<EditorView>>,
    filter: Entity<EditorView>,
    template_filter: Entity<EditorView>,
    /// The template chooser of Add is open, with this template highlighted.
    templates: Option<usize>,
    /// The picker of Before launch's Add is open.
    before_pick: bool,
    /// The entry the fields show.
    loaded: Option<usize>,
    /// The type whose template the fields show instead (v3 behaviour 16).
    loaded_template: Option<RunTemplate>,
    /// Problems of the templates as read from `run.toml` (an unknown type, an unknown key).
    template_file_problems: Vec<String>,
    /// Why nothing can be written or started (safe mode, behaviour 8).
    read_only: Option<String>,
    /// `run.toml` as last read or written, to see changes from outside (behaviour 7).
    file_text: String,
    /// What the last Apply could not write.
    problems: Vec<String>,
    focus: FocusHandle,
    /// Scroll position of the form (a configuration's or a template's).
    form_scroll: gpui::ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RunConfigDialogEvent> for RunConfigDialog {}

fn run_file_text(root: &Path) -> String {
    std::fs::read_to_string(root.join(ferris_idea_core::RUN_FILE)).unwrap_or_default()
}

impl RunConfigDialog {
    /// The editor for the workspace at `root` over its configurations of `run.toml`, the
    /// session's `temporary` and the `detected` ones, with `select` selected (behaviour 1).
    /// `read_only` says why nothing may be written or started (behaviour 8).
    pub fn new(
        root: &Path,
        temporary: Vec<RunConfig>,
        detected: Vec<RunConfig>,
        select: Option<&str>,
        read_only: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let file_text = run_file_text(root);
        let project = ferris_idea_core::entries_of(&file_text)
            .map(|e| e.into_iter().map(|e| e.config).collect())
            .unwrap_or_default();
        let templates = ferris_idea_core::parse_run_templates(&file_text);
        let model = EditorModel::new(project, temporary, detected, select)
            .with_templates(templates.templates);
        let fields: Vec<Entity<EditorView>> =
            FIELDS.iter().map(|_| cx.new(EditorView::field)).collect();
        let filter = cx.new(EditorView::field);
        let template_filter = cx.new(EditorView::field);
        let mut subscriptions: Vec<Subscription> = fields
            .iter()
            .map(|f| cx.subscribe(f, |_, _, _: &EditorEvent, cx| cx.notify()))
            .collect();
        subscriptions.push(cx.subscribe(&filter, |this, f, _: &EditorEvent, cx| {
            this.model.filter = f.read(cx).state.buffer.text();
            cx.notify();
        }));
        subscriptions.push(
            cx.subscribe(&template_filter, |this, _, _: &EditorEvent, cx| {
                this.templates = Some(0);
                cx.notify();
            }),
        );
        let mut dialog = Self {
            root: root.to_path_buf(),
            model,
            fields,
            filter,
            template_filter,
            templates: None,
            before_pick: false,
            loaded: None,
            loaded_template: None,
            template_file_problems: templates.problems,
            read_only,
            file_text,
            problems: Vec::new(),
            focus: cx.focus_handle(),
            form_scroll: gpui::ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        dialog.load_form(cx);
        dialog.watch(cx);
        dialog
    }

    /// How far the form's content is higher than the part of it in view, in points: 0 when every
    /// row is shown, more when the form scrolls (`SPEC-0327` v4 behaviour 20).
    pub fn form_overflow(&self) -> f32 {
        f32::from(self.form_scroll.max_offset().height)
    }

    /// Focus handle of the list.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The list and its forms (checks).
    pub fn model(&self) -> &EditorModel {
        &self.model
    }

    /// Why the dialog is read-only, if it is.
    pub fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref()
    }

    /// What the last Apply could not write.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Whether the template chooser of Add is open.
    pub fn templates_open(&self) -> bool {
        self.templates.is_some()
    }

    /// Looks for changes of `run.toml` on disk while the dialog is open (behaviour 7).
    fn watch(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH).await;
                let alive = this.update(cx, |this, cx| this.check_disk(cx));
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn check_disk(&mut self, cx: &mut Context<Self>) {
        let text = run_file_text(&self.root);
        if text == self.file_text {
            return;
        }
        self.file_text = text;
        let Ok(entries) = ferris_idea_core::entries_of(&self.file_text) else {
            return;
        };
        self.sync_form(cx);
        let edited = self.model.current().is_some_and(|e| e.changed());
        self.model
            .reload(entries.into_iter().map(|e| e.config).collect());
        let templates = ferris_idea_core::parse_run_templates(&self.file_text);
        self.template_file_problems = templates.problems;
        let template_edited = self
            .loaded_template
            .is_some_and(|k| self.model.template_changed(k));
        self.model.reload_templates(templates.templates);
        if self.loaded_template.is_some() {
            if !template_edited {
                self.load_form(cx);
            }
        } else if edited {
            self.loaded = self.model.selected;
        } else {
            self.load_form(cx);
        }
        cx.notify();
    }

    fn text(&self, i: usize, cx: &gpui::App) -> String {
        self.fields[i].read(cx).state.buffer.text()
    }

    /// Takes the fields' text into the entry they show.
    fn sync_form(&mut self, cx: &gpui::App) {
        if self.read_only.is_some() {
            return;
        }
        if let Some(kind) = self.loaded_template {
            let texts: Vec<String> = (0..FIELDS.len()).map(|f| self.text(f, cx)).collect();
            self.model.edit_template(kind, |f| {
                f.interpreter = texts[INTERPRETER].clone();
                f.options = texts[OPTIONS].clone();
                f.cwd = texts[CWD].clone();
                f.env = texts[ENV].clone();
                f.timeout = texts[TIMEOUT].clone();
                f.tasks = texts[TASKS].clone();
            });
            return;
        }
        let Some(i) = self.loaded else {
            return;
        };
        let texts: Vec<String> = (0..FIELDS.len()).map(|f| self.text(f, cx)).collect();
        let Some(e) = self.model.entries.get_mut(i) else {
            return;
        };
        if e.source == Source::Detected {
            return;
        }
        let f = &mut e.form;
        f.name = texts[NAME].clone();
        f.command = texts[COMMAND].clone();
        f.cwd = texts[CWD].clone();
        f.env = texts[ENV].clone();
        f.report = texts[REPORT].clone();
        f.report_file = texts[REPORT_FILE].clone();
        f.timeout = texts[TIMEOUT].clone();
        f.folder = texts[FOLDER].clone();
        f.tasks = texts[TASKS].clone();
    }

    /// Shows the selected entry in the fields.
    fn load_form(&mut self, cx: &mut Context<Self>) {
        self.before_pick = false;
        if self.model.template_mode {
            let kind = self.model.template_selected;
            self.loaded = None;
            self.loaded_template = Some(kind);
            let form = self.model.template_form(kind);
            let mut texts = vec![String::new(); FIELDS.len()];
            texts[INTERPRETER] = form.interpreter;
            texts[OPTIONS] = form.options;
            texts[CWD] = form.cwd;
            texts[ENV] = form.env;
            texts[TIMEOUT] = form.timeout;
            texts[TASKS] = form.tasks;
            let locked = self.read_only.is_some();
            for (f, t) in self.fields.iter().zip(texts) {
                f.update(cx, |f, cx| {
                    f.set_text(&t, cx);
                    f.set_read_only(locked);
                });
            }
            return;
        }
        self.loaded_template = None;
        self.loaded = self.model.selected;
        let form = self
            .model
            .current()
            .map(|e| e.form.clone())
            .unwrap_or_default();
        let texts = [
            form.name,
            form.command,
            form.cwd,
            form.env,
            form.report,
            form.report_file,
            form.timeout,
            form.folder,
            form.tasks,
            String::new(),
            String::new(),
            self.model.selected_folder.clone().unwrap_or_default(),
        ];
        let locked = self.read_only.is_some()
            || self
                .model
                .current()
                .is_none_or(|e| e.source == Source::Detected);
        let read_only = self.read_only.is_some();
        for (k, (f, t)) in self.fields.iter().zip(texts).enumerate() {
            f.update(cx, |f, cx| {
                f.set_text(&t, cx);
                // A folder is renamed whatever configuration is selected.
                f.set_read_only(if k == FOLDER_NAME { read_only } else { locked });
            });
        }
    }

    /// Shows the templates per type instead of the configurations, or back (v3 behaviour 16).
    pub fn show_templates(&mut self, on: bool, cx: &mut Context<Self>) {
        self.sync_form(cx);
        self.templates = None;
        self.model.template_mode = on;
        self.model.selected_folder = None;
        self.load_form(cx);
        cx.notify();
    }

    /// Whether the templates per type are shown.
    pub fn templates_shown(&self) -> bool {
        self.model.template_mode
    }

    /// Shows the template of type `kind`.
    pub fn select_template(&mut self, kind: RunTemplate, cx: &mut Context<Self>) {
        self.sync_form(cx);
        self.model.template_mode = true;
        self.model.template_selected = kind;
        self.load_form(cx);
        cx.notify();
    }

    /// Shows the pane of folder `name` (v3 behaviour 15).
    pub fn select_folder(&mut self, name: &str, cx: &mut Context<Self>) {
        self.sync_form(cx);
        if self.model.folders().iter().any(|f| f == name) {
            self.model.selected_folder = Some(name.to_owned());
            self.load_form(cx);
            cx.notify();
        }
    }

    /// New Folder: a folder without configurations, its name ready to be changed.
    pub fn new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only.is_some() || self.model.template_mode {
            return;
        }
        self.sync_form(cx);
        self.model.new_folder();
        self.load_form(cx);
        let f = self.fields[FOLDER_NAME].read(cx).focus();
        window.focus(&f);
        cx.notify();
    }

    /// Rename: the shown folder takes the name typed in its pane; false, with the reason shown
    /// under the field, when the name is none or taken.
    pub fn rename_folder(&mut self, cx: &mut Context<Self>) -> bool {
        if self.read_only.is_some() {
            return false;
        }
        let Some(old) = self.model.selected_folder.clone() else {
            return false;
        };
        self.sync_form(cx);
        let new = self.text(FOLDER_NAME, cx);
        let done = self.model.rename_folder(&old, &new).is_ok();
        if done {
            self.load_form(cx);
        }
        cx.notify();
        done
    }

    /// Remove Folder: the shown folder goes, its configurations are listed on their own.
    pub fn remove_folder(&mut self, cx: &mut Context<Self>) {
        if self.read_only.is_some() {
            return;
        }
        let Some(name) = self.model.selected_folder.clone() else {
            return;
        };
        self.sync_form(cx);
        if self.model.remove_folder(&name) {
            self.load_form(cx);
            cx.notify();
        }
    }

    /// Puts the selected configuration into `folder`, or takes it out of its folder.
    pub fn move_to_folder(&mut self, folder: Option<&str>, cx: &mut Context<Self>) {
        if self.read_only.is_some() {
            return;
        }
        self.sync_form(cx);
        if let Some(i) = self.loaded
            && self.model.set_folder(i, folder)
        {
            self.load_form(cx);
            cx.notify();
        }
    }

    /// Selects entry `i` of the list.
    pub fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.sync_form(cx);
        if i < self.model.entries.len() {
            self.model.selected_folder = None;
            self.model.selected = Some(i);
            self.load_form(cx);
            cx.notify();
        }
    }

    /// Selects the configuration called `name`.
    pub fn select_name(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.model.entries.iter().position(|e| e.name() == name) {
            self.select(i, cx);
        }
    }

    /// Sets the text of field `label` (one of the form's labels) as typing would.
    pub fn set_field(&mut self, label: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(i) = FIELDS.iter().position(|f| *f == label) {
            self.fields[i].update(cx, |f, cx| f.set_text(text, cx));
            self.sync_form(cx);
            cx.notify();
        }
    }

    /// Opens the template chooser of Add (⌘N, behaviour 3).
    pub fn open_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only.is_some() || self.model.template_mode {
            return;
        }
        self.templates = Some(0);
        self.template_filter.update(cx, |f, cx| f.set_text("", cx));
        let f = self.template_filter.read(cx).focus();
        window.focus(&f);
        cx.notify();
    }

    /// The templates the chooser's filter keeps.
    pub fn shown_templates(&self, cx: &gpui::App) -> Vec<RunTemplate> {
        let filter = self.template_filter.read(cx).state.buffer.text();
        RunTemplate::ALL
            .into_iter()
            .filter(|t| t.matches(&filter))
            .collect()
    }

    /// Adds a configuration of `template` and shows it in the form, its command line focused
    /// (behaviour 3).
    pub fn add(&mut self, template: RunTemplate, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only.is_some() {
            return;
        }
        self.sync_form(cx);
        self.templates = None;
        self.model.template_mode = false;
        self.model.selected_folder = None;
        self.model.add(template);
        self.load_form(cx);
        let f = self.fields[COMMAND].read(cx).focus();
        window.focus(&f);
        cx.notify();
    }

    /// Remove (⌫): removes the selected configuration.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        if self.read_only.is_some() || self.model.template_mode {
            return;
        }
        // With a folder shown, Remove removes the folder (v3 behaviour 15).
        if self.model.selected_folder.is_some() {
            self.remove_folder(cx);
            return;
        }
        self.sync_form(cx);
        if self.model.remove_selected() {
            self.load_form(cx);
            cx.notify();
        }
    }

    /// Copy (⌘D): an editable copy of the selected configuration (also of a detected one).
    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if self.read_only.is_some() || self.model.template_mode {
            return;
        }
        self.model.selected_folder = None;
        self.sync_form(cx);
        if self.model.copy_selected().is_some() {
            self.load_form(cx);
            cx.notify();
        }
    }

    /// Move up or down (⌥↑/⌥↓).
    pub fn move_selected(&mut self, up: bool, cx: &mut Context<Self>) {
        if self.read_only.is_some()
            || self.model.template_mode
            || self.model.selected_folder.is_some()
        {
            return;
        }
        self.sync_form(cx);
        if self.model.move_selected(up) {
            self.loaded = self.model.selected;
            cx.notify();
        }
    }

    /// Save Configuration of the selected temporary configuration (behaviour 9).
    pub fn save_temporary(&mut self, cx: &mut Context<Self>) {
        if self.read_only.is_some() {
            return;
        }
        self.sync_form(cx);
        if self.model.save_temporary().is_some() {
            self.load_form(cx);
            cx.notify();
        }
    }

    /// Switches "Allow multiple instances" of the selected configuration (`SPEC-0327` v2 behaviour 12).
    pub fn set_allow_multiple(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(kind) = self.loaded_template {
            if self.read_only.is_none() {
                self.sync_form(cx);
                self.model.edit_template(kind, |f| f.allow_multiple = on);
                cx.notify();
            }
            return;
        }
        self.edit_form(cx, |f| f.allow_multiple = on);
    }

    /// Changes the list of configurations to run first, of the shown configuration or of the
    /// shown template.
    fn edit_before(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Vec<String>)) {
        if let Some(kind) = self.loaded_template {
            if self.read_only.is_none() {
                self.sync_form(cx);
                self.model.edit_template(kind, |f| change(&mut f.before));
                cx.notify();
            }
            return;
        }
        self.edit_form(cx, |f| change(&mut f.before));
    }

    /// Adds configuration `name` to the list of those to run first.
    pub fn add_before(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.before_pick = false;
        self.edit_before(cx, move |b| {
            if !b.contains(&name) {
                b.push(name);
            }
        });
    }

    /// Adds a before-launch task as a line of the tasks field (v2 behaviour 13): `build`, or the
    /// start of a `tool` or `browser` line to complete; the field gets the focus.
    pub fn add_task_line(&mut self, line: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only.is_some() {
            return;
        }
        self.before_pick = false;
        let mut text = self.text(TASKS, cx);
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(line);
        self.fields[TASKS].update(cx, |f, cx| f.set_text(&text, cx));
        self.sync_form(cx);
        let f = self.fields[TASKS].read(cx).focus();
        window.focus(&f);
        cx.notify();
    }

    /// Switches the selected configuration between run and test.
    pub fn set_test(&mut self, test: bool, cx: &mut Context<Self>) {
        self.edit_form(cx, |f| f.test = test);
    }

    fn edit_form(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut crate::run_editor_model::RunForm),
    ) {
        if self.read_only.is_some() {
            return;
        }
        self.sync_form(cx);
        if let Some(i) = self.loaded
            && let Some(e) = self.model.entries.get_mut(i)
            && e.source != Source::Detected
        {
            change(&mut e.form);
            cx.notify();
        }
    }

    /// Apply (behaviour 5): writes `run.toml`; false, with the problems shown, when nothing could
    /// be written.
    pub fn apply(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(why) = self.read_only.clone() {
            cx.emit(RunConfigDialogEvent::Status(why));
            return false;
        }
        self.sync_form(cx);
        let (entries, templates) = match (self.model.to_write(), self.model.templates_to_write()) {
            (Ok(e), Ok(t)) => (e, t),
            (e, t) => {
                self.problems = e.err().unwrap_or_default();
                self.problems.extend(t.err().unwrap_or_default());
                cx.notify();
                return false;
            }
        };
        match ferris_idea_core::save_run_file(&self.root, &entries, Some(&templates)) {
            Ok(_) => {
                self.model.written();
                self.file_text = run_file_text(&self.root);
                self.template_file_problems =
                    ferris_idea_core::parse_run_templates(&self.file_text).problems;
                self.problems.clear();
                cx.emit(RunConfigDialogEvent::Temporary(self.model.temporary()));
                cx.emit(RunConfigDialogEvent::Saved);
                cx.notify();
                true
            }
            Err(p) => {
                self.problems = p;
                cx.notify();
                false
            }
        }
    }

    /// OK: writes and closes.
    pub fn ok(&mut self, cx: &mut Context<Self>) {
        if self.read_only.is_some() {
            trace("ok, read-only: close");
            cx.emit(RunConfigDialogEvent::Close);
            return;
        }
        if self.apply(cx) {
            trace("ok, written: close");
            cx.emit(RunConfigDialogEvent::Close);
        }
    }

    /// Cancel or Escape: closes without writing; with changes asks first (behaviour 5).
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.sync_form(cx);
        if self.read_only.is_none() && self.model.dirty() {
            trace("cancel with changes: ask");
            cx.emit(RunConfigDialogEvent::AskDiscard);
        } else {
            trace("cancel: close");
            cx.emit(RunConfigDialogEvent::Close);
        }
    }

    /// Run or Debug (behaviour 6): applies, then starts the selected configuration and closes.
    pub fn start(&mut self, debug: bool, cx: &mut Context<Self>) {
        if let Some(why) = self.read_only.clone() {
            cx.emit(RunConfigDialogEvent::Status(why));
            return;
        }
        if !self.apply(cx) {
            return;
        }
        if self.model.template_mode || self.model.selected_folder.is_some() {
            cx.emit(RunConfigDialogEvent::Status(
                "Select a configuration to run".into(),
            ));
            return;
        }
        if let Some(e) = self.model.current() {
            cx.emit(RunConfigDialogEvent::Start {
                name: e.name().trim().to_owned(),
                debug,
            });
        }
        trace("start: close");
        cx.emit(RunConfigDialogEvent::Close);
    }

    fn on_capture_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &event.keystroke;
        let cmd = if cfg!(target_os = "macos") {
            k.modifiers.platform
        } else {
            k.modifiers.control
        };
        let handled = match k.key.as_str() {
            "escape" if self.templates.is_some() => {
                self.templates = None;
                window.focus(&self.focus);
                true
            }
            "escape" if self.before_pick => {
                self.before_pick = false;
                true
            }
            "escape" => {
                trace("escape key: cancel");
                self.cancel(cx);
                true
            }
            "down" | "up" if self.templates.is_some() => {
                let n = self.shown_templates(cx).len().max(1);
                let at = self.templates.unwrap_or(0);
                self.templates = Some(if k.key == "down" {
                    (at + 1) % n
                } else {
                    (at + n - 1) % n
                });
                true
            }
            "enter" if self.templates.is_some() => {
                let shown = self.shown_templates(cx);
                if let Some(t) = shown.get(self.templates.unwrap_or(0)).copied() {
                    self.add(t, window, cx);
                }
                true
            }
            "enter" if self.fields[FOLDER_NAME].read(cx).focus().is_focused(window) => {
                self.rename_folder(cx);
                true
            }
            "enter" if cmd => {
                trace("⌘↩: ok");
                self.ok(cx);
                true
            }
            "n" if cmd => {
                self.open_templates(window, cx);
                true
            }
            "d" if cmd => {
                self.copy(cx);
                true
            }
            "up" | "down" if k.modifiers.alt => {
                self.move_selected(k.key == "up", cx);
                true
            }
            "down" | "up" if self.focus.is_focused(window) && self.model.template_mode => {
                let all = RunTemplate::ALL;
                let at = all
                    .iter()
                    .position(|t| *t == self.model.template_selected)
                    .unwrap_or(0);
                let next = if k.key == "down" {
                    (at + 1).min(all.len() - 1)
                } else {
                    at.saturating_sub(1)
                };
                self.select_template(all[next], cx);
                true
            }
            "down" | "up" if self.focus.is_focused(window) => {
                let visible = self.model.visible();
                let at = self
                    .model
                    .selected
                    .and_then(|s| visible.iter().position(|&v| v == s));
                let next = match (at, k.key == "down") {
                    (None, _) => visible.first().copied(),
                    (Some(a), true) => visible.get((a + 1).min(visible.len() - 1)).copied(),
                    (Some(a), false) => visible.get(a.saturating_sub(1)).copied(),
                };
                if let Some(i) = next {
                    self.select(i, cx);
                }
                true
            }
            "backspace" | "delete" if self.focus.is_focused(window) => {
                self.remove(cx);
                true
            }
            "enter" if self.focus.is_focused(window) => {
                let first = if self.model.template_mode {
                    INTERPRETER
                } else if self.model.selected_folder.is_some() {
                    FOLDER_NAME
                } else {
                    NAME
                };
                let f = self.fields[first].read(cx).focus();
                window.focus(&f);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }
}

impl Render for RunConfigDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_form(cx);
        // As high as its rows need, at most down to the window's edge (v4 behaviour 20).
        let (least_height, most_height) = dialog_heights(
            f32::from(window.viewport_size().height)
                - Palette::current().metrics.toolbar_height
                - DIALOG_TOP,
        );
        let p = Palette::current();
        let selected = self.model.selected;
        let locked = self.read_only.is_some();
        let template_mode = self.model.template_mode;
        let tool = |id: &'static str, key: &'static str, title: &str| {
            crate::metrics::header_button(id)
                .when(locked, |d| d.opacity(0.4))
                .child(icon(key))
                .tip_text(
                    id,
                    ferris_idea_ui::tooltip::Tip {
                        title: title.into(),
                        shortcut: String::new(),
                        details: Vec::new(),
                    },
                )
        };
        // A text button of the toolbar, known to the UI driver by its test id.
        let text_tool = |id: &'static str, label: &'static str, width: f32| {
            crate::metrics::header_button(id)
                .relative()
                .w(px(width))
                .when(locked, |d| d.opacity(0.4))
                .text_xs()
                .child(label)
                .semantics(control(Role::Button, id, label).enabled(!locked))
        };
        let toolbar = if template_mode {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    crate::metrics::header_button("run-config-configurations")
                        .relative()
                        .w(px(120.))
                        .text_xs()
                        .child("‹ Configurations")
                        .semantics(control(
                            Role::Button,
                            "run-config-configurations",
                            "Configurations",
                        ))
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.show_templates(false, cx);
                            window.focus(&this.focus);
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child("Templates"),
                )
        } else {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    tool("run-config-add", "general/add", &keys("Add ⌘N")).on_click(cx.listener(
                        |this, _: &gpui::ClickEvent, window, cx| this.open_templates(window, cx),
                    )),
                )
                .child(
                    tool("run-config-remove", "general/delete", &keys("Remove ⌫"))
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.remove(cx))),
                )
                .child(
                    crate::metrics::header_button("run-config-copy")
                        .w(px(40.))
                        .when(locked, |d| d.opacity(0.4))
                        .text_xs()
                        .child("Copy")
                        .tip_text(
                            "run-config-copy",
                            ferris_idea_ui::tooltip::Tip {
                                title: keys("Copy ⌘D"),
                                shortcut: String::new(),
                                details: Vec::new(),
                            },
                        )
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.copy(cx))),
                )
                .child(
                    tool("run-config-up", "general/up", &keys("Move Up ⌥↑")).on_click(cx.listener(
                        |this, _: &gpui::ClickEvent, _, cx| this.move_selected(true, cx),
                    )),
                )
                .child(
                    tool("run-config-down", "general/down", &keys("Move Down ⌥↓")).on_click(
                        cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                            this.move_selected(false, cx)
                        }),
                    ),
                )
                .child(
                    text_tool("run-config-new-folder", "New Folder", 72.).on_click(cx.listener(
                        |this, _: &gpui::ClickEvent, window, cx| this.new_folder(window, cx),
                    )),
                )
        };
        // Below the list: the way to the templates per type (v3 behaviour 16).
        let to_templates = (!template_mode).then(|| {
            div()
                .id("run-config-templates")
                .relative()
                .flex_none()
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .rounded(px(4.))
                .hover(|s| s.bg(c(p.tree_hover)))
                .text_xs()
                .text_color(c(p.ui_muted))
                .child("Edit templates per type…")
                .semantics(control(
                    Role::Button,
                    "run-config-templates",
                    "Edit templates per type",
                ))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                    this.show_templates(true, cx);
                    window.focus(&this.focus);
                }))
        });
        let items: Vec<gpui::AnyElement> = if template_mode {
            self.template_rows(cx)
        } else {
            self.list_rows(selected, cx)
        };
        let empty = (self.model.entries.is_empty()
            && self.model.folders().is_empty()
            && !template_mode)
            .then(|| {
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .items_start()
                    .text_color(c(p.ui_muted))
                    .child("No run configurations")
                    .child(
                        crate::metrics::button("run-config-empty-add", "Add Configuration…", true)
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.open_templates(window, cx)
                            })),
                    )
            });
        let left = div()
            .w(px(260.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .pr_2()
            .border_r_1()
            .border_color(c(p.separator))
            .child(toolbar)
            .when(!template_mode, |d| {
                d.child(crate::metrics::field_frame().child(self.filter.clone()))
            })
            .child(
                div()
                    .id("run-config-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(items)
                    .children(empty),
            )
            .children(to_templates)
            .children(
                (template_mode && !self.template_file_problems.is_empty()).then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .text_xs()
                        .text_color(c(p.error_text))
                        .children(
                            self.template_file_problems
                                .iter()
                                .map(|q| div().whitespace_normal().child(q.clone())),
                        )
                }),
            );
        let right = if template_mode {
            self.template_pane(cx)
        } else if let Some(folder) = self.model.selected_folder.clone() {
            self.folder_pane(&folder, cx)
        } else {
            self.form_pane(cx)
        };
        let templates = self.templates.map(|at| {
            let shown = self.shown_templates(cx);
            div()
                .id("run-config-templates")
                .absolute()
                .top(px(48.))
                .left(px(16.))
                .w(px(240.))
                .p_1()
                .rounded(px(6.))
                .border_1()
                .border_color(c(p.popup_border))
                .bg(c(p.popup))
                .shadow_lg()
                .occlude()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child("Add New Configuration"),
                )
                .child(crate::metrics::field_frame().child(self.template_filter.clone()))
                .children(shown.into_iter().enumerate().map(|(k, t)| {
                    div()
                        .id(("run-config-template", k))
                        .relative()
                        .h(px(24.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .when(k == at, |d| d.bg(c(p.tree_selection)))
                        .when(k != at, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .child(t.label())
                        .semantics(
                            control(
                                Role::ListItem,
                                format!("run-config-new-{}", t.key()),
                                t.label(),
                            )
                            .selected(k == at),
                        )
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                            this.add(t, window, cx)
                        }))
                }))
        });
        let button = |id: &'static str, label: &'static str, primary: bool| {
            crate::metrics::button(id, label, primary)
                .tip_text(id, ferris_idea_ui::tooltip::Tip::default())
        };
        let footer = div()
            .flex()
            .items_center()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(c(p.separator))
            .child(
                button("run-config-run", "Run", false)
                    .when(locked, |d| d.opacity(0.4))
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        trace("click run-config-run: start");
                        this.start(false, cx)
                    })),
            )
            .child(
                button("run-config-debug", "Debug", false)
                    .when(locked, |d| d.opacity(0.4))
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        trace("click run-config-debug: start");
                        this.start(true, cx)
                    })),
            )
            .child(div().flex_1())
            .child(
                button("run-config-cancel", "Cancel", false).on_click(cx.listener(
                    |this, _: &gpui::ClickEvent, _, cx| {
                        trace("click run-config-cancel: cancel");
                        this.cancel(cx)
                    },
                )),
            )
            .child(
                button("run-config-apply", "Apply", false)
                    .when(locked, |d| d.opacity(0.4))
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.apply(cx);
                    })),
            )
            .child(button("run-config-ok", "OK", true).on_click(cx.listener(
                |this, _: &gpui::ClickEvent, _, cx| {
                    trace("click run-config-ok: ok");
                    this.ok(cx)
                },
            )));
        div()
            .id("run-config-dialog")
            // The dialog itself holds the list's focus, so it never points at nothing.
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_capture_key))
            .relative()
            .w(px(940.))
            .min_h(px(least_height))
            .max_h(px(most_height))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(8.))
            .border_1()
            .border_color(c(p.popup_border))
            .bg(c(p.popup))
            .text_color(c(p.ui_text))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Run/Debug Configurations"),
            )
            .children(self.read_only.clone().map(|why| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c(p.separator))
                    .child(icon("status/warning"))
                    .child(div().flex_1().whitespace_normal().child(why))
            }))
            // The part between the title and the buttons takes the height its rows need; it gives
            // way, and the form scrolls, only when the dialog has reached the window's edge.
            .child(
                div()
                    .flex_auto()
                    .min_h_0()
                    .flex()
                    .gap_3()
                    .child(left)
                    .child(right),
            )
            .children((!self.problems.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .text_color(c(p.error_text))
                    .children(
                        self.problems
                            .iter()
                            .map(|q| div().whitespace_normal().child(q.clone())),
                    )
            }))
            .child(footer)
            .children(templates)
    }
}

/// The problems of `field` as lines under it.
fn under(problems: &[(RunField, String)], field: Option<RunField>) -> Vec<gpui::AnyElement> {
    let p = Palette::current();
    problems
        .iter()
        .filter(|(f, _)| Some(*f) == field)
        .map(|(_, text)| {
            div()
                .text_xs()
                .text_color(c(p.error_text))
                .child(text.clone())
                .into_any_element()
        })
        .collect()
}

impl RunConfigDialog {
    /// The list of configurations with its folders and headings (behaviour 2, v3 behaviour 15).
    fn list_rows(
        &mut self,
        selected: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        let folder_shown = self.model.selected_folder.clone();
        let mut items: Vec<gpui::AnyElement> = Vec::new();
        for (k, row) in self.model.rows().into_iter().enumerate() {
            match row {
                Row::Heading(h) => items.push(
                    div()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(h)
                        .into_any_element(),
                ),
                Row::Folder(name) => {
                    let on = folder_shown.as_deref() == Some(name.as_str());
                    let members = self.model.folder_members(&name).len();
                    let pick = name.clone();
                    items.push(
                        div()
                            .id(("run-config-folder", k))
                            .relative()
                            .h(px(24.))
                            .mt_1()
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(4.))
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(icon("nodes/folder"))
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(name.clone()),
                            )
                            .when(members == 0, |d| {
                                d.child(div().text_xs().text_color(c(p.ui_dim)).child("empty"))
                            })
                            .semantics(
                                control(
                                    Role::ListItem,
                                    format!("run-config-folder-{name}"),
                                    format!("Folder {name}"),
                                )
                                .selected(on),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                                this.select_folder(&pick, cx);
                                window.focus(&this.focus);
                            }))
                            .into_any_element(),
                    );
                }
                Row::Entry(i) => {
                    let e = &self.model.entries[i];
                    let problems = !self.model.problems(i).is_empty();
                    let greyed = e.source == Source::Detected;
                    let in_folder = self.model.folder_of(i).is_some();
                    let on = selected == Some(i) && folder_shown.is_none();
                    let name = e.name().to_owned();
                    let label = if e.changed() && e.source == Source::Project {
                        format!("{name} •")
                    } else {
                        name.clone()
                    };
                    items.push(
                        div()
                            .id(("run-config-row", i))
                            .relative()
                            .h(px(24.))
                            .px_2()
                            .when(in_folder, |d| d.pl(px(24.)))
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(4.))
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .when(greyed, |d| d.text_color(c(p.ui_muted)))
                            .child(icon("toolwindows/run"))
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(label),
                            )
                            .when(problems, |d| d.child(icon("status/error")))
                            .semantics(
                                control(Role::ListItem, format!("run-config-row-{name}"), name)
                                    .selected(on),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                                this.select(i, cx);
                                window.focus(&this.focus);
                            }))
                            .into_any_element(),
                    );
                }
            }
        }
        items
    }

    /// The types, each with the state of its template (v3 behaviour 16).
    fn template_rows(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        RunTemplate::ALL
            .into_iter()
            .enumerate()
            .map(|(k, kind)| {
                let on = self.model.template_selected == kind;
                let set = self.model.template_set(kind);
                let problems = !self.model.template_problems(kind).is_empty();
                let label = if self.model.template_changed(kind) {
                    format!("{} •", kind.label())
                } else {
                    kind.label().to_owned()
                };
                div()
                    .id(("run-config-type", k))
                    .relative()
                    .h(px(24.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(4.))
                    .when(on, |d| d.bg(c(p.tree_selection)))
                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .when(!set, |d| d.text_color(c(p.ui_muted)))
                    .child(icon("toolwindows/run"))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(label),
                    )
                    .when(problems, |d| d.child(icon("status/error")))
                    .semantics(
                        control(
                            Role::ListItem,
                            format!("run-config-type-{}", kind.key()),
                            format!("Template {}", kind.label()),
                        )
                        .selected(on),
                    )
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        this.select_template(kind, cx);
                        window.focus(&this.focus);
                    }))
                    .into_any_element()
            })
            .collect()
    }

    /// One labelled text field with its problems and its hint.
    fn field_el(
        &self,
        k: usize,
        hint: &str,
        tall: bool,
        locked: bool,
        problems: &[(RunField, String)],
    ) -> gpui::Div {
        let p = Palette::current();
        let frame = crate::metrics::field_frame().child(self.fields[k].clone());
        let frame = if tall {
            frame.h(px(68.)).items_start().py_1()
        } else {
            frame
        };
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(div().text_color(c(p.ui_muted)).child(FIELDS[k]))
            .child(frame.when(locked, |d| d.opacity(0.6)))
            .children(under(problems, field_of(k)))
            .when(!hint.is_empty(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(c(p.ui_dim))
                        .child(hint.to_owned()),
                )
            })
    }

    /// Before launch: the ordered list of configurations to run first with its picker, then the
    /// tasks field (behaviour 4, v2 behaviour 13). Used by a configuration's form and by a
    /// type's template.
    fn before_box(
        &mut self,
        before: Vec<String>,
        candidates: Vec<String>,
        locked: bool,
        problems: &[(RunField, String)],
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = Palette::current();
        let n = before.len();
        let before_rows = before.into_iter().enumerate().map(|(k, name)| {
            let small = |id: (&'static str, usize), key: &'static str| {
                div()
                    .id(id)
                    .rounded(px(3.))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(icon(key))
            };
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().flex_1().child(name))
                .child(
                    small(("run-config-before-up", k), "general/up").on_click(cx.listener(
                        move |this, _: &gpui::ClickEvent, _, cx| {
                            this.edit_before(cx, |b| {
                                if k > 0 && k < b.len() {
                                    b.swap(k, k - 1);
                                }
                            })
                        },
                    )),
                )
                .child(
                    small(("run-config-before-down", k), "general/down").on_click(cx.listener(
                        move |this, _: &gpui::ClickEvent, _, cx| {
                            this.edit_before(cx, |b| {
                                if k + 1 < n && k + 1 < b.len() {
                                    b.swap(k, k + 1);
                                }
                            })
                        },
                    )),
                )
                .child(
                    small(("run-config-before-remove", k), "general/closeSmall").on_click(
                        cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.edit_before(cx, |b| {
                                if k < b.len() {
                                    b.remove(k);
                                }
                            })
                        }),
                    ),
                )
        });
        let picker = self.before_pick.then(|| {
            div()
                .flex()
                .flex_col()
                .p_1()
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .children(
                    [
                        ("Build", "build", "build"),
                        ("Run external tool…", "tool ", "tool"),
                        ("Open browser…", "browser http://localhost:", "browser"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(k, (label, line, key))| {
                        div()
                            .id(("run-config-before-task", k))
                            .relative()
                            .px_1()
                            .rounded(px(3.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(label)
                            .semantics(control(
                                Role::ListItem,
                                format!("run-config-task-{key}"),
                                label,
                            ))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                                this.add_task_line(line, window, cx)
                            }))
                    }),
                )
                .children(candidates.into_iter().enumerate().map(|(k, name)| {
                    let add = name.clone();
                    div()
                        .id(("run-config-before-pick", k))
                        .relative()
                        .px_1()
                        .rounded(px(3.))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(name.clone())
                        .semantics(control(
                            Role::ListItem,
                            format!("run-config-before-{name}"),
                            name,
                        ))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.add_before(&add, cx)
                        }))
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_color(c(p.ui_muted)).child("Before launch"))
                    .child(
                        div()
                            .id("run-config-before-add")
                            .relative()
                            .px_1()
                            .rounded(px(3.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(icon("general/add"))
                            .semantics(control(
                                Role::Button,
                                "run-config-before-add",
                                "Add before-launch task",
                            ))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                if this.read_only.is_none() {
                                    this.before_pick = !this.before_pick;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .children(before_rows)
            .children(picker)
            .child(self.field_el(
                TASKS,
                "one per line, run in this order after the configurations above: build · tool [cwd=folder] command line · browser [wait=port] URL (opened after the start; with wait= once that local port answers)",
                true,
                locked,
                problems,
            ))
            .children(under(problems, Some(RunField::Before)))
    }

    /// The switch between one instance and several (v2 behaviour 12).
    fn instances_row(&self, multiple: bool, cx: &mut Context<Self>) -> gpui::Div {
        let p = Palette::current();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().text_color(c(p.ui_muted)).child("Instances"))
            .child(
                crate::metrics::button("run-config-single-instance", "One at a time", !multiple)
                    .relative()
                    .semantics(
                        control(Role::Button, "run-config-single-instance", "One at a time")
                            .selected(!multiple),
                    )
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.set_allow_multiple(false, cx)
                    })),
            )
            .child(
                crate::metrics::button(
                    "run-config-allow-multiple",
                    "Allow multiple instances",
                    multiple,
                )
                .relative()
                .semantics(
                    control(
                        Role::Button,
                        "run-config-allow-multiple",
                        "Allow multiple instances",
                    )
                    .selected(multiple),
                )
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    this.set_allow_multiple(true, cx)
                })),
            )
    }

    /// The names of the configurations of `run.toml` that can run before something, without
    /// `skip` and those already listed.
    fn before_candidates(&self, skip: Option<usize>, before: &[String]) -> Vec<String> {
        self.model
            .entries
            .iter()
            .enumerate()
            .filter(|(j, e)| Some(*j) != skip && e.source == Source::Project)
            .map(|(_, e)| e.name().to_owned())
            .filter(|name| !before.contains(name))
            .collect()
    }

    /// The pane of a folder (v3 behaviour 15): its name to change, its configurations, Remove.
    fn folder_pane(&mut self, folder: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let locked = self.read_only.is_some();
        let typed = self.text(FOLDER_NAME, cx);
        let problem = (typed != folder)
            .then(|| self.model.rename_problem(folder, &typed))
            .flatten();
        let members = self.model.folder_members(folder);
        let about = if members.is_empty() {
            "No configuration is in this folder yet. It is written to run.toml with its first \
             configuration: select a configuration and choose the folder under Folder."
                .to_owned()
        } else {
            format!("In this folder: {}", members.join(", "))
        };
        let can_rename = !locked && typed != folder && problem.is_none();
        div()
            .id("run-config-folder-pane")
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("Folder {folder}")),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_2()
                    .child(self.field_el(FOLDER_NAME, "", false, locked, &[]).flex_1())
                    .child(
                        crate::metrics::button("run-config-folder-rename", "Rename", false)
                            .relative()
                            .when(!can_rename, |d| d.opacity(0.4))
                            .semantics(
                                control(Role::Button, "run-config-folder-rename", "Rename")
                                    .enabled(can_rename),
                            )
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.rename_folder(cx);
                            })),
                    ),
            )
            .children(problem.map(|q| div().text_xs().text_color(c(p.error_text)).child(q)))
            .child(
                div()
                    .whitespace_normal()
                    .text_color(c(p.ui_muted))
                    .child(about),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        crate::metrics::button("run-config-folder-remove", "Remove Folder", false)
                            .relative()
                            .when(locked, |d| d.opacity(0.4))
                            .semantics(
                                control(Role::Button, "run-config-folder-remove", "Remove Folder")
                                    .enabled(!locked),
                            )
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.remove_folder(cx);
                                window.focus(&this.focus);
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .whitespace_normal()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child("Its configurations stay; they are listed without a folder."),
                    ),
            )
            .into_any_element()
    }

    /// The template of the selected type (v3 behaviour 16): what a new configuration of the
    /// type starts from.
    fn template_pane(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let kind = self.model.template_selected;
        let form = self.model.template_form(kind);
        let problems = self.model.template_problems(kind);
        let locked = self.read_only.is_some();
        let program = kind.command().first().cloned();
        let candidates = self.before_candidates(None, &form.before);
        let before_box = self.before_box(form.before.clone(), candidates, locked, &problems, cx);
        let instances = self.instances_row(form.allow_multiple, cx);
        let reset = self.model.template_set(kind) && !locked;
        div()
            .id("run-config-template-pane")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.form_scroll)
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(format!("Template: {}", kind.label())),
                    )
                    .child(
                        crate::metrics::button("run-config-template-reset", "Reset", false)
                            .relative()
                            .when(!reset, |d| d.opacity(0.4))
                            .semantics(
                                control(Role::Button, "run-config-template-reset", "Reset")
                                    .enabled(reset),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.reset_template(kind, cx)
                            })),
                    ),
            )
            .child(div().whitespace_normal().text_color(c(p.ui_muted)).child(
                "A new configuration of this type starts as a copy of this template. \
                         Configurations that exist are not changed. Stored in .ferris-idea/run.toml.",
            ))
            .when_some(program, |d, program| {
                d.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_3()
                        .child(
                            self.field_el(
                                INTERPRETER,
                                &format!("in place of `{program}`; empty for `{program}`"),
                                false,
                                locked,
                                &problems,
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(
                            self.field_el(
                                OPTIONS,
                                "placed right after the program, quoted as in a shell",
                                false,
                                locked,
                                &problems,
                            )
                            .flex_1()
                            .min_w_0(),
                        ),
                )
            })
            .when(kind.command().is_empty(), |d| {
                d.children(under(&problems, Some(RunField::Command)))
            })
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(self.field_el(
                                CWD,
                                "relative to the workspace; ${workspace} is allowed",
                                false,
                                locked,
                                &problems,
                            ))
                            .child(self.field_el(
                                ENV,
                                "one NAME=value per line",
                                true,
                                locked,
                                &problems,
                            ))
                            .child(self.field_el(
                                TIMEOUT,
                                "empty for 600",
                                false,
                                locked,
                                &problems,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(instances)
                            .child(before_box),
                    ),
            )
            .into_any_element()
    }

    /// Reset: the template of `kind` says nothing again.
    pub fn reset_template(&mut self, kind: RunTemplate, cx: &mut Context<Self>) {
        if self.read_only.is_some() {
            return;
        }
        self.sync_form(cx);
        self.model.edit_template(kind, |f| {
            *f = crate::run_editor_model::TemplateForm::default()
        });
        self.load_form(cx);
        cx.notify();
    }

    /// The form of the selected configuration with its problems under the fields (behaviour 4).
    fn form_pane(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let Some(i) = self.model.selected else {
            return div()
                .flex_1()
                .text_color(c(p.ui_muted))
                .child(keys("Select a configuration, or add one with ⌘N"))
                .into_any_element();
        };
        let entry = self.model.entries[i].clone();
        let problems = self.model.problems(i);
        let locked = self.read_only.is_some() || entry.source == Source::Detected;
        let test = entry.form.test;
        let kind = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().text_color(c(p.ui_muted)).child("Kind"))
            .child(
                crate::metrics::button("run-config-kind-run", "Run", !test).on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.set_test(false, cx)),
                ),
            )
            .child(
                crate::metrics::button("run-config-kind-test", "Test", test).on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.set_test(true, cx)),
                ),
            );
        let candidates = self.before_candidates(Some(i), &entry.form.before);
        let before_box =
            self.before_box(entry.form.before.clone(), candidates, locked, &problems, cx);
        let instances = self.instances_row(entry.form.allow_multiple, cx);
        // The folders to choose from (v3 behaviour 15): a click puts the configuration into the
        // folder or takes it out; typing a new name in the field makes a folder.
        let current = self.model.folder_of(i);
        let folders = self.model.folders();
        let chips =
            (entry.source == Source::Project && !locked && !folders.is_empty()).then(|| {
                let chip = |id: (&'static str, usize), label: String, test: String, on: bool| {
                    div()
                        .id(id)
                        .relative()
                        .px_2()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(c(if on { p.focus } else { p.separator }))
                        .when(on, |d| d.bg(c(p.tree_selection)))
                        .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .text_xs()
                        .child(label.clone())
                        .semantics(control(Role::Button, test, label).selected(on))
                };
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(
                        chip(
                            ("run-config-move-out", 0),
                            "No folder".into(),
                            "run-config-move-out".into(),
                            current.is_none(),
                        )
                        .on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, _, cx| this.move_to_folder(None, cx),
                        )),
                    )
                    .children(folders.into_iter().enumerate().map(|(k, name)| {
                        let to = name.clone();
                        chip(
                            ("run-config-move-to", k),
                            name.clone(),
                            format!("run-config-move-to-{name}"),
                            current.as_deref() == Some(name.as_str()),
                        )
                        .on_click(cx.listener(
                            move |this, _: &gpui::ClickEvent, _, cx| {
                                this.move_to_folder(Some(&to), cx)
                            },
                        ))
                    }))
            });
        let temporary = entry.source == Source::Temporary;
        div()
            .id("run-config-form")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.form_scroll)
            .flex()
            .flex_col()
            .gap_2()
            .when(entry.source == Source::Detected, |d| {
                d.child(div().text_color(c(p.ui_muted)).child(keys(
                    "Detected from the build files: Copy (⌘D) makes an editable configuration.",
                )))
            })
            .when(temporary, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_color(c(p.ui_muted))
                                .child("Temporary: kept for this session only."),
                        )
                        .child(
                            crate::metrics::button(
                                "run-config-save-temporary",
                                "Save Configuration",
                                false,
                            )
                            .on_click(cx.listener(
                                |this, _: &gpui::ClickEvent, _, cx| this.save_temporary(cx),
                            )),
                        ),
                )
            })
            .child(self.field_el(NAME, "", false, locked, &problems))
            .child(self.field_el(
                FOLDER,
                "groups the configuration in the lists; empty for none",
                false,
                locked,
                &problems,
            ))
            .children(chips)
            .child(self.field_el(
                COMMAND,
                "the program and its arguments, quoted as in a shell: node main.js \"two words\"",
                false,
                locked,
                &problems,
            ))
            // Below the command line two columns, so that the whole form is in view at once:
            // where and with what it runs on the left, what it is and what comes before it on
            // the right.
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(self.field_el(
                                CWD,
                                "relative to the workspace; ${workspace} is allowed",
                                false,
                                locked,
                                &problems,
                            ))
                            .child(self.field_el(
                                ENV,
                                "one NAME=value per line",
                                true,
                                locked,
                                &problems,
                            ))
                            .when(test, |d| {
                                d.child(self.field_el(
                                    REPORT,
                                    "libtest, tap or junit",
                                    false,
                                    locked,
                                    &problems,
                                ))
                                .child(self.field_el(
                                    REPORT_FILE,
                                    "JUnit file written by the tool, relative to the workspace",
                                    false,
                                    locked,
                                    &problems,
                                ))
                            })
                            .child(self.field_el(
                                TIMEOUT,
                                "empty for 600",
                                false,
                                locked,
                                &problems,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(kind)
                            .child(instances)
                            .child(before_box),
                    ),
            )
            .children(under(&problems, Some(RunField::Url)))
            .into_any_element()
    }
}

/// A shortcut of this dialog written for the platform: ⌘ is Ctrl and ⌥ is Alt outside macOS
/// (`SPEC-0028` v8 behaviour 13).
fn keys(text: &str) -> String {
    ferris_idea_ui::keymap::local_keys(text, cfg!(target_os = "macos"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0327 v4 AK-13: the dialog is 580 px high where the window has the room, may grow to
    /// 16 px above the window's edge, and in a low window is as high as fits — the form then
    /// scrolls and the buttons stay in view.
    #[test]
    fn the_dialog_is_as_high_as_the_window_allows() {
        // The window of the check, 885 px high: 80 px above the dialog, 805 px of room.
        assert_eq!(dialog_heights(805.), (580., 789.));
        // Exactly the room for the usual height.
        assert_eq!(dialog_heights(596.), (580., 580.));
        // A low window: least and most height are the room, so the dialog ends in the window.
        assert_eq!(dialog_heights(480.), (464., 464.));
        // Never lower than its least height, and the least is never more than the most.
        assert_eq!(dialog_heights(100.), (240., 240.));
        for room in (0..1400).step_by(20) {
            let (least, most) = dialog_heights(room as f32);
            assert!(
                least <= most && least <= DIALOG_HEIGHT && most >= 240.,
                "{room}"
            );
        }
    }
}
