//! A folder on an SSH host as a remote project (`SPEC-0317` v2, `ADR-0025` §1a, `ADR-0026` §1):
//! the backend `ferris-idea serve --stdio` of this very build runs on the host behind the system
//! `ssh` with SPEC-0121's options, checked by SHA-256 before each start of an uploaded copy; the
//! window half of the session protocol runs here. After a break, `serve --attach <session>`
//! bridges a new SSH channel to the waiting session; the resume token travels only inside it.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_session::{ClientSession, ErrorCode, Reconnected, Recovery, RpcError, Transport};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::backend::{BACKEND_DIR, Installed, ensure_backend_on};
use crate::bootstrap::{self, HostFamily, HostPlatform};
use crate::route::Route;
use crate::ssh::{Remote, RemoteError, quote};

/// How the backend is started on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOptions {
    /// Folder below the home folder for uploaded backends (normally [`BACKEND_DIR`]).
    pub backend_dir: String,
    /// `--state-dir` on the host (audit log), if not the default.
    pub state_dir: Option<String>,
    /// `--run-dir` on the host (reattach sockets), if not the default.
    pub run_dir: Option<String>,
    /// `--reattach-minutes` (1–60), if not the default 10.
    pub reattach_minutes: Option<u64>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            backend_dir: BACKEND_DIR.into(),
            state_dir: None,
            run_dir: None,
            reattach_minutes: None,
        }
    }
}

