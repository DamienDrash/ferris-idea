# SPEC-0045: Test results — reading test reports into a result tree

- **Version:** 7 · **Date:** 2026-11-17 (v7: a failed test's message, place and duration from the TAP YAML block and the libtest panic, behaviour 14, AK-17 — headless pre-check of F-078); 2026-11-17 (v6: Jest/Vitest suites keep their test file, Debug at the ▶ of a Jest test, behaviours 8 and 12, AK-15–AK-16, beta screen run of workflow (a), hand tests 19 and 208); 2026-11-13 (v5: ▶ at the tests of pytest, Go and Rust files and a run from the context of only the test at the caret, behaviour 13, AK-14, OBS-0034 part 15 R1); 2026-11-09 (v4: ▶ at the tests of Vitest and Jest files; v2: the test line after a subtest only names it; from hand test 15;
  v3: Jest/Vitest JSON and `go test -json`, test configurations for Vitest, Jest, pytest and Go)
- **Sources:** GEN (IDEs show test results as a tree of suites and tests with status, duration, failure
  message and output, and can import results from files; F-078), STD (JUnit XML as written by Ant, Maven
  Surefire, pytest `--junitxml`, Gradle and others: `testsuites`/`testsuite`/`testcase` with `failure`,
  `error`, `skipped`, `system-out`, `system-err`, `time`; TAP versions 12–14 from testanything.org:
  plan, `ok`/`not ok`, `# SKIP`/`# TODO`, diagnostics, subtests by indentation, `Bail out!`; the plain
  output of Rust's libtest: `test <name> ... ok|FAILED|ignored`, failure sections and the summary line;
  the JSON report of Jest `--json` and Vitest `--reporter=json`: `testResults[].assertionResults[]` with
  `ancestorTitles`, `title`, `status`, `duration`, `failureMessages`; the event stream of `go test -json`
  (`cmd/test2json`): `Action`, `Package`, `Test`, `Elapsed`, `Output`)
- **Feature matrix rows:** F-078

## Purpose
Turn the output of any common test tool into one result tree the IDE and agents can read.

## Behaviour
1. The format is given or detected: XML starting with `testsuites` or `testsuite` is JUnit; text with a TAP
   plan or `ok`/`not ok` lines is TAP; text with `running N tests` or `test … ... ok` lines is libtest.
   [STD]
2. The tree has suites containing tests (and nested suites). Each test has a name, a class or module
   where the format gives one, a status (passed, failed, error, skipped), a duration in seconds when
   known, a message and details for failures and skips, and captured output. Each suite has the counts
   of its subtree. [GEN]
3. JUnit: nested `testsuite` elements become nested suites; `failure` is failed, `error` is error,
   `skipped` is skipped (message from the attribute or text); `system-out`/`system-err` of a test and of
   a suite are kept. [STD]
4. TAP: numbered and unnumbered test lines, descriptions after ` - `, `# SKIP` (skipped), `# TODO` (a
   failing TODO test counts as skipped, a passing one as passed), indented subtests become a nested
   suite named by the test line that follows them (v2: that line is no test of its own; when it fails
   but the subtest has no failure, the failure is kept as a test inside the subtest; `# Subtest:` lines
   and comments after it are no details), YAML blocks and `#` lines after a test become its details; missing tests from the plan are
   reported as errors; `Bail out!` ends the run with an error. [STD]
5. libtest: test lines with `ok`, `FAILED`, `ignored` (with optional reason); module paths
   (`a::b::test`) split into module and name; the `---- name stdout ----` sections become the failing
   tests' output; several test binaries in one output become several suites. [STD]
6. Damaged input never stops the reader: unknown lines are ignored and a malformed XML document is an
   error naming the position. [GEN]
7. MCP tool `parse_test_report` (text, optional format) returning the tree and the totals. [ADR-0008]
8. **Jest/Vitest JSON (v3):** one suite per test file, named relative to the folder all files share;
   `describe` titles become nested suites; `pending`, `skipped`, `todo` and `disabled` are skipped; the
   failure messages are the details, their first line the message; a file that failed without tests (a
   syntax error) becomes the error "Test suite failed to run" with the runner's message. (v6) Each
   file's suite keeps the file's path as the report gives it (`testResults[].name`), so the tree
   can open a test's source (SPEC-0088 v5 behaviour 12); nested suites keep none. [STD]
9. **`go test -json` (v3):** one suite per package; subtests (`TestA/case`) nest under their test, which
   becomes a suite; the output of a failed test is kept and its first line that is not a `===`/`---`
   frame is the message; a package that failed without tests (a build error) becomes the error "build"
   with its output; non-JSON lines are ignored. [STD]
10. **Test configurations (v3):** the detected configurations run tests with a report: Vitest
    (`npx vitest run --reporter=json --outputFile=.idecraft/vitest.json`), Jest (`npx jest --json
    --outputFile=.idecraft/jest.json`), pytest (`python3 -m pytest --junitxml=.idecraft/pytest.xml`),
    Go (`go test -json ./...`, report from the output). `RunClass` on a `*.test.*`/`*.spec.*` file of a
    module with Vitest or Jest runs that file; on a Go `_test.go` file its package (SPEC-0097
    behaviour 3). The report folder is created before the run. [GEN]
