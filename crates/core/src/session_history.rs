//! The session history of agents (`SPEC-0344`, ROADMAP M19.10): one append-only JSON Lines file
//! per MCP session with the session's tool calls, in the user's settings folder beside the grant
//! file, never in the workspace. Arguments are redacted before they are written (behaviour 9);
//! the files are owner-only and never written through a link (behaviour 3); old sessions are
//! pruned and a session's record is bounded (behaviours 10 and 11). Reading back lists the
//! sessions of one workspace and pages through one session's lines (behaviours 12 and 13).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::permissions::GRANT_FILE;

/// The folder's name beside the grant file (behaviour 2).
pub const HISTORY_FOLDER: &str = "session-history";
/// The format version in the `start` line (behaviour 5).
pub const FORMAT_VERSION: u32 = 1;
/// What a secret is replaced by (behaviour 9).
pub const REDACTED: &str = "‹redacted›";
/// Texts longer than this many characters are cut (behaviour 9).
pub const TEXT_MAX_CHARS: usize = 1000;
/// Characters a cut text keeps.
pub const TEXT_KEPT_CHARS: usize = 200;
/// Nesting deeper than this is left out.
pub const MAX_DEPTH: usize = 32;
/// Arguments longer than this many bytes after redaction are replaced by a note.
pub const ARGUMENTS_MAX_BYTES: usize = 16 * 1024;
/// Characters an error message keeps (behaviour 6).
pub const ERROR_MAX_CHARS: usize = 300;

/// The limits of behaviours 10 and 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Retention {
    /// Sessions kept per workspace, the new one included.
    pub max_sessions: usize,
    /// Age in seconds after which a session goes.
    pub max_age_secs: u64,
    /// Lines a session records.
    pub max_events: u64,
    /// Bytes a session records.
    pub max_bytes: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            max_sessions: 100,
            max_age_secs: 30 * 24 * 60 * 60,
            max_events: 5000,
            max_bytes: 4 * 1024 * 1024,
        }
    }
}

/// How a call ended (behaviour 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The tool answered.
    Ok,
    /// The tool failed.
    Error,
    /// A right was missing or denied.
    Refused,
    /// The client cancelled the call (`SPEC-0006` v57 behaviour 15).
    Cancelled,
}

/// One recorded tool call (behaviour 6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The tool's name.
    pub tool: String,
    /// The arguments after redaction.
    pub arguments: Value,
    /// How many values were replaced.
    pub redacted: usize,
    /// The size of the answer in bytes.
    pub result_bytes: u64,
    /// How long the call took.
    pub duration_ms: u64,
    /// How it ended.
    pub outcome: Outcome,
    /// The redacted, cut message of an error or a refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolCall {
    /// A call of `tool` with `arguments`, redacted by behaviour 9 with `mask` as the extra
    /// masker of texts (it returns the masked text and how many it masked).
    pub fn new(
        tool: &str,
        arguments: Option<&Map<String, Value>>,
        mask: &dyn Fn(&str) -> (String, usize),
    ) -> Self {
        let (arguments, redacted) = match arguments {
            Some(a) => redact_arguments(a, mask),
            None => (Value::Object(Map::new()), 0),
        };
        Self {
            tool: tool.to_owned(),
            arguments,
            redacted,
            result_bytes: 0,
            duration_ms: 0,
            outcome: Outcome::Ok,
            error: None,
        }
    }

    /// The call with its result size, duration and outcome; the error message is redacted with
    /// `mask` and cut.
    pub fn finished(
        mut self,
        result_bytes: u64,
        duration_ms: u64,
        outcome: Outcome,
        error: Option<&str>,
        mask: &dyn Fn(&str) -> (String, usize),
    ) -> Self {
        self.result_bytes = result_bytes;
        self.duration_ms = duration_ms;
        self.outcome = outcome;
        self.error = error.map(|e| {
            let mut n = 0;
            let text = redact_text(e, mask, &mut n);
            cut_chars(&text, ERROR_MAX_CHARS)
        });
        self
    }
}

