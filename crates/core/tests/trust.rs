//! Safe mode refuses everything that runs project code (`SPEC-0278` AK-4). This test binary switches
//! safe mode on for its own process.

use std::path::PathBuf;

use idecraft_core::{RunEvent, parse_run_configs, run_config, start_run, trust};

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-safe-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn safe_mode_refuses_runs_and_dependency_resolution() {
    trust::enforce();
    let untrusted = project("untrusted");
    let trusted = project("trusted");
    trust::trust_for_process(&trusted);
    // A user who trusts everything through the environment or the temp folder sees no safe mode.
    if trust::allows_project_code(&untrusted) {
        eprintln!("skipped: the temporary folder is trusted on this machine");
        return;
    }
    let configs = parse_run_configs(
        "[[run]]\nname = \"touch\"\ncommand = [\"sh\", \"-c\", \"echo ran > ran.txt\"]\n",
    );
    // Run configurations: refused with the reason, nothing ran.
    let runs = run_config(&untrusted, &configs, "touch").unwrap();
    let why = runs[0].start_error.as_deref().unwrap();
    assert!(why.starts_with("Safe mode:"), "{why}");
    assert!(why.contains("Trust Project"), "{why}");
    assert!(!untrusted.join("ran.txt").exists());
    // Streamed runs (Run tool window, tests, tasks).
    let refused = start_run(&untrusted, &configs, "touch").err().unwrap();
    assert!(refused.starts_with("Safe mode:"));
    // Maven/Gradle dependency resolution (build-tool sync).
    std::fs::write(untrusted.join("pom.xml"), "<project/>").unwrap();
    let model = idecraft_core::read_project(&untrusted, &[untrusted.join("pom.xml")]);
    let m = model.modules.first().expect("a Maven module");
    let err = idecraft_core::jvmdeps::resolve(&untrusted, m).unwrap_err();
    assert!(err.starts_with("Safe mode:"), "{err}");
    // A trusted project runs.
    let handle = start_run(&trusted, &configs, "touch").unwrap();
    let finished = handle
        .events
        .iter()
        .find_map(|e| match e {
            RunEvent::Finished(o) => Some(o),
            _ => None,
        })
        .unwrap();
    assert!(finished.success(), "{finished:?}");
    assert!(trusted.join("ran.txt").exists());
    let _ = std::fs::remove_dir_all(&untrusted);
    let _ = std::fs::remove_dir_all(&trusted);
}

/// SPEC-0047 v3 AK-8: in an untrusted project no before-launch task starts and no URL is opened;
/// the URL comes from a project file.
#[test]
fn safe_mode_runs_no_task_and_opens_no_browser() {
    use std::sync::{Arc, Mutex};
    static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    idecraft_core::set_browser_opener(Some(Arc::new(|url: &str| {
        OPENED.lock().unwrap().push(url.to_owned());
        Ok(())
    })));
    trust::enforce();
    let untrusted = project("tasks-untrusted");
    let trusted = project("tasks-trusted");
    trust::trust_for_process(&trusted);
    if trust::allows_project_code(&untrusted) {
        eprintln!("skipped: the temporary folder is trusted on this machine");
        return;
    }
    std::fs::write(untrusted.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let configs = parse_run_configs(
        "[[run]]\nname = \"tool\"\ncommand = [\"sh\", \"-c\", \"echo ran > ran.txt\"]\nbefore_launch = [{ tool = [\"sh\", \"-c\", \"echo tool > tool.txt\"] }, { browser = \"http://localhost:1/safe-mode\" }]\n\n[[run]]\nname = \"build\"\ncommand = [\"sh\", \"-c\", \"echo ran > ran.txt\"]\nbefore_launch = [{ build = \"project\" }]\n\n[[run]]\nname = \"page\"\ncommand = [\"sh\", \"-c\", \"echo ran > ran.txt\"]\nbefore_launch = [{ browser = \"http://localhost:1/safe-mode\" }]\n",
    );
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    for name in ["tool", "build", "page"] {
        // Blocking: the first step is refused with the reason and nothing follows.
        let runs = run_config(&untrusted, &configs, name).unwrap();
        assert_eq!(runs.len(), 1, "{name}: {runs:?}");
        let why = runs[0].start_error.as_deref().unwrap();
        assert!(why.starts_with("Safe mode:"), "{name}: {why}");
        assert!(runs[0].browser.is_empty());
        // Streamed: refused before anything is planned.
        let refused = start_run(&untrusted, &configs, name).err().unwrap();
        assert!(refused.starts_with("Safe mode:"), "{name}: {refused}");
    }
    assert!(!untrusted.join("tool.txt").exists());
    assert!(!untrusted.join("ran.txt").exists());
    assert!(!untrusted.join("target").exists());
    // The opener itself refuses too, whoever calls it.
    let refused =
        idecraft_core::open_browser(&untrusted, "http://localhost:1/safe-mode").unwrap_err();
    assert!(refused.starts_with("Safe mode:"), "{refused}");
    assert!(OPENED.lock().unwrap().is_empty());
    // Trusted: the tool runs and the page is opened.
    let runs = run_config(&trusted, &configs, "tool").unwrap();
    assert!(runs.iter().all(|r| r.success()), "{runs:?}");
    assert!(trusted.join("tool.txt").exists());
    assert_eq!(*OPENED.lock().unwrap(), ["http://localhost:1/safe-mode"]);
    let _ = std::fs::remove_dir_all(&untrusted);
    let _ = std::fs::remove_dir_all(&trusted);
}
