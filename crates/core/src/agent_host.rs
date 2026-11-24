//! The agent host over the Agent Client Protocol (`SPEC-0348`, ROADMAP M19.5, ADR-0013 rule 3,
//! ADR-0023 rules 2 and 9): Ferris IDEA starts an agent the user configured as a child process,
//! speaks ACP version 1 with it over stdin and stdout (JSON-RPC 2.0, one message per line), and
//! derives the agent's state from the protocol: `working`, `waiting_for_approval`,
//! `waiting_for_input`, `idle`, `done`, `failed` or `unknown`.
//!
//! Security (`SPEC-0348` behaviours 11–17): the program and its arguments come only from the
//! user's settings folder, never from the workspace or a tool call; the child gets a cleaned
//! environment (an allowlist of names, no credentials); it starts in the workspace and its session
//! is opened there; the host offers no file system, terminal or MCP capability to it; and a
//! permission request is never answered by the host on its own, and never with a standing
//! approval.
//!
//! Open integration points (`SPEC-0348` open points): the sandbox of M19.9 wraps a
//! [`SpawnPlan`] before it is started; the shared agent configuration of M19.8 is not passed on,
//! and no hook of it is ever run here.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::notifications::{Kind, NotificationRegistry};
use crate::permissions::{GRANT_FILE, inside_workspace};

/// The ACP version the host speaks (behaviour 3).
pub const PROTOCOL_VERSION: u64 = 1;
/// The configuration file in the user's settings folder, beside the grant file (behaviour 11).
pub const HOST_FILE: &str = "agent-host.toml";
/// Environment variables every agent gets from Ferris IDEA's own environment, when set
/// (behaviour 13). Nothing else is passed unless the configuration names it.
pub const DEFAULT_ENV: [&str; 12] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TERM",
    "TMPDIR",
    "TZ",
];
/// Bounds of the configuration (behaviour 12).
pub const MAX_AGENTS_BOUNDS: [u32; 2] = [1, 8];
/// Agents running at once when the configuration does not say.
pub const MAX_AGENTS_DEFAULT: u32 = 2;
/// Most agent definitions in the configuration.
pub const MAX_DEFINITIONS: usize = 32;
/// Most arguments of one agent, and the longest argument in characters.
pub const MAX_ARGS: usize = 64;
const ARG_CHARS: usize = 4096;
/// Most extra environment names of one agent.
pub const MAX_ENV: usize = 32;
/// The longest prompt in characters (behaviour 6).
pub const PROMPT_MAX_CHARS: usize = 20_000;
/// How many output entries an agent keeps (behaviour 9).
pub const OUTPUT_ENTRIES: usize = 500;
/// The longest output entry in characters.
pub const ENTRY_CHARS: usize = 8_000;
/// The longest line the host reads from the agent; a longer one is dropped (behaviour 10).
pub const LINE_BYTES: usize = 4 * 1024 * 1024;
/// Finished agents kept in the list.
pub const FINISHED_KEPT: usize = 8;
/// Notices kept per agent.
pub const NOTICES_KEPT: usize = 20;
/// How long `stop` waits for the agent to end by itself before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(3);

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

// ---------------------------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------------------------

/// One agent the user configured (behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDef {
    /// 1 to 64 characters of a-z, 0-9, -, _.
    pub id: String,
    /// A name for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The program, an absolute path.
    pub command: PathBuf,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment names passed on besides [`DEFAULT_ENV`]; never names that look like
    /// credentials.
    #[serde(default)]
    pub env: Vec<String>,
}

/// The host's configuration: `max_agents` and `[[agent]]` tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    /// Agents running at once (1 to 8).
    #[serde(default = "default_max_agents")]
    pub max_agents: u32,
    /// The agents that can be started.
    #[serde(default)]
    pub agent: Vec<AgentDef>,
    /// Limits for agents that conduct agents (`SPEC-0349` behaviour 8).
    #[serde(default)]
    pub control: crate::agent_control::ControlLimits,
}

fn default_max_agents() -> u32 {
    MAX_AGENTS_DEFAULT
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            max_agents: MAX_AGENTS_DEFAULT,
            agent: Vec::new(),
            control: crate::agent_control::ControlLimits::default(),
        }
    }
}

/// Whether an environment name looks like it holds a credential (behaviour 13).
pub fn secret_like(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    const WORDS: [&str; 12] = [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "PASS",
        "KEY",
        "APIKEY",
        "CREDENTIAL",
        "CREDENTIALS",
        "AUTH",
        "COOKIE",
        "PRIVATE",
    ];
    const INSIDE: [&str; 5] = ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "APIKEY"];
    upper.split('_').any(|w| WORDS.contains(&w)) || INSIDE.iter().any(|w| upper.contains(w))
}

fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name.len() <= 128
}

