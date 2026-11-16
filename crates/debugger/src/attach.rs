//! Attaching to a program that already runs (`SPEC-0147`): the local processes, what runtime each
//! one is, and the DAP `attach` request for it.

use std::collections::BTreeMap;
use std::process::{Command, Stdio};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{DapClient, DebugSession};

/// The runtime of a process, which decides the debug adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// A native program (lldb-dap).
    Native,
    /// A Python interpreter (debugpy).
    Python,
    /// Node.js (js-debug).
    Node,
    /// A JVM (java-debug), attachable through its JDWP agent.
    Java,
    /// A Go program (Delve): a process, or a `dlv --headless` server on this machine.
    Go,
}

impl Runtime {
    /// All runtimes, in the order the dialog offers them for an address.
    pub const ALL: [Runtime; 5] = [
        Runtime::Java,
        Runtime::Python,
        Runtime::Node,
        Runtime::Native,
        Runtime::Go,
    ];

    /// Name of the adapter in [`crate::known_adapters`].
    pub fn adapter_name(self) -> &'static str {
        match self {
            Runtime::Native => "lldb-dap",
            Runtime::Python => "debugpy",
            Runtime::Node => "js-debug",
            Runtime::Java => "java-debug",
            Runtime::Go => "dlv",
        }
    }

    /// Label shown in the Attach dialog.
    pub fn label(self) -> &'static str {
        match self {
            Runtime::Native => "Native",
            Runtime::Python => "Python",
            Runtime::Node => "Node.js",
            Runtime::Java => "Java",
            Runtime::Go => "Go",
        }
    }

    /// The runtime named `name` (`native`, `python`, `node`, `java`, `go`).
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "native" | "lldb" | "lldb-dap" | "c" | "cpp" | "rust" => Some(Runtime::Native),
            "python" | "debugpy" => Some(Runtime::Python),
            "node" | "nodejs" | "node.js" | "js" | "javascript" => Some(Runtime::Node),
            "java" | "jvm" | "jdwp" => Some(Runtime::Java),
            "go" | "golang" | "dlv" | "delve" => Some(Runtime::Go),
            _ => None,
        }
    }
}

/// What to attach to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// A local process.
    Pid(u32),
    /// A debug port: JDWP, `debugpy --listen`, `node --inspect`, a gdb-remote server or
    /// `dlv --headless`.
    Address {
        /// Host name or address.
        host: String,
        /// Port.
        port: u16,
    },
}

impl Target {
    /// `host:port` or `:port` (local host) as an address.
    pub fn parse_address(text: &str) -> Option<Self> {
        let (host, port) = text.trim().rsplit_once(':')?;
        let port = port.parse().ok().filter(|p| *p > 0)?;
        let host = if host.is_empty() { "127.0.0.1" } else { host };
        let valid = host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c));
        valid.then(|| Target::Address {
            host: host.to_owned(),
            port,
        })
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Pid(pid) => write!(f, "process {pid}"),
            Target::Address { host, port } => write!(f, "{host}:{port}"),
        }
    }
}

/// A local process of this user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Process {
    /// Process id.
    pub pid: u32,
    /// Executable name.
    pub name: String,
    /// The whole command line.
    pub command: String,
    /// Its runtime.
    pub runtime: Runtime,
    /// The debug port it listens on (JDWP, `--inspect`, `debugpy --listen`), if any.
    pub debug_port: Option<u16>,
}

impl Process {
    /// What attaching to this process means: its debug port when it has one, else its id. A JVM
    /// without a JDWP agent cannot be attached to (`None`).
    pub fn target(&self) -> Option<Target> {
        match (self.debug_port, self.runtime) {
            (Some(port), _) => Some(Target::Address {
                host: "127.0.0.1".into(),
                port,
            }),
            (None, Runtime::Java) => None,
            // A Delve server is reached through its port; the process is Delve, not the program.
            (None, Runtime::Go) => None,
            (None, _) => Some(Target::Pid(self.pid)),
        }
    }
}

/// The port in `address` forms `5005`, `*:5005`, `localhost:5005`.
fn port_of(address: &str) -> Option<u16> {
    address.rsplit(':').next()?.parse().ok()
}

