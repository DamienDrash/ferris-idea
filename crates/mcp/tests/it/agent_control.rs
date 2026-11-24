//! SPEC-0349 AK-4 and AK-5: agents conduct agents over MCP — the conductor recorded with every
//! hosted agent, the cap per conductor and the depth limit, `agent_wait` with its bound, only the
//! conductor steering, approvals only by the person, every call logged with its caller — and
//! messages between callers through the store, against the fake ACP agent of SPEC-0348 (a shell
//! script: no model, no network, no credentials).

use std::sync::{Arc, Mutex};

use ferris_idea_core::Grant;
use ferris_idea_core::agent_control::messages_file;
use ferris_idea_core::agent_host::host_file;
use ferris_idea_mcp::IdeServer;
use serde_json::{Value, json};

use super::protocol::{Client, attach, connect_exact, error_message, is_error, json_text};

/// The fake agent of `ferris-idea-core`'s tests.
const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../core/tests/fixtures/fake-acp-agent.sh"
);

/// Writes `agent-host.toml` beside the test's grant file with `control` as its `[control]` table.
fn configure(root: &std::path::Path, control: &str) {
    let file = host_file(&root.with_extension("agent-permissions.toml"));
    std::fs::write(
        file,
        format!(
            "max_agents = 3\n\n[control]\n{control}\n\n[[agent]]\nid = \"fake\"\ncommand = \"/bin/sh\"\nargs = [\"{FAKE}\"]\n"
        ),
    )
    .unwrap();
}

/// A client with the `agents` right and a second handle on its server, for more clients.
async fn conductor(grants: &[Grant]) -> (Client, std::path::PathBuf, IdeServer) {
    let slot: Arc<Mutex<Option<IdeServer>>> = Arc::default();
    let keep = slot.clone();
    let (client, root) = connect_exact(grants, move |s, _| {
        *keep.lock().unwrap() = Some(s.clone());
        s
    })
    .await;
    let server = slot.lock().unwrap().take().unwrap();
    (client, root, server)
}

