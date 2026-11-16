//! Reading and writing a Git repository with `gix` (`SPEC-0032`, `SPEC-0033`, `SPEC-0034`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diff::{LineChange, line_changes, unified_hunks};

/// What went wrong reading the repository.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// No repository at or above the path.
    #[error("{0} is not inside a Git repository")]
    NotARepository(String),
    /// The request was refused before anything was written.
    #[error("{0}")]
    Rejected(String),
    /// Any other error reported by `gix`.
    #[error("git: {0}")]
    Git(String),
    /// The file is neither in HEAD nor in the index (`SPEC-0277` behaviour 9).
    #[error("{0} is not tracked by Git")]
    Untracked(String),
    /// The file is in the index but in no commit yet (`SPEC-0277` behaviour 9).
    #[error("{0} has no committed version yet")]
    NotCommitted(String),
    /// A commit hook refused the commit (`SPEC-0230` behaviour 3); nothing was committed.
    #[error("commit blocked by {hook} hook ({status})\n{output}")]
    HookFailed {
        /// Hook name.
        hook: String,
        /// `exit 1`, or why it has no exit code.
        status: String,
        /// The hook's output.
        output: String,
    },
}

fn git<E: std::fmt::Display>(e: E) -> GitError {
    GitError::Git(e.to_string())
}

/// Kind of change of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeKind {
    /// New (staged) or untracked (unstaged).
    Added,
    /// Removed.
    Deleted,
    /// Content changed.
    Modified,
    /// Moved.
    Renamed,
    /// File type changed (e.g. file → symlink).
    TypeChange,
    /// Unmerged.
    Conflict,
}

/// Status of one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatus {
    /// Path relative to the repository root, `/`-separated.
    pub path: String,
    /// HEAD → index.
    pub staged: Option<ChangeKind>,
    /// Index → working tree.
    pub unstaged: Option<ChangeKind>,
}

/// A repository at a glance (`SPEC-0032` v3 behaviour 7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSummary {
    /// The branch HEAD points at; `None` while HEAD is detached.
    pub branch: Option<String>,
    /// HEAD points at a commit, not at a branch.
    pub detached: bool,
    /// Short id of the commit HEAD points at; `None` before the first commit.
    pub head: Option<String>,
    /// Paths with a staged change (HEAD → index).
    pub staged: usize,
    /// Tracked paths changed in the working tree but not staged (index → working tree); a path
    /// with both counts in `staged` and here.
    pub changed: usize,
    /// Untracked paths (ignored ones are not counted).
    pub untracked: usize,
    /// Paths with a merge conflict.
    pub conflicts: usize,
    /// Nothing staged, changed, untracked or in conflict.
    pub clean: bool,
}

/// Folders below `root` that are the working tree of a Git repository of their own, found by
/// their `.git` entry (`SPEC-0032` v3 behaviour 6): at most `depth` folders deep, at most `limit`
/// of them, in path order. `root` itself is not one of them. Symbolic links are not followed and
/// `.git` folders not entered; `node_modules` and `target` are skipped as dependency and build
/// output. The second value says whether the search stopped at `limit` or at its bound of
/// folders read, so some may be missing.
pub fn nested_repositories(root: &Path, depth: usize, limit: usize) -> (Vec<PathBuf>, bool) {
    const MAX_FOLDERS: usize = 20_000;
    let mut found = Vec::new();
    let mut read = 0;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut stopped = false;
    while let Some((dir, level)) = stack.pop() {
        if read >= MAX_FOLDERS {
            stopped = true;
            break;
        }
        read += 1;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                !matches!(
                    e.file_name().to_str(),
                    Some(".git" | "node_modules" | "target")
                )
            })
            .map(|e| e.path())
            .collect();
        children.sort();
        for child in children.into_iter().rev() {
            if child.join(".git").symlink_metadata().is_ok() {
                found.push(child.clone());
            }
            if level + 1 < depth {
                stack.push((child, level + 1));
            }
        }
    }
    found.sort();
    if found.len() > limit {
        found.truncate(limit);
        stopped = true;
    }
    (found, stopped)
}

/// A rename the index holds against HEAD (`SPEC-0335` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rename {
    /// The path in HEAD, relative to the repository root, `/`-separated.
    pub from: String,
    /// The path in the index.
    pub to: String,
}

/// The sides of a file with a merge conflict, from index stages 1–3 (`SPEC-0101`); a side's text
/// is `None` where the side has no such file (added on one side, deleted on the other) or it is
/// not text. `has_base`, `has_ours` and `has_theirs` tell the two apart (v6 behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    /// Path relative to the repository root, `/`-separated.
    pub path: String,
    /// The common ancestor.
    pub base: Option<String>,
    /// The current branch (HEAD).
    pub ours: Option<String>,
    /// The branch being merged, rebased or picked.
    pub theirs: Option<String>,
    /// Whether the common ancestor has the file (index stage 1).
    pub has_base: bool,
    /// Whether the current branch has the file (index stage 2).
    pub has_ours: bool,
    /// Whether the other branch has the file (index stage 3).
    pub has_theirs: bool,
    /// Whether a side that has the file is not text: it holds a NUL byte, is not valid UTF-8, or
    /// is no regular file (a symbolic link, a submodule). Such a file takes a whole side.
    pub binary: bool,
}

/// One commit of the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    /// Full id.
    pub id: String,
    /// First seven hex digits.
    pub short_id: String,
    /// First line of the message.
    pub summary: String,
    /// Author name.
    pub author: String,
    /// Author e-mail.
    pub email: String,
    /// Commit time, seconds since the epoch.
    pub time: i64,
    /// Full ids of the parents, first parent first (`SPEC-0086` v3 behaviours 7–8); empty where
    /// the source does not tell them.
    pub parents: Vec<String>,
    /// The committer as `Name <email>` when it differs from the author (`SPEC-0086` v7
    /// behaviour 15); `None` when they are the same or the source does not tell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committer: Option<String>,
}

/// One commit with its changes against its first parent (`SPEC-0086`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitDetails {
    /// Id, subject, author, time.
    pub info: CommitInfo,
    /// The whole message.
    pub message: String,
    /// Full ids of the parents.
    pub parents: Vec<String>,
    /// Files changed against the first parent (everything for the first commit).
    pub changes: Vec<CommitChange>,
}

/// Changes of one file against HEAD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    /// Path relative to the repository root.
    pub path: String,
    /// Whether either side is binary (no line changes then).
    pub binary: bool,
    /// Gutter markers.
    pub changes: Vec<LineChange>,
    /// Unified hunks.
    pub hunks: Vec<String>,
}

/// Blame of a run of lines (1-based, inclusive) in the working-tree content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlameRange {
    /// First line.
    pub start_line: usize,
    /// Last line.
    pub end_line: usize,
    /// The commit that last changed these lines; `None` for uncommitted lines.
    pub commit: Option<CommitInfo>,
}

/// A commit of a file's blame with its whole message (`SPEC-0277` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlameCommit {
    /// Id, summary, author, time.
    pub info: CommitInfo,
    /// The whole commit message.
    pub message: String,
}

/// Blame of a file at HEAD, to be mapped onto any later text of it (`SPEC-0277` behaviour 7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadBlame {
    /// Repository-relative path.
    pub path: String,
    /// The file's text at HEAD.
    pub head_text: String,
    /// Per HEAD line (as `str::lines` counts them), the index into `commits`.
    pub lines: Vec<Option<u32>>,
    /// The commits of the file's lines, each once.
    pub commits: Vec<BlameCommit>,
}

impl HeadBlame {
    /// The commit index of each line of `text`: unchanged lines keep theirs, inserted and modified
    /// lines get `None` (`SPEC-0033` behaviour 2).
    pub fn map(&self, text: &str) -> Vec<Option<u32>> {
        crate::diff::map_lines(&self.head_text, text, &self.lines)
    }

    /// `per_line` (from [`HeadBlame::map`]) as ranges of equal commits, 1-based and inclusive.
    pub fn ranges(&self, per_line: &[Option<u32>]) -> Vec<BlameRange> {
        let mut out: Vec<BlameRange> = Vec::new();
        let mut last: Option<Option<u32>> = None;
        for (i, &c) in per_line.iter().enumerate() {
            match out.last_mut() {
                Some(r) if last == Some(c) && r.end_line == i => r.end_line = i + 1,
                _ => out.push(BlameRange {
                    start_line: i + 1,
                    end_line: i + 1,
                    commit: c.and_then(|c| self.commits.get(c as usize).map(|b| b.info.clone())),
                }),
            }
            last = Some(c);
        }
        out
    }
}

/// One branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    /// Short name, e.g. `main` or `origin/main`.
    pub name: String,
    /// Whether it is a remote-tracking branch.
    pub remote: bool,
    /// Whether HEAD points at it.
    pub current: bool,
    /// Short id of the commit it points at.
    pub target: String,
}

/// Branches and the state of HEAD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branches {
    /// Current first, then local, then remote, each by name.
    pub branches: Vec<Branch>,
    /// Whether HEAD is detached (points at a commit, not a branch).
    pub detached: bool,
}

/// A path selected for a commit and how it changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitChange {
    /// Repository-relative path.
    pub path: String,
    /// Added, modified or deleted.
    pub kind: ChangeKind,
}

/// Result of a commit or its preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitOutcome {
    /// The new commit; `None` for a dry run.
    pub commit: Option<CommitInfo>,
    /// What is (or would be) committed.
    pub changes: Vec<CommitChange>,
    /// Hooks that ran, or would run in a dry run (`SPEC-0230` behaviour 8).
    pub hooks: Vec<crate::hooks::HookRun>,
}