impl HostConfig {
    /// Checks the bounds of behaviour 12.
    pub fn check(&self) -> Result<(), String> {
        let [lo, hi] = MAX_AGENTS_BOUNDS;
        if !(lo..=hi).contains(&self.max_agents) {
            return Err(format!("max_agents must be {lo} to {hi}"));
        }
        self.control.check()?;
        if self.agent.len() > MAX_DEFINITIONS {
            return Err(format!("at most {MAX_DEFINITIONS} agents"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for a in &self.agent {
            if !valid_id(&a.id) {
                return Err(format!(
                    "agent id `{}`: 1 to 64 characters of a-z, 0-9, -, _",
                    a.id
                ));
            }
            if !seen.insert(a.id.as_str()) {
                return Err(format!("agent id `{}` appears twice", a.id));
            }
            if !a.command.is_absolute() {
                return Err(format!(
                    "agent `{}`: command must be an absolute path",
                    a.id
                ));
            }
            if a.args.len() > MAX_ARGS {
                return Err(format!("agent `{}`: at most {MAX_ARGS} arguments", a.id));
            }
            if a.args
                .iter()
                .any(|x| x.chars().count() > ARG_CHARS || x.contains('\0'))
            {
                return Err(format!(
                    "agent `{}`: an argument is longer than {ARG_CHARS} characters or holds NUL",
                    a.id
                ));
            }
            if a.env.len() > MAX_ENV {
                return Err(format!(
                    "agent `{}`: at most {MAX_ENV} environment names",
                    a.id
                ));
            }
            for n in &a.env {
                if !valid_env_name(n) {
                    return Err(format!(
                        "agent `{}`: `{n}` is no environment name (A-Z, 0-9, _)",
                        a.id
                    ));
                }
                if secret_like(n) {
                    return Err(format!(
                        "agent `{}`: `{n}` looks like a credential and is never passed to an agent; the agent's own program keeps its credentials (ADR-0017)",
                        a.id
                    ));
                }
            }
        }
        Ok(())
    }

    /// The definition with this id.
    pub fn get(&self, id: &str) -> Option<&AgentDef> {
        self.agent.iter().find(|a| a.id == id)
    }
}

/// The configuration file beside the grant file (the same prefix, as tests name it).
pub fn host_file(grant_file: &Path) -> PathBuf {
    let name = grant_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = name.strip_suffix(GRANT_FILE).unwrap_or("");
    grant_file.with_file_name(format!("{prefix}{HOST_FILE}"))
}

/// The configuration as read, with where it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LoadedHostConfig {
    /// The configuration; empty when there is none or it has a problem.
    pub config: HostConfig,
    /// The file consulted.
    pub file: PathBuf,
    /// The problem with the file; nothing can be started then.
    pub problem: Option<String>,
}

/// Reads the host's configuration (behaviour 11): a file inside the workspace is ignored and
/// reported, a missing file is the empty configuration, a malformed one or one out of bounds is
/// a problem and starts nothing.
pub fn load_host_config(file: &Path, root: &Path) -> LoadedHostConfig {
    let mut out = LoadedHostConfig {
        file: file.to_path_buf(),
        ..LoadedHostConfig::default()
    };
    if inside_workspace(file, root) {
        out.problem = Some("the agent host's configuration lies inside the workspace and is ignored, so a cloned project cannot start programs".into());
        return out;
    }
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            out.problem = Some(e.to_string());
            return out;
        }
    };
    match toml::from_str::<HostConfig>(&text) {
        Ok(c) => match c.check() {
            Ok(()) => out.config = c,
            Err(e) => out.problem = Some(e),
        },
        Err(e) => out.problem = Some(e.message().to_owned()),
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Spawn plan
// ---------------------------------------------------------------------------------------------

/// How an agent is started (behaviours 13 and 14). The sandbox of M19.9 wraps a plan before it
/// is started (open point 1); nothing else may change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPlan {
    /// The program, an absolute path.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The workspace, resolved; the child starts here and its session is opened here.
    pub cwd: PathBuf,
    /// The whole environment of the child.
    pub env: Vec<(String, OsString)>,
}

impl SpawnPlan {
    /// The names of the environment passed, never the values.
    pub fn env_names(&self) -> Vec<&str> {
        self.env.iter().map(|(n, _)| n.as_str()).collect()
    }
}

/// The plan for `def` in the workspace `root`, with values taken from `lookup` (Ferris IDEA's own
/// environment in the real host).
pub fn plan(
    def: &AgentDef,
    root: &Path,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Result<SpawnPlan, String> {
    if !def.command.is_absolute() {
        return Err(format!("`{}` is no absolute path", def.command.display()));
    }
    if !def.command.is_file() {
        return Err(format!(
            "the program `{}` of agent `{}` does not exist",
            def.command.display(),
            def.id
        ));
    }
    let cwd = canon(root);
    if !cwd.is_dir() {
        return Err(format!("the workspace `{}` is no folder", cwd.display()));
    }
    let mut env = Vec::new();
    for name in DEFAULT_ENV
        .iter()
        .map(|n| n.to_string())
        .chain(def.env.iter().cloned())
    {
        if secret_like(&name) || env.iter().any(|(n, _)| *n == name) {
            continue;
        }
        if let Some(v) = lookup(&name) {
            env.push((name, v));
        }
    }
    Ok(SpawnPlan {
        program: def.command.clone(),
        args: def.args.clone(),
        cwd,
        env,
    })
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

/// The visible state of a hosted agent (ADR-0023 rule 9, `SPEC-0348` behaviour 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Starting, or a prompt is running.
    Working,
    /// A permission request waits for an answer.
    WaitingForApproval,
    /// The session waits for the person: the first prompt, or the last turn stopped short.
    WaitingForInput,
    /// The last turn ended normally; the agent waits for the next prompt.
    Idle,
    /// The agent ended normally.
    Done,
    /// The agent ended otherwise, or the protocol failed.
    Failed,
    /// The host cannot tell; never counted as idle.
    Unknown,
}

impl AgentState {
    /// The name over MCP.
    pub fn name(self) -> &'static str {
        match self {
            AgentState::Working => "working",
            AgentState::WaitingForApproval => "waiting_for_approval",
            AgentState::WaitingForInput => "waiting_for_input",
            AgentState::Idle => "idle",
            AgentState::Done => "done",
            AgentState::Failed => "failed",
            AgentState::Unknown => "unknown",
        }
    }

    /// Every state, in the order of the ADR.
    pub const ALL: [AgentState; 7] = [
        AgentState::Working,
        AgentState::WaitingForApproval,
        AgentState::WaitingForInput,
        AgentState::Idle,
        AgentState::Done,
        AgentState::Failed,
        AgentState::Unknown,
    ];

    /// The state of this name.
    pub fn from_name(name: &str) -> Option<AgentState> {
        AgentState::ALL.into_iter().find(|s| s.name() == name)
    }

    /// Whether the state needs the person and raises a notice (behaviour 8).
    pub fn notifies(self) -> bool {
        matches!(
            self,
            AgentState::WaitingForApproval
                | AgentState::WaitingForInput
                | AgentState::Failed
                | AgentState::Unknown
        )
    }

    /// Whether the agent has ended.
    pub fn ended(self) -> bool {
        matches!(self, AgentState::Done | AgentState::Failed)
    }
}

/// One option of a permission request, as the agent sent it (untrusted text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionOption {
    /// The agent's id of the option.
    pub option_id: String,
    /// Its label.
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, `reject_always` or what the agent sent.
    pub kind: String,
}

/// A permission request waiting for an answer (behaviour 15).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingPermission {
    /// The host's number of the request, to answer it.
    pub request: u64,
    /// The tool call's title as the agent sent it (untrusted text).
    pub title: String,
    /// The tool call's kind as the agent sent it.
    pub kind: String,
    /// The options.
    pub options: Vec<PermissionOption>,
    /// When it was asked (Unix seconds).
    pub asked_at: u64,
    /// The edits the tool call proposes, as the agent sent them (behaviour 24; untrusted text).
    pub diffs: Vec<ProposedDiff>,
    /// The number of a request of the same kind the person refused earlier in this turn whose
    /// command or files this one repeats (behaviour 25, IMP-0547).
    pub similar_refused: Option<u64>,
    #[serde(skip)]
    rpc_id: Value,
}

/// One edit a permission request proposes (behaviour 24): the `diff` content of its tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposedDiff {
    /// The file as the agent named it.
    pub path: String,
    /// The text before; `None` for a new file.
    pub old_text: Option<String>,
    /// The text after.
    pub new_text: String,
    /// A side was longer than [`DIFF_SIDE_CHARS`] and was cut.
    pub cut: bool,
}

/// Most proposed edits kept per request (behaviour 24).
pub const DIFFS_PER_REQUEST: usize = 8;
/// Longest side of a proposed edit kept (characters).
pub const DIFF_SIDE_CHARS: usize = 64 * 1024;

fn proposed_diffs(tool_call: &Value) -> Vec<ProposedDiff> {
    let cut = |t: &str| -> (String, bool) {
        if t.chars().count() > DIFF_SIDE_CHARS {
            (t.chars().take(DIFF_SIDE_CHARS).collect(), true)
        } else {
            (t.to_owned(), false)
        }
    };
    tool_call["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| c["type"] == "diff")
                .filter_map(|c| {
                    let path = c["path"].as_str()?.to_owned();
                    let (new_text, c1) = cut(c["newText"].as_str().unwrap_or(""));
                    let (old_text, c2) = match c["oldText"].as_str() {
                        Some(t) => {
                            let (t, c) = cut(t);
                            (Some(t), c)
                        }
                        None => (None, false),
                    };
                    Some(ProposedDiff {
                        path,
                        old_text,
                        new_text,
                        cut: c1 || c2,
                    })
                })
                .take(DIFFS_PER_REQUEST)
                .collect()
        })
        .unwrap_or_default()
}

/// What a refused request is compared by (behaviour 25): its kind, its last command (the part
/// after the last `&&`, `;` or `|`) and the files of its edits.
#[derive(Debug, Clone)]
struct Refused {
    request: u64,
    kind: String,
    command: String,
    paths: Vec<String>,
}

