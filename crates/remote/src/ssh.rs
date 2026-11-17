//! Running commands on a host through the system `ssh` (behaviours 1–2).
//!
//! The user's `~/.ssh/config`, keys, agent and known hosts work unchanged. IDECraft never asks for
//! a password or passphrase and never accepts an unknown or changed host key: `ssh` runs in batch
//! mode with strict host key checking, and its refusals become [`RemoteError`]s that say what to do.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Why a remote call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum RemoteError {
    /// The host key is not in `known_hosts`; IDECraft does not accept it on its own.
    #[error(
        "{0}: the host key is not known yet. Connect once in a terminal (`ssh {0}`), check the fingerprint and accept it there"
    )]
    HostKeyUnknown(String),
    /// The host key differs from the one in `known_hosts` — possibly another machine.
    #[error(
        "{0}: the host key CHANGED since the last connection. Nothing was sent. Check with the host's owner before updating known_hosts"
    )]
    HostKeyChanged(String),
    /// No key or agent identity was accepted (passwords are never asked for).
    #[error(
        "{0}: login refused. IDECraft uses your SSH keys and agent only; add a key (`ssh-add`) or an IdentityFile in ~/.ssh/config"
    )]
    AuthRequired(String),
    /// Name not found, connection refused or timed out.
    #[error("{0}: cannot reach the host: {1}")]
    Unreachable(String, String),
    /// The command ran past its time limit and was ended.
    #[error("{0}: no answer within {1} s")]
    TimedOut(String, u64),
    /// `ssh` itself could not be started.
    #[error("ssh: {0}")]
    NoSsh(String),
    /// The file changed on the host since it was read; nothing was written.
    #[error("{0} changed on the host since it was opened; nothing was written")]
    Changed(String),
    /// The remote command failed.
    #[error("{0}")]
    Failed(String),
    /// The host offers no hybrid post-quantum key exchange (ADR-0025, PO-STATUS question 29).
    #[error(
        "{0}: the host offers no post-quantum key exchange (OpenSSH 9.0 or later needed there, ML-KEM from 9.9). IDECraft connects only with post-quantum protection (ADR-0025); nothing was sent"
    )]
    NoPqKex(String),
    /// The local `ssh` knows no allowed post-quantum key exchange; the text names the version
    /// needed.
    #[error(
        "the local ssh client offers no post-quantum key exchange; update OpenSSH ({0}). IDECraft does not fall back to a classical key exchange (ADR-0025)"
    )]
    ClientWithoutPq(String),
    /// A jump host on the way offers no hybrid post-quantum key exchange (ADR-0025 v5): the hop,
    /// then the host.
    #[error(
        "{1}: the jump host {0} offers no post-quantum key exchange (OpenSSH 9.0 or later needed there, ML-KEM from 9.9). IDECraft holds every hop to post-quantum protection (ADR-0025); nothing was sent to {1}"
    )]
    HopNoPqKex(String, String),
    /// The SSH configuration starts a `ProxyCommand` of the user's own, which IDECraft cannot hold
    /// to the post-quantum key exchange (ADR-0025 v5); refused unless the setting
    /// `ssh.allowOwnProxyCommand` allows it.
    #[error(
        "{0}: the SSH configuration runs its own ProxyCommand, which IDECraft cannot hold to the post-quantum key exchange (ADR-0025); nothing was started. Use ProxyJump instead (IDECraft protects every hop), or allow it in Settings > Version Control (ssh.allowOwnProxyCommand)"
    )]
    ProxyCommandRefused(String),
}

/// A host to connect to: an alias from `~/.ssh/config` or `user@host`, with optional extra `ssh`
/// arguments (tests use them for a private config and key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshTarget {
    /// Host alias or `[user@]host`.
    pub host: String,
    /// Port, when not the one of the config.
    #[serde(default)]
    pub port: Option<u16>,
    /// Extra arguments before the host, e.g. `-F <config>`.
    #[serde(default)]
    pub extra: Vec<String>,
    /// The `ssh` program (default `ssh` on `PATH`).
    #[serde(default)]
    pub program: Option<String>,
}

