//! SPEC-0317 AK-1, AK-2, AK-4 (in process) and the in-process parts of AK-5: the session protocol
//! end to end between a window half and a backend over the in-process transport and the reattach
//! socket.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use idecraft_session::frame::{Frame, MAX_FRAME, read_frame};
use idecraft_session::{
    Backend, BackendConfig, ClientSession, Connection, ErrorCode, Inbound, Message, Op,
    Reconnected, Recovery, Right, in_process,
};
use serde_json::json;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-sess-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn backend(dir: &Path) -> Backend {
    Backend::new(BackendConfig {
        folder: Some(dir.join("project")),
        state_dir: Some(dir.join("state")),
        peer: "test:window".into(),
        ..BackendConfig::default()
    })
}

fn connect(b: &Backend) -> idecraft_session::Transport {
    let (window, server) = in_process();
    let b = b.clone();
    std::thread::spawn(move || b.serve(server));
    window
}

fn open(b: &Backend, dir: &Path, recovery: Option<Recovery>) -> ClientSession {
    ClientSession::open(
        connect(b),
        &dir.join("project").display().to_string(),
        "test",
        "testhost",
        recovery,
    )
    .unwrap()
}

fn op(at: usize, delete: usize, insert: &str) -> Op {
    Op {
        at,
        delete,
        insert: insert.into(),
    }
}

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

fn raw_hello(
    conn: &Connection,
    project: &str,
    majors: serde_json::Value,
) -> Result<serde_json::Value, idecraft_session::RpcError> {
    conn.call(
        "session.hello",
        json!({"protocol": "idecraft-session", "majors": majors, "build": "t", "client": "window", "project": project}),
        Duration::from_secs(5),
    )
}

