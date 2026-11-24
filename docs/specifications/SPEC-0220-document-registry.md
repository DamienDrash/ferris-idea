# SPEC-0220: Document registry for open buffers

- **Version:** 6 · **Date:** 2026-11-16 (v6: the reference's reading of a line range named as a deviation, IMP-0440, OBS-0071; v5: 2026-11-14, v4: 2026-11-14, v3: 2026-11-14, v2: 2026-11-14, v1: 2026-11-03)
- **Sources:** GEN (an IDE that serves agents shows them the text the user sees, saved or not, and lets
  their edits land in that text), readiness decision A2 (`docs/architecture/READINESS.md`, taken
  2026-11-02: a headless document registry before the HTTP transport), ADR-0008 ("write tools edit
  through the editor model so undo works"), ADR-0012 rule 4, QG-1 report criterion 6, SPEC-0006,
  SPEC-0053 v8, SPEC-0058, SPEC-0112, SPEC-0114 v3, SPEC-0193 v5, SPEC-0063 v5, SPEC-0064 v3,
  SPEC-0084 v5, SPEC-0101 v7, SPEC-0163, SPEC-0164 v3, SPEC-0212, SPEC-0336
- **Changes in v2:** agent writes to open files go through the open buffer (behaviours 7–12, AK-4 to AK-6)
- **Changes in v3:** language-server edits and refactorings go through the open buffers, computed against
  the shown text, as one all-or-nothing change of several editors (behaviours 13–16, AK-7 to AK-9);
  behaviour 9 narrows to the tools that still compute from the disk
- **Changes in v4:** before a change that can touch several files the language server and Ferris IDEA's
  own planners get the unsaved text of every open file of the workspace, so other unsaved files are no
  longer refused (behaviour 13); the window keeps an agent's refactoring as one of its undo groups
  (behaviour 15, SPEC-0193 v5 behaviour 10); `move_file` with reference updates goes through the
  buffers (behaviour 16) and leaves the *refuses unsaved* list (21 → 20); AK-9 to AK-11
- **Changes in v5:** Rollback, conflict resolution, a change's rollback in the gutter, Accept Left/Right
  of Compare Two Files, SQL renames in `.sql` files, resource bundle edits and the line separator of a
  file go through the open buffers, planned against the shown text (behaviours 17–21); deleting and
  renaming a file keep refusing unsaved files, now with the reason (behaviour 22); the *refuses
  unsaved* list shrinks from 20 to 13; AK-12 to AK-15
- **Feature matrix rows:** F-084

## Purpose
Agents and the user work on the same open file: the text of every open editor, including changes not yet
saved, is kept in one registry outside the UI toolkit, so an MCP tool reads what the user sees instead of
an older version on disk.

## Behaviour
1. The registry maps a file path (resolved through links and `..`) to its current text, a revision and
   whether the text differs from the last save. It lives in the headless editor crate, not in the
   workbench; the window renders the buffer, the registry only mirrors it. [GEN, A2]
2. An editor view with a file path registers the file when it opens, publishes after every change of the
   text (typing, undo, redo, reload from disk, edits of another view of the same file) and after a save,
   and unregisters when the view goes away. Several views of one file share one entry, which stays until
   the last view is gone. Views without a path (input fields, consoles without file) are not registered.
   [GEN]
3. Publishing costs no copy of the text: the registry keeps a shared snapshot of the rope, which is
   copied into a string only when a reader asks for it. [GEN]
4. The revision of an entry starts at 1 and grows by one with every published change of the text; a save
   that does not change the text only clears the modified flag. [GEN]
5. The registry is safe to read from other threads (the MCP server runs on its own threads) and never
   blocks the window for longer than copying a pointer. [GEN]
6. MCP: `read_file` returns the registry's text when the file is open, and the file on disk otherwise; it
   says neither in the text, so a stdio client sees the same result shape. The new tool `open_documents`
   lists the open files as workspace-relative paths with `revision` and `modified`, sorted by path; files
   outside the workspace are left out. Without a running window (stdio server) the list is empty. [GEN]

7. **Agent edits land in the open editor.** When an MCP tool of the window's server changes the text of
   a file the registry holds, it does not write the disk behind the editor. It hands the window an edit:
   the file, the revision its text was read at, and the new text. On its own thread the window applies
   the edit to the file's editor as **one undo step** that replaces only the lines that differ (carets
   and selections stay on their text, other views of the file follow as for typing, SPEC-0112), and
   publishes it, so the revision grows by one. When the buffer had no unsaved changes, the window then
   saves it: the disk holds what the tool promised and the editor is unmodified (undo makes it modified
   again). When the buffer had unsaved changes, it stays unsaved, now holding the user's edits and the
   agent's; the user's save writes both. Either way the user's Undo takes the agent's edit back in one
   step and nothing the user typed is lost. The tool's answer is the same as for a write to disk. [GEN,
   A2, ADR-0008]
