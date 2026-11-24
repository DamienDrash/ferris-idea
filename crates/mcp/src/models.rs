//! The model tools `model_providers`, `model_provider_set`, `model_price_set`, `model_assign`,
//! `model_list`, `model_detect_local`, `model_usage` and `model_usage_record` (`SPEC-0350`
//! behaviours 15 to 18, ROADMAP M19.7): the provider register, the model of each task, local
//! servers and the costs booked from reported token counts. Changing needs the `settings` right;
//! no tool takes or answers a key. Their own tool router, joined in [`IdeServer::all_tools`].

use std::path::PathBuf;

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use ferris_idea_ai::providers::{
    self as prov, Booking, GroupBy, KeySource, Kind, Price, Provider, Register,
};
use ferris_idea_core::Grant;

use crate::effects::{Change, Dry, HTTP, Reach, SETTINGS, ToolEffect, tool};
use crate::{IdeServer, json_result, unix_time};

/// The rows of this module's tools in the effects table (`SPEC-0053`, ADR-0035).
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("model_assign", &[Change::Settings], Dry::DryRun(true), &[], &[SETTINGS]),
    tool("model_detect_local", &[], Dry::None, &[Reach::Network], &[]),
    tool("model_list", &[], Dry::None, &[Reach::Network], &[HTTP]),
    tool("model_price_set", &[Change::Settings], Dry::DryRun(true), &[], &[SETTINGS]),
    tool("model_provider_set", &[Change::Settings], Dry::DryRun(true), &[], &[SETTINGS]),
    tool("model_providers", &[], Dry::None, &[], &[]),
    tool("model_usage", &[], Dry::None, &[], &[]),
    tool("model_usage_record", &[Change::Settings], Dry::DryRun(true), &[], &[SETTINGS]),
];

/// Arguments of `model_provider_set`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderSetArgs {
    /// The provider id: 1 to 64 characters of a-z, 0-9, -, _.
    pub id: String,
    /// openai, anthropic, ollama or llamacpp (ollama and llamacpp are local only). Needed unless
    /// `remove`.
    pub kind: Option<String>,
    /// Base URL of the API: https, or http on 127.0.0.1, ::1 or localhost; no user, password,
    /// query or fragment. Needed unless `remove`.
    pub endpoint: Option<String>,
    /// `none` (default) or `keychain`: the system keychain's entry of the endpoint, as the AI
    /// Assistant's settings page stores it. A key itself is never given here.
    pub key: Option<String>,
    /// Known model names (at most 500); empty: not listed yet.
    #[serde(default)]
    pub models: Vec<String>,
    /// Whether tasks may use it (default true).
    pub enabled: Option<bool>,
    /// Remove the provider and its prices (refused while a task is assigned to it).
    #[serde(default)]
    pub remove: bool,
    /// Only show what would be written (default true); false writes and needs the `settings`
    /// right.
    pub dry_run: Option<bool>,
}

/// Arguments of `model_price_set`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelPriceSetArgs {
    /// The provider id.
    pub provider: String,
    /// The model, or `*` for every model of the provider.
    pub model: String,
    /// Three capital letters (default USD).
    pub currency: Option<String>,
    /// Price of a million input tokens, a decimal number as text (e.g. "2.50"). Needed unless
    /// `remove`.
    pub input_per_mtok: Option<String>,
    /// Price of a million output tokens. Needed unless `remove`.
    pub output_per_mtok: Option<String>,
    /// Remove the price row.
    #[serde(default)]
    pub remove: bool,
    /// Only show what would be written (default true); false needs the `settings` right.
    pub dry_run: Option<bool>,
}

/// Arguments of `model_assign`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelAssignArgs {
    /// The task: 1 to 64 characters of a-z, 0-9, -, _, .; `default` applies to every task
    /// without its own.
    pub task: String,
    /// The provider id. Needed unless `remove`.
    pub provider: Option<String>,
    /// The model. Needed unless `remove`.
    pub model: Option<String>,
    /// Remove the task's assignment.
    #[serde(default)]
    pub remove: bool,
    /// Only show what would be written (default true); false needs the `settings` right.
    pub dry_run: Option<bool>,
}

