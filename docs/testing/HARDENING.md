# Fuzz and soak tests

**Status:** v1 · 2026-11-03 · roadmap item M8.2

IDECraft reads a lot of input it does not control: language-server and debug-adapter messages,
MCP requests from agents, SQL scripts, terminal output, plugin modules and their answers, and the
files of a cloned project. This page describes the tests that throw malformed input at those
readers and the soak test that drives one IDE process for a long time.

## Fuzz targets

The targets live in `crates/hardening/src/targets.rs`. Each is a plain function over bytes that
must not panic, hang or allocate without bound. Several also check what their reader promises.

| Target | Input | Extra checks |
|---|---|---|
| `lsp_frame` | LSP `Content-Length` framing; hover, signature, location, completion and symbol replies | — |
| `dap_frame` | DAP `Content-Length` framing | — |
| `mcp_jsonrpc` | raw JSON-RPC lines and read-only tool calls against a live MCP server | the server still answers a ping within 20 s |
| `sql_split` | SQL statement splitter, every dialect (SPEC-0005) | ranges in order, inside the script, text equals the slice |
| `terminal_vte` | escape sequences into the screen model, in chunks, with a resize; search and links | cursor inside the screen |
| `text_buffer` | an op script of edits, undo groups, undo, redo, join, reload and positions (SPEC-0003) | text, undo history and modified flag match a plain model; undo all restores the first text |
| `diff_merge` | line diff, side-by-side rows, three-way merge | no conflict when a side is unchanged; none left after resolving all |
| `plugin_module` | arbitrary bytes as a WebAssembly plugin | — |
| `plugin_answers` | plugin manifests and the answers of edit, inspect, complete, status and panel actions; file-type highlighting | edits inside the text, status on one line, highlight runs in order |
| `keymap` | keymap XML, keystrokes, shortcuts | — |
| `theme` | color-scheme XML and UI theme JSON | — |
| `settings` | settings TOML, run configurations, tool-window layout, saved breakpoints, `.editorconfig`, agent grants, recent projects, connection profiles, HTTP environments | — |
| `idea_project` | `.idea` folder: `.name`, misc, modules, VCS mappings, libraries, module files | — |
| `project_model` | Cargo, npm, pyproject, requirements, Maven and Gradle files, lock files | — |
| `http_file` | `.http` request files, variables, URLs | — |
| `test_report` | JUnit XML, TAP, libtest output, Jest JSON | — |
| `profiler_formats` | JFR, sample, perf script, massif, malloc history, tracemalloc, cProfile, heap, `ps` | — |
| `misc_formats` | query plans, dependency trees, requirement specs, `.properties`, project dictionaries, ESLint JSON, argument lists, AI CLI output | spans inside the text |

Git objects are read by `gix`, not by IDECraft, so the Git target covers only what IDECraft
parses itself: diffs and the merge.

The MCP target calls only tools that neither write, run programs nor reach the network, inside a
scratch workspace without granted rights.

### Run the fuzz targets

Stable driver, every target for a few seconds each (part of `scripts/ci-local.sh`):

```bash
scripts/fuzz.sh smoke 3
```

Coverage-guided with libFuzzer. This needs the nightly toolchain and `cargo-fuzz`
(`rustup toolchain install nightly`, `cargo install cargo-fuzz`). It runs at low priority:

```bash
scripts/fuzz.sh libfuzzer 300
```

```bash
scripts/fuzz.sh libfuzzer 600 sql_split terminal_vte
```

List the targets, or replay one input:

```bash
cargo run -p idecraft-hardening --bin idecraft-fuzz -- list
```

```bash
cargo run -p idecraft-hardening --bin idecraft-fuzz -- replay diff_merge crates/hardening/regressions/diff_merge/lone-cr-index
```

Inputs that crash land in `fuzz-crashes/` (stable driver) or `fuzz/artifacts/<target>/`
(libFuzzer). The working corpus `fuzz/corpus/` is not in the repository; the seeds in
`targets.rs` recreate it.

`cargo test -p idecraft-hardening` runs every target on its seeds plus 400 mutated inputs
(100 for MCP) and replays every input in `crates/hardening/regressions/`, one per fixed finding.

## Soak test

`idecraft-soak` starts `idecraft mcp` on a scratch workspace and repeats rounds until the time is
up. A round:

