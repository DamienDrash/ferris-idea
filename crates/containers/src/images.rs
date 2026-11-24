//! Images (`SPEC-0119` v6 behaviour 13, gap D3 of OBS-0116): pulling with progress and the
//! resulting digest, a warning for references without a tag or with `latest`, removing that is
//! refused while a container uses the image, and an image's details with the containers that
//! use it.

use serde::Serialize;
use serde_json::Value;

use crate::stream::{StopFlag, read_lines};
use crate::{Container, DockerError, DockerHost};

/// An image reference split into the repository and its tag or digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageRef {
    /// Repository with registry and path, e.g. `busybox` or `ghcr.io/org/app`.
    pub repository: String,
    /// Tag (`1.36.1`), `None` when the reference names none.
    pub tag: Option<String>,
    /// Digest (`sha256:…`), when the reference pins one.
    pub digest: Option<String>,
}

impl ImageRef {
    /// Reads a reference as `docker pull` takes it; `None` when it is not one (empty, spaces,
    /// characters outside a reference, an option-like start).
    pub fn parse(reference: &str) -> Option<Self> {
        let r = reference.trim();
        if r.is_empty()
            || r.len() > 512
            || r.starts_with(['-', '/', ':', '@', '.'])
            || !r
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c))
        {
            return None;
        }
        let (name, digest) = match r.split_once('@') {
            Some((n, d)) => {
                let (algo, hex) = d.split_once(':')?;
                if algo.is_empty() || hex.len() < 32 || !hex.chars().all(|c| c.is_ascii_hexdigit())
                {
                    return None;
                }
                (n, Some(d.to_owned()))
            }
            None => (r, None),
        };
        // A tag follows the last `:` after the last `/` (a `:` before it is a registry port).
        let slash = name.rfind('/').map_or(0, |i| i + 1);
        let (repository, tag) = match name[slash..].rfind(':') {
            Some(i) => (&name[..slash + i], Some(name[slash + i + 1..].to_owned())),
            None => (name, None),
        };
        if repository.is_empty()
            || repository.ends_with('/')
            || tag
                .as_deref()
                .is_some_and(|t| t.is_empty() || t.len() > 128)
        {
            return None;
        }
        Some(Self {
            repository: repository.to_owned(),
            tag,
            digest,
        })
    }

    /// The reference as it is pulled: with its digest, else its tag, else `latest`.
    pub fn full(&self) -> String {
        match (&self.digest, &self.tag) {
            (Some(d), _) => format!("{}@{d}", self.repository),
            (None, Some(t)) => format!("{}:{t}", self.repository),
            (None, None) => format!("{}:latest", self.repository),
        }
    }

    /// Why the reference may not give the same image next time: no tag, or `latest`.
    pub fn warning(&self) -> Option<String> {
        if self.digest.is_some() {
            return None;
        }
        match self.tag.as_deref() {
            None => Some(format!(
                "{} has no tag: Docker pulls `latest`, which can change; name a version",
                self.repository
            )),
            Some("latest") => Some(format!(
                "{}:latest can change from one pull to the next; name a version",
                self.repository
            )),
            _ => None,
        }
    }
}

/// What a pull ended with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullResult {
    /// The reference pulled.
    pub reference: String,
    /// The digest the registry named, if any.
    pub digest: Option<String>,
    /// The engine's last status line, e.g. `Downloaded newer image for busybox:1.36.1`.
    pub status: String,
    /// The warning of [`ImageRef::warning`].
    pub warning: Option<String>,
}

/// One progress line of a pull: the layer (if any) and its status, as `layer: status progress`.
fn progress_line(v: &Value) -> Option<String> {
    let status = v.get("status").and_then(Value::as_str)?;
    let id = v.get("id").and_then(Value::as_str);
    let bar = v.get("progress").and_then(Value::as_str).unwrap_or("");
    Some(match id {
        Some(id) => format!("{id}: {status} {bar}").trim_end().to_owned(),
        None => status.to_owned(),
    })
}

