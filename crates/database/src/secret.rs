//! Passwords of connection profiles (`SPEC-0008`, behaviour 11–13).
//!
//! Secrets live only in a [`SecretStore`] — by default the operating system's
//! credential store — and never in profiles, logs or error messages.
//! Resolving credentials never waits for input; a missing password is a
//! result the caller can show without blocking.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use crate::{ConnectionProfile, SecretRef};

/// A password. Debug and display output never show the value.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a password.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The password itself; pass it straight to the driver.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// Failure of the credential store itself (not "no such entry").
#[derive(Debug, thiserror::Error)]
#[error("credential store ({service}/{account}): {message}")]
pub struct SecretError {
    /// Keychain service.
    pub service: String,
    /// Keychain account.
    pub account: String,
    /// What went wrong; never contains the secret.
    pub message: String,
}

/// Where passwords are kept.
pub trait SecretStore {
    /// Reads a password; `Ok(None)` when there is no entry.
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError>;
    /// Stores or replaces a password.
    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError>;
    /// Removes a password; removing a missing entry is not an error.
    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError>;
}

impl<T: SecretStore + ?Sized> SecretStore for &T {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        (**self).get(reference)
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        (**self).set(reference, secret)
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        (**self).delete(reference)
    }
}

/// The operating system's credential store: the macOS Keychain, the Windows Credential Manager,
/// and on Linux the Secret Service (`SPEC-0008` v8 behaviour 18) — encrypted at rest under the
/// user's login keyring, so passwords survive a restart. On Linux a password still kept in the
/// kernel keyring from an older version is moved into the Secret Service when it is first read.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeychainStore;

/// The store of the `keyring` crate: the macOS Keychain, the Windows Credential Manager, and on
/// Linux the kernel keyring — which lives only as long as the login session. On Linux
/// [`KeychainStore`] uses it only to read and remove passwords stored there by older versions
/// (`SPEC-0008` v8 behaviour 18); nothing new is written to it.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry(reference: &SecretRef) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&reference.service, &reference.account).map_err(|e| match e {
            // keyring reports a session without a usable keyring (Linux: none, or an expired one
            // inherited through `sudo`) as `NoEntry` already when the entry is made: the store
            // cannot be reached, which is not "no password" (SPEC-0008 v6 behaviour 17).
            keyring::Error::NoEntry => SecretError {
                service: reference.service.clone(),
                account: reference.account.clone(),
                message: UNREACHABLE.into(),
            },
            e => store_error(reference, &e),
        })
    }
}

/// Why the system's credential store cannot be used in this session (SPEC-0008 v6 behaviour 17).
pub const UNREACHABLE: &str = "the system keychain cannot be reached in this session (on Linux: no usable session keyring for passwords of older versions); nothing was read, stored or deleted";

/// Whether the system's credential store can be reached in this session (SPEC-0008 v6
/// behaviour 17; v8 behaviour 18: on Linux the Secret Service on the session bus, with its default
/// keyring unlocked).
pub fn keychain_reachable() -> bool {
    #[cfg(target_os = "linux")]
    {
        secret_service::SecretService::connect(APPLICATION).is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        keyring::Entry::new("ferris-idea", "reachability-check").is_ok()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        match Self::entry(reference)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error(reference, &e)),
        }
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        Self::entry(reference)?
            .set_password(secret.expose())
            .map_err(|e| store_error(reference, &e))
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        match Self::entry(reference)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error(reference, &e)),
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl SecretStore for KeychainStore {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        KeyringStore.get(reference)
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        KeyringStore.set(reference, secret)
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        KeyringStore.delete(reference)
    }
}

#[cfg(target_os = "linux")]
impl SecretStore for KeychainStore {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        SecretServiceStore::with_legacy(KeyringStore).get(reference)
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        SecretServiceStore::with_legacy(KeyringStore).set(reference, secret)
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        SecretServiceStore::with_legacy(KeyringStore).delete(reference)
    }
}

/// Linux: passwords in the Secret Service, with `legacy` (normally the kernel keyring of older
/// versions) read once to move a password over (`SPEC-0008` v8 behaviour 18). [`KeychainStore`]
/// is this store with [`KeyringStore`]; another `legacy` is for tests.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub struct SecretServiceStore<L = KeyringStore> {
    legacy: L,
    /// The `application` attribute of the items.
    application: &'static str,
}

#[cfg(target_os = "linux")]
impl<L: SecretStore + Default> Default for SecretServiceStore<L> {
    fn default() -> Self {
        Self::with_legacy(L::default())
    }
}

#[cfg(target_os = "linux")]
impl<L: SecretStore> SecretServiceStore<L> {
    /// The Secret Service, moving passwords out of `legacy` when they are read.
    pub fn with_legacy(legacy: L) -> Self {
        Self {
            legacy,
            application: APPLICATION,
        }
    }

    /// The same store for the items older versions wrote under the product's former name
    /// (`SPEC-0356` behaviour 11).
    pub fn with_former_name(mut self) -> Self {
        self.application = FORMER_SERVICE;
        self
    }

    fn persistent(
        &self,
        reference: &SecretRef,
    ) -> Result<secret_service::SecretService, SecretError> {
        secret_service::SecretService::connect(self.application)
            .map_err(|reason| unreachable(reference, &reason))
    }
}

#[cfg(target_os = "linux")]
impl<L: SecretStore> SecretStore for SecretServiceStore<L> {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        persistent::get(self.persistent(reference), &self.legacy, reference)
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        persistent::set(self.persistent(reference), &self.legacy, reference, secret)
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        persistent::delete(self.persistent(reference), &self.legacy, reference)
    }
}

/// The refusal when the Secret Service cannot be used (`SPEC-0008` v8 behaviour 18): names the
/// reason and what to do, never a value. Contains "cannot be reached in this session" like the
/// other stores (behaviour 17).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn unreachable(reference: &SecretRef, reason: &str) -> SecretError {
    SecretError {
        service: reference.service.clone(),
        account: reference.account.clone(),
        message: format!(
            "the Secret Service (the desktop keyring, such as GNOME Keyring or KWallet) cannot be \
             reached in this session: {reason}. On Linux Ferris IDEA keeps passwords only there, \
             encrypted under your login keyring, never in a file; nothing was stored or deleted. \
             Log in to a desktop session with an unlocked keyring and try again"
        ),
    }
}

