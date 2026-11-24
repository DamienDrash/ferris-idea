# SPEC-0049: Project files of the reference IDE (read only)

- **Version:** 10 · **Date:** 2026-11-19 (v10: the note over a data source taken over asks for the user only where its database system logs in, the same in the window and over MCP, behaviour 19, AK-19 (AK-17 and AK-18 are taken by RC1 #1090 and CS2 #1097); gap W1 of OBS-0112; v8 is RC1 #1090, v9 is taken by #1097, the code style of `.ipr` projects); 2026-11-22 (v9, first written as v6 in #1097 and renumbered after #1086 took v6: the code style of an `.ipr` project is read from its `ProjectCodeStyleConfiguration` component and reported, not applied, since formatting reads the folder format only (gap CS3), behaviour 17, AK-18; gap CS2 of OBS-0109); 2026-11-19 (v8: agents start the translated run configurations through
  `run_configuration` and cannot change them, behaviours 13 and 16 with SPEC-0047 v7 behaviour 18; gap RC1 of
  OBS-0109; v6 is taken by #1086, code style, v7 by #1092, data sources); 2026-11-19 (v7: the database tool's shared `.idea/dataSources.xml` is read, its
  URLs cleaned of user info and password-like parameters, and its data sources offered, never connected or added
  without the user's or an agent's explicit action, behaviours 7, 18 and 19, AK-2, AK-15, AK-16; gap DS1 of
  OBS-0109, L3 of OBS-0085; v6 is #1086 on the code style); 2026-11-19 (v6: the project's code style is applied as SPEC-0150 v6
  behaviour 8 says, and `idea_project` tells whether it applies, behaviour 17; gap CS1 of OBS-0109); 2026-11-19 (v5: HTTP request configurations of the reference become HTTP request
  configurations of SPEC-0047 v6, behaviour 13, AK-14; gap O8 of F-084 in OBS-0071; no open pull request changed
  this spec when v5 was written); 2026-11-17 (v4: the project's code style in `.idea/codeStyles` is
  read and reported, not applied, behaviour 17, AK-13; F-004 pre-check OBS-0085); 2026-11-16 (v3: projects in the file-based `.ipr` format, run
  configurations from `.idea/runConfigurations`, the `RunManager` part of `workspace.xml` and the
  `.iws`, facets as framework hints, behaviours 7 and 12–16); 2026-11-15 (v2: the Project tree shows
  the modules, source, test, resource and excluded folders of `.idea`; MCP `idea_project` lists them
  as `folders`)
- **Sources:** OSS (the directory-based project format of the open platform `JetBrains/intellij-community`,
  Apache-2.0: `.idea/modules.xml` with `ProjectModuleManager`, module files `*.iml` with
  `NewModuleRootManager`, `misc.xml` with `ProjectRootManager`, `vcs.xml` with `VcsDirectoryMappings`,
  `.idea/libraries/*.xml` with `libraryTable`, the path macros `$PROJECT_DIR$` and `$MODULE_DIR$`;
  v3: the file-based `.ipr`/`.iws` format, `ProjectRunConfigurationManager` and `RunManager` with
  the `configuration` elements of the open run configuration types Application, JUnit, Gradle,
  Maven, Python and Shell Script, `FacetManager` in module files; v4: `ProjectCodeStyleConfiguration` in
  `.idea/codeStyles/codeStyleConfig.xml` and `Project.xml` with `code_scheme`, `codeStyleSettings`
  and `indentOptions` — format knowledge, no code), v5: assumption A1 of behaviour 13 (the HTTP Client's run
  configuration is not part of the open sources; type id as Ferris IDEA's reader listed it for the comparison project
  of OBS-0071, the keys as general knowledge of such files, to be confirmed by an observation of a file the
  reference writes into an own project, RESEARCH_BOUNDARIES §3), v7: OBS-0109 step 3 (the `.idea/dataSources.xml`
  the reference wrote for a SQLite file of our own project `sql-basic`: component, element and key names as listed in
  behaviour 18, assumption A2 on their form), STD (JDBC URL forms of the PostgreSQL, MySQL, MariaDB, SQLite and SQL
  Server drivers, RFC 3986 user info), OBS-F (files the reference IDE wrote into our own reference project
  `tests/reference-projects/sql-basic`), SPEC-0048 (project model)
- **Feature matrix rows:** F-004

## Purpose
Open a folder that was used with the reference IDE and understand its structure (modules, source and test
folders, exclusions, SDK, libraries, version control) without converting or changing anything.

## Behaviour
1. A workspace with a `.idea` folder is a reference project. The project name comes from `.idea/.name`,
   otherwise from the folder name. Nothing in `.idea` is written. [OSS]
2. Modules are read from `modules.xml` in file order (module file path, optional group); the module name is
   the module file name without `.iml`. `$PROJECT_DIR$` is the project folder; `$MODULE_DIR$` is the module
   file's folder, or the project folder when the module file lies directly in `.idea`. All paths are
   reported project-relative (`..` kept for folders above the project); other macros such as
   `$USER_HOME$` or `$MAVEN_REPOSITORY$` stay as written. [OSS, OBS-F]
3. Per module: type, content roots with source folders (kind `source`, `test`, `resource`,
   `test-resource`, optional package prefix) and excluded folders, language level if overridden, whether the
   compiler output is inherited, otherwise the own output and test output folders. [OSS]
4. Module SDK (`inherited` from the project or a named one) and dependencies in order: modules, libraries
   (with level `project` or `application`) and module libraries (with their class roots, named after the
   first root when unnamed), each with scope (`compile` default, `test`, `runtime`, `provided`) and the
   exported flag. [OSS]
5. Project SDK name and type, project language level and compiler output from `misc.xml`; project libraries
   with class and source roots from `.idea/libraries/*.xml`. [OSS]
6. Version control mappings from `vcs.xml` (folder, system; an empty folder means the project). [OSS]
7. Personal and tool state is never opened: `usage.statistics.xml`, `shelf`, and the
   database tool's files (`dataSources*`, `db-forest-config.xml`, `sqldialects.xml`; **v7:** except the shared
   `dataSources.xml`, behaviour 18 — `dataSources.local.xml` and the folder `dataSources/` stay unopened). Their presence is
   listed so the user sees what was left out. **v3:** `workspace.xml` (and the `.iws` of behaviour 12)
   is opened for its `RunManager` component only (behaviour 13); nothing else in it is used or
   reported, and it stays in the list of what was left out with that note. Files outside the
   project, such as the reference IDE's own settings folder with global SDKs and libraries, are
   never read. [GEN; RESEARCH_BOUNDARIES: no licence or tool internals]
