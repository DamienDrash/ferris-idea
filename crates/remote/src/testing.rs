//! A real SSH server for tests: the system `sshd` started as the current user on a free loopback
//! port, with its own host key, a client key made for the test and a private client config.
//! Nothing of the user's SSH setup is read or changed.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::ssh::SshTarget;

/// The shared options of every SSH path for `program`, for tests that compare command lines.
/// Product code never builds an `ssh` from them: it uses [`crate::SshCommand`] (ADR-0025 section
/// 1a, workspace scan in `crates/remote/tests/it/pq_kex.rs`).
pub fn shared_options(program: &str) -> Vec<String> {
    crate::pq::ssh_options(program)
}

/// Writes the log of connections through a `ProxyCommand` of the user's own into `dir` instead of
/// the IDECraft log folder, for the rest of the process (first call wins).
pub fn redirect_logs(dir: &Path) {
    let _ = crate::jump::LOG_DIR.set(dir.to_path_buf());
}

static ISOLATED: AtomicBool = AtomicBool::new(false);
static STRETCH: AtomicU32 = AtomicU32::new(1);

/// The folder of shared connections (`ControlPath`) of a process that started a [`TestHost`]: its
/// own, so that test runs side by side never share or wait on another run's connection.
pub(crate) static CONTROL_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The stretch of call limits a process gets with its first [`TestHost`].
pub const DEFAULT_STRETCH: u32 = 4;

/// From now on, for the rest of the process, a host list read from an SSH configuration outside
/// the temporary folder (the user's `~/.ssh/config` above all, or a file it includes) and an `ssh`
/// started without a `-F <config>` (the test's own, or one the test environment names, such as the
/// Lima VM's) end the test with a panic instead of reading the user's hosts, keys, agent or known
/// hosts. Test helpers that open the Remote Host window or connect call it first; product code
/// never does.
pub fn isolate_ssh() {
    ISOLATED.store(true, Ordering::SeqCst);
}

/// Whether [`isolate_ssh`] was called in this process.
pub fn ssh_isolated() -> bool {
    ISOLATED.load(Ordering::SeqCst)
}

/// The folders a test's SSH configuration may lie in: the temporary folder and `/tmp`, also
/// with symbolic links resolved (`/var` and `/tmp` are links on macOS).
fn test_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir(), PathBuf::from("/tmp")];
    let resolved: Vec<PathBuf> = roots.iter().filter_map(|r| r.canonicalize().ok()).collect();
    roots.extend(resolved);
    roots
}

/// Whether `file` is an SSH configuration a test may read: inside [`test_roots`] (so never the
/// user's own `~/.ssh/config` or a file in the user's home folder).
pub fn is_test_config(file: &Path) -> bool {
    let resolved = file
        .parent()
        .and_then(|d| d.canonicalize().ok())
        .map(|d| d.join(file.file_name().unwrap_or_default()))
        .unwrap_or_else(|| file.to_path_buf());
    file.is_absolute()
        && !file
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        && test_roots()
            .iter()
            .any(|r| file.starts_with(r) || resolved.starts_with(r))
}

/// Panics when isolation is on and `file` is not a test's own configuration (see
/// [`isolate_ssh`]). The message names neither the file's content nor its hosts.
pub(crate) fn check_config_read(file: &Path) {
    if ssh_isolated() && !is_test_config(file) {
        panic!(
            "a test would read an SSH configuration outside its temporary folder ({}); give it a \
             config of its own (`-F`, `TestHost`)",
            if file.ends_with(".ssh/config") {
                "the user's ~/.ssh/config".to_owned()
            } else {
                "a file outside the temporary folder".to_owned()
            }
        );
    }
}

/// Panics when isolation is on and the `ssh` arguments `args` carry no `-F <config>` other than
/// the user's: without one `ssh` reads the user's `~/.ssh/config`, keys and known hosts.
pub(crate) fn check_ssh_args(args: &[String]) {
    if !ssh_isolated() {
        return;
    }
    match crate::jump::config_file(args) {
        Some(f) if !Path::new(&f).ends_with(".ssh/config") => {}
        _ => panic!(
            "a test would start ssh with the user's ~/.ssh/config; pass `-F <config>` of its own \
             (`TestHost::target`)"
        ),
    }
}

