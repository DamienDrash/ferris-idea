//! SPEC-0178 AK-1 and AK-2: conditions, hit counts and logpoints against a fake adapter, evaluated
//! by IDECraft when the adapter does not announce them and left to the adapter when it does.
//!
//! The fake adapter runs `for i in 1..=6 { line 4 }` and stops at line 4 whenever a breakpoint is
//! set there; with capabilities it evaluates `i == N` itself. `evaluate` knows `i`, `i == N`,
//! `i % N == 0` and fails for anything else; with `namesOnly` (like kotlin-debug-adapter) it answers
//! `unknown` for everything but `i`. The locals are `i` and `order` with the field `total = 10 * i`.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use idecraft_debugger::{
    CLIENT_SIDE_NOTE, DapClient, DebugSession, EvaluatedBy, Event, SourceBreakpoint, read_message,
    write_message,
};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);
const LAST: i64 = 6;

/// Requests the fake adapter received.
type Log = Arc<Mutex<Vec<Value>>>;

fn eval(expression: &str, i: i64) -> Option<String> {
    let expression = expression.trim();
    if expression == "i" {
        return Some(i.to_string());
    }
    if let Some(n) = expression.strip_prefix("i == ") {
        return Some((i == n.parse::<i64>().ok()?).to_string());
    }
    if let Some(rest) = expression.strip_prefix("i % ") {
        let n: i64 = rest.strip_suffix(" == 0")?.parse().ok()?;
        return Some((i % n == 0).to_string());
    }
    if expression == "name" {
        return Some("\"text\"".into());
    }
    None
}

/// Starts the fake adapter; returns its address and the requests it receives.
fn fake_adapter(source: PathBuf, capabilities: Value) -> (String, Log) {
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
        let mut i = 0;
        // Line 4's breakpoint as the adapter keeps it, and its own condition (when announced).
        let mut armed = false;
        let mut condition: Option<String> = None;
        let mut launch_seq = None;
        while let Ok(Some(request)) = read_message(&mut reader) {
            seen.lock().unwrap().push(request.clone());
            let command = request["command"].as_str().unwrap_or_default().to_owned();
            let answer = |body: Value| {
                json!({"type": "response", "request_seq": request["seq"], "command": command,
                       "success": true, "body": body})
            };
            // Runs to the next stop at line 4 or to the end.
            let run = |i: &mut i64, armed: bool, condition: &Option<String>| loop {
                *i += 1;
                if *i > LAST {
                    return None;
                }
                let own = condition
                    .as_deref()
                    .is_none_or(|c| eval(c, *i) == Some("true".into()));
                if armed && own {
                    return Some(*i);
                }
            };
            match command.as_str() {
                "initialize" => send(&mut writer, answer(capabilities.clone())),
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
                    armed = list.iter().any(|b| b["line"] == 4);
                    condition = list
                        .iter()
                        .find_map(|b| b["condition"].as_str().map(str::to_owned));
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
                    match run(&mut i, armed, &condition) {
                        Some(_) => send(
                            &mut writer,
                            json!({"type": "event", "event": "stopped",
                            "body": {"reason": "breakpoint", "threadId": 1}}),
                        ),
                        None => send(
                            &mut writer,
                            json!({"type": "event", "event": "exited",
                            "body": {"exitCode": 0}}),
                        ),
                    }
                }
                "stackTrace" => send(
                    &mut writer,
                    answer(json!({"stackFrames": [
                        {"id": 100 + i, "name": "main", "line": 4, "source": {"path": source}},
                    ]})),
                ),
                "scopes" => send(
                    &mut writer,
                    answer(json!({"scopes": [
                        {"name": "Locals", "variablesReference": 1},
                    ]})),
                ),
                "variables" => {
                    let list = match request["arguments"]["variablesReference"].as_i64() {
                        Some(1) => json!([
                            {"name": "i", "value": i.to_string(), "variablesReference": 0},
                            {"name": "order", "value": "Order", "variablesReference": 2},
                        ]),
                        _ => {
                            json!([{"name": "total", "value": (10 * i).to_string(), "variablesReference": 0}])
                        }
                    };
                    send(&mut writer, answer(json!({"variables": list})));
                }
                "evaluate" => {
                    let expression = request["arguments"]["expression"]
                        .as_str()
                        .unwrap_or_default();
                    if capabilities["namesOnly"] == true && expression != "i" {
                        send(&mut writer, answer(json!({"result": "unknown"})));
                        continue;
                    }
                    match eval(expression, i) {
                        Some(result) => send(&mut writer, answer(json!({"result": result}))),
                        None => send(
                            &mut writer,
                            json!({"type": "response",
                            "request_seq": request["seq"], "command": "evaluate",
                            "success": false, "message": format!("unknown name {expression}")}),
                        ),
                    }
                }
                "continue" | "next" => {
                    send(&mut writer, answer(json!({})));
                    // The program prints as it leaves the stop (D17's ordering).
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "output",
                        "body": {"category": "stdout", "output": format!("left {i}\n")}}),
                    );
                    // Stepping lands on line 4 again in the next iteration, as a step.
                    let reason = if command == "next" {
                        "step"
                    } else {
                        "breakpoint"
                    };
                    let next = if command == "next" {
                        i += 1;
                        (i <= LAST).then_some(i)
                    } else {
                        run(&mut i, armed, &condition)
                    };
                    match next {
                        Some(_) => send(
                            &mut writer,
                            json!({"type": "event", "event": "stopped",
                            "body": {"reason": reason, "threadId": 1}}),
                        ),
                        None => send(
                            &mut writer,
                            json!({"type": "event", "event": "exited",
                            "body": {"exitCode": 0}}),
                        ),
                    }
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

