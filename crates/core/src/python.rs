//! Python interpreters, virtual environments and installed packages (`SPEC-0050`), read statically.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::project::Dependency;
use crate::run::{RunConfig, RunKind};

/// Where an interpreter comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InterpreterKind {
    /// A virtual environment (`pyvenv.cfg`).
    Venv,
    /// A conda environment.
    Conda,
    /// A pyenv installation.
    Pyenv,
    /// Any other interpreter on `PATH`.
    System,
}

/// One interpreter (behaviour 1–2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Interpreter {
    /// Path of the executable.
    pub path: String,
    /// Kind.
    pub kind: InterpreterKind,
    /// Version as far as it is known without starting the interpreter.
    pub version: Option<String>,
    /// Base interpreter folder of a virtual environment (`home` in `pyvenv.cfg`).
    pub base: Option<String>,
    /// Whether the environment lies inside the workspace.
    pub in_project: bool,
    /// How it was found: `VIRTUAL_ENV`, `CONDA_PREFIX`, `project`, `pyenv`, `PATH`, `configured`.
    pub found_by: String,
}

/// An installed distribution (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Package {
    /// Normalised name (PEP 503).
    pub name: String,
    /// Version.
    pub version: String,
}

/// A declared dependency that the selected environment does not satisfy (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unsatisfied {
    /// Declared name.
    pub name: String,
    /// Declared requirement.
    pub requirement: String,
    /// Installed version, if any.
    pub installed: Option<String>,
}

/// Inputs of the search; [`PythonSearch::from_env`] fills them from the process environment.
#[derive(Debug, Clone, Default)]
pub struct PythonSearch {
    /// Workspace root.
    pub root: PathBuf,
    /// `PATH`.
    pub path_var: Option<OsString>,
    /// `VIRTUAL_ENV`.
    pub virtual_env: Option<PathBuf>,
    /// `CONDA_PREFIX`.
    pub conda_prefix: Option<PathBuf>,
    /// pyenv root (`PYENV_ROOT` or `~/.pyenv`).
    pub pyenv_root: Option<PathBuf>,
    /// Interpreter chosen by the user in the settings.
    pub configured: Option<PathBuf>,
}

impl PythonSearch {
    /// Search inputs from the environment of this process.
    pub fn from_env(root: &Path) -> Self {
        let pyenv_root = std::env::var_os("PYENV_ROOT")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".pyenv")));
        Self {
            root: root.to_path_buf(),
            path_var: std::env::var_os("PATH"),
            virtual_env: std::env::var_os("VIRTUAL_ENV").map(PathBuf::from),
            conda_prefix: std::env::var_os("CONDA_PREFIX").map(PathBuf::from),
            pyenv_root,
            configured: None,
        }
    }
}

/// Result of [`python_environments`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PythonEnvironments {
    /// Interpreters found, without duplicates, in selection order of their source.
    pub interpreters: Vec<Interpreter>,
    /// Index of the selected interpreter.
    pub selected: Option<usize>,
    /// Why it was selected.
    pub reason: Option<String>,
    /// `requires-python` of the project.
    pub requires_python: Option<String>,
    /// Whether the selected interpreter's version satisfies `requires-python` (`None`: unknown).
    pub compatible: Option<bool>,
    /// Distributions installed in the selected environment.
    pub packages: Vec<Package>,
    /// Declared dependencies that are missing or have a non-matching version.
    pub unsatisfied: Vec<Unsatisfied>,
    /// Declared dependencies with environment markers, not checked.
    pub conditional: Vec<String>,
    /// `pytest` or `unittest`.
    pub test_framework: String,
    /// Suggested test run configuration (SPEC-0047).
    pub test_config: Option<RunConfig>,
    /// Remarks.
    pub notes: Vec<String>,
}