/// Options of a commit beyond message and files (`SPEC-0063` v5 behaviours 13 and 16, `SPEC-0230`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOptions {
    /// Run `pre-commit` and `commit-msg` (`git commit` without `--no-verify`).
    pub verify: bool,
    /// Replace the last commit instead of adding one (`git commit --amend`).
    pub amend: bool,
    /// Author as `Name <email>`; `None` for the configured identity (or, when amending, the amended
    /// commit's author).
    pub author: Option<String>,
}

impl Default for CommitOptions {
    fn default() -> Self {
        Self {
            verify: true,
            amend: false,
            author: None,
        }
    }
}

/// Parses `Name <email>` (`SPEC-0063` v5 behaviour 16).
pub fn parse_author(text: &str) -> Result<(String, String), GitError> {
    let bad = || {
        GitError::Rejected(format!(
            "the author \"{text}\" is not of the form Name <email>"
        ))
    };
    let text = text.trim();
    let (name, rest) = text.split_once('<').ok_or_else(bad)?;
    let email = rest.strip_suffix('>').ok_or_else(bad)?;
    let name = name.trim();
    if name.is_empty() || email.trim().is_empty() || email.contains(['<', '>']) {
        return Err(bad());
    }
    Ok((name.to_owned(), email.trim().to_owned()))
}

/// What a rollback does (or would do) to each file (`SPEC-0063` v5 behaviour 10).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackOutcome {
    /// Files given their HEAD content in the working tree and the index.
    pub restored: Vec<String>,
    /// Files added since HEAD that leave the index and stay on disk, unversioned.
    pub unstaged: Vec<String>,
    /// Files added since HEAD that leave the index and are deleted.
    pub deleted: Vec<String>,
    /// Whether the repository was changed (false for a dry run).
    pub done: bool,
}

/// Result of a branch switch or its preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwitchOutcome {
    /// Target branch.
    pub branch: String,
    /// Whether the switch happened (false for a dry run or a refusal preview).
    pub switched: bool,
    /// Paths whose content changes.
    pub changes: Vec<CommitChange>,
    /// Paths with local changes in the way.
    pub conflicts: Vec<String>,
}

/// What a merge does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeKind {
    /// The source is already contained in HEAD.
    UpToDate,
    /// HEAD moves forward to the source.
    FastForward,
    /// A merge commit with two parents.
    Merge,
    /// The three-way merge has unresolved conflicts.
    Conflict,
}

/// Result of a merge or its preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeOutcome {
    /// What the merge does.
    pub kind: MergeKind,
    /// Whether the repository was changed.
    pub merged: bool,
    /// The new HEAD commit when merged.
    pub commit: Option<CommitInfo>,
    /// Paths whose content changes in the working tree.
    pub changes: Vec<CommitChange>,
    /// Paths with local changes in the way.
    pub local_conflicts: Vec<String>,
    /// Paths the three-way merge could not resolve.
    pub merge_conflicts: Vec<String>,
}

/// Files of a tree: path → (blob id, kind).
type Flat = BTreeMap<String, (gix::ObjectId, gix::object::tree::EntryKind)>;

/// Files of a tree, sub-trees flattened.
fn flat_tree(repo: &gix::Repository, id: gix::ObjectId) -> Result<Flat, GitError> {
    let mut recorder = gix::traverse::tree::Recorder::default();
    repo.find_tree(id)
        .map_err(git)?
        .traverse()
        .breadthfirst(&mut recorder)
        .map_err(git)?;
    Ok(recorder
        .records
        .into_iter()
        .filter(|e| !e.mode.is_tree())
        .map(|e| (e.filepath.to_string(), (e.oid, e.mode.kind())))
        .collect())
}

/// Paths that differ between two trees.
fn tree_changes(old: &Flat, new: &Flat) -> Vec<CommitChange> {
    let mut paths: Vec<&String> = old.keys().chain(new.keys()).collect();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| {
            let kind = match (old.get(path), new.get(path)) {
                (Some(a), Some(b)) if a == b => return None,
                (Some(_), Some(_)) => ChangeKind::Modified,
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Deleted,
                (None, None) => return None,
            };
            Some(CommitChange {
                path: path.clone(),
                kind,
            })
        })
        .collect()
}

/// The committer as `Name <email>` when name or e-mail differ from the author's (`SPEC-0086` v7
/// behaviour 15: the log marks such commits with `*`).
pub fn differing_committer(author: (&str, &str), committer: (&str, &str)) -> Option<String> {
    (author.0 != committer.0 || !author.1.eq_ignore_ascii_case(committer.1))
        .then(|| format!("{} <{}>", committer.0, committer.1))
}

/// An opened repository.
pub struct Repo {
    repo: gix::Repository,
}

impl Repo {
    /// Finds the repository containing `path` (`SPEC-0032` behaviour 1).
    pub fn discover(path: &Path) -> Result<Self, GitError> {
        let mut repo = gix::discover(path)
            .map_err(|_| GitError::NotARepository(path.display().to_string()))?;
        fallback_identity(&mut repo);
        Ok(Self { repo })
    }

    /// Working tree root.
    pub fn workdir(&self) -> Option<&Path> {
        self.repo.workdir()
    }

    /// The repository's Git folder (`.git`, or the worktree's folder in the main repository).
    pub fn git_dir(&self) -> &Path {
        self.repo.git_dir()
    }

    /// Staged and unstaged changes, sorted by path (behaviour 2).
    pub fn status(&self) -> Result<Vec<FileStatus>, GitError> {
        self.status_with_renames().map(|(status, _)| status)
    }

    /// The status, and the renames in the index it holds: for each path reported as renamed
    /// against HEAD the path it had there (`SPEC-0335` behaviour 4). A copy is not a rename.
    pub fn status_with_renames(&self) -> Result<(Vec<FileStatus>, Vec<Rename>), GitError> {
        use gix::status::index_worktree::iter::Summary;
        let mut by_path: BTreeMap<String, FileStatus> = BTreeMap::new();
        let mut renames = Vec::new();
        let iter = self
            .repo
            .status(gix::progress::Discard)
            .map_err(git)?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .into_iter(Vec::<gix::bstr::BString>::new())
            .map_err(git)?;
        for item in iter {
            let item = item.map_err(git)?;
            let path = item.location().to_string();
            let entry = by_path.entry(path.clone()).or_insert(FileStatus {
                path,
                staged: None,
                unstaged: None,
            });
            match &item {
                gix::status::Item::IndexWorktree(change) => {
                    entry.unstaged = change.summary().map(|s| match s {
                        Summary::Added | Summary::IntentToAdd => ChangeKind::Added,
                        Summary::Removed => ChangeKind::Deleted,
                        Summary::Modified => ChangeKind::Modified,
                        Summary::TypeChange => ChangeKind::TypeChange,
                        Summary::Renamed | Summary::Copied => ChangeKind::Renamed,
                        Summary::Conflict => ChangeKind::Conflict,
                    });
                }
                gix::status::Item::TreeIndex(change) => {
                    use gix::diff::index::ChangeRef;
                    entry.staged = Some(match change {
                        ChangeRef::Addition { .. } => ChangeKind::Added,
                        ChangeRef::Deletion { .. } => ChangeKind::Deleted,
                        ChangeRef::Modification { .. } => ChangeKind::Modified,
                        ChangeRef::Rewrite {
                            source_location,
                            location,
                            copy,
                            ..
                        } => {
                            if !*copy {
                                renames.push(Rename {
                                    from: source_location.to_string(),
                                    to: location.to_string(),
                                });
                            }
                            ChangeKind::Renamed
                        }
                    });
                }
            }
        }
        let status = by_path
            .into_values()
            .filter(|s| s.staged.is_some() || s.unstaged.is_some())
            .filter(|s| !is_untracked_ide_state(s))
            .collect();
        Ok((status, renames))
    }

    /// Commits reachable from HEAD, newest first (behaviour 3).
    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>, GitError> {
        let Ok(head) = self.repo.head_commit() else {
            return Ok(Vec::new());
        };
        self.walk_from(vec![head.id], limit)
    }

    /// Commits reachable from HEAD, every local and remote branch and every tag, newest first, so
    /// commits of other branches can be found and cherry-picked (`SPEC-0086` v2 behaviour 2).
    /// Stashes are left out.
    pub fn log_all(&self, limit: usize) -> Result<Vec<CommitInfo>, GitError> {
        let mut tips = Vec::new();
        if let Ok(head) = self.repo.head_commit() {
            tips.push(head.id);
        }
        let refs = self.repo.references().map_err(git)?;
        for iter in [
            refs.local_branches().map_err(git)?,
            refs.remote_branches().map_err(git)?,
            refs.tags().map_err(git)?,
        ] {
            for r in iter {
                let Ok(mut r) = r else {
                    continue;
                };
                let Ok(id) = r.peel_to_id() else {
                    continue;
                };
                // A tag may name a tree or blob; only commits start the walk.
                let Ok(commit) = self.repo.find_commit(id.detach()) else {
                    continue;
                };
                if !tips.contains(&commit.id) {
                    tips.push(commit.id);
                }
            }
        }
        if tips.is_empty() {
            return Ok(Vec::new());
        }
        self.walk_from(tips, limit)
    }

    /// Commits reachable from the revision `rev` (branch, remote branch, tag, id, `HEAD~2`, …),
    /// newest first (`SPEC-0086` v4 behaviour 9).
    pub fn log_from(&self, rev: &str, limit: usize) -> Result<Vec<CommitInfo>, GitError> {
        let tip = self.resolve_commit(rev)?.id;
        self.walk_from(vec![tip], limit)
    }

