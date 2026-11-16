//! The build tools window in the workbench (`SPEC-0100` v3): its events (run a task, open a
//! problem, the task menu, a typed build command), the sync badge over a changed build file and
//! ⇧⌘I, which read the project model again and resolve the changed modules; v3: debugging a task,
//! its triggers and Build Project, shortcuts of tasks, linking build files.

use std::path::{Path, PathBuf};

use gpui::{Context, Entity, Window, div, prelude::*, px};

use super::{WorkbenchView, c};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::Tipped;

/// Menu actions of a task row (behaviour 10).
pub const RUN_TASK: &str = "IDECraft.BuildToolsRun";
/// Keeps the task as a run configuration of the project.
pub const SAVE_TASK: &str = "IDECraft.BuildToolsSaveRunConfiguration";
/// Copies the task's command line with the window's options.
pub const COPY_TASK: &str = "IDECraft.BuildToolsCopyCommand";
/// Debugs the task (v3 behaviour 17).
pub const DEBUG_TASK: &str = "IDECraft.BuildToolsDebug";
/// Switches the task's Before Build trigger (v3 behaviour 18).
pub const BEFORE_BUILD_TASK: &str = "IDECraft.BuildToolsBeforeBuild";
/// Switches the task's After Build trigger.
pub const AFTER_BUILD_TASK: &str = "IDECraft.BuildToolsAfterBuild";
/// Switches the task's Before Run trigger.
pub const BEFORE_RUN_TASK: &str = "IDECraft.BuildToolsBeforeRun";
/// Switches the task's Before Sync trigger (v4).
pub const BEFORE_SYNC_TASK: &str = "IDECraft.BuildToolsBeforeSync";
/// Switches the task's After Sync trigger (v4).
pub const AFTER_SYNC_TASK: &str = "IDECraft.BuildToolsAfterSync";
/// The Run tab of the Before Sync tasks (v4).
pub const BEFORE_SYNC_RUN: &str = "Before Sync";
/// The Run tab of the After Sync tasks (v4).
pub const AFTER_SYNC_RUN: &str = "After Sync";
/// Records a shortcut for the task (v3 behaviour 19).
pub const SHORTCUT_TASK: &str = "IDECraft.BuildToolsAssignShortcut";

/// How long a debug run may take to start its JVM before the debugger stops waiting.
/// The Run tab of Build Project (SPEC-0100 v4 behaviour 18).
pub const BUILD_PROJECT_RUN: &str = "Build Project";

const DEBUG_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

