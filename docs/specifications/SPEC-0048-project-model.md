# SPEC-0048: Project model from build files

- **Version:** 8 · **Date:** 2026-11-16 (v8: a configuration that starts a Spring Boot, Quarkus or
Micronaut application, behaviour 6a, headless pre-check of F-040, OBS-0073; v7 2026-11-13: the folder roles of Maven and Gradle modules whose build file lies in a
sub-folder of a project without one, behaviour 10a new; v6 after OBS-0031 finding T1: source and test folders
recognised from the files where no build file declares them, behaviour 14 new; v5 2026-11-06; v2: Gradle dependencies and version catalog, Maven parent chain, properties and dependency management, lock files; v3 after OBS-0024: Maven profiles, plugins, repositories and source folders, the roles of module folders in the project tree, External Libraries from the resolved trees with the JDK's folder, POM errors with line and column; behaviours 5 changed, 9–11 new; v4 for OBS-0024 L7: build files linked and unlinked by hand, behaviour 12 new; v5 after the window check: the project tree follows a
link or unlink at once, the Java language server writes no Eclipse files into the project; behaviour 12
changed, 13 new)
- **Sources:** GEN (IDEs read a project's build description and show its modules, dependencies, SDK/
  language level and runnable tasks; F-003), STD (the build tools' documented file formats: Cargo
  manifest incl. workspaces and inherited fields; npm `package.json` incl. workspaces; Python
  `pyproject.toml` PEP 621 and Poetry, PEP 508 requirement strings, `requirements.txt`; Maven POM 4.0.0
  incl. parent `relativePath`, property interpolation and `dependencyManagement`; Go `go.mod`; Gradle
  settings and build scripts' dependency notation, the version catalog `gradle/libs.versions.toml`,
  `gradle.lockfile`; `Cargo.lock`, `package-lock.json` v1–v3, `poetry.lock`, `uv.lock`), SPEC-0001 (workspace files), SPEC-0047 (run configurations)
- **Feature matrix rows:** F-003

## Purpose
Understand what a project is made of — modules, their dependencies and required toolchain versions —
straight from the files the build tools use, and offer matching run configurations.

## Behaviour
1. Build files anywhere in the workspace (excluded folders as in SPEC-0001) are found: `Cargo.toml`,
   `package.json`, `pyproject.toml`, `requirements.txt` (when no `pyproject.toml` is next to it),
   `pom.xml`, `go.mod`, `settings.gradle(.kts)`/`build.gradle(.kts)`. Each becomes a module with kind,
   name, version, folder (workspace-relative), toolchain requirement, dependencies and tasks. [STD]
