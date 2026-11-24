//! SPEC-0353 AK-5 to AK-8: the internal vault over MCP — handles and never values, the `vault`
//! right, the question in the window before a use, redaction of the program's output, the
//! binding of a database connection, and removal. Every vault here lies in a temporary folder
//! with an in-memory key store; no test touches the system's key store. Values are generated
//! dummies, never real credentials.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ferris_idea_core::Grant;
use ferris_idea_mcp::window::{PermissionAnswer, PermissionRequest, Scope, Window};
use ferris_idea_vault::{KeyStore, MemoryKeyStore};
use serde_json::{Value, json};

use super::protocol::{connect_exact, error_message, is_error, json_text, text};

/// A window that answers from a script; without a scripted answer the question goes unanswered.
#[derive(Default)]
struct ScriptedWindow {
    answers: Mutex<VecDeque<PermissionAnswer>>,
    asked: Mutex<Vec<PermissionRequest>>,
}

impl ScriptedWindow {
    fn answer(&self, a: PermissionAnswer) {
        self.answers.lock().unwrap().push_back(a);
    }
    fn asked(&self) -> Vec<PermissionRequest> {
        self.asked.lock().unwrap().clone()
    }
}

impl Window for ScriptedWindow {
    fn ask(
        &self,
        request: PermissionRequest,
        reply: tokio::sync::oneshot::Sender<PermissionAnswer>,
    ) {
        self.asked.lock().unwrap().push(request);
        if let Some(a) = self.answers.lock().unwrap().pop_front() {
            let _ = reply.send(a);
        }
    }
    fn pending_work(&self, _: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PendingWork>) {}
    fn quit(
        &self,
        _: ferris_idea_mcp::window::QuitMode,
        _: tokio::sync::oneshot::Sender<Result<(), String>>,
    ) {
    }
}

/// The vault folder of a test workspace: beside it, never inside.
fn vault_dir(root: &Path) -> PathBuf {
    root.with_extension("vault")
}

/// A value made up for the test (not a credential of anything).
fn dummy(tag: &str) -> String {
    format!("dummy-{tag}-value-0123456789")
}

async fn connect_vault(
    grants: &[Grant],
    window: Option<Arc<ScriptedWindow>>,
    keys: Arc<MemoryKeyStore>,
) -> (super::protocol::Client, PathBuf) {
    connect_exact(grants, move |server, root| {
        let server = server.with_vault(vault_dir(root), keys as Arc<dyn KeyStore>);
        match window {
            Some(w) => server.with_window(w as Arc<dyn Window>),
            None => server,
        }
    })
    .await
}

