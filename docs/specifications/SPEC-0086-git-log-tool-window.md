# SPEC-0086: Git log tool window

- **Version:** 12 · **Date:** 2026-11-14 (v12: `VcsHistory.ShowAllAffected` lists every file of the selected commit, behaviour 21, AK-34; eighth package of docs/research/KEYMAP_COVERAGE.md; v11: Edit Commit Message and New Branch of the selected commit under their keymap ids, behaviour 20; docs/research/KEYMAP_COVERAGE.md; v10 2026-11-06: favourite branches in the branches pane — star, favourites first, Mark/Unmark as Favorite and Show Only Favorites in the action bar, Space, behaviour 19; planned by the project owner, partly not observed yet; v9: no counters on the HEAD row, behaviour 18; v2: commits of all branches and tags, the list follows
  changes to branches and HEAD; hand test 31, D14; v3: commit graph, the current branch marked at its
  commit, dates with time in the system's regional format, the subject uses the free width, nothing
  selected until the user chooses a commit; OBS-0013; v4: branches pane with folders and a filter,
  choosing a branch filters the log; OBS-0013; v5: filter bar with branch, author, date and path
  filters, regular expressions and case in the text filter; OBS-0013; v6: dates in the zone's rules, rows of the checked-out branch tinted; side by side in OBS-0022; v7: context menu on commits, labels placed and merged as in the reference, `*` for a different committer, branches pane with counters and an action bar; OBS-0022 L8, L10, L11, L21; v8: the rest of the reference's commit menu: Show Repository at Revision, Compare with Local, Fixup, Squash Into, Push All up to Here, the Checkout and Branch submenus; window comparison after #391–#393)
- **Sources:** GEN (the Version Control tool window with the commit history of the repository: commit
  list with branch and tag labels, author and date; a text filter; the selected commit's details and
  changed files; the diff of a changed file in that commit; F-074), OSS (action id
  `ActivateVersionControlToolWindow`, ⌘9 in the macOS keymap, Alt+9 in the default one), SPEC-0032
  (Git log), SPEC-0084 (diff viewer), OBS-0013 (side by side 2026-11-05: the reference draws the
  commit graph with branch lines, marks the current branch at its commit, shows date and time as the
  system's regional settings say, and selects nothing until a commit is chosen; on the left a tree of
  the branches with the current branch on top, local and remote branches, name prefixes as folders
  and a search field; a filter bar with text or hash, switches for regular expressions and case,
  and filters by branch, author, date and paths), STD (the regional
  date formats of the common locales, CLDR short date and time)
- **Feature matrix rows:** F-074

## Purpose
See what happened in the repository — who changed what, when and why — without leaving the IDE.

## Behaviour
1. **Open:** `ActivateVersionControlToolWindow` (⌘9 on macOS) opens the tool window "Git" with the
   "Log" tab at the bottom of the window; pressed again, or ⇧Esc, it closes. The history is read in the
   background; meanwhile the list says "Loading…". [GEN, OSS]
2. **Commit list:** commits reachable from HEAD, every local and remote branch and every tag (stashes
   left out), newest first, at most 1000, so a commit of another branch can be found by subject or hash
   and cherry-picked (SPEC-0106). [GEN] Each row shows, from left to right: the graph (behaviour 8),
   the subject, the labels of the branches, remote branches and tags pointing at the commit, the author
   and the date. The subject takes all the width the other columns leave and ends in … only when it
   does not fit; author and date are as wide as their texts need, up to a limit. (v3) [OBS-0013]
   - **Current branch:** its label comes first and is drawn emphasised (filled in the accent colour)
     at the commit it points at. A separate `HEAD` label appears only when HEAD is detached. (v3)
     [OBS-0013]
   - **Date:** "Today 14:05" and "Yesterday 09:12"; older commits show date and time in the short
     format of the system's regional settings — for example `05.11.26, 14:05` for German,
     `11/5/26, 2:05 PM` for US English, `05/11/2026, 14:05` for British English, `2026-11-05 14:05`
     for Swedish — with the 12- or 24-hour clock the region uses unless the system forces one. When
     the region is unknown the date is written `2026-11-05 14:05`. On macOS the region and the clock
     come from the system settings, elsewhere from `LC_ALL`, `LC_TIME` or `LANG`. (v3) [OBS-0013, STD]
   - **No preselection:** when the window opens no commit is selected; the details pane says to
     select a commit. Down or a click selects one; after a reload a selected commit stays selected
     when it is still listed, otherwise nothing is selected. (v3) [OBS-0013]
3. **Filter:** typing filters the list by subject, author, e-mail or the start of the hash (case
   ignored); Backspace removes, Escape clears the filter first and closes the window when it is empty.
   The field shows "Text or hash" while empty. Two switches at the field's end, also ⌥R and ⌥C,
   make the text a regular expression (searched in subject, author and e-mail; no hash match) and
   make case matter; an invalid expression lists nothing and the field says why. (v5) [OBS-0013]
   While the text, user, date or path filter hides commits, the graph shows the commits' dots
   without lines, since lines between the remaining commits would join commits that are not parent
   and child; the branch filter keeps the lines, since a branch's commits hold their parents.
   (v3, v5) [GEN]
4. **Details:** on the right, the selected commit's changed files (against its first parent; the first
   commit against nothing) with their change kind in the file-status colours, and below them the hash,
   author with e-mail, date, parents and the full message. Up/Down or a click choose a commit. [GEN]
