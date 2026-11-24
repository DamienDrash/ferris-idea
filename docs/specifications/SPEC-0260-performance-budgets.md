# SPEC-0260: Performance budgets measured in CI

- **Version:** 6 · **Date:** 2026-11-16 (v6: the editor's steps for unused declarations in their
  own phase; the indexing CPU budget covers the file list, the symbol searches and the reference
  index; CPU time per part in the history; the editor phase keeps the memory bound it had inside
  indexing, one budget row); 2026-11-15 (v5: the same step for a Kotlin file, one budget row,
  SPEC-0098 v13 behaviour 27); 2026-11-14 (v4: the editor's unused declarations after an edit, one
  budget row and behaviour 2, SPEC-0098 v12 behaviour 26); 2026-11-04 (v3: budget values confirmed by the product owner on 2026-11-04, PO-STATUS question 19; v2: overloaded runs, behaviour 1)
- **Sources:** OBS-0001 (idle memory of the reference), OBS-0002 incl. the cold-start addendum of
  2026-11-03 (start, idle, memory), OBS-0003 (indexing `java-large`), IMP-0003, IMP-0063, ADR-0003
  (typing latency criterion), DOC (public documentation of the reference: size limits above which code
  insight is switched off and files are opened read-only in parts; not observed), GEN (frame times of a
  60 Hz display, measuring processes from outside with `ps` and CPU-time deltas, cost of a Tree-sitter
  tree per byte of text), STD (Tree-sitter queries and incremental parsing)
- **Feature matrix rows:** F-001 (Grundmessung, Block 0)

## Purpose
Every change is checked against stated budgets for typing, start, memory, large files and large
projects, so Ferris IDEA stays faster and leaner than the reference measured in OBS-0001 to OBS-0003 and
regressions fail CI instead of being noticed by users. The budgets also fix how Ferris IDEA treats very
large files and projects.

## Budgets
The bar is the reference's measurement or an improvement target; the budget is Ferris IDEA's limit. A
value above the budget is reported as a warning, a value above budget plus tolerance fails CI. The
machine-readable form is `crates/bench/budgets.toml`; both change together.

| Metric | Budget | Tolerance | Bar | Where |
|---|---|---|---|---|
| Keystroke → painted frame, p50 / p95 / p99 (100 000-line file) | 8 / 16 / 25 ms | 50 / 25 / 25 % | ADR-0003: one 60 Hz frame | window |
| Keystroke, synchronous model work p50 / p99 (same file) | 1 / 4 ms | 50 % | a quarter frame, the rest for layout and paint | headless |
| Re-parse, inspections and folds after an edit, p95 (same file) | 500 ms | 30 % | the reference drops code insight above 2.5 MB (DOC); this file is 4.6 MB | headless |
| Start to project window, empty project | 1000 ms | 30 % | reference 1.5 s empty project, 1.9 s warm with a project (OBS-0002) | window |
| Project window to idle (< 5 % of a core for 10 s) | 10 s | 20 % | IMP-0063; reference 87–164 s after start (OBS-0002) | window |
| Resident memory when idle after start, empty project | 300 MiB | 10 % | IMP-0003; reference 2.2–2.6 GB (OBS-0002) | window |
| Resident memory after 10 min idle / growth | 300 / 10 MiB | 10 / 50 % | IMP-0003; reference ~3.0 GB (OBS-0001) | window |
| Cold file cache, extra time to the project window | 0.3 s | — | IMP-0063; reference +0.7 s (OBS-0002) | by hand |
| Open a file over 50 MB (log; SQL dump) | 2000 ms each | 30 % | the reference opens such files read-only in parts (DOC) | headless |
| Page jump in it, p95 | 4 ms | 50 % | a quarter frame | headless |
| Find all / regex find all in it | 1000 / 1500 ms | 50 % | GEN | headless |
| Peak memory opening both files one after the other | 1024 MiB | 25 % | GEN | headless |
| File list of `java-large` (5000 files, 485 000 lines) | 1000 ms | 50 % | GEN | headless |
| First symbol search over `java-large` | 5000 ms | 30 % | reference ~10 s load on first open (OBS-0003) | headless |
| Every further symbol search | 100 ms | 50 % | answers while typing | headless |
| CPU time of indexing `java-large`: file list, both symbol searches and the reference index (v6) | 11.9 s | 20 % | reference 56 s first open, 11.9 s warm (OBS-0003) | headless |
| Peak memory of indexing `java-large` | 500 MiB | 20 % | reference grows 0.2–0.5 GB (OBS-0003) | headless |
| Unused declarations of a 100 000-line Java file in the editor after an edit, p95, against the reference index of `java-large` (v4) | 500 ms | 30 % | the re-parse budget above: problems follow within 0.5 s | headless |
| The same for a 100 000-line Kotlin file (v5) | 500 ms | 30 % | as the Java row | headless |
| Peak memory of the editor phase (both files and the reference index; v6) | 500 MiB | 20 % | the bound these steps had inside the indexing phase (v4, v5) | headless |

