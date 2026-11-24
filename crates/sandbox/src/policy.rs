//! The sandbox policy and its validation (`SPEC-0347` behaviours 1–4): the folders a process may
//! read, the folders it may write, the hosts it may reach. Only a [`ValidPolicy`] can be launched.

use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Most roots (read and write together) a policy may have.
pub const MAX_ROOTS: usize = 32;
/// Most host entries a policy may have.
pub const MAX_HOSTS: usize = 64;

/// A policy as given: paths and host entries as text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Folders or files the process may read.
    #[serde(default)]
    pub read: Vec<PathBuf>,
    /// Folders or files the process may read and change.
    #[serde(default)]
    pub write: Vec<PathBuf>,
    /// Destinations the process may reach (behaviour 3).
    #[serde(default)]
    pub hosts: Vec<String>,
}

/// One problem of a policy: the entry it is about and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// The entry as given.
    pub entry: String,
    /// What is wrong with it.
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.entry, self.message)
    }
}

/// What a host entry names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostName {
    /// Exactly this name (lower case, no trailing dot).
    Exact(String),
    /// Any name ending in `.` followed by this domain, not the domain itself.
    Suffix(String),
    /// Exactly this address.
    Ip(IpAddr),
}

/// A validated host entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRule {
    /// The name, domain or address.
    pub name: HostName,
    /// The port.
    pub port: u16,
}

impl HostRule {
    /// Whether a destination given as `host` (lower case, without brackets) and `port` matches.
    /// A name rule never matches an address and an address rule never matches a name.
    pub fn matches(&self, host: &str, port: u16) -> bool {
        if port != self.port {
            return false;
        }
        let host = host.trim_end_matches('.');
        match (&self.name, host.parse::<IpAddr>()) {
            (HostName::Ip(ip), Ok(dest)) => *ip == dest,
            (HostName::Ip(_), Err(_)) | (_, Ok(_)) => false,
            (HostName::Exact(name), Err(_)) => host.eq_ignore_ascii_case(name),
            (HostName::Suffix(domain), Err(_)) => {
                let host = host.to_ascii_lowercase();
                host.len() > domain.len() + 1
                    && host.ends_with(domain.as_str())
                    && host.as_bytes()[host.len() - domain.len() - 1] == b'.'
            }
        }
    }
}

impl fmt::Display for HostRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            HostName::Exact(n) => write!(f, "{n}:{}", self.port),
            HostName::Suffix(d) => write!(f, "*.{d}:{}", self.port),
            HostName::Ip(IpAddr::V4(ip)) => write!(f, "{ip}:{}", self.port),
            HostName::Ip(IpAddr::V6(ip)) => write!(f, "[{ip}]:{}", self.port),
        }
    }
}

/// A policy that passed validation: real paths, no duplicates, parsed hosts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidPolicy {
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
    hosts: Vec<HostRule>,
}

impl ValidPolicy {
    /// The read roots (real paths), without those that are also write roots.
    pub fn read(&self) -> &[PathBuf] {
        &self.read
    }

    /// The write roots (real paths).
    pub fn write(&self) -> &[PathBuf] {
        &self.write
    }

    /// The host entries.
    pub fn hosts(&self) -> &[HostRule] {
        &self.hosts
    }

    /// The policy in its given form with the resolved values, as the Linux launcher reads it.
    pub fn to_policy(&self) -> Policy {
        Policy {
            read: self.read.clone(),
            write: self.write.clone(),
            hosts: self.hosts.iter().map(ToString::to_string).collect(),
        }
    }
}

