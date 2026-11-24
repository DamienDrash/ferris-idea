//! Ferris IDEA's backend on a remote host (`SPEC-0158`): the headless `ferris-idea mcp <folder>` runs on
//! the machine that holds the code, reached through the system `ssh`, and the IDE or local agents
//! call its tools (search, inspections, language servers, builds) over the SSH channel.
//!
//! The backend is a Ferris IDEA binary on the host: one on `PATH` or in `~/.cache/ferris-idea/bin` with
//! the same version, else — when the host runs the same system and processor — a copy of the local
//! binary uploaded there. Nothing else is installed.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

use crate::ssh::{Remote, RemoteError, quote};

/// Where uploaded backends live on the host (below the home folder).
pub const BACKEND_DIR: &str = ".cache/ferris-idea/bin";

/// The host's system and processor as `uname -sm` prints them, e.g. `Linux x86_64`.
pub fn host_platform(remote: &Remote) -> Result<String, RemoteError> {
    let out = remote.sh("uname -sm", &[], None, crate::ssh::TIMEOUT)?;
    if !out.success() {
        return Err(RemoteError::Failed(format!("uname: {}", out.stderr.trim())));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// This machine's system and processor in `uname -sm` terms.
pub fn local_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "Darwin",
        "linux" => "Linux",
        other => other,
    };
    let arch = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "arm64",
        (_, a) => a,
    };
    format!("{os} {arch}")
}

/// Whether two `uname -sm` answers are the same platform (`aarch64` and `arm64` are one).
pub fn same_platform(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().replace("aarch64", "arm64").to_lowercase();
    norm(a) == norm(b)
}

/// The path of the uploaded backend of `version` in `dir` (both relative to the home folder).
pub fn backend_path(dir: &str, version: &str) -> String {
    format!("{dir}/ferris-idea-{version}")
}

/// A short name for this build of the binary: its size and modification time.
pub fn fingerprint(binary: &Path) -> String {
    let meta = std::fs::metadata(binary).ok();
    let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
    let time = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    format!("{:x}", size ^ time.rotate_left(20))
}

/// How the backend was found or brought to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// An `ferris-idea` of this version was already there (path on the host).
    Present(String),
    /// The local binary was uploaded to this path.
    Uploaded(String),
}

impl Installed {
    /// Path of the backend on the host.
    pub fn path(&self) -> &str {
        match self {
            Self::Present(p) | Self::Uploaded(p) => p,
        }
    }
}

/// Finds a Ferris IDEA of `version` on the host (the one uploaded to `~/<dir>`, then `PATH`), or
/// uploads `local_binary` there when the platforms match; refuses with what to install otherwise
/// (behaviour 2). `dir` is normally [`BACKEND_DIR`].
pub fn ensure_backend(
    remote: &Remote,
    dir: &str,
    version: &str,
    local_binary: &Path,
) -> Result<Installed, RemoteError> {
    let route = crate::route::Route::Ssh(remote.clone());
    let host = crate::bootstrap::HostPlatform {
        family: crate::bootstrap::HostFamily::Posix,
        platform: String::new(),
    };
    ensure_backend_on(&route, dir, version, local_binary, &host, true)
}

/// As [`ensure_backend`] on any route with a POSIX shell. `build` is the program to bring to
/// the host. With `same_platform_only` it is uploaded only when the host has this machine's
/// platform; otherwise `host` already says the build fits.
pub(crate) fn ensure_backend_on(
    route: &crate::route::Route,
    dir: &str,
    version: &str,
    local_binary: &Path,
    host: &crate::bootstrap::HostPlatform,
    same_platform_only: bool,
) -> Result<Installed, RemoteError> {
    // The uploaded copy is named after this build, so another build of the same version is not
    // taken for it.
    let path = format!(
        "{}-{}",
        backend_path(dir, version),
        fingerprint(local_binary)
    );
    let want = format!("ferris-idea {version}");
    // `$HOME`-relative path first, then whatever `ferris-idea` is on PATH.
    let probe = route.sh(
        "for p in \"$HOME/$1\" \"$(command -v ferris-idea 2>/dev/null)\"; do \
         [ -n \"$p\" ] && [ -x \"$p\" ] && [ \"$(\"$p\" --version 2>/dev/null | cut -d' ' -f1-2)\" = \"$2\" ] && \
         { echo \"$p\"; exit 0; }; done; exit 1",
        &[&path, &want],
        None,
    )?;
    if probe.success() {
        return Ok(Installed::Present(
            String::from_utf8_lossy(&probe.stdout).trim().to_owned(),
        ));
    }
    if same_platform_only {
        let answer = route.sh("uname -sm", &[], None)?;
        if !answer.success() {
            return Err(RemoteError::Failed(format!(
                "uname: {}",
                answer.stderr.trim()
            )));
        }
        let found = String::from_utf8_lossy(&answer.stdout).trim().to_owned();
        let local = local_platform();
        if !same_platform(&found, &local) {
            return Err(RemoteError::Failed(format!(
                "{}: the host runs {found}, this Ferris IDEA is built for {local}; install Ferris IDEA \
                 {version} there (on PATH or as ~/{path})",
                route.label()
            )));
        }
    }
    let _ = host;
    upload(route, local_binary, &path)?;
    Ok(Installed::Uploaded(format!("$HOME/{path}")))
}

