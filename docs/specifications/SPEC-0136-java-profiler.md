# SPEC-0136: Profiling Java with JDK Flight Recorder

- **Version:** 3 · **Date:** 2026-11-19 (v3: costs per line and frames outside the project, behaviour 7,
  AK-7, OBS-0113 P1 and P3); 2026-11-15 (v2: AK-6, hand test 62 with real keys)
- **Sources:** GEN (IDEs profile Java run configurations and show call tree, hot spots and flame
  graph with a jump to the source; F-083), STD (JDK Flight Recorder in OpenJDK 11+:
  `-XX:StartFlightRecording=filename=…,settings=profile` and, from JDK 17, event settings such as
  `jdk.ExecutionSample#period=1ms`; `-Xlog:jfr+startup=error`; the JDK's `jfr print --events
  jdk.ExecutionSample --stack-depth 64` with frames innermost first as `pkg.Class.method(params)
  line: n`; Maven's `MAVEN_OPTS`), SPEC-0120 (profiler), SPEC-0122 (Java run configurations),
  ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Profile Java programs like native and Python ones (SPEC-0120), with the recorder every JDK
already has: nothing to install, no agent to attach.

## Behaviour
1. **Which runs:** a configuration whose program is `java` (also a path to it; single source
   files, `-cp`, `-jar`) gets the recording options right after `java`; `mvn`/`mvnw` with
   `exec:java` gets them through `MAVEN_OPTS` (kept options first), because the main class runs in
   Maven's JVM. Other JVM launchers stay with SPEC-0120. [STD]
2. **Recording:** one execution sample per millisecond and running thread into
   `.ferris-idea/profiles/last.jfr`; the recorder's start-up lines stay out of the output. JDK 17 or
   newer is needed; with an older JDK the JVM refuses the option and the error says so. [STD]
3. **Reading:** the `jfr` tool of the same JDK (next to `java` following links, else `JAVA_HOME` of
   the configuration or environment, else the macOS default JDK, else `PATH`) prints the samples
   as text (about 250 bytes per sample rather than about 9 KB in its JSON form). Each sample adds 1
   to every method of its stack under a root per thread ("Thread main"); samples taken while the
   recorder starts (`jdk.jfr.` frames) are left out. [STD]
4. **Methods:** a node is `Class.method` (nested classes keep `$`, overloads share a node), its
   module the package, its location `pkg/path/Outer.java` and the line with the most samples;
   native frames have no line. Unit: samples (milliseconds). [GEN]
5. **Source:** a reported relative path is also found as the end of a workspace path
   (`src/main/java/demo/Busy.java` for `demo/Busy.java`) before the unique-name fallback of
   SPEC-0120 behaviour 7. [GEN]
6. **Window and MCP:** as SPEC-0120; the engine reads "JFR"; `profile_run` reports `jfr` and the
   rewritten command or the `MAVEN_OPTS` value. [ADR-0008]
7. **Lines and project code (v3, OBS-0113):** every frame of a sample with a line adds the sample
   to that line's total, once per sample; the innermost frame's line gets it as self cost
   (SPEC-0120 v5 behaviour 10), so the editor shows a cost at the hot loop and at each line that
   calls into it, not only at a method's busiest line. JDK and library frames report their
   package path (`java/util/Objects.java`), which does not resolve inside the workspace, so they
   are outside the project unless the project has that very file (SPEC-0120 v5 behaviour 11). [GEN]

## Acceptance criteria
- **AK-1** Text of `jfr print`: tree per thread, recorder samples left out, nested class file,
  native frame without line, busiest line, hot spot; `java` and `mvn exec:java` plans (unit tests
  `jfr::tests`, `run::tests::java_and_maven_exec_use_flight_recorder`).
- **AK-2** A real Java program compiled into `src/main/java` layout: engine JFR, exit code 0, no
  recorder lines in the output, `Busy.inner` hottest at `demo/Busy.java:6`, the source found under
  `src/main/java` (`crates/profiler/tests/real.rs`: `java_program_under_flight_recorder`).
- **AK-3** MCP dry run of a `java` configuration names `jfr` and the recording option
  (`crates/mcp/tests/it/protocol.rs`: `profiles_a_python_configuration`).
- **AK-4** Window self-test: a `java Busy.java` configuration is profiled, the summary names JFR,
  Hot Spots lists `Busy.inner` first with its line in `Busy.java`.
- **AK-5** Hand test (round 4): profile a Maven and a single-file Java program in a real project,
  read the three views and jump to the source; compare with the reference's profiler.
- **AK-6 (v2)** Hand test 62 in a window of the test harness with real keys and a real JDK
  (`crates/workbench/tests/it/f083_handtests.rs`: `hand_tests_62_and_180_profiles_with_keys`; skipped without `java`): Find Action
  › Profile Selected on a single Java file in `src/main/java` layout records with JFR, no JFR
  messages in the output, `2` shows Hot Spots with `Rechen.teuer` first at `Rechen.java:6` and a time
  that fits its share (D37), Enter opens the file at that line. Not covered: the Maven configuration
  (needs the exec plugin from Maven Central), the comparison with the reference.
- **AK-7 (v3)** Text of `jfr print` with a recursive method and a JDK frame: line costs per line
  with the recursive line counted once and self cost at the innermost line; the JDK frame outside the
  project, the project's method inside (`jfr::tests`); in the window, SPEC-0120 v5 AK-9.

## Out of scope
Allocation, lock and I/O events, wall-clock (sleeping threads), Gradle `run` and other launchers
(Gradle forks its own JVM), Kotlin's `kotlin` launcher, attaching to a running JVM (`jcmd JFR.start`),
async-profiler. The `mvn exec:java` path is unit-tested only: the exec plugin is not in the local
Maven repository and was not downloaded.

## Known deviations from the reference
Profiles are recorded with the JDK's own Flight Recorder and read with its `jfr` tool instead of a
bundled agent, so nothing is installed (IMP-0043 as for SPEC-0120). Compared side by side in OBS-0113
(Gradle project): views, hottest method and the jump to the source agree; the reference's costs next
to the lines (P1) and its greyed frames outside the project (P3) are behaviour 7 since v3; its
timeline, events and choice of CPU or total time (P2) stay out of scope (SPEC-0120 v5).
