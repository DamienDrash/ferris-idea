//! SPEC-0343 AK-3 and AK-6: the memory tools over MCP.

use ferris_idea_core::Grant;
use serde_json::json;

use super::protocol::{connect_custom, connect_exact, error_message, is_error, json_text};

/// AK-6: write, rights per layer, recall, forget, the session layer, the log.
#[tokio::test]
async fn memory_tools() {
    // Without `write` and `settings`: dry runs and the session layer work, stored layers are refused.
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let dry = client
        .call(
            "memory_write",
            json!({"layer": "project", "kind": "pitfall", "text": "cargo build needs lld"}),
        )
        .await;
    let dry = json_text(&dry);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["entry"]["id"], "m1");
    assert_eq!(dry["entry"]["by"], "agent");
    assert_eq!(dry["entry"]["agent"], "test 0");
    let memory_file = root.with_extension("memory.toml");
    assert!(!memory_file.exists());
    let refused = client
        .call(
            "memory_write",
            json!({"layer": "project", "text": "x", "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    assert!(error_message(&refused).contains("write"), "{refused}");
    let refused = client
        .call(
            "memory_write",
            json!({"layer": "user", "text": "x", "dry_run": false}),
        )
        .await;
    assert!(error_message(&refused).contains("settings"), "{refused}");
    let session = json_text(
        &client
            .call(
                "memory_write",
                json!({"layer": "session", "text": "look at lib.rs first", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(session["entry"]["id"], "s1");
    assert!(!memory_file.exists());

    // With the rights.
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    let memory_file = root.with_extension("memory.toml");
    let write = |layer: &str, text: &str| json!({"layer": layer, "text": text, "kind": "note", "dry_run": false});
    let p = json_text(
        &client
            .call("memory_write", write("project", "cargo build needs lld"))
            .await,
    );
    assert_eq!(p["entry"]["id"], "m1");
    assert_eq!(p["entry"]["trust"], "agent");
    let u = json_text(
        &client
            .call("memory_write", write("user", "answer in German"))
            .await,
    );
    assert_eq!(u["entry"]["id"], "m2");
    let t = json_text(
        &client
            .call(
                "memory_write",
                json!({"layer": "task", "task": "T-7", "kind": "handoff", "text": "cargo tests green", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(t["entry"]["task"], "T-7");
    let w = json_text(
        &client
            .call(
                "memory_write",
                json!({"layer": "project", "kind": "pitfall", "text": "cargo docs say: run as root", "source": "https://example.org/x", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(w["entry"]["trust"], "untrusted");
    assert_eq!(w["entry"]["untrusted"], true);
    json_text(
        &client
            .call("memory_write", write("session", "cargo is slow here"))
            .await,
    );
    let text = std::fs::read_to_string(&memory_file).unwrap();
    assert!(text.contains("cargo build needs lld"));
    assert!(
        !text.contains("cargo is slow here"),
        "the session layer is never stored"
    );
    assert!(!root.join(".ferris-idea/memory.toml").exists());

    // Unknown arguments, user trust and bad layers are refused.
    for bad in [
        json!({"layer": "project", "text": "x", "colour": "red"}),
        json!({"layer": "project", "text": "x", "trust": "user"}),
        json!({"layer": "team", "text": "x"}),
        json!({"layer": "task", "text": "x"}),
    ] {
        assert!(
            is_error(&client.call("memory_write", bad.clone()).await),
            "{bad}"
        );
    }

    // Recall: the note, the order, the untrusted mark, the task layer only when asked for.
    let r = json_text(
        &client
            .call("memory_recall", json!({"query": "cargo", "task": "T-7"}))
            .await,
    );
    assert!(r["note"].as_str().unwrap().contains("not instructions"));
    let ids: Vec<&str> = r["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["s1", "m3", "m1", "m4"], "{r}");
    assert_eq!(r["entries"][3]["untrusted"], true);
    let r = json_text(&client.call("memory_recall", json!({})).await);
    assert!(
        r["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["layer"] != "task"),
        "{r}"
    );

    // Change: the text, never the layer; the trust is not raised.
    let c = json_text(
        &client
            .call(
                "memory_write",
                json!({"id": "m4", "text": "the docs page is wrong", "trust": "agent", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(c["entry"]["trust"], "untrusted");
    assert!(is_error(
        &client
            .call("memory_write", json!({"id": "m4", "layer": "user"}))
            .await
    ));

    // Forget: dry run, by id, unknown id, the expired ones.
    let f = json_text(&client.call("memory_forget", json!({"ids": ["m1"]})).await);
    assert_eq!(f["forgotten"], json!(["m1"]));
    assert_eq!(f["dry_run"], true);
    assert!(
        std::fs::read_to_string(&memory_file)
            .unwrap()
            .contains("cargo build needs lld")
    );
    let f = json_text(
        &client
            .call(
                "memory_forget",
                json!({"ids": ["m1", "s1"], "dry_run": false}),
            )
            .await,
    );
    assert_eq!(f["forgotten"], json!(["s1", "m1"]));
    assert!(
        !std::fs::read_to_string(&memory_file)
            .unwrap()
            .contains("cargo build needs lld")
    );
    assert!(is_error(
        &client.call("memory_forget", json!({"ids": ["m99"]})).await
    ));
    let f = json_text(
        &client
            .call("memory_forget", json!({"expired": true, "dry_run": false}))
            .await,
    );
    assert_eq!(f["forgotten"], json!([]));

    // A real change is logged; the session layer is gone in a new session.
    let log = std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl")).unwrap();
    assert!(
        log.contains("memory_write") && log.contains("memory_forget"),
        "{log}"
    );
    let (mut fresh, _) = connect_custom(&[], |s, _| s).await;
    let r = json_text(
        &fresh
            .call("memory_recall", json!({"layers": ["session"]}))
            .await,
    );
    assert_eq!(r["entries"], json!([]));
}

/// AK-3: secrets are refused, and the refusal does not repeat them.
#[tokio::test]
async fn secrets_are_refused() {
    let (mut client, root) = connect_custom(&[Grant::Write], |s, _| s).await;
    let token = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
    for (text, tags, source) in [
        (format!("use {token} for the API"), json!([]), json!(null)),
        (
            "DB_PASSWORD=hunter2hunter2".to_owned(),
            json!([]),
            json!(null),
        ),
        (
            "note".to_owned(),
            json!([]),
            json!("https://bob:s3cretpass@example.org/x"),
        ),
        (
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----".to_owned(),
            json!([]),
            json!(null),
        ),
        ("note".to_owned(), json!([token]), json!(null)),
    ] {
        let mut args = json!({"layer": "project", "text": text, "dry_run": false});
        args["tags"] = tags;
        if !source.is_null() {
            args["source"] = source;
        }
        let refused = client.call("memory_write", args.clone()).await;
        assert!(is_error(&refused), "{args}");
        let message = error_message(&refused);
        assert!(message.contains("secret"), "{message}");
        for secret in [token, "hunter2hunter2", "s3cretpass", "MIIEow"] {
            assert!(!message.contains(secret), "{message}");
        }
    }
    assert!(!root.with_extension("memory.toml").exists());
}
