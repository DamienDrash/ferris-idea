//! Published ports and Kubernetes port forwards (`SPEC-0241`): browser addresses of published
//! ports, and `kubectl port-forward` processes owned by this IDECraft process.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;

use crate::{KubeError, Kubectl};

/// Time kubectl has to print its forwarding line.
const START_TIMEOUT: Duration = Duration::from_secs(20);

/// The browser address of a published port as the container list shows it
/// (`0.0.0.0:8080->80/tcp`); `None` for unpublished or UDP ports (behaviour 1).
pub fn browser_url(port: &str) -> Option<String> {
    let (public, private) = port.split_once("->")?;
    if !private.ends_with("/tcp") {
        return None;
    }
    let (ip, number) = public.rsplit_once(':')?;
    let number: u16 = number.parse().ok()?;
    let ip = ip.trim_start_matches('[').trim_end_matches(']');
    let host = match ip {
        "" | "0.0.0.0" | "::" => "localhost".to_owned(),
        ip if ip.contains(':') => format!("[{ip}]"),
        ip => ip.to_owned(),
    };
    Some(format!("http://{host}:{number}"))
}

/// The local port for a forward of `remote`: `wanted` when given, else `remote` when it is 1024 or
/// more and free on 127.0.0.1, else one the system picks (behaviour 3).
pub fn local_port(remote: u16, wanted: Option<u16>) -> std::io::Result<u16> {
    if let Some(w) = wanted {
        return Ok(w);
    }
    if remote >= 1024 && std::net::TcpListener::bind(("127.0.0.1", remote)).is_ok() {
        return Ok(remote);
    }
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()?
        .port())
}

/// The local port in kubectl's forwarding line (`Forwarding from 127.0.0.1:8080 -> 80`).
pub fn forwarding_port(line: &str) -> Option<u16> {
    let rest = line.trim().strip_prefix("Forwarding from ")?;
    let (local, _) = rest.split_once(" -> ")?;
    local.rsplit_once(':')?.1.parse().ok()
}

/// One forward as listed (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForwardInfo {
    /// Number within this process.
    pub id: u32,
    /// Context.
    pub context: String,
    /// Namespace.
    pub namespace: String,
    /// `pod/web-1`, `service/web`, …
    pub target: String,
    /// Port on 127.0.0.1.
    pub local_port: u16,
    /// Port of the object.
    pub remote_port: u16,
    /// Whether kubectl still runs.
    pub running: bool,
    /// kubectl's last message.
    pub message: String,
}

impl ForwardInfo {
    /// Browser address of the forward.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.local_port)
    }
}

struct Forward {
    info: Arc<Mutex<ForwardInfo>>,
    child: Child,
    stdin: Option<ChildStdin>,
}

/// The forwards of this process (behaviour 5).
#[derive(Default)]
pub struct PortForwards {
    list: Mutex<Vec<Forward>>,
    next: Mutex<u32>,
}

/// kubectl's object name for a kind.
pub fn forward_target(kind: &str, name: &str) -> Result<String, KubeError> {
    let k = match kind {
        "Pod" => "pod",
        "Service" => "service",
        "Deployment" => "deployment",
        "StatefulSet" => "statefulset",
        "DaemonSet" => "daemonset",
        other => {
            return Err(KubeError::Failed(format!(
                "{other} has no ports to forward"
            )));
        }
    };
    Ok(format!("{k}/{name}"))
}

