//! Models and providers for agents (`SPEC-0350`, ROADMAP M19.7): the register of the providers the
//! user set up, the model of each task, local servers found on the loopback address, and the costs
//! booked from the token counts a provider reports against the user's own price list.
//!
//! Keys stay in the system keychain (ADR-0017): the register only says `keychain`, and the key goes
//! to its own endpoint and nowhere else. Prices are never built in; without one a cost is unknown.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_database::{SecretRef, SecretStore};
use ferris_idea_http::{ResolvedRequest, parse_url, send_once};
use serde::{Deserialize, Serialize};

use crate::{AiError, context, key_ref, list_models};

/// The register's file name (behaviour 1).
pub const REGISTER_FILE: &str = "providers.toml";
/// The usage ledger's file name (behaviour 13).
pub const LEDGER_FILE: &str = "model-usage.jsonl";
/// Most providers (behaviour 2).
pub const MAX_PROVIDERS: usize = 32;
/// Most model names of one provider (behaviour 2).
pub const MAX_MODELS: usize = 500;
/// Longest model name (behaviour 2).
pub const MODEL_CHARS: usize = 200;
/// Earlier choices an assignment keeps (behaviour 9).
pub const HISTORY: usize = 20;
/// Most records in the ledger (behaviour 13).
pub const MAX_RECORDS: usize = 20_000;
/// Most tokens of one record, each direction (behaviour 12).
pub const MAX_TOKENS: u64 = 1_000_000_000;
/// Highest price per million tokens (behaviour 11).
pub const MAX_PRICE: u64 = 1_000_000;
/// The default ports of local servers (behaviour 7).
pub const LOCAL_PORTS: [u16; 2] = [11434, 8080];
/// Most ports one detection asks (behaviour 7).
pub const MAX_PORTS: usize = 16;
/// Time of one detection request (behaviour 7).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// The task whose assignment applies to every task without one (behaviour 9).
pub const DEFAULT_TASK: &str = "default";

/// What kind of API a provider has (behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// An OpenAI-compatible API.
    Openai,
    /// Anthropic's API.
    Anthropic,
    /// A local Ollama server.
    Ollama,
    /// A local llama.cpp server or another local OpenAI-compatible server.
    Llamacpp,
}

impl Kind {
    /// Whether the kind is a server on this machine only (behaviour 4).
    pub fn local(self) -> bool {
        matches!(self, Kind::Ollama | Kind::Llamacpp)
    }

    /// The name in the register.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Openai => "openai",
            Kind::Anthropic => "anthropic",
            Kind::Ollama => "ollama",
            Kind::Llamacpp => "llamacpp",
        }
    }
}

/// Where a provider's key comes from (behaviour 5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    /// No key is sent.
    #[default]
    None,
    /// The system keychain's entry of the endpoint.
    Keychain,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// One provider (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    /// The provider's id.
    pub id: String,
    /// Its kind.
    pub kind: Kind,
    /// The base URL of its API.
    pub endpoint: String,
    /// Where its key comes from.
    #[serde(default)]
    pub key: KeySource,
    /// The model names known for it; empty: not listed yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// Whether tasks may use it.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn usd() -> String {
    "USD".into()
}

/// One price row (behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    /// The provider's id.
    pub provider: String,
    /// The model, or `*` for every model of the provider.
    pub model: String,
    /// Three capital letters.
    #[serde(default = "usd")]
    pub currency: String,
    /// Price of a million input tokens, as a decimal number.
    pub input_per_mtok: String,
    /// Price of a million output tokens, as a decimal number.
    pub output_per_mtok: String,
}

/// An earlier choice of an assignment (behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    /// The provider's id.
    pub provider: String,
    /// The model.
    pub model: String,
    /// When it was chosen (seconds since the Unix epoch).
    pub changed_at: u64,
}

/// The model of a task (behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    /// The task.
    pub task: String,
    /// The provider's id.
    pub provider: String,
    /// The model.
    pub model: String,
    /// When it was chosen.
    pub changed_at: u64,
    /// Earlier choices, newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous: Vec<Choice>,
}