/// `-F <dir>/ssh_config_empty`, an empty SSH configuration written into `dir`: the extra `ssh`
/// arguments of a window or server in a test that lists no hosts and reads nothing of the user's.
pub fn empty_ssh_config(dir: &Path) -> Vec<String> {
    let _ = std::fs::create_dir_all(dir);
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let file = dir.join("ssh_config_empty");
    let _ = std::fs::write(&file, "");
    vec!["-F".into(), file.display().to_string()]
}

/// From now on, for the rest of the process, every time limit of a call on a host is `factor`
/// times as long (at most 10): a test machine under heavy load then only waits longer for the same
/// answer, and a real hang still ends in the same error, later. Product code never calls it.
pub fn stretch_limits(factor: u32) {
    STRETCH.store(factor.clamp(1, 10), Ordering::SeqCst);
}

/// `limit` stretched by [`stretch_limits`] (unchanged outside tests).
pub(crate) fn stretched(limit: Duration) -> Duration {
    limit * STRETCH.load(Ordering::SeqCst)
}

/// The turn of one test that starts a [`TestHost`] and works on it through the window or a
/// backend; the next such test waits until the turn is dropped. These tests upload a debug build
/// of several hundred megabytes through SSH and start it on the host: run side by side (in one
/// test program, or in the check runs of several branches on one Mac) they load the machine on top
/// of the parallel builds, and under a load average above 150 their first sessions waited past
/// every limit at once. One at a time, each test's limits only meet the machine's load, not its
/// siblings'.
///
/// The turn is taken within the process (a lock) and across the user's processes (an exclusive
/// `flock` on `/tmp/idecraft-ssh-turn-<user>`, created with mode 0600 and not used when it is
/// a link; a file others may read or write is not used, and then the turn is only the
/// process's). The file holds nothing; nothing else is shared between the tests (each keeps its
/// own server, folders, connections and home folder). A failed test does not block the others: a
/// poisoned lock is taken over, and the file lock ends with its process.
pub fn one_host_at_a_time() -> HostTurn {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let local = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    HostTurn {
        file: turn_file().filter(|f| f.lock().is_ok()),
        _local: local,
    }
}

/// The file whose lock is the turn across processes, if it is the user's own and private.
#[cfg(unix)]
fn turn_file() -> Option<std::fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    let path = PathBuf::from(format!("/tmp/idecraft-ssh-turn-{user}"));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .ok()?;
    let meta = file.metadata().ok()?;
    // The path itself is the file opened (no link to another file), and nobody else may read or
    // write it (opened for writing, so it is ours).
    let at = std::fs::symlink_metadata(&path).ok()?;
    (at.file_type().is_file()
        && at.dev() == meta.dev()
        && at.ino() == meta.ino()
        && meta.permissions().mode() & 0o077 == 0)
        .then_some(file)
}

#[cfg(not(unix))]
fn turn_file() -> Option<std::fs::File> {
    None
}

/// A test's turn from [`one_host_at_a_time`]; the next test's turn begins when it is dropped.
pub struct HostTurn {
    file: Option<std::fs::File>,
    _local: std::sync::MutexGuard<'static, ()>,
}

impl Drop for HostTurn {
    fn drop(&mut self) {
        if let Some(f) = &self.file {
            let _ = f.unlock();
        }
    }
}

static STALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long a session process of a [`TestHost`] may stay before running its command. It needs
/// milliseconds; a slow directory lookup under load may take seconds and is left alone, only one
/// that waits this long is taken as stalled.
const STALL_AFTER: Duration = Duration::from_secs(20);

/// How many session processes of this process's test servers were ended because they stalled
/// before running their command (0 outside tests).
pub(crate) fn stalls() -> u64 {
    STALLS.load(Ordering::SeqCst)
}

