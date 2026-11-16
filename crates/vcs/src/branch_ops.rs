//! Branch popup data and branch operations (`SPEC-0068` v5), stash options (`SPEC-0106` v3),
//! update information and reset to the upstream (`SPEC-0144` v4), files per outgoing commit
//! (`SPEC-0109` v2), run through the `git` executable like the other operations.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::git::{CommitInfo, GitError, Repo};
use crate::ops::{LOCAL_TIMEOUT, OpOutcome};
use crate::remote::{PullOutcome, run_git, valid_branch_name};

/// A tag and the commit it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tag {
    /// Short name, e.g. `v1.0`.
    pub name: String,
    /// Short id of the commit (annotated tags peeled).
    pub target: String,
}

/// A path a commit or an update changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedPath {
    /// Git's status letter: `A`, `M`, `D`, `R`, `C`, `T`.
    pub status: String,
    /// Path relative to the repository root (the new path of a rename).
    pub path: String,
}

/// What an update brought in (`SPEC-0144` v4 behaviour 8): the commits between the old and the
/// new HEAD and the files that differ.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// HEAD before the update.
    pub from: String,
    /// HEAD after it.
    pub to: String,
    /// Commits now reachable that were not before, newest first.
    pub commits: Vec<CommitInfo>,
    /// Files that differ between the two.
    pub files: Vec<ChangedPath>,
}

/// Options of a new stash (`SPEC-0106` v3 behaviour 14).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StashOptions {
    /// The stash's message; empty: Git's own.
    pub message: Option<String>,
    /// Stash untracked files too.
    pub include_untracked: bool,
    /// Leave the staged changes in the index (and the working tree).
    pub keep_index: bool,
}

/// How many entries "Recent" lists at most (`SPEC-0068` v5 behaviour 17).
pub const RECENT_BRANCHES: usize = 5;

/// Text of a local git query, Git's message as the error.
fn query(repo: &Repo, args: &[&str]) -> Result<String, GitError> {
    let (ok, out, err, timed_out) = run_git(repo.dir()?, args, LOCAL_TIMEOUT)?;
    if timed_out {
        return Err(GitError::Git(format!("git {} timed out", args.join(" "))));
    }
    if ok {
        Ok(out)
    } else {
        Err(GitError::Rejected(err.trim().to_owned()))
    }
}

/// A revision as given by the user, refused when git could read it as an option.
fn revision(rev: &str) -> Result<&str, GitError> {
    let rev = rev.trim();
    if rev.is_empty() || rev.starts_with('-') || rev.contains(char::is_whitespace) {
        Err(GitError::Rejected(format!("{rev:?} is not a revision")))
    } else {
        Ok(rev)
    }
}

/// Parses `git log --name-status --format=%x00%H…` into commit id → changed paths.
fn parse_name_status(text: &str) -> BTreeMap<String, Vec<ChangedPath>> {
    let mut out: BTreeMap<String, Vec<ChangedPath>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(id) = line.strip_prefix('\0') {
            let id = id.trim().to_owned();
            out.entry(id.clone()).or_default();
            current = Some(id);
        } else if let Some(c) = current.as_ref()
            && let Some(p) = changed_path(line)
        {
            out.entry(c.clone()).or_default().push(p);
        }
    }
    out
}

/// One `--name-status` line: `M\tpath`, `R100\told\tnew`.
fn changed_path(line: &str) -> Option<ChangedPath> {
    let mut parts = line.split('\t');
    let status = parts.next()?.trim();
    let first = parts.next()?;
    let path = parts.next().unwrap_or(first);
    let letter = status.chars().next()?;
    letter.is_ascii_uppercase().then(|| ChangedPath {
        status: letter.to_string(),
        path: path.to_owned(),
    })
}

