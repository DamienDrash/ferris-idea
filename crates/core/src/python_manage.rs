//! Choosing the project's Python interpreter, creating a virtual environment and installing
//! packages (`SPEC-0117`), on top of the static model of `SPEC-0050`.
//!
//! The choice is kept in `.idecraft/python.toml`, workspace-relative when the interpreter lies
//! in the workspace. Commands are plain argument lists run without a shell.
//!
//! Every Python launch resolves its interpreter here (`SPEC-0050` v2 behaviour 8):
//! [`workspace_python`], [`python_command`] and [`show_python`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::python::{
    PythonEnvironments, PythonSearch, Unsatisfied, command_word, normalize_name,
    selected_interpreter,
};
use crate::run::RunConfig;

/// Settings file of the chosen interpreter, relative to the workspace.
pub const PYTHON_SETTINGS: &str = ".idecraft/python.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Settings {
    interpreter: Option<String>,
}

/// Format 1 of `python.toml` (`SPEC-0303`): `[python] interpreter`; format 0 had the key at the
/// top and is migrated when read.
#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    python: Settings,
}

/// The interpreter chosen for the workspace, as written (absolute or workspace-relative).
pub fn configured_interpreter(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join(PYTHON_SETTINGS)).ok()?;
    let doc = crate::format::read(&text, Some("python"), crate::format::Use::Project).ok()?;
    let file: File = doc.body_as().ok()?;
    file.python
        .interpreter
        .filter(|i| !i.trim().is_empty())
        .map(PathBuf::from)
}

/// Saves `interpreter` as the workspace's choice (behaviour 2); `None` returns to the automatic
/// choice. A path inside the workspace is stored relative to it. Returns what was written.
pub fn save_interpreter(
    root: &Path,
    interpreter: Option<&Path>,
) -> std::io::Result<Option<String>> {
    let file = root.join(PYTHON_SETTINGS);
    let Some(interpreter) = interpreter else {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => return Ok(None),
        }
    };
    let stored = interpreter
        .strip_prefix(root)
        .unwrap_or(interpreter)
        .to_string_lossy()
        .into_owned();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::format::prepare_write(&file, "python")?;
    let text = crate::format::write(
        &crate::format::Envelope::current("python"),
        &File {
            python: Settings {
                interpreter: Some(stored.clone()),
            },
        },
    )
    .map_err(std::io::Error::other)?;
    std::fs::write(
        &file,
        format!("# Python interpreter of this workspace (SPEC-0117)\n{text}"),
    )?;
    Ok(Some(stored))
}

impl PythonSearch {
    /// Search inputs from the process environment plus the workspace's saved choice.
    pub fn for_workspace(root: &Path) -> Self {
        let mut search = Self::from_env(root);
        search.configured = configured_interpreter(root);
        search
    }
}

/// Why a Python start cannot happen (`SPEC-0050` v2 behaviour 8, IMP-0394).
pub const NO_PYTHON: &str = "no Python interpreter found (none chosen, no virtual environment in the project, \
     no python3 on PATH): create one with \"New virtual environment in .venv\" or choose one in the \
     Python interpreter widget of the status bar";

/// Whether `program` is a bare `python`/`python3`, which stands for the workspace's selected
/// interpreter; paths and other names (`python3.11`) do not (`SPEC-0050` v2 behaviour 8).
pub fn is_bare_python(program: &str) -> bool {
    matches!(program, "python" | "python3")
}

/// The selected interpreter of the workspace `root`: the stored choice, the active or project
/// environment, … (`SPEC-0050` behaviour 3). `path_var` replaces the process's `PATH` for the search
/// (a `PATH` set in a run configuration). Err: [`NO_PYTHON`].
pub fn workspace_python(root: &Path, path_var: Option<OsString>) -> Result<PathBuf, String> {
    let mut search = PythonSearch::for_workspace(root);
    if path_var.is_some() {
        search.path_var = path_var;
    }
    selected_path(&search)
}

fn selected_path(search: &PythonSearch) -> Result<PathBuf, String> {
    selected_interpreter(search)
        .map(|(i, _)| PathBuf::from(i.path))
        .ok_or_else(|| NO_PYTHON.to_owned())
}