fn audit_results(audit: &Value) -> Vec<String> {
    audit["records"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .map(|r| r["result"].as_str().unwrap().to_owned())
        .collect()
}

/// SPEC-0353 AK-5: handles and metadata, never a value; bounds; the `vault` right.
#[tokio::test]
async fn agents_store_and_generate_but_never_read_back() {
    let keys = Arc::new(MemoryKeyStore::default());
    let (mut client, _) = connect_vault(&[], None, keys.clone()).await;
    for (tool, args) in [
        ("vault_list", json!({})),
        ("vault_audit", json!({})),
        (
            "vault_store",
            json!({"name": "a", "value": dummy("a"), "uses": ["process-env"]}),
        ),
        (
            "vault_generate",
            json!({"name": "b", "uses": ["process-env"]}),
        ),
        (
            "vault_remove",
            json!({"handle": "vlt_aaaaaaaaaaaaaaaaaaaaaaaaaa"}),
        ),
    ] {
        let refused = client.call(tool, args).await;
        assert!(is_error(&refused), "{tool}: {refused}");
        assert!(
            error_message(&refused).contains("not granted"),
            "{tool}: {refused}"
        );
    }

    let (mut client, root) = connect_vault(&[Grant::Vault], None, keys).await;
    let value = dummy("store");
    let stored = client
        .call(
            "vault_store",
            json!({"name": "api-token", "value": value, "kind": "token", "uses": ["process-env"]}),
        )
        .await;
    assert!(!is_error(&stored), "{stored}");
    assert!(
        !stored.to_string().contains(&value),
        "no value in the answer"
    );
    let stored = json_text(&stored);
    let handle = stored["handle"].as_str().unwrap().to_owned();
    assert!(handle.starts_with("vlt_") && handle.len() == 30, "{handle}");
    assert_eq!(stored["entry"]["seen_by_agent"], true);

    let generated = json_text(
        &client
            .call(
                "vault_generate",
                json!({"name": "db-pass", "uses": ["database"], "hosts": ["DB.Example.test:5432"], "length": 32}),
            )
            .await,
    );
    assert_eq!(generated["entry"]["seen_by_agent"], false);
    assert_eq!(generated["entry"]["hosts"], json!(["db.example.test:5432"]));

    let list = client.call("vault_list", json!({})).await;
    assert!(!list.to_string().contains(&value));
    let list = json_text(&list);
    let names: Vec<&str> = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["api-token", "db-pass"]);
    assert_eq!(list["key_store"], "memory (tests)");
    assert_eq!(list["elsewhere"], 0);

    for (args, why) in [
        (
            json!({"name": "api-token", "value": dummy("x"), "uses": ["process-env"]}),
            "exists",
        ),
        (
            json!({"name": "short", "value": "1234567", "uses": ["process-env"]}),
            "8 to 4096",
        ),
        (
            json!({"name": "db", "value": dummy("y"), "uses": ["database"]}),
            "names its hosts",
        ),
        (
            json!({"name": "w", "value": dummy("z"), "uses": ["database"], "hosts": ["*.example.test"]}),
            "no wildcards",
        ),
        (
            json!({"name": "u", "value": dummy("u"), "uses": ["http"]}),
            "no use",
        ),
    ] {
        let refused = client.call("vault_store", args).await;
        assert!(error_message(&refused).contains(why), "{why}: {refused}");
    }

    let audit = json_text(&client.call("vault_audit", json!({})).await);
    assert_eq!(audit["intact"], true);
    assert_eq!(audit_results(&audit), ["stored", "generated"]);

    // The agent log records the value by its length only (SPEC-0053 v7 behaviour 11), and the
    // vault file holds no value.
    let log = std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl")).unwrap();
    assert!(
        log.contains("vault_store") && !log.contains(&value),
        "{log}"
    );
    let file = std::fs::read_to_string(vault_dir(&root).join("vault.json")).unwrap();
    assert!(!file.contains(&value));
}

