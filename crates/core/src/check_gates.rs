//! Check gates per project (`SPEC-0338`, ADR-0023 rule 5): format, build, inspections without new
//! errors, the tests a change affects and optionally all tests, derived from the project model
//! (`SPEC-0048`), adjusted by `.ferris-idea/check-gates.toml`, run one after another without a shell
//! and judged only from the processes' exit codes and output.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::build_errors;
use crate::project::{Module, ModuleKind, ProjectModel};
use crate::run::{RunConfig, RunKind, RunOutcome};

/// The project file that adjusts the gates (behaviour 7).
pub const GATES_FILE: &str = ".ferris-idea/check-gates.toml";
/// The most findings one gate lists; `more` counts the rest (behaviour 13).
pub const MAX_FINDINGS: usize = 200;
/// How much of a step's output is kept, from its end (behaviour 14).
pub const TAIL_BYTES: usize = 4096;
/// The timeout of a gate without one of its own, in seconds.
pub const GATE_TIMEOUT_SECS: u64 = 1200;

/// What a gate checks (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// The code is formatted.
    Format,
    /// The code builds.
    Build,
    /// No new error of the linters and analysers.
    Inspections,
    /// The tests the changed files affect pass.
    AffectedTests,
    /// All tests pass.
    AllTests,
}

impl GateKind {
    /// Every kind, in the order of the default gates.
    pub const ALL: [GateKind; 5] = [
        GateKind::Format,
        GateKind::Build,
        GateKind::Inspections,
        GateKind::AffectedTests,
        GateKind::AllTests,
    ];

    /// The kind's name, also the name of its default gate.
    pub fn name(self) -> &'static str {
        match self {
            GateKind::Format => "format",
            GateKind::Build => "build",
            GateKind::Inspections => "inspections",
            GateKind::AffectedTests => "affected_tests",
            GateKind::AllTests => "all_tests",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == text)
    }

    fn tests(self) -> bool {
        matches!(self, GateKind::AffectedTests | GateKind::AllTests)
    }
}

/// What a failed gate does to the gates after it (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    /// The run ends; later gates are `not_run`.
    Stop,
    /// The next gate runs.
    Continue,
}

/// Where a gate comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Derived from the project model.
    Default,
    /// Added by the project file.
    File,
}

/// How a step's output is read for findings (behaviour 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reader {
    /// The files a formatter names.
    Format,
    /// Compiler messages (`build_errors`).
    Compiler,
    /// `file:line:column: message` lines of a linter (Ruff, Flake8).
    LintLines,
    /// Compiler messages, else linter lines.
    CompilerOrLint,
    /// ESLint's JSON report.
    EslintJson,
    /// The parsed test report.
    Tests,
}

/// One process of a gate (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// The command line, for people.
    pub name: String,
    /// Program and arguments, passed without a shell.
    pub command: Vec<String>,
    /// Workspace-relative folder (`.` for the workspace).
    pub cwd: String,
    /// Extra environment variables.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Report format of a test step (`SPEC-0045`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// Report file of a test step, workspace-relative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_file: Option<String>,
    /// Output on stdout fails the step.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fail_on_output: bool,
    /// How its output is read.
    pub reader: Reader,
}

impl Step {
    fn new(command: &[&str], cwd: &str, reader: Reader) -> Self {
        Self::owned(
            command.iter().map(|s| (*s).to_owned()).collect(),
            cwd,
            reader,
        )
    }

    fn owned(command: Vec<String>, cwd: &str, reader: Reader) -> Self {
        Self {
            name: command.join(" "),
            command,
            cwd: cwd.to_owned(),
            env: BTreeMap::new(),
            report: None,
            report_file: None,
            fail_on_output: false,
            reader,
        }
    }

    fn report(mut self, format: &str, file: Option<String>) -> Self {
        self.report = Some(format.to_owned());
        self.report_file = file;
        self
    }

    fn args(mut self, args: impl IntoIterator<Item = String>) -> Self {
        self.command.extend(args);
        self.name = self.command.join(" ");
        self
    }
}

/// One gate, resolved for the changed files (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gate {
    /// Unique name.
    pub name: String,
    /// What it checks.
    pub kind: GateKind,
    /// Default or project file.
    pub source: Source,
    /// Whether it runs.
    pub enabled: bool,
    /// What a failure does.
    pub on_failure: OnFailure,
    /// Timeout of each step, in seconds.
    pub timeout: u64,
    /// The processes it runs.
    pub steps: Vec<Step>,
    /// Why it has nothing to run, if so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<String>,
}

/// A known inspection finding that does not fail its gate (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    /// The file, workspace-relative.
    pub file: String,
    /// The message, as the finding has it.
    pub message: String,
    /// The gate it applies to; every inspection gate when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
}

/// The gates of a project and the problems of its project file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gates {
    /// In run order.
    pub gates: Vec<Gate>,
    /// Known findings.
    pub baseline: Vec<Baseline>,
    /// Problems of the project file, each naming the gate (behaviour 9).
    pub problems: Vec<String>,
    /// The changed files the gates were resolved for.
    pub changed: Vec<String>,
}

fn new_gate(kind: GateKind, steps: Vec<Step>) -> Gate {
    Gate {
        name: kind.name().to_owned(),
        kind,
        source: Source::Default,
        enabled: kind != GateKind::AllTests,
        on_failure: match kind {
            GateKind::Format | GateKind::Build => OnFailure::Stop,
            _ => OnFailure::Continue,
        },
        timeout: GATE_TIMEOUT_SECS,
        steps,
        skip: None,
    }
}

fn uses(m: &Module, name: &str) -> bool {
    m.dependencies
        .iter()
        .any(|d| d.name.eq_ignore_ascii_case(name))
}

/// Whether `dir` (workspace-relative folder) holds `file`.
fn holds(dir: &str, file: &str) -> bool {
    dir == "." || file == dir || file.starts_with(&format!("{dir}/"))
}

/// `file` relative to the folder `dir` that holds it.
fn below(dir: &str, file: &str) -> String {
    if dir == "." {
        file.to_owned()
    } else {
        file.strip_prefix(&format!("{dir}/"))
            .unwrap_or(file)
            .to_owned()
    }
}

/// A file as an argument: relative to the step's folder and starting with `./`, so it is never
/// read as an option (behaviour 6).
pub fn changed_arg(dir: &str, file: &str) -> String {
    format!("./{}", below(dir, file))
}

fn in_dir(dir: &str, path: &str) -> String {
    if dir == "." {
        path.to_owned()
    } else {
        format!("{dir}/{path}")
    }
}

/// Modules of `kind` that are not a member of another of the same kind.
fn tops(model: &ProjectModel, kind: ModuleKind) -> Vec<&Module> {
    model
        .modules
        .iter()
        .filter(|m| m.kind == kind)
        .filter(|m| {
            !model
                .modules
                .iter()
                .any(|p| p.kind == kind && p.dir != m.dir && p.members.contains(&m.dir))
        })
        .collect()
}

/// The innermost module of `kind` that holds `file`.
fn innermost<'a>(model: &'a ProjectModel, kind: ModuleKind, file: &str) -> Option<&'a Module> {
    model
        .modules
        .iter()
        .filter(|m| m.kind == kind && holds(&m.dir, file))
        .max_by_key(|m| if m.dir == "." { 0 } else { m.dir.len() + 1 })
}

/// Whether a Python file is a test file (behaviour 5).
pub fn is_python_test(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    name.ends_with(".py")
        && (name.starts_with("test_")
            || name.ends_with("_test.py")
            || file.split('/').any(|p| p == "tests"))
}

fn npm_runner(m: &Module) -> Option<&'static str> {
    if uses(m, "vitest") {
        Some("vitest")
    } else if uses(m, "jest") {
        Some("jest")
    } else {
        None
    }
}

fn has_tests(m: &Module) -> bool {
    match m.kind {
        ModuleKind::Npm => npm_runner(m).is_some() || m.tasks.contains_key("test"),
        ModuleKind::Python => uses(m, "pytest"),
        _ => true,
    }
}

fn gradle_runner(root: &Path, m: &Module) -> crate::buildtools::GradleBuild {
    crate::buildtools::gradle_build(root, m)
}

fn go_env(mut s: Step) -> Step {
    s.env.insert("GOPROXY".into(), "off".into());
    s.env.insert("GOFLAGS".into(), "-mod=readonly".into());
    s
}

