//! Agents conduct agents and exchange messages (`SPEC-0349`, ROADMAP M19.6, ADR-0023 rules 10 and
//! 12, ADR-0014 rules 4 and 5): who started which hosted agent and at what depth, the limits on
//! delegation, and the per-workspace message store.
//!
//! Security: a caller is an `mcp:` identity that can never equal a hosted agent's id
//! (behaviour 1); only an agent's conductor steers it (behaviour 6); depth and agents per conductor
//! are bounded and checked under one lock with the start (behaviours 3 and 4). The message store
//! lives in the user's settings folder (0700 folder, 0600 file), is append-only with retention,
//! and refuses texts with secrets (behaviours 11–14); messages are data, never instructions.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent_host::AgentSnapshot;
use crate::permissions::{GRANT_FILE, inside_workspace};

/// Bounds of `max_depth` (behaviour 8).
pub const MAX_DEPTH_BOUNDS: [u32; 2] = [0, 3];
/// Default `max_depth`: clients start agents, agents start none.
pub const MAX_DEPTH_DEFAULT: u32 = 1;
/// Bounds of `max_per_conductor` (behaviour 8).
pub const MAX_PER_CONDUCTOR_BOUNDS: [u32; 2] = [1, 8];
/// Default `max_per_conductor`.
pub const MAX_PER_CONDUCTOR_DEFAULT: u32 = 2;
/// Longest wait of `agent_wait` in seconds (behaviour 10).
pub const WAIT_MAX_SECONDS: u64 = 120;
/// Default wait of `agent_wait` in seconds.
pub const WAIT_DEFAULT_SECONDS: u64 = 30;
/// The folder of message stores beside the grant file (behaviour 11).
pub const MESSAGES_DIR: &str = "agent-messages";
/// Longest message text in characters (behaviour 12).
pub const MESSAGE_CHARS: usize = 4_000;
/// Messages kept per store (behaviour 14).
pub const MESSAGES_KEPT: usize = 1_000;
/// Days a message is kept (behaviour 14).
pub const MESSAGE_DAYS: u64 = 30;
/// Messages one read answers at most (behaviour 15).
pub const READ_LIMIT_MAX: usize = 200;
/// The recipient that stands for the whole project.
pub const ALL: &str = "all";
/// The prefix of a client's identity (behaviour 1).
pub const CLIENT_PREFIX: &str = "mcp:";

// ---------------------------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------------------------

/// The `[control]` table of `agent-host.toml` (behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlLimits {
    /// How deep agents may start agents: a client is at depth 0, the agents it starts at 1.
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    /// Running agents one caller may conduct at once.
    #[serde(default = "default_max_per_conductor")]
    pub max_per_conductor: u32,
}

fn default_max_depth() -> u32 {
    MAX_DEPTH_DEFAULT
}

fn default_max_per_conductor() -> u32 {
    MAX_PER_CONDUCTOR_DEFAULT
}

impl Default for ControlLimits {
    fn default() -> Self {
        ControlLimits {
            max_depth: MAX_DEPTH_DEFAULT,
            max_per_conductor: MAX_PER_CONDUCTOR_DEFAULT,
        }
    }
}