8. Malformed files, module files missing on disk and dependencies on unknown modules or unknown project
   libraries are reported as problems naming the file; everything else is still read. [GEN, improvement
   IMP-0024]
9. MCP tool `idea_project` returning the project (or null without `.idea`). [ADR-0008]
10. **Project tree (v2):** the folders the modules name get their roles in the Project tree, as
    the folders of Maven and Gradle modules do (SPEC-0048 v3 behaviour 10): each content root is a
    module folder (the module's name in brackets when it differs from the folder's name), source
    folders are sources, tests, resources or test resources by their kind, excluded folders and the
    compiler output folders (the project's and a module's own) are excluded. Only folders below the
    project that exist are marked; a content root above the project and a macro that is not
    expanded mark nothing. Where a Maven or Gradle build file gives a folder a role, that role wins;
    a role from `.idea` wins over a source folder derived from the files (SPEC-0048 v6 behaviour
    14). The marked source folders count as source roots wherever the window uses them (SPEC-0332
    v2 behaviour 13a), and the excluded ones are left out of project-wide searches as before
    (SPEC-0148 v3 behaviour 7). The tree is read again with the project (Reload, Synchronize).
    Nothing in `.idea` is written. [OSS, GEN]
11. **MCP (v2):** `idea_project` adds `folders`: each marked folder (project-relative), its role
    (`module`, `sources`, `tests`, `resources`, `test_resources`, `excluded`) and the module name
    shown in brackets. [ADR-0008]