5. **Diff:** Tab moves between the commit list and the file list. Double-click on a file, or Enter / ⌘D
   with a file selected, shows that file before and after the commit side by side over the editor area
   (SPEC-0084: F7/⇧F7, Escape returns to the tool window). [GEN]
6. **Up to date:** the list is read again when the IDE itself commits, rebases, cherry-picks, stashes
   or checks out, and when a branch, tag or HEAD moves outside the IDE (looked at every 2 seconds while
   the window is open). The filter and the selected commit stay when the commit is still there.
   [GEN, IMP-0282]
7. Agents use `git_log` (SPEC-0032; `all` lists every branch and tag as the window does) and the new `git_show`: the details and changed files of one commit,
   optionally with the side-by-side diff of one file. Every commit `git_log` returns carries the
   full ids of its `parents`, so an agent can follow branches and merges as the graph draws them;
   `git_branches` names the current branch. (v3) [ADR-0008]
8. **Graph:** a narrow column left of the subject. Every commit is a dot in a vertical lane; a line
   joins each commit to each of its parents. A commit keeps the lane of the line that reaches it from
   above (the leftmost when several lines meet it — a fork point); its first parent continues in the
   same lane, and every further parent of a merge commit gets a line to a lane of its own (or to the
   lane already waiting for that parent). A commit no line reaches — the tip of a branch — takes the
   leftmost free lane; a lane whose line has ended is free for the next tip. Lines change lanes only
   in the row of a commit, so lines passing a commit stay straight. Each line has a colour of its own
   from a fixed set of the theme, which it keeps down to its end; the first parent's line keeps the
   colour of the commit. Lines to parents beyond the 1000 listed commits run to the bottom of the
   list. (v3) [GEN, OBS-0013]
9. **Branches pane:** on the left of the commit list, a tree of the repository's branches: first the
   current branch (or `HEAD` while detached), then a folder "Local" with the local branches and a
   folder "Remote" with one folder per remote and its branches. Parts of a name before a `/` become
   folders (`feature/discount` is `discount` in the folder `feature`); folders come before branches,
   each sorted by name. A folder opens and closes with a click on it or with Right/Left. A field at
   the top of the pane filters the tree: only branches whose full name contains the text (case
   ignored) stay, with their folders open. (v4) [OBS-0013]
   - **Filtering the log:** choosing a branch (click, or Enter on it) shows only the commits reachable
     from it, with their graph; the chosen branch is highlighted and named above the commit list with
     a button to remove the filter. Choosing it again, the remove button, or Escape in the commit
     list (after the text filter is empty) removes the filter. The text filter applies on top of the
     branch filter. (v4) [OBS-0013]
   - **Keys:** Tab moves from the commit list to the file list (when the commit has files) and on to
     the branches pane, then back to the commit list. In the pane Up/Down move, Enter chooses, typing
     goes to its filter field, Backspace removes, Escape clears the field first and then returns to
     the commit list. (v4) [GEN]
   - `git_log` takes a `branch` (local or remote branch, tag or any revision): the commits reachable
     from it, as the pane's filter shows them. (v4) [ADR-0008]
