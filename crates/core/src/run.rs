//! Run configurations (`SPEC-0047`).

use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::run_tasks::{BeforeTask, TaskKind};
use crate::testreport::{ReportFormat, TestReport, parse_report};

/// File of the run configurations, relative to the workspace root.
pub const RUN_FILE: &str = ".idecraft/run.toml";
/// Default timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// What a configuration starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunKind {
    /// A program.
    #[default]
    Run,
    /// Tests whose output is a report.
    Test,
}

/// One configuration as written in `run.toml` (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Unique name.
    pub name: String,
    /// Program and arguments.
    pub command: Vec<String>,
    /// Working directory, workspace-relative.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Extra environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Run or test.
    #[serde(default)]
    pub kind: RunKind,
    /// Report format of a test configuration: `libtest`, `tap`, `junit`.
    #[serde(default)]
    pub report: Option<String>,
    /// JUnit file written by the tool, workspace-relative.
    #[serde(default)]
    pub report_file: Option<String>,
    /// Configurations to run first.
    #[serde(default)]
    pub before: Vec<String>,
    /// Timeout in seconds.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// A page to open (`SPEC-0133`): Run opens it in the default browser, Debug in Chrome under
    /// js-debug; with a URL the command may be empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The folder it is listed in (v3 behaviour 7); none: listed on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Several instances may run at once (v3 behaviour 10); else a second start asks.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_multiple: bool,
    /// Tasks before the launch other than configurations (v3 behaviour 7): build, external
    /// tool, open browser.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_launch: Vec<BeforeTask>,
    /// Program, arguments, folder and environment are used as given, without replacing `${…}`
    /// variables (`SPEC-0276` behaviour 6: commands suggested by plugins). Never read from
    /// `run.toml`.
    #[serde(skip)]
    pub verbatim: bool,
}

