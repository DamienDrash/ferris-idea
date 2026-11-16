# SPEC-0097: Java builds, running the file in the editor, Java debugging

- **Version:** 2 · **Date:** 2026-11-07 (v2: the debugger's jdtls with its own data folder, a build
  before the launch and the launch naming the project, found in the window self-test)
- **Sources:** GEN (Maven and Gradle projects offer build and test configurations; the test results of
  the build tool appear as a tree; Run and Debug of the class or file in the editor with ⌃⇧R / ⌃⇧D
  create a temporary configuration named after it; Java programs are debugged with breakpoints, stack
  and variables; F-037, F-077, F-070), OSS (action ids `RunClass`, `DebugClass` with their shortcuts in the
  ported keymaps), STD (Maven Surefire and Gradle write JUnit XML per test class; DAP), third-party open
  source: Eclipse JDT LS and Microsoft java-debug (EPL-2.0, used as separate programs, not linked),
  SPEC-0087 (run configurations), SPEC-0088 (test tree), SPEC-0091 (debugging in the window)
- **Feature matrix rows:** F-037, F-077, F-070

## Purpose
Build, test, run and debug Java the way the reference does, without writing run configurations by hand.

## Behaviour
1. **Detected builds:** a Maven module (`pom.xml`, not a sub-module) offers "mvn compile" and "mvn test";
   a Gradle build (not an included project) offers "gradle build" and "gradle test", run with the
   build's wrapper `./gradlew` when the build folder or one above it has one, else with `gradle`. The
   test configurations are test runs (SPEC-0088). [GEN]
2. **Test results:** a test configuration's `report_file` may name a folder: every `*.xml` in it
   written by this run (modified since it started, with two seconds of slack) is read as JUnit and
   merged into one tree, files in name order. Maven reads `target/surefire-reports`, Gradle
   `build/test-results/test`. A folder without such files is reported as "no test report written by
   this run". Reports of earlier runs never count. [STD]
3. **Run the file:** `RunClass` (⌃⇧R on macOS, Ctrl+Shift+F10 elsewhere) runs the file in the editor:
   - Java with `static void main(`: in a Maven module `mvn -q compile exec:java -Dexec.mainClass=…`,
     elsewhere the JDK's source launcher `java File.java`;
   - a Java or Kotlin test source (under `src/test/` or with `@Test`) in a Maven module
     `mvn test -Dtest=Class`, in a Gradle build `gradle test --tests pkg.Class`, with the test tree;
   - Kotlin with `fun main(`: compiled with `kotlinc` into `.idecraft/run/Name.jar` and run;
   - Python `python3 file` (a `test_*.py` / `*_test.py` file runs pytest with a JUnit report);
     JavaScript `node file`; Go `go run file` for `package main`; Rust `cargo run` for `src/main.rs`,
     `cargo run --bin name` for `src/bin/name.rs`.

   The command runs in the module's folder. A file that cannot be run leaves "Nothing to run in this
   file" in the status bar. [GEN, OSS]
4. **Temporary configuration:** the configuration is named after the class or file ("Main",
   "CalcTest", "tool.py"), selected in the run widget and kept for the session (it is not written to
   `run.toml`). Running the same file again replaces it. [GEN]
5. **Debug the file:** `DebugClass` (⌃⇧D on macOS, Ctrl+Shift+F9 elsewhere) makes the same
   configuration and debugs it (SPEC-0091). [GEN, OSS]
6. **Java debugging:** a configuration that runs `java File.java` or names `-Dexec.mainClass=…` is
   debugged through jdtls with the java-debug plugin (the newest
   `com.microsoft.java.debug.plugin-*.jar` in `~/.local/opt/java-debug`, or `$IDECRAFT_JAVA_DEBUG`).
   jdtls starts for the working folder with the plugin as a bundle and opens the main source, so a
   file outside any build project is known to it. It then gives the debug adapter's port and the
   main class's module and class path (waiting up to two minutes for the project import). IDECraft
   connects over TCP and launches the class with the configuration's arguments, environment and
   folder; output goes to the Console tab. **(v2)** This jdtls keeps a workspace data folder of its
   own next to the editor's (`<folder>-debug`): Eclipse locks a data folder for one process, and two
   processes on one folder hung the class path request for two minutes, renewed it under each other
   when the source roots changed, and built a saved change in the editor's process, so hot swap
   found no changed class. Before the launch it builds the workspace incrementally and waits for
   the build (`java/buildWorkspace`), so a source opened just before is compiled when the JVM looks
   for the main class. The launch names the main class's project (`projectName` from
   `vscode.java.resolveMainClass`), so java-debug evaluates expressions — `this.toString()`, views,
   fragments, the thread states — while jdtls is still importing instead of refusing them. Breakpoints, stepping, stack, variables and evaluate work
   as in SPEC-0091. Without the plugin the status bar says where it belongs. [GEN, STD]
7. Agents run the detected configurations with `run_configuration` (SPEC-0053) and debug with the
   tools of SPEC-0054. [ADR-0008]

## Acceptance criteria
- **AK-1** Configurations for Java, Kotlin, Python, JavaScript, Go and Rust files, Maven and Gradle
  modules, wrapper detection (`crates/core/tests/it/run_context.rs`); Java debug planning and launch
  arguments (unit test `debug_model::tests::java_main_classes`); **(v2)** the project of the main
  class (`java_debug::tests::the_main_class_names_its_project`), the data folder beside the
  editor's (`servers::tests::a_second_jdtls_gets_a_folder_beside`), in the window
  `IDECRAFT_SELFTEST_ONLY=java-debug` and `debug-more` without a failure.
- **AK-2** With Maven and Gradle installed: a project with a passing and a failing JUnit 5 test is
  detected, compiled and tested; the tree has 2 tests with 1 failure (`crates/core/tests/it/java_build.rs`).
- **AK-3** In a real window (`idecraft ui --selftest`): ⌃⇧R on a class with `main` shows its output;
  with jdtls and java-debug, ⌃⇧D stops at a breakpoint in `main` with `n = 21`, and resume ends with
  the program's output.
- **AK-4** By hand: a Maven and a Gradle project, run and debug of a class and of a test, next to the
  reference.

## Out of scope
Kotlin debugging, Gradle application main classes (the `application` plugin's `run` task can be added to
`run.toml`), JUnit tests without a build tool, hot swap, remote JVM attach, the Maven and Gradle tool
windows, debugging tests.

## Known deviations from the reference
The reference compiles and runs with its own build and JUnit runner; IDECraft uses the project's build
tool and its reports, so results match the command line. Test reports from earlier runs are never shown
as current (they are in the reference when the tool fails before writing new ones; IMP-0034).
