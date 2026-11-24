//! The shared core of the Ferris IDEA mobile apps (`ADR-0029`, `SPEC-0326`): pair with a machine
//! running `ferris-idea serve --pair`, then list and read the files of its project.
//!
//! The channel and the session are the desktop's (`ferris-idea-pairing`, `ferris-idea-session`); only
//! the storage differs. Keys and pins never touch a file here: they go through [`SecretVault`],
//! which the app implements with the platform keychain.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ferris_idea_pairing::{
    Identity, PairError, PendingPairing, connect_pinned, fingerprint, parse_link, start_pairing,
    start_pairing_with_link,
};
use ferris_idea_session::{ClientSession, ErrorCode, RpcError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use zeroize::Zeroizing;

uniffi::setup_scaffolding!();

/// The largest file the app opens; a larger one is refused before it is read.
pub const MAX_FILE_BYTES: u64 = 8 << 20;
/// A file that does not arrive in this time ends the read.
const STREAM_LIMIT: Duration = Duration::from_secs(60);
const IDENTITY: &str = "identity";
const PINS: &str = "pins";

/// Why the keychain refused. Never carries a secret.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum VaultError {
    /// The platform store failed.
    #[error("{reason}")]
    Failed {
        /// A status code or short description.
        reason: String,
    },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for VaultError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Failed { reason: e.reason }
    }
}

/// The platform's store for this device's key and its pins, implemented by the app: the iOS
/// Keychain, on Android the Keystore. Values must stay on this device and out of backups.
#[uniffi::export(with_foreign)]
pub trait SecretVault: Send + Sync {
    /// The value called `name`, if there is one.
    fn load(&self, name: String) -> Result<Option<Vec<u8>>, VaultError>;
    /// Stores `value` under `name`, replacing an older one.
    fn save(&self, name: String, value: Vec<u8>) -> Result<(), VaultError>;
    /// Removes `name`; removing nothing is not an error.
    fn delete(&self, name: String) -> Result<(), VaultError>;
}

/// What can go wrong; every message says what to do.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum MobileError {
    /// The keychain refused or holds something unreadable.
    #[error("the device's key store: {0}")]
    Vault(String),
    /// The address cannot be used.
    #[error("{0}")]
    Address(String),
    /// Already paired with this address.
    #[error("{0} is already paired; forget it first to pair again")]
    AlreadyPaired(String),
    /// Not paired with this address.
    #[error("{0}: not paired yet; pair with this host first")]
    NotPaired(String),
    /// Pairing or connecting failed.
    #[error("{0}")]
    Pairing(String),
    /// The host's key differs from the pinned one.
    #[error("{0}")]
    ChangedKey(String),
    /// The other side declined.
    #[error("the pairing was declined; nothing was stored")]
    Declined,
    /// This pairing has already been confirmed or declined.
    #[error("this pairing is already finished")]
    Finished,
    /// The host answered with an error.
    #[error("{0}")]
    Remote(String),
    /// The connection is gone.
    #[error("the connection to the host is lost: {0}. Open the project again.")]
    Disconnected(String),
    /// Only regular files can be opened.
    #[error("{0} is not a regular file")]
    NotAFile(String),
    /// The file is over [`MAX_FILE_BYTES`].
    #[error("{path} has {size} bytes; the app opens files up to {MAX_FILE_BYTES} bytes")]
    TooLarge {
        /// Path in the project.
        path: String,
        /// Its size.
        size: u64,
    },
}

impl From<VaultError> for MobileError {
    fn from(e: VaultError) -> Self {
        Self::Vault(e.to_string())
    }
}

impl From<PairError> for MobileError {
    fn from(e: PairError) -> Self {
        match e {
            PairError::ChangedKey(_) | PairError::WrongKey(_) => Self::ChangedKey(e.to_string()),
            PairError::NotPaired(a) => Self::NotPaired(a),
            PairError::Declined => Self::Declined,
            other => Self::Pairing(other.to_string()),
        }
    }
}

impl From<RpcError> for MobileError {
    fn from(e: RpcError) -> Self {
        match e.code {
            ErrorCode::Unavailable => Self::Disconnected(e.message),
            _ => Self::Remote(format!("{}: {}", e.code.as_str(), e.message)),
        }
    }
}

/// A host this device is paired with. Never carries a key.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairedHostInfo {
    /// `host:port`.
    pub address: String,
    /// The fingerprint of its key.
    pub fingerprint: String,
}

/// What an entry of a folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EntryKind {
    /// A folder.
    Directory,
    /// A regular file.
    File,
    /// A symbolic link.
    Link,
}

/// One entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RemoteEntry {
    /// File name.
    pub name: String,
    /// Path relative to the project.
    pub path: String,
    /// What it is.
    pub kind: EntryKind,
    /// Size in bytes.
    pub size: u64,
}

