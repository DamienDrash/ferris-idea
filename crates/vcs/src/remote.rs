//! Remotes, upstream tracking, fetch, pull and push (`SPEC-0055`) through the `git` executable.
//!
//! Network operations use the installed `git`, as the user's credential helpers, SSH configuration
//! and transports live there. Git never prompts: a missing credential fails fast (IMP-0029). Git
//! over SSH uses the shared `ssh` options with the required post-quantum key exchange on every hop
//! (ADR-0025, `ferris_idea_remote::SshCommand::git_ssh_command`); the one exception, Azure DevOps with the strongest classical
//! methods it offers, is reported as `pq: false` with a notice and logged (ADR-0025 section 1a,
//! SPEC-0055 v3).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::git::{CommitInfo, GitError, MergeOutcome, Repo};

/// Default time limit of a network operation.
pub const REMOTE_TIMEOUT: Duration = Duration::from_secs(120);

/// A configured remote (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Remote {
    /// Name, e.g. `origin`.
    pub name: String,
    /// URL used for fetching.
    pub fetch_url: String,
    /// URL used for pushing.
    pub push_url: String,
}

/// A local branch and its upstream (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tracking {
    /// Local branch name.
    pub branch: String,
    /// Whether HEAD points at it.
    pub current: bool,
    /// Upstream, e.g. `origin/main`.
    pub upstream: Option<String>,
    /// Commits on the branch that the upstream lacks.
    pub ahead: usize,
    /// Commits on the upstream that the branch lacks.
    pub behind: usize,
    /// Whether the configured upstream no longer exists.
    pub gone: bool,
}

/// One reference update reported by `git push --porcelain` or `git fetch --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefUpdate {
    /// `new`, `fast-forward`, `forced`, `deleted`, `up-to-date`, `rejected`, `pruned`.
    pub kind: String,
    /// Reference, e.g. `refs/heads/main` or `refs/remotes/origin/main`.
    pub reference: String,
    /// Git's summary, e.g. `1a2b3c4..5d6e7f8` or `[rejected] (non-fast-forward)`.
    pub summary: String,
}

/// Result of a fetch or push (behaviour 4–6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOutcome {
    /// Whether git succeeded.
    pub ok: bool,
    /// The git command that ran, without credentials.
    pub command: Vec<String>,
    /// Reference updates.
    pub updates: Vec<RefUpdate>,
    /// Whether it failed because credentials were needed and prompting is disabled.
    pub auth_required: bool,
    /// Why `ssh` refused before Git could talk to the remote: no post-quantum key exchange on the
    /// host or in the local client (ADR-0025).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_refusal: Option<String>,
    /// Whether the time limit ended it.
    pub timed_out: bool,
    /// Git's messages (stderr).
    pub messages: String,
    /// Whether the SSH connection used a post-quantum key exchange; set only for Azure DevOps, the
    /// one host allowed a classical one (ADR-0025 section 1a). Every other SSH connection is
    /// post-quantum or refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pq: Option<bool>,
    /// What to know when `pq` is `false`: the host, the method and what to use instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pq_notice: Option<String>,
}

/// Result of a pull: the fetch and the merge of the upstream (behaviour 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullOutcome {
    /// The fetch.
    pub fetch: RemoteOutcome,
    /// The merge of the upstream into the current branch, if the fetch succeeded.
    pub merge: Option<MergeOutcome>,
    /// Upstream that was merged.
    pub upstream: String,
    /// The rebase onto the upstream, when the update rebased (`SPEC-0144`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebase: Option<crate::OpOutcome>,
    /// The commits a rebase replays (or would replay), oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replayed: Vec<String>,
    /// What the update brought in, when HEAD moved (`SPEC-0144` v4 behaviour 8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<crate::UpdateInfo>,
    /// The branch's own commits a reset to the upstream gives up (or would), newest first
    /// (`SPEC-0144` v4 behaviour 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<String>,
    /// The reset to the upstream, when the update reset (`SPEC-0144` v4 behaviour 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<crate::OpOutcome>,
}

