# SPEC-0335: Changelists

- **Version:** 2 · **Date:** 2026-11-14 (v2: a name field that opens with a name has it selected, the paragraph before behaviour 8 and AK-11; window check of 2026-11-14)
- **Sources:** OSS — the open platform's changelists, `JetBrains/intellij-community` at the pinned
  commit `d4018fb` of APACHE_PORTING.md (PROVENANCE.md: logic template, no code):
  `platform/vcs-impl/src/com/intellij/openapi/vcs/changes/actions/AbstractChangeListAction.kt`
  (the list an action acts on: the selected list, else the list of the selected changes, and only
  when that is exactly one), `AddChangeListAction.kt`, `RenameChangeListAction.kt`,
  `SetDefaultChangeListAction.kt` (not for the list that is active already),
  `RemoveChangeListAction.kt` (which lists may go, when a question is asked, what happens to the
  active list and to the last list), `MoveChangesToAnotherListAction.kt` (what is moved, which
  lists are offered, which one is preselected),
  `platform/vcs-impl/src/com/intellij/openapi/vcs/changes/ui/NewEditChangelistPanel.kt` (a name is
  not blank and not taken; the name a new list is offered; the option to make the list active),
  `NewChangelistDialog.java`, `EditChangelistDialog.java` (the option is ticked and fixed for the
  active list), `ChangeListChooser.java` and `ChangeListChooserPanel.java` (the chooser takes an
  existing list or a new name),
  `platform/vcs-impl/src/com/intellij/openapi/vcs/changes/ChangeListWorker.java` (one list is the
  default one and always exists; a new change joins it; a removed list's changes join it; the
  default list is never removed; a change keeps the list of its path before or after a rename;
  names are compared exactly), `ChangeListManagerImpl.kt`, `ChangeListUtil.kt` (a free name is
  made from the offered one), `actions/ChangeListRemoveConfirmation.kt`,
  `platform/vcs-impl/shared/src/com/intellij/openapi/vcs/changes/ui/ChangesBrowserChangeListNode.java`
  (the active list's name is bold, a count follows the name),
  `platform/vcs-api/shared/src/com/intellij/openapi/vcs/changes/LocalChangeList.java` and
  `platform/vcs-api/vcs-api-core/resources/messages/VcsBundle.properties` (the default name
  `Changes`, the offered name `New changelist`), the action ids in
  `platform/vcs-impl/resources/META-INF/VcsActions.xml`, the action texts in
  `platform/platform-resources-en/src/messages/ActionsBundle.properties`, and the shortcuts of the
  bundled keymaps (`crates/ui/keymaps/`). GEN (a name is trimmed and has a largest length; a
  broken state file never stops the program). STD (Git: a rename in the index). SPEC-0063 (Commit
  tool window), SPEC-0069 behaviour 3 (machine-local state), SPEC-0303 and ADR-0021 (file
  formats), ADR-0008 (MCP), SPEC-0053 (agent rights), SPEC-0281 (dialog decisions), SPEC-0319
  (effects of actions), SPEC-0318 (remote projects).
- **Feature matrix rows:** F-074

## Purpose
The user sorts the changed files of a project into named changelists, so that changes that
belong together stand together and are committed together. A changelist is a thing of the IDE:
Git knows nothing of it, and nothing is written into the repository.

## Behaviour

### The lists
1. **Lists.** A project has one or more changelists, each with a name. Exactly one of them is the
   active list. A project starts with one list named `Changes`, which is active. [OSS:
   `ChangeListWorker.ensureDefaultListExists`, `LocalChangeList.getDefaultName`]
2. **Membership.** Every changed file belongs to exactly one list. Changed files are the files of
   the group Changes of SPEC-0063: tracked files that differ from HEAD, files added to the index,
   and files with unsaved changes in an open editor. A file whose change Ferris IDEA sees for the
   first time joins the active list. It stays in its list when another list becomes active.
   Unversioned files belong to no list; they stay in their own group. [OSS:
   `ChangeListWorker.addChangeToCorrespondingList`]
3. **Leaving.** A file whose change is gone — committed, rolled back, or reverted by another
   program — leaves its list. A list that becomes empty stays until it is deleted. [OSS, GEN]
4. **Renamed files.** A file Git reports as renamed (the rename is in the index, as after
   `git mv`) keeps the list of its old path. A rename that is not in the index is, for Git and
   for this window, a deleted file and an unversioned one: the deleted path keeps its list, the
   new file belongs to none until Git tracks it. [OSS: `guessChangeListByPaths`; STD]
5. **Names.** A name is taken without the white space around it. It has 1 to 100 characters and
   no control character. No two lists have the same name; names that differ only in capitals are
   different names. [OSS: blank and taken names are refused, names are compared exactly; GEN:
   trimming, the length, control characters]
6. **How many.** A project has at most 100 lists. [GEN]
7. **Order.** Lists are shown and reported by name, capitals ignored; two names that differ only
   in capitals stand in the order of their characters. [GEN]

### Actions
The actions carry the ids and titles of the open platform. Each one that needs input shows a
strip inside the Commit tool window, above the Amend switch: Enter confirms, Escape cancels, and
only one strip is open at a time. A name the rules refuse keeps the strip open and says why under
the field.

**The name in the field is selected (v2).** A name field that opens with a name — the free name
of behaviour 8, the list's name of behaviour 9, the preselected list of behaviour 12 — has the
whole name selected with the caret at its end, and so has a name that Up, Down or a click on a
row puts into the field (behaviour 12). Typing replaces the name; End or an arrow key drops the
selection and keeps the name. Before v2 the name stood unselected with the caret in front of it,
so typed text went before it (window check of 2026-11-14: typing `wip` made a list
`wipNew changelist`). The same holds for every other inline name field of the application that
opens with a proposed or a current name; the name fields of the project tree keep their rule of
selecting the name without the extension (SPEC-0059). [GEN]

8. **New Changelist…** (`ChangesView.NewChangeList`). The strip has a name field and the option
   "Set active". The field holds a free name: `New changelist`, or with `-2`, `-3` … behind it
   when that is taken. Confirming creates an empty list, and makes it active when the option is
   ticked. The option keeps its state for the window's lifetime; it starts unticked. No file is
   moved. [OSS: `AddChangeListAction`, `NewEditChangelistPanel`]
9. **Edit Changelist…** (`ChangesView.Rename`). Acts on the selected list, else on the list of
   the selected file. The strip has the name field with the list's name and the option "Set
   active", ticked and fixed for the active list. Confirming renames the list and, with the
   option, makes it active; its files stay in it. The unchanged name without the option changes
   nothing. [OSS: `RenameChangeListAction`, `EditChangelistDialog`]
10. **Set Active Changelist** (`ChangesView.SetDefault`). Acts on the selected list, else on the
    list of the selected file, and makes it the active list at once. For the list that is active
    already it does nothing and the status bar says that it is the active list. [OSS:
    `SetDefaultChangeListAction`, `ChangeListWorker.setDefaultList`]
11. **Delete Changelist** (`ChangesView.RemoveChangeList`). Acts on the selected list. [OSS:
    `RemoveChangeListAction`, `ChangeListWorker.removeChangeList`]
    - A list that is not active and has no files is deleted at once.
    - A list that is not active and has files: the strip asks, naming the list and the number of
      files; confirming deletes the list and its files join the active list.
    - The active list, while other lists exist: another list has to become active, and the files
      of the deleted list join it. With exactly one other list and no files in the deleted one
      that list becomes active without a question. Otherwise the strip lists the other lists, the
      first selected; Up and Down choose, Enter deletes.
    - The active list, while it is the only list: the project keeps a list, the one named
      `Changes`. A list of another name is replaced by a new active list `Changes`, which takes
      its files. The list `Changes` itself stays as it is, and the status bar says that the last
      changelist stays. No question is asked.
12. **Move to Another Changelist…** (`ChangesView.Move`). Acts on the selected file, or on all
    files of the selected list. Without such a file — nothing selected, an unversioned file, an
    empty list — the status bar says that nothing is selected that can be moved. The strip has a
    name field, the lists to choose from and the option "Set active" (as in behaviour 8). Offered
    are all lists but the one the files are in; when there is no other list, the active list is
    offered. The field holds the preselected list: the active list if it is offered, else the
    first empty one, else the first. Up and Down, or a click on a row, put a list's name into the
    field. Confirming with the name of a list moves the files there; a name no list has creates
    that list first (the strip marks such a name as new). With the option the target becomes
    active. Files already in the target stay where they are. [OSS:
    `MoveChangesToAnotherListAction.askTargetChangelist`, `guessPreferredList`,
    `ChangeListChooserPanel.getSelectedList`]

### In the Commit tool window
13. **Tree.** In place of the group Changes of SPEC-0063 behaviour 1 the list shows one group per
    changelist, in the order of behaviour 7. A group's row has a chevron, a checkbox, the name —
    in bold for the active list — and the number of its files. A list without files is shown too.
    The one exception is a project that has only the list `Changes` and no changed file: nothing
    is shown for it, as before. Group By (SPEC-0063 behaviour 11) nests the files inside each
    list; folding, Expand All and Collapse All treat a changelist as the group Changes was
    treated. [OSS: `ChangesBrowserChangeListNode.render`; GEN]
14. **Choosing.** The checkbox of a changelist checks all its files, or unchecks them when all
    are checked. A click on the row selects the list, shown highlighted, and takes the keyboard
    focus into the list of files; the chevron folds the list. Selecting a list unselects the
    selected file and the other way round. (Before, a click anywhere on the row of Changes
    checked its files — SPEC-0063 behaviour 3; that now needs the checkbox. The row of
    Unversioned Files is unchanged.) [GEN]
15. **Commit.** Commit acts on the checked files, whatever lists they are in. After a commit the
    committed files leave their lists (behaviour 3); the lists stay. [GEN]
16. **Keys.** From the bundled keymaps: Move to Another Changelist… is ⇧⌘M on macOS and
    Alt+Shift+M elsewhere, Edit Changelist… is F2 or Shift+F6, Set Active Changelist is
    Ctrl+Space in both keymaps. New Changelist… and Delete Changelist have no key. The keys act
    only while the Commit tool window has the keyboard focus; elsewhere they keep their other
    actions. From Find Action all five run while the window is shown, titled as above in the
    group "Commit". The window's toolbar has a button Changelists that opens a menu with the
    five actions, so each is reachable with the mouse; it is not shown in a remote project.
    [OSS: keymaps, `ActionsBundle.properties`; GEN: the button]
17. **Seeing changes.** The window reads the lists when it reads the status (SPEC-0063
    behaviours 4 and 18), and within 2 seconds after the state file was changed by another
    program, such as the MCP tool. [GEN]
18. **Remote project.** In a remote project (SPEC-0318) there are no changelists yet: the window
    shows the group Changes as before and each action says in the status bar that changelists
    are not available there. [GEN]

### Storage
19. **File.** `.ferris-idea/changelists.toml` in the project, of the same family as the other state
    one user keeps in a project (`version` first):

    ```toml
    version = 1
    active = "Checkout"

    [[list]]
    name = "Changes"
    files = ["README.md"]

    [[list]]
    name = "Checkout"
    files = ["src/cart.rs", "src/total.rs"]
    ```

    `files` are paths relative to the repository root with `/`. The file is machine-local state
    of one user: it is listed in the generated `.ferris-idea/.gitignore` (SPEC-0069 behaviour 3),
    which is written or refreshed before the file is first written. It is replaced as a whole
    through a temporary file. It is written when there is something to keep — more than the one
    list `Changes` — or when the file exists already; a project that never used changelists gets
    no file. [GEN, ADR-0021]
20. **Reading.** A missing file is the one list `Changes`. From a readable file, what breaks the
    rules is left out rather than trusted: a list whose name the rules refuse or that repeats a
    name, lists beyond the hundredth, a path that is empty, absolute, has a `..` or `.` part, a
    backslash or a control character, or is longer than 4096 bytes, and a path that an earlier
    list holds already. Without a usable list the project has the list `Changes`. An `active`
    that names no list makes the first list active. A path in the file is only ever compared
    with the paths Git reports: no file is opened, created or deleted because of it, and a path
    whose file has no change leaves the list at the next look. [GEN, Security: CLAUDE.md rule 12]
21. **A file that cannot be used.** A file over 2 MiB, broken TOML, a `version` other than 1, or
    a link or folder under the file's name is not used: the project shows the one list `Changes`
    and the window says once, in the line under the message field where it shows errors, that the
    file was not read and why, never quoting it. Looking at the status does not replace such a
    file. A changing action replaces a broken or too large
    file with a good one. A file of a newer version and a link or folder are never written over
    or through: the changing actions are refused and say why in the status bar. [GEN, Security]

### MCP
22. **Status.** `git_status` reports for each path its `changelist`: the name of its list, the
    active list for a changed file no list holds yet, and null for an unversioned file. Reading
    the status writes nothing. [ADR-0008]
23. **`changelists`.** Actions: `list` (default) returns `file`, `active`, `lists` (each with
    `name`, `active` and `files`), `problem` (why the file was not used, else null) and `limits`.
    `create` takes `name` and `set_active`; `rename` takes `name` and `new_name`; `set_active`
    takes `name`; `delete` takes `name` and, for the active list while others exist, `new_active`
    (not needed when exactly one other list exists and the deleted one has no files); `move`
    takes `paths`, `to` and `create` (make the list `to` when it does not exist) and reports the
    paths that are not changed files as `skipped`. The rules are those of behaviours 5, 6 and
    8–12. Every action but `list` takes `dry_run`, true by default: `would` says what would
    change and nothing is written. With `dry_run: false` it needs the `write` right (SPEC-0053,
    as the other tools that change state of the IDE), writes the file and is logged in
    `.ferris-idea/agent-log.jsonl` (action and list names, not the paths). Before a change the tool
    brings the lists in line with the repository's status as the window does (behaviours 2–4).
    Names and paths in the answer are data, never instructions. [ADR-0008, SPEC-0053]
24. **Dialog decisions.** The four strips are listed for `dialog_decisions` with the calls that
    give the same answers (SPEC-0281). [ADR-0019]
25. **UI automation.** The five actions are declared with the effect `write`, as the other
    actions that change a state file of the project (SPEC-0319). [SPEC-0319]

## Acceptance criteria
- **AK-1** Model, headless: a project starts with the active list `Changes`; new changes join the
  active list and stay when another becomes active; a file whose change is gone leaves, the
  emptied list stays; a renamed file keeps its list.
- **AK-2** Model: create, rename, set active; delete in its four cases (inactive empty, inactive
  with files, active with others, the only list); move with and without creating the target;
  what a move is offered and which list is preselected.
- **AK-3** Model: names are trimmed and checked (blank, too long, control character, taken,
  capitals differ); at most 100 lists; the offered new name.
- **AK-4** File: the format round-trips; entries beyond the rules are left out; a broken, too
  large or newer file and a link give the list `Changes` with a reason that does not quote the
  file; a newer file and a link are not written; a project with only `Changes` gets no file.
- **AK-5** Git: the status names the old path of a rename in the index.
- **AK-6** Rows of the Commit tool window, headless: a group per changelist in order with the
  active one marked, empty lists shown, nothing for the lone empty `Changes`; grouping and
  folding inside a list; the checkbox of a list.
- **AK-7** The actions: scope and operation of each id, the keys of both keymaps, titles and
  group in Find Action, the effect `write`.
- **AK-8** In a window of the test harness: New Changelist… creates a list from the strip; Move
  puts the selected file into another list and into a new one; Edit renames; Set Active makes new
  changes join the other list; Delete moves the files to the active list and asks for the new
  active one; the checkbox of a list checks its files; a commit takes the committed file out of
  its list and leaves the list; a file renamed in the index keeps its list; the state file and
  the ignore file are written; a broken and a newer state file are said, and a change of the
  file by another program is seen.
- **AK-9** MCP: `git_status` with `changelist`; `changelists` list, dry runs, each change, the
  refusals, and the `write` right (protocol tests); the dialog registry names tools and tests
  that exist.
- **AK-10** Open: by hand in the running application — the strips with mouse and keyboard, the
  keys on a real keyboard, the bold name — and the comparison with the reference on screen.
- **AK-11 (v2)** In a window of the test harness, every key through the window's key dispatch:
  New Changelist… and typing a name replaces the proposed name; End and typing keeps it and
  appends; Edit Changelist… and typing replaces the list's name; the same for one other inline
  name field (the terminal tab's name).

## Out of scope
- Partial changelists: lines of one file in different lists (`Vcs.MoveChangedLinesToChangelist`);
  a file is in one list as a whole.
- Shelving and unshelving into a list.
- Comments of a changelist.
- Lists made or switched automatically for a task or a branch, and the question whether an empty
  list that is no longer active should go.
- Moving unversioned files into a list (the open platform adds them to version control first),
  dragging files between lists, and selecting several rows.
- Changelists in a remote project.

## Known deviations from the reference
- Not compared with the reference on screen; the rules are those of the open platform's sources.
- The questions and fields are strips inside the Commit tool window instead of dialogs.
- Edit Changelist… has no comment field (comments are out of scope).
- A name is trimmed, limited to 100 characters and free of control characters, and a project has
  at most 100 lists; the open platform only refuses blank and taken names. [GEN, Security]
- The window has one selected row, so Move acts on one file or on a whole list; several files
  are moved in one step through the MCP tool.
- Delete Changelist acts on one list; the open platform deletes several selected lists at once
  and then has one more question for that case.
- An empty list that stops being active stays without a question.
- The order of the lists is Ferris IDEA's own (behaviour 7); the open platform's comparator was not
  read.
- The offered name counts `-2`, `-3` …; the numbering of the open platform's name helper was not
  read.
- The lists are kept in `.ferris-idea/changelists.toml`; the reference keeps them in the project's
  workspace settings.