/// Watches the sessions of the test server `server` (a process this process started) until
/// `stop`: a session process that has not run its command after [`STALL_AFTER`] is ended.
///
/// macOS's `sshd-session` sets up the session's environment in a forked child before it runs the
/// command; under the load of parallel builds that child at times waits without end in a
/// directory lookup (`confstr(_CS_DARWIN_USER_TEMP_DIR)` → `mbr_uid_to_uuid` → an XPC call to
/// `opendirectoryd` that is never answered; sampled on 2026-11-17), and the client waits with it
/// until its time limit. Ending that child makes the call fail at once with `ssh`'s 255, and as
/// the command never ran, [`crate::Remote`] may start it again ([`stalls`]). Only descendants of
/// `server` are looked at, never another process. Giving the sessions a `TMPDIR` of their own does
/// not avoid the lookup: macOS's sshd replaces it with `confstr`'s answer (checked with `SetEnv`
/// and with `sshd` started under another `TMPDIR`).
fn watch_stalled_sessions(server: u32, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_secs(1));
        let Ok(out) = Command::new("ps")
            .args(["-axo", "pid=,ppid=,etime=,command="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        for pid in stalled_sessions(&text, server, STALL_AFTER) {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            // Counted before the kill: the client ends as soon as the child does, and the call
            // that sees it fail must already find the count raised to repeat it.
            STALLS.fetch_add(1, Ordering::SeqCst);
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// From a `ps -axo pid=,ppid=,etime=,command=` listing: the session processes below `server`
/// (server → connection → session → this child) that still run as `sshd-session` (they have not
/// started the command) after at least `after`.
fn stalled_sessions(listing: &str, server: u32, after: Duration) -> Vec<u32> {
    struct Row<'a> {
        pid: u32,
        ppid: u32,
        age: Option<u64>,
        command: &'a str,
    }
    let rows: Vec<Row> = listing
        .lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let age = elapsed_seconds(parts.next()?);
            let mut command = l.trim_start();
            for _ in 0..3 {
                let end = command.find(char::is_whitespace)?;
                command = command[end..].trim_start();
            }
            Some(Row {
                pid,
                ppid,
                age,
                command,
            })
        })
        .collect();
    let children = |of: u32| rows.iter().filter(move |r| r.ppid == of);
    let mut stalled = Vec::new();
    for connection in children(server) {
        for session in children(connection.pid) {
            for child in children(session.pid) {
                if child.command.starts_with("sshd-session:")
                    && child.age.is_some_and(|a| a >= after.as_secs())
                {
                    stalled.push(child.pid);
                }
            }
        }
    }
    stalled
}

/// `ps`'s elapsed time (`[[dd-]hh:]mm:ss`) in seconds.
fn elapsed_seconds(etime: &str) -> Option<u64> {
    let (days, clock) = match etime.split_once('-') {
        Some((d, c)) => (d.parse::<u64>().ok()?, c),
        None => (0, etime),
    };
    let mut secs = 0;
    for part in clock.split(':') {
        secs = secs * 60 + part.parse::<u64>().ok()?;
    }
    Some(days * 86_400 + secs)
}

/// A running test server; ends with the value.
pub struct TestHost {
    /// How to reach it (alias `idecraft-test` with `-F <config>`).
    pub target: SshTarget,
    /// The test folder with keys and configs.
    pub dir: PathBuf,
    child: Child,
    /// Ends the server's [`watch_stalled_sessions`] thread.
    watch: std::sync::Arc<AtomicBool>,
}

