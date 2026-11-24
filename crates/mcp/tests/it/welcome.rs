//! SPEC-0355 AK-5: the MCP tool `welcome` — `list_recent` needs `recent-projects` and lists missing
//! folders too, `remove_recent` is a dry run first and needs `settings`, `open_recent` and `show`
//! need the window and `ui`; the log never holds a folder's path. AK-8 (v3): `list_ssh`,
//! `remove_ssh` and `open_ssh` with the test's own SSH configuration; nothing connects.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ferris_idea_core::Grant;
use serde_json::{Value, json};

use super::protocol::{connect_exact, error_message, is_error, json_text};

/// A window that notes what `welcome` asked of it.
#[derive(Default)]
struct WelcomeWindow(
    Mutex<Vec<(String, Option<PathBuf>)>>,
    Mutex<Vec<(String, String)>>,
);

impl ferris_idea_mcp::window::Window for WelcomeWindow {
    fn ask(
        &self,
        _request: ferris_idea_mcp::window::PermissionRequest,
        reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PermissionAnswer>,
    ) {
        let _ = reply.send(ferris_idea_mcp::window::PermissionAnswer::Deny);
    }
    fn pending_work(
        &self,
        reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PendingWork>,
    ) {
        let _ = reply.send(Default::default());
    }
    fn quit(
        &self,
        _mode: ferris_idea_mcp::window::QuitMode,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    ) {
        let _ = reply.send(Err("no".into()));
    }
    fn welcome(
        &self,
        action: String,
        path: Option<PathBuf>,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    ) {
        self.0.lock().unwrap().push((action.clone(), path));
        let _ = reply.send(Ok(json!({ "done": action })));
    }
    fn welcome_ssh(
        &self,
        host: String,
        folder: String,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    ) {
        self.1.lock().unwrap().push((host.clone(), folder));
        let _ = reply.send(Ok(json!({ "opened": host })));
    }
}

/// The agent log's lines the tool wrote itself, without the line every changing call gets with
/// the arguments the agent gave (`SPEC-0053` v7 behaviour 11).
fn log(root: &std::path::Path) -> String {
    std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| {
            serde_json::from_str::<Value>(l).map_or(true, |v| v["call"] != Value::Bool(true))
        })
        .map(|l| format!("{l}\n"))
        .collect()
}

/// A recent list beside the workspace with an existing and a missing project.
fn recent(root: &std::path::Path) -> (PathBuf, PathBuf) {
    let other = root.with_extension("welcome-other");
    std::fs::create_dir_all(&other).unwrap();
    let gone = root.with_extension("welcome-gone");
    std::fs::write(
        root.with_extension("recent-projects"),
        format!("{}\n{}\n", other.display(), gone.display()),
    )
    .unwrap();
    (other.canonicalize().unwrap(), gone)
}

#[tokio::test]
async fn list_recent_needs_its_right_and_lists_missing_folders() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    recent(&root);
    let refused = client.call("welcome", json!({})).await;
    assert!(is_error(&refused), "{refused}");
    let message = error_message(&refused);
    assert!(message.starts_with("not granted:"), "{message}");
    assert!(message.contains("`recent-projects`"), "{message}");
    assert!(!message.contains("welcome-other"), "{message}");
    drop(client);

    let (mut client, root) = connect_exact(&[Grant::RecentProjects], |s, _| s).await;
    let (other, gone) = recent(&root);
    std::fs::create_dir_all(other.join(".git")).unwrap();
    std::fs::write(other.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let listed = json_text(
        &client
            .call("welcome", json!({"action": "list_recent"}))
            .await,
    );
    let list = listed.as_array().unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(list.len(), 2, "{listed}");
    assert_eq!(list[0]["path"], json!(other));
    assert_eq!(list[0]["exists"], true);
    assert_eq!(list[0]["branch"], "main");
    assert_eq!(list[1]["path"], json!(gone));
    assert_eq!(
        list[1]["exists"], false,
        "the missing folder is listed, marked"
    );
    let log = log(&root);
    assert!(log.contains("\"tool\":\"welcome\""), "{log}");
    assert!(log.contains("\"count\":2"), "{log}");
    assert!(!log.contains("welcome-other"), "no path in the log: {log}");
}

