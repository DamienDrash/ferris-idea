//! What the person hands the chat besides code (`SPEC-0313`): masking secrets before anything
//! leaves the IDE, the size budget with head and tail, chips with their source and size, the
//! snapshot of a shared terminal tab, and the commands an answer suggests with the text that
//! "Insert into Terminal" types. Free of the UI toolkit; the window and the MCP server share it.

use std::sync::OnceLock;

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

/// What a masked secret is replaced by (behaviour 6).
pub const MASK: &str = "‹masked›";

/// Default of "Terminal lines to attach" (`ai.terminal_lines`, behaviour 2).
pub const DEFAULT_TERMINAL_LINES: usize = 200;
/// Smallest and largest "Terminal lines to attach".
pub const TERMINAL_LINES_RANGE: (usize, usize) = (10, 5_000);
/// Default size budget of one attachment in characters (`ai.attachment_chars`, behaviour 7).
pub const DEFAULT_ATTACHMENT_CHARS: usize =
    idecraft_core::advanced_settings::DEFAULT_ATTACHMENT_CHARS;
/// Smallest and largest budget (Settings › Advanced Settings, `SPEC-0337` behaviour 5).
pub const ATTACHMENT_CHARS_RANGE: (usize, usize) =
    idecraft_core::advanced_settings::ATTACHMENT_CHARS_RANGE;
/// Most chips that wait at once (behaviour 1).
pub const MAX_CHIPS: usize = 8;

/// The question "Explain with AI" puts into the field for output (behaviour 4).
pub const EXPLAIN_OUTPUT: &str =
    "Explain this output. If it shows an error, say what caused it and how to fix it.";
/// The question "Explain with AI" puts into the field for a failed test (behaviour 4).
pub const EXPLAIN_TEST: &str = "Why does this test fail, and how can it be fixed?";

/// `ai.terminal_lines` within its range; 0 (unset) is the default.
pub fn terminal_lines(setting: usize) -> usize {
    if setting == 0 {
        DEFAULT_TERMINAL_LINES
    } else {
        setting.clamp(TERMINAL_LINES_RANGE.0, TERMINAL_LINES_RANGE.1)
    }
}

/// `ai.attachment_chars` within its range; 0 (unset) is the default.
pub fn attachment_chars(setting: usize) -> usize {
    idecraft_core::advanced_settings::attachment_chars(setting)
}

// ---------------------------------------------------------------------------------------------
// Masking (behaviour 6)
// ---------------------------------------------------------------------------------------------

struct Patterns {
    /// Tokens with published prefixes; the whole match is masked.
    tokens: Regex,
    /// `Authorization: Bearer x`; group 1 is kept.
    header: Regex,
    /// `-u user:password`; group 1 is kept.
    basic: Regex,
    /// `scheme://user:password@`; groups 1 and 2 are kept.
    url: Regex,
    /// Assignments and options: group `k` (all before the value) is kept, `v` is the value.
    assignment: Regex,
    /// A name that looks secret.
    secret_name: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let tokens = [
            r"github_pat_[A-Za-z0-9_]{20,}",
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"glpat-[A-Za-z0-9_\-]{20,}",
            r"xox[abprs]-[A-Za-z0-9\-]{10,}",
            r"sk-(?:ant-|proj-)?[A-Za-z0-9_\-]{20,}",
            r"(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{10,}",
            r"AIza[A-Za-z0-9_\-]{35}",
            r"(?:AKIA|ASIA)[A-Z0-9]{16}",
            r"npm_[A-Za-z0-9]{30,}",
            r"pypi-[A-Za-z0-9_\-]{30,}",
            r"hf_[A-Za-z0-9]{30,}",
            r"eyJ[A-Za-z0-9_\-]{5,}\.eyJ[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}",
        ];
        let name = r"[A-Za-z_][A-Za-z0-9_.\-]*";
        Patterns {
            tokens: Regex::new(&format!(r"\b(?:{})\b", tokens.join("|"))).expect("tokens"),
            header: Regex::new(
                r"(?i)(\b(?:proxy-)?authorization[\x22']?\s*[:=]\s*[\x22']?(?:bearer|basic|token|digest)?\s*)([^\s\x22',;]+)",
            )
            .expect("header"),
            basic: Regex::new(r"(\s-u\s+[\x22']?[^\s:\x22']+:)([^\s@\x22']+)").expect("basic"),
            url: Regex::new(r"(\b[A-Za-z][A-Za-z0-9+.\-]*://)([^\s/:@]+:)([^\s/@]+)@")
                .expect("url"),
            assignment: Regex::new(&format!(
                concat!(
                    r"(?P<k>(?:^|[\s;{{,(=:\x22'])(?:export\s+|set\s+)?",
                    r"(?:--?)?[\x22']?(?P<n>{name})[\x22']?\s*(?:=|:|\s)\s*[\x22']?)",
                    r"(?P<v>[^\s\x22',;}}]+)"
                ),
                name = name
            ))
            .expect("assignment"),
            secret_name: Regex::new(
                r"(?i)(?:^|[_.\-])(?:pwd|auth|api[_.\-]key|access[_.\-]key|private[_.\-]key|session[_.\-]key)(?:$|[_.\-])|password|passwd|secret|token|apikey|credential",
            )
            .expect("secret name"),
        }
    })
}