12. **v3 – The file-based format:** a project folder without `.idea` that holds a `<name>.ipr` is a
    reference project named `<name>`. The `.ipr` holds in one file the components the folder
    format spreads over several files: `ProjectModuleManager` (behaviour 2), `ProjectRootManager`
    (behaviour 5), `VcsDirectoryMappings` (behaviour 6), `libraryTable` with the project libraries
    (behaviour 5) and `ProjectRunConfigurationManager` (behaviour 13); `$PROJECT_DIR$` is the
    folder of the `.ipr`. Its `.iws` is the personal part, read like `workspace.xml`. With several
    `.ipr` files the first by name is read and the others are named as a problem; with `.idea`
    present the `.ipr` is ignored. Everything else (modules, Project tree, MCP) is as for `.idea`.
    [OSS]
13. **v3 – Run configurations:** the `configuration` elements of `.idea/runConfigurations/*.xml`
    (shared with the project), of `ProjectRunConfigurationManager` in the `.ipr` and of the
    `RunManager` component of `workspace.xml` or the `.iws` (personal) become Ferris IDEA run
    configurations of SPEC-0047, in that order, a name seen before skipped. Templates
    (`default="true"`) are left out; a configuration's folder (`folderName`) becomes its `folder`.
    They are listed in the window and to agents after those of `.ferris-idea/run.toml` and before
    the ones detected from build files (SPEC-0087 v2 behaviour 7), are not written into
    `run.toml`, and start only as every run configuration does: in a trusted project
    (SPEC-0047, safe mode), never while reading. Translation per type, with `$PROJECT_DIR$` and
    `$MODULE_DIR$` expanded, the working folder kept when it lies in the project, and the
    environment variables taken over (behaviour 16 for agents) [OSS]:
    - **Application:** the main class with program and VM parameters. In a module of a Maven
      build `mvn -q compile exec:java` with `-Dexec.mainClass` and `-Dexec.args`; in a module of
      `.idea` alone, its source folders are compiled with `javac` into its output folder and
      `java` runs the class with the output folders of the module and the modules it depends on
      and the class roots of its libraries that lie in the project on the class path.
    - **JUnit:** a class, a method or a package in a module of a Maven build (`mvn test
      -Dtest=…`) or a Gradle build (`gradle test --tests …`, `./gradlew` when the module has it),
      as a test configuration with JUnit reports (SPEC-0097).
    - **Gradle:** the task names and script parameters in the external project's folder, with
      `./gradlew` when that folder has it, otherwise `gradle`.
    - **Maven:** the goals and the profiles (`-P a,!b`) in the working folder.
    - **Python:** a script or, in module mode, `-m module`, with interpreter options and
      parameters, run by `python3` as SPEC-0050 resolves it.
    - **Shell Script:** a script file with its interpreter (`sh` when none is named), interpreter
      options and script options, or the script text run by `sh -c`.
    - **v5 – HTTP Request** (`HttpClient.HttpRequestRunConfigurationType`): a configuration of
      SPEC-0047 v6 behaviour 17 with `http`: `file` from `path` (expanded, inside the project,
      ending in `.http` or `.rest`), `environment` from `environment` when not empty, and
      `request` from `requestIdentifier` (the request's name). With `runType` naming all requests
      of the file there is no `request`. Without `requestIdentifier`, a file whose only request it
      can be gives no `request` either; otherwise, and for a position given by `index` alone, the
      configuration is not translated, since how the reference counts positions is not known. Each
      key is read from the `configuration` element's attribute or, failing that, from an `option`
      of that name. **Assumption A1:** this format is not in the open sources; type id as listed
      by Ferris IDEA's reader for a project of the reference (OBS-0071), keys and the wording of
      `runType` (`Run all requests in file`) from general knowledge of such files. An observation
      must confirm them; until then a configuration that does not fit is listed with its reason,
      never guessed. Nothing is sent while reading. [GEN, assumption A1]
    - Any other type (npm and the other types of commercial plugins, whose format the open
      sources do not document, remote and compound configurations) and a configuration that
      lacks what its type needs (no main class, a module without a build tool for JUnit, a
      working folder outside the project) is not translated: it is listed with the reason, and a
      problem names the file and the configuration. [GEN]
14. **v3 – Facets:** the `FacetManager` component of a module file lists the module's facets
    (type and name, e.g. Spring, Web, JPA, Python, Kotlin). They are kept per module as hints of
    the frameworks the module uses; Ferris IDEA's own framework support (SPEC-0115) still decides
    from the sources and build files, so a facet neither turns a feature on nor off. [OSS, GEN]
15. **v3 – Read again:** run configurations and facets are read again with the project (Reload,
    Synchronize, a change of `.idea`), like modules. [GEN]
16. **v3 – MCP:** `idea_project` adds `run_configurations` (name, type, source file, folder, and
    for a translated one kind, command, working folder and the *names* of its environment
    variables; for one not translated the reason) and per module `facets` (type, name).
    Environment values are never returned, since they may hold secrets; the window shows them in
    the run configuration as for any configuration. [ADR-0008, CLAUDE.md rule 12]
    **v8:** MCP `run_configurations` lists the same configurations in `idea_configs` with `source` and
    `read_only`, and `run_configuration` starts a translated one by name as the window does, under the same
    `run` right and only in a trusted project; `run_configuration_edit` refuses to change one, naming its
    source (SPEC-0047 v7 behaviour 18, AK-17). The reference's files stay read only for agents too.
17. **v4 – Code style:** in the folder format, `.idea/codeStyles/codeStyleConfig.xml` says whether
    the project uses its own scheme (`USE_PER_PROJECT_SETTINGS`) or prefers a named IDE scheme
    (`PREFERRED_PROJECT_CODE_STYLE`); `.idea/codeStyles/Project.xml` holds the project scheme
    (`code_scheme`). Ferris IDEA reads the scheme's `RIGHT_MARGIN` and, per `codeStyleSettings`
    element in file order, the language id as written, its `RIGHT_MARGIN` and from
    `indentOptions` `INDENT_SIZE`, `CONTINUATION_INDENT_SIZE`, `TAB_SIZE` and
    `USE_TAB_CHARACTER`. Only what the files write is reported; a missing or non-numeric value
    stays empty and no default of the reference is filled in. A named IDE scheme is reported by
    name only, since it lives in the reference IDE's settings folder (behaviour 7). A malformed
    file is a problem naming it (behaviour 8); without either file there is no code style. Until v5
    the code style was not applied; since v6, when `USE_PER_PROJECT_SETTINGS` is on, the
    scheme's indentation and right margin apply between `.editorconfig` and Ferris IDEA's Code Style,
    with the mapping of the language ids, as SPEC-0150 v6 behaviour 8 specifies. MCP `idea_project`
    adds `code_style` (`per_project`, `preferred_scheme`, `file`, `right_margin`, `languages`;
    v6: `applies`, true when the scheme is switched on and `Project.xml` was read). **v9:** an `.ipr` project keeps its
    code style in the `.ipr` itself, one `ProjectCodeStyleConfiguration` component holding both
    the switch (`state`) and the embedded scheme (`code_scheme`); Ferris IDEA reads it with the same
    values and rules, `file` names the `.ipr`; without that component there is no code style. It is
    reported only: `applies` is false and formatting keeps to `.editorconfig` and Ferris IDEA's Code
    Style (gap CS3). [OSS, ADR-0008, OBS-0109 CS2]

18. **v7 – Data sources (gap DS1 of OBS-0109):** in the folder format Ferris IDEA reads the shared
    `.idea/dataSources.xml`. The personal `dataSources.local.xml` and the folder `dataSources/` are never
    opened, not even to check them, and stay listed as not read (behaviour 7); a `dataSources.xml` that is a
    symbolic link is not followed and is a problem. The file holds the component `DataSourceManagerImpl` with
    one `data-source` per data source: its `name`, a fixed id (`uuid`), the driver's ref (`driver-ref`, e.g.
    `sqlite.xerial`), its JDBC class (`jdbc-driver`), the URL (`jdbc-url`, with `$PROJECT_DIR$` for the project
    folder), the working folder (`working-dir`) and automatic synchronising (`synchronize`); the reference writes
    no user and no password there. **Assumption A2:** `name` and `uuid` are attributes of `data-source`, the
    others child elements whose text is the value; every key is read from the attribute or, failing that, from
    the child element, so either form reads. For each data source in file order [OBS-0109, STD, GEN]:
    - **Kind:** the first part of the driver ref (up to the first `.`), case-insensitive: `sqlite` → `sqlite`,
      `postgresql` or `postgres` → `postgresql`, `mysql` → `mysql`, `mariadb` → `mariadb`, `sqlserver` or
      `mssql` → `sqlserver`. Without a driver ref the URL's scheme (`jdbc:<scheme>:`) decides with the same
      names. Any other driver gets the reason "driver `<ref>` is not supported"; a URL whose scheme belongs to
      another kind than the driver ref gets a reason too.
    - **Clean URL:** before the URL is shown, reported or kept anywhere, `$PROJECT_DIR$` is replaced by the
      project folder, the user info before the host (`user:password@`) is removed, and every parameter — in the
      query (`?a=b&c=d`) or as a property (`;a=b`) — whose name, in lower case and without `-`, `_` and `.`,
      contains `password`, `passwd`, `pwd`, `pass`, `token`, `secret`, `key`, `credential` or `auth` is removed
      with its value. `removed` names what was taken out (`user info` and the parameter names), never a value.
      The raw URL is not kept.
    - **Target:** SQLite: the path after `jdbc:sqlite:` as `file`, project-relative when it lies in the
      project (`..` resolved lexically). A file outside the project, a path with another macro (such as
      `$USER_HOME$`), an empty path and an in-memory database get a reason. Server kinds: `host`, `port` and
      `database` from `jdbc:<scheme>://host[:port][/database]`; for SQL Server
      `jdbc:sqlserver://host[:port]` with the property `databaseName` or `database`. Several hosts, a named SQL
      Server instance (`host\instance`), a port that is not a number and any other form get a reason.
    - A data source with a reason is listed with it and is not offered; one without is offered.
    Reading connects to nothing, creates no connection profile and writes nothing (behaviour 1). A malformed
    file is a problem naming it, the rest of the project is still read (behaviour 8). The `.ipr` format is not
    searched for data sources. MCP `idea_project` adds `data_sources` with `name`, `driver` (the ref as
    written), `kind`, `url` (clean), `file` or `host`/`port`/`database`, `removed` and, for one not offered,
    `reason`. [ADR-0008, CLAUDE.md rule 12]
