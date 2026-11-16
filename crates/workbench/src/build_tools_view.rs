//! The build tools window (`SPEC-0100` v3): the workspace's modules nested as their builds nest
//! them, with task groups (Maven lifecycle and plugins, Gradle task groups), Maven profiles,
//! dependency trees and the problems of the build files; a task runs as a run configuration with
//! the window's options (offline, skip tests, profiles). v3: sources and documentation, the
//! dependency analyzer and diagram, linking and unlinking build files, triggers and shortcuts of
//! tasks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use idecraft_core::buildtasks::{self, When};
use idecraft_core::buildtools::{self, BuildOptions, Task};

use crate::build_tools_model::{
    Entry, Extras, Problem, Row, RowKind, all_group_keys, analysis_expanded, initially_expanded,
    note_key, rows,
};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::Tipped;
use crate::workbench_view::c;

/// What the window asks the workbench to do.
pub enum BuildToolsEvent {
    /// Hide the window.
    Hide,
    /// Run this configuration (a task).
    Run(Box<idecraft_core::RunConfig>),
    /// Open this build file at this 1-based line (a problem, behaviour 12).
    Open(PathBuf, usize),
    /// The context menu of a task row at this window position (behaviour 10).
    ContextMenu(gpui::Point<gpui::Pixels>),
    /// Ask for a build command to run in this module (behaviour 5).
    Execute(Box<idecraft_core::Module>),
    /// Show this in the status line.
    Status(String),
    /// Give keymap action `.0` these shortcuts (v3 behaviour 19).
    Shortcut(String, Vec<idecraft_ui::keymap::Shortcut>),
    /// Ask for a build file to link, the field prefilled with this text (v3 behaviour 16).
    LinkPrompt(String),
    /// Sync the build files with the tasks bound to a sync (the Reload button, v4).
    Sync,
    /// The modules of the project changed (a build file linked or a module unlinked, v4): the
    /// project tree reads the folder roles again.
    ModelChanged,
    /// Show the dependency diagram of a module: its name, the boxes and edges, the Mermaid text
    /// (v4 behaviour 15).
    Diagram(String, Box<idecraft_database::ErModel>, String),
}

/// The window.
pub struct BuildToolsView {
    root: PathBuf,
    entries: Vec<Entry>,
    problems: Vec<Problem>,
    profiles: Vec<(String, bool)>,
    options: BuildOptions,
    expanded: HashSet<String>,
    selected: usize,
    loading: bool,
    /// Maven and Gradle modules are being resolved by their build tool (`SPEC-0149`).
    resolving: bool,
    /// Why the last resolution failed, per module.
    resolve_errors: Vec<String>,
    /// Another resolution was asked for while one ran (`Some(force)`); it runs afterwards.
    resolve_again: Option<bool>,
    /// The analyzer's module (folder, label) and result (v3 behaviour 14).
    analysis: Option<(String, String, Vec<idecraft_core::depanalysis::Artifact>)>,
    /// Typed text narrowing the analyzer's list of all artifacts (v4).
    analysis_filter: String,
    /// The module folder to analyse once its resolution is done.
    analyse_after_resolve: Option<String>,
    /// Sources and documentation are being downloaded (v3 behaviour 13).
    downloading: bool,
    /// The next keystroke becomes a shortcut of this action, named so (v3 behaviour 19).
    recording: Option<(String, String)>,
    focus: FocusHandle,
}

impl EventEmitter<BuildToolsEvent> for BuildToolsView {}

/// What [`load`] reads.
pub struct Loaded {
    /// Modules with their groups and trees.
    pub entries: Vec<Entry>,
    /// Problems of the build files and missing artifacts.
    pub problems: Vec<Problem>,
    /// Maven profiles of the project with their default state.
    pub profiles: Vec<(String, bool)>,
}

/// "core/pom.xml: line 20, column 3: …" → the file and line of a problem of the model.
fn problem_place(text: &str) -> Option<(String, usize)> {
    let (file, rest) = text.split_once(": line ")?;
    let line = rest.split(',').next()?.trim().parse().ok()?;
    Some((file.to_owned(), line))
}

/// Reads the modules, their task groups, dependency trees and problems of the workspace at `root`
/// (behaviours 1, 2, 12).
pub fn load(root: &Path) -> Loaded {
    let files = idecraft_core::Workspace::open(root)
        .and_then(|w| w.files())
        .unwrap_or_default();
    let model = idecraft_core::read_project(root, &files);
    let mut problems: Vec<Problem> = model
        .problems
        .iter()
        .map(|p| Problem {
            text: p.clone(),
            place: problem_place(p),
        })
        .collect();
    let profiles = buildtools::project_profiles(&model);
    let entries: Vec<Entry> = model
        .modules
        .iter()
        .cloned()
        .map(|module| {
            // Entries the build tool left out are the analyzer's, not the tree's (SPEC-0149 v3
            // behaviour 9).
            let mut deps = idecraft_core::dependency_tree(root, &module)
                .map(|t| idecraft_core::without_omitted(&t));
            // Sibling modules come from the reactor, not the local repository (behaviour 8).
            if let Ok(tree) = &mut deps {
                buildtools::unmark_modules(&model, tree);
            }
            if let Ok(tree) = &deps {
                let file = match module.kind {
                    idecraft_core::ModuleKind::Maven => Some("pom.xml"),
                    idecraft_core::ModuleKind::Gradle => ["build.gradle.kts", "build.gradle"]
                        .into_iter()
                        .find(|f| root.join(&module.dir).join(f).is_file()),
                    _ => None,
                };
                for missing in buildtools::missing_artifacts(tree) {
                    let file = file.map(|f| {
                        if module.dir == "." {
                            f.to_owned()
                        } else {
                            format!("{}/{f}", module.dir)
                        }
                    });
                    problems.push(Problem {
                        text: format!("Could not find {missing} ({})", module.short_name()),
                        place: file.map(|f| {
                            let line = artifact_line(root, &f, &missing);
                            (f, line)
                        }),
                    });
                }
            }
            Entry {
                groups: buildtools::task_groups(root, &module),
                deps,
                module,
            }
        })
        .collect();
    Loaded {
        entries,
        problems,
        profiles,
    }
}

