# SPEC-0076: Context actions from the editor (Alt+Enter)

- **Version:** 9 · **Date:** 2026-11-16 (v9: Quick Actions open the context actions, behaviour 13, AK-10; eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-13 (v2: problems on the caret's line count; hand test; v3: preview
  of the selected action, one undo step for the whole action; v4: a title listed once, OBS-0027; v5: the
  list closes the problem's popup, OBS-0027 L12; v6: jdtls's refined diagnostics as context, unused code
  as a weak warning with its removal fix, OBS-0027 JQ1 and JQ2; v7: IDECraft's own Wrap using
  'String.valueOf()' and removal of an unused local in a method with an error, OBS-0027 JQ1b and JQ2;
  v8: the refined list is waited for only after a list from the build, and long enough for a busy
  machine)
- **Sources:** GEN (a popup of fixes and intentions at the caret, opened by shortcut; F-033, F-073),
  OSS (action id `ShowIntentionActions` with ⌥↩ / Alt+Enter in the ported keymaps; bulb icons
  `codeInsight/quickfixBulb` and `codeInsight/intentionBulb` from `platform/icons/src/expui/`),
  STD (LSP `textDocument/codeAction`), SPEC-0041 (code actions through the language server),
  SPEC-0072 (requests on unsaved text),
  SPEC-0193 (one undo group across files), OSS (action id `QuickJavaDoc`, F1 / ⌃J on macOS, Ctrl+Q
  elsewhere, in the ported keymaps)
- **Feature matrix rows:** F-033, F-073

## Purpose
Fix an error or apply a refactoring the language server offers at the caret without leaving the
editor.

## Behaviour
1. **Start:** `ShowIntentionActions` asks the active file's language server for the code actions at the
   caret, or for the selection when there is one. The server receives the editor's current text and the
   diagnostics it publishes for that text as context, so unsaved changes count. When the caret is in no
   problem, the problems on the caret's line are taken and the request covers them, so a fix is offered
   anywhere on the line of an error (the error of a missing `;` is reported at the next token, which is
   easy to miss). With no actions the
   status bar says "No context actions available here."; a failure is shown there too. [GEN, STD]
2. **List:** a popup below the caret lists the actions: applicable before disabled, the server's
   preferred action first, then quick fixes, then refactorings, then the rest; equal ranks keep the
   server's order. Quick fixes (an action that fixes a diagnostic or has kind `quickfix`) show the red
   bulb, others the yellow bulb. Disabled actions are shown greyed with the server's reason and cannot be
   chosen. The first applicable action is selected. [GEN, OSS, STD]
3. **Speed search:** typing filters the list to actions whose title contains every typed word, ignoring
   case; Backspace removes the last character; the filter is shown above the list. [GEN]
4. **Keys:** Up/Down move between applicable actions with wrap-around, Enter or a click applies, Escape
   closes and returns focus to the editor. [GEN]
5. **Apply:** the chosen action is planned again on the editor's current text (edits and server
   commands, lazily computed actions resolved first, SPEC-0041 v2). The whole action is one undo group
   across all files it changes (SPEC-0193). The status bar names the action and whether it worked. [GEN]
6. The MCP counterparts are `list_code_actions` and `apply_code_action` with a preview by default
   (SPEC-0041); they work on the saved file. [ADR-0008]
7. **Preview:** the key of `QuickJavaDoc` in the popup turns a preview of the selected action on or off.
   The preview plans the action without applying it (resolving it if needed) and shows, per file, the
   changed lines as removed and added lines (at most 12 lines per file, then "… n more"), or the reason
   it cannot be planned. It follows the selection; a plan still running for an earlier selection is
   ignored when it arrives. Planning runs in the background; the popup stays usable. [GEN]
8. **One row per title (v4):** an action whose title is already in the list is left out (jdtls offers
   some source actions twice, e.g. `Generate toString()`); applying looks an action up by its title.
   [OBS-0027 L11]
