# SPEC-0100: Build tools window — tasks and dependency trees

- **Version:** 5 · **Date:** 2026-11-14 (v5: Recompile builds the module of the file, behaviour 22, AK-20; eighth package of docs/research/KEYMAP_COVERAGE.md; v2: side-by-side comparison OBS-0024 — modules nested as their
  builds nest them, Maven lifecycle, plugins with goals, repositories and profiles, Gradle task groups
  run from the build's root, offline and skip-tests switches, Execute, Expand/Collapse All, problems
  with their place, the task menu, the sync badge and ⇧⌘I, test runs with the test tree, MCP
  `build_tool_options`; behaviours 1–3 and 5 changed, 6–12 new; v3: the open gaps L7, L8 and L22 of
  OBS-0024 — sources and documentation, the dependency analyzer with conflicts and paths, the
  dependency diagram as text, linking and unlinking build files, debugging a task, tasks before or
  after Build Project and before every run, a shortcut per task, suggestions in the Execute field,
  MCP `build_command_suggestions`, `dependency_analysis`, `library_sources`, `build_tool_links`,
  `build_task`; behaviours 10 and 11 changed, 13–21 new; v4 after the window check next to the
  reference (OBS-0024, re-check 2): the analyzer lists every artifact with its paths, the diagram is
  drawn over the editor with Mermaid, SVG and PNG export, a debugged Maven test JVM is found by its
  port (Surefire 3 does not pass the agent's line on), Build Project in its own Run tab without
  changing the run widget, tasks before and after a sync, check marks in their own column, long
  details of suggestions cut; behaviours 10, 14, 15, 17, 18 and 20 changed)
- **Sources:** GEN (IDEs show a tool window per build tool — Maven, Gradle, npm — with the project's
  modules, their lifecycle phases, tasks or scripts to run by double click, and their dependencies as
  a tree with transitive dependencies; F-003), OSS (action ids `ActivateMavenToolWindow`,
  `ActivateGradleToolWindow`, `ActivatenpmToolWindow`, `ExternalSystem.ProjectRefreshAction`), STD
  (`Cargo.lock` package entries with `dependencies`; `package-lock.json` v2/v3 `packages` keyed by
  install path and Node's lookup through enclosing `node_modules` folders; Maven's lifecycles, default
  plugin bindings, plugin prefixes and the goals of the common plugins, `-o`, `-DskipTests`, `-P`;
  Gradle's project paths, `gradle tasks --all`, `--offline`; the JDWP agent's
  `-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=…` and its "Listening for transport
  dt_socket at address" line, Surefire's `maven.surefire.debug`, Gradle init scripts and
  `Test`/`JavaExec` `jvmArgs`; Mermaid flowchart text), OBS-0024 (comparison with the reference,
  toolbar and task menu of the reference's Maven and Gradle windows), SPEC-0048 (project model),
  SPEC-0087 (run configurations), SPEC-0105 (keymap), SPEC-0147 (attaching to a JVM), SPEC-0149
  (resolution, sources)
- **Feature matrix rows:** F-003

## Purpose
See what a project builds, run its tasks and find where a dependency comes from, for every build tool
in one place.

## Behaviour
1. **Window:** "Build Tools" opens on the right side (stripe button; `ActivateMavenToolWindow`,
   `ActivateGradleToolWindow` and `ActivatenpmToolWindow` from Search Everywhere) instead of the
   Database window. From the top it lists the problems (behaviour 12), "Profiles" when the project has
   Maven profiles (behaviour 6), then the modules. A module is named as its build names it — the
   Maven artifactId, the Gradle project, the npm package — with the build tool and folder dimmed
   after it (`shop-core  Maven · core`). Members are nested under the module that lists them (a Maven
   aggregator's modules, a Gradle build's projects), sorted by name, after the module's own nodes.
   Modules and profiles start expanded, groups collapsed. ↑/↓ select, →/← expand, collapse or go to
   the parent, Enter runs or toggles, Esc hides, ⇧Esc hides it as a side window. [GEN, OBS-0024]
2. **Tasks:** Maven modules have "Lifecycle" with the phases `clean validate compile test package
   verify install site deploy` and "Plugins": the plugins the packaging binds by default (jar: clean,
   resources, compiler, surefire, jar, install, deploy, site; pom: clean, install, deploy, site; war
   with war instead of jar) and those of the POM, each as its prefix with its coordinates dimmed and
   its goals (`compiler:compile`, …; unknown plugins `help`); the build's top POM also has
   "Repositories" (Maven Central and the POM's own, with their URLs). Gradle modules have "Tasks"
   grouped as Gradle lists them (behaviour 3). Cargo `build check test clippy doc run`; npm `install`
   and the scripts; Go `build test vet` over `./...` and `mod tidy`; Python `pytest` when it is a
   dependency. A double click or Enter runs the task as a configuration of the session named as the
   reference names it — `shop-core [test]` for Maven, `gradle-kts [:lib:test]` for Gradle, `name
   [task]` otherwise — selected in the run widget, with its output in the Run window; Maven's `test`
   and Gradle's `test` read the JUnit reports of the run into the test tree (`target/surefire-reports`,
   `build/test-results/test`). [GEN, STD, OBS-0024]
3. **Gradle from the build's root:** a Gradle module belongs to the build whose settings file is in its
   folder or the nearest one above; its tasks run there through that build's wrapper (`./gradlew`)
   or `gradle`, as `:lib:test` for the project in `lib` (a sub-project has no wrapper of its own).
   Once Gradle resolved the module (SPEC-0149), its tasks are those of `gradle tasks --all`, grouped
   by Gradle's groups in lower case (`build`, `documentation`, `help`, `other`, `verification`, …),
   groups and tasks sorted; before that the common tasks `build assemble check test clean
   dependencies`. [STD, OBS-0024]
4. **Dependencies:** Cargo and npm modules show the transitive tree from their lock file (the nearest
   one in the module folder or above); each package is expanded once and later occurrences are marked
   "(*)", the depth is limited to 12. Maven and Gradle modules show the build tool's tree
   (SPEC-0149); an aggregator without dependencies has no Dependencies node. Direct dependencies show
   their scope (dev, build, test, …). Other build tools list their direct dependencies with the
   locked or required version. A lock file that cannot be read shows its error under
   "Dependencies". The refresh button reads the build files again and resolves all Maven and Gradle
   modules; expanded rows stay expanded. Reading happens off the UI thread ("Reading…"). [STD, GEN]
5. **Execute:** a toolbar button asks for a command line for the selected row's module, prefilled
   with `mvn ` or `gradle `; Enter runs it like a task — in the module's folder, Gradle from the build's
   root with the project path before bare task names — with the window's options; the field names the
   module ("Execute Maven Goal in shop-core"). [GEN, OBS-0024]
6. **Options:** toolbar switches for offline mode (Maven `-o`, Gradle `--offline`) and "skip tests"
   (Maven `-DskipTests`; the test phases are struck through), and the profiles: each Maven profile
   of the project's POMs with a checkbox, checked when active (active by default unless switched off);
   a click or Space switches it. Profiles that differ from their default become `-P a,!b`. The
   options apply to every task, every typed command and every resolution (SPEC-0149 v2), are shown in
   the run's command line and kept per project in `.idecraft/build-tools.toml`; switching a profile
   resolves the modules again. [STD, OBS-0024, IMP-0337]
7. **Expand All / Collapse All:** toolbar buttons open every module and group (not the dependency
   trees, which can be large) or close all. [GEN]
8. **Missing artifacts** (SPEC-0149 v2 behaviour 6): a dependency the build tool could not find has a
   warning icon and "not found"; each one is also a problem (behaviour 12) at the line of the build
   file that names it. [OBS-0024]
9. **Sync badge:** the build files of the Maven and Gradle modules (`pom.xml`, `build.gradle[.kts]`,
   `settings.gradle[.kts]`, `gradle/libs.versions.toml`; a build file opened later is watched from
   then on) are compared with their state at the last sync, without comments and blank space. When
   one changed — saved in the editor or changed by another program — its editor shows a badge in the
   top right corner: a sync button ("Sync Build Changes", ⇧⌘I on macOS, ⌃⇧O otherwise, also from
   Search Everywhere) and a close button that hides it until the next change. Syncing saves open build
   files, reads the project model again, resolves the changed modules (not in safe mode, SPEC-0278)
   and refreshes the project tree; a change only to comments shows no badge. Nothing of the build runs
   without the user's click. [GEN, OBS-0024, IMP-0338]
10. **Task menu:** a right click on a task offers Run, Debug (behaviour 17), Create Run Configuration
    (kept in the project's run configurations under the task's name, unless one has that name),
    Execute Before Sync, Execute After Sync, Execute Before Build, Execute After Build and Execute
    Before Run (behaviour 18, checked when set; v4: every label of a menu with a check mark starts in
    the same column), Assign Shortcut… (behaviour 19) and Copy Command Line (with the options).
    [GEN, OBS-0024]
11. **MCP:** `dependency_tree` returns the tree, the tasks, the window's `task_groups` and the
    `missing` artifacts of a module; `build_tool_options` reads the options with the project's
    profiles (default and active) and sets them with the `write` right. The tools of behaviour 21
    cover the rest of the window. [ADR-0008]
12. **Problems:** a build file that cannot be read (with line and column: `core/pom.xml: line 20,
    column 3: …`), a missing artifact and a failed resolution are listed at the top with an error
    icon and their file and line; Enter or a double click opens the file there. They show the state of
    the last reading only. [OBS-0024, IMP-0335]
13. **(v3) Sources and documentation:** a toolbar button downloads the sources and the documentation
    of every library of the Maven and Gradle modules in the background ("Downloading sources…") as
    SPEC-0149 v3 behaviour 10 describes; afterwards the status line says how many libraries have
    sources and documentation and names those the repositories do not have. ⌘B on a library class
    then opens its source (SPEC-0149 v3 behaviour 11). [GEN, STD, OBS-0024]
14. **(v3) Dependency analyzer:** a toolbar button analyses the selected row's module (else the first
    Maven or Gradle module) and shows at the top of the window, below the problems, a group
    "Dependency Analyzer · <module>" (expanded; the button again closes it; a row of the group
    stands for the analysed module): first the conflicts —
    an artifact the tree reaches in more than one version — each with the version the build tool
    chose dimmed after it (`org.apache.commons:commons-lang3  3.12.0 · 2 versions`), then under it
    every place it is reached: the version asked for and whether it was used, omitted for the
    conflict or replaced (`3.14.0 · omitted for conflict with 3.12.0`), with its path from the module
    through the libraries that bring it in (`l7-core → commons-text 1.12.0 → commons-lang3 3.14.0`;
    Gradle paths start with the classpath). "No version conflicts" (an information, not a warning)
    when there are none. (v4) Then "All Dependencies · <n> artifacts", expanded: every artifact of
    the tree with the version used and how many paths reach it (`· conflict` when it is one), each
    opening to its places as above — where a library comes from, also without a conflict; typing
    while the analyzer is open narrows this list to the names containing the text (`1 of 15 · “guava”`),
    Backspace and Escape take it back. Enter on a
    place opens the build file at the line of the direct dependency the path starts with. The
    analysis uses the kept resolution (Maven with the left-out entries of SPEC-0149 v3 behaviour 9);
    a module not resolved yet is resolved first. [GEN, OBS-0024, IMP-0341]
15. **(v3, v4) Dependency diagram:** a toolbar button draws the selected module's resolved tree over
    the editor area ("Dependency Diagram · <module>-dependencies · n artifact(s), m dependencies"):
    the module in the first column, a box per artifact and version (artifact id and version, the
    group below) in the column of its shortest path, an edge per dependency, left-out entries with
    dashed edges, artifacts in conflict with a dashed border and "conflict"; Gradle's classpaths are
    merged. Zoom, 1:1, Fit, Layout, dragging boxes and Escape work as in the ER diagram (SPEC-0111).
    The same graph is written as a Mermaid flowchart to
    `.idecraft/diagrams/<module>-dependencies.mmd` (conflicts in class `conflict`, left-out edges
    labelled with the reason, Gradle's classpaths as subgraphs); Export copies and writes it, or an
    SVG or PNG of the drawing. The text can be diffed, kept in the repository, rendered by any
    Mermaid viewer and read by agents (`dependency_analysis` `diagram`). [STD, OBS-0024, IMP-0339]
16. **(v3) Link and unlink build files:** a "+" button asks for a build file (`pom.xml`,
    `build.gradle[.kts]`, `settings.gradle[.kts]`; workspace-relative, prefilled with the selected
    module's folder) and links it: the project model reads it even when the workspace leaves its
    folder out (an ignored or excluded folder), and an earlier unlink of its folder is undone. A "−"
    button unlinks the selected row's module: it and the modules inside its folder leave the model
    — window, project tree roles, External Libraries, resolution — until linked again; the status
    line says how to undo it. Both are kept in `.idecraft/build-tools.toml` (SPEC-0048 v4 behaviour
    12). [GEN, OBS-0024]
17. **(v3) Debug a task:** Debug in the task menu runs the task as in behaviour 2 with a JDWP agent on a
    free local port (suspended until the debugger is there) and attaches the Java debugger
    (SPEC-0147) as soon as the run's output reports the agent listening or, for the first JVM of
    the run, its port is taken (v4: Surefire 3 prints the forked JVM's line only after a debugger
    connected; the port is probed by binding it, nothing connects to it); the Debug window shows the
    session under the run's name and the project's breakpoints apply. Maven: a task that runs the
    tests (`test`, `package`, `verify`, `install`, `deploy`, `surefire:test`) passes the agent to the
    forked test JVM (`-Dmaven.surefire.debug=…`, `-Dmaven.failsafe.debug=…`), any other goal debugs
    the Maven JVM itself (`MAVEN_OPTS`, e.g. `exec:java` and plugin code). Gradle: an init script
    (`.idecraft/gradle/idecraft-debug.gradle`, given with `--init-script`) adds the agent to the
    `Test` and `JavaExec` tasks of the run (`-PidecraftDebugPort=<port>`, one test JVM at a time). A
    run that ends without the agent listening reports that the task started no JVM to debug.
    [STD, GEN, OBS-0024]
18. **(v3, v4) Before and after a sync and Build Project, before every run:** Execute Before Sync,
    Execute After Sync, Execute Before Build, Execute After Build and Execute Before Run in the task
    menu switch a task's trigger (kept in
    `.idecraft/build-tools.toml` by module folder and task); the task row shows its triggers dimmed
    (`clean · Before Build`). Build Project (`CompileDirty`, ⌘F9 on macOS, ⌃F9 otherwise, also from
    Search Everywhere) runs as one chain in the Run window: the Before Build tasks, then the
    project's build — `mvn test-compile` for each top Maven module, `gradle classes testClasses` for
    each Gradle build's root, `cargo build` for each top Cargo module, `go build ./...` for each Go
    module — then the After Build tasks; a failing step ends the chain (SPEC-0087 behaviour 3), so
    After Build runs only after a successful build. (v4) The chain runs in the Run tab "Build
    Project", the status line names its steps, and the run widget keeps the configuration chosen
    before. A sync (⇧⌘I, the badge, the window's Reload button) first runs the Before Sync tasks as
    one chain in the Run tab "Before Sync" and reads the build files only when they succeeded (else
    the status line says the sync stopped); the After Sync tasks run in the tab "After Sync" once
    the window has read and resolved the model. Every run configuration started from the run
    widget runs the Before Run tasks first (as before-launch steps); the build tool tasks started
    from this window do not. [GEN, OBS-0024]
19. **(v3) Shortcut per task:** Assign Shortcut… in the task menu waits for the next key combination
    ("Press the shortcut for shop-core [test]", Escape cancels) and adds it to the user keymap
    (SPEC-0105) for the action `BuildTask:<module folder>|<task>`; the shortcut then runs the task
    from anywhere in the window, other actions on the same keys are named in the status line, and
    the task row shows the shortcut dimmed. Settings › Keymap and `set_shortcut` change or remove it
    like any other action. [GEN, OBS-0024]
20. **(v3) Suggestions in the Execute field:** under the field of behaviour 5 a list shows up to ten
    completions of the word at the end of the line: Maven phases and plugin goals of the module
    (`compiler:compile`), Gradle tasks of the module (as listed, else the common ones), the build
    tool's common options when the word starts with `-` (Maven `-o`, `-U`, `-DskipTests`, `-P`,
    `-pl`, `-am`, `-X`, …; Gradle `--offline`, `--refresh-dependencies`, `--continue`, `-x`,
    `--info`, …) and the project's Maven profiles after `-P`. Each shows dimmed what it is (phase,
    goal, task with its group, option with its meaning; v4: a detail too long for the list is cut
    with an ellipsis inside it). Words already on the line are left out;
    names that start with the word come first, then those that contain it. ↓/↑ choose, Tab or a click
    takes the chosen (else the first) suggestion, Enter takes a chosen suggestion and otherwise runs
    the line. [GEN, STD, OBS-0024]
21. **(v3) MCP:** `build_command_suggestions` returns the suggestions for a module and a line;
    `dependency_analysis` the conflicts, every place an artifact is reached with its path and
    whether it was used, and with `diagram: true` the Mermaid text (`resolve: true` resolves first,
    `run` right); `library_sources` says per library whether its sources and documentation are in
    the local caches and with `download: true` (`run` right, logged) downloads them;
    `build_tool_links` lists linked and unlinked build files and links or unlinks one (`write`
    right); `build_task` returns a task's run configuration, its debug variant (command,
    environment, port), its triggers and shortcut action id, sets the triggers (`write` right), and
    without a task returns the triggers and the Build Project chain. [ADR-0008]