/// The build file of the module in folder `dir`, workspace-relative: its `pom.xml` or Gradle build
/// script.
fn build_file_of(root: &Path, dir: &str) -> Option<String> {
    let folder = if dir == "." {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    ["pom.xml", "build.gradle.kts", "build.gradle"]
        .into_iter()
        .find(|f| folder.join(f).is_file())
        .map(|f| {
            if dir == "." {
                f.to_owned()
            } else {
                format!("{dir}/{f}")
            }
        })
}

/// Writes the dependency diagram of module `m` to `.idecraft/diagrams/<module>-dependencies.mmd`
/// (v3 behaviour 15, IMP-0339) and returns the file. The module must be resolved.
pub fn diagram_file(root: &Path, m: &idecraft_core::Module) -> Result<PathBuf, String> {
    let Some(tree) = idecraft_core::jvmdeps::resolved_tree(root, m) else {
        return Err(format!(
            "{} is not resolved yet: reload the build tools window first",
            m.short_name()
        ));
    };
    let text = idecraft_core::depanalysis::mermaid(m.short_name(), &tree);
    let file = root
        .join(".idecraft/diagrams")
        .join(format!("{}-dependencies.mmd", m.short_name()));
    std::fs::create_dir_all(file.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
    idecraft_core::write_atomic(&file, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(file)
}

/// The dependency diagram of module `module` as boxes and edges (SPEC-0100 v4 behaviour 15): the
/// module first, then a box per artifact and version (its group and version as the box's line) in
/// the column of its depth, an edge from each artifact to what it needs, left-out entries
/// (`omitted for conflict`) with dashed edges, and artifacts in conflict marked. Gradle's
/// classpaths are merged into one graph.
pub fn diagram_model(module: &str, tree: &[idecraft_core::DepNode]) -> idecraft_database::ErModel {
    use idecraft_database::{Dependency, Entity, ErColumn, ErModel, Relationship};
    let conflicts: HashSet<String> = idecraft_core::depanalysis::analyze(module, tree)
        .into_iter()
        .filter(|a| a.conflict)
        .map(|a| a.name)
        .collect();
    let mut model = ErModel {
        entities: vec![Entity {
            name: module.to_owned(),
            columns: Vec::new(),
            layer: 0,
            row: 0,
            view: false,
        }],
        relationships: Vec::new(),
        outside: Vec::new(),
        dependencies: Vec::new(),
    };
    // Breadth first, so an artifact sits in the column of its shortest path.
    let mut queue: std::collections::VecDeque<(String, usize, &idecraft_core::DepNode)> = tree
        .iter()
        .flat_map(|n| {
            if n.scope == "configuration" {
                n.children.iter().collect::<Vec<_>>()
            } else {
                vec![n]
            }
        })
        .map(|n| (module.to_owned(), 1, n))
        .collect();
    let mut rows: Vec<usize> = vec![1];
    while let Some((parent, depth, n)) = queue.pop_front() {
        let (asked, chosen) = idecraft_core::depanalysis::versions(n);
        let version = if n.omitted.is_some() { asked } else { chosen };
        let (group, artifact) = n.name.rsplit_once(':').unwrap_or(("", &n.name));
        let name = if version.is_empty() {
            artifact.to_owned()
        } else {
            format!("{artifact} {version}")
        };
        if !model.entities.iter().any(|e| e.name == name) {
            if rows.len() <= depth {
                rows.resize(depth + 1, 0);
            }
            model.entities.push(Entity {
                name: name.clone(),
                columns: vec![ErColumn {
                    name: group.to_owned(),
                    data_type: String::new(),
                    nullable: true,
                    keys: Vec::new(),
                }],
                layer: depth,
                row: rows[depth],
                view: conflicts.contains(&n.name),
            });
            rows[depth] += 1;
        }
        if n.omitted.is_some() {
            if !model
                .dependencies
                .iter()
                .any(|d| d.view == name && d.uses == parent)
            {
                model.dependencies.push(Dependency {
                    view: name.clone(),
                    uses: parent.clone(),
                });
            }
        } else if !model
            .relationships
            .iter()
            .any(|r| r.from == name && r.to == parent)
        {
            model.relationships.push(Relationship {
                from: name.clone(),
                from_columns: Vec::new(),
                to: parent.clone(),
                to_columns: Vec::new(),
                many: false,
                optional: false,
            });
        }
        for c in &n.children {
            queue.push_back((name.clone(), depth + 1, c));
        }
    }
    model
}

/// The 1-based line of a build file that names the artifact's id (`nowhere` of
/// `lab.missing:nowhere:9.9`), else 1.
fn artifact_line(root: &Path, file: &str, coords: &str) -> usize {
    let artifact = coords.split(':').nth(1).unwrap_or(coords);
    std::fs::read_to_string(root.join(file))
        .ok()
        .and_then(|t| t.lines().position(|l| l.contains(artifact)))
        .map_or(1, |i| i + 1)
}

impl BuildToolsView {
    /// A window for the workspace at `root`; reads it in the background.
    pub fn new(root: &Path, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            root: root.to_path_buf(),
            entries: Vec::new(),
            problems: Vec::new(),
            profiles: Vec::new(),
            options: BuildOptions::load(root),
            expanded: HashSet::new(),
            selected: 0,
            loading: false,
            resolving: false,
            resolve_errors: Vec::new(),
            resolve_again: None,
            analysis: None,
            analysis_filter: String::new(),
            analyse_after_resolve: None,
            downloading: false,
            recording: None,
            focus: cx.focus_handle(),
        };
        view.reload(cx);
        view.resolve(false, cx);
        view
    }

    /// Focus handle.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Reads the build files again (behaviour 4); expanded rows stay expanded.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        cx.notify();
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move { load(&root) });
        cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.entries.is_empty() {
                    this.expanded = initially_expanded(&loaded.entries);
                }
                this.entries = loaded.entries;
                this.problems = loaded.problems;
                this.profiles = loaded.profiles;
                this.loading = false;
                this.selected = this.selected.min(this.rows().len().saturating_sub(1));
                cx.notify();
            });
        })
        .detach();
    }

    /// Lets the build tool resolve the transitive dependencies of the Maven and Gradle modules
    /// (`SPEC-0149` behaviour 4): those whose build files changed since, or all with `force` (the
    /// reload button); the trees are read again afterwards.
    pub fn resolve(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.resolving {
            self.resolve_again = Some(force || self.resolve_again.unwrap_or(false));
            return;
        }
        let root = self.root.clone();
        let files = idecraft_core::Workspace::open(&root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        let modules: Vec<_> = idecraft_core::read_project(&root, &files)
            .modules
            .into_iter()
            .filter(|m| idecraft_core::jvmdeps::cache_file(&root, m).is_some())
            .filter(|m| force || !idecraft_core::jvmdeps::is_resolved(&root, m))
            .collect();
        if modules.is_empty() {
            return;
        }
        self.resolving = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            modules
                .iter()
                .filter_map(|m| {
                    idecraft_core::jvmdeps::resolve(&root, m)
                        .err()
                        .map(|e| format!("{}: {e}", m.name))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let errors = task.await;
            let _ = this.update(cx, |this, cx| {
                this.resolving = false;
                this.resolve_errors = errors;
                this.reload(cx);
                if let Some(force) = this.resolve_again.take() {
                    this.resolve(force, cx);
                } else if let Some(dir) = this.analyse_after_resolve.take() {
                    this.analyse_module(&dir, cx);
                }
            });
        })
        .detach();
    }

    /// The dependency tree of the module in folder `dir` (workspace-relative), as read last.
    pub fn dependencies_of(
        &self,
        dir: &str,
    ) -> Option<&Result<Vec<idecraft_core::DepNode>, String>> {
        self.entries
            .iter()
            .find(|e| e.module.dir == dir)
            .map(|e| &e.deps)
    }

    /// Whether the build tools are resolving dependencies.
    pub fn resolving(&self) -> bool {
        self.resolving
    }

    /// Why resolutions failed, one line per module.
    pub fn resolve_errors(&self) -> &[String] {
        &self.resolve_errors
    }

    /// Whether the build files are being read.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// The options of the window (behaviours 6, 7).
    pub fn options(&self) -> &BuildOptions {
        &self.options
    }

    fn extras(&self) -> Extras {
        Extras {
            problems: self.problems.clone(),
            profiles: self
                .profiles
                .iter()
                .map(|(id, default)| (id.clone(), self.options.profile_active(id, *default)))
                .collect(),
            analysis: self
                .analysis
                .as_ref()
                .map(|(_, label, artifacts)| (label.clone(), artifacts.clone())),
            task_notes: self
                .task_notes()
                .into_iter()
                .map(|(k, v)| (k, v.join(" · ")))
                .collect(),
            analysis_filter: self.analysis_filter.clone(),
        }
    }

    /// Takes text the platform's text input committed: with the analyzer open it narrows the
    /// analyzer's list (`SPEC-0004` v8 behaviour 17b); spaces are not typed.
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        if self.analysis.is_none() || committed.contains(' ') {
            return;
        }
        let text = format!("{}{committed}", self.analysis_filter);
        self.set_analysis_filter(&text, cx);
    }

    /// Narrows the analyzer's list of all artifacts to those whose name contains `text` (v4).
    pub fn set_analysis_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.analysis_filter = text.to_lowercase();
        self.selected = 0;
        cx.notify();
    }

    /// The triggers and shortcuts of tasks, by module folder and task (v3 behaviours 18, 19).
    fn task_notes(&self) -> HashMap<String, Vec<String>> {
        let mut notes: HashMap<String, Vec<String>> = HashMap::new();
        for t in &self.options.triggers {
            notes
                .entry(note_key(&t.module, &t.task))
                .or_default()
                .push(t.when.label().to_owned());
        }
        let keymap = crate::keymaps::current();
        for (id, shortcuts) in keymap.actions() {
            if let (Some((dir, label)), Some(s)) =
                (buildtasks::parse_task_action(id), shortcuts.first())
            {
                notes
                    .entry(note_key(&dir, &label))
                    .or_default()
                    .push(s.display(keymap.macos));
            }
        }
        notes
    }

    /// The visible rows.
    pub fn rows(&self) -> Vec<Row> {
        rows(&self.entries, &self.extras(), &self.expanded)
    }

    /// The selected row, if any.
    pub fn selected_row(&self) -> Option<Row> {
        self.rows().into_iter().nth(self.selected)
    }

    /// Visible rows as indented text, details, versions, scopes and states included (for tests).
    pub fn row_texts(&self) -> Vec<String> {
        self.rows()
            .iter()
            .map(|r| {
                let extra = match &r.kind {
                    RowKind::Dependency(v, scope, repeated, missing) => format!(
                        "{}{}{}{}",
                        v.as_deref().map(|v| format!(" {v}")).unwrap_or_default(),
                        if scope.is_empty() || scope == "normal" {
                            String::new()
                        } else {
                            format!(" ({scope})")
                        },
                        if *repeated { " *" } else { "" },
                        if *missing { " (not found)" } else { "" }
                    ),
                    RowKind::Profile(_, on) => format!(" [{}]", if *on { "x" } else { " " }),
                    RowKind::Module(_) => format!(" · {}", r.detail),
                    RowKind::Task(_) if !r.detail.is_empty() => format!(" · {}", r.detail),
                    _ => String::new(),
                };
                format!("{}{}{extra}", "  ".repeat(r.depth), r.label)
            })
            .collect()
    }

    /// Expands or collapses the row with `label` (for tests and clicks by label).
    pub fn toggle_label(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        let Some(i) = self.rows().iter().position(|r| r.label == label) else {
            return false;
        };
        self.toggle(i, cx);
        true
    }

    /// Selects the first row with `label` (tests).
    pub fn select_label(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        let Some(i) = self.rows().iter().position(|r| r.label == label) else {
            return false;
        };
        self.selected = i;
        cx.notify();
        true
    }

    /// Selects the first row with `label` below the module row labelled `module` (tests).
    pub fn select_label_in(&mut self, module: &str, label: &str, cx: &mut Context<Self>) -> bool {
        let rows = self.rows();
        let Some(start) = rows
            .iter()
            .position(|r| r.label == module && matches!(r.kind, RowKind::Module(_)))
        else {
            return false;
        };
        let Some(at) = rows
            .iter()
            .skip(start + 1)
            .position(|r| r.label == label)
            .map(|k| k + start + 1)
        else {
            return false;
        };
        self.selected = at;
        cx.notify();
        true
    }

    fn toggle(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows().into_iter().nth(i) else {
            return;
        };
        if !row.expandable {
            return;
        }
        if !self.expanded.remove(&row.key) {
            self.expanded.insert(row.key);
        }
        self.selected = i;
        cx.notify();
    }

    /// Opens every module and group (not the dependency trees), or closes all (behaviour 7).
    pub fn expand_all(&mut self, open: bool, cx: &mut Context<Self>) {
        self.expanded = if open {
            all_group_keys(&self.entries, &self.extras())
        } else {
            HashSet::new()
        };
        self.selected = 0;
        cx.notify();
    }

    /// Switches offline mode (behaviour 6) and keeps it.
    pub fn set_offline(&mut self, on: bool, cx: &mut Context<Self>) {
        self.options.offline = on;
        let _ = self.options.save(&self.root);
        cx.notify();
    }

    /// Switches "skip tests" (behaviour 6) and keeps it.
    pub fn set_skip_tests(&mut self, on: bool, cx: &mut Context<Self>) {
        self.options.skip_tests = on;
        let _ = self.options.save(&self.root);
        cx.notify();
    }

    /// Switches Maven profile `id` on or off (behaviour 6) and keeps it; the trees are resolved
    /// again with the new profiles.
    pub fn set_profile(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        let default = self
            .profiles
            .iter()
            .find(|(p, _)| p == id)
            .is_some_and(|(_, d)| *d);
        if on == default {
            self.options.profiles.remove(id);
        } else {
            self.options.profiles.insert(id.to_owned(), on);
        }
        let _ = self.options.save(&self.root);
        self.resolve(true, cx);
        cx.notify();
    }

    /// The command of `task` with the window's options (behaviour 6).
    pub fn command_of(&self, task: &Task) -> Vec<String> {
        self.options.apply(&task.command, &self.profiles)
    }

    /// The folder of the module whose groups hold `task`.
    pub fn module_dir_of(&self, task: &Task) -> String {
        self.entries
            .iter()
            .find(|e| e.groups.iter().any(|g| g.all_tasks().contains(&task)))
            .map(|e| e.module.dir.clone())
            .unwrap_or_else(|| task.dir.clone())
    }

    /// The run configuration of `task`: its name, the command with the options, its folder; a
    /// test task reports to the test tree (behaviour 2).
    pub fn run_config(&self, task: &Task) -> idecraft_core::RunConfig {
        buildtasks::task_config(
            &self.module_dir_of(task),
            task,
            &self.options,
            &self.profiles,
        )
    }

    /// The selected task with its module's folder, if a task row is selected.
    pub fn selected_task(&self) -> Option<(String, Task)> {
        match self.selected_row()?.kind {
            RowKind::Task(t) => Some((self.module_dir_of(&t), *t)),
            _ => None,
        }
    }

    /// Which triggers the selected task has: Before Build, After Build, Before Run.
    pub fn selected_triggers(&self) -> [bool; 5] {
        let Some((dir, task)) = self.selected_task() else {
            return [false; 5];
        };
        When::ALL.map(|w| self.options.has_trigger(w, &dir, &task.label))
    }

    /// Switches trigger `when` of the selected task and keeps it (v3 behaviour 18).
    pub fn toggle_trigger(&mut self, when: When, cx: &mut Context<Self>) {
        let Some((dir, task)) = self.selected_task() else {
            return;
        };
        let on = !self.options.has_trigger(when, &dir, &task.label);
        self.options.set_trigger(when, &dir, &task.label, on);
        let _ = self.options.save(&self.root);
        cx.emit(BuildToolsEvent::Status(format!(
            "{}: {} {}",
            task.run_name,
            if on { "runs" } else { "no longer runs" },
            when.label().to_lowercase()
        )));
        cx.notify();
    }

    /// The task of keymap action `id` (v3 behaviour 19): its module folder and run
    /// configuration.
    pub fn task_of_action(&self, id: &str) -> Option<idecraft_core::RunConfig> {
        let (dir, label) = buildtasks::parse_task_action(id)?;
        let entry = self.entries.iter().find(|e| e.module.dir == dir)?;
        let task = entry
            .groups
            .iter()
            .flat_map(|g| g.all_tasks())
            .find(|t| t.label == label)?
            .clone();
        Some(self.run_config(&task))
    }

    /// Waits for the next keystroke as a shortcut of the selected task (v3 behaviour 19).
    pub fn start_recording(&mut self, cx: &mut Context<Self>) {
        let Some((dir, task)) = self.selected_task() else {
            return;
        };
        let id = buildtasks::task_action_id(&dir, &task.label);
        cx.emit(BuildToolsEvent::Status(format!(
            "Press the shortcut for {} (Escape cancels)",
            task.run_name
        )));
        self.recording = Some((id, task.run_name));
        cx.notify();
    }

    /// Whether the next keystroke is recorded as a shortcut.
    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Takes `stroke` as the recorded shortcut: adds it to the action's shortcuts and names the
    /// other actions on the same keys (v3 behaviour 19).
    pub fn record(&mut self, stroke: idecraft_ui::keymap::Keystroke, cx: &mut Context<Self>) {
        let Some((id, name)) = self.recording.take() else {
            return;
        };
        use idecraft_ui::keymap::Shortcut;
        let keymap = crate::keymaps::current();
        let others: Vec<String> = keymap
            .actions_for(&stroke, None)
            .into_iter()
            .filter(|a| *a != id)
            .map(str::to_owned)
            .collect();
        let new = Shortcut::Keyboard {
            first: stroke,
            second: None,
        };
        let shown = new.display(keymap.macos);
        let mut shortcuts = keymap.shortcuts(&id).to_vec();
        if !shortcuts.contains(&new) {
            shortcuts.push(new);
        }
        cx.emit(BuildToolsEvent::Shortcut(id, shortcuts));
        cx.emit(BuildToolsEvent::Status(if others.is_empty() {
            format!("{shown} runs {name}")
        } else {
            format!("{shown} runs {name}; also used by {}", others.join(", "))
        }));
        cx.notify();
    }

    /// The selected row's module, else the first Maven or Gradle module.
    fn jvm_module(&self) -> Option<idecraft_core::Module> {
        let jvm = |m: &idecraft_core::Module| {
            matches!(
                m.kind,
                idecraft_core::ModuleKind::Maven | idecraft_core::ModuleKind::Gradle
            )
        };
        // A row of the analyzer stands for the analysed module (v4).
        let analysed = self
            .analysis
            .as_ref()
            .filter(|_| {
                self.rows()
                    .get(self.selected)
                    .is_some_and(|r| r.key.starts_with("analysis"))
            })
            .and_then(|(dir, _, _)| {
                self.entries
                    .iter()
                    .map(|e| &e.module)
                    .find(|m| m.dir == *dir)
                    .cloned()
            });
        if analysed.is_some() {
            return analysed;
        }
        self.module_of_row(self.selected).filter(jvm).or_else(|| {
            self.entries
                .iter()
                .map(|e| &e.module)
                .find(|m| jvm(m))
                .cloned()
        })
    }

    /// Shows the dependency analyzer for the selected module, or closes it (v3 behaviour 14). A
    /// module that is not resolved is resolved first.
    pub fn toggle_analysis(&mut self, cx: &mut Context<Self>) {
        if self.analysis.take().is_some() {
            cx.notify();
            return;
        }
        let Some(m) = self.jvm_module() else {
            cx.emit(BuildToolsEvent::Status(
                "The dependency analyzer needs a Maven or Gradle module".into(),
            ));
            return;
        };
        if !idecraft_core::jvmdeps::is_resolved(&self.root, &m) {
            self.analyse_after_resolve = Some(m.dir.clone());
            cx.emit(BuildToolsEvent::Status(format!(
                "Resolving {} before the analysis…",
                m.short_name()
            )));
            self.resolve(false, cx);
            return;
        }
        self.analyse_module(&m.dir, cx);
    }

    /// Analyses the module in folder `dir` from its kept resolution.
    pub fn analyse_module(&mut self, dir: &str, cx: &mut Context<Self>) {
        let Some(m) = self
            .entries
            .iter()
            .map(|e| e.module.clone())
            .find(|m| m.dir == dir)
        else {
            return;
        };
        let Some(mut tree) = idecraft_core::jvmdeps::resolved_tree(&self.root, &m) else {
            cx.emit(BuildToolsEvent::Status(format!(
                "{} could not be resolved; see the problems",
                m.short_name()
            )));
            return;
        };
        let files = idecraft_core::Workspace::open(&self.root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        let model = idecraft_core::read_project(&self.root, &files);
        buildtools::unmark_modules(&model, &mut tree);
        let label = m.short_name().to_owned();
        let artifacts = idecraft_core::depanalysis::analyze(&label, &tree);
        self.expanded.extend(analysis_expanded(&artifacts));
        self.analysis = Some((m.dir.clone(), label, artifacts));
        self.selected = 0;
        cx.notify();
    }

    /// The analyzer's module label and its artifacts, if it is shown (self-test).
    pub fn analysis(&self) -> Option<(&str, &[idecraft_core::depanalysis::Artifact])> {
        self.analysis
            .as_ref()
            .map(|(_, l, a)| (l.as_str(), a.as_slice()))
    }

    /// Writes the selected module's dependency diagram and asks the workbench to open it (v3
    /// behaviour 15).
    pub fn show_diagram(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.jvm_module() else {
            cx.emit(BuildToolsEvent::Status(
                "The dependency diagram needs a Maven or Gradle module".into(),
            ));
            return;
        };
        // The text stays in `.idecraft/diagrams/` (IMP-0339); the drawing opens over the editor.
        match diagram_file(&self.root, &m)
            .and_then(|file| std::fs::read_to_string(&file).map_err(|e| e.to_string()))
        {
            Ok(text) => {
                let tree =
                    idecraft_core::jvmdeps::resolved_tree(&self.root, &m).unwrap_or_default();
                let model = diagram_model(m.short_name(), &tree);
                cx.emit(BuildToolsEvent::Diagram(
                    format!("{}-dependencies", m.short_name()),
                    Box::new(model),
                    text,
                ));
            }
            Err(e) => cx.emit(BuildToolsEvent::Status(e)),
        }
    }

    /// Downloads the sources and documentation of the Maven and Gradle libraries in the
    /// background and reports what the caches hold afterwards (v3 behaviour 13).
    pub fn download_sources(&mut self, cx: &mut Context<Self>) {
        if self.downloading {
            return;
        }
        self.downloading = true;
        cx.notify();
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            let files = idecraft_core::Workspace::open(&root)
                .and_then(|w| w.files())
                .unwrap_or_default();
            let model = idecraft_core::read_project(&root, &files);
            let failures = idecraft_core::jvmsources::download(&root, &model);
            let states = idecraft_core::jvmsources::library_states(&root, &model);
            (failures, idecraft_core::jvmsources::summary(&states))
        });
        cx.spawn(async move |this, cx| {
            let (failures, summary) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.downloading = false;
                let text = match failures {
                    Ok(f) if f.is_empty() => summary,
                    Ok(f) => format!("{summary}; {}", f.join("; ")),
                    Err(e) => e,
                };
                cx.emit(BuildToolsEvent::Status(text));
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether sources and documentation are being downloaded.
    pub fn downloading(&self) -> bool {
        self.downloading
    }

    /// Links build file `file` (workspace-relative) and reads the project again (v3 behaviour 16).
    pub fn link(&mut self, file: &str, cx: &mut Context<Self>) -> Result<(), String> {
        self.options.link(&self.root, file)?;
        self.options.save(&self.root)?;
        self.reload(cx);
        self.resolve(false, cx);
        cx.emit(BuildToolsEvent::ModelChanged);
        cx.emit(BuildToolsEvent::Status(format!("Linked {}", file.trim())));
        Ok(())
    }

    /// Unlinks the selected row's module and the modules inside its folder (v3 behaviour 16).
    pub fn unlink_selected(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.module_of_row(self.selected) else {
            cx.emit(BuildToolsEvent::Status("Select a module to unlink".into()));
            return;
        };
        self.options.unlink(&m.dir);
        if let Err(e) = self.options.save(&self.root) {
            cx.emit(BuildToolsEvent::Status(e));
            return;
        }
        if self.analysis.as_ref().is_some_and(|(d, _, _)| *d == m.dir) {
            self.analysis = None;
        }
        self.selected = 0;
        self.reload(cx);
        cx.emit(BuildToolsEvent::ModelChanged);
        cx.emit(BuildToolsEvent::Status(format!(
            "Unlinked {} ({}); \"+\" links its build file again",
            m.short_name(),
            m.dir
        )));
    }

    /// Asks the workbench for a build file to link, prefilled with the selected module's folder.
    pub fn ask_link(&mut self, cx: &mut Context<Self>) {
        let start = self
            .module_of_row(self.selected)
            .map(|m| {
                if m.dir == "." {
                    String::new()
                } else {
                    format!("{}/", m.dir)
                }
            })
            .unwrap_or_default();
        cx.emit(BuildToolsEvent::LinkPrompt(start));
    }

    /// The options' linked and unlinked lists (self-test).
    pub fn links(&self) -> (Vec<String>, Vec<String>) {
        (self.options.linked.clone(), self.options.unlinked.clone())
    }

    /// The module of row `i`: the nearest module row above it.
    fn module_of_row(&self, i: usize) -> Option<idecraft_core::Module> {
        let rows = self.rows();
        let row = rows.get(i)?;
        let mut depth = row.depth;
        for r in rows[..=i].iter().rev() {
            if let RowKind::Module(_) = r.kind
                && r.depth <= depth
            {
                let index: usize = r.key.strip_prefix('m')?.parse().ok()?;
                return self.entries.get(index).map(|e| e.module.clone());
            }
            depth = depth.min(r.depth);
        }
        None
    }

    /// Asks the workbench for a build command for the selected row's module (behaviour 5).
    pub fn execute(&mut self, cx: &mut Context<Self>) {
        let module = self
            .module_of_row(self.selected)
            .or_else(|| self.entries.first().map(|e| e.module.clone()));
        if let Some(m) = module {
            cx.emit(BuildToolsEvent::Execute(Box::new(m)));
        }
    }

    /// The run configuration for a command line typed for `module` (behaviour 5): `mvn …` and
    /// `gradle …` run in the module's folder (Gradle from its build's root, with the project path
    /// before bare task names), with the window's options; a line without the tool gets it.
    pub fn typed_command(&self, module: &idecraft_core::Module, line: &str) -> Option<Task> {
        let mut words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        if words.is_empty() {
            return None;
        }
        let (dir, run_name) = match module.kind {
            idecraft_core::ModuleKind::Gradle => {
                let build = buildtools::gradle_build(&self.root, module);
                if matches!(words[0].as_str(), "gradle" | "./gradlew" | "gradlew") {
                    words[0] = build.runner.clone();
                } else {
                    words.insert(0, build.runner.clone());
                }
                if !build.path.is_empty() {
                    for w in words.iter_mut().skip(1) {
                        if !w.starts_with('-') && !w.starts_with(':') {
                            *w = format!("{}:{w}", build.path);
                        }
                    }
                }
                (
                    build.root_dir,
                    format!("{} [{}]", build.folder_name, words[1..].join(" ")),
                )
            }
            idecraft_core::ModuleKind::Maven => {
                if !matches!(words[0].as_str(), "mvn" | "./mvnw" | "mvnw") {
                    words.insert(0, "mvn".into());
                }
                (
                    module.dir.clone(),
                    format!("{} [{}]", module.short_name(), words[1..].join(" ")),
                )
            }
            _ => (
                module.dir.clone(),
                format!("{} [{}]", module.name, words.join(" ")),
            ),
        };
        Some(Task {
            label: line.trim().to_owned(),
            test: words.iter().any(|w| w == "test" || w.ends_with(":test")),
            command: words,
            dir,
            run_name,
        })
    }

    /// Runs the task in row `i`, toggles a profile, opens a problem's place, or toggles a row
    /// with children (Enter, double click; behaviour 2).
    pub fn activate(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows().into_iter().nth(i) else {
            return;
        };
        self.selected = i;
        match row.kind {
            RowKind::Task(task) => {
                let config = self.run_config(&task);
                cx.emit(BuildToolsEvent::Run(Box::new(config)));
            }
            RowKind::Profile(id, on) => self.set_profile(&id, !on, cx),
            RowKind::Problem(Some((file, line))) => {
                cx.emit(BuildToolsEvent::Open(self.root.join(file), line))
            }
            // A place of a conflict opens the line of its direct dependency (v3 behaviour 14).
            RowKind::Usage(direct, _) => {
                if let Some(file) = self
                    .analysis
                    .as_ref()
                    .and_then(|(dir, _, _)| build_file_of(&self.root, dir))
                {
                    let line = artifact_line(&self.root, &file, &direct);
                    cx.emit(BuildToolsEvent::Open(self.root.join(&file), line));
                }
            }
            _ => self.toggle(i, cx),
        }
    }

    /// Runs the task named `label` of the first module that has it (for tests).
    pub fn run_task(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        let Some(i) = self
            .rows()
            .iter()
            .position(|r| r.label == label && matches!(r.kind, RowKind::Task(..)))
        else {
            return false;
        };
        self.activate(i, cx);
        true
    }

    /// Carries out a command of the shared tree behaviour (`SPEC-0275` behaviours 8–11): opening
    /// a row runs its task.
    fn apply(&mut self, command: crate::tree_nav::TreeCommand, cx: &mut Context<Self>) {
        use crate::tree_nav::TreeCommand as C;
        match command {
            C::None => {}
            C::Select(i) => self.selected = i,
            C::Expand(i) | C::Collapse(i) => self.toggle(i, cx),
            // Everything below the row (SPEC-0275 v13 behaviour 32).
            C::ExpandFully(i) => {
                crate::tree_nav::fully_expand(self, i, Self::nav_rows, |t, row| t.toggle(row, cx));
            }
            C::Open(i) => self.activate(i, cx),
        }
        cx.notify();
    }

    fn nav_rows(&self) -> Vec<crate::tree_nav::NavRow> {
        self.rows()
            .iter()
            .map(|r| crate::tree_nav::NavRow::new(r.depth, r.expandable, r.expanded))
            .collect()
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The next keystroke becomes the task's shortcut (v3 behaviour 19).
        if self.recording.is_some() {
            let m = &k.modifiers;
            if k.key == "escape" {
                self.recording = None;
                cx.emit(BuildToolsEvent::Status("No shortcut assigned".into()));
            } else if let Some(stroke) = idecraft_ui::keymap::Keystroke::from_platform(
                &k.key, m.control, m.alt, m.shift, m.platform,
            ) {
                self.record(stroke, cx);
            } else {
                // A modifier alone: wait for the key.
                return;
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // v4: with the analyzer open, typing narrows its list; Backspace and Escape undo it.
        if self.analysis.is_some() && !k.modifiers.platform && !k.modifiers.control {
            // Typed text arrives through the platform's text input, not as the key behind it
            // (`SPEC-0004` v8 behaviour 17b): the key is left to it.
            let typed = k
                .key_char
                .as_deref()
                .filter(|c| c.chars().all(|c| !c.is_control() && c != ' ') && !c.is_empty());
            if typed.is_some() {
                return;
            }
            if !self.analysis_filter.is_empty() && (k.key == "backspace" || k.key == "escape") {
                let mut text = self.analysis_filter.clone();
                if k.key == "backspace" {
                    text.pop();
                } else {
                    text.clear();
                }
                self.set_analysis_filter(&text, cx);
                cx.stop_propagation();
                return;
            }
        }
        if k.key == "escape" {
            cx.emit(BuildToolsEvent::Hide);
            cx.stop_propagation();
            return;
        }
        // Space switches the selected profile (behaviour 6).
        if k.key == "space"
            && let Some(Row {
                kind: RowKind::Profile(id, on),
                ..
            }) = self.selected_row()
        {
            self.set_profile(&id, !on, cx);
            cx.stop_propagation();
            return;
        }
        let m = &k.modifiers;
        let platform = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        let others = m.alt || m.shift || (cfg!(target_os = "macos") && m.control);
        // Expanding and collapsing are the keymap's actions (SPEC-0275 v13 behaviour 32).
        let Some(key) = crate::keymaps::tree_action_key(k)
            .or_else(|| crate::tree_nav::tree_key(&k.key, platform, others))
        else {
            return;
        };
        let command = crate::tree_nav::on_key(&self.nav_rows(), self.selected, key);
        self.apply(command, cx);
        cx.stop_propagation();
    }
}

impl BuildToolsView {
    /// The reading or resolving status, Reload, Execute, the offline and skip-tests switches and
    /// Expand/Collapse All before Options in the shared header (SPEC-0061 v5 behaviour 18,
    /// SPEC-0100 v2 behaviours 5–7).
    pub fn header_toolbar(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        let status = if self.downloading {
            Some("Downloading sources…")
        } else if self.resolving {
            Some("Resolving dependencies…")
        } else if self.loading {
            Some("Reading…")
        } else {
            None
        };
        let mut out: Vec<gpui::AnyElement> = status
            .map(|s| {
                div()
                    .mr_2()
                    .text_color(c(p.ui_muted))
                    .child(s)
                    .into_any_element()
            })
            .into_iter()
            .collect();
        out.push(
            crate::metrics::header_button("build-tools-reload")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-reload"
                ))
                .child(icon("general/refresh"))
                // Through the workbench, so the Before/After Sync tasks run too (v4).
                .on_click(
                    cx.listener(|_, _: &gpui::ClickEvent, _, cx| cx.emit(BuildToolsEvent::Sync)),
                )
                .into_any_element(),
        );
        out.push(
            crate::metrics::header_button("build-tools-execute")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-execute"
                ))
                .child(icon("idecraft/execute"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.execute(cx)))
                .into_any_element(),
        );
        let offline = self.options.offline;
        out.push(
            crate::metrics::header_button("build-tools-offline")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-offline"
                ))
                .when(offline, |d| d.bg(c(p.tab_selected)))
                .child(icon("idecraft/offline"))
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.set_offline(!offline, cx)
                }))
                .into_any_element(),
        );
        let skip = self.options.skip_tests;
        out.push(
            crate::metrics::header_button("build-tools-skip-tests")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-skip-tests"
                ))
                .when(skip, |d| d.bg(c(p.tab_selected)))
                .child(icon("idecraft/skipTests"))
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.set_skip_tests(!skip, cx)
                }))
                .into_any_element(),
        );
        out.push(
            crate::metrics::header_button("build-tools-link")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-link"
                ))
                .child(icon("general/add"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.ask_link(cx)))
                .into_any_element(),
        );
        out.push(
            crate::metrics::header_button("build-tools-unlink")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-unlink"
                ))
                .child(icon("idecraft/remove"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.unlink_selected(cx)))
                .into_any_element(),
        );
        out.push(
            crate::metrics::header_button("build-tools-sources")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-sources"
                ))
                .child(icon("idecraft/download"))
                .on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.download_sources(cx)),
                )
                .into_any_element(),
        );
        let analysing = self.analysis.is_some();
        out.push(
            crate::metrics::header_button("build-tools-analyze")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-analyze"
                ))
                .when(analysing, |d| d.bg(c(p.tab_selected)))
                .child(icon("idecraft/dependencyAnalyzer"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_analysis(cx)))
                .into_any_element(),
        );
        out.push(
            crate::metrics::header_button("build-tools-diagram")
                .tip(crate::tips::declared!(
                    OTHER_TOOL_WINDOWS,
                    "build-tools-diagram"
                ))
                .child(icon("idecraft/diagram"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.show_diagram(cx)))
                .into_any_element(),
        );
        for (name, key, open) in [
            ("build-tools-expand", "general/expandAll", true),
            ("build-tools-collapse", "general/collapseAll", false),
        ] {
            let control = if open {
                crate::tips::declared!(OTHER_TOOL_WINDOWS, "build-tools-expand")
            } else {
                crate::tips::declared!(OTHER_TOOL_WINDOWS, "build-tools-collapse")
            };
            out.push(
                crate::metrics::header_button(name)
                    .tip(control)
                    .child(icon(key))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.expand_all(open, cx)
                    }))
                    .into_any_element(),
            );
        }
        out
    }
}

