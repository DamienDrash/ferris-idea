# SPEC-0331: Bookmarks

- **Version:** 3 · **Date:** 2026-11-14 (v3: Go to Mnemonic… (`ShowTypeBookmarks`) lists the bookmarks that have a mnemonic, behaviour 33, AK-11; sixth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-13 (v2: the menu bar entries are SPEC-0124 v7 behaviour 10,
  and the keys ⌃⇧0 … ⌃⇧9 reach their actions on every layout the toolkit describes by SPEC-0028 v5
  behaviour 10; no behaviour of this specification changes)
- **Sources:** OBS-0040 (setting a bookmark on the caret line, the popup for a mnemonic with its
  grid of characters, legend and description field, the list popup with a source preview, the
  tool window's tree and its order; facts only, in own words), OSS (the action ids
  `ToggleBookmark`, `ToggleBookmarkWithMnemonic`, `ToggleBookmark0`–`ToggleBookmark9`,
  `GotoBookmark0`–`GotoBookmark9`, `ShowBookmarks`, `ShowTypeBookmarks` and
  `ActivateBookmarksToolWindow` with their shortcuts in the ported keymaps, SPEC-0028;
  `docs/research/KEYMAP_COVERAGE.md`), GEN (a bookmark is a named place in a text that a reader
  returns to; everything OBS-0040 lists as not observed is decided here and marked GEN),
  SPEC-0091 behaviour 1 (breakpoints follow their line through edits), SPEC-0303 and ADR-0021
  (project files with an envelope), SPEC-0061 and SPEC-0167 (tool windows), SPEC-0069 (Recent
  Files and Recent Locations popups), SPEC-0067 v5 behaviour 12 (source preview), SPEC-0275 v4
  behaviour 28 (popups close on a click outside), SPEC-0053 (agent rights), SPEC-0319 (UI driver:
  declared effects), ADR-0008, ADR-0009
- **Feature matrix rows:** F-018

## Purpose
Mark lines of the project's files, optionally with a character that is a shortcut to them and
with a short description, and come back to them: from the gutter, by a key, from a list with a
preview, and from a tool window. Bookmarks stay with their line while the text changes and are
there again after a restart.

## Behaviour

### What a bookmark is
1. **A line of a project file:** a bookmark belongs to one line of one file below the project
   folder. A line has at most one bookmark. A file outside the project (a library source, a file
   opened from elsewhere), an editor without a file and a document of a remote project cannot be
   bookmarked; the status bar says so and nothing changes. [GEN]
2. **Mnemonic:** a bookmark may carry one character out of `1`–`9`, `0` and `A`–`Z` (36
   characters; letters are taken without regard to case and kept as capitals). A mnemonic is
   unique in the project: giving it to another line takes it from the bookmark that had it, which
   stays as a bookmark without mnemonic. [OBS-0040: the characters; GEN: the rest]
3. **Description:** a bookmark may carry a description: one line of text, at most 200
   characters, surrounding white space removed; line breaks become spaces; an empty description
   is no description. [OBS-0040: the popup has an optional field for it; GEN: the limits]
4. **Order:** bookmarks are listed newest first, by the time they were set. Changing the
   mnemonic or the description of a bookmark does not change its place in that order. [OBS-0040:
   the bookmark set last is on top; GEN: changes keep the place]

### Lines follow the text
5. **Edits in an editor:** while the file is open in an editor, a bookmark moves with its line
   (the mechanism of SPEC-0091 behaviour 1: every edit is applied to the marked lines as the line
   it starts on, the line breaks it removes and the line breaks it adds). Text inserted or removed
   above a bookmark shifts it by the difference in lines. A line break typed at the start of the
   bookmarked line, or after nothing but its indentation, moves the bookmark down with the line's
   text; one typed further right leaves it on the first part. [GEN]
6. **A deleted line removes its bookmark:** a bookmark is removed when the whole content of its
   line is deleted together with a line break next to it and nothing is put in its place: the
   line deleted as a whole, a line inside a deleted block, or a line whose end and whose line break
   before it are deleted. White space that is left before the deleted part counts as nothing
   left of the line. A line that is joined with its neighbour keeps its bookmark on the
   joined line. When two bookmarks come to lie on one line, the one of the upper line stays.
   Taking the deletion back with Undo does not bring the bookmark back. [GEN]
7. **A replaced line keeps its bookmark:** when an edit replaces several lines by other text (a
   reformat, a refactoring), a bookmark on a line that was replaced as a whole goes to the nearest
   line of the new text that has the same content apart from white space at its ends; without such
   a line it stays at the same distance from the start of the replaced part, at most on its last
   line. [GEN]
8. **Changes outside the editor are not followed:** a bookmark of a file that is not open keeps
   its line number when the file changes on disk. A bookmark whose line is past the end of the file
   is shown with an empty line text; jumping to it goes to the last line. A bookmark whose file is
   missing is kept but not listed, and shows again when the file is back (a branch switch). [GEN]
9. **Renamed, moved and deleted files:** a file or folder renamed or moved inside the project by
   IDECraft (project tree, Move, Rename with its type, and taking such a refactoring back) keeps
   its bookmarks under the new path. Deleting a file or folder through IDECraft removes its
   bookmarks. [GEN]

### Storage
10. **Per project, in a file with an envelope:** the bookmarks are kept in
    `.idecraft/bookmarks.toml`, a project file of kind `bookmarks`, format 1 (SPEC-0303 behaviours
    1–3 and 14): one `[[bookmark]]` table per bookmark, newest first, with `file` (relative to the
    project, `/` between the parts), `line` (1-based), and optionally `mnemonic` and
    `description`. The schema is published as `docs/formats/bookmarks.schema.json`. The file is
    written after every change and removed when the last bookmark goes. A file of a newer format
    or of another kind is neither read nor overwritten. [GEN, SPEC-0303]
11. **Reading is strict about what it keeps:** entries are dropped when their `file` is absolute
    or leaves the project (`..`), when their `line` is 0, or when the line already has a bookmark
    earlier in the file; a mnemonic outside the 36 characters, or one an earlier entry has, is
    dropped and its bookmark kept. At most 10 000 bookmarks are read. [GEN]
12. **After a restart** the bookmarks are there as they were saved: lines, mnemonics,
    descriptions and order. [GEN]
13. **Another writer:** when the file changes on disk while the project is open (an agent through
    MCP, behaviour 30), the window reads it again within one second and shows the new state in
    the gutter, the popup and the tool window. [GEN]

### Editor
14. **Gutter:** a bookmarked line shows a mark in place of its line number: a small rounded
    square in the theme's accent colour with the mnemonic character in it, empty for a bookmark
    without mnemonic. No icon file is added for it. A line that also has a breakpoint shows the
    breakpoint. The mark's tooltip says "Bookmark", and below it the mnemonic and the
    description. A click on
    the mark does what a click in the gutter does anywhere else (SPEC-0091). [GEN; the reference's
    mark was not observed]
15. **`ToggleBookmark`** (F3): sets a bookmark without mnemonic on the line of the caret, without
    a question; on a line that has a bookmark, with or without mnemonic, it removes it. With
    several carets the primary caret's line counts. [OBS-0040: sets without a question; GEN:
    removing]
