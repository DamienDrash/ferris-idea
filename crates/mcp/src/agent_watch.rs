//! The watchdog tools `agent_watch_status` and `agent_watch_configure` (`SPEC-0341` behaviours 17
//! and 18, ROADMAP M19.12): the configuration and saved state of `ferris_idea_core::agent_watch` over
//! MCP. Configuring needs the `settings` right; the wake prompt is never set here. Their own tool
//! router and their own rows of the effects table ([`TOOL_EFFECTS`]): `build.rs` finds both
//! (ADR-0035).

use std::path::PathBuf;

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use ferris_idea_core::agent_watch::{
    self as watch, AgentLogFile, Phase, WatchConfig, Watchdog, load_watch_config, load_watch_state,
    save_watch_config, save_watch_state,
};

use crate::effects::{Change, Dry, SETTINGS, ToolEffect, tool};
use crate::{IdeServer, json_result, unix_time};

/// The rows of these tools in the effects table (`SPEC-0053` v8 behaviour 13).
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("agent_watch_configure", &[Change::Settings], Dry::DryRun(true), &[], &[SETTINGS]),
    tool("agent_watch_status", &[], Dry::None, &[], &[]),
];

/// What `agent_watch_status` says while no agent host is attached (behaviour 17).
const NO_HOST: &str = "none: Ferris IDEA hosts no agents yet (ROADMAP M19.5); the configuration and the saved state are kept, and the watchdog acts once a host is attached";

/// Arguments of `agent_watch_configure`. Unknown arguments, `wake_prompt` among them, are refused.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentWatchConfigureArgs {
    /// Switch the watchdog on or off for this workspace.
    pub enabled: Option<bool>,
    /// Seconds without progress before a working agent counts as hung (60 to 86400).
    pub stall_after: Option<u64>,
    /// Seconds without progress before an idle agent counts as asleep (60 to 86400).
    pub idle_after: Option<u64>,
    /// Seconds to wait after a wake or resume before judging again (60 to 86400).
    pub grace: Option<u64>,
    /// Seconds in the unknown state before the person gets a notice (60 to 86400).
    pub unknown_after: Option<u64>,
    /// Wakes and resumes in a row without progress before escalating (1 to 10).
    pub max_attempts: Option<u32>,
    /// Wakes and resumes of an agent in all before escalating for good (1 to 50).
    pub max_actions: Option<u32>,
    /// Spending in cents at which no more actions are taken.
    pub budget_cents: Option<u64>,
    /// Remove the budget.
    pub no_budget: Option<bool>,
    /// Resume the saved session of an agent whose process is gone.
    pub resume: Option<bool>,
    /// Agent ids whose escalation the person acknowledges.
    #[serde(default)]
    pub acknowledge: Vec<String>,
    /// Only show what would be written (default true); false writes and needs the `settings`
    /// right.
    pub dry_run: Option<bool>,
}

impl IdeServer {
    /// The watchdog's configuration and state files beside the grant file (`SPEC-0341`
    /// behaviours 14 and 16).
    fn watch_files(&self) -> Result<(PathBuf, PathBuf), McpError> {
        let grant_file = self.permissions.file.as_ref().ok_or_else(|| {
            McpError::invalid_params(
                "no settings folder is known, so the watchdog has no configuration",
                None,
            )
        })?;
        Ok(watch::watch_files(grant_file))
    }
}

/// The configuration as answered: the prompt's length, never its text (behaviour 17).
fn shown(config: &WatchConfig) -> Value {
    let mut v = serde_json::to_value(config).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.remove("wake_prompt");
    }
    v["wake_prompt_set"] = json!(config.wake_prompt.is_some());
    v["wake_prompt_chars"] = json!(config.wake_prompt.as_ref().map(|p| p.chars().count()));
    v
}

