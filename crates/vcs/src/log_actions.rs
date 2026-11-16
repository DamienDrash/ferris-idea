//! Actions on one commit of the Git log (`SPEC-0086` v7 behaviour 13): which entries the
//! commit's context menu offers, whether each can be chosen now and why not, and the Git
//! operations behind them. The window and the MCP tool `git_commit_action` share these rules.

use serde::Serialize;

use crate::git::{GitError, Repo};
use crate::ops::{OpOutcome, TodoAction};
use crate::remote::{run_git, valid_branch_name};

/// An entry of the commit context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommitAction {
    /// Copy the full commit id.
    CopyRevision,
    /// Copy the commit as a patch (`git format-patch`).
    CopyPatch,
    /// Apply the commit onto the current branch.
    CherryPick,
    /// Check the commit out on a detached HEAD.
    Checkout,
    /// List the files at the commit; open one read-only (v8).
    ShowRepository,
    /// The files that differ between the commit and the working tree (v8).
    CompareLocal,
    /// Show the commit's changes.
    ShowDiff,
    /// Move the current branch to the commit (soft, mixed, hard, keep).
    Reset,
    /// Add a commit that takes the commit's change back.
    Revert,
    /// Take the latest commit back, keeping its changes.
    Undo,
    /// Change the commit's message.
    EditMessage,
    /// A `fixup!` commit for the commit, from the Commit window (v8).
    Fixup,
    /// A `squash!` commit for the commit, from the Commit window (v8).
    SquashInto,
    /// Remove the commit from the current branch.
    Drop,
    /// Interactive rebase of this and the later commits.
    RebaseFromHere,
    /// Push the current branch only up to the commit (v8).
    PushUpTo,
    /// A new branch at the commit.
    NewBranch,
    /// A new tag at the commit.
    NewTag,
    /// Select the commit's child.
    GoToChild,
    /// Select the commit's first parent.
    GoToParent,
}

impl CommitAction {
    /// Every action in menu order.
    pub const ALL: [CommitAction; 20] = [
        Self::CopyRevision,
        Self::CopyPatch,
        Self::CherryPick,
        Self::Checkout,
        Self::ShowRepository,
        Self::CompareLocal,
        Self::ShowDiff,
        Self::Reset,
        Self::Revert,
        Self::Undo,
        Self::EditMessage,
        Self::Fixup,
        Self::SquashInto,
        Self::Drop,
        Self::RebaseFromHere,
        Self::PushUpTo,
        Self::NewBranch,
        Self::NewTag,
        Self::GoToChild,
        Self::GoToParent,
    ];

    /// The id agents name the action by.
    pub fn id(self) -> &'static str {
        match self {
            Self::CopyRevision => "copy-revision",
            Self::CopyPatch => "copy-patch",
            Self::CherryPick => "cherry-pick",
            Self::Checkout => "checkout",
            Self::ShowRepository => "show-repository",
            Self::CompareLocal => "compare-local",
            Self::ShowDiff => "show-diff",
            Self::Reset => "reset",
            Self::Revert => "revert",
            Self::Undo => "undo",
            Self::EditMessage => "edit-message",
            Self::Fixup => "fixup",
            Self::SquashInto => "squash",
            Self::Drop => "drop",
            Self::RebaseFromHere => "rebase-from-here",
            Self::PushUpTo => "push-up-to",
            Self::NewBranch => "new-branch",
            Self::NewTag => "new-tag",
            Self::GoToChild => "go-to-child",
            Self::GoToParent => "go-to-parent",
        }
    }

    /// The action an id names.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::CopyRevision => "Copy Revision Number",
            Self::CopyPatch => "Copy as Patch",
            Self::CherryPick => "Cherry-Pick",
            Self::Checkout => "Checkout Revision",
            Self::ShowRepository => "Show Repository at Revision",
            Self::CompareLocal => "Compare with Local",
            Self::ShowDiff => "Show Diff",
            Self::Reset => "Reset Current Branch to Here…",
            Self::Revert => "Revert Commit",
            Self::Undo => "Undo Commit",
            Self::EditMessage => "Edit Commit Message…",
            Self::Fixup => "Fixup…",
            Self::SquashInto => "Squash Into…",
            Self::Drop => "Drop Commit",
            Self::RebaseFromHere => "Interactively Rebase from Here…",
            Self::PushUpTo => "Push All up to Here…",
            Self::NewBranch => "New Branch…",
            Self::NewTag => "New Tag…",
            Self::GoToChild => "Go to Child Commit",
            Self::GoToParent => "Go to Parent Commit",
        }
    }

    /// The shortcut shown next to the label (macOS keymap), if the log window has one.
    pub fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::CopyRevision => Some("⌥⇧⌘C"),
            Self::CherryPick => Some("⌥↩"),
            Self::ShowDiff => Some("⌘D"),
            Self::RebaseFromHere => Some("⌥I"),
            Self::GoToChild => Some("←"),
            Self::GoToParent => Some("→"),
            _ => None,
        }
    }

    /// The group the entry belongs to; a line separates groups in the menu (v8: the reference's
    /// groups).
    pub fn group(self) -> u8 {
        match self {
            Self::CopyRevision | Self::CopyPatch | Self::CherryPick => 0,
            Self::Checkout | Self::ShowRepository | Self::CompareLocal | Self::ShowDiff => 1,
            Self::Reset | Self::Revert | Self::Undo => 2,
            Self::EditMessage
            | Self::Fixup
            | Self::SquashInto
            | Self::Drop
            | Self::RebaseFromHere
            | Self::PushUpTo => 3,
            Self::NewBranch | Self::NewTag => 4,
            Self::GoToChild | Self::GoToParent => 5,
        }
    }
}

