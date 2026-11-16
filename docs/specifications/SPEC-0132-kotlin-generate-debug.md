# SPEC-0132: Kotlin — Generate and debugging

- **Version:** 7 · **Date:** 2026-11-11 (v7: a breakpoint that is not bound is hollow with the
  reason, SPEC-0127 v5; v2: conditions, hit counts and logpoints through SPEC-0178;
  v3: Generate lists Override Methods… and Implement Methods…, OBS-0035 L6; v4: values of a stop
  in the form of SPEC-0091 v16, OBS-0035 L11 and L12; v5: breakpoints in classes named unlike
  their file, OBS-0035 L14; v6: stops and ends the adapter does not report at the start, class
  paths with a time limit, a command right after a stop)
- **Sources:** GEN (in a Kotlin class the reference's Generate menu offers a secondary
  constructor, `equals()`/`hashCode()` and `toString()` over the properties; Kotlin programs of
  Gradle and Maven projects are run and debugged from the editor with breakpoints, locals and
  stepping; F-037), STD (Kotlin: top-level `main` in class `<File>Kt`, `===`, `javaClass`,
  `contentEquals`/`contentHashCode`/`contentToString` for arrays; a class or object nested in
  `Outer` is the JVM class `Outer$…`; `@file:JvmName`; DAP; JDI class filters, where a `*` at the
  end of the pattern stands for any rest), OSS
  (`fwcd/kotlin-debug-adapter` 0.4.4, MIT, used unchanged as a separate program; how it finds the
  classes of a breakpoint: `PathUtils.kt` and `JDIDebuggee.kt` of that version), SPEC-0122,
  SPEC-0091, SPEC-0039 v2, ADR-0008
- **Feature matrix rows:** F-037

## Purpose
Kotlin code is generated and debugged as conveniently as Java.

## Behaviour
1. **Generate** (⌘N / Alt+Insert) in a Kotlin class offers "Secondary Constructor",
   "equals() and hashCode()" and "toString()". Properties are the `val`/`var` parameters of the
   primary constructor and the properties of the class body (not those of a companion object). One
   undo step. [GEN, STD]
   - `equals` checks identity, `javaClass`, then each property (`contentEquals` for arrays);
     `hashCode` combines them with 31 (`Int` as is, nullable with `?: 0`, arrays with
     `contentHashCode`); `toString` gives `Name(a=$a, s='$s')` (`contentToString` for arrays).
   - A secondary constructor takes the typed body properties and is refused when the class has
     primary constructor parameters. Existing functions are not made twice; a `data class` has
     `equals`/`hashCode`/`toString` already and is refused; getters and setters are not offered.
1a. **Choosers in the list (v3):** below these the list offers "Override Methods…" and "Implement
   Methods…", which open the choosers of SPEC-0122 behaviour 15 for the class at the caret.
   [OBS-0035 item 10]
