//! SPEC-0348 AK-5: the agent host over MCP — `agent_host_list`, a dry run by default, the
//! `agents` right for everything that acts, and a whole turn with a permission request against a
//! fake ACP agent (a shell script: no model, no network, no credentials).

use ferris_idea_core::Grant;
use ferris_idea_core::agent_host::host_file;
use serde_json::json;

use super::protocol::{connect_exact, error_message, is_error, json_text};

/// A window whose person allows every question, and counts them (SPEC-0349 behaviour 7: an
/// approval is asked of the person every time).
#[derive(Default)]
struct ApprovingWindow {
    asked: std::sync::Mutex<Vec<ferris_idea_mcp::window::PermissionRequest>>,
}

impl ferris_idea_mcp::window::Window for ApprovingWindow {
    fn ask(
        &self,
        request: ferris_idea_mcp::window::PermissionRequest,
        reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PermissionAnswer>,
    ) {
        self.asked.lock().unwrap().push(request);
        let _ = reply.send(ferris_idea_mcp::window::PermissionAnswer::AllowOnce);
    }
    fn pending_work(
        &self,
        reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PendingWork>,
    ) {
        let _ = reply.send(ferris_idea_mcp::window::PendingWork::default());
    }
    fn quit(
        &self,
        _mode: ferris_idea_mcp::window::QuitMode,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    ) {
        let _ = reply.send(Err("not in this test".into()));
    }
}

/// The fake agent of `ferris-idea-core`'s tests.
const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../core/tests/fixtures/fake-acp-agent.sh"
);

