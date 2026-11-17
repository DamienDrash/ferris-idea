# Beta for workflow (a): hand tests of round 5

**As of:** 2026-11-16 · `main` @ `b01da363` · criteria: [CONTENT_STYLE_GUIDE.md §5](../design/CONTENT_STYLE_GUIDE.md)
(beta: every hand test of the workflow in the current round passed in an agent run, no open
defect) · steps and expectations: the internal hand-test log, table under round 4.

Workflow (a): a project in Python, JavaScript, TypeScript or Java — open, edit, navigate, run or
debug, and commit with Git. Counted here are the round-5 hand tests whose steps do that in one of
these languages, and the editor and project-tree tests that do not depend on a language (part of
"open, edit"; decision of the coordinator under the general authority of 2026-11-13, made on 2026-11-16). Not counted: SQL (workflow b), other
languages (C/C++, Kotlin, Go, Rust, Ruby), frameworks (Spring, Vue, browser debugging), terminal,
AI, containers, remote, plugins, settings, the app itself, and Git beyond commit (see the end).

Sources of the status: the "Stand" entries of round 5 in the internal hand-test log, [PARITY.md](PARITY.md) and
DEFECTS-2026-11-05. "Agent" means an agent run with computer
use or the reference driver. Harness tests run a window without the screen in
`crates/workbench/tests/`: the new ones of this list are in `workflow_a_beta_handtests.rs` (a
program of its own, with the language server pool and safe mode on as in the application), older
ones in `it/`. "Self-test" names a group of `scripts/ci-window.sh`.