/// The register (behaviour 1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Register {
    /// The providers.
    #[serde(default, rename = "provider", skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<Provider>,
    /// The prices.
    #[serde(default, rename = "price", skip_serializing_if = "Vec::is_empty")]
    pub prices: Vec<Price>,
    /// The assignments.
    #[serde(default, rename = "assignment", skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<Assignment>,
}

/// The register's and the ledger's paths beside the grant file (behaviours 1 and 13). A grant
/// file named `<prefix>agent-permissions.toml` gives `<prefix>providers.toml`, as the scheduler's
/// files do.
pub fn files(grant_file: &Path) -> (PathBuf, PathBuf) {
    let name = grant_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = name
        .strip_suffix(ferris_idea_core::GRANT_FILE)
        .unwrap_or("");
    (
        grant_file.with_file_name(format!("{prefix}{REGISTER_FILE}")),
        grant_file.with_file_name(format!("{prefix}{LEDGER_FILE}")),
    )
}

/// Whether `text` holds something that looks like a secret (`SPEC-0313` behaviour 6).
pub fn looks_secret(text: &str) -> bool {
    context::mask_secrets(text).1 > 0
}

fn id_ok(id: &str, extra: &[char]) -> bool {
    !id.is_empty()
        && id.chars().count() <= 64
        && id.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || "-_".contains(c) || extra.contains(&c)
        })
}

/// Whether `host` is a loopback address or `localhost` (behaviour 4).
pub fn loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Whether the endpoint's host is a loopback address.
pub fn endpoint_is_loopback(endpoint: &str) -> bool {
    parse_url(endpoint.trim()).is_ok_and(|t| loopback(&t.host))
}

/// Checks an endpoint for `kind` (behaviour 4).
pub fn check_endpoint(kind: Kind, endpoint: &str) -> Result<(), String> {
    let e = endpoint.trim();
    let rest = e
        .strip_prefix("https://")
        .or_else(|| e.strip_prefix("http://"))
        .ok_or("the endpoint is an http or https URL")?;
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        return Err(
            "the endpoint has no user name or password; keys belong in the keychain".into(),
        );
    }
    if e.contains('?') || e.contains('#') {
        return Err("the endpoint has no query or fragment".into());
    }
    let target = parse_url(e).map_err(|err| err.to_string())?;
    let local = loopback(&target.host);
    if kind.local() && !local {
        return Err(format!(
            "a {} server is local only: its endpoint is on 127.0.0.1, ::1 or localhost; a server on another machine is kind openai with https",
            kind.name()
        ));
    }
    if !target.tls && !local {
        return Err("an endpoint on another machine uses https".into());
    }
    Ok(())
}

/// A decimal price as millionths (behaviour 11).
pub fn parse_price(text: &str) -> Result<u64, String> {
    let bad = || {
        format!("{text:?} is a decimal number from 0 to {MAX_PRICE} with at most 6 decimal places")
    };
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
        || frac.len() > 6
        || (text.contains('.') && frac.is_empty())
    {
        return Err(bad());
    }
    let whole: u64 = whole.parse().map_err(|_| bad())?;
    if whole > MAX_PRICE {
        return Err(bad());
    }
    let frac: u64 = format!("{frac:0<6}").parse().map_err(|_| bad())?;
    let micro = whole * 1_000_000 + frac;
    if micro > MAX_PRICE * 1_000_000 {
        return Err(bad());
    }
    Ok(micro)
}

/// Millionths as text with six decimal places (behaviour 14).
pub fn show_micro(micro: u64) -> String {
    format!("{}.{:06}", micro / 1_000_000, micro % 1_000_000)
}

