//! The run configuration editor's model (`SPEC-0327`), free of the UI toolkit: the form of one
//! configuration as text, the list with its sections and folders, Add/Remove/Copy/Move, the
//! folders' own operations, the templates per type, what Apply writes, and a reload when
//! `run.toml` changes on disk.

use std::collections::BTreeMap;

use ferris_idea_core::{
    BeforeTask, RunConfig, RunEntry, RunField, RunKind, RunTemplate, TypeTemplate, config_problems,
    folder_name_problem, free_name, join_command_line, split_command_line, template_problems,
};

/// The environment a form's text describes, one `NAME=value` per line.
fn parse_env(text: &str, problems: &mut Vec<(RunField, String)>) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match line.split_once('=') {
            Some((k, v)) => {
                env.insert(k.trim().to_owned(), v.to_owned());
            }
            None => problems.push((RunField::Env, format!("line {}: write NAME=value", i + 1))),
        }
    }
    env
}

fn env_text(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_timeout(text: &str, problems: &mut Vec<(RunField, String)>) -> Option<u64> {
    match text.trim() {
        "" => None,
        t => match t.parse::<u64>() {
            Ok(n) => Some(n),
            Err(_) => {
                problems.push((
                    RunField::Timeout,
                    "the timeout is a number of seconds".into(),
                ));
                None
            }
        },
    }
}

/// The before-launch tasks a form's text describes, one per line.
fn parse_tasks(text: &str, problems: &mut Vec<(RunField, String)>) -> Vec<BeforeTask> {
    let mut tasks = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match BeforeTask::parse_line(line) {
            Ok(t) => tasks.push(t),
            Err(e) => problems.push((RunField::Before, format!("task line {}: {e}", i + 1))),
        }
    }
    tasks
}

fn tasks_text(tasks: &[BeforeTask]) -> String {
    tasks
        .iter()
        .map(BeforeTask::line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The template of a type as typed (v3 behaviour 16): what a new configuration of the type
/// starts from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TemplateForm {
    /// The program in place of the type's own; empty for the type's.
    pub interpreter: String,
    /// Options for that program, shell-quoted.
    pub options: String,
    /// Working directory.
    pub cwd: String,
    /// Environment, one `NAME=value` per line.
    pub env: String,
    /// Configurations to run first, in order.
    pub before: Vec<String>,
    /// Timeout in seconds.
    pub timeout: String,
    /// Several instances may run at once.
    pub allow_multiple: bool,
    /// Before-launch tasks, one per line.
    pub tasks: String,
}

impl TemplateForm {
    /// The form showing `t`.
    pub fn of(t: &TypeTemplate) -> Self {
        Self {
            interpreter: t.interpreter.clone().unwrap_or_default(),
            options: join_command_line(&t.options),
            cwd: t.cwd.clone().unwrap_or_default(),
            env: env_text(&t.env),
            before: t.before.clone(),
            timeout: t.timeout.map(|t| t.to_string()).unwrap_or_default(),
            allow_multiple: t.allow_multiple,
            tasks: tasks_text(&t.before_launch),
        }
    }

    /// The template the form describes, or the problems of reading its text.
    pub fn template(&self) -> Result<TypeTemplate, Vec<(RunField, String)>> {
        let mut problems = Vec::new();
        let options = split_command_line(&self.options).unwrap_or_else(|e| {
            problems.push((RunField::Command, e));
            Vec::new()
        });
        let env = parse_env(&self.env, &mut problems);
        let timeout = parse_timeout(&self.timeout, &mut problems);
        let before_launch = parse_tasks(&self.tasks, &mut problems);
        if !problems.is_empty() {
            return Err(problems);
        }
        let text = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        Ok(TypeTemplate {
            interpreter: text(&self.interpreter),
            options,
            cwd: text(&self.cwd),
            env,
            before: self.before.clone(),
            before_launch,
            allow_multiple: self.allow_multiple,
            timeout,
        })
    }
}

/// One line of the editor's list (v3 behaviour 15): the configurations of `run.toml` without a
/// folder, each folder with its configurations, then the temporary and the detected ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A folder of `run.toml`, or one made in this dialog that has no configuration yet.
    Folder(String),
    /// The heading of the temporary or the detected configurations.
    Heading(&'static str),
    /// Entry `i`.
    Entry(usize),
}

/// The form of one configuration as typed (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunForm {
    /// Name.
    pub name: String,
    /// Command line, shell-quoted.
    pub command: String,
    /// Working directory.
    pub cwd: String,
    /// Environment, one `NAME=value` per line.
    pub env: String,
    /// Test instead of run.
    pub test: bool,
    /// Report format of a test configuration.
    pub report: String,
    /// Report file of a test configuration.
    pub report_file: String,
    /// Configurations to run first, in order.
    pub before: Vec<String>,
    /// Timeout in seconds.
    pub timeout: String,
    /// A page to open (kept as it is; the form does not show it).
    pub url: Option<String>,
    /// The folder it is listed in; empty for none (v2 behaviour 11).
    pub folder: String,
    /// Several instances may run at once (v2 behaviour 12).
    pub allow_multiple: bool,
    /// Before-launch tasks other than configurations, one per line: `build`,
    /// `tool [cwd=<folder>] <command line>`, `browser <URL>` (v2 behaviour 13).
    pub tasks: String,
    /// The requests an HTTP request configuration sends (kept as they are; the form does not show
    /// them, `SPEC-0047` v6 behaviour 17).
    pub http: Option<ferris_idea_core::HttpRun>,
}