| No. | Title | Status | Window test | What a screen run still has to check |
|---|---|---|---|---|
| — | Core path (open, edit and save, ⌘B, ⌥F7, run, debug, commit) | no hand test of its own; parts in 12, 16, 21, 49, 136 | `core_path_python_…`, `core_path_java_…` | — (covered by the hand tests below) |
| 6 | Replace in Files | open | `hand_test_6_replace_in_files` (Python files instead of C++) | the fields as drawn; Show History through Find Action |
| 7 | Problems window | open | `hand_test_7_problems_window` | the list as drawn |
| 8 | Detected run configurations (npm scripts) | passed, agent 2026-11-04 (retest r5d, DEFECTS) | — | — |
| 9 | Multiple carets | passed, agent window run 2026-11-13 (PARITY F-012) | — | — |
| 12 | Python, TypeScript, Java in the editor | open (failed 2026-11-04: D59, language label; both fixed since) | `hand_test_12_python_typescript_java_in_the_editor` | the list and wavy lines as drawn; the hint for a missing server |
| 16 | Java in the editor | passed, agent 2026-11-07 (OBS-0027) | `core_path_java_…` | — |
| 18 | Own inspections and Inspect Code | open | `hand_test_18_own_inspections_and_inspect_code` | the wavy line and the Problems tab as drawn |
| 19 | Test trees for Jest/Vitest and pytest | passed, agent window run 2026-11-17 (retest after #936) | `hand_test_19_pytest_tree`, `hand_test_19_jest_tree` | — |
| 20 | Debug Node.js | open | `hand_test_20_debug_node` | ⌃D through a run configuration, F8/F7 from the keyboard |
| 21 | Watches, threads, values in the editor | passed, agent retest 2026-11-06 (OBS-0025) | `it/handtest_rechecks.rs` `hand_test_21_…` | — |
| 39 | Split editors | failed 2026-11-04; retest passed, agent window run 2026-11-14 | — | — |
| 44 | Python environments | passed with findings, agent 2026-11-04/05 | self-test `python` | — |
| 49 | Java: tests, imports, Generate, debug | passed with findings, agent 2026-11-04 | self-test `jvm` (Generate) | — |
| 51 | ESLint and Prettier | passed with findings, agent 2026-11-04 | self-test `js-tools` | — |
| 52 | Java inspections | passed with findings, agent 2026-11-04 | self-test `java-inspections` | — |
| 54 | Breakpoint properties | passed, agent window run 2026-11-13 | — | — |
| 65 | Java inspections with types | passed with finding D28, agent 2026-11-04 | self-test `java-inspections` | — |
| 72 | Exception and method breakpoints | Python half passed 2026-11-04; Java half failed (D30), fixed, retested headless 2026-11-14 | `it/handtest_rechecks.rs` `hand_test_72_…` | the Java half in the window |
| 73 | Attach to a running process | passed with finding, agent 2026-11-04 | — | — |
| 74 | Rename in comments, strings, text files | passed, agent window run 2026-11-10 | self-test `rename-text` | — |
| 94 | Move a tab to the other group | passed, agent window runs 2026-11-14 (both parts) | `it/tabs_and_tree_handtests.rs` `hand_test_94_…` | — |
| 95 | npm scripts, debugging TypeScript | passed with finding, agent 2026-11-04 | self-test `package-script-debug` | — |
| 100 | pyright with the chosen interpreter | passed, agent 2026-11-04/05 | self-test `python` | — |
| 101 | Search and install from the package index | passed, agent 2026-11-04/05 | self-test `python` | — |
| 112 | Hint in the expression field | passed with finding, agent 2026-11-04 | — | — |
| 123 | One undo for a refactoring over several files | passed, agent window run 2026-11-10 | self-test `undo-group` | — |
| 125 | Pause, Run to Cursor, Set Value | passed with finding, agent 2026-11-04 (Python) | — | — |
| 136 | Commit hooks | passed, agent retest 2026-11-06 | self-test `git-m63` (hooks) | — |
| 193 | Context menus, new files, trees | passed without the trackpad part, agent window run 2026-11-14 | `it/tabs_and_tree_handtests.rs` `hand_test_193_…` | the two-finger click on the trackpad |
| 195 | Annotate with Git Blame, then commit | passed, agent 2026-11-06 | — | — |
| 196 | Cut, copy and paste of files | passed, agent window run 2026-11-14 | `it/tabs_and_tree_handtests.rs` `hand_test_196_…` | — |
| 197 | Trust question and safe mode | open | `hand_test_197_safe_mode_until_trusted` (steps 3–4) | the native question, the banner, Trusted Locations, the restart (steps 1–2, 5–6) |
| 198 | Unselect Occurrence ⌃⇧G | failed 2026-11-04; retest passed, agent window run 2026-11-13 | — | — |
| 200 | Several selected rows in the project tree | passed, agent window run 2026-11-14 | `it/tabs_and_tree_handtests.rs` `hand_test_200_…` | — |
| 201 | Java package on paste | passed, agent window run 2026-11-14 | `it/tabs_and_tree_handtests.rs` `hand_test_201_…` | — |
| 207 | Files with errors in tabs and tree, Python widget | open | `hand_test_207_files_with_errors_and_the_python_widget` | the wavy underline and its colour as drawn |
| 208 | Popups close at a click beside them | open | `hand_test_208_popups_close_on_a_click_beside_them` (step 4) | the gutter's ▶ menu, a click into the tree, ⌘Tab, the run configuration dialog (steps 1–3, 5, 6) |
| 209 | Java in a single file, missing server | open (D59 fixed in a Maven project; the single-file part not repeated) | `hand_test_209_java_in_a_single_file` | step 6: the app started without jdtls on its `PATH` |
| 215 | Java: Generate lists, Delegate Methods, import hint | open | `it/java_generate_lists.rs`, `java_delegate_import.rs` (parts) | the whole hand test in the window |
| 216 | Python: Reformat, Optimize Imports, structure | passed except steps 7–8, agent 2026-11-12 (OBS-0034 part 10) | — | step 7 (without ruff), step 8 (safe mode) |
| 217 | Python: rename without the environment folder, extract | passed, agent 2026-11-12 (OBS-0034 part 11) | — | — |

**Counts:** 41 hand tests. Passed in an agent run: 27, and three in part (72 without the Java
half in the window, 193 without the trackpad, 216 without steps 7–8). Open: 11 (6, 7, 12, 18, 19,
20, 197, 207, 208, 209, 215). New harness window tests cover 10 of them (6, 7, 12, 18, 19, 20, 197,
207, 208, 209; all green on this Mac on 2026-11-16), plus the core path in Python and Java. Still
needing a screen run before the beta: 14 (the 11 open ones and the missing parts of 72, 193, 216).

**Defect found on the way:** debugging a Node.js program whose variables hold a function ended the
application: js-debug gives the function's source with line breaks, and the value drawn after the
line could not be laid out (hand test 20). Fixed in SPEC-0091 v22 (behaviour 13, AK-27).

**Findings of passed tests** (DEFECTS-2026-11-05) addressed on `main` since: D17 (#295), D25
(#304), D24 (`.venv` left out of project searches), D28 in part (a quick fix for an array printed as
text; the one for `==` on boxed values not traced), D36 (jdtls writes no metadata into the project). Not traced to a fix: the smaller
notes of 49 (field choice of Generate, now hand test 215), 54 (popup instead of the dialog), 73
(closure scope in Node), 95 (SCSS colours in `.vue`, a framework point), 125 (summary refresh).

## Outside workflow (a)

Git work beyond commit does not belong to workflow (a) (decision of the coordinator under the general authority of 2026-11-13,
2026-11-16): hand tests 25, 31, 34, 36, 70, 71, 135, 137 and 199. They stay in round 5 and count
for no beta.