impl Register {
    /// Checks every rule (behaviours 2 to 6, 9 and 11).
    pub fn check(&self) -> Result<(), String> {
        if self.providers.len() > MAX_PROVIDERS {
            return Err(format!("at most {MAX_PROVIDERS} providers"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for p in &self.providers {
            let at = |m: String| format!("provider {:?}: {m}", p.id);
            if !id_ok(&p.id, &[]) {
                return Err(format!(
                    "provider id {:?}: 1 to 64 characters of a-z, 0-9, -, _",
                    p.id
                ));
            }
            if !seen.insert(p.id.as_str()) {
                return Err(at("two providers have this id".into()));
            }
            check_endpoint(p.kind, &p.endpoint).map_err(|e| at(context::mask_secrets(&e).0))?;
            if looks_secret(&p.endpoint) {
                return Err(at(
                    "the endpoint looks like it holds a secret; keys belong in the keychain".into(),
                ));
            }
            if p.models.len() > MAX_MODELS {
                return Err(at(format!("at most {MAX_MODELS} models")));
            }
            for m in &p.models {
                check_model(m).map_err(at)?;
            }
        }
        for r in &self.prices {
            let at = |m: String| format!("price of {:?} for {:?}: {m}", r.model, r.provider);
            if !self.providers.iter().any(|p| p.id == r.provider) {
                return Err(at("no such provider".into()));
            }
            if r.model != "*" {
                check_model(&r.model).map_err(at)?;
            }
            if !(r.currency.len() == 3 && r.currency.chars().all(|c| c.is_ascii_uppercase())) {
                return Err(at("the currency is three capital letters, e.g. USD".into()));
            }
            parse_price(&r.input_per_mtok).map_err(at)?;
            parse_price(&r.output_per_mtok).map_err(at)?;
        }
        let mut prices = std::collections::BTreeSet::new();
        for r in &self.prices {
            if !prices.insert((r.provider.as_str(), r.model.as_str())) {
                return Err(format!("two prices of {:?} for {:?}", r.model, r.provider));
            }
        }
        let mut tasks = std::collections::BTreeSet::new();
        for a in &self.assignments {
            let at = |m: String| format!("assignment of task {:?}: {m}", a.task);
            if !id_ok(&a.task, &['.']) {
                return Err(at(
                    "the task is 1 to 64 characters of a-z, 0-9, -, _, .".into()
                ));
            }
            if !tasks.insert(a.task.as_str()) {
                return Err(at("two assignments of this task".into()));
            }
            check_model(&a.model).map_err(at)?;
            if a.previous.len() > HISTORY {
                return Err(at(format!("at most {HISTORY} earlier choices")));
            }
        }
        Ok(())
    }

    /// The provider with `id`.
    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Adds or replaces a provider (behaviour 16).
    pub fn set_provider(&mut self, provider: Provider) -> Result<Option<Provider>, String> {
        let mut next = self.clone();
        let old = match next.providers.iter_mut().find(|p| p.id == provider.id) {
            Some(p) => Some(std::mem::replace(p, provider)),
            None => {
                next.providers.push(provider);
                None
            }
        };
        next.check()?;
        *self = next;
        Ok(old)
    }

    /// Removes a provider and its prices; refused while an assignment uses it (behaviour 10).
    pub fn remove_provider(&mut self, id: &str) -> Result<Provider, String> {
        let using: Vec<&str> = self
            .assignments
            .iter()
            .filter(|a| a.provider == id)
            .map(|a| a.task.as_str())
            .collect();
        if !using.is_empty() {
            return Err(format!(
                "provider {id:?} is assigned to the tasks {}; assign them another model first",
                using.join(", ")
            ));
        }
        let at = self
            .providers
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| format!("no provider {id:?}"))?;
        self.prices.retain(|r| r.provider != id);
        Ok(self.providers.remove(at))
    }

    /// Sets or replaces a price row (behaviour 11).
    pub fn set_price(&mut self, price: Price) -> Result<Option<Price>, String> {
        let mut next = self.clone();
        let old = match next
            .prices
            .iter_mut()
            .find(|r| r.provider == price.provider && r.model == price.model)
        {
            Some(r) => Some(std::mem::replace(r, price)),
            None => {
                next.prices.push(price);
                None
            }
        };
        next.check()?;
        *self = next;
        Ok(old)
    }

    /// Removes a price row.
    pub fn remove_price(&mut self, provider: &str, model: &str) -> Result<Price, String> {
        let at = self
            .prices
            .iter()
            .position(|r| r.provider == provider && r.model == model)
            .ok_or_else(|| format!("no price of {model:?} for {provider:?}"))?;
        Ok(self.prices.remove(at))
    }

    /// The price of `model` at `provider`: the exact row, else the `*` row (behaviour 11).
    pub fn price(&self, provider: &str, model: &str) -> Option<&Price> {
        let of = |m: &str| {
            self.prices
                .iter()
                .find(|r| r.provider == provider && r.model == m)
        };
        of(model).or_else(|| of("*"))
    }

    /// Assigns `provider` and `model` to `task` at `now`; the earlier choice goes to the history
    /// (behaviour 9).
    pub fn assign(
        &mut self,
        task: &str,
        provider: &str,
        model: &str,
        now: u64,
    ) -> Result<Option<Choice>, String> {
        let p = self
            .provider(provider)
            .ok_or_else(|| format!("no provider {provider:?}"))?;
        if !p.enabled {
            return Err(format!("provider {provider:?} is switched off"));
        }
        if !p.models.is_empty() && !p.models.iter().any(|m| m == model) {
            return Err(format!(
                "provider {provider:?} lists no model {model:?}; list its models with model_list and add the name to its models"
            ));
        }
        let mut next = self.clone();
        let earlier = match next.assignments.iter_mut().find(|a| a.task == task) {
            Some(a) => {
                let before = Choice {
                    provider: a.provider.clone(),
                    model: a.model.clone(),
                    changed_at: a.changed_at,
                };
                a.previous.push(before.clone());
                let over = a.previous.len().saturating_sub(HISTORY);
                a.previous.drain(..over);
                a.provider = provider.to_owned();
                a.model = model.to_owned();
                a.changed_at = now;
                Some(before)
            }
            None => {
                next.assignments.push(Assignment {
                    task: task.to_owned(),
                    provider: provider.to_owned(),
                    model: model.to_owned(),
                    changed_at: now,
                    previous: Vec::new(),
                });
                None
            }
        };
        next.check()?;
        *self = next;
        Ok(earlier)
    }

    /// Removes a task's assignment.
    pub fn unassign(&mut self, task: &str) -> Result<Assignment, String> {
        let at = self
            .assignments
            .iter()
            .position(|a| a.task == task)
            .ok_or_else(|| format!("task {task:?} has no assignment"))?;
        Ok(self.assignments.remove(at))
    }

    /// The model of `task`: its own assignment, else `default`, else none (behaviour 9).
    pub fn model_for(&self, task: &str) -> Option<&Assignment> {
        let of = |t: &str| self.assignments.iter().find(|a| a.task == t);
        of(task).or_else(|| of(DEFAULT_TASK))
    }
}

fn check_model(m: &str) -> Result<(), String> {
    if m.is_empty() || m.chars().count() > MODEL_CHARS || m.chars().any(char::is_control) {
        return Err(format!(
            "a model name is 1 to {MODEL_CHARS} characters without control characters"
        ));
    }
    if looks_secret(m) {
        return Err("a model name looks like it holds a secret".into());
    }
    Ok(())
}

/// The register as loaded: a problem leaves it empty and blocks changes (behaviour 2).
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    /// The register.
    pub register: Register,
    /// Why the file could not be used.
    pub problem: Option<String>,
}

/// Refuses a path inside the workspace (behaviour 1).
pub fn outside_workspace(path: &Path, workspace: &Path) -> Result<(), String> {
    let canon = |p: &Path| {
        let mut probe = p.to_path_buf();
        let mut tail = Vec::new();
        loop {
            if let Ok(c) = probe.canonicalize() {
                return tail
                    .iter()
                    .rev()
                    .fold(c, |acc: PathBuf, t: &std::ffi::OsString| acc.join(t));
            }
            match (probe.file_name().map(ToOwned::to_owned), probe.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name);
                    probe = parent.to_path_buf();
                }
                _ => return p.to_path_buf(),
            }
        }
    };
    if canon(path).starts_with(canon(workspace)) {
        return Err(format!(
            "{} is inside the workspace; the register and the ledger live in the user's settings folder",
            path.display()
        ));
    }
    Ok(())
}

