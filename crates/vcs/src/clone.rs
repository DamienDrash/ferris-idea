//! Clone Repository and New Project of the project popup (`SPEC-0094` v5 behaviours 13 and 14):
//! the address and target checks, `git clone` with progress and cancelling, and an empty project
//! folder with an optional `git init`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::GitError;
use crate::remote::{auth_failure, git_cmd};

/// How a repository address reaches its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CloneTransport {
    /// `https://host/path`.
    Https,
    /// `ssh://[user@]host[:port]/path` or `user@host:path`.
    Ssh,
}

/// A checked repository address: only HTTPS and SSH, never credentials (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneSource {
    /// The address as given (it holds no password or token).
    pub url: String,
    /// HTTPS or SSH.
    pub transport: CloneTransport,
    /// The host name.
    pub host: String,
    /// The folder name the clone gets by default: the last path part without `.git`.
    pub folder_name: String,
}

/// Checks a repository address. Accepted are `https://host/path` without any user information,
/// query or fragment, `ssh://[user@]host[:port]/path` and `user@host:path` with a user name but
/// no password. Everything else — `http`, `git`, `file`, `ext::`, local paths, an address or host
/// that starts with `-`, blanks and control characters — is refused with the reason.
pub fn check_clone_url(url: &str) -> Result<CloneSource, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("Enter the repository's address.".into());
    }
    if url.starts_with('-') || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The address must not start with `-` or hold blanks.".into());
    }
    let credentials = || {
        Err(
            "The address must not hold a password or token; the system's credential helper \
             or your SSH key gives them."
                .to_owned(),
        )
    };
    let (transport, host, path) = if let Some(rest) = url.strip_prefix("https://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.contains('@') {
            return credentials();
        }
        if url.contains('?') || url.contains('#') {
            return Err("The address must not have a query or fragment.".into());
        }
        let host = authority.split(':').next().unwrap_or_default();
        (CloneTransport::Https, host.to_owned(), path.to_owned())
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host_port = match authority.rsplit_once('@') {
            Some((user, _)) if user.contains(':') => return credentials(),
            Some((_, h)) => h,
            None => authority,
        };
        let host = host_port.split(':').next().unwrap_or_default();
        (CloneTransport::Ssh, host.to_owned(), path.to_owned())
    } else if !url.contains("://")
        && let Some((user_host, path)) = url.split_once(':')
        && let Some((user, host)) = user_host.rsplit_once('@')
        && !user.is_empty()
        && !user_host.contains('/')
    {
        if path.starts_with('-') {
            return Err("The path must not start with `-`.".into());
        }
        (CloneTransport::Ssh, host.to_owned(), path.to_owned())
    } else {
        return Err(
            "Only HTTPS (https://host/path) and SSH (ssh://host/path or user@host:path) \
             addresses are cloned."
                .into(),
        );
    };
    if host.is_empty()
        || host.starts_with('-')
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_[]:".contains(c))
    {
        return Err("The address names no valid host.".into());
    }
    let folder_name = folder_name(&path);
    if folder_name.is_empty() {
        return Err("The address names no repository.".into());
    }
    Ok(CloneSource {
        url: url.to_owned(),
        transport,
        host,
        folder_name,
    })
}

/// The last part of `path` without `.git`, with every character that cannot stand in a folder
/// name replaced by `-`.
fn folder_name(path: &str) -> String {
    let last = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let last = last.strip_suffix(".git").unwrap_or(last);
    let name: String = last
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    if name.chars().all(|c| c == '.') {
        String::new()
    } else {
        name
    }
}

/// Checks the folder a clone or a new project is made in: an absolute path that does not exist
/// yet, in a folder that does. Nothing is ever written into a folder that is there already.
pub fn check_new_folder(target: &Path) -> Result<(), String> {
    if !target.is_absolute() {
        return Err(format!("{} is not an absolute path.", target.display()));
    }
    if target.file_name().is_none() {
        return Err(format!("{} names no folder.", target.display()));
    }
    if std::fs::symlink_metadata(target).is_ok() {
        return Err(format!(
            "{} exists already; choose a new folder.",
            target.display()
        ));
    }
    match target.parent() {
        Some(parent) if parent.is_dir() => Ok(()),
        _ => Err(format!(
            "The folder {} does not exist.",
            target.parent().unwrap_or(target).display()
        )),
    }
}

/// One step of a clone's progress, from git's `--progress` lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneProgress {
    /// What git does: "Receiving objects", "Resolving deltas", "Updating files", …
    pub phase: String,
    /// How far, when git says.
    pub percent: Option<u8>,
}