fn sshd() -> Option<PathBuf> {
    ["/usr/sbin/sshd", "/usr/bin/sshd", "/usr/local/sbin/sshd"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

fn keygen(path: &Path) -> bool {
    Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

impl TestHost {
    /// Starts a server with its files in `dir`; `None` when `sshd` or `ssh-keygen` is missing or
    /// the server does not come up.
    pub fn start(dir: &Path) -> Option<Self> {
        Self::start_with(dir, "")
    }

    /// Starts a server that offers only classical key exchanges (no post-quantum method), as an
    /// OpenSSH before 9.0 would; IDECraft must refuse it (ADR-0025, PO-STATUS question 29).
    pub fn start_classical(dir: &Path) -> Option<Self> {
        Self::start_with(
            dir,
            "KexAlgorithms curve25519-sha256,curve25519-sha256@libssh.org,ecdh-sha2-nistp256\n",
        )
    }

    /// Starts a server whose sessions get the variables `env` (`SetEnv` of `sshd_config`), such as
    /// a `HOME` of the test's own, so that nothing the test brings to the host lands in the user's
    /// home folder, or a `PATH` with a folder the test installs a program into.
    pub fn start_with_env(dir: &Path, env: &[(&str, &str)]) -> Option<Self> {
        if env.is_empty() {
            return Self::start(dir);
        }
        let words: Vec<String> = env.iter().map(|(k, v)| format!("\"{k}={v}\"")).collect();
        Self::start_with(dir, &format!("SetEnv {}\n", words.join(" ")))
    }

    /// Waits until a whole session through IDECraft's own `ssh` succeeds on the server (`true`
    /// there): the ready signal of a server that accepts connections but, under load, takes long
    /// to log a user in. A test that waits for it measures its own steps, not the server's start.
    /// Returns the last failure when `limit` passes first.
    pub fn wait_ready(&self, limit: Duration) -> Result<(), String> {
        let remote = crate::Remote::new(self.target.clone());
        let start = Instant::now();
        loop {
            let failure = match remote.sh("true", &[], None, crate::ssh::TIMEOUT) {
                Ok(out) if out.success() => return Ok(()),
                Ok(out) => format!("exit code {:?}: {}", out.exit_code, out.stderr.trim()),
                Err(e) => e.to_string(),
            };
            if start.elapsed() > limit {
                return Err(failure);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Starts a server with `extra` lines appended to its `sshd_config`. Everything a run shares
    /// with a run of the test suite beside it is made its own: the port is a free one picked anew
    /// (a server that lost the port to another run between the pick and its start is started again
    /// on another), the folder of shared connections (`ControlPath`) is one of this process, the
    /// client uses no SSH agent, and the time limits of calls are stretched for a machine under the
    /// load of parallel runs ([`stretch_limits`], [`DEFAULT_STRETCH`]).
    fn start_with(dir: &Path, extra: &str) -> Option<Self> {
        sshd()?;
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        // Short: Unix socket paths are limited to about 100 bytes.
        let _ = CONTROL_DIR.set(PathBuf::from(format!(
            "/tmp/idecraft-ssh-{user}-t{}",
            std::process::id()
        )));
        if STRETCH.load(Ordering::SeqCst) < DEFAULT_STRETCH {
            stretch_limits(DEFAULT_STRETCH);
        }
        for _ in 0..5 {
            match Self::start_once(dir, extra) {
                Ok(host) => return Some(host),
                Err(true) => continue,
                Err(false) => return None,
            }
        }
        None
    }

    /// One start; `Err(true)` when the server ended at once (its port was taken meanwhile) and
    /// another start may succeed, `Err(false)` when it cannot.
    fn start_once(dir: &Path, extra: &str) -> Result<Self, bool> {
        let sshd = sshd().ok_or(false)?;
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).map_err(|_| false)?;
        let dir = dir.canonicalize().map_err(|_| false)?;
        let (host_key, client_key) = (dir.join("host_key"), dir.join("client_key"));
        if !keygen(&host_key) || !keygen(&client_key) {
            return Err(false);
        }
        std::fs::copy(dir.join("client_key.pub"), dir.join("authorized_keys"))
            .map_err(|_| false)?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|_| false)?
            .local_addr()
            .map_err(|_| false)?
            .port();
        std::fs::write(
            dir.join("sshd_config"),
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\n\
                 PasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\n\
                 StrictModes no\nPidFile {}\n{extra}",
                host_key.display(),
                dir.join("authorized_keys").display(),
                dir.join("sshd.pid").display()
            ),
        )
        .map_err(|_| false)?;
        let host_pub = std::fs::read_to_string(dir.join("host_key.pub")).map_err(|_| false)?;
        std::fs::write(
            dir.join("known_hosts"),
            format!("[127.0.0.1]:{port} {}", host_pub.trim()),
        )
        .map_err(|_| false)?;
        let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
        std::fs::write(
            dir.join("ssh_config"),
            format!(
                "Host idecraft-test\n  HostName 127.0.0.1\n  Port {port}\n  User {user}\n  \
                 IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  \
                 UserKnownHostsFile {}\n",
                client_key.display(),
                dir.join("known_hosts").display()
            ),
        )
        .map_err(|_| false)?;
        let child = Command::new(sshd)
            .arg("-D")
            .arg("-f")
            .arg(dir.join("sshd_config"))
            .arg("-E")
            .arg(dir.join("sshd.log"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| false)?;
        let target = SshTarget {
            host: "idecraft-test".into(),
            port: None,
            extra: vec!["-F".into(), dir.join("ssh_config").display().to_string()],
            program: None,
        };
        let mut host = Self {
            target,
            dir,
            child,
            watch: std::sync::Arc::new(AtomicBool::new(false)),
        };
        // Generous: under heavy load `sshd` takes long to start, and giving up here would skip the
        // test as if there were no `sshd`. A server that never listens still ends the wait.
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(90) {
            // Our server must still run: a connection alone could reach another run's server
            // that took the port.
            if !matches!(host.child.try_wait(), Ok(None)) {
                return Err(true);
            }
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                let (server, stop) = (host.child.id(), std::sync::Arc::clone(&host.watch));
                std::thread::spawn(move || watch_stalled_sessions(server, &stop));
                return Ok(host);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(false)
    }

    /// The same server reached with an empty `known_hosts` (host key unknown).
    pub fn unknown_key_target(&self) -> SshTarget {
        self.config_variant("unknown", "")
    }

    /// The same server with a different key recorded for it (host key changed).
    pub fn changed_key_target(&self) -> SshTarget {
        let other = self.dir.join("other_key");
        let _ = keygen(&other);
        let line = std::fs::read_to_string(self.dir.join("known_hosts")).unwrap_or_default();
        let pubkey = std::fs::read_to_string(other.with_extension("pub")).unwrap_or_default();
        let host = line.split_whitespace().next().unwrap_or("").to_owned();
        self.config_variant("changed", &format!("{host} {}", pubkey.trim()))
    }

    fn config_variant(&self, name: &str, known: &str) -> SshTarget {
        let known_file = self.dir.join(format!("known_hosts_{name}"));
        let _ = std::fs::write(&known_file, known);
        let config = std::fs::read_to_string(self.dir.join("ssh_config"))
            .unwrap_or_default()
            .replace(
                &self.dir.join("known_hosts").display().to_string(),
                &known_file.display().to_string(),
            )
            .replace("Host idecraft-test", &format!("Host idecraft-{name}"));
        let file = self.dir.join(format!("ssh_config_{name}"));
        let _ = std::fs::write(&file, config);
        SshTarget {
            host: format!("idecraft-{name}"),
            port: None,
            extra: vec!["-F".into(), file.display().to_string()],
            program: None,
        }
    }
}

impl Drop for TestHost {
    fn drop(&mut self) {
        self.watch.store(true, Ordering::SeqCst);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_session_children_of_the_own_server_that_wait_are_stalled() {
        let listing = "\
  100     1    05:00 sshd: /usr/sbin/sshd -D -f /t/sshd_config [listener]
  101   100    04:59 sshd-session: me [priv]
  102   101    04:59 sshd-session: me@notty
  103   102    00:12 sshd-session: me@notty
  104   102    00:30 sleep 30
  105   102    00:02 sshd-session: me@notty
  200     1    05:00 sshd: /usr/sbin/sshd -D -f /other/sshd_config [listener]
  201   200    04:59 sshd-session: me [priv]
  202   201    04:59 sshd-session: me@notty
  203   202 01:00:12 sshd-session: me@notty
";
        assert_eq!(
            stalled_sessions(listing, 100, Duration::from_secs(10)),
            vec![103],
            "only the own server's child that has not run its command for 10 s"
        );
        assert_eq!(elapsed_seconds("1-02:03:04"), Some(93_784));
        assert_eq!(elapsed_seconds("00:07"), Some(7));
        assert_eq!(elapsed_seconds("x"), None);
    }

    #[test]
    fn a_turn_ends_when_dropped_and_its_file_is_private() {
        let first = one_host_at_a_time();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert!(first.file.is_some(), "the turn across processes is taken");
            let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
            let meta = std::fs::metadata(format!("/tmp/idecraft-ssh-turn-{user}")).unwrap();
            assert_eq!(meta.permissions().mode() & 0o077, 0);
            assert_eq!(meta.len(), 0, "the file holds nothing");
        }
        drop(first);
        // The next turn begins (after another process's turn at most, never blocked by this one).
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _second = one_host_at_a_time();
            let _ = tx.send(());
        });
        assert!(rx.recv().is_ok());
    }
}
