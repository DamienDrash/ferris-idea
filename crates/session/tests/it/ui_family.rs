//! UISEC-8 of `SPEC-0319`: the session family `ui` is reserved (phase 4). A backend never serves
//! it, and the window refuses `ui.*` that a backend of a remote project sends: a host never
//! operates the window (ADR-0025 T5).

use std::time::{Duration, Instant};

use idecraft_session::frame::{Frame, read_frame};
use idecraft_session::{Backend, BackendConfig, ClientSession, ErrorCode, Message, in_process};
use serde_json::json;

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-uifam-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

/// A backend answers `ui.*` with `denied` and logs it; nothing can bind the family.
#[test]
fn uisec8_a_backend_never_serves_the_ui_family() {
    let dir = scratch("backend");
    let b = Backend::new(BackendConfig {
        state_dir: Some(dir.join("state")),
        peer: "test:window".into(),
        ..BackendConfig::default()
    });
    let (window, server) = in_process();
    let served = b.clone();
    std::thread::spawn(move || served.serve(server));
    let s = ClientSession::open(
        window,
        &dir.join("project").display().to_string(),
        "test",
        "testhost",
        None,
    )
    .unwrap();
    for method in ["ui.snapshot", "ui.act"] {
        let e = s.call(method, json!({})).unwrap_err();
        assert_eq!(e.code, ErrorCode::Denied, "{method}: {e}");
    }
    // Not a `ui` method: unknown as before.
    let e = s.call("uix.snapshot", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(!b.capabilities().iter().any(|c| c == "ui"));
    let audit = std::fs::read_to_string(dir.join("state/remote/audit.jsonl")).unwrap();
    assert!(
        audit.contains("ui.act") && audit.contains("denied"),
        "{audit}"
    );
    assert!(idecraft_session::backend::is_ui_method("ui.find"));
    assert!(!idecraft_session::backend::is_ui_method("uifind"));
}

/// The window answers `ui.snapshot` and `ui.act` from a backend with `denied` and notes it.
#[test]
fn uisec8_the_window_refuses_ui_requests_of_a_backend() {
    let (window, mut server) = in_process();
    let answers = std::thread::spawn(move || {
        let Ok(Some(f)) = read_frame(&mut server.reader) else {
            return Vec::new();
        };
        let Ok(Message::Request { id, .. }) = Message::parse(f.text().unwrap()) else {
            return Vec::new();
        };
        let hello = Message::Response {
            id,
            result: Ok(
                json!({"session": "ab", "client": "c1", "major": 1, "minor": idecraft_session::hello::MINOR, "token": "11".repeat(32)}),
            ),
        };
        let mut w = server.writer;
        Frame::message(hello.to_text()).write_to(&mut w).unwrap();
        for (n, method) in ["ui.snapshot", "ui.act"].iter().enumerate() {
            let req = Message::request(
                100 + n as u64,
                method,
                json!({"locator": "id=commit-button"}),
            );
            Frame::message(req.to_text()).write_to(&mut w).unwrap();
        }
        // The window's two answers.
        let mut got = Vec::new();
        while got.len() < 2 {
            let Ok(Some(f)) = read_frame(&mut server.reader) else {
                break;
            };
            if let Ok(Message::Response { id, result }) =
                Message::parse(f.text().unwrap_or_default())
            {
                got.push((id, result.err().map(|e| e.code)));
            }
        }
        got
    });
    let mut s = ClientSession::open(window, "/p", "t", "evil", None).unwrap();
    let t0 = Instant::now();
    while s.refused.len() < 2 && t0.elapsed() < Duration::from_secs(3) {
        s.pump(Duration::from_millis(10));
    }
    assert_eq!(s.refused, ["ui.snapshot", "ui.act"]);
    assert!(
        s.log
            .iter()
            .any(|l| l.contains("a host never operates the window"))
    );
    let got = answers.join().unwrap();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        got.iter().all(|(_, code)| *code == Some(ErrorCode::Denied)),
        "{got:?}"
    );
}
