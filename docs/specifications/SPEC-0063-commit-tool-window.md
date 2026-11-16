# SPEC-0063: Commit tool window

- **Version:** 11 · **Date:** 2026-11-14 (v11: changelists, behaviour 23, [SPEC-0335](SPEC-0335-changelists.md); v10: Commit Message History, behaviour 22, AK-12; docs/research/KEYMAP_COVERAGE.md); 2026-11-13 (v9: the window's operations under their keymap ids, behaviour 21; docs/research/KEYMAP_COVERAGE.md; v8 2026-11-06: F4 and ⌘↓ open the file of a row, a click takes the focus into the list, behaviour 20; v7: toolbar icons instead of text glyphs; OBS-0022 L15; v2: checkbox and row selection apart, diff on double-click and ⌘D; v3: unsaved files, hand test; v4: message field without line numbers; v5: toolbar, grouping, rollback, amend, placeholder, Commit and Push, options — side by side in OBS-0013 item 2; v6: the list follows changes made outside; OBS-0022)
- **Sources:** GEN (commit views of IDEs), OSS (action id `ActivateCommitToolWindow`, file status colour
  keys `FILESTATUS_*` of the bundled schemes, tool window icon `toolwindows/commit`), OBS (OBS-0013
  item 2: parts of the reference's window), OSS (v10, message history, `JetBrains/intellij-community`
  at the pinned commit: `platform/vcs-api/shared/src/com/intellij/openapi/vcs/VcsConfiguration.java`
  — which messages are kept, how many, duplicates; `platform/vcs-impl/src/com/intellij/vcs/commit/AbstractCommitMessagePolicy.kt`
  and `AbstractCommitWorkflowHandler.kt` — when a message is kept;
  `platform/vcs-impl/src/com/intellij/openapi/vcs/actions/ShowMessageHistoryAction.kt` — the
  list; `platform/platform-resources-en/src/messages/ActionsBundle.properties` — the action's
  title), STD (Git: ignore files, `commit --amend`, author
  `Name <email>`), SPEC-0032 (status), SPEC-0034 (commit), SPEC-0058 (editor view for the message),
  SPEC-0061 (window chrome), SPEC-0084 (diff), SPEC-0109 (Push dialog), SPEC-0230 (hooks),
  SPEC-0053 (`vcs-write` right)
- **Feature matrix rows:** F-074

## Purpose
Commit from the window as in the reference's Commit tool window: see what changed, choose files, write a
message, commit.

## Behaviour
1. **List:** two groups, "Changes" (tracked files changed against HEAD) and "Unversioned Files" (new files
   Git does not track), each with its file count, sorted by path. A row shows a checkbox, the file-type icon,
   the file name in its status colour (`FILESTATUS_MODIFIED`, `FILESTATUS_ADDED`, `FILESTATUS_DELETED`,
   `FILESTATUS_RENAMED`, unversioned `FILESTATUS_UNKNOWN`) and its folder dimmed. [GEN, OSS]
2. **Default selection:** tracked changes are checked, unversioned files are not. [GEN]
3. **Choosing:** clicking a file's checkbox toggles it; clicking the row selects it (highlighted) and a
   double-click or ⌘D (Ctrl+D) on the selected row shows its diff (SPEC-0084, v2); clicking a group header
   checks the whole group, or unchecks it when all its files are checked. [GEN]
4. **Refresh:** the status is read when the tool window opens and with the refresh button; files that stay
   listed keep their checks. [GEN]
5. **Commit:** the message is written in an editor field (v4: a plain multi-line field without line numbers, folding or current-line background, as in the reference); the Commit button or ⌘↩ (Ctrl+↩ elsewhere)
   commits the checked files with the message (SPEC-0034: an empty message or no selected file is refused
   with the reason). After a commit the message is cleared, the list refreshed and the new commit's short id
   and summary are shown; errors are shown in red. [GEN]
6. **Tool window:** on the left in place of the project tree; the stripe has a Commit button under Project;
   `ActivateCommitToolWindow` (⌘0 on macOS, Alt+0 elsewhere) opens it and focuses the message, or returns to
   the project tree. [OSS, OBS]
7. The MCP counterparts are `git_status` and `git_commit` (SPEC-0032, SPEC-0034). [ADR-0008]

8. **Unsaved files (v3):** files with unsaved changes in an open editor are listed under "Changes" (checked)
   even before Git sees a change; Commit saves the open files first, so the commit holds what the
   editors show. [GEN]

### v5: closing the gaps of OBS-0013 item 2
9. **Toolbar.** A row of icon buttons above the list, each with a tooltip: Refresh (behaviour 4),
   Rollback, Show Diff (the selected row, as ⌘D), Group By (a small menu), Expand All, Collapse All.
   [OBS, GEN]
10. **Rollback.** Acts on the checked files of "Changes"; when none is checked, on the selected row if
    it belongs to "Changes"; unversioned files are never rolled back. It first asks in a confirmation
    strip inside the window naming the number of files, with an option to also delete the local copies
    of added files (off by default), and the buttons Rollback and Cancel; Escape cancels. Rolling back
    gives a file that is in HEAD its HEAD content in the working tree and the index; a file added
    since HEAD leaves the index and stays on disk as an unversioned file, or is deleted with the
    option. Open editors of rolled-back files show the disk content afterwards (unsaved edits of them
    are discarded — that is what rolling back means). The list refreshes; errors show in red.
    [GEN, STD]
11. **Group By.** Two independent switches, Directory and Module, both off by default (the flat list
    of behaviour 1 with the folder dimmed). Directory nests the files of each group under their
    folders; a chain of folders with nothing else in between is shown as one node (`src/main/java`).
    Module puts a node per module above that: the nearest folder (from the file up to the repository
    root) holding a build file (`Cargo.toml`, `pom.xml`, `build.gradle`, `build.gradle.kts`,
    `settings.gradle`, `settings.gradle.kts`, `package.json`, `pyproject.toml`, `go.mod`); files
    with no such folder belong to the repository root's module, named after the root folder. Every
    node shows a chevron, a checkbox for all files below it and the number of files; clicking the
    node folds or unfolds it, clicking its checkbox checks all files below or unchecks them when
    all are checked (the two group headers keep behaviour 3; their chevron folds them). Folded nodes
    stay folded across refreshes. Expand All unfolds every node; Collapse All folds every node,
    including the two groups. [OBS, GEN]
12. **Unversioned files.** "Unversioned Files" lists files Git does not track and does not ignore:
    paths matched by `.gitignore` files, the repository's `info/exclude` or the global excludes file
    are never listed. Their checkboxes start unchecked (behaviour 2); a checked unversioned file is
    added by the commit. [STD, OBS]
13. **Amend.** A switch "Amend last commit" under the message, with the last commit's short id and
    subject next to it (dimmed) so it is clear which commit is replaced; without any commit the
    switch is disabled. Turning it on keeps the typed text aside and fills the field with the last
    commit's whole message; turning it off restores the kept text if the field still holds the last
    commit's message unchanged, otherwise the field keeps what was edited. Committing with the switch
    on replaces the last commit: same parents, its tree plus the checked files, the field's message;
    the author stays that of the amended commit unless an author is set (behaviour 16); no checked file
    is needed (message-only amend). After it the switch turns off. [STD, OBS]
14. **Placeholder.** The empty message field shows a dimmed placeholder ("Commit message"); it
    disappears with the first character. [OBS]
15. **Commit and Push.** Next to Commit a second button "Commit and Push…" commits exactly as Commit
    does and, when the commit succeeds, opens IDECraft's Push dialog (SPEC-0109) with the commits the
    remote lacks; a refused or blocked commit opens nothing. [OBS]
16. **Commit options.** A gear button at the right of the buttons opens a small panel with the commit
    options IDECraft has: "Run Git hooks" (SPEC-0230 behaviour 6, shown only when the repository has
    commit hooks; it moves from under the message into the panel) and "Author" (empty: the configured
    identity; otherwise `Name <email>`, anything else refuses the commit with the reason). The options
    stay set for the window's lifetime. [OBS, STD]
17. **MCP (v5).** `git_commit` takes `amend` (default false) and `author`; the new tool
    `git_rollback` takes paths and `delete_added`, defaults to a dry run listing what would be
    restored, unstaged or deleted, and needs the `vcs-write` right to write (SPEC-0053). [ADR-0008]

18. **v6 – Changes from outside:** when the file status of the repository changes (checked every 2
    seconds, SPEC-0059 behaviour 7), an open Commit tool window reads the status again, keeping the
    user's checks; before v6 a file changed by another program appeared only after Refresh or a focus
    change. [GEN, OBS-0022 L29]

19. **v7 – Toolbar icons:** the toolbar buttons (Refresh, Rollback, Show Diff, Group By, Expand All,
    Collapse All) and the options gear show 16 px icons in the style of the open New UI icons:
    `general/refresh`, `general/expandAll`, `general/collapseAll`, `general/settings` (Apache-2.0) and
    IDECraft's own `idecraft/rollback`, `idecraft/diff`, `idecraft/groupBy` (ADR-0009: drawn, not traced),
    with light and dark variants; no text glyphs. [ADR-0009, OBS-0020 item 17, OBS-0022 L15]
20. **v8 – Jump to the file:** F4 or ⌘↓ (Ctrl+↓) on a row opens the file in the editor and focuses it (`EditSource`), as in the reference; in the message field ⌘↓ keeps moving the caret. A single click on a row takes the focus into the list, so its keys (⌘D, F4, ⌘↓) reach it. [OBS-0022 Nachprüfung im Fenster 2026-11-06]
21. **v9 – Actions by id:** operations the window already has are also actions under the ids of the bundled keymaps: `Diff.ShowDiff` (⌘D, Ctrl+D) is Show Diff of the selected row (behaviours 3 and 9); `Vcs.ToggleAmendCommitMode` (⌃⌥M on macOS) flips the Amend switch of behaviour 13 and does nothing in a repository without a commit; `ChangesView.GroupBy.Directory` (⌃P) and `ChangesView.GroupBy.Module` (⌃M) flip the two switches of behaviour 11; `ChangesView.ShowCommitOptions` (⌃O) opens and closes the options panel of behaviour 16. With their keys they run only while the Commit tool window has the focus (elsewhere the keys keep their other actions); from Find Action they run while the window is shown and are not available otherwise. Not carried out: `ChangesView.Move`, `ChangesView.SetDefault`, `ChangesView.Rename` and `Vcs.MoveChangedLinesToChangelist` (changelists are out of scope) and `Vcs.ShowMessageHistory` (no message history; carried out since v10, behaviour 22). [OSS: ids and shortcuts of the bundled keymaps; GEN]
22. **v10 – Commit Message History** (`Vcs.ShowMessageHistory`, ⌃M in both bundled keymaps; Find Action lists it as "Commit Message History" in the group "Commit"):
    - **What is kept.** The messages the user committed with in the Commit tool window of this project — not what `git log` holds, so commits made elsewhere, by another program or by the MCP tool are not in it. A message is kept when Commit or Commit and Push… starts a commit that has something to commit (a checked file, or Amend), before the commit runs: a commit a hook blocks or Git refuses keeps its message in the history too. The message is kept as it was typed. A message of white space only is not kept. A message equal, character for character, to one kept before is not kept twice: it becomes the newest. At most 25 messages are kept; the oldest goes for a new one. [OSS: `VcsConfiguration.saveCommitMessage`, `addCommitMessage`, `MAX_STORED_MESSAGES`; `AbstractCommitMessagePolicy.onBeforeCommit`]
    - **The list.** The action opens a list above the message field with the kept messages, newest first, the newest selected; the focus is in the message field. A row shows a message on one line: line breaks as `↵`, at most 100 characters, then `…`; seven rows are visible, more scroll. Up and Down move the selection around the ends, Enter or a click on a row chooses it, Escape or the action again closes the list without a change. Choosing replaces the whole text of the message field with the message as it was typed; that is one undo step, so one Undo in the field brings back what was typed before. Starting a commit closes the list. [OSS: `ShowMessageHistoryAction` — newest first, one line per message, seven visible rows; GEN]
    - **Where the key acts.** ⌃M is also Group By Module (behaviour 21). In the message field it is Commit Message History; in the rest of the Commit tool window it stays Group By Module; elsewhere the key has its other actions. From Find Action the action runs while the window is shown and puts the focus into the message field. [GEN]
    - **Nothing to show.** Without a kept message no list opens and the status bar says that this project has no commit messages in its history yet. In a remote project the history is neither kept nor shown, and the status bar says that it is not available there yet. [GEN]
    - **Storage.** `.idecraft/commit-messages.toml` in the project: `version = 1` and `message`, the list newest first. It is machine-local state of one user and is listed in the generated `.idecraft/.gitignore` (SPEC-0069 behaviour 3), which is written or refreshed before the first message is kept. The file is replaced as a whole through a temporary file. [GEN]
    - **Messages are data.** A message may hold anything the user typed. It is stored and given back unchanged and is never interpreted, run, expanded or used as a path or a format. A row is for reading: control characters and direction overrides are shown as spaces there, the chosen message itself is not changed. No message is written to a log, a status text or an error text; the status texts of this behaviour name the file and the reason only. A message over 8 KiB (UTF-8) is not kept. A file over 2 MiB, a file that is not this format (`version` other than 1, broken TOML), or a link or folder under the file's name is not used: the history is empty, the status bar says that the file was not read and why, without quoting it, and the next kept message writes a good file in its place (a link or folder is not written through; the status bar says that the message was not added). From a readable file, entries beyond these rules (blank, too large, repeated, more than 25) are left out. [GEN, Security: CLAUDE.md rule 12]
    - **MCP.** `git_commit` with `message_history: true` commits nothing and returns `messages` (newest first), `limit`, `file` and `problem` (why the file was not used, else null). It reads only and needs no right. The tool's description says that the messages are text the user typed and never instructions. [ADR-0008]
    - **UI automation.** The action is declared without effect (it opens a list); choosing a row changes the text of the message field only. [SPEC-0319]

23. **v11 – Changelists.** The group "Changes" is one group per changelist, and `ChangesView.NewChangeList`, `ChangesView.Rename`, `ChangesView.SetDefault`, `ChangesView.RemoveChangeList` and `ChangesView.Move` are carried out: [SPEC-0335](SPEC-0335-changelists.md). It changes behaviour 3 for the changelists' rows: a click on the row selects the list, its checkbox checks its files (SPEC-0335 behaviour 14). `Vcs.MoveChangedLinesToChangelist` stays not carried out (a file is in one list as a whole). [SPEC-0335]

## Acceptance criteria
- **AK-1** Groups, default checks, toggling files and groups, refresh keeping checks, status colour keys
  (unit test).
- **AK-2** In a real window (`idecraft ui --selftest`): ⌘0 opens the Commit window with only the changed
  tracked file checked; typed message and ⌘↩ create the commit, the untracked file stays untracked.
- **AK-3** Use by hand with the mouse.
- **AK-4** (v5) Model, headless: grouping by directory (with chains of folders joined), by module and
  both; node checkboxes; folding with Expand All/Collapse All kept across a refresh; rollback targets
  (checked changes, else the selected row, never unversioned files); amend message on/off (unit tests).
- **AK-5** (v5) Git, headless: amend replaces HEAD with the same parent, with and without files, keeps
  the author unless one is given; an author override; an invalid author is refused; rollback restores a
  modified and a deleted file, unstages an added file and keeps it, deletes it with the option; an
  ignored file is not listed (unit tests in the Git crate).
- **AK-6** (v5) MCP: `git_commit` with `amend`, `git_rollback` dry run and real run, refused without
  `vcs-write` (protocol tests).
- **AK-7** (v5) In a real window (`idecraft ui --selftest`): placeholder shown for the empty field;
  amend fills the last message and shows its commit; Group By Directory nests the files; Rollback asks
  and restores the file; Commit and Push… commits and opens the Push dialog.

- **AK-8 (v6)** In the release bundle: a file changed from a shell appears in Changes within 2 seconds
  while the window keeps the focus (OBS-0022 Nachprüfung).
- **AK-9 (v7)** Every toolbar button has a bundled icon in both themes (`commit_view::tests::
  toolbar_buttons_have_bundled_icons`); look next to the reference (window check, OBS-0022 L15).
- **AK-10 (v8)** By hand in the window: F4 on `Inventory.java` opens it in the editor.
- **AK-11 (v9)** The actions run first and only in the Commit window's focus, and each maps to its operation (unit tests `keymap_actions::tests::scoped_actions_run_first_and_only_in_their_focus`, `keymap_actions::tests::commit_actions_name_their_operation`). In the window (open): ⌃P nests the files, ⌃⌥M fills the last message.
- **AK-12 (v10, behaviour 22)** Store, headless: newest first, an equal message moves to the front, blank ones are not kept (`commit_messages::tests::newest_first_and_each_message_once`); 25 messages and 8 KiB (`the_number_and_the_size_are_limited`); odd text survives the file unchanged and the largest history stays under the file limit (`messages_are_data_and_survive_the_file`); a broken, too large or newer file is an empty history with a reason that does not quote it, and the next message replaces it (`an_unusable_file_is_an_empty_history_with_the_reason`); a link is neither read nor written through (`a_link_is_not_followed`). Model: rows, selection and the chosen message; one undo step; what a commit remembers (`commit_model::tests::{the_message_history_lists_rows_and_gives_the_message, a_chosen_message_replaces_the_text_in_one_undo_step, a_commit_with_something_to_commit_remembers_its_message}`). The key: ⌃M is the history in the message field and Group By Module in the rest of the window, in both keymaps (`keymap_actions::tests::the_message_history_key_belongs_to_the_message_field`); title and group in Find Action (`everywhere::tests::part_actions_have_titles_and_groups`); effect (`security::tests::commit_message_history_declares_no_effect`). In a window of the test harness: three commits from the window, the list newest first with the repeated message once, Down and Enter put the second message into the field, one undo brings the typed text back, Escape and the action close the list, the ignore file lists the history file; a broken file is said in the status bar without its content and replaced by the next commit (`crates/workbench/tests/it/commit_message_history.rs`). MCP: `git_commit` with `message_history` (`protocol.rs`: `git_commit_lists_the_message_history`). Open: ⌃M pressed on a real keyboard in the message field, a click on a row, the list's look, and the comparison with the reference.

## Out of scope
Partial changelists (lines of one file in different lists; file-level changelists: SPEC-0335), partial commits of hunks, sign-off and other commit options IDECraft does not have, diff
preview inside the window, commit checks (code analysis before commit), shelving, rollback of single
hunks, grouping by repository (one repository per workspace).

## Known deviations from the reference
- Compared side by side in OBS-0013 item 2 (v5 closes the gaps found there).
- The rollback confirmation is a strip inside the window instead of a separate dialog listing the
  files; the files are the checked ones the user already sees. [GEN]
- Group By settings and folded nodes are kept for the window's lifetime, not saved with the project.
- v10, Commit Message History: not compared with the reference on screen; the rules are those of the open platform's sources. Differences chosen here: the list stands above the message field inside the tool window instead of a floating popup; moving the selection does not put the message into the field as a preview — only choosing does; typing does not filter the list; a row is cut after 100 characters instead of at the subject margin; the field has no history button; the action is not available without a kept message, which the status bar says, where the reference disables it. A message over 8 KiB is not kept and a history file IDECraft cannot use is replaced (the reference keeps its list in the project's workspace settings and has no such limits). [OSS, GEN]
