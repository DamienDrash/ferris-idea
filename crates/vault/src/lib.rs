//! The internal vault for agents, phase 1 (`SPEC-0353`, ADR-0033).
//!
//! Entries are addressed by handles; values are encrypted at rest under a data key per entry,
//! wrapped by a vault key that only the platform's key store holds ([`KeyStore`]). Nothing in
//! this crate hands a value to anything but the caller that puts it in place
//! ([`Vault::value_for_use`]); metadata never carries one. Every use, refusal and change is
//! recorded in an authenticated, chained audit log ([`audit`]); every value put in place is
//! registered for redaction ([`redact`]).

pub mod audit;
mod crypto;
pub mod generate;
pub mod redact;

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub use audit::{AuditLog, AuditRecord};
pub use generate::{Charset, Policy};

/// The service of a connection profile's secret reference that names a vault handle as its
/// account (`SPEC-0353` behaviour 14).
pub const REF_SERVICE: &str = "ferris-idea-vault";
/// The service of the key store item holding the vault key (behaviour 6); never resolved as a
/// database password (behaviour 15).
pub const KEY_SERVICE: &str = "ferris-idea-vault-key";
/// The reference service under the product's former name (`SPEC-0356` behaviour 11).
const FORMER_REF_SERVICE: &str = "idecraft-vault"; // rebrand:keep

/// Whether a connection profile's secret reference names a vault handle: its service is
/// [`REF_SERVICE`], or the service older versions wrote under the product's former name.
pub fn is_ref_service(service: &str) -> bool {
    service == REF_SERVICE || service == FORMER_REF_SERVICE
}

/// The account of the key store item.
pub const KEY_ACCOUNT: &str = "vault-key";
/// The prefix of every handle (behaviour 1).
pub const HANDLE_PREFIX: &str = "vlt_";
/// Shortest and longest value in bytes (behaviour 2).
pub const VALUE_BYTES: (usize, usize) = (8, 4096);
/// Most hosts of a scope (behaviour 7).
pub const MAX_HOSTS: usize = 16;
/// Longest expiry in days (behaviour 16).
pub const MAX_EXPIRY_DAYS: u64 = 3650;

const FILE_NAME: &str = "vault.json";
const LOCK_NAME: &str = "vault.lock";
const AUDIT_NAME: &str = "audit.jsonl";
const FORMAT: u32 = 1;

/// Why a vault operation did not happen. No variant ever carries a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// The platform's key store cannot be used; the text says why.
    KeyStore(String),
    /// The vault file or key was refused (tampered, rolled back, missing key).
    Corrupt(String),
    /// A use outside the entry's scope.
    Refused(String),
    /// The entry expired (`vault_expired`).
    Expired(String),
    /// No such entry.
    NotFound(String),
    /// Arguments out of bounds.
    Invalid(String),
    /// Reading or writing the vault folder failed.
    Io(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::KeyStore(m) => write!(f, "the vault's key store cannot be used: {m}"),
            VaultError::Corrupt(m) => write!(f, "the vault was not opened: {m}"),
            VaultError::Refused(m) => write!(f, "refused: {m}"),
            VaultError::Expired(m) => write!(f, "vault_expired: {m}"),
            VaultError::NotFound(m) => write!(f, "{m}"),
            VaultError::Invalid(m) => write!(f, "{m}"),
            VaultError::Io(m) => write!(f, "the vault folder: {m}"),
        }
    }
}

impl std::error::Error for VaultError {}

fn io(e: std::io::Error) -> VaultError {
    VaultError::Io(e.to_string())
}

/// The platform's key store holding the vault key (`SPEC-0353` behaviour 6). Implementations
/// for the macOS Keychain, the Secret Service and the Windows Credential Manager live where the
/// platform crates are; [`MemoryKeyStore`] serves tests.
pub trait KeyStore: Send + Sync {
    /// The store's name for messages and `vault_list`.
    fn name(&self) -> String;
    /// The item, or `None` when there is none.
    fn load(&self) -> Result<Option<Zeroizing<String>>, String>;
    /// Stores or replaces the item.
    fn store(&self, item: &str) -> Result<(), String>;
}

/// A key store in memory: for tests only; a vault on it is gone with the process.
#[derive(Default)]
pub struct MemoryKeyStore(Mutex<Option<Zeroizing<String>>>);