    /// Of the commits `ids`, those that change one of `paths` (repository-relative files or
    /// folders): the path's content differs from that in every parent, or, for a first commit,
    /// the path exists (`SPEC-0086` v5 behaviour 10).
    pub fn commits_touching(
        &self,
        ids: &[String],
        paths: &[String],
    ) -> Result<std::collections::HashSet<String>, GitError> {
        let paths: Vec<&str> = paths
            .iter()
            .map(|p| p.trim().trim_matches('/'))
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();
        let mut out = std::collections::HashSet::new();
        if paths.is_empty() {
            return Ok(out);
        }
        let entry_of =
            |tree: gix::ObjectId, path: &str| -> Result<Option<gix::ObjectId>, GitError> {
                let tree = self.repo.find_tree(tree).map_err(git)?;
                Ok(tree
                    .lookup_entry_by_path(path)
                    .map_err(git)?
                    .map(|e| e.object_id()))
            };
        for id in ids {
            let Ok(oid) = gix::ObjectId::from_hex(id.as_bytes()) else {
                continue;
            };
            let commit = self.repo.find_commit(oid).map_err(git)?;
            let tree = commit.tree_id().map_err(git)?.detach();
            let parents: Vec<gix::ObjectId> = commit
                .parent_ids()
                .map(|p| {
                    self.repo
                        .find_commit(p)
                        .map_err(git)?
                        .tree_id()
                        .map(|t| t.detach())
                        .map_err(git)
                })
                .collect::<Result<_, _>>()?;
            for path in &paths {
                let here = entry_of(tree, path)?;
                let changed = if parents.is_empty() {
                    here.is_some()
                } else {
                    let mut differs = true;
                    for &parent in &parents {
                        if entry_of(parent, path)? == here {
                            differs = false;
                            break;
                        }
                    }
                    differs
                };
                if changed {
                    out.insert(id.clone());
                    break;
                }
            }
        }
        Ok(out)
    }