2. **Running and debugging:** a Kotlin file with `fun main` in a Gradle or Maven module runs as
   "build, then `kotlin -cp <classes> <File>Kt`" (Gradle `classes` into `build/classes/kotlin/main`,
   Maven `compile` into `target/classes`); outside a module it stays a single-file compile
   (SPEC-0097). Debugging such a configuration (⌃⇧D, the ▶ marker's Debug) runs the build step,
   shows its output and stops if it fails, then starts kotlin-debug-adapter with the module folder
   and the main class; the adapter resolves the class path through Gradle or Maven. Breakpoints
   of `.kt` and `.java` files inside the module are set (others would keep the adapter from
   stopping); stops show the stack, locals and watches as in SPEC-0091. [GEN, OSS]
3. **Adapter:** `IDECRAFT_KOTLIN_DEBUG`, else the newest `~/.local/opt/kotlin-debug-adapter*`
   (`kotlin-debug.sh install` of the reference environment: release archive with a checked SHA-256), else
   `kotlin-debug-adapter` on the PATH; missing, the status bar says what to install. It does not
   support conditional breakpoints, hit counts or logpoints itself; IDECraft evaluates them
   (SPEC-0178). [OSS]
4. **Protocol details** that this adapter needs and that are harmless for others: the source of
   `setBreakpoints` carries the file name; `launch` may take up to 120 s (class path resolution);
   an unanswered `configurationDone` is accepted once `launch` is answered; a stop reported twice
   is not taken for the next stop. [STD]
4a. **Values (v4, OBS-0035 L11 and L12):** the variables of a stop follow SPEC-0091 v16 behaviour
   42. Paused in a top-level function such as `fun main()` there is no `this` row; in a member
   function `this` is the object. Objects read `Type@id` instead of the adapter's
   `instance of …(id=…)`, a list, set or map of `java.util` or `kotlin.collections` is followed
   by ` size = N`, a boxed number shows its value. The sizes are read from the objects' fields,
   because this adapter cannot evaluate calls (SPEC-0178) and no method of the program is to run
   for a row. [OBS, third-party open source]
4b. **Breakpoints in classes named unlike their file (v5, OBS-0035 L14):** a line breakpoint in
   any `.kt` file of the module stops the program: in a member function with an expression body
   or a block body, in a lambda inside a member, in a companion object, a nested class or an
   anonymous object, in a top-level function, and in files other than the launched one.
   [OBS, OSS, STD]
   - **Cause.** kotlin-debug-adapter 0.4.4 takes the classes of a breakpoint from the path alone:
     the folders after the last one called `kotlin` or `java` are the package, the file name is
     the class, once as it is and once with a capital and `Kt`. For `zoo/Animals.kt` it looks
     into `zoo.Animals` and `zoo.AnimalsKt` and never into `zoo.Animal`, which the file declares.
     It waits for the loading of exactly these two names; classes inside them (`Outer$…`) get the
     breakpoint only when they are loaded at the moment the breakpoints are sent. Whatever it
     finds, it answers `verified: true`. Sending the breakpoints again later changes nothing.
   - **What IDECraft sends.** For a `.kt` file of a session with this adapter IDECraft reads the
     file from disk and finds the package, the class of the top-level code (`<File>Kt`, or the
     name of `@file:JvmName`) and the top-level classes, interfaces and objects with their lines.
     A line belongs to the top-level declaration it stands in. After the `setBreakpoints` of the
     file itself, and like it before `configurationDone`, IDECraft sends for every class with
     breakpoint lines two more `setBreakpoints` whose source path only names a class to the
     adapter: `<file>/kotlin/<package folders>/<Class>.kt`, unless the adapter derives this class
     from the file's own path already, and `<file>/kotlin/<package folders>/<Class>$*.kt` for the
     classes inside it, also those that load later (the adapter hands the name to the JVM as a
     class filter). These paths lie below the file's own path, so they never name a file that
     exists and never collide with the breakpoints of another file. Each carries the entries of
     its lines exactly as the file's own request does: for this adapter plain lines, because
     conditions, hit counts and log messages stay with IDECraft (SPEC-0178), which recognises a
     hit by the file and line of the stop's top frame, and the adapter reports the real file
     there.
   - **Changes during the session.** Whenever the breakpoints of the file are set again, the
     class paths are sent again; one that no longer has a line is sent once with an empty list.
   - **Why this way.** It needs no change to the adapter, no extra stop and no polling; the user
     sees nothing of it. Re-sending at stops or on a schedule was tried first and does not help,
     because the adapter never looks into the class. The package comes from the file and not
     from its folder, so a file whose folder differs from its package, or that lies under no
     folder called `kotlin` or `java`, is covered too.
   - **What cannot be bound is said.** When the package has a part called `kotlin` or `java`, no
     path can name the class to the adapter. Such a breakpoint is reported as not verified,
     whatever the adapter answers; it is listed with the breakpoints still not bound of SPEC-0039
     behaviour 8 with this reason, and the debug console says once per line that it cannot stop
     the program and why.
   - **Failures (v6).** A file that cannot be read is sent as before. A request with a class
     path has five seconds for its answer; one that is refused (a system that does not allow
     `*` or `$` in paths) or not answered is never an error of the session and never keeps
     `configurationDone` or a resume from being sent. It costs only its class: when the path
     names the class itself, the lines in it are reported as not verified with this reason, as
     above; when it names the classes inside (`$*`), the breakpoints stay verified and the
     debug console says once that they stop only in the class itself and in inner classes
     loaded already.
   - Other adapters and `.java` files of a Kotlin session are sent as before.
4e. **Seen in the gutter (v7):** a breakpoint reported as not verified — by the adapter, or by
   IDECraft for the reasons of 4b — is drawn hollow while the session runs, and its tooltip names
   the reason (SPEC-0127 v5 behaviour 14); for the lines of 4b that reason is IDECraft's
   explanation, and agents get it as the placed breakpoint's `reason` (SPEC-0127 v5 behaviour 15).
   [STD]
4c. **What the adapter does not report at the start (v6, found with PR #520 in the window):**
   kotlin-debug-adapter 0.4.4 starts the program as soon as it is connected to it, registers
   the breakpoints in the order they were sent while the program already runs, and only then
   begins to pass on what the program reports; the answer to `launch` comes after that. Its own
   work takes about a tenth of a second, a small JVM program is at its first line after about
   the same time, and every class path of behaviour 4b adds a few milliseconds. Three things
   follow for a program that reaches a breakpoint or its end at once: the breakpoint is passed,
   the stop is not reported, or the end is not reported. With the requests of v4 alone, a
   program with a breakpoint on the fourth line of its `main` stopped there in two of eight
   launches, ran past it in four and never reported its end in two. [OSS: `JDILauncher.kt`,
   `JDIDebuggee.kt`, `VMEventBus.kt`, `KotlinDebugAdapter.kt` of that version; DAP traces of
   2026-11-11; STD: `java.lang.instrument`]
   - **The program waits for the adapter.** IDECraft launches the program with a Java agent
     of its own, added to the launch's `vmArguments` (`-javaagent:<jar>=<main class>=<file>`,
     after what the launch already has there). The agent does one thing: where the JVM is about
     to define the main class — the debugger is connected by then, no line of the program has
     run — it waits until the file is gone, at most 30 seconds. IDECraft creates the file
     before `launch` and removes it when `launch` is answered, also when the launch fails; by
     then the breakpoints are registered and the adapter passes on what the program reports.
     The program does not see the agent: no frame, no output, no changed class.
   - **Where the agent comes from.** Its source is part of IDECraft
     (`crates/debugger/src/kotlin_gate/IdecraftGate.java`). It is compiled once per user with
     the `javac` and `jar` of `JAVA_HOME`, else of the PATH, for class file version 8 (11 when
     the JDK no longer writes 8), into the user's cache folder: `kotlin-gate/<digest of the
     source>/gate.jar`, built aside and moved into place; the folder and the folder of the
     files to wait for are readable by the user only (security first: nobody else may put code
     there). Without these tools, without a cache folder, for a main class of a named module,
     for a path with a `"` or a `\`, and for an adapter on another machine the program starts
     as before, and what follows is all that helps.
   - **Order.** The breakpoints of the file that declares the main class of the launch are sent
     before those of other files; the adapter registers in this order.
   - **A stop that is not reported.** The breakpoint suspends its thread, and no `stopped`
     event follows; the session would show "running" for ever. When nothing has come from a
     launched program 300 ms after IDECraft began to wait, IDECraft asks once for the threads
     and for the stack of each (two seconds per question, four in all). Only a suspended thread
     has a stack: one whose top frame is in the file and on the line of a requested breakpoint
     is the stop, and it is handled like a reported one (conditions of SPEC-0178 included). A
     `stopped` event that arrives in the next 100 ms wins. A thread suspended elsewhere, or a
     running program, changes nothing, and the question is not asked again: whatever was lost
     was lost before the answer to `launch`.
   - **An end that is not reported.** The program has ended, stays suspended in its last
     moment, and no `exited` event follows. In the same answers this shows as every thread
     having a stack (also an empty one), the JVM's `DestroyJavaVM` thread among them, and none
     at a breakpoint: the session ends as after `exited`, without an exit code. An `exited` or
     `terminated` event that arrives in the next 100 ms wins.
   - **A breakpoint that is passed** cannot be found out afterwards. It does not happen while
     the program waits for the adapter; without the agent the limit stays (see the deviations).
   - Other adapters, and sessions that attach, get no agent and are not asked.
4d. **A command right after a stop (v6):** what the user or an agent sends between the moment a
   stop is shown and the moment the session has read the stop's threads and values — Resume
   pressed at once — is carried out for the stopped program. (It was dropped: the session
   emptied its command queue as for a running program once more. The window self-test's "resume
   runs the Kotlin program to its end" failed this way.) [GEN]
5. **MCP:** `generate_code` works in `.kt` files (`constructor`, `equals_hash_code`, `to_string`);
   `debug_start` takes `main_class` with the project folder as `program`; `debug_adapters` lists the
   adapter. [ADR-0008]

## Acceptance criteria
- **AK-1** Kotlin Generate for properties of both kinds, arrays, nullable types, an existing
  `equals`, a data class, a class without body (`jvm::tests::kotlin_generate_…`); run commands of
  Kotlin in Gradle and Maven modules (`crates/core/tests/it/run_context.rs`); the plan of such a
  command (`debug_model::tests::kotlin_commands_…`).
- **AK-2** With the adapter and a Gradle project built offline: a stop in `twice` with `x = 1`,
  two more stops, the end with `total 12`; a condition evaluated by IDECraft (SPEC-0178 AK-3)
  (`crates/debugger/tests/it/kotlin.rs`).
- **AK-3** MCP: Kotlin `generate_code`, `debug_start` dry run (`crates/mcp/tests/it/protocol.rs`).
- **AK-4** Window self-test: ⌘N in a Kotlin class lists the Kotlin choices, `toString()` is
  generated and undone (`kotlin_generate_steps`); ⌃⇧D in a Gradle project's `Main.kt` builds it,
  stops in `twice` with `y = 42`, and resume runs to "kotlin result 42" (`kotlin_debug_steps`).
- **AK-5** Hand test (round 4): Generate and debugging in a real Kotlin project next to the
  reference.

- **AK-6 (v3)** The Kotlin list reads Secondary Constructor, equals() and hashCode(), toString(),
  Override Methods…, Implement Methods… (unit test `workbench_java::tests::kotlin_generate_list`); in
  a window choosing "Override Methods…" opens *Override Members*
  (`crates/workbench/tests/it/kotlin_structure.rs`).

- **AK-7 (v4)** With the adapter and a Gradle project built offline: paused in a top-level `main`
  the locals have no `this`; a mutable list reads `ArrayList@<id> size = 1`, a map `size = 2`, a
  set `size = 2`, an empty list `size = 0`, a boxed `Int?` its number, an array its length; after
  a step into a member function `this` is the object
  (`crates/debugger/tests/it/kotlin.rs::kotlin_values_have_sizes_and_no_this_without_a_receiver`).

- **AK-8 (v5)** With the adapter and a Gradle project of two files built offline: breakpoints in
  the top-level `main` of the launched file and, in the other file, in a member with an
  expression body and in one with a block body of a class named unlike its file stop in this
  order, and the program ends with its output
  (`crates/debugger/tests/it/kotlin.rs::kotlin_breakpoints_in_class_members_of_another_file_are_hit`).
  In the same file a lambda in a member, an anonymous object, a companion object, a second class
  and a top-level function stop in the order the program reaches them, and a member breakpoint
  with a condition evaluated by IDECraft stops once
  (`…::kotlin_breakpoints_in_lambdas_inner_classes_and_companions_are_hit`).
- **AK-9 (v5, v6)** Without the adapter: the declarations of a file outside comments, strings,
  templates and bodies; the class paths for a class named unlike and like its file, for
  top-level code, for a package that differs from the folder, for `@file:JvmName`; a package
  with a part called `kotlin` gives no path and the line as not bindable; the file of a main
  class (`kotlin_classes::tests`). Against a fake adapter that verifies everything and, like the
  real one, never answers `configurationDone`: the requests and their order before
  `configurationDone`, the file of the main class first, the emptied class paths when
  breakpoints change or go, nothing extra for another adapter, a `.java` file or an unreadable
  file; a refused path for inner classes said once with the breakpoint kept; a class path that
  is never answered, after which `configurationDone` is sent and only that class's line is not
  verified; the not verified breakpoint with its hint and its one console line; a stop that is
  not reported, found once and resumed to the end; a thread suspended at another line and a
  running program, asked for once and left alone; an end that is not reported; another adapter
  is not asked (`crates/debugger/tests/it/kotlin_class_sources.rs`).
- **AK-10 (v6)** The window's session (`debug_model::Controller`) with the adapter and Gradle
  projects built offline, no launch tried again: a file with seven top-level declarations and a
  `main` without a pause stops in `main` and then in the member of the class named unlike its
  file, and resume after the last stop runs the program to its end with its output, four
  launches in a row; with the only breakpoint in a member that is never called the program is
  reported as ended, four launches in a row; a program that still runs when IDECraft asks for a
  lost stop stops later and ends; the program of the window self-test stops in `twice` and
  resume, sent as soon as the stop is shown, runs it to "kotlin result 42", three launches in a
  row (`crates/workbench/tests/it/kotlin_debug_members.rs`). The agent: built once into a cache
  folder, the same jar for a second session, a file to wait for per session that goes with the
  session's guard, no agent for a main class of a named module
  (`kotlin_gate::tests`; skipped without `javac` and `jar`).

- **AK-11 (v7)** As SPEC-0127 AK-9 (v5); a line of 4b that cannot be bound carries 4b's reason as
  `reason` (`crates/debugger/tests/it/kotlin_class_sources.rs`). In the window: to be seen on the Mac.

## Out of scope
Kotlin inspections with type information, Kotlin tests from the debugger, attach to a running JVM,
Kotlin Multiplatform, Kotlin scripts (`.kts`), coroutine debugging.

## Known deviations from the reference
The reference debugs Kotlin with its own JVM debugger and supports conditions; IDECraft uses the open
kotlin-debug-adapter and evaluates conditions itself, which is slower (SPEC-0178). Not compared side by side.

v6: IDECraft runs a small agent of its own inside the debugged Kotlin program's JVM (behaviour
4c); the reference needs none, its debugger holds the program itself. Where the agent cannot be
used (no `javac` and `jar`, a main class of a named module, an adapter on another machine), a
line that runs in about the first tenth of a second of the program, before the adapter has
registered its breakpoint, is passed without a stop, as before v5. A breakpoint in the static
initialiser of the main class or in code that runs before the main class is defined (another
agent) is outside the wait.

v5 (OBS-0035 L14): limits that remain with this adapter. Breakpoints in `.java` files of a Kotlin
module are sent as before and work only in the class named like the file. A file with
`@file:JvmMultifileClass` is not handled. A class declared in `A.kt` and named like another file
`B.kt` of the same package also receives the lines of `B.kt` from the adapter. A breakpoint in an
unsaved file is placed by the lines on disk. A file whose folder differs from its package is
bound, but the adapter may not find its source for the frames of a stop; this was not tried with
the real adapter. The window has no separate marker for a breakpoint that is not verified; it
says so in the debug console. The member breakpoint was not compared with the reference.

v3 (OBS-0035): the reference's list also has Test… and Copyright. Neither is in IDECraft's Generate
list, for Java either: creating a test for a class is not built yet, and the copyright notice is its
own action (SPEC-0155).