/// Reads the register; a missing file is empty (behaviours 1 and 2).
pub fn load(file: &Path, workspace: &Path) -> Loaded {
    if let Err(e) = outside_workspace(file, workspace) {
        return Loaded {
            register: Register::default(),
            problem: Some(e),
        };
    }
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::default(),
        Err(e) => {
            return Loaded {
                register: Register::default(),
                problem: Some(format!("{}: {e}", file.display())),
            };
        }
    };
    let parsed: Result<Register, String> = toml::from_str(&text).map_err(|e| {
        // The parser may quote the offending value; never repeat what looks like a secret.
        context::mask_secrets(e.message()).0
    });
    match parsed.and_then(|r| r.check().map(|()| r)) {
        Ok(register) => Loaded {
            register,
            problem: None,
        },
        Err(e) => Loaded {
            register: Register::default(),
            problem: Some(format!("{}: {e}", file.display())),
        },
    }
}

/// Writes `text` atomically with mode 0600.
fn write_private(file: &Path, text: &str) -> Result<(), String> {
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = file.with_extension(format!("tmp-{}", std::process::id()));
    {
        use std::io::Write as _;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut f = options
            .open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(text.as_bytes())
            .and_then(|()| f.sync_all())
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", file.display())
    })
}

/// Writes the register after checking it (behaviour 1).
pub fn save(file: &Path, workspace: &Path, register: &Register) -> Result<(), String> {
    outside_workspace(file, workspace)?;
    register.check()?;
    let text = toml::to_string(register).map_err(|e| e.to_string())?;
    write_private(
        file,
        &format!(
            "# Ferris IDEA model providers (SPEC-0350). Keys stay in the system keychain.\n{text}"
        ),
    )
}

