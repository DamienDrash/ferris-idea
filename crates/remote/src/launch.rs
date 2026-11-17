//! The one start point of the system `ssh` (ADR-0025 v5 section 1a).
//!
//! Every SSH path of IDECraft — remote host commands and files, the backend and `idecraft serve`,
//! the SSH terminal, database tunnels, Docker over SSH, the Docker Compose CLI and Git — gets its
//! `ssh` from [`SshCommand`]: the program, the client check (a local `ssh` without a post-quantum
//! key exchange is refused before anything starts), the shared options first, the route over jump
//! hosts (each hop with the same options, `crate::jump`), then the caller's own options. Git and
//! the Compose CLI start `ssh` themselves and get it through [`SshCommand::git_ssh_command`] and
//! [`SshCommand::stand_in_dir`]. This file holds the only `Command::new` of `ssh` in the product;
//! a workspace scan (`crates/remote/tests/it/pq_kex.rs`) keeps it that way, and the shared options
//! themselves are private to this crate.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use crate::ssh::RemoteError;

/// The default `ssh` program: the one on `PATH`.
pub const SSH_PROGRAM: &str = "ssh";

/// What an `ssh` is started for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshPurpose {
    /// Commands and files on a remote host (`SPEC-0121`).
    Host,
    /// The backend and `idecraft serve` (`SPEC-0158`, `SPEC-0317`).
    Serve,
    /// An interactive login or a run in a terminal tab.
    Terminal,
    /// A database tunnel (`SPEC-0108`).
    Tunnel,
    /// Docker over SSH (`SPEC-0243`).
    Docker,
    /// `ssh -G`: what a connection would use. Nothing connects, so neither the client check nor
    /// the route applies.
    Describe,
}

impl SshPurpose {
    fn connects(self) -> bool {
        self != Self::Describe
    }
}

/// An `ssh` command line being built; [`SshCommand::build`] checks the client and resolves the
/// route.
#[derive(Debug, Clone)]
pub struct SshCommand {
    purpose: SshPurpose,
    program: String,
    options: Vec<String>,
    port: Option<u16>,
    login: Option<String>,
    extra: Vec<String>,
    remote: Vec<String>,
}

/// A finished `ssh` command line: the program and its arguments, the shared options first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshLine {
    /// The `ssh` program.
    program: String,
    args: Vec<String>,
    /// Where the hop stand-ins report a failing hop (`crate::jump::HOP_LOG_ENV`), for a route
    /// over jump hosts.
    hop_log: Option<PathBuf>,
}

impl SshCommand {
    /// An `ssh` for `purpose`, with the `ssh` on `PATH`.
    pub fn new(purpose: SshPurpose) -> Self {
        Self {
            purpose,
            program: SSH_PROGRAM.to_owned(),
            options: Vec::new(),
            port: None,
            login: None,
            extra: Vec::new(),
            remote: Vec::new(),
        }
    }

    /// Another `ssh` program (an engine's or a test's own).
    pub fn program(mut self, program: impl Into<String>) -> Self {
        self.program = program.into();
        self
    }

    /// `-o <option>` after the shared ones (which win where both set the same option).
    pub fn option(mut self, option: impl Into<String>) -> Self {
        self.options.push("-o".into());
        self.options.push(option.into());
        self
    }