22. **(v5) Recompile:** `Compile` (⇧⌘F9, Ctrl+Shift+F9; Build › Recompile) builds the module of
    the active file only, or of the file selected in the Project tree while it has the focus:
    the innermost module of the project model whose folder holds the file. Maven runs
    `mvn compile` in the module's folder; Gradle runs the module's `classes` task from the
    build's root folder (`:path:classes` for a subproject, `classes` for the root); Cargo runs
    `cargo build` in the crate's folder; Go runs `go build .` in the file's folder. The step runs
    in a Run tab of its own named after the module and the task, with the window's build
    options (offline, profiles) as Build Project's steps get them; the status bar names the
    step. A file outside every Maven, Gradle, Cargo and Go module says "Nothing to compile: the
    file is in no Maven, Gradle, Cargo or Go module", and so does a window without a file. MCP:
    `build_task` with `file` returns the step (name, command, folder) without running it.
    [OSS: id and keys of the bundled keymaps; GEN; DOC: the reference compiles the module or
    file it was asked for]

## Acceptance criteria
- **AK-1** Cargo tree with a repeated package, npm tree with a nested install, Go direct dependencies and
  tasks, and this workspace's own `Cargo.lock` (`crates/core/tests/it/deptree.rs`); rows of modules,
  groups, tasks and trees, problems and profiles first, members nested and sorted, missing artifacts
  marked, Expand All without dependency trees (unit tests `build_tools_model::tests`).