/// The progress in one line of git's error output (`Receiving objects:  45% (450/1000)`).
pub fn parse_progress(line: &str) -> Option<CloneProgress> {
    let line = line.trim().trim_start_matches("remote:").trim();
    if line.starts_with("Cloning into") {
        return Some(CloneProgress {
            phase: "Cloning".into(),
            percent: None,
        });
    }
    let (phase, rest) = line.split_once(':')?;
    if phase.is_empty() || !phase.chars().all(|c| c.is_alphabetic() || c == ' ') {
        return None;
    }
    let percent = rest
        .trim()
        .split_once('%')
        .and_then(|(n, _)| n.trim().parse::<u8>().ok());
    Some(CloneProgress {
        phase: phase.to_owned(),
        percent: Some(percent?),
    })
}

/// The end of a clone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneOutcome {
    /// The folder of the new repository.
    pub folder: PathBuf,
    /// git's last messages.
    pub messages: String,
}

/// The arguments of `git clone` for `source` into `target`: only the HTTPS and SSH transports
/// (also for submodules and redirects), no submodules, the progress on the error output.
pub fn clone_args(source: &CloneSource, target: &Path) -> Vec<String> {
    vec![
        "-c".into(),
        "protocol.allow=never".into(),
        "-c".into(),
        "protocol.https.allow=always".into(),
        "-c".into(),
        "protocol.ssh.allow=always".into(),
        "clone".into(),
        "--progress".into(),
        "--no-recurse-submodules".into(),
        "--".into(),
        source.url.clone(),
        target.to_string_lossy().into_owned(),
    ]
}