19. **v7 – Taking one over:** a data source becomes a connection of the workspace only by an explicit action:
    in the Database tool window (SPEC-0089 v16 behaviour 28) or by the MCP tool `add_idea_data_source` with
    the data source's `name`. The tool answers what it would add as a dry run unless `dry_run` is false; a
    real call needs the user-granted `write` right, as `save_connection` does, and adds the profile of
    SPEC-0008 v9 behaviour 19. An unknown name, a data source with a reason (the reason is named) and a name
    or target the workspace's connections already have are refused. Nothing connects. [ADR-0008, SPEC-0053]
    **v10:** the window's connection dialog, filled from a data source, and the tool's answer (`note`)
    say the same: for a database system that logs in (every one but a file such as SQLite) "From the
    project's .idea: check the fields, add the user, then Save", for a file "From the project's .idea:
    check the fields, then Save" — a file has no user to add. [OBS-0112 W1]

## Acceptance criteria
- **AK-1** A multi-module sample in the documented format: project SDK, language level and output; modules
  with group, content roots, all four source-folder kinds, package prefix, exclusions, inherited and own
  output, inherited and named SDK; module, project-library and module-library dependencies with scope and
  exported flag; project libraries with class and source roots and an unexpanded macro; version control
  mappings.
- **AK-2** Missing module file, unknown module and project library, malformed module file are problems
  while the rest is read; `workspace.xml` and `dataSources.xml` are not opened (malformed on purpose) and
  are listed. **v7:** `dataSources.local.xml` takes the place of `dataSources.xml` here, which is read since
  v7 (AK-15).