/// PEP 503 name normalisation.
pub fn normalize_name(name: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in name.trim().chars() {
        if matches!(c, '-' | '_' | '.') {
            sep = true;
        } else {
            if sep && !out.is_empty() {
                out.push('-');
            }
            sep = false;
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Executable of an environment folder (Unix `bin/`, Windows `Scripts/`).
fn env_python(prefix: &Path) -> Option<PathBuf> {
    [
        "bin/python",
        "bin/python3",
        "Scripts/python.exe",
        "python.exe",
    ]
    .iter()
    .map(|p| prefix.join(p))
    .find(|p| p.exists())
}

fn read_cfg(prefix: &Path) -> Option<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(prefix.join("pyvenv.cfg")).ok()?;
    Some(
        text.lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_owned()))
            .collect(),
    )
}

/// `X.Y` from `lib/pythonX.Y` of a prefix.
fn lib_version(prefix: &Path) -> Option<String> {
    let mut found: Vec<String> = std::fs::read_dir(prefix.join("lib"))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let v = n.strip_prefix("python")?;
            (v.contains('.') && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
                .then(|| v.to_owned())
        })
        .collect();
    found.sort_by(|a, b| compare(&parse_version(a), &parse_version(b)));
    found.pop()
}

fn venv(prefix: &Path, root: &Path, found_by: &str) -> Option<Interpreter> {
    let cfg = read_cfg(prefix)?;
    let python = env_python(prefix)?;
    Some(Interpreter {
        path: python.to_string_lossy().into_owned(),
        kind: InterpreterKind::Venv,
        version: cfg
            .get("version")
            .or_else(|| cfg.get("version_info"))
            .cloned()
            .or_else(|| lib_version(prefix)),
        base: cfg.get("home").cloned(),
        in_project: prefix.starts_with(root),
        found_by: found_by.to_owned(),
    })
}

fn plain(python: PathBuf, kind: InterpreterKind, root: &Path, found_by: &str) -> Interpreter {
    let name = python
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let from_name = name
        .strip_prefix("python")
        .map(|v| v.trim_end_matches(".exe"))
        .filter(|v| v.contains('.'))
        .map(str::to_owned);
    let resolved = std::fs::canonicalize(&python).unwrap_or_else(|_| python.clone());
    let prefix = resolved.parent().and_then(Path::parent);
    let version = from_name.or_else(|| prefix.and_then(lib_version));
    Interpreter {
        path: python.to_string_lossy().into_owned(),
        kind,
        version,
        base: None,
        in_project: python.starts_with(root),
        found_by: found_by.to_owned(),
    }
}

fn prefix_interpreter(prefix: &Path, root: &Path, found_by: &str) -> Option<Interpreter> {
    if let Some(v) = venv(prefix, root, found_by) {
        return Some(v);
    }
    let python = env_python(prefix)?;
    let kind = if prefix.join("conda-meta").is_dir() {
        InterpreterKind::Conda
    } else {
        InterpreterKind::System
    };
    Some(plain(python, kind, root, found_by))
}