impl RunConfig {
    /// `text` with variables replaced (behaviour 3), or as given for a verbatim configuration.
    pub(crate) fn expand(&self, text: &str, root: &Path) -> String {
        if self.verbatim {
            text.to_owned()
        } else {
            expand_vars(text, root)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunFile {
    /// The envelope (`SPEC-0303`), checked by [`crate::format::project_format`].
    #[serde(default, rename = "idecraft")]
    _envelope: Option<toml::Table>,
    #[serde(default)]
    run: Vec<RunConfig>,
    /// Templates per type (v4 behaviour 12), read by [`crate::run_edit::parse_run_templates`].
    #[serde(default, rename = "template")]
    _templates: Option<toml::Table>,
}

/// Loaded configurations and the problems found (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunConfigs {
    /// Configurations in file order.
    pub configs: Vec<RunConfig>,
    /// Problems, each naming the configuration.
    pub problems: Vec<String>,
}

pub(crate) fn inside(path: &str) -> bool {
    let p = Path::new(path);
    !p.is_absolute()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Loads and validates `.idecraft/run.toml` of `root`; a missing file means no configurations.
pub fn load_run_configs(root: &Path) -> RunConfigs {
    match std::fs::read_to_string(root.join(RUN_FILE)) {
        Ok(text) => parse_run_configs(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RunConfigs {
            configs: Vec::new(),
            problems: Vec::new(),
        },
        Err(e) => RunConfigs {
            configs: Vec::new(),
            problems: vec![format!("{RUN_FILE}: {e}")],
        },
    }
}

/// Appends `config` to `.idecraft/run.toml` of `root` as text, so comments and order stay
/// (`SPEC-0303` behaviour 12). A file without envelope is kept once as `run.toml.bak` and gains the
/// envelope; a file of a newer format is not touched.
pub fn append_run_config(root: &Path, config: &RunConfig) -> std::io::Result<()> {
    #[derive(Serialize)]
    struct One<'a> {
        run: [&'a RunConfig; 1],
    }
    let file = root.join(RUN_FILE);
    crate::format::prepare_write(&file, "run-configurations")?;
    let old = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut text = if crate::format::project_format(&old, "run-configurations") == Ok(0) {
        let head = crate::format::write(
            &crate::format::Envelope::current("run-configurations"),
            &toml::Table::new(),
        )
        .map_err(std::io::Error::other)?;
        if old.trim().is_empty() {
            head
        } else {
            format!("{head}\n{old}")
        }
    } else {
        old
    };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push('\n');
    text.push_str(&toml::to_string(&One { run: [config] }).map_err(std::io::Error::other)?);
    crate::write_atomic(&file, text)
}

/// Adds `detected` configurations (from the build files, `suggest_run_configs`) after those of
/// `run.toml`, skipping names that are already taken; returns the names added (`SPEC-0087` v2
/// behaviour 7).
pub fn add_detected(configs: &mut RunConfigs, detected: Vec<RunConfig>) -> Vec<String> {
    let mut added = Vec::new();
    for c in detected {
        if configs.configs.iter().all(|o| o.name != c.name) {
            added.push(c.name.clone());
            configs.configs.push(c);
        }
    }
    added
}

/// The configurations written in the text of a `run.toml`, unvalidated; `Err` when the file cannot
/// be read as run configurations (envelope, syntax, unknown keys).
pub(crate) fn read_configs(text: &str) -> Result<Vec<RunConfig>, String> {
    if let Err(p) = crate::format::project_format(text, "run-configurations") {
        return Err(format!("{RUN_FILE}: {}", crate::format::problems_text(&p)));
    }
    toml::from_str::<RunFile>(text)
        .map(|f| f.run)
        .map_err(|e| format!("{RUN_FILE}: {e}"))
}

/// Problems of one configuration's fields other than its name, among `configs` (behaviour 2),
/// each with the field it belongs to (`SPEC-0327` behaviour 4).
pub(crate) fn field_problems(
    c: &RunConfig,
    configs: &[RunConfig],
) -> Vec<(crate::run_edit::RunField, String)> {
    use crate::run_edit::RunField;
    let mut problems = Vec::new();
    let no_command = c.command.is_empty() || c.command[0].trim().is_empty();
    if no_command && c.url.is_none() {
        problems.push((
            RunField::Command,
            format!("{}: the command is empty", c.name),
        ));
    }
    if let Some(u) = &c.url
        && !(u.starts_with("http://") || u.starts_with("https://") || u.starts_with("file://"))
    {
        problems.push((
            RunField::Url,
            format!(
                "{}: the URL must start with http://, https:// or file://",
                c.name
            ),
        ));
    }
    for (field, path) in c
        .cwd
        .iter()
        .map(|p| (RunField::Cwd, p))
        .chain(c.report_file.iter().map(|p| (RunField::Report, p)))
    {
        if !inside(path) {
            problems.push((field, format!("{}: {path} leaves the workspace", c.name)));
        }
    }
    if let Some(r) = &c.report
        && ReportFormat::from_name(r).is_none()
    {
        problems.push((
            RunField::Report,
            format!("{}: unknown report format {r}", c.name),
        ));
    }
    for b in &c.before {
        if !configs.iter().any(|o| &o.name == b) {
            problems.push((
                RunField::Before,
                format!("{}: unknown before-launch configuration {b}", c.name),
            ));
        }
    }
    for (i, t) in c.before_launch.iter().enumerate() {
        if let Some(p) = t.problem() {
            problems.push((
                RunField::Before,
                format!("{}: before-launch task {}: {p}", c.name, i + 1),
            ));
        }
    }
    if let Some(p) = c
        .folder
        .as_deref()
        .and_then(crate::run_edit::folder_name_problem)
    {
        problems.push((RunField::Folder, format!("{}: {p}", c.name)));
    }
    problems
}

/// The before-launch cycle through configuration `name` among `configs`, as text, if there is one.
pub(crate) fn cycle_problem(configs: &[RunConfig], name: &str) -> Option<String> {
    let mut path = vec![name];
    cycle(configs, name, &mut path)
        .then(|| format!("{name}: before-launch cycle {}", path.join(" → ")))
}

/// Parses and validates the text of a `run.toml`.
pub fn parse_run_configs(text: &str) -> RunConfigs {
    let configs = match read_configs(text) {
        Ok(c) => c,
        Err(e) => {
            return RunConfigs {
                configs: Vec::new(),
                problems: vec![e],
            };
        }
    };
    let mut problems = Vec::new();
    let mut names = HashSet::new();
    for c in &configs {
        if c.name.trim().is_empty() {
            problems.push("a configuration has no name".to_owned());
        } else if !names.insert(c.name.as_str()) {
            problems.push(format!("{}: the name is used twice", c.name));
        }
        problems.extend(field_problems(c, &configs).into_iter().map(|(_, p)| p));
    }
    // Cycles in `before`.
    for c in &configs {
        problems.extend(cycle_problem(&configs, &c.name));
    }
    RunConfigs { configs, problems }
}

fn cycle<'a>(configs: &'a [RunConfig], name: &str, path: &mut Vec<&'a str>) -> bool {
    let Some(c) = configs.iter().find(|c| c.name == name) else {
        return false;
    };
    for b in &c.before {
        if path.first() == Some(&b.as_str()) {
            path.push(b);
            return true;
        }
        if path.contains(&b.as_str()) {
            continue;
        }
        path.push(b);
        if cycle(configs, b, path) {
            return true;
        }
        path.pop();
    }
    false
}

/// Replaces `${workspace}` and `${env:NAME}` (behaviour 3).
pub fn expand_vars(text: &str, root: &Path) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        if name == "workspace" {
            out.push_str(&root.to_string_lossy());
        } else if let Some(var) = name.strip_prefix("env:") {
            out.push_str(&std::env::var(var).unwrap_or_default());
        } else {
            out.push_str(&rest[start..start + 2 + end + 1]);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// What one process run gave (behaviour 5).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunOutcome {
    /// Configuration name.
    pub name: String,
    /// Exit code (`None` when killed or not started).
    pub exit_code: Option<i32>,
    /// Whether the timeout killed it.
    pub timed_out: bool,
    /// Whether it was stopped on request (`SPEC-0087` behaviour 5).
    pub stopped: bool,
    /// Why it could not start, if so.
    pub start_error: Option<String>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Duration in milliseconds.
    pub millis: u64,
    /// Parsed test report for test configurations.
    pub report: Option<TestReport>,
    /// Problem reading the report, if any.
    pub report_error: Option<String>,
    /// What "Open browser" did after the start, one line per URL (v3 behaviour 9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub browser: Vec<String>,
}

impl RunOutcome {
    /// Whether the run succeeded (exit code 0, no timeout, started).
    pub fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && self.start_error.is_none()
    }
}

/// Runs one configuration on its own, without its before-launch chain.
pub fn run_one(root: &Path, c: &RunConfig) -> RunOutcome {
    run_one_with(root, c, &AtomicBool::new(false), None, Open::NONE, None)
}

/// What a streaming run reports, in order (`SPEC-0087` behaviour 4).
#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent {
    /// A step starts with this command line (variables replaced).
    Started {
        /// Configuration name.
        name: String,
        /// Program and arguments.
        command: Vec<String>,
    },
    /// One line of output, without its line break.
    Output {
        /// Whether it came from stderr.
        stderr: bool,
        /// The line.
        text: String,
    },
    /// A step ended.
    Finished(Box<RunOutcome>),
    /// The chain ended early because this `before` step failed.
    ChainStopped {
        /// Name of the failed step.
        failed: String,
    },
}

/// Process ids (and process groups) of the programs this process runs right now (`SPEC-0261` v3
/// behaviour 13). An id leaves the list under the lock in the moment its program is reaped, so an
/// id in the list never names a process that is not ours.
static LIVE: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Set while [`end_all_runs`] ends everything: no further step starts.
static ENDING: AtomicBool = AtomicBool::new(false);

/// The ids [`end_all_runs`] signalled and whose runner has not reaped them yet. Taken under the
/// lock of [`LIVE`] (always that lock first), so a program it ended counts as stopped however
/// late its runner gets to reap it — not only while `ENDING` is still set.
static ENDED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn ended_ids() -> MutexGuard<'static, Vec<u32>> {
    ENDED.lock().unwrap_or_else(|e| e.into_inner())
}

