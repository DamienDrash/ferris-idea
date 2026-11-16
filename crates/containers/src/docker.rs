//! Docker through the Engine API (`SPEC-0119` behaviours 1–3): HTTP/1.1 over the daemon's Unix
//! socket or a TCP address, one connection per request.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

/// Time allowed to connect and for each read (stopping waits up to 10 s inside the daemon).
const TIMEOUT: Duration = Duration::from_secs(30);

/// Where the Docker daemon listens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "address", rename_all = "lowercase")]
pub enum DockerHost {
    /// A Unix socket.
    Unix(PathBuf),
    /// `host:port` without TLS.
    Tcp(String),
    /// Another machine over the system `ssh`, which runs `docker system dial-stdio` there
    /// (`SPEC-0243` behaviour 3).
    Ssh {
        /// `[user@]host`.
        destination: String,
        /// SSH port, when not the default.
        port: Option<u16>,
        /// Extra `ssh` arguments placed after IDECraft's own options, so they cannot weaken them
        /// (tests: a private configuration).
        #[serde(skip)]
        extra: Vec<String>,
        /// The `ssh` program; `None` for the one on `PATH` (tests: a stand-in).
        #[serde(skip)]
        program: Option<PathBuf>,
    },
}

impl std::fmt::Display for DockerHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DockerHost::Unix(p) => write!(f, "unix://{}", p.display()),
            DockerHost::Tcp(a) => write!(f, "tcp://{a}"),
            DockerHost::Ssh {
                destination, port, ..
            } => match port {
                Some(p) => write!(f, "ssh://{destination}:{p}"),
                None => write!(f, "ssh://{destination}"),
            },
        }
    }
}

/// Why a request failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DockerError {
    /// No daemon could be reached.
    #[error("cannot reach Docker at {0}: {1}")]
    Connect(String, String),
    /// The daemon refused (status and its message).
    #[error("Docker answered {0}: {1}")]
    Http(u16, String),
    /// The answer could not be read.
    #[error("unexpected answer from Docker: {0}")]
    Parse(String),
}

/// Version information of the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DockerInfo {
    /// Engine version.
    pub version: String,
    /// API version.
    pub api_version: String,
    /// Operating system and architecture of the daemon.
    pub platform: String,
}

/// A container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Container {
    /// Short id (12 characters).
    pub id: String,
    /// Name without the leading `/`.
    pub name: String,
    /// Image it runs.
    pub image: String,
    /// `running`, `exited`, `created`, `paused`, …
    pub state: String,
    /// Human status, e.g. `Up 2 hours`.
    pub status: String,
    /// Published ports, e.g. `0.0.0.0:8080->80/tcp`.
    pub ports: Vec<String>,
    /// Compose project, if the container belongs to one.
    pub compose_project: Option<String>,
    /// Compose service, if the container belongs to one (`SPEC-0242`).
    pub compose_service: Option<String>,
}

/// An image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Image {
    /// Short id (12 characters, without `sha256:`).
    pub id: String,
    /// Repository tags; empty for dangling images.
    pub tags: Vec<String>,
    /// Size in bytes.
    pub size: u64,
}

/// What can be done to a container (behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerAction {
    /// Start a stopped container.
    Start,
    /// Stop a running container (10 s grace period).
    Stop,
    /// Restart it.
    Restart,
    /// Remove it (also when running).
    Remove,
}

impl ContainerAction {
    /// Past tense for reports.
    pub fn done(self) -> &'static str {
        match self {
            ContainerAction::Start => "started",
            ContainerAction::Stop => "stopped",
            ContainerAction::Restart => "restarted",
            ContainerAction::Remove => "removed",
        }
    }
}

/// The daemon to use (behaviour 1): `DOCKER_HOST` (`unix://` or `tcp://`) when set, else the
/// first socket that exists among Docker Desktop's, the system's, Colima's, OrbStack's, Rancher
/// Desktop's and Podman's.
pub fn find_host(docker_host: Option<&str>, home: Option<&Path>) -> Option<DockerHost> {
    if let Some(h) = docker_host.map(str::trim).filter(|h| !h.is_empty()) {
        return DockerHost::parse(h, Vec::new());
    }
    crate::engines::local_sockets(home)
        .into_iter()
        .map(|(_, path)| path)
        .find(|p| p.exists())
        .map(DockerHost::Unix)
}