/// `command` with a leading bare `python`/`python3` replaced by the absolute path of the
/// workspace's selected interpreter; `Ok(None)` when the command does not start with one.
pub fn python_command(
    root: &Path,
    command: &[String],
    path_var: Option<OsString>,
) -> Result<Option<Vec<String>>, String> {
    if !command.first().is_some_and(|p| is_bare_python(p)) {
        return Ok(None);
    }
    let python = workspace_python(root, path_var)?;
    let mut out = command.to_vec();
    out[0] = python.to_string_lossy().into_owned();
    Ok(Some(out))
}

/// `config` as it will run, for showing it (MCP `project_model`): a leading bare `python`/`python3`
/// becomes the selected interpreter, `${workspace}/…` inside the workspace. Left as it is when no
/// interpreter is found (the start then says why) or for a configuration run as given.
pub fn show_python(root: &Path, config: &mut RunConfig) {
    if config.verbatim || !config.command.first().is_some_and(|p| is_bare_python(p)) {
        return;
    }
    if let Ok(python) = workspace_python(root, None) {
        config.command[0] = command_word(&python, root);
    }
}

/// Short label of the selected interpreter for the status bar (behaviour 1), e.g.
/// `Python 3.12 (.venv)`; `No interpreter` when none was found.
pub fn interpreter_label(env: &PythonEnvironments, root: &Path) -> String {
    let Some(i) = env.selected.and_then(|i| env.interpreters.get(i)) else {
        return "No interpreter".into();
    };
    let version = i
        .version
        .as_deref()
        .map(|v| v.split('.').take(2).collect::<Vec<_>>().join("."))
        .map_or_else(|| "Python".to_owned(), |v| format!("Python {v}"));
    match environment_name(Path::new(&i.path), root) {
        Some(name) => format!("{version} ({name})"),
        None => version,
    }
}