/// The runtime of a process with executable `name` and command line `args`, and the debug port
/// its command line opens (behaviour 2).
pub fn classify(name: &str, args: &[&str]) -> (Runtime, Option<u16>) {
    let base = name.rsplit('/').next().unwrap_or(name);
    if base == "java" || base.starts_with("java.") {
        let port = args.iter().find_map(|a| {
            let opts = a
                .strip_prefix("-agentlib:jdwp=")
                .or_else(|| a.strip_prefix("-Xrunjdwp:"))?;
            let map: BTreeMap<&str, &str> =
                opts.split(',').filter_map(|o| o.split_once('=')).collect();
            (map.get("server") == Some(&"y"))
                .then(|| map.get("address").and_then(|a| port_of(a)))
                .flatten()
        });
        return (Runtime::Java, port);
    }
    if base == "node" || base.starts_with("node.") {
        let port = args.iter().find_map(|a| {
            ["--inspect-brk", "--inspect-wait", "--inspect"]
                .iter()
                .find_map(|flag| {
                    let rest = a.strip_prefix(flag)?;
                    match rest.strip_prefix('=') {
                        Some(addr) => port_of(addr),
                        None if rest.is_empty() => Some(9229),
                        None => None,
                    }
                })
        });
        return (Runtime::Node, port);
    }
    if base.starts_with("python") || base == "Python" {
        let listen = args.iter().position(|a| *a == "--listen");
        let port = listen
            .filter(|_| {
                args.iter()
                    .any(|a| *a == "debugpy" || a.ends_with("/debugpy"))
            })
            .and_then(|i| args.get(i + 1))
            .and_then(|a| port_of(a));
        return (Runtime::Python, port);
    }
    // A Delve server without a client: `dlv debug|exec|test|attach --headless --listen=…`.
    if base == "dlv" && args.contains(&"--headless") {
        let port = args.iter().enumerate().find_map(|(i, a)| {
            match a
                .strip_prefix("--listen=")
                .or_else(|| a.strip_prefix("-l="))
            {
                Some(address) => port_of(address),
                None if *a == "--listen" || *a == "-l" => args.get(i + 1).and_then(|a| port_of(a)),
                None => None,
            }
        });
        return (Runtime::Go, port);
    }
    (Runtime::Native, None)
}

fn ps(field: &str) -> BTreeMap<u32, String> {
    let out = Command::new("ps")
        .args(["-x", "-o", &format!("pid=,{field}=")])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else {
        return BTreeMap::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, rest) = l.split_once(' ')?;
            Some((pid.parse().ok()?, rest.trim().to_owned()))
        })
        .collect()
}

/// The processes of this user, without this one: those with a debug port first, then the rest by
/// name (behaviour 1).
pub fn processes() -> Vec<Process> {
    let own = std::process::id();
    let commands = ps("args");
    let mut list: Vec<Process> = ps("comm")
        .into_iter()
        .filter(|(pid, _)| *pid != own)
        .map(|(pid, comm)| {
            let command = commands.get(&pid).cloned().unwrap_or_else(|| comm.clone());
            let name = comm.rsplit('/').next().unwrap_or(&comm).to_owned();
            let args: Vec<&str> = command.split_whitespace().collect();
            let (runtime, debug_port) = classify(&name, &args);
            Process {
                pid,
                name,
                command,
                runtime,
                debug_port,
            }
        })
        .collect();
    list.sort_by(|a, b| {
        (a.debug_port.is_none(), a.name.to_lowercase(), a.pid).cmp(&(
            b.debug_port.is_none(),
            b.name.to_lowercase(),
            b.pid,
        ))
    });
    list
}

