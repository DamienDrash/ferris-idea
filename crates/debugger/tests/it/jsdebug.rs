//! SPEC-0039 v5 AK-10: a Node.js program stops at a breakpoint through js-debug's child session,
//! with its locals, evaluates an expression and runs to its end. Skipped without node or js-debug.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ferris_idea_debugger::{
    DebugSession, JsDebugServer, SourceBreakpoint, StopState, known_adapters, node_launch,
};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(30);

#[test]
fn node_program_stops_at_breakpoint() {
    let Some(command) = known_adapters()
        .into_iter()
        .find(|a| a.name == "js-debug")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: node or js-debug is not installed");
        return;
    };
    let dir = std::env::temp_dir().join(format!("ferris-idea-jsdebug-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let script: PathBuf = dir.join("app.js");
    std::fs::write(
        &script,
        "function twice(n) {\n  const doubled = n * 2;\n  return doubled;\n}\nconsole.log('result ' + twice(21));\n",
    )
    .unwrap();
    let server = JsDebugServer::start(&command).unwrap();
    let address = server.address();
    let breakpoints = BTreeMap::from([(script.clone(), vec![SourceBreakpoint::from(3)])]);
    let launch = json!({
        "type": "pwa-node", "request": "launch", "name": "app.js",
        "program": script, "cwd": dir, "console": "internalConsole",
    });
    let (mut session, placed) = DebugSession::launch_js(
        &address,
        "launch",
        launch,
        &breakpoints,
        &Default::default(),
        Box::new(server),
    )
    .unwrap();
    assert_eq!(placed[&script].len(), 1);
    let thread = match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert!(frames[0].name.ends_with("twice"), "{}", frames[0].name);
            assert_eq!(frames[0].line, 3);
            let doubled = locals.iter().find(|v| v.name == "doubled").unwrap();
            assert_eq!(doubled.value, "42");
            assert_eq!(session.evaluate("n + 1", frames[0].id).unwrap(), "22");
            thread_id
        }
        other => panic!("not stopped: {other:?}"),
    };
    session.resume_continue(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Exited { .. } => {}
        other => panic!("not ended: {other:?}"),
    }
    assert!(
        session.output().contains("result 42"),
        "{}",
        session.output()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0133 AK-2: a page's script stops at a breakpoint in Chrome (headless, own throwaway
/// profile) through js-debug, with its locals, and runs on. v3 AK-5: ending the session closes the
/// browser and ends js-debug, so a second session on the same profile starts right after (Rerun).
/// Skipped without js-debug or Chrome.
#[test]
fn browser_page_stops_at_breakpoint() {
    let Some(command) = known_adapters()
        .into_iter()
        .find(|a| a.name == "js-debug")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: node or js-debug is not installed");
        return;
    };
    let Some(chrome) = ferris_idea_debugger::chrome() else {
        eprintln!("skipped: no Chrome or Chromium");
        return;
    };
    let dir = temp_project("browserdebug");
    write_page(&dir);
    let profile = dir.join(".profile");
    for round in 0..2 {
        let (session, server) = page_session(&command, &chrome, &dir, &profile, round == 0);
        // Stop; the second time the session is only dropped (a broken session), which ends js-debug
        // and the browser with its helpers by signal.
        if round == 0 {
            session.disconnect().unwrap();
        } else {
            drop(session);
        }
        assert!(
            settles(|| ferris_idea_debugger::lifetime::using_profile(&profile).is_empty()),
            "a browser still uses the profile: {:?}",
            ferris_idea_debugger::lifetime::using_profile(&profile)
        );
        assert!(settles(|| ferris_idea_debugger::lifetime::command_of(
            server
        )
        .is_none()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Whether `done` holds within 5 s.
fn settles(done: impl Fn() -> bool) -> bool {
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while !done() {
        if std::time::Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

/// `index.html` with `app.js`, whose `twice` stops on line 3 and which logs `page 42`.
fn write_page(dir: &std::path::Path) {
    std::fs::write(
        dir.join("app.js"),
        "function twice(n) {\n  const doubled = n * 2;\n  return doubled;\n}\nsetTimeout(() => console.log('page ' + twice(21)), 300);\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><html><body><script src=\"app.js\"></script></body></html>\n",
    )
    .unwrap();
}

/// Debugs `index.html` of `dir` in headless Chrome with `profile` until it stops in `twice` with
/// `doubled = 42`; with `resume` it runs on and the page logs. Returns the session and js-debug's
/// process id.
fn page_session(
    command: &[String],
    chrome: &std::path::Path,
    dir: &std::path::Path,
    profile: &std::path::Path,
    resume: bool,
) -> (DebugSession, u32) {
    let script = dir.join("app.js");
    let server = JsDebugServer::start(command).unwrap();
    let address = server.address();
    let pid = server.pid();
    let breakpoints = BTreeMap::from([(script, vec![SourceBreakpoint::from(3)])]);
    let launch = ferris_idea_debugger::browser_launch(
        &format!("file://{}", dir.join("index.html").display()),
        dir,
        chrome,
        profile,
        true,
    );
    let (mut session, _) = DebugSession::launch_js(
        &address,
        "launch",
        launch,
        &breakpoints,
        &Default::default(),
        Box::new(server),
    )
    .unwrap();
    let thread = match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert!(frames[0].name.contains("twice"), "{}", frames[0].name);
            let doubled = locals.iter().find(|v| v.name == "doubled").unwrap();
            assert_eq!(doubled.value, "42");
            thread_id
        }
        other => panic!("not stopped: {other:?}"),
    };
    if resume {
        session.resume_continue(thread).unwrap();
        let _ = session.wait_state(Duration::from_secs(3), 10);
        assert!(session.output().contains("page 42"), "{}", session.output());
    }
    (session, pid)
}

/// SPEC-0263 v2 AK-3: when the process debugging a page is killed (`SIGKILL`, no clean-up of its
/// own), its browser and js-debug end within a few seconds. The test runs itself as that process.
#[cfg(unix)]
#[test]
fn a_killed_owner_leaves_no_browser_or_adapter() {
    const ROLE: &str = "FERRIS_IDEA_TEST_PAGE_OWNER";
    let Some(command) = known_adapters()
        .into_iter()
        .find(|a| a.name == "js-debug")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: node or js-debug is not installed");
        return;
    };
    let Some(chrome) = ferris_idea_debugger::chrome() else {
        eprintln!("skipped: no Chrome or Chromium");
        return;
    };
    if let Some(dir) = std::env::var_os(ROLE) {
        // The owner: debugs the page, reports js-debug's id, and waits to be killed.
        let dir = PathBuf::from(dir);
        let (session, server) = page_session(&command, &chrome, &dir, &dir.join(".profile"), false);
        println!("READY {server}");
        std::mem::forget(session);
        std::thread::sleep(Duration::from_secs(120));
        return;
    }
    let dir = temp_project("browserowner");
    write_page(&dir);
    let mut owner = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "jsdebug::a_killed_owner_leaves_no_browser_or_adapter",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, &dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut server = None;
    let stdout = std::io::BufReader::new(owner.stdout.take().unwrap());
    for line in std::io::BufRead::lines(stdout) {
        let Ok(line) = line else { break };
        // The harness's `test … ` has no line break of its own.
        if let Some((_, pid)) = line.split_once("READY ") {
            server = pid.trim().parse::<u32>().ok();
            break;
        }
    }
    let profile = dir.join(".profile");
    let in_use = !ferris_idea_debugger::lifetime::using_profile(&profile).is_empty();
    owner.kill().unwrap();
    let _ = owner.wait();
    let server = server.expect("the owner debugged the page");
    assert!(in_use, "the owner's browser was not found");
    assert!(
        settles(|| ferris_idea_debugger::lifetime::using_profile(&profile).is_empty()),
        "the browser outlived its owner"
    );
    assert!(
        settles(|| ferris_idea_debugger::lifetime::command_of(server).is_none()),
        "js-debug outlived its owner"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Starts js-debug with `launch` and checks the stop in `twice` with `doubled = 42` and the
/// output after resuming. Returns false when js-debug is missing.
fn stops_in_twice(launch: serde_json::Value, script: &std::path::Path, line: i64) -> bool {
    let Some(command) = known_adapters()
        .into_iter()
        .find(|a| a.name == "js-debug")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: node or js-debug is not installed");
        return false;
    };
    let server = JsDebugServer::start(&command).unwrap();
    let address = server.address();
    let breakpoints = BTreeMap::from([(
        script.to_path_buf(),
        vec![SourceBreakpoint::from(line as u64)],
    )]);
    let (mut session, _) = DebugSession::launch_js(
        &address,
        "launch",
        launch,
        &breakpoints,
        &Default::default(),
        Box::new(server),
    )
    .unwrap();
    let thread = match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert!(frames[0].name.contains("twice"), "{}", frames[0].name);
            assert_eq!(frames[0].line, line);
            let doubled = locals.iter().find(|v| v.name == "doubled").unwrap();
            assert_eq!(doubled.value, "42");
            thread_id
        }
        other => panic!("not stopped: {other:?}\n{}", session.output()),
    };
    session.resume_continue(thread).unwrap();
    loop {
        match session.wait_state(WAIT, 10).unwrap() {
            StopState::Exited { .. } => break,
            StopState::Stopped { thread_id, .. } => session.resume_continue(thread_id).unwrap(),
            StopState::Running => {}
        }
    }
    let output = session.output().to_owned();
    assert!(output.contains("result 42"), "{output}");
    assert!(!output.contains("source map"), "{output}");
    let _ = session.disconnect();
    true
}

fn temp_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

/// SPEC-0168 AK-2: an npm script stops at a breakpoint in the script it runs.
#[test]
fn npm_script_stops_at_breakpoint() {
    let Some(npm) = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("npm"))
            .find(|f| f.is_file())
    }) else {
        eprintln!("skipped: npm is not installed");
        return;
    };
    let dir = temp_project("npmdebug");
    let script = dir.join("app.js");
    std::fs::write(
        &script,
        "function twice(n) {\n  const doubled = n * 2;\n  return doubled;\n}\nconsole.log('result ' + twice(21));\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("package.json"),
        "{ \"name\": \"t\", \"scripts\": { \"start\": \"node app.js\" } }\n",
    )
    .unwrap();
    let args = ["run".to_owned(), "start".to_owned()];
    let launch = node_launch(&npm, Some(&npm), &args, &dir, &Default::default());
    stops_in_twice(launch, &script, 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0168 AK-3: a TypeScript file runs on Node's type stripping and stops at a breakpoint.
#[test]
fn typescript_file_stops_at_breakpoint() {
    let version = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    if !ferris_idea_debugger::strips_types(&version) {
        eprintln!("skipped: node {version} cannot run TypeScript");
        return;
    }
    let dir = temp_project("tsdebug");
    let script = dir.join("app.ts");
    std::fs::write(
        &script,
        "function twice(n: number): number {\n  const doubled: number = n * 2;\n  return doubled;\n}\nconsole.log('result ' + twice(21));\n",
    )
    .unwrap();
    let launch = node_launch(&script, None, &[], &dir, &Default::default());
    stops_in_twice(launch, &script, 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Hand test 20 (beta of workflow (a), B4): stopped on a line whose call's argument calls a
/// function of the program, Step Into stops in that function. Skipped without node or js-debug.
#[test]
fn step_into_an_argument_call_stops_in_it() {
    let Some(command) = known_adapters()
        .into_iter()
        .find(|a| a.name == "js-debug")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: node or js-debug is not installed");
        return;
    };
    let dir = temp_project("stepin-arg");
    let script: PathBuf = dir.join("app.js");
    std::fs::write(
        &script,
        "function helper(n) {\n  const next = n + 1;\n  return next;\n}\n\nconst result = 5;\nconsole.log(\"result\", result, helper(1));\nconsole.log(\"done\");\n",
    )
    .unwrap();
    let server = JsDebugServer::start(&command).unwrap();
    let address = server.address();
    let breakpoints = BTreeMap::from([(script.clone(), vec![SourceBreakpoint::from(7)])]);
    let launch = node_launch(&script, None, &[], &dir, &Default::default());
    let (mut session, _) = DebugSession::launch_js(
        &address,
        "launch",
        launch,
        &breakpoints,
        &Default::default(),
        Box::new(server),
    )
    .unwrap();
    let thread = match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            frames, thread_id, ..
        } => {
            assert_eq!(frames[0].line, 7, "{frames:?}");
            thread_id
        }
        other => panic!("not stopped: {other:?}"),
    };
    session.step_in(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped { frames, .. } => {
            assert!(frames[0].name.ends_with("helper"), "{frames:?}");
        }
        other => panic!("Step Into did not stop in helper: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0045 v6 AK-15: a Jest test run through `npx jest … --runInBand` under js-debug stops at a
/// breakpoint in the test file, also where Jest's transformed code has the line elsewhere (js-debug
/// waits for the source map). Skipped without js-debug, npx or an installed Jest.
#[test]
fn jest_test_stops_at_breakpoint() {
    let Some(command) = known_adapters()
        .into_iter()
        .find(|a| a.name == "js-debug")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: node or js-debug is not installed");
        return;
    };
    let find = |name: &str| {
        std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(name))
                .find(|f| f.is_file())
        })
    };
    let (Some(npx), Some(jest)) = (find("npx"), find("jest")) else {
        eprintln!("skipped: npx or jest is not installed");
        return;
    };
    // …/node_modules/jest/bin/jest.js → the package folder.
    let package = std::fs::canonicalize(jest)
        .ok()
        .and_then(|j| j.parent()?.parent().map(PathBuf::from))
        .filter(|p| p.join("package.json").is_file());
    let Some(package) = package else {
        eprintln!("skipped: the jest package is not found");
        return;
    };
    let dir = temp_project("jestdebug");
    std::fs::write(
        dir.join("package.json"),
        "{ \"name\": \"t\", \"private\": true, \"devDependencies\": { \"jest\": \"*\" } }\n",
    )
    .unwrap();
    let test = dir.join("calc.test.js");
    std::fs::write(
        &test,
        // The comment and the blank line move the test's lines in Jest's transformed code: the
        // breakpoint binds only through the source map.
        "// Tests of the sums.\n\ntest(\"good\", () => {\n  expect(1 + 2).toBe(3);\n});\n\ntest(\"broken\", () => {\n  const sum = 1 + 2;\n  expect(sum).toBe(4);\n});\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("node_modules/.bin")).unwrap();
    std::os::unix::fs::symlink(&package, dir.join("node_modules/jest")).unwrap();
    std::os::unix::fs::symlink("../jest/bin/jest.js", dir.join("node_modules/.bin/jest")).unwrap();
    let server = JsDebugServer::start(&command).unwrap();
    let address = server.address();
    let breakpoints = BTreeMap::from([(test.clone(), vec![SourceBreakpoint::from(9)])]);
    let args: Vec<String> = [
        "jest",
        "--json",
        "--outputFile=.ferris-idea/jest.json",
        "calc.test.js",
        "-t",
        "broken$",
        "--runInBand",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // The working folder as a run configuration without `cwd` gives it: with a trailing slash.
    let cwd = PathBuf::from(format!("{}/", dir.display()));
    let launch = node_launch(&npx, Some(&npx), &args, &cwd, &Default::default());
    let (mut session, _) = DebugSession::launch_js(
        &address,
        "launch",
        launch,
        &breakpoints,
        &Default::default(),
        Box::new(server),
    )
    .unwrap();
    match session.wait_state(Duration::from_secs(120), 10).unwrap() {
        StopState::Stopped { frames, locals, .. } => {
            assert_eq!(frames[0].line, 9, "{frames:?}");
            let sum = locals.iter().find(|v| v.name == "sum").unwrap();
            assert_eq!(sum.value, "3");
        }
        other => panic!("not stopped: {other:?}\n{}", session.output()),
    }
    let _ = session.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}