#[tokio::test]
async fn remove_recent_is_a_dry_run_first_and_needs_settings() {
    let (mut client, root) = connect_exact(&[Grant::RecentProjects], |s, _| s).await;
    let (_, gone) = recent(&root);
    let file = root.with_extension("recent-projects");
    let path = gone.display().to_string();
    let dry = json_text(
        &client
            .call("welcome", json!({"action": "remove_recent", "path": path}))
            .await,
    );
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("welcome-gone")
    );
    let refused = client
        .call(
            "welcome",
            json!({"action": "remove_recent", "path": path, "dry_run": false}),
        )
        .await;
    assert!(error_message(&refused).contains("`settings`"), "{refused}");
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("welcome-gone")
    );
    let unknown = client
        .call(
            "welcome",
            json!({"action": "remove_recent", "path": "/nowhere/else"}),
        )
        .await;
    assert!(error_message(&unknown).contains("not in the recent projects"));
    drop(client);

    let (mut client, root) =
        connect_exact(&[Grant::RecentProjects, Grant::Settings], |s, _| s).await;
    let (other, gone) = recent(&root);
    let file = root.with_extension("recent-projects");
    let removed = json_text(
        &client
            .call(
                "welcome",
                json!({"action": "remove_recent", "path": gone.display().to_string(), "dry_run": false}),
            )
            .await,
    );
    assert_eq!(removed["removed"], true, "{removed}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!("{}\n", other.display())
    );
    assert!(
        other.is_dir(),
        "the folder of a listed project is never touched"
    );
    let log = log(&root);
    assert!(log.contains("\"removed\":1"), "{log}");
    assert!(!log.contains("welcome-gone"), "{log}");
}

#[tokio::test]
async fn open_recent_and_show_need_the_window_and_ui() {
    // Without a window (stdio): refused with "no window".
    let (mut stdio, root) = connect_exact(&[Grant::RecentProjects, Grant::Ui], |s, _| s).await;
    let (other, _) = recent(&root);
    let none = stdio.call("welcome", json!({"action": "show"})).await;
    assert!(error_message(&none).contains("no window"), "{none}");
    drop(stdio);

    let window = Arc::new(WelcomeWindow::default());
    let seen = window.clone();
    let (mut client, root) = connect_exact(&[Grant::RecentProjects], move |s, _| {
        let w: Arc<dyn ferris_idea_mcp::window::Window> = window;
        s.with_window(w)
    })
    .await;
    let (other2, gone) = recent(&root);
    let _ = other;
    let refused = client.call("welcome", json!({"action": "show"})).await;
    assert!(error_message(&refused).contains("`ui`"), "{refused}");
    let missing = client
        .call(
            "welcome",
            json!({"action": "open_recent", "path": gone.display().to_string(), "dry_run": false}),
        )
        .await;
    assert!(
        error_message(&missing).contains("no longer exists"),
        "{missing}"
    );
    let dry = json_text(
        &client
            .call(
                "welcome",
                json!({"action": "open_recent", "path": other2.display().to_string()}),
            )
            .await,
    );
    assert_eq!(dry["dry_run"], true, "{dry}");
    let refused = client
        .call(
            "welcome",
            json!({"action": "open_recent", "path": other2.display().to_string(), "dry_run": false}),
        )
        .await;
    assert!(error_message(&refused).contains("`ui`"), "{refused}");
    assert!(
        seen.0.lock().unwrap().is_empty(),
        "nothing reached the window"
    );
    drop(client);

    let window = Arc::new(WelcomeWindow::default());
    let seen = window.clone();
    let (mut client, root) = connect_exact(&[Grant::RecentProjects, Grant::Ui], move |s, _| {
        let w: Arc<dyn ferris_idea_mcp::window::Window> = window;
        s.with_window(w)
    })
    .await;
    let (other, _) = recent(&root);
    let shown = json_text(&client.call("welcome", json!({"action": "show"})).await);
    assert_eq!(shown["done"], "show", "{shown}");
    let opened = json_text(
        &client
            .call(
                "welcome",
                json!({"action": "open_recent", "path": other.display().to_string(), "dry_run": false}),
            )
            .await,
    );
    assert_eq!(opened["done"], "open_recent", "{opened}");
    assert_eq!(
        *seen.0.lock().unwrap(),
        [
            ("show".to_owned(), None),
            ("open_recent".to_owned(), Some(other))
        ]
    );
    let unknown = client.call("welcome", json!({"action": "fly"})).await;
    assert!(
        error_message(&unknown).contains("unknown action"),
        "{unknown}"
    );
}

