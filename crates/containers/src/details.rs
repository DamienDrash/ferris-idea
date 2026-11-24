//! Details of one container from `GET /containers/{id}/json` (`SPEC-0119` v5 behaviour 11, gap D5
//! of OBS-0116): environment with secret-looking values masked, ports with the address they open,
//! mounts, labels, networks and the whole inspect answer, masked the same way.

use serde::Serialize;
use serde_json::Value;

use crate::secrets::{mask_args, mask_assignment, mask_value};
use crate::{DockerError, DockerHost};

/// One environment variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvVar {
    /// Variable name.
    pub name: String,
    /// Value, [`crate::secrets::MASK`] (or with a masked URL password) when it looks secret.
    pub value: String,
    /// Whether the value was masked.
    pub masked: bool,
}

/// One container port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortEntry {
    /// Port inside the container, e.g. `80/tcp`.
    pub container: String,
    /// Where it is published on the engine's host, e.g. `0.0.0.0:8080`.
    pub published: Option<String>,
    /// Address a browser can open, for a published TCP port.
    pub url: Option<String>,
}

/// One mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountEntry {
    /// `bind`, `volume`, `tmpfs`, …
    pub kind: String,
    /// Host path or volume name.
    pub source: String,
    /// Path inside the container.
    pub destination: String,
    /// Mount mode as Docker reports it (e.g. `z`, `ro`), may be empty.
    pub mode: String,
    /// Whether the container can write to it.
    pub writable: bool,
}

/// What the details pane and MCP `docker_container inspect` show.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContainerDetails {
    /// Full id.
    pub id: String,
    /// Name without the leading `/`.
    pub name: String,
    /// Image as the container was created from it.
    pub image: String,
    /// `running`, `exited`, …
    pub state: String,
    /// When it was created (RFC 3339).
    pub created: String,
    /// When it was last started (RFC 3339), empty if never.
    pub started_at: String,
    /// Exit code of the last run, when not running.
    pub exit_code: Option<i64>,
    /// Command and arguments, secret values masked.
    pub command: Vec<String>,
    /// Environment, secret values masked.
    pub env: Vec<EnvVar>,
    /// Ports.
    pub ports: Vec<PortEntry>,
    /// Mounts.
    pub mounts: Vec<MountEntry>,
    /// Labels sorted by name, secret values masked.
    pub labels: Vec<(String, String)>,
    /// Networks with the container's address in each.
    pub networks: Vec<(String, String)>,
    /// The whole inspect answer, masked the same way.
    pub inspect: Value,
}