/// Options of the Push dialog (`SPEC-0144`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushOptions {
    /// Branch name on the remote; `None` or empty: the local branch's name.
    pub target: Option<String>,
    /// Record the upstream (for a new remote branch).
    pub set_upstream: bool,
    /// Also push the tags on the pushed branch's history, annotated and lightweight
    /// (`SPEC-0144` v2 behaviour 3; `--follow-tags` alone leaves lightweight tags behind).
    pub tags: bool,
    /// Overwrite the remote branch, but only if it is where the last fetch saw it.
    pub force_with_lease: bool,
    /// The local branch to push; `None`: the current one (`SPEC-0068` v6 behaviour 23).
    pub branch: Option<String>,
    /// Push only up to this commit of the branch (`<id>:refs/heads/<target>`, `SPEC-0086` v8
    /// behaviour 17); the upstream is then not recorded.
    pub up_to: Option<String>,
}

/// Whether `name` is usable as a branch name: Git's rules, simplified to refuse anything
/// that could be read as an option or a different reference.
pub fn valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '/', '.'])
        && !name.ends_with(['/', '.'])
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c))
}

/// What a push would send (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PushPreview {
    /// Branch to push.
    pub branch: String,
    /// Remote.
    pub remote: String,
    /// Commits that would be sent, newest first (all branch commits the remote lacks).
    pub commits: Vec<CommitInfo>,
    /// Git's own dry run (`--dry-run --porcelain`), which also shows rejections.
    pub dry_run: RemoteOutcome,
}

pub(crate) fn git_cmd(workdir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(workdir)
        // Never ask for anything: no terminal prompt, no askpass program, SSH in batch mode.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .args(["-c", "core.askPass=", "-c", "credential.interactive=never"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SSH remotes: the system `ssh` from the one start point, with the shared options — batch
    // mode, strict host keys, the post-quantum key exchange on every hop. It replaces the user's
    // `GIT_SSH_COMMAND`, `GIT_SSH` and `core.sshCommand` (no path without the post-quantum
    // exchange); `~/.ssh/config` still applies, and Ferris IDEA's own SSH hosts are reached with its
    // own file (SPEC-0055 v5, SPEC-0121 v6 behaviour 1b).
    cmd.env(
        "GIT_SSH_COMMAND",
        ferris_idea_remote::SshCommand::git_ssh_command(),
    )
    .env_remove("GIT_SSH")
    .env_remove("GIT_SSH_VARIANT")
    .args(["-c", "ssh.variant=ssh"]);
    cmd
}

