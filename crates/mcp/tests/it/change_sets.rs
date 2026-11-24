//! SPEC-0340 AK-2 to AK-6: the change-set tools over MCP — proposing needs no right, applying and
//! undoing need `write`, keep the secret-file rule and the base hash, write through the local
//! history and leave lines in the agent log.

use serde_json::{Value, json};

use super::protocol::{connect_custom, connect_exact, error_message, is_error, json_text};

const APP: &str = "a = 1\nb = 2\nc = 3\n";

fn read(root: &std::path::Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap()
}

async fn begin(client: &mut super::protocol::Client, extra: Value) -> String {
    let mut args = json!({"name": "Tidy the app"});
    if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
        a.extend(e.clone());
    }
    let begun = json_text(&client.call("change_set_begin", args).await);
    begun["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn proposing_needs_no_write_right() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let begun = json_text(
        &client
            .call(
                "change_set_begin",
                json!({
                    "name": "Tidy the app",
                    "task": "T-7 API_KEY=sk-live-0123456789abcdef",
                    "agent": "test-agent",
                    "model": "local-7b",
                    "cost": {"input_tokens": 1200, "output_tokens": 300, "amount": 0.02, "currency": "USD"},
                }),
            )
            .await,
    );
    let id = begun["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("cs-"), "{begun}");
    assert_eq!(begun["meta"]["agent"], "test-agent");
    assert_eq!(begun["meta"]["cost"]["input_tokens"], 1200);
    let task = begun["meta"]["task"].as_str().unwrap();
    assert!(task.starts_with("T-7"), "{begun}");
    assert!(!task.contains("sk-live-0123456789abcdef"), "{begun}");
    let stored = read(&root, &format!(".ferris-idea/change-sets/{id}.json"));
    assert!(!stored.contains("sk-live-0123456789abcdef"));
    // Three proposals: a replacement, a new file, a deletion.
    let edit = json_text(
        &client
            .call(
                "change_set_edit",
                json!({"id": id, "path": "src/app.py", "old_text": "b = 2", "new_text": "b = 20"}),
            )
            .await,
    );
    assert_eq!(edit["status"], "pending", "{edit}");
    assert_eq!(edit["hunks"], 1);
    let new = client
        .call(
            "change_set_edit",
            json!({"id": id, "path": "src/new.py", "text": "x = 1\n"}),
        )
        .await;
    assert!(!is_error(&new), "{new}");
    let gone = client
        .call(
            "change_set_edit",
            json!({"id": id, "path": "README.md", "delete": true}),
        )
        .await;
    assert!(!is_error(&gone), "{gone}");
    let cost = json_text(
        &client
            .call(
                "change_set_edit",
                json!({"id": id, "cost": {"input_tokens": 5000}}),
            )
            .await,
    );
    assert_eq!(cost["meta"]["cost"]["input_tokens"], 5000, "{cost}");
    assert_eq!(cost["meta"]["model"], "local-7b");
    // Status and diff read the set.
    let status = json_text(&client.call("change_set_status", json!({"id": id})).await);
    assert_eq!(status["counts"]["pending"], 3, "{status}");
    assert!(
        status["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["matches_base"] == true),
        "{status}"
    );
    let list = json_text(&client.call("change_set_status", json!({})).await);
    assert_eq!(list["sets"][0]["id"], id.as_str());
    let diff = json_text(
        &client
            .call("change_set_diff", json!({"id": id, "path": "src/app.py"}))
            .await,
    );
    let hunk = &diff["files"][0]["hunks"][0];
    assert_eq!(hunk["kind"], "modified", "{diff}");
    assert_eq!(hunk["old_lines"], json!(["b = 2"]));
    assert_eq!(hunk["new_lines"], json!(["b = 20"]));
    assert!(
        diff["files"][0]["unified"]
            .as_str()
            .unwrap()
            .contains("+b = 20")
    );
    // The dry run works, the real apply needs `write`; nothing changed.
    let dry = json_text(&client.call("change_set_apply", json!({"id": id})).await);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["conflicts"], json!([]), "{dry}");
    assert_eq!(dry["files"].as_array().unwrap().len(), 3);
    let refused = client
        .call("change_set_apply", json!({"id": id, "dry_run": false}))
        .await;
    assert!(is_error(&refused), "{refused}");
    assert!(error_message(&refused).contains("`write`"), "{refused}");
    assert_eq!(read(&root, "src/app.py"), APP);
    assert!(!root.join("src/new.py").exists());
    assert!(root.join("README.md").exists());
    // The edits are in the agent log, as calls that changed Ferris IDEA's own records.
    let log = read(&root, ".ferris-idea/agent-log.jsonl");
    assert!(log.contains("change_set_edit"), "{log}");
}

