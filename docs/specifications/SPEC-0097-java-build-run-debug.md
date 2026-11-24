# SPEC-0097: Java builds, running the file in the editor, Java debugging

- **Version:** 6 · **Date:** 2026-11-19 (v6: `run_markers` names `package_scripts` for a `package.json`, behaviour 11, AK-8; OBS-0098); 2026-11-19 (v5: the ▶ marker on the `if __name__ == "__main__":` of a Python script, behaviour 11, AK-8; B12 without a screen, `docs/research/observations/OBS-0098.md`; numbered after v4 of #1049, Java modules without a build tool, behaviour 10, AK-7); 2026-11-19 (v4: a Java class in a source folder of a module without a build tool runs with its module compiled, behaviour 10, AK-7; window check 2026-11-18 gap RJ1, F-077); 2026-11-17 (v3: the ▶ marker on `main` of a Go program and a Rust
  binary, behaviour 8, Rust binaries in their own folder and examples in behaviour 3, debugging
  `cargo run` as the program it builds, behaviour 9, AK-5 and AK-6; OBS-0081 item L14); 2026-11-07 (v2: the debugger's jdtls with its own data folder, a build
  before the launch and the launch naming the project, found in the window self-test)
- **Sources:** GEN (Maven and Gradle projects offer build and test configurations; the test results of
  the build tool appear as a tree; Run and Debug of the class or file in the editor with ⌃⇧R / ⌃⇧D
  create a temporary configuration named after it; Java programs are debugged with breakpoints, stack
  and variables; F-037, F-077, F-070), OSS (action ids `RunClass`, `DebugClass` with their shortcuts in the
  ported keymaps), STD (Maven Surefire and Gradle write JUnit XML per test class; DAP), third-party open
  source: Eclipse JDT LS and Microsoft java-debug (EPL-2.0, used as separate programs, not linked),
  SPEC-0087 (run configurations), SPEC-0088 (test tree), SPEC-0091 (debugging in the window)
  v3: OBS-0081 item L14 (the `main` of Go and Rust had no ▶ while their tests had one; Run from
  the context already ran them), GEN (Go: a program is the function `main` without parameters and
  results in the package `main`; Cargo's default target layout: `src/main.rs`, `src/bin/*.rs`,
  `src/bin/*/main.rs`, `examples/*.rs`, `examples/*/main.rs`; `cargo build` writes
  `target/debug/<name>`, examples below `examples/`, the target folder is `CARGO_TARGET_DIR` or the
  workspace root's `target`), SPEC-0122 behaviour 1 (the marker's menu), SPEC-0045 v5 behaviour 13
  (the test markers of the same files), SPEC-0039 v7 behaviour 11 (Go under Delve), SPEC-0278
  (safe mode)
- **Feature matrix rows:** F-037, F-077, F-070, F-038

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
     in a source folder of a module without a build tool the module compiled first (v4, behaviour
     10), elsewhere the JDK's source launcher `java File.java`;
   - a Java or Kotlin test source (under `src/test/` or with `@Test`) in a Maven module
     `mvn test -Dtest=Class`, in a Gradle build `gradle test --tests pkg.Class`, with the test tree;
   - Kotlin with `fun main(`: compiled with `kotlinc` into `.ferris-idea/run/Name.jar` and run;
   - Python `python3 file` (a `test_*.py` / `*_test.py` file runs pytest with a JUnit report);
     JavaScript `node file`; Go `go run file` for `package main`; Rust `cargo run` for `src/main.rs`
     (**v3:** `cargo run --bin <package>`, because a plain `cargo run` refuses to choose once
     `src/bin/` holds a further binary), `cargo run --bin name` for `src/bin/name.rs` and **(v3)**
     `src/bin/name/main.rs`,
     `cargo run --example name` for `examples/name.rs` and `examples/name/main.rs`.

   The command runs in the module's folder. A file that cannot be run leaves "Nothing to run in this
   file" in the status bar. [GEN, OSS]
4. **Temporary configuration:** the configuration is named after the class or file ("Main",
   "CalcTest", "tool.py"), selected in the run widget and kept for the session (it is not written to
   `run.toml`). Running the same file again replaces it. [GEN]
5. **Debug the file:** `DebugClass` (⌃⇧D on macOS, Ctrl+Shift+F9 elsewhere) makes the same
   configuration and debugs it (SPEC-0091). [GEN, OSS]
