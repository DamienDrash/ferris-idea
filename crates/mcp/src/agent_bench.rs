//! The agent benchmark (`SPEC-0345`, ROADMAP M19.11): tasks of our own, each a small project with
//! deterministic checks, run through the real MCP server. A recorded run replays a tool script
//! without a model, so the suite runs in CI without credentials and fails when a change breaks a
//! tool agents use. Live agents come with the agent host of M19.5; the report stays the same.
//! The tool `agent_benchmark` has its own router and row of the effects table
//! ([`TOOL_EFFECTS`]): `build.rs` finds both (ADR-0035).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ferris_idea_core::{AgentPermissions, Grant, Workspace};
use rmcp::ServiceExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};

use crate::call::{Answer, interpret};
use crate::effects::{Change, Dry, RUN, Reach, TRUSTED, ToolEffect, tool};
use crate::{IdeServer, json_result};

/// The row of `agent_benchmark` in the effects table (`SPEC-0053` v8 behaviour 13): it starts
/// processes in a trusted project and needs the `run` right.
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("agent_benchmark", &[Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN, TRUSTED]),
];

/// Where the repository keeps its tasks (behaviour 1).
pub const TASKS_DIR: &str = "tests/agent-benchmark";
/// Bounds of `max_tool_calls` and its default.
pub const TOOL_CALLS: (u32, u32, u32) = (1, 500, 50);
/// Bounds of `max_seconds` and its default.
pub const SECONDS: (u64, u64, u64) = (1, 3600, 300);
/// Longest time of a `command` check (behaviour 2).
pub const COMMAND_SECONDS: u64 = 60;

/// The rights a task may grant a run (behaviour 1).
fn grant(name: &str) -> Option<Grant> {
    match name {
        "write" => Some(Grant::Write),
        "run" => Some(Grant::Run),
        "vcs-write" => Some(Grant::VcsWrite),
        _ => None,
    }
}

/// `task.toml` as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskFile {
    id: String,
    title: String,
    prompt: String,
    #[serde(default)]
    grants: Vec<String>,
    max_tool_calls: Option<u32>,
    max_seconds: Option<u64>,
    #[serde(default)]
    check: Vec<CheckFile>,
}

/// One `[[check]]` as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckFile {
    file: Option<String>,
    contains: Option<String>,
    absent: Option<String>,
    equals: Option<toml::Value>,
    tool: Option<String>,
    args: Option<toml::Table>,
    pointer: Option<String>,
    command: Option<Vec<String>>,
    exit: Option<i32>,
}

/// A check (behaviour 2).
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    /// The file holds the text.
    Contains {
        /// Path in the project.
        file: String,
        /// The text.
        text: String,
    },
    /// The file does not hold the text, or is missing.
    Absent {
        /// Path in the project.
        file: String,
        /// The text.
        text: String,
    },
    /// The file equals a file of the task folder.
    Equals {
        /// Path in the project.
        file: String,
        /// The expected file.
        expected: PathBuf,
    },
    /// A tool's answer has `value` at `pointer`.
    Tool {
        /// Tool name.
        tool: String,
        /// Its arguments.
        args: Value,
        /// JSON pointer into the answer.
        pointer: String,
        /// The value expected there.
        value: Value,
    },
    /// A command exits with `exit`.
    Command {
        /// Program and arguments.
        argv: Vec<String>,
        /// The expected exit code.
        exit: i32,
    },
}

impl Check {
    fn kind(&self) -> &'static str {
        match self {
            Check::Contains { .. } | Check::Absent { .. } | Check::Equals { .. } => "file",
            Check::Tool { .. } => "tool",
            Check::Command { .. } => "command",
        }
    }
}

/// A task (behaviour 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    /// The folder's name.
    pub id: String,
    /// Title.
    pub title: String,
    /// What an agent is told.
    pub prompt: String,
    /// The rights of a run.
    pub grants: Vec<Grant>,
    /// Most tool calls of a run.
    pub max_tool_calls: u32,
    /// Longest run.
    pub max_seconds: u64,
    /// The checks.
    pub checks: Vec<Check>,
    /// The task folder.
    pub dir: PathBuf,
    /// The recorded runs by name, sorted.
    pub scripts: Vec<String>,
}