/// The keychain entry a provider's key would be read from, if it has one (behaviour 5).
pub fn key_entry(provider: &Provider) -> Option<SecretRef> {
    match provider.key {
        KeySource::None => None,
        KeySource::Keychain => key_ref(&provider.endpoint).ok(),
    }
}

/// The key reference of a usage record: `none` or `keychain:<host>:<port>` (behaviour 12).
pub fn key_reference(provider: &Provider) -> String {
    key_entry(provider).map_or_else(|| "none".to_owned(), |r| format!("keychain:{}", r.account))
}

/// The provider's models, asked of its endpoint with its key from `store` (behaviour 8).
pub fn provider_models(
    provider: &Provider,
    store: &dyn SecretStore,
) -> Result<Vec<String>, AiError> {
    let key = key_entry(provider).and_then(|r| store.get(&r).ok().flatten());
    list_models(&provider.endpoint, key.as_ref().map(|k| k.expose()))
}

/// A local server found (behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalServer {
    /// Its port on the loopback address.
    pub port: u16,
    /// `ollama` or `llamacpp`.
    pub kind: Kind,
    /// The base URL to register.
    pub endpoint: String,
    /// The models it offers.
    pub models: Vec<String>,
    /// The registered provider using this endpoint, if any.
    pub registered: Option<String>,
}

fn get_json(url: &str) -> Option<serde_json::Value> {
    let request = ResolvedRequest {
        name: None,
        line: 0,
        method: "GET".into(),
        url: url.to_owned(),
        headers: vec![("Accept".into(), "application/json".into())],
        body: String::new(),
        unresolved: Vec::new(),
    };
    let response = send_once(&request, PROBE_TIMEOUT).ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    serde_json::from_str(&response.text.unwrap_or_default()).ok()
}

fn names(list: Option<&Vec<serde_json::Value>>, key: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = list?
        .iter()
        .filter_map(|m| m[key].as_str())
        .filter(|n| check_model(n).is_ok())
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    Some(out)
}