impl Policy {
    /// Validates the policy (behaviours 2–4). `home` is the user's home folder; a root that is
    /// it or above it is refused. Every problem is answered at once.
    pub fn validate(&self, home: Option<&Path>) -> Result<ValidPolicy, Vec<Problem>> {
        let mut problems = Vec::new();
        if self.read.len() + self.write.len() > MAX_ROOTS {
            problems.push(Problem {
                entry: format!("{} roots", self.read.len() + self.write.len()),
                message: format!("a policy has at most {MAX_ROOTS} roots"),
            });
        }
        let home = home.map(|h| std::fs::canonicalize(h).unwrap_or_else(|_| h.to_path_buf()));
        let mut write = Vec::new();
        for p in &self.write {
            match root(p, home.as_deref()) {
                Ok(r) if !write.contains(&r) => write.push(r),
                Ok(_) => {}
                Err(m) => problems.push(Problem {
                    entry: p.display().to_string(),
                    message: m,
                }),
            }
        }
        let mut read = Vec::new();
        for p in &self.read {
            match root(p, home.as_deref()) {
                Ok(r) if !write.contains(&r) && !read.contains(&r) => read.push(r),
                Ok(_) => {}
                Err(m) => problems.push(Problem {
                    entry: p.display().to_string(),
                    message: m,
                }),
            }
        }
        if self.hosts.len() > MAX_HOSTS {
            problems.push(Problem {
                entry: format!("{} hosts", self.hosts.len()),
                message: format!("a policy has at most {MAX_HOSTS} hosts"),
            });
        }
        let mut hosts = Vec::new();
        for h in &self.hosts {
            match host_rule(h) {
                Ok(r) if !hosts.contains(&r) => hosts.push(r),
                Ok(_) => {}
                Err(m) => problems.push(Problem {
                    entry: h.clone(),
                    message: m,
                }),
            }
        }
        if problems.is_empty() {
            Ok(ValidPolicy { read, write, hosts })
        } else {
            Err(problems)
        }
    }
}

/// One root: absolute, existing, resolved, not `/`, not the home folder or above it.
fn root(p: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    let text = p.to_string_lossy();
    if text
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\\')
    {
        return Err("a root must not contain a control character, `\"` or `\\`".into());
    }
    if !p.is_absolute() {
        return Err("a root must be an absolute path".into());
    }
    let real = std::fs::canonicalize(p).map_err(|e| format!("cannot be resolved: {e}"))?;
    let real_text = real.to_string_lossy();
    if real_text
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\\')
    {
        return Err(format!(
            "resolves to {}, which contains a control character, `\"` or `\\`",
            real.display()
        ));
    }
    if real.parent().is_none() {
        return Err("the whole file system cannot be a root".into());
    }
    if let Some(home) = home
        && home.starts_with(&real)
    {
        return Err(format!(
            "{} is the home folder or contains it; name the folders below it instead",
            real.display()
        ));
    }
    Ok(real)
}

/// One host entry (behaviour 3).
fn host_rule(entry: &str) -> Result<HostRule, String> {
    let e = entry.trim();
    if e.is_empty() {
        return Err("empty host".into());
    }
    if e.contains("://") || e.contains('/') {
        return Err("a host entry is a name or an address, not a URL or a path".into());
    }
    // `[v6]` or `[v6]:port`.
    if let Some(rest) = e.strip_prefix('[') {
        let (addr, after) = rest
            .split_once(']')
            .ok_or("an IPv6 address needs its closing `]`")?;
        let ip: std::net::Ipv6Addr = addr.parse().map_err(|_| "not an IPv6 address")?;
        let port = match after {
            "" => 443,
            p => port(p.strip_prefix(':').ok_or("expected `:port` after `]`")?)?,
        };
        return Ok(HostRule {
            name: HostName::Ip(IpAddr::V6(ip)),
            port,
        });
    }
    if e.parse::<std::net::Ipv6Addr>().is_ok() {
        return Err("write an IPv6 address in brackets: `[address]` or `[address]:port`".into());
    }
    let (name, port) = match e.rsplit_once(':') {
        Some((n, p)) => (n, port(p)?),
        None => (e, 443),
    };
    if let Ok(ip) = name.parse::<std::net::Ipv4Addr>() {
        return Ok(HostRule {
            name: HostName::Ip(IpAddr::V4(ip)),
            port,
        });
    }
    let lower = name.trim_end_matches('.').to_ascii_lowercase();
    let (wild, domain) = match lower.strip_prefix("*.") {
        Some(d) => (true, d.to_string()),
        None => (false, lower.clone()),
    };
    if domain.is_empty() || domain.len() > 253 {
        return Err("a name has 1 to 253 characters".into());
    }
    for label in domain.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err("every label of a name has 1 to 63 characters".into());
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(
                "a name has only letters, digits, `-` and `.`; `*.` only in front of a domain"
                    .into(),
            );
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("a label does not start or end with `-`".into());
        }
    }
    if wild {
        if domain.split('.').count() < 2 {
            return Err(
                "`*.` needs a domain of at least two labels, such as `*.example.com`".into(),
            );
        }
        return Ok(HostRule {
            name: HostName::Suffix(domain),
            port,
        });
    }
    Ok(HostRule {
        name: HostName::Exact(domain),
        port,
    })
}