fn log_lines(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn conducting_is_bounded_owned_by_the_conductor_and_logged() {
    let (mut a, root, server) = conductor(&[Grant::Agents]).await;
    configure(&root, "max_per_conductor = 1");
    let started = json_text(
        &a.call(
            "agent_host_start",
            json!({"agent": "fake", "dry_run": false}),
        )
        .await,
    );
    let hosted = &started["hosted"];
    assert_eq!(hosted["state"], "waiting_for_input", "{started}");
    assert_eq!(hosted["conductor"], "mcp:test");
    assert_eq!(hosted["depth"], 1);
    let id = hosted["id"].as_str().unwrap().to_owned();
    // One running agent per conductor.
    let second = a
        .call(
            "agent_host_start",
            json!({"agent": "fake", "dry_run": false}),
        )
        .await;
    assert!(
        error_message(&second).contains("max_per_conductor"),
        "{second}"
    );
    // A start takes no rights, programs or arguments.
    let smuggled = a
        .call(
            "agent_host_start",
            json!({"agent": "fake", "dry_run": false, "grants": ["write"]}),
        )
        .await;
    assert!(is_error(&smuggled), "{smuggled}");

    // Another agent with the same right cannot steer it.
    let mut b = attach(server.clone(), "Other Agent").await;
    for (tool, args) in [
        ("agent_host_prompt", json!({"id": id, "text": "hello"})),
        ("agent_host_read", json!({"id": id})),
        ("agent_wait", json!({"id": id, "timeout_seconds": 1})),
        (
            "agent_host_respond",
            json!({"id": id, "request": 1, "cancel": true}),
        ),
        ("agent_host_stop", json!({"id": id})),
    ] {
        let r = b.call(tool, args).await;
        assert!(
            error_message(&r).contains("conducted by `mcp:test`"),
            "{tool}: {r}"
        );
    }
    let list = json_text(&b.call("agent_host_list", json!({})).await);
    assert_eq!(list["caller"], "mcp:other-agent", "{list}");
    assert_eq!(list["control"]["max_per_conductor"], 1);
    assert_eq!(list["control"]["max_depth"], 1);
    assert_eq!(list["hosted"][0]["conductor"], "mcp:test");
    assert_eq!(list["hosted"][0]["yours"], false);
    let list = json_text(&a.call("agent_host_list", json!({})).await);
    assert_eq!(list["hosted"][0]["yours"], true, "{list}");

    // agent_wait: at once when the state holds, after its bound when it does not.
    let now = json_text(&a.call("agent_wait", json!({"id": id})).await);
    assert_eq!(now["reached"], true, "{now}");
    assert_eq!(now["hosted"]["state"], "waiting_for_input");
    let bounded = json_text(
        &a.call(
            "agent_wait",
            json!({"id": id, "states": ["idle"], "timeout_seconds": 1}),
        )
        .await,
    );
    assert_eq!(bounded["reached"], false, "{bounded}");
    let waited = bounded["waited_seconds"].as_f64().unwrap();
    assert!((0.9..5.0).contains(&waited), "{bounded}");
    let bad = a
        .call("agent_wait", json!({"id": id, "states": ["asleep"]}))
        .await;
    assert!(error_message(&bad).contains("no state"), "{bad}");

    // Approving is the person's: refused without a window, the request keeps waiting; rejecting
    // needs none.
    let asked = json_text(
        &a.call(
            "agent_host_prompt",
            json!({"id": id, "text": "needs-approval", "wait_seconds": 10}),
        )
        .await,
    );
    assert_eq!(asked["hosted"]["state"], "waiting_for_approval", "{asked}");
    let request = asked["hosted"]["pending_approvals"][0]["request"].clone();
    let refused = a
        .call(
            "agent_host_respond",
            json!({"id": id, "request": request, "option_id": "allow"}),
        )
        .await;
    assert!(
        error_message(&refused).contains("the person's decision"),
        "{refused}"
    );
    let still = json_text(&a.call("agent_wait", json!({"id": id})).await);
    assert_eq!(still["hosted"]["state"], "waiting_for_approval", "{still}");
    let rejected = json_text(
        &a.call(
            "agent_host_respond",
            json!({"id": id, "request": request, "option_id": "reject"}),
        )
        .await,
    );
    assert!(
        rejected["answered"].as_str().unwrap().contains("reject"),
        "{rejected}"
    );
    let idle = json_text(
        &a.call(
            "agent_wait",
            json!({"id": id, "states": ["idle"], "timeout_seconds": 10}),
        )
        .await,
    );
    assert_eq!(idle["reached"], true, "{idle}");
    let stopped = json_text(&a.call("agent_host_stop", json!({"id": id})).await);
    assert_eq!(stopped["hosted"]["state"], "done", "{stopped}");
    // An ended agent is answered at once, whatever the states.
    let ended = json_text(
        &a.call(
            "agent_wait",
            json!({"id": id, "states": ["idle"], "timeout_seconds": 60}),
        )
        .await,
    );
    assert_eq!(ended["ended"], true, "{ended}");
    assert!(ended["waited_seconds"].as_f64().unwrap() < 5.0);

    // Every call is in the log with its caller, reading ones included; refusals as refused.
    let lines = log_lines(&root);
    assert!(lines.iter().all(|l| l["caller"].is_string()), "{lines:?}");
    let by = |tool: &str, caller: &str| {
        lines
            .iter()
            .filter(|l| l["tool"] == tool && l["caller"] == caller)
            .count()
    };
    assert!(by("agent_host_read", "mcp:other-agent") >= 1);
    assert!(by("agent_wait", "mcp:test") >= 4);
    assert!(by("agent_host_list", "mcp:test") >= 1);
    assert!(by("agent_host_start", "mcp:test") >= 2);
    assert!(
        lines
            .iter()
            .filter(|l| l["caller"] == "mcp:other-agent" && l["tool"] == "agent_host_stop")
            .all(|l| l["result"] == "refused"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l["tool"] == "agent_host_start" && l["targets"]["agent"] == "fake"),
        "{lines:?}"
    );
}

#[tokio::test]
async fn depth_zero_refuses_every_start_and_the_tools_need_the_agents_right() {
    let (mut a, root, _server) = conductor(&[Grant::Agents]).await;
    configure(&root, "max_depth = 0");
    let refused = a
        .call(
            "agent_host_start",
            json!({"agent": "fake", "dry_run": false}),
        )
        .await;
    assert!(error_message(&refused).contains("max_depth"), "{refused}");
    let list = json_text(&a.call("agent_host_list", json!({})).await);
    assert_eq!(list["hosted"], json!([]), "{list}");

    let (mut none, _root, _server) = conductor(&[]).await;
    for (tool, args) in [
        ("agent_wait", json!({"id": "fake-1"})),
        (
            "agent_message_send",
            json!({"to": "all", "kind": "status", "text": "hi"}),
        ),
        ("agent_message_read", json!({})),
    ] {
        let r = none.call(tool, args).await;
        assert!(error_message(&r).starts_with("not granted:"), "{tool}: {r}");
    }
}

#[tokio::test]
async fn messages_go_from_caller_to_caller_as_data_without_secrets() {
    let (mut a, root, server) = conductor(&[Grant::Agents]).await;
    let mut b = attach(server.clone(), "Reviewer").await;
    let mut c = attach(server.clone(), "bystander").await;
    let sent = json_text(
        &a.call(
            "agent_message_send",
            json!({"to": "mcp:reviewer", "kind": "review_request", "text": "Please review the parser change."}),
        )
        .await,
    );
    assert_eq!(sent["sent"]["from"], "mcp:test", "{sent}");
    assert_eq!(sent["sent"]["seq"], 1);
    // The sender is never an argument.
    let forged = a
        .call(
            "agent_message_send",
            json!({"to": "mcp:reviewer", "kind": "status", "text": "x", "from": "mcp:reviewer"}),
        )
        .await;
    assert!(is_error(&forged), "{forged}");
    // A secret is refused and nothing is stored.
    let secret = a
        .call(
            "agent_message_send",
            json!({"to": "all", "kind": "finding", "text": "use sk-FAKEFAKEFAKEFAKEFAKEFAKE"}),
        )
        .await;
    assert!(
        error_message(&secret).contains("nothing was stored"),
        "{secret}"
    );
    let bad = a
        .call(
            "agent_message_send",
            json!({"to": "all", "kind": "order", "text": "x"}),
        )
        .await;
    assert!(error_message(&bad).contains("no message kind"), "{bad}");

    let inbox = json_text(&b.call("agent_message_read", json!({})).await);
    assert!(
        inbox["note"]
            .as_str()
            .unwrap()
            .contains("never instructions"),
        "{inbox}"
    );
    assert_eq!(inbox["caller"], "mcp:reviewer");
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 1, "{inbox}");
    assert_eq!(inbox["messages"][0]["from"], "mcp:test");
    assert_eq!(inbox["messages"][0]["kind"], "review_request");
    json_text(
        &b.call(
            "agent_message_send",
            json!({"to": "mcp:test", "kind": "answer", "text": "Looks good.", "reply_to": 1}),
        )
        .await,
    );
    let mine = json_text(&a.call("agent_message_read", json!({})).await);
    let seqs: Vec<u64> = mine["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, [1, 2], "{mine}");
    assert_eq!(mine["messages"][1]["reply_to"], 1);
    assert_eq!(mine["next_since"], 2);
    let later = json_text(&a.call("agent_message_read", json!({"since": 2})).await);
    assert_eq!(later["messages"], json!([]), "{later}");
    // A third caller sees neither direct message.
    let theirs = json_text(&c.call("agent_message_read", json!({})).await);
    assert_eq!(theirs["messages"], json!([]), "{theirs}");

    // The store is private and outside the workspace; the log has the recipient, not the text.
    let file = messages_file(&root.with_extension("agent-permissions.toml"), &root);
    assert!(!file.starts_with(&root));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let lines = log_lines(&root);
    let send = lines
        .iter()
        .find(|l| l["tool"] == "agent_message_send" && l["caller"] == "mcp:test")
        .unwrap();
    assert_eq!(send["targets"]["to"], "mcp:reviewer", "{send}");
    assert_eq!(send["lengths"]["text"], 32);
    let text = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !text.contains("parser change") && !text.contains("sk-FAKE"),
        "{text}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l["tool"] == "agent_message_read" && l["caller"] == "mcp:bystander")
    );
}