8. **The text the edit is based on.** A tool that computes its new text from text alone (generators,
   Ferris IDEA's own inspections and fixes, copyright notices, import optimisers and formatters that take
   the text on standard input or from the call) reads the file as the window shows it, saved or not
   (as `read_file` does, behaviour 6), so its result contains the user's unsaved edits. The window
   refuses an edit whose revision is no longer the file's (the user typed in between), and one for a
   read-only editor (SPEC-0336 behaviour 18); the tool then fails with a message that says so and
   nothing changed. [GEN]
9. **Refusal where the edit cannot go through the buffer yet.** A tool whose change is computed from the
   disk — Git operations of the working tree as a whole, deleting and renaming files (behaviour 22,
   and the moved file of a move, behaviour 16), exports into a workspace file, encoding changes
   (behaviour 21) — refuses before changing anything when a file
   it changes is open with unsaved changes; the message names the file and says to save it first.
   Tools that change the working tree as a whole (switch, merge, pull, rebase, cherry-pick, stash,
   branch actions and the commit menu's actions of the Git log) refuse while any open file of the
   workspace has unsaved changes (moving a file with reference updates no longer does, v4). A
   file changed by such a tool that is open without unsaved changes goes through behaviour 7 when the
   tool writes one file's text, and is otherwise written to disk, which the editor takes as one undo
   step (SPEC-0058 behaviour 10). [GEN]
10. **Without the window.** Over stdio no file is open (behaviour 6). When the window does not answer an
    edit within 10 seconds, or cannot take it, a file without unsaved changes is written to disk as in
    behaviour 9, and one with unsaved changes is refused as in behaviour 9: never a silent overwrite.
    [GEN]
11. **Ferris IDEA's own files.** Tools that change only files Ferris IDEA keeps as its own data or settings
    (`.ferris-idea/…`, `.idea/…` profiles and dictionaries, the user's settings folder, run
    configurations), create files and folders that do not exist yet, change remote hosts or rooms
    (whose documents the backend owns, ADR-0026), or act through the window's own steps (closing,
    quitting, the Project tree's undo) are outside behaviours 7–9. An editor that shows such a file
    reloads it when it is saved and refuses its next save once when it has unsaved changes
    (SPEC-0058 behaviour 10), so nothing is lost silently. [GEN]
12. **Ratchet.** Every tool whose row in the effects table (SPEC-0053 v8 behaviour 13) names `files`
    is in exactly one of three lists: *through the buffer* (behaviours 7 and 8), *refuses unsaved*
    (behaviour 9, a list with a maximum length that only goes down) and *own files* (behaviour 11, each
    with its reason). A test fails for a tool in no list or in two, for a tool in a list without such a
    row, for a buffer tool whose code does not read the shown text and write through the buffer, for a
    refusing tool whose code makes no refusal, and when the refusing list grows past its maximum.
    [GEN]

13. **Language servers compute against the shown text.** Renaming a symbol, applying a code action,
    formatting through a language server or Ferris IDEA's own SQL and Markdown formatters, Change
    Signature, Override Methods, Delegate Methods, Extract Interface, Extract Superclass, Pull
    Members Up and moving a file with reference updates give the server (or Ferris IDEA's own planner)
    the text the window shows for the file the call names, saved or not, as the editor's own actions
    do (SPEC-0074, SPEC-0075, SPEC-0076); its positions refer to that text (as `read_file` returns
    it). (v4) Before such a call plans, the text of **every other open file of the workspace with
    unsaved changes** is given too: each one the same language server serves is brought to that text
    on the server — `didOpen` with it when the server does not have the file open, else `didChange`
    with the whole text and a version higher than any the server had for the file — and Ferris IDEA's
    own planners (text occurrences of a rename, SPEC-0148; a move's own reference rewriting and
    package line, SPEC-0114) read it in place of the disk. The server keeps that text afterwards, as
    it keeps the editor's. Edits of every file of the change therefore refer to the text the window
    shows; edits of a file whose text was not given (not open, or open without unsaved changes)
    refer to the disk, which is the same text. An open file with unsaved changes is no longer
    refused because its text differs from the disk (v3); the change is refused only when a file
    shows other text when it is applied than when it was planned (behaviour 14). Files outside the
    workspace and files of a remote host are not given. [GEN, STD: LSP `didOpen`/`didChange`,
    versioned text document identifiers]
14. **All or nothing.** Before anything is written, every file of the change is checked: the text each
    file's edits were computed on must still be the text the window shows (the user did not type
    meanwhile, and did not open a file of the change with unsaved changes since), else the call
    refuses with "<file> changed in the editor meanwhile; nothing was written" and changes nothing. The open files then reach
    the window as **one** request with every file's revision; on its own thread the window checks
    every file (revision, read-only editor, an editor that shows it) before it applies any, and then
    applies each file's edit as one undo step of that file's editor and saves those that had no
    unsaved changes (behaviour 7). One refused file refuses the whole request and nothing changed.
    Files not open are written to disk afterwards, as before (checked to still hold the text the
    edits were computed on). Without the window's answer (behaviour 10) the change is written to disk
    only when no open file of it has unsaved changes, and refused otherwise. [GEN]
15. **Local history and undo.** Files written to disk get local history revisions before and after,
    labelled with the refactoring's name, as before (SPEC-0193 behaviour 2); open files changed in
    their editors get the saved state labelled "Before <name>" (SPEC-0193 v2 behaviour 8). The
    refactoring is kept as one group for `undo_refactoring` (SPEC-0193 behaviour 7) with each file's
    text before and after. In each editor the user's Undo takes the agent's change back in one step.
    (v4) The window request of behaviour 14 also carries the refactoring's name and the files of the
    change that are written to disk, with their text before and after; once every open file is
    applied, the window keeps the change as an undo group of the project window whose editors show
    the files (SPEC-0193 v5 behaviour 10), so ⌘Z in any of those editors undoes the whole
    refactoring in all its editors and files on disk, as after a refactoring the user applied. A
    change that renames files (a Java type taking its file along), a change whose files are shown in
    editors of more than one project window or in a split only, is not kept as a window group:
    each editor's Undo takes back its own step. What `undo_refactoring` changes through the editors
    is kept as a window group named "Undo <name>".
    `undo_refactoring` checks that every file of the group still shows the text after the refactoring
    (else it names the file, forgets the group and changes nothing), then puts the earlier text back
    through behaviour 14: open files through their editors as one undo step each, the others on disk
    with a local history label "Undo <name>". A group that renamed files (a Java type taking its file
    along) is undone on disk as before and refused while a file of it has unsaved changes. [GEN]
16. **Moving a file with reference updates** (`move_file`, one `path` or several `paths`) plans
    against the shown texts (behaviour 13) and changes the referring files through behaviour 14:
    open ones in their editors, all or nothing, the others on disk; then the moved file's own edits
    are written to disk and the file is moved, as before. The moved file itself still refuses while
    it has unsaved changes (it is renamed on the disk, which its editor does not follow yet), and so
    does a plain move (`update_references: false`, a folder) of a file with unsaved changes. A move
    is not kept as an undo group (SPEC-0193 out of scope: undoing Move). `move_file` leaves the
    *refuses unsaved* list for *through the buffer* (behaviour 12). [GEN]

17. **One file's change computed from its shown text** (v5). The tools below compute their new text
    from the text the window shows, saved or not (as `read_file` returns it, behaviour 6), and write
    it through behaviour 14: open files in their editors as one undo step each, all or nothing,
    checked against the revision they were read at, the others on disk. Files written to disk get
    local history revisions before and after, labelled with the tool's action; open files get their
    saved state labelled "Before <action>" (behaviour 15). The window keeps the change under that
    name as one of its undo groups (SPEC-0193 v5 behaviour 10). Each tool's answer (`text`,
    `changed`, the listed changes) refers to the shown text.
    - `git_line_change` (SPEC-0064 v3): the file's changes against HEAD are those of the shown text,
      as the editor's change bar shows them; `rollback` takes the change back in the buffer
      ("Rollback Lines"); `commit` commits HEAD with only that change applied, taken from the shown
      text, and leaves the file and the editor as they are.
    - `compare_files` with `apply` (SPEC-0084 v5): both sides are the shown texts; Accept Left Side
      and Accept Right Side change the other file ("Accept Left Side", "Accept Right Side"). A file
      the file system marks read-only is still refused.
    - `resource_bundle` with `set` or `remove` (SPEC-0163, SPEC-0212): every file of the bundle as it
      is shown; the files that change are changed together ("Edit Resource Bundle").
    - `file_properties` `set_line_separator` (SPEC-0336 behaviour 8): every line break of the shown
      text is replaced ("Convert Line Separators"), as the editor's own action does.
    A buffer that had no unsaved changes is saved afterwards (behaviour 7); one with unsaved
    changes stays unsaved, holding the user's text and the tool's change. [GEN]