fn last_command(title: &str) -> String {
    title
        .rsplit(['&', ';', '|'])
        .next()
        .unwrap_or(title)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn similar_to(refused: &[Refused], kind: &str, title: &str, diffs: &[ProposedDiff]) -> Option<u64> {
    let command = last_command(title);
    let paths: Vec<String> = diffs.iter().map(|d| d.path.clone()).collect();
    refused
        .iter()
        .rev()
        .find(|r| {
            r.kind == kind
                && ((!command.is_empty() && r.command == command)
                    || (!paths.is_empty() && r.paths == paths))
        })
        .map(|r| r.request)
}

/// One entry of an agent's output (behaviour 9). The text comes from the agent and is untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputEntry {
    /// Grows by one per entry.
    pub seq: u64,
    /// Unix seconds.
    pub time: u64,
    /// `prompt`, `message`, `thought`, `tool_call`, `plan`, `permission`, `answer`, `refused`,
    /// `stderr`, `state`, `host` or `changes`.
    pub kind: String,
    /// The text.
    pub text: String,
}

/// A notice for the person (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    /// Unix seconds.
    pub time: u64,
    /// The hosted agent.
    pub agent: String,
    /// The state that raised it.
    pub state: AgentState,
    /// The text.
    pub text: String,
}

/// What an agent looks like now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentSnapshot {
    /// The hosted agent's id (`<definition>-<n>`).
    pub id: String,
    /// The definition it was started from.
    pub agent: String,
    /// The state.
    pub state: AgentState,
    /// Why, in words.
    pub detail: String,
    /// When the state was entered (Unix seconds).
    pub state_since: u64,
    /// When it was started (Unix seconds).
    pub started_at: u64,
    /// The process id.
    pub pid: u32,
    /// Whether the session is open.
    pub session_ready: bool,
    /// The protocol version the agent answered.
    pub protocol_version: Option<u64>,
    /// Prompts answered.
    pub turns: u64,
    /// Grows with every sign of progress (updates, answers).
    pub progress: u64,
    /// The stop reason of the last turn.
    pub last_stop_reason: Option<String>,
    /// Permission requests waiting for an answer.
    pub pending_approvals: Vec<PendingPermission>,
    /// The exit code once the process ended (`None` when killed by a signal or still running).
    pub exit_code: Option<i32>,
    /// Whether the process ended.
    pub exited: bool,
    /// The newest output entry's number (0: none yet).
    pub output_seq: u64,
    /// Notices of this agent, newest last.
    pub notices: Vec<Notice>,
}

#[derive(Debug, Default)]
struct Facts {
    next_id: u64,
    init_id: Option<u64>,
    new_id: Option<u64>,
    prompt_id: Option<u64>,
    session: Option<String>,
    protocol: Option<u64>,
    needs_input: Option<String>,
    permissions: VecDeque<PendingPermission>,
    next_request: u64,
    confused: Option<String>,
    fatal: Option<String>,
    stopping: bool,
    exited: bool,
    exit_code: Option<i32>,
    stdout_closed: bool,
    output: VecDeque<OutputEntry>,
    next_seq: u64,
    progress: u64,
    turns: u64,
    last_stop_reason: Option<String>,
    state: Option<AgentState>,
    detail: String,
    since: u64,
    notices: VecDeque<Notice>,
    /// Requests the person refused in the running turn (behaviour 25).
    refused: Vec<Refused>,
    /// The turn's changes are being recorded (behaviour 27).
    recording: bool,
}

impl Facts {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn push(&mut self, kind: &str, text: impl Into<String>) {
        let mut text: String = text.into();
        if text.chars().count() > ENTRY_CHARS {
            text = text.chars().take(ENTRY_CHARS).collect::<String>() + " …";
        }
        self.next_seq += 1;
        self.output.push_back(OutputEntry {
            seq: self.next_seq,
            time: unix_now(),
            kind: kind.into(),
            text,
        });
        while self.output.len() > OUTPUT_ENTRIES {
            self.output.pop_front();
        }
    }

    /// Streamed chunks of one kind join the newest entry while it is of that kind.
    fn append(&mut self, kind: &str, chunk: &str) {
        match self.output.back_mut() {
            Some(last) if last.kind == kind && last.text.chars().count() < ENTRY_CHARS => {
                last.text.push_str(chunk);
                last.time = unix_now();
            }
            _ => self.push(kind, chunk),
        }
    }

    /// The state the facts say (behaviour 7), in this order.
    fn derive(&self) -> (AgentState, String) {
        if self.exited {
            if let Some(f) = &self.fatal {
                return (AgentState::Failed, f.clone());
            }
            let how = match self.exit_code {
                Some(c) => format!("exit code {c}"),
                None => "ended by a signal".into(),
            };
            if self.stopping {
                return (AgentState::Done, format!("stopped ({how})"));
            }
            return if self.exit_code == Some(0) {
                (AgentState::Done, "the agent ended (exit code 0)".into())
            } else {
                (AgentState::Failed, format!("the agent ended: {how}"))
            };
        }
        if let Some(f) = &self.fatal {
            return (AgentState::Failed, f.clone());
        }
        if self.stopping {
            return (AgentState::Working, "stopping".into());
        }
        if let Some(c) = &self.confused {
            return (AgentState::Unknown, c.clone());
        }
        if self.stdout_closed {
            return (
                AgentState::Unknown,
                "the agent closed its output but its process still runs".into(),
            );
        }
        if let Some(p) = self.permissions.front() {
            return (
                AgentState::WaitingForApproval,
                format!(
                    "{} permission request(s) wait; the first: request {}",
                    self.permissions.len(),
                    p.request
                ),
            );
        }
        if self.session.is_none() {
            return (
                AgentState::Working,
                "starting: initialize and open the session".into(),
            );
        }
        if self.prompt_id.is_some() {
            return (AgentState::Working, "a prompt is running".into());
        }
        if self.recording {
            return (
                AgentState::Working,
                "recording what the turn changed in the workspace".into(),
            );
        }
        if let Some(n) = &self.needs_input {
            return (AgentState::WaitingForInput, n.clone());
        }
        if self.turns == 0 {
            return (
                AgentState::WaitingForInput,
                "the session is open and waits for the first prompt".into(),
            );
        }
        (
            AgentState::Idle,
            "the last turn ended; waits for the next prompt".into(),
        )
    }
}

struct Hosted {
    id: String,
    agent: String,
    root: PathBuf,
    started_at: u64,
    pid: u32,
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    facts: Mutex<Facts>,
    changed: Condvar,
    /// The turns' changes (behaviours 26–28). Locked after `facts` when both are needed.
    turns: Mutex<TurnLog>,
}

/// The workspace recorded before the running turn and the changes of the ended turns.
#[derive(Debug, Default)]
struct TurnLog {
    pending: Option<(u64, String, crate::agent_turns::Snapshot)>,
    done: VecDeque<crate::agent_turns::TurnChanges>,
}