/// Asks the loopback address on `ports` (default [`LOCAL_PORTS`]) for local servers; changes
/// nothing (behaviour 7).
pub fn detect_local(ports: &[u16], register: &Register) -> Result<Vec<LocalServer>, String> {
    let ports: Vec<u16> = if ports.is_empty() {
        LOCAL_PORTS.to_vec()
    } else {
        ports.to_vec()
    };
    if ports.len() > MAX_PORTS || ports.contains(&0) {
        return Err(format!("1 to {MAX_PORTS} ports from 1 to 65535"));
    }
    let mut found = Vec::new();
    for port in ports {
        let base = format!("http://127.0.0.1:{port}");
        let (kind, models) = if let Some(m) = get_json(&format!("{base}/api/tags"))
            .and_then(|j| names(j["models"].as_array(), "name"))
        {
            (Kind::Ollama, m)
        } else if let Some(m) =
            get_json(&format!("{base}/v1/models")).and_then(|j| names(j["data"].as_array(), "id"))
        {
            (Kind::Llamacpp, m)
        } else {
            continue;
        };
        let endpoint = format!("{base}/v1");
        let registered = register
            .providers
            .iter()
            .find(|p| same_endpoint(&p.endpoint, &endpoint))
            .map(|p| p.id.clone());
        found.push(LocalServer {
            port,
            kind,
            endpoint,
            models,
            registered,
        });
    }
    Ok(found)
}

fn same_endpoint(a: &str, b: &str) -> bool {
    match (parse_url(a.trim()), parse_url(b.trim())) {
        (Ok(x), Ok(y)) => {
            let host = |h: &str| {
                if loopback(h) {
                    "loopback".to_owned()
                } else {
                    h.to_ascii_lowercase()
                }
            };
            host(&x.host) == host(&y.host)
                && x.port == y.port
                && x.path.trim_end_matches('/') == y.path.trim_end_matches('/')
        }
        _ => false,
    }
}

/// One booked call (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageRecord {
    /// When (seconds since the Unix epoch).
    pub at: u64,
    /// The provider's id.
    pub provider: String,
    /// `none` or `keychain:<host>:<port>`; never a key.
    pub key: String,
    /// The model.
    pub model: String,
    /// The task.
    pub task: String,
    /// Input tokens as reported.
    pub input_tokens: u64,
    /// Output tokens as reported.
    pub output_tokens: u64,
    /// The currency of the cost, when known.
    pub currency: Option<String>,
    /// The cost in millionths of the currency, when known.
    pub cost_micro: Option<u64>,
}

/// What a booking is asked with (behaviour 12).
#[derive(Debug, Clone)]
pub struct Booking<'a> {
    /// The provider's id.
    pub provider: &'a str,
    /// The model.
    pub model: &'a str,
    /// The task; `None` is `default`.
    pub task: Option<&'a str>,
    /// Input tokens as reported.
    pub input_tokens: u64,
    /// Output tokens as reported.
    pub output_tokens: u64,
    /// When.
    pub at: u64,
}

/// The record of `booking` with its cost from the price in force (behaviour 12).
pub fn price_booking(register: &Register, booking: &Booking<'_>) -> Result<UsageRecord, String> {
    let provider = register
        .provider(booking.provider)
        .ok_or_else(|| format!("no provider {:?}", booking.provider))?;
    check_model(booking.model)?;
    let task = booking.task.unwrap_or(DEFAULT_TASK);
    if !id_ok(task, &['.']) {
        return Err("the task is 1 to 64 characters of a-z, 0-9, -, _, .".into());
    }
    if booking.input_tokens > MAX_TOKENS || booking.output_tokens > MAX_TOKENS {
        return Err(format!("token counts are 0 to {MAX_TOKENS}"));
    }
    let (currency, cost_micro) = match register.price(booking.provider, booking.model) {
        Some(p) => {
            let input = u128::from(parse_price(&p.input_per_mtok)?);
            let output = u128::from(parse_price(&p.output_per_mtok)?);
            let total = (u128::from(booking.input_tokens) * input
                + u128::from(booking.output_tokens) * output)
                / 1_000_000;
            (
                Some(p.currency.clone()),
                Some(u64::try_from(total).unwrap_or(u64::MAX)),
            )
        }
        None => (None, None),
    };
    Ok(UsageRecord {
        at: booking.at,
        provider: booking.provider.to_owned(),
        key: key_reference(provider),
        model: booking.model.to_owned(),
        task: task.to_owned(),
        input_tokens: booking.input_tokens,
        output_tokens: booking.output_tokens,
        currency,
        cost_micro,
    })
}

