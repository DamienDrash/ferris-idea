# SPEC-0327: Run configuration editor

- **Version:** 4 · **Date:** 2026-11-14 (v4: the dialog is as high as its rows need and the form scrolls only
  when the window is too low, behaviour 20, AK-13; second window check of 2026-11-14; v3: folders are made, renamed and removed in the dialog and by agents,
  templates per configuration type, "Open browser" may wait for a port, controls with test ids and a window
  self-test; SPEC-0047 v4. v2: folders in the lists, "Allow multiple instances", before-launch tasks
  "Build", "Run external tool" and "Open browser"; SPEC-0047 v3; the parity note of F-077 in
  `docs/research/PARITY.md`)
- **Sources:** OBS-0032 (side by side with the reference: a dialog with the configurations in a tree by type, a toolbar
  to add, remove, copy and sort them, a type chooser with a typing filter, per type the program, arguments, working
  directory and environment, a name, before-launch tasks; temporary configurations from the current file that can be
  saved), GEN (run configuration dialogs in IDEs), OSS (action id `editRunConfigurations`), SPEC-0047 (format of
  `.idecraft/run.toml`), SPEC-0087 (run widget), SPEC-0278 (trust)
- **Feature matrix rows:** F-077

## Purpose
Create, change, copy and delete run configurations in the window, without writing `.idecraft/run.toml` by hand, and
keep a configuration made from the current file.

## Behaviour
1. **Open:** `editRunConfigurations` (Run › Edit Configurations…, the run widget's list, the chooser's entry 0 and a
   row's Edit…) opens the editor as a dialog drawn in the window. Opened from a row it selects that configuration.
   [OBS-0032, OSS]
2. **List:** the left side lists the configurations of `run.toml` in file order, then the temporary configurations
   (behaviour 9) under their own heading. The detected configurations (SPEC-0087 behaviour 8) are listed greyed under
   their heading and cannot be edited; Copy turns one into an editable configuration. A typing filter narrows the list
   by name. With nothing to show the list says so and offers Add. [OBS-0032]
3. **Toolbar:** Add (⌘N), Remove (⌫), Copy (⌘D), Move up/down (⌥↑/⌥↓, the file order is the list order). Add opens a
   chooser of templates with a typing filter: Command, Shell script, Node.js file, npm script, Python file, Cargo,
   Maven, Gradle, Go test, pytest. A template only fills the command; every configuration is the same form.
   [OBS-0032, own form]
4. **Form:** Name; Command line, edited as one line of text with shell quoting (`alpha "two words"` is two
   arguments) and split into the `command` list on save; Working directory (workspace-relative, `${workspace}` allowed);
   Environment as `NAME=value` lines; Kind (run or test, with Report for tests); Before launch: an ordered list of other
   configurations to run first, with add, remove and move; Timeout in seconds. The form shows the problems of
   SPEC-0047 behaviour 2 under the field they belong to, live while typing. [OBS-0032, SPEC-0047]
5. **Apply and OK:** Apply writes `run.toml`; OK writes and closes; Cancel and Escape close without writing and, if
   something was changed, ask first. A configuration with problems cannot be written; the problems stay visible and
   nothing else in the file changes. Writing keeps unknown keys, comments and the order of the other configurations
   as far as the TOML editor allows, and adds a Local History entry for `run.toml`. [GEN, IMP-0023]
6. **Run and Debug from the dialog:** Run and Debug apply first, then start the selected configuration and close.
   [OBS-0032]
7. **Changes from outside:** when `run.toml` changes on disk while the dialog is open, the dialog reloads unchanged
   entries and keeps the one being edited, marked as changed. [GEN]
8. **Untrusted projects:** in safe mode (SPEC-0278) the dialog opens read-only and says why; nothing is written or
   started. [SPEC-0278]
9. **Temporary configurations:** Run Current File (`RunClass`) and ▶ markers that start a file without a
   configuration create a temporary configuration named after the file, select it and run it. It lives for the
   session, at most five are kept (the oldest goes), and it is listed under the heading "Temporary". Save
   Configuration writes it to `run.toml` as a normal configuration (a name already used gets a number). [OBS-0032]
10. **Agents:** MCP `run_configuration_edit` with `add`, `update`, `copy`, `remove`, `move`, `save_temporary`, each
    under the `run` right of SPEC-0053, using the same validation and writer; dry run by default. [ADR-0008]

11. **v2 – Folders:** the form has a Folder field (SPEC-0047 v3 behaviour 7); typing a name puts the configuration
    into that folder, an empty field takes it out. The editor's list, the run widget's list and the chooser (⌃⌥R)
    show the configurations of `run.toml` without a folder first, then every folder as a heading with its
    configurations under it, folders in the order of their first configuration in the file; the typing filter and
    the digits of the chooser work on the rows as listed. Move up/down moves a configuration among those listed
    with it (its folder, or those without one) and changes the file order with it. A pinned configuration is listed
    under "Pinned" as before. Folders are headings, not rows: they cannot be collapsed, selected or run.
    [OBS-0032: the reference's dialog groups configurations and has a toolbar entry for folders; folders were not
    evaluated there, so their look here is our design]
12. **v2 – Instances:** the form has a switch between "One at a time" and "Allow multiple instances" (SPEC-0047 v3
    behaviour 10). Starting a configuration that runs and allows one instance — from Run, the chooser, a ▶ marker
    or the dialog's Run — shows a question with **Stop and Rerun** (Return) and **Cancel** (Escape). [OBS-0032]
13. **v2 – Before-launch tasks:** Before launch's Add offers, above the other configurations, **Build**, **Run
    external tool…** and **Open browser…**. Each adds a line to the form's "Before-launch tasks" field, which holds
    the tasks one per line in their order, in the same spirit as the Environment field: `build`, `tool
    [cwd=<folder>] <command line>` (shell-quoted like the command line) and `browser <URL>`; the lines for a tool
    and a browser are completed by typing. The field's problems (a line that is no task, a refused URL, a folder
    outside the workspace) are shown under Before launch, live. The tasks run after the configurations listed above
    them (SPEC-0047 v3 behaviour 8). [OBS-0032; the text form is our design: one form, readable by people and
    agents]
