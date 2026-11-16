//! Where the keys live (`ADR-0025` §3, §3a; `SPEC-0321` behaviour 20): the system keychain on
//! macOS and Windows, the Secret Service on Linux when a session bus exists, a checked key file
//! otherwise. The device and host lists hold metadata and public keys only.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::noise::{Identity, fingerprint};

/// Exit status of a refused start (`EX_CONFIG`).
pub const EXIT_REFUSED: u8 = 78;

/// A refused key store; the message names the path, the finding and what to do, never a key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct KeyError(pub String);

/// Which side of a paired channel a store belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// `idecraft serve --pair`.
    Serve,
    /// The window.
    Window,
}

impl Side {
    fn key_name(self) -> &'static str {
        match self {
            Self::Serve => "serve-key",
            Self::Window => "window-key",
        }
    }
    fn list_name(self) -> &'static str {
        match self {
            Self::Serve => "paired-keys.toml",
            Self::Window => "paired-hosts.toml",
        }
    }
}

/// A paired device, as the serving side knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Name the device gave.
    pub name: String,
    /// Fingerprint of its key.
    pub fingerprint: String,
    /// Public key as hex (absent in the list when the keychain holds it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub public: String,
    /// Its rights.
    pub rights: Vec<String>,
    /// Unix time of pairing.
    pub paired_on: u64,
}

/// A paired host, as the window knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedHost {
    /// Address as typed (`host:port`).
    pub address: String,
    /// Fingerprint of its key.
    pub fingerprint: String,
    /// Public key as hex (absent in the list when the keychain holds it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub public: String,
    /// Unix time of pairing.
    pub paired_on: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct List {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    format: u32,
    #[serde(default)]
    devices: Vec<Device>,
    #[serde(default)]
    hosts: Vec<PairedHost>,
    #[serde(default)]
    revoked: BTreeSet<String>,
    /// Remote access was switched off (`SPEC-0324` behaviour 11).
    #[serde(default)]
    access_off: bool,
}

const LIST_KIND: &str = "remote-paired-keys";
const KEY_KIND: &str = "remote-serve-key";
const FORMAT: u32 = 1;
const KEYCHAIN_SERVICE: &str = "idecraft-remote";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A keychain service holding named secrets (`ADR-0025` §3): the system keychain on macOS and
/// Windows, the Secret Service on Linux. A caller with its own keychain (and tests) can bring one
/// through [`Store::open_with`].
pub trait SecretStore: Send + Sync {
    /// The secret called `name`, if there is one. A keychain that cannot be read is an error,
    /// never "no secret".
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, KeyError>;
    /// Stores `bytes` as `name`, replacing an older secret of that name.
    fn set(&self, name: &str, bytes: &[u8]) -> Result<(), KeyError>;
    /// Removes `name`; a missing one is not an error.
    fn delete(&self, name: &str) -> Result<(), KeyError>;
    /// Where the start line says the key is stored.
    fn describe(&self) -> String;
}

/// The system keychain through `keyring` (macOS, Windows).
struct SystemKeychain;

impl SystemKeychain {
    fn entry(name: &str) -> Result<keyring::Entry, KeyError> {
        keyring::Entry::new(KEYCHAIN_SERVICE, name)
            .map_err(|e| KeyError(format!("the system keychain is not available: {e}")))
    }
}

impl SecretStore for SystemKeychain {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, KeyError> {
        match Self::entry(name)?.get_secret() {
            Ok(v) => Ok(Some(Zeroizing::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyError(format!(
                "idecraft: refusing to start: the system keychain cannot be read ({e}); unlock it \
                 and try again"
            ))),
        }
    }

    fn set(&self, name: &str, bytes: &[u8]) -> Result<(), KeyError> {
        Self::entry(name)?
            .set_secret(bytes)
            .map_err(|e| KeyError(format!("the system keychain refused the key: {e}")))
    }

    fn delete(&self, name: &str) -> Result<(), KeyError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyError(format!("the system keychain refused: {e}"))),
        }
    }

    fn describe(&self) -> String {
        "key stored in the system keychain".into()
    }
}