10. **Filter bar:** next to the text field, buttons for Branch, User, Date and Paths, each naming
    its current choice. A click opens a short list under the button; Up/Down and Enter or a click
    choose, typing narrows the list, Escape closes it. All filters apply together, the branch filter
    being the one of the branches pane. (v5) [OBS-0013]
    - **Branch:** "All" and every branch of the pane.
    - **User:** "All" and the authors of the listed history, most commits first; one author at a
      time.
    - **Date:** all, the last 24 hours, 7, 30 or 365 days.
    - **Paths:** the typed text, a path relative to the repository root, is added with Enter; a file
      keeps the commits that change it, a folder those that change anything below it. A commit
      changes a path when the path's content differs from every parent's (a merge counts only when
      it differs from all parents; the first commit when the path exists). Added paths are listed
      with a button to remove each; "Clear" removes all. The commits that change the paths are read in
      the background; the list says "Filtering…" meanwhile.
    - A button with a cross clears every filter at once; it is shown only while one is active.
    - `git_log` takes `author`, `since`/`until` (seconds since the epoch), `paths`, `text` with
      `regex` and `case_sensitive`, filtering as the bar does. (v5) [ADR-0008]

11. **v6 – Time zone rules:** dates and times (list, details, the annotation column of SPEC-0277) use
    the system time zone's offset *at the commit's time*, so a commit made in summer keeps its summer
    time when it is read in winter (`05.10.26, 11:30` for 11:30 CEST, also after the change to CET).
    Without zone rules the current offset is used. [STD, OBS-0022 L1]
12. **v6 – Checked-out branch:** the rows of the commits reachable from HEAD get a light tint of the
    inactive selection colour, so the current branch's history stands out from other branches'
    commits; the selected row keeps the selection colour. [OBS-0022 L9]
13. **v7 – Commit context menu:** a secondary click on a commit (right button, two-finger click,
    ⌃-click) selects it and opens a menu at the pointer; ⇧F10 opens it for the selected commit.
    Up/Down move through all entries, Enter or → chooses, Escape closes; a click outside closes it.
    The entries, in groups separated by lines: [OBS-0022 L8, GEN, STD]
    1. Copy Revision Number (⌥⇧⌘C) — the full id to the clipboard; Copy as Patch — the commit as
       `git format-patch` text to the clipboard.
    2. Cherry-Pick (⌥↩, SPEC-0106); Checkout Revision — `git checkout --detach`, Git refuses when
       local changes would be overwritten; Show Diff (⌘D) — the diff of the commit's first changed
       file (behaviour 5).
    3. Reset Current Branch to Here… — a second page lists Soft, Mixed, Hard and Keep with what each
       does; Revert Commit — `git revert --no-edit`; Undo Commit — the latest commit goes back,
       its changes stay staged (`git reset --soft HEAD~1`).
    4. Edit Commit Message… — the interactive rebase dialog (SPEC-0145) from the commit's parent
       opens with this commit set to reword and its message field open; Drop Commit — an
       interactive rebase from the parent with this commit dropped; Interactively Rebase from
       Here… (⌥I).
    5. New Branch…, New Tag… — a second page with a name field; Enter creates the branch (not
       checked out) or the lightweight tag at the commit, Escape goes back; an invalid or existing
       name is refused under the field.
    6. Go to Child Commit (←), Go to Parent Commit (→) — select the newest listed child or the
       first parent.

    **When entries are disabled:** Cherry-Pick for a commit already on the current branch or a
    merge commit; Reset while HEAD is detached or at HEAD's commit; Revert for a commit not on the
    current branch or a merge commit; Undo unless the commit is HEAD's on a branch, not pushed, no
    merge and not the first commit; Edit Commit Message, Drop and Interactively Rebase from Here for
    a commit not on the current branch, pushed (some remote-tracking branch contains it), a merge
    commit, a commit followed by merges on the branch, or the first commit; the Go to entries
    without a child or parent. A disabled entry is drawn dim and, when highlighted or chosen,
    the menu's last line says why (IMP-0278); choosing it does nothing else. *Deviation:* the
    reference greys these entries without a reason (OBS-0022).

    **Hard reset** first keeps the uncommitted changes of tracked files in a stash
    "Ferris IDEA: before hard reset to <id>" and says so in the status bar (IMP-0320). A **revert
    that conflicts** is aborted, so nothing changes, and the status bar names the files.
    The Git operations run in the background and report in the status bar like SPEC-0106; the log
    reads the history again afterwards (behaviour 6).

    `git_commit_action` lists the entries for a commit with `enabled`, `reason` and the facts
    behind them, and runs copy-patch, cherry-pick, checkout, reset (`mode`), revert, undo,
    edit-message (`message`), drop, new-branch and new-tag (`name`); disabled entries are refused
    with the same reason. [ADR-0008]