impl KeyStore for MemoryKeyStore {
    fn name(&self) -> String {
        "memory (tests)".into()
    }

    fn load(&self) -> Result<Option<Zeroizing<String>>, String> {
        Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    fn store(&self, item: &str) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Zeroizing::new(item.to_owned()));
        Ok(())
    }
}

/// An injection form (`SPEC-0353` behaviour 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Use {
    /// The password of a database connection Ferris IDEA opens.
    Database,
    /// An environment variable of a process Ferris IDEA starts.
    ProcessEnv,
}

impl Use {
    /// The form's name as written in a scope.
    pub fn name(self) -> &'static str {
        match self {
            Use::Database => "database",
            Use::ProcessEnv => "process-env",
        }
    }

    /// The form named `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "database" => Some(Use::Database),
            "process-env" => Some(Use::ProcessEnv),
            _ => None,
        }
    }
}

/// What an entry holds (`SPEC-0353` behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A password.
    Password,
    /// An API token.
    Token,
    /// Any other secret.
    Secret,
}

impl Kind {
    /// The kind named `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "password" => Some(Kind::Password),
            "token" => Some(Kind::Token),
            "secret" => Some(Kind::Secret),
            _ => None,
        }
    }
}

/// Where an entry may be used (`SPEC-0353` behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// The workspaces (resolved roots) it may be used in.
    pub projects: Vec<String>,
    /// The injection forms.
    pub uses: Vec<Use>,
    /// `host` or `host:port`, lower case, no wildcards.
    pub hosts: Vec<String>,
}

impl Scope {
    /// The scope with its hosts in lower case, or why it is out of bounds.
    pub fn checked(mut self) -> Result<Self, VaultError> {
        let bad = |m: String| Err(VaultError::Invalid(m));
        if self.projects.is_empty() {
            return bad("a scope names at least one project".into());
        }
        self.uses.sort();
        self.uses.dedup();
        if self.uses.is_empty() {
            return bad("`uses` names at least one form: database or process-env".into());
        }
        if self.hosts.len() > MAX_HOSTS {
            return bad(format!("at most {MAX_HOSTS} hosts"));
        }
        for h in &mut self.hosts {
            *h = h.trim().to_ascii_lowercase();
            if let Err(m) = check_host(h) {
                return bad(m);
            }
        }
        self.hosts.sort();
        self.hosts.dedup();
        if self.uses.contains(&Use::Database) && self.hosts.is_empty() {
            return bad("an entry for `database` names its hosts (`host` or `host:port`)".into());
        }
        Ok(self)
    }

    /// Whether the destination `host:port` matches one of the hosts (behaviour 8).
    pub fn allows_destination(&self, destination: &str) -> bool {
        let (host, port) = split_host(&destination.to_ascii_lowercase());
        self.hosts.iter().any(|h| {
            let (h_host, h_port) = split_host(h);
            h_host == host && (h_port.is_none() || h_port == port)
        })
    }
}

fn split_host(text: &str) -> (String, Option<u16>) {
    match text.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => match p.parse::<u16>() {
            Ok(port) => (h.to_owned(), Some(port)),
            Err(_) => (text.to_owned(), None),
        },
        _ => (text.to_owned(), None),
    }
}

/// Whether `text` is `host` or `host:port` with a DNS name or IPv4 address and no wildcard.
fn check_host(text: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "`{text}` is no host: write `host` or `host:port` with a name or IPv4 address (no wildcards)"
        ))
    };
    let (host, port) = match text.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(n) if n > 0 => (h, Some(n)),
            _ => return refuse(),
        },
        None => (text, None),
    };
    let _ = port;
    if host.is_empty() || host.len() > 253 {
        return refuse();
    }
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if host.split('.').all(label_ok) {
        Ok(())
    } else {
        refuse()
    }
}

/// Who created an entry or made a use: the MCP session Ferris IDEA serves, shown with the name the
/// client reported (`SPEC-0353` behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// The random id of the session; the identity.
    pub session: String,
    /// The name the client reported; shown, never trusted.
    pub client: String,
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (session {})", self.client, self.session)
    }
}

