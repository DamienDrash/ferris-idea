//! How a host is reached (`SPEC-0321` behaviours 9–12): one host string names SSH, Docker or
//! Podman `exec`, Kubernetes `exec`, WSL or a paired channel. Parts of a host string are checked
//! before any command is built; no part can be read as an option by the engine's program.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ssh::{Remote, RemoteError, RemoteOutput, SshTarget, quote};

/// A container engine with the `docker` command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Docker.
    Docker,
    /// Podman.
    Podman,
}

impl Engine {
    fn program(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
        }
    }
    fn scheme(self) -> &'static str {
        self.program()
    }
}

/// A host as the user names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSpec {
    /// An SSH host, as typed.
    Ssh(String),
    /// A container.
    Container {
        /// The engine.
        engine: Engine,
        /// Container name or id.
        name: String,
    },
    /// A Kubernetes pod.
    Pod {
        /// `--context`, if given.
        context: Option<String>,
        /// Namespace.
        namespace: String,
        /// Pod.
        pod: String,
        /// `-c`, if given.
        container: Option<String>,
    },
    /// A WSL distribution.
    Wsl {
        /// Distribution.
        distribution: String,
    },
    /// A host reached over the paired channel.
    Paired {
        /// `host:port`.
        address: String,
    },
}

/// Whether `part` of a host string is safe to give to a program as one argument.
pub fn valid_part(part: &str, extra: &[char]) -> bool {
    !part.is_empty()
        && !part.starts_with('-')
        && part.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') || extra.contains(&c)
        })
}

fn checked(part: &str, what: &str, extra: &[char]) -> Result<String, RemoteError> {
    if valid_part(part, extra) {
        Ok(part.to_owned())
    } else {
        Err(RemoteError::Failed(format!(
            "{what} {part:?} is not valid: it may hold letters, digits, `.`, `_`, `-` and cannot start with `-`"
        )))
    }
}

impl HostSpec {
    /// Parses a host string. An SSH host keeps its existing syntax.
    pub fn parse(spec: &str) -> Result<Self, RemoteError> {
        let spec = spec.trim();
        let Some((scheme, rest)) = spec.split_once("://") else {
            return Ok(Self::Ssh(spec.to_owned()));
        };
        match scheme {
            "docker" | "podman" => {
                let engine = if scheme == "docker" {
                    Engine::Docker
                } else {
                    Engine::Podman
                };
                Ok(Self::Container {
                    engine,
                    name: checked(rest, "container", &[])?,
                })
            }
            "k8s" => {
                let parts: Vec<&str> = rest.split('/').collect();
                let (context, namespace, pod, container) = match parts.as_slice() {
                    [ns, pod] => (None, *ns, *pod, None),
                    [ctx, ns, pod] if !ctx.is_empty() && looks_like_context(ctx) => {
                        (Some(*ctx), *ns, *pod, None)
                    }
                    [ns, pod, c] => (None, *ns, *pod, Some(*c)),
                    [ctx, ns, pod, c] => (Some(*ctx), *ns, *pod, Some(*c)),
                    _ => {
                        return Err(RemoteError::Failed(format!(
                            "{spec:?} is not k8s://[<context>/]<namespace>/<pod>[/<container>]"
                        )));
                    }
                };
                Ok(Self::Pod {
                    context: context.map(|c| checked(c, "context", &[':'])).transpose()?,
                    namespace: checked(namespace, "namespace", &[])?,
                    pod: checked(pod, "pod", &[])?,
                    container: container
                        .map(|c| checked(c, "container", &[]))
                        .transpose()?,
                })
            }
            "wsl" => Ok(Self::Wsl {
                distribution: checked(rest, "distribution", &[])?,
            }),
            "paired" => {
                let address = rest.split('#').next().unwrap_or_default();
                // `<alias>[:port]@ssh` goes through an SSH tunnel (`SPEC-0324` behaviour 6).
                let (bare, suffix) = match address.strip_suffix("@ssh") {
                    Some(b) => (b, "@ssh"),
                    None => (address, ""),
                };
                let bare = checked(bare, "address", &[':', '[', ']'])?;
                let full = if bare.rsplit(':').next().is_some_and(|p| {
                    !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) && bare.contains(':')
                }) {
                    bare
                } else {
                    format!("{bare}:{}", ferris_idea_pairing::DEFAULT_PORT)
                };
                Ok(Self::Paired {
                    address: format!("{full}{suffix}"),
                })
            }
            other => Err(RemoteError::Failed(format!(
                "unknown host type {other}://; use an SSH host, docker://, podman://, k8s://, wsl:// or paired://"
            ))),
        }
    }

    /// The one-time pairing code after `#`, if the string carries one.
    pub fn pairing_code(spec: &str) -> Option<String> {
        let (_, rest) = spec.trim().split_once("://")?;
        rest.split_once('#')
            .map(|(_, c)| c.trim().to_owned())
            .filter(|c| !c.is_empty())
    }

    /// The label shown as the host, unchanged by the pairing code.
    pub fn label(&self) -> String {
        match self {
            Self::Ssh(s) => s.clone(),
            Self::Container { engine, name } => format!("{}://{name}", engine.scheme()),
            Self::Pod {
                context,
                namespace,
                pod,
                container,
            } => {
                let mut s = String::from("k8s://");
                if let Some(c) = context {
                    s.push_str(c);
                    s.push('/');
                }
                s.push_str(&format!("{namespace}/{pod}"));
                if let Some(c) = container {
                    s.push('/');
                    s.push_str(c);
                }
                s
            }
            Self::Wsl { distribution } => format!("wsl://{distribution}"),
            Self::Paired { address } => format!("paired://{address}"),
        }
    }

    /// Whether this is an SSH host.
    pub fn is_ssh(&self) -> bool {
        matches!(self, Self::Ssh(_))
    }
}