fn vitest_step(dir: &str) -> Step {
    Step::new(
        &[
            "npx",
            "--no-install",
            "vitest",
            "related",
            "--run",
            "--reporter=json",
            "--outputFile=.ferris-idea/vitest.json",
        ],
        dir,
        Reader::Tests,
    )
    .report("jest", Some(in_dir(dir, ".ferris-idea/vitest.json")))
}

fn jest_step(dir: &str) -> Step {
    Step::new(
        &[
            "npx",
            "--no-install",
            "jest",
            "--json",
            "--outputFile=.ferris-idea/jest.json",
            "--findRelatedTests",
        ],
        dir,
        Reader::Tests,
    )
    .report("jest", Some(in_dir(dir, ".ferris-idea/jest.json")))
}

fn pytest_step(dir: &str) -> Step {
    Step::new(
        &[
            "python3",
            "-m",
            "pytest",
            "--junitxml=.ferris-idea/pytest.xml",
        ],
        dir,
        Reader::Tests,
    )
    .report("junit", Some(in_dir(dir, ".ferris-idea/pytest.xml")))
}

fn cargo_test(dir: &str, args: &[&str]) -> Step {
    let mut command = vec!["cargo", "test", "--offline"];
    command.extend_from_slice(args);
    Step::new(&command, dir, Reader::Tests).report("libtest", None)
}

fn go_test(dir: &str, packages: Vec<String>) -> Step {
    go_env(Step::new(&["go", "test", "-json"], dir, Reader::Tests).report("gotest", None))
        .args(packages)
}

fn maven_test(dir: &str) -> Step {
    Step::new(&["mvn", "-o", "test"], dir, Reader::Tests)
        .report("junit", Some(in_dir(dir, "target/surefire-reports")))
}

fn gradle_test(root: &Path, m: &Module) -> Step {
    let b = gradle_runner(root, m);
    let task = if b.path.is_empty() {
        "test".to_owned()
    } else {
        format!("{}:test", b.path)
    };
    Step::owned(
        vec![b.runner, "--offline".into(), task],
        &b.root_dir,
        Reader::Tests,
    )
    .report("junit", Some(in_dir(&m.dir, "build/test-results/test")))
}

/// The default steps of every gate kind for the modules (behaviour 2), the affected tests left
/// out.
fn default_steps(root: &Path, model: &ProjectModel, kind: GateKind) -> Vec<Step> {
    let mut out = Vec::new();
    match kind {
        GateKind::Format => {
            for m in tops(model, ModuleKind::Cargo) {
                out.push(Step::new(
                    &["cargo", "fmt", "--all", "--", "--check"],
                    &m.dir,
                    Reader::Format,
                ));
            }
            for m in model.modules.iter() {
                match m.kind {
                    ModuleKind::Go => {
                        let mut s = Step::new(&["gofmt", "-l", "."], &m.dir, Reader::Format);
                        s.fail_on_output = true;
                        out.push(s);
                    }
                    ModuleKind::Npm if uses(m, "prettier") => out.push(Step::new(
                        &["npx", "--no-install", "prettier", "--check", "."],
                        &m.dir,
                        Reader::Format,
                    )),
                    ModuleKind::Python if uses(m, "ruff") => out.push(Step::new(
                        &["ruff", "format", "--check", "."],
                        &m.dir,
                        Reader::Format,
                    )),
                    ModuleKind::Python if uses(m, "black") => out.push(Step::new(
                        &["black", "--check", "."],
                        &m.dir,
                        Reader::Format,
                    )),
                    _ => {}
                }
            }
        }
        GateKind::Build => {
            for m in tops(model, ModuleKind::Cargo) {
                out.push(Step::new(
                    &["cargo", "build", "--offline", "--all-targets"],
                    &m.dir,
                    Reader::Compiler,
                ));
            }
            for m in tops(model, ModuleKind::Maven) {
                out.push(Step::new(
                    &["mvn", "-o", "-q", "test-compile"],
                    &m.dir,
                    Reader::Compiler,
                ));
            }
            let mut builds = BTreeSet::new();
            for m in model.modules.iter() {
                match m.kind {
                    ModuleKind::Gradle => {
                        let b = gradle_runner(root, m);
                        if builds.insert(b.root_dir.clone()) {
                            out.push(Step::owned(
                                vec![
                                    b.runner,
                                    "--offline".into(),
                                    "classes".into(),
                                    "testClasses".into(),
                                ],
                                &b.root_dir,
                                Reader::Compiler,
                            ));
                        }
                    }
                    ModuleKind::Go => out.push(go_env(Step::new(
                        &["go", "build", "./..."],
                        &m.dir,
                        Reader::Compiler,
                    ))),
                    ModuleKind::Npm if m.tasks.contains_key("build") => out.push(Step::new(
                        &["npm", "run", "build"],
                        &m.dir,
                        Reader::Compiler,
                    )),
                    ModuleKind::Npm if uses(m, "typescript") => out.push(Step::new(
                        &["npx", "--no-install", "tsc", "--noEmit"],
                        &m.dir,
                        Reader::Compiler,
                    )),
                    _ => {}
                }
            }
        }
        GateKind::Inspections => {
            for m in tops(model, ModuleKind::Cargo) {
                out.push(Step::new(
                    &["cargo", "clippy", "--offline", "--all-targets"],
                    &m.dir,
                    Reader::Compiler,
                ));
            }
            for m in model.modules.iter() {
                match m.kind {
                    ModuleKind::Go => out.push(go_env(Step::new(
                        &["go", "vet", "./..."],
                        &m.dir,
                        Reader::Compiler,
                    ))),
                    ModuleKind::Npm if uses(m, "eslint") => out.push(Step::new(
                        &["npx", "--no-install", "eslint", "--format", "json", "."],
                        &m.dir,
                        Reader::EslintJson,
                    )),
                    ModuleKind::Python if uses(m, "ruff") => out.push(Step::new(
                        &["ruff", "check", "--output-format=concise", "."],
                        &m.dir,
                        Reader::LintLines,
                    )),
                    ModuleKind::Python if uses(m, "flake8") => {
                        out.push(Step::new(&["flake8", "."], &m.dir, Reader::LintLines))
                    }
                    _ => {}
                }
            }
        }
        GateKind::AffectedTests => {}
        GateKind::AllTests => {
            for m in tops(model, ModuleKind::Cargo) {
                out.push(cargo_test(&m.dir, &["--workspace"]));
            }
            for m in tops(model, ModuleKind::Maven) {
                out.push(maven_test(&m.dir));
            }
            let mut builds = BTreeSet::new();
            for m in model.modules.iter() {
                match m.kind {
                    ModuleKind::Gradle => {
                        let b = gradle_runner(root, m);
                        if builds.insert(b.root_dir.clone()) {
                            out.push(
                                Step::owned(
                                    vec![b.runner, "--offline".into(), "test".into()],
                                    &b.root_dir,
                                    Reader::Tests,
                                )
                                .report("junit", Some(in_dir(&m.dir, "build/test-results/test"))),
                            );
                        }
                    }
                    ModuleKind::Go => out.push(go_test(&m.dir, vec!["./...".into()])),
                    ModuleKind::Npm if m.tasks.contains_key("test") => {
                        out.push(Step::new(&["npm", "test"], &m.dir, Reader::Tests))
                    }
                    ModuleKind::Python if uses(m, "pytest") => out.push(pytest_step(&m.dir)),
                    _ => {}
                }
            }
        }
    }
    out
}

