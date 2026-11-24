# SPEC-0252: Profiling on Linux and Gradle `run`

- **Version:** 4 · **Date:** 2026-11-22 (v4: a `perf` on the PATH that cannot run, such as the Ubuntu launcher without the kernel's tools, counts as missing, and the note names the Ubuntu package that brings the program, behaviour 1, AK-8, hand test 182; v3: a kernel that forbids `perf` is named before anything runs, also in a dry run, behaviour 1, AK-7; hand test 182 Linux part without a screen); 2026-11-16 (v2: AK-6, hand test 182's Gradle part with real keys)
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
   refused and names the package (`linux-perf`, `linux-tools-common`, `perf`). **v4:** `perf` counts
   as present only when `perf --version` answers; on Ubuntu `linux-tools-common` brings only a
   launcher that fails without the tools of the running kernel, so that launcher alone counts as
   missing, and the note names `linux-tools-generic` beside `linux-tools-common`. When the kernel
   refuses (`perf_event_paranoid` above 2), the note says so and how to allow it. Since v3 this
   is known before anything runs: Ferris IDEA reads the level (and whether the process has
   `CAP_PERFMON` or `CAP_SYS_ADMIN`, which lift it) when it plans the run, so the dry run of
   `profile_run` and the window refuse with that note before the before-launch configurations
   and the program start; a level it cannot read leaves the decision to `perf`, whose answer is
   still recognised. Ferris IDEA never changes the kernel setting. Until v2 the program and its
   before-launch configurations ran first and only `perf`'s answer gave the note. [STD]
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
   Ferris IDEA has no Windows machine to test on. Python and Java profiling are not tested there.
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
  part needs Ferris IDEA running on Linux and stays with the hand test.
- **AK-7 (v3, behaviour 1)** Unit test `perf::tests::the_kernels_level_refuses_perf_before_the_run`
  (level above 2 refuses, 2 and below do not, the two capabilities lift it, an unreadable level
  does not refuse); on Linux the plan of a native program is `perf record` or one of the two
  notes (`run::tests::native_programs_use_sample_on_macos_and_perf_on_linux`). Through MCP on a
  Linux machine with level 4 (`ferris-idea call`, OBS of hand test 182's Linux part): the dry run of
  `profile_run` for a C program answers with the note.
- **AK-8 (v4, behaviour 1)** Unit test `run::tests::a_perf_that_cannot_run_counts_as_missing`: a
  `perf` on the PATH that fails `--version` (as the Ubuntu launcher does without the kernel's
  tools) or is absent counts as missing, one that answers counts as present; the note names
  `linux-tools-common` and `linux-tools-generic`.

## Out of scope
Kernel stacks, `perf` source lines, hardware counters, other Gradle tasks than `run` (tests,
`bootRun`), Windows profiling.

## Known deviations from the reference
Linux profiling uses the distribution's `perf` instead of a bundled native agent (IMP-0043); a
kernel that forbids it is explained instead of asking for root.