impl RunForm {
    /// The form showing `c`.
    pub fn of(c: &RunConfig) -> Self {
        Self {
            name: c.name.clone(),
            command: join_command_line(&c.command),
            cwd: c.cwd.clone().unwrap_or_default(),
            env: env_text(&c.env),
            test: c.kind == RunKind::Test,
            report: c.report.clone().unwrap_or_default(),
            report_file: c.report_file.clone().unwrap_or_default(),
            before: c.before.clone(),
            timeout: c.timeout.map(|t| t.to_string()).unwrap_or_default(),
            url: c.url.clone(),
            folder: c.folder.clone().unwrap_or_default(),
            allow_multiple: c.allow_multiple,
            tasks: tasks_text(&c.before_launch),
            http: c.http.clone(),
        }
    }

    /// The configuration the form describes, or the problems of reading the text (an open
    /// quote, an environment line without `=`, a timeout that is not a number).
    pub fn config(&self) -> Result<RunConfig, Vec<(RunField, String)>> {
        let mut problems = Vec::new();
        let command = split_command_line(&self.command).unwrap_or_else(|e| {
            problems.push((RunField::Command, e));
            Vec::new()
        });
        let env = parse_env(&self.env, &mut problems);
        let timeout = parse_timeout(&self.timeout, &mut problems);
        let before_launch = parse_tasks(&self.tasks, &mut problems);
        let text = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(RunConfig {
            name: self.name.trim().to_owned(),
            command,
            cwd: text(&self.cwd),
            env,
            kind: if self.test {
                RunKind::Test
            } else {
                RunKind::Run
            },
            report: text(&self.report).filter(|_| self.test),
            report_file: text(&self.report_file).filter(|_| self.test),
            before: self.before.clone(),
            timeout,
            url: self.url.clone(),
            folder: text(&self.folder),
            allow_multiple: self.allow_multiple,
            before_launch,
            verbatim: false,
            http: self.http.clone(),
        })
    }

    /// The problems of the form among `others` (behaviour 4), each under its field; problems
    /// about the name prefix are left out, the form shows them under the field.
    pub fn problems(&self, others: &[RunConfig]) -> Vec<(RunField, String)> {
        match self.config() {
            Err(p) => p,
            Ok(c) => config_problems(&c, others)
                .into_iter()
                .map(|(f, p)| {
                    let prefix = format!("{}: ", c.name);
                    (f, p.strip_prefix(&prefix).map(str::to_owned).unwrap_or(p))
                })
                .collect(),
        }
    }
}

/// Where an entry of the editor's list comes from (behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `run.toml`, or new and to be written there.
    Project,
    /// A temporary configuration of this session (behaviour 9).
    Temporary,
    /// Detected from the build files: shown greyed, not editable.
    Detected,
}

/// One entry of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name in `run.toml`, if it is there.
    pub original: Option<String>,
    /// Where it comes from.
    pub source: Source,
    /// The configuration as loaded.
    pub base: RunConfig,
    /// The form as edited.
    pub form: RunForm,
}

impl Entry {
    fn new(source: Source, c: RunConfig, original: bool) -> Self {
        Self {
            original: original.then(|| c.name.clone()),
            source,
            form: RunForm::of(&c),
            base: c,
        }
    }

    /// Whether it differs from what was loaded (a new entry always does).
    pub fn changed(&self) -> bool {
        (self.source == Source::Project && self.original.is_none())
            || self.form != RunForm::of(&self.base)
    }

    /// The name the list shows.
    pub fn name(&self) -> &str {
        &self.form.name
    }
}

/// The editor's list and selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorModel {
    /// Entries: those of `run.toml`, then the temporary ones, then the detected ones.
    pub entries: Vec<Entry>,
    /// The selected entry.
    pub selected: Option<usize>,
    /// The typing filter of the list.
    pub filter: String,
    /// Something was removed or moved since the last write.
    pub structure_changed: bool,
    /// Folders made in this dialog that no configuration names yet (v3 behaviour 15); they are
    /// not written and end with the dialog.
    pub empty_folders: Vec<String>,
    /// The folder whose pane is shown instead of a configuration's form.
    pub selected_folder: Option<String>,
    /// The templates per type as edited (v3 behaviour 16); a type without a form has the
    /// template it was loaded with.
    pub templates: BTreeMap<RunTemplate, TemplateForm>,
    /// The templates as loaded from `run.toml`.
    pub templates_base: BTreeMap<RunTemplate, TypeTemplate>,
    /// The list shows the types and the form a type's template.
    pub template_mode: bool,
    /// The type whose template is shown.
    pub template_selected: RunTemplate,
}

impl EditorModel {
    /// The list of `project` (from `run.toml`), `temporary` and `detected` configurations, with
    /// `select` selected (else the first).
    pub fn new(
        project: Vec<RunConfig>,
        temporary: Vec<RunConfig>,
        detected: Vec<RunConfig>,
        select: Option<&str>,
    ) -> Self {
        let mut entries: Vec<Entry> = project
            .into_iter()
            .map(|c| Entry::new(Source::Project, c, true))
            .collect();
        entries.extend(
            temporary
                .into_iter()
                .map(|c| Entry::new(Source::Temporary, c, false)),
        );
        entries.extend(
            detected
                .into_iter()
                .map(|c| Entry::new(Source::Detected, c, false)),
        );
        let selected = select
            .and_then(|n| entries.iter().position(|e| e.name() == n))
            .or((!entries.is_empty()).then_some(0));
        Self {
            entries,
            selected,
            filter: String::new(),
            structure_changed: false,
            empty_folders: Vec::new(),
            selected_folder: None,
            templates: BTreeMap::new(),
            templates_base: BTreeMap::new(),
            template_mode: false,
            template_selected: RunTemplate::Command,
        }
    }