2. Dependencies carry name, version requirement (or `path`/`git`/`workspace` origin) and scope (`normal`,
   `dev`, `build`, `test`, `provided`, `peer`, `optional`, or an optional group's name). [STD]
3. Cargo: `[package]` fields (also inherited with `.workspace = true` from the workspace root's
   `[workspace.package]`), `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` (inherited
   versions resolved from `[workspace.dependencies]`), `rust-version`; a workspace root lists its members
   (globs such as `crates/*` expanded). npm: name, version, `engines.node`, the four dependency maps,
   `scripts` as tasks, `workspaces`. Python: PEP 621 `[project]` (name, version, `requires-python`,
   `dependencies`, `optional-dependencies` groups) or `[tool.poetry]`; PEP 508 strings split into name and
   requirement (extras and markers kept in the requirement). Maven: groupId (inherited from the parent),
   artifactId, version, packaging, `maven.compiler.release`/`source` (or Spring Boot's `java.version`), `modules`,
   dependencies with scope. `${…}` properties are interpolated (own `<properties>`, `project.version`,
   `project.groupId`, `project.parent.version`); a parent found on disk (`relativePath`, default
   `../pom.xml`, coordinates checked, up to five levels) contributes properties and
   `dependencyManagement` versions and scopes. A parent that is only in a repository is named in a note
   when versions stay `managed`.
   Go: module path, `go` version, `require` lines and blocks (indirect marked as scope `indirect`). [STD]
4. Gradle build scripts are programs; they are read statically: `rootProject.name` and included projects
   from the settings file, the Java toolchain, and dependency declarations with a literal coordinate
   (`implementation("g:a:v")`, `implementation 'g:a:v'`, also inside `platform(...)`), a version
   catalog alias (`libs.x.y` from `gradle/libs.versions.toml`, with `version.ref` and rich versions) or a
   project (`project(":core")`). Configurations map to scopes (implementation/api → normal, compileOnly →
   provided, runtimeOnly → runtime, test* → test, annotationProcessor/kapt/ksp → build). Declarations
   that need evaluation are counted in a note. [STD, honest limitation]
5. A malformed build file yields a problem for that file — for a POM with line and column (`core/pom.xml:
   line 20, column 3: …`); the other modules are still read. [GEN]
6. Suggested run configurations follow from the modules: Cargo `cargo build` and `cargo test` (test with
   libtest report), npm `npm run <script>` per script, Go `go test ./...`, Maven `mvn test`, Python
   `python -m pytest` when pytest is a dependency. They use the SPEC-0047 format and are not written
   automatically. [GEN]
6a. **Framework applications (v8):** the pre-check of F-040 found no way to start a Spring Boot,
   Quarkus or Micronaut application other than the `main` marker's `exec:java`; IDEs with
   framework support offer one, and Quarkus's dev mode is the framework's own way to run while
   developing. A Maven or Gradle module whose dependencies or Maven plugins name a framework gets
   one more suggestion after `mvn test`/`gradle test`: [GEN; DOC (the frameworks' Maven and Gradle
   plugin guides, read for the goal and task names); OBS-0073 part 1]
   - **Quarkus** (group `io.quarkus` or `io.quarkus.platform`, or the `quarkus-maven-plugin`):
     "Quarkus dev", `mvn quarkus:dev` or `<gradle> quarkusDev`;
   - else **Micronaut** (group `io.micronaut`, or the `micronaut-maven-plugin`): "Micronaut run",
     `mvn mn:run` or `<gradle> run`;
   - else **Spring Boot** (group `org.springframework.boot`, or the `spring-boot-maven-plugin`):
     "Spring Boot", `mvn spring-boot:run` or `<gradle> bootRun`.
   Quarkus is checked first because a Quarkus application may use Spring's compatibility
   libraries. `<gradle>` is the runner of the module's other Gradle suggestions (the wrapper when
   there is one). The application runs until stopped: its timeout is a day (`SERVER_TIMEOUT_SECS`,
   86 400 s) instead of the ten minutes of SPEC-0047. Like the others it is listed in the run
   widget and returned by `project_model`; nothing is written to `run.toml`. A module of only
   Jakarta EE gets none: an application server is not started by its build. Nothing checks that
   the build file declares the plugin the goal needs; a Maven project that inherits it from
   `spring-boot-starter-parent` finds it, one with a Quarkus BOM but without the plugin does not.
7. MCP tool `project_model` returning modules, problems and suggested run configurations. [ADR-0008]
8. Lock files give each dependency its locked version (`resolved`): `Cargo.lock` (nearest up to the
   workspace root), `package-lock.json` (top-level installs; v1 `dependencies` too), `poetry.lock` or
   `uv.lock` (names normalised), `gradle.lockfile`. Several locked versions are listed together; a broken
   lock file is a problem, the module is still read. [STD]

9. **(v3) Maven details:** a POM's profiles (id, active by default), its build plugins
   (`group:artifact[:version]`, the group defaulting to `org.apache.maven.plugins`), its
   repositories (id and URL) and its `sourceDirectory`/`testSourceDirectory` are part of the module;
   `project_model` returns profiles and plugins. A profile's own settings do not change the POM's
   values. [STD]
10. **(v3) Folder roles in the project tree:** the folders of Maven and Gradle modules are shown with
    their role when the project folder has a POM or a Gradle build file: a module folder bold with the
    module icon and, when the module's name differs from the folder's, the name in brackets
    (`maven [shop-parent]`, `core [shop-core]`); `src/{main,test}/{java,kotlin,groovy,scala}` as
    source and test folders, `src/{main,test}/resources` as resource and test resource folders (or the
    POM's source folders), Maven's `target` and Gradle's `build` and `.gradle` as excluded output —
    each with its own icon, test folders on a green and excluded folders on an orange tint. The POM
    has a build file icon, `gradlew`/`mvnw` the shell icon. [GEN, OBS-0024]
    - **10a (v7) Build files in sub-folders:** when the project folder has no POM or Gradle build
      file but a sub-folder of the project's own sources has one (`app/pom.xml`,
      `mobile/android/settings.gradle.kts`), that module's folders get the same roles, and its
      packages are compacted (SPEC-0275 v11 behaviour 28); before v7 such a module had none. They
      are read off the UI thread together with the source folders of behaviour 14, in the same walk
      of the project's own files (`OwnSources`: never installed dependencies such as a POM in
      `node_modules`, build output or what Git ignores), and only the Maven and Gradle build files
      found there and the ones linked by hand are read — statically, without running Maven or
      Gradle. The tree shows these roles once that reading is done, like the derived ones; a build
      file in the project folder itself keeps the roles of behaviour 10, read at once. Measured on
      the build server: reading the model adds 4 to 13 ms to a walk of 95 to 180 ms (this repository with
      eight nested build files; a project of 20 000 files with one nested module). [GEN; decision
      of this version]