impl SshTarget {
    /// A target from what a user typed: an alias or `[user@]host[:port]`, with `extra` `ssh`
    /// arguments.
    pub fn parse(spec: &str, extra: &[String]) -> Self {
        let spec = spec.trim();
        let mut target = match spec.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok() => {
                let mut t = Self::new(host);
                t.port = port.parse().ok();
                t
            }
            _ => Self::new(spec),
        };
        target.extra = extra.to_vec();
        target
    }

    /// A target by alias or `[user@]host`.
    pub fn new(host: &str) -> Self {
        Self {
            host: host.to_owned(),
            port: None,
            extra: Vec::new(),
            program: None,
        }
    }
}

/// What a remote command printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteOutput {
    /// Exit code of the remote command.
    pub exit_code: Option<i32>,
    /// Standard output.
    pub stdout: Vec<u8>,
    /// Standard error, as text.
    pub stderr: String,
}

impl RemoteOutput {
    /// Whether the command exited with 0.
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Quotes `arg` for a POSIX shell on the host.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c))
    {
        return arg.to_owned();
    }
    format!("'{}'", arg.replace('\'', r#"'\''"#))
}

/// Folder of the shared connections, one per host (`ControlMaster`), private to the user.
pub(crate) fn control_dir() -> PathBuf {
    if let Some(dir) = crate::testing::CONTROL_DIR.get() {
        return dir.clone();
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    // Short on purpose: Unix socket paths are limited to about 100 bytes.
    PathBuf::from(format!("/tmp/idecraft-ssh-{user}"))
}

/// Default time limit of one call.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// A connection to one host; calls share one SSH connection for a minute (`ControlPersist`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// The host.
    pub target: SshTarget,
}

/// Turns `ssh`'s own refusal (exit code 255) into a [`RemoteError`].
pub fn classify(host: &str, stderr: &str) -> RemoteError {
    let hop = crate::jump::failed_hop(stderr);
    if let Some((hop, true)) = &hop {
        return RemoteError::ProxyCommandRefused(hop.clone());
    }
    if let Some(refusal) = crate::pq::kex_refusal(host, stderr) {
        return match (refusal, hop) {
            (RemoteError::NoPqKex(_), Some((hop, false))) => {
                RemoteError::HopNoPqKex(hop, host.to_owned())
            }
            (refusal, _) => refusal,
        };
    }
    let lower = stderr.to_lowercase();
    if lower.contains("remote host identification has changed")
        || lower.contains("host key for") && lower.contains("has changed")
    {
        RemoteError::HostKeyChanged(host.to_owned())
    } else if lower.contains("host key verification failed")
        || lower.contains("no host key is known")
    {
        RemoteError::HostKeyUnknown(host.to_owned())
    } else if lower.contains("permission denied") || lower.contains("too many authentication") {
        RemoteError::AuthRequired(host.to_owned())
    } else {
        let line = stderr
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .unwrap_or("ssh failed")
            .to_owned();
        RemoteError::Unreachable(host.to_owned(), line)
    }
}

impl Remote {
    /// A connection to `target`.
    pub fn new(target: SshTarget) -> Self {
        Self { target }
    }

    /// The `ssh` command line up to and including the host, from the one start point
    /// [`crate::SshCommand`]: the client check, the shared options (post-quantum key exchange)
    /// and the route over jump hosts first, then keep-alive, the shared connection, `-t` for a
    /// terminal and the target's own extra arguments.
    pub fn line(&self, purpose: crate::SshPurpose) -> Result<crate::SshLine, RemoteError> {
        self.line_with(purpose, &[])
    }

    /// [`Remote::line`] with the further `ssh` options `options` (before the host).
    fn line_with(
        &self,
        purpose: crate::SshPurpose,
        options: &[&str],
    ) -> Result<crate::SshLine, RemoteError> {
        let dir = control_dir();
        let mut cmd = crate::SshCommand::new(purpose)
            .program(self.program())
            .option("ServerAliveInterval=15");
        if prepare_control_dir(&dir) {
            cmd = cmd
                .option("ControlMaster=auto")
                .option(format!("ControlPath={}/%C", dir.display()))
                .option("ControlPersist=60");
        }
        if purpose == crate::SshPurpose::Terminal {
            cmd = cmd.args(["-t"]);
        }
        if !options.is_empty() {
            cmd = cmd.args(options.iter().copied());
        }
        cmd.port(self.target.port)
            .extra(&self.target.extra)
            .build(&self.target.host)
    }

    /// The `ssh` arguments of [`Remote::line`] for commands on the host.
    pub fn ssh_args(&self) -> Result<Vec<String>, RemoteError> {
        Ok(self.line(crate::SshPurpose::Host)?.args().to_vec())
    }

    /// The program and arguments of an interactive login on the host that starts in `dir`
    /// (for a terminal tab).
    pub fn shell_command(&self, dir: Option<&str>) -> Result<Vec<String>, RemoteError> {
        let mut cmd = self.line(crate::SshPurpose::Terminal)?.argv();
        if let Some(dir) = dir {
            cmd.push(format!("cd {} && exec \"${{SHELL:-sh}}\" -l", quote(dir)));
        }
        Ok(cmd)
    }

    /// The program and arguments of a terminal session that runs `script` with the login shell's
    /// `sh` on the host and then stays in an interactive shell, so the output can be read
    /// (`SPEC-0158` behaviour 9).
    pub fn script_command(&self, script: &str) -> Result<Vec<String>, RemoteError> {
        let mut cmd = self.shell_command(None)?;
        cmd.push(format!(
            "{script}; s=$?; echo; echo \"Process finished with exit code $s\"; exec \"${{SHELL:-sh}}\" -l"
        ));
        Ok(cmd)
    }

    pub(crate) fn program(&self) -> String {
        self.target
            .program
            .clone()
            .unwrap_or_else(|| crate::SSH_PROGRAM.to_owned())
    }

    /// Checks that the local `ssh` offers a post-quantum key exchange (asked once per process).
    pub fn check_client(&self) -> Result<(), RemoteError> {
        crate::SshCommand::check_client(&self.program())
    }

    /// `ssh` with [`Remote::line`]; every connection of this crate starts here.
    pub(crate) fn command(&self) -> Result<Command, RemoteError> {
        Ok(self.line(crate::SshPurpose::Host)?.command())
    }

    /// Runs `script` with `sh -c` on the host, its arguments quoted, `input` on standard input,
    /// within `limit`.
    pub fn sh(
        &self,
        script: &str,
        args: &[&str],
        input: Option<&[u8]>,
        limit: Duration,
    ) -> Result<RemoteOutput, RemoteError> {
        let mut line = format!("sh -c {} sh", quote(script));
        for a in args {
            line.push(' ');
            line.push_str(&quote(a));
        }
        self.raw(&line, input, limit)
    }

    /// Runs program and arguments on the host (each argument quoted, no shell features).
    pub fn exec(
        &self,
        argv: &[String],
        input: Option<&[u8]>,
        limit: Duration,
    ) -> Result<RemoteOutput, RemoteError> {
        if argv.is_empty() {
            return Err(RemoteError::Failed("the command is empty".into()));
        }
        let line = argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        self.raw(&line, input, limit)
    }

    fn raw(
        &self,
        line: &str,
        input: Option<&[u8]>,
        limit: Duration,
    ) -> Result<RemoteOutput, RemoteError> {
        // In tests a session process of the test server that stalled before running the command
        // is ended (`testing::watch_stalled_sessions`). The command never ran, so it is started
        // again over a new connection (the stalled process's parent, the server side of the
        // shared connection, stalls its next sessions too), as long as the call's own time limit
        // lasts: stalls come in spells that outlast one or two new connections (three in a row
        // on 2026-11-17). Outside tests no session is ever ended this way.
        let start = Instant::now();
        loop {
            let stalls = crate::testing::stalls();
            let result = self.raw_once(line, input, limit);
            let ended_by_watch = crate::testing::stalls() > stalls;
            if ended_by_watch
                && result.is_err()
                && start.elapsed() < crate::testing::stretched(limit)
            {
                self.close_shared();
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            return result;
        }
    }

    /// Ends the shared connection to the host (`ssh -O exit`), if there is one; the next call
    /// opens a new one. Waits at most 10 s.
    fn close_shared(&self) {
        let Ok(ssh) = self.line_with(crate::SshPurpose::Host, &["-O", "exit"]) else {
            return;
        };
        let Ok(mut child) = ssh
            .command()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    fn raw_once(
        &self,
        line: &str,
        input: Option<&[u8]>,
        limit: Duration,
    ) -> Result<RemoteOutput, RemoteError> {
        let host = self.target.host.clone();
        let limit = crate::testing::stretched(limit);
        let ssh = self.line(crate::SshPurpose::Host)?;
        let mut child = ssh
            .command()
            .arg(line)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RemoteError::NoSsh(e.to_string()))?;
        let writer = input.map(|bytes| {
            let mut stdin = child.stdin.take();
            let bytes = bytes.to_vec();
            std::thread::spawn(move || {
                if let Some(s) = stdin.as_mut() {
                    let _ = s.write_all(&bytes);
                }
            })
        });
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_end(&mut buf);
            }
            buf
        });
        let err_reader = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(e) = err.as_mut() {
                let _ = e.read_to_string(&mut buf);
            }
            buf
        });
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if start.elapsed() >= limit => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RemoteError::TimedOut(host, limit.as_secs()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(RemoteError::NoSsh(e.to_string())),
            }
        };
        if let Some(w) = writer {
            let _ = w.join();
        }
        let stdout = reader.join().unwrap_or_default();
        let stderr = err_reader.join().unwrap_or_default();
        if status.code() == Some(255) {
            return Err(classify(&host, &ssh.messages(&stderr)));
        }
        Ok(RemoteOutput {
            exit_code: status.code(),
            stdout,
            stderr,
        })
    }
}

