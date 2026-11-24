# SPEC-0061: Window chrome in the Islands layout

- **Version:** 19 · **Date:** 2026-11-17 (v19: the selected editor tab in the active colours only
  while the keyboard focus is in its editor, else in the theme's inactive colours with their border,
  behaviour 6a, AK-v19, OBS-0079 L4; v18 is taken by an open change); 2026-11-16 (v18: Emoji & Symbols opens the system's character palette, behaviour 68; eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v17: the icon gaps of OBS-0046 — the open Structure icon, a Notifications tool window, the Build button after the first build, Run and Debug grey without anything to run, the menu marker on the Project window's "+", behaviours 63–67; v16: Stop Background Processes… (`StopBackgroundProcesses`) lists the running background tasks of the window that can be stopped and stops the chosen one, behaviour 62, AK-v16; seventh package of docs/research/KEYMAP_COVERAGE.md; v15: two quick clicks on a control of the title bar press the control twice and never zoom the window, behaviour 61 — second window check of 2026-11-14; v14: the project colour of the main toolbar in the Islands themes from the theme, behaviour 28a, OBS-0047 T1; v13: the tool windows shown at quitting come back, behaviour 59; a diff tab in front names its own file in the navigation bar, behaviour 60 — window check of 2026-11-14); 2026-11-13 (v12: full screen from the keyboard, the keymap's `ToggleFullScreen`, behaviour 58); 2026-11-07 (v2: the status bar stays free; v3: the breadcrumbs end at the
  symbol under the caret, behaviour 5a; v4: window structure before the new comparison with OBS-0007 —
  tool window header with content tabs, options and hide, stripe buttons and order, main toolbar
  buttons, status bar widgets, compact islands, the tree nodes "External Libraries" and "Scratches and
  Consoles", behaviours 9–17; v5: the shared header for the other tool windows of the layout, their
  content tabs in it, the more-actions button after the run widget, behaviours 18–20; v6: the
  measured side-by-side comparison OBS-0020 — frame margins, full-width bottom windows, default sizes,
  header pills and line, toolbar rows, Project header buttons, stripe focus highlight, main toolbar
  tint, initials and Settings menu, tree indent and selection, tabs, empty editor, status bar, menus,
  panels outside the layout, resizing, compact values, behaviours 20a–36; v7: the defects F087-1 and
  F087-2 of OBS-0020 — view modes, split placements, "Remove from Sidebar", the "…" button with the
  Structure tool window, the panels outside the layout as tool windows, the traffic light pitch,
  behaviours 37–46; v8: a long status message cuts the breadcrumbs instead of overlapping them; OBS-0022;
  v9: no hover card of the editor stays over an open menu, behaviour 48; OBS-0022 L28; v10: one tool window per placement also at the bottom — replacing instead of stacking —, ⌥-click into the other half of the side (IMP-0311 reworked), the reference's default stripes with the rare windows off the sidebar, migration of untouched layouts, behaviours 49–56; the project owner's question of 2026-11-06)