/// An entry without its value (`SPEC-0353` behaviour 1). Serialising it never yields a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMeta {
    /// `vlt_` and 26 base32 characters.
    pub handle: String,
    /// The unique name.
    pub name: String,
    /// What it holds.
    pub kind: Kind,
    /// Where it may be used.
    pub scope: Scope,
    /// Who stored or generated it.
    pub created_by: Agent,
    /// When, in seconds since the Unix epoch.
    pub created_at: u64,
    /// When it expires, if ever.
    pub expires_at: Option<u64>,
    /// Whether an agent held the value (stored rather than generated).
    pub seen_by_agent: bool,
    /// Raised when the value changes.
    pub version: u32,
}

impl EntryMeta {
    /// Whether the entry has expired at `now`.
    pub fn expired(&self, now: u64) -> bool {
        self.expires_at.is_some_and(|t| now >= t)
    }
}

/// What a new entry is (its value comes separately).
#[derive(Debug, Clone)]
pub struct NewEntry {
    /// The unique name.
    pub name: String,
    /// What it holds.
    pub kind: Kind,
    /// Where it may be used.
    pub scope: Scope,
    /// Who stores it.
    pub created_by: Agent,
    /// Expiry in days from now, if any.
    pub expires_in_days: Option<u64>,
}

/// A use about to happen (`SPEC-0353` behaviour 8).
#[derive(Debug, Clone)]
pub struct UseRequest {
    /// The resolved workspace root.
    pub project: String,
    /// The injection form.
    pub form: Use,
    /// `host:port` for a database; `None` for a process.
    pub destination: Option<String>,
}

/// Whether `meta` may be used for `request` at `now` (`SPEC-0353` behaviour 8).
pub fn check_use(meta: &EntryMeta, request: &UseRequest, now: u64) -> Result<(), VaultError> {
    let name = &meta.name;
    if meta.expired(now) {
        return Err(VaultError::Expired(format!(
            "the vault entry `{name}` expired at {}; ask the user for a new one",
            meta.expires_at.unwrap_or_default()
        )));
    }
    if !meta.scope.projects.contains(&request.project) {
        return Err(VaultError::Refused(format!(
            "the vault entry `{name}` is not for this project"
        )));
    }
    if !meta.scope.uses.contains(&request.form) {
        return Err(VaultError::Refused(format!(
            "the vault entry `{name}` may not be used for {}; it allows {}",
            request.form.name(),
            meta.scope
                .uses
                .iter()
                .map(|u| u.name())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if request.form == Use::Database {
        let dest = request.destination.as_deref().unwrap_or_default();
        if !meta.scope.allows_destination(dest) {
            return Err(VaultError::Refused(format!(
                "the vault entry `{name}` is not for {dest}; its hosts are {}",
                meta.scope.hosts.join(", ")
            )));
        }
    }
    Ok(())
}

/// Whether `handle` has the form of a handle.
pub fn is_handle(handle: &str) -> bool {
    handle.strip_prefix(HANDLE_PREFIX).is_some_and(|rest| {
        rest.len() == 26
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    })
}

fn new_handle() -> Result<String, VaultError> {
    let bytes = crypto::random::<16>()?;
    Ok(format!(
        "{HANDLE_PREFIX}{}",
        data_encoding::BASE32_NOPAD
            .encode(&bytes)
            .to_ascii_lowercase()
    ))
}

fn check_name(name: &str) -> Result<(), VaultError> {
    let ok = !name.is_empty()
        && name.chars().count() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(VaultError::Invalid(format!(
            "`{name}` is no entry name: 1 to 64 characters of A-Z, a-z, 0-9, ., _, -"
        )))
    }
}

fn check_value(value: &str) -> Result<(), VaultError> {
    let (min, max) = VALUE_BYTES;
    if value.len() < min || value.len() > max {
        return Err(VaultError::Invalid(format!(
            "a value has {min} to {max} bytes (shorter values cannot be redacted reliably); nothing was stored"
        )));
    }
    if value.contains('\0') {
        return Err(VaultError::Invalid(
            "a value holds no NUL character; nothing was stored".into(),
        ));
    }
    Ok(())
}

/// One entry as stored: metadata in clear, data key and value encrypted (behaviour 5).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEntry {
    #[serde(flatten)]
    meta: EntryMeta,
    key_nonce: String,
    key: String,
    value_nonce: String,
    value: String,
}

/// The vault file (behaviour 5).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultFile {
    format: u32,
    aead: String,
    kdf: String,
    mac_alg: String,
    vault_id: String,
    counter: u64,
    entries: Vec<StoredEntry>,
    mac: String,
}

/// The key store item (behaviour 6).
#[derive(Serialize, Deserialize)]
struct KeyItem {
    format: u32,
    vault_id: String,
    key: String,
    counter: u64,
}

/// The vault key and what goes with it.
struct Keys {
    vk: Zeroizing<[u8; 32]>,
    vault_id: [u8; 16],
    counter: u64,
}

/// The vault open under its lock.
struct Opened {
    file: VaultFile,
    keys: Keys,
    _lock: fs::File,
}

/// The vault of one user (`SPEC-0353`).
#[derive(Clone)]
pub struct Vault {
    dir: PathBuf,
    keys: Arc<dyn KeyStore>,
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault")
            .field("dir", &self.dir)
            .field("keys", &self.keys.name())
            .finish()
    }
}

