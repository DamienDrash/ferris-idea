//! The assistant through a chat subscription (`SPEC-0160`): Ferris IDEA runs the provider's own command
//! line program — Claude Code (`claude`), Codex (`codex`) or Gemini CLI (`gemini`) — that the user
//! installed and signed in to with their subscription, and reads its answer. Ferris IDEA never sees,
//! stores or sends the subscription's tokens; signing in happens in the program itself.
//!
//! The program runs as a plain chat: in an empty temporary folder, with its tools switched off or
//! read only, without the user's project settings, hooks or MCP servers, and with nothing kept
//! after the answer.
//!
//! Claude's plan has a mode of its own (`SPEC-0160` v2, `ai.claude_plan_mode`): "Chat" (the
//! default) runs Claude Code behind the chat as above; "Terminal" runs the user's own Claude Code
//! only in a terminal tab of the project ([`claude_code_command`]) and the chat then uses Claude
//! through the Anthropic API key, or sends nothing and says why ([`CLAUDE_PLAN_MESSAGE`]).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::{AiError, Message};

/// A provider's command line program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cli {
    /// Claude Code with a Claude Pro or Max plan.
    Claude,
    /// Codex with a ChatGPT plan.
    Codex,
    /// Gemini CLI with a Google account.
    Gemini,
}

/// The programs in the order the settings offer them.
pub const CLIS: [Cli; 3] = [Cli::Claude, Cli::Codex, Cli::Gemini];

/// What the chat and the settings say while Claude's plan is set to the Terminal and no Anthropic
/// API key is set up (`SPEC-0160` v2 behaviour 6).
pub const CLAUDE_PLAN_MESSAGE: &str = "Claude with your Claude plan is set to run in the \
Terminal, as Claude Code. In AI Chat, Claude then needs your own Anthropic API key. Open Claude \
Code in the Terminal, or set up the Anthropic API.";

/// What the settings say under the Claude plan row (`SPEC-0160` v2 behaviour 6).
pub const CLAUDE_PLAN_MODE_NOTE: &str = "Chat uses your Claude plan behind AI Chat and waits for \
Anthropic's approval of this use; Terminal runs Claude Code in a terminal of the project, the use \
within Anthropic's current terms.";

/// What a terminal tab of the project runs for Claude with the plan (`SPEC-0160` v2 behaviour 7):
/// the user's own Claude Code, `claude` when it is on `PATH`, else its full path in quotes; `None`
/// when it is not installed. Ferris IDEA only types the command; Claude Code signs in by itself.
pub fn claude_code_command() -> Option<String> {
    let path = find(Cli::Claude)?;
    let on_path = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(Cli::Claude.program()) == path));
    if on_path {
        return Some(Cli::Claude.program().to_owned());
    }
    Some(format!(
        "'{}'",
        path.display().to_string().replace('\'', "'\\''")
    ))
}

impl Cli {
    /// The id kept in the settings (`ai.subscription`).
    pub fn id(self) -> &'static str {
        match self {
            Cli::Claude => "claude",
            Cli::Codex => "codex",
            Cli::Gemini => "gemini",
        }
    }

    /// The program of `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        CLIS.into_iter().find(|c| c.id() == id.trim())
    }

    /// Name shown in the settings.
    pub fn name(self) -> &'static str {
        match self {
            Cli::Claude => "Claude subscription (Claude Code)",
            Cli::Codex => "ChatGPT subscription (Codex)",
            Cli::Gemini => "Google account (Gemini CLI)",
        }
    }

    /// The program's file name.
    pub fn program(self) -> &'static str {
        self.id()
    }

    /// What the user runs to sign in (Ferris IDEA types it into a terminal tab).
    pub fn login_command(self) -> &'static str {
        match self {
            Cli::Claude => "claude auth login",
            Cli::Codex => "codex login",
            // Gemini CLI asks how to sign in when started interactively.
            Cli::Gemini => "gemini",
        }
    }

    /// Where to get the program.
    pub fn install_hint(self) -> &'static str {
        match self {
            Cli::Claude => "install Claude Code: https://docs.claude.com/en/docs/claude-code",
            Cli::Codex => "install Codex: https://developers.openai.com/codex/cli",
            Cli::Gemini => "install Gemini CLI: https://github.com/google-gemini/gemini-cli",
        }
    }

    /// The arguments for one answer to `prompt` (behaviour 2). Claude Code and Codex read the
    /// prompt from standard input; Gemini CLI takes it as an argument.
    pub fn args(self, model: &str, system: &str, prompt: &str) -> Vec<String> {
        let mut a: Vec<String> = match self {
            Cli::Claude => [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                // No tools, no settings, hooks, MCP servers or slash commands, nothing kept.
                "--tools",
                "",
                "--setting-sources",
                "",
                "--strict-mcp-config",
                "--no-session-persistence",
                "--disable-slash-commands",
                "--system-prompt",
                system,
            ]
            .map(str::to_owned)
            .to_vec(),
            Cli::Codex => [
                "exec",
                "--json",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "-c",
                "approval_policy=\"never\"",
            ]
            .map(str::to_owned)
            .to_vec(),
            Cli::Gemini => [
                "-p",
                prompt,
                "--output-format",
                "stream-json",
                "--approval-mode",
                "plan",
            ]
            .map(str::to_owned)
            .to_vec(),
        };
        if !model.trim().is_empty() {
            a.push(if self == Cli::Claude { "--model" } else { "-m" }.into());
            a.push(model.trim().to_owned());
        }
        if self == Cli::Codex {
            // The prompt follows on standard input.
            a.push("-".into());
        }
        a
    }

    /// Whether the prompt goes to standard input.
    fn prompt_on_stdin(self) -> bool {
        self != Cli::Gemini
    }

    /// Whether the system prompt has to be part of the prompt (no option for it).
    fn system_in_prompt(self) -> bool {
        self != Cli::Claude
    }
}

