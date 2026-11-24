//! Editing run configurations (`SPEC-0327`): the shell-quoted command line, the problems of a
//! configuration by field, templates, the writer of `.ferris-idea/run.toml` that keeps comments and
//! keys it does not know, the operations agents use, temporary configurations, and why a
//! configuration cannot start (`SPEC-0087` v6 behaviours 13 and 14, IMP-0394, IMP-0395).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, value};

use crate::run::{RUN_FILE, RunConfig, RunKind, cycle_problem, field_problems, read_configs};
use crate::run_tasks::BeforeTask;

// ---------------------------------------------------------------------------------------------
// The command line (behaviour 4; SPEC-0087 v6 behaviour 13).

/// Splits a command line as a POSIX shell does, without expanding anything: blanks separate
/// arguments, `'…'` keeps everything literally, `"…"` keeps everything but `\"`, `\\`, `\$` and
/// `` \` ``, a backslash outside quotes keeps the next character, and quoted parts next to each
/// other form one argument (`'it''s'` is `its`).
pub fn split_command_line(text: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                if in_word {
                    args.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("a ' quote is not closed".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.peek().copied() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => {
                                chars.next();
                                word.push(c);
                            }
                            Some('\n') => {
                                chars.next();
                            }
                            _ => word.push('\\'),
                        },
                        Some(c) => word.push(c),
                        None => return Err("a \" quote is not closed".into()),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(c) => {
                    in_word = true;
                    word.push(c);
                }
                None => return Err("the command line ends with a backslash".into()),
            },
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        args.push(word);
    }
    Ok(args)
}