## Behaviour
1. **Headless measurement.** `ferris-idea-bench run` (release build) measures every headless budget and
   fails when one is exceeded beyond its tolerance or not measured. Each phase (typing, large files,
   indexing, editor; v6: the editor is its own phase) runs in a child process whose peak resident
   memory and CPU time are sampled every 50 ms from outside with `ps` (`{phase}_peak_rss_mib`,
   `{phase}_cpu_s`). v6: a phase also reports the CPU time of its parts, as deltas of its own CPU
   time read with `ps` at the part boundaries (`…_cpu_s`, in the history, no budget unless the table
   names one). A phase with a failing value is measured again, up to two more times, and
   the lowest value of each metric counts, because load from other processes only makes timings
   slower. When the one-minute load average is above twice the number of cores (the CI Mac is shared
   with other builds), a wall-clock budget (`…_ms`) that still fails is reported as noisy with the load
   and does not fail the run; memory and CPU-time budgets always fail. The history keeps the load of
   every run. [GEN]
2. **What is measured headless.** The work the editor view does, on its own code paths:
   - typing: 230 keystrokes (text with umlauts, Backspace, Down, Enter) at the start of a statement
     inside a function in the middle of a generated 100 000-line Rust file; per keystroke the
     synchronous part (edit, snapshot for the document registry, highlighting the visible page) and,
     every five edits as when edits queue up during a parse, the background part (incremental
     re-parse, inspections, fold regions);
   - large files: a 50 MB server log and a 50 MB SQL dump — the synchronous open (read, buffer, copy for
     reconciling with the disk, plain-text inspections), 200 jumps to random pages, find-all of a
     literal and of a regular expression as the find bar runs it;
   - indexing: `java-large` as generated by `scripts/gen-java-large.sh` (the project of
     OBS-0003) — the file list, a first symbol search (`Item042`, all 50 classes found) and a second one;
     v4: the reference index of `java-large` built once (`index_reference_ms`, no wall-clock
     budget: it runs in the background when the window opens its first Java file). v6: the phase
     is what opening `java-large` and its first Java file costs, and `index_cpu_s` counts all of
     it: the file list, both symbol searches and the reference index. The reference index stays in,
     because the window builds it as soon as the first Java file is open and the bar is the
     reference's open, after which the whole project is usable (OBS-0003); an IDE's project index
     also answers where names are used (GEN).
     The CPU time of the file list with both searches (`index_symbols_cpu_s`) and of the reference
     index (`index_reference_cpu_s`) is recorded without a budget of its own;
   - editor (v6: its own phase, before in indexing): the reference index of `java-large` built
     again, because the steps need it held in the same process, then 20 edits of a generated
     100 000-line Java file that is not on disk, each re-parsed and followed by the editor's step
     for its unused declarations (v4, SPEC-0098 v12 behaviour 26) against that index
     (`editor_unused_ms_p95`); v5: the same 20 edits and steps for a generated 100 000-line
     Kotlin file (SPEC-0098 v13 behaviour 27, `editor_unused_kotlin_ms_p95`). The CPU time of each
     file's 20 edits, re-parses and steps (`editor_unused_cpu_s`, `editor_unused_kotlin_cpu_s`,
     after the file's first parse) and of the whole phase (`editor_cpu_s`) is recorded without a
     budget: each step runs on one thread and its wall-clock p95 is budgeted. The phase's peak
     memory keeps the 500 MiB bound it had inside indexing (`editor_peak_rss_mib`).
   Fixtures are generated once into `$FERRIS_IDEA_BENCH_DIR` (default `~/.cache/ferris-idea-bench`). [GEN]
