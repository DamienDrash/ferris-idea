//! Hosts from the user's SSH configuration (behaviour 1).

use std::path::{Path, PathBuf};

use serde::Serialize;

/// A host the user configured, with what `ssh -G` resolves it to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostInfo {
    /// Alias from `Host`.
    pub alias: String,
    /// Login user.
    pub user: Option<String>,
    /// Real host name.
    pub hostname: Option<String>,
    /// Port.
    pub port: Option<u16>,
}

fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if Path::new(path).is_absolute() => PathBuf::from(path),
        None => home.join(".ssh").join(path),
    }
}

/// Files an `Include` names: plain paths, or a `*` in the last part.
fn included(pattern: &str, home: &Path) -> Vec<PathBuf> {
    let path = expand(pattern, home);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !name.contains('*') {
        return vec![path];
    }
    let (before, after) = name.split_once('*').unwrap_or((&name, ""));
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .is_some_and(|n| n.starts_with(before) && n.ends_with(after))
        })
        .collect();
    out.sort();
    out
}

fn collect(file: &Path, home: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > 8 {
        return;
    }
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        let (key, rest) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, r)) => (k, r.trim_start_matches([' ', '\t', '='])),
            None => continue,
        };
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                for alias in rest.split_whitespace() {
                    if !alias.contains(['*', '?', '!']) && !out.iter().any(|a| a == alias) {
                        out.push(alias.to_owned());
                    }
                }
            }
            "include" => {
                for pattern in rest.split_whitespace() {
                    for f in included(pattern, home) {
                        collect(&f, home, depth + 1, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Host aliases of `~/.ssh/config` and the files it includes, in file order; patterns are left out.
pub fn config_hosts(home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect(&home.join(".ssh/config"), home, 0, &mut out);
    out
}

/// What `ssh -G` makes of `alias` (no connection is made), with the shared options, so it shows
/// what a connection would use.
pub fn describe(alias: &str, extra: &[String]) -> HostInfo {
    let mut info = HostInfo {
        alias: alias.to_owned(),
        user: None,
        hostname: None,
        port: None,
    };
    let Ok(line) = crate::SshCommand::new(crate::SshPurpose::Describe)
        .args(["-G"])
        .extra(extra)
        .build(alias)
    else {
        return info;
    };
    let Ok(out) = line.command().stdin(std::process::Stdio::null()).output() else {
        return info;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        match line.split_once(' ') {
            Some(("user", v)) => info.user = Some(v.to_owned()),
            Some(("hostname", v)) => info.hostname = Some(v.to_owned()),
            Some(("port", v)) => info.port = v.parse().ok(),
            _ => {}
        }
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_from_config_and_includes() {
        let home = std::env::temp_dir().join(format!("idecraft-sshcfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".ssh/config.d")).unwrap();
        std::fs::write(
            home.join(".ssh/config"),
            "Include config.d/*.conf\n\nHost dev box\n  HostName 10.0.0.5\nHost *.internal !x\n  User me\nHost=build\n",
        )
        .unwrap();
        std::fs::write(home.join(".ssh/config.d/a.conf"), "Host staging\n").unwrap();
        std::fs::write(home.join(".ssh/config.d/b.txt"), "Host ignored\n").unwrap();
        assert_eq!(config_hosts(&home), ["staging", "dev", "box", "build"]);
        let _ = std::fs::remove_dir_all(&home);
        assert!(config_hosts(Path::new("/no/such/home")).is_empty());
    }
}
