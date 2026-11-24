//! The target of an attach (`SPEC-0322` behaviours 10–13): a process or an address on the host, a
//! container or a Kubernetes pod the host reaches. The adapter runs on the host and connects from
//! there, so none of these opens a port on the window's machine.

use std::path::Path;
use std::process::{Command, Stdio};

use ferris_idea_session::{ErrorCode, RpcError};
use serde_json::Value;

use super::{Group, run_error, wait_line};
use crate::{HostOptions, internal, invalid};

/// A resolved target.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolved {
    /// Address the adapter connects to from the host.
    pub address: Option<(String, u16)>,
    /// Process on the host.
    pub pid: Option<u32>,
    /// Host path and path in the target (behaviour 15).
    pub mappings: Vec<(String, String)>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
}

fn port_of(params: &Value) -> Result<u16, RpcError> {
    params["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p > 0)
        .ok_or_else(|| invalid("`port` is not a TCP port"))
}

/// Resolves `params` (behaviour 10); a tunnel it starts belongs to `group`.
pub(super) fn resolve(
    params: &Value,
    options: &HostOptions,
    root: &Path,
    group: &Group,
) -> Result<Resolved, RpcError> {
    match params["kind"].as_str().unwrap_or_default() {
        "pid" => {
            let pid = params["pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .ok_or_else(|| invalid("`pid` is missing"))?;
            Ok(Resolved {
                pid: Some(pid),
                ..Resolved::default()
            })
        }
        "address" => {
            let host = params["host"].as_str().unwrap_or("127.0.0.1").trim();
            if host.is_empty() || host.contains(['/', ' ', '@']) {
                return Err(invalid("`host` is not a host name or address"));
            }
            Ok(Resolved {
                address: Some((host.to_owned(), port_of(params)?)),
                ..Resolved::default()
            })
        }
        "docker" => docker(params, options, root),
        "pod" => pod(params, options, group),
        other => Err(invalid(format!("unknown attach target {other:?}"))),
    }
}

fn docker(params: &Value, options: &HostOptions, root: &Path) -> Result<Resolved, RpcError> {
    let name = params["container"].as_str().unwrap_or_default();
    if !valid_name(name) {
        return Err(invalid(format!("{name:?} is not a container name")));
    }
    let port = port_of(params)?;
    let programs: Vec<String> = match &options.docker_program {
        Some(p) => vec![p.clone()],
        None => vec!["docker".into(), "podman".into()],
    };
    let mut last = String::new();
    for program in &programs {
        match Command::new(program)
            .args(["inspect", name])
            .stdin(Stdio::null())
            .output()
        {
            Ok(o) if o.status.success() => {
                let list: Value = serde_json::from_slice(&o.stdout).map_err(internal)?;
                return docker_target(&list[0], port, root);
            }
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_owned(),
            Err(_) => {}
        }
    }
    Err(RpcError::new(
        ErrorCode::NotFound,
        if last.is_empty() {
            "neither docker nor podman is available on the host".to_owned()
        } else {
            last
        },
    ))
}

/// Where the adapter reaches port `port` of the container described by `inspect` (one element of
/// `docker inspect`): a port published on loopback, else the container's own address; a port
/// published on every interface is refused (behaviour 12).
pub fn docker_target(inspect: &Value, port: u16, root: &Path) -> Result<Resolved, RpcError> {
    let settings = &inspect["NetworkSettings"];
    let bindings = settings["Ports"][format!("{port}/tcp")]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let loopback = bindings.iter().find_map(|b| {
        let ip = b["HostIp"].as_str().unwrap_or_default();
        let host_port: u16 = b["HostPort"].as_str()?.parse().ok()?;
        matches!(ip, "127.0.0.1" | "::1").then(|| (ip.to_owned(), host_port))
    });
    let address = if let Some(a) = loopback {
        a
    } else if !bindings.is_empty() {
        return Err(RpcError::new(
            ErrorCode::Denied,
            format!("port {port} of the container is published on every interface"),
        )
        .hint(format!(
            "a debug port must not be open: publish it with -p 127.0.0.1:{port}:{port} (ADR-0025 §5)"
        )));
    } else {
        let own = settings["IPAddress"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                settings["Networks"].as_object().and_then(|n| {
                    n.values().find_map(|net| {
                        net["IPAddress"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                    })
                })
            })
            .ok_or_else(|| {
                RpcError::new(
                    ErrorCode::Unavailable,
                    format!("port {port} of the container is not reachable from the host"),
                )
                .hint(format!("publish it with -p 127.0.0.1:{port}:{port}"))
            })?;
        (own, port)
    };
    Ok(Resolved {
        address: Some(address),
        pid: None,
        mappings: mappings_of(inspect, root),
    })
}

/// Bind mounts covering the project folder (behaviour 12).
fn mappings_of(inspect: &Value, root: &Path) -> Vec<(String, String)> {
    let root = root.to_string_lossy();
    let root = root.trim_end_matches('/');
    let mut out = Vec::new();
    for mount in inspect["Mounts"].as_array().into_iter().flatten() {
        if mount["Type"].as_str() != Some("bind") {
            continue;
        }
        let (Some(src), Some(dst)) = (mount["Source"].as_str(), mount["Destination"].as_str())
        else {
            continue;
        };
        let (src, dst) = (src.trim_end_matches('/'), dst.trim_end_matches('/'));
        if let Some(rest) = root.strip_prefix(src)
            && (rest.is_empty() || rest.starts_with('/'))
        {
            out.push((root.to_owned(), format!("{dst}{rest}")));
        } else if src
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
        {
            out.push((src.to_owned(), dst.to_owned()));
        }
    }
    out
}

/// `kubectl`'s arguments for a tunnel to `port` of `pod` on a free local port (behaviour 13).
pub fn kubectl_args(
    pod: &str,
    namespace: Option<&str>,
    context: Option<&str>,
    port: u16,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(c) = context {
        args.extend(["--context".to_owned(), c.to_owned()]);
    }
    if let Some(n) = namespace {
        args.extend(["--namespace".to_owned(), n.to_owned()]);
    }
    args.extend([
        "port-forward".to_owned(),
        "--address".to_owned(),
        "127.0.0.1".to_owned(),
        format!("pod/{pod}"),
        format!(":{port}"),
    ]);
    args
}

/// The local port of `kubectl port-forward`'s `Forwarding from 127.0.0.1:N -> P` line.
pub fn forwarding_line(line: &str) -> Option<u16> {
    let rest = line.strip_prefix("Forwarding from 127.0.0.1:")?;
    rest.split_whitespace().next()?.parse().ok()
}

fn pod(params: &Value, options: &HostOptions, group: &Group) -> Result<Resolved, RpcError> {
    let name = params["pod"].as_str().unwrap_or_default();
    let namespace = params["namespace"].as_str();
    let context = params["context"].as_str();
    if !valid_name(name)
        || namespace.is_some_and(|n| !valid_name(n))
        || context.is_some_and(|c| !valid_name(c))
    {
        return Err(invalid("not a pod, namespace or context name"));
    }
    let port = port_of(params)?;
    let program = options
        .kubectl_program
        .clone()
        .unwrap_or_else(|| "kubectl".to_owned());
    let mut child = Command::new(&program)
        .args(kubectl_args(name, namespace, context, port))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| run_error(format!("{program}: {e}")))?;
    let waited = wait_line(&mut child, "kubectl", forwarding_line);
    *group.tunnel.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    let local = waited?;
    Ok(Resolved {
        address: Some(("127.0.0.1".to_owned(), local)),
        pid: None,
        mappings: Vec::new(),
    })
}

/// The inspection a test shows for a container.
#[cfg(test)]
pub(crate) fn sample_inspect(ports: Value, ip: &str, mounts: Value) -> Value {
    serde_json::json!({ "NetworkSettings": { "Ports": ports, "IPAddress": ip }, "Mounts": mounts })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn root() -> &'static Path {
        Path::new("/srv/app")
    }

    #[test]
    fn a_container_port_published_on_loopback_is_used() {
        let i = sample_inspect(
            json!({ "5678/tcp": [{ "HostIp": "127.0.0.1", "HostPort": "15678" }] }),
            "172.17.0.2",
            json!([]),
        );
        let r = docker_target(&i, 5678, root()).unwrap();
        assert_eq!(r.address, Some(("127.0.0.1".into(), 15678)));
    }

    #[test]
    fn an_open_container_port_is_refused_with_a_hint() {
        let i = sample_inspect(
            json!({ "5678/tcp": [{ "HostIp": "0.0.0.0", "HostPort": "5678" }] }),
            "172.17.0.2",
            json!([]),
        );
        let e = docker_target(&i, 5678, root()).unwrap_err();
        assert_eq!(e.code, ErrorCode::Denied);
        assert!(e.hint.unwrap().contains("127.0.0.1:5678:5678"));
    }

    #[test]
    fn an_unpublished_port_uses_the_container_address_and_mounts_give_mappings() {
        let i = sample_inspect(
            json!({}),
            "172.17.0.2",
            json!([
                { "Type": "bind", "Source": "/srv", "Destination": "/work" },
                { "Type": "bind", "Source": "/srv/app/data", "Destination": "/data" },
                { "Type": "volume", "Source": "/x", "Destination": "/y" },
            ]),
        );
        let r = docker_target(&i, 9229, root()).unwrap();
        assert_eq!(r.address, Some(("172.17.0.2".into(), 9229)));
        assert_eq!(
            r.mappings,
            vec![
                ("/srv/app".to_owned(), "/work/app".to_owned()),
                ("/srv/app/data".to_owned(), "/data".to_owned()),
            ]
        );
    }

    #[test]
    fn no_address_at_all_says_how_to_publish() {
        let i = sample_inspect(json!({}), "", json!([]));
        let e = docker_target(&i, 1, root()).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable);
    }

    #[test]
    fn pods_use_a_loopback_tunnel() {
        assert_eq!(
            kubectl_args("api-1", Some("prod"), Some("ctx"), 5005),
            [
                "--context",
                "ctx",
                "--namespace",
                "prod",
                "port-forward",
                "--address",
                "127.0.0.1",
                "pod/api-1",
                ":5005"
            ]
        );
        assert_eq!(
            forwarding_line("Forwarding from 127.0.0.1:43211 -> 5005"),
            Some(43211)
        );
        assert_eq!(forwarding_line("Forwarding from [::1]:43211 -> 5005"), None);
    }

    #[test]
    fn names_are_checked() {
        assert!(valid_name("web_1.a-b"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("a;rm"));
        assert!(!valid_name(""));
    }
}