/// The steps of the affected tests for `changed` (behaviour 5), or why there are none.
pub fn affected_steps(
    root: &Path,
    model: &ProjectModel,
    changed: &[String],
) -> Result<Vec<Step>, String> {
    if changed.is_empty() {
        return Err("no changed files".into());
    }
    let exists = |f: &str| root.join(f).exists();
    let mut out: Vec<Step> = Vec::new();
    // Cargo: the innermost package, or the whole workspace for a file of no member.
    for top in tops(model, ModuleKind::Cargo) {
        let mut packages = BTreeSet::new();
        let mut whole = false;
        for f in changed.iter().filter(|f| holds(&top.dir, f)) {
            match innermost(model, ModuleKind::Cargo, f) {
                Some(m) if m.dir == top.dir && !top.members.is_empty() => whole = true,
                Some(m) if m.dir == top.dir || top.members.contains(&m.dir) => {
                    packages.insert(m.name.clone());
                }
                _ => {}
            }
        }
        if whole {
            out.push(cargo_test(&top.dir, &["--workspace"]));
        } else {
            for p in packages {
                out.push(cargo_test(&top.dir, &["-p", &p]));
            }
        }
    }
    for m in &model.modules {
        let mine: Vec<&String> = changed
            .iter()
            .filter(|f| innermost(model, m.kind, f).is_some_and(|i| i.dir == m.dir))
            .collect();
        if mine.is_empty() || !has_tests(m) {
            continue;
        }
        match m.kind {
            ModuleKind::Cargo => {}
            ModuleKind::Go => {
                let whole = mine.iter().any(|f| {
                    let rel = below(&m.dir, f);
                    rel == "go.mod" || rel == "go.sum"
                });
                let packages: BTreeSet<String> = if whole {
                    BTreeSet::from(["./...".to_owned()])
                } else {
                    mine.iter()
                        .filter(|f| f.ends_with(".go"))
                        .map(|f| match below(&m.dir, f).rsplit_once('/') {
                            Some((dir, _)) => format!("./{dir}"),
                            None => ".".to_owned(),
                        })
                        .collect()
                };
                if !packages.is_empty() {
                    out.push(go_test(&m.dir, packages.into_iter().collect()));
                }
            }
            ModuleKind::Npm => {
                let files: Vec<String> = mine
                    .iter()
                    .filter(|f| exists(f))
                    .map(|f| changed_arg(&m.dir, f))
                    .collect();
                match npm_runner(m) {
                    Some(_) if files.is_empty() => {}
                    Some("vitest") => out.push(vitest_step(&m.dir).args(files)),
                    Some(_) => out.push(jest_step(&m.dir).args(files)),
                    None => out.push(Step::new(&["npm", "test"], &m.dir, Reader::Tests)),
                }
            }
            ModuleKind::Python => {
                let py: Vec<&&String> = mine.iter().filter(|f| f.ends_with(".py")).collect();
                if py.is_empty() {
                    continue;
                }
                if py.iter().all(|f| is_python_test(f)) {
                    let files: Vec<String> = py
                        .iter()
                        .filter(|f| exists(f))
                        .map(|f| changed_arg(&m.dir, f))
                        .collect();
                    if !files.is_empty() {
                        out.push(pytest_step(&m.dir).args(files));
                    }
                } else {
                    out.push(pytest_step(&m.dir));
                }
            }
            ModuleKind::Maven => out.push(maven_test(&m.dir)),
            ModuleKind::Gradle => out.push(gradle_test(root, m)),
        }
    }
    if out.is_empty() {
        Err("no tests are affected by the changed files".into())
    } else {
        Ok(out)
    }
}

/// The default gates of the project (behaviour 2), resolved for `changed`.
pub fn default_gates(root: &Path, model: &ProjectModel, changed: &[String]) -> Vec<Gate> {
    let mut out = Vec::new();
    for kind in GateKind::ALL {
        if kind == GateKind::AffectedTests {
            if !model.modules.iter().any(has_tests) {
                continue;
            }
            let mut g = new_gate(kind, Vec::new());
            match affected_steps(root, model, changed) {
                Ok(steps) => g.steps = steps,
                Err(why) => g.skip = Some(why),
            }
            out.push(g);
            continue;
        }
        let steps = default_steps(root, model, kind);
        if !steps.is_empty() {
            out.push(new_gate(kind, steps));
        }
    }
    out
}

/// One `[[gate]]` entry of the project file (behaviour 7).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateEntry {
    name: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    on_failure: Option<String>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    command: Option<Vec<String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    fail_on_output: Option<bool>,
    #[serde(default)]
    pass_changed: Option<bool>,
}

fn reader_of(kind: GateKind) -> Reader {
    match kind {
        GateKind::Format => Reader::Format,
        GateKind::Build => Reader::Compiler,
        GateKind::Inspections => Reader::CompilerOrLint,
        GateKind::AffectedTests | GateKind::AllTests => Reader::Tests,
    }
}

/// Applies one entry to `gates` (an existing gate of its name, or a new one); problems name it.
fn apply_entry(
    gates: &mut Vec<Gate>,
    e: GateEntry,
    changed: &[String],
    problems: &mut Vec<String>,
) {
    let name = e.name.clone();
    let mut bad = |why: String| problems.push(format!("gate `{name}`: {why}"));
    let kind = match e.kind.as_deref().map(|k| (k, GateKind::parse(k))) {
        Some((k, None)) => {
            bad(format!(
                "unknown kind `{k}`: format, build, inspections, affected_tests or all_tests"
            ));
            return;
        }
        Some((_, Some(k))) => Some(k),
        None => None,
    };
    let on_failure = match e.on_failure.as_deref() {
        None => None,
        Some("stop") => Some(OnFailure::Stop),
        Some("continue") => Some(OnFailure::Continue),
        Some(other) => {
            bad(format!("unknown on_failure `{other}`: stop or continue"));
            return;
        }
    };
    if let Some(c) = &e.command
        && c.first().is_none_or(|p| p.trim().is_empty())
    {
        bad("empty command".into());
        return;
    }
    let cwd = e.cwd.clone().unwrap_or_else(|| ".".into());
    if !crate::run::inside(&cwd) {
        bad(format!("cwd `{cwd}` is not a folder inside the workspace"));
        return;
    }
    if e.pass_changed == Some(true) && e.command.is_none() {
        bad("pass_changed needs a command".into());
        return;
    }
    let cwd = cwd
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_owned();
    let cwd = if cwd.is_empty() { ".".to_owned() } else { cwd };
    let index = match gates.iter().position(|g| g.name == e.name) {
        Some(i) => {
            if kind.is_some_and(|k| k != gates[i].kind) {
                bad(format!(
                    "a default gate keeps its kind `{}`",
                    gates[i].kind.name()
                ));
                return;
            }
            i
        }
        None => {
            let (Some(kind), Some(_)) = (kind, &e.command) else {
                bad("a new gate needs kind and command".into());
                return;
            };
            let mut g = new_gate(kind, Vec::new());
            g.name = e.name.clone();
            g.source = Source::File;
            g.enabled = true;
            gates.push(g);
            gates.len() - 1
        }
    };
    let g = &mut gates[index];
    if let Some(command) = e.command {
        let mut step = Step::owned(command, &cwd, reader_of(g.kind));
        if g.kind.tests() {
            step.reader = Reader::Tests;
        }
        g.skip = None;
        if e.pass_changed == Some(true) {
            let files: Vec<String> = changed
                .iter()
                .filter(|f| holds(&cwd, f))
                .map(|f| changed_arg(&cwd, f))
                .collect();
            if files.is_empty() {
                g.skip = Some(format!("no changed files in `{cwd}`"));
            }
            step = step.args(files);
        }
        g.steps = vec![step];
    }
    if let Some(v) = e.enabled {
        g.enabled = v;
    }
    if let Some(v) = on_failure {
        g.on_failure = v;
    }
    if let Some(v) = e.timeout {
        g.timeout = v.max(1);
    }
    for s in &mut g.steps {
        if let Some(env) = &e.env {
            s.env.extend(env.clone());
        }
        if let Some(v) = e.fail_on_output {
            s.fail_on_output = v;
        }
    }
}

/// The gates of `text` (the project file) over `defaults` (behaviours 7–9).
pub fn parse_gates(text: &str, defaults: Vec<Gate>, changed: &[String]) -> Gates {
    let mut out = Gates {
        gates: defaults,
        baseline: Vec::new(),
        problems: Vec::new(),
        changed: changed.to_vec(),
    };
    let mut table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => {
            out.problems.push(format!("{GATES_FILE}: {}", e.message()));
            return out;
        }
    };
    crate::format::adopt_former_envelope(&mut table);
    for (key, value) in &table {
        match key.as_str() {
            // The envelope (`SPEC-0303`).
            "ferris-idea" => {}
            "defaults" => match value.as_bool() {
                Some(false) => out.gates.clear(),
                Some(true) => {}
                None => out.problems.push("defaults: true or false".into()),
            },
            "gate" | "baseline" => {}
            other => out.problems.push(format!(
                "{GATES_FILE}: unknown key `{other}`: defaults, gate or baseline"
            )),
        }
    }
    let mut seen = BTreeSet::new();
    let entries = table.get("gate").and_then(|v| v.as_array()).cloned();
    if table.contains_key("gate") && entries.is_none() {
        out.problems.push("gate: a list of [[gate]] tables".into());
    }
    for (i, value) in entries.unwrap_or_default().into_iter().enumerate() {
        let label = value
            .get("name")
            .and_then(|n| n.as_str())
            .map_or_else(|| format!("#{}", i + 1), str::to_owned);
        if !seen.insert(label.clone()) {
            out.problems.push(format!("gate `{label}`: named twice"));
            continue;
        }
        match value.try_into::<GateEntry>() {
            Ok(e) => apply_entry(&mut out.gates, e, changed, &mut out.problems),
            Err(err) => out
                .problems
                .push(format!("gate `{label}`: {}", err.message())),
        }
    }
    if let Some(list) = table.get("baseline") {
        match list.clone().try_into::<Vec<Baseline>>() {
            Ok(b) => out.baseline = b,
            Err(err) => out.problems.push(format!("baseline: {}", err.message())),
        }
    }
    out
}