/// Which store a Linux password operation uses (`SPEC-0008` v8 behaviour 18). Platform-neutral so
/// its rules are tested everywhere; only the Linux [`SecretServiceStore`] calls it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod persistent {
    use super::{Secret, SecretError, SecretStore};
    use crate::SecretRef;

    /// The persistent store, or why it cannot be used; then `legacy` is read as before, and a
    /// password found only in `legacy` is moved: written to the persistent store and, once that
    /// worked, removed from `legacy`. A failed move still gives the password and leaves `legacy`
    /// as it was.
    pub fn get<P: SecretStore>(
        persistent: Result<P, SecretError>,
        legacy: &dyn SecretStore,
        reference: &SecretRef,
    ) -> Result<Option<Secret>, SecretError> {
        let persistent = match persistent {
            Ok(store) => store,
            Err(_) => return legacy.get(reference),
        };
        if let Some(secret) = persistent.get(reference)? {
            return Ok(Some(secret));
        }
        // A kernel keyring that cannot be reached holds nothing to move.
        let Ok(Some(old)) = legacy.get(reference) else {
            return Ok(None);
        };
        if persistent.set(reference, &old).is_ok() {
            // A copy that cannot be removed ends with the session; the persistent one wins.
            let _ = legacy.delete(reference);
        }
        Ok(Some(old))
    }

    /// Stores only in the persistent store; without one the save fails with its reason and
    /// nothing weaker is tried. An older copy in `legacy` is removed.
    pub fn set<P: SecretStore>(
        persistent: Result<P, SecretError>,
        legacy: &dyn SecretStore,
        reference: &SecretRef,
        secret: &Secret,
    ) -> Result<(), SecretError> {
        persistent?.set(reference, secret)?;
        let _ = legacy.delete(reference);
        Ok(())
    }

    /// Removes the password from both stores. Without the persistent store the removal fails,
    /// since a password there would be left behind (behaviour 17).
    pub fn delete<P: SecretStore>(
        persistent: Result<P, SecretError>,
        legacy: &dyn SecretStore,
        reference: &SecretRef,
    ) -> Result<(), SecretError> {
        let persistent = persistent?;
        persistent.delete(reference)?;
        let _ = legacy.delete(reference);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod secret_service {
    //! The Secret Service (freedesktop.org) on the session bus through `oo7` over `zbus`, the same
    //! crate and features as the remote key store (`ADR-0025` §3, `SPEC-0321` behaviour 20). Only
    //! an encrypted Secret Service session and the default collection are used, and nothing is
    //! unlocked here: a locked keyring is refused instead of showing a prompt (resolving
    //! credentials never waits for input, behaviour 13).

    use std::collections::HashMap;

    use futures_lite::future::block_on;
    use oo7::dbus::{Collection, Service};

    use super::{Secret, SecretError, SecretStore};
    use crate::SecretRef;

    pub struct SecretService {
        // Keeps the D-Bus connection and the encrypted session open for the collection.
        _service: Service<'static>,
        collection: Collection<'static>,
        application: &'static str,
    }

    impl SecretService {
        fn attributes<'a>(&self, reference: &'a SecretRef) -> HashMap<&'a str, &'a str> {
            HashMap::from([
                ("application", self.application),
                ("service", reference.service.as_str()),
                ("account", reference.account.as_str()),
            ])
        }

        /// Connects and checks the default collection; the error is the reason, without values.
        /// Items carry `application` as their application attribute.
        pub fn connect(application: &'static str) -> Result<Self, String> {
            block_on(async {
                let service = Service::encrypted().await.map_err(|e| {
                    format!("no Secret Service answers on the session bus over an encrypted session ({e})")
                })?;
                let collection = service
                    .with_alias(Service::DEFAULT_COLLECTION)
                    .await
                    .map_err(|e| format!("the Secret Service failed ({e})"))?
                    .ok_or_else(|| "the Secret Service has no default keyring".to_string())?;
                let locked = collection
                    .is_locked()
                    .await
                    .map_err(|e| format!("the Secret Service failed ({e})"))?;
                if locked {
                    return Err("the default keyring of the Secret Service is locked".into());
                }
                Ok(Self {
                    _service: service,
                    collection,
                    application,
                })
            })
        }
    }

    fn failed(reference: &SecretRef, e: &oo7::dbus::Error) -> SecretError {
        SecretError {
            service: reference.service.clone(),
            account: reference.account.clone(),
            message: format!("the Secret Service failed: {e}"),
        }
    }

    impl SecretStore for SecretService {
        fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
            block_on(async {
                let items = self
                    .collection
                    .search_items(&self.attributes(reference))
                    .await
                    .map_err(|e| failed(reference, &e))?;
                let Some(item) = items.first() else {
                    return Ok(None);
                };
                let secret = item.secret().await.map_err(|e| failed(reference, &e))?;
                let text = std::str::from_utf8(secret.as_bytes()).map_err(|_| SecretError {
                    service: reference.service.clone(),
                    account: reference.account.clone(),
                    message: "the Secret Service holds an entry that is not text".into(),
                })?;
                Ok(Some(Secret::new(text)))
            })
        }

        fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
            block_on(async {
                self.collection
                    .create_item(
                        &format!(
                            "Ferris IDEA password ({}/{})",
                            reference.service, reference.account
                        ),
                        &self.attributes(reference),
                        oo7::Secret::text(secret.expose()),
                        true,
                        None,
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| failed(reference, &e))
            })
        }

        fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
            block_on(async {
                let items = self
                    .collection
                    .search_items(&self.attributes(reference))
                    .await
                    .map_err(|e| failed(reference, &e))?;
                for item in items {
                    item.delete(None).await.map_err(|e| failed(reference, &e))?;
                }
                Ok(())
            })
        }
    }
}

fn store_error(reference: &SecretRef, error: &keyring::Error) -> SecretError {
    SecretError {
        service: reference.service.clone(),
        account: reference.account.clone(),
        message: error.to_string(),
    }
}