#[test]
fn hello_chooses_the_version_and_refuses_bad_folders() {
    let dir = scratch("hello");
    let b = backend(&dir);
    let project = dir.join("project").display().to_string();
    // No common major: refused with both versions.
    let (c, _rx) = Connection::start(connect(&b));
    let e = raw_hello(&c, &project, json!([7])).unwrap_err();
    assert_eq!(e.code, ErrorCode::UnsupportedVersion);
    assert!(
        e.message.contains("[7]") && e.message.contains("[1]"),
        "{e}"
    );
    // A folder that is missing or a file.
    let (c, _rx) = Connection::start(connect(&b));
    let e = raw_hello(&c, &dir.join("nope").display().to_string(), json!([1])).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    std::fs::write(dir.join("file"), "x").unwrap();
    let (c, _rx) = Connection::start(connect(&b));
    let e = raw_hello(&c, &dir.join("file").display().to_string(), json!([1])).unwrap_err();
    assert_eq!(e.code, ErrorCode::Invalid);
    // The real one: session id, token, rights and capabilities.
    let s = open(&b, &dir, None);
    assert_eq!(s.info.major, 1);
    assert_eq!(s.info.session.len(), 32);
    assert!(s.info.rights.contains(&"edit".to_owned()));
    assert!(s.info.capabilities.contains(&"doc.ops".to_owned()));
    assert!(!s.info.capabilities.contains(&"lsp".to_owned()));
    // A second workspace in the same backend is refused (one per process, M10.4 adds more).
    std::fs::create_dir_all(dir.join("other")).unwrap();
    let (c, _rx) = Connection::start(connect(&b));
    let e = raw_hello(&c, &dir.join("other").display().to_string(), json!([1])).unwrap_err();
    assert_eq!(e.code, ErrorCode::Invalid);
    // Unknown methods are "method not found"; families not bound yet too.
    for m in ["lsp.start", "nope.nope"] {
        let e = s.call(m, json!({})).unwrap_err();
        assert_eq!(e.number, -32601, "{m}");
    }
    // Audit: open and refusals, never the token.
    let audit = std::fs::read_to_string(dir.join("state/remote/audit.jsonl")).unwrap();
    assert!(audit.contains("\"method\":\"session.hello\""), "{audit}");
    assert!(audit.contains("unsupported_version"));
    assert!(!audit.contains(&s.resume_token().expose()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rights_cancellation_and_deadlines() {
    let dir = scratch("rights");
    std::fs::write(dir.join("project/a.txt"), "abc").unwrap();
    // A session with only `view` (paired rights arrive with M16; here the backend is told so).
    let b = Backend::new(BackendConfig {
        rights: vec![Right::View],
        state_dir: Some(dir.join("state")),
        ..BackendConfig::default()
    });
    let mut s = open(&b, &dir, None);
    s.open_doc("a.txt").unwrap();
    let e = s
        .call(
            "doc.edit",
            json!({"doc": "a.txt", "base": 1, "cseq": 1, "ops": []}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted);
    assert!(e.message.contains("`edit`"), "{e}");
    let e = s
        .call("fs.write", json!({"path": "a.txt", "text": "x"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted);
    assert_eq!(
        std::fs::read_to_string(dir.join("project/a.txt")).unwrap(),
        "abc"
    );
    let audit = std::fs::read_to_string(dir.join("state/remote/audit.jsonl")).unwrap();
    assert!(audit.contains("not_granted"), "{audit}");

    // A long handler of a later family: `$/cancel` and `deadline_ms`.
    let started = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&started);
    b.register(
        "test",
        "test.wait",
        Right::View,
        Arc::new(move |call: &idecraft_session::Call| {
            flag.store(true, Ordering::SeqCst);
            while call.cancel.stopped().is_none() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(json!("too late"))
        }),
    );
    let (c, _rx) = Connection::start(connect(&b));
    raw_hello(&c, &dir.join("project").display().to_string(), json!([1])).unwrap();
    let c2 = c.clone();
    let waiter =
        std::thread::spawn(move || c2.call("test.wait", json!({}), Duration::from_secs(10)));
    assert!(wait_until(Duration::from_secs(5), || started.load(Ordering::SeqCst)));
    // The request was the second one on this connection (after the hello).
    c.cancel(2).unwrap();
    assert_eq!(
        waiter.join().unwrap().unwrap_err().code,
        ErrorCode::Cancelled
    );
    let e = c
        .call(
            "test.wait",
            json!({"deadline_ms": 50}),
            Duration::from_secs(10),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::DeadlineExceeded);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn edits_of_two_windows_meet_and_large_texts_stream_with_credit() {
    let dir = scratch("two");
    std::fs::write(dir.join("project/a.txt"), "hello").unwrap();
    let big = "x".repeat(700 << 10);
    std::fs::write(dir.join("project/big.txt"), &big).unwrap();
    let registry = idecraft_editor::documents::DocumentRegistry::default();
    let b = Backend::new(BackendConfig {
        registry: registry.clone(),
        ..BackendConfig::default()
    });
    let mut w1 = open(&b, &dir, None);
    let mut w2 = open(&b, &dir, None);
    w1.open_doc("a.txt").unwrap();
    w2.open_doc("a.txt").unwrap();
    // Both type at the same place before either sees the other.
    w1.edit("a.txt", &[op(5, 0, " world")]).unwrap();
    w2.edit("a.txt", &[op(0, 0, ">> ")]).unwrap();
    w2.edit("a.txt", &[op(8, 0, "!")]).unwrap();
    assert_eq!(w2.doc("a.txt").unwrap().text(), ">> hello!");
    assert!(wait_until(Duration::from_secs(5), || {
        w1.pump(Duration::from_millis(10));
        w2.pump(Duration::from_millis(10));
        w1.pending_edits() == 0
            && w2.pending_edits() == 0
            && w1.doc("a.txt").unwrap().text() == w2.doc("a.txt").unwrap().text()
            && w1.doc("a.txt").unwrap().server_rev == 4
            && w2.doc("a.txt").unwrap().server_rev == 4
    }));
    // Both inserted right after `hello` at once; whichever the backend received first comes first
    // there. Both windows end with the same text and keep every edit.
    let text = w1.doc("a.txt").unwrap().text();
    assert!(
        text == ">> hello world!" || text == ">> hello! world",
        "{text:?}"
    );
    // Agents on the host read the same unsaved text from the registry.
    let doc = registry.get(&dir.join("project/a.txt")).unwrap();
    assert_eq!(doc.text.to_string(), text);
    assert!(doc.modified);
    w1.save("a.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("project/a.txt")).unwrap(),
        text
    );
    assert!(wait_until(Duration::from_secs(5), || {
        w2.pump(Duration::from_millis(10));
        !w2.doc("a.txt").unwrap().modified
    }));
    // A text above 512 KiB arrives through a stream with credit (256 KiB at first).
    let d = w1.open_doc("big.txt").unwrap();
    assert_eq!(d.text().len(), big.len());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn malformed_and_oversized_frames_close_and_replays_apply_once() {
    let dir = scratch("frames");
    std::fs::write(dir.join("project/a.txt"), "abc").unwrap();
    let b = backend(&dir);
    let (c, rx) = Connection::start(connect(&b));
    let hello = raw_hello(&c, &dir.join("project").display().to_string(), json!([1])).unwrap();
    c.call("doc.open", json!({"path": "a.txt"}), Duration::from_secs(5))
        .unwrap();
    let edit = json!({"doc": "a.txt", "base": 1, "cseq": 1, "ops": [{"at": 3, "insert": "d"}]});
    let first = c
        .call("doc.edit", edit.clone(), Duration::from_secs(5))
        .unwrap();
    // The same edit again (a replay): acknowledged, not applied twice.
    let again = c.call("doc.edit", edit, Duration::from_secs(5)).unwrap();
    assert_eq!(again["duplicate"], true);
    assert_eq!(again["revision"], first["revision"]);
    let snap = c
        .call(
            "doc.snapshot",
            json!({"doc": "a.txt"}),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(snap["text"], "abcd");
    // An oversized frame closes the connection; the session waits for reattachment.
    let mut huge = ((MAX_FRAME + 1) as u32).to_be_bytes().to_vec();
    huge.extend_from_slice(&[1, 0, 0, 0, 0]);
    c.send_raw(huge);
    let closed = loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Inbound::Closed(why)) => break why,
            Ok(_) => continue,
            Err(_) => panic!("not closed"),
        }
    };
    assert!(!closed.is_empty());
    let sid = hello["session"].as_str().unwrap();
    assert!(wait_until(Duration::from_secs(5), || b.connected(sid) == Some(false)));
    // A frame with an unknown type closes too.
    let (c, rx) = Connection::start(connect(&b));
    raw_hello(&c, &dir.join("project").display().to_string(), json!([1])).unwrap();
    c.send_raw(vec![0, 0, 0, 5, 9, 0, 0, 0, 0]);
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5)),
        Ok(Inbound::Closed(_))
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stalled_hellos_and_wrong_tokens_end_connections_and_sessions() {
    let dir = scratch("limits");
    assert_eq!(
        BackendConfig::default().hello_timeout,
        Duration::from_secs(10)
    );
    assert_eq!(BackendConfig::default().reattach, Duration::from_secs(600));
    let b = Backend::new(BackendConfig {
        hello_timeout: Duration::from_millis(300),
        ..BackendConfig::default()
    });
    // Nothing sent: closed after the hello time.
    let mut t = connect(&b);
    let t0 = Instant::now();
    let mut buf = [0u8; 16];
    assert_eq!(t.reader.read(&mut buf).unwrap(), 0);
    assert!(
        t0.elapsed() >= Duration::from_millis(250),
        "{:?}",
        t0.elapsed()
    );

    let mut s = open(&b, &dir, None);
    let sid = s.info.session.clone();
    s.drop_connection();
    assert!(wait_until(Duration::from_secs(5), || b.connected(&sid) == Some(false)));
    // Three wrong tokens end the waiting session.
    for i in 0..3 {
        let (c, _rx) = Connection::start(connect(&b));
        let e = c
            .call(
                "session.resume",
                json!({"session": sid, "token": "00".repeat(32), "last_seq": 0}),
                Duration::from_secs(5),
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Denied, "{i}");
    }
    assert_eq!(b.connected(&sid), None);
    let e = s.resume(connect(&b)).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_break_keeps_every_edit_and_resumes_with_replay_or_resync() {
    let dir = scratch("resume");
    std::fs::write(dir.join("project/a.txt"), "one\n").unwrap();
    let b = backend(&dir);
    let recovery = Recovery {
        dir: dir.join("window-state/recovery"),
        host: "testhost".into(),
        folder: dir.join("project").display().to_string(),
    };
    let mut w = open(&b, &dir, Some(recovery.clone()));
    let mut other = open(&b, &dir, None);
    w.open_doc("a.txt").unwrap();
    other.open_doc("a.txt").unwrap();
    w.edit("a.txt", &[op(4, 0, "two\n")]).unwrap();
    assert!(w.settle(Duration::from_secs(5)));
    // The connection breaks during typing; the window keeps editing.
    w.drop_connection();
    assert!(!w.connected());
    w.edit("a.txt", &[op(8, 0, "three\n")]).unwrap();
    w.edit("a.txt", &[op(0, 0, "# ")]).unwrap();
    assert_eq!(w.pending_edits(), 2);
    assert!(w.has_recovery("a.txt"));
    let e = w.save("a.txt", false).unwrap_err();
    assert!(
        e.message
            .starts_with("Not saved — reconnecting to testhost"),
        "{e}"
    );
    // Meanwhile another window edits; the replay brings it.
    other.edit("a.txt", &[op(0, 0, "zero\n")]).unwrap();
    assert!(other.settle(Duration::from_secs(5)));
    let r = w.resume(connect(&b)).unwrap();
    assert!(
        matches!(r, Reconnected::Resumed { resync: false, .. }),
        "{r:?}"
    );
    assert!(w.settle(Duration::from_secs(5)));
    let text = w.doc("a.txt").unwrap().text();
    assert_eq!(text, "zero\n# one\ntwo\nthree\n");
    // The save asked for during the break was carried out after the resume.
    assert_eq!(
        std::fs::read_to_string(dir.join("project/a.txt")).unwrap(),
        text
    );
    assert!(!w.has_recovery("a.txt"));
    assert!(
        w.log
            .iter()
            .any(|l| l.contains("saved a.txt after the resume")),
        "{:?}",
        w.log
    );
    assert!(wait_until(Duration::from_secs(5), || {
        other.pump(Duration::from_millis(10));
        other.doc("a.txt").unwrap().text() == text
    }));

    // Resuming after the ring buffer overflowed goes through a resync.
    let small = Backend::new(BackendConfig {
        ring: (2, 1 << 20),
        ..BackendConfig::default()
    });
    std::fs::write(dir.join("project/b.txt"), "b").unwrap();
    let mut w = open(&small, &dir, None);
    let mut other = open(&small, &dir, None);
    w.open_doc("b.txt").unwrap();
    other.open_doc("b.txt").unwrap();
    w.drop_connection();
    w.edit("b.txt", &[op(1, 0, "W")]).unwrap();
    for i in 0..5 {
        other.edit("b.txt", &[op(0, 0, &i.to_string())]).unwrap();
        assert!(other.settle(Duration::from_secs(5)));
    }
    let r = w.resume(connect(&small)).unwrap();
    assert!(
        matches!(r, Reconnected::Resumed { resync: true, .. }),
        "{r:?}"
    );
    assert!(w.tree_stale);
    let mine = w.doc("b.txt").unwrap();
    assert_eq!(mine.text(), "43210bW");
    assert!(mine.local_version.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_new_backend_reconciles_unchanged_and_changed_files() {
    let dir = scratch("gone");
    std::fs::write(dir.join("project/same.txt"), "same\n").unwrap();
    std::fs::write(dir.join("project/changed.txt"), "old\n").unwrap();
    let recovery = Recovery {
        dir: dir.join("window-state/recovery"),
        host: "testhost".into(),
        folder: dir.join("project").display().to_string(),
    };
    let b = Backend::new(BackendConfig {
        reattach: Duration::from_millis(200),
        ..BackendConfig::default()
    });
    let mut w = open(&b, &dir, Some(recovery.clone()));
    w.open_doc("same.txt").unwrap();
    w.open_doc("changed.txt").unwrap();
    w.drop_connection();
    w.edit("same.txt", &[op(5, 0, "mine\n")]).unwrap();
    w.edit("changed.txt", &[op(4, 0, "mine\n")]).unwrap();
    // The reattach period passes; the host file of one document changes meanwhile.
    assert!(b.wait_idle(Duration::from_secs(5)));
    std::fs::write(dir.join("project/changed.txt"), "theirs\n").unwrap();
    assert_eq!(w.resume(connect(&b)).unwrap_err().code, ErrorCode::NotFound);
    let fresh = backend(&dir);
    let r = w.replace_backend(connect(&fresh)).unwrap();
    assert_eq!(
        r,
        Reconnected::NewBackend {
            followed: vec!["same.txt".into()],
            kept: vec!["changed.txt".into()],
        }
    );
    assert_eq!(w.doc("same.txt").unwrap().text(), "same\nmine\n");
    let kept = w.doc("changed.txt").unwrap();
    assert_eq!(kept.text(), "theirs\n");
    assert_eq!(kept.local_version.as_deref(), Some("old\nmine\n"));
    assert!(kept.modified);
    assert!(
        w.log.iter().any(|l| l.contains("kept as local versions")),
        "{:?}",
        w.log
    );
    // Recovery copies exist for both until saved; they never hold the token.
    let copies = Recovery::list(&recovery.dir);
    assert_eq!(copies.len(), 2, "{copies:?}");
    for f in std::fs::read_dir(&recovery.dir).unwrap() {
        let text = std::fs::read_to_string(f.unwrap().path()).unwrap();
        assert!(!text.contains(&w.resume_token().expose()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(&recovery.dir).unwrap().mode() & 0o777,
            0o700
        );
        for f in std::fs::read_dir(&recovery.dir).unwrap() {
            assert_eq!(f.unwrap().metadata().unwrap().mode() & 0o777, 0o600);
        }
    }
    w.save("same.txt", false).unwrap();
    assert_eq!(Recovery::list(&recovery.dir).len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_tree_and_the_disk_reach_the_window() {
    let dir = scratch("watch");
    std::fs::write(dir.join("project/a.txt"), "a\n").unwrap();
    std::fs::write(dir.join("project/b.txt"), "b\n").unwrap();
    let b = backend(&dir);
    let mut w = open(&b, &dir, None);
    let listed = w.list(".").unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(w.list("../").unwrap_err().code, ErrorCode::Invalid);
    w.watch(&["build"]).unwrap();
    w.open_doc("a.txt").unwrap();
    w.open_doc("b.txt").unwrap();
    // A host-side create appears within 1 s; excluded folders stay out.
    let t0 = Instant::now();
    std::fs::create_dir_all(dir.join("project/build")).unwrap();
    std::fs::write(dir.join("project/build/out.o"), "o").unwrap();
    std::fs::write(dir.join("project/new.rs"), "fn x() {}").unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        w.pump(Duration::from_millis(10));
        w.tree_events
            .iter()
            .any(|e| e["path"] == "new.rs" && e["kind"] == "created")
    }));
    assert!(
        t0.elapsed() < Duration::from_millis(1500),
        "{:?}",
        t0.elapsed()
    );
    assert!(
        !w.tree_events
            .iter()
            .any(|e| e["path"].as_str().unwrap().starts_with("build"))
    );
    // Unmodified document changed on the host: reloaded silently.
    std::fs::write(dir.join("project/a.txt"), "a changed\n").unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        w.pump(Duration::from_millis(10));
        w.doc("a.txt").unwrap().text() == "a changed\n"
    }));
    assert!(!w.doc("a.txt").unwrap().modified);
    // Modified document changed on the host: the window is told, saving refuses with conflict.
    w.edit("b.txt", &[op(0, 0, "mine ")]).unwrap();
    assert!(w.settle(Duration::from_secs(5)));
    std::fs::write(dir.join("project/b.txt"), "theirs\n").unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        w.pump(Duration::from_millis(10));
        w.doc("b.txt").unwrap().disk_changed
    }));
    assert_eq!(
        w.save("b.txt", false).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("project/b.txt")).unwrap(),
        "theirs\n"
    );
    w.save("b.txt", true).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("project/b.txt")).unwrap(),
        "mine b\n"
    );
    // Paths that leave the project are refused by the backend.
    for bad in ["../outside.txt", "/etc/hosts"] {
        let e = w
            .call("fs.write", json!({"path": bad, "text": "x"}))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid, "{bad}");
        assert_eq!(w.open_doc(bad).unwrap_err().code, ErrorCode::Invalid);
    }
    assert!(!dir.join("outside.txt").exists());
    let _ = std::fs::remove_dir_all(dir);
}