/// The gates of the workspace at `root` for the project `model` and the `changed` files: the
/// defaults with the project file over them (behaviours 2–9).
pub fn load_gates(root: &Path, model: &ProjectModel, changed: &[String]) -> Gates {
    let defaults = default_gates(root, model, changed);
    match std::fs::read_to_string(root.join(GATES_FILE)) {
        Ok(text) => parse_gates(&text, defaults, changed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Gates {
            gates: defaults,
            baseline: Vec::new(),
            problems: Vec::new(),
            changed: changed.to_vec(),
        },
        Err(e) => Gates {
            gates: defaults,
            baseline: Vec::new(),
            problems: vec![format!("{GATES_FILE}: {e}")],
            changed: changed.to_vec(),
        },
    }
}

/// A changed file an agent passes, checked (behaviour 4): workspace-relative, inside.
pub fn checked_changed(path: &str) -> Result<String, String> {
    let p = path.trim_start_matches("./");
    if p.is_empty() || !crate::run::inside(p) {
        return Err(format!(
            "changed file `{path}` is not a workspace-relative path inside the workspace"
        ));
    }
    Ok(p.replace('\\', "/"))
}

/// The status of a gate after a run (behaviour 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    /// Every step passed its check.
    Passed,
    /// A check failed.
    Failed,
    /// Nothing ran: disabled, not asked for, or nothing to run.
    Skipped,
    /// An earlier gate stopped the run.
    NotRun,
}

/// One finding of a gate (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Workspace-relative where it can be made so.
    pub file: Option<String>,
    /// 1-based line.
    pub line: Option<u32>,
    /// 1-based column.
    pub column: Option<u32>,
    /// `error` or `warning`.
    pub severity: String,
    /// The message.
    pub message: String,
    /// What would fix it, where known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// The failed test, for test findings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
    /// A known finding of the baseline (behaviour 8).
    pub baseline: bool,
}

fn finding(
    file: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    message: String,
) -> Finding {
    Finding {
        file,
        line,
        column,
        severity: "error".into(),
        message,
        suggestion: None,
        test: None,
        baseline: false,
    }
}

/// What one step did (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepResult {
    /// The command line.
    pub name: String,
    /// The program and arguments that ran.
    pub command: Vec<String>,
    /// The folder.
    pub cwd: String,
    /// Exit code; none when killed or not started.
    pub exit_code: Option<i32>,
    /// Killed by the timeout.
    pub timed_out: bool,
    /// Why it could not start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_error: Option<String>,
    /// Duration in milliseconds.
    pub millis: u64,
    /// The end of its output (behaviour 14).
    pub output: String,
    /// More output was cut.
    pub truncated: bool,
}

/// The result of one gate (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateResult {
    /// The gate's name.
    pub name: String,
    /// Its kind.
    pub kind: GateKind,
    /// The verdict.
    pub status: GateStatus,
    /// Why it was skipped or failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Duration in milliseconds.
    pub millis: u64,
    /// Its steps.
    pub steps: Vec<StepResult>,
    /// Its findings, at most [`MAX_FINDINGS`].
    pub findings: Vec<Finding>,
    /// Findings beyond the list.
    pub more: usize,
}

/// The result of a run (behaviours 11, 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GatesRun {
    /// No gate failed and none was left unrun.
    pub passed: bool,
    /// Per gate, in order.
    pub gates: Vec<GateResult>,
    /// The changed files the gates were resolved for.
    pub changed: Vec<String>,
}

impl GatesRun {
    /// Whether the run passed: computed from the gates, never set from outside (behaviour 12).
    fn judge(&mut self) {
        self.passed = self
            .gates
            .iter()
            .all(|g| matches!(g.status, GateStatus::Passed | GateStatus::Skipped));
    }
}

/// The last [`TAIL_BYTES`] of `text`, at a character boundary, and whether something was cut.
pub fn tail(text: &str) -> (String, bool) {
    if text.len() <= TAIL_BYTES {
        return (text.to_owned(), false);
    }
    let mut start = text.len() - TAIL_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_owned(), true)
}