impl Vault {
    /// The vault in `dir` with its key in `keys`. Nothing is read or made until an operation.
    pub fn new(dir: impl Into<PathBuf>, keys: Arc<dyn KeyStore>) -> Self {
        Self {
            dir: dir.into(),
            keys,
        }
    }

    /// The vault folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The key store's name.
    pub fn key_store_name(&self) -> String {
        self.keys.name()
    }

    /// Every entry, without values; empty when no vault exists yet.
    pub fn list(&self) -> Result<Vec<EntryMeta>, VaultError> {
        Ok(self
            .open(false)?
            .map(|o| o.file.entries.into_iter().map(|e| e.meta).collect())
            .unwrap_or_default())
    }

    /// The entry with `handle`, without its value.
    pub fn entry(&self, handle: &str) -> Result<EntryMeta, VaultError> {
        self.list()?
            .into_iter()
            .find(|e| e.handle == handle)
            .ok_or_else(|| not_found(handle))
    }

    /// Stores `value` as a new entry an agent held (`seen_by_agent`) and answers its metadata
    /// (behaviour 16).
    pub fn put(&self, new: NewEntry, value: &str, now: u64) -> Result<EntryMeta, VaultError> {
        check_value(value)?;
        self.insert(new, value, true, now)
    }

    /// Generates a value inside the vault for a new entry and answers its metadata
    /// (behaviour 16).
    pub fn generate(
        &self,
        new: NewEntry,
        policy: Policy,
        now: u64,
    ) -> Result<EntryMeta, VaultError> {
        let value = generate::generate(policy)?;
        self.insert(new, &value, false, now)
    }

    fn insert(
        &self,
        new: NewEntry,
        value: &str,
        seen_by_agent: bool,
        now: u64,
    ) -> Result<EntryMeta, VaultError> {
        check_name(&new.name)?;
        let scope = new.scope.checked()?;
        if let Some(days) = new.expires_in_days
            && !(1..=MAX_EXPIRY_DAYS).contains(&days)
        {
            return Err(VaultError::Invalid(format!(
                "expires_in_days is 1 to {MAX_EXPIRY_DAYS}"
            )));
        }
        let mut opened = self
            .open(true)?
            .ok_or_else(|| VaultError::Corrupt("the vault could not be created".into()))?;
        if opened.file.entries.iter().any(|e| e.meta.name == new.name) {
            return Err(VaultError::Invalid(format!(
                "a vault entry `{}` exists; choose another name (replacing a value needs the user)",
                new.name
            )));
        }
        let meta = EntryMeta {
            handle: new_handle()?,
            name: new.name,
            kind: new.kind,
            scope,
            created_by: new.created_by,
            created_at: now,
            expires_at: new.expires_in_days.map(|d| now + d * 86_400),
            seen_by_agent,
            version: 1,
        };
        let stored = crypto::seal_entry(&opened.keys, meta.clone(), value)?;
        opened.file.entries.push(stored);
        self.commit(&mut opened)?;
        Ok(meta)
    }

    /// Removes the entry with `handle`: its data key and value are gone from the file at once
    /// (behaviour 19).
    pub fn remove(&self, handle: &str) -> Result<EntryMeta, VaultError> {
        let mut opened = self.open(false)?.ok_or_else(|| not_found(handle))?;
        let i = opened
            .file
            .entries
            .iter()
            .position(|e| e.meta.handle == handle)
            .ok_or_else(|| not_found(handle))?;
        let removed = opened.file.entries.remove(i);
        self.commit(&mut opened)?;
        Ok(removed.meta)
    }

