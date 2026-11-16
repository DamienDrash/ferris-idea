//! SPEC-0045 v3 AK-8: Go and pytest projects are detected and tested with the real tools; the reports
//! of the run become a result tree. Skipped when the tool is not installed.

use std::path::{Path, PathBuf};

use idecraft_core::{
    RunConfigs, RunKind, TestStatus, TestSuite, Workspace, read_project, run_config,
    suggest_run_configs,
};

fn works(cmd: &[&str]) -> bool {
    std::process::Command::new(cmd[0])
        .args(&cmd[1..])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn project(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("idecraft-runner-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The detected configurations; with `path_first`, that folder goes before `PATH` (a Python with
/// pytest when the system's `python3` has none).
fn detected(root: &Path, path_first: Option<&Path>) -> RunConfigs {
    let files = Workspace::open(root).unwrap().files().unwrap();
    let mut configs = suggest_run_configs(&read_project(root, &files));
    if let Some(dir) = path_first {
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        for c in &mut configs {
            c.env.insert("PATH".into(), path.clone());
        }
    }
    RunConfigs {
        configs,
        problems: Vec::new(),
    }
}

/// A folder whose `python3` has pytest: none needed, or the folder of a `pytest` on `PATH`.
fn pytest_python() -> Option<Option<PathBuf>> {
    if works(&["python3", "-m", "pytest", "--version"]) {
        return Some(None);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("pytest"))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .filter(|d| d.join("python3").is_file())
        .map(Some)
}

fn cases(s: &TestSuite, out: &mut Vec<(String, TestStatus)>) {
    out.extend(s.tests.iter().map(|t| (t.name.clone(), t.status)));
    for n in &s.suites {
        cases(n, out);
    }
}

fn tested(root: &Path, name: &str, path_first: Option<&Path>) -> Vec<(String, TestStatus)> {
    let configs = detected(root, path_first);
    let config = configs.configs.iter().find(|c| c.name == name).unwrap();
    assert_eq!(config.kind, RunKind::Test);
    let runs = run_config(root, &configs, name).unwrap();
    let outcome = runs.last().unwrap();
    let report = outcome.report.as_ref().unwrap_or_else(|| {
        panic!(
            "no report: {:?}\n{}\n{}",
            outcome.report_error, outcome.stdout, outcome.stderr
        )
    });
    let mut out = Vec::new();
    for s in &report.suites {
        cases(s, &mut out);
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn go_test_json_tree() {
    if !works(&["go", "version"]) {
        eprintln!("skipped: go is not installed");
        return;
    }
    let root = project("go");
    std::fs::write(root.join("go.mod"), "module demo\n\ngo 1.21\n").unwrap();
    std::fs::write(
        root.join("m_test.go"),
        "package demo\n\nimport \"testing\"\n\nfunc TestOk(t *testing.T) {}\n\nfunc TestBad(t *testing.T) { t.Error(\"no\") }\n\nfunc TestT(t *testing.T) {\n\tt.Run(\"a\", func(t *testing.T) {})\n}\n",
    )
    .unwrap();
    assert_eq!(
        tested(&root, "go test", None),
        [
            ("TestBad".to_owned(), TestStatus::Failed),
            ("TestOk".to_owned(), TestStatus::Passed),
            ("a".to_owned(), TestStatus::Passed),
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pytest_junit_tree_and_no_stale_report() {
    let Some(python) = pytest_python() else {
        eprintln!("skipped: no python3 with pytest");
        return;
    };
    let root = project("pytest");
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\nversion = \"0.1\"\n\n[project.optional-dependencies]\ntest = [\"pytest\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("test_m.py"),
        "def test_ok():\n    pass\n\ndef test_bad():\n    assert 1 == 2\n",
    )
    .unwrap();
    assert_eq!(
        tested(&root, "pytest", python.as_deref()),
        [
            ("test_bad".to_owned(), TestStatus::Failed),
            ("test_ok".to_owned(), TestStatus::Passed),
        ]
    );
    // A run that writes no report does not show the previous run's tree.
    let report = root.join(".idecraft/pytest.xml");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&report)
        .unwrap()
        .set_modified(old)
        .unwrap();
    std::fs::write(root.join("test_m.py"), "import (\n").unwrap();
    std::fs::write(root.join("conftest.py"), "raise SystemExit(4)\n").unwrap();
    let configs = detected(&root, python.as_deref());
    let runs = run_config(&root, &configs, "pytest").unwrap();
    let outcome = runs.last().unwrap();
    if outcome.report.is_none() {
        assert!(
            outcome
                .report_error
                .as_deref()
                .is_some_and(|e| e.contains("no test report written by this run")),
            "{:?}",
            outcome.report_error
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
