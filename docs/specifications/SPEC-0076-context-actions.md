# SPEC-0076: Context actions from the editor (Alt+Enter)

- **Version:** 13 · **Date:** 2026-11-19 (v13: a row's submenu goes left of the list when the window's right edge would cut it off, behaviour 19, AK-16; window check after #1046, OBS-0100 SM1); 2026-11-19 (v12 (v10 was set aside for it): the list opens at once with Ferris IDEA's own offers, the language server's actions join it, behaviour 18, AK-15; window check 2026-11-18, F-081/F-033: the menu came after 3–5 s); 2026-11-18 (v11: Run, Debug and Profile at a run marker, behaviour 14; a submenu per
  intention with Disable and the intention settings, behaviour 15; the preview as a before/after pane
  beside the list, behaviour 16; MCP `context_actions` and `apply_context_action`, behaviour 17;
  OBS-0096 AE1 and AE2); 2026-11-16 (v9: Quick Actions open the context actions, behaviour 13, AK-10; eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-13 (v2: problems on the caret's line count; hand test; v3: preview
  of the selected action, one undo step for the whole action; v4: a title listed once, OBS-0027; v5: the
  list closes the problem's popup, OBS-0027 L12; v6: jdtls's refined diagnostics as context, unused code
  as a weak warning with its removal fix, OBS-0027 JQ1 and JQ2; v7: Ferris IDEA's own Wrap using
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
  v11: OBS-0096 (AE1: at a class with `main` the reference lists Run, Debug, Run with Coverage, Profile
  and Modify Run Configuration above the intentions; AE2: every intention has a submenu and F1
  toggles a preview, named in the list's footer), SPEC-0122 behaviour 1 and SPEC-0097 v3
  behaviour 8 (the run markers and their menu), SPEC-0045 v5 behaviour 13 (test markers), SPEC-0120
  and SPEC-0136 (profiling a configuration), SPEC-0125 v2 behaviour 8 (the submenu of an own
  finding), SPEC-0129 (the user's settings file), ADR-0008
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
    "Remove 'never', keep assignments with side effects" and "Remove unused import". Ferris IDEA keeps
    jdtls's default compiler settings (these problems are warnings) and does not override a project's
    own settings. [OBS-0027 JQ1, JQ2; STD]
11. **Wrap using 'String.valueOf()' (v7):** where jdtls reports "Type mismatch: cannot convert from
    `int` to String" (likewise `long`, `double`, `boolean`, `char`) and offers no wrapping fix itself,
    ⌥↩ and `code_actions` add Ferris IDEA's own quick fix "Wrap using 'String.valueOf()'" after jdtls's
    quick fixes. It replaces exactly the reported expression (the problem's refined range, behaviour
    10) by `String.valueOf(<expression>)` and is applied by Ferris IDEA, also through `apply_code_action`.
    An expression over several lines, other types (the reference converts objects with `toString()`
    in other ways) and any place where jdtls offers its own wrapping get none. [OBS-0027 JQ2; GEN]
12. **Unused local in a method with an error (v7):** the Eclipse compiler skips the unused-code analysis
    of a method that contains an error, so jdtls offers no removal there; Ferris IDEA's own Java
    inspection (SPEC-0125, `java.unused-local`) still finds the local. Where jdtls offers no removal
    ("Remove '<name>' …") at an unused local that the inspection can remove safely (one declarator, a
    value without side effects), ⌥↩ and `code_actions` list the inspection's fix "Remove variable
    '<name>'" as a quick fix, applied by Ferris IDEA. In the window the inspection's row and this action
    share the title and are listed once (behaviour v4). [OBS-0027 JQ1b; GEN]
13. **Quick actions (v9):** `QuickActionPopup` (⌥⌘↩ in the bundled macOS keymap) opens the
    context actions at the caret, as `ShowIntentionActions` does (behaviour 1). [GEN]
14. **Run, Debug and Profile (v11):** when the caret's line holds a run marker (SPEC-0122 behaviour 1:
    a top-level class with `main` and that method, a test class and a test method; SPEC-0097 v3
    behaviour 8: the `main` of a Go program or a Rust binary; SPEC-0045: the tests of pytest, Go,
    Rust, Vitest and Jest), the list offers that marker's targets, the same the ▶ menu offers:
    "Run '<name>'"; "Debug '<name>'" for a `main` (and a Jest test, as the ▶ menu); "Profile
    '<name>'" for a `main` whose command the CPU profiler can run (SPEC-0120, SPEC-0136: Java,
    Python, native programs). `<name>` is the configuration's name in the ▶ menu and the run widget.
    The rows carry the Run, Debug and Profiler icons and stand after the quick fixes and before the
    other actions. Enter does what the ▶ menu's entry does: Run and Debug start the configuration as
    a temporary one (SPEC-0097 behaviour 4), Profile selects it and profiles it (SPEC-0120 behaviour
    5). The preview of a run row shows the command and its working folder. Run rows count as own
    entries: with them at hand the list waits at most 3 s for a slow language server, and a server
    that is missing or fails leaves them alone in the list. A remote project's markers are decided on
    its host and offer no rows here; scripts of `package.json` and requests of `.http` files keep
    their own ▶ only. [OBS-0096 AE1; GEN]
15. **Options of an intention (v11):** every action of the language server that is not a quick fix
    (an intention: yellow bulb) has a submenu, opened with → or a click on its "›", with
    "Disable '<title>'" and "Edit intention settings". Disable keeps the title in the user's list of
    disabled intentions (`[intentions] disabled` in the settings file of SPEC-0129, never in a
    project); from then on ⌥↩ leaves that intention out, and the status bar says how to turn it on
    again. Edit intention settings opens Settings › Editor › General, which lists the disabled
    intentions under "Disabled Intentions"; Enter, Space or a click turns one on again. Own findings
    keep their submenu of SPEC-0125 v2 behaviour 8, with "Fix all '<name>' problems in file" where
    Ferris IDEA can fix every finding of the kind in the file at once. Quick fixes of the language
    server and run rows have no submenu. [OBS-0096 AE2; GEN]
16. **Preview beside the list (v11):** the preview of behaviour 7 stands as a pane to the right of
    the list instead of below it: per file its path, then per changed block the lines before (left,
    red) and after (right, green), side by side; a reason that the action cannot be planned, or
    "Planning…", in its place. It is computed without applying the action, as in behaviour 7, and
    follows the selection. The list's footer names the key: "<key> toggles preview", with
    " · → options" when the selected row has a submenu. [OBS-0096 AE2; GEN]
17. **MCP (v11):** `context_actions` lists what ⌥↩ lists at a place of a saved file: the run rows
    (title, `run`/`debug`/`profile`, the configuration), the language server's actions with the
    labels of their submenu (`options`), Ferris IDEA's own fixes of SPEC-0125 at the place with theirs,
    and the disabled intentions the server offers there (`hidden`). `apply_context_action` takes a
    title and optionally an option label; `dry_run` (default true) returns the preview as before/
    after blocks per file (behaviour 16) or the configuration a run row would start. With
    `dry_run=false`: an action is applied as by `apply_code_action` (right `write`); a run row runs
    (`run`) or profiles (`run`) its configuration once and returns the outcome, a debug row is
    refused with the hint to `debug_start`; "Disable '<title>'" and "Enable '<title>'" (for a
    hidden one) change the user's list (right `settings`). [ADR-0008]
18. **At once, then growing (v12):** when Ferris IDEA has offers of its own at the caret (its
    quick fixes such as spelling and grammar, a package to install, rows with options), the list
    opens with them at once, without waiting for the language server; the reference shows its
    menu without a noticeable delay. The server is asked at the same time, as in behaviour 1, and
    its actions join the open list when they come: the order rules of behaviour 2 hold, a title
    already listed is not repeated, the typed filter and the selected row stay. A server that
    fails, is missing or answers after the list was closed changes nothing. Without own offers
    the status bar says "Context actions…" until the server's list or a message takes its place,
    as before. The MCP tool `code_actions` is unchanged: it answers once, with the server's list.
    The replacements of a typo are looked for once per word and kept: the editor's own
    inspection of the open file has found them before ⌥↩, so the list does not search the
    dictionary again. Before v12 the list waited for the server (up to 3 s with own offers, then
    for its diagnostics as context) and searched each typo's replacements twice, so a Markdown
    file with a typo opened its menu after 3–5 s.
    [GEN; window check 2026-11-18]
19. **Submenu at the right edge (v13):** the submenu of a row (behaviour 15) stands right of the
    list, level with its row, whenever it fits between the list and the window's right edge
    (keeping a margin of 8 px). When it does not fit there and the left side of the list has
    more room, it stands left of the list instead, level with its row; keys stay the same (→ opens
    it, ← or Escape closes it). The list itself does not move. The same rule as the side of an
    open submenu of Ferris IDEA's context menus. Before v13 a list opened near the right edge had
    its submenu cut off by the window edge. [GEN; window check after #1046, OBS-0100 SM1]

## Acceptance criteria
- **AK-1** Actions and their edits on unsaved text: a quick fix offered only for the editor's text, and
  its edit inserting the missing `;` at the right place; the fix is also offered at the start of the
  error's line (integration test with clangd).
- **AK-2** Order, speed search and disabled rows (unit tests of `intentions_model`); v4: a repeated
  title is listed once (`a_repeated_title_is_listed_once`).
- **AK-3** In a real window (`ferris-idea ui --selftest`, with clangd): after deleting a `;`, ⌥↩ on the next
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
- **AK-11 (v11, behaviour 14)** Unit tests of `context_actions` (languages): at the class line and at the
  `main` line of a Java class Run, Debug and, when profilable, Profile with the ▶ menu's
  configuration; on another line none; a test method only Run. Unit test of `intentions_model`: the
  run rows stand after the quick fixes and before the other actions. In a window (no language
  server): ⌥↩ on the class line of a Java class with `main` lists "Run 'Inventory'", "Debug
  'Inventory'" and "Profile 'Inventory'"; its preview names the command.
- **AK-12 (v11, behaviour 15)** Unit tests: an intention gets "Disable '<title>'" and "Edit intention
  settings", a quick fix and a run row none; the settings keep the disabled titles and round-trip;
  a disabled intention is left out of the list. In a window: the submenu of an intention row lists
  both options; Disable writes the test's own settings file and the intention is gone at the next
  ⌥↩; the General page lists it and Enter turns it on again.
- **AK-13 (v11, behaviour 16)** Unit test: the preview's lines become before/after blocks per file.
  In a window: F1 in the list shows the pane beside the list with the before and after lines of an
  own fix; the footer names the key.
- **AK-14 (v11, behaviour 17)** Through MCP (`crates/mcp/tests/it/protocol.rs`): `context_actions` at
  the class line of a Java class with `main` lists the run rows; `apply_context_action` previews a
  run row's configuration, previews an own fix as before/after blocks, and Disable/Enable change the
  isolated settings file (`FERRIS_IDEA_CONFIG_DIR`).

- **AK-15 (v12, behaviour 18)** Without a language server, Ferris IDEA's own offers for a typo in a
  README the editor has inspected become the first list in under 100 ms (also in a debug build);
  a typo's replacements are searched once (`grazie::tests::suggestions_are_kept_per_word`); installs, own fixes and rows with options keep
  their order; the server's later actions join without a repeated title
  (`intentions_model::tests::own_offers_open_at_once_and_the_server_adds_to_them`). In a window
  next to the reference: the menu of the README of the window check opens without a noticeable
  delay (window check).
- **AK-16 (v13, behaviour 19)** Unit test of the pure placement rule
  (`popup_place::tests::submenu_flips_left_near_the_right_edge`): right of the list while it
  fits, left of it near the right edge, right again when the left side has even less room. In a
  window (macOS, window check): ⌥↩ on a line near the right edge, → on a row with options: the
  submenu stands left of the list, uncut.

## Out of scope
The bulb in the gutter, suppressing inspections from a server's action (own findings: SPEC-0125),
a tree of all intentions in the settings (v11: only the disabled ones are listed).

## Known deviations from the reference
The reference's own intentions and inspections are replaced by the language server's code actions; which
actions exist depends on the server. Disabled actions with their reason are listed instead of hidden, so
it is clear why a refactoring is not available; not compared side by side yet.

**Java quick fixes (v7, OBS-0027 JQ1, JQ1b, JQ2):**
- "Wrap using 'String.valueOf()'" is Ferris IDEA's own fix (behaviour 11), offered only for the five
  primitive types `int`, `long`, `double`, `boolean` and `char` named in jdtls's type mismatch; the
  reference also offers it for other types and adds "Convert number to…". jdtls 1.63 has no such fix.
- The Eclipse compiler skips the flow analysis of a method that contains an error, so jdtls gives an
  unused local variable there no warning and no removal fix until the error is fixed; in the same way
  unused imports are reported only in a file without errors. In a method with an error the local is
  reported by Ferris IDEA's own inspection and removed by its fix (behaviour 12), only for a single
  declarator with a value without side effects; unused imports in a file with errors stay unreported
  by jdtls (the reference reports them).
- The reference names the removal "Remove local variable 'unused'"; jdtls offers "Remove 'unused' and all
  assignments" and "Remove 'unused', keep assignments with side effects".
- **Quick actions (v9):** the reference's popup of quick actions groups actions of its
  floating code toolbar; Ferris IDEA has no such toolbar and shows the context actions.

**Run rows, submenus and preview (v11, OBS-0096 AE1 and AE2):**
- Run with Coverage is not offered: Ferris IDEA has no coverage runner yet. Modify Run Configuration is
  not offered either; the run widget's Edit Configurations does it.
- The rows are named after the configuration as in the ▶ menu, "Run 'Inventory'", where the reference
  names the method, "Run 'Inventory.main()'".
- "Fix all … in file" exists for Ferris IDEA's own inspections only; the language server's quick fixes
  have no submenu, because a server answers per place and offers no batch for most of its fixes.
- An intention is disabled by its exact title, the only name a language server gives it; a title that
  names a symbol ("Generate toString() for 'Inventory'") is therefore disabled for that symbol only.
- The settings list only the disabled intentions, since the set of intentions depends on the language
  server and is not known in advance.
- Through MCP a debug row is not started by `apply_context_action`: a debug session has its own tools
  (`debug_start` with the configuration it returns).
