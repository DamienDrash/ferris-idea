//! The heartbeat for agents (`SPEC-0341`, ROADMAP M19.12): a watchdog that notices hung, sleeping
//! and dead agents, wakes them with the prompt the user wrote or resumes their saved session, and
//! escalates to the person when a limit is reached.
//!
//! Ferris IDEA does not host agents yet (M19.5). The watchdog decides over [`AgentHost`], which the
//! host implements; time comes from a [`Clock`] and every action goes to a [`WatchLog`] first, so
//! the decisions are deterministic and testable without sleeping. Configuration and the
//! watchdog's own state live in the user's settings folder beside the grant file (behaviours 14
//! and 16), never in the workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::permissions::{GRANT_FILE, inside_workspace};
use crate::{AGENT_LOG, Grant};

/// Name of the configuration file in the user's settings folder (behaviour 14).
pub const WATCH_CONFIG_FILE: &str = "agent-watch.toml";
/// Name of the watchdog's state file beside it (behaviour 16).
pub const WATCH_STATE_FILE: &str = "agent-watch-state.json";

/// Shortest and longest time in seconds a setting may name (behaviour 15).
pub const TIME_BOUNDS: (u64, u64) = (60, 86_400);
/// Bounds of `max_attempts`.
pub const ATTEMPT_BOUNDS: (u32, u32) = (1, 10);
/// Bounds of `max_actions`.
pub const ACTION_BOUNDS: (u32, u32) = (1, 50);
/// Longest wake prompt in characters.
pub const PROMPT_MAX_CHARS: usize = 2_000;

/// What the host says an agent is doing (behaviour 1, ADR-0023 rule 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Working on its task.
    Working,
    /// Blocked on an approval of the person.
    WaitingForApproval,
    /// Waiting for an answer of the person.
    WaitingForInput,
    /// Idle while its task is open.
    Idle,
    /// Finished its task.
    Done,
    /// Failed its task.
    Failed,
    /// The host cannot tell; never counts as idle.
    Unknown,
}

impl AgentState {
    /// Waiting for the person: never woken (behaviour 5).
    pub fn needs_person(self) -> bool {
        matches!(self, Self::WaitingForApproval | Self::WaitingForInput)
    }

    /// The watch has ended (behaviour 4).
    pub fn ended(self) -> bool {
        matches!(self, Self::Done | Self::Failed)
    }
}

/// Whether the agent's process runs (behaviour 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    /// It runs.
    Alive,
    /// It is gone.
    Gone,
    /// Not known; never taken as gone.
    Unknown,
}

/// One look at an agent (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// Its state.
    pub state: AgentState,
    /// Its process.
    pub process: Liveness,
    /// A counter that grows with every sign of progress; `None` when the host has none.
    pub progress: Option<u64>,
    /// Money spent so far in cents; `None` when the host does not know.
    pub spent_cents: Option<u64>,
}

/// The session the host saved for an agent (behaviours 7 and 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSession {
    /// The host's id of the session (ACP session id, CLI resume id).
    pub session: String,
    /// The rights the session had.
    pub rights: BTreeSet<Grant>,
    /// Whether the agent can resume it.
    pub resumable: bool,
}

/// A notice to the person (behaviours 6 and 10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    /// The agent.
    pub agent: String,
    /// `escalated` or `unknown_state`.
    pub kind: &'static str,
    /// Why, in words.
    pub reason: String,
}

/// What the agent host offers the watchdog; the M19.5 host implements it.
pub trait AgentHost {
    /// The ids of the agents to watch.
    fn agents(&self) -> Vec<String>;
    /// Looks at one agent.
    fn observe(&mut self, agent: &str) -> Observation;
    /// The session saved for the agent, if any.
    fn saved_session(&self, agent: &str) -> Option<SavedSession>;
    /// Sends the wake prompt to the agent as its next turn.
    fn wake(&mut self, agent: &str, prompt: &str) -> Result<(), String>;
    /// Starts the agent again on its saved session with exactly `rights`.
    fn resume(
        &mut self,
        agent: &str,
        session: &str,
        rights: &BTreeSet<Grant>,
    ) -> Result<(), String>;
    /// Tells the person (the window's notification, a headless host's log).
    fn notify(&mut self, notice: &Notice);
}

