# SPEC-0250: Memory profiles

- **Version:** 3 · **Date:** 2026-11-15 (v3: AK-6, hand test 180 with real keys; v2: the notes are shown under the summary, file and
  line of native functions)
- **Sources:** GEN (IDE profilers also show where a program allocates memory, as an allocation
  call tree and a list of allocating functions; F-083), STD (Python's `tracemalloc`
  (`start(nframe)`, `take_snapshot()`, `statistics("traceback")`, `get_traced_memory()`) and
  `runpy`/`ast`; macOS `leaks --atExit --outputGraph=<file> -- <command>` with
  `MallocStackLogging=1` and `malloc_history <memgraph> -callTree`; Valgrind Massif on Linux
  (`valgrind --tool=massif --massif-out-file=<file>`, the `massif.out` format with
  `heap_tree=peak` and `nN: <bytes> <address>: <function> (<file>:<line>)` lines)), SPEC-0120,
  SPEC-0053, ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Find out which code holds memory without leaving the IDE and without installing an agent: run a
configuration once for memory and read the allocation call tree like a CPU profile.

## Behaviour
1. **Kinds:** a profile is either CPU (SPEC-0120) or memory. Memory costs are bytes (unit
   `bytes`); the call tree, hot spots and flame graph of SPEC-0120 show them unchanged, with sizes
   (`12.3 MB`) instead of times. Self bytes are bytes allocated by the function itself. [GEN]
2. **Python:** a Python interpreter call runs a small runner (written next to the profile) with
   the same interpreter and its options: `tracemalloc` starts with 64 frames, the script (`runpy`
   run path) or `-m` module runs as `__main__` with its arguments, and when it ends (normally,
   with `SystemExit` or an exception) a snapshot is taken while the module's globals are still
   held. Each allocation site becomes a stack of frames from the program's first frame down; the
   runner's and `runpy`'s own frames are left out; a frame's name is the innermost function or
   class around its line (`<module>` at top level, found with `ast`). Traced bytes at the end and
   the peak are notes. `-c` and a call without script are refused as for CPU. [STD]
3. **Native programs on macOS:** the program runs under `leaks --atExit --outputGraph` with
   `MallocStackLogging=1`; when it ends, the memory graph is read with `malloc_history -callTree`.
   The tree shows the bytes still allocated at the end of the program (what it kept or leaked)
   by call path; the malloc library's own frames are left out, so the allocating function carries
   the bytes. The program's own functions get `file:line` from its debug information through
   `atos` with the report's load address (v2, hand test 180: none were shown although built with
   `-g`). The program's exit code is not known under `leaks` and is shown as unknown; the
   peak physical footprint is a note. A program killed at the time limit leaves no graph and is
   reported as such. [STD]
4. **Native programs on Linux:** when `valgrind` is installed the program runs under Massif; the
   peak snapshot's allocation tree (allocator at the top, callers below) is turned around into a
   call tree with file and line from debug information; allocation sites below Massif's
   threshold become one "(smaller sites)" frame. Without `valgrind` the run is refused and names
   the package to install. [STD]
5. **Not supported:** Java memory (JFR allocation samples) and other runtimes are refused with
   the reason; CPU profiling is unchanged for them. [GEN]
6. **Window and actions:** `Profiler.ProfileMemory` ("Profile Memory") profiles the selected
   configuration for memory the same way as `Profiler.ProfileSelected` (before-launch
   configurations first, time limit, background, one at a time, kept as `last.json`). The summary
   line names the engine (`tracemalloc`, `leaks`, `massif`) and the bytes measured; F5 repeats the
   kind shown. The profile's notes (that `leaks` does not report the exit code, the peak footprint)
   stand in a line under the summary, which says "exit code unknown" where the engine cannot know
   it (v2, hand test 180: the remark was not shown anywhere). [GEN]
7. **MCP:** `profile_run` takes `kind` (`cpu` default, `memory`); the dry run names the engine and
   command; the answer is the same as for CPU with `unit: "bytes"`. `profile_last` returns
   whichever profile was kept last. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Python runner output → tree (runner frames left out, names per function, merge of
  equal frames, notes); `malloc_history` call tree with sizes (`112 bytes`, `793K`, `19.5M`),
  malloc frames left out; Massif peak tree turned around, threshold line; plans per kind and
  refusals (unit tests in `crates/profiler`).
- **AK-2** Real runs: a Python script keeping lists holds most bytes in `make` at `app.py`; a C
  program built with `cc -g` keeps its blocks under `grow` called from `build` (macOS); the Massif
  run on Linux skips when `valgrind` is missing (`crates/profiler/tests/real.rs`).
- **AK-3** MCP: dry run with `kind: memory`, a real Python memory run with hot spots in bytes
  (`crates/mcp/tests/it/protocol.rs`: `profiles_memory_and_attaches`).
- **AK-4** Window self-test: Profile Memory on a Python configuration shows `tracemalloc`, Hot Spots
  lists the allocating function first with its line, sizes read in bytes.
- **AK-5** Hand test (round 4, no. 180): memory profile of a Python script and a C program in a
  real project, next to the reference's memory views.
- **AK-6 (v3)** Hand test 180 in a window of the test harness with real keys
  (`crates/workbench/tests/it/f083_handtests.rs`: `hand_tests_62_and_180_profiles_with_keys`): Profile Memory on a Python script
  shows `tracemalloc` and "held", Hot Spots with `fuelle` first at `mem.py:5` in MB, Enter opens it;
  a C program built with `-g` is recorded with `leaks`, `lege_an` first, and the notes say that
  `leaks` does not report the exit code (the finding of the hand test). Not covered: file and line
  of C functions, the comparison with the reference.

## Out of scope
Java allocation profiles, a heap dump browser (objects and references), comparing two memory
profiles, allocation timelines, freed allocations on macOS (only what is still allocated at the
end), Windows.

## Known deviations from the reference
Memory is measured with what Python, macOS and Valgrind ship instead of a bundled agent (IMP-0043);
the result is a call tree in the same readable JSON as CPU profiles. On macOS the tree shows what
is still allocated when the program ends rather than every allocation; on Linux it is Massif's
peak. Not compared side by side yet.