/// The `application` attribute of the product's Secret Service items (Linux).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const APPLICATION: &str = "ferris-idea";

/// The keychain service of database passwords under the product's former name, and the start of
/// the other services it named (`SPEC-0356` behaviour 11).
const FORMER_SERVICE: &str = "idecraft"; // rebrand:keep

/// The vault key's service under the product's former name.
const FORMER_VAULT_KEY_SERVICE: &str = "idecraft-vault-key"; // rebrand:keep

/// What follows the product's short name in the keychain services that change with it: database
/// passwords, the API keys of AI endpoints and the vault key.
const RENAMED_SUFFIXES: [&str; 3] = ["", "-ai", "-vault-key"];

/// (former service, today's service) for every keychain service the product's new name changed
/// (`SPEC-0356` behaviour 11); empty while the names are the same.
pub fn renamed_services() -> Vec<(String, String)> {
    let current = crate::KEYCHAIN_SERVICE;
    if FORMER_SERVICE == current {
        return Vec::new();
    }
    RENAMED_SUFFIXES
        .iter()
        .map(|s| (format!("{FORMER_SERVICE}{s}"), format!("{current}{s}")))
        .collect()
}

/// `reference` with a service of the product's former name replaced by today's (`SPEC-0356`
/// behaviour 11): a profile saved by an older version names the entry it keeps today.
pub fn current_secret_ref(reference: &SecretRef) -> SecretRef {
    let mut out = reference.clone();
    if let Some((_, new)) = renamed_services()
        .into_iter()
        .find(|(old, _)| *old == reference.service)
    {
        out.service = new;
    }
    out
}

/// Whether `service` is the keychain service of the vault's key, under today's or the product's
/// former name (`SPEC-0353` behaviour 15, `SPEC-0356` behaviour 11).
pub fn is_vault_key_service(service: &str) -> bool {
    service == VAULT_KEY_SERVICE || service == FORMER_VAULT_KEY_SERVICE
}

/// A store that still finds the entries older versions saved under the product's former keychain
/// service names (`SPEC-0356` behaviour 11). `current` holds today's services, `former` the old
/// ones (the same keychain; on Linux with the former application attribute); `renames` pairs
/// (former, today's) service names, as [`renamed_services`] gives them.
///
/// Today's service is read first. An entry found only under the former service is returned and
/// copied to today's; the former entry is removed only after the copy reads back the same value,
/// so a failed or partial copy loses nothing. Values are never logged or put into an error.
/// Writing goes to today's service (and removes a former entry once the write reads back);
/// deleting removes both.
#[derive(Debug, Clone)]
pub struct RenamedStore<C, F> {
    current: C,
    former: F,
    renames: Vec<(String, String)>,
}

impl<C: SecretStore, F: SecretStore> RenamedStore<C, F> {
    /// A store over `current` and `former` with the service `renames`.
    pub fn new(current: C, former: F, renames: Vec<(String, String)>) -> Self {
        Self {
            current,
            former,
            renames,
        }
    }

    fn today(&self, reference: &SecretRef) -> SecretRef {
        let mut out = reference.clone();
        if let Some((_, new)) = self.renames.iter().find(|(old, _)| *old == out.service) {
            out.service = new.clone();
        }
        out
    }

    fn before(&self, today: &SecretRef) -> Option<SecretRef> {
        let (old, _) = self.renames.iter().find(|(_, new)| *new == today.service)?;
        Some(SecretRef {
            service: old.clone(),
            account: today.account.clone(),
        })
    }

    /// Whether today's entry reads back exactly `secret`.
    fn holds(&self, today: &SecretRef, secret: &Secret) -> bool {
        matches!(self.current.get(today), Ok(Some(found)) if found == *secret)
    }
}

impl<C: SecretStore, F: SecretStore> SecretStore for RenamedStore<C, F> {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        let today = self.today(reference);
        if let Some(secret) = self.current.get(&today)? {
            return Ok(Some(secret));
        }
        let Some(old) = self.before(&today) else {
            return Ok(None);
        };
        let Some(secret) = self.former.get(&old)? else {
            return Ok(None);
        };
        if self.current.set(&today, &secret).is_ok() && self.holds(&today, &secret) {
            // A former entry that cannot be removed stays; today's is read first from now on.
            let _ = self.former.delete(&old);
        }
        Ok(Some(secret))
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        let today = self.today(reference);
        self.current.set(&today, secret)?;
        if let Some(old) = self.before(&today)
            && self.holds(&today, secret)
        {
            let _ = self.former.delete(&old);
        }
        Ok(())
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        let today = self.today(reference);
        self.current.delete(&today)?;
        if let Some(old) = self.before(&today) {
            self.former.delete(&old)?;
        }
        Ok(())
    }
}

/// The system's credential store for the entries older versions wrote under the product's former
/// name: [`KeychainStore`] itself on macOS and Windows (only the service names differ), on Linux
/// the Secret Service items with the former application attribute (`SPEC-0356` behaviour 11).
#[derive(Debug, Default, Clone, Copy)]
pub struct FormerKeychainStore;

impl SecretStore for FormerKeychainStore {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        #[cfg(target_os = "linux")]
        {
            SecretServiceStore::with_legacy(KeyringStore)
                .with_former_name()
                .get(reference)
        }
        #[cfg(not(target_os = "linux"))]
        {
            KeychainStore.get(reference)
        }
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        #[cfg(target_os = "linux")]
        {
            SecretServiceStore::with_legacy(KeyringStore)
                .with_former_name()
                .set(reference, secret)
        }
        #[cfg(not(target_os = "linux"))]
        {
            KeychainStore.set(reference, secret)
        }
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        #[cfg(target_os = "linux")]
        {
            SecretServiceStore::with_legacy(KeyringStore)
                .with_former_name()
                .delete(reference)
        }
        #[cfg(not(target_os = "linux"))]
        {
            KeychainStore.delete(reference)
        }
    }
}

