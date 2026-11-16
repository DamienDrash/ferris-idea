# SPEC-0106: Git stash, cherry-pick and rebase

- **Version:** 4 · **Date:** 2026-11-06 (v4: Unstash opens on the newest stash, behaviour 15; v2: skip, merge view and widget on stopped operations; v3: stash dialog with message and Keep index; OBS-0022 L14)
- **Sources:** STD (Git: `git stash`, `git cherry-pick`, `git rebase` with `--continue`/`--abort`, the
  state files `rebase-merge`, `rebase-apply`, `CHERRY_PICK_HEAD`, `MERGE_HEAD`), GEN (stash and rebase
  commands of desktop IDEs), SPEC-0033, SPEC-0036, SPEC-0055, ADR-0008, ADR-0012
- **Feature matrix rows:** F-074

## Purpose
The user stashes local changes and brings them back, applies a commit of another branch to the current
one and rebases the current branch, and finishes or aborts an operation that stopped on conflicts.

## Behaviour
1. **Stash list:** the stashes, newest first, with Git's message. [STD]
2. **Stash:** the local changes are stashed, optionally with untracked files and a message. [STD]
3. **Apply, pop, drop:** a stash is applied (kept), popped (dropped when it applied without
   conflicts) or dropped; an unknown position is refused. [STD]
4. **Cherry-pick:** the change of one commit is applied to the current branch as a new commit. [STD]
5. **Rebase:** the current branch's own commits are replayed onto another branch or revision; the
   preview lists them, oldest first. [STD]
6. **Stopped operations:** a rebase, cherry-pick or merge that stops on conflicts reports the files
   with conflicts and waits; it is continued after the conflicts are resolved and staged, or aborted,
   which restores the state before it. While one waits, no other cherry-pick or rebase starts. [STD]
7. **Stash popup:** `Git.Stash` and `Git.Unstash` (Search Everywhere) open a popup with "Stash
   Changes", "Stash Changes with Untracked Files" and the stashes; Enter applies a stash, Shift+Enter
   pops it, Delete drops it, Escape closes. [GEN]
8. **Starting from lists:** in the branch popup Alt+Enter rebases the current branch onto the selected
   branch; in the Git log Alt+Enter cherry-picks the selected commit. [GEN]
9. **Feedback:** operations run in the background; the status bar says when one is done, why it
   failed, or which files have conflicts and that `Git.Continue` or `Git.Abort` goes on. Open editors
   pick up the changed files. [GEN, ADR-0012]
10. **MCP:** `git_stash` (list, push, apply, pop, drop), `git_cherry_pick` and `git_rebase` (onto, or
    continue/abort); cherry-pick and rebase default to a dry run that shows the commit or the commits
    to replay. [ADR-0008]
11. Operations run the `git` executable with a time limit, never prompt, and pass the editor as
    `true` so continuing does not wait for a message editor. [STD, ADR-0012]

12. **v2 – Skip:** `Git.Skip` (and the branch popup row "Skip Commit") drops the commit a rebase or
    cherry-pick stopped on (`git rebase --skip`, `git cherry-pick --skip`) and goes on; `git_rebase` takes
    `action: "skip"`. A merge cannot be skipped. [STD, OBS-0009]
13. **v2 – After an operation:** when an operation stops on conflicts the merge view opens (SPEC-0101 v3)
    and the branch widget shows the waiting operation (SPEC-0068 v3); when it ends, a smart checkout's
    stash comes back (SPEC-0231). [OBS-0009]

14. **v3 – Stash dialog:** `Git.Stash` (VCS popup "9") opens a dialog naming the current branch with a
    message field, "Keep index" (⌥K: the staged changes stay staged and in the working tree, `git stash
    push --keep-index`) and "Include untracked files" (⌥U). ↑ fills the field with an earlier message
    (this session's, newest first, then those of the stash list without Git's `On <branch>:` prefix and
    without Git's own `WIP on …` messages), ↓ goes back; Enter stashes, Escape cancels. `Git.Unstash`
    (VCS popup "0") opens the stash list of behaviour 7. `git_stash` takes `keep_index`. [STD,
    OBS-0009 item 9, OBS-0022 L14]
15. **v4 – Unstash selection:** when the stash list opens with stashes (Unstash, ⌃V 0), the newest stash is selected, so Enter applies and ⇧Enter pops it, as the reference selects it; without stashes the first command stays selected. Before v4 Enter made a new stash. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L14]

## Acceptance criteria
- **AK-1** Stash push with untracked files, list, apply, pop and drop on a scratch repository; an unknown
  stash is refused (integration test).
- **AK-2** Cherry-pick applies a commit; a conflicting one stops with the file and the operation in
  progress, refuses a rebase meanwhile, and abort restores the file (integration test).
- **AK-3** Rebase preview lists the commits; a conflicting rebase stops, continues after resolving and
  yields the expected history (integration test).
- **AK-4** `git_stash`, `git_cherry_pick` and `git_rebase` with dry runs and real runs, and refusals of
  unknown actions and incomplete calls (protocol test); status texts for done, conflicts and failures
  (unit test).
- **AK-5** Window self-test: the stash popup stashes a change and pops it back; a commit of another
  branch is cherry-picked.
- **AK-7 (v2)** Skip of a stopped rebase drops the commit and finishes; skip without an operation is
  refused (integration test); `git_rebase` with `skip` (protocol test).
- **AK-6** Hand test (round 4): Alt+Enter in the branch popup and the Git log, a stopped rebase finished
  with Git: Continue.
- **AK-8 (v3)** `Repo::stash_push_with` keeps the index and takes the message
  (`crates/vcs/tests/it/branch_ops.rs`: `stash_keeps_the_index_and_takes_a_message`); message history
  (`vcs_ops_model::tests`).
- **AK-9 (v3)** `git_stash` with `keep_index` (`protocol.rs`: `git_branch_popup_tools`).
- **AK-10 (v3)** Window self-test: the dialog stashes with the typed message and ⌥K keeps the staged
  file; look of the dialog next to the reference (window check).
- **AK-11 (v4)** The first selection is the newest stash when there is one (unit test `stash_view::tests::unstash_opens_on_the_newest_stash`).

## Out of scope
Interactive rebase (reordering, squashing, editing messages), stashing single files, showing a stash's
diff, cherry-picking several commits at once, a dedicated rebase dialog with options.

## Known deviations from the reference
Rebase and cherry-pick are reached with Alt+Enter in the branch popup and the Git log instead of
context-menu entries, and continue/abort are actions instead of a banner with buttons. The status bar
names the files with conflicts; resolving them uses the merge view of SPEC-0101 once merged.
Stashing can include untracked files (popup entry and `git_stash` `include_untracked`), which the
reference's stash dialog cannot (IMP-0040).
