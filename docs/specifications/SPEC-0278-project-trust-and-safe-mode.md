# SPEC-0278: Project trust and safe mode

- **Version:** 4 · **Date:** 2026-11-19 (v4: after *Don't Open* in the only project window the Welcome window comes, as after the last Close Project, behaviour 2, AK-9; OBS-0102 W1); 2026-11-14 (v3: Trusted Locations under Appearance & Behavior, SPEC-0337 behaviour 1, OBS-0048 S2; v1 2026-11-04; v2: in safe mode agents use no database connection of the project, and revoking trust through MCP needs the right `trust` — security review 2026-11-14, SPEC-0012 v3, SPEC-0053 v8)
- **Sources:** PO decision 2026-11-04 (PO-STATUS question 20: always ask, as the reference does);
  OBS-0003 (the reference asks on opening, also for a folder without build scripts; a safe-mode
  choice exists; the decision was lost after `SIGTERM`; the question was not reachable through the
  accessibility interface), OBS-0006 (question answered with trust before a Maven import); DOC
  (vendor help on project security: trusted locations listed and removable in the settings, an
  option to trust every project below a parent folder, a banner in safe mode with a way to trust
  the project); OSS (rust-analyzer configuration `cargo.buildScripts.enable`, `procMacro.enable`,
  `checkOnSave`; Eclipse JDT LS settings `java.import.gradle.enabled`, `java.import.maven.enabled`;
  typescript-language-server option `tsserver.path`); GEN (which build files run code: Cargo build
  scripts and procedural macros, Gradle and Maven builds, Swift package manifests, Bundler's
  Gemfile, `.Rprofile`, npm packages); SPEC-0042, SPEC-0053, SPEC-0087, SPEC-0090, SPEC-0094,
  SPEC-0105, SPEC-0149, SPEC-0262
- **Feature matrix rows:** F-001, F-077, F-084

## Purpose
Opening a project somebody else wrote must not run that project's code until the user says so.
Ferris IDEA asks once per project whether to trust it, explains what in the project would run, and
until then opens it in safe mode: files open and can be edited, nothing of the project runs.

## Behaviour

### The question
1. **Asked on opening.** When a project window opens on a folder that is not trusted (behaviour
   6), Ferris IDEA asks whether to trust it, as the reference does — also for a folder without build
   files. [PO 2026-11-04, OBS-0003]
2. **Answers.** *Preview in Safe Mode* (the default answer, Return), *Trust Project*, *Trust
   Projects in "<parent>"* (trusts the parent folder, so every project below it, behaviour 6) and
   *Don't Open* (Escape: closes the window; the project was not opened, so it does not move to
   the front of the recent projects). When it was the only project window, the Welcome window
   follows, as after Close Project of the last window (SPEC-0094 v10 behaviour 9, SPEC-0355
   behaviour 2); until v4 Ferris IDEA quit, which left the user without a window. [DOC, IMP-0006;
   v4: OBS-0102 W1]