impl WorkbenchView {
    /// The build tools window, created (not shown) if it does not exist yet.
    pub fn ensure_build_tools(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<crate::BuildToolsView> {
        if let Some(v) = &self.build_tools {
            return v.clone();
        }
        let root = self.root.clone();
        let view = cx.new(|cx| crate::BuildToolsView::new(&root, cx));
        self._build_tools_events = Some(cx.subscribe_in(
            &view,
            window,
            |this, _, event: &crate::BuildToolsEvent, window, cx| match event {
                crate::BuildToolsEvent::Hide => {
                    this.show_build_tools = false;
                    this.focus_active(window, cx);
                    cx.notify();
                }
                crate::BuildToolsEvent::Run(config) => {
                    this.run_temporary_config((**config).clone(), window, cx);
                }
                crate::BuildToolsEvent::Open(path, line) => {
                    this.open_at_line(path, line.saturating_sub(1), window, cx);
                }
                crate::BuildToolsEvent::Sync => this.sync_with_triggers(true, window, cx),
                crate::BuildToolsEvent::ModelChanged => {
                    let root = this.root.clone();
                    this.tree.reload_roles(&root);
                    cx.notify();
                }
                crate::BuildToolsEvent::Diagram(name, model, mermaid) => {
                    let root = this.root.clone();
                    let (name, model, mermaid) = (name.clone(), (**model).clone(), mermaid.clone());
                    let view = cx.new(|cx| {
                        crate::ErView::dependency_diagram(name, model, mermaid, root, cx)
                    });
                    this.show_er_view(view, window, cx);
                }
                crate::BuildToolsEvent::ContextMenu(at) => {
                    this.open_context_menu(super::MenuPlace::BuildTools, None, *at, cx);
                }
                crate::BuildToolsEvent::Execute(module) => {
                    this.start_build_command((**module).clone(), window, cx);
                }
                crate::BuildToolsEvent::Status(text) => {
                    this.status = Some(text.clone());
                    cx.notify();
                }
                crate::BuildToolsEvent::Shortcut(action, shortcuts) => {
                    if let Err(e) =
                        crate::keymaps::set_shortcuts(action, Some(shortcuts.clone()), cx)
                    {
                        this.status = Some(format!("Keymap not saved: {e}"));
                    }
                    crate::menu_bar::install(cx);
                    cx.notify();
                }
                crate::BuildToolsEvent::LinkPrompt(start) => {
                    this.start_link_prompt(start, window, cx);
                }
            },
        ));
        self.build_tools = Some(view.clone());
        view
    }

    /// Looks at the watched build files (behaviour 9); the first call takes the project's build
    /// files as synced.
    pub(super) fn poll_build_files(&mut self, cx: &mut Context<Self>) {
        if !self.build_sync_ready {
            self.build_sync_ready = true;
            let root = self.root.clone();
            let files = idecraft_core::Workspace::open(&root)
                .and_then(|w| w.files())
                .unwrap_or_default();
            let model = idecraft_core::read_project(&root, &files);
            let watched = idecraft_core::buildtools::sync_files(&root, &model)
                .into_iter()
                .map(|f| f.canonicalize().unwrap_or(f))
                .collect::<Vec<_>>();
            // The Build window gets its stripe button (SPEC-0061 v17 behaviour 65).
            if !watched.is_empty() {
                self.has_build_files = true;
                cx.notify();
            }
            self.build_sync = crate::build_sync_model::BuildSync::new(watched);
            return;
        }
        // A build file opened in the editor that came after the last sync (a new module) is
        // watched from now on.
        if let Some(path) = self
            .active_path()
            .filter(|p| crate::build_sync_model::is_build_file(p))
            && self.build_sync.adopt(&path)
        {
            self.has_build_files = true;
        }
        if self.build_sync.check() {
            cx.notify();
        }
    }

    /// Whether the build file `path` is watched for the sync badge (self-test).
    pub fn build_file_watched(&self, path: &Path) -> bool {
        let path = path.canonicalize().unwrap_or(path.to_path_buf());
        self.build_sync.watches(&path)
    }

    /// Looks at the build files now instead of at the next poll (self-test).
    pub fn check_build_files(&mut self, cx: &mut Context<Self>) {
        self.poll_build_files(cx);
    }

    /// Files changed since the last sync (MCP, self-test).
    pub fn build_changes_pending(&self) -> Vec<PathBuf> {
        self.build_sync.pending()
    }

    /// Whether the editor of `path` shows the sync badge.
    pub fn build_sync_badge_shown(&self, path: &Path) -> bool {
        let path = path.canonicalize().unwrap_or(path.to_path_buf());
        self.build_sync.badge(&path)
    }

    /// Sync Build Changes (⇧⌘I, the badge; behaviour 9): reads the project model again, lets the
    /// build tools resolve the changed modules and takes the build files as synced; the project
    /// tree shows the new modules, folders and libraries.
    pub fn sync_build_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_with_triggers(false, window, cx);
    }