/// The store Ferris IDEA uses for passwords: the operating system's credential store
/// ([`KeychainStore`]). Only in debug builds (`debug_assertions`) and only while
/// `FERRIS_IDEA_TEST_SECRETS_DIR` is set, a [`test_store::DirStore`] in that folder takes its place, so
/// tests never touch (or prompt for) the keychain (SPEC-0008 v3, behaviour 14). Release builds
/// contain neither the variable nor the folder store.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemStore;

impl SystemStore {
    /// The test secrets folder in effect: the one a test program named with
    /// [`test_store::use_dir`], else `FERRIS_IDEA_TEST_SECRETS_DIR`; always `None` in release builds.
    pub fn test_secrets_dir() -> Option<std::path::PathBuf> {
        #[cfg(debug_assertions)]
        {
            test_store::chosen_dir().or_else(|| {
                ferris_idea_core::product_var_os(test_store::TEST_SECRETS_ENV)
                    .filter(|dir| !dir.is_empty())
                    .map(std::path::PathBuf::from)
            })
        }
        #[cfg(not(debug_assertions))]
        {
            None
        }
    }
}

/// Runs `op` on [`SystemStore`]'s store, which also finds the entries of the product's former
/// name (`SPEC-0356` behaviour 11); debug builds with a test secrets folder use that folder.
fn with_system_store<T>(op: impl FnOnce(&dyn SecretStore) -> T) -> T {
    #[cfg(debug_assertions)]
    if let Some(dir) = SystemStore::test_secrets_dir() {
        let store = test_store::DirStore::new(dir);
        return op(&RenamedStore::new(&store, &store, renamed_services()));
    }
    op(&RenamedStore::new(
        KeychainStore,
        FormerKeychainStore,
        renamed_services(),
    ))
}

impl SecretStore for SystemStore {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        with_system_store(|s| s.get(reference))
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        with_system_store(|s| s.set(reference, secret))
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        with_system_store(|s| s.delete(reference))
    }
}

/// Test-only password store (debug builds only, SPEC-0008 v3 behaviour 14).
#[cfg(debug_assertions)]
pub mod test_store {
    use std::path::{Path, PathBuf};

    use super::{Secret, SecretError, SecretStore};
    use crate::SecretRef;

    /// Names the folder of the test store; honoured only in debug builds.
    pub const TEST_SECRETS_ENV: &str = "FERRIS_IDEA_TEST_SECRETS_DIR";

    static CHOSEN: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

    /// Uses the test store in `dir` for the rest of this process, like `FERRIS_IDEA_TEST_SECRETS_DIR`
    /// but without changing the environment (test programs run their tests on several threads and
    /// may not set variables). The first folder named stays; the folder this call settled on.
    pub fn use_dir(dir: impl Into<PathBuf>) -> PathBuf {
        CHOSEN.get_or_init(|| dir.into()).clone()
    }

    pub(super) fn chosen_dir() -> Option<PathBuf> {
        CHOSEN.get().cloned()
    }

    /// Passwords as files `<root>/<service>/<account>`. The root and service folders must have
    /// mode 0700 and the files 0600 (no symbolic links); anything else is refused, naming the path
    /// but never a value. A missing root or file means "no password". A trailing line break is
    /// not part of the password.
    #[derive(Debug, Clone)]
    pub struct DirStore {
        root: PathBuf,
    }

    impl DirStore {
        /// A store in `root`.
        pub fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }

