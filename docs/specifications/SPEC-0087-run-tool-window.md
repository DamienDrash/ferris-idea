# SPEC-0087: Run widget and Run tool window

- **Version:** 11 · **Date:** 2026-11-19 (v11: a run of an HTTP request configuration (SPEC-0047 v6 behaviour 17)
  shows the requests and responses in its tab, behaviour 19, AK-13; no open pull request changed this spec when v11
  was written); 2026-11-14 (v10: Send EOF (`SendEOF`) ends the input of the program of the selected tab; a program started for the Run tool window has an open standard input for it, behaviour 18, AK-12; seventh package of docs/research/KEYMAP_COVERAGE.md; v9: instances and folders in the Run tool window and the run widget's list, checked in the window; v8: the chooser opened for debugging, the keymap's `ChooseDebugConfiguration`; v7: typing filters the rows of the list; v6: list entries for editing and the current file, row actions, temporary configurations, run window toolbar, quoted command line, a start that cannot happen says why; v5: modified documents are saved before anything starts; v4: Stop asks the program to end first; v2: configurations detected from build files; v3: the run
  widget's list only selects)
- **Sources:** GEN (a run widget in the main toolbar with the selected configuration, Run and Stop; a Run
  tool window with one tab per started configuration, live output with errors in the error colour and a
  closing line with the exit code; F-077), OSS (action ids `Run` ⌃R, `ChooseRunConfiguration` ⌃⌥R,
  `Stop` ⌘F2, `ActivateRunToolWindow` ⌘4 in the macOS keymap; ⇧F10, ⌥⇧F10, ⌃F2, Alt+4 in the default
  one; colour keys `CONSOLE_NORMAL_OUTPUT`, `CONSOLE_ERROR_OUTPUT`, `CONSOLE_SYSTEM_OUTPUT`), SPEC-0047 (run
  configurations), SPEC-0045 (test reports), OBS-0032 (side by side, v6), SPEC-0327 (configuration editor)
- **Feature matrix rows:** F-077

## Purpose
Start the project's programs and tests from the window and watch them run.

## Behaviour
1. **Widget:** the main toolbar shows the selected configuration of `.ferris-idea/run.toml` (the first one
   at the start), a Run button and, while it runs, a Stop button. Without configurations it reads
   "Add Configuration…" and Run explains in the status bar where they go; a file with problems names the
   first problem. [GEN]
2. **Choose:** `ChooseRunConfiguration` or a click on the name opens a popup listing the configurations
   with their command; ↑/↓ choose, Enter selects and runs, Escape closes. Opened from the run
   widget the list only selects: Enter or a click makes the configuration the selected one without
   starting it (v3, hand test 47, D23). [GEN, OSS]
3. **Run:** `Run` runs the selected configuration with its `before` chain (SPEC-0047 behaviour 4) in the
   background and opens the Run tool window at the bottom with a tab for it; running it again while it
   runs stops the old run first. [GEN, OSS]
3a. **v5 – Saved first:** a run, a debug session (also Rerun in the Debug window), ⌃⇧R/⌃⇧D, a ▶ marker,
   a build tool task and a run on a remote project's host write all modified documents first, because
   what starts reads the files, not the editors. A document that cannot be saved stops the start, and
   the status bar names it. [GEN, OBS-0028 window check 2026-11-09]
4. **Output:** each step starts with its command line in the system-output colour, then stdout and
   stderr as they arrive (stderr in the error colour), then "Process finished with exit code N" — or,
   for a stopped or timed-out process, "Process finished with exit code N (stopped)" or "(timed out)".
   A failed `before` step ends the chain with "Run stopped: <name> failed". A test configuration adds
   "Tests passed: P, failed: F, skipped: S" from its report. At most 10 000 lines are kept; older lines
   are dropped from the top. [GEN]
5. **Stop:** `Stop` or the Stop button ends the running process together with its children (its process
   group): SIGTERM first, so that the program can clean up, SIGKILL for what still runs after 3 s.
   A program ended by a signal reports 128 + the signal (143, or 137 after the SIGKILL). A timeout
   and quitting keep their own rules. [GEN, OSS]
6. **Tool window:** `ActivateRunToolWindow` shows or hides it; ⇧Esc hides it; one tab per configuration
   that was run, the newest selected; a tab's × closes it (stopping its run). The tab of a running
   configuration shows a running marker. [GEN, OSS]
7. Agents keep using `run_configurations` and `run_configuration` (SPEC-0047, SPEC-0053). [ADR-0008]
8. **v2 – Detected configurations:** after the configurations of `run.toml` come those that fit the
   workspace's build files, as the project model suggests them (SPEC-0048 behaviour 6):
   - `cargo build` and `cargo test` for Cargo
   - one `npm run <script>` per script of `package.json`
   - `go test ./...` for Go
   - `mvn test` for Maven
   - `pytest` for Python projects that depend on it

   The chooser lists them under the heading "Detected from build files". They run, debug and stop like
   the others. A name already used in `run.toml` keeps the `run.toml` configuration. Without `run.toml`
   the first detected configuration is selected, so the widget can run something at once. [GEN]

9. **v6 – List entries:** the run widget's list and the chooser end with "Current File" (selects running the file of
   the focused editor, as `RunClass`) and "Edit Configurations…" (SPEC-0327). The chooser numbers its first ten rows
   (0 = Edit Configurations…, then 1–9) so that a digit picks a row, and its footer says that holding ⇧ debugs.
   [OBS-0032]