- **AK-2** MCP: `dependency_tree` of an npm module with a transitive package and its tasks; an unknown
  module is an error (`crates/mcp/tests/it/protocol.rs`).
- **AK-3** In a real window (`idecraft ui --selftest`): the window lists a Go module, its tasks, and
  running "test" shows the failing test in the Run window under `gomod [test]`.
- **AK-4** By hand: a Maven, a Gradle and an npm project next to the reference (OBS-0024).
- **AK-5** Maven lifecycle with nine phases, default and declared plugins with goals, repositories of
  the top POM, run names by artifactId, profiles of the project (`buildtools::tests::
  maven_lifecycle_plugins_and_profiles`).
- **AK-6** Options as flags, profiles only where they differ from their default, kept and read again
  (`buildtools::tests::options_become_flags_and_are_kept`).
- **AK-7** Gradle sub-project tasks run from the build's root with their path; `gradle tasks --all`
  grouped without other projects' tasks and rules (`buildtools::tests::gradle_tasks_run_from_the_build_root`,
  `gradle_task_list_is_grouped`).
- **AK-8** Sync: a real change is pending until the sync, a comment is not, a closed badge comes back
  with the next change, a build file opened later is watched (`build_sync_model::tests`,
  `buildtools::tests::comments_do_not_count_as_changes`); ⇧⌘I and ⌃⇧O are bound when free
  (`keymap` tests).
