//! SPEC-0127 v3 AK-5: the further breakpoint options IDECraft applies at each hit — dependent
//! breakpoints, removal after the first hit, messages, "no suspend" and the instance, class and
//! caller filters — against a fake adapter.
//!
//! The fake adapter runs `for i in 1..=6 { line 3; line 4 }` in `Main.work(int)`, called from
//! `Main.main` for odd `i` and from `Main.other` for even `i`; `this` is `Main$Item@{30 + i}`. It
//! stops at every line with a breakpoint whose own condition `i == N` (if any) holds.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use idecraft_debugger::{
    BreakpointRules, DapClient, DebugSession, Event, SourceBreakpoint, Suspend, read_message,
    write_message,
};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);
const LAST: i64 = 6;

type Log = Arc<Mutex<Vec<Value>>>;

/// Starts the fake adapter; returns its address and the requests it receives.
fn fake_adapter(source: PathBuf) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let log: Log = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut seq = 0;
        let mut send = |w: &mut std::net::TcpStream, mut m: Value| {
            seq += 1;
            m["seq"] = json!(seq);
            write_message(w, &m).unwrap();
        };
        // Where the program is: iteration and line (3 or 4); 0 before the start.
        let (mut i, mut line) = (1i64, 2i64);
        // Lines with a breakpoint and their own condition `i == N`.
        let mut lines: BTreeMap<i64, Option<i64>> = BTreeMap::new();
        let mut launch_seq = None;
        while let Ok(Some(request)) = read_message(&mut reader) {
            seen.lock().unwrap().push(request.clone());
            let command = request["command"].as_str().unwrap_or_default().to_owned();
            let answer = |body: Value| {
                json!({"type": "response", "request_seq": request["seq"], "command": command,
                       "success": true, "body": body})
            };
            // Runs to the next line with a breakpoint whose condition holds, or to the end.
            let run = |i: &mut i64, line: &mut i64, lines: &BTreeMap<i64, Option<i64>>| loop {
                if *line == 3 {
                    *line = 4;
                } else {
                    if *line == 4 {
                        *i += 1;
                    }
                    *line = 3;
                }
                if *i > LAST {
                    return false;
                }
                if let Some(c) = lines.get(line)
                    && c.is_none_or(|n| n == *i)
                {
                    return true;
                }
            };
            let stop_or_end = |w: &mut std::net::TcpStream,
                               send: &mut dyn FnMut(&mut std::net::TcpStream, Value),
                               go: bool| {
                if go {
                    send(
                        w,
                        json!({"type": "event", "event": "stopped",
                               "body": {"reason": "breakpoint", "threadId": 1}}),
                    );
                } else {
                    send(
                        w,
                        json!({"type": "event", "event": "exited", "body": {"exitCode": 0}}),
                    );
                }
            };
            match command.as_str() {
                "initialize" => send(
                    &mut writer,
                    answer(json!({"supportsConditionalBreakpoints": true})),
                ),
                "launch" => {
                    launch_seq = Some(request.clone());
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "initialized"}),
                    );
                }
                "setBreakpoints" => {
                    let list = request["arguments"]["breakpoints"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    lines = list
                        .iter()
                        .filter_map(|b| {
                            let n = b["condition"]
                                .as_str()
                                .and_then(|c| c.strip_prefix("i == "))
                                .and_then(|n| n.parse().ok());
                            Some((b["line"].as_i64()?, n))
                        })
                        .collect();
                    let placed: Vec<Value> = list
                        .iter()
                        .enumerate()
                        .map(|(n, b)| json!({"id": n + 1, "verified": true, "line": b["line"]}))
                        .collect();
                    send(&mut writer, answer(json!({"breakpoints": placed})));
                }
                "configurationDone" => {
                    send(&mut writer, answer(Value::Null));
                    if let Some(l) = launch_seq.take() {
                        send(
                            &mut writer,
                            json!({"type": "response", "request_seq": l["seq"],
                                   "command": "launch", "success": true}),
                        );
                    }
                    let go = run(&mut i, &mut line, &lines);
                    stop_or_end(&mut writer, &mut send, go);
                }
                "stackTrace" => {
                    let caller = if i % 2 == 1 {
                        "Main.main(String[])"
                    } else {
                        "Main.other()"
                    };
                    send(
                        &mut writer,
                        answer(json!({"stackFrames": [
                            {"id": 100 + i, "name": "Main.work(int)", "line": line,
                             "source": {"path": source}},
                            {"id": 200 + i, "name": caller, "line": 9, "source": {"path": source}},
                        ]})),
                    );
                }
                "scopes" => send(
                    &mut writer,
                    answer(json!({"scopes": [{"name": "Locals", "variablesReference": 1}]})),
                ),
                "variables" => send(
                    &mut writer,
                    answer(json!({"variables": [
                        {"name": "this", "value": format!("Main$Item@{}", 30 + i),
                         "type": "Main$Item", "variablesReference": 0},
                        {"name": "i", "value": i.to_string(), "variablesReference": 0},
                    ]})),
                ),
                "evaluate" => send(&mut writer, answer(json!({"result": i.to_string()}))),
                "continue" => {
                    send(&mut writer, answer(json!({})));
                    let go = run(&mut i, &mut line, &lines);
                    stop_or_end(&mut writer, &mut send, go);
                }
                "disconnect" => {
                    send(&mut writer, answer(Value::Null));
                    return;
                }
                _ => send(&mut writer, answer(json!({}))),
            }
        }
    });
    (address, log)
}

