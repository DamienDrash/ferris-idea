//! The Docker engines Ferris IDEA can use (`SPEC-0243` behaviours 1, 2, 5): `DOCKER_HOST`, the Docker
//! CLI's contexts, the sockets of local tools and of every Lima instance.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::DockerHost;

/// One engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Engine {
    /// `DOCKER_HOST`, a context name or the tool the socket belongs to.
    pub name: String,
    /// The address as written (`unix://…`, `tcp://…`, `ssh://…`).
    pub address: String,
    /// How to reach it; `None` when Ferris IDEA cannot (TLS, an unknown scheme).
    #[serde(skip)]
    pub host: Option<DockerHost>,
    /// Why it cannot be used, or a warning.
    pub note: Option<String>,
}

impl Engine {
    fn new(name: &str, address: &str, tls: bool) -> Self {
        let host = if tls {
            None
        } else {
            DockerHost::parse(address, Vec::new())
        };
        let note = if tls {
            Some("TLS is not supported yet".to_owned())
        } else if host.is_none() {
            Some("address not understood".to_owned())
        } else {
            host.as_ref()
                .and_then(DockerHost::warning)
                .map(str::to_owned)
        };
        Self {
            name: name.to_owned(),
            address: address.to_owned(),
            host,
            note,
        }
    }

    /// The engine for a known host (tests, settings).
    pub fn from_host(name: &str, host: DockerHost) -> Self {
        Self {
            name: name.to_owned(),
            address: host.to_string(),
            note: host.warning().map(str::to_owned),
            host: Some(host),
        }
    }
}

/// Sockets of local tools, in the order they are tried, with the tool's name.
pub(crate) fn local_sockets(home: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Some(home) = home {
        for (name, rel) in [
            ("Docker Desktop", ".docker/run/docker.sock"),
            ("Colima", ".colima/default/docker.sock"),
            ("OrbStack", ".orbstack/run/docker.sock"),
            ("Rancher Desktop", ".rd/docker.sock"),
            (
                "Podman",
                ".local/share/containers/podman/machine/podman.sock",
            ),
        ] {
            out.push((name.to_owned(), home.join(rel)));
        }
    }
    out.push(("system".to_owned(), PathBuf::from("/var/run/docker.sock")));
    if let Some(home) = home
        && let Ok(dir) = std::fs::read_dir(home.join(".lima"))
    {
        let mut lima: Vec<(String, PathBuf)> = dir
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('_'))
            .map(|e| {
                (
                    format!("Lima {}", e.file_name().to_string_lossy()),
                    e.path().join("sock/docker.sock"),
                )
            })
            .collect();
        lima.sort();
        out.extend(lima);
    }
    out
}