/// A recorded run as written (behaviour 3).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// Agent name for the report.
    pub agent: String,
    /// Model name for the report; `none` for a hand-written script.
    pub model: String,
    /// What the run cost, when known.
    pub cost_cents: Option<u64>,
    /// The calls in order.
    #[serde(default)]
    pub call: Vec<ScriptCall>,
}

/// One call of a recorded run.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptCall {
    /// Tool name.
    pub tool: String,
    /// Arguments.
    #[serde(default)]
    pub args: toml::Table,
    /// `ok` (default) or `error`.
    pub expect: Option<String>,
}

/// Whether `rel` is a relative path that stays inside its folder.
fn inside(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn toml_to_json(v: toml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Reads the task in `dir` (behaviours 1 and 2).
pub fn load_task(dir: &Path) -> Result<Task, String> {
    let file = dir.join("task.toml");
    let shown = file.display().to_string();
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{shown}: {e}"))?;
    let t: TaskFile = toml::from_str(&text).map_err(|e| format!("{shown}: {}", e.message()))?;
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bad = |m: String| format!("{shown}: {m}");
    if t.id != folder {
        return Err(bad(format!(
            "id {:?} is not the folder's name {folder:?}",
            t.id
        )));
    }
    if t.id.is_empty() || !t.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(bad("the id has letters, digits and '-' only".into()));
    }
    let mut grants = Vec::new();
    for g in &t.grants {
        grants.push(grant(g).ok_or_else(|| {
            bad(format!(
                "grant {g:?}: a task grants write, run or vcs-write"
            ))
        })?);
    }
    let max_tool_calls = t.max_tool_calls.unwrap_or(TOOL_CALLS.2);
    if !(TOOL_CALLS.0..=TOOL_CALLS.1).contains(&max_tool_calls) {
        return Err(bad(format!(
            "max_tool_calls is {} to {}",
            TOOL_CALLS.0, TOOL_CALLS.1
        )));
    }
    let max_seconds = t.max_seconds.unwrap_or(SECONDS.2);
    if !(SECONDS.0..=SECONDS.1).contains(&max_seconds) {
        return Err(bad(format!(
            "max_seconds is {} to {}",
            SECONDS.0, SECONDS.1
        )));
    }
    if t.check.is_empty() {
        return Err(bad("a task needs at least one [[check]]".into()));
    }
    let mut checks = Vec::new();
    for (i, c) in t.check.into_iter().enumerate() {
        checks.push(check(dir, c).map_err(|m| bad(format!("check {}: {m}", i + 1)))?);
    }
    if !dir.join("project").is_dir() {
        return Err(bad("the task has no project/ folder".into()));
    }
    let mut scripts: Vec<String> = std::fs::read_dir(dir.join("scripts"))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .strip_suffix(".toml")
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    scripts.sort();
    Ok(Task {
        id: t.id,
        title: t.title,
        prompt: t.prompt,
        grants,
        max_tool_calls,
        max_seconds,
        checks,
        dir: dir.to_path_buf(),
        scripts,
    })
}

fn check(dir: &Path, c: CheckFile) -> Result<Check, String> {
    let kinds = [c.file.is_some(), c.tool.is_some(), c.command.is_some()]
        .iter()
        .filter(|k| **k)
        .count();
    if kinds != 1 {
        return Err("a check has exactly one of file, tool and command".into());
    }
    if let Some(file) = c.file {
        if !inside(&file) {
            return Err(format!("{file:?} is not a path inside the project"));
        }
        if c.tool.is_some() || c.args.is_some() || c.pointer.is_some() || c.exit.is_some() {
            return Err("a file check takes contains, absent or equals".into());
        }
        return match (c.contains, c.absent, c.equals) {
            (Some(text), None, None) => Ok(Check::Contains { file, text }),
            (None, Some(text), None) => Ok(Check::Absent { file, text }),
            (None, None, Some(toml::Value::String(rel))) if inside(&rel) => {
                let expected = dir.join(&rel);
                if !expected.is_file() {
                    return Err(format!("the expected file {rel} is missing"));
                }
                Ok(Check::Equals { file, expected })
            }
            (None, None, Some(_)) => Err("equals names a file of the task folder".into()),
            _ => Err("a file check takes one of contains, absent and equals".into()),
        };
    }
    if let Some(tool) = c.tool {
        if c.contains.is_some() || c.absent.is_some() || c.exit.is_some() {
            return Err("a tool check takes args, pointer and equals".into());
        }
        let pointer = c.pointer.ok_or("a tool check needs pointer")?;
        if !(pointer.is_empty() || pointer.starts_with('/')) {
            return Err("pointer is a JSON pointer such as /entries/0/name".into());
        }
        let value = c.equals.ok_or("a tool check needs equals")?;
        return Ok(Check::Tool {
            tool,
            args: toml_to_json(toml::Value::Table(c.args.unwrap_or_default())),
            pointer,
            value: toml_to_json(value),
        });
    }
    let argv = c.command.unwrap_or_default();
    if argv.is_empty() || argv[0].is_empty() {
        return Err("command names the program first".into());
    }
    if c.contains.is_some() || c.absent.is_some() || c.equals.is_some() || c.args.is_some() {
        return Err("a command check takes exit only".into());
    }
    Ok(Check::Command {
        argv,
        exit: c.exit.unwrap_or(0),
    })
}

/// Reads the recorded run `name` of `task` (behaviour 3).
pub fn load_script(task: &Task, name: &str) -> Result<Script, String> {
    let file = task.dir.join("scripts").join(format!("{name}.toml"));
    let shown = file.display().to_string();
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{shown}: {e}"))?;
    let s: Script = toml::from_str(&text).map_err(|e| format!("{shown}: {}", e.message()))?;
    for (i, c) in s.call.iter().enumerate() {
        if !matches!(c.expect.as_deref(), None | Some("ok" | "error")) {
            return Err(format!("{shown}: call {}: expect is ok or error", i + 1));
        }
    }
    Ok(s)
}

/// Every task in `dir`, by id (behaviour 1).
pub fn load_tasks(dir: &Path) -> Result<Vec<Task>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("task.toml").is_file())
        .collect();
    dirs.sort();
    dirs.iter().map(|d| load_task(d)).collect()
}

