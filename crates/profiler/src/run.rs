//! Running a program under a profiler (behaviour 5) and keeping the result (behaviour 6).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cprofile::{DUMP_SCRIPT, parse_cprofile};
use crate::jfr::{PRINT, RECORDING, is_java, jfr_tool, parse_jfr};
use crate::memory::{TRACEMALLOC_RUNNER, parse_malloc_history, parse_massif, parse_tracemalloc};
use crate::model::Profile;
use crate::perf::{
    PERF_FIELDS, PERF_MISSING, PERF_RECORD, PERF_REFUSED, PERF_SCRIPT, gradle_init_script,
    is_gradle_run, parse_perf_script,
};
use crate::sample::parse_sample;

/// macOS `leaks`, which runs a program and writes its memory graph when it ends (`SPEC-0250`).
pub const LEAKS: &str = "/usr/bin/leaks";

/// macOS `malloc_history`, which reads the call trees of a memory graph.
pub const MALLOC_HISTORY: &str = "/usr/bin/malloc_history";

/// Why native programs cannot be profiled on Windows (`SPEC-0252` behaviour 4).
pub const WINDOWS_REFUSED: &str = "native programs cannot be profiled on Windows: it ships no \
sampler that records call stacks without administrator rights";

/// The macOS sampler; part of the system, needs no rights for the user's own processes.
pub const SAMPLE: &str = "/usr/bin/sample";

/// Note when macOS refused `sample` access to the program: the process that started Ferris IDEA
/// (here: a launchd service) has no Developer Tools permission.
pub const SAMPLE_REFUSED: &str = "macOS did not let /usr/bin/sample examine the program; allow the \
app that starts Ferris IDEA in System Settings › Privacy & Security › Developer Tools";

/// File name of the last profile in the profile directory.
pub const LAST: &str = "last.json";

/// The profiler a target runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// `/usr/bin/sample`, attached right after the start.
    Sample,
    /// Python's `cProfile`, through the program's own interpreter.
    CProfile,
    /// JDK Flight Recorder, started with the JVM (`SPEC-0136`).
    Jfr,
    /// Linux `perf record` (`SPEC-0252`).
    Perf,
    /// Python's `tracemalloc` through a runner (`SPEC-0250`).
    Tracemalloc,
    /// macOS `leaks --atExit` and `malloc_history` (`SPEC-0250`).
    Leaks,
    /// Valgrind Massif on Linux (`SPEC-0250`).
    Massif,
    /// macOS `heap` of a running process (`SPEC-0251`).
    Heap,
}

/// What a profile measures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Where time goes (`SPEC-0120`).
    #[default]
    Cpu,
    /// Where memory is held (`SPEC-0250`).
    Memory,
}

/// What to profile: the command a run configuration would start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileTarget {
    /// Program and arguments.
    pub command: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Extra environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// The plan of one profiling run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilePlan {
    /// The profiler.
    pub engine: Engine,
    /// The command actually started (for `cProfile` the rewritten interpreter call).
    pub command: Vec<String>,
    /// Environment set for the profiler on top of the configuration's (`MAVEN_OPTS` for JFR in
    /// Maven's JVM, `SPEC-0136`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// A finished run: the profile and what the program did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileRun {
    /// What was started.
    pub plan: ProfilePlan,
    /// Exit code; `None` when killed at the time limit or by a signal.
    pub exit_code: Option<i32>,
    /// Whether the time limit ended the program.
    pub timed_out: bool,
    /// Wall time in milliseconds.
    pub millis: u64,
    /// Last lines of the program's output.
    pub output: String,
    /// The profile.
    pub profile: Profile,
    /// CPU or memory.
    #[serde(default)]
    pub kind: Kind,
    /// Whether the program's exit code is not known (`leaks`, attached processes).
    #[serde(default)]
    pub exit_unknown: bool,
    /// Process id when attached to a running process (`SPEC-0251`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached: Option<u32>,
    /// SHA-256 of each project source file with line costs, by the file as reported, read when
    /// the profile was stored (`SPEC-0120` v5 behaviour 10).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, String>,
    /// Whether the project's frames are marked (`SPEC-0120` v5 behaviour 11).
    #[serde(default)]
    pub marked: bool,
}

/// Stores `run` as `dir/last.json`; in a workspace's profile directory its project frames are
/// marked and the digests of its source files kept first (`SPEC-0120` v5 behaviours 10, 11).
pub(crate) fn store(dir: &Path, run: &mut ProfileRun) -> Result<(), String> {
    if let Some(root) = crate::project::workspace_of(dir) {
        crate::project::prepare(run, root);
    }
    let json = serde_json::to_string(run).map_err(|e| e.to_string())?;
    fs::write(dir.join(LAST), json).map_err(|e| e.to_string())
}

