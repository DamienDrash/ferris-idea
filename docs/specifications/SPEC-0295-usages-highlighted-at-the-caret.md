# SPEC-0295: Usages highlighted at the caret

- **Version:** 2 · **Date:** 2026-11-13 (v2: `GotoNextElementUnderCaretUsage` and
  `GotoPrevElementUnderCaretUsage` step through the usages, behaviour 15, AK-9)
- **Sources:** GEN (an editor marks the other occurrences of the name the caret is on, and tells
  places that change a variable from places that read it; F-034, F-013), STD (LSP 3.17
  `textDocument/documentHighlight`: the server capability `documentHighlightProvider`, the answer as
  ranges with the kinds 1 *text*, 2 *read* and 3 *write*, *text* when no kind is given; `$/cancelRequest`),
  OSS (the action id `HighlightUsagesInFile` with Ctrl+Shift+F7 / ⇧⌘F7 in the ported keymaps; the
  scheme keys `IDENTIFIER_UNDER_CARET_ATTRIBUTES` and `WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES` with
  their background and `ERROR_STRIPE_COLOR` in the bundled colour schemes), DOC (the reference marks the
  usages of the element at the caret by itself, has a switch for it on its general editor page and an
  action that marks the usages in the file until Escape; facts only, in own words), SPEC-0042 and
  SPEC-0090 (language servers, the pool, readiness), SPEC-0060 (colour schemes), SPEC-0065 (find
  results and their stripe marks), SPEC-0070 (problems and the error stripe), SPEC-0073 and SPEC-0293
  (usages and their wording), SPEC-0260 behaviour 4 (large texts), ADR-0008
- **Feature matrix rows:** F-034, F-013

## Purpose
See at a glance where else in the file the name under the caret is used, and where it is changed,
without starting a search; and keep these marks while reading through the file.

## Behaviour
1. **Automatic highlight:** when the editor's only caret, without a selection, has rested for 300 ms
   on a name (letters, digits and `_`; the caret inside the name or directly before or after it),
   the editor asks the file's language server for the occurrences of that symbol in the file and
   marks them. The rest is checked every 100 ms, so the question is sent 300 to 400 ms after the
   last movement; the marks appear when the server has answered. Typing, moving and scrolling never
   wait for it. [GEN, STD]
2. **Only what the server says:** the occurrences come from `textDocument/documentHighlight` on the
   editor's current text. Nothing is marked when the file has no language server, when the server
   does not announce `documentHighlightProvider`, when it answers with an error or not within 10 s,
   or when the answer is empty. There is no fallback to occurrences of the same text: a mark on a
   different symbol with the same name is worse than no mark. [STD, GEN]
3. **A server that is not ready is not asked:** while the pool is starting the file's server or the
   server is still loading the project (SPEC-0042 behaviours 6, 13, 24, 25; `pool::loading`), no
   question is sent. The rest of the caret is not used up by that: the question follows when the
   server is ready and the caret still rests there. [SPEC-0042, SPEC-0090]
4. **Colours:** an occurrence of kind *write* has the background of
   `WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES`, one of kind *read* or *text* the background of
   `IDENTIFIER_UNDER_CARET_ATTRIBUTES`, both from the colour scheme in use (SPEC-0060 behaviour 3).
   A scheme that defines no background for a key draws nothing for it; IDECraft has no colour of its
   own for these marks. [OSS]
5. **Layers:** from bottom to top the editor draws the caret row and line backgrounds, the
   occurrence marks, the find results (SPEC-0065 behaviour 5), the selection, the text with the wavy
   underlines of problems (SPEC-0070 behaviour 2). A find result or a selection therefore covers an
   occurrence mark; a problem's underline stays visible on it. [GEN]
6. **Stripe marks:** as for find results (SPEC-0065 behaviour 9a), each line with an occurrence gets
   a mark in the editor's error stripe, in the `ERROR_STRIPE_COLOR` of the occurrence's key, else its
   background; a line with a write occurrence takes the write colour. A click puts the caret on the
   line's first occurrence. Problem marks are drawn over them. [OSS, SPEC-0065, SPEC-0070]
7. **When the automatic marks go:** they are removed as soon as the caret is no longer inside or at
   the edge of one of the marked occurrences, when a second caret or a selection appears, and with
   every change of the text. While the caret moves within the marked occurrences (also from one to
   another) they stay and nothing is asked. [GEN]