/// Percent-encodes a query value (only the characters a reference may hold pass unchanged).
fn query_value(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

/// Details of one image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageDetails {
    /// Full id (`sha256:…`).
    pub id: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Repository digests.
    pub digests: Vec<String>,
    /// Size in bytes.
    pub size: u64,
    /// `os/architecture[/variant]`.
    pub platform: String,
    /// When it was built (RFC 3339).
    pub created: String,
    /// Names of the containers that use it, with their state.
    pub containers: Vec<(String, String)>,
}

/// The containers of `list` that use the image with id `id` (full or short) or one of `tags`.
pub fn users(list: &[Container], id: &str, tags: &[String]) -> Vec<(String, String)> {
    let short = crate::docker::short_id(id);
    list.iter()
        .filter(|c| {
            (!c.image_id.is_empty() && c.image_id == short)
                || tags.contains(&c.image)
                || c.image == short
        })
        .map(|c| (c.name.clone(), c.state.clone()))
        .collect()
}

impl DockerHost {
    /// Pulls `reference`; `on` gets each progress line. Fails with the registry's message.
    pub fn pull(
        &self,
        reference: &str,
        stop: &StopFlag,
        mut on: impl FnMut(&str),
    ) -> Result<PullResult, DockerError> {
        let r = ImageRef::parse(reference)
            .ok_or_else(|| DockerError::Parse(format!("not an image reference: {reference:?}")))?;
        let warning = r.warning();
        let tag = r
            .digest
            .clone()
            .or_else(|| r.tag.clone())
            .unwrap_or_else(|| "latest".into());
        let path = format!(
            "/images/create?fromImage={}&tag={}",
            query_value(&r.repository),
            query_value(&tag)
        );
        let stream = self.open_stream("POST", &path)?;
        let mut digest = None;
        let mut status = String::new();
        let mut failure = None;
        read_lines(stream, stop, |line| {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                return;
            };
            if let Some(e) = v
                .pointer("/errorDetail/message")
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
            {
                failure = Some(e.to_owned());
                return;
            }
            if let Some(s) = v.get("status").and_then(Value::as_str) {
                if let Some(d) = s.strip_prefix("Digest: ") {
                    digest = Some(d.trim().to_owned());
                }
                if let Some(rest) = s.strip_prefix("Status: ") {
                    status = rest.to_owned();
                }
            }
            if let Some(p) = progress_line(&v) {
                on(&p);
            }
        })
        .map_err(|e| DockerError::Connect(self.to_string(), e.to_string()))?;
        if let Some(e) = failure {
            return Err(DockerError::Http(500, e));
        }
        if stop.is_stopped() {
            return Err(DockerError::Parse("pull cancelled".into()));
        }
        Ok(PullResult {
            reference: r.full(),
            digest,
            status,
            warning,
        })
    }

    /// The details of image `name` (tag or id).
    pub fn image_details(&self, name: &str) -> Result<ImageDetails, DockerError> {
        let r = checked_image(name)?;
        let v = self.json(&format!("/images/{r}/json"))?;
        let s = |p: &str| {
            v.pointer(p)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        let list = |p: &str| -> Vec<String> {
            v.pointer(p)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut platform = format!("{}/{}", s("/Os"), s("/Architecture"));
        if !s("/Variant").is_empty() {
            platform.push('/');
            platform.push_str(&s("/Variant"));
        }
        let id = s("/Id");
        let tags = list("/RepoTags");
        let containers = users(&self.containers()?, &id, &tags);
        Ok(ImageDetails {
            id,
            tags,
            digests: list("/RepoDigests"),
            size: v.get("Size").and_then(Value::as_u64).unwrap_or(0),
            platform,
            created: s("/Created"),
            containers,
        })
    }

    /// Removes image `name` (tag or id) without force. While a container uses it, nothing is
    /// asked of the engine and the answer names the containers. Returns the engine's lines
    /// (`Untagged: …`, `Deleted: …`).
    pub fn remove_image(&self, name: &str) -> Result<Vec<String>, DockerError> {
        let r = checked_image(name)?;
        let v = self.json(&format!("/images/{r}/json"))?;
        let id = v.get("Id").and_then(Value::as_str).unwrap_or("").to_owned();
        let tags: Vec<String> = v
            .get("RepoTags")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let using = users(&self.containers()?, &id, &tags);
        if !using.is_empty() {
            let names: Vec<String> = using.iter().map(|(n, s)| format!("{n} ({s})")).collect();
            return Err(DockerError::Http(
                409,
                format!(
                    "{name} is used by {}: remove {} first",
                    names.join(", "),
                    if using.len() == 1 { "it" } else { "them" }
                ),
            ));
        }
        let body = self.call("DELETE", &format!("/images/{r}"))?;
        let v: Value =
            serde_json::from_slice(&body).map_err(|e| DockerError::Parse(e.to_string()))?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_object())
                    .flat_map(|o| o.iter())
                    .filter_map(|(k, v)| v.as_str().map(|v| format!("{k}: {v}")))
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// An image name or id for a request path, percent-encoded; refused when it is not a reference.
fn checked_image(name: &str) -> Result<String, DockerError> {
    let ok = !name.is_empty()
        && !name.starts_with(['-', '.', '/'])
        && name.len() <= 512
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c))
        && !name.contains("..");
    if !ok {
        return Err(DockerError::Parse(format!(
            "not an image name or id: {name:?}"
        )));
    }
    // `/` stays (the engine takes repository paths as they are); `:` and `@` are encoded.
    Ok(name.replace(':', "%3A").replace('@', "%40"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_tags_digests_and_warnings() {
        let r = ImageRef::parse("busybox:1.36.1").unwrap();
        assert_eq!(
            (r.repository.as_str(), r.tag.as_deref()),
            ("busybox", Some("1.36.1"))
        );
        assert_eq!(r.warning(), None);
        let r = ImageRef::parse("localhost:5000/team/app").unwrap();
        assert_eq!(
            (r.repository.as_str(), r.tag.as_deref()),
            ("localhost:5000/team/app", None)
        );
        assert_eq!(r.full(), "localhost:5000/team/app:latest");
        assert!(r.warning().unwrap().contains("has no tag"));
        assert!(
            ImageRef::parse("nginx:latest")
                .unwrap()
                .warning()
                .unwrap()
                .contains(":latest can change")
        );
        let d = format!("alpine@sha256:{}", "a".repeat(64));
        let r = ImageRef::parse(&d).unwrap();
        assert_eq!(r.full(), d);
        assert_eq!(r.warning(), None, "a digest pins the image");
        for bad in [
            "",
            "-x",
            "a b",
            "x:",
            "/x",
            "alpine@sha256:zz",
            "x?y=1",
            "a&b",
        ] {
            assert!(ImageRef::parse(bad).is_none(), "{bad}");
        }
        assert_eq!(query_value("ghcr.io/o/a"), "ghcr.io%2Fo%2Fa");
        assert_eq!(checked_image("nginx:1.27").unwrap(), "nginx%3A1.27");
        assert!(checked_image("../x").is_err());
        assert!(checked_image("x?force=1").is_err());
    }

    #[test]
    fn progress_lines_and_users() {
        let v: Value = serde_json::from_str(
            r#"{"status":"Downloading","progressDetail":{"current":1,"total":2},"progress":"[=>  ] 1B/2B","id":"abc"}"#,
        )
        .unwrap();
        assert_eq!(progress_line(&v).unwrap(), "abc: Downloading [=>  ] 1B/2B");
        let c = |name: &str, image: &str, id: &str| Container {
            id: name.into(),
            name: name.into(),
            image: image.into(),
            image_id: id.into(),
            state: "running".into(),
            status: String::new(),
            ports: vec![],
            compose_project: None,
            compose_service: None,
            compose_dir: None,
            networks: vec![],
        };
        let list = [
            c("web", "nginx:1.27", "cccccccccccc"),
            c("db", "postgres:18", "dddddddddddd"),
        ];
        assert_eq!(
            users(&list, "sha256:cccccccccccc3333", &[]),
            [("web".to_owned(), "running".to_owned())]
        );
        assert_eq!(users(&list, "x", &["postgres:18".into()]).len(), 1);
        assert!(users(&list, "x", &["redis:7".into()]).is_empty());
    }
}