/// Writes `agent-host.toml` beside the test's grant file, outside the workspace.
fn configure(root: &std::path::Path) {
    let file = host_file(&root.with_extension("agent-permissions.toml"));
    std::fs::write(
        file,
        format!(
            "max_agents = 1\n\n[[agent]]\nid = \"fake\"\nname = \"Fake agent\"\ncommand = \"/bin/sh\"\nargs = [\"{FAKE}\"]\n"
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn agent_host_list_shows_the_configuration_and_no_agents() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let list = json_text(&client.call("agent_host_list", json!({})).await);
    assert_eq!(list["agents"], json!([]), "{list}");
    assert_eq!(list["hosted"], json!([]));
    assert_eq!(list["max_agents"], 2);
    assert!(
        list["config_file"]
            .as_str()
            .unwrap()
            .ends_with(".agent-host.toml"),
        "{list}"
    );
    assert!(
        list["states"]
            .as_array()
            .unwrap()
            .contains(&json!("unknown"))
    );
    configure(&root);
    let list = json_text(&client.call("agent_host_list", json!({})).await);
    assert_eq!(list["agents"][0]["id"], "fake", "{list}");
    assert_eq!(list["max_agents"], 1);
    // An unknown agent is refused with the configured ones.
    let refused = client
        .call("agent_host_start", json!({"agent": "other"}))
        .await;
    assert!(error_message(&refused).contains("`fake`"), "{refused}");
}

#[tokio::test]
async fn agent_host_start_is_a_dry_run_by_default_and_needs_the_agents_right() {
    let (mut client, root) = connect_exact(&[Grant::Run, Grant::Settings], |s, _| s).await;
    configure(&root);
    let dry = json_text(
        &client
            .call("agent_host_start", json!({"agent": "fake"}))
            .await,
    );
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["plan"]["program"], "/bin/sh");
    assert_eq!(dry["offered"]["fs"], false);
    assert_eq!(dry["offered"]["mcp_servers"], json!([]));
    assert!(dry["sandbox"].as_str().unwrap().contains("M19.9"));
    let names = dry["plan"]["env_names"].as_array().unwrap();
    assert!(names.iter().all(|n| n != "CARGO_MANIFEST_DIR"), "{dry}");
    let refused = client
        .call(
            "agent_host_start",
            json!({"agent": "fake", "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused));
    assert!(
        error_message(&refused).starts_with("not granted:"),
        "{refused}"
    );
    for (tool, args) in [
        (
            "agent_host_prompt",
            json!({"id": "fake-1", "text": "hello"}),
        ),
        ("agent_host_read", json!({"id": "fake-1"})),
        (
            "agent_host_respond",
            json!({"id": "fake-1", "request": 1, "cancel": true}),
        ),
        ("agent_host_stop", json!({"id": "fake-1"})),
    ] {
        let r = client.call(tool, args).await;
        assert!(error_message(&r).starts_with("not granted:"), "{tool}: {r}");
    }
    let list = json_text(&client.call("agent_host_list", json!({})).await);
    assert_eq!(list["hosted"], json!([]));
}

#[tokio::test]
async fn a_hosted_agent_is_prompted_asks_for_approval_and_is_stopped() {
    let window = std::sync::Arc::new(ApprovingWindow::default());
    let w = window.clone();
    let (mut client, root) = connect_exact(&[Grant::Agents], move |s, _| s.with_window(w)).await;
    configure(&root);
    let started = json_text(
        &client
            .call(
                "agent_host_start",
                json!({"agent": "fake", "dry_run": false}),
            )
            .await,
    );
    let hosted = &started["hosted"];
    assert_eq!(hosted["state"], "waiting_for_input", "{started}");
    assert_eq!(hosted["session_ready"], true);
    let id = hosted["id"].as_str().unwrap().to_owned();
    // max_agents = 1: a second start is refused.
    let second = client
        .call(
            "agent_host_start",
            json!({"agent": "fake", "dry_run": false}),
        )
        .await;
    assert!(error_message(&second).contains("stop one"), "{second}");

    let after = json_text(
        &client
            .call(
                "agent_host_prompt",
                json!({"id": id, "text": "hello", "wait_seconds": 10}),
            )
            .await,
    );
    assert_eq!(after["hosted"]["state"], "idle", "{after}");
    let read = json_text(&client.call("agent_host_read", json!({"id": id})).await);
    let texts: Vec<&str> = read["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["text"].as_str().unwrap())
        .collect();
    assert!(texts.contains(&"echo: hello"), "{read}");
    let since = read["next_since"].as_u64().unwrap();

    // A permission request waits; allow_always is refused, allow_once goes through.
    let asked = json_text(
        &client
            .call(
                "agent_host_prompt",
                json!({"id": id, "text": "needs-approval", "wait_seconds": 10}),
            )
            .await,
    );
    let hosted = &asked["hosted"];
    assert_eq!(hosted["state"], "waiting_for_approval", "{asked}");
    let request = &hosted["pending_approvals"][0];
    assert_eq!(request["title"], "Write notes.txt");
    let always = request["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "allow_always")
        .unwrap();
    assert_eq!(always["allowed"], false);
    let list = json_text(&client.call("agent_host_list", json!({})).await);
    assert_eq!(list["hosted"][0]["state"], "waiting_for_approval", "{list}");
    assert!(
        list["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["state"] == "waiting_for_approval"),
        "{list}"
    );
    let refused = client
        .call(
            "agent_host_respond",
            json!({"id": id, "request": request["request"], "option_id": "always"}),
        )
        .await;
    assert!(
        error_message(&refused).contains("standing approval"),
        "{refused}"
    );
    let answered = json_text(
        &client
            .call(
                "agent_host_respond",
                json!({"id": id, "request": request["request"], "option_id": "allow"}),
            )
            .await,
    );
    assert!(
        answered["answered"]
            .as_str()
            .unwrap()
            .contains("allow_once")
    );
    // The person was asked for exactly this approval (SPEC-0349 behaviour 7).
    let asked = window.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].right, "approve");
    assert!(asked[0].meaning.contains("Write notes.txt"), "{asked:?}");
    let read = json_text(
        &client
            .call(
                "agent_host_read",
                json!({"id": id, "since": since, "wait_seconds": 10, "until": ["idle"]}),
            )
            .await,
    );
    assert_eq!(read["hosted"]["state"], "idle", "{read}");
    assert!(
        read["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "message" && e["text"] == "approved"),
        "{read}"
    );
    assert!(
        read["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["seq"].as_u64().unwrap() > since)
    );

    let stopped = json_text(&client.call("agent_host_stop", json!({"id": id})).await);
    assert_eq!(stopped["hosted"]["state"], "done", "{stopped}");
    assert_eq!(stopped["hosted"]["exited"], true);
    // The changing calls went to the agent log.
    let log = std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl")).unwrap();
    assert!(log.contains("agent_host_start"), "{log}");
    assert!(log.contains("agent_host_respond"), "{log}");
}

/// SPEC-0348 v2 AK-7: `agent_host_changes` lists what a turn changed (also a file a command
/// created), shows the diff and undoes only with `write`; an edit request carries its diff, and
/// a refused edit is no change.
#[tokio::test]
async fn agent_host_changes_lists_diffs_and_undoes_a_turn() {
    let (mut client, root) = connect_exact(&[Grant::Agents], |s, _| s).await;
    configure(&root);
    std::fs::write(root.join("notes.txt"), "hello\n").unwrap();
    let started = json_text(
        &client
            .call(
                "agent_host_start",
                json!({"agent": "fake", "dry_run": false}),
            )
            .await,
    );
    let id = started["hosted"]["id"].as_str().unwrap().to_owned();
    let none = client.call("agent_host_changes", json!({"id": id})).await;
    assert!(error_message(&none).contains("no turn yet"), "{none}");

    let asked = json_text(
        &client
            .call(
                "agent_host_prompt",
                json!({"id": id, "text": "edit-twice", "wait_seconds": 10}),
            )
            .await,
    );
    let request = &asked["hosted"]["pending_approvals"][0];
    assert_eq!(request["diffs"][0]["path"], "notes.txt", "{asked}");
    assert_eq!(request["diffs"][0]["new_text"], "hello world\n");
    let mut marked = false;
    for _ in 0..2 {
        let read = json_text(
            &client
                .call("agent_host_read", json!({"id": id, "wait_seconds": 10}))
                .await,
        );
        let Some(p) = read["hosted"]["pending_approvals"]
            .as_array()
            .unwrap()
            .first()
            .cloned()
        else {
            break;
        };
        marked |= p["similar_refused"] == 1;
        client
            .call(
                "agent_host_respond",
                json!({"id": id, "request": p["request"], "option_id": "reject"}),
            )
            .await;
    }
    assert!(marked, "the repeated edit carries the mark of the refusal");
    let read = json_text(
        &client
            .call("agent_host_read", json!({"id": id, "wait_seconds": 10}))
            .await,
    );
    assert_eq!(read["hosted"]["state"], "idle", "{read}");
    let refused = json_text(&client.call("agent_host_changes", json!({"id": id})).await);
    assert_eq!(refused["files"], json!([]), "{refused}");

    client
        .call(
            "agent_host_prompt",
            json!({"id": id, "text": "write-files", "wait_seconds": 10}),
        )
        .await;
    let changes = json_text(
        &client
            .call("agent_host_changes", json!({"id": id, "diff": true}))
            .await,
    );
    assert_eq!(changes["turn"], 2, "{changes}");
    let paths: Vec<&str> = changes["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["Built.class"], "{changes}");
    assert!(
        changes["diff"]
            .as_str()
            .unwrap()
            .contains("Binary or unrecorded file Built.class"),
        "{changes}"
    );
    let dry = json_text(
        &client
            .call("agent_host_changes", json!({"id": id, "undo": true}))
            .await,
    );
    assert_eq!(dry["undo"]["dry_run"], true, "{dry}");
    assert_eq!(dry["undo"]["blockers"], json!([]));
    assert_eq!(dry["undo"]["would_put_back"], json!(["Built.class"]));
    let no_write = client
        .call(
            "agent_host_changes",
            json!({"id": id, "undo": true, "dry_run": false}),
        )
        .await;
    assert!(is_error(&no_write), "{no_write}");
    assert!(root.join("Built.class").exists());
    client.call("agent_host_stop", json!({"id": id})).await;
}

/// SPEC-0348 v2 AK-7: with `write`, the newest turn is undone through MCP.
#[tokio::test]
async fn agent_host_changes_undoes_with_the_write_right() {
    let (mut client, root) = connect_exact(&[Grant::Agents, Grant::Write], |s, _| s).await;
    configure(&root);
    let started = json_text(
        &client
            .call(
                "agent_host_start",
                json!({"agent": "fake", "dry_run": false}),
            )
            .await,
    );
    let id = started["hosted"]["id"].as_str().unwrap().to_owned();
    client
        .call(
            "agent_host_prompt",
            json!({"id": id, "text": "write-files", "wait_seconds": 10}),
        )
        .await;
    assert!(root.join("notes.txt").exists());
    let undone = json_text(
        &client
            .call(
                "agent_host_changes",
                json!({"id": id, "undo": true, "dry_run": false}),
            )
            .await,
    );
    assert_eq!(undone["undone"], true, "{undone}");
    assert!(!root.join("notes.txt").exists());
    assert!(!root.join("Built.class").exists());
    let log = std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl")).unwrap();
    assert!(log.contains("agent_host_changes"), "{log}");
    client.call("agent_host_stop", json!({"id": id})).await;
}

/// SPEC-0357 AK-4: `agent_host_detect` lists installed ACP programs, needs no right and never
/// adds one.
#[tokio::test]
async fn agent_host_detect_lists_programs_and_adds_nothing() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let detected = json_text(&client.call("agent_host_detect", json!({})).await);
    assert!(detected["found"].is_array(), "{detected}");
    assert!(
        detected["adding"]
            .as_str()
            .unwrap()
            .contains("only the person"),
        "{detected}"
    );
    let file = host_file(&root.with_extension("agent-permissions.toml"));
    assert!(!file.exists(), "detecting writes no configuration");
}