/// What one line records (behaviours 5 to 8). Lines of a kind this reader does not know are
/// counted as unreadable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// The first line of a session.
    Start {
        /// The format version.
        version: u32,
        /// The client's name and version.
        client: String,
    },
    /// A tool call.
    ToolCall(ToolCall),
    /// A context pack handed to the agent (M19.2).
    Context {
        /// What it was for.
        target: String,
        /// How many pieces it held.
        pieces: u64,
        /// Its size in characters.
        chars: u64,
    },
    /// A change set (M19.4).
    ChangeSet {
        /// Its id.
        id: String,
        /// The files it touches.
        files: Vec<String>,
        /// Its state (proposed, applied, rejected, undone).
        state: String,
    },
    /// A check gate's result (M19.3).
    CheckGate {
        /// The gate.
        gate: String,
        /// Whether it passed.
        passed: bool,
        /// How many findings it reported.
        findings: u64,
    },
    /// What a step cost (M19.5, M19.7).
    Cost {
        /// The model.
        model: String,
        /// Input tokens.
        input_tokens: u64,
        /// Output tokens.
        output_tokens: u64,
        /// The cost in micro-units of a currency.
        cost_micros: u64,
    },
    /// The session reached a limit; nothing after it is recorded.
    Truncated {
        /// `events` or `bytes`.
        reason: String,
    },
}

impl Event {
    /// The `kind` written for it.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::Start { .. } => "start",
            Event::ToolCall(_) => "tool_call",
            Event::Context { .. } => "context",
            Event::ChangeSet { .. } => "change_set",
            Event::CheckGate { .. } => "check_gate",
            Event::Cost { .. } => "cost",
            Event::Truncated { .. } => "truncated",
        }
    }
}

/// One line of a session file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// Its number, from 0.
    pub seq: u64,
    /// Milliseconds since the epoch.
    pub time: u64,
    /// What it records.
    #[serde(flatten)]
    pub event: Event,
}

// ---------------------------------------------------------------------------------------------
// Redaction (behaviour 9)
// ---------------------------------------------------------------------------------------------

/// The words of a key name: split at `-`, `_`, `.`, spaces and lower-to-upper case changes,
/// lower-cased.
fn key_words(key: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in key.chars() {
        if c == '-' || c == '_' || c == '.' || c == ' ' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Whether a key's name says its value is secret (behaviour 9).
pub fn is_secret_key(key: &str) -> bool {
    const WORDS: &[&str] = &[
        "password",
        "passwd",
        "passphrase",
        "secret",
        "token",
        "credential",
        "credentials",
        "authorization",
        "auth",
        "cookie",
        "apikey",
        "privatekey",
        "accesskey",
        "clientsecret",
    ];
    const PAIRS: &[(&str, &str)] = &[
        ("api", "key"),
        ("private", "key"),
        ("access", "key"),
        ("client", "secret"),
    ];
    let words = key_words(key);
    words.iter().any(|w| WORDS.contains(&w.as_str()))
        || words
            .windows(2)
            .any(|p| PAIRS.contains(&(p[0].as_str(), p[1].as_str())))
}

/// The patterns of secrets in texts, each with its replacement.
static TEXT_PATTERNS: LazyLock<Vec<(Regex, String)>> = LazyLock::new(|| {
    let r = |p: &str| Regex::new(p).expect("secret pattern");
    vec![
        (
            r(r"-----BEGIN [A-Z ]*PRIVATE KEY-----(?s:.*?)(?:-----END [A-Z ]*PRIVATE KEY-----|\z)"),
            REDACTED.to_owned(),
        ),
        (
            r(r"(?P<head>\b[A-Za-z][A-Za-z0-9+.\-]*://[^\s:/@]+:)[^\s@/]+@"),
            format!("${{head}}{REDACTED}@"),
        ),
        (
            r(r"(?i)\b(?P<head>bearer|basic)\s+[A-Za-z0-9._~+/=\-]{8,}"),
            format!("${{head}} {REDACTED}"),
        ),
        (
            r(concat!(
                r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}",
                r"|glpat-[A-Za-z0-9_\-]{20,}|sk-[A-Za-z0-9_\-]{20,}|xox[abprs]-[A-Za-z0-9\-]{10,}",
                r"|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_\-]{35}",
                r"|eyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,})"
            )),
            REDACTED.to_owned(),
        ),
        (
            r(concat!(
                r"(?i)(?P<head>\b[A-Z0-9_\-]*(?:PASSWORD|PASSWD|PASSPHRASE|SECRET|TOKEN",
                r"|API_?KEY|PRIVATE_?KEY|ACCESS_?KEY|CREDENTIALS?)[A-Z0-9_\-]*",
                r#"["']?\s*[=:]\s*)(?:"[^"]*"|'[^']*'|[^\s"',;&]+)"#
            )),
            format!("${{head}}{REDACTED}"),
        ),
    ]
});

/// `text` with first its first [`TEXT_KEPT_CHARS`] characters and a note when it is longer
/// than `max` characters.
fn cut_chars(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_owned();
    }
    let keep = TEXT_KEPT_CHARS.min(max);
    let head: String = text.chars().take(keep).collect();
    format!("{head}‹… {} more characters›", total - keep)
}

