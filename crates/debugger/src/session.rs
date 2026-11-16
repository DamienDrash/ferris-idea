//! A debugging session on top of a debug adapter (`SPEC-0039` behaviours 2–5); conditions, hit
//! counts and logpoints the adapter cannot evaluate are evaluated here (`SPEC-0178`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use crate::DapError;
use crate::client::DapClient;
use crate::expr;
use crate::extras::{self, Lang, ThreadInfo, object_id_of};
use crate::values::{self, Need, Size};

/// Time one request may take.
const REQUEST: Duration = Duration::from_secs(30);
/// Lazy variables of one answer resolved at once for their text (`SPEC-0091` v11 behaviour 17).
const LAZY_RESOLVED: usize = 30;
/// Rows of one `variables` answer whose fields are read for a summary (`SPEC-0091` v16 behaviour
/// 42).
const SUMMARY_ROWS: usize = 30;
/// How long one read of an object's fields for a summary may take.
const SUMMARY_REQUEST: Duration = Duration::from_secs(1);
/// How long the summaries of one answer may take together.
const SUMMARY_TOTAL: Duration = Duration::from_secs(2);
/// Calls of one line Smart Step Into enters and leaves at most before giving up.
const SMART_STEPS: usize = 12;
/// Steps Step Out of Code Block takes at most (`SPEC-0091` v12 behaviour 25).
pub const BLOCK_STEPS: usize = 200;
/// How long `launch` may take: kotlin-debug-adapter answers only after resolving the classpath with
/// Gradle or Maven (`SPEC-0132`).
const LAUNCH: Duration = Duration::from_secs(120);

/// The `initialize` arguments; child sessions are accepted for js-debug only.
fn initialize_arguments(adapter_id: &str) -> Value {
    json!({
        "clientID": "idecraft", "clientName": "IDECraft", "adapterID": adapter_id,
        "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path",
        "supportsRunInTerminalRequest": false, "locale": "en",
        "supportsStartDebuggingRequest": adapter_id.starts_with("pwa-"),
    })
}

/// A line breakpoint as the adapter reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Breakpoint {
    /// Adapter's id, used by later `breakpoint` events.
    pub id: Option<i64>,
    /// Whether the adapter could place it.
    pub verified: bool,
    /// Line it ended up on (1-based), if reported.
    pub line: Option<u64>,
    /// The line asked for (1-based); `None` for function breakpoints (`SPEC-0127` v5
    /// behaviour 14: the gutter matches by it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_line: Option<u64>,
    /// Why the adapter could not bind it, as the adapter says (DAP `message`; `SPEC-0127` v5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Who evaluates its condition, hit count and log message (`SPEC-0178` behaviour 1).
    pub evaluated_by: EvaluatedBy,
}

/// Who evaluates a breakpoint's condition, hit count and log message (`SPEC-0178` behaviour 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EvaluatedBy {
    /// The debug adapter (or there is nothing to evaluate).
    #[default]
    Adapter,
    /// IDECraft, at each hit, because the adapter cannot.
    IdeCraft,
}

/// IDECraft's hint why a breakpoint in `path` may not be bound (`SPEC-0039` behaviour 8; the
/// window's tooltip when the adapter gives no reason, `SPEC-0127` v5 behaviour 14).
pub fn unbound_hint(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).ok();
    if !path.exists() {
        "the file does not exist".to_owned()
    } else if resolved.as_deref().is_some_and(|r| r != path) {
        format!(
            "the path is reached through a symbolic link; the program may refer to {} (map it with a source map)",
            resolved.unwrap_or_default().display()
        )
    } else {
        "no code at this line in the loaded program, or the program was built from a different path (e.g. through a symbolic link: map it with a source map) or without debug information".to_owned()
    }
}

/// The adapter's reason for a breakpoint it did not bind (DAP `Breakpoint.message`), if any.
fn adapter_reason(b: &Value) -> Option<String> {
    if b["verified"].as_bool() == Some(true) {
        return None;
    }
    b["message"]
        .as_str()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
}

/// The note for the debug console when IDECraft evaluates breakpoints (`SPEC-0178` behaviour 6).
pub const CLIENT_SIDE_NOTE: &str = "[IDECraft] The debug adapter cannot evaluate breakpoint conditions, hit counts or log messages; IDECraft evaluates them at each hit. This is slower: the program stops briefly every time such a line is reached.\n";

/// A hit condition: `N`/`==N`, `>N`, `>=N`, `<N`, `<=N` or `%N` (`SPEC-0178` behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitCondition {
    /// Only the N-th hit.
    Equal(u64),
    /// Hits after the N-th.
    Greater(u64),
    /// From the N-th hit on.
    AtLeast(u64),
    /// Hits before the N-th.
    Less(u64),
    /// Up to the N-th hit.
    AtMost(u64),
    /// Every N-th hit.
    Every(u64),
}

impl HitCondition {
    /// Reads the DAP syntax; `None` for anything else (and `%0`).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (make, rest): (fn(u64) -> Self, &str) = if let Some(r) = text.strip_prefix(">=") {
            (Self::AtLeast, r)
        } else if let Some(r) = text.strip_prefix("<=") {
            (Self::AtMost, r)
        } else if let Some(r) = text.strip_prefix("==") {
            (Self::Equal, r)
        } else if let Some(r) = text.strip_prefix('>') {
            (Self::Greater, r)
        } else if let Some(r) = text.strip_prefix('<') {
            (Self::Less, r)
        } else if let Some(r) = text.strip_prefix('%') {
            (Self::Every, r)
        } else {
            (Self::Equal, text)
        };
        let n = rest.trim().parse().ok()?;
        let made = make(n);
        (made != Self::Every(0)).then_some(made)
    }

    /// Whether the program stops at hit number `hits` (counting from 1).
    pub fn met(self, hits: u64) -> bool {
        match self {
            Self::Equal(n) => hits == n,
            Self::Greater(n) => hits > n,
            Self::AtLeast(n) => hits >= n,
            Self::Less(n) => hits < n,
            Self::AtMost(n) => hits <= n,
            Self::Every(n) => hits.is_multiple_of(n),
        }
    }
}

/// Refuses to change values when the adapter cannot (`SPEC-0194` behaviour 4).
pub fn set_variable_supported(capabilities: &Value) -> Result<(), DapError> {
    if capabilities["supportsSetVariable"] == true {
        Ok(())
    } else {
        Err(DapError::Failed {
            command: "setVariable".into(),
            message: "The debug adapter cannot change values".into(),
        })
    }
}

/// Refuses `command` with `message` unless the adapter reports `capability` (IMP-0022: features the
/// adapter lacks are refused with a reason, not ignored; `SPEC-0091` v11 behaviours 18–22).
pub fn supported(
    capabilities: &Value,
    capability: &str,
    command: &str,
    message: &str,
) -> Result<(), DapError> {
    if capabilities[capability] == true {
        Ok(())
    } else {
        Err(DapError::Failed {
            command: command.into(),
            message: message.into(),
        })
    }
}