/// Arguments of `model_list`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelListArgs {
    /// The provider id.
    pub provider: String,
}

/// Arguments of `model_detect_local`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelDetectLocalArgs {
    /// Ports on 127.0.0.1 to ask (default 11434 and 8080, at most 16).
    #[serde(default)]
    pub ports: Vec<u16>,
}

/// Arguments of `model_usage`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelUsageArgs {
    /// provider (default), key, model or task.
    pub group_by: Option<String>,
    /// Only records at or after this time (seconds since the Unix epoch).
    pub since: Option<u64>,
}

/// Arguments of `model_usage_record`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelUsageRecordArgs {
    /// The provider id.
    pub provider: String,
    /// The model.
    pub model: String,
    /// The task (default `default`).
    pub task: Option<String>,
    /// Input tokens as the provider reported them (0 to 10^9).
    pub input_tokens: u64,
    /// Output tokens as the provider reported them (0 to 10^9).
    pub output_tokens: u64,
    /// Only answer the computed cost (default true); false books it and needs the `settings`
    /// right.
    pub dry_run: Option<bool>,
}

fn invalid(m: impl Into<String>) -> McpError {
    McpError::invalid_params(m.into(), None)
}

fn kind_named(name: &str) -> Result<Kind, McpError> {
    serde_json::from_value(json!(name)).map_err(|_| {
        invalid(format!(
            "`{name}` is no kind: openai, anthropic, ollama or llamacpp"
        ))
    })
}

fn key_named(name: &str) -> Result<KeySource, McpError> {
    if name.starts_with("vault:") {
        return Err(invalid(
            "vault references wait for the vault of ROADMAP M19.15; use `keychain`",
        ));
    }
    serde_json::from_value(json!(name)).map_err(|_| {
        invalid("`key` is `none` or `keychain`; a key itself is never given to a tool")
    })
}

/// A refusal that never repeats a secret-looking value.
fn refused(m: String) -> McpError {
    invalid(ferris_idea_ai::context::mask_secrets(&m).0)
}

fn provider_json(p: &Provider) -> Value {
    json!({
        "id": p.id,
        "kind": p.kind.name(),
        "endpoint": p.endpoint,
        "key": match p.key { KeySource::None => "none", KeySource::Keychain => "keychain" },
        "models": p.models,
        "enabled": p.enabled,
        "local": p.kind.local(),
    })
}

impl IdeServer {
    /// The register's and the ledger's files beside the grant file (`SPEC-0350` behaviour 1).
    fn model_files(&self) -> Result<(PathBuf, PathBuf), McpError> {
        let grant_file =
            self.permissions.file.as_ref().ok_or_else(|| {
                invalid("no settings folder is known, so there is no model register")
            })?;
        Ok(prov::files(grant_file))
    }

    /// The register for a change; refused while the file has a problem.
    fn register_for_change(&self) -> Result<(PathBuf, Register), McpError> {
        let (file, _) = self.model_files()?;
        let loaded = prov::load(&file, self.workspace.root());
        if let Some(p) = loaded.problem {
            return Err(invalid(format!(
                "{p}; nothing was changed, fix the file first"
            )));
        }
        Ok((file, loaded.register))
    }

    /// Writes `register` unless `dry_run` (default true); writing needs the `settings` right.
    async fn write_register(
        &self,
        file: &std::path::Path,
        register: &Register,
        dry_run: Option<bool>,
        mut answer: Value,
    ) -> Result<CallToolResult, McpError> {
        let dry_run = dry_run.unwrap_or(true);
        answer["dry_run"] = json!(dry_run);
        answer["register_file"] = json!(file);
        if dry_run {
            return json_result(answer);
        }
        self.require(Grant::Settings).await?;
        prov::save(file, self.workspace.root(), register)
            .map_err(|e| McpError::internal_error(e, None))?;
        json_result(answer)
    }
}