- **Sources:** OBS-0007 (main window of the reference beside Ferris IDEA), OSS (`ManyIslandsDark.theme.json`:
  `MainWindow`, `Island`, `EditorTabs`, `Tree`, `StatusBar`, `MainToolbar`, `ToolWindow` values; icons
  `toolwindows/project`, `general/vcs`), SPEC-0059 (tree and tabs), SPEC-0060 (themes), ADR-0009; v3:
  OBS-0011 item 3 (the reference's path in the status bar continues past the file to the class the
  caret is in), SPEC-0031 (definitions from the grammars' tags queries); v4: OBS-0007 items 2–4, 6 and 8,
  OBS-0011 item 3 (the tree's extra nodes), OSS (intellij-community at `750ce16`: `JBUI.CurrentTheme`
  values `ToolWindow.Header.height` default 41, `ToolWindow.Header.labelLeftRightInsets` 0,12,0,16,
  `ToolWindow.Header.toolbarLeftRightInsets` 0,12,0,8, `ToolWindow.HeaderTab.leftRightInsets` 0,12,
  `ToolWindow.HeaderTab.underlineHeight` over `DefaultTabs.underlineHeight` 3,
  `ToolWindow.HeaderTab.underlineArc` 4, `StripeToolbar.Button.size` default 40,40,
  `StripeToolbar.Button.iconSize` 20, `StripeToolbar.Button.*StripeIcon.padding` default 5,
  `Button.ToolWindow.arc` 12 / compact 8, `MainToolbar.Button.size` 30,30, `MainToolbar.Button.iconSize`
  20, `MainToolbar.Button.arc` 12, `MainToolbar.Dropdown.borderInsets` 5,10,5,6, `MainToolbar.Dropdown.arc`
  12, `StatusBar.Widget.widgetInsets` 6,8, `StatusBar.Widget.hoverInsets` 3,4,1,4, `StatusBar.topBorderWidth`
  1; `ActionToolbar.DEFAULT_MINIMUM_BUTTON_SIZE` 22; `ToolWindowHeader.kt` (options and hide actions on
  the right of the header); `IdeStatusBarImpl.kt` (10 px left and right of the New UI status bar);
  `defaultToolWindowlayoutProvider.kt` (New UI default stripes: Project, Commit, Structure on the left;
  Version Control, Problems, Terminal, Services at the bottom; Notifications, AI, Database, Gradle, Maven
  on the right; Project's content as a combo); `SquareStripeButton.kt` (20 px stripe icons, 16 px icons
  scaled); `ExternalLibrariesNode.java`, `ScratchTreeStructureProvider.java` (the two extra top-level
  nodes); the bundled Islands themes' values of these keys; New UI icons `general/moreVertical`,
  `general/hide`, `general/settings`, `nodes/library`, `fileTypes/scratches` and the 20 px
  `toolwindows/*@20x20` icons), SPEC-0167 (stripe groups), SPEC-0165 (compact values); v5: OBS-0007
  item 2 (right of the run configuration: run, debug, **a menu button**, search, settings; the
  observation does not record what the menu holds), OSS (`ToolWindowHeader.kt` as in v4: every tool
  window of the platform gets the same header, contents of a tool window become its header tabs),
  SPEC-0087 (Run sessions), SPEC-0194 (Debug session and toolbar), SPEC-0190 (Services result tabs),
  SPEC-0086 (Git log), SPEC-0120 and SPEC-0047 (actions on the selected run configuration), GEN;
  v6: OBS-0020 (measurements of the reference in Islands Dark and Light, normal and compact, items
  1–16), OSS (New UI icons `general/locate`, `general/expandAll`, `general/collapseAll`,
  `general/add` at `750ce16`; the keymaps' `ResizeToolWindow*` and `MaximizeToolWindow`), SPEC-0167
  (layout), SPEC-0059 (empty editor), IMP-0311, IMP-0312; v7: OBS-0020 items 5, 6 and 17 (the
  Options menu's view modes and six placements, "Remove from Sidebar", the stripe's separator,
  Structure and "…", the 20 pt traffic light pitch), OSS (`defaultToolWindowlayoutProvider.kt` at
  `750ce16`: Structure in the left stripe's second group; the keymaps' `ActivateStructureToolWindow`
  and `ActivateFindToolWindow`), GEN (GPUI 0.2.2: `TitlebarOptions::traffic_light_position` moves the
  close button only and keeps the pitch AppKit gives the buttons; `WindowKind::Floating`), SPEC-0079
  (file structure), SPEC-0167 (layout), IMP-0313, IMP-0314; v10: OBS-0020 items 2 and 6 (a second bottom
  window replaces the first; the default stripes), OBS-0007 item 3, OSS (`defaultToolWindowlayoutProvider.kt`
  at `750ce16`: Maven and Gradle on the right), SPEC-0167 v4, IMP-0311, GEN
  v14: OBS-0046 items A1, A2, A4–A6, OSS (intellij-community at `d4018fb`: New UI icons
  `toolwindows/structure`, `toolwindows/notifications` at 16 and 20 px, `general/dropdown`;
  `defaultToolWindowlayoutProvider.kt` as in v4: Notifications on the right), SPEC-0070 (the status
  bar's notes), SPEC-0087 (run widget), SPEC-0122 (what the editor's Run starts), ADR-0008; v19:
  OBS-0079 L4 (in `Islands Light` the reference's selected tab is white with a thin grey border,
  Ferris IDEA's light blue with a blue border; where the keyboard focus was is not recorded), OSS
  (`JetBrains/intellij-community` master of 2026-11-17, equal in the keys below to the bundled
  `750ce16`: `ManyIslandsLight.theme.json` `EditorTabs.underlinedTabBackground` = `blue-150`,
  `underlinedBorderColor` = `blue-120`, `inactiveUnderlinedTabBackground` = `gray-150`,
  `inactiveUnderlinedTabBorderColor` = `gray-130`; as logic templates `IslandsTabPainter.kt` (a
  selected tab takes the first pair while its tabs are active, else the second, each as fill and
  outline) and `EditorTabbedContainer.kt` (the editor's tabs are active while the keyboard focus is
  inside them); no code taken)
- **Feature matrix rows:** F-087, F-085, F-086

## Purpose
The frame around tree and editor looks and works like the reference's default window: islands on the
window background, a main toolbar in the title bar, a tool window stripe and a status bar.

## Behaviour
1. **Islands:** the window background is `MainWindow.background`; the project tree and the editor (with
   its tabs) are islands in `ToolWindow.background` with corner radius `Island.arc`/2 and gaps of
   `Island.borderWidth`. [OSS, OBS]
2. **Main toolbar** in the title bar (the macOS window buttons sit in it): a project widget with the
   project's initials in a coloured square, its name and a menu arrow; a branch widget with the checked-out
   branch (read from the repository's `HEAD`, worktrees followed) when the folder is a Git repository.
   Widgets get the toolbar hover background. [OBS, OSS]
3. **Tool window stripe** on the left with a button per tool window that exists in Ferris IDEA (now: Project);
   the button is highlighted while its tool window is shown and toggles it (like
   `ActivateProjectToolWindow`). Buttons of tool windows Ferris IDEA does not have yet are not shown. [OBS]
4. **Tool window header** "Project" above the tree; the root row shows the folder's path after its name,
   with the home folder as `~`, in the dimmed text colour. The selected row is `Tree.selectionBackground`
   while the tree has focus and `Tree.selectionInactiveBackground` otherwise; rows are inset with rounded
   corners. [OBS, OSS]
5. **Status bar** on the window background: breadcrumbs (project, folders, file of the active tab) on the
   left; on the right the caret position (`line:column`, 1-based), the line ending (`LF`/`CRLF`), the
   encoding and the indentation (`4 spaces`, `Tab`), each with a hover background; messages of the
   workbench appear before them. [OBS]
5a. **(v3) Symbol in the breadcrumbs:** after the file come the definitions that enclose the caret,
   outermost first (for example the class, then the method), as the grammar's tags query finds them
   (SPEC-0031); methods and functions end in `()`. The list follows the caret and edits. Languages
   without a tags query, and a caret outside every definition, end at the file. [OBS-0011, GEN]
6. **Tabs** in the editor island: rounded; the selected tab has `EditorTabs.underlinedTabBackground` and a
   border in `EditorTabs.underlinedBorderColor`; other tabs use `EditorTabs.unselectedAlpha` and get
   `EditorTabs.hoverBackground` under the pointer. A line in `EditorTabs.underTabsBorderColor` separates
   tabs and text and gutter and text. [OSS, OBS]
6a. **(v19) The selected tab follows the focus:** with pill tabs (the Islands themes and the brand
   themes over them) the selected tab is filled with `EditorTabs.underlinedTabBackground` and
   outlined with `EditorTabs.underlinedBorderColor` while the window is active and the keyboard
   focus is in that tab group's editor; otherwise it is filled with
   `EditorTabs.inactiveUnderlinedTabBackground` and outlined with
   `EditorTabs.inactiveUnderlinedTabBorderColor`. In `Islands Light` that is `#E3EBFE` with a
   `#A7C5FF` border with the focus and `#E9EAEE` with a `#D1D3D9` border without it; in `Islands
   Dark` the file's `blue-40`/`blue-60` and `gray-30`/`gray-50`. Until v19 the selected tab of the
   current group kept the active fill whatever had the focus, with the border only while the group
   was current, and the tab of another group had the active fill without a border. Themes with
   underlined tabs set no inactive pair and keep their one look. The other surfaces that use the
   selected tab's colours (header pills of behaviour 23, search scopes, the profiler) are
   unchanged. [OSS, OBS-0079 L4]
7. The MCP counterparts of the information shown are `workspace_info`, `git_status`/`git_branches` and
   `editor_keystrokes` (caret positions). [ADR-0008]
8. **Status bar stays free (v2):** the status bar is a strip across the full window width at its foot,
   below the stripes, the tool windows and the editor. No island, tool window, stripe or panel reaches
   into it, whatever the window size and however many bottom windows are shown; the islands end at its
   top edge (SPEC-0167 v2 behaviour 10 says how the bottom windows make room). [OBS, GEN]

9. **(v4) Tool window header:** `ToolWindow.Header.height` high (41 px, no bundled theme sets it); the
   title 12 px from the left edge and 16 px before what follows; on the right a toolbar 12 px after the
   last tab and 8 px from the right edge with two 22 px buttons: **Options** (⋮, `general/moreVertical`)
   opens the menu of the tool window — "Move to …" for the other sides and "Hide", as the stripe button's
   context menu (SPEC-0167 behaviour 5) — and **Hide** (`general/hide`) hides the window and returns the
   focus to the editor like `HideActiveWindow`. Built for the Project and the Terminal tool window; the
   other tool windows keep their own header rows (deviation below). [OSS]
10. **(v4) Content tabs in the header:** a tool window with several contents (the Terminal's shells)
   shows a tab per content after the title, each `ToolWindow.HeaderTab.leftRightInsets` (12 px) left and
   right of its text and as high as the header. The selected tab is underlined at the bottom with
   `ToolWindow.HeaderTab.underlineHeight` (3 px) and corners of `ToolWindow.HeaderTab.underlineArc`
   (4 px), in `ToolWindow.HeaderTab.underlineColor` while the tool window has the focus and in
   `ToolWindow.HeaderTab.inactiveUnderlineColor` otherwise; the other tabs use the muted text colour and
   get `ToolWindow.HeaderTab.hoverInactiveBackground` under the pointer. Renaming, closing and adding
   shells stay as in SPEC-0205. [OSS]
11. **(v4) View chooser:** the Project tool window's title is a chooser (a chevron after it, as the
   reference's combo content) with the views "Project" (the tree with the extra nodes of behaviour 16)
   and "Project Files" (the project folder only); the chosen view is ticked. [OSS, OBS]
12. **(v4) Stripe buttons:** each button is `StripeToolbar.Button.size` (Islands 37 × 40, else 40 × 40)
   and they follow each other without a gap; the stripe is as wide as a button. The highlight while the
   window is shown and the hover background fill the button less
   `StripeToolbar.Button.leftStripeIcon.padding` on the left stripe and `….rightStripeIcon.padding` on
   the right stripe (Islands 5,5,5,2 and 5,2,5,5, compact 4,5,4,3 and 4,3,4,5, else 5 all round), with
   corners of `Button.ToolWindow.arc` / 2 (6 px, compact 4 px). Icons are 20 px
   (`StripeToolbar.Button.iconSize`): the open 20 px tool window icons where they exist, else the 16 px
   icon scaled. [OSS]
13. **(v4) Stripe order:** the default bottom windows are, in order, Git, Problems, Terminal, Services,
   then Run and Debug (registered when they first exist), and the stripe's foot group stacks them from
   the foot upward, so Git sits lowest; left and right windows stack from the top. The order kept in
   `.ferris-idea/tool-windows.json` wins over the default. [OSS, OBS]
14. **(v4) Main toolbar:** the project, branch and run configuration widgets have 10 px before and 6 px
   after their content, 5 px above and below, corners of `MainToolbar.Dropdown.arc` / 2 (6 px) and at
   most `MainToolbar.Dropdown.maxWidth` (350 px), the name cut with "…"; the 20 px project avatar stays.
   Right of the run configuration: **Run**, **Debug** (the selected configuration), **Stop** while
   something runs, then **Search Everywhere** (`general/search`) and **Settings** (`general/settings`,
   opens the Settings dialog), each a `MainToolbar.Button.size` (30 px) square button with a 20 px icon
   and corners of `MainToolbar.Button.arc` / 2 (6 px). [OSS, OBS]
15. **(v4) Status bar widgets:** the bar keeps 10 px left and right; each widget has
   `StatusBar.Widget.widgetInsets` (6 px above and below, 8 px left and right) and widgets follow each
   other without a gap, so the bar is 28 px high; the hover background fills the widget less
   `StatusBar.Widget.hoverInsets` (3, 4, 1, 4) with 4 px corners. A top border of
   `StatusBar.topBorderWidth` (Islands 0, else 1 px) in `StatusBar.borderColor` is drawn when wider than
   0. [OSS]
15a. **(v4) Compact islands:** in compact mode (SPEC-0165) the island radius and gap take
   `Island.arc.compact` / 2 and `Island.borderWidth.compact` (Islands: 8 px and 4 px instead of 10 and 6).
   [OSS]
16. **(v4) Extra tree nodes:** below the project folder's subtree the tree has two more top-level rows.
   **External Libraries** (`nodes/library`) lists, sorted and without duplicates, each module's toolchain
   as `< toolchain >` first, then the dependencies of the build files as `Tool: name:version` (the
   locked version, else the requirement), read when the node is first expanded (SPEC-0048 model).
   **Scratches and Consoles** (`fileTypes/scratches`) holds the folder **Scratches** (the `scratches`
   folder in the Ferris IDEA settings folder) and **Database Consoles** (`.ferris-idea/consoles` of the
   project) where they exist; files in them open like project files. The two rows and the libraries
   cannot be renamed, deleted, cut, copied or pasted into, and only the project folder's row shows its
   path. The view "Project Files" leaves both rows out. [OSS, OBS]
17. **(v4) MCP:** `themes` returns these values in `metrics` and `compact_metrics` (`header`, `stripe`,
   `toolbar`, `status`, `island`); `tool_windows` gives each window its stripe icon, stripe group
   (`top` or `foot`) and position, header actions and views, lists the Project view's extra nodes
   (`project_view`: libraries, scratches and consoles folders) and, with `width` and `height`, returns
   the laid-out frame (stripes, side windows, editor, bottom windows, status bar) for the saved layout
   with the given windows shown (`shown`, default Project). [ADR-0008]
18. **(v5) One header for every tool window of the layout:** Commit, Git, Run, Debug, Services,
   Database, Build and Plugins get the header of behaviour 9 from the same shared component as
   Project and Terminal: title (the window's title of SPEC-0167), content tabs (behaviour 10), the
   window's own toolbar buttons, then Options (⋮) and Hide. Their former title rows go; their own hide
   or close buttons go too, Hide of the header replaces them (Escape keeps working where it did).
   What each window shows in the header:

   | Window | Content tabs in the header | Own toolbar buttons before Options |
   |---|---|---|
   | Commit | — | — |
   | Git | one tab "Log" (selected) | — |
   | Run | a tab per run session with its × (SPEC-0087); a click selects it | Stop |
   | Debug | the tab of the debug session (SPEC-0194), when there is one | — |
   | Services | "Output", a tab per result with pin and × (SPEC-0190), the extra tab | connection, "Running…", environment |
   | Database | — | New, Edit, Refresh |
   | Build | — | Reload, after "Reading…"/"Resolving dependencies…" |
   | Plugins | — | Refresh |

   Rows below the header stay in the window: the Debug toolbar (rerun, resume, pause, stop, steps,
   breakpoints) with its "Threads & Variables"/"Console" tabs, the Git log's filter row, the Commit
   toolbar. The Problems tool window keeps its own header for now (deviation below). [OSS, GEN]
19. **(v5) More actions in the main toolbar:** after Debug (and Stop while something runs), before
   Search Everywhere, a `MainToolbar.Button.size` button with the 20 px `general/moreVertical` icon
   opens a menu below it. OBS-0007 item 2 records the button's place, **not its content**; Ferris IDEA's
   content is its own choice from the actions on the selected run configuration that have no button of
   their own: "Profile" and "Profile Memory" (SPEC-0120), a line, "Run Current File" and "Debug Current
   File" (SPEC-0047). A click runs the action and closes the menu; a click outside closes it. What the
   reference's menu holds is an open question for the comparison of AK-10. [OBS, GEN]
20. **(v5) MCP:** `tool_windows` gives each window a `header`: whether it uses the shared header
   (`shared`), its height, title, what its content tabs are (`content_tabs`: `shells`, `sessions`,
   `results`, `log` or none), its own toolbar buttons and the header actions (`options`, `hide`; none
   for a window without the shared header). With `width` and `height` the frame lists `headers`: per
   shown window its header rectangle and those of Options and Hide (22 px buttons, 8 px from the right
   edge, centred in the header). `main_toolbar` lists the buttons right of the run configuration in
   order and the entries of the more-actions menu (label and action). [ADR-0008]

20a. **(v6) Frame margins:** with Islands the islands start `Island.top` below the main toolbar (3 px,
   compact 2) and `Island.edge` away from the stripes or the window edge (3.5 px, compact 2.5); the
   visible gap between islands is `Island.borderWidth` less 1 px (5 px, compact 3), which the
   reference keeps for its antialiased edges. The status bar fills the room below the islands: 32 px
   (compact 26), its widgets centred, their hover background 22 px (compact 20) high with 4 px at
   the sides. Themes without islands keep 0 margins and the 28 px bar. [OBS-0020 items 1, 10, 15]
21. **(v6) Bottom across the full width:** the bottom tool windows span the full width between the
   stripes, below the left and right windows and the editor; the side windows end above them.
   Several bottom windows still stack (IMP-0311). [OBS-0020 item 2] (v10: one per part, behaviour 49.)
22. **(v6) Default sizes:** a left or right tool window is a third of the window's width less the
   island gap (457 px in a 1400 px window), never below 240 px; a bottom window a third of the
   window's height less the gap (310 px in a 954 px window) while the editor keeps 120 px.
   [OBS-0020 item 2]
23. **(v6) Header pills:** with Islands a content tab is a pill 28 px high (compact 24) with 1 px
   border and 6 px corners, 8 px before its text and 6 px after its content (the × included),
   centred in the header, 4 px before each pill so that the first starts 20 px after the title;
   while the window has the focus the selected pill has the selected editor tab's colours, without
   it the window colour and the popup border colour; the others get the hover background. Themes
   without islands keep the underline of behaviour 10. Problems uses the shared header too: its
   scopes (File, Project Errors, the Inspection result) are its pills, "Attach to Chat" sits before
   Options; its former row, summary and close button go. [OBS-0020 items 3, 4]
24. **(v6) Header line and buttons:** every header but Project's has a 1 px line in the window colour
   at its foot (inside the 41 px). The buttons on the right (the window's header buttons, Options,
   Hide) are shown while the window has the keyboard focus or its Options menu is open, and while
   the pointer is over the window; otherwise they are invisible but stay clickable. [OBS-0020 item 3]
25. **(v6) Toolbar rows:** the window's own buttons of behaviour 18 (Run: Stop; Services: connection,
   "Running…", environment; Database: New, Edit, Refresh; Build: status, Reload; Plugins: Refresh)
   move from the header into a 30 px row below it, 8 px from the left edge. [OBS-0020 item 4]
26. **(v6) Project header buttons:** before Options the Project header has New (`general/add`, runs
   `NewElement`), a line, Select Opened File (`general/locate`: the active tab's file is revealed and
   selected in the tree), Expand All (`general/expandAll`: the selected folder, else the project
   folder, and every folder below it, at most 200 folders) and Collapse All (`general/collapseAll`:
   every folder but the project folder; a hidden selection moves to its top-level folder). Header
   buttons are 26 px squares, 26 px apart; Hide is 8 px from the right edge. The view chooser's title
   starts at the 12 px label inset. [OBS-0020 items 3, 4]
27. **(v6) Stripe highlight:** a stripe button has the accent background (`ToolWindow.Button.
   selectedBackground`) only while its window has the keyboard focus; a window shown without the
   focus has a grey background: the window colour 16 % towards white (dark themes) or 12.5 %
   towards black (light themes). [OBS-0020 item 6]
28. **(v6) Main toolbar:** the project's initials are the first letters of its name's first and last
   word (`cmp-win-ij` → `CI`); the toolbar is tinted from the left with the avatar's colour at 25 %,
   fading to nothing at 700 px; a widget has 6 px between avatar or icon and name and its chevron 7
   px after the name; the macOS window buttons get 83 px. Without a chosen configuration the run
   widget reads "Current File" and Run and Debug start the current file (`RunClass`, `DebugClass`).
   The Settings button opens a menu below it with Settings… (⌘,), Plugins…, a line, Theme… and
   Keymap… (the settings pages); the reference's other entries name features of other rows. A
   menu's tooltip is not shown while the menu is open. [OBS-0020 items 11, 13]
28a. **(v14) Project colour of the main toolbar in the Islands themes:** in `Islands Dark` and
   `Islands Light` the toolbar takes the project colour the theme defines, not the accent:
   one of the nine `RecentProject.ColorN.MainToolbarGradientStart` colours (in `Islands Dark` dark,
   muted tones such as `#394773` and `#335661`), chosen by the project's folder so that a project
   keeps its colour, and mixed 85 % into the window colour (`#394773` → `#364268` on `#26282C`).
   From the left edge the colour rises from the window colour to the mix at the middle of the
   project avatar and falls back to the window colour over `RecentProject.MainToolbarGradient.width`
   (600 px); downwards it fades towards the window colour over
   `RecentProject.MainToolbarGradient.height` (300 px), of which the toolbar shows its own height.
   Frame, stripes and islands keep their colours (SPEC-0060 v11 behaviour 17). The other themes keep
   the accent tint of behaviour 28. Before v14 the Islands themes drew the bright accent `#3871E1` at
   25 %, which read as a clearly blue toolbar next to the reference's dark one with a faint tint
   (OBS-0047 T1, re-checked 2026-11-14 14:22 with the reference's project colours in the main toolbar
   switched on). [OBS-0047, OSS: `IslandsGradientPainter.kt` (ramp to the widget and back, downward
   fade, 85 % mix), `ProjectWindowCustomizerService.kt` (nine theme colours per project), logic
   templates, no code]
   The reference counts up a colour index for each new project and stores it; Ferris IDEA derives it
   from the folder path (FNV-1a), so the colour of a given project may differ from the reference's
   and stays the same across restarts without stored state. The avatar keeps the accent colour. The
   reference extends the same gradient over the top of the stripes and the status bar; Ferris IDEA
   tints the toolbar only.
29. **(v6) Empty editor:** without tabs there is no tab row. The lines are left-aligned in a block
   centred in the editor area (IMP-0312), 22 px apart, each label followed 8 px later by its
   shortcut; Search Everywhere reads "⇧ ⇧" on macOS; with the tree hidden "Project View" follows
   Search Everywhere, and the navigation bar line is always there. [OBS-0020 item 9]
30. **(v6) Tabs and status bar details:** the editor tab bar starts 6 px from the island's edge; the
   close button follows the label after 4 px and is 4 px from the tab's edge (the pill's border
   included), so the × is 8.5 px after the text. The status bar shows the module icon
   (`nodes/module`) before the project's name. [OBS-0020 items 8, 10]
31. **(v6) Tree rows:** the Project tree's rows span the list's width less 12 px on each side (the
   selection too), the first row 4 px below the header; a level is indented 19 px (Jewel's 23 is not
   used); the top-level chevron starts at the selection's left edge; the project folder's name is
   bold and its path in the muted colour. The indent applies to every tree that uses the shared row
   metrics. [OBS-0020 item 7]
32. **(v6) Menus:** a menu item is one 24 px line (longer content grows); separators are 1 px lines
   in the popup border colour, 8 px in from the sides. The header's Options menu holds "Move to"
   (Left, Bottom, Right; the current side disabled) and "Resize" (Stretch to Left, Right, Top,
   Bottom with their shortcuts, those across the window disabled, and Maximize Tool Window /
   Restore), each a submenu opened to the left when the pointer is over it; its items have 2 px
   between them (a 26 px pitch). The stripe button's context menu stays as in SPEC-0167.
   [OBS-0020 items 5, 14]
33. **(v6) Panels outside the layout:** Find, Find in Files, Endpoints, Spring, AI Chat, Python
   Packages, Containers, Profiler and Remote Host draw their header row with the shared header's
   height (41 px), title inset (12 px, semibold) and right inset (8 px). [OBS-0020 item 4]
34. **(v6) Resizing:** dragging a 4 px strip along a tool window's inner edge (right edge of a left
   window, left edge of a right one, top edge of a bottom one) sets its width or height; ⌃⌥←/→/↑/↓
   (`ResizeToolWindowLeft` … `Down`) move the inner edge of the focused window by 8.9 % of the
   window's height (85 px at 954 px, as measured), a direction across the window does nothing;
   ⇧⌘' (`MaximizeToolWindow`) gives the focused window the whole room (a bottom window the whole
   column, the editor collapsed) and a second press restores its size. Sizes are at least 80 px,
   kept per project in `.ferris-idea/tool-windows.json` (`size` per window) and win over the default of
   behaviour 22; side windows never get wider than the room between the stripes. [OBS-0020 items
   5, 16]35. **(v6) Compact values (Islands):** header 32 px with 24 px pills; stripe buttons 32 × 32 with a
   24 px highlight (the theme's compact padding) and 16 px icons; editor tabs 24 px in a 32 px bar;
   status bar 26 px; island gap 3, edge 2.5, top 2 (behaviour 20a). [OBS-0020 item 15]
36. **(v6) MCP:** `themes` reports the new values (`island.edge`, `island.top`, the header's `pill*`
   values, the status bar's height and hover insets, compact stripes); `tool_windows` reports per
   window its `size` and in `header` the `buttons`, `toolbar_row` and `line`, lays out the frame with
   margins, full-width bottom windows and third sizes, sets a size with `resize` and `size` (0 for
   the default), and lists the Settings menu (`main_toolbar.settings_menu`) and the run widget's
   label without a configuration. [ADR-0008]

37. **(v7) Placements:** each side has two halves, so a tool window has one of six placements: Left
   Top, Left Bottom, Bottom Left, Bottom Right, Right Top and Right Bottom (the second of each pair
   is the *split* half; v10: one window per placement everywhere, behaviour 49). On the left and right one
   window per half is shown at a time (showing one
   hides the shown window of the same half, SPEC-0167 behaviour 4); a shown top window and a shown
   bottom window share the column, the top one above, each half of the column's height less half
   the island gap, the column as wide as the top window (or the bottom one when it is alone). At
   the bottom, Bottom Left windows stack in the left part and Bottom Right windows in the right part
   (IMP-0311 within each part; v10: one window per part, behaviour 49); with both parts in use each takes half the width less half the gap,
   and both parts are as high as the stack with more windows needs. The placement is kept per
   window (`side`, `split`). The Options menu's "Move to" (behaviour 32) lists the six placements
   (the current one disabled); the stripe button's context menu lists them as "Move to …" entries.
   [OBS-0020 item 5, OSS]
38. **(v7) Stripe groups:** the left stripe shows, from the top, the Left Top windows, a separator
   line (1 px high, 16 px wide, in the popup border colour, 4 px above and below) when Left Bottom
   windows follow, the Left Bottom windows and the **More tool windows** button (behaviour 41); at
   its foot the Bottom Left windows, stacked from the foot upward. The right stripe shows the Right
   Top windows, the separator, the Right Bottom windows, and at its foot the Bottom Right windows.
   Structure is a Left Bottom window by default, so it follows Commit after the line. Removed
   windows (behaviour 40) and windows that do not exist yet have no button. [OBS-0020 item 6, OSS]
39. **(v7) View modes:** the Options menu has "View Mode" (a submenu like "Move to") with **Dock
   Pinned** (the default), **Dock Unpinned**, **Undock**, **Float** and **Window**; the current one
   is ticked. Choosing a mode shows the window in it.
   - *Dock Pinned:* as before.
   - *Dock Unpinned:* docked and laid out like a pinned window; once it had the keyboard focus it
     hides as soon as the focus moves to another part of the same IDE window (the editor, another
     tool window). The focus going to another application does not hide it (IMP-0314).
   - *Undock:* drawn over the editor area at its placement's edge (left, right or bottom) with its
     size and a shadow; the editor and the other windows keep their room. It stays until hidden.
   - *Float:* a separate window of its own (GPUI `WindowKind::Floating`, kept above the IDE
     window), titled with the tool window's title, holding the window's header (title, tabs,
     Options, Hide) and content.
   - *Window:* a separate normal window of its own (`WindowKind::Normal`), otherwise like Float.

   Float and Window windows take no room in the IDE window; the stripe button stays and toggles
   them; Hide in their header, the stripe button or closing the window hides the tool window. Their
   last frame is kept (`bounds`) and used when they open again, unless it lies off every display
   (IMP-0313); the first frame is the window's docked size centred on the IDE window. Dock Pinned,
   Dock Unpinned and Undock keep the placement; Float and Window keep it for when the window is
   docked again. [OBS-0020 item 5, OSS, GEN]
40. **(v7) Remove from Sidebar:** the last entry of the Options menu and of the stripe button's
   context menu hides the window and takes its stripe button away (`removed`). Its activation
   action, the More tool windows popup and MCP can still show it; showing it brings its button
   back at its placement. [OBS-0020 item 5]
41. **(v7) More tool windows ("…"):** a stripe button with three horizontal dots (own icon
   `ferris-idea/moreHorizontal`) after the left stripe's top groups opens a popup listing every tool
   window that exists, by title in alphabetical order, each with its stripe icon and its activation
   shortcut; shown windows are ticked and removed windows marked "removed". A click activates the
   window like its stripe button (a hidden window is shown and focused, a shown one hidden) and
   closes the popup; a click outside closes it. [OBS-0020 item 6, GEN]
42. **(v7) Structure tool window:** id `structure`, Left Bottom, activated by
   `ActivateStructureToolWindow` (⌘7, Alt+7), own icon `ferris-idea/structure`. It shows the symbols of
   the active editor's file as the File Structure popup does (SPEC-0079: order, nesting, speed
   search, filters), filling the window, and loads them again when it is shown and when the active
   tab changes while it is shown. Enter or a click moves the caret to the symbol and keeps the
   window open; Escape gives the focus back to the editor. Without a file with symbols it says so.
   Shared header without tabs. [OSS, SPEC-0079]
43. **(v7) The panels outside the layout become tool windows:** Find (`find`,
   `ActivateFindToolWindow`, ⌘3), Find in Files (`find_in_files`, `FindInPath`), Endpoints
   (`endpoints`), Spring (`spring`), AI Chat (`ai_chat`), Python Packages (`python`), Containers
   (`containers`), Profiler (`profiler`) and Remote Host (`remote`) are tool windows of the layout,
   Bottom Left by default and in this order after Plugins. Each gets a stripe button once it was
   opened (like Run, SPEC-0167 behaviour 2), takes part in placements, view modes, sizes, Hide All
   and "Remove from Sidebar", and gets **Options** (⋮) and **Hide** at the right end of its own
   header row (behaviour 33), which keeps its title and controls (`shared: false` in the header
   model, no line). Stripe icons: `general/search` (Find, Find in Files), `fileTypes/http`
   (Endpoints), `fileTypes/python`, `fileTypes/docker` (Containers), own `ferris-idea/spring`,
   `ferris-idea/chat`, `ferris-idea/profiler` and `ferris-idea/remote`, drawn at 20 px. [OBS-0020 item 4
   (L15), GEN]
44. **(v7) Traffic lights:** GPUI 0.2.2 sets only the position of the macOS close button
   (`traffic_light_position`, 14 × 13 as before) and keeps the pitch AppKit gives the three buttons
   (23 pt on macOS 27). Ferris IDEA places the minimize and zoom buttons 20 pt after the one before
   (`TRAFFIC_LIGHT_PITCH`) when a window opens and again after every change of its bounds or
   activation; GPUI keeps a pitch it finds when it moves the buttons itself. Other systems are not
   affected. [OBS-0020 item 17, GEN]
45. **(v7) Persistence:** placement, mode, `removed` and the Float/Window `bounds` are kept with the
   sides and sizes per project in `.ferris-idea/tool-windows.json`; a missing entry means Dock Pinned,
   the window's default half and a stripe button; an unknown mode reads as Dock Pinned.
   [SPEC-0167 behaviour 8]
46. **(v7) MCP:** `tool_windows` reports per window its `placement` (`left_top` …), `split`, `mode`,
   `removed` and `bounds`, and the stripe group `split` besides `top` and `foot`; `move` takes
   `split` (default false) besides `side`; `set_mode` with `mode` sets a window's view mode;
   `remove` and `restore` take a window id and take its stripe button away or bring it back. The
   frame lays out split columns and bottom parts, leaves Float and Window windows out and lists
   Undock windows as `overlays`; `more_tool_windows` lists the popup's entries. [ADR-0008]

47. **v8 – Long status messages:** the breadcrumbs of the status bar shrink and are cut at the right
    when a status message needs the room; the message takes at most 70 % of the bar. Before v8 a long
    message (a stopped rebase, a refused push) was drawn over the breadcrumbs. [GEN, OBS-0022 L5]
48. **v9 – Hover cards and menus (extends 32):** opening any menu or popup of the window (context
    menus, the tab menu, the menu bar's menus, the change bar of SPEC-0064 v3) hides the hover cards
    of every editor view (the annotation card of SPEC-0277 behaviour 4, the problem popup of
    SPEC-0078), and a hover card also goes when the pointer leaves its editor view, also when a
    popup now lies between pointer and editor. While a menu is open no editor shows a new hover
    card. Before v9 the annotation card of a split view stayed over the menu. [GEN, OBS-0022 L28]

49. **v10 – Replace instead of stack (changes 21 and 37; SPEC-0167 v4 behaviour 4):** in each of the
    six placements at most one tool window is shown. Showing a window — by its stripe button, its
    activation action, a menu, the More tool windows popup, or by itself (a run session opening Run,
    a search opening Find) — hides the window shown in the same placement; the hidden window keeps
    its sessions, shells and state where it kept them before (hiding Git drops its log view as
    before). Two windows at the bottom at once only as Bottom Left and Bottom Right (behaviour 37)
    or with ⌥-click (behaviour 50). Commit still only covers Project in the same placement
    (SPEC-0063 behaviour 6). It applies to Dock Pinned, Dock Unpinned and Undock windows; Float and
    Window windows take no placement. When several windows of one placement become shown at the
    same time (window start), the one with the keyboard focus stays, else the first in stripe
    order. Reason (project owner, 2026-11-06): every further stacked window made the others smaller, their
    content jumped, and IntelliJ users expect the replacement (F-087 parity). [OBS-0020 item 2]
50. **v10 – ⌥-click into the other half (IMP-0311):** ⌥-click (Alt-click) on a stripe button shows
    the window in the other half of its side — Left Top ↔ Left Bottom, Bottom Left ↔ Bottom Right,
    Right Top ↔ Right Bottom — for as long as it stays shown. Its saved placement, the place of its
    stripe button and `.ferris-idea/tool-windows.json` do not change; hiding it ends the exception.
    A window shown in that half is replaced (behaviour 49). ⌥-click on a window shown in its own
    half moves it to the other half, on a window shown in the other half hides it. A plain click
    replaces as in behaviour 49. [GEN]
51. **v10 – Default stripes (changes 13 and 38; SPEC-0167 v4 behaviour 1):**

    | Placement | Windows, in stripe order |
    |---|---|
    | Left Top | Project, Commit (button only while the folder is in a Git repository) |
    | Left Bottom | Structure (after the line), then the More tool windows button |
    | Bottom Left (left stripe's foot, from the foot upward) | Git, Problems, Terminal, Services, Run and Debug (once they exist), Find and Find in Files (once used) |
    | Right Top | AI Chat (always), Database, Build |
    | off the sidebar | Plugins (Right Top), Endpoints, Spring, Python Packages, Containers, Profiler, Remote Host (Bottom Left) |

    Build is Ferris IDEA's build tools window (Maven, Gradle and the other build tools, SPEC-0100); the
    reference keeps Maven and Gradle on the right, its Build output window has no counterpart of
    its own. AI Chat stands for the reference's AI window. The reference's Notifications window does
    not exist in Ferris IDEA (out of scope). Windows off the sidebar are reached through "…", their
    activation action and MCP until the user adds them (behaviour 52). [OBS-0020 item 6, OBS-0007
    item 3, OSS]
52. **v10 – Windows off the sidebar (changes 40):** a window off the sidebar — by default
    (behaviour 51) or after "Remove from Sidebar" — has a stripe button in its placement's group
    only while it is shown; the button goes when it hides. Showing it no longer adds it to the
    sidebar (v7 did). **Add to Sidebar** takes the place of "Remove from Sidebar" as the last entry
    of the Options menu and of the stripe button's context menu for such a window and keeps its
    button; MCP `restore` does the same. [GEN]
53. **v10 – More tool windows (changes 41):** the popup lists every tool window that can be opened
    now — all but Run and Debug without a session and Commit outside a Git repository — by title;
    shown windows are ticked, windows off the sidebar marked "not in sidebar". A click on a hidden
    window shows it in its placement (replacing, behaviour 49), on a shown one hides it; it does
    not add it to the sidebar. [GEN]
54. **v10 – Migration of untouched layouts:** a `.ferris-idea/tool-windows.json` without `"version": 2`
    whose listed windows are all at their v7 default placement (Project, Commit Left Top; Structure
    Left Bottom; Database, Build, Plugins Right Top; all others Bottom Left), in the v7 default order
    within each placement and none removed was never arranged by the user: it is read with the
    defaults of behaviour 51 (placements, order, sidebar) and keeps its sizes, view modes, frames
    and the stripes setting. Any other file — a window moved, reordered or removed — is read as it
    is. Files are written with `"version": 2`, so a migrated layout is not looked at again. [GEN]
55. **v10 – What else follows from it:** the frame lays out at most one window per bottom part
    (both parts each half the width less half the gap); the heights of SPEC-0167 behaviour 10 apply
    to that one window; ⌃⌥←/→/↑/↓ and ⇧⌘' act on the focused window, a ⌥-clicked window in the half
    it is shown in; Hide All and its restore bring back exactly the hidden windows, at most one per
    placement. [GEN]
56. **v10 – MCP `tool_windows`:** per window `default_placement`, `on_sidebar` (has a button when
    not shown), `on_sidebar_by_default` and `alt_click` (the placement ⌥-click shows it in); at the
    top `layout_version`, `migrated` (the file was read as an untouched v7 layout) and
    `one_per_placement: true`; the frame resolves `shown` with one window per placement — a later id
    replaces an earlier one of the same placement, listed in `replaced` — and takes `split_shown`
    (ids shown in the other half, as with ⌥-click); `more_tool_windows` lists the windows with
    `on_sidebar`. [ADR-0008]
57. **v11 – Window check of v10 (OBS-0020 re-check 2):** Services has its stripe button from the
    start, as in the reference (behaviour 51 lists it without "once they exist"). The More tool
    windows popup lays out tick, icon, title, "not in sidebar" and shortcut in columns with gaps, and
    any key closes it — Escape only the popup (behaviour 53). Listing every window there, not only the
    ones off the sidebar as the reference does, is kept as an improvement (IMP-0360). Hints in a
    tool window header are cut before the header's buttons. [OBS]
58. **v12 – Full screen:** `ToggleFullScreen` (⌃⌘F in the macOS keymap, the chord macOS itself gives a
    window's full-screen mode; no key in the default keymap; Window › Toggle Full Screen after Zoom, Find
    Action) puts the project window into the system's full-screen mode and takes it out again, as the
    system's own control of the window does. The layout of the window does not change and nothing is
    stored: the mode is the system's. The menu item keeps one name in both states, as Ferris IDEA's menus do
    not change with the context (SPEC-0124). Agents need no tool for it; `ui_act` presses the key. [GEN,
    STD: the full-screen mode of the platform's windows; OSS: the action id and its shortcut in the
    bundled keymap]
59. **v13 – Tool windows come back (window check 2026-11-14):** the tool windows shown when the
    project's window closes or Ferris IDEA quits — Terminal, Database, the Project tree and every other
    — are shown again when the project opens, in the arrangement they had; they are written with the
    editor tabs (`.ferris-idea/editor-tabs.json`, `tool_windows`). A window that cannot be shown at
    once (Run or Debug without a session) stays hidden; unknown names are skipped; a file written
    before v13 has none and leaves the start as it was. Until v13 every tool window but the Project
    tree came back closed. [GEN]
60. **v13 – Navigation bar with a diff tab:** while a diff tab is the active tab of the main group,
    the navigation bar names the file it compares (the project alone for a diff of no file); the
    caret's definitions, position and line ending are those of an editor and are not shown. Until
    v13 the bar kept naming the file tab that was active before the diff. [GEN]
61. **v15 – Clicks on the controls of the title bar (second window check 2026-11-14):** the main
    toolbar shares the title bar (behaviour 14), and macOS counts the clicks in a title bar
    itself: two within the system's double-click time zoom the window (or minimise it, as set in
    the system settings). A press of the primary button on a control of the toolbar — the project,
    branch and run widgets, Run, Debug, Stop, the more-actions, search and settings buttons —
    belongs to the control: the system's count starts anew with it, so two quick clicks on a
    control are two clicks on the control, and the window keeps its size and place. Two clicks on
    the empty part of the title bar stay the system's gesture and zoom the window, as do two
    clicks between two controls; dragging the empty part moves the window. Until v14 the second
    of two quick clicks on the run widget zoomed the window and opened nothing. Other systems draw
    their own title bar above the toolbar: nothing changes there. [GEN, STD: the platform's
    double click in a title bar]
62. **v16 – Stop Background Processes…:** `StopBackgroundProcesses` (⇧⌘F2 in the bundled macOS
    keymap, Ctrl+Shift+F2 in the default one; Run › Stop Background Processes…, SPEC-0124 v14
    behaviour 16) opens a list of the background tasks of this window that run now and that
    Ferris IDEA can stop.
    - **The tasks,** in this order, each one row while it runs: a repository being cloned from
      the project popup, with the progress line of the clone behind it (SPEC-0094 v5); a
      remote project that is opening (SPEC-0318); a search of Find Usages (SPEC-0293); the
      project scan of the TODO tool window (SPEC-0334); a statement that runs in the SQL
      console and that the database can cancel (SPEC-0093 v3 behaviour 12); an answer of the AI
      Assistant that is arriving (SPEC-0157).
    - **Stopping.** ↑, ↓, Home and End (SPEC-0275 v19 behaviour 47) move, Enter or a click
      stops the task of the row and closes the list; Escape closes it and stops nothing. Each
      task is stopped the way its own Cancel or Stop stops it, with the same result: the
      clone ends and removes what it wrote, the opening of the remote project ends what was
      started on the host, Find Usages shows what it has, the TODO window keeps its last
      result, the console sends the cancel request and reports how it went, the assistant
      keeps the text that arrived. The status bar names the task that was told to stop. A
      task that ended while the list was open is no longer there to stop, and the status bar
      says that nothing runs. Only the chosen task is stopped.
    - **Nothing to stop.** With no such task no list opens and the status bar says that no
      background task is running that can be stopped.
    - **Not in the list,** because Ferris IDEA has no way to stop them: the loading of a
      project's model and its symbol index, the start of a language server and its requests,
      Git operations other than the clone (fetch, pull, push, the log), an inspection run,
      a search of Find in Files (it ends when its popup closes), builds and build
      synchronisation, and downloads. Programs of the Run and Debug tool windows are stopped
      with Stop (SPEC-0087), not here.
    - UI automation counts the action among those that act on what runs, like Stop
      (SPEC-0319). There is no MCP tool for it: the tasks belong to what the user started in
      the window.
    [OSS: the id, the title "Stop Background Processes…" from `ActionsBundle.properties` and
    the chords of the bundled keymap files; GEN: a list of running tasks with a way to stop
    one. What the reference's list shows and how its rows are stopped was not observed.]
63. **v17 – Structure icon (OBS-0046 A1):** the Structure tool window's stripe button, its row in
    the More tool windows popup and its menus use the open New UI tool window icon
    `toolwindows/structure` (20 px on the stripe, 16 px elsewhere) instead of Ferris IDEA's own tree
    drawing. [OBS-0046 A1, OSS]
64. **v17 – Notifications tool window (OBS-0046 A2):** id `notifications`, title "Notifications",
    action `ActivateNotificationsToolWindow` (no key in the bundled keymaps; View › Tool Windows,
    Find Action), Right Top, first on the right stripe and on the sidebar by default, open icons
    `toolwindows/notifications` (16 and 20 px). It lists the notes the workbench showed in the status
    bar of this project window (SPEC-0070 behaviour 4: outcomes, hints and failures of actions),
    newest first, each with the time it was first shown (hours and minutes) and an icon: failures
    (as SPEC-0070 v13 behaviour 4d tells them apart) the error icon, the others the info icon. A note
    shown again while it is the newest one is not listed twice: the row counts its repeats ("×3")
    and takes the new time. Passing hints that go when the caret moves (SPEC-0070 v10 behaviour 4b)
    are not listed. The window keeps the newest 100 notes of its project window for as long as that
    window is open; nothing is written to disk. Its toolbar has **Clear All** (`general/delete`),
    which empties the list; without notes the window says "No notifications". Shared header without
    tabs, no focusable content. The notes stay in the status bar as before: there are no balloons
    over the window. MCP **`notifications`** (reads, needs no right): the notifications of the
    workspace's window, newest first, each with `text`, `kind` (`error` or `info`), `count` and
    `time` (seconds since the Unix epoch of the last showing), at most `limit` (1–100, default 100);
    an empty list when no window serves the workspace. [OBS-0046 A2, OSS, GEN, ADR-0008]
65. **v17 – Build button after the first build (OBS-0046 A4):** Build (the build tools window,
    behaviour 51) gets its stripe button once it has something to show or did something: the project
    has a Maven or Gradle build file the build tools read, Build Project or a build tool task ran in
    this window, or the window was opened (its action, "…", MCP). Before that it has no button; "…"
    and its action still show it. Until v14 it always had a button. [OBS-0046 A4]
66. **v17 – Run and Debug without anything to run (OBS-0046 A5):** while the run widget shows
    "Current File" (no configuration chosen), the main toolbar's Run and Debug buttons are grey —
    their icons in the disabled text colour, no hover — and a click does nothing, when the active
    file offers nothing to run: no editor is active, the file lies outside the project, or the
    editor's Run (`RunClass`, SPEC-0122) would find no configuration for it (for example a Java or
    Kotlin file without `main` or tests, a Rust file without `main`, a text file). HTML pages count
    as runnable (they open in the browser). The tooltip of a grey button says "Nothing to run in the
    current file". The state follows the active editor's text and the project's modules as read
    when the file became active. With a chosen configuration, and in a remote project (its host
    decides), the buttons stay as before. [OBS-0046 A5, SPEC-0087, SPEC-0122]
67. **v17 – Menu marker (OBS-0046 A6):** a tool window header button that opens a menu or popup
    carries the open `general/dropdown` marker, a small triangle in the bottom-right corner of its
    16 px icon: the Project window's "+" (New, which opens the New popup). Buttons that act at once
    have none. [OBS-0046 A6, OSS]
68. **v18 – Emoji & Symbols:** `EmojiAndSymbols` opens the macOS character palette for the
    focused text input. macOS itself takes its key ⌃⌘Space with its default settings, so the
    action is reached by Find Action. On other systems the status bar says that there is no
    such palette. [DOC, GEN]

## Acceptance criteria
- **AK-1** Project initials, branch from `HEAD` and from a worktree's `.git` file, detached head and no
  repository, `~` paths, breadcrumbs, indentation labels (unit tests).
- **AK-1a (v3)** The definitions around a position in Java, Python and Rust, outermost first, none
  outside every definition and none for a language without tags (unit test
  `syntax::tests::definitions_around_a_position`); the breadcrumbs append them
  (`chrome::tests::paths_and_labels`).
- **AK-2** The Islands palette: window and island colours, corner radius 10, gap 6, selected tab colours,
  inactive tree selection, hover alpha (unit test).
- **AK-3** The window self-test passes with the new layout and typing latency stays below 16 ms p95.
- **AK-5 (v2)** The laid-out frame keeps every island and stripe out of the status bar at several
  window sizes, with and without stripes and with up to six bottom windows (unit test
  `tool_windows::tests::bottom_windows_fit_above_the_status_bar`); in a real window with Terminal,
  Problems and Git at the bottom, at 1100 × 620 and 1600 × 1000, no painted island or stripe reaches
  into the full-width status bar (`--selftest`); by hand with Problems, Run and Debug open (hand test).
- **AK-4** Side-by-side check with the reference by hand (layout, colours, window dragging by the
  toolbar).
- **AK-6 (v4)** Header, tab, stripe, toolbar, status bar and island values of Islands Dark, Islands
  Light and Dark and the compact Islands values; the stripe highlight inside the button on both sides
  (unit tests `metrics::tests::window_structure_metrics`, `metrics::tests::stripe_highlight_insets`).
- **AK-7 (v4)** Default stripe order: bottom windows Git, Problems, Terminal, Services, Run, Debug, the
  foot group drawn bottom-up (unit test `tool_windows::tests::stripe_groups_follow_the_reference`).
- **AK-8 (v4)** Extra tree nodes: order of the top-level rows, library entries from a Cargo and a Maven
  module, scratches and consoles folders only where they exist, opening a scratch file, the fixed rows
  refused as targets of file operations, the "Project Files" view without them, reloading keeps them
  expanded (unit tests `project_tree::tests::extra_nodes_*`, `project::tests::library_entries`).
- **AK-9 (v4)** MCP: `themes` metrics with `header`, `stripe`, `toolbar`, `status`, `island`;
  `tool_windows` with stripe groups, header actions, views, `project_view` and a frame for 1200 × 800
  (protocol test `tool_windows_layout`).
- **AK-10 (v4)** Side-by-side comparison of the main window with the reference after these changes
  (new observation against OBS-0007); open. (v5) It also records what the reference's more-actions
  menu after the run widget holds and the headers of Commit, Git, Run, Debug, Services and Database.
- **AK-11 (v5)** Header model: every window of the layout but Problems uses the shared header with
  Options and Hide; Run, Debug and Services have session/result tabs, Terminal shells, Git the log;
  the own toolbar buttons per window as in behaviour 18 (unit test
  `tool_windows::tests::headers_of_every_tool_window`).
- **AK-12 (v5)** Header layout: title at 12 px, the first tab 16 px after the title, Hide 8 px from
  the right edge and Options right before it, both centred in the 41 px header, own toolbar buttons
  before Options; a narrow header keeps Options and Hide inside (unit test
  `metrics::tests::header_layout_places_tabs_and_actions`).
- **AK-13 (v5)** Main toolbar: the button order right of the run configuration with the more-actions
  button after Debug and Stop, the menu's entries and their actions known to the menus (unit test
  `menus::tests::run_widget_more_actions`).
- **AK-14 (v5)** MCP: `tool_windows` reports `header` per window (shared, content tabs, toolbar,
  actions; Problems without), `headers` in the frame for the shown windows, and `main_toolbar` with
  the more-actions entries (protocol test `tool_windows_layout`).

- **AK-15 (v6)** Frame of the reference's 1400 × 954 window: islands at 40.5 and up to 1359.5, 43
  below the top, status bar from 922, Project 457 wide, the bottom window 310 high across the full
  width below Project and the editor (unit test `tool_windows::tests::frame_matches_the_reference_window`);
  with up to six bottom windows nothing reaches into the status bar or a side window
  (`tool_windows::tests::bottom_windows_fit_above_the_status_bar`).
- **AK-16 (v6)** Metrics: tree indent 19, island gap/edge/top normal and compact, status bar 32/26
  with its hover insets, header buttons 26, pills 28/24 laid out 20 px after the title, compact
  stripes 32 × 32 with 16 px icons, compact tabs 24/32 (unit tests `metrics::tests::*`).
- **AK-17 (v6)** Headers: every window shared incl. Problems, the line on all but Project, Project's
  buttons (unit test `tool_windows::tests::headers_of_every_tool_window`).
- **AK-18 (v6)** Expand All and Collapse All in the tree model (unit test
  `project_tree::tests::expand_all_and_collapse_all`); initials of first and last word
  (`chrome::tests::initials`); empty editor lines (`chrome::tests::empty_editor_hints_follow_the_keymap`);
  the grey stripe colour (`palette::tests`).
- **AK-19 (v6)** The Settings button's menu entries are actions the workbench runs (unit test
  `menus::tests::settings_button_menu`).
- **AK-20 (v6)** A dragged size is saved and read back, never below 80 px, and can be cleared (unit
  test `tool_windows::tests::saved_and_read_with_unknown_and_missing_entries`).
- **AK-21 (v6)** MCP: the frame with margins and third sizes, headers with 26 px buttons, `resize`,
  `settings_menu` and the run widget's label (protocol test `tool_windows_layout`).
- **AK-22 (v6)** Side-by-side check against the reference in the window (OBS-0020 Nachprüfung).
- **AK-23 (v7)** Placements: six placements with names and labels, one window per half on the
  left and right, Structure Left Bottom by default, stripe groups with the split group and the
  foot of the right stripe, removed windows without a button (unit tests
  `tool_windows::tests::placements_and_split_halves`,
  `tool_windows::tests::stripe_groups_follow_the_reference`).
- **AK-24 (v7)** Frame: a split left column in two halves, Bottom Left and Bottom Right parts side
  by side, Undock windows as overlays, nothing in the status bar (unit tests
  `tool_windows::tests::split_columns_and_bottom_parts`,
  `tool_windows::tests::bottom_windows_fit_above_the_status_bar`).
- **AK-25 (v7)** Modes, removed buttons and bounds are saved and read back, an unknown mode reads
  as Dock Pinned, off-screen bounds fall back to the centre of the IDE window (IMP-0313), and the
  auto-hide rule of Dock Unpinned (IMP-0314) (unit tests
  `tool_windows::tests::modes_removed_and_bounds_are_saved`,
  `tool_windows::tests::float_bounds_stay_on_a_display`,
  `tool_windows::tests::unpinned_windows_hide_when_the_focus_leaves`).
- **AK-26 (v7)** Every tool window has an id, title, action and icon and a header with Options and
  Hide, the former panels with their own header; the More tool windows popup is sorted by title
  (unit tests `tool_windows::tests::headers_of_every_tool_window`,
  `tool_windows::tests::more_tool_windows_are_sorted`,
  `icons::tests::every_icon_has_both_variants_and_files_map_to_bundled_icons`).
- **AK-27 (v7)** Traffic lights: close button at 14 × 13, pitch 20 pt (unit test
  `metrics::tests::traffic_lights_follow_the_reference`); measured in the window against the
  reference (open).
- **AK-28 (v7)** MCP: `tool_windows` with `move` and `split`, `set_mode`, `remove`, `restore`, the
  new fields per window, the split frame, `overlays` and `more_tool_windows` (protocol test
  `tool_windows_modes_split_and_remove`; `ferris-idea call … tool_windows`).
- **AK-29 (v7)** Window self-test steps for the view modes, split placements, removing and
  restoring, "…" and Structure (`tool_window_mode_steps`); written, run with the next window
  self-test (open).

- **AK-30 (v8)** In the release bundle: with "Rebase stopped: conflicts in …" the breadcrumbs end
  before the message, nothing overlaps (OBS-0022 Nachprüfung).
- **AK-31 (v9)** Check in the window (needs a real pointer, after the coordinator's run): with the
  annotations shown in the second view of a split and the card of a line shown, a secondary click
  opens the menu and no card is shown; Escape and moving the pointer back onto the column show it
  again.

- **AK-32 (v10)** Replacement in all six placements, the focused or first window staying when
  several appear at once, Commit covering Project, Float and Window windows not counted (unit
  tests `tool_windows::tests::one_window_per_placement_replaces`,
  `tool_windows::tests::newcomers_replace_the_shown_window`).
- **AK-33 (v10)** ⌥-click: the other half of each side, the saved placement and the file unchanged,
  replacement there, the exception gone after hiding (unit test
  `tool_windows::tests::alt_click_opens_in_the_other_half`).
- **AK-34 (v10)** Default stripes and sidebar, a button only while shown, Add to Sidebar, the More
  list (unit tests `tool_windows::tests::default_sides_and_stripe_groups`,
  `tool_windows::tests::stripe_groups_follow_the_reference`,
  `tool_windows::tests::off_sidebar_windows_have_a_button_while_shown`).
- **AK-35 (v10)** Migration: an untouched v7 file gets the new defaults with its sizes and modes, a
  moved, reordered or removed one stays, a version 2 file is not migrated (unit test
  `tool_windows::tests::untouched_layouts_are_migrated`).
- **AK-36 (v10)** MCP: the new fields, `replaced` and `split_shown` in the frame (protocol test
  `tool_windows_replace_and_alt_click`; `ferris-idea call <folder> tool_windows`).
- **AK-37 (v10)** Window self-test steps (`tool_window_replace_steps`; `tool_window_mode_steps` and
  `status_bar_steps` changed to one window per part): written, run with the next window self-test
  (open); check in the window (open).
- **AK-v11** Window self-test (full run green), OBS-0020 re-check 2 (migration, replacing per placement, ⌥-click, "…", Add to Sidebar).

- **AK-v12 (behaviour 58)** The action has its title, its place in the Window menu, its key in the macOS
  keymap and is offered without one in the default keymap
  (`everywhere::tests::run_debug_and_window_actions_from_the_keymap`,
  `menus::tests::run_debug_and_window_actions_are_in_their_menus`), and the workbench carries it out
  (`menu_bar::tests::every_menu_action_is_carried_out_somewhere`). Entering and leaving full screen in a
  window: not checked.
- **AK-v13 (behaviour 59)** `editor_layout::tests::the_shown_tool_windows_come_back`: the shown
  windows are written and read back, each once, unknown names skipped, a file of an earlier version
  leaves the start alone. By hand: Terminal and Database open, quit, start — both open again.
- **AK-v15 (behaviour 61)** `title_bar::tests::a_press_on_a_control_is_not_the_title_bars`: a press
  of the primary button on a control's place is the control's, one beside it, between two controls
  or with the secondary button is not. Window self-test, group `title-bar-clicks` (12 checks, with
  native double clicks posted to Ferris IDEA's own event queue): two quick clicks on the run widget and
  on the search button leave the window's size as it was and reach the control twice; a double
  click on the empty title bar is no press on a control and zooms the window, a second one gives
  it its size back (only where the system setting zooms on a double click). Without the change the
  group fails on the two size checks.
- **AK-v13b (behaviour 60)** `chrome::tests` (`shown_file`): a diff tab in front names its file and
  no editor details; without one the active file and its details. By hand: open a file, then the
  diff of another — the bar names the other file.
- **AK-v16 (behaviour 62)** The six tasks, their rows with and without a detail and the text
  after a stop (`keymap_actions::tests::background_tasks_have_rows_in_a_fixed_order`). In a
  window of the test harness, with four tasks counted as running by a stop flag of their own
  (no network, host or language server takes part): the list has its title and the four rows
  in order with the clone's progress line; ↓ twice and Enter set the stop flag of Find Usages
  and of no other task, and the status bar names it; the list then has three rows; End and
  Enter stop the last one; a task that does not run is not stopped; without a task no list
  opens and the status bar says so
  (`seventh_package_keymap.rs::background_tasks_are_listed_and_stopped_by_row`). The action is
  in Run and Find Action with its key in both bundled keymaps
  (`menus::tests::seventh_package_items_are_in_their_menus`,
  `everywhere::tests::seventh_package_actions_have_titles`). Not checked: a real clone, a real
  remote opening, a running console statement and an arriving answer stopped from the list,
  and the list on screen.
- **AK-v17a (behaviour 63)** `tool_windows::tests::f086_icons_of_oss0046`: Structure's icons are
  the open `toolwindows/structure` ones and bundled light and dark
  (`icons::tests::every_icon_has_both_variants_and_files_map_to_bundled_icons`).
- **AK-v17b (behaviour 64)** `notifications::tests` (repeats, the limit of 100, clearing, one log per
  project window); the window's id, title, action, place, icons and header
  (`tool_windows::tests::f086_icons_of_oss0046`), View › Tool Windows and Find Action
  (`menus::tests`, `everywhere::tests`); window test `f086_icon_gaps::a_status_note_is_listed_and_cleared`
  (a note is listed with its repeats, a passing hint is not, Clear All empties it, the button on the
  right stripe); MCP protocol test `notifications_tool`.
- **AK-v17c (behaviour 65)** Window test `f086_icon_gaps::the_build_button_waits_for_a_build`: no
  Build button in a project without a Maven or Gradle file until Build Project ran; a project with a
  `pom.xml` has it from the start.
- **AK-v17d (behaviour 66)** Window test `f086_icon_gaps::run_and_debug_follow_the_current_file`:
  with "Current File", grey for a text file and for no editor, active for a Python file; a chosen
  configuration keeps them active.
- **AK-v17e (behaviours 63–67)** Side-by-side check against the reference (OBS-0046 re-check):
  open.
- **AK-v19 (behaviour 6a)** `palette::tests::selected_tab_follows_the_focus_in_islands_light`: the
  four colours of `Islands Light`; `metrics::pill_colours` gives the active pair with the focus and
  the inactive pair without; every pill theme reads both pairs from its file, the underline themes
  keep one. Window test `crates/workbench/tests/islands_light_editor.rs`: the selected tab of the
  focused editor is active, with the focus in the Project tree it is not. Side by side with the
  reference, with the focus in the editor in both windows and then in the Project tree: open
  (coordinator's window check of OBS-0079).
- **AK-v14 (behaviour 28a)** `palette::tests::frame_islands_and_selection_come_from_the_open_theme`:
  `Islands Dark` has the nine project colours mixed 85 % into `#26282C` (`#394773` → `#364268`),
  each below 2 : 1 against the window, width 600 and height 300, the same colour for the same
  folder; `Islands Light` has a gradient, `Ferris IDEA Dark`, `Ferris IDEA Light`, `Dark` and `Light`
  none (they keep the accent tint). On screen: next comparison run.
- **AK-v18 (behaviour 68)** `EmojiAndSymbols` is carried out (`docs/research/KEYMAP_COVERAGE.md`);
  the palette itself is a hand test on macOS (the test window has none).

## Out of scope
The AI stripe button (no such tool
window), inlay hints in the editor, the horizontal scrollbar, the contents of libraries (classes inside
jars), the "Extensions" folder under Scratches and Consoles, the tab list button of a header whose tabs
do not fit, dragging stripe buttons, (v5, v6) Options and Hide for the panels outside the tool window
layout (Find, Endpoints, Spring, AI Chat, Python Packages, Containers, Profiler, Remote Host, Find in
Files: they have no stripe button and no side to move to yet; v6 aligns their header metrics),
the KI stripe button; (v7) dragging a Float or Window window onto a stripe, the
workbench's own shortcuts (⌘1 …) while the keys are in a Float or Window window. (v10) Dragging a window from "…" onto a stripe. (v6 listed the
view modes, the split sides, "Remove from Sidebar" and "…" with Structure here as defect F087-1;
v7 specifies them in behaviours 37–43.)
(v14) In the Notifications window: links and actions inside a notification, marking notifications
as read, grouping them by day, the settings per kind of notification and balloons over the window.

## Known deviations from the reference
(v6) Bottom windows stack instead of replacing each other (IMP-0311; v10: they replace each other
as in the reference, behaviour 49, and ⌥-click opens a window in the other half of its side without
moving it, behaviour 50); the empty editor's lines are
centred in the editor area, not offset towards the window's centre and top (IMP-0312). The run
widget's more-actions menu keeps Ferris IDEA's entries: the reference's coverage run and parameter
dialog belong to the run configurations (F-077). The Settings menu lists only entries Ferris IDEA has.
(v6) The macOS window buttons were 23 px apart instead of 20 (GPUI's title bar; defect F087-2);
v7 sets the pitch itself (behaviour 44).
Only tool windows that exist get a stripe button (the reference shows all registered tool windows).
(v4) The header of behaviour 9 is built for Project and Terminal only. (v5) It is built for every
tool window of the layout except Problems, which keeps its own header row while its view is reworked
(F-032); v6 gives it the shared header (behaviour 23). The Debug toolbar stays a row below the
header, as OBS-0020 confirmed. (v6) The more-actions menu: see above; the Settings button opens a
menu (behaviour 28). External Libraries lists the declared dependencies (with locked versions), not the
jars of a resolved classpath. (v6) The header height 41 is confirmed by OBS-0020; the tree indentation
is 19 px as measured, not Jewel's 23. (v7) A Dock Unpinned window stays shown while the focus is in
another application (IMP-0314); a Float or Window frame off every display opens centred on the IDE
window (IMP-0313). An Undock window stays until hidden; whether the reference hides it on focus
loss was not observed. The More tool windows popup lists every existing tool window with its
shortcut; OBS-0020 records the button, not what the reference's popup lists. Hide All also hides
the former panels outside the layout, now tool windows. Bottom Left and Bottom Right parts each
stack their windows (IMP-0311; v10: one window per part).
(v10) ⌥-click shows a window in the other half of its side for as long as it is shown (IMP-0311); the
reference needs the window moved there. Showing a window off the sidebar gives it a button only while
it is shown and "Add to Sidebar" keeps it; whether the reference keeps the button of such a window
after it was opened was not observed. AI Chat takes the reference's AI place; Build (build tools)
stays on the right, where the reference keeps Maven and Gradle.

**(v11)** The More tool windows popup lists every window that can be opened, with its state, not only the ones without a stripe button as the reference does (IMP-0360).

**(v14)** The Notifications window lists the notes of the status bar, which are Ferris IDEA's
notifications; the reference shows balloons and lists those. The Database tool window keeps its own
icon (OBS-0046 A3): the reference's database tools are not open source.
