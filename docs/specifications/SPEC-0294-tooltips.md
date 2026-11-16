# SPEC-0294: Tooltips in the application window

- **Version:** 5 · **Date:** 2026-11-14 (v5: the tooltips of the caret line's gutter icons from the keyboard, behaviour 12, AK-12; eighth package of docs/research/KEYMAP_COVERAGE.md; v4: no tooltip next to a menu a tool window opens itself, a press on the control hides its tooltip — window check, the "More" tooltip beside the Run window's ⋮ menu; v3: no tooltip or problem card over any popup; v2: no tooltip over an open menu, popup or change bar, behaviour 11)
- **Sources:** GEN (desktop applications explain icon-only controls with a small text that appears
  when the pointer rests on the control for a moment and goes away when it leaves; the text names the
  command and, when one is bound, its keyboard shortcut), DOC (the reference's help describes that
  hovering a toolbar button, a tool window stripe button or a gutter icon shows its name or what it
  marks), OSS (theme keys `ToolTip.background`, `ToolTip.borderColor`, `ToolTip.foreground`,
  `ToolTip.shortcutForeground` of the open New UI themes; the action ids and shortcuts of the ported
  keymaps `$default.xml` and `Mac OS X 10.5+.xml`, `JetBrains/intellij-community`), the project owner
  2026-11-05 (side-by-side comparison must catch tooltips and hover effects; the reference shows a
  tooltip on every icon-only control, with the shortcut of the active keymap, e.g. Rerun with ⌘R),
  SPEC-0028 (keymaps), SPEC-0105 (keymap in use), SPEC-0275 v5 behaviour 14a (shortcut labels of the
  keymap in use), SPEC-0061 (window layout), SPEC-0167 (stripes), SPEC-0063, SPEC-0086, SPEC-0087,
  SPEC-0122, SPEC-0127, SPEC-0194, SPEC-0293 (the controls named below), ADR-0008, ADR-0009,
  ADR-0018, IMP-0286
- **Feature matrix rows:** F-087

## Purpose
Every control that shows only an icon or a symbol says what it does when the pointer rests on it,
with the shortcut that does the same in the keymap the user chose, so the window can be learned
without the menus and compared control by control with the reference.

## Behaviour
1. **Which controls.** These controls have a tooltip [GEN, DOC, project owner]:
   - every icon-only or symbol-only button of a tool window toolbar: Debug, Run, Find (Find Usages
     and Find in Files), Git log, Commit, Database, Services (result tabs and pager), Terminal (tabs
     and its find bar), Problems; the Structure popup has no icon-only control;
   - the tool window stripe buttons;
   - the main toolbar: project widget, branch widget, run configuration chooser, Run and Stop;
   - editor tabs: the close button, and the tab's name (its path, which also shows a name that is cut
     short);
   - the switches and arrows of the editor's find bar;
   - gutter icons: ▶ run markers, breakpoints, folding marks;
   - status bar widgets: caret position, line separator, encoding, indentation, column selection
     mode, Python interpreter, plugin widgets (their own tooltip, SPEC-0210 behaviour 4);
   - the close, hide and reload buttons of the other tool windows and views (Debug's hide and
     watch removal, Build Tools, Endpoints, Hierarchy, the ER diagram, plugin panels) and the problem
     indicator in the editor's corner;
   - icon-only buttons of dialogs and settings pages (none exist at this version; a new one gets a
     tooltip by behaviour 9).
2. **Text of an action control.** A control that runs an action shows the action's name followed by
   its first shortcut in the keymap in use, written as menus write it (SPEC-0275 v5 behaviour 14a:
   `⌘R` on macOS, `Ctrl+F5` in the default keymap); without a shortcut only the name. The text is
   made when the tooltip appears, so it follows a keymap change or a user's own shortcut at once.
   [OSS, GEN]
3. **Stripe buttons** show the tool window's title and the shortcut of its activation action
   (Project ⌘1). [OSS, DOC]