#[tool_router(router = models_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "The model providers for agents (SPEC-0350, ROADMAP M19.7), read only: `providers` (id, kind openai/anthropic/ollama/llamacpp, endpoint, key `none` or `keychain` — never a key, models, enabled, local), `prices` (per provider and model or `*`, currency, input_per_mtok and output_per_mtok; no price means the cost is unknown, nothing is built in), `assignments` (the model of each task with its earlier choices; `default` applies to tasks without their own), `register_file` and `ledger_file` (in the user's settings folder beside the grant file, never in the workspace), `problems` and `bounds`. Needs no right."
    )]
    async fn model_providers(&self) -> Result<CallToolResult, McpError> {
        let (file, ledger) = self.model_files()?;
        let loaded = prov::load(&file, self.workspace.root());
        let r = &loaded.register;
        json_result(json!({
            "providers": r.providers.iter().map(provider_json).collect::<Vec<_>>(),
            "prices": r.prices,
            "assignments": r.assignments,
            "register_file": file,
            "ledger_file": ledger,
            "problems": loaded.problem.into_iter().collect::<Vec<_>>(),
            "bounds": {
                "providers": prov::MAX_PROVIDERS,
                "models": prov::MAX_MODELS,
                "model_chars": prov::MODEL_CHARS,
                "history": prov::HISTORY,
                "records": prov::MAX_RECORDS,
                "tokens": prov::MAX_TOKENS,
                "price_per_mtok": prov::MAX_PRICE,
                "local_ports": prov::LOCAL_PORTS,
            },
        }))
    }

    #[tool(
        description = "Add, replace or `remove` a model provider (removal refused while a task uses it). A key is never passed or returned; `key` only references the system keychain. Secret-looking values are refused. Dry run unless `dry_run` false, which needs the `settings` right and writes `providers.toml` (mode 0600) in the user's settings folder."
    )]
    async fn model_provider_set(
        &self,
        Parameters(args): Parameters<ModelProviderSetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (file, mut register) = self.register_for_change()?;
        let answer = if args.remove {
            let old = register.remove_provider(&args.id).map_err(refused)?;
            json!({"removed": provider_json(&old)})
        } else {
            let kind = kind_named(
                args.kind
                    .as_deref()
                    .ok_or_else(|| invalid("`kind` is needed"))?,
            )?;
            let endpoint = args
                .endpoint
                .clone()
                .ok_or_else(|| invalid("`endpoint` is needed"))?;
            let key = key_named(args.key.as_deref().unwrap_or("none"))?;
            let provider = Provider {
                id: args.id.clone(),
                kind,
                endpoint,
                key,
                models: args.models.clone(),
                enabled: args.enabled.unwrap_or(true),
            };
            let old = register.set_provider(provider.clone()).map_err(refused)?;
            json!({
                "provider": provider_json(&provider),
                "replaced": old.as_ref().map(provider_json),
            })
        };
        self.write_register(&file, &register, args.dry_run, answer)
            .await
    }

    #[tool(
        description = "Set or remove a price row: `provider`, `model` (or `*` for every model of the provider; an exact model wins), `currency` (three capital letters, default USD), `input_per_mtok` and `output_per_mtok` (decimal numbers as text, at most 6 decimal places, 0 to 1000000, per million tokens), or `remove`. Prices are the user's configuration; Ferris IDEA never invents one, and a booking without a matching row keeps its cost unknown. dry_run defaults to true; dry_run=false needs the user-granted `settings` right."
    )]
    async fn model_price_set(
        &self,
        Parameters(args): Parameters<ModelPriceSetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (file, mut register) = self.register_for_change()?;
        let answer = if args.remove {
            let old = register
                .remove_price(&args.provider, &args.model)
                .map_err(refused)?;
            json!({"removed": old})
        } else {
            let price = Price {
                provider: args.provider.clone(),
                model: args.model.clone(),
                currency: args.currency.clone().unwrap_or_else(|| "USD".into()),
                input_per_mtok: args
                    .input_per_mtok
                    .clone()
                    .ok_or_else(|| invalid("`input_per_mtok` is needed"))?,
                output_per_mtok: args
                    .output_per_mtok
                    .clone()
                    .ok_or_else(|| invalid("`output_per_mtok` is needed"))?,
            };
            let old = register.set_price(price.clone()).map_err(refused)?;
            json!({"price": price, "replaced": old})
        };
        self.write_register(&file, &register, args.dry_run, answer)
            .await
    }

    #[tool(
        description = "Choose the model of a task, or change it later: `task` (`default` applies to every task without its own), `provider` (registered and enabled), `model` (one of the provider's models when it lists any), or `remove`. The earlier choice is kept in the assignment's history (last 20, with the time of each change). Answers the `previous` choice and the model that now applies. dry_run defaults to true; dry_run=false needs the user-granted `settings` right."
    )]
    async fn model_assign(
        &self,
        Parameters(args): Parameters<ModelAssignArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (file, mut register) = self.register_for_change()?;
        let answer = if args.remove {
            let old = register.unassign(&args.task).map_err(refused)?;
            json!({"removed": old, "now_applies": register.model_for(&args.task)})
        } else {
            let provider = args
                .provider
                .as_deref()
                .ok_or_else(|| invalid("`provider` is needed"))?;
            let model = args
                .model
                .as_deref()
                .ok_or_else(|| invalid("`model` is needed"))?;
            let previous = register
                .assign(&args.task, provider, model, unix_time())
                .map_err(refused)?;
            json!({"previous": previous, "now_applies": register.model_for(&args.task)})
        };
        self.write_register(&file, &register, args.dry_run, answer)
            .await
    }

    #[tool(
        description = "List the models a registered provider offers: asks `GET <endpoint>/models` without following redirects, with the provider's key from the system keychain only when its `key` is `keychain`, and only toward that endpoint; errors have the key and anything secret-looking masked. Changes nothing (add the names with model_provider_set). Needs the user-granted `http` right unless the endpoint is on a loopback address."
    )]
    async fn model_list(
        &self,
        Parameters(args): Parameters<ModelListArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (file, _) = self.model_files()?;
        let loaded = prov::load(&file, self.workspace.root());
        if let Some(p) = loaded.problem {
            return Err(invalid(p));
        }
        let provider = loaded
            .register
            .provider(&args.provider)
            .cloned()
            .ok_or_else(|| invalid(format!("no provider {:?}", args.provider)))?;
        if !prov::endpoint_is_loopback(&provider.endpoint) {
            self.require(Grant::Http).await?;
        }
        let asked = provider.clone();
        let models = tokio::task::spawn_blocking(move || {
            prov::provider_models(&asked, &ferris_idea_database::SystemStore)
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let unknown: Vec<&String> = models
            .iter()
            .filter(|m| !provider.models.contains(m))
            .collect();
        json_result(json!({
            "provider": provider.id,
            "endpoint": provider.endpoint,
            "key_sent": provider.key == KeySource::Keychain,
            "models": models,
            "not_in_register": unknown,
        }))
    }

    #[tool(
        description = "Find local model servers: asks 127.0.0.1 only, on ports 11434 (Ollama) and 8080 (llama.cpp server) or the `ports` given (at most 16), with `GET /api/tags` (Ollama) and `GET /v1/models` (llama.cpp or another OpenAI-compatible server), 2 seconds each. Answers per server the port, kind, endpoint to register, models and the registered provider using it. Never starts a server, never pulls or downloads a model and never changes the register. Needs no right."
    )]
    async fn model_detect_local(
        &self,
        Parameters(args): Parameters<ModelDetectLocalArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (file, _) = self.model_files()?;
        let register = prov::load(&file, self.workspace.root()).register;
        let ports = args.ports.clone();
        let found = tokio::task::spawn_blocking(move || prov::detect_local(&ports, &register))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .map_err(invalid)?;
        json_result(json!({
            "servers": found,
            "asked": if args.ports.is_empty() { prov::LOCAL_PORTS.to_vec() } else { args.ports },
        }))
    }

    #[tool(
        description = "The costs of model calls, read only: the booked records since `since` (Unix seconds, default all) grouped by `group_by` (provider — default, key, model or task), each with calls, input and output tokens, the cost per currency (six decimal places) and `unpriced_calls` (no price was known at booking; never guessed). Also `records`, `dropped` (over the bound of 20000), `unreadable` and `ledger_file`. Needs no right."
    )]
    async fn model_usage(
        &self,
        Parameters(args): Parameters<ModelUsageArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (_, ledger_file) = self.model_files()?;
        let by: GroupBy = match args.group_by.as_deref() {
            None => GroupBy::Provider,
            Some(name) => serde_json::from_value(json!(name))
                .map_err(|_| invalid("`group_by` is provider, key, model or task"))?,
        };
        let ledger = prov::read_ledger(&ledger_file);
        json_result(json!({
            "group_by": by,
            "groups": prov::summarize(&ledger.records, by, args.since.unwrap_or(0)),
            "records": ledger.records.len(),
            "dropped": ledger.dropped,
            "unreadable": ledger.unreadable,
            "ledger_file": ledger_file,
        }))
    }

    #[tool(
        description = "Book the token counts a provider reported for one call: `provider` (registered), `model`, `task` (default `default`), `input_tokens` and `output_tokens` (0 to 10^9 each). The cost is computed once from the price in force (exact model, else `*`) in millionths of its currency, or stays unknown without a price; the record names the key as `none` or `keychain:<host>:<port>`, never the key. dry_run defaults to true and answers the record; dry_run=false needs the user-granted `settings` right and appends it to `model-usage.jsonl` in the user's settings folder (mode 0600, at most 20000 records, the oldest dropped)."
    )]
    async fn model_usage_record(
        &self,
        Parameters(args): Parameters<ModelUsageRecordArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (file, register) = self.register_for_change()?;
        let (_, ledger_file) = self.model_files()?;
        let record = prov::price_booking(
            &register,
            &Booking {
                provider: &args.provider,
                model: &args.model,
                task: args.task.as_deref(),
                input_tokens: args.input_tokens,
                output_tokens: args.output_tokens,
                at: unix_time(),
            },
        )
        .map_err(refused)?;
        let dry_run = args.dry_run.unwrap_or(true);
        let cost = record.cost_micro.map(prov::show_micro);
        if dry_run {
            return json_result(json!({
                "dry_run": true,
                "record": record,
                "cost": cost,
                "ledger_file": ledger_file,
                "register_file": file,
            }));
        }
        self.require(Grant::Settings).await?;
        let ledger = prov::book(
            &ledger_file,
            self.workspace.root(),
            &record,
            prov::MAX_RECORDS,
        )
        .map_err(|e| McpError::internal_error(e, None))?;
        json_result(json!({
            "dry_run": false,
            "record": record,
            "cost": cost,
            "records": ledger.records.len(),
            "dropped": ledger.dropped,
            "ledger_file": ledger_file,
        }))
    }
}

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("model_assign", crate::groups::Group::Ide),
    ("model_detect_local", crate::groups::Group::Ide),
    ("model_list", crate::groups::Group::Ide),
    ("model_price_set", crate::groups::Group::Ide),
    ("model_provider_set", crate::groups::Group::Ide),
    ("model_providers", crate::groups::Group::Ide),
    ("model_usage", crate::groups::Group::Ide),
    ("model_usage_record", crate::groups::Group::Ide),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25).
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] = &[
    ("model_assign", &[]),
    ("model_detect_local", &[crate::untrusted::Source::Web]),
    ("model_list", &[]),
    ("model_price_set", &[]),
    ("model_provider_set", &[]),
    ("model_providers", &[]),
    ("model_usage", &[]),
    ("model_usage_record", &[]),
];
