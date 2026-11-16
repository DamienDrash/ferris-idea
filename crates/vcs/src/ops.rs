//! Stash, cherry-pick and rebase (`SPEC-0106`), run through the `git` executable like the network
//! operations, so hooks, rerere and the user's Git configuration apply as on the command line.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::git::{GitError, Repo, SwitchOutcome};
use crate::remote::run_git;

/// One stash entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stash {
    /// Position in the stash list, 0 is the newest.
    pub index: usize,
    /// The stash's message as Git shows it, e.g. `WIP on main: 1a2b3c4 subject`.
    pub message: String,
}

/// An operation that stopped and waits to be continued or aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    /// A rebase.
    Rebase,
    /// A cherry-pick.
    CherryPick,
    /// A merge.
    Merge,
}

/// One changed file of a stash: its text before and in the stash (`SPEC-0145`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashFile {
    /// Path relative to the repository root.
    pub path: String,
    /// Text at the commit the stash was made on (empty for a new file).
    pub old: String,
    /// Text in the stash (empty for a deleted file).
    pub new: String,
}

/// What interactive rebase does with one commit (`SPEC-0145`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoAction {
    /// Keep the commit.
    Pick,
    /// Keep it with a new message.
    Reword,
    /// Meld it into the previous commit, keeping both messages.
    Squash,
    /// Meld it into the previous commit, dropping its message.
    Fixup,
    /// Leave it out.
    Drop,
}

impl TodoAction {
    /// The word in Git's todo list.
    pub fn word(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Reword => "reword",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
        }
    }
}

/// One line of an interactive rebase (`SPEC-0145`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoEntry {
    /// What happens to the commit.
    pub action: TodoAction,
    /// Full commit id.
    pub id: String,
    /// Abbreviated id.
    #[serde(default)]
    pub short_id: String,
    /// First line of the message.
    #[serde(default)]
    pub subject: String,
    /// The new message for `reword`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// What a stash, cherry-pick or rebase did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpOutcome {
    /// Whether Git finished without stopping.
    pub ok: bool,
    /// Files left with conflicts.
    pub conflicts: Vec<String>,
    /// The operation that now waits to be continued or aborted, if any.
    pub in_progress: Option<Operation>,
    /// Git's own message (standard error and output).
    pub output: String,
}

/// Local changes a smart checkout or merge put aside (`SPEC-0231`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SmartStash {
    /// Commit id of the stash.
    pub id: String,
    /// Its message.
    pub message: String,
    /// Whether it came back cleanly (and was dropped).
    pub restored: bool,
    /// Files left with conflicts when it came back; it is kept then.
    pub conflicts: Vec<String>,
}

/// An operation run with local changes put aside around it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SmartOutcome<T> {
    /// What the operation did.
    pub result: T,
    /// The stash, if changes were put aside; `restored` false while it waits.
    pub stash: Option<SmartStash>,
}

/// State shown in the branch widget (`SPEC-0068` v3 behaviour 10).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BranchState {
    /// Checked-out branch; `None` on a detached HEAD.
    pub branch: Option<String>,
    /// Commits ahead of the upstream as of the last fetch.
    pub ahead: usize,
    /// Commits behind it.
    pub behind: usize,
    /// The operation that waits on conflicts.
    pub operation: Option<Operation>,
    /// The branch a waiting rebase replays.
    pub rebasing: Option<String>,
}

/// Time limit of local operations; they never touch the network.
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(60);

/// Git's refusal of a merge or checkout in one line, the files listed after a colon.
fn local_changes_message(output: &str) -> String {
    let files: Vec<&str> = output
        .lines()
        .filter(|l| l.starts_with('\t'))
        .map(str::trim)
        .collect();
    if files.is_empty() {
        output
            .lines()
            .next()
            .unwrap_or("git refused the merge")
            .to_owned()
    } else {
        format!("local changes would be overwritten: {}", files.join(", "))
    }
}

impl Repo {
    pub(crate) fn dir(&self) -> Result<&Path, GitError> {
        self.workdir()
            .ok_or_else(|| GitError::Rejected("the repository has no working tree".into()))
    }

