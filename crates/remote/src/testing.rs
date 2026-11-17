//! A real SSH server for tests: the system `sshd` started as the current user on a free loopback
//! port, with its own host key, a client key made for the test and a private client config.
//! Nothing of the user's SSH setup is read or changed.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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

/// A running test server; ends with the value.
pub struct TestHost {
    /// How to reach it (alias `idecraft-test` with `-F <config>`).
    pub target: SshTarget,
    /// The test folder with keys and configs.
    pub dir: PathBuf,
    child: Child,
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

    /// Starts a server with `extra` lines appended to its `sshd_config`.
    fn start_with(dir: &Path, extra: &str) -> Option<Self> {
        let sshd = sshd()?;
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).ok()?;
        let dir = dir.canonicalize().ok()?;
        let (host_key, client_key) = (dir.join("host_key"), dir.join("client_key"));
        if !keygen(&host_key) || !keygen(&client_key) {
            return None;
        }
        std::fs::copy(dir.join("client_key.pub"), dir.join("authorized_keys")).ok()?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .ok()?
            .local_addr()
            .ok()?
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
        .ok()?;
        let host_pub = std::fs::read_to_string(dir.join("host_key.pub")).ok()?;
        std::fs::write(
            dir.join("known_hosts"),
            format!("[127.0.0.1]:{port} {}", host_pub.trim()),
        )
        .ok()?;
        let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
        std::fs::write(
            dir.join("ssh_config"),
            format!(
                "Host idecraft-test\n  HostName 127.0.0.1\n  Port {port}\n  User {user}\n  \
                 IdentityFile {}\n  IdentitiesOnly yes\n  UserKnownHostsFile {}\n",
                client_key.display(),
                dir.join("known_hosts").display()
            ),
        )
        .ok()?;
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
            .ok()?;
        let target = SshTarget {
            host: "idecraft-test".into(),
            port: None,
            extra: vec!["-F".into(), dir.join("ssh_config").display().to_string()],
            program: None,
        };
        let host = Self { target, dir, child };
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Some(host);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
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
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