    /// With the templates per type of `run.toml`.
    pub fn with_templates(mut self, templates: BTreeMap<RunTemplate, TypeTemplate>) -> Self {
        self.templates_base = templates;
        self
    }

    // -------------------------------------------------------------------------------------
    // Templates per type (v3 behaviour 16).

    /// The template of `kind` as loaded.
    fn template_base(&self, kind: RunTemplate) -> TypeTemplate {
        self.templates_base.get(&kind).cloned().unwrap_or_default()
    }

    /// The form of the template of `kind`.
    pub fn template_form(&self, kind: RunTemplate) -> TemplateForm {
        self.templates
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| TemplateForm::of(&self.template_base(kind)))
    }

    /// Changes the form of the template of `kind`.
    pub fn edit_template(&mut self, kind: RunTemplate, change: impl FnOnce(&mut TemplateForm)) {
        let mut form = self.template_form(kind);
        change(&mut form);
        self.templates.insert(kind, form);
    }

    /// Whether the template of `kind` differs from what was loaded.
    pub fn template_changed(&self, kind: RunTemplate) -> bool {
        self.templates
            .get(&kind)
            .is_some_and(|f| *f != TemplateForm::of(&self.template_base(kind)))
    }

    /// Whether the template of `kind` says anything.
    pub fn template_set(&self, kind: RunTemplate) -> bool {
        self.template_form(kind) != TemplateForm::default()
    }

    /// The configurations of `run.toml` as edited (those whose form reads).
    fn project_configs(&self) -> Vec<RunConfig> {
        self.entries
            .iter()
            .filter(|e| e.source == Source::Project)
            .filter_map(|e| e.form.config().ok())
            .collect()
    }

    /// Problems of the template of `kind`, each under its field, without the template's name
    /// in front.
    pub fn template_problems(&self, kind: RunTemplate) -> Vec<(RunField, String)> {
        match self.template_form(kind).template() {
            Err(p) => p,
            Ok(t) => {
                let prefix = format!("template {}: ", kind.key());
                template_problems(kind, &t, &self.project_configs())
                    .into_iter()
                    .map(|(f, p)| (f, p.strip_prefix(&prefix).map(str::to_owned).unwrap_or(p)))
                    .collect()
            }
        }
    }

    /// What Apply writes as templates: the template of every type that was changed (one that
    /// now says nothing removes the type's table), or the problems with the name of their type.
    /// A type that was not changed is not named, so its table stays as it is in the file.
    pub fn templates_to_write(&self) -> Result<BTreeMap<RunTemplate, TypeTemplate>, Vec<String>> {
        let mut out = BTreeMap::new();
        let mut problems = Vec::new();
        for kind in RunTemplate::ALL {
            if !self.template_changed(kind) {
                continue;
            }
            for (_, p) in self.template_problems(kind) {
                problems.push(format!("Template {}: {p}", kind.label()));
            }
            if let Ok(t) = self.template_form(kind).template() {
                out.insert(kind, t);
            }
        }
        if problems.is_empty() {
            Ok(out)
        } else {
            Err(problems)
        }
    }

    /// `run.toml` changed on disk: templates without changes take the new ones, a changed one
    /// stays as edited.
    pub fn reload_templates(&mut self, templates: BTreeMap<RunTemplate, TypeTemplate>) {
        let changed: Vec<RunTemplate> = RunTemplate::ALL
            .into_iter()
            .filter(|k| self.template_changed(*k))
            .collect();
        self.templates.retain(|k, _| changed.contains(k));
        self.templates_base = templates;
    }

    // -------------------------------------------------------------------------------------
    // Folders (v3 behaviour 15).

    /// The lines of the list as shown: what the filter keeps, each folder above its
    /// configurations. Without a filter a folder without configurations is listed too.
    pub fn rows(&self) -> Vec<Row> {
        let visible = self.visible();
        let filtering = !self.filter.trim().is_empty();
        let end = self.project_end();
        let mut out: Vec<Row> = visible
            .iter()
            .copied()
            .filter(|&i| i < end && self.folder_of(i).is_none())
            .map(Row::Entry)
            .collect();
        for folder in self.folders() {
            let members: Vec<usize> = visible
                .iter()
                .copied()
                .filter(|&i| i < end && self.folder_of(i).as_deref() == Some(folder.as_str()))
                .collect();
            if members.is_empty() && filtering {
                continue;
            }
            out.push(Row::Folder(folder));
            out.extend(members.into_iter().map(Row::Entry));
        }
        for (source, heading) in [
            (Source::Temporary, crate::run_list_model::TEMPORARY),
            (Source::Detected, crate::run_list_model::DETECTED),
        ] {
            let listed: Vec<usize> = visible
                .iter()
                .copied()
                .filter(|&i| self.entries[i].source == source)
                .collect();
            if !listed.is_empty() {
                out.push(Row::Heading(heading));
                out.extend(listed.into_iter().map(Row::Entry));
            }
        }
        out
    }

    /// The names of the configurations in `folder`, in list order.
    pub fn folder_members(&self, folder: &str) -> Vec<String> {
        (0..self.project_end())
            .filter(|i| self.folder_of(*i).as_deref() == Some(folder))
            .map(|i| self.entries[i].name().to_owned())
            .collect()
    }

    /// New Folder: a folder with a free name, without configurations, selected. It is written
    /// once a configuration is put into it.
    pub fn new_folder(&mut self) -> String {
        let name = free_name("New Folder", &self.folders());
        self.empty_folders.push(name.clone());
        self.selected_folder = Some(name.clone());
        name
    }

    /// Why folder `old` cannot be renamed to `new`, if it cannot.
    pub fn rename_problem(&self, old: &str, new: &str) -> Option<String> {
        folder_name_problem(new).or_else(|| {
            (new != old && self.folders().iter().any(|f| f == new))
                .then(|| "a folder of that name exists already".to_owned())
        })
    }

    /// Renames folder `old` to `new`: its configurations name the new folder.
    pub fn rename_folder(&mut self, old: &str, new: &str) -> Result<(), String> {
        if !self.folders().iter().any(|f| f == old) {
            return Err(format!("no folder {old}"));
        }
        if let Some(p) = self.rename_problem(old, new) {
            return Err(p);
        }
        for i in 0..self.project_end() {
            if self.folder_of(i).as_deref() == Some(old) {
                self.entries[i].form.folder = new.to_owned();
            }
        }
        for f in &mut self.empty_folders {
            if f == old {
                *f = new.to_owned();
            }
        }
        if self.selected_folder.as_deref() == Some(old) {
            self.selected_folder = Some(new.to_owned());
        }
        Ok(())
    }

    /// Removes folder `name`: its configurations are listed on their own again.
    pub fn remove_folder(&mut self, name: &str) -> bool {
        if !self.folders().iter().any(|f| f == name) {
            return false;
        }
        for i in 0..self.project_end() {
            if self.folder_of(i).as_deref() == Some(name) {
                self.entries[i].form.folder = String::new();
            }
        }
        self.empty_folders.retain(|f| f != name);
        if self.selected_folder.as_deref() == Some(name) {
            self.selected_folder = None;
        }
        true
    }

    /// Puts entry `i` into `folder`, or takes it out of its folder.
    pub fn set_folder(&mut self, i: usize, folder: Option<&str>) -> bool {
        match self.entries.get_mut(i) {
            Some(e) if e.source == Source::Project => {
                e.form.folder = folder.unwrap_or_default().to_owned();
                true
            }
            _ => false,
        }
    }

    /// Indexes of the entries the filter keeps, in the order the list shows them (v2 behaviour
    /// 11): the configurations of `run.toml` without a folder in file order, then each folder in
    /// the order of its first configuration with its configurations in file order, then the
    /// temporary and the detected ones.
    pub fn visible(&self) -> Vec<usize> {
        let f = self.filter.trim().to_lowercase();
        let keeps = |i: &usize| f.is_empty() || self.entries[*i].name().to_lowercase().contains(&f);
        let end = self.project_end();
        let mut folders: Vec<Option<String>> = vec![None];
        for i in 0..end {
            let folder = self.folder_of(i);
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        let mut out: Vec<usize> = Vec::new();
        for folder in &folders {
            out.extend((0..end).filter(|&i| &self.folder_of(i) == folder));
        }
        out.extend(end..self.entries.len());
        out.into_iter().filter(keeps).collect()
    }

    /// The folder entry `i` is listed in: only configurations of `run.toml` have one.
    pub fn folder_of(&self, i: usize) -> Option<String> {
        let e = self.entries.get(i)?;
        let folder = e.form.folder.trim();
        (e.source == Source::Project && !folder.is_empty()).then(|| folder.to_owned())
    }

    /// The folders of the configurations of `run.toml`, in the order of their first one, then
    /// the folders made in this dialog that have no configuration yet.
    pub fn folders(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..self.project_end() {
            if let Some(f) = self.folder_of(i)
                && !out.contains(&f)
            {
                out.push(f);
            }
        }
        for f in &self.empty_folders {
            if !out.contains(f) {
                out.push(f.clone());
            }
        }
        out
    }

    /// The selected entry.
    pub fn current(&self) -> Option<&Entry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    /// Whether anything would be written or was changed (behaviour 5: Cancel asks first).
    pub fn dirty(&self) -> bool {
        self.structure_changed
            || self.entries.iter().any(Entry::changed)
            || RunTemplate::ALL
                .into_iter()
                .any(|k| self.template_changed(k))
    }

    fn names(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.name().to_owned()).collect()
    }

    fn project_end(&self) -> usize {
        self.entries
            .iter()
            .position(|e| e.source != Source::Project)
            .unwrap_or(self.entries.len())
    }

    /// Add with a type (behaviour 3): a new configuration at the end of those of `run.toml`,
    /// selected; it starts as a copy of the type's template.
    pub fn add(&mut self, template: RunTemplate) -> usize {
        // A copy of the type's template as it stands in this dialog (v3 behaviour 16).
        let from = self.template_form(template).template().ok();
        let c = template.config_from(&self.names(), from.as_ref());
        let at = self.project_end();
        self.entries.insert(
            at,
            Entry {
                original: None,
                source: Source::Project,
                form: RunForm::of(&c),
                base: c,
            },
        );
        self.selected = Some(at);
        at
    }

    /// Removes the selected entry (a detected one cannot be removed).
    pub fn remove_selected(&mut self) -> bool {
        let Some(i) = self.selected else {
            return false;
        };
        if self.entries[i].source == Source::Detected {
            return false;
        }
        self.entries.remove(i);
        self.structure_changed = true;
        self.selected = if self.entries.is_empty() {
            None
        } else {
            Some(i.min(self.entries.len() - 1))
        };
        true
    }

    /// Copies the selected entry into a new editable configuration with a free name, after those
    /// of `run.toml` (behaviour 2: Copy turns a detected one into an editable one); for a temporary
    /// one this is Save Configuration (behaviour 9).
    pub fn copy_selected(&mut self) -> Option<usize> {
        let i = self.selected?;
        let mut c = self.entries[i].form.config().ok()?;
        let source = self.entries[i].source;
        // A detected configuration's name does not count: a configuration of `run.toml` with
        // that name replaces the detected one (SPEC-0087 v2 behaviour 8).
        let taken: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.source != Source::Detected)
            .map(|e| e.name().to_owned())
            .collect();
        c.name = free_name(&c.name, &taken);
        let at = match source {
            Source::Project => i + 1,
            _ => self.project_end(),
        };
        self.entries.insert(
            at,
            Entry {
                original: None,
                source: Source::Project,
                form: RunForm::of(&c),
                base: c,
            },
        );
        self.selected = Some(at);
        Some(at)
    }

    /// Save Configuration of the selected temporary entry (behaviour 9): it becomes a
    /// configuration of `run.toml` with a free name. Returns the temporary name.
    pub fn save_temporary(&mut self) -> Option<String> {
        let i = self.selected?;
        if self.entries[i].source != Source::Temporary {
            return None;
        }
        let old = self.entries[i].base.name.clone();
        let mut e = self.entries.remove(i);
        let others: Vec<String> = self.names();
        e.form.name = free_name(e.form.name.trim(), &others);
        e.source = Source::Project;
        e.original = None;
        let at = self.project_end();
        self.entries.insert(at, e);
        self.selected = Some(at);
        Some(old)
    }

    /// Moves the selected configuration of `run.toml` up or down among those listed with it
    /// (behaviour 3; v2: within its folder, or among those without one). The file order changes
    /// with it.
    pub fn move_selected(&mut self, up: bool) -> bool {
        let Some(i) = self.selected else {
            return false;
        };
        if self.entries[i].source != Source::Project {
            return false;
        }
        let folder = self.folder_of(i);
        let end = self.project_end();
        let same = |j: &usize| self.folder_of(*j) == folder;
        let j = if up {
            (0..i).rev().find(same)
        } else {
            (i + 1..end).find(same)
        };
        let Some(j) = j else {
            return false;
        };
        self.entries.swap(i, j);
        self.selected = Some(j);
        self.structure_changed = true;
        true
    }

    /// The configurations other than entry `i` (for the problems of its form).
    pub fn others(&self, i: usize) -> Vec<RunConfig> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(j, e)| *j != i && e.source == Source::Project)
            .filter_map(|(_, e)| e.form.config().ok())
            .collect()
    }

    /// Problems of the entry `i`'s form.
    pub fn problems(&self, i: usize) -> Vec<(RunField, String)> {
        match self.entries.get(i) {
            Some(e) if e.source == Source::Project => e.form.problems(&self.others(i)),
            Some(e) => e.form.config().err().unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// What Apply writes: the configurations of `run.toml` in list order, or the problems with
    /// the name of their configuration (behaviour 5).
    pub fn to_write(&self) -> Result<Vec<RunEntry>, Vec<String>> {
        let mut out = Vec::new();
        let mut problems = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.source != Source::Project {
                continue;
            }
            for (_, p) in self.problems(i) {
                problems.push(format!("{}: {p}", e.name()));
            }
            if let Ok(c) = e.form.config() {
                out.push(RunEntry {
                    original: e.original.clone(),
                    config: c,
                });
            }
        }
        if problems.is_empty() {
            Ok(out)
        } else {
            Err(problems)
        }
    }

    /// The temporary configurations as edited.
    pub fn temporary(&self) -> Vec<RunConfig> {
        self.entries
            .iter()
            .filter(|e| e.source == Source::Temporary)
            .filter_map(|e| e.form.config().ok())
            .collect()
    }

    /// After a write: what was written is what is loaded now.
    pub fn written(&mut self) {
        for e in &mut self.entries {
            if e.source == Source::Project
                && let Ok(c) = e.form.config()
            {
                e.original = Some(c.name.clone());
                e.base = c;
                e.form = RunForm::of(&e.base);
            }
        }
        self.structure_changed = false;
        for kind in RunTemplate::ALL {
            if let Ok(t) = self.template_form(kind).template() {
                if t.is_empty() {
                    self.templates_base.remove(&kind);
                } else {
                    self.templates_base.insert(kind, t);
                }
            }
        }
        self.templates.clear();
    }

    /// `run.toml` changed on disk (behaviour 7): entries without changes take the new
    /// configurations (new ones appear, removed ones go), changed ones stay as edited and remain
    /// marked as changed. The selection stays on its entry.
    pub fn reload(&mut self, project: Vec<RunConfig>) {
        let selected = self.current().map(|e| e.name().to_owned());
        let mut next: Vec<Entry> = Vec::new();
        for c in project {
            let kept = self
                .entries
                .iter()
                .find(|e| e.source == Source::Project && e.original.as_deref() == Some(&c.name));
            match kept {
                Some(e) if e.changed() => next.push(Entry {
                    base: c,
                    ..e.clone()
                }),
                _ => next.push(Entry::new(Source::Project, c, true)),
            }
        }
        // Changed entries whose configuration went from the file, and new ones, stay.
        for e in &self.entries {
            let listed = next
                .iter()
                .any(|n| n.original.is_some() && n.original == e.original);
            if e.source == Source::Project && e.changed() && !listed {
                let mut e = e.clone();
                e.original = None;
                next.push(e);
            }
        }
        next.extend(
            self.entries
                .iter()
                .filter(|e| e.source != Source::Project)
                .cloned(),
        );
        self.entries = next;
        self.selected = selected
            .and_then(|n| self.entries.iter().position(|e| e.name() == n))
            .or((!self.entries.is_empty()).then_some(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(name: &str, command: &[&str]) -> RunConfig {
        RunConfig {
            name: name.into(),
            command: command.iter().map(|s| (*s).to_owned()).collect(),
            cwd: None,
            env: Default::default(),
            kind: RunKind::Run,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: false,
            http: None,
        }
    }

    /// SPEC-0047 v6 behaviour 17: editing an HTTP request configuration keeps what it sends.
    #[test]
    fn the_form_keeps_the_requests_of_an_http_configuration() {
        let mut c = config("api", &[]);
        c.http = Some(ferris_idea_core::HttpRun {
            file: "api.http".into(),
            request: Some("Liste".into()),
            environment: Some("local".into()),
        });
        let mut form = RunForm::of(&c);
        form.folder = "HTTP".into();
        let back = form.config().unwrap();
        assert_eq!(back.http, c.http);
        assert!(form.problems(&[]).is_empty(), "{:?}", form.problems(&[]));
    }

    #[test]
    fn the_form_reads_command_environment_and_timeout() {
        let mut c = config("app", &["node", "main.js", "two words"]);
        c.env.insert("GREETING".into(), "hi there".into());
        c.timeout = Some(30);
        let form = RunForm::of(&c);
        assert_eq!(form.command, "node main.js 'two words'");
        assert_eq!(form.env, "GREETING=hi there");
        assert_eq!(form.config().unwrap(), c);
        let mut bad = form.clone();
        bad.command = "node 'open".into();
        bad.env = "GREETING".into();
        bad.timeout = "soon".into();
        let fields: Vec<RunField> = bad.config().unwrap_err().into_iter().map(|p| p.0).collect();
        assert_eq!(
            fields,
            [RunField::Command, RunField::Env, RunField::Timeout]
        );
        // Live problems name the field, without the configuration's name in front.
        let mut empty = form.clone();
        empty.command = String::new();
        assert_eq!(
            empty.problems(&[]),
            [(RunField::Command, "the command is empty".into())]
        );
        assert_eq!(
            form.problems(&[config("app", &["x"])]),
            [(RunField::Name, "the name is used twice".into())]
        );
    }

    #[test]
    fn add_remove_copy_move_and_what_apply_writes() {
        let mut m = EditorModel::new(
            vec![config("a", &["x"]), config("b", &["y"])],
            vec![config("hello.js", &["node", "hello.js"])],
            vec![config("npm run dev", &["npm", "run", "dev"])],
            Some("b"),
        );
        assert_eq!(m.selected, Some(1));
        assert!(!m.dirty());
        let at = m.add(RunTemplate::NodeFile);
        assert_eq!(at, 2);
        assert_eq!(m.entries[2].name(), "Node.js file");
        assert!(m.dirty());
        assert!(m.move_selected(true));
        assert_eq!(m.selected, Some(1));
        // Detected entries cannot be removed or moved; Copy makes an editable one.
        m.selected = Some(4);
        assert!(!m.remove_selected());
        assert!(!m.move_selected(true));
        let copy = m.copy_selected().unwrap();
        assert_eq!(m.entries[copy].source, Source::Project);
        assert_eq!(m.entries[copy].name(), "npm run dev");
        let names = |m: &EditorModel| {
            m.to_write()
                .unwrap()
                .into_iter()
                .map(|e| e.config.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&m), ["a", "Node.js file", "b", "npm run dev"]);
        // A problem keeps everything from being written.
        m.entries[0].form.command = String::new();
        assert_eq!(m.to_write().unwrap_err(), ["a: the command is empty"]);
        m.entries[0].form.command = "x".into();
        // Save Configuration of the temporary one gives it a free name in run.toml.
        let t = m.entries.iter().position(|e| e.source == Source::Temporary);
        m.selected = t;
        assert_eq!(m.save_temporary().as_deref(), Some("hello.js"));
        assert_eq!(
            names(&m),
            ["a", "Node.js file", "b", "npm run dev", "hello.js"]
        );
        m.written();
        assert!(!m.dirty());
        assert!(m.entries[..5].iter().all(|e| e.original.is_some()));
    }

    /// SPEC-0327 v2 AK-7: folder, instances and before-launch tasks in the form.
    #[test]
    fn the_form_reads_folder_instances_and_tasks() {
        let mut c = config("web", &["npm", "run", "dev"]);
        c.folder = Some("Frontend".into());
        c.allow_multiple = true;
        c.before_launch = vec![
            BeforeTask::build(),
            BeforeTask::tool(vec!["sh".into(), "gen.sh".into()], Some("scripts".into())),
            BeforeTask::browser("http://localhost:3000"),
        ];
        let form = RunForm::of(&c);
        assert_eq!(form.folder, "Frontend");
        assert!(form.allow_multiple);
        assert_eq!(
            form.tasks,
            "build\ntool cwd=scripts sh gen.sh\nbrowser http://localhost:3000"
        );
        assert_eq!(form.config().unwrap(), c);
        // A line that is no task is a problem of Before launch, with its line.
        let mut bad = form.clone();
        bad.tasks = "build\n\ndeploy now".into();
        let problems = bad.config().unwrap_err();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].0, RunField::Before);
        assert!(problems[0].1.starts_with("task line 3: "), "{problems:?}");
        // A URL that would start a program is refused while typing.
        let mut scheme = form.clone();
        scheme.tasks = "browser vscode://x/y".into();
        let problems = scheme.problems(&[]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].0, RunField::Before);
        assert!(problems[0].1.contains("only http, https and file URLs"));
        // An empty folder field means no folder.
        let mut none = form.clone();
        none.folder = "  ".into();
        assert_eq!(none.config().unwrap().folder, None);
    }

    /// SPEC-0327 v2 AK-7: the list groups by folder; Move stays inside the folder.
    #[test]
    fn the_list_groups_by_folder_and_moves_within_it() {
        let in_folder = |name: &str, folder: &str| {
            let mut c = config(name, &["x"]);
            c.folder = Some(folder.into());
            c
        };
        let mut m = EditorModel::new(
            vec![
                in_folder("api", "Backend"),
                config("lint", &["x"]),
                in_folder("web", "Frontend"),
                in_folder("worker", "Backend"),
                config("docs", &["x"]),
            ],
            vec![config("tmp.js", &["node", "tmp.js"])],
            Vec::new(),
            Some("worker"),
        );
        let shown = |m: &EditorModel| {
            m.visible()
                .into_iter()
                .map(|i| (m.folder_of(i), m.entries[i].name().to_owned()))
                .collect::<Vec<_>>()
        };
        let f = |s: &str| Some(s.to_owned());
        assert_eq!(
            shown(&m),
            [
                (None, "lint".to_owned()),
                (None, "docs".to_owned()),
                (f("Backend"), "api".to_owned()),
                (f("Backend"), "worker".to_owned()),
                (f("Frontend"), "web".to_owned()),
                (None, "tmp.js".to_owned()),
            ]
        );
        assert_eq!(m.folders(), ["Backend", "Frontend"]);
        // worker moves above api, the only other one of its folder; not further.
        assert!(m.move_selected(true));
        assert_eq!(m.current().unwrap().name(), "worker");
        assert!(!m.move_selected(true));
        assert_eq!(
            m.to_write()
                .unwrap()
                .into_iter()
                .map(|e| e.config.name)
                .collect::<Vec<_>>(),
            ["worker", "lint", "web", "api", "docs"]
        );
        // The filter keeps the grouping.
        m.filter = "w".into();
        assert_eq!(
            shown(&m).into_iter().map(|(_, n)| n).collect::<Vec<_>>(),
            ["worker", "web"]
        );
    }

    /// SPEC-0327 behaviour 7.
    #[test]
    fn a_change_on_disk_keeps_the_entry_being_edited() {
        let mut m = EditorModel::new(
            vec![config("a", &["x"]), config("b", &["y"])],
            Vec::new(),
            Vec::new(),
            Some("b"),
        );
        m.entries[1].form.command = "y --edited".into();
        m.reload(vec![
            config("a", &["x", "--from-disk"]),
            config("b", &["y", "--from-disk"]),
            config("c", &["z"]),
        ]);
        assert_eq!(m.entries.len(), 3);
        assert_eq!(m.entries[0].form.command, "x --from-disk");
        assert!(!m.entries[0].changed());
        assert_eq!(m.entries[1].form.command, "y --edited");
        assert!(m.entries[1].changed());
        assert_eq!(m.entries[2].name(), "c");
        assert_eq!(m.selected, Some(1));
    }

    /// SPEC-0327 v3 AK-9: folders are made, renamed and removed in the list, and configurations
    /// move into and out of them.
    #[test]
    fn folders_are_made_renamed_and_removed() {
        let in_folder = |name: &str, folder: &str| {
            let mut c = config(name, &["x"]);
            c.folder = Some(folder.into());
            c
        };
        let mut m = EditorModel::new(
            vec![
                config("lint", &["x"]),
                in_folder("api", "Backend"),
                in_folder("worker", "Backend"),
            ],
            vec![config("tmp.js", &["node", "tmp.js"])],
            vec![config("npm run dev", &["npm", "run", "dev"])],
            Some("lint"),
        );
        let e = |m: &EditorModel, name: &str| {
            Row::Entry(m.entries.iter().position(|e| e.name() == name).unwrap())
        };
        let f = |name: &str| Row::Folder(name.to_owned());
        assert_eq!(
            m.rows(),
            [
                e(&m, "lint"),
                f("Backend"),
                e(&m, "api"),
                e(&m, "worker"),
                Row::Heading(crate::run_list_model::TEMPORARY),
                e(&m, "tmp.js"),
                Row::Heading(crate::run_list_model::DETECTED),
                e(&m, "npm run dev"),
            ]
        );
        // New Folder: listed after the folders in use, selected, nothing to write yet.
        assert_eq!(m.new_folder(), "New Folder");
        assert_eq!(m.new_folder(), "New Folder (2)");
        assert_eq!(m.selected_folder.as_deref(), Some("New Folder (2)"));
        assert_eq!(m.folders(), ["Backend", "New Folder", "New Folder (2)"]);
        assert!(!m.dirty());
        assert_eq!(
            m.rows()[..6],
            [
                e(&m, "lint"),
                f("Backend"),
                e(&m, "api"),
                e(&m, "worker"),
                f("New Folder"),
                f("New Folder (2)"),
            ]
        );
        // Rename: a name that is none or taken is refused; the members follow.
        assert!(m.rename_folder("New Folder", " ").is_err());
        assert_eq!(
            m.rename_folder("New Folder", "Backend").unwrap_err(),
            "a folder of that name exists already"
        );
        m.rename_folder("New Folder", "Checks").unwrap();
        m.rename_folder("Backend", "Services").unwrap();
        assert_eq!(m.folder_members("Services"), ["api", "worker"]);
        assert!(m.dirty());
        // Move into and out of a folder.
        assert!(m.set_folder(0, Some("Checks")));
        assert!(m.set_folder(1, None));
        assert!(
            !m.set_folder(3, Some("Checks")),
            "a temporary one has no folder"
        );
        assert_eq!(m.folder_members("Checks"), ["lint"]);
        assert_eq!(
            m.rows()[..7],
            [
                e(&m, "api"),
                f("Checks"),
                e(&m, "lint"),
                f("Services"),
                e(&m, "worker"),
                f("New Folder (2)"),
                Row::Heading(crate::run_list_model::TEMPORARY),
            ]
        );
        // The filter shows a folder only above what it keeps.
        m.filter = "work".into();
        assert_eq!(m.rows(), [f("Services"), e(&m, "worker")]);
        m.filter.clear();
        // Remove: the members are listed on their own again; an empty folder just goes.
        assert!(m.remove_folder("Services"));
        assert!(m.remove_folder("New Folder (2)"));
        assert!(!m.remove_folder("Nope"));
        assert_eq!(m.selected_folder, None);
        let written: Vec<(String, Option<String>)> = m
            .to_write()
            .unwrap()
            .into_iter()
            .map(|e| (e.config.name, e.config.folder))
            .collect();
        assert_eq!(
            written,
            [
                ("lint".to_owned(), Some("Checks".to_owned())),
                ("api".to_owned(), None),
                ("worker".to_owned(), None),
            ]
        );
    }

    /// SPEC-0327 v3 AK-10: the templates per type are edited as a form, checked, written with
    /// Apply, and a new configuration is a copy of its type's template.
    #[test]
    fn templates_are_edited_and_copied_into_new_configurations() {
        let loaded = TypeTemplate {
            cwd: Some("web".into()),
            ..TypeTemplate::default()
        };
        let mut m = EditorModel::new(vec![config("server", &["x"])], Vec::new(), Vec::new(), None)
            .with_templates(BTreeMap::from([(RunTemplate::NpmScript, loaded.clone())]));
        assert!(m.template_set(RunTemplate::NpmScript));
        assert!(!m.template_set(RunTemplate::NodeFile));
        assert!(!m.dirty());
        m.edit_template(RunTemplate::NodeFile, |f| {
            f.interpreter = "/opt/node22/bin/node".into();
            f.options = "--enable-source-maps \"--title=two words\"".into();
            f.env = "NODE_ENV=development".into();
            f.before = vec!["server".into()];
            f.tasks = "build\nbrowser wait=3000 http://localhost:3000".into();
            f.allow_multiple = true;
            f.timeout = "120".into();
        });
        assert!(m.template_changed(RunTemplate::NodeFile));
        assert!(m.dirty());
        assert!(m.template_problems(RunTemplate::NodeFile).is_empty());
        // A new configuration of the type is a copy of the template as it stands now.
        let at = m.add(RunTemplate::NodeFile);
        let c = m.entries[at].form.config().unwrap();
        assert_eq!(
            c.command,
            [
                "/opt/node22/bin/node",
                "--enable-source-maps",
                "--title=two words",
                "index.js"
            ]
        );
        assert_eq!(
            c.env.get("NODE_ENV").map(String::as_str),
            Some("development")
        );
        assert_eq!(c.before, ["server"]);
        assert_eq!(
            c.before_launch,
            [
                BeforeTask::build(),
                BeforeTask::browser_after_port("http://localhost:3000", 3000)
            ]
        );
        assert!(c.allow_multiple);
        assert_eq!(c.timeout, Some(120));
        // Changing the template afterwards does not reach the configuration.
        m.edit_template(RunTemplate::NodeFile, |f| f.timeout = "5".into());
        assert_eq!(m.entries[at].form.timeout, "120");
        // Another type starts from its own template, a third from nothing.
        let npm = m.add(RunTemplate::NpmScript);
        assert_eq!(m.entries[npm].form.cwd, "web");
        let go = m.add(RunTemplate::GoTest);
        assert_eq!(
            m.entries[go].form,
            RunForm::of(&RunTemplate::GoTest.config(&[]))
        );
        // Problems are named under their field and keep the templates from being written.
        m.edit_template(RunTemplate::Command, |f| f.interpreter = "sh".into());
        m.edit_template(RunTemplate::Pytest, |f| {
            f.cwd = "../out".into();
            f.tasks = "browser vscode://x".into();
            f.before = vec!["gone".into()];
        });
        let fields: Vec<RunField> = m
            .template_problems(RunTemplate::Pytest)
            .into_iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(fields, [RunField::Cwd, RunField::Before, RunField::Before]);
        assert_eq!(
            m.template_problems(RunTemplate::Command),
            [(
                RunField::Command,
                "this type has no program, so it takes no interpreter and no options".into()
            )]
        );
        let problems = m.templates_to_write().unwrap_err();
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(problems[0].starts_with("Template Command: "));
        m.edit_template(RunTemplate::Command, |f| *f = TemplateForm::default());
        m.edit_template(RunTemplate::Pytest, |f| *f = TemplateForm::default());
        let write = m.templates_to_write().unwrap();
        // Only the changed type is named; the others keep their table in the file.
        assert_eq!(
            write.keys().copied().collect::<Vec<_>>(),
            [RunTemplate::NodeFile]
        );
        m.written();
        assert!(!m.dirty());
        assert_eq!(
            m.templates_base.keys().copied().collect::<Vec<_>>(),
            [RunTemplate::NodeFile, RunTemplate::NpmScript]
        );
        assert_eq!(m.templates_base[&RunTemplate::NpmScript], loaded);
        // A change on disk reaches a template that is not being edited.
        m.edit_template(RunTemplate::NodeFile, |f| f.timeout = "7".into());
        m.reload_templates(BTreeMap::from([(
            RunTemplate::Cargo,
            TypeTemplate {
                timeout: Some(900),
                ..TypeTemplate::default()
            },
        )]));
        assert_eq!(m.template_form(RunTemplate::Cargo).timeout, "900");
        assert_eq!(m.template_form(RunTemplate::NodeFile).timeout, "7");
        assert!(!m.template_set(RunTemplate::NpmScript));
    }
}
