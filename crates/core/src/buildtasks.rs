//! What a task of the build tools window offers besides running it (`SPEC-0100` v3): its run
//! configuration, debugging it with a JDWP agent (behaviour 17), running it before or after Build
//! Project or before every run (behaviour 18), a keymap action of its own (behaviour 19), and the
//! suggestions of the Execute field (behaviour 20).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::buildtools::{BuildOptions, MAVEN_PHASES, Task, gradle_build, task_groups};
use crate::project::{Module, ModuleKind, ProjectModel};
use crate::run::{RunConfig, RunKind};

/// When a task runs by itself (behaviour 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    /// Before the build files are synced (v4): the model is read when they succeeded.
    BeforeSync,
    /// After a sync read the model (v4).
    AfterSync,
    /// Before Build Project's build steps.
    BeforeBuild,
    /// After Build Project's build steps succeeded.
    AfterBuild,
    /// Before every run configuration started from the run widget.
    BeforeRun,
}

impl When {
    /// All, in the order of the task menu.
    pub const ALL: [When; 5] = [
        When::BeforeSync,
        When::AfterSync,
        When::BeforeBuild,
        When::AfterBuild,
        When::BeforeRun,
    ];

    /// As the task row and the menu name it.
    pub fn label(self) -> &'static str {
        match self {
            When::BeforeSync => "Before Sync",
            When::AfterSync => "After Sync",
            When::BeforeBuild => "Before Build",
            When::AfterBuild => "After Build",
            When::BeforeRun => "Before Run",
        }
    }

    /// `before_sync`, `after_sync`, `before_build`, `after_build`, `before_run`.
    pub fn id(self) -> &'static str {
        match self {
            When::BeforeSync => "before_sync",
            When::AfterSync => "after_sync",
            When::BeforeBuild => "before_build",
            When::AfterBuild => "after_build",
            When::BeforeRun => "before_run",
        }
    }
}

/// A task bound to Build Project or to every run, by its module's folder and its label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    /// When it runs.
    pub when: When,
    /// The module's folder, workspace-relative (`.` for the root).
    pub module: String,
    /// The task's label (`clean`, `compiler:compile`).
    pub task: String,
}

impl BuildOptions {
    /// Whether task `task` of the module in `module` runs `when`.
    pub fn has_trigger(&self, when: When, module: &str, task: &str) -> bool {
        self.triggers
            .iter()
            .any(|t| t.when == when && t.module == module && t.task == task)
    }

    /// Binds task `task` of the module in `module` to `when`, or takes it off.
    pub fn set_trigger(&mut self, when: When, module: &str, task: &str, on: bool) {
        self.triggers
            .retain(|t| !(t.when == when && t.module == module && t.task == task));
        if on {
            self.triggers.push(Trigger {
                when,
                module: module.to_owned(),
                task: task.to_owned(),
            });
        }
    }

    /// The labels of the triggers of a task, for its row (`Before Build`).
    pub fn trigger_labels(&self, module: &str, task: &str) -> Vec<&'static str> {
        When::ALL
            .into_iter()
            .filter(|w| self.has_trigger(*w, module, task))
            .map(When::label)
            .collect()
    }
}

/// The task labelled `label` among the task groups of module `m`.
pub fn find_task(root: &Path, m: &Module, label: &str) -> Option<Task> {
    task_groups(root, m)
        .iter()
        .flat_map(|g| g.all_tasks())
        .find(|t| t.label == label)
        .cloned()
}