10. **v6 – Row actions:** hovering or selecting a row shows Run, Debug and a ⋮ menu with Edit…, Delete (with a
    confirmation naming the configuration; it removes it from `run.toml`), Pin/Unpin and, for a temporary
    configuration, Save Configuration (SPEC-0327 behaviour 9). Pinned configurations come first under "Pinned"; the
    pins are kept in the project's workspace state, not in `run.toml`. Detected configurations offer Run, Debug and
    Copy to run.toml. [OBS-0032]
11. **v6 – Temporary heading:** a configuration made by Run Current File is listed under "Temporary", never under
    "Detected from build files". [OBS-0032 L3]
12. **v6 – Run window toolbar:** Rerun (⌘R, also while running: stops first), Stop, Scroll to End (on while new lines
    follow), Soft-Wrap, Clear (empties the console of that tab, not the run), and a ⋮ with Copy Command Line and Open
    Run Configuration. [OBS-0032 L5]
13. **v6 – Command line shown quoted:** the first line of a step shows the command shell-quoted, so that copying it
    into a terminal runs the same program with the same arguments (`node src/main.js alpha 'two words'`). [IMP-0395]
14. **v6 – A start that cannot happen says why:** a configuration that cannot start (problems in `run.toml`, a
    program that is not found, safe mode, a missing file) never starts silently: the status bar and a line in the Run
    window name the reason, and the list marks the configuration with a warning sign whose hint is the reason.
    [IMP-0394]
