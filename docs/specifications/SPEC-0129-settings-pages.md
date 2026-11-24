# SPEC-0129: Editor, Font, Build Tools, Languages and Terminal settings

- **Version:** 8 · **Date:** 2026-11-16 (v8: the pages' fields take Select All and the clipboard
  actions also from the menu bar, a page taller than its area rolls, behaviour 11, OBS-0062 S6,
  S7; v7: the Font page with a list of families, fields for size and line spacing and a preview of one's own text, behaviour 10, OBS-0062 S4; v6: the Font page's controls in the semantic tree, behaviour 9; AK-9, the steps of hand tests 56 and 76 that needed a real window, in a window test; v5: the outline with Appearance & Behavior and Advanced Settings, SPEC-0337; v4: the font size by action, behaviour 8, AK-6 to AK-8;
  v3: a value of the wrong kind costs its setting alone,
  every setting survives a restart; v2: Enter and Escape in text fields; hand test 56)
- **Sources:** GEN (IDE settings dialogs have, besides Appearance and Keymap, the pages Editor ›
  General (what saving does), Editor › Font (size, line spacing), Build, Execution, Deployment ›
  Build Tools (JDK, Maven, Gradle), Languages & Frameworks and Tools › Terminal (shell path), in a
  tree grouped like that; F-002), SPEC-0105 (Settings dialog), SPEC-0042 (language servers,
  IMP-0019), SPEC-0052 (JDKs), SPEC-0038 (terminal), ADR-0008; v4: OSS — the open platform's
  action, `JetBrains/intellij-community` at the pinned commit `d4018fb` of APACHE_PORTING.md
  (PROVENANCE.md: logic template, no code):
  `platform/platform-impl/src/com/intellij/openapi/editor/actions/ChangeEditorFontSizeAction.java`
  (a step of 1, needs an editor, nothing outside the limits, the global variants change the
  setting), `platform/editor-ui-ex/src/com/intellij/application/options/EditorFontsConstants.java`
  (the largest size, 40), `ResetFontSizeActionBase.java` and `ResetFontSizeAppInitListener.kt`
  beside the action (read for what is left out); the action ids, their texts
  (`platform/platform-resources-en/src/messages/ActionsBundle.properties`) and their shortcuts
  in the ported keymaps (SPEC-0028); SPEC-0319 (declared effects). Nothing of v4 was observed in
  the reference IDE.
- **Feature matrix rows:** F-002

## Purpose
The settings people change most often are in the dialog, take effect at once and stay for the
next start.

## Behaviour
1. **Storage:** the values are kept as TOML in `settings.toml` of the user's settings folder;
   missing or unreadable values fall back to the defaults, values out of range are clamped (font
   size 8–40, line spacing 1.0–2.0). Every change is applied to all windows at once and saved. The
   page list groups the pages as the reference does: Appearance, Keymap, Editor › General and
   Font, Plugins, Build, Execution, Deployment › Build Tools, Languages & Frameworks, Tools ›
   Terminal (v5: with Appearance & Behavior first and Advanced Settings last, SPEC-0337
   behaviour 1); the search also finds pages by their words (e.g. "font size", "jdk", "shell") and by
   the labels of their settings (SPEC-0105 v5 behaviour 3b). [GEN]
   (v3) The fallback is per value: a value of the wrong kind in the file (a text where a number
   belongs, a choice that does not exist, a section that is no table) leaves its own setting at the
   default, and every other value of the file counts. Until v3 one such value made the whole file
   unreadable, every setting fell back to its default, and the next change in the dialog wrote the
   defaults over the file. Values that only make sense together (the indent size, tab size and
   tabs-or-spaces of one language) are taken or left together. The settings of this dialog belong
   to the IDE layer: the one `settings.toml` of the user, the same for every project; the
   inspection profile alone is kept in the project (SPEC-0315). [GEN]
2. **Font:** size (default 13) and line spacing (default 1.2) of the editor font, with −/+ or
   Left/Right on the selected row and a sample in that size; open editors follow at once. [GEN]
3. **General:** "Remove trailing spaces on save (not on the caret line)" and "Ensure every saved
   file ends with a line break", both off by default; Enter, Space or a click switches them. [GEN]
4. Saving a file then removes spaces and tabs at the ends of its lines except the caret's and adds
   the final line break, in the editor as one undo step before writing; line endings (`\r\n`) stay.
