# SPEC-0057: Editor commands by keymap action

- **Version:** 8 · **Date:** 2026-11-14 (v8: where undo and redo go by keyboard focus, input
  fields with a history of their own, the history of a file outlives its tab; behaviours 29–31,
  AK-28 to AK-31, from the side-by-side comparison OBS-0042. v7: undo and redo put every caret and selection back,
  and a read-only text takes neither; behaviour 9, from the check of feature row F-011 without a
  screen. v6: `EditorCutLineEnd`, `EditorToggleInsertState`,
  `AutoIndentLines`, `EditorContextInfo`; behaviours 24–28, AK-19 to AK-26; overwrite mode is no
  longer out of scope. v5: `HippieCompletion` and `HippieBackwardCompletion`,
  behaviour 23. v4: `EditorMatchBrace`, `EditorScrollToCenter`; behaviours
  21–22. v2: consecutive typing is one undo step; from hand test 13. v3:
  side by side in OBS-0018 — selection by syntax, word stops, Smart Home in the indentation,
  Backspace to the indentation, Tab without selection, undo of repeated deletes, paste with
  indentation and paste from history; behaviours 13–20)
- **Sources:** OSS (action ids of the ported default keymaps `$default.xml` and `Mac OS X 10.5+.xml`,
  SPEC-0028), GEN (what the everyday caret, selection and clipboard commands of IDE editors do; F-010),
  SPEC-0003 (buffer and undo), SPEC-0025 (carets), SPEC-0056 (editing actions); v6: OSS — the open
  platform's actions, `JetBrains/intellij-community` at the pinned commit `d4018fb` of
  APACHE_PORTING.md (PROVENANCE.md: logic template, no code):
  `platform/platform-impl/src/com/intellij/openapi/editor/actions/CutLineEndAction.java` (the
  range a caret cuts, one caret and several),
  `platform/platform-impl/src/com/intellij/openapi/editor/actions/KillRingUtil.java`,
  `platform/platform-api/src/com/intellij/openapi/ide/KillRingTransferable.java` and
  `platform/platform-impl/src/com/intellij/ide/CopyPasteManagerWithHistory.java` (line breaks of
  the cut text, cuts in a row joined on the clipboard),
  `platform/platform-impl/src/com/intellij/openapi/editor/impl/EditorCopyPasteHelperImpl.java`
  (the texts of several carets, one per line),
  `platform/platform-impl/src/com/intellij/openapi/editor/actions/ToggleInsertStateAction.java`
  and `platform/platform-impl/src/com/intellij/openapi/editor/impl/EditorImpl.java` (the mode is a
  state of one editor), `platform/code-style-api/src/com/intellij/openapi/editor/EditorModificationUtilEx.java`
  (what typed text replaces in overwrite mode),
  `platform/lang-impl/src/com/intellij/codeInsight/generation/actions/AutoIndentLinesAction.java`
  and `platform/lang-impl/src/com/intellij/codeInsight/generation/AutoIndentLinesHandler.java`
  (which lines, the caret afterwards, only for languages with a formatter),
  `platform/lang-impl/src/com/intellij/codeInsight/hint/ShowContainerInfoHandler.java` (which
  declaration is shown and what a second use does); the action ids in
  `platform/platform-impl/resources/idea/PlatformActions.xml`, the action texts in
  `platform/platform-resources-en/src/messages/ActionsBundle.properties`, the shortcuts of the
  ported keymaps (SPEC-0028). SPEC-0061 v3 behaviour 5a (the declarations around the caret),
  SPEC-0149 v4 behaviour 11 (read-only texts), SPEC-0151 (the status bar's mode widget),
  SPEC-0319 (declared effects), ADR-0008. Nothing of v6 was observed in the reference IDE.
  v8: OBS-0042 (undo with the focus in the project tree leaves the editor's text alone and
  offers the last file operation; a search field takes back its own typing; the history of a
  file is still there after its tab was closed and opened again; one undo reaches across a
  save), GEN (text fields of desktop applications undo their own input), SPEC-0003 v3 (buffer
  and its history), SPEC-0112 (views of one file share a history), SPEC-0193 (a refactoring
  across files is one step), SPEC-0275 v16 (the history of file operations).
- **Feature matrix rows:** F-010, F-011, F-018

## Purpose
One place that turns a keymap action id into its effect on a buffer and its carets, so that the editor
view (M3.7), agents and tests press exactly the same commands. The view only translates key events into
keystrokes; everything after the keymap lookup is defined here.

## Behaviour
1. **State:** a command works on a buffer, its carets, an editor clipboard (text plus whether it holds
   whole lines), the indentation unit, the language's line comment token (if any) and the viewport (first
   visible line and number of visible lines, for page commands). [GEN]