/// Where a private key is held.
enum Backing {
    /// The checked file store; the text says why it is used.
    File(&'static str),
    Secrets(Box<dyn SecretStore>),
}

/// The keys of one side, and its list of paired peers.
pub struct Store {
    side: Side,
    dir: PathBuf,
    backing: Backing,
    /// The folder and list are checked and written like the file store's (`ADR-0025` §3a).
    #[cfg_attr(not(unix), allow(dead_code))]
    checked: bool,
}

/// Whether this machine must use the system keychain (`ADR-0025` §3a).
pub fn keychain_required() -> bool {
    cfg!(any(target_os = "macos", windows))
}

/// Debug builds on macOS and Linux keep keys in the file store, however they are started, so a
/// test never reaches the keychain or Secret Service of the person running it: the test suites
/// start real backends and windows through [`Store::open`]. On macOS every rebuilt program also
/// has a new ad-hoc signature, so the keychain would ask for the password each time. Release
/// builds always use the keychain or Secret Service (`ADR-0025` §3a). The Secret Service code is
/// tested against a private session bus through [`Store::open_secret_service`].
const DEBUG_BUILD_KEEPS_KEYS_IN_FILES: bool = cfg!(all(
    debug_assertions,
    any(target_os = "macos", target_os = "linux")
));

const BECAUSE_NO_KEYCHAIN: &str = "no keychain on this host";
const BECAUSE_CHOSEN: &str = "file store chosen with --key-file or IDECRAFT_KEY_FILE";
const BECAUSE_KEPT: &str = "kept from before; delete it and pair again to move to the keychain";
const BECAUSE_DEBUG: &str = "debug build: never the system keychain";

/// Whether a session bus exists, so a keychain service might (`SPEC-0321` behaviour 20).
pub fn has_session_bus() -> bool {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
}

impl Store {
    /// The store of `side` in `dir` (normally `<settings>/remote`): the system keychain on macOS
    /// and Windows; on Linux with a session bus the Secret Service; else the checked file store.
    /// `allow_file` is the user's explicit choice of the file store on a machine with a session
    /// bus. A key file that already exists is kept, so the host keeps its identity. Every check
    /// runs here, before anything listens; a Secret Service that cannot be reached or is locked
    /// refuses the start instead of falling back to a file.
    pub fn open(side: Side, dir: &Path, allow_file: bool) -> Result<Self, KeyError> {
        if DEBUG_BUILD_KEEPS_KEYS_IN_FILES {
            return Self::open_file_because(side, dir, BECAUSE_DEBUG);
        }
        if keychain_required() {
            return Self::open_with(side, dir, Box::new(SystemKeychain));
        }
        if !has_session_bus() {
            return Self::open_file_because(side, dir, BECAUSE_NO_KEYCHAIN);
        }
        if allow_file {
            return Self::open_file_because(side, dir, BECAUSE_CHOSEN);
        }
        if std::fs::symlink_metadata(dir.join(side.key_name())).is_ok() {
            // Checked like any key file; a link or open mode still refuses the start.
            return Self::open_file_because(side, dir, BECAUSE_KEPT);
        }
        Self::open_secret_service(side, dir)
    }

    /// The Secret Service on the session bus (`SPEC-0321` behaviour 20), over an encrypted
    /// Secret Service session only. Refused when the bus or the service cannot be reached or the
    /// default keyring is locked.
    #[cfg(target_os = "linux")]
    pub fn open_secret_service(side: Side, dir: &Path) -> Result<Self, KeyError> {
        let secrets = secret_service::SecretService::connect(&dir.join(side.key_name()))?;
        Self::open_with_checked_dir(side, dir, Box::new(secrets))
    }

    /// This build has no Secret Service support (only Linux has it).
    #[cfg(not(target_os = "linux"))]
    pub fn open_secret_service(side: Side, dir: &Path) -> Result<Self, KeyError> {
        Err(KeyError(format!(
            "idecraft: refusing to start: this machine has a session bus, so a keychain may \
             exist, but this build has no Secret Service support. Run `idecraft serve` with \
             --key-file (the window: IDECRAFT_KEY_FILE=1) to keep the key in the file {} \
             (mode 0600) instead.",
            dir.join(side.key_name()).display()
        )))
    }

    /// A store whose private key and pins live in `secrets`; the list of peers stays in `dir`.
    pub fn open_with(
        side: Side,
        dir: &Path,
        secrets: Box<dyn SecretStore>,
    ) -> Result<Self, KeyError> {
        std::fs::create_dir_all(dir).map_err(|e| KeyError(format!("{}: {e}", dir.display())))?;
        Ok(Self {
            side,
            dir: dir.to_path_buf(),
            backing: Backing::Secrets(secrets),
            checked: false,
        })
    }

    /// As [`Self::open_with`], with the folder created 0700 and checked like the file store's:
    /// the list holds the rights.
    #[cfg(target_os = "linux")]
    fn open_with_checked_dir(
        side: Side,
        dir: &Path,
        secrets: Box<dyn SecretStore>,
    ) -> Result<Self, KeyError> {
        unix_files::prepare_dir(dir)?;
        unix_files::check_if_present(&dir.join(side.list_name()), 0)?;
        Ok(Self {
            side,
            dir: dir.to_path_buf(),
            backing: Backing::Secrets(secrets),
            checked: true,
        })
    }

    /// The checked file store (`ADR-0025` §3a), for hosts without a keychain and for tests.
    pub fn open_file(side: Side, dir: &Path) -> Result<Self, KeyError> {
        Self::open_file_because(side, dir, BECAUSE_NO_KEYCHAIN)
    }

    #[cfg(unix)]
    fn open_file_because(side: Side, dir: &Path, why: &'static str) -> Result<Self, KeyError> {
        unix_files::prepare_dir(dir)?;
        unix_files::check_if_present(&dir.join(side.key_name()), 32)?;
        unix_files::check_if_present(&dir.join(side.list_name()), 0)?;
        Ok(Self {
            side,
            dir: dir.to_path_buf(),
            backing: Backing::File(why),
            checked: true,
        })
    }

