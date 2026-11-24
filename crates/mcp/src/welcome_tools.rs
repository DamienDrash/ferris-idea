//! The MCP tool `welcome` (`SPEC-0355` behaviour 22): the Welcome window's recent projects for
//! agents — list them with their missing mark, remove one from the list, open one, and show the
//! window — and (v3) the section Remote Development › SSH: list, remove and open the recent SSH
//! projects, list the configured hosts. The folders lie outside the workspace, so every action that names them needs the
//! user-granted `recent-projects` right (`SPEC-0094` v9 behaviour 23). Its own tool router and
//! rows of the effects table ([`TOOL_EFFECTS`]): `build.rs` finds both (ADR-0035).

use std::path::{Path, PathBuf};

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use ferris_idea_core::{Grant, RecentEntry};

use crate::effects::{
    Change, Dry, RECENT_PROJECTS, RUN, Reach, SETTINGS, SSH, ToolEffect, UI, tool,
};
use crate::{IdeServer, json_result, unix_time};

/// The row of `welcome` in the effects table (`SPEC-0053` v8 behaviour 13): removing changes the
/// user's list (`settings`), opening and showing change the window (`ui`); naming a folder of the
/// list needs `recent-projects`; opening an SSH project (v3) connects and starts the backend on
/// the host (`ssh`, `run`), and listing the hosts runs `ssh -G`.
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("welcome", &[Change::Settings, Change::Ide, Change::Processes, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RECENT_PROJECTS, SETTINGS, UI, SSH, RUN]),
];

/// Arguments of `welcome`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WelcomeArgs {
    /// `list_recent` (default), `remove_recent`, `open_recent`, `show`, `list_ssh`, `remove_ssh`
    /// or `open_ssh`.
    pub action: Option<String>,
    /// For `remove_recent` and `open_recent`: the folder as `list_recent` names it.
    pub path: Option<String>,
    /// For `remove_ssh` and `open_ssh`: the host as `list_ssh` names it.
    pub host: Option<String>,
    /// For `remove_ssh` and `open_ssh`: the folder on the host as `list_ssh` names it.
    pub folder: Option<String>,
    /// For `remove_recent`, `open_recent`, `remove_ssh` and `open_ssh`: only name what would
    /// happen (default true).
    pub dry_run: Option<bool>,
}

fn invalid(m: impl Into<String>) -> McpError {
    McpError::invalid_params(m.into(), None)
}

/// The entry of `entries` that `path` names, by its canonical path or as written.
fn find<'a>(entries: &'a [RecentEntry], path: &str) -> Option<&'a RecentEntry> {
    let wanted =
        std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path.trim_end_matches('/')));
    entries.iter().find(|e| e.path == wanted)
}

/// One entry as the tool answers it.
fn entry_json(e: &RecentEntry) -> Value {
    let name = e
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| e.path.display().to_string());
    json!({
        "path": e.path, "name": name, "exists": e.exists,
        "branch": if e.exists { branch(&e.path) } else { None },
    })
}

/// Why `show`, `open_recent` and `open_ssh` are refused without a window.
const NO_WINDOW: &str = "no window: this MCP server runs without the Ferris IDEA window (stdio, `ferris-idea mcp`); use its MCP server over HTTP";

/// Whether `host` of a recent remote project is an SSH host, not a container, WSL or a paired
/// host (`SPEC-0355` v3 behaviour 12a).
fn is_ssh(host: &str) -> bool {
    matches!(
        ferris_idea_remote::HostSpec::parse(host),
        Ok(ferris_idea_remote::HostSpec::Ssh(_))
    )
}

/// One recent SSH project as the tool answers it.
fn ssh_json(r: &ferris_idea_remote::RecentRemote) -> Value {
    let name = r
        .folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(&r.folder);
    json!({ "host": r.host, "folder": r.folder, "name": name })
}

