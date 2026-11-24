# SPEC-0070: Diagnostics in the editor

- **Version:** 13 · **Date:** 2026-11-14 (v13: the workbench's own status note goes after a fixed time, failures later, behaviour 4d); 2026-11-13 (v12: occurrence marks below problems, behaviour 14, SPEC-0295; v11: a tool's finding does not hide a fresh note, behaviour 4c, AK-11, OBS-0028 T6); 2026-11-11 (v10: a passing hint leaves with the caret, behaviour 4b — OBS-0027 second window re-check; v9: a failure the user just caused is held in front of the caret's problem, behaviour 4a — OBS-0034 L4; v8: the second view counts the same problems, the compact form follows a split at once, behaviour 13; v2: the problem under the caret wins over older notes; v3: diagnostics while typing through the server pool, SPEC-0090; v4: the counts move to a problem indicator in the editor's top right corner, behaviour 3; v5: grey unused code, one problem per place, error stripe,
  weak warnings and arrows at the indicator, problems inside folded text — OBS-0019 items 1–2; v6: the
  highlighting level per file at the indicator — defect F032-1; v7: a compact indicator in narrow
  editors and one per split view, behaviour 12 — OBS-0022 L27)
- **Sources:** GEN (errors and warnings of the analysis appear as wavy underlines, with counts in the
  editor and the message for the caret position; F-032), OSS (scheme keys `ERRORS_ATTRIBUTES`,
  `WARNING_ATTRIBUTES`, `WEAK_WARNING_ATTRIBUTES` with their effect colour and wave effect), SPEC-0023 (LSP
  diagnostics), SPEC-0058 (editor view); v4: OBS-0011 item 3 (the reference shows a check mark at the
  editor's top right for a file without problems, counts otherwise), SPEC-0092 (Problems tool window); v5: OBS-0019 items 1–2, OSS (scheme keys
  `NOT_USED_ELEMENT_ATTRIBUTES` and the `ERROR_STRIPE_COLOR` of the problem keys), STD (LSP
  `DiagnosticTag.Unnecessary`); v6: OBS-0019 item 2 (resting on the indicator shows the summary and
  a choice of the highlighting level: none, syntax, essential, all), OSS (action id `PopupHector`
  with Ctrl+Alt+Shift+H in the ported default keymap)
- **Feature matrix rows:** F-032

## Purpose
See the language server's errors and warnings where they are, while editing.

## Behaviour
1. **When:** when a file opens in a tab and after each successful save, its diagnostics are requested in
   the background from the language's server (SPEC-0023: open, wait for `publishDiagnostics`, stop; 30 s
   limit). The window never waits. A missing server or a timeout leaves the editor without problems. [STD]
2. **Underlines:** each diagnostic is mapped onto the buffer (UTF-16 columns converted to chars; an empty
   range covers the next char, or the previous one at a line end) and drawn as a wavy underline in the
   effect colour of `ERRORS_ATTRIBUTES` (errors), `WARNING_ATTRIBUTES` (warnings) or
   `WEAK_WARNING_ATTRIBUTES` (information, hints). Until fresh diagnostics arrive, underlines move with
   edits; one inside replaced text disappears. [GEN, OSS]
3. **Counts (v4):** a problem indicator sits at the top right corner of the editor, above the text:
   a check mark in the theme's success colour while the file has neither errors nor warnings, else the
   number of errors (●) and of warnings (▲) in their colours, each only when not zero. It follows the
   same problems as the underlines, live. A click on it opens the Problems tool window (SPEC-0092).
   The status bar no longer repeats the counts. [OBS-0011, GEN]