11. **No stale results (v3):** a report file is read only if it was written by this run (modified
    after the run started, two seconds of slack); otherwise the run reports "no test report written by
    this run" instead of showing the previous run's tree. [GEN]
12. **Markers in Vitest and Jest files (v4):** a `*.test.*`/`*.spec.*` file of a package that depends on
    Vitest or Jest (the nearest `package.json`) gets a ▶ in the gutter (SPEC-0122 behaviour 1) on every
    `describe`, `suite`, `it` and `test` call with a literal name, also with `.only`, `.skip`, `.each`,
    following edits. A click offers "Run '<file>: <full name>'": the file's configuration of behaviour 10
    with the runner's name filter `-t`, a regular expression of the full name (the suites' names and
    its own, joined by spaces; `.each` placeholders match anything; a test's filter ends at the name, a
    suite's is followed by a space). `RunClass` with the caret inside a test or suite runs only that
    one (innermost first), also on a remote project's host. Names computed at run time get no marker.
    `run_markers` (MCP) lists them. [GEN, STD, OBS-0028]
    (v6) For Jest the menu also offers "Debug '<file>: <full name>'", as a package script's ▶
    does: the same command, `npx jest …`, runs under js-debug as a `package.json` script does
    (SPEC-0168 behaviour 1), with `--runInBand` added unless it is there, so the tests run in the
    process the debugger follows; breakpoints in the test file stop it. Jest runs its transformed
    test files at once and the transformation can move a test's lines, so js-debug is asked to
    wait for each script's source map before it runs (`pauseForSourceMap`); without that a
    breakpoint below a leading comment was set only after the test had run. Vitest keeps Run
    only: its workers were not checked under js-debug. [STD: Jest's `--runInBand`; OSS: js-debug's
    launch option `pauseForSourceMap`; GEN; beta screen run of workflow (a), hand test 208]

13. **One test of pytest, Go and Rust (v5, OBS-0034 item 62):** ▶ markers stand at the tests of
    `test_*.py`/`*_test.py` (pytest's default collection: `test*` functions of the module, `Test*`
    classes without `__init__` and their `test*` methods), of `*_test.go` (`func TestXxx(t
    *testing.T)`, Go's naming rule) and of Rust sources (functions with `#[test]`,
    `#[tokio::test]` and the like, inside their inline `mod`s). Each runs alone, as a temporary
    configuration named after the test: pytest with the node id (`tests/test_prices.py::test_total`,
    `…::TestCart::test_empty`, a class `…::TestCart`), named `test_prices.py::test_total`; Go with
    `-run '^TestTotal$'`, named `TestTotal`; Rust with `cargo test <path> -- --exact`, the path
    being the file's module path in its crate plus the inline modules (`shop::cart::tests::adds`
    for `src/shop/cart.rs`), a file below `tests/` with `--test <target>` (`tests/it/cart.rs` →
    `--test it cart::adds`), named after the path. Run from the context (⌃⇧R) with the caret
    inside such a test runs only it, as inside a JUnit test method (SPEC-0122) or a Vitest/Jest
    test (behaviour 12); outside a test the whole file runs. A Rust source with tests and no `main`
    now runs its tests from the context (`cargo test <module path>`, a file below `tests/` as its
    target), report `libtest`. MCP `run_markers` takes these files too. Before v5 the context run
    of a pytest file always ran the whole file (OBS-0034 item 62). [OBS, STD]
