//! `mcp.*` (`SPEC-0318` behaviour 29): Ferris IDEA's MCP tools for the host project, run on the host
//! with the host project's agent rights, permissions file and safe mode (`SPEC-0053`), never the
//! window's. `ferris-idea serve` passes the MCP server as a [`McpBridge`].

use std::path::Path;

use ferris_idea_session::{Backend, ErrorCode, Right, RpcError};
use serde_json::{Value, json};

use crate::{HostOptions, add, text};

/// Runs the host's MCP tools.
pub trait McpBridge: Send + Sync {
    /// The tools of the MCP server for `root`: name, description and input schema each.
    fn tools(&self, root: &Path) -> Result<Value, String>;
    /// Calls `tool` with `arguments` for `root`; `Err` is the tool's refusal or failure.
    fn call(&self, root: &Path, tool: &str, arguments: Value) -> Result<Value, String>;
}

/// Binds `mcp.*` when a bridge is given.
pub fn bind(backend: &Backend, options: &HostOptions) {
    let Some(bridge) = options.mcp.clone() else {
        return;
    };
    let b = bridge.clone();
    add(
        backend,
        "mcp",
        "mcp.tools",
        Right::Agents,
        false,
        move |call| {
            let tools = b
                .tools(&call.root)
                .map_err(|e| RpcError::new(ErrorCode::Internal, e))?;
            Ok(json!({ "tools": tools }))
        },
    );
    add(
        backend,
        "mcp",
        "mcp.call",
        Right::Agents,
        true,
        move |call| {
            let tool = text(&call.params, "tool")?;
            let arguments = match &call.params["arguments"] {
                Value::Null => json!({}),
                v @ Value::Object(_) => v.clone(),
                _ => return Err(crate::invalid("`arguments` must be an object")),
            };
            match bridge.call(&call.root, tool, arguments) {
                Ok(v) => Ok(json!({ "tool": tool, "result": v })),
                Err(e) => Err(RpcError::new(ErrorCode::Denied, e).hint(
                    "the tool refused or failed on the host, with the host project's rights",
                )),
            }
        },
    );
}