- **AK-3** The `.idea` written by the reference IDE for `tests/reference-projects/sql-basic` reads without
  problems (module in `.idea` with content root `.`, VCS mapping above the project). The folder is not
  committed, so the test runs only where it exists.
- **AK-4** The MCP tool returns null without `.idea` and the module, content root, SDK and problems with it.

- **AK-5 (v2)** Roles from the model (`crates/core/tests/it/ideaproject.rs`:
  `folder_roles_of_the_modules`): module folders with and without a differing name, the four
  source kinds, excluded folders, inherited and own output, project output; a content root above
  the project, a missing folder and an unexpanded macro mark nothing.
- **AK-6 (v2)** In a window of the test harness with real keys
  (`crates/workbench/tests/it/idea_project_tree.rs`: `idea_modules_mark_the_project_tree`): a folder
  with `.idea`, two modules and no build file opens with the module, source, test, resource and
  excluded folders marked; ↓/→ in the tree reach the rows; a Maven `pom.xml` in one module wins for
  its folders; after the module file changes on disk, Reload marks the new source folder.
- **AK-7 (v2)** MCP (`crates/mcp/tests/it/protocol.rs`: `idea_project_lists_the_folders`): the
  `folders` of the sample with their roles and module names.

- **AK-8 (v3, behaviour 12)** The fixture `tests/idea-projects/ipr-basic` (an `.ipr` with two
  modules, a project library, a VCS mapping and a shared run configuration, its `.iws` with a
  personal one) reads like the same project in the folder format; a second `.ipr` is a problem;
  a folder with `.idea` and an `.ipr` reads `.idea`.
