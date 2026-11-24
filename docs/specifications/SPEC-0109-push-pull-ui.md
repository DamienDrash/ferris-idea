# SPEC-0109: Push, Update Project and the VCS operations popup

- **Version:** 4 · **Date:** 2026-11-23 (v4: Rollback lists every changed file with the current one checked and confirms with their number, behaviour 12, AK-10; OBS-0119 K4); 2026-11-06 (v3: ⌘T and ⇧⌘K, shortcuts in the VCS operations popup, Commit runs ⌘K, behaviour 11; v2: the reference's entries and digits in the popup, files per commit, ⌘A in the dialog, "(new)" only for a branch the remote lacks; OBS-0022 L13, L17, L23, L24)
- **Sources:** GEN (VCS operations quick list, Push dialog with outgoing commits, Update Project, Fetch),
  SPEC-0055 (remote operations), SPEC-0106 (stash), SPEC-0101 (conflicts), ADR-0008, ADR-0012
- **Feature matrix rows:** F-045

## Purpose
Push, pull and fetch from the window with the reference's shortcuts, seeing what a push sends before
it happens.

## Behaviour
1. **VCS operations popup:** `Vcs.QuickListPopupAction` (macOS ⌃V, default keymap Alt+`) lists numbered
   operations: Commit, Push, Update Project, Fetch, Stash Changes, Branches, Show History, Resolve
   Conflicts. ↑/↓ and Enter or the digit run one; Escape closes. [GEN]
2. **Actions:** `Vcs.Push`, `Vcs.UpdateProject` (also `Git.Pull`), `Git.Fetch` and `Git.Branches` are
   actions and are found by Search Everywhere. [GEN]
3. **Push dialog:** shows branch → remote/branch ("(new)" without upstream) and the commits the remote
   lacks as of the last fetch, newest first, without network access. Enter or Push pushes and sets the
   upstream for a new branch; Escape or Cancel closes. [GEN, SPEC-0055]
4. **Background and reports:** fetch, update and push run in the background with git's time limit; the
   status bar says "Push…" and then the result: pushed commits, rejection with the reference, missing
   credentials, time out, "already up to date", files changed, or local changes or conflicts stopping
   the update (pointing to Resolve Conflicts). Afterwards the branch in the status bar and open editors
   are refreshed. [ADR-0012, IMP-0011]
5. **MCP:** `git_outgoing` returns what Push would send; `git_fetch`/`git_pull`/`git_push` already exist.
   [ADR-0008]
12. **v4 – Rollback of files:** `ChangesView.Revert` (⌥⌘Z on macOS, Ctrl+Alt+Z elsewhere; Rollback…
    in the VCS operations popup) lists every file of the repository with a change Git knows,
    staged or not, by path, each with a check box; untracked files are never listed. The file of
    the active editor is checked when it is listed, and only it, so Enter right away rolls back
    just that file as before v4. Space checks and unchecks the selected row, ⌘A checks all,
    Escape cancels. Enter with nothing checked does nothing. Enter with checked files asks for a
    confirmation that names the file, or the number of files, with Rollback first and Cancel
    second; it is destructive, so it is never skipped. Rolling back gives each file its HEAD
    content in the working tree and the index (an added file leaves the index and stays on disk;
    SPEC-0063 behaviour 10). Before, the Local History records each file's state under the label
    "Before Rollback", and afterwards labels the step "Rollback", so the earlier text can be
    restored there; open editors show the rolled-back text, discarding their unsaved edits, which
    is what rolling back means. The status bar says how many files were rolled back. With no
    change to roll back the status bar says so and no list opens. ⌥⌘Z is shared with
    `Vcs.RollbackChangedLines` (SPEC-0064 behaviour 7): with the caret in a changed range it
    takes back that change; anywhere else it is this Rollback (before v4 it said there was no
    change at the caret). MCP: `git_rollback` takes several paths (SPEC-0063). [OBS-0119 K4, GEN]

## Acceptance criteria
- **AK-1** Reports for fetch, push and update outcomes (unit tests).
- **AK-2** `Repo::outgoing`: none, two local commits newest first, a new branch without upstream
  (integration test with a bare remote).
- **AK-3** `git_outgoing` with a bare remote (protocol test).
- **AK-4** Window self-test: the Push dialog lists the local commit, Enter pushes it to a bare remote; the
  quick list shortcut opens the popup and 4 fetches.
- **AK-5** Hand test (round 4): push to a real remote and Update Project.
- **AK-6 (v2)** `Repo::outgoing` names the files of each commit and the remote's branches
  (`crates/vcs/tests/it/branch_ops.rs`: `outgoing_names_files_and_remote_branches`; `protocol.rs`:
  `git_outgoing_tool`); entries, digits, "(new)" rule (`vcs_ops_model::tests`); `Repo::add_paths`
  (`branch_ops.rs`: `stash_keeps_the_index_and_takes_a_message`).
- **AK-7 (v2)** Window self-test: the popup lists 16 entries with "1. Commit…" and "0. Unstash
  Changes…"; the Push dialog shows the commit's file; ⌘A leaves the dialog open; the title for the
  existing `main` has no "(new)".
- **AK-8 (v2)** In the window next to the reference: the popup's entries and separator, the Push
  dialog's file column, ⌘A not reaching the editor (window check, OBS-0022 L13, L17, L23).

6. **v2 – Entries of the popup:** in the reference's order, the first ten with the digits 1–9 and 0:
   Commit…, Commit File… (`CheckinFiles`: the Commit window with only the active file checked),
   Rollback… (`ChangesView.Revert`: the changed files, behaviour 12), Show History
   (`Vcs.ShowTabbedFileHistory`: the Git log filtered to the active file), Annotate with Git Blame,
   Show Diff (`Compare.SameVersion`: the active file against HEAD), Branches…, Push…, Stash Changes…,
   Unstash Changes…; after a line: Update Project…, Fetch, Resolve Conflicts…, Add to VCS
   (`ChangesView.AddUnversioned`: `git add` of the active file), Copy Branch Name
   (`Git.CopyBranchName`), Local History. [OBS-0022 L13]
7. **v2 – Files per commit:** the Push dialog lists the commits left and the files the selected commit
   changes right (Git's status letter and path, renames by their new path); ↑/↓ or a click select a
   commit. [OBS-0022 L17]
8. **v2 – Edit keys in the dialog:** ⌘A (and ⌘C, ⌘X, ⌘V, undo/redo from the menu bar) act on the
   target field while the Push dialog is open, and on the message field while the stash dialog is
   open — never on the editor behind them. [OBS-0022 L23]
9. **v2 – "(new)":** the title says "(new)" only when the remote has no branch of the target's name as
   of the last fetch, regardless of whether an upstream is configured. [OBS-0022 L24]
10. **v2 – MCP:** `git_outgoing` also returns `files` (by commit id) and `remote_branches`. [ADR-0008]
11. **v3 – Shortcuts:** Update Project is ⌘T (Ctrl+T) and Push ⇧⌘K (Ctrl+Shift+K), as the reference's VCS plugin binds them (they are not in the platform keymap); the VCS operations popup shows each entry's shortcut from the keymap in use on the right, and its Commit… entry runs Commit (⌘K: shows the Commit window with the focus and never hides it). Before v3 ⌘T and ⇧⌘K did nothing. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L13]
- **AK-9 (v3)** The macOS keymap maps ⌘T to `Vcs.UpdateProject` and ⇧⌘K to `Vcs.Push`, the default keymap Ctrl+T (unit test `keymap::tests::update_and_push_have_the_reference_shortcuts`); `vcs_ops_model` lists `CheckinProject` first.

- **AK-10 (v4, behaviour 12)** Unit test (`vcs_ops_model::tests::rollback_lists_the_changed_files_with_the_current_one_checked`):
  the rows leave out untracked files and check only the current file; the confirmation names one
  file or the number. In a window of the test harness with a Git repository of its own, two
  changed files and an untracked one, by keys
  (`crates/workbench/tests/it/keymap_gaps_obs0119.rs::rollback_lists_the_changed_files_and_rolls_back_the_checked_ones`):
  ⌥⌘Z lists the two changed files with the current one checked; Enter asks about that file,
  Cancel changes nothing; ⌥⌘Z, ↓ Space Enter asks about 2 files, Rollback rolls both back, the
  untracked file stays, the editor shows HEAD's text, the Local History has the label; ⌥⌘Z then
  says there is nothing to roll back.

## Out of scope
Choosing another remote in the dialog, update of several repositories, worktrees in the popup.

## Known deviations from the reference
The Push dialog's files are a flat list per commit, not a tree. "Manage Worktrees" is not in the popup
(no worktree support).