/// Whether `value` is already a placeholder that needs no masking.
fn placeholder(value: &str) -> bool {
    let v = value.trim_matches(|c| c == '"' || c == '\'');
    v.is_empty()
        || v == MASK
        || v.chars()
            .all(|c| c == '*' || c == 'x' || c == 'X' || c == '.')
        || v.starts_with('<')
        || v.starts_with("${")
        || (v.starts_with('$')
            && v[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// Masks the secrets of one line; adds the number masked to `count`.
fn mask_line(line: &str, count: &mut usize) -> String {
    let p = patterns();
    let mut n = 0;
    let out = p.tokens.replace_all(line, |_: &Captures| {
        n += 1;
        MASK.to_owned()
    });
    let out = p.header.replace_all(&out, |c: &Captures| {
        if placeholder(&c[2]) {
            return c[0].to_owned();
        }
        n += 1;
        format!("{}{MASK}", &c[1])
    });
    let out = p.basic.replace_all(&out, |c: &Captures| {
        if placeholder(&c[2]) {
            return c[0].to_owned();
        }
        n += 1;
        format!("{}{MASK}", &c[1])
    });
    let out = p.url.replace_all(&out, |c: &Captures| {
        if placeholder(&c[3]) {
            return c[0].to_owned();
        }
        n += 1;
        format!("{}{}{MASK}@", &c[1], &c[2])
    });
    // Assignments: a match whose name is not secret gives way at its value, so a secret
    // assignment inside it (`error: TOKEN=x`, `tool --token=x`) is still found.
    let mut result = String::new();
    let mut pos = 0;
    while let Some(c) = p.assignment.captures_at(&out, pos) {
        let (k, v) = (c.name("k").expect("k"), c.name("v").expect("v"));
        let name = &c["n"];
        let value = v.as_str();
        let spaced = !k.as_str().contains(['=', ':']);
        // `--token value` counts only as an option; plain words followed by a space do not.
        let option = k.as_str().trim_start().starts_with('-');
        // The shell's own `PWD` and `OLDPWD` hold folders, not passwords.
        let folder = matches!(name, "PWD" | "OLDPWD");
        if !p.secret_name.is_match(name) || folder || placeholder(value) || (spaced && !option) {
            // Go on from the separator before the value, which may start the next match.
            let back = match out.as_bytes()[..v.start()].last() {
                // A quote too: `printf 'TOKEN=x'` puts the assignment inside a quoted word.
                Some(b) if b.is_ascii_whitespace() || b";{,(=:\"'".contains(b) => v.start() - 1,
                _ => v.start(),
            };
            let back = back.max(pos + 1).min(v.start());
            result.push_str(&out[pos..back]);
            pos = back;
            continue;
        }
        n += 1;
        result.push_str(&out[pos..k.end()]);
        result.push_str(MASK);
        pos = v.end();
    }
    result.push_str(&out[pos..]);
    let out = result;
    *count += n;
    out
}

/// `text` with every secret of behaviour 6 replaced by [`MASK`], and how many were masked.
/// Idempotent: masked text comes back unchanged with a count of 0.
pub fn mask_secrets(text: &str) -> (String, usize) {
    let mut count = 0;
    let mut out = Vec::new();
    let mut in_key: Option<String> = None;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if let Some(end) = &in_key {
            if trimmed.contains(end.as_str()) {
                out.push(line.to_owned());
                in_key = None;
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("-----BEGIN ")
            && let Some(kind) = rest.strip_suffix("-----")
            && kind.ends_with("PRIVATE KEY")
        {
            out.push(line.to_owned());
            out.push(MASK.to_owned());
            count += 1;
            in_key = Some(format!("-----END {kind}-----"));
            continue;
        }
        out.push(mask_line(line, &mut count));
    }
    (out.join("\n"), count)
}

// ---------------------------------------------------------------------------------------------
// Budget (behaviour 7)
// ---------------------------------------------------------------------------------------------

/// `text` within `budget` characters: unchanged when it fits; otherwise its head (a third of the
/// budget) and tail (the rest), cut at line ends, joined by a note of what was left out. The flag
/// says whether it was cut.
pub fn fit_budget(text: &str, budget: usize) -> (String, bool) {
    let total = text.chars().count();
    if total <= budget {
        return (text.to_owned(), false);
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let head_budget = budget / 3;
    let tail_budget = budget - head_budget;
    let mut head = Vec::new();
    let mut used = 0;
    for l in &lines {
        let n = l.chars().count() + 1;
        if used + n > head_budget {
            break;
        }
        used += n;
        head.push(*l);
    }
    let mut tail = Vec::new();
    let mut used_tail = 0;
    for l in lines[head.len()..].iter().rev() {
        let n = l.chars().count() + 1;
        if used_tail + n > tail_budget {
            break;
        }
        used_tail += n;
        tail.push(*l);
    }
    tail.reverse();
    // One line longer than the budget: keep its start and end characters.
    if head.is_empty() && tail.is_empty() {
        let chars: Vec<char> = text.chars().collect();
        let start: String = chars[..head_budget].iter().collect();
        let end: String = chars[chars.len() - tail_budget..].iter().collect();
        let left = total - head_budget - tail_budget;
        return (
            format!("{start}\n‹… 0 lines ({left} characters) left out …›\n{end}"),
            true,
        );
    }
    let left_lines = lines.len() - head.len() - tail.len();
    let left_chars = total.saturating_sub(used + used_tail);
    let mut out = head.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!(
        "‹… {left_lines} lines ({left_chars} characters) left out …›"
    ));
    if !tail.is_empty() {
        out.push('\n');
        out.push_str(&tail.join("\n"));
    }
    (out, true)
}

// ---------------------------------------------------------------------------------------------
// Chips (behaviour 1)
// ---------------------------------------------------------------------------------------------

/// Where an attachment comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Code from the editor (`SPEC-0138`).
    #[default]
    Code,
    /// The selection of a terminal tab.
    TerminalSelection,
    /// The last command of a terminal tab with its output and exit code.
    TerminalCommand,
    /// The last lines of a terminal tab.
    TerminalLines,
    /// The console of a Run tab.
    Run,
    /// A failed test.
    Test,
    /// Problems of a file.
    Problems,
    /// The working tree's Git diff.
    GitDiff,
}

impl Source {
    /// Whether it comes from a terminal tab.
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Source::TerminalSelection | Source::TerminalCommand | Source::TerminalLines
        )
    }
}

/// The last command of a terminal tab, as the shell marked it (`SPEC-0313` behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandText {
    /// The command line.
    pub command: String,
    /// Its output.
    pub output: String,
    /// Its exit code; `None` while it runs.
    pub exit: Option<i32>,
}