/// The program's file name (`mvn` for `/x/mvnw` is `mvnw`).
fn program_name(command: &[String]) -> String {
    command
        .first()
        .map(|p| {
            Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

/// The run configuration of `task` of the module in folder `module_dir` (`SPEC-0100` v2
/// behaviour 2): its run name, its command with the window's options, its folder; a test task
/// reports to the test tree.
pub fn task_config(
    module_dir: &str,
    task: &Task,
    options: &BuildOptions,
    profiles: &[(String, bool)],
) -> RunConfig {
    let command = options.apply(&task.command, profiles);
    let in_module = |p: &str| {
        if module_dir == "." {
            p.to_owned()
        } else {
            format!("{module_dir}/{p}")
        }
    };
    let report_file = (task.test && !options.skip_tests)
        .then(|| match program_name(&task.command).as_str() {
            "mvn" | "mvnw" => Some(in_module("target/surefire-reports")),
            "gradle" | "gradlew" => Some(in_module("build/test-results/test")),
            _ => None,
        })
        .flatten();
    RunConfig {
        name: task.run_name.clone(),
        command,
        cwd: (task.dir != ".").then(|| task.dir.clone()),
        env: Default::default(),
        kind: if report_file.is_some() {
            RunKind::Test
        } else {
            RunKind::Run
        },
        report: report_file.as_ref().map(|_| "junit".to_owned()),
        report_file,
        before: Vec::new(),
        timeout: None,
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: false,
    }
}

/// Prefix of the keymap actions of tasks (behaviour 19).
pub const TASK_ACTION_PREFIX: &str = "BuildTask:";

/// The keymap action of task `label` of the module in folder `module_dir`:
/// `BuildTask:<folder>|<task>`.
pub fn task_action_id(module_dir: &str, label: &str) -> String {
    format!("{TASK_ACTION_PREFIX}{module_dir}|{label}")
}

/// The module folder and task label of a task's keymap action.
pub fn parse_task_action(id: &str) -> Option<(String, String)> {
    let (dir, label) = id.strip_prefix(TASK_ACTION_PREFIX)?.split_once('|')?;
    (!dir.is_empty() && !label.is_empty()).then(|| (dir.to_owned(), label.to_owned()))
}

/// The JDWP agent that waits on `port` of this machine until a debugger attaches (STD).
pub fn jdwp_agent(port: u16) -> String {
    format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:{port}")
}

/// Whether `output` shows the JDWP agent listening on `port` as often as `times` (each JVM of a
/// run prints it once when it starts).
pub fn agent_listening(output: &str, port: u16, times: usize) -> bool {
    listening_count(output, port) >= times
}

/// Whether a local process listens on `port` (SPEC-0100 v4 behaviour 17): binding it on the
/// loopback address fails because it is taken. Nothing connects, so a JDWP agent that accepts a
/// single debugger is not used up. Surefire 3 does not pass the agent's "Listening" line of its
/// forked test JVM on to the console, so the port is the only sign there.
pub fn port_listening(port: u16) -> bool {
    ["127.0.0.1", "[::1]"].iter().any(|host| {
        matches!(
            std::net::TcpListener::bind(format!("{host}:{port}")),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse
        )
    })
}

/// How often `output` reports a JDWP agent listening on `port`.
pub fn listening_count(output: &str, port: u16) -> usize {
    output
        .matches(&format!(
            "Listening for transport dt_socket at address: {port}"
        ))
        .count()
}

/// The init script that puts the agent into a Gradle run's `Test` and `JavaExec` tasks
/// (behaviour 17).
pub const GRADLE_DEBUG_SCRIPT: &str = r#"// IDECraft: debugs the test and application JVMs of a Gradle run (SPEC-0100 v3 behaviour 17).
def port = gradle.startParameter.projectProperties['idecraftDebugPort']
if (port) {
    def agent = "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:${port}".toString()
    allprojects {
        tasks.withType(Test).configureEach {
            jvmArgs agent
            maxParallelForks = 1
            forkEvery = 0
        }
        tasks.withType(JavaExec).configureEach {
            jvmArgs agent
        }
    }
}
"#;

/// Writes init script `name` with `text` to `.idecraft/gradle/` of `root` and returns its path.
pub fn write_init_script(
    root: &Path,
    name: &str,
    text: &str,
) -> Result<std::path::PathBuf, String> {
    let file = root.join(".idecraft/gradle").join(name);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(text) {
        std::fs::create_dir_all(file.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
        crate::write_atomic(&file, text.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(file)
}

/// Maven phases and goals after which the tests have run.
fn runs_maven_tests(args: &[String]) -> bool {
    let skipped = args
        .iter()
        .any(|a| a == "-DskipTests" || a.starts_with("-Dmaven.test.skip"));
    !skipped
        && args.iter().any(|a| {
            matches!(
                a.as_str(),
                "test" | "package" | "verify" | "install" | "deploy" | "surefire:test"
            ) || a.starts_with("failsafe:")
        })
}

/// `config` (a task's run configuration) as a debug run with the agent on `port` (behaviour 17):
/// Maven test runs give it to the forked test JVMs, other goals to the Maven JVM; Gradle runs get
/// the init script of [`GRADLE_DEBUG_SCRIPT`]. Other programs cannot be debugged this way.
pub fn debug_config(root: &Path, config: &RunConfig, port: u16) -> Result<RunConfig, String> {
    let mut out = config.clone();
    let program = program_name(&config.command);
    let agent = jdwp_agent(port);
    match program.as_str() {
        "mvn" | "mvnw" | "mvn.cmd" | "mvnw.cmd" => {
            if runs_maven_tests(&config.command[1..]) {
                out.command
                    .insert(1, format!("-Dmaven.surefire.debug={agent}"));
                out.command
                    .insert(2, format!("-Dmaven.failsafe.debug={agent}"));
            } else {
                let before = config
                    .env
                    .get("MAVEN_OPTS")
                    .cloned()
                    .or_else(|| std::env::var("MAVEN_OPTS").ok())
                    .unwrap_or_default();
                let opts = if before.trim().is_empty() {
                    agent
                } else {
                    format!("{} {agent}", before.trim())
                };
                out.env.insert("MAVEN_OPTS".into(), opts);
            }
        }
        "gradle" | "gradlew" | "gradle.bat" | "gradlew.bat" => {
            let script = write_init_script(root, "idecraft-debug.gradle", GRADLE_DEBUG_SCRIPT)?;
            out.command.insert(1, "--init-script".into());
            out.command.insert(2, script.display().to_string());
            out.command.insert(3, format!("-PidecraftDebugPort={port}"));
        }
        _ => {
            return Err(format!(
                "{}: only Maven goals and Gradle tasks can be debugged",
                config.name
            ));
        }
    }
    Ok(out)
}

/// The run configurations of the tasks bound to `when` that still exist, in the order they were
/// bound (behaviour 18).
pub fn trigger_configs(
    root: &Path,
    model: &ProjectModel,
    options: &BuildOptions,
    when: When,
) -> Vec<RunConfig> {
    let profiles = crate::buildtools::project_profiles(model);
    let mut out: Vec<RunConfig> = Vec::new();
    for t in options.triggers.iter().filter(|t| t.when == when) {
        let Some(m) = model.modules.iter().find(|m| m.dir == t.module) else {
            continue;
        };
        let Some(task) = find_task(root, m, &t.task) else {
            continue;
        };
        let config = task_config(&m.dir, &task, options, &profiles);
        if !out.iter().any(|c| c.name == config.name) {
            out.push(config);
        }
    }
    out
}

/// The modules no other module of the same kind lists as a member.
fn top_modules(model: &ProjectModel, kind: ModuleKind) -> Vec<&Module> {
    model
        .modules
        .iter()
        .filter(|m| m.kind == kind)
        .filter(|m| {
            !model
                .modules
                .iter()
                .any(|p| p.kind == kind && p.dir != m.dir && p.members.contains(&m.dir))
        })
        .collect()
}

/// The build steps of Build Project (behaviour 18): `mvn test-compile` for each top Maven module,
/// `gradle classes testClasses` for each Gradle build's root, `cargo build` for each top Cargo
/// module, `go build ./...` for each Go module; with the window's options.
pub fn build_steps(root: &Path, model: &ProjectModel, options: &BuildOptions) -> Vec<RunConfig> {
    let profiles = crate::buildtools::project_profiles(model);
    let mut tasks: Vec<Task> = Vec::new();
    for m in top_modules(model, ModuleKind::Maven) {
        tasks.push(Task {
            label: "test-compile".into(),
            command: vec!["mvn".into(), "test-compile".into()],
            dir: m.dir.clone(),
            run_name: format!("{} [test-compile]", m.short_name()),
            test: false,
        });
    }
    let mut builds: Vec<String> = Vec::new();
    for m in model
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Gradle)
    {
        let build = gradle_build(root, m);
        if builds.contains(&build.root_dir) {
            continue;
        }
        builds.push(build.root_dir.clone());
        tasks.push(Task {
            label: "classes testClasses".into(),
            command: vec![build.runner.clone(), "classes".into(), "testClasses".into()],
            dir: build.root_dir.clone(),
            run_name: format!("{} [classes testClasses]", build.folder_name),
            test: false,
        });
    }
    for m in top_modules(model, ModuleKind::Cargo) {
        tasks.push(Task {
            label: "build".into(),
            command: vec!["cargo".into(), "build".into()],
            dir: m.dir.clone(),
            run_name: format!("{} [build]", m.short_name()),
            test: false,
        });
    }
    for m in model.modules.iter().filter(|m| m.kind == ModuleKind::Go) {
        tasks.push(Task {
            label: "build".into(),
            command: vec!["go".into(), "build".into(), "./...".into()],
            dir: m.dir.clone(),
            run_name: format!("{} [build]", m.short_name()),
            test: false,
        });
    }
    tasks
        .iter()
        .map(|t| task_config(&t.dir, t, options, &profiles))
        .collect()
}

/// The step of Recompile (`Compile`, `SPEC-0100` v5 behaviour 22) for `file` (absolute or
/// workspace-relative): the build of the innermost Maven, Gradle, Cargo or Go module whose folder
/// holds it, with the window's build `options`. `None` for a file outside every such module.
pub fn recompile_step(
    root: &Path,
    model: &ProjectModel,
    options: &BuildOptions,
    file: &Path,
) -> Option<RunConfig> {
    // A file outside the workspace is in none of its modules.
    let rel = match file.strip_prefix(root) {
        Ok(rel) => rel,
        Err(_) if file.is_absolute() => return None,
        Err(_) => file,
    };
    let rel = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    let holds = |dir: &str| dir == "." || rel.starts_with(&format!("{dir}/"));
    let module = model
        .modules
        .iter()
        .filter(|m| {
            matches!(
                m.kind,
                ModuleKind::Maven | ModuleKind::Gradle | ModuleKind::Cargo | ModuleKind::Go
            )
        })
        .filter(|m| holds(&m.dir))
        .max_by_key(|m| if m.dir == "." { 0 } else { m.dir.len() + 1 })?;
    let name = module.short_name();
    let task = match module.kind {
        ModuleKind::Maven => Task {
            label: "compile".into(),
            command: vec!["mvn".into(), "compile".into()],
            dir: module.dir.clone(),
            run_name: format!("{name} [compile]"),
            test: false,
        },
        ModuleKind::Gradle => {
            let build = gradle_build(root, module);
            let label = if build.path.is_empty() {
                "classes".to_owned()
            } else {
                format!("{}:classes", build.path)
            };
            Task {
                command: vec![build.runner.clone(), label.clone()],
                dir: build.root_dir.clone(),
                run_name: format!("{name} [{label}]"),
                label,
                test: false,
            }
        }
        ModuleKind::Cargo => Task {
            label: "build".into(),
            command: vec!["cargo".into(), "build".into()],
            dir: module.dir.clone(),
            run_name: format!("{name} [build]"),
            test: false,
        },
        _ => {
            let folder = rel
                .rsplit_once('/')
                .map_or(".".to_owned(), |(d, _)| d.to_owned());
            Task {
                label: "build .".into(),
                command: vec!["go".into(), "build".into(), ".".into()],
                run_name: format!("{name} [build {folder}]"),
                dir: folder,
                test: false,
            }
        }
    };
    let profiles = crate::buildtools::project_profiles(model);
    Some(task_config(&module.dir, &task, options, &profiles))
}

/// The steps of the before-launch task "Build" for the project at `root` (`SPEC-0047` v3
/// behaviour 8): the build steps of Build Project ([`build_steps`]: Maven, Gradle, Cargo, Go, with
/// the build tool window's options) and, for every package with a script named `build`, that
/// script with the package's package manager. `Err` when the project model knows no build.
pub fn before_launch_build(root: &Path) -> Result<Vec<RunConfig>, String> {
    let files = crate::Workspace::open(root)
        .and_then(|ws| ws.files())
        .map_err(|e| e.to_string())?;
    let model = crate::project::read_project(root, &files);
    let mut steps = build_steps(root, &model, &BuildOptions::load(root));
    for m in model
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Npm && m.tasks.contains_key("build"))
    {
        let dir = root.join(&m.dir);
        let text = std::fs::read_to_string(dir.join("package.json")).unwrap_or_default();
        let mut step = crate::node::package_script_config(&dir, &m.dir, &text, "build");
        if m.dir != "." {
            step.name = format!("{} ({})", step.name, m.dir);
        }
        steps.push(step);
    }
    if steps.is_empty() {
        return Err(
            "the project has no build IDECraft knows (Cargo, Maven, Gradle, Go, or a package \
             script named build)"
                .into(),
        );
    }
    Ok(steps)
}

/// The tasks bound to `when` as one chain in a Run tab named `tab` (v4: Before Sync, After Sync):
/// the last carries the others as before-launch steps, so the chain stops at the first failure.
/// Empty when no task is bound.
pub fn trigger_chain(
    root: &Path,
    model: &ProjectModel,
    options: &BuildOptions,
    when: When,
    tab: &str,
) -> Vec<RunConfig> {
    let mut chain = trigger_configs(root, model, options, when);
    let names: Vec<String> = chain.iter().map(|c| c.name.clone()).collect();
    if let Some(last) = chain.last_mut() {
        last.name = tab.to_owned();
        last.before = names[..names.len() - 1].to_vec();
    }
    chain
}

/// Build Project as one chain (behaviour 18): the Before Build tasks, the build steps, the After
/// Build tasks. The last configuration has all others as before-launch steps, so the chain stops
/// at the first failure. Empty when there is nothing to build or run.
pub fn build_project_chain(
    root: &Path,
    model: &ProjectModel,
    options: &BuildOptions,
) -> Vec<RunConfig> {
    let mut chain = trigger_configs(root, model, options, When::BeforeBuild);
    for step in build_steps(root, model, options) {
        if !chain.iter().any(|c| c.name == step.name) {
            chain.push(step);
        }
    }
    for mut after in trigger_configs(root, model, options, When::AfterBuild) {
        // The same task before and after the build runs twice.
        if chain.iter().any(|c| c.name == after.name) {
            after.name = format!("{} · after build", after.name);
        }
        chain.push(after);
    }
    let names: Vec<String> = chain.iter().map(|c| c.name.clone()).collect();
    if let Some(last) = chain.last_mut() {
        last.before = names[..names.len() - 1].to_vec();
    }
    chain
}

/// Puts the Before Run tasks `tasks` before every configuration of `configs` that is not one of
/// them, and adds them to `configs` (behaviour 18).
pub fn add_before_run(configs: &mut Vec<RunConfig>, tasks: &[RunConfig]) {
    if tasks.is_empty() {
        return;
    }
    let names: Vec<String> = tasks.iter().map(|t| t.name.clone()).collect();
    for c in configs.iter_mut() {
        if names.contains(&c.name) {
            continue;
        }
        let mut before = names.clone();
        before.extend(c.before.iter().filter(|b| !names.contains(b)).cloned());
        c.before = before;
    }
    for t in tasks {
        if !configs.iter().any(|c| c.name == t.name) {
            configs.push(t.clone());
        }
    }
}

/// One suggestion of the Execute field (behaviour 20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    /// The word it puts on the line.
    pub text: String,
    /// `phase`, `goal`, `task`, `option` or `profile`.
    pub kind: &'static str,
    /// What it is, shown dimmed.
    pub detail: String,
}

/// Most suggestions shown.
pub const SUGGESTION_LIMIT: usize = 10;

const MAVEN_OPTIONS: &[(&str, &str)] = &[
    ("-o", "offline: only the local repository"),
    ("-U", "update snapshots and releases"),
    ("-DskipTests", "compile the tests without running them"),
    (
        "-Dmaven.test.skip=true",
        "neither compile nor run the tests",
    ),
    ("-P", "profiles: -P a,!b"),
    ("-pl", "only these projects: -pl core,app"),
    ("-am", "also make the projects they need"),
    ("-amd", "also make the projects that need them"),
    ("-N", "this project only, not its modules"),
    ("-T", "threads: -T 1C"),
    ("-X", "debug output"),
    ("-e", "error stack traces"),
    ("-q", "errors only"),
    ("-B", "batch mode"),
    ("-fae", "fail at the end"),
    ("-rf", "resume from a project: -rf :core"),
    ("-f", "the POM to use"),
    ("-s", "the user settings file"),
];

const GRADLE_OPTIONS: &[(&str, &str)] = &[
    ("--offline", "only what the caches hold"),
    ("--refresh-dependencies", "check every dependency again"),
    ("--continue", "go on after a failing task"),
    ("-x", "leave a task out: -x test"),
    ("--rerun-tasks", "run every task, up to date or not"),
    ("--info", "more output"),
    ("--debug", "debug output"),
    ("--stacktrace", "stack traces of errors"),
    ("--parallel", "projects in parallel"),
    ("--dry-run", "list the tasks without running them"),
    ("--build-cache", "use the build cache"),
    ("--no-daemon", "without the Gradle daemon"),
    ("--console=plain", "plain output"),
    ("--warning-mode=all", "every deprecation warning"),
    ("-q", "errors only"),
];

/// Everything the Execute field can suggest for module `m`.
fn vocabulary(root: &Path, m: &Module) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    let mut add = |text: String, kind: &'static str, detail: String| {
        if !out.iter().any(|s| s.text == text) {
            out.push(Suggestion { text, kind, detail });
        }
    };
    let groups = task_groups(root, m);
    match m.kind {
        ModuleKind::Maven => {
            for p in MAVEN_PHASES {
                add(p.to_owned(), "phase", "lifecycle phase".into());
            }
            for g in groups.iter().filter(|g| g.name == "Plugins") {
                for plugin in &g.groups {
                    for t in &plugin.tasks {
                        add(
                            t.label.clone(),
                            "goal",
                            format!("goal of {}", plugin.detail),
                        );
                    }
                }
            }
            for (o, d) in MAVEN_OPTIONS {
                add((*o).to_owned(), "option", (*d).to_owned());
            }
        }
        ModuleKind::Gradle => {
            for g in &groups {
                for t in &g.tasks {
                    add(t.label.clone(), "task", "task".into());
                }
                for sub in &g.groups {
                    for t in &sub.tasks {
                        add(t.label.clone(), "task", format!("task · {}", sub.name));
                    }
                }
            }
            for (o, d) in GRADLE_OPTIONS {
                add((*o).to_owned(), "option", (*d).to_owned());
            }
        }
        _ => {
            for g in &groups {
                for t in g.all_tasks() {
                    add(t.label.clone(), "task", "task".into());
                }
            }
        }
    }
    out
}

/// The suggestions for the word at the end of `line` in the Execute field of module `m`
/// (behaviour 20): after `-P` the project's profiles, for a word starting with `-` the options,
/// else phases, goals and tasks; words already on the line are left out, names starting with the
/// word come before those containing it; at most [`SUGGESTION_LIMIT`].
pub fn command_suggestions(
    root: &Path,
    m: &Module,
    profiles: &[(String, bool)],
    line: &str,
) -> Vec<Suggestion> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let ends_open = line.is_empty() || line.ends_with(char::is_whitespace);
    let (word, before): (&str, &[&str]) = if ends_open {
        ("", &words[..])
    } else {
        (
            words.last().copied().unwrap_or(""),
            &words[..words.len().saturating_sub(1)],
        )
    };
    // The first word is the build tool itself.
    if before.is_empty() && !ends_open {
        return Vec::new();
    }
    let used: Vec<&str> = before.iter().skip(1).copied().collect();
    let candidates: Vec<Suggestion> = if before.last() == Some(&"-P") {
        // A comma list: complete the part after the last comma, keep the rest.
        let (head, part) = match word.rsplit_once(',') {
            Some((h, p)) => (format!("{h},"), p),
            None => (String::new(), word),
        };
        let part = part.trim_start_matches('!');
        return profiles
            .iter()
            .filter(|(id, _)| id.to_lowercase().contains(&part.to_lowercase()))
            .map(|(id, default)| Suggestion {
                text: format!("{head}{id}"),
                kind: "profile",
                detail: if *default {
                    "profile, active by default".into()
                } else {
                    "profile".into()
                },
            })
            .take(SUGGESTION_LIMIT)
            .collect();
    } else {
        vocabulary(root, m)
            .into_iter()
            .filter(|s| (s.kind == "option") == word.starts_with('-'))
            .filter(|s| !used.contains(&s.text.as_str()))
            .collect()
    };
    let lower = word.to_lowercase();
    let mut starts: Vec<Suggestion> = Vec::new();
    let mut contains: Vec<Suggestion> = Vec::new();
    for s in candidates {
        let t = s.text.to_lowercase();
        if t.starts_with(&lower) {
            starts.push(s);
        } else if t.contains(&lower) {
            contains.push(s);
        }
    }
    starts.extend(contains);
    starts.truncate(SUGGESTION_LIMIT);
    starts
}