14. **v7 – Labels:** the labels of a commit stand right-aligned between the subject and the
    author; the subject takes the rest of the width and ends in … first. A local branch and a
    remote-tracking branch of the same name on the same commit are one label `origin & main`.
    Order: `HEAD` while detached, the current branch (filled in the accent colour), other local
    branches, remote branches (dimmer), tags. A tag is drawn with a tag symbol and its name
    (IMP-0321; the reference shows the symbol alone). [OBS-0022 L10]
15. **v7 – Committer:** when the committer differs from the author (name, or e-mail ignoring
    case), the author column shows `*` after the name and the details say "committed by Name
    <email>". `git_log` and `git_show` carry `committer` for such commits. [OBS-0022 L21, STD]
16. **v7 – Branches pane:** the first row is "HEAD (Current Branch)" (choosing it filters by the
    current branch); a local branch with an upstream shows `↙n` for the upstream's commits it lacks
    (incoming) and `↗n` for its own the upstream lacks (outgoing), as of the last fetch, hidden
    when 0. A vertical action bar left of the tree has New Branch… (a field at the top of the
    pane names the branch, made at the highlighted branch or HEAD; Enter creates, Escape
    cancels), Checkout Selected Branch (local branches; local changes in the way lead to the smart
    checkout question of SPEC-0231), Fetch, Expand All and Collapse All, each with a tooltip.
    `git_remotes` returns the counters. [OBS-0022 L11]

17. **v8 – The rest of the commit menu:** the menu of behaviour 13 gets the reference's other
    entries; the groups become [OBS-0022 L8, GEN, STD]:
    1. Copy Revision Number, Copy as Patch, Cherry-Pick.
    2. Checkout Revision — or, when local branches point at the commit, **Checkout ›**, a page
       listing each of those branches and "Revision '<short id>'" (a branch is checked out like
       the branches pane's action, the revision detached); Show Repository at Revision — a list
       of the files at the commit, Enter shows the chosen file's text at the commit read-only (a
       viewer with that revision on both sides, so it shows no differences); Compare with Local —
       the files that differ between the commit and the working tree (`git diff --name-status`),
       Enter opens the diff viewer of SPEC-0084 v2 with the commit on the left and the working
       copy on the right, read-only (no `»`); Show Diff.
    3. Reset Current Branch to Here…, Revert Commit, Undo Commit.
    4. Edit Commit Message…, **Fixup…**, **Squash Into…**, Drop Commit, Interactively Rebase from
       Here…, **Push All up to Here…**. Fixup… and Squash Into… open the Commit tool window with the
       message `fixup! <subject>` or `squash! <subject>` of the commit, so the next commit of the
       chosen files is folded into it by a later `git rebase --autosquash` (what `git commit
       --fixup` and `--squash` write); they have the rules of Edit Commit Message (on the current
       branch, not pushed, no merge, no merges after it, not the first commit). Push All up to
       Here… opens the push popup of SPEC-0109 with only that commit and the older unpushed ones
       listed and pushes `<id>:refs/heads/<target>`; it is for a commit on the current branch that
       is not pushed, while HEAD is on a branch.
    5. **Branch '<name>' ›** for each branch label at the commit except the current branch (local
       before remote), then New Branch…, New Tag…. The branch page lists the entries of that
       branch's submenu in the branch popup (SPEC-0068 v5 behaviour 20, v6) and does the same;
       New Branch from… and Rename… use the menu's name page.
    6. Go to Child Commit, Go to Parent Commit.
    A page opens with → or Enter and closes with ← or Escape. `git_commit_action` lists the new
    entries (`show-repository`, `compare-local`, `fixup`, `squash`, `push-up-to`) with `enabled`
    and `reason`, the branch submenus as `branches` (name, remote) and the checkout choices as
    `checkout` (branch names); it runs `show-repository` (returns the files, or with `path` the
    text), `compare-local` (returns the changed files, or with `path` the diff rows), `fixup` and
    `squash` (commit `paths` with the message; `dry_run` as `git_commit`) and `push-up-to` (pushes
    to the upstream branch, or `name` as target, needs `vcs-write`). [ADR-0008]