14. **v2 – Agents:** `run_configuration_edit` takes `folder`, `allow_multiple` and `before_launch` in `config` for
    `add` and `update`, checked and written like everything else. [ADR-0008]

15. **v3 – Folders in the dialog:** v2 let a folder come and go only with the text of the Folder field. Now:
    - **New Folder** (toolbar) makes a folder with a free name ("New Folder", "New Folder (2)", …) without
      configurations and shows its pane. A folder without configurations is listed in the dialog, marked as empty,
      is not something to write (the dialog is not changed by it) and ends with the dialog; it is written with
      the first configuration put into it (SPEC-0047 behaviour 7: a folder exists as the key of its
      configurations).
    - A folder is a row of the list: a click shows the **folder pane** instead of a form, with the folder's name
      in a field, **Rename** (also Return in the field), the names of its configurations and **Remove Folder**
      (also ⌫ while the folder is shown). Rename is refused, with the reason under the field while typing, for an
      empty name, a name SPEC-0047 behaviour 7 refuses and the name of another folder. Remove Folder takes no
      configuration away: they are listed on their own again.
    - **Moving:** under the Folder field of a configuration of `run.toml` stand "No folder" and every folder of
      the dialog; a click puts the configuration into that folder or takes it out. Typing a name in the field
      still works and makes a folder that is new.
    - The list shows the configurations without a folder, then each folder with its configurations indented under
      it (folders in the order of their first configuration, empty ones last), then Temporary and Detected. With a
      typing filter a folder is shown only above a configuration the filter keeps.
    - Nothing of this is written before Apply or OK, and in safe mode (behaviour 8) the controls do nothing.
    [OBS-0032: the reference's dialog has a toolbar entry for folders; folders were not evaluated there, so the
    pane, the chips and the empty folder are our design; SPEC-0047 v4 behaviour 14]