/// A connection to the daemon that can be split for a session that reads and writes at once.
pub(crate) trait Conn: Read + Write + Send {
    /// The reading and the writing half.
    fn split(self: Box<Self>) -> (Box<dyn Read + Send>, Box<dyn Write + Send>);
    /// No read timeout: for exec output and interactive shells, which may be quiet for long.
    fn wait_forever(&self) {}
}

#[cfg(unix)]
impl Conn for std::os::unix::net::UnixStream {
    fn wait_forever(&self) {
        let _ = self.set_read_timeout(None);
    }
    fn split(self: Box<Self>) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
        match self.try_clone() {
            Ok(w) => (self, Box::new(w)),
            Err(_) => (self, Box::new(std::io::sink())),
        }
    }
}

impl Conn for std::net::TcpStream {
    fn wait_forever(&self) {
        let _ = self.set_read_timeout(None);
    }
    fn split(self: Box<Self>) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
        match self.try_clone() {
            Ok(w) => (self, Box::new(w)),
            Err(_) => (self, Box::new(std::io::sink())),
        }
    }
}

/// `ssh … docker system dial-stdio`: the child's stdout is read, its stdin written; its stderr is
/// kept to name `ssh`'s own refusal (for example a host without a post-quantum key exchange).
struct SshConn {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: std::process::ChildStdout,
    host: String,
    stderr: std::sync::Arc<std::sync::Mutex<String>>,
    got: bool,
    /// The command line, for the messages of a refused jump host.
    line: idecraft_remote::SshLine,
}

impl SshConn {
    /// `ssh`'s refusal when it ended with 255 before sending anything.
    fn refusal(&mut self) -> Option<String> {
        let start = std::time::Instant::now();
        let code = loop {
            match self.child.try_wait() {
                Ok(Some(s)) => break s.code(),
                Ok(None) if start.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return None,
            }
        };
        if code != Some(255) {
            return None;
        }
        // The reader thread may still be copying the last lines.
        let text = loop {
            let text = self.stderr.lock().map(|t| t.clone()).unwrap_or_default();
            if !text.is_empty() || start.elapsed() > Duration::from_secs(3) {
                break text;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Some(idecraft_remote::classify(&self.host, &self.line.messages(&text)).to_string())
    }
}

impl Read for SshConn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.stdout.read(buf)?;
        if n == 0
            && !self.got
            && !buf.is_empty()
            && let Some(why) = self.refusal()
        {
            return Err(std::io::Error::other(why));
        }
        self.got |= n > 0;
        Ok(n)
    }
}

impl Write for SshConn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.stdin {
            Some(i) => i.write(buf),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.stdin {
            Some(i) => i.flush(),
            None => Ok(()),
        }
    }
}

impl Drop for SshConn {
    fn drop(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The reading half of a split SSH connection; it ends the child when dropped.
struct SshReader(SshConn);

impl Read for SshReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Conn for SshConn {
    fn split(mut self: Box<Self>) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
        let stdin = self.stdin.take();
        let writer: Box<dyn Write + Send> = match stdin {
            Some(i) => Box::new(i),
            None => Box::new(std::io::sink()),
        };
        // `self` is a Box; move the connection out so the reader owns the child.
        (Box::new(SshReader(*self)), writer)
    }
}

/// Folder for shared SSH connections, private to the user; short because Unix socket paths are
/// limited to about 100 bytes.
fn control_dir() -> Option<PathBuf> {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    let dir = PathBuf::from(format!("/tmp/idecraft-docker-ssh-{user}"));
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(&dir).ok()?;
        if !meta.is_dir() {
            return None;
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    Some(dir)
}

/// What the Engine API answered to an exec without a TTY (`SPEC-0240` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecOutput {
    /// Stdout and stderr in the order they were written.
    pub output: String,
    /// Exit code of the command (`None` when it was cut off by the time limit).
    pub exit_code: Option<i64>,
    /// Whether output beyond the limit was dropped.
    pub truncated: bool,
}

/// Options of an exec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecOptions {
    /// Working folder inside the container.
    pub workdir: Option<String>,
    /// `NAME=value` entries.
    pub env: Vec<String>,
    /// User (name or uid, optionally `:group`).
    pub user: Option<String>,
}

/// Command line that starts `bash` when the image has it, else `sh` (`SPEC-0240` behaviour 1).
pub const SHELL_CHOOSER: &str =
    "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi";

/// Most output kept from an exec without TTY.
pub const EXEC_LIMIT: usize = 1 << 20;

