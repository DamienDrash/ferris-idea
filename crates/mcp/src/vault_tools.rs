//! The internal vault for agents over MCP (`SPEC-0353`, ADR-0033, ROADMAP M19.15): the tools
//! `vault_list`, `vault_store`, `vault_generate`, `vault_remove` and `vault_audit`, the use of an
//! entry where Ferris IDEA puts its value in place (a process environment, a database connection),
//! the question in the window before an agent's first use, and the redaction of every answer.
//! No tool here or anywhere returns a value. Their own tool router, joined to the server's in
//! [`IdeServer::all_tools`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

use ferris_idea_core::Grant;
use ferris_idea_vault::{
    Agent, AuditRecord, Charset, EntryMeta, KeyStore, Kind, NewEntry, Policy, Scope, Use,
    UseRequest, Vault,
};

use crate::effects::{ASKS, Change, Dry, ToolEffect, VAULT, tool};
use crate::window::{PermissionAnswer, Scope as AskScope};
use crate::{CALL, IdeServer, json_result, unix_time};

/// The rows of this module's tools in the effects table (`SPEC-0053`, ADR-0035).
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("vault_audit", &[], Dry::None, &[], &[VAULT]),
    tool("vault_generate", &[Change::Settings], Dry::None, &[], &[VAULT]),
    tool("vault_list", &[], Dry::None, &[], &[VAULT]),
    tool("vault_remove", &[Change::Settings], Dry::None, &[], &[VAULT, ASKS]),
    tool("vault_store", &[Change::Settings], Dry::None, &[], &[VAULT]),
];

/// How long *Allow for This Session* holds at most (`SPEC-0353` behaviour 9).
const GRANT_SECONDS: u64 = 30 * 86_400;

/// What one MCP session knows about the vault: its identity and the answers of the person
/// (`SPEC-0353` behaviours 9, 11 and 13). Kept in memory only.
pub(crate) struct VaultSession {
    /// The random id of this session: the agent identity.
    id: String,
    /// Grants by `handle|form|destination`: `Some(until)` allowed, `None` denied.
    answers: Mutex<HashMap<String, Option<u64>>>,
}

impl Default for VaultSession {
    fn default() -> Self {
        let mut bytes = [0u8; 8];
        // Without a random source the id is still unique enough per process for display; the
        // vault itself refuses to work without one.
        let _ = getrandom::fill(&mut bytes);
        Self {
            id: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            answers: Mutex::default(),
        }
    }
}

/// Where the vault is and which key store holds its key; tests set both.
#[derive(Clone, Default)]
pub(crate) struct VaultPlace {
    pub(crate) dir: Option<PathBuf>,
    pub(crate) keys: Option<Arc<dyn KeyStore>>,
}

/// The platform's key store for the vault key (`SPEC-0353` behaviour 6): the macOS Keychain, the
/// Secret Service on Linux (never the kernel keyring), the Windows Credential Manager. Any other
/// system has none.
struct PlatformKeys;

impl PlatformKeys {
    fn reference() -> ferris_idea_database::SecretRef {
        ferris_idea_database::SecretRef {
            service: ferris_idea_vault::KEY_SERVICE.into(),
            account: ferris_idea_vault::KEY_ACCOUNT.into(),
        }
    }

    /// The key store, which also finds a vault key saved under the product's former service
    /// name and moves it once its copy reads back (`SPEC-0356` behaviour 11).
    #[cfg(any(target_os = "macos", windows))]
    fn backend() -> Result<
        ferris_idea_database::RenamedStore<
            ferris_idea_database::KeyringStore,
            ferris_idea_database::KeyringStore,
        >,
        String,
    > {
        Ok(ferris_idea_database::RenamedStore::new(
            ferris_idea_database::KeyringStore,
            ferris_idea_database::KeyringStore,
            ferris_idea_database::renamed_services(),
        ))
    }

