//! SPEC-0039 v6 AK-11: a breakpoint condition or log message the adapter fails to evaluate is
//! told in the debug console. java-debug says it in an event of its own (`usernotification`);
//! debugpy is asked to say every failure and to stop at that hit (`setDebuggerProperty`), other
//! adapters are not asked. With real debugpy a condition naming an unknown variable stops at the
//! first hit with the error in the console (skipped without debugpy).

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use idecraft_debugger::{
    DapClient, DebugSession, Event, SourceBreakpoint, StopState, known_adapters, read_message,
    write_message,
};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);
const NOTE: &str = "Breakpoint condition 'i == 3' error: Cannot evaluate";

/// A scripted adapter that notes every request and, once configured, sends an adapter note
/// without a line break, java-debug's notification and a stop.
fn scripted_adapter(requests: Arc<Mutex<Vec<Value>>>) -> String {
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
            requests.lock().unwrap().push(request.clone());
            let command = request["command"].as_str().unwrap_or_default().to_owned();
            let answer = |body: Value| {
                json!({"type": "response", "request_seq": request["seq"], "command": command,
                       "success": true, "body": body})
            };
            match command.as_str() {
                "launch" => {
                    launch = Some(request.clone());
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "initialized"}),
                    );
                }
                "setBreakpoints" => send(
                    &mut writer,
                    answer(json!({"breakpoints": [{"id": 1, "verified": true, "line": 3}]})),
                ),
                "configurationDone" => {
                    send(&mut writer, answer(Value::Null));
                    if let Some(l) = launch.take() {
                        send(
                            &mut writer,
                            json!({"type": "response", "request_seq": l["seq"],
                                   "command": "launch", "success": true}),
                        );
                    }
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "output",
                               "body": {"category": "console", "output": "[Warn] a note"}}),
                    );
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "usernotification",
                               "body": {"notificationType": "ERROR", "message": NOTE}}),
                    );
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "stopped",
                               "body": {"reason": "breakpoint", "threadId": 1}}),
                    );
                }
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

fn start(adapter_id: &str) -> (DebugSession, Arc<Mutex<Vec<Value>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let address = scripted_adapter(requests.clone());
    let breakpoints = BTreeMap::from([(
        PathBuf::from("/nowhere/Main.java"),
        vec![SourceBreakpoint {
            line: 3,
            condition: Some("i == 3".into()),
            ..Default::default()
        }],
    )]);
    let (session, _) = DebugSession::launch_on(
        DapClient::connect(&address, None).unwrap(),
        adapter_id,
        json!({}),
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    (session, requests)
}

fn commands(requests: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
    requests
        .lock()
        .unwrap()
        .iter()
        .map(|r| r["command"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn a_notification_of_the_adapter_reaches_the_console() {
    let (mut session, requests) = start("java");
    assert!(matches!(session.wait(WAIT).unwrap(), Event::Stopped(_)));
    let console = session.console().to_owned();
    assert!(
        console.contains(&format!("[Warn] a note\n{NOTE}\n")),
        "on a line of its own: {console:?}"
    );
    assert!(session.transcript().contains(NOTE));
    // Only debugpy is asked for its setting.
    assert!(
        !commands(&requests).contains(&"setDebuggerProperty".to_owned()),
        "{:?}",
        commands(&requests)
    );
    let _ = session.disconnect();
}

#[test]
fn debugpy_is_asked_to_report_failed_conditions_before_the_breakpoints() {
    let (mut session, requests) = start("debugpy");
    assert!(matches!(session.wait(WAIT).unwrap(), Event::Stopped(_)));
    let list = commands(&requests);
    let at = |name: &str| list.iter().position(|c| c == name);
    let asked = at("setDebuggerProperty").unwrap_or_else(|| panic!("not asked: {list:?}"));
    assert!(asked < at("setBreakpoints").unwrap(), "{list:?}");
    let arguments = requests.lock().unwrap()[asked]["arguments"].clone();
    assert_eq!(arguments["skipPrintBreakpointException"], json!([]));
    assert_eq!(arguments["skipSuspendOnBreakpointException"], json!([]));
    let _ = session.disconnect();
}

const APP: &str = "def main():\n    total = 0\n    for i in range(4):\n        total += i\n    print('total', total)\n\n\nmain()\n";

#[test]
fn a_failed_condition_stops_and_is_told_with_debugpy() {
    let Some(adapter) = known_adapters()
        .into_iter()
        .find(|a| a.name == "debugpy")
        .and_then(|a| a.command)
    else {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: debugpy"
        );
        eprintln!("skipped: debugpy is not installed");
        return;
    };
    let dir = std::env::temp_dir().join(format!("idecraft-conderr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir: PathBuf = dir.canonicalize().unwrap();
    let script = dir.join("app.py");
    std::fs::write(&script, APP).unwrap();
    let launch = json!({
        "type": "python", "request": "launch", "program": script, "cwd": dir,
        "console": "internalConsole", "justMyCode": true, "stopOnEntry": false,
    });
    // Line 4 `total += i`; debugpy alone says nothing about an unknown name and never stops.
    let breakpoints = BTreeMap::from([(
        script.clone(),
        vec![SourceBreakpoint {
            line: 4,
            condition: Some("nosuch == 3".into()),
            ..Default::default()
        }],
    )]);
    let (mut s, _) = DebugSession::launch(
        &adapter,
        "debugpy",
        launch,
        &breakpoints,
        &Default::default(),
    )
    .unwrap();
    let state = s.wait_state(Duration::from_secs(60), 20).unwrap();
    let StopState::Stopped {
        reason,
        frames,
        locals,
        ..
    } = &state
    else {
        panic!("not stopped at the failed condition: {state:?}");
    };
    assert_eq!(reason, "breakpoint");
    assert_eq!(frames[0].line, 4);
    assert!(
        locals.iter().any(|v| v.name == "i" && v.value == "0"),
        "the first hit: {locals:?}"
    );
    let console = s.console().to_owned();
    assert!(
        console.contains("nosuch == 3") && console.contains("NameError"),
        "{console:?}"
    );
    let _ = s.disconnect();
    let _ = std::fs::remove_dir_all(&dir);
}