    fn walk_from(
        &self,
        tips: Vec<gix::ObjectId>,
        limit: usize,
    ) -> Result<Vec<CommitInfo>, GitError> {
        let walk = self
            .repo
            .rev_walk(tips)
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                Default::default(),
            ))
            .all()
            .map_err(git)?;
        let mut out = Vec::new();
        for info in walk.take(limit) {
            let info = info.map_err(git)?;
            let commit = info.object().map_err(git)?;
            let message = commit.message().map_err(git)?;
            let author = commit.author().map_err(git)?;
            let committer = commit.committer().map_err(git)?;
            let id = info.id.to_string();
            out.push(CommitInfo {
                short_id: id[..7].to_owned(),
                id,
                summary: message.summary().to_string(),
                author: author.name.to_string(),
                email: author.email.to_string(),
                time: author.time().map_err(git)?.seconds,
                parents: info.parent_ids.iter().map(|p| p.to_string()).collect(),
                committer: differing_committer(
                    (&author.name.to_string(), &author.email.to_string()),
                    (&committer.name.to_string(), &committer.email.to_string()),
                ),
            });
        }
        Ok(out)
    }

    fn commit_info(&self, id: gix::ObjectId) -> Result<CommitInfo, GitError> {
        let commit = self.repo.find_commit(id).map_err(git)?;
        let message = commit.message().map_err(git)?;
        let author = commit.author().map_err(git)?;
        let committer = commit.committer().map_err(git)?;
        let parents = commit.parent_ids().map(|p| p.to_string()).collect();
        let id = id.to_string();
        Ok(CommitInfo {
            short_id: id[..7].to_owned(),
            id,
            summary: message.summary().to_string(),
            author: author.name.to_string(),
            email: author.email.to_string(),
            time: author.time().map_err(git)?.seconds,
            parents,
            committer: differing_committer(
                (&author.name.to_string(), &author.email.to_string()),
                (&committer.name.to_string(), &committer.email.to_string()),
            ),
        })
    }

    /// The seven-character id of the commit `rev` names (`HEAD~1`, `<id>^`, a branch, …).
    pub fn short_commit_id(&self, rev: &str) -> Result<String, GitError> {
        let id = self.resolve_commit(rev)?.id.to_string();
        Ok(id[..7].to_owned())
    }

    /// The commit `rev` names (full or short id, branch, tag, `HEAD~1`, …).
    fn resolve_commit(&self, rev: &str) -> Result<gix::Commit<'_>, GitError> {
        self.repo
            .rev_parse_single(rev)
            .map_err(|e| GitError::Rejected(format!("unknown revision {rev}: {e}")))?
            .object()
            .map_err(git)?
            .peel_to_commit()
            .map_err(git)
    }

    /// Tree ids of `commit` and of its first parent (`None` for the first commit).
    fn trees_of(
        &self,
        commit: &gix::Commit<'_>,
    ) -> Result<(Option<gix::ObjectId>, gix::ObjectId), GitError> {
        let tree = commit.tree_id().map_err(git)?.detach();
        let parent = match commit.parent_ids().next() {
            Some(p) => Some(
                self.repo
                    .find_commit(p)
                    .map_err(git)?
                    .tree_id()
                    .map_err(git)?
                    .detach(),
            ),
            None => None,
        };
        Ok((parent, tree))
    }

    /// Details and changed files of the commit `rev` names (`SPEC-0086` behaviour 4).
    pub fn commit_details(&self, rev: &str) -> Result<CommitDetails, GitError> {
        let commit = self.resolve_commit(rev)?;
        let (parent, tree) = self.trees_of(&commit)?;
        let old = match parent {
            Some(p) => flat_tree(&self.repo, p)?,
            None => Flat::new(),
        };
        let new = flat_tree(&self.repo, tree)?;
        Ok(CommitDetails {
            info: self.commit_info(commit.id)?,
            message: commit.message_raw().map_err(git)?.to_string(),
            parents: commit.parent_ids().map(|p| p.to_string()).collect(),
            changes: tree_changes(&old, &new),
        })
    }

    /// `path` before and after the commit `rev` names as side-by-side rows (`SPEC-0086`
    /// behaviour 5); empty for binary files.
    pub fn side_by_side_in_commit(
        &self,
        rev: &str,
        path: &str,
    ) -> Result<Vec<crate::DiffRow>, GitError> {
        let commit = self.resolve_commit(rev)?;
        let (parent, tree) = self.trees_of(&commit)?;
        let blob = |tree: Option<gix::ObjectId>| -> Result<Vec<u8>, GitError> {
            let Some(tree) = tree else {
                return Ok(Vec::new());
            };
            let tree = self.repo.find_tree(tree).map_err(git)?;
            Ok(match tree.lookup_entry_by_path(path).map_err(git)? {
                Some(entry) => entry.object().map_err(git)?.data.clone(),
                None => Vec::new(),
            })
        };
        let (old, new) = (blob(parent)?, blob(Some(tree))?);
        if old.contains(&0) || new.contains(&0) {
            return Ok(Vec::new());
        }
        Ok(crate::side_by_side(
            &String::from_utf8_lossy(&old),
            &String::from_utf8_lossy(&new),
        ))
    }

    /// Labels per full commit id: `HEAD` (only while detached), local branches, remote branches,
    /// tags (`SPEC-0086` v3 behaviour 2).
    pub fn ref_labels(&self) -> Result<BTreeMap<String, Vec<String>>, GitError> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Ok(head) = self.repo.head_id()
            && self.current_branch()?.is_none()
        {
            out.entry(head.to_string()).or_default().push("HEAD".into());
        }
        let refs = self.repo.references().map_err(git)?;
        for iter in [
            refs.local_branches().map_err(git)?,
            refs.remote_branches().map_err(git)?,
            refs.tags().map_err(git)?,
        ] {
            for r in iter {
                let mut r = r.map_err(git)?;
                let name = r.name().shorten().to_string();
                if name.ends_with("/HEAD") {
                    continue;
                }
                let Ok(id) = r.peel_to_id() else {
                    continue;
                };
                out.entry(id.to_string()).or_default().push(name);
            }
        }
        Ok(out)
    }

    /// Blame of `path` mapped onto its working-tree content (`SPEC-0033` behaviours 1–3).
    pub fn blame(&self, path: &str) -> Result<Vec<BlameRange>, GitError> {
        let new = self
            .workdir()
            .map(|w| w.join(path))
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        let new = String::from_utf8_lossy(&new).into_owned();
        let (old, head_lines) = self.head_line_commits(path)?.unwrap_or_default();
        let per_line = crate::diff::map_lines(&old, &new, &head_lines);
        let mut infos: BTreeMap<gix::ObjectId, CommitInfo> = BTreeMap::new();
        let mut out: Vec<BlameRange> = Vec::new();
        for (i, id) in per_line.into_iter().enumerate() {
            let commit = match id {
                Some(id) => Some(match infos.get(&id) {
                    Some(info) => info.clone(),
                    None => {
                        let info = self.commit_info(id)?;
                        infos.insert(id, info.clone());
                        info
                    }
                }),
                None => None,
            };
            match out.last_mut() {
                Some(last) if last.commit == commit && last.end_line == i => last.end_line = i + 1,
                _ => out.push(BlameRange {
                    start_line: i + 1,
                    end_line: i + 1,
                    commit,
                }),
            }
        }
        Ok(out)
    }

    /// The HEAD text of `path` and the commit of each of its lines; `None` when HEAD has no such
    /// file (or there is no HEAD yet).
    #[allow(clippy::type_complexity)]
    fn head_line_commits(
        &self,
        path: &str,
    ) -> Result<Option<(String, Vec<Option<gix::ObjectId>>)>, GitError> {
        let (Ok(head), Some(blob)) = (self.repo.head_commit(), self.head_blob(path)?) else {
            return Ok(None);
        };
        let old = String::from_utf8_lossy(&blob).into_owned();
        let outcome = self
            .repo
            .blame_file(path.into(), head.id, Default::default())
            .map_err(git)?;
        let mut head_lines = vec![None; old.lines().count()];
        for entry in outcome.entries {
            let start = entry.start_in_blamed_file as usize;
            for line in start..start + entry.len.get() as usize {
                if let Some(slot) = head_lines.get_mut(line) {
                    *slot = Some(entry.commit_id);
                }
            }
        }
        Ok(Some((old, head_lines)))
    }

    /// Blame of `path` (relative to the repository root) at HEAD with each commit's whole message
    /// (`SPEC-0277` behaviours 7 and 9): [`GitError::Untracked`] for a file neither in HEAD nor in
    /// the index, [`GitError::NotCommitted`] for one only in the index.
    pub fn blame_head(&self, path: &str) -> Result<HeadBlame, GitError> {
        let Some((head_text, ids)) = self.head_line_commits(path)? else {
            let index = self.repo.index_or_empty().map_err(git)?;
            return Err(if index.entry_by_path(path.into()).is_some() {
                GitError::NotCommitted(path.to_owned())
            } else {
                GitError::Untracked(path.to_owned())
            });
        };
        let mut at: BTreeMap<gix::ObjectId, u32> = BTreeMap::new();
        let mut commits = Vec::new();
        let mut lines = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(id) = id else {
                lines.push(None);
                continue;
            };
            let i = match at.get(&id) {
                Some(&i) => i,
                None => {
                    let message = self
                        .repo
                        .find_commit(id)
                        .map_err(git)?
                        .message_raw()
                        .map_err(git)?
                        .to_string();
                    commits.push(BlameCommit {
                        info: self.commit_info(id)?,
                        message: message.trim_end().to_owned(),
                    });
                    let i = (commits.len() - 1) as u32;
                    at.insert(id, i);
                    i
                }
            };
            lines.push(Some(i));
        }
        Ok(HeadBlame {
            path: path.to_owned(),
            head_text,
            lines,
            commits,
        })
    }

    /// Branch, HEAD and how many paths are staged, changed, untracked and in conflict
    /// (`SPEC-0032` v3 behaviour 7), counted from [`Self::status`].
    pub fn summary(&self) -> Result<RepoSummary, GitError> {
        let branch = self.current_branch()?;
        let head = self
            .repo
            .head_id()
            .ok()
            .map(|id| id.to_string()[..7].to_owned());
        let mut summary = RepoSummary {
            detached: branch.is_none() && head.is_some(),
            branch,
            head,
            ..RepoSummary::default()
        };
        for s in self.status()? {
            let conflict =
                s.staged == Some(ChangeKind::Conflict) || s.unstaged == Some(ChangeKind::Conflict);
            if conflict {
                summary.conflicts += 1;
                continue;
            }
            if s.staged.is_some() {
                summary.staged += 1;
            }
            match s.unstaged {
                Some(ChangeKind::Added) if s.staged.is_none() => summary.untracked += 1,
                Some(_) => summary.changed += 1,
                None => {}
            }
        }
        summary.clean =
            summary.staged + summary.changed + summary.untracked + summary.conflicts == 0;
        Ok(summary)
    }

    /// Short name of the branch HEAD points at; `None` while HEAD is detached (`SPEC-0086` v3
    /// behaviour 2).
    pub fn current_branch(&self) -> Result<Option<String>, GitError> {
        Ok(self
            .repo
            .head_name()
            .map_err(git)?
            .map(|n| n.shorten().to_string()))
    }

    /// Local and remote-tracking branches (`SPEC-0033` behaviour 4).
    pub fn branches(&self) -> Result<Branches, GitError> {
        let head = self.repo.head_name().map_err(git)?;
        let detached = head.is_none() && self.repo.head_id().is_ok();
        let refs = self.repo.references().map_err(git)?;
        let mut out = Vec::new();
        for (remote, iter) in [
            (false, refs.local_branches().map_err(git)?),
            (true, refs.remote_branches().map_err(git)?),
        ] {
            for r in iter {
                let mut r = r.map_err(git)?;
                let full = r.name().as_bstr().to_string();
                let name = r.name().shorten().to_string();
                if remote && name.ends_with("/HEAD") {
                    continue;
                }
                let target = r
                    .peel_to_id()
                    .map(|id| id.to_string()[..7].to_owned())
                    .unwrap_or_default();
                out.push(Branch {
                    current: head.as_ref().is_some_and(|h| h.as_bstr() == full.as_str()),
                    name,
                    remote,
                    target,
                });
            }
        }
        out.sort_by(|a, b| (!a.current, a.remote, &a.name).cmp(&(!b.current, b.remote, &b.name)));
        Ok(Branches {
            branches: out,
            detached,
        })
    }

    /// Commits the working-tree state of `paths` on top of HEAD (`SPEC-0034` behaviours 1–5), running
    /// the repository's commit hooks (`SPEC-0230`).
    pub fn commit(
        &self,
        message: &str,
        paths: &[String],
        dry_run: bool,
    ) -> Result<CommitOutcome, GitError> {
        self.commit_with(message, paths, dry_run, true)
    }

    /// As `commit`; without `verify` the `pre-commit` and `commit-msg` hooks are skipped
    /// (`SPEC-0230` behaviour 6).
    pub fn commit_with(
        &self,
        message: &str,
        paths: &[String],
        dry_run: bool,
        verify: bool,
    ) -> Result<CommitOutcome, GitError> {
        let options = CommitOptions {
            verify,
            ..CommitOptions::default()
        };
        self.commit_opts(message, paths, dry_run, &options)
    }

    /// As `commit_with`, with amend and author (`SPEC-0063` v5 behaviours 13 and 16): an amend
    /// replaces HEAD by a commit with HEAD's parents, HEAD's tree plus `paths` and HEAD's author
    /// (unless one is given); it needs no path.
    pub fn commit_opts(
        &self,
        message: &str,
        paths: &[String],
        dry_run: bool,
        options: &CommitOptions,
    ) -> Result<CommitOutcome, GitError> {
        self.commit_content(message, paths, dry_run, options, None)
    }

    /// Commits `content` as the new text of `path` (repository-relative), leaving the working file
    /// as it is (`SPEC-0064` v3 behaviour 8: one change of a file). Refused for a file not in HEAD,
    /// one with a merge conflict and one whose index differs from HEAD (staged changes).
    pub fn commit_change(
        &self,
        message: &str,
        path: &str,
        content: &str,
        dry_run: bool,
        options: &CommitOptions,
    ) -> Result<CommitOutcome, GitError> {
        let path = path.replace('\\', "/");
        let Some((head_id, _)) = self.head_entry(&path) else {
            return Err(GitError::Rejected(format!(
                "{path} has no committed version: commit the whole file"
            )));
        };
        if self.conflicts()?.iter().any(|c| c.path == path) {
            return Err(GitError::Rejected(format!(
                "{path} has a merge conflict: resolve it first"
            )));
        }
        let index = self.repo.index_or_empty().map_err(git)?;
        if index
            .entry_by_path(path.as_str().into())
            .is_some_and(|e| e.id != head_id)
        {
            return Err(GitError::Rejected(format!(
                "{path} has staged changes: commit them from the Commit tool window"
            )));
        }
        self.commit_content(
            message,
            std::slice::from_ref(&path),
            dry_run,
            options,
            Some((&path, content.as_bytes())),
        )
    }

    fn commit_content(
        &self,
        message: &str,
        paths: &[String],
        dry_run: bool,
        options: &CommitOptions,
        content: Option<(&str, &[u8])>,
    ) -> Result<CommitOutcome, GitError> {
        use gix::object::tree::EntryKind;
        let reject = |m: String| Err(GitError::Rejected(m));
        let verify = options.verify;
        if message.trim().is_empty() {
            return reject("the commit message is empty".into());
        }
        if paths.is_empty() && !options.amend {
            return reject("no files selected".into());
        }
        let author = options
            .author
            .as_deref()
            .filter(|a| !a.trim().is_empty())
            .map(parse_author)
            .transpose()?;
        let workdir = self
            .workdir()
            .ok_or_else(|| GitError::Rejected("the repository has no working tree".into()))?
            .to_path_buf();
        let head = self.repo.head_commit().ok();
        if options.amend && head.is_none() {
            return reject("there is no commit to amend".into());
        }
        let base_tree = match &head {
            Some(c) => c.tree_id().map_err(git)?.detach(),
            None => gix::ObjectId::empty_tree(self.repo.object_hash()),
        };
        let mut editor = self.repo.edit_tree(base_tree).map_err(git)?;
        let mut changes = Vec::new();
        // (path, Some((id, mode, stat)) to stage, None to remove)
        let mut index_updates = Vec::new();
        let mut all_same = true;
        let mut blobs = Vec::new();
        for path in paths {
            let p = Path::new(path);
            if p.is_absolute()
                || p.components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return reject(format!("{path}: only plain repository-relative paths"));
            }
            let path = path.replace('\\', "/");
            let file = workdir.join(&path);
            let in_head = self.head_blob(&path)?;
            match std::fs::symlink_metadata(&file) {
                Ok(meta) if meta.is_dir() => return reject(format!("{path} is a directory")),
                Ok(meta) => {
                    let (bytes, kind, mode) = if meta.file_type().is_symlink() {
                        let target = std::fs::read_link(&file).map_err(git)?;
                        (
                            target.to_string_lossy().into_owned().into_bytes(),
                            EntryKind::Link,
                            gix::index::entry::Mode::SYMLINK,
                        )
                    } else if is_executable(&meta) {
                        (
                            std::fs::read(&file).map_err(git)?,
                            EntryKind::BlobExecutable,
                            gix::index::entry::Mode::FILE_EXECUTABLE,
                        )
                    } else {
                        (
                            std::fs::read(&file).map_err(git)?,
                            EntryKind::Blob,
                            gix::index::entry::Mode::FILE,
                        )
                    };
                    // Given content stands in for the file; the index then knows no stat of it,
                    // so Git compares the file's content again.
                    let given = content.filter(|(p, _)| *p == path).map(|(_, b)| b.to_vec());
                    let stat_known = given.is_none();
                    let bytes = given.unwrap_or(bytes);
                    let id = gix::objs::compute_hash(
                        self.repo.object_hash(),
                        gix::object::Kind::Blob,
                        &bytes,
                    )
                    .map_err(git)?;
                    blobs.push(bytes);
                    if self.head_entry(&path) != Some((id, kind)) {
                        all_same = false;
                    }
                    editor.upsert(path.as_str(), kind, id).map_err(git)?;
                    let stat = stat_known
                        .then(|| {
                            gix::index::fs::Metadata::from_path_no_follow(&file)
                                .ok()
                                .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
                        })
                        .flatten()
                        .unwrap_or_default();
                    index_updates.push((path.clone(), Some((id, mode, stat))));
                    let kind = if in_head.is_some() {
                        ChangeKind::Modified
                    } else {
                        ChangeKind::Added
                    };
                    changes.push(CommitChange { path, kind });
                }
                Err(_) if in_head.is_some() => {
                    all_same = false;
                    editor.remove(path.as_str()).map_err(git)?;
                    index_updates.push((path.clone(), None));
                    changes.push(CommitChange {
                        path,
                        kind: ChangeKind::Deleted,
                    });
                }
                Err(_) => return reject(format!("{path} neither exists nor is committed")),
            }
        }
        if all_same && !options.amend {
            return reject("nothing to commit: the selected files equal HEAD".into());
        }
        let hooks = self.commit_hooks(verify);
        if dry_run {
            return Ok(CommitOutcome {
                commit: None,
                changes,
                hooks: hooks
                    .into_iter()
                    .map(|hook| crate::hooks::HookRun {
                        hook,
                        exit: None,
                        output: String::new(),
                    })
                    .collect(),
            });
        }
        // Objects are only written once every path is valid.
        for bytes in &blobs {
            self.repo.write_blob(bytes).map_err(git)?;
        }
        let planned = editor.write().map_err(git)?.detach();
        let (tree, message, mut runs) = if hooks.is_empty() {
            (planned, message.to_owned(), Vec::new())
        } else {
            self.run_commit_hooks(planned, message, verify, options.amend)?
        };
        if tree != planned {
            // A hook staged more (a formatter's `git add`): the index follows the committed tree.
            let before = flat_tree(&self.repo, planned)?;
            let after = flat_tree(&self.repo, tree)?;
            for change in tree_changes(&before, &after) {
                index_updates.retain(|(p, _)| *p != change.path);
                changes.retain(|c| c.path != change.path);
                let update = after.get(&change.path).map(|(id, kind)| {
                    let mode = match kind {
                        EntryKind::BlobExecutable => gix::index::entry::Mode::FILE_EXECUTABLE,
                        EntryKind::Link => gix::index::entry::Mode::SYMLINK,
                        _ => gix::index::entry::Mode::FILE,
                    };
                    let file = workdir.join(&change.path);
                    let same = std::fs::read(&file).ok().is_some_and(|b| {
                        gix::objs::compute_hash(
                            self.repo.object_hash(),
                            gix::object::Kind::Blob,
                            &b,
                        )
                        .ok()
                            == Some(*id)
                    });
                    let stat = same
                        .then(|| {
                            gix::index::fs::Metadata::from_path_no_follow(&file)
                                .ok()
                                .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
                        })
                        .flatten()
                        .unwrap_or_default();
                    (*id, mode, stat)
                });
                index_updates.push((change.path.clone(), update));
                let kind = match (
                    self.head_entry(&change.path),
                    after.contains_key(&change.path),
                ) {
                    (Some(_), true) => ChangeKind::Modified,
                    (None, _) => ChangeKind::Added,
                    (Some(_), false) => ChangeKind::Deleted,
                };
                changes.push(CommitChange {
                    path: change.path,
                    kind,
                });
            }
            changes.sort_by(|a, b| a.path.cmp(&b.path));
        }
        let id = if options.amend || author.is_some() {
            self.write_commit(tree, &message, head.as_ref(), options.amend, author)?
        } else {
            let parents: Vec<gix::ObjectId> = head.iter().map(|c| c.id).collect();
            self.repo
                .commit("HEAD", message.as_str(), tree, parents)
                .map_err(git)?
                .detach()
        };
        // Bring the index in line for the committed paths only.
        let mut index: gix::index::File = (**self.repo.index_or_empty().map_err(git)?).clone();
        let removed: Vec<&str> = index_updates.iter().map(|(p, _)| p.as_str()).collect();
        index.remove_entries(|_, p, _| removed.iter().any(|r| p == r.as_bytes()));
        for (path, update) in &index_updates {
            if let Some((oid, mode, stat)) = update {
                index.dangerously_push_entry(
                    *stat,
                    *oid,
                    gix::index::entry::Flags::empty(),
                    *mode,
                    path.as_str().into(),
                );
            }
        }
        index.sort_entries();
        // The cached trees no longer match; Git rebuilds them on demand.
        index.remove_tree();
        index.write(Default::default()).map_err(git)?;
        if hooks.iter().any(|h| h == "post-commit")
            && let Some(run) = self.run_hook("post-commit", &[], &[])?
        {
            runs.push(run);
        }
        Ok(CommitOutcome {
            commit: Some(self.commit_info(id)?),
            changes,
            hooks: runs,
        })
    }

    /// Runs `pre-commit`, `prepare-commit-msg` and `commit-msg` against a temporary index holding
    /// `tree` (`SPEC-0230` behaviours 2–4); returns the tree and message to commit and the runs.
    fn run_commit_hooks(
        &self,
        tree: gix::ObjectId,
        message: &str,
        verify: bool,
        amend: bool,
    ) -> Result<(gix::ObjectId, String, Vec<crate::hooks::HookRun>), GitError> {
        let workdir = self
            .workdir()
            .ok_or_else(|| GitError::Rejected("the repository has no working tree".into()))?;
        let work = self.git_dir().join("idecraft");
        std::fs::create_dir_all(&work).map_err(git)?;
        let index = work.join(format!("commit-index-{}", std::process::id()));
        struct Remove<'a>(&'a Path);
        impl Drop for Remove<'_> {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(self.0);
            }
        }
        let _remove = Remove(&index);
        let git_with_index = |args: &[&str]| -> Result<String, GitError> {
            let out = std::process::Command::new("git")
                .current_dir(workdir)
                .env("GIT_INDEX_FILE", &index)
                .args(args)
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| GitError::Git(format!("cannot start git: {e}")))?;
            if !out.status.success() {
                return Err(GitError::Git(
                    String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                ));
            }
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        };
        git_with_index(&["read-tree", &tree.to_string()])?;
        let msg_file = self.git_dir().join("COMMIT_EDITMSG");
        let mut text = message.to_owned();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        std::fs::write(&msg_file, text).map_err(git)?;
        let msg_arg = msg_file.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", index.as_path())];
        let mut runs = Vec::new();
        let steps: [(&str, Vec<&str>, bool); 3] = [
            ("pre-commit", vec![], verify),
            (
                "prepare-commit-msg",
                if amend {
                    vec![msg_arg.as_str(), "commit", "HEAD"]
                } else {
                    vec![msg_arg.as_str(), "message"]
                },
                true,
            ),
            ("commit-msg", vec![msg_arg.as_str()], verify),
        ];
        for (hook, args, enabled) in steps {
            if !enabled {
                continue;
            }
            let Some(run) = self.run_hook(hook, &args, &env)? else {
                continue;
            };
            if run.exit != Some(0) {
                return Err(GitError::HookFailed {
                    hook: hook.to_owned(),
                    status: run
                        .exit
                        .map_or("no exit code".to_owned(), |c| format!("exit {c}")),
                    output: run.output,
                });
            }
            runs.push(run);
        }
        let message =
            crate::hooks::clean_message(&std::fs::read_to_string(&msg_file).map_err(git)?);
        if message.trim().is_empty() {
            return Err(GitError::Rejected(
                "the commit message is empty after the commit hooks".into(),
            ));
        }
        let tree = git_with_index(&["write-tree"])?;
        let tree = gix::ObjectId::from_hex(tree.as_bytes()).map_err(git)?;
        Ok((tree, message, runs))
    }

    /// Writes a commit of `tree` and moves HEAD (or the branch it points at) to it: on top of `head`,
    /// or with `head`'s parents when amending. The author is `author`, else the amended commit's,
    /// else the committer (`SPEC-0063` v5 behaviours 13 and 16).
    fn write_commit(
        &self,
        tree: gix::ObjectId,
        message: &str,
        head: Option<&gix::Commit<'_>>,
        amend: bool,
        author: Option<(String, String)>,
    ) -> Result<gix::ObjectId, GitError> {
        use gix::refs::Target;
        use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
        let committer: gix::actor::Signature = self
            .repo
            .committer()
            .ok_or_else(|| GitError::Git("no committer identity configured".into()))?
            .map_err(git)?
            .into();
        let author = match (author, head.filter(|_| amend)) {
            (Some((name, email)), _) => gix::actor::Signature {
                name: name.into(),
                email: email.into(),
                time: committer.time,
            },
            (None, Some(h)) => h.author().map_err(git)?.into(),
            (None, None) => committer.clone(),
        };
        let parents: Vec<gix::ObjectId> = match head {
            Some(h) if amend => h.parent_ids().map(|p| p.detach()).collect(),
            Some(h) => vec![h.id],
            None => Vec::new(),
        };
        let summary = message.lines().next().unwrap_or_default();
        let commit = gix::objs::Commit {
            tree,
            parents: parents.into_iter().collect(),
            author,
            committer,
            encoding: None,
            message: message.into(),
            extra_headers: Vec::new(),
        };
        let id = self.repo.write_object(&commit).map_err(git)?.detach();
        let operation = if amend { "commit (amend)" } else { "commit" };
        self.repo
            .edit_reference(RefEdit {
                change: Change::Update {
                    log: LogChange {
                        mode: RefLog::AndReference,
                        force_create_reflog: false,
                        message: format!("{operation}: {summary}").into(),
                    },
                    expected: match head {
                        Some(h) => PreviousValue::MustExistAndMatch(Target::Object(h.id)),
                        None => PreviousValue::MustNotExist,
                    },
                    new: Target::Object(id),
                },
                name: "HEAD".try_into().map_err(git)?,
                deref: true,
            })
            .map_err(git)?;
        Ok(id)
    }

    /// Rolls `paths` back to HEAD (`SPEC-0063` v5 behaviour 10): a file in HEAD gets its HEAD
    /// content in the working tree and the index; a file added since leaves the index and stays on
    /// disk, or is deleted with `delete_added`. Untracked files are refused. A dry run only reports.
    pub fn rollback(
        &self,
        paths: &[String],
        delete_added: bool,
        dry_run: bool,
    ) -> Result<RollbackOutcome, GitError> {
        let reject = |m: String| Err(GitError::Rejected(m));
        if paths.is_empty() {
            return reject("no files selected".into());
        }
        let workdir = self
            .workdir()
            .ok_or_else(|| GitError::Rejected("the repository has no working tree".into()))?
            .to_path_buf();
        let index = self.repo.index_or_empty().map_err(git)?;
        let mut out = RollbackOutcome::default();
        for path in paths {
            let p = Path::new(path);
            if p.is_absolute()
                || p.components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return reject(format!("{path}: only plain repository-relative paths"));
            }
            let path = path.replace('\\', "/");
            if self.head_entry(&path).is_some() {
                out.restored.push(path);
            } else if index.entry_by_path(path.as_str().into()).is_some() {
                if delete_added {
                    out.deleted.push(path);
                } else {
                    out.unstaged.push(path);
                }
            } else {
                return Err(GitError::Untracked(path));
            }
        }
        if dry_run {
            return Ok(out);
        }
        let run = |args: Vec<&str>| -> Result<(), GitError> {
            let (ok, _, err, timed_out) =
                crate::remote::run_git(&workdir, &args, crate::LOCAL_TIMEOUT)?;
            if timed_out {
                return Err(GitError::Git("git timed out".into()));
            }
            if !ok {
                return Err(GitError::Git(err.trim().to_owned()));
            }
            Ok(())
        };
        if !out.restored.is_empty() {
            let mut args = vec!["restore", "--source=HEAD", "--staged", "--worktree", "--"];
            args.extend(out.restored.iter().map(String::as_str));
            run(args)?;
        }
        let added: Vec<&str> = out
            .unstaged
            .iter()
            .chain(&out.deleted)
            .map(String::as_str)
            .collect();
        if !added.is_empty() {
            let mut args = vec!["rm", "--cached", "-q", "-f", "--"];
            args.extend(added);
            run(args)?;
        }
        for path in &out.deleted {
            match std::fs::remove_file(workdir.join(path)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(git(e)),
            }
        }
        out.done = true;
        Ok(out)
    }

    /// Id and kind of `path` in HEAD's tree.
    fn head_entry(&self, path: &str) -> Option<(gix::ObjectId, gix::object::tree::EntryKind)> {
        let tree = self.repo.head_tree().ok()?;
        let entry = tree.lookup_entry_by_path(path).ok()??;
        Some((entry.oid().to_owned(), entry.mode().kind()))
    }

    /// Creates branch `name` at `start` (default HEAD) without switching (`SPEC-0034` behaviour 6).
    pub fn create_branch(&self, name: &str, start: Option<&str>) -> Result<Branch, GitError> {
        let target = self
            .repo
            .rev_parse_single(start.unwrap_or("HEAD"))
            .map_err(|e| GitError::Rejected(format!("unknown start revision: {e}")))?
            .object()
            .map_err(git)?
            .peel_to_commit()
            .map_err(git)?
            .id;
        let full = format!("refs/heads/{name}");
        if self
            .repo
            .try_find_reference(full.as_str())
            .map_err(git)?
            .is_some()
        {
            return Err(GitError::Rejected(format!("branch {name} already exists")));
        }
        self.repo
            .reference(
                full.as_str(),
                target,
                gix::refs::transaction::PreviousValue::MustNotExist,
                format!("branch: Created from {}", start.unwrap_or("HEAD")),
            )
            .map_err(|e| GitError::Rejected(format!("invalid branch name {name}: {e}")))?;
        Ok(Branch {
            name: name.to_owned(),
            remote: false,
            current: false,
            target: target.to_string()[..7].to_owned(),
        })
    }

    /// Files of HEAD's tree (empty for an unborn HEAD).
    fn head_flat(&self) -> Result<Flat, GitError> {
        match self.repo.head_commit() {
            Ok(c) => flat_tree(&self.repo, c.tree_id().map_err(git)?.detach()),
            Err(_) => Ok(BTreeMap::new()),
        }
    }

    /// Paths of `changes` that have a local change (staged, unstaged or untracked).
    fn local_conflicts(&self, changes: &[CommitChange]) -> Result<Vec<String>, GitError> {
        let local = self.status()?;
        Ok(changes
            .iter()
            .filter(|c| local.iter().any(|s| s.path == c.path))
            .map(|c| c.path.clone())
            .collect())
    }

    /// Writes `changes` to the working tree and the index as they are in `new`.
    fn write_tree_changes(&self, changes: &[CommitChange], new: &Flat) -> Result<(), GitError> {
        use gix::object::tree::EntryKind;
        let workdir = self
            .workdir()
            .ok_or_else(|| GitError::Rejected("the repository has no working tree".into()))?
            .to_path_buf();
        let mut index: gix::index::File = (**self.repo.index_or_empty().map_err(git)?).clone();
        let touched: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
        index.remove_entries(|_, p, _| touched.iter().any(|t| p == t.as_bytes()));
        for change in changes {
            let file = workdir.join(&change.path);
            if file.symlink_metadata().is_ok() {
                std::fs::remove_file(&file).map_err(git)?;
            }
            let Some((id, kind)) = new.get(&change.path) else {
                // Remove directories that became empty, up to the working tree root.
                let mut dir = file.parent();
                while let Some(d) = dir {
                    if d == workdir || std::fs::remove_dir(d).is_err() {
                        break;
                    }
                    dir = d.parent();
                }
                continue;
            };
            let data = self.repo.find_object(*id).map_err(git)?.data.clone();
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(git)?;
            }
            let mode = match kind {
                EntryKind::Link => {
                    write_symlink(&String::from_utf8_lossy(&data), &file)?;
                    gix::index::entry::Mode::SYMLINK
                }
                EntryKind::BlobExecutable => {
                    std::fs::write(&file, &data).map_err(git)?;
                    set_executable(&file)?;
                    gix::index::entry::Mode::FILE_EXECUTABLE
                }
                EntryKind::Blob => {
                    std::fs::write(&file, &data).map_err(git)?;
                    gix::index::entry::Mode::FILE
                }
                _ => continue,
            };
            let stat = gix::index::fs::Metadata::from_path_no_follow(&file)
                .ok()
                .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
                .unwrap_or_default();
            index.dangerously_push_entry(
                stat,
                *id,
                gix::index::entry::Flags::empty(),
                mode,
                change.path.as_str().into(),
            );
        }
        index.sort_entries();
        index.remove_tree();
        index.write(Default::default()).map_err(git)?;
        Ok(())
    }

    /// Merges `source` into HEAD (`SPEC-0036`).
    pub fn merge(&self, source: &str, dry_run: bool) -> Result<MergeOutcome, GitError> {
        let head = self
            .repo
            .head_commit()
            .map_err(|_| GitError::Rejected("HEAD does not point at a commit".into()))?
            .id;
        let theirs = self
            .repo
            .rev_parse_single(source)
            .map_err(|e| GitError::Rejected(format!("unknown revision {source}: {e}")))?
            .object()
            .map_err(git)?
            .peel_to_commit()
            .map_err(git)?
            .id;
        let mut outcome = MergeOutcome {
            kind: MergeKind::UpToDate,
            merged: false,
            commit: None,
            changes: Vec::new(),
            local_conflicts: Vec::new(),
            merge_conflicts: Vec::new(),
        };
        let base = self.repo.merge_base(head, theirs).ok().map(|b| b.detach());
        if base == Some(theirs) {
            return Ok(outcome);
        }
        let old = self.head_flat()?;
        // A preview must not leave objects behind.
        let repo = if dry_run {
            self.repo.clone().with_object_memory()
        } else {
            self.repo.clone()
        };
        let new_tree = if base == Some(head) {
            outcome.kind = MergeKind::FastForward;
            repo.find_commit(theirs)
                .map_err(git)?
                .tree_id()
                .map_err(git)?
                .detach()
        } else {
            outcome.kind = MergeKind::Merge;
            let options = repo.tree_merge_options().map_err(git)?;
            let labels = gix::merge::blob::builtin_driver::text::Labels {
                ancestor: None,
                current: Some("HEAD".into()),
                other: Some(source.into()),
            };
            let mut merged = repo
                .merge_commits(head, theirs, labels, options.into())
                .map_err(git)?;
            let how = gix::merge::tree::TreatAsUnresolved::default();
            if merged.tree_merge.has_unresolved_conflicts(how) {
                outcome.kind = MergeKind::Conflict;
                let mut paths: Vec<String> = merged
                    .tree_merge
                    .conflicts
                    .iter()
                    .filter(|c| c.is_unresolved(how))
                    .map(|c| c.ours.location().to_string())
                    .collect();
                paths.sort();
                paths.dedup();
                outcome.merge_conflicts = paths;
                if dry_run {
                    // Local changes in files the merge touches stop Git before it merges, also
                    // when the merge itself would conflict; the preview names them so the IDE can
                    // offer a smart merge (`SPEC-0231` v2 behaviour 7, OBS-0022 L2).
                    let id = merged.tree_merge.tree.write().map_err(git)?.detach();
                    let changes = tree_changes(&old, &flat_tree(&repo, id)?);
                    outcome.local_conflicts = self.local_conflicts(&changes)?;
                    return Ok(outcome);
                }
                return Err(GitError::Rejected(format!(
                    "merge conflicts in: {}",
                    outcome.merge_conflicts.join(", ")
                )));
            }
            merged.tree_merge.tree.write().map_err(git)?.detach()
        };
        let new = flat_tree(&repo, new_tree)?;
        outcome.changes = tree_changes(&old, &new);
        outcome.local_conflicts = self.local_conflicts(&outcome.changes)?;
        if !outcome.local_conflicts.is_empty() && !dry_run {
            return Err(GitError::Rejected(format!(
                "local changes would be overwritten: {}",
                outcome.local_conflicts.join(", ")
            )));
        }
        if dry_run {
            return Ok(outcome);
        }
        // Like Git: working tree and index first, then the reference.
        self.write_tree_changes(&outcome.changes, &new)?;
        let id = if outcome.kind == MergeKind::FastForward {
            use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
            self.repo
                .edit_reference(RefEdit {
                    change: Change::Update {
                        log: LogChange {
                            mode: RefLog::AndReference,
                            force_create_reflog: false,
                            message: format!("merge {source}: Fast-forward").into(),
                        },
                        expected: PreviousValue::MustExistAndMatch(gix::refs::Target::Object(head)),
                        new: gix::refs::Target::Object(theirs),
                    },
                    name: "HEAD".try_into().map_err(git)?,
                    deref: true,
                })
                .map_err(git)?;
            theirs
        } else {
            let current = self
                .repo
                .head_name()
                .map_err(git)?
                .map(|n| n.shorten().to_string());
            let is_branch = self
                .repo
                .try_find_reference(format!("refs/heads/{source}").as_str())
                .map_err(git)?
                .is_some();
            // Git's own wording, also for a remote-tracking branch such as `origin/main` when an
            // update merges (`SPEC-0144` v3 behaviour 6, OBS-0022 L19).
            let is_remote = !is_branch
                && self
                    .repo
                    .try_find_reference(format!("refs/remotes/{source}").as_str())
                    .map_err(git)?
                    .is_some();
            let mut message = if is_branch {
                format!("Merge branch '{source}'")
            } else if is_remote {
                format!("Merge remote-tracking branch '{source}'")
            } else {
                format!("Merge commit '{source}'")
            };
            if let Some(current) = current.filter(|c| c != "main" && c != "master") {
                message.push_str(&format!(" into {current}"));
            }
            self.repo
                .commit("HEAD", message, new_tree, [head, theirs])
                .map_err(git)?
                .detach()
        };
        outcome.merged = true;
        outcome.commit = Some(self.commit_info(id)?);
        Ok(outcome)
    }

    /// Switches to local branch `name` (`SPEC-0035`).
    pub fn switch_branch(&self, name: &str, dry_run: bool) -> Result<SwitchOutcome, GitError> {
        let full = format!("refs/heads/{name}");
        let mut target_ref = self
            .repo
            .try_find_reference(full.as_str())
            .map_err(git)?
            .ok_or_else(|| GitError::Rejected(format!("no local branch {name}")))?;
        let target = target_ref.peel_to_id().map_err(git)?.detach();
        let head_name = self.repo.head_name().map_err(git)?;
        let mut outcome = SwitchOutcome {
            branch: name.to_owned(),
            switched: false,
            changes: Vec::new(),
            conflicts: Vec::new(),
        };
        if head_name
            .as_ref()
            .is_some_and(|h| h.as_bstr() == full.as_str())
        {
            return Ok(outcome);
        }
        let old_id = self.repo.head_id().ok().map(|id| id.detach());
        let old = self.head_flat()?;
        let new_tree = self
            .repo
            .find_commit(target)
            .map_err(git)?
            .tree_id()
            .map_err(git)?
            .detach();
        let new = flat_tree(&self.repo, new_tree)?;
        outcome.changes = tree_changes(&old, &new);
        outcome.conflicts = self.local_conflicts(&outcome.changes)?;
        if !outcome.conflicts.is_empty() {
            if dry_run {
                return Ok(outcome);
            }
            return Err(GitError::Rejected(format!(
                "local changes would be overwritten: {}",
                outcome.conflicts.join(", ")
            )));
        }
        if dry_run {
            return Ok(outcome);
        }
        self.write_tree_changes(&outcome.changes, &new)?;
        let from = head_name
            .as_ref()
            .map(|h| h.shorten().to_string())
            .or_else(|| self.repo.head_id().ok().map(|id| id.to_string()))
            .unwrap_or_default();
        use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
        self.repo
            .edit_reference(RefEdit {
                change: Change::Update {
                    log: LogChange {
                        mode: RefLog::AndReference,
                        force_create_reflog: false,
                        message: Default::default(),
                    },
                    expected: PreviousValue::Any,
                    new: gix::refs::Target::Symbolic(target_ref.name().to_owned()),
                },
                name: "HEAD".try_into().map_err(git)?,
                deref: false,
            })
            .map_err(git)?;
        // gix keeps no reflog for symbolic updates; Git records checkouts in HEAD's log.
        if let Some(Ok(who)) = self.repo.committer() {
            let previous = old_id.unwrap_or_else(|| gix::ObjectId::null(self.repo.object_hash()));
            let line = format!(
                "{previous} {target} {} <{}> {}\tcheckout: moving from {from} to {name}\n",
                who.name, who.email, who.time
            );
            let log = self.repo.git_dir().join("logs").join("HEAD");
            if let Some(dir) = log.parent() {
                std::fs::create_dir_all(dir).map_err(git)?;
            }
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .and_then(|mut f| f.write_all(line.as_bytes()))
                .map_err(git)?;
        }
        outcome.switched = true;
        Ok(outcome)
    }

    /// Checks out a branch for the branch popup (`SPEC-0068` behaviour 4): a local branch is switched
    /// to; a remote-tracking branch (`origin/x`) is switched to its local branch `x`, which is created
    /// at the remote branch first when missing; with `create`, `name` is first created at HEAD.
    /// Local changes in the way refuse the switch as in `switch_branch`.
    pub fn checkout(&self, name: &str, create: bool) -> Result<SwitchOutcome, GitError> {
        let local = if create {
            self.create_branch(name, None)?;
            name.to_owned()
        } else if self
            .repo
            .try_find_reference(format!("refs/remotes/{name}").as_str())
            .map_err(git)?
            .is_some()
        {
            let local = name
                .split_once('/')
                .map(|(_, rest)| rest.to_owned())
                .unwrap_or_else(|| name.to_owned());
            let exists = self
                .repo
                .try_find_reference(format!("refs/heads/{local}").as_str())
                .map_err(git)?
                .is_some();
            if !exists {
                self.create_branch(&local, Some(name))?;
            }
            local
        } else {
            name.to_owned()
        };
        self.switch_branch(&local, false)
    }

    /// Content of `path` (relative to the repository root) at HEAD, if it exists there.
    fn head_blob(&self, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let Ok(tree) = self.repo.head_tree() else {
            return Ok(None);
        };
        let Some(entry) = tree.lookup_entry_by_path(path).map_err(git)? else {
            return Ok(None);
        };
        Ok(Some(entry.object().map_err(git)?.data.clone()))
    }

    /// Files with an unresolved merge conflict and their three sides (`SPEC-0101` behaviour 1).
    pub fn conflicts(&self) -> Result<Vec<Conflict>, GitError> {
        let index = self.repo.open_index().map_err(git)?;
        let mut by_path: BTreeMap<String, Conflict> = BTreeMap::new();
        for entry in index.entries() {
            let stage = entry.stage_raw();
            if stage == 0 {
                continue;
            }
            let path = entry.path(&index).to_string();
            // Text only when it can be written back unchanged: a regular file of valid UTF-8
            // without a NUL byte. Anything else is kept out of the text merge (v6 behaviour 12).
            let regular = entry.mode == gix::index::entry::Mode::FILE
                || entry.mode == gix::index::entry::Mode::FILE_EXECUTABLE;
            let text = self
                .repo
                .find_object(entry.id)
                .ok()
                .filter(|_| regular)
                .map(|o| o.data.clone())
                .filter(|b| !b.contains(&0))
                .and_then(|b| String::from_utf8(b).ok());
            let c = by_path.entry(path.clone()).or_insert(Conflict {
                path,
                base: None,
                ours: None,
                theirs: None,
                has_base: false,
                has_ours: false,
                has_theirs: false,
                binary: false,
            });
            c.binary |= text.is_none();
            match stage {
                1 => (c.base, c.has_base) = (text, true),
                2 => (c.ours, c.has_ours) = (text, true),
                _ => (c.theirs, c.has_theirs) = (text, true),
            }
        }
        Ok(by_path.into_values().collect())
    }

    /// Writes the resolved `text` of `path` and marks the conflict resolved, as `git add` does
    /// (`SPEC-0101` behaviour 4).
    pub fn resolve_conflict(&self, path: &str, text: &str) -> Result<(), GitError> {
        let workdir = self
            .workdir()
            .ok_or_else(|| GitError::Git("a bare repository has no working tree".into()))?;
        if !self.conflicts()?.iter().any(|c| c.path == path) {
            return Err(GitError::Rejected(format!("{path} has no merge conflict")));
        }
        std::fs::write(workdir.join(path), text).map_err(git)?;
        let out = std::process::Command::new("git")
            .args(["add", "--", path])
            .current_dir(workdir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(git)?;
        if !out.status.success() {
            return Err(GitError::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ));
        }
        Ok(())
    }

    /// Text of `path` (relative to the repository root) at HEAD; `None` when the file is not in HEAD
    /// or binary (base of the gutter markers, `SPEC-0064`).
    pub fn head_text(&self, path: &str) -> Result<Option<String>, GitError> {
        Ok(self
            .head_blob(path)?
            .filter(|b| !b.contains(&0))
            .map(|b| String::from_utf8_lossy(&b).into_owned()))
    }

    /// Text of `path` (relative to the repository root) in the commit `rev` names (a branch, tag
    /// or id); `None` when the file is not there or binary (`SPEC-0084` v2 behaviour 11).
    pub fn text_at(&self, rev: &str, path: &str) -> Result<Option<String>, GitError> {
        let commit = self.resolve_commit(rev)?;
        let tree = commit.tree().map_err(git)?;
        let Some(entry) = tree.lookup_entry_by_path(path).map_err(git)? else {
            return Ok(None);
        };
        let data = entry.object().map_err(git)?.data.clone();
        Ok((!data.contains(&0)).then(|| String::from_utf8_lossy(&data).into_owned()))
    }

    /// `path` (relative to the repository root) at HEAD and in the working tree as side-by-side rows
    /// (`SPEC-0084`); empty for binary files.
    pub fn side_by_side_file(&self, path: &str) -> Result<Vec<crate::DiffRow>, GitError> {
        let old = self.head_blob(path)?.unwrap_or_default();
        let new = self
            .workdir()
            .map(|w| w.join(path))
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        if old.contains(&0) || new.contains(&0) {
            return Ok(Vec::new());
        }
        Ok(crate::side_by_side(
            &String::from_utf8_lossy(&old),
            &String::from_utf8_lossy(&new),
        ))
    }

    /// Changes of `path` (relative to the repository root) at HEAD → working tree (behaviour 4).
    pub fn diff_file(&self, path: &str) -> Result<FileDiff, GitError> {
        let old = self.head_blob(path)?.unwrap_or_default();
        let new = self
            .workdir()
            .map(|w| w.join(path))
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        let binary = old.contains(&0) || new.contains(&0);
        let (changes, hunks) = if binary {
            (Vec::new(), Vec::new())
        } else {
            let (o, n) = (String::from_utf8_lossy(&old), String::from_utf8_lossy(&new));
            (line_changes(&o, &n), unified_hunks(&o, &n))
        };
        Ok(FileDiff {
            path: path.to_owned(),
            binary,
            changes,
            hunks,
        })
    }
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), GitError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).map_err(git)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(path, perms).map_err(git)
}