/// SPEC-0353 AK-6: a process environment is asked for on every use, the program's output that
/// prints the value comes back redacted, a denial holds for the session, and without a window
/// nothing is used.
#[tokio::test]
async fn process_environment_uses_ask_every_time_and_output_is_redacted() {
    let keys = Arc::new(MemoryKeyStore::default());
    let window = Arc::new(ScriptedWindow::default());
    let (mut client, root) =
        connect_vault(&[Grant::Vault, Grant::Run], Some(window.clone()), keys).await;
    let value = dummy("env");
    let handle = json_text(
        &client
            .call(
                "vault_store",
                json!({"name": "api-token", "value": value, "uses": ["process-env"]}),
            )
            .await,
    )["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let run = json!({
        "command": ["sh", "-c", "echo \"plain $API_TOKEN\"; printf %s \"$API_TOKEN\" | od -An -tx1 | tr -d ' \\n'; echo"],
        "env_from_vault": {"API_TOKEN": handle},
        "dry_run": false,
    });

    // The dry run names the variable and the handle and asks nobody.
    let dry = json_text(
        &client
            .call(
                "run_command",
                json!({"command": ["true"], "env_from_vault": {"API_TOKEN": handle}}),
            )
            .await,
    );
    assert_eq!(dry["would_run"]["env_from_vault"]["API_TOKEN"], handle);
    assert!(window.asked().is_empty());

    window.answer(PermissionAnswer::AllowOnce);
    let ran = client.call("run_command", run.clone()).await;
    assert!(!ran.to_string().contains(&value), "{ran}");
    let out = json_text(&ran);
    assert_eq!(out["success"], true, "{out}");
    let stdout = out["stdout"].as_str().unwrap();
    assert!(stdout.starts_with("plain «vault:api-token»"), "{stdout}");
    assert_eq!(
        stdout.matches("«vault:api-token»").count(),
        2,
        "raw and hex: {stdout}"
    );
    let asked = window.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].scope, Scope::Vault("api-token".into()));
    assert_eq!(asked[0].right, "vault");
    assert!(asked[0].meaning.contains("process-env") && asked[0].meaning.contains("asks again"));
    assert!(!format!("{:?}", asked[0]).contains(&value));
    assert_eq!(
        asked[0].answers()[2].0,
        "Allow for This Session",
        "the window offers the session grant"
    );

    // "Allow for This Session" counts once for a process environment: the next run asks again.
    window.answer(PermissionAnswer::Always);
    assert!(!is_error(&client.call("run_command", run.clone()).await));
    window.answer(PermissionAnswer::AllowOnce);
    assert!(!is_error(&client.call("run_command", run.clone()).await));
    assert_eq!(window.asked().len(), 3);

    // A denial refuses, and holds for the session without asking again.
    window.answer(PermissionAnswer::Deny);
    let denied = client.call("run_command", run.clone()).await;
    assert!(error_message(&denied).starts_with("denied:"), "{denied}");
    let denied = client.call("run_command", run.clone()).await;
    assert!(error_message(&denied).starts_with("denied:"), "{denied}");
    assert_eq!(window.asked().len(), 4);

    // A handle that is not for process environments is refused before anyone is asked.
    let db = json_text(
        &client
            .call(
                "vault_generate",
                json!({"name": "db", "uses": ["database"], "hosts": ["127.0.0.1"]}),
            )
            .await,
    )["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let refused = client
        .call(
            "run_command",
            json!({"command": ["true"], "env_from_vault": {"DB": db}, "dry_run": false}),
        )
        .await;
    assert!(error_message(&refused).starts_with("refused:"), "{refused}");
    assert_eq!(window.asked().len(), 4);

    let audit = json_text(&client.call("vault_audit", json!({})).await);
    assert_eq!(audit["intact"], true);
    assert_eq!(
        audit_results(&audit),
        [
            "stored",
            "used",
            "used",
            "used",
            "denied",
            "denied",
            "generated",
            "refused"
        ]
    );
    let first_use = &audit["records"].as_array().unwrap()[6];
    assert_eq!(first_use["form"], "process-env");
    assert_eq!(first_use["tool"], "run_command");
    assert_eq!(first_use["approval"], "window: once");
    let raw = std::fs::read_to_string(vault_dir(&root).join("audit.jsonl")).unwrap();
    assert!(!raw.contains(&value));

    // Without a window, nobody can allow the use: refused, and recorded (a new workspace, so a
    // vault of its own with a key store of its own).
    let (mut client, _) = connect_vault(&[Grant::Vault, Grant::Run], None, Arc::default()).await;
    let handle = json_text(
        &client
            .call(
                "vault_generate",
                json!({"name": "t", "uses": ["process-env"]}),
            )
            .await,
    )["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let refused = client
        .call(
            "run_command",
            json!({"command": ["true"], "env_from_vault": {"T": handle}, "dry_run": false}),
        )
        .await;
    assert!(
        error_message(&refused).starts_with("not granted:"),
        "{refused}"
    );
    let audit = json_text(&client.call("vault_audit", json!({})).await);
    assert_eq!(audit_results(&audit), ["generated", "unanswered"]);
}

/// SPEC-0353 AK-7: a connection is bound to a vault entry only for a matching host and port,
/// and an agent's connection asks before the value reaches the driver.
#[tokio::test]
async fn a_connection_takes_a_vault_entry_only_for_its_host() {
    let keys = Arc::new(MemoryKeyStore::default());
    let window = Arc::new(ScriptedWindow::default());
    let (mut client, root) =
        connect_vault(&[Grant::Vault, Grant::Write], Some(window.clone()), keys).await;
    let handle = json_text(
        &client
            .call(
                "vault_generate",
                json!({"name": "pg-agent", "uses": ["database"], "hosts": ["127.0.0.1:9"]}),
            )
            .await,
    )["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let connection = |port: u16| {
        json!({"name": "pg", "dialect": "postgresql", "host": "127.0.0.1", "port": port,
               "database": "shop", "user": "agent", "ssl": "disable", "vault": handle, "dry_run": false})
    };
    let refused = client.call("save_connection", connection(5432)).await;
    assert!(
        error_message(&refused).contains("not for 127.0.0.1:5432"),
        "{refused}"
    );
    assert!(
        !root.join(".ferris-idea/connections.toml").exists(),
        "nothing written"
    );

    let saved = json_text(&client.call("save_connection", connection(9)).await);
    assert_eq!(saved["vault"], handle);
    let file = std::fs::read_to_string(root.join(".ferris-idea/connections.toml")).unwrap();
    assert!(
        file.contains("ferris-idea-vault") && file.contains(&handle),
        "{file}"
    );

    // Connecting asks the window; without an answer nothing reaches the driver.
    let id = saved["id"].as_str().unwrap().to_owned();
    let test = client
        .call("test_connection", json!({"connection": id}))
        .await;
    assert!(error_message(&test).starts_with("not granted:"), "{test}");
    let asked = window.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].meaning.contains("127.0.0.1:9"));

    // Allowed: the value goes to the driver (nothing listens on port 9, so the test fails), and
    // the use is recorded with its destination.
    window.answer(PermissionAnswer::AllowOnce);
    let test = client
        .call("test_connection", json!({"connection": id}))
        .await;
    assert!(!is_error(&test), "{test}");
    assert_eq!(json_text(&test)["ok"], false);
    let audit = json_text(&client.call("vault_audit", json!({})).await);
    let last = &audit["records"][0];
    assert_eq!(last["result"], "used");
    assert_eq!(last["form"], "database");
    assert_eq!(last["destination"], "127.0.0.1:9");
    assert_eq!(last["tool"], "test_connection");
}