    /// The value of `handle` for `request`, checked against the entry's scope at this moment
    /// (behaviour 8). Only the code that puts the value in place calls this; register the value
    /// for redaction ([`redact::register`]) before it reaches anything that writes output.
    pub fn value_for_use(
        &self,
        handle: &str,
        request: &UseRequest,
        now: u64,
    ) -> Result<(EntryMeta, Zeroizing<String>), VaultError> {
        let opened = self.open(false)?.ok_or_else(|| not_found(handle))?;
        let stored = opened
            .file
            .entries
            .iter()
            .find(|e| e.meta.handle == handle)
            .ok_or_else(|| not_found(handle))?;
        check_use(&stored.meta, request, now)?;
        let value = crypto::open_entry(&opened.keys, stored)?;
        Ok((stored.meta.clone(), value))
    }

    /// Appends `record` to the audit log, chained and authenticated (behaviour 20). Its `seq`,
    /// `prev` and `mac` are set here. Without a vault there is nothing to record.
    pub fn record(&self, record: AuditRecord) -> Result<(), VaultError> {
        let Some(opened) = self.open(false)? else {
            return Ok(());
        };
        audit::append(&self.dir.join(AUDIT_NAME), &opened.keys, record)
    }

    /// The audit log and whether its chain is intact (behaviour 20).
    pub fn audit(&self) -> Result<AuditLog, VaultError> {
        match self.open(false)? {
            Some(opened) => audit::read(&self.dir.join(AUDIT_NAME), &opened.keys),
            None => Ok(AuditLog::default()),
        }
    }

    /// Opens the vault under its lock; `None` when none exists and `create` is false.
    fn open(&self, create: bool) -> Result<Option<Opened>, VaultError> {
        let lock = self.lock()?;
        let path = self.dir.join(FILE_NAME);
        let text = read_private(&path)?;
        let item = self.keys.load().map_err(VaultError::KeyStore)?;
        let keys = match &item {
            Some(item) => Some(parse_key_item(item)?),
            None => None,
        };
        match (text, keys) {
            (None, None) if !create => Ok(None),
            (None, None) => {
                let keys = Keys {
                    vk: Zeroizing::new(crypto::random::<32>()?),
                    vault_id: crypto::random::<16>()?,
                    counter: 0,
                };
                self.store_keys(&keys)?;
                Ok(Some(Opened {
                    file: empty_file(&keys),
                    keys,
                    _lock: lock,
                }))
            }
            (Some(_), None) => Err(VaultError::Corrupt(format!(
                "{} exists but its key is not in the {}; nothing can be decrypted",
                path.display(),
                self.keys.name()
            ))),
            (None, Some(keys)) if keys.counter == 0 => {
                if create {
                    Ok(Some(Opened {
                        file: empty_file(&keys),
                        keys,
                        _lock: lock,
                    }))
                } else {
                    Ok(None)
                }
            }
            (None, Some(_)) => Err(VaultError::Corrupt(format!(
                "{} is missing while its key says entries were written: deleted or restored from a backup?",
                path.display()
            ))),
            (Some(text), Some(keys)) => {
                let file: VaultFile = serde_json::from_str(&text).map_err(|e| {
                    VaultError::Corrupt(format!("{} cannot be read: {e}", path.display()))
                })?;
                crypto::verify_file(&keys, &file)?;
                if file.counter < keys.counter {
                    return Err(VaultError::Corrupt(format!(
                        "{} is older than its key (restored from a backup?); it was not opened",
                        path.display()
                    )));
                }
                Ok(Some(Opened {
                    file,
                    keys,
                    _lock: lock,
                }))
            }
        }
    }

    /// Writes the file with a raised counter, then the key store's counter (behaviours 4–6).
    fn commit(&self, opened: &mut Opened) -> Result<(), VaultError> {
        opened.file.counter = opened.file.counter.max(opened.keys.counter) + 1;
        opened.file.mac = crypto::file_mac(&opened.keys, &opened.file)?;
        let text = serde_json::to_string_pretty(&opened.file)
            .map_err(|e| VaultError::Io(e.to_string()))?;
        write_private(&self.dir.join(FILE_NAME), text.as_bytes())?;
        opened.keys.counter = opened.file.counter;
        self.store_keys(&opened.keys)
    }

