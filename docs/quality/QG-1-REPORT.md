# QG-1 gate report

- **Gate:** [ADR-0015](../adr/0015-quality-gate-ai-native.md), seven criteria; ROADMAP M9.1
- **Measured on:** `main` at `c92f957c` (2026-11-15 20:05 CET, after #827). Parity and documents in a
  worktree on the Mac (macOS 27.0, arm64); tests from the server CI (the project's build server, Ubuntu
  24.04.5) and from the scheduled run of the CI workflow on the Mac's own runner. **No test run and no
  window run on the Mac in this measurement**, on the coordinator's order (the Project Owner was working
  on the Mac and the load was high); macOS values come from runs already made, each with its commit.
- **Measured:** 2026-11-15, 20:27–20:55 CET, by an agent on the coordinator's order. Second measurement;
  the first was on 2026-11-14 at `40ccdff4` (see [Earlier measurement](#earlier-measurement)).
- **Pending, not counted:** PR #830 (branch `f084-headless`, not on `main`): the new tool
  `replace_text_in_file` and a changed `create_file`.
- **Update 2026-11-16 05:00 CET (coordinator):** criterion 3 measured again. The series of full green `main` runs
  reached 20 of 20 (3d), and the scheduled-workflow run on the Mac's own runner passed on `main` at `df649abc`
  (3a: fmt, clippy, `cargo test --workspace` with the reference databases running, and every headless budget).
  The verdict of criterion 3 and the overall verdict are changed below; nothing else was measured again.
- **Rule:** every value is what was measured; a criterion that is only partly met says so. Nothing is
  rounded up.

## Verdict

| # | Criterion | Threshold | Measured | Verdict |
|---|---|---|---|---|
| 1 | Parity | ≥ 85 % overall; every row of weight 4–5 at stage ≥ 3 | 90.2 %; 20 of 20 weight-4–5 rows at stage 4 | **met, with a named limit** (12 rows were set to stage 4 by the coordinator on 2026-11-14 and 2026-11-15; without them 84.1 %, below the threshold) |
| 2 | Verified parity | ≥ 50 % of the weight at stage 4 | 73.1 % (128 of 175) | **met, with a named limit** (same ratings; without them 52.6 %) |
| 3 | Tests and CI | workspace tests and window self-test pass on macOS and Linux; 20 consecutive CI runs on `main` without a flaky failure | Linux workspace at `c92f957c`: 3703 passed / 0 failed / 32 skipped, 42 early returns; macOS workspace on `main` at `df649abc` (2026-11-16 03:21–03:58 UTC): **passed**, with every headless budget; window self-test macOS 1060/0 (`fdeeb75c`), Linux 1067/0 (`e12d45ea`); series of full green `main` runs **20 of 20** (2026-11-15 22:54:24 UTC) | **met, with a named limit** (the window self-test ran once per system, not on the current `main`) |
| 4 | Performance | typing p95 < 16 ms in a 100 000-line file; cold start and idle memory at or below the reference | from the macOS window run of `fdeeb75c`: typing p95 1.73 ms; start to project window 474 ms; 151 MiB after 10 min idle, growth 0 MiB | **met, with a named limit** (not measured again on `c92f957c`; a true cold start after `purge` was never measured) |
| 5 | Stability | no open blocker or escalation; no known data-loss bug | blockers: none; escalations: none; open questions 9, 33, 37, 38, 41, 42, 43, none a blocker; data-loss bugs: 9 found, 9 fixed, 0 known open | **met, with a named limit** (question 33 is an unexplained window defect; the hand test of #708 passed on 2026-11-18) |
| 6 | Architecture readiness | A1–A3 decided; A2 implemented | A1 decided (ADR-0012 accepted 2026-11-14); A2: of 86 tools that change files, 28 write through the open buffer, 13 refuse a file with unsaved changes, 45 write Ferris IDEA's own or new files to the disk; A3: 316 tools, every changing one checks a right | **met, with named limits** (what still writes to the disk, section 6) |
| 7 | Release | explicit go of the Project Owner (GO-5) | conditional go in the owner's words of 2026-11-05 and 2026-11-14, read by the coordinator | **owner's decision** (effective, in the coordinator's reading, only once 1–6 are met) |

Since 2026-11-16, criteria 1–6 are met, each with the named limits in its row. Criterion 7 is the owner's
go: his words of 2026-11-05 and 2026-11-14 make it conditional on 1–6, so in the coordinator's reading
**QG-1 is reached** with the limits named here (PO-STATUS, decision of 2026-11-16). Before 2026-11-16 criterion 3 was not met:
the series stood at 14 of 20 and `cargo test --workspace` on macOS was red on `main` (two MySQL live tests
with expectations older than the reference schema, fixed by #833).

## 1. Parity

- **Command:** `scripts/parity.sh` · 2026-11-15 20:36 CET · source `docs/research/PARITY.md` at `c92f957c`
- **Output:** `parity: 90.2 %` · `M3: 92.9 % of 18 rows` · `M4: 96.6 % of 11 rows` · `M5: 81.4 % of 11
  rows` · `M6: 86.5 % of 17 rows` · `M7 (verified against the reference): 73.1 %` · `functional tests: 55
  of 57 rows tested, 2 with a named reason`
- **Threshold 85 %:** met (90.2 %).
- **Weight 4–5 rows at stage ≥ 3:** all 20 are at stage 4:

| Row | Function | Weight | Stage |
|---|---|---|---|
| F-003 | Project model from build files | 4 | 4 |
| F-010 | Text editor basics | 5 | 4 |
| F-013 | Syntax highlighting | 4 | 4 |
| F-015 | Find/replace | 4 | 4 |
| F-016 | Search everywhere | 4 | 4 |
| F-020 | Editor tabs, split views | 4 | 4 |
| F-031 | Code completion | 5 | 4 |
| F-032 | Diagnostics / inspections | 5 | 4 |
| F-034 | Go to definition / usages | 5 | 4 |
| F-035 | Python support | 4 | 4 |
| F-036 | JavaScript/TypeScript, frameworks | 5 | 4 |
| F-037 | Java/Kotlin | 5 | 4 |
| F-040 | Spring, Jakarta EE, Micronaut, Quarkus | 4 | 4 |
| F-054 | Result grid | 4 | 4 |
| F-070 | Debugger | 4 | 4 |
| F-072 | Rename | 4 | 4 |
| F-073 | Extract, inline | 4 | 4 |
| F-074 | Git | 5 | 4 |
| F-077 | Run configurations | 4 | 4 |
| F-087 | Window layout and metrics | 5 | 4 |

- **Rows changed since `40ccdff4`** (each a rating of the coordinator under the Project Owner's general
  authority of 2026-11-13, recorded in the PARITY row and the PO-STATUS run entries):

| Row | Weight | Stage | By |
|---|---|---|---|
| F-086 Icon set | 2 | 3 → 4 | #754 (OBS-0049) |
| F-012 Multiple carets, column selection | 2 | 3 → 4 | #822 (OBS-0058) |
| F-030 Language of a file | 2 | 2 → 3 | #787 |
| F-082 AI assistance | 2 | 2 → 3 | #787 |
| F-004 Project files of the reference IDE | 1 | 2 → 3 | #790 |
| F-083 Profiler, Docker, Kubernetes, remote | 2 | 2 → 3 | #790 |
| F-056 ER diagrams | 2 | 2 → 3 | #802 |
| F-058 Schema compare, DDL | 2 | 2 → 3 | #802 |
| F-059 Query plan | 2 | 2 → 3 | #802 |

- **Named limit:** the matrix is a weighted estimate (PARITY.md "Grenzen"). Twelve rows (weight 36) are
  at stage 4 by the coordinator's rating, not the owner's: the ten of 2026-11-14 named in the earlier
  report (F-020, F-051, F-052, F-076, F-011, F-075, F-040, F-014, F-001, F-077), F-086 and F-012. Had they
  stayed at stage 3, parity would be **84.1 %, below 85 %**; without the seven stage-3 ratings of the table
  as well, 81.1 %. Without only the nine changes since `40ccdff4`, 86.6 % (the value of the earlier report).

## 2. Verified parity

- **Command:** the same run of `scripts/parity.sh` (line `M7`) · 2026-11-15 20:36 CET
- **Measured:** 73.1 % — 35 of 57 rows, weight 128 of 175, are at stage 4. Threshold 50 %: met.
- **Named limit:** without the twelve stage-4 ratings of section 1, verified parity is 52.6 % (92 of 175),
  still above 50 %. Which parts of each row were checked by hand next to the reference and which only by
  tests is in its PARITY row; the earlier report lists the rows with the largest test-only part.

## 3. Tests and CI

### 3a. `cargo test --workspace` on macOS

- **Update 2026-11-16: passed.** The CI workflow on the Mac's own runner (`scripts/ci-local.sh`: fmt, clippy,
  `cargo test --workspace` with PostgreSQL, MySQL and MariaDB of the reference environment running, the docs
  check and the headless performance budgets), started by hand on `main` at `df649abc` (after #847), GitHub
  run `43797677863`, 2026-11-16 03:21–03:58 UTC, load average 73 at the budget step: **success**.
  - Every headless budget `ok`, `index_cpu_s` 5.53 s ≤ 11.9 s; one warning within its tolerance:
    `typing_background_ms_p95` 569.7 ms > 500 ms (+30 % allowed). No budget needed a second measurement and
    none was reported as noisy.
  - The two runs before on `main` (2026-11-15 23:50 UTC at `f12d7b9f`, 2026-11-16 01:23 UTC at `e3b8a746`)
    passed every test and failed only `index_cpu_s` (16.1 and 15.9 s): since #768 and #781 the indexing phase
    also counted 40 editor steps. #847 (SPEC-0260 v6) moves them into their own phase; the budget was not
    raised (PO-STATUS, decision of 2026-11-16).
  - The window self-test was not part of this run (`selftest=false`): another agent used the screen.
- **First measurement (2026-11-15), not run then** (the coordinator's order).
- **Last run on `main`:** the scheduled run of the CI workflow on the Mac's own runner
  (`scripts/ci-local.sh`: fmt, clippy, `cargo test --workspace` with the local reference databases
  running), GitHub run `43705598115`, `main` at `6c7e22a7` (after #794), 2026-11-15 10:45–10:59 UTC:
  **failed**. `cargo test` stopped at the first failing test program, `ferris-idea-database --test it`:
  944 tests had passed, 2 failed, 1 ignored; the rest of the workspace did not run.
  - `introspect::mysql_reference_schema` (`crates/database/tests/it/introspect.rs:144`) expects no
    foreign keys on MySQL's `stock` table (its comment: MySQL ignores the column form of `REFERENCES`).
    The reference database has two, `sku → item.sku` and `warehouse_id → warehouse.id`: the MySQL schema
    of the reference project declares them as table constraints since #414 (2026-11-07), and the
    reference environment's database script adds them to an older database. Introspection reports
    keys that exist.
  - `plan::mysql_plans` (`crates/database/tests/it/plan.rs:93`): the plan of the test query carries no
    "full scan" hint; probably the same cause (the foreign keys give the optimizer an index), not
    checked.
  - **Class:** test expectations older than the reference schema; not flaky, no product defect found.
    Linux cannot see it: the server CI has no MySQL, so both tests return early there.
  - **Fix (after this measurement):** #833 updates both tests to the reference schema and makes the shared-server check of `introspect` ignore other tests' scratch tables (a flaky `mariadb_reference_schema`, 1 of 4 full runs, found while checking); `ferris-idea-database --test it` 104/104 in five full runs on the Mac. Criterion 3 still needs a green scheduled macOS run after #833.
- **Earlier scheduled runs on `main`:** 2026-11-14 10:16 UTC at `1f159057` failed
  (`remote_project::a_folder_on_an_ssh_host_opens_survives_breaks_and_stays_private` timed out, the open
  flaky test of the earlier report; fixed by #761 the same evening); 2026-11-13 10:52 UTC at `ae3b2903`
  failed (`run_stop::a_child_left_behind…`, fixed by #676); 2026-11-12 green.
- **Last full pass on macOS:** the measurement of 2026-11-14 at `40ccdff4`, 3610 passed / 0 failed / 4
  ignored, without the reference databases (the MySQL tests returned early then).
- **Verdict 3a: met** on `main` at `df649abc` (2026-11-16); not met at the first measurement.

### 3b. `cargo test --workspace` on Linux (server CI)

- **Source:** the result file and the run log of `c92f957c` on the build server, read 2026-11-15
  20:32 CET (read only).
- **`main` at `c92f957c`:** `success full origin/main 1068 - fmt=21,clippy=53,build=74,test=875,doctest=41,docs=2
  skipped-live=42` (2026-11-15 19:23:48 UTC, a run of the series loop, 3d). nextest: `Starting 3703 tests
  across 87 binaries (32 tests skipped)` · `3703 tests run: 3703 passed (1 slow), 32 skipped`. Doc tests: 28
  crates, none with a doc test.
- **The 32 skipped:** the tests named in the build server's skip list (27 lines since #739, each with the
  tool the build server does not provide: lldb-dap 6, debugpy 5, python3-venv 2, groovy-language-server 2,
  lemminx 2, and one each for R, bundler/solargraph, dbus/gnome-keyring, lua-language-server, marksman,
  metals, ngserver with the Angular project, ruby-lsp, taplo and the Vue project) and the `#[ignore]` tests.
- **The 42 early returns** (passed in nextest, listed by the CI as `skipped live tests: 42`): the
  reference databases are not running (10), Kotlin's Gradle cache (8), Go and dlv (6), the JavaScript tools
  of the reference environment (3), Chrome (3), Docker and Kubernetes (3), and 9 single ones (no X11
  session, no `DISPLAY`, no valgrind, no clangd on the host, macOS `sample`, sourcekit-lsp, TypeScript 7,
  a case-sensitive `/tmp`, no `.idea` in `sql-basic`).
- **Verdict 3b: met** (0 failed), with the named limit that Linux checks less than the Mac (32 skipped, 42
  early returns; the earlier report had 5 skipped and an unknown number of early returns).

### 3c. Window self-test `ferris-idea ui --selftest`

- **Not run in this measurement.**
- **macOS:** #791, `main` at `fdeeb75c` (after #776 and #785), 2026-11-15 09:22–09:42 CET, release bundle,
  nobody else on the screen: `{"checks":1060,"failed":[],"skipped":[]}`, 120 groups, all 8 window budgets
  `ok`. Not covered: run-config-folders and run-config-templates (need `FERRIS_IDEA_SELFTEST_DRIVER=1`), the
  Ruby server steps (no Ruby language server on the Mac).
- **Linux:** #776, branch `linux-selftest-mouse` at `e12d45ea` (merged as `9254c08c`), 2026-11-15
  02:31–02:37 CET in a Lima VM, X11 on Xvfb: `{"checks":1067,"failed":[],"skipped":[]}`
  ([QG-1-LINUX-WINDOW.md](QG-1-LINUX-WINDOW.md)). Not covered: browser-debug (no Chrome for Linux on
  ARM), the two run-config groups (driver), profiler-attach samples nothing (`perf_event_paranoid`).
- **Named limits:** one run each, not a series; neither on `c92f957c` (32 and 41 commits later).
- **Verdict 3c: met, with a named limit.**

### 3d. 20 consecutive CI runs on `main` without a flaky failure

- **Source:** the series script of the build server (reads the service log and `streak.log`), run
  2026-11-15 20:31 CET; `streak.log`, `streak-loop.log`.
- **How it counts:** only full own runs of `main`; a cached result, a docs-only check and a branch run do
  not count; any failed run of `main` ends the series; the CI's nextest profile retries nothing, so a
  flaky test fails its run.
- **Series: 20 of 20** full green runs of `main`; the twentieth ended 2026-11-15 22:54:24 UTC on `aad673a4`,
  and the loop ended at 22:57 UTC (`streak-loop.log`: series reached). Runs 15–20: 20:00 `c92f957c`,
  20:20 `a514c77f`, 21:05 `19ee2b99`, 21:46 `24bde752`, 22:30 `978733e3`, 22:54 `aad673a4` (UTC; 1049–1362 s
  each; report `qg1-serie.md` on the build server). At the time of the first measurement it stood at 14.
- **The runs:** 2026-11-13 18:57 `e198de58`, 19:57 `4cc20fdb`, 21:15 `40c294de`; 2026-11-14 14:14
  `8ceee5f4`, 20:10 `639e0e23`, 23:19 `477718e9`; 2026-11-15 02:31 `135ec9ae`, 05:58 `6f0dd7f9`, 10:04
  `9e993335`, 16:51 `e31fab89`, 17:39 `398adb30`, 18:22 `11d5dc85`, 19:04 `9f4b0f64`, 19:23 `c92f957c`
  (UTC; 587–2023 s each). Since 17:25 CET the loop starts one run 25 minutes after the last start (PO-STATUS,
  decision of 2026-11-15).
- **What ended the last series:** the failure on `main` of 2026-11-13 16:45:33 UTC at `04a91a6e`
  (`kotlin_semantic_tokens_with_the_real_server`, a load failure fixed by #691; the script calls it `red`
  because that tree never passed a full run). No run of `main` in the server CI failed since.
- **Named limit:** the series counts the server CI. The Mac's scheduled runs of `main` (3a) are not part
  of it; within the series' period one of them failed by a flaky test (2026-11-14, the SSH project test,
  fixed by #761) and one by the stale MySQL expectations (2026-11-15).
- **All failures on any branch since the earlier report** (2026-11-14 11:30 UTC – 2026-11-15 19:31 UTC:
  164 results, 157 in the service log and 7 in `streak.log`; 14 failures):

| Time (UTC) | Branch | Step | Cause | Class |
|---|---|---|---|---|
| 8 results | 8 branches | merge | branch does not merge into main's tip | merge conflict |
| 11-14 14:47 | readme-and-public-presentation | test | `remote_projects_of_windows_are_seen_and_acted_on_by_agents`: the closed project was still listed | timeout/flaky — product race (fixed by #826) |
| 11-14 23:35 | ci-streak-own-log | test | the log was replaced by a later green run of the same commit; cause not recorded | unknown |
| 11-14 23:56 | ai-assistant-handtests | test | `gradle_build_and_junit_tests`, `gradle_resolves_classpaths`, `libtest_from_real_cargo`: sccache and Gradle daemons of another `/tmp`; `scans::ak11_ratchet…`: 39 marks, 26 allowed | environment (fixed by #772); ratchet |
| 11-15 12:37 | db-handtest-retest | test | `scans::ak11_ratchet…` | ratchet |
| 11-15 14:28 | queue/801 | test | `menu_bar` tests: `super-[` instead of `cmd-[` | real test failure (branch) |
| 11-15 16:31 | f038-headless | test | `remote_projects_of_windows_are_seen_and_acted_on_by_agents`: the closed project was still listed | timeout/flaky — product race (fixed by #826) |

- **Verdict 3d: met** (20 of 20 on 2026-11-15; at the first measurement 14 < 20). No failure of `main` in the server CI since 2026-11-13 16:45 UTC; the
  two flaky failures on branches are one product race, fixed on `main` by #826.

## 4. Performance

- **Not measured again.** The values are those of the macOS window run of #791 (`main` at `fdeeb75c`,
  2026-11-15 09:22–09:42 CET, `scripts/ci-window.sh --force`, load average 212 at the start and 4.7 at the
  end); budgets in `crates/bench/budgets.toml` (SPEC-0260 v3).

| Metric | Budget | Reference (OBS) | Measured | Verdict |
|---|---|---|---|---|
| Typing, keystroke to painted frame, p95 (100 000-line file, 220 keystrokes) | < 16 ms | — | **1.73 ms** (p50 1.11, p99 4.39, max 18.15; 1 of 220 over 16 ms) | met |
| Start to project window, empty project | 1000 ms | 1.5 s warm, 2.5–2.7 s cold after `purge` (OBS-0002) | **474 ms** (warm file cache) | met for the warm start; cold start not measured |
| Project window to idle | 10 s | 87–164 s (OBS-0002) | **1.1 s** | met |
| Memory after start | 300 MiB | 2.2–2.6 GB (OBS-0002) | **151 MiB** | met |
| Memory after 10 min idle / growth | 300 MiB / 10 MiB | ~3.0 GB (OBS-0001) | **151 MiB / 0 MiB** (idle CPU 0.49 %) | met |
| Scrolling (spike-scroll, 900 frames) | ≥ 50 fps | — | 119.0 fps, interval p95 9.19 ms | met |
| Two-window layout (spike-layout, 100 000 table rows) | ≥ 50 fps | — | 120.0 fps per window | met |

- **Window budget check of `ci-window.sh`:** all 8 window budgets `ok`. The headless benchmark was not run.
- **Named limits:** not measured on `c92f957c`; the cold start after `purge` (a measurement by hand that
  needs `sudo`, SPEC-0260) was never done. The warm start is below the reference's cold start, which is not
  the same measurement.
- **Verdict:** met for typing latency and idle memory; start met as a warm start, the cold start open.

## 5. Stability

- **Source:** PO-STATUS at `c92f957c`, sections "ESKALIERT-Flags", "Blocker", "Offene Fragen"; searched the
  PO-STATUS run entries of 2026-11-14 and 2026-11-15 and `CHANGELOG.md` `## Unreleased` for data loss
  (unsaved changes, file writes, database edits).
- **ESKALIERT-Flags:** *(keine)*. **Blocker:** *(keine)* — BL-1 lifted 2026-10-31.
- **Open questions — none is a blocker:**
  - **9** (points of the reference Computer Use cannot operate): hand tests; the owner tests them himself
    (2026-11-09).
  - **33** (Go to File and Recent Files empty after "Trust Project"): seen once, not reproduced in three
    window runs, no cause found, no test. A window defect, not data loss.
  - **37** (keychain access for test bundles): partly overtaken — #802 retested hand tests 37, 120 and 121
    in the window and set F-056, F-058 and F-059 to stage 3; the question of a fixed signature for test
    bundles is still listed.
  - **38** (history of the menu-bar branch): settled by the fresh public history while the working
    repository stays private.
  - **41** (passwords of the reference IDE's data sources): limits database comparisons next to the
    reference (OBS-0051 L6 stays "to be clarified"); needs the owner, not a defect of Ferris IDEA.
  - **42** (Islands Light in the reference): F-085 stays at stage 3 with the named gap.
  - **43** (brand licence before publication): keeps the export dry run for GO-4 red; it concerns the
    publication, not QG-1.
  - **39** and **40** are answered and still listed.
- **Data-loss bugs found and their state** (the first seven from the earlier report, the last two found
  since):

| Bug | Fixed by | State |
|---|---|---|
| ⌘Q and Quit ended with unsaved files without asking; `quit --save-all` did not save; runs left orphaned (D51, D57, D58) | #279 | fixed |
| Project room: a file being written by another program was read half-written, merged and overwritten | #553 | fixed |
| Merge conflicts: cases where resolving lost content (OBS-0038: binary file, deleted side) | #628 | fixed |
| A wrong-typed value in `settings.toml` reset all settings | #656 | fixed |
| A file in a second editor group with unsaved changes could be renamed or moved away from its tab | #689 | fixed |
| The close button of a project window closed at once, unsaved changes were lost | #708 | fixed (hand test SPEC-0094 AK-9 passed in the window on 2026-11-18: the close button asks “Save changes before closing …?” with the file name; Cancel keeps the window, Save All and Close writes the file and closes; then the folder chooser of behaviour 9 opens) |
| Saving a non-UTF-8 file could write characters its encoding cannot hold | #710 | fixed |
| Renaming a database object also changed `.sql` files bound to another connection (OBS-0051 L1) | #775 | fixed |
| The migration script of Compare Schemas ran on the connection selected in the Database window, often the source, instead of the target | #802 | fixed |

- **Verdict:** met, with the limits named above.

## 6. Architecture readiness

- **Source:** ADR-0012 and the ADR index, `docs/architecture/READINESS.md` (updated in this branch),
  SPEC-0220 v5, SPEC-0053 v8, `crates/mcp/src/effects.rs`, `crates/mcp/src/open_files.rs`, the ratchets
  `crates/mcp/tests/it/effects.rs` and `crates/mcp/tests/it/open_files.rs` (both passed in the server CI run
  of `c92f957c`, 3b).

### A1 — domain operations outside MCP and views

- **Decided.** ADR-0012: "Status: accepted · 2026-11-14 · decided by the coordinator under the Project
  Owner's general authority of 2026-11-13"; rules 1–4 apply now, rule 5 (capability layer, `ferris-idea serve`
  as the core for all callers) stays planned for M10. The ADR index has the row as accepted on 2026-11-14;
  merged with #750 (`44949feb`, 2026-11-14 16:03 CET); READINESS A1 says so.
- **Applied in new code:** rule 2 in `crates/session/src/rpc.rs` (stable error codes) and
  `crates/session/src/transport.rs` (cancellation handle); rule 4 is SPEC-0220; file operations of the MCP
  tools live in `ferris_idea_core::file_ops`, refactoring plans in `ferris_idea_languages`.
- **Named limits:** no test enforces rules 1–3; many tool bodies still live in `crates/mcp/src/lib.rs`
  (rule 1 lets them move when they are touched). The decision is recorded in the ADR and in #750, not as an
  entry under PO-STATUS "Freigaben und Entscheidungen".

### A2 — one document service for open buffers

`crates/mcp/src/effects.rs` lists **316 tools**; **86** name `files` as what they change. The ratchet
`open_files::every_file_changing_tool_edits_open_files_through_the_buffer_or_refuses` keeps each of them in
exactly one list of `crates/mcp/src/open_files.rs`. What each does with a file open in the window, saved or
not:

- **Through the buffer (28)** — SPEC-0220 behaviours 6–8 and 13–19: the tool reads the text the editor
  shows and hands the window one edit (one undo step; saved when the buffer was saved, left unsaved
  otherwise); a change of several files is all or nothing. `apply_code_action`, `change_signature`,
  `compare_files`, `delegate_methods`, `eslint_file`, `extract_interface`, `extract_superclass`,
  `file_properties`, `fix_inspection`, `format_file`, `generate_code`, `git_line_change`,
  `git_resolve_conflict`, `git_rollback`, `move_file`, `optimize_imports`, `override_methods`,
  `prettier_file`, `pull_members_up`, `rename_sql_object`, `rename_symbol`, `resource_bundle`,
  `restore_local_revision`, `safe_delete`, `suppress_inspection`, `surround_with`, `undo_refactoring`,
  `update_copyright`. Parts of six of them still refuse a file with unsaved changes instead of editing it:
  `file_properties` `convert_encoding`, `reload_encoding` and `set_read_only` (behaviour 21); `git_rollback`
  of an added file and `git_resolve_conflict` taking a whole side (behaviour 18); `move_file` for the moved
  file itself, a plain move and folders (behaviour 16); `undo_refactoring` of a group that renamed files
  (behaviour 15); `compare_files` for a file the file system marks read-only (behaviour 17). After the
  database renamed an object, `rename_sql_object` leaves the database renamed and the files unchanged when
  the window then refuses the edit (behaviour 19).
- **Refuses unsaved (13)** — behaviours 9 and 22: with unsaved changes the call refuses ("save it first")
  and changes nothing; with the file open and saved, the tool **writes the disk** and the editor takes the
  change as one undo step (SPEC-0058 behaviour 10). `delete_file`, `rename_file`, `git_branch_action`,
  `git_cherry_pick`, `git_commit_action`, `git_merge`, `git_pull`, `git_rebase`, `git_stash`,
  `git_switch_branch`, `er_diagram`, `export_file`, `export_store_result`.
- **Own files (45)** — behaviour 11: they **write the disk**. An editor that shows such a file reloads it
  when it was saved and refuses its next save once when it has unsaved changes, so nothing is lost
  silently, but the agent's write does not reach the buffer.
  - Ferris IDEA's own data and settings in the project (`.ferris-idea/…`, `.idea/…` profiles and dictionaries):
    `build_task`, `build_tool_links`, `build_tool_options`, `changelists`, `debug_renderers`, `file_type`,
    `git_branch_favorite`, `highlighting_level`, `import_file`, `inspection_profile`,
    `project_dictionary`, `python_select_interpreter`, `remove_bookmark`, `set_bookmark`,
    `remove_connection`, `save_connection`, `save_store`, `run_configuration_edit`, `set_breakpoint`,
    `set_special_breakpoints`, `sql_file_data_source`, `tool_windows` (22).
  - New files and folders only: `clone_repository`, `copy_file`, `create_file`, `new_project`,
    `python_create_venv`, `sql_console`, `ui_trace` (7).
  - The user's settings folder: `import_connector`, `window_layouts` (2).
  - SSH hosts and rooms, whose documents the backend owns (ADR-0026): `remote_create`, `remote_delete`,
    `remote_rename`, `remote_write`, `remote_project_close`, `room_apply`, `room_join`, `room_restore`,
    `room_undo`, `room_version` (10). `remote_write` writes a file's content on the host with a stamp check
    against changes on the disk; it does not look at a remote project's open documents
    (`remote_documents`).
  - The window's own steps and Ferris IDEA's own record: `close_project`, `quit_ide`, `file_operation_undo`,
    `local_history` (4).
- **Tools without `files` (230):** for the 143 that only read, the effects ratchet fails on a direct write
  call in their code (it passed). For the 87 that change something else, the same scan of the MCP sources,
  made for this report, finds three with write calls, all writing the user's settings and classified so:
  `set_ide_settings`, `set_shortcut`, `ui_driver_stop`. Writes inside the domain crates are not visible to
  such a scan; there the `files` column rests on each row's classification (SPEC-0053 v8 behaviour 13).
- **Reading:** `read_file` and `open_documents` read the registry (behaviour 6); search, diagnostics and
  symbols still read the disk (SPEC-0220 "Out of scope").
- **In a running window:** checked once, 2026-11-14 20:23 at `c48507b6` (`format_file` over HTTP on a file
  with an unsaved line: edited in the buffer, the line kept, the disk unchanged, one ⌘Z back), before
  SPEC-0220 v3–v5.
- **Pending, not counted:** #830 adds `replace_text_in_file` (writes through the open editor, SPEC-0006 v54
  behaviour 8) and gives `create_file` a text and a dry run (still never overwrites).
- **Verdict A2:** implemented as SPEC-0220 v5 decides it: one document owner, and every tool that changes
  files writes through it, refuses an unsaved file, or writes an own or new file. **Named limits:** 13 tools
  and parts of 6 more do not write through the buffer (they refuse unsaved files and write the disk under a
  saved editor), and 45 tools write the disk by design.

### A3 — writes need a right

- **Met.** `crates/mcp/src/effects.rs` names the change, dry run and checks of all 316 tools;
  `effects::every_tool_with_a_changing_effect_checks_a_right` passed in the server CI run of `c92f957c`.
  Two named exceptions without a right: `ui_driver_stop` (emergency stop) and `local_history` (Ferris IDEA's
  own record). Known limits that need the Project Owner: SPEC-0053 v8 behaviour 14. The ratchet's scan for
  write calls covers only tools classified as reading.

## 7. Release (GO-5)

Owner's decision. The owner's words in PO-STATUS: on 2026-11-05 the overnight order, recorded as an
advance go for GO-5 "sobald das Gate erfüllt ist"; on 2026-11-14 "Setze das quality Gate qg-1 um / Ziehe
die Datenbankeerkzeuge nach / Danach Go-4 / Dann ai native Phase und Fernsteuerung, Mobile Apps und
Web-IDE starten", which the coordinator reads as GO-5 effective once this report shows the criteria met.
With criterion 3 not met, that point has not come.

## Proposed tasks for what is not met

- **Done 2026-11-16:** criterion 3 (series 20 of 20, green macOS run at `df649abc`). Kept for the record:
- **Criterion 3, series (3d):** let the series loop on the build server run; when it reaches 20, put the
  count and the time of the twentieth run into the series line of 3d and change criterion 3 in the verdict
  if 3a holds by then.
- **Criterion 3, macOS workspace (3a):** bring the two MySQL live tests in line with the reference schema
  (`introspect::mysql_reference_schema` expects the two foreign keys, as the PostgreSQL test does;
  `plan::mysql_plans` with a query that still reads a table whole), then one green `cargo test --workspace`
  on macOS on `main` with the reference databases running (the scheduled run, or a run by hand on a quiet
  Mac).
- **Criterion 4:** measure the cold start after `purge` by hand (SPEC-0260).
- **Criterion 6 (named limits, not needed for the gate):** SPEC-0220 "Out of scope": the 13 refusing tools
  through the buffer (Git operations of the working tree, deleting and renaming, exports), the encoding
  changes, a moved file's editor following the move, the reading tools on open buffers; for own files, an
  agent's write that reaches an open editor's buffer; `remote_write` through a remote project's documents;
  merge #830.

## Earlier measurement

The first measurement, 2026-11-14 on `main` at `40ccdff4` (#746, with the addenda of #762, #771, #776 and
#791), is in this file's history. What changed:

| | 2026-11-14 at `40ccdff4` | 2026-11-15 at `c92f957c` |
|---|---|---|
| Parity / verified | 86.6 % / 70.9 % | 90.2 % / 73.1 % |
| Linux workspace | 3502 passed / 0 failed / 5 skipped | 3703 passed / 0 failed / 32 skipped, 42 early returns |
| macOS workspace | 3610 passed / 0 failed / 4 ignored (by hand, without reference databases) | not run; the scheduled run of `6c7e22a7` failed 2 MySQL live tests |
| Window self-test | macOS timed out; Linux never run (later: macOS 1056/1057, then 1060/0; Linux 1067/0) | macOS 1060/0, Linux 1067/0 (one run each, not on `c92f957c`) |
| Green runs of `main` | 3 full (57 with cached and docs) | 14 full of 20 (cached and docs no longer count) |
| A1 / A2 | ADR-0012 proposed / reading only | accepted / writing through the buffer or refusing (SPEC-0220 v5) |
| Tools in the effects table | 312 | 316 |

## Time taken

| Measurement | When (CET) | Took |
|---|---|---|
| Effects table, `open_files.rs`, SPEC-0220, tool handlers (A2) | 20:27–20:47 | in between |
| Server CI: series, results, run logs, service log (ssh, read only), classification | 20:30–20:45 | about 15 min |
| Parity (`scripts/parity.sh`, with `functional.sh --check`) | 20:36 | under 1 s |
| Scheduled Mac runs of `main` (GitHub run logs), MySQL test cause | 20:37–20:43 | about 6 min |
| Stability review, this report, READINESS, `scripts/check-pr.sh --changed-only` | 20:43–20:55 | about 12 min |