1. saves a workspace file from outside and reads it back through `read_file`;
2. types in an editor and undoes it (`editor_keystrokes`), expecting the saved text;
3. records local history and, every fifth round, restores the previous revision and compares it
   with the disk;
4. alternates SQLite queries (`run_sql`), a shell command in a pseudo terminal
   (`terminal_command`), terminal rendering and SQL splitting, and file search;
5. every tenth round asks the Python language server for diagnostics (`file_diagnostics`).

Every 40 rounds a second, short-lived `idecraft mcp` process starts, runs a terminal command and
a language server, and ends. No process below it may still run 10 s later.

The test fails when:

- a request gets no answer within 60 s (a stuck task) or the server ends;
- resident memory grows more than 96 MiB from the warmed-up level (median of the second fifth of
  the samples) to the end (median of the last fifth);
- open file descriptors grow by more than 24 over the same span;
- child processes grow by more than one, or any process outlives its server;
- a result differs from what the round expects.

### Run the soak test

```bash
scripts/soak.sh 2
```

```bash
scripts/soak.sh 120 --csv soak.csv
```

Other options of `idecraft-soak`: `--secs`, `--seed`, `--workspace`, `--sample-secs`,
`--churn-every`, `--timeout-secs`, `--max-rss-growth-mb`, `--max-fd-growth`, `--no-lsp`.
The run grants the `run` right only in its own settings folder (`IDECRAFT_CONFIG_DIR`), never in
yours.

## Where the runs happen

| Run | When | Time |
|---|---|---|
| `cargo test -p idecraft-hardening` | every `cargo test --workspace` | about 4 s |
| `scripts/fuzz.sh smoke 2` and `scripts/soak.sh 2` | `scripts/ci-local.sh`, so every push on the Mac runner | about 3 min |
| `scripts/soak.sh 120`, `scripts/fuzz.sh smoke 60`, `scripts/fuzz.sh libfuzzer 240` | nightly job `hardening` in `.github/workflows/ci.yml`, or on request | about 3.5 h |

## Findings

Runs on 2026-11-03 (M1 Max): every target with the stable driver (3–60 s each, about
17 million inputs in all), then 180 s per target with libFuzzer (about 7 million executions;
`test_report` again after its fix), and the soak test for 2 min and 30 min.

| Finding | Found by | Fix | Regression test |
|---|---|---|---|
| A huge LSP `Content-Length` allocated its size up front and aborted the process | `lsp_frame` | bodies over 256 MiB are a protocol error; the body is read as it arrives | `lsp::tests::huge_or_cut_length_is_an_error` |
| The same for DAP | `dap_frame` | as for LSP | `protocol::tests::huge_or_cut_length_is_an_error` |
| A lone `\r` made the three-way merge index past its lines (index and slice panics; reachable through MCP `merge_three_way`) | `diff_merge`, `mcp_jsonrpc` | the merge diffs the lines it indexes | `merge3::tests::lone_carriage_returns` |
| MySQL plan text ending in a multibyte character after `  (cost=` was cut inside the character | `misc_formats` | the closing parenthesis is stripped only when present | `plan::tests::mysql_tree_without_closing_parenthesis` |
| A TAP plan `1..64666…` listed every missing test and ran out of memory | `test_report` (libFuzzer) | beyond 1000 missing tests, one entry says how many | `huge_tap_plan_gives_one_missing_entry` |

Each crashing input is also in `crates/hardening/regressions/<target>/`.

Recorded, not changed:

- **A panicking MCP tool never answers.** When a tool handler panicked (the merge finding
  above), the MCP server sent no answer at all, so the client waited until its own timeout.
  The underlying panics are fixed; turning a future panic into an error answer is a follow-up.
- **An offset between `\r` and `\n` has no line/column round trip.** `offset_to_position` gives
  it a column past the line's content, which `position_to_offset` refuses. Carets do not stop
  there in normal editing; the `text_buffer` target skips this case.
- **Resident memory of `idecraft mcp` (debug build) drifts upward slowly.** In the 30-minute
  soak (17,058 rounds, 81,875 requests, 426 short sessions) it rose from 69 MiB after warm-up to
  89 MiB, within the 96 MiB limit. The live heap stayed flat (2.18 MB → 2.23 MB, macOS `heap`)
  and `leaks` found 0 leaked bytes, so the drift is memory the allocator keeps, not data the
  process holds. The nightly two-hour run watches the trend. File descriptors stayed at 16, no
  process outlived its server, and the slowest answer took 3.2 s (`initialize`).
