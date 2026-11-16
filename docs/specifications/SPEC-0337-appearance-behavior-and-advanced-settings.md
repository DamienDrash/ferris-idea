# SPEC-0337: Appearance & Behavior, System Settings, Notifications and Advanced Settings

- **Version:** 2 · **Date:** 2026-11-15 (v2: Advanced Settings with its own search and "Show modified only", behaviour 5b, OBS-0062 S5)
- **Sources:** OBS-0048 (outline of the settings dialog next to the reference: the reference groups
  Appearance, System Settings, Notifications, Path Variables and Trusted Locations under one
  group "Appearance & Behavior" and has a top-level page of advanced settings; IDECraft showed
  Appearance at the top without a group and Trusted Locations under Build, Execution,
  Deployment; the Appearance page of the reference chooses the editor colour scheme apart from
  the theme and can override the UI font, S1–S3); GEN (desktop IDEs offer, on a page of system
  settings: whether quitting asks first, whether the last project reopens at start-up and
  whether files are saved when the window loses the focus; a page of notification settings; a
  flat, searchable list of low-level options with their defaults and a reset); SPEC-0105
  (Settings dialog), SPEC-0129 (`settings.toml`), SPEC-0060 (themes and editor schemes),
  SPEC-0061 v17 behaviour 64 (Notifications tool window), SPEC-0094 (start-up folder),
  SPEC-0261 (quitting), SPEC-0278 (trust), SPEC-0319 behaviour 32, ADR-0008. Nothing of the
  pages' texts or layout was taken from the reference.
- **Feature matrix rows:** F-002

## Purpose
The settings dialog has the reference's outline: the pages about the IDE itself stand together
under Appearance & Behavior, and every low-level setting of `settings.toml` can be found, read,
changed and reset in the dialog, not only in the file.

## Behaviour
1. **Outline.** The page list starts with the group "Appearance & Behavior" holding, in this
   order, Appearance, System Settings, Notifications and Trusted Locations; then Keymap, the
   Editor group, Plugins, Version Control, Build, Execution, Deployment (Build Tools), Languages
   & Frameworks, Tools (Terminal, AI Assistant) and, last and without a group, Advanced
   Settings. Pages of a group stand indented under its name (SPEC-0105). Tab goes through the
   pages in this order. Trusted Locations moved from Build, Execution, Deployment into this
   group and is found by the search as before ("trust", "safe mode"); it keeps its behaviour
   (SPEC-0278 behaviour 8). [OBS-0048 S1, S2]