8. **`HighlightUsagesInFile`** (Ctrl+Shift+F7, ⇧⌘F7 on macOS; Find Action lists it as "Highlight
   Usages in File"): marks the occurrences of the symbol at the primary caret, from the same
   question, and keeps them when the caret moves, until Escape or a change of the text. The status
   bar says how it ended, in the words of Find Usages (SPEC-0293 behaviours 4 and 9):
   - `1 usage of <symbol> highlighted (Escape removes the highlighting)`, `N usages of <symbol>
     highlighted (…)`; with more occurrences than behaviour 11 draws, `N usages of <symbol>, the
     first 1000 highlighted (…)`;
   - `No usages of <symbol> found in this file` when the server answers with none;
   - `No usages found: <server> does not offer usage highlights` when the server lacks the
     capability or does not know the request;
   - `No usages found yet: <server> is still loading the project` when the server is not ready
     (behaviour 3; nothing is asked and nothing is queued);
   - `No usages found: the caret is not on a name`, `No usages found: the file is larger than 8 MiB`,
     `No usages found: the text changed meanwhile`, and for a file without a usable server or a
     failed question `No usages found: <reason>`.
   A second use on another symbol replaces the kept marks. [OSS, DOC, GEN, SPEC-0293]
9. **Kept marks and the automatic highlight:** while kept marks are shown the automatic highlight is
   off in that editor. Escape removes the kept marks and does nothing else (carets and selection
   stay; a second Escape acts as before); the automatic highlight then starts again with the next
   movement of the caret or change of the text, not for the place the caret rests on. An inline
   completion shown at the same time is dismissed by the first Escape, the marks by the next. [GEN]
10. **One question at a time:** an editor has at most one question in flight. A question whose reason
    is gone — the caret moved away, the text changed, the setting was switched off — is withdrawn
    with `$/cancelRequest`; the next one is sent only when the withdrawn one has ended. An answer is
    dropped when the text is not the one it was asked for (the editor's revision changed), and an
    automatic answer also when the caret is no longer on one of the answered occurrences. A use of
    `HighlightUsagesInFile` while a question is in flight withdraws it and is asked next. [STD, GEN]
11. **Bounds:** at most 1000 occurrences are drawn, the first ones in the order of the text;
    occurrences without text (empty or reversed ranges) and ranges outside the text are left out,
    equal ranges are drawn once with the strongest kind (write before read before text). For a text
    over 8 MiB, the limit above which the editor has no syntax tree (SPEC-0260 behaviour 4), nothing
    is asked. [GEN, SPEC-0260]
12. **Setting:** Editor › General › Code Insight › *Highlight usages of the symbol at the caret* (on
    by default), stored as `highlight_usages_at_caret` under `[editor]` in `settings.toml` like its
    neighbours (SPEC-0129 behaviour 1). Off, the editor neither asks by itself nor keeps automatic
    marks, in every open editor at once; `HighlightUsagesInFile` works all the same. Also
    `highlight_usages_at_caret` in `set_ide_settings`. [GEN, DOC, ADR-0008]
13. **Agents:** MCP tool `document_highlights` (path, 0-based line and character): `occurrences`
    with 0-based start and end line and character (Unicode scalar values) and `kind` (`read`,
    `write`, `text`), in the order of the text, not bounded by behaviour 11; `offered: false` with
    the server's name when it has no usage highlights. Unlike the editor the tool waits for a server
    that is starting, within the usual time limit of the code insight tools. [ADR-0008]
14. **Client capability:** IDECraft announces `textDocument.documentHighlight` in `initialize`. [STD]

### Version 2
15. **Next and previous usage** (`GotoNextElementUnderCaretUsage`, ⌃⌥↓, and
    `GotoPrevElementUnderCaretUsage`, ⌃⌥↑, in the macOS keymap; the default keymap gives them no
    key, so there they are run from Find Action or given a key in Settings › Keymap. Find Action
    lists them as "Next Highlighted Usage" and "Previous Highlighted Usage" under Navigate): the editor's only caret, without a
    selection, goes to the start of the next or the previous usage of the symbol it is on, in the
    order of the text; after the last usage comes the first and before the first the last. The
    target is scrolled into view and a fold around it is opened, as for any jump in the file.
    - **With marks:** when the caret is inside or at one of the marked occurrences (automatic or
      kept), the step is taken among these marks at once, without a question to the server. The
      marks stay: the caret is on one of them again (behaviours 7 and 9).
    - **Without marks:** when the caret is on a name that is not marked — the rest of behaviour 1
      has not passed, the automatic highlight is off, or kept marks belong to another symbol — the
      server is asked for this place as in behaviour 2, and the step is taken when the answer
      comes, from the answered occurrences. A question in flight is withdrawn first (behaviour
      10). With the automatic highlight on and no kept marks the answer is also marked, as if the
      caret had rested; otherwise the marks stay as they are. The step is given up when the caret
      moves or the text changes before the answer (behaviour 10).
    - **Nothing to go to:** with several carets or a selection nothing happens. Otherwise the
      status bar says why in the words of behaviour 8: the caret is not on a name, the file has no
      usable server, the server is still loading (it is not asked and nothing is queued), has no
      usage highlights, the file is too large, the text changed meanwhile, or the server knows no
      occurrence. When the symbol's only usage in the file is the one the caret is on, the caret
      stays and the status bar says `No other usage of <symbol> in this file`.
    - More than the 1000 occurrences that are drawn (behaviour 11) are still stepped through when
      the step is taken from an answer; from marks only the drawn ones.
    - **Agents:** `document_highlights` also returns `next` and `previous`: the 0-based indexes in
      `occurrences` of the usages these actions go to from the asked position (wrapping; the
      nearest one after or before the position when it is on none), `null` without occurrences.
    [OSS (action ids and keys of the ported keymaps), GEN, STD, ADR-0008]

## Acceptance criteria
- **AK-1** The capability is read from `documentHighlightProvider` (`true` or an object); an answer
  is decoded with its kinds, a missing kind as *text*, UTF-16 positions as characters, sorted,
  without empty ranges and with equal ranges once (unit tests `occurrences::tests`).
- **AK-2** With a scripted server: the question carries the caret's position and its answer comes
  back with read and write kinds; a server without the capability is not asked at all, and one that
  answers "method not found" counts as not offering; a stopped question is withdrawn with
  `$/cancelRequest`; a text over 8 MiB is not asked about (unit tests `occurrences::tests`).
- **AK-3** The editor's model with a scripted server (unit tests `occurrence_model::tests`):
  the question is due only after 300 ms of rest on a name, not on white space, not with the setting
  off, not while the server is not ready (and then as soon as it is); a server without the
  capability leaves the editor unmarked; read and write kinds reach the marks; an answer for an
  older text and an automatic answer after the caret left are dropped; one question at a time, the
  withdrawn one first ends; the marks go when the caret leaves the symbol and with a change of the
  text, and stay while the caret moves between occurrences; more than 1000 occurrences are cut.
- **AK-4** Kept marks (same tests): `HighlightUsagesInFile` keeps its marks when the caret moves,
  suspends the automatic highlight, and Escape removes them without bringing an automatic highlight
  back for the resting caret; the status texts of behaviour 8 for one, several, more than 1000 and
  no usages, a server without the capability, a server that is not ready and a changed text.
- **AK-5** The bundled schemes define a background for both keys, and the write background differs
  from the read one (unit test in `idecraft-ui`); stripe lines carry the write kind when a line has
  both (unit test `occurrence_model::tests`).
- **AK-6** The setting is read, saved and listed in the settings format (`ide_settings::tests`,
  `format` schema); the Editor page has the switch under Code Insight; `set_ide_settings` changes it
  (`protocol.rs`: `ide_settings_tools`).
- **AK-7** With a real server (clangd; skipped without it): the occurrences of a local variable in a
  C file come back with its assignment as *write*, its uses as *read* and its declaration, to which
  clangd gives no kind although it has an initializer, as *text*; white space gives an empty answer
  (`crates/languages/tests/it/clangd.rs::clangd_highlights_the_usages_in_a_file`); the MCP tool
  returns them (`protocol.rs`: `code_insight_tools`).
- **AK-9 (v2, behaviour 15)** The step among occurrences: next and previous with wrap-around at
  both ends, from inside, the start and the end of an occurrence, from between occurrences, with
  one occurrence and with none (unit test `occurrences::tests::the_neighbour_wraps_at_the_ends`).
  The editor's model with the scripted server (unit tests `occurrence_model::tests`): with marks
  the step asks nothing and the marks stay; without marks the server is asked at once and the
  answer gives the target and the marks; with the automatic highlight off or with kept marks of
  another symbol the answer gives the target and changes no mark; a caret that moved before the
  answer gets no step; the status texts. The MCP tool returns `next` and `previous`
  (`protocol.rs`: `code_insight_tools`, with clangd). By hand in a window, not done yet: ⌃⌥↓ and
  ⌃⌥↑ in a file with a server.
- **AK-8** By hand in a window, not done yet: the marks and their colours in a dark and a light
  scheme, the stripe marks, ⇧⌘F7 and Escape, the switch on the settings page; next to the reference.

## Out of scope
Stepping through the kept marks with F3/⇧F3 or the keys of the find bar (the usages of the symbol
at the caret have their own keys since v2, behaviour 15); stepping with several carets; highlighting the usages of a `throw`, `return` or an import as exit points or
unused imports; matching braces and tags; occurrences in other files (Find Usages, SPEC-0293);
files of a remote project (SPEC-0318), where the action says that it is not available yet; a
separate question per view of a split file beyond each view's own caret; `workspace` refresh
requests of servers.

## Known deviations from the reference
The reference finds the occurrences with its own index; IDECraft asks the language server, so the
marks appear after the server's answer and only in files whose server offers them. Where the
reference may fall back to other means for files it does not understand, IDECraft marks nothing
(behaviour 2). Not compared side by side yet: which occurrences the reference marks (declarations,
occurrences in comments and strings), after which delay, whether Escape there also takes the
automatic marks away, what its action shows in the status bar and whether it adds to marks already
kept. A server decides what counts as a write: clangd reports a declaration with an initializer
without a kind, so it gets the read colour (AK-7).

v2: not compared with the reference. Open there: where in the usage its actions put the caret
(IDECraft: at its start), whether they wrap around at the ends, in which order they visit usages
(IDECraft: the order of the text), and what they do when the usages are not highlighted yet
(IDECraft asks the server and then steps).