/// The branch checked out in `root`, read from `HEAD` (a worktree's `.git` file is followed).
fn branch(root: &Path) -> Option<String> {
    let git = root.join(".git");
    let dir = if git.is_file() {
        let text = std::fs::read_to_string(&git).ok()?;
        root.join(text.trim().strip_prefix("gitdir:")?.trim())
    } else {
        git
    };
    let head = std::fs::read_to_string(dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

impl IdeServer {
    /// The list with missing folders, after the `recent-projects` right is checked.
    async fn recent_entries(&self) -> Result<(Option<PathBuf>, Vec<RecentEntry>), McpError> {
        self.require(Grant::RecentProjects).await?;
        let file = self.recent_projects_file.clone();
        let entries = file
            .as_deref()
            .map(ferris_idea_core::load_recent_entries)
            .unwrap_or_default();
        Ok((file, entries))
    }

    /// The recent SSH projects (`SPEC-0355` v3 behaviour 12a), after the `recent-projects` right
    /// is checked.
    async fn recent_ssh(&self) -> Result<Vec<ferris_idea_remote::RecentRemote>, McpError> {
        self.require(Grant::RecentProjects).await?;
        Ok(self
            .recent_remote_file
            .as_deref()
            .map(ferris_idea_remote::load_recent_remote)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| is_ssh(&r.host))
            .collect())
    }

    /// `list_ssh`, `remove_ssh` and `open_ssh` (`SPEC-0355` v3 behaviour 22).
    async fn welcome_ssh_action(
        &self,
        action: &str,
        args: &WelcomeArgs,
        dry_run: bool,
        root: &Path,
    ) -> Result<CallToolResult, McpError> {
        if action == "list_ssh" {
            let recent = self.recent_ssh().await?;
            let extra = self.ssh_extra.clone();
            let hosts = tokio::task::spawn_blocking(move || ferris_idea_remote::host_list(&extra))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            self.log(
                root,
                json!({
                    "time": unix_time(), "tool": "welcome", "action": action,
                    "count": recent.len(), "hosts": hosts.len(),
                    "right": Grant::RecentProjects.name(),
                }),
            )?;
            let recent: Vec<Value> = recent.iter().map(ssh_json).collect();
            return json_result(json!({ "recent": recent, "hosts": hosts }));
        }
        let (Some(host), Some(folder)) = (args.host.as_deref(), args.folder.as_deref()) else {
            return Err(invalid(format!("`{action}` needs `host` and `folder`")));
        };
        let recent = self.recent_ssh().await?;
        let entry = recent
            .iter()
            .find(|r| r.host == host.trim() && r.folder == folder.trim())
            .cloned()
            .ok_or_else(|| {
                invalid(
                    "not among the recent SSH projects: list them with `list_ssh`; other folders open with `remote_project_open`",
                )
            })?;
        if dry_run {
            let needs: &[&str] = if action == "remove_ssh" {
                &["recent-projects", "settings"]
            } else {
                &["recent-projects", "ssh", "run", "ui"]
            };
            return json_result(json!({
                "dry_run": true, "action": action, "entry": ssh_json(&entry), "needs": needs,
                "note": "dry run: nothing changed; call with dry_run=false",
            }));
        }
        if action == "remove_ssh" {
            self.require(Grant::Settings).await?;
            let file = self
                .recent_remote_file
                .clone()
                .ok_or_else(|| invalid("no settings folder is known"))?;
            ferris_idea_remote::forget_remote(&file, &entry.host, &entry.folder)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            self.log(
                root,
                json!({"time": unix_time(), "tool": "welcome", "action": action, "removed": 1}),
            )?;
            return json_result(json!({"removed": true, "entry": ssh_json(&entry)}));
        }
        self.require(Grant::Ssh).await?;
        self.require(Grant::Run).await?;
        self.require(Grant::Ui).await?;
        let Some(window) = self.window.clone() else {
            return Err(invalid(NO_WINDOW));
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        window.welcome_ssh(entry.host.clone(), entry.folder.clone(), reply);
        let opened = answer
            .await
            .map_err(|_| McpError::internal_error("the window did not answer", None))?;
        self.log(
            root,
            json!({"time": unix_time(), "tool": "welcome", "action": action, "opened": u8::from(opened.is_ok())}),
        )?;
        json_result(opened.map_err(invalid)?)
    }

    /// Asks the window for `action`; refused when this server runs without one.
    async fn ask_welcome(&self, action: &str, path: Option<PathBuf>) -> Result<Value, McpError> {
        let Some(window) = self.window.clone() else {
            return Err(invalid(NO_WINDOW));
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        window.welcome(action.to_owned(), path, reply);
        answer
            .await
            .map_err(|_| McpError::internal_error("the window did not answer", None))?
            .map_err(invalid)
    }
}

#[tool_router(router = welcome_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "The Welcome window's recent projects. `list_recent` lists entries (newest first) with `exists` and Git branch; `remove_recent` drops one from the list (folder untouched); `open_recent` opens it in a project window; `show` shows the Welcome window. SSH: `list_ssh` lists recent SSH projects and configured hosts (nothing connects); `remove_ssh` and `open_ssh` act on that list. Listing, removing and opening need the `recent-projects` right; removals also `settings`, opening and `show` need `ui` and the window over HTTP, `open_ssh` also `ssh` and `run`. Changing actions are dry runs by default. Host keys must already be known."
    )]
    async fn welcome(
        &self,
        Parameters(args): Parameters<WelcomeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let action = args.action.as_deref().unwrap_or("list_recent");
        let dry_run = args.dry_run.unwrap_or(true);
        let root = self.workspace.root().to_path_buf();
        match action {
            "list_recent" => {
                let (_, entries) = self.recent_entries().await?;
                self.log(
                    &root,
                    json!({
                        "time": unix_time(), "tool": "welcome", "action": action,
                        "count": entries.len(), "right": Grant::RecentProjects.name(),
                    }),
                )?;
                json_result(entries.iter().map(entry_json).collect::<Vec<_>>())
            }
            "remove_recent" | "open_recent" => {
                let path = args
                    .path
                    .as_deref()
                    .ok_or_else(|| invalid(format!("`{action}` needs `path`")))?;
                let (file, entries) = self.recent_entries().await?;
                let entry = find(&entries, path)
                    .ok_or_else(|| {
                        invalid("not in the recent projects: list them with `list_recent`")
                    })?
                    .clone();
                if action == "open_recent" && !entry.exists {
                    return Err(invalid(format!(
                        "{} no longer exists; remove it with `remove_recent`",
                        entry.path.display()
                    )));
                }
                if dry_run {
                    return json_result(json!({
                        "dry_run": true, "action": action, "entry": entry_json(&entry),
                        "note": "dry run: nothing changed; call with dry_run=false",
                    }));
                }
                if action == "remove_recent" {
                    self.require(Grant::Settings).await?;
                    let file = file.ok_or_else(|| invalid("no settings folder is known"))?;
                    ferris_idea_core::forget_project(&file, &entry.path)
                        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                    self.log(
                        &root,
                        json!({"time": unix_time(), "tool": "welcome", "action": action, "removed": 1}),
                    )?;
                    return json_result(json!({"removed": true, "entry": entry_json(&entry)}));
                }
                self.require(Grant::Ui).await?;
                let answer = self.ask_welcome(action, Some(entry.path.clone())).await?;
                self.log(
                    &root,
                    json!({"time": unix_time(), "tool": "welcome", "action": action, "opened": 1}),
                )?;
                json_result(answer)
            }
            "show" => {
                self.require(Grant::Ui).await?;
                let answer = self.ask_welcome(action, None).await?;
                self.log(
                    &root,
                    json!({"time": unix_time(), "tool": "welcome", "action": action}),
                )?;
                json_result(answer)
            }
            "list_ssh" | "remove_ssh" | "open_ssh" => {
                self.welcome_ssh_action(action, &args, dry_run, &root).await
            }
            other => Err(invalid(format!(
                "unknown action `{other}` (list_recent, remove_recent, open_recent, show, list_ssh, remove_ssh, open_ssh)"
            ))),
        }
    }
}

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("welcome", crate::groups::Group::Ide),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25).
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] = &[("welcome", &[])];
