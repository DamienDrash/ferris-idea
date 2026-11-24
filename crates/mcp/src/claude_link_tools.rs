//! The MCP tool `claude_code_link` (`SPEC-0358` behaviour 23, ADR-0008): the state of the link
//! Claude Code connects to when it runs in a Ferris IDEA terminal — listening, port, connected
//! clients, waiting diffs, the lock file — never the token. Read only. Its own tool router and rows
//! of the effects table: `build.rs` finds both (ADR-0035).

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::json;

use crate::effects::{Dry, ToolEffect, tool};
use crate::{IdeServer, json_result};

/// The row of `claude_code_link` in the effects table: it changes nothing.
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("claude_code_link", &[], Dry::None, &[], &[]),
];

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("claude_code_link", crate::groups::Group::Ide),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25): the
/// client's name and version are what Claude Code said of itself.
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] =
    &[("claude_code_link", &[crate::untrusted::Source::Agents])];

/// Arguments of `claude_code_link`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaudeCodeLinkArgs {}

#[tool_router(router = claude_link_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "State of the Claude Code link of this project: when Anthropic's `claude` CLI runs in a Ferris IDEA terminal it connects to the window as its IDE (selection, open files, diagnostics, diffs the user accepts or rejects). Answers `running`, `listening`, the `port` on 127.0.0.1, the connected `clients` (name, version, pid, seconds connected), `pending_diffs` and the `lock_file`; never the token. The link starts with the project's first terminal. In a server without the window (stdio) it lists the running Ferris IDEA links of this project found in the lock folder (port and pid only). Read only."
    )]
    async fn claude_code_link(
        &self,
        Parameters(_args): Parameters<ClaudeCodeLinkArgs>,
    ) -> Result<CallToolResult, McpError> {
        let root = self.canonical_root()?;
        if ferris_idea_claude_link::installed() {
            return json_result(match ferris_idea_claude_link::state(&root) {
                Some(state) => json!({
                    "in_this_process": true,
                    "running": true,
                    "listening": state.listening,
                    "port": state.port,
                    "clients": state.clients,
                    "connected": !state.clients.is_empty(),
                    "pending_diffs": state.pending_diffs,
                    "lock_file": state.lock_file,
                }),
                None => json!({
                    "in_this_process": true,
                    "running": false,
                    "hint": "the link starts when a terminal of this project opens",
                }),
            });
        }
        let found = ferris_idea_claude_link::lockfile::default_lock_dir()
            .map(|dir| {
                ferris_idea_claude_link::lockfile::running_links(
                    &dir,
                    ferris_idea_claude_link::IDE_NAME,
                    &root,
                )
            })
            .unwrap_or_default();
        json_result(json!({
            "in_this_process": false,
            "running": !found.is_empty(),
            "links": found
                .iter()
                .map(|(port, pid)| json!({"port": port, "pid": pid}))
                .collect::<Vec<_>>(),
            "hint": "connected clients are reported by the window's MCP server over HTTP",
        }))
    }
}