/// What one line of the program's JSON output says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// A piece of the answer, streamed.
    Piece(String),
    /// A complete message (Codex sends whole messages; Claude Code its final result).
    Whole(String),
    /// The program failed with this message.
    Error(String),
}

/// Reads one line of `cli`'s JSON output (behaviour 3).
pub fn parse_line(cli: Cli, line: &str) -> Option<Out> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let s = |v: &serde_json::Value| v.as_str().map(str::to_owned);
    match cli {
        Cli::Claude => match v["type"].as_str()? {
            "stream_event" if v["event"]["delta"]["type"] == "text_delta" => {
                s(&v["event"]["delta"]["text"]).map(Out::Piece)
            }
            "result" if v["is_error"].as_bool() == Some(true) => Some(Out::Error(
                s(&v["result"]).unwrap_or_else(|| "Claude Code failed".into()),
            )),
            "result" => s(&v["result"]).map(Out::Whole),
            _ => None,
        },
        Cli::Codex => match v["type"].as_str()? {
            "item.completed" if v["item"]["type"] == "agent_message" => {
                s(&v["item"]["text"]).map(Out::Whole)
            }
            "error" => s(&v["message"]).map(Out::Error),
            "turn.failed" => Some(Out::Error(
                s(&v["error"]["message"]).unwrap_or_else(|| "Codex failed".into()),
            )),
            _ => None,
        },
        Cli::Gemini => match v["type"].as_str()? {
            "message" if v["role"] == "assistant" => s(&v["content"]).map(Out::Piece),
            "error" if v["severity"] != "warning" => s(&v["message"]).map(Out::Error),
            "result" if v["status"] == "error" => Some(Out::Error(
                s(&v["error"]["message"]).unwrap_or_else(|| "Gemini CLI failed".into()),
            )),
            _ => None,
        },
    }
}

/// The conversation as one prompt: the earlier turns marked by role, then the last question.
pub fn transcript(system: Option<&str>, messages: &[Message]) -> String {
    let mut out = String::new();
    if let Some(s) = system {
        out.push_str(s);
        out.push_str("\n\n");
    }
    let (last, earlier) = match messages.split_last() {
        Some((l, e)) => (Some(l), e),
        None => (None, &[][..]),
    };
    if !earlier.is_empty() {
        out.push_str("Conversation so far:\n\n");
        for m in earlier {
            let who = if m.role == "assistant" {
                "Assistant"
            } else {
                "User"
            };
            out.push_str(&format!("{who}: {}\n\n", m.content));
        }
        out.push_str("Answer the user's next message.\n\n");
    }
    if let Some(m) = last {
        out.push_str(&m.content);
    }
    out
}

#[cfg(debug_assertions)]
static TEST_PROGRAMS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Debug builds only: searches `dir` before every other folder for the rest of this process, so a
/// test program finds stand-ins of the providers' programs there without changing `PATH` (test
/// programs run their tests on several threads and may not set variables). The first folder named
/// stays; the folder this call settled on.
#[cfg(debug_assertions)]
pub fn use_test_programs(dir: impl Into<PathBuf>) -> PathBuf {
    TEST_PROGRAMS.get_or_init(|| dir.into()).clone()
}