18. **v9 – HEAD row:** the "HEAD (Current Branch)" row of the branches pane shows no ↙/↗ counters (the current branch's own row has them, and the reference leaves them out there); they cut the row's name. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L11]

19. **v10 – Favourites in the branches pane:** the pane uses the favourites of SPEC-0068 v8
    behaviour 25 (the same store, per repository, local and remote apart, `main`/`master` by default).
    [DOC (JetBrains help, Log tab: favourites on top of the list, a star button in the pane's
    toolbar), GEN]
    - **Star:** a favourite branch's row shows a filled star in the slot left of its name (where a
      folder has its chevron); the other branch rows show an outline star there while highlighted
      or under the pointer. A click on the star marks or unmarks without filtering the log. The
      "HEAD (Current Branch)" row shows the star of the current branch.
    - **Order:** in every folder the favourite branches come first, in name order, then the
      sub-folders, then the other branches. [DOC; the place of the favourites against the
      sub-folders is not observed, check in the next side-by-side comparison]
    - **Action bar:** a star button, **Mark/Unmark as Favorite**, toggles the highlighted branch
      (its tooltip names the action); a second button, **Show Only Favorites**, keeps only the
      favourite branches in the tree (the HEAD row stays, the folders on their way are open) and is
      drawn pressed while on; the pane's filter field applies on top. [DOC for the star button;
      Show Only Favorites: not observed — the help instead names a favourites choice in the Branch
      filter of the filter bar — check in the next side-by-side comparison]
    - **Keys:** Space in the pane marks or unmarks the highlighted branch (branch names cannot
      contain spaces, so Space is never typed into the pane's filter). [GEN, as SPEC-0068 v8
      behaviour 28]
    - The commit menu's Branch pages (behaviour 17) end with Mark/Unmark as Favorite, as the branch
      popup's submenu does. `git_branches` carries `favorite` and `git_branch_favorite` sets it
      (SPEC-0068 v8 behaviour 30); the pane reads the store again with every reload of the history.
      [ADR-0008]
20. **v11 – Actions of the selected commit:** while the commit list has the focus and a commit is selected, `Git.Reword.Commit` (F2, ⇧F6) does what Edit Commit Message… of the commit menu does (behaviours 13 and 17: the interactive rebase dialog from the commit's parent with the commit set to reword, nothing rewritten before the dialog is confirmed), and `Git.New.Branch.In.Log` (⌥⌘N on macOS) what the menu's New Branch… does (the name page). They go through the menu: it opens at the commit, and an entry the menu shows disabled stays disabled and shows its reason there. Elsewhere the keys keep their other actions; from Find Action the two run on the selected commit while the Git tool window is shown. [OSS: ids and shortcuts of the bundled keymaps; GEN]
21. **v12 – All files of the commit:** while the commit list has the focus and a commit is
    selected, `VcsHistory.ShowAllAffected` (⌃⌘A in the bundled macOS keymap, ⌥⇧A in the default
    one) lists every file the commit changed against its first parent — also when the log is
    filtered to a path, as for the file history of `Vcs.ShowTabbedFileHistory` — one row per file
    with the change letter and the repository-relative path, titled
    with the short id and the subject. Enter opens the diff of that file before and after the
    commit (behaviour 5); Escape closes the list. A commit without changed files says so in the
    status bar. Elsewhere the key does nothing. MCP: `git_show` returns the commit's files. [OSS:
    id and keys of the bundled keymaps; GEN]

## Acceptance criteria
- **AK-1** Date texts, the filter and the selection after filtering (unit tests of `git_log_model`).
- **AK-2** `Repo::commit_details`, `Repo::ref_labels` and `Repo::side_by_side_in_commit` on a repository
  with a root commit, a later commit, a branch and a tag (integration test).
- **AK-3** `git_show` returns details, changes and the diff of one file (protocol test).
- **AK-4** In a real window (`ferris-idea ui --selftest`): ⌘9 lists the fixture's commits newest first with
  no commit selected and the hint in the details pane (v3); Down shows the details of the newest; the
  graph has a dot in a lane for every listed commit (v3); filtering keeps the matching commit; the diff of a changed file opens and
  Escape returns; a commit made outside on another branch appears while the window is open; ⌘9 closes.