/// One argument as a shell reads it back unchanged: plain when it holds only characters without
/// meaning to a shell, else in single quotes (a quote inside becomes `'\''`).
pub fn quote_arg(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@+,%".contains(c));
    if plain {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// A command as one line that a shell runs with the same program and arguments (IMP-0395). A
/// program name with `=` is quoted too, so a shell does not read it as a variable.
pub fn join_command_line(args: &[String]) -> String {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            if i == 0 && a.contains('=') {
                format!("'{}'", a.replace('\'', "'\\''"))
            } else {
                quote_arg(a)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------------------------
// Problems by field (behaviour 4).

/// A field of the configuration form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunField {
    /// The name.
    Name,
    /// The command line.
    Command,
    /// The working directory.
    Cwd,
    /// The environment.
    Env,
    /// Run or test, and the report.
    Report,
    /// The before-launch list.
    Before,
    /// The timeout.
    Timeout,
    /// The page to open.
    Url,
    /// The folder it is listed in.
    Folder,
}

/// The problems of `c` among the `others` of the same file (`SPEC-0047` behaviour 2), each under
/// the field it belongs to.
pub fn config_problems(c: &RunConfig, others: &[RunConfig]) -> Vec<(RunField, String)> {
    let mut problems = Vec::new();
    if c.name.trim().is_empty() {
        problems.push((RunField::Name, "a configuration has no name".to_owned()));
    } else if others.iter().any(|o| o.name == c.name) {
        problems.push((
            RunField::Name,
            format!("{}: the name is used twice", c.name),
        ));
    }
    for k in c.env.keys() {
        if k.trim().is_empty() || k.contains('=') || k.contains(char::is_whitespace) {
            problems.push((
                RunField::Env,
                format!("{}: {k:?} is not a variable name", c.name),
            ));
        }
    }
    if c.timeout == Some(0) {
        problems.push((
            RunField::Timeout,
            format!("{}: the timeout must be at least 1 second", c.name),
        ));
    }
    let mut all: Vec<RunConfig> = others.to_vec();
    all.push(c.clone());
    problems.extend(field_problems(c, &all));
    if let Some(p) = cycle_problem(&all, &c.name) {
        problems.push((RunField::Before, p));
    }
    problems
}

// ---------------------------------------------------------------------------------------------
// Templates (behaviour 3).

/// What Add offers: the types of configurations. A type fills the command; its template
/// ([`TypeTemplate`]) fills the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunTemplate {
    /// Any program; the command line is left to type.
    Command,
    /// A shell script.
    ShellScript,
    /// A Node.js file.
    NodeFile,
    /// An npm script.
    NpmScript,
    /// A Python file.
    PythonFile,
    /// Cargo.
    Cargo,
    /// Maven.
    Maven,
    /// Gradle.
    Gradle,
    /// Go tests.
    GoTest,
    /// pytest.
    Pytest,
}

impl RunTemplate {
    /// Every template in the order the chooser lists them.
    pub const ALL: [RunTemplate; 10] = [
        RunTemplate::Command,
        RunTemplate::ShellScript,
        RunTemplate::NodeFile,
        RunTemplate::NpmScript,
        RunTemplate::PythonFile,
        RunTemplate::Cargo,
        RunTemplate::Maven,
        RunTemplate::Gradle,
        RunTemplate::GoTest,
        RunTemplate::Pytest,
    ];

    /// Its label in the chooser, also the name of a new configuration.
    pub fn label(self) -> &'static str {
        match self {
            RunTemplate::Command => "Command",
            RunTemplate::ShellScript => "Shell script",
            RunTemplate::NodeFile => "Node.js file",
            RunTemplate::NpmScript => "npm script",
            RunTemplate::PythonFile => "Python file",
            RunTemplate::Cargo => "Cargo",
            RunTemplate::Maven => "Maven",
            RunTemplate::Gradle => "Gradle",
            RunTemplate::GoTest => "Go test",
            RunTemplate::Pytest => "pytest",
        }
    }

    /// The command it fills in.
    pub fn command(self) -> Vec<String> {
        let words: &[&str] = match self {
            RunTemplate::Command => &[],
            RunTemplate::ShellScript => &["sh", "script.sh"],
            RunTemplate::NodeFile => &["node", "index.js"],
            RunTemplate::NpmScript => &["npm", "run", "start"],
            RunTemplate::PythonFile => &["python3", "main.py"],
            RunTemplate::Cargo => &["cargo", "run"],
            RunTemplate::Maven => &["mvn", "package"],
            RunTemplate::Gradle => &["./gradlew", "build"],
            RunTemplate::GoTest => &["go", "test", "./..."],
            RunTemplate::Pytest => &["pytest"],
        };
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    /// Its name in `run.toml` (`[template.<key>]`) and for agents.
    pub fn key(self) -> &'static str {
        match self {
            RunTemplate::Command => "command",
            RunTemplate::ShellScript => "shell_script",
            RunTemplate::NodeFile => "node_file",
            RunTemplate::NpmScript => "npm_script",
            RunTemplate::PythonFile => "python_file",
            RunTemplate::Cargo => "cargo",
            RunTemplate::Maven => "maven",
            RunTemplate::Gradle => "gradle",
            RunTemplate::GoTest => "go_test",
            RunTemplate::Pytest => "pytest",
        }
    }

    /// The type called `key`.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }

    /// A new configuration of this type that starts from `template` (`SPEC-0047` v4 behaviour
    /// 12), with a name not in `taken`: the type's command with the template's interpreter in
    /// place of its program and the template's options right after it, and a copy of the
    /// template's working directory, environment, before-launch lists, instance switch and
    /// timeout. Later changes of the template do not reach the configuration.
    pub fn config_from(self, taken: &[String], template: Option<&TypeTemplate>) -> RunConfig {
        let mut c = self.config(taken);
        let Some(t) = template else {
            return c;
        };
        if !c.command.is_empty() {
            if let Some(i) = &t.interpreter {
                c.command[0] = i.clone();
            }
            c.command.splice(1..1, t.options.iter().cloned());
        }
        c.cwd = t.cwd.clone();
        c.env = t.env.clone();
        c.before = t.before.clone();
        c.before_launch = t.before_launch.clone();
        c.allow_multiple = t.allow_multiple;
        c.timeout = t.timeout;
        c
    }

    /// Whether the chooser's typing filter `filter` keeps it (case-insensitive part of the label).
    pub fn matches(self, filter: &str) -> bool {
        self.label()
            .to_lowercase()
            .contains(&filter.trim().to_lowercase())
    }

    /// A new configuration of this template with a name not in `taken`.
    pub fn config(self, taken: &[String]) -> RunConfig {
        RunConfig {
            name: free_name(self.label(), taken),
            command: self.command(),
            cwd: None,
            env: BTreeMap::new(),
            kind: RunKind::Run,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: false,
            http: None,
        }
    }
}

/// The defaults a new configuration of a type starts from (`SPEC-0047` v4 behaviour 12), as
/// written in `[template.<type>]` of `run.toml`. Everything is optional; a template without any
/// key is not written.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeTemplate {
    /// The program that runs the type's file or task, in place of the type's own (`node`,
    /// `python3`, `sh`, `npm`, …): an interpreter of a certain version or of the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<String>,
    /// Arguments for that program, placed right after it and before the type's own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Working directory, workspace-relative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Environment variables.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Configurations to run first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    /// Before-launch tasks other than configurations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_launch: Vec<BeforeTask>,
    /// Several instances may run at once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_multiple: bool,
    /// Timeout in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

impl TypeTemplate {
    /// Whether it says nothing: a new configuration is the type's command alone.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The templates of a `run.toml` by type and the problems found in them. A problem of a
/// template never keeps a configuration from running: templates are only read when a
/// configuration is created.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunTemplates {
    /// Templates that say something, by type.
    pub templates: BTreeMap<RunTemplate, TypeTemplate>,
    /// Problems, each naming the template.
    pub problems: Vec<String>,
}

impl RunTemplates {
    /// The templates as agents see them: type key → template.
    pub fn json(&self) -> serde_json::Value {
        serde_json::Value::Object(
            self.templates
                .iter()
                .map(|(k, t)| {
                    (
                        k.key().to_owned(),
                        serde_json::to_value(t).unwrap_or_default(),
                    )
                })
                .collect(),
        )
    }
}

/// The problems of the template of type `kind` among the configurations `configs` of the file,
/// each under the field of the form it belongs to (the interpreter and its options under
/// [`RunField::Command`]).
pub fn template_problems(
    kind: RunTemplate,
    t: &TypeTemplate,
    configs: &[RunConfig],
) -> Vec<(RunField, String)> {
    let mut problems = Vec::new();
    let me = format!("template {}", kind.key());
    if kind.command().is_empty() && (t.interpreter.is_some() || !t.options.is_empty()) {
        problems.push((
            RunField::Command,
            format!("{me}: this type has no program, so it takes no interpreter and no options"),
        ));
    }
    if let Some(i) = &t.interpreter
        && (i.trim().is_empty() || i.chars().any(char::is_control))
    {
        problems.push((
            RunField::Command,
            format!("{me}: the interpreter is empty or has a control character"),
        ));
    }
    if t.options.iter().any(|o| o.chars().any(char::is_control)) {
        problems.push((
            RunField::Command,
            format!("{me}: an option has a control character"),
        ));
    }
    if let Some(d) = &t.cwd
        && !crate::run::inside(d)
    {
        problems.push((RunField::Cwd, format!("{me}: {d} leaves the workspace")));
    }
    for k in t.env.keys() {
        if k.trim().is_empty() || k.contains('=') || k.contains(char::is_whitespace) {
            problems.push((RunField::Env, format!("{me}: {k:?} is not a variable name")));
        }
    }
    if t.timeout == Some(0) {
        problems.push((
            RunField::Timeout,
            format!("{me}: the timeout must be at least 1 second"),
        ));
    }
    for b in &t.before {
        if !configs.iter().any(|c| &c.name == b) {
            problems.push((
                RunField::Before,
                format!("{me}: unknown before-launch configuration {b}"),
            ));
        }
    }
    for (i, task) in t.before_launch.iter().enumerate() {
        if let Some(p) = task.problem() {
            problems.push((
                RunField::Before,
                format!("{me}: before-launch task {}: {p}", i + 1),
            ));
        }
    }
    problems
}

/// The templates written in the text of a `run.toml` (`[template.<type>]`), validated against
/// the file's configurations. A file that does not read as run configurations has none.
pub fn parse_run_templates(text: &str) -> RunTemplates {
    let mut out = RunTemplates::default();
    if text.trim().is_empty() {
        return out;
    }
    let Ok(configs) = read_configs(text) else {
        return out;
    };
    let Ok(file) = text.parse::<toml::Table>() else {
        return out;
    };
    let Some(table) = file.get("template").and_then(toml::Value::as_table) else {
        return out;
    };
    for (key, value) in table {
        let Some(kind) = RunTemplate::from_key(key) else {
            out.problems.push(format!(
                "template {key}: no such type; the types are {}",
                RunTemplate::ALL.map(RunTemplate::key).join(", ")
            ));
            continue;
        };
        match value.clone().try_into::<TypeTemplate>() {
            Ok(t) => {
                out.problems.extend(
                    template_problems(kind, &t, &configs)
                        .into_iter()
                        .map(|(_, p)| p),
                );
                if !t.is_empty() {
                    out.templates.insert(kind, t);
                }
            }
            Err(e) => out.problems.push(format!("template {key}: {e}")),
        }
    }
    out
}

/// The templates of `.ferris-idea/run.toml` of `root`; a missing file has none.
pub fn load_run_templates(root: &Path) -> RunTemplates {
    match std::fs::read_to_string(root.join(RUN_FILE)) {
        Ok(text) => parse_run_templates(&text),
        Err(_) => RunTemplates::default(),
    }
}

/// The folders the configurations are listed in, in the order of their first configuration
/// (`SPEC-0047` v3 behaviour 7).
pub fn folders_of(configs: &[RunConfig]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in configs.iter().filter_map(|c| c.folder.as_ref()) {
        if !out.contains(f) {
            out.push(f.clone());
        }
    }
    out
}

/// Why `name` cannot be a folder's name, if it cannot (`SPEC-0047` v3 behaviour 7).
pub fn folder_name_problem(name: &str) -> Option<String> {
    (name.trim().is_empty() || name.trim() != name || name.chars().any(char::is_control)).then(
        || {
            "the folder name is empty, starts or ends with a blank or has a control character"
                .to_owned()
        },
    )
}

/// `base`, or with the first number that makes it free: `base (2)`, `base (3)`, …
pub fn free_name(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base} ({n})"))
        .find(|n| !taken.contains(n))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The writer (behaviour 5).

/// One configuration of the list being written: its name in the file (`None` for a new one) and
/// what it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEntry {
    /// The name it has in the file, if it is there.
    pub original: Option<String>,
    /// The configuration as it is to be written.
    pub config: RunConfig,
}

impl RunEntry {
    /// An entry of the file, unchanged.
    pub fn kept(config: RunConfig) -> Self {
        Self {
            original: Some(config.name.clone()),
            config,
        }
    }
}