fn live() -> MutexGuard<'static, Vec<u32>> {
    // A panic while holding the lock leaves plain data behind; go on with it.
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// How many programs started by run configurations of this process are running now.
pub fn live_runs() -> usize {
    live().len()
}

/// Ends every program started by a run configuration of this process, each with its process group,
/// before the IDE quits (`SPEC-0261` v3 behaviour 13): first asks them to end (SIGTERM), then kills
/// what still runs after `grace` (SIGKILL). Later steps of their chains do not start. Returns how
/// many were running.
pub fn end_all_runs(grace: Duration) -> usize {
    ENDING.store(true, Ordering::SeqCst);
    let count = {
        let ids = live();
        let mut marked = ended_ids();
        for id in ids.iter() {
            if !marked.contains(id) {
                marked.push(*id);
            }
            #[cfg(unix)]
            signal_group(*id, nix::sys::signal::Signal::SIGTERM);
        }
        ids.len()
    };
    let until = Instant::now() + grace;
    while live_runs() > 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    {
        let ids = live();
        #[cfg(unix)]
        for id in ids.iter() {
            signal_group(*id, nix::sys::signal::Signal::SIGKILL);
        }
    }
    // The runners reap what was killed (SIGKILL cannot be ignored, so this is only the runner
    // threads' turn coming, slow on a loaded machine): wait for the list to empty, bounded only
    // against a runner that is stuck. A program reaped later still counts as stopped (`ENDED`).
    let until = Instant::now() + Duration::from_secs(2);
    while live_runs() > 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    ENDING.store(false, Ordering::SeqCst);
    count
}

