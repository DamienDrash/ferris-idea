# SPEC-0253: Creating, renaming and deleting files on a remote host

- **Version:** 3 · **Date:** 2026-11-23 (v3: behaviour 9, the folder list as a tree — folders
  open in place, typing jumps to a name, names sorted without regard to case, folders first;
  OBS-0118 gap R2, window part; v2 2026-11-15: behaviour 8, the field takes Enter for the window; AK-6, hand test 183 with real keys)
- **Sources:** GEN (remote host browsers in IDEs create files and folders, rename and delete
  entries; F-083 "Remote"), STD (POSIX `sh`, `mkdir`, `mv`, `rm`, `rmdir`, `test -e`, `set -C`
  (noclobber), `find … | wc -l`), SPEC-0121, SPEC-0053, ADR-0008, OBS-0118 (gap R2), GEN (file
  trees open folders in place and jump to a typed name)
- **Feature matrix rows:** F-083

## Purpose
Change the layout of a folder on a host from the Remote Host window without opening a terminal.

## Behaviour
1. **Create:** a new empty file is created only if nothing of that name exists (noclobber); a new
   folder with `mkdir` (parents are not created). An existing name is refused with "already
   exists". A new file is opened for editing right away (SPEC-0121 behaviour 4). [STD]
2. **Rename and move:** an entry is renamed (or moved by an absolute or relative target) with `mv`
   only if the target does not exist yet; a target in another folder must be an existing folder.
   An opened local copy of a renamed file stays bound to the old path; the status bar says so.
   [STD]
3. **Delete:** a file or link is removed with `rm -f`, an empty folder with `rmdir`. A non-empty
   folder is only removed when asked explicitly: the window first names the number of entries
   inside ("delete src/ and its 42 entries?"), MCP needs `recursive: true`. The root folder `/`,
   the home folder and `.`/`..` are always refused. Links are removed, never followed. [STD]
4. **Window:** in the Remote Host window "New File" (⌘N), "New Folder" (⇧⌘N), "Rename" (⇧F6) and
   "Delete" (⌘⌫) work on the folder shown and the selected entry. New and Rename put the name
   into the field with a prompt ("New file in /srv/app: name, Enter creates, Escape cancels");
   Delete asks in the list line ("Delete notes.txt? Enter deletes, Escape keeps it"). After each
   change the folder is read again and the new or renamed entry is selected. [GEN]
5. **MCP:** `remote_create` (`host`, `path`, `kind` `file`/`dir`), `remote_rename` (`host`, `from`,
   `to`) and `remote_delete` (`host`, `path`, `recursive`), each with `dry_run` default true (what
   would happen); a real change needs the user-granted `ssh` right and is logged in
   `.ferris-idea/agent-log.jsonl`. [ADR-0008, SPEC-0053]

8. **Keys in the field (v2):** while the field has the focus, Enter, Escape, ↑ and ↓ act on the
   window before the field sees them, so a typed host, path or name goes with Enter; every other
   key edits the field, and ⌘⌫ there deletes text, never an entry. Before, Enter put a line break
   into the field and nothing typed there could be confirmed with the keyboard (found by the
   re-check of hand test 183 on 2026-11-15). [GEN]

9. **Folder list as a tree (v3):** the window's list is a tree under the folder shown.
   - **Open in place:** → (or a click on the triangle before a folder) opens a folder below its
     row, indented, without leaving the folder shown; ← on an open folder closes it, on an entry
     inside one selects that folder. Enter on a folder still goes into it, Enter on a file inside
     an open folder opens that file. A folder is read when it is opened ("reading…" in its place
     until it arrives); an error says so in the list. F5 reads the folder shown and every open
     folder again; going to another folder closes them all. New File, New Folder, Rename and
     Delete act on the selected row's own folder for Rename and Delete, and on the folder shown
     for New.
   - **Type to jump:** while the list has the focus, typed letters, digits and punctuation
     (without ⌘, ⌃ or ⌥) select the first row whose name starts with the typed text, without
     regard to case, else the first that contains it; the typed text is shown in the list's
     status line and is forgotten after 1 s without a key, on Escape or on any other key. Nothing
     is selected when no row matches, and the selection stays.
   - **Order:** in every folder of the tree, folders first, then files; each group by name
     without regard to case (`alpha`, `beta`, `Zeta`), names equal but for case in byte order.
     The order is one shared function of `ferris_idea_remote` (`sort_entries`); the window applies
     it to every folder it shows, whatever order the listing delivered. [GEN, deviation from SPEC-0121 v4 behaviour 3's byte order: OBS-0118 R2]

## Acceptance criteria
- **AK-1** Path checks (absolute, not `/`, not `.`/`..`), prompts and confirmation text, window
  rows (unit tests in `crates/remote`, `remote_view::tests`).
- **AK-2** Against a real `sshd`: create a file and a folder, refuse existing names, rename a file
  with a space and a `$`, refuse an existing target, delete a file, refuse a non-empty folder
  without `recursive`, delete it with `recursive`, refuse the home folder (`crates/remote/tests/it/ssh.rs`).
- **AK-3** MCP: dry runs without rights, refusal without `ssh`, create/rename/delete with log
  entries (`crates/mcp/tests/it/protocol.rs`: `remote_file_operations`).
- **AK-4** Window self-test against the test `sshd`: New File opens the new file, Rename renames
  it, Delete removes it after the question.
- **AK-5** Hand test (round 4, no. 183): on a real host, next to the reference's remote host browser.
- **AK-6 (v2)** Hand test 183 in a window of the test harness with real keys against the system
  `sshd` started as the current user (`crates/workbench/tests/it/f083_handtests.rs`: `hand_test_183_files_on_the_host_with_keys`;
  skipped without `sshd`): a folder typed into the field with Enter is shown; ⌘N, a typed name and
  Enter make and open a file; ⇧F6 puts the name into the field and Enter renames; ⇧⌘N makes a
  folder; ⌘⌫ on the folder asks with its one entry and deletes nothing before Enter; ⌘⌫ on the file
  deletes it.

- **AK-7 (v3)** Unit tests of the order (`files::tests`) and of the tree rows, ←/→ and type to
  jump (`remote_view::tests`); in a window of the test harness with real keys against the test
  `sshd` (`crates/workbench/tests/it/f083_remote_r1r2.rs`): a folder with `alpha/`, `Beta/`,
  `zeta.txt`, `Alpha.txt` and `beta.txt` lists `alpha/`, `Beta/`, `Alpha.txt`, `beta.txt`,
  `zeta.txt`; → opens `alpha/` in place with its entries indented below while the folder shown
  stays; Enter on a file inside opens it; ← closes it; typing `zet` selects `zeta.txt`, `BE`
  selects `Beta/`.

## Out of scope
Copying between hosts, changing permissions or owners, moving into the trash, undo of a remote
change, drag and drop.

## Known deviations from the reference
A non-empty folder is never deleted on a single generic confirmation: the question names how
many entries go with it, and the home and root folders are refused (IMP-0120).