/// One check's outcome (behaviour 7).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckResult {
    /// `file`, `tool` or `command`.
    pub kind: &'static str,
    /// Whether it passed.
    pub passed: bool,
    /// A short reason; never file contents.
    pub detail: String,
}

/// One call's outcome (behaviour 7).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepResult {
    /// Tool name.
    pub tool: String,
    /// Whether it answered without an error.
    pub ok: bool,
    /// Milliseconds.
    pub ms: u64,
}

/// The step a run failed at.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FailedStep {
    /// Index of the call, from 0.
    pub index: usize,
    /// Why.
    pub reason: String,
}

/// The report of one run (behaviour 7).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunReport {
    /// Task id.
    pub task: String,
    /// Script name.
    pub script: String,
    /// Agent name.
    pub agent: String,
    /// Model name.
    pub model: String,
    /// A negative control (its name starts with `fail`).
    pub control: bool,
    /// Every check passed, every call went as expected and no limit was reached.
    pub success: bool,
    /// The step the run failed at.
    pub failed_step: Option<FailedStep>,
    /// The limit reached: `tool_calls` or `seconds`.
    pub limit: Option<&'static str>,
    /// The checks in order.
    pub checks: Vec<CheckResult>,
    /// Calls made.
    pub tool_calls: usize,
    /// Calls that answered with an error.
    pub tool_errors: usize,
    /// Duration of the run, checks included.
    pub duration_ms: u64,
    /// Cost, when the script knows it.
    pub cost_cents: Option<u64>,
    /// The calls.
    pub steps: Vec<StepResult>,
    /// Why the run could not be made at all, when it could not.
    pub error: Option<String>,
}