#[cfg(unix)]
fn signal_group(id: u32, signal: nix::sys::signal::Signal) {
    if let Ok(pid) = i32::try_from(id) {
        // The group was created for the child at spawn, so its id is the child's pid.
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), signal);
    }
}

/// The standard input of the step that runs now, kept open until the end of input is sent or
/// the step ends (`SPEC-0087` v10 behaviour 18).
type StdinSlot = Arc<Mutex<Option<std::process::ChildStdin>>>;

/// A run in the background: its events and a way to stop it.
pub struct RunHandle {
    stop: Arc<AtomicBool>,
    /// Events until the channel closes at the end of the run.
    pub events: Receiver<RunEvent>,
    /// The running step's standard input; `None` for a run that runs elsewhere.
    stdin: Option<StdinSlot>,
}

impl RunHandle {
    /// A handle of a run that runs elsewhere (on the host of a remote project, `SPEC-0318` v2):
    /// `stop` is set by [`RunHandle::stop`], `events` are delivered by whoever follows the run.
    pub fn from_parts(stop: Arc<AtomicBool>, events: Receiver<RunEvent>) -> Self {
        Self {
            stop,
            events,
            stdin: None,
        }
    }

    /// Sends the end of input to the program that runs now: its standard input is closed, so a
    /// program that reads it to the end goes on (`SPEC-0087` v10 behaviour 18). Whether there
    /// was an open input to close: false once it was sent, between two steps, after the run and
    /// for a run that runs elsewhere.
    pub fn send_eof(&self) -> bool {
        self.stdin
            .as_ref()
            .and_then(|slot| slot.lock().unwrap_or_else(|e| e.into_inner()).take())
            .is_some()
    }

    /// Whether the program that runs now still has its standard input open.
    pub fn stdin_open(&self) -> bool {
        self.stdin
            .as_ref()
            .is_some_and(|slot| slot.lock().unwrap_or_else(|e| e.into_inner()).is_some())
    }

    /// Ends the running process and its process group; later steps do not start.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// The flag [`RunHandle::stop`] sets, for stopping the run from another thread while this
    /// handle's events are read (`SPEC-0318` behaviour 21).
    pub fn stopper(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

/// Starts configuration `name` with its before-launch chain in the background (`SPEC-0087`
/// behaviour 3).
pub fn start_run(root: &Path, configs: &RunConfigs, name: &str) -> Result<RunHandle, String> {
    start_run_on(root, configs, name, true)
}

/// [`start_run`] on the host of a remote project (`SPEC-0318`): "Open browser" opens nothing
/// there and says so in the output.
pub fn start_host_run(root: &Path, configs: &RunConfigs, name: &str) -> Result<RunHandle, String> {
    start_run_on(root, configs, name, false)
}

fn start_run_on(
    root: &Path,
    configs: &RunConfigs,
    name: &str,
    here: bool,
) -> Result<RunHandle, String> {
    crate::trust::require_trusted(root)?;
    if !configs.problems.is_empty() {
        return Err(format!(
            "{RUN_FILE} has problems: {}",
            configs.problems.join("; ")
        ));
    }
    let (steps, open) = launch_steps(root, configs, name)?;
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let instance = crate::run_tasks::register_instance(root, name, stop.clone());
    let root = root.to_path_buf();
    let flag = stop.clone();
    // A run on this machine keeps its program's standard input open for `send_eof`; a run on
    // the host of a remote project has nobody who could close it and gets none, as before.
    let stdin: Option<StdinSlot> = here.then(|| Arc::new(Mutex::new(None)));
    let slot = stdin.clone();
    std::thread::spawn(move || {
        // Listed as a running instance until the chain ended (v3 behaviour 10).
        let _instance = instance;
        let last = steps.len().saturating_sub(1);
        for (i, c) in steps.iter().enumerate() {
            let urls: &[BrowserOpen] = if i == last { &open } else { &[] };
            let outcome = run_one_with(
                &root,
                c,
                &flag,
                Some(&tx),
                Open { urls, here },
                slot.as_ref(),
            );
            let ok = outcome.success();
            let stopped = outcome.stopped;
            let _ = tx.send(RunEvent::Finished(Box::new(outcome)));
            if stopped || flag.load(Ordering::SeqCst) || ENDING.load(Ordering::SeqCst) {
                break;
            }
            if !ok && i < last {
                let _ = tx.send(RunEvent::ChainStopped {
                    failed: c.name.clone(),
                });
                break;
            }
        }
    });
    Ok(RunHandle {
        stop,
        events: rx,
        stdin,
    })
}

/// The URLs "Open browser" opens once a step has started (v3 behaviour 9), and whether this
/// machine is where the user looks.
#[derive(Clone, Copy)]
struct Open<'a> {
    urls: &'a [BrowserOpen],
    here: bool,
}

/// A page "Open browser" opens, and the port it waits for first (v4 behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserOpen {
    url: String,
    wait_port: Option<u16>,
}