/// The configurations of a `run.toml` as entries; `Err` when the file cannot be read.
pub fn entries_of(text: &str) -> Result<Vec<RunEntry>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(read_configs(text)?
        .into_iter()
        .map(RunEntry::kept)
        .collect())
}

/// The problems of `entries` as a set, each naming its configuration; empty when they can be
/// written.
pub fn entries_problems(entries: &[RunEntry]) -> Vec<String> {
    let configs: Vec<RunConfig> = entries.iter().map(|e| e.config.clone()).collect();
    let mut out = Vec::new();
    for (i, c) in configs.iter().enumerate() {
        let others: Vec<RunConfig> = configs
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, o)| o.clone())
            .collect();
        for (_, p) in config_problems(c, &others) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

fn strings(item: Option<&Item>) -> Option<Vec<String>> {
    let arr = item?.as_array()?;
    arr.iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
}

fn string_array(values: &[String]) -> Item {
    let mut a = Array::new();
    for v in values {
        a.push(v.as_str());
    }
    value(a)
}

/// Sets `key` to `want`, leaving it (and its formatting) alone when it already says that, and
/// removing it for `None`.
fn set_str(t: &mut Table, key: &str, want: Option<&str>) {
    match want {
        None => {
            t.remove(key);
        }
        Some(w) => {
            if t.get(key).and_then(Item::as_str) != Some(w) {
                t[key] = value(w);
            }
        }
    }
}

fn set_strings(t: &mut Table, key: &str, want: &[String], keep_empty: bool) {
    if want.is_empty() && !keep_empty {
        t.remove(key);
    } else if strings(t.get(key)).as_deref() != Some(want) {
        t[key] = string_array(want);
    }
}

fn env_of(item: Option<&Item>) -> Option<BTreeMap<String, String>> {
    item?
        .as_table_like()?
        .iter()
        .map(|(k, v)| v.as_str().map(|v| (k.to_owned(), v.to_owned())))
        .collect::<Option<BTreeMap<_, _>>>()
}

/// Writes the fields of `c` into the table `t`, touching only what differs, so comments, keys it
/// does not know and the formatting of unchanged values stay.
fn set_fields(t: &mut Table, c: &RunConfig) {
    set_str(t, "name", Some(&c.name));
    set_strings(t, "command", &c.command, c.url.is_none());
    set_str(t, "cwd", c.cwd.as_deref());
    set_env(t, &c.env);
    let kind_now = match t.get("kind").and_then(Item::as_str) {
        Some("test") => RunKind::Test,
        _ => RunKind::Run,
    };
    if kind_now != c.kind || (t.contains_key("kind") && t["kind"].as_str().is_none()) {
        match c.kind {
            RunKind::Run => {
                t.remove("kind");
            }
            RunKind::Test => t["kind"] = value("test"),
        }
    }
    set_str(t, "report", c.report.as_deref());
    set_str(t, "report_file", c.report_file.as_deref());
    set_strings(t, "before", &c.before, false);
    set_timeout(t, c.timeout);
    set_str(t, "url", c.url.as_deref());
    set_str(t, "folder", c.folder.as_deref());
    set_allow_multiple(t, c.allow_multiple);
    set_tasks(t, &c.before_launch);
}

/// Writes the before-launch tasks, one per line so the list reads and diffs well; untouched when
/// the table already says the same.
fn set_tasks(t: &mut Table, tasks: &[BeforeTask]) {
    if tasks.is_empty() {
        t.remove("before_launch");
    } else if tasks_of(t.get("before_launch")).as_deref() != Some(tasks) {
        let mut a = Array::new();
        for task in tasks {
            let mut inline = InlineTable::new();
            if let Some(b) = &task.build {
                inline.insert("build", b.as_str().into());
            }
            if let Some(tool) = &task.tool {
                let mut words = Array::new();
                for w in tool {
                    words.push(w.as_str());
                }
                inline.insert("tool", words.into());
            }
            if let Some(d) = &task.cwd {
                inline.insert("cwd", d.as_str().into());
            }
            if let Some(u) = &task.browser {
                inline.insert("browser", u.as_str().into());
            }
            if let Some(port) = task.wait_port {
                inline.insert("wait_port", i64::from(port).into());
            }
            let mut v = toml_edit::Value::InlineTable(inline);
            v.decor_mut().set_prefix("\n    ");
            a.push_formatted(v);
        }
        a.set_trailing_comma(true);
        a.set_trailing("\n");
        t["before_launch"] = value(a);
    }
}

fn set_env(t: &mut Table, env: &BTreeMap<String, String>) {
    if env.is_empty() {
        t.remove("env");
    } else if env_of(t.get("env")).as_ref() != Some(env) {
        let mut inline = InlineTable::new();
        for (k, v) in env {
            inline.insert(k, v.as_str().into());
        }
        t["env"] = value(inline);
    }
}

fn set_timeout(t: &mut Table, timeout: Option<u64>) {
    match timeout {
        None => {
            t.remove("timeout");
        }
        Some(s) => {
            let want = i64::try_from(s).unwrap_or(i64::MAX);
            if t.get("timeout").and_then(Item::as_integer) != Some(want) {
                t["timeout"] = value(want);
            }
        }
    }
}

fn set_allow_multiple(t: &mut Table, on: bool) {
    if on {
        if t.get("allow_multiple").and_then(Item::as_bool) != Some(true) {
            t["allow_multiple"] = value(true);
        }
    } else {
        t.remove("allow_multiple");
    }
}

/// The before-launch tasks an item of the file holds, when it reads as such: inline tables or
/// tables with the keys of [`BeforeTask`].
fn tasks_of(item: Option<&Item>) -> Option<Vec<BeforeTask>> {
    fn task(t: &dyn toml_edit::TableLike) -> Option<BeforeTask> {
        let mut out = BeforeTask::default();
        for (k, v) in t.iter() {
            match k {
                "build" => out.build = Some(v.as_str()?.to_owned()),
                "tool" => out.tool = Some(strings(Some(v))?),
                "cwd" => out.cwd = Some(v.as_str()?.to_owned()),
                "browser" => out.browser = Some(v.as_str()?.to_owned()),
                "wait_port" => out.wait_port = Some(u16::try_from(v.as_integer()?).ok()?),
                _ => return None,
            }
        }
        Some(out)
    }
    match item? {
        Item::ArrayOfTables(aot) => aot.iter().map(|t| task(t)).collect(),
        other => other
            .as_array()?
            .iter()
            .map(|v| task(v.as_inline_table()?))
            .collect(),
    }
}

/// Gives `t` and the tables nested in it positions from `next` on, so they are written in that
/// order.
fn place(t: &mut Table, next: &mut isize) {
    t.set_position(Some(*next));
    *next += 1;
    for (_, item) in t.iter_mut() {
        if let Some(inner) = item.as_table_mut() {
            place(inner, next);
        }
    }
}

fn highest_position(t: &Table) -> isize {
    let mut max = t.position().unwrap_or(0);
    for (_, item) in t.iter() {
        if let Some(inner) = item.as_table() {
            max = max.max(highest_position(inner));
        }
    }
    max
}

/// The text of `run.toml` `text` with its configurations replaced by `entries`, in their order:
/// a configuration of the file keeps its comments, unknown keys and unchanged values; one that is
/// no longer listed goes; a new one is added. A file without the envelope (`SPEC-0303`) gains it.
/// `Err` with the problems when the entries have problems or the file cannot be read; then
/// nothing is to be written.
pub fn rewrite_run_toml(text: &str, entries: &[RunEntry]) -> Result<String, Vec<String>> {
    rewrite_run_file(text, entries, None)
}

