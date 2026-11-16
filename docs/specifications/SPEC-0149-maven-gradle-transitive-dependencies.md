# SPEC-0149: Transitive Maven and Gradle dependencies

- **Version:** 4 · **Date:** 2026-11-06 (v2 after OBS-0024: missing artifacts, the build tools
  window's options in the resolution, Gradle's task list; behaviours 6–8 new; v3 for the open gap L7
  of OBS-0024: the entries Maven leaves out (conflicts, duplicates), downloading sources and
  documentation, library sources in the editor; behaviours 9–11 new; v4 after the window check:
  the library source scrolled to its declaration, named in the status line and not checked;
  behaviour 11 changed)
- **Sources:** GEN (IDEs import Maven and Gradle projects through the build tool, show each
  module's resolved dependency tree with transitive dependencies, Gradle per classpath
  configuration with the versions Gradle chose, and reload when the build file changes or on
  request; F-003), STD (`mvn dependency:tree` text output of the Maven Dependency Plugin;
  `gradle dependencies` report with `+---`/`\---` levels, `a -> b` for a chosen version, `(*)`
  repeated, `(c)` constraint, `(n)` not resolved; wrapper scripts `mvnw`, `gradlew`), SPEC-0048,
  SPEC-0100, ADR-0008
- **Feature matrix rows:** F-003

## Purpose
See what a Maven or Gradle module really pulls in, not only what its build file names.

