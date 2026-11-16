//! SPEC-0158 AK-2: IDECraft's backend on a host reached by SSH — against a real `sshd` started as
//! the current user (skipped without one). The local binary is uploaded to a test folder in the
//! home folder, started for a project folder and asked through MCP.

use idecraft_remote::{Backend, Installed, Remote, ensure_backend, testing::TestHost};

#[test]
fn backend_is_uploaded_started_and_answers_tools() {
    let base = std::env::temp_dir().join(format!("idecraft-backend-{}", std::process::id()));
    let Some(host) = TestHost::start(&base.join("ssh")) else {
        eprintln!("skipped: no sshd");
        return;
    };
    let remote = Remote::new(host.target.clone());
    let project = base.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "pub fn needle() {}\n").unwrap();
    std::fs::create_dir_all(project.join(".idecraft")).unwrap();
    std::fs::write(
        project.join(".idecraft/run.toml"),
        "[[run]]\nname = \"build\"\ncommand = [\"echo\", \"built\"]\n\n[[run]]\nname = \"app\"\ncommand = [\"sh\", \"-c\", \"echo $MODE\"]\nenv = { MODE = \"dev\" }\nbefore = [\"build\"]\n",
    )
    .unwrap();
    let dir = format!(".cache/idecraft-test-{}", std::process::id());
    let version = env!("CARGO_PKG_VERSION");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_idecraft"));

    let first = ensure_backend(&remote, &dir, version, binary).unwrap();
    assert!(matches!(first, Installed::Uploaded(_)), "{first:?}");
    let again = ensure_backend(&remote, &dir, version, binary).unwrap();
    assert!(matches!(again, Installed::Present(_)), "{again:?}");

    let mut backend =
        Backend::start(&remote, again.path(), &project.display().to_string()).unwrap();
    assert!(backend.server.starts_with("idecraft"), "{}", backend.server);
    assert!(backend.tools.iter().any(|(n, _)| n == "search_in_files"));
    let found = backend
        .call("search_in_files", serde_json::json!({"pattern": "needle"}))
        .unwrap();
    assert!(found.to_string().contains("lib.rs"), "{found}");
    assert!(backend.call("no_such_tool", serde_json::json!({})).is_err());

    // SPEC-0158 v2: symbols, run configurations and the host-side check through the backend.
    let symbols = backend
        .call(
            "search_everywhere",
            serde_json::json!({"query": "needle", "categories": ["symbols"]}),
        )
        .unwrap();
    assert_eq!(symbols["symbols"][0]["name"], "needle", "{symbols}");
    assert_eq!(symbols["symbols"][0]["path"], "src/lib.rs");
    let configs = backend
        .call("run_configurations", serde_json::json!({}))
        .unwrap();
    let plan = backend
        .call(
            "run_configuration",
            serde_json::json!({"name": "app", "dry_run": true}),
        )
        .unwrap();
    let steps = idecraft_remote::run_steps(&configs, &plan).unwrap();
    let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["build", "app"]);
    assert_eq!(steps[1].env, [("MODE".to_owned(), "dev".to_owned())]);
    // The chain runs on the host as the window's SSH terminal would run it.
    let script = idecraft_remote::run_script(&project.display().to_string(), &steps);
    let out = remote
        .sh(&script, &[], None, std::time::Duration::from_secs(30))
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("built") && text.contains("dev"), "{text}");
    // The language server runs on the host: problems or the missing server, never a hang.
    let checked = backend.call(
        "file_diagnostics",
        serde_json::json!({"path": "src/lib.rs", "timeout_seconds": 60}),
    );
    match checked {
        Ok(list) => assert!(list.is_array(), "{list}"),
        Err(e) => assert!(!e.to_string().is_empty()),
    }
    assert!(backend.alive());
    drop(backend);

    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap();
    let _ = std::fs::remove_dir_all(home.join(&dir));
    let _ = std::fs::remove_dir_all(&base);
}
