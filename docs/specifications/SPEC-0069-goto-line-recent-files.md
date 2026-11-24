# SPEC-0069: Go to Line and Recent Files

- **Version:** 5 · **Date:** 2026-11-23 (v5: a navigation elsewhere closes Go to Line, Recent Files
  and Recent Locations, and Escape closes them wherever the focus is, behaviour 10, AK-7; OBS-0119 K1);
  2026-11-14 (v4: the generated ignore file also lists the window state of one user — open tabs, terminal tabs, pinned run configurations — and caches, and is extended line by line, behaviour 3a, AK-6; window check of 2026-11-14; v3 2026-11-13: Recently Changed Files and the edited-only key under the ids of the bundled keymaps, behaviour 9, AK-5; docs/research/KEYMAP_COVERAGE.md; v2 2026-11-06: side-by-side comparison OBS-0023 — current file first,
  tool windows, edited only, footer, Recent Locations)
- **Sources:** GEN (a dialog jumps to `line:column`; a popup lists recently opened files; F-010, F-020), OSS
  (action ids `GotoLine` and `RecentFiles` of the ported keymaps; the reference keeps its machine-local
  project state out of version control with `.idea/.gitignore`), SPEC-0058 (editor view), SPEC-0059 (tabs)
- **Feature matrix rows:** F-010, F-020, F-016 (v2)

## Purpose
Jump to a position and back to recently used files from the keyboard.

## Behaviour
1. **Go to Line:** `GotoLine` (⌘L on macOS, Ctrl+G elsewhere) opens a small popup with the caret's
   current `line:column` selected. Accepted input: `line`, `line:column`, `line,column`, `:column`
   (1-based; a missing line keeps the current one, a missing column means column 1; 0 counts as 1). [GEN, OSS]
2. Enter puts the caret there, clamped to the text, scrolls it into view and returns to the editor;
   invalid input does nothing; Escape closes. [GEN]
3. **Recent files:** every file opened in a tab moves to the front of the workspace's recent list (at most
   50, kept in `.ferris-idea/recent-files`, missing files dropped when read). When Ferris IDEA first writes
   machine-local state into `.ferris-idea/`, it adds `.ferris-idea/.gitignore` listing `history/`,
   `recent-files` and `agent-log.jsonl`, so such state does not appear as a change to commit. [GEN, OSS]
   - **3a. What the file lists, and how it grows (v4).** `.ferris-idea/` holds project files that
     are meant for version control (ADR-0021 class "Project": TOML, read and reviewed by people)
     and state of one user or one machine (class "State": machine-written, JSON or logs, caches).
     The generated file lists the state: besides the entries above and those other specifications
     name (`tool-windows.json`, `branch-favorites.toml`, `commit-messages.toml`,
     `changelists.toml`, test reports, `run/`) these are `editor-tabs.json` (the tabs open in
     this window, SPEC-0059 behaviour 22e), `terminal-sessions.json` (the terminal tabs with
     their folders and up to 1,000 lines of each shell's history, SPEC-0205 behaviour 6 — text
     that may hold secrets), `run-pins.json` (the pinned run configurations of this user),
     `profiles/` (profiler output, ADR-0021 decision 5), `deps/` (resolved dependency trees kept
     until a build file changes, SPEC-0149 behaviour 4) and `browser-profile/` (the browser
     profile of an earlier version where it could not be moved away, SPEC-0133). The file is
     made sure of before each of `editor-tabs.json`, `terminal-sessions.json` and
     `run-pins.json` is written, as before `recent-files`. A generated file — one whose first
     line is still the generated comment — that lacks entries gets the missing ones appended,
     each on a line of its own; every line already in it, also one the user added, stays as it
     is and where it is. A file whose first line the user replaced is theirs and is not touched.
     Before v4 a generated file that differed from the current list was replaced as a whole,
     which dropped lines a user had added below the comment. [GEN, Security]
4. **Recent Files popup:** `RecentFiles` (⌘E / Ctrl+E) lists the recent files, most recent first, with
   the file shown in the editor last, so Enter right away switches to the previous file. Typing filters by
   file name (fuzzy, SPEC-0031). Up/Down move with wrap-around; Enter or a click opens the file. [GEN, OSS]
5. Escape closes either popup and returns to the editor. [GEN]
6. The MCP counterpart for positions is `editor_keystrokes` / `editor_action` (SPEC-0057); recent files
   are a window convenience without an MCP tool. [ADR-0008]

**v2 (OBS-0023 L20–L23).**
4a. **Order and selection:** the file in the editor comes first and the previous file is selected, so
   Enter right away still switches back; a typed filter selects the best match. The selection spans
   the whole row. [GEN; OBS-0023 item 7]
5a. **Tool windows:** a left column lists the tool windows with their shortcuts (Project, Find,
   Terminal, Problems, Structure, Services, Version Control, Commit, Run, Debug, Database) and "Recent
   Locations" at the end; ← and → (with an empty filter) move between the columns, Enter or a click
   opens the window. [GEN; OBS-0023 item 7]
6a. **Edited only:** a checkbox and ⌘E (pressed again inside the popup) list only the files changed in
   this session. Under the list the selected file's folder is shown, with `~` for the home folder.
   [GEN; OBS-0023 item 7]
