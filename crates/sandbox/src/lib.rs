//! Sandbox for agent processes on the desktop (`SPEC-0347`, ROADMAP M19.9).
//!
//! A [`Policy`] names the folders a process may read and write and the hosts it may reach;
//! [`SandboxCommand`] starts a program inside the operating system's sandbox built from it — on
//! macOS Seatbelt through `/usr/bin/sandbox-exec` ([`seatbelt`]), on Linux Landlock and a user and
//! network namespace through the launcher `ferris-idea-sandbox-launch` — and refuses to start it
//! when the sandbox cannot be applied (behaviour 11). There is no switch that turns the sandbox
//! off. Hosts are reached only through the [`proxy::AllowlistProxy`] that the sandbox alone lets
//! the process reach.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::Serialize;

#[cfg(target_os = "linux")]
pub mod linux;
pub mod policy;
pub mod proxy;
pub mod seatbelt;

pub use policy::{HostName, HostRule, Policy, Problem, ValidPolicy};
pub use proxy::{AllowlistProxy, Decision};

/// Exit code of the Linux launcher when it refuses (behaviour 8).
pub const REFUSED_EXIT: i32 = 125;
/// The start of the launcher's refusal line on standard error.
pub const REFUSED_PREFIX: &str = "ferris-idea-sandbox: refused:";
/// File name of the Linux launcher.
pub const LAUNCHER: &str = "ferris-idea-sandbox-launch";
/// The macOS sandbox program.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The sandbox mechanism of this system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mechanism {
    /// Seatbelt through `sandbox-exec`.
    MacosSeatbelt,
    /// Landlock and namespaces through the launcher.
    LinuxLandlock,
    /// No sandbox: every launch is refused.
    Unsupported,
}

impl Mechanism {
    /// The mechanism of the running system.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacosSeatbelt
        } else if cfg!(target_os = "linux") {
            Self::LinuxLandlock
        } else {
            Self::Unsupported
        }
    }
}

/// Whether the mechanism can be used here, and why not (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Availability {
    /// The mechanism.
    pub mechanism: Mechanism,
    /// Whether it is available.
    pub available: bool,
    /// What was checked, or why it is not available.
    pub reason: String,
}

/// Checks the mechanism without starting anything.
pub fn availability() -> Availability {
    let mechanism = Mechanism::current();
    let (available, reason) = match mechanism {
        Mechanism::MacosSeatbelt if Path::new(SANDBOX_EXEC).exists() => {
            (true, format!("{SANDBOX_EXEC} is present"))
        }
        Mechanism::MacosSeatbelt => (false, format!("{SANDBOX_EXEC} is missing")),
        Mechanism::LinuxLandlock => linux_available(),
        Mechanism::Unsupported => (
            false,
            "this operating system has no sandbox for agent processes; launches are refused".into(),
        ),
    };
    Availability {
        mechanism,
        available,
        reason,
    }
}

#[cfg(target_os = "linux")]
fn linux_available() -> (bool, String) {
    match linux::landlock_available() {
        Ok(()) => (
            true,
            "the kernel enforces Landlock ABI 9; the user namespace is checked at launch".into(),
        ),
        Err(e) => (false, e),
    }
}

#[cfg(not(target_os = "linux"))]
fn linux_available() -> (bool, String) {
    (false, "not Linux".into())
}

/// The fixed locations beside the policy on this system (behaviour 5).
pub fn fixed_locations() -> Vec<&'static str> {
    match Mechanism::current() {
        Mechanism::MacosSeatbelt => seatbelt::SYSTEM_READ
            .iter()
            .chain(seatbelt::DEVICES)
            .copied()
            .collect(),
        #[cfg(target_os = "linux")]
        Mechanism::LinuxLandlock => linux::SYSTEM_READ
            .iter()
            .chain(linux::DEVICES)
            .copied()
            .collect(),
        _ => Vec::new(),
    }
}