/// Streams `local` to `~/<path>` on the host through a temporary file, made executable and renamed
/// into place.
fn upload(route: &crate::route::Route, local: &Path, path: &str) -> Result<(), RemoteError> {
    let file = std::fs::File::open(local)
        .map_err(|e| RemoteError::Failed(format!("{}: {e}", local.display())))?;
    let script = "set -e; d=\"$HOME/$(dirname \"$1\")\"; mkdir -p \"$d\"; \
                  t=\"$HOME/$1.part.$$\"; cat > \"$t\"; chmod 755 \"$t\"; mv -f \"$t\" \"$HOME/$1\"";
    let line = format!("sh -c {} sh {}", quote(script), quote(path));
    let out = route
        .command()?
        .arg(line)
        .stdin(Stdio::from(file))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| RemoteError::NoSsh(e.to_string()))?;
    if out.status.code() == Some(255) {
        return Err(route.refusal(&String::from_utf8_lossy(&out.stderr)));
    }
    if !out.status.success() {
        return Err(RemoteError::Failed(format!(
            "upload: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// A running backend: `ferris-idea mcp <folder>` on the host, spoken to with MCP over SSH (behaviour
/// 3). Ends when dropped.
pub struct Backend {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
    /// Name and version the backend reported.
    pub server: String,
    /// Its tools: name and description.
    pub tools: Vec<(String, String)>,
}

impl Backend {
    /// Starts the backend at `path` (as [`ensure_backend`] found it) for `folder` on the host and
    /// reads its tools.
    pub fn start(remote: &Remote, path: &str, folder: &str) -> Result<Self, RemoteError> {
        // `$HOME/…` is expanded by the host's shell; everything else is quoted.
        let program = match path.strip_prefix("$HOME/") {
            Some(rest) => format!("\"$HOME\"/{}", quote(rest)),
            None => quote(path),
        };
        let line = format!("exec {program} mcp {}", quote(folder));
        let mut child = remote
            .command()?
            .arg(line)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| RemoteError::NoSsh(e.to_string()))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| RemoteError::Failed("no input to the backend".into()))?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| RemoteError::Failed("no output from the backend".into()))?,
        );
        let mut b = Self {
            child,
            input,
            output,
            next_id: 0,
            server: String::new(),
            tools: Vec::new(),
        };
        let init = b.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-03-26",
                // The backend's tools are listed for others: all of them (SPEC-0006 v58
                // behaviour 18); a backend before v58 lists all anyway.
                "capabilities": {"experimental": {"ferris-idea/tools": {"groups": ["all"]}}},
                "clientInfo": {"name": "ferris-idea-remote", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        b.server = format!(
            "{} {}",
            init["serverInfo"]["name"].as_str().unwrap_or("?"),
            init["serverInfo"]["version"].as_str().unwrap_or("")
        )
        .trim()
        .to_owned();
        b.notify("notifications/initialized")?;
        let list = b.request("tools/list", serde_json::json!({}))?;
        b.tools = list["tools"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|t| {
                        (
                            t["name"].as_str().unwrap_or_default().to_owned(),
                            t["description"].as_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(b)
    }

    fn send(&mut self, message: &serde_json::Value) -> Result<(), RemoteError> {
        let line = format!("{message}\n");
        self.input
            .write_all(line.as_bytes())
            .and_then(|()| self.input.flush())
            .map_err(|e| RemoteError::Failed(format!("the backend stopped: {e}")))
    }

    fn notify(&mut self, method: &str) -> Result<(), RemoteError> {
        self.send(&serde_json::json!({"jsonrpc": "2.0", "method": method}))
    }

    /// Sends a request and waits for its answer (other messages are skipped).
    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RemoteError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(
            &serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )?;
        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .output
                .read_line(&mut line)
                .map_err(|e| RemoteError::Failed(format!("the backend stopped: {e}")))?;
            if n == 0 {
                return Err(RemoteError::Failed(
                    "the backend ended (is Ferris IDEA installed and the folder there?)".into(),
                ));
            }
            let Ok(msg) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            if msg["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(e) = msg.get("error") {
                return Err(RemoteError::Failed(
                    e["message"].as_str().unwrap_or("error").to_owned(),
                ));
            }
            return Ok(msg["result"].clone());
        }
    }

    /// Calls tool `name` with `arguments`: its JSON answer (the text of the first content part,
    /// parsed when it is JSON), or the tool's error.
    pub fn call(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, RemoteError> {
        let result = self.request(
            "tools/call",
            serde_json::json!({"name": name, "arguments": arguments}),
        )?;
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        let value = serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!(text));
        if result["isError"].as_bool() == Some(true) {
            return Err(RemoteError::Failed(format!("{name}: {text}")));
        }
        Ok(value)
    }

    /// Whether the backend process still runs.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One step of a run configuration's chain as the backend plans it: working folder relative to
/// the project, environment and command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStep {
    /// Configuration name.
    pub name: String,
    /// Folder relative to the project (`.` for the project itself).
    pub cwd: String,
    /// Environment variables.
    pub env: Vec<(String, String)>,
    /// Program and arguments.
    pub command: Vec<String>,
}

/// A POSIX `sh` script that runs `steps` one after another in `folder` on the host and stops at
/// the first failing step (`SPEC-0158` behaviour 9).
pub fn run_script(folder: &str, steps: &[RunStep]) -> String {
    use crate::ssh::quote;
    steps
        .iter()
        .map(|s| {
            let dir = if s.cwd.starts_with('/') {
                s.cwd.clone()
            } else {
                crate::mirror::join(folder, &s.cwd)
            };
            let mut line = format!(
                "echo {} && cd -- {}",
                quote(&format!("▶ {}", s.name)),
                quote(&dir)
            );
            line.push_str(" && ");
            if !s.env.is_empty() {
                line.push_str("env ");
                for (k, v) in &s.env {
                    line.push_str(&quote(&format!("{k}={v}")));
                    line.push(' ');
                }
            }
            line.push_str(
                &s.command
                    .iter()
                    .map(|a| quote(a))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            line
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

/// The chain of run configuration `name` from the backend's `run_configurations` and
/// `run_configuration` (dry run) answers.
pub fn run_steps(
    configs: &serde_json::Value,
    plan: &serde_json::Value,
) -> Result<Vec<RunStep>, String> {
    let steps = plan["would_run"]
        .as_array()
        .ok_or("the backend returned no plan")?;
    Ok(steps
        .iter()
        .map(|step| {
            let name = step["name"].as_str().unwrap_or_default().to_owned();
            let env = configs["configs"]
                .as_array()
                .and_then(|cs| cs.iter().find(|c| c["name"] == name.as_str()))
                .and_then(|c| c["env"].as_object())
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            RunStep {
                cwd: step["cwd"].as_str().unwrap_or(".").to_owned(),
                command: step["command"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|x| x.as_str().unwrap_or_default().to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
                env,
                name,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms_and_paths() {
        assert!(same_platform("Darwin arm64", "Darwin aarch64"));
        assert!(!same_platform("Linux x86_64", "Darwin arm64"));
        assert_eq!(
            backend_path(BACKEND_DIR, "0.0.1"),
            ".cache/ferris-idea/bin/ferris-idea-0.0.1"
        );
        assert!(local_platform().contains(' '));
        assert_eq!(Installed::Uploaded("$HOME/x".into()).path(), "$HOME/x");
    }

    #[test]
    fn run_chains_become_one_script() {
        let configs = serde_json::json!({"configs": [
            {"name": "build", "command": ["make"], "env": {}},
            {"name": "app", "command": ["./app"], "env": {"MODE": "dev x"}}
        ], "problems": []});
        let plan = serde_json::json!({"dry_run": true, "would_run": [
            {"name": "build", "command": ["make", "-j4"], "cwd": ".", "timeout": 120},
            {"name": "app", "command": ["./app", "it's"], "cwd": "bin", "timeout": 120}
        ]});
        let steps = run_steps(&configs, &plan).unwrap();
        assert_eq!(steps[1].env, [("MODE".to_owned(), "dev x".to_owned())]);
        assert_eq!(
            run_script("/srv/p", &steps),
            "echo '▶ build' && cd -- /srv/p/. && make -j4 && echo '▶ app' && cd -- /srv/p/bin && env 'MODE=dev x' ./app 'it'\\''s'"
        );
        assert!(run_steps(&configs, &serde_json::json!({})).is_err());
    }
}
