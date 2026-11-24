# SPEC-0025: Selections and multiple carets

- **Version:** 5 · **Date:** 2026-11-16 (v5: Clone Caret Above/Below by the doubled modifier,
  behaviour 9; a suggestion accepted at several carets goes in at every caret, behaviour 10; from
  the check of feature row F-012 without a screen; v4: occurrences of a word are whole words, behaviour 4b,
  OBS-0058 M1; text pasted at one caret in plain text is indented to its column, behaviour 6a,
  OBS-0058 R1; v3: where the carets of added occurrences stand, behaviour 4a, OBS-0053 C1; v2:
  Unselect Occurrence, behaviour 8)
- **Sources:** GEN (editors keep several carets with optional selections that move and edit together; add a
  caret above/below, block (column) selection, add the next occurrence of the selection, select all
  occurrences; F-012), SPEC-0003 (buffer, offsets in Unicode scalar values); v2: OSS (action id
  `UnselectPreviousOccurrence` with ⌃⇧G in the bundled macOS keymap and Alt+Shift+J in the default
  keymap of `JetBrains/intellij-community`, already in `crates/ui/keymaps`), DOC (jetbrains.com/help:
  the action removes the selection of the last added occurrence), SPEC-0056 (`editor_keystrokes`);
  v3: OBS (OBS-0053: with the caret at the start of a word, ⌃G and ⌃⌘G leave every caret at the
  start of its occurrence); v4: OBS (OBS-0058: in `x xz x` ⌃⌘G from an empty caret gives two carets;
  a rectangle copied in column mode and pasted at the end of a line puts its first line after the
  caret and the others on new lines below, indented with spaces to the caret's column), OSS
  (`JetBrains/intellij-community` at `d4018fb`, read as a logic template, no code taken:
  `SelectOccurrencesActionHandler`, `SelectNextOccurrenceAction` and `SelectAllOccurrencesAction`
  search whole words only after the word was selected at an empty caret; `PasteHandler` inserts
  pasted text at one caret as text and then, in a file whose language has no formatter, indents the
  block with `DefaultTypingActionsExtension.indentPlainTextBlock`; `EditorCopyPasteHelperImpl`,
  `ClipboardTextPerCaretSplitter` and `TextBlockTransferable` give a single caret outside column
  mode the whole text; `docs/legal/PROVENANCE.md`); v5: OSS (`JetBrains/intellij-community` at
  `d4018fb`, read as a logic template, no code taken:
  `platform/platform-impl/src/com/intellij/openapi/keymap/impl/ModifierKeyDoubleClickHandler.kt`
  — the clone actions are bound to the multi-caret modifier pressed twice, ⌥ on macOS and Ctrl
  elsewhere, held on the second press, with ↑ or ↓; a press, release and press each within
  300 ms; another key or modifier ends it), GEN (editors with several carets insert a chosen
  completion at each caret), SPEC-0072 (the completion list)
- **Feature matrix rows:** F-012

## Purpose
Edit in several places at once, predictably, with one undo step per action.

## Behaviour
1. A caret has an anchor and a head (char offsets); the selection is the range between them, empty when
   they are equal. Carets are kept sorted; carets whose selections overlap, empty carets at the same
   offset, and an empty caret at the start or end of another caret's selection are merged. There is
   always at least one caret; the most recently added is the primary one. [GEN]
2. Moving: left/right by a character, up/down by a line keeping the column the caret had before the first
   vertical move (goal column, clamped to shorter lines), line start/end, word left/right (a word is a run
   of letters, digits or `_`), document start/end. Moving with "extend" moves only the head; moving
   left/right without "extend" from a non-empty selection collapses it to its start/end. [GEN]
3. Adding carets: at an offset; one line above/below each caret at its goal column (no caret is added
   beyond the first or last line); a block selection from one position to another gives one caret per
   line with the selection clamped to each line's length. [GEN]
4. Occurrences: "add next occurrence" selects the word at the primary caret when its selection is empty;
   otherwise adds a caret selecting the next occurrence of the primary selection's text after it,
   wrapping around, skipping occurrences already selected. "Select all occurrences" does that for every
   occurrence. Matching is exact (case-sensitive). [GEN]
4a. **v3 – Where the carets stand:** every added occurrence gets its caret at the same offset
   within its selection as the caret had within the first selection: at the start when the first
   selection's head is its start (a selection made backwards, or the word selected from a caret at
   its start), else at the end. When "add next occurrence" or "select all occurrences" selects the
   word at an empty caret, the caret keeps its place in the word: a caret at the word's start stays
   at the start, a caret at its end stays at the end. [OBS] A caret inside the word goes to the
   word's end, because a caret in Ferris IDEA always stands at one end of its selection (Ferris IDEA's
   choice; not observed in the reference). Until v2 every caret stood at the end. [GEN]