/// Contexts of the Docker CLI: the current one's name and every context's name, address and
/// whether it uses TLS.
fn contexts(docker_dir: &Path) -> (Option<String>, Vec<(String, String, bool)>) {
    let current = std::fs::read(docker_dir.join("config.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| {
            v.get("currentContext")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|c| !c.is_empty() && c != "default");
    let mut all = Vec::new();
    if let Ok(dir) = std::fs::read_dir(docker_dir.join("contexts/meta")) {
        for entry in dir.flatten() {
            let Some(meta) = std::fs::read(entry.path().join("meta.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            else {
                continue;
            };
            let (Some(name), Some(address)) = (
                meta.get("Name").and_then(Value::as_str),
                meta.pointer("/Endpoints/docker/Host")
                    .and_then(Value::as_str),
            ) else {
                continue;
            };
            let tls = docker_dir
                .join("contexts/tls")
                .join(entry.file_name())
                .join("docker")
                .is_dir();
            all.push((name.to_owned(), address.to_owned(), tls));
        }
    }
    all.sort();
    (current, all)
}

/// All engines in the order of `SPEC-0243` behaviour 1; each address once. `docker_host` and
/// `tls_verify` are `DOCKER_HOST` and `DOCKER_TLS_VERIFY`.
pub fn engines(
    docker_host: Option<&str>,
    tls_verify: Option<&str>,
    home: Option<&Path>,
) -> Vec<Engine> {
    let mut out: Vec<Engine> = Vec::new();
    let mut add = |e: Engine| {
        if !out.iter().any(|o| o.address == e.address) {
            out.push(e);
        }
    };
    if let Some(h) = docker_host.map(str::trim).filter(|h| !h.is_empty()) {
        let tls = tls_verify.is_some_and(|v| !v.is_empty() && v != "0");
        add(Engine::new("DOCKER_HOST", h, tls));
    }
    let (current, all) = home
        .map(|h| contexts(&h.join(".docker")))
        .unwrap_or_default();
    if let Some(c) = &current
        && let Some((name, address, tls)) = all.iter().find(|(n, ..)| n == c)
    {
        add(Engine::new(name, address, *tls));
    }
    for (name, path) in local_sockets(home) {
        if path.exists() {
            add(Engine::new(
                &name,
                &format!("unix://{}", path.display()),
                false,
            ));
        }
    }
    for (name, address, tls) in &all {
        add(Engine::new(name, address, *tls));
    }
    out
}

/// The engines of this process's environment and home folder.
pub fn engines_here() -> Vec<Engine> {
    engines(
        std::env::var("DOCKER_HOST").ok().as_deref(),
        std::env::var("DOCKER_TLS_VERIFY").ok().as_deref(),
        std::env::var_os("HOME").map(PathBuf::from).as_deref(),
    )
}

/// The engine named `wanted` (name or address), else the first usable one.
pub fn choose<'a>(engines: &'a [Engine], wanted: Option<&str>) -> Option<&'a Engine> {
    match wanted {
        Some(w) => engines.iter().find(|e| e.name == w || e.address == w),
        None => engines.iter().find(|e| e.host.is_some()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha_dir(home: &Path, id: &str, name: &str, address: &str) {
        let d = home.join(".docker/contexts/meta").join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("meta.json"),
            serde_json::json!({"Name": name, "Endpoints": {"docker": {"Host": address}}})
                .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn order_contexts_lima_tls_and_duplicates() {
        let home = std::env::temp_dir().join(format!("ferris-idea-engines-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".lima/docker/sock")).unwrap();
        std::fs::create_dir_all(home.join(".lima/_config")).unwrap();
        std::fs::write(home.join(".lima/docker/sock/docker.sock"), "").unwrap();
        let lima = format!(
            "unix://{}",
            home.join(".lima/docker/sock/docker.sock").display()
        );
        std::fs::write(
            home.join(".docker/config.json").tap_dir(),
            r#"{"currentContext":"build"}"#,
        )
        .unwrap();
        sha_dir(&home, "a1", "build", "ssh://ci@build.example:2222");
        sha_dir(&home, "b2", "lima", &lima);
        sha_dir(&home, "c3", "secure", "tcp://10.0.0.5:2376");
        std::fs::create_dir_all(home.join(".docker/contexts/tls/c3/docker")).unwrap();
        let list = engines(Some("tcp://192.168.1.9:2375"), None, Some(&home));
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        // The Lima socket comes once, under the tool's name (it was found before the context). The
        // machine's own `/var/run/docker.sock` comes before it when there is one (a Linux host
        // with Docker), so the expectation follows the machine.
        let mut expected = vec!["DOCKER_HOST", "build"];
        if Path::new("/var/run/docker.sock").exists() {
            expected.push("system");
        }
        expected.extend(["Lima docker", "secure"]);
        assert_eq!(names, expected);
        let secure = list.iter().find(|e| e.name == "secure").unwrap();
        assert_eq!(
            list[0].note.as_deref(),
            Some("unencrypted and unauthenticated TCP connection")
        );
        assert_eq!(
            list[1].host,
            Some(DockerHost::Ssh {
                destination: "ci@build.example".into(),
                port: Some(2222),
                extra: vec![],
                program: None,
            })
        );
        assert_eq!(secure.host, None);
        assert_eq!(secure.note.as_deref(), Some("TLS is not supported yet"));
        assert_eq!(
            choose(&list, Some("secure")).unwrap().address,
            "tcp://10.0.0.5:2376"
        );
        assert_eq!(choose(&list, None).unwrap().name, "DOCKER_HOST");
        assert!(choose(&list, Some("nope")).is_none());
        let tls = engines(Some("tcp://10.0.0.5:2376"), Some("1"), None);
        assert_eq!(tls[0].host, None);
        let _ = std::fs::remove_dir_all(home);
    }

    trait TapDir {
        fn tap_dir(self) -> Self;
    }
    impl TapDir for PathBuf {
        fn tap_dir(self) -> Self {
            if let Some(p) = self.parent() {
                std::fs::create_dir_all(p).unwrap();
            }
            self
        }
    }
}