16. **v3 – Templates per type:** **Edit templates per type…** below the list exchanges the list of configurations
    for the list of the types of behaviour 3; **Configurations** leads back. A type with a template is listed in the normal colour,
    one without greyed; a changed one carries the mark of a changed configuration, one with problems the error
    sign.
    - The form of a type's template has Interpreter and Options (not for the type Command, which has no program),
      Working directory, Environment, Instances, Before launch with its list of configurations and its tasks field,
      and Timeout — the same fields as a configuration's form, with the same live problems under them (SPEC-0047
      v4 behaviour 12). **Reset** empties the template.
    - Add (behaviour 3) makes the new configuration as a copy of the type's template as it stands in the dialog at
      that moment, also before Apply. Without a template the type fills the command only, as before.
    - Apply and OK write the changed templates together with the configurations to `run.toml`; a template with
      problems keeps everything from being written, and the problems are listed with the type's name. Cancel asks
      first when a template was changed. The template of a type that was not changed is not touched in the file.
    - Problems of the templates as read from the file (an unknown type, an unknown key) are listed under the
      types.
    - Run and Debug of the dialog need a configuration: with the templates or a folder shown they say so.
    [OBS-0032 names the entry for templates, not evaluated; GEN; the form is our design: one form for every type]
17. **v3 – Agents:** `run_configuration_edit` gains the operations `set_folder` (`name`, `folder`; without `folder`
    the configuration leaves its folder), `rename_folder` (`folder`, `new_name`), `remove_folder` (`folder`) and
    `set_template` (`type`, `template`; an empty or missing template removes it), and `add` takes `type`: the new
    configuration is the type's command with a copy of the type's template, changed by the keys `config` names
    (`config` may be left out; the name is then the type's label with a free number). Everything goes through the
    same checks and the same writer as the dialog, is a dry run unless `dry_run` is false, and writes only with
    the rights `run` and `write` in a trusted project, as before. `before_launch` takes `wait_port` with
    `browser`. [ADR-0008, SPEC-0053]
18. **v3 – Before-launch task line:** the line of an "Open browser" task may be `browser wait=<port> <URL>`
    (SPEC-0047 v4 behaviour 13); a `wait=` that is no port is a problem of Before launch, shown live.
    [own form]
19. **v3 – Controls for automation:** the rows of the list (configurations, folders, types), the entries of the
    type chooser and of Before launch's Add, the folder chips, the instance switch and every button this version
    adds describe themselves in the window's semantic tree with a role, a name and a test id (SPEC-0319 behaviour
    1), so that the UI driver and the window self-test operate them by clicks. So that a click reaches every
    control without scrolling (the driver of a real window cannot scroll a list yet), the form has two columns
    below the command line: working directory, environment, report and timeout on the left; kind, instances and
    Before launch on the right. [SPEC-0319; layout: own design]
20. **v4 – Every row in view (second window check 2026-11-14):** the dialog is 580 px high and grows with the
    form: when the rows of the selected configuration, template or folder need more, the dialog gets as high as
    they need, so that nothing is cut at its lower edge. It ends 16 px above the lower edge of the window at the
    latest (its top is 40 px under the main toolbar); only then the form scrolls, and the list of configurations
    scrolls as before. The title, the row of buttons (Run, Debug, Cancel, Apply, OK) and the problems of the last
    Apply are outside the scrolling part and always in view, also in a window lower than 580 px. Until v4 the
    dialog was 580 px high whatever it showed: with v3's fields the hint lines at the end of both columns (under
    Timeout and under Before launch) were cut at its edge without a sign that the form scrolls, and in a low
    window the buttons were outside the window. [GEN; layout: own design]

## Acceptance criteria
- **AK-1** Command line text round-trips through the quoting: `alpha "two words" 'it''s'` gives three arguments and
  shows back the same arguments (unit test of the splitter and joiner).
- **AK-2** Writing keeps comments and unknown keys of other configurations, rejects a configuration with problems
  without changing the file, and adds a Local History entry (unit tests with a fixture `run.toml`).
- **AK-3** Templates fill the command for each listed type (unit test).
- **AK-4** Temporary configurations: Run Current File creates, selects and runs one; at most five; Save Configuration
  writes it with a free name (unit tests of the model; window check).
