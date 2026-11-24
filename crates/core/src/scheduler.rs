//! The scheduler for agent tasks (`SPEC-0342`, ROADMAP M19.13): tasks the user defined, fired by
//! time or by event, queued by priority and dependency, started within hard limits per resource,
//! outside quiet hours and within a budget per run.
//!
//! Ferris IDEA does not host agents yet (M19.5). The scheduler decides over [`TaskHost`], which the
//! host implements (the agent host of `SPEC-0341`'s `AgentHost` can implement it beside that
//! trait); time comes from a [`Clock`], events from [`EventSource`]s, and every action goes to a
//! [`ScheduleLog`] first, so the decisions are deterministic and testable without sleeping.
//! Configuration and the scheduler's own state live in the user's settings folder beside the grant
//! file (behaviours 19 and 20), never in the workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::permissions::{GRANT_FILE, inside_workspace};
use crate::{AGENT_LOG, Grant};

/// Name of the configuration file in the user's settings folder (behaviour 20).
pub const SCHEDULE_FILE: &str = "agent-schedule.toml";
/// Name of the scheduler's state file beside it (behaviour 19).
pub const SCHEDULE_STATE_FILE: &str = "agent-schedule-state.json";

/// Longest prompt in characters (behaviour 2).
pub const PROMPT_MAX_CHARS: usize = 4_000;
/// Most tasks per workspace.
pub const MAX_TASKS: usize = 100;
/// Most resources per task.
pub const MAX_RESOURCES: usize = 8;
/// Longest id or resource name.
pub const NAME_MAX_CHARS: usize = 64;
/// Highest priority.
pub const PRIORITY_MAX: u8 = 9;
/// Default priority.
pub const PRIORITY_DEFAULT: u8 = 5;
/// Bounds of `budget_cents`.
pub const BUDGET_BOUNDS: (u64, u64) = (1, 10_000);
/// Bounds of `max_minutes`.
pub const MINUTES_BOUNDS: (u64, u64) = (1, 1_440);
/// Bounds of a recurring trigger's `every` in seconds (behaviour 5).
pub const EVERY_BOUNDS: (u64, u64) = (300, 2_592_000);
/// Longest event filter (behaviour 6).
pub const FILTER_MAX_CHARS: usize = 256;
/// Bounds of `utc_offset_minutes`.
pub const OFFSET_BOUNDS: (i32, i32) = (-720, 840);
/// Bounds of `max_parallel`.
pub const PARALLEL_BOUNDS: (u32, u32) = (1, 8);
/// Bounds of `max_queue`.
pub const QUEUE_BOUNDS: (u32, u32) = (1, 200);
/// Bounds of `max_runs_per_day`.
pub const RUNS_PER_DAY_BOUNDS: (u32, u32) = (1, 500);
/// Bounds of a resource limit.
pub const LIMIT_BOUNDS: (u32, u32) = (1, 16);
/// Longest kind, subject or data of an event handed to the host (behaviour 8).
pub const EVENT_FIELD_MAX_CHARS: usize = 4_000;
/// Longest event subject written to the agent log (behaviour 8).
pub const LOG_SUBJECT_MAX_CHARS: usize = 256;
/// Finished runs kept (behaviour 19).
pub const HISTORY_MAX: usize = 200;
/// Most files the file-change source looks at (behaviour 7).
pub const FILE_SOURCE_MAX_FILES: usize = 20_000;
/// The resource a task occupies when it names none.
pub const DEFAULT_RESOURCE: &str = "machine";
/// Folders the file-change source skips.
pub const FILE_SOURCE_SKIPPED: [&str; 4] = [".git", ".ferris-idea", "target", "node_modules"];

const DAY: i64 = 86_400;

/// The scheduler's time in seconds (behaviour 3); the same shape as `SPEC-0341`'s clock.
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
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// The kinds of events a task can wait for (behaviour 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A CI run failed.
    CiFailed,
    /// A pull request got a comment.
    PrComment,
    /// A branch has a merge conflict.
    MergeConflict,
    /// A file of the workspace was added, changed or removed.
    FileChanged,
    /// A ticket changed its status (ROADMAP M20).
    TicketStatus,
}

impl EventKind {
    /// Every kind.
    pub const ALL: [EventKind; 5] = [
        EventKind::CiFailed,
        EventKind::PrComment,
        EventKind::MergeConflict,
        EventKind::FileChanged,
        EventKind::TicketStatus,
    ];

    /// The kind's name in files and answers.
    pub fn name(self) -> &'static str {
        match self {
            EventKind::CiFailed => "ci_failed",
            EventKind::PrComment => "pr_comment",
            EventKind::MergeConflict => "merge_conflict",
            EventKind::FileChanged => "file_changed",
            EventKind::TicketStatus => "ticket_status",
        }
    }

    /// The kind named `name`.
    pub fn from_name(name: &str) -> Option<EventKind> {
        EventKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// One event from a source (behaviour 6). `subject` and `data` are untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Its kind.
    pub kind: EventKind,
    /// Branch, pull request, path, ticket and status.
    pub subject: String,
    /// What the source knows beyond the subject (a comment, a CI log line); untrusted.
    pub data: String,
}

/// A source of events, polled once per round (behaviour 7).
pub trait EventSource {
    /// The events since the last poll.
    fn poll(&mut self) -> Vec<Event>;
}

/// The events of every source, in the order of the sources.
pub fn collect_events(sources: &mut [&mut dyn EventSource]) -> Vec<Event> {
    sources.iter_mut().flat_map(|s| s.poll()).collect()
}