fn source() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-conditions-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Main.kt");
    std::fs::write(
        &path,
        "fun main() {\n    for (i in 1..6) {\n        val x = i\n        println(x)\n    }\n}\n",
    )
    .unwrap();
    path
}

fn start(
    capabilities: Value,
    breakpoint: SourceBreakpoint,
) -> (DebugSession, Vec<idecraft_debugger::Breakpoint>, Log) {
    let path = source();
    let (address, log) = fake_adapter(path.clone(), capabilities);
    let breakpoints = BTreeMap::from([(path.clone(), vec![breakpoint])]);
    let (session, mut placed) = DebugSession::launch_on(
        DapClient::connect(&address, None).unwrap(),
        "kotlin",
        json!({}),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    (session, placed.remove(&path).unwrap(), log)
}

/// The values of `i` at which the program stopped, until it ended.
fn stops(session: &mut DebugSession) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        match session.wait(WAIT).unwrap() {
            Event::Stopped(s) => {
                let frame = session.stack_trace(s.thread_id).unwrap()[0].id;
                out.push(session.evaluate("i", frame).unwrap());
                session.resume_continue(s.thread_id).unwrap();
            }
            Event::Exited(_) => return out,
        }
    }
}

fn sent(log: &Log, command: &str) -> Vec<Value> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|r| r["command"] == command)
        .cloned()
        .collect()
}

fn line(condition: Option<&str>, hit: Option<&str>, log: Option<&str>) -> SourceBreakpoint {
    SourceBreakpoint {
        line: 4,
        condition: condition.map(str::to_owned),
        hit_condition: hit.map(str::to_owned),
        log_message: log.map(str::to_owned),
        ..SourceBreakpoint::default()
    }
}

#[test]
fn condition_evaluated_by_idecraft() {
    let (mut session, placed, log) = start(json!({}), line(Some("i == 3"), None, None));
    assert_eq!(placed[0].evaluated_by, EvaluatedBy::IdeCraft);
    assert_eq!(stops(&mut session), ["3"]);
    let set = sent(&log, "setBreakpoints");
    assert!(
        set[0]["arguments"]["breakpoints"][0]
            .get("condition")
            .is_none(),
        "{set:?}"
    );
    assert!(session.console().contains(CLIENT_SIDE_NOTE));
}

#[test]
fn hit_conditions_evaluated_by_idecraft() {
    let (mut session, _, _) = start(json!({}), line(None, Some("3"), None));
    assert_eq!(stops(&mut session), ["3"]);
    let (mut session, _, _) = start(json!({}), line(None, Some("%2"), None));
    assert_eq!(stops(&mut session), ["2", "4", "6"]);
    // The hit count counts only hits whose condition is true.
    let (mut session, _, _) = start(json!({}), line(Some("i % 2 == 0"), Some("2"), None));
    assert_eq!(stops(&mut session), ["4"]);
}

#[test]
fn logpoint_logs_and_never_stops() {
    let (mut session, _, _) = start(
        json!({}),
        line(Some("i % 3 == 0"), None, Some("i is {i}, {nope}")),
    );
    assert_eq!(stops(&mut session), Vec::<String>::new());
    let console = session.console().to_owned();
    assert!(console.contains("i is 3, <error: "), "{console}");
    assert!(console.contains("i is 6, "), "{console}");
    assert!(!console.contains("i is 1"), "{console}");
    // D17: the messages stand between the program's output as it ran.
    let all = session.transcript().to_owned();
    let order: Vec<&str> = all
        .lines()
        .filter(|l| l.starts_with("i is") || l.starts_with("left"))
        .map(|l| &l[..6])
        .collect();
    assert_eq!(
        order,
        [
            "left 1", "left 2", "i is 3", "left 3", "left 4", "left 5", "i is 6", "left 6"
        ],
        "{all}"
    );
}