/// Clones `source` into the new folder `target` (behaviour 13). `progress` gets every step;
/// setting `cancel` or reaching `timeout` ends git and removes the folder again. Credentials
/// come only from the credential helper or the SSH agent: nothing is asked, host keys are
/// checked strictly (ADR-0025).
pub fn clone_repository(
    source: &CloneSource,
    target: &Path,
    cancel: &AtomicBool,
    timeout: Duration,
    mut progress: impl FnMut(CloneProgress),
) -> Result<CloneOutcome, GitError> {
    check_new_folder(target).map_err(GitError::Rejected)?;
    let parent = target.parent().unwrap_or(target);
    let mut cmd = git_cmd(parent);
    cmd.args(clone_args(source, target));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| GitError::Git(format!("cannot start git: {e}")))?;
    let (lines, received) = std::sync::mpsc::channel::<String>();
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while let Some(pipe) = stderr.as_mut() {
            match pipe.read(&mut byte) {
                Ok(1) if byte[0] == b'\r' || byte[0] == b'\n' => {
                    if !line.is_empty() {
                        let _ = lines.send(String::from_utf8_lossy(&line).into_owned());
                        line.clear();
                    }
                }
                Ok(1) => line.push(byte[0]),
                _ => break,
            }
        }
        if !line.is_empty() {
            let _ = lines.send(String::from_utf8_lossy(&line).into_owned());
        }
    });
    let started = Instant::now();
    let mut messages: Vec<String> = Vec::new();
    let mut take = |received: &std::sync::mpsc::Receiver<String>| {
        while let Ok(l) = received.try_recv() {
            match parse_progress(&l) {
                Some(p) => progress(p),
                None => {
                    messages.push(l);
                    if messages.len() > 20 {
                        messages.remove(0);
                    }
                }
            }
        }
    };
    let stopped = loop {
        take(&received);
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    None
                } else {
                    Some("failed")
                };
            }
            Ok(None) if cancel.load(Ordering::SeqCst) => break Some("cancelled"),
            Ok(None) if started.elapsed() >= timeout => break Some("timed out"),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(GitError::Git(e.to_string())),
        }
    };
    if matches!(stopped, Some("cancelled" | "timed out")) {
        #[cfg(unix)]
        if let Ok(pid) = i32::try_from(child.id()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = reader.join();
    take(&received);
    let text = messages.join("\n");
    match stopped {
        None => Ok(CloneOutcome {
            folder: target.to_path_buf(),
            messages: text,
        }),
        Some(why) => {
            // The folder was new: what git left of it goes again.
            let _ = std::fs::remove_dir_all(target);
            let hint = if auth_failure(&text) {
                " The host refused the credentials or its key is unknown: add the key to \
                 ~/.ssh/known_hosts or store the credentials in the credential helper first."
            } else {
                ""
            };
            Err(GitError::Rejected(format!(
                "The clone {why}; nothing was kept.{hint}\n{text}"
            )))
        }
    }
}

/// Makes the empty project folder `target`, with `git init` when `git` (behaviour 14).
pub fn new_project(target: &Path, git: bool) -> Result<(), GitError> {
    check_new_folder(target).map_err(GitError::Rejected)?;
    std::fs::create_dir(target)
        .map_err(|e| GitError::Rejected(format!("{}: {e}", target.display())))?;
    if git {
        let out = git_cmd(target)
            .args(["init", "--quiet"])
            .output()
            .map_err(|e| GitError::Git(format!("cannot start git: {e}")))?;
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(target);
            return Err(GitError::Rejected(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0094 v5 AK-11: HTTPS and SSH only, no credentials, no options in disguise.
    #[test]
    fn only_https_and_ssh_addresses_without_credentials_are_cloned() {
        let ok = check_clone_url("https://github.com/acme/shop.git").unwrap();
        assert_eq!(ok.transport, CloneTransport::Https);
        assert_eq!(
            (ok.host.as_str(), ok.folder_name.as_str()),
            ("github.com", "shop")
        );
        let scp = check_clone_url("git@github.com:acme/shop-api.git").unwrap();
        assert_eq!(scp.transport, CloneTransport::Ssh);
        assert_eq!(scp.folder_name, "shop-api");
        let ssh = check_clone_url("ssh://git@example.org:2222/srv/repos/tools/").unwrap();
        assert_eq!(
            (ssh.host.as_str(), ssh.folder_name.as_str()),
            ("example.org", "tools")
        );
        for refused in [
            "https://user:token@github.com/acme/shop.git",
            "https://token@github.com/acme/shop.git",
            "ssh://git:secret@example.org/x.git",
        ] {
            let e = check_clone_url(refused).unwrap_err();
            assert!(e.contains("password or token"), "{refused}: {e}");
            assert!(!e.contains("token@") && !e.contains("secret"), "{e}");
        }
        for refused in [
            "http://github.com/acme/shop.git",
            "git://github.com/acme/shop.git",
            "file:///tmp/repo",
            "/tmp/repo",
            "ext::sh -c touch% /tmp/pwned",
            "--upload-pack=touch /tmp/x",
            "https://-oProxyCommand=x/repo",
            "git@-oProxyCommand=x:repo",
            "git@host:-repo",
            "https://github.com/acme/shop.git?access_token=1",
            "https://github.com/",
            "",
        ] {
            assert!(check_clone_url(refused).is_err(), "{refused}");
        }
    }

    /// SPEC-0094 v5 AK-11: only new folders in an existing one.
    #[test]
    fn a_clone_or_new_project_needs_a_new_folder() {
        let dir = std::env::temp_dir().join(format!("idecraft-clone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(check_new_folder(&dir.join("fresh")).is_ok());
        assert!(
            check_new_folder(&dir)
                .unwrap_err()
                .contains("exists already")
        );
        assert!(check_new_folder(&dir.join("no/such")).is_err());
        assert!(check_new_folder(Path::new("relative/x")).is_err());
        new_project(&dir.join("plain"), false).unwrap();
        assert!(dir.join("plain").is_dir() && !dir.join("plain/.git").exists());
        new_project(&dir.join("with-git"), true).unwrap();
        assert!(dir.join("with-git/.git").is_dir());
        assert!(new_project(&dir.join("plain"), false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0094 v5 AK-11: git's progress lines and the arguments that keep other transports out.
    #[test]
    fn progress_lines_and_clone_arguments() {
        assert_eq!(
            parse_progress("Receiving objects:  45% (450/1000), 1.2 MiB | 2 MiB/s"),
            Some(CloneProgress {
                phase: "Receiving objects".into(),
                percent: Some(45)
            })
        );
        assert_eq!(
            parse_progress("remote: Counting objects: 100% (12/12), done.")
                .unwrap()
                .percent,
            Some(100)
        );
        assert_eq!(
            parse_progress("Cloning into '/tmp/x'...").unwrap().percent,
            None
        );
        assert_eq!(parse_progress("fatal: repository not found"), None);
        let source = check_clone_url("https://example.org/a/b.git").unwrap();
        let args = clone_args(&source, Path::new("/tmp/b"));
        assert_eq!(
            args[..6],
            [
                "-c",
                "protocol.allow=never",
                "-c",
                "protocol.https.allow=always",
                "-c",
                "protocol.ssh.allow=always"
            ]
        );
        assert!(args.contains(&"--no-recurse-submodules".to_owned()));
        assert_eq!(
            args[args.len() - 3..],
            ["--", "https://example.org/a/b.git", "/tmp/b"]
        );
    }

    /// SPEC-0094 v5 AK-11: a clone that cannot reach its host leaves no folder; cancelling ends
    /// git and leaves none either. Uses an unroutable address, no network.
    #[test]
    fn a_failed_or_cancelled_clone_leaves_nothing() {
        let dir = std::env::temp_dir().join(format!("idecraft-clone2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = check_clone_url("https://idecraft-clone-test.invalid/a/b.git").unwrap();
        let target = dir.join("b");
        let failed = clone_repository(
            &source,
            &target,
            &AtomicBool::new(false),
            Duration::from_secs(60),
            |_| {},
        );
        assert!(failed.is_err());
        assert!(!target.exists());
        let cancelled = clone_repository(
            &source,
            &target,
            &AtomicBool::new(true),
            Duration::from_secs(60),
            |_| {},
        );
        assert!(cancelled.is_err());
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