/// Folders searched besides `PATH`: apps started from the Dock get a short `PATH`.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(debug_assertions)]
    dirs.extend(TEST_PROGRAMS.get().cloned());
    dirs.extend(
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for d in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".local/node/bin",
            ".volta/bin",
            ".bun/bin",
        ] {
            dirs.push(home.join(d));
        }
    }
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

/// The installed program of `cli`, if any.
pub fn find(cli: Cli) -> Option<PathBuf> {
    search_dirs()
        .into_iter()
        .map(|d| d.join(cli.program()))
        .find(|p| p.is_file())
}

/// Whether the user is signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Login {
    /// The program is not installed.
    NotInstalled,
    /// Installed, not signed in.
    SignedOut,
    /// Signed in.
    SignedIn,
    /// Installed; whether signed in shows at the first question.
    Unknown,
}

/// Asks the program whether the user is signed in (behaviour 1). Only yes or no is read; no
/// account details are kept or shown.
pub fn login(cli: Cli) -> Login {
    let Some(program) = find(cli) else {
        return Login::NotInstalled;
    };
    let run = |args: &[&str]| {
        Command::new(&program)
            .args(args)
            .env("PATH", child_path(&program))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
    };
    match cli {
        Cli::Claude => match run(&["auth", "status"]) {
            Some(out) => {
                let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
                match v["loggedIn"].as_bool() {
                    Some(true) => Login::SignedIn,
                    Some(false) => Login::SignedOut,
                    None => Login::Unknown,
                }
            }
            None => Login::Unknown,
        },
        Cli::Codex => match run(&["login", "status"]) {
            Some(out) if out.status.success() => Login::SignedIn,
            Some(_) => Login::SignedOut,
            None => Login::Unknown,
        },
        Cli::Gemini => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let signed = std::env::var_os("GEMINI_API_KEY").is_some()
                || home.is_some_and(|h| h.join(".gemini/oauth_creds.json").is_file());
            if signed {
                Login::SignedIn
            } else {
                Login::SignedOut
            }
        }
    }
}

