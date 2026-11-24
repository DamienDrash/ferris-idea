//! A remote project whose backend runs in this process (`SPEC-0317` v3 AK-7, AK-8): the same
//! session protocol over in-process pipes instead of SSH, for the window's self-test and the
//! tests of reconnection and typing latency. A break closes the pipes as a lost network would; a
//! "host restart" replaces the backend, so the session cannot be resumed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ferris_idea_session::{
    Backend, BackendConfig, ClientSession, ErrorCode, Peer, Reconnected, Recovery, RpcError,
    Transport, in_process,
};
use serde_json::{Value, json};

use crate::live::ProjectLink;
use crate::project::Unsaved;
use crate::ssh::RemoteError;

/// Binds further functions into a backend (the host functions of `SPEC-0318`, which live in a
/// crate above this one).
pub type Setup = Arc<dyn Fn(&Backend) + Send + Sync>;

/// A project folder served by a backend in this process.
pub struct InProcessProject {
    host: String,
    folder: String,
    config: BackendConfig,
    setup: Option<Setup>,
    backend: Backend,
    delay: Duration,
    /// The window half of the session.
    pub session: ClientSession,
    switch: HostSwitch,
    log: Vec<String>,
    /// Who the session is on the backend, when not the owner (`SPEC-0323` v7 window tests).
    peer: Option<Peer>,
}

/// What a test does to the "host" of an [`InProcessProject`] after the window took it over.
#[derive(Clone, Default)]
pub struct HostSwitch {
    unreachable: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
}

impl HostSwitch {
    /// Reconnecting fails while the host is unreachable.
    pub fn set_reachable(&self, on: bool) {
        self.unreachable.store(!on, Ordering::SeqCst);
    }

    /// The host restarts: the next reconnection finds a new backend that does not know the
    /// session (`SPEC-0317` behaviour 15).
    pub fn restart(&self) {
        self.restart.store(true, Ordering::SeqCst);
    }
}

fn rpc(host: &str, e: RpcError) -> RemoteError {
    RemoteError::Failed(format!("{host}: {}: {}", e.code.as_str(), e.message))
}

impl InProcessProject {
    /// Opens `folder` under the label `host`; the backend keeps its audit log in `state_dir`,
    /// broken sessions wait `reattach`, the window keeps recovery copies in `recovery`, and every
    /// answer arrives `delay` later (a slow network).
    pub fn open(
        host: &str,
        folder: &Path,
        state_dir: &Path,
        reattach: Duration,
        recovery: Option<PathBuf>,
        delay: Duration,
    ) -> Result<Self, RemoteError> {
        Self::open_with(host, folder, state_dir, reattach, recovery, delay, None)
    }

    /// [`InProcessProject::open`] with `setup` run on each backend (also after a "host
    /// restart"), e.g. to bind the host functions of `SPEC-0318`.
    pub fn open_with(
        host: &str,
        folder: &Path,
        state_dir: &Path,
        reattach: Duration,
        recovery: Option<PathBuf>,
        delay: Duration,
        setup: Option<Setup>,
    ) -> Result<Self, RemoteError> {
        Self::open_inner(
            host, folder, state_dir, reattach, recovery, delay, setup, false,
        )
    }