3. **Why it asks.** The question names what was found and what each would run, e.g. `Cargo.toml`
   (build scripts and procedural macros of the package and its dependencies), `build.rs`,
   `build.gradle.kts` (Gradle build logic on sync), `gradlew`/`mvnw` (downloads and runs a build
   tool), `pom.xml` (Maven plugins), `Package.swift` (the manifest is compiled and run),
   `Gemfile` (Ruby code run by Bundler), `.Rprofile`, `package.json` scripts, `node_modules`
   (ESLint, Prettier, TypeScript of the project), `.venv` (the project's Python interpreter),
   run configurations (`.ferris-idea/run.toml`, `.run/`, `.idea/runConfigurations`). With nothing
   found it says so and that trusting allows running configurations, tests and tasks. [GEN,
   IMP-0006]
4. **Native and accessible.** The question is a native alert of the platform, so screen readers and
   automation can answer it (unlike drawn dialogs, IMP-0005). [GEN]

### Safe mode
5. **Nothing of the project runs** in an untrusted project:
   - language servers start without build execution where the server supports it, and do not start
     where it does not (behaviour 9);
   - no build-tool sync: Maven and Gradle dependency resolution (SPEC-0149) is refused;
   - no run configurations, tests, tasks, profiling or debugging (SPEC-0087), no ESLint or Prettier
     from the project's `node_modules`, no package installation into the project's environment;
   - opening, editing, searching, Git, the terminal and the database tools work as in a trusted
     project for the person at the window. [PO 2026-11-04, DOC]
   - v2: agents (MCP clients) use no database connection or store of the project: a connection
     file is a file of the project, and its host, database file, keychain entry, tunnel and
     startup script are whatever the repository says (SPEC-0012 v3 behaviour 12). [rule 12]
   A refused action says why and how to trust: `Safe mode: <root> is not trusted, so Ferris IDEA
   runs none of its code. Trust it with "Trust Project…" in the banner …`. [IMP-0006]
6. **Remembered per path.** Trusted locations are kept in `trusted-projects.toml` in the user's
   settings folder (SPEC-0053), never in the project, written atomically when they change
   (SPEC-0262). Each entry is a path and whether it was trusted as a project or as a folder; a
   path is trusted when it equals an entry or lies below one (paths compared after resolving
   links). A trust file inside the checked project is ignored. [DOC, IMP-0004, IMP-0028]
7. **Banner.** A window in safe mode shows a banner above the editors: that the project is in safe
   mode, what it would run (behaviour 3) and a *Trust Project…* button, which asks behaviour 2's
   question again without *Don't Open*. Trusting removes the banner at once and restarts the
   project's language servers with full options. [DOC]
8. **Managing trust.** Settings › Appearance & Behavior › Trusted Locations (v3; until v2 under
   Build, Execution, Deployment) lists the
   entries (path, project or folder) and whether the current project is trusted and through which
   entry; Delete removes the selected entry. Windows check their trust again when they become
   active, so a removed entry puts its projects back into safe mode. [DOC]
9. **Language servers in safe mode** (SPEC-0042):
   - rust-analyzer: build scripts, procedural macros and check on save off (initialization
     options and `workspace/configuration`);
   - jdtls: Gradle and Maven import off, build configuration updates disabled;
   - typescript-language-server: uses the TypeScript installed with the server (`tsserver.path`),
     never the project's; without one it is not started in safe mode;
   - Angular language server: probes only its own installation, not the project;
   - pyright, basedpyright: not given the project's interpreter;
   - Ruby: never started through `bundle exec`; ruby-lsp not started (it runs Bundler);
   - not started: kotlin-language-server (runs Gradle or Maven), metals (imports sbt builds),
     sourcekit-lsp (runs `Package.swift`), lua-language-server (runs workspace plugins), the R
     language server (reads `.Rprofile`);
   - all others start as usual (they read files, they run nothing of the project).
   A server not started says so: `<program> is not started in safe mode: it would run project
   code …`. The MCP tool `language_servers` reports `safe_mode` per server. [OSS, GEN]

### Without a window
10. **Non-interactive trust.** Headless and test runs never ask:
    - `FERRIS_IDEA_TRUST_PROJECTS=all` trusts every project for the process; a list of paths
      (separated as in `PATH`) trusts those paths and everything below;
    - `ferris-idea ui <folder> --trust` and `ferris-idea mcp <folder> --trust` trust that folder for the
      process without remembering it;
    - `ferris-idea trust status <folder>`, `ferris-idea trust add <folder> [--folder]`,
      `ferris-idea trust list`, `ferris-idea trust remove <path>` read and change the remembered entries;
    - the window self-test trusts its own fixture folders for the process. [PO 2026-11-04]
11. **Headless MCP is gated too.** `ferris-idea mcp` applies safe mode to untrusted projects like the
    window; the library used by tests applies it only when the program switches it on, so unit
    tests see trusted projects. [GEN]

### MCP (ADR-0008)
12. **`project_trust`** (read): whether the workspace (or a given path) is trusted and through what
    (entry, environment, flag), whether safe mode is enforced in this process, what was found
    (behaviour 3), what safe mode blocks, the remembered entries and the file. [ADR-0008]
13. **`trust_project`** is a guarded action: `dry_run` defaults to true and only reports what would
    be trusted and what would then run; really trusting needs the agent right `trust` for the
    workspace in the user's grant file (SPEC-0053), which only the user can add. Without it the
    refusal names the exact entry to add. Every trust change through MCP is logged in
    `.ferris-idea/agent-log.jsonl`. `scope` is `project` (default) or `folder` (the parent folder of
    the path). After trusting, the project's language servers are restarted. [IMP-0028]
14. **`revoke_project_trust`** removes the entry for a path and says when the path stays trusted
    through a parent folder entry. v2: it needs the right `trust` like `trust_project`, because it
    changes the user's trust file (SPEC-0053 v8 behaviour 13; v1 asked for no right). [GEN, rule 12]

## Acceptance criteria
- **AK-1** Store: adding a project, adding a folder, a path below a folder entry is trusted, a
  sibling is not, removing an entry, a trust file inside the project is ignored, links resolved
  (`trust::tests`).
- **AK-2** Process overrides: `FERRIS_IDEA_TRUST_PROJECTS=all`, a path list, `--trust`; not enforced →
  everything trusted (`trust::tests`).
- **AK-3** Findings list the build files of behaviour 3 with what they run; an empty folder has
  none (`trust::tests`).
- **AK-4** Safe mode refuses run configurations, streamed runs and Maven/Gradle resolution with the
  safe-mode message; trusted projects run (`crates/core/tests/trust.rs`).
- **AK-5** Language servers in safe mode: rust-analyzer and jdtls options, Angular probes, Ruby
  without `bundle exec`, servers not started in safe mode skipped with the reason
  (`servers::tests`).
- **AK-6** MCP: `project_trust` reports state and findings; `trust_project` dry run without right,
  refusal without `trust` right, trust with it (entry written, log line); `revoke_project_trust`;
  `run_configuration` refused in an untrusted workspace with enforcement on
  (`crates/mcp/tests/it/protocol.rs`).
- **AK-7** Settings page model: Trusted Locations page in its group (v3: Appearance & Behavior), found by search
  (`settings_model::tests`); question text and answers (`trust_model::tests`).
- **AK-8** Hand test (round 5, no. 197): question with findings on first open, safe mode with
  banner and refused run, trust from the banner, entry in Settings, removal, second open asks no
  more.
- **AK-9 (v4, behaviour 2)** Unit test `trust_model::tests::dont_open_in_the_only_window_shows_the_welcome_window`:
  Don't Open in the only project window shows the Welcome window, with others open only that
  window goes, without a Welcome window (harness) the app quits. In the window (macOS, window
  check): start with an untrusted project, Don't Open or Escape: the Welcome window appears.

## Out of scope
Git hooks (SPEC-0230) run in trusted and untrusted projects alike: a clone brings no hooks, and the
hooks folder is not part of the project. Plugins are trusted by the user, not by the project
(SPEC-0126). The terminal is the user's own typing. Adding a trusted location by hand in Settings
(use the banner or `ferris-idea trust add`). Projects created by Ferris IDEA being trusted automatically
(Ferris IDEA has no project wizard yet).

## Known deviations from the reference
- The question explains concretely what would run (behaviour 3) instead of a generic warning, and
  its default answer is safe mode, so pressing Return out of habit runs nothing (IMP-0006).
- The question is a native, accessible alert (IMP-0005).
- Trust survives any exit, because the trust file is written when it changes (IMP-0004).
- Agents can query trust, but trusting through MCP needs a right only the user can grant, and is
  logged (IMP-0028).