/// Why a launch did not start.
#[derive(Debug)]
pub enum SandboxError {
    /// The sandbox cannot be applied here or failed; nothing was started.
    Refused(String),
    /// Starting the sandboxed process failed.
    Io(std::io::Error),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(m) => write!(f, "sandbox refused: {m}"),
            Self::Io(e) => write!(f, "cannot start the sandboxed process: {e}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// What the agent host shows about a sandboxed process (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// The mechanism.
    pub mechanism: Mechanism,
    /// The read roots.
    pub read: Vec<PathBuf>,
    /// The write roots.
    pub write: Vec<PathBuf>,
    /// The host entries.
    pub hosts: Vec<String>,
    /// The allowlist proxy's port, when there is one.
    pub proxy_port: Option<u16>,
    /// The proxy's decisions so far.
    pub decisions: Vec<Decision>,
}

/// A program to start inside the sandbox.
pub struct SandboxCommand {
    policy: ValidPolicy,
    program: OsString,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    cwd: Option<PathBuf>,
    launcher: Option<PathBuf>,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

impl SandboxCommand {
    /// A command for `program` under `policy`; the environment starts empty.
    pub fn new(policy: ValidPolicy, program: impl AsRef<OsStr>) -> Self {
        Self {
            policy,
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            launcher: None,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    /// Adds arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }

    /// Sets one environment variable; nothing else is inherited (behaviour 11).
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    /// The working folder.
    pub fn current_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// The Linux launcher (default: `ferris-idea-sandbox-launch` beside the running executable).
    pub fn launcher(mut self, path: impl Into<PathBuf>) -> Self {
        self.launcher = Some(path.into());
        self
    }

    /// Standard input.
    pub fn stdin(mut self, s: Stdio) -> Self {
        self.stdin = Some(s);
        self
    }

    /// Standard output.
    pub fn stdout(mut self, s: Stdio) -> Self {
        self.stdout = Some(s);
        self
    }

    /// Standard error.
    pub fn stderr(mut self, s: Stdio) -> Self {
        self.stderr = Some(s);
        self
    }

    /// Probes the sandbox, then starts the program in it. Any failure to apply the sandbox is
    /// [`SandboxError::Refused`] and nothing is started.
    pub fn spawn(self) -> Result<SandboxedChild, SandboxError> {
        self.spawn_on(Mechanism::current())
    }

    fn spawn_on(self, mechanism: Mechanism) -> Result<SandboxedChild, SandboxError> {
        let proxy = match (mechanism, self.policy.hosts().is_empty()) {
            (_, true) => None,
            (Mechanism::MacosSeatbelt, false) => Some(
                AllowlistProxy::start(self.policy.hosts().to_vec())
                    .map_err(|e| SandboxError::Refused(format!("allowlist proxy: {e}")))?,
            ),
            (Mechanism::LinuxLandlock, false) => {
                return Err(SandboxError::Refused(
                    "a policy with hosts cannot be enforced on Linux in this version (SPEC-0347 behaviour 10)".into(),
                ));
            }
            (Mechanism::Unsupported, false) => None,
        };
        let mut cmd = match mechanism {
            Mechanism::MacosSeatbelt => {
                if !Path::new(SANDBOX_EXEC).exists() {
                    return Err(SandboxError::Refused(format!("{SANDBOX_EXEC} is missing")));
                }
                let profile = seatbelt::profile(&self.policy, proxy.as_ref().map(|p| p.port()));
                probe(
                    Command::new(SANDBOX_EXEC)
                        .arg("-p")
                        .arg(&profile)
                        .arg("--")
                        .arg("/usr/bin/true"),
                )?;
                let mut c = Command::new(SANDBOX_EXEC);
                c.arg("-p").arg(&profile).arg("--").arg(&self.program);
                c
            }
            Mechanism::LinuxLandlock => {
                let launcher = match &self.launcher {
                    Some(l) => l.clone(),
                    None => default_launcher().ok_or_else(|| {
                        SandboxError::Refused(format!(
                            "{LAUNCHER} is not beside the running executable"
                        ))
                    })?,
                };
                if !launcher.is_file() {
                    return Err(SandboxError::Refused(format!(
                        "the launcher {} is missing",
                        launcher.display()
                    )));
                }
                let json = serde_json::to_string(&self.policy.to_policy())
                    .map_err(|e| SandboxError::Refused(e.to_string()))?;
                probe(Command::new(&launcher).arg(&json).arg("--probe"))?;
                let mut c = Command::new(&launcher);
                c.arg(&json).arg("--").arg(&self.program);
                c
            }
            Mechanism::Unsupported => {
                return Err(SandboxError::Refused(availability().reason));
            }
        };
        cmd.args(&self.args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)));
        if let Some(p) = &proxy {
            cmd.envs(p.env());
        }
        if let Some(d) = &self.cwd {
            cmd.current_dir(d);
        }
        for (s, set) in [
            (
                self.stdin,
                Command::stdin as fn(&mut Command, Stdio) -> &mut Command,
            ),
            (self.stdout, Command::stdout),
            (self.stderr, Command::stderr),
        ] {
            if let Some(s) = s {
                set(&mut cmd, s);
            }
        }
        let child = cmd.spawn().map_err(SandboxError::Io)?;
        Ok(SandboxedChild {
            child,
            mechanism,
            policy: self.policy,
            proxy,
        })
    }
}

/// Runs the probe with an empty environment; a failure refuses the launch.
fn probe(cmd: &mut Command) -> Result<(), SandboxError> {
    let out = cmd
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| SandboxError::Refused(format!("the sandbox probe did not start: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let line = err
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default();
    Err(SandboxError::Refused(format!(
        "the sandbox probe failed ({}){}",
        out.status,
        if line.is_empty() {
            String::new()
        } else {
            format!(": {line}")
        }
    )))
}

fn default_launcher() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(LAUNCHER))
}

/// A process running inside the sandbox; the allowlist proxy lives as long as this value.
pub struct SandboxedChild {
    /// The process (on macOS `sandbox-exec`, which became the program; on Linux the launcher,
    /// which executed it).
    pub child: Child,
    mechanism: Mechanism,
    policy: ValidPolicy,
    proxy: Option<AllowlistProxy>,
}

impl SandboxedChild {
    /// What the agent host shows (behaviour 12).
    pub fn summary(&self) -> Summary {
        Summary {
            mechanism: self.mechanism,
            read: self.policy.read().to_vec(),
            write: self.policy.write().to_vec(),
            hosts: self
                .policy
                .hosts()
                .iter()
                .map(ToString::to_string)
                .collect(),
            proxy_port: self.proxy.as_ref().map(AllowlistProxy::port),
            decisions: self
                .proxy
                .as_ref()
                .map(AllowlistProxy::decisions)
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(hosts: &[&str]) -> (tempfile::TempDir, ValidPolicy) {
        let dir = tempfile::tempdir().unwrap();
        let p = Policy {
            write: vec![dir.path().to_path_buf()],
            hosts: hosts.iter().map(|h| h.to_string()).collect(),
            ..Policy::default()
        }
        .validate(None)
        .unwrap();
        (dir, p)
    }

    /// SPEC-0347 AK-6: an unsupported system and a missing launcher refuse; nothing starts.
    #[test]
    fn launches_that_cannot_be_sandboxed_are_refused() {
        let (dir, p) = policy(&[]);
        let marker = dir.path().join("ran");
        let script = format!("touch {}", marker.display());
        let run = |m: Mechanism, launcher: Option<&Path>| {
            let mut c = SandboxCommand::new(p.clone(), "/bin/sh").args(["-c", &script]);
            if let Some(l) = launcher {
                c = c.launcher(l);
            }
            c.spawn_on(m)
        };
        let unsupported = run(Mechanism::Unsupported, None);
        assert!(matches!(unsupported, Err(SandboxError::Refused(_))));
        let missing = run(
            Mechanism::LinuxLandlock,
            Some(&dir.path().join("no-launcher")),
        );
        match missing {
            Err(SandboxError::Refused(m)) => assert!(m.contains("missing"), "{m}"),
            other => panic!("{:?}", other.map(|_| ())),
        }
        // A launcher whose probe fails (here: a program that refuses everything).
        let refusing = dir.path().join("refusing");
        std::fs::write(
            &refusing,
            "#!/bin/sh\necho 'ferris-idea-sandbox: refused: test' >&2\nexit 125\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&refusing, std::fs::Permissions::from_mode(0o755)).unwrap();
            match run(Mechanism::LinuxLandlock, Some(&refusing)) {
                Err(SandboxError::Refused(m)) => {
                    assert!(m.contains("ferris-idea-sandbox: refused: test"), "{m}")
                }
                other => panic!("{:?}", other.map(|_| ())),
            }
        }
        let (_d, with_hosts) = policy(&["api.example.com"]);
        assert!(matches!(
            SandboxCommand::new(with_hosts, "/bin/true").spawn_on(Mechanism::LinuxLandlock),
            Err(SandboxError::Refused(_))
        ));
        assert!(!marker.exists());
    }
}