/// Writes the fields of the template `t` into its table, touching only what differs.
fn set_template_fields(table: &mut Table, t: &TypeTemplate) {
    set_str(table, "interpreter", t.interpreter.as_deref());
    set_strings(table, "options", &t.options, false);
    set_str(table, "cwd", t.cwd.as_deref());
    set_env(table, &t.env);
    set_strings(table, "before", &t.before, false);
    set_tasks(table, &t.before_launch);
    set_allow_multiple(table, t.allow_multiple);
    set_timeout(table, t.timeout);
}

/// Sets the `[template.<type>]` tables of `doc` for the types in `templates`: a template of the
/// file keeps its comments and unchanged values, one that now says nothing goes, a new one is
/// added after what the file has. The table of a type that is not named stays as it is.
fn set_templates(
    doc: &mut DocumentMut,
    templates: &BTreeMap<RunTemplate, TypeTemplate>,
    next: &mut isize,
) {
    let mut parent = match doc.remove("template") {
        Some(Item::Table(t)) => t,
        _ => Table::new(),
    };
    parent.set_implicit(true);
    let known: Vec<String> = parent.iter().map(|(k, _)| k.to_owned()).collect();
    for key in known {
        if let Some(kind) = RunTemplate::from_key(&key)
            && templates.get(&kind).is_some_and(TypeTemplate::is_empty)
        {
            parent.remove(&key);
        }
    }
    for (kind, t) in templates.iter().filter(|(_, t)| !t.is_empty()) {
        let fresh = !parent.contains_key(kind.key());
        if fresh {
            parent.insert(kind.key(), Item::Table(Table::new()));
        }
        if let Some(table) = parent.get_mut(kind.key()).and_then(Item::as_table_mut) {
            set_template_fields(table, t);
            if fresh {
                place(table, next);
            }
        }
    }
    if !parent.is_empty() {
        doc.insert("template", Item::Table(parent));
    }
}

/// [`rewrite_run_toml`] that also sets the templates of the types in `templates` (`SPEC-0047` v4
/// behaviour 12): a template that says nothing removes the type's table, a type that is not
/// named keeps its table. A template with problems keeps the whole file from being written.
pub fn rewrite_run_file(
    text: &str,
    entries: &[RunEntry],
    templates: Option<&BTreeMap<RunTemplate, TypeTemplate>>,
) -> Result<String, Vec<String>> {
    let mut problems = entries_problems(entries);
    if let Some(templates) = templates {
        let configs: Vec<RunConfig> = entries.iter().map(|e| e.config.clone()).collect();
        for (kind, t) in templates {
            problems.extend(
                template_problems(*kind, t, &configs)
                    .into_iter()
                    .map(|(_, p)| p),
            );
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let base = if crate::format::project_format(text, "run-configurations") == Ok(0) {
        let head = crate::format::write(
            &crate::format::Envelope::current("run-configurations"),
            &toml::Table::new(),
        )
        .map_err(|e| vec![e])?;
        if text.trim().is_empty() {
            head
        } else {
            format!("{head}\n{text}")
        }
    } else {
        text.to_owned()
    };
    // Only a file that reads as run configurations is changed.
    read_configs(&base).map_err(|e| vec![e])?;
    let mut doc: DocumentMut = base
        .parse()
        .map_err(|e: toml_edit::TomlError| vec![format!("{RUN_FILE}: {e}")])?;
    let mut old: Vec<Table> = match doc.remove("run") {
        Some(Item::ArrayOfTables(aot)) => aot.into_iter().collect(),
        _ => Vec::new(),
    };
    let mut next = highest_position(doc.as_table()) + 1;
    if let Some(templates) = templates {
        set_templates(&mut doc, templates, &mut next);
    }
    let mut aot = ArrayOfTables::new();
    for e in entries {
        let found = e.original.as_ref().and_then(|n| {
            old.iter()
                .position(|t| t.get("name").and_then(Item::as_str) == Some(n.as_str()))
        });
        let mut t = match found {
            Some(i) => old.remove(i),
            None => Table::new(),
        };
        set_fields(&mut t, &e.config);
        place(&mut t, &mut next);
        aot.push(t);
    }
    if !aot.is_empty() {
        doc.insert("run", Item::ArrayOfTables(aot));
    }
    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    // What is written must read back as exactly these configurations.
    let back = crate::run::parse_run_configs(&out);
    let want: Vec<&RunConfig> = entries.iter().map(|e| &e.config).collect();
    if !back.problems.is_empty() || back.configs.iter().collect::<Vec<_>>() != want {
        let mut p = back.problems;
        if p.is_empty() {
            p.push(format!(
                "{RUN_FILE}: the written file would not read back the same"
            ));
        }
        return Err(p);
    }
    if let Some(templates) = templates {
        let back = parse_run_templates(&out);
        for (kind, t) in templates {
            let want = (!t.is_empty()).then_some(t);
            if back.templates.get(kind) != want {
                return Err(vec![format!(
                    "{RUN_FILE}: the written template {} would not read back the same",
                    kind.key()
                )]);
            }
        }
    }
    Ok(out)
}

/// Writes `entries` to `.ferris-idea/run.toml` of `root` (behaviour 5): nothing is written when they
/// have problems or the file cannot be read; a write keeps the file's comments and unknown keys and
/// records the old and the new content in the Local History. Returns whether the file changed.
pub fn save_run_entries(root: &Path, entries: &[RunEntry]) -> Result<bool, Vec<String>> {
    save_run_file(root, entries, None)
}

/// [`save_run_entries`] that also writes the templates per type when given.
pub fn save_run_file(
    root: &Path,
    entries: &[RunEntry],
    templates: Option<&BTreeMap<RunTemplate, TypeTemplate>>,
) -> Result<bool, Vec<String>> {
    let file = root.join(RUN_FILE);
    crate::format::prepare_write(&file, "run-configurations").map_err(|e| vec![e.to_string()])?;
    let old = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(vec![format!("{RUN_FILE}: {e}")]),
    };
    let new = rewrite_run_file(&old, entries, templates)?;
    if new == old {
        return Ok(false);
    }
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut history = crate::LocalHistory::open(&canonical).ok();
    if let Some(h) = history.as_mut() {
        let _ = h.record(RUN_FILE);
    }
    crate::write_atomic(&file, &new).map_err(|e| vec![format!("{RUN_FILE}: {e}")])?;
    if let Some(h) = history.as_mut() {
        let _ = h.record(RUN_FILE);
    }
    Ok(true)
}

// ---------------------------------------------------------------------------------------------
// Operations (behaviour 10).

/// One change of the configurations, as agents ask for it (MCP `run_configuration_edit`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEditOp {
    /// Adds `config` at the end.
    Add(RunConfig),
    /// Replaces configuration `name` (it may be renamed).
    Update(String, RunConfig),
    /// Copies configuration `name` right after it, under the given name or a free one.
    Copy(String, Option<String>),
    /// Removes configuration `name`.
    Remove(String),
    /// Moves configuration `name` by this many places (negative: up).
    Move(String, i64),
    /// Adds a temporary configuration as a normal one, with a free name (behaviour 9).
    SaveTemporary(RunConfig),
    /// Puts configuration `name` into this folder, or takes it out of its folder (v3 behaviour
    /// 15). A folder that no configuration names comes into being with its first one.
    SetFolder(String, Option<String>),
    /// Renames the folder: every configuration in it names the new one.
    RenameFolder(String, String),
    /// Removes the folder: its configurations are listed on their own again.
    RemoveFolder(String),
}

