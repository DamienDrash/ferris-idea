# SPEC-0144: Update with rebase and push options

- **Version:** 5 · **Date:** 2026-11-06 (v5: the reset choice wraps inside the popup, behaviour 10; v4: Update Info and reset to the upstream; OBS-0022 L16; v2: lightweight tags, Enter in the target field, title
  follows the target, the reason of a rejection; hand test 70, D35; v3: Git's merge message for a remote-tracking branch; OBS-0022)
- **Sources:** GEN (IDEs ask how Update Project brings in incoming commits — merge or rebase — and
  remember the choice; their push dialog lets the user edit the target branch, push tags and
  force push; F-074), STD (`git rebase --autostash <upstream>`, `git push --follow-tags`,
  `--force-with-lease=<ref>` against the remote-tracking branch, `refspec src:dst`,
  `git check-ref-format` rules), SPEC-0055, SPEC-0106, SPEC-0109, IMP-0029, ADR-0008, ADR-0012
- **Feature matrix rows:** F-074

## Purpose
Close the gaps SPEC-0109 left out: update by rebase, push under another branch name, with tags,
or over rewritten history, without a plain force push.

## Behaviour
1. **Update Project dialog:** `Vcs.UpdateProject` (also `Git.Pull`, the VCS popup's "3") opens a
   dialog with "Merge incoming changes into the current branch" and "Rebase the current branch on
   top of incoming changes"; the last choice is preselected (`settings.toml` `[vcs]
   update_method`, default merge). ↑/↓, ⌥M, ⌥R or a click choose; Enter updates and remembers the
   choice; Escape cancels. [GEN]
2. **Merge** behaves as in SPEC-0109. **Rebase** fetches the upstream's remote, then replays the
   branch's own commits onto the upstream with local changes stashed and restored around it. The
   status bar says "Updated from origin/main by rebase: n commit(s) replayed", "Update: already up
   to date", or, on conflicts, which files conflict and that `Git.Continue` or `Git.Abort` goes on
   (SPEC-0106 behaviour 6). [STD, ADR-0012]
3. **Push dialog options:** "Target branch" (prefilled with the local name; Tab focuses it; Enter
   in it pushes like anywhere in the dialog; the title "Push main → origin/<target>" follows what is
   typed and says "(new)" for another branch name), "Push tags" (⌥T, the annotated and lightweight
   tags on the pushed branch's history: `--follow-tags` plus a refspec per lightweight tag; tags the
   remote already has unchanged are up to date) and "Force push, only if
   unchanged since fetch" (⌥F; the button reads "Force Push"). A target that is not a valid
   branch name, or that starts with `-`, is refused before git runs. The upstream is set only
   when a new branch is pushed under its own name. [GEN, STD]
4. **Force push** is always `--force-with-lease` against the remote-tracking branch: if someone
   pushed since the last fetch, the push is rejected with git's reason. A plain force push does
   not exist. Every rejection names the branch, git's reason and what to do: "stale info" — fetch
   and look at the new commits; "fetch first"/"non-fast-forward" — Update Project first; a tag that
   exists — the remote has another tag of that name. [STD, IMP-0029]
5. **MCP:** `git_pull` takes `rebase` (dry run lists the commits to replay); `git_push` takes
   `target` and `tags` for the current branch. [ADR-0008]

6. **v3 – Merge message:** an update by merge (and any merge of a remote-tracking branch) gets Git's
   message `Merge remote-tracking branch 'origin/main'`; before v3 it read `Merge commit
   'origin/main'`. [STD, OBS-0022 L19]

7. **v4 – Dialog choices:** besides merge and rebase the dialog offers "Reset the current branch to its
   upstream", naming how many local commits it gives up (as of the last fetch) (⌥E; ↑/↓ cycle through
   the three). The reset is never remembered as the preselected choice. [OBS-0022 L16, IMP-0322]
8. **v4 – Update Info:** when an update (merge, rebase or reset) moved HEAD, a popup "Update Info: n
   commit(s), m file(s)" lists the commits that came in (newest first, with author) and the files that
   differ (Git's status letter and path); Enter on a commit selects it in the Git log, on a file opens
   it (a deleted file is named in the status bar). `FerrisIDEA.ShowUpdateInfo` shows the last one again.
   `git_pull` returns it as `info`. [OBS-0022 L16]
9. **v4 – Reset to the upstream:** fetches the upstream's remote, lists the branch's own commits that
   the upstream lacks (`dropped`), stashes local changes with untracked files, `git reset --hard
   <upstream>`, and brings the changes back; changes that conflict stay in the stash and are named. The
   status bar names the short ids of the commits given up. `git_pull` takes `reset` (the dry run only
   lists `dropped`). Refused while an operation waits. [STD, IMP-0322]
10. **v5 – Reset choice:** the reset choice's text wraps inside the popup instead of running past its edge, and names the commits it gives up with the right plural ("1 local commit", "2 local commits"). [OBS-0022 Nachprüfung im Fenster 2026-11-06, L16]

## Acceptance criteria
- **AK-1** Against a bare remote and two clones (`crates/vcs/tests/it/remote.rs`): update by rebase
  previews the commit to replay, keeps history linear and an uncommitted change, and stops on a
  conflict with the file named; push to another target with tags; a plain push of rewritten
  history is rejected, a lease push goes through, and a lease push after someone else pushed is
  rejected with "stale info"; a lightweight tag is pushed too and a second push of the same tags is
  up to date; `--delete` as target is refused.
- **AK-6** Rejection texts with reason and hint (`vcs_ops_model::tests::reports_say_what_happened`).
- **AK-2** Reports for rebase, up to date and conflicts (`vcs_ops_model::tests::rebase_reports`).
- **AK-3** MCP: target and tags, refusal when combined with `branch`, pull with rebase as dry run
  and for real (`crates/mcp/tests/it/protocol.rs`: `git_update_with_rebase_and_push_options`).
- **AK-4** Window self-test: the Push dialog with target and ⌥T pushes the commit and its
  lightweight tag to the target branch, its title names the typed target, and Enter in the target
  field pushes; ⌃V 3 opens Update Project with merge preselected; ↓ Enter rebases, reports
  "already up to date" and remembers rebase.
- **AK-5** Hand test (round 4, no. 70): both update methods and the push options against a real
  remote, next to the reference.

- **AK-7 (v3)** Merging `origin/other` writes `Merge remote-tracking branch 'origin/other'`
  (`crates/vcs/tests/it/smart.rs`: `merging_a_remote_tracking_branch_uses_gits_message`).
- **AK-8 (v4)** A pull that brings a commit has `info` with the commit and the added file; nothing new:
  no `info`; reset gives up the local commit, keeps the local change, empties the stash and reports the
  deleted file (`crates/vcs/tests/it/branch_ops.rs`: `update_info_and_reset_to_upstream`).
- **AK-9 (v4)** Reports and rows: `vcs_ops_model::tests::reset_reports_name_what_was_given_up`,
  `operations_digits_push_target_history_and_update_rows`; `git_pull` with `reset` as dry run
  (`protocol.rs`: `git_outgoing_tool`).
- **AK-10 (v4)** In the window next to the reference: the third choice and the Update Info popup after
  an update with incoming commits (window check, OBS-0022 L16).
- **AK-11 (v5)** `reset_choice_label` for 0, 1 and 2 commits (unit test in `vcs_ops_model::tests`).

## Out of scope
Interactive rebase, choosing another remote in the push dialog, pushing several branches, the
reference's "Shelve" alternative to stashing, update of several repositories.

## Known deviations from the reference
The reference's force push can also be plain; Ferris IDEA only offers the lease variant (IMP-0029).
- The reference wrote the merge commit of its own branch merge as `Merge branch
  'refs/heads/feature/conflict'` (OBS-0022); Ferris IDEA keeps Git's wording with the short name
  (IMP-0318).
- v4: the reference resets to the remote branch without naming what is lost; Ferris IDEA names the number
  of local commits before and their short ids after, keeps local changes, and never preselects the
  reset next time (IMP-0322). The update information is one popup with commits and files instead of a
  tool window tab plus a notification.