impl ControlLimits {
    /// Checks the bounds of behaviour 8.
    pub fn check(&self) -> Result<(), String> {
        let [lo, hi] = MAX_DEPTH_BOUNDS;
        if !(lo..=hi).contains(&self.max_depth) {
            return Err(format!("control.max_depth must be {lo} to {hi}"));
        }
        let [lo, hi] = MAX_PER_CONDUCTOR_BOUNDS;
        if !(lo..=hi).contains(&self.max_per_conductor) {
            return Err(format!("control.max_per_conductor must be {lo} to {hi}"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------------------------

/// The caller identity of an MCP client named `name` at `initialize` (behaviour 1): `mcp:` and the
/// name in lower case, other characters than a-z, 0-9, `.`, `-`, `_` as `-`, at most 64.
pub fn client_identity(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() {
        format!("{CLIENT_PREFIX}unnamed")
    } else {
        format!("{CLIENT_PREFIX}{cleaned}")
    }
}

/// Whether `id` can be a hosted agent's id (`<definition>-<n>`).
fn hosted_like(id: &str) -> bool {
    (1..=80).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Whether `id` is a client identity as [`client_identity`] writes it.
fn client_like(id: &str) -> bool {
    id.strip_prefix(CLIENT_PREFIX).is_some_and(|rest| {
        (1..=64).contains(&rest.len())
            && rest.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_')
            })
    })
}

// ---------------------------------------------------------------------------------------------
// Conductors
// ---------------------------------------------------------------------------------------------

/// Who started a hosted agent, and how deep it is (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Origin {
    /// The caller that started it.
    pub conductor: String,
    /// One more than its conductor's depth.
    pub depth: u32,
}

/// The conductors of the hosted agents of one workspace.
#[derive(Debug, Default)]
pub struct Conductors {
    origins: Mutex<HashMap<String, Origin>>,
    /// Held across the checks and the start (behaviour 4).
    starting: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Conductors {
    /// No agents conducted yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The lock to hold from [`Conductors::check_start`] to [`Conductors::record`].
    pub fn start_lock(&self) -> MutexGuard<'_, ()> {
        lock(&self.starting)
    }

    /// The origin of hosted agent `id`, if it was started through these records.
    pub fn origin(&self, id: &str) -> Option<Origin> {
        lock(&self.origins).get(id).cloned()
    }

    /// The depth of `caller`: a hosted agent's own, a client's 0.
    pub fn depth_of(&self, caller: &str) -> u32 {
        lock(&self.origins).get(caller).map_or(0, |o| o.depth)
    }

    /// Checks a start by `caller` (behaviours 3 and 4) against `limits`, given every hosted agent
    /// as it is; answers the depth of the agent to start.
    pub fn check_start(
        &self,
        caller: &str,
        limits: &ControlLimits,
        hosted: &[AgentSnapshot],
    ) -> Result<u32, String> {
        let depth = self.depth_of(caller) + 1;
        if depth > limits.max_depth {
            return Err(format!(
                "refused: `{caller}` is at depth {} and may not start an agent: control.max_depth is {} in agent-host.toml (SPEC-0349 behaviour 3)",
                depth - 1,
                limits.max_depth
            ));
        }
        let origins = lock(&self.origins);
        let running = hosted
            .iter()
            .filter(|s| !s.exited)
            .filter(|s| origins.get(&s.id).is_some_and(|o| o.conductor == caller))
            .count();
        if running >= limits.max_per_conductor as usize {
            return Err(format!(
                "refused: `{caller}` conducts {running} running agent(s) already, the most control.max_per_conductor allows; stop one first (SPEC-0349 behaviour 4)"
            ));
        }
        Ok(depth)
    }

    /// Records that `caller` started hosted agent `id` at `depth`.
    pub fn record(&self, id: &str, caller: &str, depth: u32) {
        lock(&self.origins).insert(
            id.to_owned(),
            Origin {
                conductor: caller.to_owned(),
                depth,
            },
        );
    }

    /// Whether `caller` conducts hosted agent `id`.
    pub fn conducts(&self, caller: &str, id: &str) -> bool {
        self.origin(id).is_some_and(|o| o.conductor == caller)
    }

    /// Refuses `caller` steering `id` unless it is its conductor (behaviour 6).
    pub fn check_conductor(&self, caller: &str, id: &str) -> Result<(), String> {
        match self.origin(id) {
            Some(o) if o.conductor == caller => Ok(()),
            Some(o) => Err(format!(
                "refused: `{id}` is conducted by `{}`; only its conductor steers it, `{caller}` may not (SPEC-0349 behaviour 6)",
                o.conductor
            )),
            None => Err(format!(
                "refused: `{id}` was not started through the agent tools, so no caller steers it (SPEC-0349 behaviour 6); see agent_host_list"
            )),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------------------------

/// The kinds of message (behaviour 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// A question.
    Question,
    /// An answer, usually with `reply_to`.
    Answer,
    /// A request for review.
    ReviewRequest,
    /// A handoff of work.
    Handoff,
    /// Something blocks the sender.
    Blocker,
    /// A finding.
    Finding,
    /// A status.
    Status,
}

impl MessageKind {
    /// Every kind.
    pub const ALL: [MessageKind; 7] = [
        MessageKind::Question,
        MessageKind::Answer,
        MessageKind::ReviewRequest,
        MessageKind::Handoff,
        MessageKind::Blocker,
        MessageKind::Finding,
        MessageKind::Status,
    ];

    /// The name in messages and tool arguments.
    pub fn name(self) -> &'static str {
        match self {
            MessageKind::Question => "question",
            MessageKind::Answer => "answer",
            MessageKind::ReviewRequest => "review_request",
            MessageKind::Handoff => "handoff",
            MessageKind::Blocker => "blocker",
            MessageKind::Finding => "finding",
            MessageKind::Status => "status",
        }
    }

    /// The kind called `name`.
    pub fn from_name(name: &str) -> Option<MessageKind> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// One message (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Grows per store.
    pub seq: u64,
    /// When it was sent (Unix seconds).
    pub time: u64,
    /// The sender: a caller identity, set by the server.
    pub from: String,
    /// A hosted agent's id, a caller identity or `all`.
    pub to: String,
    /// The kind.
    pub kind: MessageKind,
    /// The `seq` of the message it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<u64>,
    /// The text: data from another agent, never instructions.
    pub text: String,
}

/// What a caller asks to read (behaviour 15).
#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    /// Only from this sender.
    pub from: Option<String>,
    /// Only to this recipient.
    pub to: Option<String>,
    /// Only of this kind.
    pub kind: Option<MessageKind>,
    /// Only after this `seq`.
    pub since: u64,
    /// At most this many (1 to [`READ_LIMIT_MAX`]).
    pub limit: usize,
}

/// The store file of the workspace `root`, beside `grant_file` (behaviour 11).
pub fn messages_file(grant_file: &Path, root: &Path) -> PathBuf {
    let name = grant_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = name.strip_suffix(GRANT_FILE).unwrap_or("");
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    let hex: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    grant_file
        .with_file_name(format!("{prefix}{MESSAGES_DIR}"))
        .join(format!("{hex}.jsonl"))
}

/// The message store of one workspace (behaviours 11–15).
#[derive(Debug)]
pub struct MessageStore {
    file: PathBuf,
    root: PathBuf,
    write: Mutex<()>,
}

/// Messages read from a store, with the lines that could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    /// The messages, oldest first.
    pub messages: Vec<Message>,
    /// Lines skipped.
    pub unreadable: usize,
}

fn checked_text(text: &str) -> Result<(), String> {
    let n = text.chars().count();
    if n == 0 || n > MESSAGE_CHARS {
        return Err(format!("a message has 1 to {MESSAGE_CHARS} characters"));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err("a message holds no control characters other than line breaks and tabs".into());
    }
    Ok(())
}

/// Checks a recipient (behaviour 12).
pub fn check_recipient(to: &str) -> Result<(), String> {
    if to == ALL || client_like(to) || hosted_like(to) {
        Ok(())
    } else {
        Err(format!(
            "`{to}` is no recipient: a hosted agent's id, a caller (`mcp:<name>`) or `all`"
        ))
    }
}

impl MessageStore {
    /// The store in `file` for the workspace `root`.
    pub fn new(file: PathBuf, root: &Path) -> Self {
        MessageStore {
            file,
            root: root.to_path_buf(),
            write: Mutex::new(()),
        }
    }

    /// The store's file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    fn usable(&self) -> Result<(), String> {
        if inside_workspace(&self.file, &self.root) {
            return Err("the message store would lie inside the workspace; it is never used there (SPEC-0349 behaviour 11)".into());
        }
        Ok(())
    }

    /// Every message kept, oldest first.
    pub fn load(&self) -> Result<Loaded, String> {
        self.usable()?;
        let text = match std::fs::read_to_string(&self.file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Loaded::default()),
            Err(e) => return Err(format!("{}: {e}", self.file.display())),
        };
        let mut out = Loaded::default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match serde_json::from_str::<Message>(line) {
                Ok(m) => out.messages.push(m),
                Err(_) => out.unreadable += 1,
            }
        }
        Ok(out)
    }

    /// Adds a message from `from` at `now` (behaviours 12–14). `secrets` counts what the secret
    /// masking would mask in a text; a text with any is refused and nothing is stored.
    #[allow(clippy::too_many_arguments)]
    pub fn send(
        &self,
        from: &str,
        to: &str,
        kind: MessageKind,
        reply_to: Option<u64>,
        text: &str,
        now: u64,
        secrets: impl Fn(&str) -> usize,
    ) -> Result<Message, String> {
        self.usable()?;
        check_recipient(to)?;
        checked_text(text)?;
        let found = secrets(text);
        if found > 0 {
            return Err(format!(
                "refused: the message holds {found} secret(s) (a key, token, password or private key); messages never carry credentials, nothing was stored (SPEC-0349 behaviour 13)"
            ));
        }
        let _write = lock(&self.write);
        let loaded = self.load()?;
        if let Some(r) = reply_to
            && !loaded.messages.iter().any(|m| m.seq == r)
        {
            return Err(format!("reply_to: there is no message {r} in the store"));
        }
        let message = Message {
            seq: loaded.messages.last().map_or(0, |m| m.seq) + 1,
            time: now,
            from: from.to_owned(),
            to: to.to_owned(),
            kind,
            reply_to,
            text: text.to_owned(),
        };
        let cutoff = now.saturating_sub(MESSAGE_DAYS * 24 * 3600);
        let mut kept: Vec<&Message> = loaded
            .messages
            .iter()
            .filter(|m| m.time >= cutoff)
            .collect();
        let over = (kept.len() + 1).saturating_sub(MESSAGES_KEPT);
        kept.drain(..over);
        let dir = self
            .file
            .parent()
            .ok_or_else(|| "the store has no folder".to_owned())?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        private(dir, 0o700)?;
        let line = |m: &Message| -> Result<String, String> {
            let mut s = serde_json::to_string(m).map_err(|e| e.to_string())?;
            s.push('\n');
            Ok(s)
        };
        if kept.len() == loaded.messages.len() && loaded.unreadable == 0 {
            // Append-only: the one write of a new line.
            let mut options = std::fs::OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let text = line(&message)?;
            options
                .open(&self.file)
                .and_then(|mut f| f.write_all(text.as_bytes()))
                .map_err(|e| format!("{}: {e}", self.file.display()))?;
        } else {
            // Retention dropped messages (or unreadable lines): rewrite the newest at once.
            let mut text = String::new();
            for m in kept.iter().copied().chain(std::iter::once(&message)) {
                text.push_str(&line(m)?);
            }
            crate::lifecycle::write_atomic(&self.file, text.as_bytes())
                .map_err(|e| format!("{}: {e}", self.file.display()))?;
        }
        private(&self.file, 0o600)?;
        Ok(message)
    }

    /// The messages `caller` may see (behaviour 15): to it, to `all`, from it, or to or from an
    /// agent it conducts (`conducts`), narrowed by `filter`; answers them oldest first, whether
    /// there are more, and the lines skipped.
    pub fn read(
        &self,
        caller: &str,
        conducts: impl Fn(&str) -> bool,
        filter: &MessageFilter,
    ) -> Result<(Vec<Message>, bool, usize), String> {
        let loaded = self.load()?;
        let visible = |m: &Message| {
            m.to == caller
                || m.to == ALL
                || m.from == caller
                || conducts(&m.to)
                || conducts(&m.from)
        };
        let matching: Vec<Message> = loaded
            .messages
            .into_iter()
            .filter(|m| m.seq > filter.since)
            .filter(visible)
            .filter(|m| filter.from.as_ref().is_none_or(|f| &m.from == f))
            .filter(|m| filter.to.as_ref().is_none_or(|t| &m.to == t))
            .filter(|m| filter.kind.is_none_or(|k| m.kind == k))
            .collect();
        let limit = filter.limit.clamp(1, READ_LIMIT_MAX);
        let more = matching.len() > limit;
        Ok((
            matching.into_iter().take(limit).collect(),
            more,
            loaded.unreadable,
        ))
    }
}