impl Open<'_> {
    const NONE: Open<'static> = Open {
        urls: &[],
        here: true,
    };
}

/// Runs one configuration; with `events`, reports its start and every output line as it comes.
fn run_one_with(
    root: &Path,
    c: &RunConfig,
    stop: &AtomicBool,
    events: Option<&Sender<RunEvent>>,
    open: Open,
    stdin: Option<&StdinSlot>,
) -> RunOutcome {
    let started = Instant::now();
    let run_started = std::time::SystemTime::now();
    let mut outcome = RunOutcome {
        name: c.name.clone(),
        exit_code: None,
        timed_out: false,
        stopped: false,
        start_error: None,
        stdout: String::new(),
        stderr: String::new(),
        millis: 0,
        report: None,
        report_error: None,
        browser: Vec::new(),
    };
    // Safe mode: an untrusted project runs nothing (SPEC-0278 behaviour 5).
    if let Err(why) = crate::trust::require_trusted(root) {
        outcome.start_error = Some(why);
        return outcome;
    }
    let args = match launch_command(root, c) {
        Ok(args) => args,
        Err(why) => {
            outcome.start_error = Some(format!("{}: {why}", c.name));
            return outcome;
        }
    };
    if let Some(tx) = events {
        let _ = tx.send(RunEvent::Started {
            name: c.name.clone(),
            command: args.clone(),
        });
    }
    let cwd: PathBuf = match &c.cwd {
        // A command a plugin suggested starts only in a folder of the project that no symbolic
        // link leads to, checked now and not when it was suggested (SPEC-0276 v3 behaviour 3).
        Some(d) if c.verbatim => match crate::folder_below(root, Path::new(d)) {
            Ok(folder) => folder,
            Err(why) => {
                outcome.start_error = Some(format!("{}: the folder `{d}` {why}", c.name));
                return outcome;
            }
        },
        Some(d) => root.join(c.expand(d, root)),
        None => root.to_path_buf(),
    };
    // Report files of test runners that do not create their folder (Jest).
    if let Some(f) = c.report_file.as_deref().filter(|_| c.kind == RunKind::Test) {
        let path = root.join(expand_vars(f, root));
        if let Some(parent) = path.parent().filter(|_| path.extension().is_some()) {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..])
        .current_dir(&cwd)
        // With a slot the input stays open until the end of input is sent (`send_eof`);
        // without one the program reads nothing.
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &c.env {
        cmd.env(k, c.expand(v, root));
    }
    // Own process group, so a timeout or stop ends the program together with its children.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = {
        // Under the lock, so ending everything never misses a program that is just starting.
        let mut ids = live();
        if ENDING.load(Ordering::SeqCst) {
            outcome.stopped = true;
            return outcome;
        }
        match cmd.spawn() {
            Ok(child) => {
                ids.push(child.id());
                child
            }
            Err(e) => {
                outcome.start_error = Some(format!("{}: {e}", args[0]));
                return outcome;
            }
        }
    };
    let id = child.id();
    if let Some(slot) = stdin {
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = child.stdin.take();
    }
    // "Open browser" (v3 behaviour 9): once the program has started; a URL that is not opened
    // says why and the run goes on. A page that waits for a port (v4 behaviour 13) is opened from
    // its own thread once the port accepts connections, or not at all when the program ends or
    // the wait runs out first.
    let ended = Arc::new(AtomicBool::new(false));
    let mut waiting = Vec::new();
    for page in open.urls {
        let report = {
            let events = events.cloned();
            move |opened: Result<String, String>| {
                let (stderr, text) = match opened {
                    Ok(text) => (false, text),
                    Err(why) => (true, format!("Open browser: {why}")),
                };
                if let Some(tx) = &events {
                    let _ = tx.send(RunEvent::Output {
                        stderr,
                        text: text.clone(),
                    });
                }
                text
            }
        };
        if !open.here {
            // The host of a remote project has no screen of the user's (v3 behaviour 9).
            let text = report(
                crate::run_tasks::browser_url(root, &page.url)
                    .map(|url| format!("Open browser: {url} is not opened on the host")),
            );
            outcome.browser.push(text);
            continue;
        }
        let open_now = |root: &Path, url: &str| {
            crate::run_tasks::open_browser(root, url)
                .map(|opened| format!("Opened {opened} in the browser"))
        };
        match page.wait_port {
            None => outcome.browser.push(report(open_now(root, &page.url))),
            Some(port) => {
                let (root, url, ended) = (root.to_path_buf(), page.url.clone(), ended.clone());
                waiting.push(std::thread::spawn(move || {
                    let waited = crate::run_tasks::wait_for_local_port(
                        port,
                        &ended,
                        crate::run_tasks::BROWSER_PORT_WAIT,
                    );
                    report(match waited {
                        Ok(()) => open_now(&root, &url),
                        Err(why) => Err(format!("{url} is not opened: {why}")),
                    })
                }));
            }
        }
    }
    // Each pipe is read line by line on its own thread; lines go out as events as they arrive.
    let read = |pipe: Option<Box<dyn Read + Send>>, stderr: bool| {
        let tx = events.cloned();
        std::thread::spawn(move || {
            let mut all = Vec::new();
            if let Some(p) = pipe {
                let mut reader = BufReader::new(p);
                let mut line = Vec::new();
                while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
                    all.extend_from_slice(&line);
                    if let Some(tx) = &tx {
                        let text = String::from_utf8_lossy(&line);
                        let text = text.trim_end_matches(['\n', '\r']).to_owned();
                        let _ = tx.send(RunEvent::Output { stderr, text });
                    }
                    line.clear();
                }
            }
            String::from_utf8_lossy(&all).into_owned()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        false,
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        true,
    );
    let timeout = Duration::from_secs(c.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS));
    // Whether `end_all_runs` was ending everything when the program was reaped, read under the
    // same lock as the id leaves the list: `end_all_runs` clears the flag only after it saw the
    // list empty, so a program it ended is never reported as ended on its own.
    let ending_at_reap = AtomicBool::new(false);
    // Reaps the program (or waits for it) and takes its id off the list in one step.
    let reap = |child: &mut std::process::Child, wait: bool| {
        let mut ids = live();
        let got = if wait {
            child.wait().map(Some)
        } else {
            child.try_wait()
        };
        if !matches!(got, Ok(None)) {
            let mut marked = ended_ids();
            let ended_here = marked.contains(&id);
            marked.retain(|i| *i != id);
            ending_at_reap.store(
                ended_here || ENDING.load(Ordering::SeqCst),
                Ordering::SeqCst,
            );
            ids.retain(|i| *i != id);
        }
        got
    };
    let status = loop {
        match reap(&mut child, false) {
            Ok(Some(status)) => {
                // Ended by `end_all_runs` (a signal): counts as stopped.
                if status.code().is_none() && ending_at_reap.load(Ordering::SeqCst) {
                    outcome.stopped = true;
                    break None;
                }
                break Some(status);
            }
            Ok(None) if stop.load(Ordering::SeqCst) => {
                outcome.stopped = true;
                break stop_gracefully(&mut child, &reap);
            }
            Ok(None) if started.elapsed() >= timeout => {
                kill_tree(&mut child);
                let _ = reap(&mut child, true);
                outcome.timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                live().retain(|i| *i != id);
                outcome.start_error = Some(e.to_string());
                break None;
            }
        }
    };
    // The step ended: its input is closed, whatever still holds the pipe sees the end.
    if let Some(slot) = stdin {
        slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
    outcome.exit_code = status.and_then(exit_code_of);
    ended.store(true, Ordering::SeqCst);
    for w in waiting {
        outcome.browser.extend(w.join().ok());
    }
    outcome.stdout = out.join().unwrap_or_default();
    outcome.stderr = err.join().unwrap_or_default();
    outcome.millis = started.elapsed().as_millis() as u64;
    if c.kind == RunKind::Test {
        let format = c.report.as_deref().and_then(ReportFormat::from_name);
        let parsed = match &c.report_file {
            Some(f) => {
                let path = root.join(expand_vars(f, root));
                if path.is_dir() {
                    // A folder of JUnit files, one per test class (Maven Surefire, Gradle); only
                    // those written by this run count (SPEC-0097 behaviour 2).
                    crate::testreport::read_junit_dir(&path, run_started)
                        .map_err(|e| format!("{f}: {e}"))
                } else if crate::testreport::written_since(&path, run_started) {
                    std::fs::read_to_string(&path)
                        .map_err(|e| format!("{f}: {e}"))
                        .and_then(|t| parse_report(&t, format).map_err(|e| e.to_string()))
                } else {
                    // A report left from an earlier run is not this run's result.
                    Err(format!("{f}: no test report written by this run"))
                }
            }
            None => parse_report(&outcome.stdout, format).map_err(|e| e.to_string()),
        };
        match parsed {
            Ok(r) => outcome.report = Some(r),
            Err(e) => outcome.report_error = Some(e),
        }
    }
    outcome
}