/// Finds interpreters (behaviour 1–2); duplicates (same resolved file) are dropped.
pub fn find_interpreters(search: &PythonSearch) -> Vec<Interpreter> {
    let root = &search.root;
    let mut out: Vec<Interpreter> = Vec::new();
    if let Some(c) = &search.configured {
        let c = if c.is_absolute() {
            c.clone()
        } else {
            root.join(c)
        };
        if c.is_dir() {
            out.extend(prefix_interpreter(&c, root, "configured"));
        } else if c.exists() {
            let prefix = c.parent().and_then(Path::parent);
            out.push(
                prefix
                    .and_then(|p| venv(p, root, "configured"))
                    .unwrap_or_else(|| {
                        plain(c.clone(), InterpreterKind::System, root, "configured")
                    }),
            );
        }
    }
    if let Some(v) = &search.virtual_env {
        out.extend(prefix_interpreter(v, root, "VIRTUAL_ENV"));
    }
    // Project environments: direct sub-folders with `pyvenv.cfg`, `.venv` and `venv` first.
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.join("pyvenv.cfg").is_file())
                .collect()
        })
        .unwrap_or_default();
    let rank = |p: &PathBuf| match p.file_name().and_then(|n| n.to_str()) {
        Some(".venv") => 0,
        Some("venv") => 1,
        _ => 2,
    };
    dirs.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    for d in dirs {
        out.extend(venv(&d, root, "project"));
    }
    if let Some(c) = &search.conda_prefix {
        out.extend(prefix_interpreter(c, root, "CONDA_PREFIX"));
    }
    if let Some(pyenv) = &search.pyenv_root {
        let mut versions: Vec<PathBuf> = std::fs::read_dir(pyenv.join("versions"))
            .map(|r| r.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        versions.sort_by(|a, b| {
            let v =
                |p: &PathBuf| parse_version(&p.file_name().unwrap_or_default().to_string_lossy());
            compare(&v(b), &v(a))
        });
        for v in versions {
            if let Some(python) = env_python(&v) {
                let mut i = plain(python, InterpreterKind::Pyenv, root, "pyenv");
                if let Some(name) = v.file_name() {
                    let name = name.to_string_lossy();
                    if name.starts_with(|c: char| c.is_ascii_digit()) {
                        i.version = Some(name.into_owned());
                    }
                }
                out.push(i);
            }
        }
    }
    if let Some(path) = &search.path_var {
        for dir in std::env::split_paths(path) {
            for name in ["python3", "python", "python3.exe", "python.exe"] {
                let p = dir.join(name);
                if p.is_file() {
                    out.push(plain(p, InterpreterKind::System, root, "PATH"));
                }
            }
        }
    }
    let mut seen = Vec::new();
    out.retain(|i| {
        // A virtual environment's executable links to its base interpreter, so it is told apart by
        // its own path; other interpreters by the resolved file.
        let id = if i.kind == InterpreterKind::Venv {
            PathBuf::from(&i.path)
        } else {
            std::fs::canonicalize(&i.path).unwrap_or_else(|_| PathBuf::from(&i.path))
        };
        if seen.contains(&id) {
            false
        } else {
            seen.push(id);
            true
        }
    });
    out
}

/// Site-packages folders of the environment that contains `python`.
fn site_packages(python: &Path) -> Vec<PathBuf> {
    let Some(prefix) = python.parent().and_then(Path::parent) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    if let Ok(r) = std::fs::read_dir(prefix.join("lib")) {
        for e in r.flatten() {
            if e.file_name().to_string_lossy().starts_with("python") {
                dirs.push(e.path().join("site-packages"));
            }
        }
    }
    dirs.push(prefix.join("Lib/site-packages"));
    dirs.retain(|d| d.is_dir());
    dirs.sort();
    dirs
}

/// What the files of an environment say about installing into it (`SPEC-0171` v2 behaviour 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnvironmentTools {
    /// `pyvenv.cfg` has a `uv = …` line: the environment was made by `uv venv`.
    pub made_by_uv: bool,
    /// A site-packages folder exists and none holds a `pip` package: `python -m pip` cannot run.
    pub lacks_pip: bool,
}

/// Reads [`EnvironmentTools`] for the environment that contains `python` from `pyvenv.cfg` and
/// the site-packages folders; the interpreter is not started (IMP-0025). Without any
/// site-packages folder nothing is known about pip, and it is not reported as lacking.
pub fn environment_tools(python: &Path) -> EnvironmentTools {
    let made_by_uv = python
        .parent()
        .and_then(Path::parent)
        .and_then(read_cfg)
        .is_some_and(|cfg| cfg.contains_key("uv"));
    let sites = site_packages(python);
    EnvironmentTools {
        made_by_uv,
        lacks_pip: !sites.is_empty() && !sites.iter().any(|d| d.join("pip").is_dir()),
    }
}

/// Installed distributions from `*.dist-info` and `*.egg-info` folders (behaviour 4).
pub fn installed_packages(python: &Path) -> Vec<Package> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for dir in site_packages(python) {
        let Ok(r) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in r.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let Some(stem) = n
                .strip_suffix(".dist-info")
                .or_else(|| n.strip_suffix(".egg-info"))
            else {
                continue;
            };
            // `name-version`, the name escaped with `_` (wheel binary format); METADATA wins when readable.
            let meta = ["METADATA", "PKG-INFO"]
                .iter()
                .find_map(|m| std::fs::read_to_string(e.path().join(m)).ok());
            let field = |key: &str| {
                meta.as_deref().and_then(|m| {
                    m.lines()
                        .take_while(|l| !l.is_empty())
                        .find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_owned()))
                })
            };
            let (dir_name, dir_version) = match stem.split_once('-') {
                Some((a, b)) => (a.to_owned(), b.split('-').next().unwrap_or(b).to_owned()),
                None => (stem.to_owned(), String::new()),
            };
            let name = field("Name:").unwrap_or(dir_name);
            let version = field("Version:").unwrap_or(dir_version);
            out.insert(normalize_name(&name), version);
        }
    }
    out.into_iter()
        .map(|(name, version)| Package { name, version })
        .collect()
}