/// Compares the workspace after a turn with the record taken before it (behaviour 27), keeps the
/// changes and lets the agent leave `working`.
fn record_turn(h: &Hosted) {
    let pending = lock(&h.turns).pending.take();
    let Some((turn, prompt, before)) = pending else {
        return;
    };
    let changes = crate::agent_turns::compare(&h.root, &before, turn, &prompt);
    let summary = if changes.files.is_empty() {
        format!("turn {turn} changed no file in the workspace")
    } else {
        format!(
            "turn {turn} changed {} file(s) in the workspace: {}",
            changes.files.len(),
            changes
                .files
                .iter()
                .map(|c| format!("{} ({:?}, +{} −{})", c.path, c.kind, c.added, c.removed))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    {
        let mut t = lock(&h.turns);
        t.done.push_back(changes);
        while t.done.len() > crate::agent_turns::TURNS_KEPT {
            t.done.pop_front();
        }
    }
    let mut f = lock(&h.facts);
    f.recording = false;
    f.push("changes", summary);
    h.settle(&mut f);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Hosted {
    /// Recomputes the state after a change of the facts; a new state that needs the person
    /// raises a notice (behaviour 8). Wakes every waiter.
    fn settle(&self, f: &mut Facts) {
        let (state, detail) = f.derive();
        if f.state != Some(state) {
            f.state = Some(state);
            f.since = unix_now();
            f.push("state", format!("{}: {detail}", state.name()));
            if state.notifies() {
                let text = format!(
                    "Agent `{}` is {}: {detail}",
                    self.id,
                    state.name().replace('_', " ")
                );
                f.notices.push_back(Notice {
                    time: unix_now(),
                    agent: self.id.clone(),
                    state,
                    text: text.clone(),
                });
                while f.notices.len() > NOTICES_KEPT {
                    f.notices.pop_front();
                }
                let kind = if state == AgentState::Failed {
                    Kind::Error
                } else {
                    Kind::Info
                };
                NotificationRegistry::global().record(&self.root, &text, kind);
            }
        }
        f.detail = detail;
        self.changed.notify_all();
    }

    fn snapshot(&self, f: &Facts) -> AgentSnapshot {
        AgentSnapshot {
            id: self.id.clone(),
            agent: self.agent.clone(),
            state: f.state.unwrap_or(AgentState::Unknown),
            detail: f.detail.clone(),
            state_since: f.since,
            started_at: self.started_at,
            pid: self.pid,
            session_ready: f.session.is_some(),
            protocol_version: f.protocol,
            turns: f.turns,
            progress: f.progress,
            last_stop_reason: f.last_stop_reason.clone(),
            pending_approvals: f.permissions.iter().cloned().collect(),
            exit_code: f.exit_code,
            exited: f.exited,
            output_seq: f.next_seq,
            notices: f.notices.iter().cloned().collect(),
        }
    }

    /// Writes messages to the agent, one per line. A write that fails marks the protocol failed.
    fn send(&self, messages: Vec<Value>) {
        if messages.is_empty() {
            return;
        }
        let mut failed = None;
        {
            let mut stdin = lock(&self.stdin);
            match stdin.as_mut() {
                Some(w) => {
                    for m in &messages {
                        let mut line = m.to_string();
                        line.push('\n');
                        if let Err(e) = w.write_all(line.as_bytes()).and_then(|()| w.flush()) {
                            failed = Some(e.to_string());
                            break;
                        }
                    }
                }
                None => failed = Some("the agent's input is closed".into()),
            }
        }
        if let Some(e) = failed {
            let mut f = lock(&self.facts);
            if !f.stopping && !f.exited {
                f.fatal = Some(format!("writing to the agent failed: {e}"));
                self.settle(&mut f);
            }
        }
    }

    fn kill(&self) {
        let _ = lock(&self.child).kill();
    }

    /// Handles one line from the agent's stdout and answers what must be sent back.
    fn handle_line(&self, f: &mut Facts, line: &[u8]) -> Vec<Value> {
        let mut out = Vec::new();
        let msg: Value = match serde_json::from_slice(line) {
            Ok(v @ Value::Object(_)) if v["jsonrpc"] == "2.0" => v,
            _ => {
                if line.iter().all(u8::is_ascii_whitespace) {
                    return out;
                }
                f.confused = Some("the agent wrote a line that is no JSON-RPC 2.0 message".into());
                f.push("host", "a line that is no JSON-RPC 2.0 message was ignored");
                return out;
            }
        };
        f.confused = None;
        let method = msg["method"].as_str();
        let id = msg.get("id").filter(|v| !v.is_null()).cloned();
        match (method, id) {
            (Some(m), Some(rpc_id)) => self.agent_request(f, m, rpc_id, &msg["params"], &mut out),
            (Some(m), None) => {
                if m == "session/update" {
                    self.session_update(f, &msg["params"]["update"]);
                }
            }
            (None, Some(rpc_id)) => self.response(f, &rpc_id, &msg, &mut out),
            (None, None) => {
                f.confused = Some("the agent sent a message without method and id".into());
            }
        }
        out
    }

    fn agent_request(
        &self,
        f: &mut Facts,
        method: &str,
        rpc_id: Value,
        params: &Value,
        out: &mut Vec<Value>,
    ) {
        f.progress += 1;
        if method == "session/request_permission" {
            let options: Vec<PermissionOption> = params["options"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| {
                            Some(PermissionOption {
                                option_id: o["optionId"].as_str()?.to_owned(),
                                name: o["name"].as_str().unwrap_or("").to_owned(),
                                kind: o["kind"].as_str().unwrap_or("").to_owned(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            if options.is_empty() || f.stopping {
                out.push(json!({"jsonrpc": "2.0", "id": rpc_id, "result": {"outcome": {"outcome": "cancelled"}}}));
                return;
            }
            f.next_request += 1;
            let title = params["toolCall"]["title"]
                .as_str()
                .unwrap_or("(no title)")
                .to_owned();
            let kind = params["toolCall"]["kind"].as_str().unwrap_or("").to_owned();
            let diffs = proposed_diffs(&params["toolCall"]);
            let similar_refused = similar_to(&f.refused, &kind, &title, &diffs);
            f.push(
                "permission",
                format!(
                    "request {}: {title} — options: {}{}{}",
                    f.next_request,
                    options
                        .iter()
                        .map(|o| format!("{} ({})", o.option_id, o.kind))
                        .collect::<Vec<_>>()
                        .join(", "),
                    if diffs.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " — edits: {}",
                            diffs
                                .iter()
                                .map(|d| d.path.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    },
                    similar_refused
                        .map(|r| format!(
                            " — you refused a similar request in this turn (request {r})"
                        ))
                        .unwrap_or_default()
                ),
            );
            f.permissions.push_back(PendingPermission {
                request: f.next_request,
                title,
                kind,
                options,
                asked_at: unix_now(),
                diffs,
                similar_refused,
                rpc_id,
            });
            return;
        }
        // No other capability is offered (behaviour 14): file system, terminal, anything else.
        f.push(
            "refused",
            format!("the agent asked for `{method}`, which Ferris IDEA does not offer"),
        );
        out.push(json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "error": {"code": -32601, "message": format!("method not found: Ferris IDEA offers no `{method}` to hosted agents (SPEC-0348)")},
        }));
    }

    fn session_update(&self, f: &mut Facts, update: &Value) {
        f.progress += 1;
        let text_of = |c: &Value| match c["type"].as_str() {
            Some("text") => c["text"].as_str().unwrap_or("").to_owned(),
            Some(t) => format!("[{t} content]"),
            None => String::new(),
        };
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => f.append("message", &text_of(&update["content"])),
            Some("agent_thought_chunk") => f.append("thought", &text_of(&update["content"])),
            Some("tool_call") => f.push(
                "tool_call",
                format!(
                    "{} [{}]",
                    update["title"].as_str().unwrap_or("(no title)"),
                    update["status"].as_str().unwrap_or("pending")
                ),
            ),
            Some("tool_call_update") => {
                let what = update["title"]
                    .as_str()
                    .or(update["toolCallId"].as_str())
                    .unwrap_or("(tool call)");
                if let Some(s) = update["status"].as_str() {
                    f.push("tool_call", format!("{what} [{s}]"));
                }
            }
            Some("plan") => {
                let items: Vec<String> = update["entries"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|e| {
                                format!(
                                    "- [{}] {}",
                                    e["status"].as_str().unwrap_or("pending"),
                                    e["content"].as_str().unwrap_or("")
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                f.push("plan", items.join("\n"));
            }
            _ => {}
        }
    }

    fn response(&self, f: &mut Facts, rpc_id: &Value, msg: &Value, out: &mut Vec<Value>) {
        f.progress += 1;
        let id = rpc_id.as_u64();
        let error = msg.get("error").map(|e| {
            e["message"]
                .as_str()
                .unwrap_or("(no message)")
                .chars()
                .take(500)
                .collect::<String>()
        });
        let result = &msg["result"];
        if id.is_some() && id == f.init_id {
            f.init_id = None;
            if let Some(e) = error {
                f.fatal = Some(format!("initialize failed: {e}"));
                return;
            }
            let version = result["protocolVersion"].as_u64();
            f.protocol = version;
            if version != Some(PROTOCOL_VERSION) {
                f.fatal = Some(format!(
                    "the agent answered protocol version {}; Ferris IDEA speaks ACP version {PROTOCOL_VERSION}",
                    result["protocolVersion"]
                ));
                return;
            }
            let new_id = f.id();
            f.new_id = Some(new_id);
            out.push(json!({
                "jsonrpc": "2.0",
                "id": new_id,
                "method": "session/new",
                "params": {"cwd": self.root, "mcpServers": []},
            }));
        } else if id.is_some() && id == f.new_id {
            f.new_id = None;
            if let Some(e) = error {
                f.fatal = Some(format!(
                    "opening the session failed: {e} (sign-in happens in the agent's own program; Ferris IDEA never passes credentials)"
                ));
                return;
            }
            match result["sessionId"].as_str() {
                Some(s) if !s.is_empty() => {
                    f.session = Some(s.to_owned());
                    f.push("host", "session open");
                }
                _ => f.fatal = Some("the agent opened a session without an id".into()),
            }
        } else if id.is_some() && id == f.prompt_id {
            f.prompt_id = None;
            f.turns += 1;
            if let Some(e) = error {
                f.last_stop_reason = None;
                f.needs_input = Some(format!("the agent answered the prompt with an error: {e}"));
                return;
            }
            let reason = result["stopReason"].as_str().unwrap_or("").to_owned();
            f.needs_input = match reason.as_str() {
                "end_turn" | "cancelled" => None,
                "max_tokens" => Some("the turn stopped at the token limit".into()),
                "max_turn_requests" => {
                    Some("the turn stopped at the limit of model requests".into())
                }
                "refusal" => Some("the agent refused to go on".into()),
                other => {
                    f.confused = Some(format!(
                        "the agent ended the turn with the unknown stop reason `{other}`"
                    ));
                    None
                }
            };
            f.last_stop_reason = Some(reason);
        } else {
            f.confused = Some(format!(
                "the agent answered a request the host never sent ({rpc_id})"
            ));
        }
    }
}

/// Reads one line of at most `max` bytes; `None` at the end. A longer line is consumed and
/// answered as `Some(None)`.
fn read_line(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Option<Vec<u8>>>> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if buf.is_empty() && !too_long {
                None
            } else if too_long {
                Some(None)
            } else {
                Some(Some(buf))
            });
        }
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            if buf.len() + take > max {
                too_long = true;
                buf = Vec::new();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        r.consume(take);
        if done {
            if too_long {
                return Ok(Some(None));
            }
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            return Ok(Some(Some(buf)));
        }
    }
}

fn read_stdout(h: Arc<Hosted>, stdout: impl Read) {
    let mut r = BufReader::new(stdout);
    while let Ok(Some(line)) = read_line(&mut r, LINE_BYTES) {
        let send = {
            let mut f = lock(&h.facts);
            let send = match line {
                Some(l) => h.handle_line(&mut f, &l),
                None => {
                    f.confused = Some(format!(
                        "the agent wrote a line longer than {LINE_BYTES} bytes; it was dropped"
                    ));
                    Vec::new()
                }
            };
            let fatal = f.fatal.is_some();
            let ended = f.prompt_id.is_none() && lock(&h.turns).pending.is_some();
            if ended {
                f.recording = true;
            }
            h.settle(&mut f);
            (send, fatal, ended)
        };
        let (send, fatal, ended) = send;
        h.send(send);
        if ended {
            record_turn(&h);
        }
        if fatal {
            // The protocol cannot go on: end the process (behaviour 10).
            h.kill();
        }
    }
    let mut f = lock(&h.facts);
    f.stdout_closed = true;
    h.settle(&mut f);
}

fn read_stderr(h: Arc<Hosted>, stderr: impl Read) {
    let mut r = BufReader::new(stderr);
    while let Ok(Some(line)) = read_line(&mut r, 64 * 1024) {
        let Some(line) = line else { continue };
        let text: String = String::from_utf8_lossy(&line).chars().take(2000).collect();
        if text.trim().is_empty() {
            continue;
        }
        let mut f = lock(&h.facts);
        f.push("stderr", text);
        h.changed.notify_all();
    }
}

fn watch_exit(h: Arc<Hosted>) {
    loop {
        let status = lock(&h.child).try_wait();
        match status {
            Ok(Some(status)) => {
                // Let the reader take what the agent wrote before it ended.
                let deadline = Instant::now() + Duration::from_secs(1);
                let mut f = lock(&h.facts);
                while !f.stdout_closed && Instant::now() < deadline {
                    f = h
                        .changed
                        .wait_timeout(f, Duration::from_millis(50))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                f.exited = true;
                f.exit_code = status.code();
                f.prompt_id = None;
                f.permissions.clear();
                h.settle(&mut f);
                drop(f);
                lock(&h.stdin).take();
                // A turn the process ended in the middle of still has its changes.
                record_turn(&h);
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                let mut f = lock(&h.facts);
                f.exited = true;
                f.fatal
                    .get_or_insert_with(|| "the agent's process cannot be watched".into());
                h.settle(&mut f);
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------------------------

/// The answer to a permission request (behaviour 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// One of the request's options; never one of kind `allow_always`.
    Option(String),
    /// Cancel the request.
    Cancel,
}

/// The agents hosted for one workspace.
pub struct AgentHost {
    root: PathBuf,
    agents: Mutex<Vec<Arc<Hosted>>>,
    started: Mutex<u64>,
}

impl std::fmt::Debug for AgentHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentHost")
            .field("root", &self.root)
            .field("agents", &lock(&self.agents).len())
            .finish()
    }
}

impl AgentHost {
    /// A host for the workspace `root` with no agents.
    pub fn new(root: &Path) -> Self {
        AgentHost {
            root: canon(root),
            agents: Mutex::new(Vec::new()),
            started: Mutex::new(0),
        }
    }

    fn find(&self, id: &str) -> Result<Arc<Hosted>, String> {
        lock(&self.agents)
            .iter()
            .find(|h| h.id == id)
            .cloned()
            .ok_or_else(|| format!("there is no hosted agent `{id}`"))
    }

    /// Agents whose process still runs.
    pub fn running(&self) -> usize {
        lock(&self.agents)
            .iter()
            .filter(|h| !lock(&h.facts).exited)
            .count()
    }

    /// Starts `plan` as agent `def` (behaviour 4): refused when `max_agents` run already or the
    /// plan's workspace is not this host's. Answers the hosted agent's id; the protocol goes on
    /// in the background.
    pub fn start(
        &self,
        def: &AgentDef,
        plan: SpawnPlan,
        max_agents: u32,
    ) -> Result<String, String> {
        if plan.cwd != self.root {
            return Err("an agent runs in its workspace only".into());
        }
        if self.running() >= max_agents as usize {
            return Err(format!(
                "{max_agents} agent(s) run already, the most the configuration allows (max_agents); stop one first"
            ));
        }
        let mut cmd = Command::new(&plan.program);
        cmd.args(&plan.args)
            .current_dir(&plan.cwd)
            .env_clear()
            .envs(plan.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = crate::spawn::spawn(&mut cmd)
            .map_err(|e| format!("starting `{}` failed: {e}", plan.program.display()))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let n = {
            let mut s = lock(&self.started);
            *s += 1;
            *s
        };
        let h = Arc::new(Hosted {
            id: format!("{}-{n}", def.id),
            agent: def.id.clone(),
            root: self.root.clone(),
            started_at: unix_now(),
            pid: child.id(),
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            facts: Mutex::new(Facts::default()),
            changed: Condvar::new(),
            turns: Mutex::new(TurnLog::default()),
        });
        let init = {
            let mut f = lock(&h.facts);
            let id = f.id();
            f.init_id = Some(id);
            f.push(
                "host",
                format!(
                    "started `{}` in {} (environment: {})",
                    plan.program.display(),
                    plan.cwd.display(),
                    plan.env_names().join(", ")
                ),
            );
            h.settle(&mut f);
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "initialize",
                "params": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": {"readTextFile": false, "writeTextFile": false},
                        "terminal": false,
                    },
                    "clientInfo": {"name": "ferris-idea", "version": env!("CARGO_PKG_VERSION")},
                },
            })
        };
        if let Some(out) = stdout {
            let h2 = h.clone();
            std::thread::spawn(move || read_stdout(h2, out));
        }
        if let Some(err) = stderr {
            let h2 = h.clone();
            std::thread::spawn(move || read_stderr(h2, err));
        }
        {
            let h2 = h.clone();
            std::thread::spawn(move || watch_exit(h2));
        }
        let id = h.id.clone();
        {
            let mut agents = lock(&self.agents);
            agents.push(h.clone());
            // Keep the newest finished agents only.
            let finished: Vec<String> = agents
                .iter()
                .filter(|a| lock(&a.facts).exited)
                .map(|a| a.id.clone())
                .collect();
            if finished.len() > FINISHED_KEPT {
                let drop_ids = &finished[..finished.len() - FINISHED_KEPT];
                agents.retain(|a| !drop_ids.contains(&a.id));
            }
        }
        h.send(vec![init]);
        Ok(id)
    }

    /// Every hosted agent, oldest first.
    pub fn list(&self) -> Vec<AgentSnapshot> {
        let agents: Vec<Arc<Hosted>> = lock(&self.agents).clone();
        agents.iter().map(|h| h.snapshot(&lock(&h.facts))).collect()
    }

    /// One hosted agent.
    pub fn snapshot(&self, id: &str) -> Result<AgentSnapshot, String> {
        let h = self.find(id)?;
        let f = lock(&h.facts);
        Ok(h.snapshot(&f))
    }

    /// Waits up to `timeout` until `until` holds for the agent; answers it as it is then.
    pub fn wait(
        &self,
        id: &str,
        timeout: Duration,
        until: impl Fn(&AgentSnapshot) -> bool,
    ) -> Result<AgentSnapshot, String> {
        let h = self.find(id)?;
        let deadline = Instant::now() + timeout;
        let mut f = lock(&h.facts);
        loop {
            let s = h.snapshot(&f);
            let now = Instant::now();
            if until(&s) || now >= deadline {
                return Ok(s);
            }
            f = h
                .changed
                .wait_timeout(f, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Sends a prompt (behaviour 6): only while the agent waits for input or is idle.
    pub fn prompt(&self, id: &str, text: &str) -> Result<(), String> {
        let n = text.chars().count();
        if n == 0 || n > PROMPT_MAX_CHARS {
            return Err(format!("a prompt has 1 to {PROMPT_MAX_CHARS} characters"));
        }
        if text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(
                "a prompt holds no control characters other than line breaks and tabs".into(),
            );
        }
        let h = self.find(id)?;
        // The workspace before the turn (behaviour 26), recorded before the prompt leaves.
        let before = crate::agent_turns::snapshot(&h.root);
        let msg = {
            let mut f = lock(&h.facts);
            let state = f.state.unwrap_or(AgentState::Unknown);
            if !matches!(state, AgentState::WaitingForInput | AgentState::Idle) {
                return Err(format!(
                    "agent `{id}` is {} ({}); a prompt is sent only while it waits for input or is idle",
                    state.name(),
                    f.detail
                ));
            }
            let Some(session) = f.session.clone() else {
                return Err(format!("agent `{id}` has no open session"));
            };
            let rpc = f.id();
            f.prompt_id = Some(rpc);
            f.needs_input = None;
            f.refused.clear();
            f.push("prompt", text);
            lock(&h.turns).pending = Some((f.turns + 1, text.to_owned(), before));
            h.settle(&mut f);
            json!({
                "jsonrpc": "2.0",
                "id": rpc,
                "method": "session/prompt",
                "params": {"sessionId": session, "prompt": [{"type": "text", "text": text}]},
            })
        };
        h.send(vec![msg]);
        Ok(())
    }

    /// Answers a permission request (behaviour 15). A standing approval (`allow_always`) is
    /// never given.
    pub fn respond(&self, id: &str, request: u64, choice: Choice) -> Result<String, String> {
        let h = self.find(id)?;
        let (msg, said) = {
            let mut f = lock(&h.facts);
            let Some(i) = f.permissions.iter().position(|p| p.request == request) else {
                return Err(format!(
                    "agent `{id}` has no waiting permission request {request}"
                ));
            };
            let (outcome, said) = match &choice {
                Choice::Cancel => (json!({"outcome": "cancelled"}), "cancelled".to_owned()),
                Choice::Option(o) => {
                    let Some(opt) = f.permissions[i].options.iter().find(|x| x.option_id == *o)
                    else {
                        return Err(format!(
                            "request {request} has no option `{o}`; its options: {}",
                            f.permissions[i]
                                .options
                                .iter()
                                .map(|x| x.option_id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    };
                    if opt.kind == "allow_always" {
                        return Err("Ferris IDEA never gives a hosted agent a standing approval (`allow_always`); choose an option of kind `allow_once` or reject".into());
                    }
                    (
                        json!({"outcome": "selected", "optionId": o}),
                        format!("{} ({})", opt.option_id, opt.kind),
                    )
                }
            };
            let p = f.permissions.remove(i).unwrap_or_else(|| unreachable!());
            let refused = match &choice {
                Choice::Cancel => true,
                Choice::Option(o) => p
                    .options
                    .iter()
                    .any(|x| x.option_id == *o && x.kind.starts_with("reject")),
            };
            if refused {
                f.refused.push(Refused {
                    request,
                    kind: p.kind.clone(),
                    command: last_command(&p.title),
                    paths: p.diffs.iter().map(|d| d.path.clone()).collect(),
                });
            }
            f.push("answer", format!("request {request}: {said}"));
            h.settle(&mut f);
            (
                json!({"jsonrpc": "2.0", "id": p.rpc_id, "result": {"outcome": outcome}}),
                said,
            )
        };
        h.send(vec![msg]);
        Ok(said)
    }

    /// Output entries after `since`, at most `limit`, oldest first, with the agent as it is.
    pub fn read(
        &self,
        id: &str,
        since: u64,
        limit: usize,
    ) -> Result<(AgentSnapshot, Vec<OutputEntry>, bool), String> {
        let h = self.find(id)?;
        let f = lock(&h.facts);
        let newer: Vec<&OutputEntry> = f.output.iter().filter(|e| e.seq > since).collect();
        let more = newer.len() > limit;
        let entries = newer.into_iter().take(limit).cloned().collect();
        Ok((h.snapshot(&f), entries, more))
    }

    /// Stops an agent (behaviour 5): cancels a running turn and every waiting permission request,
    /// closes its input, and kills it when it has not ended after a grace period.
    pub fn stop(&self, id: &str) -> Result<AgentSnapshot, String> {
        let h = self.find(id)?;
        let msgs = {
            let mut f = lock(&h.facts);
            if f.exited {
                return Ok(h.snapshot(&f));
            }
            f.stopping = true;
            let mut msgs: Vec<Value> = f
                .permissions
                .drain(..)
                .map(|p| json!({"jsonrpc": "2.0", "id": p.rpc_id, "result": {"outcome": {"outcome": "cancelled"}}}))
                .collect();
            if let (Some(s), Some(_)) = (&f.session, f.prompt_id) {
                msgs.push(json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": s}}));
            }
            f.push("host", "stop requested");
            h.settle(&mut f);
            msgs
        };
        h.send(msgs);
        lock(&h.stdin).take();
        let ended = |s: &AgentSnapshot| s.exited;
        let s = self.wait(id, STOP_GRACE, ended)?;
        if s.exited {
            return Ok(s);
        }
        h.kill();
        self.wait(id, STOP_GRACE, ended)
    }

    /// The changes of the agent's kept turns, oldest first (behaviour 27).
    pub fn changes(&self, id: &str) -> Result<Vec<crate::agent_turns::TurnChanges>, String> {
        let h = self.find(id)?;
        Ok(lock(&h.turns).done.iter().cloned().collect())
    }

    /// Undoes the newest turn of the agent that was not undone yet (behaviour 28); `turn` must
    /// name it. Refused while the agent works. Answers the paths put back.
    pub fn undo_turn(&self, id: &str, turn: u64) -> Result<Vec<String>, String> {
        let h = self.find(id)?;
        let f = lock(&h.facts);
        if f.prompt_id.is_some() || f.recording {
            return Err(format!(
                "agent `{id}` is working; a turn is undone only between turns"
            ));
        }
        let mut t = lock(&h.turns);
        let Some(newest) = t.done.iter().rev().find(|c| !c.undone).map(|c| c.turn) else {
            return Err(format!("agent `{id}` has no turn to undo"));
        };
        if newest != turn {
            return Err(format!(
                "turns are undone newest first: undo turn {newest} before turn {turn}"
            ));
        }
        let root = h.root.clone();
        let changes = t
            .done
            .iter_mut()
            .find(|c| c.turn == turn)
            .unwrap_or_else(|| unreachable!());
        let done = changes.undo(&root)?;
        drop(t);
        drop(f);
        let mut f = lock(&h.facts);
        f.push(
            "changes",
            format!("turn {turn} undone: {} file(s) put back", done.len()),
        );
        h.changed.notify_all();
        Ok(done)
    }

    /// Notices of every agent, oldest first.
    pub fn notices(&self) -> Vec<Notice> {
        let mut all: Vec<Notice> = self.list().into_iter().flat_map(|s| s.notices).collect();
        all.sort_by_key(|n| n.time);
        all
    }
}

/// One host per workspace and process (`SPEC-0357` behaviour 1): the window and its in-process
/// MCP server see the same agents. A host ends, and stops its agents, with its last holder.
pub fn shared(root: &Path) -> Arc<AgentHost> {
    type Hosts = Mutex<Vec<(PathBuf, std::sync::Weak<AgentHost>)>>;
    static HOSTS: std::sync::LazyLock<Hosts> = std::sync::LazyLock::new(Hosts::default);
    let root = canon(root);
    let mut hosts = lock(&HOSTS);
    hosts.retain(|(_, w)| w.strong_count() > 0);
    if let Some(h) = hosts
        .iter()
        .find(|(r, _)| *r == root)
        .and_then(|(_, w)| w.upgrade())
    {
        return h;
    }
    let h = Arc::new(AgentHost::new(&root));
    hosts.push((root, Arc::downgrade(&h)));
    h
}

impl Drop for AgentHost {
    /// No agent outlives its host.
    fn drop(&mut self) {
        for h in lock(&self.agents).iter() {
            if !lock(&h.facts).exited {
                lock(&h.stdin).take();
                h.kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fake ACP agent: a POSIX shell script, no model, no network.
    fn fake_agent() -> AgentDef {
        AgentDef {
            id: "fake".into(),
            name: None,
            command: PathBuf::from("/bin/sh"),
            args: vec![format!(
                "{}/tests/fixtures/fake-acp-agent.sh",
                env!("CARGO_MANIFEST_DIR")
            )],
            env: vec![],
        }
    }

    fn workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-agent-host-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn started(host: &AgentHost, root: &Path) -> String {
        let def = fake_agent();
        let p = plan(&def, root, |n| std::env::var_os(n)).unwrap();
        let id = host.start(&def, p, 2).unwrap();
        let s = host
            .wait(&id, Duration::from_secs(10), |s| {
                s.state != AgentState::Working
            })
            .unwrap();
        assert_eq!(s.state, AgentState::WaitingForInput, "{s:?}");
        assert!(s.session_ready);
        assert_eq!(s.protocol_version, Some(1));
        id
    }

    fn settled(host: &AgentHost, id: &str) -> AgentSnapshot {
        host.wait(id, Duration::from_secs(10), |s| {
            s.state != AgentState::Working
        })
        .unwrap()
    }

    fn messages(host: &AgentHost, id: &str) -> String {
        let (_, entries, _) = host.read(id, 0, OUTPUT_ENTRIES).unwrap();
        entries
            .iter()
            .filter(|e| e.kind == "message")
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn a_turn_records_every_changed_file_and_undo_puts_them_back() {
        let root = workspace();
        std::fs::write(root.join("keep.txt"), "keep\n").unwrap();
        let host = AgentHost::new(&root);
        let id = started(&host, &root);
        host.prompt(&id, "write-files").unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::Idle, "{s:?}");
        let turns = host.changes(&id).unwrap();
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert_eq!(t.turn, 1);
        assert_eq!(t.prompt, "write-files");
        let files: Vec<(&str, crate::agent_turns::ChangeKind, bool)> = t
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.kind, f.binary))
            .collect();
        // A file the agent's command created counts like its edit (IMP-0548).
        assert_eq!(
            files,
            vec![
                ("Built.class", crate::agent_turns::ChangeKind::Created, true),
                ("notes.txt", crate::agent_turns::ChangeKind::Created, false),
            ]
        );
        let (_, entries, _) = host.read(&id, 0, OUTPUT_ENTRIES).unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.kind == "changes" && e.text.contains("changed 2 file(s)")),
            "{entries:?}"
        );
        assert!(host.undo_turn(&id, 2).is_err());
        let back = host.undo_turn(&id, 1).unwrap();
        assert_eq!(back.len(), 2);
        assert!(!root.join("notes.txt").exists());
        assert!(!root.join("Built.class").exists());
        assert!(root.join("keep.txt").exists());
        assert!(host.changes(&id).unwrap()[0].undone);
        assert!(
            host.undo_turn(&id, 1)
                .unwrap_err()
                .contains("no turn to undo")
        );
        host.stop(&id).unwrap();
    }

    #[test]
    fn an_edit_request_carries_its_diff_and_a_repeat_of_a_refusal_is_marked() {
        let root = workspace();
        std::fs::write(root.join("notes.txt"), "hello\n").unwrap();
        let host = AgentHost::new(&root);
        let id = started(&host, &root);
        host.prompt(&id, "edit-twice").unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::WaitingForApproval, "{s:?}");
        let p = &s.pending_approvals[0];
        assert_eq!(
            p.diffs,
            vec![ProposedDiff {
                path: "notes.txt".into(),
                old_text: Some("hello\n".into()),
                new_text: "hello world\n".into(),
                cut: false,
            }]
        );
        assert_eq!(p.similar_refused, None);
        host.respond(&id, p.request, Choice::Option("reject".into()))
            .unwrap();
        let s = settled(&host, &id);
        let p = &s.pending_approvals[0];
        assert_eq!(p.similar_refused, Some(1), "{p:?}");
        host.respond(&id, p.request, Choice::Option("reject".into()))
            .unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::Idle, "{s:?}");
        // A refused edit is no change (IMP-0549).
        assert!(host.changes(&id).unwrap()[0].files.is_empty());
        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "hello\n"
        );
        host.stop(&id).unwrap();
    }

    #[test]
    fn config_bounds_and_credentials_are_refused() {
        let mut c = HostConfig::default();
        c.agent.push(fake_agent());
        assert_eq!(c.check(), Ok(()));
        c.agent[0].env = vec!["GITHUB_TOKEN".into()];
        assert!(c.check().unwrap_err().contains("looks like a credential"));
        c.agent[0].env = vec!["ANTHROPIC_API_KEY".into()];
        assert!(c.check().is_err());
        c.agent[0].env = vec!["NODE_OPTIONS".into()];
        assert_eq!(c.check(), Ok(()));
        c.agent[0].command = PathBuf::from("sh");
        assert!(c.check().unwrap_err().contains("absolute"));
        c.agent[0].command = PathBuf::from("/bin/sh");
        c.max_agents = 9;
        assert!(c.check().is_err());
        c.max_agents = 2;
        c.agent.push(fake_agent());
        assert!(c.check().unwrap_err().contains("twice"));
    }

    #[test]
    fn a_config_inside_the_workspace_is_ignored() {
        let root = workspace();
        let file = root.join(HOST_FILE);
        std::fs::write(&file, "[[agent]]\nid = \"x\"\ncommand = \"/bin/sh\"\n").unwrap();
        let loaded = load_host_config(&file, &root);
        assert!(loaded.problem.unwrap().contains("inside the workspace"));
        assert!(loaded.config.agent.is_empty());
        let outside = root.with_extension(HOST_FILE);
        std::fs::write(&outside, "[[agent]]\nid = \"x\"\ncommand = \"/bin/sh\"\n").unwrap();
        let loaded = load_host_config(&outside, &root);
        assert_eq!(loaded.problem, None);
        assert_eq!(loaded.config.agent.len(), 1);
        std::fs::write(&outside, "surprise = 1\n").unwrap();
        assert!(load_host_config(&outside, &root).problem.is_some());
    }

    #[test]
    fn the_plan_passes_only_allowlisted_environment_names() {
        let root = workspace();
        let mut def = fake_agent();
        def.env = vec!["NODE_OPTIONS".into()];
        let env = |n: &str| match n {
            "PATH" => Some(OsString::from("/usr/bin")),
            "NODE_OPTIONS" => Some(OsString::from("--x")),
            "AWS_SECRET_ACCESS_KEY" | "OPENAI_API_KEY" => Some(OsString::from("s3cr3t")),
            _ => None,
        };
        let p = plan(&def, &root, env).unwrap();
        assert_eq!(p.env_names(), vec!["PATH", "NODE_OPTIONS"]);
        assert_eq!(p.cwd, canon(&root));
        def.command = PathBuf::from("/no/such/agent");
        assert!(
            plan(&def, &root, env)
                .unwrap_err()
                .contains("does not exist")
        );
    }

    #[test]
    fn a_turn_goes_from_waiting_for_input_through_working_to_idle() {
        let root = workspace();
        let host = AgentHost::new(&root);
        let id = started(&host, &root);
        host.prompt(&id, "hello").unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::Idle, "{s:?}");
        assert_eq!(s.turns, 1);
        assert_eq!(s.last_stop_reason.as_deref(), Some("end_turn"));
        assert!(messages(&host, &id).contains("echo: hello"));
        // The child never sees the test process's own variables (cargo sets this one).
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        host.prompt(&id, "env").unwrap();
        settled(&host, &id);
        assert!(
            messages(&host, &id).contains("cargo=unset"),
            "{}",
            messages(&host, &id)
        );
        let s = host.stop(&id).unwrap();
        assert_eq!(s.state, AgentState::Done, "{s:?}");
        assert!(s.exited);
    }

    #[test]
    fn a_permission_request_waits_for_an_answer_and_never_allows_always() {
        let root = workspace();
        let host = AgentHost::new(&root);
        let id = started(&host, &root);
        host.prompt(&id, "needs-approval").unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::WaitingForApproval, "{s:?}");
        let p = &s.pending_approvals[0];
        assert_eq!(p.title, "Write notes.txt");
        // Nothing happens on its own.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            host.snapshot(&id).unwrap().state,
            AgentState::WaitingForApproval
        );
        assert!(
            s.notices
                .iter()
                .any(|n| n.state == AgentState::WaitingForApproval)
        );
        let refused = host
            .respond(&id, p.request, Choice::Option("always".into()))
            .unwrap_err();
        assert!(refused.contains("standing approval"));
        assert!(
            host.respond(&id, p.request, Choice::Option("nope".into()))
                .is_err()
        );
        host.respond(&id, p.request, Choice::Option("allow".into()))
            .unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::Idle, "{s:?}");
        assert!(messages(&host, &id).contains("approved"));
        host.stop(&id).unwrap();
    }

    #[test]
    fn stopping_cancels_a_waiting_request_and_ends_done() {
        let root = workspace();
        let host = AgentHost::new(&root);
        let id = started(&host, &root);
        host.prompt(&id, "needs-approval").unwrap();
        assert_eq!(settled(&host, &id).state, AgentState::WaitingForApproval);
        let s = host.stop(&id).unwrap();
        assert_eq!(s.state, AgentState::Done, "{s:?}");
        assert!(s.pending_approvals.is_empty());
    }

    #[test]
    fn short_turns_garbage_refused_capabilities_and_crashes_have_their_states() {
        let root = workspace();
        let host = AgentHost::new(&root);
        let id = started(&host, &root);
        host.prompt(&id, "max-tokens").unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::WaitingForInput, "{s:?}");
        assert!(s.detail.contains("token limit"));
        // A file system request is refused with "method not found".
        host.prompt(&id, "fs").unwrap();
        settled(&host, &id);
        assert!(
            messages(&host, &id).contains("fs-refused"),
            "{}",
            messages(&host, &id)
        );
        // A line that is no JSON-RPC leaves the state unknown, never idle.
        host.prompt(&id, "garbage").unwrap();
        let s = settled(&host, &id);
        assert_eq!(s.state, AgentState::Unknown, "{s:?}");
        assert!(host.prompt(&id, "hello").is_err());
        // A crash fails.
        let id2 = started(&host, &root);
        host.prompt(&id2, "crash").unwrap();
        let s = host
            .wait(&id2, Duration::from_secs(10), |s| s.exited)
            .unwrap();
        assert_eq!(s.state, AgentState::Failed, "{s:?}");
        assert_eq!(s.exit_code, Some(3));
        // At most max_agents run at once.
        let def = fake_agent();
        let p = plan(&def, &root, |n| std::env::var_os(n)).unwrap();
        let id3 = host.start(&def, p.clone(), 2).unwrap();
        assert!(host.start(&def, p, 2).unwrap_err().contains("stop one"));
        host.stop(&id).unwrap();
        host.stop(&id3).unwrap();
    }

    #[test]
    fn a_long_line_is_dropped_and_lines_are_split() {
        let mut r = std::io::Cursor::new(b"abc\ndefghij\nxy".to_vec());
        assert_eq!(read_line(&mut r, 5).unwrap(), Some(Some(b"abc".to_vec())));
        assert_eq!(read_line(&mut r, 5).unwrap(), Some(None));
        assert_eq!(read_line(&mut r, 5).unwrap(), Some(Some(b"xy".to_vec())));
        assert_eq!(read_line(&mut r, 5).unwrap(), None);
    }
}
