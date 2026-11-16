# SPEC-0333: Named tool window layouts

- **Version:** 1 · **Date:** 2026-11-13
- **Sources:** OSS — the open platform's layout actions, `JetBrains/intellij-community` at the
  pinned commit `d4018fb` of APACHE_PORTING.md (PROVENANCE.md: logic template, no code):
  `platform/platform-impl/src/com/intellij/ide/actions/StoreNewLayoutAction.kt` (a new layout is
  saved under a name and becomes the selected one), `StoreNamedLayoutAction.kt` and
  `StoreDefaultLayoutAction.java` (saving the current arrangement into the selected layout, not
  into the default one), `RestoreDefaultLayoutAction.java` (the selected layout is applied again as
  it was saved), `RestoreFactoryDefaultLayoutAction.kt` (the default layout is selected and
  applied), `CustomLayoutsActionGroup.kt` (what is offered per layout: for the selected one restore
  and save, for the others apply, for both rename and delete), `RenameLayoutAction.kt`,
  `DeleteNamedLayoutAction.kt` (asks first; the selected layout cannot be deleted),
  `LayoutNameInputDialog.kt` (a name is not blank, not longer than a limit and not taken),
  `platform/platform-impl/src/com/intellij/toolWindow/ToolWindowDefaultLayoutManager.kt` (the
  layouts and the selected name are kept for the user, not per project; what a stored layout holds
  per tool window; the selected layout follows a rename), the action ids and their order in the
  layouts group of `platform/platform-impl/resources/idea/PlatformActions.xml`, and the action
  texts in `platform/platform-resources-en/src/messages/ActionsBundle.properties`. GEN (a saved
  arrangement is applied as a whole; a name is trimmed; a broken settings file never stops the
  program). SPEC-0167 (the tool window layout and its file), SPEC-0061 (placements, sizes, view
  modes), ADR-0021 (state files of the user are JSON with a version), ADR-0008 (MCP), SPEC-0053
  (agent rights), SPEC-0281 (dialog decisions), SPEC-0319 (effects of actions).
- **Feature matrix rows:** F-087

## Purpose
The user saves the current arrangement of the tool windows under a name, switches between saved
arrangements, restores the selected one after rearranging, and renames or deletes them. The default
layout always stays.

## Behaviour

### What a layout is
1. **Arrangement.** A layout holds, for every tool window, its side and half, its order there, the
   size the user gave it, its view mode, whether it has a stripe button and its last Float or
   Window frame (all as SPEC-0167 behaviour 8 saves them), whether the stripes are shown, which
   tool windows are shown, and which tool window had the keyboard focus. It holds nothing of the
   editor area: no split, no tabs, no open files; and nothing a tool window shows. [OSS, GEN]
2. **The default layout** always exists and is titled "Default". It is the arrangement of
   SPEC-0167 behaviours 1 and 14 and says nothing about which windows are shown. It cannot be
   saved over, renamed or deleted, and no saved layout can be named "Default" (in any case).
   [OSS, GEN]
3. **The selected layout.** Exactly one layout is selected: the default one until the user selects
   or saves another. The selection and the saved layouts belong to the user, not to a project:
   every project window offers the same layouts. Rearranging tool windows changes the window and
   the project's own layout file (SPEC-0167 behaviour 8), never a saved layout. [OSS]

### Actions
4. **Save Current Layout as New… (`StoreNewLayout`)** asks for a name and saves the arrangement
   the window had when the action started under it. The new layout becomes the selected one.
   Escape saves nothing. At most 50 layouts are kept; a further one is refused with the reason.
   [OSS; GEN: the limit]
