//! SPEC-0171/AK-2 and AK-3: the real package index (skipped offline) and a real uv installing
//! with the generated command (skipped without uv or `python3 -m venv`).

use std::path::Path;
use std::process::Command;

use idecraft_core::{
    Installer, PYPI, PythonSearch, http_fetch, install_plan, installed_packages, package_info,
    python_environments, requirement, save_interpreter,
};

fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn temp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("idecraft-pyindex-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

#[test]
fn details_from_the_real_index() {
    let info = match package_info(&http_fetch, PYPI, "Requests") {
        Ok(info) => info.expect("requests is on the index"),
        Err(e) => {
            // Offline is not a missing tool: skip without failing CI.
            eprintln!("skipped: the package index is not reachable ({e})");
            return;
        }
    };
    assert_eq!(info.name, "requests");
    assert!(!info.summary.is_empty());
    assert!(info.releases.iter().any(|r| r.version == "2.31.0"));
    assert!(info.releases.len() > 100);
    assert_eq!(
        package_info(&http_fetch, PYPI, "idecraft-surely-not-a-package-4711").unwrap(),
        None
    );
}

/// A wheel with one module, made by hand (a zip with the dist-info files).
fn make_wheel(dir: &Path) -> bool {
    let script = r#"
import sys, zipfile
out = sys.argv[1]
files = {
    "demo_wheel/__init__.py": "VALUE = 1\n",
    "demo_wheel-1.0.dist-info/METADATA": "Metadata-Version: 2.1\nName: demo-wheel\nVersion: 1.0\n",
    "demo_wheel-1.0.dist-info/WHEEL": "Wheel-Version: 1.0\nGenerator: hand\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
}
record = "".join(f"{name},,\n" for name in files) + "demo_wheel-1.0.dist-info/RECORD,,\n"
with zipfile.ZipFile(out, "w") as z:
    for name, text in files.items():
        z.writestr(name, text)
    z.writestr("demo_wheel-1.0.dist-info/RECORD", record)
"#;
    Command::new("python3")
        .args(["-I", "-c", script])
        .arg(dir.join("demo_wheel-1.0-py3-none-any.whl"))
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn uv_installs_with_the_generated_command() {
    let Some(uv) = idecraft_core::on_path("uv") else {
        skip("uv not installed");
        return;
    };
    let root = temp("uv");
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\nversion = \"0.1\"\n\n[tool.uv]\n",
    )
    .unwrap();
    let ok = Command::new("python3")
        .args(["-m", "venv", "--without-pip", ".venv"])
        .current_dir(&root)
        .status()
        .is_ok_and(|s| s.success());
    let wheels = root.join("wheels");
    std::fs::create_dir_all(&wheels).unwrap();
    if !ok || !make_wheel(&wheels) {
        skip("python3 -m venv or zipfile failed");
        return;
    }
    save_interpreter(&root, Some(&root.join(".venv/bin/python"))).unwrap();
    let env = python_environments(&PythonSearch::for_workspace(&root), None, &[]);
    let chosen = env.selected.map(|i| &env.interpreters[i]);
    let plan = install_plan(
        &root,
        chosen,
        &requirement("demo-wheel", Some("1.0")),
        &idecraft_core::on_path,
    );
    assert_eq!(plan.installer, Installer::Uv);
    assert_eq!(plan.command[0], uv.display().to_string());
    let mut command = plan.command.clone();
    command.extend(
        ["--offline", "--no-index", "--find-links"]
            .map(str::to_owned)
            .into_iter()
            .chain([wheels.display().to_string()]),
    );
    let run = idecraft_core::run_tool(&command, &root);
    assert!(run.ok, "{}", run.output);
    let packages = installed_packages(&root.join(".venv/bin/python"));
    assert!(
        packages
            .iter()
            .any(|p| p.name == "demo-wheel" && p.version == "1.0"),
        "{packages:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