9. **Nothing over the list (v5):** when the list opens — for context actions, Refactor This or a
   refactoring's choice of expressions — the popup at the caret closes: the problem's text shown by the
   resting pointer or by its key (SPEC-0078 v2) and documentation (SPEC-0077). While the list is open, a
   pointer resting on a problem or a word opens neither. Before v5 the problem's text lay over the list.
   [OBS-0027 item 5 (L12)]
10. **Refined diagnostics as context (v6):** jdtls publishes the diagnostics of an opened file in two
    steps: first those of its build of the saved file, where a type error is named by its line only
    (character 0), then those of its own reading of the opened text with exact ranges. jdtls finds a
    fix by the exact range of the problem sent as context, so with the first list it offered no fix for
    the type error and, at an unused local variable or private field, only "Ignore compiler problem(s)"
    and "Add @SuppressWarnings" instead of its removal fixes. A question to a server marked as refining
    its diagnostics (jdtls) waits up to 3 s after the first list for the second and sends the newest
    one as context; in the pool of running servers this wait happens once, when the file is opened, and
    later questions read the newest list anyway. **v8:** jdtls does not always publish two lists: when
    it has the build of the file already, the first list is the refined one, and under load the
    refined list came more than 3 s after the build's (the fix of a type error was then missing:
    server CI, 2026-11-13, LSP trace). A list from the build is told by its ranges: every problem at
    character 0 with no width. A question waits for the refined list only after such a list, and then
    up to 30 s; after a refined list it asks at once. [OBS: LSP trace of
    `the_fixes_of_a_type_error_are_found_at_its_place`, 2026-11-13] Unused code then appears as jdtls reports it: an unused
    import, private field, private method or local variable is a warning marked unnecessary, drawn as
    unused (grey, SPEC-0070 v5 behaviour 6), and ⌥↩ and the MCP tool `code_actions` offer jdtls's fixes, such as
    "Remove 'unused' and all assignments", "Remove 'unused', keep assignments with side effects",
    "Remove 'never', keep assignments with side effects" and "Remove unused import". IDECraft keeps
    jdtls's default compiler settings (these problems are warnings) and does not override a project's
    own settings. [OBS-0027 JQ1, JQ2; STD]
11. **Wrap using 'String.valueOf()' (v7):** where jdtls reports "Type mismatch: cannot convert from
    `int` to String" (likewise `long`, `double`, `boolean`, `char`) and offers no wrapping fix itself,
    ⌥↩ and `code_actions` add IDECraft's own quick fix "Wrap using 'String.valueOf()'" after jdtls's
    quick fixes. It replaces exactly the reported expression (the problem's refined range, behaviour
    10) by `String.valueOf(<expression>)` and is applied by IDECraft, also through `apply_code_action`.
    An expression over several lines, other types (the reference converts objects with `toString()`
    in other ways) and any place where jdtls offers its own wrapping get none. [OBS-0027 JQ2; GEN]
12. **Unused local in a method with an error (v7):** the Eclipse compiler skips the unused-code analysis
    of a method that contains an error, so jdtls offers no removal there; IDECraft's own Java
    inspection (SPEC-0125, `java.unused-local`) still finds the local. Where jdtls offers no removal
    ("Remove '<name>' …") at an unused local that the inspection can remove safely (one declarator, a
    value without side effects), ⌥↩ and `code_actions` list the inspection's fix "Remove variable
    '<name>'" as a quick fix, applied by IDECraft. In the window the inspection's row and this action
    share the title and are listed once (behaviour v4). [OBS-0027 JQ1b; GEN]
13. **Quick actions (v9):** `QuickActionPopup` (⌥⌘↩ in the bundled macOS keymap) opens the
    context actions at the caret, as `ShowIntentionActions` does (behaviour 1). [GEN]

## Acceptance criteria
- **AK-1** Actions and their edits on unsaved text: a quick fix offered only for the editor's text, and
  its edit inserting the missing `;` at the right place; the fix is also offered at the start of the
  error's line (integration test with clangd).
- **AK-2** Order, speed search and disabled rows (unit tests of `intentions_model`); v4: a repeated
  title is listed once (`a_repeated_title_is_listed_once`).