/// How Reset Current Branch to Here treats the index and the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResetMode {
    /// Keep index and working tree; the undone commits' changes stay staged.
    Soft,
    /// Keep the working tree, reset the index.
    Mixed,
    /// Reset index and working tree; uncommitted changes are kept in a stash first (IMP-0320).
    Hard,
    /// Reset, but keep local changes; refused when a changed file differs between the commits.
    Keep,
}

impl ResetMode {
    /// Every mode in the order the menu lists them.
    pub const ALL: [ResetMode; 4] = [Self::Soft, Self::Mixed, Self::Hard, Self::Keep];

    /// The word of `git reset --<mode>` and of the MCP argument.
    pub fn id(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Mixed => "mixed",
            Self::Hard => "hard",
            Self::Keep => "keep",
        }
    }

    /// The mode a word names.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    /// The entry in the mode list, with what it does.
    pub fn label(self) -> &'static str {
        match self {
            Self::Soft => "Soft — keep the changes staged",
            Self::Mixed => "Mixed — keep the changes unstaged",
            Self::Hard => "Hard — discard the changes (stashed first)",
            Self::Keep => "Keep — keep local changes, refuse if they collide",
        }
    }
}

/// What decides which commit actions are available (`SPEC-0086` v7 behaviour 13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CommitFacts {
    /// HEAD points at the commit.
    pub is_head: bool,
    /// The commit is reachable from HEAD.
    pub on_current_branch: bool,
    /// A remote-tracking branch contains the commit.
    pub pushed: bool,
    /// The commit has more than one parent.
    pub merge: bool,
    /// The commit has no parent.
    pub root: bool,
    /// Between the commit and HEAD there is a merge commit.
    pub merges_after: bool,
    /// HEAD is detached.
    pub detached: bool,
    /// A listed commit has the commit as a parent.
    pub has_child: bool,
}

/// One entry of the menu with its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionState {
    /// The action.
    pub action: CommitAction,
    /// Its id (as `action` serialises).
    pub id: &'static str,
    /// Menu label.
    pub label: &'static str,
    /// Shortcut shown next to it.
    pub shortcut: Option<&'static str>,
    /// Whether it can be chosen now.
    pub enabled: bool,
    /// Why not, when it cannot (IMP-0278: a disabled entry names its reason).
    pub reason: Option<String>,
}