/// The `attach` arguments for `runtime`'s adapter (behaviour 3); `cwd` is where sources are looked
/// up. Errors name what is missing.
pub fn attach_arguments(
    runtime: Runtime,
    target: &Target,
    cwd: &std::path::Path,
) -> Result<Value, String> {
    Ok(match (runtime, target) {
        (Runtime::Native, Target::Pid(pid)) => json!({"pid": pid}),
        (Runtime::Native, Target::Address { host, port }) => {
            json!({"gdb-remote-hostname": host, "gdb-remote-port": port})
        }
        (Runtime::Python, Target::Pid(pid)) => json!({
            "type": "python", "request": "attach", "processId": pid, "justMyCode": true,
        }),
        (Runtime::Python, Target::Address { host, port }) => json!({
            "type": "python", "request": "attach", "connect": {"host": host, "port": port},
            "justMyCode": true,
        }),
        (Runtime::Node, Target::Pid(pid)) => json!({
            "type": "pwa-node", "request": "attach", "name": format!("process {pid}"),
            "processId": pid.to_string(), "cwd": cwd,
        }),
        (Runtime::Node, Target::Address { host, port }) => json!({
            "type": "pwa-node", "request": "attach", "name": format!("{host}:{port}"),
            "address": host, "port": port, "cwd": cwd,
        }),
        (Runtime::Java, Target::Address { host, port }) => json!({
            "type": "java", "request": "attach", "hostName": host, "port": port,
            "timeout": 10000,
        }),
        (Runtime::Go, Target::Pid(pid)) => json!({
            "request": "attach", "mode": "local", "processId": pid, "stopOnEntry": false,
        }),
        // A Delve server speaks the protocol itself; only one on this machine is talked to, since
        // its port is neither encrypted nor authenticated (`SPEC-0147` v3 behaviour 7).
        (Runtime::Go, Target::Address { host, .. }) => {
            if !crate::delve::is_loopback(host) {
                return Err(format!(
                    "a Delve server is attached to on this machine only (127.0.0.1), not on {host}: \
                     its port is not protected; forward it through SSH or open the project on that host"
                ));
            }
            json!({ "request": "attach", "mode": "remote", "stopOnEntry": false })
        }
        (Runtime::Java, Target::Pid(pid)) => {
            return Err(format!(
                "process {pid} has no JDWP agent; start it with \
                 -agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005 and attach to that port"
            ));
        }
    })
}

/// Attaches `runtime`'s adapter to `target` with `breakpoints` set (behaviour 3). `adapter` is the
/// adapter command; js-debug runs as a server, `debugpy --listen` is the adapter itself and is
/// talked to directly. Java goes through jdtls and is attached by the caller with
/// [`DebugSession::attach_on`].
pub fn attach(
    runtime: Runtime,
    target: &Target,
    adapter: &[String],
    cwd: &std::path::Path,
    breakpoints: &BTreeMap<std::path::PathBuf, Vec<crate::SourceBreakpoint>>,
    special: &crate::SpecialBreakpoints,
) -> Result<
    (
        DebugSession,
        BTreeMap<std::path::PathBuf, Vec<crate::Breakpoint>>,
    ),
    String,
> {
    let arguments = attach_arguments(runtime, target, cwd)?;
    let started = match (runtime, target) {
        (Runtime::Java, _) => return Err("Java attaches through jdtls".into()),
        (Runtime::Node, _) => crate::JsDebugServer::start(adapter).and_then(|server| {
            let address = server.address();
            DebugSession::launch_js(
                &address,
                "attach",
                arguments,
                breakpoints,
                special,
                Box::new(server),
            )
        }),
        (Runtime::Python, Target::Address { host, port }) => {
            DapClient::connect(&format!("{host}:{port}"), None).and_then(|client| {
                DebugSession::attach_on(client, "debugpy", arguments, breakpoints, special)
            })
        }
        (Runtime::Go, Target::Address { host, port }) => {
            DapClient::connect(&format!("{host}:{port}"), None)
                .and_then(|client| {
                    DebugSession::attach_on(client, "dlv", arguments, breakpoints, special)
                })
                .map(|(mut session, placed)| {
                    // What Stop does to the program is the server's doing, so it is said.
                    session.add_console_note(crate::delve::SERVER_NOTE);
                    (session, placed)
                })
        }
        (Runtime::Go, Target::Pid(_)) => {
            crate::delve::attach_go(adapter, arguments, breakpoints, special)
        }
        _ => DebugSession::attach(
            adapter,
            runtime.adapter_name(),
            arguments,
            breakpoints,
            special,
        ),
    };
    started.map_err(|e| format!("cannot attach to {target}: {e}"))
}

/// Adds the adapter's source mapping for `mappings` (host path, path in the target) to an attach
/// request (`SPEC-0322` behaviour 15); java-debug finds sources through jdtls and needs none.
pub fn with_source_map(runtime: Runtime, arguments: &mut Value, mappings: &[(String, String)]) {
    if mappings.is_empty() {
        return;
    }
    match runtime {
        Runtime::Python => {
            arguments["pathMappings"] = mappings
                .iter()
                .map(|(host, target)| json!({ "localRoot": host, "remoteRoot": target }))
                .collect();
        }
        Runtime::Node => {
            let (host, target) = &mappings[0];
            arguments["localRoot"] = json!(host);
            arguments["remoteRoot"] = json!(target);
        }
        Runtime::Native => {
            arguments["sourceMap"] = mappings
                .iter()
                .map(|(host, target)| json!([target, host]))
                .collect();
        }
        Runtime::Go => {
            arguments["substitutePath"] = mappings
                .iter()
                .map(|(host, target)| json!({ "from": host, "to": target }))
                .collect();
        }
        Runtime::Java => {}
    }
}