    /// The file store is not offered on this system (`ADR-0025` §3a).
    #[cfg(not(unix))]
    fn open_file_because(_side: Side, dir: &Path, _why: &'static str) -> Result<Self, KeyError> {
        Err(KeyError(format!(
            "{}: the file store is only offered on Unix systems",
            dir.display()
        )))
    }

    /// Where the list lives.
    pub fn list_path(&self) -> PathBuf {
        self.dir.join(self.side.list_name())
    }

    /// A line for the start message: where the key is stored.
    pub fn describe(&self) -> String {
        match &self.backing {
            Backing::File(why) => format!(
                "key stored in file {} ({why})",
                self.dir.join(self.side.key_name()).display()
            ),
            Backing::Secrets(s) => s.describe(),
        }
    }

    /// The keychain service, unless the file store is used.
    fn secrets(&self) -> Option<&dyn SecretStore> {
        match &self.backing {
            Backing::File(_) => None,
            Backing::Secrets(s) => Some(s.as_ref()),
        }
    }

    /// The installation's static key, created on first use.
    pub fn identity(&self) -> Result<Identity, KeyError> {
        let Some(secrets) = self.secrets() else {
            return self.file_identity();
        };
        let name = self.side.key_name();
        if let Some(v) = secrets.get(name)? {
            if v.len() == 64 {
                return Identity::from_bytes(&v[..32], &v[32..])
                    .ok_or_else(|| KeyError("the key in the system keychain is damaged".into()));
            }
            return Err(KeyError(
                "the key in the system keychain has the wrong length".into(),
            ));
        }
        let id =
            Identity::generate().map_err(|e| KeyError(format!("no key could be made: {e}")))?;
        let mut blob = Zeroizing::new(Vec::with_capacity(64));
        blob.extend_from_slice(&id.public);
        blob.extend_from_slice(id.private.as_ref());
        secrets.set(name, &blob)?;
        Ok(id)
    }

    #[cfg(unix)]
    fn file_identity(&self) -> Result<Identity, KeyError> {
        let path = self.dir.join(self.side.key_name());
        if let Some(text) = unix_files::read_checked(&path, 32)? {
            let text = Zeroizing::new(text);
            let parsed: toml::Value = toml::from_str(&text).map_err(|_| {
                KeyError(format!(
                    "idecraft: refusing to start: the key file {} is damaged; delete it to create \
                     a new key (paired devices must pair again).",
                    path.display()
                ))
            })?;
            let bad = || {
                KeyError(format!(
                    "idecraft: refusing to start: the key file {} has the wrong kind or length; \
                     delete it to create a new key (paired devices must pair again).",
                    path.display()
                ))
            };
            if parsed.get("kind").and_then(toml::Value::as_str) != Some(KEY_KIND) {
                return Err(bad());
            }
            let private = parsed
                .get("key")
                .and_then(toml::Value::as_str)
                .and_then(unhex32)
                .ok_or_else(bad)?;
            let public = parsed
                .get("public")
                .and_then(toml::Value::as_str)
                .and_then(unhex32)
                .ok_or_else(bad)?;
            return Identity::from_bytes(&public, &private).ok_or_else(bad);
        }
        let id =
            Identity::generate().map_err(|e| KeyError(format!("no key could be made: {e}")))?;
        let body = Zeroizing::new(format!(
            "kind = \"{KEY_KIND}\"\nformat = {FORMAT}\npublic = \"{}\"\nkey = \"{}\"\n",
            hex(&id.public),
            hex(id.private.as_ref())
        ));
        unix_files::create_private(&path, body.as_bytes())?;
        Ok(id)
    }

    #[cfg(not(unix))]
    fn file_identity(&self) -> Result<Identity, KeyError> {
        Err(KeyError("the file store is only offered on Unix".into()))
    }

    fn load(&self) -> Result<List, KeyError> {
        let path = self.list_path();
        #[cfg(unix)]
        let text = if self.checked {
            unix_files::read_checked(&path, 0)?
        } else {
            std::fs::read_to_string(&path).ok()
        };
        #[cfg(not(unix))]
        let text = std::fs::read_to_string(&path).ok();
        let Some(text) = text else {
            return Ok(List {
                kind: LIST_KIND.into(),
                format: FORMAT,
                ..List::default()
            });
        };
        let list: List = toml::from_str(&text).map_err(|e| {
            KeyError(format!(
                "idecraft: refusing to start: {} is damaged: {e}",
                path.display()
            ))
        })?;
        if list.kind != LIST_KIND {
            return Err(KeyError(format!(
                "idecraft: refusing to start: {} is not a list of paired keys",
                path.display()
            )));
        }
        Ok(list)
    }

    fn save(&self, list: &List) -> Result<(), KeyError> {
        let path = self.list_path();
        let mut body = list.clone();
        body.kind = LIST_KIND.into();
        body.format = FORMAT;
        let text = toml::to_string(&body).map_err(|e| KeyError(e.to_string()))?;
        #[cfg(unix)]
        if self.checked {
            return unix_files::replace_private(&path, text.as_bytes());
        }
        // Synced before the rename and the folder after it: a device is reported as stored only
        // once a crash cannot lose it (`SPEC-0321` behaviour 17).
        let tmp = path.with_extension("tmp");
        let written = std::fs::File::create(&tmp).and_then(|mut f| {
            use std::io::Write;
            f.write_all(text.as_bytes())?;
            f.sync_all()
        });
        written
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| KeyError(format!("{}: {e}", path.display())))?;
        #[cfg(unix)]
        if let Some(Ok(d)) = path.parent().map(std::fs::File::open) {
            let _ = d.sync_all();
        }
        Ok(())
    }