6. **Java debugging:** a configuration that runs `java File.java` or names `-Dexec.mainClass=…` is
   debugged through jdtls with the java-debug plugin (the newest
   `com.microsoft.java.debug.plugin-*.jar` in `~/.local/opt/java-debug`, or `$FERRIS_IDEA_JAVA_DEBUG`).
   jdtls starts for the working folder with the plugin as a bundle and opens the main source, so a
   file outside any build project is known to it. It then gives the debug adapter's port and the
   main class's module and class path (waiting up to two minutes for the project import). Ferris IDEA
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
8. **v3 – ▶ on `main` in Go and Rust:** a Go source that is not a `_test.go` file, declares
   `package main` and has a top-level `func main()` without parameters and results gets a run
   marker on the line of `main`; so does the top-level `fn main()` (any return type, no
   parameters) of a Rust binary root by Cargo's layout: `src/main.rs`, `src/bin/name.rs`,
   `src/bin/name/main.rs`, `examples/name.rs`, `examples/name/main.rs`, also below a member's
   folder. A method named `main`, a `main` in an inner `mod`, other packages and other Rust files get
   none. The markers come from the syntax tree, without a language server, and stand beside the
   test markers of the same file (SPEC-0045 v5 behaviour 13). The marker opens the menu of
   SPEC-0122 behaviour 1 with Run and Debug of the configuration of behaviour 3 (`go run file`,
   `cargo run --bin <package>`, `--bin name`, `--example name`), started as ⌃⇧R and ⌃⇧D start
   it (temporary configuration, behaviour 4); a Rust file outside a Cargo package says "Nothing to run in this
   file". In safe mode (SPEC-0278) the marker is shown, but Run and Debug run nothing, as every run.
   In a remote project the host decides what runs (SPEC-0318). The MCP tool `run_markers` lists them
   with `kind` `main` and the configuration in `run`. [GEN; ADR-0008]
9. **v3 – Debugging `cargo run`:** Debug of a configuration whose command is `cargo run` with
   `--bin name` or `--example name` (also `=name`), `-q`/`-v`/`--locked`/`--offline`/`--frozen`,
   and program arguments after `--` builds first with `cargo build` and the same selection (in the
   working folder, output in the Console tab; a failed build debugs nothing), then debugs the built
   program with lldb-dap (SPEC-0091), not cargo: `<target>/debug/name` or
   `<target>/debug/examples/name`, the target folder `CARGO_TARGET_DIR` (relative to the working
   folder) or else `target` in the topmost folder from the working folder up whose `Cargo.toml`
   has a `[workspace]` table, else in the working folder; without `--bin`/`--example` the name is
   the package's `default-run` or else its `name`. Other options (`--release`, `--profile`,
   `--target`, `-p`, `--manifest-path`, …) are not guessed at: the command is debugged as before.
   Go programs run under Delve as before (SPEC-0039 v7 behaviour 11). The build runs only where the
   debug start's trust check passed. [GEN]