4b. **v4 – Whole words:** when "add next occurrence" selected the word at an empty caret, the
   occurrences it adds afterwards are whole words only (not preceded or followed by a letter, a
   number or `_`), as long as only "add next occurrence" and Unselect Occurrence run; any other
   command or edit ends that, and a selection made otherwise matches its exact text anywhere.
   "Select all occurrences" from an empty caret selects the word and all its whole-word
   occurrences; from a selection, every occurrence of the exact text. In `x xz x` both give two
   carets, not the `x` in `xz`. Until v3 occurrences matched the exact text in every case. [OBS, OSS]
5. Editing at all carets as one undo step: typing replaces each selection (or inserts at the caret);
   backspace/delete remove the selection, or the character before/after the caret. Edits are applied from
   the last caret to the first so earlier offsets stay valid; afterwards each caret is empty and sits
   after its own inserted text. [GEN]
6. Copy returns the selected texts joined by line breaks (whole lines for empty selections are out of
   scope). Paste: when the pasted text has exactly as many lines as there are carets, each caret gets its
   line; otherwise every caret gets the whole text. [GEN]
6a. **v4 – Pasting a block at one caret in plain text:** Paste (`$Paste`) of text with line breaks at
   a single caret in a plain-text file (no language, so no formatter) inserts the text at the caret
   (replacing its selection) and indents every further line of it with spaces to the column the
   caret (or the selection's start) stood at, a tab counting to its next tab stop. A line break at
   the end opens no indented line, and text that starts with a line break or holds only blanks stays
   as it is. A rectangle copied in column mode is such text, so at the end of `x xz x` the
   rectangle `a`/`c`/`e` gives `x xz xa`, then `      c` and `      e` as new lines before the
   following ones. It is one undo step and the caret stands after the pasted text. Paste Simple
   (`EditorPasteSimple`), several carets and source files keep their own ways (behaviour 6 and
   SPEC-0057 v3 behaviour 19). Until v3 the lines came without indentation. [OBS, OSS]
7. MCP tool `preview_multi_caret_edit` (path, occurrence, replacement): carets on all occurrences, the
   replacement typed at all of them; returns the caret offsets and the resulting text without writing the
   file. [ADR-0008]
8. **Unselect Occurrence** (`UnselectPreviousOccurrence`, ⌃⇧G on macOS, Alt+Shift+J in the default
   keymap): with several carets it removes the most recently added one — the last occurrence added by
   "add next occurrence" or the last caret added above/below — and the caret added before it becomes
   the primary one; it is the inverse of "add next occurrence" step by step, also after typing at all
   carets. With one caret and a selection it removes the selection and leaves the caret at its head;
   with one empty caret it does nothing. It is in the keymap, in Edit › Unselect Occurrence of the
   menu bar and in Find Action (⇧⌘A); agents reach it through the MCP tool `editor_keystrokes` with
   the keystroke of the keymap (`control shift G` in the macOS keymap). [OSS, DOC, GEN, ADR-0008]

9. **v5 – Clone Caret Above/Below by the doubled modifier:** `EditorCloneCaretAbove` and
   `EditorCloneCaretBelow` have no key of their own in the keymaps; in the editor they are reached
   by pressing the multi-caret modifier — ⌥ on macOS, Ctrl elsewhere — twice and keeping it held
   on the second press: each ↑ then clones the carets one line up and each ↓ one line down
   (behaviour 3), as often as the arrow is pressed while the modifier stays held. Press, release
   and second press each come within 300 ms of the step before; a slower step, another modifier
   or any other key ends the gesture, and so does releasing the modifier. Without the gesture
   ⌥↑ and ⌥↓ keep their keymap actions (Extend and Shrink Selection on macOS). Input fields have
   no gesture. Find Action (⇧⌘A) still runs both actions; agents use `EditorCloneCaretAbove` and
   `EditorCloneCaretBelow` through the editor action tools. Until v4 the actions were reachable
   only through Find Action. [OSS, GEN, ADR-0008]
10. **v5 – Completion at several carets:** a suggestion accepted from the completion list
   (SPEC-0072, Enter or Tab) while the editor has several carets goes in at every caret, as one
   undo step: at the primary caret as with one caret; at every other caret it replaces that
   caret's selection or, at an empty caret, as many characters before it as the prefix typed at
   the primary caret (and with Tab the rest of the identifier after it). Afterwards every caret
   stands after its inserted text and the primary caret stays primary. With several carets the
   text of a template goes in without its tab stops, and the edits the suggestion brings
   elsewhere (an import) are made once. Until v4 the suggestion went in at the primary caret only
   and the other carets were dropped. [GEN]

## Acceptance criteria
- **AK-1** Normalisation: sorting, merging of overlapping selections and of equal empty carets, primary
  caret kept.
- **AK-2** Movement incl. goal column across shorter lines, word movement with Unicode text, extend and
  collapse.
- **AK-3** Carets above/below and block selection with clamping.
- **AK-4** Next/all occurrences incl. word at caret, wrap-around, skipping selected ones.
- **AK-5** Typing, backspace and delete at three carets produce the expected text and carets, and one undo
  restores the text.
- **AK-6** Copy and paste distribution.
- **AK-7** The MCP preview replaces three occurrences and leaves the file unchanged.
- **AK-8** Given three occurrences added one after another (also after typing at them, and a caret
  added below), when Unselect Occurrence runs repeatedly, then the carets go away in reverse order of
  adding, the one before becomes primary, a single selection is cleared and a single caret stays;
  the editor command, the menu bar entry, Find Action and the macOS keymap's ⌃⇧G reach it, and MCP
  `editor_keystrokes` with `control shift G` does the same. (`selection::tests::unselect_occurrence`,
  `commands` tests, `menu_bar::tests::occurrence_keys_on_macos`, `crates/mcp/tests/it/protocol.rs`:
  `unselect_occurrence_over_mcp`)

- **AK-9 (v3)** Given `foo one foo` / `bar foo two` / `foo three` with the caret at offset 0, when
  ⌃G is pressed three times, then three `foo` are selected with every caret at the start of its
  `foo`; ⌃⇧G removes the last; ⌃⌘G from the start of the file selects all four with the carets at
  their starts; from the end of a word all carets stand at the ends; from inside a word at the
  ends; a selection made backwards with ⇧← gives carets at the starts. Keys only, in a window of
  the test harness (`crates/workbench/tests/it/occurrence_carets.rs`:
  `carets_keep_their_place_in_the_occurrence`); unit test
  `selection::tests::occurrence_carets_keep_their_offset`.

- **AK-10 (v4)** Unit test `selection::tests::occurrences_of_a_word_are_whole_words`: in `x xz x`
  ⌃⌘G and ⌃G from an empty caret give two carets, from a selection of `x` three; a command in
  between ends the whole-word matching; `selection::tests::occurrences` skips `foo_x`.
- **AK-11 (v4)** Unit test `commands::tests::pasted_block_is_indented_to_the_caret_in_plain_text`:
  the rectangle `a`/`c`/`e` pasted at the end of `x xz x` gives the two indented new lines, one undo
  restores the text; Paste Simple does not indent; a tab counts to its tab stop; `indent_block`
  leaves text starting with a line break and opens no line after a final break.
- **AK-12 (v4)** In a window of the test harness, by keys only with the macOS keymap
  (`crates/workbench/tests/it/occurrence_carets.rs`: `whole_words_and_a_pasted_rectangle`): ⌃⌘G
  and ⌃G in `x xz x` give two carets; ⇧⌘8, ⇧→, ⇧↓ ⇧↓, ⌘C, ⇧⌘8, ⌘→, ⌘V give the indented block and
  ⌘Z takes it back.

- **AK-13 (v5, behaviour 9)** Unit tests `caret_gesture::tests` in `ferris-idea-workbench`: ⌥ down,
  up, down within 300 ms arms the gesture and keeps it while held; a slow step, another modifier,
  another key and the release disarm it; a single press does not arm it.
- **AK-14 (v5, behaviour 10)** Unit test `completion_model::tests::a_suggestion_goes_in_at_every_caret`:
  the ranges a suggestion replaces at empty carets and at a selection, and the offsets after the
  inserted texts.
- **AK-15 (v5)** In a window of the test harness with the macOS keymap
  (`crates/workbench/tests/it/multi_caret_gestures.rs`): ⌥ pressed twice and held, ↓ ↓ gives three
  carets that type together and one ⌘Z takes the typing back; after releasing ⌥, ⌥↑ is Extend
  Selection again (`clone_caret_with_the_doubled_option_key`); with three carets after `ar` a
  suggestion `area` accepted with Enter gives `area` at all three carets, one ⌘Z takes it back
  (`a_suggestion_goes_in_at_every_caret`).

## Out of scope
Rendering, mouse handling, soft wrap, rectangular paste of whole lines, smart indentation, file watching
(own spec).

## Known deviations from the reference
Default key bindings come with the keymap port (M3.9); behaviour to be compared in the guided session.
(v3) A caret inside a word goes to the word's end when the word is selected (behaviour 4a); the
reference's behaviour in that case was not observed.
(v4) The reference keeps whole-word matching also through Find Next and Find Previous between the
occurrence commands; Ferris IDEA ends it with any other command, Find included (behaviour 4b).
Behaviour 6a follows the reference's way for languages without a formatter; in source files
Ferris IDEA keeps its own re-indentation (SPEC-0057 v3 behaviour 19), and Markdown, which the
reference formats, keeps its pasted text as it is.
(v5) Behaviours 9 and 10 were not compared side by side; the timing of behaviour 9 follows the open
sources, and how the reference places a template's tab stops at several carets was not observed.