    fn store_keys(&self, keys: &Keys) -> Result<(), VaultError> {
        let item = KeyItem {
            format: FORMAT,
            vault_id: data_encoding::HEXLOWER.encode(&keys.vault_id),
            key: data_encoding::HEXLOWER.encode(keys.vk.as_ref()),
            counter: keys.counter,
        };
        let text = Zeroizing::new(
            serde_json::to_string(&item).map_err(|e| VaultError::Io(e.to_string()))?,
        );
        let KeyItem { key, .. } = item;
        drop(Zeroizing::new(key));
        self.keys.store(&text).map_err(VaultError::KeyStore)
    }

    /// The folder (0700) and the exclusive lock on `vault.lock` (behaviour 4).
    fn lock(&self) -> Result<fs::File, VaultError> {
        private_dir(&self.dir)?;
        let path = self.dir.join(LOCK_NAME);
        refuse_link(&path)?;
        let mut options = fs::OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(io)?;
        file.lock().map_err(io)?;
        Ok(file)
    }
}

fn not_found(handle: &str) -> VaultError {
    VaultError::NotFound(format!("there is no vault entry `{handle}`"))
}

fn empty_file(keys: &Keys) -> VaultFile {
    VaultFile {
        format: FORMAT,
        aead: "xchacha20poly1305".into(),
        kdf: "hkdf-sha256".into(),
        mac_alg: "hmac-sha256".into(),
        vault_id: data_encoding::HEXLOWER.encode(&keys.vault_id),
        counter: 0,
        entries: Vec::new(),
        mac: String::new(),
    }
}

fn parse_key_item(text: &str) -> Result<Keys, VaultError> {
    let bad = || VaultError::Corrupt("the vault's key store item cannot be read".into());
    let item: KeyItem = serde_json::from_str(text).map_err(|_| bad())?;
    let key = Zeroizing::new(item.key);
    if item.format != FORMAT {
        return Err(bad());
    }
    let vk_bytes = Zeroizing::new(
        data_encoding::HEXLOWER
            .decode(key.as_bytes())
            .map_err(|_| bad())?,
    );
    let id_bytes = data_encoding::HEXLOWER
        .decode(item.vault_id.as_bytes())
        .map_err(|_| bad())?;
    let mut vk = Zeroizing::new([0u8; 32]);
    let mut vault_id = [0u8; 16];
    if vk_bytes.len() != 32 || id_bytes.len() != 16 {
        return Err(bad());
    }
    vk.copy_from_slice(&vk_bytes);
    vault_id.copy_from_slice(&id_bytes);
    Ok(Keys {
        vk,
        vault_id,
        counter: item.counter,
    })
}

/// Refuses a symbolic link where a vault file or folder belongs.
fn refuse_link(path: &Path) -> Result<(), VaultError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(VaultError::Io(format!(
            "{} is a symbolic link; the vault does not follow links",
            path.display()
        ))),
        _ => Ok(()),
    }
}

fn private_dir(dir: &Path) -> Result<(), VaultError> {
    refuse_link(dir)?;
    if !dir.is_dir() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(dir).map_err(io)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    Ok(())
}

fn read_private(path: &Path) -> Result<Option<String>, VaultError> {
    refuse_link(path)?;
    match fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(e)),
    }
}

/// Writes `bytes` to a temporary file (0600) beside `path` and renames it (behaviour 4).
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    refuse_link(path)?;
    let tmp = path.with_extension(format!(
        "tmp-{}",
        data_encoding::HEXLOWER.encode(&crypto::random::<6>()?)
    ));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut f = options.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(io)
}

