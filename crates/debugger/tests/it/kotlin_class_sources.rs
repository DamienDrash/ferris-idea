//! SPEC-0132 v6 AK-9: what a session sends to kotlin-debug-adapter for the breakpoints of a Kotlin
//! file whose classes are named unlike the file, what happens when the adapter refuses or does not
//! answer such a request, and how a stop or an end the adapter did not report is found — against a
//! fake adapter that, like the real one, reports every breakpoint as verified.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ferris_idea_debugger::{
    Breakpoint, DapClient, DapError, DebugSession, Event, SourceBreakpoint, read_message,
    write_message,
};
use serde_json::{Value, json};

/// The requests the fake adapter received: `setBreakpoints` as path and lines, others as the
/// command and no lines.
type Log = Arc<Mutex<Vec<(String, Vec<u64>)>>>;

/// What the program of the fake adapter did before the adapter listened to it.
#[derive(Clone, Default)]
enum Lost {
    /// Nothing: it runs, and no thread answers `stackTrace`.
    #[default]
    Nothing,
    /// It stopped at this file and line, and no `stopped` event was sent.
    Stop(PathBuf, u64),
    /// It ended, and no `exited` event was sent.
    End,
}

/// How the fake adapter behaves.
#[derive(Clone, Default)]
struct Fake {
    /// Sources whose path contains this text are refused.
    refuse: Option<&'static str>,
    /// Sources whose path contains this text are never answered.
    silent: Option<&'static str>,
    lost: Lost,
}

/// Starts the fake adapter.
fn fake_adapter(fake: Fake) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let log: Log = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut seq = 0;
        let mut send = |mut m: Value| {
            seq += 1;
            m["seq"] = json!(seq);
            write_message(&mut writer, &m).unwrap();
        };
        let mut launch = None;
        while let Ok(Some(request)) = read_message(&mut reader) {
            let command = request["command"].as_str().unwrap_or_default().to_owned();
            let answer = |success: bool, body: Value| {
                json!({"type": "response", "request_seq": request["seq"], "command": command,
                       "success": success, "body": body})
            };
            if command != "setBreakpoints" {
                seen.lock().unwrap().push((command.clone(), Vec::new()));
            }
            match command.as_str() {
                "launch" => {
                    launch = Some(request["seq"].clone());
                    send(json!({"type": "event", "event": "initialized"}));
                }
                "setBreakpoints" => {
                    let path = request["arguments"]["source"]["path"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    let list = request["arguments"]["breakpoints"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let lines = list.iter().filter_map(|b| b["line"].as_u64()).collect();
                    let refused = fake.refuse.is_some_and(|text| path.contains(text));
                    let silent = fake.silent.is_some_and(|text| path.contains(text));
                    seen.lock().unwrap().push((path, lines));
                    if silent {
                        continue;
                    }
                    let placed: Vec<Value> = list
                        .iter()
                        .map(|b| json!({"verified": true, "line": b["line"]}))
                        .collect();
                    send(answer(!refused, json!({"breakpoints": placed})));
                }
                // Like the real adapter: no answer to `configurationDone`, then `launch`'s.
                "configurationDone" => {
                    send(json!({"type": "response", "request_seq": launch.take(),
                                "command": "launch", "success": true}));
                }
                "threads" => {
                    let mut threads = vec![
                        json!({"id": 1, "name": "main"}),
                        json!({"id": 2, "name": "Reference Handler"}),
                    ];
                    if matches!(fake.lost, Lost::End) {
                        threads[0] = json!({"id": 7, "name": "DestroyJavaVM"});
                    }
                    send(answer(true, json!({"threads": threads})));
                }
                "stackTrace" => {
                    let thread = request["arguments"]["threadId"].as_i64();
                    match (&fake.lost, thread) {
                        (Lost::Stop(path, line), Some(1)) => send(answer(
                            true,
                            json!({"stackFrames": [
                                {"id": 10, "name": "main", "line": line, "source": {"path": path}},
                            ]}),
                        )),
                        // Suspended with the whole program: frames without sources, or none.
                        (Lost::End, _) => send(answer(true, json!({"stackFrames": []}))),
                        // A running thread has no frames to give.
                        _ => send(answer(false, Value::Null)),
                    }
                }
                "continue" => {
                    send(answer(true, json!({})));
                    send(json!({"type": "event", "event": "exited", "body": {"exitCode": 0}}));
                }
                _ => send(answer(true, json!({}))),
            }
        }
    });
    (address, log)
}