/// Per agent and model (behaviour 8).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentSummary {
    /// Agent name.
    pub agent: String,
    /// Model name.
    pub model: String,
    /// Runs, negative controls not counted.
    pub runs: usize,
    /// Successful runs.
    pub successes: usize,
    /// `successes / runs`.
    pub success_rate: f64,
    /// Total duration.
    pub duration_ms: u64,
    /// Total cost, when every run knows it.
    pub cost_cents: Option<u64>,
}

/// The report of a suite (behaviour 8).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SuiteReport {
    /// Every run.
    pub runs: Vec<RunReport>,
    /// Per agent and model.
    pub summary: Vec<AgentSummary>,
    /// Negative controls.
    pub controls: usize,
    /// Negative controls the checks caught.
    pub controls_caught: usize,
    /// Every run passed and every control was caught.
    pub passed: bool,
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A temporary folder of the run, removed when dropped.
struct RunDir(PathBuf);

impl RunDir {
    fn new() -> std::io::Result<Self> {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "ferris-idea-agent-bench-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("home"))?;
        Ok(Self(p))
    }
}

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A server for `work` with exactly `grants`, trusted through its own trust file, with every user
/// file in `home` (behaviour 4).
fn server(work: &Path, home: &Path, grants: &[Grant]) -> Result<IdeServer, String> {
    let trust = home.join("trust.toml");
    if !trust.exists() {
        ferris_idea_core::trust::add_location(
            &trust,
            work,
            ferris_idea_core::trust::TrustKind::Project,
        )?;
    }
    let permissions = AgentPermissions {
        allow: grants.iter().copied().collect(),
        file: None,
        problem: None,
    };
    let workspace = Workspace::open(work).map_err(|e| e.to_string())?;
    Ok(IdeServer::with_permissions(workspace, permissions)
        .with_trust(ferris_idea_core::trust::TrustSettings::with_file(&trust))
        .with_plugins_dir(home.join("plugins"))
        .with_test_history(home.join("test-history"))
        .with_plugin_run_file(home.join("run-permissions.toml"))
        .with_theme_file(home.join("theme"))
        .with_recent_projects_file(home.join("recent-projects"))
        .with_log_dir(home.join("logs"))
        .with_ide_settings_file(home.join("settings.toml"))
        .with_keymap_file(home.join("keymap.toml"))
        .with_window_layouts_file(home.join("window-layouts.json"))
        .with_connectors_dir(home.join("connectors"))
        .with_python_tool_path(home.join("tools"))
        .with_package_index("http://127.0.0.1:9".into(), home.join("pypi-names.txt")))
}

/// One MCP session over an in-memory pipe (behaviour 4).
struct Session {
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    lines: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    next: u64,
    serving: tokio::task::JoinHandle<()>,
}