/// Whether `program` is found on the `PATH`.
pub(crate) fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// Whether the first `perf` on `path` runs: `perf --version` answers. The Ubuntu launcher of
/// `linux-tools-common` is on the PATH but fails without the running kernel's tools
/// (`SPEC-0252` v4 behaviour 1).
pub(crate) fn perf_runs_on(path: &std::ffi::OsStr) -> bool {
    let Some(perf) = std::env::split_paths(path)
        .map(|d| d.join("perf"))
        .find(|p| p.is_file())
    else {
        return false;
    };
    Command::new(perf)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// [`perf_runs_on`] for this process's PATH.
pub(crate) fn perf_runs() -> bool {
    std::env::var_os("PATH").is_some_and(|p| perf_runs_on(&p))
}

fn is_python(program: &str) -> bool {
    let base = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    base == "python" || base.starts_with("python3") || base == "pypy3"
}

/// Chooses the CPU profiler: a Python interpreter call runs under `cProfile` (scripts and `-m`
/// modules), every other program under `sample` on macOS. Interpreter options before the script
/// stay in front of `-m cProfile`.
pub fn plan(target: &ProfileTarget) -> Result<ProfilePlan, String> {
    plan_kind(target, Kind::Cpu)
}

/// Whether the CPU profiler can run `command` (`SPEC-0076` v11 behaviour 14: the context actions
/// offer Profile only then).
pub fn can_profile(command: &[String]) -> bool {
    plan(&ProfileTarget {
        command: command.to_vec(),
        cwd: PathBuf::new(),
        env: BTreeMap::new(),
    })
    .is_ok()
}

/// Where the script or module starts in a Python call's arguments (after interpreter options).
fn python_script_at(args: &[String]) -> Result<usize, String> {
    let mut at = 0;
    while at < args.len() && args[at].starts_with('-') && args[at] != "-m" {
        if matches!(args[at].as_str(), "-X" | "-W") {
            at += 1;
        }
        at += 1;
    }
    if at >= args.len() {
        return Err("a Python call without a script or module cannot be profiled".into());
    }
    if args[at..].first().is_some_and(|a| a == "-c") || args[..at].iter().any(|a| a == "-c") {
        return Err("`python -c` cannot be profiled; put the code in a file".into());
    }
    Ok(at)
}

/// The memory profiler for `target` (`SPEC-0250`).
fn memory_plan(target: &ProfileTarget) -> Result<ProfilePlan, String> {
    let program = &target.command[0];
    if is_python(program) {
        let args = &target.command[1..];
        let at = python_script_at(args)?;
        let mut command = vec![program.clone()];
        command.extend(args[..at].iter().cloned());
        command.extend(["{runner}".into(), "{out}".into()]);
        if args[at] != "-m" {
            command.push("path".into());
        }
        command.extend(args[at..].iter().cloned());
        return Ok(ProfilePlan {
            engine: Engine::Tracemalloc,
            command,
            env: BTreeMap::new(),
        });
    }
    let base = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    if is_java(program) || matches!(base, "mvn" | "mvnw" | "gradle" | "gradlew") {
        return Err(
            "Java memory profiles are not supported yet; profile the CPU of this configuration instead"
                .into(),
        );
    }
    if cfg!(target_os = "macos") && Path::new(LEAKS).exists() {
        let mut command: Vec<String> = [LEAKS, "--atExit", "--outputGraph={out}", "--"]
            .map(str::to_owned)
            .to_vec();
        command.extend(target.command.iter().cloned());
        return Ok(ProfilePlan {
            engine: Engine::Leaks,
            command,
            env: BTreeMap::from([("MallocStackLogging".to_owned(), "1".to_owned())]),
        });
    }
    if cfg!(target_os = "linux") {
        if !on_path("valgrind") {
            return Err("native memory profiles on Linux use Valgrind Massif, which is not installed: install the `valgrind` package".into());
        }
        let mut command: Vec<String> =
            ["valgrind", "--tool=massif", "--massif-out-file={out}", "--"]
                .map(str::to_owned)
                .to_vec();
        command.extend(target.command.iter().cloned());
        return Ok(ProfilePlan {
            engine: Engine::Massif,
            command,
            env: BTreeMap::new(),
        });
    }
    Err("native memory profiles need macOS (leaks) or Linux (Valgrind)".into())
}

/// Chooses the profiler for `kind`: CPU as in [`plan`], plus Gradle `run` under JFR and `perf`
/// on Linux (`SPEC-0252`); memory as in `SPEC-0250`.
pub fn plan_kind(target: &ProfileTarget, kind: Kind) -> Result<ProfilePlan, String> {
    let Some(program) = target.command.first() else {
        return Err("nothing to profile: the command is empty".into());
    };
    if kind == Kind::Memory {
        return memory_plan(target);
    }
    if is_python(program) {
        let args = &target.command[1..];
        // Options such as `-u` or `-X dev` stay with the interpreter.
        let at = python_script_at(args)?;
        let mut command = vec![program.clone()];
        command.extend(args[..at].iter().cloned());
        command.extend(["-m".into(), "cProfile".into(), "-o".into(), "{out}".into()]);
        command.extend(args[at..].iter().cloned());
        return Ok(ProfilePlan {
            engine: Engine::CProfile,
            command,
            env: BTreeMap::new(),
        });
    }
    if is_java(program) {
        // JVM options go before the main class or `-jar`.
        let mut command = vec![program.clone()];
        command.extend(RECORDING.iter().map(|s| (*s).to_owned()));
        command.extend(target.command[1..].iter().cloned());
        return Ok(ProfilePlan {
            engine: Engine::Jfr,
            command,
            env: BTreeMap::new(),
        });
    }
    // `mvn exec:java` runs the main class inside Maven's JVM, which takes its options from
    // `MAVEN_OPTS`.
    let base = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    if matches!(base, "mvn" | "mvnw") && target.command.iter().any(|a| a == "exec:java") {
        let mut opts: Vec<String> = target
            .env
            .get("MAVEN_OPTS")
            .cloned()
            .or_else(|| std::env::var("MAVEN_OPTS").ok())
            .into_iter()
            .collect();
        opts.extend(RECORDING.iter().map(|s| (*s).to_owned()));
        return Ok(ProfilePlan {
            engine: Engine::Jfr,
            command: target.command.clone(),
            env: BTreeMap::from([("MAVEN_OPTS".to_owned(), opts.join(" "))]),
        });
    }
    // Gradle's `run` starts the application in its own JVM; an init script records it.
    if is_gradle_run(&target.command) {
        let mut command = vec![program.clone(), "--init-script".into(), "{init}".into()];
        command.extend(target.command[1..].iter().cloned());
        return Ok(ProfilePlan {
            engine: Engine::Jfr,
            command,
            env: BTreeMap::new(),
        });
    }
    if cfg!(target_os = "linux") {
        if !perf_runs() {
            return Err(PERF_MISSING.into());
        }
        // Said before anything runs, also in a dry run (SPEC-0252 v3 behaviour 1).
        if crate::perf::perf_blocked() {
            return Err(PERF_REFUSED.into());
        }
        let mut command: Vec<String> = vec!["perf".into()];
        command.extend(PERF_RECORD.map(str::to_owned));
        command.extend(["-o".into(), "{out}".into(), "--".into()]);
        command.extend(target.command.iter().cloned());
        return Ok(ProfilePlan {
            engine: Engine::Perf,
            command,
            env: BTreeMap::new(),
        });
    }
    if cfg!(windows) {
        return Err(WINDOWS_REFUSED.into());
    }
    if !cfg!(target_os = "macos") || !Path::new(SAMPLE).exists() {
        return Err(format!(
            "native programs are profiled with {SAMPLE}, which exists only on macOS"
        ));
    }
    Ok(ProfilePlan {
        engine: Engine::Sample,
        command: target.command.clone(),
        env: BTreeMap::new(),
    })
}

fn tail(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

/// Waits for `child` until `limit`; kills it when the limit passes.
fn wait(child: &mut Child, limit: Duration) -> (Option<i32>, bool) {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return (status.code(), false),
            Ok(None) if start.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => return (None, false),
        }
    }
}

