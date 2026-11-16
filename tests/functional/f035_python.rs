//! F-035 · Python support: interpreter, virtual environments, tests (SPEC-0050, IMP-0025).

use crate::harness::*;
use serde_json::json;

#[test]
fn interpreters_requirements_and_test_configuration_are_read_statically() {
    let p = Project::reference("flask-basic");
    p.write(
        "pyproject.toml",
        "[project]\nname = \"inv\"\nrequires-python = \">=3.9\"\ndependencies = [\"flask\"]\n\n[project.optional-dependencies]\ntest = [\"pytest\"]\n",
    );
    let mut ide = Ide::stdio(&p);
    let env = ide.ok("python_environments", json!({}));
    let interpreters = env["interpreters"].as_array().unwrap();
    if interpreters.is_empty() {
        skip("no Python interpreter on this machine");
        return;
    }
    assert_eq!(env["requires_python"], ">=3.9", "{env}");
    // pytest is declared, so it is the test framework, with a JUnit report for the result tree.
    assert_eq!(env["test_framework"], "pytest", "{env}");
    assert!(text(&env["test_config"]).contains("pytest"), "{env}");
    // No interpreter is started: an unknown version is reported as such.
    for i in env["interpreters"].as_array().unwrap() {
        if i["version"].is_null() {
            assert!(text(&env["notes"]).contains("version"), "{env}");
        }
    }
}

#[test]
fn choosing_an_interpreter_is_saved_in_the_project() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    let env = ide.ok("python_environments", json!({}));
    let Some(first) = env["interpreters"].as_array().unwrap().first().cloned() else {
        skip("no Python interpreter on this machine");
        return;
    };
    ide.ok(
        "python_select_interpreter",
        json!({"interpreter": first["path"], "dry_run": false}),
    );
    assert!(
        p.read(".idecraft/python.toml")
            .contains(&text(&first["path"]))
    );
    let venv = ide.ok("python_create_venv", json!({}));
    assert!(
        text(&venv).contains("venv"),
        "dry run shows the command: {venv}"
    );
    assert!(!p.path(".venv").exists(), "dry run creates nothing");
}