/// The watchdog's time in seconds (behaviour 2).
pub trait Clock {
    /// Now, in seconds since the Unix epoch.
    fn now(&self) -> u64;
}

/// The system's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// Where the watchdog's lines go (behaviour 13).
pub trait WatchLog {
    /// Writes one line; an error means it was not written.
    fn record(&mut self, line: &Value) -> Result<(), String>;
}

/// The workspace's agent log `.ferris-idea/agent-log.jsonl`.
#[derive(Debug, Clone)]
pub struct AgentLogFile {
    /// The workspace root.
    pub root: PathBuf,
}

impl WatchLog for AgentLogFile {
    fn record(&mut self, line: &Value) -> Result<(), String> {
        crate::log_agent_action(&self.root, line)
            .map_err(|e| format!("{}: {e}", self.root.join(AGENT_LOG).display()))
    }
}

/// The configuration of one workspace (behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WatchConfig {
    /// Whether the watchdog acts.
    pub enabled: bool,
    /// The text sent to wake an agent, written by the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wake_prompt: Option<String>,
    /// Seconds without progress before a working agent counts as hung.
    pub stall_after: u64,
    /// Seconds without progress before an idle agent counts as asleep.
    pub idle_after: u64,
    /// Seconds to wait after a wake or resume before judging again.
    pub grace: u64,
    /// Seconds in `unknown` before the person gets a notice.
    pub unknown_after: u64,
    /// Wakes and resumes in a row without progress.
    pub max_attempts: u32,
    /// Wakes and resumes of an agent in all.
    pub max_actions: u32,
    /// Spending in cents at which no more actions are taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_cents: Option<u64>,
    /// Whether a dead agent's saved session is resumed.
    pub resume: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            wake_prompt: None,
            stall_after: 600,
            idle_after: 300,
            grace: 300,
            unknown_after: 900,
            max_attempts: 3,
            max_actions: 10,
            budget_cents: None,
            resume: true,
        }
    }
}

impl WatchConfig {
    /// The first value outside the bounds of behaviour 15, in words.
    pub fn check(&self) -> Result<(), String> {
        let times = [
            ("stall_after", self.stall_after),
            ("idle_after", self.idle_after),
            ("grace", self.grace),
            ("unknown_after", self.unknown_after),
        ];
        for (name, v) in times {
            if !(TIME_BOUNDS.0..=TIME_BOUNDS.1).contains(&v) {
                return Err(format!(
                    "`{name}` is {v} seconds; it must be from {} to {}",
                    TIME_BOUNDS.0, TIME_BOUNDS.1
                ));
            }
        }
        for (name, v, (lo, hi)) in [
            ("max_attempts", self.max_attempts, ATTEMPT_BOUNDS),
            ("max_actions", self.max_actions, ACTION_BOUNDS),
        ] {
            if !(lo..=hi).contains(&v) {
                return Err(format!("`{name}` is {v}; it must be from {lo} to {hi}"));
            }
        }
        if let Some(p) = &self.wake_prompt {
            check_prompt(p)?;
        }
        Ok(())
    }
}

/// Whether `prompt` is a wake prompt the watchdog may send (behaviour 15).
pub fn check_prompt(prompt: &str) -> Result<(), String> {
    let chars = prompt.chars().count();
    if prompt.trim().is_empty() || chars > PROMPT_MAX_CHARS {
        return Err(format!(
            "the wake prompt has {chars} characters; it must have 1 to {PROMPT_MAX_CHARS} and not be blank"
        ));
    }
    if prompt
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err("the wake prompt holds a control character".into());
    }
    Ok(())
}

/// The configuration and state files beside `grant_file` (behaviours 14 and 16): its name with
/// `agent-permissions.toml` replaced, so a test's grant file `x.agent-permissions.toml` has
/// `x.agent-watch.toml` beside it.
pub fn watch_files(grant_file: &Path) -> (PathBuf, PathBuf) {
    let name = grant_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = name.strip_suffix(GRANT_FILE).unwrap_or("");
    (
        grant_file.with_file_name(format!("{prefix}{WATCH_CONFIG_FILE}")),
        grant_file.with_file_name(format!("{prefix}{WATCH_STATE_FILE}")),
    )
}

