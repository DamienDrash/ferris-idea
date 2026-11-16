//! SPEC-0323: the project room end to end over real sessions in the process (every client a real
//! `ClientSession` on the session protocol; the store, the journal and the disk are real).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use idecraft_session::room::{self, RoomConfig, Rooms};
use idecraft_session::{
    Backend, BackendConfig, ClientSession, ErrorCode, Op, Peer, Recovery, Right, in_process,
};
use serde_json::{Value, json};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-room-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

/// The clock of a test: real time plus what the test adds.
fn clock() -> (room::bind::Clock, Arc<AtomicU64>) {
    let skew = Arc::new(AtomicU64::new(0));
    let s = Arc::clone(&skew);
    let start = Instant::now();
    (
        Arc::new(move || 1_000_000 + start.elapsed().as_millis() as u64 + s.load(Ordering::SeqCst)),
        skew,
    )
}

/// A clock that stands still until the test moves it, for limits counted per second.
fn still_clock() -> (room::bind::Clock, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(1_000_000));
    let n = Arc::clone(&now);
    (Arc::new(move || n.load(Ordering::SeqCst)), now)
}

fn make_backend(dir: &Path, clk: room::bind::Clock) -> (Backend, Arc<Rooms>) {
    make_backend_with(dir, clk, true)
}

fn make_backend_with(dir: &Path, clk: room::bind::Clock, autosave: bool) -> (Backend, Arc<Rooms>) {
    let b = Backend::new(BackendConfig {
        folder: Some(dir.join("project")),
        state_dir: Some(dir.join("state")),
        peer: "ssh:owner@10.0.0.9".into(),
        watch_interval: Duration::from_millis(30),
        ..BackendConfig::default()
    });
    let cfg = RoomConfig {
        now: clk,
        tick: Duration::from_millis(20),
        autosave,
        ..RoomConfig::default()
    };
    let rooms = room::bind(&b, cfg);
    (b, rooms)
}

fn folder(dir: &Path) -> String {
    dir.join("project").display().to_string()
}

fn owner(b: &Backend, dir: &Path) -> ClientSession {
    let (window, server) = in_process();
    let bb = b.clone();
    std::thread::spawn(move || bb.serve(server));
    ClientSession::open(window, &folder(dir), "t", "h", None).unwrap()
}

fn device(b: &Backend, dir: &Path, name: &str, rights: &[Right]) -> ClientSession {
    let (window, server) = in_process();
    let bb = b.clone();
    let peer = Peer {
        name: format!("device:{name}:{name}"),
        rights: rights.to_vec(),
    };
    std::thread::spawn(move || bb.serve_as(server, peer));
    ClientSession::open(window, &folder(dir), "t", "h", None).unwrap()
}

fn op(at: usize, delete: usize, insert: &str) -> Op {
    Op {
        at,
        delete,
        insert: insert.into(),
    }
}

fn wait(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    f()
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join("project").join(name), text).unwrap();
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join("project").join(name)).unwrap()
}

/// A room with an owner session, and a link of `role`.
fn room_with_link(b: &Backend, dir: &Path, role: &str) -> (ClientSession, String) {
    let own = owner(b, dir);
    own.call("room.create", json!({"name": "demo"})).unwrap();
    let link = own.call("room.share", json!({"role": role})).unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned();
    (own, link)
}

fn join(c: &ClientSession, link: &str) -> Value {
    c.call("room.join", json!({"link": link})).unwrap()
}

fn editor_rights() -> Vec<Right> {
    vec![Right::View, Right::Edit, Right::VcsWrite]
}

fn settle(sessions: &mut [&mut ClientSession]) {
    for _ in 0..6 {
        for s in sessions.iter_mut() {
            s.pump(Duration::from_millis(15));
            s.settle(Duration::from_secs(5));
        }
    }
}