- **AK-6** `Repo::log_all` lists other branches and tags but no stash (`crates/vcs/tests/it/git.rs`:
  `full_history_lists_other_branches_and_tags`); `git_log` with `all` (protocol test
  `git_tools_on_a_scratch_repository`).
- **AK-5** Use by hand next to the reference.
- **AK-7** Graph layout (unit tests of `git_graph`): a straight history in one lane; a branch with
  its own commit forks in a second lane and joins its fork point; a merge commit draws a line to the
  lane of its second parent; a lane freed by an ended line is used by the next branch tip; lines
  passing a commit stay in their lane; parents outside the list run to the bottom. (v3)
- **AK-8** Dates (unit tests of `git_log_model`): today, yesterday and older dates in German, US
  English, British English, Swedish and unknown regions; forced 12- and 24-hour clocks; the region
  read from `AppleLocale` (with a region override) and from `LANG`. (v3)
- **AK-9** `Repo::ref_labels` has no `HEAD` label when HEAD is a branch and one when it is detached;
  `git_log` lists `parents` (integration and protocol tests). (v3)
- **AK-10** Branches tree (unit tests of `git_branch_tree`): current branch first, local and remote
  folders, `/` prefixes as folders, folders before branches, closing a folder, the filter keeping the
  matching branches with their folders; the commits reachable from a branch (unit tests of
  `git_log_model`). (v4)
- **AK-11** `git_log` with `branch` lists only that branch's commits (protocol test). (v4)
- **AK-12** In a real window: the branches pane lists the current branch on top and `log-side` under
  Local; choosing `log-side` lists only its commits; choosing it again lists all. (v4)
- **AK-13** Filters (unit tests of `git_log_model`): text plain and as a regular expression, with
  and without case, an invalid expression; author; the date ranges; branch and paths combined with
  the others; the list of authors. (v5)
- **AK-14** `Repo::commits_touching` keeps the commits that change a file or anything below a folder,
  a merge only when it differs from all parents (integration test); `git_log` with `author`, `since`,
  `paths`, `text`, `regex` (protocol test). (v5)
- **AK-15** In a real window: ⌥R and ⌥C switch the text filter's modes; a regular expression filters;
  the user filter keeps the fixture author's commits; a path filter on a file keeps the commits that
  change it; clearing all filters lists everything again. (v5)

- **AK-16 (v6)** A commit of 2026-10-05 11:30 CEST read on 2026-11-06 with the zone Europe/Berlin shows
  `05.10.26, 11:30`; a fixed zone keeps its offset (`git_log_model::tests::
  summer_times_keep_their_offset_in_winter`).
- **AK-17 (v6)** In the release bundle next to the reference: the rows of HEAD's history are tinted
  (OBS-0022 Nachprüfung).
- **AK-18 (v7)** Menu rules (unit tests `ferris_idea_vcs` `log_actions::tests`): every entry once in
  order with ids that round-trip; HEAD unpushed allows all but Cherry-Pick, Reset and Go to Child;
  pushed commits refuse rewriting with a reason; other branches allow Cherry-Pick only; merges
  after the commit, the first commit and a detached HEAD. Facts from the loaded history
  (`git_log_marks::tests::facts_from_the_loaded_history`).
- **AK-19 (v7)** Operations (`crates/vcs/tests/it/log_actions.rs`): facts of HEAD, a pushed commit,
  the first commit and another branch; hard reset keeps a stash, soft keeps the change staged, a
  clean hard reset makes none; reword, drop, undo, revert; a conflicting revert changes nothing;
  tag, patch of the first commit, detached checkout, reset refused while detached.
- **AK-20 (v7)** `git_commit_action` lists entries with reasons, refuses a disabled entry with the
  reason, runs new-tag, edit-message, copy-patch and reset; unknown actions and a missing name are
  refused (protocol test `git_commit_action_lists_and_runs_the_commit_menu`).
- **AK-21 (v7)** Labels (`git_log_marks::tests`): local and remote branch of one name on one commit
  are one label, order current, local, remote, tag; HEAD first while detached; counters text
  (`counters_name_incoming_then_outgoing`); the current row "HEAD (Current Branch)" (unit tests of
  `git_branch_tree`).
- **AK-22 (v7)** Committer: `CommitInfo::committer` only when it differs (`crates/vcs/tests/
  log_actions.rs`: `a_different_committer_is_reported`); `*` in the author column
  (`git_log_marks::tests::a_different_committer_gets_a_star`).