#[cfg(not(unix))]
fn set_executable(_: &Path) -> Result<(), GitError> {
    Ok(())
}

#[cfg(unix)]
fn write_symlink(target: &str, link: &Path) -> Result<(), GitError> {
    std::os::unix::fs::symlink(target, link).map_err(git)
}

#[cfg(not(unix))]
fn write_symlink(target: &str, link: &Path) -> Result<(), GitError> {
    std::fs::write(link, target).map_err(git)
}

/// Gives the in-memory configuration a fallback identity when none is configured, the way git
/// derives one from the user and host names. Without it gix refuses to write reflogs, so
/// creating or checking out a branch fails in a repository without `user.name`/`user.email`
/// (SPEC-0068 v2). Nothing is written to any config file.
fn fallback_identity(repo: &mut gix::Repository) {
    if repo.committer().is_some() && repo.author().is_some() {
        return;
    }
    use gix::config::tree::gitoxide;
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "idecraft".to_owned());
    let email = format!("{user}@{}", host_name());
    let mut config = repo.config_snapshot_mut();
    for (key, value) in [
        (&gitoxide::Committer::NAME_FALLBACK, user.as_str()),
        (&gitoxide::Committer::EMAIL_FALLBACK, email.as_str()),
        (&gitoxide::Author::NAME_FALLBACK, user.as_str()),
        (&gitoxide::Author::EMAIL_FALLBACK, email.as_str()),
    ] {
        let _ = config.set_value(key, value);
    }
    let _ = config.commit();
}