    /// See the macOS variant.
    #[cfg(target_os = "linux")]
    fn backend() -> Result<
        ferris_idea_database::RenamedStore<
            ferris_idea_database::SecretServiceStore<NoLegacy>,
            ferris_idea_database::SecretServiceStore<NoLegacy>,
        >,
        String,
    > {
        if !ferris_idea_database::keychain_reachable() {
            return Err("the Secret Service (the desktop keyring) cannot be reached in this session; Ferris IDEA keeps the vault key only there, never in a file".into());
        }
        Ok(ferris_idea_database::RenamedStore::new(
            ferris_idea_database::SecretServiceStore::with_legacy(NoLegacy),
            ferris_idea_database::SecretServiceStore::with_legacy(NoLegacy).with_former_name(),
            ferris_idea_database::renamed_services(),
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    fn backend() -> Result<ferris_idea_database::MemoryStore, String> {
        Err("this system has no key store Ferris IDEA can use, so there is no vault".into())
    }
}

/// No older store on Linux: the vault key is never read from or moved out of the kernel keyring.
#[cfg(target_os = "linux")]
#[derive(Debug, Default, Clone, Copy)]
struct NoLegacy;

#[cfg(target_os = "linux")]
impl ferris_idea_database::SecretStore for NoLegacy {
    fn get(
        &self,
        _: &ferris_idea_database::SecretRef,
    ) -> Result<Option<ferris_idea_database::Secret>, ferris_idea_database::SecretError> {
        Ok(None)
    }
    fn set(
        &self,
        _: &ferris_idea_database::SecretRef,
        _: &ferris_idea_database::Secret,
    ) -> Result<(), ferris_idea_database::SecretError> {
        Ok(())
    }
    fn delete(
        &self,
        _: &ferris_idea_database::SecretRef,
    ) -> Result<(), ferris_idea_database::SecretError> {
        Ok(())
    }
}

impl KeyStore for PlatformKeys {
    fn name(&self) -> String {
        if cfg!(target_os = "macos") {
            "macOS Keychain".into()
        } else if cfg!(target_os = "linux") {
            "Secret Service".into()
        } else if cfg!(windows) {
            "Windows Credential Manager".into()
        } else {
            "no key store".into()
        }
    }

    fn load(&self) -> Result<Option<Zeroizing<String>>, String> {
        use ferris_idea_database::SecretStore;
        let store = Self::backend()?;
        store
            .get(&Self::reference())
            .map(|s| s.map(|s| Zeroizing::new(s.expose().to_owned())))
            .map_err(|e| e.message)
    }

    fn store(&self, item: &str) -> Result<(), String> {
        use ferris_idea_database::SecretStore;
        let store = Self::backend()?;
        store
            .set(&Self::reference(), &ferris_idea_database::Secret::new(item))
            .map_err(|e| e.message)
    }
}

/// The arguments with `vault_store`'s value hidden, for the question in the window
/// (`SPEC-0353` behaviour 18).
pub(crate) fn shown_arguments(
    tool: &str,
    arguments: Option<&Map<String, Value>>,
) -> Option<Map<String, Value>> {
    let mut args = arguments.cloned()?;
    if tool == "vault_store" && args.contains_key("value") {
        args.insert("value".into(), json!("«hidden»"));
    }
    Some(args)
}

/// `result` with every value the vault registered replaced by its entry's name (`SPEC-0353`
/// behaviour 18).
pub(crate) fn redact_result(
    result: Result<rmcp::model::CallToolResponse, McpError>,
) -> Result<rmcp::model::CallToolResponse, McpError> {
    if !ferris_idea_vault::redact::active() {
        return result;
    }
    let redact = ferris_idea_vault::redact::redact;
    match result {
        Ok(rmcp::model::CallToolResponse::Complete(answer)) => {
            let text = serde_json::to_string(&answer)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            let (clean, n) = redact(&text);
            if n == 0 {
                return Ok(rmcp::model::CallToolResponse::Complete(answer));
            }
            serde_json::from_str(&clean)
                .map(rmcp::model::CallToolResponse::Complete)
                .map_err(|_| {
                    McpError::internal_error(
                        "the answer held a value of the vault and could not be redacted; it was withheld",
                        None,
                    )
                })
        }
        Ok(other) => Ok(other),
        Err(mut e) => {
            e.message = redact(&e.message).0.into();
            if let Some(data) = e.data.take() {
                let text = data.to_string();
                e.data = serde_json::from_str(&redact(&text).0).ok();
            }
            Err(e)
        }
    }
}

fn invalid(m: impl Into<String>) -> McpError {
    McpError::invalid_params(m.into(), None)
}

fn vault_error(e: ferris_idea_vault::VaultError) -> McpError {
    invalid(e.to_string())
}

/// Arguments of `vault_store`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VaultStoreArgs {
    /// The entry's unique name: 1 to 64 characters of A-Z, a-z, 0-9, ., _, -.
    pub name: String,
    /// The value, 8 to 4096 bytes. It is never shown again, to anyone.
    pub value: String,
    /// password, token or secret (default secret).
    pub kind: Option<String>,
    /// Where Ferris IDEA may put the value: `database` (a connection's password) and/or
    /// `process-env` (an environment variable of `run_command`).
    pub uses: Vec<String>,
    /// For `database`: the hosts, `host` (any port) or `host:port`; no wildcards.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Days until the entry expires (1 to 3650); default never.
    pub expires_in_days: Option<u64>,
}

/// Arguments of `vault_generate`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VaultGenerateArgs {
    /// The entry's unique name: 1 to 64 characters of A-Z, a-z, 0-9, ., _, -.
    pub name: String,
    /// Length of the value, 16 to 128 (default 24).
    pub length: Option<usize>,
    /// `printable` (the 94 printable ASCII characters, default) or `alphanumeric`.
    pub charset: Option<String>,
    /// password, token or secret (default password).
    pub kind: Option<String>,
    /// Where Ferris IDEA may put the value: `database` and/or `process-env`.
    pub uses: Vec<String>,
    /// For `database`: the hosts, `host` or `host:port`; no wildcards.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Days until the entry expires (1 to 3650); default never.
    pub expires_in_days: Option<u64>,
}

/// Arguments of `vault_remove`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VaultRemoveArgs {
    /// The entry's handle (`vlt_…`).
    pub handle: String,
}