/// A file read from the project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RemoteFile {
    /// Path relative to the project.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// The text; `None` when the file is not UTF-8.
    pub text: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Pins {
    hosts: BTreeMap<String, String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The canonical `host:port` of a typed address. A phone has no `ssh`, so the SSH path of the
/// desktop is refused and named instead of being misread as a host name.
pub fn normalize_address(text: &str) -> Result<String, MobileError> {
    let bad = |why: &str| MobileError::Address(format!("{text}: {why}"));
    let bare = text.trim();
    let bare = bare.strip_prefix("paired://").unwrap_or(bare);
    if bare.ends_with("@ssh") {
        return Err(bad(
            "the app cannot use the SSH path; use host:port of a listener started with \
             --allow-network",
        ));
    }
    if bare.is_empty() {
        return Err(bad("enter host:port"));
    }
    let (host, port) = if let Some(rest) = bare.strip_prefix('[') {
        let (inner, after) = rest
            .split_once(']')
            .ok_or_else(|| bad("the closing ] is missing"))?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p),
            None if after.is_empty() => None,
            None => return Err(bad("expected host:port")),
        };
        if inner.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(bad("not an IPv6 address"));
        }
        (format!("[{inner}]"), port)
    } else {
        match bare.split_once(':') {
            Some((_, p)) if p.contains(':') => {
                return Err(bad("write an IPv6 address as [address]:port"));
            }
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (bare.to_owned(), None),
        }
    };
    let name_ok = host.starts_with('[')
        || (!host.is_empty()
            && !host.starts_with('-')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'));
    if !name_ok {
        return Err(bad("not a host name"));
    }
    let port = match port {
        Some(p) => p
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| bad("the port must be a number from 1 to 65535"))?,
        None => ferris_idea_pairing::DEFAULT_PORT,
    };
    Ok(format!("{host}:{port}"))
}

/// The app's entry point: one per process, with the platform's keychain behind it.
#[derive(uniffi::Object)]
pub struct MobileCore {
    vault: Arc<dyn SecretVault>,
    device: String,
    pins: Mutex<()>,
}

#[uniffi::export]
impl MobileCore {
    /// A core storing in `vault`; `device_name` is what the host shows for this phone.
    #[uniffi::constructor]
    pub fn new(vault: Arc<dyn SecretVault>, device_name: String) -> Arc<Self> {
        Arc::new(Self {
            vault,
            device: device_name,
            pins: Mutex::new(()),
        })
    }

    /// Pairs with the listener at `address` using its short `code`, up to the six digits.
    pub fn begin_pairing(
        self: Arc<Self>,
        address: String,
        code: String,
    ) -> Result<Arc<PairingInProgress>, MobileError> {
        let address = normalize_address(&address)?;
        self.refuse_if_paired(&address)?;
        let identity = self.identity()?;
        let pending = start_pairing(&address, code.trim(), &identity, &self.device)?;
        Ok(PairingInProgress::new(self, address, pending))
    }

    /// Pairs from an `ferris-idea-pair://` link; the host's key must equal the link's.
    pub fn begin_pairing_link(
        self: Arc<Self>,
        link: String,
    ) -> Result<Arc<PairingInProgress>, MobileError> {
        let parsed = parse_link(&link)
            .ok_or_else(|| MobileError::Address("this is not a pairing link".to_owned()))?;
        let address = normalize_address(&parsed.address)?;
        self.refuse_if_paired(&address)?;
        let identity = self.identity()?;
        let pending = start_pairing_with_link(&parsed, &identity, &self.device)?;
        Ok(PairingInProgress::new(self, address, pending))
    }

    /// The hosts this device is paired with.
    pub fn hosts(&self) -> Result<Vec<PairedHostInfo>, MobileError> {
        let _guard = lock(&self.pins);
        let pins = self.load_pins()?;
        Ok(pins
            .hosts
            .iter()
            .filter_map(|(address, key)| {
                Some(PairedHostInfo {
                    address: address.clone(),
                    fingerprint: fingerprint(&unhex32(key)?),
                })
            })
            .collect())
    }

    /// Removes the pin of `address`; `false` when it was not paired.
    pub fn forget_host(&self, address: String) -> Result<bool, MobileError> {
        let address = normalize_address(&address)?;
        let _guard = lock(&self.pins);
        let mut pins = self.load_pins()?;
        let removed = pins.hosts.remove(&address).is_some();
        if removed {
            self.save_pins(&pins)?;
        }
        Ok(removed)
    }