- **AK-9** MCP `build_tool_options` reads, refuses without the `write` right, sets and resets;
  `dependency_tree` returns the task groups (`protocol.rs`: `build_tool_options_and_task_groups`).
- **AK-10** Window self-test (Maven module): Lifecycle with `site` and `deploy`, Plugins with
  `compiler:testCompile`, offline and skip tests in the task's command, Copy Command Line from the
  task menu, a typed `mvn validate` runs as `mvnapp [validate]`, the badge after a saved change of the
  POM and ⇧⌘I clearing it.
- **AK-11** (v3) Suggestions: phases, goals, Gradle tasks, options after `-`, profiles after `-P`, words
  already typed left out, prefix before infix, completing the last word
  (`buildtools::tests::execute_suggestions`); MCP `build_command_suggestions` (`protocol.rs`:
  `build_tools_v3_tools`).
- **AK-12** (v3) Analyzer and diagram: a Maven tree with an omitted conflict and a Gradle tree with a
  replaced version give the conflict, its places, paths and the used version; the Mermaid text has
  the root, the edges, the dashed omitted edge and the conflict class (`depanalysis::tests`); rows
  of the analyzer group (`build_tools_model::tests::analyzer_rows`); with real `mvn` the verbose
  resolution of a project with a conflict (`crates/core/tests/it/jvm_resolve.rs`).