4. **Gutter icons explain themselves** [GEN, DOC]:
   - a run marker: `Run`, the shortcut of `RunClass`, and on a second line that a click offers Run
     and Debug;
   - a breakpoint: `Line breakpoint`, or `Line breakpoint (disabled)`; below it the condition
     (`Condition: <expression>`), the pass count (`Pass count: <n>`) and the log message (`Logs:
     <message>`, `does not suspend`) when set, and that a right click edits it;
   - a folding mark: `Click to fold` or `Click to unfold`.
5. **Status bar widgets** say what they show and what a click does, where a click does something
   (caret position: Go to Line:Column with its shortcut; column selection mode: a click turns it off;
   Python interpreter: a click chooses one). [GEN, DOC]
6. **Timing and dismissal.** A tooltip appears when the pointer has rested on the control for half a
   second (500 ms) and disappears when the pointer leaves the control, a mouse button is pressed, the
   view scrolls or a key is pressed. After a key press it appears again only when the pointer leaves
   and comes back. [GEN]
7. **Placement.** The tooltip lies inside the window and never under the pointer: it starts 20 px
   below the pointer's tip and slightly right of it; where it does not fit below or to the right, it
   goes above or to the left of the pointer, and at the window's edge it is pushed inside. [GEN]
8. **Look.** One shared component for the whole window: a box with the theme's tooltip background,
   border and text colours (`ToolTip.*`, falling back to the popup colours), 4 px corner radius, 8 px
   by 4 px padding, 12 px text, at most 400 px wide, each line on one line (a longer one ends in …,
   detail lines are cut to 120 characters); the shortcut follows the name in the theme's shortcut
   colour; detail lines below in the same muted colour. It follows the
   theme chosen (IDECraft Dark/Light, Islands, Darcula/Light; ADR-0009, ADR-0018). [OSS]
9. **One registry.** The icon-only controls of the toolbars, the main toolbar, the find bar, editor
   tabs and status bar are declared in one table per area with their name, title and action. A
   toolbar button cannot be built without a declared entry (a missing entry is a build error), so
   every icon-only control has a tooltip. Controls whose text depends on the moment (stripe buttons,
   editor tabs, gutter icons, plugin widgets) make it from the tool window, file or breakpoint.
   [GEN]
10. **Readable by agents (ADR-0008, IMP-0286).** The window keeps, per control name, the tooltip the
    control declares, and the tooltip shown at the moment with its text and bounds and where the
    pointer was. The MCP tool `ui_tooltips` (window over HTTP, no right needed, it only reads) returns
    `shown` (`text`, `x`, `y`, `width`, `height`, `pointer`) or null, and `controls` (`name`, `text`),
    optionally only those whose name contains `control`. The window self-test reads the same through
    the inspection it already uses for clicks (`hit_targets`). Without a window (stdio) the tool says
    so. [GEN]
11. **v2 – Not over menus:** while a context, tab, stripe or header menu, a Git popup (branches, VCS operations, push, update, interactive rebase, stashes), any other popup of the workbench (v3: context actions, run marker menu, Go to, Search Everywhere and the rest of the popup registry, SPEC-0275 v4; quick documentation excepted) or the change bar is open, no tooltip is drawn and the card of a problem under the pointer closes (SPEC-0078 v2 behaviour 5), so the tooltip of the control under the pointer (a tab, the branch widget, a run icon) never lies over it. **v4:** the same holds for the menus a tool window or bar opens itself: the Run window's ⋮ menu, the run chooser's row menu, the result toolbar's ⋮, page size, transaction and copy format menus (SPEC-0190 v5), the main toolbar's ⋮ and settings menus and the find bar's menus. A press on a control with a tooltip hides the tooltip at once (behaviour 6), also when the control stops the press for itself; it comes back only when the pointer leaves and returns. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L28; window check 2026-11-10]
12. **v5 – Gutter icon tooltip from the keyboard:** `EditorShowGutterIconTooltip` (⌥⇧6, then T,
    in both bundled keymaps) shows the tooltips of the gutter icons on the caret's line —
    behaviour 4's run marker, breakpoint, folding mark, bookmark, declaration and framework
    marks — below the caret as a hint that closes like the documentation popup, one icon after
    the other in the order they are drawn, title first, then the shortcut in brackets and the
    detail lines. A line without icons says "No gutter icons on this line" in the status bar.
    [OSS: id and key of the bundled keymaps; GEN]