18. **Git steps that need the change on the disk** (v5). Rolling files back (`git_rollback`,
    SPEC-0063 v5 behaviour 10) and resolving a merge conflict (`git_resolve_conflict`, SPEC-0101 v7)
    end with a step of Git that takes the file from the disk (the index is set to HEAD, or the
    resolved file is added). Their new text (HEAD's text, the merge result) is therefore written
    first through behaviour 14 ("Rollback", "Resolve Conflict") and **every** changed buffer is saved,
    also one that had unsaved changes: the disk then holds exactly what Git takes, and the user's
    Undo in the editor gives back the earlier text, typed changes included, as unsaved changes. Only
    then does Git run; a Git step that fails afterwards leaves the editors' change in place (one Undo
    each) and names the failure. Before any of it, the rollback is checked as a dry run (untracked
    files, plain paths), so a refusal changes nothing. A file a rollback deletes (added since HEAD,
    `delete_added`) and a conflict resolved by taking a whole side (a binary file, a file one side
    deleted) are file operations: they refuse while the file has unsaved changes (behaviour 22).
    [GEN]
19. **SQL renames** (`rename_sql_object` with `apply`, SPEC-0164 v3). The edits of every `.sql` file
    are computed from its shown text. Before the statement runs, every file of the change must still
    show the text the edits were computed on (else the call refuses, nothing ran). After the
    database renamed the object, the files are written through behaviour 14 ("Rename <object>"). When
    the window refuses them then (the user typed in between, a read-only editor), the database stays
    renamed: the error says so, names the file and that no `.sql` file was changed, so the agent can
    call again with `sql` per file or the user can rename by hand. [GEN]