4. **Message:** when the caret is on a problem, the status bar shows its message with the source in front
   (e.g. `clang: use of undeclared identifier 'missing'`); the most serious one wins. It takes the place of
   older notes such as the save message, which come back when the caret leaves the problem. [GEN]
   - **4a. A held failure (v9, OBS-0034 L4):** an action may report its failure as *held* (so far:
     a Python install that failed or was refused, SPEC-0117 v5 behaviour 9). A held message is
     shown instead of the problem at the caret, because it answers what the user just did there.
     It is held exactly as long as all of this is true: the active editor tab is the one that was
     active when the failure was reported (or there was and is none), the primary caret of that
     editor is at the same offset, and no other status note has been set since. With the first
     change of any of them (the caret moves, also through typing; another tab becomes active; any
     later note) the hold ends for good: the message is then an ordinary note under the rule
     above, so the problem at the caret wins again, even if the caret returns to the old place.
     Messages of successes and progress are never held. [GEN, OBS-0034]
   - **4b. A passing hint (v10, OBS-0027):** an action that cannot start where the caret is may report
     that as a *passing hint* (so far: Generate outside a class, SPEC-0122 v5 behaviour 3c). It is held
     under the conditions of 4a, but when the hold ends it is removed instead of staying as an ordinary
     note: the status bar then shows the problem at the caret or the editor's own note. The action also
     ends it when it shows something of its own (a list opens). [GEN, OBS-0027]
   - **4c. A fresh note before a tool's finding (v11, OBS-0028 T6):** a finding of a tool that runs
     after each change of the text — ESLint (SPEC-0123), a framework model — does not take the place
     of a status note set less than 4 s ago: the note is the result of what the user just did
     (Optimize Imports' summary), and the tool's finding came from that very change. After 4 s, at
     the next update of the status bar, the finding shows again. A language server's problem keeps
     winning at once, so after saving the problem under the caret still replaces "Saved" (hand test
     2026-11-01). [GEN, OBS-0028]
   - **4d. A note goes (v13, window check 2026-11-14):** the workbench's own note in the status bar
     — the outcome of an action or why it could not be done — gives way to the next note at once,
     and otherwise goes by itself: a hint or outcome after 10 s, a failure (a note that says
     something could not be done, failed, was refused or not found) after 30 s. Running a statement
     takes the note away before it runs. Problems under the caret (behaviour 4) are not notes and
     stay while they hold. Until v13 a note stood until the next one. [GEN]
5. The MCP counterpart is `file_diagnostics` (SPEC-0023). [ADR-0008]
6. **Unused code (v5):** a problem that marks something unused — Ferris IDEA's rules `java.unused-import`,
   `java.unused-local`, `java.unused-private` and `java.unused-parameter` (SPEC-0125 v2), and every
   language-server diagnostic tagged *unnecessary* — is drawn as text in the foreground colour of
   `NOT_USED_ELEMENT_ATTRIBUTES` (grey) instead of a wavy underline. Its severity, count, message and
   fixes stay. [OBS-0019, OSS, STD]
7. **One problem per place (v5):** when one of Ferris IDEA's findings covers exactly the same text as a
   language-server diagnostic (same start and end), only the server's diagnostic is shown and counted;
   the own quick fix stays available in ⌥↩. [OBS-0019]
8. **Error stripe (v5):** along the right edge of the editor's text area runs a stripe 12 px wide. Each
   line with problems gets a mark 3 px high at the height that the line has in the whole document
   (line ÷ line count × stripe height), in the `ERROR_STRIPE_COLOR` of the most serious problem's key
   (`ERRORS_ATTRIBUTES`, `WARNING_ATTRIBUTES`, `WEAK_WARNING_ATTRIBUTES`; unused code uses
   `NOT_USED_ELEMENT_ATTRIBUTES`). A click on a mark (±3 px) puts the caret at the start of the most
   serious problem of that line and scrolls it into view. Resting the pointer on a mark shows a tooltip
   with the messages of that line, most serious first, one per line. A file without problems shows no
   marks. [OBS-0019, OSS]
9. **Indicator (v5, extends 3):** weak warnings and information count separately and are shown as a
   third count in the weak-warning colour; the check mark stands only when the file has no problem at
   all. Two arrow buttons ↑ ↓ follow the counts and act as `GotoPreviousError` and `GotoNextError`
   (SPEC-0078). Resting the pointer on the counts shows a tooltip with the summary in words, e.g.
   "1 error, 4 warnings, 1 weak warning"; the Problems tool window's header uses the same words.
   [OBS-0019]
10. **Folded text (v5):** when a folded region hides a problem that is not unused code, its placeholder
    is drawn with the wavy underline of the most serious hidden one, so the problem stays visible.
    [OBS-0019]
11. **Highlighting level (v6):** each file has a highlighting level, **All** by default:
    - **None:** no problem is shown — no underline, no grey text, no stripe mark, no count;
    - **Syntax:** only errors;
    - **Essential:** errors and warnings, no weak warnings, information or grey unused code;
    - **All:** every problem.
    The level decides what the editor shows and counts, what F2/⇧F2 visit and what the Problems
    tool window's File tab lists (SPEC-0092); the analysis itself still runs, so a higher level
    shows the problems at once. Resting the pointer on the indicator (or the action `PopupHector`,
    Ctrl+Alt+Shift+H in the default keymap) opens a small popup below it: the summary in words of
    behaviour 9, then the four levels with the current one marked; a click (or Up/Down and Enter,
    Escape closes) sets the level for this file. The popup stays while the pointer is on the
    indicator or on the popup. With a level other than All the indicator starts with the level's
    name ("Syntax", "Essential") or, for None, a crossed-out eye "⊘" instead of the counts, and the
    popup says how many problems the level hides, e.g. "4 problems hidden by the level" (IMP-0310).
    The levels are kept per project in `.ferris-idea/highlighting.json` (`{"files": {"<path relative
    to the project>": "syntax"}}`; files at All are not written) and read when a file opens and
    when its problems are recomputed. The MCP tool `highlighting_level` (path; optional `level`
    `none`, `syntax`, `essential`, `all`) reads or sets the level of a file and returns it with
    what it shows. [OBS-0019, OSS, GEN]
12. **Narrow editors (v7):** in an editor view narrower than 520 px (one half of a split, a small
    window) the indicator shrinks to one sign and one number: the sign of the most severe kind
    present with the sum of all shown problems, the check mark for none, or the level's mark for
    None; the level name and the arrows are left out (the popup of behaviour 11 keeps the summary and
    the levels). It stands on the editor's background with the popup border, so the code under it
    is not shown through it, and it takes at most 64 px; a click opens the Problems tool window. The
    second group's view (SPEC-0112) has its own indicator for its file, always in the compact form.
    Before v7 the full indicator covered up to 200 px of code in a narrow half and the second view
    had none. [OBS-0022 L27, GEN]
13. **v8 – Second view and narrow views:** the second group's view of a file (SPEC-0112) gets the same problems as the first (language server, frameworks, plugins), so both compact indicators show the same sign and sum; a view that becomes narrow (a split) or wide again redraws the indicator in the next frame instead of after the next input. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L25, L27]
14. **v12 – With occurrence marks:** the marks of the usages at the caret (SPEC-0295) are backgrounds
    below the find results and the selection; a problem's wavy underline and the grey of unused code
    are drawn over them and stay visible. In the error stripe a line's problem mark is drawn over
    its occurrence mark, and a click there goes to the problem. [GEN, SPEC-0295]

## Acceptance criteria
- **AK-1** Mapping with UTF-16 columns, empty ranges, sorting, counts and the problem at an offset (unit
  test); effect colours read from the bundled scheme (unit test).
- **AK-2** In a real window (`ferris-idea ui --selftest`, with `clangd` installed): opening a C file with an
  undeclared identifier shows one error.
- **AK-3** Look by hand: wave, colours, message.
- **AK-4 (v4)** The indicator is a check mark without errors and warnings, and shows only the non-zero
  counts otherwise (unit test `diagnostics_model::tests::indicator_is_a_check_mark_or_counts`); the window
  self-test sees the indicator on an open file (`ferris-idea ui --selftest`).

- **AK-5 (v5)** Unused problems are marked as such (own rules and the server's unnecessary tag), an own
  finding on the same range as a server diagnostic is dropped, stripe marks are placed and coloured
  per line, the summary words and the three counts, the most serious problem inside a folded range
  (unit tests in `diagnostics_model`); the window self-test sees stripe marks and the third count on
  the Java file of `java_inspection_steps`; by hand next to the reference (OBS-0019 re-check).

- **AK-6 (v6)** The four levels filter the problems as stated, the indicator's label per level and the
  hidden count, reading and writing `.ferris-idea/highlighting.json` (unit tests
  `diagnostics_model::tests::highlighting_levels_filter_problems` and
  `ferris_idea_languages::highlighting::tests`); MCP `highlighting_level` reads, sets, resets and refuses
  an unknown level (`crates/mcp/tests/it/protocol.rs`: `highlighting_level_tool`); window self-test
  (`java_inspection_steps`, written): setting Syntax on the Java file leaves only errors, All brings
  the warnings back; by hand next to the reference.
- **AK-7 (v7)** The compact indicator: sign of the most severe kind with the sum, check mark, level
  mark, and the width threshold (unit test `diagnostics_model::tests::compact_indicator_in_narrow_editors`);
  window self-test step (written, run later): after Split Right both halves show a compact indicator
  of at most 64 px.
- **AK-8 (v8)** By hand in the window: after Split Right both halves show the same compact sign and sum at once.

- **AK-9 (v9)** In a window: with the caret on a problem a held failure is shown instead of the problem's
  message; after the caret moved the problem's message is back, also at the old offset; off the problem the
  failure shows as the last note (`crates/workbench/tests/it/python_install_failure.rs`).
- **AK-10 (v10)** In a window: a passing hint shows until the caret moves and is gone afterwards, also back
  at the old offset; an opening list removes it
  (`crates/workbench/tests/it/java_generate_lists.rs`: `the_hint_outside_a_class_passes`).

- **AK-11 (v11, behaviour 4c)** The order of the status bar: a note younger than 4 s before a tool's
  finding, the finding afterwards, a language server's problem before a fresh "Saved"
  (`workbench_view::status_order_tests::a_fresh_note_stands_before_the_problem_at_the_caret`).
- **AK-13 (v12, behaviour 14)** By hand in a window, not done yet: an error on a marked occurrence keeps
  its underline; the stripe shows both marks (SPEC-0295/AK-8).
- **AK-12 (v13, behaviour 4d)** `status_note_model::tests::notes_go_after_a_fixed_time_failures_later`:
  outcomes go after 10 s, failures after 30 s. By hand: Copy Path — the note goes after 10 s.

## Out of scope
The Problems tool window, quick fixes from the underline and hover popups (SPEC-0078 v2), the
reference's other entries of the indicator popup (power save mode, per-language levels of a
file with injected languages).

## Known deviations from the reference
v3: with the language server pool (SPEC-0090) diagnostics also follow typing; open and save still ask
for them. v4: the corner indicator does not show an "analysing" state while diagnostics are still on the
way; until they arrive it shows the check mark. v5: compared side by side in OBS-0019. v6: the
reference decides "syntax" and "essential" by kinds of analysis; Ferris IDEA decides by severity,
because the language servers do not mark which of their errors are syntax errors. The reference
keeps the levels in its workspace settings; Ferris IDEA keeps them in `.ferris-idea/highlighting.json`,
which agents read and change through MCP, and the popup names the number of hidden problems
(IMP-0310). The reference also greys unused public methods and classes in the editor (OBS-0019
item 1); Ferris IDEA's editor marks unused code within the file only and reports the project-wide
case as the global inspection of SPEC-0316 in Inspect Code.