- **AK-3** In a real window (`idecraft ui --selftest`, with clangd): after deleting a `;`, ⌥↩ on the next
  statement lists the fix and Enter applies it in the editor.
- **AK-4** Use by hand: bulb icons, refactorings on a selection, disabled rows, undo.
- **AK-5** The preview text of a plan (changed lines per file, limit) is a unit test; in a real window
  (with clangd) F1 in the popup shows the fix's added `;` before Enter applies it.
- **AK-6 (v5)** In a window, without a language server
  (`crates/workbench/tests/it/jvm_editor_rest.rs::the_list_of_context_actions_closes_the_problems_popup`): on a
  properties file with trailing spaces the resting pointer opens the problem's popup; ⌥↩ opens the list
  and the popup is gone; the pointer resting on the problem again opens nothing while the list is open.

- **AK-7 (v6)** With jdtls (`crates/languages/tests/it/java_quick_fixes.rs`): an unused import, private
  field, private method and local variable are warnings marked unnecessary with jdtls's messages, and
  its removal fixes are offered and applied; at a type error the fix "Change type of 'text' to 'int'"
  is offered at the literal and at the start of its line; through MCP
  (`crates/mcp/tests/it/protocol.rs::java_unused_local_is_removed_through_mcp`) the removal is listed
  and applied.
- **AK-8 (v7)** Unit tests of `java_quick_fixes` (the wrapping replaces the reported expression, at the
  caret and at the line start; none for `Object`, a place jdtls covers or another language; the
  removal of an unused local without jdtls's own; own fixes after the server's quick fixes). With
  jdtls (`crates/languages/tests/it/java_quick_fixes.rs`): at `String text = 5;` "Wrap using
  'String.valueOf()'" is offered once and gives `String text = String.valueOf(5);`; in a method with
  an unresolved type jdtls offers no removal of `unused` and "Remove variable 'unused'" removes its
  line.
- **AK-9 (v8, behaviour 10)** Unit test `insight::tests::the_build_list_of_jdtls_is_told_by_its_ranges`:
  a list whose problems all sit at character 0 with no width is the build's; the refined list, an
  empty list and a mixed list are not. With jdtls, the live tests of AK-7 and AK-8 pass also on a
  machine under load (three runs of the live tests with a build running beside them).
- **AK-10 (v9, behaviour 13)** `QuickActionPopup` is carried out (`docs/research/KEYMAP_COVERAGE.md`
  lists it as mapped); the popup is the one of AK-1.

## Out of scope
The bulb in the gutter, the inline action submenu (→), "Fix all in file", suppressing inspections,
intention settings, a side-by-side diff in the preview.

## Known deviations from the reference
The reference's own intentions and inspections are replaced by the language server's code actions; which
actions exist depends on the server. Disabled actions with their reason are listed instead of hidden, so
it is clear why a refactoring is not available; not compared side by side yet.

**Java quick fixes (v7, OBS-0027 JQ1, JQ1b, JQ2):**
- "Wrap using 'String.valueOf()'" is IDECraft's own fix (behaviour 11), offered only for the five
  primitive types `int`, `long`, `double`, `boolean` and `char` named in jdtls's type mismatch; the
  reference also offers it for other types and adds "Convert number to…". jdtls 1.63 has no such fix.
- The Eclipse compiler skips the flow analysis of a method that contains an error, so jdtls gives an
  unused local variable there no warning and no removal fix until the error is fixed; in the same way
  unused imports are reported only in a file without errors. In a method with an error the local is
  reported by IDECraft's own inspection and removed by its fix (behaviour 12), only for a single
  declarator with a value without side effects; unused imports in a file with errors stay unreported
  by jdtls (the reference reports them).
- The reference names the removal "Remove local variable 'unused'"; jdtls offers "Remove 'unused' and all
  assignments" and "Remove 'unused', keep assignments with side effects".
- **Quick actions (v9):** the reference's popup of quick actions groups actions of its
  floating code toolbar; IDECraft has no such toolbar and shows the context actions.