/// The tables of the configuration file: `path` and the keys of [`WatchConfig`] each.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    workspace: Vec<toml::Table>,
}

/// The `path` and the configuration of one table.
fn split_table(table: &toml::Table) -> Result<(PathBuf, WatchConfig), String> {
    let mut rest = table.clone();
    let path = match rest.remove("path") {
        Some(toml::Value::String(p)) => PathBuf::from(p),
        _ => return Err("a [[workspace]] table needs `path`".into()),
    };
    let config: WatchConfig = toml::Value::Table(rest)
        .try_into()
        .map_err(|e: toml::de::Error| format!("{}: {}", path.display(), e.message()))?;
    config
        .check()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((path, config))
}

/// The configuration of a workspace as read, with where it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LoadedConfig {
    /// The configuration; the default (off) when there is none or it has a problem.
    pub config: WatchConfig,
    /// The file consulted.
    pub file: Option<PathBuf>,
    /// Problem with the file; the watchdog stays off then.
    pub problem: Option<String>,
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn read_config_file(file: &Path, root: &Path) -> Result<ConfigFile, String> {
    if inside_workspace(file, root) {
        return Err("the watchdog's configuration lies inside the workspace and is ignored".into());
    }
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ConfigFile::default()),
        Err(e) => return Err(e.to_string()),
    };
    let parsed: ConfigFile = toml::from_str(&text).map_err(|e| e.to_string())?;
    for table in &parsed.workspace {
        split_table(table)?;
    }
    Ok(parsed)
}

/// Reads the configuration of `root` from `file` (behaviours 14 and 15).
pub fn load_watch_config(file: &Path, root: &Path) -> LoadedConfig {
    let mut out = LoadedConfig {
        file: Some(file.to_path_buf()),
        ..LoadedConfig::default()
    };
    match read_config_file(file, root) {
        Ok(parsed) => {
            let root = canon(root);
            if let Some((_, config)) = parsed
                .workspace
                .iter()
                .filter_map(|t| split_table(t).ok())
                .find(|(path, _)| canon(path) == root)
            {
                out.config = config;
            }
        }
        Err(e) => out.problem = Some(e),
    }
    out
}

/// Writes `config` as the configuration of `root` into `file`, keeping the other workspaces'
/// tables. Refused for a file inside the workspace, one with a problem, or values out of bounds.
pub fn save_watch_config(file: &Path, root: &Path, config: &WatchConfig) -> Result<(), String> {
    config.check()?;
    let mut parsed =
        read_config_file(file, root).map_err(|e| format!("{}: {e}", file.display()))?;
    let root = canon(root);
    parsed
        .workspace
        .retain(|t| split_table(t).is_ok_and(|(path, _)| canon(&path) != root));
    let mut table = toml::Table::new();
    table.insert(
        "path".into(),
        toml::Value::String(root.to_string_lossy().into_owned()),
    );
    let toml::Value::Table(values) = toml::Value::try_from(config).map_err(|e| e.to_string())?
    else {
        return Err("the configuration is no table".into());
    };
    table.extend(values);
    parsed.workspace.push(table);
    let text = toml::to_string(&parsed).map_err(|e| e.to_string())?;
    crate::write_atomic(file, text).map_err(|e| format!("{}: {e}", file.display()))
}

/// Where the watchdog stands with an agent (behaviour 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Making progress, or not long enough without it.
    Watching,
    /// Waiting for an approval or an answer.
    NeedsPerson,
    /// Its state is unknown.
    Unknown,
    /// Woken; within the grace time.
    Woken,
    /// Resumed; within the grace time.
    Resumed,
    /// Escalated to the person; no action until acknowledged.
    Escalated,
    /// Done or failed.
    Ended,
}

/// The watchdog's record of one agent (behaviour 16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// Where the watchdog stands.
    pub phase: Phase,
    /// The last state seen.
    pub state: AgentState,
    /// The last progress counter seen.
    pub progress: Option<u64>,
    /// When progress was last seen.
    pub progress_at: u64,
    /// Wakes and resumes in a row without progress.
    pub attempts: u32,
    /// Wakes and resumes in all; never set back.
    pub actions: u32,
    /// When the last wake or resume was sent.
    pub action_at: Option<u64>,
    /// Why it was escalated.
    pub escalation: Option<String>,
    /// Whether the notice for the unknown state was sent.
    pub unknown_noticed: bool,
}