// ---------- Versions (PEP 440 release segment; pre/post/dev tags ordered before/after) ----------

#[derive(Debug, Clone, PartialEq)]
struct Version {
    release: Vec<u64>,
    /// -1 for pre-releases and dev releases, 0 final, 1 post-releases.
    tag: i8,
}

fn parse_version(text: &str) -> Version {
    let t = text.trim().trim_start_matches(['v', 'V']);
    let t = t.split('+').next().unwrap_or(t);
    let mut release = Vec::new();
    let mut rest = "";
    for part in t.split('.') {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            rest = part;
            break;
        }
        release.push(digits.parse().unwrap_or(u64::MAX));
        if digits.len() < part.len() {
            rest = &part[digits.len()..];
            break;
        }
    }
    let rest = rest.to_lowercase();
    let tag = if rest.contains("post") || rest.starts_with('-') {
        1
    } else if rest.is_empty() {
        0
    } else {
        -1
    };
    Version { release, tag }
}

fn compare(a: &Version, b: &Version) -> Ordering {
    let n = a.release.len().max(b.release.len());
    for i in 0..n {
        let x = a.release.get(i).copied().unwrap_or(0);
        let y = b.release.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    a.tag.cmp(&b.tag)
}

fn bump(release: &[u64], keep: usize) -> Version {
    let mut r: Vec<u64> = release.iter().take(keep.max(1)).copied().collect();
    while r.len() < keep.max(1) {
        r.push(0);
    }
    if let Some(last) = r.last_mut() {
        *last += 1;
    }
    Version {
        release: r,
        tag: -1,
    }
}

fn clause(version: &Version, op: &str, v: &str) -> Option<bool> {
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(prefix) = v.strip_suffix(".*") {
        let p = parse_version(prefix).release;
        let matches = version.release.len() >= p.len() && version.release[..p.len()] == p[..];
        return match op {
            "==" => Some(matches),
            "!=" => Some(!matches),
            _ => None,
        };
    }
    let w = parse_version(v);
    if w.release.is_empty() {
        return None;
    }
    let c = compare(version, &w);
    Some(match op {
        "==" | "===" => c == Ordering::Equal,
        "!=" => c != Ordering::Equal,
        ">=" => c != Ordering::Less,
        "<=" => c != Ordering::Greater,
        ">" => c == Ordering::Greater,
        "<" => c == Ordering::Less,
        "~=" => {
            let keep = w.release.len().saturating_sub(1);
            c != Ordering::Less && compare(version, &bump(&w.release, keep)) == Ordering::Less
        }
        // Poetry caret: up to the next change of the first non-zero segment.
        "^" => {
            let first = w
                .release
                .iter()
                .position(|&x| x != 0)
                .unwrap_or(w.release.len().saturating_sub(1));
            c != Ordering::Less && compare(version, &bump(&w.release, first + 1)) == Ordering::Less
        }
        // Poetry tilde: next minor (or next major when only the major is given).
        "~" => {
            let keep = if w.release.len() >= 2 { 2 } else { 1 };
            c != Ordering::Less && compare(version, &bump(&w.release, keep)) == Ordering::Less
        }
        _ => return None,
    })
}

/// Whether `version` satisfies a PEP 440 specifier set (also Poetry `^`/`~`); `None` if not understood.
pub fn satisfies(version: &str, spec: &str) -> Option<bool> {
    let spec = spec.trim();
    if spec.is_empty() || spec == "*" {
        return Some(true);
    }
    let v = parse_version(version);
    let mut all = true;
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() || part == "*" {
            continue;
        }
        let op_len = part
            .find(|c: char| !matches!(c, '=' | '!' | '<' | '>' | '~' | '^'))
            .unwrap_or(part.len());
        let (op, rest) = part.split_at(op_len);
        let op = if op.is_empty() { "==" } else { op };
        all &= clause(&v, op, rest)?;
    }
    Some(all)
}