/// `PATH` for the program: the usual folders plus its own (Gemini CLI needs `node` beside it).
fn child_path(program: &Path) -> std::ffi::OsString {
    let mut dirs = search_dirs();
    if let Some(d) = program.parent() {
        dirs.insert(0, d.to_path_buf());
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// Turns the program's last error lines into a message, recognising a missing sign-in.
fn failure(cli: Cli, text: &str) -> AiError {
    let lower = text.to_lowercase();
    let signed_out = [
        "not logged in",
        "please log in",
        "login required",
        "run /login",
        "auth method",
        "not authenticated",
        "unauthorized",
        "401",
    ]
    .iter()
    .any(|p| lower.contains(p));
    if signed_out {
        return AiError::Subscription(format!(
            "not signed in to {}: run `{}` (Settings › Tools › AI Assistant, Sign in)",
            cli.name(),
            cli.login_command()
        ));
    }
    let last: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let tail = last[last.len().saturating_sub(3)..].join(" ");
    AiError::Subscription(format!(
        "{} failed: {}",
        cli.program(),
        tail.chars().take(300).collect::<String>()
    ))
}

static RUN: AtomicU64 = AtomicU64::new(0);

/// Asks `cli` for the answer to `messages` (behaviour 2): `on_piece` gets the answer as it comes
/// and stops it by returning false; the whole answer. Waits at most `timeout`.
pub fn ask(
    cli: Cli,
    model: &str,
    system: &str,
    messages: &[Message],
    timeout: Duration,
    on_piece: &mut dyn FnMut(&str) -> bool,
) -> Result<String, AiError> {
    let program = find(cli).ok_or_else(|| {
        AiError::Subscription(format!(
            "{} is not installed; {}",
            cli.program(),
            cli.install_hint()
        ))
    })?;
    ask_with(&program, cli, model, system, messages, timeout, on_piece)
}

/// [`ask`] with the program's path (tests run a stand-in).
pub fn ask_with(
    program: &Path,
    cli: Cli,
    model: &str,
    system: &str,
    messages: &[Message],
    timeout: Duration,
    on_piece: &mut dyn FnMut(&str) -> bool,
) -> Result<String, AiError> {
    let prompt = transcript(cli.system_in_prompt().then_some(system), messages);
    // An empty folder of its own, removed afterwards: the program sees no project files.
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-ai-{}-{}",
        std::process::id(),
        RUN.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| AiError::Transport(e.to_string()))?;
    let result = run(
        program, cli, model, system, &prompt, &dir, timeout, on_piece,
    );
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[allow(clippy::too_many_arguments)]
fn run(
    program: &Path,
    cli: Cli,
    model: &str,
    system: &str,
    prompt: &str,
    dir: &Path,
    timeout: Duration,
    on_piece: &mut dyn FnMut(&str) -> bool,
) -> Result<String, AiError> {
    let mut child = Command::new(program)
        .args(cli.args(model, system, prompt))
        .current_dir(dir)
        .env("PATH", child_path(program))
        .stdin(if cli.prompt_on_stdin() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AiError::Transport(format!("{}: {e}", program.display())))?;
    if let Some(mut input) = child.stdin.take() {
        let prompt = prompt.to_owned();
        // Written from a thread so a large prompt cannot block reading the answer.
        std::thread::spawn(move || {
            let _ = input.write_all(prompt.as_bytes());
        });
    }
    let mut stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.take(64 * 1024).read_to_string(&mut s);
        }
        s
    });
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AiError::Transport("no output".into()))?;
    let pid = child.id();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = timed_out.clone();
    let watchdog = std::thread::spawn(move || {
        if done_rx.recv_timeout(timeout).is_err() {
            flag.store(true, Ordering::Relaxed);
            kill(pid);
        }
    });
    let mut answer = String::new();
    let mut streamed = false;
    let mut error = None;
    let mut stopped = false;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        match parse_line(cli, &line) {
            Some(Out::Piece(p)) => {
                streamed = true;
                answer.push_str(&p);
                if !on_piece(&p) {
                    stopped = true;
                    break;
                }
            }
            // Claude Code repeats the streamed answer as its result.
            Some(Out::Whole(_)) if streamed && cli == Cli::Claude => {}
            Some(Out::Whole(w)) => {
                let piece = if answer.is_empty() {
                    w
                } else {
                    format!("\n\n{w}")
                };
                answer.push_str(&piece);
                if !on_piece(&piece) {
                    stopped = true;
                    break;
                }
            }
            Some(Out::Error(e)) => error = Some(e),
            None => {}
        }
    }
    if stopped {
        kill(pid);
    }
    let status = child.wait().ok();
    let _ = done_tx.send(());
    let _ = watchdog.join();
    let stderr = errors.join().unwrap_or_default();
    if stopped {
        return Ok(answer);
    }
    if timed_out.load(Ordering::Relaxed) {
        return Err(AiError::Transport(format!(
            "{} gave no answer within {} s",
            cli.program(),
            timeout.as_secs()
        )));
    }
    if let Some(e) = error {
        return Err(failure(cli, &e));
    }
    if answer.is_empty() && !status.is_some_and(|s| s.success()) {
        return Err(failure(cli, &stderr));
    }
    Ok(answer)
}