    fn pin_name(fp: &str) -> String {
        format!("peer-{fp}")
    }

    fn store_public(&self, fp: &str, public: &[u8; 32]) -> Result<String, KeyError> {
        if let Some(secrets) = self.secrets() {
            secrets.set(&Self::pin_name(fp), public)?;
            return Ok(String::new());
        }
        Ok(hex(public))
    }

    /// The paired devices (serving side).
    pub fn devices(&self) -> Result<Vec<Device>, KeyError> {
        Ok(self.load()?.devices)
    }

    /// Whether `fp` has been revoked.
    pub fn is_revoked(&self, fp: &str) -> Result<bool, KeyError> {
        Ok(self.load()?.revoked.contains(fp))
    }

    /// The revoked fingerprints.
    pub fn revoked(&self) -> Result<BTreeSet<String>, KeyError> {
        Ok(self.load()?.revoked)
    }

    /// The device with this static key, when paired and not revoked.
    pub fn device_for(&self, public: &[u8; 32]) -> Result<Option<Device>, KeyError> {
        let fp = fingerprint(public);
        let list = self.load()?;
        if list.revoked.contains(&fp) {
            return Ok(None);
        }
        let Some(d) = list.devices.iter().find(|d| d.fingerprint == fp) else {
            return Ok(None);
        };
        let stored = if let Some(secrets) = self.secrets() {
            secrets
                .get(&Self::pin_name(&fp))?
                .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
        } else {
            unhex32(&d.public)
        };
        Ok((stored == Some(*public)).then(|| d.clone()))
    }

    /// Stores a device after both sides confirmed. A revoked key cannot be stored again.
    pub fn add_device(
        &self,
        public: &[u8; 32],
        name: &str,
        rights: &[String],
    ) -> Result<(), KeyError> {
        let fp = fingerprint(public);
        let mut list = self.load()?;
        if list.revoked.contains(&fp) {
            return Err(KeyError(format!("device {fp} was revoked")));
        }
        let hexed = self.store_public(&fp, public)?;
        list.devices.retain(|d| d.fingerprint != fp);
        list.devices.push(Device {
            name: name.to_owned(),
            fingerprint: fp,
            public: hexed,
            rights: rights.to_vec(),
            paired_on: now(),
        });
        self.save(&list)
    }

    /// Revokes a device: its key is removed and its fingerprint is remembered.
    pub fn revoke(&self, fp: &str) -> Result<bool, KeyError> {
        let mut list = self.load()?;
        let known = list.devices.iter().any(|d| d.fingerprint == fp);
        list.devices.retain(|d| d.fingerprint != fp);
        list.revoked.insert(fp.to_owned());
        if let Some(secrets) = self.secrets() {
            secrets.delete(&Self::pin_name(fp))?;
        }
        self.save(&list)?;
        Ok(known)
    }

    /// Replaces a device's rights (`SPEC-0324` behaviour 8); `false` when it is not paired.
    pub fn set_rights(&self, fp: &str, rights: &[String]) -> Result<bool, KeyError> {
        let mut list = self.load()?;
        let Some(d) = list.devices.iter_mut().find(|d| d.fingerprint == fp) else {
            return Ok(false);
        };
        d.rights = rights.to_vec();
        self.save(&list)?;
        Ok(true)
    }

    /// Whether remote access was switched off (`SPEC-0324` behaviour 11).
    pub fn access_off(&self) -> Result<bool, KeyError> {
        Ok(self.load()?.access_off)
    }

    /// Switches remote access off or on again.
    pub fn set_access_off(&self, off: bool) -> Result<(), KeyError> {
        let mut list = self.load()?;
        if list.access_off == off {
            return Ok(());
        }
        list.access_off = off;
        self.save(&list)
    }

    /// Copies the pin of `address` to `path`; `false` when `address` is not paired. The next
    /// handshake proves the key (`SPEC-0324` behaviour 7).
    pub fn add_path(&self, address: &str, path: &str) -> Result<bool, KeyError> {
        let mut list = self.load()?;
        let Some(h) = list.hosts.iter().find(|h| h.address == address).cloned() else {
            return Ok(false);
        };
        list.hosts.retain(|x| x.address != path);
        list.hosts.push(PairedHost {
            address: path.to_owned(),
            ..h
        });
        self.save(&list)?;
        Ok(true)
    }

    /// The paired hosts (window side).
    pub fn hosts(&self) -> Result<Vec<PairedHost>, KeyError> {
        Ok(self.load()?.hosts)
    }