/// The targets of Smart Step Into from a `stepInTargets` answer: id and label, in the adapter's
/// order (`SPEC-0091` v11 behaviour 18).
pub fn step_targets(body: &Value) -> Vec<(i64, String)> {
    body["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some((t["id"].as_i64()?, t["label"].as_str()?.to_owned())))
        .collect()
}

/// The texts of a `completions` answer, without repeats, in the adapter's order (`SPEC-0091` v11
/// behaviour 22).
pub fn completion_texts(body: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in body["targets"].as_array().into_iter().flatten() {
        let Some(label) = t["label"].as_str() else {
            continue;
        };
        // Without a `text` the label's name part is inserted (java-debug: `get(Object) : V`).
        let text = match t["text"].as_str() {
            Some(text) => text.to_owned(),
            None => label
                .split(['(', ' ', ':'])
                .next()
                .unwrap_or(label)
                .to_owned(),
        };
        if !text.is_empty() && !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// The method a Smart Step Into label names: `square(3)` → `square`, `item.total()` → `total`.
pub fn target_method(label: &str) -> &str {
    let head = label.split('(').next().unwrap_or(label).trim();
    head.rsplit(['.', ' ']).next().unwrap_or(head)
}

/// Whether frame name `frame` (`Smart.square(int)`, `square`, `Item.total`) is method `name`.
pub fn frame_is_method(frame: &str, name: &str) -> bool {
    let head = frame.split('(').next().unwrap_or(frame).trim();
    head.rsplit(['.', ' ', '$']).next() == Some(name)
}

/// Reads an evaluated condition as a truth value (`SPEC-0178` behaviour 4); `None` when it is none.
pub fn truth(value: &str) -> Option<bool> {
    let value = value.trim();
    match value {
        "true" | "True" => Some(true),
        "false" | "False" | "null" | "nil" | "None" | "undefined" => Some(false),
        _ => value.parse::<f64>().ok().map(|n| n != 0.0),
    }
}

/// Whether an `evaluate` answer means the adapter could not evaluate the expression
/// (kotlin-debug-adapter answers `unknown` for anything but a variable name).
fn unanswered(value: &str) -> bool {
    value.is_empty() || value == "unknown"
}

/// What IDECraft evaluates itself for one line breakpoint (`SPEC-0178` behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kept {
    /// Adapter's id, to follow a line the adapter moves.
    id: Option<i64>,
    /// Line it is on (as placed, else as asked).
    line: u64,
    condition: Option<String>,
    hit_condition: Option<HitCondition>,
    log_message: Option<String>,
    /// Hits that passed the condition so far.
    hits: u64,
    /// The further options (`SPEC-0127` v3).
    rules: BreakpointRules,
    /// The breakpoint this one waits for (resolved path, 1-based line).
    master: Option<(PathBuf, u64)>,
    /// Whether that breakpoint was hit, so this one is active.
    armed: bool,
}

impl Kept {
    fn same_rule(&self, other: &Self) -> bool {
        self.line == other.line
            && self.condition == other.condition
            && self.hit_condition == other.hit_condition
            && self.log_message == other.log_message
            && self.rules == other.rules
    }

    /// Whether the adapter left a condition, pass count or log message to IDECraft.
    fn client_side(&self) -> bool {
        self.condition.is_some() || self.hit_condition.is_some() || self.log_message.is_some()
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Replaces each `{expression}` of a log message with `value(expression)`.
fn interpolate(message: &str, mut value: impl FnMut(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&value(&rest[open + 1..open + close]));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// Why kotlin-debug-adapter cannot be told the class of a line (`SPEC-0132` v5 behaviour 4b).
const UNBINDABLE_KOTLIN: &str = "kotlin-debug-adapter finds the class of a line from the folders after the last one called `kotlin` or `java`, and the package of this file has a part with one of these names";

/// Why a line stays unset when kotlin-debug-adapter did not take the path naming its class.
const UNTAKEN_KOTLIN: &str =
    "kotlin-debug-adapter refused or did not answer the request that names the class of this line";
/// How long kotlin-debug-adapter may take for a request that names a class; it answers these
/// at once, and a missing answer must not hold up the launch (`SPEC-0132` v6 behaviour 4b).
const CLASS_SOURCE: Duration = Duration::from_secs(5);
/// How long nothing may come from a launched Kotlin program before a lost stop is asked for.
const LOST_STOP_AFTER: Duration = Duration::from_millis(300);
/// How long one question for a lost stop may take, and all of them together.
const LOST_STOP_REQUEST: Duration = Duration::from_secs(2);
const LOST_STOP_TOTAL: Duration = Duration::from_secs(4);

/// A breakpoint that is still not bound after the program loaded (`SPEC-0039` behaviour 8, IMP-0030).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingBreakpoint {
    /// Source file as given.
    pub path: PathBuf,
    /// Requested line.
    pub line: Option<u64>,
    /// Why it is not bound, as the adapter says (`SPEC-0127` v5 behaviour 15).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Why it may not bind.
    pub hint: String,
}

/// A line breakpoint to set, optionally conditional or as a logpoint (`SPEC-0039` behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
pub struct SourceBreakpoint {
    /// Line (1-based).
    pub line: u64,
    /// Expression that must be true for the program to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// How many hits to pass before stopping (adapter syntax, usually a number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Message to log instead of stopping; `{expr}` parts are evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
    /// What IDECraft does at a hit besides (`SPEC-0127` v3 behaviours 6–11).
    #[serde(default, skip_serializing_if = "BreakpointRules::is_empty")]
    pub rules: BreakpointRules,
}

/// Which threads a breakpoint stops (`SPEC-0127` v3 behaviour 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Suspend {
    /// Every thread of the program.
    All,
    /// Only the thread that hit it.
    Thread,
    /// None: the program goes on after the breakpoint's messages.
    None,
}

/// What IDECraft does at a breakpoint's hit besides the adapter's condition, pass count and log
/// message (`SPEC-0127` v3 behaviours 6–11); all empty for a plain breakpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BreakpointRules {
    /// Which threads stop; `None` keeps the adapter's own way (java-debug: the thread, debugpy:
    /// all).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suspend: Option<Suspend>,
    /// Write "Breakpoint hit" with the place to the console.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub log_hit: bool,
    /// Write the call stack to the console.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub log_stack: bool,
    /// Remove the breakpoint after its first hit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub remove_once_hit: bool,
    /// Do nothing until the breakpoint `path:line` (1-based) was hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<String>,
    /// After that hit stay active; otherwise wait for it again after each own hit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub depends_keep: bool,
    /// Only for these objects: ids as the values show them (`33`, `0x1f`), separated by spaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_filter: Option<String>,
    /// Only in these classes: patterns with `*`, `-` before one excludes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_filter: Option<String>,
    /// Only when called from these methods (`Class.method` patterns, `-` excludes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_filter: Option<String>,
}

impl BreakpointRules {
    /// Whether nothing is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The breakpoint `path:line` a rule names: the path and the 1-based line.
pub fn breakpoint_place(text: &str) -> Option<(PathBuf, u64)> {
    let (path, line) = text.trim().rsplit_once(':')?;
    let line = line.trim().parse().ok().filter(|l| *l > 0)?;
    (!path.is_empty()).then(|| (PathBuf::from(path), line))
}

impl From<u64> for SourceBreakpoint {
    fn from(line: u64) -> Self {
        Self {
            line,
            ..Self::default()
        }
    }
}

/// Breakpoints that are not on a line (`SPEC-0146`): on exceptions and on functions or methods.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SpecialBreakpoints {
    /// Stop when an exception is thrown, caught or not.
    pub caught: bool,
    /// Stop when an exception is not caught.
    pub uncaught: bool,
    /// Only for these exception types (adapter syntax, e.g. `java.lang.NullPointerException`);
    /// empty: every exception.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub exception_condition: String,
    /// Stop on entry of these functions or methods.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<FunctionBreakpoint>,
    /// Exceptions only where thrown in these classes: patterns with `*`, a leading `-` excludes
    /// (`SPEC-0146` v3; java-debug only).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub exception_classes: String,
    /// Stop when these fields are read or written (`SPEC-0146` v3; adapters with data
    /// breakpoints, java-debug).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldWatchpoint>,
    /// Exception settings of single languages (`SPEC-0146` v6 behaviour 15); a language without
    /// its own uses the shared ones above.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub languages: std::collections::BTreeMap<String, LanguageExceptions>,
}

impl SpecialBreakpoints {
    /// Whether nothing is set.
    pub fn is_empty(&self) -> bool {
        !self.caught
            && !self.uncaught
            && self.functions.is_empty()
            && self.fields.is_empty()
            && self
                .languages
                .values()
                .all(|l| *l == LanguageExceptions::default())
    }

    /// The exception settings of `language`: its own, else the shared ones (behaviour 15).
    pub fn exceptions_of(&self, language: &str) -> LanguageExceptions {
        self.languages
            .get(language)
            .cloned()
            .unwrap_or_else(|| LanguageExceptions {
                caught: self.caught,
                uncaught: self.uncaught,
                exception_condition: self.exception_condition.clone(),
                exception_classes: self.exception_classes.clone(),
            })
    }

    /// What a session of adapter `adapter_id` gets: the exception settings of its language as the
    /// shared ones, no other language's, and only the method breakpoints that are on.
    pub fn for_adapter(&self, adapter_id: &str) -> SpecialBreakpoints {
        let e = self.exceptions_of(exception_language(adapter_id));
        SpecialBreakpoints {
            caught: e.caught,
            uncaught: e.uncaught,
            exception_condition: e.exception_condition,
            exception_classes: e.exception_classes,
            functions: self
                .functions
                .iter()
                .filter(|f| !f.disabled && (f.entry || f.exit))
                .cloned()
                .collect(),
            fields: self.fields.clone(),
            languages: std::collections::BTreeMap::new(),
        }
    }

    /// The method breakpoint ⌘F8 set on 0-based `line` of `file`, if any (behaviour 12).
    pub fn method_at(&self, file: &std::path::Path, line: usize) -> Option<usize> {
        self.functions
            .iter()
            .position(|f| f.line == Some(line) && f.file.as_deref() == Some(file))
    }
}

/// A field watchpoint (`SPEC-0146` v3): stop when a field is accessed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct FieldWatchpoint {
    /// The field: `pkg.Class.field`, or the adapter's own id (java-debug `pkg.Class#field`, `$`
    /// for nested classes).
    pub field: String,
    /// `write` (the default), `read` or `readWrite`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub access: String,
    /// Expression that must be true for the program to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

impl FieldWatchpoint {
    /// The DAP access type: `write` unless `read` or `readWrite` is given.
    pub fn access_type(&self) -> &str {
        match self.access.as_str() {
            "read" => "read",
            "readWrite" | "readwrite" | "both" => "readWrite",
            _ => "write",
        }
    }
}

/// A breakpoint on entry of a function or method (`SPEC-0146`), or on its exit (v6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct FunctionBreakpoint {
    /// Name in the adapter's syntax: `main`, `Shapes::area`, `com.acme.Shop.total`.
    pub name: String,
    /// Expression that must be true for the program to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Stop on entry (`SPEC-0146` v6 behaviour 13); on by default.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub entry: bool,
    /// Stop after the method returned, emulated by IDECraft with a step out (v6 behaviour 13).
    #[serde(default, skip_serializing_if = "is_false")]
    pub exit: bool,
    /// Switched off: kept, not sent (v6 behaviour 14).
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// The file of the header line ⌘F8 set it on (v6 behaviour 12), for the gutter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<std::path::PathBuf>,
    /// The 0-based header line in `file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

impl Default for FunctionBreakpoint {
    fn default() -> Self {
        Self {
            name: String::new(),
            condition: None,
            entry: true,
            exit: false,
            disabled: false,
            file: None,
            line: None,
        }
    }
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The exception settings of one language (`SPEC-0146` v6 behaviour 15).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LanguageExceptions {
    /// Stop when an exception is thrown, caught or not.
    pub caught: bool,
    /// Stop when an exception is not caught.
    pub uncaught: bool,
    /// Only these exception types.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub exception_condition: String,
    /// Only exceptions thrown in these classes.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub exception_classes: String,
}

impl LanguageExceptions {
    /// Whether any exception option is on.
    pub fn on(&self) -> bool {
        self.caught || self.uncaught
    }
}

/// The languages whose exception breakpoints the dialog groups (`SPEC-0146` v6 behaviour 15):
/// key and heading.
pub const EXCEPTION_LANGUAGES: [(&str, &str); 5] = [
    ("java", "Java"),
    ("python", "Python"),
    ("javascript", "JavaScript"),
    ("native", "Native"),
    ("go", "Go"),
];

/// The exception language of adapter id `adapter_id` (behaviour 15).
pub fn exception_language(adapter_id: &str) -> &'static str {
    match adapter_id {
        "java" | "java-debug" | "kotlin" | "kotlin-debug-adapter" => "java",
        "debugpy" | "python" => "python",
        "pwa-node" | "node" | "js-debug" | "pwa-chrome" | "chrome" => "javascript",
        "dlv" | "go" | "delve" => "go",
        _ => "native",
    }
}

/// The exception types of `condition`, separated by commas or white space.
pub fn exception_types(condition: &str) -> Vec<String> {
    condition
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A method breakpoint's name as java-debug reads it, `package.Class#method` (D30: the written
/// `demo.Crash.area` never stopped); `Class::method` and names with `#` are taken as well.
pub fn java_method_name(name: &str) -> String {
    let name = name.trim();
    if name.contains('#') {
        return name.to_owned();
    }
    if let Some((class, method)) = name.rsplit_once("::") {
        return format!("{class}#{method}");
    }
    match name.rsplit_once('.') {
        Some((class, method)) => format!("{class}#{method}"),
        None => name.to_owned(),
    }
}

/// Which of the adapter's exception filters stand for "caught" (every thrown exception) and
/// "uncaught" (`SPEC-0146` behaviour 2), by the ids the adapters use: java-debug, debugpy,
/// js-debug, lldb-dap and others.
pub fn exception_filters(
    capabilities: &Value,
    special: &SpecialBreakpoints,
) -> Result<Vec<String>, DapError> {
    let ids: Vec<String> = capabilities["exceptionBreakpointFilters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["filter"].as_str().map(str::to_owned))
        .collect();
    let pick = |wanted: &[&str]| {
        ids.iter()
            .filter(|id| wanted.contains(&id.as_str()))
            .cloned()
            .collect::<Vec<_>>()
    };
    let mut out = Vec::new();
    let refuse = |what: &str| DapError::Failed {
        command: "setExceptionBreakpoints".into(),
        message: format!("the debug adapter offers no breakpoint on {what} exceptions"),
    };
    if special.caught {
        let found = pick(&[
            "caught",
            "raised",
            "all",
            "cpp_throw",
            "objc_throw",
            "swift_throw",
        ]);
        if found.is_empty() {
            return Err(refuse("caught"));
        }
        out.extend(found);
    }
    if special.uncaught {
        // One filter only, the adapter's plainest: with debugpy's `uncaught` and `userUnhandled`
        // both on, one uncaught exception stopped the program twice (`SPEC-0146` v4).
        let mut found: Vec<String> = ["uncaught", "unhandled", "userUnhandled"]
            .iter()
            .find_map(|wanted| ids.iter().find(|id| id == wanted).cloned())
            .into_iter()
            .collect();
        // Delve: a panic nothing recovers and a fatal error of the runtime; one event is never
        // both, so both are on (`SPEC-0146` v5 behaviour 11).
        if found.is_empty() {
            found = pick(&["unrecovered-panic", "runtime-fatal-throw"]);
        }
        // A "throw" filter already stops on every exception, the uncaught ones included.
        if found.is_empty() && out.is_empty() {
            return Err(refuse("uncaught"));
        }
        out.extend(found);
    }
    out.dedup();
    Ok(out)
}

/// Why and where the program stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stopped {
    /// `breakpoint`, `step`, `entry`, `exception`, `pause`, …
    pub reason: String,
    /// Thread that stopped.
    pub thread_id: i64,
}

/// What happened after resuming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Event {
    /// Stopped again.
    Stopped(Stopped),
    /// Ended with this exit code (if the adapter reported one).
    Exited(Option<i64>),
}

/// One stack frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Frame {
    /// Frame id for scopes and evaluation.
    pub id: i64,
    /// Function name.
    pub name: String,
    /// Source file, if known.
    pub path: Option<PathBuf>,
    /// Line (1-based).
    pub line: i64,
    /// The adapter marks the frame as less important: a `subtle` frame or a `deemphasize` source
    /// (`SPEC-0091` v10 behaviour 15).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deemphasized: bool,
}

/// The variables of a `variables` answer and the references of those marked lazy.
pub fn parse_variables(body: &Value) -> (Vec<Variable>, Vec<i64>) {
    let mut lazy = Vec::new();
    let vars = body["variables"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let reference = v["variablesReference"].as_i64().unwrap_or_default();
            if reference != 0 && v["presentationHint"]["lazy"] == true {
                lazy.push(reference);
            }
            Variable {
                name: v["name"].as_str().unwrap_or_default().to_owned(),
                value: v["value"].as_str().unwrap_or_default().to_owned(),
                kind: v["type"].as_str().map(str::to_owned),
                reference,
                evaluate_name: v["evaluateName"]
                    .as_str()
                    .filter(|e| !e.is_empty())
                    .map(str::to_owned),
            }
        })
        .collect();
    (vars, lazy)
}

/// What is written for an object after reading its fields (`SPEC-0091` v16 behaviour 42).
enum Summary {
    /// ` size = N` after type and id.
    Size(u64),
    /// The value instead of type and id (a boxed number).
    Value(String),
}

/// The reference of a lazy variable's value, from the answer for its children: its one child, when
/// that has children of its own.
pub fn lazy_value(children: &[Variable]) -> Option<i64> {
    match children {
        [value] if value.reference != 0 => Some(value.reference),
        _ => None,
    }
}

/// One variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Variable {
    /// Name.
    pub name: String,
    /// Value as the adapter shows it.
    pub value: String,
    /// Type, if reported.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Reference for children (0 = none).
    pub reference: i64,
    /// The expression the adapter gives for the value (DAP `evaluateName`, java-debug
    /// `items.get(0)`), for Inspect, views and renderers (`SPEC-0091` v12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluate_name: Option<String>,
}

/// Where the program is after waiting (`SPEC-0054` behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum StopState {
    /// Stopped: reason, thread, top frames and the local variables of the top frame.
    Stopped {
        /// Why it stopped.
        reason: String,
        /// Thread that stopped.
        thread_id: i64,
        /// Top of the call stack.
        frames: Vec<Frame>,
        /// Local variables of the top frame.
        locals: Vec<Variable>,
        /// Breakpoints that are still not bound.
        pending_breakpoints: Vec<PendingBreakpoint>,
    },
    /// Ended.
    Exited {
        /// Exit code, if reported.
        exit_code: Option<i64>,
        /// Breakpoints that never bound.
        pending_breakpoints: Vec<PendingBreakpoint>,
    },
    /// Still running when the wait ended.
    Running,
}

/// A debugging session.
pub struct DebugSession {
    client: DapClient,
    output: String,
    console: String,
    /// Output and console together in the order they came (D17).
    transcript: String,
    capabilities: Value,
    /// Placed breakpoints per file, kept current from `breakpoint` events.
    placed: BTreeMap<PathBuf, Vec<Breakpoint>>,
    /// Files whose placed breakpoints such events changed, for [`DebugSession::take_rebound`].
    rebound: std::collections::BTreeSet<PathBuf>,
    /// Attached to a program that ran before (`SPEC-0147`): ending detaches, the program goes on.
    attached: bool,
    /// Breakpoint parts IDECraft evaluates, per file with symbolic links resolved (`SPEC-0178`).
    kept: BTreeMap<PathBuf, Vec<Kept>>,
    /// Whether the console already says that IDECraft evaluates breakpoints.
    noted: bool,
    /// js-debug's process tree (`SPEC-0168` behaviour 2); `None` for other adapters.
    js: Option<JsTree>,
    /// A `pause` was asked for and its stop has not come yet (`SPEC-0194` behaviour 1).
    pausing: bool,
    /// The line breakpoints asked for per file, put back after Run to Cursor.
    requested: BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
    /// The file that holds Run to Cursor's added breakpoint until the next stop (`SPEC-0194`
    /// behaviour 2).
    run_to: Option<PathBuf>,
    /// The line breakpoints taken away for Force Step Over or Force Run to Cursor, put back at
    /// the next stop (`SPEC-0091` v11 behaviour 20).
    silenced: Option<BTreeMap<PathBuf, Vec<SourceBreakpoint>>>,
    /// References of variables the adapter marked lazy (java-debug: objects with their own
    /// `toString`), each with the reference of its resolved value once fetched, else 0; cleared
    /// on resume, when references become invalid.
    lazy: HashMap<i64, i64>,
    /// The adapter's id (`java`, `debugpy`, …), for what differs per language (`SPEC-0091` v12).
    adapter_id: String,
    /// java-debug: every thread was suspended for a breakpoint or Pause All, so Resume resumes
    /// them all (`SPEC-0127` v3 behaviour 6).
    all_paused: bool,
    /// Breakpoints removed after their hit (`path`, 1-based line), for the window.
    removed: Vec<(PathBuf, u64)>,
    /// Whether the console already says that debugpy always stops every thread.
    suspend_noted: bool,
    /// kotlin-debug-adapter: the paths naming classes that carry the breakpoints of a file
    /// (`SPEC-0132` v5 behaviour 4b).
    class_sources: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// kotlin-debug-adapter: the requested lines of a file whose class cannot be named to it or
    /// whose class path it did not take, each with the reason.
    unbindable: BTreeMap<PathBuf, Vec<(u64, &'static str)>>,
    /// kotlin-debug-adapter: class paths for inner classes the adapter did not take, said once.
    refused_sources: Vec<PathBuf>,
    /// kotlin-debug-adapter: the program was launched and nothing came from it yet, so a stop
    /// from before the adapter listened may be lost (`SPEC-0132` v6 behaviour 4c).
    unreported: bool,
    /// The placed method breakpoints by the adapter's id: (watches entry, watches exit)
    /// (`SPEC-0146` v6 behaviour 13).
    method_watch: HashMap<i64, (bool, bool)>,
    /// Threads whose next Resume steps out first, to stop at the method's exit.
    exit_on_resume: std::collections::HashSet<i64>,
    /// Threads stepping out for a method exit; their next stop is reported as `method exit`.
    exiting: std::collections::HashSet<i64>,
}

/// The js-debug sessions around the one being debugged: a child process asks for a session of its
/// own, which then becomes the debugged one until it ends (`SPEC-0168` behaviour 2).
struct JsTree {
    connect: crate::DapConnect,
    breakpoints: BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
    special: SpecialBreakpoints,
    /// The launching connection first, then the processes whose children are being debugged.
    outer: Vec<DapClient>,
}

impl DebugSession {
    /// Starts `adapter`, initialises it, launches with `launch` arguments and sets `breakpoints`
    /// (file → lines) before the program runs. Returns the session and the placed breakpoints.
    pub fn launch(
        adapter: &[String],
        adapter_id: &str,
        launch: Value,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        Self::launch_on(
            DapClient::start(adapter)?,
            adapter_id,
            launch,
            breakpoints,
            special,
        )
    }

