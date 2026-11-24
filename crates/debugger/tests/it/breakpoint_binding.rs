//! SPEC-0127 v5 AK-9: whether the adapter bound a breakpoint, and why not, reaches the placed
//! breakpoints with the line asked for; a later `breakpoint` event that binds one is reported once
//! as a change of its file; pending breakpoints carry the adapter's reason.
//!
//! The scripted adapter binds the first breakpoint of a file (moving it one line down) and
//! answers the second with `verified: false` and a message. With `binds_later` it sends a
//! `breakpoint` event binding the second one after `configurationDone`, before the program stops.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_debugger::{
    DapClient, DebugSession, Event, SourceBreakpoint, read_message, write_message,
};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);
const REASON: &str = "Class Main is not loaded yet";

/// Starts the scripted adapter; returns its address.
fn scripted_adapter(source: PathBuf, binds_later: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
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
        let mut launch = None;
        while let Ok(Some(request)) = read_message(&mut reader) {
            let command = request["command"].as_str().unwrap_or_default().to_owned();
            let answer = |body: Value| {
                json!({"type": "response", "request_seq": request["seq"], "command": command,
                       "success": true, "body": body})
            };
            match command.as_str() {
                "initialize" => send(&mut writer, answer(json!({}))),
                "launch" => {
                    launch = Some(request.clone());
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
                    let placed: Vec<Value> = list
                        .iter()
                        .enumerate()
                        .map(|(n, b)| {
                            let line = b["line"].as_i64().unwrap_or(0);
                            if n == 0 {
                                json!({"id": 1, "verified": true, "line": line + 1})
                            } else {
                                json!({"id": n + 1, "verified": false, "line": line,
                                       "message": REASON})
                            }
                        })
                        .collect();
                    send(&mut writer, answer(json!({"breakpoints": placed})));
                }
                "configurationDone" => {
                    send(&mut writer, answer(Value::Null));
                    if let Some(l) = launch.take() {
                        send(
                            &mut writer,
                            json!({"type": "response", "request_seq": l["seq"],
                                   "command": "launch", "success": true}),
                        );
                    }
                    if binds_later {
                        send(
                            &mut writer,
                            json!({"type": "event", "event": "breakpoint", "body": {
                                "reason": "changed",
                                "breakpoint": {"id": 2, "verified": true, "line": 7}}}),
                        );
                    }
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "stopped",
                               "body": {"reason": "breakpoint", "threadId": 1}}),
                    );
                }
                "stackTrace" => send(
                    &mut writer,
                    answer(json!({"stackFrames": [
                        {"id": 100, "name": "Main.main", "line": 4, "source": {"path": source}},
                    ]})),
                ),
                "disconnect" => {
                    send(&mut writer, answer(Value::Null));
                    return;
                }
                _ => send(&mut writer, answer(json!({}))),
            }
        }
    });
    address
}

fn source(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-binding-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Main.java");
    std::fs::write(
        &path,
        "class Main {\n  public static void main(String[] a) {\n\n    run();\n  }\n\n  void member() {}\n}\n",
    )
    .unwrap();
    path.canonicalize().unwrap()
}

fn start(
    path: &Path,
    binds_later: bool,
) -> (
    DebugSession,
    BTreeMap<PathBuf, Vec<ferris_idea_debugger::Breakpoint>>,
) {
    let address = scripted_adapter(path.to_path_buf(), binds_later);
    let breakpoints = BTreeMap::from([(
        path.to_path_buf(),
        vec![SourceBreakpoint::from(3), SourceBreakpoint::from(7)],
    )]);
    DebugSession::launch_on(
        DapClient::connect(&address, None).unwrap(),
        "java",
        json!({}),
        &breakpoints,
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn the_answer_says_which_breakpoints_are_bound_and_why_not() {
    let path = source("answer");
    let (mut session, placed) = start(&path, false);
    let list = &placed[&path];
    assert_eq!(list.len(), 2, "{list:?}");
    assert!(list[0].verified);
    assert_eq!(list[0].requested_line, Some(3));
    assert_eq!(list[0].line, Some(4), "the adapter moved it");
    assert_eq!(list[0].reason, None);
    assert!(!list[1].verified);
    assert_eq!(list[1].requested_line, Some(7));
    assert_eq!(list[1].reason.as_deref(), Some(REASON));
    // Agents see the reason in the JSON of the placed breakpoints (behaviour 15).
    let shown = serde_json::to_value(&list[1]).unwrap();
    assert_eq!(shown["verified"], json!(false));
    assert_eq!(shown["reason"], json!(REASON));
    assert!(matches!(session.wait(WAIT).unwrap(), Event::Stopped(_)));
    // Nothing changed through events.
    assert!(session.take_rebound().is_empty());
    let pending = session.pending_breakpoints();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0].line, Some(7));
    assert_eq!(pending[0].reason.as_deref(), Some(REASON));
    assert!(!pending[0].hint.is_empty());
    let _ = session.disconnect();
}

#[test]
fn a_later_binding_is_reported_once_for_its_file() {
    let path = source("event");
    let (mut session, placed) = start(&path, true);
    assert!(!placed[&path][1].verified);
    assert!(matches!(session.wait(WAIT).unwrap(), Event::Stopped(_)));
    // The breakpoints as the adapter has them now (v6 behaviour 15: what agents are told after
    // the wait; js-debug binds every breakpoint only through such events).
    let now = session.placed();
    assert!(now[&path][1].verified, "{now:?}");
    assert_eq!(now[&path][1].reason, None);
    assert_eq!(now[&path][1].requested_line, Some(7));
    let rebound = session.take_rebound();
    assert_eq!(rebound.len(), 1, "{rebound:?}");
    let (file, list) = &rebound[0];
    assert_eq!(file, &path);
    assert!(list[1].verified);
    assert_eq!(
        list[1].reason, None,
        "the reason went with the unbound state"
    );
    assert_eq!(list[1].requested_line, Some(7));
    assert!(session.take_rebound().is_empty(), "reported once");
    assert!(session.pending_breakpoints().is_empty());
    let _ = session.disconnect();
}