/// A fake backend from before the revision check, as a stale file on a host: the window refuses it
/// when connecting, with what to do, and ends the session it had just opened.
#[test]
fn the_window_refuses_a_backend_older_than_itself() {
    let (window, mut server) = in_process();
    let backend = std::thread::spawn(move || {
        let Ok(Some(f)) = read_frame(&mut server.reader) else {
            return false;
        };
        let Ok(Message::Request { id, .. }) = Message::parse(f.text().unwrap()) else {
            return false;
        };
        let answer = Message::Response {
            id,
            result: Ok(json!({
                "session": "ab", "client": "c1", "major": 1, "build": "idecraft 0.11.0",
                "token": "11".repeat(32),
            })),
        };
        let mut w = server.writer;
        Frame::message(answer.to_text()).write_to(&mut w).unwrap();
        let Ok(Some(f)) = read_frame(&mut server.reader) else {
            return false;
        };
        let Ok(Message::Request { id, method, .. }) = Message::parse(f.text().unwrap()) else {
            return false;
        };
        let answer = Message::Response {
            id,
            result: Ok(json!({"unsaved": []})),
        };
        Frame::message(answer.to_text()).write_to(&mut w).unwrap();
        method == "session.bye"
    });
    let Err(e) = ClientSession::open(window, "/p", "t", "stale", None) else {
        panic!("a backend without a revision was accepted");
    };
    assert_eq!(e.code, ErrorCode::UnsupportedVersion);
    assert!(e.message.contains("idecraft 0.11.0"), "{e}");
    assert!(e.message.contains("build-backends.sh"), "{e}");
    assert!(backend.join().unwrap(), "the window ended the session");
}

