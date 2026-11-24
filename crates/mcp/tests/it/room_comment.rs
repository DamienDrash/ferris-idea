//! SPEC-0323 v8 AK-35: the MCP tool `room_comment` against a real session: listing needs no right,
//! every change needs the user-granted `write` right and reaches the room.

use serde_json::json;

use super::protocol::{connect_exact, error_message, is_error, json_text};

#[tokio::test(flavor = "multi_thread")]
async fn room_comment_lists_freely_and_changes_with_write() {
    use ferris_idea_remote::{LiveProject, Timing, inprocess::InProcessProject};
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-mcp-room-comment-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("project/a.txt"), "hello\nworld\n").unwrap();
    let folder = dir.join("project").display().to_string();
    let setup: ferris_idea_remote::inprocess::Setup = std::sync::Arc::new(|b| {
        ferris_idea_session::room::bind(b, ferris_idea_session::room::RoomConfig::default());
    });
    let project = InProcessProject::open_with(
        "room-comment-host",
        &dir.join("project"),
        &dir.join("state"),
        std::time::Duration::from_secs(600),
        None,
        std::time::Duration::ZERO,
        Some(setup),
    )
    .unwrap();
    let live = LiveProject::start(Box::new(project), Timing::default());
    live.publish();
    live.call("room.create", json!({"name": "demo"})).unwrap();
    let args = |extra: serde_json::Value| {
        let mut a = json!({"host": "room-comment-host", "folder": folder});
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        a
    };

    // Without rights: listing works, adding is refused naming the right.
    let (mut reader, _r) = connect_exact(&[], |s, _| s).await;
    let empty = json_text(&reader.call("room_comment", args(json!({}))).await);
    assert_eq!(empty["comments"], json!([]), "{empty}");
    let refused = reader
        .call(
            "room_comment",
            args(json!({"action": "add", "doc": "a.txt", "text": "hi"})),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    assert!(error_message(&refused).contains("write"), "{refused}");

    // With `write`: a thread, a reply, resolve, reopen and delete reach the room.
    let (mut writer, _w) = connect_exact(&[ferris_idea_core::Grant::Write], |s, _| s).await;
    let added = json_text(
        &writer
            .call(
                "room_comment",
                args(json!({"action": "add", "doc": "a.txt", "at": 6, "text": "look here"})),
            )
            .await,
    );
    let id = added["id"].as_u64().expect("an id");
    json_text(
        &writer
            .call(
                "room_comment",
                args(json!({"action": "reply", "id": id, "text": "seen"})),
            )
            .await,
    );
    let resolved = json_text(
        &writer
            .call("room_comment", args(json!({"action": "resolve", "id": id})))
            .await,
    );
    assert_eq!(resolved["resolved"], true);
    json_text(
        &writer
            .call("room_comment", args(json!({"action": "reopen", "id": id})))
            .await,
    );
    let list = json_text(
        &reader
            .call("room_comment", args(json!({"doc": "a.txt"})))
            .await,
    );
    let list = list["comments"].as_array().unwrap().clone();
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!(list[0]["at"], 6);
    assert_eq!(list[0]["resolved"], false);
    assert_eq!(list[1]["reply_to"], id);
    let bad = writer
        .call("room_comment", args(json!({"action": "shout"})))
        .await;
    assert!(is_error(&bad), "{bad}");
    let deleted = json_text(
        &writer
            .call("room_comment", args(json!({"action": "delete", "id": id})))
            .await,
    );
    assert_eq!(deleted["deleted"], 2, "{deleted}");
    let _ = live.close(ferris_idea_remote::Unsaved::Discard);
    let _ = std::fs::remove_dir_all(&dir);
}