/// What a trigger is (behaviours 4 to 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    /// Once at a time.
    Once {
        /// Seconds since the Unix epoch.
        at: u64,
    },
    /// Every `seconds` at `from + k × seconds`.
    Every {
        /// The interval.
        seconds: u64,
        /// The first fire time; default: creation time plus `seconds`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<u64>,
    },
    /// Daily at `HH:MM` in the workspace's time.
    Daily {
        /// `HH:MM`.
        at: String,
    },
    /// On an event of `kind` whose subject matches `filter`.
    Event {
        /// The kind.
        kind: EventKind,
        /// The pattern for the subject; none matches every subject.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<String>,
    },
}

impl Trigger {
    /// `once`, `every`, `daily` or `event`.
    pub fn name(&self) -> &'static str {
        match self {
            Trigger::Once { .. } => "once",
            Trigger::Every { .. } => "every",
            Trigger::Daily { .. } => "daily",
            Trigger::Event { .. } => "event",
        }
    }
}

fn default_priority() -> u8 {
    PRIORITY_DEFAULT
}

fn default_resources() -> Vec<String> {
    vec![DEFAULT_RESOURCE.to_owned()]
}

fn yes() -> bool {
    true
}

/// A task the user defined (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDef {
    /// Its id.
    pub id: String,
    /// The instruction the agent gets, written by the user.
    pub prompt: String,
    /// When it fires.
    pub trigger: Trigger,
    /// 0 to 9, higher first.
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Ids of the tasks it depends on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    /// The resources a run occupies.
    #[serde(default = "default_resources")]
    pub resources: Vec<String>,
    /// Budget of one run in cents.
    pub budget_cents: u64,
    /// Longest run in minutes.
    pub max_minutes: u64,
    /// The rights a run may use.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub rights: BTreeSet<Grant>,
    /// Whether it fires.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Who created it (`user` for the file, `mcp` for `schedule_add`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    /// When it was created; 0 when not known.
    #[serde(default)]
    pub created_at: u64,
}

/// Quiet hours in the workspace's time (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    /// Start, `HH:MM`.
    pub from: String,
    /// End, `HH:MM`; before `from` spans midnight.
    pub to: String,
}

impl QuietHours {
    /// Whether the minute of the day `minute` lies in the quiet hours.
    pub fn contains(&self, minute: u32) -> bool {
        let (Some(from), Some(to)) = (parse_hhmm(&self.from), parse_hhmm(&self.to)) else {
            return false;
        };
        if from < to {
            (from..to).contains(&minute)
        } else {
            minute >= from || minute < to
        }
    }
}

/// The schedule of one workspace (behaviour 20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Schedule {
    /// Whether triggers fire and runs start.
    pub enabled: bool,
    /// The workspace's time as an offset from UTC in minutes.
    pub utc_offset_minutes: i32,
    /// When no run starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<QuietHours>,
    /// Runs at once.
    pub max_parallel: u32,
    /// Queued runs at most.
    pub max_queue: u32,
    /// Runs started per day at most.
    pub max_runs_per_day: u32,
    /// Runs at once per resource; 1 for a resource not named.
    pub limits: BTreeMap<String, u32>,
    /// The tasks.
    pub task: Vec<TaskDef>,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            enabled: false,
            utc_offset_minutes: 0,
            quiet_hours: None,
            max_parallel: 1,
            max_queue: 50,
            max_runs_per_day: 24,
            limits: BTreeMap::new(),
            task: Vec::new(),
        }
    }
}

/// `HH:MM` as minutes of the day.
pub fn parse_hhmm(text: &str) -> Option<u32> {
    let (h, m) = text.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn check_name(what: &str, name: &str, extra: &[char]) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.chars().count() <= NAME_MAX_CHARS
        && name.chars().all(|c| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || c == '-'
                || c == '_'
                || extra.contains(&c)
        });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{what} `{name}` must have 1 to {NAME_MAX_CHARS} characters of a-z, 0-9, -, _{}",
            extra.iter().map(|c| format!(", {c}")).collect::<String>()
        ))
    }
}

fn in_bounds<T: PartialOrd + std::fmt::Display + Copy>(
    name: &str,
    v: T,
    (lo, hi): (T, T),
) -> Result<(), String> {
    if lo <= v && v <= hi {
        Ok(())
    } else {
        Err(format!("`{name}` is {v}; it must be from {lo} to {hi}"))
    }
}

/// Whether `prompt` is a prompt a task may have (behaviour 2).
pub fn check_prompt(prompt: &str) -> Result<(), String> {
    let chars = prompt.chars().count();
    if prompt.trim().is_empty() || chars > PROMPT_MAX_CHARS {
        return Err(format!(
            "the prompt has {chars} characters; it must have 1 to {PROMPT_MAX_CHARS} and not be blank"
        ));
    }
    if prompt
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err("the prompt holds a control character".into());
    }
    Ok(())
}