/// `file:line` for the program's own functions of a `leaks` profile, from its debug information
/// through `atos` (hand test 180); nothing when `atos` or the symbols are missing.
fn symbolize(profile: &mut Profile, report: &str, command: &[String], cwd: &Path) {
    let Some(program) = command
        .iter()
        .position(|a| a == "--")
        .and_then(|i| command.get(i + 1))
    else {
        return;
    };
    let binary = Path::new(program)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some((load, functions)) = crate::memory::history_addresses(report, &binary) else {
        return;
    };
    if functions.is_empty() {
        return;
    }
    let Ok(out) = Command::new("atos")
        .arg("-o")
        .arg(program)
        .arg("-l")
        .arg(&load)
        .args(functions.iter().map(|(_, a)| a))
        .current_dir(cwd)
        .output()
    else {
        return;
    };
    let found = crate::memory::atos_locations(&String::from_utf8_lossy(&out.stdout));
    crate::memory::set_locations(&mut profile.roots, &found);
}

fn started(plan: &ProfilePlan, target: &ProfileTarget, log: &Path) -> Result<Child, String> {
    let command = &plan.command;
    let out = fs::File::create(log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&target.cwd)
        .envs(&target.env)
        .envs(&plan.env)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("{}: {e}", command[0]))
}

/// Runs `target` once under its CPU profiler with a time limit and stores the profile as
/// `dir/last.json` (the raw profiler output stays next to it).
pub fn profile(target: &ProfileTarget, dir: &Path, limit: Duration) -> Result<ProfileRun, String> {
    profile_kind(target, dir, limit, Kind::Cpu)
}