/// Ends the process `pid` (the program's own children end with its pipes).
fn kill(pid: u32) {
    let _ = Command::new("kill")
        .arg(pid.to_string())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in program printing `output` and copying its arguments and input to `seen`.
    fn stand_in(name: &str, output: &str, exit: i32) -> (PathBuf, PathBuf) {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-cli-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seen = dir.join("seen");
        let out = dir.join("out");
        std::fs::write(&out, output).unwrap();
        let script = dir.join(name);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{s}.args'\npwd > '{s}.cwd'\ncat > '{s}.in'\ncat '{o}'\nexit {exit}\n",
                s = seen.display(),
                o = out.display()
            ),
        )
        .unwrap();
        Command::new("chmod")
            .arg("755")
            .arg(&script)
            .status()
            .unwrap();
        (script, seen)
    }

    #[test]
    fn claude_streams_pieces_without_tools_in_an_empty_folder() {
        let output = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hel"}}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"lo"}}}"#,
            "\n",
            r#"{"type":"result","is_error":false,"result":"hello"}"#,
            "\n",
        );
        let (program, seen) = stand_in("claude", output, 0);
        let mut pieces = Vec::new();
        let messages = [
            Message::new("user", "hi"),
            Message::new("assistant", "yes?"),
            Message::new("user", "say hello"),
        ];
        let answer = ask_with(
            &program,
            Cli::Claude,
            "",
            "SYS",
            &messages,
            Duration::from_secs(10),
            &mut |p| {
                pieces.push(p.to_owned());
                true
            },
        )
        .unwrap();
        assert_eq!(answer, "hello");
        assert_eq!(pieces, ["hel", "lo"]);
        let args = std::fs::read_to_string(seen.with_extension("args")).unwrap();
        for a in [
            "--tools",
            "--strict-mcp-config",
            "--no-session-persistence",
            "SYS",
        ] {
            assert!(args.lines().any(|l| l == a), "{a}: {args}");
        }
        let input = std::fs::read_to_string(seen.with_extension("in")).unwrap();
        assert!(input.contains("User: hi") && input.contains("Assistant: yes?"));
        assert!(input.ends_with("say hello") && !input.contains("SYS"));
        let cwd = std::fs::read_to_string(seen.with_extension("cwd")).unwrap();
        assert!(cwd.contains("ferris-idea-ai-"), "{cwd}");
        assert!(!Path::new(cwd.trim()).exists(), "the folder is removed");
    }

    #[test]
    fn codex_and_gemini_answers_and_errors() {
        let (program, seen) = stand_in(
            "codex",
            "{\"type\":\"thread.started\"}\n{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"done\"}}\n",
            0,
        );
        let answer = ask_with(
            &program,
            Cli::Codex,
            "gpt-x",
            "SYS",
            &[Message::new("user", "q")],
            Duration::from_secs(10),
            &mut |_| true,
        )
        .unwrap();
        assert_eq!(answer, "done");
        let args = std::fs::read_to_string(seen.with_extension("args")).unwrap();
        assert!(
            args.contains("read-only") && args.contains("gpt-x") && args.trim_end().ends_with('-')
        );
        assert!(
            std::fs::read_to_string(seen.with_extension("in"))
                .unwrap()
                .starts_with("SYS")
        );

        let (program, seen) = stand_in(
            "gemini",
            "{\"type\":\"message\",\"role\":\"user\",\"content\":\"q\"}\n{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"a\",\"delta\":true}\n{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"b\",\"delta\":true}\n",
            0,
        );
        let answer = ask_with(
            &program,
            Cli::Gemini,
            "",
            "SYS",
            &[Message::new("user", "q")],
            Duration::from_secs(10),
            &mut |_| true,
        )
        .unwrap();
        assert_eq!(answer, "ab");
        assert!(
            std::fs::read_to_string(seen.with_extension("args"))
                .unwrap()
                .contains("plan")
        );

        let (program, _) = stand_in("gemini", "", 41);
        // No output and a failing exit: the error lines decide; a missing sign-in is named.
        let err = failure(Cli::Gemini, "Please set an Auth method in settings.json");
        assert!(err.to_string().contains("not signed in"), "{err}");
        let err = ask_with(
            &program,
            Cli::Gemini,
            "",
            "",
            &[Message::new("user", "q")],
            Duration::from_secs(10),
            &mut |_| true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("gemini failed"), "{err}");
        assert_eq!(
            parse_line(
                Cli::Claude,
                r#"{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}"#
            ),
            Some(Out::Error("Not logged in · Please run /login".into()))
        );
    }

    /// `SPEC-0160` v2 AK-9: Claude Code outside `PATH` is typed into the terminal as its quoted
    /// full path; the message names the Terminal, Claude Code and the Anthropic API key.
    #[cfg(debug_assertions)]
    #[test]
    fn claude_code_is_found_for_the_terminal() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-cli-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = use_test_programs(dir);
        let program = dir.join("claude");
        std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
        assert_eq!(
            claude_code_command().unwrap(),
            format!("'{}'", program.display())
        );
        for part in ["Terminal", "Claude Code", "Anthropic API key"] {
            assert!(CLAUDE_PLAN_MESSAGE.contains(part), "{part}");
        }
        assert!(CLAUDE_PLAN_MODE_NOTE.contains("approval"));
    }

    #[test]
    fn stops_and_times_out() {
        let (program, _) = stand_in(
            "claude",
            "{\"type\":\"stream_event\",\"event\":{\"delta\":{\"type\":\"text_delta\",\"text\":\"a\"}}}\n",
            0,
        );
        let answer = ask_with(
            &program,
            Cli::Claude,
            "",
            "",
            &[],
            Duration::from_secs(10),
            &mut |_| false,
        )
        .unwrap();
        assert_eq!(answer, "a");
        let dir = std::env::temp_dir().join(format!("ferris-idea-cli-slow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let slow = dir.join("codex");
        std::fs::write(&slow, "#!/bin/sh\nexec sleep 30\n").unwrap();
        Command::new("chmod")
            .arg("755")
            .arg(&slow)
            .status()
            .unwrap();
        let start = std::time::Instant::now();
        let err = ask_with(
            &slow,
            Cli::Codex,
            "",
            "",
            &[],
            Duration::from_millis(500),
            &mut |_| true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no answer within"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(10));
        assert_eq!(Cli::from_id("codex"), Some(Cli::Codex));
        assert_eq!(Cli::from_id(""), None);
    }
}
