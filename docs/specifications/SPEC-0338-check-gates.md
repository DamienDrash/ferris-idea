# SPEC-0338: Check gates per project

- **Version:** 1 · **Date:** 2026-11-17
- **Sources:** `GEN` (the usual check commands of Cargo, Go, npm tools, Python tools, Maven and
  Gradle; reading compiler and linter output); ADR-0023 rule 5 (verification gates), rule 3
  (structured tool answers) and rule 7 (safety rules for MCP tools); ADR-0008 (every feature has an
  MCP tool); SPEC-0047 (run configurations: how a command is run), SPEC-0048 (project model: which
  build systems a workspace has), SPEC-0045 (test reports), SPEC-0278 (project trust and safe
  mode), SPEC-0053 (agent rights and the agent log), SPEC-0006 v56 behaviour 12 (`build_project`:
  the errors of a build's output). No observation of the reference: the reference has no check
  gates.
- **Feature matrix rows:** none (ROADMAP M19.3, harness phase after 1.0).

## Purpose
An agent's change is "done" only when the checks the project asks for have passed. A check gate is
one such check: format, build, inspections without new errors, the tests the change affects, and
optionally all tests. Ferris IDEA derives sensible gates from the build systems it detects, lets the
project adjust them in a file, runs them as deterministic Rust code and returns a structured
verdict: per gate its status, duration, findings with file, line, message and suggestion, and the
end of its output. The verdict comes only from the processes' exit codes and output; an agent
cannot declare a gate passed.

## Behaviour

### Gates and their defaults
1. A gate has a `name`, a `kind` (`format`, `build`, `inspections`, `affected_tests`,
   `all_tests`), `enabled`, `on_failure` (`stop` or `continue`), a timeout in seconds and its
   steps. A step is one program with its arguments, a workspace-relative folder, extra environment
   variables and, for tests, the report format of SPEC-0045. Every step runs as a process without a
   shell: arguments are passed as they are, nothing is interpolated into a command line.
   [ADR-0023, GEN]
2. The default gates come from the modules of the project model (SPEC-0048), in this fixed order:
   `format`, `build`, `inspections`, `affected_tests`, `all_tests`. Every default gate has
   `on_failure = "stop"` except `inspections` and the test gates (`continue`), and `all_tests` is
   disabled by default. A gate with no step for the project is not listed. Per build system
   (a Cargo or Maven member module is covered by its top module; Gradle and Go per module):
   [GEN]

   | Build system | format | build | inspections | affected tests | all tests |
   |---|---|---|---|---|---|
   | Cargo | `cargo fmt --all -- --check` | `cargo build --offline --all-targets` | `cargo clippy --offline --all-targets` | `cargo test --offline -p <package>` per affected package | `cargo test --offline --workspace` |
   | Go | `gofmt -l .` (fails on any output) | `go build ./...` | `go vet ./...` | `go test -json ./<dir>` per affected package folder | `go test -json ./...` |
   | npm | `npx --no-install prettier --check .` with Prettier | `npm run build` with a `build` script, else `npx --no-install tsc --noEmit` with TypeScript | `npx --no-install eslint .` with ESLint | `npx --no-install vitest related --run <files>` with Vitest, `npx --no-install jest --findRelatedTests <files>` with Jest, else the `test` script | `npm test` with a `test` script |
   | Python | `ruff format --check .` with Ruff, else `black --check .` with Black | — | `ruff check --output-format=concise .` with Ruff, else `flake8 .` with Flake8 | `python3 -m pytest <test files>` with pytest | `python3 -m pytest` with pytest |
   | Maven | — | `mvn -o -q test-compile` | — | `mvn -o test` in each affected module | `mvn -o test` |
   | Gradle | — | `<gradle> --offline classes testClasses` | — | `<gradle> --offline test` in each affected module | `<gradle> --offline test` |

   "With X" means the module declares X as a dependency; `<gradle>` is the runner the project
   model names (the wrapper or `gradle`). Go steps run with `GOPROXY=off` and
   `GOFLAGS=-mod=readonly`. A leading `python3` is the workspace's selected interpreter, as in
   run configurations (SPEC-0047).
3. The default gates never fetch anything over the network: Cargo, Maven and Gradle run offline,
   `npx` does not install, Go has no module proxy. A missing dependency fails the gate with the
   tool's own message instead of a download. [ADR-0023 rule 7, deviation D1]

### Affected tests
4. The changed files are the paths an agent passes (`changed`), or else the paths Git reports as
   changed, staged or untracked in the repository of the workspace, read without starting a `git`
   process; paths outside the workspace are left out. A path an agent passes must be
   workspace-relative and stay inside the workspace (no `..`, not absolute); otherwise the call is
   refused. [GEN, SPEC-0032]
5. The affected tests are chosen from the changed files without guessing:
   - Cargo: every package whose folder holds a changed file (the innermost package); a changed file
     of a workspace that lies in no member package (for example `Cargo.lock`) affects the whole
     workspace (`cargo test --offline --workspace`).
   - Go: every folder with a changed `.go` file, as a package of its module; a changed `go.mod` or
     `go.sum` affects `./...`.
   - npm with Vitest or Jest: the changed files of the module, passed to the runner's related-test
     selection; without either runner the module's `test` script, once, when any file of the module
     changed.
   - Python with pytest: when only test files changed (`test_*.py`, `*_test.py`, or files under a
     `tests` folder), just those; when any other Python file changed, all tests of the module.
   - Maven and Gradle: the tests of every module whose folder holds a changed file (the innermost
     module).

   With no changed file the gate is `skipped` with the reason. [GEN]
6. File paths passed as arguments are relative to the step's folder and always start with `./`, so
   a file name can never be read as an option. Files that no longer exist are not passed.
   [ADR-0023 rule 7, GEN]

### Project file
7. `.ferris-idea/check-gates.toml` adjusts the defaults. `defaults = false` drops every default gate.
   Each `[[gate]]` entry with the name of a default gate changes only the keys it gives (`enabled`,
   `on_failure`, `timeout`, `command`, `cwd`, `env`, `fail_on_output`, `pass_changed`); an entry
   with a new name adds a gate after the defaults, in file order, and needs `kind` and `command`.
   `command` replaces the gate's steps by one step. `pass_changed = true` appends the changed files
   of the step's folder (behaviour 6) to the command; a gate with `pass_changed` and no changed file
   is skipped. `fail_on_output = true` fails a step that prints anything on its standard output
   (for checkers like `gofmt -l` that exit with 0). [GEN]
8. `[[baseline]]` entries (`file`, `message`, optional `gate`) name known inspection findings. A
   finding matches when its file and message are equal; line numbers are ignored so that moving code
   does not break the baseline. A matching finding is reported with `baseline: true` and does not
   fail its gate. [ADR-0023 rule 5 "without new errors", GEN]
9. Unknown keys, an unknown `kind` or `on_failure`, an empty `command`, a `cwd` outside the
   workspace, a new gate without `kind` or `command`, two gates with one name, or a malformed file
   are problems, each naming the gate. With problems, listing shows them and running refuses.
   [SPEC-0047 behaviour 2]
10. The project file is a file that makes the project run code: project trust (SPEC-0278) names it
    among its findings. [SPEC-0278 behaviour 3]

### Running
11. The gates run one after another in their order, each step of a gate in turn. A step that
    times out is killed and fails. A failed gate with `on_failure = "stop"` ends the run: the
    remaining gates are `not_run`. A disabled gate is `skipped`. The default `all_tests` gate is
    disabled, so it runs only when the project file enables it or the call passes
    `all_tests = true`, which enables every gate of that kind for the call. A call can name a
    subset of the gates to run; an unknown name is refused. A gate with nothing to run is `skipped`
    with its reason. [ADR-0023 rule 5]
12. A gate's status is `passed`, `failed`, `skipped` (with a reason) or `not_run`. It is failed
    when:
    - `format`, `build`, tests: a step exits with a code other than 0, cannot start, or times out;
      `format` also when a step with `fail_on_output` printed anything;
    - `inspections`: a finding with severity `error` is not in the baseline, or a step failed
      without any finding that explains it (the tool itself failed). Warnings never fail it.

    The whole run passes when no gate failed and none was left `not_run`. There is no argument,
    file key or environment variable that marks a gate or the run passed. [ADR-0023 rule 5]
13. Each gate's result has its `name`, `kind`, `status`, `reason` (for skipped and failed),
    duration in milliseconds, its steps (command, folder, exit code, timed out, duration) and
    `findings`. A finding has `file` (workspace-relative where it can be made so), `line` and
    `column` (1-based, where known), `severity` (`error` or `warning`), `message`, `suggestion`
    (where Ferris IDEA knows one) and `baseline`. Findings come from:
    - build and inspections: the compiler and linter messages of rustc, Clippy, javac, kotlinc,
      Go, GCC and Clang, tsc, ESLint and Ruff/Flake8 (`file:line:column: message`), as
      `build_project` reads them;
    - format: the files the formatter names (`cargo fmt` "Diff in", `gofmt -l`, Prettier `[warn]`,
      Black and Ruff "would reformat"), each with the suggestion to format the file;
    - tests: each failed test from the parsed report (name, class, message).

    At most 200 findings per gate are listed; `more` counts the rest. [ADR-0023 rules 3 and 5]
14. Each step's output is kept as its last 4 KiB (standard output and error together), with
    `truncated` when more was cut, and with known secret patterns masked. Output and messages are
    project content: tool descriptions mark them as untrusted data, not instructions.
    [ADR-0023 rules 3 and 7]

### MCP tools (ADR-0008)
15. `list_check_gates` reads only: the gates in order with their source (`default` or `file`),
    whether they are enabled, `on_failure`, and the steps they would run for the current changed
    files (behaviour 4, or `changed`), plus the changed files and the problems of the project file.
    It needs no right and runs nothing.
16. `run_check_gates` takes `gates` (names, optional), `all_tests` (default false), `changed`
    (optional) and `dry_run` (default true). The dry run answers like `list_check_gates` for the
    gates that would run. `dry_run = false` needs the agent right `run` and a trusted project: in
    safe mode it refuses with the safe-mode message and runs nothing. It returns `passed`, the gate
    results (behaviour 13), and is logged in `.ferris-idea/agent-log.jsonl` with each gate's name and
    status. [SPEC-0053, SPEC-0278, ADR-0023 rule 7]

## Acceptance criteria
- **AK-1** Given a Cargo workspace, a Go module, an npm package with Prettier, ESLint, Vitest and a
  build script, a Python module with Ruff and pytest, a Maven and a Gradle project, when the default
  gates are derived, then each has the gates and commands of the table in behaviour 2, in order,
  with `all_tests` disabled and offline flags set (behaviours 2, 3).
- **AK-2** Given changed files in two Cargo member packages, a Go subfolder, a Python test file
  only, a Python source file, and an npm module with Vitest, when the affected tests are chosen,
  then they are the packages, folders and files of behaviour 5, every file argument starts with
  `./`, and with no changed file the gate is skipped (behaviours 5, 6).
- **AK-3** Given a project file that disables a default gate, changes another's `on_failure`, adds a
  custom gate with `pass_changed` and has a baseline, when the gates are loaded, then the result
  follows behaviour 7; given unknown keys, an unknown kind, a `cwd` with `..` or a duplicate name,
  then each is a problem naming the gate (behaviours 7, 9).
- **AK-4** Given gates whose commands fail, when they run, then a failed `stop` gate leaves the
  later gates `not_run`, a failed `continue` gate does not, a disabled gate is `skipped`,
  `all_tests` runs only when asked, a timed-out step fails its gate, and the run passes only when
  no gate failed or was left unrun (behaviours 11, 12).
- **AK-5** Given compiler, linter, formatter and test-report output, when a gate's findings are
  read, then they carry workspace-relative file, line, column, severity, message and the format
  suggestion; an inspection gate with only warnings, or only baselined errors, passes; one with a
  new error fails; long output is cut to its last 4 KiB with `truncated` (behaviours 8, 12, 13, 14).
- **AK-6** Given an MCP client, when it calls `list_check_gates` and `run_check_gates` without
  `dry_run`, then nothing runs; with `dry_run = false` and without the `run` right the call is
  refused; in an untrusted project it is refused with the safe-mode message even with the right,
  and the project file is a trust finding; with the right in a trusted project the gates run and
  the result is structured and logged (behaviours 10, 15, 16).

## Out of scope
- A user interface: no tool window, status-bar item or settings page for the gates in this
  version. The result is structured so that a window can show it later.
- Gate watchers shared between agents (ADR-0023 rule 14) and remembering the last verdict.
- Selecting affected tests through the call graph or by test-to-source mapping; the selection here
  is by module, package or folder (behaviour 5).
- Format and inspection gates for Maven and Gradle (no formatter or linter is standard there); a
  project adds them through the project file.
- Change sets and the "done" state of an agent task (ADR-0023 rule 6, ROADMAP M19.4).

## Known deviations from the reference
The reference has no check gates; nothing is copied. Improvements over the usual practice of
running checks through a shell:
- **D1** The default gates run offline (behaviour 3): a gate never downloads build tools or
  dependencies, so a check cannot fetch code from the network.
- **D2** No shell: commands from the project file are run as a program with arguments; changed file
  paths are passed as `./`-prefixed arguments (behaviour 6).