/// Applies `op` to `entries`; returns the name of the configuration it made or changed.
pub fn apply_op(entries: &mut Vec<RunEntry>, op: RunEditOp) -> Result<String, String> {
    let taken: Vec<String> = entries.iter().map(|e| e.config.name.clone()).collect();
    let find = |name: &str| {
        entries
            .iter()
            .position(|e| e.config.name == name)
            .ok_or_else(|| format!("no run configuration {name}"))
    };
    match op {
        RunEditOp::Add(c) => {
            if taken.contains(&c.name) {
                return Err(format!("{}: the name is used already", c.name));
            }
            let name = c.name.clone();
            entries.push(RunEntry {
                original: None,
                config: c,
            });
            Ok(name)
        }
        RunEditOp::Update(name, c) => {
            let i = find(&name)?;
            let new = c.name.clone();
            entries[i].config = c;
            Ok(new)
        }
        RunEditOp::Copy(name, new_name) => {
            let i = find(&name)?;
            let mut c = entries[i].config.clone();
            c.name = match new_name {
                Some(n) if taken.contains(&n) => {
                    return Err(format!("{n}: the name is used already"));
                }
                Some(n) => n,
                None => free_name(&name, &taken),
            };
            let made = c.name.clone();
            entries.insert(
                i + 1,
                RunEntry {
                    original: None,
                    config: c,
                },
            );
            Ok(made)
        }
        RunEditOp::Remove(name) => {
            let i = find(&name)?;
            entries.remove(i);
            Ok(name)
        }
        RunEditOp::Move(name, by) => {
            let i = find(&name)?;
            let last = entries.len().saturating_sub(1) as i64;
            let to = (i as i64 + by).clamp(0, last) as usize;
            let e = entries.remove(i);
            entries.insert(to, e);
            Ok(name)
        }
        RunEditOp::SetFolder(name, folder) => {
            let i = find(&name)?;
            if let Some(p) = folder.as_deref().and_then(folder_name_problem) {
                return Err(p);
            }
            entries[i].config.folder = folder;
            Ok(name)
        }
        RunEditOp::RenameFolder(old, new) => {
            if let Some(p) = folder_name_problem(&new) {
                return Err(p);
            }
            let has = |f: &str| {
                entries
                    .iter()
                    .any(|e| e.config.folder.as_deref() == Some(f))
            };
            if !has(&old) {
                return Err(format!("no folder {old}"));
            }
            if new != old && has(&new) {
                return Err(format!("{new}: a folder of that name exists already"));
            }
            for e in entries.iter_mut() {
                if e.config.folder.as_deref() == Some(old.as_str()) {
                    e.config.folder = Some(new.clone());
                }
            }
            Ok(new)
        }
        RunEditOp::RemoveFolder(folder) => {
            let mut found = false;
            for e in entries.iter_mut() {
                if e.config.folder.as_deref() == Some(folder.as_str()) {
                    e.config.folder = None;
                    found = true;
                }
            }
            if !found {
                return Err(format!("no folder {folder}"));
            }
            Ok(folder)
        }
        RunEditOp::SaveTemporary(mut c) => {
            c.name = free_name(&c.name, &taken);
            c.verbatim = false;
            let name = c.name.clone();
            entries.push(RunEntry {
                original: None,
                config: c,
            });
            Ok(name)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Temporary configurations (behaviour 9).

/// Most temporary configurations kept in a session.
pub const MAX_TEMPORARY: usize = 5;

/// Adds the temporary configuration `config` as the newest (one of the same name is replaced);
/// beyond [`MAX_TEMPORARY`] the oldest goes. Returns the names that went.
pub fn push_temporary(list: &mut Vec<RunConfig>, config: RunConfig) -> Vec<String> {
    list.retain(|c| c.name != config.name);
    list.push(config);
    let mut gone = Vec::new();
    while list.len() > MAX_TEMPORARY {
        gone.push(list.remove(0).name);
    }
    gone
}

// ---------------------------------------------------------------------------------------------
// Why a configuration cannot start (SPEC-0087 v6 behaviour 14, IMP-0394).

/// Interpreters whose first file argument must exist.
const INTERPRETERS: &[&str] = &[
    "node", "deno", "bun", "python", "python3", "sh", "bash", "zsh", "ruby", "perl", "php",
];
/// Extensions of files an interpreter runs.
const SCRIPT_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "ts", "mts", "py", "sh", "bash", "zsh", "rb", "pl", "php",
];

fn found_on_path(program: &str, path_var: Option<&str>) -> bool {
    let Some(path) = path_var else {
        return false;
    };
    std::env::split_paths(path).any(|dir| is_runnable(&dir.join(program)))
}

fn is_runnable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Why configuration `c` of the workspace at `root` cannot start, if it cannot: safe mode, a
/// problem of `run.toml` (`problems` as loaded), a program that is not found, or a file the
/// program should run that is missing. `None` when nothing speaks against starting it.
pub fn start_blocker(root: &Path, c: &RunConfig, problems: &[String]) -> Option<String> {
    start_blocker_trusted(root, c, problems, crate::trust::allows_project_code(root))
}

/// [`start_blocker`] with the trust of the project given.
pub fn start_blocker_trusted(
    root: &Path,
    c: &RunConfig,
    problems: &[String],
    trusted: bool,
) -> Option<String> {
    if !trusted {
        return Some("Safe mode: this project is not trusted, so nothing in it runs".into());
    }
    let mine = format!("{}: ", c.name);
    if let Some(p) = problems
        .iter()
        .find(|p| p.starts_with(&mine) || p.starts_with(RUN_FILE))
    {
        return Some(p.clone());
    }
    // Variables replaced and the Python interpreter resolved, as the start will (SPEC-0050 v2).
    let command = match crate::run::launch_command(root, c) {
        Ok(command) => command,
        Err(why) => return Some(format!("{}: {why}", c.name)),
    };
    let Some(program) = command.first().cloned() else {
        return Some(format!("{}: the command is empty", c.name));
    };
    if program.trim().is_empty() {
        return Some(format!("{}: the command is empty", c.name));
    }
    let cwd: PathBuf = match &c.cwd {
        Some(d) => root.join(c.expand(d, root)),
        None => root.to_path_buf(),
    };
    if !cwd.is_dir() {
        return Some(format!(
            "the working directory {} does not exist",
            c.cwd.as_deref().unwrap_or(".")
        ));
    }
    if program.contains('/') {
        let p = Path::new(&program);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        if !full.exists() {
            return Some(format!("program not found: {program}"));
        }
    } else {
        let path_var = c
            .env
            .get("PATH")
            .map(|p| c.expand(p, root))
            .or_else(|| std::env::var("PATH").ok());
        if !found_on_path(&program, path_var.as_deref()) {
            return Some(format!("program not found: {program}"));
        }
    }
    let base = Path::new(&program)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // The first argument that is not an option is the file an interpreter runs, unless an option
    // before it says the code comes another way (`sh -c '…'`, `python -m pkg`, `node -e '…'`).
    let args: Vec<String> = command[1..].to_vec();
    let first = args.iter().position(|a| !a.starts_with('-'));
    let inline = first.is_some_and(|i| {
        args[..i]
            .iter()
            .any(|a| matches!(a.as_str(), "-c" | "-e" | "-m" | "-p" | "--eval" | "--print"))
    });
    if INTERPRETERS.contains(&base.as_str())
        && !inline
        && let Some(file) = first
            .map(|i| args[i].clone())
            .filter(|a| !a.contains(char::is_whitespace))
            .filter(|a| {
                Path::new(a)
                    .extension()
                    .is_some_and(|e| SCRIPT_EXTENSIONS.contains(&e.to_string_lossy().as_ref()))
            })
    {
        let p = Path::new(&file);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        if !full.is_file() {
            return Some(format!("file not found: {file}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(name: &str, command: &[&str]) -> RunConfig {
        RunConfig {
            name: name.into(),
            command: command.iter().map(|s| (*s).to_owned()).collect(),
            cwd: None,
            env: BTreeMap::new(),
            kind: RunKind::Run,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: false,
            http: None,
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-run-edit-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join(".ferris-idea")).unwrap();
        dir
    }

    /// SPEC-0327 AK-1.
    #[test]
    fn command_line_round_trips_through_the_quoting() {
        let args = split_command_line(r#"alpha "two words" 'it''s'"#).unwrap();
        assert_eq!(args, ["alpha", "two words", "its"]);
        let shown = join_command_line(&args);
        assert_eq!(shown, "alpha 'two words' its");
        assert_eq!(split_command_line(&shown).unwrap(), args);
        // Quotes, backslashes, empty arguments and shell characters survive both ways.
        let hard: Vec<String> = [
            "node",
            "it's",
            "",
            "a\"b",
            "back\\slash",
            "$HOME",
            "*.js",
            "x y\tz",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        assert_eq!(split_command_line(&join_command_line(&hard)).unwrap(), hard);
        assert_eq!(
            split_command_line(r#"say "a \"b\" \$x" c\ d"#).unwrap(),
            ["say", "a \"b\" $x", "c d"]
        );
        assert!(split_command_line("echo 'open").is_err());
        assert!(split_command_line("echo \"open").is_err());
        assert!(split_command_line("echo \\").is_err());
        assert_eq!(split_command_line("  ").unwrap(), Vec::<String>::new());
        assert_eq!(
            join_command_line(&["A=b".into(), "c=d".into()]),
            "'A=b' c=d"
        );
    }

    /// SPEC-0327 AK-3.
    #[test]
    fn templates_fill_the_command() {
        let want: [(&str, &[&str]); 10] = [
            ("Command", &[]),
            ("Shell script", &["sh", "script.sh"]),
            ("Node.js file", &["node", "index.js"]),
            ("npm script", &["npm", "run", "start"]),
            ("Python file", &["python3", "main.py"]),
            ("Cargo", &["cargo", "run"]),
            ("Maven", &["mvn", "package"]),
            ("Gradle", &["./gradlew", "build"]),
            ("Go test", &["go", "test", "./..."]),
            ("pytest", &["pytest"]),
        ];
        for (t, (label, command)) in RunTemplate::ALL.iter().zip(want) {
            assert_eq!(t.label(), label);
            assert_eq!(t.command(), command);
            let c = t.config(&[]);
            assert_eq!(c.name, label);
            assert_eq!(c.command, command);
        }
        let taken = vec!["Cargo".to_owned(), "Cargo (2)".to_owned()];
        assert_eq!(RunTemplate::Cargo.config(&taken).name, "Cargo (3)");
        let kept: Vec<_> = RunTemplate::ALL
            .iter()
            .filter(|t| t.matches("py"))
            .map(|t| t.label())
            .collect();
        assert_eq!(kept, ["Python file", "pytest"]);
    }

    const FIXTURE: &str = r#"[ferris-idea]
format = 1
kind = "run-configurations"

# The server, started first.
[[run]]
name = "server"
command = ["node", "server.js"]   # keep this comment
env = { PORT = "8080" }

# Tests after the server.
[[run]]
name = "tests"
kind = "test"
report = "tap"
command = ["npm", "test"]
before = ["server"]
"#;

    /// SPEC-0327 AK-2: comments and the formatting of other configurations stay; a configuration
    /// with problems is not written and the file stays as it was; a write is in the Local History.
    #[test]
    fn writing_keeps_comments_rejects_problems_and_records_history() {
        let mut entries = entries_of(FIXTURE).unwrap();
        assert_eq!(entries.len(), 2);
        entries[1].config.command = vec!["npm".into(), "run".into(), "test:ci".into()];
        let out = rewrite_run_toml(FIXTURE, &entries).unwrap();
        assert!(out.contains("# The server, started first."));
        assert!(out.contains("command = [\"node\", \"server.js\"]   # keep this comment"));
        assert!(out.contains("env = { PORT = \"8080\" }"));
        assert!(out.contains("# Tests after the server."));
        assert!(out.contains("\"test:ci\""));
        // Moving keeps each configuration's comment with it.
        let mut moved = entries.clone();
        apply_op(&mut moved, RunEditOp::Move("tests".into(), -1)).unwrap();
        let out = rewrite_run_toml(FIXTURE, &moved).unwrap();
        let tests = out.find("# Tests after the server.").unwrap();
        let server = out.find("# The server, started first.").unwrap();
        assert!(tests < out.find("name = \"tests\"").unwrap());
        assert!(tests < server && server < out.find("name = \"server\"").unwrap());
        assert_eq!(
            crate::run::parse_run_configs(&out)
                .configs
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["tests", "server"]
        );
        // A new configuration is added at the end, the others stay.
        let mut added = entries_of(FIXTURE).unwrap();
        let mut c = config("app", &["node", "main.js", "two words"]);
        c.env.insert("GREETING".into(), "hi there".into());
        apply_op(&mut added, RunEditOp::Add(c.clone())).unwrap();
        let out = rewrite_run_toml(FIXTURE, &added).unwrap();
        assert!(out.starts_with(FIXTURE.trim_end()));
        assert_eq!(crate::run::parse_run_configs(&out).configs[2], c);
        // Keys the format of a newer Ferris IDEA might add at the top stay.
        let with_unknown = FIXTURE.replace(
            "kind = \"run-configurations\"\n",
            "kind = \"run-configurations\"\nnote = \"kept\"\n",
        );
        if let Ok(e) = entries_of(&with_unknown)
            && let Ok(out) = rewrite_run_toml(&with_unknown, &e)
        {
            assert!(out.contains("note = \"kept\""));
        }

        // On disk: problems leave the file unchanged; a write is recorded.
        let root = temp_root("write");
        let file = root.join(RUN_FILE);
        std::fs::write(&file, FIXTURE).unwrap();
        let mut bad = entries_of(FIXTURE).unwrap();
        bad[0].config.command.clear();
        bad[1].config.before = vec!["nowhere".into()];
        let problems = save_run_entries(&root, &bad).unwrap_err();
        assert!(problems.iter().any(|p| p == "server: the command is empty"));
        assert!(
            problems
                .iter()
                .any(|p| p == "tests: unknown before-launch configuration nowhere")
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), FIXTURE);
        let mut dup = entries_of(FIXTURE).unwrap();
        dup[1].config.name = "server".into();
        assert!(save_run_entries(&root, &dup).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), FIXTURE);

        assert!(save_run_entries(&root, &entries).unwrap());
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("# keep this comment") && written.contains("test:ci"));
        let history = crate::LocalHistory::open(&root.canonicalize().unwrap()).unwrap();
        let revisions = history.revisions(RUN_FILE);
        assert!(revisions.len() >= 2, "{revisions:?}");
        // Writing the same again changes nothing.
        assert!(!save_run_entries(&root, &entries).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_file_gets_the_envelope() {
        let root = temp_root("new");
        let c = config("hello", &["echo", "hi"]);
        assert!(
            save_run_entries(
                &root,
                &[RunEntry {
                    original: None,
                    config: c.clone()
                }]
            )
            .unwrap()
        );
        let text = std::fs::read_to_string(root.join(RUN_FILE)).unwrap();
        assert!(text.starts_with("[ferris-idea]"));
        assert_eq!(crate::run::parse_run_configs(&text).configs, [c]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn operations_add_copy_remove_move_and_save_temporary() {
        let mut e = entries_of(FIXTURE).unwrap();
        assert!(apply_op(&mut e, RunEditOp::Add(config("server", &["x"]))).is_err());
        assert_eq!(
            apply_op(&mut e, RunEditOp::Copy("server".into(), None)).unwrap(),
            "server (2)"
        );
        assert_eq!(e[1].config.name, "server (2)");
        assert_eq!(e[1].original, None);
        apply_op(&mut e, RunEditOp::Remove("server (2)".into())).unwrap();
        assert!(apply_op(&mut e, RunEditOp::Remove("nope".into())).is_err());
        apply_op(&mut e, RunEditOp::Move("server".into(), 5)).unwrap();
        assert_eq!(e[1].config.name, "server");
        let mut renamed = e[1].config.clone();
        renamed.name = "api".into();
        apply_op(&mut e, RunEditOp::Update("server".into(), renamed)).unwrap();
        assert_eq!(e[1].original.as_deref(), Some("server"));
        assert_eq!(
            apply_op(
                &mut e,
                RunEditOp::SaveTemporary(config("api", &["node", "a.js"]))
            )
            .unwrap(),
            "api (2)"
        );
    }

    /// SPEC-0327 AK-4 (model): at most five temporary configurations, the oldest goes.
    #[test]
    fn temporary_configurations_keep_five() {
        let mut list = Vec::new();
        for i in 0..5 {
            assert!(push_temporary(&mut list, config(&format!("t{i}"), &["x"])).is_empty());
        }
        assert_eq!(push_temporary(&mut list, config("t5", &["x"])), ["t0"]);
        assert_eq!(list.len(), MAX_TEMPORARY);
        // The same file again replaces its configuration and makes it the newest.
        assert!(push_temporary(&mut list, config("t2", &["y"])).is_empty());
        assert_eq!(
            list.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["t1", "t3", "t4", "t5", "t2"]
        );
    }

    #[test]
    fn problems_belong_to_their_field() {
        let mut c = config("app", &[]);
        c.cwd = Some("../out".into());
        c.env.insert("BAD NAME".into(), "1".into());
        c.before = vec!["app".into()];
        c.timeout = Some(0);
        let fields: Vec<RunField> = config_problems(&c, &[config("other", &["x"])])
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        assert_eq!(
            config_problems(&config("other", &["y"]), &[config("other", &["x"])]),
            [(RunField::Name, "other: the name is used twice".to_owned())]
        );
        for f in [
            RunField::Command,
            RunField::Cwd,
            RunField::Env,
            RunField::Timeout,
            RunField::Before,
        ] {
            assert!(fields.contains(&f), "{f:?} in {fields:?}");
        }
    }

    /// SPEC-0087 v6 AK-7: the reason for a start that cannot happen.
    #[test]
    fn a_start_that_cannot_happen_names_the_reason() {
        let root = temp_root("blocker");
        let missing = config("ghost", &["ferris-idea-no-such-program-xyz", "a"]);
        assert_eq!(
            start_blocker(&root, &missing, &[]).as_deref(),
            Some("program not found: ferris-idea-no-such-program-xyz")
        );
        let rel = config("local", &["./bin/tool"]);
        assert_eq!(
            start_blocker(&root, &rel, &[]).as_deref(),
            Some("program not found: ./bin/tool")
        );
        let script = config("script", &["sh", "missing.sh"]);
        assert_eq!(
            start_blocker(&root, &script, &[]).as_deref(),
            Some("file not found: missing.sh")
        );
        // Code given inline is not a file.
        let inline = config("inline", &["sh", "-c", "node x.js"]);
        assert_eq!(start_blocker(&root, &inline, &[]), None);
        std::fs::write(root.join("missing.sh"), "echo hi\n").unwrap();
        assert_eq!(start_blocker(&root, &script, &[]), None);
        let problems = vec!["script: the name is used twice".to_owned()];
        assert_eq!(
            start_blocker(&root, &script, &problems).as_deref(),
            Some("script: the name is used twice")
        );
        let mut nowhere = config("cwd", &["sh", "-c", "true"]);
        nowhere.cwd = Some("nope".into());
        assert_eq!(
            start_blocker(&root, &nowhere, &[]).as_deref(),
            Some("the working directory nope does not exist")
        );
        // Safe mode: an untrusted project starts nothing.
        assert_eq!(
            start_blocker_trusted(&root, &script, &[], false).as_deref(),
            Some("Safe mode: this project is not trusted, so nothing in it runs")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0047 v4 AK-11: folder operations of the configurations.
    #[test]
    fn folders_are_set_renamed_and_removed() {
        let mut e = entries_of(FIXTURE).unwrap();
        apply_op(
            &mut e,
            RunEditOp::SetFolder("server".into(), Some("Backend".into())),
        )
        .unwrap();
        apply_op(
            &mut e,
            RunEditOp::SetFolder("tests".into(), Some("Backend".into())),
        )
        .unwrap();
        let configs = |e: &[RunEntry]| e.iter().map(|e| e.config.clone()).collect::<Vec<_>>();
        assert_eq!(folders_of(&configs(&e)), ["Backend"]);
        // A name that is none, a folder that is not there and a name that is taken are refused.
        assert!(
            apply_op(
                &mut e,
                RunEditOp::SetFolder("server".into(), Some(" x".into()))
            )
            .is_err()
        );
        assert!(apply_op(&mut e, RunEditOp::RenameFolder("Nope".into(), "X".into())).is_err());
        assert!(apply_op(&mut e, RunEditOp::RenameFolder("Backend".into(), "".into())).is_err());
        apply_op(
            &mut e,
            RunEditOp::SetFolder("tests".into(), Some("Checks".into())),
        )
        .unwrap();
        assert_eq!(
            apply_op(
                &mut e,
                RunEditOp::RenameFolder("Backend".into(), "Checks".into())
            )
            .unwrap_err(),
            "Checks: a folder of that name exists already"
        );
        assert_eq!(
            apply_op(
                &mut e,
                RunEditOp::RenameFolder("Backend".into(), "Services".into())
            )
            .unwrap(),
            "Services"
        );
        assert_eq!(folders_of(&configs(&e)), ["Services", "Checks"]);
        // The file keeps its comments; the folder is one key of each configuration.
        let out = rewrite_run_toml(FIXTURE, &e).unwrap();
        assert!(out.contains("# The server, started first."));
        assert!(out.contains("folder = \"Services\""));
        // Removing a folder puts its configurations on their own; taking the last one out of a
        // folder ends the folder.
        apply_op(&mut e, RunEditOp::RemoveFolder("Services".into())).unwrap();
        apply_op(&mut e, RunEditOp::SetFolder("tests".into(), None)).unwrap();
        assert!(apply_op(&mut e, RunEditOp::RemoveFolder("Services".into())).is_err());
        assert!(folders_of(&configs(&e)).is_empty());
        assert_eq!(rewrite_run_toml(FIXTURE, &e).unwrap(), FIXTURE);
    }

    /// SPEC-0047 v4 AK-12: a new configuration is a copy of its type's template.
    #[test]
    fn a_new_configuration_copies_the_template_of_its_type() {
        let template = TypeTemplate {
            interpreter: Some(".venv/bin/python".into()),
            options: vec!["-u".into(), "-X".into(), "dev".into()],
            cwd: Some("services/api".into()),
            env: BTreeMap::from([("PYTHONPATH".into(), "src".into())]),
            before: vec!["server".into()],
            before_launch: vec![BeforeTask::build()],
            allow_multiple: true,
            timeout: Some(90),
        };
        let c = RunTemplate::PythonFile.config_from(&["Python file".into()], Some(&template));
        assert_eq!(c.name, "Python file (2)");
        assert_eq!(
            c.command,
            [".venv/bin/python", "-u", "-X", "dev", "main.py"]
        );
        assert_eq!(c.cwd.as_deref(), Some("services/api"));
        assert_eq!(c.env.get("PYTHONPATH").map(String::as_str), Some("src"));
        assert_eq!(c.before, ["server"]);
        assert_eq!(c.before_launch, [BeforeTask::build()]);
        assert!(c.allow_multiple);
        assert_eq!(c.timeout, Some(90));
        // Without a template, and for the type without a program, the command is the type's.
        assert_eq!(
            RunTemplate::PythonFile.config_from(&[], None),
            RunTemplate::PythonFile.config(&[])
        );
        let plain = TypeTemplate {
            cwd: Some("tools".into()),
            ..TypeTemplate::default()
        };
        let c = RunTemplate::Command.config_from(&[], Some(&plain));
        assert!(c.command.is_empty());
        assert_eq!(c.cwd.as_deref(), Some("tools"));
        for t in RunTemplate::ALL {
            assert_eq!(RunTemplate::from_key(t.key()), Some(t));
        }
        assert_eq!(RunTemplate::from_key("java"), None);
    }

    /// SPEC-0047 v4 AK-12: templates are read from and written to `[template.<type>]`, a file
    /// without them reads and writes as before, and a template's problems name it.
    #[test]
    fn templates_round_trip_through_run_toml() {
        // A file written before v4 has no templates and is written back unchanged.
        assert_eq!(parse_run_templates(FIXTURE), RunTemplates::default());
        let entries = entries_of(FIXTURE).unwrap();
        let none = BTreeMap::new();
        assert_eq!(
            rewrite_run_file(FIXTURE, &entries, Some(&none)).unwrap(),
            FIXTURE
        );
        let node = TypeTemplate {
            interpreter: Some("/opt/node22/bin/node".into()),
            options: vec!["--enable-source-maps".into()],
            env: BTreeMap::from([("NODE_ENV".into(), "development".into())]),
            before_launch: vec![
                BeforeTask::tool(vec!["npm".into(), "ci".into()], None),
                BeforeTask::browser_after_port("http://localhost:3000", 3000),
            ],
            ..TypeTemplate::default()
        };
        let cargo = TypeTemplate {
            before: vec!["server".into()],
            timeout: Some(1200),
            ..TypeTemplate::default()
        };
        let templates = BTreeMap::from([
            (RunTemplate::NodeFile, node.clone()),
            (RunTemplate::Cargo, cargo.clone()),
            // A template that says nothing is not written.
            (RunTemplate::Maven, TypeTemplate::default()),
        ]);
        let out = rewrite_run_file(FIXTURE, &entries, Some(&templates)).unwrap();
        assert!(out.contains("[template.node_file]"), "{out}");
        assert!(out.contains("[template.cargo]"));
        assert!(!out.contains("[template.maven]"));
        assert!(!out.contains("[template]\n"));
        // The configurations and their comments are as they were, and still read.
        assert!(out.contains("command = [\"node\", \"server.js\"]   # keep this comment"));
        assert_eq!(
            crate::run::parse_run_configs(&out),
            crate::run::parse_run_configs(FIXTURE)
        );
        let back = parse_run_templates(&out);
        assert!(back.problems.is_empty(), "{:?}", back.problems);
        assert_eq!(back.templates.len(), 2);
        assert_eq!(back.templates[&RunTemplate::NodeFile], node);
        assert_eq!(back.templates[&RunTemplate::Cargo], cargo);
        assert_eq!(back.json()["cargo"]["timeout"], 1200);
        // Writing the configurations alone leaves the templates as they are, comment included.
        let commented = out.replace(
            "[template.cargo]",
            "# Builds may take long.\n[template.cargo]",
        );
        let e2 = entries_of(&commented).unwrap();
        assert_eq!(rewrite_run_toml(&commented, &e2).unwrap(), commented);
        // Changing one template keeps the other's text and the comment.
        let mut changed = back.templates.clone();
        changed.get_mut(&RunTemplate::Cargo).unwrap().timeout = Some(60);
        let out2 = rewrite_run_file(&commented, &e2, Some(&changed)).unwrap();
        assert!(out2.contains("# Builds may take long.\n[template.cargo]"));
        assert!(out2.contains("timeout = 60"));
        assert!(out2.contains("interpreter = \"/opt/node22/bin/node\""));
        // A type that is not named keeps its table; a template that says nothing removes it.
        assert_eq!(rewrite_run_file(&out2, &e2, Some(&none)).unwrap(), out2);
        let empty = BTreeMap::from([
            (RunTemplate::NodeFile, TypeTemplate::default()),
            (RunTemplate::Cargo, TypeTemplate::default()),
        ]);
        let out3 = rewrite_run_file(&out2, &e2, Some(&empty)).unwrap();
        assert!(!out3.contains("template"), "{out3}");
        // Problems name the template and keep the file from being written.
        let bad = BTreeMap::from([
            (
                RunTemplate::Command,
                TypeTemplate {
                    interpreter: Some("sh".into()),
                    ..TypeTemplate::default()
                },
            ),
            (
                RunTemplate::Pytest,
                TypeTemplate {
                    cwd: Some("../out".into()),
                    before: vec!["nope".into()],
                    before_launch: vec![BeforeTask::browser("javascript:alert(1)")],
                    timeout: Some(0),
                    env: BTreeMap::from([("A B".into(), "x".into())]),
                    ..TypeTemplate::default()
                },
            ),
        ]);
        let problems = rewrite_run_file(FIXTURE, &entries, Some(&bad)).unwrap_err();
        assert_eq!(problems.len(), 6, "{problems:?}");
        assert!(problems[0].starts_with("template command: this type has no program"));
        assert!(problems.iter().all(|p| p.starts_with("template ")));
        // Read from a file: an unknown type and an unknown key are problems of the templates,
        // not of the configurations.
        let odd = format!(
            "{FIXTURE}\n[template.java]\ncwd = \"x\"\n\n[template.cargo]\nshell = \"zsh\"\n"
        );
        assert!(crate::run::parse_run_configs(&odd).problems.is_empty());
        let read = parse_run_templates(&odd);
        assert!(read.templates.is_empty());
        assert_eq!(read.problems.len(), 2, "{:?}", read.problems);
        assert!(
            read.problems
                .iter()
                .any(|p| p.starts_with("template java: no such type"))
        );
        assert!(
            read.problems
                .iter()
                .any(|p| p.starts_with("template cargo: "))
        );
    }
}