/// Opens (creating, 0600) the file at `path` for appending.
fn append_private(path: &Path) -> Result<fs::File, VaultError> {
    refuse_link(path)?;
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0356 AK-11: a profile saved by an older version still names a vault handle.
    #[test]
    fn the_reference_service_is_recognised_under_both_names() {
        assert!(is_ref_service(REF_SERVICE));
        assert!(is_ref_service(FORMER_REF_SERVICE));
        assert!(!is_ref_service(KEY_SERVICE));
        assert_eq!(KEY_SERVICE, format!("{REF_SERVICE}-key"));
    }

    fn agent() -> Agent {
        Agent {
            session: "s1".into(),
            client: "test-client".into(),
        }
    }

    fn scope(uses: &[Use], hosts: &[&str]) -> Scope {
        Scope {
            projects: vec!["/work/shop".into()],
            uses: uses.to_vec(),
            hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
        }
    }

    fn new_entry(name: &str, s: Scope) -> NewEntry {
        NewEntry {
            name: name.into(),
            kind: Kind::Password,
            scope: s,
            created_by: agent(),
            expires_in_days: None,
        }
    }

    fn vault() -> (tempfile::TempDir, Vault, Arc<MemoryKeyStore>) {
        let dir = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeyStore::default());
        let v = Vault::new(dir.path().join("vault"), keys.clone());
        (dir, v, keys)
    }

    fn db_use(dest: &str) -> UseRequest {
        UseRequest {
            project: "/work/shop".into(),
            form: Use::Database,
            destination: Some(dest.into()),
        }
    }

    /// A value made for the test, not a real credential.
    fn dummy(tag: &str) -> String {
        format!("dummy-{tag}-0123456789")
    }

    /// SPEC-0353 AK-1: entries survive, the file holds no value, a value decrypts only for its
    /// own entry, and tampering, rollback, a missing or foreign key are refused.
    #[test]
    fn entries_are_encrypted_bound_and_tamper_evident() {
        let (_d, v, keys) = vault();
        assert!(v.list().unwrap().is_empty(), "no vault yet");
        let a = v
            .put(
                new_entry("shop-db", scope(&[Use::Database], &["127.0.0.1:5432"])),
                &dummy("a"),
                100,
            )
            .unwrap();
        let b = v
            .put(
                new_entry("other-db", scope(&[Use::Database], &["db.example.test"])),
                &dummy("b"),
                100,
            )
            .unwrap();
        assert!(is_handle(&a.handle), "{}", a.handle);
        assert!(a.seen_by_agent);
        let again = Vault::new(v.dir().to_path_buf(), keys.clone());
        assert_eq!(again.list().unwrap(), vec![a.clone(), b.clone()]);
        let text = fs::read_to_string(v.dir().join(FILE_NAME)).unwrap();
        assert!(
            !text.contains(&dummy("a")) && !text.contains(&dummy("b")),
            "no plain value"
        );
        let (_, value) = again
            .value_for_use(&a.handle, &db_use("127.0.0.1:5432"), 101)
            .unwrap();
        assert_eq!(value.as_str(), dummy("a"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(v.dir()), 0o700);
            assert_eq!(mode(&v.dir().join(FILE_NAME)), 0o600);
        }

        // Swapping the encrypted values of two entries: authentication of the file fails, and
        // with a valid file MAC the associated data still binds each value to its entry.
        let mut file: VaultFile = serde_json::from_str(&text).unwrap();
        let (va, vb) = (file.entries[0].value.clone(), file.entries[1].value.clone());
        file.entries[0].value = vb;
        file.entries[1].value = va;
        let opened = v.open(false).unwrap().unwrap();
        file.mac = crypto::file_mac(&opened.keys, &file).unwrap();
        drop(opened);
        fs::write(
            v.dir().join(FILE_NAME),
            serde_json::to_string(&file).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            v.value_for_use(&a.handle, &db_use("127.0.0.1:5432"), 101),
            Err(VaultError::Corrupt(_))
        ));

        // A changed byte without a new MAC.
        let mut changed = text.clone();
        changed = changed.replacen("shop-db", "shop-dc", 1);
        fs::write(v.dir().join(FILE_NAME), &changed).unwrap();
        assert!(matches!(v.list(), Err(VaultError::Corrupt(_))));

        // Rollback: an older file than the key store's counter.
        fs::write(v.dir().join(FILE_NAME), &text).unwrap();
        v.put(
            new_entry("third", scope(&[Use::ProcessEnv], &[])),
            &dummy("c"),
            102,
        )
        .unwrap();
        fs::write(v.dir().join(FILE_NAME), &text).unwrap();
        let err = v.list().unwrap_err();
        assert!(err.to_string().contains("older than its key"), "{err}");

        // A missing key, and a key of another vault.
        let empty = Vault::new(v.dir().to_path_buf(), Arc::new(MemoryKeyStore::default()));
        assert!(empty.list().unwrap_err().to_string().contains("not in the"));
        let (_d2, other, other_keys) = vault();
        other
            .put(
                new_entry("x", scope(&[Use::ProcessEnv], &[])),
                &dummy("x"),
                1,
            )
            .unwrap();
        let foreign = Vault::new(v.dir().to_path_buf(), other_keys);
        assert!(matches!(foreign.list(), Err(VaultError::Corrupt(_))));
    }

    /// SPEC-0353 AK-1 (behaviour 6): a deleted file is refused once entries were written.
    #[test]
    fn a_missing_file_is_refused_once_entries_were_written() {
        let (_d, v, _keys) = vault();
        v.put(
            new_entry("a", scope(&[Use::ProcessEnv], &[])),
            &dummy("a"),
            1,
        )
        .unwrap();
        fs::remove_file(v.dir().join(FILE_NAME)).unwrap();
        assert!(v.list().unwrap_err().to_string().contains("missing"));
    }

    /// SPEC-0353 AK-2: scope checks and bounds.
    #[test]
    fn uses_pass_only_inside_the_scope() {
        let (_d, v, _keys) = vault();
        let mut e = new_entry(
            "db",
            scope(&[Use::Database], &["DB.Example.test", "127.0.0.1:5432"]),
        );
        e.expires_in_days = Some(1);
        let m = v.put(e, &dummy("db"), 1000).unwrap();
        assert_eq!(m.scope.hosts, vec!["127.0.0.1:5432", "db.example.test"]);
        assert!(check_use(&m, &db_use("db.example.test:6543"), 1001).is_ok());
        assert!(check_use(&m, &db_use("127.0.0.1:5432"), 1001).is_ok());
        assert!(check_use(&m, &db_use("127.0.0.1:5433"), 1001).is_err());
        assert!(check_use(&m, &db_use("evil.example.test:5432"), 1001).is_err());
        let mut other_project = db_use("127.0.0.1:5432");
        other_project.project = "/work/other".into();
        assert!(check_use(&m, &other_project, 1001).is_err());
        let env = UseRequest {
            project: "/work/shop".into(),
            form: Use::ProcessEnv,
            destination: None,
        };
        assert!(matches!(
            check_use(&m, &env, 1001),
            Err(VaultError::Refused(_))
        ));
        let expired = check_use(&m, &db_use("127.0.0.1:5432"), 1000 + 86_400).unwrap_err();
        assert!(
            expired.to_string().starts_with("vault_expired:"),
            "{expired}"
        );

        // Bounds: names, values, hosts, duplicate names.
        let bad_name = v.put(
            new_entry("a b", scope(&[Use::ProcessEnv], &[])),
            &dummy("n"),
            1,
        );
        assert!(matches!(bad_name, Err(VaultError::Invalid(_))));
        let short = v.put(
            new_entry("short", scope(&[Use::ProcessEnv], &[])),
            "1234567",
            1,
        );
        assert!(matches!(short, Err(VaultError::Invalid(_))));
        for host in ["*.example.test", "a b", "host:0", "host:99999", ""] {
            let r = v.put(
                new_entry("h", scope(&[Use::Database], &[host])),
                &dummy("h"),
                1,
            );
            assert!(matches!(r, Err(VaultError::Invalid(_))), "{host}");
        }
        let no_hosts = v.put(new_entry("h", scope(&[Use::Database], &[])), &dummy("h"), 1);
        assert!(matches!(no_hosts, Err(VaultError::Invalid(_))));
        let dup = v.put(
            new_entry("db", scope(&[Use::ProcessEnv], &[])),
            &dummy("d"),
            1,
        );
        assert!(dup.unwrap_err().to_string().contains("exists"));
    }

    /// SPEC-0353 behaviour 19: removing takes the value out of the file.
    #[test]
    fn removing_drops_the_entry() {
        let (_d, v, _keys) = vault();
        let m = v
            .generate(
                new_entry("gen", scope(&[Use::ProcessEnv], &[])),
                Policy::default(),
                1,
            )
            .unwrap();
        assert!(!m.seen_by_agent);
        v.remove(&m.handle).unwrap();
        assert!(v.list().unwrap().is_empty());
        assert!(matches!(v.remove(&m.handle), Err(VaultError::NotFound(_))));
    }
}