- **AK-9 (v3, behaviour 13)** The fixture `tests/idea-projects/run-configs`: Application in a
  Maven module and in a plain `.idea` module (class path from outputs and libraries), JUnit class,
  method and package for Maven and Gradle, Gradle tasks with `./gradlew`, Maven goals with
  profiles, Python script and module mode, Shell script file and text, folders, environment, the
  order and the skipping of a repeated name; npm, a compound configuration, JUnit in a module
  without build tool and a working folder outside the project are listed with their reason and
  as problems; templates are left out.
- **AK-10 (v3, behaviours 7, 13)** Only `RunManager` of `workspace.xml` is used: another
  component with a configuration in it adds nothing, and `workspace.xml` stays listed as not
  read; a malformed `workspace.xml` is a problem naming it while the rest is read.
- **AK-11 (v3, behaviour 13)** The window's and the host's list of run configurations hold the
  translated ones after `run.toml` and before the detected ones; nothing is written into
  `run.toml`; in an untrusted project starting one is refused as for every configuration.
- **AK-12 (v3, behaviours 14, 16)** Facets per module; MCP `idea_project` lists
  `run_configurations` with the environment's names and no value, and `facets`.
- **AK-13 (v4, behaviour 17)** `crates/core/tests/it/ideaproject.rs`: `code_style_of_the_project`:
  a project scheme with a right margin and two languages (indent, continuation, tab size, tabs;
  a non-numeric value stays empty); a preferred IDE scheme by name only; a malformed scheme file
  is a problem naming it; without `.idea/codeStyles` there is no code style.
  v6: `applies` is true only with `USE_PER_PROJECT_SETTINGS` and a scheme file; applying the
  scheme is SPEC-0150 v6 AK-8 to AK-10.
