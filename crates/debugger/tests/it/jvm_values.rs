//! SPEC-0091 v16 AK-21: against a fake adapter that answers like kotlin-debug-adapter — a `this`
//! that is null in a frame without a receiver, objects as `instance of …(id=…)`, no child counts —
//! the session leaves `this` out, writes `Type@id`, reads the fields of library collections and
//! boxed values for their summary, and sends no `evaluate` for it.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ferris_idea_debugger::{DapClient, DebugSession, read_message, write_message};
use serde_json::{Value, json};

type Log = Arc<Mutex<Vec<Value>>>;

fn object(name: &str, class: &str, id: i64) -> Value {
    json!({"name": name, "type": class, "value": format!("instance of {class}(id={id})"),
           "variablesReference": id})
}

fn plain(name: &str, value: &str) -> Value {
    json!({"name": name, "type": "java.util.X", "value": value, "variablesReference": 0})
}

/// The variables behind `reference`: 2 is the frame of a top-level `main`, 3 the frame of a member
/// function, 4 a frame with forty lists; the others are objects.
fn variables(reference: i64) -> Vec<Value> {
    match reference {
        2 => vec![
            json!({"name": "this", "type": "Unknown type", "value": "null",
                   "variablesReference": 0}),
            object("names", "java.util.ArrayList", 566),
            json!({"name": "first", "type": "java.lang.String", "value": "\"Rex\"",
                   "variablesReference": 567}),
            object("pet", "demo.Dog", 568),
            json!({"name": "arr", "type": "int[]", "value": "instance of int[3] (id=570)",
                   "variablesReference": 570}),
            object("boxed", "java.lang.Integer", 571),
            object("set", "java.util.LinkedHashSet", 572),
            object("empty", "kotlin.collections.EmptyList", 573),
            json!({"name": "nothing", "type": "Unknown type", "value": "null",
                   "variablesReference": 0}),
            object("odd", "java.util.Random", 580),
        ],
        3 => vec![object("this", "demo.Dog", 568)],
        4 => (0..40)
            .map(|i| object(&format!("l{i}"), "java.util.ArrayList", 1000 + i))
            .collect(),
        566 | 1000.. => vec![
            plain("DEFAULT_CAPACITY", "10"),
            json!({"name": "elementData", "type": "java.util.ArrayList",
                   "value": "instance of java.lang.Object[10] (id=581)",
                   "variablesReference": 581}),
            plain("size", "1"),
        ],
        571 => vec![plain("MAX_VALUE", "2147483647"), plain("value", "300")],
        572 => vec![object("map", "java.util.LinkedHashMap", 593)],
        593 => vec![plain("size", "2"), plain("modCount", "2")],
        580 => vec![plain("seed", "5")],
        _ => Vec::new(),
    }
}

/// Starts the fake adapter; returns its address and the requests it receives.
fn fake_kotlin_debug() -> (String, Log) {
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
                "launch" => {
                    launch = Some(request.clone());
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "initialized"}),
                    );
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
                }
                "variables" => {
                    let reference = request["arguments"]["variablesReference"]
                        .as_i64()
                        .unwrap_or_default();
                    send(
                        &mut writer,
                        answer(json!({"variables": variables(reference)})),
                    );
                }
                // As the adapter answers a name it knows.
                "evaluate" => send(
                    &mut writer,
                    answer(json!({"result": "instance of java.util.ArrayList(id=566)",
                                  "variablesReference": 566})),
                ),
                _ => send(&mut writer, answer(json!({}))),
            }
        }
    });
    (address, log)
}

fn session(adapter_id: &str) -> (DebugSession, Log) {
    let (address, log) = fake_kotlin_debug();
    let client = DapClient::connect(&address, None).unwrap();
    let (session, _) = DebugSession::launch_on(
        client,
        adapter_id,
        json!({}),
        &BTreeMap::new(),
        &Default::default(),
    )
    .unwrap();
    (session, log)
}

/// The references of the `variables` requests and the number of `evaluate` requests so far.
fn asked(log: &Log) -> (Vec<i64>, usize) {
    let log = log.lock().unwrap();
    let references = log
        .iter()
        .filter(|r| r["command"] == "variables")
        .filter_map(|r| r["arguments"]["variablesReference"].as_i64())
        .collect();
    let evaluated = log.iter().filter(|r| r["command"] == "evaluate").count();
    (references, evaluated)
}

#[test]
fn fields_are_read_and_nothing_is_evaluated() {
    let (mut session, log) = session("kotlin");
    let locals = session.variables(2).unwrap();
    let shown: Vec<(&str, &str, Option<&str>)> = locals
        .iter()
        .map(|v| (v.name.as_str(), v.value.as_str(), v.kind.as_deref()))
        .collect();
    assert_eq!(
        shown,
        [
            (
                "names",
                "ArrayList@566 size = 1",
                Some("java.util.ArrayList")
            ),
            ("first", "\"Rex\"", Some("java.lang.String")),
            ("pet", "Dog@568", Some("demo.Dog")),
            ("arr", "int[3]@570", Some("int[]")),
            ("boxed", "300", Some("java.lang.Integer")),
            (
                "set",
                "LinkedHashSet@572 size = 2",
                Some("java.util.LinkedHashSet")
            ),
            (
                "empty",
                "EmptyList@573 size = 0",
                Some("kotlin.collections.EmptyList")
            ),
            ("nothing", "null", None),
            ("odd", "Random@580", Some("java.util.Random")),
        ]
    );
    // The rows still open: the references are the adapter's.
    assert_eq!(locals[0].reference, 566);
    // Fields were read for the list, the boxed number, the set with its map and the library
    // class without a size; not for the program's own class, the array, the string or the list
    // that is always empty. Nothing was evaluated.
    assert_eq!(asked(&log), (vec![2, 566, 571, 572, 593, 580], 0));

    // A member function keeps its object.
    let member = session.variables(3).unwrap();
    assert_eq!(
        (member[0].name.as_str(), member[0].value.as_str()),
        ("this", "Dog@568")
    );

    // At most thirty rows of one answer are asked; the rest show type and id.
    let before = asked(&log).0.len();
    let many = session.variables(4).unwrap();
    assert_eq!(many.len(), 40);
    assert_eq!(many[29].value, "ArrayList@1029 size = 1");
    assert_eq!(many[30].value, "ArrayList@1030");
    assert_eq!(asked(&log).0.len() - before, 1 + 30);

    // An evaluated value has the same form: the one request the user asked for, then fields.
    assert_eq!(
        session.evaluate("names", 1).unwrap(),
        "ArrayList@566 size = 1"
    );
    assert_eq!(asked(&log).1, 1);
}

#[test]
fn other_adapters_keep_their_values() {
    let (mut session, log) = session("lldb");
    let locals = session.variables(2).unwrap();
    assert_eq!(locals.len(), 10);
    assert_eq!(
        (locals[0].name.as_str(), locals[0].kind.as_deref()),
        ("this", Some("Unknown type"))
    );
    assert_eq!(locals[1].value, "instance of java.util.ArrayList(id=566)");
    assert_eq!(asked(&log), (vec![2], 0));
}