/// A container or pod attach target typed into the dialog of a remote project (`SPEC-0322`
/// behaviour 17): `docker:<container>:<port>` or `pod:[<namespace>/]<pod>:<port>`, optionally with
/// `=<folder in the target>`. Returns the `attach` parameter and that folder.
pub fn parse_host_target(text: &str) -> Option<(Value, Option<String>)> {
    let text = text.trim();
    let (spec, folder) = match text.split_once('=') {
        Some((s, f)) => (s, Some(f.trim().to_owned()).filter(|f| !f.is_empty())),
        None => (text, None),
    };
    let (kind, rest) = spec.split_once(':')?;
    let (name, port) = rest.rsplit_once(':')?;
    let port: u16 = port.parse().ok().filter(|p| *p > 0)?;
    if name.is_empty() {
        return None;
    }
    let attach = match kind {
        "docker" => json!({ "kind": "docker", "container": name, "port": port }),
        "pod" => match name.split_once('/') {
            Some((ns, pod)) if !ns.is_empty() && !pod.is_empty() => {
                json!({ "kind": "pod", "pod": pod, "namespace": ns, "port": port })
            }
            None => json!({ "kind": "pod", "pod": name, "port": port }),
            Some(_) => return None,
        },
        _ => return None,
    };
    Some((attach, folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtimes_and_debug_ports() {
        let java = [
            "java",
            "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005",
            "Main",
        ];
        assert_eq!(classify("java", &java), (Runtime::Java, Some(5005)));
        let client = [
            "java",
            "-agentlib:jdwp=transport=dt_socket,server=n,address=9000",
        ];
        assert_eq!(classify("java", &client), (Runtime::Java, None));
        assert_eq!(
            classify("/usr/bin/java", &["java", "Main"]),
            (Runtime::Java, None)
        );
        assert_eq!(
            classify("node", &["node", "--inspect", "app.js"]),
            (Runtime::Node, Some(9229))
        );
        assert_eq!(
            classify("node", &["node", "--inspect-brk=127.0.0.1:9333", "app.js"]),
            (Runtime::Node, Some(9333))
        );
        assert_eq!(classify("node", &["node", "app.js"]), (Runtime::Node, None));
        assert_eq!(
            classify(
                "python3.12",
                &["python3.12", "-m", "debugpy", "--listen", "5678", "app.py"]
            ),
            (Runtime::Python, Some(5678))
        );
        assert_eq!(
            classify("python3", &["python3", "app.py", "--listen", "80"]),
            (Runtime::Python, None)
        );
        assert_eq!(classify("server", &["./server"]), (Runtime::Native, None));
        // A Delve server (`SPEC-0147` v3 behaviour 7); `dlv dap` serves a client of its own.
        let headless = ["dlv", "debug", "--headless", "--listen=127.0.0.1:2345", "."];
        assert_eq!(classify("dlv", &headless), (Runtime::Go, Some(2345)));
        let spaced = ["dlv", "exec", "./app", "--headless", "-l", ":4000"];
        assert_eq!(classify("dlv", &spaced), (Runtime::Go, Some(4000)));
        assert_eq!(
            classify("dlv", &["dlv", "dap", "--listen=127.0.0.1:1"]),
            (Runtime::Native, None)
        );
        assert_eq!(Runtime::parse("Go"), Some(Runtime::Go));
        assert_eq!(Runtime::Go.adapter_name(), "dlv");
    }

    #[test]
    fn targets_of_processes() {
        let p = |runtime, debug_port| Process {
            pid: 42,
            name: "x".into(),
            command: "x".into(),
            runtime,
            debug_port,
        };
        assert_eq!(p(Runtime::Native, None).target(), Some(Target::Pid(42)));
        assert_eq!(p(Runtime::Java, None).target(), None);
        assert_eq!(
            p(Runtime::Java, Some(5005)).target(),
            Target::parse_address(":5005")
        );
        assert_eq!(
            Target::parse_address("db.local:5005"),
            Some(Target::Address {
                host: "db.local".into(),
                port: 5005
            })
        );
        assert_eq!(Target::parse_address("nope"), None);
        assert_eq!(Target::parse_address("a b:1"), None);
        assert_eq!(Target::parse_address("host:0"), None);
    }

    #[test]
    fn attach_requests_per_adapter() {
        let cwd = std::path::Path::new("/w");
        let at = Target::parse_address("h:1").unwrap();
        // Go (`SPEC-0147` v3 behaviour 7): a process through Delve, a Delve server only here.
        assert_eq!(
            attach_arguments(Runtime::Go, &Target::Pid(7), cwd).unwrap(),
            json!({"request": "attach", "mode": "local", "processId": 7, "stopOnEntry": false})
        );
        let here = Target::parse_address(":2345").unwrap();
        assert_eq!(
            attach_arguments(Runtime::Go, &here, cwd).unwrap()["mode"],
            "remote"
        );
        assert!(
            attach_arguments(Runtime::Go, &at, cwd)
                .unwrap_err()
                .contains("this machine only")
        );
        let mut mapped = json!({});
        with_source_map(Runtime::Go, &mut mapped, &[("/w".into(), "/app".into())]);
        assert_eq!(
            mapped["substitutePath"],
            json!([{"from": "/w", "to": "/app"}])
        );
        assert_eq!(
            attach_arguments(Runtime::Native, &Target::Pid(7), cwd).unwrap(),
            json!({"pid": 7})
        );
        assert_eq!(
            attach_arguments(Runtime::Native, &at, cwd).unwrap()["gdb-remote-port"],
            1
        );
        assert_eq!(
            attach_arguments(Runtime::Python, &at, cwd).unwrap()["connect"],
            json!({"host": "h", "port": 1})
        );
        assert_eq!(
            attach_arguments(Runtime::Python, &Target::Pid(7), cwd).unwrap()["processId"],
            7
        );
        assert_eq!(
            attach_arguments(Runtime::Node, &Target::Pid(7), cwd).unwrap()["processId"],
            "7"
        );
        assert_eq!(
            attach_arguments(Runtime::Node, &at, cwd).unwrap()["address"],
            "h"
        );
        assert_eq!(
            attach_arguments(Runtime::Java, &at, cwd).unwrap()["hostName"],
            "h"
        );
        let err = attach_arguments(Runtime::Java, &Target::Pid(7), cwd).unwrap_err();
        assert!(err.contains("-agentlib:jdwp"), "{err}");
    }

    #[test]
    fn source_maps_per_adapter() {
        let map = [("/srv/app".to_owned(), "/app".to_owned())];
        let mut a = json!({});
        with_source_map(Runtime::Python, &mut a, &map);
        assert_eq!(a["pathMappings"][0]["remoteRoot"], "/app");
        let mut a = json!({});
        with_source_map(Runtime::Node, &mut a, &map);
        assert_eq!(a["localRoot"], "/srv/app");
        assert_eq!(a["remoteRoot"], "/app");
        let mut a = json!({});
        with_source_map(Runtime::Native, &mut a, &map);
        assert_eq!(a["sourceMap"], json!([["/app", "/srv/app"]]));
        let mut a = json!({});
        with_source_map(Runtime::Java, &mut a, &map);
        assert_eq!(a, json!({}));
    }

    #[test]
    fn container_and_pod_targets_are_parsed() {
        let (t, f) = parse_host_target("docker:web:5678=/app").unwrap();
        assert_eq!(
            t,
            json!({"kind": "docker", "container": "web", "port": 5678})
        );
        assert_eq!(f.as_deref(), Some("/app"));
        let (t, f) = parse_host_target("pod:ns/api-1:5005").unwrap();
        assert_eq!(
            t,
            json!({"kind": "pod", "pod": "api-1", "namespace": "ns", "port": 5005})
        );
        assert!(f.is_none());
        assert_eq!(parse_host_target("pod:api:9229").unwrap().0["pod"], "api");
        assert!(parse_host_target("docker:web").is_none());
        assert!(parse_host_target("127.0.0.1:5005").is_none());
        assert!(parse_host_target("pod:/x:1").is_none());
    }

    #[test]
    fn this_process_is_not_listed() {
        let own = std::process::id();
        assert!(processes().iter().all(|p| p.pid != own));
    }
}
