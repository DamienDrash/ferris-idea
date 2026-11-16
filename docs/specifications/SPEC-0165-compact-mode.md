# SPEC-0165: Compact mode

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** OSS (intellij-community at `d71d906`: action `ToggleCompactMode`; UI density applies
  every theme key ending in `.compact` over the key without it, `LafManagerImpl.applyDensityOnUpdateUi`;
  main toolbar height 40, compact 32, `CustomWindowHeaderUtil.HEADER_HEIGHT_NORMAL/COMPACT`;
  the `.compact` values of the bundled Islands themes), SPEC-0154, ADR-0008
- **Feature matrix rows:** F-087

## Purpose
More code and tree on small screens, as the reference's View › Appearance › Compact Mode gives.

## Behaviour
1. **Switch:** the action `ToggleCompactMode` (View › Compact Mode, Search Everywhere), the row
   "Compact mode" in Settings › Appearance and `set_ide_settings compact_mode` switch it for every
   window and save it (`appearance.compact_mode`). [OSS]
2. **Metrics:** in compact mode a theme's `.compact` value wins over the plain key: tree rows
   (`Tree.rowHeight` 24 → 22 in the Islands themes), list rows, the tree border (12 → 8 px left
   and right) and the horizontal room of editor tabs (8/8 → 6/4); the main toolbar is 32 px
   instead of 40. Themes without compact values keep their metrics except the toolbar. [OSS]
3. **MCP:** `themes` returns `compact_metrics` beside `metrics`; `ide_settings` shows the
   setting. [ADR-0008]

## Acceptance criteria
- **AK-1** Compact metrics of Islands Dark (`metrics::tests::metrics_follow_jewel_and_the_theme`).
- **AK-2** MCP `themes` (`themes` protocol test).
- **AK-3** Window self-test: the action switches it on (rows, toolbar, saved) and off again.
- **AK-4** Hand test (round 4, no. 91).

## Out of scope
The height of the editor tab bar in compact mode: the themes set only its horizontal insets and
the open sources give no fixed value, so it stays as it is (to be compared with the reference).