    pub(crate) fn git_run(&self, args: &[&str]) -> Result<OpOutcome, GitError> {
        let dir = self.dir()?;
        let mut all = vec!["-c", "core.editor=true"];
        all.extend_from_slice(args);
        let (ok, out, err, timed_out) = run_git(dir, &all, LOCAL_TIMEOUT)?;
        if timed_out {
            return Err(GitError::Git(format!("git {} timed out", args.join(" "))));
        }
        let mut output = err.trim().to_owned();
        if !out.trim().is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(out.trim());
        }
        Ok(OpOutcome {
            ok,
            conflicts: self.unmerged_paths()?,
            in_progress: self.operation_in_progress(),
            output,
        })
    }

    fn unmerged_paths(&self) -> Result<Vec<String>, GitError> {
        let (_, out, _, _) = run_git(
            self.dir()?,
            &["diff", "--name-only", "--diff-filter=U"],
            LOCAL_TIMEOUT,
        )?;
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// The rebase, cherry-pick or merge that stopped and waits, read from the Git folder.
    pub fn operation_in_progress(&self) -> Option<Operation> {
        let git = self.git_dir();
        if git.join("rebase-merge").is_dir() || git.join("rebase-apply").is_dir() {
            Some(Operation::Rebase)
        } else if git.join("CHERRY_PICK_HEAD").is_file() {
            Some(Operation::CherryPick)
        } else if git.join("MERGE_HEAD").is_file() {
            Some(Operation::Merge)
        } else {
            None
        }
    }

    /// The branch a stopped rebase replays (`SPEC-0101` v2 behaviour 2), without `refs/heads/`;
    /// `None` without a rebase or when it started on a detached HEAD.
    pub fn rebasing_branch(&self) -> Option<String> {
        let git = self.git_dir();
        ["rebase-merge", "rebase-apply"].iter().find_map(|d| {
            let name = std::fs::read_to_string(git.join(d).join("head-name")).ok()?;
            let name = name.trim();
            name.strip_prefix("refs/heads/").map(str::to_owned)
        })
    }

    /// The stash list, newest first (behaviour 1).
    pub fn stashes(&self) -> Result<Vec<Stash>, GitError> {
        let (ok, out, err, _) = run_git(
            self.dir()?,
            &["stash", "list", "--format=%gs"],
            LOCAL_TIMEOUT,
        )?;
        if !ok {
            return Err(GitError::Git(err.trim().to_owned()));
        }
        Ok(out
            .lines()
            .enumerate()
            .map(|(index, message)| Stash {
                index,
                message: message.to_owned(),
            })
            .collect())
    }

    /// Stashes the local changes, with untracked files when asked (behaviour 2).
    pub fn stash_push(
        &self,
        message: Option<&str>,
        include_untracked: bool,
    ) -> Result<OpOutcome, GitError> {
        let mut args = vec!["stash", "push"];
        if include_untracked {
            args.push("--include-untracked");
        }
        if let Some(m) = message.filter(|m| !m.trim().is_empty()) {
            args.extend(["--message", m]);
        }
        self.git_run(&args)
    }

    /// Applies stash `index`; with `pop` it is dropped when it applied without conflicts
    /// (behaviour 3).
    pub fn stash_apply(&self, index: usize, pop: bool) -> Result<OpOutcome, GitError> {
        let name = format!("stash@{{{index}}}");
        self.stash_exists(index)?;
        self.git_run(&["stash", if pop { "pop" } else { "apply" }, &name])
    }

    /// Deletes stash `index` (behaviour 3).
    pub fn stash_drop(&self, index: usize) -> Result<OpOutcome, GitError> {
        self.stash_exists(index)?;
        self.git_run(&["stash", "drop", &format!("stash@{{{index}}}")])
    }

    fn stash_exists(&self, index: usize) -> Result<(), GitError> {
        if index < self.stashes()?.len() {
            Ok(())
        } else {
            Err(GitError::Rejected(format!("there is no stash {index}")))
        }
    }

    pub(crate) fn refuse_while_in_progress(&self) -> Result<(), GitError> {
        match self.operation_in_progress() {
            Some(op) => Err(GitError::Rejected(format!(
                "a {} is in progress: continue or abort it first",
                match op {
                    Operation::Rebase => "rebase",
                    Operation::CherryPick => "cherry-pick",
                    Operation::Merge => "merge",
                }
            ))),
            None => Ok(()),
        }
    }

    /// Applies the change of commit `rev` onto the current branch as a new commit (behaviour 4).
    pub fn cherry_pick(&self, rev: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        self.git_run(&["cherry-pick", rev])
    }

    /// Replays the current branch's own commits onto `onto` (behaviour 5).
    pub fn rebase(&self, onto: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        self.git_run(&["rebase", onto])
    }

    /// Like [`Repo::rebase`], but local changes are stashed before and restored after
    /// (`--autostash`), as Update Project with rebase needs (`SPEC-0144`).
    pub fn rebase_autostash(&self, onto: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        self.git_run(&["rebase", "--autostash", onto])
    }

    /// Subjects of the commits `rebase(onto)` would replay, oldest first (its preview).
    pub fn rebase_preview(&self, onto: &str) -> Result<Vec<String>, GitError> {
        let range = format!("{onto}..HEAD");
        let (ok, out, err, _) = run_git(
            self.dir()?,
            &["log", "--reverse", "--format=%h %s", &range],
            LOCAL_TIMEOUT,
        )?;
        if !ok {
            return Err(GitError::Rejected(err.trim().to_owned()));
        }
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// The files stash `index` changes, with their text before and in the stash, untracked files
    /// included (`SPEC-0145`).
    pub fn stash_files(&self, index: usize) -> Result<Vec<StashFile>, GitError> {
        self.stash_exists(index)?;
        let dir = self.dir()?;
        let name = format!("stash@{{{index}}}");
        let show = |args: &[&str]| -> Result<Option<String>, GitError> {
            let (ok, out, _, _) = run_git(dir, args, LOCAL_TIMEOUT)?;
            Ok(ok.then_some(out))
        };
        let tracked = show(&["stash", "show", "--name-only", "-z", &name])?.unwrap_or_default();
        let untracked_rev = format!("{name}^3");
        let untracked =
            show(&["ls-tree", "-r", "-z", "--name-only", &untracked_rev])?.unwrap_or_default();
        let blob = |rev: &str, path: &str| -> Result<String, GitError> {
            Ok(show(&["show", &format!("{rev}:{path}")])?.unwrap_or_default())
        };
        let mut files = Vec::new();
        for path in tracked.split('\0').filter(|p| !p.is_empty()) {
            files.push(StashFile {
                path: path.to_owned(),
                old: blob(&format!("{name}^1"), path)?,
                new: blob(&name, path)?,
            });
        }
        for path in untracked.split('\0').filter(|p| !p.is_empty()) {
            if !files.iter().any(|f| f.path == path) {
                files.push(StashFile {
                    path: path.to_owned(),
                    old: String::new(),
                    new: blob(&untracked_rev, path)?,
                });
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// The commits an interactive rebase onto `onto` lists, oldest first, all `pick`
    /// (`SPEC-0145`). Refused when the range holds merge commits.
    pub fn rebase_todo(&self, onto: &str) -> Result<Vec<TodoEntry>, GitError> {
        if onto.starts_with('-') || onto.trim().is_empty() {
            return Err(GitError::Rejected(format!("{onto} is not a revision")));
        }
        let dir = self.dir()?;
        let range = format!("{onto}..HEAD");
        let (ok, merges, err, _) =
            run_git(dir, &["rev-list", "--merges", &range, "--"], LOCAL_TIMEOUT)?;
        if !ok {
            return Err(GitError::Rejected(err.trim().to_owned()));
        }
        if !merges.trim().is_empty() {
            return Err(GitError::Rejected(
                "the commits contain merges; interactive rebase keeps only linear history".into(),
            ));
        }
        let (_, out, _, _) = run_git(
            dir,
            &["log", "--reverse", "--format=%H%x00%h%x00%s", &range, "--"],
            LOCAL_TIMEOUT,
        )?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.splitn(3, '\0').collect();
                (f.len() == 3).then(|| TodoEntry {
                    action: TodoAction::Pick,
                    id: f[0].to_owned(),
                    short_id: f[1].to_owned(),
                    subject: f[2].to_owned(),
                    message: None,
                })
            })
            .collect())
    }

    /// Runs an interactive rebase onto `onto` with `entries` in their order (`SPEC-0145`):
    /// they must be exactly the commits of [`Repo::rebase_todo`], the first kept one may not be
    /// squash or fixup, and reword needs a message. Local changes are stashed around it; a stop
    /// on conflicts waits for Continue or Abort like any rebase.
    pub fn rebase_interactive(
        &self,
        onto: &str,
        entries: &[TodoEntry],
    ) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        let listed = self.rebase_todo(onto)?;
        let mut want: Vec<&str> = listed.iter().map(|e| e.id.as_str()).collect();
        let mut got: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        want.sort_unstable();
        got.sort_unstable();
        if want != got {
            return Err(GitError::Rejected(
                "the list must hold each commit since the base exactly once".into(),
            ));
        }
        if let Some(first) = entries.iter().find(|e| e.action != TodoAction::Drop)
            && matches!(first.action, TodoAction::Squash | TodoAction::Fixup)
        {
            return Err(GitError::Rejected(format!(
                "{} has no earlier commit to {} into",
                first.short_id,
                first.action.word()
            )));
        }
        let (_, git_dir, _, _) = run_git(
            self.dir()?,
            &["rev-parse", "--absolute-git-dir"],
            LOCAL_TIMEOUT,
        )?;
        let work = Path::new(git_dir.trim()).join("idecraft-rebase");
        let text = work.to_string_lossy();
        if text.contains(['\'', '\n']) {
            return Err(GitError::Rejected(
                "the repository path contains a quote or line break".into(),
            ));
        }
        let io = |e: std::io::Error| GitError::Git(e.to_string());
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(io)?;
        let mut todo = String::new();
        for (i, e) in entries.iter().enumerate() {
            if !e.id.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(GitError::Rejected(format!("{} is not a commit id", e.id)));
            }
            // Reword runs as a pick plus an amend with the message from a file, so no editor
            // has to open.
            let (word, message) = match e.action {
                TodoAction::Reword => {
                    let m = e.message.as_deref().map(str::trim).unwrap_or_default();
                    if m.is_empty() {
                        return Err(GitError::Rejected(format!(
                            "reword of {} needs a message",
                            e.short_id
                        )));
                    }
                    ("pick", Some(m))
                }
                other => (other.word(), None),
            };
            todo.push_str(&format!("{word} {}\n", e.id));
            if let Some(m) = message {
                let file = work.join(format!("message-{i}"));
                std::fs::write(&file, format!("{m}\n")).map_err(io)?;
                todo.push_str(&format!(
                    "exec git commit --amend --only --allow-empty --no-verify -F '{}'\n",
                    file.display()
                ));
            }
        }
        let todo_file = work.join("todo");
        std::fs::write(&todo_file, todo).map_err(io)?;
        let editor = format!("sequence.editor=cp '{}'", todo_file.display());
        let done = self.git_run(&[
            "-c",
            &editor,
            "rebase",
            "--interactive",
            "--autostash",
            onto,
        ])?;
        if done.in_progress.is_none() {
            let _ = std::fs::remove_dir_all(&work);
        }
        Ok(done)
    }

    /// Continues the stopped operation once its conflicts are resolved and staged (behaviour 6);
    /// when it ends, a smart checkout's stash comes back (`SPEC-0231` behaviour 4).
    pub fn continue_operation(&self) -> Result<OpOutcome, GitError> {
        let done = match self.operation_in_progress() {
            Some(Operation::Rebase) => self.git_run(&["rebase", "--continue"]),
            Some(Operation::CherryPick) => self.git_run(&["cherry-pick", "--continue"]),
            Some(Operation::Merge) => self.git_run(&["commit", "--no-edit"]),
            None => Err(GitError::Rejected("no operation is in progress".into())),
        }?;
        self.after_operation(done)
    }

    /// Aborts the stopped operation and restores the state before it (behaviour 6).
    pub fn abort_operation(&self) -> Result<OpOutcome, GitError> {
        let done = match self.operation_in_progress() {
            Some(Operation::Rebase) => self.git_run(&["rebase", "--abort"]),
            Some(Operation::CherryPick) => self.git_run(&["cherry-pick", "--abort"]),
            Some(Operation::Merge) => self.git_run(&["merge", "--abort"]),
            None => Err(GitError::Rejected("no operation is in progress".into())),
        }?;
        self.after_operation(done)
    }

    /// Drops the commit a rebase or cherry-pick stopped on and goes on (`SPEC-0106` v2
    /// behaviour 12).
    pub fn skip_operation(&self) -> Result<OpOutcome, GitError> {
        let done = match self.operation_in_progress() {
            Some(Operation::Rebase) => self.git_run(&["rebase", "--skip"]),
            Some(Operation::CherryPick) => self.git_run(&["cherry-pick", "--skip"]),
            Some(Operation::Merge) => Err(GitError::Rejected("a merge cannot be skipped".into())),
            None => Err(GitError::Rejected("no operation is in progress".into())),
        }?;
        self.after_operation(done)
    }

    /// Merges `source` into the current branch with the `git` program; conflicts leave the
    /// repository merging for the merge view (`SPEC-0036` v2 behaviour 7).
    pub fn merge_stopping(&self, source: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        let done = self.git_run(&["merge", "--no-edit", source])?;
        if !done.ok && done.conflicts.is_empty() && done.in_progress.is_none() {
            // Git refused before merging, e.g. because local changes would be overwritten.
            return Err(GitError::Rejected(local_changes_message(&done.output)));
        }
        Ok(done)
    }

    /// Where the id of a smart checkout's stash is kept until it is brought back (`SPEC-0231`
    /// behaviour 2).
    fn smart_stash_file(&self) -> std::path::PathBuf {
        self.git_dir().join("idecraft").join("smart-stash")
    }

    /// The stash a smart checkout or merge put aside and has not brought back yet.
    pub fn pending_smart_stash(&self) -> Option<String> {
        std::fs::read_to_string(self.smart_stash_file())
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    /// Puts all local changes aside in a stash named `message` and records it; `None` when there
    /// was nothing to stash (`SPEC-0231` behaviour 2).
    pub fn smart_stash(&self, message: &str) -> Result<Option<SmartStash>, GitError> {
        if let Some(id) = self.pending_smart_stash() {
            return Err(GitError::Rejected(format!(
                "changes of an earlier smart checkout ({id}) are still put aside"
            )));
        }
        let before = self.stash_ids()?;
        let done = self.stash_push(Some(message), true)?;
        if !done.ok {
            return Err(GitError::Git(done.output));
        }
        let after = self.stash_ids()?;
        match after.first() {
            Some(id) if before.first() != Some(id) => {
                let file = self.smart_stash_file();
                if let Some(dir) = file.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| GitError::Git(e.to_string()))?;
                }
                std::fs::write(&file, id).map_err(|e| GitError::Git(e.to_string()))?;
                Ok(Some(SmartStash {
                    id: id.clone(),
                    message: message.to_owned(),
                    restored: false,
                    conflicts: Vec::new(),
                }))
            }
            _ => Ok(None),
        }
    }

    /// Commit ids of the stashes, newest first.
    fn stash_ids(&self) -> Result<Vec<String>, GitError> {
        let (ok, out, err, _) = run_git(
            self.dir()?,
            &["stash", "list", "--format=%H"],
            LOCAL_TIMEOUT,
        )?;
        if !ok {
            return Err(GitError::Git(err.trim().to_owned()));
        }
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// Brings back the recorded smart stash by its id: applied, then dropped when it applied
    /// cleanly, else kept in the list (`SPEC-0231` behaviour 3). `None` without one.
    pub fn restore_smart_stash(&self) -> Result<Option<SmartStash>, GitError> {
        let Some(id) = self.pending_smart_stash() else {
            return Ok(None);
        };
        let _ = std::fs::remove_file(self.smart_stash_file());
        let ids = self.stash_ids()?;
        let Some(index) = ids.iter().position(|i| *i == id) else {
            return Err(GitError::Rejected(format!(
                "the stash {id} of the smart checkout is no longer in the stash list"
            )));
        };
        let message = self
            .stashes()?
            .get(index)
            .map(|s| s.message.clone())
            .unwrap_or_default();
        let done = self.git_run(&["stash", "apply", "--index", &id]);
        // `--index` fails when the staged state cannot be restored; the changes still come back.
        let done = match done {
            Ok(d) if !d.ok && d.conflicts.is_empty() => self.git_run(&["stash", "apply", &id])?,
            other => other?,
        };
        let restored = done.ok && done.conflicts.is_empty();
        if restored {
            self.git_run(&["stash", "drop", &format!("stash@{{{index}}}")])?;
        } else if done.conflicts.is_empty() {
            return Err(GitError::Git(done.output));
        }
        Ok(Some(SmartStash {
            id,
            message,
            restored,
            conflicts: done.conflicts,
        }))
    }

    /// After an operation: once nothing waits any more, a pending smart stash comes back and its
    /// conflicts and message join the outcome (`SPEC-0231` behaviour 4).
    fn after_operation(&self, mut done: OpOutcome) -> Result<OpOutcome, GitError> {
        if done.in_progress.is_none()
            && done.conflicts.is_empty()
            && let Some(stash) = self.restore_smart_stash()?
        {
            if stash.restored {
                done.output
                    .push_str("\nlocal changes restored from the smart checkout stash");
            } else {
                done.output.push_str(&format!(
                    "\nlocal changes kept in the stash \"{}\": they conflict",
                    stash.message
                ));
                done.conflicts = stash.conflicts;
            }
        }
        Ok(done)
    }

    /// Checkout with local changes put aside around it (`SPEC-0231`).
    pub fn smart_checkout(
        &self,
        name: &str,
        create: bool,
    ) -> Result<SmartOutcome<SwitchOutcome>, GitError> {
        let from = self.current_branch_name().unwrap_or_else(|| "HEAD".into());
        let stash = self.smart_stash(&format!("IDECraft smart checkout: {from} → {name}"))?;
        match self.checkout(name, create) {
            Ok(result) => Ok(SmartOutcome {
                result,
                stash: match stash {
                    Some(_) => self.restore_smart_stash()?,
                    None => None,
                },
            }),
            Err(e) => {
                if stash.is_some() {
                    self.restore_smart_stash()?;
                }
                Err(e)
            }
        }
    }

    /// Merge that may stop, with local changes put aside around it; they stay aside while the
    /// merge waits on conflicts (`SPEC-0231` behaviour 4).
    pub fn smart_merge(&self, source: &str) -> Result<SmartOutcome<OpOutcome>, GitError> {
        self.refuse_while_in_progress()?;
        let into = self.current_branch_name().unwrap_or_else(|| "HEAD".into());
        let stash = self.smart_stash(&format!("IDECraft smart merge: {source} → {into}"))?;
        match self.git_run(&["merge", "--no-edit", source]) {
            Ok(done) if done.in_progress.is_some() || !done.conflicts.is_empty() => {
                Ok(SmartOutcome {
                    result: done,
                    stash,
                })
            }
            Ok(done) if !done.ok => {
                if stash.is_some() {
                    self.restore_smart_stash()?;
                }
                Err(GitError::Git(done.output))
            }
            Ok(done) => {
                let stash = match stash {
                    Some(_) => self.restore_smart_stash()?,
                    None => None,
                };
                Ok(SmartOutcome {
                    result: done,
                    stash,
                })
            }
            Err(e) => {
                if stash.is_some() {
                    self.restore_smart_stash()?;
                }
                Err(e)
            }
        }
    }

    /// Name of the checked-out branch, `None` on a detached HEAD.
    pub(crate) fn current_branch_name(&self) -> Option<String> {
        let head = std::fs::read_to_string(self.git_dir().join("HEAD")).ok()?;
        head.trim()
            .strip_prefix("ref: refs/heads/")
            .map(str::to_owned)
    }

    /// What the branch widget shows (`SPEC-0068` v3 behaviour 10): branch, ahead/behind its
    /// upstream as of the last fetch, and the operation that waits.
    pub fn branch_state(&self) -> BranchState {
        let branch = self.current_branch_name();
        let (ahead, behind) = branch
            .as_ref()
            .and_then(|b| {
                let (ok, out, _, _) = run_git(
                    self.dir().ok()?,
                    &[
                        "rev-list",
                        "--left-right",
                        "--count",
                        &format!("refs/heads/{b}...{b}@{{upstream}}"),
                    ],
                    LOCAL_TIMEOUT,
                )
                .ok()?;
                if !ok {
                    return None;
                }
                let mut n = out
                    .split_whitespace()
                    .map(|n| n.parse::<usize>().unwrap_or(0));
                Some((n.next()?, n.next()?))
            })
            .unwrap_or((0, 0));
        let operation = self.operation_in_progress();
        let rebasing = (operation == Some(Operation::Rebase))
            .then(|| self.rebasing_branch())
            .flatten();
        BranchState {
            branch,
            ahead,
            behind,
            operation,
            rebasing,
        }
    }
}