fn source(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-rules-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Main.java");
    std::fs::write(
        &path,
        "class Main {\n  void work(int i) {\n    a();\n    b();\n  }\n}\n",
    )
    .unwrap();
    path.canonicalize().unwrap()
}

fn start(path: &Path, adapter: &str, breakpoints: Vec<SourceBreakpoint>) -> (DebugSession, Log) {
    let (address, log) = fake_adapter(path.to_path_buf());
    let breakpoints = BTreeMap::from([(path.to_path_buf(), breakpoints)]);
    let (session, _) = DebugSession::launch_on(
        DapClient::connect(&address, None).unwrap(),
        adapter,
        json!({}),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    (session, log)
}

/// `line:i` of each stop, until the end.
fn stops(session: &mut DebugSession) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        match session.wait(WAIT).unwrap() {
            Event::Stopped(s) => {
                let top = session.stack_trace(s.thread_id).unwrap()[0].clone();
                let i = session.evaluate("i", top.id).unwrap();
                out.push(format!("{}:{i}", top.line));
                session.resume_continue(s.thread_id).unwrap();
            }
            Event::Exited(_) => return out,
        }
    }
}

fn at(line: u64, rules: BreakpointRules) -> SourceBreakpoint {
    SourceBreakpoint {
        line,
        rules,
        ..SourceBreakpoint::default()
    }
}

/// Behaviour 10: a breakpoint waits for another one; without "stay active" it waits again after
/// each own hit.
#[test]
fn dependent_breakpoints_wait_for_their_master() {
    let path = source("dep");
    let master = SourceBreakpoint {
        line: 3,
        condition: Some("i == 3".into()),
        ..SourceBreakpoint::default()
    };
    let rules = |keep| BreakpointRules {
        depends_on: Some(format!("{}:3", path.display())),
        depends_keep: keep,
        ..BreakpointRules::default()
    };
    let (mut s, _) = start(&path, "java", vec![master.clone(), at(4, rules(false))]);
    assert_eq!(stops(&mut s), ["3:3", "4:3"]);
    let (mut s, _) = start(&path, "java", vec![master, at(4, rules(true))]);
    assert_eq!(stops(&mut s), ["3:3", "4:3", "4:4", "4:5", "4:6"]);
}

/// Behaviours 7–9: the hit message and the stack in the console, no suspend, removal after the
/// first hit.
#[test]
fn messages_no_suspend_and_removal() {
    let path = source("msg");
    let logging = BreakpointRules {
        log_hit: true,
        log_stack: true,
        suspend: Some(Suspend::None),
        ..BreakpointRules::default()
    };
    let (mut s, _) = start(&path, "java", vec![at(4, logging)]);
    assert!(stops(&mut s).is_empty(), "no suspend: the program goes on");
    let console = s.console().to_owned();
    assert_eq!(
        console
            .matches("Breakpoint hit: Main.java:4 in Main.work(int)")
            .count(),
        6,
        "{console}"
    );
    assert!(
        console.contains("\tat Main.other() (Main.java:9)"),
        "{console}"
    );

    let once = BreakpointRules {
        remove_once_hit: true,
        ..BreakpointRules::default()
    };
    let (mut s, log) = start(&path, "java", vec![at(4, once)]);
    assert_eq!(stops(&mut s), ["4:1"]);
    assert_eq!(s.take_removed(), [(path.clone(), 4)]);
    let last = log
        .lock()
        .unwrap()
        .iter()
        .rfind(|r| r["command"] == "setBreakpoints")
        .cloned()
        .unwrap();
    assert_eq!(last["arguments"]["breakpoints"], json!([]));
}

/// Behaviour 11: instance, class and caller filters.
#[test]
fn instance_class_and_caller_filters() {
    let path = source("filters");
    let run = |rules: BreakpointRules| {
        let (mut s, _) = start(&path, "java", vec![at(4, rules)]);
        stops(&mut s)
    };
    let instance = BreakpointRules {
        instance_filter: Some("33 @35".into()),
        ..BreakpointRules::default()
    };
    assert_eq!(run(instance), ["4:3", "4:5"]);
    let excluded = BreakpointRules {
        class_filter: Some("-Main.Item".into()),
        ..BreakpointRules::default()
    };
    assert!(run(excluded).is_empty());
    let class = BreakpointRules {
        class_filter: Some("Main*".into()),
        ..BreakpointRules::default()
    };
    assert_eq!(run(class).len(), 6);
    let caller = BreakpointRules {
        caller_filter: Some("Main.main".into()),
        ..BreakpointRules::default()
    };
    assert_eq!(run(caller), ["4:1", "4:3", "4:5"]);
}

/// Behaviour 6: "this thread only" cannot be honoured by an adapter that stops every thread; the
/// console says so once.
#[test]
fn thread_only_is_explained_where_impossible() {
    let path = source("thread");
    let rules = BreakpointRules {
        suspend: Some(Suspend::Thread),
        ..BreakpointRules::default()
    };
    let (mut s, _) = start(&path, "debugpy", vec![at(4, rules)]);
    assert_eq!(stops(&mut s).len(), 6);
    let console = s.console().to_owned();
    assert_eq!(
        console.matches("stops every thread").count(),
        1,
        "{console}"
    );
}