/// Runs git with a time limit; returns (status ok, stdout, stderr, timed out).
pub(crate) fn run_git(
    workdir: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(bool, String, String, bool), GitError> {
    let mut cmd = git_cmd(workdir);
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| GitError::Git(format!("cannot start git: {e}")))?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let (ok, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.success(), false),
            Ok(None) if started.elapsed() >= timeout => {
                #[cfg(unix)]
                if let Ok(pid) = i32::try_from(child.id()) {
                    let _ = nix::sys::signal::killpg(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
                let _ = child.kill();
                let _ = child.wait();
                break (false, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(GitError::Git(e.to_string())),
        }
    };
    Ok((
        ok,
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
        timed_out,
    ))
}

/// Local git command without network; fails with git's message.
fn query(workdir: &Path, args: &[&str]) -> Result<String, GitError> {
    let (ok, out, err, _) = run_git(workdir, args, Duration::from_secs(30))?;
    if ok {
        Ok(out)
    } else {
        Err(GitError::Rejected(err.trim().to_owned()))
    }
}

pub(crate) fn auth_failure(messages: &str) -> bool {
    let m = messages.to_lowercase();
    m.contains("terminal prompts disabled")
        || m.contains("could not read username")
        || m.contains("could not read password")
        || m.contains("unable to get password")
        || m.contains("authentication failed")
        || m.contains("permission denied (publickey")
        || m.contains("host key verification failed")
}

/// Parses `--porcelain` lines of push (`flag\tfrom:to\tsummary`) and fetch (`flag old new ref`).
fn parse_updates(stdout: &str) -> Vec<RefUpdate> {
    let kind = |flag: char| match flag {
        '*' => "new",
        ' ' => "fast-forward",
        '+' => "forced",
        '-' => "deleted",
        '=' => "up-to-date",
        '!' => "rejected",
        't' => "tag",
        _ => "other",
    };
    stdout
        .lines()
        .filter_map(|line| {
            let flag = line.chars().next()?;
            if line.starts_with("To ") || line == "Done" {
                return None;
            }
            if let Some(rest) = line.get(1..).and_then(|r| r.strip_prefix('\t')) {
                // push: from:to \t summary
                let mut parts = rest.splitn(2, '\t');
                let refs = parts.next()?;
                let summary = parts.next().unwrap_or_default().to_owned();
                let reference = refs.rsplit(':').next().unwrap_or(refs).to_owned();
                return Some(RefUpdate {
                    kind: kind(flag).into(),
                    reference,
                    summary,
                });
            }
            // fetch: flag old new ref
            let mut parts = line.get(2..)?.split(' ');
            let old = parts.next()?;
            let new = parts.next()?;
            let reference = parts.next()?.to_owned();
            Some(RefUpdate {
                kind: if flag == '-' {
                    "pruned".into()
                } else {
                    kind(flag).into()
                },
                reference,
                summary: format!("{}..{}", &old[..old.len().min(7)], &new[..new.len().min(7)]),
            })
        })
        .collect()
}

/// `ssh`'s refusal of the key exchange in Git's messages, named for the peer `ssh` reported.
fn ssh_refusal(messages: &str) -> Option<String> {
    let peer =
        ferris_idea_remote::negotiating_peer(messages).unwrap_or_else(|| "the remote".into());
    ferris_idea_remote::kex_refusal(&peer, messages).map(|e| e.to_string())
}

/// File name of the log of Git operations without a post-quantum key exchange, in the Ferris IDEA log
/// folder (ADR-0025 section 1a).
pub const CLASSIC_KEX_LOG: &str = "git-ssh-kex.log";

/// `pq` and `pq_notice` from the stand-in's reports: `false` and a notice when any connection used
/// a classical method.
fn kex_result(reports: &[ferris_idea_remote::KexReport]) -> (Option<bool>, Option<String>) {
    let classic: Vec<_> = reports.iter().filter(|r| r.pq() == Some(false)).collect();
    if !classic.is_empty() {
        let mut hosts: Vec<String> = classic.iter().map(|r| r.host.clone()).collect();
        let mut kex: Vec<String> = classic.iter().map(|r| r.kex.clone()).collect();
        hosts.dedup();
        kex.dedup();
        return (
            Some(false),
            Some(ferris_idea_remote::classic_kex_notice(&hosts, &kex)),
        );
    }
    (reports.iter().find_map(|r| r.pq()), None)
}

/// The log line of a Git operation without a post-quantum key exchange: time, Git's subcommand,
/// host and method; no URL, reference or other argument.
fn classic_kex_log_line(
    args: &[&str],
    reports: &[ferris_idea_remote::KexReport],
) -> Option<String> {
    let classic: Vec<_> = reports.iter().filter(|r| r.pq() == Some(false)).collect();
    if classic.is_empty() {
        return None;
    }
    let sub = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .copied()
        .unwrap_or("?");
    let hosts: Vec<String> = classic
        .iter()
        .map(|r| format!("host={} kex={}", r.host, r.kex))
        .collect();
    Some(format!(
        "{} git {sub} {} pq=false (ADR-0025 section 1a exception, {})",
        ferris_idea_core::logs::utc_stamp(std::time::SystemTime::now()),
        hosts.join(" "),
        ferris_idea_remote::AZURE_CLASSIC_SETTING
    ))
}

fn outcome(args: &[&str], result: (bool, String, String, bool)) -> RemoteOutcome {
    let (ok, stdout, stderr, timed_out) = result;
    let (reports, stderr) = ferris_idea_remote::take_kex_reports(&stderr);
    if let (Some(line), Some(dir)) = (
        classic_kex_log_line(args, &reports),
        ferris_idea_core::logs::log_dir(),
    ) {
        let _ = ferris_idea_core::logs::append_line(&dir.join(CLASSIC_KEX_LOG), &line);
    }
    let (pq, pq_notice) = kex_result(&reports);
    let refusal = if ok { None } else { ssh_refusal(&stderr) };
    RemoteOutcome {
        ok,
        command: std::iter::once("git")
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect(),
        updates: parse_updates(&stdout),
        auth_required: !ok && refusal.is_none() && auth_failure(&stderr),
        ssh_refusal: refusal,
        timed_out,
        messages: stderr.trim().to_owned(),
        pq,
        pq_notice,
    }
}

/// Lightweight tags (`refs/tags/…` naming a commit directly) on the history of `branch`; the
/// annotated ones are left to `--follow-tags`.
fn lightweight_tags(dir: &Path, tip: &str) -> Result<Vec<String>, GitError> {
    let merged = format!("--merged={tip}");
    let text = query(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objecttype)",
            &merged,
            "refs/tags",
        ],
    )?;
    Ok(text
        .lines()
        .filter_map(|l| l.split_once('\0'))
        .filter(|(name, kind)| *kind == "commit" && !name.contains(['\n', ':']))
        .map(|(name, _)| name.to_owned())
        .collect())
}