/// A `k8s://` first segment that can only be a context (it holds `:`, like an EKS ARN).
fn looks_like_context(part: &str) -> bool {
    part.contains(':')
}

/// A route of `exec` type: Docker, Podman, Kubernetes or WSL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecRoute {
    spec: HostSpec,
    program: Option<PathBuf>,
}

impl ExecRoute {
    /// A route for `spec`, or `None` for SSH and paired hosts.
    pub fn new(spec: HostSpec) -> Option<Self> {
        match spec {
            HostSpec::Container { .. } | HostSpec::Pod { .. } | HostSpec::Wsl { .. } => {
                Some(Self {
                    spec,
                    program: None,
                })
            }
            _ => None,
        }
    }

    /// Another engine program (a test's or an installation's own).
    pub fn with_program(mut self, program: PathBuf) -> Self {
        self.program = Some(program);
        self
    }

    fn base(&self, default: &str) -> Command {
        Command::new(self.program.as_deref().unwrap_or(Path::new(default)))
    }

    /// The local command that runs one `sh` line on the host (the line goes after it).
    pub fn command(&self) -> Result<Command, RemoteError> {
        Ok(match &self.spec {
            HostSpec::Container { engine, name } => {
                let mut c = self.base(engine.program());
                c.args(["exec", "-i"]).arg(name).args(["sh", "-c"]);
                c
            }
            HostSpec::Pod {
                context,
                namespace,
                pod,
                container,
            } => {
                let mut c = self.base("kubectl");
                if let Some(ctx) = context {
                    c.arg("--context").arg(ctx);
                }
                c.args(["exec", "-i", "-n"]).arg(namespace).arg(pod);
                if let Some(x) = container {
                    c.arg("-c").arg(x);
                }
                c.args(["--", "sh", "-c"]);
                c
            }
            HostSpec::Wsl { distribution } => {
                if !cfg!(windows) && self.program.is_none() {
                    return Err(RemoteError::Failed(
                        "wsl:// hosts need a window running on Windows; this one is not".into(),
                    ));
                }
                let mut c = self.base("wsl.exe");
                c.arg("-d").arg(distribution).args(["--exec", "sh", "-c"]);
                c
            }
            _ => {
                return Err(RemoteError::Failed("not an exec route".into()));
            }
        })
    }

    fn label(&self) -> String {
        self.spec.label()
    }

    fn refusal(&self, stderr: &str) -> RemoteError {
        let label = self.label();
        let text = stderr.trim();
        let lower = text.to_lowercase();
        let what = if lower.contains("no such container") || lower.contains("not found") {
            format!("{label}: the container or pod does not exist or is not running: {text}")
        } else if lower.contains("permission denied") || lower.contains("forbidden") {
            format!("{label}: access was refused: {text}")
        } else if lower.contains("not running") {
            format!("{label}: the container is not running: {text}")
        } else if text.is_empty() {
            format!("{label}: the command failed")
        } else {
            format!("{label}: {text}")
        };
        RemoteError::Failed(what)
    }
}

/// Any route that runs a shell line on the host.
#[derive(Debug, Clone)]
pub enum Route {
    /// SSH.
    Ssh(Remote),
    /// `exec` of an engine.
    Exec(ExecRoute),
}

impl Route {
    /// The route for a non-paired host string; `extra` are the SSH arguments of the window.
    pub fn for_spec(spec: &HostSpec, extra: &[String]) -> Result<Self, RemoteError> {
        match spec {
            HostSpec::Ssh(s) => Ok(Self::Ssh(Remote::new(SshTarget::parse(s, extra)))),
            HostSpec::Paired { .. } => Err(RemoteError::Failed(
                "a paired host is not reached through a route".into(),
            )),
            other => ExecRoute::new(other.clone())
                .map(Self::Exec)
                .ok_or_else(|| RemoteError::Failed("not an exec route".into())),
        }
    }

    /// The host label.
    pub fn label(&self) -> String {
        match self {
            Self::Ssh(r) => r.target.host.clone(),
            Self::Exec(e) => e.label(),
        }
    }

    /// A command that runs one line on the host (the line goes after it).
    pub fn command(&self) -> Result<Command, RemoteError> {
        match self {
            Self::Ssh(r) => r.command(),
            Self::Exec(e) => e.command(),
        }
    }