7. **Recent places:** the window remembers up to 50 places (file, line, column, time, changed or
   not), newest first: the place left by a jump (SPEC-0071) or by opening another file, and the caret's
   place after each change of text. A place within three lines of a known one in the same file
   replaces it and keeps its "changed" mark. [GEN]
8. **Recent Locations:** `RecentLocations` (⇧⌘E / Ctrl+Shift+E) lists those places without the one at
   the caret, each with the file name, the declarations around it (`PriceFormatter > format()`), how
   long ago ("just now", "5 minutes ago"), "changed" for edited places, and the place's line with one
   line before and two after in syntax colours with line numbers; the title counts them. Typing filters
   by path, declaration or text; ⇧⌘E inside the popup (or its checkbox) lists only changed places;
   Enter or a click opens the file at the place. The places live as long as the window. [GEN;
   OBS-0023 item 7]
9. **Recently changed files (v3):** `RecentChangedFiles` (no key in the bundled keymaps; View menu and Find Action, titled Recently Changed Files) opens the Recent Files popup of behaviour 4 with "Show edited only" on, so it lists the files changed in this session (behaviour 6a); the checkbox and its key switch back to all files. The key that flips "Show edited only" inside the Recent Files popup is the shortcut of `SwitcherRecentEditedChangedToggleCheckBox` in the keymap in use (⌘E on macOS, Ctrl+E elsewhere, the same chord that opened the popup), no longer a fixed key: a shortcut the user gives the action in Settings › Keymap works in the popup. Outside the popup the action does nothing. Recent Locations keeps ⇧⌘E for its own checkbox. `SwitcherIterateItems`, bound to the same chord, is not carried out: what it steps through is not known from its name. No MCP tool: the lists are window state, as before (behaviour 6). [OSS: ids and shortcuts of the bundled keymaps; GEN]
10. **Closing (v5):** while Go to Line, Recent Files or Recent Locations is open, a navigation that
   moves the caret to another place or file — Back and Forward, Go to Declaration and the other
   jumps that leave a place for Back — closes the popup without a choice, as Escape does: the popup
   belongs to the place it was opened at. Escape closes it also when the keyboard focus is no
   longer in its field (a click into the editor, a navigation that took the focus); with the focus
   in the field the popup handles Escape itself as before. The name popups that share the popup's
   form (Rename, Change Signature, Move; SPEC-0074, SPEC-0113, SPEC-0114) are not closed by a
   navigation, since they hold a name being typed. Before v5 the popup stayed open after ⌘B and
   ⌘[ and took Escape only after a click into it (OBS-0119 K1, reproduced in a window of the test
   harness with keys sent through the window, without an input method). [OBS-0119, GEN]

## Acceptance criteria
- **AK-1** Parsing of `line[:column]` variants and invalid input; recent order, current file last, filter,
  persistence with missing files dropped, `.gitignore` written (unit tests).
- **AK-2** In a real window (`ferris-idea ui --selftest`): ⌘L `1,4` Enter puts the caret at 1:4; ⌘E lists
  the current file last and Enter opens the previous file; the list is on disk.
- **AK-3** Use by hand.
- **AK-6 (v4)** The generated list names the window state and caches of behaviour 3a; a generated
  file of an earlier version gets the missing entries appended and keeps a line the user added;
  a file without the generated comment is left alone; writing the open tabs makes sure of the
  ignore file (unit tests).
- **AK-7 (v5, behaviour 10)** In a window of the test harness, by keys sent through the window
  (`crates/workbench/tests/it/keymap_gaps_obs0119.rs::go_to_line_closes_on_navigation_and_on_escape`):
  ⌘L `6` Enter jumps; ⌘L again, then ⌘[ goes back to line 1 and the popup is closed; ⌘L, the
  focus put into the editor, Escape closes the popup.

- **AK-4 (v2)** Current file first (unit test `goto_model::tests::recent_order_filter_and_persistence`);
  recent places merge, keep the changed mark, leave out the caret's place, time in words (unit test
  `nav_model::tests::recent_places_merge_and_filter`); in the window (`--selftest`): ⌘E selects the
  previous file, ⇧⌘E lists a place left by a jump; OBS-0023 Nachprüfung.
- **AK-5 (v3, behaviour 9)** The popup's toggle key comes from the keymap, in the macOS and in the default keymap, and a chord of another action is not it (unit test `keymap_actions::tests::the_edited_only_key_is_the_actions_shortcut`); title and menu place (`everywhere::tests::navigation_actions_from_the_keymap_have_titles`, `menus::tests::navigation_actions_from_the_keymap_are_in_their_menus`). In the window (the popup opened with only the changed files): open.

## Out of scope
Removing entries from the list, keeping recent places across restarts, recently changed files kept
across restarts (v3 lists the files changed in this session).

## Known deviations from the reference
The reference keeps its recent list inside its project directory's workspace file; Ferris IDEA keeps a
plain list in `.ferris-idea/recent-files`. Recent Locations shows a filter field (the reference filters by
typing without a field). Recent Files and Recent Locations are window conveniences without an MCP
tool, as before; agents navigate with `read_file` and the editor tools.