15. **v7 – Typing filters the rows:** in the chooser and in the run widget's list, typed text narrows the rows to
    those whose label contains it, ignoring case, in their order; the text is shown beside the title "Run
    Configurations". "Current File" and "Edit Configurations…" are rows like the others and are found by their text.
    A heading (Pinned, a folder of SPEC-0327 behaviour 11, Temporary, Detected from build files, a plugin's) stays
    above the first row left of its group and disappears with its last row. The highlight goes to the first row whose
    label starts with the text, else to the first row left; Enter chooses the highlighted row as before (⇧ debugs),
    and with no row left the list says "No matching configuration" and Enter does nothing. Backspace removes the last
    typed character. Escape first clears the text and closes the list only when there is none. The digits of
    behaviour 9 pick a row while nothing is typed; after the first character a digit is part of the text (names such
    as `test2`), and the rows show no digits then. Found in the window on 2026-11-13: typing did nothing, unlike in
    the other lists of Ferris IDEA that filter (SPEC-0122 behaviour 14). [GEN]
16. **v8 – The chooser for debugging:** `ChooseDebugConfiguration` (⌃⌥D in the macOS keymap, Alt+Shift+F9 in the
    default one; Run › Debug…, Find Action "Debug…") opens the chooser of behaviours 2 and 9 with the two ways of
    starting exchanged: Enter, a click and a digit debug the chosen configuration (or the current file), and
    holding ⇧ runs it. The footer says that holding ⇧ runs. Everything else is the chooser as it is: the rows, the
    headings, the row actions Run and Debug (each does what its name says), the ⋮ menu, typing (behaviour 15),
    "Edit Configurations…" and Escape. The run widget's list (behaviour 3 of v3) is not affected; it only selects.
    [OSS: `ChooseDebugConfigurationPopupAction` of intellij-community is the Run chooser with the default and the
    alternative way of starting exchanged; the action id and its shortcuts in the bundled keymaps]

17. **v9 – Instances and folders in the window:** what SPEC-0047 behaviour 10 and SPEC-0327 behaviours 11 and 15
    rule is shown here as follows.
    - **One instance:** Run on a configuration that runs and allows one instance shows the question drawn in the
      window, with the Run tool window shown behind it. **Cancel** (Escape) changes nothing: the tab keeps its
      run and its output. **Stop and Rerun** (Return) stops the tab's run as Stop does and starts the
      configuration again in the same tab, whose console starts anew.
    - **Several instances:** with "Allow multiple instances" a further start opens a further tab titled with the
      name and a counter (`name (2)`, `name (3)`, …; the first free one among the tabs whose run still goes), each
      with its own console, Stop and Rerun. No question is asked.
    - **Folders:** the run widget's list and the chooser show a configuration of a folder under the folder's
      heading; a folder made, renamed or removed in the editor is there, under its new name or gone, the next time
      the list opens.
    - **Before launch:** the tab of a configuration with tasks shows the steps in the order they ran, each
      starting with its command line (behaviour 4); a failing step ends with "Run stopped: <step> failed", where
      the step of an external tool is named `<configuration> · <program>`; the line of an opened page, or why it
      was not opened, stands among the output.
    [SPEC-0047 v3/v4, SPEC-0327 v2/v3; OBS-0032 for the switch and the groups]

18. **v10 – Send EOF:** `SendEOF` (⌘D in the bundled macOS keymap, Ctrl+D in the default one; Find Action "Send
    EOF") sends the end of input to the program of the selected tab of the Run tool window.
    - **The input of a program.** A program started for the Run tool window (Run, Rerun, the chooser, a tab's
      toolbar; every step of a chain with tasks before launch) gets a standard input that stays open and that
      nothing is written to. Before v10 a program had no input at all and read its end at once. A program that
      reads its input therefore waits now, as it does in a terminal, until Send EOF, Stop or the configuration's
      timeout ends the wait. This also holds for a tool that reads its input only when it is given one instead
      of a terminal (some search and filter tools do).
    - **The action.** It closes that input: the program reads the end of its input and goes on by itself; it
      is not stopped, and its exit code is its own. The status bar says that the end of input was sent. A second
      Send EOF for the same program says that its input is already closed. A tab whose program has ended, and a
      window in which nothing was started in the selected tab, say that no program is running there. Each step
      of a chain has an input of its own; the end of input reaches the step that runs.
    - **Where the key acts.** Only while the Run tool window has the focus, and before the other actions of the
      chord: everywhere else ⌘D and Ctrl+D stay what they are (Duplicate Line in the editor, SPEC-0028
      behaviour 11). From Find Action the action acts on the selected tab wherever the focus was; without a
      Run tool window it is not available.
    - **Not covered.** No text can be typed to the program: the end of input is the only thing sent. A run on
      the host of a remote project (SPEC-0318) and the blocking run of agents (`run_config`, SPEC-0047) have
      no input, as before, and Send EOF says for a remote run that the input is already closed. A debugged
      program (SPEC-0039) and a terminal (SPEC-0062, where Ctrl+D is the shell's) are not affected.
    - UI automation counts the action among those that act on started programs, like Stop (SPEC-0319); it
      needs the right to run. There is no MCP tool for it: an agent's own runs have no input to end.
    [OSS: the id, the title "Send EOF" from `ActionsBundle.properties` and the chords of the bundled keymap
    files; GEN: the end of input is the closed standard input of a child process. Whether the reference closes
    the input or writes an end-of-transmission character was not observed.]

19. **v11 – HTTP request configurations:** a configuration that sends requests (SPEC-0047 v6 behaviour 17) runs
    in a tab like every configuration. The first line is its command line, `HTTP request` with file, request and
    environment; then per request what SPEC-0047 behaviour 17 lists, as the lines arrive, a refusal or a
    connection error in the error colour; the closing line has the exit code. Stop, Rerun, Clear and the instance
    rule are unchanged. [own design]

## Acceptance criteria
- **AK-1** Streaming runs report the command, stdout and stderr lines in order, the exit code, a stop that
  ends the process group, and a failing `before` step; the blocking run gives the same results (unit and
  integration tests in `ferris-idea-core`).
- **AK-2** Console lines, the line limit and the closing texts (unit tests of `run_model`).
- **AK-3** In a real window (`ferris-idea ui --selftest`): with a `run.toml` of the fixture, ⌃R runs the
  selected configuration, the Run tool window shows its output and "Process finished with exit code 0";
  a long-running configuration is stopped with Stop; ⌘4 hides the tool window.
- **AK-4** Use by hand next to the reference.
- **AK-5 (v2)** Detected configurations come after those of `run.toml` and do not replace them (test
  `detected_configurations_come_after_and_do_not_replace`); in a real window an npm script of the
  fixture's `package.json` is listed after the `run.toml` configuration and runs (`--selftest`).

- **AK-6 (v5)** The refusal text names the document that could not be saved (`run_model` unit test
  `a_document_that_cannot_be_saved_stops_the_start`); in the window: a modified `hello.js`, ⌃⇧D stops
  at the buffer's line and the tab loses its mark.

- **AK-7 (v6)** Chooser rows, numbering, the Temporary and Pinned headings and the order of entries (unit tests of the
  list model); quoting of the shown command line (unit test with an argument with a space and one with a quote); the
  reason for a start that cannot happen (unit test for a missing program and for safe mode).
- **AK-8 (v6)** In a real window: Rerun, Clear and Soft-Wrap in the Run window; Delete, Pin and Save Configuration in
  the widget's row menu (window check next to the reference).

- **AK-9 (v7)** The rows a typed text leaves, with headings above what is left of their group and none for an empty
  group, the fixed rows found by their text, no digits while text is typed; a digit picks a row only with no text
  (unit tests of the list model). In a window of the test harness: typing, Backspace, Enter on the highlighted row,
  Enter with no row left, Escape clearing first and closing second, a digit with no text.

- **AK-10 (v8, behaviour 16)** In the chooser for debugging a row is debugged without ⇧ and run with it, the current
  file likewise, "Edit Configurations…" stays, and the footer names the other way
  (`run_list_model::tests::the_chooser_for_debugging_exchanges_run_and_debug`); the action has its title, its place
  in the Run menu and a shortcut in both keymaps (`everywhere::tests::run_debug_and_window_actions_from_the_keymap`,
  `menus::tests::run_debug_and_window_actions_are_in_their_menus`). In a window of the test harness Debug… opens
  the chooser for debugging with the rows of Run…, and Escape closes it
  (`run_debug_actions_keymap::debug_opens_the_chooser_for_debugging`). On screen, and Enter starting a debug
  session from it: not checked.

- **AK-11 (v9, behaviour 17)** In a real window (`ferris-idea ui --selftest`): a second ⌃R on a running configuration
  shows the question; Cancel keeps the run (same tab, same output, still running); Stop and Rerun gives the tab a
  new run (a new process id in its output); with "Allow multiple instances" two tabs `rc-many` and `rc-many (2)`
  run at once without a question (group `run-config-instances`); the tab of a configuration with tasks shows the
  tool's output before the program's and the line of the opened page, a failing tool gives "Run stopped: … · sh
  failed" and the program does not run (group `run-config-before-launch`); with the UI driver the run widget's
  list shows a configuration under the heading of the folder made in the editor (group `run-config-folders`).

- **AK-12 (v10, behaviour 18)** A program started as the Run tool window starts it waits for its input, goes on
  after the end of input and ends with its own exit code, not stopped; a second end of input finds nothing to
  close; Stop still ends a waiting program; a program that does not read is not held up; the blocking run and a
  run on a host read nothing, as before (`crates/core/tests/it/run_eof.rs`, three tests, Unix). The key is
  limited to the Run tool window and stands in both bundled keymaps
  (`keymap_actions::tests::send_eof_is_an_action_of_the_run_window`,
  `everywhere::tests::seventh_package_actions_have_titles`), and the action is declared to UI automation as one
  that acts on started programs (`security::tests::seventh_package_actions_declare_their_effects`). In a window of
  the test harness: ⌘D in the editor duplicates the line and the program keeps waiting; with the focus in the Run
  tool window ⌘D lets the program write its last line and end with exit code 0, and the status bar says so; once
  more it says that nothing runs; without a Run tool window the action is not available
  (`seventh_package_keymap.rs::send_eof_lets_the_program_of_the_run_tab_go_on`). On screen, on a real keyboard
  and next to the reference: not checked.
- **AK-13 (v11, behaviour 19)** The streamed run of an HTTP request configuration reports its start with the
  `HTTP request` command line, the request and response lines as output and a refusal on the error stream, then
  the end with the exit code (`crates/core/tests/it/run_http.rs`: `the_streamed_run_reports_the_requests`). In a
  window and on screen: not checked.

## Out of scope
Debugging (SPEC-0039), text typed to the program (v10: only the end of input is sent, behaviour 18), ANSI colours in the output, clickable file links, the test tree
(F-078).

## Known deviations from the reference
Since SPEC-0047 v3 (behaviour 10) a start of a configuration that still runs asks first or, with "Allow multiple
instances", opens a further tab; the silent stop of behaviour 3 remains for a tab's own Rerun (behaviour 12).
The reference keeps configurations in its own XML; Ferris IDEA uses `.ferris-idea/run.toml` (SPEC-0047) and edits it with
the dialog of SPEC-0327. v6: a start that cannot happen always names the reason (IMP-0394; the reference starts a
broken temporary configuration silently not at all), and the shown command line is quoted (IMP-0395; the reference
drops the quotes). The reference's console keeps 1024 KB by default; Ferris IDEA keeps
10 000 lines.

**Window check 2026-11-09** (`main` @ 3b3cc1e0, agent run with real input, OBS-0028): behaviour of v5 confirmed: Debug saved the modified document, the tab lost its marker and the program paused at the shown line.