/// Arguments of `vault_audit`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VaultAuditArgs {
    /// How many records, newest first (default 50, at most 500).
    pub limit: Option<usize>,
}

impl IdeServer {
    /// The vault of the user: beside the grant file unless tests set a folder, with the
    /// platform's key store unless tests set one (`SPEC-0353` behaviours 4 and 6).
    pub(crate) fn vault(&self) -> Result<Vault, McpError> {
        let dir = match &self.vault_place.dir {
            Some(d) => d.clone(),
            None => self
                .permissions
                .file
                .as_ref()
                .and_then(|f| f.parent())
                .map(|d| d.join("vault"))
                .ok_or_else(|| invalid("no settings folder is known, so there is no vault"))?,
        };
        let keys: Arc<dyn KeyStore> = match &self.vault_place.keys {
            Some(k) => k.clone(),
            None => Arc::new(PlatformKeys),
        };
        Ok(Vault::new(dir, keys))
    }

    /// The resolved workspace root, as scopes name projects (behaviour 7).
    fn vault_project(&self) -> String {
        let root = self.workspace.root();
        std::fs::canonicalize(root)
            .unwrap_or_else(|_| root.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    /// The agent identity of this session (behaviour 13).
    fn vault_agent(&self) -> Agent {
        Agent {
            session: self.vault_session.id.clone(),
            client: CALL
                .try_with(|c| c.client.clone())
                .unwrap_or_else(|_| "an unnamed client".into()),
        }
    }

    /// A record of the audit log for `meta` (behaviour 20).
    fn vault_record(
        &self,
        meta: &EntryMeta,
        form: &str,
        destination: &str,
        result: &str,
        approval: &str,
    ) -> AuditRecord {
        AuditRecord {
            time: unix_time(),
            project: self.vault_project(),
            handle: meta.handle.clone(),
            entry: meta.name.clone(),
            agent: self.vault_agent().to_string(),
            tool: CALL.try_with(|c| c.tool.clone()).unwrap_or_default(),
            form: form.into(),
            destination: destination.into(),
            result: result.into(),
            approval: approval.into(),
            ..AuditRecord::default()
        }
    }

    /// Asks the person in the window about the vault entry `name` (behaviours 9 and 19); the
    /// error says why nobody answered.
    async fn vault_ask_user(
        &self,
        name: &str,
        meaning: &str,
    ) -> Result<PermissionAnswer, &'static str> {
        match self
            .ask_window("vault", meaning, AskScope::Vault(name.to_owned()))
            .await
        {
            Err("") => Err("nobody was asked (no Ferris IDEA window could ask)"),
            other => other,
        }
    }

    /// The value of the entry `handle` for one use in `form` towards `destination`, after the
    /// scope check, the person's approval and the audit record (`SPEC-0353` behaviours 8–11, 17,
    /// 20). Only code that puts the value in place calls this; it never reaches an answer.
    pub(crate) async fn vault_use(
        &self,
        handle: &str,
        form: Use,
        destination: Option<String>,
    ) -> Result<Zeroizing<String>, McpError> {
        if !ferris_idea_vault::is_handle(handle) {
            return Err(invalid(format!("`{handle}` is no vault handle (vlt_…)")));
        }
        let vault = self.vault()?;
        let now = unix_time();
        let meta = vault.entry(handle).map_err(vault_error)?;
        let shown = destination.clone().unwrap_or_else(|| "process".into());
        let request = UseRequest {
            project: self.vault_project(),
            form,
            destination: destination.clone(),
        };
        let record = |result: &str, approval: &str| {
            vault.record(self.vault_record(&meta, form.name(), &shown, result, approval))
        };
        if let Err(e) = ferris_idea_vault::check_use(&meta, &request, now) {
            let result = if matches!(e, ferris_idea_vault::VaultError::Expired(_)) {
                "expired"
            } else {
                "refused"
            };
            record(result, "").map_err(vault_error)?;
            return Err(vault_error(e));
        }
        let key = format!("{handle}|{}|{shown}", form.name());
        let remembered = self
            .vault_session
            .answers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .copied();
        let approval = match remembered {
            Some(Some(until)) if now < until => format!("session grant until {until}"),
            Some(None) => {
                record("denied", "denied earlier in this session").map_err(vault_error)?;
                return Err(invalid(format!(
                    "denied: the user did not allow this session to use the vault entry `{}` for {} to {shown}",
                    meta.name,
                    form.name()
                )));
            }
            _ => {
                let tool = CALL.try_with(|c| c.tool.clone()).unwrap_or_default();
                let exposure = match form {
                    Use::Database => format!(
                        "Ferris IDEA hands the value to the database driver for the connection to {shown}"
                    ),
                    Use::ProcessEnv => "Ferris IDEA sets it as an environment variable of a program started from the project: that program can read it, and no sandbox limits it yet, so every such use asks again".into(),
                };
                let meaning = format!(
                    "use the vault entry \"{}\" ({:?}) through the tool {tool}, form {}, destination {shown}; {exposure}. The agent never sees the value.",
                    meta.name,
                    meta.kind,
                    form.name()
                );
                match self.vault_ask_user(&meta.name, &meaning).await {
                    Ok(PermissionAnswer::Deny) => {
                        self.vault_session
                            .answers
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key, None);
                        record("denied", "window: deny").map_err(vault_error)?;
                        return Err(invalid(format!(
                            "denied: the user did not allow the use of the vault entry `{}`; nothing was used",
                            meta.name
                        )));
                    }
                    Ok(PermissionAnswer::Always) if form != Use::ProcessEnv => {
                        let until = meta
                            .expires_at
                            .map_or(now + GRANT_SECONDS, |t| t.min(now + GRANT_SECONDS));
                        self.vault_session
                            .answers
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key, Some(until));
                        format!("window: session grant until {until}")
                    }
                    Ok(_) => "window: once".into(),
                    Err(why) => {
                        record("unanswered", why).map_err(vault_error)?;
                        return Err(invalid(format!(
                            "not granted: {why}; a vault entry is used only after the person allowed it in the Ferris IDEA window (SPEC-0353)"
                        )));
                    }
                }
            }
        };
        // The scope is checked again at the moment the value is taken (behaviour 8).
        let (meta, value) = vault
            .value_for_use(handle, &request, unix_time())
            .map_err(vault_error)?;
        ferris_idea_vault::redact::register(&meta.name, &value);
        // A use that cannot be recorded does not happen (behaviour 20).
        record("used", &approval).map_err(vault_error)?;
        Ok(value)
    }

    /// The credentials of `profile` for an agent's connection: a vault handle is used through
    /// [`Self::vault_use`] (`SPEC-0353` behaviour 14), any other reference is read from the
    /// keychain as before (SPEC-0008).
    pub(crate) async fn agent_credentials(
        &self,
        profile: &ferris_idea_database::ConnectionProfile,
    ) -> Result<ferris_idea_database::Credentials, McpError> {
        if let Some(reference) = profile
            .secret
            .as_ref()
            .filter(|_| is_vault_profile(profile))
        {
            let destination = vault_destination(profile)?;
            let value = self
                .vault_use(&reference.account, Use::Database, Some(destination))
                .await?;
            return Ok(ferris_idea_database::Credentials::Available(
                ferris_idea_database::Secret::new(value.as_str()),
            ));
        }
        ferris_idea_database::resolve_credentials(profile, &ferris_idea_database::SystemStore)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    /// Checks that the vault entry `handle` may be bound to `profile` (`SPEC-0353` behaviour 14):
    /// it exists in this project, allows `database`, and its hosts match the connection's.
    pub(crate) fn vault_binding(
        &self,
        handle: &str,
        profile: &ferris_idea_database::ConnectionProfile,
    ) -> Result<ferris_idea_database::SecretRef, McpError> {
        if !ferris_idea_vault::is_handle(handle) {
            return Err(invalid(format!("`{handle}` is no vault handle (vlt_…)")));
        }
        let destination = vault_destination(profile)?;
        let meta = self.vault()?.entry(handle).map_err(vault_error)?;
        let request = UseRequest {
            project: self.vault_project(),
            form: Use::Database,
            destination: Some(destination),
        };
        ferris_idea_vault::check_use(&meta, &request, unix_time()).map_err(vault_error)?;
        Ok(ferris_idea_database::SecretRef {
            service: ferris_idea_vault::REF_SERVICE.into(),
            account: handle.into(),
        })
    }

    /// A new entry of this session in this project (behaviours 7 and 16).
    fn vault_new_entry(
        &self,
        name: &str,
        kind: Kind,
        uses: &[String],
        hosts: &[String],
        expires_in_days: Option<u64>,
    ) -> Result<NewEntry, McpError> {
        let uses = uses
            .iter()
            .map(|u| {
                Use::from_name(u)
                    .ok_or_else(|| invalid(format!("`{u}` is no use: database or process-env")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(NewEntry {
            name: name.into(),
            kind,
            scope: Scope {
                projects: vec![self.vault_project()],
                uses,
                hosts: hosts.to_vec(),
            },
            created_by: self.vault_agent(),
            expires_in_days,
        })
    }

    /// The entry `handle` if it belongs to this project (behaviour 21).
    fn vault_entry_here(&self, vault: &Vault, handle: &str) -> Result<EntryMeta, McpError> {
        let meta = vault.entry(handle).map_err(vault_error)?;
        if meta.scope.projects.contains(&self.vault_project()) {
            Ok(meta)
        } else {
            Err(invalid(format!(
                "there is no vault entry `{handle}` in this project"
            )))
        }
    }
}

/// Checks `run_command`'s `env_from_vault` (`SPEC-0353` behaviour 12): at most 16 variables
/// with names of `[A-Za-z_][A-Za-z0-9_]*`, each naming a handle.
pub(crate) fn check_env_from_vault(
    env: &std::collections::BTreeMap<String, String>,
) -> Result<(), McpError> {
    if env.len() > 16 {
        return Err(invalid("env_from_vault names at most 16 variables"));
    }
    for (name, handle) in env {
        let ok = name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !ok {
            return Err(invalid(format!(
                "`{name}` is no environment variable name ([A-Za-z_][A-Za-z0-9_]*)"
            )));
        }
        if !ferris_idea_vault::is_handle(handle) {
            return Err(invalid(format!("`{handle}` is no vault handle (vlt_…)")));
        }
    }
    Ok(())
}

/// Whether `profile` takes its password from the vault (`SPEC-0353` behaviour 14).
pub(crate) fn is_vault_profile(profile: &ferris_idea_database::ConnectionProfile) -> bool {
    profile
        .secret
        .as_ref()
        .is_some_and(|r| ferris_idea_vault::is_ref_service(&r.service))
}

/// `host:port` of a connection profile (behaviour 14); a file database takes no vault entry.
fn vault_destination(
    profile: &ferris_idea_database::ConnectionProfile,
) -> Result<String, McpError> {
    let host = profile
        .host
        .as_deref()
        .filter(|h| !h.trim().is_empty() && !profile.dialect.file_based())
        .ok_or_else(|| {
            invalid(format!(
                "connection `{}` has no host: a file database takes no vault entry",
                profile.name
            ))
        })?;
    Ok(match profile.effective_port() {
        Some(port) => format!("{}:{port}", host.trim()),
        None => host.trim().to_owned(),
    })
}

fn kind_named(name: Option<&str>, default: Kind) -> Result<Kind, McpError> {
    match name {
        None => Ok(default),
        Some(n) => Kind::from_name(n)
            .ok_or_else(|| invalid(format!("`{n}` is no kind: password, token or secret"))),
    }
}

/// An entry's metadata as answered: never a value.
fn entry_json(meta: &EntryMeta, now: u64, uses: usize, last_used: Option<u64>) -> Value {
    json!({
        "handle": meta.handle,
        "name": meta.name,
        "kind": meta.kind,
        "uses": meta.scope.uses,
        "hosts": meta.scope.hosts,
        "projects": meta.scope.projects,
        "created_by": meta.created_by.to_string(),
        "created_at": meta.created_at,
        "expires_at": meta.expires_at,
        "expired": meta.expired(now),
        "seen_by_agent": meta.seen_by_agent,
        "use_count": uses,
        "last_used": last_used,
    })
}

#[tool_router(router = vault_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "The entries of Ferris IDEA's internal vault usable in this workspace: handle (vlt_…), name, kind, uses (database, process-env), hosts, created_by, created_at, expires_at and expired, seen_by_agent (the value was once in an agent's context), use_count and last_used (from the audit log); `elsewhere` counts entries of other projects; `key_store` names where the vault key is (macOS Keychain, Secret Service, Windows Credential Manager) and `vault_dir` the folder in the user's settings. Never a value: no tool returns one. Use a handle with run_command `env_from_vault` or save_connection `vault`. Needs the `vault` right."
    )]
    async fn vault_list(&self) -> Result<CallToolResult, McpError> {
        self.require(Grant::Vault).await?;
        let vault = self.vault()?;
        let entries = vault.list().map_err(vault_error)?;
        let audit = vault.audit().map_err(vault_error)?;
        let project = self.vault_project();
        let now = unix_time();
        let (here, elsewhere): (Vec<&EntryMeta>, Vec<&EntryMeta>) = entries
            .iter()
            .partition(|e| e.scope.projects.contains(&project));
        let listed: Vec<Value> = here
            .iter()
            .map(|e| {
                let used: Vec<&AuditRecord> = audit
                    .records
                    .iter()
                    .filter(|r| r.handle == e.handle && r.result == "used")
                    .collect();
                entry_json(e, now, used.len(), used.last().map(|r| r.time))
            })
            .collect();
        json_result(json!({
            "entries": listed,
            "elsewhere": elsewhere.len(),
            "key_store": vault.key_store_name(),
            "vault_dir": vault.dir(),
            "audit_intact": audit.intact,
        }))
    }

    #[tool(
        description = "Store a credential you hold in Ferris IDEA's vault and get its handle; it is encrypted and never readable by any agent. `uses` database (save_connection `vault`, needs `hosts`) and/or process-env (run_command `env_from_vault`). Workspace-only and marked seen_by_agent: tell the user to rotate it; prefer vault_generate. Each later use asks the person first. Needs the `vault` right."
    )]
    async fn vault_store(
        &self,
        Parameters(args): Parameters<VaultStoreArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.require(Grant::Vault).await?;
        let kind = kind_named(args.kind.as_deref(), Kind::Secret)?;
        let new = self.vault_new_entry(
            &args.name,
            kind,
            &args.uses,
            &args.hosts,
            args.expires_in_days,
        )?;
        let vault = self.vault()?;
        let value = Zeroizing::new(args.value);
        let meta = vault.put(new, &value, unix_time()).map_err(vault_error)?;
        ferris_idea_vault::redact::register(&meta.name, &value);
        vault
            .record(self.vault_record(&meta, "", "", "stored", ""))
            .map_err(vault_error)?;
        json_result(json!({
            "handle": meta.handle,
            "entry": entry_json(&meta, unix_time(), 0, None),
            "note": "The value was in your context and so in your provider's transcript: ask the user to rotate it at the service and prefer vault_generate next time.",
        }))
    }

    #[tool(
        description = "Generate a password or token inside Ferris IDEA's internal vault and get only its handle: the value is drawn from the system's random source and is never shown to any agent. `name` (unique), `length` (16 to 128, default 24), `charset` (printable: 94 printable ASCII characters, default; alphanumeric), `kind` (default password), `uses` (database, process-env), `hosts` (required for database: host or host:port), `expires_in_days`. Use the handle with save_connection `vault` or run_command `env_from_vault`; each use asks the person in the Ferris IDEA window first. Needs the `vault` right."
    )]
    async fn vault_generate(
        &self,
        Parameters(args): Parameters<VaultGenerateArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.require(Grant::Vault).await?;
        let kind = kind_named(args.kind.as_deref(), Kind::Password)?;
        let charset = match args.charset.as_deref() {
            None => Charset::Printable,
            Some(c) => Charset::from_name(c).ok_or_else(|| {
                invalid(format!("`{c}` is no charset: printable or alphanumeric"))
            })?,
        };
        let policy = Policy {
            length: args.length.unwrap_or(Policy::default().length),
            charset,
        };
        let new = self.vault_new_entry(
            &args.name,
            kind,
            &args.uses,
            &args.hosts,
            args.expires_in_days,
        )?;
        let vault = self.vault()?;
        let meta = vault
            .generate(new, policy, unix_time())
            .map_err(vault_error)?;
        vault
            .record(self.vault_record(&meta, "", "", "generated", ""))
            .map_err(vault_error)?;
        json_result(json!({
            "handle": meta.handle,
            "entry": entry_json(&meta, unix_time(), 0, None),
        }))
    }

    #[tool(
        description = "Remove an entry of Ferris IDEA's internal vault in this workspace: `handle`. An entry this session created and never used is removed at once; any other asks the person in the Ferris IDEA window (without a window it is refused). The value is gone from the vault file at once; removing a handle does not revoke the credential at the service. Needs the `vault` right."
    )]
    async fn vault_remove(
        &self,
        Parameters(args): Parameters<VaultRemoveArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.require(Grant::Vault).await?;
        let vault = self.vault()?;
        let meta = self.vault_entry_here(&vault, &args.handle)?;
        let used = vault
            .audit()
            .map_err(vault_error)?
            .records
            .iter()
            .any(|r| r.handle == meta.handle && r.result == "used");
        let own = meta.created_by.session == self.vault_session.id && !used;
        let approval = if own {
            "own entry".to_owned()
        } else {
            let meaning = format!(
                "remove the vault entry \"{}\" (created by {}); its value is destroyed and uses through its handle stop",
                meta.name, meta.created_by
            );
            match self.vault_ask_user(&meta.name, &meaning).await {
                Ok(PermissionAnswer::Deny) => {
                    return Err(invalid(format!(
                        "denied: the user did not allow removing the vault entry `{}`",
                        meta.name
                    )));
                }
                Ok(_) => "window".to_owned(),
                Err(why) => {
                    return Err(invalid(format!(
                        "not granted: {why}; removing an entry another session created or used needs the person"
                    )));
                }
            }
        };
        let removed = vault.remove(&meta.handle).map_err(vault_error)?;
        vault
            .record(self.vault_record(&removed, "", "", "removed", &approval))
            .map_err(vault_error)?;
        json_result(
            json!({ "removed": removed.handle, "name": removed.name, "approval": approval }),
        )
    }

    #[tool(
        description = "The audit log of Ferris IDEA's internal vault for this workspace: the newest records first (`limit` default 50, at most 500), each with seq, time, handle, entry, agent (session and client name), tool, form, destination, result (used, denied, refused, expired, unanswered, stored, generated, removed) and approval; `intact` says whether the authenticated chain of the whole log holds, `problem` where it breaks. Never a value. Needs the `vault` right."
    )]
    async fn vault_audit(
        &self,
        Parameters(args): Parameters<VaultAuditArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.require(Grant::Vault).await?;
        let log = self.vault()?.audit().map_err(vault_error)?;
        let project = self.vault_project();
        let limit = args.limit.unwrap_or(50).clamp(1, 500);
        let records: Vec<&AuditRecord> = log
            .records
            .iter()
            .rev()
            .filter(|r| r.project == project)
            .take(limit)
            .collect();
        json_result(json!({
            "records": records,
            "intact": log.intact,
            "problem": log.problem,
        }))
    }
}

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("vault_audit", crate::groups::Group::Ide),
    ("vault_generate", crate::groups::Group::Ide),
    ("vault_list", crate::groups::Group::Ide),
    ("vault_remove", crate::groups::Group::Ide),
    ("vault_store", crate::groups::Group::Ide),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25).
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] = &[
    ("vault_audit", &[]),
    ("vault_generate", &[]),
    ("vault_list", &[]),
    ("vault_remove", &[]),
    ("vault_store", &[]),
];
