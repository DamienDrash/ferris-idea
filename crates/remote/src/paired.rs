//! A folder on a paired host as a remote project (`SPEC-0321` behaviours 13–23): a session over the
//! encrypted paired channel to a running `ferris-idea serve --pair`. A broken channel is resumed
//! with a new handshake; a restarted backend means a new session with the documents reconciled.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_pairing::{
    ChildPipe, Identity, KeyError, PairError, PairLink, PendingPairing, Side, Store, connect_known,
    connect_known_on, default_device_name, start_pairing, start_pairing_on,
    start_pairing_with_link,
};
use ferris_idea_session::{ClientSession, ErrorCode, Reconnected, Recovery};
use serde_json::{Value, json};

use crate::project::Unsaved;
use crate::ssh::RemoteError;
use crate::{SshCommand, SshPurpose};

static SSH_EXTRA: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Extra `ssh` arguments for the SSH path (a private `-F` for tests); never read from outside.
pub fn set_ssh_extra(extra: Vec<String>) {
    *SSH_EXTRA.lock().unwrap_or_else(|e| e.into_inner()) = extra;
}

/// The SSH tunnel of a path `<alias>[:<port>]@ssh`: alias and port.
fn ssh_path(address: &str) -> Option<(&str, u16)> {
    let bare = address.strip_suffix("@ssh")?;
    let (alias, port) = bare.rsplit_once(':')?;
    Some((alias, port.parse().ok()?))
}

/// The `ssh -W` command of an SSH path, with all the rules of [`SshCommand`] (`SPEC-0324`
/// behaviour 6). The listener on the far side stays on its loopback address.
fn ssh_line(alias: &str, port: u16) -> Result<crate::SshLine, RemoteError> {
    let extra = SSH_EXTRA.lock().unwrap_or_else(|e| e.into_inner()).clone();
    SshCommand::new(SshPurpose::Tunnel)
        .args(["-W".to_owned(), format!("127.0.0.1:{port}")])
        .extra(&extra)
        .build(alias)
}

/// A pipe through `ssh -W` and what `ssh` said on standard error.
pub struct SshPipe {
    /// The byte pipe.
    pub pipe: ChildPipe,
    /// The diagnostics `ssh` printed, for a refusal that happened before the handshake.
    pub said: Arc<Mutex<String>>,
}

/// A byte pipe to the listener of an SSH path.
pub fn ssh_pipe(alias: &str, port: u16) -> Result<SshPipe, RemoteError> {
    let line = ssh_line(alias, port)?;
    let mut child = line
        .command()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| RemoteError::Failed(format!("{alias}: cannot start ssh: {e}")))?;
    let said = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = child.stderr.take() {
        let said = Arc::clone(&said);
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            while let Ok(n) = std::io::Read::read(&mut stderr, &mut buf) {
                if n == 0 {
                    return;
                }
                let mut text = said.lock().unwrap_or_else(|e| e.into_inner());
                if text.len() < 4096 {
                    text.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
            }
        });
    }
    let pipe = ChildPipe::new(child)
        .ok_or_else(|| RemoteError::Failed(format!("{alias}: ssh gave no pipes")))?;
    Ok(SshPipe { pipe, said })
}

/// A failure on an SSH path, named after what `ssh` said when it said anything: a refused login
/// must not read like a changed host key.
fn ssh_failed(alias: &str, said: &Mutex<String>, e: PairError) -> RemoteError {
    std::thread::sleep(Duration::from_millis(100));
    let text = said.lock().unwrap_or_else(|e| e.into_inner());
    let text = text.trim();
    if text.is_empty() || matches!(e, PairError::Declined) {
        pair_error(e)
    } else {
        RemoteError::Failed(format!("{alias}: ssh: {text}"))
    }
}

/// Connects to the key pinned for `address`, over the path its string names.
fn connect_path(
    address: &str,
    store: &Store,
    identity: &Identity,
    device: &str,
) -> Result<ferris_idea_session::Transport, RemoteError> {
    let Some((alias, port)) = ssh_path(address) else {
        return connect_known(address, store, identity, device).map_err(pair_error);
    };
    let pinned = store
        .host_key(address)
        .map_err(key_error)?
        .ok_or_else(|| pair_error(PairError::NotPaired(address.to_owned())))?;
    let SshPipe { pipe, said } = ssh_pipe(alias, port)?;
    connect_known_on(pipe, address, &pinned, identity, device)
        .map_err(|e| ssh_failed(alias, &said, e))
}