    /// Plain arguments after the shared options, such as `-t`, `-v` or `-L <spec>`.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.options.extend(args.into_iter().map(Into::into));
        self
    }

    /// The port, when not the one of the configuration.
    pub fn port(mut self, port: Option<u16>) -> Self {
        self.port = port;
        self
    }

    /// The login name, when not the one of the configuration.
    pub fn login(mut self, login: Option<&str>) -> Self {
        self.login = login.map(str::to_owned);
        self
    }

    /// Extra arguments before the destination (`-F <config>` and the like); a `-J` among them is
    /// replaced by IDECraft's own route.
    pub fn extra(mut self, extra: &[String]) -> Self {
        self.extra.extend(extra.iter().cloned());
        self
    }

    /// The command to run on the host, after the destination.
    pub fn remote<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.remote.extend(args.into_iter().map(Into::into));
        self
    }

    /// The command line to `destination`: the client check, the shared options, the route (every
    /// jump host with the same options; `ProxyCommand=none` without one; a `ProxyCommand` of the
    /// user's own refused unless allowed), the caller's options, port, login, extra arguments,
    /// `--`, the destination and the remote command.
    pub fn build(self, destination: &str) -> Result<SshLine, RemoteError> {
        let connects = self.purpose.connects();
        if connects {
            crate::pq::check_client(&self.program)?;
        }
        let mut args = crate::pq::ssh_options(&self.program);
        let mut conf = Vec::new();
        let mut hop_log = None;
        if let Some(p) = self.port {
            conf.extend(["-p".to_owned(), p.to_string()]);
        }
        if let Some(l) = &self.login {
            conf.extend(["-l".to_owned(), l.clone()]);
        }
        let extra = if connects {
            let mut probe = conf.clone();
            probe.extend(self.extra.iter().cloned());
            let route = crate::jump::route(&self.program, &args, &probe, destination);
            if matches!(route, crate::jump::Route::Jump(_)) {
                hop_log = Some(crate::jump::hop_log_path());
            }
            args.extend(crate::jump::route_options(
                &route,
                &self.program,
                crate::jump::config_file(&self.extra).as_deref(),
                destination,
            )?);
            crate::jump::without_jump(&self.extra)
        } else {
            self.extra
        };
        args.extend(self.options);
        args.extend(conf);
        args.extend(extra);
        args.push("--".into());
        args.push(destination.to_owned());
        args.extend(self.remote);
        Ok(SshLine {
            program: self.program,
            args,
            hop_log,
        })
    }

    /// Checks that the local `program` offers a post-quantum key exchange (asked once per
    /// process).
    pub fn check_client(program: &str) -> Result<(), RemoteError> {
        crate::pq::check_client(program).map(|_| ())
    }

    /// `GIT_SSH_COMMAND` for Git over SSH: a private stand-in that applies the shared options,
    /// the route over jump hosts and the one exception for Azure DevOps (`crate::pq_git`). It
    /// replaces a `GIT_SSH_COMMAND` or `core.sshCommand` of the user.
    pub fn git_ssh_command() -> String {
        crate::pq_git::git_ssh_command()
    }

    /// The folder to put first on `PATH` for programs that start `ssh` from `PATH` themselves (the
    /// Docker Compose CLI with an `ssh://` engine): its `ssh` applies the shared options and the
    /// route over jump hosts.
    pub fn stand_in_dir() -> Result<PathBuf, RemoteError> {
        crate::pq::ssh_shim_dir()
    }
}

impl SshLine {
    /// The `ssh` program.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The arguments, shared options first.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Program and arguments, for a terminal that starts the process itself.
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect()
    }

    /// The process to start.
    pub fn command(&self) -> Command {
        crate::testing::check_ssh_args(&self.args);
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);
        if let Some(log) = &self.hop_log {
            cmd.env(crate::jump::HOP_LOG_ENV, log);
        }
        cmd
    }

    /// `stderr` of a failed `ssh` with what the hop stand-ins reported (read once and removed):
    /// with a shared connection `ssh` hides a hop's messages, and only these name the hop that
    /// refused. Classify the result with [`crate::classify`].
    pub fn messages(&self, stderr: &str) -> String {
        let Some(log) = &self.hop_log else {
            return stderr.to_owned();
        };
        let hops = std::fs::read_to_string(log).unwrap_or_default();
        let _ = std::fs::remove_file(log);
        if hops.is_empty() {
            stderr.to_owned()
        } else {
            format!("{hops}\n{stderr}")
        }
    }
}

/// Runs the `ssh` program `program` with `args` for a query that opens no connection (`-Q kex`,
/// `-G`): no input,
/// its messages discarded.
pub(crate) fn output(program: &str, args: &[String]) -> std::io::Result<Output> {
    if args.iter().any(|a| a == "-G") {
        crate::testing::check_ssh_args(args);
    }
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describing_needs_no_client_and_no_route() {
        let line = SshCommand::new(SshPurpose::Describe)
            .program("/no/such/ssh")
            .args(["-G"])
            .extra(&["-J".into(), "a".into()])
            .build("box")
            .unwrap();
        let a = line.args();
        assert!(a.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(!a.iter().any(|x| x.starts_with("ProxyCommand=")));
        assert_eq!(&a[a.len() - 4..], ["-J", "a", "--", "box"]);
    }

    #[test]
    fn a_missing_client_is_refused_before_anything_starts() {
        let e = SshCommand::new(SshPurpose::Host)
            .program("/no/such/ssh")
            .build("box")
            .unwrap_err();
        assert!(matches!(e, RemoteError::NoSsh(_)), "{e:?}");
    }

    #[test]
    fn the_shared_options_come_first_and_the_route_is_pinned() {
        let Ok(line) = SshCommand::new(SshPurpose::Tunnel)
            .args(["-N", "-L", "127.0.0.1:1:db:5432"])
            .port(Some(2222))
            .login(Some("ops"))
            .remote(["true"])
            .build("idecraft-no-such-host.invalid")
        else {
            eprintln!("skipped: no ssh with a post-quantum key exchange");
            return;
        };
        let a = line.args();
        assert_eq!(&a[..2], ["-o", "BatchMode=yes"]);
        let kex = a
            .iter()
            .position(|x| x.starts_with("KexAlgorithms="))
            .unwrap();
        let pin = a.iter().position(|x| x == "ProxyCommand=none").unwrap();
        let own = a.iter().position(|x| x == "-N").unwrap();
        assert!(kex < pin && pin < own, "{a:?}");
        assert_eq!(
            &a[a.len() - 3..],
            ["--", "idecraft-no-such-host.invalid", "true"]
        );
        assert_eq!(line.argv()[0], "ssh");
    }
}