/// Name of the environment folder of `python` (`.venv`, `env`, a conda env name), if it is one.
fn environment_name(python: &Path, root: &Path) -> Option<String> {
    let prefix = python.parent()?.parent()?;
    if !(prefix.join("pyvenv.cfg").is_file() || prefix.join("conda-meta").is_dir()) {
        return None;
    }
    prefix
        .strip_prefix(root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| prefix.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// One line per interpreter for the chooser: label, kind, where it was found and the path.
pub fn interpreter_choices(env: &PythonEnvironments) -> Vec<String> {
    env.interpreters
        .iter()
        .enumerate()
        .map(|(n, i)| {
            let mark = if env.selected == Some(n) {
                "● "
            } else {
                "  "
            };
            let version = i.version.as_deref().unwrap_or("?");
            format!(
                "{mark}Python {version} · {} · {} — {}",
                serde_json::to_value(i.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                i.found_by,
                i.path
            )
        })
        .collect()
}

/// Base interpreter for a new environment: the selected one when it is not itself an environment,
/// else its base, else `python3` (behaviour 3).
pub fn venv_base(env: &PythonEnvironments) -> PathBuf {
    let chosen = env.selected.and_then(|i| env.interpreters.get(i));
    if let Some(i) = chosen {
        if i.kind != crate::python::InterpreterKind::Venv {
            return PathBuf::from(&i.path);
        }
        if let Some(home) = &i.base {
            for name in ["python3", "python", "python.exe"] {
                let p = Path::new(home).join(name);
                if p.is_file() {
                    return p;
                }
            }
        }
    }
    PathBuf::from("python3")
}

/// `<base> -m venv <target>` (behaviour 3).
pub fn venv_command(base: &Path, target: &Path) -> Vec<String> {
    vec![
        base.to_string_lossy().into_owned(),
        "-m".into(),
        "venv".into(),
        target.to_string_lossy().into_owned(),
    ]
}

/// The interpreter inside a new environment folder.
pub fn venv_python(target: &Path) -> PathBuf {
    if cfg!(windows) {
        target.join("Scripts").join("python.exe")
    } else {
        target.join("bin").join("python")
    }
}

/// `<python> -m pip install …` for `specs` (behaviour 5).
pub fn pip_install_command(python: &Path, specs: &[String]) -> Vec<String> {
    let mut command = vec![
        python.to_string_lossy().into_owned(),
        "-m".into(),
        "pip".into(),
        "install".into(),
        "--disable-pip-version-check".into(),
    ];
    command.extend(specs.iter().cloned());
    command
}

/// A pip requirement for an unsatisfied dependency: PEP 440 specifiers are kept, extras go after
/// the name, Poetry's `^`/`~` and `*` become the bare name (pip cannot read them).
pub fn pip_spec(dep: &Unsatisfied) -> String {
    let req = dep.requirement.trim();
    let (extras, spec) = match req.find(']') {
        Some(end) if req.starts_with('[') => (&req[..=end], req[end + 1..].trim()),
        _ => ("", req),
    };
    let pep440 = spec.starts_with(['=', '!', '<', '>']) || spec.starts_with("~=");
    if pep440 {
        format!("{}{extras}{spec}", dep.name)
    } else {
        format!("{}{extras}", dep.name)
    }
}

/// Splits what the user typed into requirements at spaces; commas inside a requirement stay, so
/// `requests>=2,<3 rich` gives two.
pub fn parse_specs(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Rules of pyright that mark an import it cannot resolve (`SPEC-0117` v4 behaviour 8).
pub const MISSING_IMPORT_RULES: [&str; 2] = ["reportMissingImports", "reportMissingModuleSource"];

/// Well-known distributions whose import name differs from their own name, as (import,
/// distribution) (`SPEC-0117` v4 behaviour 8 b). Dotted imports match their submodules too.
pub const KNOWN_DISTRIBUTIONS: &[(&str, &str)] = &[
    ("yaml", "PyYAML"),
    ("PIL", "Pillow"),
    ("cv2", "opencv-python"),
    ("sklearn", "scikit-learn"),
    ("bs4", "beautifulsoup4"),
    ("dateutil", "python-dateutil"),
    ("dotenv", "python-dotenv"),
    ("jwt", "PyJWT"),
    ("serial", "pyserial"),
    ("usb", "pyusb"),
    ("Crypto", "pycryptodome"),
    ("zmq", "pyzmq"),
    ("OpenSSL", "pyOpenSSL"),
    ("MySQLdb", "mysqlclient"),
    ("docx", "python-docx"),
    ("pptx", "python-pptx"),
    ("magic", "python-magic"),
    ("attr", "attrs"),
    ("google.protobuf", "protobuf"),
];

/// Where the distribution of an import was found (`SPEC-0117` v4 behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageSource {
    /// A declared dependency the environment does not satisfy.
    Requirement,
    /// [`KNOWN_DISTRIBUTIONS`].
    Known,
    /// The import name itself.
    Import,
}

/// The distribution that provides an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportPackage {
    /// The module as written, e.g. `yaml` or `google.protobuf.message`.
    pub import: String,
    /// Distribution name, e.g. `PyYAML`.
    pub package: String,
    /// What pip installs: the declared requirement (`PyYAML>=6`) or the distribution name.
    pub spec: String,
    /// Where the distribution was found.
    pub source: PackageSource,
}

fn is_identifier(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// The distribution for the module `module` of an import (`SPEC-0117` v4 behaviour 8): a declared
/// dependency in `unsatisfied` first (by PEP 503 name of the import or of the well-known
/// distribution), then [`KNOWN_DISTRIBUTIONS`], then the import name. `None` for a relative import
/// or text that is no module name.
pub fn import_package(module: &str, unsatisfied: &[Unsatisfied]) -> Option<ImportPackage> {
    let module = module.trim();
    if !module.split('.').all(is_identifier) {
        return None;
    }
    let top = module.split('.').next()?;
    let known = KNOWN_DISTRIBUTIONS
        .iter()
        .filter(|(import, _)| {
            module == *import
                || module
                    .strip_prefix(import)
                    .is_some_and(|rest| rest.starts_with('.'))
        })
        .max_by_key(|(import, _)| import.len())
        .map(|(_, dist)| *dist);
    let names = [Some(top), known];
    let declared = unsatisfied.iter().find(|u| {
        let name = normalize_name(&u.name);
        names.iter().flatten().any(|n| normalize_name(n) == name)
    });
    let (package, spec, source) = match (declared, known) {
        (Some(u), _) => (u.name.clone(), pip_spec(u), PackageSource::Requirement),
        (None, Some(k)) => (k.to_owned(), k.to_owned(), PackageSource::Known),
        (None, None) => (top.to_owned(), top.to_owned(), PackageSource::Import),
    };
    Some(ImportPackage {
        import: module.to_owned(),
        package,
        spec,
        source,
    })
}

/// One install entry of the ⌥↩ list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallFix {
    /// Title, e.g. `Install package PyYAML`.
    pub title: String,
    /// Requirements pip installs.
    pub specs: Vec<String>,
}

/// The install entries for the unresolved imports `modules` (`SPEC-0117` v4 behaviour 8): one
/// `Install package <distribution>` per distribution, then `Install all missing requirements (n)`
/// when two or more declared dependencies are unsatisfied. Empty without an importable module.
pub fn install_fixes(modules: &[String], unsatisfied: &[Unsatisfied]) -> Vec<InstallFix> {
    let mut out: Vec<InstallFix> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for found in modules
        .iter()
        .filter_map(|m| import_package(m, unsatisfied))
    {
        let key = normalize_name(&found.package);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(InstallFix {
            title: format!("Install package {}", found.package),
            specs: vec![found.spec],
        });
    }
    if !out.is_empty() && unsatisfied.len() >= 2 {
        out.push(InstallFix {
            title: format!("Install all missing requirements ({})", unsatisfied.len()),
            specs: unsatisfied.iter().map(pip_spec).collect(),
        });
    }
    out
}

/// Outcome of a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolRun {
    /// The command line.
    pub command: Vec<String>,
    /// Whether it exited with 0.
    pub ok: bool,
    /// Exit code, if it exited.
    pub exit_code: Option<i32>,
    /// Last lines of stdout and stderr (at most 40).
    pub output: String,
}

/// Runs `command` in `cwd` and waits for it (no shell; meant for background threads).
pub fn run_tool(command: &[String], cwd: &Path) -> ToolRun {
    let Some((program, args)) = command.split_first() else {
        return ToolRun {
            command: Vec::new(),
            ok: false,
            exit_code: None,
            output: "empty command".into(),
        };
    };
    match Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("PIP_NO_INPUT", "1")
        .output()
    {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            let tail = lines[lines.len().saturating_sub(40)..].join("\n");
            ToolRun {
                command: command.to_vec(),
                ok: out.status.success(),
                exit_code: out.status.code(),
                output: tail,
            }
        }
        Err(e) => ToolRun {
            command: command.to_vec(),
            ok: false,
            exit_code: None,
            output: format!("{program}: {e}"),
        },
    }
}