/// An interactive exec: the raw stream of the TTY and the exec id for resizing.
pub struct ExecSession {
    /// Exec id.
    pub id: String,
    pub(crate) conn: Box<dyn Conn>,
}

impl ExecSession {
    /// Reading and writing halves of the shell's terminal.
    pub fn split(self) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
        self.conn.split()
    }
}

/// `id` is a container id or name the API accepts in a path.
fn checked_id(id: &str) -> Result<&str, DockerError> {
    if !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
    {
        Ok(id)
    } else {
        Err(DockerError::Parse(format!(
            "not a container id or name: {id:?}"
        )))
    }
}

impl DockerHost {
    /// An engine address: `unix://path`, `tcp://host:port` or `ssh://[user@]host[:port]`; `extra`
    /// are additional `ssh` arguments.
    pub fn parse(address: &str, extra: Vec<String>) -> Option<Self> {
        let a = address.trim();
        if let Some(path) = a.strip_prefix("unix://") {
            return Some(DockerHost::Unix(PathBuf::from(path)));
        }
        if let Some(addr) = a.strip_prefix("tcp://") {
            return Some(DockerHost::Tcp(addr.trim_end_matches('/').to_owned()));
        }
        let rest = a.strip_prefix("ssh://")?.trim_end_matches('/');
        let (user, host) = match rest.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, rest),
        };
        let (host, port) = match host.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h, Some(p.parse::<u16>().ok()?)),
            _ => (host, None),
        };
        let valid = |t: &str| {
            !t.is_empty()
                && !t.starts_with('-')
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
        };
        if !valid(host) || !user.is_none_or(valid) {
            return None;
        }
        Some(DockerHost::Ssh {
            destination: match user {
                Some(u) => format!("{u}@{host}"),
                None => host.to_owned(),
            },
            port,
            extra,
            program: None,
        })
    }

    /// Warning for an engine reached without encryption and authentication (`SPEC-0243`
    /// behaviour 4).
    pub fn warning(&self) -> Option<&'static str> {
        match self {
            DockerHost::Tcp(addr) => {
                let host = addr.rsplit_once(':').map_or(addr.as_str(), |(h, _)| h);
                let host = host.trim_start_matches('[').trim_end_matches(']');
                let local = host == "localhost"
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback());
                (!local).then_some("unencrypted and unauthenticated TCP connection")
            }
            _ => None,
        }
    }

    /// The `ssh` program of an SSH engine.
    fn ssh_program(&self) -> String {
        match self {
            DockerHost::Ssh {
                program: Some(p), ..
            } => p.display().to_string(),
            _ => idecraft_remote::SSH_PROGRAM.to_owned(),
        }
    }

    /// The `ssh` command line of an SSH engine (`SPEC-0243` behaviour 3), from the one start
    /// point `idecraft_remote::SshCommand`: the client check, the shared options (batch mode,
    /// strict host keys, post-quantum key exchange) and the route over jump hosts first, then the
    /// shared connection and the engine's extra arguments. `None` for other engines.
    pub fn ssh_line(
        &self,
    ) -> Option<Result<idecraft_remote::SshLine, idecraft_remote::RemoteError>> {
        let DockerHost::Ssh {
            destination,
            port,
            extra,
            ..
        } = self
        else {
            return None;
        };
        let mut cmd = idecraft_remote::SshCommand::new(idecraft_remote::SshPurpose::Docker)
            .program(self.ssh_program());
        if let Some(dir) = control_dir() {
            cmd = cmd
                .option("ControlMaster=auto")
                .option(format!("ControlPath={}/%C", dir.display()))
                .option("ControlPersist=60");
        }
        Some(
            cmd.port(*port)
                .extra(extra)
                .remote(["docker", "system", "dial-stdio"])
                .build(destination),
        )
    }

    /// Opens one connection to the daemon.
    pub(crate) fn connect(&self) -> Result<Box<dyn Conn>, DockerError> {
        let connect = |e: std::io::Error| DockerError::Connect(self.to_string(), e.to_string());
        match self {
            DockerHost::Unix(socket) => {
                #[cfg(unix)]
                {
                    let s = std::os::unix::net::UnixStream::connect(socket).map_err(connect)?;
                    s.set_read_timeout(Some(TIMEOUT)).map_err(connect)?;
                    Ok(Box::new(s))
                }
                #[cfg(not(unix))]
                {
                    let _ = socket;
                    Err(DockerError::Connect(
                        self.to_string(),
                        "Unix sockets need macOS or Linux".into(),
                    ))
                }
            }
            DockerHost::Tcp(addr) => {
                use std::net::ToSocketAddrs;
                let target = addr
                    .to_socket_addrs()
                    .map_err(connect)?
                    .next()
                    .ok_or_else(|| DockerError::Connect(self.to_string(), "no address".into()))?;
                let s = std::net::TcpStream::connect_timeout(&target, TIMEOUT).map_err(connect)?;
                s.set_read_timeout(Some(TIMEOUT)).map_err(connect)?;
                Ok(Box::new(s))
            }
            DockerHost::Ssh { destination, .. } => {
                let line = self
                    .ssh_line()
                    .ok_or_else(|| DockerError::Connect(self.to_string(), "not ssh".into()))?
                    .map_err(|e| DockerError::Connect(self.to_string(), e.to_string()))?;
                let mut child = line
                    .command()
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(connect)?;
                let stdin = child.stdin.take();
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| DockerError::Connect(self.to_string(), "no output".into()))?;
                let stderr = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                if let Some(mut err) = child.stderr.take() {
                    let sink = std::sync::Arc::clone(&stderr);
                    std::thread::spawn(move || {
                        let mut chunk = [0u8; 1024];
                        while let Ok(n) = err.read(&mut chunk) {
                            if n == 0 {
                                break;
                            }
                            if let Ok(mut s) = sink.lock() {
                                // Only the first lines matter; never grow without bound.
                                if s.len() < 16 * 1024 {
                                    s.push_str(&String::from_utf8_lossy(&chunk[..n]));
                                }
                            }
                        }
                    });
                }
                Ok(Box::new(SshConn {
                    child,
                    stdin,
                    stdout,
                    host: destination.clone(),
                    stderr,
                    got: false,
                    line,
                }))
            }
        }
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        upgrade: bool,
    ) -> Result<Box<dyn Conn>, DockerError> {
        let body = body.map(Value::to_string).unwrap_or_default();
        let kind = if body.is_empty() {
            String::new()
        } else {
            "Content-Type: application/json\r\n".to_owned()
        };
        let connection = if upgrade {
            "Connection: Upgrade\r\nUpgrade: tcp\r\n"
        } else {
            "Connection: close\r\n"
        };
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: docker\r\nUser-Agent: IDECraft\r\n{kind}Content-Length: {}\r\n{connection}\r\n{body}",
            body.len()
        );
        let mut conn = self.connect()?;
        conn.write_all(request.as_bytes())
            .map_err(|e| DockerError::Connect(self.to_string(), e.to_string()))?;
        Ok(conn)
    }

    /// Sends one request and returns the status and body.
    fn request_with(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Vec<u8>), DockerError> {
        let mut conn = self.send(method, path, body, false)?;
        let mut raw = Vec::new();
        conn.read_to_end(&mut raw)
            .map_err(|e| DockerError::Connect(self.to_string(), e.to_string()))?;
        parse_response(&raw)
    }

    fn request(&self, method: &str, path: &str) -> Result<(u16, Vec<u8>), DockerError> {
        self.request_with(method, path, None)
    }

    /// Sends a JSON body; a status of 300 or more is an error with the daemon's message.
    fn call_json(&self, method: &str, path: &str, body: &Value) -> Result<Value, DockerError> {
        let (status, body) = self.request_with(method, path, Some(body))?;
        if status >= 300 {
            return Err(DockerError::Http(status, message(&body)));
        }
        serde_json::from_slice(&body).map_err(|e| DockerError::Parse(e.to_string()))
    }

    /// Creates an exec in container `id` (`SPEC-0240`).
    fn exec_create(
        &self,
        id: &str,
        command: &[String],
        tty: bool,
        options: &ExecOptions,
    ) -> Result<String, DockerError> {
        let id = checked_id(id)?;
        if command.is_empty() {
            return Err(DockerError::Parse("empty command".into()));
        }
        let v = self.call_json(
            "POST",
            &format!("/containers/{id}/exec"),
            &exec_body(command, tty, options),
        )?;
        v.get("Id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| DockerError::Parse("no exec id".into()))
    }

    /// Exit code of a finished exec.
    pub fn exec_exit_code(&self, exec: &str) -> Result<Option<i64>, DockerError> {
        let exec = checked_id(exec)?;
        let v = self.json(&format!("/exec/{exec}/json"))?;
        Ok(if v.get("Running").and_then(Value::as_bool) == Some(true) {
            None
        } else {
            v.get("ExitCode").and_then(Value::as_i64)
        })
    }

    /// Runs `command` in container `id` without a TTY and returns its output and exit code
    /// (`SPEC-0240` behaviour 4). Output beyond [`EXEC_LIMIT`] is dropped; after `timeout` the
    /// connection is closed and the exit code is unknown.
    pub fn exec(
        &self,
        id: &str,
        command: &[String],
        options: &ExecOptions,
        timeout: Duration,
    ) -> Result<ExecOutput, DockerError> {
        let exec = self.exec_create(id, command, false, options)?;
        let mut conn = self.send(
            "POST",
            &format!("/exec/{exec}/start"),
            Some(&serde_json::json!({"Detach": false, "Tty": false})),
            false,
        )?;
        conn.wait_forever();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut raw = Vec::new();
            let mut buf = [0u8; 16384];
            let mut truncated = false;
            loop {
                match conn.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if raw.len() < EXEC_LIMIT + 4096 {
                            raw.extend_from_slice(&buf[..n]);
                        } else {
                            truncated = true;
                        }
                    }
                }
            }
            let _ = tx.send((raw, truncated));
        });
        let (raw, mut truncated, finished) = match rx.recv_timeout(timeout) {
            Ok((raw, t)) => (raw, t, true),
            Err(_) => (Vec::new(), false, false),
        };
        if !finished {
            return Ok(ExecOutput {
                output: String::new(),
                exit_code: None,
                truncated: false,
            });
        }
        let (status, body) = parse_head(&raw)?;
        if status >= 300 {
            let (_, body) = parse_response(&raw)?;
            return Err(DockerError::Http(status, message(&body)));
        }
        let mut output = demux_logs(body);
        if output.len() > EXEC_LIMIT {
            let mut cut = EXEC_LIMIT;
            while !output.is_char_boundary(cut) {
                cut -= 1;
            }
            output.truncate(cut);
            truncated = true;
        }
        Ok(ExecOutput {
            output,
            exit_code: self.exec_exit_code(&exec)?,
            truncated,
        })
    }

    /// Starts an interactive shell in container `id` with a TTY of `rows` × `cols` and returns
    /// its raw stream (`SPEC-0240` behaviours 1, 2).
    pub fn exec_attach(
        &self,
        id: &str,
        command: &[String],
        rows: u16,
        cols: u16,
    ) -> Result<ExecSession, DockerError> {
        let options = ExecOptions {
            env: vec!["TERM=xterm-256color".into()],
            ..ExecOptions::default()
        };
        let exec = self.exec_create(id, command, true, &options)?;
        let mut conn = self.send(
            "POST",
            &format!("/exec/{exec}/start"),
            Some(&serde_json::json!({"Detach": false, "Tty": true, "ConsoleSize": [rows, cols]})),
            true,
        )?;
        // Read the answer's head byte by byte so no byte of the stream is taken with it.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match conn.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => return Err(DockerError::Parse("no answer to the exec start".into())),
            }
            if head.len() > 16384 {
                return Err(DockerError::Parse("answer head too long".into()));
            }
        }
        let (status, _) = parse_head(&head)?;
        if status != 101 && status != 200 {
            let mut rest = head.clone();
            let _ = conn.read_to_end(&mut rest);
            let (_, body) = parse_response(&rest)?;
            return Err(DockerError::Http(status, message(&body)));
        }
        conn.wait_forever();
        let _ = self.exec_resize(&exec, rows, cols);
        Ok(ExecSession { id: exec, conn })
    }

    /// Tells the daemon the terminal size of an interactive exec.
    pub fn exec_resize(&self, exec: &str, rows: u16, cols: u16) -> Result<(), DockerError> {
        let exec = checked_id(exec)?;
        self.call("POST", &format!("/exec/{exec}/resize?h={rows}&w={cols}"))
            .map(|_| ())
    }

    /// Sends a request; a status of 300 or more is an error with the daemon's message.
    fn call(&self, method: &str, path: &str) -> Result<Vec<u8>, DockerError> {
        let (status, body) = self.request(method, path)?;
        if status >= 300 {
            return Err(DockerError::Http(status, message(&body)));
        }
        Ok(body)
    }

    fn json(&self, path: &str) -> Result<Value, DockerError> {
        let body = self.call("GET", path)?;
        serde_json::from_slice(&body).map_err(|e| DockerError::Parse(e.to_string()))
    }

    /// Version of the daemon.
    pub fn info(&self) -> Result<DockerInfo, DockerError> {
        let v = self.json("/version")?;
        let s = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(DockerInfo {
            version: s("Version"),
            api_version: s("ApiVersion"),
            platform: format!("{}/{}", s("Os"), s("Arch")),
        })
    }

    /// All containers, running first, then by name.
    pub fn containers(&self) -> Result<Vec<Container>, DockerError> {
        let v = self.json("/containers/json?all=1")?;
        let mut out: Vec<Container> = v
            .as_array()
            .ok_or_else(|| DockerError::Parse("container list is not an array".into()))?
            .iter()
            .map(container)
            .collect();
        out.sort_by(|a, b| (a.state != "running", &a.name).cmp(&(b.state != "running", &b.name)));
        Ok(out)
    }

    /// All images, tagged ones first by tag.
    pub fn images(&self) -> Result<Vec<Image>, DockerError> {
        let v = self.json("/images/json")?;
        let mut out: Vec<Image> = v
            .as_array()
            .ok_or_else(|| DockerError::Parse("image list is not an array".into()))?
            .iter()
            .map(|i| Image {
                id: short_id(i.get("Id").and_then(Value::as_str).unwrap_or_default()),
                tags: i
                    .get("RepoTags")
                    .and_then(Value::as_array)
                    .map(|t| {
                        t.iter()
                            .filter_map(Value::as_str)
                            .filter(|t| *t != "<none>:<none>")
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
                size: i.get("Size").and_then(Value::as_u64).unwrap_or(0),
            })
            .collect();
        out.sort_by(|a, b| (a.tags.is_empty(), &a.tags).cmp(&(b.tags.is_empty(), &b.tags)));
        Ok(out)
    }

    /// Starts, stops, restarts or removes container `id` (behaviour 2). Starting a running or
    /// stopping a stopped container is not an error.
    pub fn act(&self, id: &str, action: ContainerAction) -> Result<(), DockerError> {
        let id = checked_id(id)?;
        let (method, path) = match action {
            ContainerAction::Start => ("POST", format!("/containers/{id}/start")),
            ContainerAction::Stop => ("POST", format!("/containers/{id}/stop?t=10")),
            ContainerAction::Restart => ("POST", format!("/containers/{id}/restart?t=10")),
            ContainerAction::Remove => ("DELETE", format!("/containers/{id}?force=1")),
        };
        match self.request(method, &path)? {
            // 304: already started or stopped.
            (status, _) if status < 300 || status == 304 => Ok(()),
            (status, body) => {
                let message = serde_json::from_slice::<Value>(&body)
                    .ok()
                    .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
                    .unwrap_or_default();
                Err(DockerError::Http(status, message))
            }
        }
    }

    /// The last `tail` lines of container `id`'s output, stdout and stderr interleaved
    /// (behaviour 3).
    pub fn logs(&self, id: &str, tail: usize) -> Result<String, DockerError> {
        let id = checked_id(id)?;
        let body = self.call(
            "GET",
            &format!("/containers/{id}/logs?stdout=1&stderr=1&tail={tail}"),
        )?;
        Ok(demux_logs(&body))
    }
}

/// The daemon's error message in a body.
fn message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_owned())
}

