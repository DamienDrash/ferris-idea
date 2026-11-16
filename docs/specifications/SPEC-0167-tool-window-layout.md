# SPEC-0167: Tool window layout: stripes, show and hide, docking side

- **Version:** 7 · **Date:** 2026-11-14 (v7: `ShowContent` lists the tabs of the focused tool window, behaviour 19, AK-9; sixth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-13 (v6: behaviour 17 is the default layout of SPEC-0333; v5: Jump to Last Tool Window, Restore Default Layout and Close Active Tab, behaviours 16–18; docs/research/KEYMAP_COVERAGE.md; v4 2026-11-06; v2: bottom windows share the height; v3: activation
  focuses the window, Escape returns to the editor; v4: one window per placement at the bottom too,
  the reference's default sides, behaviours 13–15; SPEC-0061 v10)
- **Sources:** GEN (IDEs dock tool windows on the left, right and bottom of the editor; a stripe at
  the window edge has a button per tool window that shows and hides it; a tool window can be moved to
  another side from the button's context menu; the layout is kept per project), OSS (action ids and
  shortcuts from the bundled keymaps: `HideActiveWindow` ⇧Esc, `HideAllWindows` ⌃⇧F12 / ⇧⌘F12,
  `ViewToolButtons` without shortcut; default sides of the tool windows from the open plugin
  registrations: Project and Commit left; Version Control, Problems, Run, Debug, Services and Terminal
  bottom; Database, Maven and Gradle right), OBS-0007 (New UI: the left stripe shows the left-docked
  windows at its top and the bottom-docked windows at its foot, the right stripe the right-docked
  windows), SPEC-0061 (stripe look), ADR-0008 (MCP)
- **Feature matrix rows:** F-087

## Purpose
Every tool window has a button on a stripe and a side it docks to; the user shows and hides tool
windows from the stripes and the keyboard, moves them to another side and hides the stripes, and the
layout survives a restart.

## Behaviour
1. **Layout:** each tool window of the layout (Project, Commit, Git, Problems, Run, Debug, Services,
   Terminal, Database, Build) has a side — left, bottom or right — and an order on that side. The
   default layout puts them on the sides of the reference, in the order listed; the bottom windows'
   default order and the foot group's direction follow SPEC-0061 v4 behaviour 13. [OSS]
2. **Stripes:** the left stripe shows the buttons of the left windows at its top and of the bottom
   windows at its foot; the right stripe shows the right windows. A button is highlighted while its
   window is shown and toggles it on click. Windows that need a running session (Run, Debug,
   Services) get a button once they exist (SPEC-0061 behaviour 3). [OBS, GEN]
3. **Placement:** a shown left window sits between the left stripe and the editor, a right window
   between the editor and the right stripe (300 px wide, Database and Build 400 px), a bottom window
   below the editor (280 px high). The same window looks the same on every side. [GEN]
4. **One per side:** on the left and on the right one window is shown at a time: showing a window
   there hides the other shown window of that side. Bottom windows stack. [GEN]
5. **Move to:** a right click on a stripe button opens a menu with "Move to Left", "Move to Bottom",
   "Move to Right" (without the current side) and "Hide" while the window is shown. Moving puts the
   window last on the new side and keeps it shown when it was. [GEN]
6. **Hide all:** `HideAllWindows` hides every shown tool window; run again before another one is
   shown, it brings back exactly the windows it hid. `HideActiveWindow` keeps its behaviour. [OSS, GEN]
7. **Tool window bars:** `ViewToolButtons` (View › Appearance › Tool Window Bars) hides both stripes;
   tool windows still open from the keyboard and the menu, and run again it shows the stripes. [OSS,
   GEN]
8. **Kept per project:** sides, order and the stripes setting are saved in
   `.idecraft/tool-windows.json` (machine-local, ignored by Git) after every change and read at the
   start; unknown entries are skipped and missing windows get their default side. [GEN]
9. **MCP `tool_windows`:** lists the tool windows with id, title, side, order, default side and
   activation action, and whether the stripes are shown; with `move` (id and side) or `stripes`
   (true/false) it changes the saved layout, which a window uses at its next start. [ADR-0008]

10. **Bottom windows share the height (v2):** the editor column ends at the top of the status bar
   (SPEC-0061 v2 behaviour 8). Each stacked bottom window — also the panels outside the layout
   (Hierarchy, Find, …) — is 280 px high while that fits; otherwise they share what is left after
   the editor keeps 120 px equally, and each keeps at least 60 px while the column has room for it.
   When even that does not fit, editor and bottom windows shrink so their sum, with the island gaps,
   never exceeds the column. [GEN]
11. **Activation focuses (v3):** a tool window's activation action (on macOS ⌘1 Project, ⌘0 Commit,
   ⌘4 Run, ⌘5 Debug, ⌘6 Problems, ⌘8 Services, ⌘9 Git, ⌥F12 Terminal) shows a hidden window and
   moves the keyboard focus into it; a shown window without the focus gets the focus; only a window
   that has the focus is hidden, and the focus goes back to the editor. The editor's popups
   (completion list, parameter info, documentation) close when a tool window is activated and take
   keys only while their editor has the focus, so Up, Down and Enter reach the window. [GEN]
12. **Escape (v3):** Escape in a tool window that does not use it itself (`FocusEditor`, bound to
   Escape in the bundled keymaps) moves the focus back to the editor; the terminal keeps Escape for
   the shell, and popups and fields close or clear first. [OSS, GEN]

13. **One window per placement (v4, changes 4 and 10):** at the bottom, too, showing a window hides
   the one shown in the same placement (Bottom Left or Bottom Right), whichever way it is shown; two
   bottom windows at once only side by side as Bottom Left and Bottom Right, or with ⌥-click
   (SPEC-0061 v10 behaviours 49 and 50). Behaviour 10 thus lays out at most one window per bottom
   part. [OBS-0020 item 2]
14. **Default sides (v4, changes 1):** Project and Commit left, Structure in the left side's second
   half; Git, Problems, Terminal, Services, Run, Debug, Find and Find in Files at the bottom; AI
   Chat, Database and Build on the right; Plugins, Endpoints, Spring, Python Packages, Containers,
   Profiler and Remote Host off the sidebar (SPEC-0061 v10 behaviours 51 and 52). Layouts never
   arranged by the user move to these defaults, others stay (SPEC-0061 v10 behaviour 54).
   [OBS-0020 item 6, OSS]
15. **MCP (v4):** `tool_windows` reports `one_per_placement`, per window `alt_click`, `on_sidebar`
   and `default_placement`, and lays out `shown` with one window per placement (SPEC-0061 v10
   behaviour 56). [ADR-0008]
16. **Jump to the last tool window (v5):** `JumpToLastWindow` (F12) moves the keyboard focus into the tool window that had it last, showing it again when it was hidden meanwhile. While a tool window has the focus it is that window itself, so nothing changes; when no tool window had the focus since the project was opened, the action says so in the status line. [OSS: id and shortcut of the bundled keymaps; GEN]
17. **Restore the default layout (v5):** `RestoreDefaultLayout` (⇧F12) puts every tool window back to its default side, half and order (behaviours 1 and 14), drops the sizes set by dragging or stretching, the view modes, the removed stripe buttons and the saved Float and Window frames, shows the stripes, and saves the layout. Which windows are shown does not change, except that a window displaced by another one of the same placement hides (behaviour 13). MCP `tool_windows` does the same to the saved layout with `restore_default=true`. [OSS: id and shortcut; GEN; ADR-0008]
   **v6:** with the named layouts of SPEC-0333 the action is titled Restore Current Layout and restores the selected layout: this default arrangement while the default layout is selected (SPEC-0333 behaviours 7 and 8), which it is until the user saves or selects another one. `RestoreFactoryDefaultLayout` always gives this default arrangement. `tool_windows` with `restore_default` is unchanged. [OSS]
18. **Close the active tab (v5):** `CloseActiveTab` (⌃⇧F4) closes the selected tab of the focused tool window where that window has tabs of its own: in Terminal the tab with its shells (SPEC-0205 behaviour 4), in Run the tab as its close button does (SPEC-0087). In every other tool window, and without a focused tool window, it does nothing and leaves the key to other actions. `ShowContent` (the list of a tool window's tabs) is not carried out: no tool window has such a list. [OSS: id and shortcut; GEN]
19. **List of tabs (v7):** `ShowContent` (Show List of Tabs, ⌃⇧↓ in the bundled macOS keymap,
    Alt+Down in the default one) lists the tabs of the focused tool
    window — the sessions of Terminal, the runs of Run — in a popup titled `Tabs of <tool
    window>`, the selected tab first selected; ↑/↓ and Enter choose a tab, which becomes the
    selected one, Escape closes the list, and the focus is back where it was. With a single
    tab, in a tool window without tabs and outside a tool window the action does nothing and
    its key goes on to the next action. [OSS: id, title, keys and "only with more than one
    tab" from `ShowContentAction.java`, logic template; GEN for the list]

## Acceptance criteria
- **AK-1** Default sides, stripe groups, move to another side (order, current side excluded from the
  menu), one window per side, hide all and restore, saving and reading with unknown and missing
  entries (unit tests `tool_window_layout::tests`).
- **AK-2** MCP: `tool_windows` lists the default layout, `move` puts Terminal on the right, `stripes`
  hides the stripes, and a second call shows the saved layout (protocol test).
- **AK-3** Window self-test: Terminal moved to the right shows right of the editor and its button sits
  on the right stripe; showing Database there hides Terminal; hide all hides the shown windows and run
  again restores them; Tool Window Bars hides and shows the stripes; Terminal moved back to the bottom.
- **AK-5 (v2)** Heights: 280 px while they fit, shared above the editor's 120 px, never more than the
  column (unit test `tool_windows::tests::bottom_heights_prefer_280_then_share`); the frame and
  window checks of SPEC-0061 AK-5.
- **AK-6 (v3)** Activation and Escape: unit tests
  `tool_windows::tests::activation_focuses_before_it_hides` (decision and macOS shortcuts) and
  `completion_model::tests::the_list_takes_keys_only_while_its_editor_has_the_focus`; hand test 7
  of round 4 again (⌘6, ↓, Enter, Escape).
- **AK-7 (v4)** One window per placement and the default sides: unit tests
  `tool_windows::tests::one_window_per_placement_replaces`,
  `tool_windows::tests::default_sides_and_stripe_groups`,
  `tool_windows::tests::untouched_layouts_are_migrated`; window self-test
  `tool_window_replace_steps` (open).
- **AK-4** Hand test (round 4): move tool windows between the sides with the context menu, hide all,
  hide the stripes, restart, next to the reference.
- **AK-8 (v5)** The last focused tool window is remembered across losing the focus and forgotten never (unit test `keymap_actions::tests::last_tool_window_is_the_one_focused_last`); the default layout comes back with stripes, sides, sizes, modes and removed buttons (`tool_windows::tests::restore_default_puts_everything_back`); MCP `tool_windows` with `restore_default` (protocol test `tool_windows_restore_default`). In the window (open): F12 after Escape returns to the tool window; ⌃⇧F4 closes a terminal tab.
- **AK-9 (v7, behaviour 19)** In a window of the test harness: with two Terminal tabs and the
  Terminal in focus the action opens the list with both titles and the selected tab marked,
  Enter on the other row selects that tab; with one tab, and with the editor in focus, the
  action is none (`sixth_package_keymap.rs::show_content_lists_the_tabs_of_the_terminal`). The
  Run tool window's list is not exercised by a test (it needs running programs). On screen:
  not checked.

## Out of scope
Dragging buttons between stripes, split sides (top and bottom half of a side, "Left Bottom", "Right
Bottom"), floating, windowed and undocked modes, resizing by dragging the edge, maximising
(`MaximizeToolWindow`), tool windows outside the list (Hierarchy, Find, Endpoints, Spring, AI Chat,
Python Packages, Containers, Profiler, Remote Host keep the bottom), the "More tool windows" button.

## Known deviations from the reference
Bottom windows stack instead of replacing each other, so two outputs can be seen at once (as before;
v4: they replace each other as in the reference, and ⌥-click shows a window in the other half of its
side without moving it, IMP-0311).
The layout is a small JSON file in `.idecraft/` instead of the reference's workspace file. The MCP
tool changes the saved layout, not a running window. Not compared side by side yet.
