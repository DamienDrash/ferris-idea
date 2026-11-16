//! Kubernetes through `kubectl` (`SPEC-0119` behaviours 4–6): every call is `kubectl … -o json`
//! (or `logs`) with an explicit context and namespace, without a shell or a terminal, ended after
//! a timeout.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

/// Time a `kubectl` call may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Why a call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KubeError {
    /// `kubectl` is not installed.
    #[error("kubectl is not installed (https://kubernetes.io/docs/tasks/tools/)")]
    Missing,
    /// `kubectl` failed; its message.
    #[error("kubectl: {0}")]
    Failed(String),
    /// The output could not be read.
    #[error("unexpected output from kubectl: {0}")]
    Parse(String),
}

/// A context of the kubeconfig.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KubeContext {
    /// Context name.
    pub name: String,
    /// Cluster it points to.
    pub cluster: String,
    /// Default namespace (`default` when unset).
    pub namespace: String,
    /// Whether it is the current context.
    pub current: bool,
}

/// One workload or service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KubeResource {
    /// `Pod`, `Deployment`, `StatefulSet`, `DaemonSet`, `Job`, `Service`.
    pub kind: String,
    /// Name.
    pub name: String,
    /// Namespace.
    pub namespace: String,
    /// Phase or condition, e.g. `Running`, `CrashLoopBackOff`, `Available`, `ClusterIP`.
    pub status: String,
    /// Ready count as `ready/wanted` (pods: containers; workloads: replicas), if it applies.
    pub ready: Option<String>,
    /// Container restarts (pods).
    pub restarts: Option<u64>,
    /// Age, e.g. `5m`, `3h`, `2d`.
    pub age: String,
    /// More detail: images of a pod, ports of a service.
    pub detail: String,
    /// TCP ports: of a pod's containers, a workload's pod template, a service (`SPEC-0241`).
    pub ports: Vec<u16>,
}

/// The `kubectl` program to call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kubectl {
    /// Executable.
    pub program: PathBuf,
}

/// Seconds since the epoch of an RFC 3339 UTC time (`2026-11-01T10:00:00Z`).
fn epoch_seconds(time: &str) -> Option<i64> {
    let (date, clock) = time.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut c = clock
        .split(':')
        .map(|p| p.split('.').next().unwrap_or(p).parse::<i64>());
    let (h, min, s) = (c.next()?.ok()?, c.next()?.ok()?, c.next()?.ok()?);
    // Days from the civil date (proleptic Gregorian), Howard Hinnant's algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + s)
}