    /// Like [`DebugSession::launch`], on an adapter already connected (`SPEC-0097` behaviour 6).
    pub fn launch_on(
        client: DapClient,
        adapter_id: &str,
        launch: Value,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        Self::start_on(client, adapter_id, "launch", launch, breakpoints, special)
    }

    /// Starts `adapter` and attaches it with `attach` arguments to a program that already runs
    /// (`SPEC-0147` behaviour 3); `disconnect` then detaches instead of ending the program.
    pub fn attach(
        adapter: &[String],
        adapter_id: &str,
        attach: Value,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        Self::attach_on(
            DapClient::start(adapter)?,
            adapter_id,
            attach,
            breakpoints,
            special,
        )
    }

    /// Like [`DebugSession::attach`], on an adapter already connected (java-debug in jdtls).
    pub fn attach_on(
        client: DapClient,
        adapter_id: &str,
        attach: Value,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        Self::start_on(client, adapter_id, "attach", attach, breakpoints, special)
    }

    /// Whether the session attached to a program that ran before.
    pub fn attached(&self) -> bool {
        self.attached
    }

    /// The id of the debug adapter the session talks to, e.g. `lldb-dap` or `debugpy`
    /// (`SPEC-0054` v5 behaviour 10).
    pub fn adapter_id(&self) -> &str {
        &self.adapter_id
    }

    /// Debugs a Node.js program with js-debug listening on `address` (`SPEC-0039` v5 behaviour
    /// 10): the first connection launches, js-debug asks for a child session with
    /// `startDebugging`, and the child session on a second connection is the one that stops at
    /// breakpoints. `keep` (the server process) lives as long as the session. `request` is
    /// `launch` or `attach` (`SPEC-0147`).
    pub fn launch_js(
        address: &str,
        request: &str,
        launch: Value,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
        keep: Box<dyn Send>,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        let address = address.to_owned();
        Self::launch_js_on(
            std::sync::Arc::new(move || DapClient::connect(&address, None)),
            request,
            launch,
            breakpoints,
            special,
            keep,
        )
    }

    /// Like [`DebugSession::launch_js`], with connections made by `connect` (the backend's
    /// session stream, `SPEC-0322` behaviour 2).
    pub fn launch_js_on(
        connect: crate::DapConnect,
        request: &str,
        launch: Value,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
        keep: Box<dyn Send>,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        let mut parent = connect()?;
        parent.keep_also(keep);
        parent.request("initialize", initialize_arguments("pwa-node"), REQUEST)?;
        let launch_seq = parent.send(request, launch)?;
        parent.wait_event(&["initialized"], REQUEST)?;
        parent.request("configurationDone", json!({}), REQUEST)?;
        parent.wait_response(launch_seq, REQUEST)?;
        let start = parent.wait_start_debugging(REQUEST)?;
        let (mut session, placed) =
            Self::js_child(&connect, &start, request, breakpoints, special)?;
        session.js = Some(JsTree {
            connect,
            breakpoints: breakpoints.clone(),
            special: special.clone(),
            outer: vec![parent],
        });
        Ok((session, placed))
    }

    /// The session js-debug asked for with `start` (`startDebugging` arguments), on a connection
    /// of its own.
    fn js_child(
        connect: &crate::DapConnect,
        start: &Value,
        request: &str,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        let request = start["request"].as_str().unwrap_or(request).to_owned();
        let child = connect()?;
        Self::start_on(
            child,
            "pwa-node",
            &request,
            start["configuration"].clone(),
            breakpoints,
            special,
        )
    }

    /// Debugs the newest child process js-debug reported, if any (`SPEC-0168` behaviour 2): the
    /// current session moves to the outer ones.
    fn follow_children(&mut self) -> Result<(), DapError> {
        let Some(js) = &mut self.js else {
            return Ok(());
        };
        let mut start = self.client.take_start();
        for outer in js.outer.iter_mut().rev() {
            outer.drain(Duration::ZERO);
            if start.is_none() {
                start = outer.take_start();
            }
        }
        let Some(start) = start else {
            return Ok(());
        };
        let (connect, breakpoints, special) = (
            js.connect.clone(),
            js.breakpoints.clone(),
            js.special.clone(),
        );
        let (child, placed) = Self::js_child(&connect, &start, "launch", &breakpoints, &special)?;
        let previous = std::mem::replace(&mut self.client, child.client);
        self.capabilities = child.capabilities;
        self.placed = placed;
        self.kept = child.kept;
        if let Some(js) = &mut self.js {
            js.outer.push(previous);
        }
        Ok(())
    }

    /// When a followed child process ended: debugs its parent again and returns true; false when
    /// the launched program itself ended.
    fn back_to_parent(&mut self) -> bool {
        if self.js.as_ref().is_none_or(|js| js.outer.len() < 2) {
            return false;
        }
        // The ended child's last output first.
        self.client.drain(Duration::from_millis(100));
        self.collect_output();
        let Some(js) = &mut self.js else {
            return false;
        };
        let parent = js.outer.pop().expect("two outer sessions");
        self.client = parent;
        true
    }

    /// Initialises `client`, sends `request` (`launch` or `attach`) with `arguments` and sets
    /// `breakpoints` before the program runs.
    fn start_on(
        mut client: DapClient,
        adapter_id: &str,
        request: &str,
        launch: Value,
        breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
        special: &SpecialBreakpoints,
    ) -> Result<(Self, BTreeMap<PathBuf, Vec<Breakpoint>>), DapError> {
        let capabilities =
            client.request("initialize", initialize_arguments(adapter_id), REQUEST)?;
        let main_class = launch["mainClass"].as_str().map(str::to_owned);
        // kotlin-debug-adapter starts the program before it is ready for it: the program waits
        // before its main class until `launch` is answered (`SPEC-0132` v6 behaviour 4c). Not
        // for an adapter on another machine, which has no use for a file here.
        let kotlin = matches!(adapter_id, "kotlin" | "kotlin-debug-adapter");
        let gate = main_class
            .as_deref()
            .filter(|_| kotlin && request == "launch" && !client.remote())
            .and_then(crate::kotlin_gate::prepare);
        let mut launch = launch;
        if let Some((argument, _)) = &gate {
            let own = launch["vmArguments"].as_str().unwrap_or_default();
            launch["vmArguments"] = json!(format!("{own} {argument}").trim());
        }
        // Some adapters answer `launch` only after `configurationDone`.
        let launch_seq = client.send(request, launch)?;
        // Delve builds the program before it is ready for breakpoints (`SPEC-0039` v7 behaviour
        // 11); a launch that fails before that is the adapter's error, not a timeout.
        let delve = matches!(adapter_id, "dlv" | "go");
        let ready = if delve { LAUNCH } else { REQUEST };
        client.wait_event_unless_failed(&["initialized"], launch_seq, ready)?;
        // What Delve said while building is a note of the debugger, not the program's output.
        let building: String = if delve {
            client
                .take_events("output")
                .iter()
                .filter_map(|e| e["body"]["output"].as_str())
                .collect()
        } else {
            String::new()
        };
        let mut session = Self {
            client,
            output: String::new(),
            console: building.clone(),
            transcript: building,
            capabilities,
            placed: BTreeMap::new(),
            rebound: Default::default(),
            attached: request == "attach",
            kept: BTreeMap::new(),
            noted: false,
            js: None,
            pausing: false,
            requested: BTreeMap::new(),
            run_to: None,
            silenced: None,
            lazy: HashMap::new(),
            adapter_id: adapter_id.to_owned(),
            all_paused: false,
            removed: Vec::new(),
            suspend_noted: false,
            class_sources: BTreeMap::new(),
            unbindable: BTreeMap::new(),
            refused_sources: Vec::new(),
            unreported: false,
            method_watch: HashMap::new(),
            exit_on_resume: Default::default(),
            exiting: Default::default(),
        };
        session.report_condition_errors();
        let mut placed = BTreeMap::new();
        let mut order: Vec<&PathBuf> = breakpoints.keys().collect();
        // kotlin-debug-adapter registers breakpoints in the order they were sent, while the
        // program already starts: the file of the main class, whose code runs first, goes
        // first (`SPEC-0132` v6 behaviour 4c).
        if session.kotlin_adapter()
            && let Some(main) = &main_class
        {
            order.sort_by_key(|path| {
                !std::fs::read_to_string(path)
                    .is_ok_and(|text| crate::kotlin_classes::declares(path, &text, main))
            });
        }
        for path in order {
            placed.insert(
                path.clone(),
                session.set_breakpoints(path, &breakpoints[path])?,
            );
        }
        // What the adapter cannot do is said in the output; the line breakpoints still work.
        if !special.is_empty()
            && let Err(e) = session.set_special_breakpoints(special)
        {
            session.output.push_str(&format!("[IDECraft] {e}\n"));
        }
        // kotlin-debug-adapter never answers `configurationDone`; `launch`'s answer is what counts.
        let done = session.client.send("configurationDone", json!({}))?;
        session.client.wait_response(launch_seq, LAUNCH)?;
        // The adapter listens now: the program may go on.
        drop(gate);
        match session.client.wait_response(done, Duration::from_secs(2)) {
            Ok(_) | Err(DapError::Timeout) => {}
            Err(e) => return Err(e),
        }
        session.unreported = request == "launch" && session.kotlin_adapter();
        Ok((session, placed))
    }

    fn kotlin_adapter(&self) -> bool {
        matches!(self.adapter_id.as_str(), "kotlin" | "kotlin-debug-adapter")
    }

    /// debugpy drops a breakpoint condition that fails with a `NameError` without a word and
    /// goes on, and goes on after any other failure: its own setting (`setDebuggerProperty`,
    /// a request of debugpy beside the protocol) makes it say every failure in the console
    /// and stop at that hit, as IDECraft does for the conditions it evaluates itself
    /// (`SPEC-0039` v6 behaviour 3a, `SPEC-0178` behaviour 5). An adapter version that does
    /// not know the request keeps its own way.
    fn report_condition_errors(&mut self) {
        if self.lang() != Lang::Python {
            return;
        }
        let _ = self.client.request(
            "setDebuggerProperty",
            json!({
                "skipPrintBreakpointException": [],
                "skipSuspendOnBreakpointException": [],
            }),
            Duration::from_secs(5),
        );
    }

    /// The adapter's capabilities from `initialize`.
    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