/// Why `action` is not available for a commit with `facts`; `None` when it is.
pub fn unavailable(action: CommitAction, f: &CommitFacts) -> Option<&'static str> {
    const NOT_ON_BRANCH: &str = "the commit is not on the current branch";
    const PUSHED: &str =
        "the commit is pushed already; rewriting it would change published history";
    const MERGES_AFTER: &str =
        "merge commits follow it on the branch; interactive rebase keeps only linear history";
    const MERGE: &str = "it is a merge commit";
    const ROOT: &str = "the first commit has no parent";
    let rewrite = || {
        if !f.on_current_branch {
            Some(NOT_ON_BRANCH)
        } else if f.pushed {
            Some(PUSHED)
        } else if f.merge {
            Some(MERGE)
        } else if f.merges_after {
            Some(MERGES_AFTER)
        } else if f.root {
            Some(ROOT)
        } else {
            None
        }
    };
    match action {
        CommitAction::CopyRevision
        | CommitAction::CopyPatch
        | CommitAction::Checkout
        | CommitAction::ShowDiff
        | CommitAction::ShowRepository
        | CommitAction::CompareLocal
        | CommitAction::NewBranch
        | CommitAction::NewTag => None,
        CommitAction::CherryPick if f.on_current_branch => {
            Some("the commit is already on the current branch")
        }
        CommitAction::CherryPick if f.merge => Some(MERGE),
        CommitAction::CherryPick => None,
        CommitAction::Reset if f.detached => Some("HEAD is detached: there is no branch to reset"),
        CommitAction::Reset if f.is_head => {
            Some("the current branch already points at this commit")
        }
        CommitAction::Reset => None,
        CommitAction::Revert if !f.on_current_branch => Some(NOT_ON_BRANCH),
        CommitAction::Revert if f.merge => Some(MERGE),
        CommitAction::Revert => None,
        CommitAction::Undo if !f.is_head || f.detached => {
            Some("only the latest commit of the current branch can be undone")
        }
        CommitAction::Undo if f.pushed => Some("the commit is pushed already; revert it instead"),
        CommitAction::Undo if f.merge => Some(MERGE),
        CommitAction::Undo if f.root => Some(ROOT),
        CommitAction::Undo => None,
        CommitAction::EditMessage
        | CommitAction::Fixup
        | CommitAction::SquashInto
        | CommitAction::Drop
        | CommitAction::RebaseFromHere => rewrite(),
        CommitAction::PushUpTo if f.detached => {
            Some("HEAD is detached: there is no branch to push")
        }
        CommitAction::PushUpTo if !f.on_current_branch => Some(NOT_ON_BRANCH),
        CommitAction::PushUpTo if f.pushed => Some("the commit is pushed already"),
        CommitAction::PushUpTo => None,
        CommitAction::GoToChild if !f.has_child => Some("no listed commit is a child of it"),
        CommitAction::GoToChild => None,
        CommitAction::GoToParent if f.root => Some(ROOT),
        CommitAction::GoToParent => None,
    }
}

/// The entries of the commit context menu in order, with their state (`SPEC-0086` v7
/// behaviour 13).
pub fn commit_actions(facts: &CommitFacts) -> Vec<ActionState> {
    CommitAction::ALL
        .into_iter()
        .map(|action| {
            let reason = unavailable(action, facts);
            ActionState {
                action,
                id: action.id(),
                label: action.label(),
                shortcut: action.shortcut(),
                enabled: reason.is_none(),
                reason: reason.map(str::to_owned),
            }
        })
        .collect()
}

impl Repo {
    fn plain_git(&self, args: &[&str]) -> Result<(bool, String, String), GitError> {
        let (ok, out, err, timed_out) = run_git(self.dir()?, args, crate::ops::LOCAL_TIMEOUT)?;
        if timed_out {
            return Err(GitError::Git(format!("git {} timed out", args.join(" "))));
        }
        Ok((ok, out, err))
    }

    /// The full id of the commit `rev` names, refusing options and unknown revisions.
    fn commit_id(&self, rev: &str) -> Result<String, GitError> {
        if rev.starts_with('-') || rev.trim().is_empty() {
            return Err(GitError::Rejected(format!("{rev} is not a revision")));
        }
        let spec = format!("{rev}^{{commit}}");
        let (ok, out, _) = self.plain_git(&["rev-parse", "--verify", "--quiet", &spec])?;
        if !ok {
            return Err(GitError::Rejected(format!("unknown revision {rev}")));
        }
        Ok(out.trim().to_owned())
    }