11. **(v3) External Libraries:** the node lists each module's toolchain first — a Java release with
    the folder of an installed JDK of that release (`< 17 > ~/Library/Java/…`) — then the libraries:
    for a Maven or Gradle module the whole tree the build tool resolved (SPEC-0149), once each, with
    the version Gradle chose, without the project's own modules and without artifacts that were not
    found; before a resolution the declared dependencies without sibling modules. [GEN, OBS-0024]
12. **(v4) Linked and unlinked build files:** `.idecraft/build-tools.toml` lists build files linked by
    hand (`linked`, workspace-relative) and module folders unlinked by hand (`unlinked`). A linked file
    is read like a found one, also in a folder the workspace leaves out (ignored or excluded); a
    module whose folder is unlinked, and every module inside that folder, is left out of the model,
    with its problems. Linking a file inside an unlinked folder removes that folder from `unlinked`.
    Files outside the workspace cannot be linked. (v5) The project tree shows the folder roles of
    the new model at once. [GEN, OBS-0024]
13. **(v5) No foreign project files:** reading the project never writes into it; the Java language
    server is told to keep Eclipse's `.project`, `.classpath` and `.settings/` in its own data
    folder (the system property `java.import.generatesMetadataFilesAtProjectRoot=false` at its start,
    the same setting in its initialization options). [GEN, OBS-0024]

14. **(v6) Source folders without a build file:** in a project without any build file IDECraft knew
    no source folder, so its packages were not shown as packages; the reference knew the source
    folder there from its own project file (OBS-0031, finding T1). Where the project model has no
    Maven or Gradle module for a folder, a source folder is recognised from the files themselves.
    [OBS-0031 T1 for the gap; GEN: a Java file's folders below its source folder spell its package
    (STD: Java Language Specification on packages in a file system); decision of this version]
    - **The rule:** for a Java or Kotlin file that declares a package, the folder that is left when
      the package's names are taken off the end of the file's folder is a source folder:
      `src/main/java` for `src/main/java/lab/inventory/Inventory.java` with
      `package lab.inventory;`. A file without a package declaration, and a file whose folder does
      not end in its package (Kotlin allows that), give nothing.
    - **Tests:** a derived folder at or below a folder `src/test` (`src/test/java`,
      `src/test/kotlin`) is a test folder, every other one a source folder. Resource folders are
      not derived.
    - **A real build file always wins:** nothing is derived in a folder with a `pom.xml`,
      `build.gradle(.kts)` or `settings.gradle(.kts)` or below one, and no derived folder holds
      such a folder; there the roles of behaviour 10 apply or none. A module folder unlinked by
      hand (behaviour 12) has no module, so its sources are derived.
    - **Never the project folder:** a file whose package leaves the project folder itself gives
      nothing — every folder of the project would become a package. Of two derived folders of
      which one holds the other, the outer one counts.
    - **Bounds:** only the project's own sources are looked at (SPEC-0148 v4 behaviour 7,
      `OwnSources`): no virtual environment, no installed dependencies, no build output, nothing
      behind a symbolic link, nothing Git ignores. Of each folder the first files by name are
      read, at most 3, until one gives a source folder; folders below a source folder already
      found are not read; in all at most 1 000 files are read, of each at most its first 64 KiB.
      What lies beyond these bounds gives no source folder.
    - **Kept and refreshed:** the window reads the derived folders in the background when it
      opens and keeps them; reading the tree again (after New, Paste, Delete, Reload) reuses
      them. They are read again, in the background, when the tree was read again, when a Java or
      Kotlin file was saved in the editor, when the files Git reports as changed differ from the
      last look, and when the watcher of a running language server (SPEC-0042 v11 behaviour 20)
      reports a created, changed or deleted Java, Kotlin or build file — each at the window's next
      look at the disk, every 2 seconds. Linking or unlinking a build file reads them at once.
    - **Where a derived folder counts like a declared one:** its icon in the project tree
      (behaviour 10; a test folder with its tint), compacted packages below it (SPEC-0275
      behaviour 30), the `role` of its row in the MCP tool `project_tree`, and the package of a
      new or moved Java or Kotlin file (SPEC-0275 behaviours 3 and 26).
    - **Where it deliberately does not:** the Java language server keeps its own rule for a
      project without a build file (SPEC-0042 v8 behaviour 17: the Maven convention folders, else
      the same package rule, Java files only, decided once for the whole project by build files
      in the project folder) — its workspace data is tied to those roots, and a change there
      empties it. The debugger's source lookup uses that rule too. The model the MCP tool
      `project_model` returns lists modules of build files only; derived folders are no modules.
      A remote project's tree has no roles (SPEC-0275 behaviour 30).

## Acceptance criteria
- **AK-1** Cargo: this repository's own workspace — members from `crates/*`, inherited version and
  `rust-version`, workspace-inherited dependency versions, dev-dependencies.