impl Session {
    async fn start(server: IdeServer, agent: &str) -> Result<Self, String> {
        let (client_io, server_io) = tokio::io::duplex(1 << 20);
        let serving = tokio::spawn(async move {
            if let Ok(running) = server.serve(tokio::io::split(server_io)).await {
                let _ = running.waiting().await;
            }
        });
        let (read, write) = tokio::io::split(client_io);
        let mut s = Session {
            write,
            lines: BufReader::new(read).lines(),
            next: 0,
            serving,
        };
        s.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": agent, "version": "agent-bench"},
            }),
        )
        .await?;
        s.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await?;
        Ok(s)
    }

    async fn send(&mut self, message: Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        self.write
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        self.write.flush().await.map_err(|e| e.to_string())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let line = self
                .lines
                .next_line()
                .await
                .map_err(|e| e.to_string())?
                .ok_or("the MCP server closed before answering")?;
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message["id"] == json!(id) {
                return Ok(message);
            }
        }
    }

    async fn call(&mut self, tool: &str, args: Value) -> Result<Answer, String> {
        let response = self
            .request("tools/call", json!({"name": tool, "arguments": args}))
            .await?;
        Ok(interpret(&response))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Runs a `command` check in `work` (behaviour 2).
async fn run_command(argv: &[String], work: &Path, home: &Path, exit: i32) -> (bool, String) {
    let mut command = tokio::process::Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(work)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LANG", "C")
        .env("HOME", home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return (false, format!("{} could not start: {e}", argv[0])),
    };
    match tokio::time::timeout(Duration::from_secs(COMMAND_SECONDS), child.wait()).await {
        Err(_) => {
            let _ = child.kill().await;
            (false, format!("{} ran over {COMMAND_SECONDS} s", argv[0]))
        }
        Ok(Err(e)) => (false, format!("{}: {e}", argv[0])),
        Ok(Ok(status)) => {
            let code = status.code();
            let passed = code == Some(exit);
            let shown = code.map_or("no exit code (signal)".to_owned(), |c| format!("exit {c}"));
            (passed, format!("{} {shown}, expected exit {exit}", argv[0]))
        }
    }
}

async fn run_check(c: &Check, work: &Path, home: &Path) -> CheckResult {
    let (passed, detail) = match c {
        Check::Contains { file, text } => match std::fs::read_to_string(work.join(file)) {
            Ok(t) if t.contains(text.as_str()) => (true, format!("{file} holds the text")),
            Ok(_) => (false, format!("{file} does not hold the text")),
            Err(e) => (false, format!("{file}: {e}")),
        },
        Check::Absent { file, text } => match std::fs::read_to_string(work.join(file)) {
            Ok(t) if t.contains(text.as_str()) => (false, format!("{file} still holds the text")),
            Ok(_) => (true, format!("{file} does not hold the text")),
            Err(_) => (true, format!("{file} is missing")),
        },
        Check::Equals { file, expected } => {
            match (std::fs::read(work.join(file)), std::fs::read(expected)) {
                (Ok(a), Ok(b)) if a == b => (true, format!("{file} equals the expected file")),
                (Ok(_), Ok(_)) => (false, format!("{file} differs from the expected file")),
                (Err(e), _) | (_, Err(e)) => (false, format!("{file}: {e}")),
            }
        }
        Check::Tool {
            tool,
            args,
            pointer,
            value,
        } => {
            // No rights: a check never changes the result.
            let answer = match server(work, home, &[]) {
                Ok(s) => crate::call::call_once(s, tool, args.clone()).await,
                Err(e) => Err(e),
            };
            match answer {
                Ok(Answer::Ok(v)) => match v.pointer(pointer) {
                    Some(found) if found == value => (true, format!("{tool}: {pointer} matches")),
                    Some(_) => (false, format!("{tool}: {pointer} has another value")),
                    None => (false, format!("{tool}: the answer has no {pointer}")),
                },
                Ok(Answer::ToolError(m)) => (false, format!("{tool} refused: {}", first_line(&m))),
                Err(e) => (false, format!("{tool}: {e}")),
            }
        }
        Check::Command { argv, exit } => run_command(argv, work, home, *exit).await,
    };
    CheckResult {
        kind: c.kind(),
        passed,
        detail,
    }
}

fn first_line(s: &str) -> &str {
    let line = s.lines().next().unwrap_or_default();
    match line.char_indices().nth(200) {
        Some((i, _)) => &line[..i],
        None => line,
    }
}

/// Runs the recorded run `script` of `task` (behaviours 4 to 7).
pub async fn run(task: &Task, name: &str) -> RunReport {
    let start = Instant::now();
    let mut report = RunReport {
        task: task.id.clone(),
        script: name.to_owned(),
        agent: String::new(),
        model: String::new(),
        control: name.starts_with("fail"),
        success: false,
        failed_step: None,
        limit: None,
        checks: Vec::new(),
        tool_calls: 0,
        tool_errors: 0,
        duration_ms: 0,
        cost_cents: None,
        steps: Vec::new(),
        error: None,
    };
    let script = match load_script(task, name) {
        Ok(s) => s,
        Err(e) => {
            report.error = Some(e);
            return report;
        }
    };
    report.agent.clone_from(&script.agent);
    report.model.clone_from(&script.model);
    report.cost_cents = script.cost_cents;
    let dir = match RunDir::new() {
        Ok(d) => d,
        Err(e) => {
            report.error = Some(format!("no temporary folder: {e}"));
            return report;
        }
    };
    let work = dir.0.join("work");
    let home = dir.0.join("home");
    if let Err(e) = copy_dir(&task.dir.join("project"), &work) {
        report.error = Some(format!("the project could not be copied: {e}"));
        return report;
    }
    let deadline = start + Duration::from_secs(task.max_seconds);
    let session = match server(&work, &home, &task.grants) {
        Ok(s) => Session::start(s, &script.agent).await,
        Err(e) => Err(e),
    };
    match session {
        Err(e) => report.error = Some(format!("the MCP server did not start: {e}")),
        Ok(mut session) => {
            for (index, c) in script.call.iter().enumerate() {
                if report.tool_calls >= task.max_tool_calls as usize {
                    report.limit = Some("tool_calls");
                    break;
                }
                let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                    report.limit = Some("seconds");
                    break;
                };
                let args = toml_to_json(toml::Value::Table(c.args.clone()));
                let began = Instant::now();
                report.tool_calls += 1;
                let answer = match tokio::time::timeout(left, session.call(&c.tool, args)).await {
                    Err(_) => {
                        report.limit = Some("seconds");
                        break;
                    }
                    Ok(a) => a,
                };
                let ok = matches!(answer, Ok(Answer::Ok(_)));
                if !ok {
                    report.tool_errors += 1;
                }
                report.steps.push(StepResult {
                    tool: c.tool.clone(),
                    ok,
                    ms: ms(began.elapsed()),
                });
                let expected_ok = c.expect.as_deref() != Some("error");
                if ok != expected_ok {
                    let reason = match answer {
                        Ok(Answer::ToolError(m)) => {
                            format!("{} refused: {}", c.tool, first_line(&m))
                        }
                        Err(e) => format!("{}: {e}", c.tool),
                        Ok(Answer::Ok(_)) => format!("{} answered, an error was expected", c.tool),
                    };
                    report.failed_step = Some(FailedStep { index, reason });
                    break;
                }
            }
        }
    }
    for c in &task.checks {
        report.checks.push(run_check(c, &work, &home).await);
    }
    report.success = report.error.is_none()
        && report.failed_step.is_none()
        && report.limit.is_none()
        && report.checks.iter().all(|c| c.passed);
    report.duration_ms = ms(start.elapsed());
    report
}