/// `text` with the secrets of behaviour 9 replaced (counted in `n`), then masked by `mask`.
fn redact_text(text: &str, mask: &dyn Fn(&str) -> (String, usize), n: &mut usize) -> String {
    let mut out = text.to_owned();
    for (pattern, with) in TEXT_PATTERNS.iter() {
        let found = pattern.find_iter(&out).count();
        if found > 0 {
            *n += found;
            out = pattern.replace_all(&out, with.as_str()).into_owned();
        }
    }
    let (masked, m) = mask(&out);
    *n += m;
    masked
}

fn redact_value(
    value: &Value,
    depth: usize,
    mask: &dyn Fn(&str) -> (String, usize),
    n: &mut usize,
) -> Value {
    if depth > MAX_DEPTH {
        return Value::String("‹nested too deep›".to_owned());
    }
    match value {
        Value::String(s) => Value::String(cut_chars(&redact_text(s, mask, n), TEXT_MAX_CHARS)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| redact_value(v, depth + 1, mask, n))
                .collect(),
        ),
        Value::Object(map) => Value::Object(redact_map(map, depth + 1, mask, n)),
        other => other.clone(),
    }
}

fn redact_map(
    map: &Map<String, Value>,
    depth: usize,
    mask: &dyn Fn(&str) -> (String, usize),
    n: &mut usize,
) -> Map<String, Value> {
    map.iter()
        .map(|(k, v)| {
            let kept = matches!(v, Value::Null | Value::Bool(_));
            let v = if is_secret_key(k) && !kept {
                *n += 1;
                Value::String(REDACTED.to_owned())
            } else {
                redact_value(v, depth, mask, n)
            };
            (k.clone(), v)
        })
        .collect()
}

/// `arguments` after behaviour 9 with `mask` as the extra masker, and how many values were
/// replaced.
pub fn redact_arguments(
    arguments: &Map<String, Value>,
    mask: &dyn Fn(&str) -> (String, usize),
) -> (Value, usize) {
    let mut n = 0;
    let out = Value::Object(redact_map(arguments, 0, mask, &mut n));
    let size = serde_json::to_string(&out).map_or(0, |s| s.len());
    if size > ARGUMENTS_MAX_BYTES {
        return (
            Value::String(format!("‹{size} bytes of arguments left out›")),
            n,
        );
    }
    (out, n)
}

/// A masker that masks nothing.
pub fn no_mask(text: &str) -> (String, usize) {
    (text.to_owned(), 0)
}

// ---------------------------------------------------------------------------------------------
// Files (behaviours 2, 3 and 10)
// ---------------------------------------------------------------------------------------------

/// The history folder beside `grant_file`: its name with `agent-permissions.toml` replaced, so a
/// test's grant file `x.agent-permissions.toml` has `x.session-history` beside it.
pub fn history_dir(grant_file: &Path) -> PathBuf {
    let name = grant_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = name.strip_suffix(GRANT_FILE).unwrap_or("");
    grant_file.with_file_name(format!("{prefix}{HISTORY_FOLDER}"))
}

/// The folder of the workspace at `root` below `dir`, named by a hash of its resolved path.
pub fn workspace_dir(dir: &Path, root: &Path) -> PathBuf {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let hex: String = Sha256::digest(root.to_string_lossy().as_bytes())[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    dir.join(hex)
}

/// Whether `id` has the form of behaviour 2: 13 digits, a dash, 8 lower-case hex digits.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 22
        && b[..13].iter().all(u8::is_ascii_digit)
        && b[13] == b'-'
        && b[14..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

/// The start time in milliseconds of a session id.
fn id_time(id: &str) -> u64 {
    id[..13].parse().unwrap_or(0)
}

/// A new session id for `now_ms`.
fn new_id(now_ms: u64) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut h = Sha256::new();
    h.update(nanos.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(n.to_le_bytes());
    let hex: String = h.finalize()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{:013}-{hex}", now_ms.min(9_999_999_999_999))
}

/// Creates `dir` and its missing parents owner-only, and refuses a link in its place.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(dir)
        && meta.file_type().is_symlink()
    {
        return Err(std::io::Error::other(format!(
            "{} is a link; the session history is never written through a link",
            dir.display()
        )));
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// The session ids of the workspace folder `dir`, oldest first; other files are left out.
fn session_ids(dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".jsonl")?;
            valid_id(id).then(|| id.to_owned())
        })
        .collect();
    ids.sort();
    ids
}