impl TaskDef {
    /// The first problem of the task on its own (behaviours 1, 2, 5, 6).
    pub fn check(&self) -> Result<(), String> {
        check_name("the task id", &self.id, &[])?;
        let at = |e: String| format!("task `{}`: {e}", self.id);
        check_prompt(&self.prompt).map_err(at)?;
        if self.priority > PRIORITY_MAX {
            return Err(at(format!(
                "`priority` is {}; it must be from 0 to {PRIORITY_MAX}",
                self.priority
            )));
        }
        in_bounds("budget_cents", self.budget_cents, BUDGET_BOUNDS).map_err(at)?;
        in_bounds("max_minutes", self.max_minutes, MINUTES_BOUNDS).map_err(at)?;
        if self.resources.len() > MAX_RESOURCES {
            return Err(at(format!(
                "it names {} resources; at most {MAX_RESOURCES}",
                self.resources.len()
            )));
        }
        for r in &self.resources {
            check_name("the resource", r, &[':', '.']).map_err(at)?;
        }
        if self.after.iter().any(|d| d == &self.id) {
            return Err(at("it depends on itself".into()));
        }
        match &self.trigger {
            Trigger::Once { .. } => {}
            Trigger::Every { seconds, .. } => {
                in_bounds("every", *seconds, EVERY_BOUNDS).map_err(at)?;
            }
            Trigger::Daily { at: time } => {
                if parse_hhmm(time).is_none() {
                    return Err(at(format!("the daily time `{time}` is not HH:MM")));
                }
            }
            Trigger::Event { filter, .. } => {
                if let Some(f) = filter {
                    let n = f.chars().count();
                    if n == 0 || n > FILTER_MAX_CHARS || f.chars().any(char::is_control) {
                        return Err(at(format!(
                            "the filter must have 1 to {FILTER_MAX_CHARS} characters without control characters"
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Schedule {
    /// The task `id`.
    pub fn task(&self, id: &str) -> Option<&TaskDef> {
        self.task.iter().find(|t| t.id == id)
    }

    /// The first value outside the bounds, unknown dependency or cycle, in words (behaviours 2
    /// and 20).
    pub fn check(&self) -> Result<(), String> {
        in_bounds("utc_offset_minutes", self.utc_offset_minutes, OFFSET_BOUNDS)?;
        in_bounds("max_parallel", self.max_parallel, PARALLEL_BOUNDS)?;
        in_bounds("max_queue", self.max_queue, QUEUE_BOUNDS)?;
        in_bounds(
            "max_runs_per_day",
            self.max_runs_per_day,
            RUNS_PER_DAY_BOUNDS,
        )?;
        for (name, limit) in &self.limits {
            check_name("the resource", name, &[':', '.'])?;
            in_bounds(&format!("limits.{name}"), *limit, LIMIT_BOUNDS)?;
        }
        if let Some(q) = &self.quiet_hours {
            match (parse_hhmm(&q.from), parse_hhmm(&q.to)) {
                (Some(a), Some(b)) if a != b => {}
                (Some(_), Some(_)) => {
                    return Err("the quiet hours start and end at the same time".into());
                }
                _ => return Err("the quiet hours need `from` and `to` as HH:MM".into()),
            }
        }
        if self.task.len() > MAX_TASKS {
            return Err(format!(
                "{} tasks; at most {MAX_TASKS} per workspace",
                self.task.len()
            ));
        }
        let mut ids = BTreeSet::new();
        for t in &self.task {
            t.check()?;
            if !ids.insert(t.id.as_str()) {
                return Err(format!("the task id `{}` is used twice", t.id));
            }
        }
        for t in &self.task {
            if let Some(d) = t.after.iter().find(|d| !ids.contains(d.as_str())) {
                return Err(format!(
                    "task `{}` depends on `{d}`, which does not exist",
                    t.id
                ));
            }
        }
        if let Some(cycle) = self.cycle() {
            return Err(format!(
                "the dependencies form a cycle: {}",
                cycle.join(" → ")
            ));
        }
        Ok(())
    }

    /// A cycle of dependencies, if any, as the ids along it.
    fn cycle(&self) -> Option<Vec<String>> {
        fn visit<'a>(
            id: &'a str,
            s: &'a Schedule,
            path: &mut Vec<&'a str>,
            done: &mut BTreeSet<&'a str>,
        ) -> Option<Vec<String>> {
            if let Some(i) = path.iter().position(|p| *p == id) {
                let mut c: Vec<String> = path[i..].iter().map(|p| (*p).to_owned()).collect();
                c.push(id.to_owned());
                return Some(c);
            }
            if done.contains(id) {
                return None;
            }
            path.push(id);
            if let Some(t) = s.task(id) {
                for d in &t.after {
                    if let Some(c) = visit(d, s, path, done) {
                        return Some(c);
                    }
                }
            }
            path.pop();
            done.insert(id);
            None
        }
        let mut done = BTreeSet::new();
        self.task
            .iter()
            .find_map(|t| visit(&t.id, self, &mut Vec::new(), &mut done))
    }

    /// The tasks that depend on `id`.
    pub fn dependents(&self, id: &str) -> Vec<String> {
        self.task
            .iter()
            .filter(|t| t.after.iter().any(|d| d == id))
            .map(|t| t.id.clone())
            .collect()
    }

    /// The limit of `resource` (behaviour 12).
    pub fn limit(&self, resource: &str) -> u32 {
        self.limits.get(resource).copied().unwrap_or(1)
    }

    fn local(&self, now: u64) -> i64 {
        now as i64 + i64::from(self.utc_offset_minutes) * 60
    }

    /// The day of `now` in the workspace's time.
    pub fn day(&self, now: u64) -> i64 {
        self.local(now).div_euclid(DAY)
    }

    /// Whether no run may start at `now` (behaviour 13).
    pub fn quiet(&self, now: u64) -> bool {
        let minute = (self.local(now).rem_euclid(DAY) / 60) as u32;
        self.quiet_hours
            .as_ref()
            .is_some_and(|q| q.contains(minute))
    }

    /// The first time at or after `after` that is `HH:MM` in the workspace's time.
    pub fn next_daily(&self, minute: u32, after: u64) -> u64 {
        let local = self.local(after);
        let day_start = local.div_euclid(DAY) * DAY;
        let mut at = day_start + i64::from(minute) * 60;
        if at < local {
            at += DAY;
        }
        (at - i64::from(self.utc_offset_minutes) * 60).max(0) as u64
    }
}

/// Whether `subject` matches `pattern`: `*` any characters except `/`, `**` any characters,
/// everything else itself (behaviour 6).
pub fn matches_filter(pattern: &str, subject: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p {
            [] => s.is_empty(),
            ['*', '*', rest @ ..] => (0..=s.len()).any(|i| go(rest, &s[i..])),
            ['*', rest @ ..] => {
                let stop = s.iter().position(|c| *c == '/').unwrap_or(s.len());
                (0..=stop).any(|i| go(rest, &s[i..]))
            }
            [c, rest @ ..] => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = subject.chars().collect();
    go(&p, &s)
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The event that queued a run, as handed to the host: untrusted data beside the prompt, never
/// part of it (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UntrustedEvent {
    /// Its kind.
    pub kind: EventKind,
    /// Its subject, cut to [`EVENT_FIELD_MAX_CHARS`].
    pub subject: String,
    /// Its data, cut to [`EVENT_FIELD_MAX_CHARS`]; emptied once the run has ended.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub data: String,
}

impl From<&Event> for UntrustedEvent {
    fn from(e: &Event) -> Self {
        Self {
            kind: e.kind,
            subject: cut(&e.subject, EVENT_FIELD_MAX_CHARS),
            data: cut(&e.data, EVENT_FIELD_MAX_CHARS),
        }
    }
}

/// What the host gets to start a run (behaviours 8, 14, 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// The run's id.
    pub run: String,
    /// The task's id.
    pub task: String,
    /// The task's prompt: the only instruction.
    pub prompt: String,
    /// The rights of the run.
    pub rights: BTreeSet<Grant>,
    /// Its budget in cents.
    pub budget_cents: u64,
    /// Its longest time in minutes.
    pub max_minutes: u64,
    /// The event that queued it, to be shown to the agent as untrusted content.
    pub untrusted: Option<UntrustedEvent>,
}

/// What the host says about a run (behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    /// It runs; the spending so far, if known.
    Running {
        /// Cents spent.
        spent_cents: Option<u64>,
    },
    /// It ended well.
    Succeeded {
        /// Cents spent.
        spent_cents: Option<u64>,
    },
    /// It ended badly.
    Failed {
        /// Why.
        reason: String,
        /// Cents spent.
        spent_cents: Option<u64>,
    },
    /// The host cannot tell.
    Unknown,
}

/// What the agent host offers the scheduler; the M19.5 host implements it.
pub trait TaskHost {
    /// Starts an agent on `request` with exactly its rights and budget.
    fn start(&mut self, request: &RunRequest) -> Result<(), String>;
    /// The status of the run `run`.
    fn status(&mut self, run: &str) -> RunStatus;
    /// Stops the run `run`.
    fn stop(&mut self, run: &str, reason: &str) -> Result<(), String>;
}

/// Where the scheduler's lines go (behaviour 18).
pub trait ScheduleLog {
    /// Writes one line; an error means it was not written.
    fn record(&mut self, line: &Value) -> Result<(), String>;
}

/// The workspace's agent log `.ferris-idea/agent-log.jsonl`.
#[derive(Debug, Clone)]
pub struct AgentLog {
    /// The workspace root.
    pub root: PathBuf,
}

impl ScheduleLog for AgentLog {
    fn record(&mut self, line: &Value) -> Result<(), String> {
        crate::log_agent_action(&self.root, line)
            .map_err(|e| format!("{}: {e}", self.root.join(AGENT_LOG).display()))
    }
}

/// Where a run stands (behaviour 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// Waiting to start.
    Queued,
    /// Started.
    Running,
    /// Ended well.
    Succeeded,
    /// Ended badly or did not start.
    Failed,
    /// Stopped by the scheduler.
    Stopped,
    /// Never started.
    Skipped,
}

impl RunState {
    /// Whether the run has ended.
    pub fn finished(self) -> bool {
        !matches!(self, RunState::Queued | RunState::Running)
    }
}

/// One run of a task (behaviour 19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// `task#n`.
    pub id: String,
    /// The task.
    pub task: String,
    /// Where it stands.
    pub state: RunState,
    /// `once`, `every`, `daily` or `event`.
    pub trigger: String,
    /// The task's priority when queued.
    pub priority: u8,
    /// When it was queued.
    pub queued_at: u64,
    /// When it started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    /// When it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    /// Why it waits, ended or was skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Cents spent, as last reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spent_cents: Option<u64>,
    /// Its budget, fixed at the start.
    #[serde(default)]
    pub budget_cents: u64,
    /// Its longest time, fixed at the start.
    #[serde(default)]
    pub max_minutes: u64,
    /// The resources it occupies, fixed at the start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
    /// Its rights, fixed at the start.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub rights: BTreeSet<Grant>,
    /// The event that queued it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<UntrustedEvent>,
    /// Fires coalesced into it while it was queued.
    #[serde(default)]
    pub coalesced: u32,
}

/// The scheduler's record of one task (behaviour 19).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskState {
    /// When it fires next (time triggers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_due: Option<u64>,
    /// When it last fired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired: Option<u64>,
    /// How its last run ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_outcome: Option<RunState>,
    /// Whether a once-trigger fired.
    #[serde(default)]
    pub fired: bool,
}

/// The scheduler's state of one workspace (behaviour 19).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleState {
    /// Per task.
    #[serde(default)]
    pub tasks: BTreeMap<String, TaskState>,
    /// Queued and running runs.
    #[serde(default)]
    pub active: Vec<Run>,
    /// The last finished runs, oldest first.
    #[serde(default)]
    pub history: Vec<Run>,
    /// The day of `runs_today`.
    #[serde(default)]
    pub day: i64,
    /// Runs started that day.
    #[serde(default)]
    pub runs_today: u32,
    /// The number of the next run.
    #[serde(default)]
    pub next_run: u64,
}

/// What a round did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleEvent {
    /// The task.
    pub task: String,
    /// The run.
    pub run: String,
    /// `queued`, `coalesced`, `started`, `succeeded`, `failed`, `stopped` or `skipped`.
    pub action: &'static str,
    /// Why.
    pub reason: String,
}

/// The scheduler of one workspace.
#[derive(Debug, Clone)]
pub struct Scheduler {
    /// Its configuration.
    pub schedule: Schedule,
    /// The rights the workspace grants agents now (behaviour 16).
    pub grants: BTreeSet<Grant>,
    /// Its state.
    pub state: ScheduleState,
}

impl Scheduler {
    /// A scheduler with `schedule`, the workspace's current `grants` and saved `state`.
    pub fn new(schedule: Schedule, grants: BTreeSet<Grant>, state: ScheduleState) -> Self {
        Self {
            schedule,
            grants,
            state,
        }
    }

    /// One round (behaviour 3): supervises the running runs, fires the due triggers and the
    /// matching `events`, and starts what may start. Returns what it did.
    pub fn tick(
        &mut self,
        host: &mut dyn TaskHost,
        clock: &dyn Clock,
        events: &[Event],
        log: &mut dyn ScheduleLog,
    ) -> Vec<ScheduleEvent> {
        let now = clock.now();
        let mut out = Vec::new();
        let day = self.schedule.day(now);
        if day != self.state.day {
            self.state.day = day;
            self.state.runs_today = 0;
        }
        self.supervise(host, log, now, &mut out);
        self.drop_orphans(log, now, &mut out);
        if self.schedule.enabled {
            self.fire(events, log, now, &mut out);
            self.start(host, log, now, &mut out);
        }
        out
    }

    fn line(&self, run: &Run, action: &str, reason: &str, now: u64) -> Value {
        let mut l = json!({
            "time": now, "tool": "scheduler", "task": run.task, "run": run.id,
            "action": action, "reason": reason, "trigger": run.trigger, "priority": run.priority,
        });
        if let Some(e) = &run.event {
            l["event_kind"] = json!(e.kind.name());
            l["event_subject"] = json!(cut(&e.subject, LOG_SUBJECT_MAX_CHARS));
        }
        if let Some(s) = run.spent_cents {
            l["spent_cents"] = json!(s);
        }
        l
    }

    /// Ends `active[i]` as `state` and moves it to the history (behaviours 17, 18, 19).
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        i: usize,
        state: RunState,
        action: &'static str,
        reason: String,
        log: &mut dyn ScheduleLog,
        now: u64,
        out: &mut Vec<ScheduleEvent>,
    ) {
        let mut run = self.state.active.remove(i);
        run.state = state;
        run.ended_at = Some(now);
        run.reason = Some(reason.clone());
        let _ = log.record(&self.line(&run, action, &reason, now));
        self.archive(run, now, out, action, reason);
    }

    fn archive(
        &mut self,
        mut run: Run,
        now: u64,
        out: &mut Vec<ScheduleEvent>,
        action: &'static str,
        reason: String,
    ) {
        run.ended_at.get_or_insert(now);
        if let Some(e) = &mut run.event {
            e.data.clear();
        }
        self.state
            .tasks
            .entry(run.task.clone())
            .or_default()
            .last_outcome = Some(run.state);
        out.push(ScheduleEvent {
            task: run.task.clone(),
            run: run.id.clone(),
            action,
            reason,
        });
        self.state.history.push(run);
        let over = self.state.history.len().saturating_sub(HISTORY_MAX);
        self.state.history.drain(..over);
    }

    /// Asks the host about every running run and stops it at its budget (behaviours 14, 17).
    fn supervise(
        &mut self,
        host: &mut dyn TaskHost,
        log: &mut dyn ScheduleLog,
        now: u64,
        out: &mut Vec<ScheduleEvent>,
    ) {
        let mut i = 0;
        while i < self.state.active.len() {
            let run = &self.state.active[i];
            if run.state != RunState::Running {
                i += 1;
                continue;
            }
            let id = run.id.clone();
            let gone = match self.schedule.task(&run.task) {
                None => Some("the task was removed"),
                Some(t) if !t.enabled => Some("the task was disabled"),
                Some(_) => None,
            };
            let status = host.status(&id);
            let (spent, ended) = match &status {
                RunStatus::Succeeded { spent_cents } => (
                    *spent_cents,
                    Some((RunState::Succeeded, "succeeded", String::new())),
                ),
                RunStatus::Failed {
                    reason,
                    spent_cents,
                } => (
                    *spent_cents,
                    Some((RunState::Failed, "failed", reason.clone())),
                ),
                RunStatus::Running { spent_cents } => (*spent_cents, None),
                RunStatus::Unknown => (None, None),
            };
            let run = &mut self.state.active[i];
            if spent.is_some() {
                run.spent_cents = spent;
            }
            if let Some((state, action, reason)) = ended {
                let reason = if reason.is_empty() {
                    "the agent finished".to_owned()
                } else {
                    reason
                };
                self.finish(i, state, action, reason, log, now, out);
                continue;
            }
            let started = run.started_at.unwrap_or(now);
            let stop = if let Some(why) = gone {
                Some(why.to_owned())
            } else if now.saturating_sub(started) >= run.max_minutes * 60 {
                Some(format!("its time of {} minutes is up", run.max_minutes))
            } else if status == RunStatus::Unknown {
                Some("the host cannot tell its status, so its spending cannot be checked".into())
            } else {
                match spent {
                    None => {
                        Some("the host reports no spending, so the budget cannot be checked".into())
                    }
                    Some(s) if s >= run.budget_cents => Some(format!(
                        "its budget of {} cents is spent ({s} cents)",
                        run.budget_cents
                    )),
                    Some(_) => None,
                }
            };
            match stop {
                Some(reason) => {
                    let reason = match host.stop(&id, &reason) {
                        Ok(()) => reason,
                        Err(e) => format!("{reason}; stopping failed: {e}"),
                    };
                    self.finish(i, RunState::Stopped, "stopped", reason, log, now, out);
                }
                None => i += 1,
            }
        }
    }

    /// Skips the queued runs of removed or disabled tasks (behaviour 17).
    fn drop_orphans(&mut self, log: &mut dyn ScheduleLog, now: u64, out: &mut Vec<ScheduleEvent>) {
        let mut i = 0;
        while i < self.state.active.len() {
            let run = &self.state.active[i];
            let why = match self.schedule.task(&run.task) {
                _ if run.state != RunState::Queued => None,
                None => Some("the task was removed"),
                Some(t) if !t.enabled => Some("the task was disabled"),
                Some(_) => None,
            };
            match why {
                Some(why) => {
                    self.finish(i, RunState::Skipped, "skipped", why.into(), log, now, out);
                }
                None => i += 1,
            }
        }
    }

    /// Fires the due time triggers and the matching events (behaviours 4 to 6, 9).
    fn fire(
        &mut self,
        events: &[Event],
        log: &mut dyn ScheduleLog,
        now: u64,
        out: &mut Vec<ScheduleEvent>,
    ) {
        let mut tasks: Vec<TaskDef> = self
            .schedule
            .task
            .iter()
            .filter(|t| t.enabled)
            .cloned()
            .collect();
        tasks.sort_by(|a, b| a.id.cmp(&b.id));
        for task in tasks {
            let base = if task.created_at > 0 {
                task.created_at
            } else {
                now
            };
            let ts = self.state.tasks.entry(task.id.clone()).or_default();
            let due = match &task.trigger {
                Trigger::Once { at } => {
                    let due = !ts.fired && now >= *at;
                    if due {
                        ts.fired = true;
                    }
                    due
                }
                Trigger::Every { seconds, from } => {
                    let from = from.unwrap_or(base + seconds);
                    let next = *ts.next_due.get_or_insert(from);
                    if now >= next {
                        let k = (now - from) / seconds + 1;
                        ts.next_due = Some(from + k * seconds);
                        true
                    } else {
                        false
                    }
                }
                Trigger::Daily { at } => {
                    let minute = parse_hhmm(at).unwrap_or(0);
                    let next = match ts.next_due {
                        Some(n) => n,
                        None => self.schedule.next_daily(minute, base),
                    };
                    ts.next_due = Some(next);
                    if now >= next {
                        ts.next_due = Some(self.schedule.next_daily(minute, now + 1));
                        true
                    } else {
                        false
                    }
                }
                Trigger::Event { .. } => false,
            };
            if due {
                self.enqueue(&task, None, log, now, out);
            }
            if let Trigger::Event { kind, filter } = &task.trigger {
                for e in events.iter().filter(|e| {
                    e.kind == *kind
                        && filter
                            .as_deref()
                            .is_none_or(|f| matches_filter(f, &e.subject))
                }) {
                    self.enqueue(&task, Some(e), log, now, out);
                }
            }
        }
    }

    /// Queues one run of `task`, or coalesces into its queued one, or skips at the queue's cap
    /// (behaviour 9).
    fn enqueue(
        &mut self,
        task: &TaskDef,
        event: Option<&Event>,
        log: &mut dyn ScheduleLog,
        now: u64,
        out: &mut Vec<ScheduleEvent>,
    ) {
        self.state
            .tasks
            .entry(task.id.clone())
            .or_default()
            .last_fired = Some(now);
        if let Some(i) = self
            .state
            .active
            .iter()
            .position(|r| r.task == task.id && r.state == RunState::Queued)
        {
            self.state.active[i].coalesced += 1;
            let mut l = self.line(
                &self.state.active[i],
                "coalesced",
                "a run of the task is already queued",
                now,
            );
            if let Some(e) = event {
                l["event_kind"] = json!(e.kind.name());
                l["event_subject"] = json!(cut(&e.subject, LOG_SUBJECT_MAX_CHARS));
            }
            let _ = log.record(&l);
            out.push(ScheduleEvent {
                task: task.id.clone(),
                run: self.state.active[i].id.clone(),
                action: "coalesced",
                reason: "a run of the task is already queued".into(),
            });
            return;
        }
        self.state.next_run += 1;
        let mut run = Run {
            id: format!("{}#{}", task.id, self.state.next_run),
            task: task.id.clone(),
            state: RunState::Queued,
            trigger: task.trigger.name().to_owned(),
            priority: task.priority,
            queued_at: now,
            started_at: None,
            ended_at: None,
            reason: None,
            spent_cents: None,
            budget_cents: task.budget_cents,
            max_minutes: task.max_minutes,
            resources: task.resources.clone(),
            rights: BTreeSet::new(),
            event: event.map(UntrustedEvent::from),
            coalesced: 0,
        };
        let queued = self
            .state
            .active
            .iter()
            .filter(|r| r.state == RunState::Queued)
            .count();
        if queued >= self.schedule.max_queue as usize {
            run.state = RunState::Skipped;
            let reason = format!("the queue is full ({} runs)", self.schedule.max_queue);
            run.reason = Some(reason.clone());
            let _ = log.record(&self.line(&run, "skipped", &reason, now));
            self.archive(run, now, out, "skipped", reason);
            return;
        }
        let _ = log.record(&self.line(&run, "queued", "its trigger fired", now));
        out.push(ScheduleEvent {
            task: task.id.clone(),
            run: run.id.clone(),
            action: "queued",
            reason: "its trigger fired".into(),
        });
        self.state.active.push(run);
    }

    /// Why the queued run `run` cannot start now, or `Ok` (behaviours 11, 12).
    fn blocked(&self, run: &Run, task: &TaskDef) -> Result<(), Block> {
        for d in &task.after {
            if self.state.active.iter().any(|r| &r.task == d) {
                return Err(Block::Wait(format!("waits for a run of `{d}`")));
            }
            match self.state.tasks.get(d).and_then(|s| s.last_outcome) {
                Some(RunState::Succeeded) => {}
                Some(other) => {
                    return Err(Block::Skip(format!(
                        "its dependency `{d}` last ended {}",
                        serde_json::to_value(other)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_owned))
                            .unwrap_or_default()
                    )));
                }
                None => {
                    return Err(Block::Skip(format!("its dependency `{d}` never ran")));
                }
            }
        }
        let running: Vec<&Run> = self
            .state
            .active
            .iter()
            .filter(|r| r.state == RunState::Running)
            .collect();
        if running.len() >= self.schedule.max_parallel as usize {
            return Err(Block::Wait(format!(
                "{} runs at once, the limit",
                self.schedule.max_parallel
            )));
        }
        for res in &run.resources {
            let used = running.iter().filter(|r| r.resources.contains(res)).count();
            let limit = self.schedule.limit(res);
            if used >= limit as usize {
                return Err(Block::Wait(format!(
                    "the resource `{res}` is in use ({used} of {limit})"
                )));
            }
        }
        if self.state.runs_today >= self.schedule.max_runs_per_day {
            return Err(Block::Skip(format!(
                "{} runs started today, the limit",
                self.schedule.max_runs_per_day
            )));
        }
        Ok(())
    }

    /// Starts the queued runs that may start (behaviours 10 to 16, 18).
    fn start(
        &mut self,
        host: &mut dyn TaskHost,
        log: &mut dyn ScheduleLog,
        now: u64,
        out: &mut Vec<ScheduleEvent>,
    ) {
        let quiet = self.schedule.quiet(now);
        let mut order: Vec<(u8, u64, String, String)> = self
            .state
            .active
            .iter()
            .filter(|r| r.state == RunState::Queued)
            .map(|r| (r.priority, r.queued_at, r.task.clone(), r.id.clone()))
            .collect();
        order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        for (_, _, task_id, run_id) in order {
            let Some(i) = self.state.active.iter().position(|r| r.id == run_id) else {
                continue;
            };
            let Some(task) = self.schedule.task(&task_id).cloned() else {
                continue;
            };
            if quiet {
                self.state.active[i].reason = Some("quiet hours".into());
                continue;
            }
            match self.blocked(&self.state.active[i], &task) {
                Err(Block::Wait(why)) => {
                    self.state.active[i].reason = Some(why);
                    continue;
                }
                Err(Block::Skip(why)) => {
                    self.finish(i, RunState::Skipped, "skipped", why, log, now, out);
                    continue;
                }
                Ok(()) => {}
            }
            let rights: BTreeSet<Grant> = task.rights.intersection(&self.grants).copied().collect();
            let run = &self.state.active[i];
            let request = RunRequest {
                run: run.id.clone(),
                task: task.id.clone(),
                prompt: task.prompt.clone(),
                rights: rights.clone(),
                budget_cents: task.budget_cents,
                max_minutes: task.max_minutes,
                untrusted: run.event.clone(),
            };
            let mut l = self.line(run, "started", "it may start", now);
            l["rights"] = json!(rights.iter().map(|g| g.name()).collect::<Vec<_>>());
            l["budget_cents"] = json!(task.budget_cents);
            l["max_minutes"] = json!(task.max_minutes);
            l["prompt_chars"] = json!(task.prompt.chars().count());
            let dropped: Vec<&str> = task.rights.difference(&rights).map(|g| g.name()).collect();
            if !dropped.is_empty() {
                l["rights_no_longer_granted"] = json!(dropped);
            }
            if let Err(e) = log.record(&l) {
                // Nothing starts unlogged (behaviour 18); the run stays queued.
                self.state.active[i].reason =
                    Some(format!("the agent log cannot be written ({e})"));
                return;
            }
            let run = &mut self.state.active[i];
            run.budget_cents = task.budget_cents;
            run.max_minutes = task.max_minutes;
            run.resources = task.resources.clone();
            run.rights = rights;
            match host.start(&request) {
                Ok(()) => {
                    run.state = RunState::Running;
                    run.started_at = Some(now);
                    run.reason = None;
                    self.state.runs_today += 1;
                    out.push(ScheduleEvent {
                        task: task.id.clone(),
                        run: run_id,
                        action: "started",
                        reason: "it may start".into(),
                    });
                }
                Err(e) => {
                    self.state.runs_today += 1;
                    self.finish(
                        i,
                        RunState::Failed,
                        "failed",
                        format!("the host could not start it: {e}"),
                        log,
                        now,
                        out,
                    );
                }
            }
        }
    }
}

