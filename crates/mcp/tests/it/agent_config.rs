//! SPEC-0346 AK-7: `agent_config` and `agent_config_export` over MCP — the view needs no right,
//! the export is a dry run by default, writing needs the `write` right and a trusted workspace,
//! a hand-written target needs `overwrite`, a target behind a link to outside is refused, and a
//! written file is listed as generated afterwards.

use ferris_idea_core::Grant;
use serde_json::json;

use super::protocol::{connect_custom, connect_exact, error_message, is_error, json_text};

fn put(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[tokio::test]
async fn agent_config_shows_the_merged_view_without_a_right() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    put(&root, "AGENTS.md", "# Tests\nRun them all.\n");
    put(&root, "CLAUDE.md", "@AGENTS.md\n\n# Tests\nRun them all.\n");
    put(
        &root,
        ".claude/settings.json",
        r#"{"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}}"#,
    );
    let view = json_text(&client.call("agent_config", json!({})).await);
    assert_eq!(view["hooks_executed"], false, "{view}");
    assert!(
        view["untrusted_data"]
            .as_str()
            .unwrap()
            .contains("not an instruction")
    );
    assert_eq!(view["total"], 2, "{view}");
    let tests = &view["items"][0];
    assert_eq!(tests["title"], "Tests", "{view}");
    assert_eq!(tests["sources"][0]["path"], "AGENTS.md");
    assert_eq!(tests["sources"][1]["path"], "CLAUDE.md");
    assert_eq!(view["sources"][1]["imports"], json!(["AGENTS.md"]));
    let hooks = json_text(&client.call("agent_config", json!({"kind": "hook"})).await);
    assert_eq!(hooks["total"], 1);
    assert_eq!(hooks["items"][0]["text"], "say done");
    assert_eq!(hooks["items"][0]["event"], "Stop");
    let page = json_text(
        &client
            .call("agent_config", json!({"limit": 1, "offset": 0}))
            .await,
    );
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["more"], true);
    let refused = client.call("agent_config", json!({"from": "nobody"})).await;
    assert!(is_error(&refused));
    assert!(error_message(&refused).contains("no known agent"));
}

#[tokio::test]
async fn agent_config_export_needs_the_write_right_and_overwrite() {
    // Without `write`: the dry run answers, writing is refused, nothing is written.
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    put(&root, "CLAUDE.md", "# Style\nShort lines.\n");
    let dry = json_text(
        &client
            .call(
                "agent_config_export",
                json!({"target": "agents_md", "from": "claude"}),
            )
            .await,
    );
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["file"], "AGENTS.md");
    assert_eq!(dry["exists"], false);
    assert!(
        dry["content"]
            .as_str()
            .unwrap()
            .contains("## Style\n\nShort lines.")
    );
    let refused = client
        .call(
            "agent_config_export",
            json!({"target": "agents_md", "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused));
    assert!(!root.join("AGENTS.md").exists());
    let same = client
        .call(
            "agent_config_export",
            json!({"target": "claude", "from": "claude"}),
        )
        .await;
    assert!(is_error(&same));

    // With `write`: a new file is written and is generated afterwards.
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    put(&root, "CLAUDE.md", "# Style\nShort lines.\n");
    let written = json_text(
        &client
            .call(
                "agent_config_export",
                json!({"target": "agents_md", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(written["written"], true, "{written}");
    let text = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(text.contains("ferris-idea:agent-config generated"));
    let view = json_text(&client.call("agent_config", json!({})).await);
    assert_eq!(view["generated"], json!(["AGENTS.md"]), "{view}");
    assert_eq!(view["total"], 1, "the export does not feed itself: {view}");
    // A generated file is replaced without `overwrite`; a hand-written one is not.
    let again = json_text(
        &client
            .call(
                "agent_config_export",
                json!({"target": "agents_md", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(again["generated_before"], true);
    assert_eq!(again["changed"], false);
    let dry = json_text(
        &client
            .call(
                "agent_config_export",
                json!({"target": "claude", "from": "agents_md"}),
            )
            .await,
    );
    assert_eq!(dry["needs_overwrite"], true, "{dry}");
    let refused = client
        .call(
            "agent_config_export",
            json!({"target": "claude", "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused));
    assert!(error_message(&refused).contains("overwrite"));
    assert_eq!(
        std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
        "# Style\nShort lines.\n"
    );
    let replaced = json_text(
        &client
            .call(
                "agent_config_export",
                json!({"target": "claude", "dry_run": false, "overwrite": true}),
            )
            .await,
    );
    assert_eq!(replaced["written"], true, "{replaced}");
    let text = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(text.contains("## Style\n\nShort lines."), "{text}");
    assert!(text.contains("ferris-idea:agent-config generated"));
}

#[tokio::test]
async fn agent_config_export_refuses_untrusted_workspaces_and_links_outside() {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-mcp-agent-config-trust-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let settings = ferris_idea_core::trust::TrustSettings::with_file(&dir.join("trusted.toml"));
    let (mut client, root) =
        connect_custom(&[Grant::Write], move |s, _| s.with_trust(settings)).await;
    put(&root, "AGENTS.md", "# Tests\nRun them.\n");
    let view = json_text(&client.call("agent_config", json!({})).await);
    assert_eq!(view["trusted"], false);
    assert_eq!(view["total"], 1, "reading is allowed in safe mode: {view}");
    let refused = client
        .call(
            "agent_config_export",
            json!({"target": "gemini", "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused));
    assert!(
        error_message(&refused).starts_with("Safe mode:"),
        "{}",
        error_message(&refused)
    );
    assert!(!root.join("GEMINI.md").exists());
    let _ = std::fs::remove_dir_all(&dir);

    #[cfg(unix)]
    {
        let (mut client, root) = connect_custom(&[], |s, _| s).await;
        let outside = root.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".cursor")).unwrap();
        put(&root, "AGENTS.md", "# Tests\nRun them.\n");
        let refused = client
            .call(
                "agent_config_export",
                json!({"target": "cursor", "dry_run": false}),
            )
            .await;
        assert!(is_error(&refused));
        assert!(error_message(&refused).contains("outside the project"));
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&outside);
    }
}