/// `line` with its last word replaced by `text` and a space after it (behaviour 20).
pub fn complete_line(line: &str, text: &str) -> String {
    let keep = if line.ends_with(char::is_whitespace) {
        line
    } else {
        match line.rfind(char::is_whitespace) {
            Some(i) => &line[..=i],
            None => "",
        }
    };
    format!("{keep}{text} ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("idecraft-buildtasks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn model(root: &Path) -> ProjectModel {
        let files = crate::Workspace::open(root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        crate::read_project(root, &files)
    }

    const POM: &str = r#"<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId><artifactId>shop</artifactId><version>1</version><profiles><profile><id>dev</id><activation><activeByDefault>true</activeByDefault></activation></profile><profile><id>ci</id></profile></profiles></project>"#;

    /// SPEC-0100 v5 AK-20: Recompile builds the innermost module of the file with its tool.
    #[test]
    fn recompile_steps_build_the_files_module() {
        let root = tmp("recompile");
        let write = |p: &str, text: &str| {
            let path = root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "shop/pom.xml",
            r#"<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId><artifactId>shop</artifactId><version>1</version></project>"#,
        );
        write("shop/src/main/java/A.java", "class A {}\n");
        write("gr/settings.gradle", "include 'lib'\n");
        write("gr/build.gradle", "\n");
        write("gr/lib/build.gradle", "\n");
        write("gr/lib/src/main/java/B.java", "class B {}\n");
        write("gr/src/main/java/C.java", "class C {}\n");
        write(
            "tool/Cargo.toml",
            "[package]\nname = \"tool\"\nversion = \"0.1.0\"\n",
        );
        write("tool/src/main.rs", "fn main() {}\n");
        write("svc/go.mod", "module example.com/svc\n\ngo 1.22\n");
        write("svc/api/h.go", "package api\n");
        write("notes/readme.txt", "x\n");
        let model = model(&root);
        let options = BuildOptions::default();
        let step = |file: &str| {
            recompile_step(&root, &model, &options, &root.join(file))
                .map(|c| (c.name, c.command, c.cwd))
        };
        assert_eq!(
            step("shop/src/main/java/A.java"),
            Some((
                "shop [compile]".into(),
                vec!["mvn".into(), "compile".into()],
                Some("shop".into())
            ))
        );
        let (name, command, cwd) = step("gr/lib/src/main/java/B.java").unwrap();
        assert_eq!(command, ["gradle", ":lib:classes"], "{name}");
        assert_eq!(cwd.as_deref(), Some("gr"));
        let (_, command, cwd) = step("gr/src/main/java/C.java").unwrap();
        assert_eq!(command, ["gradle", "classes"]);
        assert_eq!(cwd.as_deref(), Some("gr"));
        assert_eq!(
            step("tool/src/main.rs"),
            Some((
                "tool [build]".into(),
                vec!["cargo".into(), "build".into()],
                Some("tool".into())
            ))
        );
        let (_, command, cwd) = step("svc/api/h.go").unwrap();
        assert_eq!(command, ["go", "build", "."]);
        assert_eq!(cwd.as_deref(), Some("svc/api"));
        // A workspace-relative path names the same file.
        assert_eq!(
            recompile_step(&root, &model, &options, Path::new("tool/src/main.rs")).map(|c| c.name),
            Some("tool [build]".into())
        );
        assert_eq!(step("notes/readme.txt"), None);
        // A crate at the root holds every file below it.
        let flat = tmp("recompile-root");
        std::fs::create_dir_all(flat.join("src")).unwrap();
        std::fs::write(
            flat.join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(flat.join("src/main.rs"), "fn main() {}\n").unwrap();
        let model = super::tests::model(&flat);
        let dirs: Vec<_> = model
            .modules
            .iter()
            .map(|m| (m.dir.clone(), m.kind))
            .collect();
        let found = recompile_step(&flat, &model, &options, &flat.join("src/main.rs"));
        assert_eq!(
            found.map(|c| (c.name, c.cwd)),
            Some(("shop [build]".to_owned(), None)),
            "{dirs:?}"
        );
        // A file outside the workspace is in none of its modules.
        let outside = std::env::temp_dir().join("idecraft-recompile-outside.txt");
        assert_eq!(recompile_step(&flat, &model, &options, &outside), None);
    }

    /// SPEC-0100 v3 AK-11: phases, goals, options after `-`, profiles after `-P`, words already
    /// on the line left out, prefix before infix, completing the last word.
    #[test]
    fn execute_suggestions() {
        let root = tmp("suggest");
        std::fs::write(root.join("pom.xml"), POM).unwrap();
        let model = model(&root);
        let m = &model.modules[0];
        let profiles = crate::buildtools::project_profiles(&model);
        let texts = |line: &str| -> Vec<String> {
            command_suggestions(&root, m, &profiles, line)
                .into_iter()
                .map(|s| s.text)
                .collect()
        };
        assert!(texts("mv").is_empty(), "the tool's own word");
        let all = texts("mvn ");
        assert_eq!(all[0], "clean");
        assert_eq!(all.len(), SUGGESTION_LIMIT);
        let comp = command_suggestions(&root, m, &profiles, "mvn comp");
        assert_eq!(comp[0].text, "compile");
        assert_eq!(comp[0].kind, "phase");
        assert!(
            comp.iter()
                .any(|s| s.text == "compiler:compile" && s.kind == "goal")
        );
        // Prefix matches first, then those that contain the word.
        let test = texts("mvn test");
        assert_eq!(test[0], "test");
        assert!(test.iter().any(|t| t == "compiler:testCompile"));
        assert!(
            test.iter().position(|t| t == "test").unwrap()
                < test.iter().position(|t| t == "surefire:test").unwrap()
        );
        assert!(
            !texts("mvn clean cl").contains(&"clean".to_owned()),
            "already on the line"
        );
        let options = command_suggestions(&root, m, &profiles, "mvn clean -D");
        assert_eq!(options[0].text, "-DskipTests");
        assert_eq!(options[0].kind, "option");
        assert_eq!(texts("mvn package -P "), ["ci", "dev"]);
        assert_eq!(texts("mvn package -P ci,d"), ["ci,dev"]);
        assert_eq!(complete_line("mvn comp", "compile"), "mvn compile ");
        assert_eq!(complete_line("mvn clean ", "install"), "mvn clean install ");
        // Gradle: the common tasks before the build listed its own, and Gradle's options.
        std::fs::create_dir_all(root.join("g")).unwrap();
        std::fs::write(root.join("g/build.gradle.kts"), "plugins { java }\n").unwrap();
        let model = self::model(&root);
        let g = model.modules.iter().find(|m| m.dir == "g").unwrap();
        let tasks = command_suggestions(&root, g, &[], "gradle te");
        assert_eq!(tasks[0].text, "test");
        let off = command_suggestions(&root, g, &[], "gradle build --of");
        assert_eq!(off[0].text, "--offline");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v4 AK-14: a port a process listens on counts as the agent listening.
    #[test]
    fn a_taken_port_is_listening() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(port_listening(port));
        drop(listener);
        assert!(!port_listening(port));
    }

    /// SPEC-0100 v3 AK-14: Maven test runs debug the forked JVM, other goals the Maven JVM; Gradle
    /// runs get the init script and the port; the agent's line is recognised.
    #[test]
    fn debug_commands() {
        let root = tmp("debug");
        let config = |command: &[&str]| RunConfig {
            name: "shop [x]".into(),
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
        };
        let test = debug_config(&root, &config(&["mvn", "-o", "test"]), 5123).unwrap();
        assert_eq!(
            test.command[1],
            "-Dmaven.surefire.debug=-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:5123"
        );
        assert!(test.command[2].starts_with("-Dmaven.failsafe.debug="));
        assert_eq!(&test.command[3..], ["-o", "test"]);
        assert!(!test.env.contains_key("MAVEN_OPTS"));
        let exec = debug_config(&root, &config(&["mvn", "exec:java"]), 5124).unwrap();
        assert_eq!(exec.command, ["mvn", "exec:java"]);
        assert!(exec.env["MAVEN_OPTS"].ends_with("address=localhost:5124"));
        let skipped =
            debug_config(&root, &config(&["mvn", "-DskipTests", "package"]), 5125).unwrap();
        assert!(
            skipped.env.contains_key("MAVEN_OPTS"),
            "no tests run: the Maven JVM"
        );
        let gradle = debug_config(&root, &config(&["./gradlew", ":lib:test"]), 5126).unwrap();
        assert_eq!(gradle.command[1], "--init-script");
        let script = std::path::PathBuf::from(&gradle.command[2]);
        assert!(script.ends_with(".idecraft/gradle/idecraft-debug.gradle"));
        assert!(
            std::fs::read_to_string(&script)
                .unwrap()
                .contains("tasks.withType(Test)")
        );
        assert_eq!(gradle.command[3], "-PidecraftDebugPort=5126");
        assert_eq!(gradle.command[4], ":lib:test");
        assert!(debug_config(&root, &config(&["npm", "test"]), 1).is_err());
        let out = "[INFO] T E S T S\nListening for transport dt_socket at address: 5123\n";
        assert!(agent_listening(out, 5123, 1));
        assert!(!agent_listening(out, 5123, 2));
        assert!(!agent_listening(out, 5000, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v3 AK-16: a task's keymap action and back.
    #[test]
    fn task_action_ids() {
        let id = task_action_id("core", "compiler:compile");
        assert_eq!(id, "BuildTask:core|compiler:compile");
        assert_eq!(
            parse_task_action(&id),
            Some(("core".to_owned(), "compiler:compile".to_owned()))
        );
        assert_eq!(parse_task_action("BuildTask:.|test").unwrap().0, ".");
        assert!(parse_task_action("SaveAll").is_none());
        assert!(parse_task_action("BuildTask:|x").is_none());
        let root = tmp("action");
        std::fs::write(root.join("pom.xml"), POM).unwrap();
        let model = model(&root);
        let task = find_task(&root, &model.modules[0], "compiler:compile").unwrap();
        assert_eq!(task.command, ["mvn", "compiler:compile"]);
        assert!(find_task(&root, &model.modules[0], "nope").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v3 AK-15: triggers are kept and shown; Build Project chains Before Build, the
    /// build steps per tool and After Build; Before Run tasks go before other configurations.
    #[test]
    fn triggers_and_build_project_chain() {
        let root = tmp("chain");
        std::fs::write(
            root.join("pom.xml"),
            r#"<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId><artifactId>shop</artifactId><version>1</version><packaging>pom</packaging><modules><module>core</module></modules></project>"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("core")).unwrap();
        std::fs::write(
            root.join("core/pom.xml"),
            r#"<project><modelVersion>4.0.0</modelVersion><parent><groupId>lab</groupId><artifactId>shop</artifactId><version>1</version></parent><artifactId>shop-core</artifactId></project>"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("tool")).unwrap();
        std::fs::write(
            root.join("tool/Cargo.toml"),
            "[package]\nname = \"tool\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let mut options = BuildOptions::default();
        options.set_trigger(When::BeforeBuild, "core", "clean", true);
        options.set_trigger(When::AfterBuild, "core", "install", true);
        options.set_trigger(When::BeforeRun, "core", "validate", true);
        options.set_trigger(When::BeforeRun, "gone", "clean", true);
        options.offline = true;
        options.save(&root).unwrap();
        let options = BuildOptions::load(&root);
        assert!(options.has_trigger(When::BeforeBuild, "core", "clean"));
        assert_eq!(options.trigger_labels("core", "clean"), ["Before Build"]);
        let model = model(&root);
        let chain = build_project_chain(&root, &model, &options);
        let names: Vec<&str> = chain.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "shop-core [clean]",
                "shop [test-compile]",
                "tool [build]",
                "shop-core [install]"
            ]
        );
        assert_eq!(chain[1].command, ["mvn", "-o", "test-compile"]);
        assert_eq!(chain[2].command, ["cargo", "build"]);
        assert_eq!(chain[2].cwd.as_deref(), Some("tool"));
        assert_eq!(chain[3].before, names[..3]);
        // A task that no longer exists is skipped.
        let before_run = trigger_configs(&root, &model, &options, When::BeforeRun);
        assert_eq!(before_run.len(), 1);
        let mut configs = vec![RunConfig {
            before: vec!["prep".into()],
            ..before_run[0].clone()
        }];
        configs[0].name = "app".into();
        add_before_run(&mut configs, &before_run);
        assert_eq!(configs[0].before, ["shop-core [validate]", "prep"]);
        assert_eq!(configs[1].name, "shop-core [validate]");
        assert!(configs[1].before.is_empty(), "not before itself");
        let mut options = options;
        options.set_trigger(When::BeforeBuild, "core", "clean", false);
        assert!(options.trigger_labels("core", "clean").is_empty());
        // v4: tasks before and after a sync, as one chain in their own tab.
        options.set_trigger(When::BeforeSync, "core", "clean", true);
        options.set_trigger(When::BeforeSync, "core", "validate", true);
        options.set_trigger(When::AfterSync, "core", "install", true);
        options.save(&root).unwrap();
        let options = BuildOptions::load(&root);
        assert_eq!(
            options.trigger_labels("core", "validate"),
            ["Before Sync", "Before Run"]
        );
        let before = trigger_chain(&root, &model, &options, When::BeforeSync, "Before Sync");
        let names: Vec<&str> = before.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["shop-core [clean]", "Before Sync"]);
        assert_eq!(before[1].command, ["mvn", "-o", "validate"]);
        assert_eq!(before[1].before, ["shop-core [clean]"]);
        let after = trigger_chain(&root, &model, &options, When::AfterSync, "After Sync");
        assert_eq!(after.len(), 1);
        assert_eq!(
            (after[0].name.as_str(), after[0].before.len()),
            ("After Sync", 0)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
