//! SPEC-0054 v6 AK-7: registers, memory and disassembly of a stopped native session against a stub
//! adapter answering with fixed DAP responses the way lldb-dap does (no lldb-dap on the build
//! server). A second stub reports no such capabilities: every read is refused with a reason.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use idecraft_debugger::native::{MAX_INSTRUCTIONS, MAX_MEMORY_BYTES};
use idecraft_debugger::{
    DapClient, DebugSession, Event, SpecialBreakpoints, read_message, write_message,
};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);

type Log = Arc<Mutex<Vec<Value>>>;

/// A stub of lldb-dap stopped on entry in `main`; `native` says whether it reports
/// `supportsReadMemoryRequest` and `supportsDisassembleRequest` and a register scope.
fn stub_adapter(native: bool) -> (String, Log) {
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
                    answer(json!({"supportsReadMemoryRequest": native,
                                  "supportsDisassembleRequest": native})),
                ),
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
                    send(
                        &mut writer,
                        json!({"type": "event", "event": "stopped",
                               "body": {"reason": "entry", "threadId": 1}}),
                    );
                }
                "threads" => send(
                    &mut writer,
                    answer(json!({"threads": [{"id": 1, "name": "main"}]})),
                ),
                "stackTrace" => send(
                    &mut writer,
                    answer(json!({"stackFrames": [
                        {"id": 7, "name": "main", "line": 3, "source": {"path": "/src/main.c"},
                         "instructionPointerReference": "0x100003f84"}]})),
                ),
                "scopes" => {
                    let mut scopes = vec![json!({"name": "Locals", "variablesReference": 1})];
                    if native {
                        scopes.push(json!({"name": "Registers", "variablesReference": 2}));
                    }
                    send(&mut writer, answer(json!({"scopes": scopes})));
                }
                "variables" => {
                    let vars = match request["arguments"]["variablesReference"].as_i64() {
                        Some(2) => json!([
                            {"name": "General Purpose Registers", "value": "",
                             "variablesReference": 3},
                            {"name": "Floating Point Registers", "value": "",
                             "variablesReference": 4},
                        ]),
                        Some(3) => json!([
                            {"name": "rip", "value": "0x0000000100003f84", "variablesReference": 0},
                            {"name": "rsp", "value": "0x00007ff7bfeff6b0", "variablesReference": 0},
                        ]),
                        Some(4) => json!([
                            {"name": "fctrl", "value": "0x037f", "variablesReference": 0},
                        ]),
                        _ => json!([{"name": "n", "value": "3", "variablesReference": 0}]),
                    };
                    send(&mut writer, answer(json!({"variables": vars})));
                }
                // "UH\x89\xe5" — push rbp; mov rbp, rsp — then 2 unreadable bytes.
                "readMemory" => send(
                    &mut writer,
                    answer(json!({"address": "0x100003f80", "data": "VUiJ5Q==",
                                  "unreadableBytes": 2})),
                ),
                "disassemble" => send(
                    &mut writer,
                    answer(json!({"instructions": [
                        {"address": "0x100003f80", "instructionBytes": "55",
                         "instruction": "pushq  %rbp", "symbol": "main",
                         "location": {"path": "/src/main.c"}, "line": 2},
                        {"address": "0x100003f81", "instructionBytes": "48 89 e5",
                         "instruction": "movq   %rsp, %rbp", "symbol": "main"},
                        {"address": "0x100003f84", "instructionBytes": "31 c0",
                         "instruction": "xorl   %eax, %eax", "symbol": "main", "line": 3},
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
    (address, log)
}

fn start(native: bool) -> (DebugSession, Log, i64) {
    let (address, log) = stub_adapter(native);
    let (mut session, _) = DebugSession::launch_on(
        DapClient::connect(&address, None).unwrap(),
        "lldb-dap",
        json!({}),
        &BTreeMap::new(),
        &SpecialBreakpoints::default(),
    )
    .unwrap();
    let thread = match session.wait(WAIT).unwrap() {
        Event::Stopped(s) => s.thread_id,
        Event::Exited(_) => panic!("the stub stops on entry"),
    };
    (session, log, thread)
}

fn requests(log: &Log, command: &str) -> Vec<Value> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|r| r["command"] == command)
        .map(|r| r["arguments"].clone())
        .collect()
}

#[test]
fn registers_memory_and_disassembly_of_a_stopped_native_session() {
    let (mut s, log, thread) = start(true);

    let groups = s.registers(thread).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name, "General Purpose Registers");
    assert_eq!(groups[0].registers[0].name, "rip");
    assert_eq!(groups[0].registers[0].value, "0x0000000100003f84");
    assert_eq!(groups[1].registers[0].name, "fctrl");

    let pc = s.instruction_pointer(thread).unwrap();
    assert_eq!(pc, 0x1_0000_3f84);

    let block = s.read_memory(0x1_0000_3f80, Some(1_000_000)).unwrap();
    assert!(block.capped);
    assert_eq!(block.requested, MAX_MEMORY_BYTES);
    assert_eq!((block.read, block.unreadable), (4, 2));
    assert_eq!(block.rows[0].address, "0x0000000100003f80");
    assert_eq!(block.rows[0].hex, "55 48 89 e5");
    assert_eq!(
        requests(&log, "readMemory")[0],
        json!({"memoryReference": "0x0000000100003f80", "offset": 0, "count": MAX_MEMORY_BYTES})
    );

    let code = s.disassemble(pc, 2, Some(10_000)).unwrap();
    assert_eq!(code.len(), 3);
    assert_eq!(code[2].address, "0x0000000100003f84");
    assert_eq!(code[2].line, Some(3));
    assert_eq!(code[0].path.as_deref(), Some("/src/main.c"));
    let asked = &requests(&log, "disassemble")[0];
    assert_eq!(asked["instructionOffset"], -2);
    assert_eq!(asked["instructionCount"], MAX_INSTRUCTIONS);
    assert_eq!(asked["memoryReference"], "0x0000000100003f84");
    assert_eq!(asked["resolveSymbols"], true);

    // Reading changed nothing: no request that writes, steps or resumes was sent.
    for command in [
        "writeMemory",
        "setVariable",
        "continue",
        "next",
        "stepIn",
        "evaluate",
    ] {
        assert!(requests(&log, command).is_empty(), "{command} was sent");
    }
}

#[test]
fn an_adapter_without_native_reads_is_refused_with_a_reason() {
    let (mut s, log, thread) = start(false);
    let memory = s.read_memory(0x1000, None).unwrap_err().to_string();
    assert!(memory.contains("cannot read memory"), "{memory}");
    let code = s.disassemble(0x1000, 0, None).unwrap_err().to_string();
    assert!(code.contains("cannot disassemble"), "{code}");
    let registers = s.registers(thread).unwrap_err().to_string();
    assert!(registers.contains("no registers"), "{registers}");
    assert!(requests(&log, "readMemory").is_empty());
    assert!(requests(&log, "disassemble").is_empty());
}
