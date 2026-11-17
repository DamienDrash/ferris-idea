# SPEC-0145: Interactive rebase and stash changes

- **Version:** 4 · **Date:** 2026-11-06 (v4: the diff in the dialog looks like the diff viewer and opens at the first change, behaviour 8; v3: files, details and diff of the selected commit; OBS-0022 L30; v2: keys of the reword field, focus after a click, the base
  named by its commit; hand test 71, D19, D33, D34)
- **Sources:** GEN (IDEs offer "Interactively Rebase from Here" in the Git log with a list of
  the commits where each can be picked, reworded, squashed, fixed up or dropped and moved up or
  down, and show the changes of a stash before it is applied; F-074), STD (`git rebase
  --interactive` with a todo list of `pick`/`squash`/`fixup`/`drop` lines and `exec`,
  `sequence.editor`, `git commit --amend --only -F`, `git stash show`, the untracked-files
  parent `stash@{n}^3`), SPEC-0084, SPEC-0106, SPEC-0144, ADR-0008, ADR-0012
- **Feature matrix rows:** F-074

## Purpose
Clean up local history before pushing and look into a stash before applying it, the two Git
operations SPEC-0106 left out.

## Behaviour
1. **Opening:** in the Git log ⌥I on a commit opens the dialog for that commit and every later
   one on the current branch ("from here"); `Git.Interactive.Rebase` (Search Everywhere, menu
   Git › Interactively Rebase…) opens it for the commits since the upstream. A range with merge
   commits, no commits or no upstream is refused in the status bar. [GEN]
2. **Dialog:** the commits oldest first with action, short id and subject. ↑/↓ select, ⌥↑/⌥↓
   move the selected commit, P/R/S/F/D set pick, reword, squash, fixup, drop. Reword shows a
   message field prefilled with the subject; Enter or Tab keeps the message (neither goes into the
   field), Escape leaves it. A click on a row keeps an open message, selects the row and gives the keys
   back to the list, so Escape and the action keys work after it (v2). The title "Interactive Rebase
   onto <base>" names the commit the rebase starts from — for ⌥I on a commit that is its parent — by
   its short id, an upstream by its name (v2). A
   dropped commit is struck through, a reworded one shows its new message. Enter starts,
   Escape cancels. [GEN]
3. **Checks before starting:** the first kept commit may not be squash or fixup; a reword
   needs a message. The rebase itself checks again that the list holds each commit since the
   base exactly once and that ids are commit ids. [STD]
4. **Running:** IDECraft writes the todo list and lets Git copy it in (`sequence.editor`), so
   no editor opens; reword runs as `pick` plus `exec git commit --amend --only -F <file>` with
   the message in a file under the Git directory, so the text never passes through a shell.
   Squash keeps both messages — the first as the subject, the squashed one in the body — fixup the
   first. Local changes are stashed around it. A stop on
   conflicts reports the files and waits for `Git.Continue`/`Git.Abort` (SPEC-0106). [STD,
   ADR-0012]
5. **Stash changes:** in the stash popup ⌘D on a stash shows its changes side by side, file
   after file, each file under a header row with its path and "(new)" or "(deleted)";
   untracked files stashed with it are included. The columns are "Base" and "Stash". [GEN, STD]
6. **MCP:** `git_rebase` with `interactive` returns the todo list (dry run) and runs a list
   sent back as `todo`; `git_stash` `show` returns the files with their text before and in the
   stash. [ADR-0008]
7. **v3 – The selected commit:** right of the list the dialog shows the selected commit's changed
   files in the file-status colours and below them its message, id, author and, when someone else
   committed it, the committer (SPEC-0086 v7 behaviour 15); they follow the selection and are read
   in the background. Tab moves the keys to the files: ↑/↓ choose, Enter (or a click) shows the
   file before and after the commit side by side with line numbers and change colours under the
   list, Escape closes the diff and then returns to the list, Tab returns directly. While the keys
   are on the files, the action keys do nothing. Agents read the same with `git_show` (files,
   details, the diff of one file). [OBS-0022 L30, ADR-0008]
8. **v4 – The dialog's diff:** the diff of a file shown in the dialog (behaviour 7) colours both sides by the file's language and marks the changed words of a modified line as the diff viewer does (SPEC-0084 v2 behaviours 7–8); it opens scrolled to the first change, F7 and ⇧F7 go to the next and the previous change, stopping at the ends, and the change shown is marked with a line. Before v4 the diff opened at the top of the file, plain, and F7 reached the debugger's Step Into. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L30]

## Acceptance criteria
- **AK-1** `crates/vcs/tests/it/ops.rs`: the files of a stash with tracked and untracked changes;
  an interactive rebase that reorders, rewords, fixes up and drops; refusals (squash first,
  missing commit, reword without message, an option as base); a reorder that conflicts stops
  and aborts cleanly.
- **AK-2** Header rows of a stash diff (`vcs_ops_model::tests::stash_rows_have_a_header_per_file`).
- **AK-3** MCP: `git_stash` `show`, `git_rebase` `interactive` as dry run and with reword and
  fixup, an unknown action refused (`crates/mcp/tests/it/protocol.rs`:
  `git_stash_show_and_interactive_rebase`).
- **AK-4** Window self-test: ⌘D in the stash popup shows the stash's diff; the dialog lists two
  commits oldest first; ↓ F Enter folds the second into the first.
- **AK-8 (v2)** A squash keeps both messages (`crates/vcs/tests/it/ops.rs`:
  `interactive_squash_keeps_both_messages`); the base label (`git_ops_model::tests::rebase_base_is_named_by_the_resolved_commit`);
  window self-test: the title names HEAD~2's id, Enter and Tab end the reword field, keys work after a
  click on a row.
- **AK-7 (v3)** Window self-test: the dialog shows the files of the selected commit; Tab and Enter
  show the diff of its file; Escape closes the diff; ↓ in the list shows the next commit's files.
  Next to the reference in the release bundle (window check open).
- **AK-5** Hand test (round 4, no. 71): from the Git log with ⌥I, every action and a reorder
  with a conflict, next to the reference; a stash's changes.
- **AK-6 (v4)** Each side's text for the syntax colours keeps its line numbers (unit test `rebase_view::tests::each_side_keeps_its_line_numbers`); by hand in the window: F7/⇧F7, first change shown.

## Out of scope
`edit` and `exec` lines chosen by the user, rebasing merges (`--rebase-merges`), changing the
base inside the dialog, a stash diff per file in a separate tab, applying single files of a
stash.

## Known deviations from the reference
Reword is done as an amend after the pick instead of `reword`, so no message editor opens; the
result is the same commit history.
v3: the diff of a file opens inside the dialog under the list (the reference opens a diff window on
a double-click); a merged view of several selected commits is not offered.