    /// Opens `folder`, the folder the host serves, on the paired host at `address`.
    pub fn open_project(
        &self,
        address: String,
        folder: String,
    ) -> Result<Arc<RemoteProject>, MobileError> {
        let address = normalize_address(&address)?;
        let pinned = {
            let _guard = lock(&self.pins);
            let pins = self.load_pins()?;
            let key = pins
                .hosts
                .get(&address)
                .ok_or_else(|| MobileError::NotPaired(address.clone()))?;
            unhex32(key)
                .ok_or_else(|| MobileError::Vault(format!("the pin of {address} is damaged")))?
        };
        let identity = self.identity()?;
        // A pin exists only after the host said it stored this device with its rights
        // (`SPEC-0321` behaviour 17), so a refusal is reported at once, also right after pairing.
        let session = self.open_session(&address, &folder, &pinned, &identity)?;
        Ok(Arc::new(RemoteProject {
            session: Mutex::new(session),
        }))
    }
}

impl MobileCore {
    fn open_session(
        &self,
        address: &str,
        folder: &str,
        pinned: &[u8; 32],
        identity: &Identity,
    ) -> Result<ClientSession, MobileError> {
        let transport = connect_pinned(address, pinned, identity, &self.device)?;
        Ok(ClientSession::open(
            transport,
            folder,
            concat!("ferris-idea-mobile ", env!("CARGO_PKG_VERSION")),
            &format!("paired://{address}"),
            None,
        )?)
    }

    /// This device's key, created on first use. A stored key that cannot be read is an error:
    /// replacing it silently would drop every pairing.
    fn identity(&self) -> Result<Identity, MobileError> {
        let _guard = lock(&self.pins);
        if let Some(stored) = self.vault.load(IDENTITY.to_owned())? {
            let stored = Zeroizing::new(stored);
            if stored.len() != 64 {
                return Err(MobileError::Vault(
                    "this device's key is damaged; reinstall the app and pair again".to_owned(),
                ));
            }
            return Identity::from_bytes(&stored[..32], &stored[32..])
                .ok_or_else(|| MobileError::Vault("this device's key is damaged".to_owned()));
        }
        let identity = Identity::generate().map_err(|e| MobileError::Vault(e.to_string()))?;
        let mut bytes = Zeroizing::new(Vec::with_capacity(64));
        bytes.extend_from_slice(&identity.public);
        bytes.extend_from_slice(identity.private.as_ref());
        self.vault.save(IDENTITY.to_owned(), bytes.to_vec())?;
        Ok(identity)
    }

    fn load_pins(&self) -> Result<Pins, MobileError> {
        match self.vault.load(PINS.to_owned())? {
            None => Ok(Pins::default()),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| MobileError::Vault("the list of paired hosts is damaged".to_owned())),
        }
    }

    fn save_pins(&self, pins: &Pins) -> Result<(), MobileError> {
        let bytes = serde_json::to_vec(pins).map_err(|e| MobileError::Vault(e.to_string()))?;
        Ok(self.vault.save(PINS.to_owned(), bytes)?)
    }

    fn refuse_if_paired(&self, address: &str) -> Result<(), MobileError> {
        let _guard = lock(&self.pins);
        if self.load_pins()?.hosts.contains_key(address) {
            return Err(MobileError::AlreadyPaired(address.to_owned()));
        }
        Ok(())
    }

    fn pin(&self, address: &str, key: &[u8; 32]) -> Result<(), MobileError> {
        let _guard = lock(&self.pins);
        let mut pins = self.load_pins()?;
        pins.hosts.insert(address.to_owned(), hex(key));
        self.save_pins(&pins)?;
        Ok(())
    }
}

/// A pairing after its handshake: the digits are known, nothing is stored yet.
#[derive(uniffi::Object)]
pub struct PairingInProgress {
    core: Arc<MobileCore>,
    address: String,
    digits: String,
    host_fingerprint: String,
    pending: Mutex<Option<PendingPairing>>,
}

impl PairingInProgress {
    fn new(core: Arc<MobileCore>, address: String, pending: PendingPairing) -> Arc<Self> {
        Arc::new(Self {
            core,
            address,
            digits: pending.digits().to_owned(),
            host_fingerprint: pending.host_fingerprint(),
            pending: Mutex::new(Some(pending)),
        })
    }
}

#[uniffi::export]
impl PairingInProgress {
    /// The six digits to compare with the ones the host shows.
    pub fn digits(&self) -> String {
        self.digits.clone()
    }

    /// The fingerprint of the host's key.
    pub fn host_fingerprint(&self) -> String {
        self.host_fingerprint.clone()
    }

    /// The address that will be pinned.
    pub fn address(&self) -> String {
        self.address.clone()
    }

    /// The user compared the digits: waits for the host's confirmation, then pins its key.
    pub fn confirm(&self) -> Result<String, MobileError> {
        let pending = lock(&self.pending).take().ok_or(MobileError::Finished)?;
        let key = pending.confirm_for_pin()?;
        self.core.pin(&self.address, &key)?;
        Ok(fingerprint(&key))
    }