/// Body of the exec create request.
fn exec_body(command: &[String], tty: bool, options: &ExecOptions) -> Value {
    let mut body = serde_json::json!({
        "AttachStdin": tty,
        "AttachStdout": true,
        "AttachStderr": true,
        "Tty": tty,
        "Cmd": command,
    });
    if !options.env.is_empty() {
        body["Env"] = serde_json::json!(options.env);
    }
    if let Some(w) = &options.workdir {
        body["WorkingDir"] = serde_json::json!(w);
    }
    if let Some(u) = &options.user {
        body["User"] = serde_json::json!(u);
    }
    body
}

/// Status of an answer head and the bytes after it (not de-chunked).
fn parse_head(raw: &[u8]) -> Result<(u16, &[u8]), DockerError> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| DockerError::Parse("no HTTP header".into()))?;
    let head = String::from_utf8_lossy(&raw[..end]);
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| DockerError::Parse("no HTTP status".into()))?;
    Ok((status, &raw[end + 4..]))
}

fn short_id(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

fn container(c: &Value) -> Container {
    let s = |k: &str| {
        c.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut ports: Vec<String> = c
        .get("Ports")
        .and_then(Value::as_array)
        .map(|ps| {
            ps.iter()
                .map(|p| {
                    let private = p.get("PrivatePort").and_then(Value::as_u64).unwrap_or(0);
                    let kind = p.get("Type").and_then(Value::as_str).unwrap_or("tcp");
                    match p.get("PublicPort").and_then(Value::as_u64) {
                        Some(public) => format!(
                            "{}:{public}->{private}/{kind}",
                            p.get("IP").and_then(Value::as_str).unwrap_or("0.0.0.0")
                        ),
                        None => format!("{private}/{kind}"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    ports.sort();
    ports.dedup();
    let label = |k: &str| {
        c.get("Labels")
            .and_then(|l| l.get(k))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Container {
        id: short_id(&s("Id")),
        name: c
            .get("Names")
            .and_then(Value::as_array)
            .and_then(|n| n.first())
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned(),
        image: s("Image"),
        state: s("State"),
        status: s("Status"),
        ports,
        compose_project: label("com.docker.compose.project"),
        compose_service: label("com.docker.compose.service"),
    }
}

/// Status and body of an HTTP/1.1 response; chunked bodies are joined.
fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), DockerError> {
    let (status, body) = parse_head(raw)?;
    let end = raw.len() - body.len() - 4;
    let head = String::from_utf8_lossy(&raw[..end]);
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if !chunked {
        return Ok((status, body.to_vec()));
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at < body.len() {
        let line_end = body[at..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .map(|p| at + p)
            .ok_or_else(|| DockerError::Parse("broken chunk".into()))?;
        let size_text = String::from_utf8_lossy(&body[at..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| DockerError::Parse(format!("bad chunk size {size_text:?}")))?;
        if size == 0 {
            break;
        }
        let start = line_end + 2;
        let stop = (start + size).min(body.len());
        out.extend_from_slice(&body[start..stop]);
        at = stop + 2;
    }
    Ok((status, out))
}

/// Log output: the daemon's multiplexed frames (8-byte header with stream and length) for
/// containers without a TTY, raw bytes otherwise.
pub fn demux_logs(bytes: &[u8]) -> String {
    let framed = bytes.len() >= 8 && bytes[0] <= 2 && bytes[1..4] == [0, 0, 0];
    if !framed {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let start = at + 8;
        let stop = (start + len).min(bytes.len());
        out.extend_from_slice(&bytes[start..stop]);
        at = stop;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_from_docker_host_and_known_sockets() {
        assert_eq!(
            find_host(Some("unix:///tmp/d.sock"), None),
            Some(DockerHost::Unix("/tmp/d.sock".into()))
        );
        assert_eq!(
            find_host(Some("tcp://10.0.0.2:2375/"), None),
            Some(DockerHost::Tcp("10.0.0.2:2375".into()))
        );
        assert_eq!(find_host(Some("npipe:////./pipe/x"), None), None);
        let home = std::env::temp_dir().join(format!("idecraft-dockhome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".colima/default")).unwrap();
        std::fs::write(home.join(".colima/default/docker.sock"), "").unwrap();
        assert_eq!(
            find_host(None, Some(&home)),
            Some(DockerHost::Unix(home.join(".colima/default/docker.sock")))
        );
        let _ = std::fs::remove_dir_all(home);
        assert_eq!(
            DockerHost::Unix("/var/run/docker.sock".into()).to_string(),
            "unix:///var/run/docker.sock"
        );
    }

    #[test]
    fn engine_addresses_ssh_and_warnings() {
        assert_eq!(
            DockerHost::parse("ssh://ci@build.example:2222", vec![]),
            Some(DockerHost::Ssh {
                destination: "ci@build.example".into(),
                port: Some(2222),
                extra: vec![],
                program: None,
            })
        );
        let h = DockerHost::parse("ssh://box/", vec!["-F".into(), "cfg".into()]).unwrap();
        assert_eq!(h.to_string(), "ssh://box");
        let Ok(line) = h.ssh_line().unwrap() else {
            eprintln!("skipped: no ssh with a post-quantum key exchange");
            return;
        };
        let args = line.args();
        // The shared options come first, so extra arguments cannot weaken them; the route is
        // pinned after them (ADR-0025 v5).
        let shared = idecraft_remote::testing::shared_options("ssh");
        assert_eq!(args[..shared.len()], shared[..]);
        assert_eq!(
            args[shared.len()..shared.len() + 2],
            ["-o", "ProxyCommand=none"]
        );
        let f = args.iter().position(|a| a == "-F").unwrap();
        assert_eq!(args[f + 1], "cfg");
        let kex = args
            .iter()
            .find_map(|a| a.strip_prefix("KexAlgorithms="))
            .unwrap();
        assert!(kex.split(',').all(|a| idecraft_remote::PQ_KEX.contains(&a)));
        assert!(args.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(
            args.windows(2)
                .any(|w| w == ["-o", "StrictHostKeyChecking=yes"])
        );
        assert!(args.ends_with(&[
            "--".into(),
            "box".into(),
            "docker".into(),
            "system".into(),
            "dial-stdio".into()
        ]));
        assert_eq!(DockerHost::parse("ssh://-oProxyCommand=x", vec![]), None);
        assert_eq!(DockerHost::parse("ssh://a b", vec![]), None);
        assert_eq!(DockerHost::parse("npipe:////./pipe/docker", vec![]), None);
        assert_eq!(
            DockerHost::Tcp("10.0.0.2:2375".into()).warning(),
            Some("unencrypted and unauthenticated TCP connection")
        );
        assert_eq!(DockerHost::Tcp("127.0.0.1:2375".into()).warning(), None);
        assert_eq!(DockerHost::Tcp("[::1]:2375".into()).warning(), None);
        assert_eq!(DockerHost::Tcp("localhost:2375".into()).warning(), None);
    }

    #[test]
    fn exec_bodies_and_heads() {
        let b = exec_body(
            &["ls".into(), "-l".into()],
            false,
            &ExecOptions {
                workdir: Some("/srv".into()),
                env: vec!["A=1".into()],
                user: Some("1000".into()),
            },
        );
        assert_eq!(
            b,
            serde_json::json!({"AttachStdin": false, "AttachStdout": true, "AttachStderr": true,
                "Tty": false, "Cmd": ["ls", "-l"], "Env": ["A=1"], "WorkingDir": "/srv", "User": "1000"})
        );
        assert_eq!(
            exec_body(&["sh".into()], true, &ExecOptions::default())["AttachStdin"],
            true
        );
        let (status, rest) =
            parse_head(b"HTTP/1.1 101 UPGRADED\r\nUpgrade: tcp\r\n\r\n$ ").unwrap();
        assert_eq!((status, rest), (101, &b"$ "[..]));
        assert_eq!(
            message(br#"{"message":"is not running"}"#),
            "is not running"
        );
    }

    #[test]
    fn responses_plain_and_chunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
        assert_eq!(parse_response(plain).unwrap(), (200, b"[]".to_vec()));
        let chunked =
            b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"me\r\n9\r\nssage\":1}\r\n0\r\n\r\n";
        assert_eq!(
            parse_response(chunked).unwrap(),
            (404, b"{\"message\":1}".to_vec())
        );
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn containers_and_ids() {
        let c = container(&serde_json::json!({
            "Id": "0123456789abcdef0123", "Names": ["/web-1"], "Image": "nginx:1.27",
            "State": "running", "Status": "Up 5 minutes",
            "Ports": [{"PrivatePort": 80, "PublicPort": 8080, "Type": "tcp", "IP": "0.0.0.0"},
                      {"PrivatePort": 80, "PublicPort": 8080, "Type": "tcp", "IP": "0.0.0.0"},
                      {"PrivatePort": 443, "Type": "tcp"}],
            "Labels": {"com.docker.compose.project": "shop", "com.docker.compose.service": "web"}
        }));
        assert_eq!(c.id, "0123456789ab");
        assert_eq!(c.name, "web-1");
        assert_eq!(c.ports, ["0.0.0.0:8080->80/tcp", "443/tcp"]);
        assert_eq!(c.compose_project.as_deref(), Some("shop"));
        assert_eq!(c.compose_service.as_deref(), Some("web"));
        assert!(checked_id("web-1").is_ok());
        assert!(checked_id("../x").is_err());
        assert!(checked_id("").is_err());
    }

    #[test]
    fn logs_with_and_without_frames() {
        let mut framed = vec![1, 0, 0, 0, 0, 0, 0, 6];
        framed.extend_from_slice(b"hello\n");
        framed.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 4]);
        framed.extend_from_slice(b"oops");
        assert_eq!(demux_logs(&framed), "hello\noops");
        assert_eq!(demux_logs(b"plain tty output\n"), "plain tty output\n");
    }
}