/// Runs every recorded run of every task in `dir` (behaviours 8 and 9).
pub async fn run_suite(dir: &Path) -> Result<SuiteReport, String> {
    let tasks = load_tasks(dir)?;
    let mut runs = Vec::new();
    for task in &tasks {
        for name in &task.scripts {
            runs.push(run(task, name).await);
        }
    }
    Ok(suite(runs))
}

/// The suite report of `runs` (behaviour 8).
pub fn suite(runs: Vec<RunReport>) -> SuiteReport {
    let mut by: BTreeMap<(String, String), AgentSummary> = BTreeMap::new();
    for r in runs.iter().filter(|r| !r.control) {
        let s = by
            .entry((r.agent.clone(), r.model.clone()))
            .or_insert_with(|| AgentSummary {
                agent: r.agent.clone(),
                model: r.model.clone(),
                runs: 0,
                successes: 0,
                success_rate: 0.0,
                duration_ms: 0,
                cost_cents: Some(0),
            });
        s.runs += 1;
        s.successes += usize::from(r.success);
        s.duration_ms += r.duration_ms;
        s.cost_cents = match (s.cost_cents, r.cost_cents) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
    }
    let summary: Vec<AgentSummary> = by
        .into_values()
        .map(|mut s| {
            s.success_rate = if s.runs == 0 {
                0.0
            } else {
                s.successes as f64 / s.runs as f64
            };
            s
        })
        .collect();
    let controls = runs.iter().filter(|r| r.control).count();
    let controls_caught = runs
        .iter()
        .filter(|r| r.control && !r.success && r.error.is_none())
        .count();
    let passed =
        runs.iter().filter(|r| !r.control).all(|r| r.success) && controls_caught == controls;
    SuiteReport {
        runs,
        summary,
        controls,
        controls_caught,
        passed,
    }
}