#[test]
fn failing_condition_stops_with_the_reason() {
    let (mut session, _, _) = start(json!({}), line(Some("nope > 1"), None, None));
    assert_eq!(stops(&mut session).len(), LAST as usize);
    let console = session.console().to_owned();
    assert!(
        console.contains("The condition `nope > 1` at Main.kt:4 could not be evaluated: evaluate failed: unknown name nope"),
        "{console}"
    );
    // Not a truth value: stops as well.
    let (mut session, _, _) = start(json!({}), line(Some("name"), None, None));
    assert_eq!(stops(&mut session).len(), LAST as usize);
    assert!(session.console().contains("not true or false"));
}

#[test]
fn a_step_onto_the_line_is_not_evaluated() {
    let (mut session, _, log) = start(json!({}), line(Some("i == 2"), None, None));
    let Event::Stopped(s) = session.wait(WAIT).unwrap() else {
        panic!("not stopped")
    };
    session.next(s.thread_id).unwrap();
    let Event::Stopped(s) = session.wait(WAIT).unwrap() else {
        panic!("not stopped after the step")
    };
    assert_eq!(s.reason, "step");
    let frame = session.stack_trace(s.thread_id).unwrap()[0].id;
    assert_eq!(session.evaluate("i", frame).unwrap(), "3");
    // Two evaluations for the hits 1 and 2, none for the step.
    let conditions = sent(&log, "evaluate")
        .iter()
        .filter(|r| r["arguments"]["expression"] == "i == 2")
        .count();
    assert_eq!(conditions, 2);
}

#[test]
fn adapter_with_capabilities_evaluates_itself() {
    let caps = json!({"supportsConditionalBreakpoints": true});
    let (mut session, placed, log) = start(caps, line(Some("i == 5"), None, None));
    assert_eq!(placed[0].evaluated_by, EvaluatedBy::Adapter);
    assert_eq!(stops(&mut session), ["5"]);
    let set = sent(&log, "setBreakpoints");
    assert_eq!(set[0]["arguments"]["breakpoints"][0]["condition"], "i == 5");
    // Only the test's own `i` evaluations, no condition.
    assert!(
        sent(&log, "evaluate")
            .iter()
            .all(|r| r["arguments"]["expression"] == "i")
    );
    assert!(!session.console().contains(CLIENT_SIDE_NOTE));
}

#[test]
fn unknown_hit_condition_is_refused() {
    let path = source();
    let (address, _) = fake_adapter(path.clone(), json!({}));
    let breakpoints = BTreeMap::from([(path, vec![line(None, Some("often"), None)])]);
    let err = DebugSession::launch_on(
        DapClient::connect(&address, None).unwrap(),
        "kotlin",
        json!({}),
        &breakpoints,
        &Default::default(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(
        err.contains("hit condition `often` is not understood"),
        "{err}"
    );
}

#[test]
fn names_only_adapter_gets_conditions_evaluated_by_idecraft() {
    let names_only = json!({"namesOnly": true});
    let (mut session, _, _) = start(
        names_only.clone(),
        line(
            Some("i % 2 == 0 && order.total > 20"),
            None,
            Some("i={i}, twice={i * 2}, total={order.total}"),
        ),
    );
    assert_eq!(stops(&mut session), Vec::<String>::new());
    let console = session.console().to_owned();
    assert!(
        console.contains("i=4, twice=8, total=40\ni=6, twice=12, total=60\n"),
        "{console}"
    );
    let (mut session, _, _) = start(
        names_only.clone(),
        line(Some("i == 2 || order.total == 50"), None, None),
    );
    assert_eq!(stops(&mut session), ["2", "5"]);
    // A call cannot be evaluated by either: stops with both reasons.
    let (mut session, _, _) = start(
        names_only,
        line(Some("i.coerceAtLeast(3) == 3"), None, None),
    );
    assert_eq!(stops(&mut session).len(), LAST as usize);
    let console = session.console().to_owned();
    assert!(
        console.contains(
            "gave `unknown`, not true or false; IDECraft cannot evaluate it either: calls"
        ),
        "{console}"
    );
}
