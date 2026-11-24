# SPEC-0080: Parameter Info

- **Version:** 5 · **Date:** 2026-11-18 (v5: where the popup stands when it is wider than the room right of the caret, behaviour 9, AK-7; window check 2026-11-18 gap P1, F-031); 2026-11-14 (v4: `MethodOverloadSwitchUp` and `MethodOverloadSwitchDown` choose the highlighted signature, behaviour 8, AK-6; eighth package of docs/research/KEYMAP_COVERAGE.md; v3: `NextParameter` and `PrevParameter` move the caret from argument to argument while parameter info is shown, behaviour 7, AK-5; sixth package of docs/research/KEYMAP_COVERAGE.md); 2026-10-31 (v2: opens by itself after `(`)
- **Sources:** GEN (the signatures of the called function above the caret, with the current parameter
  highlighted; F-031), OSS (action id `ParameterInfo` with ⌘P / Ctrl+P in the ported keymaps), STD (LSP
  `textDocument/signatureHelp` with parameter label offsets and active parameter), SPEC-0072 (requests
  on unsaved text)
- **Feature matrix rows:** F-031

## Purpose
See which arguments a call takes, and which one you are typing, without leaving the call.

## Behaviour
1. **Start:** `ParameterInfo` asks the active file's language server for the signatures of the call
   around the caret on the editor's current text. Outside a call the status bar says "No parameters to
   show here."; a failure is shown there too. [GEN, OSS, STD]
2. **Popup:** above the caret's line, anchored where it was opened and kept inside the window, one row
   per overload in the editor font. The overload the server picks is shown in normal colour with the
   parameter the caret is in bold; the other overloads are dimmed. [GEN]
3. **Follow:** while open, every edit or caret move in that editor asks again (one request at a time;
   the latest caret wins). The highlighted parameter moves with the caret; when the caret leaves the
   call (the server has no signature) the popup closes. [GEN]
4. **Close:** Escape closes the popup and is not passed on. [GEN]
5. **MCP:** `signature_help` returns the overloads with labels, documentation, parameter ranges
   (chars of the label), the active overload and the active parameter for a position of a saved file.
   [ADR-0008]