20. **Without the window** (behaviour 10) the tools of behaviours 17–19 write files without unsaved
    changes to disk and refuse files with unsaved changes; the Git steps of behaviour 18 refuse a
    file with unsaved changes as well (a buffer can only be saved by the window). [GEN]
21. **Encodings stay refused for unsaved files.** Converting a file's encoding writes other bytes
    for the same text, and reading a file again in another encoding replaces the editor's text with
    the disk's: neither is a change of the shown text. `file_properties` `convert_encoding` and
    `reload_encoding` keep refusing while the file has unsaved changes (the window's own File
    Encoding action asks the user, SPEC-0336 behaviours 13–15); `set_read_only` on a file with
    unsaved changes keeps refusing too, as before. [GEN]
22. **Deleting and renaming a file with unsaved changes stay refused.** `delete_file` would remove
    the file under an editor whose text exists nowhere else: the user's unsaved text would survive
    only as that editor's buffer, without a file to save to. `rename_file` renames the file on the
    disk, which the editor of the old name does not follow (as a moved file, behaviour 16): its next
    save would bring the old name back with the user's text. Both refuse before changing anything
    while the file (or, for a folder, a file inside it) has unsaved changes, naming the file and
    "save it first"; saved open files are deleted or renamed as before (SPEC-0058). They stay on
    the *refuses unsaved* list. [GEN]

## Acceptance criteria
- **AK-1** Unit: open, publish, save and close move revision and modified flag as stated; two views of
  one file keep the entry until both are closed; paths are resolved.
- **AK-2** Protocol: with an unsaved buffer in the registry, `read_file` over MCP returns the buffer and
  `open_documents` lists it as modified; after closing it, `read_file` returns the disk text again.
- **AK-3** Window self-test: typing into an open file without saving makes the file's entry modified and
  its text equal to the editor's.
- **AK-4** Protocol: with a file open with unsaved changes, a tool of the *through the buffer* list
  changes it through the window's edit: the disk is unchanged, the buffer holds the user's unsaved
  text and the agent's edit, the revision grew by one and the entry stays modified; one undo gives back
  exactly the user's text. With a saved open file the same edit is saved (disk equals the buffer, entry
  unmodified). An edit at a stale revision is refused and changes nothing. Unit: applying an edit to a
  buffer is one undo step.
- **AK-5** Protocol: a tool of the *refuses unsaved* list called on a file open with unsaved changes
  fails with a message naming the file and "save it first"; the disk and the entry are unchanged.
  Without an answer from the window, a buffer tool refuses a file with unsaved changes the same way.