5. **Build Tools** lists the JDKs found with the one chosen for the project and why, the project's
   Java problems, and how Maven and Gradle run (the project's wrapper, from `PATH`, or not
   installed, marked when the project needs it). **Languages & Frameworks** lists per language the
   server used or what to install. Both are read when shown. [GEN]
6. **Terminal:** the shell path; empty means `$SHELL`. Enter applies it; new terminals start it.
   Typing in the field stays in the field. (v2) Enter, Escape and Tab reach the dialog before the
   field: Enter applies without adding a line, Escape and Tab leave the field (a second Escape
   closes the dialog); the same holds for the protected branches and AI fields. [GEN]
7. **MCP:** `ide_settings` reads the settings and their file; `set_ide_settings` changes them
   (`dry_run` default true; clamped like the dialog); open windows read the file at their next
   start. [ADR-0008]

8. **Font size by action (v4):** `EditorIncreaseFontSizeGlobal` (⌃⇧. / Alt+Shift+.) and
   `EditorDecreaseFontSizeGlobal` (⌃⇧, / Alt+Shift+,) make the editor font size of behaviour 2 one
   larger or smaller while the window has an editor open; without one they are not carried out.
   The new size is the user's setting: it is applied to all windows and saved as behaviour 1 says,
   an open Settings dialog shows it, and the status bar names it (`Editor font size: 14`). At the
   smallest size (8) and the largest (40) the action changes nothing and the status bar says that
   this is the limit. Find
   Action lists them under View as Increase Font Size in All Editors and Decrease Font Size in
   All Editors; for UI automation they write (the user's settings file, SPEC-0319).
   `editor_action` (SPEC-0057) answers `increase_font_size` and `decrease_font_size` with
   `font_size`, the size the action would set from the saved settings, and `at_limit`; it writes
   nothing — `set_ide_settings` does (behaviour 7). [OSS, `ChangeEditorFontSizeAction.java`,
   `EditorFontsConstants.java`; GEN: the status messages; ADR-0008]

9. **Controls in the semantic tree (v6):** each − and + of Editor › Font is a button named by
    what it changes ("Increase Size", "Decrease Line spacing", test ids `settings-font-more-N` and
    `settings-font-less-N`, N the row), each value a label with the value (`settings-font-value-N`),
    so agents and window tests find them as a person does (SPEC-0319). [GEN, ADR-0008]

10. **Font page (v7, OBS-0062 S4):** the family is chosen from a list of the families the page
    offers (the bundled font and the installed monospaced families, SPEC-0150), the chosen one
    marked; ←/→ on the row still step through it. Size and Line spacing each have a field between
    − and +: Enter applies the number typed (a comma counts as the decimal point; a value out of
    range is clamped to it, a text that is no number is refused with a note and the field shows the
    value in effect again), Escape and Tab leave the field; − and + and the keys keep working and the
    field follows them. Under the switches a preview shows a text in the editors' font, size and
    line spacing; it starts with a sample and can be edited, Escape and Tab leave it; it is not
    saved. The list entries, fields and preview are nodes of the semantic tree. Typography
    settings are not offered (Known deviations). [OBS, GEN]

11. **Fields and tall pages (v8, OBS-0062 S6, S7):** the Size and Line spacing fields, the
    preview, the shell path and the protected branches take Select All, Copy, Cut, Paste, Undo
    and Redo from their keys and from the menu bar's Edit menu, as SPEC-0105 v8 behaviour 10
    says: Select All then typing replaces the field's text, and the editor behind the dialog
    keeps its text and selection. A page taller than its area rolls (SPEC-0105 v8 behaviour 12),
    so the Font page's preview and the last lines of each page stay reachable in a low window;
    the Build Tools and Languages lines roll with their page. [OBS, GEN]

## Acceptance criteria
- **AK-1** Reading, saving and clamping; saving's edits including the caret line and CRLF
  (`ide_settings::tests`); page search (`settings_model::tests`).
- **AK-2** Window self-test (`ide_settings_steps`): Right makes the font 14 in the editors and in the
  file, Left takes it back; both save options switched on; the shell kept; Build Tools and
  Languages list their lines; saving a file strips trailing spaces except on the caret line and adds
  the line break.
- **AK-3** MCP: read, preview with clamping, saved change (`crates/mcp/tests/it/protocol.rs`:
  `ide_settings_tools`).
- **AK-4** Hand test (round 4): each page next to the reference.
- **AK-5 (v3)** A file with wrong values in five sections keeps every other value, a section that is
  no table costs that section alone (`ide_settings::tests::a_wrong_value_keeps_the_other_settings`);
  every setting of the IDE layer, changed from its default, is in the file under its section and is
  read back unchanged, and a setting added later fails the test until it is given a changed value
  (`ide_settings::tests::every_setting_survives_a_restart`).

- **AK-6 (v4)** `ide_settings::tests::font_size_steps_stop_at_the_limits`: 13 → 14 → 13, 40 stays
  40 and 8 stays 8, a size between whole numbers lands on a whole one.
- **AK-7 (v4)** In a window, through the window's key dispatch, with a settings file of its own
  (`crates/workbench/tests/font_size_actions.rs::font_size_keys_change_the_setting`): ⌃⇧. makes
  the size 14 in the editors, in the settings in effect and in the file, and the status bar says
  so; ⌃⇧, takes it back; at 40 the key changes nothing and the status bar names the limit;
  without an editor the action is not carried out. Titles and effects:
  `everywhere::tests::fifth_package_actions_have_titles`,
  `security::tests::fifth_package_actions_declare_their_effects`; `editor_action`:
  `protocol.rs`: `editor_action_fifth_package`.
- **AK-8 (v4)** Hand test, open: the keys on a real keyboard (⌃⇧. and ⌃⇧, arrive as the
  characters the layout types there), next to the reference.

- **AK-9 (v6)** In a window of the test harness with a settings file of its own and a process of
  its own, by clicks on controls found through the semantic tree and by real keys
  (`crates/workbench/tests/settings_window_checks.rs`:
  `font_shell_and_protected_branches_in_the_window`): the + buttons of Editor › Font bring the size
  to 16 and the line spacing to 1.4, and the editor uses them at once and the file keeps them; the
  shell `/bin/bash` set on the Terminal page is what a new terminal tab starts; the saved file read
  again holds every value. This closes the window steps of hand test 56 and the font steps of hand
  test 76 without a screen; a restart of the app itself and the look stay for the comparison
  (`docs/research/observations/OBS-0060.md`).

- **AK-10 (v7)** In a window of the test harness (`crates/workbench/tests/settings_window_checks.rs`:
  `search_revert_defaults_font_and_advanced`): 18 typed into the Size field and Enter set and save
  the size; a click on the second family of the list chooses it; the preview takes a text of its
  own; the +/− buttons and the field stay in step (`font_shell_and_protected_branches_in_the_window`).

- **AK-11 (v8)** In a window of the test harness (`crates/workbench/tests/settings_window_checks.rs`:
  `select_all_and_rolling_lists`): with the Size field focused, Select All from the menu bar
  selects the field's text, "20" typed and Enter set and save the size 20, and the open editor
  keeps its caret without a selection; in a window of 1300 × 460 points the Font page's preview
  is cut at first and in view after the wheel over the page.

## Out of scope
Font family choice, ligatures, the console font, code style, soft wraps, whitespace display, project
JDK choice from the page, Maven/Gradle settings, per-language settings, terminal environment and
start directory.

## Known deviations from the reference
(v7) The reference's Font page also has typography settings (the weight of the regular and the
bold text, which font fills in missing characters). Ferris IDEA draws with the font's own weights and
the system's fallback; a setting that only some installed fonts can honour would look as if it
did nothing, so it is not offered. The preview is a field of its own, not a full editor with
highlighting.
The reference removes trailing spaces of modified lines by default; Ferris IDEA does nothing unless
switched on and then strips the whole file except the caret line. Build Tools and Languages only
show what Ferris IDEA finds. Not compared side by side.

v4: taken from the open platform's sources, not compared with the reference IDE. The open action
does nothing at the limits without saying so; Ferris IDEA names the limit in the status bar. Left
out: `EditorIncreaseFontSize`, `EditorDecreaseFontSize` and `EditorResetFontSize`, which change
the size of one editor until it is closed — Ferris IDEA has one size for all editors and no size of
an editor's own; `EditorResetFontSizeGlobal`, which goes back to the size the open platform noted
when the colour scheme was last changed, a value Ferris IDEA does not keep. None of the four has a
shortcut in the ported keymaps. The open action scales its step with the IDE zoom; Ferris IDEA has
no IDE zoom.