- **AK-13** (v3) Links: a linked build file in an ignored folder becomes a module, an unlinked module
  and the modules inside its folder leave the model, linking again undoes it
  (`buildtools::tests::linked_and_unlinked_build_files`); MCP `build_tool_links` refuses without
  the `write` right, links and unlinks (`protocol.rs`: `build_tools_v3_tools`).
- **AK-14** (v3) Debug: the debug command of Maven test and other goals and of Gradle tasks with the
  init script and port; the listening line is recognised (`buildtools::tests::debug_commands`).
- **AK-15** (v3) Triggers: kept and read again, shown on the task, the Build Project chain with
  before and after tasks and the build steps per tool, Before Run tasks put before run
  configurations but not before themselves (`buildtools::tests::triggers_and_build_project_chain`);
  MCP `build_task` sets them (`protocol.rs`: `build_tools_v3_tools`).
- **AK-16** (v3) Shortcut: the action id of a task and back, the task it runs
  (`buildtools::tests::task_action_ids`).
- **AK-17** (v3, v4) Window self-test (Maven module): the Execute field suggests `compile` for `mvn
  comp` and Tab completes it; the task menu shows Debug, the triggers and Assign Shortcut; Execute
  Before Build marks the task and Build Project runs the chain in the tab "Build Project"; Execute
  Before Sync marks a task and a sync runs it in the tab "Before Sync" before reading; the analyzer
  group of the module with "All Dependencies" and junit; the diagram shows the module, junit and
  hamcrest as boxes and keeps the Mermaid file, Escape closes it; unlink and link again; a recorded
  shortcut runs the task.