/// The last non-empty line of `run`'s output, for a one-line report.
pub fn last_line(run: &ToolRun) -> &str {
    run.output.lines().last().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::python::{Interpreter, InterpreterKind};

    fn temp(tag: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("idecraft-pymanage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn env_with(interpreters: Vec<Interpreter>, selected: Option<usize>) -> PythonEnvironments {
        PythonEnvironments {
            interpreters,
            selected,
            reason: None,
            requires_python: None,
            compatible: None,
            packages: Vec::new(),
            unsatisfied: Vec::new(),
            conditional: Vec::new(),
            test_framework: "unittest".into(),
            test_config: None,
            notes: Vec::new(),
        }
    }

    fn interp(
        path: &str,
        kind: InterpreterKind,
        version: Option<&str>,
        base: Option<&str>,
    ) -> Interpreter {
        Interpreter {
            path: path.into(),
            kind,
            version: version.map(str::to_owned),
            base: base.map(str::to_owned),
            in_project: false,
            found_by: "project".into(),
        }
    }

    #[test]
    fn saves_relative_choices_and_clears_them() {
        let root = temp("save");
        assert_eq!(configured_interpreter(&root), None);
        let stored = save_interpreter(&root, Some(&root.join(".venv/bin/python"))).unwrap();
        assert_eq!(stored.as_deref(), Some(".venv/bin/python"));
        assert_eq!(
            configured_interpreter(&root),
            Some(PathBuf::from(".venv/bin/python"))
        );
        save_interpreter(&root, Some(Path::new("/usr/bin/python3"))).unwrap();
        assert_eq!(
            PythonSearch::for_workspace(&root).configured,
            Some(PathBuf::from("/usr/bin/python3"))
        );
        assert_eq!(save_interpreter(&root, None).unwrap(), None);
        assert_eq!(configured_interpreter(&root), None);
        assert_eq!(
            save_interpreter(&root, None).unwrap(),
            None,
            "clearing twice is fine"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// SPEC-0303 AK-5: a `python.toml` of format 0 is read, and saving keeps it once as `.bak`.
    #[test]
    fn old_python_files_are_read_and_migrated() {
        let root = temp("migrate");
        let file = root.join(PYTHON_SETTINGS);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "interpreter = \".venv/bin/python\"\n").unwrap();
        assert_eq!(
            configured_interpreter(&root),
            Some(PathBuf::from(".venv/bin/python"))
        );
        save_interpreter(&root, Some(&root.join("env/bin/python"))).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("[idecraft]\nkind = \"python\"\nformat = 1\n"),
            "{text}"
        );
        assert!(
            text.contains("[python]\ninterpreter = \"env/bin/python\""),
            "{text}"
        );
        assert!(
            std::fs::read_to_string(root.join(".idecraft/python.toml.bak"))
                .unwrap()
                .contains(".venv")
        );
        assert_eq!(
            configured_interpreter(&root),
            Some(PathBuf::from("env/bin/python"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn labels_name_version_and_environment() {
        let root = temp("label");
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        std::fs::write(root.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
        let venv = root.join(".venv/bin/python").display().to_string();
        let e = env_with(
            vec![
                interp(
                    &venv,
                    InterpreterKind::Venv,
                    Some("3.12.4"),
                    Some("/usr/bin"),
                ),
                interp("/usr/bin/python3", InterpreterKind::System, None, None),
            ],
            Some(0),
        );
        assert_eq!(interpreter_label(&e, &root), "Python 3.12 (.venv)");
        let choices = interpreter_choices(&e);
        assert!(
            choices[0].starts_with("● Python 3.12.4 · venv · project — "),
            "{choices:?}"
        );
        assert!(choices[1].starts_with("  Python ? · system"), "{choices:?}");
        let system = env_with(e.interpreters.clone(), Some(1));
        assert_eq!(interpreter_label(&system, &root), "Python");
        assert_eq!(
            interpreter_label(&env_with(vec![], None), &root),
            "No interpreter"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn new_environments_start_from_a_base_interpreter() {
        let system = env_with(
            vec![interp(
                "/opt/py/bin/python3.12",
                InterpreterKind::System,
                Some("3.12"),
                None,
            )],
            Some(0),
        );
        assert_eq!(venv_base(&system), PathBuf::from("/opt/py/bin/python3.12"));
        let missing_base = env_with(
            vec![interp(
                "/p/.venv/bin/python",
                InterpreterKind::Venv,
                None,
                Some("/nowhere"),
            )],
            Some(0),
        );
        assert_eq!(venv_base(&missing_base), PathBuf::from("python3"));
        assert_eq!(
            venv_command(Path::new("python3"), Path::new("/p/.venv")),
            ["python3", "-m", "venv", "/p/.venv"]
        );
        assert!(venv_python(Path::new("/p/.venv")).ends_with("bin/python") || cfg!(windows));
    }

    #[test]
    fn pip_requirements_from_declarations() {
        let u = |name: &str, requirement: &str| Unsatisfied {
            name: name.into(),
            requirement: requirement.into(),
            installed: None,
        };
        assert_eq!(pip_spec(&u("requests", ">=2.31")), "requests>=2.31");
        assert_eq!(pip_spec(&u("flask", "==3.*")), "flask==3.*");
        assert_eq!(pip_spec(&u("httpx", "~=0.27")), "httpx~=0.27");
        assert_eq!(pip_spec(&u("sphinx", "[docs]>=7")), "sphinx[docs]>=7");
        assert_eq!(pip_spec(&u("django", "^5.0")), "django");
        assert_eq!(pip_spec(&u("rich", "*")), "rich");
        assert_eq!(pip_spec(&u("click", "")), "click");
        assert_eq!(
            parse_specs(" requests>=2,<3  rich "),
            ["requests>=2,<3", "rich"]
        );
        assert_eq!(
            pip_install_command(Path::new("/p/python"), &["rich".into()]),
            [
                "/p/python",
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "rich"
            ]
        );
    }

    #[test]
    fn tool_runs_report_exit_and_output() {
        let dir = temp("run");
        let ok = run_tool(
            &["sh".into(), "-c".into(), "echo one; echo two >&2".into()],
            &dir,
        );
        assert!(ok.ok && ok.exit_code == Some(0));
        assert_eq!(ok.output, "one\ntwo");
        assert_eq!(last_line(&ok), "two");
        let bad = run_tool(&["sh".into(), "-c".into(), "exit 3".into()], &dir);
        assert_eq!((bad.ok, bad.exit_code), (false, Some(3)));
        let missing = run_tool(&["idecraft-no-such-program".into()], &dir);
        assert!(!missing.ok && missing.output.starts_with("idecraft-no-such-program: "));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// SPEC-0050 v2 AK-5: without any interpreter the start names the reason; only a bare
    /// `python`/`python3` stands for the selected interpreter.
    #[test]
    fn no_interpreter_names_the_reason() {
        let dir = temp("none");
        let search = PythonSearch {
            root: dir.clone(),
            ..Default::default()
        };
        assert_eq!(selected_path(&search), Err(NO_PYTHON.to_owned()));
        assert!(NO_PYTHON.contains(".venv"));
        assert!(is_bare_python("python") && is_bare_python("python3"));
        for other in [
            "python3.11",
            ".venv/bin/python",
            "/usr/bin/python3",
            "pythonw",
            "",
        ] {
            assert!(!is_bare_python(other), "{other}");
        }
        let words: Vec<String> = vec!["node".into(), "a.js".into()];
        assert_eq!(python_command(&dir, &words, None), Ok(None));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn missing(name: &str, requirement: &str) -> Unsatisfied {
        Unsatisfied {
            name: name.into(),
            requirement: requirement.into(),
            installed: None,
        }
    }

    /// SPEC-0117 v4 AK-8: a declared requirement wins, by its PEP 503 name or through the table.
    #[test]
    fn imports_map_to_declared_requirements_first() {
        let declared = [
            missing("PyYAML", ">=6"),
            missing("typing_extensions", ">=4.8"),
        ];
        let yaml = import_package("yaml", &declared).unwrap();
        assert_eq!(
            (yaml.package.as_str(), yaml.spec.as_str(), yaml.source),
            ("PyYAML", "PyYAML>=6", PackageSource::Requirement)
        );
        let te = import_package("typing_extensions", &declared).unwrap();
        assert_eq!(te.spec, "typing_extensions>=4.8");
        assert_eq!(te.source, PackageSource::Requirement);
        let requests = import_package("requests", &[missing("requests", ">=2.31")]).unwrap();
        assert_eq!(requests.spec, "requests>=2.31");
        // Poetry's caret becomes the bare name, as in the tool window.
        let rich = import_package("rich.console", &[missing("Rich", "^13")]).unwrap();
        assert_eq!(
            (rich.package.as_str(), rich.spec.as_str()),
            ("Rich", "Rich")
        );
    }

    /// SPEC-0117 v4 AK-8: the table, then the import name; dotted and relative imports.
    #[test]
    fn well_known_imports_map_to_their_distribution() {
        for (import, dist) in [
            ("yaml", "PyYAML"),
            ("PIL.Image", "Pillow"),
            ("cv2", "opencv-python"),
            ("sklearn.linear_model", "scikit-learn"),
            ("bs4", "beautifulsoup4"),
            ("google.protobuf.message", "protobuf"),
        ] {
            let found = import_package(import, &[]).unwrap();
            assert_eq!(
                (found.package.as_str(), found.source),
                (dist, PackageSource::Known),
                "{import}"
            );
            assert_eq!(found.import, import);
        }
        let plain = import_package("requests.adapters", &[]).unwrap();
        assert_eq!(
            (plain.package.as_str(), plain.spec.as_str(), plain.source),
            ("requests", "requests", PackageSource::Import)
        );
        // `google.cloud` is not `google.protobuf`; `yamlx` is not `yaml`.
        assert_eq!(
            import_package("google.cloud", &[]).unwrap().package,
            "google"
        );
        assert_eq!(import_package("yamlx", &[]).unwrap().package, "yamlx");
        for wrong in [".models", "..", "", "a b", "1abc", "x;rm"] {
            assert_eq!(import_package(wrong, &[]), None, "{wrong:?}");
        }
    }

    /// SPEC-0117 v4 AK-8: one entry per distribution, "Install all" only with two or more missing.
    #[test]
    fn install_fixes_offer_all_missing_with_two_or_more() {
        let one = [missing("requests", ">=2.31")];
        let fixes = install_fixes(&["requests".into()], &one);
        assert_eq!(
            fixes,
            [InstallFix {
                title: "Install package requests".into(),
                specs: vec!["requests>=2.31".into()],
            }]
        );
        let two = [missing("requests", ">=2.31"), missing("PyYAML", "")];
        let fixes = install_fixes(&["yaml".into(), "yaml.loader".into(), "cv2".into()], &two);
        let titles: Vec<&str> = fixes.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Install package PyYAML",
                "Install package opencv-python",
                "Install all missing requirements (2)"
            ]
        );
        assert_eq!(fixes[2].specs, ["requests>=2.31", "PyYAML"]);
        assert!(install_fixes(&[".local".into()], &two).is_empty());
        assert!(install_fixes(&[], &two).is_empty());
    }
}
