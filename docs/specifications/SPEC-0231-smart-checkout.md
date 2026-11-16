# SPEC-0231: Smart checkout and smart merge

- **Version:** 2 · **Date:** 2026-11-06 (v2: the question also when the merge itself would conflict; OBS-0022)
- **Sources:** OBS-0009 (the reference lists the files a merge would overwrite and offers to put the
  local changes aside, merge and bring them back; during a merge with conflicts they stay aside until the
  merge is finished), GEN (IDEs offer the same when checking out a branch), STD (`git stash push
  --include-untracked`, `git stash apply`/`drop` by commit id, a stash that does not apply cleanly stays
  in the list), SPEC-0035, SPEC-0036, SPEC-0068, SPEC-0106, ADR-0008
- **Feature matrix rows:** F-074

## Purpose
Switching branches or merging while files are changed does not end in a refusal: the local changes are
put aside, the operation runs, and the changes come back.

## Behaviour
1. **Question:** when a checkout from the branch popup or a merge into the current branch from the popup
   is refused because local changes would be overwritten (SPEC-0035, SPEC-0036), a popup titled "Local
   changes would be overwritten: <files>" (at most three names, then "+n more") offers "Smart Checkout
   (stash, check out, unstash)" or "Smart Merge (stash, merge, unstash)", selected, and "Cancel". [OBS,
   GEN]
2. **Put aside:** Smart stashes all local changes including untracked files with the message
   `IDECraft smart checkout: <from> → <to>` (or `smart merge`) and records the stash's commit id in
   `.git/idecraft/smart-stash`. Nothing to stash: the operation runs directly. A failing stash leaves
   everything as it was and reports why. [STD, IMP-0080]
3. **Bring back:** after the checkout, or after a merge that finished, the recorded stash is applied and
   then dropped — found by its commit id, so stashes made meanwhile do not matter. When applying it
   conflicts, the stash stays in the stash list, the files with conflicts are reported and the merge view
   opens (SPEC-0101); the status bar names the stash. [STD, GEN]
4. **Merge that stops:** if the merge stops on conflicts, the changes stay in the stash and the status
   bar says so; when the merge is finished (Git: Continue, or Apply of the last conflict) or aborted, the
   stash is brought back as in behaviour 3. [OBS]
5. **Failed operation:** when the checkout or merge fails for another reason, the stash is brought back at
   once, so the working tree is as before. [GEN]
6. **MCP:** `git_switch_branch` and `git_merge` take `smart` (with `dry_run` false): local changes in the
   way are put aside and brought back as above; the result carries `stash` with its message, whether it
   was restored and the files with conflicts. Without `smart` they are refused as before. [ADR-0008]

7. **v2 – Merge that would also conflict:** the preview of a merge names the local changes in the way
   also when the merge itself would conflict (it writes the conflicted tree to memory and compares),
   so the question of behaviour 1 comes before Git refuses; before v2 the status bar only said that
   the merge failed. [STD, OBS-0022 L2]

## Acceptance criteria
- **AK-1** Scratch repository: a modified file that the other branch changes; smart checkout switches,
  the change is back afterwards and the stash list is as before (integration test).
- **AK-2** The change conflicts with the other branch: the file has conflict markers and stays in the
  conflict list, the stash is kept and named (integration test).
- **AK-3** Smart merge that stops on a conflict keeps the stash; after resolving and continuing the
  local change is back (integration test).
- **AK-4** `git_switch_branch` with and without `smart` (protocol test).
- **AK-5** In a real window (`idecraft ui --selftest`): checkout of a branch with a local change in the
  way asks, Smart Checkout switches and the change is still in the file.
- **AK-6** Hand test next to the reference (merge and checkout with local changes).

- **AK-7 (v2)** A local change in a file that the merge would also conflict in: the preview lists it in
  `local_conflicts` and `merge_conflicts` (`crates/vcs/tests/it/smart.rs`:
  `smart_merge_waits_with_the_stash_until_the_merge_is_done`).

## Out of scope
Force checkout (discarding the local changes), the reference's shelf and its tool window tab, choosing
single files to put aside.

## Known deviations from the reference
The changes go into a named `git stash` entry instead of an IDE-internal shelf, so they stay visible and
recoverable with Git itself (`git stash list`) if IDECraft is closed in between (IMP-0080). The question
is a short popup instead of a dialog with a file tree.