10. **v4 – Java modules without a build tool:** a Java file with `main` that lies in a source
    folder of a module without Maven or Gradle (an IntelliJ module of `.iml` files) runs with the
    whole module compiled, as the reference does: `javac` compiles every `.java` file of the
    module's source folders (with the test folders when the file is a test source) into
    `out/production/<module>` (the module's own output when its `.iml` names one), then `java -cp
    <output>[:<libraries>] <full class name>` runs in the project folder. The command is one
    `sh -c` line: `mkdir -p .ferris-idea/run <output> && find <source folders> -name '*.java' >
    .ferris-idea/run/<module>.sources && javac -d <output> [-cp <libraries>] @… && java -cp …`.
    - The project's `.idea` files name the module, its source folders, output and libraries (its
      module libraries and the project libraries it uses, SPEC-0049). Without them a folder counts
      as a source folder when the file's folders below it equal its declared package and it is
      `src` or ends in `/src`, `src/main/java` or `src/test/java`; the module is named after its
      `.iml` file in the folder above `src`, else after that folder.
    - Only a Java file outside every source folder runs alone with the source launcher; a class
      that uses another class of its module ran into "cannot find symbol" there before v4.
    - A compile error stops the line before the run: the run window shows `javac`'s messages
      (`path:line: error: …`) and the exit code. Every run compiles the whole module again (no
      incremental compilation yet).
    - Debugging (behaviour 6) recognises the line: jdtls builds and launches the main class.
    - Maven and Gradle modules run as before; the MCP tools that run the file in the editor use
      the same configuration. [GEN; window check 2026-11-18 B12]
11. **v5 – ▶ on the main clause of a Python script:** a Python source that is no pytest file
    (`test_*.py`, `*_test.py`) and has a top-level `if __name__ == "__main__":` (either quotes,
    either side of `==`) has the ▶ marker on that line, as the reference shows it; it runs the
    file as behaviour 3 does (`python3 file`, with the project's interpreter). A clause inside a
    function, another condition or a file without one has no marker. Read from the syntax tree.
    The MCP tool `run_markers` lists it with `kind` `main`; for a file it has no markers for
    (plain JavaScript, a text file) it says which files have them, that `package_scripts` lists
    the scripts of a `package.json` (v6) and that `run_command` runs the others. [GEN; B12
    without a screen]

## Acceptance criteria
- **AK-1** Configurations for Java, Kotlin, Python, JavaScript, Go and Rust files, Maven and Gradle
  modules, wrapper detection (`crates/core/tests/it/run_context.rs`); Java debug planning and launch
  arguments (unit test `debug_model::tests::java_main_classes`); **(v2)** the project of the main
  class (`java_debug::tests::the_main_class_names_its_project`), the data folder beside the
  editor's (`servers::tests::a_second_jdtls_gets_a_folder_beside`), in the window
  `FERRIS_IDEA_SELFTEST_ONLY=java-debug` and `debug-more` without a failure.
- **AK-2** With Maven and Gradle installed: a project with a passing and a failing JUnit 5 test is
  detected, compiled and tested; the tree has 2 tests with 1 failure (`crates/core/tests/it/java_build.rs`).
- **AK-3** In a real window (`ferris-idea ui --selftest`): ⌃⇧R on a class with `main` shows its output;
  with jdtls and java-debug, ⌃⇧D stops at a breakpoint in `main` with `n = 21`, and resume ends with
  the program's output.
- **AK-4** By hand: a Maven and a Gradle project, run and debug of a class and of a test, next to the
  reference.

- **AK-5 (v3, behaviours 3 and 8)** Markers on `main` of Go and Rust binary roots and none
  elsewhere (`main_markers::tests::{go_main, rust_main_in_binary_roots, which_files_apply}`), the
  configurations of a binary folder and of examples (`crates/core/tests/it/run_context.rs`); through
  MCP `run_markers` on a small Go and Cargo project lists `main` with `go run shapes.go` and
  `cargo run --bin shapes`, and `run_command` with them prints the programs' output (agent run in PR). In a
  window: the ▶ menu on `func main` and `fn main` runs and debugs (window check open).
- **AK-6 (v3, behaviour 9)** `cargo run` with `--bin`, `--example`, arguments after `--` and a
  workspace root plans the build and the program, other options are left alone
  (`debug_plan::tests::cargo_run_debugs_the_built_program`). In a window: Debug at the ▶ of `fn main`
  stops at a breakpoint (window check open).
- **AK-7 (v4, behaviour 10)** Configuration: two classes of `lab.stock` beside an `.iml` file are
  compiled into `out/production/lab` and `lab.stock.Report` runs from there; with `.idea` the
  module's name, source folders and library come from its files; a file outside a source folder
  runs alone (`crates/core/tests/it/run_context.rs`:
  `a_module_without_a_build_tool_is_compiled_before_the_run`). With a JDK: the line prints
  "Total: 40" with exit code 0, and a compile error in the other class stops it with javac's
  `Inventory.java:5: error` and no run (`the_compiled_module_runs_and_a_compile_error_stops_it`;
  passes without running where no `javac` is installed). Debug planning: the line is debugged as
  its class (`debug_plan::tests::a_compiled_module_is_debugged_as_its_class`). In a window next to
  the reference: ⌃⇧R on `Report` prints "Total: 40" (window check).
- **AK-8 (v5, behaviour 11)** The marker on the top-level main clause, with either quotes and
  sides, none inside a function, for another condition or in a pytest file
  (`main_markers::tests::python_main_clause`, `which_files_apply`); `run_markers` lists it with
  `python3 main.py` and names `run_command` and (v6) `package_scripts` for a plain JavaScript file
  (`protocol.rs`: `single_tests_and_rerun_failed_tools`).

## Out of scope
Kotlin debugging, Gradle application main classes (the `application` plugin's `run` task can be added to
`run.toml`), JUnit tests without a build tool, hot swap, remote JVM attach, the Maven and Gradle tool
windows, debugging tests; (v3) Cargo targets declared with another `path` in `Cargo.toml`, the target
folder of `.cargo/config.toml`, building before debugging in a remote project.

## Known deviations from the reference
The reference compiles and runs with its own build and JUnit runner; Ferris IDEA uses the project's build
tool and its reports, so results match the command line. Test reports from earlier runs are never shown
as current (they are in the reference when the tool fails before writing new ones; IMP-0034).