#[tool_router(router = agent_watch_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "The agent watchdog of this workspace, read only: `config` (thresholds in seconds, limits, budget, resume, whether a wake prompt is set, never its text), `config_file` and `state_file` (in the user's settings folder), `problems`, per-agent records (phase, last state and progress, attempts, actions, escalation reason), `bounds` and `host`. The watchdog wakes hung or sleeping agents with the user's prompt, resumes dead ones without wider rights, never nudges agents waiting for approval or in unknown state, and escalates at its limits. Needs no right."
    )]
    async fn agent_watch_status(&self) -> Result<CallToolResult, McpError> {
        let (file, state_file) = self.watch_files()?;
        let root = self.workspace.root();
        let loaded = load_watch_config(&file, root);
        let (records, state_problem) = load_watch_state(&state_file, root);
        let problems: Vec<String> = [loaded.problem.clone(), state_problem]
            .into_iter()
            .flatten()
            .collect();
        json_result(json!({
            "enabled": loaded.config.enabled,
            "config": shown(&loaded.config),
            "config_file": file,
            "state_file": state_file,
            "problems": problems,
            "agents": records,
            "escalated": records.iter().filter(|(_, r)| r.phase == Phase::Escalated).map(|(id, _)| id).collect::<Vec<_>>(),
            "bounds": {
                "times": watch::TIME_BOUNDS,
                "max_attempts": watch::ATTEMPT_BOUNDS,
                "max_actions": watch::ACTION_BOUNDS,
                "wake_prompt_chars": watch::PROMPT_MAX_CHARS,
            },
            "host": NO_HOST,
        }))
    }

    #[tool(
        description = "Change the agent watchdog of this workspace; `acknowledge` resets the attempts in a row of acknowledged escalations. Dry run by default, answering the configuration it would write; dry_run=false needs the user-granted `settings` right, writes `agent-watch.toml` in the user's settings folder and is logged. The wake prompt cannot be set here. Out-of-bounds values, a faulty file and unknown agent ids are refused."
    )]
    async fn agent_watch_configure(
        &self,
        Parameters(args): Parameters<AgentWatchConfigureArgs>,
    ) -> Result<CallToolResult, McpError> {
        let invalid = |m: String| McpError::invalid_params(m, None);
        let (file, state_file) = self.watch_files()?;
        let root = self.workspace.root().to_path_buf();
        let loaded = load_watch_config(&file, &root);
        if let Some(p) = &loaded.problem {
            return Err(invalid(format!(
                "{}: {p}; nothing was changed, fix the file first",
                file.display()
            )));
        }
        let mut config = loaded.config.clone();
        let set = |slot: &mut u64, v: Option<u64>| {
            if let Some(v) = v {
                *slot = v;
            }
        };
        set(&mut config.stall_after, args.stall_after);
        set(&mut config.idle_after, args.idle_after);
        set(&mut config.grace, args.grace);
        set(&mut config.unknown_after, args.unknown_after);
        if let Some(v) = args.enabled {
            config.enabled = v;
        }
        if let Some(v) = args.max_attempts {
            config.max_attempts = v;
        }
        if let Some(v) = args.max_actions {
            config.max_actions = v;
        }
        if let Some(v) = args.resume {
            config.resume = v;
        }
        match (args.budget_cents, args.no_budget.unwrap_or(false)) {
            (Some(_), true) => {
                return Err(invalid(
                    "`budget_cents` and `no_budget` contradict each other".into(),
                ));
            }
            (Some(b), false) => config.budget_cents = Some(b),
            (None, true) => config.budget_cents = None,
            (None, false) => {}
        }
        config.check().map_err(invalid)?;
        let (records, state_problem) = load_watch_state(&state_file, &root);
        let mut acknowledged = Vec::new();
        let mut not_escalated = Vec::new();
        for id in &args.acknowledge {
            match records.get(id) {
                None => {
                    return Err(invalid(format!(
                        "the watchdog has no record of an agent `{id}`"
                    )));
                }
                Some(r) if r.phase == Phase::Escalated => acknowledged.push(id.clone()),
                Some(_) => not_escalated.push(id.clone()),
            }
        }
        let dry_run = args.dry_run.unwrap_or(true);
        let answer = json!({
            "dry_run": dry_run,
            "config": shown(&config),
            "changed": config != loaded.config,
            "acknowledged": acknowledged,
            "not_escalated": not_escalated,
            "config_file": file,
            "state_problem": state_problem,
        });
        if dry_run {
            return json_result(answer);
        }
        self.require(ferris_idea_core::Grant::Settings).await?;
        if config != loaded.config {
            save_watch_config(&file, &root, &config)
                .map_err(|e| McpError::internal_error(e, None))?;
        }
        if !acknowledged.is_empty() {
            let mut dog = Watchdog::new(config, Default::default(), records);
            let mut log = AgentLogFile { root: root.clone() };
            let now = unix_time();
            for id in &acknowledged {
                dog.acknowledge(id, now, &mut log)
                    .map_err(|e| McpError::internal_error(e, None))?;
            }
            save_watch_state(&state_file, &root, &dog.records)
                .map_err(|e| McpError::internal_error(e, None))?;
        }
        json_result(answer)
    }
}

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("agent_watch_configure", crate::groups::Group::Ide),
    ("agent_watch_status", crate::groups::Group::Ide),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25).
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] = &[
    ("agent_watch_configure", &[]),
    ("agent_watch_status", &[crate::untrusted::Source::Agents]),
];