/// The ledger as read: its records and the lines it could not read.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    /// The records, oldest first.
    pub records: Vec<UsageRecord>,
    /// Lines that were no record.
    pub unreadable: usize,
    /// Records dropped over the bound, as noted in the file.
    pub dropped: u64,
}

const DROPPED: &str = "# dropped ";

/// Reads the ledger; a missing file is empty (behaviour 13).
pub fn read_ledger(file: &Path) -> Ledger {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Ledger::default();
    };
    let mut ledger = Ledger::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if let Some(n) = line.strip_prefix(DROPPED) {
            ledger.dropped = n.trim().parse().unwrap_or(0);
        } else if let Ok(r) = serde_json::from_str::<UsageRecord>(line) {
            ledger.records.push(r);
        } else {
            ledger.unreadable += 1;
        }
    }
    ledger
}

/// Appends `record`; beyond `max` records the oldest are dropped (behaviour 13).
pub fn book(
    file: &Path,
    workspace: &Path,
    record: &UsageRecord,
    max: usize,
) -> Result<Ledger, String> {
    outside_workspace(file, workspace)?;
    let mut ledger = read_ledger(file);
    ledger.records.push(record.clone());
    let over = ledger.records.len().saturating_sub(max.max(1));
    ledger.records.drain(..over);
    ledger.dropped += over as u64;
    let mut text = String::new();
    if ledger.dropped > 0 {
        text.push_str(&format!("{DROPPED}{}\n", ledger.dropped));
    }
    for r in &ledger.records {
        text.push_str(&serde_json::to_string(r).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    write_private(file, &text)?;
    Ok(ledger)
}

/// How a summary groups (behaviour 14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupBy {
    /// By provider.
    #[default]
    Provider,
    /// By key reference.
    Key,
    /// By model.
    Model,
    /// By task.
    Task,
}

/// One group of a summary (behaviour 14).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UsageGroup {
    /// The provider, key reference, model or task.
    pub group: String,
    /// Calls.
    pub calls: u64,
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Cost per currency, six decimal places.
    pub cost: BTreeMap<String, String>,
    /// Calls without a known price.
    pub unpriced_calls: u64,
}

/// The records since `since`, grouped (behaviour 14).
pub fn summarize(records: &[UsageRecord], by: GroupBy, since: u64) -> Vec<UsageGroup> {
    let mut groups: BTreeMap<String, (UsageGroup, BTreeMap<String, u64>)> = BTreeMap::new();
    for r in records.iter().filter(|r| r.at >= since) {
        let name = match by {
            GroupBy::Provider => &r.provider,
            GroupBy::Key => &r.key,
            GroupBy::Model => &r.model,
            GroupBy::Task => &r.task,
        };
        let (g, costs) = groups.entry(name.clone()).or_insert_with(|| {
            (
                UsageGroup {
                    group: name.clone(),
                    ..UsageGroup::default()
                },
                BTreeMap::new(),
            )
        });
        g.calls += 1;
        g.input_tokens += r.input_tokens;
        g.output_tokens += r.output_tokens;
        match (&r.currency, r.cost_micro) {
            (Some(c), Some(m)) => *costs.entry(c.clone()).or_default() += m,
            _ => g.unpriced_calls += 1,
        }
    }
    groups
        .into_values()
        .map(|(mut g, costs)| {
            g.cost = costs.into_iter().map(|(c, m)| (c, show_micro(m))).collect();
            g
        })
        .collect()
}
