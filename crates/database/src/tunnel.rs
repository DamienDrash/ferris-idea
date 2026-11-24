//! SSH tunnels to database servers (`SPEC-0108` v4 behaviour 8, ADR-0025).
//!
//! The system `ssh` forwards a port on `127.0.0.1` to the database host and port as the SSH host
//! sees them, exactly as SPEC-0121 reaches hosts: batch mode (no password or passphrase prompt),
//! strict host key checking, the user's keys, agent and `~/.ssh/config`. The tunnel counts as
//! ready only when `ssh` itself reports that it listens on the chosen port, so a process of another
//! user that grabs the port first is never mistaken for the tunnel: `ssh` then fails to bind and
//! exits (`ExitOnForwardFailure`). The tunnel ends when the value is dropped.

use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub use ferris_idea_remote::RemoteError as TunnelError;

use crate::profile::SshTunnel;

/// The loopback address every tunnel listens on; never another interface (ADR-0025).
pub const TUNNEL_BIND: Ipv4Addr = Ipv4Addr::LOCALHOST;

/// A running SSH tunnel.
pub struct Tunnel {
    child: Child,
    local: SocketAddr,
    host: String,
}

impl std::fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tunnel")
            .field("local", &self.local)
            .field("host", &self.host)
            .finish()
    }
}

/// `host` in the form `ssh -L` takes: IPv6 addresses in brackets.
fn forward_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

/// The `ssh` command line of a tunnel from `127.0.0.1:local_port` to `target_host:target_port`
/// through `ssh`; `extra` goes before the host (tests pass `-F <config>`). Nothing secret is ever
/// part of it: there is no password option in `ssh` and Ferris IDEA adds none. It comes from the one
/// start point `ferris_idea_remote::SshCommand` for the client `program`: the client check, the
/// shared options first (batch mode, strict host keys, connect timeout, post-quantum key exchange,
/// SPEC-0121, ADR-0025) and every jump host held to the same options.
pub fn tunnel_line(
    program: &str,
    ssh: &SshTunnel,
    target_host: &str,
    target_port: u16,
    local_port: u16,
    extra: &[String],
) -> Result<ferris_idea_remote::SshLine, TunnelError> {
    ferris_idea_remote::SshCommand::new(ferris_idea_remote::SshPurpose::Tunnel)
        .program(program)
        .args([
            "-v",
            "-N",
            "-T",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveCountMax=3",
            // One connection of its own: no shared master that outlives the tunnel.
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            // Least privilege towards the SSH host.
            "-o",
            "ForwardAgent=no",
            "-o",
            "ForwardX11=no",
            "-o",
            "PermitLocalCommand=no",
            "-L",
        ])
        .args([format!(
            "{TUNNEL_BIND}:{local_port}:{}:{target_port}",
            forward_host(target_host)
        )])
        .port(ssh.port)
        .login(ssh.user.as_deref())
        .extra(extra)
        .build(&ssh.host)
}

/// A free port on `127.0.0.1` at this moment.
/// Whether `line` of `ssh -v` reports the channel of a local forward's listening socket, which
/// OpenSSH creates after `listen` (`channel 0: new port-listener [port listener] …`).
pub fn is_listener_channel(line: &str) -> bool {
    line.contains("port-listener") || line.contains("[port listener]")
}

fn free_port() -> Result<u16, TunnelError> {
    TcpListener::bind((TUNNEL_BIND, 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| TunnelError::Failed(format!("no free local port: {e}")))
}

/// Extra `ssh` arguments for tests: in builds with debug assertions only, the environment
/// variable `FERRIS_IDEA_TEST_SSH_CONFIG` names an `ssh` config file used instead of the user's.
pub fn test_extra_args() -> Vec<String> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    ferris_idea_core::product_var("FERRIS_IDEA_TEST_SSH_CONFIG")
        .ok()
        .filter(|f| !f.is_empty())
        .map(|f| vec!["-F".to_owned(), f])
        .unwrap_or_default()
}

