//! Hosts from the user's SSH configuration (behaviour 1) and Ferris IDEA's own (`SPEC-0121` v5
//! behaviour 1b), each with its source.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where a listed host comes from (`SPEC-0121` v5 behaviour 1b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HostSource {
    /// The user's SSH configuration (`~/.ssh/config`, or a `-F` file); Ferris IDEA never changes it.
    #[default]
    SshConfig,
    /// Ferris IDEA's own host list, which the window and `remote_host_add` change.
    #[serde(rename = "ferris-idea")]
    FerrisIdea,
}

impl HostSource {
    /// How the window names the source.
    pub fn label(self) -> &'static str {
        match self {
            Self::SshConfig => "~/.ssh/config",
            Self::FerrisIdea => "Ferris IDEA",
        }
    }
}

/// A host the user configured, with what `ssh -G` resolves it to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct HostInfo {
    /// Alias from `Host`.
    pub alias: String,
    /// Login user.
    pub user: Option<String>,
    /// Real host name.
    pub hostname: Option<String>,
    /// Port.
    pub port: Option<u16>,
    /// Where the host comes from (v5).
    pub source: HostSource,
    /// The configuration file it comes from (v5).
    pub config: Option<String>,
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
    crate::testing::check_config_read(file);
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
    config_hosts_in(&home.join(".ssh/config"), home)
}

/// Host aliases of the configuration `file` and the files it includes (relative ones in
/// `home`'s `.ssh`, as `ssh` does), in file order; patterns are left out.
pub fn config_hosts_in(file: &Path, home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect(file, home, 0, &mut out);
    out
}

/// The configuration `ssh` reads with the extra arguments `extra`: the file of a `-F <file>` among
/// them, else `~/.ssh/config` of `home`. The host list follows it, so the list shows exactly the
/// hosts a connection with `extra` can reach by alias. Product windows pass no `-F` (the user's
/// own configuration); tests pass a configuration of their own.
pub fn ssh_config_file(extra: &[String], home: &Path) -> PathBuf {
    match crate::jump::config_file(extra) {
        Some(f) => PathBuf::from(f),
        None => home.join(".ssh/config"),
    }
}

/// The hosts of the configuration [`ssh_config_file`] names for `extra` (home folder from `HOME`),
/// each with what `ssh -G` resolves for it and its source: the one source of the Remote Host
/// window's and the `remote_hosts` tool's host list. Without a `-F` among `extra` and with
/// Ferris IDEA's own files set ([`crate::set_host_files`]), Ferris IDEA's own hosts come first, then
/// those of the user's `~/.ssh/config`; an alias in both is listed once, as Ferris IDEA's
/// (`SPEC-0121` v5 behaviour 1b). A `-F` file replaces both lists (tests).
pub fn host_list(extra: &[String]) -> Vec<HostInfo> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let files = crate::own_hosts::host_files();
    list_hosts(extra, files.as_ref(), &home, describe)
}

/// [`host_list`] with Ferris IDEA's files `files`, the home folder `home` and `describe` for what
/// `ssh -G` resolves.
fn list_hosts(
    extra: &[String],
    files: Option<&crate::HostFiles>,
    home: &Path,
    describe: impl Fn(&str, &[String]) -> HostInfo,
) -> Vec<HostInfo> {
    let files = files.filter(|_| crate::jump::config_file(extra).is_none());
    let mut out: Vec<HostInfo> = Vec::new();
    let (user_file, user_home) = match files {
        Some(files) => {
            let own = files.own.display().to_string();
            for h in crate::own_hosts::own_hosts(files) {
                let mut info = describe(&h.alias, extra);
                info.source = HostSource::FerrisIdea;
                info.config = Some(own.clone());
                out.push(info);
            }
            let user_home = files
                .user
                .parent()
                .and_then(Path::parent)
                .unwrap_or(home)
                .to_path_buf();
            (files.user.clone(), user_home)
        }
        None => (ssh_config_file(extra, home), home.to_path_buf()),
    };
    let config = user_file.display().to_string();
    // Resolved as a connection reaches them: without `-F` from the user's `~/.ssh/config`; a user
    // file set elsewhere (tests) is named with `-F`.
    let user_extra: Vec<String> = match files {
        Some(f) if f.user != home.join(".ssh/config") => {
            let mut e = vec!["-F".to_owned(), config.clone()];
            e.extend(extra.iter().cloned());
            e
        }
        _ => extra.to_vec(),
    };
    for alias in config_hosts_in(&user_file, &user_home) {
        if out.iter().any(|h| h.alias == alias) {
            continue;
        }
        let mut info = describe(&alias, &user_extra);
        info.source = HostSource::SshConfig;
        info.config = Some(config.clone());
        out.push(info);
    }
    out
}