crate::committed_text::committed_text_input!(BuildToolsView, take_committed);

impl Render for BuildToolsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let rows = self.rows();
        let selected = self.selected;
        let skip_tests = self.options.skip_tests;
        div()
            .id("build-tools-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .size_full()
            .flex()
            .flex_col()
            // Title, status and Reload are in the shared header (SPEC-0061 v5 behaviour 18).
            .when(!self.resolve_errors.is_empty(), |d| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(c(p.ui_muted))
                        .children(self.resolve_errors.iter().map(|e| div().child(e.clone()))),
                )
            })
            .child(
                div()
                    .id("build-tools-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .px_1()
                    .when(rows.is_empty() && !self.loading, |d| {
                        d.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_color(c(p.ui_muted))
                                .child("No build files in this project"),
                        )
                    })
                    .children(rows.into_iter().enumerate().map(|(i, r)| {
                        let on = i == selected;
                        let chevron = if !r.expandable {
                            None
                        } else if r.expanded {
                            Some("general/chevronDown")
                        } else {
                            Some("general/chevronRight")
                        };
                        // A skipped test phase is struck through (behaviour 6).
                        let struck = skip_tests
                            && matches!(&r.kind, RowKind::Task(t) if t.test
                                && t.command.first().is_some_and(|c| c.ends_with("mvn")));
                        let (glyph, detail) = match &r.kind {
                            RowKind::Problem(_) => ("status/error", r.detail.clone()),
                            RowKind::Profiles => ("nodes/folder", String::new()),
                            RowKind::Profile(..) => ("", String::new()),
                            RowKind::Module(_) => ("nodes/module", r.detail.clone()),
                            RowKind::Group => ("nodes/folder", r.detail.clone()),
                            RowKind::Task(_) => ("run/run", r.detail.clone()),
                            RowKind::Usage(_, true) => ("nodes/library", r.detail.clone()),
                            RowKind::Usage(_, false) => ("status/warning", r.detail.clone()),
                            RowKind::Dependency(v, scope, repeated, missing) => (
                                if *missing {
                                    "status/warning"
                                } else {
                                    "nodes/library"
                                },
                                format!(
                                    "{}{}{}{}",
                                    v.clone().unwrap_or_default(),
                                    if scope.is_empty() || scope == "normal" {
                                        String::new()
                                    } else {
                                        format!(" · {scope}")
                                    },
                                    if *repeated { " · (*)" } else { "" },
                                    if *missing { " · not found" } else { "" }
                                ),
                            ),
                            // "No version conflicts" is good news, not a warning (v4).
                            RowKind::Note if r.key == "analysis/none" => {
                                ("status/info", String::new())
                            }
                            RowKind::Note => ("status/warning", String::new()),
                        };
                        let checkbox = match &r.kind {
                            RowKind::Profile(_, active) => Some(
                                div()
                                    .flex_none()
                                    .size(px(14.))
                                    .rounded(px(3.))
                                    .border_1()
                                    .border_color(c(p.ui_muted))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.))
                                    .when(*active, |d| d.bg(c(p.tab_selected)).child("✓")),
                            ),
                            _ => None,
                        };
                        div()
                            .id(("build-tools-row", i))
                            .h(px(crate::palette::Palette::current().tree_row_height))
                            .flex()
                            .items_center()
                            .gap_1()
                            .pl(px(crate::metrics::tree_indent(r.depth)))
                            .pr_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(match chevron {
                                // One click on the arrow toggles (SPEC-0275 behaviour 8).
                                Some(k) => div()
                                    .id(("build-tools-arrow", i))
                                    .flex_none()
                                    .child(icon(k))
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, window, cx| {
                                            cx.stop_propagation();
                                            window.focus(&this.focus);
                                            for c in crate::tree_nav::on_click(
                                                &this.nav_rows(),
                                                i,
                                                true,
                                                1,
                                            ) {
                                                this.apply(c, cx);
                                            }
                                        },
                                    ))
                                    .into_any_element(),
                                None => div().w(px(16.)).flex_none().into_any_element(),
                            })
                            .children(checkbox)
                            .when(!glyph.is_empty(), |d| d.child(icon(glyph)))
                            .child(
                                div()
                                    .flex_none()
                                    .when(struck, |d| d.line_through().text_color(c(p.ui_muted)))
                                    .child(r.label.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .text_color(c(if on { p.ui_muted } else { p.ui_dim }))
                                    .child(detail),
                            )
                            // A click selects (a profile's checkbox switches it); a double click
                            // runs a task or toggles a row with children (SPEC-0275 behaviour 9).
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                                window.focus(&this.focus);
                                let clicks = e.click_count().min(2);
                                if clicks == 1
                                    && let Some(Row {
                                        kind: RowKind::Profile(..),
                                        ..
                                    }) = this.rows().into_iter().nth(i)
                                {
                                    this.activate(i, cx);
                                    return;
                                }
                                for c in
                                    crate::tree_nav::on_click(&this.nav_rows(), i, false, clicks)
                                {
                                    this.apply(c, cx);
                                }
                            }))
                            // The context menu of a task (behaviour 10).
                            .map(|d| {
                                crate::mouse::on_secondary_click(
                                    d,
                                    cx.listener(
                                        move |this, e: &gpui::MouseDownEvent, window, cx| {
                                            window.focus(&this.focus);
                                            this.selected = i;
                                            if matches!(
                                                this.selected_row().map(|r| r.kind),
                                                Some(RowKind::Task(_))
                                            ) {
                                                cx.emit(BuildToolsEvent::ContextMenu(e.position));
                                            }
                                            cx.notify();
                                        },
                                    ),
                                )
                            })
                    })),
            )
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use gpui::{EntityInputHandler, Modifiers, TestAppContext, VisualTestContext, px};

    use super::BuildToolsView;

    /// SPEC-0004 v8 AK-13: with the analyzer open, key events of the key `a` committing other
    /// text narrow it by that text (lower case, `ü` included); real keys with Shift go on; ⌫ and
    /// Escape undo it; without the analyzer committed text changes nothing.
    #[gpui::test]
    fn committed_text_narrows_the_analyzer(cx: &mut TestAppContext) {
        let root = std::env::temp_dir().join(format!("idecraft-bt-text-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let at = root.clone();
        let window = cx.add_window(move |_, cx| BuildToolsView::new(&at, cx));
        let view = window.root(cx).unwrap();
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.simulate_resize(gpui::size(px(400.), px(600.)));
        view.update_in(cx, |v, window, _| window.focus(&v.focus));
        cx.run_until_parked();
        let commit = |cx: &mut VisualTestContext, text: &str| {
            cx.simulate_event(gpui::KeyDownEvent {
                keystroke: gpui::Keystroke {
                    modifiers: Modifiers::none(),
                    key: "a".into(),
                    key_char: Some("a".into()),
                },
                is_held: false,
            });
            view.update_in(cx, |v, window, cx| {
                v.replace_text_in_range(None, text, window, cx)
            });
            cx.run_until_parked();
        };
        let filter =
            |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.analysis_filter.clone());
        commit(cx, "j");
        assert_eq!(filter(cx), "", "no analyzer, nothing typed");
        view.update(cx, |v, cx| {
            v.analysis = Some(("app".into(), "app".into(), Vec::new()));
            cx.notify();
        });
        cx.run_until_parked();
        commit(cx, "J");
        commit(cx, "ü");
        assert_eq!(filter(cx), "jü");
        cx.simulate_keystrokes("shift-x");
        cx.run_until_parked();
        assert_eq!(filter(cx), "jüx");
        cx.simulate_keystrokes("backspace");
        cx.run_until_parked();
        assert_eq!(filter(cx), "jü");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(filter(cx), "");
        let _ = std::fs::remove_dir_all(&root);
    }
}