/// The recent remote projects beside the workspace (two SSH projects and a container) and an SSH
/// configuration of the test's own with one host, given to every `ssh` with `-F`, so the user's is
/// never read. (`isolate_ssh` is not switched on here: it would hold for the whole test binary,
/// whose other tests plan without a configuration of their own.)
fn ssh_setup(root: &std::path::Path) -> Vec<String> {
    let file = root.with_extension("recent-remote-projects.json");
    for (host, folder) in [
        ("docker://web", "/app"),
        ("me@box:2222", "/home/me/site"),
        ("build", "/srv/shop"),
    ] {
        ferris_idea_remote::remember_remote(&file, host, folder).unwrap();
    }
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-welcome-ssh-{}",
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let config = dir.join("ssh_config");
    std::fs::write(
        &config,
        "Host staging\n  HostName 10.0.0.9\n  User deploy\n",
    )
    .unwrap();
    vec!["-F".into(), config.display().to_string()]
}

/// SPEC-0355 v3 AK-8: `list_ssh` needs `recent-projects`, lists the SSH projects (not the
/// container's) and the configured host, and logs counts only.
#[tokio::test]
async fn ssh_list_needs_its_right_and_lists_projects_and_hosts() {
    let (mut client, root) = connect_exact(&[], |s, root| {
        let extra = ssh_setup(root);
        s.with_ssh(extra)
    })
    .await;
    let refused = client.call("welcome", json!({"action": "list_ssh"})).await;
    assert!(
        error_message(&refused).contains("`recent-projects`"),
        "{refused}"
    );
    assert!(!error_message(&refused).contains("/srv/shop"), "{refused}");
    drop(client);
    let _ = root;

    let (mut client, root) = connect_exact(&[Grant::RecentProjects], |s, root| {
        let extra = ssh_setup(root);
        s.with_ssh(extra)
    })
    .await;
    let listed = json_text(&client.call("welcome", json!({"action": "list_ssh"})).await);
    let recent = listed["recent"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(recent.len(), 2, "{listed}");
    assert_eq!(recent[0]["host"], "build");
    assert_eq!(recent[0]["folder"], "/srv/shop");
    assert_eq!(recent[0]["name"], "shop");
    assert_eq!(recent[1]["host"], "me@box:2222");
    let hosts = listed["hosts"].as_array().unwrap();
    assert_eq!(hosts.len(), 1, "{listed}");
    assert_eq!(hosts[0]["alias"], "staging");
    assert_eq!(hosts[0]["hostname"], "10.0.0.9");
    assert_eq!(hosts[0]["user"], "deploy");
    let log = log(&root);
    assert!(log.contains("\"action\":\"list_ssh\""), "{log}");
    assert!(log.contains("\"count\":2"), "{log}");
    assert!(
        !log.contains("/srv/shop") && !log.contains("me@box"),
        "{log}"
    );
}

/// SPEC-0355 v3 AK-8: `remove_ssh` is a dry run first, needs `settings`, removes one entry and
/// logs no host or folder; an entry not in the list is refused.
#[tokio::test]
async fn ssh_remove_is_a_dry_run_first_and_needs_settings() {
    let (mut client, root) = connect_exact(&[Grant::RecentProjects], |s, root| {
        let extra = ssh_setup(root);
        s.with_ssh(extra)
    })
    .await;
    let file = root.with_extension("recent-remote-projects.json");
    let entry = json!({"action": "remove_ssh", "host": "build", "folder": "/srv/shop"});
    let dry = json_text(&client.call("welcome", entry.clone()).await);
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["needs"], json!(["recent-projects", "settings"]));
    let mut real = entry.clone();
    real["dry_run"] = json!(false);
    let refused = client.call("welcome", real.clone()).await;
    assert!(error_message(&refused).contains("`settings`"), "{refused}");
    assert_eq!(ferris_idea_remote::load_recent_remote(&file).len(), 3);
    let container = client
        .call(
            "welcome",
            json!({"action": "remove_ssh", "host": "docker://web", "folder": "/app"}),
        )
        .await;
    assert!(
        error_message(&container).contains("not among the recent SSH projects"),
        "{container}"
    );
    drop(client);

    let (mut client, root) = connect_exact(&[Grant::RecentProjects, Grant::Settings], |s, root| {
        let extra = ssh_setup(root);
        s.with_ssh(extra)
    })
    .await;
    let file = root.with_extension("recent-remote-projects.json");
    let removed = json_text(&client.call("welcome", real).await);
    assert_eq!(removed["removed"], true, "{removed}");
    let left: Vec<String> = ferris_idea_remote::load_recent_remote(&file)
        .into_iter()
        .map(|r| r.host)
        .collect();
    assert_eq!(left, ["me@box:2222", "docker://web"]);
    let log = log(&root);
    assert!(log.contains("\"removed\":1"), "{log}");
    assert!(!log.contains("/srv/shop"), "{log}");
}

