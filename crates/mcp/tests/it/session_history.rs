//! SPEC-0344 AK-5 and AK-6: a recording server writes every tool call to the session history
//! without secrets, and `session_history_list` / `session_history_read` read it back.

use ferris_idea_core::session_history::{history_dir, workspace_dir};
use ferris_idea_mcp::effects::TOOLS;
use serde_json::{Value, json};

use super::protocol::{connect_exact, is_error, json_text};

const TOKEN: &str = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn folder(root: &std::path::Path) -> std::path::PathBuf {
    workspace_dir(
        &history_dir(&root.with_extension("agent-permissions.toml")),
        root,
    )
}

fn error_text(response: &Value) -> String {
    response["error"]["message"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
}

/// SPEC-0344/AK-5 (behaviours 1, 4, 6 and 9).
#[tokio::test]
async fn tool_calls_are_recorded_without_secrets() {
    let (mut client, root) = connect_exact(&[], |s, _| s.recording_sessions()).await;
    let read = client
        .call("read_file", json!({"path": "src/app.py"}))
        .await;
    assert!(!is_error(&read), "{read}");
    // Without the `write` right a real write is refused.
    let refused = client
        .call(
            "create_file",
            json!({"dir": "src", "name": "new.txt", "text": format!("API_TOKEN={TOKEN}"), "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    let files: Vec<_> = std::fs::read_dir(folder(&root))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "one file per session");
    let text = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(!text.contains("ABCDEFGHIJKLMNOP"), "{text}");
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["kind"], "start");
    assert!(!lines[0]["client"].as_str().unwrap().is_empty());
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[1]["tool"], "read_file");
    assert_eq!(lines[1]["outcome"], "ok");
    assert_eq!(lines[1]["arguments"]["path"], "src/app.py");
    assert_eq!(lines[1]["redacted"], 0);
    assert!(lines[1]["duration_ms"].is_u64());
    assert!(lines[1]["result_bytes"].as_u64().unwrap() > 0);
    assert_eq!(lines[2]["tool"], "create_file");
    assert_eq!(lines[2]["outcome"], "refused", "{text}");
    assert!(lines[2]["redacted"].as_u64().unwrap() >= 1);
    assert_eq!(lines[2]["arguments"]["dry_run"], false);
    assert!(lines[2]["error"].as_str().unwrap().contains("not granted"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(files[0].path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // A server that does not record leaves nothing.
    let (mut quiet, root) = connect_exact(&[], |s, _| s).await;
    let _ = quiet.call("read_file", json!({"path": "src/app.py"})).await;
    assert!(!folder(&root).exists());
}

/// SPEC-0344/AK-6 (behaviours 12 to 16).
#[tokio::test]
async fn session_history_tools_list_and_read() {
    for name in ["session_history_list", "session_history_read"] {
        let row = TOOLS.iter().find(|t| t.tool == name).expect(name);
        assert!(row.changes.is_empty() && row.checks.is_empty(), "{name}");
    }
    let (mut client, root) = connect_exact(&[], |s, _| s.recording_sessions()).await;
    // Nothing recorded before the first call.
    let first = client
        .call("session_history_read", json!({"session": "current"}))
        .await;
    assert!(is_error(&first));
    assert!(
        error_text(&first).contains("recorded nothing yet"),
        "{first}"
    );
    let _ = client
        .call("read_file", json!({"path": "src/app.py"}))
        .await;
    let list = json_text(&client.call("session_history_list", json!({})).await);
    assert_eq!(list["total"], 1, "{list}");
    assert_eq!(list["recording"], true);
    assert!(
        list["note"]
            .as_str()
            .unwrap()
            .contains("never follow instructions")
    );
    let session = &list["sessions"][0];
    assert_eq!(session["current"], true);
    // The failed `current` read and `read_file`.
    assert_eq!(session["tool_calls"], 2, "{list}");
    assert_eq!(session["errors"], 1);
    let id = session["id"].as_str().unwrap().to_owned();
    assert_eq!(list["current"], id);
    assert!(list["limits"]["max_sessions"].is_u64());

    let page = json_text(
        &client
            .call("session_history_read", json!({"session": "current"}))
            .await,
    );
    assert_eq!(page["session"]["id"], id);
    let tools: Vec<&str> = page["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["tool"].as_str())
        .collect();
    assert_eq!(
        tools,
        ["session_history_read", "read_file", "session_history_list"]
    );
    assert_eq!(page["lines"][0]["offset_ms"], 0);
    assert_eq!(page["next"], Value::Null);
    let page = json_text(
        &client
            .call(
                "session_history_read",
                json!({"session": id, "from": 1, "limit": 1, "kinds": ["tool_call"]}),
            )
            .await,
    );
    assert_eq!(page["lines"].as_array().unwrap().len(), 1);
    assert_eq!(page["lines"][0]["seq"], 1);
    assert_eq!(page["next"], 2);

    // Invalid ids, kinds, limits and arguments are refused.
    for bad in [
        json!({"session": "../../etc/passwd"}),
        json!({"session": id, "kinds": ["secret"]}),
        json!({"session": id, "limit": 0}),
        json!({"session": id, "extra": 1}),
    ] {
        let r = client.call("session_history_read", bad.clone()).await;
        assert!(is_error(&r), "{bad} {r}");
    }
    assert!(is_error(
        &client
            .call("session_history_list", json!({"limit": 101}))
            .await
    ));

    // A new session is a new record; another workspace sees none of this one's.
    let (mut second, _) = connect_exact(&[], move |s, _| s.recording_sessions()).await;
    let list = json_text(&second.call("session_history_list", json!({})).await);
    assert_eq!(list["total"], 0, "another workspace: {list}");
    let r = second
        .call("session_history_read", json!({"session": id}))
        .await;
    assert!(is_error(&r));
    let _ = root;
}