impl CommandText {
    /// The text of the chip: the command line after a `$`, its output, and how it ended.
    pub fn text(&self) -> String {
        let mut t = format!("$ {}\n", self.command);
        if !self.output.is_empty() {
            t.push_str(&self.output);
            t.push('\n');
        }
        match self.exit {
            Some(code) => t.push_str(&format!("[exit code {code}]")),
            None => t.push_str("[still running]"),
        }
        t
    }

    /// The chip label's part for the command: `last command · cargo test · exit 101`.
    pub fn label(&self) -> String {
        // Masked before it is cut, so no shortened secret slips past the patterns.
        let (masked, _) = mask_secrets(&self.command);
        let mut command: String = masked.chars().take(40).collect();
        if masked.chars().count() > 40 {
            command.push('…');
        }
        let end = match self.exit {
            Some(code) => format!("exit {code}"),
            None => "running".to_owned(),
        };
        format!("last command · {command} · {end}")
    }
}

/// Masks `raw`, fits it into `budget` and makes the attachment (behaviours 6, 7).
pub fn attachment(
    source: Source,
    label: impl Into<String>,
    language: &str,
    raw: &str,
    budget: usize,
) -> crate::Attachment {
    let (masked_text, masked) = mask_secrets(raw.trim_end_matches('\n'));
    let (code, truncated) = fit_budget(&masked_text, budget);
    crate::Attachment {
        // A label made from a command line may carry a secret too.
        label: mask_secrets(&label.into()).0,
        language: language.to_owned(),
        code,
        source,
        masked,
        truncated,
    }
}