    /// Replaces the line breakpoints of `path` (behaviour 3). What the adapter cannot evaluate is
    /// kept back and evaluated at each hit (`SPEC-0178` behaviour 1).
    pub fn set_breakpoints(
        &mut self,
        path: &Path,
        breakpoints: &[SourceBreakpoint],
    ) -> Result<Vec<Breakpoint>, DapError> {
        self.requested
            .insert(path.to_path_buf(), breakpoints.to_vec());
        let can = |flag: &str| self.capabilities[flag] == true;
        let (conditions, hit_conditions, logs) = (
            can("supportsConditionalBreakpoints"),
            can("supportsHitConditionalBreakpoints"),
            can("supportsLogPoints"),
        );
        let mut list = Vec::new();
        let mut kept = Vec::new();
        for b in breakpoints {
            let mut v = json!({"line": b.line});
            let mut k = Kept {
                id: None,
                line: b.line,
                condition: None,
                hit_condition: None,
                log_message: None,
                hits: 0,
                rules: b.rules.clone(),
                master: b
                    .rules
                    .depends_on
                    .as_deref()
                    .and_then(breakpoint_place)
                    .map(|(p, l)| (canonical(&p), l)),
                armed: false,
            };
            if let Some(c) = &b.condition {
                if conditions {
                    v["condition"] = json!(c);
                } else {
                    k.condition = Some(c.clone());
                }
            }
            if let Some(h) = &b.hit_condition {
                if hit_conditions {
                    v["hitCondition"] = json!(h);
                } else {
                    k.hit_condition = Some(HitCondition::parse(h).ok_or_else(|| {
                        DapError::Failed {
                            command: "setBreakpoints".into(),
                            message: format!(
                                "hit condition `{h}` is not understood (use N, ==N, >N, >=N, <N, <=N or %N)"
                            ),
                        }
                    })?);
                }
            }
            if let Some(m) = &b.log_message {
                if logs {
                    v["logMessage"] = json!(m);
                } else {
                    k.log_message = Some(m.clone());
                }
            }
            list.push(v);
            kept.push(k);
        }
        let body = self.client.request(
            "setBreakpoints",
            // The file name too: some adapters (kotlin-debug-adapter) require it.
            json!({
                "source": {
                    "name": path.file_name().map(|n| n.to_string_lossy().into_owned()),
                    "path": path,
                },
                "breakpoints": list,
            }),
            REQUEST,
        )?;
        let unbindable = self.set_class_sources(path, &list)?;
        let reasons = self.unbindable.get(path).cloned().unwrap_or_default();
        let why_unbindable = |line: u64| {
            reasons
                .iter()
                .find(|(l, _)| *l == line)
                .map(|(_, why)| (*why).to_owned())
        };
        let placed: Vec<Breakpoint> = body["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .zip(&mut kept)
            .map(|(b, k)| {
                let requested = k.line;
                k.id = b["id"].as_i64();
                // A line whose class the adapter cannot be told is not set, whatever it says.
                let bound = !unbindable.contains(&k.line);
                k.line = b["line"].as_u64().unwrap_or(k.line);
                let client = k.client_side();
                Breakpoint {
                    id: k.id,
                    verified: bound && b["verified"].as_bool().unwrap_or(false),
                    line: Some(k.line),
                    requested_line: Some(requested),
                    // IDECraft's reason for such a line, else the adapter's (SPEC-0132 v7
                    // behaviour 4e).
                    reason: if bound {
                        adapter_reason(b)
                    } else {
                        why_unbindable(requested)
                    },
                    evaluated_by: if client {
                        EvaluatedBy::IdeCraft
                    } else {
                        EvaluatedBy::Adapter
                    },
                }
            })
            .collect();
        kept.truncate(placed.len());
        let key = canonical(path);
        // Hit counters go on while a breakpoint's rule stays the same (behaviour 7).
        let old = self.kept.remove(&key).unwrap_or_default();
        // Kept as well: breakpoints with further options IDECraft applies at each hit (SPEC-0127
        // v3).
        let mut kept: Vec<Kept> = placed
            .iter()
            .zip(kept)
            .filter(|(b, k)| b.evaluated_by == EvaluatedBy::IdeCraft || !k.rules.is_empty())
            .map(|(_, mut k)| {
                if let Some(o) = old.iter().find(|o| o.same_rule(&k)) {
                    k.hits = o.hits;
                    k.armed = o.armed;
                }
                k
            })
            .collect();
        if !kept.is_empty() {
            if !self.noted && kept.iter().any(Kept::client_side) {
                self.noted = true;
                self.note(CLIENT_SIDE_NOTE);
            }
            self.kept.insert(key, std::mem::take(&mut kept));
        }
        self.placed.insert(path.to_path_buf(), placed.clone());
        Ok(placed)
    }

    /// kotlin-debug-adapter looks for the code of a line only in the classes it derives from the
    /// path, so the lines of `path` are sent again under paths that name the classes declared in
    /// the file (`SPEC-0132` v5 behaviour 4b). `entries` are the breakpoints as sent for the
    /// file itself. Returns the lines whose class cannot be named; the console says so once per
    /// line.
    fn set_class_sources(&mut self, path: &Path, entries: &[Value]) -> Result<Vec<u64>, DapError> {
        if !self.kotlin_adapter() || path.extension().is_none_or(|e| e != "kt") {
            return Ok(Vec::new());
        }
        let lines: Vec<u64> = entries.iter().filter_map(|e| e["line"].as_u64()).collect();
        // An unreadable file is sent as before.
        let plan = std::fs::read_to_string(path)
            .map(|text| crate::kotlin_classes::class_sources(path, &text, &lines))
            .unwrap_or_default();
        let before = self.class_sources.remove(path).unwrap_or_default();
        let gone: Vec<(PathBuf, Vec<u64>)> = before
            .into_iter()
            .filter(|old| plan.sources.iter().all(|(new, _)| new != old))
            .map(|old| (old, Vec::new()))
            .collect();
        let mut sent = Vec::new();
        let mut unbound: Vec<(u64, &'static str)> = plan
            .unbindable
            .iter()
            .map(|l| (*l, UNBINDABLE_KOTLIN))
            .collect();
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        for (source, lines) in gone.into_iter().chain(plan.sources) {
            // With what the adapter evaluates itself (condition, hit count, log message).
            let list: Vec<&Value> = entries
                .iter()
                .filter(|e| e["line"].as_u64().is_some_and(|l| lines.contains(&l)))
                .collect();
            let answer = self.client.request(
                "setBreakpoints",
                json!({
                    "source": {
                        "name": source.file_name().map(|n| n.to_string_lossy().into_owned()),
                        "path": source,
                    },
                    "breakpoints": list,
                }),
                CLASS_SOURCE,
            );
            match answer {
                Ok(_) => {}
                // Refused (a name the adapter's system does not take as a path) or not answered
                // in time: only this class is affected, the session goes on.
                Err(DapError::Failed { .. } | DapError::Timeout) if lines.is_empty() => {}
                Err(DapError::Failed { .. } | DapError::Timeout) => {
                    let name = source.file_stem().unwrap_or_default().to_string_lossy();
                    match name.strip_suffix("$*") {
                        // The classes inside: the breakpoint still stops in the class itself.
                        Some(class) => {
                            if !self.refused_sources.contains(&source) {
                                self.refused_sources.push(source.clone());
                                self.note(&format!(
                                    "[IDECraft] kotlin-debug-adapter did not take the breakpoints of {file} for the classes inside `{class}`; they stop only in code of `{class}` itself and of inner classes that are loaded already.\n",
                                ));
                            }
                        }
                        None => unbound.extend(lines.iter().map(|l| (*l, UNTAKEN_KOTLIN))),
                    }
                }
                Err(e) => return Err(e),
            }
            if !lines.is_empty() {
                sent.push(source);
            }
        }
        if !sent.is_empty() {
            self.class_sources.insert(path.to_path_buf(), sent);
        }
        unbound.sort_unstable_by_key(|(line, _)| *line);
        unbound.dedup_by_key(|(line, _)| *line);
        let known = self.unbindable.remove(path).unwrap_or_default();
        for (line, why) in unbound.iter().filter(|u| !known.contains(u)) {
            self.note(&format!(
                "[IDECraft] The breakpoint at {file}:{line} cannot stop the program: {why}.\n",
            ));
        }
        let lines = unbound.iter().map(|(line, _)| *line).collect();
        if !unbound.is_empty() {
            self.unbindable.insert(path.to_path_buf(), unbound);
        }
        Ok(lines)
    }

    /// The stop or the end kotlin-debug-adapter did not report (`SPEC-0132` v6 behaviour 4c): it
    /// starts the program before it listens to it. A breakpoint reached in between suspends
    /// its thread and no `stopped` event follows; a program that ends in between stays
    /// suspended in its last moment and no `exited` event follows. Only suspended threads
    /// answer `stackTrace`: one whose top frame is on a requested breakpoint line is the lost
    /// stop; all of them suspended, with the JVM's `DestroyJavaVM` thread among them and none
    /// at a breakpoint, is the lost end.
    fn lost_stop(&mut self) -> Option<Value> {
        let until = Instant::now() + LOST_STOP_TOTAL;
        let threads = self
            .client
            .request("threads", json!({}), LOST_STOP_REQUEST)
            .ok()?;
        let threads = threads["threads"].as_array()?;
        let mut all_suspended = !threads.is_empty();
        for thread in threads {
            if Instant::now() >= until {
                return None;
            }
            let Some(id) = thread["id"].as_i64() else {
                continue;
            };
            let Ok(body) =
                self.client
                    .request("stackTrace", json!({"threadId": id}), LOST_STOP_REQUEST)
            else {
                all_suspended = false;
                continue;
            };
            let top = &body["stackFrames"][0];
            let (Some(path), Some(line)) = (top["source"]["path"].as_str(), top["line"].as_u64())
            else {
                continue;
            };
            let path = canonical(Path::new(path));
            let requested = self
                .requested
                .iter()
                .any(|(file, list)| list.iter().any(|b| b.line == line) && canonical(file) == path);
            if !requested {
                continue;
            }
            // A stop that is only late is taken as it comes.
            self.client.drain(Duration::from_millis(100));
            if self.client.has_event("stopped") {
                return None;
            }
            return Some(json!({
                "event": "stopped",
                "body": {"reason": "breakpoint", "threadId": id},
            }));
        }
        let ended = all_suspended && threads.iter().any(|t| t["name"] == "DestroyJavaVM");
        if !ended {
            return None;
        }
        // An end that is only late is taken as it comes.
        self.client.drain(Duration::from_millis(100));
        if self.client.has_event("exited") || self.client.has_event("terminated") {
            return None;
        }
        Some(json!({"event": "exited", "body": {}}))
    }

    /// Sets the exception and function breakpoints (`SPEC-0146` behaviours 2–4); refused with a
    /// reason when the adapter cannot do what is asked. Returns the function breakpoints as placed.
    pub fn set_special_breakpoints(
        &mut self,
        special: &SpecialBreakpoints,
    ) -> Result<Vec<Breakpoint>, DapError> {
        // The exceptions of this session's language and the method breakpoints that are on
        // (`SPEC-0146` v6 behaviours 13–15).
        let special = &special.for_adapter(&self.adapter_id);
        self.method_watch.clear();
        let filters = exception_filters(&self.capabilities, special)?;
        let condition = special.exception_condition.trim();
        let types = exception_types(condition);
        let classes = exception_types(&special.exception_classes);
        // java-debug takes the types and the classes from its settings, not from the request
        // (D30, SPEC-0146 v3).
        let settings = if self.lang() == Lang::Java {
            let (skip, allow): (Vec<String>, Vec<String>) =
                classes.iter().cloned().partition(|c| c.starts_with('-'));
            let skip: Vec<String> = skip
                .into_iter()
                .map(|c| c.trim_start_matches('-').to_owned())
                .collect();
            let mut filters = json!({"exceptionTypes": types});
            // Only when given: java-debug fails on empty class lists.
            if !allow.is_empty() {
                filters["allowClasses"] = json!(allow);
            }
            if !skip.is_empty() {
                filters["skipClasses"] = json!(skip);
            }
            self.client.update_settings(&json!({
                "exceptionFilters": filters,
                "exceptionFiltersUpdated": true,
            }))
        } else {
            None
        };
        let by_host = settings.is_some();
        let by_settings = match settings.or_else(|| self.client.set_exception_types(&types)) {
            Some(Err(message)) => {
                return Err(DapError::Failed {
                    command: "setExceptionBreakpoints".into(),
                    message,
                });
            }
            Some(Ok(())) => true,
            None => false,
        };
        if !classes.is_empty() && !by_host {
            return Err(DapError::Failed {
                command: "setExceptionBreakpoints".into(),
                message: "the debug adapter cannot limit exception breakpoints to classes".into(),
            });
        }
        let arguments = if condition.is_empty() || by_settings {
            json!({"filters": filters})
        } else if self.capabilities["supportsExceptionFilterOptions"] == true {
            json!({
                "filters": [],
                "filterOptions": filters
                    .iter()
                    .map(|f| json!({"filterId": f, "condition": condition}))
                    .collect::<Vec<_>>(),
            })
        } else {
            return Err(DapError::Failed {
                command: "setExceptionBreakpoints".into(),
                message: "the debug adapter cannot limit exception breakpoints to types".into(),
            });
        };
        if self.capabilities["exceptionBreakpointFilters"].is_array() {
            self.client
                .request("setExceptionBreakpoints", arguments, REQUEST)?;
        }
        // Field watchpoints are data breakpoints (SPEC-0146 v3).
        if self.capabilities["supportsDataBreakpoints"] == true {
            let list: Vec<Value> = special
                .fields
                .iter()
                .map(|f| {
                    let mut v = json!({
                        "dataId": java_method_name(&f.field),
                        "accessType": f.access_type(),
                    });
                    if let Some(c) = &f.condition {
                        v["condition"] = json!(c);
                    }
                    v
                })
                .collect();
            self.client
                .request("setDataBreakpoints", json!({"breakpoints": list}), REQUEST)?;
        } else if !special.fields.is_empty() {
            return Err(DapError::Failed {
                command: "setDataBreakpoints".into(),
                message: "the debug adapter cannot watch fields".into(),
            });
        }
        if special.functions.is_empty() {
            return Ok(Vec::new());
        }
        if self.capabilities["supportsFunctionBreakpoints"] != true {
            return Err(DapError::Failed {
                command: "setFunctionBreakpoints".into(),
                message: "the debug adapter does not support method or function breakpoints".into(),
            });
        }
        if special.functions.iter().any(|f| f.condition.is_some())
            && self.capabilities["supportsConditionalBreakpoints"] != true
        {
            return Err(DapError::Failed {
                command: "setFunctionBreakpoints".into(),
                message: "the debug adapter does not support conditional breakpoints".into(),
            });
        }
        let list: Vec<Value> = special
            .functions
            .iter()
            .map(|f| {
                let name = if by_settings {
                    java_method_name(&f.name)
                } else {
                    f.name.clone()
                };
                let mut v = json!({"name": name});
                if let Some(c) = &f.condition {
                    v["condition"] = json!(c);
                }
                v
            })
            .collect();
        let body = self.client.request(
            "setFunctionBreakpoints",
            json!({"breakpoints": list}),
            REQUEST,
        )?;
        let placed: Vec<Breakpoint> = body["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|b| Breakpoint {
                id: b["id"].as_i64(),
                verified: b["verified"].as_bool().unwrap_or(false),
                line: b["line"].as_u64(),
                requested_line: None,
                reason: adapter_reason(b),
                evaluated_by: EvaluatedBy::Adapter,
            })
            .collect();
        for (b, f) in placed.iter().zip(&special.functions) {
            if let Some(id) = b.id {
                self.method_watch.insert(id, (f.entry, f.exit));
            }
        }
        Ok(placed)
    }

    /// A stop at a method breakpoint that watches the exit (`SPEC-0146` v6 behaviour 13): one
    /// that watches only the exit steps out at once (`true`: the wait goes on); one that watches
    /// both stops here and steps out at the next Resume.
    fn method_exit(&mut self, body: &Value) -> Result<bool, DapError> {
        let thread = body["threadId"].as_i64().unwrap_or_default();
        let watch = body["hitBreakpointIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_i64)
            .find_map(|id| self.method_watch.get(&id).copied());
        match watch {
            Some((false, true)) => {
                self.exiting.insert(thread);
                self.resume("stepOut", thread)?;
                Ok(true)
            }
            Some((true, true)) => {
                self.exit_on_resume.insert(thread);
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    /// What the adapter says about the exception the program stopped on (`SPEC-0146`
    /// behaviour 5): type and message, if it can tell.
    pub fn exception_info(&mut self, thread_id: i64) -> Option<String> {
        if self.capabilities["supportsExceptionInfoRequest"] != true {
            return None;
        }
        let body = self
            .client
            .request("exceptionInfo", json!({"threadId": thread_id}), REQUEST)
            .ok()?;
        let id = body["exceptionId"].as_str().unwrap_or_default();
        let text = body["description"]
            .as_str()
            .or_else(|| body["details"]["message"].as_str())
            .unwrap_or_default();
        let joined = match (id.is_empty(), text.is_empty()) {
            (true, true) => return None,
            (false, true) => id.to_owned(),
            (true, false) => text.to_owned(),
            (false, false) if text.contains(id) => text.to_owned(),
            (false, false) => format!("{id}: {text}"),
        };
        Some(joined)
    }

    /// Applies `breakpoint` events (an adapter verifies pending breakpoints once code is loaded).
    fn apply_breakpoint_events(&mut self) {
        for event in self.client.take_events("breakpoint") {
            let b = &event["body"]["breakpoint"];
            let Some(id) = b["id"].as_i64() else { continue };
            for (path, list) in &mut self.placed {
                for bp in list.iter_mut().filter(|bp| bp.id == Some(id)) {
                    let before = bp.clone();
                    if let Some(v) = b["verified"].as_bool() {
                        bp.verified = v;
                        // A message belongs to the state it came with (SPEC-0127 v5).
                        bp.reason = adapter_reason(b);
                    }
                    if let Some(l) = b["line"].as_u64() {
                        bp.line = Some(l);
                    }
                    if *bp != before {
                        self.rebound.insert(path.clone());
                    }
                }
            }
            if let Some(l) = b["line"].as_u64() {
                for k in self.kept.values_mut().flatten() {
                    if k.id == Some(id) {
                        k.line = l;
                    }
                }
            }
        }
    }

    /// The files whose placed breakpoints changed through `breakpoint` events since the last call,
    /// with their breakpoints now (`SPEC-0127` v5 behaviour 14: the window follows binding).
    pub fn take_rebound(&mut self) -> Vec<(PathBuf, Vec<Breakpoint>)> {
        self.client.drain(Duration::ZERO);
        self.apply_breakpoint_events();
        std::mem::take(&mut self.rebound)
            .into_iter()
            .filter_map(|path| {
                let list = self.placed.get(&path)?.clone();
                Some((path, list))
            })
            .collect()
    }

    /// The line breakpoints of every file as the adapter has them now: its answers to
    /// `setBreakpoints` with every `breakpoint` event since applied (`SPEC-0127` v6 behaviour
    /// 15: js-debug answers every breakpoint as not bound and binds it once the script is
    /// loaded). Files whose breakpoints were all taken away are left out.
    pub fn placed(&mut self) -> BTreeMap<PathBuf, Vec<Breakpoint>> {
        self.client.drain(Duration::ZERO);
        self.apply_breakpoint_events();
        self.placed
            .iter()
            .filter(|(_, list)| !list.is_empty())
            .map(|(path, list)| (path.clone(), list.clone()))
            .collect()
    }

    /// Breakpoints still not bound (behaviour 8). Meaningful once the program has loaded, i.e. after
    /// the first stop or the end.
    pub fn pending_breakpoints(&mut self) -> Vec<PendingBreakpoint> {
        self.client.drain(Duration::from_millis(50));
        self.apply_breakpoint_events();
        let mut out = Vec::new();
        for (path, bps) in &self.placed {
            for bp in bps.iter().filter(|b| !b.verified) {
                let unbindable = self.unbindable.get(path).and_then(|lines| {
                    lines
                        .iter()
                        .find(|(line, _)| bp.line == Some(*line))
                        .map(|(_, why)| *why)
                });
                let hint = match unbindable {
                    Some(why) => why.to_owned(),
                    None => unbound_hint(path),
                };
                out.push(PendingBreakpoint {
                    path: path.clone(),
                    line: bp.line,
                    reason: bp.reason.clone(),
                    hint,
                });
            }
        }
        out
    }

    /// Waits for the program to stop or end. Hits of breakpoints whose condition, hit count or log
    /// message IDECraft evaluates are handled here and do not end the wait unless they stop
    /// (`SPEC-0178` behaviours 2–7).
    pub fn wait(&mut self, timeout: Duration) -> Result<Event, DapError> {
        let deadline = Instant::now() + timeout;
        loop {
            self.follow_children()?;
            // In slices while child processes may appear (`SPEC-0168` behaviour 2).
            let left = deadline.saturating_duration_since(Instant::now());
            let slice = if self.js.is_some() {
                left.min(Duration::from_millis(100))
            } else if self.unreported {
                left.min(LOST_STOP_AFTER)
            } else {
                left
            };
            let event = match self
                .client
                .wait_event(&["stopped", "exited", "terminated"], slice)
            {
                Ok(e) if e["event"] != "stopped" && self.back_to_parent() => continue,
                Ok(e) => e,
                // Nothing since the launch: a stop from before the adapter listened is asked
                // for, once.
                Err(DapError::Timeout) if std::mem::take(&mut self.unreported) => {
                    match self.lost_stop() {
                        Some(stop) => stop,
                        None => continue,
                    }
                }
                Err(DapError::Timeout) if !left.is_zero() => continue,
                Err(e) => return Err(e),
            };
            self.unreported = false;
            self.collect_output();
            let mut event = event;
            if event["event"] == "stopped" {
                let thread = event["body"]["threadId"].as_i64().unwrap_or_default();
                // The stop after stepping out for a method exit (`SPEC-0146` v6 behaviour 13).
                if self.exiting.remove(&thread) {
                    event["body"]["reason"] = json!("method exit");
                    event["body"]["description"] = json!("method exit");
                    return self.finish_wait(&event);
                }
                if self.method_exit(&event["body"])? {
                    continue;
                }
            }
            if event["event"] == "stopped" && self.passes(&event["body"])? {
                continue;
            }
            return self.finish_wait(&event);
        }
    }

    /// Evaluates a stop at a breakpoint kept by IDECraft; `true` when the program was continued.
    fn passes(&mut self, body: &Value) -> Result<bool, DapError> {
        if self.kept.is_empty() {
            return Ok(false);
        }
        let by_breakpoint = body["reason"] == "breakpoint"
            || body["hitBreakpointIds"]
                .as_array()
                .is_some_and(|ids| !ids.is_empty());
        if !by_breakpoint {
            return Ok(false);
        }
        let thread = body["threadId"].as_i64().unwrap_or_default();
        let frames = self.stack_trace(thread)?;
        let Some(top) = frames.first().cloned() else {
            return Ok(false);
        };
        let Some(path) = top.path.as_deref().map(canonical) else {
            return Ok(false);
        };
        let line = u64::try_from(top.line).unwrap_or_default();
        let Some(index) = self
            .kept
            .get(&path)
            .and_then(|list| list.iter().position(|k| k.line == line))
        else {
            // A plain breakpoint stops: the breakpoints waiting for it become active.
            self.arm(&path, line);
            return Ok(false);
        };
        let rule = self.kept[&path][index].clone();
        let place = format!(
            "{}:{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            rule.line
        );
        // Waiting for another breakpoint: nothing happens yet (SPEC-0127 v3 behaviour 10).
        if rule.master.is_some() && !rule.armed {
            self.resume("continue", thread)?;
            return Ok(true);
        }
        // Instance, class and caller filters (behaviour 11).
        if !self.filters_allow(&rule.rules, &frames)? {
            self.resume("continue", thread)?;
            return Ok(true);
        }
        if let Some(condition) = &rule.condition {
            let failed = match self.condition(condition, top.id) {
                Ok(true) => None,
                Ok(false) => {
                    self.resume("continue", thread)?;
                    return Ok(true);
                }
                Err(why) => Some(why),
            };
            if let Some(why) = failed {
                self.note(&format!(
                    "[IDECraft] The condition `{condition}` at {place} could not be evaluated: {why}. Stopped there.\n"
                ));
                return Ok(false);
            }
        }
        let hits = {
            let k = &mut self.kept.get_mut(&path).expect("kept above")[index];
            k.hits += 1;
            k.hits
        };
        if rule.hit_condition.is_some_and(|h| !h.met(hits)) {
            self.resume("continue", thread)?;
            return Ok(true);
        }
        // From here the breakpoint counts as hit.
        self.arm(&path, line);
        if rule.master.is_some()
            && !rule.rules.depends_keep
            && let Some(k) = self.kept.get_mut(&path).and_then(|l| l.get_mut(index))
        {
            k.armed = false;
        }
        if let Some(message) = &rule.log_message {
            let text = interpolate(message, |expression| {
                self.value_text(expression, top.id)
                    .unwrap_or_else(|e| format!("<error: {e}>"))
            });
            self.note(&format!("{text}\n"));
        }
        // Messages, removal and which threads stop (SPEC-0127 v3 behaviours 6–9).
        if rule.rules.log_hit {
            self.note(&format!("Breakpoint hit: {place} in {}\n", top.name));
        }
        if rule.rules.log_stack {
            let stack: String = frames
                .iter()
                .map(|f| {
                    let file = f
                        .path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    format!("\tat {} ({file}:{})\n", f.name, f.line)
                })
                .collect();
            self.note(&format!("Stack at {place}:\n{stack}"));
        }
        if rule.rules.remove_once_hit {
            self.remove_breakpoint(&path, line)?;
        }
        if rule.log_message.is_some() || rule.rules.suspend == Some(Suspend::None) {
            self.resume("continue", thread)?;
            return Ok(true);
        }
        match rule.rules.suspend {
            Some(Suspend::All) => self.suspend_others(thread),
            Some(Suspend::Thread)
                if Lang::of_adapter(&self.adapter_id) != Lang::Java
                    && !std::mem::replace(&mut self.suspend_noted, true) =>
            {
                self.note("[IDECraft] This debug adapter stops every thread at a breakpoint; stopping only the thread that hit it is not possible.\n");
            }
            _ => {}
        }
        Ok(false)
    }

    /// Activates the breakpoints that wait for the one at `path` (resolved) and 1-based `line`
    /// (`SPEC-0127` v3 behaviour 10).
    fn arm(&mut self, path: &Path, line: u64) {
        for k in self.kept.values_mut().flatten() {
            if k.master
                .as_ref()
                .is_some_and(|(p, l)| p == path && *l == line)
            {
                k.armed = true;
            }
        }
    }

    /// Whether a stop with `frames` passes the instance, class and caller filters of `rules`
    /// (`SPEC-0127` v3 behaviour 11). The object is the top frame's `this` or `self`; its class
    /// is that variable's type, else the class part of the frame's name.
    fn filters_allow(
        &mut self,
        rules: &BreakpointRules,
        frames: &[Frame],
    ) -> Result<bool, DapError> {
        let Some(top) = frames.first() else {
            return Ok(true);
        };
        if let Some(caller) = &rules.caller_filter {
            let name = frames
                .get(1)
                .map(|f| extras::frame_method(&f.name).to_owned())
                .unwrap_or_default();
            if !extras::filter_allows(caller, &name) {
                return Ok(false);
            }
        }
        if rules.instance_filter.is_none() && rules.class_filter.is_none() {
            return Ok(true);
        }
        let object = match self.locals_reference(top.id)? {
            Some(r) => self
                .variables(r)?
                .into_iter()
                .find(|v| v.name == "this" || v.name == "self"),
            None => None,
        };
        if let Some(filter) = &rules.instance_filter {
            let id = object.as_ref().and_then(|o| object_id_of(&o.value));
            if !extras::instance_allows(filter, id.as_deref()) {
                return Ok(false);
            }
        }
        if let Some(filter) = &rules.class_filter {
            let class = object
                .as_ref()
                .and_then(|o| o.kind.clone())
                .filter(|k| !k.is_empty())
                .unwrap_or_else(|| extras::frame_class(&top.name).to_owned());
            if !extras::filter_allows(filter, &class) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Removes the line breakpoint at `path` (resolved) and 1-based `line` after its hit and
    /// remembers it for the window (`SPEC-0127` v3 behaviour 9).
    fn remove_breakpoint(&mut self, path: &Path, line: u64) -> Result<(), DapError> {
        let Some((file, list)) = self
            .requested
            .iter()
            .find(|(p, _)| canonical(p) == path)
            .map(|(p, l)| (p.clone(), l.clone()))
        else {
            return Ok(());
        };
        let rest: Vec<SourceBreakpoint> = list.into_iter().filter(|b| b.line != line).collect();
        self.set_breakpoints(&file, &rest)?;
        self.removed.push((file, line));
        Ok(())
    }

    /// Breakpoints removed after their hit since the last call (`SPEC-0127` v3 behaviour 9).
    pub fn take_removed(&mut self) -> Vec<(PathBuf, u64)> {
        std::mem::take(&mut self.removed)
    }

    /// The language the session debugs (`SPEC-0091` v12).
    pub fn lang(&self) -> Lang {
        Lang::of_adapter(&self.adapter_id)
    }

    /// A condition's truth value: the adapter's answer, else IDECraft's own evaluation (`SPEC-0178`
    /// behaviours 4 and 4a).
    fn condition(&mut self, condition: &str, frame: i64) -> Result<bool, String> {
        let adapter = match self.evaluate(condition, frame) {
            Ok(value) => match truth(&value) {
                Some(b) => return Ok(b),
                None => format!("gave `{value}`, not true or false"),
            },
            Err(e) => e.to_string(),
        };
        match expr::evaluate(condition, |path| self.lookup(path, frame)) {
            Ok(expr::Value::Bool(b)) => Ok(b),
            Ok(other) => Err(format!(
                "{adapter}; IDECraft's own evaluation gave `{}`, not true or false",
                other.text()
            )),
            Err(e) => Err(format!(
                "{adapter}; IDECraft cannot evaluate it either: {e}"
            )),
        }
    }

    /// A log message part as text: the adapter's answer, else IDECraft's own evaluation.
    fn value_text(&mut self, expression: &str, frame: i64) -> Result<String, String> {
        let adapter = match self.evaluate(expression, frame) {
            Ok(value) if !unanswered(&value) => return Ok(value),
            Ok(value) => format!("gave `{value}`"),
            Err(e) => e.to_string(),
        };
        expr::evaluate(expression, |path| self.lookup(path, frame))
            .map(|v| v.text())
            .map_err(|e| format!("{adapter}; {e}"))
    }

    /// The value of a name or field path: asked from the adapter, else looked up in the frame's
    /// variables and their children (`SPEC-0178` behaviour 4a).
    fn lookup(&mut self, path: &str, frame: i64) -> Result<expr::Value, String> {
        if let Ok(value) = self.evaluate(path, frame)
            && !unanswered(&value)
        {
            return Ok(expr::Value::shown(&value));
        }
        let mut parts = path.split('.');
        let first = parts.next().unwrap_or_default();
        let mut found = None;
        for (_, reference) in self.scopes(frame).map_err(|e| e.to_string())? {
            let variables = self.variables(reference).map_err(|e| e.to_string())?;
            if let Some(v) = variables.into_iter().find(|v| v.name == first) {
                found = Some(v);
                break;
            }
        }
        let mut variable = found.ok_or_else(|| format!("`{first}` is not a variable here"))?;
        for part in parts {
            let children = match variable.reference {
                0 => Vec::new(),
                r => self.variables(r).map_err(|e| e.to_string())?,
            };
            variable = children
                .into_iter()
                .find(|v| v.name == part)
                .ok_or_else(|| format!("`{path}`: no field `{part}`"))?;
        }
        Ok(expr::Value::shown(&variable.value))
    }

    fn finish_wait(&mut self, event: &Value) -> Result<Event, DapError> {
        Ok(match event["event"].as_str() {
            Some("stopped") => {
                let mut reason = event["body"]["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let thread_id = event["body"]["threadId"].as_i64().unwrap_or_default();
                // Xcode's lldb-dap reports the stop of a pause as the signal it sends
                // (`SPEC-0194` behaviour 1).
                if std::mem::take(&mut self.pausing)
                    && reason == "exception"
                    && self
                        .exception_info(thread_id)
                        .is_some_and(|i| i.contains("SIGSTOP"))
                {
                    reason = "pause".into();
                }
                // Run to Cursor's breakpoint goes at the next stop, wherever it is.
                if let Some(path) = self.run_to.take() {
                    let own = self.requested.get(&path).cloned().unwrap_or_default();
                    let _ = self.set_breakpoints(&path, &own);
                }
                // Breakpoints taken away by a forced step come back (v11 behaviour 20).
                if let Some(saved) = self.silenced.take() {
                    for (path, list) in saved {
                        let _ = self.set_breakpoints(&path, &list);
                    }
                }
                Event::Stopped(Stopped { reason, thread_id })
            }
            Some("exited") => Event::Exited(event["body"]["exitCode"].as_i64()),
            _ => {
                // `terminated` may come without or before `exited`.
                let code = self
                    .client
                    .wait_event(&["exited"], Duration::from_millis(500))
                    .ok()
                    .and_then(|e| e["body"]["exitCode"].as_i64());
                Event::Exited(code)
            }
        })
    }

    fn collect_output(&mut self) {
        self.apply_breakpoint_events();
        let mut events = Vec::new();
        if let Some(js) = &mut self.js {
            for outer in &mut js.outer {
                outer.drain(Duration::ZERO);
                events.extend(outer.take_events("output"));
            }
        }
        events.extend(self.client.take_events("output"));
        for event in events {
            let category = event["body"]["category"].as_str().unwrap_or("console");
            let text = event["body"]["output"].as_str().unwrap_or_default();
            // Delve sends a logpoint's message as program output that names its source line
            // (`SPEC-0039` v7 behaviour 11).
            let category = if category == "stdout"
                && event["body"]["source"].is_object()
                && matches!(self.adapter_id.as_str(), "dlv" | "go")
            {
                "console"
            } else {
                category
            };
            match category {
                "stdout" | "stderr" => self.output.push_str(text),
                // Logpoint messages and adapter notes.
                "console" | "important" => self.console.push_str(text),
                _ => continue,
            }
            self.transcript.push_str(text);
        }
        // java-debug says what it could not do — a breakpoint condition or a log message it
        // failed to evaluate — in an event of its own beside the protocol; it belongs to the
        // debug console like any other adapter note (`SPEC-0039` v6 behaviour 3a). Each on a
        // line of its own, also after a note that ended without a line break.
        for event in self.client.take_events("usernotification") {
            let Some(message) = event["body"]["message"]
                .as_str()
                .map(str::trim)
                .filter(|m| !m.is_empty())
            else {
                continue;
            };
            for text in [&mut self.console, &mut self.transcript] {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(message);
                text.push('\n');
            }
        }
    }

    /// Adds IDECraft's own console line after what the adapter sent before it, so a logpoint's
    /// message stands where the program was (D17: they came after the program's later output).
    fn note(&mut self, text: &str) {
        self.collect_output();
        self.console.push_str(text);
        self.transcript.push_str(text);
    }

    /// Adds a line of IDECraft's own to the debug console (`SPEC-0322` behaviour 4).
    pub fn add_console_note(&mut self, text: &str) {
        self.note(text);
    }

    /// Program output and debug console together, in the order they came.
    pub fn transcript(&mut self) -> &str {
        self.client.drain(Duration::from_millis(100));
        self.collect_output();
        &self.transcript
    }

    /// Debug console output (logpoint messages, adapter notes) received so far.
    pub fn console(&mut self) -> &str {
        self.client.drain(Duration::from_millis(100));
        self.collect_output();
        &self.console
    }

    /// Program output (stdout and stderr) received so far.
    pub fn output(&mut self) -> &str {
        self.client.drain(Duration::from_millis(100));
        self.collect_output();
        &self.output
    }

    /// Thread ids and names.
    pub fn threads(&mut self) -> Result<Vec<(i64, String)>, DapError> {
        let body = self.client.request("threads", json!({}), REQUEST)?;
        Ok(body["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                (
                    t["id"].as_i64().unwrap_or_default(),
                    t["name"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect())
    }

    /// Call stack of a thread, innermost first.
    pub fn stack_trace(&mut self, thread_id: i64) -> Result<Vec<Frame>, DapError> {
        let body = self
            .client
            .request("stackTrace", json!({"threadId": thread_id}), REQUEST)?;
        Ok(body["stackFrames"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| Frame {
                id: f["id"].as_i64().unwrap_or_default(),
                name: f["name"].as_str().unwrap_or_default().to_owned(),
                path: f["source"]["path"].as_str().map(PathBuf::from),
                line: f["line"].as_i64().unwrap_or_default(),
                deemphasized: f["presentationHint"] == "subtle"
                    || f["source"]["presentationHint"] == "deemphasize",
            })
            .collect())
    }

    /// Scopes of a frame as (name, variables reference).
    pub fn scopes(&mut self, frame_id: i64) -> Result<Vec<(String, i64)>, DapError> {
        let body = self
            .client
            .request("scopes", json!({"frameId": frame_id}), REQUEST)?;
        Ok(body["scopes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                (
                    s["name"].as_str().unwrap_or_default().to_owned(),
                    s["variablesReference"].as_i64().unwrap_or_default(),
                )
            })
            .collect())
    }

    /// Variables behind a reference (a scope or a structured value). The children of a variable
    /// the adapter marked lazy are those of its resolved value: DAP answers a lazy variable with
    /// one child, its value, and java-debug gives that value a reference of its own (a record's
    /// fields, `SPEC-0091` v9 behaviour 14).
    pub fn variables(&mut self, reference: i64) -> Result<Vec<Variable>, DapError> {
        let resolved = self.lazy.get(&reference).copied().filter(|&r| r != 0);
        let vars = self.fetch_variables(resolved.unwrap_or(reference))?;
        if resolved.is_none()
            && self.lazy.contains_key(&reference)
            && let Some(value) = lazy_value(&vars)
        {
            self.lazy.insert(reference, value);
            return self.fetch_variables(value);
        }
        Ok(vars)
    }

    fn fetch_variables(&mut self, reference: i64) -> Result<Vec<Variable>, DapError> {
        let body = self.client.request(
            "variables",
            json!({"variablesReference": reference}),
            REQUEST,
        )?;
        let (mut vars, lazy) = parse_variables(&body);
        // A lazy variable's value is its one child: java-debug puts an object's own `toString()`
        // text there. The first ones are resolved at once, so every row shows the text, not only
        // `this` (`SPEC-0091` v11 behaviour 17, OBS-0025).
        for (i, r) in lazy.into_iter().enumerate() {
            self.lazy.entry(r).or_insert(0);
            if i >= LAZY_RESOLVED {
                continue;
            }
            let Ok(child) =
                self.client
                    .request("variables", json!({"variablesReference": r}), REQUEST)
            else {
                continue;
            };
            let (children, _) = parse_variables(&child);
            // Only a value that is the same object with more to say replaces the row's; any
            // other lazy row keeps expanding as before (logical structure of collections).
            if let [value] = children.as_slice()
                && let Some(v) = vars.iter_mut().find(|v| v.reference == r)
                && value
                    .value
                    .strip_prefix(v.value.as_str())
                    .is_some_and(|rest| rest.starts_with(" \""))
            {
                v.value = value.value.clone();
                if value.reference != 0 {
                    self.lazy.insert(r, value.reference);
                }
            }
        }
        self.present(&mut vars, &values::counts(&body));
        Ok(vars)
    }

    /// Brings the variables of one answer of a JVM adapter into IDECraft's form (`SPEC-0091` v16
    /// behaviour 42): no `this` of a frame without a receiver, objects as `Type@id`, collections
    /// with their size, boxed values as their value. `counts` are the adapter's child counts in
    /// the variables' order. Sizes kotlin-debug-adapter does not give are read from the objects'
    /// fields ([`Self::summary`]); nothing is evaluated for them.
    fn present(&mut self, vars: &mut Vec<Variable>, counts: &[u64]) {
        if self.lang() != Lang::Java {
            return;
        }
        let deadline = Instant::now() + SUMMARY_TOTAL;
        let mut asked = 0;
        for (i, v) in vars.iter_mut().enumerate() {
            let need = values::needs(&v.value).filter(|_| v.reference != 0);
            v.value = values::present(&v.value, counts.get(i).copied().unwrap_or(0));
            let Some(need) = need else { continue };
            if asked == SUMMARY_ROWS {
                continue;
            }
            asked += 1;
            match self.summary(need, v.reference, deadline) {
                Some(Summary::Size(n)) => v.value.push_str(&values::size_text(n)),
                Some(Summary::Value(text)) => v.value = text,
                None => {}
            }
        }
        vars.retain(|v| !values::receiverless_this(v));
        for v in vars {
            v.kind = values::known_type(v.kind.take());
        }
    }

    /// The summary of the object behind `reference` from its fields: one `variables` request, one
    /// more for a set kept in a map. Each waits at most [`SUMMARY_REQUEST`] and not past
    /// `deadline`; a failed or late answer gives none.
    fn summary(&mut self, need: Need, reference: i64, deadline: Instant) -> Option<Summary> {
        let fields = self.fields(reference, deadline)?;
        match need {
            Need::Boxed => values::boxed_of_fields(&fields).map(Summary::Value),
            Need::Size => match values::size_of_fields(&fields) {
                Size::Known(n) => Some(Summary::Size(n)),
                Size::InMap(map) => match values::size_of_fields(&self.fields(map, deadline)?) {
                    Size::Known(n) => Some(Summary::Size(n)),
                    _ => None,
                },
                Size::Unknown => None,
            },
        }
    }

    /// The fields behind `reference` as the adapter sends them, for a summary.
    fn fields(&mut self, reference: i64, deadline: Instant) -> Option<Vec<Variable>> {
        let left = deadline.checked_duration_since(Instant::now())?;
        let body = self
            .client
            .request(
                "variables",
                json!({"variablesReference": reference}),
                left.min(SUMMARY_REQUEST),
            )
            .ok()?;
        Some(parse_variables(&body).0)
    }

    /// The value of an `evaluate` answer in the form of the variables (`SPEC-0091` v16 behaviour
    /// 42).
    fn presented(&mut self, body: &Value, name: &str, evaluate_name: Option<&str>) -> Variable {
        let value = Variable {
            name: name.to_owned(),
            value: body["result"].as_str().unwrap_or_default().to_owned(),
            kind: body["type"].as_str().map(str::to_owned),
            reference: body["variablesReference"].as_i64().unwrap_or_default(),
            evaluate_name: evaluate_name.map(str::to_owned),
        };
        // An evaluated `this` that is null stays: only the rows of a frame lose it.
        let mut one = vec![value.clone()];
        self.present(&mut one, &values::counts(body));
        one.pop().unwrap_or(value)
    }

    /// Evaluates `expression` in a frame.
    pub fn evaluate(&mut self, expression: &str, frame_id: i64) -> Result<String, DapError> {
        self.evaluate_within(expression, frame_id, REQUEST)
    }

    /// Evaluates `expression` in a frame, waiting at most `timeout` (`SPEC-0091` v9 behaviour 14);
    /// an answer that comes later is dropped.
    pub fn evaluate_within(
        &mut self,
        expression: &str,
        frame_id: i64,
        timeout: Duration,
    ) -> Result<String, DapError> {
        let body = self.client.request(
            "evaluate",
            json!({"expression": expression, "frameId": frame_id, "context": "watch"}),
            timeout,
        )?;
        Ok(self.presented(&body, "", None).value)
    }

    fn resume(&mut self, command: &str, thread_id: i64) -> Result<(), DapError> {
        // A stop reported twice (kotlin-debug-adapter) must not count as the next stop.
        self.client.discard_events("stopped");
        self.lazy.clear();
        // After every thread was suspended, Resume resumes them all (SPEC-0127 v3 behaviour 6).
        let command = if command == "continue" && std::mem::take(&mut self.all_paused) {
            "continueAll"
        } else {
            command
        };
        self.client
            .request(command, json!({"threadId": thread_id}), REQUEST)
            .map(|_| ())
    }

    /// java-debug: suspends the other threads too after a stop and drops the stop it reports for
    /// that (`SPEC-0127` v3 behaviour 6); other adapters already stop every thread.
    fn suspend_others(&mut self, thread_id: i64) {
        if self.lang() != Lang::Java || self.all_paused {
            return;
        }
        // Only the other threads: suspending the stopped one a second time (java-debug's
        // `pauseAll`) keeps it from running evaluations, which would then never answer.
        let done = match self
            .client
            .request("pauseOthers", json!({"threadId": thread_id}), REQUEST)
        {
            Ok(_) => Ok(()),
            Err(_) => self.threads().map(|threads| {
                for (id, _) in threads.into_iter().filter(|(id, _)| *id != thread_id) {
                    let _ = self
                        .client
                        .request("pause", json!({"threadId": id}), REQUEST);
                }
            }),
        };
        match done {
            Ok(()) => {
                // The stops reported for the others are not the program's next stop.
                self.client.drain(Duration::from_millis(300));
                self.client.discard_events("stopped");
                self.all_paused = true;
            }
            Err(e) => self.note(&format!(
                "[IDECraft] The other threads could not be suspended: {e}\n"
            )),
        }
    }

    /// Resume All Threads (`SPEC-0091` v12 behaviour 27): java-debug's `continueAll`, else
    /// `continue`, which resumes every thread in DAP.
    pub fn resume_all(&mut self, thread_id: i64) -> Result<(), DapError> {
        if self.lang() == Lang::Java {
            self.all_paused = true;
        }
        self.resume("continue", thread_id)
    }

    /// Pause All (`SPEC-0091` v12 behaviour 27) while the program runs: java-debug's `pauseAll`,
    /// else `pause` of the first thread (debugpy and lldb-dap stop every thread). The stop
    /// arrives like any other. While stopped, the other threads are suspended at once.
    pub fn pause_all(&mut self, thread_id: i64, stopped: bool) -> Result<(), DapError> {
        if stopped {
            if self.lang() == Lang::Java {
                self.suspend_others(thread_id);
            }
            return Ok(());
        }
        if self.lang() == Lang::Java {
            let thread_id = match thread_id {
                0 => self.threads()?.first().map(|(id, _)| *id).unwrap_or(1),
                id => id,
            };
            self.pausing = true;
            self.all_paused = true;
            return self
                .client
                .request("pauseAll", json!({"threadId": thread_id}), REQUEST)
                .map(|_| ());
        }
        self.pause(thread_id)
    }

    /// Force Return (`SPEC-0091` v12 behaviour 29): refused, DAP has no request for leaving a
    /// method early and neither java-debug nor debugpy offers one of its own.
    pub fn force_return(&mut self, _frame_id: i64) -> Result<(), DapError> {
        Err(DapError::Failed {
            command: "forceReturn".into(),
            message: "The debug adapter cannot return from a method early (the Debug Adapter Protocol has no such request)".into(),
        })
    }

    /// Throw Exception (`SPEC-0091` v12 behaviour 29): refused, an exception thrown by an
    /// evaluation stays inside the evaluation with both adapters.
    pub fn throw_exception(&mut self, _frame_id: i64, _exception: &str) -> Result<(), DapError> {
        Err(DapError::Failed {
            command: "throwException".into(),
            message: "The debug adapter cannot throw an exception in the program (an exception thrown by an evaluation stays inside the evaluation)".into(),
        })
    }

    /// Step Out of Code Block (`SPEC-0091` v12 behaviour 25): steps over until the top frame is
    /// on a line outside the innermost block around its line, the function returned or a
    /// breakpoint stopped it; at most [`BLOCK_STEPS`] steps. Without a block it is Step Out.
    /// Returns stopped; read the stop with [`Self::stopped_state`].
    pub fn step_out_of_block(&mut self, thread_id: i64) -> Result<(), DapError> {
        let start = self.stack_trace(thread_id)?;
        let Some(top) = start.first() else {
            return Err(DapError::Protocol("no frames".into()));
        };
        let text = top
            .path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let exec = usize::try_from(top.line - 1).unwrap_or(0);
        let Some((first, last)) = extras::block_lines(&lines, exec, self.lang() == Lang::Python)
        else {
            self.step_out(thread_id)?;
            return self.wait_stop().map(|_| ());
        };
        let depth = start.len();
        for _ in 0..BLOCK_STEPS {
            self.next(thread_id)?;
            let reason = self.wait_stop()?;
            let frames = self.stack_trace(thread_id)?;
            let line = frames
                .first()
                .and_then(|f| usize::try_from(f.line - 1).ok())
                .unwrap_or(0);
            if frames.len() != depth || reason.contains("breakpoint") || line < first || line > last
            {
                return Ok(());
            }
        }
        self.note(&format!(
            "[IDECraft] Step Out of Code Block: still in the block after {BLOCK_STEPS} steps\n"
        ));
        Ok(())
    }

    /// The program's threads with their state and group where the adapter can tell, and up to
    /// `max_frames` frames of each suspended thread (`SPEC-0091` v12 behaviours 26, 28). For Java
    /// the state and group come from the JVM, read by a fragment evaluated in `frame_id`.
    pub fn thread_dump(
        &mut self,
        frame_id: Option<i64>,
        max_frames: usize,
    ) -> Result<Vec<(ThreadInfo, Vec<Frame>)>, DapError> {
        let states = match (self.lang(), frame_id) {
            (Lang::Java, Some(frame)) => self
                .evaluate_within(extras::JAVA_THREAD_STATES, frame, Duration::from_secs(10))
                .map(|r| extras::parse_thread_states(&r))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for (id, name) in self.threads()? {
            let mut frames = self.stack_trace(id).unwrap_or_default();
            let suspended = !frames.is_empty();
            frames.truncate(max_frames);
            let name = extras::thread_name(&name).to_owned();
            let (state, group) = states
                .iter()
                .find(|(n, _, _)| *n == name)
                .map(|(_, s, g)| (s.clone(), g.clone()))
                .unwrap_or_default();
            out.push((
                ThreadInfo {
                    id,
                    name,
                    state,
                    group,
                    suspended,
                },
                frames,
            ));
        }
        Ok(out)
    }

    /// Evaluates `expression` in a frame in DAP `context` (`watch`, `repl`, `hover`) and returns
    /// the value with its type and reference, for Inspect, views and renderers (`SPEC-0091` v12
    /// behaviours 30–33).
    pub fn evaluate_value(
        &mut self,
        expression: &str,
        frame_id: i64,
        context: &str,
    ) -> Result<Variable, DapError> {
        let body = self.client.request(
            "evaluate",
            json!({"expression": expression, "frameId": frame_id, "context": context}),
            REQUEST,
        )?;
        Ok(self.presented(&body, expression, Some(expression)))
    }

    /// Evaluates a code fragment of several statements (`SPEC-0091` v12 behaviour 36): java-debug
    /// takes the statements as they are and gives the last one's value; for debugpy the
    /// statements run in the `repl` context and the last line is evaluated after them (empty
    /// value when it is no expression).
    pub fn evaluate_fragment(&mut self, code: &str, frame_id: i64) -> Result<String, DapError> {
        match self.lang() {
            Lang::Python => {
                let (statements, last) = extras::python_fragment(code);
                if let Some(s) = statements {
                    self.evaluate_value(&s, frame_id, "repl")?;
                }
                match last {
                    Some(l) => Ok(self.evaluate_value(&l, frame_id, "repl")?.value),
                    None => Ok(String::new()),
                }
            }
            _ => self.evaluate(code, frame_id),
        }
    }

    /// The objects that refer to the value of `expression` (`SPEC-0091` v12 behaviour 31), as one
    /// structured value: Python's `gc.get_referrers` without frames; refused for other adapters,
    /// which offer no such request.
    pub fn referring_objects(
        &mut self,
        expression: &str,
        frame_id: i64,
    ) -> Result<Variable, DapError> {
        if self.lang() != Lang::Python {
            return Err(DapError::Failed {
                command: "referringObjects".into(),
                message: "The debug adapter cannot list the objects that refer to a value".into(),
            });
        }
        let mut v = self.evaluate_value(
            &format!(
                "[r for r in __import__('gc').get_referrers({expression}) if not __import__('inspect').isframe(r)]"
            ),
            frame_id,
            "watch",
        )?;
        v.name = format!("referring to {expression}");
        Ok(v)
    }

    /// The data breakpoint id of field `name` in container `reference` (`dataBreakpointInfo`;
    /// java-debug: `Main$Counter#n`), for a field watchpoint (`SPEC-0146` v3); refused without
    /// data breakpoints.
    pub fn field_watch_id(&mut self, reference: i64, name: &str) -> Result<String, DapError> {
        supported(
            &self.capabilities,
            "supportsDataBreakpoints",
            "dataBreakpointInfo",
            "The debug adapter cannot watch fields",
        )?;
        let reference = match self.lazy.get(&reference) {
            Some(&r) if r != 0 => r,
            _ => reference,
        };
        let body = self.client.request(
            "dataBreakpointInfo",
            json!({"variablesReference": reference, "name": name}),
            REQUEST,
        )?;
        body["dataId"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| DapError::Failed {
                command: "dataBreakpointInfo".into(),
                message: body["description"]
                    .as_str()
                    .unwrap_or("this value cannot be watched")
                    .to_owned(),
            })
    }

    /// Hands debug settings to the adapter's host (java-debug through jdtls:
    /// `showStaticVariables`, `hotCodeReplace`, …); refused without one (`SPEC-0091` v12
    /// behaviour 34).
    pub fn update_settings(&mut self, settings: &Value) -> Result<(), DapError> {
        match self.client.update_settings(settings) {
            Some(Ok(())) => Ok(()),
            Some(Err(message)) => Err(DapError::Failed {
                command: "updateDebugSettings".into(),
                message,
            }),
            None => Err(DapError::Failed {
                command: "updateDebugSettings".into(),
                message: "the debug adapter takes no settings".into(),
            }),
        }
    }

    /// Hot swap, first half (`SPEC-0091` v12 behaviour 37): tells java-debug's host that `files`
    /// changed and waits up to `timeout` for the build of the changed classes. Refused for other
    /// adapters, which cannot reload code.
    pub fn build_changed(&mut self, files: &[PathBuf], timeout: Duration) -> Result<(), DapError> {
        self.hot_swap_supported()?;
        self.client.discard_events("hotcodereplace");
        match self.client.sources_changed(files) {
            Some(Ok(())) => {}
            Some(Err(message)) => {
                return Err(DapError::Failed {
                    command: "hot swap".into(),
                    message,
                });
            }
            None => return self.hot_swap_supported_by_host(),
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = match self.client.wait_event(&["hotcodereplace"], left) {
                Ok(e) => e,
                Err(DapError::Timeout) => {
                    return Err(DapError::Failed {
                        command: "hot swap".into(),
                        message: "the changed classes were not built in time".into(),
                    });
                }
                Err(e) => return Err(e),
            };
            match event["body"]["changeType"].as_str() {
                Some("BUILD_COMPLETE") => return Ok(()),
                Some("ERROR") => {
                    return Err(DapError::Failed {
                        command: "hot swap".into(),
                        message: event["body"]["message"]
                            .as_str()
                            .unwrap_or("the build failed")
                            .to_owned(),
                    });
                }
                _ => {}
            }
        }
    }

    /// Hot swap, second half: reloads the classes built since [`Self::build_changed`] into the
    /// running program (java-debug `redefineClasses`) and returns their names.
    pub fn reload_classes(&mut self) -> Result<Vec<String>, DapError> {
        self.hot_swap_supported()?;
        let body = self
            .client
            .request("redefineClasses", json!({}), Duration::from_secs(60))?;
        self.client.discard_events("hotcodereplace");
        if let Some(message) = body["errorMessage"].as_str().filter(|m| !m.is_empty()) {
            return Err(DapError::Failed {
                command: "redefineClasses".into(),
                message: message.to_owned(),
            });
        }
        Ok(body["changedClasses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().map(str::to_owned))
            .collect())
    }

    fn hot_swap_supported(&self) -> Result<(), DapError> {
        if self.adapter_id == "java" {
            Ok(())
        } else {
            Err(DapError::Failed {
                command: "hot swap".into(),
                message: "The debug adapter cannot reload changed code; restart the session".into(),
            })
        }
    }

    fn hot_swap_supported_by_host(&self) -> Result<(), DapError> {
        Err(DapError::Failed {
            command: "hot swap".into(),
            message: "nothing builds the changed classes for this session".into(),
        })
    }

    /// Continues a thread; after a stop at the entry of a method breakpoint that also watches
    /// the exit, steps out first (`SPEC-0146` v6 behaviour 13).
    pub fn resume_continue(&mut self, thread_id: i64) -> Result<(), DapError> {
        if self.exit_on_resume.remove(&thread_id) {
            self.exiting.insert(thread_id);
            return self.resume("stepOut", thread_id);
        }
        self.resume("continue", thread_id)
    }

    /// Steps over the current line.
    pub fn next(&mut self, thread_id: i64) -> Result<(), DapError> {
        self.resume("next", thread_id)
    }

    /// Steps into a call.
    pub fn step_in(&mut self, thread_id: i64) -> Result<(), DapError> {
        self.resume("stepIn", thread_id)
    }

    /// Steps out of the current function.
    pub fn step_out(&mut self, thread_id: i64) -> Result<(), DapError> {
        self.resume("stepOut", thread_id)
    }

    /// The calls Smart Step Into can enter from `frame_id`'s line: id and label (`SPEC-0091` v11
    /// behaviour 18); refused without `supportsStepInTargetsRequest`.
    pub fn step_in_targets(&mut self, frame_id: i64) -> Result<Vec<(i64, String)>, DapError> {
        supported(
            &self.capabilities,
            "supportsStepInTargetsRequest",
            "stepInTargets",
            "The debug adapter cannot list the calls of a line (Smart Step Into)",
        )?;
        let body = self
            .client
            .request("stepInTargets", json!({"frameId": frame_id}), REQUEST)?;
        Ok(step_targets(&body))
    }

    /// Steps into the call `target_id` of [`Self::step_in_targets`] and returns once the program
    /// stopped there (`SPEC-0091` v11 behaviour 18): IDECraft steps into the line's calls one by
    /// one and back out of each that is not the chosen method, until it is entered or the line is
    /// left, the same with every adapter whatever its own `stepIn` with `targetId` does. The stop
    /// is then read with [`Self::stopped_state`].
    pub fn step_in_target(&mut self, thread_id: i64, target_id: i64) -> Result<(), DapError> {
        let start = self.stack_trace(thread_id)?;
        let top = start.first().map(|f| f.id).unwrap_or_default();
        let label = self
            .step_in_targets(top)?
            .into_iter()
            .find(|(id, _)| *id == target_id)
            .map(|(_, l)| l)
            .ok_or_else(|| DapError::Protocol(format!("no call {target_id} on this line")))?;
        let name = target_method(&label);
        let depth = start.len();
        for _ in 0..SMART_STEPS {
            self.step_in(thread_id)?;
            self.wait_stop()?;
            let frames = self.stack_trace(thread_id)?;
            if frames.len() <= depth {
                // The line had no further call to enter: stopped on the next line.
                return Ok(());
            }
            if frames
                .first()
                .is_some_and(|f| frame_is_method(&f.name, name))
            {
                return Ok(());
            }
            self.step_out(thread_id)?;
            self.wait_stop()?;
            let frames = self.stack_trace(thread_id)?;
            let same_line = frames.len() == depth
                && frames.first().map(|f| f.line) == start.first().map(|f| f.line);
            if !same_line {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Waits for the stop of a step and returns its reason; an end of the program is an error
    /// here.
    fn wait_stop(&mut self) -> Result<String, DapError> {
        match self.wait(REQUEST)? {
            Event::Stopped(s) => Ok(s.reason),
            Event::Exited(_) => Err(DapError::Protocol("the program ended".into())),
        }
    }

    /// The state of thread `thread_id`, stopped now for `reason`, with at most `max_frames` frames
    /// and the top frame's locals (after [`Self::step_in_target`]).
    pub fn stopped_state(
        &mut self,
        reason: &str,
        thread_id: i64,
        max_frames: usize,
    ) -> Result<StopState, DapError> {
        let mut frames = self.stack_trace(thread_id)?;
        frames.truncate(max_frames);
        let locals = match frames.first() {
            Some(top) => {
                let scopes = self.scopes(top.id)?;
                let scope = scopes
                    .iter()
                    .find(|(n, _)| n.to_lowercase().starts_with("local"))
                    .or(scopes.first());
                match scope {
                    Some((_, r)) => self.variables(*r)?,
                    None => Vec::new(),
                }
            }
            None => Vec::new(),
        };
        Ok(StopState::Stopped {
            reason: reason.to_owned(),
            thread_id,
            frames,
            locals,
            pending_breakpoints: self.pending_breakpoints(),
        })
    }

    /// Runs frame `frame_id` again from its start (Reset Frame, `SPEC-0091` v11 behaviour 19); the
    /// adapter reports the stop like any other. Refused without `supportsRestartFrame`.
    pub fn restart_frame(&mut self, frame_id: i64) -> Result<(), DapError> {
        supported(
            &self.capabilities,
            "supportsRestartFrame",
            "restartFrame",
            "The debug adapter cannot reset frames",
        )?;
        self.client.discard_events("stopped");
        self.lazy.clear();
        self.client
            .request("restartFrame", json!({"frameId": frame_id}), REQUEST)
            .map(|_| ())
    }

    /// Takes every line breakpoint away until the next stop (`SPEC-0091` v11 behaviour 20).
    fn silence(&mut self) -> Result<(), DapError> {
        let saved = self.requested.clone();
        for path in saved.keys() {
            self.set_breakpoints(path, &[])?;
        }
        self.requested = saved.clone();
        self.silenced = Some(saved);
        Ok(())
    }

    /// Steps over the line without stopping at breakpoints on the way (Force Step Over).
    pub fn force_next(&mut self, thread_id: i64) -> Result<(), DapError> {
        self.silence()?;
        self.next(thread_id)
    }

    /// Runs to 1-based `line` of `path` without stopping at other breakpoints (Force Run to
    /// Cursor); like [`Self::run_to_line`] the program is not resumed when the line has no code.
    pub fn force_run_to_line(
        &mut self,
        thread_id: i64,
        path: &Path,
        line: u64,
    ) -> Result<(), DapError> {
        self.silence()?;
        let saved = self.silenced.take().unwrap_or_default();
        let placed = self.set_breakpoints(path, &[line.into()])?;
        if !placed.first().is_some_and(|b| b.verified) {
            for (p, list) in &saved {
                self.set_breakpoints(p, list)?;
            }
            if !saved.contains_key(path) {
                self.set_breakpoints(path, &[])?;
            }
            self.requested = saved;
            return Err(DapError::Failed {
                command: "force run to cursor".into(),
                message: format!("no code on line {line}; the program was not resumed"),
            });
        }
        // Every file's own breakpoints come back at the stop (none for a file without any).
        let mut restore = saved.clone();
        restore.entry(path.to_path_buf()).or_default();
        self.requested = saved;
        self.silenced = Some(restore);
        self.resume_continue(thread_id)
    }

    /// Completions for `text` with the caret at 1-based `column` in frame `frame_id` (`SPEC-0091`
    /// v11 behaviour 22); refused without `supportsCompletionsRequest`.
    pub fn completions(
        &mut self,
        frame_id: i64,
        text: &str,
        column: usize,
    ) -> Result<Vec<String>, DapError> {
        supported(
            &self.capabilities,
            "supportsCompletionsRequest",
            "completions",
            "The debug adapter offers no completion",
        )?;
        let body = self.client.request(
            "completions",
            json!({"frameId": frame_id, "text": text, "column": column}),
            REQUEST,
        )?;
        Ok(completion_texts(&body))
    }

    /// Pauses a running thread, or the first thread the adapter lists when `thread_id` is 0; the
    /// stop arrives like any other (`SPEC-0194` behaviour 1).
    pub fn pause(&mut self, thread_id: i64) -> Result<(), DapError> {
        let thread_id = match thread_id {
            0 => self
                .threads()?
                .first()
                .map(|(id, _)| *id)
                .ok_or_else(|| DapError::Protocol("the program has no threads".into()))?,
            id => id,
        };
        self.pausing = true;
        self.client
            .request("pause", json!({"threadId": thread_id}), REQUEST)
            .map(|_| ())
    }

    /// Runs a stopped thread to 1-based `line` of `path` (`SPEC-0194` behaviour 2): a breakpoint is
    /// added to the file's own until the next stop. When the adapter does not verify it (no code on
    /// that line) the file's breakpoints are put back and the program is not resumed.
    pub fn run_to_line(&mut self, thread_id: i64, path: &Path, line: u64) -> Result<(), DapError> {
        let own = self.requested.get(path).cloned().unwrap_or_default();
        let mut lines = own.clone();
        if !lines.iter().any(|b| b.line == line) {
            lines.push(line.into());
        }
        let placed = self.set_breakpoints(path, &lines)?;
        self.requested.insert(path.to_path_buf(), own.clone());
        let at = lines.iter().position(|b| b.line == line);
        if !at.and_then(|i| placed.get(i)).is_some_and(|b| b.verified) {
            self.set_breakpoints(path, &own)?;
            return Err(DapError::Failed {
                command: "run to cursor".into(),
                message: format!("no code on line {line}; the program was not resumed"),
            });
        }
        self.run_to = Some(path.to_path_buf());
        self.resume_continue(thread_id)
    }

    /// The variables reference of a frame's local variables: the scope whose name starts with
    /// "local", else the first scope.
    pub fn locals_reference(&mut self, frame_id: i64) -> Result<Option<i64>, DapError> {
        let scopes = self.scopes(frame_id)?;
        Ok(scopes
            .iter()
            .find(|(n, _)| n.to_lowercase().starts_with("local"))
            .or(scopes.first())
            .map(|(_, r)| *r))
    }

    /// Changes variable `name` in container `reference` (a scope or a structured value) to
    /// `value` and returns the value as the adapter now shows it; refused without
    /// `supportsSetVariable` (`SPEC-0194` behaviours 3, 4; IMP-0022).
    pub fn set_variable(
        &mut self,
        reference: i64,
        name: &str,
        value: &str,
    ) -> Result<String, DapError> {
        set_variable_supported(&self.capabilities)?;
        let reference = match self.lazy.get(&reference) {
            Some(&r) if r != 0 => r,
            _ => reference,
        };
        let body = self.client.request(
            "setVariable",
            json!({"variablesReference": reference, "name": name, "value": value}),
            REQUEST,
        )?;
        Ok(body["value"].as_str().unwrap_or_default().to_owned())
    }

    /// Sends a raw request (for adapter-specific features and tests).
    pub fn request(&mut self, command: &str, arguments: Value) -> Result<Value, DapError> {
        self.client.request(command, arguments, REQUEST)
    }

    /// Waits up to `timeout` and describes the result with at most `max_frames` frames.
    pub fn wait_state(
        &mut self,
        timeout: Duration,
        max_frames: usize,
    ) -> Result<StopState, DapError> {
        match self.wait(timeout) {
            Ok(Event::Stopped(s)) => {
                let mut frames = self.stack_trace(s.thread_id)?;
                frames.truncate(max_frames);
                let locals = match frames.first() {
                    Some(top) => {
                        let scopes = self.scopes(top.id)?;
                        // The first scope is the locals scope by convention ("Locals", "Local").
                        let scope = scopes
                            .iter()
                            .find(|(n, _)| n.to_lowercase().starts_with("local"))
                            .or(scopes.first());
                        match scope {
                            Some((_, r)) => self.variables(*r)?,
                            None => Vec::new(),
                        }
                    }
                    None => Vec::new(),
                };
                Ok(StopState::Stopped {
                    reason: s.reason,
                    thread_id: s.thread_id,
                    frames,
                    locals,
                    pending_breakpoints: self.pending_breakpoints(),
                })
            }
            Ok(Event::Exited(code)) => Ok(StopState::Exited {
                exit_code: code,
                pending_breakpoints: self.pending_breakpoints(),
            }),
            Err(DapError::Timeout) => Ok(StopState::Running),
            Err(e) => Err(e),
        }
    }

    /// Ends the program and the adapter; an attached program is detached and goes on running
    /// (`SPEC-0147` behaviour 5).
    pub fn disconnect(mut self) -> Result<(), DapError> {
        let ended = self.client.request(
            "disconnect",
            json!({"terminateDebuggee": !self.attached}),
            Duration::from_secs(5),
        );
        // The processes around a followed child (the package manager of a script) end too, and
        // so does the launching session: for a page it owns the browser, which only closes when
        // that session ends (`SPEC-0133` v3 behaviour 2).
        if let Some(js) = &mut self.js {
            for outer in js.outer.iter_mut().rev() {
                let _ = outer.request(
                    "disconnect",
                    json!({"terminateDebuggee": !self.attached}),
                    Duration::from_secs(2),
                );
            }
        }
        ended.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0146 v6 AK-v6-1: each adapter's session gets its language's exception settings,
    /// the shared ones where its language has none, and only the method breakpoints that are on.
    #[test]
    fn exceptions_per_language() {
        let special = SpecialBreakpoints {
            uncaught: true,
            exception_condition: "Boom".into(),
            functions: vec![
                FunctionBreakpoint {
                    name: "on".into(),
                    ..FunctionBreakpoint::default()
                },
                FunctionBreakpoint {
                    name: "off".into(),
                    disabled: true,
                    ..FunctionBreakpoint::default()
                },
            ],
            languages: [(
                "python".to_owned(),
                LanguageExceptions {
                    caught: true,
                    ..LanguageExceptions::default()
                },
            )]
            .into(),
            ..SpecialBreakpoints::default()
        };
        let python = special.for_adapter("debugpy");
        assert!(python.caught && !python.uncaught && python.exception_condition.is_empty());
        let java = special.for_adapter("java");
        assert!(!java.caught && java.uncaught && java.exception_condition == "Boom");
        assert!(java.languages.is_empty());
        let names: Vec<&str> = java.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["on"]);
        for (adapter, language) in [
            ("java", "java"),
            ("kotlin", "java"),
            ("debugpy", "python"),
            ("pwa-node", "javascript"),
            ("lldb-dap", "native"),
            ("dlv", "go"),
        ] {
            assert_eq!(exception_language(adapter), language, "{adapter}");
        }
        assert_eq!(
            special.method_at(Path::new("/p/A.java"), 3),
            None,
            "only breakpoints with their place"
        );
    }

    /// Smart Step Into lists the adapter's targets in its order; Reset Frame and completion are
    /// refused with a reason when the adapter lacks them (`SPEC-0091` v11 behaviours 18, 19, 22).
    #[test]
    fn step_targets_completions_and_refusals() {
        let body = json!({"targets": [
            {"id": 7, "label": "twice(int)", "line": 46},
            {"id": 8, "label": "square(int)"},
            {"label": "no id"},
        ]});
        assert_eq!(
            step_targets(&body),
            vec![(7, "twice(int)".to_owned()), (8, "square(int)".to_owned())]
        );
        let body = json!({"targets": [
            {"label": "get(Object)", "text": "get"},
            {"label": "getOrDefault(Object, V) : V"},
            {"label": "get(Object, int)", "text": "get"},
        ]});
        assert_eq!(completion_texts(&body), vec!["get", "getOrDefault"]);
        let caps = json!({"supportsRestartFrame": true});
        assert!(supported(&caps, "supportsRestartFrame", "restartFrame", "no").is_ok());
        let refused = supported(
            &caps,
            "supportsStepInTargetsRequest",
            "stepInTargets",
            "cannot",
        )
        .unwrap_err()
        .to_string();
        assert_eq!(refused, "stepInTargets failed: cannot");
        assert_eq!(target_method("square(3)"), "square");
        assert_eq!(target_method("add(twice(sum),square(3))"), "add");
        assert_eq!(target_method("item.total()"), "total");
        assert!(frame_is_method("Smart.square(int)", "square"));
        assert!(frame_is_method("square", "square"));
        assert!(frame_is_method("Main$Item.total()", "total"));
        assert!(!frame_is_method("Smart.squareRoot(int)", "square"));
    }

    /// java-debug marks a record with its own `toString` lazy; its one child is the value, whose
    /// reference holds the fields (window self-test "paused in a record's method").
    #[test]
    fn lazy_variables_are_marked_and_resolve_to_their_one_value() {
        let locals = json!({"variables": [
            {"name": "this", "value": "Dbg$Item@15", "type": "Dbg$Item",
             "variablesReference": 13, "presentationHint": {"lazy": true}},
            {"name": "n", "value": "21", "type": "int", "variablesReference": 0},
            {"name": "list", "value": "ArrayList@3", "variablesReference": 9},
        ]});
        let (vars, lazy) = parse_variables(&locals);
        assert_eq!(vars.len(), 3);
        assert_eq!(lazy, [13]);
        let value = json!({"variables": [
            {"name": "", "value": "Dbg$Item@15 \"Item[qty=3, unitPriceCents=25]\"",
             "type": "", "variablesReference": 14},
        ]});
        let (children, _) = parse_variables(&value);
        assert_eq!(lazy_value(&children), Some(14));
        assert_eq!(lazy_value(&vars), None);
        assert_eq!(
            lazy_value(&vars[1..2]),
            None,
            "a plain value has no children"
        );
    }

    #[test]
    fn set_value_needs_the_capability() {
        assert!(set_variable_supported(&json!({"supportsSetVariable": true})).is_ok());
        let refused = set_variable_supported(&json!({})).unwrap_err();
        assert!(
            refused.to_string().contains("cannot change values"),
            "{refused}"
        );
    }

    #[test]
    fn exception_filters_by_adapter() {
        let caps = |ids: &[&str]| json!({"exceptionBreakpointFilters": ids.iter().map(|i| json!({"filter": i, "label": i})).collect::<Vec<_>>()});
        let both = SpecialBreakpoints {
            caught: true,
            uncaught: true,
            ..SpecialBreakpoints::default()
        };
        let uncaught = SpecialBreakpoints {
            uncaught: true,
            ..SpecialBreakpoints::default()
        };
        // java-debug, debugpy, js-debug, lldb-dap.
        assert_eq!(
            exception_filters(&caps(&["caught", "uncaught"]), &both).unwrap(),
            ["caught", "uncaught"]
        );
        assert_eq!(
            exception_filters(&caps(&["raised", "uncaught", "userUnhandled"]), &uncaught).unwrap(),
            ["uncaught"],
            "one filter, so one exception stops once (v4)"
        );
        assert_eq!(
            exception_filters(&caps(&["raised", "userUnhandled"]), &uncaught).unwrap(),
            ["userUnhandled"]
        );
        assert_eq!(
            exception_filters(&caps(&["all", "uncaught"]), &both).unwrap(),
            ["all", "uncaught"]
        );
        assert_eq!(
            exception_filters(&caps(&["cpp_catch", "cpp_throw"]), &both).unwrap(),
            ["cpp_throw"]
        );
        assert!(exception_filters(&caps(&["cpp_catch", "cpp_throw"]), &uncaught).is_err());
        // Delve: a panic nothing recovers and a fatal error are "uncaught"; no stop on every
        // panic (`SPEC-0146` v5 behaviour 11).
        let delve = caps(&["unrecovered-panic", "runtime-fatal-throw"]);
        assert_eq!(
            exception_filters(&delve, &uncaught).unwrap(),
            ["unrecovered-panic", "runtime-fatal-throw"]
        );
        assert!(exception_filters(&delve, &both).is_err());
        assert!(exception_filters(&json!({}), &both).is_err());
        assert!(
            exception_filters(&json!({}), &SpecialBreakpoints::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn hit_conditions_and_truth_values() {
        let met = |text: &str| {
            let h = HitCondition::parse(text).unwrap();
            (1..=6).filter(|&n| h.met(n)).collect::<Vec<_>>()
        };
        assert_eq!(met("3"), [3]);
        assert_eq!(met(" == 3"), [3]);
        assert_eq!(met(">4"), [5, 6]);
        assert_eq!(met(">=4"), [4, 5, 6]);
        assert_eq!(met("<3"), [1, 2]);
        assert_eq!(met("<=2"), [1, 2]);
        assert_eq!(met("%2"), [2, 4, 6]);
        for bad in ["", "x", "%0", ">-1", "3 times"] {
            assert_eq!(HitCondition::parse(bad), None, "{bad}");
        }
        for yes in ["true", "True", "1", "-2", "0.5"] {
            assert_eq!(truth(yes), Some(true), "{yes}");
        }
        for no in [
            "false",
            "False",
            "0",
            "null",
            "nil",
            "None",
            "undefined",
            " 0.0 ",
        ] {
            assert_eq!(truth(no), Some(false), "{no}");
        }
        for neither in ["\"abc\"", "Point(x=1)", ""] {
            assert_eq!(truth(neither), None, "{neither}");
        }
    }

    #[test]
    fn breakpoints_say_who_evaluates_them() {
        // SPEC-0178 behaviour 9: the MCP answer of `debug_start` shows this per breakpoint.
        let placed = Breakpoint {
            id: Some(1),
            verified: true,
            line: Some(4),
            requested_line: None,
            reason: None,
            evaluated_by: EvaluatedBy::IdeCraft,
        };
        assert_eq!(
            serde_json::to_value(&placed).unwrap(),
            json!({"id": 1, "verified": true, "line": 4, "evaluated_by": "idecraft"})
        );
        // SPEC-0127 v5 behaviour 15: an unbound one names the line asked for and the reason.
        let unbound = Breakpoint {
            id: Some(2),
            verified: false,
            line: Some(8),
            requested_line: Some(7),
            reason: Some("no executable code".into()),
            evaluated_by: EvaluatedBy::Adapter,
        };
        assert_eq!(
            serde_json::to_value(&unbound).unwrap(),
            json!({"id": 2, "verified": false, "line": 8, "requested_line": 7,
                   "reason": "no executable code", "evaluated_by": "adapter"})
        );
        assert_eq!(
            adapter_reason(&json!({"verified": false, "message": " not loaded "})),
            Some("not loaded".into())
        );
        assert_eq!(
            adapter_reason(&json!({"verified": true, "message": "ok"})),
            None
        );
        assert_eq!(
            serde_json::to_value(EvaluatedBy::default()).unwrap(),
            json!("adapter")
        );
    }

    #[test]
    fn log_messages_are_interpolated() {
        let upper = |e: &str| e.to_uppercase();
        assert_eq!(interpolate("i={i}, j={j}!", upper), "i=I, j=J!");
        assert_eq!(interpolate("no braces", upper), "no braces");
        assert_eq!(interpolate("open {i", upper), "open {i");
    }
}
