//! SPEC-0358 AK-14: the MCP tool `claude_code_link` reports the link of the project — running,
//! port, clients, lock file — and never the token. The link's lock file goes to a temporary folder.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use ferris_idea_claude_link::{
    Diagnostic, DiffDecision, DiffProposal, Editor, HubConfig, OpenEditor, OpenRequest, Selection,
};
use serde_json::json;

use super::protocol::{connect_exact, is_error, json_text};

/// A window that answers nothing.
struct Silent;

impl Editor for Silent {
    fn selection(&self, _: PathBuf, _: Sender<Option<Selection>>) {}
    fn open_editors(&self, _: PathBuf, _: Sender<Vec<OpenEditor>>) {}
    fn diagnostics(&self, _: PathBuf, _: Option<PathBuf>, _: Sender<Vec<Diagnostic>>) {}
    fn open_file(&self, _: PathBuf, _: OpenRequest, _: Sender<Result<(), String>>) {}
    fn show_diff(&self, _: PathBuf, _: DiffProposal, _: Sender<DiffDecision>) {}
    fn close_diff(&self, _: PathBuf, _: String) {}
}

#[tokio::test]
async fn claude_code_link_reports_the_state_without_the_token() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let locks =
        std::env::temp_dir().join(format!("ferris-idea-mcp-claude-ide-{}", std::process::id()));
    ferris_idea_claude_link::install(
        HubConfig {
            lock_dir: locks.clone(),
            ide_version: "test".into(),
        },
        Arc::new(Silent),
    );
    ferris_idea_claude_link::set_projects(vec![root.clone()]);

    let before = client.call("claude_code_link", json!({})).await;
    assert!(!is_error(&before), "{before}");
    let before = json_text(&before);
    assert_eq!(before["running"], false, "{before}");
    assert_eq!(before["in_this_process"], true);

    let port = ferris_idea_claude_link::ensure_started(&root).unwrap();
    let lock = locks.join(format!("{port}.lock"));
    let token = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&lock).unwrap())
        .unwrap()["authToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let answer = client.call("claude_code_link", json!({})).await;
    let state = json_text(&answer);
    assert_eq!(state["running"], true, "{state}");
    assert_eq!(state["listening"], true);
    assert_eq!(state["port"], port);
    assert_eq!(state["clients"], json!([]));
    assert_eq!(state["lock_file"], json!(lock));
    assert!(
        !answer.to_string().contains(&token),
        "the token never leaves"
    );

    // The terminal of the project gets the port and the switch.
    let env = ferris_idea_claude_link::terminal_env(&root);
    assert!(env.contains(&("CLAUDE_CODE_SSE_PORT".into(), port.to_string())));
    assert!(env.contains(&("ENABLE_IDE_INTEGRATION".into(), "true".into())));

    // The project window closes: the link stops and its lock file goes.
    ferris_idea_claude_link::set_projects(Vec::new());
    assert!(!lock.exists());
    let after = json_text(&client.call("claude_code_link", json!({})).await);
    assert_eq!(after["running"], false);
    let _ = std::fs::remove_dir_all(&locks);
}
