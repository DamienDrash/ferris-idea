//! SPEC-0323 v8 AK-31: the comments of a room end to end over real sessions in the process:
//! threads with replies, resolving and reopening, deleting one's own, the roles, plain text within
//! the limit, paths inside the project, the notification to the others and anchors moving with
//! the text.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_session::room::{self, RoomConfig};
use ferris_idea_session::{
    Backend, BackendConfig, ClientSession, ErrorCode, Peer, Right, RpcError, in_process,
};
use serde_json::{Value, json};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-room-comments-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn backend(dir: &Path) -> (Backend, std::sync::Arc<room::Rooms>) {
    let b = Backend::new(BackendConfig {
        folder: Some(dir.join("project")),
        state_dir: Some(dir.join("state")),
        peer: "ssh:owner@10.0.0.9".into(),
        watch_interval: Duration::from_millis(30),
        ..BackendConfig::default()
    });
    let rooms = room::bind(
        &b,
        RoomConfig {
            tick: Duration::from_millis(20),
            ..RoomConfig::default()
        },
    );
    (b, rooms)
}

fn session(b: &Backend, dir: &Path, device: Option<&str>) -> ClientSession {
    let (window, server) = in_process();
    let bb = b.clone();
    match device {
        None => {
            std::thread::spawn(move || bb.serve(server));
        }
        Some(name) => {
            let peer = Peer {
                name: format!("device:{name}:{name}"),
                rights: Right::ALL.to_vec(),
            };
            std::thread::spawn(move || bb.serve_as(server, peer));
        }
    }
    let folder = dir.join("project").display().to_string();
    ClientSession::open(window, &folder, "t", "h", None).unwrap()
}

fn code(r: Result<Value, RpcError>) -> ErrorCode {
    r.expect_err("refused").code
}