enum Block {
    Wait(String),
    Skip(String),
}

/// A source of `file_changed` events: size and modification time of the workspace's files
/// compared with the last poll (behaviour 7).
#[derive(Debug, Clone)]
pub struct FileChangeSource {
    root: PathBuf,
    seen: Option<BTreeMap<String, (u64, u128)>>,
}

impl FileChangeSource {
    /// A source for the workspace `root`; its first poll takes the baseline.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            seen: None,
        }
    }

    fn scan(&self) -> BTreeMap<String, (u64, u128)> {
        let mut out = BTreeMap::new();
        let mut dirs = vec![self.root.clone()];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                if out.len() >= FILE_SOURCE_MAX_FILES {
                    return out;
                }
                // Links are not followed, so nothing outside the workspace is looked at.
                let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                    continue;
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if meta.is_dir() {
                    if !FILE_SOURCE_SKIPPED.contains(&name.as_str()) {
                        dirs.push(entry.path());
                    }
                    continue;
                }
                let Ok(rel) = entry.path().strip_prefix(&self.root).map(Path::to_path_buf) else {
                    continue;
                };
                let rel = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                out.insert(rel, (meta.len(), modified));
            }
        }
        out
    }
}

impl EventSource for FileChangeSource {
    fn poll(&mut self) -> Vec<Event> {
        let now = self.scan();
        let Some(before) = self.seen.replace(now.clone()) else {
            return Vec::new();
        };
        let changed = now
            .iter()
            .filter(|(p, v)| before.get(*p) != Some(v))
            .map(|(p, _)| p.clone());
        let removed = before.keys().filter(|p| !now.contains_key(*p)).cloned();
        let mut paths: Vec<String> = changed.chain(removed).collect();
        paths.sort();
        paths
            .into_iter()
            .map(|subject| Event {
                kind: EventKind::FileChanged,
                subject,
                data: String::new(),
            })
            .collect()
    }
}

