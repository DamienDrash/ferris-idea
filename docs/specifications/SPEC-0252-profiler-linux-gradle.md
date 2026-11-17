# SPEC-0252: Profiling on Linux and Gradle `run`

- **Version:** 2 · **Date:** 2026-11-16 (v2: AK-6, hand test 182's Gradle part with real keys)
- **Sources:** GEN (IDE profilers work on every desktop system and for build-tool launches;
  F-083), STD (Linux `perf record -F 1000 -g -e cpu-clock:u -o <file> -- <command>`,
  `perf script -F comm,tid,ip,sym,dso -i <file>` and its output: a header line per sample, then
  one frame per line `<address> <symbol>+<offset> (<object>)`, innermost first, blank line after
  each sample; `kernel.perf_event_paranoid`; Gradle `--init-script`, `tasks.withType(JavaExec)`,
  `jvmArgs`), SPEC-0120, SPEC-0136, SPEC-0250, SPEC-0251
- **Feature matrix rows:** F-083

## Purpose
Profile the same programs on Linux as on macOS, and Java programs started by Gradle's `run`.

## Behaviour
1. **Native programs on Linux:** when `perf` is on the PATH, the program runs under
   `perf record` (1000 samples per second of CPU time, user space only, call graphs) and the
   samples are read with `perf script`; each thread (`name tid`) becomes a root with the frames
   from the outermost call down; offsets are dropped; an unknown symbol shows its object
   (`[unknown] (libfoo.so)`). Costs are samples of one millisecond. Without `perf` the run is
   refused and names the package (`linux-perf`, `linux-tools-common`, `perf`). When the kernel
   refuses (`perf_event_paranoid` above 2), the note says so and how to allow it. [STD]
2. **Same elsewhere:** Python (cProfile, SPEC-0120; tracemalloc, SPEC-0250) and Java (JFR,
   SPEC-0136) work on Linux unchanged; native memory uses Massif (SPEC-0250), attaching uses
   `perf` (SPEC-0251). [STD]
3. **Gradle `run`:** a configuration whose program is `gradle` or `gradlew` and that runs the
   `run` task (`run` or `:<project>:run`) is profiled with JDK Flight Recorder: an init script
   written next to the profile adds the recording options to every `JavaExec` task (the
   application's JVM, not Gradle's), and `--init-script <file>` is added after the program. The
   recording is read as in SPEC-0136. [STD]
4. **Windows:** profiling native programs is refused on Windows: Windows ships no sampler that
   records call stacks without administrator rights (ETW kernel sampling needs them), and
   IDECraft has no Windows machine to test on. Python and Java profiling are not tested there.
   [GEN]

## Acceptance criteria
- **AK-1** `perf script` output → threads, frames from outermost, offsets dropped, unknown symbols;
  Gradle detection (`gradle run`, `./gradlew :app:run`, not `gradle build`) and the init script's
  text with the path escaped; the Linux and Windows refusals (unit tests in `crates/profiler`).
- **AK-2** Real runs: `gradle run` of a small application has `Busy.inner` as the hottest method
  (skips without Gradle or JDK 17); a C program under `perf` on Linux (skips without `perf`)
  (`crates/profiler/tests/memory.rs`).
- **AK-3** MCP: `profile_run` dry run of a Gradle configuration names JFR and `--init-script`
  (`profiles_memory_and_attaches`).
- **AK-4** — (covered by the self-tests of SPEC-0120/0136; Linux has no window self-test machine).
- **AK-5** Hand test (round 4, no. 182): a Gradle application and, on a Linux machine with
  `perf`, a C program; compare with the reference's profiler on Linux.
- **AK-6 (v2)** Hand test 182's Gradle part in a window of the test harness with real keys
  (`crates/workbench/tests/it/f083_handtests.rs`: `hand_test_182_gradle_run_profiles_the_application`;
  skipped without Gradle or `java`, or when the wrapper's distribution is not in the Gradle cache):
  a Gradle application (`application` plugin) with the run configuration `./gradlew run`, Find
  Action › Profile Selected and Enter record it with JFR; the application ran, Hot Spots shows
  `Busy.inner` first with `Busy.java:6` and no Gradle frame, and Enter opens that line. The Linux
  part needs IDECraft running on Linux and stays with the hand test.

## Out of scope
Kernel stacks, `perf` source lines, hardware counters, other Gradle tasks than `run` (tests,
`bootRun`), Windows profiling.

## Known deviations from the reference
Linux profiling uses the distribution's `perf` instead of a bundled native agent (IMP-0043); a
kernel that forbids it is explained instead of asking for root.
