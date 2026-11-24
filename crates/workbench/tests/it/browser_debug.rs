//! SPEC-0133 v3 AK-5: Rerun of a page session — Stop to the running session and a new session
//! started at once, as the Debug window does it — stops at the breakpoint again, and after the
//! last Stop no browser uses the project's profile. An old profile inside the project moves out.
//! Headless Chrome; skipped without js-debug or Chrome.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_debugger::{BrowserProfile, OLD_PROFILE, SourceBreakpoint};
use ferris_idea_workbench::debug_model::{Command, Controller, Kind, Launch, Update};

/// Polls `c` until `done` accepts an update; panics with what came after 60 s or on a failure.
fn wait_for(c: &Controller, seen: &mut Vec<Update>, done: impl Fn(&Update) -> bool) {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        for u in c.poll() {
            if let Update::Failed(e) = &u {
                panic!("failed: {e}\n{seen:#?}");
            }
            let hit = done(&u);
            seen.push(u);
            if hit {
                return;
            }
        }
        assert!(Instant::now() < until, "timed out: {seen:#?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn stopped_in_twice(u: &Update) -> bool {
    matches!(u, Update::Stopped { frames, .. } if frames.first().is_some_and(|f| f.name.contains("twice")))
}

fn settles(done: impl Fn() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

fn page(root: &Path) -> PathBuf {
    let script = root.join("app.js");
    std::fs::write(
        &script,
        "function twice(n) {\n  const doubled = n * 2;\n  return doubled;\n}\nsetInterval(() => console.log('page ' + twice(21)), 500);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("index.html"),
        "<!doctype html><html><body><script src=\"app.js\"></script></body></html>\n",
    )
    .unwrap();
    script
}

#[test]
fn rerun_restarts_the_page_session() {
    let Some(adapter) = ferris_idea_debugger::known_adapters()
        .into_iter()
        .find(|a| a.name == "js-debug")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: node or js-debug is not installed");
        return;
    };
    if ferris_idea_debugger::chrome().is_none() {
        eprintln!("skipped: no Chrome or Chromium");
        return;
    }
    ferris_idea_workbench::debug_model::set_browser_headless(true);
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-browser-rerun-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("web");
    std::fs::create_dir_all(root.join(OLD_PROFILE).join("Default")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join(OLD_PROFILE).join("Default/Cookies"), "c").unwrap();
    ferris_idea_core::trust::trust_for_process(&root);
    let script = page(&root);
    let profile = BrowserProfile::in_cache(&dir.join("cache"), &root);
    let launch = Launch {
        kind: Kind::Browser,
        adapter,
        interpreter: None,
        program: PathBuf::from(format!("file://{}", root.join("index.html").display())),
        args: Vec::new(),
        cwd: root.clone(),
        env: BTreeMap::new(),
        breakpoints: BTreeMap::from([(script, vec![SourceBreakpoint::from(3)])]),
        prepare: String::new(),
        special: Default::default(),
        attach: None,
        browser: Some(profile.clone()),
        remote: None,
    };
    let mut seen = Vec::new();
    let first = Controller::start(launch.clone());
    wait_for(&first, &mut seen, stopped_in_twice);
    // The old profile moved out of the project, and the console said so.
    assert!(!root.join(OLD_PROFILE).exists());
    assert!(profile.folder.join("Default/Cookies").is_file());
    assert!(
        seen.iter()
            .any(|u| matches!(u, Update::Output(t) if t.contains("moved out of the project")))
    );
    assert!(!ferris_idea_debugger::lifetime::using_profile(&profile.folder).is_empty());

    // Rerun: Stop to the old session, the new one starts at once.
    first.send(Command::Stop);
    let second = Controller::start(launch);
    let mut seen = Vec::new();
    wait_for(&second, &mut seen, stopped_in_twice);
    wait_for(&first, &mut Vec::new(), |u| matches!(u, Update::Exited(_)));

    second.send(Command::Stop);
    wait_for(&second, &mut seen, |u| matches!(u, Update::Exited(_)));
    assert!(
        settles(|| ferris_idea_debugger::lifetime::using_profile(&profile.folder).is_empty()),
        "a browser still uses the profile after Stop"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