## Behaviour
1. **Resolution by the build tool:** a Maven module is resolved with `mvn -B -q -N
   dependency:tree` (the project's `mvnw` when there is one), a Gradle module with `gradle -q
   dependencies` (`gradlew` when there is one), each in the module's folder. A run is stopped
   after five minutes. [STD]
2. **Maven tree:** each entry of the output becomes a node `group:artifact` with its version; the
   direct dependencies keep their scope (`compile`, `test`, …, with `optional` where marked). [STD]
3. **Gradle tree:** one node per classpath configuration that has dependencies
   (`compileClasspath`, `runtimeClasspath`, `testCompileClasspath`, `testRuntimeClasspath`), the
   dependencies under it; a version Gradle replaced reads `2.0.7 → 2.0.9`; repeated subtrees are
   marked; constraints and unresolvable entries are left out; `project :lib` appears as `:lib`.
   Only the module's own project is read from a report that covers sub-projects too. [STD]
4. **Keeping and reloading:** the output is kept in `.idecraft/deps/` and used until a build file
   of the module changes (`pom.xml`; `build.gradle[.kts]`, `settings.gradle[.kts]`,
   `gradle/libs.versions.toml`, also those at the root). Opening the Build Tools window resolves
   modules whose result is missing or stale in the background ("Resolving dependencies…"); the
   reload button resolves all Maven and Gradle modules again. Until then the direct dependencies
   are shown as before. A failed resolution is shown under the header with the module and the
   tool's last lines; the previous result stays. A request while a resolution runs is done after
   it. [GEN]
5. **MCP:** `dependency_tree` returns the kept tree for Maven and Gradle modules and `resolved`
   (whether it is current); `resolve: true` runs the build tool first, needs the `run` right and
   is logged in `.idecraft/agent-log.jsonl`. [ADR-0008]
6. **(v2) Missing artifacts:** Maven lists an artifact it could not download like any other; a Maven
   entry whose file is not in the local repository (`~/.m2/repository`, or `localRepository` of
   `~/.m2/settings.xml`; a jar, a POM for `pom`, the classifier in the name) is marked `missing`.
   Gradle's `FAILED` entries are kept and marked `missing` (the version without `FAILED`). The build
   tools window and `dependency_tree` name them (SPEC-0100 v2 behaviours 8, 11). [STD, OBS-0024]
7. **(v2) Options:** the build tools window's offline mode and Maven profiles (SPEC-0100 v2
   behaviour 6) are passed to the resolution too (`-o`, `-P …`, `--offline`). [STD]
8. **(v2) Gradle's tasks:** after a successful Gradle resolution `gradle -q tasks --all` runs in the
   module's folder and its output is kept beside the tree (`….gradle.tasks.txt`) for the window's task
   groups (SPEC-0100 v2 behaviour 3); a failure keeps the previous list. [STD]
9. **(v3) Left-out entries:** the Maven resolution passes `-Dverbose`; an entry Maven left out of the
   tree — `(g:a:jar:3.14.0:compile - omitted for conflict with 3.12.0)`, `omitted for duplicate`,
   with `version managed from …` before it — is kept as an omitted node with its reason and without
   children; it is never missing. The window's Dependencies, External Libraries and the missing
   artifacts leave omitted nodes out; the dependency analyzer and the diagram use them (SPEC-0100 v3
   behaviours 14, 15). `dependency_tree` returns them with `omitted`. [STD]
10. **(v3) Sources and documentation:** downloading runs the build tool, with the window's options:
    for each Maven module `mvn -B -N dependency:sources` and `mvn -B -N dependency:resolve
    -Dclassifier=javadoc` in its folder; for each Gradle build `gradle --init-script
    <.idecraft/gradle/idecraft-sources.gradle> -PidecraftKinds=sources,javadoc idecraftDownloadSources`
    from its root, a task the init script adds to every project, which asks Gradle for the sources
    and Javadoc artifacts of the components on the resolvable classpaths. Whether a library has
    them is read from the local caches: for Maven `<artifact>-<version>-sources.jar` and
    `-javadoc.jar` beside the jar in the local repository, for Gradle the same names in any folder of
    the version in `~/.gradle/caches/modules-2/files-2.1/<group>/<artifact>/<version>/` (the Gradle
    cache keeps each file in a folder of its own). Offline, the build tool only finds what it fetched
    before (Gradle then usually nothing, which is said). [STD]
11. **(v3) Library sources in the editor:** when a declaration, type or implementation found by the
    language server is only in a library class (SPEC-0042 v7 behaviour 16), its source is read from
    the library's sources archive (as in behaviour 10, also the Gradle cache's other folders, and
    the JDK's `lib/src.zip`), copied to IDECraft's cache folder
    (`<user cache>/library-sources/<archive>/<package path>/<Class>.java`) and opened at the
    declaration (the place the language server named; else the line that declares the class). The
    copy is written again from the archive each time. (v4) The jump scrolls the declaration into
    view like any other jump, also in a short editor area; the file is read-only (typing, deleting,
    pasting, completions and saving change nothing); the status line says it is a copy from
    `<archive>`, read-only, not checked and not used by the build; no diagnostics are shown for it (outside its project the language
    server reports errors that are none). Without a sources archive the status line
    names the class and says that Download Sources in the build tools window fetches it.
    [GEN, STD]

## Acceptance criteria
- **AK-1** Parsing of Maven and Gradle output, keeping and staleness, wrapper choice
  (`jvmdeps::tests`).
- **AK-2** With real `mvn` and `gradle`: junit's `hamcrest-core` appears under it, Gradle lists
  `testRuntimeClasspath` and leaves the empty `compileClasspath` out, a broken POM reports Maven's
  error and keeps nothing (`crates/core/tests/it/jvm_resolve.rs`).
- **AK-3** MCP: direct dependencies and `resolved: false` before, refusal without the `run` right,
  the resolved tree with `mvn` and the log entry (`dependency_tree_resolves_maven`).
- **AK-4** Window self-test: a new Maven module in the open Build Tools window is resolved and
  then shows junit with `hamcrest-core`.
- **AK-5** Hand test (round 4, no. 75): a Maven and a Gradle project next to the reference's
  Maven and Gradle tool windows.

- **AK-6** Missing artifacts: Maven entries the repository lacks and Gradle `FAILED` entries are
  marked; repository paths of jars, POMs and classifiers (`jvmdeps::tests::missing_artifacts_are_marked`).
- **AK-7** (v3) Verbose Maven output: omitted entries with their reasons, managed versions, never
  missing, left out of the libraries (`jvmdeps::tests::verbose_maven_entries`); with real `mvn` a
  conflict is kept as omitted (`crates/core/tests/it/jvm_resolve.rs`).
- **AK-8** (v3) Sources: the download commands per tool with the options, the init script, the
  availability of sources and Javadoc in a Maven repository and a Gradle cache layout, reading the
  init script's report (`jvmsources::tests`); MCP `library_sources` (`protocol.rs`:
  `build_tools_v3_tools`).
- **AK-9** (v3) Library source: a `jdt://` place of a library class finds the class in the sources
  archive beside the jar and in another folder of the Gradle cache, is copied and opened at the
  declaration (`javadoc::tests::library_source_from_archives`); ⌘B on a library class in the window
  is checked by hand next to the reference.

## Out of scope
Resolving through an embedded Maven or the Gradle Tooling API, reloading automatically while the
window is closed, decompiling classes without sources, downloading sources for npm or Cargo.

## Known deviations from the reference
The tree comes from the build tool's own report, so resolution matches the command line exactly;
the reference imports the model in-process. Maven modules are resolved one by one without the
reactor (`-N`); siblings that are not installed may fail to resolve and say so.