/// What `ssh -G` makes of `alias` (no connection is made), with the shared options, so it shows
/// what a connection would use.
pub fn describe(alias: &str, extra: &[String]) -> HostInfo {
    let mut info = HostInfo {
        alias: alias.to_owned(),
        ..HostInfo::default()
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
        let home = std::env::temp_dir().join(format!("ferris-idea-sshcfg-{}", std::process::id()));
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

    #[test]
    fn the_file_of_dash_f_replaces_the_users_config() {
        let home = Path::new("/home/u");
        assert_eq!(ssh_config_file(&[], home), home.join(".ssh/config"));
        let extra = [
            "-o".to_owned(),
            "X=y".to_owned(),
            "-F".into(),
            "/t/cfg".into(),
        ];
        assert_eq!(ssh_config_file(&extra, home), PathBuf::from("/t/cfg"));
        assert_eq!(
            ssh_config_file(&["-F/t/other".to_owned()], home),
            PathBuf::from("/t/other")
        );
        let dir = std::env::temp_dir().join(format!("ferris-idea-sshcfg-f-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cfg"), "Host only-this\n").unwrap();
        let file = dir.join("cfg");
        assert_eq!(config_hosts_in(&file, home), ["only-this"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The guard of `crate::testing::isolate_ssh`: a test's own configuration lies in the
    /// temporary folder; the user's `~/.ssh/config` never does.
    #[test]
    fn a_test_config_lies_in_the_temporary_folder() {
        use crate::testing::is_test_config;
        let tmp = std::env::temp_dir().join("ferris-idea-x/ssh_config");
        assert!(is_test_config(&tmp));
        assert!(is_test_config(Path::new("/tmp/idc-remote-1/ssh_config")));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
            && !test_roots_hold(&home)
        {
            assert!(!is_test_config(&home.join(".ssh/config")));
        }
        assert!(!is_test_config(Path::new("relative/ssh_config")));
        assert!(!is_test_config(
            &std::env::temp_dir().join("../etc/ssh_config")
        ));
    }

    /// Ferris IDEA's hosts first, then the user's, each with its source; an alias in both once; a
    /// `-F` file replaces both (`SPEC-0121` v5 behaviour 1b).
    #[test]
    fn the_list_names_each_hosts_source() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-sshcfg-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = crate::HostFiles::in_folders(&dir.join("settings"), &dir.join("home"));
        std::fs::create_dir_all(dir.join("home/.ssh")).unwrap();
        std::fs::write(&files.user, "Host theirs both\n").unwrap();
        crate::add_host(
            &files,
            &crate::OwnHost {
                alias: "mine".into(),
                hostname: "10.0.0.9".into(),
                ..Default::default()
            },
            false,
        )
        .unwrap();
        // "both" is in the user's file only; written into Ferris IDEA's by hand it is listed once.
        let text = std::fs::read_to_string(&files.own).unwrap();
        std::fs::write(
            &files.own,
            text.replace("Match all", "Host both\n  HostName b\n\nMatch all"),
        )
        .unwrap();
        let stub = |alias: &str, _: &[String]| HostInfo {
            alias: alias.to_owned(),
            ..HostInfo::default()
        };
        let list = list_hosts(&[], Some(&files), Path::new("/nowhere"), stub);
        let got: Vec<(&str, HostSource)> =
            list.iter().map(|h| (h.alias.as_str(), h.source)).collect();
        assert_eq!(
            got,
            [
                ("mine", HostSource::FerrisIdea),
                ("both", HostSource::FerrisIdea),
                ("theirs", HostSource::SshConfig)
            ]
        );
        assert_eq!(
            list[0].config.as_deref(),
            Some(files.own.display().to_string().as_str())
        );
        assert_eq!(
            list[2].config.as_deref(),
            Some(files.user.display().to_string().as_str())
        );
        let json = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(json["source"], "ferris-idea");
        assert_eq!(
            serde_json::to_value(&list[2]).unwrap()["source"],
            "ssh_config"
        );
        std::fs::write(dir.join("other"), "Host only\n").unwrap();
        let f = ["-F".to_owned(), dir.join("other").display().to_string()];
        let list = list_hosts(&f, Some(&files), Path::new("/nowhere"), stub);
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].alias.as_str(), list[0].source),
            ("only", HostSource::SshConfig)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn test_roots_hold(home: &Path) -> bool {
        home.starts_with(std::env::temp_dir()) || home.starts_with("/tmp")
    }
}