/// Commits from `git log --format=%H%x00%s%x00%an%x00%ae%x00%at`.
fn parse_commits(text: &str) -> Vec<CommitInfo> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\0').collect();
            (f.len() == 5 && f[0].len() >= 7).then(|| CommitInfo {
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
        .collect()
}

const COMMIT_FORMAT: &str = "--format=%H%x00%s%x00%an%x00%ae%x00%at";

impl Repo {
    /// Tags, by name, with the commit each names (`SPEC-0068` v5 behaviour 19).
    pub fn tags(&self) -> Result<Vec<Tag>, GitError> {
        let text = query(
            self,
            &[
                "for-each-ref",
                "--sort=refname",
                "--format=%(refname:short)%00%(objectname)%00%(*objectname)",
                "refs/tags",
            ],
        )?;
        Ok(text
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\0').collect();
                let id = f.get(2).filter(|p| !p.is_empty()).or(f.get(1))?;
                Some(Tag {
                    name: f.first()?.to_string(),
                    target: id.get(..7).unwrap_or(id).to_owned(),
                })
            })
            .collect())
    }

    /// Local branches checked out recently, newest first, without the current branch and without
    /// branches that are gone; at most `limit` (`SPEC-0068` v5 behaviour 17). Read from HEAD's
    /// reflog, where Git and IDECraft record every checkout.
    pub fn recent_branches(&self, limit: usize) -> Result<Vec<String>, GitError> {
        let (ok, text, _, _) = run_git(
            self.dir()?,
            &["reflog", "show", "--format=%gs", "-n", "500", "HEAD", "--"],
            LOCAL_TIMEOUT,
        )?;
        if !ok {
            // A repository without commits has no reflog yet.
            return Ok(Vec::new());
        }
        let local: Vec<String> = self
            .branches()?
            .branches
            .into_iter()
            .filter(|b| !b.remote)
            .map(|b| b.name)
            .collect();
        let current = self.current_branch_name();
        let mut out: Vec<String> = Vec::new();
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("checkout: moving from ") else {
                continue;
            };
            let Some((from, to)) = rest.split_once(" to ") else {
                continue;
            };
            for name in [to.trim(), from.trim()] {
                if Some(name) != current.as_deref()
                    && local.iter().any(|l| l == name)
                    && !out.iter().any(|o| o == name)
                {
                    out.push(name.to_owned());
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    /// Deletes local branch `name` if it is merged into HEAD (`git branch -d`); never the current
    /// branch, never forced (`SPEC-0068` v5 behaviour 20).
    pub fn delete_branch(&self, name: &str) -> Result<OpOutcome, GitError> {
        if !valid_branch_name(name) {
            return Err(GitError::Rejected(format!("{name} is not a branch name")));
        }
        if self.current_branch_name().as_deref() == Some(name) {
            return Err(GitError::Rejected(format!(
                "{name} is checked out; switch to another branch first"
            )));
        }
        let done = self.git_run(&["branch", "-d", name])?;
        if done.ok {
            Ok(done)
        } else {
            Err(GitError::Rejected(
                done.output
                    .lines()
                    .find(|l| l.starts_with("error:"))
                    .unwrap_or(done.output.as_str())
                    .trim_start_matches("error:")
                    .trim()
                    .to_owned(),
            ))
        }
    }

    /// Renames local branch `old` to `new`; an existing `new` is refused (`SPEC-0068` v5
    /// behaviour 20).
    pub fn rename_branch(&self, old: &str, new: &str) -> Result<OpOutcome, GitError> {
        for name in [old, new] {
            if !valid_branch_name(name) {
                return Err(GitError::Rejected(format!("{name} is not a branch name")));
            }
        }
        let done = self.git_run(&["branch", "-m", old, new])?;
        if done.ok {
            Ok(done)
        } else {
            Err(GitError::Rejected(done.output.trim().to_owned()))
        }
    }

    /// Checks out local branch `branch` and replays its own commits onto `onto` (`git rebase
    /// <onto> <branch>`; `SPEC-0068` v5 behaviour 20). Stops on conflicts like any rebase.
    pub fn checkout_and_rebase(&self, branch: &str, onto: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        if !valid_branch_name(branch) {
            return Err(GitError::Rejected(format!("{branch} is not a branch name")));
        }
        let onto = revision(onto)?;
        self.git_run(&["rebase", onto, branch])
    }

    /// Whether every commit of `branch` is in HEAD's history (`SPEC-0068` v5 behaviour 22).
    pub fn is_merged(&self, branch: &str) -> Result<bool, GitError> {
        let rev = revision(branch)?;
        let (ok, _, _, _) = run_git(
            self.dir()?,
            &["merge-base", "--is-ancestor", rev, "HEAD"],
            LOCAL_TIMEOUT,
        )?;
        Ok(ok)
    }

    /// Stashes the local changes with a message, untracked files and the index kept as asked
    /// (`SPEC-0106` v3 behaviour 14).
    pub fn stash_push_with(&self, options: &StashOptions) -> Result<OpOutcome, GitError> {
        let mut args = vec!["stash", "push"];
        if options.include_untracked {
            args.push("--include-untracked");
        }
        if options.keep_index {
            args.push("--keep-index");
        }
        if let Some(m) = options.message.as_deref().filter(|m| !m.trim().is_empty()) {
            args.extend(["--message", m]);
        }
        self.git_run(&args)
    }

    /// Puts untracked `paths` under version control (`git add`; `SPEC-0109` v2 behaviour 6, "Add
    /// to VCS").
    pub fn add_paths(&self, paths: &[String]) -> Result<OpOutcome, GitError> {
        if paths.is_empty() || paths.iter().any(|p| p.starts_with('-')) {
            return Err(GitError::Rejected("no path to add".into()));
        }
        let mut args = vec!["add", "--"];
        args.extend(paths.iter().map(String::as_str));
        let done = self.git_run(&args)?;
        if done.ok {
            Ok(done)
        } else {
            Err(GitError::Rejected(done.output.trim().to_owned()))
        }
    }

    /// Commits reachable from `tip` but not from `base`, newest first (`git log base..tip`).
    pub fn commits_between(&self, base: &str, tip: &str) -> Result<Vec<CommitInfo>, GitError> {
        let range = format!("{}..{}", revision(base)?, revision(tip)?);
        Ok(parse_commits(&query(
            self,
            &["log", COMMIT_FORMAT, &range, "--"],
        )?))
    }

    /// The commits `other` has and HEAD lacks, and those HEAD has and `other` lacks, newest first
    /// (`SPEC-0068` v6 behaviour 23: Compare with).
    pub fn compare_with(
        &self,
        other: &str,
    ) -> Result<(Vec<CommitInfo>, Vec<CommitInfo>), GitError> {
        Ok((
            self.commits_between("HEAD", other)?,
            self.commits_between(other, "HEAD")?,
        ))
    }

    /// The files at revision `rev`, sorted (`SPEC-0086` v8 behaviour 17: Show Repository at
    /// Revision).
    pub fn files_at(&self, rev: &str) -> Result<Vec<String>, GitError> {
        let spec = format!("{}^{{tree}}", revision(rev)?);
        let text = query(self, &["ls-tree", "-r", "-z", "--name-only", &spec])?;
        let mut files: Vec<String> = text
            .split('\0')
            .filter(|f| !f.is_empty())
            .map(str::to_owned)
            .collect();
        files.sort();
        Ok(files)
    }

    /// The tracked files that differ between revision `rev` and the working tree (`SPEC-0086` v8
    /// behaviour 17: Compare with Local; `SPEC-0068` v6: Show Diff with Working Tree).
    pub fn changes_against_worktree(&self, rev: &str) -> Result<Vec<ChangedPath>, GitError> {
        let rev = revision(rev)?;
        Ok(query(self, &["diff", "--name-status", "-M", rev, "--"])?
            .lines()
            .filter_map(changed_path)
            .collect())
    }

    /// Full id of HEAD, `None` without commits.
    pub fn head_id(&self) -> Option<String> {
        query(self, &["rev-parse", "--verify", "--quiet", "HEAD"])
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    /// What changed between `from` and `to` (`SPEC-0144` v4 behaviour 8).
    pub fn update_info(&self, from: &str, to: &str) -> Result<UpdateInfo, GitError> {
        let (from, to) = (revision(from)?, revision(to)?);
        let range = format!("{from}..{to}");
        let commits = parse_commits(&query(self, &["log", COMMIT_FORMAT, &range, "--"])?);
        let files = query(self, &["diff", "--name-status", "-M", from, to, "--"])?
            .lines()
            .filter_map(changed_path)
            .collect();
        Ok(UpdateInfo {
            from: from.to_owned(),
            to: to.to_owned(),
            commits,
            files,
        })
    }

    /// The files each commit of `branch` that `remote` lacks changes, by commit id (`SPEC-0109`
    /// v2 behaviour 7).
    pub fn outgoing_files(
        &self,
        branch: &str,
        remote: &str,
    ) -> Result<BTreeMap<String, Vec<ChangedPath>>, GitError> {
        let exclude = format!("--remotes={remote}");
        let tip = format!("refs/heads/{branch}");
        let text = query(
            self,
            &[
                "log",
                "--name-status",
                "-M",
                "--format=%x00%H",
                &tip,
                "--not",
                &exclude,
                "--",
            ],
        )?;
        Ok(parse_name_status(&text))
    }

    /// Branch names `remote` has as of the last fetch, without the remote's name.
    pub fn remote_branch_names(&self, remote: &str) -> Result<Vec<String>, GitError> {
        let prefix = format!("refs/remotes/{remote}/");
        let text = query(self, &["for-each-ref", "--format=%(refname)", &prefix])?;
        Ok(text
            .lines()
            .filter_map(|l| l.strip_prefix(&prefix))
            .filter(|n| *n != "HEAD")
            .map(str::to_owned)
            .collect())
    }

    /// Fetches like [`Repo::pull`], then moves the current branch to its upstream, discarding the
    /// branch's own commits; local changes are stashed and brought back around it (`SPEC-0144` v4
    /// behaviour 9, IMP-0322). The commits given up are named in `dropped`. With `dry_run` only
    /// those are listed (as of the fetch).
    pub fn reset_to_upstream(
        &self,
        dry_run: bool,
        timeout: Duration,
    ) -> Result<PullOutcome, GitError> {
        self.refuse_while_in_progress()?;
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
        out.dropped = self
            .commits_between(&out.upstream, "HEAD")?
            .into_iter()
            .map(|c| format!("{} {}", c.short_id, c.summary))
            .collect();
        if dry_run {
            return Ok(out);
        }
        let before = self.head_id();
        let dirty = !self.status()?.is_empty();
        if dirty {
            let stashed = self.stash_push_with(&StashOptions {
                message: Some(format!("IDECraft reset to {}", out.upstream)),
                include_untracked: true,
                keep_index: false,
            })?;
            if !stashed.ok {
                return Err(GitError::Git(stashed.output));
            }
        }
        let mut done = self.git_run(&["reset", "--hard", &out.upstream])?;
        if dirty {
            let back = self.git_run(&["stash", "pop"])?;
            done.output.push('\n');
            done.output.push_str(&back.output);
            done.conflicts = back.conflicts;
            if !back.ok {
                done.output
                    .push_str("\nlocal changes kept in the stash: they conflict with the upstream");
            }
        }
        if let (Some(from), Some(to)) = (before, self.head_id())
            && from != to
        {
            out.info = Some(self.update_info(&from, &to)?);
        }
        out.reset = Some(done);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_status_and_commits_parse() {
        let text = "\0aaaaaaa1\n\nM\tsrc/a.rs\nR100\told.txt\tnew.txt\n\0bbbbbbb2\n\nA\tb.txt\n";
        let files = parse_name_status(text);
        assert_eq!(files.len(), 2);
        assert_eq!(
            files["aaaaaaa1"],
            [
                ChangedPath {
                    status: "M".into(),
                    path: "src/a.rs".into()
                },
                ChangedPath {
                    status: "R".into(),
                    path: "new.txt".into()
                }
            ]
        );
        assert_eq!(files["bbbbbbb2"][0].status, "A");
        let commits =
            parse_commits("0123456789\x00subject\x00Anna\x00a@example.invalid\x00100\nbad\n");
        assert_eq!(commits.len(), 1);
        assert_eq!(
            (commits[0].short_id.as_str(), commits[0].time),
            ("0123456", 100)
        );
        assert!(revision("-x").is_err() && revision("a b").is_err());
        assert_eq!(revision(" v1.0 ").unwrap(), "v1.0");
    }
}