/// Picks the interpreter (behaviour 3).
fn select(interpreters: &[Interpreter], root: &Path) -> (Option<usize>, Option<String>) {
    let by = |what: &str| interpreters.iter().position(|i| i.found_by == what);
    if let Some(i) = by("configured") {
        return (Some(i), Some("chosen in the settings".into()));
    }
    if let Some(i) = by("VIRTUAL_ENV") {
        return (
            Some(i),
            Some("active virtual environment (VIRTUAL_ENV)".into()),
        );
    }
    if let Some(i) = by("project") {
        return (Some(i), Some("virtual environment in the project".into()));
    }
    if let Ok(text) = std::fs::read_to_string(root.join(".python-version")) {
        let wanted = text.lines().next().unwrap_or("").trim().to_owned();
        if let Some(i) = interpreters.iter().position(|i| {
            i.kind == InterpreterKind::Pyenv
                && i.version
                    .as_deref()
                    .is_some_and(|v| v == wanted || v.starts_with(&format!("{wanted}.")))
        }) {
            return (Some(i), Some(format!(".python-version asks for {wanted}")));
        }
    }
    if let Some(i) = by("CONDA_PREFIX") {
        return (
            Some(i),
            Some("active conda environment (CONDA_PREFIX)".into()),
        );
    }
    if let Some(i) = by("PATH") {
        return (Some(i), Some("first interpreter on PATH".into()));
    }
    by("pyenv").map_or((None, None), |i| {
        (Some(i), Some("newest pyenv version".into()))
    })
}

/// The selected interpreter and why, without reading packages or declarations (SPEC-0050
/// behaviour 3); used where only the interpreter matters (SPEC-0170).
pub fn selected_interpreter(search: &PythonSearch) -> Option<(Interpreter, String)> {
    let mut interpreters = find_interpreters(search);
    let (selected, reason) = select(&interpreters, &search.root);
    let i = selected?;
    Some((interpreters.swap_remove(i), reason.unwrap_or_default()))
}

/// `python` as the first word of a run command: `${workspace}/…` inside the workspace `root`, else
/// as it is (behaviour 6, v2 behaviour 8).
pub(crate) fn command_word(python: &Path, root: &Path) -> String {
    python
        .strip_prefix(root)
        .map(|p| format!("${{workspace}}/{}", p.to_string_lossy()))
        .unwrap_or_else(|_| python.to_string_lossy().into_owned())
}