impl AgentRecord {
    fn first(obs: &Observation, now: u64) -> Self {
        Self {
            phase: Phase::Watching,
            state: obs.state,
            progress: obs.progress,
            progress_at: now,
            attempts: 0,
            actions: 0,
            action_at: None,
            escalation: None,
            unknown_noticed: false,
        }
    }
}

/// What a round did for an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatchEvent {
    /// The agent.
    pub agent: String,
    /// `wake`, `resume`, `escalate`, `notice` or `acknowledge`.
    pub action: &'static str,
    /// Why.
    pub reason: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    workspaces: BTreeMap<String, BTreeMap<String, AgentRecord>>,
}

fn state_key(root: &Path) -> String {
    canon(root).to_string_lossy().into_owned()
}

fn read_state_file(file: &Path) -> Result<StateFile, String> {
    match std::fs::read_to_string(file) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StateFile::default()),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

/// The saved records of `root`'s agents; empty with the problem when the file cannot be read.
pub fn load_watch_state(
    file: &Path,
    root: &Path,
) -> (BTreeMap<String, AgentRecord>, Option<String>) {
    match read_state_file(file) {
        Ok(mut s) => (
            s.workspaces.remove(&state_key(root)).unwrap_or_default(),
            None,
        ),
        Err(e) => (BTreeMap::new(), Some(e)),
    }
}

/// Saves the records of `root`'s agents into `file`, keeping other workspaces' records; a file
/// that cannot be read is replaced.
pub fn save_watch_state(
    file: &Path,
    root: &Path,
    records: &BTreeMap<String, AgentRecord>,
) -> Result<(), String> {
    let mut s = read_state_file(file).unwrap_or_default();
    s.workspaces.insert(state_key(root), records.clone());
    let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    crate::write_atomic(file, text).map_err(|e| format!("{}: {e}", file.display()))
}

/// The watchdog of one workspace.
#[derive(Debug, Clone)]
pub struct Watchdog {
    /// Its configuration.
    pub config: WatchConfig,
    /// The rights the workspace grants agents now (behaviour 12).
    pub grants: BTreeSet<Grant>,
    /// Its records by agent id.
    pub records: BTreeMap<String, AgentRecord>,
}

/// What to do with an agent in this round.
enum Due {
    Nothing,
    Wake(String),
    Resume(String),
    Escalate(String),
}

impl Watchdog {
    /// A watchdog with `config`, the workspace's current `grants` and saved `records`.
    pub fn new(
        config: WatchConfig,
        grants: BTreeSet<Grant>,
        records: BTreeMap<String, AgentRecord>,
    ) -> Self {
        Self {
            config,
            grants,
            records,
        }
    }

    /// One round (behaviour 3): observes every agent once and takes at most one action each.
    /// Returns what it did; the records show where it stands.
    pub fn tick(
        &mut self,
        host: &mut dyn AgentHost,
        clock: &dyn Clock,
        log: &mut dyn WatchLog,
    ) -> Vec<WatchEvent> {
        if !self.config.enabled {
            return Vec::new();
        }
        let now = clock.now();
        let mut ids = host.agents();
        ids.sort();
        ids.dedup();
        let mut events = Vec::new();
        for id in ids {
            let obs = host.observe(&id);
            let due = self.judge(&id, &obs, host, now);
            self.act(&id, &obs, due, host, log, now, &mut events);
        }
        events
    }

