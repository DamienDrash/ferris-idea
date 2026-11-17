# SPEC-0058: Editor view

- **Version:** 5 · **Date:** 2026-11-16 (v5: the gutter icons answer to keys, `EditorFocusGutter`, behaviour 13, AK-8; twelfth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-15 (v4 after OBS-0065 S1: Save All saved only the editor in
  front; it now saves every editor of the window with unsaved changes, behaviour 12, AK-7; v3: saving when the window loses the focus is a setting, SPEC-0337 behaviour 2; v2: saving with local history, changes on disk)
- **Sources:** GEN (how IDE editors draw and handle a document: gutter with line numbers, caret row,
  selections, carets, mouse selection, input methods; F-010), SPEC-0057 (editor commands), SPEC-0028
  (keymaps), SPEC-0027 (syntax highlighting), SPEC-0044 (local history), SPEC-0026 (reconciling disk and
  buffer), ADR-0003 (GPUI), ADR-0009 (look and feel); v4: GEN (Save All of IDEs saves every
  document with unsaved changes), SPEC-0112 (split groups), SPEC-0261 behaviour 3 (Save All when
  quitting), SPEC-0317 v3 behaviour 10 (saving remote documents), SPEC-0336 (read-only files)
- **Feature matrix rows:** F-010, F-018

## Purpose
The window part users type into: it draws a buffer and turns keys, mouse and input-method events into
the editor commands of SPEC-0057.

## Behaviour
1. **Gutter:** line numbers right-aligned in a gutter wide enough for the last line number (at least
   three digits); the caret line's number is brighter. [GEN]
2. **Text:** a monospaced font; tabs are drawn up to the next multiple of four columns; only the visible
   lines are laid out, so a 100 000-line file opens and scrolls without delay. [GEN]
3. **Colours:** syntax colours come from the syntax tree (SPEC-0027). The tree is re-parsed on a
   background thread after edits; until it is current, each line keeps the colours last seen for the same
   text, so typing never waits for a parse. Colours are an interim dark scheme until the theme files of
   M3.8 are loaded. [GEN, ADR-0009]
4. **Carets and selections:** every caret is drawn as a bar, every selection as a background behind the
   text (a selected line break extends one character width); the caret row has its own background when
   the primary caret has no selection. Carets are drawn only while the editor has focus. [GEN]
5. **Keys:** a key event becomes a keymap keystroke (`Keystroke::from_platform`) and is looked up in the
   bundled keymap of the platform (macOS or default); the first editor command bound to it runs
   (SPEC-0057), and the event then goes no further. Keys without an editor command reach the input
   method, which types text through `type_text` (bracket pairs, all carets). While the input method
   composes text, keys belong to it. `$Copy`/`$Cut` also write the system clipboard; `$Paste` reads it;
   `SaveDocument` writes the file, `SaveAll` every modified file of the window (v4, behaviour 12).
   [GEN, OSS]
6. **Scrolling:** wheel and trackpad scroll by pixels; the last line can scroll up to the top. After a
   command the primary caret is scrolled into view with as little movement as possible; page commands
   move the view by their page. [GEN]
7. **Mouse:** a click puts the caret, Shift+click extends the primary selection, Alt+Shift+click adds a
   caret, a double click selects the word, a triple click the line with its break, and dragging extends
   the selection. [GEN]
8. **Input methods:** the view tells the platform the selection, composed range and text in UTF-16 code
   units (without building the whole text), places the candidate window at the composed text, underlines
   composed text, and replaces it on commit. [GEN]
9. Commands: `idecraft edit <file>` opens a file in the view; `idecraft spike-editor [file]` types 220
   keystrokes (text with umlauts, Backspace, Down, Enter) into line 50 000 of a 100 000-line file, one per
   frame, and reports the time from each keystroke to the painted frame. The MCP counterparts of the view
   are `editor_keystrokes` (SPEC-0057) and `highlight_file` (SPEC-0027). [ADR-0008]

10. **Saving:** `SaveDocument` (and Save All for each file, behaviour 12) writes the buffer and records the file's previous and new content
    in the workspace's local history (SPEC-0044). If another program changed the file since the editor read
    or last saved it, the first save is refused with a message in the status bar and the second save
    overwrites the disk version; a write error is reported there too. [GEN, SPEC-0044]
11. **Changes on disk:** every 2 seconds the open files are compared with what the editor last read or
    wrote (modification time and size first, then the text). An editor without unsaved changes reloads the
    disk version as one undo step and says so; an editor with unsaved changes keeps its text and warns that
    saving will overwrite the disk version (SPEC-0026 behaviour of `reconcile`). [GEN]
12. **Save All (v4):** `SaveAll` — from the File menu, its key (⌘S on macOS, Ctrl+S elsewhere) or
    Find Action, with the focus in an editor or in a tool window — saves every editor of the
    project window that has unsaved changes, in every split group (SPEC-0112), not only the editor
    in front. [GEN]
    a. Each file is saved once, the way `SaveDocument` saves it (behaviour 10): the on-save edits
       of Settings and EditorConfig (SPEC-0129 behaviour 4, SPEC-0137 behaviour 3), local history
       and the changed-on-disk check. A file shown in both groups is saved from one view; the other
       view takes the saved text. The editor in front is written as before also when it has no
       unsaved changes. [GEN]
    b. A read-only file is not written (SPEC-0336); a read-only editor in front does not keep the
       other files from being saved. [GEN]
    c. A file that could not be written, or that changed on disk, stays modified and its editor
       says why, as when quitting (SPEC-0261 behaviour 3); the next Save All tries it again, and
       the second save of a file changed on disk overwrites it (behaviour 10). [GEN]
    d. In a remote project every modified document is saved on the host the way ⌘S saves one
       (SPEC-0317 v3 behaviour 10), each with its own conflict check and second save. [GEN]
    e. `SaveDocument` still saves only its own editor. An editor outside a project window
       (`idecraft edit`, behaviour 9) saves its own file on `SaveAll`. [GEN]
13. **Focus Gutter (v5):** `EditorFocusGutter` (⌥⇧6 then F in the bundled keymaps) gives the keys
    to the gutter icons drawn in the visible part of the editor — run markers, breakpoints,
    folding arrows, bookmarks and the marks of declarations and beans (SPEC-0294 behaviour 4) —
    starting at the first icon of the caret's line, else of the nearest line below, else of the
    last line above. The icon that has the keys gets a ring in the accent colour and keeps its
    tooltip (SPEC-0294). ↑ and ↓ go to the previous and next line with icons, ← and → along the
    icons of a line, left to right; they stop at the ends. Enter and Space do what a click on the
    icon does: run what a ▶ marks, remove the breakpoint, fold or unfold, go to the other side of
    a declaration's or a bean's mark; a bookmark has only its tooltip. Escape gives the keys back
    to the text; any other key does too and then acts as it would. Without a drawn icon the
    status bar says "No gutter icons in the visible part of the editor". A one-line field has no
    gutter. [GEN]

## Acceptance criteria
- **AK-1** Tab expansion with mapping between columns and display bytes, gutter width, capture colours
  and scroll arithmetic (unit tests).
- **AK-2** Keystrokes from toolkit key names with modifiers (unit test in `idecraft-ui`).
- **AK-3** Buffer change log and UTF-16 mapping used by the view (unit test in `idecraft-editor`).
- **AK-4** Measurement on 100 000 lines: keystroke to painted frame p95 below 16 ms on Apple M1 Max.
- **AK-5** Mouse, input method and clipboard checked by hand in the window.
- **AK-6** In a real window (`idecraft ui --selftest`): an unmodified editor reloads a change made by
  another program; a modified one keeps its text and warns; saving then writes the editor's text and the
  local history holds the revisions.
- **AK-7** (v4) Window test in the test harness (behaviour 12): two files changed in two tabs are
  both on disk after `SaveAll` through the window's key dispatch (⌘S), through the File menu and
  through Find Action, also with the focus in the Project tree; with a file in each split group
  and with one file shown in both groups every change is on disk, each file saved once; the
  EditorConfig on-save edits apply to the file behind the one in front; a file changed on disk
  stays modified with its editor's note while the others are saved, and the next Save All writes
  it; a read-only file in front does not stop the others; `SaveDocument` writes only its own file.
- **AK-8** (v5, behaviour 13) Unit test
  `editor_view::gutter_focus::tests::the_gutter_icons_are_walked_by_line_and_place` (icons by
  line and place, start line, moves stop at the ends); window test
  `crates/workbench/tests/it/twelfth_package_keymap.rs`: `the_gutter_icons_answer_to_keys`
  (macOS; written, not yet built).

## Out of scope
Soft wraps, folding in the view, inlay hints, error stripes and gutter icons, minimap, tabs and the
tool windows around the editor (rest of M3.7), caret blinking, font settings, ligatures.

## Known deviations from the reference
Not yet compared side by side. (v3) Saving when the window loses the focus is a setting, off by
default (the reference saves automatically by default; reason in SPEC-0337); a conflict is shown in the status bar instead of a dialog. Interim colours and font (Menlo / DejaVu Sans Mono) until M3.8; the caret
does not blink.
- **Focus Gutter (v5):** only the icons drawn on screen take the keys; ↓ at the last visible
  line with icons does not scroll to icons further down. A breakpoint's Enter removes it as a
  click does; its properties stay with the breakpoint dialog (SPEC-0127).