/// Interpreters, selection, installed packages and the check against the declared project
/// (`requires_python`, dependencies from SPEC-0048) (behaviour 1–6).
pub fn python_environments(
    search: &PythonSearch,
    requires_python: Option<&str>,
    declared: &[Dependency],
) -> PythonEnvironments {
    let interpreters = find_interpreters(search);
    let (selected, reason) = select(&interpreters, &search.root);
    let mut notes = Vec::new();
    let chosen = selected.map(|i| &interpreters[i]);
    let compatible = match (chosen.and_then(|i| i.version.as_deref()), requires_python) {
        (Some(v), Some(spec)) => satisfies(v, spec),
        _ => None,
    };
    if compatible == Some(false) {
        notes.push(format!(
            "the selected interpreter ({}) does not satisfy requires-python {}",
            chosen.and_then(|i| i.version.clone()).unwrap_or_default(),
            requires_python.unwrap_or_default()
        ));
    }
    if chosen.is_some_and(|i| i.version.is_none()) {
        notes
            .push("the version of the selected interpreter is only known after starting it".into());
    }
    let packages = chosen
        .map(|i| installed_packages(Path::new(&i.path)))
        .unwrap_or_default();
    let is_env = chosen.is_some_and(|i| i.kind != InterpreterKind::System);
    let mut unsatisfied = Vec::new();
    let mut conditional = Vec::new();
    if is_env {
        for d in declared {
            if d.requirement.contains(';') {
                conditional.push(format!("{} {}", d.name, d.requirement));
                continue;
            }
            if d.requirement.starts_with("path:")
                || d.requirement.starts_with("git:")
                || d.requirement == "workspace"
            {
                continue;
            }
            let installed = packages
                .iter()
                .find(|p| p.name == normalize_name(&d.name))
                .map(|p| p.version.clone());
            // Drop extras (`[docs]`) before checking the version.
            let spec = match d.requirement.find(']') {
                Some(end) if d.requirement.starts_with('[') => d.requirement[end + 1..].trim(),
                _ => d.requirement.trim(),
            };
            let ok = installed
                .as_deref()
                .is_some_and(|v| satisfies(v, spec).unwrap_or(true));
            if !ok {
                unsatisfied.push(Unsatisfied {
                    name: d.name.clone(),
                    requirement: d.requirement.clone(),
                    installed,
                });
            }
        }
    } else if chosen.is_some() && !declared.is_empty() {
        notes.push(
            "the selected interpreter is not an environment; installed packages are not checked"
                .into(),
        );
    }
    let pytest = packages.iter().any(|p| p.name == "pytest")
        || declared.iter().any(|d| normalize_name(&d.name) == "pytest");
    let test_framework = if pytest { "pytest" } else { "unittest" }.to_owned();
    let test_config = chosen.map(|i| {
        let python = command_word(Path::new(&i.path), &search.root);
        if pytest {
            RunConfig {
                name: "pytest".into(),
                command: vec![
                    python,
                    "-m".into(),
                    "pytest".into(),
                    "--junitxml=.ferris-idea/pytest.xml".into(),
                ],
                cwd: None,
                env: BTreeMap::new(),
                kind: RunKind::Test,
                report: Some("junit".into()),
                report_file: Some(".ferris-idea/pytest.xml".into()),
                before: Vec::new(),
                timeout: None,
                url: None,
                folder: None,
                allow_multiple: false,
                before_launch: Vec::new(),
                verbatim: false,
                http: None,
            }
        } else {
            RunConfig {
                name: "unittest".into(),
                command: vec![python, "-m".into(), "unittest".into(), "-v".into()],
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
    });
    PythonEnvironments {
        interpreters,
        selected,
        reason,
        requires_python: requires_python.map(str::to_owned),
        compatible,
        packages,
        unsatisfied,
        conditional,
        test_framework,
        test_config,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specifiers() {
        let ok = |v: &str, s: &str| satisfies(v, s).unwrap();
        assert!(ok("3.11.4", ">=3.9"));
        assert!(!ok("3.8.10", ">=3.9"));
        assert!(ok("3.11.4", ">=3.9,<3.13"));
        assert!(!ok("3.13.0", ">=3.9,<3.13"));
        assert!(ok("3.11.4", "==3.11.*"));
        assert!(!ok("3.12.0", "==3.11.*"));
        assert!(ok("2.2.1", "~=2.2"));
        assert!(!ok("3.0", "~=2.2"));
        assert!(ok("1.4.2", "~=1.4.1"));
        assert!(!ok("1.5.0", "~=1.4.1"));
        assert!(ok("1.9", "^1.2"));
        assert!(!ok("2.0", "^1.2"));
        assert!(ok("0.2.9", "^0.2.3"));
        assert!(!ok("0.3.0", "^0.2.3"));
        assert!(ok("1.2.9", "~1.2"));
        assert!(!ok("1.3", "~1.2"));
        assert!(ok("3.0.3", "3.0.3"));
        assert!(!ok("3.1.0rc1", ">=3.1"));
        assert!(ok("3.1.0.post1", ">3.1.0"));
        assert!(ok("4.2", "*"));
        assert_eq!(satisfies("1.0", "@1"), None);
    }

    #[test]
    fn names() {
        assert_eq!(normalize_name("Flask_SQLAlchemy"), "flask-sqlalchemy");
        assert_eq!(normalize_name("zope.interface"), "zope-interface");
        assert_eq!(normalize_name("A--b__c"), "a-b-c");
    }
}
