# SPEC-0166: Inspections from plugins

- **Version:** 2 · **Date:** 2026-11-15 (v2: AK-5)
- **Sources:** GEN (IDE plugins contribute inspections with quick fixes), ADR-0016, SPEC-0104,
  SPEC-0156, SPEC-0098, ADR-0008
- **Feature matrix rows:** F-080

## Purpose
The first extension point beyond actions: a plugin checks files and its findings show like
Ferris IDEA's own, with quick fixes, without running anything outside its sandbox.

## Behaviour
1. **ABI:** an action of kind `inspect` (optional `files`: extensions it checks; empty for all)
   gets `{path, language, text}` as JSON and answers `{findings: [{start, end, message, level,
   fix}]}` with character offsets, `level` `error`, `warning` (default) or `weak`, and an optional
   `fix` `{title, edits}`. It runs with a fifth of an action's instruction budget; the first 500
   findings count; a finding or fix edit outside the text, or overlapping fix edits, make the run
   fail with that reason. Inspections are not offered as commands. [ADR-0016]
2. **Editor:** 600 ms after typing stops, and when a file opens, every enabled plugin's matching
   inspection runs in the background; its findings are shown as `<plugin>: <message>` with their
   level while the text is the one checked. A failing plugin is named in the status bar. [GEN]
3. **Quick fixes:** ⌥↩ on a finding offers its fix beside Ferris IDEA's own; choosing it applies the
   edits as one undo step. [GEN]
4. **MCP:** `plugin_inspections` runs them on a workspace file and returns the findings with line
   and column, and the plugins' errors; `plugins` shows the action kind. [ADR-0008]

## Acceptance criteria
- **AK-1** `inspect` actions: findings checked, budget, not a command
  (`runtime::inspect_actions_report_checked_findings`).
- **AK-2** MCP (`plugin_inspections_tool`).
- **AK-3** Window self-test: an installed plugin's finding appears after the pause, ⌥↩ offers its
  fix and Enter applies it.
- **AK-4** Hand test (round 4, no. 92).
- **AK-5 (v2)** Window test with keys (`crates/workbench/tests/it/f080_window.rs`:
  `hand_test_92_a_plugin_inspection_and_its_fix`): "TODO write" shows "Todo: TODO left" as a
  warning, ⌥↩ lists "Remove TODO", typing narrows the list to it and Enter applies the fix.

## Out of scope
Extension points for completion, tool windows and settings pages; a marketplace.