/// The chip of a terminal tab named `tab`: its last command when the shell marks prompts, else its
/// last lines (behaviour 2).
pub fn terminal_attachment(
    tab: &str,
    command: Option<&CommandText>,
    lines: &[String],
    budget: usize,
) -> crate::Attachment {
    match command {
        Some(c) => attachment(
            Source::TerminalCommand,
            format!("Terminal \"{tab}\" · {}", c.label()),
            "console",
            &c.text(),
            budget,
        ),
        None => attachment(
            Source::TerminalLines,
            format!("Terminal \"{tab}\" · last {} lines", lines.len()),
            "console",
            &lines.join("\n"),
            budget,
        ),
    }
}

impl crate::Attachment {
    /// The chip's text: label, size, masked count and whether it was cut.
    pub fn chip(&self) -> String {
        let lines = self.code.lines().count();
        let mut out = format!(
            "{} · {lines} line{}",
            self.label,
            if lines == 1 { "" } else { "s" }
        );
        if self.masked > 0 {
            out.push_str(&format!(" · {} masked", self.masked));
        }
        if self.truncated {
            out.push_str(" · truncated");
        }
        out
    }

    /// Characters of the text.
    pub fn chars(&self) -> usize {
        self.code.chars().count()
    }
}

/// Why a chip cannot be added (behaviours 1, 7).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChipError {
    /// Already [`MAX_CHIPS`] wait.
    #[error("at most {MAX_CHIPS} attachments per question; send or remove one first")]
    TooMany,
    /// Together they would exceed four times the budget.
    #[error("the attachments of one question may have at most {0} characters; remove one first")]
    TooLarge(usize),
}

/// Whether `new` may join `waiting` with the per-attachment `budget` (behaviours 1, 7).
pub fn may_add(
    waiting: &[crate::Attachment],
    new: &crate::Attachment,
    budget: usize,
) -> Result<(), ChipError> {
    if waiting.len() >= MAX_CHIPS {
        return Err(ChipError::TooMany);
    }
    let total: usize = waiting.iter().map(crate::Attachment::chars).sum::<usize>() + new.chars();
    if total > budget * 4 {
        return Err(ChipError::TooLarge(budget * 4));
    }
    Ok(())
}

/// The last snapshot of a shared tab that went out (behaviour 5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedTab {
    last: Option<String>,
}

impl SharedTab {
    /// Whether the snapshot `now` goes with the next question: only when it changed since the
    /// last one that went.
    pub fn take(&mut self, now: &str) -> bool {
        if self.last.as_deref() == Some(now) {
            return false;
        }
        self.last = Some(now.to_owned());
        true
    }
}

// ---------------------------------------------------------------------------------------------
// Suggested commands (behaviour 8)
// ---------------------------------------------------------------------------------------------