fn workdir(repo: &Repo) -> Result<&Path, GitError> {
    repo.workdir()
        .ok_or_else(|| GitError::Rejected("the repository has no working tree".into()))
}

impl Repo {
    /// Configured remotes (behaviour 1).
    pub fn remotes(&self) -> Result<Vec<Remote>, GitError> {
        let dir = workdir(self)?;
        let names = query(dir, &["remote"])?;
        let mut out = Vec::new();
        for name in names.lines().filter(|l| !l.is_empty()) {
            let fetch_url = query(dir, &["remote", "get-url", name])?.trim().to_owned();
            let push_url = query(dir, &["remote", "get-url", "--push", name])?
                .trim()
                .to_owned();
            out.push(Remote {
                name: name.to_owned(),
                fetch_url,
                push_url,
            });
        }
        Ok(out)
    }

    /// Local branches with upstream and ahead/behind counts from the last fetch (behaviour 2).
    pub fn tracking(&self) -> Result<Vec<Tracking>, GitError> {
        let dir = workdir(self)?;
        let text = query(
            dir,
            &[
                "for-each-ref",
                "--format=%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(upstream:track,nobracket)",
                "refs/heads",
            ],
        )?;
        let mut out = Vec::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\0').collect();
            if f.len() < 4 {
                continue;
            }
            let track = f[3];
            let count = |word: &str| {
                track
                    .split(", ")
                    .find_map(|p| p.strip_prefix(word))
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0)
            };
            out.push(Tracking {
                branch: f[0].to_owned(),
                current: f[1] == "*",
                upstream: (!f[2].is_empty()).then(|| f[2].to_owned()),
                ahead: count("ahead "),
                behind: count("behind "),
                gone: track == "gone",
            });
        }
        Ok(out)
    }

    /// Fetches `remote` (default: all remotes), pruning deleted remote branches (behaviour 4).
    pub fn fetch(
        &self,
        remote: Option<&str>,
        timeout: Duration,
    ) -> Result<RemoteOutcome, GitError> {
        let dir = workdir(self)?;
        let mut args = vec!["fetch", "--prune", "--porcelain"];
        match remote {
            Some(r) => args.push(r),
            None => args.push("--all"),
        }
        let result = run_git(dir, &args, timeout)?;
        Ok(outcome(&args, result))
    }

    /// Fetches the current branch's remote and merges its upstream (behaviour 5). With `dry_run`
    /// only the merge is a preview; the fetch updates remote-tracking branches, as it changes no
    /// local branch or file.
    pub fn pull(&self, dry_run: bool, timeout: Duration) -> Result<PullOutcome, GitError> {
        let current = self
            .tracking()?
            .into_iter()
            .find(|t| t.current)
            .ok_or_else(|| GitError::Rejected("HEAD is not on a branch".into()))?;
        let upstream = current.upstream.ok_or_else(|| {
            GitError::Rejected(format!("branch {} has no upstream", current.branch))
        })?;
        let remote = upstream.split('/').next().unwrap_or_default().to_owned();
        let fetch = self.fetch(Some(&remote), timeout)?;
        if !fetch.ok {
            return Ok(PullOutcome {
                fetch,
                merge: None,
                upstream,
                rebase: None,
                replayed: Vec::new(),
                info: None,
                dropped: Vec::new(),
                reset: None,
            });
        }
        let before = self.head_id();
        // Preview first: a merge that would conflict is reported and not attempted.
        let preview = self.merge(&upstream, true)?;
        let blocked =
            preview.kind == crate::git::MergeKind::Conflict || !preview.local_conflicts.is_empty();
        let merge = if dry_run || blocked {
            preview
        } else {
            self.merge(&upstream, false)?
        };
        let info = self.moved_since(before)?;
        Ok(PullOutcome {
            fetch,
            merge: Some(merge),
            upstream,
            rebase: None,
            replayed: Vec::new(),
            info,
            dropped: Vec::new(),
            reset: None,
        })
    }

    /// Fetches like [`Repo::pull`], then replays the current branch's own commits onto its
    /// upstream, stashing local changes around it (`SPEC-0144`). With `dry_run` only the commits
    /// to replay are listed. A rebase that stops on conflicts waits for Continue or Abort.
    pub fn pull_rebase(&self, dry_run: bool, timeout: Duration) -> Result<PullOutcome, GitError> {
        let current = self
            .tracking()?
            .into_iter()
            .find(|t| t.current)
            .ok_or_else(|| GitError::Rejected("HEAD is not on a branch".into()))?;
        let upstream = current.upstream.ok_or_else(|| {
            GitError::Rejected(format!("branch {} has no upstream", current.branch))
        })?;
        let remote = upstream.split('/').next().unwrap_or_default().to_owned();
        let fetch = self.fetch(Some(&remote), timeout)?;
        let mut out = PullOutcome {
            fetch,
            merge: None,
            upstream,
            rebase: None,
            replayed: Vec::new(),
            info: None,
            dropped: Vec::new(),
            reset: None,
        };
        if !out.fetch.ok {
            return Ok(out);
        }
        out.replayed = self.rebase_preview(&out.upstream)?;
        if !dry_run {
            let before = self.head_id();
            out.rebase = Some(self.rebase_autostash(&out.upstream)?);
            if out.rebase.as_ref().is_some_and(|r| r.ok) {
                out.info = self.moved_since(before)?;
            }
        }
        Ok(out)
    }

    /// Commits of `branch` that `remote` does not have yet (as of the last fetch), newest first.
    pub fn unpushed_commits(
        &self,
        branch: &str,
        remote: &str,
    ) -> Result<Vec<CommitInfo>, GitError> {
        let dir = workdir(self)?;
        let exclude = format!("--remotes={remote}");
        let text = query(
            dir,
            &[
                "log",
                "--format=%H%x00%s%x00%an%x00%ae%x00%at",
                branch,
                "--not",
                &exclude,
                "--",
            ],
        )?;
        Ok(text
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\0').collect();
                (f.len() == 5).then(|| CommitInfo {
                    short_id: f[0][..7].to_owned(),
                    id: f[0].to_owned(),
                    summary: f[1].to_owned(),
                    author: f[2].to_owned(),
                    email: f[3].to_owned(),
                    time: f[4].parse().unwrap_or(0),
                    parents: Vec::new(),
                    committer: None,
                })
            })
            .collect())
    }

    /// Pushes `branch` (default: current) to `remote` (default: its upstream's remote, else
    /// `origin`). `set_upstream` records the upstream; `force_with_lease` overwrites the remote
    /// branch only if it still points at the given commit — a plain force push does not exist
    /// (behaviour 6, IMP-0029).
    pub fn push(
        &self,
        branch: Option<&str>,
        remote: Option<&str>,
        set_upstream: bool,
        force_with_lease: Option<&str>,
        timeout: Duration,
    ) -> Result<RemoteOutcome, GitError> {
        let (branch, remote) = self.push_target(branch, remote)?;
        let dir = workdir(self)?;
        let lease;
        let mut args = vec!["push", "--porcelain"];
        if set_upstream {
            args.push("--set-upstream");
        }
        if let Some(expected) = force_with_lease {
            lease = format!("--force-with-lease=refs/heads/{branch}:{expected}");
            args.push(&lease);
        }
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        args.push(&remote);
        args.push(&refspec);
        let result = run_git(dir, &args, timeout)?;
        Ok(outcome(&args, result))
    }

    /// Pushes the current branch with the Push dialog's options (`SPEC-0144`): to `target` on the
    /// remote (default: the same name), with `--follow-tags`, and with a force push that is
    /// refused when the remote branch moved since the last fetch (`--force-with-lease` against
    /// the remote-tracking branch; IMP-0029).
    pub fn push_with(
        &self,
        options: &PushOptions,
        timeout: Duration,
    ) -> Result<RemoteOutcome, GitError> {
        let (branch, remote) = self.push_target(options.branch.as_deref(), None)?;
        // The source: the branch, or one of its unpushed commits (SPEC-0086 v8 behaviour 17).
        let source = match options.up_to.as_deref() {
            Some(rev) => self.outgoing_commit(&branch, &remote, rev)?,
            None => format!("refs/heads/{branch}"),
        };
        let target = options
            .target
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(&branch)
            .to_owned();
        if !valid_branch_name(&target) {
            return Err(GitError::Rejected(format!(
                "{target} is not a valid branch name"
            )));
        }
        let dir = workdir(self)?;
        let mut args: Vec<String> = vec!["push".into(), "--porcelain".into()];
        if options.set_upstream && options.up_to.is_none() {
            args.push("--set-upstream".into());
        }
        if options.tags {
            args.push("--follow-tags".into());
        }
        if options.force_with_lease {
            args.push(format!("--force-with-lease=refs/heads/{target}"));
        }
        let lightweight = if options.tags {
            lightweight_tags(dir, &source)?
        } else {
            Vec::new()
        };
        args.push(remote);
        args.push(format!("{source}:refs/heads/{target}"));
        // Hand test 70 (D35): lightweight tags go as their own refspecs; one the remote already
        // has unchanged is reported as up to date.
        args.extend(lightweight.into_iter().map(|t| format!("{t}:{t}")));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = run_git(dir, &refs, timeout)?;
        Ok(outcome(&refs, result))
    }

    /// What [`Repo::push`] would send, including git's own dry run (behaviour 6).
    pub fn push_preview(
        &self,
        branch: Option<&str>,
        remote: Option<&str>,
        timeout: Duration,
    ) -> Result<PushPreview, GitError> {
        let (branch, remote) = self.push_target(branch, remote)?;
        let dir = workdir(self)?;
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        let args = [
            "push",
            "--dry-run",
            "--porcelain",
            remote.as_str(),
            refspec.as_str(),
        ];
        let result = run_git(dir, &args, timeout)?;
        Ok(PushPreview {
            commits: self.unpushed_commits(&branch, &remote)?,
            dry_run: outcome(&args, result),
            branch,
            remote,
        })
    }

    fn push_target(
        &self,
        branch: Option<&str>,
        remote: Option<&str>,
    ) -> Result<(String, String), GitError> {
        let tracking = self.tracking()?;
        let entry = match branch {
            Some(b) => tracking.iter().find(|t| t.branch == b),
            None => tracking.iter().find(|t| t.current),
        }
        .ok_or_else(|| {
            GitError::Rejected(match branch {
                Some(b) => format!("no local branch {b}"),
                None => "HEAD is not on a branch".into(),
            })
        })?;
        let remote = match remote {
            Some(r) => r.to_owned(),
            None => entry
                .upstream
                .as_deref()
                .and_then(|u| u.split('/').next())
                .unwrap_or("origin")
                .to_owned(),
        };
        if !self.remotes()?.iter().any(|r| r.name == remote) {
            return Err(GitError::Rejected(format!("no remote {remote}")));
        }
        Ok((entry.branch.clone(), remote))
    }

    /// What Push would send from the current branch, without network access (`SPEC-0109`
    /// behaviour 3): the branch, the remote (its upstream's, else `origin`), whether an upstream is
    /// set, and the commits the remote lacks as of the last fetch, newest first.
    pub fn outgoing(&self) -> Result<Outgoing, GitError> {
        self.outgoing_of(None, None)
    }

    /// As [`Repo::outgoing`] for `branch` (default: the current one), and with `up_to` only that
    /// unpushed commit and the older ones (`SPEC-0068` v6 behaviour 23, `SPEC-0086` v8
    /// behaviour 17).
    pub fn outgoing_of(
        &self,
        branch: Option<&str>,
        up_to: Option<&str>,
    ) -> Result<Outgoing, GitError> {
        let (branch, remote) = self.push_target(branch, None)?;
        let has_upstream = self
            .tracking()?
            .into_iter()
            .any(|t| t.branch == branch && t.upstream.is_some());
        let mut commits = self.unpushed_commits(&branch, &remote)?;
        let up_to = match up_to {
            Some(rev) => {
                let id = self.outgoing_commit(&branch, &remote, rev)?;
                let at = commits.iter().position(|c| c.id == id).unwrap_or(0);
                commits.drain(..at);
                Some(id)
            }
            None => None,
        };
        let mut files = self.outgoing_files(&branch, &remote)?;
        files.retain(|id, _| commits.iter().any(|c| &c.id == id));
        let remote_branches = self.remote_branch_names(&remote)?;
        Ok(Outgoing {
            branch,
            remote,
            has_upstream,
            commits,
            files,
            remote_branches,
            up_to,
        })
    }

    /// The full id of `rev` when it is one of `branch`'s commits that `remote` lacks.
    fn outgoing_commit(&self, branch: &str, remote: &str, rev: &str) -> Result<String, GitError> {
        let dir = workdir(self)?;
        if rev.starts_with('-') || rev.trim().is_empty() {
            return Err(GitError::Rejected(format!("{rev} is not a revision")));
        }
        let spec = format!("{rev}^{{commit}}");
        let id = query(dir, &["rev-parse", "--verify", "--quiet", &spec])
            .map_err(|_| GitError::Rejected(format!("unknown revision {rev}")))?
            .trim()
            .to_owned();
        if !self
            .unpushed_commits(branch, remote)?
            .iter()
            .any(|c| c.id == id)
        {
            return Err(GitError::Rejected(format!(
                "{} is not an unpushed commit of {branch}",
                &id[..id.len().min(7)]
            )));
        }
        Ok(id)
    }

    /// What an update changed since HEAD was `before`, `None` when HEAD did not move.
    fn moved_since(&self, before: Option<String>) -> Result<Option<crate::UpdateInfo>, GitError> {
        match (before, self.head_id()) {
            (Some(from), Some(to)) if from != to => Ok(Some(self.update_info(&from, &to)?)),
            _ => Ok(None),
        }
    }
}

