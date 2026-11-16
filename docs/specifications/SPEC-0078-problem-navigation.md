# SPEC-0078: Jump between problems in the editor

- **Version:** 2 · **Date:** 2026-11-06 (v2: hover popup with the first fix, the inspection's
  description on a second ⌘F1, ⌥⇧↩ — OBS-0019 items 2–3)
- **Sources:** GEN (next/previous highlighted problem and its description by shortcut; F-032), OSS
  (action ids `GotoNextError` F2, `GotoPreviousError` ⇧F2 and `ShowErrorDescription` Ctrl+F1, ⌘F1 on
  macOS, in the ported keymaps), SPEC-0070 (diagnostics in the editor), SPEC-0077 (popup below the
  caret); v2: OBS-0019 items 2–3, SPEC-0098 v3 (descriptions), SPEC-0125 v2 (fixes)
- **Feature matrix rows:** F-032

## Purpose
Walk through the errors of a file from the keyboard and read each message where it occurs.

## Behaviour
1. **Next / previous:** `GotoNextError` puts the caret at the start of the next problem after the
   caret, `GotoPreviousError` at the start of the previous one before it; both wrap around at the end
   of the file. When the file has errors only errors are visited; otherwise warnings and weaker
   problems. Problems starting at the same place count once. Without problems the status bar says "No
   problems in this file." [GEN, OSS]
2. **Message:** after the jump the problem's message (with its source, e.g. `clang:`) appears in the
   popup below the caret (SPEC-0077) and in the status bar (SPEC-0070). [GEN]
3. **Description:** `ShowErrorDescription` shows the message of the most serious problem at the caret
   in the same popup; nothing happens when there is none. Escape closes the popup, other keys close it
   and act as usual. [GEN]
4. Agents read the same problems with the MCP tool `diagnostics` (SPEC-0023); jumping is an editor
   action without its own tool. [ADR-0008]
5. **Hover (v2):** resting the mouse pointer on a problem's text (wavy or grey) for half a second shows
   the problem popup at the pointer: the messages of the problems under the pointer, most serious
   first; below them, when IDECraft has a quick fix for one of them, the first fix's title as a link
   with its shortcut ⌥⇧↩, and a link "More actions…" with ⌥↩. Clicking the fix link applies it like
   ⌥↩ would (one undo step); "More actions…" moves the caret to the problem and opens the ⌥↩ list.
   Moving the pointer off the problem and the popup, typing, scrolling or Escape hides it. [OBS-0019]
6. **First fix and description (v2):** the popup of behaviour 3 shows the same fix links as behaviour 5.
   `ShowErrorDescription` while that popup is shown extends it with the description of the
   inspection that reported the problem (SPEC-0098 v3 behaviour 6); language-server problems have
   none and the popup stays as it is. [OBS-0019]
7. **Apply the first fix (v2):** ⌥⇧↩ applies the first of IDECraft's quick fixes for the problems at
   the caret, without opening a list; with none the status bar says so. [OBS-0019, GEN]

## Acceptance criteria
- **AK-1** Order, errors first, wrap-around in both directions, no problems (unit tests in
  `diagnostics_model`).
- **AK-2** In a real window (`idecraft ui --selftest`, with clangd): F2 from the top of the file jumps
  to the undeclared identifier and shows its message; Escape closes it.
- **AK-3** Use by hand: ⇧F2, several errors, warnings only, ⌘F1.
- **AK-4 (v2)** Problems under a point, the popup's lines and its fix link, the extended text with the
  description (unit tests `diagnostics_model::tests::popup_*`); window self-test: ⌘F1 twice on the
  `ready == true` finding of `java_inspection_steps` shows the fix link and then the description,
  ⌥⇧↩ applies the fix; hover by hand next to the reference.

## Out of scope
The setting to visit warnings together with errors, problems in other files (Problems tool window),
the ⋮ menu of the reference's popup, documentation of the symbol under the pointer in the same
popup.

## Known deviations from the reference
The reference has a setting for whether F2 skips warnings when errors exist; IDECraft always visits
errors first and has no setting yet. v2: compared side by side in OBS-0019; F2 keeps a one-line
folded method folded in the reference and unfolds it in IDECraft.