/// Replaces `{name}` in every part of the plan's command.
fn fill(plan: &mut ProfilePlan, name: &str, value: &Path) {
    let key = format!("{{{name}}}");
    let value = value.display().to_string();
    for part in plan.command.iter_mut().chain(plan.env.values_mut()) {
        *part = part.replace(&key, &value);
    }
}

/// Runs `target` once under the profiler for `kind` (`SPEC-0120`, `SPEC-0250`, `SPEC-0252`).
pub fn profile_kind(
    target: &ProfileTarget,
    dir: &Path,
    limit: Duration,
    kind: Kind,
) -> Result<ProfileRun, String> {
    let mut plan = plan_kind(target, kind)?;
    let mut exit_unknown = false;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let log = dir.join("output.log");
    let start = Instant::now();
    let (exit_code, timed_out, profile) = match plan.engine {
        Engine::CProfile => {
            let raw = dir.join("last.prof");
            let _ = fs::remove_file(&raw);
            for part in &mut plan.command {
                if part == "{out}" {
                    *part = raw.display().to_string();
                }
            }
            let mut child = started(&plan, target, &log)?;
            let (code, timed_out) = wait(&mut child, limit);
            if !raw.exists() {
                return Err(if timed_out {
                    "the time limit ended the program before cProfile wrote its statistics".into()
                } else {
                    format!(
                        "cProfile wrote no statistics (exit code {code:?}):\n{}",
                        tail(&log)
                    )
                });
            }
            let dump = Command::new(&plan.command[0])
                .args(["-I", "-c", DUMP_SCRIPT])
                .arg(&raw)
                .output()
                .map_err(|e| e.to_string())?;
            if !dump.status.success() {
                return Err(String::from_utf8_lossy(&dump.stderr).into_owned());
            }
            let profile = parse_cprofile(&String::from_utf8_lossy(&dump.stdout))?;
            (code, timed_out, profile)
        }
        Engine::Jfr => {
            let raw = dir.join("last.jfr");
            let _ = fs::remove_file(&raw);
            fill(&mut plan, "out", &raw);
            if plan.command.iter().any(|p| p == "{init}") {
                let init = dir.join("jfr-init.gradle");
                fs::write(&init, gradle_init_script(&raw)).map_err(|e| e.to_string())?;
                fill(&mut plan, "init", &init);
            }
            // Maven and Gradle run on the JDK of JAVA_HOME; a direct call names its `java`.
            let java = if is_java(&plan.command[0]) {
                plan.command[0].as_str()
            } else {
                "java"
            };
            let Some(tool) = jfr_tool(java, &target.env) else {
                return Err(
                    "the JDK's `jfr` tool was not found next to java, in JAVA_HOME or on PATH"
                        .into(),
                );
            };
            let mut child = started(&plan, target, &log)?;
            let (code, timed_out) = wait(&mut child, limit);
            if !raw.exists() {
                return Err(if timed_out {
                    "the time limit ended the program before the JVM wrote its recording".into()
                } else {
                    format!(
                        "the JVM wrote no recording (exit code {code:?}; JDK 17 or newer is needed):\n{}",
                        tail(&log)
                    )
                });
            }
            let printed = Command::new(&tool)
                .args(PRINT)
                .arg(&raw)
                .output()
                .map_err(|e| format!("{}: {e}", tool.display()))?;
            if !printed.status.success() {
                return Err(String::from_utf8_lossy(&printed.stderr).into_owned());
            }
            let profile = parse_jfr(&String::from_utf8_lossy(&printed.stdout));
            (code, timed_out, profile)
        }
        Engine::Sample => {
            let raw = dir.join("last.sample.txt");
            let _ = fs::remove_file(&raw);
            let mut child = started(&plan, target, &log)?;
            // One sample per millisecond until the program ends (or the limit).
            let seconds = limit.as_secs().max(1).to_string();
            // The sampler's own messages say why it could not attach (no permission).
            let sampler_log = dir.join("sample.log");
            let sampler = fs::File::create(&sampler_log)
                .map_err(|e| e.to_string())
                .and_then(|log| {
                    Command::new(SAMPLE)
                        .arg(child.id().to_string())
                        .args([seconds.as_str(), "1", "-mayDie", "-f"])
                        .arg(&raw)
                        .stdout(Stdio::null())
                        .stderr(log)
                        .spawn()
                        .map_err(|e| e.to_string())
                });
            let (code, timed_out) = wait(&mut child, limit);
            let mut notes = Vec::new();
            match sampler {
                Ok(mut s) => {
                    // The sampler writes its report once the program is gone.
                    let _ = wait(&mut s, Duration::from_secs(30));
                }
                Err(e) => notes.push(format!("{SAMPLE}: {e}")),
            }
            let mut profile = parse_sample(&fs::read_to_string(&raw).unwrap_or_default());
            let said = fs::read_to_string(&sampler_log).unwrap_or_default();
            if profile.roots.is_empty() && said.contains("cannot examine process") {
                profile.notes = vec![SAMPLE_REFUSED.into()];
            } else if profile.roots.is_empty() && start.elapsed() < Duration::from_millis(50) {
                profile.notes = vec!["the program ended before the sampler could attach".into()];
            }
            profile.notes.extend(notes);
            (code, timed_out, profile)
        }
        Engine::Perf => {
            let raw = dir.join("last.perf.data");
            let _ = fs::remove_file(&raw);
            fill(&mut plan, "out", &raw);
            let mut child = started(&plan, target, &log)?;
            let (code, timed_out) = wait(&mut child, limit);
            if fs::read_to_string(&log)
                .unwrap_or_default()
                .contains("perf_event_paranoid")
            {
                return Err(PERF_REFUSED.into());
            }
            let printed = Command::new("perf")
                .args(PERF_SCRIPT)
                .arg(PERF_FIELDS)
                .arg("-i")
                .arg(&raw)
                .output()
                .map_err(|e| format!("perf: {e}"))?;
            (
                code,
                timed_out,
                parse_perf_script(&String::from_utf8_lossy(&printed.stdout)),
            )
        }
        Engine::Tracemalloc => {
            let raw = dir.join("last.tracemalloc.json");
            let runner = dir.join("tracemalloc_runner.py");
            let _ = fs::remove_file(&raw);
            fs::write(&runner, TRACEMALLOC_RUNNER).map_err(|e| e.to_string())?;
            fill(&mut plan, "out", &raw);
            fill(&mut plan, "runner", &runner);
            let mut child = started(&plan, target, &log)?;
            let (code, timed_out) = wait(&mut child, limit);
            let Ok(json) = fs::read_to_string(&raw) else {
                return Err(if timed_out {
                    "the time limit ended the program before tracemalloc wrote its snapshot".into()
                } else {
                    format!(
                        "tracemalloc wrote no snapshot (exit code {code:?}):\n{}",
                        tail(&log)
                    )
                });
            };
            (code, timed_out, parse_tracemalloc(&json)?)
        }
        Engine::Leaks => {
            let raw = dir.join("last.memgraph");
            let _ = fs::remove_file(&raw);
            fill(&mut plan, "out", &raw);
            let mut child = started(&plan, target, &log)?;
            let (_, timed_out) = wait(&mut child, limit);
            exit_unknown = true;
            if !raw.exists() {
                let said = fs::read_to_string(&log).unwrap_or_default();
                return Err(if timed_out {
                    "the time limit ended the program before leaks wrote its memory graph".into()
                } else if said.contains("cannot examine") || said.contains("task_for_pid") {
                    SAMPLE_REFUSED.replace(SAMPLE, LEAKS)
                } else {
                    format!("leaks wrote no memory graph:\n{}", tail(&log))
                });
            }
            let history = Command::new(MALLOC_HISTORY)
                .arg(&raw)
                .arg("-callTree")
                .output()
                .map_err(|e| format!("{MALLOC_HISTORY}: {e}"))?;
            let report = String::from_utf8_lossy(&history.stdout);
            let mut profile = parse_malloc_history(&report);
            symbolize(&mut profile, &report, &plan.command, &target.cwd);
            profile
                .notes
                .push("leaks does not report the program's exit code".into());
            (None, timed_out, profile)
        }
        Engine::Massif => {
            let raw = dir.join("last.massif.out");
            let _ = fs::remove_file(&raw);
            fill(&mut plan, "out", &raw);
            let mut child = started(&plan, target, &log)?;
            let (code, timed_out) = wait(&mut child, limit);
            let Ok(text) = fs::read_to_string(&raw) else {
                return Err(format!("Massif wrote no profile:\n{}", tail(&log)));
            };
            (code, timed_out, parse_massif(&text))
        }
        Engine::Heap => return Err("a heap snapshot needs a running process".into()),
    };
    // The memory tools' own messages are not the program's output.
    let output = tail(&log)
        .lines()
        .filter(|l| !l.starts_with("leaks(") && !l.starts_with("Output graph successfully"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut run = ProfileRun {
        plan,
        exit_code,
        timed_out,
        millis: start.elapsed().as_millis() as u64,
        output,
        profile,
        kind,
        exit_unknown,
        attached: None,
        sources: BTreeMap::new(),
        marked: false,
    };
    store(dir, &mut run)?;
    Ok(run)
}

/// Profile directory of a workspace.
pub const PROFILES_DIR: &str = ".ferris-idea/profiles";

/// Profiles run configuration `name` of `root` (behaviour 5): its before-launch configurations run
/// normally first, each once; then the configuration itself runs under its profiler with its own
/// time limit. The result is stored in [`PROFILES_DIR`].
pub fn profile_config(
    root: &Path,
    configs: &ferris_idea_core::RunConfigs,
    name: &str,
) -> Result<ProfileRun, String> {
    profile_config_kind(root, configs, name, Kind::Cpu)
}

/// [`profile_config`] for CPU or memory (`SPEC-0250` behaviour 6).
pub fn profile_config_kind(
    root: &Path,
    configs: &ferris_idea_core::RunConfigs,
    name: &str,
    kind: Kind,
) -> Result<ProfileRun, String> {
    if !configs.problems.is_empty() {
        return Err(format!(
            "{} has problems: {}",
            ferris_idea_core::RUN_FILE,
            configs.problems.join("; ")
        ));
    }
    let steps = ferris_idea_core::run_plan(root, configs, name)?;
    let (last, before) = steps.split_last().ok_or("nothing to run")?;
    // The profiler is chosen before anything runs, so a missing or forbidden one is said first
    // (SPEC-0252 v3 behaviour 1).
    plan_kind(
        &ProfileTarget {
            command: last.command.clone(),
            cwd: root.join(&last.cwd),
            env: BTreeMap::new(),
        },
        kind,
    )?;
    for step in before {
        let config = configs
            .configs
            .iter()
            .find(|c| c.name == step.name)
            .ok_or_else(|| format!("no run configuration {}", step.name))?;
        let outcome = ferris_idea_core::run_one(root, config);
        if !outcome.success() {
            return Err(format!(
                "{} failed before profiling (exit code {:?})",
                step.name, outcome.exit_code
            ));
        }
    }
    let config = configs
        .configs
        .iter()
        .find(|c| c.name == last.name)
        .ok_or_else(|| format!("no run configuration {}", last.name))?;
    let target = ProfileTarget {
        command: last.command.clone(),
        cwd: root.join(&last.cwd),
        env: config
            .env
            .iter()
            .map(|(k, v)| (k.clone(), ferris_idea_core::expand_vars(v, root)))
            .collect(),
    };
    profile_kind(
        &target,
        &root.join(PROFILES_DIR),
        Duration::from_secs(last.timeout),
        kind,
    )
}

/// The file a profiler reported for a function (behaviour 7): an existing absolute path as it is,
/// else the workspace file at that relative path, else the only workspace file whose path ends
/// with it (`demo/Busy.java` in `src/main/java`, SPEC-0136), else the only one with that name.
pub fn find_source(root: &Path, reported: &str) -> Option<PathBuf> {
    let path = Path::new(reported);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    if root.join(path).is_file() {
        return Some(root.join(path));
    }
    let name = path.file_name()?;
    let files = ferris_idea_core::Workspace::open(root).ok()?.files().ok()?;
    if path.components().count() > 1 {
        let mut found = files.iter().filter(|f| f.ends_with(path));
        if let (Some(first), None) = (found.next(), found.next()) {
            return Some(root.join(first));
        }
    }
    let mut found = files.iter().filter(|f| f.file_name() == Some(name));
    let first = found.next()?;
    found.next().is_none().then(|| root.join(first))
}

/// The last stored run in `dir`, if any; one stored before `SPEC-0120` v5 gets its project
/// frames marked (behaviour 11).
pub fn last_profile(dir: &Path) -> Option<ProfileRun> {
    let mut run: ProfileRun =
        serde_json::from_str(&fs::read_to_string(dir.join(LAST)).ok()?).ok()?;
    if !run.marked
        && let Some(root) = crate::project::workspace_of(dir)
    {
        crate::project::mark_older(&mut run, root);
    }
    Some(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0252 AK-8 (v4, behaviour 1).
    #[cfg(unix)]
    #[test]
    fn a_perf_that_cannot_run_counts_as_missing() {
        use std::os::unix::fs::PermissionsExt;
        let base =
            std::env::temp_dir().join(format!("ferris-idea-perf-runs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let make = |dir: &str, body: &str| {
            let d = base.join(dir);
            fs::create_dir_all(&d).unwrap();
            let perf = d.join("perf");
            fs::write(&perf, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&perf, fs::Permissions::from_mode(0o755)).unwrap();
            d
        };
        // Like the Ubuntu launcher without the kernel's tools.
        let launcher = make(
            "launcher",
            "echo 'WARNING: perf not found for kernel' >&2; exit 2",
        );
        let works = make("works", "echo 'perf version 6.8.12'");
        let empty = base.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let path = |dirs: &[&PathBuf]| std::env::join_paths(dirs).unwrap();
        assert!(!perf_runs_on(&path(&[&launcher])));
        assert!(!perf_runs_on(&path(&[&empty])));
        assert!(perf_runs_on(&path(&[&works])));
        assert!(perf_runs_on(&path(&[&empty, &works])));
        assert!(
            !perf_runs_on(&path(&[&launcher, &works])),
            "the first perf is the one run"
        );
        assert!(PERF_MISSING.contains("linux-tools-common"));
        assert!(PERF_MISSING.contains("linux-tools-generic"));
        let _ = fs::remove_dir_all(&base);
    }

    fn target(command: &[&str]) -> ProfileTarget {
        ProfileTarget {
            command: command.iter().map(|s| s.to_string()).collect(),
            cwd: PathBuf::from("."),
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn python_calls_are_rewritten_for_cprofile() {
        let p = plan(&target(&["/venv/bin/python3", "-u", "app.py", "--n", "3"])).unwrap();
        assert_eq!(p.engine, Engine::CProfile);
        assert_eq!(
            p.command,
            [
                "/venv/bin/python3",
                "-u",
                "-m",
                "cProfile",
                "-o",
                "{out}",
                "app.py",
                "--n",
                "3"
            ]
        );
        let m = plan(&target(&["python", "-X", "dev", "-m", "pkg.tool"])).unwrap();
        assert_eq!(
            m.command,
            [
                "python", "-X", "dev", "-m", "cProfile", "-o", "{out}", "-m", "pkg.tool"
            ]
        );
        assert!(plan(&target(&["python3", "-c", "print(1)"])).is_err());
        assert!(plan(&target(&["python3"])).is_err());
        assert!(plan(&target(&[])).is_err());
    }

    #[test]
    fn sources_are_found_by_path_or_unique_name() {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-find-source-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/a")).unwrap();
        fs::create_dir_all(root.join("src/b")).unwrap();
        fs::write(root.join("src/busy.c"), "").unwrap();
        fs::write(root.join("src/a/util.c"), "").unwrap();
        fs::write(root.join("src/b/util.c"), "").unwrap();
        assert_eq!(find_source(&root, "busy.c"), Some(root.join("src/busy.c")));
        assert_eq!(
            find_source(&root, "src/a/util.c"),
            Some(root.join("src/a/util.c"))
        );
        assert_eq!(
            find_source(&root, "util.c"),
            None,
            "two files with that name"
        );
        let abs = root.join("src/busy.c").display().to_string();
        assert_eq!(find_source(&root, &abs), Some(root.join("src/busy.c")));
        assert_eq!(find_source(&root, "/no/such/file.c"), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn native_programs_use_sample_on_macos_and_perf_on_linux() {
        let p = plan(&target(&["./busy", "7"]));
        if cfg!(target_os = "macos") {
            assert_eq!(p.unwrap().engine, Engine::Sample);
        } else if cfg!(target_os = "linux") {
            match p {
                Ok(p) => assert_eq!(&p.command[..2], ["perf", "record"]),
                // No perf, or a kernel that does not allow it (SPEC-0252 v3 behaviour 1).
                Err(e) => assert!(e == PERF_MISSING || e == PERF_REFUSED, "{e}"),
            }
        } else {
            assert_eq!(p.unwrap_err(), WINDOWS_REFUSED);
        }
    }

    #[test]
    fn memory_plans_per_program() {
        let py = plan_kind(&target(&["python3", "-u", "app.py", "x"]), Kind::Memory).unwrap();
        assert_eq!(py.engine, Engine::Tracemalloc);
        assert_eq!(
            py.command,
            ["python3", "-u", "{runner}", "{out}", "path", "app.py", "x"]
        );
        let m = plan_kind(&target(&["python3", "-m", "pkg.tool"]), Kind::Memory).unwrap();
        assert_eq!(
            m.command,
            ["python3", "{runner}", "{out}", "-m", "pkg.tool"]
        );
        assert!(plan_kind(&target(&["python3", "-c", "1"]), Kind::Memory).is_err());
        assert!(
            plan_kind(&target(&["java", "-cp", "o", "A"]), Kind::Memory)
                .unwrap_err()
                .contains("Java memory")
        );
        let native = plan_kind(&target(&["./alloc"]), Kind::Memory);
        if cfg!(target_os = "macos") {
            let native = native.unwrap();
            assert_eq!(native.engine, Engine::Leaks);
            assert_eq!(native.command.last().unwrap(), "./alloc");
            assert_eq!(native.env["MallocStackLogging"], "1");
        } else if cfg!(target_os = "linux") {
            match native {
                Ok(p) => assert_eq!(p.engine, Engine::Massif),
                Err(e) => assert!(e.contains("valgrind")),
            }
        }
    }

    #[test]
    fn gradle_run_uses_an_init_script() {
        let p = plan(&target(&["./gradlew", "-q", ":app:run"])).unwrap();
        assert_eq!(p.engine, Engine::Jfr);
        assert_eq!(
            p.command,
            ["./gradlew", "--init-script", "{init}", "-q", ":app:run"]
        );
        assert!(p.env.is_empty());
    }

    #[test]
    fn java_and_maven_exec_use_flight_recorder() {
        let p = plan(&target(&["/jdk/bin/java", "-cp", "out", "demo.Busy"])).unwrap();
        assert_eq!(p.engine, Engine::Jfr);
        assert_eq!(p.command[0], "/jdk/bin/java");
        assert!(p.command[2].starts_with("-XX:StartFlightRecording=filename={out},"));
        assert_eq!(&p.command[3..], ["-cp", "out", "demo.Busy"]);
        let m = plan(&target(&[
            "mvn",
            "-q",
            "compile",
            "exec:java",
            "-Dexec.mainClass=a.B",
        ]))
        .unwrap();
        assert_eq!(m.engine, Engine::Jfr);
        assert_eq!(
            m.command,
            ["mvn", "-q", "compile", "exec:java", "-Dexec.mainClass=a.B"]
        );
        assert!(m.env["MAVEN_OPTS"].contains("jdk.ExecutionSample#period=1ms"));
    }

    // SPEC-0120 v5 behaviours 10 and 11: a stored profile has its project frames marked and the
    // digests of its project files; one stored before has marks once read, but no digests.
    #[test]
    fn stored_profiles_keep_marks_and_digests() {
        let root = crate::project::tests::workspace("store", &["app.py"]);
        let dir = root.join(PROFILES_DIR);
        fs::create_dir_all(&dir).unwrap();
        let mut tree = crate::StackTree::default();
        let at = |name: &str, file: &str, line: u32| crate::StackFrame {
            name: name.into(),
            module: None,
            location: Some((file.into(), line)),
        };
        tree.add(
            "all",
            &[
                at("main", "app.py", 2),
                at("dumps", "/usr/lib/python3/json/__init__.py", 231),
            ],
            5.0,
        );
        let mut run = ProfileRun {
            plan: ProfilePlan {
                engine: Engine::CProfile,
                command: vec!["python3".into(), "app.py".into()],
                env: BTreeMap::new(),
            },
            exit_code: Some(0),
            timed_out: false,
            millis: 5,
            output: String::new(),
            profile: tree.profile("test", crate::Unit::Samples),
            kind: Kind::Cpu,
            exit_unknown: false,
            attached: None,
            sources: BTreeMap::new(),
            marked: false,
        };
        let older = serde_json::to_string(&run).unwrap();
        store(&dir, &mut run).unwrap();
        let stored = last_profile(&dir).unwrap();
        assert!(stored.marked);
        let main = &stored.profile.roots[0].children[0];
        assert!(main.in_project && !main.children[0].in_project);
        assert_eq!(
            stored.sources.get("app.py"),
            Some(&ferris_idea_core::sha256_hex(b"// app.py\n"))
        );
        assert_eq!(stored.sources.len(), 1, "only the project's files");
        fs::write(dir.join(LAST), older).unwrap();
        let read = last_profile(&dir).unwrap();
        assert!(read.marked && read.profile.roots[0].children[0].in_project);
        assert!(read.sources.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