/// A task as `agent_benchmark` lists it (behaviour 10).
pub fn listed(task: &Task) -> Value {
    json!({
        "id": task.id,
        "title": task.title,
        "prompt": task.prompt,
        "grants": task.grants.iter().map(|g| g.name()).collect::<Vec<_>>(),
        "max_tool_calls": task.max_tool_calls,
        "max_seconds": task.max_seconds,
        "checks": task.checks.len(),
        "scripts": task.scripts,
    })
}

/// Arguments of `agent_benchmark`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentBenchmarkArgs {
    /// The task to run; left out, the tasks are listed.
    pub task: Option<String>,
    /// With `task`: only this recorded run (default: all of the task's).
    pub script: Option<String>,
    /// Only answer what would run (default true); false runs and needs the `run` right and a
    /// trusted project.
    pub dry_run: Option<bool>,
}

#[tool_router(router = agent_bench_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "The agent benchmark of this workspace: tasks in `tests/agent-benchmark/<id>/`, each run through the MCP server in a temporary copy and checked deterministically. Without `task`: the task list. With `task` (optionally `script`): replays recorded runs without a model and returns their reports (never file contents) and a summary per agent and model. Dry run by default; dry_run=false needs the `run` right and a trusted project, since checks run the task's commands."
    )]
    async fn agent_benchmark(
        &self,
        Parameters(args): Parameters<AgentBenchmarkArgs>,
    ) -> Result<CallToolResult, McpError> {
        let dir = self.workspace.root().join(TASKS_DIR);
        if !dir.is_dir() {
            return json_result(
                json!({"tasks_dir": TASKS_DIR, "tasks": [], "problems": [format!("{TASKS_DIR} is missing")]}),
            );
        }
        let tasks = load_tasks(&dir).map_err(|e| McpError::invalid_params(e, None))?;
        let Some(id) = args.task else {
            return json_result(json!({
                "tasks_dir": TASKS_DIR,
                "tasks": tasks.iter().map(listed).collect::<Vec<_>>(),
            }));
        };
        let task = tasks.iter().find(|t| t.id == id).ok_or_else(|| {
            McpError::invalid_params(format!("no task {id:?} in {TASKS_DIR}"), None)
        })?;
        let scripts: Vec<String> = match args.script {
            Some(s) if task.scripts.contains(&s) => vec![s],
            Some(s) => {
                return Err(McpError::invalid_params(
                    format!("task {id:?} has no recorded run {s:?}"),
                    None,
                ));
            }
            None => task.scripts.clone(),
        };
        let dry_run = args.dry_run.unwrap_or(true);
        if dry_run {
            return json_result(json!({
                "dry_run": true,
                "task": listed(task),
                "would_run": scripts,
            }));
        }
        self.require(Grant::Run).await?;
        self.require_trusted()?;
        let mut runs = Vec::new();
        for name in &scripts {
            runs.push(run(task, name).await);
        }
        let mut answer = serde_json::to_value(suite(runs)).unwrap_or_default();
        answer["dry_run"] = json!(false);
        json_result(answer)
    }
}

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("agent_benchmark", crate::groups::Group::Ide),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25).
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] = &[(
    "agent_benchmark",
    &[
        crate::untrusted::Source::Files,
        crate::untrusted::Source::Process,
    ],
)];

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ferris-idea-bench-unit-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("demo/project")).unwrap();
        std::fs::create_dir_all(p.join("demo/scripts")).unwrap();
        p
    }

    /// SPEC-0345 AK-1.
    #[test]
    fn task_files_and_their_bounds() {
        let root = scratch("bounds");
        let dir = root.join("demo");
        std::fs::write(dir.join("expected.txt"), "x").unwrap();
        let write = |text: &str| std::fs::write(dir.join("task.toml"), text).unwrap();
        write(
            r#"id = "demo"
title = "Demo"
prompt = "Fix it"
grants = ["write", "run"]
max_tool_calls = 10
max_seconds = 30
[[check]]
file = "a.txt"
contains = "x"
[[check]]
file = "a.txt"
absent = "y"
[[check]]
file = "a.txt"
equals = "expected.txt"
[[check]]
tool = "read_file"
args = { path = "a.txt" }
pointer = "/text"
equals = "x"
[[check]]
command = ["true"]
"#,
        );
        std::fs::write(
            dir.join("scripts/solve.toml"),
            "agent = \"a\"\nmodel = \"none\"\n",
        )
        .unwrap();
        let t = load_task(&dir).unwrap();
        assert_eq!(t.grants, [Grant::Write, Grant::Run]);
        assert_eq!((t.max_tool_calls, t.max_seconds), (10, 30));
        assert_eq!(t.checks.len(), 5);
        assert_eq!(t.scripts, ["solve"]);
        assert!(matches!(&t.checks[4], Check::Command { exit: 0, .. }));

        let refused = |text: &str, part: &str| {
            write(text);
            let e = load_task(&dir).unwrap_err();
            assert!(e.contains(part), "{e}");
        };
        let base = "title = \"t\"\nprompt = \"p\"\n";
        let check = "[[check]]\nfile = \"a\"\ncontains = \"x\"\n";
        refused(&format!("id = \"other\"\n{base}{check}"), "folder's name");
        refused(
            &format!("id = \"demo\"\n{base}colour = 1\n{check}"),
            "unknown field",
        );
        refused(
            &format!("id = \"demo\"\n{base}grants = [\"settings\"]\n{check}"),
            "grants write",
        );
        refused(
            &format!("id = \"demo\"\n{base}max_tool_calls = 0\n{check}"),
            "max_tool_calls",
        );
        refused(
            &format!("id = \"demo\"\n{base}max_seconds = 4000\n{check}"),
            "max_seconds",
        );
        refused(&format!("id = \"demo\"\n{base}"), "at least one");
        refused(
            &format!("id = \"demo\"\n{base}[[check]]\nfile = \"../x\"\ncontains = \"x\"\n"),
            "inside the project",
        );
        refused(
            &format!("id = \"demo\"\n{base}[[check]]\nfile = \"a\"\ncommand = [\"true\"]\n"),
            "exactly one",
        );
        refused(
            &format!("id = \"demo\"\n{base}[[check]]\ntool = \"read_file\"\nequals = 1\n"),
            "pointer",
        );
        refused(
            &format!("id = \"demo\"\n{base}[[check]]\nfile = \"a\"\nequals = \"missing.txt\"\n"),
            "missing",
        );

        write(&format!("id = \"demo\"\n{base}{check}"));
        let t = load_task(&dir).unwrap();
        std::fs::write(
            dir.join("scripts/solve.toml"),
            "agent = \"a\"\nmodel = \"none\"\ncost_cents = 3\n[[call]]\ntool = \"read_file\"\nargs = { path = \"a\" }\n[[call]]\ntool = \"x\"\nexpect = \"error\"\n",
        )
        .unwrap();
        let s = load_script(&t, "solve").unwrap();
        assert_eq!(s.cost_cents, Some(3));
        assert_eq!(s.call.len(), 2);
        assert_eq!(s.call[1].expect.as_deref(), Some("error"));
        std::fs::write(
            dir.join("scripts/solve.toml"),
            "agent = \"a\"\nmodel = \"none\"\n[[call]]\ntool = \"x\"\nexpect = \"maybe\"\n",
        )
        .unwrap();
        assert!(load_script(&t, "solve").unwrap_err().contains("expect"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