2. **System Settings** (`[system]` in `settings.toml`), each a switch that Enter, Space or a
   click changes, applied and saved at once:
   - **Confirm before quitting** (`confirm_quit`, off): Quit IDECraft from ⌘Q or the
     application menu with nothing unsaved and nothing running asks first, in the question
     drawn in the window in front (SPEC-0261 behaviour 5): "Quit IDECraft?" with Quit (the
     default, Return) and Cancel (Escape). Quit quits only if still nothing is pending;
     otherwise the window says what became pending and stays open. With pending work the
     question of SPEC-0261 behaviour 2 is asked as before, never two questions. `quit_ide`,
     `idecraft quit` and signals are not asked: they carry their own decision and right
     (SPEC-0261 behaviours 7, 11, SPEC-0262), so automation is never stopped by this question
     (IMP-0002). [GEN]
   - **Reopen the last project at start-up** (`reopen_last_project`, on): IDECraft started
     without a folder (Dock, Finder, `idecraft launch`) opens the most recent project that still
     exists, as before (SPEC-0094 behaviour 8). Off, it shows the folder chooser instead. A
     folder named on the command line opens either way. The project's trust is checked as for
     every opened project (SPEC-0278). [GEN]
   - **Save files when the window loses the focus** (`save_on_frame_deactivation`, off): when
     a project window stops being the active window (another application or another IDECraft
     window comes to the front), its editors with unsaved changes are saved as Save All saves
     them (the on-save edits of SPEC-0129 behaviour 4, local history, the changed-on-disk
     check; a remote project's files on the host, SPEC-0317 behaviour 10). A file that cannot
     be saved stays modified and is reported as Save All reports it. [GEN]
   Below the switches the page says that a project always opens in a window of its own
   (behaviour 8, deviations). [GEN]
3. **Notifications** (`[notifications]`): **Keep informational notes in the Notifications
   tool window** (`log_info`, on). Off, the Notifications tool window and the MCP tool
   `notifications` keep only failures; the status bar still shows every note. Failures are
   always kept, whatever the setting says — a setting never hides that something did not
   happen (security first). The page says so. [GEN, SPEC-0061 v17]
4. **Appearance: editor colour scheme** (`appearance.editor_scheme`, empty): a row "Editor
   color scheme" after the themes. Left/Right, Enter, Space or a click move through "Same as
   theme" and the bundled schemes IDECraft Dark, IDECraft Light, Islands Dark, Dark and Light
   (SPEC-0060 behaviour 1). Editors, consoles, diffs and the terminal draw with the chosen
   scheme at once in every window, the rest of the window with the theme's colours; "Same as
   theme" (the stored empty value) uses the theme's own scheme, as before. An unknown stored
   name counts as "Same as theme". The scheme stays when the theme changes, also with Sync
   with OS. [OBS-0048 S3, SPEC-0060]
5. **Advanced Settings.** A flat list of the settings of `settings.toml` that no other page
   shows. Each row has the label, the key (`project_view.compact_middle_packages`), the value,
   the default and one sentence of what it does; a row whose value differs from its default is
   marked "modified". The list holds:
   - `project_view.compact_middle_packages`, "Compact middle packages in the project tree"
     (switch, on; SPEC-0275 v10 behaviour 30);
   - `ai.attachment_chars`, "Largest chat attachment" (number of characters, 1 000–200 000 in
     steps of 1 000, default 24 000; SPEC-0313 behaviour 7);
   - `database.report_url`, "Address for database problem reports" (text: empty for
     IDECraft's issue template, `off` for copying only, else an `https://` address with
     `{title}` and `{body}`; SPEC-0301).
   Enter, Space or a click switch a switch; Left/Right change a number; Enter on a text row
   puts the focus in a field below the list, where Enter applies (an invalid address is refused
   with the reason and nothing changes), Escape and Tab leave. Delete, and ⌫ with the search
   empty, resets the selected row to its default; a "Reset" control on a modified row does the
   same. [GEN]
5a. **Search.** The dialog's search keeps Advanced Settings in the page list when a row's
   label, key or sentence contains the search text (case ignored), and the page then lists only
   those rows; with the search empty, or naming the page, it lists every row. The search finds
   the new pages of behaviours 2–4 by the labels of their settings (SPEC-0105 v5 behaviour 3b).
   [GEN]
5b. **Own search and modified only (v2, OBS-0062 S5).** Above its rows Advanced Settings has a
   search field of its own and a check box "Show modified only". The rows listed are those the
   dialog's search leaves (behaviour 5a) whose label, key or sentence also contains the page's
   search text (case ignored), and with the check box on only those changed from their default.
   Typing in the field narrows the list at once; Enter, Escape and Tab leave the field. Both are
   nodes of the semantic tree (`settings-advanced-search`, `settings-advanced-modified-only`).
   [OBS]
6. **Security first.** Advanced Settings lists no setting whose change weakens security: the
   SSH key exchange and ProxyCommand switches, MCP over HTTP, UI automation and trust stay on
   their own pages with their warnings and confirmations (SPEC-0150, SPEC-0221, SPEC-0319,
   SPEC-0278). None of the settings of this spec runs code of a project or opens a network
   path. [GEN, ADR-0025, CLAUDE.md rule 12]
7. **MCP.** `ide_settings` returns the new values under `settings.system`,
   `settings.notifications` and `settings.appearance.editor_scheme`, and the Advanced Settings
   rows under `advanced` (key, label, value, default, modified). `set_ide_settings` takes
   `confirm_quit`, `reopen_last_project`, `save_on_frame_deactivation`,
   `notifications_log_info`, `editor_scheme` (a bundled scheme's name, or empty for the
   theme's) and `advanced`, an object of Advanced Settings keys to values, `null` resetting a
   key. An unknown key, a value of the wrong kind or an invalid address is refused and nothing
   changes; `ai.*` keys are refused as every AI Assistant setting is (they are the user's,
   SPEC-0129 behaviour 7); numbers are clamped. Dry run and the `settings` right as before;
   no new tool. [ADR-0008]
8. **Saving.** The new values belong to the IDE layer (`settings.toml`) like the other pages'
   (SPEC-0129 behaviour 1): a value of the wrong kind costs its setting alone, every value
   survives a restart. [SPEC-0129]

## Acceptance criteria
- **AK-1** `ide_settings::tests::every_setting_survives_a_restart` covers the new settings;
  defaults (`system_and_notification_defaults`); the Advanced list: rows, values, defaults,
  set, reset, clamping, refused address and wrong kind (`advanced_settings::tests`).
- **AK-2** Outline: the pages in the order of behaviour 1 under their groups, Trusted
  Locations under Appearance & Behavior, the new pages found by their labels, Advanced
  Settings found by a key and its rows filtered (`settings_model::tests`).
- **AK-3** The question before quitting with nothing pending (`quit_model::tests::
  confirming_a_quit_with_nothing_pending`); the start folder with and without reopening
  (`launch::tests::start_folder_follows_reopen_last_project`).
- **AK-4** The editor scheme over a theme: the palette takes the scheme's editor colours and
  the theme's UI colours; "Same as theme" is the theme's own palette; every bundled scheme name
  is offered once (`palette::tests::editor_scheme_over_the_theme`).
- **AK-5** Window test (`crates/workbench/tests/it/settings_pages.rs`), with a settings file of
  its own: the page list starts with Appearance & Behavior and its four pages; Space on the
  System Settings rows switches and saves them; with "Confirm before quitting" on, Quit with
  nothing pending draws "Quit IDECraft?" and Cancel keeps the window; with "Save files when
  the window loses the focus" on, deactivating the window saves a modified file, off it stays
  modified; Notifications off keeps a failure but not an informational note; Advanced
  Settings: the search "packages" leaves one row, Space switches it, Delete resets it; an
  `http://` address is refused in the field and an `https://` one with `{title}` and `{body}`
  saved.
- **AK-6** MCP: `ide_settings` lists `advanced`; `set_ide_settings` sets the system and
  notification switches and the scheme, sets and resets an advanced key, refuses an unknown
  key, an `ai.*` key and an `http://` report address (`protocol.rs`: `ide_settings_tools`).
- **AK-7** Hand test, open: the pages next to the reference.
- **AK-8 (v2)** Unit test `settings_model::page_settings_tests::advanced_rows_search_and_modified_only`:
  nothing listed as modified at the defaults, one row after a change; the own search narrows the
  rows, a text that names no setting leaves none. In a window of the test harness
  (`crates/workbench/tests/settings_window_checks.rs`: `search_revert_defaults_font_and_advanced`):
  a click on "Show modified only" empties the list at the defaults and a second shows every row;
  "report" typed into the page's search narrows it.

## Out of scope
Path Variables: IDECraft resolves only `$PROJECT_DIR$` and `$MODULE_DIR$` when it imports
`.iml` files (SPEC-0049) and has no user-defined path variables in run configurations or paths;
the page comes with such a mechanism (planned). Autosave after a time without typing; the
default folder for new projects; the further pages of Appearance & Behavior (Menus and
Toolbars, File Colors, Scopes, Quick Lists, Data Views, Required Plugins, Presentation
Assistant); Settings Sync and backups; the further Appearance options of OBS-0048 S3 (a
different tool window background, UI zoom, screen reader support, high-contrast scroll bars,
colour-blindness adjustment).

## Known deviations from the reference
- **Saving on focus loss is off by default.** The reference saves when its window loses the
  focus unless switched off. IDECraft saves only on ⌘S, Save All and before quitting unless the
  user switches it on: a file written without being asked starts build watchers, dev servers
  and test runners on half-typed code, and an agent reading the file sees text the user has
  not finished. Unsaved text is never lost: quitting and closing a window ask (SPEC-0261,
  SPEC-0094 behaviour 7), and local history keeps saved states.
- **Confirm before quitting is off by default** (IMP-0002): IDECraft asks only when there is
  something to lose; the switch adds the question for the user's own quits, never for
  automation.
- **A project always opens in a window of its own.** The reference can open a project in the
  current window or ask; IDECraft opens every project in its own window and brings an open
  one to the front (SPEC-0094 behaviour 2).
- **Notifications** are the status bar's notes; IDECraft has no balloons, so the reference's
  per-group display choices (balloon, sticky, log only) and sounds have nothing to act on.
- **No UI font override.** The window's controls are laid out with fixed pixel sizes, so a
  different UI font family or size would cut or overlap labels; the option waits for the
  change to scalable metrics that UI zoom needs as well (OBS-0048).
- **Advanced Settings** lists IDECraft's own low-level settings, not the reference's.