5. **Names.** A name is taken without the white space around it. It has 1 to 64 characters, holds
   no `/`, no `\` and no control character (line break, tab), is not "Default" and differs from
   every other saved layout's name when capitals are ignored. A name that breaks a rule keeps the
   name field open and says which rule. [GEN; OSS: not blank, a length limit, not taken]
6. **Apply** (a saved layout that is not selected) selects the layout and arranges the window by
   it: every tool window goes to its saved side, half, order, size, view mode and stripe button,
   the stripes are shown or hidden, the saved shown windows are shown and every other tool window
   is hidden, and the keyboard focus goes into the saved focused window when it is shown, else
   into the editor. A saved shown window that cannot be shown now (Run or Debug without a session,
   Find without a search, Commit outside a repository) stays hidden. Tool windows a saved layout
   does not name take their default placement. The project's layout file is saved. [OSS, GEN]
7. **Default (`RestoreFactoryDefaultLayout`)** selects the default layout and arranges the window
   as Restore Default Layout did (SPEC-0167 v5 behaviour 17): default sides, halves and order, no
   own sizes, view modes, removed buttons or saved frames, stripes shown; the shown windows stay,
   one per placement. [OSS]
8. **Restore Current Layout (`RestoreDefaultLayout`, ⇧F12)** arranges the window by the selected
   layout as it was saved, dropping what was rearranged since: behaviour 6 for a saved layout,
   behaviour 7 while the default one is selected. This replaces the title and, while a saved
   layout is selected, the effect of SPEC-0167 v5 behaviour 17. [OSS]
9. **Save Changes in Current Layout (`StoreDefaultLayout`)** saves the window's arrangement into
   the selected layout. While the default layout is selected it is not offered and, run from
   elsewhere, says so and saves nothing. [OSS]
10. **Rename…** asks for the new name in a field that starts with the old one; the rules of
    behaviour 5 apply, and the layout's own name in other capitals is allowed. The selected layout
    stays selected under its new name. Escape, or the unchanged name, renames nothing. [OSS]
11. **Delete…** asks "Delete layout "…"?" with Delete and Cancel; Cancel is the default answer.
    The selected layout cannot be deleted: its Delete… is not available, and another layout has to
    be selected first. [OSS]

### Where the actions are
12. **Window menu.** The Window menu has a Layouts submenu where Restore Default Layout stood. It
    offers Default, with a ✓ before the title while the default layout is selected; then the
    saved layouts by name (capitals ignored), each with its own submenu — for the selected one,
    which has a ✓ before its name, Restore, Save Changes and Rename…; for every other one Apply,
    Rename… and Delete…; then Restore Current Layout, Save Changes in Current Layout and Save
    Current Layout as New…. What is not available is left out: Delete… of the selected layout,
    and Save Changes in Current Layout while the default layout is selected. The submenu shows
    the saved layouts as they were when IDECraft started or last changed them in this process.
    [OSS: ids, texts and order of the layouts group; GEN]
13. **Find Action** finds Restore Current Layout, Save Changes in Current Layout, Save Current
    Layout as New… and the default layout as "Restore Default Layout", all in the group Layouts.
    The per-layout entries are in the menu only. [OSS, GEN]
14. **Effects for external driving (SPEC-0319 behaviour 35).** The default layout shows no
    window that was hidden and changes nothing outside the window (`none`, as hiding tool
    windows). Applying a saved layout, and Restore Current Layout, can show the Terminal, which
    starts a shell: they need `run`. Saving, renaming and deleting a layout write the user's
    settings and need `write`. [GEN]

### Storage
15. **File.** The layouts are kept in `window-layouts.json` in the user's settings folder
    (SPEC-0053 behaviour 2: `$IDECRAFT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/idecraft`, else
    `~/.config/idecraft`), written whole and atomically after every change and read again before
    every action, so two windows do not overwrite each other's layouts. [ADR-0021, GEN]
16. **Format (version 1).** A JSON object:
    - `version` — `1`.
    - `active` — the selected layout's name; missing, or naming no saved layout, means the
      default layout.
    - `layouts` — an array of objects, each with
      - `name` — the name (behaviour 5);
      - `layout` — the arrangement in the form of `.idecraft/tool-windows.json` (SPEC-0167
        behaviour 8): `version`, `stripes` and `windows`, each window with `id`, `side`
        (`left`, `bottom`, `right`) and, where they apply, `size`, `split`, `mode`, `removed`
        and `bounds`;
      - `shown` — the ids of the shown tool windows;
      - `active_window` — the id of the tool window that had the focus, if one had.

    The schema is published as `docs/formats/window-layouts.schema.json`; a test keeps it equal
    to the one the code gives. [ADR-0021, SPEC-0167]
17. **Reading.** Unknown tool window ids in `windows`, `shown` and `active_window` are ignored;
    a window `windows` does not list takes its default placement; an `active_window` that is not
    in `shown` is dropped. An entry without a valid name, or with a name an earlier entry has, is
    skipped. Unknown keys are ignored. A missing file means no saved layouts. [GEN]
18. **A file that cannot be read** — no JSON, not an object, a `layouts` that is no array, or a
    `version` above 1 — leaves the default layout selected and no saved layouts; the window says
    so in the status line when a layout action runs, and nothing crashes. A file of a newer
    version is never overwritten: saving, renaming and deleting are refused with the reason. A
    broken file is kept once as `window-layouts.json.bak` before the next save replaces it. [GEN]

### MCP
19. **`window_layouts`** [ADR-0008]:
    - `action: "list"` (the default) returns `file`, `selected` (the name, `Default` for the
      default layout), `layouts` (each with `name`, `selected`, `shown`, `active_window`,
      `stripes` and `windows` with `id`, `side`, `placement`, `order`, `size`, `mode`,
      `removed`), `skipped` (entries the file holds but behaviour 17 skips), `problem` (behaviour
      18) and `limits` (name length, number of layouts).
    - `action: "save"` with `name` saves the workspace's saved tool window layout
      (`.idecraft/tool-windows.json`) with `shown` (ids, default none) and `active_window` as a
      new layout and selects it; without `name` it saves them into the selected layout.
    - `action: "apply"` with `name` (`Default` for the default layout) selects the layout and
      writes its arrangement to the workspace's layout file; `action: "restore"` does so for the
      selected layout. The result names `shown` and `active_window`; a running window uses the
      file at its next start (as `tool_windows`, SPEC-0167 behaviour 9).
    - `action: "rename"` with `name` and `new_name`; `action: "delete"` with `name`.
    - Every action but `list` takes `dry_run`, which defaults to true: the result is the list as
      it is, with what would change in `would`, and nothing is written. With `dry_run: false`
      the call needs the `write` right (SPEC-0053), also for the user's file; the result is the
      list afterwards, with what changed in `done`, and the call is written to the agent log.
      Names are checked as in behaviour 5, and a refused call says which rule it broke. A file
      of a newer version refuses every action but `list`. [ADR-0008, SPEC-0053]
20. **Dialog decisions (SPEC-0281).** The name field of Save Current Layout as New… and Rename…
    and the Delete question are in the list of `dialog_decisions`, each answer with
    `window_layouts` and its arguments. The Delete question is drawn in the window. [SPEC-0281]

## Acceptance criteria
- **AK-1** Model: save as new (selected afterwards), save changes (refused for the default
  layout), apply and restore give back the saved arrangement, rename keeps the selection, delete
  is refused for the selected layout and for the default one, the limit of 50 (unit tests
  `window_layouts::tests`).
- **AK-2** Names: trimmed; empty, 65 characters, `/`, `\`, a line break, "default" and a taken
  name in other capitals are refused, each with its reason; 64 characters and the layout's own
  name in other capitals on rename are accepted (`window_layouts::tests`).
- **AK-3** File: what is written is read back equal; unknown ids, an `active_window` that is not
  shown, entries with broken or repeated names and unknown keys are handled as behaviour 17 says;
  no JSON, a wrong shape and a newer version give the default layout with a problem; a newer
  file is not overwritten and a broken one is kept as `.bak` (`window_layouts::tests`).
- **AK-4** The published schema equals the one the code gives
  (`window_layouts::tests::published_schema_matches`).
- **AK-5** Menu: the entries of behaviour 12 for no layouts, a selected saved layout and several
  layouts, with ticks and availability (`window_layouts::tests::menu_*`); what the menu bar
  makes of them (`menu_bar::tests::the_layouts_submenu_lists_the_saved_layouts`); the static
  part and the Find Action titles (`menus::tests`, `everywhere::tests`).
- **AK-6** Effects: `security::tests::layout_actions_declare_their_effects`.
- **AK-7** MCP: list, save with a dry run and for real, apply writing the workspace's layout file,
  restore, rename, delete, the refusals of behaviours 5, 9 and 11, and the `write` right
  (`protocol.rs`: `window_layouts_tool`, `window_layouts_need_the_write_right`).
- **AK-8** Window (harness, nothing drawn on a screen): Save Current Layout as New… with its name
  field, rearranging, Restore Current Layout, Default, Apply, Rename…, Delete… with its question,
  and a broken file (`crates/workbench/tests/it/window_layouts.rs`).
- **AK-9** Hand test (open): the Layouts submenu in the menu bar of the running application, its
  ticks and unavailable items, next to the reference.

## Out of scope
Layouts per project; exporting and importing layouts; a layout switcher in a toolbar or popup;
the editor area (splits and tabs); the content, tabs and options of the tool windows themselves.

## Known deviations from the reference
- The open platform keeps the layouts in an XML file of its settings and starts with a saved
  layout named "Custom" selected; IDECraft keeps a JSON file and starts with the default layout
  selected and no saved layout.
- Names: the open platform allows 50 characters, compares names exactly and trims nothing;
  IDECraft allows 64, ignores capitals when comparing, trims, and refuses path separators,
  control characters and "Default", so that a name is safe wherever it is shown or passed on.
- The open platform also stores per window whether it is maximised, the width shared by a side
  and the tab style of a window's content; IDECraft has no saved state for these and does not
  store them.
- The default layout of the open platform says which windows are shown; IDECraft's keeps the
  shown windows (SPEC-0167 behaviour 17).
- In the main menu the open platform offers Restore and Save Changes only inside the selected
  layout's own submenu; IDECraft also offers them in the Layouts submenu itself, so Restore
  Current Layout is reachable while the default layout is selected.
- The toolkit's menu bar can neither tick nor grey out an item. The selected layout and the
  selected Default have a ✓ before the title instead, and an item that is not available is left
  out instead of shown greyed out.
- A second IDECraft process, or another project window, shows saved layouts another one changed
  only after its own next layout action or a restart; the actions themselves always work on the
  file as it is.
- The MCP tool changes the saved files, not a running window (as `tool_windows`).
- Not compared side by side with the reference: the behaviour comes from the open sources named
  above.