const ZOO: &str = "package zoo\n\nabstract class Animal(val name: String) {\n    open fun describe(): String = name\n}\n\nclass Cat(name: String) : Animal(name) {\n    fun sound() = \"meow\"\n}\n\nfun legs() = 4\n";

/// A folder with `text` as the file `relative`.
fn file(test: &str, relative: &str, text: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ferris-idea-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

fn lines(list: &[u64]) -> Vec<SourceBreakpoint> {
    list.iter().copied().map(SourceBreakpoint::from).collect()
}

/// The `setBreakpoints` requests since `from`, with paths below `file` shortened to what follows
/// it.
fn sent(log: &Log, from: usize, file: &Path) -> Vec<(String, Vec<u64>)> {
    let whole = file.to_string_lossy().into_owned();
    log.lock().unwrap()[from..]
        .iter()
        .filter(|(path, _)| path.starts_with('/'))
        .map(|(path, lines)| (path.replacen(&whole, "", 1), lines.clone()))
        .collect()
}

/// How often `command` was received.
fn count(log: &Log, command: &str) -> usize {
    let log = log.lock().unwrap();
    log.iter().filter(|(c, _)| c == command).count()
}

fn start(
    adapter_id: &str,
    address: &str,
    breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
) -> (DebugSession, BTreeMap<PathBuf, Vec<Breakpoint>>) {
    let client = DapClient::connect(address, None).unwrap();
    DebugSession::launch_on(
        client,
        adapter_id,
        json!({}),
        breakpoints,
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn lines_are_sent_again_for_the_classes_of_the_file_and_taken_back() {
    let (dir, zoo) = file("ktsources", "src/main/kotlin/zoo/Animals.kt", ZOO);
    let (address, log) = fake_adapter(Fake::default());
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4, 8]))]);
    let (mut session, placed) = start("kotlin-debug-adapter", &address, &breakpoints);
    assert!(placed[&zoo].iter().all(|b| b.verified));
    // Before `configurationDone`: the file, then its classes and what is inside them.
    assert_eq!(
        sent(&log, 0, &zoo),
        [
            (String::new(), vec![4, 8]),
            ("/kotlin/zoo/Animal.kt".to_owned(), vec![4]),
            ("/kotlin/zoo/Animal$*.kt".to_owned(), vec![4]),
            ("/kotlin/zoo/Cat.kt".to_owned(), vec![8]),
            ("/kotlin/zoo/Cat$*.kt".to_owned(), vec![8]),
        ]
    );
    assert_eq!(
        log.lock().unwrap().last().map(|l| l.0.clone()),
        Some("configurationDone".to_owned())
    );
    // The breakpoint in `Cat` goes, one in the top-level function comes: the sources of `Cat`
    // are emptied, the file's own class needs only its inner classes.
    let from = log.lock().unwrap().len();
    session.set_breakpoints(&zoo, &lines(&[4, 11])).unwrap();
    assert_eq!(
        sent(&log, from, &zoo),
        [
            (String::new(), vec![4, 11]),
            ("/kotlin/zoo/Cat.kt".to_owned(), vec![]),
            ("/kotlin/zoo/Cat$*.kt".to_owned(), vec![]),
            ("/kotlin/zoo/Animal.kt".to_owned(), vec![4]),
            ("/kotlin/zoo/Animal$*.kt".to_owned(), vec![4]),
            ("/kotlin/zoo/AnimalsKt$*.kt".to_owned(), vec![11]),
        ]
    );
    // No breakpoint left: everything is taken back, once.
    let from = log.lock().unwrap().len();
    session.set_breakpoints(&zoo, &[]).unwrap();
    session.set_breakpoints(&zoo, &[]).unwrap();
    assert_eq!(
        sent(&log, from, &zoo),
        [
            (String::new(), vec![]),
            ("/kotlin/zoo/Animal.kt".to_owned(), vec![]),
            ("/kotlin/zoo/Animal$*.kt".to_owned(), vec![]),
            ("/kotlin/zoo/AnimalsKt$*.kt".to_owned(), vec![]),
            (String::new(), vec![]),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_file_of_the_main_class_is_sent_first() {
    let (dir, zoo) = file("ktsources-order", "src/main/kotlin/zoo/Animals.kt", ZOO);
    let main = dir.join("src/main/kotlin/zoo/Main.kt");
    std::fs::write(
        &main,
        "package zoo\n\nfun main() {\n    println(legs())\n}\n",
    )
    .unwrap();
    let (address, log) = fake_adapter(Fake::default());
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4])), (main.clone(), lines(&[4]))]);
    let client = DapClient::connect(&address, None).unwrap();
    let _session = DebugSession::launch_on(
        client,
        "kotlin",
        json!({"mainClass": "zoo.MainKt"}),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let order: Vec<String> = sent(&log, 0, &dir).into_iter().map(|s| s.0).collect();
    assert_eq!(
        order,
        [
            "/src/main/kotlin/zoo/Main.kt",
            "/src/main/kotlin/zoo/Main.kt/kotlin/zoo/MainKt$*.kt",
            "/src/main/kotlin/zoo/Animals.kt",
            "/src/main/kotlin/zoo/Animals.kt/kotlin/zoo/Animal.kt",
            "/src/main/kotlin/zoo/Animals.kt/kotlin/zoo/Animal$*.kt",
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn other_adapters_and_other_files_get_only_the_file() {
    let (dir, zoo) = file("ktsources-other", "src/main/kotlin/zoo/Animals.kt", ZOO);
    let java = dir.join("src/main/java/zoo/Animals.java");
    let (address, log) = fake_adapter(Fake::default());
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4]))]);
    let (mut session, _) = start("java", &address, &breakpoints);
    assert_eq!(sent(&log, 0, &zoo), [(String::new(), vec![4])]);
    // Another adapter is not asked for a lost stop either.
    assert!(matches!(
        session.wait(Duration::from_millis(700)),
        Err(DapError::Timeout)
    ));
    assert_eq!(count(&log, "threads"), 0);
    // A Java file of a Kotlin session, and a Kotlin file that cannot be read.
    let (address, log) = fake_adapter(Fake::default());
    let missing = dir.join("src/main/kotlin/zoo/Gone.kt");
    let breakpoints = BTreeMap::from([(java.clone(), lines(&[4])), (missing, lines(&[2]))]);
    let _session = start("kotlin", &address, &breakpoints);
    assert_eq!(sent(&log, 0, &dir).len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_source_for_inner_classes_is_said_and_keeps_the_breakpoint() {
    let (dir, zoo) = file("ktsources-refused", "src/main/kotlin/zoo/Animals.kt", ZOO);
    let (address, log) = fake_adapter(Fake {
        refuse: Some("*"),
        ..Fake::default()
    });
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4]))]);
    let (mut session, placed) = start("kotlin", &address, &breakpoints);
    assert!(placed[&zoo][0].verified);
    assert_eq!(sent(&log, 0, &zoo).len(), 3);
    // Said once, also when the breakpoints are set again.
    session.set_breakpoints(&zoo, &lines(&[4])).unwrap();
    let console = session.console().to_owned();
    assert_eq!(
        console.matches("for the classes inside `Animal`").count(),
        1,
        "{console}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unanswered_class_source_does_not_hold_up_the_launch() {
    let (dir, zoo) = file("ktsources-silent", "src/main/kotlin/zoo/Animals.kt", ZOO);
    let (address, log) = fake_adapter(Fake {
        silent: Some("/kotlin/zoo/Animal.kt"),
        ..Fake::default()
    });
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4, 8]))]);
    let started = Instant::now();
    let (mut session, placed) = start("kotlin", &address, &breakpoints);
    // The request has five seconds; `configurationDone` follows whatever comes of it.
    assert!(started.elapsed() < Duration::from_secs(12));
    assert_eq!(count(&log, "configurationDone"), 1);
    // Only the lines of that class are not verified.
    let verified: Vec<bool> = placed[&zoo].iter().map(|b| b.verified).collect();
    assert_eq!(verified, [false, true]);
    assert!(
        placed[&zoo][0]
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("did not answer")),
        "{:?}",
        placed[&zoo][0]
    );
    assert_eq!(placed[&zoo][1].reason, None);
    let pending = session.pending_breakpoints();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].line, Some(4));
    assert!(pending[0].hint.contains("did not answer"), "{pending:?}");
    assert!(
        session
            .console()
            .contains("Animals.kt:4 cannot stop the program")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_line_whose_class_cannot_be_named_is_not_verified() {
    let text = "package org.kotlin.tools\n\nclass Tool {\n    fun run() = 1\n}\n";
    let (dir, tool) = file(
        "ktsources-unbound",
        "src/main/kotlin/org/kotlin/tools/Tool.kt",
        text,
    );
    let (address, log) = fake_adapter(Fake::default());
    let breakpoints = BTreeMap::from([(tool.clone(), lines(&[4]))]);
    let (mut session, placed) = start("kotlin", &address, &breakpoints);
    // The adapter says verified; it would look into `tools.Tool`, which does not exist.
    assert!(!placed[&tool][0].verified);
    // SPEC-0132 v7 AK-11: the reason goes with the placed breakpoint (window and agents).
    assert!(
        placed[&tool][0]
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("`kotlin` or `java`")),
        "{:?}",
        placed[&tool][0]
    );
    assert_eq!(placed[&tool][0].requested_line, Some(4));
    assert_eq!(sent(&log, 0, &tool), [(String::new(), vec![4])]);
    let pending = session.pending_breakpoints();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].line, Some(4));
    assert!(
        pending[0].hint.contains("`kotlin` or `java`"),
        "{pending:?}"
    );
    // The console says it once, also when the breakpoints are set again.
    session.set_breakpoints(&tool, &lines(&[4])).unwrap();
    let console = session.console().to_owned();
    assert_eq!(
        console.matches("Tool.kt:4 cannot stop the program").count(),
        1,
        "{console}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stop_the_adapter_did_not_report_is_found() {
    let (dir, zoo) = file("ktsources-lost", "src/main/kotlin/zoo/Animals.kt", ZOO);
    let (address, log) = fake_adapter(Fake {
        lost: Lost::Stop(zoo.clone(), 4),
        ..Fake::default()
    });
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4]))]);
    let (mut session, _) = start("kotlin-debug-adapter", &address, &breakpoints);
    // No `stopped` event comes; the thread at the breakpoint line is the stop.
    let Event::Stopped(stop) = session.wait(Duration::from_secs(10)).unwrap() else {
        panic!("not stopped");
    };
    assert_eq!((stop.reason.as_str(), stop.thread_id), ("breakpoint", 1));
    assert_eq!(count(&log, "threads"), 1);
    // From here the session is as after any stop.
    session.resume_continue(stop.thread_id).unwrap();
    assert!(matches!(
        session.wait(Duration::from_secs(10)).unwrap(),
        Event::Exited(Some(0))
    ));
    assert_eq!(count(&log, "threads"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_thread_at_another_line_is_no_lost_stop_and_is_asked_for_once() {
    let (dir, zoo) = file("ktsources-running", "src/main/kotlin/zoo/Animals.kt", ZOO);
    // Suspended at line 8, where no breakpoint is: not a stop Ferris IDEA may invent.
    let (address, log) = fake_adapter(Fake {
        lost: Lost::Stop(zoo.clone(), 8),
        ..Fake::default()
    });
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4]))]);
    let (mut session, _) = start("kotlin", &address, &breakpoints);
    for _ in 0..3 {
        assert!(matches!(
            session.wait(Duration::from_millis(500)),
            Err(DapError::Timeout)
        ));
    }
    assert_eq!(count(&log, "threads"), 1);
    assert_eq!(count(&log, "continue"), 0);
    // A running program: no thread has frames.
    let (address, log) = fake_adapter(Fake::default());
    let (mut session, _) = start("kotlin", &address, &breakpoints);
    assert!(matches!(
        session.wait(Duration::from_millis(700)),
        Err(DapError::Timeout)
    ));
    assert_eq!(count(&log, "threads"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_end_the_adapter_did_not_report_is_found() {
    let (dir, zoo) = file("ktsources-ended", "src/main/kotlin/zoo/Animals.kt", ZOO);
    let (address, _log) = fake_adapter(Fake {
        lost: Lost::End,
        ..Fake::default()
    });
    let breakpoints = BTreeMap::from([(zoo.clone(), lines(&[4]))]);
    let (mut session, _) = start("kotlin", &address, &breakpoints);
    // Every thread suspended, the JVM's shutdown thread among them, none at a breakpoint.
    assert!(matches!(
        session.wait(Duration::from_secs(10)).unwrap(),
        Event::Exited(None)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}
