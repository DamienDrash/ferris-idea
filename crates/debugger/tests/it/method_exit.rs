//! SPEC-0146 v6 AK-v6-2: method breakpoints that watch the exit, emulated by IDECraft with a step
//! out, and switched-off method breakpoints, against a fake adapter.
//!
//! The fake program is `main` calling `area` once: a function breakpoint on `area` stops on
//! entry (reason `function breakpoint`, the breakpoint's id among `hitBreakpointIds`); `stepOut`
//! stops in `main` on line 7; `continue` ends the program.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use idecraft_debugger::{
    DapClient, DebugSession, Event, FunctionBreakpoint, SpecialBreakpoints, read_message,
    write_message,
};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);

type Log = Arc<Mutex<Vec<Value>>>;

fn fake_adapter() -> (String, Log) {
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
        let mut launch = None;
        // The id of the function breakpoint on `area`, if one was set.
        let mut area: Option<i64> = None;
        // Where the program stands: "start", "area", "main", "end".
        let mut at = "start";
        while let Ok(Some(request)) = read_message(&mut reader) {
            seen.lock().unwrap().push(request.clone());
            let command = request["command"].as_str().unwrap_or_default().to_owned();
            let answer = |body: Value| {
                json!({"type": "response", "request_seq": request["seq"], "command": command,
                       "success": true, "body": body})
            };
            let end =
                |w: &mut std::net::TcpStream,
                 send: &mut dyn FnMut(&mut std::net::TcpStream, Value)| {
                    send(
                        w,
                        json!({"type": "event", "event": "exited", "body": {"exitCode": 0}}),
                    );
                };
            match command.as_str() {
                "initialize" => send(
                    &mut writer,
                    answer(json!({"supportsFunctionBreakpoints": true,
                                  "supportsConditionalBreakpoints": true})),
                ),
                "launch" => {
                    launch = Some(request.clone());
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "initialized"}),
                    );
                }
                "setFunctionBreakpoints" => {
                    let list = request["arguments"]["breakpoints"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    area = list
                        .iter()
                        .position(|b| b["name"] == "area")
                        .map(|i| i as i64 + 1);
                    let placed: Vec<Value> = (1..=list.len())
                        .map(|id| json!({"id": id, "verified": true}))
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
                    match area {
                        Some(id) => {
                            at = "area";
                            send(
                                &mut writer,
                                json!({"type": "event", "event": "stopped", "body": {
                                    "reason": "function breakpoint", "threadId": 1,
                                    "hitBreakpointIds": [id]}}),
                            );
                        }
                        None => {
                            at = "end";
                            end(&mut writer, &mut send);
                        }
                    }
                }
                "threads" => send(
                    &mut writer,
                    answer(json!({"threads": [{"id": 1, "name": "main"}]})),
                ),
                "stackTrace" => {
                    let frames = match at {
                        "area" => json!([
                            {"id": 1, "name": "area", "line": 3, "source": {"path": "/p/a.c"}},
                            {"id": 2, "name": "main", "line": 7, "source": {"path": "/p/a.c"}},
                        ]),
                        _ => json!([
                            {"id": 2, "name": "main", "line": 7, "source": {"path": "/p/a.c"}},
                        ]),
                    };
                    send(&mut writer, answer(json!({"stackFrames": frames})));
                }
                "stepOut" => {
                    send(&mut writer, answer(json!({})));
                    at = "main";
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "stopped",
                               "body": {"reason": "step", "threadId": 1}}),
                    );
                }
                "continue" => {
                    send(&mut writer, answer(json!({})));
                    at = "end";
                    end(&mut writer, &mut send);
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

fn start(functions: Vec<FunctionBreakpoint>) -> (DebugSession, Log) {
    let (address, log) = fake_adapter();
    let special = SpecialBreakpoints {
        functions,
        ..SpecialBreakpoints::default()
    };
    let (session, _) = DebugSession::launch_on(
        DapClient::connect(&address, None).unwrap(),
        "lldb-dap",
        json!({}),
        &BTreeMap::new(),
        &special,
    )
    .unwrap();
    (session, log)
}

fn area(entry: bool, exit: bool) -> FunctionBreakpoint {
    FunctionBreakpoint {
        name: "area".into(),
        entry,
        exit,
        ..FunctionBreakpoint::default()
    }
}

fn commands(log: &Log) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|r| r["command"].as_str().map(str::to_owned))
        .collect()
}

/// The top frame's name and line of a stop, with its reason.
fn stop(session: &mut DebugSession) -> Option<(String, String, i64)> {
    match session.wait(WAIT).unwrap() {
        Event::Stopped(s) => {
            let top = session.stack_trace(s.thread_id).unwrap()[0].clone();
            Some((s.reason, top.name, top.line))
        }
        Event::Exited(_) => None,
    }
}

/// Exit only: the entry hit steps out at once; the wait ends after the return.
#[test]
fn exit_only_steps_out_at_the_entry_hit() {
    let (mut s, log) = start(vec![area(false, true)]);
    assert_eq!(stop(&mut s), Some(("method exit".into(), "main".into(), 7)));
    assert!(commands(&log).contains(&"stepOut".to_owned()));
    s.resume_continue(1).unwrap();
    assert_eq!(stop(&mut s), None);
}

/// Entry and exit: the stop at entry, Resume steps out and stops after the return, the next
/// Resume runs on.
#[test]
fn entry_and_exit_stop_twice() {
    let (mut s, log) = start(vec![area(true, true)]);
    let first = stop(&mut s).unwrap();
    assert_eq!((first.1.as_str(), first.2), ("area", 3));
    assert!(first.0.contains("function"), "{first:?}");
    assert!(!commands(&log).contains(&"stepOut".to_owned()));
    s.resume_continue(1).unwrap();
    assert_eq!(stop(&mut s), Some(("method exit".into(), "main".into(), 7)));
    s.resume_continue(1).unwrap();
    assert_eq!(stop(&mut s), None);
}

/// Entry only stops once, and a switched-off method breakpoint is not sent.
#[test]
fn entry_only_and_switched_off() {
    let (mut s, _) = start(vec![area(true, false)]);
    assert_eq!(stop(&mut s).map(|t| t.1), Some("area".into()));
    s.resume_continue(1).unwrap();
    assert_eq!(stop(&mut s), None);

    let off = FunctionBreakpoint {
        disabled: true,
        ..area(true, false)
    };
    let (mut s, log) = start(vec![off]);
    assert_eq!(stop(&mut s), None, "nothing stops");
    assert!(!commands(&log).contains(&"setFunctionBreakpoints".to_owned()));
}
