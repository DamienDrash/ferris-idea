//! The tool `sandbox_policy` (`SPEC-0347` behaviour 13, ROADMAP M19.9): shows and checks a
//! sandbox policy for agent processes without starting anything. Its own tool router, joined to
//! the server's in [`IdeServer::all_tools`], and its rows of the effects table
//! ([`TOOL_EFFECTS`]): `build.rs` finds both (ADR-0035).

use std::path::{Path, PathBuf};

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use ferris_idea_sandbox::{
    Mechanism, Policy, ValidPolicy, availability, fixed_locations, seatbelt,
};

use crate::effects::{Dry, ToolEffect, tool};
use crate::{IdeServer, json_result};

/// The row of `sandbox_policy` in the effects table (`SPEC-0053` v8 behaviour 13): it only reads.
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("sandbox_policy", &[], Dry::None, &[], &[]),
];

/// Arguments of `sandbox_policy`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SandboxPolicyArgs {
    /// Folders or files the process may read; relative paths are taken below the workspace root.
    pub read: Option<Vec<String>>,
    /// Folders or files the process may read and change; relative paths are taken below the
    /// workspace root. Default (when no argument is given): the workspace root.
    pub write: Option<Vec<String>>,
    /// Destinations the process may reach: `name`, `name:port`, `*.domain[:port]`, an IPv4
    /// address or `[IPv6]`, port 443 by default.
    pub hosts: Option<Vec<String>>,
}

fn below(root: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[tool_router(router = sandbox_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "Check a sandbox policy for agent processes without starting anything: `read` and `write` roots (relative to the workspace) and `hosts` (port 443 by default). Without arguments, the default policy (workspace as the only write root, no network). Returns `valid`, `problems`, the resolved `policy`, this machine's `mechanism` and whether it is `available`, the `fixed_locations` every sandboxed process may read, and the macOS Seatbelt `profile`. Agents can never run unsandboxed. Needs no right; changes nothing."
    )]
    async fn sandbox_policy(
        &self,
        Parameters(args): Parameters<SandboxPolicyArgs>,
    ) -> Result<CallToolResult, McpError> {
        let root = self.workspace.root();
        let default = args.read.is_none() && args.write.is_none() && args.hosts.is_none();
        let paths = |list: Option<Vec<String>>| -> Vec<PathBuf> {
            list.unwrap_or_default()
                .iter()
                .map(|p| below(root, p))
                .collect()
        };
        let policy = Policy {
            read: paths(args.read),
            write: if default {
                vec![root.to_path_buf()]
            } else {
                paths(args.write)
            },
            hosts: args.hosts.unwrap_or_default(),
        };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let a = availability();
        let mut answer = json!({
            "default": default,
            "mechanism": a.mechanism,
            "available": a.available,
            "reason": a.reason,
            "fixed_locations": fixed_locations(),
        });
        match policy.validate(home.as_deref()) {
            Ok(valid) => {
                answer["valid"] = json!(true);
                answer["problems"] = json!([]);
                answer["policy"] = json!({
                    "read": valid.read(),
                    // Called by path, so the effects ratchet does not take the method call for a file write.
                    "write": ValidPolicy::write(&valid),
                    "hosts": valid.hosts().iter().map(ToString::to_string).collect::<Vec<_>>(),
                });
                if a.mechanism == Mechanism::MacosSeatbelt {
                    answer["profile"] = json!(seatbelt::profile(&valid, None));
                }
                if a.mechanism == Mechanism::LinuxLandlock && !valid.hosts().is_empty() {
                    answer["problems"] = json!([{
                        "entry": "hosts",
                        "message": "a policy with hosts is refused on Linux in this version (SPEC-0347 behaviour 10)",
                    }]);
                    answer["valid"] = json!(false);
                }
            }
            Err(problems) => {
                answer["valid"] = json!(false);
                answer["problems"] = json!(problems);
                answer["policy"] = Value::Null;
            }
        }
        json_result(answer)
    }
}

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("sandbox_policy", crate::groups::Group::Run),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25).
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] = &[("sandbox_policy", &[])];