/// `file` as a workspace-relative path where it lies in the workspace: compilers name files
/// absolute or relative to the step's folder.
fn relative_file(root: &Path, cwd: &str, file: &str) -> String {
    let path = Path::new(file);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(cwd).join(path)
    };
    let absolute = absolute.canonicalize().unwrap_or(absolute);
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    match absolute.strip_prefix(&base) {
        // `strip_prefix` keeps a `.` inside the path (`web/./src/new.js` where the root needs no
        // canonicalizing, as on Linux), so the components are joined again without it.
        Ok(rel) => rel
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => {
            // A path that does not exist (yet) cannot be canonicalized: tidy it by hand.
            let joined = Path::new(cwd).join(path);
            if path.is_relative() && crate::run::inside(&joined.to_string_lossy()) {
                joined
                    .components()
                    .filter(|c| !matches!(c, std::path::Component::CurDir))
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            } else {
                file.to_owned()
            }
        }
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

/// The files a formatter names as not formatted (behaviour 13).
pub fn format_findings(output: &str) -> Vec<Finding> {
    static RUSTFMT: OnceLock<Regex> = OnceLock::new();
    static PRETTIER: OnceLock<Regex> = OnceLock::new();
    static WOULD: OnceLock<Regex> = OnceLock::new();
    static PLAIN: OnceLock<Regex> = OnceLock::new();
    let rustfmt = re(&RUSTFMT, r"^Diff in (.+?)(?: at line (\d+)|:(\d+))?:?\s*$");
    let prettier = re(&PRETTIER, r"^\[warn\] (\S+)\s*$");
    let would = re(&WOULD, r"(?i)^would reformat:? (.+?)\s*$");
    // `gofmt -l`: one file per line.
    let plain = re(&PLAIN, r"^(\S+\.go)\s*$");
    let mut out: Vec<Finding> = Vec::new();
    let mut seen = BTreeSet::new();
    for line in output.lines().map(|l| l.trim_end_matches('\r')) {
        let (file, at) = if let Some(c) = rustfmt.captures(line) {
            (
                c[1].to_owned(),
                c.get(2).or(c.get(3)).and_then(|m| m.as_str().parse().ok()),
            )
        } else if let Some(c) = prettier.captures(line) {
            (c[1].to_owned(), None)
        } else if let Some(c) = would.captures(line) {
            (c[1].to_owned(), None)
        } else if let Some(c) = plain.captures(line) {
            (c[1].to_owned(), None)
        } else {
            continue;
        };
        if !seen.insert(file.clone()) {
            continue;
        }
        let mut f = finding(Some(file), at, None, "the file is not formatted".into());
        f.suggestion =
            Some("format the file with the project's formatter (MCP `format_file`)".into());
        out.push(f);
    }
    out
}

/// `file:line:column: message` lines of a linter (Ruff, Flake8), as errors (behaviour 13).
pub fn lint_findings(output: &str) -> Vec<Finding> {
    static LINT: OnceLock<Regex> = OnceLock::new();
    let lint = re(&LINT, r"^(\S[^:]*?):(\d+):(\d+): (.+)$");
    output
        .lines()
        .filter_map(|l| lint.captures(l.trim_end_matches('\r')))
        .map(|c| {
            finding(
                Some(c[1].to_owned()),
                c[2].parse().ok(),
                c[3].parse().ok(),
                c[4].to_owned(),
            )
        })
        .collect()
}

/// The messages of ESLint's JSON report (behaviour 13).
fn eslint_findings(stdout: &str) -> Vec<Finding> {
    #[derive(Deserialize)]
    struct File {
        #[serde(rename = "filePath")]
        file_path: String,
        #[serde(default)]
        messages: Vec<Message>,
    }
    #[derive(Deserialize)]
    struct Message {
        #[serde(default)]
        line: Option<u32>,
        #[serde(default)]
        column: Option<u32>,
        #[serde(default)]
        severity: u8,
        #[serde(default)]
        message: String,
        #[serde(default, rename = "ruleId")]
        rule: Option<String>,
    }
    let Some(start) = stdout.find('[') else {
        return Vec::new();
    };
    let Ok(files) = serde_json::from_str::<Vec<File>>(stdout[start..].trim_end()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for f in files {
        for m in f.messages {
            let mut x = finding(
                Some(f.file_path.clone()),
                m.line,
                m.column,
                match m.rule {
                    Some(r) => format!("{} ({r})", m.message),
                    None => m.message,
                },
            );
            if m.severity < 2 {
                x.severity = "warning".into();
            }
            out.push(x);
        }
    }
    out
}

fn compiler_findings(text: &str) -> Vec<Finding> {
    build_errors::parse(text)
        .0
        .into_iter()
        .map(|p| {
            let mut f = finding(p.file, p.line, p.column, p.message);
            f.severity = p.severity;
            f
        })
        .collect()
}

fn test_findings(o: &RunOutcome) -> Vec<Finding> {
    fn walk(suite: &crate::TestSuite, file: Option<&str>, out: &mut Vec<Finding>) {
        let file = suite.file.as_deref().or(file);
        for case in &suite.tests {
            if matches!(
                case.status,
                crate::TestStatus::Failed | crate::TestStatus::Error
            ) {
                let name = match &case.class {
                    Some(c) => format!("{c}::{}", case.name),
                    None => case.name.clone(),
                };
                let mut f = finding(
                    file.map(str::to_owned),
                    None,
                    None,
                    case.message
                        .clone()
                        .unwrap_or_else(|| format!("test {name} failed")),
                );
                f.test = Some(name);
                out.push(f);
            }
        }
        for s in &suite.suites {
            walk(s, file, out);
        }
    }
    let mut out = Vec::new();
    if let Some(r) = &o.report {
        for s in &r.suites {
            walk(s, None, &mut out);
        }
    }
    out
}

/// The findings of one step's output, read as its [`Reader`] says (behaviour 13).
pub fn findings_of(reader: Reader, o: &RunOutcome) -> Vec<Finding> {
    let both = format!("{}\n{}", o.stdout, o.stderr);
    match reader {
        Reader::Format => format_findings(&both),
        Reader::Compiler => compiler_findings(&both),
        Reader::LintLines => lint_findings(&both),
        Reader::CompilerOrLint => {
            let found = compiler_findings(&both);
            if found.is_empty() {
                lint_findings(&both)
            } else {
                found
            }
        }
        Reader::EslintJson => eslint_findings(&o.stdout),
        Reader::Tests => test_findings(o),
    }
}

fn run_config_of(
    root: &Path,
    step: &Step,
    timeout: u64,
    kind: GateKind,
) -> Result<RunConfig, String> {
    // A leading bare `python3` is the workspace's selected interpreter (`SPEC-0050` v2), resolved
    // here: the step runs verbatim, so no `${…}` in a file name is ever expanded.
    let command = crate::python_manage::python_command(root, &step.command, None)?
        .unwrap_or_else(|| step.command.clone());
    Ok(RunConfig {
        name: step.name.clone(),
        command,
        cwd: (step.cwd != ".").then(|| step.cwd.clone()),
        env: step.env.clone(),
        kind: if kind.tests() && step.report.is_some() {
            RunKind::Test
        } else {
            RunKind::Run
        },
        report: step.report.clone(),
        report_file: step.report_file.clone(),
        before: Vec::new(),
        timeout: Some(timeout),
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: true,
        http: None,
    })
}

/// Runs one step: the process, or why it could not start.
fn run_step(root: &Path, step: &Step, timeout: u64, kind: GateKind) -> RunOutcome {
    match run_config_of(root, step, timeout, kind) {
        Ok(c) => crate::run::run_one(root, &c),
        Err(why) => RunOutcome {
            name: step.name.clone(),
            exit_code: None,
            timed_out: false,
            stopped: false,
            start_error: Some(why),
            stdout: String::new(),
            stderr: String::new(),
            millis: 0,
            report: None,
            report_error: None,
            browser: Vec::new(),
        },
    }
}

fn run_gate(root: &Path, gate: &Gate, baseline: &[Baseline]) -> GateResult {
    let started = Instant::now();
    let mut result = GateResult {
        name: gate.name.clone(),
        kind: gate.kind,
        status: GateStatus::Passed,
        reason: None,
        millis: 0,
        steps: Vec::new(),
        findings: Vec::new(),
        more: 0,
    };
    let mut failures: Vec<String> = Vec::new();
    for step in &gate.steps {
        let o = run_step(root, step, gate.timeout, gate.kind);
        let mut found = findings_of(step.reader, &o);
        for f in &mut found {
            if let Some(file) = &f.file {
                f.file = Some(relative_file(root, &step.cwd, file));
            }
            f.baseline = gate.kind == GateKind::Inspections
                && baseline.iter().any(|b| {
                    b.gate.as_deref().is_none_or(|g| g == gate.name)
                        && Some(&b.file) == f.file.as_ref()
                        && b.message == f.message
                });
        }
        let ran = o.start_error.is_none() && !o.timed_out;
        let failed = if !ran {
            true
        } else if gate.kind == GateKind::Inspections {
            let errors = found.iter().filter(|f| f.severity == "error");
            let new = errors.clone().filter(|f| !f.baseline).count();
            new > 0 || (o.exit_code != Some(0) && errors.count() == 0)
        } else {
            o.exit_code != Some(0) || (step.fail_on_output && !o.stdout.trim().is_empty())
        };
        if failed {
            failures.push(if let Some(why) = &o.start_error {
                format!("{}: cannot start: {why}", step.name)
            } else if o.timed_out {
                format!("{}: timed out after {} s", step.name, gate.timeout)
            } else if gate.kind == GateKind::Inspections && o.exit_code == Some(0) {
                format!("{}: new errors", step.name)
            } else if o.exit_code == Some(0) {
                format!("{}: printed files that need a change", step.name)
            } else {
                format!(
                    "{}: exit code {}",
                    step.name,
                    o.exit_code.map_or("none".to_owned(), |c| c.to_string())
                )
            });
        }
        for f in found {
            if result.findings.len() < MAX_FINDINGS {
                result.findings.push(f);
            } else {
                result.more += 1;
            }
        }
        let both = if o.stderr.is_empty() {
            o.stdout.clone()
        } else if o.stdout.is_empty() {
            o.stderr.clone()
        } else {
            format!("{}\n{}", o.stdout, o.stderr)
        };
        let (output, truncated) = tail(&both);
        result.steps.push(StepResult {
            name: step.name.clone(),
            command: step.command.clone(),
            cwd: step.cwd.clone(),
            exit_code: o.exit_code,
            timed_out: o.timed_out,
            start_error: o.start_error,
            millis: o.millis,
            output,
            truncated,
        });
    }
    if !failures.is_empty() {
        result.status = GateStatus::Failed;
        result.reason = Some(failures.join("; "));
    }
    result.millis = started.elapsed().as_millis() as u64;
    result
}

/// Whether `gate` runs in a call: enabled, or an all-tests gate the call asks for (behaviour 11).
pub fn runs(gate: &Gate, all_tests: bool) -> bool {
    gate.enabled || (all_tests && gate.kind == GateKind::AllTests)
}

/// The gates a call selects: all, or those named in `only` (an unknown name is an error).
pub fn select<'a>(gates: &'a Gates, only: Option<&[String]>) -> Result<Vec<&'a Gate>, String> {
    let Some(only) = only else {
        return Ok(gates.gates.iter().collect());
    };
    for name in only {
        if !gates.gates.iter().any(|g| &g.name == name) {
            let known: Vec<&str> = gates.gates.iter().map(|g| g.name.as_str()).collect();
            return Err(format!(
                "no check gate `{name}`; the project has: {}",
                known.join(", ")
            ));
        }
    }
    Ok(gates
        .gates
        .iter()
        .filter(|g| only.contains(&g.name))
        .collect())
}