/// What `ssh` said, without its debug lines, for classifying a refusal.
fn refusal_text(lines: &[String]) -> String {
    lines
        .iter()
        .filter(|l| !l.starts_with("debug"))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

impl Tunnel {
    /// Opens a tunnel through `ssh` to `target_host:target_port` and waits until `ssh` listens,
    /// at most `limit`.
    pub fn open(
        ssh: &SshTunnel,
        target_host: &str,
        target_port: u16,
        limit: Duration,
    ) -> Result<Self, TunnelError> {
        Self::open_with(
            ssh,
            target_host,
            target_port,
            limit,
            &test_extra_args(),
            ferris_idea_remote::SSH_PROGRAM,
        )
    }

    /// [`Tunnel::open`] with extra `ssh` arguments and another `ssh` program (tests).
    pub fn open_with(
        ssh: &SshTunnel,
        target_host: &str,
        target_port: u16,
        limit: Duration,
        extra: &[String],
        program: &str,
    ) -> Result<Self, TunnelError> {
        let port = free_port()?;
        // A local `ssh` without a post-quantum key exchange is refused before anything starts.
        let line = tunnel_line(program, ssh, target_host, target_port, port, extra)?;
        let mut child = line
            .command()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TunnelError::NoSsh(e.to_string()))?;
        let stderr = child.stderr.take();
        let (lines_tx, lines) = mpsc::channel::<String>();
        // Reads ssh's messages for as long as it runs, so its pipe never fills up.
        std::thread::Builder::new()
            .name("ssh-tunnel-log".into())
            .spawn(move || {
                if let Some(e) = stderr {
                    for line in BufReader::new(e).lines().map_while(Result::ok) {
                        let _ = lines_tx.send(line);
                    }
                }
            })
            .map_err(|e| TunnelError::Failed(e.to_string()))?;
        // OpenSSH names the forward before it binds and listens; it is ready only when it reports
        // the listening channel it created after `listen` (`SPEC-0108` v7 AK-18: a connection right
        // after the first line was refused under load).
        let listening = format!("Local forwarding listening on {TUNNEL_BIND} port {port}");
        let host = ssh.host.clone();
        let mut seen = Vec::new();
        let mut named = false;
        let started = Instant::now();
        loop {
            match lines.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => {
                    if line.contains(&listening) {
                        named = true;
                    } else if named && is_listener_channel(&line) {
                        return Ok(Self {
                            child,
                            local: SocketAddr::from((TUNNEL_BIND, port)),
                            host,
                        });
                    }
                    seen.push(line);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // ssh closed its error output: it has ended.
                    let _ = child.wait();
                    return Err(Self::refused(&host, &seen));
                }
            }
            if let Ok(Some(_)) = child.try_wait() {
                // Collect what is still in the pipe before judging.
                while let Ok(line) = lines.recv_timeout(Duration::from_millis(100)) {
                    seen.push(line);
                }
                return Err(Self::refused(&host, &seen));
            }
            if started.elapsed() >= limit {
                let _ = child.kill();
                let _ = child.wait();
                return Err(TunnelError::TimedOut(host, limit.as_secs()));
            }
        }
    }

    fn refused(host: &str, lines: &[String]) -> TunnelError {
        let text = refusal_text(lines);
        let lower = text.to_lowercase();
        if lower.contains("address already in use") || lower.contains("cannot listen") {
            return TunnelError::Failed(format!(
                "{host}: the local tunnel port was taken by another program; nothing was sent"
            ));
        }
        ferris_idea_remote::classify(host, &text)
    }

    /// What the connection dialog shows when the tunnel could not be opened: the refusals of the
    /// post-quantum rule (ADR-0025) say what to update, the others keep `ssh`'s classified text.
    pub fn explain(err: &TunnelError) -> String {
        match err {
            TunnelError::NoPqKex(host) => format!(
                "SSH host `{host}` offers no post-quantum key exchange. Ferris IDEA opens SSH tunnels only with post-quantum protection (ADR-0025): update OpenSSH on the SSH host to 9.0 or later (ML-KEM from 9.9). Nothing was sent to the database."
            ),
            TunnelError::ClientWithoutPq(needs) => format!(
                "The ssh client on this computer offers no post-quantum key exchange. Update OpenSSH here ({needs}); Ferris IDEA does not fall back to a classical key exchange (ADR-0025). Nothing was sent."
            ),
            other => other.to_string(),
        }
    }

    /// The local address the tunnel listens on (always `127.0.0.1`).
    pub fn local(&self) -> SocketAddr {
        self.local
    }

    /// The SSH host.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Whether `ssh` still runs.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    /// SPEC-0108 v7 AK-18: the line that says the forward listens, not the one that names it.
    #[test]
    fn the_listening_channel_marks_the_tunnel_ready() {
        use super::is_listener_channel;
        assert!(is_listener_channel(
            "debug1: channel 0: new port-listener [port listener] (inactive timeout: 0)"
        ));
        assert!(is_listener_channel(
            "debug1: channel 0: new [port listener]"
        ));
        assert!(!is_listener_channel(
            "debug1: Local forwarding listening on 127.0.0.1 port 42041."
        ));
        assert!(!is_listener_channel(
            "debug1: Local connections to 127.0.0.1:42041 forwarded to remote address 127.0.0.1:40305"
        ));
    }

    use super::*;

    fn spec() -> SshTunnel {
        SshTunnel {
            host: "bastion".into(),
            port: Some(2222),
            user: Some("ops".into()),
        }
    }

    /// SPEC-0108 v4 AK-11: batch mode, strict host keys, the forward bound to 127.0.0.1 only,
    /// `--` before the host, no secret anywhere.
    #[test]
    fn arguments_are_strict_and_loopback_only() {
        let Ok(line) = tunnel_line("ssh", &spec(), "db.internal", 5432, 40001, &[]) else {
            eprintln!("skipped: no ssh with a post-quantum key exchange");
            return;
        };
        let args = line.args();
        let text = args.join(" ");
        for must in [
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "ExitOnForwardFailure=yes",
            "ForwardAgent=no",
            "ControlMaster=no",
        ] {
            assert!(args.iter().any(|a| a == must), "{must} missing: {text}");
        }
        assert!(
            args.iter().any(|a| a.starts_with("KexAlgorithms=")),
            "post-quantum key exchange (ADR-0025): {text}"
        );
        assert_eq!(
            args[..8],
            ferris_idea_remote::testing::shared_options("ssh")[..8],
            "shared options first"
        );
        assert!(
            args.windows(2).any(|w| w == ["-o", "ProxyCommand=none"]),
            "route pinned: {text}"
        );
        let at = args.iter().position(|a| a == "-L").unwrap();
        assert_eq!(args[at + 1], "127.0.0.1:40001:db.internal:5432");
        assert_eq!(&args[args.len() - 2..], ["--", "bastion"]);
        assert!(text.contains("-p 2222") && text.contains("-l ops"));
        assert!(!text.to_lowercase().contains("pass"));
        let v6 = tunnel_line("ssh", &spec(), "::1", 3306, 40002, &[]).unwrap();
        assert!(v6.args().iter().any(|a| a == "127.0.0.1:40002:[::1]:3306"));
    }

    #[test]
    fn refusals_are_named() {
        let lines = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(matches!(
            Tunnel::refused("h", &lines(&["debug1: x", "Host key verification failed."])),
            TunnelError::HostKeyUnknown(_)
        ));
        assert!(matches!(
            Tunnel::refused("h", &lines(&["ops@h: Permission denied (publickey)."])),
            TunnelError::AuthRequired(_)
        ));
        let taken = Tunnel::refused(
            "h",
            &lines(&["bind [127.0.0.1]:40001: Address already in use"]),
        );
        assert!(taken.to_string().contains("taken by another program"));
    }

    /// SPEC-0108 v4 AK-11: a host or client without a post-quantum key exchange is named in plain
    /// words with what to update (ADR-0025, question 29).
    #[test]
    fn post_quantum_refusals_are_explained() {
        let host = Tunnel::explain(&TunnelError::NoPqKex("bastion".into()));
        assert!(host.contains("`bastion` offers no post-quantum key exchange"));
        assert!(host.contains("OpenSSH on the SSH host"));
        let refused = Tunnel::refused(
            "bastion",
            &["Unable to negotiate with 10.0.0.5 port 22: no matching key exchange method found. Their offer: curve25519-sha256".into()],
        );
        assert!(matches!(refused, TunnelError::NoPqKex(_)), "{refused}");
        let client = Tunnel::explain(&TunnelError::ClientWithoutPq("OpenSSH 9.0".into()));
        assert!(client.contains("ssh client on this computer"));
    }

    #[test]
    fn missing_ssh_program_is_reported() {
        let err = Tunnel::open_with(
            &spec(),
            "db",
            5432,
            Duration::from_secs(1),
            &[],
            "/nonexistent/ssh",
        )
        .unwrap_err();
        assert!(matches!(err, TunnelError::NoSsh(_)), "{err}");
    }
}