- **AK-23 (v7)** Window self-test: ⇧F10 opens the menu of the selected commit; Cherry-Pick is
  disabled with its reason; New Tag… with a name creates the tag and its label appears; the first
  branches row is "HEAD (Current Branch)"; New Branch… from the action bar creates a branch.
- **AK-24 (v7)** In the release bundle next to the reference: the menu on commits of the current
  branch, another branch and a pushed commit; labels and counters (window check open).

- **AK-25 (v8)** Menu rules: the new entries in their groups and order, Fixup/Squash/Push up to
  Here refused for pushed commits and other branches with a reason (unit tests
  `log_actions::tests`).
- **AK-26 (v8)** Operations (`crates/vcs/tests/it/log_actions.rs`): files at a revision, files that
  differ from the working tree, `fixup!`/`squash!` commits of chosen paths, push up to a commit
  leaves the newer commit unpushed.
- **AK-27 (v8)** `git_commit_action` lists the new entries and branch submenus and runs
  show-repository, compare-local and fixup (protocol test).
- **AK-28 (v8)** Menu pages (unit tests of `git_log_view` model helpers): the Checkout page lists
  the local branches at the commit and the revision; Branch pages for every label but the current
  branch.
- **AK-29 (v9)** By hand in the window: the HEAD row reads "HEAD (Current Branch)" in full.

- **AK-30 (v10)** Tree (unit tests of `git_branch_tree`): favourites first in each folder, the star
  flag per row, Show Only Favorites keeping the HEAD row and the favourites with their folders open,
  together with the text filter.
- **AK-31 (v10)** Window self-test: the star button of the action bar marks the highlighted branch,
  Show Only Favorites then lists it and hides the other branches; the Branch page of the commit menu
  ends with Mark/Unmark as Favorite (unit test `branch_page_lists_the_popups_entries`).
- **AK-32 (v10)** In the window next to the reference (open, not observed yet): star, order, the
  action bar's buttons and Space.
- **AK-33 (v11)** The two ids map to the menu's entries and run first and only in the log's focus (unit tests `keymap_actions::tests::log_actions_name_their_menu_entry`, `keymap_actions::tests::scoped_actions_run_first_and_only_in_their_focus`). In the window (open): F2 on an unpushed commit opens the rebase dialog with its message field.
- **AK-34 (v12, behaviour 21)** Rows of a change list, renames and an empty list
  (`keymap_actions::tests::affected_rows_name_letter_and_path`); in a window of the test harness
  with a repository of two commits and the log filtered to one file, the list holds both files
  of the selected commit and Enter opens the diff of the chosen one
  (`eighth_package_keymap.rs::all_files_of_a_commit_are_listed`).

## Out of scope
Several authors at once, dates typed by hand, picking paths from the project tree, merging,
deleting and renaming branches in the pane (the branch popup does that), tags in
the pane, v10: a favourites choice in the filter bar's Branch filter (DOC) and the reference's
Show My Branches, more than 1000 commits, the history of a file or selection, the Console tab. From the
reference's commit menu: selecting several commits (Squash Commits, Drop Commits of several) and
a tree view of the repository at a revision (v8 lists its files instead).

## Known deviations from the reference
The graph's lanes are laid out by the rules
of behaviour 8, so line shapes can differ from the reference's in histories with many crossing
branches. The regional date format is taken from a table of common regions, not from the
system's full locale data; rarer regions fall back to `2026-11-05 14:05`. The reference does not see commits and branches made
outside until its refresh button is pressed (OBS-0009); Ferris IDEA reads them by itself (IMP-0282).
v7: Create Patch… copies the patch to the clipboard (Copy as Patch) instead of asking for a file;
disabled menu entries name their reason (IMP-0278); a hard reset keeps the local changes in a
stash (IMP-0320); a conflicting revert is aborted instead of stopping with conflicts; tag labels
show the name next to the symbol (IMP-0321); the action bar has fewer buttons than the
reference's. v8: Show Repository at Revision lists the files of the revision instead of opening a
project tree at it; Fixup… and Squash Into… prefill the Commit window's message and leave the choice of files there. v10: the pane's Show Only Favorites switch is
specified from the plan and not observed in the reference, whose help names a favourites choice in
the Branch filter instead (AK-32).