- **AK-18** (v4) Analyzer rows with all artifacts and their paths (`build_tools_model::tests::
  analyzer_rows`); a port a process listens on counts as listening (`buildtasks::tests::
  a_taken_port_is_listening`); Before/After Sync chains in their own tabs
  (`buildtasks::tests::triggers_and_build_project_chain`); MCP `build_task` takes `before_sync` and
  `after_sync`.
- **AK-19** (v4) By hand next to the reference (OBS-0024, re-check 2): check marks, shortcut
  recording, a Maven test debugged to its breakpoint, suggestions, analyzer, diagram, ⌘B into a
  library class, sources, link/unlink, icons in Islands Dark and Light, Build Project ⌘F9.
- **AK-20** (v5) The step for a file of a Maven module, a Gradle subproject and the root
  project, a Cargo crate, a Go package, a nested module and a file outside every module
  (`buildtasks::tests::recompile_steps_build_the_files_module`); `build_task` with `file`
  (protocol test `build_task_names_the_recompile_step`); in a window of the test harness the
  action starts the step of the file's crate in its Run tab and a text file outside every module
  says so (`eighth_package_keymap.rs::recompile_builds_the_module_of_the_file`).

## Out of scope
Profiling a task, a graphical dependency diagram inside the IDE (the diagram is text, behaviour 15),
choosing per run configuration which Before Run tasks apply (they apply to every run configuration),
decompiling library classes without sources.