/// SPEC-0355 v3 AK-8: `open_ssh` names the rights in its dry run, needs `ssh`, `run`, `ui` and
/// the window, and hands host and folder to it.
#[tokio::test]
async fn ssh_open_needs_the_window_and_its_rights() {
    let all = [Grant::RecentProjects, Grant::Ssh, Grant::Run, Grant::Ui];
    let (mut stdio, _) = connect_exact(&all, |s, root| {
        let extra = ssh_setup(root);
        s.with_ssh(extra)
    })
    .await;
    let entry = json!({"action": "open_ssh", "host": "me@box:2222", "folder": "/home/me/site"});
    let dry = json_text(&stdio.call("welcome", entry.clone()).await);
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["needs"], json!(["recent-projects", "ssh", "run", "ui"]));
    let mut real = entry.clone();
    real["dry_run"] = json!(false);
    let none = stdio.call("welcome", real.clone()).await;
    assert!(error_message(&none).contains("no window"), "{none}");
    drop(stdio);

    let window = Arc::new(WelcomeWindow::default());
    let seen = window.clone();
    let (mut client, _) = connect_exact(&[Grant::RecentProjects, Grant::Ui], move |s, root| {
        let extra = ssh_setup(root);
        let w: Arc<dyn ferris_idea_mcp::window::Window> = window;
        s.with_ssh(extra).with_window(w)
    })
    .await;
    let refused = client.call("welcome", real.clone()).await;
    assert!(error_message(&refused).contains("`ssh`"), "{refused}");
    assert!(
        seen.1.lock().unwrap().is_empty(),
        "nothing reached the window"
    );
    drop(client);

    let window = Arc::new(WelcomeWindow::default());
    let seen = window.clone();
    let (mut client, _) = connect_exact(&all, move |s, root| {
        let extra = ssh_setup(root);
        let w: Arc<dyn ferris_idea_mcp::window::Window> = window;
        s.with_ssh(extra).with_window(w)
    })
    .await;
    let opened = json_text(&client.call("welcome", real).await);
    assert_eq!(opened["opened"], "me@box:2222", "{opened}");
    assert_eq!(
        *seen.1.lock().unwrap(),
        [("me@box:2222".to_owned(), "/home/me/site".to_owned())]
    );
    let missing = client
        .call(
            "welcome",
            json!({"action": "open_ssh", "host": "me@box:2222", "dry_run": false}),
        )
        .await;
    assert!(
        error_message(&missing).contains("needs `host` and `folder`"),
        "{missing}"
    );
}
