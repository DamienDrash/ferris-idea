//! SPEC-0146 v2 (hand test 72, D30): against a fake adapter that answers like java-debug — caught and
//! uncaught filters, function breakpoints, no exception filter options — exception types go through
//! the client's settings hook and method breakpoints are named `package.Class#method`.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use idecraft_debugger::{
    DapClient, DebugSession, FunctionBreakpoint, SpecialBreakpoints, java_method_name,
    read_message, write_message,
};
use serde_json::{Value, json};

type Log = Arc<Mutex<Vec<Value>>>;

/// Starts the fake adapter; returns its address and the requests it receives.
fn fake_java_debug() -> (String, Log) {
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
        while let Ok(Some(request)) = read_message(&mut reader) {
            seen.lock().unwrap().push(request.clone());
            let command = request["command"].as_str().unwrap_or_default().to_owned();
            let answer = |body: Value| {
                json!({"type": "response", "request_seq": request["seq"], "command": command,
                       "success": true, "body": body})
            };
            match command.as_str() {
                "initialize" => send(
                    &mut writer,
                    answer(json!({
                        "supportsFunctionBreakpoints": true,
                        "supportsConditionalBreakpoints": true,
                        "exceptionBreakpointFilters": [
                            {"filter": "caught", "label": "Caught Exceptions"},
                            {"filter": "uncaught", "label": "Uncaught Exceptions"},
                        ],
                    })),
                ),
                "launch" => {
                    launch = Some(request.clone());
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "initialized"}),
                    );
                }
                "setFunctionBreakpoints" => {
                    let placed: Vec<Value> = request["arguments"]["breakpoints"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|_| json!({"verified": true}))
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
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "exited", "body": {"exitCode": 0}}),
                    );
                }
                _ => send(&mut writer, answer(Value::Null)),
            }
        }
    });
    (address, log)
}

#[test]
fn exception_types_and_method_breakpoints_reach_java_debug() {
    let (address, log) = fake_java_debug();
    let types: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let given = types.clone();
    let client = DapClient::connect(&address, None)
        .unwrap()
        .with_exception_types(Box::new(move |t| {
            given.lock().unwrap().push(t.to_vec());
            Ok(())
        }));
    let special = SpecialBreakpoints {
        caught: true,
        uncaught: false,
        exception_condition: "java.lang.NullPointerException".into(),
        functions: vec![FunctionBreakpoint {
            name: "demo.Crash.area".into(),
            condition: None,
            ..Default::default()
        }],
        ..SpecialBreakpoints::default()
    };
    let (mut session, _) =
        DebugSession::launch_on(client, "java", json!({}), &BTreeMap::new(), &special).unwrap();
    let output = session.output().to_owned();
    assert!(!output.contains("[IDECraft]"), "{output}");
    assert_eq!(
        *types.lock().unwrap(),
        vec![vec!["java.lang.NullPointerException".to_owned()]]
    );
    let log = log.lock().unwrap();
    let request = |c: &str| {
        log.iter()
            .find(|r| r["command"] == c)
            .unwrap_or_else(|| panic!("no {c}"))["arguments"]
            .clone()
    };
    assert_eq!(
        request("setExceptionBreakpoints"),
        json!({"filters": ["caught"]})
    );
    assert_eq!(
        request("setFunctionBreakpoints")["breakpoints"][0]["name"],
        "demo.Crash#area"
    );
}

#[test]
fn method_names_in_java_debug_syntax() {
    assert_eq!(java_method_name("demo.Crash.area"), "demo.Crash#area");
    assert_eq!(java_method_name("demo.Crash#area"), "demo.Crash#area");
    assert_eq!(java_method_name("Crash::area"), "Crash#area");
    assert_eq!(java_method_name("main"), "main");
    assert_eq!(
        idecraft_debugger::exception_types("a.B, c.D  e.F"),
        ["a.B", "c.D", "e.F"]
    );
}