/// Runs the gates in order (behaviours 11–14). The verdict comes only from the runs.
pub fn run_gates(root: &Path, gates: &Gates, selected: &[&Gate], all_tests: bool) -> GatesRun {
    let mut run = GatesRun {
        passed: false,
        gates: Vec::new(),
        changed: gates.changed.clone(),
    };
    let mut stopped: Option<String> = None;
    for gate in selected {
        let mut result = GateResult {
            name: gate.name.clone(),
            kind: gate.kind,
            status: GateStatus::Skipped,
            reason: None,
            millis: 0,
            steps: Vec::new(),
            findings: Vec::new(),
            more: 0,
        };
        if let Some(by) = &stopped {
            result.status = GateStatus::NotRun;
            result.reason = Some(format!("the failed gate `{by}` stopped the run"));
        } else if !runs(gate, all_tests) {
            result.reason = Some(if gate.kind == GateKind::AllTests {
                "all tests run only when asked for (all_tests)".into()
            } else {
                "disabled".into()
            });
        } else if let Some(why) = &gate.skip {
            result.reason = Some(why.clone());
        } else {
            result = run_gate(root, gate, &gates.baseline);
            if result.status == GateStatus::Failed && gate.on_failure == OnFailure::Stop {
                stopped = Some(gate.name.clone());
            }
        }
        run.gates.push(result);
    }
    run.judge();
    run
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Dependency;

    fn module(kind: ModuleKind, name: &str, dir: &str, deps: &[&str]) -> Module {
        let mut m = Module::new(kind, name.into(), dir);
        m.dependencies = deps
            .iter()
            .map(|d| Dependency {
                name: (*d).into(),
                requirement: "1".into(),
                scope: "dev".into(),
                resolved: None,
            })
            .collect();
        m
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-check-gates-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commands(g: &Gate) -> Vec<String> {
        g.steps
            .iter()
            .map(|s| format!("{} @{}", s.name, s.cwd))
            .collect()
    }

    fn gate<'a>(gates: &'a [Gate], name: &str) -> &'a Gate {
        gates
            .iter()
            .find(|g| g.name == name)
            .unwrap_or_else(|| panic!("no gate {name}: {gates:?}"))
    }

    fn changed(files: &[&str]) -> Vec<String> {
        files.iter().map(|f| (*f).to_owned()).collect()
    }

    /// SPEC-0338 AK-1: the default gates per build system, in order, offline, all tests off.
    #[test]
    fn default_gates_per_build_system() {
        let root = scratch("defaults");
        let mut cargo = module(ModuleKind::Cargo, "app", ".", &[]);
        cargo.members = vec!["crates/core".into()];
        let core = module(ModuleKind::Cargo, "core", "crates/core", &[]);
        let model = ProjectModel {
            modules: vec![cargo, core],
            problems: Vec::new(),
        };
        let gates = default_gates(&root, &model, &[]);
        let names: Vec<&str> = gates.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "format",
                "build",
                "inspections",
                "affected_tests",
                "all_tests"
            ]
        );
        assert_eq!(
            commands(gate(&gates, "format")),
            ["cargo fmt --all -- --check @."]
        );
        assert_eq!(
            commands(gate(&gates, "build")),
            ["cargo build --offline --all-targets @."]
        );
        assert_eq!(
            commands(gate(&gates, "inspections")),
            ["cargo clippy --offline --all-targets @."]
        );
        assert_eq!(
            commands(gate(&gates, "all_tests")),
            ["cargo test --offline --workspace @."]
        );
        assert!(!gate(&gates, "all_tests").enabled);
        assert_eq!(gate(&gates, "format").on_failure, OnFailure::Stop);
        assert_eq!(gate(&gates, "inspections").on_failure, OnFailure::Continue);
        assert_eq!(
            gate(&gates, "affected_tests").skip.as_deref(),
            Some("no changed files")
        );

        // Go, npm, Python, Maven and Gradle side by side.
        let mut npm = module(
            ModuleKind::Npm,
            "web",
            "web",
            &["prettier", "eslint", "vitest", "typescript"],
        );
        npm.tasks.insert("build".into(), "vite build".into());
        npm.tasks.insert("test".into(), "vitest".into());
        std::fs::create_dir_all(root.join("jvm")).unwrap();
        std::fs::write(root.join("jvm/gradlew"), "").unwrap();
        let model = ProjectModel {
            modules: vec![
                module(ModuleKind::Go, "svc", "svc", &[]),
                npm,
                module(ModuleKind::Python, "py", "py", &["ruff", "pytest"]),
                module(ModuleKind::Maven, "g:shop", "shop", &[]),
                module(ModuleKind::Gradle, "jvm", "jvm", &[]),
            ],
            problems: Vec::new(),
        };
        let gates = default_gates(&root, &model, &[]);
        assert_eq!(
            commands(gate(&gates, "format")),
            [
                "gofmt -l . @svc",
                "npx --no-install prettier --check . @web",
                "ruff format --check . @py",
            ]
        );
        assert!(gate(&gates, "format").steps[0].fail_on_output);
        assert_eq!(
            commands(gate(&gates, "build")),
            [
                "mvn -o -q test-compile @shop",
                "go build ./... @svc",
                "npm run build @web",
                "./gradlew --offline classes testClasses @jvm",
            ]
        );
        let go = &gate(&gates, "build").steps[1];
        assert_eq!(go.env["GOPROXY"], "off");
        assert_eq!(go.env["GOFLAGS"], "-mod=readonly");
        assert_eq!(
            commands(gate(&gates, "inspections")),
            [
                "go vet ./... @svc",
                "npx --no-install eslint --format json . @web",
                "ruff check --output-format=concise . @py",
            ]
        );
        assert_eq!(
            commands(gate(&gates, "all_tests")),
            [
                "mvn -o test @shop",
                "go test -json ./... @svc",
                "npm test @web",
                "python3 -m pytest --junitxml=.ferris-idea/pytest.xml @py",
                "./gradlew --offline test @jvm",
            ]
        );
        // Without a build script TypeScript builds; Black and Flake8 when there is no Ruff.
        let model = ProjectModel {
            modules: vec![
                module(ModuleKind::Npm, "t", ".", &["typescript"]),
                module(ModuleKind::Python, "p", "p", &["black", "flake8"]),
            ],
            problems: Vec::new(),
        };
        let gates = default_gates(&root, &model, &[]);
        assert_eq!(
            commands(gate(&gates, "build")),
            ["npx --no-install tsc --noEmit @."]
        );
        assert_eq!(commands(gate(&gates, "format")), ["black --check . @p"]);
        assert_eq!(commands(gate(&gates, "inspections")), ["flake8 . @p"]);
        // No test runner: no test gates.
        assert!(gates.iter().all(|g| !g.kind.tests()), "{gates:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0338 AK-2: the affected tests by package, folder and file, `./` arguments.
    #[test]
    fn affected_tests_from_changed_files() {
        let root = scratch("affected");
        for f in [
            "web/src/a.ts",
            "py/tests/test_a.py",
            "py/app/core.py",
            "js/-rf.ts",
        ] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        let mut cargo = module(ModuleKind::Cargo, "app", ".", &[]);
        cargo.members = vec!["crates/core".into(), "crates/ui".into()];
        let model = ProjectModel {
            modules: vec![
                cargo,
                module(ModuleKind::Cargo, "core", "crates/core", &[]),
                module(ModuleKind::Cargo, "ui", "crates/ui", &[]),
            ],
            problems: Vec::new(),
        };
        let names = |c: &[&str]| -> Vec<String> {
            affected_steps(&root, &model, &changed(c))
                .unwrap()
                .iter()
                .map(|s| s.name.clone())
                .collect()
        };
        assert_eq!(
            names(&["crates/core/src/lib.rs", "crates/ui/src/a.rs"]),
            ["cargo test --offline -p core", "cargo test --offline -p ui"]
        );
        assert_eq!(
            names(&["Cargo.lock", "crates/ui/src/a.rs"]),
            ["cargo test --offline --workspace"]
        );
        assert_eq!(
            affected_steps(&root, &model, &[]).unwrap_err(),
            "no changed files"
        );
        assert_eq!(
            affected_steps(&root, &model, &changed(&["README.md"]))
                .map(|s| s.len())
                .unwrap_or(0),
            1,
            "a file of the workspace root affects the whole workspace"
        );

        let model = ProjectModel {
            modules: vec![
                module(ModuleKind::Go, "svc", "svc", &[]),
                module(ModuleKind::Npm, "web", "web", &["vitest"]),
                module(ModuleKind::Npm, "js", "js", &["jest"]),
                module(ModuleKind::Python, "py", "py", &["pytest"]),
            ],
            problems: Vec::new(),
        };
        let steps = |c: &[&str]| -> Vec<String> {
            affected_steps(&root, &model, &changed(c))
                .map(|s| s.iter().map(|s| format!("{} @{}", s.name, s.cwd)).collect())
                .unwrap_or_else(|e| vec![e])
        };
        assert_eq!(
            steps(&["svc/api/h.go", "svc/main.go", "svc/README.md"]),
            ["go test -json . ./api @svc"]
        );
        assert_eq!(steps(&["svc/go.sum"]), ["go test -json ./... @svc"]);
        assert_eq!(
            steps(&["web/src/a.ts", "web/src/gone.ts"]),
            [
                "npx --no-install vitest related --run --reporter=json --outputFile=.ferris-idea/vitest.json ./src/a.ts @web"
            ]
        );
        // A file whose name looks like an option is passed as a path.
        assert_eq!(
            steps(&["js/-rf.ts"]),
            [
                "npx --no-install jest --json --outputFile=.ferris-idea/jest.json --findRelatedTests ./-rf.ts @js"
            ]
        );
        assert_eq!(
            steps(&["py/tests/test_a.py"]),
            ["python3 -m pytest --junitxml=.ferris-idea/pytest.xml ./tests/test_a.py @py"]
        );
        assert_eq!(
            steps(&["py/tests/test_a.py", "py/app/core.py"]),
            ["python3 -m pytest --junitxml=.ferris-idea/pytest.xml @py"]
        );
        assert_eq!(
            steps(&["py/README.md"]),
            ["no tests are affected by the changed files"]
        );
        assert!(is_python_test("a/b_test.py") && !is_python_test("a/b.py"));
        assert_eq!(changed_arg("web", "web/-x"), "./-x");

        // Maven and Gradle: the innermost module.
        let mut parent = module(ModuleKind::Maven, "g:parent", ".", &[]);
        parent.members = vec!["api".into()];
        let model = ProjectModel {
            modules: vec![parent, module(ModuleKind::Maven, "g:api", "api", &[])],
            problems: Vec::new(),
        };
        assert_eq!(
            affected_steps(&root, &model, &changed(&["api/src/A.java"])).unwrap()[0].cwd,
            "api"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn cargo_model() -> ProjectModel {
        ProjectModel {
            modules: vec![module(ModuleKind::Cargo, "app", ".", &[])],
            problems: Vec::new(),
        }
    }

    /// SPEC-0338 AK-3: the project file disables, changes, adds and names known findings.
    #[test]
    fn project_file_adjusts_the_defaults() {
        let root = scratch("file");
        let changed = changed(&["src/a.rs", "docs/x.md"]);
        let defaults = default_gates(&root, &cargo_model(), &changed);
        let text = r#"
[[gate]]
name = "format"
enabled = false

[[gate]]
name = "inspections"
on_failure = "stop"
timeout = 30

[[gate]]
name = "spelling"
kind = "inspections"
command = ["typos", "--format", "brief"]
cwd = "src"
pass_changed = true

[[baseline]]
file = "src/a.rs"
message = "old"
"#;
        let gates = parse_gates(text, defaults.clone(), &changed);
        assert_eq!(gates.problems, Vec::<String>::new());
        assert!(!gate(&gates.gates, "format").enabled);
        let i = gate(&gates.gates, "inspections");
        assert_eq!((i.on_failure, i.timeout), (OnFailure::Stop, 30));
        let s = gate(&gates.gates, "spelling");
        assert_eq!(s.source, Source::File);
        assert_eq!(s.steps[0].command, ["typos", "--format", "brief", "./a.rs"]);
        assert_eq!(s.steps[0].cwd, "src");
        assert_eq!(gates.gates.last().unwrap().name, "spelling");
        assert_eq!(gates.baseline[0].message, "old");
        // pass_changed with nothing changed in the folder: skipped.
        let gates = parse_gates(text, defaults.clone(), &[]);
        assert_eq!(
            gate(&gates.gates, "spelling").skip.as_deref(),
            Some("no changed files in `src`")
        );
        // defaults = false keeps only the file's own gates.
        let gates = parse_gates(
            "defaults = false\n[[gate]]\nname = \"t\"\nkind = \"all_tests\"\ncommand = [\"make\", \"test\"]\n",
            defaults,
            &[],
        );
        assert_eq!(gates.gates.len(), 1);
        assert_eq!(gates.gates[0].steps[0].reader, Reader::Tests);
        assert!(gates.gates[0].enabled);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0338 AK-3: every problem of the project file names its gate.
    #[test]
    fn project_file_problems_name_the_gate() {
        let root = scratch("problems");
        let defaults = default_gates(&root, &cargo_model(), &[]);
        let text = r#"
colour = "red"
[[gate]]
name = "a"
kind = "lint"
command = ["x"]
[[gate]]
name = "b"
kind = "build"
command = ["x"]
cwd = "../up"
[[gate]]
name = "c"
kind = "build"
command = ["x"]
flavour = 1
[[gate]]
name = "c"
kind = "build"
command = ["y"]
[[gate]]
name = "d"
kind = "build"
command = []
[[gate]]
name = "e"
kind = "build"
[[gate]]
name = "build"
kind = "format"
[[gate]]
name = "f"
kind = "build"
command = ["x"]
on_failure = "retry"
"#;
        let gates = parse_gates(text, defaults.clone(), &[]);
        let p = gates.problems.join("\n");
        for needle in [
            "unknown key `colour`",
            "gate `a`: unknown kind `lint`",
            "gate `b`: cwd `../up`",
            "gate `c`: unknown field `flavour`",
            "gate `c`: named twice",
            "gate `d`: empty command",
            "gate `e`: a new gate needs kind and command",
            "gate `build`: a default gate keeps its kind `build`",
            "gate `f`: unknown on_failure `retry`",
        ] {
            assert!(p.contains(needle), "{needle} missing in:\n{p}");
        }
        let broken = parse_gates("[[gate]\n", defaults, &[]);
        assert!(broken.problems[0].starts_with(GATES_FILE), "{broken:?}");
        assert_eq!(
            checked_changed("../x").unwrap_err(),
            "changed file `../x` is not a workspace-relative path inside the workspace"
        );
        assert!(checked_changed("/etc/passwd").is_err());
        assert_eq!(checked_changed("./src/a.rs").unwrap(), "src/a.rs");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sh(name: &str, kind: GateKind, script: &str) -> Gate {
        let mut g = new_gate(
            kind,
            vec![Step::new(&["sh", "-c", script], ".", reader_of(kind))],
        );
        g.name = name.into();
        g.enabled = true;
        g
    }

    fn statuses(run: &GatesRun) -> Vec<(String, GateStatus)> {
        run.gates
            .iter()
            .map(|g| (g.name.clone(), g.status))
            .collect()
    }

    fn gates_of(list: Vec<Gate>) -> Gates {
        Gates {
            gates: list,
            baseline: Vec::new(),
            problems: Vec::new(),
            changed: Vec::new(),
        }
    }

    /// SPEC-0338 AK-4: order, stop and continue, disabled and all tests, timeout, the verdict.
    #[test]
    fn run_order_stop_continue_and_verdict() {
        let root = scratch("run");
        let mut off = sh("off", GateKind::Build, "touch off.txt");
        off.enabled = false;
        let mut all = sh("all", GateKind::AllTests, "touch all.txt");
        all.enabled = false;
        let mut lint = sh(
            "lint",
            GateKind::Inspections,
            "echo 'a.py:1:1: E1 bad'; exit 1",
        );
        lint.on_failure = OnFailure::Continue;
        let gates = gates_of(vec![
            sh("fmt", GateKind::Format, "true"),
            off,
            lint,
            all,
            sh("build", GateKind::Build, "exit 3"),
            sh("after", GateKind::AffectedTests, "touch after.txt"),
        ]);
        let selected = select(&gates, None).unwrap();
        let run = run_gates(&root, &gates, &selected, false);
        use GateStatus::*;
        assert_eq!(
            statuses(&run),
            [
                ("fmt".into(), Passed),
                ("off".into(), Skipped),
                ("lint".into(), Failed),
                ("all".into(), Skipped),
                ("build".into(), Failed),
                ("after".into(), NotRun),
            ]
        );
        assert!(!run.passed);
        assert!(!root.join("off.txt").exists() && !root.join("after.txt").exists());
        assert_eq!(
            run.gates[4].reason.as_deref(),
            Some("sh -c exit 3: exit code 3")
        );
        assert_eq!(
            run.gates[3].reason.as_deref(),
            Some("all tests run only when asked for (all_tests)")
        );
        // all_tests asked for; a subset by name.
        let selected = select(&gates, Some(&["fmt".into(), "all".into()])).unwrap();
        let run = run_gates(&root, &gates, &selected, true);
        assert_eq!(
            statuses(&run),
            [("fmt".into(), Passed), ("all".into(), Passed)]
        );
        assert!(run.passed && root.join("all.txt").exists());
        assert!(select(&gates, Some(&["nope".into()])).is_err());
        // A timed-out step fails its gate.
        let mut slow = sh("slow", GateKind::Build, "sleep 5");
        slow.timeout = 1;
        let gates = gates_of(vec![slow]);
        let run = run_gates(&root, &gates, &select(&gates, None).unwrap(), false);
        assert_eq!(run.gates[0].status, Failed);
        assert!(run.gates[0].steps[0].timed_out);
        assert!(!run.passed);
        // A gate with nothing to run is skipped and does not fail the run.
        let mut empty = sh("t", GateKind::AffectedTests, "false");
        empty.skip = Some("no changed files".into());
        let gates = gates_of(vec![empty]);
        let run = run_gates(&root, &gates, &select(&gates, None).unwrap(), false);
        assert_eq!(run.gates[0].status, Skipped);
        assert!(run.passed);
        // fail_on_output: gofmt -l exits 0 but names the file.
        let mut fmt = sh("gofmt", GateKind::Format, "echo main.go");
        fmt.steps[0].fail_on_output = true;
        let gates = gates_of(vec![fmt]);
        let run = run_gates(&root, &gates, &select(&gates, None).unwrap(), false);
        assert_eq!(run.gates[0].status, Failed);
        assert_eq!(run.gates[0].findings[0].file.as_deref(), Some("main.go"));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn outcome(stdout: &str, stderr: &str) -> RunOutcome {
        RunOutcome {
            name: "x".into(),
            exit_code: Some(1),
            timed_out: false,
            stopped: false,
            start_error: None,
            stdout: stdout.into(),
            stderr: stderr.into(),
            millis: 0,
            report: None,
            report_error: None,
            browser: Vec::new(),
        }
    }

    fn short(f: &Finding) -> String {
        format!(
            "{} {}:{}:{} {}",
            f.severity,
            f.file.as_deref().unwrap_or("-"),
            f.line.unwrap_or(0),
            f.column.unwrap_or(0),
            f.message
        )
    }

    /// SPEC-0338 AK-5: findings of compilers, linters, formatters and test reports.
    #[test]
    fn findings_from_output() {
        let rust = outcome(
            "",
            "error[E0425]: cannot find value `y` in this scope\n --> src/main.rs:2:13\nwarning: unused\n --> src/lib.rs:1:1\n",
        );
        let got: Vec<String> = findings_of(Reader::Compiler, &rust)
            .iter()
            .map(short)
            .collect();
        assert_eq!(
            got,
            [
                "error src/main.rs:2:13 cannot find value `y` in this scope",
                "warning src/lib.rs:1:1 unused",
            ]
        );
        let ruff = outcome(
            "src/a.py:1:8: F401 [*] `os` imported but unused\nFound 1 error.\n",
            "",
        );
        let got: Vec<String> = findings_of(Reader::LintLines, &ruff)
            .iter()
            .map(short)
            .collect();
        assert_eq!(
            got,
            ["error src/a.py:1:8 F401 [*] `os` imported but unused"]
        );
        let eslint = outcome(
            r#"[{"filePath":"/w/src/a.js","messages":[{"line":3,"column":5,"severity":2,"message":"'x' is not defined.","ruleId":"no-undef"},{"line":4,"column":1,"severity":1,"message":"Unexpected console statement.","ruleId":"no-console"}]}]"#,
            "",
        );
        let got: Vec<String> = findings_of(Reader::EslintJson, &eslint)
            .iter()
            .map(short)
            .collect();
        assert_eq!(
            got,
            [
                "error /w/src/a.js:3:5 'x' is not defined. (no-undef)",
                "warning /w/src/a.js:4:1 Unexpected console statement. (no-console)",
            ]
        );
        let fmt = outcome(
            "Diff in /w/src/main.rs at line 3:\nDiff in /w/src/lib.rs:7:\n[warn] web/a.ts\n[warn] Code style issues found in the above file. Run Prettier with --write to fix.\n",
            "would reformat py/a.py\nWould reformat: py/b.py\n",
        );
        let got: Vec<String> = findings_of(Reader::Format, &fmt)
            .iter()
            .map(short)
            .collect();
        assert_eq!(
            got,
            [
                "error /w/src/main.rs:3:0 the file is not formatted",
                "error /w/src/lib.rs:7:0 the file is not formatted",
                "error web/a.ts:0:0 the file is not formatted",
                "error py/a.py:0:0 the file is not formatted",
                "error py/b.py:0:0 the file is not formatted",
            ]
        );
        assert!(findings_of(Reader::Format, &fmt)[0].suggestion.is_some());
        // A failed test of the report, with its class and message.
        let mut tests = outcome("", "");
        tests.report = Some(
            crate::parse_report(
                "<testsuite name=\"s\"><testcase classname=\"a.B\" name=\"adds\"><failure message=\"expected 2\"/></testcase><testcase classname=\"a.B\" name=\"ok\"/></testsuite>",
                Some(crate::ReportFormat::Junit),
            )
            .unwrap(),
        );
        let got = findings_of(Reader::Tests, &tests);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].test.as_deref(), Some("a.B::adds"));
        assert_eq!(got[0].message, "expected 2");
        // Files are made workspace-relative.
        let root = scratch("relative");
        std::fs::create_dir_all(root.join("web/src")).unwrap();
        std::fs::write(root.join("web/src/a.js"), "").unwrap();
        let abs = root.join("web/src/a.js").to_string_lossy().into_owned();
        assert_eq!(relative_file(&root, "web", &abs), "web/src/a.js");
        assert_eq!(relative_file(&root, "web", "src/a.js"), "web/src/a.js");
        assert_eq!(
            relative_file(&root, "web", "./src/new.js"),
            "web/src/new.js"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0338 AK-5: warnings and baselined errors pass the inspections; a new error fails.
    #[test]
    fn inspections_fail_only_on_new_errors() {
        let root = scratch("inspections");
        let gate = |script: &str| sh("lint", GateKind::Inspections, script);
        let run = |g: Gate, baseline: Vec<Baseline>| {
            let gates = Gates {
                gates: vec![g],
                baseline,
                problems: Vec::new(),
                changed: Vec::new(),
            };
            run_gates(&root, &gates, &select(&gates, None).unwrap(), false)
        };
        let warnings = run(gate("echo 'a.c:1:1: warning: odd' >&2; exit 0"), Vec::new());
        assert_eq!(warnings.gates[0].status, GateStatus::Passed, "{warnings:?}");
        let old = Baseline {
            file: "a.py".into(),
            message: "E1 old".into(),
            gate: None,
        };
        let known = run(gate("echo 'a.py:9:1: E1 old'; exit 1"), vec![old.clone()]);
        assert_eq!(known.gates[0].status, GateStatus::Passed, "{known:?}");
        assert!(known.gates[0].findings[0].baseline);
        let new = run(
            gate("echo 'a.py:9:1: E1 old'; echo 'a.py:10:1: E2 new'; exit 1"),
            vec![old],
        );
        assert_eq!(new.gates[0].status, GateStatus::Failed);
        assert!(!new.gates[0].findings[1].baseline);
        // The tool itself failing, with nothing that explains it, fails the gate.
        let broken = run(gate("echo 'cannot read config' >&2; exit 2"), Vec::new());
        assert_eq!(broken.gates[0].status, GateStatus::Failed);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0338 AK-5: output is kept as its last 4 KiB, at a character boundary.
    #[test]
    fn output_is_cut_to_its_tail() {
        let (t, cut) = tail("short");
        assert_eq!((t.as_str(), cut), ("short", false));
        let long = format!("{}é{}", "a".repeat(TAIL_BYTES), "z".repeat(TAIL_BYTES - 1));
        let (t, cut) = tail(&long);
        assert!(cut);
        assert!(t.len() <= TAIL_BYTES && t.ends_with('z'), "{}", t.len());
        let root = scratch("tail");
        let gates = gates_of(vec![sh("b", GateKind::Build, "yes x | head -c 10000")]);
        let run = run_gates(&root, &gates, &select(&gates, None).unwrap(), false);
        assert!(run.gates[0].steps[0].truncated);
        assert_eq!(run.gates[0].steps[0].output.len(), TAIL_BYTES);
        let _ = std::fs::remove_dir_all(&root);
    }
}