    /// The facts that decide the commit menu for `rev` (`SPEC-0086` v7 behaviour 13), read with
    /// `git`; "pushed" means some remote-tracking branch contains the commit.
    pub fn commit_facts(&self, rev: &str) -> Result<CommitFacts, GitError> {
        let id = self.commit_id(rev)?;
        let head = self
            .plain_git(&["rev-parse", "--verify", "--quiet", "HEAD"])?
            .1
            .trim()
            .to_owned();
        let parents = self.commit_details(&id)?.parents.len();
        let on_current_branch = !head.is_empty()
            && self
                .plain_git(&["merge-base", "--is-ancestor", &id, "HEAD"])?
                .0;
        let (_, remote, _) = self.plain_git(&["branch", "-r", "--contains", &id])?;
        let pushed = remote
            .lines()
            .any(|l| !l.trim().is_empty() && !l.contains(" -> "));
        let merges_after = on_current_branch && {
            let range = format!("{id}..HEAD");
            let (_, out, _) = self.plain_git(&["rev-list", "--merges", &range, "--"])?;
            !out.trim().is_empty()
        };
        let (_, children, _) = self.plain_git(&["rev-list", "--all", "--children"])?;
        let has_child = children.lines().any(|l| {
            let mut words = l.split_whitespace();
            words.next() == Some(id.as_str()) && words.next().is_some()
        });
        Ok(CommitFacts {
            is_head: head == id,
            on_current_branch,
            pushed,
            merge: parents > 1,
            root: parents == 0,
            merges_after,
            detached: self.current_branch()?.is_none(),
            has_child,
        })
    }

    /// Refuses `action` on `rev` with the menu's reason when it is not available.
    pub fn check_commit_action(&self, rev: &str, action: CommitAction) -> Result<(), GitError> {
        match unavailable(action, &self.commit_facts(rev)?) {
            Some(why) => Err(GitError::Rejected(format!("{}: {why}", action.label()))),
            None => Ok(()),
        }
    }

    /// The commit `rev` as a patch in mail format (`git format-patch`), the first commit included.
    pub fn commit_patch(&self, rev: &str) -> Result<String, GitError> {
        let id = self.commit_id(rev)?;
        let (ok, out, err) = self.plain_git(&[
            "format-patch",
            "-1",
            "--root",
            "--stdout",
            "--no-color",
            &id,
        ])?;
        if !ok {
            return Err(GitError::Git(err.trim().to_owned()));
        }
        Ok(out)
    }

    /// Checks out the commit `rev` on a detached HEAD; Git refuses when local changes would be
    /// overwritten.
    pub fn checkout_revision(&self, rev: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        let id = self.commit_id(rev)?;
        self.git_run(&["checkout", "--detach", &id])
    }

