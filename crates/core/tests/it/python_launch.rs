//! SPEC-0050 v2 AK-5: every Python launch IDECraft builds uses the workspace's selected
//! interpreter; explicit paths stay as written; without any interpreter the start says why.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use idecraft_core::{
    NO_PYTHON, PythonSearch, RunConfig, Workspace, context_config, launch_command,
    load_run_configs, python_environments, read_project, run_config, run_one, run_plan,
    save_interpreter, show_python, start_blocker, suggest_run_configs,
};

struct Root(PathBuf);

impl Root {
    fn new(tag: &str) -> Self {
        let p =
            std::env::temp_dir().join(format!("idecraft-pylaunch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join(".idecraft")).unwrap();
        Self(p.canonicalize().unwrap())
    }
    fn path(&self) -> &Path {
        &self.0
    }
    /// A virtual environment `dir` whose `bin/python` is a stub printing its name and arguments.
    fn venv(&self, dir: &str) -> PathBuf {
        let prefix = self.0.join(dir);
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("pyvenv.cfg"), "version = 3.12.4\n").unwrap();
        let python = prefix.join("bin/python");
        std::fs::write(&python, format!("#!/bin/sh\necho \"{dir}-python $*\"\n")).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        python
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An active environment of the test process would rightly win over the project's (behaviour 3).
fn outside_environment() -> bool {
    let active =
        std::env::var_os("VIRTUAL_ENV").is_some() || std::env::var_os("CONDA_PREFIX").is_some();
    if active {
        eprintln!("skipped: an environment is active in the test process");
    }
    active
}

fn config(name: &str, command: &[&str]) -> RunConfig {
    RunConfig {
        name: name.into(),
        command: command.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env: Default::default(),
        kind: Default::default(),
        report: None,
        report_file: None,
        before: Vec::new(),
        timeout: None,
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: false,
    }
}

#[test]
fn every_python_launch_uses_the_project_environment() {
    if outside_environment() {
        return;
    }
    let root = Root::new("venv");
    let python = root.venv(".venv");
    let venv = python.to_string_lossy().into_owned();
    std::fs::write(root.path().join("requirements.txt"), "pytest>=8\n").unwrap();
    std::fs::write(root.path().join("main.py"), "print(1)\n").unwrap();
    let files = Workspace::open(root.path()).unwrap().files().unwrap();
    let model = read_project(root.path(), &files);

    // Run current file and its ▶ marker.
    let run = context_config(&model, "main.py", "print(1)\n").unwrap();
    assert_eq!(
        run.command[0], "python3",
        "the generated configuration stays portable"
    );
    assert_eq!(
        launch_command(root.path(), &run).unwrap(),
        [&venv, "main.py"]
    );
    // A test file runs pytest of the environment.
    let test = context_config(&model, "test_calc.py", "def test_x(): pass\n").unwrap();
    let words = launch_command(root.path(), &test).unwrap();
    assert_eq!(words[..4], [&venv, "-m", "pytest", "test_calc.py"]);

    // The project model's pytest suggestion, as the MCP tool shows it.
    let mut suggested = suggest_run_configs(&model)
        .into_iter()
        .find(|c| c.name == "pytest")
        .expect("pytest is declared");
    assert_eq!(launch_command(root.path(), &suggested).unwrap()[0], venv);
    show_python(root.path(), &mut suggested);
    assert_eq!(suggested.command[0], "${workspace}/.venv/bin/python");
    // The same word as the test configuration of behaviour 6.
    let env = python_environments(&PythonSearch::for_workspace(root.path()), None, &[]);
    assert_eq!(env.test_config.unwrap().command[0], suggested.command[0]);

    // A stored configuration with a bare `python3`: planned, shown and run with the environment.
    std::fs::write(
        root.path().join(".idecraft/run.toml"),
        "[[run]]\nname = \"app\"\ncommand = [\"python3\", \"main.py\", \"x\"]\n",
    )
    .unwrap();
    let configs = load_run_configs(root.path());
    let plan = run_plan(root.path(), &configs, "app").unwrap();
    assert_eq!(plan[0].command, [&venv, "main.py", "x"]);
    let outcomes = run_config(root.path(), &configs, "app").unwrap();
    assert_eq!(outcomes[0].stdout.trim(), ".venv-python main.py x");
    assert_eq!(start_blocker(root.path(), &configs.configs[0], &[]), None);
}

#[test]
fn the_stored_choice_wins_and_paths_stay_as_written() {
    if outside_environment() {
        return;
    }
    let root = Root::new("choice");
    let venv = root.venv(".venv").to_string_lossy().into_owned();
    let chosen = root.venv("env2");
    save_interpreter(root.path(), Some(&chosen)).unwrap();
    let bare = config("bare", &["python", "app.py"]);
    assert_eq!(
        launch_command(root.path(), &bare).unwrap(),
        [chosen.to_string_lossy().as_ref(), "app.py"]
    );
    let mut shown = bare.clone();
    show_python(root.path(), &mut shown);
    assert_eq!(shown.command[0], "${workspace}/env2/bin/python");
    assert_eq!(
        run_one(root.path(), &bare).stdout.trim(),
        "env2-python app.py"
    );

    // Paths and other names are not replaced.
    for explicit in ["/usr/bin/python3", "python3.11", "./tools/python3"] {
        let c = config("explicit", &[explicit, "app.py"]);
        assert_eq!(launch_command(root.path(), &c).unwrap()[0], explicit);
    }
    let workspace = config("workspace", &["${workspace}/.venv/bin/python", "app.py"]);
    assert_eq!(launch_command(root.path(), &workspace).unwrap()[0], venv);
    // A plugin's configuration runs as given (SPEC-0276).
    let mut plugin = config("plugin", &["python3", "app.py"]);
    plugin.verbatim = true;
    assert_eq!(launch_command(root.path(), &plugin).unwrap()[0], "python3");
    let mut shown = plugin.clone();
    show_python(root.path(), &mut shown);
    assert_eq!(shown.command[0], "python3");
}

#[test]
fn without_an_interpreter_the_start_names_the_reason() {
    if outside_environment() {
        return;
    }
    // pyenv versions of the user would still be found (behaviour 1).
    let pyenv = std::env::var_os("PYENV_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".pyenv")));
    if pyenv.is_some_and(|p| p.join("versions").is_dir()) {
        eprintln!("skipped: pyenv versions are installed");
        return;
    }
    let root = Root::new("none");
    let empty = root.path().join("empty-bin");
    std::fs::create_dir_all(&empty).unwrap();
    let mut c = config("app", &["python3", "app.py"]);
    c.env
        .insert("PATH".into(), empty.to_string_lossy().into_owned());
    let why = launch_command(root.path(), &c).unwrap_err();
    assert_eq!(why, NO_PYTHON);
    let outcome = run_one(root.path(), &c);
    assert_eq!(outcome.start_error, Some(format!("app: {NO_PYTHON}")));
    assert!(!outcome.success());
    assert_eq!(
        start_blocker(root.path(), &c, &[]),
        Some(format!("app: {NO_PYTHON}"))
    );
    // Other programs are not affected by the missing interpreter.
    let mut other = config("other", &["sh", "-c", "true"]);
    other.env = c.env.clone();
    assert_eq!(launch_command(root.path(), &other).unwrap()[0], "sh");
}