6. **Automatic:** one second after `(` was typed (the reference's default autopopup delay), parameter
   info opens by itself if the caret is still right after the parenthesis, the same editor is active and
   no completion list or parameter info is open. Outside a call nothing is shown and nothing is reported.
   [GEN]
7. **From argument to argument (v3):** while parameter info is shown, `NextParameter` (Tab in
   the bundled keymaps) puts the caret at the start of the argument after the one it is in, and
   `PrevParameter` (⇧Tab) at the start of the one before, in the innermost call around the caret;
   the highlighted parameter follows (behaviour 3). Without a further argument in that direction,
   without parameter info, while the completion list is open or a template is being filled in, the
   key keeps what it does in the editor (indenting, the list's choice, the next template place).
   Arguments are what the commas outside nested brackets and outside string literals of one
   line separate; an argument starts at its first character that is no white space, an empty
   place after a comma counts and is entered at its end. The bracket of the call is looked for
   backwards from the caret without regard to string literals. A selection is dropped and
   several carets become one. [OSS: ids,
   titles and keys of the ported keymaps; "only while the hint is shown, only with a further
   argument, the caret goes to the start of that argument" from `PrevNextParameterHandler.java`
   and `ParameterInfoControllerBase.java` of the open platform, logic template. The open
   platform finds the arguments in its syntax tree and, with parameter name hints on completion,
   also leaves the call with Tab; Ferris IDEA reads the text and never leaves the call]
8. **Choosing an overload (v4):** while parameter info shows more than one signature,
   `MethodOverloadSwitchDown` (⌥↓ in the bundled macOS keymap, Ctrl+Down in the default one)
   highlights the signature below the highlighted one and `MethodOverloadSwitchUp` (⌥↑,
   Ctrl+Up) the one above, wrapping at the ends: it is drawn in normal colour with its active
   parameter bold, the others dimmed (behaviour 2). The choice holds while the popup follows
   typing and caret moves (behaviour 3) as long as the server answers with the same number of
   signatures; another number, a new popup and Escape go back to the server's choice. With one
   signature, without parameter info and while the completion list is open the keys keep their
   other actions (Extend and Shrink Selection in the editor). The text is not changed. No MCP
   tool: `signature_help` already returns every overload. [OSS: ids, keys and the titles
   "Previous Method Overload" and "Next Method Overload" of the open platform; GEN]

9. **Placement (v5):** the popup stands over the caret's line whenever it fits there, under it
   when only that fits, else on the roomier side, and starts at the caret. It moves left only as
   far as it must to stay inside the editor area, and never further left than the editor area's
   edge: a popup wider than the room right of the caret (the twelve overloads of Java's
   `List.of`) is shifted, not turned around to end at the caret, so it never covers the project
   tree. Its size is taken from the longest signature in the editor font and the number of
   signatures; the window keeps it inside its edges. [GEN; window check 2026-11-18 P1]

## Acceptance criteria
- **AK-1** Parsing of signature help: parameter labels as UTF-16 offsets and as plain strings, active
  parameter per signature or global, empty and null answers (unit test); with clangd on unsaved text the
  second argument of `add(1, 2)` is active (integration test).
- **AK-2** MCP `signature_help` reports the active parameter (protocol test with clangd).
- **AK-3** In a real window (`ferris-idea ui --selftest`, with clangd): typing `calc(` opens the
  signature by itself; typing `)` leaves the call and closes it; after Backspace ⌘P shows it again;
  Escape closes it.
- **AK-4** Use by hand: several parameters, overloads (C++), moving with the arrow keys.
- **AK-5 (v3, behaviour 7)** The offset for calls with one, several and no arguments, nested
  calls, brackets, string literals with commas, an empty last place and a caret outside every
  call (`keymap_actions::tests::parameter_steps_go_to_the_starts_of_arguments`); in a window of
  the test harness, with parameter info shown, the actions move the caret forth and back and
  return the key at both ends, and do nothing without parameter info
  (`sixth_package_keymap.rs::next_and_prev_parameter_move_between_arguments`). With a language
  server and on screen: not checked.
- **AK-6 (v4, behaviour 8)** The next index in both directions, wrapping, and no index for one
  signature (`keymap_actions::tests::overloads_step_and_wrap`); in a window of the test harness
  with three signatures shown, ⌥↓ and ⌥↑ move the highlight and wrap, the choice holds through a
  refresh with the same count and the keys reach Extend Selection with one signature
  (`eighth_package_keymap.rs::overloads_are_chosen_in_parameter_info`). With a language server:
  not checked.

- **AK-7 (v5, behaviour 9)** Placement without a window: over the line, under it near the top,
  shifted left only as far as needed, never left of the editor area, starting at the caret when
  it fits (`popup_place::tests::parameter_info_over_the_line_at_the_caret`,
  `wide_parameter_info_stays_over_the_editor`); in a window of the test harness the twelve
  overloads of `List.of` at a caret far right stand over the editor at the caret's line
  (`eighth_package_keymap.rs::wide_parameter_info_stays_over_the_editor`, macOS). The editor
  drawn empty under the popup's line (window check P1): window check needed.

## Out of scope
Opening after accepting a completion, a setting for the delay, parameter name hints inside the code, documentation in the popup, rewriting the arguments for another overload (v4: choosing which overload is highlighted is behaviour 8).

## Known deviations from the reference
The signatures come from the language server; the delay is fixed at the reference's default and cannot
be changed yet.
- v3: Tab never leaves the call and never switches to another overload; the reference's
  `MethodOverloadSwitchUp` and `MethodOverloadSwitchDown` belong to its parameter name hints on
  completion, which Ferris IDEA does not have. Not compared next to the reference.
- v4: the reference's two actions belong to its Java support, where they also put the arguments
  of the other overload into the code; Ferris IDEA chooses the highlighted signature in every
  language and changes no text. Not compared next to the reference.