- **AK-5** In a real window: open the editor from the run widget, add a Node.js file configuration with arguments with
  a space and an environment variable, OK, Run: the program receives the two arguments and the variable (selftest step
  and window check).
- **AK-6** MCP `run_configuration_edit` performs each operation with dry run by default and refuses without the `run`
  right (unit tests).

- **AK-7** v2 – The form reads and writes folder, instances and tasks (round trip, a line that is no task and a
  refused URL as problems of Before launch); the editor's list and the chooser's rows group by folder with the
  headings and the order of behaviour 11, Move stays within the folder; the writer keeps comments and writes the
  three keys only when set (unit tests). Window check of the dialog, the question and the instance tabs: open.
- **AK-8** v2 – `run_configuration_edit` adds and updates a configuration with a folder, the instance switch and
  the three kinds of tasks, and refuses a task with a URL scheme that is not allowed without changing the file
  (protocol test).

- **AK-9** v3 – Folders in the list model: the rows with folders and headings, New Folder (free names, not a
  change), Rename (refusals, members follow), moving in and out, the filter, Remove Folder (members stay), and
  what Apply writes (unit test `folders_are_made_renamed_and_removed`).
- **AK-10** v3 – Templates in the model: the form of a template reads and writes every key, a new configuration
  copies the template as it stands and is not reached by later changes, problems under their field, only changed
  types are written, a reload from disk keeps the template being edited (unit test
  `templates_are_edited_and_copied_into_new_configurations`).
- **AK-11** v3 – `run_configuration_edit`: `set_folder`, `rename_folder`, `remove_folder`, `set_template` and
  `add` with `type` as dry runs and as writes; refused without changing the file: a template with a refused URL,
  an unknown before-launch configuration, an interpreter for the type Command, an unknown type, an unknown key, a
  folder that is not there, a folder name that is none; writing refused without the rights; `run_configurations`
  lists folders, types, templates and template problems (protocol test).
- **AK-12** v3 – By clicks on the controls, found by their test ids: in a window of the test harness New Folder,
  Rename, the folder chips, Remove Folder and Apply with the file's comment kept; Templates, a type, its fields,
  the instance switch, Before launch's Add with a configuration and Build, Apply, Add of that type as a copy,
  Reset (`run_config_folders_templates`). In a real window (`idecraft ui --selftest` with
  `IDECRAFT_SELFTEST_DRIVER=1`, groups `run-config-folders`, `run-config-templates`, `run-config-before-launch`):
  the same through the run widget's list, the folder heading in the run widget's list, a configuration made from
  a template runs with the template's environment, Build added from the picker, a refused URL named and not
  written.
- **AK-13** v4 – In a window of the test harness (`window_check_1114b.rs::the_run_configuration_editor_shows_every_row`):
  in a window of 1372 × 885 the form of a configuration is shown whole (its content is not higher than the part
  in view); in a window 560 px high the form scrolls and Run, Cancel and OK are inside the window. The heights
  for a room (unit test `run_config_dialog::tests::the_dialog_is_as_high_as_the_window_allows`).

## Out of scope
A form per type (the single form is deliberate), sharing switch (configurations
are always the project file; personal configurations and templates outside the project are a later step, SPEC-0047
v4), the reference's XML.
v2: nested folders, collapsing a folder, dragging configurations between folders,
pickers for a tool's program or a page. v3: moving a whole folder up or down (a folder stands where its first
configuration stands), choosing several configurations at once for a folder, typing text into the dialog's fields
through the UI driver (the fields are not yet in the semantic tree; the self-test sets their text).

## Known deviations from the reference
- One form for every configuration instead of a different form per type; a type fills the command and its
  template (v3) the fields every configuration has. Every configuration and every template can therefore be read
  and changed the same way, by people and agents.
- v3: a folder is the key of its configurations, so a folder without configurations is not kept after the dialog
  closes; the dialog says so in the folder's pane.
- A configuration that cannot start always says why (IMP-0394); the reference creates a temporary configuration that
  silently does not start.
- Detected configurations need no dialog; the reference suggests nothing from `package.json` until a file is opened.
