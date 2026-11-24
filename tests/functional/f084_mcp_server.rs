//! F-084 · MCP server for LLM agents: agents use IDE features as tools (SPEC-0006 AK-1…AK-5,
//! SPEC-0053 v4, SPEC-0221 v2, SPEC-0261 v2, IMP-0002, IMP-0028).

use crate::harness::*;
use serde_json::json;

#[test]
fn stdio_and_http_offer_the_same_tools_and_answers() {
    let p = Project::reference("python-basic");
    let mut stdio = Ide::stdio(&p);
    let mut http = Ide::http(&p);
    let tools = stdio.tools();
    assert!(tools.len() >= 60, "{}", tools.len());
    assert_eq!(tools, http.tools());
    for (tool, args) in [
        ("workspace_info", json!({})),
        ("list_files", json!({"language": "python"})),
        (
            "read_file",
            json!({"path": "test_model.py", "start_line": 4, "end_line": 5}),
        ),
        ("search_in_files", json!({"pattern": "restock"})),
        ("folding_ranges", json!({"path": "src/inventory/model.py"})),
    ] {
        assert_eq!(stdio.ok(tool, args.clone()), http.ok(tool, args), "{tool}");
    }
}

#[test]
fn paths_outside_the_workspace_are_rejected() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    ide.refused(
        "read_file",
        json!({"path": "../config/agent-permissions.toml"}),
    );
    ide.refused("read_file", json!({"path": "/etc/hosts"}));
    ide.refused("read_file", json!({"path": "missing.py"}));
    let lines = ide.ok(
        "read_file",
        json!({"path": "test_model.py", "start_line": 1, "end_line": 1}),
    );
    assert!(
        text(&lines).contains("from src.inventory.model import"),
        "{lines}"
    );
}

#[test]
fn rights_come_only_from_the_users_settings_folder() {
    let p = Project::empty();
    // A grant file inside the workspace is ignored.
    p.write(
        "agent-permissions.toml",
        &format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"run\"]\n",
            p.root.display()
        ),
    );
    let mut ide = Ide::stdio(&p);
    let rights = ide.ok("agent_permissions", json!({}));
    assert!(!text(&rights["allow"]).contains("run"), "{rights}");
    let reason = ide.refused(
        "run_command",
        json!({"command": ["true"], "dry_run": false}),
    );
    // The refusal names the right, the grant file and the entry to add.
    assert!(
        reason.contains("`run`") && reason.contains(&p.config.display().to_string()),
        "{reason}"
    );
    drop(ide);
    p.grant(&["run"]);
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "run_command",
        json!({"command": ["sh", "-c", "echo ran"], "dry_run": false}),
    );
    assert_eq!(r["stdout"], "ran\n");
}

/// A stand-in for the window: reports `pending` work, allows each asked right once and records how
/// it was asked to quit.
#[derive(Default)]
struct StandInWindow {
    pending: std::sync::Mutex<ferris_idea_mcp::window::PendingWork>,
    quits: std::sync::Mutex<Vec<ferris_idea_mcp::window::QuitMode>>,
}

impl ferris_idea_mcp::window::Window for StandInWindow {
    fn ask(
        &self,
        _request: ferris_idea_mcp::window::PermissionRequest,
        reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PermissionAnswer>,
    ) {
        let _ = reply.send(ferris_idea_mcp::window::PermissionAnswer::AllowOnce);
    }
    fn pending_work(
        &self,
        reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PendingWork>,
    ) {
        let _ = reply.send(self.pending.lock().unwrap().clone());
    }
    fn quit(
        &self,
        mode: ferris_idea_mcp::window::QuitMode,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    ) {
        self.quits.lock().unwrap().push(mode);
        let _ = reply.send(Ok(()));
    }
}

/// SPEC-0261 v2 AK-4: `ferris-idea quit` finds the running window through the discovery file, quits
/// it when nothing is pending, lists pending work without a terminal (status 2) and quits
/// discarding with `--discard`; without a running window it says so (status 1).
#[test]
fn ferris_idea_quit_ends_the_running_window() {
    use ferris_idea_mcp::window::{PendingWork, QuitMode};
    let p = Project::empty();
    let none = p.cli(&["quit"]);
    assert_eq!(none.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&none.stderr).contains("no running Ferris IDEA window"),
        "{none:?}"
    );

    let window = std::sync::Arc::new(StandInWindow::default());
    let server = ferris_idea_mcp::http::HttpServer::start(Some(&p.config)).unwrap();
    let w: std::sync::Arc<dyn ferris_idea_mcp::window::Window> = window.clone();
    server
        .add_workspace(
            ferris_idea_mcp::IdeServer::new(ferris_idea_core::Workspace::open(&p.root).unwrap())
                .with_window(w),
        )
        .unwrap();

    let quit = p.cli(&["quit"]);
    assert_eq!(quit.status.code(), Some(0), "{quit:?}");
    assert!(String::from_utf8_lossy(&quit.stdout).contains("quit"));
    assert_eq!(*window.quits.lock().unwrap(), [QuitMode::IfNothingPending]);

    *window.pending.lock().unwrap() = PendingWork {
        unsaved: vec!["src/main.rs".into()],
        running: vec!["ws: run configuration \"server\"".into()],
    };
    let asked = p.cli(&["quit"]);
    assert_eq!(asked.status.code(), Some(2), "{asked:?}");
    let out = String::from_utf8_lossy(&asked.stdout);
    assert!(
        out.contains("src/main.rs") && out.contains("run configuration"),
        "{out}"
    );
    assert!(String::from_utf8_lossy(&asked.stderr).contains("--discard"));
    assert_eq!(window.quits.lock().unwrap().len(), 1);

    let discarded = p.cli(&["quit", "--discard"]);
    assert_eq!(discarded.status.code(), Some(0), "{discarded:?}");
    assert_eq!(
        *window.quits.lock().unwrap(),
        [QuitMode::IfNothingPending, QuitMode::Discard]
    );
    server.stop();
}

#[test]
fn the_cli_prints_its_version_and_usage() {
    let p = Project::empty();
    let v = p.cli(&["--version"]);
    assert!(String::from_utf8_lossy(&v.stdout).starts_with("ferris-idea "));
    let u = p.cli(&["no-such-command"]);
    assert_eq!(u.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&u.stderr).contains("ferris-idea mcp <directory>"));
}
