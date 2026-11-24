//! Traces (`SPEC-0319` behaviour 26) and audit entries (behaviour 38): what happened, never what
//! was typed — typed text and set values appear only as their length, secret values not at all.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::tree::{Effect, ElementInfo, TreeDiff};

/// How long one check waited.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckWait {
    /// The check (`visible`, `stable`, …).
    pub check: String,
    /// How long it waited, in milliseconds.
    pub waited_ms: u64,
}

/// One recorded call.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceEntry {
    /// Milliseconds since the trace started.
    pub time_ms: u64,
    /// `selftest` or `driver:<client>`.
    pub actor: String,
    /// `act`, `expect`, `find`, `snapshot`.
    pub tool: String,
    /// The locator in the text form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    /// The action or matcher, typed text only as its length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// The resolved element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<ElementInfo>,
    /// Each actionability check with its waiting time.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckWait>,
    /// The delivered input events (text as its length).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<serde_json::Value>,
    /// The tree difference before and after.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<TreeDiff>,
    /// `ok` or the error code.
    pub outcome: String,
    /// The error message, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Lines the window process wrote to stderr meanwhile (none are captured in phase 1).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stderr: Vec<String>,
}

/// A running trace.
#[derive(Debug)]
pub struct Trace {
    started: SystemTime,
    started_at: Instant,
    entries: Vec<TraceEntry>,
}

impl Trace {
    /// Starts recording at `now`.
    pub fn start(now: Instant) -> Self {
        Self {
            started: SystemTime::now(),
            started_at: now,
            entries: Vec::new(),
        }
    }

    /// Milliseconds since the start at `now`.
    pub fn elapsed_ms(&self, now: Instant) -> u64 {
        u64::try_from(now.saturating_duration_since(self.started_at).as_millis())
            .unwrap_or(u64::MAX)
    }

    /// Adds an entry.
    pub fn record(&mut self, entry: TraceEntry) {
        self.entries.push(entry);
    }

    /// The entries so far.
    pub fn entries(&self) -> &[TraceEntry] {
        &self.entries
    }

    /// Writes the trace as JSON Lines into a new folder `ui-traces/<time>/` under `base` (folder
    /// 0700, file 0600) and returns the file.
    pub fn write(&self, base: &Path) -> std::io::Result<PathBuf> {
        let stamp = self
            .started
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let traces = base.join("ui-traces");
        create_private_dir(&traces)?;
        let mut dir = traces.join(stamp.to_string());
        let mut n = 1;
        while dir.exists() {
            n += 1;
            dir = traces.join(format!("{stamp}-{n}"));
        }
        create_private_dir(&dir)?;
        let file = dir.join("trace.jsonl");
        let mut text = String::new();
        for e in &self.entries {
            text.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        use std::io::Write;
        options.open(&file)?.write_all(text.as_bytes())?;
        Ok(file)
    }
}

/// Creates `dir` (and its parents) and makes it 0700; refused off Unix, where it cannot be made
/// private yet (`SPEC-0321` v6 behaviour 8a).
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    if cfg!(not(unix)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "private trace folders are not supported on this platform yet",
        ));
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// What the audit log records of one external call (behaviour 38).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditFacts {
    /// The resolved element.
    pub element: Option<ElementInfo>,
    /// The effects of the action.
    pub effects: Vec<Effect>,
}

/// One line of `.ferris-idea/agent-log.jsonl` for an external call: time, actor, tool, locator,
/// resolved role, name and test id, effects, outcome and duration; typed text only as its length.
#[allow(clippy::too_many_arguments)]
pub fn audit_entry(
    client: &str,
    session: &str,
    tool: &str,
    locator: Option<&str>,
    facts: &AuditFacts,
    outcome: &str,
    duration: Duration,
    typed_len: Option<usize>,
) -> serde_json::Value {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut entry = serde_json::json!({
        "time": time,
        "actor": {"client": client, "session": session},
        "tool": tool,
        "outcome": outcome,
        "duration_ms": u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        "effects": facts.effects.iter().map(|e| e.name()).collect::<Vec<_>>(),
    });
    if let Some(l) = locator {
        entry["locator"] = serde_json::json!(l);
    }
    if let Some(e) = &facts.element {
        entry["element"] =
            serde_json::json!({"role": e.role, "name": e.name, "test_id": e.test_id});
    }
    if let Some(n) = typed_len {
        entry["typed_length"] = serde_json::json!(n);
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_are_private_files() {
        let base = std::env::temp_dir().join(format!("uidriver-trace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let now = Instant::now();
        let mut t = Trace::start(now);
        t.record(TraceEntry {
            time_ms: t.elapsed_ms(now),
            actor: "selftest".into(),
            tool: "act".into(),
            locator: Some("id=x".into()),
            action: Some("type (5 characters)".into()),
            element: None,
            checks: vec![CheckWait {
                check: "visible".into(),
                waited_ms: 3,
            }],
            inputs: Vec::new(),
            diff: None,
            outcome: "ok".into(),
            message: None,
            stderr: Vec::new(),
        });
        let file = t.write(&base).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"waited_ms\":3"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&file), 0o600);
            assert_eq!(mode(file.parent().unwrap()), 0o700);
        }
        // A second trace of the same moment gets its own folder.
        let again = t.write(&base).unwrap();
        assert_ne!(again, file);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn audit_entries_hold_lengths_not_text() {
        let e = audit_entry(
            "claude-code 2.1",
            "s1",
            "ui_act",
            Some("role=textinput[name=\"Search\"]"),
            &AuditFacts::default(),
            "ok",
            Duration::from_millis(12),
            Some(11),
        );
        assert_eq!(e["typed_length"], 11);
        assert_eq!(e["actor"]["client"], "claude-code 2.1");
        assert_eq!(e["duration_ms"], 12);
    }
}