fn text(v: &Value, pointer: &str) -> String {
    v.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The inspect answer with every secret-looking value masked: `Config.Env`, `Config.Labels`,
/// `Config.Cmd`, `Config.Entrypoint` and `Args`.
pub fn mask_inspect(inspect: &Value) -> Value {
    let mut v = inspect.clone();
    if let Some(env) = v.pointer_mut("/Config/Env").and_then(Value::as_array_mut) {
        for e in env.iter_mut() {
            if let Some(s) = e.as_str() {
                *e = Value::String(mask_assignment(s).0);
            }
        }
    }
    if let Some(labels) = v
        .pointer_mut("/Config/Labels")
        .and_then(Value::as_object_mut)
    {
        for (k, val) in labels.iter_mut() {
            if let Some(s) = val.as_str() {
                *val = Value::String(mask_value(k, s).0);
            }
        }
    }
    for p in ["/Config/Cmd", "/Config/Entrypoint", "/Args"] {
        if let Some(slot) = v.pointer_mut(p)
            && slot.is_array()
        {
            *slot = serde_json::json!(mask_args(&strings(Some(slot))));
        }
    }
    v
}

/// The details of an inspect answer.
pub fn details(inspect: &Value) -> ContainerDetails {
    let masked = mask_inspect(inspect);
    let env = strings(masked.pointer("/Config/Env"))
        .into_iter()
        .zip(strings(inspect.pointer("/Config/Env")))
        .map(|(shown, raw)| {
            let (name, value) = shown.split_once('=').unwrap_or((shown.as_str(), ""));
            EnvVar {
                name: name.to_owned(),
                value: value.to_owned(),
                masked: shown != raw,
            }
        })
        .collect();
    let mut ports: Vec<PortEntry> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let bindings = inspect
        .pointer("/NetworkSettings/Ports")
        .and_then(Value::as_object);
    let exposed = inspect
        .pointer("/Config/ExposedPorts")
        .and_then(Value::as_object);
    for (port, b) in bindings.into_iter().flatten() {
        seen.insert(port.clone());
        let list = b.as_array().cloned().unwrap_or_default();
        if list.is_empty() {
            ports.push(PortEntry {
                container: port.clone(),
                published: None,
                url: None,
            });
        }
        for one in list {
            let ip = one.get("HostIp").and_then(Value::as_str).unwrap_or("");
            let hp = one.get("HostPort").and_then(Value::as_str).unwrap_or("");
            let ip = if ip.is_empty() { "0.0.0.0" } else { ip };
            let published = format!("{ip}:{hp}");
            let url = crate::ports::browser_url(&format!("{published}->{port}"));
            ports.push(PortEntry {
                container: port.clone(),
                published: Some(published),
                url,
            });
        }
    }
    for port in exposed.into_iter().flatten().map(|(k, _)| k) {
        if seen.insert(port.clone()) {
            ports.push(PortEntry {
                container: port.clone(),
                published: None,
                url: None,
            });
        }
    }
    // An IPv4 and an IPv6 binding of one port open the same address once.
    ports.dedup_by(|a, b| a.container == b.container && a.url.is_some() && a.url == b.url);
    let mounts = inspect
        .get("Mounts")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .map(|m| {
                    let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
                    MountEntry {
                        kind: s("Type"),
                        source: if s("Type") == "volume" && !s("Name").is_empty() {
                            s("Name")
                        } else {
                            s("Source")
                        },
                        destination: s("Destination"),
                        mode: s("Mode"),
                        writable: m.get("RW").and_then(Value::as_bool).unwrap_or(true),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut labels: Vec<(String, String)> = masked
        .pointer("/Config/Labels")
        .and_then(Value::as_object)
        .map(|l| {
            l.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    labels.sort();
    let networks = inspect
        .pointer("/NetworkSettings/Networks")
        .and_then(Value::as_object)
        .map(|n| {
            n.iter()
                .map(|(k, v)| (k.clone(), text(v, "/IPAddress")))
                .collect()
        })
        .unwrap_or_default();
    let mut command = strings(masked.pointer("/Config/Entrypoint"));
    command.extend(strings(masked.pointer("/Config/Cmd")));
    let running = inspect.pointer("/State/Running").and_then(Value::as_bool) == Some(true);
    ContainerDetails {
        id: text(inspect, "/Id"),
        name: text(inspect, "/Name").trim_start_matches('/').to_owned(),
        image: text(inspect, "/Config/Image"),
        state: text(inspect, "/State/Status"),
        created: text(inspect, "/Created"),
        started_at: text(inspect, "/State/StartedAt"),
        exit_code: (!running)
            .then(|| inspect.pointer("/State/ExitCode").and_then(Value::as_i64))
            .flatten(),
        command,
        env,
        ports,
        mounts,
        labels,
        networks,
        inspect: masked,
    }
}

impl DockerHost {
    /// The raw inspect answer of container `id`. Callers that show or return it use
    /// [`DockerHost::container_details`] or [`mask_inspect`] instead.
    pub fn inspect_raw(&self, id: &str) -> Result<Value, DockerError> {
        let id = crate::docker::checked_id(id)?;
        self.json(&format!("/containers/{id}/json"))
    }

    /// The details of container `id`, secret-looking values masked.
    pub fn container_details(&self, id: &str) -> Result<ContainerDetails, DockerError> {
        self.inspect_raw(id).map(|v| details(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MASK;

    #[test]
    fn details_mask_secrets_everywhere() {
        let raw = serde_json::json!({
            "Id": "abc123", "Name": "/web", "Created": "2026-11-23T10:00:00Z",
            "Args": ["--api-token=t0k"],
            "State": {"Status": "running", "Running": true, "StartedAt": "2026-11-23T10:00:01Z", "ExitCode": 0},
            "Config": {
                "Image": "nginx:1.27",
                "Env": ["GREETING=hi", "DB_PASSWORD=hunter2", "URL=postgres://u:p@db/x"],
                "Labels": {"tier": "front", "deploy.token": "zzz"},
                "Cmd": ["nginx", "--password", "pw"],
                "ExposedPorts": {"80/tcp": {}, "9000/tcp": {}}
            },
            "NetworkSettings": {
                "Ports": {"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}, {"HostIp": "::", "HostPort": "8080"}], "443/tcp": null},
                "Networks": {"shop_default": {"IPAddress": "172.18.0.2"}}
            },
            "Mounts": [{"Type": "volume", "Name": "data", "Source": "/var/lib/docker/volumes/data/_data", "Destination": "/data", "Mode": "z", "RW": true},
                       {"Type": "bind", "Source": "/srv/conf", "Destination": "/etc/nginx", "Mode": "ro", "RW": false}]
        });
        let d = details(&raw);
        assert_eq!(d.name, "web");
        assert_eq!(d.exit_code, None);
        assert_eq!(
            d.env,
            [
                EnvVar {
                    name: "GREETING".into(),
                    value: "hi".into(),
                    masked: false
                },
                EnvVar {
                    name: "DB_PASSWORD".into(),
                    value: MASK.into(),
                    masked: true
                },
                EnvVar {
                    name: "URL".into(),
                    value: format!("postgres://u:{MASK}@db/x"),
                    masked: true
                },
            ]
        );
        assert_eq!(
            d.labels,
            [
                ("deploy.token".into(), MASK.into()),
                ("tier".into(), "front".into())
            ]
        );
        assert_eq!(d.command, ["nginx", "--password", MASK]);
        assert!(
            d.ports
                .iter()
                .any(|p| p.container == "80/tcp"
                    && p.url.as_deref() == Some("http://localhost:8080"))
        );
        assert_eq!(
            d.ports.iter().filter(|p| p.container == "80/tcp").count(),
            1
        );
        assert!(
            d.ports
                .iter()
                .any(|p| p.container == "443/tcp" && p.published.is_none())
        );
        assert!(d.ports.iter().any(|p| p.container == "9000/tcp"));
        assert_eq!(d.mounts[0].source, "data");
        assert!(!d.mounts[1].writable);
        assert_eq!(d.networks, [("shop_default".into(), "172.18.0.2".into())]);
        let all = d.inspect.to_string();
        for secret in ["hunter2", "t0k", "zzz", "\"pw\"", "u:p@"] {
            assert!(!all.contains(secret), "{secret} in {all}");
        }
        assert!(all.contains("GREETING=hi"));
    }
}