#[tokio::test]
async fn apply_reject_undo_round_trip() {
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    let id = begin(&mut client, json!({})).await;
    for args in [
        json!({"id": id, "path": "src/app.py", "text": "a = 10\nb = 2\nc = 30\n"}),
        json!({"id": id, "path": "src/new.py", "text": "x = 1\n"}),
        json!({"id": id, "path": "README.md", "delete": true}),
    ] {
        let r = client.call("change_set_edit", args).await;
        assert!(!is_error(&r), "{r}");
    }
    let reject = json_text(
        &client
            .call(
                "change_set_reject",
                json!({"id": id, "hunks": {"src/app.py": [2]}}),
            )
            .await,
    );
    assert_eq!(reject["files"][0]["rejected_hunks"], json!([2]), "{reject}");
    let refused = client
        .call(
            "change_set_apply",
            json!({"id": id, "hunks": {"src/app.py": [2]}}),
        )
        .await;
    assert!(is_error(&refused), "a rejected hunk is never applied");
    let applied = json_text(
        &client
            .call("change_set_apply", json!({"id": id, "dry_run": false}))
            .await,
    );
    assert_eq!(applied["applied"].as_array().unwrap().len(), 3, "{applied}");
    assert_eq!(read(&root, "src/app.py"), "a = 10\nb = 2\nc = 3\n");
    assert_eq!(read(&root, "src/new.py"), "x = 1\n");
    assert!(!root.join("README.md").exists());
    // The local history holds the state before and the labelled state after.
    let history = json_text(
        &client
            .call("local_history", json!({"path": "src/app.py"}))
            .await,
    );
    let label = format!("Change set {id}: Tidy the app");
    assert!(
        history["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["label"] == label.as_str()),
        "{history}"
    );
    let log = read(&root, ".ferris-idea/agent-log.jsonl");
    assert!(
        log.lines()
            .any(|l| l.contains("\"change_set_apply\"") && l.contains("\"written\"")),
        "{log}"
    );
    let status = json_text(&client.call("change_set_status", json!({"id": id})).await);
    assert_eq!(status["counts"]["applied"], 3, "{status}");
    assert!(
        status["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["matches_written"] == true),
        "{status}"
    );
    let again = client
        .call(
            "change_set_edit",
            json!({"id": id, "path": "src/app.py", "text": "z\n"}),
        )
        .await;
    assert!(is_error(&again), "an applied file takes no more edits");
    // Undo needs each file to hold what the set wrote.
    std::fs::write(root.join("src/new.py"), "x = 2\n").unwrap();
    let dry = json_text(&client.call("change_set_undo", json!({"id": id})).await);
    assert_eq!(dry["conflicts"].as_array().unwrap().len(), 1, "{dry}");
    let refused = client
        .call("change_set_undo", json!({"id": id, "dry_run": false}))
        .await;
    assert!(
        error_message(&refused).starts_with("conflict:"),
        "{refused}"
    );
    assert_eq!(read(&root, "src/app.py"), "a = 10\nb = 2\nc = 3\n");
    std::fs::write(root.join("src/new.py"), "x = 1\n").unwrap();
    let undone = json_text(
        &client
            .call("change_set_undo", json!({"id": id, "dry_run": false}))
            .await,
    );
    assert_eq!(undone["undone"].as_array().unwrap().len(), 3, "{undone}");
    assert_eq!(read(&root, "src/app.py"), APP);
    assert!(!root.join("src/new.py").exists());
    assert_eq!(read(&root, "README.md"), "#");
    let log = read(&root, ".ferris-idea/agent-log.jsonl");
    assert!(log.contains("\"change_set_undo\""), "{log}");
}

#[tokio::test]
async fn conflicts_and_guards() {
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    let id = begin(&mut client, json!({})).await;
    for args in [
        json!({"id": id, "path": "src/app.py", "old_text": "c = 3", "new_text": "c = 4"}),
        json!({"id": id, "path": "src/other.py", "text": "y = 1\n"}),
    ] {
        let r = client.call("change_set_edit", args).await;
        assert!(!is_error(&r), "{r}");
    }
    // The file changed after the edit was proposed: a conflict, no file of the set written.
    std::fs::write(root.join("src/app.py"), "a = 1\nb = 2\nc = 3\nd = 4\n").unwrap();
    let dry = json_text(&client.call("change_set_apply", json!({"id": id})).await);
    assert_eq!(dry["conflicts"].as_array().unwrap().len(), 1, "{dry}");
    let refused = client
        .call("change_set_apply", json!({"id": id, "dry_run": false}))
        .await;
    assert!(
        error_message(&refused).starts_with("conflict:"),
        "{refused}"
    );
    assert!(error_message(&refused).contains("src/app.py"));
    assert!(!root.join("src/other.py").exists());
    assert_eq!(read(&root, "src/app.py"), "a = 1\nb = 2\nc = 3\nd = 4\n");
    // Paths that leave the workspace or reach Ferris IDEA's or Git's folders are refused.
    for bad in [
        "../outside.py",
        "/etc/hosts",
        ".ferris-idea/agent-log.jsonl",
        ".git/config",
    ] {
        let r = client
            .call(
                "change_set_edit",
                json!({"id": id, "path": bad, "text": "x"}),
            )
            .await;
        assert!(is_error(&r), "{bad}: {r}");
    }
    #[cfg(unix)]
    {
        let outside = root.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let r = client
            .call(
                "change_set_edit",
                json!({"id": id, "path": "link/x.py", "text": "x"}),
            )
            .await;
        assert!(is_error(&r), "a link out of the workspace: {r}");
        let _ = std::fs::remove_dir_all(&outside);
    }
    let r = client
        .call("change_set_status", json!({"id": "../../etc/passwd"}))
        .await;
    assert!(is_error(&r));
    // A file whose name commonly holds secrets: proposing needs `secret-read`.
    std::fs::write(root.join(".env"), "TOKEN=1\n").unwrap();
    let refused = client
        .call(
            "change_set_edit",
            json!({"id": id, "path": ".env", "text": "TOKEN=2\n"}),
        )
        .await;
    assert!(
        error_message(&refused).starts_with("withheld:"),
        "{refused}"
    );
    // A set that holds one anyway (proposed by an agent with the right): its diff is withheld
    // and applying it needs `secret-read`.
    let store = ferris_idea_vcs::change_set::Store::of(&root);
    let mut set = store.load(&id).unwrap();
    set.propose(
        ".env",
        Some("TOKEN=1\n".into()),
        ferris_idea_vcs::change_set::Edit::Text("TOKEN=2\n".into()),
    )
    .unwrap();
    store.save(&set).unwrap();
    let diff = json_text(&client.call("change_set_diff", json!({"id": id})).await);
    assert_eq!(diff["withheld"], json!([".env"]), "{diff}");
    assert!(!diff.to_string().contains("TOKEN=2"));
    let refused = client
        .call(
            "change_set_apply",
            json!({"id": id, "hunks": {".env": [1]}, "dry_run": false}),
        )
        .await;
    assert!(
        error_message(&refused).starts_with("withheld:"),
        "{refused}"
    );
    assert_eq!(read(&root, ".env"), "TOKEN=1\n");
}

#[tokio::test]
async fn records_the_worktree() {
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status();
    if !init.is_ok_and(|s| s.success()) {
        eprintln!("git is not available; skipped");
        return;
    }
    let begun = json_text(
        &client
            .call("change_set_begin", json!({"name": "In the main tree"}))
            .await,
    );
    let worktree = &begun["worktree"];
    assert_eq!(worktree["linked"], false, "{begun}");
    assert_eq!(
        worktree["root"].as_str().unwrap(),
        root.canonicalize().unwrap().to_string_lossy()
    );
    assert!(
        begun["notice"]
            .as_str()
            .unwrap()
            .contains("own linked worktree"),
        "{begun}"
    );
    let id = begun["id"].as_str().unwrap();
    let status = json_text(&client.call("change_set_status", json!({"id": id})).await);
    assert_eq!(status["worktree"], *worktree);
}