## Acceptance criteria
- **AK-1** Given the macOS keymap, the label of Rerun is `Rerun ⌘R`; given the default keymap,
  `Rerun Ctrl+F5`; an action without a shortcut gives its name only; after the user gives the action
  another shortcut the label shows that one.
- **AK-2** Every declared control in every area has a non-empty title and a unique name in its area;
  every declared action has a shortcut in the macOS or the default keymap.
- **AK-3** Every tool window has a stripe tooltip with its title and the shortcut of its activation
  action.
- **AK-4** The breakpoint tooltip names the condition, the pass count and the log message when set and
  says when the breakpoint is disabled; the run marker and folding mark tooltips are as in behaviour 4.
- **AK-5** A tooltip is placed inside the window and does not contain the pointer, also near the right
  and bottom edges; a key press hides it.
- **AK-6** `ui_tooltips` returns the shown tooltip and declared controls from the window; without a
  window it explains that it needs the window over HTTP; it is listed among the tools.
- **AK-7** Window self-test: hovering the Run button, a stripe button and the debug toolbar's Rerun
  shows their tooltip with the expected text inside the window and away from the pointer; a key press
  hides it.
- **AK-8** By hand: the tooltips next to the reference (text, delay, place, look).
- **AK-9 (v2)** Tooltips are held back while the flag is set (unit test `tooltip::tests::tooltips_are_held_back_while_a_menu_is_open`); by hand: no tooltip over the tab menu and the branch popup.
- **AK-10 (v3)** By hand: with the pointer resting on a problem at the caret, ⌥↩ shows its first rows
  uncovered.
- **AK-11 (v4)** A view's own open menu holds tooltips back for the frame it draws, and the next
  frame without it shows them again (unit test
  `tooltip::tests::tooltips_are_held_back_while_a_menu_is_open`); window self-test
  (`sql_console_steps`): resting on the result toolbar's ⋮ shows "More", a click opens its menu and
  no tooltip is left beside it; in the window: the same for the Run window's ⋮.
- **AK-12 (v5, behaviour 12)** The text of one and of several tips (`tips::tests::
  gutter_tips_read_as_one_text`); in a window of the test harness with a bookmark on the caret's
  line, the hint shows the bookmark's tooltip; on a line without icons the status bar says so
  (`eighth_package_keymap.rs::the_gutter_tooltip_is_shown_from_the_keyboard`).

## Out of scope
Rich tooltips with links or actions (the reference's help tooltips and "Got It" hints), tooltips of
tree rows and table cells, error and warning tooltips on the editor text (SPEC-0070 shows those on
hover in the editor), the delay setting, tooltips in the native macOS menu bar (the system draws them). v5: `EditorFocusGutter` (the keyboard focus in the gutter, moving from icon to icon): the gutter has no focus of its own yet.

## Known deviations from the reference
Texts are IDECraft's own wording: action names come from the main menus or the action table, other
texts are written here. The shown delay is fixed at 500 ms. A key press always hides the tooltip.
Placement follows the toolkit's rule (below right of the pointer, flipped at the window edges) with
a gap below the pointer, so the pointer never covers the text.

**Window check 2026-11-09** (`main` @ 3b3cc1e0, agent run with real input, OBS-0028): the Alt+Enter list is no longer covered by a card when the pointer rests near the caret.