fn entries(c: &ClientSession, since: u64) -> Vec<Value> {
    c.call("room.since", json!({"seq": since})).unwrap()["entries"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn clients_an_agent_and_changes_from_outside_converge_on_one_order() {
    let dir = scratch("converge");
    write(&dir, "a.txt", "0123456789\n");
    let (clk, _) = clock();
    let (b, rooms) = make_backend(&dir, clk);
    let (mut own, link) = room_with_link(&b, &dir, "editor");
    let mut bob = device(&b, &dir, "bob", &editor_rights());
    join(&bob, &link);
    let mut agent = owner(&b, &dir);
    let joined = agent.call("room.join", json!({"agent": "Claude"})).unwrap();
    assert_eq!(joined["role"], "editor");
    for s in [&mut own, &mut bob, &mut agent] {
        s.open_doc("a.txt").unwrap();
    }
    for round in 0..6 {
        own.edit("a.txt", &[op(0, 0, "A")]).unwrap();
        bob.edit("a.txt", &[op(2, 1, "BB")]).unwrap();
        agent.edit("a.txt", &[op(4 + round, 0, "g")]).unwrap();
        if round == 2 {
            // A refactoring from outside the typing, as one change set.
            agent
                .call(
                    "room.apply",
                    json!({"changes": [
                        {"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "X"}]},
                        {"kind": "create", "path": "new.txt", "text": "made\n"},
                    ]}),
                )
                .unwrap();
        }
    }
    settle(&mut [&mut own, &mut bob, &mut agent]);
    // The program outside reads the file and writes it back with a line more. It starts after the
    // automatic save: one that came between its read and its write would be replaced by the
    // older text it read, which is the outside program's choice, not a merge.
    assert!(
        wait(Duration::from_secs(10), || b
            .document("a.txt")
            .is_some_and(|d| d.text == read(&dir, "a.txt"))),
        "the edits are saved"
    );
    let on_disk = read(&dir, "a.txt");
    write(&dir, "a.txt", &format!("{on_disk}outside\n"));
    assert!(
        wait(Duration::from_secs(10), || b
            .document("a.txt")
            .is_some_and(|d| d.text.contains("outside"))),
        "the change from outside arrives"
    );
    own.edit("a.txt", &[op(0, 0, "Z")]).unwrap();
    settle(&mut [&mut own, &mut bob, &mut agent]);
    assert!(wait(Duration::from_secs(10), || {
        settle(&mut [&mut own, &mut bob, &mut agent]);
        let server = b.document("a.txt").unwrap().text;
        own.doc("a.txt").unwrap().text() == server
            && bob.doc("a.txt").unwrap().text() == server
            && agent.doc("a.txt").unwrap().text() == server
    }));
    let text = b.document("a.txt").unwrap().text;
    assert!(text.contains("outside") && text.contains('X') && text.contains('Z'));
    assert_eq!(read(&dir, "new.txt"), "made\n");
    // One order: rising sequence numbers, every author named, the agent under its own name.
    rooms.flush(&b);
    let log = entries(&own, 0);
    let seqs: Vec<u64> = log.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    let authors: Vec<&str> = log.iter().map(|e| e["actor"].as_str().unwrap()).collect();
    assert!(authors.contains(&"ssh:owner"));
    assert!(authors.contains(&"device:bob"));
    assert!(authors.contains(&"disk"));
    assert!(authors.iter().any(|a| a.starts_with("agent:Claude>")));
    let agent_entry = log
        .iter()
        .find(|e| e["actor"].as_str().unwrap().starts_with("agent:"))
        .unwrap();
    assert!(
        agent_entry["author"]
            .as_str()
            .unwrap()
            .starts_with("Claude (")
    );
    assert!(
        log.iter()
            .any(|e| e["kind"] == "version" && e["name"].as_str().unwrap().contains("Claude"))
    );
}

#[test]
fn a_change_set_is_all_or_nothing_and_one_undo_step() {
    let dir = scratch("set");
    write(&dir, "a.txt", "aaaa\n");
    write(&dir, "b.txt", "bbbb\n");
    let (clk, _) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let own = owner(&b, &dir);
    own.call("room.create", json!({})).unwrap();
    // One invalid member: nothing happens.
    let e = own
        .call(
            "room.apply",
            json!({"changes": [
                {"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "X"}]},
                {"kind": "create", "path": "c.txt"},
                {"kind": "edit", "path": "b.txt", "ops": [{"at": 99, "insert": "X"}]},
            ]}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Invalid);
    assert_eq!(read(&dir, "a.txt"), "aaaa\n");
    assert!(!dir.join("project/c.txt").exists());
    assert_eq!(b.document("a.txt").unwrap().text, "aaaa\n");
    assert_eq!(entries(&own, 0).len(), 0);
    // A valid one is one group.
    let done = own
        .call(
            "room.apply",
            json!({"changes": [
                {"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "X"}]},
                {"kind": "edit", "path": "b.txt", "ops": [{"at": 0, "insert": "Y"}]},
                {"kind": "rename", "from": "b.txt", "to": "sub/b2.txt"},
                {"kind": "create", "path": "c.txt", "text": "c"},
            ]}),
        )
        .unwrap();
    assert_eq!(done["documents"]["a.txt"]["revision"], 2);
    let log = entries(&own, 0);
    assert_eq!(log.len(), 4);
    assert!(log.iter().all(|e| e["group"] == log[0]["group"]));
    // The document moved with the file; a late edit under the old name still arrives.
    assert_eq!(b.document("sub/b2.txt").unwrap().text, "Ybbbb\n");
    own.undo_all(&b);
    assert_eq!(read(&dir, "a.txt"), "aaaa\n");
    assert!(dir.join("project/b.txt").exists());
    assert!(!dir.join("project/c.txt").exists());
    assert!(!dir.join("project/sub/b2.txt").exists());
    assert_eq!(b.document("b.txt").unwrap().text, "bbbb\n");
}

trait UndoAll {
    fn undo_all(&self, b: &Backend);
}

impl UndoAll for ClientSession {
    fn undo_all(&self, b: &Backend) {
        self.call("room.undo", json!({})).unwrap();
        b.autosave_room();
    }
}

#[test]
fn undo_takes_back_only_ones_own_work() {
    let dir = scratch("undo");
    write(&dir, "a.txt", "hello\n");
    let (clk, skew) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let (mut own, link) = room_with_link(&b, &dir, "editor");
    let mut bob = device(&b, &dir, "bob", &editor_rights());
    join(&bob, &link);
    own.open_doc("a.txt").unwrap();
    bob.open_doc("a.txt").unwrap();
    own.edit("a.txt", &[op(5, 0, " mine")]).unwrap();
    settle(&mut [&mut own, &mut bob]);
    skew.fetch_add(5000, Ordering::SeqCst);
    bob.edit("a.txt", &[op(0, 0, "bob ")]).unwrap();
    settle(&mut [&mut own, &mut bob]);
    assert_eq!(b.document("a.txt").unwrap().text, "bob hello mine\n");
    // The owner undoes: bob's words stay.
    own.call("room.undo", json!({})).unwrap();
    settle(&mut [&mut own, &mut bob]);
    assert_eq!(b.document("a.txt").unwrap().text, "bob hello\n");
    assert_eq!(own.doc("a.txt").unwrap().text(), "bob hello\n");
    // Redo brings it back; then undo again.
    own.call("room.redo", json!({})).unwrap();
    settle(&mut [&mut own, &mut bob]);
    assert_eq!(b.document("a.txt").unwrap().text, "bob hello mine\n");
    // Bob undoes his own while the owner's stays.
    bob.call("room.undo", json!({})).unwrap();
    settle(&mut [&mut own, &mut bob]);
    assert_eq!(b.document("a.txt").unwrap().text, "hello mine\n");
    // A new edit clears redo.
    bob.edit("a.txt", &[op(0, 0, "q")]).unwrap();
    settle(&mut [&mut own, &mut bob]);
    let r = bob.call("room.redo", json!({})).unwrap();
    assert_eq!(r["changes"], 0);
    // File operations.
    own.call(
        "room.apply",
        json!({"changes": [{"kind": "delete", "path": "a.txt"}]}),
    )
    .unwrap();
    assert!(!dir.join("project/a.txt").exists());
    own.call("room.undo", json!({})).unwrap();
    assert_eq!(read(&dir, "a.txt"), "qhello mine\n");
}

#[test]
fn a_change_on_disk_merges_without_a_dialog() {
    let dir = scratch("disk");
    write(&dir, "a.txt", "one\ntwo\nthree\n");
    let (clk, _) = clock();
    let (b, _rooms) = make_backend_with(&dir, clk, false);
    let (mut own, _) = room_with_link(&b, &dir, "viewer");
    own.open_doc("a.txt").unwrap();
    // Automatic saving is off here so that the edit is really unsaved when the file changes.
    own.edit("a.txt", &[op(0, 0, "START ")]).unwrap();
    settle(&mut [&mut own]);
    write(&dir, "a.txt", "one\ntwo\nthree\nfour\n");
    assert!(wait(Duration::from_secs(10), || {
        own.pump(Duration::from_millis(10));
        b.document("a.txt")
            .is_some_and(|d| d.text.contains("START") && d.text.contains("four"))
    }));
    assert!(wait(Duration::from_secs(10), || {
        let t = read(&dir, "a.txt");
        t.contains("START") && t.contains("four")
    }));
    assert!(
        !own.doc("a.txt").unwrap().disk_changed,
        "no dialog in a room"
    );
}

#[test]
fn without_a_room_the_old_behaviour_stays() {
    let dir = scratch("noroom");
    write(&dir, "a.txt", "x\n");
    let (clk, _) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let mut own = owner(&b, &dir);
    own.open_doc("a.txt").unwrap();
    own.edit("a.txt", &[op(0, 0, "Y")]).unwrap();
    settle(&mut [&mut own]);
    write(&dir, "a.txt", "changed\n");
    assert!(wait(Duration::from_secs(5), || {
        own.pump(Duration::from_millis(10));
        own.doc("a.txt").unwrap().disk_changed
    }));
}

#[test]
fn a_lost_connection_merges_silently_and_is_shown_as_away() {
    let dir = scratch("resync");
    write(&dir, "a.txt", "base\n");
    let (clk, skew) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let (mut own, link) = room_with_link(&b, &dir, "editor");
    let rec_dir = dir.join("rec");
    std::fs::create_dir_all(&rec_dir).unwrap();
    let (window, server) = in_process();
    let (bb, peer) = (
        b.clone(),
        Peer {
            name: "device:bob:bob".into(),
            rights: editor_rights(),
        },
    );
    std::thread::spawn(move || bb.serve_as(server, peer));
    let recovery = Recovery {
        dir: rec_dir,
        host: "h".into(),
        folder: folder(&dir),
    };
    let mut bob = ClientSession::open(window, &folder(&dir), "t", "h", Some(recovery)).unwrap();
    join(&bob, &link);
    own.open_doc("a.txt").unwrap();
    bob.open_doc("a.txt").unwrap();
    let seen = own.call("room.status", json!({})).unwrap()["seq"]
        .as_u64()
        .unwrap();
    bob.drop_connection();
    bob.edit("a.txt", &[op(0, 0, "offline ")]).unwrap();
    // Offline time: real for AK-6 when asked for (IDECRAFT_ROOM_OFFLINE_SECONDS=61), short here.
    let secs = std::env::var("IDECRAFT_ROOM_OFFLINE_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1u64);
    own.edit("a.txt", &[op(4, 0, " typed")]).unwrap();
    settle(&mut [&mut own]);
    assert!(wait(Duration::from_secs(5), || {
        let s = own.call("room.status", json!({})).unwrap();
        s["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "device:bob" && p["away"] == true)
    }));
    // The other participant stays connected the whole time, as a window would.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs.min(70)) {
        let _ = own.ping();
        own.pump(Duration::from_millis(200));
    }
    let (window, server) = in_process();
    let (bb, peer) = (
        b.clone(),
        Peer {
            name: "device:bob:bob".into(),
            rights: editor_rights(),
        },
    );
    std::thread::spawn(move || bb.serve_as(server, peer));
    bob.resume(window).unwrap();
    assert!(wait(Duration::from_secs(10), || {
        settle(&mut [&mut own, &mut bob]);
        let server = b.document("a.txt").unwrap().text;
        bob.doc("a.txt").unwrap().text() == server && own.doc("a.txt").unwrap().text() == server
    }));
    let text = b.document("a.txt").unwrap().text;
    assert_eq!(text, "offline base typed\n");
    assert!(!bob.doc("a.txt").unwrap().disk_changed);
    // Catching up by sequence number.
    let missed = entries(&bob, seen);
    assert!(missed.iter().any(|e| e["actor"] == "ssh:owner"));
    assert!(missed.iter().any(|e| e["actor"] == "device:bob"));
    // Past the away limit, the participant is gone from the list, without losing its session.
    bob.drop_connection();
    assert!(wait(Duration::from_secs(5), || {
        let s = own.call("room.status", json!({})).unwrap();
        s["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "device:bob" && p["away"] == true)
    }));
    skew.fetch_add(61_000, Ordering::SeqCst);
    assert!(wait(Duration::from_secs(5), || {
        let s = own.call("room.status", json!({})).unwrap();
        !s["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "device:bob")
    }));
}

#[test]
fn roles_limit_what_a_session_can_do() {
    let dir = scratch("roles");
    write(&dir, "a.txt", "text\n");
    let (clk, _) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    b.register(
        "run",
        "run.start",
        Right::Run,
        Arc::new(|_| Ok(json!({"ran": true}))),
    );
    let (own, viewer_link) = room_with_link(&b, &dir, "viewer");
    let commenter_link = own
        .call("room.share", json!({"role": "commenter"}))
        .unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned();
    // Viewer.
    let viewer = device(&b, &dir, "vera", &Right::ALL);
    let j = join(&viewer, &viewer_link);
    assert_eq!(j["role"], "viewer");
    let code = |r: Result<Value, idecraft_session::RpcError>| r.unwrap_err().code;
    assert_eq!(
        code(
            viewer
                .call("doc.open", json!({"path": "a.txt"}))
                .and_then(|d| {
                    viewer.call(
                        "doc.edit",
                        json!({"doc": d["doc"], "base": d["revision"], "cseq": 1, "ops": []}),
                    )
                })
        ),
        ErrorCode::NotGranted
    );
    assert_eq!(
        code(viewer.call("fs.write", json!({"path": "a.txt", "text": "x"}))),
        ErrorCode::NotGranted
    );
    assert_eq!(
        code(viewer.call("run.start", json!({}))),
        ErrorCode::NotGranted
    );
    assert_eq!(
        code(viewer.call(
            "room.apply",
            json!({"changes": [{"kind": "delete", "path": "a.txt"}]})
        )),
        ErrorCode::NotGranted
    );
    assert_eq!(
        code(viewer.call("room.comment", json!({"doc": "a.txt", "text": "hi"}))),
        ErrorCode::NotGranted
    );
    assert_eq!(
        code(viewer.call("room.share", json!({}))),
        ErrorCode::NotGranted
    );
    // A commenter comments, and a comment moves with the text.
    let carl = device(&b, &dir, "carl", &Right::ALL);
    assert_eq!(join(&carl, &commenter_link)["role"], "commenter");
    let c = carl
        .call(
            "room.comment",
            json!({"doc": "a.txt", "at": 3, "text": "look"}),
        )
        .unwrap();
    own.call(
        "room.apply",
        json!({"changes": [{"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "ab"}]}]}),
    )
    .unwrap();
    let list = carl.call("room.comments", json!({"doc": "a.txt"})).unwrap();
    assert_eq!(list["comments"][0]["at"], 5);
    assert_eq!(
        code(carl.call("fs.write", json!({"path": "a.txt", "text": "x"}))),
        ErrorCode::NotGranted
    );
    carl.call("room.resolve_comment", json!({"id": c["id"]}))
        .unwrap();
    // A link never widens what the channel allows.
    let editor_link = own.call("room.share", json!({"role": "editor"})).unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned();
    let weak = device(&b, &dir, "weak", &[Right::View]);
    assert_eq!(join(&weak, &editor_link)["role"], "commenter");
    assert_eq!(
        code(weak.call("fs.write", json!({"path": "a.txt", "text": "x"}))),
        ErrorCode::NotGranted
    );
}

#[test]
fn only_members_act_and_removal_is_final_and_quick() {
    let dir = scratch("gate");
    write(&dir, "a.txt", "text\n");
    let (clk, _) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let (own, link) = room_with_link(&b, &dir, "editor");
    let bob = device(&b, &dir, "bob", &editor_rights());
    // Not a member yet: everything but joining is refused.
    let e = bob.call("fs.list", json!({"path": "."})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted);
    assert!(e.message.contains("room"), "{e}");
    join(&bob, &link);
    assert!(bob.call("fs.list", json!({"path": "."})).is_ok());
    // A new session of a member needs no link.
    let again = device(&b, &dir, "bob", &editor_rights());
    assert!(again.call("fs.list", json!({"path": "."})).is_ok());
    // Removal closes the sessions within a second, and no link brings bob back.
    let t0 = Instant::now();
    own.call("room.remove", json!({"person": "device:bob"}))
        .unwrap();
    assert!(wait(Duration::from_secs(1), || {
        bob.call("session.ping", json!({})).is_err()
    }));
    assert!(t0.elapsed() < Duration::from_secs(1));
    let back = device(&b, &dir, "bob", &editor_rights());
    let e = back.call("room.join", json!({"link": link})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted);
    assert_eq!(
        back.call("fs.list", json!({"path": "."})).unwrap_err().code,
        ErrorCode::NotGranted
    );
    // A role change also closes the channel at once.
    let carl_link = own.call("room.share", json!({"role": "editor"})).unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned();
    let carl = device(&b, &dir, "carl", &editor_rights());
    join(&carl, &carl_link);
    own.call(
        "room.set_role",
        json!({"person": "device:carl", "role": "viewer"}),
    )
    .unwrap();
    assert!(wait(Duration::from_secs(1), || {
        carl.call("session.ping", json!({})).is_err()
    }));
    // The last admin stays.
    assert_eq!(
        own.call("room.remove", json!({"person": "ssh:owner"}))
            .unwrap_err()
            .code,
        ErrorCode::Invalid
    );
}

#[test]
fn links_stay_secret_and_guests_stay_guests() {
    let dir = scratch("links");
    write(&dir, "a.txt", "text\n");
    let (clk, skew) = clock();
    let (b, rooms) = make_backend(&dir, clk);
    let own = owner(&b, &dir);
    own.call("room.create", json!({})).unwrap();
    // Refusals of the rule.
    let e = own
        .call(
            "room.share",
            json!({"role": "editor", "audience": "anonymous"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Invalid);
    let e = own
        .call(
            "room.share",
            json!({"audience": "anonymous", "ttl_seconds": 8 * 24 * 3600}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Invalid);
    // Default audience and role.
    let plain = own.call("room.share", json!({})).unwrap();
    assert_eq!(plain["role"], "viewer");
    assert_eq!(plain["audience"], "organisation");
    let anon = own
        .call(
            "room.share",
            json!({"audience": "anonymous", "ttl_seconds": 3600}),
        )
        .unwrap();
    let link = anon["link"].as_str().unwrap().to_owned();
    assert!(link.contains('#'));
    let secret = link.rsplit_once('#').unwrap().1.to_owned();
    // A guest reads, and cannot run or change anything.
    let guest = device(&b, &dir, "guest", &Right::ALL);
    let j = guest.call("room.join", json!({"link": link})).unwrap();
    assert_eq!(j["role"], "viewer");
    assert!(guest.call("fs.list", json!({"path": "."})).is_ok());
    assert_eq!(
        guest
            .call("fs.write", json!({"path": "a.txt", "text": "x"}))
            .unwrap_err()
            .code,
        ErrorCode::NotGranted
    );
    let members = own.call("room.status", json!({})).unwrap();
    assert!(members["members"].get("device:guest").is_none());
    // Only an admin sees the member list.
    let seen_by_guest = guest.call("room.status", json!({})).unwrap();
    assert!(seen_by_guest.get("members").is_none());
    // The secret is in no stored file and no log.
    rooms.flush(&b);
    assert!(wait(Duration::from_secs(3), || {
        dir.join("state/remote/audit.jsonl").exists()
    }));
    let mut seen = Vec::new();
    collect(&dir.join("state"), &mut seen);
    assert!(seen.len() >= 2, "{seen:?}");
    for f in &seen {
        let text = String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned();
        assert!(!text.contains(&secret), "{f:?} holds the link secret");
    }
    let wrong = device(&b, &dir, "mallory", &Right::ALL);
    assert_eq!(
        wrong
            .call("room.join", json!({"link": "idecraft://room/x#nope"}))
            .unwrap_err()
            .code,
        ErrorCode::NotGranted
    );
    // Revoking ends a guest within a second; a revoked link no longer admits.
    let t0 = Instant::now();
    own.call("room.revoke_link", json!({"id": anon["id"]}))
        .unwrap();
    assert!(wait(Duration::from_secs(1), || {
        guest.call("session.ping", json!({})).is_err()
    }));
    assert!(t0.elapsed() < Duration::from_secs(1));
    let late = device(&b, &dir, "late", &Right::ALL);
    assert!(late.call("room.join", json!({"link": link})).is_err());
    // Expiry does the same.
    let short = own
        .call(
            "room.share",
            json!({"audience": "anonymous", "ttl_seconds": 60}),
        )
        .unwrap();
    let next = device(&b, &dir, "next", &Right::ALL);
    next.call("room.join", json!({"link": short["link"]}))
        .unwrap();
    skew.fetch_add(61_000, Ordering::SeqCst);
    assert!(wait(Duration::from_secs(1), || {
        next.call("session.ping", json!({})).is_err()
    }));
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            collect(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}

#[test]
fn agents_are_capped_and_recorded() {
    let dir = scratch("agent");
    write(&dir, "a.txt", "text\n");
    let (clk, _) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let own = owner(&b, &dir);
    own.call("room.create", json!({})).unwrap();
    let agent = owner(&b, &dir);
    let j = agent
        .call("room.join", json!({"agent": "Claude", "role": "admin"}))
        .unwrap();
    assert_eq!(j["role"], "runner");
    let list = own.call("room.status", json!({})).unwrap();
    assert!(
        list["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["kind"] == "agent" && p["role"] == "runner")
    );
    let e = agent.call("room.share", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted);
    // Viewer owner: its agent stays a viewer.
    let link = own.call("room.share", json!({"role": "viewer"})).unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned();
    let vera = device(&b, &dir, "vera", &Right::ALL);
    let j = vera
        .call("room.join", json!({"link": link, "agent": "Helper"}))
        .unwrap();
    assert_eq!(j["role"], "viewer");
}

#[test]
fn the_history_restores_and_survives_a_restart() {
    let dir = scratch("history");
    write(&dir, "a.txt", "first\n");
    write(&dir, "gone.txt", "precious\n");
    let (clk, _) = clock();
    let (b, rooms) = make_backend(&dir, clk.clone());
    let (own, _) = room_with_link(&b, &dir, "viewer");
    let change = |text: &str| {
        own.call(
            "room.apply",
            json!({"changes": [{"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": text}]}]}),
        )
        .unwrap();
    };
    change("two ");
    let marker = own.call("room.version", json!({"name": "v1"})).unwrap();
    change("three ");
    own.call(
        "room.apply",
        json!({"changes": [{"kind": "delete", "path": "gone.txt"}]}),
    )
    .unwrap();
    let hist = own.call("room.history", json!({"doc": "a.txt"})).unwrap();
    assert_eq!(hist["entries"].as_array().unwrap().len(), 2);
    assert!(
        hist["entries"][0].get("ops").is_none(),
        "no text in a listing"
    );
    // Restore to a named point.
    let seq = marker["seq"].as_u64().unwrap();
    own.call("room.restore", json!({"doc": "a.txt", "seq": seq}))
        .unwrap();
    assert_eq!(b.document("a.txt").unwrap().text, "two first\n");
    // Undoing the restore brings the later text back.
    own.call("room.undo", json!({})).unwrap();
    assert_eq!(b.document("a.txt").unwrap().text, "three two first\n");
    // A deleted file comes back.
    own.call("room.restore", json!({"doc": "gone.txt", "seq": seq}))
        .unwrap();
    assert_eq!(read(&dir, "gone.txt"), "precious\n");
    b.autosave_room();
    rooms.flush(&b);
    let id = own.call("room.status", json!({})).unwrap()["id"].clone();
    let before = own.call("room.history", json!({})).unwrap()["entries"]
        .as_array()
        .unwrap()
        .len();
    let share = own.call("room.share", json!({"role": "editor"})).unwrap();
    rooms.flush(&b);
    let mut own = own;
    own.drop_connection();
    drop(own);
    // A new backend on the same state: the room is back, the owner still admin.
    let (b2, _r2) = make_backend(&dir, clk);
    let own2 = owner(&b2, &dir);
    let st = own2.call("room.status", json!({})).unwrap();
    assert_eq!(st["id"], id);
    assert_eq!(
        own2.call("room.history", json!({})).unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        before
    );
    // Links survive, as hashes; the journal keeps its order.
    let carl = device(&b2, &dir, "carl", &editor_rights());
    assert_eq!(
        join(&carl, share["link"].as_str().unwrap())["role"],
        "editor"
    );
    // A change while no backend watched is refused for a restore, not guessed.
    // No backend may watch the outside change: one that sees it records it, and a restore over a
    // recorded change is then allowed (this raced under load). Dropping a client session does not
    // end its connection, whose serving threads keep the backend and its watcher alive, so the
    // connections are ended; the pause lets those threads see it (they wake every 250 ms).
    let (mut carl, mut own2) = (carl, own2);
    carl.drop_connection();
    own2.drop_connection();
    drop((carl, own2, b, rooms, b2, _r2));
    std::thread::sleep(Duration::from_millis(600));
    write(&dir, "a.txt", "something else entirely\n");
    let (clk3, _) = clock();
    let (b3, _r3) = make_backend(&dir, clk3);
    let own3 = owner(&b3, &dir);
    let e = own3
        .call("room.restore", json!({"doc": "a.txt", "seq": seq}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
}

#[test]
fn retention_drops_old_entries() {
    let dir = scratch("retain");
    write(&dir, "a.txt", "x\n");
    let (clk, skew) = clock();
    let (b, rooms) = make_backend(&dir, clk);
    let own = owner(&b, &dir);
    own.call("room.create", json!({})).unwrap();
    own.call(
        "room.apply",
        json!({"changes": [{"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "y"}]}]}),
    )
    .unwrap();
    assert_eq!(
        own.call("room.history", json!({})).unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    skew.fetch_add(31 * 24 * 3600 * 1000, Ordering::SeqCst);
    rooms.tick(&b);
    assert!(wait(Duration::from_secs(2), || own
        .call("room.history", json!({}))
        .unwrap()["entries"]
        .as_array()
        .unwrap()
        .is_empty()));
}

#[test]
fn presence_is_volatile_and_limited() {
    let dir = scratch("presence");
    write(&dir, "a.txt", "x\n");
    // The limit is per second of the room's clock; a still clock keeps all calls in one second
    // however slowly the machine runs them.
    let (clk, now) = still_clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let (mut own, link) = room_with_link(&b, &dir, "viewer");
    let rx = own.listen_host("room.presence");
    let bob = device(&b, &dir, "bob", &Right::ALL);
    join(&bob, &link);
    bob.call("room.presence", json!({"doc": "a.txt", "caret": 1}))
        .unwrap();
    assert!(wait(Duration::from_secs(3), || {
        own.pump(Duration::from_millis(10));
        rx.try_recv().is_ok()
    }));
    let history = own.call("room.history", json!({})).unwrap();
    assert_eq!(history["entries"].as_array().unwrap().len(), 0);
    let presence = |caret: u64| {
        bob.call("room.presence", json!({"doc": "a.txt", "caret": caret}))
            .unwrap()["dropped"]
            == true
    };
    // 30 updates a second: the first one above and 29 more pass, the rest of the second is dropped.
    let dropped: Vec<bool> = (0..40).map(|_| presence(2)).collect();
    assert_eq!(dropped.iter().filter(|d| !**d).count(), 29, "{dropped:?}");
    assert!(dropped[29..].iter().all(|d| *d), "{dropped:?}");
    // The next second passes again.
    now.fetch_add(1000, Ordering::SeqCst);
    assert!(!presence(3));
}

/// Guests see no participant ids and nothing else that names a login or a device: not in the
/// status, not in notifications, the history or comments, and they get no host notices. Members
/// keep what their role shows (`SPEC-0323` behaviour 21a, AK-15).
#[test]
fn guests_see_no_ids_or_names_of_others() {
    let dir = scratch("guest-privacy");
    write(&dir, "a.txt", "text\n");
    let (clk, _) = clock();
    let (b, _rooms) = make_backend(&dir, clk);
    let (own, editor_link) = room_with_link(&b, &dir, "editor");
    let anon = own
        .call(
            "room.share",
            json!({"audience": "anonymous", "ttl_seconds": 3600}),
        )
        .unwrap();
    own.call("room.version", json!({"name": "v1"})).unwrap();
    own.call(
        "room.comment",
        json!({"doc": "a.txt", "at": 1, "text": "look"}),
    )
    .unwrap();

    let mut guest = device(&b, &dir, "visitor-laptop", &Right::ALL);
    let rx = guest.listen_host("room.");
    let notices = guest.listen_host("notice.");
    let joined = guest
        .call("room.join", json!({"link": anon["link"]}))
        .unwrap();
    assert_eq!(joined["role"], "viewer");

    // A member arrives after the guest, follows the owner's session and then the owner's id.
    let alice = device(&b, &dir, "alice", &Right::ALL);
    join(&alice, &editor_link);
    let owner_client = own.info.client.clone();
    alice
        .call(
            "room.presence",
            json!({"doc": "a.txt", "caret": 2, "follows": owner_client}),
        )
        .unwrap();
    alice
        .call(
            "room.presence",
            json!({"doc": "a.txt", "caret": 3, "follows": "ssh:owner"}),
        )
        .unwrap();
    own.call("room.version", json!({"name": "v2"})).unwrap();
    b.post_notice("device", "Device paired: alice", None);

    let mut seen: Vec<(String, Value)> = Vec::new();
    let take = |guest: &mut ClientSession, seen: &mut Vec<(String, Value)>| {
        guest.pump(Duration::from_millis(10));
        while let Ok(m) = rx.try_recv() {
            seen.push(m);
        }
    };
    assert!(wait(Duration::from_secs(5), || {
        take(&mut guest, &mut seen);
        seen.iter().filter(|(m, _)| m == "room.presence").count() >= 2
            && seen
                .iter()
                .any(|(m, p)| m == "room.changed" && p["name"] == "v2")
    }));
    // Leaving ends the session, so the reply may not arrive.
    let _ = alice.call("room.leave", json!({}));
    assert!(wait(Duration::from_secs(5), || {
        take(&mut guest, &mut seen);
        seen.iter().any(|(m, _)| m == "room.left")
    }));
    assert!(seen.iter().any(|(m, _)| m == "room.joined"));

    let status = guest.call("room.status", json!({})).unwrap();
    let history = guest.call("room.history", json!({})).unwrap();
    let since = guest.call("room.since", json!({"seq": 0})).unwrap();
    let comments = guest.call("room.comments", json!({})).unwrap();
    let everything = json!([status, history, since, comments, seen]).to_string();
    for secret in ["ssh:owner", "owner", "device:", "alice", "visitor"] {
        assert!(
            !everything.contains(secret),
            "a guest sees {secret:?}: {everything}"
        );
    }
    for p in status["participants"].as_array().unwrap() {
        assert!(p.get("id").is_none(), "{p}");
        let name = p["name"].as_str().unwrap();
        assert!(name == "Member" || name.starts_with("Guest "), "{p}");
    }
    let presences: Vec<&Value> = seen
        .iter()
        .filter(|(m, _)| m == "room.presence")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(presences[0]["presence"]["follows"], owner_client.as_str());
    assert_eq!(presences[1]["presence"]["follows"], Value::Null);
    assert_eq!(history["entries"][0]["author"], "Member");
    assert!(history["entries"][0].get("actor").is_none());
    assert_eq!(comments["comments"][0]["name"], "Member");
    // No host notices for a guest.
    guest.pump(Duration::from_millis(50));
    assert!(notices.try_recv().is_err());
    assert_eq!(
        guest.call("notice.list", json!({})).unwrap_err().code,
        ErrorCode::NotGranted
    );

    // Members still see ids, the owner (admin) also the members.
    let by_owner = own.call("room.status", json!({})).unwrap();
    assert!(by_owner.to_string().contains("ssh:owner"));
    assert!(by_owner["members"].get("device:alice").is_some());
    let listed = own.call("notice.list", json!({})).unwrap();
    assert!(!listed["notices"].as_array().unwrap().is_empty());
}