    /// Turns what the channel wrote to standard error into a named refusal.
    pub fn refusal(&self, stderr: &str) -> RemoteError {
        match self {
            Self::Ssh(r) => crate::ssh::classify(&r.target.host, stderr),
            Self::Exec(e) => e.refusal(stderr),
        }
    }

    /// Runs `script` with `args` through `sh`, feeding `input`.
    pub fn sh(
        &self,
        script: &str,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<RemoteOutput, RemoteError> {
        self.sh_within(script, args, input, crate::ssh::TIMEOUT)
    }

    /// As [`Self::sh`] with a time limit of its own over SSH (work whose length follows a file's
    /// size, `SPEC-0317` v7).
    pub fn sh_within(
        &self,
        script: &str,
        args: &[&str],
        input: Option<&[u8]>,
        limit: std::time::Duration,
    ) -> Result<RemoteOutput, RemoteError> {
        match self {
            Self::Ssh(r) => r.sh(script, args, input, limit),
            Self::Exec(e) => {
                let mut line = format!("sh -c {} sh", quote(script));
                for a in args {
                    line.push(' ');
                    line.push_str(&quote(a));
                }
                run_exec(e, &line, input)
            }
        }
    }
}

fn run_exec(e: &ExecRoute, line: &str, input: Option<&[u8]>) -> Result<RemoteOutput, RemoteError> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = e
        .command()?
        .arg(line)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            RemoteError::Failed(format!(
                "{}: the engine program could not be started: {err}",
                e.label()
            ))
        })?;
    if let (Some(data), Some(mut si)) = (input, child.stdin.take()) {
        let data = data.to_vec();
        std::thread::spawn(move || {
            let _ = si.write_all(&data);
        });
    }
    let out = child
        .wait_with_output()
        .map_err(|err| RemoteError::Failed(err.to_string()))?;
    Ok(RemoteOutput {
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        exit_code: out.status.code(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(c: &Command) -> Vec<String> {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn ak2_routes_build_the_expected_command_lines() {
        let route = |s: &str| ExecRoute::new(HostSpec::parse(s).unwrap()).unwrap();
        assert_eq!(
            argv(&route("docker://web").command().unwrap()),
            ["docker", "exec", "-i", "web", "sh", "-c"]
        );
        assert_eq!(
            argv(&route("podman://web").command().unwrap()),
            ["podman", "exec", "-i", "web", "sh", "-c"]
        );
        assert_eq!(
            argv(&route("k8s://prod/api").command().unwrap()),
            [
                "kubectl", "exec", "-i", "-n", "prod", "api", "--", "sh", "-c"
            ]
        );
        assert_eq!(
            argv(&route("k8s://arn:cluster/prod/api/app").command().unwrap()),
            [
                "kubectl",
                "--context",
                "arn:cluster",
                "exec",
                "-i",
                "-n",
                "prod",
                "api",
                "-c",
                "app",
                "--",
                "sh",
                "-c"
            ]
        );
        assert_eq!(
            argv(&route("k8s://prod/api/app").command().unwrap()),
            [
                "kubectl", "exec", "-i", "-n", "prod", "api", "-c", "app", "--", "sh", "-c"
            ]
        );
    }

    #[test]
    fn ak2_wsl_needs_windows_or_a_named_program() {
        let spec = HostSpec::parse("wsl://Ubuntu").unwrap();
        let plain = ExecRoute::new(spec.clone()).unwrap();
        if !cfg!(windows) {
            let e = plain.command().unwrap_err().to_string();
            assert!(e.contains("Windows"), "{e}");
        }
        let fake = ExecRoute::new(spec)
            .unwrap()
            .with_program("/bin/fake-wsl".into());
        assert_eq!(
            argv(&fake.command().unwrap()),
            ["/bin/fake-wsl", "-d", "Ubuntu", "--exec", "sh", "-c"]
        );
    }

    #[test]
    fn ak2_bad_parts_are_refused() {
        for bad in [
            "docker://-privileged",
            "docker://a b",
            "docker://a;rm",
            "docker://$(x)",
            "k8s://-ns/pod",
            "k8s://ns/-pod",
            "wsl://--list",
            "paired://-oProxyCommand=x",
        ] {
            assert!(HostSpec::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn host_strings_round_trip_as_labels() {
        for s in ["docker://web", "k8s://prod/api/app", "wsl://Ubuntu"] {
            assert_eq!(HostSpec::parse(s).unwrap().label(), s);
        }
        assert_eq!(
            HostSpec::parse("paired://host").unwrap().label(),
            "paired://host:47100"
        );
        assert_eq!(
            HostSpec::parse("paired://host:9000#ABCDEFGH23")
                .unwrap()
                .label(),
            "paired://host:9000"
        );
        assert_eq!(
            HostSpec::pairing_code("paired://host:9000#ABCDEFGH23").as_deref(),
            Some("ABCDEFGH23")
        );
        assert_eq!(HostSpec::pairing_code("paired://host"), None);
        assert_eq!(
            HostSpec::parse("user@dev:2222").unwrap(),
            HostSpec::Ssh("user@dev:2222".into())
        );
        assert!(HostSpec::parse("ftp://x").is_err());
    }
}