/// Sets POSIX permission bits; other systems have none to set.
pub(crate) fn set_mode(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// Without POSIX modes the folder cannot be shown to be private, so no connection is shared.
#[cfg(not(unix))]
pub(crate) fn prepare_control_dir(_dir: &std::path::Path) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn prepare_control_dir(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    // Only the user may reach the shared connections.
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).is_ok()
        && std::fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o077 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_for_the_host_shell() {
        assert_eq!(quote("src/main.rs"), "src/main.rs");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), r#"'it'\''s'"#);
        assert_eq!(quote(""), "''");
        assert_eq!(quote("$(rm -rf ~)"), "'$(rm -rf ~)'");
        assert_eq!(quote("-n"), "-n");
    }

    #[test]
    fn ssh_refusals_say_what_to_do() {
        let changed = "@@@@@@@\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\nHost key verification failed.\n";
        assert_eq!(
            classify("example-host", changed),
            RemoteError::HostKeyChanged("example-host".into())
        );
        assert_eq!(
            classify(
                "h",
                "No ED25519 host key is known for h and you have requested strict checking.\nHost key verification failed.\n"
            ),
            RemoteError::HostKeyUnknown("h".into())
        );
        assert_eq!(
            classify("h", "me@h: Permission denied (publickey).\n"),
            RemoteError::AuthRequired("h".into())
        );
        assert_eq!(
            classify("h", "ssh: connect to host h port 22: Connection refused\n"),
            RemoteError::Unreachable(
                "h".into(),
                "ssh: connect to host h port 22: Connection refused".into()
            )
        );
        assert!(
            RemoteError::HostKeyChanged("s".into())
                .to_string()
                .contains("Nothing was sent")
        );
    }

    #[test]
    fn typed_hosts_with_user_and_port() {
        let t = SshTarget::parse(" me@dev.example:2222 ", &["-F".into(), "c".into()]);
        assert_eq!((t.host.as_str(), t.port), ("me@dev.example", Some(2222)));
        assert_eq!(t.extra, ["-F", "c"]);
        assert_eq!(SshTarget::parse("dev", &[]).port, None);
        assert_eq!(SshTarget::parse("dev:x", &[]).host, "dev:x");
    }

    #[test]
    fn batch_mode_strict_keys_and_terminal() {
        let mut t = SshTarget::new("idecraft-dev.invalid");
        t.port = Some(2222);
        let r = Remote::new(t);
        let Ok(args) = r.ssh_args() else {
            eprintln!("skipped: no ssh with a post-quantum key exchange");
            return;
        };
        assert!(args.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(
            args.windows(2)
                .any(|w| w == ["-o", "StrictHostKeyChecking=yes"])
        );
        assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
        assert_eq!(&args[args.len() - 2..], ["--", "idecraft-dev.invalid"]);
        assert!(args.windows(2).any(|w| w == ["-o", "ProxyCommand=none"]));
        let shell = r.shell_command(Some("/srv/my app")).unwrap();
        assert_eq!(shell[0], "ssh");
        let t_at = shell.iter().position(|a| a == "-t").unwrap();
        let host_at = shell
            .iter()
            .position(|a| a == "idecraft-dev.invalid")
            .unwrap();
        assert!(t_at < host_at);
        assert_eq!(
            shell.last().unwrap(),
            "cd '/srv/my app' && exec \"${SHELL:-sh}\" -l"
        );
    }
}