    /// A sync with the tasks bound to it (v4): the Before Sync tasks run first in their Run tab
    /// and the model is read when they succeeded; the After Sync tasks run once the build tools
    /// have resolved the model. `force` resolves every module again (the window's Reload button).
    pub(super) fn sync_with_triggers(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use idecraft_core::buildtasks::{When, trigger_chain};
        let files = idecraft_core::Workspace::open(&self.root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        let model = idecraft_core::read_project(&self.root, &files);
        let options = idecraft_core::buildtools::BuildOptions::load(&self.root);
        let before = trigger_chain(
            &self.root,
            &model,
            &options,
            When::BeforeSync,
            BEFORE_SYNC_RUN,
        );
        let after = trigger_chain(
            &self.root,
            &model,
            &options,
            When::AfterSync,
            AFTER_SYNC_RUN,
        );
        if before.is_empty() {
            self.read_build_files(force, window, cx);
            self.after_sync(after, window, cx);
            return;
        }
        if !self.start_chain(before, window, cx) {
            return;
        }
        self.status = Some("Sync waits for the Before Sync tasks…".into());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(300))
                    .await;
                let Ok(done) = this.update_in(cx, |this, window, cx| {
                    let run = this.run_view.clone()?;
                    let (running, output) = {
                        let r = run.read(cx);
                        (
                            r.running_names().iter().any(|n| n == BEFORE_SYNC_RUN),
                            r.output_of(BEFORE_SYNC_RUN).unwrap_or_default(),
                        )
                    };
                    if running {
                        return None;
                    }
                    if output
                        .lines()
                        .last()
                        .is_some_and(|l| l == "Process finished with exit code 0")
                    {
                        this.read_build_files(force, window, cx);
                        this.after_sync(after.clone(), window, cx);
                    } else {
                        this.status = Some(
                            "Sync stopped: a Before Sync task failed (see its Run tab)".into(),
                        );
                        cx.notify();
                    }
                    Some(())
                }) else {
                    return;
                };
                if done.is_some() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Runs `chain` (its last configuration names the Run tab) without changing the run widget's
    /// choice; false with the problem in the status line when it cannot start.
    fn start_chain(
        &mut self,
        chain: Vec<idecraft_core::RunConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = chain.last().map(|c| c.name.clone()) else {
            return false;
        };
        let names: Vec<String> = chain.iter().map(|c| c.name.clone()).collect();
        self.run_temporary.retain(|t| !names.contains(&t.name));
        self.run_temporary.extend(chain);
        self.reload_run_configs();
        if let Some(problem) = self.run_problem() {
            self.status = Some(problem);
            cx.notify();
            return false;
        }
        self.start_run_named(&tab, window, cx);
        true
    }

    /// Starts the After Sync tasks once the build tools window has read and resolved the model.
    fn after_sync(
        &mut self,
        chain: Vec<idecraft_core::RunConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if chain.is_empty() {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(300))
                    .await;
                let Ok(done) = this.update_in(cx, |this, window, cx| {
                    let busy = this.build_tools.as_ref().is_some_and(|v| {
                        let v = v.read(cx);
                        v.loading() || v.resolving()
                    });
                    if busy {
                        return false;
                    }
                    this.start_chain(chain.clone(), window, cx);
                    true
                }) else {
                    return;
                };
                if done {
                    return;
                }
            }
        })
        .detach();
    }

    /// Reads the build files again (the sync proper, behaviour 9).
    fn read_build_files(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Unsaved build files are saved first: the build tool reads the disk.
        let unsaved = self
            .tabs
            .paths()
            .iter()
            .any(|p| self.build_sync.watches(p) && self.modified(p, cx));
        if unsaved {
            self.save_all_editors(cx);
        }
        let view = self.ensure_build_tools(window, cx);
        view.update(cx, |v, cx| {
            v.reload(cx);
            if !self.safe_mode {
                v.resolve(force, cx);
            }
        });
        self.build_sync_ready = false;
        self.poll_build_files(cx);
        self.tree = self.tree.reloaded();
        self.status = Some(if self.safe_mode {
            "Build files read again; dependencies are not resolved in safe mode".into()
        } else {
            "Syncing build changes…".into()
        });
        cx.notify();
    }

    /// The badge over the editor of a build file that changed since the last sync (behaviour 9).
    pub(super) fn build_sync_badge(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let path = self.active_path()?;
        if !self.build_sync.badge(&path) {
            return None;
        }
        let p = Palette::current();
        Some(
            div()
                .absolute()
                .top(px(44.))
                .right(px(24.))
                .flex()
                .items_center()
                .gap_1()
                .p_1()
                .rounded(px(6.))
                .bg(c(p.popup))
                .border_1()
                .border_color(c(p.separator))
                .child(
                    crate::metrics::header_button("build-sync")
                        .tip(crate::tips::declared!(OTHER_TOOL_WINDOWS, "build-sync"))
                        .child(icon("general/refresh"))
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.sync_build_changes(window, cx)
                        })),
                )
                .child(
                    crate::metrics::header_button("build-sync-dismiss")
                        .tip(crate::tips::declared!(
                            OTHER_TOOL_WINDOWS,
                            "build-sync-dismiss"
                        ))
                        .child(icon("general/closeSmall"))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            if let Some(path) = this.active_path() {
                                this.build_sync.dismiss(&path);
                            }
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    /// The selected task of the build tools window, if a task row is selected.
    fn selected_build_task(&self, cx: &gpui::App) -> Option<idecraft_core::buildtools::Task> {
        let v = self.build_tools.as_ref()?.read(cx);
        match v.selected_row()?.kind {
            crate::build_tools_model::RowKind::Task(t) => Some(*t),
            _ => None,
        }
    }

    /// The menu of a task row (behaviour 10).
    pub(super) fn build_tools_menu(&self, cx: &gpui::App) -> Vec<crate::context_menu::MenuEntry> {
        let triggers = self
            .build_tools
            .as_ref()
            .map(|v| v.read(cx).selected_triggers())
            .unwrap_or_default();
        crate::context_menu::build_task_menu(self.selected_build_task(cx).is_some(), triggers)
    }

    /// Carries out an item of a task row's menu (behaviour 10).
    pub(super) fn build_tools_item(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.selected_build_task(cx) else {
            return;
        };
        let Some(view) = self.build_tools.clone() else {
            return;
        };
        let config = view.read(cx).run_config(&task);
        match action {
            RUN_TASK => self.run_temporary_config(config, window, cx),
            SAVE_TASK => {
                let known = idecraft_core::load_run_configs(&self.root)
                    .configs
                    .iter()
                    .any(|c| c.name == config.name);
                self.status = Some(if known {
                    format!("A run configuration named '{}' exists already", config.name)
                } else {
                    match idecraft_core::append_run_config(&self.root, &config) {
                        Ok(()) => {
                            self.reload_run_configs();
                            self.run_selected = Some(config.name.clone());
                            format!("Saved the run configuration '{}'", config.name)
                        }
                        Err(e) => format!("Could not save the run configuration: {e}"),
                    }
                });
            }
            COPY_TASK => {
                let line = config.command.join(" ");
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(line.clone()));
                self.status = Some(format!("Copied: {line}"));
            }
            DEBUG_TASK => self.debug_build_task(config, window, cx),
            BEFORE_SYNC_TASK | AFTER_SYNC_TASK | BEFORE_BUILD_TASK | AFTER_BUILD_TASK
            | BEFORE_RUN_TASK => {
                let when = match action {
                    BEFORE_SYNC_TASK => idecraft_core::buildtasks::When::BeforeSync,
                    AFTER_SYNC_TASK => idecraft_core::buildtasks::When::AfterSync,
                    BEFORE_BUILD_TASK => idecraft_core::buildtasks::When::BeforeBuild,
                    AFTER_BUILD_TASK => idecraft_core::buildtasks::When::AfterBuild,
                    _ => idecraft_core::buildtasks::When::BeforeRun,
                };
                view.update(cx, |v, cx| v.toggle_trigger(when, cx));
            }
            SHORTCUT_TASK => {
                view.update(cx, |v, cx| v.start_recording(cx));
                window.focus(&view.read(cx).focus());
            }
            _ => {}
        }
        cx.notify();
    }

    /// Debugs a task (v3 behaviour 17): runs it with a JDWP agent on a free port and attaches the
    /// Java debugger each time the run's output reports an agent listening there.
    pub fn debug_build_task(
        &mut self,
        config: idecraft_core::RunConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let port = match std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()) {
            Ok(a) => a.port(),
            Err(e) => {
                self.status = Some(format!("No free port for the debugger: {e}"));
                cx.notify();
                return;
            }
        };
        let debug = match idecraft_core::buildtasks::debug_config(&self.root, &config, port) {
            Ok(c) => c,
            Err(e) => {
                self.status = Some(e);
                cx.notify();
                return;
            }
        };
        let name = debug.name.clone();
        self.run_temporary_config(debug, window, cx);
        self.status = Some(format!(
            "{name}: waiting for the JVM to listen on port {port}…"
        ));
        cx.notify();
        let started = std::time::Instant::now();
        cx.spawn_in(window, async move |this, cx| {
            // Attaches made: one per "Listening" line, the first also when the port is taken
            // (v4). A second attach to a JVM that has its debugger would end the first session.
            let mut attached = 0usize;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                let Ok(step) = this.update_in(cx, |this, window, cx| {
                    let Some(run) = this.run_view.clone() else {
                        return Some(false);
                    };
                    let (output, running) = {
                        let r = run.read(cx);
                        (
                            r.output_of(&name).unwrap_or_default(),
                            r.running_names().contains(&name),
                        )
                    };
                    let count = idecraft_core::buildtasks::listening_count(&output, port);
                    // Surefire 3 keeps the forked JVM's line to itself: the taken port says it.
                    let first = attached == 0 && idecraft_core::buildtasks::port_listening(port);
                    if count > attached || first {
                        attached += 1;
                        this.status = None;
                        this.attach(
                            idecraft_debugger::Runtime::Java,
                            idecraft_debugger::Target::Address {
                                host: "localhost".into(),
                                port,
                            },
                            &name,
                            window,
                            cx,
                        );
                        return None;
                    }
                    if !running {
                        if attached == 0 {
                            this.status = Some(format!("{name}: the task started no JVM to debug"));
                            cx.notify();
                        }
                        return Some(false);
                    }
                    None
                }) else {
                    return;
                };
                if step.is_some() || started.elapsed() > DEBUG_WAIT {
                    return;
                }
            }
        })
        .detach();
    }

    /// Build Project (`CompileDirty`, v3 behaviour 18): the Before Build tasks, the build tools'
    /// compile steps and the After Build tasks as one chain in the Run window.
    pub fn build_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let files = idecraft_core::Workspace::open(&self.root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        let model = idecraft_core::read_project(&self.root, &files);
        let options = idecraft_core::buildtools::BuildOptions::load(&self.root);
        let mut chain =
            idecraft_core::buildtasks::build_project_chain(&self.root, &model, &options);
        if chain.is_empty() {
            self.status = Some("Nothing to build: no Maven, Gradle, Cargo or Go module".into());
            cx.notify();
            return;
        }
        let steps: Vec<String> = chain.iter().map(|c| c.name.clone()).collect();
        // v4: the chain runs in a Run tab of its own, "Build Project", and the run widget keeps
        // the configuration the user chose (the reference's run widget does too, OBS-0024).
        if let Some(last) = chain.last_mut() {
            last.name = BUILD_PROJECT_RUN.into();
        }
        self.run_temporary
            .retain(|t| t.name != BUILD_PROJECT_RUN && !steps.contains(&t.name));
        self.run_temporary.extend(chain);
        self.reload_run_configs();
        if let Some(problem) = self.run_problem() {
            self.status = Some(problem);
            cx.notify();
            return;
        }
        self.start_run_named(BUILD_PROJECT_RUN, window, cx);
        // A build ran: the Build window gets its stripe button (SPEC-0061 v17 behaviour 65).
        self.build_ran = true;
        self.status = Some(format!("Build Project: {}", steps.join(" → ")));
        cx.notify();
    }

    /// Runs the task of keymap action `id` (`BuildTask:<folder>|<task>`, v3 behaviour 19); false
    /// when it names no task of the project.
    pub(super) fn run_task_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let view = self.ensure_build_tools(window, cx);
        let config = view.read(cx).task_of_action(id).or_else(|| {
            // The window may not have read the project yet.
            let (dir, label) = idecraft_core::buildtasks::parse_task_action(id)?;
            let files = idecraft_core::Workspace::open(&self.root)
                .and_then(|w| w.files())
                .unwrap_or_default();
            let model = idecraft_core::read_project(&self.root, &files);
            let m = model.modules.iter().find(|m| m.dir == dir)?;
            let task = idecraft_core::buildtasks::find_task(&self.root, m, &label)?;
            let options = idecraft_core::buildtools::BuildOptions::load(&self.root);
            let profiles = idecraft_core::buildtools::project_profiles(&model);
            Some(idecraft_core::buildtasks::task_config(
                &m.dir, &task, &options, &profiles,
            ))
        });
        match config {
            Some(config) => {
                self.build_ran = true;
                self.run_temporary_config(config, window, cx);
                true
            }
            None => {
                self.status = Some(format!("{id}: no such task in this project"));
                cx.notify();
                false
            }
        }
    }

    /// The Before Run tasks as before-launch steps of the configurations (v3 behaviour 18).
    pub(super) fn add_before_run_tasks(&self, configs: &mut Vec<idecraft_core::RunConfig>) {
        let options = idecraft_core::buildtools::BuildOptions::load(&self.root);
        if !options
            .triggers
            .iter()
            .any(|t| t.when == idecraft_core::buildtasks::When::BeforeRun)
        {
            return;
        }
        let files = idecraft_core::Workspace::open(&self.root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        let model = idecraft_core::read_project(&self.root, &files);
        let tasks = idecraft_core::buildtasks::trigger_configs(
            &self.root,
            &model,
            &options,
            idecraft_core::buildtasks::When::BeforeRun,
        );
        idecraft_core::buildtasks::add_before_run(configs, &tasks);
    }

    /// Runs the command line typed for `module` (behaviour 5).
    pub(super) fn run_build_command(
        &mut self,
        module: &idecraft_core::Module,
        line: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let view = self.ensure_build_tools(window, cx);
        let (task, config) = {
            let v = view.read(cx);
            let task = v
                .typed_command(module, line)
                .ok_or("Type a goal or task, e.g. clean install")?;
            let config = v.run_config(&task);
            (task, config)
        };
        let _ = task;
        self.run_temporary_config(config, window, cx);
        Ok(())
    }
}
