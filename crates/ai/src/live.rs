//! What the window shows of runs and of the AI chat, for the MCP tools `run_output` and
//! `ai_assistant` on other threads (`SPEC-0313` behaviour 9). The window publishes; readers get
//! copies. Text is published as shown and masked by the reader before it leaves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::context::Source;

/// A failed test of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedTest {
    /// Suite and test name, e.g. `calc › divides`.
    pub name: String,
    /// Its message.
    pub message: String,
    /// Details or stack trace.
    pub details: String,
}

impl FailedTest {
    /// The attachment text: name, message, details.
    pub fn text(&self) -> String {
        let mut t = self.name.clone();
        for part in [&self.message, &self.details] {
            if !part.trim().is_empty() {
                t.push_str("\n\n");
                t.push_str(part.trim_end());
            }
        }
        t
    }
}

/// One tab of the Run tool window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunTab {
    /// Configuration name.
    pub name: String,
    /// Whether it still runs.
    pub running: bool,
    /// Exit code of the last run, once it ended.
    pub exit_code: Option<i32>,
    /// The console text.
    #[serde(skip)]
    pub text: String,
    /// Failed tests of the last run.
    #[serde(skip)]
    pub failed: Vec<FailedTest>,
}

/// A chip waiting in the chat, without its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChipInfo {
    /// Where it comes from.
    pub source: Source,
    /// Its label.
    pub label: String,
    /// Lines of text.
    pub lines: usize,
    /// Characters of text.
    pub chars: usize,
    /// Secrets masked.
    pub masked: usize,
    /// Whether it was cut to the budget.
    pub truncated: bool,
}

impl From<&crate::Attachment> for ChipInfo {
    fn from(a: &crate::Attachment) -> Self {
        Self {
            source: a.source,
            label: a.label.clone(),
            lines: a.code.lines().count(),
            chars: a.chars(),
            masked: a.masked,
            truncated: a.truncated,
        }
    }
}

/// The chat's waiting chips and shared tabs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChatState {
    /// Chips waiting for the next question.
    pub attachments: Vec<ChipInfo>,
    /// Names of the terminal tabs shared with the chat.
    pub shared: Vec<String>,
}

#[derive(Default)]
struct State {
    runs: HashMap<PathBuf, Vec<RunTab>>,
    chats: HashMap<PathBuf, ChatState>,
    /// The temporary run configurations of each workspace (`SPEC-0327` behaviour 9).
    temporary: HashMap<PathBuf, Vec<idecraft_core::RunConfig>>,
}

/// Runs and chats by workspace. One per process.
#[derive(Default)]
pub struct Live(Mutex<State>);

fn key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

impl Live {
    /// The registry of this process, which the window publishes to.
    pub fn global() -> &'static Live {
        static GLOBAL: OnceLock<Live> = OnceLock::new();
        GLOBAL.get_or_init(Live::default)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The Run tool window of the workspace at `root` now shows `tabs`.
    pub fn set_runs(&self, root: &Path, tabs: Vec<RunTab>) {
        self.state().runs.insert(key(root), tabs);
    }

    /// The Run tabs of the workspace at `root`.
    pub fn runs(&self, root: &Path) -> Vec<RunTab> {
        self.state()
            .runs
            .get(&key(root))
            .cloned()
            .unwrap_or_default()
    }

    /// The temporary run configurations of the workspace at `root` are now `configs`, for MCP
    /// `run_configuration_edit` with `save_temporary` (`SPEC-0327` behaviour 10).
    pub fn set_temporary_runs(&self, root: &Path, configs: Vec<idecraft_core::RunConfig>) {
        self.state().temporary.insert(key(root), configs);
    }

    /// The temporary run configurations of the workspace at `root`.
    pub fn temporary_runs(&self, root: &Path) -> Vec<idecraft_core::RunConfig> {
        self.state()
            .temporary
            .get(&key(root))
            .cloned()
            .unwrap_or_default()
    }

    /// The chat of the workspace at `root` now shows `chat`.
    pub fn set_chat(&self, root: &Path, chat: ChatState) {
        self.state().chats.insert(key(root), chat);
    }

    /// The chat of the workspace at `root`.
    pub fn chat(&self, root: &Path) -> ChatState {
        self.state()
            .chats
            .get(&key(root))
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_chats_per_workspace() {
        let live = Live::default();
        let tab = RunTab {
            name: "tests".into(),
            running: false,
            exit_code: Some(1),
            text: "boom".into(),
            failed: vec![FailedTest {
                name: "calc › divides".into(),
                message: "expected 2".into(),
                details: "at calc.rs:3".into(),
            }],
        };
        live.set_runs(Path::new("/ws"), vec![tab.clone()]);
        assert_eq!(live.runs(Path::new("/ws")), vec![tab.clone()]);
        assert!(live.runs(Path::new("/other")).is_empty());
        assert_eq!(
            tab.failed[0].text(),
            "calc › divides\n\nexpected 2\n\nat calc.rs:3"
        );
        let a = crate::context::attachment(Source::Run, "Run \"tests\"", "text", "a\nb", 100);
        live.set_chat(
            Path::new("/ws"),
            ChatState {
                attachments: vec![ChipInfo::from(&a)],
                shared: vec!["Local".into()],
            },
        );
        let chat = live.chat(Path::new("/ws"));
        assert_eq!(
            (chat.attachments[0].lines, chat.attachments[0].chars),
            (2, 3)
        );
        assert_eq!(chat.shared, ["Local"]);
        assert_eq!(live.chat(Path::new("/other")), ChatState::default());
    }
}