/// A fake backend that answers the hello and then sends a path leaving the project and an
/// oversized frame: the window refuses the path and closes the connection.
#[test]
fn the_window_refuses_what_a_hostile_backend_sends() {
    let (window, mut server) = in_process();
    std::thread::spawn(move || {
        let Ok(Some(f)) = read_frame(&mut server.reader) else {
            return;
        };
        let Ok(Message::Request { id, .. }) = Message::parse(f.text().unwrap()) else {
            return;
        };
        let answer = Message::Response {
            id,
            result: Ok(json!({
                "session": "ab", "client": "c1", "major": 1,
                "minor": idecraft_session::hello::MINOR, "token": "11".repeat(32),
            })),
        };
        let mut w = server.writer;
        Frame::message(answer.to_text()).write_to(&mut w).unwrap();
        let bad = Message::notification(
            "fs.changed",
            json!({"seq": 1, "events": [{"kind": "created", "path": "../../.ssh/authorized_keys"},
                                         {"kind": "created", "path": "/etc/passwd"},
                                         {"kind": "created", "path": "ok.txt"}]}),
        );
        Frame::message(bad.to_text()).write_to(&mut w).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let mut huge = ((MAX_FRAME + 1) as u32).to_be_bytes().to_vec();
        huge.extend_from_slice(&[2, 0, 0, 0, 1]);
        let _ = w.write_all(&huge);
        std::thread::sleep(Duration::from_millis(500));
    });
    let mut s = ClientSession::open(window, "/p", "t", "evil", None).unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        s.pump(Duration::from_millis(10));
        !s.connected()
    }));
    assert_eq!(s.tree_events.len(), 1);
    assert_eq!(s.tree_events[0]["path"], "ok.txt");
    assert_eq!(s.refused.len(), 2, "{:?}", s.refused);
}

