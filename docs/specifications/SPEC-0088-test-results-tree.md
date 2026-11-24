# SPEC-0088: Test results tree in the Run tool window

- **Version:** 5 · **Date:** 2026-11-17 (v5: the jump from a test to its source and the details from their first line, behaviours 12–13, AK-9–AK-10, beta screen run of workflow (a), hand test 19); 2026-11-13 (v4: Rerun Tests, the keymap's `RerunTests`, behaviour 11, AK-8; v3: Rerun Failed Tests, sorting, Expand and Collapse All, the history of the last 10 runs and the top node named after what ran, behaviours 7–10, AK-4–AK-7, OBS-0034 part 15 R2 and R3); 2026-11-01 (v2: summary above the console, toolbar above the tree; from hand test 15)
- **Sources:** GEN (after a test run the IDE shows a tree of suites and tests with their outcome and
  duration, a summary line, a filter for passed tests, and the failure message and output of the selected
  test; F-078), OSS (status icons `expui/run/testPassed|testFailed|testError|testIgnored` from
  `PlatformIconMappings.json`), SPEC-0045 (test reports), SPEC-0087 (Run tool window)
- **Feature matrix rows:** F-078

## Purpose
See at a glance which tests failed and why, without reading the whole output.

## Behaviour
1. **Tree:** when a test configuration (SPEC-0047 `kind = "test"`) finishes with a report, its Run tab
   shows the results at the left: the root "Test Results", then suites and tests, each with its status
   icon (passed, failed, error, ignored) and duration ("12 ms", "1 s 500 ms", "1 m 15 s"). A suite has the
   status of its subtree: failed or error wins, ignored only if nothing ran. All suites start expanded;
   the first failed test is selected. ↑/↓ choose, →/← expand and collapse, a click selects. [GEN, OSS]
2. **Summary:** above the console (v2; v1 had it above the tree, where it was cut off), the overall status
   icon and a line such as "Tests failed: 1, passed: 2 of 3
   tests – 512 ms", "Tests passed: 3 of 3 tests" or "…, ignored: 1 of 3 tests". [GEN]
3. **Show Passed:** a toggle in the toolbar above the tree hides passed tests and suites with only passed
   tests. [GEN]
4. **Details:** with a test selected, the console shows its message, details and captured output
   (duplicates once); with the root or a suite selected, the whole output. [GEN]
5. A report that cannot be read adds "Test report: <reason>" to the console in the error colour. [GEN]
6. Agents use `run_configuration` (the report is part of its result) and `parse_test_report`
   (SPEC-0045). [ADR-0008]

7. **Rerun Failed Tests (v3, OBS-0034 item 64, R2):** a button in the toolbar above the tree runs
   only the tests that failed or had an error in the tab's last run, as a temporary configuration
   `Failed tests of <name>`, without the before-launch chain. Each runner selects them its way:
   pytest by node ids (`file::Class::name`, the file from the report's module and the file of the
   command), Maven `-Dtest=Class#method,…`, Gradle `--tests Class.method` per test, Vitest and
   Jest `-t '(?:title|title)$'`, Go `-run '^(?:TestA|TestB)$'`, Cargo `-- --exact path…`. Where
   that is not possible — nothing failed, a remembered run is shown, the runner selects no single
   tests (any other command), the report names no class — the button is greyed and its tooltip
   says why. MCP: `run_configuration` with `failed_only: true` does the same with the last
   remembered run of the configuration (behaviour 9). [OBS, GEN]
8. **Sort, Expand All, Collapse All (v3, R3):** a sort button switches the order of suites and tests
   at every level between the runner's order, alphabetical (case ignored) and by duration (longest
   first, none last); its tooltip names the next order. Expand All opens every suite, Collapse All
   leaves the top node only. [OBS, GEN]
9. **History (v3, R3):** the last 10 finished runs of each configuration are kept, newest first, in
   the user's settings folder (`<settings>/test-history/<hash>.json`, the hash of the project
   folder and the configuration's name, so neither appears in the file name; the folder 0700, the
   file 0600), never in the project. Only the tree is kept: names, classes, outcomes, durations and
   counts; messages, stack traces, output, the command and its environment are not, since they
   can hold secrets. A history button lists "Last run" and the remembered runs with their time
   (UTC) and summary line; choosing one shows its tree (no details); "Last run" goes back. Runs
   through MCP `run_configuration` are remembered the same way. [GEN, security first: ADR-0025
   spirit, CLAUDE.md rule 12]
10. **The top node (v3, OBS-0034 item 63):** a suite named only after the runner (`pytest`) is named
   after what ran: the module its tests share (`test_prices`), else the configuration's file. The
   root stays "Test Results". [OBS]
11. **Rerun Tests (v4):** `RerunTests` (⌃⌘R and ⌥⇧R in the macOS keymap, Alt+Shift+R in the
   default one; Run › Rerun Tests, Find Action) runs every test run of the Run tool window
   again: each tab whose configuration is a test configuration (SPEC-0047 `kind = "test"`),
   the temporary ones of behaviour 7 included, in the order of the tabs — whichever tab is
   selected and wherever the keyboard focus is, so a test run stays one key away after another
   program was run or the editor got the focus. Each run starts in its own tab as that tab's
   Rerun does (SPEC-0087 v6 behaviour 12): a run that still goes there is stopped first, without
   a question and without a further tab, and the modified documents are saved first (SPEC-0087
   v5). The Run tool window is shown, and the tab that was selected stays selected. A tab whose
   configuration no longer exists, and every tab of a configuration that is not a test, is left
   as it is. Without a tab of a test configuration the action does nothing. Agents run tests
   again with `run_configuration`; no further tool is needed. [OSS: `RerunTestsAction` of
   intellij-community restarts every registered run of the project, whether or not its tab is the
   selected one, which its documentation gives as the reason for the action beside Rerun; the
   action id and its shortcuts in the bundled keymaps. GEN]
