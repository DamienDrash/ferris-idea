//! SPEC-0323 v9 AK-40: the MCP tools `room_diff` and `room_restore` with `docs` against a real
//! session: the diff reads without any right, restoring several files needs the user-granted
//! `write` right and lands as one group.

use serde_json::json;

use super::protocol::{connect_exact, error_message, is_error, json_text};

#[tokio::test(flavor = "multi_thread")]
async fn room_diff_reads_freely_and_restore_takes_several_files_with_write() {
    use ferris_idea_remote::{LiveProject, Timing, inprocess::InProcessProject};
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-mcp-room-history-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("project/a.txt"), "alpha\n").unwrap();
    std::fs::write(dir.join("project/b.txt"), "beta\n").unwrap();
    let folder = dir.join("project").display().to_string();
    let setup: ferris_idea_remote::inprocess::Setup = std::sync::Arc::new(|b| {
        ferris_idea_session::room::bind(b, ferris_idea_session::room::RoomConfig::default());
    });
    let project = InProcessProject::open_with(
        "room-history-host",
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
    live.call(
        "room.apply",
        json!({"changes": [
            {"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "one "}]},
            {"kind": "edit", "path": "b.txt", "ops": [{"at": 0, "insert": "two "}]},
        ]}),
    )
    .unwrap();
    let args = |extra: serde_json::Value| {
        let mut a = json!({"host": "room-history-host", "folder": folder});
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        a
    };

    // Without rights: the history and the diff of the change set's first entry.
    let (mut reader, _r) = connect_exact(&[], |s, _| s).await;
    let history = json_text(&reader.call("room_history", args(json!({}))).await);
    let entry = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["doc"] == "a.txt")
        .cloned()
        .expect("the change set's a.txt entry");
    let seq = entry["seq"].as_u64().unwrap();
    let diff = json_text(
        &reader
            .call(
                "room_diff",
                args(json!({"doc": "a.txt", "from": seq - 1, "to": seq})),
            )
            .await,
    );
    assert_eq!(diff["lines"], json!(["-alpha", "+one alpha"]), "{diff}");
    assert_eq!(
        (diff["added"].as_u64(), diff["removed"].as_u64()),
        (Some(1), Some(1))
    );
    let refused = reader
        .call(
            "room_restore",
            args(json!({"docs": ["a.txt", "b.txt"], "seq": seq - 1})),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    assert!(error_message(&refused).contains("write"), "{refused}");
    assert_eq!(
        live.call("doc.open", json!({"path": "a.txt"})).unwrap()["text"],
        "one alpha\n"
    );

    // With `write`: both files back as one group.
    let (mut writer, _w) = connect_exact(&[ferris_idea_core::Grant::Write], |s, _| s).await;
    let none = writer
        .call("room_restore", args(json!({"seq": seq - 1})))
        .await;
    assert!(is_error(&none), "{none}");
    let done = json_text(
        &writer
            .call(
                "room_restore",
                args(json!({"docs": ["a.txt", "b.txt"], "seq": seq - 1})),
            )
            .await,
    );
    assert_eq!(done["changes"], 2, "{done}");
    let group = done["group"].clone();
    let after = json_text(&reader.call("room_history", args(json!({}))).await);
    let restored: Vec<_> = after["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["group"] == group)
        .collect();
    assert_eq!(restored.len(), 2, "{after}");
    let _ = live.close(ferris_idea_remote::Unsaved::Discard);
    let _ = std::fs::remove_dir_all(&dir);
}