- **AK-2** npm, Python (PEP 621 and Poetry, requirements.txt), Maven (parent groupId, modules, scopes),
  Go (block and single-line require, indirect) from sample files in each tool's documented format.
- **AK-3** Gradle detection with names and includes; a malformed file is a problem while others load.
- **AK-4** Suggested run configurations per kind validate with SPEC-0047.
- **AK-5** The MCP tool returns modules, problems and suggestions.
- **AK-6** Maven: parent on disk with properties and dependency management (versions and scopes),
  `${project.*}` interpolation, unknown managed versions; repository-only parent named in a note.
- **AK-7** Gradle: Kotlin and Groovy scripts, catalog aliases with `version.ref` and `strictly`,
  `platform(...)`, project dependencies, scopes, unread declarations counted, locked versions from
  `gradle.lockfile`.
- **AK-8** Lock files: this repository's `Cargo.lock`; `package-lock.json` v3 with scoped and nested
  packages; `poetry.lock` with normalised names; a broken lock file as problem.

- **AK-9** Profiles, plugins and repositories of POMs (`buildtools::tests::maven_lifecycle_plugins_and_profiles`).
- **AK-10** Module folders with their names, source, test, resource and excluded folders, only those
  that exist (`buildtools::tests::folder_roles_of_maven_and_gradle_modules`); every role has an icon,
  test and excluded rows a tint (`project_tree_style::tests`).
- **AK-11** Libraries from the resolved tree without the sibling module and the missing artifact; the
  JDK's folder (`buildtools::tests::libraries_from_the_resolved_tree`).
- **AK-12** (v4) A linked POM in an ignored folder becomes a module; an unlinked folder drops its module
  and those inside it; linking again brings them back (`buildtools::tests::linked_and_unlinked_build_files`).
- **AK-13** (v5) jdtls starts with the system property and gets the setting, in safe mode and without
  it (`servers::tests`, `servers_safe_tests`); with real jdtls a Maven project keeps no Eclipse
  files after `file_diagnostics` (OBS-0024 re-check 2).

- **AK-14** (v6) Behaviour 14: the package of a Java and a Kotlin source after comments and
  annotations; source and test folders from the declared packages, nothing from a file without a
  package, with another package or in the project folder itself; nothing in and around folders
  with a build file, again when the folder is unlinked; nothing from ignored files, installed
  dependencies, virtual environments and build output; the bounds of 3 files of a folder and
  1 000 files in all (`source_roots::tests::{package_declarations,
  roots_from_the_declared_packages, a_build_file_speaks_for_its_folder,
  only_own_sources_and_a_bounded_number_of_files}`). In the tree: icon role, compacted rows and
  refresh (SPEC-0275 AK-28). This repository's own model is unchanged — derived folders are no
  modules (`crates/core/tests/it/project.rs`: `this_repository_as_cargo_workspace`).
- **AK-15** (v7) Behaviour 10a: in a project without a build file in its folder, a Maven module in
  `app` (named `shop-app`) and a Gradle module in `lib` get the module, source, test, resource and
  excluded roles, a Kotlin source folder outside them is still derived, and a POM below
  `node_modules` gives nothing (`source_roots::tests::modules_in_sub_folders_get_their_roles`;
  `a_build_file_speaks_for_its_folder` with the module roles). In the tree: SPEC-0275 AK-29.
- **AK-16** (v8) Behaviour 6a: the sample projects of F-040 — `spring-shop` (Spring Boot parent,
  no plugin declared) "Spring Boot" `mvn spring-boot:run`, `quarkus-greeting` "Quarkus dev"
  `mvn quarkus:dev`, `micronaut-notes` "Micronaut run" `gradle run`, `jakarta-library` none — and
  a Gradle Spring Boot module (`bootRun`), a Maven module with only the Micronaut plugin
  (`mn:run`) and a plain one (none), each with the timeout of a day
  (`crates/core/tests/it/project.rs`: `framework_applications_get_a_start_configuration`); MCP
  `project_model` lists "Quarkus dev" (`crates/mcp/tests/it/protocol.rs`:
  `configuration_keys_complete_and_quarkus_starts_in_dev_mode`).

## Out of scope
(v8) Debugging the application started by behaviour 6a (the `main` marker's configuration debugs),
the active profile given to it, an application server for Jakarta EE.

Resolving transitive dependencies, downloading dependencies or parents from repositories, evaluating Gradle
scripts or Maven profiles, `go.sum`, SDK management, writing build files.

## Known deviations from the reference
The reference imports Maven and Gradle through their tooling; IDECraft reads files statically and says
where that is not enough (Gradle). Because the files are read at once, the modules and their folder
roles are there when the window opens; the reference first showed the modules without them and asked
a second time before it marked the folders (OBS-0024, IMP-0334). Library contents (the JDK's modules,
the classes of a jar) cannot be expanded; package folders are not joined (`lab.shop.core`).
