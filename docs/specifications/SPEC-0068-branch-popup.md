# SPEC-0068: Branch popup

- **Version:** 9 · **Date:** 2026-11-13 (v9: New Branch and Rename as actions of their own, behaviours 32–33; docs/research/KEYMAP_COVERAGE.md; v8 2026-11-06: favourite branches with a star, favourites first, Space and Mark/Unmark as Favorite, kept per repository in IDECraft's settings, behaviours 25–31; planned by the project owner, partly not observed yet; v7: the actions at the top show their shortcuts, behaviour 24; v6: Compare with, Show Diff with Working Tree and Push… in the submenu of a branch; window comparison after #392; v5: actions at the top, Checkout Tag or Revision, Recent, folders, tracking, tags, a submenu per branch, the offer to delete a merged branch; OBS-0022 L12, L20; v2: "New Branch…" entry, look, fallback identity; from hand test 8; v3: ahead/behind and stopped operations in the widget, Continue/Abort/Skip, merge into current, smart checkout; v4: the widget follows changes made outside without leaving the window; OBS-0022)
- **Sources:** GEN (the branch widget of the main toolbar opens a list of local and remote branches with a
  filter, "new branch" and checkout; F-074), SPEC-0033 (branches), SPEC-0034 (create branch), SPEC-0035
  (switch branch), SPEC-0061 (branch widget)
- **Feature matrix rows:** F-074

## Purpose
Switch and create branches from the toolbar, as with the reference's branch widget.

## Behaviour
1. **Open:** a click on the branch widget opens the popup under it with the filter field focused. [GEN]
2. **List:** groups "Local" and "Remote" (empty groups hidden), each branch with its short commit id; the
   current branch is marked and selected. Typing filters both groups fuzzily (SPEC-0031 matching) and the
   best match is selected. [GEN]
3. **New branch:** when the filter text is a valid branch name that no local branch has, a row "New Branch
   '<name>'" comes first; it is selected only when no branch matches. Invalid names (spaces, `..`, `~ ^ : ?
   * [ \ @ {`, leading `-` `/` `.`, trailing `/` `.` or `.lock`, `//`) get no such row. Up and Down move over
   the rows (skipping headings) and wrap around. [GEN, STD (Git ref names)]
4. **Checkout:** Enter or a click: a local branch is switched to; a remote-tracking branch `origin/x` is
   switched to the local branch `x`, created at `origin/x` when missing; "New Branch" creates the branch at
   HEAD and switches to it. Local changes in the way refuse the switch with the paths (SPEC-0035). The
   result appears in the status bar, the toolbar shows the new branch, and open editors pick up changed
   files through the disk check (SPEC-0058 behaviour 11). Choosing the current branch just closes. [GEN]
5. **Close:** Escape closes the popup and returns to the editor. [GEN]
6. The MCP counterparts are `git_branches`, `git_create_branch` and `git_switch_branch`; the checkout of
   the popup is the domain function `Repo::checkout`. [ADR-0008, ADR-0012]
7. **v2 – "New Branch…" entry:** with an empty filter a row "+ New Branch…" comes first (the current
   branch stays selected). Choosing it switches the popup to naming: the field is emptied, its placeholder
   reads "Name of the new branch, then Enter", and the list shows only the "New Branch '<name>'" row while
   the typed name is valid and unused. Enter creates and checks out the branch (behaviour 4); Escape returns
   to the branch list instead of closing. [GEN]
8. **v2 – Look:** the popup uses the theme's popup background and border (as SPEC-0067 v2); rows span the
   full width with a branch icon, the commit id and "current" right-aligned and readable on the selection;
   the filter field shows a magnifier and the placeholder "Filter branches". A mouse click keeps the
   keyboard focus in the filter field. [GEN]
9. **v2 – Fallback identity:** in a repository without `user.name`/`user.email` (none in any config file),
   creating and checking out branches still works: like git, IDECraft uses the user name and
   `<user>@<host>` for the reflog. The identity is held in memory only; no config file is written. [STD
   (git's fallback identity)]

10. **v3 – Widget:** after the branch name the widget shows `↑n` and `↓m` — commits the current branch is
    ahead of and behind its upstream as of the last fetch — when the branch has an upstream and the count
    is not zero. While a merge, rebase or cherry-pick waits on conflicts, the widget shows a warning icon
    and "Merging <branch>", "Rebasing <branch>" or "Cherry-picking" instead. The widget is refreshed after
    every Git operation of IDECraft and when the window gets the focus. [GEN, OBS-0009]
11. **v3 – Stopped operation:** while an operation waits, the popup starts with a group named after it
    ("Rebase in progress", …) with the rows "Continue Rebase", "Abort Rebase" and, for a rebase or
    cherry-pick, "Skip Commit" (Git: Continue / Abort / Skip, SPEC-0106 v2). [OBS-0009]
12. **v3 – Merge and rebase from the list:** ⌘↩ merges the selected branch into the current one (SPEC-0036
    v2 behaviour 7); ⌥↩ rebases the current branch onto it (SPEC-0106). A line at the bottom of the popup
    names the three keys: "↩ Checkout · ⌥↩ Rebase onto · ⌘↩ Merge into current". [GEN, OBS-0009]
13. **v3 – Local changes in the way:** a checkout or merge refused because of local changes asks for a
    smart checkout or smart merge (SPEC-0231). [OBS-0009]

14. **v4 – Changes from outside:** the branch widget reads branch, ahead/behind and a waiting operation
    in the background every 2 seconds (with the file status of SPEC-0059 behaviour 7), so a checkout,
    commit or rebase in the IDE's terminal or in another program shows without the window losing and
    regaining the focus. [GEN, OBS-0022 L29]

15. **v5 – Actions at the top:** with an empty filter the popup starts (after the rows of a waiting
    operation) with "Update Project…", "Commit…", "Push…", "New Branch…" and "Checkout Tag or
    Revision…", each with an icon; the first three run `Vcs.UpdateProject`,
    `ActivateCommitToolWindow` and `Vcs.Push`. [OBS-0009 item 1, OBS-0022 L12]
16. **v5 – Checkout Tag or Revision:** the field then takes a tag, branch or commit (placeholder
    "Tag, branch or commit to check out, then Enter"; not empty, no spaces, no leading `-`); Enter
    checks it out as a detached HEAD (`git switch --detach`), Git refusing when local changes would be
    overwritten; Escape returns to the list. A tag row checks out the same way. [STD, OBS-0022 L12]
17. **v5 – Recent:** a group "Recent" lists up to five local branches checked out recently, newest
    first, read from HEAD's reflog (`checkout: moving from A to B`), without the current branch and
    without branches deleted since. [STD, OBS-0009 item 1]
18. **v5 – Folders and tracking:** in "Local" branches with a `/` sit in a folder named after the part
    before the first `/` (the current branch's folder first, then in name order); in "Remote" every
    remote is a folder. A folder row shows a chevron; Enter, a click or → opens or closes it, ← closes
    it (also from a branch inside it). Rows inside a folder are indented and show the name without the
    folder. A local branch with an upstream shows `↑n ↓m <upstream>` (counts as of the last fetch, only
    when not zero) or `<upstream> (gone)`. All folders start open (IMP-0323). While filtering, the
    matches are listed flat with their full names. [GEN, OBS-0022 L12]
19. **v5 – Tags:** a folder "Tags (n)" at the end lists the tags with their commit; it starts closed
    (IMP-0323). A filter also matches tags (group "Tags"). [OBS-0022 L12]
20. **v5 – Submenu per branch:** → on a branch or tag (or a click on the row's chevron) opens its
    submenu to the right of the list; ↑/↓ choose, Enter or → runs, ← or Escape closes it. Local branch:
    Checkout; New Branch from '<b>'…; Checkout and Rebase onto '<current>' (`git rebase <current>
    <b>`); Rebase '<current>' onto '<b>'; Merge '<b>' into '<current>'; Rename… (the field takes the
    new name, prefilled); Delete (only a branch merged into HEAD, never the current one, never forced).
    Current branch: New Branch from…, Push…, Rename…. Remote branch: Checkout, New Branch from…,
    Rebase onto, Merge into. Tag: Checkout, New Branch from…, Merge into. The footer names "→ More".
    [OBS-0009 item 1, OBS-0022 L12]
21. **v5 – MCP:** `git_branches` also returns `tracking`, `recent` and `tags`; the new tool
    `git_branch_action` deletes (only merged), renames, checks out a revision detached, checks out and
    rebases, and tells whether a branch is merged; write actions default to a dry run and need the
    `vcs-write` right. [ADR-0008]
22. **v5 – Delete after merge:** after a merge of a local branch from the popup (⌘↩ or the submenu,
    also a smart merge) a popup offers "Delete branch '<b>'" or "Keep '<b>'"; a merge that waits on
    conflicts offers it once the merge commit exists (the branch is in HEAD's history and no operation
    waits), an aborted merge never. Deleting uses the same safe delete as behaviour 20. [OBS-0009
    item 5, OBS-0022 L20]

23. **v6 – More in the submenu:** the submenu of a local branch that is not the current one and of a
    remote branch also has, after Merge: **Compare with '<current>'** — a list of the commits the
    branch has and the current branch lacks (marked `+`) and of those the current branch has and the
    branch lacks (marked `-`), newest first in each part; Enter selects the chosen commit in the Git
    log tool window (opening it); **Show Diff with Working Tree** — the files that differ between the
    branch and the working tree, Enter opens the diff viewer of SPEC-0084 v2 with the branch on the
    left and the working copy on the right, read-only (as Compare with Branch… for one file); and,
    for a local branch, **Push…** — the push popup of SPEC-0109 for that branch (its unpushed
    commits, its upstream's remote; pushed as `refs/heads/<branch>`). A tag has Compare with and Show
    Diff with Working Tree too. `git_branch_action` gets `compare` (returns `ahead` and `behind`
    commits of the branch against HEAD) and `diff-worktree` (returns the changed files; with `path`
    the diff rows); both read only. [OBS-0022 L12, GEN]
24. **v7 – Shortcuts of the actions:** Update Project…, Commit… and Push… at the top of the popup show their shortcut (⌘T, ⌘K, ⇧⌘K on macOS) from the keymap in use, as the reference does; Commit… runs Commit (⌘K). [OBS-0022 Nachprüfung im Fenster 2026-11-06, L12]

25. **v8 – Favourites:** any local or remote-tracking branch can be marked as a favourite. The
    favourites are kept per repository (keyed by its work tree relative to the workspace root) in the
    workspace's IDECraft settings, `.idecraft/branch-favorites.toml`, which is machine-local state
    (listed in `.idecraft/.gitignore`); nothing is written into `.git`. Local and remote-tracking
    branches are kept apart: marking `main` does not mark `origin/main`. `main` and `master`, and
    `<remote>/main` and `<remote>/master` on every remote, are favourites until the user unmarks them;
    an unmarked default stays unmarked. [DOC (JetBrains help "Manage branches": the main branch is a
    favourite by default), GEN; `master` and the remote defaults: not observed, check in the next
    side-by-side comparison]
26. **v8 – Star:** the row of a favourite branch shows a filled star before its branch icon; the other
    branch rows show an outline star there while they are highlighted or under the pointer, and an
    empty slot otherwise, so names stay aligned. A click on the star marks or unmarks the branch
    without checking it out and without closing the popup; the same branch stays highlighted. Tags,
    folders and the other rows have no star. Both stars are own drawings in the style of the open New
    UI icons, light and dark (ADR-0009). [OBS-0009 item 1 (a star appears when the pointer is over a
    branch), DOC]
27. **v8 – Favourites first:** in "Local" and in "Remote" the current branch comes first, then the
    favourites in name order, then the other branches; a folder takes the place of its first branch,
    so a folder holding a favourite moves up with it, and inside a folder the favourites come first
    too. "Recent" keeps its order. While a filter is typed, the matches keep the order of how well
    they match and show their stars. [DOC ("favourites are listed at the top"); folders and filter:
    not observed, check in the next side-by-side comparison]
28. **v8 – Keys:** Space marks or unmarks the highlighted local or remote branch, also while a filter
    is typed (branch names cannot contain spaces); while a name or revision is typed (behaviours 7,
    16, 20) Space is typed as usual. [DOC]
29. **v8 – Submenu:** the submenu of every local branch (the current one included) and every remote
    branch ends with **Mark as Favorite** or, for a favourite, **Unmark as Favorite**; choosing it
    toggles the mark, closes the submenu and keeps the popup open with the branch highlighted. The
    Branch pages of the Git log's commit menu (SPEC-0086 v8 behaviour 17) list the entry too.
    [GEN (the action name of the Git log's branches pane, DOC); in the popup's submenu not observed,
    check in the next side-by-side comparison]
30. **v8 – MCP:** `git_branches` gives every branch `favorite`; the new tool `git_branch_favorite`
    marks (`favorite: true`), unmarks (`false`) or toggles (left out) branch `name` (`remote: true`
    picks the remote-tracking branch when a local branch has the same name; by default the local one
    when it exists) and returns the new state. It changes only IDECraft's own settings, not the
    repository or the user's files, so it needs no right; the popup and the Git log show the change
    the next time they read the branches. [ADR-0008]
31. **v8 – Rename keeps the mark:** renaming a favourite from the popup's submenu or with
    `git_branch_action` `rename` moves the mark to the new name (IMP-0340). [GEN]
32. **v9 – New Branch as an action:** `Git.CreateNewBranch` (⌥⌘N on macOS; Git › New Branch…) opens the popup with the field already taking the new branch's name, as choosing the "New Branch…" row does (behaviour 7): Enter creates the branch at HEAD and checks it out, Escape goes back to the list. [OSS: id and shortcut of the bundled keymaps; GEN]
33. **v9 – Rename as an action:** `Git.Rename.Local.Branch` (F2, ⇧F6) while the popup has the focus and a local branch is highlighted lets the field take the new name, as Rename… of the branch's submenu does (behaviour 20, with the checks of SPEC-0034 and behaviour 31). With its keys it runs only in the popup (elsewhere the keys keep their other actions). From Find Action, without an open popup, it opens the popup renaming the current branch; on a detached HEAD it says so in the status line. Nothing is rewritten: a rename moves the local name only. [OSS: id and shortcuts; GEN]

## Acceptance criteria
- **AK-1** `Repo::checkout`: local branch, remote-tracking branch creating and later reusing its local
  branch, new branch, an existing name refused (integration test on a scratch repository).
- **AK-2** Rows, current selection, filter, new-branch row and its selection, name validation, wrapping
  (unit tests).
- **AK-3** In a real window (`idecraft ui --selftest`): the popup selects `main`; typing a new name and
  Enter creates and checks out `topic`; typing `main` and Enter switches back.
- **AK-4** Use by hand with the mouse and with remote branches.
- **AK-5 (v2)** Rows with the "New Branch…" entry and naming rows (unit tests); in a real window the entry
  is chosen, a name typed and Enter creates the branch (`--selftest`); a repository opened without any
  identity creates and checks out branches (unit test in `vcs::git`).

- **AK-6 (v3)** Widget label for ahead/behind and each stopped operation, operation rows, footer
  (unit tests); in a real window a stopped rebase shows "Rebasing topic" and the Abort row restores the
  branch (`--selftest`).

- **AK-7 (v4)** In the release bundle: `git checkout main` from a shell while the window keeps the
  focus changes the widget within 2 seconds (OBS-0022 Nachprüfung).

- **AK-8 (v5)** Git, headless (`crates/vcs/tests/it/branch_ops.rs`): tags peeled, recent branches newest
  first without the current and deleted ones, detached checkout of a tag, an option-like revision
  refused; delete refused while not merged and for the current branch, rename, checkout and rebase,
  `is_merged`.
- **AK-9 (v5)** Model and MCP: rows with actions, Recent, folders (open/closed), tracking hints, tags,
  filter over tags, submenus per kind of row, revision rows (`branches_model::tests`); `git_branches`
  with `tracking`/`recent`/`tags` and `git_branch_action` (`protocol.rs`: `git_branch_popup_tools`).
- **AK-10 (v5)** In a real window (`idecraft ui --selftest`): the popup starts with Update, lists the
  recent branch, → opens the current branch's submenu, Delete in a branch's submenu deletes it.
- **AK-11 (v5)** In the window next to the reference: look of the actions, folders, tracking and the
  submenu; the delete offer after a merge (window check after the parallel work, OBS-0022 L12, L20).

- **AK-12 (v6)** Submenus with Compare with, Show Diff with Working Tree and Push… per kind of row
  (`branches_model::tests`); `Repo::outgoing_of` for another branch and `push_with` of another
  branch (`crates/vcs/tests/it/log_actions.rs`); `git_branch_action` `compare` and `diff-worktree`
  (`protocol.rs`).
- **AK-13 (v7)** By hand in the window: the three actions show ⌘T, ⌘K, ⇧⌘K.

- **AK-14 (v8)** Store (`idecraft_core` `branch_favorites::tests`): `main`/`master` and
  `<remote>/main` by default, marking, unmarking a default and marking it again, local and remote
  apart, per repository, saved under `.idecraft/` and not in `.git`.
- **AK-15 (v8)** Rows (`branches_model::tests`): favourites after the current branch in Local and
  Remote, a folder moving up with its favourite, stars per row, filter order unchanged, the submenu's
  last entry Mark/Unmark as Favorite; both star icons bundled light and dark (`icons::tests`).
- **AK-16 (v8)** MCP (`protocol.rs`: `git_branch_favorites_tool`): `git_branches` with `favorite`,
  `git_branch_favorite` marks, unmarks and toggles; `git_branch_action` `rename` moves the mark.
- **AK-17 (v8)** In a real window (`idecraft ui --selftest`): Space on a branch marks it and its row
  moves up with the star; the submenu's Unmark as Favorite unmarks it again, the popup stays open.
- **AK-18 (v8)** In the window next to the reference (open, not observed yet): look and place of the
  star, hover, order with folders and while filtering, Space, the defaults `main`/`master` and the
  remote ones, the submenu entry.
- **AK-19 (v9)** The popup started by the two actions takes a name for a new branch and for the rename of a given local branch, and refuses to rename what is not a local branch (unit test `branches_view::tests::naming_started_by_the_actions`); the keys act only in the popup (`keymap_actions::tests::scoped_actions_run_first_and_only_in_their_focus`). In the window (open): ⌥⌘N, a name and Enter create and check out the branch.

## Out of scope
Upstream tracking configuration for branches created from remote branches, worktrees,
fetching from the popup, deleting remote branches. v8: showing only the favourites in the popup (the
Git log's branches pane can, SPEC-0086 v10), favourites of tags. v6: Compare with lists the commits in a popup
instead of a log tab of its own.

## Known deviations from the reference
A local branch created from a remote branch has no upstream configured yet. Not compared side by side.
The popup reads the branches each time it opens, so branches and commits made outside IDECraft
show up without a manual refresh (IMP-0038).
- v5: all folders start open and only the tags start closed; the reference started with its "Local"
  group closed (OBS-0009 item 1), which hides the branches one most often wants (IMP-0323).
- v5: Delete in the submenu and after a merge is the safe delete (`git branch -d`); a branch that is
  not merged is refused with Git's reason instead of offering a forced delete.
- v8: renaming a favourite carries the mark to the new name (IMP-0340); the reference's behaviour on
  rename is not observed. Favourites, their order and the submenu entry are specified from the public
  help and the plan, not yet compared side by side (AK-18).