    /// [`InProcessProject::open_with`] with the backend bound to `folder`, as
    /// `ferris-idea serve --pair <folder>` is, so that other people may connect to it as paired
    /// devices or SSH users through [`Backend::serve_as`] (`SPEC-0323` v6 window tests).
    pub fn open_fixed_with(
        host: &str,
        folder: &Path,
        state_dir: &Path,
        reattach: Duration,
        recovery: Option<PathBuf>,
        delay: Duration,
        setup: Option<Setup>,
    ) -> Result<Self, RemoteError> {
        Self::open_inner(
            host, folder, state_dir, reattach, recovery, delay, setup, true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn open_inner(
        host: &str,
        folder: &Path,
        state_dir: &Path,
        reattach: Duration,
        recovery: Option<PathBuf>,
        delay: Duration,
        setup: Option<Setup>,
        fixed: bool,
    ) -> Result<Self, RemoteError> {
        let config = BackendConfig {
            folder: fixed.then(|| folder.to_path_buf()),
            state_dir: Some(state_dir.to_path_buf()),
            peer: format!("test:{host}"),
            reattach,
            ..BackendConfig::default()
        };
        let backend = Backend::new(config.clone());
        if let Some(s) = &setup {
            s(&backend);
        }
        let folder_text = folder.display().to_string();
        let recovery = recovery.map(|dir| Recovery {
            dir,
            host: host.to_owned(),
            folder: folder_text.clone(),
        });
        let transport = Self::channel(&backend, delay, None);
        let session = ClientSession::open(
            transport,
            &folder_text,
            &format!("ferris-idea {}", env!("CARGO_PKG_VERSION")),
            host,
            recovery,
        )
        .map_err(|e| rpc(host, e))?;
        Ok(Self {
            host: host.to_owned(),
            folder: folder_text,
            config,
            setup,
            backend,
            delay,
            session,
            switch: HostSwitch::default(),
            log: Vec::new(),
            peer: None,
        })
    }

    /// A second person's project on the running `backend` of another [`InProcessProject`]: the
    /// session is `peer` there, e.g. another SSH user joining the room by link (`SPEC-0323` v7
    /// window tests). `folder` is the backend's bound folder.
    pub fn open_on(
        backend: &Backend,
        host: &str,
        folder: &Path,
        peer: Peer,
    ) -> Result<Self, RemoteError> {
        let folder_text = folder.display().to_string();
        let transport = Self::channel(backend, Duration::ZERO, Some(&peer));
        let session = ClientSession::open(
            transport,
            &folder_text,
            &format!("ferris-idea {}", env!("CARGO_PKG_VERSION")),
            host,
            None,
        )
        .map_err(|e| rpc(host, e))?;
        Ok(Self {
            host: host.to_owned(),
            folder: folder_text,
            config: BackendConfig::default(),
            setup: None,
            backend: backend.clone(),
            delay: Duration::ZERO,
            session,
            switch: HostSwitch::default(),
            log: Vec::new(),
            peer: Some(peer),
        })
    }

    fn channel(backend: &Backend, delay: Duration, peer: Option<&Peer>) -> Transport {
        let (window, server) = in_process();
        let b = backend.clone();
        match peer.cloned() {
            Some(p) => std::thread::spawn(move || b.serve_as(server, p)),
            None => std::thread::spawn(move || b.serve(server)),
        };
        ferris_idea_session::testing::delayed(window, delay)
    }

    /// The switch of this project's "host".
    pub fn switch(&self) -> HostSwitch {
        self.switch.clone()
    }

    /// The backend serving the session now (a second client or an agent connects to it).
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// A transport to the current backend (a second client).
    pub fn connect(&self) -> Transport {
        Self::channel(&self.backend, Duration::ZERO, None)
    }
}

impl ProjectLink for InProcessProject {
    fn session(&mut self) -> &mut ClientSession {
        &mut self.session
    }

    fn session_ref(&self) -> &ClientSession {
        &self.session
    }

    fn reconnect(&mut self) -> Result<Reconnected, RemoteError> {
        if self.switch.unreachable.load(Ordering::SeqCst) {
            return Err(RemoteError::Unreachable(
                self.host.clone(),
                "the test host is switched off".into(),
            ));
        }
        if self.switch.restart.swap(false, Ordering::SeqCst) {
            self.backend = Backend::new(self.config.clone());
            if let Some(s) = &self.setup {
                s(&self.backend);
            }
            self.log.push("the host was restarted".into());
        }
        let t = Self::channel(&self.backend, self.delay, self.peer.as_ref());
        match self.session.resume(t) {
            Ok(r) => Ok(r),
            Err(e) if matches!(e.code, ErrorCode::NotFound | ErrorCode::Denied) => {
                let t = Self::channel(&self.backend, self.delay, self.peer.as_ref());
                self.session
                    .replace_backend(t)
                    .map_err(|e| rpc(&self.host, e))
            }
            Err(e) => Err(rpc(&self.host, e)),
        }
    }

    fn break_connection(&mut self) {
        self.session.drop_connection();
    }

    fn close(&mut self, unsaved: Unsaved) -> Result<Vec<String>, RemoteError> {
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
                        .map_err(|e| rpc(&self.host, e))?;
                }
            }
            _ => {}
        }
        let _ = self.session.bye();
        Ok(open)
    }

    fn describe(&mut self) -> Value {
        self.session.pump(Duration::ZERO);
        let rtt = if self.session.connected() {
            self.session.ping_within(Duration::from_secs(5)).ok()
        } else {
            None
        };
        json!({
            "host": self.host,
            "folder": self.folder,
            "state": if self.session.connected() { "connected" } else { "disconnected" },
            "round_trip_ms": rtt.map(|d| d.as_millis() as u64),
            "backend": {
                "version": self.session.info.build,
                "platform": self.session.info.platform,
                "installed": "in process",
                "session": self.session.info.session,
            },
            "pending_edits": self.session.pending_edits(),
            "unsaved": self.session.docs().filter(|d| d.modified || d.pending() > 0)
                .map(|d| d.doc.clone()).collect::<Vec<_>>(),
        })
    }

    fn connection_log(&self) -> Vec<String> {
        let mut out = self.log.clone();
        out.extend(self.session.log.iter().cloned());
        out
    }

    fn host(&self) -> String {
        self.host.clone()
    }

    fn folder(&self) -> String {
        self.folder.clone()
    }
}