/// The commits Push would send (`SPEC-0109`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outgoing {
    /// Current branch.
    pub branch: String,
    /// Remote it goes to.
    pub remote: String,
    /// Whether the branch has an upstream; without one, Push sets it.
    pub has_upstream: bool,
    /// Commits the remote lacks, newest first.
    pub commits: Vec<CommitInfo>,
    /// The files each of those commits changes, by full commit id (`SPEC-0109` v2 behaviour 7).
    pub files: std::collections::BTreeMap<String, Vec<crate::ChangedPath>>,
    /// Branches the remote has as of the last fetch, without the remote's name; a target among
    /// them is not new (`SPEC-0109` v2 behaviour 9).
    pub remote_branches: Vec<String>,
    /// The commit Push All up to Here stops at (`SPEC-0086` v8 behaviour 17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_to: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_lines() {
        let push = "To /tmp/remote.git\n*\trefs/heads/feat:refs/heads/feat\t[new branch]\n \trefs/heads/main:refs/heads/main\t1a2b3c4..5d6e7f8\n!\trefs/heads/old:refs/heads/old\t[rejected] (non-fast-forward)\nDone\n";
        let u = parse_updates(push);
        let summary: Vec<(&str, &str)> = u
            .iter()
            .map(|r| (r.kind.as_str(), r.reference.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("new", "refs/heads/feat"),
                ("fast-forward", "refs/heads/main"),
                ("rejected", "refs/heads/old")
            ]
        );
        assert_eq!(u[2].summary, "[rejected] (non-fast-forward)");
        let fetch = "  1111111111111111111111111111111111111111 2222222222222222222222222222222222222222 refs/remotes/origin/main\n* 0000000000000000000000000000000000000000 3333333333333333333333333333333333333333 refs/remotes/origin/new\n- 4444444444444444444444444444444444444444 0000000000000000000000000000000000000000 refs/remotes/origin/gone\n";
        let u = parse_updates(fetch);
        let summary: Vec<(&str, &str)> = u
            .iter()
            .map(|r| (r.kind.as_str(), r.reference.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("fast-forward", "refs/remotes/origin/main"),
                ("new", "refs/remotes/origin/new"),
                ("pruned", "refs/remotes/origin/gone")
            ]
        );
        assert_eq!(u[0].summary, "1111111..2222222");
    }

    #[test]
    fn auth_messages() {
        assert!(auth_failure(
            "fatal: could not read Username for 'https://x': terminal prompts disabled"
        ));
        assert!(auth_failure("git@host: Permission denied (publickey)."));
        assert!(auth_failure("fatal: unable to get password from user"));
        assert!(!auth_failure("fatal: repository 'x' not found"));
        let refused = outcome(
            &["fetch"],
            (
                false,
                String::new(),
                "Unable to negotiate with 40.74.28.9 port 22: no matching key exchange method found. \
                 Their offer: diffie-hellman-group14-sha256\nfatal: Could not read from remote \
                 repository.\n"
                    .into(),
                false,
            ),
        );
        let why = refused.ssh_refusal.unwrap();
        assert!(
            why.starts_with("40.74.28.9: the host offers no post-quantum key exchange"),
            "{why}"
        );
        assert!(!refused.auth_required);
    }

    #[test]
    fn classical_exchanges_are_reported_and_logged_without_details() {
        let stderr = "ferris-idea-git-ssh: host=ssh.dev.azure.com kex=diffie-hellman-group18-sha512\n\
                      Everything up-to-date\n";
        let (reports, rest) = ferris_idea_remote::take_kex_reports(stderr);
        assert_eq!(rest.trim(), "Everything up-to-date");
        let (pq, notice) = kex_result(&reports);
        assert_eq!(pq, Some(false));
        let notice = notice.unwrap();
        assert!(notice.contains("ssh.dev.azure.com"), "{notice}");
        assert!(notice.contains("diffie-hellman-group18-sha512"), "{notice}");
        let line = classic_kex_log_line(
            &["push", "--porcelain", "https://user:secret@x/y", "main"],
            &reports,
        )
        .unwrap();
        assert!(line.contains("git push host=ssh.dev.azure.com"), "{line}");
        assert!(line.contains("pq=false"), "{line}");
        assert!(!line.contains("secret") && !line.contains("main"), "{line}");
        // A post-quantum connection to Azure (once it offers one) needs no notice.
        let pq = ferris_idea_remote::take_kex_reports(
            "ferris-idea-git-ssh: host=ssh.dev.azure.com kex=mlkem768x25519-sha256",
        )
        .0;
        assert_eq!(kex_result(&pq), (Some(true), None));
        assert!(classic_kex_log_line(&["fetch"], &pq).is_none());
        // Every other host: no marker, no field.
        assert_eq!(kex_result(&[]), (None, None));
        let o = outcome(
            &["fetch"],
            (true, String::new(), "From github.com:a/b\n".into(), false),
        );
        assert_eq!((o.pq, o.pq_notice), (None, None));
    }

    #[test]
    fn git_over_ssh_gets_the_shared_options() {
        let cmd = git_cmd(Path::new("."));
        let env: Vec<_> = cmd.get_envs().collect();
        let ssh = env
            .iter()
            .find(|(k, _)| *k == "GIT_SSH_COMMAND")
            .and_then(|(_, v)| *v)
            .and_then(|v| v.to_str())
            .unwrap();
        assert_eq!(ssh, ferris_idea_remote::SshCommand::git_ssh_command());
        // The stand-in, which runs the strict options for every host but Azure DevOps and routes
        // jump hosts through Ferris IDEA's hop stand-in, or the strict command with the route pinned.
        let text = if ssh.starts_with("ssh ") {
            ssh.to_owned()
        } else {
            std::fs::read_to_string(ssh).unwrap()
        };
        let strict = ferris_idea_remote::testing::shared_options("ssh").join(" ");
        assert!(text.contains(&strict), "{text}");
        assert!(text.contains("-o KexAlgorithms="), "{text}");
        assert!(text.contains("ProxyCommand"), "{text}");
        // The user's own SSH command cannot bring back a classical exchange.
        assert!(env.iter().any(|(k, v)| *k == "GIT_SSH" && v.is_none()));
        let args: Vec<_> = cmd.get_args().filter_map(|a| a.to_str()).collect();
        assert!(args.windows(2).any(|w| w == ["-c", "ssh.variant=ssh"]));
    }
}