/// The store of this window's paired hosts.
fn window_list() -> Result<Store, RemoteError> {
    let dir = ferris_idea_core::user_config_dir()
        .ok_or_else(|| RemoteError::Failed("no settings folder for paired hosts".into()))?
        .join("remote");
    Store::open(
        Side::Window,
        &dir,
        ferris_idea_core::product_var_os("FERRIS_IDEA_KEY_FILE").is_some(),
    )
    .map_err(key_error)
}

/// The store and this window's key, which is created on first use.
fn window_store() -> Result<(Store, Identity), RemoteError> {
    let store = window_list()?;
    let identity = store.identity().map_err(key_error)?;
    Ok((store, identity))
}

fn key_error(e: KeyError) -> RemoteError {
    RemoteError::Failed(e.0)
}

fn pair_error(e: PairError) -> RemoteError {
    RemoteError::Failed(e.to_string())
}

/// Whether this window is paired with `address`.
pub fn is_paired(address: &str) -> Result<bool, RemoteError> {
    let store = window_list()?;
    Ok(store.host_key(address).map_err(key_error)?.is_some())
}

/// The paired hosts for `remote_hosts_paired` (never a key).
pub fn paired_hosts() -> Result<Vec<Value>, RemoteError> {
    let store = window_list()?;
    Ok(store
        .hosts()
        .map_err(key_error)?
        .iter()
        .map(|h| json!({"address": h.address, "fingerprint": h.fingerprint, "paired_on": h.paired_on}))
        .collect())
}

/// Removes the pin of `address`; `false` when it was not paired.
pub fn forget_paired(address: &str) -> Result<bool, RemoteError> {
    let store = window_list()?;
    store.forget_host(address).map_err(key_error)
}

/// Runs the pairing with `address` up to the six digits the user has to compare.
pub fn begin_pairing(address: &str, code: &str) -> Result<PendingPairing, RemoteError> {
    let (_, identity) = window_store()?;
    let device = default_device_name();
    match ssh_path(address) {
        Some((alias, port)) => {
            let SshPipe { pipe, said } = ssh_pipe(alias, port)?;
            start_pairing_on(pipe, address, code, None, &identity, &device)
                .map_err(|e| ssh_failed(alias, &said, e))
        }
        None => start_pairing(address, code, &identity, &device).map_err(pair_error),
    }
}

/// Runs the pairing from a QR link (`SPEC-0324` behaviour 3), up to the digits.
pub fn begin_pairing_link(link: &PairLink) -> Result<PendingPairing, RemoteError> {
    let (_, identity) = window_store()?;
    start_pairing_with_link(link, &identity, &default_device_name()).map_err(pair_error)
}

/// Copies the pin of a paired host to another path (`SPEC-0324` behaviour 7).
pub fn add_path(address: &str, path: &str) -> Result<bool, RemoteError> {
    // The path must read as a host string; a bad one is refused before it is stored.
    let spec = crate::HostSpec::parse(&format!("paired://{path}"))?;
    let path = spec.label();
    let path = path.trim_start_matches("paired://");
    window_list()?.add_path(address, path).map_err(key_error)
}

/// The accepting side's store (paired devices).
fn serve_store() -> Result<(Store, PathBuf), RemoteError> {
    let state = ferris_idea_core::user_config_dir()
        .ok_or_else(|| RemoteError::Failed("no settings folder for paired devices".into()))?;
    let store = Store::open(
        Side::Serve,
        &state.join("remote"),
        ferris_idea_core::product_var_os("FERRIS_IDEA_KEY_FILE").is_some(),
    )
    .map_err(key_error)?;
    Ok((store, state))
}

/// The paired devices for `device_list` (never a key).
pub fn devices() -> Result<Vec<Value>, RemoteError> {
    let (store, _) = serve_store()?;
    Ok(store
        .devices()
        .map_err(key_error)?
        .iter()
        .map(|d| {
            json!({
                "name": d.name, "fingerprint": d.fingerprint, "rights": d.rights,
                "paired_on": d.paired_on,
            })
        })
        .collect())
}

