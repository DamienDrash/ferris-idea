//! SPEC-0170/AK-2: a real pyright resolves imports against the selected interpreter and follows a
//! change of interpreter without a restart. Skipped when pyright or `python3 -m venv` is missing.
//! A test binary of its own, because the pool is switched on for the whole process.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use ferris_idea_languages::{lsp, pool, python_server};

fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn installed(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// A virtual environment without pip at `dir`; returns its interpreter.
fn venv(dir: &Path) -> Option<std::path::PathBuf> {
    let ok = Command::new("python3")
        .args(["-m", "venv", "--without-pip"])
        .arg(dir)
        .status()
        .is_ok_and(|s| s.success());
    ok.then(|| dir.join("bin").join("python"))
}

/// Waits until the pool's diagnostics for `file` satisfy `want`.
async fn wait_for(file: &Path, since: &mut u64, want: impl Fn(&[lsp::Diagnostic]) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let (now, changed) = pool::diagnostics_since(*since);
        *since = now;
        if changed.iter().any(|(p, d)| p == file && want(d)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn unresolved(d: &[lsp::Diagnostic]) -> bool {
    d.iter().any(|d| d.message.contains("demo_only_in_a"))
}

#[tokio::test(flavor = "multi_thread")]
async fn pyright_follows_the_selected_interpreter() {
    if !installed("pyright-langserver") {
        skip("pyright-langserver not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-pyserver-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    // Neither environment is called `.venv`, so only the settings can point pyright at one.
    let (Some(a), Some(b)) = (venv(&root.join("env_a")), venv(&root.join("env_b"))) else {
        skip("python3 -m venv failed");
        return;
    };
    let site = std::fs::read_dir(root.join("env_a/lib"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path()
        .join("site-packages/demo_only_in_a");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(
        site.join("__init__.py"),
        "def hello() -> str:\n    return 'hi'\n",
    )
    .unwrap();
    let file = root.join("main.py");
    let text = "import demo_only_in_a\n\nprint(demo_only_in_a.hello())\n";
    std::fs::write(&file, text).unwrap();

    ferris_idea_core::save_interpreter(&root, Some(&a)).unwrap();
    assert_eq!(
        python_server::settings(&root)["python"]["pythonPath"],
        a.display().to_string()
    );
    pool::enable();
    let mut since = pool::diagnostics_since(0).0;
    pool::sync_text(&root, &file, text).await.unwrap();
    assert!(
        wait_for(&file, &mut since, |d| !unresolved(d)).await,
        "env_a: the import resolves"
    );

    // Switching to the empty environment marks the import; switching back clears it.
    ferris_idea_core::save_interpreter(&root, Some(&b)).unwrap();
    assert_eq!(python_server::interpreter_changed(&root), 1);
    assert!(
        wait_for(&file, &mut since, unresolved).await,
        "env_b: the import is unresolved"
    );
    ferris_idea_core::save_interpreter(&root, Some(&a)).unwrap();
    assert_eq!(python_server::interpreter_changed(&root), 1);
    assert!(
        wait_for(&file, &mut since, |d| !unresolved(d)).await,
        "env_a again: resolved"
    );
    // Hand test 44 (D25): a package installed into the environment afterwards resolves once the
    // server was restarted, as the window does after pip succeeds (SPEC-0117 v3 behaviour 7).
    let fresh =
        "import demo_only_in_a\nimport fresh_pkg\n\nprint(demo_only_in_a.hello(), fresh_pkg.X)\n";
    std::fs::write(&file, fresh).unwrap();
    let missing = |d: &[lsp::Diagnostic]| d.iter().any(|d| d.message.contains("fresh_pkg"));
    pool::sync_text(&root, &file, fresh).await.unwrap();
    assert!(
        wait_for(&file, &mut since, missing).await,
        "fresh_pkg is not installed yet"
    );
    let installed_pkg = site.parent().unwrap().join("fresh_pkg");
    std::fs::create_dir_all(&installed_pkg).unwrap();
    std::fs::write(installed_pkg.join("__init__.py"), "X = 1\n").unwrap();
    assert_eq!(
        pool::restart_language(&root, ferris_idea_languages::LanguageId::Python),
        1
    );
    pool::sync_text(&root, &file, fresh).await.unwrap();
    assert!(
        wait_for(&file, &mut since, |d| !missing(d) && !unresolved(d)).await,
        "fresh_pkg resolves after the restart"
    );
    let report = python_server::report(&root, false);
    assert_eq!(report.server.as_deref(), Some("pyright-langserver"));
    assert_eq!(report.interpreter, Some(a.display().to_string()));
    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