fn port(p: &str) -> Result<u16, String> {
    match p.parse::<u16>() {
        Ok(0) | Err(_) => Err(format!("`{p}` is no port (1 to 65535)")),
        Ok(n) => Ok(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems(p: &Policy, home: Option<&Path>) -> Vec<Problem> {
        p.validate(home).expect_err("refused")
    }

    /// SPEC-0347 AK-1: roots are refused with their entry; links are resolved, duplicates merged.
    #[test]
    fn roots_are_checked_and_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let work = base.join("work");
        let home = base.join("home");
        std::fs::create_dir_all(work.join("sub")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let quoted = base.join("a\"b");
        std::fs::create_dir_all(&quoted).unwrap();
        let bad = Policy {
            read: vec![
                PathBuf::from("relative/path"),
                base.join("missing"),
                PathBuf::from("/"),
                quoted,
            ],
            write: vec![home.clone(), base.clone()],
            hosts: vec![],
        };
        let found = problems(&bad, Some(&home));
        let entries: Vec<&str> = found.iter().map(|p| p.entry.as_str()).collect();
        assert_eq!(found.len(), 6, "{found:?}");
        assert!(entries.contains(&"relative/path"));
        assert!(entries.contains(&"/"));
        assert!(
            found
                .iter()
                .filter(|p| p.message.contains("home folder"))
                .count()
                == 2,
            "{found:?}"
        );
        let link = base.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&work, &link).unwrap();
        #[cfg(unix)]
        {
            let ok = Policy {
                read: vec![work.join("sub"), link.clone()],
                write: vec![link.clone(), work.clone()],
                hosts: vec![],
            }
            .validate(Some(&home))
            .unwrap();
            assert_eq!(ok.write(), std::slice::from_ref(&work));
            assert_eq!(ok.read(), [work.join("sub")]);
        }
        let many = Policy {
            read: vec![work.clone(); MAX_ROOTS + 1],
            ..Policy::default()
        };
        assert!(problems(&many, None)[0].message.contains("at most 32"));
    }

    /// SPEC-0347 AK-1: host entries.
    #[test]
    fn hosts_are_parsed_and_refused() {
        let ok = Policy {
            hosts: vec![
                "api.example.com".into(),
                "*.Example.com:8443".into(),
                "10.1.2.3:80".into(),
                "[2001:db8::1]".into(),
                "API.example.com.".into(),
            ],
            ..Policy::default()
        }
        .validate(None)
        .unwrap();
        let shown: Vec<String> = ok.hosts().iter().map(ToString::to_string).collect();
        assert_eq!(
            shown,
            [
                "api.example.com:443",
                "*.example.com:8443",
                "10.1.2.3:80",
                "[2001:db8::1]:443"
            ]
        );
        for bad in [
            "*",
            "*.com",
            "a*b.example.com",
            "https://example.com",
            "example.com/x",
            "example.com:0",
            "example.com:70000",
            "2001:db8::1",
            "-a.example.com",
            "",
        ] {
            let p = Policy {
                hosts: vec![bad.into()],
                ..Policy::default()
            };
            assert_eq!(problems(&p, None).len(), 1, "{bad}");
        }
        let many = Policy {
            hosts: (0..=MAX_HOSTS)
                .map(|i| format!("h{i}.example.com"))
                .collect(),
            ..Policy::default()
        };
        assert!(
            problems(&many, None)
                .iter()
                .any(|p| p.message.contains("at most 64"))
        );
    }

    #[test]
    fn host_rules_match_names_domains_and_addresses() {
        let p = Policy {
            hosts: vec![
                "api.example.com".into(),
                "*.example.org".into(),
                "127.0.0.1:9".into(),
            ],
            ..Policy::default()
        }
        .validate(None)
        .unwrap();
        let any = |h: &str, port: u16| p.hosts().iter().any(|r| r.matches(h, port));
        assert!(any("API.example.com", 443));
        assert!(!any("api.example.com", 80));
        assert!(!any("x.api.example.com", 443));
        assert!(any("a.b.example.org", 443));
        assert!(!any("example.org", 443));
        assert!(!any("badexample.org", 443));
        assert!(any("127.0.0.1", 9));
        assert!(!any("localhost", 9));
        assert!(!any("127.0.0.2", 9));
    }
}