12. **Jump to the test (v5):** a double click on a test, or Enter with a test selected, opens the
   file that declares it in the editor, whether it was open or not, with the caret at the start of
   the test's line, and gives the editor the focus; the tree keeps its selection. A single click
   still only selects and shows the details (behaviour 4); on a suite a double click still opens
   or closes it. The file and line come from the report and the ▶ markers of SPEC-0045:
   - Jest and Vitest: the test file the report names for the test's suite (SPEC-0045 v6
     behaviour 8); the line of the marker whose full name is the suites' names below the file and
     the test's own, joined by spaces (behaviour 12 there).
   - pytest: the module part of the test's class (`tests.test_prices.TestCart` →
     `tests/test_prices.py`; the classes are the names with a capital), in the project folder or,
     when pytest ran from a folder below it, the shortest path of the project that ends with it
     (hidden folders and those of packages and builds not looked in); the line of the marker of
     the test's name in its class, parameters (`[…]`) left out (behaviour 13 there).
   - Without a marker for the name (a name made at run time) the file opens at its first line.
     When no file is found — another runner, a module that no longer exists, a remembered run of
     Jest or Vitest (the history keeps no paths, behaviour 9) — the status bar says that the
     source of the test was not found and nothing opens. [GEN; beta screen run of workflow (a),
     hand test 19]
13. **Details from their first line (v5):** when the console shows a test's details (behaviour
   4) — after the run, at a click, at a key — it starts at their first line, where the message and
   the expected and received values are, whatever Scroll to End says; Scroll to End applies to the
   run's output. Before v5 a failed Jest test's details opened scrolled to the end of the stack
   trace. [GEN; beta screen run, hand test 19]

## Acceptance criteria
- **AK-1** Rows, subtree status, the passed filter, collapsing, the summary line, durations, the test of a
  row and its details (unit tests of `test_tree_model`).
- **AK-2** In a real window (`ferris-idea ui --selftest`): a TAP test configuration shows the tree, the
  summary "Tests failed: 1, passed: 1, ignored: 1 of 3 tests" and the message of the selected failed test;
  hiding passed tests removes the passed one.
- **AK-3** Use by hand next to the reference.

- **AK-4 (v3, behaviour 7)** The selection of the failed tests for pytest (a module test, a class
  test, a run of the whole project), Maven, Gradle, Vitest/Jest, Go (subtests to their test) and
  Cargo, and the refusals (nothing failed, an unknown runner, no class)
  (`test_rerun::tests::{pytest_reruns_the_failed_node_ids, every_runner_selects_its_failed_tests,
  refusals_say_why}`); in the window by hand: the button after a failing pytest run, greyed with
  its reason after a passing one. Open.
- **AK-5 (v3, behaviours 8 and 10)** Sorting by name and duration, the top node named after the
  module or the file, other runners' suites kept, the history's times
  (`test_tree_model::tests::the_tree_is_named_after_its_file_and_sorts`).
- **AK-6 (v3, behaviour 9)** Ten runs per configuration, newest first, no message, detail, output
  or environment in the file, no path or name in the file name, mode 0600, one file per
  configuration (`test_history::tests::the_last_ten_runs_are_kept_without_output`).
- **AK-7 (v3, behaviours 7 and 9)** MCP `run_configuration` with `failed_only`: refused without a
  remembered run, the failed pytest test of the remembered run with `dry_run`
  (`protocol.rs::single_tests_and_rerun_failed_tools`). The window self-test was not extended: it
  needs a display and pytest, which the server CI has neither of.
- **AK-8 (v4, behaviour 11)** Which tabs run again: those of a test configuration in tab order,
  a further instance's tab under its own title, none for a program's tab or a configuration that
  is gone (`run_debug_actions::tests::rerun_tests_takes_the_tabs_of_test_configurations`); the
  action has its title, its place in the Run menu and a shortcut in both keymaps
  (`everywhere::tests::run_debug_and_window_actions_from_the_keymap`,
  `menus::tests::run_debug_and_window_actions_are_in_their_menus`). In a window, with test runs
  that start again: not checked.
- **AK-9 (v5, behaviour 12)** The file and line of a pytest test (module, class, parametrized,
  module below the folder pytest ran from), of a Jest test inside a `describe` and at the top;
  nothing for a suite, a missing module or a remembered Jest run
  (`test_tree_model::tests::the_source_of_a_test`); in the harness window with pytest and Jest
  installed: a double click with real mouse events and Enter open the file at the test's line,
  the pytest file open, the Jest file closed before
  (`workflow_a_beta_handtests.rs`: `hand_test_19_pytest_tree`, `hand_test_19_jest_tree`).
- **AK-10 (v5, behaviour 13)** After a failing Jest run and after a click on another test the
  console's first line is the details' first line (`hand_test_19_jest_tree`).
## Out of scope
Live updates while the tests run (the tree appears at the end), exporting results. The jump to the
source of a test of other runners (JUnit of Maven and Gradle, Go, libtest, TAP) — later.

## Known deviations from the reference
The reference fills the tree while tests run through its own runner integrations; Ferris IDEA reads the
report when the run ends (SPEC-0045), so the tree appears at the end.
v4: the reference reruns the runs its test runners registered; Ferris IDEA takes the tabs of test
configurations, which is what it can tell a test run by. Which runs the reference registers was not
compared in a window.