    /// Updates the record of `id` from `obs` and says what is due (behaviours 2 and 4–9).
    fn judge(&mut self, id: &str, obs: &Observation, host: &dyn AgentHost, now: u64) -> Due {
        let config = &self.config;
        let rec = self
            .records
            .entry(id.to_owned())
            .or_insert_with(|| AgentRecord::first(obs, now));
        let grew = matches!((obs.progress, rec.progress), (Some(n), Some(o)) if n > o)
            || (obs.progress.is_some() && rec.progress.is_none());
        let changed = obs.state != rec.state;
        if grew || changed {
            rec.progress_at = now;
            rec.attempts = 0;
            if changed {
                rec.unknown_noticed = false;
            }
        }
        rec.progress = obs.progress.or(rec.progress);
        rec.state = obs.state;
        if rec.phase == Phase::Escalated {
            return Due::Nothing;
        }
        if obs.state.ended() {
            rec.phase = Phase::Ended;
            return Due::Nothing;
        }
        let in_grace = rec.action_at.is_some_and(|at| now < at + config.grace);
        // A process that is gone outweighs every state it last showed; an unknown liveness is
        // never taken as gone (behaviour 7).
        if obs.process == Liveness::Gone {
            if in_grace {
                return Due::Nothing;
            }
            if !config.resume {
                return Due::Escalate("the process is gone and resuming is off".into());
            }
            return match host.saved_session(id) {
                Some(s) if s.resumable => Due::Resume("the process is gone".into()),
                _ => Due::Escalate("the process is gone and no resumable session was saved".into()),
            };
        }
        if obs.state.needs_person() {
            rec.phase = Phase::NeedsPerson;
            return Due::Nothing;
        }
        if obs.state == AgentState::Unknown {
            rec.phase = Phase::Unknown;
            return Due::Nothing;
        }
        if in_grace {
            return Due::Nothing;
        }
        let (limit, what) = if obs.state == AgentState::Idle {
            (config.idle_after, "asleep: idle")
        } else {
            (config.stall_after, "hung: working")
        };
        let quiet = now.saturating_sub(rec.progress_at);
        if quiet < limit {
            rec.phase = Phase::Watching;
            return Due::Nothing;
        }
        Due::Wake(format!("{what} without progress for {quiet} s"))
    }