- **AK-14 (v5, behaviour 13)** The fixture `tests/idea-projects/http-configs` with HTTP request
  configurations (`crates/core/tests/it/ideaproject.rs`: `http_request_configurations_are_translated`):
  one request by name with an environment in `workspace.xml`, all requests of a file and the only request of
  a file without a name in `.idea/runConfigurations`, each with `http` and no command; a position alone, a
  missing name of a file with several requests and a file outside the project are listed with their reason
  and as problems; the translated ones load as `run.toml` configurations without problems; MCP
  `run_configurations` lists the `http` table of a translated one (`crates/mcp/tests/it/protocol.rs`:
  `an_http_run_configuration_needs_the_http_right`).
- **AK-18 (v9, behaviour 17)** `crates/core/tests/it/ideaproject.rs`:
  `code_style_of_an_ipr_project`: an `.ipr` whose `ProjectCodeStyleConfiguration` holds the switch
  and a scheme (right margin 100, Java indent 2) reports them as the folder format does, with
  `file` naming the `.ipr`; the fixture `ipr-basic` without the component has no code style.

- **AK-15 (v7, behaviour 18)** `crates/core/tests/it/ideaproject.rs`: `data_sources_of_the_project`: a
  `dataSources.xml` with a SQLite file under `$PROJECT_DIR$`, PostgreSQL with user info, a password and a
  token parameter, MySQL, MariaDB, SQL Server with a `;password=` property, an unknown driver, a SQLite file
  outside the project and a named SQL Server instance: kinds, clean URLs without any secret, `removed` names,
  targets and reasons; `dataSources.local.xml` and `dataSources/` hold a marker that appears nowhere in the
  result and are listed as not read; a malformed `dataSources.xml` is a problem while the modules are still
  read; a symbolic link is not followed. Unit tests `ideadatasources::tests` for cleaning, the macro and the
  mapping of driver refs.
- **AK-16 (v7, behaviour 19)** `crates/mcp/tests/it/protocol.rs`:
  `idea_data_sources_are_offered_and_added_only_on_request`: a temporary project with `.idea/dataSources.xml`
  for a SQLite file: `idea_project` lists it and writes no `connections.toml`; `add_idea_data_source` as dry
  run answers the profile and writes nothing; a real call without the `write` right is refused; with it the
  profile is written with the relative file and no password; adding it again, an unknown name and an
  unsupported driver are refused with their reason.
- **AK-19 (v10, behaviour 19)** The note asks for the user for a server's data source and not for a
  SQLite file's (`crates/database/src/idea_sources.rs`: `the_note_asks_for_a_user_only_where_one_logs_in`);
  MCP `add_idea_data_source` answers the same `note` (`crates/mcp/tests/it/protocol.rs`, the test of
  AK-16).

## Out of scope
Applying the code style of an `.ipr` project (v9 reads and reports it; gap CS3), code style options other than margin and indentation (v6
applies those two through SPEC-0150 v6 behaviour 8), inspection profiles (they are
SPEC-0315's), the database tool's personal and generated files (behaviour 7; v7 reads the shared data sources
only, behaviour 18), data sources of an `.ipr` project, drivers other than those of behaviour 18, artifacts, global (application-level) libraries and SDK
definitions outside the project, run configuration types whose format the open sources do not
document (npm and other commercial plugins; v5: except HTTP Request, behaviour 13, assumption A1), before-launch tasks of the reference's
configurations, writing or converting `.idea`.

## Known deviations from the reference
Broken references are listed together (IMP-0024); the reference's own handling was not observed.
**v3:** global SDKs and libraries are not resolved, since they live in the reference IDE's
settings folder, which Ferris IDEA never reads (only their names are shown). A run configuration
that cannot be translated is listed with its reason instead of being dropped silently. Before-launch
tasks (build, other configurations) of the reference's configurations are not taken over; the
translated commands compile themselves where they need to.
**v7:** data sources are offered, never connected or added on their own; their URLs lose user info and
password-like parameters before anything shows them, and a server profile taken over starts with Ferris IDEA's
secure SSL default, not the mode in the URL (IMP-0530). A SQLite file outside the project is not offered.