/// Narrows a device's rights; a widening is refused (`SPEC-0324` behaviour 16). `view` stays.
pub fn device_restrict(fingerprint: &str, rights: &[String]) -> Result<Vec<String>, RemoteError> {
    restrict(&serve_store()?.0, fingerprint, rights)
}

fn restrict(
    store: &Store,
    fingerprint: &str,
    rights: &[String],
) -> Result<Vec<String>, RemoteError> {
    let list = store.devices().map_err(key_error)?;
    let device = list
        .iter()
        .find(|d| d.fingerprint == fingerprint)
        .ok_or_else(|| RemoteError::Failed(format!("device {fingerprint} is not paired")))?;
    if let Some(extra) = rights.iter().find(|r| !device.rights.contains(r)) {
        return Err(RemoteError::Failed(format!(
            "`{extra}` is not among this device's rights; only the user can widen them with \
             `ferris-idea serve --rights`"
        )));
    }
    let mut kept: Vec<String> = vec!["view".to_owned()];
    kept.extend(rights.iter().filter(|r| *r != "view").cloned());
    store.set_rights(fingerprint, &kept).map_err(key_error)?;
    Ok(kept)
}

/// Revokes a device; a running listener closes its sessions within a second.
pub fn device_revoke(fingerprint: &str) -> Result<bool, RemoteError> {
    serve_store()?.0.revoke(fingerprint).map_err(key_error)
}

/// The newest audit lines, for one device or all (`SPEC-0324` behaviour 10).
pub fn device_log(fingerprint: Option<&str>, limit: usize) -> Result<Vec<Value>, RemoteError> {
    let (_, state) = serve_store()?;
    Ok(ferris_idea_pairing::audit::read_log(
        &state,
        fingerprint,
        limit,
    ))
}

/// Whether remote access is on and how many devices are paired.
pub fn access_status() -> Result<Value, RemoteError> {
    let (store, _) = serve_store()?;
    Ok(json!({
        "access_off": store.access_off().map_err(key_error)?,
        "devices": store.devices().map_err(key_error)?.len(),
    }))
}

/// Switches remote access off: a running listener closes within a second (`SPEC-0324` 11).
pub fn access_off() -> Result<(), RemoteError> {
    serve_store()?.0.set_access_off(true).map_err(key_error)
}

/// The user confirmed the digits: pins the host's key.
pub fn finish_pairing(pending: PendingPairing) -> Result<String, RemoteError> {
    let store = window_list()?;
    pending.confirm(&store).map_err(pair_error)
}

/// An open project on a paired host.
pub struct PairedProject {
    address: String,
    label: String,
    folder: String,
    store: Mutex<Store>,
    identity: Identity,
    /// The window half of the session.
    pub session: ClientSession,
}

impl PairedProject {
    /// Opens `folder` on the paired host at `address`.
    pub fn open(
        address: &str,
        folder: &str,
        recovery: Option<PathBuf>,
    ) -> Result<Self, RemoteError> {
        let (store, identity) = window_store()?;
        Self::open_with(address, folder, recovery, store, identity)
    }

    /// As [`Self::open`] with a given store (tests).
    pub fn open_with(
        address: &str,
        folder: &str,
        recovery: Option<PathBuf>,
        store: Store,
        identity: Identity,
    ) -> Result<Self, RemoteError> {
        let label = format!("paired://{address}");
        let transport = connect_path(address, &store, &identity, &default_device_name())?;
        let recovery = recovery.map(|dir| Recovery {
            dir,
            host: label.clone(),
            folder: folder.to_owned(),
        });
        let session = ClientSession::open(
            transport,
            folder,
            &format!("ferris-idea {}", env!("CARGO_PKG_VERSION")),
            &label,
            recovery,
        )
        .map_err(|e| RemoteError::Failed(format!("{label}: {}: {}", e.code.as_str(), e.message)))?;
        Ok(Self {
            address: address.to_owned(),
            label,
            folder: folder.to_owned(),
            store: Mutex::new(store),
            identity,
            session,
        })
    }

    /// Host label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Project folder.
    pub fn folder(&self) -> &str {
        &self.folder
    }

    /// Breaks the channel as a network failure would.
    pub fn break_connection(&mut self) {
        self.session.drop_connection();
        self.session.pump(Duration::from_millis(100));
    }