impl PortForwards {
    /// The forwards shared by the window and the MCP server of this process.
    pub fn global() -> &'static PortForwards {
        static GLOBAL: OnceLock<PortForwards> = OnceLock::new();
        GLOBAL.get_or_init(PortForwards::default)
    }

    /// The forwards, oldest first.
    pub fn list(&self) -> Vec<ForwardInfo> {
        let Ok(mut list) = self.list.lock() else {
            return Vec::new();
        };
        for f in list.iter_mut() {
            if let Ok(Some(_)) = f.child.try_wait()
                && let Ok(mut i) = f.info.lock()
            {
                i.running = false;
            }
        }
        list.iter()
            .filter_map(|f| f.info.lock().ok().map(|i| i.clone()))
            .collect()
    }

    /// Starts `kubectl port-forward` for `kind`/`name` and waits for its forwarding line
    /// (behaviour 3).
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        kubectl: &Kubectl,
        context: &str,
        namespace: &str,
        kind: &str,
        name: &str,
        remote: u16,
        local: Option<u16>,
    ) -> Result<ForwardInfo, KubeError> {
        let target = forward_target(kind, name)?;
        let local = local_port(remote, local).map_err(|e| KubeError::Failed(e.to_string()))?;
        let args = kubectl.port_forward_args(context, namespace, &target, local, remote)?;
        // A small shell ties kubectl to this process: a watcher waits for the end of the pipe on
        // its input (closed on stop or when IDECraft ends) and then ends kubectl, which replaces
        // the shell and so keeps its process id.
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"p=$$; exec 3<&0; (cat <&3 >/dev/null; kill $p) >/dev/null 2>&1 & exec "$@" 3<&-"#)
            .arg("sh")
            .arg(&kubectl.program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| KubeError::Failed(e.to_string()))?;
        let id = {
            let mut n = self
                .next
                .lock()
                .map_err(|_| KubeError::Failed("lock".into()))?;
            *n += 1;
            *n
        };
        let info = Arc::new(Mutex::new(ForwardInfo {
            id,
            context: context.to_owned(),
            namespace: namespace.to_owned(),
            target: target.clone(),
            local_port: local,
            remote_port: remote,
            running: true,
            message: String::new(),
        }));
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        // The end is reported by the last reader to finish, so a message still on its way on the
        // other pipe (kubectl writes errors to stderr) is not lost to "kubectl ended".
        let open_pipes = Arc::new(std::sync::atomic::AtomicUsize::new(
            usize::from(child.stdout.is_some()) + usize::from(child.stderr.is_some()),
        ));
        for pipe in [
            child
                .stdout
                .take()
                .map(|o| Box::new(o) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|e| Box::new(e) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let (info, tx, open_pipes) = (info.clone(), tx.clone(), open_pipes.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines() {
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    if forwarding_port(&line).is_some() {
                        let _ = tx.send(Ok(()));
                    } else if let Ok(mut i) = info.lock() {
                        i.message = line.trim().to_owned();
                    }
                }
                if open_pipes.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1 {
                    let message = info.lock().map(|i| i.message.clone()).unwrap_or_default();
                    let _ = tx.send(Err(message));
                }
            });
        }
        drop(tx);
        let started = rx.recv_timeout(START_TIMEOUT);
        let mut forward = Forward {
            info: info.clone(),
            stdin: child.stdin.take(),
            child,
        };
        match started {
            Ok(Ok(())) => {
                if let Ok(mut i) = info.lock() {
                    i.message = format!("forwarding 127.0.0.1:{local} -> {remote}");
                }
                let shown = info
                    .lock()
                    .map(|i| i.clone())
                    .unwrap_or_else(|p| p.into_inner().clone());
                if let Ok(mut list) = self.list.lock() {
                    list.push(forward);
                }
                Ok(shown)
            }
            other => {
                stop_forward(&mut forward);
                let why = match other {
                    Ok(Err(m)) if !m.is_empty() => m,
                    Ok(Err(_)) => "kubectl ended".into(),
                    _ => format!("no forwarding within {} s", START_TIMEOUT.as_secs()),
                };
                Err(KubeError::Failed(why))
            }
        }
    }

    /// Stops forward `id`; false when there is none.
    pub fn stop(&self, id: u32) -> bool {
        let Ok(mut list) = self.list.lock() else {
            return false;
        };
        let Some(at) = list
            .iter()
            .position(|f| f.info.lock().is_ok_and(|i| i.id == id))
        else {
            return false;
        };
        let mut f = list.remove(at);
        stop_forward(&mut f);
        true
    }

    /// Stops every forward.
    pub fn stop_all(&self) {
        let ids: Vec<u32> = self.list().iter().map(|f| f.id).collect();
        for id in ids {
            self.stop(id);
        }
    }
}

fn stop_forward(f: &mut Forward) {
    // Closing the shell's input ends kubectl; kill the shell if it does not end by itself.
    if let Some(mut stdin) = f.stdin.take() {
        let _ = stdin.flush();
    }
    for _ in 0..40 {
        if let Ok(Some(_)) = f.child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = f.child.kill();
    let _ = f.child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_addresses_of_published_ports() {
        assert_eq!(
            browser_url("0.0.0.0:8080->80/tcp").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            browser_url(":::8080->80/tcp").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            browser_url("127.0.0.1:5432->5432/tcp").as_deref(),
            Some("http://127.0.0.1:5432")
        );
        assert_eq!(
            browser_url("fe80::1:9000->9000/tcp").as_deref(),
            Some("http://[fe80::1]:9000")
        );
        assert_eq!(browser_url("443/tcp"), None);
        assert_eq!(browser_url("0.0.0.0:53->53/udp"), None);
    }

    #[test]
    fn local_ports_and_forwarding_lines() {
        assert_eq!(local_port(80, Some(9999)).unwrap(), 9999);
        let privileged = local_port(80, None).unwrap();
        assert!(privileged >= 1024);
        let busy = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let taken = busy.local_addr().unwrap().port();
        assert_ne!(local_port(taken, None).unwrap(), taken);
        drop(busy);
        assert_eq!(
            forwarding_port("Forwarding from 127.0.0.1:8080 -> 80"),
            Some(8080)
        );
        assert_eq!(
            forwarding_port("Forwarding from [::1]:8080 -> 80"),
            Some(8080)
        );
        assert_eq!(forwarding_port("Handling connection for 8080"), None);
        assert_eq!(forward_target("Service", "web").unwrap(), "service/web");
        assert!(forward_target("Job", "x").is_err());
    }
}