14. **Why a test failed, and where (v7, headless pre-check of F-078 on 2026-11-16):** the tree
    shows a failed test's message next to it and its place in the details; two formats carried
    them only inside text the tree did not read.
    - **TAP:** the YAML block under a test line (TAP 13/14) gives `duration_ms` as the test's
      duration (Node's test runner writes it for every test) and `error` or `message` as the
      message of a failed or erroneous test where the test line gave none: a one-line value
      without its quotes, or the first non-empty line of a block value (`error: |-`, as Node
      writes an assertion's message). The block itself stays in the details as before, with
      Node's `location` (file:line:column of the test) and `stack`.
    - **libtest:** a failed test whose captured output holds a panic gets the panic message's
      first line as its message (``assertion `left == right` failed: wrong sum``) and `panicked at
      src/main.rs:10:18` as its details, the place the window and agents can open; the whole
      output stays. Rust 1.73 and later print the place first (`panicked at file:line:col:`, the
      message on the next line), older releases `panicked at 'message', file:line:col`; both are
      read.
    - Before v6 both showed no message: a Node test only `not ok` with the YAML text as details,
      a Rust test only its output. libtest on stable Rust prints no time per test, so a Rust
      test still has no duration (only the nightly `--report-time` does). [GEN, STD]

## Acceptance criteria
- **AK-1** JUnit: nested suites, all statuses, times, messages, output, counts; malformed XML error.
- **AK-2** TAP: plan, skip/todo, diagnostics, subtests, missing tests, bail out; real output of Perl's
  `Test::More`. (Real-producer part skipped without `perl`.)
- **AK-3** libtest: passing, failing, ignored with reason, module paths, failure output, several
  binaries; real output of `cargo test` on a scratch crate. (Skipped without `cargo`.)
- **AK-4** Format detection for all three.
- **AK-5** The MCP tool returns tree and totals.
- **AK-6** (v3) Real reports of Vitest and Jest (with a file that does not parse) become trees
  (`crates/core/tests/it/testreport.rs`: `vitest_report`, `jest_report`).
- **AK-7** (v3) A real `go test -json` stream with a subtest table and a build failure
  (`gotest_report`).
- **AK-8** (v3) With Go and pytest installed: the detected configurations run and their trees list the
  tests; an old report is not shown again (`crates/core/tests/it/test_runners.rs`).
- **AK-9** (v3) `RunClass` configurations for JS/TS test files and Go test files
  (`crates/core/tests/it/run_context.rs`: `test_files_of_js_and_go`); in a real window ⌃⇧R on a Go test
  file shows the tree with the failure selected (`idecraft ui --selftest`).
- **AK-10** (v3) By hand: a Vitest, a Jest and a pytest project next to the reference.

- **AK-11 (v4)** Markers with their full names, kinds and lines; JavaScript and TSX files; the test
  around a line; the nearest manifest names the runner (`js_tests` unit tests).
- **AK-12 (v4)** MCP `run_markers` on a Vitest file: suite and test, command with `-t` and the escaped
  name (`tests/functional/f036_javascript.rs`: `tests_of_a_vitest_file_get_run_markers`).
- **AK-13 (v4)** Name filters: escaping, `.each` placeholders, end of test or suite
  (`crates/core/tests/it/run_context.rs`: `js_test_name_filters`); checked against Vitest 2.1.5 by hand.

- **AK-14 (v5, behaviour 13)** Markers and the test at the caret for pytest (functions, decorated
  ones, a `Test*` class and its methods, not a class with `__init__`, not helpers), Go (`TestX`, not
  `Testable`) and Rust (`#[test]`, `#[tokio::test]`, nested `mod`s) (unit tests
  `test_markers::tests::{pytest_markers_and_the_test_at_the_caret, go_and_rust_markers}`); the
  configurations, their names and the Rust file run (`crates/core/tests/it/run_context.rs::single_tests_of_pytest_go_and_rust`);
  MCP `run_markers` on a pytest and a Go file (`protocol.rs::single_tests_and_rerun_failed_tools`).
  In the window by hand: ⌃⇧R in `test_total` of `test_prices.py` runs one test. Open.

- **AK-15 (v6, behaviour 12)** `npx jest …` is planned for js-debug with `--runInBand`, Vitest and
  other npx commands are not (`debug_plan::tests::jest_through_npx_runs_under_js_debug`); with
  js-debug and Jest a breakpoint below a leading comment stops the test
  (`crates/debugger/tests/it/jsdebug.rs::jest_test_stops_at_breakpoint`); in the harness window
  with Jest and js-debug installed: the ▶ menu of a Jest test lists Run and Debug,
  and Debug stops at a breakpoint in the test (`workflow_a_beta_handtests.rs::hand_test_19_jest_tree`).
- **AK-16 (v6, behaviour 8)** The suites of a real Jest report keep their files, nested suites none
  (`crates/core/tests/it/testreport.rs::jest_report`); the history keeps no path
  (`test_history::tests::the_last_ten_runs_are_kept_without_output`).
- **AK-17 (v7, behaviour 14)** `crates/core/tests/it/testreport.rs`:
  `tap_yaml_gives_duration_and_message` (real `node --test --test-reporter=tap` output of Node 22:
  durations in seconds, the block message's first line, a quoted one-line message, `location` and
  `stack` kept in the details, no message on a passed test) and
  `libtest_panic_gives_message_and_place` (the panic of Rust 1.73+ and the older form: message,
  `panicked at` place, output kept). Through MCP `run_configuration` on a Node and a Cargo project
  on 2026-11-16 the tree had no message before and has it after.
## Out of scope
Running tests (run configurations, F-077, SPEC-0087), live streaming results while tests run, other
formats (NUnit, xUnit.net, TRX) — later. Rerunning failed tests and the history of runs are
specified in SPEC-0088 (v3); a single test or `describe` block from the gutter in behaviours 12 and
13 here. Single tests and rerunning failed ones of Node's built-in test runner (`node:test`, run with
`--test-name-pattern`), a duration per Rust test (stable libtest prints none) and markers for
`unittest.TestCase` subclasses whose name does not start with `Test` (pytest collects them) are
open; they wait for the comparison with the reference (OBS-0078, gaps L3–L5). Debugging a single Vitest test from the gutter — later.

## Known deviations from the reference
Not compared yet; the reference's import of test results was not observed. Improvement (IMP-0036): a
report left over from an earlier run is never shown as the result of a run that wrote none.

**Window check 2026-11-09** (`main` @ 3b3cc1e0, agent run with real input, OBS-0028): ▶ at `describe` and `it` in a Vitest file seen in the window.
