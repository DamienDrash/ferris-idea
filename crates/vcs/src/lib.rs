//! Version control integration for Ferris IDEA (Git first).
//!
//! Implements `SPEC-0032` (Git status, history and changes of a file; v3 the summary of a
//! repository and the repositories below a folder),
//! `SPEC-0033` (blame, branches), `SPEC-0277` (blame at HEAD for annotations), `SPEC-0034` (commit, create branch),
//! `SPEC-0035` (switch branch), `SPEC-0036` (merge), `SPEC-0037`
//! (three-way text merge), `SPEC-0292` v4 (authors of declarations), `SPEC-0064` v3 and `SPEC-0084` v2
//! (one change of a file, changed words, unified rows), `SPEC-0055` (remotes, fetch, pull, push), `SPEC-0230`
//! (commit hooks) and `SPEC-0231` (smart checkout) with
//! `gix` for repository access, the `git` executable for network operations
//! and `similar` for line diffs; `SPEC-0340` (change sets of agents, [`change_set`]).

mod authors;
mod branch_ops;
pub mod change_set;
mod clone;
mod diff;
mod git;
mod hooks;
mod log_actions;
mod merge3;
mod ops;
mod remote;
mod resolve;

pub use authors::{
    AUTHOR_CHARS, CodeAuthor, DeclarationLines, author_label, changed_within, declaration_authors,
};
pub use branch_ops::{ChangedPath, RECENT_BRANCHES, StashOptions, Tag, UpdateInfo};
pub use clone::{
    CloneOutcome, CloneProgress, CloneSource, CloneTransport, check_clone_url, check_new_folder,
    clone_args, clone_repository, new_project, parse_progress,
};
pub use diff::{
    DiffRow, LineChange, LineChangeKind, UnifiedRow, WordRanges, apply_change, change_at_line,
    change_of_rows, change_starts, changed_words, line_changes, map_lines, old_text, revert_change,
    row_changes, side_by_side, take_change, unified_hunks, unified_rows,
};
pub use git::{
    BlameCommit, BlameRange, Branch, Branches, ChangeKind, CommitChange, CommitDetails, CommitInfo,
    CommitOptions, CommitOutcome, Conflict, FileDiff, FileStatus, GitError, HeadBlame, MergeKind,
    MergeOutcome, Rename, Repo, RepoSummary, RollbackOutcome, SwitchOutcome, differing_committer,
    nested_repositories, parse_author,
};
pub use hooks::{COMMIT_HOOKS, HOOK_TIMEOUT, HookRun};
pub use log_actions::{
    ActionState, CommitAction, CommitFacts, ResetMode, commit_actions, unavailable,
};
pub use merge3::{Chunk, ChunkKind, Labels, MarkerStyle, Merge3, Resolution, has_conflict_markers};
pub use ops::{
    BranchState, LOCAL_TIMEOUT, OpOutcome, Operation, SmartOutcome, SmartStash, Stash, StashFile,
    TodoAction, TodoEntry,
};
pub use remote::{
    Outgoing, PullOutcome, PushOptions, PushPreview, REMOTE_TIMEOUT, RefUpdate, Remote,
    RemoteOutcome, Tracking, valid_branch_name,
};
pub use resolve::{
    ConflictRegion, Outcome, Side, merge_model, merge_open, merge_outcome, merge_result,
};