    /// The pinned key of `address`.
    pub fn host_key(&self, address: &str) -> Result<Option<[u8; 32]>, KeyError> {
        let list = self.load()?;
        let Some(h) = list.hosts.iter().find(|h| h.address == address) else {
            return Ok(None);
        };
        if let Some(secrets) = self.secrets() {
            return Ok(secrets
                .get(&Self::pin_name(&h.fingerprint))?
                .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok()));
        }
        Ok(unhex32(&h.public))
    }

    /// Pins `public` for `address` after both sides confirmed.
    pub fn add_host(&self, address: &str, public: &[u8; 32]) -> Result<(), KeyError> {
        let fp = fingerprint(public);
        let mut list = self.load()?;
        let hexed = self.store_public(&fp, public)?;
        list.hosts.retain(|h| h.address != address);
        list.hosts.push(PairedHost {
            address: address.to_owned(),
            fingerprint: fp,
            public: hexed,
            paired_on: now(),
        });
        self.save(&list)
    }

    /// Removes the pin of `address`.
    pub fn forget_host(&self, address: &str) -> Result<bool, KeyError> {
        let mut list = self.load()?;
        let Some(h) = list.hosts.iter().find(|h| h.address == address).cloned() else {
            return Ok(false);
        };
        list.hosts.retain(|x| x.address != address);
        if let Some(secrets) = self.secrets() {
            secrets.delete(&Self::pin_name(&h.fingerprint))?;
        }
        self.save(&list)?;
        Ok(true)
    }
}

#[cfg(target_os = "linux")]
mod secret_service {
    //! The Secret Service (freedesktop.org) on the session bus, through `oo7` over `zbus` in pure
    //! Rust (`SPEC-0321` behaviour 20). Only an encrypted Secret Service session is used, only the
    //! default collection, and nothing is unlocked here: a locked keyring refuses the start
    //! instead of showing a prompt nobody may be there to answer.

    use std::collections::HashMap;
    use std::path::Path;

    use futures_lite::future::block_on;
    use oo7::dbus::{Collection, Service};
    use zeroize::Zeroizing;

    use super::{KEYCHAIN_SERVICE, KeyError, SecretStore};

    pub struct SecretService {
        // Keeps the D-Bus connection and the encrypted session open for the collection.
        _service: Service<'static>,
        collection: Collection<'static>,
    }

    fn refused(what: &str, key_file: &Path) -> KeyError {
        KeyError(format!(
            "idecraft: refusing to start: this machine has a session bus, but {what}. Unlock or \
             start the keyring (for example by logging in to the desktop) and try again, or run \
             `idecraft serve` with --key-file (the window: IDECRAFT_KEY_FILE=1) to keep the key \
             in the file {} (mode 0600) instead.",
            key_file.display()
        ))
    }

    fn attributes(name: &str) -> HashMap<&str, &str> {
        HashMap::from([
            ("application", "idecraft"),
            ("service", KEYCHAIN_SERVICE),
            ("name", name),
        ])
    }

    impl SecretService {
        /// Connects and checks the default collection; `key_file` is named in a refusal.
        pub fn connect(key_file: &Path) -> Result<Self, KeyError> {
            block_on(async {
                let service = Service::encrypted().await.map_err(|e| {
                    refused(
                        &format!(
                            "no Secret Service could be reached over an encrypted session ({e})"
                        ),
                        key_file,
                    )
                })?;
                let collection = service
                    .with_alias(Service::DEFAULT_COLLECTION)
                    .await
                    .map_err(|e| refused(&format!("the Secret Service failed ({e})"), key_file))?
                    .ok_or_else(|| {
                        refused("the Secret Service has no default keyring", key_file)
                    })?;
                let locked = collection
                    .is_locked()
                    .await
                    .map_err(|e| refused(&format!("the Secret Service failed ({e})"), key_file))?;
                if locked {
                    return Err(refused(
                        "the default keyring of the Secret Service is locked",
                        key_file,
                    ));
                }
                Ok(Self {
                    _service: service,
                    collection,
                })
            })
        }
    }

    fn failed(e: oo7::dbus::Error) -> KeyError {
        KeyError(format!(
            "idecraft: refusing to start: the Secret Service cannot be read ({e}); unlock the \
             keyring and try again"
        ))
    }

    impl SecretStore for SecretService {
        fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, KeyError> {
            block_on(async {
                let items = self
                    .collection
                    .search_items(&attributes(name))
                    .await
                    .map_err(failed)?;
                let Some(item) = items.first() else {
                    return Ok(None);
                };
                let secret = item.secret().await.map_err(failed)?;
                Ok(Some(Zeroizing::new(secret.as_bytes().to_vec())))
            })
        }

        fn set(&self, name: &str, bytes: &[u8]) -> Result<(), KeyError> {
            block_on(async {
                self.collection
                    .create_item(
                        &format!("IDECraft remote key ({name})"),
                        &attributes(name),
                        oo7::Secret::blob(bytes),
                        true,
                        None,
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| KeyError(format!("the Secret Service refused the key: {e}")))
            })
        }

