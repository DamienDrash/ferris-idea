//! SPEC-0322 behaviour 4: what a real local Java debug session listens on. Skipped without jdtls,
//! java-debug or `lsof`. Every TCP listener of this test's child processes is sampled from before
//! the launch until the end of a hot swap; the program start is slowed down through `javaExec` so
//! that the debugger's short-lived wait for the JVM cannot slip between two samples.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use idecraft_debugger::{DapClient, DebugSession, SourceBreakpoint, StopState};
use idecraft_languages::java_debug::JavaDebugServer;
use serde_json::json;

const PROGRAM: &str = "public class App {\n    static int v() {\n        return 1;\n    }\n    public static void main(String[] a) {\n        int n = v();\n        System.out.println(n);\n        System.out.println(v());\n    }\n}\n";

/// This process's descendants.
fn descendants() -> Vec<u32> {
    let out = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .expect("ps");
    let pairs: Vec<(u32, u32)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
        })
        .collect();
    let mut found = vec![std::process::id()];
    let mut i = 0;
    while i < found.len() {
        let parent = found[i];
        found.extend(pairs.iter().filter(|p| p.1 == parent).map(|p| p.0));
        i += 1;
    }
    found.remove(0);
    found
}

/// The listening addresses of this process's descendants, as `lsof` prints them (`*:5005`,
/// `127.0.0.1:5005`, `[::1]:5005`).
fn listeners() -> BTreeSet<String> {
    let pids = descendants()
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    if pids.is_empty() {
        return BTreeSet::new();
    }
    let out = Command::new("lsof")
        .args(["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-Fn", "-p", &pids])
        .output()
        .expect("lsof");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('n').map(str::to_owned))
        .collect()
}

fn on_loopback(address: &str) -> bool {
    address.starts_with("127.") || address.starts_with("[::1]")
}

/// A JDK whose `java` waits before starting `real`, for the debugger's wait for the JVM.
fn slow_java(dir: &Path, real: &Path) -> PathBuf {
    let bin = dir.join("slow-jdk/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join("java");
    std::fs::write(
        &path,
        format!("#!/bin/sh\nsleep 3\nexec {} \"$@\"\n", real.display()),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn a_local_java_session_listens_on_no_other_interface() {
    if !idecraft_languages::java_debug::java_debug_usable()
        || idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err()
        || Command::new("lsof").arg("-v").output().is_err()
    {
        eprintln!("skipped: jdtls, java-debug or lsof is missing");
        return;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-javaport-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.canonicalize().unwrap();
    idecraft_core::trust::trust_for_process(&root);
    let file = root.join("App.java");
    std::fs::write(&file, PROGRAM).unwrap();
    let server = JavaDebugServer::start(&root).expect("jdtls with java-debug");
    server.open(&file).unwrap();
    let (modules, classes) = server.classpath("App", Duration::from_secs(120)).unwrap();
    server.build().unwrap();
    let project = server.project_name("App");
    // The `java` of the JDK jdtls compiles for, not `JAVA_HOME`'s: on a machine where jdtls runs on
    // a newer JDK, an older `java` cannot load the classes and the program ends at once.
    let real = server
        .java_executable("App", project.as_deref())
        .or_else(|| std::env::var_os("JAVA_HOME").map(|h| PathBuf::from(h).join("bin/java")))
        .unwrap_or_else(|| PathBuf::from("/usr/bin/java"));
    let java = slow_java(&root, &real);

    let seen: Arc<Mutex<BTreeSet<String>>> = Arc::default();
    let done = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (seen, done) = (seen.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                seen.lock().unwrap().extend(listeners());
                std::thread::sleep(Duration::from_millis(50));
            }
        })
    };

    assert!(
        server.dap_address().starts_with("unix:"),
        "{}",
        server.dap_address()
    );
    let address = server.dap_address();
    let server = Arc::new(Mutex::new(server));
    let sources = server.clone();
    let settings = server.clone();
    let client = DapClient::connect(&address, Some(Box::new(server)))
        .unwrap()
        .with_host(Box::new(idecraft_debugger::FnHost {
            settings: Box::new(move |s| settings.lock().unwrap().update_settings(s)),
            sources: Box::new(move |f| sources.lock().unwrap().sources_changed(f)),
        }));
    let breakpoints = BTreeMap::from([(
        file.clone(),
        vec![SourceBreakpoint {
            line: 7,
            ..SourceBreakpoint::default()
        }],
    )]);
    let (mut session, _) = DebugSession::launch_on(
        client,
        "java",
        json!({
            "type": "java", "request": "launch", "mainClass": "App", "cwd": root,
            "console": "internalConsole", "stopOnEntry": false, "javaExec": java,
            "modulePaths": modules, "classPaths": classes, "projectName": project,
        }),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let state = session.wait_state(Duration::from_secs(120), 5).unwrap();
    let StopState::Stopped { frames, .. } = state else {
        panic!("not stopped: {state:?}");
    };
    assert_eq!(frames[0].line, 7, "{:?}", frames[0]);

    // Hot swap, with everything it may open.
    std::fs::write(&file, PROGRAM.replace("return 1", "return 2")).unwrap();
    session
        .build_changed(std::slice::from_ref(&file), Duration::from_secs(60))
        .unwrap();
    let reloaded = session.reload_classes();

    done.store(true, Ordering::SeqCst);
    sampler.join().unwrap();
    let _ = session.disconnect();
    let seen = seen.lock().unwrap().clone();
    eprintln!("listeners seen: {seen:?}; hot swap: {reloaded:?}");
    let open: Vec<&String> = seen.iter().filter(|a| !on_loopback(a)).collect();
    assert!(open.is_empty(), "open on other interfaces: {open:?}");
    assert!(reloaded.is_ok(), "hot swap: {reloaded:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