16. **`ToggleBookmarkWithMnemonic`** (⌥F3) opens a small popup for the caret's line: the 36
    characters as a grid (`1`–`9`, `0`, then `A`–`Z`), the character of this line's bookmark and
    the characters other bookmarks have marked differently and explained in a legend, and a field
    for the description, filled with the line's description when it has one.
    - The grid has the focus at first, and the character of this line's bookmark is selected.
    - Typing one of the characters while the grid has the focus, or a double click on it, sets the
      bookmark with that character and closes the popup.
    - Arrow keys or a click select a character (ten in a row, the digits first); ⌫ or ⌦ in the
      grid selects none; Tab moves between the grid and the description field; Enter sets the
      bookmark with the selected character (none selected: without mnemonic) and the
      description, and closes the popup.
    - Escape, or a click outside, closes the popup and changes nothing.
    A line that already had a bookmark keeps it and gets the new mnemonic and description; a
    character that another bookmark has is taken from it (behaviour 2). [OBS-0040: grid, legend,
    description field, typing, double click, Enter with a description; GEN: focus, Tab, arrows]
17. **`ToggleBookmark0` … `ToggleBookmark9`** (⌃⇧0 … ⌃⇧9): when the caret line's bookmark has
    that digit, the bookmark is removed; otherwise the line gets a bookmark with that digit, or its
    bookmark gets the digit (behaviour 2). [GEN]