        fn delete(&self, name: &str) -> Result<(), KeyError> {
            block_on(async {
                let items = self
                    .collection
                    .search_items(&attributes(name))
                    .await
                    .map_err(failed)?;
                for item in items {
                    item.delete(None)
                        .await
                        .map_err(|e| KeyError(format!("the Secret Service refused: {e}")))?;
                }
                Ok(())
            })
        }

        fn describe(&self) -> String {
            "key stored in the Secret Service (default keyring)".into()
        }
    }
}

#[cfg(unix)]
mod unix_files {
    //! The file store's checks: open without following links, `fstat` on the open descriptor, no
    //! repairs (`ADR-0025` §3a).

    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};

    use super::KeyError;

    fn own_uid() -> u32 {
        nix::unistd::geteuid().as_raw()
    }

    fn refuse(what: String) -> KeyError {
        KeyError(format!("idecraft serve: refusing to start: {what}"))
    }

    fn describe_kind(meta: &std::fs::Metadata) -> &'static str {
        let t = meta.file_type();
        if t.is_dir() {
            "a folder"
        } else if t.is_symlink() {
            "a symbolic link"
        } else if t.is_fifo() {
            "a pipe"
        } else if t.is_socket() {
            "a socket"
        } else {
            "not a regular file"
        }
    }

    /// Creates the folder (0700) when missing, otherwise checks it; checks the parents up to the
    /// home folder (`StrictModes`).
    pub fn prepare_dir(dir: &Path) -> Result<(), KeyError> {
        match std::fs::symlink_metadata(dir) {
            Err(_) => {
                if let Some(parent) = dir.parent() {
                    std::fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(parent)
                        .map_err(|e| KeyError(format!("{}: {e}", parent.display())))?;
                }
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(dir)
                    .map_err(|e| KeyError(format!("{}: {e}", dir.display())))?;
            }
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(refuse(format!(
                        "the folder {} is a symbolic link; refusing to follow it.",
                        dir.display()
                    )));
                }
                if !meta.is_dir() {
                    return Err(refuse(format!("{} is not a folder.", dir.display())));
                }
            }
        }
        let f = File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(dir)
            .map_err(|e| refuse(format!("the folder {}: {e}", dir.display())))?;
        let meta = f
            .metadata()
            .map_err(|e| refuse(format!("the folder {}: {e}", dir.display())))?;
        if meta.uid() != own_uid() {
            return Err(refuse(format!(
                "the folder {} is owned by uid {}, not by you (uid {}). Check where it came from.",
                dir.display(),
                meta.uid(),
                own_uid()
            )));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(refuse(format!(
                "the folder {} can be read or written by group or others (mode {:04o}); run: chmod 700 {}",
                dir.display(),
                meta.mode() & 0o777,
                dir.display()
            )));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        for parent in dir.ancestors().skip(1) {
            if Some(parent) == home.as_deref() || parent == Path::new("/") {
                break;
            }
            let Ok(m) = std::fs::metadata(parent) else {
                continue;
            };
            if m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0 {
                return Err(refuse(format!(
                    "the folder {} can be written by group or others (mode {:04o}), so the key \
                     folder could be replaced; run: chmod go-w {}",
                    parent.display(),
                    m.mode() & 0o777,
                    parent.display()
                )));
            }
        }
        Ok(())
    }

    fn open_checked(path: &Path, min_len: u64) -> Result<Option<File>, KeyError> {
        let file = match File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
                return Err(refuse(format!(
                    "the key file {} is a symbolic link; refusing to follow it.",
                    path.display()
                )));
            }
            Err(e) => {
                return Err(refuse(format!("the key file {}: {e}", path.display())));
            }
        };
        let meta = file
            .metadata()
            .map_err(|e| refuse(format!("the key file {}: {e}", path.display())))?;
        if !meta.is_file() {
            return Err(refuse(format!(
                "the key file {} is {}; refusing to use it.",
                path.display(),
                describe_kind(&meta)
            )));
        }
        if meta.uid() != own_uid() {
            return Err(refuse(format!(
                "the key file {} is owned by uid {}, not by you (uid {}). Check where it came \
                 from; delete it to create a new key (paired devices must pair again).",
                path.display(),
                meta.uid(),
                own_uid()
            )));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(refuse(format!(
                "the key file {} can be read by group or others (mode {:04o}). If someone else \
                 may have read it, delete it and pair your devices again; otherwise run: chmod \
                 600 {}",
                path.display(),
                meta.mode() & 0o777,
                path.display()
            )));
        }
        if meta.len() < min_len {
            return Err(refuse(format!(
                "the key file {} has the wrong length; delete it to create a new key (paired \
                 devices must pair again).",
                path.display()
            )));
        }
        Ok(Some(file))
    }

    pub fn check_if_present(path: &Path, min_len: u64) -> Result<(), KeyError> {
        open_checked(path, min_len).map(|_| ())
    }

    pub fn read_checked(path: &Path, min_len: u64) -> Result<Option<String>, KeyError> {
        let Some(mut file) = open_checked(path, min_len)? else {
            return Ok(None);
        };
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|e| refuse(format!("the key file {}: {e}", path.display())))?;
        Ok(Some(text))
    }

    fn write_new(path: &Path, bytes: &[u8]) -> Result<(), KeyError> {
        let parent = path.parent().unwrap_or(Path::new("."));
        let tmp = parent.join(format!(
            ".{}.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("idecraft"),
            std::process::id()
        ));
        let mut f = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(|e| KeyError(format!("{}: {e}", tmp.display())))?;
        let done = f
            .write_all(bytes)
            .and_then(|()| f.sync_all())
            .and_then(|()| std::fs::rename(&tmp, path));
        if done.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        done.map_err(|e| KeyError(format!("{}: {e}", path.display())))?;
        if let Ok(d) = File::open(parent) {
            let _ = d.sync_all();
        }
        Ok(())
    }

    pub fn create_private(path: &Path, bytes: &[u8]) -> Result<(), KeyError> {
        write_new(path, bytes)
    }

    pub fn replace_private(path: &Path, bytes: &[u8]) -> Result<(), KeyError> {
        write_new(path, bytes)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn root() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("remote");
        std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        (t, dir)
    }

    /// Pairing tests run real backends; none of them may reach the login keychain or the Secret
    /// Service of the person running them, with or without a session bus.
    #[test]
    fn test_runs_never_use_the_login_keychain() {
        let (_t, dir) = root();
        let store = Store::open(Side::Serve, &dir, false).unwrap();
        assert!(store.describe().contains("file"), "{}", store.describe());
        assert!(
            store.describe().contains("debug build"),
            "{}",
            store.describe()
        );
        let window = Store::open(Side::Window, &dir, false).unwrap();
        assert!(window.describe().contains("file"), "{}", window.describe());
    }

    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).unwrap().mode() & 0o777
    }

    #[test]
    fn a_new_key_is_private_whatever_the_umask() {
        let (_t, dir) = root();
        let old = nix::sys::stat::umask(nix::sys::stat::Mode::empty());
        let store = Store::open_file(Side::Serve, &dir).unwrap();
        let id = store.identity().unwrap();
        nix::sys::stat::umask(old);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("serve-key")), 0o600);
        let text = std::fs::read_to_string(dir.join("serve-key")).unwrap();
        assert!(text.contains(&hex(id.private.as_ref())));
        assert!(text.contains("kind = \"remote-serve-key\""));
        // The same identity on the next start.
        let again = Store::open_file(Side::Serve, &dir)
            .unwrap()
            .identity()
            .unwrap();
        assert_eq!(again.public, id.public);
    }

    fn refused(dir: &Path) -> String {
        Store::open_file(Side::Serve, dir).err().expect("refused").0
    }

    #[test]
    fn sec16_open_rights_on_the_key_file_refuse_the_start() {
        for m in [0o640, 0o604, 0o644] {
            let (_t, dir) = root();
            Store::open_file(Side::Serve, &dir)
                .unwrap()
                .identity()
                .unwrap();
            let key = dir.join("serve-key");
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(m)).unwrap();
            let msg = refused(&dir);
            assert!(msg.contains("serve-key"), "{msg}");
            assert!(msg.contains("group or others"), "{msg}");
            assert!(!msg.contains("key = "), "{msg}");
            // Nothing is repaired.
            assert_eq!(mode(&key), m);
        }
    }

    #[test]
    fn sec16_the_paired_list_and_links_and_kinds_are_checked() {
        let (_t, dir) = root();
        let store = Store::open_file(Side::Serve, &dir).unwrap();
        store.identity().unwrap();
        store
            .add_device(&[9; 32], "phone", &["view".into()])
            .unwrap();
        let list = dir.join("paired-keys.toml");
        assert_eq!(mode(&list), 0o600);
        std::fs::set_permissions(&list, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(refused(&dir).contains("paired-keys.toml"));
        std::fs::set_permissions(&list, std::fs::Permissions::from_mode(0o600)).unwrap();

        // A symbolic link to a 0600 file.
        let key = dir.join("serve-key");
        let real = dir.join("real");
        std::fs::rename(&key, &real).unwrap();
        symlink(&real, &key).unwrap();
        assert!(refused(&dir).contains("symbolic link"));
        std::fs::remove_file(&key).unwrap();
        // Not a regular file.
        std::fs::create_dir(&key).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(refused(&dir).contains("a folder"));
    }

    #[test]
    fn sec16_folders_are_checked() {
        for m in [0o750, 0o755] {
            let (_t, dir) = root();
            Store::open_file(Side::Serve, &dir)
                .unwrap()
                .identity()
                .unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(m)).unwrap();
            assert!(refused(&dir).contains("chmod 700"));
            assert_eq!(mode(&dir), m);
        }
        // A parent folder writable by others.
        let (t, dir) = root();
        Store::open_file(Side::Serve, &dir).unwrap();
        std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(refused(&dir).contains("written by group or others"));
    }

    #[test]
    fn sec16_a_key_of_the_wrong_length_is_refused() {
        let (_t, dir) = root();
        let store = Store::open_file(Side::Serve, &dir).unwrap();
        store.identity().unwrap();
        let key = dir.join("serve-key");
        std::fs::write(
            &key,
            "kind = \"remote-serve-key\"\nformat = 1\nkey = \"ab12cd34ef56ab12\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let again = Store::open_file(Side::Serve, &dir).unwrap();
        let msg = again.identity().unwrap_err().0;
        assert!(msg.contains("wrong kind or length"), "{msg}");
        // Longer than a temporary folder's random name, so the path in the message cannot hold it
        // by chance (a two-letter value failed once in a few hundred runs).
        assert!(!msg.contains("ab12cd34ef56ab12"), "{msg}");
    }

    #[test]
    fn sec3_a_revoked_device_stays_out_and_cannot_be_added_back() {
        let (_t, dir) = root();
        let store = Store::open_file(Side::Serve, &dir).unwrap();
        let public = [7u8; 32];
        store
            .add_device(&public, "phone", &["view".into()])
            .unwrap();
        assert!(store.device_for(&public).unwrap().is_some());
        assert!(store.revoke(&fingerprint(&public)).unwrap());
        assert!(store.device_for(&public).unwrap().is_none());
        assert!(store.add_device(&public, "phone", &[]).is_err());
        assert!(store.is_revoked(&fingerprint(&public)).unwrap());
    }

    #[test]
    fn hosts_are_pinned_and_forgotten() {
        let (_t, dir) = root();
        let store = Store::open_file(Side::Window, &dir).unwrap();
        store.add_host("host:47100", &[3; 32]).unwrap();
        assert_eq!(store.host_key("host:47100").unwrap(), Some([3; 32]));
        assert_eq!(store.host_key("other:1").unwrap(), None);
        assert_eq!(store.hosts().unwrap().len(), 1);
        assert!(store.forget_host("host:47100").unwrap());
        assert_eq!(store.host_key("host:47100").unwrap(), None);
    }

    #[test]
    fn sec10_the_list_holds_no_private_key() {
        let (_t, dir) = root();
        let store = Store::open_file(Side::Serve, &dir).unwrap();
        let id = store.identity().unwrap();
        store
            .add_device(&[1; 32], "phone", &["view".into()])
            .unwrap();
        let list = std::fs::read_to_string(dir.join("paired-keys.toml")).unwrap();
        assert!(!list.contains(&hex(id.private.as_ref())));
        assert!(!list.contains("key = "), "{list}");
    }

    /// A keychain in memory, standing in for the Secret Service or the system keychain.
    #[derive(Clone, Default)]
    struct Memory {
        items: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>>,
        broken: bool,
    }

    impl SecretStore for Memory {
        fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, KeyError> {
            if self.broken {
                return Err(KeyError("the keyring is locked".into()));
            }
            Ok(self
                .items
                .lock()
                .unwrap()
                .get(name)
                .map(|v| Zeroizing::new(v.clone())))
        }
        fn set(&self, name: &str, bytes: &[u8]) -> Result<(), KeyError> {
            if self.broken {
                return Err(KeyError("the keyring is locked".into()));
            }
            self.items
                .lock()
                .unwrap()
                .insert(name.to_owned(), bytes.to_vec());
            Ok(())
        }
        fn delete(&self, name: &str) -> Result<(), KeyError> {
            self.items.lock().unwrap().remove(name);
            Ok(())
        }
        fn describe(&self) -> String {
            "key stored in memory".into()
        }
    }

    #[test]
    fn a_keychain_store_keeps_keys_and_pins_out_of_files() {
        let (_t, dir) = root();
        let memory = Memory::default();
        let store = Store::open_with(Side::Serve, &dir, Box::new(memory.clone())).unwrap();
        let id = store.identity().unwrap();
        let again = Store::open_with(Side::Serve, &dir, Box::new(memory.clone())).unwrap();
        assert_eq!(again.identity().unwrap().public, id.public);
        assert!(!dir.join("serve-key").exists());

        let phone = [5u8; 32];
        store.add_device(&phone, "phone", &["view".into()]).unwrap();
        let fp = fingerprint(&phone);
        assert!(
            memory
                .items
                .lock()
                .unwrap()
                .contains_key(&format!("peer-{fp}"))
        );
        let list = std::fs::read_to_string(dir.join("paired-keys.toml")).unwrap();
        assert!(!list.contains(&hex(&phone)), "{list}");
        assert!(!list.contains(&hex(id.private.as_ref())), "{list}");
        assert!(store.device_for(&phone).unwrap().is_some());

        // A pin changed in the keychain no longer matches.
        memory
            .items
            .lock()
            .unwrap()
            .insert(format!("peer-{fp}"), vec![6; 32]);
        assert!(store.device_for(&phone).unwrap().is_none());

        assert!(store.revoke(&fp).unwrap());
        assert!(
            !memory
                .items
                .lock()
                .unwrap()
                .contains_key(&format!("peer-{fp}"))
        );
    }

    #[test]
    fn a_keychain_that_cannot_be_read_is_an_error_not_an_empty_one() {
        let (_t, dir) = root();
        let broken = Memory {
            broken: true,
            ..Memory::default()
        };
        let store = Store::open_with(Side::Serve, &dir, Box::new(broken)).unwrap();
        // No new key replaces one that cannot be read.
        assert!(store.identity().is_err());
        assert!(!dir.join("serve-key").exists());
    }
}