/// The configuration and state files beside `grant_file` (behaviours 19 and 20): its name with
/// `agent-permissions.toml` replaced, so a test's grant file `x.agent-permissions.toml` has
/// `x.agent-schedule.toml` beside it.
pub fn schedule_files(grant_file: &Path) -> (PathBuf, PathBuf) {
    let name = grant_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = name.strip_suffix(GRANT_FILE).unwrap_or("");
    (
        grant_file.with_file_name(format!("{prefix}{SCHEDULE_FILE}")),
        grant_file.with_file_name(format!("{prefix}{SCHEDULE_STATE_FILE}")),
    )
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    workspace: Vec<toml::Table>,
}

fn split_table(table: &toml::Table) -> Result<(PathBuf, Schedule), String> {
    let mut rest = table.clone();
    let path = match rest.remove("path") {
        Some(toml::Value::String(p)) => PathBuf::from(p),
        _ => return Err("a [[workspace]] table needs `path`".into()),
    };
    let schedule: Schedule = toml::Value::Table(rest)
        .try_into()
        .map_err(|e: toml::de::Error| format!("{}: {}", path.display(), e.message()))?;
    schedule
        .check()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((path, schedule))
}

/// The schedule of a workspace as read, with where it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LoadedSchedule {
    /// The schedule; the default (off, no tasks) when there is none or it has a problem.
    pub schedule: Schedule,
    /// The file consulted.
    pub file: Option<PathBuf>,
    /// Problem with the file; the scheduler stays off then.
    pub problem: Option<String>,
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn read_config_file(file: &Path, root: &Path) -> Result<ConfigFile, String> {
    if inside_workspace(file, root) {
        return Err("the schedule lies inside the workspace and is ignored".into());
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

/// Reads the schedule of `root` from `file` (behaviour 20).
pub fn load_schedule(file: &Path, root: &Path) -> LoadedSchedule {
    let mut out = LoadedSchedule {
        file: Some(file.to_path_buf()),
        ..LoadedSchedule::default()
    };
    match read_config_file(file, root) {
        Ok(parsed) => {
            let root = canon(root);
            if let Some((_, schedule)) = parsed
                .workspace
                .iter()
                .filter_map(|t| split_table(t).ok())
                .find(|(path, _)| canon(path) == root)
            {
                out.schedule = schedule;
            }
        }
        Err(e) => out.problem = Some(e),
    }
    out
}

/// Writes `schedule` as the schedule of `root` into `file`, keeping the other workspaces' tables.
/// Refused for a file inside the workspace, one with a problem, or a schedule out of bounds.
pub fn save_schedule(file: &Path, root: &Path, schedule: &Schedule) -> Result<(), String> {
    schedule.check()?;
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
    let toml::Value::Table(values) = toml::Value::try_from(schedule).map_err(|e| e.to_string())?
    else {
        return Err("the schedule is no table".into());
    };
    table.extend(values);
    parsed.workspace.push(table);
    let text = toml::to_string(&parsed).map_err(|e| e.to_string())?;
    crate::write_atomic(file, text).map_err(|e| format!("{}: {e}", file.display()))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    workspaces: BTreeMap<String, ScheduleState>,
}

fn read_state_file(file: &Path) -> Result<StateFile, String> {
    match std::fs::read_to_string(file) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StateFile::default()),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

/// The saved state of `root`; empty with the problem when the file cannot be read (behaviour 19).
pub fn load_schedule_state(file: &Path, root: &Path) -> (ScheduleState, Option<String>) {
    match read_state_file(file) {
        Ok(mut s) => (
            s.workspaces
                .remove(&canon(root).to_string_lossy().into_owned())
                .unwrap_or_default(),
            None,
        ),
        Err(e) => (ScheduleState::default(), Some(e)),
    }
}

/// Saves the state of `root` into `file`, keeping other workspaces' states; a file that cannot be
/// read is replaced (behaviour 19).
pub fn save_schedule_state(file: &Path, root: &Path, state: &ScheduleState) -> Result<(), String> {
    let mut s = read_state_file(file).unwrap_or_default();
    s.workspaces
        .insert(canon(root).to_string_lossy().into_owned(), state.clone());
    let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    crate::write_atomic(file, text).map_err(|e| format!("{}: {e}", file.display()))
}