/// How long a stopped program and its process group may take to end after SIGTERM
/// (`SPEC-0087` v4 behaviour 5).
pub const STOP_GRACE: Duration = Duration::from_secs(3);

/// The exit code of `status`; a program ended by a signal reports 128 + the signal, as shells do.
fn exit_code_of(status: std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.code().or_else(|| status.signal().map(|s| 128 + s))
    }
    #[cfg(not(unix))]
    {
        status.code()
    }
}

/// Ends a program on the user's request: SIGTERM to its process group, SIGKILL to what still runs
/// after [`STOP_GRACE`]. Returns how the program ended.
fn stop_gracefully(
    child: &mut std::process::Child,
    reap: &impl Fn(&mut std::process::Child, bool) -> std::io::Result<Option<std::process::ExitStatus>>,
) -> Option<std::process::ExitStatus> {
    let mut status = None;
    #[cfg(unix)]
    {
        let id = child.id();
        signal_group(id, nix::sys::signal::Signal::SIGTERM);
        let until = Instant::now() + STOP_GRACE;
        while Instant::now() < until {
            if status.is_none() {
                status = reap(child, false).ok().flatten();
            }
            if status.is_some() && !group_alive(id) {
                return status;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    kill_tree(child);
    if status.is_none() {
        status = reap(child, true).ok().flatten();
    }
    status
}

/// Whether a process of the group of `id` still exists.
#[cfg(unix)]
fn group_alive(id: u32) -> bool {
    i32::try_from(id)
        .is_ok_and(|pid| nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), None).is_ok())
}

/// Kills the process and everything in its process group.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // The group was created for the child at spawn, so its id is the child's pid.
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    let _ = child.kill();
}