    /// The limit an action would pass, if any (behaviour 9).
    fn limit_reached(&self, rec: &AgentRecord, obs: &Observation) -> Option<String> {
        let c = &self.config;
        if rec.attempts >= c.max_attempts {
            return Some(format!(
                "no progress after {} wakes or resumes in a row",
                rec.attempts
            ));
        }
        if rec.actions >= c.max_actions {
            return Some(format!(
                "{} wakes or resumes in all, the limit",
                rec.actions
            ));
        }
        match (c.budget_cents, obs.spent_cents) {
            (Some(b), Some(s)) if s >= b => {
                Some(format!("the budget of {b} cents is spent ({s} cents)"))
            }
            (Some(_), None) => Some("a budget is set but the spending is not reported".into()),
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn act(
        &mut self,
        id: &str,
        obs: &Observation,
        due: Due,
        host: &mut dyn AgentHost,
        log: &mut dyn WatchLog,
        now: u64,
        events: &mut Vec<WatchEvent>,
    ) {
        let Some(rec) = self.records.get(id).cloned() else {
            return;
        };
        let line = |action: &str, reason: &str| {
            json!({
                "time": now, "tool": "agent_watch", "agent": id, "action": action,
                "reason": reason, "attempt": rec.attempts + 1, "actions": rec.actions,
            })
        };
        if matches!(due, Due::Nothing) {
            self.notice_unknown(id, host, log, now, events);
            return;
        }
        let limit = self.limit_reached(&rec, obs);
        let due = match (due, limit) {
            (Due::Wake(reason) | Due::Resume(reason), Some(limit)) => {
                Due::Escalate(format!("{reason}; {limit}"))
            }
            (Due::Wake(reason), None) if self.config.wake_prompt.is_none() => {
                Due::Escalate(format!("{reason}; no wake prompt is configured"))
            }
            (other, _) => other,
        };
        let outcome = match &due {
            Due::Wake(reason) => {
                let prompt = self.config.wake_prompt.clone().unwrap_or_default();
                let mut l = line("wake", reason);
                l["prompt_chars"] = json!(prompt.chars().count());
                log.record(&l)
                    .map_err(|e| format!("{reason}; the agent log cannot be written ({e})"))
                    .and_then(|()| {
                        host.wake(id, &prompt)
                            .map_err(|e| format!("{reason}; waking failed: {e}"))
                    })
                    .map(|()| ("wake", reason.clone(), Phase::Woken))
            }
            Due::Resume(reason) => {
                let session = host.saved_session(id);
                let session = session.filter(|s| s.resumable);
                match session {
                    None => Err(format!("{reason}; the saved session is gone")),
                    Some(s) => {
                        let rights: BTreeSet<Grant> =
                            s.rights.intersection(&self.grants).copied().collect();
                        let mut l = line("resume", reason);
                        l["session"] = json!(s.session);
                        l["rights"] = json!(rights.iter().map(|g| g.name()).collect::<Vec<_>>());
                        log.record(&l)
                            .map_err(|e| format!("{reason}; the agent log cannot be written ({e})"))
                            .and_then(|()| {
                                host.resume(id, &s.session, &rights)
                                    .map_err(|e| format!("{reason}; resuming failed: {e}"))
                            })
                            .map(|()| ("resume", reason.clone(), Phase::Resumed))
                    }
                }
            }
            Due::Escalate(reason) => Err(reason.clone()),
            Due::Nothing => return,
        };
        let rec = self.records.get_mut(id).expect("judged above");
        match outcome {
            Ok((action, reason, phase)) => {
                rec.attempts += 1;
                rec.actions += 1;
                rec.action_at = Some(now);
                rec.phase = phase;
                events.push(WatchEvent {
                    agent: id.to_owned(),
                    action,
                    reason,
                });
            }
            Err(reason) => {
                rec.phase = Phase::Escalated;
                rec.escalation = Some(reason.clone());
                let _ = log.record(&json!({
                    "time": now, "tool": "agent_watch", "agent": id, "action": "escalate",
                    "reason": reason, "attempts": rec.attempts, "actions": rec.actions,
                }));
                host.notify(&Notice {
                    agent: id.to_owned(),
                    kind: "escalated",
                    reason: reason.clone(),
                });
                events.push(WatchEvent {
                    agent: id.to_owned(),
                    action: "escalate",
                    reason,
                });
            }
        }
    }

    /// The one notice for an agent long in `unknown` (behaviour 6).
    fn notice_unknown(
        &mut self,
        id: &str,
        host: &mut dyn AgentHost,
        log: &mut dyn WatchLog,
        now: u64,
        events: &mut Vec<WatchEvent>,
    ) {
        let after = self.config.unknown_after;
        let Some(rec) = self.records.get_mut(id) else {
            return;
        };
        if rec.phase != Phase::Unknown
            || rec.unknown_noticed
            || now.saturating_sub(rec.progress_at) < after
        {
            return;
        }
        rec.unknown_noticed = true;
        let reason = format!(
            "the state has been unknown without progress for {} s; it is not woken",
            now.saturating_sub(rec.progress_at)
        );
        let _ = log.record(&json!({
            "time": now, "tool": "agent_watch", "agent": id, "action": "notice", "reason": reason,
        }));
        host.notify(&Notice {
            agent: id.to_owned(),
            kind: "unknown_state",
            reason: reason.clone(),
        });
        events.push(WatchEvent {
            agent: id.to_owned(),
            action: "notice",
            reason,
        });
    }

    /// The person acknowledges the escalation of `agent` (behaviour 10): the attempts in a row
    /// and the clock of progress start again; all actions and the budget do not. `false` when
    /// the agent is not escalated.
    pub fn acknowledge(
        &mut self,
        agent: &str,
        now: u64,
        log: &mut dyn WatchLog,
    ) -> Result<bool, String> {
        let Some(rec) = self.records.get_mut(agent) else {
            return Ok(false);
        };
        if rec.phase != Phase::Escalated {
            return Ok(false);
        }
        log.record(&json!({
            "time": now, "tool": "agent_watch", "agent": agent, "action": "acknowledge",
            "reason": rec.escalation, "actions": rec.actions,
        }))?;
        acknowledge_record(rec, now);
        Ok(true)
    }
}

/// Sets `rec` back after an acknowledged escalation (behaviour 10).
pub fn acknowledge_record(rec: &mut AgentRecord, now: u64) {
    rec.phase = Phase::Watching;
    rec.escalation = None;
    rec.attempts = 0;
    rec.progress_at = now;
    rec.action_at = None;
}