2. **Caret motion:** `EditorLeft`, `EditorRight`, `EditorUp`, `EditorDown`, `EditorPreviousWord`,
   `EditorNextWord`, `EditorLineEnd`, `EditorTextStart`, `EditorTextEnd`, `EditorPageUp`,
   `EditorPageDown` (one viewport height, the viewport moves along), `EditorMoveToPageTop`,
   `EditorMoveToPageBottom` (first/last visible line) and `EditorCodeBlockStart`/`EditorCodeBlockEnd`
   (the enclosing bracket pair's opener/closer) move every caret; each also exists with suffix
   `WithSelection`, which moves only the head. Up/Down keep the goal column. Left/Right on a selection
   without `WithSelection` collapse it to its start/end. [OSS, GEN]
3. **Smart Home:** `EditorLineStart` goes to the first non-blank character of the line; pressed again (or
   when the caret already is there, or the line is blank) it goes to column 0, and from column 0 back to
   the first non-blank character. [GEN]
4. **Deleting:** `EditorBackSpace` deletes the selection or the character before the caret, both halves
   of an empty bracket/quote pair (SPEC-0056) and, in leading indentation that is a multiple of the unit,
   one whole unit; `$Delete` deletes the selection or the character after the caret;
   `EditorDeleteToWordStart`/`EditorDeleteToWordEnd` delete up to the previous word start/next word end;
   `EditorDeleteLine` removes the caret lines (SPEC-0056). [GEN]
5. **Lines:** `EditorEnter` breaks the line with smart indentation (SPEC-0056); `EditorStartNewLine`
   opens an indented line below without breaking the current one, `EditorStartNewLineBefore` one above;
   `EditorSplitLine` breaks the line but leaves the caret before the break; `EditorDuplicate`,
   `EditorJoinLines`, `MoveLineUp`, `MoveLineDown`, `CommentByLineComment` as in SPEC-0056. [GEN]
6. **Tab:** `EditorTab` indents the selected lines when a selection spans a line break, otherwise inserts
   spaces up to the next multiple of the unit (a tab character when the unit is a tab);
   `EditorIndentSelection` always indents and `EditorUnindentSelection` unindents the caret lines. [GEN]
7. **Selection:** `$SelectAll` selects the document with one caret; `EditorSelectWord` grows each
   selection from the caret to the word, then to the line without its break, then to the whole line,
   then to the document; `EditorUnSelectWord` steps back through the same stages;
   `EditorEscape` drops selections and secondary carets, keeping the primary caret;
   `SelectNextOccurrence`, `SelectAllOccurrences`, `EditorCloneCaretAbove`, `EditorCloneCaretBelow`
   as in SPEC-0025. [OSS, GEN]
8. **Clipboard:** `$Copy` and `$Cut` take the selections; with no selection at all they take the caret's
   whole line including its break and mark the clipboard as whole lines. `$Paste` and
   `EditorPasteSimple` replace selections; whole-line clipboard text pasted at an empty caret is inserted
   above the caret's line and the caret keeps its column; with several carets and as many clipboard
   lines, each caret gets its own line. [GEN]
9. **Case and history:** `EditorToggleCase` lower-cases selections that contain an upper-case letter and
   upper-cases the others (the word at the caret without a selection); `$Undo`/`$Redo` undo and redo one
   step. [GEN]
   - **Carets (v7):** undo puts every caret and every selection back where it was before the
     step's command — for a run of typing or deleting, before its first key — and redo where they
     were after it. So a wrong key at five carets is taken back with the five carets still there,
     typing over a selection is taken back with the selection, and undoing never moves the caret
     away from the place the user edited (before v6 one caret went to the end of the changed text:
     the end of the document after undoing a moved line, the second caret's line after an edit at
     two carets). This holds for the editor commands and typing, for Cyclic Expand Word (the carets
     at the prefix), and for the steps the window makes with the carets known: a formatter's
     result, text edits of a language feature, a change taken over in the diff viewer, a reload
     from disk. A step made in the other view of a split (SPEC-0112) puts the carets where that
     view had them: at the place of the edit. A step without known carets (an edit by a tool or
     an agent) puts one caret at the end of the changed text, as before.
     [OSS (logic template: the platform's undo keeps the editor's state before and after every
     command group and puts it back on undo and redo), GEN]
   - **No step for the caret alone:** as before, every undo changes text (IMP-0305).
   - **Read-only (v7):** in a read-only text undo and redo change nothing — neither the text nor
     the carets — and the status bar says that the text is read-only (before v6 the caret jumped
     to the place of the newest step). [OSS (logic template: undo is refused for a read-only
     document), GEN]
10. Every command that changes text is one undo step and applies to all carets. An action id that is
    not an editor command is reported as not handled and changes nothing, so the caller can route it
    elsewhere (e.g. `GotoFile`). Moving carets never builds the whole document text (commands stay
    fast in a 100 000-line file). [GEN]
11. MCP tool `editor_keystrokes`: on a workspace file with given carets, presses a list of keystrokes
    (keymap notation such as `meta shift UP`, or a plain text to type) through the bundled default or
    macOS keymap, carrying out the first editor command bound to it (`Editor…` actions before others,
    as in a focused editor: on macOS `⌘⌫` deletes the line rather than running `$Delete`), and returns per keystroke the resolved action, whether it was handled, and the
    resulting text and carets; the file is not written. [ADR-0008]

12. **v2 – Typing groups:** characters typed one after another at the caret where the previous typing
    ended form one undo step, so ⌘Z removes the whole run and ⇧⌘Z restores it. Any other command (Enter,
    Backspace, paste, a caret move, a mouse click, undo) or a save starts a new step; undo never goes past
    the saved state inside one run. [OSS (logic template: the platform's raw typing handler runs every
    typed character in a command grouped by the document, other editor actions in commands of their own,
    and the undo manager merges commands of the same group)]


### Version 3 (OBS-0018)
13. **Extend selection by syntax:** `EditorSelectWord` grows each selection to the smallest of these
    ranges that contains it and is larger: the word at the caret; every node of the syntax tree
    around it (nodes named `…declarator` left out, so a declaration grows from its initialiser to the
    whole statement); the text of a string literal without its quotes; for a node that starts at the
    first non-blank character of its line and ends at a line end, its whole lines with indentation
    and line break; for a `{…}` block, the whole lines between the braces; the document. Without a
    syntax tree the stages of behaviour 7 apply. `EditorUnSelectWord` steps back through the same
    stages; copying (`$Copy`) between the steps does not end the chain. [OBS-0018 item 1]
14. **Word stops:** `EditorNextWord` skips spaces and tabs and then moves over a run of word
    characters or over one other character (each punctuation character is a stop of its own); at a
    line end it moves to the start of the next line, and spaces before a line end stop there.
    `EditorPreviousWord` mirrors it; with only whitespace before the caret on its line it moves to the
    end of the previous line. Deleting to word start or end uses the same stops. [OBS-0018 item 2]
15. **Smart Home in the indentation:** a caret inside the leading indentation (before the first
    non-blank character) goes to column 0; from column 0 it goes to the first non-blank character;
    from behind the first non-blank character to it (behaviour 3). [OBS-0018 item 9]
16. **Backspace in the indentation:** in languages with braces, with only whitespace before an
    empty caret and the caret at or before the line's proper indentation, Backspace removes the
    line's leading whitespace and the line break before it, joining the line to the end of the
    previous one (on a blank line this removes the line); after the proper indentation it goes back
    to it. The proper indentation is the previous non-blank line's indentation, one unit more after
    an opening bracket at its end, one unit less when the line starts with a closing bracket. In the
    first line and in other languages behaviour 4 applies. [OBS-0018 item 11]
17. **Tab without a selection:** `EditorIndentSelection` without any selection is not handled, so
    Tab falls through to `EditorTab` and inserts spaces up to the next tab stop at the caret, also in
    the middle of a word; `EditorUnindentSelection` (⇧Tab) still unindents the caret line.
    [OBS-0018 item 11]
18. **Repeated deleting is one undo step:** Backspace and Delete pressed one after another at the
    caret where the previous one ended are merged into one undo step, like typing (behaviour 12); any
    other command ends the run. [OBS-0018 item 12]
19. **Paste with indentation:** in source files (not Markdown or plain text), when text with a
    line break, or any text onto a blank line, is pasted at one caret with only whitespace before it
    on its line, the pasted lines are re-indented: the first goes to the proper
    indentation of the target line (behaviour 16; the caret's column if it is larger and the line is
    blank), the following keep their indentation relative to the first; in languages with braces each
    line instead gets the target indentation plus one unit per open bracket of the pasted lines
    before it (a line starting with a closer one unit less). Text pasted after other text on the line
    and whole-line pastes of behaviour 8 are inserted as they are. `EditorPasteSimple` never
    re-indents. [OBS-0018 item 8]
20. **Paste from history (`PasteMultiple`, ⌘⇧V / Ctrl+Shift+V):** the window keeps the texts copied
    or cut in any editor and the system clipboard texts pasted into one, newest first, at most 30, a
    repeated text moving to the top. The action opens a popup with one numbered line per entry
    (first line of the text, line breaks shown as `↵`, at most 80 characters); Up/Down choose,
    Enter (or a click) puts the entry on the clipboard and pastes it like `$Paste` (behaviour 19),
    Escape closes. With an empty history the status line says so. MCP tool `paste_history` returns
    the entries of the running window (empty without a window). [OBS-0018 item 8; ADR-0008]

### Version 4
21. **Matching brace (`EditorMatchBrace`, ⌃M / Ctrl+Shift+M):** every caret that has a bracket `(`, `[`,
    `{`, `)`, `]` or `}` at it or directly before it (the bracket at the caret first) moves to the
    bracket matching it (SPEC-0056 behaviour 9): before an opening bracket, after a closing one, so
    the caret ends outside the pair and pressing again leads back. When the bracket at the caret
    has no partner, the one directly before the caret is taken. Selections are dropped. A caret
    without such a bracket, or whose brackets have no partner, stays where it is.
    [OSS (action id), GEN]
22. **Scroll to centre (`EditorScrollToCenter`, ⌃L / Ctrl+M):** the viewport moves so that the line of
    the primary caret is the middle one of the visible lines — the first visible line becomes the
    caret's line less half of the other visible lines, not before the first line; carets and
    selections stay. The view counts visible rows, so folded lines and rows above lines do not shift
    the middle (SPEC-0058, SPEC-0083). [OSS (action id), GEN]

Both are editor commands of behaviour 10 and reachable through `editor_keystrokes` (behaviour 11).

### Version 5
23. **Cyclic Expand Word (`HippieCompletion`, ⌥/ / Alt+Slash; `HippieBackwardCompletion`, ⌥⇧/ /
    Alt+Shift+Slash):** completes the word before the caret with a word of the open text, without
    a language server and without a list.
    - **Prefix:** the letters, digits and `_` directly before the caret. Without any, or with a
      selection, the command does nothing (the key is used up all the same).
    - **Words:** the runs of letters, digits and `_` that begin with the prefix, upper and lower
      case as typed, and are longer than it. The word the caret is in or at is not one of them.
    - **Order:** `HippieCompletion` takes the words before the caret, the nearest first, then the
      words after the caret from the end of the text back to the caret;
      `HippieBackwardCompletion` takes the words after the caret, the nearest first, then the words
      from the start of the text on to the caret. After the words of the text come those of the
      other files open in editors, file by file in the order of their paths, each file's in the
      order of its text. Every word is offered once, at its first place in this order.
    - **Cycle:** the first use inserts the rest of the first word at the caret; a use directly
      after it replaces what was inserted by the rest of the next word; after the last word the
      prefix stands alone again, and the next use starts over with the first word. The cycle ends
      with anything else: typing, a caret move, another command, an undo. Using the other of the
      two commands inside a cycle puts the prefix back and starts that command's order.
    - **Undo:** however far a cycle went, one undo puts back the prefix as it was; a cycle that
      ended on the prefix leaves no undo step.
    - **Several carets:** each caret has its own prefix and its own words and steps through them
      at the same pace; a caret that has run out of words shows its prefix while the others go on,
      and the cycle starts over when every caret has. Carets without a prefix are left alone.
    - **Where:** in every editor; a field that hides its text (a password) is not completed, and
      fields and files other than those open in editors are no source of words. A text over 8 MiB
      (SPEC-0260 behaviour 4) offers and takes no words.
    - **Agents:** `editor_action` has `expand_word` and `expand_word_backward`, with `count` for
      the number of uses in a row (default 1); it knows the words of the given file only.
      `editor_keystrokes` carries the commands out by their keys, also without other files.
    [OSS (action ids and keys of the ported keymaps), DOC (the reference has a command that
    completes the word at the caret from the words of the text, cyclically, in both directions;
    fact only), GEN (dynamic word completion as editors have had it for decades), ADR-0008]

### Version 6
Editor actions of the bundled keymaps whose rules the open platform's sources give. Each is listed
by Find Action under the title given here, has its keymap shortcut, and declares its effect for UI
automation (SPEC-0319): `EditorCutLineEnd` and `AutoIndentLines` write, `EditorToggleInsertState`
and `EditorContextInfo` change nothing outside the window. A chord of an editor action that the
keyboard reports under another name than the keymap's — ⇧⌘. arrives as `>` — reaches the focused
editor under the name SPEC-0028 v5 behaviour 10 resolves it to; until v6 only the workbench's
actions were found that way. Of the four, input fields — the find bar, the commit message and
the like — carry out `EditorCutLineEnd` alone; a field that hides its text (a password) does not
cut either, so its text never reaches the clipboard.

24. **Cut up to Line End (`EditorCutLineEnd`, ⌃K in the macOS keymap; no key in the default
    keymap):** every caret cuts the text from itself to the end of its line; a selection plays
    no part, the range starts at the caret.
    - **The line break:** when nothing or only spaces and tabs stand between the caret and the end
      of its line and the line is not the last one, the line break is cut as well, so the next
      line joins the caret's line; a `\r\n` goes as a whole. Otherwise the line break stays. A
      caret at the end of the text cuts nothing.
    - **Clipboard:** the cut text goes to the clipboard with every line break as `\n`, not marked
      as whole lines (behaviour 8). With several carets the texts stand in the order of the
      carets, one `\n` between them; ranges that overlap are cut once. When no caret has
      anything to cut, the clipboard and the text stay as they are.
    - **Cuts in a row:** with one caret, a cut directly after another cut of this command at the
      same place adds its text to the end of what the clipboard got from the cut before, so ⌃K
      pressed several times and one paste bring back all of it. Typing, any other command, an
      undo, a caret somewhere else, another text on this editor's clipboard, and the editor or
      its window losing the focus end the row; with several carets nothing is joined. The
      command never reads the system clipboard: what was copied elsewhere is noticed by the
      focus alone.
    - Each use is one undo step; the caret stays where it was and selections are dropped. The
      cut text joins the paste history (behaviour 20). A read-only text stays as it is, and so
      does the clipboard (SPEC-0149 v4 behaviour 11).
    [OSS, `CutLineEndAction.java` (range, line break, several carets), `KillRingUtil.java` and
    `CopyPasteManagerWithHistory.java` (line breaks as `\n`, cuts at the same place joined),
    `EditorCopyPasteHelperImpl.java` (one text per caret); GEN: what ends a row, an empty cut
    (IMP-0405)]
25. **Insert and overwrite (`EditorToggleInsertState`, Insert):** an editor is in insert mode or
    in overwrite mode; the command switches. The mode belongs to one editor — another editor and
    a split of the same file keep their own — starts as insert mode and is not saved.
    - **Typing in overwrite mode:** at every caret a selection is removed first; then the typed
      text takes the place of as many characters after the caret as it has itself, but never of
      the line break or anything behind it: at the end of a line, and for the part of a longer
      text that reaches past it, typing inserts. A character is a Unicode scalar value. Brackets
      and quotes are not closed and a closing one is not stepped over (SPEC-0056 behaviour 7);
      consecutive typing is one undo step as in behaviour 12.
    - Enter, Backspace, Delete, paste, text an input method composes and every other command work
      as in insert mode.
    - **Shown:** in overwrite mode the caret is a block as wide as the character it stands on (one
      character cell at a line end) that lets the character show through, and the status bar
      shows `Overwrite` beside the caret position, where column selection mode shows its widget
      (SPEC-0151); a click on it switches back. Input fields — the find bar, the commit message
      and the like — have no overwrite mode: there the command is not carried out.
    [OSS, `ToggleInsertStateAction.java`, `EditorImpl.java` (a state of the editor, not of the
    settings), `EditorModificationUtilEx.java` (selection first, then as many characters as
    typed up to the line end); GEN: the block caret, the status bar, no pairs]
26. **Auto-Indent Lines (`AutoIndentLines`, ⌃⌥I / Ctrl+Alt+I):** gives lines the indentation their
    place in the code asks for, without changing anything else in them.
    - **Lines:** with a selection at the primary caret, the lines from the one it starts on to
      the one its last selected character is on (a selection ending at the start of a line does
      not take that line); without one, the caret's line.
    - **Indentation:** in languages with braces (behaviour 16) every such line that is not blank
      gets its proper indentation, the lines from top to bottom, each seeing the lines above as
      they are by then: the indentation of the previous non-blank line, one unit more after an
      opening bracket at that line's end, one unit less when the line itself starts with a closing
      bracket. A line that starts with `*` directly under a line starting with `/*` stands one
      space further in than that line, under a line starting with `*` as far in as it; the line
      after one that starts with `*` and ends with `*/` stands one space further out. Blank
      lines stay as they are.
    - **Other languages** (Python, YAML, Markdown, plain text and every language without braces)
      have no such rule here: nothing changes and the status bar says so.
    - **Afterwards:** without a selection the caret goes to the next line, to the column it had
      before the change or to that line's end if it is shorter; on the last line it stays. With
      a selection, the selection and the other carets stay on their text. The whole change is
      one undo step; when every line already stood right there is none.
    [OSS, `AutoIndentLinesHandler.java` (lines, the caret going down, one line or a range),
    `AutoIndentLinesAction.java` (only where the language has a formatter); SPEC-0057 behaviour
    16 (the proper indentation); GEN: comment lines, the status message]
27. **Context Info (`EditorContextInfo`, ⌃⇧Q / Alt+Q):** shows the declaration the caret is in
    when its beginning is scrolled out of view.
    - **Which:** the declarations around the caret are those of the breadcrumbs (SPEC-0061 v3
      behaviour 5a), the innermost first. The first of them whose line — the line of its name —
      is above the first visible row is shown. When all of them are visible nothing is shown.
    - **Again:** used while a declaration is shown, the command goes on outwards from it and
      shows the next declaration that is above the first visible row; after the outermost the
      strip goes.
    - **Shown:** as a strip over the top of the editor with that line's text, in the editor font
      at the place the text has in the editor. It goes with the next caret move, edit, scroll or
      Escape; it takes no focus and changes neither caret nor scroll position.
    - When no declaration is around the caret — also in plain text and in a language whose
      declarations the syntax tree does not know — nothing is shown and the status bar says so.
    [OSS, `ShowContainerInfoHandler.java` (innermost declaration not in view, one level up on a
    second use); SPEC-0061; GEN: one line, plain colours]
28. **Agents (v6):** `editor_action` has `cut_to_line_end` (the text and carets afterwards and
    `copied`, what the clipboard would get; the clipboard is not touched), `type` with
    `overwrite: true` (behaviour 25; the text may then be longer than one character),
    `auto_indent_lines` (with `changed_lines`; an error names a language without the rule) and
    `context_info` (with `first_visible_line`, 1-based, and optionally `shown_line`, the line a
    previous call returned: `context` is the line, name, kind and text of the declaration to
    show, or `null`). `editor_keystrokes` carries out `EditorCutLineEnd`, `EditorToggleInsertState` and
    `AutoIndentLines` by their keys; typed steps follow the mode. [ADR-0008]

### Version 8 (OBS-0042)

29. **Where undo and redo go:** `$Undo` and `$Redo` act on what has the keyboard focus, whether
    they come from their keys or from the Edit menu. (Chosen in Find Action they act on the
    active editor: the popup had the focus, not the tree.)
    - **An editor:** the history of its document (behaviours 9, 12 and 18).
    - **An input field** (behaviour 30): the history of that field. The editor behind it is
      never changed, also when the field has nothing left to take back.
    - **The project tree:** the history of file operations (SPEC-0275 v16 behaviours 37–43).
      The text of every editor stays as it is, also when the last thing the user did was typing
      in an editor and also when no file operation is left to take back; the status bar then
      says that there is nothing to undo (or redo) in the project tree.
    - **Anything else** (another tool window, the tab row): as before v8 the document of the
      active editor. OBS-0042 looked at the project tree only.
    The two histories are separate: undo in an editor never takes back a file operation, undo
    in the project tree never takes back text. [OBS-0042, GEN]
30. **Input fields have a history of their own:** every text field of the window — the fields of
    the find bar, Search Everywhere and the other search popups, Find in Files, the name field of
    New and Rename, the fields of dialogs, the commit message — keeps the edits made in it since
    it was opened or filled by the window. `$Undo` in a field takes back its last step and puts
    the caret and the selection where they were before it, `$Redo` makes it again; a run of
    typing is one step, as in the editor (behaviour 12). What the field drives follows the text:
    after undo in the find bar the matches are those of the text that is back. A text the window
    puts into a field (the name to rename, the selection taken over as the search text) is the
    field's starting point, not a step: undo never empties a prefilled field. The history goes
    with the field: a popup opened again starts without one. A masked field (a password) has a
    history like any other; nothing of it is kept after the field closed. [OBS-0042, GEN]
31. **The history of a file outlives its tab:** when the last editor of a file closes while the
    file has no unsaved changes, its undo and redo history is kept for this session of the
    project window. When the file is opened again and its text on disk is exactly the text the
    history ended with, the new editor continues that history: `$Undo` takes back the last edit
    made before the tab closed (the file is then modified, as after any undo past the saved
    state, SPEC-0003 behaviour 8), `$Redo` what had been undone before it closed.
    - **Never onto another text:** when the file on disk differs from the kept text (changed by
      another program, by an agent's file tool, by Git) the kept history is dropped and the
      editor starts without one. A history is never applied to a text it was not made for.
    - **Not kept:** the history of a tab closed with unsaved changes thrown away; of a file
      whose history holds a refactoring across files (SPEC-0193: such a step is undone in all
      its files or not at all, and that bookkeeping ends with the editor); of a document of a
      remote project (SPEC-0317: the host owns it); of a file that is renamed, moved or deleted
      while closed.
    - **Bounded:** the histories of the 32 files closed last are kept, in memory only; nothing
      of it is written to disk and nothing survives closing the project window.
    [OBS-0042, SPEC-0003, GEN]

## Acceptance criteria
- **AK-1** Motion commands with and without selection on single and multiple carets, goal column across
  short lines, page commands moving the viewport, code block start/end.
- **AK-2** Smart Home cycles between first non-blank character and column 0, also on blank lines.
- **AK-3** Backspace removes an indentation unit in leading whitespace and an empty pair; delete to
  word start/end.
- **AK-4** Copy/cut without a selection takes the whole line; pasting it inserts above the caret line;
  per-caret paste with as many lines as carets.
- **AK-5** Select word grows word → line → line with break → document and shrinks back; Escape keeps
  only the primary caret; toggle case; undo/redo place the caret.
- **AK-6** Every action id of the default keymap whose name starts with `Editor` and that this spec
  lists is handled; unknown ids report not handled without change.
- **AK-7** The MCP tool resolves `shift alt UP` in the default keymap to `MoveLineUp` and, in the macOS
  keymap, `meta BACK_SPACE` to `EditorDeleteLine` and `alt UP` to `EditorSelectWord`; applies a
  keystroke sequence and typed text, reports a keystroke without editor command, and leaves the file
  unchanged.
- **AK-8 (v2)** A typed run is undone and redone at once; Enter, a caret move and a save each end the
  run; stepping over a typed closer stays in the run (unit test).
- **AK-9** Hand test 9 of round 4 as a test: caret on `area`, Add Selection for Next Occurrence three
  times, typing, Escape, Select All Occurrences — each step leaves the selections and carets the editor
  view paints, one caret per occurrence (unit test
  `editor_view::caret_tests::occurrences_are_painted_with_a_caret_each`); ⌃G, ⌃⇧G, ⌃⌘G each reach one
  action in the macOS keymap and Go to Line is ⌘L (unit test
  `menu_bar::tests::occurrence_keys_on_macos`).

- **AK-10 (v3)** Selection by syntax in Java from inside `"Hello, "`: word, string content, string,
  binary expression, statement, whole line, the three body lines, the block, and back with
  `EditorUnSelectWord` after a copy (unit tests `syntax_selection_stages` in `ferris_idea_languages` and
  `select_word_uses_the_given_stages`).
- **AK-11 (v3)** Word stops at `=`, `.`, `(`, `)` and `;`, line end to next line start, line start to
  previous line end (unit test `word_stops_follow_punctuation_and_line_ends`).
- **AK-12 (v3)** Smart Home from inside the indentation goes to column 0; Backspace before `return`
  at the proper indentation joins the line; on a blank indented line it removes the line; Tab inside
  a word inserts spaces at the caret (unit tests `smart_home_cycles`, `backspace_to_the_indentation`,
  `tab_inside_a_word`).
- **AK-13 (v3)** Three Backspaces are undone at once; Backspace after a caret move starts a new step
  (unit test `repeated_deleting_is_one_undo_step`).
- **AK-14 (v3)** Pasting an unindented `if` block into a method body indents it 8/12/8, an over-indented
  pair on a blank member line to 4; text after other text is unchanged (unit test
  `paste_reindents_lines`).
- **AK-15 (v3)** The paste history keeps the newest first, moves repeats to the top and holds 30
  entries; the popup row text; MCP `paste_history` (unit tests `paste_history::tests`, protocol
  test `paste_history_tool`).
- **AK-16 (v4)** `EditorMatchBrace` from before an opener, after an opener, after a closer and back,
  with two carets, across a bracket in a string, and without a bracket (unit test
  `commands::tests::match_brace_moves_outside_the_pair`).
- **AK-17 (v4)** `EditorScrollToCenter` puts the caret line in the middle of a 10-line viewport, stops
  at the first line and moves no caret (unit test `commands::tests::scroll_to_center`); the scroll
  offset for a row within the scroll limits (unit test `layout::tests::scrolling`); in the view
  the caret's row is centred with a fold above it
  (`crates/workbench/tests/it/editor_actions_keymap.rs::scroll_to_center_counts_rows`).
- **AK-18 (v5, behaviour 23)** Unit tests in `commands::tests`: the nearest word before the caret
  first, then the next, then the words after the caret, then the prefix again and the first word
  once more (`expand_word_cycles_through_the_words`); the backward command's order and the switch
  between the two (`expand_word_backward_and_switching`); the words of other files after the
  text's own, each word once (`expand_word_takes_other_files_last`); one undo for a whole cycle
  and none after a cycle back to the prefix, a new cycle after typing or a caret move
  (`expand_word_is_one_undo_step`); two carets with different prefixes, no prefix and a selection
  (`expand_word_with_several_carets`). `editor_action` `expand_word` with `count`, and ⌥/ through
  `editor_keystrokes` (`protocol.rs`: `expand_word_tools`). In the view the other open file's
  words follow (`crates/workbench/tests/it/editor_actions_keymap.rs::expand_word_in_the_window`).

- **AK-19 (v6, behaviour 24)** Unit tests in `commands::tests`: from the middle of a line, at a line
  end (the next line joins), before trailing spaces, on the last line and at the end of the text,
  with `\r\n`, with letters outside the Basic Multilingual Plane, with a selection, one undo
  (`cut_to_line_end_takes_the_rest_of_the_line`); three cuts in a row give one clipboard text, an
  undo, a caret move, typing, a foreign clipboard text and the lost focus end the row
  (`cut_to_line_end_joins_cuts_in_a_row`); two carets on two lines, two carets on one line, carets
  with nothing to cut (`cut_to_line_end_with_several_carets`).
- **AK-20 (v6, behaviour 25)** Unit tests in `commands::tests`: a typed character replaces the one
  under the caret, inserts at the line end and before `\r\n`, a longer text replaces up to the
  line end and inserts the rest, a letter outside the Basic Multilingual Plane counts once, no
  pair is closed (`overwrite_mode_replaces_up_to_the_line_end`); a selection goes first, every
  caret types, a typed run is one undo step, the mode switches back
  (`overwrite_mode_with_carets_selections_and_undo`); the caret's width
  (`layout::tests::caret_widths`).
- **AK-21 (v6, behaviour 26)** `smart::tests::auto_indent_follows_the_brackets` (nested blocks,
  closing brackets, tabs as the unit, comment lines, blank lines, `\r\n`, lines that already
  stand right); `commands::tests::auto_indent_lines_moves_the_caret_down` (the caret's column, a
  shorter next line, the last line, a selection ending at a line start, one undo, further
  carets); `commands::tests::auto_indent_lines_without_rules_says_so` (Python and plain text).
- **AK-22 (v6, behaviour 27)** `syntax::tests::context_declaration_walks_outwards`: the innermost
  declaration above the first visible line, none when all are in view, the next outer one on a
  second use, none after the outermost.
- **AK-23 (v6)** `security::tests::fifth_package_actions_declare_their_effects`;
  `everywhere::tests::fifth_package_actions_have_titles` (titles, groups, a key in the macOS
  keymap or offered without one).
- **AK-24 (v6, behaviour 28)** `crates/mcp/tests/it/protocol.rs`: `editor_action_fifth_package`
  (`cut_to_line_end` with `copied`, `type` with `overwrite`, `auto_indent_lines` on a Java and on
  a Python file, `context_info` twice, ⌃K and Insert through `editor_keystrokes`).
- **AK-25 (v6)** In a window, through the window's key dispatch
  (`crates/workbench/tests/it/editor_actions_fifth_package.rs`): ⌃K cuts to the line end, joins
  the next line, fills the clipboard over two presses and leaves a read-only text and the
  clipboard alone (`ctrl_k_cuts_to_the_line_end`); Insert switches the mode of one editor, typing
  replaces, the status and the caret's width follow (`insert_switches_overwrite_mode`); ⌃⌥I
  re-indents the selected lines of a Java file and says in a text file that there is no rule
  (`auto_indent_lines_in_the_window`); ⌃⇧Q shows the method's line when it is scrolled out of
  view, the class's on the second press, nothing on the third, and the strip goes with a caret
  move (`context_info_shows_the_hidden_declaration`).
- **AK-26 (v6)** Hand test, open: the keys on a real keyboard (Insert exists on few Mac
  keyboards), the block caret and the strip on screen, each action next to the reference.
- **AK-27 (v7, behaviour 9)** Carets after undo and redo. Unit tests in `commands::tests`:
  typing over a selection, an edit at three carets, a moved line, a deleted line, an auto-closed
  bracket and its removal, a run of typing and a run of deleting come back with their carets and
  selections, and redo puts the carets after the command back
  (`undo_and_redo_put_the_carets_back`); Cyclic Expand Word goes back to the prefix
  (`expand_word_is_one_undo_step`); a step without noted carets and noted carets that do not fit
  the text fall back to the end of the changed text (`buffer::tests::carets_noted_on_a_step`).
  In a window through the key dispatch (`crates/workbench/tests/it/undo_redo.rs`): ⌘Z and ⇧⌘Z
  after typing, paste, cut, delete line, duplicate, move line, two carets, column selection, a
  reformat and a reload from disk (`editing_commands_are_undone_with_their_carets`,
  `steps_of_the_window_are_undone_with_their_carets`); a read-only text keeps text and caret
  and names the reason (`a_read_only_text_takes_no_undo`).
- **AK-28 (v8, behaviour 29)** Given a window of the test harness with a file open and text
  typed into it, when the project tree gets the focus and ⌘Z and ⇧⌘Z are pressed (through the
  key dispatch and, where nothing in the window takes the chord, the menu bar's action), then
  the editor's text is unchanged and the status bar says that there is nothing to undo or redo
  in the project tree; back in the editor ⌘Z takes the typing back
  (`crates/workbench/tests/it/undo_file_operations.rs::undo_in_the_tree_leaves_the_editor_alone`).
- **AK-29 (v8, behaviour 30)** Given the find bar over an editor with typed text, when `ab` is
  typed into the field, the caret is moved and `c` is typed, then the first ⌘Z leaves `ab` and
  the matches of `ab`, the second an empty field, a third changes nothing, two ⇧⌘Z bring `abc`
  and its matches back, and the editor's text is the same throughout; a Rename field prefilled
  with the file's name keeps that name after ⌘Z with nothing typed, and gets it back after
  typing over it
  (`crates/workbench/tests/it/undo_redo.rs::an_input_field_undoes_its_own_typing`).
- **AK-30 (v8, behaviour 31)** Given a file edited and saved, when its tab is closed and the
  file opened again, then ⌘Z takes the edit back and ⇧⌘Z makes it again; given the tab was
  closed with a redo step left, ⇧⌘Z makes that step again; given the file was written by
  another program while closed, then ⌘Z changes nothing, also after the old text is on disk
  again (`crates/workbench/tests/it/undo_redo.rs::the_history_of_a_file_outlives_its_tab`);
  at most 32 histories are kept, the oldest goes first, and a history is handed out only for
  the text it ended with (`closed_history::tests::the_oldest_history_goes_first`).
- **AK-31 (v8)** Hand test, open: points 3–5 of OBS-0042 in the application's window next to
  the reference (the question before a file operation is taken back, the Edit menu's titles
  with the focus in the tree, the find bar, a closed and reopened tab).

## Out of scope
The editor view itself (M3.7), code completion popups (`EditorChooseLookupItem*`, SPEC-0072), column selection
mode, camel-hump word motion, soft wraps, folding commands (SPEC-0083),
`EditorScrollTop` and `EditorScrollBottom` (no shortcut in the ported keymaps). v6: the other
kill-ring commands (`EditorKillToWordStart`, `EditorKillToWordEnd`, `EditorKillRegion`,
`EditorKillRingSave`; no shortcut in the ported keymaps), overwrite mode in input fields, a
setting for the caret's shape, Auto-Indent Lines through a language server's formatter, the
declaration's whole header and its colours in Context Info.

## Known deviations from the reference
Compared side by side in OBS-0018 (2026-11-05). Undo never stops at a step that only restores the caret
(IMP-0305). Selection by syntax leaves out the reference's stage that adds the blank line after a
method. The paste history popup has no separate "paste as plain text" button; text is always pasted
as plain text.

v4: not compared with the reference. Open there: on which side of the bracket the reference's
matching-brace command leaves the caret, and what it does for a caret that is not at a bracket
(Ferris IDEA: nothing).

v6: not compared with the reference. Open there: whether the reference keeps all carets and the
column selection after undoing an edit made with them, and where its caret is after undoing a
reformat, an optimize imports and a reload from disk (Ferris IDEA: where it was before the step).

v5: not compared with the reference. Open there: in which order it visits the words after those
before the caret, whether it matches case, what belongs to a word in each language (Ferris IDEA:
letters, digits and `_` everywhere), whether it also takes words of files that are not open, and
what the two commands do when used one after the other.

v6: taken from the open platform's sources, not compared with the reference IDE.
- `EditorCutLineEnd`: the open action puts an empty text on the clipboard when the caret is at
  the end of the text; Ferris IDEA leaves the clipboard alone (IMP-0405). Where the open platform
  ends a row of joined cuts was not read in full (it keeps a flag per clipboard entry that other
  parts of the editor clear, and it sees every text that reaches the clipboard); Ferris IDEA ends
  it with anything but the same command at the same place and with the lost focus, and does not
  look at the system clipboard for it. Selections are dropped by the cut; the open action leaves a single caret's selection to
  the document change.
- `EditorToggleInsertState`: how the reference draws the caret and whether its status bar names
  the mode was not looked up. Whether the reference still closes brackets in overwrite mode is
  open; Ferris IDEA does not.
- `AutoIndentLines`: the open action asks the language's formatter for the indentation of each
  line, so it knows continuation lines, `case` labels, bodies without braces and languages
  without braces. Ferris IDEA uses the line rule of behaviour 16 and therefore puts a continuation
  line under the start of its statement, leaves a body without braces unindented and does
  nothing in languages without braces. A caret column beyond the end of the next line is cut to
  that line's end (the open editor can keep it in virtual space).
- `EditorContextInfo`: the open action shows the declaration's whole header, with its colours, as
  a hint that also goes when the pointer moves; Ferris IDEA shows the one line of the name in the
  plain text colour. Which elements count as declarations follows the breadcrumbs, not the
  reference's structure view.

v8: compared side by side in OBS-0042 (2026-11-14).
- **A save ends an undo step** (IMP-0407). In the reference one undo after typing, saving and
  typing again takes back the text from both sides of the save. Ferris IDEA keeps the step it has
  had since SPEC-0003 behaviour 8: undo stops at the saved text, so the state that was written
  to disk can always be reached again and an unmodified editor always shows the saved text.
  Deliberate; Ferris IDEA is not changed.
- **A field takes back a run of typing, not one character.** After three characters typed into
  the reference's search field one undo left two; Ferris IDEA takes back the run, as its editor
  does (behaviour 12), so fields and editors group typing by one rule. How the reference groups
  fast typing in a field, and what its other fields do, was not observed.
- **Undo in an editor never reaches a file operation** (behaviour 29). Whether the reference's
  editor undo does, and in which order with text edits, was not observed.
- The history of a closed file is dropped when the file on disk no longer holds the kept text,
  and is bounded to 32 files (behaviour 31); what the reference does in these cases was not
  observed.