#[cfg(unix)]
#[test]
fn the_reattach_socket_is_private_and_speaks_only_resume() {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::net::UnixStream;
    let dir = scratch("sock");
    // Unix socket paths are short; the run folder lives below /tmp for this test.
    let run = PathBuf::from(format!("/tmp/idc-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&run);
    std::fs::write(dir.join("project/a.txt"), "a").unwrap();
    let b = Backend::new(BackendConfig {
        run_dir: Some(run.clone()),
        ..BackendConfig::default()
    });
    let mut w = open(&b, &dir, None);
    let sock = b.socket_of(&w.info.session).unwrap();
    assert_eq!(std::fs::metadata(&run).unwrap().mode() & 0o777, 0o700);
    // Plain JSON and an HTTP request: closed without any content in reply.
    for probe in [
        &b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"session.resume\"}\n"[..],
        &b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n"[..],
        &b"SSH-2.0-OpenSSH_9.6\r\n"[..],
    ] {
        let mut s = UnixStream::connect(&sock).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(probe).unwrap();
        let mut got = Vec::new();
        let _ = s.read_to_end(&mut got);
        assert!(got.is_empty(), "{:?}", String::from_utf8_lossy(&got));
    }
    // A framed hello is not accepted on the reattach socket either.
    let mut s = UnixStream::connect(&sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    Frame::message(Message::request(1, "session.hello", json!({})).to_text())
        .write_to(&mut s)
        .unwrap();
    let mut got = Vec::new();
    let _ = s.read_to_end(&mut got);
    assert!(got.is_empty());
    // The real resume works through the socket.
    w.open_doc("a.txt").unwrap();
    w.drop_connection();
    w.edit("a.txt", &[op(1, 0, "b")]).unwrap();
    let stream = UnixStream::connect(&sock).unwrap();
    let reader = stream.try_clone().unwrap();
    let closing = stream.try_clone().unwrap();
    let t = idecraft_session::Transport {
        reader: Box::new(reader),
        writer: Box::new(stream),
        closer: Arc::new(move || {
            let _ = closing.shutdown(std::net::Shutdown::Both);
        }),
    };
    assert!(matches!(w.resume(t).unwrap(), Reconnected::Resumed { .. }));
    assert!(w.settle(Duration::from_secs(5)));
    w.save("a.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("project/a.txt")).unwrap(),
        "ab"
    );
    // Ending the session removes its socket.
    assert!(w.bye().unwrap().is_empty());
    assert!(wait_until(Duration::from_secs(3), || !sock.exists()));
    let _ = std::fs::remove_dir_all(run);
    let _ = std::fs::remove_dir_all(dir);
}

/// SPEC-0317 AK-2 and AK-7: rounds of breaks, typing during them, an agent's edits and changes on
/// the host, for two windows and an agent: everyone ends with the same text, which is the host's
/// after a save, and no edit is lost.
#[test]
fn breaks_host_changes_and_concurrent_writers_converge() {
    let dir = scratch("stress");
    let file = dir.join("project/s.txt");
    std::fs::write(&file, "start\n").unwrap();
    let b = backend(&dir);
    let mut w = open(&b, &dir, None);
    let mut other = open(&b, &dir, None);
    let mut agent = open(&b, &dir, None);
    for c in [&mut w, &mut other, &mut agent] {
        c.open_doc("s.txt").unwrap();
    }
    let limit = Duration::from_secs(10);
    for round in 0..60 {
        w.drop_connection();
        if round % 3 == 0 {
            other.drop_connection();
        }
        // The agent works while the windows are away, and saves.
        agent.edit("s.txt", &[op(0, 0, "a")]).unwrap();
        assert!(agent.settle(limit), "round {round}");
        agent.save("s.txt", false).unwrap();
        // The host changes the file behind everyone's back.
        let line = format!("host {round}\n");
        std::fs::write(&file, &line).unwrap();
        assert!(
            wait_until(limit, || {
                agent.pump(Duration::from_millis(2));
                agent.doc("s.txt").unwrap().text() == line
            }),
            "round {round}: the backend did not follow the host"
        );
        // The window types during the break on the text it still has.
        if round % 2 == 1 {
            w.edit("s.txt", &[op(0, 0, "w")]).unwrap();
        }
        let r = w.resume(connect(&b)).unwrap();
        assert!(
            matches!(r, Reconnected::Resumed { resync: false, .. }),
            "{r:?}"
        );
        if !other.connected() {
            other.resume(connect(&b)).unwrap();
        }
        assert!(w.settle(limit), "round {round}");
        // The host's text and the window's typing both survive, the same for everyone.
        let typed = round % 2 == 1;
        let complete = |t: &str| t.contains(line.trim_end()) && t.contains('w') == typed;
        assert!(
            wait_until(limit, || {
                w.pump(Duration::from_millis(2));
                other.pump(Duration::from_millis(2));
                agent.pump(Duration::from_millis(2));
                let text = w.doc("s.txt").unwrap().text();
                complete(&text)
                    && other.doc("s.txt").unwrap().text() == text
                    && agent.doc("s.txt").unwrap().text() == text
            }),
            "round {round}: {:?} / {:?} / {:?}",
            w.doc("s.txt").unwrap().text(),
            other.doc("s.txt").unwrap().text(),
            agent.doc("s.txt").unwrap().text(),
        );
        if typed {
            w.save("s.txt", false).unwrap();
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                w.doc("s.txt").unwrap().text()
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// `SPEC-0321` behaviour 18: a session of a paired device has that device's rights and only that
/// device can resume it, even with the right token.
#[test]
fn a_session_of_a_device_cannot_be_taken_over_by_another_device() {
    use idecraft_session::{Peer, Right, in_process};
    let dir = scratch("takeover");
    let b = backend(&dir);
    let peer = |name: &str| Peer {
        name: name.to_owned(),
        rights: vec![Right::View],
    };
    let (window, server) = in_process();
    let (me, p) = (b.clone(), peer("device:aaaa:one"));
    std::thread::spawn(move || me.serve_as(server, p));
    let (c, _rx) = Connection::start(window);
    let project = dir.join("project").display().to_string();
    let hello = raw_hello(&c, &project, json!([1])).unwrap();
    assert_eq!(hello["rights"], json!(["view"]));
    let (session, token) = (hello["session"].clone(), hello["token"].clone());
    c.close();

    let resume = |who: &str| {
        let (window, server) = in_process();
        let (me, p) = (b.clone(), peer(who));
        std::thread::spawn(move || me.serve_as(server, p));
        let (c, _rx) = Connection::start(window);
        c.call(
            "session.resume",
            json!({"session": session, "token": token, "last_seq": 0}),
            Duration::from_secs(5),
        )
    };
    let e = resume("device:bbbb:two").unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied);
    assert!(e.message.contains("another device"), "{e}");
    assert!(resume("device:aaaa:one").is_ok());
}
