# SPEC-0154: Metrics of trees, tabs and menus

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (the reference draws trees, editor tabs and popup menus in fixed New UI sizes
  that its themes can change; F-087), OSS (Jewel Int UI metrics in
  `platform/jewel/int-ui/int-ui-standalone/…/styling/IntUiLazyTreeStyling.kt`,
  `IntUiTabStyling.kt`, `IntUiMenuStyling.kt`; theme keys `Tree.rowHeight`, `Tree.border`,
  `TabbedPane.tabHeight`, `EditorTabs.tabInsets`, `EditorTabs.underlineArc`,
  `EditorTabs.underlineHeight` in the bundled themes; Apache-2.0 at `9d3eaff`), SPEC-0131,
  ADR-0009
- **Feature matrix rows:** F-087

## Purpose
Trees, tabs and menus have the reference's proportions, so IDECraft looks like it next to it.

## Behaviour
1. **Trees** (Project, Database, Structure, Hierarchy, Run, Debug, Services, Build Tools): rows of
   `Tree.rowHeight` (24 px), 23 px per level, 4 px before the content, 12 px left and right of the
   selection (`Tree.border`), 2 px selection corners, 4 px between icon and text. [OSS]
2. **Editor tabs:** the bar is `TabbedPane.tabHeight` (40 px) high. Themes with negative top and
   bottom `EditorTabs.tabInsets` (Islands) draw the selected tab as a pill of the remaining height
   (28 px) with `EditorTabs.underlineArc` corners; the others draw an underline of
   `EditorTabs.underlineHeight` at the bottom of a full-height tab. Side padding comes from the
   insets, else 8 px; 4 px between icon and label, 8 px before the close button. The split editor's
   tabs follow the same rules. [OSS]
3. **Popup menus** (context actions ⌥↩, choice popups): 8 px corners, 7 px above and below the
   items, items 24 px high (12 px side and 4 px vertical padding, at least 20 px), 6 px left and
   right of the selection, 4 px selection corners. The menu bar is the system's. [OSS]
4. **MCP:** `themes` returns the chosen theme's `metrics` (tree row height, indent, margins; tab
   bar and tab height, `pill` or `underline`, padding, corners; menu item height and paddings).
   [ADR-0008]

## Acceptance criteria
- **AK-1** Indentation and menu item height; Islands Dark has pill tabs of 28 px in a 40 px bar,
  12 px tree margins; Dark has underlined tabs of 40 px with a 4 px underline
  (`metrics::tests`); the theme's insets are read (`theme::tests`, `palette::tests`).
- **AK-2** MCP: `metrics` for Islands Dark and Light (`themes_tool`).
- **AK-3** Window self-test runs unchanged with the new metrics (356/356).
- **AK-4** Hand test (round 4, no. 80): project tree, editor tabs and ⌥↩ menu next to the
  reference in Islands Dark and Light.

## Out of scope
Compact mode (`*.compact` keys), tool window tabs and stripes, the native menu bar, list popups
such as completion and Search Everywhere.

## Known deviations from the reference
None known; not yet compared side by side.