/// Deletes the sessions of the workspace folder `dir` that behaviour 10 lets go, keeping room
/// for one new session; returns how many went.
pub fn prune(dir: &Path, now_ms: u64, retention: &Retention) -> usize {
    let ids = session_ids(dir);
    let oldest_kept = now_ms.saturating_sub(retention.max_age_secs.saturating_mul(1000));
    let mut gone = 0;
    let mut left = ids.len();
    for id in &ids {
        let too_old = id_time(id) < oldest_kept;
        let too_many = left >= retention.max_sessions;
        if (too_old || too_many) && std::fs::remove_file(dir.join(format!("{id}.jsonl"))).is_ok() {
            gone += 1;
            left -= 1;
        }
    }
    gone
}

// ---------------------------------------------------------------------------------------------
// Writing (behaviours 1, 5 and 11)
// ---------------------------------------------------------------------------------------------

/// The record of one session being written.
#[derive(Debug)]
pub struct SessionWriter {
    id: String,
    path: PathBuf,
    file: std::fs::File,
    seq: u64,
    bytes: u64,
    retention: Retention,
    ended: bool,
}

impl SessionWriter {
    /// Starts the record of a new session of the workspace at `root` in the history folder
    /// `dir`: prunes the old ones, creates the file new and owner-only, writes the `start` line.
    pub fn start(
        dir: &Path,
        root: &Path,
        client: &str,
        now_ms: u64,
        retention: Retention,
    ) -> std::io::Result<Self> {
        private_dir(dir)?;
        let folder = workspace_dir(dir, root);
        private_dir(&folder)?;
        prune(&folder, now_ms, &retention);
        let id = new_id(now_ms);
        let path = folder.join(format!("{id}.jsonl"));
        let mut options = std::fs::OpenOptions::new();
        // `create_new` fails on any existing entry, a link included, so nothing is written
        // through a link (behaviour 3).
        options.write(true).append(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        let mut writer = Self {
            id,
            path,
            file,
            seq: 0,
            bytes: 0,
            retention,
            ended: false,
        };
        writer.write_line(
            Event::Start {
                version: FORMAT_VERSION,
                client: cut_chars(client, 200),
            },
            now_ms,
        )?;
        Ok(writer)
    }

    /// The session's id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The session's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the session reached a limit and records nothing more.
    pub fn ended(&self) -> bool {
        self.ended
    }

    fn write_line(&mut self, event: Event, now_ms: u64) -> std::io::Result<u64> {
        let mut text = serde_json::to_string(&Line {
            seq: self.seq,
            time: now_ms,
            event,
        })
        .map_err(std::io::Error::other)?;
        text.push('\n');
        self.file.write_all(text.as_bytes())?;
        self.seq += 1;
        self.bytes += text.len() as u64;
        Ok(text.len() as u64)
    }

    /// Appends `event` at `now_ms`; once a limit of behaviour 11 is reached, one `truncated`
    /// line is written instead and nothing after it. Returns whether `event` was written.
    pub fn record(&mut self, event: Event, now_ms: u64) -> std::io::Result<bool> {
        if self.ended {
            return Ok(false);
        }
        let size = serde_json::to_string(&event).map_or(0, |s| s.len() as u64) + 40;
        let reason = if self.seq + 1 >= self.retention.max_events {
            Some("events")
        } else if self.bytes + size > self.retention.max_bytes {
            Some("bytes")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.ended = true;
            self.write_line(
                Event::Truncated {
                    reason: reason.to_owned(),
                },
                now_ms,
            )?;
            return Ok(false);
        }
        self.write_line(event, now_ms)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------------------------
// Reading (behaviours 12 and 13)
// ---------------------------------------------------------------------------------------------

/// What a session holds, from its lines (behaviour 12).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SessionSummary {
    /// Its id.
    pub id: String,
    /// When it started, milliseconds since the epoch.
    pub started: u64,
    /// The time of its last line.
    pub ended: u64,
    /// The client's name and version.
    pub client: String,
    /// Its lines.
    pub lines: u64,
    /// Its tool calls.
    pub tool_calls: u64,
    /// Calls that ended with an error.
    pub errors: u64,
    /// Calls that were refused.
    pub refused: u64,
    /// The summed duration of its calls.
    pub duration_ms: u64,
    /// Whether it reached a limit.
    pub truncated: bool,
    /// Lines that could not be read.
    pub unreadable: u64,
}

/// Reads the lines of `path`, handing each readable one to `each`; returns how many were
/// unreadable.
fn each_line(path: &Path, mut each: impl FnMut(Line)) -> std::io::Result<u64> {
    let file = std::fs::File::open(path)?;
    let mut unreadable = 0;
    for raw in std::io::BufReader::new(file).lines() {
        let Ok(raw) = raw else {
            unreadable += 1;
            continue;
        };
        if raw.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Line>(&raw) {
            Ok(line) => each(line),
            Err(_) => unreadable += 1,
        }
    }
    Ok(unreadable)
}

fn summarize(id: &str, path: &Path) -> std::io::Result<SessionSummary> {
    let mut s = SessionSummary {
        id: id.to_owned(),
        started: id_time(id),
        ..SessionSummary::default()
    };
    s.unreadable = each_line(path, |line| {
        s.lines += 1;
        s.ended = s.ended.max(line.time);
        match line.event {
            Event::Start { client, .. } => {
                s.client = client;
                s.started = line.time;
            }
            Event::ToolCall(call) => {
                s.tool_calls += 1;
                s.duration_ms += call.duration_ms;
                match call.outcome {
                    Outcome::Error => s.errors += 1,
                    Outcome::Refused => s.refused += 1,
                    Outcome::Ok | Outcome::Cancelled => {}
                }
            }
            Event::Truncated { .. } => s.truncated = true,
            _ => {}
        }
    })?;
    Ok(s)
}

/// A page of the sessions of a workspace, newest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SessionList {
    /// The sessions of the page.
    pub sessions: Vec<SessionSummary>,
    /// How many sessions there are in all.
    pub total: usize,
}

/// The sessions of the workspace at `root` in the history folder `dir`, newest first, `limit`
/// of them after skipping `offset`.
pub fn list_sessions(dir: &Path, root: &Path, offset: usize, limit: usize) -> SessionList {
    let folder = workspace_dir(dir, root);
    let mut ids = session_ids(&folder);
    ids.reverse();
    let total = ids.len();
    let sessions = ids
        .iter()
        .skip(offset)
        .take(limit)
        .filter_map(|id| summarize(id, &folder.join(format!("{id}.jsonl"))).ok())
        .collect();
    SessionList { sessions, total }
}

/// One line as read back, with its distance from the session's start (behaviour 13).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReadLine {
    /// Milliseconds since the session's start.
    pub offset_ms: u64,
    /// The line.
    #[serde(flatten)]
    pub line: Line,
}