/// Runs configuration `name` after its before-launch chain (behaviour 4); returns every run in order.
pub fn run_config(
    root: &Path,
    configs: &RunConfigs,
    name: &str,
) -> Result<Vec<RunOutcome>, String> {
    if !configs.problems.is_empty() {
        return Err(format!(
            "{RUN_FILE} has problems: {}",
            configs.problems.join("; ")
        ));
    }
    let (steps, open) = launch_steps(root, configs, name)?;
    let stop = Arc::new(AtomicBool::new(false));
    let _instance = crate::run_tasks::register_instance(root, name, stop.clone());
    let last = steps.len().saturating_sub(1);
    let mut outcomes = Vec::new();
    for (i, c) in steps.iter().enumerate() {
        let urls: &[BrowserOpen] = if i == last { &open } else { &[] };
        let outcome = run_one_with(root, c, &stop, None, Open { urls, here: true }, None);
        let ok = outcome.success();
        outcomes.push(outcome);
        if !ok {
            break;
        }
    }
    Ok(outcomes)
}

/// One step of a run as it would happen (dry run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedRun {
    /// Configuration name.
    pub name: String,
    /// Command with variables replaced.
    pub command: Vec<String>,
    /// Working directory, workspace-relative.
    pub cwd: String,
    /// Timeout in seconds.
    pub timeout: u64,
    /// URLs "Open browser" opens once this step has started (v3 behaviour 9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub opens: Vec<String>,
    /// Those of `opens` that wait for a port of this machine first, with the port (v4
    /// behaviour 13).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub opens_after_port: Vec<(String, u16)>,
}

/// The command a configuration runs: its own, or for a page without one the system's way of
/// opening a URL in the default browser (`SPEC-0133` behaviour 1).
pub fn effective_command(c: &RunConfig) -> Vec<String> {
    match (&c.url, c.command.first()) {
        (Some(url), None) => {
            let open = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            vec![open.to_owned(), url.clone()]
        }
        _ => c.command.clone(),
    }
}