        fn file(&self, reference: &SecretRef) -> Result<PathBuf, SecretError> {
            for part in [&reference.service, &reference.account] {
                if part.is_empty()
                    || part == "."
                    || part == ".."
                    || part.contains(['/', '\\', '\0'])
                {
                    return Err(fail(reference, "service and account must be plain names"));
                }
            }
            Ok(self.root.join(&reference.service).join(&reference.account))
        }
    }

    impl SecretStore for DirStore {
        fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
            let file = self.file(reference)?;
            if !self.root.exists() || !file.exists() {
                return Ok(None);
            }
            check_mode(reference, &self.root, 0o700, true)?;
            check_mode(reference, file.parent().unwrap_or(&self.root), 0o700, true)?;
            check_mode(reference, &file, 0o600, false)?;
            let text = std::fs::read_to_string(&file)
                .map_err(|e| fail(reference, &format!("{}: {e}", file.display())))?;
            Ok(Some(Secret::new(text.trim_end_matches(['\n', '\r']))))
        }

        fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
            let file = self.file(reference)?;
            check_mode(reference, &self.root, 0o700, true)?;
            let dir = file.parent().unwrap_or(&self.root);
            if !dir.exists() {
                create_private_dir(reference, dir)?;
            }
            check_mode(reference, dir, 0o700, true)?;
            write_private(reference, &file, secret.expose())
        }

        fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
            let file = self.file(reference)?;
            match std::fs::remove_file(&file) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(fail(reference, &format!("{}: {e}", file.display()))),
            }
        }
    }

    fn fail(reference: &SecretRef, message: &str) -> SecretError {
        SecretError {
            service: reference.service.clone(),
            account: reference.account.clone(),
            message: format!("test secrets ({TEST_SECRETS_ENV}): {message}"),
        }
    }

    #[cfg(unix)]
    fn check_mode(
        reference: &SecretRef,
        path: &Path,
        mode: u32,
        dir: bool,
    ) -> Result<(), SecretError> {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(path)
            .map_err(|e| fail(reference, &format!("{}: {e}", path.display())))?;
        let kind_ok = if dir { meta.is_dir() } else { meta.is_file() };
        let actual = meta.permissions().mode() & 0o777;
        if !kind_ok || actual != mode {
            return Err(fail(
                reference,
                &format!(
                    "{} must be a {} with mode {mode:o} (is {actual:o}{})",
                    path.display(),
                    if dir { "folder" } else { "file" },
                    if kind_ok { "" } else { ", wrong kind" }
                ),
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn check_mode(
        reference: &SecretRef,
        _path: &Path,
        _mode: u32,
        _dir: bool,
    ) -> Result<(), SecretError> {
        Err(fail(reference, "only supported on Unix"))
    }

    #[cfg(unix)]
    fn create_private_dir(reference: &SecretRef, dir: &Path) -> Result<(), SecretError> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|e| fail(reference, &format!("{}: {e}", dir.display())))
    }

    #[cfg(not(unix))]
    fn create_private_dir(reference: &SecretRef, _dir: &Path) -> Result<(), SecretError> {
        Err(fail(reference, "only supported on Unix"))
    }

    #[cfg(unix)]
    fn write_private(reference: &SecretRef, file: &Path, value: &str) -> Result<(), SecretError> {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let err = |e: std::io::Error| fail(reference, &format!("{}: {e}", file.display()));
        if std::fs::symlink_metadata(file).is_ok_and(|m| !m.is_file()) {
            return Err(fail(
                reference,
                &format!("{} is not a file", file.display()),
            ));
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(file)
            .map_err(err)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(err)?;
        f.write_all(value.as_bytes()).map_err(err)
    }

    #[cfg(not(unix))]
    fn write_private(reference: &SecretRef, _file: &Path, _value: &str) -> Result<(), SecretError> {
        Err(fail(reference, "only supported on Unix"))
    }
}

/// A store in memory, for tests and for sessions that must not touch the keychain.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<(String, String), Secret>>);

impl SecretStore for MemoryStore {
    fn get(&self, reference: &SecretRef) -> Result<Option<Secret>, SecretError> {
        Ok(self.map().get(&key(reference)).cloned())
    }

    fn set(&self, reference: &SecretRef, secret: &Secret) -> Result<(), SecretError> {
        self.map().insert(key(reference), secret.clone());
        Ok(())
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretError> {
        self.map().remove(&key(reference));
        Ok(())
    }
}

impl MemoryStore {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), Secret>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn key(reference: &SecretRef) -> (String, String) {
    (reference.service.clone(), reference.account.clone())
}

/// What is known about a profile's password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credentials {
    /// The profile has no secret reference.
    NotNeeded,
    /// The password was found.
    Available(Secret),
    /// The profile names a keychain entry that does not exist (yet).
    Missing {
        /// Profile name.
        profile: String,
        /// Entry that should hold the password.
        reference: SecretRef,
    },
}

/// The keychain service of the vault's key (`SPEC-0353` behaviour 15; the same text as
/// `ferris_idea_vault::KEY_SERVICE`).
pub const VAULT_KEY_SERVICE: &str = "ferris-idea-vault-key";

/// Looks up the password of `profile` without waiting for input.
pub fn resolve_credentials(
    profile: &ConnectionProfile,
    store: &dyn SecretStore,
) -> Result<Credentials, SecretError> {
    let Some(reference) = &profile.secret else {
        return Ok(Credentials::NotNeeded);
    };
    // The vault's own key store item is never a database password, whatever a profile names
    // (`SPEC-0353` behaviour 15).
    if is_vault_key_service(&reference.service) {
        return Err(SecretError {
            service: reference.service.clone(),
            account: reference.account.clone(),
            message: "this keychain entry holds the key of Ferris IDEA's vault and is never used as a password".into(),
        });
    }
    Ok(match store.get(reference)? {
        Some(secret) => Credentials::Available(secret),
        None => Credentials::Missing {
            profile: profile.name.clone(),
            reference: reference.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_with(secret: Option<SecretRef>) -> ConnectionProfile {
        let mut p =
            ConnectionProfile::from_url("ref-pg", "postgresql://ferris-idea@127.0.0.1/inventory")
                .unwrap();
        p.secret = secret;
        p
    }

    fn reference() -> SecretRef {
        SecretRef {
            service: "ferris-idea-test".into(),
            account: "ref-pg".into(),
        }
    }

    #[test]
    fn resolves_not_needed_available_and_missing() {
        let store = MemoryStore::default();
        assert_eq!(
            resolve_credentials(&profile_with(None), &store).unwrap(),
            Credentials::NotNeeded
        );

        let profile = profile_with(Some(reference()));
        assert_eq!(
            resolve_credentials(&profile, &store).unwrap(),
            Credentials::Missing {
                profile: "ref-pg".into(),
                reference: reference(),
            }
        );

        store.set(&reference(), &Secret::new("pw")).unwrap();
        match resolve_credentials(&profile, &store).unwrap() {
            Credentials::Available(secret) => assert_eq!(secret.expose(), "pw"),
            other => panic!("expected password, got {other:?}"),
        }

        store.delete(&reference()).unwrap();
        store.delete(&reference()).unwrap();
        assert!(store.get(&reference()).unwrap().is_none());
    }

    /// SPEC-0353 behaviour 15: a profile naming the vault's key store item gets no password,
    /// even when the store holds the item.
    #[test]
    fn the_vault_key_is_never_a_password() {
        let store = MemoryStore::default();
        let item = SecretRef {
            service: VAULT_KEY_SERVICE.into(),
            account: "vault-key".into(),
        };
        store.set(&item, &Secret::new("not-a-real-key")).unwrap();
        let err = resolve_credentials(&profile_with(Some(item)), &store).unwrap_err();
        assert!(err.message.contains("never used as a password"), "{err}");
    }

    /// SPEC-0356: the stores and services of the rename, for the tests below.
    mod renamed {
        use super::*;

        pub(super) fn renames() -> Vec<(String, String)> {
            vec![("old-svc".into(), "new-svc".into())]
        }

        pub(super) fn at(service: &str) -> SecretRef {
            SecretRef {
                service: service.into(),
                account: "connection:shop".into(),
            }
        }

        /// A store whose writes fail.
        #[derive(Default)]
        pub(super) struct NoWrites(pub MemoryStore);

        impl SecretStore for NoWrites {
            fn get(&self, r: &SecretRef) -> Result<Option<Secret>, SecretError> {
                self.0.get(r)
            }
            fn set(&self, r: &SecretRef, _: &Secret) -> Result<(), SecretError> {
                Err(SecretError {
                    service: r.service.clone(),
                    account: r.account.clone(),
                    message: "refused".into(),
                })
            }
            fn delete(&self, r: &SecretRef) -> Result<(), SecretError> {
                self.0.delete(r)
            }
        }

        /// A store that keeps something other than what it was given.
        #[derive(Default)]
        pub(super) struct Garbles(pub MemoryStore);

        impl SecretStore for Garbles {
            fn get(&self, r: &SecretRef) -> Result<Option<Secret>, SecretError> {
                self.0.get(r)
            }
            fn set(&self, r: &SecretRef, s: &Secret) -> Result<(), SecretError> {
                self.0
                    .set(r, &Secret::new(format!("{}-cut", &s.expose()[..1])))
            }
            fn delete(&self, r: &SecretRef) -> Result<(), SecretError> {
                self.0.delete(r)
            }
        }
    }

    /// SPEC-0356 AK-10 (behaviour 11): an entry saved under the former service is found, copied
    /// to today's service and removed from the former one only after the copy reads back.
    #[test]
    fn an_entry_under_the_former_service_moves_after_a_verified_copy() {
        use renamed::*;
        let keychain = MemoryStore::default();
        keychain.set(&at("old-svc"), &Secret::new("pw-1")).unwrap();
        let store = RenamedStore::new(&keychain, &keychain, renames());
        // Asked by today's name or by the former name a profile still carries.
        for asked in ["new-svc", "old-svc"] {
            assert_eq!(store.get(&at(asked)).unwrap().unwrap().expose(), "pw-1");
        }
        assert_eq!(
            keychain.get(&at("new-svc")).unwrap().unwrap().expose(),
            "pw-1"
        );
        assert!(keychain.get(&at("old-svc")).unwrap().is_none());
        // Other services are left alone.
        keychain.set(&at("other"), &Secret::new("pw-2")).unwrap();
        assert_eq!(store.get(&at("other")).unwrap().unwrap().expose(), "pw-2");
        assert!(store.get(&at("missing")).unwrap().is_none());
    }

    /// SPEC-0356 AK-10: today's entry wins; the former one is not read or touched.
    #[test]
    fn todays_entry_wins_over_the_former_one() {
        use renamed::*;
        let keychain = MemoryStore::default();
        keychain.set(&at("old-svc"), &Secret::new("older")).unwrap();
        keychain.set(&at("new-svc"), &Secret::new("newer")).unwrap();
        let store = RenamedStore::new(&keychain, &keychain, renames());
        assert_eq!(
            store.get(&at("old-svc")).unwrap().unwrap().expose(),
            "newer"
        );
        assert_eq!(
            keychain.get(&at("old-svc")).unwrap().unwrap().expose(),
            "older"
        );
    }

    /// SPEC-0356 AK-10: a copy that fails or reads back differently keeps the former entry; the
    /// password is still given.
    #[test]
    fn a_failed_or_wrong_copy_keeps_the_former_entry() {
        use renamed::*;
        let former = MemoryStore::default();
        former.set(&at("old-svc"), &Secret::new("pw-3")).unwrap();
        let refusing = NoWrites::default();
        let store = RenamedStore::new(&refusing, &former, renames());
        assert_eq!(store.get(&at("new-svc")).unwrap().unwrap().expose(), "pw-3");
        assert!(former.get(&at("old-svc")).unwrap().is_some());

        let garbling = Garbles::default();
        let store = RenamedStore::new(&garbling, &former, renames());
        assert_eq!(store.get(&at("new-svc")).unwrap().unwrap().expose(), "pw-3");
        assert_eq!(
            former.get(&at("old-svc")).unwrap().unwrap().expose(),
            "pw-3"
        );
    }

    /// SPEC-0356 AK-10: saving writes today's service and then drops the former entry; deleting
    /// removes both; a failure of the former store is an error that names no value.
    #[test]
    fn saving_and_deleting_cover_both_names() {
        use renamed::*;
        let keychain = MemoryStore::default();
        keychain.set(&at("old-svc"), &Secret::new("stale")).unwrap();
        let store = RenamedStore::new(&keychain, &keychain, renames());
        store.set(&at("old-svc"), &Secret::new("fresh")).unwrap();
        assert_eq!(
            keychain.get(&at("new-svc")).unwrap().unwrap().expose(),
            "fresh"
        );
        assert!(keychain.get(&at("old-svc")).unwrap().is_none());

        keychain.set(&at("old-svc"), &Secret::new("again")).unwrap();
        store.delete(&at("new-svc")).unwrap();
        assert!(keychain.get(&at("new-svc")).unwrap().is_none());
        assert!(keychain.get(&at("old-svc")).unwrap().is_none());

        struct Broken;
        impl SecretStore for Broken {
            fn get(&self, r: &SecretRef) -> Result<Option<Secret>, SecretError> {
                Err(SecretError {
                    service: r.service.clone(),
                    account: r.account.clone(),
                    message: "locked".into(),
                })
            }
            fn set(&self, _: &SecretRef, _: &Secret) -> Result<(), SecretError> {
                unreachable!()
            }
            fn delete(&self, r: &SecretRef) -> Result<(), SecretError> {
                self.get(r).map(|_| ())
            }
        }
        let empty = MemoryStore::default();
        let store = RenamedStore::new(&empty, Broken, renames());
        let err = store.get(&at("new-svc")).unwrap_err();
        assert_eq!(err.service, "old-svc");
        assert!(store.delete(&at("new-svc")).is_err());
    }

    /// SPEC-0356 AK-10: through the test store of debug builds, the copy is a private file like
    /// any password it holds.
    #[cfg(all(unix, debug_assertions))]
    #[test]
    fn the_copy_in_the_test_store_is_private() {
        use renamed::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("renamed-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let files = test_store::DirStore::new(&dir);
        files.set(&at("old-svc"), &Secret::new("pw-4")).unwrap();
        let store = RenamedStore::new(&files, &files, renames());
        assert_eq!(store.get(&at("new-svc")).unwrap().unwrap().expose(), "pw-4");
        let copy = dir.join("new-svc/connection:shop");
        assert_eq!(
            std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!dir.join("old-svc/connection:shop").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0356 AK-11: the renamed services are the product's own (database passwords, AI keys,
    /// the vault key), map onto today's names, and are none while the names are the same.
    #[test]
    fn the_renamed_services_are_the_products_own() {
        assert_eq!(
            VAULT_KEY_SERVICE,
            format!("{}-vault-key", crate::KEYCHAIN_SERVICE)
        );
        let renames = renamed_services();
        if FORMER_SERVICE == crate::KEYCHAIN_SERVICE {
            assert!(renames.is_empty());
        } else {
            assert_eq!(renames.len(), 3);
            assert!(renames.contains(&(FORMER_VAULT_KEY_SERVICE.into(), VAULT_KEY_SERVICE.into())));
            assert!(renames.contains(&(FORMER_SERVICE.into(), crate::KEYCHAIN_SERVICE.into())));
        }
        let old = SecretRef {
            service: FORMER_SERVICE.into(),
            account: "connection:shop".into(),
        };
        assert_eq!(current_secret_ref(&old), crate::secret_ref("shop"));
    }

    /// SPEC-0356 AK-11 (SPEC-0353 behaviour 15): the vault's key is never a password, also when a
    /// profile names it by the product's former service.
    #[test]
    fn the_vault_key_under_the_former_name_is_never_a_password() {
        let store = MemoryStore::default();
        let item = SecretRef {
            service: FORMER_VAULT_KEY_SERVICE.into(),
            account: "vault-key".into(),
        };
        store.set(&item, &Secret::new("not-a-real-key")).unwrap();
        let err = resolve_credentials(&profile_with(Some(item)), &store).unwrap_err();
        assert!(err.message.contains("never used as a password"), "{err}");
        assert!(!err.to_string().contains("not-a-real-key"));
        assert!(is_vault_key_service(VAULT_KEY_SERVICE));
        assert!(!is_vault_key_service(crate::KEYCHAIN_SERVICE));
    }

    #[test]
    fn secrets_never_print() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "***");
        assert_eq!(secret.to_string(), "***");
        let available = Credentials::Available(secret);
        assert!(!format!("{available:?}").contains("hunter2"));
    }

    /// AK-11: run by hand on a Mac: `cargo test -p ferris-idea-database -- --ignored keychain`.
    #[test]
    #[ignore = "touches the real system keychain"]
    fn keychain_round_trip() {
        let store = KeychainStore;
        let reference = SecretRef {
            service: "ferris-idea-selftest".into(),
            account: format!("probe-{}", std::process::id()),
        };
        store.set(&reference, &Secret::new("throw-away")).unwrap();
        assert_eq!(
            store.get(&reference).unwrap().unwrap().expose(),
            "throw-away"
        );
        store.delete(&reference).unwrap();
        assert!(store.get(&reference).unwrap().is_none());
    }

    /// SPEC-0008 v8 AK-18: which store a Linux password operation uses, tested on every
    /// platform with in-memory stands-ins for the Secret Service and the kernel keyring.
    mod linux_store_rules {
        use super::super::persistent;
        use super::*;

        /// A store that cannot be reached; every call fails with `why`.
        struct Unreachable(&'static str);

        impl SecretStore for Unreachable {
            fn get(&self, r: &SecretRef) -> Result<Option<Secret>, SecretError> {
                Err(self.fail(r))
            }
            fn set(&self, r: &SecretRef, _: &Secret) -> Result<(), SecretError> {
                Err(self.fail(r))
            }
            fn delete(&self, r: &SecretRef) -> Result<(), SecretError> {
                Err(self.fail(r))
            }
        }

        impl Unreachable {
            fn fail(&self, r: &SecretRef) -> SecretError {
                SecretError {
                    service: r.service.clone(),
                    account: r.account.clone(),
                    message: self.0.into(),
                }
            }
        }

        fn no_service(r: &SecretRef) -> Result<&'static MemoryStore, SecretError> {
            Err(super::super::unreachable(r, "no session bus"))
        }

        fn leak(store: MemoryStore) -> &'static MemoryStore {
            Box::leak(Box::new(store))
        }

        #[test]
        fn saving_goes_only_to_the_secret_service_and_drops_an_old_copy() {
            let r = reference();
            let service = leak(MemoryStore::default());
            let kernel = MemoryStore::default();
            kernel.set(&r, &Secret::new("old")).unwrap();
            persistent::set(Ok(service), &kernel, &r, &Secret::new("new")).unwrap();
            assert_eq!(service.get(&r).unwrap().unwrap().expose(), "new");
            assert!(kernel.get(&r).unwrap().is_none());
            // A kernel keyring that cannot be reached does not stop the save.
            persistent::set(
                Ok(service),
                &Unreachable("no keyring"),
                &r,
                &Secret::new("x"),
            )
            .unwrap();
            assert_eq!(service.get(&r).unwrap().unwrap().expose(), "x");
        }

        #[test]
        fn without_a_secret_service_saving_fails_with_the_reason_and_writes_nothing() {
            let r = reference();
            let kernel = MemoryStore::default();
            let err = persistent::set(no_service(&r), &kernel, &r, &Secret::new("hunter2"))
                .unwrap_err()
                .to_string();
            assert!(err.contains("Secret Service"), "{err}");
            assert!(err.contains("cannot be reached in this session"), "{err}");
            assert!(err.contains("no session bus"), "{err}");
            assert!(err.contains("never in a file"), "{err}");
            assert!(!err.contains("hunter2"), "{err}");
            // Nothing weaker took the password instead.
            assert!(kernel.get(&r).unwrap().is_none());
        }

        #[test]
        fn reading_moves_a_kernel_keyring_password_once() {
            let r = reference();
            let service = leak(MemoryStore::default());
            let kernel = MemoryStore::default();
            kernel.set(&r, &Secret::new("from-kernel")).unwrap();
            let got = persistent::get(Ok(service), &kernel, &r).unwrap().unwrap();
            assert_eq!(got.expose(), "from-kernel");
            assert_eq!(service.get(&r).unwrap().unwrap().expose(), "from-kernel");
            assert!(kernel.get(&r).unwrap().is_none());
            // The Secret Service wins over a copy left in the kernel keyring.
            kernel.set(&r, &Secret::new("stale")).unwrap();
            let got = persistent::get(Ok(service), &kernel, &r).unwrap().unwrap();
            assert_eq!(got.expose(), "from-kernel");
        }

        #[test]
        fn a_failed_move_still_gives_the_password_and_keeps_the_old_copy() {
            let r = reference();
            let kernel = MemoryStore::default();
            kernel.set(&r, &Secret::new("keep")).unwrap();
            struct ReadOnly;
            impl SecretStore for ReadOnly {
                fn get(&self, _: &SecretRef) -> Result<Option<Secret>, SecretError> {
                    Ok(None)
                }
                fn set(&self, r: &SecretRef, _: &Secret) -> Result<(), SecretError> {
                    Err(Unreachable("refused").fail(r))
                }
                fn delete(&self, _: &SecretRef) -> Result<(), SecretError> {
                    Ok(())
                }
            }
            let got = persistent::get(Ok(ReadOnly), &kernel, &r).unwrap().unwrap();
            assert_eq!(got.expose(), "keep");
            assert_eq!(kernel.get(&r).unwrap().unwrap().expose(), "keep");
        }

        #[test]
        fn without_a_secret_service_reading_stays_as_before() {
            let r = reference();
            let kernel = MemoryStore::default();
            assert!(
                persistent::get(no_service(&r), &kernel, &r)
                    .unwrap()
                    .is_none()
            );
            kernel.set(&r, &Secret::new("session")).unwrap();
            let got = persistent::get(no_service(&r), &kernel, &r)
                .unwrap()
                .unwrap();
            assert_eq!(got.expose(), "session");
            // An unreachable kernel keyring is still named as such (behaviour 17).
            let err = persistent::get(no_service(&r), &Unreachable(UNREACHABLE), &r).unwrap_err();
            assert!(err.message.contains("cannot be reached in this session"));
            // A Secret Service that answers but holds nothing, with no kernel keyring: no password.
            let service = leak(MemoryStore::default());
            assert!(
                persistent::get(Ok(service), &Unreachable(UNREACHABLE), &r)
                    .unwrap()
                    .is_none()
            );
        }

        #[test]
        fn removing_clears_both_and_fails_without_a_secret_service() {
            let r = reference();
            let service = leak(MemoryStore::default());
            let kernel = MemoryStore::default();
            service.set(&r, &Secret::new("a")).unwrap();
            kernel.set(&r, &Secret::new("b")).unwrap();
            persistent::delete(Ok(service), &kernel, &r).unwrap();
            assert!(service.get(&r).unwrap().is_none());
            assert!(kernel.get(&r).unwrap().is_none());
            persistent::delete(Ok(service), &kernel, &r).unwrap();

            kernel.set(&r, &Secret::new("b")).unwrap();
            let err = persistent::delete(no_service(&r), &kernel, &r).unwrap_err();
            assert!(err.message.contains("cannot be reached in this session"));
        }
    }

    /// Behaviour 14: a release build neither contains nor honours the test store. Run by
    /// `scripts/ci-local.sh` as `FERRIS_IDEA_TEST_SECRETS_DIR=… cargo test --release … --ignored`.
    #[cfg(not(debug_assertions))]
    #[test]
    #[ignore = "needs FERRIS_IDEA_TEST_SECRETS_DIR set; run by scripts/ci-local.sh"]
    fn release_build_ignores_test_secrets_dir() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_TEST_SECRETS_DIR").is_some(),
            "run with FERRIS_IDEA_TEST_SECRETS_DIR set"
        );
        assert_eq!(SystemStore::test_secrets_dir(), None);
    }

    #[cfg(all(debug_assertions, unix))]
    mod dir_store {
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        use super::super::test_store::DirStore;
        use super::super::*;

        struct Dir(PathBuf);

        impl Dir {
            fn new(name: &str) -> Self {
                let path = std::env::temp_dir()
                    .join(format!("ferris-idea-secrets-{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).unwrap();
                chmod(&path, 0o700);
                Self(path)
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn chmod(path: &Path, mode: u32) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        fn mode(path: &Path) -> u32 {
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        fn reference(account: &str) -> SecretRef {
            SecretRef {
                service: "ferris-idea-test".into(),
                account: account.into(),
            }
        }

        #[test]
        fn round_trip_with_private_modes() {
            let dir = Dir::new("roundtrip");
            let store = DirStore::new(&dir.0);
            let r = reference("refdb-pg");
            assert!(store.get(&r).unwrap().is_none());
            store.set(&r, &Secret::new("value-one")).unwrap();
            let file = dir.0.join("ferris-idea-test/refdb-pg");
            assert_eq!(mode(&file), 0o600);
            assert_eq!(mode(&dir.0.join("ferris-idea-test")), 0o700);
            assert_eq!(store.get(&r).unwrap().unwrap().expose(), "value-one");
            store.delete(&r).unwrap();
            store.delete(&r).unwrap();
            assert!(store.get(&r).unwrap().is_none());
            // A missing folder is an empty store.
            assert!(DirStore::new(dir.0.join("none")).get(&r).unwrap().is_none());
        }

        #[test]
        fn a_trailing_line_break_is_not_part_of_the_password() {
            let dir = Dir::new("newline");
            let service = dir.0.join("ferris-idea-test");
            std::fs::create_dir(&service).unwrap();
            chmod(&service, 0o700);
            std::fs::write(service.join("a"), "value-three\n").unwrap();
            chmod(&service.join("a"), 0o600);
            let got = DirStore::new(&dir.0).get(&reference("a")).unwrap().unwrap();
            assert_eq!(got.expose(), "value-three");
        }

        #[test]
        fn loose_modes_and_odd_names_are_refused_without_the_value() {
            let dir = Dir::new("modes");
            let store = DirStore::new(&dir.0);
            let r = reference("a");
            store.set(&r, &Secret::new("value-two")).unwrap();
            let file = dir.0.join("ferris-idea-test/a");

            chmod(&file, 0o644);
            let err = store.get(&r).unwrap_err().to_string();
            assert!(err.contains("600") && !err.contains("value-two"), "{err}");
            chmod(&file, 0o600);

            chmod(&dir.0, 0o755);
            let err = store.get(&r).unwrap_err().to_string();
            assert!(err.contains("700"), "{err}");
            assert!(store.set(&r, &Secret::new("x")).is_err());
            chmod(&dir.0, 0o700);

            for bad in ["..", "a/b", ""] {
                assert!(store.get(&reference(bad)).is_err(), "{bad}");
            }
            assert_eq!(store.get(&r).unwrap().unwrap().expose(), "value-two");
        }
    }
}