- **AK-6** Ratchet test of behaviour 12.
- **AK-7** Protocol (with clangd installed): a file open with unsaved typing that adds a further usage
  is renamed through `rename_symbol`: every usage, the typed one included, is renamed in the buffer,
  the disk is unchanged, the revision grew by one and the entry stays modified; one undo in the editor
  gives back exactly the user's text. `undo_refactoring` after a second rename gives back the text
  before it through the buffer. A rename whose window request finds a file at another revision (the
  user typed in between) is refused and changes nothing.
- **AK-8** Protocol: a rename with text occurrences that changes two open files is refused as a whole
  when one of them changed in between: neither buffer and no disk file changed.
- **AK-9** Protocol: the LSP path of `format_file` on a file open with unsaved changes formats the
  shown text and leaves the disk unchanged. (v4) Unit: edits of another file computed on the text
  the planner was given apply to an open file with unsaved text different from the disk; an open
  file whose text changed since the plan is refused with "changed in the editor meanwhile".
- **AK-10 (v4)** Protocol (with clangd): a rename in `calc.cpp` whose function is declared in
  `calc.h`, open with unsaved typing that adds a further usage, renames every usage in both buffers,
  the typed one included, without "save it first" (clangd renames in another file only when that
  file is open on it, so the header's rename shows that it got the shown text); the header's disk
  is unchanged; one undo in each editor gives back exactly the user's text. Unit: while a call plans, Ferris IDEA's planners read the given
  texts in place of the disk (paths resolved), and the disk outside it. Protocol: `move_file` with reference updates of a file included by
  an open file with unsaved typing rewrites the include in that buffer, leaves its disk alone and
  moves the file; a moved file with unsaved changes is refused with "save it first".
- **AK-11 (v4)** Protocol: the window request of an agent's rename across files carries the
  refactoring's name and the files written to disk. Window test of the harness: see SPEC-0193 v5
  AK-9.

- **AK-12 (v5)** Protocol: `git_line_change` on a file open with unsaved typing lists the change the
  typing made, and `rollback` of a committed line's change takes it back in the buffer only: the
  disk is unchanged, the typing elsewhere stays, the entry stays modified and one undo gives back
  the user's text. `compare_files` `apply` changes an unsaved target in its buffer, computed from
  both shown texts.
- **AK-13 (v5)** Protocol: `git_rollback` of a file open with unsaved typing puts HEAD's text in the
  buffer, saves it and restores the index: disk, buffer and HEAD agree and the entry is unmodified;
  one undo gives back the typed text, unsaved. `git_resolve_conflict` with `take` of a file open
  with unsaved changes puts the result in the buffer, saves it and marks the conflict resolved.
  A rollback that deletes an added file with unsaved changes is refused with "save it first".
- **AK-14 (v5)** Protocol: `resource_bundle` `set` and `file_properties` `set_line_separator` change
  unsaved buffers from their shown texts and leave the disk alone; `convert_encoding` of a file with
  unsaved changes is refused with "save it first". A change of two open bundle files is refused as
  a whole when one of them changed in between. `rename_sql_object` with `apply` on an SQLite
  connection renames a column in an open `.sql` file with unsaved typing that adds a further
  reference: both references are renamed in the buffer, the disk is unchanged, one undo gives back
  the typed text.
- **AK-15 (v5)** Ratchet: the *refuses unsaved* list holds 13 tools, `delete_file` and `rename_file`
  among them, and the tools of behaviours 17–19 are on the *through the buffer* list.

## Out of scope
Edits through the buffer for the tools still on the *refuses unsaved* list (Git operations of the
whole working tree, deleting and renaming files, exports, encoding changes); a SQL rename whose
database statement and file edits are undone together; moving a file whose editor has unsaved changes
(its editor following the move); window undo groups for agent changes that rename files or span
several project windows; the other reading tools (search, diagnostics, symbols) reading open buffers
instead of the disk; all are open in M6.8. Events for document changes (readiness B3).

## Known deviations from the reference
None observed; the reference's behaviour for unsaved buffers over MCP was not examined. Refusing a
change computed from the disk while the file has unsaved changes (behaviour 9) is Ferris IDEA's own rule:
it never overwrites an editor's text or leaves the disk and the editor disagreeing behind the user's
back.

v6 (OBS-0071): the reference's MCP reading with an offset and a line limit returns the first and the
last lines of the rest of the file with a marker for what was left out between them, not the lines
asked for (12–14 came back as 12, 19 and 20). `read_file` returns exactly the 1-based inclusive range
of `start_line` and `end_line`, from the open buffer or the disk as behaviour 6 says (IMP-0440).