/// The command a configuration starts: [`effective_command`] with variables replaced and a leading
/// bare `python`/`python3` replaced by the workspace's selected interpreter (`SPEC-0050` v2 behaviour
/// 8; not for a configuration run as given). Err: why it cannot start.
pub fn launch_command(root: &Path, c: &RunConfig) -> Result<Vec<String>, String> {
    let args: Vec<String> = effective_command(c)
        .iter()
        .map(|a| c.expand(a, root))
        .collect();
    if c.verbatim {
        return Ok(args);
    }
    let path_var = c
        .env
        .get("PATH")
        .map(|p| std::ffi::OsString::from(c.expand(p, root)));
    Ok(crate::python_manage::python_command(root, &args, path_var)?.unwrap_or(args))
}

/// What [`run_config`] would start, in order, with variables replaced and the Python interpreter
/// resolved — nothing is run. Err also when a step could not start.
pub fn run_plan(root: &Path, configs: &RunConfigs, name: &str) -> Result<Vec<PlannedRun>, String> {
    let (steps, open) = launch_steps(root, configs, name)?;
    let last = steps.len().saturating_sub(1);
    steps
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let pages: &[BrowserOpen] = if i == last { &open } else { &[] };
            let opens = pages
                .iter()
                .map(|p| crate::run_tasks::browser_url(root, &p.url))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|why| format!("{}: {why}", c.name))?;
            let opens_after_port = pages
                .iter()
                .zip(&opens)
                .filter_map(|(p, url)| p.wait_port.map(|port| (url.clone(), port)))
                .collect();
            Ok(PlannedRun {
                name: c.name.clone(),
                command: launch_command(root, c).map_err(|why| format!("{}: {why}", c.name))?,
                cwd: c
                    .cwd
                    .as_deref()
                    .map(|d| c.expand(d, root))
                    .unwrap_or_else(|| ".".into()),
                timeout: c.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS),
                opens,
                opens_after_port,
            })
        })
        .collect()
}

/// The steps of a run of configuration `name` in order (behaviour 4, v3 behaviour 8) and the URLs
/// to open once the last one, the configuration itself, has started: before each configuration
/// its `before` configurations (each once), then its "Build" and "Run external tool" tasks in
/// the order written. "Open browser" tasks count for the configuration that was started, not for
/// those run before it.
fn launch_steps(
    root: &Path,
    configs: &RunConfigs,
    name: &str,
) -> Result<(Vec<RunConfig>, Vec<BrowserOpen>), String> {
    let mut steps = Vec::new();
    let mut seen = Vec::new();
    let mut build = None;
    collect(root, configs, name, &mut seen, &mut steps, &mut build)?;
    let open = configs
        .configs
        .iter()
        .find(|c| c.name == name)
        .map(|c| {
            c.before_launch
                .iter()
                .filter_map(|t| {
                    t.browser.clone().map(|url| BrowserOpen {
                        url,
                        wait_port: t.wait_port,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((steps, open))
}

/// A step that is not a configuration of the file: runs as given, with nothing before it.
fn task_step(name: String, command: Vec<String>, cwd: Option<String>) -> RunConfig {
    RunConfig {
        name,
        command,
        cwd,
        env: BTreeMap::new(),
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
    }
}

/// Depth-first order of `name`, its before-launch configurations and their tasks, each
/// configuration and each build step once.
fn collect(
    root: &Path,
    configs: &RunConfigs,
    name: &str,
    seen: &mut Vec<String>,
    steps: &mut Vec<RunConfig>,
    build: &mut Option<Vec<RunConfig>>,
) -> Result<(), String> {
    let c = configs
        .configs
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| format!("no run configuration {name}"))?;
    if seen.iter().any(|n| n == name) {
        return Ok(());
    }
    seen.push(name.to_owned());
    for b in &c.before {
        collect(root, configs, b, seen, steps, build)?;
    }
    for (i, t) in c.before_launch.iter().enumerate() {
        let task = |why: String| format!("{name}: before-launch task {}: {why}", i + 1);
        match t.kind().map_err(task)? {
            TaskKind::Build => {
                if build.is_none() {
                    *build = Some(crate::buildtasks::before_launch_build(root).map_err(task)?);
                    steps.extend(build.iter().flatten().cloned());
                }
            }
            TaskKind::Tool(command, cwd) => {
                let program = Path::new(&command[0])
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| command[0].clone());
                steps.push(task_step(
                    format!("{name} · {program}"),
                    command.to_vec(),
                    cwd.map(str::to_owned),
                ));
            }
            // Opened after the start (behaviour 9).
            TaskKind::Browser(_) => {}
        }
    }
    steps.push(c.clone());
    Ok(())
}