/// Fence languages whose blocks are shell commands.
pub const SHELL_LANGUAGES: &[&str] = &["sh", "bash", "zsh", "shell", "console", "terminal"];

/// The shell commands of an answer: the text of every fenced block in a shell language.
pub fn command_blocks(answer: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut open: Option<(String, bool, Vec<&str>)> = None;
    for line in answer.lines() {
        let t = line.trim_start();
        match &mut open {
            None => {
                if let Some(rest) = t.strip_prefix("```") {
                    let fence = format!(
                        "```{}",
                        "`".repeat(rest.len() - rest.trim_start_matches('`').len())
                    );
                    let lang = rest.trim_start_matches('`').trim().to_ascii_lowercase();
                    let shell =
                        SHELL_LANGUAGES.contains(&lang.split_whitespace().next().unwrap_or(""));
                    open = Some((fence, shell, Vec::new()));
                }
            }
            Some((fence, shell, lines)) => {
                if t.starts_with(fence.as_str()) && t.trim_start_matches('`').trim().is_empty() {
                    if *shell && !lines.is_empty() {
                        out.push(lines.join("\n"));
                    }
                    open = None;
                } else {
                    lines.push(line);
                }
            }
        }
    }
    out
}

/// Why a command cannot be inserted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InsertError {
    /// Nothing left to insert.
    #[error("the block holds no command")]
    Empty,
    /// Several lines and the shell would run each one at its line end.
    #[error(
        "this command has several lines and the shell has no bracketed paste, so each line would run at once; copy it instead"
    )]
    SeveralLines,
}