3. **Window measurement, nightly.** `scripts/perf-window.sh` starts the release build of `ferris-idea ui`
   on an empty folder with its own settings folder and in its own process group, after one
   `ferris-idea --version` (macOS checks a newly linked executable at its first start, which is not the
   app's start time), and measures from outside, as OBS-0002 measured the reference: wall clock from the launch to the first window at least 1000 px wide; CPU share from
   CPU-time deltas once per second until it stays below 5 % of one core for 10 s; resident memory then
   and after 10 minutes without input. Keystroke-to-frame latency comes from `ferris-idea spike-editor`
   (M3.4). `scripts/ci-window.sh` runs both at night when the Mac is idle and checks the window budgets
   with `ferris-idea-bench check window`. [GEN, OBS-0002]
4. **Large texts in the editor.** A text over 1 MiB gets its syntax tree, inspections and fold regions on
   a background thread when it is opened and shows plain text until they arrive; opening never waits for
   a parse. A text over 8 MiB gets no syntax tree: a tree costs up to ~45 bytes per byte of text (a
   50 MB SQL dump: 2.2 GB), so such files are edited, scrolled and searched as plain text. For every
   file, inspections and fold regions after an edit are computed with the re-parse on the background
   thread; the main thread only installs them, and results for a text that has changed since are
   dropped. [GEN, IMP-0260]
5. **Symbol search over a large project.** Search Everywhere keeps the definitions of each file between
   queries while the file's size and modification time stay the same, and parses the other files on all
   cores (at most 8 threads). A cancelled query returns no symbols. The cache starts over above 200 000
   files. [GEN]
6. **No quadratic work on large texts.** Converting highlight spans, comment ranges, find matches and
   fold regions to positions is linear in the text: offsets are counted on from the previous position,
   never from the start of the text, and no tree node is asked for its parent. [GEN]
7. **History.** Every headless run and every nightly window run appends one JSON line (date, commit,
   host, one-minute load average, mode, metrics) to `history.jsonl` in `$FERRIS_IDEA_BENCH_DIR`, outside the
   repository; the numbers quoted in `ROADMAP.md` and the status file come from there. [GEN]
8. **Cold start.** The extra time with an empty file cache needs `sudo purge`, which CI cannot run; it is
   measured by hand with `scripts/perf-window.sh` right after `purge` and recorded in the status file.
   [GEN, OBS-0002]

## Acceptance criteria
- **AK-1** `crates/bench/budgets.toml` parses, names every metric once, and the check reports a value at
  the budget as within, above budget but inside the tolerance as warning, beyond it as failure, and a
  missing metric as not measured.
- **AK-2** `ferris-idea-bench run` produces every headless metric of the table, appends to the history and
  passes on the self-hosted runner as a step of `scripts/ci-local.sh`.
- **AK-3** The nightly window job measures start, idle and memory of the release build and the typing
  latency and fails when a window budget is exceeded beyond its tolerance.
- **AK-4** A 50 MB SQL dump opens without a syntax tree; a text between 1 and 8 MiB is opened without
  waiting for its tree; the editor computes inspections and fold regions off the main thread.
- **AK-5** A second symbol search over unchanged files is answered from the cache; a changed file is
  parsed again.
- **AK-6** Line ending detection, char/byte conversion, comment ranges and find positions give the same
  results as before on small texts, including `\r\n` across rope chunks and multi-byte characters.

## Out of scope
- Start of a non-empty project in the window (restored session, language servers); measured once the
  window opens projects with language servers in its own budget.
- Indexing with build models (Maven/Gradle import, JDK library) — OBS-0006 measured the reference with
  Maven; Ferris IDEA has no persistent index yet.
- An MCP tool: the budgets are a property of CI, not a feature of the IDE (ADR-0008 does not apply).

## Known deviations from the reference
- Files over 2.5 MB keep full highlighting, inspections and folding up to 8 MiB, and files over 50 MB open
  editable instead of read-only in parts (DOC); above 8 MiB Ferris IDEA shows plain text. IMP-0260.
- Start-up work is bounded: idle within 10 s of the project window instead of minutes (IMP-0063); memory
  stays below 300 MiB with an empty project instead of 2–3 GB (IMP-0003).
- Measurement method: idle is judged from CPU-time deltas per second, while OBS-0002 used the decaying
  CPU average `ps` reports; a decaying average lags behind, so Ferris IDEA's idle times are not inflated by
  the method, and the reference's are slightly. The window budgets are far enough from the reference
  that this does not change the comparison.