    /// Moves the current branch to `rev` (`git reset --<mode>`). A hard reset first keeps the
    /// uncommitted changes of tracked files in a stash named after the reset, so nothing is lost
    /// (IMP-0320); the output names it.
    pub fn reset_to(&self, rev: &str, mode: ResetMode) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        let id = self.commit_id(rev)?;
        if self.current_branch()?.is_none() {
            return Err(GitError::Rejected(
                "HEAD is detached: there is no branch to reset".into(),
            ));
        }
        let mut kept = None;
        if mode == ResetMode::Hard {
            let (ok, stash, err) = self.plain_git(&["stash", "create"])?;
            if !ok {
                return Err(GitError::Git(err.trim().to_owned()));
            }
            let stash = stash.trim();
            if !stash.is_empty() {
                let message = format!("IDECraft: before hard reset to {}", &id[..7]);
                let (ok, _, err) = self.plain_git(&["stash", "store", "-m", &message, stash])?;
                if !ok {
                    return Err(GitError::Git(err.trim().to_owned()));
                }
                kept = Some(message);
            }
        }
        let flag = format!("--{}", mode.id());
        let mut done = self.git_run(&["reset", &flag, &id])?;
        if let Some(message) = kept {
            done.output = format!("local changes kept in stash \"{message}\"\n{}", done.output)
                .trim_end()
                .to_owned();
        }
        Ok(done)
    }

    /// Adds a commit that takes the change of `rev` back (`git revert --no-edit`). When that
    /// conflicts, the revert is aborted, nothing changes and the files are reported.
    pub fn revert_commit(&self, rev: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        let id = self.commit_id(rev)?;
        let mut done = self.git_run(&["revert", "--no-edit", &id])?;
        if !done.ok && !done.conflicts.is_empty() {
            let _ = self.plain_git(&["revert", "--abort"]);
            done.output = format!(
                "the revert conflicts in {}; nothing was changed",
                done.conflicts.join(", ")
            );
            done.in_progress = self.operation_in_progress();
        }
        Ok(done)
    }

    /// Takes the latest commit back, keeping its changes staged (`git reset --soft HEAD~1`);
    /// only for an unpushed, non-merge commit HEAD points at.
    pub fn undo_commit(&self, rev: &str) -> Result<OpOutcome, GitError> {
        self.refuse_while_in_progress()?;
        self.check_commit_action(rev, CommitAction::Undo)?;
        self.git_run(&["reset", "--soft", "HEAD~1"])
    }

    /// Rebases from the parent of `rev` with that commit's action changed: `Drop` removes it,
    /// `Reword` gives it `message`; the other commits stay as they are.
    fn rewrite_one(
        &self,
        rev: &str,
        action: TodoAction,
        message: Option<&str>,
    ) -> Result<OpOutcome, GitError> {
        let id = self.commit_id(rev)?;
        let onto = format!("{id}^");
        let mut entries = self.rebase_todo(&onto)?;
        let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
            return Err(GitError::Rejected(format!(
                "{} is not among the commits since its parent",
                &id[..7]
            )));
        };
        entry.action = action;
        entry.message = message.map(str::to_owned);
        self.rebase_interactive(&onto, &entries)
    }

    /// Removes the commit `rev` from the current branch (an interactive rebase from its parent
    /// with the commit dropped).
    pub fn drop_commit(&self, rev: &str) -> Result<OpOutcome, GitError> {
        self.check_commit_action(rev, CommitAction::Drop)?;
        self.rewrite_one(rev, TodoAction::Drop, None)
    }

    /// Gives the commit `rev` the message `message` (an interactive rebase from its parent with
    /// the commit reworded).
    pub fn reword_commit(&self, rev: &str, message: &str) -> Result<OpOutcome, GitError> {
        if message.trim().is_empty() {
            return Err(GitError::Rejected("the new message is empty".into()));
        }
        self.check_commit_action(rev, CommitAction::EditMessage)?;
        self.rewrite_one(rev, TodoAction::Reword, Some(message))
    }

    /// The message of a `fixup!` (or `squash!`) commit for `rev`, as `git commit --fixup` writes
    /// it (`SPEC-0086` v8 behaviour 17).
    pub fn fixup_message(&self, rev: &str, squash: bool) -> Result<String, GitError> {
        let id = self.commit_id(rev)?;
        let subject = self.commit_details(&id)?.info.summary;
        let word = if squash { "squash" } else { "fixup" };
        Ok(format!("{word}! {subject}"))
    }

    /// Commits `paths` as a `fixup!` (or `squash!`) commit for `rev`, refused like Edit Commit
    /// Message when `rev` may not be rewritten (`SPEC-0086` v8 behaviour 17).
    pub fn commit_fixup(
        &self,
        rev: &str,
        squash: bool,
        paths: &[String],
        dry_run: bool,
    ) -> Result<crate::CommitOutcome, GitError> {
        let action = if squash {
            CommitAction::SquashInto
        } else {
            CommitAction::Fixup
        };
        self.check_commit_action(rev, action)?;
        let message = self.fixup_message(rev, squash)?;
        self.commit(&message, paths, dry_run)
    }

    /// Creates the lightweight tag `name` at `rev`.
    pub fn create_tag(&self, name: &str, rev: &str) -> Result<(), GitError> {
        if !valid_branch_name(name) {
            return Err(GitError::Rejected(format!(
                "{name} is not a valid tag name"
            )));
        }
        let id = self.commit_id(rev)?;
        let full = format!("refs/tags/{name}");
        if self
            .plain_git(&["rev-parse", "--verify", "--quiet", &full])?
            .0
        {
            return Err(GitError::Rejected(format!("tag {name} already exists")));
        }
        let (ok, _, err) = self.plain_git(&["tag", name, &id])?;
        if !ok {
            return Err(GitError::Rejected(err.trim().to_owned()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(f: &CommitFacts, a: CommitAction) -> Option<&'static str> {
        unavailable(a, f)
    }

    #[test]
    fn ids_and_modes_round_trip() {
        for a in CommitAction::ALL {
            assert_eq!(CommitAction::from_id(a.id()), Some(a));
        }
        for m in ResetMode::ALL {
            assert_eq!(ResetMode::from_id(m.id()), Some(m));
        }
        assert_eq!(CommitAction::from_id("nope"), None);
    }

    #[test]
    fn unpushed_head_allows_everything_but_cherry_pick_and_reset() {
        let f = CommitFacts {
            is_head: true,
            on_current_branch: true,
            ..Default::default()
        };
        let menu = commit_actions(&f);
        assert_eq!(menu.len(), CommitAction::ALL.len());
        let off: Vec<_> = menu
            .iter()
            .filter(|e| !e.enabled)
            .map(|e| e.action)
            .collect();
        assert_eq!(
            off,
            [
                CommitAction::CherryPick,
                CommitAction::Reset,
                CommitAction::GoToChild
            ]
        );
        assert!(menu.iter().all(|e| e.enabled == e.reason.is_none()));
    }

    #[test]
    fn pushed_commits_refuse_rewriting_with_a_reason() {
        let f = CommitFacts {
            on_current_branch: true,
            pushed: true,
            has_child: true,
            ..Default::default()
        };
        for a in [
            CommitAction::EditMessage,
            CommitAction::Drop,
            CommitAction::RebaseFromHere,
        ] {
            assert!(state(&f, a).is_some_and(|r| r.contains("pushed")), "{a:?}");
        }
        assert!(state(&f, CommitAction::Undo).is_some());
        assert_eq!(state(&f, CommitAction::Revert), None);
        assert_eq!(state(&f, CommitAction::Reset), None);
    }

    #[test]
    fn other_branches_allow_cherry_pick_but_no_rewriting() {
        let f = CommitFacts {
            has_child: true,
            ..Default::default()
        };
        assert_eq!(state(&f, CommitAction::CherryPick), None);
        assert!(state(&f, CommitAction::Revert).is_some());
        assert!(
            state(&f, CommitAction::RebaseFromHere).is_some_and(|r| r.contains("current branch"))
        );
        let merge = CommitFacts { merge: true, ..f };
        assert!(state(&merge, CommitAction::CherryPick).is_some());
    }

    #[test]
    fn v8_entries_sit_in_the_reference_groups() {
        let ids: Vec<&str> = CommitAction::ALL.iter().map(|a| a.id()).collect();
        let at = |id: &str| ids.iter().position(|i| *i == id).unwrap();
        assert!(at("checkout") < at("show-repository"));
        assert!(at("show-repository") < at("compare-local"));
        assert!(at("edit-message") < at("fixup") && at("fixup") < at("squash"));
        assert!(at("squash") < at("drop") && at("rebase-from-here") < at("push-up-to"));
        assert_eq!(CommitAction::CherryPick.group(), 0);
        assert_eq!(CommitAction::CompareLocal.group(), 1);
        assert_eq!(CommitAction::PushUpTo.group(), 3);
        // Groups never go back.
        assert!(
            CommitAction::ALL
                .windows(2)
                .all(|w| w[0].group() <= w[1].group())
        );
        let head = CommitFacts {
            is_head: true,
            on_current_branch: true,
            ..Default::default()
        };
        for a in [
            CommitAction::Fixup,
            CommitAction::SquashInto,
            CommitAction::PushUpTo,
            CommitAction::ShowRepository,
            CommitAction::CompareLocal,
        ] {
            assert_eq!(state(&head, a), None, "{a:?}");
        }
        let pushed = CommitFacts {
            pushed: true,
            ..head
        };
        assert!(state(&pushed, CommitAction::Fixup).is_some_and(|r| r.contains("pushed")));
        assert!(state(&pushed, CommitAction::PushUpTo).is_some_and(|r| r.contains("pushed")));
        let other = CommitFacts::default();
        assert!(state(&other, CommitAction::SquashInto).is_some());
        assert!(state(&other, CommitAction::PushUpTo).is_some_and(|r| r.contains("branch")));
        let detached = CommitFacts {
            detached: true,
            ..head
        };
        assert!(state(&detached, CommitAction::PushUpTo).is_some_and(|r| r.contains("detached")));
        assert_eq!(state(&other, CommitAction::CompareLocal), None);
    }

    #[test]
    fn merges_after_root_and_detached_heads() {
        let after = CommitFacts {
            on_current_branch: true,
            merges_after: true,
            ..Default::default()
        };
        assert!(state(&after, CommitAction::Drop).is_some_and(|r| r.contains("linear")));
        let root = CommitFacts {
            on_current_branch: true,
            root: true,
            ..Default::default()
        };
        assert!(state(&root, CommitAction::RebaseFromHere).is_some());
        assert!(state(&root, CommitAction::GoToParent).is_some());
        let detached = CommitFacts {
            detached: true,
            ..Default::default()
        };
        assert!(state(&detached, CommitAction::Reset).is_some_and(|r| r.contains("detached")));
    }
}