/// The text "Insert into Terminal" types for `command` (behaviour 8): prompts (`$ `, `% `) removed,
/// trailing line ends dropped, control characters other than tab and line end removed; several
/// lines only when the shell has bracketed paste on (`bracketed`). Never ends with a line end, so
/// nothing runs until the person presses Enter.
pub fn insertion(command: &str, bracketed: bool) -> Result<String, InsertError> {
    let cleaned: String = command
        .chars()
        .filter(|c| !c.is_control() || *c == '\t' || *c == '\n')
        .collect();
    let lines: Vec<&str> = cleaned
        .lines()
        .map(|l| {
            let t = l.trim_start();
            t.strip_prefix("$ ")
                .or_else(|| t.strip_prefix("% "))
                .unwrap_or(l)
                .trim_end()
        })
        .collect();
    let text = lines.join("\n").trim_matches('\n').to_owned();
    if text.trim().is_empty() {
        return Err(InsertError::Empty);
    }
    if text.contains('\n') && !bracketed {
        return Err(InsertError::SeveralLines);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(text: &str) -> String {
        mask_secrets(text).0
    }

    #[test]
    fn tokens_with_published_prefixes() {
        // Obviously fake values of the right shape. The token is joined at run time so secret
        // scanners do not flag this fake value.
        let jwt = [
            "eyJhbGciOiJub25lIn0",
            ".eyJzdWIiOiJmYWtlIn0",
            ".ZmFrZXNpZ25hdHVyZQ",
        ]
        .concat();
        let cases = [
            "ghp_FAKEFAKEFAKEFAKEFAKEFAKEFAKE0000",
            "gho_FAKEFAKEFAKEFAKEFAKEFAKE",
            "github_pat_FAKE0000FAKE0000FAKE0000_FAKE",
            "glpat-FAKEFAKEFAKEFAKEFAKE",
            "xoxb-0000000000-FAKEFAKEFAKE",
            "sk-FAKEFAKEFAKEFAKEFAKEFAKE",
            "sk-proj-FAKE_FAKE_FAKE_FAKE_FAKE",
            "sk-ant-api03-FAKEFAKEFAKEFAKEFAKE",
            "sk_live_FAKEFAKEFAKE0000",
            "rk_test_FAKEFAKEFAKE0000",
            "AIzaFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAK",
            "AKIAFAKEFAKEFAKE0000",
            "npm_FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE00",
            "pypi-FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE",
            "hf_FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE",
            jwt.as_str(),
        ];
        for token in cases {
            let (out, n) = mask_secrets(&format!("using {token} now"));
            assert_eq!(out, format!("using {MASK} now"), "{token}");
            assert_eq!(n, 1, "{token}");
        }
    }

    #[test]
    fn secret_named_assignments_keep_the_name() {
        assert_eq!(
            masked("DATABASE_PASSWORD=fake-pass-123"),
            format!("DATABASE_PASSWORD={MASK}")
        );
        assert_eq!(
            masked("export GITHUB_TOKEN=notreal"),
            format!("export GITHUB_TOKEN={MASK}")
        );
        assert_eq!(masked("api_key: notreal"), format!("api_key: {MASK}"));
        assert_eq!(
            masked(r#"  "client_secret": "notreal","#),
            format!(r#"  "client_secret": "{MASK}","#)
        );
        assert_eq!(
            masked(r#"password = "notreal""#),
            format!(r#"password = "{MASK}""#)
        );
        assert_eq!(masked("db.pwd=notreal"), format!("db.pwd={MASK}"));
        assert_eq!(
            masked("AWS_ACCESS_KEY_ID=x1"),
            format!("AWS_ACCESS_KEY_ID={MASK}")
        );
        assert_eq!(masked("Auth-Token: x1"), format!("Auth-Token: {MASK}"));
        assert_eq!(
            masked(&["mytool --to", "ken=notreal --verbose"].concat()),
            format!("mytool --token={MASK} --verbose")
        );
        assert_eq!(
            masked(&["mytool --pass", "word notreal"].concat()),
            format!("mytool --password {MASK}")
        );
        assert_eq!(masked("SESSION_KEY=abc"), format!("SESSION_KEY={MASK}"));
        // Inside a quoted word of a command line (hand test 213).
        assert_eq!(
            masked(r"printf 'API_TOKEN=nicht-echt\n'"),
            format!(r"printf 'API_TOKEN={MASK}'")
        );
        assert_eq!(
            masked(r#"echo "DB_PASSWORD=x1" done"#),
            format!(r#"echo "DB_PASSWORD={MASK}" done"#)
        );
    }

    #[test]
    fn ordinary_text_and_placeholders_stay() {
        for text in [
            "cargo test -- --nocapture",
            "commit 3f2a9c1e5b7d8f0a1b2c3d4e5f6a7b8c9d0e1f2a",
            "/Users/me/src/token_parser.rs:12:5",
            "PATH=/usr/bin:/bin",
            "HOME=/Users/me",
            "PWD=/Users/me/src",
            "PASSWORD=",
            "PASSWORD=***",
            "API_KEY=<your key>",
            "TOKEN=${TOKEN}",
            "TOKEN=$TOKEN",
            "the password is wrong",
            "token count: 12",
            "https://example.com/path?a=1",
            "git@github.com:owner/repo.git",
            "sk-short",
            "authorization failed",
            "",
        ] {
            assert_eq!(mask_secrets(text), (text.to_owned(), 0), "{text}");
        }
    }

    #[test]
    fn headers_basic_auth_and_url_credentials() {
        assert_eq!(
            masked("Authorization: Bearer notreal.value"),
            format!("Authorization: Bearer {MASK}")
        );
        assert_eq!(
            masked("curl -H 'Proxy-Authorization: Basic ZmFrZTpmYWtl' x"),
            format!("curl -H 'Proxy-Authorization: Basic {MASK}' x")
        );
        assert_eq!(
            masked(&["curl -u ali", "ce:notreal https://example.com"].concat()),
            format!("curl -u alice:{MASK} https://example.com")
        );
        assert_eq!(
            masked("fatal: unable to access 'https://bob:notreal@git.example.com/r.git/'"),
            format!("fatal: unable to access 'https://bob:{MASK}@git.example.com/r.git/'")
        );
        assert_eq!(
            masked("postgres://app:notreal@localhost:5432/db"),
            format!("postgres://app:{MASK}@localhost:5432/db")
        );
        // A user without password is not a secret.
        assert_eq!(masked("ssh://git@example.com/r"), "ssh://git@example.com/r");
    }

    #[test]
    fn a_dotenv_file_and_secrets_inside_other_text() {
        let env = "# settings\nAPP_NAME=demo\nDB_PASSWORD=notreal\nSTRIPE_SECRET_KEY=notreal\nDATABASE_URL=postgres://app:notreal@db:5432/app\nDEBUG=true";
        let (out, n) = mask_secrets(env);
        assert_eq!(
            out,
            format!(
                "# settings\nAPP_NAME=demo\nDB_PASSWORD={MASK}\nSTRIPE_SECRET_KEY={MASK}\nDATABASE_URL=postgres://app:{MASK}@db:5432/app\nDEBUG=true"
            )
        );
        assert_eq!(n, 3);
        assert_eq!(
            masked("error: TOKEN=notreal rejected"),
            format!("error: TOKEN={MASK} rejected")
        );
        assert_eq!(
            masked(r#"{"Authorization": "Bearer notreal", "x": 1}"#),
            format!(r#"{{"Authorization": "Bearer {MASK}", "x": 1}}"#)
        );
        assert_eq!(
            masked("env | grep KEY; API_KEY=notreal; done"),
            format!("env | grep KEY; API_KEY={MASK}; done")
        );
    }

    #[test]
    fn private_key_blocks_are_masked_whole() {
        let text = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nRkFLRUZBS0U=\nRkFLRQ==\n-----END OPENSSH PRIVATE KEY-----\nafter";
        let (out, n) = mask_secrets(text);
        assert_eq!(
            out,
            format!(
                "before\n-----BEGIN OPENSSH PRIVATE KEY-----\n{MASK}\n-----END OPENSSH PRIVATE KEY-----\nafter"
            )
        );
        assert_eq!(n, 1);
        // A public key or certificate is no secret.
        let cert = "-----BEGIN CERTIFICATE-----\nRkFLRQ==\n-----END CERTIFICATE-----";
        assert_eq!(mask_secrets(cert), (cert.to_owned(), 0));
    }

    #[test]
    fn masking_is_idempotent_and_counts() {
        let text = "A_TOKEN=one\nB_SECRET=two\nghp_FAKEFAKEFAKEFAKEFAKEFAKE00\nplain";
        let (once, n) = mask_secrets(text);
        assert_eq!(n, 3);
        assert_eq!(mask_secrets(&once), (once.clone(), 0));
        assert!(!once.contains("one") && !once.contains("two"));
        assert!(once.ends_with("plain"));
    }

    #[test]
    fn budget_keeps_head_and_tail_at_line_ends() {
        assert_eq!(fit_budget("short", 100), ("short".to_owned(), false));
        let text: String = (0..100)
            .map(|i| format!("line {i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (out, cut) = fit_budget(&text, 90);
        assert!(cut);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "line 000");
        assert_eq!(*lines.last().unwrap(), "line 099");
        assert!(out.contains("lines (") && out.contains("characters) left out"));
        // Whole lines only, the head about a third.
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("line ") || l.starts_with('‹'))
        );
        let head = lines.iter().take_while(|l| l.starts_with("line ")).count();
        assert_eq!(head, 3);
        assert!(out.chars().count() < 90 + 60);
    }

    #[test]
    fn one_huge_line_keeps_its_ends() {
        let text = format!("{}{}", "a".repeat(500), "z".repeat(500));
        let (out, cut) = fit_budget(&text, 300);
        assert!(cut);
        assert!(out.starts_with(&"a".repeat(100)));
        assert!(out.ends_with(&"z".repeat(200)));
        assert!(out.contains("700 characters) left out"));
    }

    #[test]
    fn chips_name_source_size_and_masking() {
        let c = CommandText {
            command: "cargo test".into(),
            output: "error: TOKEN=fake\nfailed".into(),
            exit: Some(101),
        };
        let a = terminal_attachment("Local", Some(&c), &[], 1_000);
        assert_eq!(a.source, Source::TerminalCommand);
        assert_eq!(
            a.chip(),
            "Terminal \"Local\" · last command · cargo test · exit 101 · 4 lines · 1 masked"
        );
        assert_eq!(
            a.code,
            format!("$ cargo test\nerror: TOKEN={MASK}\nfailed\n[exit code 101]")
        );
        let running = CommandText {
            exit: None,
            ..c.clone()
        };
        assert!(running.label().ends_with("· running"));
        assert!(running.text().ends_with("[still running]"));
        let lines = vec!["one".to_owned(), "two".to_owned()];
        let b = terminal_attachment("Local (2)", None, &lines, 1_000);
        assert_eq!(b.source, Source::TerminalLines);
        assert_eq!(b.chip(), "Terminal \"Local (2)\" · last 2 lines · 2 lines");
        let big = attachment(
            Source::Run,
            "Run \"x\"",
            "text",
            &"y\n".repeat(5_000),
            1_000,
        );
        assert!(big.truncated && big.chip().ends_with("· truncated"));
    }

    #[test]
    fn several_chips_go_in_order_before_the_question() {
        let a = attachment(Source::Run, "Run \"tests\"", "text", "boom", 1_000);
        let b = attachment(Source::GitDiff, "Git diff · 1 file", "diff", "+x", 1_000);
        let m = crate::user_message_with("Why?", &[a.clone(), b.clone()]);
        let at_a = m.content.find("Run \"tests\"").unwrap();
        let at_b = m.content.find("Git diff").unwrap();
        let at_q = m.content.find("Why?").unwrap();
        assert!(at_a < at_b && at_b < at_q, "{}", m.content);
        assert!(m.content.contains("```diff\n+x\n```"));
        // Limits: count and total size.
        let many = vec![a.clone(); MAX_CHIPS];
        assert_eq!(may_add(&many, &b, 1_000), Err(ChipError::TooMany));
        let large = attachment(Source::Run, "r", "text", &"x".repeat(900), 1_000);
        let four = vec![large.clone(); 4];
        assert_eq!(
            may_add(&four, &large, 1_000),
            Err(ChipError::TooLarge(4_000))
        );
        assert_eq!(may_add(&four[..3], &large, 1_000), Ok(()));
    }

    #[test]
    fn a_shared_tab_goes_once_per_change() {
        let mut shared = SharedTab::default();
        assert!(shared.take("a"));
        assert!(!shared.take("a"));
        assert!(shared.take("b"));
        assert!(shared.take("a"));
    }

    #[test]
    fn settings_ranges() {
        assert_eq!(terminal_lines(0), 200);
        assert_eq!(terminal_lines(3), 10);
        assert_eq!(terminal_lines(9_999), 5_000);
        assert_eq!(attachment_chars(0), 24_000);
        assert_eq!(attachment_chars(5), 1_000);
    }

    #[test]
    fn command_blocks_by_language() {
        let answer = "Run this:\n```bash\n$ cargo test -p x\n```\nor\n```rust\nfn main() {}\n```\n```console\nls\npwd\n```\n```\nplain\n```\n";
        assert_eq!(
            command_blocks(answer),
            vec!["$ cargo test -p x".to_owned(), "ls\npwd".to_owned()]
        );
        // A longer fence holds a shorter one.
        let nested = "````sh\necho '```'\n````\n";
        assert_eq!(command_blocks(nested), vec!["echo '```'".to_owned()]);
    }

    #[test]
    fn insertion_never_presses_enter() {
        assert_eq!(
            insertion("$ cargo test\n\n", false),
            Ok("cargo test".into())
        );
        assert_eq!(insertion("% ls -la", false), Ok("ls -la".into()));
        // Escape sequences and carriage returns cannot press keys.
        assert_eq!(
            insertion("echo hi\x1b[A\r\x03\x07", false),
            Ok("echo hi[A".into())
        );
        assert_eq!(insertion("a\nb", false), Err(InsertError::SeveralLines));
        assert_eq!(insertion("$ a\n$ b\n", true), Ok("a\nb".into()));
        assert_eq!(insertion(" \n\x1b ", true), Err(InsertError::Empty));
        assert!(!insertion("x\n", true).unwrap().ends_with('\n'));
    }
}