/// SHA-256 of a local file as hex.
pub fn sha256_file(path: &Path) -> Result<String, RemoteError> {
    use std::io::Read;
    let failed = |e: std::io::Error| RemoteError::Failed(format!("{}: {e}", path.display()));
    // In blocks: a backend build can be large (a debug build is gigabytes).
    let mut file = std::fs::File::open(path).map_err(failed)?;
    let mut hasher = Sha256::new();
    let mut block = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut block).map_err(failed)?;
        if n == 0 {
            break;
        }
        hasher.update(&block[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// How long the host may take to compute the SHA-256 of a backend of `size` bytes: the usual
/// limit of a call and one second for each started 16 MiB (`SPEC-0317` v7; a 2.5 GiB
/// debug build took 39 s on a busy host).
pub fn checksum_limit(size: u64) -> Duration {
    crate::ssh::TIMEOUT + Duration::from_secs(size.div_ceil(16 << 20))
}

/// The program part of a remote command line for a backend at `path` (`$HOME/…` expanded by
/// the host's shell, everything else quoted).
fn program(path: &str) -> String {
    match path.strip_prefix("$HOME/") {
        Some(rest) => format!("\"$HOME\"/{}", quote(rest)),
        None => quote(path),
    }
}

/// The remote command line of `serve --stdio` (the folder goes in `session.hello`, not here).
pub fn serve_line(path: &str, opts: &ServeOptions) -> String {
    let mut line = format!("exec {} serve --stdio", program(path));
    if let Some(d) = &opts.state_dir {
        line.push_str(&format!(" --state-dir {}", quote(d)));
    }
    if let Some(d) = &opts.run_dir {
        line.push_str(&format!(" --run-dir {}", quote(d)));
    }
    if let Some(m) = opts.reattach_minutes {
        line.push_str(&format!(" --reattach-minutes {m}"));
    }
    line
}

/// The remote command line of `serve --attach <session>`.
pub fn attach_line(path: &str, session: &str, opts: &ServeOptions) -> String {
    let mut line = format!("exec {} serve --attach {}", program(path), quote(session));
    if let Some(d) = &opts.run_dir {
        line.push_str(&format!(" --run-dir {}", quote(d)));
    }
    line
}

/// Compares the SHA-256 of the uploaded backend at `path` on the host with `local` (computed
/// here, never taken from the host). A mismatch deletes the copy and refuses.
/// `size` is the local build's size in bytes; it sets how long the host may take.
pub fn verify_backend(
    remote: &Remote,
    path: &str,
    local: &str,
    size: u64,
) -> Result<(), RemoteError> {
    verify_on(&Route::Ssh(remote.clone()), path, local, size)
}

fn verify_on(route: &Route, path: &str, local: &str, size: u64) -> Result<(), RemoteError> {
    let rel = path.strip_prefix("$HOME/").unwrap_or(path);
    let out = route.sh_within(
        "p=\"$1\"; case \"$p\" in /*) ;; *) p=\"$HOME/$p\";; esac; \
         { sha256sum \"$p\" 2>/dev/null || shasum -a 256 \"$p\"; } | cut -d' ' -f1",
        &[rel],
        None,
        checksum_limit(size),
    )?;
    let theirs = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if theirs == local {
        return Ok(());
    }
    let _ = route.sh(
        "p=\"$1\"; case \"$p\" in /*) ;; *) p=\"$HOME/$p\";; esac; rm -f -- \"$p\"",
        &[rel],
        None,
    );
    Err(RemoteError::Failed(format!(
        "{}: the backend on the host does not match this build (SHA-256 differs); the copy was \
         deleted and is uploaded again on the next open",
        route.label()
    )))
}

/// A running `ssh … serve` and its standard error lines (the connection log).
struct Channel {
    child: Arc<Mutex<Child>>,
}

impl Channel {
    fn start(
        route: &Route,
        line: &str,
        log: &Arc<Mutex<Vec<String>>>,
    ) -> Result<(Self, Transport), RemoteError> {
        let mut child = route
            .command()?
            .arg(line)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RemoteError::NoSsh(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| RemoteError::Failed("no input to the backend".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RemoteError::Failed("no output from the backend".into()))?;
        if let Some(err) = child.stderr.take() {
            let log = Arc::clone(log);
            std::thread::spawn(move || {
                for l in BufReader::new(err).lines().map_while(Result::ok) {
                    if let Ok(mut v) = log.lock() {
                        v.push(l);
                    }
                }
            });
        }
        let child = Arc::new(Mutex::new(child));
        let killer = Arc::clone(&child);
        let transport = Transport {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            closer: Arc::new(move || {
                if let Ok(mut c) = killer.lock() {
                    let _ = c.kill();
                }
            }),
        };
        Ok((Self { child }, transport))
    }

    fn pid(&self) -> Option<u32> {
        self.child.lock().ok().map(|c| c.id())
    }

    fn kill(&self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// The exit code once the process ended (within `limit`).
    fn exit_code(&self, limit: Duration) -> Option<i32> {
        let t0 = std::time::Instant::now();
        while t0.elapsed() < limit {
            if let Ok(mut c) = self.child.lock()
                && let Ok(Some(s)) = c.try_wait()
            {
                return s.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Brings `build` to the host; returns where it is and its SHA-256.
fn install(
    route: &Route,
    platform: &HostPlatform,
    build: &Path,
    version: &str,
    opts: &ServeOptions,
) -> Result<(Installed, String), RemoteError> {
    match platform.family {
        HostFamily::Windows => {
            let (path, sha) = bootstrap::install_windows(route, build, version)?;
            Ok((Installed::Uploaded(path), sha))
        }
        HostFamily::Posix => {
            let same = crate::same_platform(&platform.platform, &crate::local_platform());
            let installed =
                ensure_backend_on(route, &opts.backend_dir, version, build, platform, false)?;
            let sha = sha256_file(build)?;
            // A dedicated build must really start there (a glibc build on musl does not).
            if !same {
                check_runs(route, installed.path(), platform)?;
            }
            Ok((installed, sha))
        }
    }
}

fn check_runs(route: &Route, path: &str, platform: &HostPlatform) -> Result<(), RemoteError> {
    let out = route.sh(
        "p=\"$1\"; case \"$p\" in '$HOME/'*) p=\"$HOME/${p#'$HOME/'}\";; esac; \"$p\" --version",
        &[path],
        None,
    )?;
    if out.success() && String::from_utf8_lossy(&out.stdout).starts_with("ferris-idea ") {
        return Ok(());
    }
    Err(RemoteError::Failed(format!(
        "{}: the backend build for {} does not start there: {}. A musl-based system needs its own \
         build; start ferris-idea-serve there by hand with --pair",
        route.label(),
        platform.platform,
        out.stderr.trim()
    )))
}

/// The command line that starts the backend.
fn start_line(family: HostFamily, backend: &str, opts: &ServeOptions) -> String {
    match family {
        HostFamily::Posix => serve_line(backend, opts),
        HostFamily::Windows => bootstrap::windows_serve_line(backend, "serve --stdio"),
    }
}

/// What to do with unsaved files when closing (`remote_project_close`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsaved {
    /// Nothing happens while files are unsaved.
    Refuse,
    /// Save them first.
    Save,
    /// Close anyway.
    Discard,
}

/// An open remote project.
pub struct RemoteProject {
    /// Host as given.
    pub host: String,
    /// Project folder on the host.
    pub folder: String,
    route: Route,
    family: HostFamily,
    backend: String,
    opts: ServeOptions,
    local_sha: String,
    /// Size of that build in bytes, for the time limit of the check.
    local_size: u64,
    channel: Option<Channel>,
    /// How the backend was found or brought.
    pub installed: Installed,
    /// The window half of the session.
    pub session: ClientSession,
    log: Arc<Mutex<Vec<String>>>,
}

fn rpc(host: &str, e: RpcError) -> RemoteError {
    RemoteError::Failed(format!("{host}: {}: {}", e.code.as_str(), e.message))
}

impl RemoteProject {
    /// Opens `folder` on the host: finds or uploads the backend (same version and build),
    /// checks an uploaded copy by SHA-256, starts `serve --stdio` and opens the session.
    /// `recovery` is the window's folder for recovery copies.
    pub fn open(
        remote: Remote,
        folder: &str,
        local_binary: &Path,
        opts: ServeOptions,
        recovery: Option<PathBuf>,
    ) -> Result<Self, RemoteError> {
        Self::open_route(Route::Ssh(remote), folder, local_binary, opts, recovery)
    }

    /// Opens `folder` over any route (`SPEC-0321`): finds the host's platform, picks the build
    /// for it, brings it there, checks it by SHA-256, starts `serve --stdio`, opens the session.
    pub fn open_route(
        route: Route,
        folder: &str,
        local_binary: &Path,
        opts: ServeOptions,
        recovery: Option<PathBuf>,
    ) -> Result<Self, RemoteError> {
        let host = route.label();
        let version = env!("CARGO_PKG_VERSION");
        let platform = bootstrap::detect(&route)?;
        let build = bootstrap::select_build(&platform, local_binary)?;
        let (installed, local_sha) = install(&route, &platform, &build, version, &opts)?;
        let local_size = std::fs::metadata(&build).map(|m| m.len()).unwrap_or(0);
        let backend = installed.path().to_owned();
        if platform.family == HostFamily::Posix && backend.contains(&opts.backend_dir) {
            verify_on(&route, &backend, &local_sha, local_size)?;
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        if platform.family == HostFamily::Windows
            && let Ok(mut l) = log.lock()
        {
            l.push(
                "this host has no reattach socket; a broken channel starts a new backend and \
                 reconciles the documents"
                    .to_owned(),
            );
        }
        let (channel, transport) =
            Channel::start(&route, &start_line(platform.family, &backend, &opts), &log)?;
        let recovery = recovery.map(|dir| Recovery {
            dir,
            host: host.clone(),
            folder: folder.to_owned(),
        });
        let session = ClientSession::open(
            transport,
            folder,
            &format!("ferris-idea {version}"),
            &host,
            recovery,
        )
        .map_err(|e| {
            // The backend's own refusal names the cause; ssh's refusals are on standard error.
            let stderr = log.lock().map(|l| l.join("\n")).unwrap_or_default();
            let lower = stderr.to_lowercase();
            let named = lower.contains("host key") || lower.contains("permission denied");
            if named || (!matches!(route, Route::Ssh(_)) && !stderr.trim().is_empty()) {
                route.refusal(&stderr)
            } else {
                rpc(&host, e)
            }
        })?;
        Ok(Self {
            host,
            folder: folder.to_owned(),
            route,
            family: platform.family,
            backend,
            opts,
            local_sha,
            local_size,
            channel: Some(channel),
            installed,
            session,
            log,
        })
    }

    /// The local `ssh` process of the current channel.
    pub fn ssh_pid(&self) -> Option<u32> {
        self.channel.as_ref().and_then(Channel::pid)
    }

    /// Breaks the channel as a network failure would (the `ssh` process is killed).
    pub fn break_connection(&mut self) {
        if let Some(c) = self.channel.take() {
            c.kill();
        }
        // The session's reader notices the end of the ssh process on its own thread; wait (at
        // most five seconds) until it has, so the session says it is disconnected once this
        // returns. A fixed 100 ms was too short under load.
        let t0 = std::time::Instant::now();
        loop {
            self.session.pump(Duration::from_millis(20));
            if !self.session.connected() || t0.elapsed() >= Duration::from_secs(5) {
                break;
            }
        }
    }

    /// The connection log: `ssh` and backend lines, then the session's own.
    pub fn connection_log(&self) -> Vec<String> {
        let mut out = self.log.lock().map(|l| l.clone()).unwrap_or_default();
        out.extend(self.session.log.iter().cloned());
        out
    }

    /// Resumes the session through `serve --attach`; when the backend is gone (reattach period
    /// over, host restarted) starts a new one and reconciles the documents. A host that cannot
    /// be reached is an error and the project stays disconnected.
    pub fn reconnect(&mut self) -> Result<Reconnected, RemoteError> {
        if let Some(c) = self.channel.take() {
            c.kill();
        }
        let sid = self.session.info.session.clone();
        // A Windows host has no reattach socket (SPEC-0321 behaviour 8).
        if self.family == HostFamily::Posix {
            let (channel, transport) = Channel::start(
                &self.route,
                &attach_line(&self.backend, &sid, &self.opts),
                &self.log,
            )?;
            match self.session.resume(transport) {
                Ok(r) => {
                    self.channel = Some(channel);
                    return Ok(r);
                }
                Err(e) => {
                    let code = channel.exit_code(Duration::from_secs(5));
                    if code == Some(255) && matches!(self.route, Route::Ssh(_)) {
                        let stderr = self.log.lock().map(|l| l.join("\n")).unwrap_or_default();
                        return Err(self.route.refusal(&stderr));
                    }
                    if !matches!(
                        e.code,
                        ErrorCode::NotFound | ErrorCode::Denied | ErrorCode::Unavailable
                    ) {
                        return Err(rpc(&self.host, e));
                    }
                }
            }
        }
        // The backend is gone: a new one, checked again, and every document reconciled.
        match self.family {
            HostFamily::Posix if self.backend.contains(&self.opts.backend_dir) => {
                verify_on(&self.route, &self.backend, &self.local_sha, self.local_size)?;
            }
            HostFamily::Windows => {
                bootstrap::verify_windows(&self.route, &self.backend, &self.local_sha)?;
            }
            HostFamily::Posix => {}
        }
        let (channel, transport) = Channel::start(
            &self.route,
            &start_line(self.family, &self.backend, &self.opts),
            &self.log,
        )?;
        let r = self
            .session
            .replace_backend(transport)
            .map_err(|e| rpc(&self.host, e))?;
        self.channel = Some(channel);
        Ok(r)
    }

    /// Unsaved files and pending edits (the dry run of closing).
    pub fn unsaved(&self) -> Vec<String> {
        self.session
            .docs()
            .filter(|d| d.modified || d.pending() > 0)
            .map(|d| d.doc.clone())
            .collect()
    }

    /// Closes the project (`session.bye`): refuses while files are unsaved unless told to save
    /// or discard them. Returns the files that were unsaved.
    pub fn close(&mut self, unsaved: Unsaved) -> Result<Vec<String>, RemoteError> {
        let open = self.unsaved();
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
        if let Some(c) = self.channel.take() {
            let _ = c.exit_code(Duration::from_secs(5));
        }
        Ok(open)
    }

    /// The state for `remote_projects` and the status bar: host, folder, connection, round trip,
    /// backend, pending edits and unsaved documents. Never the resume token.
    pub fn describe(&mut self) -> Value {
        self.session.pump(Duration::ZERO);
        let rtt = if self.session.connected() {
            self.session.ping().ok()
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
                "installed": match &self.installed {
                    Installed::Present(_) => "present",
                    Installed::Uploaded(_) => "uploaded",
                },
                "sha256": self.local_sha,
                "session": self.session.info.session,
            },
            "pending_edits": self.session.pending_edits(),
            "unsaved": self.unsaved(),
        })
    }

    /// The open documents for `remote_documents`.
    pub fn documents(&mut self) -> Value {
        self.session.pump(Duration::ZERO);
        json!({ "host": self.host, "folder": self.folder, "documents": self.session.describe_docs() })
    }
}

#[cfg(test)]
mod tests {
    /// SPEC-0317 v7 AK-10: the check's limit grows with the build; the local hash
    /// reads in blocks and matches a hash of the whole.
    #[test]
    fn checksum_limit_and_streamed_hash() {
        use super::*;
        assert_eq!(checksum_limit(0), crate::ssh::TIMEOUT);
        assert_eq!(
            checksum_limit(1),
            crate::ssh::TIMEOUT + Duration::from_secs(1)
        );
        assert_eq!(
            checksum_limit(2_694_000_000),
            crate::ssh::TIMEOUT + Duration::from_secs(161)
        );
        let dir = std::env::temp_dir().join(format!("ferris-idea-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("b");
        let bytes: Vec<u8> = (0..(3u32 << 20) + 17).map(|i| (i % 251) as u8).collect();
        std::fs::write(&file, &bytes).unwrap();
        let whole: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(sha256_file(&file).unwrap(), whole);
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn command_lines_carry_no_secret_and_quote_paths() {
        let opts = ServeOptions {
            state_dir: Some("/tmp/my state".into()),
            run_dir: Some("/tmp/r".into()),
            reattach_minutes: Some(2),
            ..ServeOptions::default()
        };
        assert_eq!(
            serve_line("$HOME/.cache/ferris-idea/bin/ferris-idea-1", &opts),
            "exec \"$HOME\"/.cache/ferris-idea/bin/ferris-idea-1 serve --stdio --state-dir '/tmp/my state' --run-dir /tmp/r --reattach-minutes 2"
        );
        assert_eq!(
            attach_line("/usr/bin/ferris-idea", "ab12", &opts),
            "exec /usr/bin/ferris-idea serve --attach ab12 --run-dir /tmp/r"
        );
        let me = std::env::current_exe().unwrap();
        assert_eq!(sha256_file(&me).unwrap().len(), 64);
    }
}