/// Age of `created` at `now` (seconds since the epoch) the way `kubectl get` shows it.
pub fn age(created: &str, now: i64) -> String {
    let Some(t) = epoch_seconds(created) else {
        return "?".into();
    };
    let s = (now - t).max(0);
    match s {
        0..=119 => format!("{s}s"),
        120..=7199 => format!("{}m", s / 60),
        7200..=172_799 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn resource(item: &Value, now: i64) -> KubeResource {
    let kind = item
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let meta = item.get("metadata").cloned().unwrap_or(Value::Null);
    let name = meta
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let namespace = meta
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let age = age(
        meta.get("creationTimestamp")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        now,
    );
    let spec = item.get("spec").cloned().unwrap_or(Value::Null);
    let st = item.get("status").cloned().unwrap_or(Value::Null);
    let n = |v: &Value, k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    let (status, ready, restarts, detail) = match kind.as_str() {
        "Pod" => {
            let statuses = st
                .get("containerStatuses")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let waiting = statuses.iter().find_map(|c| {
                c.pointer("/state/waiting/reason")
                    .or_else(|| c.pointer("/state/terminated/reason"))
                    .and_then(Value::as_str)
            });
            let phase = st.get("phase").and_then(Value::as_str).unwrap_or("Unknown");
            let deleting = meta.get("deletionTimestamp").is_some();
            let status = if deleting {
                "Terminating".to_owned()
            } else {
                waiting.unwrap_or(phase).to_owned()
            };
            let wanted = spec
                .get("containers")
                .and_then(Value::as_array)
                .map_or(statuses.len(), Vec::len);
            let ready = statuses
                .iter()
                .filter(|c| c.get("ready").and_then(Value::as_bool) == Some(true))
                .count();
            let images: Vec<&str> = spec
                .get("containers")
                .and_then(Value::as_array)
                .map(|cs| {
                    cs.iter()
                        .filter_map(|c| c.get("image").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default();
            (
                status,
                Some(format!("{ready}/{wanted}")),
                Some(statuses.iter().map(|c| n(c, "restartCount")).sum()),
                images.join(", "),
            )
        }
        "Deployment" | "StatefulSet" => {
            let wanted = spec.get("replicas").and_then(Value::as_u64).unwrap_or(1);
            let ready = n(&st, "readyReplicas");
            let status = if ready >= wanted {
                "Available"
            } else {
                "Progressing"
            };
            (
                status.to_owned(),
                Some(format!("{ready}/{wanted}")),
                None,
                String::new(),
            )
        }
        "DaemonSet" => {
            let wanted = n(&st, "desiredNumberScheduled");
            let ready = n(&st, "numberReady");
            let status = if ready >= wanted {
                "Available"
            } else {
                "Progressing"
            };
            (
                status.to_owned(),
                Some(format!("{ready}/{wanted}")),
                None,
                String::new(),
            )
        }
        "Job" => {
            let wanted = spec.get("completions").and_then(Value::as_u64).unwrap_or(1);
            let done = n(&st, "succeeded");
            let status = if n(&st, "failed") > 0 && done < wanted {
                "Failed"
            } else if done >= wanted {
                "Complete"
            } else {
                "Running"
            };
            (
                status.to_owned(),
                Some(format!("{done}/{wanted}")),
                None,
                String::new(),
            )
        }
        "Service" => {
            let kind = spec
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("ClusterIP");
            let ip = spec
                .get("clusterIP")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let ports: Vec<String> = spec
                .get("ports")
                .and_then(Value::as_array)
                .map(|ps| {
                    ps.iter()
                        .map(|p| {
                            let port = n(p, "port");
                            let proto = p.get("protocol").and_then(Value::as_str).unwrap_or("TCP");
                            match p.get("nodePort").and_then(Value::as_u64) {
                                Some(node) => format!("{port}:{node}/{proto}"),
                                None => format!("{port}/{proto}"),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            (
                kind.to_owned(),
                None,
                None,
                format!("{ip} {}", ports.join(",")).trim().to_owned(),
            )
        }
        _ => (String::new(), None, None, String::new()),
    };
    let containers = match kind.as_str() {
        "Pod" => spec.get("containers"),
        "Job" => spec.pointer("/template/spec/containers"),
        _ => spec.pointer("/template/spec/containers"),
    };
    let mut ports: Vec<u16> = if kind == "Service" {
        spec.get("ports")
            .and_then(Value::as_array)
            .map(|ps| {
                ps.iter()
                    .filter(|p| p.get("protocol").and_then(Value::as_str).unwrap_or("TCP") == "TCP")
                    .filter_map(|p| p.get("port").and_then(Value::as_u64))
                    .filter_map(|p| u16::try_from(p).ok())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        containers
            .and_then(Value::as_array)
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| c.get("ports").and_then(Value::as_array))
                    .flatten()
                    .filter(|p| p.get("protocol").and_then(Value::as_str).unwrap_or("TCP") == "TCP")
                    .filter_map(|p| p.get("containerPort").and_then(Value::as_u64))
                    .filter_map(|p| u16::try_from(p).ok())
                    .collect()
            })
            .unwrap_or_default()
    };
    ports.dedup();
    KubeResource {
        kind,
        name,
        namespace,
        status,
        ready,
        restarts,
        age,
        detail,
        ports,
    }
}

/// kubectl's object for a kind that runs containers (`SPEC-0240` behaviour 3).
pub fn exec_object(kind: &str, name: &str) -> Result<String, KubeError> {
    checked(name)?;
    match kind {
        "Pod" => Ok(name.to_owned()),
        "Deployment" | "StatefulSet" | "DaemonSet" | "Job" => {
            Ok(format!("{}/{name}", kind.to_lowercase()))
        }
        other => Err(KubeError::Failed(format!("{other} runs no containers"))),
    }
}

/// `name` is a valid Kubernetes object, context or namespace name for an argument.
fn checked(name: &str) -> Result<&str, KubeError> {
    if !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:@/".contains(c))
    {
        Ok(name)
    } else {
        Err(KubeError::Failed(format!("not a valid name: {name:?}")))
    }
}

impl Kubectl {
    /// `kubectl` on `PATH`, if installed.
    pub fn find(path_var: Option<&std::ffi::OsStr>) -> Option<Self> {
        let path = path_var?;
        std::env::split_paths(path)
            .map(|d| {
                d.join(if cfg!(windows) {
                    "kubectl.exe"
                } else {
                    "kubectl"
                })
            })
            .find(|p| p.is_file())
            .map(|program| Self { program })
    }

    /// Runs `kubectl args…` with no input; stdout on success, stderr's last line as the error.
    fn run(&self, args: &[&str]) -> Result<String, KubeError> {
        let mut child = Command::new(&self.program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => KubeError::Missing,
                _ => KubeError::Failed(e.to_string()),
            })?;
        // Read both pipes on threads so a large answer cannot block the child.
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let out_thread = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(o) = out.as_mut() {
                let _ = std::io::Read::read_to_string(o, &mut s);
            }
            s
        });
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(e) = err.as_mut() {
                let _ = std::io::Read::read_to_string(e, &mut s);
            }
            s
        });
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|e| KubeError::Failed(e.to_string()))?
            {
                break status;
            }
            if started.elapsed() > TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(KubeError::Failed(format!(
                    "no answer within {} s (is the cluster reachable?)",
                    TIMEOUT.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();
        if status.success() {
            Ok(stdout)
        } else {
            let line = stderr
                .lines()
                .rfind(|l| !l.trim().is_empty())
                .unwrap_or("failed")
                .to_owned();
            Err(KubeError::Failed(line))
        }
    }

    fn json(&self, args: &[&str]) -> Result<Value, KubeError> {
        let out = self.run(args)?;
        serde_json::from_str(&out).map_err(|e| KubeError::Parse(e.to_string()))
    }

    /// Contexts of the kubeconfig (`kubectl config view`, secrets redacted by kubectl).
    pub fn contexts(&self) -> Result<Vec<KubeContext>, KubeError> {
        let v = self.json(&["config", "view", "-o", "json"])?;
        let current = v
            .get("current-context")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Ok(v.get("contexts")
            .and_then(Value::as_array)
            .map(|cs| {
                cs.iter()
                    .map(|c| {
                        let name = c.get("name").and_then(Value::as_str).unwrap_or_default();
                        let s =
                            |k: &str| c.pointer(&format!("/context/{k}")).and_then(Value::as_str);
                        KubeContext {
                            name: name.to_owned(),
                            cluster: s("cluster").unwrap_or_default().to_owned(),
                            namespace: s("namespace").unwrap_or("default").to_owned(),
                            current: name == current,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Namespaces of `context`.
    pub fn namespaces(&self, context: &str) -> Result<Vec<String>, KubeError> {
        let v = self.json(&[
            "get",
            "namespaces",
            "-o",
            "json",
            "--context",
            checked(context)?,
        ])?;
        let mut names: Vec<String> = v
            .get("items")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.pointer("/metadata/name").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        Ok(names)
    }

    /// Workloads and services in `namespace` of `context`, grouped by kind, then by name.
    pub fn resources(
        &self,
        context: &str,
        namespace: &str,
    ) -> Result<Vec<KubeResource>, KubeError> {
        let v = self.json(&[
            "get",
            "deployments,statefulsets,daemonsets,jobs,pods,services",
            "-o",
            "json",
            "--context",
            checked(context)?,
            "--namespace",
            checked(namespace)?,
        ])?;
        let now = now();
        let order = [
            "Deployment",
            "StatefulSet",
            "DaemonSet",
            "Job",
            "Pod",
            "Service",
        ];
        let mut out: Vec<KubeResource> = v
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| KubeError::Parse("no `items`".into()))?
            .iter()
            .map(|i| resource(i, now))
            .collect();
        out.sort_by_key(|r| {
            (
                order
                    .iter()
                    .position(|k| *k == r.kind)
                    .unwrap_or(order.len()),
                r.name.clone(),
            )
        });
        Ok(out)
    }

    /// The last `tail` lines of all containers of pod `pod`.
    pub fn logs(
        &self,
        context: &str,
        namespace: &str,
        pod: &str,
        tail: usize,
    ) -> Result<String, KubeError> {
        let tail = format!("--tail={tail}");
        self.run(&[
            "logs",
            checked(pod)?,
            "--all-containers=true",
            "--prefix=false",
            &tail,
            "--context",
            checked(context)?,
            "--namespace",
            checked(namespace)?,
        ])
    }

    /// Restarts a workload: `rollout restart` for deployments, stateful and daemon sets, deleting
    /// the pod (its controller makes a new one) for pods (behaviour 6).
    pub fn restart(
        &self,
        context: &str,
        namespace: &str,
        kind: &str,
        name: &str,
    ) -> Result<String, KubeError> {
        let (context, namespace, name) = (checked(context)?, checked(namespace)?, checked(name)?);
        let target = match kind {
            "Deployment" => format!("deployment/{name}"),
            "StatefulSet" => format!("statefulset/{name}"),
            "DaemonSet" => format!("daemonset/{name}"),
            "Pod" => {
                return self.run(&[
                    "delete",
                    "pod",
                    name,
                    "--wait=false",
                    "--context",
                    context,
                    "--namespace",
                    namespace,
                ]);
            }
            other => return Err(KubeError::Failed(format!("{other} cannot be restarted"))),
        };
        self.run(&[
            "rollout",
            "restart",
            &target,
            "--context",
            context,
            "--namespace",
            namespace,
        ])
    }

    /// Arguments of `kubectl exec` (`SPEC-0240` behaviours 3, 5): with `tty` for an interactive
    /// shell, else for one command.
    pub fn exec_args(
        &self,
        context: &str,
        namespace: &str,
        object: &str,
        container: Option<&str>,
        command: &[String],
        tty: bool,
    ) -> Result<Vec<String>, KubeError> {
        if command.is_empty() {
            return Err(KubeError::Failed("empty command".into()));
        }
        let mut args = vec!["exec".to_owned()];
        if tty {
            args.push("-it".into());
        }
        args.push(checked(object)?.to_owned());
        if let Some(c) = container {
            args.extend(["-c".into(), checked(c)?.to_owned()]);
        }
        args.extend([
            "--context".into(),
            checked(context)?.to_owned(),
            "--namespace".into(),
            checked(namespace)?.to_owned(),
            "--".into(),
        ]);
        args.extend(command.iter().cloned());
        Ok(args)
    }

    /// Runs one command in a pod without a TTY: output (stdout, then stderr), exit code and
    /// whether output was cut (`SPEC-0240` behaviour 5).
    pub fn exec(&self, args: &[String], timeout: Duration) -> Result<crate::ExecOutput, KubeError> {
        let mut child = Command::new(&self.program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => KubeError::Missing,
                _ => KubeError::Failed(e.to_string()),
            })?;
        let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(p) = pipe {
                    let _ = std::io::Read::read_to_end(
                        &mut std::io::Read::take(p, crate::EXEC_LIMIT as u64 + 1),
                        &mut buf,
                    );
                }
                buf
            })
        };
        let out = read(child.stdout.take().map(|o| Box::new(o) as _));
        let err = read(child.stderr.take().map(|e| Box::new(e) as _));
        let started = Instant::now();
        let status = loop {
            if let Some(s) = child
                .try_wait()
                .map_err(|e| KubeError::Failed(e.to_string()))?
            {
                break Some(s);
            }
            if started.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut bytes = out.join().unwrap_or_default();
        bytes.extend(err.join().unwrap_or_default());
        let truncated = bytes.len() > crate::EXEC_LIMIT;
        bytes.truncate(crate::EXEC_LIMIT);
        Ok(crate::ExecOutput {
            output: String::from_utf8_lossy(&bytes).into_owned(),
            exit_code: status.and_then(|s| s.code()).map(i64::from),
            truncated,
        })
    }

    /// Arguments of `kubectl port-forward` on 127.0.0.1 (`SPEC-0241` behaviour 3).
    pub fn port_forward_args(
        &self,
        context: &str,
        namespace: &str,
        target: &str,
        local: u16,
        remote: u16,
    ) -> Result<Vec<String>, KubeError> {
        Ok(vec![
            "port-forward".into(),
            checked(target)?.to_owned(),
            format!("{local}:{remote}"),
            "--address".into(),
            "127.0.0.1".into(),
            "--context".into(),
            checked(context)?.to_owned(),
            "--namespace".into(),
            checked(namespace)?.to_owned(),
        ])
    }

    /// The command line of a call, for previews.
    pub fn describe(&self, args: &[&str]) -> Vec<String> {
        std::iter::once(self.program.display().to_string())
            .chain(args.iter().map(|a| (*a).to_owned()))
            .collect()
    }
}

/// Whether `path` looks like a `kubectl` executable (for tests and settings).
pub fn is_kubectl(path: &Path) -> bool {
    path.file_stem().is_some_and(|s| s == "kubectl") && path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_like_kubectl() {
        let t = epoch_seconds("2026-11-01T10:00:00Z").unwrap();
        assert_eq!(epoch_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_seconds("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(age("2026-11-01T10:00:00Z", t + 59), "59s");
        assert_eq!(age("2026-11-01T10:00:00Z", t + 600), "10m");
        assert_eq!(age("2026-11-01T10:00:00.5Z", t + 3 * 3600), "3h");
        assert_eq!(age("2026-11-01T10:00:00Z", t + 5 * 86_400), "5d");
        assert_eq!(age("garbage", t), "?");
    }

    #[test]
    fn pods_workloads_and_services() {
        let now = epoch_seconds("2026-11-01T12:00:00Z").unwrap();
        let pod = resource(
            &serde_json::json!({
                "kind": "Pod",
                "metadata": {"name": "web-7d9", "namespace": "shop", "creationTimestamp": "2026-11-01T11:00:00Z"},
                "spec": {"containers": [{"image": "nginx:1.27"}, {"image": "envoy:1.30"}]},
                "status": {"phase": "Running", "containerStatuses": [
                    {"ready": true, "restartCount": 1, "state": {"running": {}}},
                    {"ready": false, "restartCount": 4, "state": {"waiting": {"reason": "CrashLoopBackOff"}}}
                ]}
            }),
            now,
        );
        assert_eq!(pod.status, "CrashLoopBackOff");
        assert_eq!(pod.ready.as_deref(), Some("1/2"));
        assert_eq!(pod.restarts, Some(5));
        assert_eq!(pod.age, "60m");
        assert_eq!(pod.detail, "nginx:1.27, envoy:1.30");
        let deploy = resource(
            &serde_json::json!({"kind": "Deployment", "metadata": {"name": "web"},
                "spec": {"replicas": 3}, "status": {"readyReplicas": 2}}),
            now,
        );
        assert_eq!(
            (deploy.status.as_str(), deploy.ready.as_deref()),
            ("Progressing", Some("2/3"))
        );
        let svc = resource(
            &serde_json::json!({"kind": "Service", "metadata": {"name": "web"},
                "spec": {"type": "NodePort", "clusterIP": "10.0.0.7",
                         "ports": [{"port": 80, "nodePort": 30080, "protocol": "TCP"}]}}),
            now,
        );
        assert_eq!(
            (svc.status.as_str(), svc.detail.as_str()),
            ("NodePort", "10.0.0.7 80:30080/TCP")
        );
        let job = resource(
            &serde_json::json!({"kind": "Job", "metadata": {"name": "migrate"},
                "spec": {"completions": 1}, "status": {"failed": 2}}),
            now,
        );
        assert_eq!(job.status, "Failed");
    }

    #[test]
    fn ports_exec_and_forward_arguments() {
        let pod = resource(
            &serde_json::json!({"kind": "Pod", "metadata": {"name": "p"},
                "spec": {"containers": [{"ports": [{"containerPort": 80}, {"containerPort": 53, "protocol": "UDP"}]},
                                        {"ports": [{"containerPort": 9090, "protocol": "TCP"}]}]}}),
            0,
        );
        assert_eq!(pod.ports, [80, 9090]);
        let dep = resource(
            &serde_json::json!({"kind": "Deployment", "metadata": {"name": "d"},
                "spec": {"template": {"spec": {"containers": [{"ports": [{"containerPort": 8080}]}]}}}}),
            0,
        );
        assert_eq!(dep.ports, [8080]);
        let svc = resource(
            &serde_json::json!({"kind": "Service", "metadata": {"name": "s"},
                "spec": {"ports": [{"port": 80, "protocol": "TCP"}, {"port": 53, "protocol": "UDP"}]}}),
            0,
        );
        assert_eq!(svc.ports, [80]);
        let k = Kubectl {
            program: "kubectl".into(),
        };
        assert_eq!(
            k.exec_args("dev", "shop", "deployment/web", None, &["sh".into()], true)
                .unwrap()
                .join(" "),
            "exec -it deployment/web --context dev --namespace shop -- sh"
        );
        assert_eq!(
            k.exec_args(
                "dev",
                "shop",
                "web-1",
                Some("app"),
                &["ls".into(), "-l".into()],
                false
            )
            .unwrap()
            .join(" "),
            "exec web-1 -c app --context dev --namespace shop -- ls -l"
        );
        assert!(
            k.exec_args("dev", "shop", "--raw", None, &["x".into()], false)
                .is_err()
        );
        assert!(k.exec_args("dev", "shop", "web", None, &[], false).is_err());
        assert_eq!(exec_object("Job", "j").unwrap(), "job/j");
        assert!(exec_object("Service", "s").is_err());
        assert_eq!(
            k.port_forward_args("dev", "shop", "service/web", 8080, 80)
                .unwrap()
                .join(" "),
            "port-forward service/web 8080:80 --address 127.0.0.1 --context dev --namespace shop"
        );
    }

    #[test]
    fn names_are_checked_before_they_become_arguments() {
        assert!(checked("web-7d9").is_ok());
        assert!(checked("arn:aws:eks:eu-central-1:1:cluster/prod").is_ok());
        assert!(checked("--kubeconfig=/etc/x").is_err());
        assert!(checked("a b").is_err());
        assert!(checked("").is_err());
    }
}