    /// Resumes through a new handshake; a backend that is gone gets a new session.
    pub fn reconnect(&mut self) -> Result<Reconnected, RemoteError> {
        let device = default_device_name();
        let transport = {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            connect_path(&self.address, &store, &self.identity, &device)?
        };
        match self.session.resume(transport) {
            Ok(r) => return Ok(r),
            Err(e)
                if matches!(
                    e.code,
                    ErrorCode::NotFound | ErrorCode::Denied | ErrorCode::Unavailable
                ) => {}
            Err(e) => {
                return Err(RemoteError::Failed(format!(
                    "{}: {}: {}",
                    self.label,
                    e.code.as_str(),
                    e.message
                )));
            }
        }
        let transport = {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            connect_path(&self.address, &store, &self.identity, &device)?
        };
        self.session
            .replace_backend(transport)
            .map_err(|e| RemoteError::Failed(format!("{}: {}", self.label, e.message)))
    }

    /// Closes the project.
    pub fn close(&mut self, unsaved: Unsaved) -> Result<Vec<String>, RemoteError> {
        let open: Vec<String> = self
            .session
            .docs()
            .filter(|d| d.modified || d.pending() > 0)
            .map(|d| d.doc.clone())
            .collect();
        match unsaved {
            Unsaved::Refuse if !open.is_empty() => {
                return Err(RemoteError::Failed(format!(
                    "{} unsaved: {}; nothing was closed",
                    open.len(),
                    open.join(", ")
                )));
            }
            Unsaved::Save => {
                for doc in &open {
                    self.session
                        .save(doc, false)
                        .map_err(|e| RemoteError::Failed(e.to_string()))?;
                }
            }
            _ => {}
        }
        let _ = self.session.bye();
        Ok(open)
    }

    /// The state for agents and the status bar (never the resume token).
    pub fn describe(&mut self) -> Value {
        self.session.pump(Duration::ZERO);
        json!({
            "host": self.label,
            "folder": self.folder,
            "transport": "paired",
            "state": if self.session.connected() { "connected" } else { "disconnected" },
            "backend": {
                "version": self.session.info.build,
                "platform": self.session.info.platform,
                "session": self.session.info.session,
            },
            "pending_edits": self.session.pending_edits(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ak9_restrict_only_narrows_and_keeps_view() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let store = Store::open_file(Side::Serve, &root.join("remote")).unwrap();
        let public = [5u8; 32];
        store
            .add_device(&public, "phone", &["view".into(), "edit".into()])
            .unwrap();
        let fp = ferris_idea_pairing::fingerprint(&public);
        let refused = restrict(&store, &fp, &["run".into()]).unwrap_err();
        assert!(refused.to_string().contains("only the user"), "{refused}");
        let kept = restrict(&store, &fp, &["edit".into()]).unwrap();
        assert_eq!(kept, ["view", "edit"]);
        let kept = restrict(&store, &fp, &[]).unwrap();
        assert_eq!(kept, ["view"]);
        assert_eq!(store.devices().unwrap()[0].rights, ["view"]);
        assert!(restrict(&store, "nope", &[]).is_err());
    }

    #[test]
    fn ak9_a_path_copies_the_pin() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let store = Store::open_file(Side::Window, &root.join("remote")).unwrap();
        store.add_host("h:1", &[3u8; 32]).unwrap();
        assert!(store.add_path("h:1", "h:1@ssh").unwrap());
        assert_eq!(store.host_key("h:1@ssh").unwrap(), Some([3u8; 32]));
        assert!(!store.add_path("nope:1", "x:1").unwrap());
    }

    #[test]
    fn ak4_ssh_path_strings_are_read() {
        assert_eq!(ssh_path("nas:47100@ssh"), Some(("nas", 47100)));
        assert_eq!(ssh_path("nas:47100"), None);
        assert_eq!(ssh_path("nas@ssh"), None);
    }

    #[test]
    fn ak4_ssh_path_builds_a_stdio_tunnel_through_ssh_command() {
        let Ok(line) = ssh_line("nas", 47100) else {
            // The client check needs a local `ssh` with post-quantum key exchange.
            return;
        };
        let argv = line.argv();
        let w = argv.iter().position(|a| a == "-W").unwrap();
        assert_eq!(argv[w + 1], "127.0.0.1:47100");
        assert_eq!(argv.last().unwrap(), "nas");
        assert!(!argv.iter().any(|a| a == "-L"), "no local port is opened");
    }
}
