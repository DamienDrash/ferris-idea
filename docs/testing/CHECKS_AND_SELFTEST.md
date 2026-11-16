# Checks, batch merges and the window self-test

Working notes for the people and agents who merge into the project's own repository. An outside
reader needs only [CONTRIBUTING.md](../../CONTRIBUTING.md). Moved here from CONTRIBUTING.md on
2026-11-14; the rules themselves are unchanged (CLAUDE.md and AGENTS.md rule 6,
[ADR-0030](../adr/0030-ci-strategy.md)).

## The server check

The whole workspace is tested by the project's own CI server: it checks every branch pushed to the
project's working repository as it would be on `main`, merged into `main`'s tip. A branch is merged
only after that check passed; a merge script waits for it and merges. Nobody repeats that run by hand, and nobody waits for it before starting the next task.
The server runs with the language tools switched off: tests that need a language server, a debug
adapter or a database return early there and are reported as skipped, so a green run does not
prove them. `scripts/ci-local.sh` runs the checks together with the slower ones (performance
budgets, app bundle, fuzz smoke, soak).

Several waiting branches are checked as one batch ([ADR-0030](../adr/0030-ci-strategy.md)
decision 10): up to four tips of the same kind (documents only, or code) merged onto `main`'s tip
and checked once. Each member gets its own result, marked `batch=<id> members=<…>`, so pull
requests are still merged one by one. A red batch is halved until the single failing branch is
found; only that branch gets `failure`. A branch that conflicts with another member waits for the
next round. `IDECRAFT_CI_BATCH=1` in the server's `env` checks every branch alone.

## Checking a pull request before the merge

For branches of the project's repository the server check is the gate, and the full macOS
suite runs at night. The coordinator has two scripts for the Mac that avoid repeating that work:

```sh
scripts/check-pr.sh --list          # changed crates, affected crates, the commands; runs nothing
scripts/check-pr.sh                 # fmt, clippy (workspace), tests of the affected crates, docs check
scripts/check-pr.sh --full          # the same with `cargo test --workspace`
scripts/check-batch.sh --dry-run BRANCH…   # do these branches merge together?
scripts/check-batch.sh BRANCH…             # merge them on a temporary branch, build one app bundle
```

- `check-pr.sh` compares the checked-out tree with its merge base (`--base`, default
  `origin/main`). A changed file belongs to the crate whose folder contains it; the crates that
  depend on a changed crate are tested as well. The root `Cargo.toml` or `Cargo.lock`, the
  toolchain file, `.cargo/`, a build script, the shared `tests/` folder, or a file the script
  cannot place make it test the whole workspace. A change to documents only runs
  `scripts/check-docs.sh` alone. It stops at the first failing step and prints one line per step
  with its duration.
- `check-batch.sh` merges `origin/BRANCH` for each named branch, in the given order, onto a new
  local branch `batch/<date-time>` cut from `origin/main`, runs `scripts/bundle-macos.sh` and
  prints the self-test command. `--selftest` runs the window self-test itself, with a 30-minute
  watchdog; it takes over the screen. The script never pushes and never changes `main`; it ends
  with the merged branches and their tips.
- `CHANGELOG.md` and `docs/specifications/TRACEABILITY.md` merge with `merge=union`
  (`.gitattributes`): a local `git merge origin/main` keeps the lines both sides added. GitHub's
  mergeability check does not use this rule, so a pull request can still show a conflict there.
  A branch cut before a version was released can also end up with its `## Unreleased` lines
  inside the released section; `check-batch.sh` lists the section every added changelog line
  landed in, and the lines are moved by hand.

## The window self-test

`idecraft ui --selftest` opens the window on a temporary folder, presses keys and clicks in it and
checks what they did (steps in `crates/app/src/workbench.rs`). It takes over the screen for a few
minutes (a full run of the release bundle on 2026-11-16: under 3 minutes, its groups summed to
149 s). Its last line on stdout is the result, in the form
`{"checks":988,"failed":[],"skipped":[]}` (the number is an example, not the current count): the
number of checks, the names of the failed checks and the groups left out on request. A group whose tools are not installed prints
`<name> steps skipped: <what is missing>` on stderr and has no steps. A few steps click a control
by its test id and run only with `IDECRAFT_SELFTEST_DRIVER=1` (the UI driver of SPEC-0319);
without it the group prints a line naming `IDECRAFT_SELFTEST_DRIVER=1` on stderr, leaves those
steps out, and the result line does not list them.

The steps are in named groups that run in a fixed order:

```sh
idecraft ui --selftest --list-groups                        # the groups with their step counts; no window
IDECRAFT_SELFTEST_ONLY=mouse,tabs idecraft ui --selftest    # only these groups
IDECRAFT_SELFTEST_SKIP=debuggers idecraft ui --selftest     # everything but these groups
```

- `IDECRAFT_SELFTEST_ONLY` and `IDECRAFT_SELFTEST_SKIP` take comma-separated names of groups and
  meta groups. With both set, the run takes the groups of `ONLY` without those of `SKIP`. An
  unknown name ends the run before the window opens and lists the names. Many groups build on
  what earlier groups left in the folder or the window, so a group that passes in a full run can
  fail when it runs alone; the names listed as meta groups and `mouse`, `tabs`, `tooltips`,
  `build-tools` and `containers` run alone.
- Each group left out by `SKIP` prints `<name> steps skipped: IDECRAFT_SELFTEST_SKIP` on stderr,
  and the result line names it under `skipped`. A run with skipped groups is not a full run.
- The meta group **`debuggers`** is every group that starts a debug adapter (lldb-dap, debugpy,
  java-debug, js-debug for Node.js and Chrome, kotlin-debug-adapter), attaches to a running
  program or samples one with the profiler. Reason: on macOS a debugger that takes over another
  process needs the "Developer Tools Access" right. In the session of a standard user the system
  asks for an administrator in a dialog, and while that dialog is open, debugging hangs in every
  session of the Mac. A self-test that runs in a second user's session, so that it does not
  disturb the person at the Mac, therefore sets `IDECRAFT_SELFTEST_SKIP=debuggers`. Adapters that
  only talk to a port of a program they started themselves are in the group as well, so one
  switch turns off everything that debugs.
- A second user's session that is not the one on screen (fast user switching) does not run the
  self-test at all: the window gets no frames there, and the run stops at its first step until
  the time limit (seen on 2026-11-12 with `IDECRAFT_SELFTEST_TRACE=1`, also with
  `IDECRAFT_SELFTEST_SKIP=debuggers`). The self-test needs the session on screen.
- At the end the run prints one line per group on stderr with its duration, slowest first
  (`self-test group <name>: <seconds> s (<n> steps)`), and their sum. `IDECRAFT_SELFTEST_TRACE=1`
  names every group and step as it starts, to find where a run hangs.