fn link(own: &ClientSession, role: &str) -> String {
    own.call("room.share", json!({ "role": role })).unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn list(c: &ClientSession) -> Vec<Value> {
    c.call("room.comments", json!({"doc": "a.txt"})).unwrap()["comments"]
        .as_array()
        .unwrap()
        .clone()
}

fn by_id(list: &[Value], id: &Value) -> Value {
    list.iter()
        .find(|c| &c["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn threads_replies_resolving_and_deleting() {
    let dir = scratch("threads");
    std::fs::write(dir.join("project/a.txt"), "one\ntwo\nthree\n").unwrap();
    let (b, _rooms) = backend(&dir);
    let own = session(&b, &dir, None);
    own.call("room.create", json!({"name": "demo"})).unwrap();
    let carl_link = link(&own, "commenter");
    let viewer_link = link(&own, "viewer");
    let mut carl = session(&b, &dir, Some("carl"));
    let seen = carl.listen_host("room.");
    carl.call("room.join", json!({ "link": carl_link }))
        .unwrap();
    let viewer = session(&b, &dir, Some("vera"));
    viewer
        .call("room.join", json!({ "link": viewer_link }))
        .unwrap();

    // A thread by the owner; Carl hears that a.txt's comments changed.
    let root = own
        .call(
            "room.comment",
            json!({"doc": "a.txt", "at": 4, "text": "rename this"}),
        )
        .unwrap();
    assert_eq!(root["doc"], "a.txt");
    let t0 = Instant::now();
    let mut heard = None;
    while heard.is_none() && t0.elapsed() < Duration::from_secs(5) {
        carl.pump(Duration::from_millis(20));
        while let Ok((m, p)) = seen.try_recv() {
            if m == "room.comments_changed" {
                heard = Some(p);
            }
        }
    }
    assert_eq!(heard.expect("Carl is told")["doc"], "a.txt");

    // Carl replies; a reply to the reply joins the same thread at the thread's place.
    let reply = carl
        .call(
            "room.comment",
            json!({"reply_to": root["id"], "text": "agreed"}),
        )
        .unwrap();
    assert_eq!(reply["reply_to"], root["id"]);
    let again = own
        .call(
            "room.comment",
            json!({"reply_to": reply["id"], "text": "done"}),
        )
        .unwrap();
    assert_eq!(again["reply_to"], root["id"], "replies hang on the first");
    let all = list(&own);
    assert_eq!(all.len(), 3);
    assert_eq!(by_id(&all, &reply["id"])["at"], 4);
    assert_eq!(by_id(&all, &reply["id"])["doc"], "a.txt");
    assert_eq!(by_id(&all, &root["id"])["own"], true);
    assert_eq!(by_id(&all, &reply["id"])["own"], false);
    assert_eq!(by_id(&list(&carl), &reply["id"])["own"], true);
    assert_eq!(
        code(carl.call("room.comment", json!({"reply_to": 999, "text": "x"}))),
        ErrorCode::NotFound
    );

    // An edit before the place moves every comment of the thread.
    own.call(
        "room.apply",
        json!({"changes": [{"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "zero\n"}]}]}),
    )
    .unwrap();
    assert!(list(&own).iter().all(|c| c["at"] == 9), "{:?}", list(&own));

    // Resolving through a reply resolves the thread; reopening takes it back.
    let r = carl
        .call("room.resolve_comment", json!({"id": reply["id"]}))
        .unwrap();
    assert_eq!(r, json!({"id": root["id"], "resolved": true}));
    assert_eq!(by_id(&list(&own), &root["id"])["resolved"], true);
    carl.call(
        "room.resolve_comment",
        json!({"id": root["id"], "resolved": false}),
    )
    .unwrap();
    assert_eq!(by_id(&list(&own), &root["id"])["resolved"], false);

    // A viewer reads but neither comments, replies, resolves nor deletes.
    assert_eq!(list(&viewer).len(), 3);
    assert!(list(&viewer).iter().all(|c| c["own"] == false));
    for (method, params) in [
        ("room.comment", json!({"doc": "a.txt", "text": "hi"})),
        (
            "room.comment",
            json!({"reply_to": root["id"], "text": "hi"}),
        ),
        ("room.resolve_comment", json!({"id": root["id"]})),
        ("room.delete_comment", json!({"id": root["id"]})),
    ] {
        assert_eq!(
            code(viewer.call(method, params)),
            ErrorCode::NotGranted,
            "{method}"
        );
    }

    // Only the author deletes; a first comment others replied to stays.
    assert_eq!(
        code(carl.call("room.delete_comment", json!({"id": root["id"]}))),
        ErrorCode::NotGranted
    );
    assert_eq!(
        code(own.call("room.delete_comment", json!({"id": root["id"]}))),
        ErrorCode::Conflict
    );
    assert_eq!(
        carl.call("room.delete_comment", json!({"id": reply["id"]}))
            .unwrap()["deleted"],
        1
    );
    // Without others' replies, the first comment takes its own replies along.
    assert_eq!(
        own.call("room.delete_comment", json!({"id": root["id"]}))
            .unwrap()["deleted"],
        2
    );
    assert!(list(&own).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn text_and_place_are_checked() {
    let dir = scratch("checks");
    std::fs::write(dir.join("project/a.txt"), "text\n").unwrap();
    let (b, rooms) = backend(&dir);
    let own = session(&b, &dir, None);
    own.call("room.create", json!({"name": "demo"})).unwrap();
    for params in [
        json!({"doc": "../outside.txt", "text": "x"}),
        json!({"doc": "/etc/passwd", "text": "x"}),
        json!({"doc": "a.txt", "text": ""}),
        json!({"doc": "a.txt", "text": "   "}),
        json!({"doc": "a.txt", "text": "colour \u{1b}[31m"}),
        json!({"doc": "a.txt", "text": "x".repeat(16 * 1024 + 1)}),
        json!({"doc": "a.txt", "at": -1, "text": "x"}),
        json!({"doc": "a.txt", "at": "3", "text": "x"}),
        json!({"doc": "a.txt", "reply_to": "1", "text": "x"}),
        json!({"doc": "x".repeat(5000), "text": "x"}),
    ] {
        assert_eq!(
            code(own.call("room.comment", params.clone())),
            ErrorCode::Invalid,
            "{params}"
        );
    }
    let longest = own
        .call(
            "room.comment",
            json!({"doc": "a.txt", "text": "x".repeat(16 * 1024)}),
        )
        .unwrap();
    own.call("room.delete_comment", json!({"id": longest["id"]}))
        .unwrap();
    let ok = own
        .call("room.comment", json!({"doc": "a.txt", "text": "first"}))
        .unwrap();
    assert_eq!(
        code(own.call(
            "room.resolve_comment",
            json!({"id": ok["id"], "resolved": "no"})
        )),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(own.call("room.delete_comment", json!({"id": 4242}))),
        ErrorCode::NotFound
    );
    // Replies survive a restart with their thread.
    own.call(
        "room.comment",
        json!({"reply_to": ok["id"], "text": "kept"}),
    )
    .unwrap();
    rooms.flush(&b);
    drop((own, b, rooms));
    let (b, _rooms) = backend(&dir);
    let own = session(&b, &dir, None);
    let kept = list(&own);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[1]["reply_to"], ok["id"]);
    let _ = std::fs::remove_dir_all(&dir);
}