/// A page of one session's lines (behaviour 13).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionPage {
    /// The session's summary.
    pub session: SessionSummary,
    /// The lines of the page, in order.
    pub lines: Vec<ReadLine>,
    /// The `seq` to continue from, `None` at the end.
    pub next: Option<u64>,
}

/// The lines of session `id` of the workspace at `root` from `seq` `from`, at most `limit`, only
/// of `kinds` when that is not empty. An id not of the form of behaviour 2 is refused before any
/// file is touched; a session that does not exist is an error.
pub fn read_session(
    dir: &Path,
    root: &Path,
    id: &str,
    from: u64,
    limit: usize,
    kinds: &[String],
) -> Result<SessionPage, String> {
    if !valid_id(id) {
        return Err(format!(
            "`{}` is no session id (13 digits, a dash, 8 hexadecimal digits)",
            cut_chars(id, 40)
        ));
    }
    let path = workspace_dir(dir, root).join(format!("{id}.jsonl"));
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("session {id} is a link and is not read"));
    }
    let session = summarize(id, &path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("this workspace has no session {id}"),
        _ => format!("session {id} cannot be read: {e}"),
    })?;
    let start = session.started;
    let mut lines = Vec::new();
    let mut next = None;
    each_line(&path, |line| {
        if line.seq < from || next.is_some() {
            return;
        }
        if !kinds.is_empty() && !kinds.iter().any(|k| k == line.event.kind()) {
            return;
        }
        if lines.len() >= limit {
            next = Some(line.seq);
            return;
        }
        lines.push(ReadLine {
            offset_ms: line.time.saturating_sub(start),
            line,
        });
    })
    .map_err(|e| format!("session {id} cannot be read: {e}"))?;
    Ok(SessionPage {
        session,
        lines,
        next,
    })
}

/// Milliseconds since the epoch now.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