/// SPEC-0353 AK-8: an entry this session created and never used goes without asking; one
/// another session created asks the window and, unanswered, stays.
#[tokio::test]
async fn removal_asks_unless_the_entry_is_this_sessions_own() {
    let keys = Arc::new(MemoryKeyStore::default());
    let window = Arc::new(ScriptedWindow::default());
    let (mut client, root) =
        connect_vault(&[Grant::Vault], Some(window.clone()), keys.clone()).await;
    let own = json_text(
        &client
            .call(
                "vault_generate",
                json!({"name": "mine", "uses": ["process-env"]}),
            )
            .await,
    )["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let removed = json_text(&client.call("vault_remove", json!({"handle": own})).await);
    assert_eq!(removed["approval"], "own entry");
    assert!(window.asked().is_empty());

    // An entry another session stored in this project.
    let vault = ferris_idea_vault::Vault::new(vault_dir(&root), keys as Arc<dyn KeyStore>);
    let project = std::fs::canonicalize(&root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let other = vault
        .put(
            ferris_idea_vault::NewEntry {
                name: "theirs".into(),
                kind: ferris_idea_vault::Kind::Token,
                scope: ferris_idea_vault::Scope {
                    projects: vec![project],
                    uses: vec![ferris_idea_vault::Use::ProcessEnv],
                    hosts: vec![],
                },
                created_by: ferris_idea_vault::Agent {
                    session: "another".into(),
                    client: "other client".into(),
                },
                expires_in_days: None,
            },
            &dummy("theirs"),
            1,
        )
        .unwrap();
    let refused = client
        .call("vault_remove", json!({"handle": other.handle}))
        .await;
    assert!(
        error_message(&refused).starts_with("not granted:"),
        "{refused}"
    );
    assert_eq!(window.asked().len(), 1);
    assert_eq!(window.asked()[0].scope, Scope::Vault("theirs".into()));
    window.answer(PermissionAnswer::AllowOnce);
    let removed = json_text(
        &client
            .call("vault_remove", json!({"handle": other.handle}))
            .await,
    );
    assert_eq!(removed["approval"], "window");
    let list = json_text(&client.call("vault_list", json!({})).await);
    assert_eq!(list["entries"], json!([]));
    assert!(text(&client.call("vault_audit", json!({})).await).contains("removed"));
}