/// Sets `mode` on `path` (Unix).
fn private(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_host::{AgentState, HostConfig, load_host_config};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-agent-control-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("workspace")).unwrap();
        dir.canonicalize().unwrap()
    }

    fn snapshot(id: &str, exited: bool) -> AgentSnapshot {
        AgentSnapshot {
            id: id.into(),
            agent: "fake".into(),
            state: if exited {
                AgentState::Done
            } else {
                AgentState::Idle
            },
            detail: String::new(),
            state_since: 0,
            started_at: 0,
            pid: 1,
            session_ready: true,
            protocol_version: Some(1),
            turns: 0,
            progress: 0,
            last_stop_reason: None,
            pending_approvals: Vec::new(),
            exit_code: None,
            exited,
            output_seq: 0,
            notices: Vec::new(),
        }
    }

    fn no_secrets(_: &str) -> usize {
        0
    }

    /// SPEC-0349 AK-1: identities, depth, the cap per conductor and ownership.
    #[test]
    fn depth_cap_and_ownership_bound_conducting() {
        assert_eq!(client_identity("Claude Code"), "mcp:claude-code");
        assert_eq!(client_identity(""), "mcp:unnamed");
        assert_eq!(client_identity("fake-1"), "mcp:fake-1");
        // A client's identity never equals a hosted agent's id.
        assert!(client_identity("fake-1").contains(':'));
        assert!(client_identity(&"x".repeat(200)).len() <= 68);

        let c = Conductors::new();
        let limits = ControlLimits::default();
        assert_eq!(c.check_start("mcp:a", &limits, &[]), Ok(1));
        c.record("fake-1", "mcp:a", 1);
        // The hosted agent at depth 1 may not start one with max_depth 1.
        let deep = c.check_start("fake-1", &limits, &[]).unwrap_err();
        assert!(deep.contains("max_depth"), "{deep}");
        let deeper = ControlLimits {
            max_depth: 2,
            ..limits
        };
        assert_eq!(c.check_start("fake-1", &deeper, &[]), Ok(2));
        // max_depth 0: nobody starts agents.
        let none = ControlLimits {
            max_depth: 0,
            ..limits
        };
        assert!(c.check_start("mcp:a", &none, &[]).is_err());
        // Two running agents of `mcp:a` reach the cap of 2; an ended one does not count.
        c.record("fake-2", "mcp:a", 1);
        let hosted = [snapshot("fake-1", false), snapshot("fake-2", false)];
        let cap = c.check_start("mcp:a", &limits, &hosted).unwrap_err();
        assert!(cap.contains("max_per_conductor"), "{cap}");
        assert_eq!(c.check_start("mcp:b", &limits, &hosted), Ok(1));
        let hosted = [snapshot("fake-1", false), snapshot("fake-2", true)];
        assert_eq!(c.check_start("mcp:a", &limits, &hosted), Ok(1));
        // Only the conductor steers.
        assert!(c.check_conductor("mcp:a", "fake-1").is_ok());
        assert!(c.conducts("mcp:a", "fake-1"));
        let other = c.check_conductor("mcp:b", "fake-1").unwrap_err();
        assert!(other.contains("`mcp:a`"), "{other}");
        assert!(c.check_conductor("mcp:a", "fake-9").is_err());
    }

    /// SPEC-0349 AK-3: the `[control]` table of `agent-host.toml`.
    #[test]
    fn control_limits_have_defaults_and_bounds() {
        let dir = temp("limits");
        let root = dir.join("workspace");
        let file = dir.join("agent-host.toml");
        std::fs::write(&file, "max_agents = 2\n").unwrap();
        let loaded = load_host_config(&file, &root);
        assert_eq!(loaded.problem, None);
        assert_eq!(loaded.config.control, ControlLimits::default());
        assert_eq!(loaded.config.control.max_depth, 1);
        assert_eq!(loaded.config.control.max_per_conductor, 2);
        std::fs::write(&file, "[control]\nmax_depth = 2\nmax_per_conductor = 4\n").unwrap();
        let loaded = load_host_config(&file, &root);
        assert_eq!(loaded.config.control.max_depth, 2);
        assert_eq!(loaded.config.control.max_per_conductor, 4);
        for bad in [
            "[control]\nmax_depth = 4\n",
            "[control]\nmax_per_conductor = 0\n",
            "[control]\nmax_per_conductor = 9\n",
            "[control]\nbudget = 1\n",
        ] {
            std::fs::write(&file, bad).unwrap();
            let loaded = load_host_config(&file, &root);
            assert!(loaded.problem.is_some(), "{bad}");
            assert_eq!(loaded.config, HostConfig::default());
        }
    }

    /// SPEC-0349 AK-2: the store beside the grant file, private, append-only, without secrets,
    /// visible to the right callers.
    #[test]
    fn messages_are_stored_privately_and_shown_to_their_callers() {
        let dir = temp("store");
        let root = dir.join("workspace");
        let file = messages_file(&dir.join("agent-permissions.toml"), &root);
        assert_eq!(file.parent().unwrap(), dir.join(MESSAGES_DIR));
        assert_eq!(
            file,
            messages_file(&dir.join("agent-permissions.toml"), &root)
        );
        let store = MessageStore::new(file.clone(), &root);
        let q = store
            .send(
                "mcp:a",
                "mcp:b",
                MessageKind::Question,
                None,
                "Which branch?",
                100,
                no_secrets,
            )
            .unwrap();
        assert_eq!(q.seq, 1);
        let a = store
            .send(
                "mcp:b",
                "mcp:a",
                MessageKind::Answer,
                Some(1),
                "main",
                101,
                no_secrets,
            )
            .unwrap();
        assert_eq!(a.seq, 2);
        store
            .send(
                "mcp:c",
                ALL,
                MessageKind::Status,
                None,
                "tests green",
                102,
                no_secrets,
            )
            .unwrap();
        store
            .send(
                "mcp:c",
                "fake-1",
                MessageKind::Handoff,
                None,
                "take over",
                103,
                no_secrets,
            )
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&file), 0o600);
            assert_eq!(mode(file.parent().unwrap()), 0o700);
        }
        // Refusals store nothing.
        let secret = store
            .send(
                "mcp:a",
                "mcp:b",
                MessageKind::Finding,
                None,
                "the key",
                104,
                |_| 1,
            )
            .unwrap_err();
        assert!(secret.contains("nothing was stored"), "{secret}");
        for (to, text, reply) in [
            ("Bad Recipient", "x", None),
            ("mcp:", "x", None),
            ("mcp:b", "", None),
            ("mcp:b", "a\u{7}b", None),
            ("mcp:b", &"x".repeat(MESSAGE_CHARS + 1) as &str, None),
            ("mcp:b", "x", Some(99)),
        ] {
            assert!(
                store
                    .send(
                        "mcp:a",
                        to,
                        MessageKind::Status,
                        reply,
                        text,
                        105,
                        no_secrets
                    )
                    .is_err(),
                "{to} {text}"
            );
        }
        assert_eq!(store.load().unwrap().messages.len(), 4);
        assert!(MessageKind::from_name("handoff").is_some());
        assert!(MessageKind::from_name("order").is_none());

        let all = MessageFilter {
            limit: 50,
            ..MessageFilter::default()
        };
        let seqs = |caller: &str, conducts: &dyn Fn(&str) -> bool, f: &MessageFilter| {
            store
                .read(caller, conducts, f)
                .unwrap()
                .0
                .iter()
                .map(|m| m.seq)
                .collect::<Vec<_>>()
        };
        assert_eq!(seqs("mcp:a", &|_| false, &all), [1, 2, 3]);
        assert_eq!(seqs("mcp:d", &|_| false, &all), [3]);
        // The conductor of fake-1 sees the handoff to it.
        assert_eq!(seqs("mcp:d", &|id| id == "fake-1", &all), [3, 4]);
        let narrowed = MessageFilter {
            from: Some("mcp:b".into()),
            since: 1,
            limit: 50,
            ..MessageFilter::default()
        };
        assert_eq!(seqs("mcp:a", &|_| false, &narrowed), [2]);
        let one = MessageFilter {
            limit: 1,
            ..MessageFilter::default()
        };
        let (got, more, _) = store.read("mcp:a", |_| false, &one).unwrap();
        assert_eq!((got.len(), more), (1, true));
        // A store inside the workspace is never used.
        let inside = MessageStore::new(root.join("m.jsonl"), &root);
        assert!(inside.load().is_err());
    }

    /// SPEC-0349 AK-2: retention keeps the newest messages and drops old ones; unreadable lines
    /// are skipped.
    #[test]
    fn retention_keeps_the_newest_and_drops_the_old() {
        let dir = temp("retention");
        let root = dir.join("workspace");
        let store = MessageStore::new(
            messages_file(&dir.join("agent-permissions.toml"), &root),
            &root,
        );
        let day = 24 * 3600;
        store
            .send(
                "mcp:a",
                ALL,
                MessageKind::Status,
                None,
                "old",
                0,
                no_secrets,
            )
            .unwrap();
        let now = (MESSAGE_DAYS + 1) * day;
        store
            .send(
                "mcp:a",
                ALL,
                MessageKind::Status,
                None,
                "new",
                now,
                no_secrets,
            )
            .unwrap();
        let kept = store.load().unwrap().messages;
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].text.as_str(), kept[0].seq), ("new", 2));
        // Fill past the count limit: the newest MESSAGES_KEPT stay.
        let mut text = String::new();
        for i in 0..MESSAGES_KEPT as u64 {
            let m = Message {
                seq: 10 + i,
                time: now,
                from: "mcp:a".into(),
                to: ALL.into(),
                kind: MessageKind::Status,
                reply_to: None,
                text: format!("m{i}"),
            };
            text.push_str(&serde_json::to_string(&m).unwrap());
            text.push('\n');
        }
        text.push_str("not a message\n");
        std::fs::write(store.file(), text).unwrap();
        assert_eq!(store.load().unwrap().unreadable, 1);
        let last = store
            .send(
                "mcp:a",
                ALL,
                MessageKind::Status,
                None,
                "last",
                now,
                no_secrets,
            )
            .unwrap();
        assert_eq!(last.seq, 10 + MESSAGES_KEPT as u64);
        let loaded = store.load().unwrap();
        assert_eq!(loaded.messages.len(), MESSAGES_KEPT);
        assert_eq!(loaded.unreadable, 0);
        assert_eq!(loaded.messages[0].text, "m1");
        assert_eq!(loaded.messages.last().unwrap().text, "last");
    }
}