    /// The user declined: nothing is stored on either side.
    pub fn decline(&self) {
        if let Some(pending) = lock(&self.pending).take() {
            pending.decline();
        }
    }
}

/// An open project on a paired host. Calls block; the app makes them off the main thread.
#[derive(uniffi::Object)]
pub struct RemoteProject {
    session: Mutex<ClientSession>,
}

#[uniffi::export]
impl RemoteProject {
    /// The project's folder on the host.
    pub fn root(&self) -> String {
        lock(&self.session).info.root.clone()
    }

    /// The rights the host granted this device.
    pub fn rights(&self) -> Vec<String> {
        lock(&self.session).info.rights.clone()
    }

    /// The entries of a folder, folders first; `.` is the project itself.
    pub fn list(&self, path: String) -> Result<Vec<RemoteEntry>, MobileError> {
        let mut session = lock(&self.session);
        let entries = session.list(&path)?;
        Ok(entries
            .iter()
            .map(|e| RemoteEntry {
                name: e["name"].as_str().unwrap_or_default().to_owned(),
                path: e["path"].as_str().unwrap_or_default().to_owned(),
                kind: match e["kind"].as_str() {
                    Some("dir") => EntryKind::Directory,
                    Some("link") => EntryKind::Link,
                    _ => EntryKind::File,
                },
                size: e["size"].as_u64().unwrap_or(0),
            })
            .collect())
    }

    /// Reads one regular file of at most [`MAX_FILE_BYTES`].
    pub fn read_file(&self, path: String) -> Result<RemoteFile, MobileError> {
        let session = lock(&self.session);
        let stat = session.call("fs.stat", json!({ "path": path }))?;
        if stat["kind"].as_str() != Some("file") {
            return Err(MobileError::NotAFile(path));
        }
        let size = stat["size"].as_u64().unwrap_or(0);
        if size > MAX_FILE_BYTES {
            return Err(MobileError::TooLarge { path, size });
        }
        let answer = session.call("fs.read", json!({ "path": path }))?;
        if let Some(text) = answer["text"].as_str() {
            return Ok(RemoteFile {
                path,
                size: text.len() as u64,
                text: Some(text.to_owned()),
            });
        }
        let stream = answer["stream"]
            .as_u64()
            .and_then(|s| u32::try_from(s).ok())
            .ok_or_else(|| MobileError::Remote("the host sent no file content".to_owned()))?;
        let connection = session
            .connection()
            .ok_or_else(|| MobileError::Disconnected("no connection".to_owned()))?;
        let bytes = connection.stream(stream, STREAM_LIMIT)?;
        Ok(RemoteFile {
            path,
            size: bytes.len() as u64,
            text: String::from_utf8(bytes).ok(),
        })
    }

    /// Whether the connection is alive.
    pub fn is_connected(&self) -> bool {
        lock(&self.session).connected()
    }

    /// Ends the session on the host. Not `close`: in Kotlin that name belongs to `AutoCloseable`,
    /// which frees the object itself, and the bindings would not compile.
    pub fn disconnect(&self) {
        let _ = lock(&self.session).bye();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_normalized_and_bad_ones_named() {
        assert_eq!(normalize_address(" nas:9000 ").unwrap(), "nas:9000");
        assert_eq!(normalize_address("paired://nas").unwrap(), "nas:47100");
        assert_eq!(normalize_address("[::1]:9").unwrap(), "[::1]:9");
        assert_eq!(normalize_address("[::1]").unwrap(), "[::1]:47100");
        for bad in [
            "", "-x:1", "a b:1", "::1", "h:0", "h:99999", "[zz]:1", "[::1",
        ] {
            assert!(normalize_address(bad).is_err(), "{bad}");
        }
        let e = normalize_address("nas@ssh").unwrap_err().to_string();
        assert!(e.contains("cannot use the SSH path"), "{e}");
    }

    #[test]
    fn hex_round_trips_and_refuses_bad_input() {
        let key = [0xabu8; 32];
        assert_eq!(unhex32(&hex(&key)), Some(key));
        assert_eq!(unhex32("00"), None);
        assert_eq!(unhex32(&"g".repeat(64)), None);
    }

    /// The exception to the workspace's `forbid(unsafe_code)` covers UniFFI's macros only.
    #[test]
    fn no_handwritten_unsafe_code() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let word = ["uns", "afe"].concat();
        let words = [" ", "(", "{"].map(|end| format!("{word}{end}"));
        for file in ["lib.rs", "bin/uniffi-bindgen.rs"] {
            let text = std::fs::read_to_string(format!("{dir}/{file}")).unwrap();
            let code: String = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            for w in &words {
                assert!(!code.contains(w.as_str()), "{file}: {w}");
            }
        }
    }
}