## Known deviations from the reference
- The reference has one window per build tool, npm's only after a command on `package.json`; IDECraft
  shows all modules of all build tools in one window, so a mixed project (e.g. a Rust backend with an
  npm frontend) is seen at once.
- The reference syncs a Maven project by itself when its POM changes outside the editor; IDECraft
  shows the badge and runs the build tool only on request (IMP-0338).
- The reference's Gradle window closed every expanded node after a sync; IDECraft keeps them
  (IMP-0336).
- Offline mode: the reference's Maven run still downloaded from Maven Central with the switch on
  (OBS-0024); IDECraft puts `-o` into every command and shows it (IMP-0337).
- Problems show the state of the last reading only; the reference kept an old error beside the new
  one (IMP-0335).
- Dependency diagram: like the reference (OBS-0024, re-check 2) IDECraft draws the graph; it also
  keeps it as Mermaid text and exports SVG or PNG, so it can be diffed, versioned and read by agents
  (IMP-0339).
- Debugging a Maven test task: the reference debugs the Maven JVM, so a breakpoint in the test is not
  reached (OBS-0024, re-check 2); IDECraft attaches to Surefire's forked test JVM and stops there
  (IMP-0350).
- Download Sources and Documentation: the reference offers three choices and says nothing afterwards;
  IDECraft downloads both and names how many libraries have them and which have none (IMP-0351).
- Assign Shortcut: the reference opens the Keymap settings at the task; IDECraft takes the next key
  combination directly and names other actions on the same keys (IMP-0352).
- The reference also offers tasks before and after Rebuild; IDECraft has no compiler of its own and
  no Rebuild (behaviour 18).
- Dependency analyzer: each place of a conflict opens the build file at the direct dependency that
  brings the version in, and agents get the same analysis (IMP-0341).
- IDECraft has no compiler of its own; Build Project, to which the Before/After Build triggers
  belong, runs the build tools' compile steps (behaviour 18).
- v5: the reference recompiles a single file with its own compiler; IDECraft has no compiler of
  its own and builds the whole module of the file with its build tool.