/// The machine's host name as git uses it for its fallback email, read once; `localhost` when unknown.
fn host_name() -> &'static str {
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        std::env::var("HOSTNAME")
            .ok()
            .or_else(|| {
                let out = std::process::Command::new("hostname").output().ok()?;
                Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
            })
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "localhost".to_owned())
    })
}

/// The local history the IDE keeps below `.idecraft/history` is its own state, in any folder of
/// the repository: never offered for committing (`SPEC-0032` v2). A file someone added to Git on
/// purpose stays listed.
fn is_untracked_ide_state(s: &FileStatus) -> bool {
    s.staged.is_none()
        && s.unstaged == Some(ChangeKind::Added)
        && (s.path.starts_with(".idecraft/history/") || s.path.contains("/.idecraft/history/"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_local_history_is_not_offered_for_committing() {
        let added = |p: &str| FileStatus {
            path: p.into(),
            staged: None,
            unstaged: Some(ChangeKind::Added),
        };
        assert!(is_untracked_ide_state(&added(".idecraft/history/ab/1")));
        assert!(is_untracked_ide_state(&added("sub/.idecraft/history/1")));
        assert!(!is_untracked_ide_state(&added(".idecraft/run.toml")));
        assert!(!is_untracked_ide_state(&added("my.idecraft/history/1")));
        let tracked = FileStatus {
            unstaged: Some(ChangeKind::Modified),
            ..added(".idecraft/history/ab/1")
        };
        assert!(!is_untracked_ide_state(&tracked));
    }

    use super::*;

    /// SPEC-0335 AK-5 (behaviour 4): a rename in the index is reported with the path the file
    /// had in HEAD; a rename outside the index is a deletion and an untracked file.
    #[test]
    fn status_names_the_old_path_of_a_staged_rename() {
        let dir = std::env::temp_dir().join(format!("idecraft-renames-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        let body: String = (0..40).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("src/old.rs"), &body).unwrap();
        std::fs::write(dir.join("plain.rs"), body.replace("line", "row")).unwrap();
        std::fs::write(dir.join("stays.rs"), "stays\n").unwrap();
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=a",
            "-c",
            "user.email=a@b.c",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "base",
        ]);
        git(&["mv", "src/old.rs", "src/new.rs"]);
        std::fs::rename(dir.join("plain.rs"), dir.join("moved.rs")).unwrap();
        let repo = Repo::discover(&dir).unwrap();
        let (status, renames) = repo.status_with_renames().unwrap();
        assert_eq!(
            renames,
            [Rename {
                from: "src/old.rs".into(),
                to: "src/new.rs".into()
            }]
        );
        let of = |p: &str| status.iter().find(|s| s.path == p).cloned();
        assert_eq!(
            of("src/new.rs").and_then(|s| s.staged),
            Some(ChangeKind::Renamed)
        );
        assert_eq!(of("src/old.rs"), None);
        assert_eq!(
            of("plain.rs").and_then(|s| s.unstaged),
            Some(ChangeKind::Deleted)
        );
        assert_eq!(
            of("moved.rs").and_then(|s| s.unstaged),
            Some(ChangeKind::Added)
        );
        assert_eq!(repo.status().unwrap(), status);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn branches_work_without_a_configured_identity() {
        let dir = std::env::temp_dir().join(format!("idecraft-identity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init", "-q", "-b", "main"]);
        git(&[
            "-c",
            "user.name=a",
            "-c",
            "user.email=a@b.c",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ]);
        // Isolated: no global, system or environment identity, as on a fresh machine.
        let mut repo = gix::open_opts(&dir, gix::open::Options::isolated()).unwrap();
        assert!(repo.committer().is_none());
        fallback_identity(&mut repo);
        assert!(repo.committer().is_some() && repo.author().is_some());
        let repo = Repo { repo };
        assert_eq!(repo.create_branch("feature", None).unwrap().name, "feature");
        assert_eq!(repo.checkout("test", true).unwrap().branch, "test");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