18. **`GotoBookmark0` … `GotoBookmark9`** (⌃0 … ⌃9): opens the file of the bookmark with that
    digit and puts the caret at the start of its line; the place left is remembered for Back
    (SPEC-0071). Without such a bookmark the status bar says that no bookmark has this mnemonic.
    [GEN]
19. **`GotoNextBookmark`, `GotoPreviousBookmark`** (no shortcut in the keymap): move the caret to
    the next bookmark below, or the previous one above, the caret's line in the current file, and
    go on from the other end of the file when there is none. With no bookmark in the file the
    status bar says so. [GEN]

### List popup
20. **`ShowBookmarks`** (⌘F3) opens a popup titled "Bookmarks" with every listed bookmark
    (behaviours 4 and 8), one row each: the mnemonic, the file's name, the line number and the
    line's text without the white space at its ends — the description in place of the text when
    there is one — and beside the list a read-only preview of the source around the selected
    bookmark (six lines before and eight after it) with its line marked, coloured by the file's
    grammar. The text comes from the open editor when the file is open, else from disk. The
    first row is selected. [OBS-0040: title,
    list and preview on the right; GEN: the row's parts]
21. **Keys in the popup:** Up and Down select (and wrap around); Enter, or a double click, jumps
    to the selected bookmark as behaviour 18 does and closes the popup; a typed mnemonic character
    jumps to the bookmark that has it; Delete (⌫ or ⌦) removes the selected bookmark and keeps the
    popup open; Escape, or a click outside, closes it. Without bookmarks the popup says so and how
    to set one. [GEN]

### Tool window
22. **"Bookmarks" tool window** (`ActivateBookmarksToolWindow`, ⌘2): a tool window of the layout
    with the shared header (SPEC-0061 v5 behaviour 18), docked left by default, reached through the
    More tool windows list until the user adds it to the sidebar (SPEC-0061 v10 behaviours 51–52).
    The action shows and focuses it, and hides it when it has the focus, like the other tool
    windows. [OBS-0040: the window and its action; GEN: placement and stripe]
23. **Tree:** the project as the top node; below it one node per file that has listed bookmarks,
    with the file's name and, dimmed, its folder relative to the project; below a file one row per
    bookmark: `line: text` with the mnemonic in front (the description in place of the text when
    there is one). Bookmarks of a file are ordered newest first, files by their newest bookmark.
    All nodes are open at first. [OBS-0040: the three levels, the folder, `line:` and text, the
    mnemonic, newest on top; GEN: the order of files]
24. **Keys and mouse in the tree:** Up and Down select; Left closes a node or goes to its parent,
    Right opens it; Enter or a double click on a bookmark jumps to it (behaviour 18), on a file
    opens the file, on the project opens or closes the node; Delete (⌫ or ⌦) on a bookmark removes
    it, on other nodes it does nothing; F2 or ⇧F6 on a bookmark opens a popup with one field for
    its description — Enter saves it (empty: no description), Escape leaves it. Without bookmarks
    the window says so and names the shortcut of `ToggleBookmark`. [GEN]
25. **Everything shows the same state:** gutter, popup and tool window follow every change at
    once, whoever made it. [GEN]

### Find Action, UI driver
26. **Titles:** Find Action lists "Toggle Bookmark", "Toggle Bookmark with Mnemonic", "Show
    Bookmarks", "Bookmarks" (the tool window), "Go to Next Bookmark", "Go to Previous Bookmark",
    "Go to Bookmark 0" … "Go to Bookmark 9" and "Toggle Bookmark 0" … "Toggle Bookmark 9", each
    with its shortcut. [GEN]
27. **Declared effects (SPEC-0319 behaviour 35):** the toggle actions change a file of the
    workspace (`.idecraft/bookmarks.toml`) and are declared with the effect `write`; the go-to
    actions, the popup and the tool window are declared with no effect. The rows of the popup and
    of the tool window are annotated elements of the semantic tree. [SPEC-0319, GEN; security
    first: the stricter effect]

### MCP (ADR-0008)
28. **`bookmarks`** lists the bookmarks, newest first: `path` (relative to the workspace),
    `line` (1-based), `mnemonic`, `description`, `text` (the line's text from disk, without white
    space at its ends; null when the line is past the end) and `missing: true` for a bookmark
    whose file is not there. [GEN]
29. **`set_bookmark`** takes `path`, `line`, and optionally `mnemonic` and `description`: it sets
    a bookmark on that line of a file inside the workspace, or changes the one that is there (an
    omitted field keeps its value, an empty string removes it). The line must exist in the file.
    `dry_run` defaults to true and returns the bookmark as it would be saved and, when the
    mnemonic is taken, the place it would be taken from. With `dry_run: false` it needs the
    user-granted `write` right, like the other tools that change files, and is logged in
    `.idecraft/agent-log.jsonl` (tool, path, line, mnemonic; not the description). [GEN, SPEC-0053]
30. **`remove_bookmark`** takes `path` and `line`, or `mnemonic`; it fails when there is no such
    bookmark. The file need not exist any more. `dry_run`, the `write` right and the log are as
    for `set_bookmark`. An open window shows the change (behaviour 13). [GEN, SPEC-0053]
31. **`dialog_decisions`** (SPEC-0281) lists the mnemonic popup with the tools that make its
    decisions: `set_bookmark` and `remove_bookmark`. [SPEC-0281]
32. **Files moved by agents:** `move_file` and the other tools that rename or move files carry
    the bookmarks along as behaviour 9 says. [GEN]
33. **Go to Mnemonic… (v3):** `ShowTypeBookmarks` (⌥⌘F3 in the bundled macOS keymap,
    Ctrl+Shift+F11 in the default one; Edit › Bookmarks › Go to Mnemonic…) lists the
    bookmarks that have a mnemonic, ordered by it, digits before letters, one row each: the
    mnemonic, `file name:line` and the description or the line's text. Enter opens the selected
    bookmark as `GotoBookmark0`…`9` does (behaviour 18), Escape closes the list. Bookmarks whose
    file is gone are left out; without a bookmark with a mnemonic the status bar says so and no
    list opens. [OSS: id, title, keys and "the bookmarks with a mnemonic, in a popup, Enter
    navigates" from `ShowTypeBookmarksAction.kt`, logic template. In the open platform a key of
    a mnemonic also jumps from the open list; IDECraft's list takes arrows and Enter only]

## Acceptance criteria
- **AK-1** Given bookmarks on lines 7, 12 and 16 of a file, when two lines are inserted above
  line 12 and then line 7 is removed as a whole, then the bookmarks are on lines 13 and 17 (their
  original text), and the one of line 7 is gone. (Behaviours 5, 6)
- **AK-2** Given a bookmarked line, when a line break is typed at its start or after its
  indentation the bookmark follows the text down; when it is typed in the middle the bookmark
  stays; when the line is joined to the line above the bookmark is on the joined line; when a
  block of lines is replaced, a bookmark inside goes to the line with the same text, else stays
  inside the replaced part. (Behaviours 5–7)
- **AK-3** Given a bookmark with mnemonic `3` on one line, when `3` is given to another line,
  then that line has it and the first bookmark is still there without mnemonic; letters are
  accepted in either case and kept as capitals; other characters are refused. (Behaviour 2)
- **AK-4** Given bookmarks with mnemonics and descriptions, when they are saved and the project
  is read again, then lines, mnemonics, descriptions and the order newest first are the same; the
  file has the envelope `kind = "bookmarks"`, `format = 1`; removing the last bookmark removes the
  file; entries that leave the project, repeat a line or repeat a mnemonic are handled as
  behaviour 11 says; the published schema matches the code. (Behaviours 4, 10–12)
- **AK-5** Given a bookmarked file, when it or a folder above it is renamed or moved inside the
  project through IDECraft's file operations, then its bookmarks are under the new path; when it
  is deleted that way they are gone. (Behaviour 9)
- **AK-6** `ToggleBookmark` on a line without bookmark sets one and on the same line again removes
  it; `ToggleBookmark3` sets, moves and removes the mnemonic as behaviour 17 says; the next and
  previous bookmark of a file wrap around. (Behaviours 15, 17, 19)
- **AK-7** The rows of the popup and of the tool window: file, line, text or description,
  mnemonic; newest first; the tree has the project, the files with their folders and the
  bookmarks; a missing file is not listed. (Behaviours 8, 20, 23)
- **AK-8** Over MCP: `set_bookmark` with `dry_run` omitted changes nothing; with
  `dry_run: false` and the `write` right it sets the bookmark, `bookmarks` lists it with its
  line text, a second one with the same mnemonic takes the mnemonic over, `remove_bookmark`
  removes it; without the `write` right nothing is changed; the changes are in the agent log.
  (Behaviours 28–30)
- **AK-9** Find Action finds the titles of behaviour 26; the 25 action ids of the reference's
  keymap for bookmarks except `ShowTypeBookmarks` count as present in
  `docs/research/KEYMAP_COVERAGE.md` with the same shortcuts; the toggle actions are declared with
  the effect `write`. (Behaviours 26, 27)
- **AK-10** In the window: the gutter mark, the mnemonic popup, the list popup with its preview
  and the tool window behave as behaviours 14–25 say. (Hand test and window self-test; open.)
- **AK-11 (v3, behaviour 33)** The rows: only bookmarks with a mnemonic, in its order, with file,
  line and text (`bookmarks_model::tests::go_to_mnemonic_lists_the_mnemonics_in_order`); in a
  window of the test harness the action lists them, Enter opens the chosen one at its line,
  and without a mnemonic the status bar says so
  (`sixth_package_keymap.rs::go_to_mnemonic_lists_and_opens`). On screen: not checked.

## Out of scope
- Bookmarks on files and folders in the Project tree, and bookmarks of anything but a line.
- Several bookmark lists, a default list, moving bookmarks between lists.
- Sorting and grouping options of the tool window, its toolbar, drag and drop of bookmarks, a
  preview inside the tool window.
- `ShowTypeBookmarks` (⌥⌘F3): what it shows was not observed.
- Go-to and toggle actions for the letter mnemonics as actions of their own: the keymap names
  only the digits; letters are reached through the popups.
- A click on the gutter mark with a behaviour of its own, a context menu on it, dragging it.
- Following changes made to a file outside the editor (behaviour 8), and bringing a removed
  bookmark back with Undo (behaviour 6).
- Bookmarks in a remote project and in files outside the project.
- A bookmark icon: the bundled icon set has none; the open New UI bookmark icons can be copied
  later under APACHE_PORTING.md. Until then the gutter mark is drawn (behaviour 14) and the
  window's stripe button, which shows while the window is shown, uses the bundled tag icon.
- Entries in context menus. (The menu bar: SPEC-0124 v7 behaviour 10, Edit › Bookmarks.)

## Known deviations from the reference
- Everything marked GEN above was decided without an observation of the reference; OBS-0040
  lists it as not observed (gutter mark, jumping by mnemonic, next and previous, renaming,
  deleting, what edits and a restart do). A later observation may change these behaviours.
- The store is a readable project file with a published schema, and agents reach bookmarks
  through MCP tools with a dry run and the `write` right (ADR-0008); the reference keeps them in
  its own workspace state.
- The tool window is reached through the More tool windows list by default, because IDECraft
  has no bookmark icon of its own for a stripe button yet.
