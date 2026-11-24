//! SPEC-0050 AK-1 – AK-4.

use std::path::{Path, PathBuf};
use std::process::Command;

use ferris_idea_core::{
    Dependency, InterpreterKind, PythonSearch, RunConfigs, RunKind, find_interpreters,
    installed_packages, python_environments, run_config,
};

/// Skips a test whose tool is missing; with `FERRIS_IDEA_REQUIRE_TOOLS` set (CI) it fails instead.
fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p =
            std::env::temp_dir().join(format!("ferris-idea-python-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p.canonicalize().unwrap())
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    fn search(&self) -> PythonSearch {
        PythonSearch {
            root: self.0.join("proj"),
            ..PythonSearch::default()
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dep(name: &str, requirement: &str) -> Dependency {
    Dependency {
        name: name.into(),
        requirement: requirement.into(),
        scope: "normal".into(),
        resolved: None,
    }
}

fn dist(s: &Scratch, site: &str, name: &str, version: &str) {
    s.write(
        &format!(
            "{site}/{}-{version}.dist-info/METADATA",
            name.replace('-', "_")
        ),
        &format!("Metadata-Version: 2.1\nName: {name}\nVersion: {version}\n\nlong description\n"),
    );
}

#[test]
fn layouts_and_selection_order() {
    let s = Scratch::new("layouts");
    std::fs::create_dir_all(s.0.join("proj")).unwrap();
    // Project environments: `.venv` (uv style `version_info`) and `env2`.
    s.write(
        "proj/.venv/pyvenv.cfg",
        "home = /usr/local/bin\nimplementation = CPython\nversion_info = 3.12.4\n",
    );
    s.write("proj/.venv/bin/python", "");
    s.write(
        "proj/env2/pyvenv.cfg",
        "home = C:\\Python311\nversion = 3.11.9\n",
    );
    s.write("proj/env2/Scripts/python.exe", "");
    // pyenv with two versions and a conda environment.
    s.write("pyenv/versions/3.10.14/bin/python", "");
    s.write("pyenv/versions/3.12.1/bin/python", "");
    s.write("conda/bin/python", "");
    std::fs::create_dir_all(s.0.join("conda/conda-meta")).unwrap();
    s.write("conda/lib/python3.11/os.py", "");
    // An active environment outside the project.
    s.write("active/pyvenv.cfg", "version = 3.13.0\n");
    s.write("active/bin/python3", "");
    // PATH with a versioned interpreter name.
    s.write("usr/bin/python3", "");
    s.write("usr/lib/python3.9/os.py", "");

    let mut search = s.search();
    search.pyenv_root = Some(s.0.join("pyenv"));
    search.conda_prefix = Some(s.0.join("conda"));
    search.path_var = Some(s.0.join("usr/bin").into_os_string());

    let found = find_interpreters(&search);
    let summary: Vec<(&str, InterpreterKind, Option<&str>)> = found
        .iter()
        .map(|i| (i.found_by.as_str(), i.kind, i.version.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("project", InterpreterKind::Venv, Some("3.12.4")),
            ("project", InterpreterKind::Venv, Some("3.11.9")),
            ("CONDA_PREFIX", InterpreterKind::Conda, Some("3.11")),
            ("pyenv", InterpreterKind::Pyenv, Some("3.12.1")),
            ("pyenv", InterpreterKind::Pyenv, Some("3.10.14")),
            ("PATH", InterpreterKind::System, Some("3.9")),
        ]
    );
    assert!(found[0].in_project && found[0].path.ends_with(".venv/bin/python"));
    assert!(found[1].path.ends_with("env2/Scripts/python.exe"));
    assert_eq!(found[1].base.as_deref(), Some("C:\\Python311"));

    let env = python_environments(&search, None, &[]);
    assert_eq!(
        env.reason.as_deref(),
        Some("virtual environment in the project")
    );
    assert_eq!(env.selected, Some(0));

    // `.python-version` only matters without a project environment.
    std::fs::remove_dir_all(s.0.join("proj/.venv")).unwrap();
    std::fs::remove_dir_all(s.0.join("proj/env2")).unwrap();
    s.write("proj/.python-version", "3.10\n");
    let env = python_environments(&search, None, &[]);
    let chosen = &env.interpreters[env.selected.unwrap()];
    assert_eq!(chosen.version.as_deref(), Some("3.10.14"));
    assert_eq!(env.reason.as_deref(), Some(".python-version asks for 3.10"));

    // An active environment wins over that; a configured interpreter wins over everything.
    search.virtual_env = Some(s.0.join("active"));
    let env = python_environments(&search, None, &[]);
    assert_eq!(
        env.interpreters[env.selected.unwrap()].version.as_deref(),
        Some("3.13.0")
    );
    search.configured = Some(s.0.join("pyenv/versions/3.12.1/bin/python"));
    let env = python_environments(&search, None, &[]);
    assert_eq!(env.reason.as_deref(), Some("chosen in the settings"));

    // Without `.python-version` and environments: conda before PATH.
    search.configured = None;
    search.virtual_env = None;
    std::fs::remove_file(s.0.join("proj/.python-version")).unwrap();
    let env = python_environments(&search, None, &[]);
    assert_eq!(
        env.reason.as_deref(),
        Some("active conda environment (CONDA_PREFIX)")
    );
}

#[test]
fn packages_requirements_and_test_framework() {
    let s = Scratch::new("packages");
    s.write("proj/.venv/pyvenv.cfg", "version = 3.11.2\n");
    s.write("proj/.venv/bin/python", "");
    let site = "proj/.venv/lib/python3.11/site-packages";
    dist(&s, site, "Flask", "3.0.3");
    dist(&s, site, "Werkzeug", "2.3.0");
    dist(&s, site, "zope.interface", "6.1");
    s.write(
        &format!("{site}/legacy_pkg-0.4-py3.11.egg-info/PKG-INFO"),
        "Name: legacy-pkg\nVersion: 0.4\n",
    );
    s.write(&format!("{site}/pytest-8.1.0.dist-info/RECORD"), "");

    let packages = installed_packages(&s.0.join("proj/.venv/bin/python"));
    let names: Vec<(&str, &str)> = packages
        .iter()
        .map(|p| (p.name.as_str(), p.version.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("flask", "3.0.3"),
            ("legacy-pkg", "0.4"),
            ("pytest", "8.1.0"),
            ("werkzeug", "2.3.0"),
            ("zope-interface", "6.1"),
        ]
    );

    let declared = [
        dep("flask", ">=3.0"),
        dep("Werkzeug", ">=3.0"),
        dep("requests", "*"),
        dep("zope.interface", "[docs] ==6.*"),
        dep("tomli", ">=1; python_version < '3.11'"),
        dep("mylib", "path:../mylib"),
    ];
    let env = python_environments(&s.search(), Some(">=3.12"), &declared);
    assert_eq!(env.compatible, Some(false));
    assert!(
        env.notes[0].contains("does not satisfy requires-python >=3.12"),
        "{:?}",
        env.notes
    );
    let bad: Vec<(&str, Option<&str>)> = env
        .unsatisfied
        .iter()
        .map(|u| (u.name.as_str(), u.installed.as_deref()))
        .collect();
    assert_eq!(bad, [("Werkzeug", Some("2.3.0")), ("requests", None)]);
    assert_eq!(env.conditional, ["tomli >=1; python_version < '3.11'"]);
    assert_eq!(env.test_framework, "pytest");
    let config = env.test_config.unwrap();
    assert_eq!(config.kind, RunKind::Test);
    assert_eq!(config.command[0], "${workspace}/.venv/bin/python");
    assert_eq!(
        config.report_file.as_deref(),
        Some(".ferris-idea/pytest.xml")
    );

    let env = python_environments(&s.search(), Some(">=3.9,<4"), &[]);
    assert_eq!(env.compatible, Some(true));
}

/// A real virtual environment made by the installed Python, compared with what that interpreter
/// says about itself; the suggested unittest configuration really runs.
#[test]
fn real_virtual_environment() {
    let s = Scratch::new("real");
    let proj = s.0.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let made = Command::new("python3")
        .args(["-m", "venv", ".venv"])
        .current_dir(&proj)
        .status();
    if !made.is_ok_and(|st| st.success()) {
        skip("python3 -m venv not available");
        return;
    }
    let env = python_environments(
        &s.search(),
        Some(">=3.6"),
        &[dep("pip", ">=1"), dep("flask", ">=3")],
    );
    let chosen = &env.interpreters[env.selected.unwrap()];
    assert_eq!(chosen.kind, InterpreterKind::Venv);
    assert!(chosen.in_project);
    let real = Command::new(&chosen.path)
        .args(["-c", "import sys; print('%d.%d.%d' % sys.version_info[:3])"])
        .output()
        .unwrap();
    let real = String::from_utf8(real.stdout).unwrap();
    assert_eq!(chosen.version.as_deref(), Some(real.trim()));
    assert_eq!(env.compatible, Some(true));
    // `pip` is installed by `venv`; `flask` is not.
    assert!(
        env.packages.iter().any(|p| p.name == "pip"),
        "{:?}",
        env.packages
    );
    let missing: Vec<&str> = env.unsatisfied.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(missing, ["flask"]);

    // The suggested configuration runs the project's unit tests with the environment's interpreter.
    assert_eq!(env.test_framework, "unittest");
    std::fs::write(
        proj.join("test_sample.py"),
        "import sys, unittest\n\nclass T(unittest.TestCase):\n    def test_env(self):\n        self.assertIn('.venv', sys.prefix)\n",
    )
    .unwrap();
    let configs = RunConfigs {
        configs: vec![env.test_config.unwrap()],
        problems: Vec::new(),
    };
    let out = run_config(Path::new(&proj), &configs, "unittest").unwrap();
    assert!(out[0].success(), "{:?}", out[0]);
    assert!(out[0].stderr.contains("test_env"), "{}", out[0].stderr);
}

/// SPEC-0117 AK-2: create an environment with the generated command, choose it, and let pip
/// report a failed install (offline, so nothing is downloaded).
#[test]
fn create_choose_and_install_in_a_real_environment() {
    let s = Scratch::new("manage");
    let proj = s.0.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let before = python_environments(&s.search(), None, &[]);
    let command =
        ferris_idea_core::venv_command(&ferris_idea_core::venv_base(&before), Path::new("env"));
    let made = ferris_idea_core::run_tool(&command, &proj);
    if !made.ok {
        skip(&format!("python3 -m venv failed: {}", made.output));
        return;
    }
    let python = ferris_idea_core::venv_python(&proj.join("env"));
    assert!(python.is_file());
    ferris_idea_core::save_interpreter(&proj, Some(&python)).unwrap();
    let mut search = PythonSearch::for_workspace(&proj);
    search.path_var = None;
    let env = python_environments(&search, None, &[]);
    let chosen = &env.interpreters[env.selected.unwrap()];
    assert_eq!(chosen.found_by, "configured");
    assert_eq!(chosen.kind, InterpreterKind::Venv);
    assert!(
        ferris_idea_core::interpreter_label(&env, &proj).ends_with(" (env)"),
        "{}",
        ferris_idea_core::interpreter_label(&env, &proj)
    );
    assert!(
        env.packages.iter().any(|p| p.name == "pip"),
        "{:?}",
        env.packages
    );
    let mut install =
        ferris_idea_core::pip_install_command(&python, &["ferris-idea-no-such-package".into()]);
    install.insert(4, "--no-index".into());
    let run = ferris_idea_core::run_tool(&install, &proj);
    assert!(!run.ok, "{run:?}");
    assert!(
        run.output
            .to_lowercase()
            .contains("no matching distribution"),
        "{}",
        run.output
    );
}
